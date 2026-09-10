export const WORKSPACE_SOCKET_RECONNECT_MS = 1000;

export type WorkspaceSocketAction =
  | { type: "query-state" }
  | { type: "reconnect"; delayMs: number };

export const nextWorkspaceSocketAction = (
  eventType: "resync" | "close",
  stopped: boolean,
): WorkspaceSocketAction | null => {
  if (stopped) {
    return null;
  }
  if (eventType === "resync") {
    return { type: "query-state" };
  }
  return { type: "reconnect", delayMs: WORKSPACE_SOCKET_RECONNECT_MS };
};
