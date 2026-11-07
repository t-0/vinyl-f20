//! Everything the Surfans F20 needs around vinyl. See `src/main.rs`.
pub mod audiotest;
pub mod fb;
pub mod font;
pub mod keys;
pub mod png;
pub mod run;
pub mod settings;

#[cfg(target_os = "linux")]
pub mod device;
#[cfg(target_os = "linux")]
pub mod fbdev;
#[cfg(target_os = "linux")]
pub mod hw;
#[cfg(target_os = "linux")]
pub mod input;
#[cfg(target_os = "linux")]
pub mod platform;
#[cfg(target_os = "linux")]
pub mod trace;
#[cfg(not(target_os = "linux"))]
pub mod preview;

pub fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(target_os = "linux")]
    {
        device::main(args)
    }
    #[cfg(not(target_os = "linux"))]
    {
        preview::main(args)
    }
}
