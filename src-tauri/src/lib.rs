#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use herdr_workbench_domain::{PreviewSession, Workspace};
use herdr_workbench_server::serve;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

/// Windows Preview 的 Tauri/WebView2 实现。
///
/// Application Core 只依赖 `PreviewAdapter`；窗口创建、复用和聚焦都隐藏在这个 Adapter 内。
#[derive(Clone, Debug)]
pub struct TauriWebViewPreviewAdapter {
    app: AppHandle,
}

impl TauriWebViewPreviewAdapter {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn window_label(workspace: &Workspace) -> String {
        format!("preview-{}", workspace.workspace_id.as_uuid().simple())
    }

    fn target_url(url: Option<String>) -> Result<Url, herdr_workbench_app_core::PreviewError> {
        let value = url.unwrap_or_else(|| "about:blank".to_owned());
        let parsed = Url::parse(&value).map_err(|error| {
            herdr_workbench_app_core::PreviewError::unavailable(format!(
                "preview URL is invalid: {error}"
            ))
        })?;
        if (parsed.scheme() != "http" && parsed.scheme() != "https")
            && parsed.as_str() != "about:blank"
        {
            return Err(herdr_workbench_app_core::PreviewError::unavailable(
                "preview URL must use http or https (or be about:blank)",
            ));
        }
        Ok(parsed)
    }
}

#[async_trait::async_trait]
impl herdr_workbench_app_core::PreviewAdapter for TauriWebViewPreviewAdapter {
    async fn open(
        &self,
        workspace: &Workspace,
        url: Option<String>,
    ) -> Result<PreviewSession, herdr_workbench_app_core::PreviewError> {
        let target = Self::target_url(url.clone())?;
        let label = Self::window_label(workspace);

        if let Some(window) = self.app.get_webview_window(&label) {
            window
                .show()
                .and_then(|_| window.set_focus())
                .and_then(|_| window.navigate(target.clone()))
                .map_err(|error| {
                    herdr_workbench_app_core::PreviewError::unavailable(format!(
                        "failed to focus preview window: {error}"
                    ))
                })?;
        } else {
            WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External(target.clone()))
                .title(format!("Preview · {}", workspace.label))
                .inner_size(1280.0, 800.0)
                .center()
                .focused(true)
                .build()
                .map_err(|error| {
                    herdr_workbench_app_core::PreviewError::unavailable(format!(
                        "failed to create preview window: {error}"
                    ))
                })?;
        }

        Ok(PreviewSession::opening(workspace, Some(target.to_string())).mark_open())
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let preview_adapter = Arc::new(TauriWebViewPreviewAdapter::new(app.handle().clone()));
            let server_adapter = Arc::clone(&preview_adapter);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = serve(server_adapter).await {
                    eprintln!("Workbench API stopped: {error}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Herdr Workbench");
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_workbench_domain::HerdrWorkspaceContext;

    #[test]
    fn workspace_gets_a_stable_preview_window_label() {
        let workspace = Workspace::bind(
            HerdrWorkspaceContext::new(
                "herdr-1",
                "Test",
                std::path::PathBuf::from(r"C:\workspace"),
            )
            .unwrap(),
        );
        assert_eq!(
            TauriWebViewPreviewAdapter::window_label(&workspace),
            format!("preview-{}", workspace.workspace_id.as_uuid().simple())
        );
    }

    #[test]
    fn empty_url_becomes_about_blank() {
        let url = TauriWebViewPreviewAdapter::target_url(None).unwrap();
        assert_eq!(url.as_str(), "about:blank");
    }

    #[test]
    fn unsupported_url_scheme_is_rejected() {
        let result = TauriWebViewPreviewAdapter::target_url(Some("file:///tmp/index.html".into()));
        assert!(result.is_err());
    }
}
