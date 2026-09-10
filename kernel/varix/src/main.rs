//! Varix kernel image entry point (F001~F025 boot sequence).
//!
//! Limine (base revision 1) calls `_start` as a plain C function on the BSP
//! with a valid stack; every request response is already filled in. The boot
//! sequence below brackets each stage into the timeline (F024), advances the
//! honest progress bar (F014) and finishes with the self-test (F025).

#![no_std]
#![no_main]

use varix::timeline::{Stage, TIMELINE};

extern crate varix;

#[no_mangle]
extern "C" fn _start() -> ! {
    boot();
}

/// Number of timeline stages = progress bar total (F014 truth mapping).
const TOTAL_STAGES: usize = 14;

/// Scratch buffer for the "bootloader name + version" banner line. Kept as a
/// `static` so the banner can borrow a `&'static str` out of it.
const ZERO_U8: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static BL_LINE: [core::sync::atomic::AtomicU8; 64] = [ZERO_U8; 64];

fn boot() -> ! {
    TIMELINE.begin_boot(varix::timeline::read_tsc());

    // --- serial (F005/F006) ------------------------------------------------
    TIMELINE.stage_begin(Stage::Serial, varix::timeline::read_tsc());
    varix::logger::early_init();
    TIMELINE.stage_end(Stage::Serial, varix::timeline::read_tsc());

    // --- cmdline (F017) ------------------------------------------------------
    TIMELINE.stage_begin(Stage::Cmdline, varix::timeline::read_tsc());
    let cmdline = varix::cmdline::init();
    varix::logger::apply_cmdline(cmdline.source());
    TIMELINE.stage_end(Stage::Cmdline, varix::timeline::read_tsc());
    varix::kinfo!("cmdline: {}", cmdline.source());

    // --- framebuffer (F003/F004) --------------------------------------------
    TIMELINE.stage_begin(Stage::Framebuffer, varix::timeline::read_tsc());
    let surface = varix::limine::framebuffer().and_then(|fb| {
        varix::fb::Surface::from_limine(fb).ok()
    });
    // Backdrop belongs to framebuffer bring-up: the logo blends into it, so it
    // must exist before the mark is rasterized.
    if let Some(s) = surface.as_ref() {
        varix::banner::paint_backdrop(s);
    }
    TIMELINE.stage_end(Stage::Framebuffer, varix::timeline::read_tsc());
    let surface = match surface {
        Some(s) => s,
        None => {
            varix::kerror!("no usable framebuffer — serial-only boot");
            finish_no_fb();
        }
    };

    // --- logo (F023) ----------------------------------------------------------
    TIMELINE.stage_begin(Stage::Logo, varix::timeline::read_tsc());
    let (lcx, lcy, lsize) = varix::logo::metrics_for(&surface);
    let logo_bottom = varix::logo::draw(&surface, lcx, lcy, lsize);
    TIMELINE.stage_end(Stage::Logo, varix::timeline::read_tsc());

    // --- banner (F013) --------------------------------------------------------
    TIMELINE.stage_begin(Stage::Banner, varix::timeline::read_tsc());
    let firmware = varix::limine::BootInfo::collect();
    let bl_line = {
        let (name, version) = varix::limine::bootloader_info().unwrap_or(("Limine", "?"));
        let mut m = 0usize;
        let mut push = |b: u8| {
            if m < BL_LINE.len() {
                BL_LINE[m].store(b, core::sync::atomic::Ordering::Relaxed);
                m += 1;
            }
        };
        for &b in name.as_bytes() {
            push(b);
        }
        push(b' ');
        for &b in version.as_bytes() {
            push(b);
        }
        // SAFETY: `BL_LINE` is a `static`, so the view is `'static`; it is
        // written once here on the single-threaded boot path.
        let view: &'static [u8] =
            unsafe { core::slice::from_raw_parts(BL_LINE.as_ptr().cast::<u8>(), m) };
        core::str::from_utf8(view).unwrap_or("Limine")
    };
    let hud_y = varix::banner::draw_boot_banner_at(
        &surface,
        firmware.firmware.as_str(),
        bl_line,
        logo_bottom + 12,
    );
    TIMELINE.stage_end(Stage::Banner, varix::timeline::read_tsc());

    // --- console (F011/F012) -------------------------------------------------
    TIMELINE.stage_begin(Stage::Console, varix::timeline::read_tsc());
    varix::console::init(surface);
    varix::console::enable_mirror();
    TIMELINE.stage_end(Stage::Console, varix::timeline::read_tsc());

    // --- platform (F019/F020) ------------------------------------------------
    TIMELINE.stage_begin(Stage::Platform, varix::timeline::read_tsc());
    let platform = varix::platform::detect();
    TIMELINE.stage_end(Stage::Platform, varix::timeline::read_tsc());
    varix::kinfo!(
        "cpu: {} fam{} mod{} step{} @{}MHz",
        platform.vendor.as_str(),
        platform.signature.family,
        platform.signature.model,
        platform.signature.stepping,
        platform.tsc_hz / 1_000_000
    );

    // --- acpi (F008/F009) ------------------------------------------------------
    TIMELINE.stage_begin(Stage::Acpi, varix::timeline::read_tsc());
    let acpi = varix::acpi::init();
    TIMELINE.stage_end(Stage::Acpi, varix::timeline::read_tsc());
    match acpi {
        Some(state) => varix::kinfo!(
            "acpi: rev {} tables {} cpus {} ioapics {}",
            state.revision,
            state.table_count,
            state.enabled_cpus,
            state.ioapics
        ),
        None => varix::kwarn!("acpi: RSDP not found"),
    }

    // --- smbios (F010) ---------------------------------------------------------
    TIMELINE.stage_begin(Stage::Smios, varix::timeline::read_tsc());
    let smbios = varix::smbios::init();
    TIMELINE.stage_end(Stage::Smios, varix::timeline::read_tsc());
    if let Some(info) = smbios {
        varix::kinfo!(
            "smbios: {} {} / {} {}",
            info.manufacturer_str(),
            info.product_str(),
            info.bios_vendor_str(),
            info.bios_version_str()
        );
    }

    // --- memmap (F015/F016) ------------------------------------------------------
    TIMELINE.stage_begin(Stage::Memmap, varix::timeline::read_tsc());
    match varix::memmap::init() {
        Some(state) => {
            varix::kinfo!(
                "memmap: {} entries, {} usable",
                state.entry_count,
                state.usable_bytes
            );
            varix::memmap::register_boot_reservations();
        }
        None => varix::kwarn!("memmap: no memory map from bootloader"),
    }
    TIMELINE.stage_end(Stage::Memmap, varix::timeline::read_tsc());

    // --- kaslr (F018) -------------------------------------------------------------
    TIMELINE.stage_begin(Stage::Kaslr, varix::timeline::read_tsc());
    varix::kaslr::init();
    TIMELINE.stage_end(Stage::Kaslr, varix::timeline::read_tsc());
    let (pool, seeded) = varix::kaslr::pool_state();
    varix::kinfo!("kaslr: seeded={} pool={:#x}", seeded, pool);

    // --- integrity (F021) ------------------------------------------------------------
    TIMELINE.stage_begin(Stage::Integrity, varix::timeline::read_tsc());
    let chain = varix::integrity::init();
    TIMELINE.stage_end(Stage::Integrity, varix::timeline::read_tsc());
    let prefix = core::str::from_utf8(&chain.digest_prefix).unwrap_or("--------");
    varix::kinfo!(
        "integrity: {} image={}B sha256={}",
        chain.verdict.as_str(),
        chain.image_size,
        prefix
    );

    // --- bootopt (F022) ---------------------------------------------------------------
    TIMELINE.stage_begin(Stage::BootOpt, varix::timeline::read_tsc());
    let opts = varix::bootopt::init();
    TIMELINE.stage_end(Stage::BootOpt, varix::timeline::read_tsc());
    varix::kinfo!(
        "bootopt: default={} timeout={}s",
        opts.default_entry,
        opts.timeout_secs
    );

    // --- selftest (F025) — runs its own stage bracket -------------------------------
    TIMELINE.stage_begin(Stage::SelfTest, varix::timeline::read_tsc());
    let (_pass, _fail) = varix::selftest::run_boot_checks();
    TIMELINE.stage_end(Stage::SelfTest, varix::timeline::read_tsc());

    // --- HUD: progress bar + timeline + self-test (F014/F024/F025) -------------------
    draw_hud(&surface, hud_y);
    varix::selftest::render_to_console();
    varix::timeline::render_to_console();

    varix::kinfo!("boot complete — halting");
    halt()
}

/// HUD block under the banner: progress bar (F014) + boot message.
fn draw_hud(surf: &varix::fb::Surface, y: i64) {
    let w = surf.width() as i64;
    let inset = w / 12;
    let bar_w = w - inset * 2 - 56; // leave room for the % text
    let completed = TIMELINE.finished_stages();
    varix::progress::draw(surf, inset, y + 16, bar_w, completed, TOTAL_STAGES);
    // Boot message line under the bar.
    let msg = "BOOT SEQUENCE COMPLETE";
    let mw = varix::font::text_width_scaled(msg, 1);
    varix::font::draw_text_scaled(
        surf,
        (w - mw) / 2,
        y + 44,
        msg,
        varix::banner::TAGLINE,
        1,
    );
}

/// Serial-only tail when no framebuffer exists (degraded but honest boot).
fn finish_no_fb() -> ! {
    varix::selftest::run_boot_checks();
    varix::kerror!("boot degraded — no display");
    halt()
}

/// Park the CPU forever with interrupts disabled (`hlt` loop).
#[cfg(target_os = "none")]
fn halt() -> ! {
    // `cli` once, then park in `hlt` forever. Expressed as raw instructions:
    // the legacy `core::arch::x86_64::_hlt` wrappers are not available for the
    // `x86_64-unknown-none` target.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
        loop {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Host build (never executed — the bin is only linked for the kernel target).
#[cfg(not(target_os = "none"))]
fn halt() -> ! {
    loop {}
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Best-effort: emit to serial + ring, then park.
    if let Some(c) = varix::console::installed_ref() {
        c.set_colors(
            varix::console::Ink::White,
            varix::console::Ink::Red,
        );
        c.write_str("\nKERNEL PANIC\n");
        c.write_str(core::str::from_utf8(
            &format_panic(info),
        ).unwrap_or(""));
    }
    halt()
}

/// PanicInfo → bytes without fmt machinery on the stack-heavy path.
fn format_panic(info: &core::panic::PanicInfo<'_>) -> [u8; 256] {
    let mut out = [0u8; 256];
    let mut n = 0usize;
    let push = |bytes: &[u8], out: &mut [u8; 256], n: &mut usize| {
        for &b in bytes {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    push(info.location().map(|l| l.file()).unwrap_or("?").as_bytes(), &mut out, &mut n);
    push(b":", &mut out, &mut n);
    // Location line as decimal.
    if let Some(l) = info.location() {
        let mut num = [0u8; 10];
        let mut w = 0usize;
        let mut v = l.line();
        if v == 0 {
            num[0] = b'0';
            w = 1;
        } else {
            while v > 0 && w < num.len() {
                num[w] = b'0' + (v % 10) as u8;
                v /= 10;
                w += 1;
            }
        }
        while w > 0 {
            w -= 1;
            push(&[num[w]], &mut out, &mut n);
        }
    }
    push(b"\n", &mut out, &mut n);
    out
}
