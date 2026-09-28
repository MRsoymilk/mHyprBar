# mHyprBar

A native Wayland status bar for Hyprland, intended to replace wibar.

mHyprBar uses wlr-layer-shell directly through Smithay Client Toolkit. It does not depend on GTK or Qt.

## Current status

The base panel and first status-module renderer are implemented:

- one Layer Shell surface per Wayland output;
- top or bottom placement;
- configurable height and margins;
- configurable exclusive zone;
- wl_shm software buffers;
- no keyboard focus capture;
- hot-plug aware output creation/removal;
- compile-time selectable status modules;
- UTF-8 text rendering with cosmic-text;
- configurable left / center / right module groups;
- per-module font, color, padding and minimum width;
- independent refresh intervals per periodic module;
- event-driven active-window title updates;
- live clock, CPU, memory, disk and battery text sources;
- Hyprland monitor/workspace state over native Unix-socket IPC;
- event-driven workspace/monitor updates from Hyprland socket2;
- local workspace labels 1–9 on every monitor;
- active and occupied workspace highlighting;
- clickable workspace switching per monitor;
- StatusNotifierItem system tray with event-driven add/remove/update, IconPixmap rendering and primary activation.

P4 status modules are complete. The default build keeps external desktop integrations disabled: audio (`wpctl`) and MPRIS (`playerctl`) are opt-in compile-time modules.

## Build

~~~bash
cargo build --release
~~~

### Compile-time modules

Edit `build.modules.toml` before building:

~~~toml
[modules]
menu = true
active_window = true
clock = true
cpu = true
memory = true
network = true
audio = false
mpris = true
disk = true
battery = true
tray = true
~~~

`true` means the module source is compiled into the binary. `false` excludes that module from the binary.

After changing the file, rebuild:

~~~bash
cargo build --release
~~~

Inspect what was compiled:

~~~bash
cargo run -- --list-modules
~~~

## Dependency policy

Core modules read Hyprland IPC, Linux `/proc` and `/sys` directly. Audio and MPRIS remain
compile-time selectable integrations:

- `audio` uses `wpctl` and therefore requires a working PipeWire/WirePlumber session;
- `mpris` uses `playerctl` and an MPRIS-capable media player;
- `tray` adds one direct Rust dependency, `rustsni`, over the session D-Bus. It is vendored at `vendor/rustsni` with bounded synchronous D-Bus waits so a non-responsive tray application cannot freeze the bar; it still uses pure-Rust `rustbus`, with no GTK, Qt, Tokio, libdbus, or image-decoding crate.

Enable or disable these modules in `build.modules.toml` as needed. Battery discovery is
native through `/sys/class/power_supply`; `device = "auto"` hides the module silently when no
battery exists.

Repeated module errors are de-duplicated: an unchanged failure is logged once, and can be logged
again only after the module has recovered successfully.

## Runtime configuration

Global bar geometry is configured by:

~~~text
~/.config/mhyprbar/bar.toml
~~~

Each compiled module has its own configuration file:

~~~text
~/.config/mhyprbar/modules/menu.toml
~/.config/mhyprbar/modules/active_window.toml
~/.config/mhyprbar/modules/clock.toml
~/.config/mhyprbar/modules/cpu.toml
~/.config/mhyprbar/modules/memory.toml
~/.config/mhyprbar/modules/network.toml
~/.config/mhyprbar/modules/audio.toml
~/.config/mhyprbar/modules/mpris.toml
~/.config/mhyprbar/modules/disk.toml
~/.config/mhyprbar/modules/battery.toml
~/.config/mhyprbar/modules/tray.toml
~~~

The per-module file controls module-specific behavior and visual style. Periodic modules also define their own refresh interval; event-driven modules such as `active_window` do not poll. An empty module value is hidden completely and consumes no bar width.

Install the example configuration:

~~~bash
mkdir -p ~/.config/mhyprbar/modules
cp config.example/bar.toml ~/.config/mhyprbar/bar.toml
cp config.example/modules/*.toml ~/.config/mhyprbar/modules/
~~~

Validate the active configuration:

~~~bash
mhyprbar --check-config
~~~

For development/tests, `MHYPRBAR_CONFIG_DIR` can point directly at another config directory:

~~~bash
MHYPRBAR_CONFIG_DIR=config.example cargo run -- --check-config
~~~

## Module placement

`bar.toml` decides where compiled modules appear:

~~~toml
left = ["menu"]
center = ["active_window"]
right = ["network", "cpu", "memory", "disk", "battery", "tray", "clock"]
~~~

A module named in `bar.toml` must also be enabled in `build.modules.toml`.

## System tray

The `tray` module implements the StatusNotifierWatcher + StatusNotifierHost pair and is
event-driven through the same calloop loop as Wayland.

Current tray interaction supports:

- dynamic item registration/removal/update;
- SNI `IconPixmap` (ARGB32) rendered directly into the existing wl_shm buffer;
- `NeedsAttention` pixmaps;
- left-click `Activate` with monitor-relative positions converted to screen coordinates;
- right-click DBusMenu lists rendered directly by mHyprBar as a Layer Shell overlay;
- fallback `ContextMenu(x, y)` for items that do not expose a DBusMenu tree;
- middle-click `SecondaryActivate`;
- horizontal/vertical wheel forwarding through `Scroll`;
- `ItemIsMenu` items prefer their menu;
- delayed hover tooltip using SNI `ToolTip.title/text`, falling back to the item title/id;
- hidden passive items by default.

~~~toml
# modules/tray.toml
icon_size = 18
spacing = 6
show_passive = false

[tooltip]
enabled = true
delay_ms = 350
offset = 6
max_chars = 96

[tooltip.style]
foreground = "#F2F2F2"
background = "#202020EE"
font_family = "sans-serif"
font_size = 12.0
padding_x = 8
padding_y = 5
min_width = 0

[menu]
width = 280
item_height = 30
padding_x = 10
border_width = 1
separator_inset = 8
indicator = "›"
hover_background = "#3A3A3AF0"
border = "#626262"
separator = "#626262"

[menu.style]
foreground = "#F2F2F2"
background = "#202020F2"
font_family = "sans-serif"
font_size = 13.0
padding_x = 0
padding_y = 0
min_width = 0
~~~

Items that expose only `IconName` are resolved from XDG icon directories. SVG theme icons are
rasterized once through the system `rsvg-convert` + `magick` tools and cached in memory; PNG
theme icons use `magick` directly. This keeps GTK/Qt/SVG/image-decoding crates out of mHyprBar's
Rust dependency graph. If those optional tools are unavailable, the item falls back without
crashing the bar.

DBusMenu is handled entirely in-process by mHyprBar. The bar reads the DBusMenu tree through
`rustsni`, renders a transparent full-output `Layer::Overlay` surface, performs pointer hit
testing itself, and sends the selected numeric node id directly back through
`rustsni::menu_click`. No temporary TOML, shell callback, IPC round-trip, or mHyprMenu process is
involved. First-level and second-level menus remain cascading; deeper DBusMenu levels are flattened
into the second level with `›` prefixes. Separators and check/radio state are preserved.

Tooltips are independent `Layer::Overlay` surfaces on the same output as the hovered bar. They do
not reserve screen space and are destroyed when the pointer leaves the tray, changes item, clicks,
or reloads the tray config. `--tray-tooltip N` forces item `N`'s tooltip for debugging.

mHyprBar vendors the same `rustsni 0.2.2` source under `vendor/rustsni` with bounded synchronous
D-Bus waits. A non-responsive StatusNotifierItem therefore times out instead of freezing the bar's
single-threaded calloop event loop.

`mhyprbar --status` reports `tray_items=N`, which is useful for distinguishing an empty tray
from a rendering problem.

## mHyprMenu integration

The optional `menu` module is separate from the tray. The tray does not depend on mHyprMenu.
The normal `menu` module is a clickable bar module whose default configuration launches
`mhyprmenu` directly:

~~~toml
label = "Menu"
command = "mhyprmenu"
args = []
~~~

The command and arguments are configurable, and execution does not go through a shell. mHyprBar
does not require mHyprMenu to be installed in order to start; if the configured command is missing,
the click failure is logged and the bar keeps running.

## Workspaces

Every monitor displays local workspace labels `1..9` by default. Internally, each
monitor gets its own global workspace range:

~~~text
slot 0 -> labels 1..9 -> Hyprland workspace ids 1..9
slot 1 -> labels 1..9 -> Hyprland workspace ids 10..18
slot 2 -> labels 1..9 -> Hyprland workspace ids 19..27
~~~

The default slot is the Hyprland monitor id. For a persistent mapping by connector
name, set explicit overrides:

~~~toml
[workspaces.monitor_slots]
"eDP-1" = 0
"DP-1" = 1
~~~

Workspace state is refreshed from Hyprland's event socket rather than by polling
`hyprctl`. Clicking a workspace focuses that monitor and switches to the mapped
global workspace.

## Command-line control

A running bar listens on `$XDG_RUNTIME_DIR/mhyprbar.sock` with user-only permissions.

~~~bash
mhyprbar --status
mhyprbar --reload
mhyprbar --tray-list
mhyprbar --tray-menu 0
mhyprbar --tray-tooltip 0
mhyprbar --quit
~~~

`--reload` reloads `bar.toml` and every compiled module TOML without restarting the process.
The new configuration is fully loaded and validated first; if validation fails, the command exits
non-zero and the currently running configuration remains active.

Changes to `build.modules.toml` are compile-time changes and still require rebuilding/restarting
mHyprBar.

`--status` prints the running PID, output count, position/height, tray item count, compiled
modules and current left/center/right layout.

`--tray-list` prints the current visual tray order with index, title, menu availability, SNI
status, tooltip text, and per-output tray x ranges. `--tray-menu N` opens item `N`'s
in-process DBusMenu overlay. `--tray-tooltip N` forces item `N`'s tooltip. Both are
debug/control equivalents of the normal pointer interactions.

## Run

~~~bash
mhyprbar
~~~

The runtime creates one bar per output, updates each module on its own interval and redraws all visible bars only when module data changes.
