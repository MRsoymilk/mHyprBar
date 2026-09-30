use std::time::{Duration, Instant};

use anyhow::Result;

use crate::config::ModuleStyle;

#[cfg(mhypr_module = "active_window")]
pub mod active_window;
#[cfg(mhypr_module = "audio")]
pub mod audio;
#[cfg(mhypr_module = "battery")]
pub mod battery;
#[cfg(mhypr_module = "brightness")]
pub mod brightness;
#[cfg(mhypr_module = "clock")]
pub mod clock;
#[cfg(mhypr_module = "cpu")]
pub mod cpu;
#[cfg(mhypr_module = "disk")]
pub mod disk;
#[cfg(mhypr_module = "gpu")]
pub mod gpu;
#[cfg(mhypr_module = "layout")]
pub mod layout;
#[cfg(mhypr_module = "memory")]
pub mod memory;
#[cfg(mhypr_module = "menu")]
pub mod menu;
#[cfg(mhypr_module = "monitor")]
pub mod monitor;
#[cfg(mhypr_module = "mpris")]
pub mod mpris;
#[cfg(mhypr_module = "network")]
pub mod network;
pub mod tray;

#[derive(Clone, Copy, Debug)]
pub struct CompiledModule {
    pub name: &'static str,
    pub config_file: &'static str,
}

pub enum ModuleVisual {
    Text,
    #[cfg(mhypr_module = "active_window")]
    ActiveWindow(active_window::ActiveWindowVisual),
    #[cfg(mhypr_module = "audio")]
    Audio(audio::AudioVisual),
    #[cfg(mhypr_module = "battery")]
    Battery(battery::BatteryVisual),
    #[cfg(mhypr_module = "brightness")]
    Brightness(brightness::BrightnessVisual),
    #[cfg(mhypr_module = "clock")]
    Clock(clock::ClockVisual),
    #[cfg(mhypr_module = "layout")]
    Layout(layout::LayoutVisual),
    #[cfg(mhypr_module = "cpu")]
    Cpu(cpu::CpuVisual),
    #[cfg(mhypr_module = "disk")]
    Disk(disk::DiskVisual),
    #[cfg(mhypr_module = "gpu")]
    Gpu(gpu::GpuVisual),
    #[cfg(mhypr_module = "monitor")]
    Monitor(monitor::MonitorVisual),
    #[cfg(mhypr_module = "memory")]
    Memory(memory::MemoryVisual),
    #[cfg(mhypr_module = "menu")]
    Menu(menu::MenuVisual),
    #[cfg(mhypr_module = "network")]
    Network(network::NetworkVisual),
}

pub(super) trait StatusModule {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Option<Duration>;
    fn style(&self) -> &ModuleStyle;
    fn sample(&mut self) -> Result<String>;
    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Text
    }
    fn visual_revision(&self) -> u64 {
        0
    }
    fn activate(&mut self) -> Result<bool> {
        Ok(false)
    }
    fn activate_at(&mut self, _x: f64, _y: f64) -> Result<bool> {
        self.activate()
    }
    fn scroll(&mut self, _direction: i32) -> Result<bool> {
        Ok(false)
    }
    #[cfg(mhypr_module = "gpu")]
    fn gpu_processes(&mut self, _limit: usize) -> Result<Vec<crate::modules::gpu::backend::GpuProcess>> {
        Ok(Vec::new())
    }
}

struct RuntimeModule {
    module: Box<dyn StatusModule>,
    text: String,
    next_update: Option<Instant>,
    last_error: Option<String>,
    width_override: Option<i32>,
    visual_revision: u64,
}

impl RuntimeModule {
    fn new(module: Box<dyn StatusModule>) -> Self {
        let text = module.name().to_uppercase();
        let visual_revision = module.visual_revision();
        Self {
            module,
            text,
            next_update: Some(Instant::now()),
            last_error: None,
            width_override: None,
            visual_revision,
        }
    }

    fn refresh_if_due(&mut self, now: Instant) -> bool {
        let Some(next_update) = self.next_update else {
            return false;
        };
        if now < next_update {
            return false;
        }
        self.refresh_now(now)
    }

    fn refresh_now(&mut self, now: Instant) -> bool {
        let name = self.module.name();
        let changed = match self.module.sample() {
            Ok(text) => {
                self.last_error = None;
                let visual_revision = self.module.visual_revision();
                let visual_changed = visual_revision != self.visual_revision;
                self.visual_revision = visual_revision;
                if text != self.text {
                    self.text = text;
                    true
                } else {
                    visual_changed
                }
            }
            Err(error) => {
                let error_text = format!("{error:#}");
                if self.last_error.as_deref() != Some(error_text.as_str()) {
                    eprintln!("mhyprbar: {name} module update failed: {error_text}");
                    self.last_error = Some(error_text);
                }

                let text = format!("{} ?", name.to_uppercase());
                if text != self.text {
                    self.text = text;
                    true
                } else {
                    false
                }
            }
        };
        self.next_update = self.module.interval().map(|interval| now + interval);
        changed
    }
}

pub struct ModuleView<'a> {
    pub text: &'a str,
    pub style: &'a ModuleStyle,
    pub width_override: Option<i32>,
    pub visual: ModuleVisual,
}

pub struct ModuleManager {
    modules: Vec<RuntimeModule>,
}

impl ModuleManager {
    pub fn load() -> Result<Self> {
        let modules = vec![
            #[cfg(mhypr_module = "menu")]
            RuntimeModule::new(Box::new(menu::MenuModule::load()?)),
            #[cfg(mhypr_module = "active_window")]
            RuntimeModule::new(Box::new(active_window::ActiveWindowModule::load()?)),
            #[cfg(mhypr_module = "clock")]
            RuntimeModule::new(Box::new(clock::ClockModule::load()?)),
            #[cfg(mhypr_module = "layout")]
            RuntimeModule::new(Box::new(layout::LayoutModule::load()?)),
            #[cfg(mhypr_module = "cpu")]
            RuntimeModule::new(Box::new(cpu::CpuModule::load()?)),
            #[cfg(mhypr_module = "gpu")]
            RuntimeModule::new(Box::new(gpu::GpuModule::load()?)),
            #[cfg(mhypr_module = "monitor")]
            RuntimeModule::new(Box::new(monitor::MonitorModule::load()?)),
            #[cfg(mhypr_module = "memory")]
            RuntimeModule::new(Box::new(memory::MemoryModule::load()?)),
            #[cfg(mhypr_module = "network")]
            RuntimeModule::new(Box::new(network::NetworkModule::load()?)),
            #[cfg(mhypr_module = "audio")]
            RuntimeModule::new(Box::new(audio::AudioModule::load()?)),
            #[cfg(mhypr_module = "brightness")]
            RuntimeModule::new(Box::new(brightness::BrightnessModule::load()?)),
            #[cfg(mhypr_module = "mpris")]
            RuntimeModule::new(Box::new(mpris::MprisModule::load()?)),
            #[cfg(mhypr_module = "disk")]
            RuntimeModule::new(Box::new(disk::DiskModule::load()?)),
            #[cfg(mhypr_module = "battery")]
            RuntimeModule::new(Box::new(battery::BatteryModule::load()?)),
            #[cfg(mhypr_module = "tray")]
            RuntimeModule::new(Box::new(tray::TrayModule::load()?)),
        ];

        Ok(Self { modules })
    }

    pub fn refresh_due(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        for module in &mut self.modules {
            changed |= module.refresh_if_due(now);
        }
        changed
    }

    pub fn next_timeout(&self) -> Duration {
        let now = Instant::now();
        self.modules
            .iter()
            .filter_map(|module| module.next_update)
            .map(|next_update| next_update.saturating_duration_since(now))
            .min()
            .unwrap_or(Duration::from_secs(60))
    }

    pub fn force_refresh(&mut self, name: &str) -> bool {
        let now = Instant::now();
        self.modules
            .iter_mut()
            .find(|module| module.module.name() == name)
            .is_some_and(|module| module.refresh_now(now))
    }

    pub fn set_width_override(&mut self, name: &str, width: Option<i32>) -> bool {
        let Some(module) = self
            .modules
            .iter_mut()
            .find(|module| module.module.name() == name)
        else {
            return false;
        };
        if module.width_override == width {
            return false;
        }
        module.width_override = width;
        true
    }

    pub fn activate(&mut self, name: &str) -> Result<bool> {
        let Some(module) = self
            .modules
            .iter_mut()
            .find(|module| module.module.name() == name)
        else {
            return Ok(false);
        };
        let handled = module.module.activate()?;
        if handled {
            module.refresh_now(Instant::now());
        }
        Ok(handled)
    }

    pub fn activate_at(&mut self, name: &str, x: f64, y: f64) -> Result<bool> {
        let Some(module) = self
            .modules
            .iter_mut()
            .find(|module| module.module.name() == name)
        else {
            return Ok(false);
        };
        let handled = module.module.activate_at(x, y)?;
        if handled {
            module.refresh_now(Instant::now());
        }
        Ok(handled)
    }

    pub fn scroll(&mut self, name: &str, direction: i32) -> Result<bool> {
        let Some(module) = self
            .modules
            .iter_mut()
            .find(|module| module.module.name() == name)
        else {
            return Ok(false);
        };
        let handled = module.module.scroll(direction)?;
        if handled {
            module.refresh_now(Instant::now());
        }
        Ok(handled)
    }

    #[cfg(mhypr_module = "gpu")]
    pub fn gpu_processes(&mut self, limit: usize) -> Result<Vec<crate::modules::gpu::backend::GpuProcess>> {
        let Some(module) = self
            .modules
            .iter_mut()
            .find(|module| module.module.name() == "gpu")
        else {
            return Ok(Vec::new());
        };
        module.module.gpu_processes(limit)
    }

    pub fn view(&self, name: &str) -> Option<ModuleView<'_>> {
        self.modules
            .iter()
            .find(|module| module.module.name() == name)
            .map(|module| ModuleView {
                text: &module.text,
                style: module.module.style(),
                width_override: module.width_override,
                visual: module.module.visual(),
            })
    }
}

pub fn compiled() -> Vec<CompiledModule> {
    vec![
        #[cfg(mhypr_module = "menu")]
        CompiledModule {
            name: menu::NAME,
            config_file: menu::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "active_window")]
        CompiledModule {
            name: active_window::NAME,
            config_file: active_window::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "clock")]
        CompiledModule {
            name: clock::NAME,
            config_file: clock::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "layout")]
        CompiledModule {
            name: layout::NAME,
            config_file: layout::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "cpu")]
        CompiledModule {
            name: cpu::NAME,
            config_file: cpu::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "gpu")]
        CompiledModule {
            name: gpu::NAME,
            config_file: gpu::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "monitor")]
        CompiledModule {
            name: monitor::NAME,
            config_file: monitor::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "memory")]
        CompiledModule {
            name: memory::NAME,
            config_file: memory::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "network")]
        CompiledModule {
            name: network::NAME,
            config_file: network::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "audio")]
        CompiledModule {
            name: audio::NAME,
            config_file: audio::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "brightness")]
        CompiledModule {
            name: brightness::NAME,
            config_file: brightness::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "mpris")]
        CompiledModule {
            name: mpris::NAME,
            config_file: mpris::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "disk")]
        CompiledModule {
            name: disk::NAME,
            config_file: disk::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "battery")]
        CompiledModule {
            name: battery::NAME,
            config_file: battery::CONFIG_FILE,
        },
        #[cfg(mhypr_module = "tray")]
        CompiledModule {
            name: tray::NAME,
            config_file: tray::CONFIG_FILE,
        },
    ]
}

pub fn validate_configs() -> Result<()> {
    let _ = ModuleManager::load()?;
    Ok(())
}
