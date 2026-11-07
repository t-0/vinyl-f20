# vinyl for the Surfans F20

Firmware that turns a Surfans F20 into a [vinyl](external/vinyl) player: the
same terminal music player, drawn on the F20's 320x240 screen and driven by
its wheel and buttons, with vinyl's tape/vinyl effects, equaliser and
loudness.

It is "firmware on an SD card": nothing is flashed except, once, the Rockbox
hosted bootloader, which is a patched copy of the stock firmware. After that
the whole player lives in two folders on the card and can be updated, or
removed, by copying files.

**Status: runs on an F20 (hardware revision with the DAC volume controls):
screen, wheel, buttons and playback work. Audio quality and battery life are
still being tuned.** See *First boot* for how to test a new build in stages.

## How it works

The F20 (a rebadged AIGO Eros K) is an Ingenic X1000 MIPS32 system with
32 MiB of RAM, an ES9018K2M DAC and a 320x240 panel, running HiBy's Linux
(kernel 3.10). Rockbox has a *hosted* port for it: its bootloader, installed
through the stock firmware updater, shows a boot menu and, for the "Rockbox"
entry, mounts the SD card at `/mnt/sd_0`, copies `/.rockbox/rockbox.erosq`
to `/tmp` and executes it. It does not check what that file is.

So `rockbox.erosq` here is vinyl, built for `mipsel-unknown-linux-musl` as
one static binary. `external/vinyl` is used untouched (it is a submodule and
is not modified by this repository); `src/main.rs` compiles its modules by
path and `src/f20` adds what a pocket player needs instead of a terminal:

| Module | Replaces | With |
| --- | --- | --- |
| `fb.rs`, `font.rs`, `fbdev.rs` | the terminal | a ratatui backend drawing the Spleen bitmap font into `/dev/fb0` |
| `trace.rs` | | fatal-signal handler that writes the failing step to `boot.log` |
| `input.rs`, `keys.rs`, `platform.rs` | the keyboard | evdev buttons and wheel, short/long press, repeat |
| `hw.rs` | nothing | DAC volume, headphone/line-out switch, battery, backlight, power off |
| `run.rs` | vinyl's `main.rs` loop | the same loop and key handlers, plus a battery/volume overlay |
| `device.rs` | | SD card discovery, `$HOME` on the card, logs, diagnostics |

Audio is vinyl's own path (rodio → cpal → alsa-lib) against a static
alsa-lib cross-built here; `.asoundrc` on the card routes `default` through
a software volume stage to `hw:0,0`.

## Building

Prerequisites on macOS or Linux:

```sh
rustup toolchain install nightly --component rust-src   # tier-3 target: std is built from source
brew install zig pkg-config                              # zig is the C compiler, musl and linker
```

Then:

```sh
tools/build-alsa.sh     # once: static alsa-lib for MIPS into build/alsa
tools/build.sh          # the firmware: build/rockbox.erosq, and the card as build/sdcard
```

`build/sdcard` is the SD card: copy its contents (including the two dot
folders) to the root of a card and drop music into `Music/`.

On the desktop the same crate builds natively and renders the player's
screen, using your own `~/.vinyl` config and music, so layout can be checked
without the device:

```sh
cargo test
cargo run -- --preview screen.png --play            # 6x12 font, 53x20 cells
cargo run -- --preview eq.png --dialog eq --font 8x16
```

## Installing on the player

1. **Rockbox hosted bootloader (once).** Follow the Rockbox wiki page for the
   AIGO Eros Q/K (the F20 is listed there) to get the *hosted* bootloader as
   a patched `update.upt` for your firmware version (a Rockbox forum thread
   names `F20-v27-patched.upt` for stock 2.7). Copy it to the card root as
   `update.upt`, then on the player: System settings → Firmware update. Keep
   a copy of your stock `update.upt` (ask help@surfans.net) for going back.
   The *native* Rockbox port (`erosqnative`, which replaces the kernel) is a
   different thing and does not run this.
2. **Card.** Copy the contents of `build/sdcard` to the card root, or write
   it there directly with `tools/make-sd.sh /Volumes/F20`. That is
   `.rockbox/rockbox.erosq`, `.vinyl/config` (music.home is `Music/` on the
   card), `.vinyl/f20.conf`, `.vinyl/.asoundrc`, `.vinyl/alsa/` and an empty
   `Music/`. Put music in `Music/`.
3. **Boot.** The bootloader menu offers Rockbox, Tools and the original
   firmware; pick Rockbox. It remembers the choice and boots it after 1 s
   next time (10 s after a change), so a wheel click during that second gets
   the menu back. "Original firmware" is always there: nothing here can lock
   you out.

## First boot

Stage it. Create a file `.vinyl/mode` on the card containing one word; it is
consumed on the next boot:

| `mode` | What happens |
| --- | --- |
| `fbtest` | Colour bars and a font sample for 6 s, then back to the bootloader script (which reboots). Proves the framebuffer path. |
| `diag` | Writes `.vinyl/diag.txt`: kernel, memory, input devices, framebuffer mode, backlight, battery, jack switches and every ALSA mixer control with its value. |
| `audiotest` | Decodes the first file of each type under `Music/` on the player and writes `.vinyl/audiotest.txt`: channel levels, mid/side energy and left/right correlation, before and after the effects chain. On the desktop, `cargo run -- --audiotest [files]` does the same for comparison. |

Then boot normally. Logs land on the card: `.vinyl/boot.log` (this layer:
each start, what it found, panics, exit reason), `.vinyl/.vinyl.log`
(anything the audio stack prints), `.vinyl/.running` left behind means the
last run did not exit cleanly.

If the screen stays black after the bootloader, read `boot.log` and
`diag.txt`, and compare the framebuffer mode and mixer control names with
what `src/f20/fbdev.rs` and `src/f20/hw.rs` expect (taken from Rockbox's
hosted port). Those are the two places a different hardware revision would
need a tweak.

## Buttons

| Button | Short press | Held |
| --- | --- | --- |
| Wheel | move selection | |
| Play | play the selection; confirm in dialogs | play / pause |
| Prev / Next | previous / next track (in dialogs: adjust the value) | rewind / fast-forward 5 s steps |
| Menu | next list: songs → albums → artists | sort dialog |
| Back | FX dialog (in dialogs: close) | equaliser dialog |
| Vol + / − | volume (DAC attenuator on hw2+, software on hw1) | keeps changing |
| Power | screen off / on | 2 s: power off |
| Menu + Play | shuffle on / off | |
| Menu + Back | loudness on / off | |
| Menu + Prev / Next | playback speed −1% / +1% (Menu + Vol resets it) | |
| Menu + wheel | step the DAC output port selector 0–5 (diagnostic; `port =` in `f20.conf` pins it) | |

The backlight goes off after `screen_timeout` seconds without a press; the
first press wakes it and does nothing else. The footer still shows vinyl's
keyboard hints; a 53-column screen only has room for the first few.

Not reachable from the buttons yet: search (needs a keyboard) and reindex.

## Settings

`.vinyl/config` is vinyl's own file (see its README). `.vinyl/f20.conf`:

```
font = 6x12            # 5x8 (64x30 cells), 6x12 (53x20), 8x16 (40x15)
swap_prev_next = false # if the kernel reports Prev/Next the other way round
screen_timeout = 30    # seconds without a press before the backlight goes off; 0 never
volume = 50            # starting volume until one is saved (.vinyl/volume)
```

## Layout

```
Cargo.toml, .cargo/config.toml   crate; cross target config (zig as linker)
src/main.rs                      vinyl's modules by path + mod f20
src/f20/                         the F20 layer (see table above)
external/vinyl/                  the player, as a submodule, unmodified
third_party/spleen/              bitmap font (BSD-2)
sd/.vinyl/                       templates copied to the card
tools/build-alsa.sh              static alsa-lib for MIPS
tools/build.sh                   nightly + build-std cross build
tools/make-sd.sh                 lay the card out (build/sdcard, or a mounted card)
tools/zig-cc, tools/zig-ar       the C toolchain wrappers
```

## Known gaps

- Use a FAT32 card. With an exFAT card the player's old kernel driver and
  macOS disagreed about directory contents and files written on the player
  vanished.
- The build is hard-float through a custom target spec (`tools/mipsel-f20.json`);
  Rust's built-in MIPS musl target is soft-float, which could not keep the
  audio buffer fed on the player.
- No Bluetooth, no USB mass storage mode from inside the player (use the
  original firmware for that), no on-screen keyboard.
- 32 MiB of RAM: large libraries (tens of thousands of tracks) are untested.
