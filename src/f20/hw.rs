//! The player's hardware beyond screen and buttons: DAC volume and output
//! routing through ALSA mixer controls, battery and jack state from sysfs,
//! backlight, and powering off.
//!
//! Volume: hardware revisions 2+ expose the ES9018K2M attenuator as
//! "Left/Right Playback Volume" (0..255 in 0.5 dB steps, 0 = loudest); on
//! revision 1 there is no such control and the "Software Volume" that our
//! `.asoundrc` softvol stage creates is used instead. "Output Port Switch"
//! routes to the headphone (2) or line (1) jack, 0 is muted; which jack is in
//! use comes from `/sys/class/switch/{headset,lineout}/state`, as in Rockbox.
use alsa::ctl::{ElemId, ElemType, ElemValue};
use alsa::hctl::HCtl;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const DAC_L: &str = "Left Playback Volume";
const DAC_R: &str = "Right Playback Volume";
const SOFT: &str = "Software Volume";
const PORT: &str = "Output Port Switch";
/// Volume 0% on the DAC attenuator: 60 dB down (120 half-dB steps).
const DAC_RANGE_STEPS: i32 = 120;

pub struct Hw {
    hctl: Option<HCtl>,
    volume: i32,
    volume_file: PathBuf,
    last_port: Option<i32>,
    switches: (Option<i64>, Option<i64>),
    pinned_port: Option<i32>,
    backlight: Option<PathBuf>,
    brightness: String,
    screen_on: bool,
    last_activity: Instant,
    screen_timeout: Option<Duration>,
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_int(p: &Path) -> Option<i64> {
    read_trim(p)?.parse().ok()
}

impl Hw {
    pub fn new(home: &Path, start_volume: i32, screen_timeout_secs: u64, use_mixer: bool, pinned_port: Option<i32>) -> Hw {
        use super::trace::step;
        step(5);
        let volume_file = home.join("volume");
        let volume = read_int(&volume_file)
            .map(|v| v as i32)
            .unwrap_or(start_volume)
            .clamp(0, 100);
        let backlight = find_backlight();
        let brightness = backlight
            .as_ref()
            .and_then(|d| read_trim(&d.join("brightness")))
            .filter(|b| b != "0")
            .or_else(|| {
                backlight
                    .as_ref()
                    .and_then(|d| read_int(&d.join("max_brightness")))
                    .map(|m| (m * 7 / 10).max(1).to_string())
            })
            .unwrap_or_else(|| "128".to_string());
        step(6);
        // snd_hctl_load must run exactly once per handle: a second call on a
        // populated list corrupts it (that was a SIGSEGV on the player).
        let hctl = if use_mixer {
            HCtl::new("hw:0", false).ok().filter(|h| h.load().is_ok())
        } else {
            None
        };
        let mut hw = Hw {
            hctl,
            volume,
            volume_file,
            last_port: None,
            switches: (None, None),
            pinned_port,
            backlight,
            brightness,
            screen_on: true,
            last_activity: Instant::now(),
            screen_timeout: (screen_timeout_secs > 0).then(|| Duration::from_secs(screen_timeout_secs)),
        };
        step(7);
        step(8);
        hw.update_outputs();
        step(9);
        hw.apply_volume();
        hw
    }

    pub fn describe(&self) -> String {
        format!(
            "ctl {}; dac volume {}; volume {}%; port {:?} (headset/lineout switches {:?}); backlight {:?} (brightness {})",
            if self.hctl.is_some() { "open" } else { "unavailable" },
            self.has(DAC_L),
            self.volume,
            self.last_port,
            self.switches,
            self.backlight,
            self.brightness
        )
    }

    fn has(&self, name: &str) -> bool {
        self.hctl.as_ref().is_some_and(|h| {
            h.elem_iter()
                .any(|e| e.get_id().ok().and_then(|id| id.get_name().ok().map(|n| n == name)).unwrap_or(false))
        })
    }

    fn set_ints(&self, name: &str, vals: &[i32]) -> Result<()> {
        let h = self.hctl.as_ref().context("no mixer")?;
        let elem = h
            .elem_iter()
            .find(|e| e.get_id().ok().and_then(|id| id.get_name().ok().map(|n| n == name)).unwrap_or(false))
            .with_context(|| format!("no mixer control `{name}`"))?;
        let count = elem.info()?.get_count();
        let mut v = ElemValue::new(ElemType::Integer)?;
        let id: ElemId = elem.get_id()?;
        v.set_id(&id);
        for i in 0..count {
            let x = vals[(i as usize).min(vals.len() - 1)];
            v.set_integer(i, x);
        }
        elem.write(&v)?;
        Ok(())
    }

    pub fn volume(&self) -> i32 {
        self.volume
    }

    /// Changes the volume by `delta` percent and applies it.
    pub fn volume_step(&mut self, delta: i32) -> i32 {
        self.volume = (self.volume + delta).clamp(0, 100);
        self.apply_volume();
        self.volume
    }

    fn apply_volume(&mut self) {
        if self.has(DAC_L) {
            let att = (100 - self.volume) * DAC_RANGE_STEPS / 100;
            let att = if self.volume == 0 { 255 } else { att };
            let _ = self.set_ints(DAC_L, &[att]);
            let _ = self.set_ints(DAC_R, &[att]);
            // Leave the software stage at full scale.
            let _ = self.set_ints(SOFT, &[255, 255]);
        } else {
            let v = self.volume * 255 / 100;
            let _ = self.set_ints(SOFT, &[v, v]);
        }
    }

    pub fn save(&self) {
        let _ = std::fs::write(&self.volume_file, format!("{}\n", self.volume));
    }

    /// Routes audio to whichever jack has a plug in it (headphones win).
    pub fn update_outputs(&mut self) {
        let lo = read_int(Path::new("/sys/class/switch/lineout/state"));
        let hs = read_int(Path::new("/sys/class/switch/headset/state"));
        // Rockbox mutes (0) when neither switch reports a plug; on the unit
        // this was developed on both read 0 with headphones in, so prefer
        // the headphone jack and never mute.
        let port = match (self.pinned_port, hs, lo) {
            (Some(p), _, _) => p,
            (_, Some(h), _) if h != 0 => 2,
            (_, _, Some(l)) if l != 0 => 1,
            _ => 2,
        };
        self.switches = (hs, lo);
        if self.last_port != Some(port) {
            if self.set_ints(PORT, &[port]).is_ok() {
                self.last_port = Some(port);
                // The attenuator is per port on some revisions; re-apply.
                self.apply_volume();
            } else {
                self.last_port = Some(port);
            }
        }
    }

    /// Steps the output port selector and pins it there (diagnostic).
    pub fn port_step(&mut self, delta: i32) -> i32 {
        let cur = self.last_port.unwrap_or(2);
        let next = (cur + delta).rem_euclid(6);
        self.pinned_port = Some(next);
        let _ = self.set_ints(PORT, &[next]);
        self.last_port = Some(next);
        next
    }

    pub fn port(&self) -> Option<i32> {
        self.last_port
    }

    /// What ALSA negotiated for the playback stream, once it is open.
    pub fn stream_params() -> Option<String> {
        let t = std::fs::read_to_string("/proc/asound/card0/pcm0p/sub0/hw_params").ok()?;
        let t = t.trim();
        if t.is_empty() || t == "closed" {
            return None;
        }
        Some(t.lines().collect::<Vec<_>>().join("; "))
    }

    pub fn battery(&self) -> Option<(i64, bool)> {
        for dir in ["/sys/class/power_supply/battery", "/sys/class/power_supply/axp_battery"] {
            let d = Path::new(dir);
            if let Some(cap) = read_int(&d.join("capacity")) {
                let charging = read_trim(&d.join("status")).is_some_and(|s| s.starts_with("Charging"));
                return Some((cap.clamp(0, 100), charging));
            }
        }
        None
    }

    pub fn screen_on(&self) -> bool {
        self.screen_on
    }

    pub fn screen(&mut self, on: bool) {
        self.screen_on = on;
        if let Some(d) = &self.backlight {
            let level = if on { self.brightness.as_str() } else { "0" };
            let _ = std::fs::write(d.join("brightness"), level);
            let _ = std::fs::write(d.join("bl_power"), if on { "0" } else { "1" });
        }
    }

    pub fn note_activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// Turns the backlight off after the configured idle time.
    pub fn tick(&mut self, playing_screen_stays: bool) {
        if let Some(t) = self.screen_timeout
            && self.screen_on
            && !playing_screen_stays
            && self.last_activity.elapsed() >= t
        {
            self.screen(false);
        }
    }

    pub fn power_off(&mut self) -> Result<()> {
        self.save();
        self.screen(false);
        // SAFETY: sync(2) has no preconditions.
        unsafe { libc::sync() };
        let _ = Command::new("/sbin/poweroff").status();
        let _ = Command::new("poweroff").status();
        std::thread::sleep(Duration::from_secs(3));
        // SAFETY: the last resort the init system did not take.
        unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_POWER_OFF) };
        std::thread::sleep(Duration::from_secs(5));
        anyhow::bail!("power off did not happen")
    }
}

fn find_backlight() -> Option<PathBuf> {
    for p in [
        "/sys/class/backlight/backlight",
        "/sys/class/backlight/pwm-backlight.0",
        "/sys/class/backlight/backlight_pwm0",
    ] {
        if Path::new(p).join("brightness").exists() {
            return Some(PathBuf::from(p));
        }
    }
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("brightness").exists())
}

/// Everything worth knowing when the player misbehaves. Each section is
/// handed to `emit` as soon as it is known, so a crash part-way still
/// leaves the earlier ones on disk.
pub fn diagnostics(mut emit: impl FnMut(&str)) {
    let mut sect = |title: &str, body: String| {
        emit(&format!("== {title}\n{body}\n"));
    };
    for f in ["/proc/version", "/proc/cmdline", "/proc/mounts"] {
        sect(f, std::fs::read_to_string(f).unwrap_or_else(|e| e.to_string()));
    }
    for f in ["/proc/cpuinfo", "/proc/meminfo"] {
        let t = std::fs::read_to_string(f).unwrap_or_else(|e| e.to_string());
        sect(f, t.lines().take(12).collect::<Vec<_>>().join("\n"));
    }
    let ls = |dir: &str| -> String {
        std::fs::read_dir(dir)
            .map(|d| {
                let mut v: Vec<String> = d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                v.sort();
                v.join(" ")
            })
            .unwrap_or_else(|e| e.to_string())
    };
    for d in ["/dev/input", "/dev/snd", "/sys/class/backlight", "/sys/class/switch", "/sys/class/power_supply", "/sys/class/graphics"] {
        sect(d, ls(d));
    }
    for d in ["/sys/class/switch/headset", "/sys/class/switch/lineout"] {
        sect(d, read_trim(&Path::new(d).join("state")).unwrap_or_default());
    }
    for d in ["/sys/class/power_supply/battery", "/sys/class/power_supply/axp_battery"] {
        let body = ["capacity", "status", "voltage_now"]
            .iter()
            .map(|k| format!("{k}={}", read_trim(&Path::new(d).join(k)).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(" ");
        sect(d, body);
    }
    if let Some(bl) = find_backlight() {
        let body = ["brightness", "max_brightness", "bl_power", "actual_brightness"]
            .iter()
            .map(|k| format!("{k}={}", read_trim(&bl.join(k)).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(" ");
        sect(&bl.display().to_string(), body);
    }
    match super::fbdev::LinuxFb::open("/dev/fb0") {
        Ok(fb) => sect("/dev/fb0", fb.describe()),
        Err(e) => sect("/dev/fb0", e.to_string()),
    }
    sect(
        "env",
        std::env::vars().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n"),
    );
    for f in ["/proc/bus/input/devices"] {
        sect(f, std::fs::read_to_string(f).unwrap_or_else(|e| e.to_string()));
    }
    sect("alsa", "opening hw:0 control (if this is the last section, that crashed)".into());
    match HCtl::new("hw:0", false) {
        Ok(h) => {
            let _ = h.load();
            let mut body = String::new();
            for e in h.elem_iter() {
                let name = e.get_id().ok().and_then(|i| i.get_name().map(String::from).ok()).unwrap_or_default();
                let (ty, count) = e.info().map(|i| (format!("{:?}", i.get_type()), i.get_count())).unwrap_or_default();
                let val = e
                    .read()
                    .ok()
                    .map(|v| (0..count).filter_map(|i| v.get_integer(i)).map(|x| x.to_string()).collect::<Vec<_>>().join(","))
                    .unwrap_or_default();
                body.push_str(&format!("{name}: {ty} x{count} = {val}\n"));
            }
            sect("alsa hw:0 controls", body);
        }
        Err(e) => sect("alsa hw:0 controls", e.to_string()),
    }
    sect("done", String::new());
}
