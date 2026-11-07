//! The main loop on the player: vinyl's own loop from `external/vinyl/src/main.rs`
//! with the terminal replaced by a [`Platform`] (buttons, volume, power) and
//! a small overlay for battery and volume. The key handlers are vinyl's,
//! copied verbatim so the buttons drive exactly the same commands as the
//! keyboard does on a desktop.
use super::keys::{Action, Ctx};
use crate::app::{self, App};
use crate::ui;
use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Clear, Paragraph};
use std::time::{Duration, Instant};

const SEEK_SECS: f64 = 5.0;
/// How often the whole screen is rewritten cell by cell (vinyl does the same
/// to paint over stray terminal output; here it also heals a glitched panel).
const FULL_REPAINT: Duration = Duration::from_secs(5);
const POPUP_FOR: Duration = Duration::from_millis(1500);

/// The device side of the loop.
pub trait Platform {
    /// Waits up to `timeout` for buttons, firing held-button repeats on time.
    fn wait(&mut self, timeout: Duration, ctx: &Ctx) -> Result<Vec<Action>>;
    /// Changes the volume by `delta` percent; returns text for the popup.
    fn volume_step(&mut self, delta: i32) -> Option<String>;
    fn screen_toggle(&mut self);
    fn power_off(&mut self) -> Result<()>;
    /// Short status text for the top-right corner (battery, charging).
    fn status(&mut self) -> Option<String>;
    /// Housekeeping once per tick: output jack detection, screen timeout.
    fn tick(&mut self);
    /// Diagnostic: step the DAC output port selector; returns popup text.
    fn port_step(&mut self, delta: i32) -> Option<String>;
}

pub fn ctx_of(app: &App) -> Ctx {
    Ctx {
        dialog: app.fx_open || app.eq_open || app.sort_open,
        search: app.search_input.is_some(),
        section: app.section,
    }
}

pub fn run<B, P>(terminal: &mut Terminal<B>, app: &mut App, plat: &mut P) -> Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
    P: Platform,
{
    let mut last_tick = Instant::now();
    let mut last_repaint = Instant::now();
    let mut popup: Option<(String, Instant)> = None;
    let mut status: Option<String> = plat.status();
    let mut last_status = Instant::now();
    let mut first_frame = true;
    let mut last_spectrum = Instant::now() - Duration::from_secs(1);
    loop {
        if last_spectrum.elapsed() >= Duration::from_millis(100) {
            app.update_spectrum();
            last_spectrum = Instant::now();
        }
        let repaint = last_repaint.elapsed() >= FULL_REPAINT;
        if let Some((_, at)) = &popup
            && at.elapsed() >= POPUP_FOR
        {
            popup = None;
        }
        let mut snapshot = None;
        terminal.draw(|f| {
            ui::draw(f, app);
            overlay(f, status.as_deref(), popup.as_ref().map(|p| p.0.as_str()));
            if repaint {
                snapshot = Some(f.buffer_mut().clone());
            }
        })?;
        if first_frame {
            first_frame = false;
            #[cfg(target_os = "linux")]
            super::trace::step(12);
        }
        if let Some(buf) = snapshot {
            let backend = terminal.backend_mut();
            backend.draw(buf.content.iter().enumerate().map(|(i, c)| {
                let (x, y) = buf.pos_of(i);
                (x, y, c)
            }))?;
            backend.flush()?;
            last_repaint = Instant::now();
        }
        // Vinyl redraws at 20 fps while playing; 10 fps is plenty on a
        // 320x240 panel and leaves the CPU to the audio thread.
        let tick = if app.audio_active() {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(200)
        };
        let wait = tick.saturating_sub(last_tick.elapsed());
        let ctx = ctx_of(app);
        for action in plat.wait(wait, &ctx)? {
            match action {
                Action::Key(code) => handle_key(app, code, KeyModifiers::NONE),
                Action::VolUp => popup = plat.volume_step(5).map(|s| (s, Instant::now())),
                Action::VolDown => popup = plat.volume_step(-5).map(|s| (s, Instant::now())),
                Action::ScreenToggle => plat.screen_toggle(),
                Action::PortStep(d) => popup = plat.port_step(d).map(|s| (s, Instant::now())),
                Action::PowerOff => {
                    if let Some(a) = &app.audio {
                        a.pause();
                    }
                    plat.power_off()?;
                    app.should_quit = true;
                }
            }
            if app.should_quit {
                break;
            }
        }
        // No editor on the player: a request to open one is dropped.
        app.pending_editor = None;
        if last_tick.elapsed() >= tick {
            app.tick()?;
            plat.tick();
            last_tick = Instant::now();
        }
        if last_status.elapsed() >= Duration::from_secs(10) {
            status = plat.status();
            last_status = Instant::now();
        }
        app.flush_play();
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Battery in the bottom-right corner, and a volume popup in the middle.
pub fn overlay(f: &mut ratatui::Frame, status: Option<&str>, popup: Option<&str>) {
    let area = f.area();
    if let Some(s) = status {
        // Bottom-right corner, over the end of the key hints; a leading space
        // keeps it from running into whatever was there.
        let w = (s.chars().count() as u16 + 1).min(area.width);
        let r = Rect {
            x: area.width - w,
            y: area.height.saturating_sub(1),
            width: w,
            height: 1,
        };
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(format!(" {s}"))
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::DarkGray)),
            r,
        );
    }
    if let Some(p) = popup {
        let w = (p.chars().count() as u16 + 4).min(area.width);
        let r = Rect {
            x: (area.width - w) / 2,
            y: area.height / 2,
            width: w,
            height: 1,
        };
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(p)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Black).bg(Color::Yellow)),
            r,
        );
    }
}

// --- vinyl's key handlers (external/vinyl/src/main.rs), unchanged ---------

pub fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    if app.search_input.is_some() {
        handle_search_key(app, code, mods);
        return;
    }
    if app.fx_open {
        handle_fx_key(app, code);
        return;
    }
    if app.eq_open {
        handle_eq_key(app, code);
        return;
    }
    if app.sort_open {
        match code {
            KeyCode::Esc | KeyCode::Char('o') | KeyCode::Char('q') => app.sort_open = false,
            KeyCode::Enter | KeyCode::Char(' ') => app.apply_sort(),
            KeyCode::Up | KeyCode::Char('k') => app.sort_move(-1),
            KeyCode::Down | KeyCode::Char('j') => app.sort_move(1),
            KeyCode::Char('a') => {
                app.sort_row = 0;
                app.apply_sort()
            }
            KeyCode::Char('t') => {
                app.sort_row = 1;
                app.apply_sort()
            }
            KeyCode::Char('b') => {
                app.sort_row = 2;
                app.apply_sort()
            }
            KeyCode::Char('r') => {
                app.sort_row = 3;
                app.apply_sort()
            }
            _ => {}
        }
        return;
    }
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => app.should_quit = true,
        KeyCode::Char('s') => app.section = app::Section::Songs,
        KeyCode::Char('a') => app.section = app::Section::Albums,
        KeyCode::Char('r') => app.section = app::Section::Artists,
        KeyCode::Char(' ') => app.toggle_play(),
        KeyCode::Char('n') => app.step_track(1),
        KeyCode::Char('p') => app.step_track(-1),
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::Home => app.move_cursor(i64::MIN / 2),
        KeyCode::End => app.move_cursor(i64::MAX / 2),
        KeyCode::Left | KeyCode::Char(',') => app.seek(-SEEK_SECS),
        KeyCode::Right | KeyCode::Char('.') => app.seek(SEEK_SECS),
        KeyCode::Char('=') | KeyCode::Char('+') => app.adjust_speed(1),
        KeyCode::Char('-') | KeyCode::Char('_') => app.adjust_speed(-1),
        KeyCode::PageUp => app.move_cursor(-(app.page_size as i64)),
        KeyCode::PageDown => app.move_cursor(app.page_size as i64),
        KeyCode::Char('0') => app.reset_speed(),
        KeyCode::Enter => app.activate_cursor(),
        KeyCode::Char('/') => app.open_search(),
        KeyCode::Char('x') => app.fx_open = true,
        KeyCode::Char('e') => app.eq_open = true,
        KeyCode::Char('o') => app.open_sort(),
        KeyCode::Char('l') => app.toggle_loudness(),
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Char('f') => app.reveal_in_finder(),
        KeyCode::Char('E') => app.edit_in_editor(),
        KeyCode::Char('i') => app.start_index(true),
        _ => {}
    }
}

fn handle_search_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    let Some(buf) = app.search_input.as_mut() else {
        return;
    };
    match code {
        KeyCode::Esc => app.cancel_search(),
        KeyCode::Enter => app.submit_search(),
        KeyCode::Backspace => {
            buf.pop();
        }
        KeyCode::Char('u') if mods.contains(KeyModifiers::CONTROL) => buf.clear(),
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => app.cancel_search(),
        KeyCode::Char(c) => buf.push(c),
        _ => {}
    }
}

fn handle_fx_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('x') | KeyCode::Char('q') => app.close_fx(),
        KeyCode::Up | KeyCode::Char('k') => app.fx_move(-1),
        KeyCode::Down | KeyCode::Char('j') => app.fx_move(1),
        KeyCode::Left | KeyCode::Char('h') => app.fx_adjust(-5),
        KeyCode::Right | KeyCode::Char('l') => app.fx_adjust(5),
        KeyCode::Char('-') | KeyCode::Char('_') => app.fx_adjust(-1),
        KeyCode::Char('=') | KeyCode::Char('+') => app.fx_adjust(1),
        KeyCode::Char(' ') => app.fx_adjust(5),
        KeyCode::Char('0') => app.fx_reset_row(),
        _ => {}
    }
}

fn handle_eq_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('q') => app.close_eq(),
        KeyCode::Up | KeyCode::Char('k') => app.eq_move(-1),
        KeyCode::Down | KeyCode::Char('j') => app.eq_move(1),
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') | KeyCode::Char('_') => {
            app.eq_adjust(-1)
        }
        KeyCode::Right
        | KeyCode::Char('l')
        | KeyCode::Char('=')
        | KeyCode::Char('+')
        | KeyCode::Char(' ') => app.eq_adjust(1),
        KeyCode::PageUp => app.eq_adjust(3),
        KeyCode::PageDown => app.eq_adjust(-3),
        KeyCode::Char('0') => app.eq_reset_row(),
        _ => {}
    }
}
