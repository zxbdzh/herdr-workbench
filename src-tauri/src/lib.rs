#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use herdr_workbench_domain::{PreviewSession, Workspace};
use herdr_workbench_server::serve;
use tauri::Manager;

/// Windows Preview 的稳定 seam。
///
/// 当前实现明确报告 capability unavailable；后续 WebView2 实现只需替换此 Adapter，
/// Application Core 和 REST 契约无需感知 Tauri 类型。
#[derive(Debug, Default)]
pub struct TauriWebViewPreviewAdapter;

#[async_trait::async_trait]
impl herdr_workbench_app_core::PreviewAdapter for TauriWebViewPreviewAdapter {
    async fn open(
        &self,
        _workspace: &Workspace,
        _url: Option<String>,
    ) -> Result<PreviewSession, herdr_workbench_app_core::PreviewError> {
        Err(herdr_workbench_app_core::PreviewError::unavailable(
            "Tauri WebView2 preview adapter is not implemented yet",
        ))
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let preview_adapter = Arc::new(TauriWebViewPreviewAdapter);
            let server_adapter = Arc::clone(&preview_adapter);
            app.manage(preview_adapter);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = serve(server_adapter).await {
                    eprintln!("Workbench API stopped: {error}");
                }
            });
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Herdr Workbench");
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_workbench_app_core::PreviewAdapter;

    #[tokio::test]
    async fn preview_adapter_reports_unavailable_without_fake_success() {
        let adapter = TauriWebViewPreviewAdapter;
        let workspace = Workspace::bind(
            herdr_workbench_domain::HerdrWorkspaceContext::new(
                "herdr-1",
                "Test",
                std::path::PathBuf::from(r"C:\workspace"),
            )
            .unwrap(),
        );
        let result = adapter.open(&workspace, None).await;
        assert!(result.is_err());
    }
}
