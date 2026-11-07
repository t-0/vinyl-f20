//! Entry point on the player.
//!
//! The Rockbox hosted bootloader has mounted the SD card at /mnt/sd_0 and
//! exec'd this binary (as /tmp/rockbox.erosq). Everything vinyl writes goes
//! under `/mnt/sd_0/.vinyl`: its config, index, FX settings and log, plus
//! this layer's `f20.conf`, `volume`, `boot.log` and `diag.txt`.
use super::fb::FbBackend;
use super::fbdev::LinuxFb;
use super::font::{self, Font};
use super::platform::DevicePlatform;
use super::settings::Settings;
use crate::app::{self, App};
use crate::{audio, config, stderr};
use anyhow::{Context, Result};
use ratatui::Terminal;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const CANDIDATES: [&str; 3] = ["/mnt/sd_0", "/mnt/sdcard", "/mnt/mmc"];

fn is_mounted(p: &str) -> bool {
    std::fs::read_to_string("/proc/mounts")
        .map(|m| m.lines().any(|l| l.split_whitespace().nth(1) == Some(p)))
        .unwrap_or(false)
}

/// Where the SD card is. The bootloader mounts it; if not, try ourselves.
fn find_sd() -> PathBuf {
    for c in CANDIDATES {
        if is_mounted(c) {
            return PathBuf::from(c);
        }
    }
    let target = CANDIDATES[0];
    let _ = std::fs::create_dir_all(target);
    for dev in ["/dev/mmcblk0p1", "/dev/mmcblk0"] {
        if Path::new(dev).exists() && Command::new("mount").args([dev, target]).status().is_ok_and(|s| s.success()) {
            break;
        }
    }
    PathBuf::from(target)
}

struct BootLog(Option<std::fs::File>);

impl BootLog {
    fn open(p: &Path) -> BootLog {
        BootLog(OpenOptions::new().create(true).append(true).open(p).ok())
    }
    fn line(&mut self, s: &str) {
        if let Some(f) = &mut self.0 {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{t}] {s}");
        }
    }
}

pub fn main(args: Vec<String>) -> Result<()> {
    let sd = find_sd();
    let home = sd.join(".vinyl");
    std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
    let mut boot = BootLog::open(&home.join("boot.log"));
    boot.line(&format!("vinyl-f20 {} starting, sd at {}", env!("CARGO_PKG_VERSION"), sd.display()));
    if let Some(f) = &boot.0 {
        use std::os::unix::io::AsRawFd;
        super::trace::install(f.as_raw_fd());
    }
    // Before any thread exists: vinyl keeps its files under $HOME.
    // SAFETY: single-threaded at this point.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("VINYL_CONFIG", home.join("config"));
        std::env::set_var("ALSA_CONFIG_PATH", home.join("alsa").join("alsa.conf"));
    }
    let config_path = home.join("config");
    if !config_path.exists() {
        let music = sd.join("Music");
        let _ = std::fs::create_dir_all(&music);
        std::fs::write(
            &config_path,
            format!("music.home = {}\nindex.file = {}\n", music.display(), home.join("index.json").display()),
        )?;
    }
    let settings = Settings::load(&home.join("f20.conf"));
    let panic_log = home.join("boot.log");
    let panic_settings = settings.clone();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = OpenOptions::new().append(true).open(&panic_log) {
            let _ = writeln!(f, "PANIC: {info}");
        }
        // Best effort: put it on the panel too, then the abort reboots us.
        let _ = show_message(&panic_settings, &format!("vinyl panicked:\n{info}\n\nSee .vinyl/boot.log"), 10);
    }));

    let mode_file = home.join("mode");
    let mode = args
        .first()
        .cloned()
        .or_else(|| std::fs::read_to_string(&mode_file).ok().map(|s| format!("--{}", s.trim())));
    let _ = std::fs::remove_file(&mode_file);
    match mode.as_deref() {
        Some("--probe") => {
            let path = args.get(1).context("usage: --probe <audio file>")?;
            let config = config::load()?;
            let report = audio::probe(Path::new(path), app::load_fx(&config))?;
            std::fs::write(home.join("probe.txt"), &report)?;
            print!("{report}");
            return Ok(());
        }
        Some("--diag") => {
            boot.line("writing diag.txt");
            let mut out = std::fs::File::create(home.join("diag.txt"))?;
            super::hw::diagnostics(|section| {
                let _ = out.write_all(section.as_bytes());
                let _ = out.sync_all();
            });
            show_message(&settings, "diag.txt written to .vinyl", 4)?;
            return Ok(());
        }
        Some("--audiotest") => {
            boot.line("audio test");
            let config = config::load()?;
            let fx = app::load_fx(&config);
            let files: Vec<PathBuf> = if args.len() > 1 {
                args[1..].iter().map(Into::into).collect()
            } else {
                super::audiotest::sample_files(&config.music_home)
            };
            let mut text = String::new();
            let mut summary = String::new();
            for f in &files {
                let r = super::audiotest::report(f, fx);
                text.push_str(&r);
                let _ = std::fs::write(home.join("audiotest.txt"), &text);
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let dec = super::audiotest::measure(f, None).map(|s| s.verdict()).unwrap_or("error");
                let fxv = super::audiotest::measure(f, Some(fx)).map(|s| s.verdict()).unwrap_or("error");
                summary.push_str(&format!("{name}: decoder {dec}, fx {fxv}\n"));
            }
            if files.is_empty() {
                summary.push_str("no audio files found under Music\n");
            }
            show_message(&settings, &format!("audio test (see .vinyl/audiotest.txt)\n{summary}"), 12)?;
            return Ok(());
        }
        Some("--fbtest") => {
            boot.line("framebuffer test");
            return fb_test(&settings);
        }
        _ => {}
    }

    // Crash-loop breaker: the bootloader script reboots whenever we exit, and
    // the boot menu re-selects Rockbox after a second, so a start that keeps
    // failing would loop forever. Count unclean starts; after three, show
    // the reason and boot the original firmware instead.
    let marker = home.join(".running");
    let attempts_file = home.join("failed-starts");
    let mut failed: u32 = std::fs::read_to_string(&attempts_file)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    if marker.exists() {
        failed += 1;
        boot.line(&format!("previous run did not exit cleanly ({failed} in a row); see .vinyl.log and boot.log"));
        let _ = std::fs::write(&attempts_file, failed.to_string());
    }
    if failed >= MAX_FAILED_STARTS {
        boot.line("too many failed starts: handing over to the original firmware");
        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::write(&attempts_file, "0");
        let _ = show_message(
            &settings,
            &format!("vinyl failed to start {failed} times.\nBooting the original firmware.\nSee .vinyl/boot.log and .vinyl/.vinyl.log on the card."),
            8,
        );
        boot_original_firmware();
    }
    let _ = std::fs::write(&marker, "");
    let _quiet = stderr::Redirect::to_file(&config::log_path());
    let result = run_player(&settings, &home, &mut boot);
    match &result {
        Ok(()) => {
            boot.line("exited normally");
            let _ = std::fs::write(&attempts_file, "0");
        }
        Err(e) => {
            boot.line(&format!("failed: {e:#}"));
            let _ = std::fs::write(&attempts_file, (failed + 1).to_string());
        }
    }
    let _ = std::fs::remove_file(&marker);
    if let Err(e) = &result {
        // Leave the reason on screen before the bootloader script reboots.
        let _ = show_message(&settings, &format!("vinyl failed: {e:#}"), 10);
    }
    result
}

const MAX_FAILED_STARTS: u32 = 3;

/// Replaces this process with the stock player, as the bootloader's
/// "Original firmware" entry does.
fn boot_original_firmware() {
    use std::os::unix::process::CommandExt;
    let _ = std::fs::remove_file("/tmp/rockbox.erosq");
    let err = Command::new("/usr/bin/hiby_player").exec();
    // exec only returns on failure; fall back to a plain exit (and a reboot).
    eprintln!("could not start the original firmware: {err}");
    std::process::exit(1);
}

fn load_font(settings: &Settings) -> Result<Font> {
    Font::named(&settings.font).or_else(|_| Font::named(font::DEFAULT))
}

fn run_player(settings: &Settings, home: &Path, boot: &mut BootLog) -> Result<()> {
    use super::trace::step;
    step(1);
    let config = config::load().context("loading vinyl config")?;
    boot.line(&format!("music.home = {}", config.music_home.display()));
    step(2);
    let fbdev = LinuxFb::open("/dev/fb0").context("opening /dev/fb0")?;
    boot.line(&fbdev.describe());
    step(3);
    let (w, h) = (fbdev.width(), fbdev.height());
    let backend = FbBackend::new(load_font(settings)?, w, h, fbdev)?;
    let mut terminal = Terminal::new(backend)?;
    let mut plat = DevicePlatform::new(home, settings);
    boot.line(&plat.describe());
    step(10);
    let mut app = App::new(config);
    if let Some(e) = &app.audio_error {
        boot.line(&format!("audio: {e}"));
    }
    boot.line("audio and index started");
    // The audio callback thread already exists; make this (UI) thread yield
    // to it. On Linux setpriority with a thread id affects only that thread.
    // SAFETY: plain syscalls with no memory arguments.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
    }
    step(11);
    let r = super::run::run(&mut terminal, &mut app, &mut plat);
    step(13);
    plat.hw.save();
    r
}

/// A line of text on the panel for `secs` seconds (used when the UI cannot run).
fn show_message(settings: &Settings, msg: &str, secs: u64) -> Result<()> {
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Paragraph, Wrap};
    let fbdev = LinuxFb::open("/dev/fb0")?;
    let (w, h) = (fbdev.width(), fbdev.height());
    let backend = FbBackend::new(load_font(settings)?, w, h, fbdev)?;
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| {
        f.render_widget(
            Paragraph::new(msg.to_string())
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: true }),
            f.area(),
        );
    })?;
    std::thread::sleep(Duration::from_secs(secs));
    Ok(())
}

/// Colour bars and a font sample: the first thing to try on a new unit.
fn fb_test(settings: &Settings) -> Result<()> {
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Block, Paragraph};
    let fbdev = LinuxFb::open("/dev/fb0")?;
    let info = fbdev.describe();
    let (w, h) = (fbdev.width(), fbdev.height());
    let backend = FbBackend::new(load_font(settings)?, w, h, fbdev)?;
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| {
        let area = f.area();
        let colors = [Color::Red, Color::Green, Color::Blue, Color::Yellow, Color::Cyan, Color::Magenta, Color::White, Color::DarkGray];
        let bw = area.width / colors.len() as u16;
        for (i, c) in colors.iter().enumerate() {
            let r = Rect { x: i as u16 * bw, y: 0, width: bw, height: 3 };
            f.render_widget(Block::default().style(Style::default().bg(*c)), r);
        }
        let text = format!(
            "vinyl-f20 framebuffer test\n{}x{} cells\n{info}\nABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789\n┌──┐ ░▒▓█ ▁▂▃▄▅▆▇█\n└──┘ the quick brown fox jumps over the lazy dog",
            area.width, area.height
        );
        let r = Rect { x: 0, y: 4, width: area.width, height: area.height - 4 };
        f.render_widget(Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: false }), r);
    })?;
    std::thread::sleep(Duration::from_secs(6));
    Ok(())
}
