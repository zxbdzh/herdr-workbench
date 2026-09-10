use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use herdr_workbench_app_core::{
    CapturePreview, EventBus, OpenPreview, PreviewAdapter, PreviewDiagnosticsSink, PreviewError,
    PreviewScreenshotRepository, PreviewTransactionRepository, ScreenshotStore,
    WorkspaceRepository,
};
use herdr_workbench_contracts::{
    ApiDoc, PreviewDiagnosticDto, PreviewDiagnosticsResponse, PreviewOpenRequest,
    PreviewScreenshotResponse, PreviewStateResponse, WorkspaceDto, WorkspaceEventEnvelope,
    WorkspaceListResponse,
};
use rust_embed::RustEmbed;
use utoipa::OpenApi;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct WebAssets;

pub struct AppState<W, P, A, S> {
    pub workspaces: Arc<W>,
    pub previews: Arc<P>,
    pub preview_adapter: Arc<A>,
    pub events: Arc<EventBus>,
    pub screenshots: Arc<S>,
    pub diagnostics: Arc<dyn PreviewDiagnosticsSink>,
}

impl<W, P, A, S> Clone for AppState<W, P, A, S> {
    fn clone(&self) -> Self {
        Self {
            workspaces: Arc::clone(&self.workspaces),
            previews: Arc::clone(&self.previews),
            preview_adapter: Arc::clone(&self.preview_adapter),
            events: Arc::clone(&self.events),
            screenshots: Arc::clone(&self.screenshots),
            diagnostics: Arc::clone(&self.diagnostics),
        }
    }
}

impl<W, P, A, S> AppState<W, P, A, S> {
    pub fn new(
        workspaces: Arc<W>,
        previews: Arc<P>,
        preview_adapter: Arc<A>,
        events: Arc<EventBus>,
        screenshots: Arc<S>,
        diagnostics: Arc<dyn PreviewDiagnosticsSink>,
    ) -> Self {
        Self {
            workspaces,
            previews,
            preview_adapter,
            events,
            screenshots,
            diagnostics,
        }
    }
}

pub fn router<W, P, A, S>(state: AppState<W, P, A, S>) -> Router
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/app", get(web_index))
        .route("/app/", get(web_index))
        .route("/app/{*path}", get(web_asset))
        .route("/api/v1/workspaces", get(workspaces::<W, P, A, S>))
        .route(
            "/api/v1/workspaces/{id}/state",
            get(workspace_state::<W, P, A, S>),
        )
        .route(
            "/api/v1/workspaces/{id}/preview/open",
            post(open_preview::<W, P, A, S>),
        )
        .route(
            "/api/v1/workspaces/{id}/preview/screenshot",
            post(capture_screenshot::<W, P, A, S>).get(get_screenshot::<W, P, A, S>),
        )
        .route(
            "/api/v1/workspaces/{id}/preview/diagnostics",
            get(get_diagnostics::<W, P, A, S>),
        )
        .route(
            "/ws/v1/workspaces/{id}",
            get(workspace_events::<W, P, A, S>),
        )
        .with_state(state)
}

pub fn empty_router() -> Router {
    router(AppState::new(
        Arc::new(EmptyWorkspaceRepository),
        Arc::new(EmptyPreviewRepository),
        Arc::new(UnavailablePreviewAdapter),
        Arc::new(herdr_workbench_app_core::EventBus::new(16)),
        Arc::new(EmptyScreenshotStore),
        Arc::new(herdr_workbench_app_core::InMemoryPreviewDiagnostics::default()),
    ))
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

async fn web_index() -> Response {
    web_asset(Path(String::from("index.html"))).await
}

async fn web_asset(Path(path): Path<String>) -> Response {
    let path = path.trim_start_matches('/');
    let asset = WebAssets::get(path).or_else(|| WebAssets::get("index.html"));
    let Some(asset) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let content_type = match path.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    };

    let mut response = Response::new(Body::from(asset.data.into_owned()));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

async fn workspaces<W, P, A, S>(
    State(state): State<AppState<W, P, A, S>>,
) -> Result<Json<WorkspaceListResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let items = state
        .workspaces
        .list()
        .await
        .map_err(ApiError::repository)?;
    Ok(Json(WorkspaceListResponse {
        workspaces: items.into_iter().map(WorkspaceDto::from).collect(),
    }))
}

async fn workspace_state<W, P, A, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, S>>,
) -> Result<Json<PreviewStateResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let preview = state
        .previews
        .find_by_workspace(&workspace_id)
        .await
        .map_err(ApiError::repository)?;

    Ok(Json(PreviewStateResponse::from_parts(workspace, preview)))
}

async fn open_preview<W, P, A, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, S>>,
    Json(request): Json<PreviewOpenRequest>,
) -> Result<Json<PreviewStateResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let session = OpenPreview::new(
        state.previews.as_ref(),
        state.preview_adapter.as_ref(),
        state.events.as_ref(),
    )
    .execute(&workspace, request.url)
    .await
    .map_err(ApiError::application)?;

    Ok(Json(PreviewStateResponse::from_parts(
        workspace,
        Some(session),
    )))
}

async fn capture_screenshot<W, P, A, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, S>>,
) -> Result<Json<PreviewScreenshotResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let screenshot = CapturePreview::new(
        state.previews.as_ref(),
        state.preview_adapter.as_ref(),
        state.events.as_ref(),
        state.screenshots.as_ref(),
    )
    .execute(&workspace)
    .await
    .map_err(ApiError::application)?;
    Ok(Json(PreviewScreenshotResponse::from(screenshot)))
}

async fn get_screenshot<W, P, A, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, S>>,
) -> Result<Response, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let screenshot = state
        .previews
        .find_latest(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or(ApiError::screenshot_not_found())?;
    let png = state
        .screenshots
        .load(&screenshot.path)
        .await
        .map_err(ApiError::repository)?;
    let mut response = Response::new(Body::from(png));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("image/png"));
    Ok(response)
}

async fn get_diagnostics<W, P, A, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, S>>,
) -> Result<Json<PreviewDiagnosticsResponse>, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let _ = workspace;
    let diagnostics = state.diagnostics.list(&workspace_id).await;
    Ok(Json(PreviewDiagnosticsResponse {
        diagnostics: diagnostics
            .into_iter()
            .map(PreviewDiagnosticDto::from)
            .collect(),
    }))
}

async fn workspace_events<W, P, A, S>(
    Path(id): Path<String>,
    State(state): State<AppState<W, P, A, S>>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError>
where
    W: WorkspaceRepository + 'static,
    P: PreviewTransactionRepository + PreviewScreenshotRepository + 'static,
    A: PreviewAdapter + 'static,
    S: ScreenshotStore + 'static,
{
    let workspace_id = parse_workspace_id(&id)?;
    let workspace = state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::repository)?
        .ok_or_else(|| ApiError::workspace_not_found(id.clone()))?;
    let preview = state
        .previews
        .find_by_workspace(&workspace_id)
        .await
        .map_err(ApiError::repository)?;
    let snapshot =
        WorkspaceEventEnvelope::snapshot(PreviewStateResponse::from_parts(workspace, preview));
    let receiver = state.events.subscribe();
    Ok(
        ws.on_upgrade(move |socket| {
            push_workspace_events(socket, workspace_id, snapshot, receiver)
        }),
    )
}

async fn push_workspace_events(
    mut socket: WebSocket,
    workspace_id: herdr_workbench_domain::WorkbenchWorkspaceId,
    snapshot: WorkspaceEventEnvelope,
    mut receiver: tokio::sync::broadcast::Receiver<herdr_workbench_domain::AppEvent>,
) {
    if send_envelope(&mut socket, &snapshot).await.is_err() {
        return;
    }
    loop {
        match receiver.recv().await {
            Ok(event) if event.workspace_id == workspace_id => {
                let envelope = WorkspaceEventEnvelope::from_app_event(event);
                if send_envelope(&mut socket, &envelope).await.is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({"event_type":"resync"})
                            .to_string()
                            .into(),
                    ))
                    .await;
                break;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn send_envelope(
    socket: &mut WebSocket,
    envelope: &WorkspaceEventEnvelope,
) -> Result<(), ()> {
    let payload = serde_json::to_string(envelope).map_err(|_| ())?;
    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}

fn parse_workspace_id(id: &str) -> Result<herdr_workbench_domain::WorkbenchWorkspaceId, ApiError> {
    uuid::Uuid::parse_str(id)
        .map(herdr_workbench_domain::WorkbenchWorkspaceId::from_uuid)
        .map_err(|_| ApiError::workspace_not_found(id.to_owned()))
}

#[derive(Debug)]
pub struct EmptyWorkspaceRepository;

#[async_trait::async_trait]
impl WorkspaceRepository for EmptyWorkspaceRepository {
    async fn find_by_herdr_id(
        &self,
        _: &herdr_workbench_domain::HerdrWorkspaceId,
    ) -> Result<Option<herdr_workbench_domain::Workspace>, herdr_workbench_app_core::RepositoryError>
    {
        Ok(None)
    }

    async fn find_by_id(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<Option<herdr_workbench_domain::Workspace>, herdr_workbench_app_core::RepositoryError>
    {
        Ok(None)
    }

    async fn list(
        &self,
    ) -> Result<Vec<herdr_workbench_domain::Workspace>, herdr_workbench_app_core::RepositoryError>
    {
        Ok(Vec::new())
    }

    async fn insert(
        &self,
        _: herdr_workbench_domain::Workspace,
    ) -> Result<(), herdr_workbench_app_core::RepositoryError> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct EmptyPreviewRepository;

#[async_trait::async_trait]
impl PreviewTransactionRepository for EmptyPreviewRepository {
    async fn find_by_workspace(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<
        Option<herdr_workbench_domain::PreviewSession>,
        herdr_workbench_app_core::RepositoryError,
    > {
        Ok(None)
    }

    async fn commit_preview_open(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
        _: herdr_workbench_domain::PreviewSession,
    ) -> Result<
        herdr_workbench_app_core::DurablePreviewCommit,
        herdr_workbench_app_core::RepositoryError,
    > {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "preview repository unavailable",
        ))
    }
}

#[async_trait::async_trait]
impl PreviewScreenshotRepository for EmptyPreviewRepository {
    async fn find_latest(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
    ) -> Result<
        Option<herdr_workbench_domain::PreviewScreenshot>,
        herdr_workbench_app_core::RepositoryError,
    > {
        Ok(None)
    }

    async fn commit_screenshot(
        &self,
        _: herdr_workbench_domain::PreviewScreenshot,
    ) -> Result<
        herdr_workbench_app_core::DurableScreenshotCommit,
        herdr_workbench_app_core::RepositoryError,
    > {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "preview repository unavailable",
        ))
    }
}

#[derive(Debug)]
pub struct EmptyScreenshotStore;

#[async_trait::async_trait]
impl ScreenshotStore for EmptyScreenshotStore {
    async fn save(
        &self,
        _: &herdr_workbench_domain::WorkbenchWorkspaceId,
        _: &[u8],
    ) -> Result<String, herdr_workbench_app_core::RepositoryError> {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "screenshot store unavailable",
        ))
    }

    async fn load(&self, _: &str) -> Result<Vec<u8>, herdr_workbench_app_core::RepositoryError> {
        Err(herdr_workbench_app_core::RepositoryError::new(
            "screenshot store unavailable",
        ))
    }
}

#[derive(Debug)]
pub struct UnavailablePreviewAdapter;

#[async_trait::async_trait]
impl PreviewAdapter for UnavailablePreviewAdapter {
    async fn open(
        &self,
        _: &herdr_workbench_domain::Workspace,
        _: Option<String>,
    ) -> Result<herdr_workbench_domain::PreviewSession, PreviewError> {
        Err(PreviewError::unavailable(
            "preview adapter is not configured",
        ))
    }

    async fn capture_screenshot(
        &self,
        _: &herdr_workbench_domain::Workspace,
    ) -> Result<herdr_workbench_app_core::CapturedPreviewImage, PreviewError> {
        Err(PreviewError::unavailable(
            "preview adapter is not configured",
        ))
    }
}

#[derive(Debug)]
pub struct ApiError {
    code: &'static str,
    message: String,
    status: StatusCode,
}

impl ApiError {
    fn workspace_not_found(_: String) -> Self {
        Self {
            code: "workspace_not_found",
            message: "workspace was not found".to_owned(),
            status: StatusCode::NOT_FOUND,
        }
    }

    fn screenshot_not_found() -> Self {
        Self {
            code: "screenshot_not_found",
            message: "preview screenshot was not found".to_owned(),
            status: StatusCode::NOT_FOUND,
        }
    }

    fn repository(error: herdr_workbench_app_core::RepositoryError) -> Self {
        Self {
            code: "internal_error",
            message: error.to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn application(error: herdr_workbench_app_core::ApplicationError) -> Self {
        match error {
            herdr_workbench_app_core::ApplicationError::Preview(error) => Self {
                code: "preview_unavailable",
                message: error.to_string(),
                status: StatusCode::SERVICE_UNAVAILABLE,
            },
            herdr_workbench_app_core::ApplicationError::Repository(error) => {
                Self::repository(error)
            }
            herdr_workbench_app_core::ApplicationError::Domain(error) => Self {
                code: "invalid_request",
                message: error.to_string(),
                status: StatusCode::BAD_REQUEST,
            },
            herdr_workbench_app_core::ApplicationError::Herdr(error) => Self {
                code: "herdr_unavailable",
                message: error.to_string(),
                status: StatusCode::SERVICE_UNAVAILABLE,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"error": {"code": self.code, "message": self.message, "request_id": null}})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use herdr_workbench_app_core::{
        BindWorkspace, EventBus, InMemoryPreviewDiagnostics, InMemoryPreviewRepository,
        InMemoryScreenshotStore, InMemoryWorkspaceRepository, OpenPreview, PreviewDiagnosticsSink,
        PreviewStateUpdater, PublishingPreviewStateUpdater,
    };
    use herdr_workbench_domain::{HerdrWorkspaceContext, PreviewSession};
    use tower::ServiceExt;

    struct FakePreviewAdapter;

    #[async_trait]
    impl PreviewAdapter for FakePreviewAdapter {
        async fn open(
            &self,
            workspace: &herdr_workbench_domain::Workspace,
            url: Option<String>,
        ) -> Result<PreviewSession, PreviewError> {
            Ok(PreviewSession::opening(workspace, url).mark_open())
        }

        async fn capture_screenshot(
            &self,
            _: &herdr_workbench_domain::Workspace,
        ) -> Result<herdr_workbench_app_core::CapturedPreviewImage, PreviewError> {
            Ok(herdr_workbench_app_core::CapturedPreviewImage {
                png: vec![137, 80, 78, 71, 13, 10, 26, 10],
            })
        }
    }

    #[tokio::test]
    async fn health_endpoint_reports_ready() {
        let response = empty_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_workspace_returns_the_stable_error_envelope() {
        let response = empty_router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/missing/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn app_route_serves_the_embedded_react_page() {
        let response = empty_router()
            .oneshot(Request::builder().uri("/app/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_open_and_state_routes_use_the_application_core() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            Arc::new(InMemoryPreviewDiagnostics::default()),
        );
        let app = router(state);
        let open_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/preview/open",
                workspace.workspace_id.as_uuid()
            ))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"url":"http://localhost:3000"}"#))
            .unwrap();
        let open_response = app.clone().oneshot(open_request).await.unwrap();
        assert_eq!(open_response.status(), StatusCode::OK);
        let state_response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/state",
                        workspace.workspace_id.as_uuid()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(state_response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(state_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["preview_url"], "http://localhost:3000");
    }

    #[tokio::test]
    async fn preview_screenshot_routes_capture_and_return_png() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            Arc::new(InMemoryPreviewDiagnostics::default()),
        );
        let app = router(state);
        let capture_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/preview/screenshot",
                workspace.workspace_id.as_uuid()
            ))
            .body(Body::empty())
            .unwrap();
        let capture_response = app.clone().oneshot(capture_request).await.unwrap();
        assert_eq!(capture_response.status(), StatusCode::OK);
        let screenshot_response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/preview/screenshot",
                        workspace.workspace_id.as_uuid()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(screenshot_response.status(), StatusCode::OK);
        assert_eq!(
            screenshot_response.headers().get(CONTENT_TYPE).unwrap(),
            "image/png"
        );
        let body = axum::body::to_bytes(screenshot_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), [137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[tokio::test]
    async fn preview_diagnostics_route_returns_recorded_events() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let diagnostics = Arc::new(InMemoryPreviewDiagnostics::default());
        diagnostics
            .record(herdr_workbench_domain::PreviewDiagnostic {
                workspace_id: workspace.workspace_id.clone(),
                kind: herdr_workbench_domain::PreviewDiagnosticKind::Console,
                level: herdr_workbench_domain::PreviewDiagnosticLevel::Error,
                message: "boom".into(),
                source: Some("http://localhost:3000/app.js".into()),
                status: None,
                occurred_at: chrono::Utc::now(),
            })
            .await;
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            diagnostics,
        );
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/preview/diagnostics",
                        workspace.workspace_id.as_uuid()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["diagnostics"][0]["kind"], "console");
        assert_eq!(json["diagnostics"][0]["level"], "error");
        assert_eq!(json["diagnostics"][0]["message"], "boom");
    }

    #[tokio::test]
    async fn preview_diagnostics_route_returns_empty_list_when_none_are_recorded() {
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            Arc::new(EventBus::new(8)),
            Arc::new(InMemoryScreenshotStore::default()),
            Arc::new(InMemoryPreviewDiagnostics::default()),
        );
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/workspaces/{}/preview/diagnostics",
                        workspace.workspace_id.as_uuid()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["diagnostics"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn unknown_workspace_websocket_returns_not_found() {
        let app = empty_router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!("ws://{addr}/ws/v1/workspaces/00000000-0000-0000-0000-000000000000");
        let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("404") || message.contains("Not Found"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn workspace_websocket_sends_snapshot_then_preview_opened() {
        use futures_util::StreamExt;
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let events = Arc::new(EventBus::new(8));
        let diagnostics = Arc::new(InMemoryPreviewDiagnostics::with_publisher(
            Arc::clone(&events) as Arc<dyn herdr_workbench_app_core::EventPublisher>,
        ));
        let state = AppState::new(
            workspaces,
            Arc::new(InMemoryPreviewRepository::default()),
            Arc::new(FakePreviewAdapter),
            events,
            Arc::new(InMemoryScreenshotStore::default()),
            diagnostics,
        );
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!(
            "ws://{addr}/ws/v1/workspaces/{}",
            workspace.workspace_id.as_uuid()
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let snapshot = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["event_type"], "workspace.snapshot");
        let request = format!(
            "POST /api/v1/workspaces/{}/preview/open HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}",
            workspace.workspace_id.as_uuid(),
            addr
        );
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        use tokio::io::AsyncWriteExt;
        stream.write_all(request.as_bytes()).await.unwrap();
        let opened = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let opened: serde_json::Value = serde_json::from_str(&opened).unwrap();
        assert_eq!(opened["event_type"], "preview.opened");
    }

    #[test]
    fn app_events_map_to_workspace_envelopes() {
        let workspace_id =
            herdr_workbench_domain::WorkbenchWorkspaceId::from_uuid(uuid::Uuid::nil());
        let session = PreviewSession {
            session_id: herdr_workbench_domain::PreviewSessionId::from_uuid(uuid::Uuid::nil()),
            workspace_id: workspace_id.clone(),
            url: Some("http://localhost:3000".into()),
            title: None,
            status: herdr_workbench_domain::PreviewStatus::Open,
        };
        let opened = WorkspaceEventEnvelope::from_app_event(
            herdr_workbench_domain::AppEvent::preview_opened(session.clone(), 1),
        );
        assert_eq!(opened.event_type, "preview.opened");
        assert_eq!(opened.revision, 1);
        assert_eq!(
            opened.payload["preview_session_id"],
            session.session_id.as_uuid().to_string()
        );
        assert_eq!(opened.payload["preview_url"], "http://localhost:3000");
        assert_eq!(opened.payload["preview_status"], "Open");
        assert!(opened.payload.get("PreviewOpened").is_none());
        let screenshot = herdr_workbench_domain::PreviewScreenshot {
            screenshot_id: uuid::Uuid::nil(),
            workspace_id: workspace_id.clone(),
            path: "latest.png".into(),
            sha256: "abc".into(),
            byte_size: 8,
            revision: 2,
        };
        let captured = WorkspaceEventEnvelope::from_app_event(
            herdr_workbench_domain::AppEvent::preview_screenshot_captured(screenshot, 2),
        );
        assert_eq!(captured.event_type, "preview.screenshot_captured");
        assert_eq!(captured.payload["path"], "latest.png");
        assert!(captured.payload.get("PreviewScreenshotCaptured").is_none());
        let diagnostic = herdr_workbench_domain::PreviewDiagnostic {
            workspace_id,
            kind: herdr_workbench_domain::PreviewDiagnosticKind::Console,
            level: herdr_workbench_domain::PreviewDiagnosticLevel::Error,
            message: "boom".into(),
            source: None,
            status: None,
            occurred_at: chrono::Utc::now(),
        };
        let updated = WorkspaceEventEnvelope::from_app_event(
            herdr_workbench_domain::AppEvent::preview_diagnostics_updated(diagnostic),
        );
        assert_eq!(updated.event_type, "preview.diagnostics_updated");
        assert_eq!(updated.revision, 0);
        assert_eq!(updated.payload["kind"], "console");
        assert!(updated.payload.get("PreviewDiagnosticsUpdated").is_none());
        let state = WorkspaceEventEnvelope::from_app_event(
            herdr_workbench_domain::AppEvent::preview_state_updated(session),
        );
        assert_eq!(state.event_type, "preview.state_updated");
        assert_eq!(state.revision, 0);
        assert_eq!(state.payload["preview_status"], "Open");
        assert_eq!(state.payload["preview_url"], "http://localhost:3000");
        assert!(state.payload["preview_title"].is_null());
        assert!(state.payload.get("PreviewStateUpdated").is_none());
    }

    #[tokio::test]
    async fn workspace_websocket_sends_snapshot_then_preview_state_updated() {
        use futures_util::StreamExt;
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let events = Arc::new(EventBus::new(8));
        let previews = Arc::new(InMemoryPreviewRepository::default());
        OpenPreview::new(previews.as_ref(), &FakePreviewAdapter, events.as_ref())
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        let diagnostics = Arc::new(InMemoryPreviewDiagnostics::with_publisher(
            Arc::clone(&events) as Arc<dyn herdr_workbench_app_core::EventPublisher>,
        ));
        let state = AppState::new(
            workspaces,
            Arc::clone(&previews),
            Arc::new(FakePreviewAdapter),
            Arc::clone(&events),
            Arc::new(InMemoryScreenshotStore::default()),
            diagnostics,
        );
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!(
            "ws://{addr}/ws/v1/workspaces/{}",
            workspace.workspace_id.as_uuid()
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let snapshot = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["event_type"], "workspace.snapshot");
        PublishingPreviewStateUpdater::new(Arc::clone(&previews), Arc::clone(&events))
            .update_preview_state(
                &workspace.workspace_id,
                herdr_workbench_domain::PreviewStatus::Open,
                Some("http://localhost:3000/app".into()),
                Some("App".into()),
            )
            .await
            .unwrap();
        let updated = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let updated: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["event_type"], "preview.state_updated");
        assert_eq!(updated["revision"], 0);
    }

    #[tokio::test]
    async fn workspace_websocket_reconnect_receives_a_fresh_snapshot() {
        use futures_util::StreamExt;
        let workspaces = Arc::new(InMemoryWorkspaceRepository::default());
        let workspace = BindWorkspace::new(workspaces.as_ref())
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    std::path::PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let events = Arc::new(EventBus::new(8));
        let previews = Arc::new(InMemoryPreviewRepository::default());
        OpenPreview::new(previews.as_ref(), &FakePreviewAdapter, events.as_ref())
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();
        let diagnostics = Arc::new(InMemoryPreviewDiagnostics::with_publisher(
            Arc::clone(&events) as Arc<dyn herdr_workbench_app_core::EventPublisher>,
        ));
        let state = AppState::new(
            workspaces,
            Arc::clone(&previews),
            Arc::new(FakePreviewAdapter),
            Arc::clone(&events),
            Arc::new(InMemoryScreenshotStore::default()),
            diagnostics,
        );
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!(
            "ws://{addr}/ws/v1/workspaces/{}",
            workspace.workspace_id.as_uuid()
        );
        let (mut first, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let snapshot = first.next().await.unwrap().unwrap().into_text().unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["event_type"], "workspace.snapshot");
        drop(first);
        let (mut second, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let snapshot = second.next().await.unwrap().unwrap().into_text().unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["event_type"], "workspace.snapshot");
        PublishingPreviewStateUpdater::new(Arc::clone(&previews), Arc::clone(&events))
            .update_preview_state(
                &workspace.workspace_id,
                herdr_workbench_domain::PreviewStatus::Open,
                Some("http://localhost:3000/app".into()),
                Some("App".into()),
            )
            .await
            .unwrap();
        let updated = second.next().await.unwrap().unwrap().into_text().unwrap();
        let updated: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["event_type"], "preview.state_updated");
        assert_eq!(updated["payload"]["preview_title"], "App");
    }
}
