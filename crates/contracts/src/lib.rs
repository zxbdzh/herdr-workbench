use serde::Serialize;
use utoipa::{OpenApi, ToSchema};

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceDto {
    pub workspace_id: String,
    pub herdr_workspace_id: String,
    pub label: String,
    pub cwd: String,
    pub revision: u64,
}

impl From<herdr_workbench_domain::Workspace> for WorkspaceDto {
    fn from(workspace: herdr_workbench_domain::Workspace) -> Self {
        Self {
            workspace_id: workspace.workspace_id.as_uuid().to_string(),
            herdr_workspace_id: workspace.herdr_workspace_id.as_str().to_owned(),
            label: workspace.label,
            cwd: workspace.cwd.to_string_lossy().into_owned(),
            revision: workspace.revision,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceListResponse {
    pub workspaces: Vec<WorkspaceDto>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PreviewStateResponse {
    pub workspace: WorkspaceDto,
    pub preview_session_id: Option<String>,
    pub preview_url: Option<String>,
    pub preview_status: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        list_workspaces,
        get_workspace_state,
    ),
    components(schemas(
        HealthResponse,
        WorkspaceDto,
        WorkspaceListResponse,
        PreviewStateResponse,
        ErrorResponse,
        ErrorBody,
    )),
    tags((name = "system", description = "Workbench system endpoints"))
)]
pub struct ApiDoc;

#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "system",
    responses((status = 200, body = HealthResponse))
)]
pub fn health() {}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces",
    responses((status = 200, body = WorkspaceListResponse))
)]
pub fn list_workspaces() {}

#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/state",
    params(("id" = String, Path, description = "Workbench workspace ID")),
    responses(
        (status = 200, body = PreviewStateResponse),
        (status = 404, body = ErrorResponse)
    )
)]
pub fn get_workspace_state() {}
