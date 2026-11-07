//! Decodes a file and measures how the two channels relate, with and without
//! vinyl's effects chain. Written to find out where "karaoke" sound came
//! from on the player: centre-panned sound cancels when left and right are
//! in opposite phase, so mid energy far below side energy is the fingerprint.
use crate::audio::open_decoder;
use crate::fx::{Fx, FxParams};
use anyhow::Result;
use rodio::Source;
use std::fmt::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Stats {
    pub channels: u16,
    pub rate: u32,
    pub frames: usize,
    pub rms_l: f64,
    pub rms_r: f64,
    pub rms_mid: f64,
    pub rms_side: f64,
    pub corr: f64,
}

const SECONDS: usize = 8;

fn stats<I: Iterator<Item = f32>>(it: I, channels: u16, rate: u32) -> Stats {
    let ch = channels.max(1) as usize;
    let (mut sl, mut sr, mut sm, mut ss, mut slr) = (0f64, 0f64, 0f64, 0f64, 0f64);
    let mut frames = 0usize;
    let mut frame = Vec::with_capacity(ch);
    let limit = rate as usize * SECONDS;
    for s in it {
        frame.push(s as f64);
        if frame.len() == ch {
            let l = frame[0];
            let r = if ch > 1 { frame[1] } else { l };
            sl += l * l;
            sr += r * r;
            sm += (l + r) * (l + r) / 4.0;
            ss += (l - r) * (l - r) / 4.0;
            slr += l * r;
            frames += 1;
            frame.clear();
            if frames >= limit {
                break;
            }
        }
    }
    let n = frames.max(1) as f64;
    let rms = |x: f64| (x / n).sqrt();
    let corr = if sl > 0.0 && sr > 0.0 { slr / (sl * sr).sqrt() } else { 0.0 };
    Stats {
        channels,
        rate,
        frames,
        rms_l: rms(sl),
        rms_r: rms(sr),
        rms_mid: rms(sm),
        rms_side: rms(ss),
        corr,
    }
}

pub fn measure(path: &Path, fx: Option<FxParams>) -> Result<Stats> {
    let src = open_decoder(path)?;
    let (channels, rate) = (src.channels(), src.sample_rate());
    Ok(match fx {
        None => stats(src, channels.get(), rate.get()),
        Some(p) => {
            let fx = Fx::new(src, Arc::new(Mutex::new(p)), None);
            let (c, r) = (fx.channels(), fx.sample_rate());
            stats(fx, c.get(), r.get())
        }
    })
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-9).log10()
}

impl Stats {
    pub fn line(&self) -> String {
        format!(
            "{}ch {}Hz {} frames: L {:.1} dB, R {:.1} dB, mid {:.1} dB, side {:.1} dB, L/R correlation {:+.3}",
            self.channels,
            self.rate,
            self.frames,
            db(self.rms_l),
            db(self.rms_r),
            db(self.rms_mid),
            db(self.rms_side),
            self.corr
        )
    }

    /// One word for the panel: normal stereo has mid well above side and a
    /// positive correlation; anti-phase channels have the opposite.
    pub fn verdict(&self) -> &'static str {
        if self.frames == 0 {
            "no audio"
        } else if self.corr < -0.3 {
            "ANTI-PHASE"
        } else if self.corr > 0.3 {
            "normal"
        } else {
            "uncorrelated"
        }
    }
}

/// Full report for one file: decoder output, then the effects chain output.
pub fn report(path: &Path, fx: FxParams) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "== {}", path.display());
    match measure(path, None) {
        Ok(st) => {
            let _ = writeln!(s, "decoder: {}  [{}]", st.line(), st.verdict());
        }
        Err(e) => {
            let _ = writeln!(s, "decoder: error {e:#}");
        }
    }
    match measure(path, Some(fx)) {
        Ok(st) => {
            let _ = writeln!(s, "fx:      {}  [{}]", st.line(), st.verdict());
        }
        Err(e) => {
            let _ = writeln!(s, "fx:      error {e:#}");
        }
    }
    let _ = writeln!(s, "fx params: {fx:?}");
    s
}

/// The first file of each common type under `root`, for a quick survey.
pub fn sample_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found: Vec<(String, std::path::PathBuf)> = Vec::new();
    let wanted = ["mp3", "m4a", "flac", "wav", "ogg", "opus", "aiff"];
    for e in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .flatten()
    {
        let p = e.path();
        let Some(ext) = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase()) else {
            continue;
        };
        if wanted.contains(&ext.as_str()) && !found.iter().any(|(x, _)| *x == ext) {
            found.push((ext, p.to_path_buf()));
            if found.len() == wanted.len() {
                break;
            }
        }
    }
    found.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_phase_and_anti_phase_are_told_apart() {
        let tone: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.05).sin()).collect();
        let inphase = tone.iter().flat_map(|&s| [s, s]);
        let st = stats(inphase, 2, 44100);
        assert_eq!(st.verdict(), "normal");
        assert!(st.rms_mid > st.rms_side * 100.0);
        let anti = tone.iter().flat_map(|&s| [s, -s]);
        let st = stats(anti, 2, 44100);
        assert_eq!(st.verdict(), "ANTI-PHASE");
        assert!(st.rms_side > st.rms_mid * 100.0);
    }
}
