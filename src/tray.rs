#[cfg(mhypr_module = "tray")]
mod enabled {
    use std::{
        collections::HashMap,
        env, fs,
        os::fd::{FromRawFd, OwnedFd},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        time::Duration,
    };

    use crate::modules::tray::TrayConfig;
    use anyhow::{Context, Result, bail, ensure};
    use rustsni::{IconPixmap, ItemId, MenuNode, TrayHost, TrayItem};

    struct CachedThemeIcon {
        source_name: String,
        size: i32,
        pixels: Vec<u8>,
    }

    pub struct TrayState {
        host: TrayHost,
        config: TrayConfig,
        order: Vec<ItemId>,
        theme_icons: HashMap<ItemId, CachedThemeIcon>,
    }

    pub struct TrayIconView<'a> {
        pub width: i32,
        pub height: i32,
        pub pixels: Option<&'a [u8]>,
    }

    pub struct TrayMenuRequest {
        pub item_id: ItemId,
        pub nodes: Vec<MenuNode>,
    }

    impl TrayState {
        pub fn new() -> Result<Self> {
            let config = TrayConfig::load()?;
            let mut host = None;
            let mut last_error = None;
            for attempt in 0..6 {
                match TrayHost::new() {
                    Ok(value) => {
                        host = Some(value);
                        break;
                    }
                    Err(error) => {
                        last_error = Some(error);
                        if attempt < 5 {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            }
            let host = host.ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to start StatusNotifierHost after retries: {}",
                    last_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "unknown error".into())
                )
            })?;
            let mut state = Self {
                host,
                config,
                order: Vec::new(),
                theme_icons: HashMap::new(),
            };
            let _ = state.poll();
            state.sync_order();
            Ok(state)
        }

        pub fn duplicate_fd(&self) -> Result<OwnedFd> {
            let fd = unsafe { libc::dup(self.host.fd()) };
            if fd < 0 {
                bail!(
                    "failed to duplicate tray D-Bus fd: {}",
                    std::io::Error::last_os_error()
                );
            }
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }

        pub fn reload_config(&mut self) -> Result<()> {
            self.config = TrayConfig::load()?;
            self.theme_icons.clear();
            self.sync_order();
            Ok(())
        }

        pub fn poll(&mut self) -> Result<bool> {
            let events = self
                .host
                .poll()
                .context("failed to process tray D-Bus events")?;
            let changed = !events.is_empty();
            if changed {
                self.sync_order();
            }
            Ok(changed)
        }

        pub fn len(&self) -> usize {
            self.order.len()
        }

        pub fn has_item(&self, item_id: &ItemId) -> bool {
            self.host.items().contains_key(item_id)
        }

        pub fn list_text(&self) -> String {
            let mut output = String::new();
            for (index, id) in self.order.iter().enumerate() {
                let Some(item) = self.host.items().get(id) else {
                    continue;
                };
                let title = if item.title.trim().is_empty() {
                    if item.item_id.trim().is_empty() {
                        id.0.as_str()
                    } else {
                        item.item_id.as_str()
                    }
                } else {
                    item.title.as_str()
                };
                let has_menu = item.has_menu();
                let tooltip = self
                    .tooltip_text_for_index(index)
                    .unwrap_or_default()
                    .replace(['\t', '\n'], " ");
                output.push_str(&format!(
                    "{index}\t{title}\tmenu={}\tstatus={}\ttooltip={tooltip:?}\n",
                    u8::from(has_menu),
                    item.status
                ));
            }
            output
        }

        pub fn menu_request_index(&mut self, index: usize) -> Result<TrayMenuRequest> {
            let id = self
                .order
                .get(index)
                .cloned()
                .with_context(|| format!("tray index {index} is out of range"))?;
            self.menu_request_for_id(id)
        }

        pub fn menu_request_at(&mut self, x: i32) -> Result<Option<TrayMenuRequest>> {
            let Some(id) = self.item_at(x).cloned() else {
                return Ok(None);
            };
            if !self.host.items().get(&id).is_some_and(TrayItem::has_menu) {
                return Ok(None);
            }
            self.menu_request_for_id(id).map(Some)
        }

        pub fn menu_style(&self) -> crate::modules::tray::MenuConfig {
            self.config.menu.clone()
        }

        pub fn menu_click_item(&mut self, item_id: &ItemId, node_id: i32) -> Result<()> {
            ensure!(
                self.host.items().contains_key(item_id),
                "tray menu item disappeared"
            );
            self.host
                .menu_click(item_id, node_id)
                .context("tray DBusMenu click failed")
        }

        fn menu_request_for_id(&mut self, id: ItemId) -> Result<TrayMenuRequest> {
            let nodes = self
                .host
                .get_menu(&id, 0)
                .context("failed to read tray DBusMenu")?;
            ensure!(!nodes.is_empty(), "tray item has no DBusMenu entries");
            Ok(TrayMenuRequest { item_id: id, nodes })
        }

        pub fn width(&self) -> i32 {
            let count = self.order.len() as i32;
            if count == 0 {
                return 0;
            }
            self.config
                .style
                .padding_x
                .max(0)
                .saturating_mul(2)
                .saturating_add(count.saturating_mul(self.config.icon_size))
                .saturating_add(count.saturating_sub(1).saturating_mul(self.config.spacing))
        }

        pub fn icon_size(&self) -> i32 {
            self.config.icon_size
        }

        pub fn spacing(&self) -> i32 {
            self.config.spacing
        }

        pub fn padding_x(&self) -> i32 {
            self.config.style.padding_x.max(0)
        }

        pub fn icons(&self) -> Vec<TrayIconView<'_>> {
            self.order
                .iter()
                .filter_map(|id| {
                    let item = self.host.items().get(id)?;
                    if let Some(pixmap) = choose_pixmap(item, self.config.icon_size) {
                        return Some(TrayIconView {
                            width: pixmap.width as i32,
                            height: pixmap.height as i32,
                            pixels: Some(pixmap.data.as_slice()),
                        });
                    }

                    let cached = self.theme_icons.get(id);
                    Some(TrayIconView {
                        width: self.config.icon_size,
                        height: self.config.icon_size,
                        pixels: cached
                            .filter(|cached| !cached.pixels.is_empty())
                            .map(|cached| cached.pixels.as_slice()),
                    })
                })
                .collect()
        }

        pub fn item_index_at(&self, x: i32) -> Option<usize> {
            let mut cursor = self.padding_x();
            for (index, _) in self.order.iter().enumerate() {
                let end = cursor.saturating_add(self.config.icon_size);
                if x >= cursor && x < end {
                    return Some(index);
                }
                cursor = end.saturating_add(self.config.spacing);
            }
            None
        }

        pub fn item_at(&self, x: i32) -> Option<&ItemId> {
            self.item_index_at(x)
                .and_then(|index| self.order.get(index))
        }

        pub fn item_key_for_index(&self, index: usize) -> Option<&str> {
            self.order.get(index).map(|id| id.0.as_str())
        }

        pub fn tooltip_enabled(&self) -> bool {
            self.config.tooltip.enabled
        }

        pub fn tooltip_delay(&self) -> Duration {
            Duration::from_millis(self.config.tooltip.delay_ms)
        }

        pub fn tooltip_offset(&self) -> i32 {
            self.config.tooltip.offset
        }

        pub fn tooltip_style(&self) -> &crate::config::ModuleStyle {
            &self.config.tooltip.style
        }

        pub fn tooltip_text_for_index(&self, index: usize) -> Option<String> {
            let id = self.order.get(index)?;
            let item = self.host.items().get(id)?;

            let title = if item.tooltip.title.trim().is_empty() {
                item.title.trim()
            } else {
                item.tooltip.title.trim()
            };
            let description = strip_tooltip_markup(&item.tooltip.text);

            let mut text = match (title.is_empty(), description.is_empty()) {
                (false, false) => format!("{title} — {description}"),
                (false, true) => title.to_owned(),
                (true, false) => description,
                (true, true) => {
                    if item.item_id.trim().is_empty() {
                        return None;
                    }
                    item.item_id.trim().to_owned()
                }
            };
            text = truncate_chars(&text, self.config.tooltip.max_chars);
            (!text.trim().is_empty()).then_some(text)
        }

        pub fn activate_at(&mut self, x: i32, screen_x: i32, screen_y: i32) -> Result<bool> {
            let Some(id) = self.item_at(x).cloned() else {
                return Ok(false);
            };
            self.host
                .activate(&id, screen_x, screen_y)
                .context("tray Activate failed")?;
            Ok(true)
        }

        pub fn item_is_menu_at(&self, x: i32) -> bool {
            self.item_at(x)
                .and_then(|id| self.host.items().get(id))
                .is_some_and(|item| item.item_is_menu)
        }

        pub fn context_menu_at(&mut self, x: i32, screen_x: i32, screen_y: i32) -> Result<bool> {
            let Some(id) = self.item_at(x).cloned() else {
                return Ok(false);
            };
            self.host
                .context_menu(&id, screen_x, screen_y)
                .context("tray ContextMenu failed")?;
            Ok(true)
        }

        pub fn secondary_activate_at(
            &mut self,
            x: i32,
            screen_x: i32,
            screen_y: i32,
        ) -> Result<bool> {
            let Some(id) = self.item_at(x).cloned() else {
                return Ok(false);
            };
            self.host
                .secondary_activate(&id, screen_x, screen_y)
                .context("tray SecondaryActivate failed")?;
            Ok(true)
        }

        pub fn scroll_at(&mut self, x: i32, delta: i32, orientation: &str) -> Result<bool> {
            if delta == 0 {
                return Ok(false);
            }
            let Some(id) = self.item_at(x).cloned() else {
                return Ok(false);
            };
            self.host
                .scroll(&id, delta, orientation)
                .context("tray Scroll failed")?;
            Ok(true)
        }

        fn sync_order(&mut self) {
            let mut order = self
                .host
                .items()
                .iter()
                .filter(|(_, item)| self.config.show_passive || item.status != "Passive")
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            order.sort_by(|a, b| a.0.cmp(&b.0));
            self.order = order;
            self.sync_theme_icons();
        }

        fn sync_theme_icons(&mut self) {
            self.theme_icons
                .retain(|id, _| self.order.iter().any(|current| current == id));

            let target = self.config.icon_size;
            let tasks =
                self.order
                    .iter()
                    .filter_map(|id| {
                        let item = self.host.items().get(id)?;
                        if choose_pixmap(item, target).is_some() {
                            return Some((id.clone(), None));
                        }

                        let name = icon_name(item);
                        if name.is_empty() {
                            return Some((id.clone(), None));
                        }

                        let cached_matches = self.theme_icons.get(id).is_some_and(|cached| {
                            cached.source_name == name && cached.size == target
                        });
                        if cached_matches {
                            return None;
                        }

                        Some((
                            id.clone(),
                            Some((name.to_owned(), item.icon_theme_path.clone())),
                        ))
                    })
                    .collect::<Vec<_>>();

            for (id, task) in tasks {
                let Some((name, theme_path)) = task else {
                    self.theme_icons.remove(&id);
                    continue;
                };

                let pixels = resolve_icon_path(&name, &theme_path, target)
                    .and_then(|path| rasterize_icon(&path, target).ok())
                    .unwrap_or_default();
                if pixels.is_empty() {
                    eprintln!("mhyprbar: tray icon {name:?} has no usable IconPixmap/theme image");
                }
                self.theme_icons.insert(
                    id,
                    CachedThemeIcon {
                        source_name: name,
                        size: target,
                        pixels,
                    },
                );
            }
        }
    }

    fn strip_tooltip_markup(text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        let mut in_tag = false;
        for ch in text.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => output.push(ch),
                _ => {}
            }
        }
        output
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn truncate_chars(text: &str, max_chars: usize) -> String {
        if text.chars().count() <= max_chars {
            return text.to_owned();
        }
        let keep = max_chars.saturating_sub(1);
        let mut value = text.chars().take(keep).collect::<String>();
        value.push('…');
        value
    }

    fn icon_name(item: &TrayItem) -> &str {
        if item.status == "NeedsAttention" && !item.attention_icon_name.is_empty() {
            &item.attention_icon_name
        } else {
            &item.icon_name
        }
    }

    fn choose_pixmap(item: &TrayItem, target: i32) -> Option<&IconPixmap> {
        let pixmaps = if item.status == "NeedsAttention" && !item.attention_icon_pixmaps.is_empty()
        {
            &item.attention_icon_pixmaps
        } else {
            &item.icon_pixmaps
        };
        pixmaps.iter().min_by_key(|pixmap| {
            let size = pixmap.width.max(pixmap.height) as i32;
            size.abs_diff(target.max(0))
        })
    }

    fn resolve_icon_path(name: &str, item_theme_path: &str, target: i32) -> Option<PathBuf> {
        let direct = Path::new(name);
        if direct.is_absolute() && direct.is_file() {
            return Some(direct.to_path_buf());
        }

        let mut roots = Vec::new();
        if !item_theme_path.trim().is_empty() {
            roots.push(PathBuf::from(item_theme_path));
        }
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
        for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
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

            let stem_matches = path.file_stem().and_then(|value| value.to_str()) == Some(name);
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            if !stem_matches || !matches!(ext.as_deref(), Some("svg" | "png")) {
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
            bail!("invalid tray icon size {size}");
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
                .with_context(|| "failed to launch rsvg-convert for tray IconName")?;

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
                .with_context(|| "failed to launch magick for tray IconName")?;
            let status = svg.wait().context("failed to wait for rsvg-convert")?;
            if !status.success() || !output.status.success() {
                bail!("tray SVG rasterization failed for {}", path.display());
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
                .with_context(|| "failed to launch magick for tray IconName")?;
            if !output.status.success() {
                bail!("tray image rasterization failed for {}", path.display());
            }
            output.stdout
        };

        let expected = size as usize * size as usize * 4;
        if output.len() != expected {
            bail!(
                "tray rasterizer returned {} bytes for {size}x{size}, expected {expected}",
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

    #[cfg(test)]
    mod tests {
        use std::path::Path;

        use rustsni::{IconPixmap, ItemId, ToolTip, TrayItem};

        use super::{
            choose_pixmap, icon_path_score, rgba_to_native_argb, strip_tooltip_markup,
            truncate_chars,
        };

        fn test_item() -> TrayItem {
            TrayItem {
                id: ItemId(":1.42/test".into()),
                bus_name: ":1.42".into(),
                object_path: "/test".into(),
                category: String::new(),
                item_id: String::new(),
                title: String::new(),
                status: "Active".into(),
                window_id: 0,
                icon_theme_path: String::new(),
                icon_name: String::new(),
                icon_pixmaps: Vec::new(),
                attention_icon_name: String::new(),
                attention_icon_pixmaps: Vec::new(),
                attention_movie_name: String::new(),
                overlay_icon_name: String::new(),
                overlay_icon_pixmaps: Vec::new(),
                item_is_menu: false,
                menu_path: String::new(),
                tooltip: ToolTip::default(),
            }
        }

        #[test]
        fn chooses_closest_pixmap_size() {
            let mut item = test_item();
            item.icon_pixmaps = vec![
                IconPixmap {
                    width: 16,
                    height: 16,
                    data: vec![0; 16 * 16 * 4],
                },
                IconPixmap {
                    width: 32,
                    height: 32,
                    data: vec![0; 32 * 32 * 4],
                },
            ];
            assert_eq!(choose_pixmap(&item, 18).unwrap().width, 16);
            assert_eq!(choose_pixmap(&item, 30).unwrap().width, 32);
        }

        #[test]
        fn scores_nearest_theme_size() {
            assert_eq!(
                icon_path_score(Path::new("/icons/hicolor/16x16/status/icon.svg"), 18),
                2
            );
            assert_eq!(
                icon_path_score(Path::new("/icons/hicolor/scalable/status/icon.svg"), 18),
                0
            );
        }

        #[test]
        fn converts_rgba_to_native_argb() {
            let value = rgba_to_native_argb(vec![0x11, 0x22, 0x33, 0xFF]);
            if cfg!(target_endian = "little") {
                assert_eq!(value, vec![0x33, 0x22, 0x11, 0xFF]);
            } else {
                assert_eq!(value, vec![0xFF, 0x11, 0x22, 0x33]);
            }
        }

        #[test]
        fn normalizes_tooltip_markup_and_length() {
            assert_eq!(
                strip_tooltip_markup("<b>Remmina</b> &amp; <i>Server</i>"),
                "Remmina & Server"
            );
            assert_eq!(truncate_chars("abcdef", 4), "abc…");
            assert_eq!(truncate_chars("中文测试", 3), "中文…");
        }
    }
}

#[cfg(mhypr_module = "tray")]
pub use enabled::*;

#[cfg(not(mhypr_module = "tray"))]
pub struct TrayState;

#[cfg(not(mhypr_module = "tray"))]
pub struct TrayIconView<'a> {
    pub width: i32,
    pub height: i32,
    pub pixels: Option<&'a [u8]>,
}

#[cfg(not(mhypr_module = "tray"))]
impl TrayState {
    pub fn duplicate_fd(&self) -> anyhow::Result<std::os::fd::OwnedFd> {
        anyhow::bail!("tray module is not compiled")
    }

    pub fn reload_config(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn poll(&mut self) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub fn item_index_at(&self, _x: i32) -> Option<usize> {
        None
    }

    pub fn item_key_for_index(&self, _index: usize) -> Option<&str> {
        None
    }

    pub fn tooltip_enabled(&self) -> bool {
        false
    }

    pub fn tooltip_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    pub fn tooltip_offset(&self) -> i32 {
        0
    }

    pub fn tooltip_style(&self) -> &'static crate::config::ModuleStyle {
        static STYLE: std::sync::OnceLock<crate::config::ModuleStyle> = std::sync::OnceLock::new();
        STYLE.get_or_init(crate::config::ModuleStyle::default)
    }

    pub fn tooltip_text_for_index(&self, _index: usize) -> Option<String> {
        None
    }

    pub fn activate_at(&mut self, _x: i32, _screen_x: i32, _screen_y: i32) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub fn secondary_activate_at(
        &mut self,
        _x: i32,
        _screen_x: i32,
        _screen_y: i32,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub fn scroll_at(&mut self, _x: i32, _delta: i32, _orientation: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    pub fn len(&self) -> usize {
        0
    }

    pub fn list_text(&self) -> String {
        String::new()
    }

    pub fn width(&self) -> i32 {
        0
    }

    pub fn icon_size(&self) -> i32 {
        0
    }

    pub fn spacing(&self) -> i32 {
        0
    }

    pub fn padding_x(&self) -> i32 {
        0
    }

    pub fn icons(&self) -> Vec<TrayIconView<'_>> {
        Vec::new()
    }
}
