#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use herdr_workbench_app_core::{PreviewDiagnosticsSink, PreviewStateUpdater};
use herdr_workbench_domain::{
    PreviewDiagnostic, PreviewDiagnosticKind, PreviewDiagnosticLevel, PreviewSession,
    PreviewStatus, WorkbenchWorkspaceId, Workspace,
};
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
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
}

impl TauriWebViewPreviewAdapter {
    pub fn new(
        app: AppHandle,
        state: Arc<dyn PreviewStateUpdater>,
        diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    ) -> Self {
        Self {
            app,
            state,
            diagnostics,
        }
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

    fn observe_window_close(
        window: &tauri::WebviewWindow,
        state: Arc<dyn PreviewStateUpdater>,
        diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    ) {
        let label = window.label().to_owned();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                Self::persist_page_state(&state, &label, PreviewStatus::Unavailable, None, None);
                if let Some(workspace_id) = Self::workspace_id_from_label(&label) {
                    let diagnostics = Arc::clone(&diagnostics);
                    tauri::async_runtime::spawn(async move {
                        diagnostics.clear(&workspace_id).await;
                    });
                }
            }
        });
    }

    fn observe_diagnostics(
        window: &tauri::WebviewWindow,
        diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    ) {
        let label = window.label().to_owned();
        let _ = window.with_webview(move |webview| {
            if let Err(error) = attach_preview_diagnostics(webview, label, diagnostics) {
                eprintln!("failed to attach Preview diagnostics: {error}");
            }
        });
    }

    async fn apply_open_plan(
        &self,
        workspace: &Workspace,
        window: &tauri::WebviewWindow,
        plan: PreviewOpenPlan,
    ) {
        if plan.clear_diagnostics {
            self.diagnostics.clear(&workspace.workspace_id).await;
        }
        if plan.observe_close {
            Self::observe_window_close(
                window,
                Arc::clone(&self.state),
                Arc::clone(&self.diagnostics),
            );
        }
        if plan.attach_diagnostics {
            Self::observe_diagnostics(window, Arc::clone(&self.diagnostics));
        }
    }
}

struct PreviewOpenPlan {
    clear_diagnostics: bool,
    attach_diagnostics: bool,
    observe_close: bool,
}

impl PreviewOpenPlan {
    fn reuse() -> Self {
        Self {
            clear_diagnostics: true,
            attach_diagnostics: false,
            observe_close: false,
        }
    }

    fn create() -> Self {
        Self {
            clear_diagnostics: false,
            attach_diagnostics: true,
            observe_close: true,
        }
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
            self.apply_open_plan(workspace, &window, PreviewOpenPlan::reuse())
                .await;
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
            self.apply_open_plan(workspace, &window, PreviewOpenPlan::create())
                .await;
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

fn attach_preview_diagnostics(
    webview: tauri::webview::PlatformWebview,
    label: String,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        unsafe { attach_webview2_diagnostics(webview, label, diagnostics) }
    }
    #[cfg(not(windows))]
    {
        let _ = (webview, label, diagnostics);
        Ok(())
    }
}

#[cfg(windows)]
unsafe fn attach_webview2_diagnostics(
    webview: tauri::webview::PlatformWebview,
    label: String,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

    let Some(workspace_id) = TauriWebViewPreviewAdapter::workspace_id_from_label(&label) else {
        return Ok(());
    };
    let controller = webview.controller();
    let core = unsafe { controller.CoreWebView2() }.map_err(|error| error.to_string())?;
    let core: ICoreWebView2 = core;
    enable_runtime(&core)?;
    subscribe_console(&core, Arc::clone(&diagnostics), workspace_id.clone())?;
    subscribe_exception(&core, Arc::clone(&diagnostics), workspace_id.clone())?;
    subscribe_failed_network(&core, diagnostics, workspace_id)?;
    Ok(())
}

#[cfg(windows)]
fn enable_runtime(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) -> Result<(), String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows::core::PCWSTR;
    let method: Vec<u16> = "Runtime.enable\0".encode_utf16().collect();
    let params: Vec<u16> = "{}\0".encode_utf16().collect();
    unsafe {
        core.CallDevToolsProtocolMethod(
            PCWSTR::from_raw(method.as_ptr()),
            PCWSTR::from_raw(params.as_ptr()),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_hr, _json| Ok(()))),
        )
    }
    .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn subscribe_console(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    workspace_id: WorkbenchWorkspaceId,
) -> Result<(), String> {
    subscribe_cdp_event(
        core,
        "Runtime.consoleAPICalled",
        move |payload| {
            parse_console_event(payload).map(|mut diagnostic| {
                diagnostic.workspace_id = workspace_id.clone();
                diagnostic
            })
        },
        diagnostics,
    )
}

#[cfg(windows)]
fn subscribe_exception(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    workspace_id: WorkbenchWorkspaceId,
) -> Result<(), String> {
    subscribe_cdp_event(
        core,
        "Runtime.exceptionThrown",
        move |payload| {
            parse_exception_event(payload).map(|mut diagnostic| {
                diagnostic.workspace_id = workspace_id.clone();
                diagnostic
            })
        },
        diagnostics,
    )
}

#[cfg(windows)]
fn subscribe_cdp_event<F>(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    event_name: &str,
    parse: F,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
) -> Result<(), String>
where
    F: Fn(String) -> Option<PreviewDiagnostic> + Send + Sync + 'static,
{
    use webview2_com::{DevToolsProtocolEventReceivedEventHandler, take_pwstr};
    use windows::core::{PCWSTR, PWSTR};
    let name: Vec<u16> = format!("{event_name}\0").encode_utf16().collect();
    let receiver =
        unsafe { core.GetDevToolsProtocolEventReceiver(PCWSTR::from_raw(name.as_ptr())) }
            .map_err(|error| error.to_string())?;
    let mut token = 0_i64;
    let handler =
        DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args {
                let mut json = PWSTR::null();
                if unsafe { args.ParameterObjectAsJson(&mut json) }.is_ok() {
                    let payload = take_pwstr(json);
                    if let Some(diagnostic) = parse(payload) {
                        record_diagnostic(Arc::clone(&diagnostics), diagnostic);
                    }
                }
            }
            Ok(())
        }));
    unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn subscribe_failed_network(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    workspace_id: WorkbenchWorkspaceId,
) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
    use webview2_com::{WebResourceResponseReceivedEventHandler, take_pwstr};
    use windows::core::{Interface, PWSTR};
    let core2: ICoreWebView2_2 = core.cast().map_err(|error| error.to_string())?;
    let mut token = 0_i64;
    let handler =
        WebResourceResponseReceivedEventHandler::create(Box::new(move |_sender, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let Ok(request) = (unsafe { args.Request() }) else {
                return Ok(());
            };
            let Ok(response) = (unsafe { args.Response() }) else {
                return Ok(());
            };
            let mut uri = PWSTR::null();
            let source = if unsafe { request.Uri(&mut uri) }.is_ok() {
                let value = take_pwstr(uri);
                if value.is_empty() { None } else { Some(value) }
            } else {
                None
            };
            let mut status = 0_i32;
            let status_code = if unsafe { response.StatusCode(&mut status) }.is_ok() {
                Some(status as u16)
            } else {
                None
            };
            let failed = status_code.map(|code| code >= 400).unwrap_or(true);
            if !failed {
                return Ok(());
            }
            record_diagnostic(
                Arc::clone(&diagnostics),
                PreviewDiagnostic {
                    workspace_id: workspace_id.clone(),
                    kind: PreviewDiagnosticKind::Network,
                    level: PreviewDiagnosticLevel::Error,
                    message: status_code
                        .map(|code| format!("HTTP {code}"))
                        .unwrap_or_else(|| "network request failed".to_owned()),
                    source,
                    status: status_code,
                    occurred_at: chrono::Utc::now(),
                },
            );
            Ok(())
        }));
    unsafe { core2.add_WebResourceResponseReceived(&handler, &mut token) }
        .map_err(|error| error.to_string())
}

fn parse_console_event(payload: String) -> Option<PreviewDiagnostic> {
    let value: serde_json::Value = serde_json::from_str(&payload).ok()?;
    let level = value.get("type")?.as_str()?;
    let diagnostic_level = match level {
        "error" => PreviewDiagnosticLevel::Error,
        "warning" => PreviewDiagnosticLevel::Warning,
        _ => return None,
    };
    let message = value
        .get("args")
        .and_then(|args| args.as_array())
        .map(|args| {
            args.iter()
                .filter_map(|arg| {
                    arg.get("value")
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .or_else(|| {
                            arg.get("description")
                                .and_then(|value| value.as_str().map(str::to_owned))
                        })
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "console error".to_owned());
    Some(PreviewDiagnostic {
        workspace_id: WorkbenchWorkspaceId::from_uuid(uuid::Uuid::nil()),
        kind: PreviewDiagnosticKind::Console,
        level: diagnostic_level,
        message,
        source: value
            .get("stackTrace")
            .and_then(|stack| stack.get("callFrames"))
            .and_then(|frames| frames.as_array())
            .and_then(|frames| frames.first())
            .and_then(|frame| frame.get("url"))
            .and_then(|url| url.as_str())
            .map(str::to_owned),
        status: None,
        occurred_at: chrono::Utc::now(),
    })
}

fn parse_exception_event(payload: String) -> Option<PreviewDiagnostic> {
    let value: serde_json::Value = serde_json::from_str(&payload).ok()?;
    let details = value.get("exceptionDetails")?;
    let message = details
        .get("text")
        .and_then(|text| text.as_str())
        .or_else(|| {
            details
                .get("exception")
                .and_then(|exception| {
                    exception
                        .get("description")
                        .or_else(|| exception.get("value"))
                })
                .and_then(|value| value.as_str())
        })
        .unwrap_or("uncaught exception")
        .to_owned();
    Some(PreviewDiagnostic {
        workspace_id: WorkbenchWorkspaceId::from_uuid(uuid::Uuid::nil()),
        kind: PreviewDiagnosticKind::Exception,
        level: PreviewDiagnosticLevel::Error,
        message,
        source: details
            .get("url")
            .and_then(|url| url.as_str())
            .map(str::to_owned),
        status: None,
        occurred_at: chrono::Utc::now(),
    })
}

fn record_diagnostic(diagnostics: Arc<dyn PreviewDiagnosticsSink>, diagnostic: PreviewDiagnostic) {
    tauri::async_runtime::spawn(async move {
        diagnostics.record(diagnostic).await;
    });
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match connect_repository().await {
                    Ok(repository) => {
                        let state: Arc<dyn PreviewStateUpdater> = repository.clone();
                        let diagnostics: Arc<dyn PreviewDiagnosticsSink> = Arc::new(
                            herdr_workbench_app_core::InMemoryPreviewDiagnostics::default(),
                        );
                        let preview_adapter = Arc::new(TauriWebViewPreviewAdapter::new(
                            handle,
                            state,
                            Arc::clone(&diagnostics),
                        ));
                        if let Err(error) =
                            serve_with_repository(repository, preview_adapter, diagnostics).await
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

    #[test]
    fn reusing_an_open_preview_window_clears_diagnostics_and_keeps_existing_listeners() {
        let plan = PreviewOpenPlan::reuse();
        assert!(plan.clear_diagnostics);
        assert!(!plan.attach_diagnostics);
        assert!(!plan.observe_close);
    }

    #[test]
    fn creating_a_preview_window_attaches_diagnostics_once() {
        let plan = PreviewOpenPlan::create();
        assert!(!plan.clear_diagnostics);
        assert!(plan.attach_diagnostics);
        assert!(plan.observe_close);
    }

    #[test]
    fn console_error_payload_becomes_a_diagnostic() {
        let diagnostic = parse_console_event(
            r#"{"type":"error","args":[{"type":"string","value":"boom"}],"stackTrace":{"callFrames":[{"url":"http://localhost:3000/app.js"}]}}"#.into(),
        )
        .unwrap();
        assert_eq!(diagnostic.kind, PreviewDiagnosticKind::Console);
        assert_eq!(diagnostic.level, PreviewDiagnosticLevel::Error);
        assert_eq!(diagnostic.message, "boom");
        assert_eq!(
            diagnostic.source.as_deref(),
            Some("http://localhost:3000/app.js")
        );
    }

    #[test]
    fn console_log_payload_is_ignored() {
        assert!(parse_console_event(r#"{"type":"log","args":[{"value":"hi"}]}"#.into()).is_none());
    }

    #[test]
    fn exception_payload_becomes_a_diagnostic() {
        let diagnostic = parse_exception_event(
            r#"{"exceptionDetails":{"text":"Uncaught TypeError","url":"http://localhost:3000/app.js"}}"#.into(),
        )
        .unwrap();
        assert_eq!(diagnostic.kind, PreviewDiagnosticKind::Exception);
        assert_eq!(diagnostic.message, "Uncaught TypeError");
    }
}
