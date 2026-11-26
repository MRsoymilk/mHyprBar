use super::{primitives::*, *};
use crate::modules::monitor::{
    popup::MonitorPopupConfig,
    settings::{Area, DisplaySettings, Field, Layout, TrialView},
};

const ACCENT: [u8; 4] = [162, 211, 178, 255];
const CONTROL: [u8; 4] = [39, 46, 48, 255];

struct Panel<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}

impl Panel<'_> {
    fn rect(&self, a: Area) -> Rect {
        Rect {
            x: self.x + a.x,
            y: self.y + a.y,
            w: a.w,
            h: a.h,
        }
    }

    fn fill(&mut self, area: Area, color: [u8; 4]) {
        let rect = self.rect(area);
        fill_rect(self.pixels, self.width, self.height, rect, color);
    }

    fn border(&mut self, area: Area, color: [u8; 4]) {
        let rect = self.rect(area);
        draw_rect_border(self.pixels, self.width, self.height, rect, color, 1);
    }

    fn text(
        &mut self,
        renderer: &mut Renderer,
        area: Area,
        text: &str,
        style: &ModuleStyle,
        center: bool,
    ) -> Result<()> {
        if area.w < 2 || f64::from(area.h) < f64::from(style.font_size) {
            return Ok(());
        }
        renderer.draw_text_content_aligned(
            self.pixels,
            self.width,
            self.height,
            self.rect(area),
            text,
            style,
            0,
            0,
            center.then_some(cosmic_text::Align::Center),
        )
    }
}

impl Renderer {
    pub(super) fn draw_display_settings(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        cfg: &MonitorPopupConfig,
        display: &DisplaySettings,
        layout: &Layout,
        origin: (i32, i32),
        trial: Option<TrialView>,
    ) -> Result<()> {
        let mut panel = Panel {
            pixels: canvas,
            width,
            height,
            x: origin.0,
            y: origin.1,
        };
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let highlight = cfg.selected_background_rgba()?;
        let bounds = Area {
            x: 0,
            y: 0,
            w: cfg.width,
            h: layout.height,
        };
        panel.fill(bounds, cfg.style.background_rgba()?);
        panel.border(bounds, border);
        panel.fill(layout.canvas, [20, 24, 25, 255]);
        panel.border(layout.canvas, separator);
        for x in (layout.canvas.x + 8..layout.canvas.x + layout.canvas.w - 4).step_by(10) {
            for y in (layout.canvas.y + 8..layout.canvas.y + layout.canvas.h - 4).step_by(10) {
                panel.fill(Area { x, y, w: 1, h: 1 }, [40, 48, 50, 255]);
            }
        }

        let body = ModuleStyle {
            font_size: cfg.style.font_size.clamp(10.0, 16.0),
            ..cfg.style.clone()
        };
        let muted = ModuleStyle {
            font_size: (body.font_size - 2.0).max(9.0),
            foreground: "#AAB4B0".into(),
            ..body.clone()
        };
        let accent = ModuleStyle {
            foreground: "#A2D3B2".into(),
            ..body.clone()
        };
        let small_accent = ModuleStyle {
            font_size: muted.font_size,
            ..accent.clone()
        };
        panel.text(
            self,
            Area {
                x: layout.canvas.x + 7,
                y: layout.canvas.y + 3,
                w: layout.canvas.w - 14,
                h: 14,
            },
            "LAYOUT",
            &muted,
            false,
        )?;

        for (i, rect) in display.cards(layout) {
            let item = &display.displays[i];
            let selected = i == display.selected;
            panel.fill(
                rect,
                if selected {
                    highlight
                } else {
                    [36, 43, 46, 255]
                },
            );
            panel.border(rect, if selected { ACCENT } else { border });
            let number = ModuleStyle {
                font_size: (rect.h as f32 * 0.42).clamp(10.0, 26.0),
                ..body.clone()
            };
            let digits = item.number.to_string();
            if rect.h >= 35 {
                let h = (number.font_size as i32 + 4).min(rect.h - 14);
                let y = rect.y + (rect.h - h - 13) / 2;
                panel.text(self, Area { y, h, ..rect }, &digits, &number, true)?;
                panel.text(
                    self,
                    Area {
                        y: y + h,
                        h: 13,
                        ..rect
                    },
                    &item.info.name,
                    if selected { &small_accent } else { &muted },
                    true,
                )?;
            } else {
                panel.text(self, rect, &digits, &number, true)?;
            }
        }

        if let Some(item) = display.selected() {
            let heading = layout.heading;
            let number = Area {
                w: 22,
                h: 20,
                y: heading.y + 2,
                ..heading
            };
            panel.fill(number, highlight);
            panel.text(self, number, &item.number.to_string(), &accent, true)?;
            panel.text(
                self,
                Area {
                    x: heading.x + 27,
                    w: (heading.w - 145).max(1),
                    ..heading
                },
                &format!("{}  {}", item.info.name, item.info.model),
                &body,
                false,
            )?;
            panel.text(
                self,
                Area {
                    x: heading.x + heading.w - 108,
                    w: 108,
                    ..heading
                },
                &format!("Pos: {}, {}", item.draft.x, item.draft.y),
                &muted,
                true,
            )?;
            let menu = display.menu(layout);
            for (field, rect) in Field::ALL.into_iter().zip(layout.fields) {
                let opened = menu.as_ref().is_some_and(|menu| menu.field == field);
                panel.text(
                    self,
                    Area {
                        y: rect.y - 15,
                        h: 14,
                        ..rect
                    },
                    field.title(),
                    &muted,
                    false,
                )?;
                panel.fill(rect, if opened { highlight } else { CONTROL });
                panel.border(rect, if opened { ACCENT } else { border });
                panel.text(
                    self,
                    Area {
                        x: rect.x + 7,
                        w: (rect.w - 22).max(1),
                        ..rect
                    },
                    &item.label(field),
                    &body,
                    false,
                )?;
                panel.text(
                    self,
                    Area {
                        x: rect.x + rect.w - 15,
                        w: 14,
                        ..rect
                    },
                    if opened { "⌃" } else { "⌄" },
                    &muted,
                    true,
                )?;
            }
        } else {
            panel.text(self, layout.heading, "No active displays", &body, false)?;
        }

        let footer = layout.footer;
        panel.fill(Area { h: 1, ..footer }, separator);
        let (label, left, right, enable_apply) = match trial {
            Some(TrialView::Applying) => (
                "Applying display settings…".into(),
                "Identify",
                "Apply",
                false,
            ),
            Some(TrialView::Confirm(seconds)) => (
                format!("Keep these display settings?  {seconds}s"),
                "Revert",
                "Keep",
                true,
            ),
            Some(TrialView::Restoring) => (
                "Restoring display settings…".into(),
                "Identify",
                "Apply",
                false,
            ),
            None => {
                let message = if !display.message.is_empty() {
                    display.message.clone()
                } else if display.dirty() {
                    "Unsaved changes".into()
                } else {
                    "Drag to rearrange displays".into()
                };
                (message, "Identify", "Apply", display.dirty())
            }
        };
        self.draw_wrapped_text_content(
            panel.pixels,
            width,
            height,
            panel.rect(Area {
                y: footer.y + 6,
                w: (layout.identify.x - footer.x - 7).max(1),
                h: footer.h - 6,
                ..footer
            }),
            &label,
            &muted,
        )?;
        panel.fill(layout.identify, CONTROL);
        panel.border(layout.identify, border);
        panel.text(
            self,
            layout.identify,
            left,
            if trial.is_none() || enable_apply {
                &body
            } else {
                &muted
            },
            true,
        )?;
        panel.fill(layout.apply, if enable_apply { ACCENT } else { CONTROL });
        if !enable_apply {
            panel.border(layout.apply, border);
        }
        let dark = ModuleStyle {
            foreground: "#15251C".into(),
            ..body.clone()
        };
        panel.text(
            self,
            layout.apply,
            right,
            if enable_apply { &dark } else { &muted },
            true,
        )?;

        if let Some(menu) = display.menu(layout) {
            for (row, label, active) in &menu.rows {
                panel.fill(
                    *row,
                    if *active {
                        highlight
                    } else {
                        [31, 38, 39, 255]
                    },
                );
                panel.text(
                    self,
                    Area {
                        x: row.x + 7,
                        w: (row.w - 22).max(1),
                        ..*row
                    },
                    label,
                    if *active { &accent } else { &body },
                    false,
                )?;
            }
            if let (Some((first, ..)), Some((last, ..))) = (menu.rows.first(), menu.rows.last()) {
                panel.border(
                    Area {
                        h: last.y + last.h - first.y,
                        ..*first
                    },
                    border,
                );
                if menu.above {
                    panel.text(
                        self,
                        Area {
                            x: first.x + first.w - 15,
                            w: 14,
                            ..*first
                        },
                        "⌃",
                        &muted,
                        true,
                    )?;
                }
                if menu.below {
                    panel.text(
                        self,
                        Area {
                            x: last.x + last.w - 15,
                            w: 14,
                            ..*last
                        },
                        "⌄",
                        &muted,
                        true,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn draw_identify_badge(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        number: usize,
    ) -> Result<()> {
        canvas.fill(0);
        let area = Area {
            x: 0,
            y: 0,
            w: width as i32,
            h: height as i32,
        };
        let mut panel = Panel {
            pixels: canvas,
            width,
            height,
            x: 0,
            y: 0,
        };
        panel.fill(area, [17, 24, 22, 238]);
        panel.border(area, ACCENT);
        let style = ModuleStyle {
            font_size: (height as f32 * 0.61).min(70.0),
            foreground: "#FFFFFF".into(),
            font_family: "sans-serif".into(),
            ..ModuleStyle::default()
        };
        panel.text(self, area, &number.to_string(), &style, true)
    }
}
