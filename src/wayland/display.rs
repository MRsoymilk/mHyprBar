use smithay_client_toolkit::compositor::Region;

use super::{output_management::RequestKind, *};
use crate::modules::monitor::settings::{Action, Change, DisplayState, TrialView};

const CONFIRM_TIME: Duration = Duration::from_secs(12);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(150);
const IDENTIFY_TIME: Duration = Duration::from_secs(3);
const BADGE_SIZE: (u32, u32) = (144, 104);

enum Phase {
    Applying {
        accepted: bool,
    },
    Confirming,
    Restoring {
        accepted: bool,
        message: &'static str,
    },
}

pub(super) struct MonitorConfigTrial {
    ticket: u64,
    changes: Vec<Change>,
    phase: Phase,
    deadline: Instant,
    next_check: Instant,
}

pub(super) struct IdentifyBadge {
    pub(super) layer: LayerSurface,
    pub(super) output: wl_output::WlOutput,
    number: usize,
}

impl App {
    pub(super) fn display_trial_view(&self) -> Option<TrialView> {
        let trial = self.monitor_config_trial.as_ref()?;
        Some(match trial.phase {
            Phase::Applying { .. } => TrialView::Applying,
            Phase::Confirming => TrialView::Confirm(
                trial
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .as_secs_f64()
                    .ceil() as u64,
            ),
            Phase::Restoring { .. } => TrialView::Restoring,
        })
    }

    pub(super) fn display_timeout(&self) -> Option<Duration> {
        let trial = self
            .monitor_config_trial
            .as_ref()
            .map(|trial| trial.next_check.min(trial.deadline));
        trial
            .into_iter()
            .chain(self.monitor_identify_until)
            .min()
            .map(|next| next.saturating_duration_since(Instant::now()))
    }

    pub(super) fn sync_monitor_settings(&mut self) {
        if self.monitor_config_trial.is_some() {
            return;
        }
        if let Some(settings) = self
            .monitor_popup
            .as_mut()
            .and_then(|popup| popup.model.settings.as_mut())
        {
            match hyprland::monitor_infos() {
                Ok(monitors) => {
                    if settings.refresh(monitors) {
                        self.monitor_identify.clear();
                        self.monitor_identify_until = None;
                        self.draw_monitor_popup();
                    }
                }
                Err(error) => eprintln!("mhyprbar: display settings refresh failed: {error:#}"),
            }
        }
    }

    fn start_monitor_config(&mut self) -> Result<()> {
        let outputs = self
            .output_management
            .as_ref()
            .context("Hyprland did not expose output management")?;
        let settings = self
            .monitor_popup
            .as_mut()
            .and_then(|popup| popup.model.settings.as_mut())
            .context("Display settings are closed")?;
        if settings.refresh(hyprland::monitor_infos()?) {
            return Ok(());
        }
        let changes = settings.changes()?;
        let Some(ticket) = outputs.submit(&changes, RequestKind::Apply)? else {
            return Ok(());
        };
        let now = Instant::now();
        self.monitor_config_trial = Some(MonitorConfigTrial {
            ticket,
            changes,
            phase: Phase::Applying { accepted: false },
            deadline: now + REQUEST_TIMEOUT,
            next_check: now + POLL_INTERVAL,
        });
        Ok(())
    }

    fn finish_monitor_config(&mut self, message: &str) {
        self.monitor_config_trial = None;
        if let Some(settings) = self
            .monitor_popup
            .as_mut()
            .and_then(|popup| popup.model.settings.as_mut())
        {
            if let Ok(monitors) = hyprland::monitor_infos() {
                settings.reset(monitors);
            }
            settings.message = message.into();
        }
        if self.modules.force_refresh("monitor") {
            self.draw_all();
        }
        self.draw_monitor_popup();
    }

    pub(super) fn restore_monitor_config(&mut self, message: &'static str) {
        let Some(trial) = &mut self.monitor_config_trial else {
            return;
        };
        if matches!(trial.phase, Phase::Restoring { .. }) {
            return;
        }
        let result = self
            .output_management
            .as_ref()
            .context("Hyprland output management disconnected")
            .and_then(|outputs| outputs.submit(&trial.changes, RequestKind::Restore));
        match result {
            Ok(Some(ticket)) => {
                let now = Instant::now();
                trial.ticket = ticket;
                trial.phase = Phase::Restoring {
                    accepted: false,
                    message,
                };
                trial.deadline = now + REQUEST_TIMEOUT;
                trial.next_check = now + POLL_INTERVAL;
            }
            Ok(None) => self.finish_monitor_config("The edited displays are no longer connected"),
            Err(error) => {
                eprintln!("mhyprbar: could not restore previous display settings: {error:#}");
                self.finish_monitor_config("Could not restore previous display settings");
            }
        }
    }

    pub(super) fn monitor_config_result(
        &mut self,
        ticket: u64,
        kind: RequestKind,
        result: Result<(), &'static str>,
    ) {
        let Some(trial) = &mut self.monitor_config_trial else {
            return;
        };
        if trial.ticket != ticket {
            return;
        }
        match (kind, &mut trial.phase) {
            (RequestKind::Apply, Phase::Applying { accepted })
            | (RequestKind::Restore, Phase::Restoring { accepted, .. }) => match result {
                Ok(()) => {
                    *accepted = true;
                    trial.next_check = Instant::now();
                }
                Err(message) if matches!(kind, RequestKind::Restore) => {
                    eprintln!("mhyprbar: could not restore previous display settings: {message}");
                    self.finish_monitor_config(message);
                }
                Err(message) => self.finish_monitor_config(message),
            },
            _ => {}
        }
    }

    pub(super) fn update_display_timers(&mut self) {
        let now = Instant::now();
        if self
            .monitor_identify_until
            .is_some_and(|deadline| now >= deadline)
        {
            self.monitor_identify.clear();
            self.monitor_identify_until = None;
        }
        let Some(trial) = &mut self.monitor_config_trial else {
            return;
        };
        if now < trial.next_check && now < trial.deadline {
            return;
        }
        trial.next_check = now + POLL_INTERVAL;
        match trial.phase {
            Phase::Applying { accepted } => {
                if accepted
                    && hyprland::monitor_infos().is_ok_and(|monitors| {
                        trial
                            .changes
                            .iter()
                            .all(|change| change.is_applied(&monitors))
                    })
                {
                    trial.phase = Phase::Confirming;
                    trial.deadline = now + CONFIRM_TIME;
                    trial.next_check = now + Duration::from_secs(1);
                    self.draw_monitor_popup();
                } else if now >= trial.deadline {
                    self.restore_monitor_config(
                        "Hyprland did not apply the requested settings; restored",
                    );
                    self.draw_monitor_popup();
                }
            }
            Phase::Confirming if now >= trial.deadline => {
                self.restore_monitor_config("Confirmation timed out; previous settings restored");
                self.draw_monitor_popup();
            }
            Phase::Confirming => {
                trial.next_check = now + Duration::from_secs(1);
                self.draw_monitor_popup();
            }
            Phase::Restoring { accepted, message } => {
                if accepted
                    && hyprland::monitor_infos().is_ok_and(|monitors| {
                        trial.changes.iter().all(|change| {
                            monitors
                                .iter()
                                .find(|monitor| monitor.name == change.name)
                                .is_none_or(|monitor| {
                                    change.before.matches(DisplayState::from_monitor(monitor))
                                })
                        })
                    })
                {
                    self.finish_monitor_config(message);
                } else if now >= trial.deadline {
                    eprintln!("mhyprbar: timed out waiting for previous display settings");
                    self.finish_monitor_config("Could not verify the previous display settings");
                }
            }
        }
    }

    pub(super) fn handle_monitor_pointer(&mut self, qh: &QueueHandle<Self>, event: &PointerEvent) {
        let trial = self.display_trial_view();
        let Some(popup) = self.monitor_popup.as_mut() else {
            return;
        };
        let point = (
            event.position.0 - popup.panel_x,
            event.position.1 - popup.panel_y,
        );
        let layout = popup.model.display_layout();
        if let Some(settings) = &mut popup.model.settings {
            let mut redraw = true;
            let action = match event.kind {
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => settings.press(
                    &layout,
                    point,
                    trial.is_some(),
                    matches!(trial, Some(TrialView::Confirm(_))),
                ),
                PointerEventKind::Press {
                    button: BTN_RIGHT, ..
                } => {
                    if settings.dismiss_menu() {
                        Action::None
                    } else {
                        Action::Close
                    }
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    redraw = settings.release(&layout, point);
                    Action::None
                }
                PointerEventKind::Motion { .. } => {
                    redraw = settings.motion(point);
                    Action::None
                }
                PointerEventKind::Axis { vertical, .. } => {
                    redraw = settings.scroll(&layout, point, axis_scroll_delta(vertical));
                    Action::None
                }
                _ => return,
            };
            let result = match action {
                Action::Close => {
                    self.close_monitor_popup();
                    return;
                }
                Action::Identify => self.identify_displays(qh),
                Action::Apply => self.start_monitor_config(),
                Action::Keep => {
                    self.finish_monitor_config("Display settings kept");
                    return;
                }
                Action::Revert => {
                    self.restore_monitor_config("Previous display settings restored");
                    Ok(())
                }
                Action::None => Ok(()),
            };
            if let Err(error) = result
                && let Some(settings) = self
                    .monitor_popup
                    .as_mut()
                    .and_then(|popup| popup.model.settings.as_mut())
            {
                settings.message = error.to_string();
            }
            if redraw {
                self.draw_monitor_popup();
            }
        } else {
            let inside = point.0 >= 0.0
                && point.0 < f64::from(popup.model.config.width)
                && point.1 >= 0.0
                && point.1 < f64::from(popup.model.panel_height());
            let row = inside.then(|| popup.model.row_at(point.1)).flatten();
            match event.kind {
                PointerEventKind::Motion { .. }
                | PointerEventKind::Enter { .. }
                | PointerEventKind::Leave { .. } => {
                    let hover = if matches!(event.kind, PointerEventKind::Leave { .. }) {
                        None
                    } else {
                        row
                    };
                    if popup.model.hovered_row != hover {
                        popup.model.hovered_row = hover;
                        self.draw_monitor_popup();
                    }
                }
                PointerEventKind::Press {
                    button: BTN_RIGHT, ..
                } => self.close_monitor_popup(),
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => {
                    if !inside {
                        self.close_monitor_popup();
                    } else if let Some(row) = row {
                        match popup.model.recall_window(row) {
                            Ok(Some(address)) => {
                                self.monitor_hotplug.acknowledge_window(&address);
                                self.close_monitor_popup();
                                self.draw_all();
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("mhyprbar: monitor window recall failed: {error:#}")
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn identify_displays(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        let settings = self
            .monitor_popup
            .as_ref()
            .and_then(|popup| popup.model.settings.as_ref())
            .context("Display settings are closed")?;
        let empty_region = Region::new(&self.compositor)?;
        self.monitor_identify.clear();
        for output in self.output_state.outputs() {
            let Some(name) = self.output_state.info(&output).and_then(|info| info.name) else {
                continue;
            };
            let Some(display) = settings
                .displays
                .iter()
                .find(|display| display.info.name == name)
            else {
                continue;
            };
            let surface = self.compositor.create_surface(qh);
            surface.set_input_region(Some(empty_region.wl_region()));
            let layer = self.layer_shell.create_layer_surface(
                qh,
                surface,
                Layer::Overlay,
                Some("mhyprbar-display-identify"),
                Some(&output),
            );
            layer.set_size(BADGE_SIZE.0, BADGE_SIZE.1);
            layer.set_anchor(Anchor::empty());
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            self.monitor_identify.push(IdentifyBadge {
                layer,
                output,
                number: display.number,
            });
        }
        if self.monitor_identify.is_empty() {
            anyhow::bail!("No active displays to identify");
        }
        self.monitor_identify_until = Some(Instant::now() + IDENTIFY_TIME);
        Ok(())
    }

    pub(super) fn draw_identify_badge(&mut self, index: usize, size: (u32, u32)) -> Result<()> {
        let badge = &self.monitor_identify[index];
        let (width, height) = (
            if size.0 == 0 { BADGE_SIZE.0 } else { size.0 },
            if size.1 == 0 { BADGE_SIZE.1 } else { size.1 },
        );
        let surface = badge.layer.wl_surface();
        let (buffer, canvas) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        self.renderer
            .draw_identify_badge(canvas, width, height, badge.number)?;
        buffer.attach_to(surface)?;
        surface.damage_buffer(0, 0, width as i32, height as i32);
        badge.layer.commit();
        Ok(())
    }
}
