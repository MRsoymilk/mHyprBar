use std::{
    env,
    io::Write,
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "menu";
pub const CONFIG_FILE: &str = "modules/menu.toml";

const ICON_MENU_PNG: &[u8] = include_bytes!("../../../res/menu/icon.png");

#[derive(Debug, Deserialize)]
struct MenuConfig {
    #[serde(default = "default_label")]
    label: String,
    #[serde(default = "default_command")]
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_icon_scale")]
    icon_scale: f32,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone)]
struct MenuIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
pub struct MenuVisual {
    pub icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
    pub icon_scale: f32,
}

pub struct MenuModule {
    config: MenuConfig,
    icon: MenuIcon,
}

impl MenuModule {
    pub fn load() -> Result<Self> {
        let config: MenuConfig = config::load_module(NAME)?;
        ensure!(!config.label.is_empty(), "menu label must not be empty");
        ensure!(!config.command.is_empty(), "menu command must not be empty");
        ensure!(
            config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
            "menu icon_scale must be in 0.1..=1.0"
        );
        config.style.validate()?;
        let icon = rasterize_png("icon.png", ICON_MENU_PNG)?;
        Ok(Self { config, icon })
    }

    fn send_daemon(&self, position: Option<(f64, f64)>) -> Result<()> {
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
        let path = PathBuf::from(runtime_dir).join("mhyprmenu.sock");
        let mut stream = UnixStream::connect(&path)
            .with_context(|| format!("failed to connect to {}", path.display()))?;
        let request = match position {
            Some((x, y)) => format!("popup-at {x:.3} {y:.3}\n"),
            None => "popup\n".into(),
        };
        stream
            .write_all(request.as_bytes())
            .with_context(|| format!("failed to write to {}", path.display()))
    }

    fn spawn(&self, position: Option<(f64, f64)>) -> Result<()> {
        let mut command = Command::new(&self.config.command);
        command.args(&self.config.args);
        if let Some((x, y)) = position {
            command
                .arg("--popup-at")
                .arg(format!("{x:.3}"))
                .arg(format!("{y:.3}"));
        }
        command
            .spawn()
            .with_context(|| format!("failed to launch {}", self.config.command))?;
        Ok(())
    }

    fn open(&self, position: Option<(f64, f64)>) -> Result<()> {
        if self.send_daemon(position).is_ok() {
            return Ok(());
        }
        self.spawn(position)
    }
}

impl StatusModule for MenuModule {
    fn name(&self) -> &'static str {
        NAME
    }

    fn interval(&self) -> Option<Duration> {
        None
    }

    fn style(&self) -> &ModuleStyle {
        &self.config.style
    }

    fn sample(&mut self) -> Result<String> {
        Ok(self.config.label.clone())
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Menu(MenuVisual {
            icon_pixels: Arc::clone(&self.icon.pixels),
            icon_width: self.icon.width,
            icon_height: self.icon.height,
            icon_scale: self.config.icon_scale,
        })
    }

    fn activate(&mut self) -> Result<bool> {
        self.open(None)?;
        Ok(false)
    }

    fn activate_at(&mut self, x: f64, y: f64) -> Result<bool> {
        self.open(Some((x, y)))?;
        Ok(false)
    }
}

fn rasterize_png(name: &str, png_bytes: &[u8]) -> Result<MenuIcon> {
    const RASTER_SIZE: i32 = 96;

    let mut magick = Command::new("magick")
        .arg("png:-")
        .arg("-resize")
        .arg(format!("{RASTER_SIZE}x{RASTER_SIZE}"))
        .arg("-background")
        .arg("none")
        .arg("-gravity")
        .arg("center")
        .arg("-extent")
        .arg(format!("{RASTER_SIZE}x{RASTER_SIZE}"))
        .arg("-depth")
        .arg("8")
        .arg("-alpha")
        .arg("on")
        .arg("rgba:-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch magick for {name}"))?;

    let mut stdin = magick.stdin.take().context("failed to open magick stdin")?;
    stdin
        .write_all(png_bytes)
        .with_context(|| format!("failed to feed PNG data for {name}"))?;
    drop(stdin);

    let output = magick
        .wait_with_output()
        .with_context(|| format!("failed to wait for magick for {name}"))?;
    if !output.status.success() {
        bail!("menu PNG rasterization failed for {name}");
    }

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "menu PNG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    Ok(MenuIcon {
        pixels: Arc::from(output.stdout),
        width: RASTER_SIZE,
        height: RASTER_SIZE,
    })
}

fn default_label() -> String {
    "Menu".into()
}

fn default_command() -> String {
    "mhyprmenu".into()
}

fn default_icon_scale() -> f32 {
    0.82
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{MenuConfig, MenuIcon, MenuModule};
    use crate::config::ModuleStyle;

    #[test]
    fn spawns_configured_command_without_shell() {
        let module = MenuModule {
            config: MenuConfig {
                label: "Menu".into(),
                command: "/bin/true".into(),
                args: Vec::new(),
                icon_scale: 0.82,
                style: ModuleStyle::default(),
            },
            icon: MenuIcon {
                pixels: Arc::from(vec![255_u8; 4]),
                width: 1,
                height: 1,
            },
        };
        module.spawn(None).unwrap();
    }
}
