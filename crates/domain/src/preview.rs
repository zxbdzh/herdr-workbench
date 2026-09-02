use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{WorkbenchWorkspaceId, Workspace};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PreviewSessionId(Uuid);

impl PreviewSessionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for PreviewSessionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewStatus {
    Opening,
    Open,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewSession {
    pub session_id: PreviewSessionId,
    pub workspace_id: WorkbenchWorkspaceId,
    pub url: Option<String>,
    pub status: PreviewStatus,
}

impl PreviewSession {
    pub fn opening(workspace: &Workspace, url: Option<String>) -> Self {
        Self {
            session_id: PreviewSessionId::new(),
            workspace_id: workspace.workspace_id.clone(),
            url,
            status: PreviewStatus::Opening,
        }
    }

    pub fn mark_open(mut self) -> Self {
        self.status = PreviewStatus::Open;
        self
    }
}
