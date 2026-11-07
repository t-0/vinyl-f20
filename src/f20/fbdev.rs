//! The Linux framebuffer (`/dev/fb0`) on the player.
//!
//! The HiBy kernel exposes the 320x240 panel as a framebuffer that is pushed
//! to the LCD controller on `FBIOPAN_DISPLAY`, so every frame ends with that
//! ioctl (Rockbox's hosted port does the same). Pixel format is taken from
//! the driver (Rockbox's HiBy config uses 32 bpp; 16 bpp is handled too).
use super::fb::Present;
use std::ffi::CString;
use std::io;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Bitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct VarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: Bitfield,
    green: Bitfield,
    blue: Bitfield,
    transp: Bitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct FixScreeninfo {
    id: [u8; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    type_: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: libc::c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

const FBIOGET_VSCREENINFO: u32 = 0x4600;
const FBIOPUT_VSCREENINFO: u32 = 0x4601;
const FBIOGET_FSCREENINFO: u32 = 0x4602;
const FBIOPAN_DISPLAY: u32 = 0x4606;

pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 240;

pub struct LinuxFb {
    fd: libc::c_int,
    map: *mut u8,
    map_len: usize,
    var: VarScreeninfo,
    fix: FixScreeninfo,
}

unsafe impl Send for LinuxFb {}

fn ioctl<T>(fd: libc::c_int, req: u32, arg: &mut T) -> io::Result<()> {
    // SAFETY: `arg` is a properly sized kernel struct for this request.
    let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl LinuxFb {
    pub fn open(path: &str) -> io::Result<Self> {
        let c = CString::new(path).map_err(|_| io::Error::other("bad path"))?;
        // SAFETY: plain open(2) on a C string.
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut var = VarScreeninfo::default();
        // SAFETY: all-zero is a valid FixScreeninfo.
        let mut fix: FixScreeninfo = unsafe { std::mem::zeroed() };
        if let Err(e) = ioctl(fd, FBIOGET_VSCREENINFO, &mut var)
            .and_then(|_| ioctl(fd, FBIOGET_FSCREENINFO, &mut fix))
        {
            unsafe { libc::close(fd) };
            return Err(e);
        }
        if var.xres != WIDTH || var.yres != HEIGHT || !matches!(var.bits_per_pixel, 16 | 32) {
            // Ask for the panel's native mode, as Rockbox does; keep a usable
            // depth if the driver has one, otherwise try 32 then 16.
            let mut want = var;
            want.xres = WIDTH;
            want.yres = HEIGHT;
            want.xres_virtual = WIDTH;
            want.yres_virtual = want.yres_virtual.max(HEIGHT);
            if !matches!(want.bits_per_pixel, 16 | 32) {
                want.bits_per_pixel = 32;
            }
            if ioctl(fd, FBIOPUT_VSCREENINFO, &mut want).is_err() {
                want.bits_per_pixel = 16;
                let _ = ioctl(fd, FBIOPUT_VSCREENINFO, &mut want);
            }
            let _ = ioctl(fd, FBIOGET_VSCREENINFO, &mut var);
            let _ = ioctl(fd, FBIOGET_FSCREENINFO, &mut fix);
        }
        if !matches!(var.bits_per_pixel, 16 | 32) {
            unsafe { libc::close(fd) };
            return Err(io::Error::other(format!(
                "unsupported framebuffer depth {} bpp",
                var.bits_per_pixel
            )));
        }
        let map_len = fix.smem_len as usize;
        // SAFETY: mapping the framebuffer memory the driver advertises.
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if map == libc::MAP_FAILED {
            let e = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        Ok(Self {
            fd,
            map: map as *mut u8,
            map_len,
            var,
            fix,
        })
    }

    pub fn width(&self) -> usize {
        self.var.xres as usize
    }

    pub fn height(&self) -> usize {
        self.var.yres as usize
    }

    pub fn describe(&self) -> String {
        let id = String::from_utf8_lossy(&self.fix.id)
            .trim_end_matches('\0')
            .to_string();
        format!(
            "fb '{}': {}x{} (virtual {}x{}) {} bpp, line {} bytes, mem {} bytes, r/g/b offsets {}/{}/{} lengths {}/{}/{}",
            id,
            self.var.xres,
            self.var.yres,
            self.var.xres_virtual,
            self.var.yres_virtual,
            self.var.bits_per_pixel,
            self.fix.line_length,
            self.fix.smem_len,
            self.var.red.offset,
            self.var.green.offset,
            self.var.blue.offset,
            self.var.red.length,
            self.var.green.length,
            self.var.blue.length
        )
    }

    fn channel(bf: &Bitfield, v: u8) -> u32 {
        let len = bf.length.min(8);
        ((v as u32) >> (8 - len)) << bf.offset
    }

    fn encode(&self, p: u32) -> u32 {
        let (r, g, b) = ((p >> 16) as u8, (p >> 8) as u8, p as u8);
        Self::channel(&self.var.red, r) | Self::channel(&self.var.green, g) | Self::channel(&self.var.blue, b)
    }
}

impl Present for LinuxFb {
    fn present(&mut self, pixels: &[u32], width: usize, height: usize) -> io::Result<()> {
        let rows = height.min(self.var.yres as usize);
        let cols = width.min(self.var.xres as usize);
        let stride = self.fix.line_length as usize;
        let bpp = self.var.bits_per_pixel as usize / 8;
        if stride * rows > self.map_len || cols * bpp > stride {
            return Err(io::Error::other("framebuffer smaller than the frame"));
        }
        for y in 0..rows {
            let src = &pixels[y * width..y * width + cols];
            // SAFETY: the bounds above keep every write inside the mapping.
            let dst = unsafe { self.map.add(y * stride) };
            match bpp {
                2 => {
                    for (x, &p) in src.iter().enumerate() {
                        let v = self.encode(p) as u16;
                        unsafe { (dst as *mut u16).add(x).write_unaligned(v) };
                    }
                }
                _ => {
                    for (x, &p) in src.iter().enumerate() {
                        let v = self.encode(p);
                        unsafe { (dst as *mut u32).add(x).write_unaligned(v) };
                    }
                }
            }
        }
        let mut var = self.var;
        var.xoffset = 0;
        var.yoffset = 0;
        // Pushes the frame to the panel; a driver without panning just says so.
        let _ = ioctl(self.fd, FBIOPAN_DISPLAY, &mut var);
        Ok(())
    }
}

impl Drop for LinuxFb {
    fn drop(&mut self) {
        // SAFETY: undoing the mmap/open from `open`.
        unsafe {
            libc::munmap(self.map as *mut libc::c_void, self.map_len);
            libc::close(self.fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_struct_sizes_match_the_32bit_abi() {
        assert_eq!(std::mem::size_of::<VarScreeninfo>(), 160);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(std::mem::size_of::<FixScreeninfo>(), 68);
    }
}
