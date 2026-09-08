CREATE TABLE IF NOT EXISTS preview_screenshots (
    screenshot_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
