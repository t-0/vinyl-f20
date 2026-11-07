//! `f20.conf`: the few settings that only exist on the player. vinyl's own
//! config parser rejects keys it does not know, so these live in their own
//! `key = value` file next to it.
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub font: String,
    pub swap_prev_next: bool,
    pub screen_timeout_secs: u64,
    pub volume: i32,
    /// Open the ALSA mixer for volume and jack routing (set false to bypass).
    pub mixer: bool,
    /// Pin the DAC "Output Port Switch" to this value instead of following
    /// the jack switches (None = automatic).
    pub port: Option<i32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font: super::font::DEFAULT.to_string(),
            swap_prev_next: false,
            screen_timeout_secs: 30,
            volume: 50,
            mixer: true,
            port: None,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "font" => s.font = v.to_string(),
                "swap_prev_next" => s.swap_prev_next = matches!(v, "true" | "yes" | "1"),
                "screen_timeout" => s.screen_timeout_secs = v.parse().unwrap_or(s.screen_timeout_secs),
                "volume" => s.volume = v.parse::<i32>().unwrap_or(s.volume).clamp(0, 100),
                "mixer" => s.mixer = !matches!(v, "false" | "no" | "0"),
                "port" => s.port = v.parse::<i32>().ok().filter(|p| (0..=5).contains(p)),
                _ => {}
            }
        }
        s
    }

    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_keys_and_ignores_the_rest() {
        let s = Settings::parse("# c\nfont = 8x16\nswap_prev_next = yes\nscreen_timeout=45\nvolume = 120\nbogus = 1\n");
        assert_eq!(s.font, "8x16");
        assert!(s.swap_prev_next);
        assert_eq!(s.screen_timeout_secs, 45);
        assert_eq!(s.volume, 100);
    }

    #[test]
    fn shipped_template_matches_defaults() {
        let s = Settings::parse(include_str!("../../sd/.vinyl/f20.conf"));
        assert_eq!(s, Settings::default());
    }
}
