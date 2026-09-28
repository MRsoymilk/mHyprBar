use std::{
    collections::HashMap,
    env,
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
struct MonitorJson {
    id: i32,
    name: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    height: i32,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WorkspaceRefJson,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkspaceRefJson {
    id: i32,
}

#[cfg(mhypr_module = "active_window")]
#[derive(Clone, Debug, Deserialize, Default)]
struct ActiveWindowJson {
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
}

#[cfg(mhypr_module = "active_window")]
#[derive(Clone, Debug)]
pub struct ActiveWindow {
    pub class: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkspaceJson {
    id: i32,
    #[serde(default)]
    windows: i32,
    #[serde(rename = "monitorID", default)]
    monitor_id: i32,
    #[serde(default)]
    monitor: String,
}

#[derive(Clone, Debug)]
pub struct MonitorState {
    pub id: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub height: i32,
    pub active_workspace: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    monitors: Vec<MonitorState>,
    workspace_windows: HashMap<i32, u32>,
}

impl Snapshot {
    pub fn refresh() -> Result<Self> {
        let monitors: Vec<MonitorJson> = serde_json::from_str(request("j/monitors")?.trim())
            .context("invalid Hyprland monitors JSON")?;
        let workspaces: Vec<WorkspaceJson> = serde_json::from_str(request("j/workspaces")?.trim())
            .context("invalid Hyprland workspaces JSON")?;

        let monitors = monitors
            .into_iter()
            .map(|monitor| MonitorState {
                id: monitor.id,
                name: monitor.name,
                x: monitor.x,
                y: monitor.y,
                height: monitor.height,
                active_workspace: monitor.active_workspace.id,
            })
            .collect();

        let workspace_windows = workspaces
            .into_iter()
            .filter(|workspace| workspace.id > 0)
            .map(|workspace| {
                let _ = (workspace.monitor_id, workspace.monitor);
                (workspace.id, workspace.windows.max(0) as u32)
            })
            .collect();

        Ok(Self {
            monitors,
            workspace_windows,
        })
    }

    pub fn monitor_by_name(&self, name: &str) -> Option<&MonitorState> {
        self.monitors.iter().find(|monitor| monitor.name == name)
    }

    pub fn monitor_by_index(&self, index: usize) -> Option<&MonitorState> {
        let mut monitors = self.monitors.iter().collect::<Vec<_>>();
        monitors.sort_by_key(|monitor| monitor.id);
        monitors.get(index).copied()
    }

    pub fn workspace_windows(&self, workspace_id: i32) -> u32 {
        self.workspace_windows
            .get(&workspace_id)
            .copied()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct EventBatch {
    pub state_changed: bool,
    pub active_window_changed: bool,
}

#[cfg(mhypr_module = "active_window")]
pub fn active_window() -> Result<ActiveWindow> {
    let raw = request("j/activewindow")?;
    let window: ActiveWindowJson =
        serde_json::from_str(raw.trim()).context("invalid Hyprland activewindow JSON")?;
    Ok(ActiveWindow {
        class: window.class,
        title: window.title,
    })
}

pub fn event_stream() -> Result<UnixStream> {
    let stream = UnixStream::connect(socket_path(".socket2.sock")?)
        .context("failed to connect Hyprland event socket")?;
    stream
        .set_nonblocking(true)
        .context("failed to make Hyprland event socket nonblocking")?;
    Ok(stream)
}

pub fn read_event_batch(stream: &UnixStream, buffer: &mut String) -> Result<EventBatch> {
    let mut batch = EventBatch::default();
    let mut bytes = [0_u8; 4096];
    let mut reader = stream;

    loop {
        match reader.read(&mut bytes) {
            Ok(0) => bail!("Hyprland event socket closed"),
            Ok(count) => buffer.push_str(&String::from_utf8_lossy(&bytes[..count])),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error).context("failed to read Hyprland event socket"),
        }
    }

    while let Some(newline) = buffer.find('\n') {
        let line = buffer[..newline].trim_end_matches('\r').to_owned();
        buffer.drain(..=newline);
        let event = line.split_once(">>").map(|(event, _)| event).unwrap_or("");
        match event {
            "workspace" | "workspacev2" | "focusedmon" | "focusedmonv2" | "createworkspace"
            | "createworkspacev2" | "destroyworkspace" | "destroyworkspacev2" | "moveworkspace"
            | "moveworkspacev2" | "monitoradded" | "monitoraddedv2" | "monitorremoved"
            | "monitorremovedv2" | "configreloaded" => batch.state_changed = true,
            "activewindow" | "activewindowv2" | "windowtitle" | "windowtitlev2" => {
                batch.active_window_changed = true
            }
            _ => {}
        }
    }

    Ok(batch)
}

pub fn switch_workspace(monitor_name: &str, workspace_id: i32) -> Result<()> {
    let monitor = lua_quote(monitor_name);
    let lua = format!(
        "hl.dispatch(hl.dsp.focus({{ monitor = {monitor} }})); hl.dispatch(hl.dsp.focus({{ workspace = \"{workspace_id}\", on_current_monitor = true }}))"
    );

    match request(&format!("eval {lua}")) {
        Ok(response) if command_succeeded(&response) => Ok(()),
        _ => {
            let focus = request(&format!("dispatch focusmonitor {monitor_name}"))?;
            if !command_succeeded(&focus) {
                bail!("Hyprland rejected focusmonitor request: {}", focus.trim());
            }
            let workspace = request(&format!("dispatch workspace {workspace_id}"))?;
            if !command_succeeded(&workspace) {
                bail!("Hyprland rejected workspace request: {}", workspace.trim());
            }
            Ok(())
        }
    }
}

fn command_succeeded(response: &str) -> bool {
    let response = response.trim();
    response.is_empty() || response == "ok" || response.ends_with("\nok")
}

fn lua_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn request(command: &str) -> Result<String> {
    let mut stream = UnixStream::connect(socket_path(".socket.sock")?)
        .context("failed to connect Hyprland IPC")?;
    let timeout = Some(Duration::from_millis(500));
    stream
        .set_read_timeout(timeout)
        .context("failed to set Hyprland IPC read timeout")?;
    stream
        .set_write_timeout(timeout)
        .context("failed to set Hyprland IPC write timeout")?;
    stream
        .write_all(command.as_bytes())
        .context("failed to write Hyprland IPC request")?;
    stream
        .shutdown(Shutdown::Write)
        .context("failed to finish Hyprland IPC request")?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("failed to read Hyprland IPC response")?;
    Ok(response)
}

fn socket_path(socket_name: &str) -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .context("HYPRLAND_INSTANCE_SIGNATURE is not set")?;

    Ok(PathBuf::from(runtime_dir)
        .join("hypr")
        .join(signature)
        .join(socket_name))
}

#[cfg(test)]
mod tests {
    use super::lua_quote;

    #[test]
    fn quotes_monitor_name_for_lua() {
        assert_eq!(lua_quote("DP-1"), "\"DP-1\"");
        assert_eq!(lua_quote("a\\b\"c"), "\"a\\\\b\\\"c\"");
    }
}
