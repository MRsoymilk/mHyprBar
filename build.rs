use std::{collections::BTreeMap, fs, process};

const KNOWN_MODULES: &[&str] = &[
    "menu",
    "active_window",
    "clock",
    "layout",
    "cpu",
    "memory",
    "network",
    "audio",
    "brightness",
    "mpris",
    "disk",
    "battery",
    "tray",
];

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    process::exit(1);
}

fn parse_modules(source: &str) -> BTreeMap<String, bool> {
    let mut in_modules = false;
    let mut result = BTreeMap::new();

    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_modules = line == "[modules]";
            continue;
        }
        if !in_modules {
            fail(format!(
                "build.modules.toml:{}: settings must be inside [modules]",
                index + 1
            ));
        }

        let Some((name, value)) = line.split_once('=') else {
            fail(format!(
                "build.modules.toml:{}: expected name = true|false",
                index + 1
            ));
        };
        let name = name.trim();
        let enabled = match value.trim() {
            "true" => true,
            "false" => false,
            other => fail(format!(
                "build.modules.toml:{}: invalid boolean {other:?}",
                index + 1
            )),
        };
        if !KNOWN_MODULES.contains(&name) {
            fail(format!(
                "build.modules.toml:{}: unknown module {name:?}",
                index + 1
            ));
        }
        if result.insert(name.to_owned(), enabled).is_some() {
            fail(format!(
                "build.modules.toml:{}: duplicate module {name:?}",
                index + 1
            ));
        }
    }

    result
}

fn main() {
    println!("cargo:rerun-if-changed=build.modules.toml");
    println!(
        "cargo:rustc-check-cfg=cfg(mhypr_module, values({}))",
        KNOWN_MODULES
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let source = fs::read_to_string("build.modules.toml")
        .unwrap_or_else(|error| fail(format!("failed to read build.modules.toml: {error}")));
    let table = parse_modules(&source);

    let mut enabled = Vec::new();
    for &name in KNOWN_MODULES {
        if table.get(name).copied().unwrap_or(false) {
            println!("cargo:rustc-cfg=mhypr_module=\"{name}\"");
            enabled.push(name);
        }
    }

    println!(
        "cargo:rustc-env=MHYPRBAR_COMPILED_MODULES={}",
        enabled.join(",")
    );
}
