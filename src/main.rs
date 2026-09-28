mod config;
mod hyprland;
mod ipc;
mod modules;
mod render;
mod wayland;

use std::{collections::HashSet, env};

use anyhow::{Result, bail};

use crate::config::BarConfig;

const NAME: &str = "mHyprBar";

fn print_help() {
    println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
    println!("Native Wayland status bar for Hyprland.");
    println!();
    println!("Usage:");
    println!("  mhyprbar                 run the bar");
    println!("  mhyprbar --reload        reload the running bar configuration");
    println!("  mhyprbar --status        show status of the running bar");
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
    match env::args().nth(1).as_deref() {
        Some("-h" | "--help") => print_help(),
        Some("-V" | "--version") => println!("mhyprbar {}", env!("CARGO_PKG_VERSION")),
        Some("--reload") => run_control(ipc::Request::Reload)?,
        Some("--status") => run_control(ipc::Request::Status)?,
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
