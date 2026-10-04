//! F005 串口日志 — 16550 UART COM1 output, the QEMU debug channel
//! (`-serial stdio` / `-serial file:` captures this output).
//!
//! The port-I/O layer is compiled only for the kernel target; the divisor
//! configuration math is pure and unit-tested on the host.

/// COM1 I/O port base.
pub const COM1: u16 = 0x3F8;

/// 16550 register offsets from the port base (target I/O only).
#[cfg(target_os = "none")]
mod reg {
    pub const DATA: u16 = 0;
    pub const IER: u16 = 1;
    pub const FCR: u16 = 2;
    pub const LCR: u16 = 3;
    pub const MCR: u16 = 4;
    pub const LSR: u16 = 5;
}

/// LSR bit: transmitter holding register empty (ready to write).
#[cfg(target_os = "none")]
const LSR_THRE: u8 = 0x20;

/// Compute the divisor latch for a target baud rate.
///
/// The 16550 input clock is 1.8432 MHz; baud = 115200 / divisor.
/// Returns `None` for rates the classic clock cannot express.
pub fn divisor_for(baud: u32) -> Option<u16> {
    if baud == 0 || baud > 115200 {
        return None;
    }
    let div = (115200 + baud / 2) / baud;
    if div == 0 || div > 0xFFFF {
        None
    } else {
        Some(div as u16)
    }
}

/// Pure description of the line setup (8 data bits, no parity, 1 stop bit).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineConfig {
    pub divisor: u16,
}

impl LineConfig {
    pub const fn default_115200() -> LineConfig {
        LineConfig { divisor: 1 }
    }

    /// LCR value: 8N1 + DLAB set for the divisor write pass.
    pub const fn lcr_dlab(self) -> u8 {
        0x83
    }

    /// LCR value: 8N1, DLAB clear (normal operation).
    pub const fn lcr_normal(self) -> u8 {
        0x03
    }

    /// FCR value: enable + clear FIFOs, 14-byte RX trigger.
    pub const fn fcr(self) -> u8 {
        0xC7
    }

    /// MCR value: DTR | RTS (data-terminal + request-to-send).
    pub const fn mcr(self) -> u8 {
        0x03
    }
}

#[cfg(target_os = "none")]
mod io {
    use super::*;

    /// Port write — raw `out dx, al`. `core::arch::x86_64` does not export
    /// `outb` for the `none` target, so the port I/O is expressed directly.
    #[inline]
    pub unsafe fn out(port: u16, val: u8) {
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") val,
                options(nomem, nostack, preserves_flags)
            )
        }
    }

    /// Port read — raw `in al, dx`.
    #[inline]
    pub unsafe fn inp(port: u16) -> u8 {
        let val: u8;
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") val,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            )
        }
        val
    }

    static mut INITIALIZED: bool = false;

    /// Program COM1 for 115200-8N1 with FIFOs. Safe to call once at boot.
    pub fn init() {
        let cfg = LineConfig::default_115200();
        unsafe {
            out(COM1 + reg::IER, 0x00); // disable interrupts
            out(COM1 + reg::LCR, cfg.lcr_dlab());
            out(COM1 + reg::DATA, (cfg.divisor & 0xFF) as u8); // divisor low
            out(COM1 + reg::IER, (cfg.divisor >> 8) as u8); // divisor high
            out(COM1 + reg::LCR, cfg.lcr_normal()); // 8N1
            out(COM1 + reg::FCR, cfg.fcr());
            out(COM1 + reg::MCR, cfg.mcr());
        }
        unsafe { INITIALIZED = true };
    }

    /// Blocking single-byte write, polls LSR.THR empty first.
    pub fn write_byte(b: u8) {
        unsafe {
            if !INITIALIZED {
                return;
            }
            let mut guard = 0;
            while (inp(COM1 + reg::LSR) & LSR_THRE) == 0 {
                guard += 1;
                if guard > 1_000_000 {
                    return; // hardware absent — drop instead of hanging boot
                }
            }
            out(COM1 + reg::DATA, b);
        }
    }

    pub fn write_bytes(bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' {
                write_byte(b'\r'); // terminal-friendly line endings
            }
            write_byte(b);
        }
    }
}

#[cfg(target_os = "none")]
pub use io::{init, write_byte, write_bytes};

#[cfg(target_os = "none")]
pub use io::init as target_init;

#[cfg(not(target_os = "none"))]
mod io {
    /// Host build: port I/O does not exist; these exist so the boot sequence
    /// source stays identical under `cfg`.
    pub fn init() {}
    pub fn write_byte(_b: u8) {}
    pub fn write_bytes(_b: &[u8]) {}
}

#[cfg(not(target_os = "none"))]
pub use io::{init, write_byte, write_bytes};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisor_math() {
        assert_eq!(divisor_for(115200), Some(1));
        assert_eq!(divisor_for(57600), Some(2));
        assert_eq!(divisor_for(38400), Some(3));
        assert_eq!(divisor_for(9600), Some(12));
        assert_eq!(divisor_for(0), None);
        assert_eq!(divisor_for(115201), None);
        assert_eq!(divisor_for(230400), None);
    }

    #[test]
    fn line_config_values() {
        let c = LineConfig::default_115200();
        assert_eq!(c.lcr_dlab(), 0x83);
        assert_eq!(c.lcr_normal(), 0x03);
        assert_eq!(c.fcr(), 0xC7);
        assert_eq!(c.mcr(), 0x03);
        assert_eq!(c.divisor, 1);
    }
}
