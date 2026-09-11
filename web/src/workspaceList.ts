export const EMPTY_WORKSPACE_REFRESH_MS = 2000;
export const BOUND_WORKSPACE_REFRESH_MS = 30_000;

export const nextWorkspaceListRefreshMs = (boundCount: number): number =>
  boundCount === 0 ? EMPTY_WORKSPACE_REFRESH_MS : BOUND_WORKSPACE_REFRESH_MS;
