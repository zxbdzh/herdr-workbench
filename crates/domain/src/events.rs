use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{PreviewSession, WorkbenchWorkspaceId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    PreviewOpened,
    PreviewScreenshotCaptured,
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
pub enum EventPayload {
    PreviewOpened(PreviewOpened),
    PreviewScreenshotCaptured(PreviewScreenshotCaptured),
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
}
