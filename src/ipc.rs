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
}

impl Request {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Reload => b"reload\n",
            Self::Status => b"status\n",
            Self::Quit => b"quit\n",
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match std::str::from_utf8(bytes).ok()?.trim() {
            "reload" => Some(Self::Reload),
            "status" => Some(Self::Status),
            "quit" => Some(Self::Quit),
            _ => None,
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
    stream
        .write_all(request.as_bytes())
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
    let mut buffer = [0_u8; 64];
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
        assert_eq!(Request::parse(b"unknown\n"), None);
    }
}
