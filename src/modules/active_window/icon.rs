use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};

#[derive(Clone, Debug)]
pub struct WindowIcon {
    pub pixels: Arc<[u8]>,
    pub width: i32,
    pub height: i32,
}

static ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<WindowIcon>>>> = OnceLock::new();

pub fn resolve_window_icon(class: &str, initial_class: &str, size: i32) -> Option<WindowIcon> {
    if size <= 0 {
        return None;
    }

    let cache_key = format!(
        "{}\u{1f}{}\u{1f}{size}",
        class.trim().to_ascii_lowercase(),
        initial_class.trim().to_ascii_lowercase()
    );
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(icon) = cache.get(&cache_key)
    {
        return icon.clone();
    }

    let icon = resolve_window_icon_uncached(class, initial_class, size);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(cache_key, icon.clone());
    }
    icon
}

fn resolve_window_icon_uncached(class: &str, initial_class: &str, size: i32) -> Option<WindowIcon> {
    for name in icon_name_candidates(class, initial_class) {
        let Some(path) = resolve_icon_path(&name, size) else {
            continue;
        };
        if let Ok(pixels) = rasterize_icon(&path, size) {
            return Some(WindowIcon {
                pixels: pixels.into(),
                width: size,
                height: size,
            });
        }
    }
    None
}

fn icon_name_candidates(class: &str, initial_class: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();

    if let Some(icon) = desktop_icon_name(class, initial_class) {
        push_candidate(&mut names, &mut seen, &icon);
    }
    for value in [class, initial_class] {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        push_candidate(&mut names, &mut seen, value);
        push_candidate(&mut names, &mut seen, &value.to_ascii_lowercase());
        if let Some(last) = value.rsplit('.').next() {
            push_candidate(&mut names, &mut seen, last);
            push_candidate(&mut names, &mut seen, &last.to_ascii_lowercase());
        }
    }
    names
}

fn push_candidate(names: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let key = value.to_ascii_lowercase();
    if seen.insert(key) {
        names.push(value.to_owned());
    }
}

fn desktop_icon_name(class: &str, initial_class: &str) -> Option<String> {
    let targets = [class, initial_class]
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return None;
    }

    for root in application_roots() {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if !desktop_matches(&path, &content, &targets) {
                continue;
            }
            if let Some(icon) = desktop_value(&content, "Icon") {
                return Some(icon.to_owned());
            }
        }
    }
    None
}

fn application_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("applications"));
    } else if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }
    let data_dirs =
        env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':').filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(dir).join("applications"));
    }
    roots
}

fn desktop_matches(path: &Path, content: &str, targets: &[String]) -> bool {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if targets.iter().any(|target| target == &stem) {
        return true;
    }

    ["StartupWMClass", "X-GNOME-WMClass"]
        .into_iter()
        .filter_map(|key| desktop_value(content, key))
        .map(str::to_ascii_lowercase)
        .any(|value| targets.iter().any(|target| target == &value))
}

fn desktop_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    content.lines().find_map(|line| {
        let line = line.trim();
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim())
    })
}

fn resolve_icon_path(name: &str, target: i32) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct.to_path_buf());
    }

    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local/share/icons"));
        roots.push(home.join(".icons"));
    }
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("icons"));
    }
    let data_dirs =
        env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':').filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(dir).join("icons"));
    }
    roots.push(PathBuf::from("/usr/share/pixmaps"));

    let mut best: Option<(u32, PathBuf)> = None;
    for root in roots {
        scan_icon_root(&root, name, target, 0, &mut best);
    }
    best.map(|(_, path)| path)
}

fn scan_icon_root(
    dir: &Path,
    name: &str,
    target: i32,
    depth: u8,
    best: &mut Option<(u32, PathBuf)>,
) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_icon_root(&path, name, target, depth + 1, best);
            continue;
        }

        let stem_matches = path
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case(name));
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if !stem_matches || !matches!(ext.as_deref(), Some("svg" | "png" | "xpm")) {
            continue;
        }

        let score = icon_path_score(&path, target);
        if best.as_ref().is_none_or(|(current, _)| score < *current) {
            *best = Some((score, path));
        }
    }
}

fn icon_path_score(path: &Path, target: i32) -> u32 {
    for component in path.components() {
        let value = component.as_os_str().to_string_lossy();
        if value == "scalable" {
            return 0;
        }
        if let Some((width, _)) = value.split_once('x')
            && let Ok(size) = width.parse::<i32>()
        {
            return size.abs_diff(target.max(0));
        }
    }
    10_000
}

fn rasterize_icon(path: &Path, size: i32) -> Result<Vec<u8>> {
    if size <= 0 {
        bail!("invalid window icon size {size}");
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let output = if extension == "svg" {
        let mut svg = Command::new("rsvg-convert")
            .arg("--width")
            .arg(size.to_string())
            .arg("--height")
            .arg(size.to_string())
            .arg("--keep-aspect-ratio")
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| "failed to launch rsvg-convert for window icon")?;

        let stdout = svg
            .stdout
            .take()
            .context("failed to capture rsvg-convert output")?;
        let output = Command::new("magick")
            .arg("png:-")
            .arg("rgba:-")
            .stdin(Stdio::from(stdout))
            .stderr(Stdio::null())
            .output()
            .with_context(|| "failed to launch magick for window icon")?;
        let status = svg.wait().context("failed to wait for rsvg-convert")?;
        if !status.success() || !output.status.success() {
            bail!("window SVG rasterization failed for {}", path.display());
        }
        output.stdout
    } else {
        let output = Command::new("magick")
            .arg(path)
            .arg("-background")
            .arg("none")
            .arg("-resize")
            .arg(format!("{size}x{size}"))
            .arg("-gravity")
            .arg("center")
            .arg("-extent")
            .arg(format!("{size}x{size}"))
            .arg("rgba:-")
            .stderr(Stdio::null())
            .output()
            .with_context(|| "failed to launch magick for window icon")?;
        if !output.status.success() {
            bail!("window image rasterization failed for {}", path.display());
        }
        output.stdout
    };

    let expected = size as usize * size as usize * 4;
    if output.len() != expected {
        bail!(
            "window icon rasterizer returned {} bytes for {size}x{size}, expected {expected}",
            output.len()
        );
    }
    Ok(rgba_to_native_argb(output))
}

fn rgba_to_native_argb(mut pixels: Vec<u8>) -> Vec<u8> {
    for pixel in pixels.chunks_exact_mut(4) {
        let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]];
        if cfg!(target_endian = "little") {
            pixel.copy_from_slice(&[b, g, r, a]);
        } else {
            pixel.copy_from_slice(&[a, r, g, b]);
        }
    }
    pixels
}
