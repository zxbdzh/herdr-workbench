use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use async_trait::async_trait;
use herdr_workbench_app_core::{
    AGENT_TRANSCRIPT_LINES, HerdrAgentBridge, HerdrAgentInfo, HerdrClientFactory,
    HerdrClientSession, HerdrEventSource, HerdrHost, HerdrHostError, HerdrLifecycleEvent,
    HerdrPaneInfo, HerdrWorkspaceInfo,
};
use serde::Deserialize;
use tokio::process::Command;

pub struct HerdrCliHost {
    binary: PathBuf,
}

impl HerdrCliHost {
    pub fn from_env() -> Self {
        let binary = std::env::var_os("HERDR_BIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("herdr"));
        Self { binary }
    }

    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }
}

#[derive(Debug, Deserialize)]
struct CliEnvelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    workspaces: Vec<CliWorkspace>,
}

#[derive(Debug, Deserialize)]
struct CliWorkspace {
    workspace_id: String,
    label: String,
    #[serde(default)]
    worktree: Option<CliWorktree>,
}

#[derive(Debug, Deserialize)]
struct CliWorktree {
    checkout_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PaneListResult {
    panes: Vec<CliPane>,
}

#[derive(Debug, Deserialize)]
struct CliPane {
    workspace_id: String,
    cwd: Option<String>,
    #[serde(default)]
    focused: bool,
}

pub fn parse_workspace_list(stdout: &str) -> Result<Vec<HerdrWorkspaceInfo>, HerdrHostError> {
    let envelope: CliEnvelope<WorkspaceListResult> =
        serde_json::from_str(stdout).map_err(|error| {
            HerdrHostError::unavailable(format!("invalid herdr workspace list JSON: {error}"))
        })?;
    Ok(envelope
        .result
        .workspaces
        .into_iter()
        .map(|workspace| HerdrWorkspaceInfo {
            workspace_id: workspace.workspace_id,
            label: workspace.label,
            worktree_checkout_path: workspace
                .worktree
                .and_then(|worktree| worktree.checkout_path)
                .map(PathBuf::from),
        })
        .collect())
}

pub fn parse_pane_list(stdout: &str) -> Result<Vec<HerdrPaneInfo>, HerdrHostError> {
    let envelope: CliEnvelope<PaneListResult> = serde_json::from_str(stdout).map_err(|error| {
        HerdrHostError::unavailable(format!("invalid herdr pane list JSON: {error}"))
    })?;
    Ok(envelope
        .result
        .panes
        .into_iter()
        .map(|pane| HerdrPaneInfo {
            workspace_id: pane.workspace_id,
            cwd: pane
                .cwd
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            focused: pane.focused,
        })
        .collect())
}

pub fn windows_herdr_creation_flags() -> u32 {
    0x0800_0000
}

pub fn herdr_client_attach_args() -> Vec<String> {
    Vec::new()
}

pub fn mapped_herdr_client_config() -> &'static str {
    concat!(
        "onboarding = false\n",
        "[terminal]\n",
        "kitty_graphics = false\n",
        "[ui]\n",
        "redraw_on_focus_gained = false\n",
    )
}

pub fn mapped_herdr_client_config_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("HerdrWorkbench")
        .join("mapped-herdr-client.toml")
}

pub fn write_mapped_herdr_client_config(path: &Path) -> Result<PathBuf, HerdrHostError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            HerdrHostError::unavailable(format!(
                "failed to create mapped Herdr config dir {}: {error}",
                parent.display()
            ))
        })?;
    }
    std::fs::write(path, mapped_herdr_client_config()).map_err(|error| {
        HerdrHostError::unavailable(format!(
            "failed to write mapped Herdr config {}: {error}",
            path.display()
        ))
    })?;
    Ok(path.to_path_buf())
}

async fn run_herdr(binary: &Path, args: &[&str]) -> Result<String, HerdrHostError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(windows_herdr_creation_flags());
    let output = command.output().await.map_err(|error| {
        HerdrHostError::unavailable(format!("failed to spawn {}: {error}", binary.display()))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HerdrHostError::unavailable(format!(
            "herdr {} failed: {}",
            args.join(" "),
            stderr.trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| {
        HerdrHostError::unavailable(format!("herdr stdout was not UTF-8: {error}"))
    })
}

#[async_trait]
impl HerdrHost for HerdrCliHost {
    async fn list_workspaces(&self) -> Result<Vec<HerdrWorkspaceInfo>, HerdrHostError> {
        let stdout = run_herdr(&self.binary, &["workspace", "list"]).await?;
        parse_workspace_list(&stdout)
    }

    async fn list_panes(&self) -> Result<Vec<HerdrPaneInfo>, HerdrHostError> {
        let stdout = run_herdr(&self.binary, &["pane", "list"]).await?;
        parse_pane_list(&stdout)
    }
}

#[derive(Debug, Deserialize)]
struct AgentListResult {
    agents: Vec<CliAgent>,
}

#[derive(Debug, Deserialize)]
struct CliAgent {
    workspace_id: String,
    pane_id: String,
    agent: String,
    #[serde(default, alias = "agent_status")]
    status: Option<String>,
    #[serde(default)]
    focused: bool,
}

pub fn parse_agent_list(stdout: &str) -> Result<Vec<HerdrAgentInfo>, HerdrHostError> {
    let envelope: CliEnvelope<AgentListResult> = serde_json::from_str(stdout).map_err(|error| {
        HerdrHostError::unavailable(format!("invalid herdr agent list JSON: {error}"))
    })?;
    Ok(envelope
        .result
        .agents
        .into_iter()
        .map(|agent| HerdrAgentInfo {
            workspace_id: agent.workspace_id,
            pane_id: agent.pane_id,
            agent: agent.agent,
            status: agent.status.unwrap_or_default(),
            focused: agent.focused,
        })
        .collect())
}

fn agent_read_args(target: &str) -> Vec<String> {
    vec![
        "agent".into(),
        "read".into(),
        target.into(),
        "--source".into(),
        "recent".into(),
        "--lines".into(),
        AGENT_TRANSCRIPT_LINES.to_string(),
        "--format".into(),
        "text".into(),
    ]
}

fn agent_send_keys_args(target: &str, keys: &[&str]) -> Vec<String> {
    let mut args = vec!["agent".into(), "send-keys".into(), target.into()];
    args.extend(keys.iter().map(|key| (*key).to_string()));
    args
}

#[async_trait]
impl HerdrAgentBridge for HerdrCliHost {
    async fn list_agents(&self) -> Result<Vec<HerdrAgentInfo>, HerdrHostError> {
        let stdout = run_herdr(&self.binary, &["agent", "list"]).await?;
        parse_agent_list(&stdout)
    }

    async fn prompt_agent(&self, target: &str, text: &str) -> Result<(), HerdrHostError> {
        let _ = run_herdr(&self.binary, &["agent", "prompt", target, text]).await?;
        Ok(())
    }

    async fn read_agent(&self, target: &str) -> Result<String, HerdrHostError> {
        let args = agent_read_args(target);
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        run_herdr(&self.binary, &argv).await
    }

    async fn send_agent_keys(&self, target: &str, keys: &[&str]) -> Result<(), HerdrHostError> {
        let args = agent_send_keys_args(target, keys);
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let _ = run_herdr(&self.binary, &argv).await?;
        Ok(())
    }
}

pub struct PtyHerdrClientFactory {
    binary: PathBuf,
}

impl PtyHerdrClientFactory {
    pub fn from_env() -> Self {
        Self {
            binary: std::env::var_os("HERDR_BIN_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("herdr")),
        }
    }
}

struct PtyHerdrClientSession {
    writer: std::sync::Mutex<Box<dyn std::io::Write + Send>>,
    master: std::sync::Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    output: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>,
    child: std::sync::Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

#[async_trait]
impl HerdrClientSession for PtyHerdrClientSession {
    async fn write(&self, bytes: &[u8]) -> Result<(), HerdrHostError> {
        let bytes = bytes.to_vec();
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        use std::io::Write;
        writer
            .write_all(&bytes)
            .and_then(|_| writer.flush())
            .map_err(|error| {
                HerdrHostError::unavailable(format!("herdr client write failed: {error}"))
            })
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<(), HerdrHostError> {
        self.master
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                HerdrHostError::unavailable(format!("herdr client resize failed: {error}"))
            })
    }

    async fn read(&self) -> Result<Option<Vec<u8>>, HerdrHostError> {
        Ok(self.output.lock().await.recv().await)
    }

    async fn close(&self) -> Result<(), HerdrHostError> {
        let mut child = self.child.lock().unwrap_or_else(|error| error.into_inner());
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }
}

#[async_trait]
impl HerdrClientFactory for PtyHerdrClientFactory {
    async fn attach(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<std::sync::Arc<dyn HerdrClientSession>, HerdrHostError> {
        let binary = self.binary.clone();
        tokio::task::spawn_blocking(move || {
            let system = portable_pty::native_pty_system();
            let pair = system
                .openpty(portable_pty::PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|error| {
                    HerdrHostError::unavailable(format!("failed to open ConPTY: {error}"))
                })?;
            let mut command = portable_pty::CommandBuilder::new(&binary);
            for arg in herdr_client_attach_args() {
                command.arg(arg);
            }
            command.env("TERM", "xterm-256color");
            command.env("COLORTERM", "truecolor");
            let config_path = write_mapped_herdr_client_config(&mapped_herdr_client_config_path())?;
            command.env("HERDR_CONFIG_PATH", config_path.as_os_str());
            let child = pair.slave.spawn_command(command).map_err(|error| {
                HerdrHostError::unavailable(format!(
                    "failed to spawn {} in ConPTY: {error}",
                    binary.display()
                ))
            })?;
            let mut reader = pair.master.try_clone_reader().map_err(|error| {
                HerdrHostError::unavailable(format!("failed to clone ConPTY reader: {error}"))
            })?;
            let writer = pair.master.take_writer().map_err(|error| {
                HerdrHostError::unavailable(format!("failed to take ConPTY writer: {error}"))
            })?;
            let (tx, rx) = tokio::sync::mpsc::channel(32);
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buffer = [0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.blocking_send(buffer[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            Ok(std::sync::Arc::new(PtyHerdrClientSession {
                writer: std::sync::Mutex::new(writer),
                master: std::sync::Mutex::new(pair.master),
                output: tokio::sync::Mutex::new(rx),
                child: std::sync::Mutex::new(child),
            }) as std::sync::Arc<dyn HerdrClientSession>)
        })
        .await
        .map_err(|error| {
            HerdrHostError::unavailable(format!("ConPTY attach task failed: {error}"))
        })?
    }
}

pub fn herdr_socket_path() -> PathBuf {
    std::env::var_os("HERDR_SOCKET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("herdr")
                .join("herdr.sock")
        })
}

pub fn windows_named_pipe_path(socket_path: &Path) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\{}", socket_path.display()))
}

#[derive(Debug, Deserialize)]
struct SocketResultEnvelope {
    #[serde(default)]
    result: Option<SocketResultBody>,
    #[serde(default)]
    error: Option<SocketErrorBody>,
}

#[derive(Debug, Deserialize)]
struct SocketResultBody {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct SocketErrorBody {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SocketEventEnvelope {
    event: String,
}

pub fn parse_subscription_ack(line: &str) -> Result<(), HerdrHostError> {
    let envelope: SocketResultEnvelope = serde_json::from_str(line).map_err(|error| {
        HerdrHostError::unavailable(format!("invalid herdr subscribe ack: {error}"))
    })?;
    if let Some(error) = envelope.error {
        return Err(HerdrHostError::unavailable(
            error
                .message
                .unwrap_or_else(|| "herdr subscribe failed".into()),
        ));
    }
    match envelope.result {
        Some(result) if result.kind == "subscription_started" => Ok(()),
        Some(result) => Err(HerdrHostError::unavailable(format!(
            "unexpected herdr subscribe ack: {}",
            result.kind
        ))),
        None => Err(HerdrHostError::unavailable(
            "herdr subscribe ack missing result",
        )),
    }
}

pub fn parse_lifecycle_event(line: &str) -> Result<Option<HerdrLifecycleEvent>, HerdrHostError> {
    let envelope: SocketEventEnvelope = serde_json::from_str(line).map_err(|error| {
        HerdrHostError::unavailable(format!("invalid herdr event JSON: {error}"))
    })?;
    Ok(match envelope.event.as_str() {
        "workspace_created" => Some(HerdrLifecycleEvent::WorkspaceCreated),
        "workspace_closed" => Some(HerdrLifecycleEvent::WorkspaceClosed),
        "pane_created" => Some(HerdrLifecycleEvent::PaneCreated),
        "pane_updated" => Some(HerdrLifecycleEvent::PaneUpdated),
        _ => None,
    })
}

type HerdrPipeReader = tokio::io::BufReader<tokio::net::windows::named_pipe::NamedPipeClient>;

pub struct HerdrNamedPipeEventSource {
    socket_path: PathBuf,
    reader: tokio::sync::Mutex<Option<HerdrPipeReader>>,
}

impl HerdrNamedPipeEventSource {
    pub fn from_env() -> Self {
        Self {
            socket_path: herdr_socket_path(),
            reader: tokio::sync::Mutex::new(None),
        }
    }

    async fn ensure_reader(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<HerdrPipeReader>>, HerdrHostError> {
        let mut guard = self.reader.lock().await;
        if guard.is_none() {
            let pipe = windows_named_pipe_path(&self.socket_path);
            let client = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(&pipe)
                .map_err(|error| {
                    HerdrHostError::unavailable(format!(
                        "failed to open Herdr named pipe {}: {error}",
                        pipe.display()
                    ))
                })?;
            let mut reader = tokio::io::BufReader::new(client);
            use tokio::io::AsyncWriteExt;
            let request = serde_json::json!({
                "id": "workbench:events:subscribe",
                "method": "events.subscribe",
                "params": {
                    "subscriptions": [
                        {"type": "workspace.created"},
                        {"type": "workspace.closed"},
                        {"type": "pane.created"},
                        {"type": "pane.updated"}
                    ]
                }
            });
            reader
                .get_mut()
                .write_all(format!("{request}\n").as_bytes())
                .await
                .map_err(|error| {
                    HerdrHostError::unavailable(format!(
                        "failed to subscribe to Herdr events: {error}"
                    ))
                })?;
            let mut ack = String::new();
            use tokio::io::AsyncBufReadExt;
            reader.read_line(&mut ack).await.map_err(|error| {
                HerdrHostError::unavailable(format!("failed to read Herdr subscribe ack: {error}"))
            })?;
            parse_subscription_ack(ack.trim_end())?;
            *guard = Some(reader);
        }
        Ok(guard)
    }
}

#[async_trait]
impl HerdrEventSource for HerdrNamedPipeEventSource {
    async fn next_event(&self) -> Result<HerdrLifecycleEvent, HerdrHostError> {
        loop {
            let mut guard = self.ensure_reader().await?;
            let reader = guard.as_mut().expect("connected herdr pipe");
            let mut line = String::new();
            use tokio::io::AsyncBufReadExt;
            let read = reader.read_line(&mut line).await.map_err(|error| {
                HerdrHostError::unavailable(format!("failed to read Herdr event: {error}"))
            })?;
            if read == 0 {
                *guard = None;
                return Err(HerdrHostError::unavailable("Herdr event pipe closed"));
            }
            if let Some(event) = parse_lifecycle_event(line.trim_end())? {
                return Ok(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_read_args, agent_send_keys_args, herdr_client_attach_args,
        mapped_herdr_client_config, parse_agent_list, parse_lifecycle_event, parse_pane_list,
        parse_subscription_ack, parse_workspace_list, windows_herdr_creation_flags,
        windows_named_pipe_path,
    };
    use herdr_workbench_app_core::HerdrLifecycleEvent;
    use std::path::{Path, PathBuf};

    #[test]
    fn parse_workspace_list_reads_cli_envelope() {
        let stdout = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"wD:t1","agent_status":"idle","focused":true,"label":"code","number":1,"pane_count":1,"tab_count":1,"workspace_id":"wD"},{"workspace_id":"wT","label":"tree","worktree":{"checkout_path":"C:\\projects\\siftmark"}}]}}"#;
        let workspaces = parse_workspace_list(stdout).unwrap();
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].workspace_id, "wD");
        assert_eq!(workspaces[0].label, "code");
        assert_eq!(workspaces[0].worktree_checkout_path, None);
        assert_eq!(
            workspaces[1].worktree_checkout_path,
            Some(PathBuf::from(r"C:\projects\siftmark"))
        );
    }

    #[test]
    fn parse_pane_list_reads_cwd_and_focus() {
        let stdout = r#"{"id":"cli:pane:list","result":{"type":"pane_list","panes":[{"workspace_id":"wD","cwd":"D:\\Code\\huajingweb","focused":false},{"workspace_id":"w9","cwd":"F:\\github\\QuickPane","focused":true}]}}"#;
        let panes = parse_pane_list(stdout).unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].cwd, Some(PathBuf::from(r"D:\Code\huajingweb")));
        assert!(!panes[0].focused);
        assert_eq!(panes[1].cwd, Some(PathBuf::from(r"F:\github\QuickPane")));
        assert!(panes[1].focused);
    }

    #[test]
    fn parse_workspace_list_rejects_a_bare_array() {
        let error = parse_workspace_list(r#"[{"workspace_id":"wD"}]"#).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid herdr workspace list JSON")
        );
    }

    #[test]
    fn windows_pipe_path_prefixes_the_filesystem_socket_path() {
        let socket = Path::new(r"C:\Users\1\AppData\Roaming\herdr\herdr.sock");
        assert_eq!(
            windows_named_pipe_path(socket),
            PathBuf::from(r"\\.\pipe\C:\Users\1\AppData\Roaming\herdr\herdr.sock")
        );
    }

    #[test]
    fn parse_subscription_ack_accepts_subscription_started() {
        parse_subscription_ack(r#"{"id":"sub_ws","result":{"type":"subscription_started"}}"#)
            .unwrap();
    }

    #[test]
    fn parse_subscription_ack_rejects_errors() {
        let error = parse_subscription_ack(
            r#"{"id":"sub_ws","error":{"code":"invalid_params","message":"bad subscription"}}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("bad subscription"));
    }

    #[test]
    fn parse_lifecycle_event_reads_workspace_created() {
        let event = parse_lifecycle_event(
            r#"{"event":"workspace_created","data":{"type":"workspace_created","workspace":{"workspace_id":"wZ"}}}"#,
        )
        .unwrap();
        assert_eq!(event, Some(HerdrLifecycleEvent::WorkspaceCreated));
    }

    #[test]
    fn parse_lifecycle_event_ignores_unrelated_kinds() {
        let event = parse_lifecycle_event(
            r#"{"event":"workspace_focused","data":{"type":"workspace_focused","workspace_id":"wD"}}"#,
        )
        .unwrap();
        assert_eq!(event, None);
    }

    #[test]
    fn parse_agent_list_reads_cli_envelope() {
        let stdout = r#"{"id":"cli:agent:list","result":{"type":"agent_list","agents":[{"agent":"pi","agent_status":"idle","focused":true,"pane_id":"w9:p3S","workspace_id":"w9"},{"agent":"claude","agent_status":"working","focused":false,"pane_id":"wD:p1","workspace_id":"wD"}]}}"#;
        let agents = parse_agent_list(stdout).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].pane_id, "w9:p3S");
        assert_eq!(agents[0].status, "idle");
        assert!(agents[0].focused);
        assert_eq!(agents[1].agent, "claude");
        assert!(!agents[1].focused);
    }

    #[test]
    fn agent_read_uses_recent_plain_text_argv() {
        assert_eq!(
            agent_read_args("wD:p3M"),
            vec![
                "agent", "read", "wD:p3M", "--source", "recent", "--lines", "80", "--format",
                "text",
            ]
        );
    }

    #[test]
    fn agent_send_keys_appends_yes_or_no_without_a_shell() {
        assert_eq!(
            agent_send_keys_args("wD:p3M", &["y", "enter"]),
            vec!["agent", "send-keys", "wD:p3M", "y", "enter"]
        );
        assert_eq!(
            agent_send_keys_args("wD:p3M", &["n", "enter"]),
            vec!["agent", "send-keys", "wD:p3M", "n", "enter"]
        );
    }

    #[test]
    fn windows_herdr_spawn_hides_the_console() {
        assert_eq!(windows_herdr_creation_flags(), 0x0800_0000);
    }

    #[test]
    fn herdr_client_attach_runs_herdr_with_no_extra_args() {
        assert!(herdr_client_attach_args().is_empty());
    }

    #[test]
    fn mapped_herdr_client_config_disables_kitty_graphics() {
        let config = mapped_herdr_client_config();
        assert!(config.contains("kitty_graphics = false"));
        assert!(config.contains("redraw_on_focus_gained = false"));
        assert!(!config.contains("kitty_graphics = true"));
    }

    #[test]
    fn write_mapped_herdr_client_config_does_not_touch_user_herdr_config() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-workbench-mapped-config-{}",
            std::process::id()
        ));
        let path = dir.join("mapped-herdr-client.toml");
        let written = super::write_mapped_herdr_client_config(&path).unwrap();
        let body = std::fs::read_to_string(&written).unwrap();
        assert_eq!(body, mapped_herdr_client_config());
        assert!(
            !written
                .to_string_lossy()
                .replace('\\', "/")
                .contains("/herdr/config.toml")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
