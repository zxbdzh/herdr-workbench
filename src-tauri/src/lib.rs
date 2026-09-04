#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use herdr_workbench_app_core::PreviewStateUpdater;
use herdr_workbench_domain::{PreviewSession, PreviewStatus, WorkbenchWorkspaceId, Workspace};
use herdr_workbench_server::{connect_repository, serve_with_repository};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

/// Windows Preview 的 Tauri/WebView2 实现。
///
/// Application Core 只依赖 `PreviewAdapter`；窗口和页面事件都隐藏在这个 Adapter 内。
#[derive(Clone)]
pub struct TauriWebViewPreviewAdapter {
    app: AppHandle,
    state: Arc<dyn PreviewStateUpdater>,
}

impl TauriWebViewPreviewAdapter {
    pub fn new(app: AppHandle, state: Arc<dyn PreviewStateUpdater>) -> Self {
        Self { app, state }
    }

    fn window_label(workspace: &Workspace) -> String {
        format!("preview-{}", workspace.workspace_id.as_uuid().simple())
    }

    fn workspace_id_from_label(label: &str) -> Option<WorkbenchWorkspaceId> {
        let value = label.strip_prefix("preview-")?;
        uuid::Uuid::parse_str(value)
            .ok()
            .map(WorkbenchWorkspaceId::from_uuid)
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

    fn persist_page_state(
        state: &Arc<dyn PreviewStateUpdater>,
        label: &str,
        status: PreviewStatus,
        url: Option<String>,
        title: Option<String>,
    ) {
        let Some(workspace_id) = Self::workspace_id_from_label(label) else {
            eprintln!("ignoring page event for unknown Preview label: {label}");
            return;
        };
        let state = Arc::clone(state);
        tauri::async_runtime::spawn(async move {
            if let Err(error) = state
                .update_preview_state(&workspace_id, status, url, title)
                .await
            {
                eprintln!("failed to persist Preview page state: {error}");
            }
        });
    }
}

#[async_trait::async_trait]
impl herdr_workbench_app_core::PreviewAdapter for TauriWebViewPreviewAdapter {
    async fn open(
        &self,
        workspace: &Workspace,
        url: Option<String>,
    ) -> Result<PreviewSession, herdr_workbench_app_core::PreviewError> {
        let target = Self::target_url(url)?;
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
            let state_started = Arc::clone(&self.state);
            let state_title = Arc::clone(&self.state);
            WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External(target.clone()))
                .title(format!("Preview · {}", workspace.label))
                .inner_size(1280.0, 800.0)
                .center()
                .focused(true)
                .on_page_load(move |window, payload| {
                    let status = match payload.event() {
                        PageLoadEvent::Started => PreviewStatus::Opening,
                        PageLoadEvent::Finished => PreviewStatus::Open,
                    };
                    Self::persist_page_state(
                        &state_started,
                        window.label(),
                        status,
                        Some(payload.url().to_string()),
                        None,
                    );
                })
                .on_document_title_changed(move |window, title| {
                    Self::persist_page_state(
                        &state_title,
                        window.label(),
                        PreviewStatus::Open,
                        None,
                        Some(title),
                    );
                })
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
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match connect_repository().await {
                    Ok(repository) => {
                        let state: Arc<dyn PreviewStateUpdater> = repository.clone();
                        let preview_adapter =
                            Arc::new(TauriWebViewPreviewAdapter::new(handle, state));
                        if let Err(error) = serve_with_repository(repository, preview_adapter).await
                        {
                            eprintln!("Workbench API stopped: {error}");
                        }
                    }
                    Err(error) => eprintln!("Workbench API initialization failed: {error}"),
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
    fn label_round_trips_to_workspace_id() {
        let workspace = Workspace::bind(
            HerdrWorkspaceContext::new(
                "herdr-1",
                "Test",
                std::path::PathBuf::from(r"C:\workspace"),
            )
            .unwrap(),
        );
        let label = TauriWebViewPreviewAdapter::window_label(&workspace);
        assert_eq!(
            TauriWebViewPreviewAdapter::workspace_id_from_label(&label),
            Some(workspace.workspace_id)
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
