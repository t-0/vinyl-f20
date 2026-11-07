//! [`Platform`] for the real player: buttons through evdev, the mapper for
//! long presses, and the hardware module for volume, battery and power.
use super::hw::Hw;
use super::input::Input;
use super::keys::{Action, Ctx, Mapper, button_for};
use super::run::Platform;
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

pub struct DevicePlatform {
    input: Input,
    mapper: Mapper,
    pub hw: Hw,
    swap_prev_next: bool,
    playing: bool,
    log: std::path::PathBuf,
    stream_logged: bool,
}

impl DevicePlatform {
    pub fn new(home: &Path, settings: &super::settings::Settings) -> DevicePlatform {
        super::trace::step(4);
        let input = Input::open();
        DevicePlatform {
            input,
            mapper: Mapper::default(),
            hw: Hw::new(home, settings.volume, settings.screen_timeout_secs, settings.mixer, settings.port),
            log: home.join("boot.log"),
            stream_logged: false,
            swap_prev_next: settings.swap_prev_next,
            playing: false,
        }
    }

    fn note(&self, line: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&self.log) {
            let _ = writeln!(f, "[run] {line}");
        }
    }

    pub fn describe(&self) -> String {
        format!("{} input devices; {}", self.input.device_count(), self.hw.describe())
    }
}

impl Platform for DevicePlatform {
    fn wait(&mut self, timeout: Duration, ctx: &Ctx) -> Result<Vec<Action>> {
        let deadline = Instant::now() + timeout;
        let mut acts = Vec::new();
        loop {
            let now = Instant::now();
            let remaining = deadline.saturating_duration_since(now);
            let step = self.mapper.next_wake(now).map_or(remaining, |w| w.min(remaining));
            let raws = self.input.wait(step);
            let now = Instant::now();
            let mut pressed_any = false;
            for raw in raws {
                let Some(b) = button_for(raw.code, self.swap_prev_next) else {
                    continue;
                };
                match raw.value {
                    1 => {
                        pressed_any = true;
                        acts.extend(self.mapper.event(b, true, now, ctx));
                    }
                    0 => acts.extend(self.mapper.event(b, false, now, ctx)),
                    _ => {}
                }
            }
            acts.extend(self.mapper.tick(now, ctx));
            if pressed_any {
                self.hw.note_activity(now);
                if !self.hw.screen_on() {
                    // The press that wakes the screen does nothing else.
                    self.hw.screen(true);
                    self.mapper.clear();
                    acts.clear();
                }
            }
            if !acts.is_empty() || Instant::now() >= deadline {
                return Ok(acts);
            }
        }
    }

    fn volume_step(&mut self, delta: i32) -> Option<String> {
        let v = self.hw.volume_step(delta);
        self.hw.save();
        Some(format!("Volume {v}%"))
    }

    fn screen_toggle(&mut self) {
        let on = !self.hw.screen_on();
        self.hw.screen(on);
        self.hw.note_activity(Instant::now());
    }

    fn power_off(&mut self) -> Result<()> {
        self.hw.power_off()
    }

    fn status(&mut self) -> Option<String> {
        self.hw.battery().map(|(pct, charging)| {
            if charging {
                format!("{pct}%+")
            } else {
                format!("{pct}%")
            }
        })
    }

    fn tick(&mut self) {
        self.hw.update_outputs();
        self.hw.tick(false);
        if !self.stream_logged && let Some(p) = Hw::stream_params() {
            self.stream_logged = true;
            self.note(&format!("alsa stream: {p}"));
        }
    }

    fn port_step(&mut self, delta: i32) -> Option<String> {
        let p = self.hw.port_step(delta);
        self.note(&format!("output port switch set to {p} (Menu + wheel)"));
        Some(format!("Output port {p}"))
    }
}
