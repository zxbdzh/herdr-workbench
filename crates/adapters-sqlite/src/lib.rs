use std::path::PathBuf;

use async_trait::async_trait;
use herdr_workbench_app_core::{
    DurablePreviewCommit, DurableScreenshotCommit, PreviewScreenshotRepository,
    PreviewStateUpdater, PreviewTransactionRepository, RepositoryError, ScreenshotStore,
    WorkspaceRepository,
};
use herdr_workbench_domain::{
    AppEvent, HerdrWorkspaceContext, HerdrWorkspaceId, PreviewScreenshot, PreviewSession,
    PreviewStatus, WorkbenchWorkspaceId, Workspace,
};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

#[derive(Clone)]
pub struct SqliteWorkspaceRepository {
    pool: SqlitePool,
}

impl SqliteWorkspaceRepository {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let options: SqliteConnectOptions =
            url.parse::<SqliteConnectOptions>()?.create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl WorkspaceRepository for SqliteWorkspaceRepository {
    async fn find_by_herdr_id(
        &self,
        id: &HerdrWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError> {
        self.find_workspace("herdr_workspace_id", id.as_str()).await
    }

    async fn find_by_id(
        &self,
        id: &WorkbenchWorkspaceId,
    ) -> Result<Option<Workspace>, RepositoryError> {
        self.find_workspace("workspace_id", &id.as_uuid().to_string())
            .await
    }

    async fn list(&self) -> Result<Vec<Workspace>, RepositoryError> {
        let rows = sqlx::query_as::<_, WorkspaceRow>("SELECT workspace_id, herdr_workspace_id, label, cwd, revision FROM workspaces ORDER BY created_at, workspace_id")
            .fetch_all(&self.pool).await.map_err(db_error)?;
        rows.into_iter()
            .map(WorkspaceRow::into_workspace)
            .map(|result| result.map_err(RepositoryError::new))
            .collect()
    }

    async fn insert(&self, workspace: Workspace) -> Result<(), RepositoryError> {
        sqlx::query("INSERT INTO workspaces (workspace_id, herdr_workspace_id, label, cwd, revision) VALUES (?, ?, ?, ?, ?)")
            .bind(workspace.workspace_id.as_uuid().to_string()).bind(workspace.herdr_workspace_id.as_str())
            .bind(workspace.label).bind(workspace.cwd.to_string_lossy().to_string()).bind(workspace.revision as i64)
            .execute(&self.pool).await.map(|_| ()).map_err(db_error)
    }
}

impl SqliteWorkspaceRepository {
    async fn find_workspace(
        &self,
        column: &str,
        value: &str,
    ) -> Result<Option<Workspace>, RepositoryError> {
        let sql = format!(
            "SELECT workspace_id, herdr_workspace_id, label, cwd, revision FROM workspaces WHERE {column} = ?"
        );
        let row = sqlx::query_as::<_, WorkspaceRow>(&sql)
            .bind(value)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(WorkspaceRow::into_workspace)
            .transpose()
            .map_err(RepositoryError::new)
    }
}

#[async_trait]
impl PreviewTransactionRepository for SqliteWorkspaceRepository {
    async fn find_by_workspace(
        &self,
        id: &WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewSession>, RepositoryError> {
        let row = sqlx::query_as::<_, PreviewRow>("SELECT session_id, workspace_id, url, title, status FROM preview_sessions WHERE workspace_id = ?")
            .bind(id.as_uuid().to_string()).fetch_optional(&self.pool).await.map_err(db_error)?;
        row.map(PreviewRow::into_session)
            .transpose()
            .map_err(RepositoryError::new)
    }

    async fn commit_preview_open(
        &self,
        id: &WorkbenchWorkspaceId,
        session: PreviewSession,
    ) -> Result<DurablePreviewCommit, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let revision: i64 = sqlx::query_scalar("UPDATE workspaces SET revision = revision + 1 WHERE workspace_id = ? RETURNING revision")
            .bind(id.as_uuid().to_string()).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or_else(|| RepositoryError::new("workspace not found"))?;
        let event = AppEvent::preview_opened(session.clone(), revision as u64);
        let payload =
            serde_json::to_string(&event).map_err(|e| RepositoryError::new(e.to_string()))?;
        sqlx::query("INSERT INTO preview_sessions (session_id, workspace_id, url, title, status) VALUES (?, ?, ?, ?, ?)")
                    .bind(session.session_id.as_uuid().to_string()).bind(id.as_uuid().to_string()).bind(session.url.clone()).bind(session.title.clone()).bind("open")
            .execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO durable_events (event_id, event_type, workspace_id, occurred_at, revision, payload) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(event.event_id.to_string()).bind("preview_opened").bind(id.as_uuid().to_string()).bind(event.occurred_at.to_rfc3339()).bind(revision).bind(payload)
            .execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(DurablePreviewCommit { session, event })
    }
}

#[async_trait]
impl PreviewStateUpdater for SqliteWorkspaceRepository {
    async fn update_preview_state(
        &self,
        id: &WorkbenchWorkspaceId,
        status: PreviewStatus,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<PreviewSession, RepositoryError> {
        let status = match status {
            PreviewStatus::Open => "open",
            PreviewStatus::Opening => "opening",
            PreviewStatus::Unavailable => "unavailable",
        };
        let result = sqlx::query("UPDATE preview_sessions SET url = COALESCE(?, url), title = COALESCE(?, title), status = ?, updated_at = CURRENT_TIMESTAMP WHERE workspace_id = ?")
            .bind(&url)
            .bind(&title)
            .bind(status)
            .bind(id.as_uuid().to_string())
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(RepositoryError::new("preview session not found"));
        }
        PreviewTransactionRepository::find_by_workspace(self, id)
            .await?
            .ok_or_else(|| RepositoryError::new("preview session not found"))
    }
}

#[async_trait]
impl PreviewScreenshotRepository for SqliteWorkspaceRepository {
    async fn find_latest(
        &self,
        id: &WorkbenchWorkspaceId,
    ) -> Result<Option<PreviewScreenshot>, RepositoryError> {
        let row = sqlx::query_as::<_, ScreenshotRow>(
            "SELECT screenshot_id, workspace_id, path, sha256, byte_size, revision FROM preview_screenshots WHERE workspace_id = ?",
        )
        .bind(id.as_uuid().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(ScreenshotRow::into_screenshot)
            .transpose()
            .map_err(RepositoryError::new)
    }

    async fn commit_screenshot(
        &self,
        screenshot: PreviewScreenshot,
    ) -> Result<DurableScreenshotCommit, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let revision: i64 = sqlx::query_scalar("UPDATE workspaces SET revision = revision + 1 WHERE workspace_id = ? RETURNING revision")
            .bind(screenshot.workspace_id.as_uuid().to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or_else(|| RepositoryError::new("workspace not found"))?;
        let mut screenshot = screenshot;
        screenshot.revision = revision as u64;
        let event = AppEvent::preview_screenshot_captured(screenshot.clone(), screenshot.revision);
        let payload =
            serde_json::to_string(&event).map_err(|e| RepositoryError::new(e.to_string()))?;
        sqlx::query(
            "INSERT INTO preview_screenshots (screenshot_id, workspace_id, path, sha256, byte_size, revision) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(workspace_id) DO UPDATE SET screenshot_id = excluded.screenshot_id, path = excluded.path, sha256 = excluded.sha256, byte_size = excluded.byte_size, revision = excluded.revision, captured_at = CURRENT_TIMESTAMP",
        )
        .bind(screenshot.screenshot_id.to_string())
        .bind(screenshot.workspace_id.as_uuid().to_string())
        .bind(&screenshot.path)
        .bind(&screenshot.sha256)
        .bind(screenshot.byte_size as i64)
        .bind(revision)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("INSERT INTO durable_events (event_id, event_type, workspace_id, occurred_at, revision, payload) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(event.event_id.to_string())
            .bind("preview_screenshot_captured")
            .bind(screenshot.workspace_id.as_uuid().to_string())
            .bind(event.occurred_at.to_rfc3339())
            .bind(revision)
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(DurableScreenshotCommit { screenshot, event })
    }
}

#[derive(Clone)]
pub struct FilesystemScreenshotStore {
    root: PathBuf,
}

impl FilesystemScreenshotStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl ScreenshotStore for FilesystemScreenshotStore {
    async fn save(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
        png: &[u8],
    ) -> Result<String, RepositoryError> {
        let dir = self.root.join(workspace_id.as_uuid().simple().to_string());
        std::fs::create_dir_all(&dir).map_err(|e| RepositoryError::new(e.to_string()))?;
        let path = dir.join("latest.png");
        std::fs::write(&path, png).map_err(|e| RepositoryError::new(e.to_string()))?;
        Ok(path.to_string_lossy().into_owned())
    }

    async fn load(&self, path: &str) -> Result<Vec<u8>, RepositoryError> {
        let requested = PathBuf::from(path);
        let canonical_root = self
            .root
            .canonicalize()
            .map_err(|e| RepositoryError::new(e.to_string()))?;
        let canonical_path = requested
            .canonicalize()
            .map_err(|e| RepositoryError::new(e.to_string()))?;
        if !canonical_path.starts_with(&canonical_root) {
            return Err(RepositoryError::new(
                "screenshot path is outside the screenshot store",
            ));
        }
        std::fs::read(&canonical_path).map_err(|e| RepositoryError::new(e.to_string()))
    }
}

fn db_error(e: sqlx::Error) -> RepositoryError {
    RepositoryError::new(e.to_string())
}

#[derive(sqlx::FromRow)]
struct WorkspaceRow {
    workspace_id: String,
    herdr_workspace_id: String,
    label: String,
    cwd: String,
    revision: i64,
}
impl WorkspaceRow {
    fn into_workspace(self) -> Result<Workspace, String> {
        let context = HerdrWorkspaceContext::new(
            self.herdr_workspace_id,
            self.label,
            PathBuf::from(self.cwd),
        )
        .map_err(|e| e.to_string())?;
        let mut w = Workspace::bind(context);
        w.workspace_id = WorkbenchWorkspaceId::from_uuid(
            uuid::Uuid::parse_str(&self.workspace_id).map_err(|e| e.to_string())?,
        );
        w.revision = self.revision as u64;
        Ok(w)
    }
}
#[derive(sqlx::FromRow)]
struct PreviewRow {
    session_id: String,
    workspace_id: String,
    url: Option<String>,
    title: Option<String>,
    status: String,
}
impl PreviewRow {
    fn into_session(self) -> Result<PreviewSession, String> {
        Ok(PreviewSession {
            session_id: herdr_workbench_domain::PreviewSessionId::from_uuid(
                uuid::Uuid::parse_str(&self.session_id).map_err(|e| e.to_string())?,
            ),
            workspace_id: WorkbenchWorkspaceId::from_uuid(
                uuid::Uuid::parse_str(&self.workspace_id).map_err(|e| e.to_string())?,
            ),
            url: self.url,
            title: self.title,
            status: match self.status.as_str() {
                "open" => PreviewStatus::Open,
                "opening" => PreviewStatus::Opening,
                _ => PreviewStatus::Unavailable,
            },
        })
    }
}

#[derive(sqlx::FromRow)]
struct ScreenshotRow {
    screenshot_id: String,
    workspace_id: String,
    path: String,
    sha256: String,
    byte_size: i64,
    revision: i64,
}

impl ScreenshotRow {
    fn into_screenshot(self) -> Result<PreviewScreenshot, String> {
        Ok(PreviewScreenshot {
            screenshot_id: uuid::Uuid::parse_str(&self.screenshot_id).map_err(|e| e.to_string())?,
            workspace_id: WorkbenchWorkspaceId::from_uuid(
                uuid::Uuid::parse_str(&self.workspace_id).map_err(|e| e.to_string())?,
            ),
            path: self.path,
            sha256: self.sha256,
            byte_size: self.byte_size as u64,
            revision: self.revision as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use herdr_workbench_app_core::{
        BindWorkspace, CapturePreview, EventBus, OpenPreview, PreviewAdapter, PreviewError,
        PreviewScreenshotRepository, PreviewStateUpdater,
    };
    struct FakePreview;
    #[async_trait]
    impl PreviewAdapter for FakePreview {
        async fn open(
            &self,
            w: &Workspace,
            u: Option<String>,
        ) -> Result<PreviewSession, PreviewError> {
            Ok(PreviewSession::opening(w, u).mark_open())
        }

        async fn capture_screenshot(
            &self,
            _: &Workspace,
        ) -> Result<herdr_workbench_app_core::CapturedPreviewImage, PreviewError> {
            Ok(herdr_workbench_app_core::CapturedPreviewImage {
                png: vec![137, 80, 78, 71, 13, 10, 26, 10],
            })
        }
    }
    #[tokio::test]
    async fn transaction_persists_preview_and_event() {
        let db = SqliteWorkspaceRepository::connect("sqlite://file:tx?mode=memory&cache=shared")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let w = BindWorkspace::new(&db)
            .execute(HerdrWorkspaceContext::new("h", "w", PathBuf::from(r"C:\w")).unwrap())
            .await
            .unwrap();
        let bus = EventBus::new(4);
        let mut rx = bus.subscribe();
        OpenPreview::new(&db, &FakePreview, &bus)
            .execute(&w, None)
            .await
            .unwrap();
        let e = rx.recv().await.unwrap();
        assert_eq!(e.revision, 1);
        assert!(
            db.find_by_workspace(&w.workspace_id)
                .await
                .unwrap()
                .is_some()
        );
        let preview_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM preview_sessions")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM durable_events")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM workspaces")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(preview_count, 1);
        assert_eq!(event_count, 1);
        assert_eq!(revision, 1);
    }

    #[tokio::test]
    async fn updating_status_preserves_existing_url_and_title() {
        let db = SqliteWorkspaceRepository::connect("sqlite://file:close?mode=memory&cache=shared")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let workspace = BindWorkspace::new(&db)
            .execute(HerdrWorkspaceContext::new("h", "w", PathBuf::from(r"C:\w")).unwrap())
            .await
            .unwrap();
        let bus = EventBus::new(4);
        OpenPreview::new(&db, &FakePreview, &bus)
            .execute(&workspace, Some("http://localhost:3000".into()))
            .await
            .unwrap();

        db.update_preview_state(
            &workspace.workspace_id,
            PreviewStatus::Open,
            Some("http://localhost:3000/app".into()),
            Some("App".into()),
        )
        .await
        .unwrap();

        let closed = db
            .update_preview_state(
                &workspace.workspace_id,
                PreviewStatus::Unavailable,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(closed.status, PreviewStatus::Unavailable);
        assert_eq!(closed.url.as_deref(), Some("http://localhost:3000/app"));
        assert_eq!(closed.title.as_deref(), Some("App"));
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM workspaces")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(revision, 1);
    }

    #[tokio::test]
    async fn capturing_screenshot_persists_file_and_metadata() {
        let temp = std::env::temp_dir().join(format!(
            "herdr-screenshot-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let db = SqliteWorkspaceRepository::connect("sqlite://file:shot?mode=memory&cache=shared")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let workspace = BindWorkspace::new(&db)
            .execute(HerdrWorkspaceContext::new("h", "w", PathBuf::from(r"C:\w")).unwrap())
            .await
            .unwrap();
        let bus = EventBus::new(4);
        let store = FilesystemScreenshotStore::new(temp.clone());
        let screenshot = CapturePreview::new(&db, &FakePreview, &bus, &store)
            .execute(&workspace)
            .await
            .unwrap();
        let stored = db
            .find_latest(&workspace.workspace_id)
            .await
            .unwrap()
            .expect("screenshot metadata");
        assert_eq!(stored.screenshot_id, screenshot.screenshot_id);
        assert_eq!(stored.revision, 1);
        assert_eq!(stored.byte_size, 8);
        assert_eq!(
            store.load(&stored.path).await.unwrap(),
            vec![137, 80, 78, 71, 13, 10, 26, 10]
        );
        let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM durable_events")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(event_count, 1);
        let _ = std::fs::remove_dir_all(temp);
    }
}
