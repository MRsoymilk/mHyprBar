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

    use anyhow::{Context, Result, bail, ensure};
    use rustsni::{IconPixmap, ItemId, MenuNode, TrayHost, TrayItem};
    use serde::Serialize;

    use crate::modules::tray::TrayConfig;

    struct CachedThemeIcon {
        source_name: String,
        size: i32,
        pixels: Vec<u8>,
    }

    #[derive(Serialize)]
    struct DynamicMenuConfig {
        items: Vec<DynamicMenuItem>,
    }

    #[derive(Serialize)]
    struct DynamicMenuItem {
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        children: Vec<DynamicMenuItem>,
        #[serde(skip_serializing_if = "is_false")]
        separator_before: bool,
    }

    fn is_false(value: &bool) -> bool {
        !*value
    }

    pub struct TrayState {
        host: TrayHost,
        config: TrayConfig,
        order: Vec<ItemId>,
        theme_icons: HashMap<ItemId, CachedThemeIcon>,
        menu_target: Option<(u64, ItemId)>,
        next_menu_generation: u64,
    }

    pub struct TrayIconView<'a> {
        pub width: i32,
        pub height: i32,
        pub pixels: Option<&'a [u8]>,
    }

    impl TrayState {
        pub fn new() -> Result<Self> {
            let config = TrayConfig::load()?;
            cleanup_dynamic_menu_dirs();
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
                menu_target: None,
                next_menu_generation: 0,
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

        pub fn open_menu_index(&mut self, index: usize) -> Result<()> {
            let id = self
                .order
                .get(index)
                .cloned()
                .with_context(|| format!("tray index {index} is out of range"))?;
            let nodes = self
                .host
                .get_menu(&id, 0)
                .context("failed to read tray DBusMenu")?;
            ensure!(
                !nodes.is_empty(),
                "tray item {index} has no DBusMenu entries"
            );

            self.next_menu_generation = self.next_menu_generation.wrapping_add(1).max(1);
            let generation = self.next_menu_generation;
            self.menu_target = Some((generation, id));
            if let Err(error) = launch_dynamic_menu(&nodes, generation) {
                self.menu_target = None;
                return Err(error);
            }
            Ok(())
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
            let is_menu = self
                .host
                .items()
                .get(&id)
                .is_some_and(|item| item.item_is_menu);
            if is_menu {
                return self.context_menu_at(x, screen_x, screen_y);
            }

            self.host
                .activate(&id, screen_x, screen_y)
                .context("tray Activate failed")?;
            Ok(true)
        }

        pub fn context_menu_at(&mut self, x: i32, screen_x: i32, screen_y: i32) -> Result<bool> {
            let Some(id) = self.item_at(x).cloned() else {
                return Ok(false);
            };

            let has_dbus_menu = self.host.items().get(&id).is_some_and(TrayItem::has_menu);
            if has_dbus_menu {
                let nodes = self
                    .host
                    .get_menu(&id, 0)
                    .context("failed to read tray DBusMenu")?;
                if !nodes.is_empty() {
                    self.next_menu_generation = self.next_menu_generation.wrapping_add(1).max(1);
                    let generation = self.next_menu_generation;
                    self.menu_target = Some((generation, id.clone()));
                    if let Err(error) = launch_dynamic_menu(&nodes, generation) {
                        self.menu_target = None;
                        return Err(error);
                    }
                    return Ok(true);
                }
            }

            self.host
                .context_menu(&id, screen_x, screen_y)
                .context("tray ContextMenu failed")?;
            Ok(true)
        }

        pub fn menu_click(&mut self, generation: u64, node_id: i32) -> Result<()> {
            let Some((active_generation, id)) = self.menu_target.as_ref() else {
                bail!("no active tray menu");
            };
            ensure!(
                *active_generation == generation,
                "stale tray menu generation"
            );
            let id = id.clone();
            ensure!(
                self.host.items().contains_key(&id),
                "tray menu item disappeared"
            );
            self.host
                .menu_click(&id, node_id)
                .context("tray DBusMenu click failed")?;
            self.menu_target = None;
            Ok(())
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
            if self
                .menu_target
                .as_ref()
                .is_some_and(|(_, id)| !self.host.items().contains_key(id))
            {
                self.menu_target = None;
            }
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

    fn launch_dynamic_menu(nodes: &[MenuNode], generation: u64) -> Result<()> {
        let bar_exe = menu_callback_executable()?;
        let items = build_dynamic_menu_items(nodes, generation, &bar_exe);
        ensure!(!items.is_empty(), "tray DBusMenu contains no visible items");

        let config = DynamicMenuConfig { items };
        let source =
            toml::to_string_pretty(&config).context("failed to serialize tray DBusMenu")?;
        let config_dir = prepare_dynamic_menu_dir(generation)?;
        fs::write(config_dir.join("config.toml"), source)
            .context("failed to write dynamic tray menu config")?;

        let style = resolve_mhyprmenu_style()
            .context("mHyprMenu style.toml is unavailable for tray menu")?;
        fs::copy(style, config_dir.join("style.toml"))
            .context("failed to prepare dynamic tray menu style")?;

        let binary = resolve_mhyprmenu_binary();
        Command::new(&binary)
            .arg("--oneshot")
            .env("MHYPRMENU_CONFIG_DIR", &config_dir)
            .spawn()
            .with_context(|| format!("failed to launch {}", binary.display()))?;
        Ok(())
    }

    fn build_dynamic_menu_items(
        nodes: &[MenuNode],
        generation: u64,
        bar_exe: &Path,
    ) -> Vec<DynamicMenuItem> {
        let mut result = Vec::new();
        let mut separator_before = false;

        for node in nodes.iter().filter(|node| node.visible) {
            if is_menu_separator(node) {
                separator_before = true;
                continue;
            }

            let children =
                flatten_dynamic_children(&node.children, generation, bar_exe, String::new());
            let command = if children.is_empty() && node.enabled {
                Some(menu_callback_command(bar_exe, generation, node.id))
            } else {
                None
            };
            result.push(DynamicMenuItem {
                label: decorated_menu_label(node),
                command,
                children,
                separator_before,
            });
            separator_before = false;
        }

        result
    }

    fn flatten_dynamic_children(
        nodes: &[MenuNode],
        generation: u64,
        bar_exe: &Path,
        prefix: String,
    ) -> Vec<DynamicMenuItem> {
        let mut result = Vec::new();
        let mut separator_before = false;

        for node in nodes.iter().filter(|node| node.visible) {
            if is_menu_separator(node) {
                separator_before = true;
                continue;
            }

            let label = decorated_menu_label(node);
            if !node.children.is_empty() {
                let next_prefix = if prefix.is_empty() {
                    format!("{label} › ")
                } else {
                    format!("{prefix}{label} › ")
                };
                let mut nested =
                    flatten_dynamic_children(&node.children, generation, bar_exe, next_prefix);
                if separator_before && let Some(first) = nested.first_mut() {
                    first.separator_before = true;
                }
                result.extend(nested);
                separator_before = false;
                continue;
            }

            let label = if prefix.is_empty() {
                label
            } else {
                format!("{prefix}{label}")
            };
            result.push(DynamicMenuItem {
                label,
                command: node
                    .enabled
                    .then(|| menu_callback_command(bar_exe, generation, node.id)),
                children: Vec::new(),
                separator_before,
            });
            separator_before = false;
        }

        result
    }

    fn is_menu_separator(node: &MenuNode) -> bool {
        node.label.trim().is_empty() && node.children.is_empty()
    }

    fn decorated_menu_label(node: &MenuNode) -> String {
        let label = strip_menu_mnemonic(&node.label);
        match (node.toggle_type.as_str(), node.toggle_state) {
            ("checkmark", 1) => format!("✓ {label}"),
            ("radio", 1) => format!("● {label}"),
            ("checkmark" | "radio", 0) => format!("  {label}"),
            _ => label,
        }
    }

    fn strip_menu_mnemonic(label: &str) -> String {
        let mut result = String::with_capacity(label.len());
        let mut chars = label.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '_' {
                if chars.peek() == Some(&'_') {
                    result.push('_');
                    let _ = chars.next();
                }
                continue;
            }
            result.push(ch);
        }
        result
    }

    fn menu_callback_executable() -> Result<PathBuf> {
        let proc_exe = PathBuf::from(format!("/proc/{}/exe", std::process::id()));
        if proc_exe.exists() {
            return Ok(proc_exe);
        }

        let current = env::current_exe().context("failed to resolve mHyprBar executable")?;
        if current.exists() {
            return Ok(current);
        }

        let current_text = current.to_string_lossy();
        if let Some(path) = current_text.strip_suffix(" (deleted)") {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
        }

        bail!("no executable path is available for tray menu callbacks")
    }

    fn menu_callback_command(bar_exe: &Path, generation: u64, node_id: i32) -> String {
        format!(
            "{} --tray-menu-click {generation} {node_id}",
            shell_quote(&bar_exe.to_string_lossy())
        )
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }

    fn cleanup_dynamic_menu_dirs() {
        let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") else {
            return;
        };
        let Ok(entries) = fs::read_dir(runtime_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("mhyprbar-tray-menu-") {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    fn prepare_dynamic_menu_dir(generation: u64) -> Result<PathBuf> {
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
        let root =
            PathBuf::from(runtime_dir).join(format!("mhyprbar-tray-menu-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join(generation.to_string());
        fs::create_dir_all(&dir).context("failed to create dynamic tray menu directory")?;
        Ok(dir)
    }

    fn resolve_mhyprmenu_style() -> Option<PathBuf> {
        if let Some(path) = env::var_os("MHYPRMENU_STYLE") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }

        let user_style = if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(config_home).join("mhyprmenu/style.toml")
        } else {
            PathBuf::from(env::var_os("HOME")?).join(".config/mhyprmenu/style.toml")
        };
        if user_style.is_file() {
            return Some(user_style);
        }

        let project_style = mhypr_root_from_current_exe()?
            .join("mHyprMenu")
            .join("style.example.toml");
        project_style.is_file().then_some(project_style)
    }

    fn resolve_mhyprmenu_binary() -> PathBuf {
        if let Some(path) = env::var_os("MHYPRMENU_BIN") {
            return PathBuf::from(path);
        }

        if let Some(root) = mhypr_root_from_current_exe() {
            let binary = root.join("mHyprMenu").join("target/release/mhyprmenu");
            if binary.is_file() {
                return binary;
            }
        }

        PathBuf::from("mhyprmenu")
    }

    fn mhypr_root_from_current_exe() -> Option<PathBuf> {
        let exe = env::current_exe().ok()?;
        exe.parent()?
            .parent()?
            .parent()?
            .parent()
            .map(Path::to_path_buf)
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
        use std::path::{Path, PathBuf};

        use rustsni::{IconPixmap, ItemId, MenuNode, ToolTip, TrayItem};

        use super::{
            DynamicMenuConfig, build_dynamic_menu_items, choose_pixmap, icon_path_score,
            rgba_to_native_argb, strip_tooltip_markup, truncate_chars,
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

        #[test]
        fn callback_executable_uses_running_proc_entry() {
            let path = super::menu_callback_executable().expect("callback executable");
            assert_eq!(
                path,
                PathBuf::from(format!("/proc/{}/exe", std::process::id()))
            );
        }

        #[test]
        fn builds_dynamic_menu_callbacks() {
            let nodes = vec![MenuNode {
                id: 10,
                label: "_Connections".into(),
                enabled: true,
                visible: true,
                icon_name: String::new(),
                icon_data: Vec::new(),
                toggle_type: String::new(),
                toggle_state: -1,
                is_submenu: true,
                children: vec![MenuNode {
                    id: 11,
                    label: "_Server".into(),
                    enabled: true,
                    visible: true,
                    icon_name: String::new(),
                    icon_data: Vec::new(),
                    toggle_type: "checkmark".into(),
                    toggle_state: 1,
                    is_submenu: false,
                    children: Vec::new(),
                }],
            }];

            let items = build_dynamic_menu_items(&nodes, 77, Path::new("/tmp/mhyprbar"));
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].label, "Connections");
            assert_eq!(items[0].children.len(), 1);
            assert_eq!(items[0].children[0].label, "✓ Server");
            assert_eq!(
                items[0].children[0].command.as_deref(),
                Some("'/tmp/mhyprbar' --tray-menu-click 77 11")
            );

            let source =
                toml::to_string_pretty(&DynamicMenuConfig { items }).expect("serialize menu");
            assert!(source.contains("[[items.children]]"));
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

    pub fn context_menu_at(
        &mut self,
        _x: i32,
        _screen_x: i32,
        _screen_y: i32,
    ) -> anyhow::Result<bool> {
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

    pub fn menu_click(&mut self, _generation: u64, _node_id: i32) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn len(&self) -> usize {
        0
    }

    pub fn list_text(&self) -> String {
        String::new()
    }

    pub fn open_menu_index(&mut self, _index: usize) -> anyhow::Result<()> {
        anyhow::bail!("tray module is not compiled")
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
