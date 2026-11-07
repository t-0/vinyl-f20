//! vinyl on the Surfans F20.
//!
//! The player itself is the `vinyl` terminal app in `external/vinyl`, used
//! untouched: its modules are compiled straight out of the submodule by path.
//! `f20` supplies what a pocket player needs instead of a terminal: a
//! framebuffer backend for ratatui, the buttons and scroll wheel, hardware
//! volume, battery and power. On a desktop the same binary renders previews
//! of the 320x240 screen so the layout can be checked without the device.
#![allow(dead_code)]

#[path = "../external/vinyl/src/app.rs"]
mod app;
#[path = "../external/vinyl/src/audio.rs"]
mod audio;
#[path = "../external/vinyl/src/config.rs"]
mod config;
#[path = "../external/vinyl/src/fx.rs"]
mod fx;
#[path = "../external/vinyl/src/index.rs"]
mod index;
#[path = "../external/vinyl/src/spectrum.rs"]
mod spectrum;
#[path = "../external/vinyl/src/stderr.rs"]
mod stderr;
#[path = "../external/vinyl/src/ui.rs"]
mod ui;

mod f20;

fn main() -> anyhow::Result<()> {
    f20::main()
}
