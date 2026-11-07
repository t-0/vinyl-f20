//! The F20's buttons and wheel, mapped onto vinyl's keyboard commands.
//!
//! The player has a scroll wheel plus Play, Prev, Next, Menu, Back, Vol+,
//! Vol- and Power. A short press and a long press of each do different
//! things, and Prev/Next repeat while held. All of that is decided here from
//! timestamps, so it can be tested without a device.
//!
//! | Button      | Short press                               | Held                          |
//! |-------------|-------------------------------------------|-------------------------------|
//! | Wheel       | move selection up / down                  |                               |
//! | Play        | play selection / confirm                  | play / pause                  |
//! | Prev / Next | previous / next track (dialogs: adjust)   | rewind / fast-forward         |
//! | Menu        | next list: songs, albums, artists         | sort order                    |
//! | Back        | FX dialog (dialogs: close)                | equaliser                     |
//! | Vol + / -   | volume                                    | keeps changing                |
//! | Power       | screen on / off                           | 2 s: power off                |
//!
//! Menu is also a modifier: with Menu held, Play toggles shuffle, Back
//! toggles loudness, Prev/Next change playback speed, and Vol-/Vol+ step the
//! speed back to normal, and the wheel steps the DAC output port selector
//! (a diagnostic). A chord cancels Menu's own short and long actions.
use crate::app::Section;
use ratatui::crossterm::event::KeyCode;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Power,
    Menu,
    Back,
    Prev,
    Next,
    Play,
    WheelBack,
    WheelFwd,
    VolUp,
    VolDown,
}

/// Linux input event codes (input-event-codes.h) as the HiBy kernel sends
/// them. Rockbox found KEY_NEXTSONG/KEY_PREVIOUSSONG reversed relative to the
/// button labels, so that is the default; `swap` flips it.
pub fn button_for(code: u16, swap: bool) -> Option<Button> {
    Some(match code {
        116 => Button::Power,       // KEY_POWER
        139 => Button::Menu,        // KEY_MENU
        158 => Button::Back,        // KEY_BACK
        164 | 28 => Button::Play,   // KEY_PLAYPAUSE, KEY_ENTER
        163 => if swap { Button::Next } else { Button::Prev }, // KEY_NEXTSONG
        165 => if swap { Button::Prev } else { Button::Next }, // KEY_PREVIOUSSONG
        105 | 103 => Button::WheelBack, // KEY_LEFT, KEY_UP
        106 | 108 => Button::WheelFwd,  // KEY_RIGHT, KEY_DOWN
        115 => Button::VolUp,       // KEY_VOLUMEUP
        114 => Button::VolDown,     // KEY_VOLUMEDOWN
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Key(KeyCode),
    VolUp,
    VolDown,
    ScreenToggle,
    PowerOff,
    /// Menu + wheel: step the DAC's output port selector (diagnostic).
    PortStep(i32),
}

/// What the app is showing, which changes what a button means.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    /// An FX, EQ or sort dialog is open.
    pub dialog: bool,
    /// The search box is open.
    pub search: bool,
    pub section: Section,
}

struct Held {
    button: Button,
    since: Instant,
    long_fired: bool,
    last_repeat: Instant,
    /// Took part in a chord: no further actions from this press.
    consumed: bool,
}

pub struct Mapper {
    held: Vec<Held>,
    pub long_press: Duration,
    pub seek_repeat: Duration,
    pub vol_repeat: Duration,
    pub power_hold: Duration,
}

impl Default for Mapper {
    fn default() -> Self {
        Self {
            held: Vec::new(),
            long_press: Duration::from_millis(600),
            seek_repeat: Duration::from_millis(150),
            vol_repeat: Duration::from_millis(120),
            power_hold: Duration::from_millis(2000),
        }
    }
}

fn key(k: KeyCode) -> Action {
    Action::Key(k)
}

impl Mapper {
    /// A button went down (`pressed`) or up. Returns what to do now.
    pub fn event(&mut self, b: Button, pressed: bool, now: Instant, ctx: &Ctx) -> Vec<Action> {
        if pressed {
            self.press(b, now, ctx)
        } else {
            self.release(b, now, ctx)
        }
    }

    fn press(&mut self, b: Button, now: Instant, ctx: &Ctx) -> Vec<Action> {
        self.held.retain(|h| h.button != b);
        if b != Button::Menu
            && let Some(menu) = self.held.iter_mut().find(|h| h.button == Button::Menu && !h.long_fired)
        {
            let chord = match b {
                Button::Play => Some(key(KeyCode::Char('z'))),
                Button::Back => Some(key(KeyCode::Char('l'))),
                Button::Next => Some(key(KeyCode::Char('='))),
                Button::Prev => Some(key(KeyCode::Char('-'))),
                Button::VolUp | Button::VolDown => Some(key(KeyCode::Char('0'))),
                Button::WheelFwd => Some(Action::PortStep(1)),
                Button::WheelBack => Some(Action::PortStep(-1)),
                Button::Power | Button::Menu => None,
            };
            if let Some(a) = chord {
                menu.consumed = true;
                if !matches!(b, Button::WheelFwd | Button::WheelBack) {
                    self.held.push(Held {
                        button: b,
                        since: now,
                        long_fired: true,
                        last_repeat: now,
                        consumed: true,
                    });
                }
                return vec![a];
            }
        }
        let immediate = match b {
            Button::WheelBack => return vec![key(KeyCode::Up)],
            Button::WheelFwd => return vec![key(KeyCode::Down)],
            Button::Prev if ctx.dialog => vec![key(KeyCode::Left)],
            Button::Next if ctx.dialog => vec![key(KeyCode::Right)],
            Button::VolUp => vec![Action::VolUp],
            Button::VolDown => vec![Action::VolDown],
            _ => vec![],
        };
        self.held.push(Held {
            button: b,
            since: now,
            long_fired: false,
            last_repeat: now,
            consumed: false,
        });
        immediate
    }

    fn release(&mut self, b: Button, _now: Instant, ctx: &Ctx) -> Vec<Action> {
        let Some(i) = self.held.iter().position(|h| h.button == b) else {
            return vec![];
        };
        let h = self.held.remove(i);
        if h.long_fired || h.consumed {
            return vec![];
        }
        match b {
            Button::Play => vec![key(KeyCode::Enter)],
            Button::Prev if !ctx.dialog && !ctx.search => vec![key(KeyCode::Char('p'))],
            Button::Next if !ctx.dialog && !ctx.search => vec![key(KeyCode::Char('n'))],
            Button::Menu if ctx.dialog || ctx.search => vec![key(KeyCode::Enter)],
            Button::Menu => vec![key(KeyCode::Char(match ctx.section {
                Section::Songs => 'a',
                Section::Albums => 'r',
                Section::Artists => 's',
            }))],
            Button::Back if ctx.dialog || ctx.search => vec![key(KeyCode::Esc)],
            Button::Back => vec![key(KeyCode::Char('x'))],
            Button::Power => vec![Action::ScreenToggle],
            _ => vec![],
        }
    }

    /// Call regularly while buttons are held: fires long presses and repeats.
    pub fn tick(&mut self, now: Instant, ctx: &Ctx) -> Vec<Action> {
        let mut out = Vec::new();
        let (long, seek_rep, vol_rep, power) =
            (self.long_press, self.seek_repeat, self.vol_repeat, self.power_hold);
        for h in &mut self.held {
            if h.consumed {
                continue;
            }
            let held_for = now.duration_since(h.since);
            match h.button {
                Button::Prev | Button::Next => {
                    if held_for >= long && now.duration_since(h.last_repeat) >= seek_rep {
                        h.long_fired = true;
                        h.last_repeat = now;
                        out.push(key(if h.button == Button::Prev {
                            KeyCode::Left
                        } else {
                            KeyCode::Right
                        }));
                    }
                }
                Button::VolUp | Button::VolDown => {
                    if held_for >= long && now.duration_since(h.last_repeat) >= vol_rep {
                        h.last_repeat = now;
                        out.push(if h.button == Button::VolUp {
                            Action::VolUp
                        } else {
                            Action::VolDown
                        });
                    }
                }
                Button::Play if !h.long_fired && held_for >= long && !ctx.dialog && !ctx.search => {
                    h.long_fired = true;
                    out.push(key(KeyCode::Char(' ')));
                }
                Button::Menu if !h.long_fired && held_for >= long && !ctx.dialog && !ctx.search => {
                    h.long_fired = true;
                    out.push(key(KeyCode::Char('o')));
                }
                Button::Back if !h.long_fired && held_for >= long && !ctx.dialog && !ctx.search => {
                    h.long_fired = true;
                    out.push(key(KeyCode::Char('e')));
                }
                Button::Power if !h.long_fired && held_for >= power => {
                    h.long_fired = true;
                    out.push(Action::PowerOff);
                }
                _ => {}
            }
        }
        out
    }

    /// How long until `tick` could have something to do, if anything is held.
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        self.held
            .iter()
            .map(|h| {
                let held_for = now.duration_since(h.since);
                match h.button {
                    Button::Prev | Button::Next => {
                        if held_for < self.long_press {
                            self.long_press - held_for
                        } else {
                            self.seek_repeat.saturating_sub(now.duration_since(h.last_repeat))
                        }
                    }
                    Button::VolUp | Button::VolDown => {
                        if held_for < self.long_press {
                            self.long_press - held_for
                        } else {
                            self.vol_repeat.saturating_sub(now.duration_since(h.last_repeat))
                        }
                    }
                    Button::Power => self.power_hold.saturating_sub(held_for),
                    _ => self.long_press.saturating_sub(held_for),
                }
            })
            .min()
            .map(|d| d.max(Duration::from_millis(10)))
    }

    pub fn anything_held(&self) -> bool {
        !self.held.is_empty()
    }

    /// Forgets every held button, so their releases do nothing.
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            dialog: false,
            search: false,
            section: Section::Songs,
        }
    }

    fn dialog() -> Ctx {
        Ctx {
            dialog: true,
            ..ctx()
        }
    }

    #[test]
    fn wheel_moves_the_selection_immediately() {
        let mut m = Mapper::default();
        let t = Instant::now();
        assert_eq!(m.event(Button::WheelFwd, true, t, &ctx()), vec![key(KeyCode::Down)]);
        assert_eq!(m.event(Button::WheelBack, true, t, &ctx()), vec![key(KeyCode::Up)]);
        assert!(m.event(Button::WheelFwd, false, t, &ctx()).is_empty());
    }

    #[test]
    fn short_play_is_enter_and_long_play_toggles_pause() {
        let mut m = Mapper::default();
        let t = Instant::now();
        assert!(m.event(Button::Play, true, t, &ctx()).is_empty());
        assert_eq!(
            m.event(Button::Play, false, t + Duration::from_millis(100), &ctx()),
            vec![key(KeyCode::Enter)]
        );
        m.event(Button::Play, true, t, &ctx());
        assert!(m.tick(t + Duration::from_millis(300), &ctx()).is_empty());
        assert_eq!(
            m.tick(t + Duration::from_millis(700), &ctx()),
            vec![key(KeyCode::Char(' '))]
        );
        assert!(m.tick(t + Duration::from_millis(900), &ctx()).is_empty(), "fires once");
        assert!(m.event(Button::Play, false, t + Duration::from_secs(1), &ctx()).is_empty());
    }

    #[test]
    fn next_skips_on_tap_and_scans_while_held() {
        let mut m = Mapper::default();
        let t = Instant::now();
        m.event(Button::Next, true, t, &ctx());
        assert_eq!(
            m.event(Button::Next, false, t + Duration::from_millis(50), &ctx()),
            vec![key(KeyCode::Char('n'))]
        );
        m.event(Button::Prev, true, t, &ctx());
        let mut seeks = 0;
        let mut now = t;
        while now < t + Duration::from_millis(1200) {
            seeks += m.tick(now, &ctx()).len();
            now += Duration::from_millis(50);
        }
        assert!((3..=5).contains(&seeks), "{seeks} seek steps in 600 ms of scanning");
        assert!(m.event(Button::Prev, false, now, &ctx()).is_empty(), "no skip after a scan");
    }

    #[test]
    fn in_a_dialog_prev_next_adjust_and_back_closes() {
        let mut m = Mapper::default();
        let t = Instant::now();
        assert_eq!(m.event(Button::Next, true, t, &dialog()), vec![key(KeyCode::Right)]);
        assert!(m.event(Button::Next, false, t, &dialog()).is_empty());
        m.event(Button::Back, true, t, &dialog());
        assert_eq!(m.event(Button::Back, false, t, &dialog()), vec![key(KeyCode::Esc)]);
        m.event(Button::Menu, true, t, &dialog());
        assert_eq!(m.event(Button::Menu, false, t, &dialog()), vec![key(KeyCode::Enter)]);
    }

    #[test]
    fn menu_cycles_sections() {
        let mut m = Mapper::default();
        let t = Instant::now();
        for (from, to) in [(Section::Songs, 'a'), (Section::Albums, 'r'), (Section::Artists, 's')] {
            let c = Ctx { section: from, ..ctx() };
            m.event(Button::Menu, true, t, &c);
            assert_eq!(m.event(Button::Menu, false, t, &c), vec![key(KeyCode::Char(to))]);
        }
    }

    #[test]
    fn power_short_toggles_screen_and_long_powers_off() {
        let mut m = Mapper::default();
        let t = Instant::now();
        m.event(Button::Power, true, t, &ctx());
        assert_eq!(m.event(Button::Power, false, t, &ctx()), vec![Action::ScreenToggle]);
        m.event(Button::Power, true, t, &ctx());
        assert!(m.tick(t + Duration::from_millis(1500), &ctx()).is_empty());
        assert_eq!(m.tick(t + Duration::from_millis(2100), &ctx()), vec![Action::PowerOff]);
        assert!(m.event(Button::Power, false, t + Duration::from_secs(3), &ctx()).is_empty());
    }

    #[test]
    fn volume_repeats_while_held() {
        let mut m = Mapper::default();
        let t = Instant::now();
        assert_eq!(m.event(Button::VolUp, true, t, &ctx()), vec![Action::VolUp]);
        assert!(m.next_wake(t).unwrap() <= Duration::from_millis(600));
        let mut n = 0;
        let mut now = t;
        while now < t + Duration::from_millis(1000) {
            n += m.tick(now, &ctx()).len();
            now += Duration::from_millis(20);
        }
        assert!((2..=5).contains(&n), "{n} repeats");
        assert!(m.event(Button::VolUp, false, now, &ctx()).is_empty());
        assert!(m.next_wake(now).is_none());
    }

    #[test]
    fn menu_chords_toggle_shuffle_and_cancel_menu() {
        let mut m = Mapper::default();
        let t = Instant::now();
        assert!(m.event(Button::Menu, true, t, &ctx()).is_empty());
        let t2 = t + Duration::from_millis(200);
        assert_eq!(m.event(Button::Play, true, t2, &ctx()), vec![key(KeyCode::Char('z'))]);
        assert!(m.tick(t + Duration::from_secs(1), &ctx()).is_empty(), "no sort dialog after a chord");
        assert!(m.event(Button::Play, false, t + Duration::from_secs(1), &ctx()).is_empty());
        assert!(m.event(Button::Menu, false, t + Duration::from_secs(1), &ctx()).is_empty());
        m.event(Button::Menu, true, t, &ctx());
        assert_eq!(m.event(Button::Back, true, t2, &ctx()), vec![key(KeyCode::Char('l'))]);
        m.event(Button::Back, false, t2, &ctx());
        m.event(Button::Menu, false, t2, &ctx());
        assert!(m.next_wake(t2).is_none());
    }

    #[test]
    fn kernel_codes_map_to_buttons() {
        assert_eq!(button_for(164, false), Some(Button::Play));
        assert_eq!(button_for(163, false), Some(Button::Prev));
        assert_eq!(button_for(163, true), Some(Button::Next));
        assert_eq!(button_for(105, false), Some(Button::WheelBack));
        assert_eq!(button_for(1, false), None);
    }
}
