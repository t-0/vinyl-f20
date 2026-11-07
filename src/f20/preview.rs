//! Desktop side: renders the player's screen to a PNG so the 320x240 layout
//! can be checked without the device. Uses the desktop's own vinyl config
//! and music library, exactly as `vinyl` would.
//!
//!   vinyl-f20 --preview out.png [--font 6x12] [--dialog fx|eq|sort] [--play] [--text]
use super::fb::{Capture, FbBackend};
use super::font::{self, Font};
use crate::app::{self, App};
use crate::{audio, config, ui};
use anyhow::{Context, Result, bail};
use ratatui::Terminal;
use std::path::Path;
use std::time::Duration;

pub fn main(args: Vec<String>) -> Result<()> {
    if args.first().map(String::as_str) == Some("--probe") {
        let path = args.get(1).context("usage: --probe <audio file>")?;
        let config = config::load()?;
        print!("{}", audio::probe(Path::new(path), app::load_fx(&config))?);
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--audiotest") {
        let config = config::load()?;
        let fx = app::load_fx(&config);
        let files: Vec<std::path::PathBuf> = if args.len() > 1 {
            args[1..].iter().map(Into::into).collect()
        } else {
            super::audiotest::sample_files(&config.music_home)
        };
        for f in files {
            print!("{}", super::audiotest::report(&f, fx));
        }
        return Ok(());
    }
    let mut out: Option<String> = None;
    let mut font_name = font::DEFAULT.to_string();
    let mut dialog: Option<String> = None;
    let mut play = false;
    let mut text = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--preview" => out = it.next(),
            "--font" => font_name = it.next().unwrap_or_default(),
            "--dialog" => dialog = it.next(),
            "--play" => play = true,
            "--text" => text = true,
            other => bail!("unknown argument {other}; on a desktop this build only renders previews (--preview out.png [--font 5x8|6x12|8x16] [--dialog fx|eq|sort] [--play] [--text]) or measures files (--audiotest [files])"),
        }
    }
    let Some(out) = out else {
        bail!("usage: vinyl-f20 --preview out.png [--font 5x8|6x12|8x16] [--dialog fx|eq|sort] [--play] [--text]");
    };
    let config = config::load()?;
    let mut app = App::new(config);
    // Let the background indexer finish so the lists have content.
    for _ in 0..60 {
        app.tick()?;
        if app.indexing.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if play {
        app.activate_cursor();
        for _ in 0..10 {
            app.tick()?;
            app.flush_play();
            std::thread::sleep(Duration::from_millis(100));
        }
        app.update_spectrum();
    }
    match dialog.as_deref() {
        Some("fx") => app.fx_open = true,
        Some("eq") => app.eq_open = true,
        Some("sort") => app.open_sort(),
        Some(d) => bail!("unknown dialog {d}"),
        None => {}
    }
    let backend = FbBackend::new(Font::named(&font_name)?, 320, 240, Capture::default())?;
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| {
        ui::draw(f, &mut app);
        super::run::overlay(f, Some("87%"), None);
    })?;
    if let Some(a) = &app.audio {
        a.pause();
    }
    let b = terminal.backend();
    let cap = b.sink();
    super::png::write(Path::new(&out), &cap.frame, cap.width, cap.height)?;
    if text {
        print!("{}", b.text());
    }
    let (c, r) = b.cells();
    println!("wrote {out}: 320x240, {c}x{r} cells, font {font_name}");
    Ok(())
}
