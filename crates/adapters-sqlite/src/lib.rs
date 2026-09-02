use std::path::PathBuf;

use async_trait::async_trait;
use herdr_workbench_app_core::{RepositoryError, WorkspaceRepository};
use herdr_workbench_domain::{
    HerdrWorkspaceContext, HerdrWorkspaceId, WorkbenchWorkspaceId, Workspace,
};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

#[derive(Clone)]
pub struct SqliteWorkspaceRepository {
    pool: SqlitePool,
}

impl SqliteWorkspaceRepository {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
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
        let row = sqlx::query_as::<_, WorkspaceRow>(
            "SELECT workspace_id, herdr_workspace_id, label, cwd, revision
             FROM workspaces WHERE herdr_workspace_id = ?",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| RepositoryError::new(error.to_string()))?;

        row.map(WorkspaceRow::try_into_workspace)
            .transpose()
            .map_err(|error| RepositoryError::new(error.to_string()))
    }

    async fn list(&self) -> Result<Vec<Workspace>, RepositoryError> {
        let rows = sqlx::query_as::<_, WorkspaceRow>(
            "SELECT workspace_id, herdr_workspace_id, label, cwd, revision
             FROM workspaces ORDER BY created_at, workspace_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| RepositoryError::new(error.to_string()))?;

        rows.into_iter()
            .map(WorkspaceRow::try_into_workspace)
            .map(|result| result.map_err(|error| RepositoryError::new(error.to_string())))
            .collect()
    }

    async fn insert(&self, workspace: Workspace) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO workspaces
             (workspace_id, herdr_workspace_id, label, cwd, revision)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(workspace.workspace_id.as_uuid().to_string())
        .bind(workspace.herdr_workspace_id.as_str())
        .bind(workspace.label)
        .bind(workspace.cwd.to_string_lossy().as_ref())
        .bind(workspace.revision as i64)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| RepositoryError::new(error.to_string()))
    }

    async fn increment_revision(
        &self,
        workspace_id: &WorkbenchWorkspaceId,
    ) -> Result<u64, RepositoryError> {
        let row = sqlx::query_as::<_, RevisionRow>(
            "UPDATE workspaces
             SET revision = revision + 1
             WHERE workspace_id = ?
             RETURNING revision",
        )
        .bind(workspace_id.as_uuid().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| RepositoryError::new(error.to_string()))?
        .ok_or_else(|| RepositoryError::new("workspace not found"))?;

        Ok(row.revision as u64)
    }
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
    fn try_into_workspace(self) -> Result<Workspace, String> {
        let workspace_id =
            uuid::Uuid::parse_str(&self.workspace_id).map_err(|error| error.to_string())?;
        let herdr_workspace_id =
            HerdrWorkspaceId::parse(self.herdr_workspace_id).map_err(|error| error.to_string())?;
        let context = HerdrWorkspaceContext::new(
            herdr_workspace_id.as_str(),
            self.label,
            PathBuf::from(self.cwd),
        )
        .map_err(|error| error.to_string())?;
        let mut workspace = Workspace::bind(context);
        workspace.workspace_id = WorkbenchWorkspaceId::from_uuid(workspace_id);
        workspace.revision = self.revision as u64;
        Ok(workspace)
    }
}

#[derive(sqlx::FromRow)]
struct RevisionRow {
    revision: i64,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use herdr_workbench_app_core::{BindWorkspace, WorkspaceRepository};
    use herdr_workbench_domain::HerdrWorkspaceContext;

    use super::SqliteWorkspaceRepository;

    #[tokio::test]
    async fn workspace_survives_a_new_sqlite_repository_connection() {
        let database_url = "sqlite://file:workbench-restart?mode=memory&cache=shared";
        let first = SqliteWorkspaceRepository::connect(database_url)
            .await
            .unwrap();
        first.migrate().await.unwrap();

        let workspace = BindWorkspace::new(&first)
            .execute(
                HerdrWorkspaceContext::new(
                    "herdr-1",
                    "Siftmark",
                    PathBuf::from(r"C:\projects\siftmark"),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let second = SqliteWorkspaceRepository::connect(database_url)
            .await
            .unwrap();
        second.migrate().await.unwrap();
        let restored = second
            .find_by_herdr_id(&workspace.herdr_workspace_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(restored.workspace_id, workspace.workspace_id);
        assert_eq!(restored.cwd, workspace.cwd);
    }
}
