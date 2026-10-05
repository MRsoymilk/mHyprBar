use rustsni::MenuNode;

use crate::modules::tray::MenuConfig;

#[derive(Debug, Clone)]
pub struct TrayPopupInlineControl {
    pub label: String,
    pub node_id: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct TrayPopupItem {
    pub label: String,
    pub node_id: Option<i32>,
    pub children: Vec<TrayPopupItem>,
    pub inline_controls: Vec<TrayPopupInlineControl>,
    pub separator_before: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPopupHit {
    Root(usize),
    Inline { root: usize, control: usize },
    Child { root: usize, child: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPopupClick {
    Keep,
    Close,
    Activate(i32),
}

#[derive(Debug, Clone, Copy)]
pub struct TrayPopupRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl TrayPopupRect {
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

pub struct TrayPopupModel {
    pub items: Vec<TrayPopupItem>,
    pub style: MenuConfig,
    pub hovered: Option<TrayPopupHit>,
    pub open_root: Option<usize>,
    origin: (f64, f64),
}

impl TrayPopupModel {
    pub fn from_nodes(nodes: &[MenuNode], style: MenuConfig, origin: (f64, f64)) -> Self {
        Self {
            items: build_items(nodes),
            style,
            hovered: None,
            open_root: None,
            origin,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_origin(&mut self, x: f64, y: f64) {
        self.origin = (x, y);
        self.hovered = None;
        self.open_root = None;
    }

    pub fn root_height(&self) -> f64 {
        self.items.len() as f64 * self.style.item_height as f64
    }

    pub fn root_rect(&self, surface_w: f64, surface_h: f64) -> TrayPopupRect {
        let w = self.style.width as f64;
        let h = self.items.len() as f64 * self.style.item_height as f64;
        TrayPopupRect {
            x: self.origin.0.clamp(0.0, (surface_w - w).max(0.0)),
            y: self.origin.1.clamp(0.0, (surface_h - h).max(0.0)),
            w: w.min(surface_w.max(1.0)),
            h: h.min(surface_h.max(1.0)),
        }
    }

    pub fn root_item_rect(
        &self,
        index: usize,
        surface_w: f64,
        surface_h: f64,
    ) -> Option<TrayPopupRect> {
        if index >= self.items.len() {
            return None;
        }
        let root = self.root_rect(surface_w, surface_h);
        Some(TrayPopupRect {
            x: root.x,
            y: root.y + index as f64 * self.style.item_height as f64,
            w: root.w,
            h: self.style.item_height as f64,
        })
    }

    pub fn submenu_rect(&self, surface_w: f64, surface_h: f64) -> Option<TrayPopupRect> {
        let root_index = self.open_root?;
        let parent = self.root_item_rect(root_index, surface_w, surface_h)?;
        let count = self.items.get(root_index)?.children.len();
        if count == 0 {
            return None;
        }
        let w = self.style.width as f64;
        let h = count as f64 * self.style.item_height as f64;
        let x = if parent.x + parent.w + w <= surface_w {
            parent.x + parent.w
        } else {
            (parent.x - w).max(0.0)
        };
        Some(TrayPopupRect {
            x,
            y: parent.y.clamp(0.0, (surface_h - h).max(0.0)),
            w: w.min(surface_w.max(1.0)),
            h: h.min(surface_h.max(1.0)),
        })
    }

    pub fn child_item_rect(
        &self,
        child: usize,
        surface_w: f64,
        surface_h: f64,
    ) -> Option<TrayPopupRect> {
        let root = self.open_root?;
        let submenu = self.submenu_rect(surface_w, surface_h)?;
        if child >= self.items.get(root)?.children.len() {
            return None;
        }
        Some(TrayPopupRect {
            x: submenu.x,
            y: submenu.y + child as f64 * self.style.item_height as f64,
            w: submenu.w,
            h: self.style.item_height as f64,
        })
    }

    pub fn pointer_moved(&mut self, x: f64, y: f64, surface_w: f64, surface_h: f64) -> bool {
        let hit = self.hit_test(x, y, surface_w, surface_h);
        let mut changed = hit != self.hovered;
        self.hovered = hit;

        let next_open = match hit {
            Some(TrayPopupHit::Root(index)) if !self.items[index].children.is_empty() => {
                Some(index)
            }
            Some(TrayPopupHit::Child { root, .. }) => Some(root),
            Some(TrayPopupHit::Inline { .. }) | Some(TrayPopupHit::Root(_)) | None => None,
        };
        if self.open_root != next_open {
            self.open_root = next_open;
            changed = true;
        }
        changed
    }

    pub fn click(&self, x: f64, y: f64, surface_w: f64, surface_h: f64) -> TrayPopupClick {
        match self.hit_test(x, y, surface_w, surface_h) {
            Some(TrayPopupHit::Root(index)) => {
                let item = &self.items[index];
                if !item.enabled || !item.children.is_empty() {
                    TrayPopupClick::Keep
                } else {
                    item.node_id
                        .map(TrayPopupClick::Activate)
                        .unwrap_or(TrayPopupClick::Keep)
                }
            }
            Some(TrayPopupHit::Inline { root, control }) => {
                let item = &self.items[root].inline_controls[control];
                if item.enabled {
                    TrayPopupClick::Activate(item.node_id)
                } else {
                    TrayPopupClick::Keep
                }
            }
            Some(TrayPopupHit::Child { root, child }) => {
                let item = &self.items[root].children[child];
                if !item.enabled {
                    TrayPopupClick::Keep
                } else {
                    item.node_id
                        .map(TrayPopupClick::Activate)
                        .unwrap_or(TrayPopupClick::Keep)
                }
            }
            None => TrayPopupClick::Close,
        }
    }

    fn hit_test(&self, x: f64, y: f64, surface_w: f64, surface_h: f64) -> Option<TrayPopupHit> {
        if let Some(root) = self.open_root
            && let Some(submenu) = self.submenu_rect(surface_w, surface_h)
            && submenu.contains(x, y)
        {
            let child = ((y - submenu.y) / self.style.item_height as f64) as usize;
            if child < self.items[root].children.len() {
                return Some(TrayPopupHit::Child { root, child });
            }
        }

        let root = self.root_rect(surface_w, surface_h);
        if root.contains(x, y) {
            let index = ((y - root.y) / self.style.item_height as f64) as usize;
            if index < self.items.len() {
                let item = &self.items[index];
                if !item.inline_controls.is_empty() {
                    let count = item.inline_controls.len();
                    let relative_x = (x - root.x).clamp(0.0, root.w.max(1.0));
                    let control = ((relative_x / root.w.max(1.0)) * count as f64) as usize;
                    return Some(TrayPopupHit::Inline {
                        root: index,
                        control: control.min(count - 1),
                    });
                }
                return Some(TrayPopupHit::Root(index));
            }
        }
        None
    }
}

fn build_items(nodes: &[MenuNode]) -> Vec<TrayPopupItem> {
    let visible = nodes.iter().filter(|node| node.visible).collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut separator_before = false;
    let mut index = 0;

    while index < visible.len() {
        let node = visible[index];
        if is_separator(node) {
            separator_before = true;
            index += 1;
            continue;
        }

        if let Some(inline_controls) = media_control_triplet(&visible, index) {
            result.push(TrayPopupItem {
                label: String::new(),
                node_id: None,
                children: Vec::new(),
                inline_controls,
                separator_before,
                enabled: true,
            });
            separator_before = false;
            index += 3;
            continue;
        }

        let children = flatten_children(&node.children, String::new());
        result.push(TrayPopupItem {
            label: decorated_label(node),
            node_id: children.is_empty().then_some(node.id),
            children,
            inline_controls: Vec::new(),
            separator_before,
            enabled: node.enabled,
        });
        separator_before = false;
        index += 1;
    }
    result
}

fn media_control_triplet(nodes: &[&MenuNode], start: usize) -> Option<Vec<TrayPopupInlineControl>> {
    let triplet = nodes.get(start..start + 3)?;
    let previous = triplet[0];
    let toggle = triplet[1];
    let next = triplet[2];
    if previous.icon_name != "media-skip-backward"
        || !matches!(
            toggle.icon_name.as_str(),
            "media-playback-start" | "media-playback-pause"
        )
        || next.icon_name != "media-skip-forward"
        || triplet.iter().any(|node| is_separator(node) || !node.children.is_empty())
    {
        return None;
    }

    Some(vec![
        TrayPopupInlineControl {
            label: "|<".into(),
            node_id: previous.id,
            enabled: previous.enabled,
        },
        TrayPopupInlineControl {
            label: if toggle.icon_name == "media-playback-pause" {
                "||".into()
            } else {
                ">".into()
            },
            node_id: toggle.id,
            enabled: toggle.enabled,
        },
        TrayPopupInlineControl {
            label: ">|".into(),
            node_id: next.id,
            enabled: next.enabled,
        },
    ])
}

fn flatten_children(nodes: &[MenuNode], prefix: String) -> Vec<TrayPopupItem> {
    let mut result = Vec::new();
    let mut separator_before = false;

    for node in nodes.iter().filter(|node| node.visible) {
        if is_separator(node) {
            separator_before = true;
            continue;
        }

        let label = decorated_label(node);
        if !node.children.is_empty() {
            let next_prefix = if prefix.is_empty() {
                format!("{label} › ")
            } else {
                format!("{prefix}{label} › ")
            };
            let mut nested = flatten_children(&node.children, next_prefix);
            if separator_before && let Some(first) = nested.first_mut() {
                first.separator_before = true;
            }
            result.extend(nested);
            separator_before = false;
            continue;
        }

        result.push(TrayPopupItem {
            label: if prefix.is_empty() {
                label
            } else {
                format!("{prefix}{label}")
            },
            node_id: Some(node.id),
            children: Vec::new(),
            inline_controls: Vec::new(),
            separator_before,
            enabled: node.enabled,
        });
        separator_before = false;
    }
    result
}

fn is_separator(node: &MenuNode) -> bool {
    node.label.trim().is_empty() && node.children.is_empty()
}

fn decorated_label(node: &MenuNode) -> String {
    let label = strip_mnemonic(&node.label);
    match (node.toggle_type.as_str(), node.toggle_state) {
        ("checkmark", 1) => format!("✓ {label}"),
        ("radio", 1) => format!("● {label}"),
        ("checkmark" | "radio", 0) => format!("  {label}"),
        _ => label,
    }
}

fn strip_mnemonic(label: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i32, label: &str) -> MenuNode {
        MenuNode {
            id,
            label: label.into(),
            enabled: true,
            visible: true,
            icon_name: String::new(),
            icon_data: Vec::new(),
            toggle_type: String::new(),
            toggle_state: -1,
            is_submenu: false,
            children: Vec::new(),
        }
    }

    fn media_node(id: i32, label: &str, icon_name: &str) -> MenuNode {
        let mut node = node(id, label);
        node.icon_name = icon_name.into();
        node
    }

    #[test]
    fn click_returns_dbus_node_id() {
        let model =
            TrayPopupModel::from_nodes(&[node(12, "_Quit")], MenuConfig::default(), (100.0, 50.0));
        assert_eq!(
            model.click(120.0, 60.0, 1920.0, 1080.0),
            TrayPopupClick::Activate(12)
        );
    }

    #[test]
    fn hover_opens_submenu() {
        let mut parent = node(1, "Connections");
        parent.children = vec![node(2, "Server")];
        let mut model = TrayPopupModel::from_nodes(&[parent], MenuConfig::default(), (100.0, 50.0));
        assert!(model.pointer_moved(120.0, 60.0, 1920.0, 1080.0));
        assert_eq!(model.open_root, Some(0));
    }

    #[test]
    fn media_controls_share_one_row_and_keep_individual_clicks() {
        let nodes = vec![
            media_node(10, "Previous", "media-skip-backward"),
            media_node(11, "Play", "media-playback-start"),
            media_node(12, "Next", "media-skip-forward"),
            node(13, "Web"),
        ];
        let model = TrayPopupModel::from_nodes(&nodes, MenuConfig::default(), (0.0, 0.0));
        assert_eq!(model.items.len(), 2);
        assert_eq!(model.items[0].inline_controls.len(), 3);

        let segment = model.style.width as f64 / 3.0;
        let y = model.style.item_height as f64 / 2.0;
        assert_eq!(
            model.click(segment * 0.5, y, 1920.0, 1080.0),
            TrayPopupClick::Activate(10)
        );
        assert_eq!(
            model.click(segment * 1.5, y, 1920.0, 1080.0),
            TrayPopupClick::Activate(11)
        );
        assert_eq!(
            model.click(segment * 2.5, y, 1920.0, 1080.0),
            TrayPopupClick::Activate(12)
        );
    }
}
