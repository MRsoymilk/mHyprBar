use std::time::{Duration, Instant};

use anyhow::Result;

use crate::{
    hyprland::{self, EventBatch, MonitorState, Snapshot},
    modules::monitor::{MissingMonitorWindow, MonitorHotplugConfig},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct MonitorIdentity {
    connector: String,
    description: String,
    make: String,
    model: String,
    serial: String,
}

impl MonitorIdentity {
    fn from_monitor(monitor: &MonitorState) -> Self {
        Self {
            connector: normalize(&monitor.name),
            description: normalize_description(&monitor.description, &monitor.name),
            make: normalize(&monitor.make),
            model: normalize(&monitor.model),
            serial: normalize(&monitor.serial),
        }
    }

    fn match_score(&self, other: &Self) -> u8 {
        if !self.serial.is_empty() && !other.serial.is_empty() {
            if self.serial != other.serial {
                return 0;
            }
            if fields_conflict(&self.make, &other.make) || fields_conflict(&self.model, &other.model)
            {
                return 0;
            }
            return 100;
        }

        if !self.make.is_empty()
            && !self.model.is_empty()
            && !other.make.is_empty()
            && !other.model.is_empty()
        {
            if self.make != other.make || self.model != other.model {
                return 0;
            }
            return if !self.description.is_empty()
                && self.description == other.description
            {
                85
            } else {
                75
            };
        }

        if !self.description.is_empty()
            && !other.description.is_empty()
            && self.description == other.description
        {
            return 65;
        }

        if !self.connector.is_empty() && self.connector == other.connector {
            return 45;
        }

        0
    }
}

#[derive(Clone, Debug)]
struct MonitorProfile {
    identity: MonitorIdentity,
    workspaces: Vec<i32>,
    active_workspace: i32,
}

#[derive(Clone, Debug)]
struct MissingWindowRecord {
    origin: MonitorIdentity,
    window: MissingMonitorWindow,
}

pub(super) struct MonitorHotplugState {
    config: MonitorHotplugConfig,
    profiles: Vec<MonitorProfile>,
    connected: Vec<MonitorIdentity>,
    added_connectors: Vec<String>,
    removed_connectors: Vec<String>,
    missing_windows: Vec<MissingWindowRecord>,
    deadline: Option<Instant>,
}

impl MonitorHotplugState {
    pub(super) fn new(snapshot: &Snapshot) -> Result<Self> {
        let config = MonitorHotplugConfig::load()?;
        let mut state = Self {
            config,
            profiles: Vec::new(),
            connected: Vec::new(),
            added_connectors: Vec::new(),
            removed_connectors: Vec::new(),
            missing_windows: Vec::new(),
            deadline: None,
        };
        state.sync_snapshot(snapshot);
        Ok(state)
    }

    pub(super) fn reload_config(&mut self) -> Result<()> {
        self.config = MonitorHotplugConfig::load()?;
        if !self.config.enabled {
            self.deadline = None;
            self.added_connectors.clear();
            self.removed_connectors.clear();
            self.missing_windows.clear();
        }
        Ok(())
    }

    pub(super) fn schedule(&mut self, snapshot: &Snapshot, batch: &EventBatch) -> bool {
        if !self.config.enabled || !batch.monitor_topology_changed() {
            return false;
        }

        // Capture the last stable ownership before Hyprland migrates workspaces
        // away from a disappearing output. Window clients may already have been
        // reassigned by Hyprland, so match them by the previous workspace IDs.
        self.sync_profiles(snapshot);
        if !batch.monitor_removed.is_empty()
            && let Err(error) = self.capture_missing_windows(snapshot, &batch.monitor_removed)
        {
            eprintln!("mhyprbar: failed to capture windows from removed monitor: {error:#}");
        }
        extend_unique(&mut self.added_connectors, &batch.monitor_added);
        extend_unique(&mut self.removed_connectors, &batch.monitor_removed);
        self.deadline = Some(Instant::now() + self.config.debounce_interval());
        true
    }

    pub(super) fn pending(&self) -> bool {
        self.deadline.is_some()
    }

    pub(super) fn timeout(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub(super) fn due(&self) -> bool {
        self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub(super) fn defer(&mut self) {
        if self.config.enabled {
            self.deadline = Some(Instant::now() + self.config.debounce_interval());
        }
    }

    pub(super) fn restore_workspaces(&self, snapshot: &Snapshot) -> Result<usize> {
        if !self.config.enabled || !self.config.restore_workspaces {
            return Ok(0);
        }

        let plan = self.restore_plan(snapshot);
        let mut restored = 0;
        for (workspace, monitor) in plan {
            match hyprland::move_workspace_to_monitor(workspace, &monitor) {
                Ok(()) => restored += 1,
                Err(error) => eprintln!(
                    "mhyprbar: failed to restore workspace {workspace} to {monitor}: {error:#}"
                ),
            }
        }
        Ok(restored)
    }

    pub(super) fn complete(&mut self, snapshot: &Snapshot) {
        self.sync_snapshot(snapshot);
        self.prune_reconnected_windows(snapshot);
        self.added_connectors.clear();
        self.removed_connectors.clear();
        self.deadline = None;
    }

    pub(super) fn missing_windows(&mut self) -> Result<Vec<MissingMonitorWindow>> {
        let live = hyprland::window_clients()?;
        self.missing_windows.retain(|record| {
            live.iter().any(|client| {
                client.address.eq_ignore_ascii_case(&record.window.address)
            })
        });
        Ok(self
            .missing_windows
            .iter()
            .map(|record| record.window.clone())
            .collect())
    }

    pub(super) fn acknowledge_window(&mut self, address: &str) {
        self.missing_windows
            .retain(|record| !record.window.address.eq_ignore_ascii_case(address));
    }

    pub(super) fn sync_if_idle(&mut self, snapshot: &Snapshot) {
        if !self.pending() {
            self.sync_snapshot(snapshot);
        }
    }

    fn capture_missing_windows(
        &mut self,
        snapshot: &Snapshot,
        removed_connectors: &[String],
    ) -> Result<()> {
        let clients = hyprland::window_clients()?;
        for connector in removed_connectors {
            let Some(monitor) = snapshot.monitor_by_name(connector) else {
                continue;
            };
            let origin = MonitorIdentity::from_monitor(monitor);
            let workspaces = snapshot.workspaces_for_monitor(connector);
            self.missing_windows
                .retain(|record| record.origin.match_score(&origin) == 0);

            for client in clients
                .iter()
                .filter(|client| workspaces.contains(&client.workspace_id))
            {
                let class = if client.class.trim().is_empty() {
                    client.initial_class.clone()
                } else {
                    client.class.clone()
                };
                self.missing_windows.push(MissingWindowRecord {
                    origin: origin.clone(),
                    window: MissingMonitorWindow {
                        origin_monitor: monitor.name.clone(),
                        address: client.address.clone(),
                        class,
                        title: client.title.clone(),
                        workspace_id: client.workspace_id,
                    },
                });
            }
        }
        self.missing_windows.sort_by(|left, right| {
            left.window
                .origin_monitor
                .cmp(&right.window.origin_monitor)
                .then_with(|| left.window.workspace_id.cmp(&right.window.workspace_id))
                .then_with(|| left.window.class.cmp(&right.window.class))
                .then_with(|| left.window.title.cmp(&right.window.title))
        });
        Ok(())
    }

    fn prune_reconnected_windows(&mut self, snapshot: &Snapshot) {
        self.missing_windows.retain(|record| {
            !snapshot.monitors().iter().any(|monitor| {
                record
                    .origin
                    .match_score(&MonitorIdentity::from_monitor(monitor))
                    > 0
            })
        });
    }

    fn restore_plan(&self, snapshot: &Snapshot) -> Vec<(i32, String)> {
        let mut plan = Vec::new();

        for monitor in snapshot.monitors() {
            let identity = MonitorIdentity::from_monitor(monitor);
            let was_connected = self
                .connected
                .iter()
                .any(|previous| previous.match_score(&identity) > 0);
            let explicitly_added = self
                .added_connectors
                .iter()
                .any(|name| name == &monitor.name);

            if was_connected && !explicitly_added {
                continue;
            }

            let Some(profile) = self.best_profile(&identity) else {
                continue;
            };

            let mut workspaces = profile.workspaces.clone();
            workspaces.sort_unstable();
            if let Some(position) = workspaces
                .iter()
                .position(|workspace| *workspace == profile.active_workspace)
            {
                let active = workspaces.remove(position);
                workspaces.push(active);
            }

            for workspace in workspaces {
                if !snapshot.has_workspace(workspace)
                    || snapshot.monitor_for_workspace(workspace) == Some(monitor.name.as_str())
                {
                    continue;
                }
                plan.push((workspace, monitor.name.clone()));
            }
        }

        plan
    }

    fn best_profile(&self, identity: &MonitorIdentity) -> Option<&MonitorProfile> {
        self.profiles
            .iter()
            .filter_map(|profile| {
                let score = profile.identity.match_score(identity);
                (score > 0).then_some((score, profile))
            })
            .max_by_key(|(score, _)| *score)
            .map(|(_, profile)| profile)
    }

    fn sync_snapshot(&mut self, snapshot: &Snapshot) {
        self.sync_profiles(snapshot);
        self.connected = snapshot
            .monitors()
            .iter()
            .map(MonitorIdentity::from_monitor)
            .collect();
    }

    fn sync_profiles(&mut self, snapshot: &Snapshot) {
        for monitor in snapshot.monitors() {
            let identity = MonitorIdentity::from_monitor(monitor);
            let workspaces = snapshot.workspaces_for_monitor(&monitor.name).to_vec();
            let active_workspace = monitor.active_workspace;
            let matching = self
                .profiles
                .iter()
                .enumerate()
                .filter_map(|(index, profile)| {
                    let score = profile.identity.match_score(&identity);
                    (score > 0).then_some((score, index))
                })
                .max_by_key(|(score, _)| *score)
                .map(|(_, index)| index);

            if let Some(index) = matching {
                let profile = &mut self.profiles[index];
                profile.identity = identity;
                if !workspaces.is_empty() {
                    profile.workspaces = workspaces;
                }
                if active_workspace > 0 {
                    profile.active_workspace = active_workspace;
                }
            } else {
                self.profiles.push(MonitorProfile {
                    identity,
                    workspaces,
                    active_workspace,
                });
            }
        }
    }
}

fn fields_conflict(left: &str, right: &str) -> bool {
    !left.is_empty() && !right.is_empty() && left != right
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_description(description: &str, connector: &str) -> String {
    let description = description.trim();
    let connector = connector.trim();
    let suffix = format!(" ({connector})");
    normalize(description.strip_suffix(&suffix).unwrap_or(description))
}

fn extend_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, make: &str, model: &str, serial: &str, active: i32) -> MonitorState {
        MonitorState {
            id: 0,
            name: name.into(),
            description: format!("{make} {model} ({name})"),
            make: make.into(),
            model: model.into(),
            serial: serial.into(),
            x: 0,
            y: 0,
            height: 1080,
            active_workspace: active,
        }
    }

    fn config() -> MonitorHotplugConfig {
        MonitorHotplugConfig {
            enabled: true,
            restore_workspaces: true,
            debounce_ms: 250,
        }
    }

    #[test]
    fn serial_identity_survives_connector_changes() {
        let a = MonitorIdentity::from_monitor(&monitor("HDMI-A-1", "Dell", "U2720Q", "ABC", 7));
        let b = MonitorIdentity::from_monitor(&monitor("DP-3", "Dell", "U2720Q", "ABC", 7));
        assert_eq!(a.match_score(&b), 100);
    }

    #[test]
    fn different_serials_do_not_restore_to_replacement_display() {
        let a = MonitorIdentity::from_monitor(&monitor("HDMI-A-1", "Dell", "U2720Q", "ABC", 7));
        let b = MonitorIdentity::from_monitor(&monitor("HDMI-A-1", "Dell", "U2720Q", "XYZ", 7));
        assert_eq!(a.match_score(&b), 0);
    }

    #[test]
    fn reconnect_restores_only_existing_original_workspaces() {
        let hdmi = monitor("HDMI-A-1", "Dell", "U2720Q", "ABC", 8);
        let internal = monitor("eDP-1", "Panel", "Internal", "INT", 1);
        let before = Snapshot::test_snapshot(
            vec![internal.clone(), hdmi.clone()],
            &[(1, "eDP-1"), (7, "HDMI-A-1"), (8, "HDMI-A-1"), (9, "HDMI-A-1")],
        );

        let mut state = MonitorHotplugState {
            config: config(),
            profiles: Vec::new(),
            connected: Vec::new(),
            added_connectors: Vec::new(),
            removed_connectors: Vec::new(),
            missing_windows: Vec::new(),
            deadline: None,
        };
        state.sync_snapshot(&before);

        let disconnected = Snapshot::test_snapshot(
            vec![internal.clone()],
            &[(1, "eDP-1"), (7, "eDP-1"), (8, "eDP-1"), (9, "eDP-1")],
        );
        state.sync_snapshot(&disconnected);
        state.added_connectors.push("HDMI-A-1".into());

        let reconnected = Snapshot::test_snapshot(
            vec![internal, hdmi],
            &[(1, "eDP-1"), (7, "eDP-1"), (8, "eDP-1"), (10, "HDMI-A-1")],
        );
        assert_eq!(
            state.restore_plan(&reconnected),
            vec![(7, "HDMI-A-1".into()), (8, "HDMI-A-1".into())]
        );
    }

    #[test]
    fn removal_only_does_not_move_workspaces_to_remaining_monitor() {
        let hdmi = monitor("HDMI-A-1", "Dell", "U2720Q", "ABC", 7);
        let internal = monitor("eDP-1", "Panel", "Internal", "INT", 1);
        let before = Snapshot::test_snapshot(
            vec![internal.clone(), hdmi],
            &[(1, "eDP-1"), (7, "HDMI-A-1")],
        );

        let mut state = MonitorHotplugState {
            config: config(),
            profiles: Vec::new(),
            connected: Vec::new(),
            added_connectors: Vec::new(),
            removed_connectors: vec!["HDMI-A-1".into()],
            missing_windows: Vec::new(),
            deadline: None,
        };
        state.sync_snapshot(&before);

        let after = Snapshot::test_snapshot(
            vec![internal],
            &[(1, "eDP-1"), (7, "eDP-1")],
        );
        assert!(state.restore_plan(&after).is_empty());
    }
}
