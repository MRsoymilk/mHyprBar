use anyhow::{Result, ensure};

use crate::hyprland::MonitorInfo;

const DROP_ROWS: usize = 6;
const SNAP_PIXELS: f64 = 9.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    pub mhz: i32,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        let (size, rate) = value.trim().trim_end_matches("Hz").split_once('@')?;
        let (width, height) = size.split_once('x')?;
        let (width, height) = (width.parse().ok()?, height.parse().ok()?);
        let hz: f64 = rate.parse().ok()?;
        (width > 0 && height > 0 && hz.is_finite() && (1.0..=2000.0).contains(&hz)).then_some(
            Self {
                width,
                height,
                mhz: (hz * 1000.0).round() as i32,
            },
        )
    }

    pub fn size(self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub fn matches(self, other: Self) -> bool {
        self.size() == other.size() && self.mhz.abs_diff(other.mhz) <= 15
    }

    fn resolution(self) -> String {
        format!("{} × {}", self.width, self.height)
    }

    fn rate(self) -> String {
        let value = format!("{:.2}", self.mhz as f64 / 1000.0);
        format!("{} Hz", value.trim_end_matches('0').trim_end_matches('.'))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayState {
    pub mode: Mode,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u8,
}

impl DisplayState {
    pub fn from_monitor(monitor: &MonitorInfo) -> Self {
        Self {
            mode: Mode {
                width: monitor.width,
                height: monitor.height,
                mhz: (monitor.refresh_rate * 1000.0).round() as i32,
            },
            x: monitor.x,
            y: monitor.y,
            scale: monitor.scale,
            transform: monitor.transform,
        }
    }

    pub fn size(self) -> (i32, i32) {
        let (mut w, mut h) = self.mode.size();
        if self.transform % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        let scale = self.scale.max(0.1);
        (
            (w as f64 / scale).round().max(1.0) as i32,
            (h as f64 / scale).round().max(1.0) as i32,
        )
    }

    fn bounds(self) -> Area {
        let (w, h) = self.size();
        Area {
            x: self.x,
            y: self.y,
            w,
            h,
        }
    }

    pub fn matches(self, other: Self) -> bool {
        self.mode.matches(other.mode)
            && self.x == other.x
            && self.y == other.y
            && (self.scale - other.scale).abs() < 0.005
            && self.transform == other.transform
    }
}

#[derive(Clone, Debug)]
pub struct Change {
    pub name: String,
    pub before: DisplayState,
    pub after: DisplayState,
    pub set_position: bool,
}

impl Change {
    pub fn is_applied(&self, monitors: &[MonitorInfo]) -> bool {
        monitors.iter().any(|monitor| {
            monitor.name == self.name && self.after.matches(DisplayState::from_monitor(monitor))
        })
    }
}

pub struct Display {
    pub info: MonitorInfo,
    pub number: usize,
    pub draft: DisplayState,
    current: DisplayState,
    modes: Vec<Mode>,
}

impl Display {
    fn new(info: MonitorInfo, number: usize) -> Self {
        let current = DisplayState::from_monitor(&info);
        let mut modes = vec![current.mode];
        for mode in info
            .available_modes
            .iter()
            .filter_map(|mode| Mode::parse(mode))
        {
            if !modes.iter().any(|existing| existing.matches(mode)) {
                modes.push(mode);
            }
        }
        modes.sort_by_key(|mode| std::cmp::Reverse((mode.width, mode.height, mode.mhz)));
        Self {
            info,
            number,
            draft: current,
            current,
            modes,
        }
    }

    pub fn label(&self, field: Field) -> String {
        match field {
            Field::Resolution => self.draft.mode.resolution(),
            Field::RefreshRate => self.draft.mode.rate(),
            Field::Scale => format!("{:.0}%", self.draft.scale * 100.0),
            Field::Orientation => orientation(self.draft.transform).into(),
        }
    }

    fn choices(&self, field: Field) -> Vec<Choice> {
        match field {
            Field::Resolution => {
                let mut sizes = self.modes.clone();
                sizes.dedup_by_key(|mode| mode.size());
                sizes.into_iter().map(Choice::Mode).collect()
            }
            Field::RefreshRate => self
                .modes
                .iter()
                .copied()
                .filter(|mode| mode.size() == self.draft.mode.size())
                .map(Choice::Mode)
                .collect(),
            Field::Scale => {
                // Hyprland requires integral logical dimensions for non-integer scaling.
                let (w, h) = self.draft.mode.size();
                let mut scales: Vec<_> = [
                    50, 75, 100, 125, 150, 175, 200, 225, 250, 275, 300, 350, 400,
                ]
                .into_iter()
                .filter(|percent| {
                    (i64::from(w) * 100) % i64::from(*percent) == 0
                        && (i64::from(h) * 100) % i64::from(*percent) == 0
                })
                .map(|percent| f64::from(percent) / 100.0)
                .collect();
                if !scales
                    .iter()
                    .any(|scale| (scale - self.draft.scale).abs() < 0.0001)
                {
                    scales.push(self.draft.scale);
                    scales.sort_by(f64::total_cmp);
                }
                scales.into_iter().map(Choice::Scale).collect()
            }
            Field::Orientation => {
                let mut rotations = vec![0, 1, 2, 3];
                if self.draft.transform >= 4 {
                    rotations.push(self.draft.transform);
                }
                rotations.into_iter().map(Choice::Transform).collect()
            }
        }
    }

    fn select(&mut self, field: Field, choice: Choice) {
        match choice {
            Choice::Mode(mode) if field == Field::Resolution => {
                self.draft.mode = self
                    .modes
                    .iter()
                    .find(|candidate| {
                        candidate.size() == mode.size() && candidate.mhz == self.draft.mode.mhz
                    })
                    .copied()
                    .unwrap_or(mode);
                let (w, h) = self.draft.mode.size();
                if [w, h].iter().any(|px| {
                    let logical = f64::from(*px) / self.draft.scale;
                    (logical - logical.round()).abs() > 0.001
                }) {
                    self.draft.scale = 1.0;
                }
            }
            Choice::Mode(mode) => self.draft.mode = mode,
            Choice::Scale(scale) => self.draft.scale = scale,
            Choice::Transform(transform) => self.draft.transform = transform,
        }
    }
}

fn orientation(transform: u8) -> &'static str {
    match transform {
        0 => "Landscape",
        1 => "Portrait",
        2 => "Landscape 180°",
        3 => "Portrait 270°",
        _ => "Mirrored",
    }
}

fn is_mirror(info: &MonitorInfo) -> bool {
    !matches!(info.mirror_of.as_str(), "" | "none")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Resolution,
    RefreshRate,
    Scale,
    Orientation,
}

impl Field {
    pub const ALL: [Self; 4] = [
        Self::Resolution,
        Self::RefreshRate,
        Self::Scale,
        Self::Orientation,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Resolution => "Resolution",
            Self::RefreshRate => "Refresh rate",
            Self::Scale => "Scale",
            Self::Orientation => "Orientation",
        }
    }
}

#[derive(Clone, Copy)]
enum Choice {
    Mode(Mode),
    Scale(f64),
    Transform(u8),
}

impl Choice {
    fn label(self, field: Field) -> String {
        match self {
            Self::Mode(mode) if field == Field::Resolution => mode.resolution(),
            Self::Mode(mode) => mode.rate(),
            Self::Scale(scale) => format!("{:.0}%", scale * 100.0),
            Self::Transform(transform) => orientation(transform).into(),
        }
    }

    fn is_selected(self, display: &Display, field: Field) -> bool {
        let draft = display.draft;
        match self {
            Self::Mode(mode) if field == Field::Resolution => mode.size() == draft.mode.size(),
            Self::Mode(mode) => mode == draft.mode,
            Self::Scale(scale) => scale == draft.scale,
            Self::Transform(transform) => transform == draft.transform,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    fn right(self) -> i32 {
        self.x.saturating_add(self.w)
    }
    fn bottom(self) -> i32 {
        self.y.saturating_add(self.h)
    }

    pub fn contains(self, (x, y): (f64, f64)) -> bool {
        x >= f64::from(self.x)
            && x < f64::from(self.right())
            && y >= f64::from(self.y)
            && y < f64::from(self.bottom())
    }

    fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let w = self.right().min(other.right()).saturating_sub(x).max(0);
        let h = self.bottom().min(other.bottom()).saturating_sub(y).max(0);
        Self { x, y, w, h }
    }

    fn overlaps(self, other: Self) -> bool {
        let overlap = self.intersection(other);
        overlap.w > 0 && overlap.h > 0
    }
}

pub struct Layout {
    pub width: i32,
    pub canvas: Area,
    pub heading: Area,
    pub fields: [Area; 4],
    pub footer: Area,
    pub identify: Area,
    pub apply: Area,
    pub height: i32,
}

impl Layout {
    pub fn new(width: i32, pad: i32, control_height: i32) -> Self {
        let pad = pad.clamp(4, (width / 8).max(4));
        let w = (width - pad * 2).max(1);
        let control = control_height.clamp(22, 40);
        let canvas = Area {
            x: pad,
            y: pad,
            w,
            h: 174,
        };
        let heading = Area {
            x: pad,
            y: canvas.bottom() + 5,
            w,
            h: 24,
        };
        let fields = std::array::from_fn(|i| Area {
            x: pad + (w * i as i32) / 4,
            y: heading.bottom() + 16,
            w: ((w * (i as i32 + 1)) / 4 - (w * i as i32) / 4 - if i < 3 { 6 } else { 0 }).max(1),
            h: control,
        });
        let footer = Area {
            x: pad,
            y: fields[0].bottom() + 8,
            w,
            h: control + 6,
        };
        let apply = Area {
            x: footer.right() - 68,
            y: footer.y + 6,
            w: 68,
            h: control,
        };
        let identify = Area {
            x: apply.x - 82,
            y: apply.y,
            w: 76,
            h: control,
        };
        // Enough space inside the panel for six menu rows above or below a field.
        Self {
            width,
            canvas,
            heading,
            fields,
            footer,
            identify,
            apply,
            height: footer.bottom() + pad + 18,
        }
    }
}

#[derive(Clone, Copy)]
struct Map {
    zoom: f64,
    x: f64,
    y: f64,
}

impl Map {
    fn fit(displays: &[Display], canvas: Area) -> Self {
        let min_x = displays.iter().map(|d| d.draft.x).min().unwrap_or(0);
        let min_y = displays.iter().map(|d| d.draft.y).min().unwrap_or(0);
        let max_x = displays
            .iter()
            .map(|d| d.draft.bounds().right())
            .max()
            .unwrap_or(1);
        let max_y = displays
            .iter()
            .map(|d| d.draft.bounds().bottom())
            .max()
            .unwrap_or(1);
        let span_x = f64::from(max_x.saturating_sub(min_x).max(1));
        let span_y = f64::from(max_y.saturating_sub(min_y).max(1));
        let zoom = ((f64::from(canvas.w - 28) / span_x).min(f64::from(canvas.h - 28) / span_y))
            .clamp(0.000001, 0.15);
        Self {
            zoom,
            x: f64::from(canvas.x) + (f64::from(canvas.w) - span_x * zoom) * 0.5
                - f64::from(min_x) * zoom,
            y: f64::from(canvas.y) + (f64::from(canvas.h) - span_y * zoom) * 0.5
                - f64::from(min_y) * zoom,
        }
    }

    fn project(self, bounds: Area) -> Area {
        let x = (self.x + f64::from(bounds.x) * self.zoom).round() as i32;
        let y = (self.y + f64::from(bounds.y) * self.zoom).round() as i32;
        let right = (self.x + f64::from(bounds.right()) * self.zoom).round() as i32;
        let bottom = (self.y + f64::from(bounds.bottom()) * self.zoom).round() as i32;
        Area {
            x,
            y,
            w: right.saturating_sub(x).max(1),
            h: bottom.saturating_sub(y).max(1),
        }
    }
}

struct Drag {
    pointer: (f64, f64),
    start: DisplayState,
    map: Map,
}

struct Dropdown {
    field: Field,
    offset: usize,
}

pub struct Menu {
    pub field: Field,
    pub rows: Vec<(Area, String, bool)>,
    pub above: bool,
    pub below: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    Identify,
    Apply,
    Keep,
    Revert,
}

#[derive(Clone, Copy)]
pub enum TrialView {
    Applying,
    Confirm(u64),
    Restoring,
}

pub struct DisplaySettings {
    pub displays: Vec<Display>,
    pub selected: usize,
    pub message: String,
    drag: Option<Drag>,
    dropdown: Option<Dropdown>,
}

impl DisplaySettings {
    pub fn new(monitors: Vec<MonitorInfo>) -> Self {
        let displays: Vec<_> = monitors
            .into_iter()
            .enumerate()
            .map(|(index, info)| Display::new(info, index + 1))
            .collect();
        let selected = displays
            .iter()
            .position(|display| display.info.focused)
            .unwrap_or(0);
        Self {
            displays,
            selected,
            message: String::new(),
            drag: None,
            dropdown: None,
        }
    }

    pub fn refresh(&mut self, monitors: Vec<MonitorInfo>) -> bool {
        let same = monitors.len() == self.displays.len()
            && monitors.iter().zip(&self.displays).all(|(info, display)| {
                info.name == display.info.name
                    && info.id == display.info.id
                    && info.model == display.info.model
                    && info.mirror_of == display.info.mirror_of
                    && info.available_modes == display.info.available_modes
                    && display.current.matches(DisplayState::from_monitor(info))
            });
        if same {
            return false;
        }
        let message = if self.dirty() {
            "Displays changed externally; pending edits were discarded.".to_owned()
        } else {
            self.message.clone()
        };
        self.reset(monitors);
        self.message = message;
        true
    }

    pub fn reset(&mut self, monitors: Vec<MonitorInfo>) {
        let selected_name = self.selected().map(|display| display.info.name.clone());
        *self = Self::new(monitors);
        if let Some(index) = self
            .displays
            .iter()
            .position(|d| Some(&d.info.name) == selected_name.as_ref())
        {
            self.selected = index;
        }
    }

    pub fn selected(&self) -> Option<&Display> {
        self.displays.get(self.selected)
    }

    pub fn dirty(&self) -> bool {
        self.displays
            .iter()
            .any(|display| display.current != display.draft)
    }

    pub fn changes(&self) -> Result<Vec<Change>> {
        let mut changes = Vec::new();
        // Keep the whole map in place when a display moves or changes logical size.
        // Otherwise a compositor rule using `auto` can move an unedited neighbor.
        let set_position = self
            .displays
            .iter()
            .any(|display| display.current.bounds() != display.draft.bounds());
        for (i, display) in self.displays.iter().enumerate() {
            let (before, after) = (display.current, display.draft);
            let mirrored = is_mirror(&display.info);
            if before == after && (!set_position || mirrored) {
                continue;
            }
            ensure!(!mirrored, "Mirrored displays cannot be edited");
            if before.bounds() != after.bounds() {
                for (j, other) in self.displays.iter().enumerate() {
                    if i != j
                        && !is_mirror(&other.info)
                        && after.bounds().overlaps(other.draft.bounds())
                    {
                        anyhow::bail!(
                            "{} overlaps {}; drag them apart",
                            display.info.name,
                            other.info.name
                        );
                    }
                }
            }
            changes.push(Change {
                name: display.info.name.clone(),
                before,
                after,
                set_position,
            });
        }
        Ok(changes)
    }

    pub fn cards(&self, layout: &Layout) -> Vec<(usize, Area)> {
        let map = self
            .drag
            .as_ref()
            .map_or_else(|| Map::fit(&self.displays, layout.canvas), |drag| drag.map);
        (0..self.displays.len())
            .filter(|index| *index != self.selected)
            .chain(std::iter::once(self.selected).filter(|&index| index < self.displays.len()))
            .filter_map(|index| {
                let rect = map
                    .project(self.displays[index].draft.bounds())
                    .intersection(layout.canvas);
                (rect.w > 0 && rect.h > 0).then_some((index, rect))
            })
            .collect()
    }

    pub fn menu(&self, layout: &Layout) -> Option<Menu> {
        let dropdown = self.dropdown.as_ref()?;
        let display = self.selected()?;
        let choices = display.choices(dropdown.field);
        let index = Field::ALL
            .iter()
            .position(|field| *field == dropdown.field)?;
        let slot = layout.fields[index];
        let visible = choices.len().min(DROP_ROWS);
        let row_h = 21;
        let menu_h = visible as i32 * row_h;
        let y = if slot.bottom() + 2 + menu_h < layout.height {
            slot.bottom() + 2
        } else {
            slot.y - menu_h - 2
        };
        let rows = choices
            .iter()
            .skip(dropdown.offset)
            .take(visible)
            .enumerate()
            .map(|(i, choice)| {
                (
                    Area {
                        x: slot.x,
                        y: y + i as i32 * row_h,
                        w: slot.w,
                        h: row_h,
                    },
                    choice.label(dropdown.field),
                    choice.is_selected(display, dropdown.field),
                )
            })
            .collect();
        Some(Menu {
            field: dropdown.field,
            rows,
            above: dropdown.offset > 0,
            below: dropdown.offset + visible < choices.len(),
        })
    }

    pub fn press(
        &mut self,
        layout: &Layout,
        point: (f64, f64),
        busy: bool,
        confirming: bool,
    ) -> Action {
        if confirming {
            if layout.apply.contains(point) {
                return Action::Keep;
            }
            if layout.identify.contains(point) {
                return Action::Revert;
            }
        }
        if let Some(menu) = self.menu(layout) {
            if let Some(row) = menu
                .rows
                .iter()
                .position(|(area, _, _)| area.contains(point))
            {
                if let Some(dropdown) = self.dropdown.take() {
                    let display = &mut self.displays[self.selected];
                    if let Some(choice) = display
                        .choices(dropdown.field)
                        .get(dropdown.offset + row)
                        .copied()
                    {
                        display.select(dropdown.field, choice);
                        self.message.clear();
                    }
                }
                return Action::None;
            }
            self.dropdown = None;
            if Field::ALL
                .iter()
                .zip(layout.fields)
                .any(|(field, area)| *field == menu.field && area.contains(point))
            {
                return Action::None;
            }
        }
        if point.0 < 0.0
            || point.1 < 0.0
            || point.0 >= f64::from(layout.width)
            || point.1 >= f64::from(layout.height)
        {
            return Action::Close;
        }
        if busy {
            return Action::None;
        }
        if layout.identify.contains(point) {
            return Action::Identify;
        }
        if layout.apply.contains(point) && self.dirty() {
            return Action::Apply;
        }
        if let Some((index, _)) = self
            .cards(layout)
            .into_iter()
            .rev()
            .find(|(_, area)| area.contains(point))
        {
            self.selected = index;
            self.message.clear();
            let display = &self.displays[index];
            if !is_mirror(&display.info) {
                self.drag = Some(Drag {
                    pointer: point,
                    start: display.draft,
                    map: Map::fit(&self.displays, layout.canvas),
                });
            }
        } else if let Some(display) = self.selected().filter(|d| !is_mirror(&d.info))
            && let Some((field, _)) = Field::ALL
                .into_iter()
                .zip(layout.fields)
                .find(|(_, area)| area.contains(point))
        {
            let choices = display.choices(field);
            let active = choices
                .iter()
                .position(|choice| choice.is_selected(display, field))
                .unwrap_or(0);
            let offset = active
                .saturating_sub(DROP_ROWS / 2)
                .min(choices.len().saturating_sub(DROP_ROWS));
            self.dropdown = Some(Dropdown { field, offset });
        }
        Action::None
    }

    pub fn dismiss_menu(&mut self) -> bool {
        self.dropdown.take().is_some()
    }

    pub fn scroll(&mut self, layout: &Layout, point: (f64, f64), delta: i32) -> bool {
        if delta == 0
            || !self.menu(layout).is_some_and(|menu| {
                menu.rows.iter().any(|(area, _, _)| area.contains(point))
                    || Field::ALL
                        .into_iter()
                        .zip(layout.fields)
                        .any(|(field, area)| field == menu.field && area.contains(point))
            })
        {
            return false;
        }
        let dropdown = self.dropdown.as_mut().unwrap();
        let len = self.displays[self.selected].choices(dropdown.field).len();
        let next = dropdown
            .offset
            .saturating_add_signed(delta.signum() as isize)
            .min(len.saturating_sub(DROP_ROWS));
        let changed = next != dropdown.offset;
        dropdown.offset = next;
        changed
    }

    pub fn motion(&mut self, point: (f64, f64)) -> bool {
        let Some(drag) = &self.drag else {
            return false;
        };
        let dx = point.0 - drag.pointer.0;
        let dy = point.1 - drag.pointer.1;
        let mut draft = drag.start;
        if dx.hypot(dy) >= 3.0 {
            draft.x = draft.x.saturating_add((dx / drag.map.zoom).round() as i32);
            draft.y = draft.y.saturating_add((dy / drag.map.zoom).round() as i32);
            let neighbors = self
                .displays
                .iter()
                .enumerate()
                .filter(|(i, d)| *i != self.selected && !is_mirror(&d.info))
                .map(|(_, d)| d.draft.bounds());
            let (x, y) = snap(
                draft.bounds(),
                neighbors,
                (SNAP_PIXELS / drag.map.zoom).round() as i32,
            );
            draft.x = x;
            draft.y = y;
        }
        let selected = &mut self.displays[self.selected].draft;
        let changed = *selected != draft;
        *selected = draft;
        changed
    }

    pub fn release(&mut self, layout: &Layout, point: (f64, f64)) -> bool {
        if let Some(drag) = self.drag.take() {
            if !layout.canvas.contains(point) {
                self.displays[self.selected].draft = drag.start;
            }
            return true;
        }
        false
    }
}

fn snap(rect: Area, neighbors: impl Iterator<Item = Area>, threshold: i32) -> (i32, i32) {
    let (mut best_x, mut best_y) = (None, None);
    fn nearest(best: &mut Option<i32>, offset: i32, threshold: i32) {
        if offset.unsigned_abs() <= threshold as u32
            && best.is_none_or(|prev| offset.unsigned_abs() < prev.unsigned_abs())
        {
            *best = Some(offset);
        }
    }
    for other in neighbors {
        if rect.y <= other.bottom().saturating_add(threshold)
            && other.y <= rect.bottom().saturating_add(threshold)
        {
            for from in [rect.x, rect.right()] {
                for to in [other.x, other.right()] {
                    nearest(&mut best_x, to.saturating_sub(from), threshold);
                }
            }
        }
        if rect.x <= other.right().saturating_add(threshold)
            && other.x <= rect.right().saturating_add(threshold)
        {
            for from in [rect.y, rect.bottom()] {
                for to in [other.y, other.bottom()] {
                    nearest(&mut best_y, to.saturating_sub(from), threshold);
                }
            }
        }
    }
    (
        rect.x.saturating_add(best_x.unwrap_or(0)),
        rect.y.saturating_add(best_y.unwrap_or(0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, x: i32, y: i32) -> MonitorInfo {
        MonitorInfo {
            id: x,
            name: name.into(),
            x,
            y,
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1920,
            height: 1080,
            refresh_rate: 59.999,
            scale: 1.0,
            transform: 0,
            mirror_of: String::new(),
            focused: x == 0,
            dpms_status: true,
            active_workspace: 1,
            available_modes: [
                "1920x1080@60.00Hz",
                "1920x1080@59.94Hz",
                "1920x1080@48.00Hz",
                "1280x720@50.00Hz",
            ]
            .map(str::to_owned)
            .to_vec(),
        }
    }

    #[test]
    fn refresh_list_uses_the_selected_resolution_and_keeps_nearby_but_distinct_rates() {
        let mut display = Display::new(monitor("DP-1", 0, 0), 1);
        assert_eq!(
            display
                .choices(Field::RefreshRate)
                .iter()
                .map(|choice| choice.label(Field::RefreshRate))
                .collect::<Vec<_>>(),
            ["60 Hz", "59.94 Hz", "48 Hz"]
        );
        let choices = display.choices(Field::Resolution);
        display.select(Field::Resolution, choices[1]);
        assert_eq!(display.label(Field::Resolution), "1280 × 720");
        assert_eq!(display.label(Field::RefreshRate), "50 Hz");
        assert_eq!(display.choices(Field::RefreshRate).len(), 1);
    }

    #[test]
    fn drag_snaps_to_neighbor_edges_in_logical_coordinates() {
        let area = Area {
            x: -1916,
            y: 4,
            w: 1920,
            h: 1080,
        };
        let neighbor = Area {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(snap(area, [neighbor].into_iter(), 10), (-1920, 0));
        assert_eq!(
            snap(Area { y: 2000, ..area }, [neighbor].into_iter(), 10),
            (-1916, 2000)
        );
    }

    #[test]
    fn changes_reject_overlap_after_rotation_but_accept_adjacent_screens() {
        let mut settings =
            DisplaySettings::new(vec![monitor("eDP-1", 0, 0), monitor("DP-1", 0, 1080)]);
        settings.displays[0].draft.transform = 1;
        assert_eq!(settings.displays[0].draft.size(), (1080, 1920));
        assert!(settings.changes().is_err());
        settings.displays[1].draft.y = 1920;
        assert_eq!(settings.changes().unwrap().len(), 2);
    }

    #[test]
    fn refresh_preserves_drafts_on_focus_change_and_invalidates_them_on_hotplug() {
        let live = monitor("eDP-1", 0, 0);
        let mut settings = DisplaySettings::new(vec![live.clone()]);
        settings.displays[0].draft.x = 100;
        assert!(!settings.refresh(vec![MonitorInfo {
            focused: false,
            ..live.clone()
        }]));
        assert!(settings.dirty());
        assert!(settings.refresh(vec![live, monitor("DP-1", 1920, 0)]));
        assert!(!settings.dirty());
        assert!(settings.message.contains("discarded"));
    }

    #[test]
    fn dragging_stages_a_move_until_apply_and_cancels_an_outside_drop() {
        let mut settings =
            DisplaySettings::new(vec![monitor("eDP-1", 0, 0), monitor("DP-1", 1920, 0)]);
        let layout = Layout::new(560, 12, 26);
        let second = settings
            .cards(&layout)
            .into_iter()
            .find(|(index, _)| *index == 1)
            .unwrap()
            .1;
        let start = (
            f64::from(second.x + second.w / 2),
            f64::from(second.y + second.h / 2),
        );
        let end = (start.0 + 28.0, start.1);
        assert!(settings.press(&layout, start, false, false) == Action::None);
        assert!(settings.motion(end));
        assert!(settings.release(&layout, (-1.0, -1.0)));
        assert!(!settings.dirty());
        settings.press(&layout, start, false, false);
        settings.motion(end);
        assert!(settings.release(&layout, end));
        let changes = settings.changes().unwrap();
        assert!(changes.iter().all(|change| change.set_position));
        let change = changes.iter().find(|change| change.name == "DP-1").unwrap();
        assert_eq!((change.name.as_str(), change.before.x), ("DP-1", 1920));
        assert!(change.after.x > change.before.x);
        let apply = (f64::from(layout.apply.x + 1), f64::from(layout.apply.y + 1));
        assert!(settings.press(&layout, apply, false, false) == Action::Apply);
    }

    #[test]
    fn dropdown_edit_is_staged_for_the_selected_display() {
        let mut settings =
            DisplaySettings::new(vec![monitor("eDP-1", 0, 0), monitor("DP-1", 1920, 0)]);
        settings.selected = 1;
        let layout = Layout::new(560, 12, 26);
        let field = layout.fields[1];
        settings.press(
            &layout,
            (f64::from(field.x + 1), f64::from(field.y + 1)),
            false,
            false,
        );
        let menu = settings.menu(&layout).unwrap();
        let (row, label, _) = &menu.rows[2];
        assert_eq!(label, "48 Hz");
        settings.press(
            &layout,
            (f64::from(row.x + 1), f64::from(row.y + 1)),
            false,
            false,
        );
        assert!(settings.menu(&layout).is_none());
        let changes = settings.changes().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "DP-1");
        assert_eq!(changes[0].after.mode.mhz, 48000);
    }
}
