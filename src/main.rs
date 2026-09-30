#[cfg(mhypr_module = "active_window")]
mod active_window_popup;
#[cfg(mhypr_module = "audio")]
mod audio_popup;
#[cfg(mhypr_module = "battery")]
mod battery_popup;
#[cfg(mhypr_module = "clock")]
mod clock_popup;
mod config;
#[cfg(mhypr_module = "cpu")]
mod cpu_popup;
#[cfg(mhypr_module = "disk")]
mod disk_popup;
#[cfg(mhypr_module = "gpu")]
mod gpu;
#[cfg(mhypr_module = "gpu")]
mod gpu_popup;
mod hyprland;
mod ipc;
#[cfg(mhypr_module = "memory")]
mod memory_popup;
mod modules;
#[cfg(mhypr_module = "monitor")]
mod monitor_popup;
#[cfg(mhypr_module = "network")]
mod network_popup;
mod render;
mod tray;
#[cfg(mhypr_module = "tray")]
mod tray_popup;
mod wayland;

use std::{collections::HashSet, env};

use anyhow::{Context, Result, bail};

use crate::config::BarConfig;

const NAME: &str = "mHyprBar";
#[cfg(mhypr_module = "active_window")]
mod window_icon;

fn print_help() {
    println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
    println!("Native Wayland status bar for Hyprland.");
    println!();
    println!("Usage:");
    println!("  mhyprbar                 run the bar");
    println!("  mhyprbar --reload        reload the running bar configuration");
    println!("  mhyprbar --status        show status of the running bar");
    println!("  mhyprbar --cpu-popup     toggle CPU details popup (debug)");
    println!("  mhyprbar --tray-list     list current tray items");
    println!("  mhyprbar --tray-menu N   open tray item N menu (debug)");
    println!("  mhyprbar --tray-tooltip N show tray item N tooltip (debug)");
    println!("  mhyprbar --quit          stop the running bar");
    println!("  mhyprbar --list-modules  list modules compiled into this binary");
    println!("  mhyprbar --check-config  validate bar.toml and all compiled module configs");
    println!("  mhyprbar --version       print version");
}

fn print_modules() {
    for module in modules::compiled() {
        println!("{}\t{}", module.name, module.config_file);
    }
}

fn validate_layout(config: &BarConfig) -> Result<()> {
    let compiled: HashSet<&str> = modules::compiled()
        .into_iter()
        .map(|module| module.name)
        .collect();
    for name in config.module_order() {
        if !compiled.contains(name) {
            bail!("bar config references module {name:?}, but it is not compiled into this binary");
        }
    }
    Ok(())
}

pub(crate) fn validate_config() -> Result<BarConfig> {
    let config = BarConfig::load()?;
    validate_layout(&config)?;
    modules::validate_configs()?;
    Ok(config)
}

fn run_control(request: ipc::Request) -> Result<()> {
    let response = ipc::request(request)?;
    if let Some(error) = response.strip_prefix("error: ") {
        bail!("{}", error.trim());
    }
    print!("{response}");
    Ok(())
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next();
    match command.as_deref() {
        Some("-h" | "--help") => print_help(),
        Some("-V" | "--version") => println!("mhyprbar {}", env!("CARGO_PKG_VERSION")),
        Some("--reload") => run_control(ipc::Request::Reload)?,
        Some("--status") => run_control(ipc::Request::Status)?,
        Some("--cpu-popup") => run_control(ipc::Request::CpuPopupToggle)?,
        Some("--tray-list") => run_control(ipc::Request::TrayList)?,
        Some("--tray-menu") => {
            let index = args
                .next()
                .context("--tray-menu requires an index")?
                .parse()
                .context("invalid tray index")?;
            if args.next().is_some() {
                bail!("too many arguments for --tray-menu");
            }
            run_control(ipc::Request::TrayMenuOpen { index })?;
        }
        Some("--tray-tooltip") => {
            let index = args
                .next()
                .context("--tray-tooltip requires an index")?
                .parse()
                .context("invalid tray index")?;
            if args.next().is_some() {
                bail!("too many arguments for --tray-tooltip");
            }
            run_control(ipc::Request::TrayTooltipOpen { index })?;
        }
        Some("--quit") => run_control(ipc::Request::Quit)?,
        Some("--list-modules") => print_modules(),
        Some("--check-config") => {
            validate_config()?;
            println!("bar and compiled module configs are valid");
        }
        Some(arg) => {
            eprintln!("unknown argument: {arg}");
            eprintln!("try 'mhyprbar --help'");
            std::process::exit(2);
        }
        None => {
            let config = validate_config()?;
            wayland::run(config)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("mhyprbar: {error:#}");
        std::process::exit(1);
    }
}
