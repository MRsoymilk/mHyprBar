use std::cell::Cell;

use anyhow::{Context, ensure};
use smithay_client_toolkit::reexports::protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1 as config_head, zwlr_output_configuration_v1 as config,
    zwlr_output_head_v1 as head, zwlr_output_manager_v1 as manager, zwlr_output_mode_v1 as mode,
};
use wayland_client::{Dispatch, Proxy, delegate_noop, event_created_child, globals::GlobalList};

use super::*;
use crate::modules::monitor::settings::{Change, Mode as DisplayMode};

struct ModeInfo {
    proxy: mode::ZwlrOutputModeV1,
    value: DisplayMode,
}

struct Head {
    proxy: head::ZwlrOutputHeadV1,
    name: String,
    enabled: bool,
    modes: Vec<ModeInfo>,
}

#[derive(Clone, Copy)]
pub(super) enum RequestKind {
    Apply,
    Restore,
}

#[derive(Clone, Copy)]
struct Ticket {
    id: u64,
    kind: RequestKind,
}

pub(super) struct OutputManagement {
    proxy: manager::ZwlrOutputManagerV1,
    qh: QueueHandle<App>,
    serial: Option<u32>,
    heads: Vec<Head>,
    last_request: Cell<u64>,
}

impl OutputManagement {
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Option<Self> {
        Some(Self {
            proxy: globals.bind(qh, 1..=4, ()).ok()?,
            qh: qh.clone(),
            serial: None,
            heads: Vec::new(),
            last_request: Cell::new(0),
        })
    }

    pub(super) fn submit(&self, changes: &[Change], kind: RequestKind) -> Result<Option<u64>> {
        let restoring = matches!(kind, RequestKind::Restore);
        let serial = self
            .serial
            .context("Display management has not received output information")?;
        let mut edits = Vec::new();
        for change in changes {
            let Some(head) = self
                .heads
                .iter()
                .find(|head| head.enabled && head.name == change.name)
            else {
                if restoring {
                    continue;
                }
                anyhow::bail!("{} is no longer available", change.name);
            };
            let target = if restoring {
                change.before
            } else {
                change.after
            };
            let transform = wl_output::Transform::try_from(u32::from(target.transform))
                .ok()
                .with_context(|| format!("Invalid orientation on {}", head.name))?;
            let mode_changed = change.before.mode != change.after.mode;
            let selected_mode = if mode_changed {
                head.modes
                    .iter()
                    .filter(|mode| mode.value.matches(target.mode))
                    .min_by_key(|mode| mode.value.mhz.abs_diff(target.mode.mhz))
                    .map(|mode| &mode.proxy)
            } else {
                None
            };
            ensure!(
                !mode_changed || selected_mode.is_some() || restoring,
                "The selected mode is unavailable on {}",
                head.name
            );
            edits.push((head, change, target, selected_mode, transform));
        }
        if edits.is_empty() {
            return Ok(None);
        }

        let id = self.last_request.get().wrapping_add(1);
        self.last_request.set(id);
        let request = self
            .proxy
            .create_configuration(serial, &self.qh, Ticket { id, kind });
        for head in &self.heads {
            if !head.enabled {
                request.disable_head(&head.proxy);
                continue;
            }
            let output = request.enable_head(&head.proxy, &self.qh, ());
            if let Some((_, change, target, selected_mode, transform)) =
                edits.iter().find(|(edited, ..)| edited.proxy == head.proxy)
            {
                if let Some(mode) = selected_mode {
                    output.set_mode(mode);
                } else if change.before.mode != change.after.mode {
                    output.set_custom_mode(target.mode.width, target.mode.height, target.mode.mhz);
                }
                if change.set_position {
                    output.set_position(target.x, target.y);
                }
                if change.before.transform != change.after.transform {
                    output.set_transform(*transform);
                }
                if change.before.scale != change.after.scale {
                    output.set_scale(target.scale);
                }
            }
        }
        request.apply();
        Ok(Some(id))
    }
}

impl Dispatch<manager::ZwlrOutputManagerV1, ()> for App {
    event_created_child!(App, manager::ZwlrOutputManagerV1, [
        manager::EVT_HEAD_OPCODE => (head::ZwlrOutputHeadV1, ()),
    ]);

    fn event(
        state: &mut Self,
        _: &manager::ZwlrOutputManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(outputs) = &mut state.output_management else {
            return;
        };
        match event {
            manager::Event::Head { head } => outputs.heads.push(Head {
                proxy: head,
                name: String::new(),
                enabled: false,
                modes: Vec::new(),
            }),
            manager::Event::Done { serial } => outputs.serial = Some(serial),
            manager::Event::Finished => state.output_management = None,
            _ => {}
        }
    }
}

impl Dispatch<head::ZwlrOutputHeadV1, ()> for App {
    event_created_child!(App, head::ZwlrOutputHeadV1, [
        head::EVT_MODE_OPCODE => (mode::ZwlrOutputModeV1, ()),
    ]);

    fn event(
        state: &mut Self,
        proxy: &head::ZwlrOutputHeadV1,
        event: head::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(outputs) = &mut state.output_management else {
            return;
        };
        if matches!(event, head::Event::Finished) {
            outputs.heads.retain(|head| head.proxy != *proxy);
            if proxy.version() >= 3 {
                proxy.release();
            }
            return;
        }
        let Some(head) = outputs.heads.iter_mut().find(|head| head.proxy == *proxy) else {
            return;
        };
        match event {
            head::Event::Name { name } => head.name = name,
            head::Event::Enabled { enabled } => head.enabled = enabled != 0,
            head::Event::Mode { mode } => head.modes.push(ModeInfo {
                proxy: mode,
                value: DisplayMode::default(),
            }),
            _ => {}
        }
    }
}

impl Dispatch<mode::ZwlrOutputModeV1, ()> for App {
    fn event(
        state: &mut Self,
        proxy: &mode::ZwlrOutputModeV1,
        event: mode::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(outputs) = &mut state.output_management else {
            return;
        };
        if matches!(event, mode::Event::Finished) {
            for head in &mut outputs.heads {
                head.modes.retain(|mode| mode.proxy != *proxy);
            }
            if proxy.version() >= 3 {
                proxy.release();
            }
            return;
        }
        let Some(mode) = outputs
            .heads
            .iter_mut()
            .flat_map(|head| &mut head.modes)
            .find(|mode| mode.proxy == *proxy)
        else {
            return;
        };
        match event {
            mode::Event::Size { width, height } => {
                mode.value.width = width;
                mode.value.height = height;
            }
            mode::Event::Refresh { refresh } => mode.value.mhz = refresh,
            _ => {}
        }
    }
}

impl Dispatch<config::ZwlrOutputConfigurationV1, Ticket> for App {
    fn event(
        state: &mut Self,
        request: &config::ZwlrOutputConfigurationV1,
        event: config::Event,
        ticket: &Ticket,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let result = match event {
            config::Event::Succeeded => Ok(()),
            config::Event::Failed => Err("Hyprland rejected the display settings"),
            config::Event::Cancelled => Err("Display topology changed; try again"),
            _ => return,
        };
        request.destroy();
        state.monitor_config_result(ticket.id, ticket.kind, result);
    }
}

delegate_noop!(App: ignore config_head::ZwlrOutputConfigurationHeadV1);
