#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use herdr_workbench_app_core::PreviewStateUpdater;
use herdr_workbench_domain::{PreviewSession, PreviewStatus, WorkbenchWorkspaceId, Workspace};
use herdr_workbench_server::{connect_repository, serve_with_repository};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
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

    fn observe_window_close(window: &tauri::WebviewWindow, state: Arc<dyn PreviewStateUpdater>) {
        let label = window.label().to_owned();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                Self::persist_page_state(&state, &label, PreviewStatus::Unavailable, None, None);
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
            Self::observe_window_close(&window, Arc::clone(&self.state));
        } else {
            let state_started = Arc::clone(&self.state);
            let state_title = Arc::clone(&self.state);
            let window =
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
            Self::observe_window_close(&window, Arc::clone(&self.state));
        }

        Ok(PreviewSession::opening(workspace, Some(target.to_string())).mark_open())
    }

    async fn capture_screenshot(
        &self,
        workspace: &Workspace,
    ) -> Result<
        herdr_workbench_app_core::CapturedPreviewImage,
        herdr_workbench_app_core::PreviewError,
    > {
        let label = Self::window_label(workspace);
        let Some(window) = self.app.get_webview_window(&label) else {
            return Err(herdr_workbench_app_core::PreviewError::unavailable(
                "preview window is not open",
            ));
        };
        capture_webview_png(&window)
    }
}

fn capture_webview_png(
    window: &tauri::WebviewWindow,
) -> Result<herdr_workbench_app_core::CapturedPreviewImage, herdr_workbench_app_core::PreviewError>
{
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| {
            let result = unsafe { capture_webview2_png(webview) };
            let _ = tx.send(result);
        })
        .map_err(|error| {
            herdr_workbench_app_core::PreviewError::unavailable(format!(
                "failed to access preview webview: {error}"
            ))
        })?;
    rx.recv().map_err(|_| {
        herdr_workbench_app_core::PreviewError::unavailable(
            "preview screenshot capture was cancelled",
        )
    })?
}

#[cfg(windows)]
unsafe fn capture_webview2_png(
    webview: tauri::webview::PlatformWebview,
) -> Result<herdr_workbench_app_core::CapturedPreviewImage, herdr_workbench_app_core::PreviewError>
{
    use std::fs;
    use webview2_com::{
        CapturePreviewCompletedHandler,
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    };
    use windows::{
        Win32::System::Com::{STGC_DEFAULT, STGM_CREATE, STGM_READWRITE, STGM_SHARE_EXCLUSIVE},
        Win32::UI::Shell::SHCreateStreamOnFileEx,
        core::HSTRING,
    };

    let controller = webview.controller();
    let core = unsafe { controller.CoreWebView2() }.map_err(|error| {
        herdr_workbench_app_core::PreviewError::unavailable(format!(
            "failed to access WebView2: {error}"
        ))
    })?;
    let path = unique_capture_path()?;
    let path_wide = HSTRING::from(path.as_os_str());
    let stream = unsafe {
        SHCreateStreamOnFileEx(
            &path_wide,
            STGM_CREATE.0 | STGM_READWRITE.0 | STGM_SHARE_EXCLUSIVE.0,
            0,
            true,
            None,
        )
    }
    .map_err(|error| {
        herdr_workbench_app_core::PreviewError::unavailable(format!(
            "failed to create screenshot stream: {error}"
        ))
    })?;
    let stream_for_capture = stream.clone();
    CapturePreviewCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            unsafe {
                core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream_for_capture,
                    &handler,
                )
            }
            .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(|result| result),
    )
    .map_err(|error| {
        herdr_workbench_app_core::PreviewError::unavailable(format!(
            "failed to capture preview screenshot: {error}"
        ))
    })?;
    unsafe { stream.Commit(STGC_DEFAULT) }.map_err(|error| {
        herdr_workbench_app_core::PreviewError::unavailable(format!(
            "failed to commit screenshot stream: {error}"
        ))
    })?;
    drop(stream);

    let png = fs::read(&path).map_err(|error| {
        herdr_workbench_app_core::PreviewError::unavailable(format!(
            "failed to read screenshot file: {error}"
        ))
    })?;
    let _ = fs::remove_file(&path);
    if png.is_empty() {
        return Err(herdr_workbench_app_core::PreviewError::unavailable(
            "preview screenshot was empty",
        ));
    }
    Ok(herdr_workbench_app_core::CapturedPreviewImage { png })
}

#[cfg(windows)]
fn unique_capture_path() -> Result<std::path::PathBuf, herdr_workbench_app_core::PreviewError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(std::env::temp_dir().join(format!("herdr-preview-capture-{nanos}.png")))
}

#[cfg(not(windows))]
unsafe fn capture_webview2_png(
    _: tauri::webview::PlatformWebview,
) -> Result<herdr_workbench_app_core::CapturedPreviewImage, herdr_workbench_app_core::PreviewError>
{
    Err(herdr_workbench_app_core::PreviewError::unavailable(
        "preview screenshots are only supported on Windows",
    ))
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

    #[test]
    fn unknown_preview_label_does_not_map_to_a_workspace() {
        assert_eq!(
            TauriWebViewPreviewAdapter::workspace_id_from_label("main"),
            None
        );
        assert_eq!(
            TauriWebViewPreviewAdapter::workspace_id_from_label("preview-not-a-uuid"),
            None
        );
    }
}
