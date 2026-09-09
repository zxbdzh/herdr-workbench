use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{PreviewDiagnostic, PreviewSession, WorkbenchWorkspaceId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    PreviewOpened,
    PreviewScreenshotCaptured,
    PreviewDiagnosticsUpdated,
    PreviewStateUpdated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewOpened {
    pub session: PreviewSession,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewScreenshotCaptured {
    pub screenshot: crate::PreviewScreenshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewDiagnosticsUpdated {
    pub diagnostic: PreviewDiagnostic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewStateUpdated {
    pub session: PreviewSession,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPayload {
    PreviewOpened(PreviewOpened),
    PreviewScreenshotCaptured(PreviewScreenshotCaptured),
    PreviewDiagnosticsUpdated(PreviewDiagnosticsUpdated),
    PreviewStateUpdated(PreviewStateUpdated),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEvent {
    pub event_id: Uuid,
    pub event_type: EventType,
    pub workspace_id: WorkbenchWorkspaceId,
    pub occurred_at: DateTime<Utc>,
    pub revision: u64,
    pub payload: EventPayload,
}

impl AppEvent {
    pub fn preview_opened(session: PreviewSession, revision: u64) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: EventType::PreviewOpened,
            workspace_id: session.workspace_id.clone(),
            occurred_at: Utc::now(),
            revision,
            payload: EventPayload::PreviewOpened(PreviewOpened { session }),
        }
    }

    pub fn preview_screenshot_captured(
        screenshot: crate::PreviewScreenshot,
        revision: u64,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: EventType::PreviewScreenshotCaptured,
            workspace_id: screenshot.workspace_id.clone(),
            occurred_at: Utc::now(),
            revision,
            payload: EventPayload::PreviewScreenshotCaptured(PreviewScreenshotCaptured {
                screenshot,
            }),
        }
    }

    pub fn preview_diagnostics_updated(diagnostic: PreviewDiagnostic) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: EventType::PreviewDiagnosticsUpdated,
            workspace_id: diagnostic.workspace_id.clone(),
            occurred_at: Utc::now(),
            revision: 0,
            payload: EventPayload::PreviewDiagnosticsUpdated(PreviewDiagnosticsUpdated {
                diagnostic,
            }),
        }
    }

    pub fn preview_state_updated(session: PreviewSession) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: EventType::PreviewStateUpdated,
            workspace_id: session.workspace_id.clone(),
            occurred_at: Utc::now(),
            revision: 0,
            payload: EventPayload::PreviewStateUpdated(PreviewStateUpdated { session }),
        }
    }
}
