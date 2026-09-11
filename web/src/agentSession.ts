export interface WorkspaceAgent {
  pane_id: string;
  agent: string;
  status: string;
  focused: boolean;
}

export const agentIsBlocked = (status: string): boolean =>
  status.toLowerCase() === "blocked";

export const pickDefaultAgent = (
  agents: WorkspaceAgent[],
  currentPaneId?: string | null,
): WorkspaceAgent | null => {
  if (currentPaneId) {
    const current = agents.find((agent) => agent.pane_id === currentPaneId);
    if (current) return current;
  }
  return agents.find((agent) => agent.focused) ?? agents[0] ?? null;
};
