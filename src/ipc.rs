use std::{
    env, fs,
    io::{Read, Write},
    net::Shutdown,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Reload,
    Status,
    Quit,
    TrayList,
    TrayMenuOpen { index: usize },
    TrayTooltipOpen { index: usize },
    TrayMenuClick { token: u64, node_id: i32 },
}

impl Request {
    pub fn encode(self) -> String {
        match self {
            Self::Reload => "reload\n".into(),
            Self::Status => "status\n".into(),
            Self::Quit => "quit\n".into(),
            Self::TrayList => "tray-list\n".into(),
            Self::TrayMenuOpen { index } => format!("tray-menu-open {index}\n"),
            Self::TrayTooltipOpen { index } => format!("tray-tooltip-open {index}\n"),
            Self::TrayMenuClick { token, node_id } => {
                format!("tray-menu-click {token} {node_id}\n")
            }
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?.trim();
        match text {
            "reload" => Some(Self::Reload),
            "status" => Some(Self::Status),
            "quit" => Some(Self::Quit),
            "tray-list" => Some(Self::TrayList),
            _ => {
                let mut fields = text.split_whitespace();
                match fields.next()? {
                    "tray-menu-open" => {
                        let index = fields.next()?.parse().ok()?;
                        if fields.next().is_some() {
                            return None;
                        }
                        Some(Self::TrayMenuOpen { index })
                    }
                    "tray-tooltip-open" => {
                        let index = fields.next()?.parse().ok()?;
                        if fields.next().is_some() {
                            return None;
                        }
                        Some(Self::TrayTooltipOpen { index })
                    }
                    "tray-menu-click" => {
                        let token = fields.next()?.parse().ok()?;
                        let node_id = fields.next()?.parse().ok()?;
                        if fields.next().is_some() {
                            return None;
                        }
                        Some(Self::TrayMenuClick { token, node_id })
                    }
                    _ => None,
                }
            }
        }
    }
}

pub fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime_dir).join("mhyprbar.sock"))
}

pub fn request(request: Request) -> Result<String> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("failed to connect to {}", path.display()))?;
    let encoded = request.encode();
    stream
        .write_all(encoded.as_bytes())
        .with_context(|| format!("failed to write to {}", path.display()))?;
    stream
        .shutdown(Shutdown::Write)
        .with_context(|| format!("failed to finish request to {}", path.display()))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .with_context(|| format!("failed to read from {}", path.display()))?;
    Ok(response)
}

pub fn write_response(stream: &mut UnixStream, response: &str) -> Result<()> {
    stream
        .write_all(response.as_bytes())
        .context("failed to write control response")
}

pub fn bind_listener() -> Result<(UnixListener, SocketGuard)> {
    let path = socket_path()?;

    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!("mhyprbar is already running");
        }

        fs::remove_file(&path)
            .with_context(|| format!("failed to remove stale {}", path.display()))?;
    }

    let listener =
        UnixListener::bind(&path).with_context(|| format!("failed to bind {}", path.display()))?;
    listener
        .set_nonblocking(true)
        .context("failed to make control socket nonblocking")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;

    Ok((listener, SocketGuard(path)))
}

pub fn read_request(stream: &mut UnixStream) -> Result<Option<Request>> {
    let mut buffer = [0_u8; 128];
    let size = stream
        .read(&mut buffer)
        .context("failed to read control request")?;
    Ok(Request::parse(&buffer[..size]))
}

pub struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::Request;

    #[test]
    fn parses_control_requests() {
        assert_eq!(Request::parse(b"reload\n"), Some(Request::Reload));
        assert_eq!(Request::parse(b" status \n"), Some(Request::Status));
        assert_eq!(Request::parse(b"quit"), Some(Request::Quit));
        assert_eq!(Request::parse(b"tray-list"), Some(Request::TrayList));
        assert_eq!(
            Request::parse(b"tray-menu-open 3\n"),
            Some(Request::TrayMenuOpen { index: 3 })
        );
        assert_eq!(
            Request::parse(b"tray-tooltip-open 2\n"),
            Some(Request::TrayTooltipOpen { index: 2 })
        );
        assert_eq!(
            Request::parse(b"tray-menu-click 42 -7\n"),
            Some(Request::TrayMenuClick {
                token: 42,
                node_id: -7,
            })
        );
        assert_eq!(Request::parse(b"unknown\n"), None);
    }

    #[test]
    fn round_trips_tray_menu_click() {
        let request = Request::TrayMenuClick {
            token: 123,
            node_id: 9,
        };
        assert_eq!(Request::parse(request.encode().as_bytes()), Some(request));
    }
}
