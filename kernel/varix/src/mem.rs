//! AI-03 · 内存管理域（F051~F075）.
//!
//! Bring-up order: the page-frame allocators must be live before anything can
//! be mapped, the heap must be usable before any subsystem can allocate, and
//! the watermarks must be armed before the first reclaim decision. `init` walks
//! that order and reports the numbers it actually observed.

pub mod heap;
pub mod mm;
pub mod paging;
pub mod pmm;

use pmm::PAGE_SIZE;

/// Everything the rest of the kernel wants to know about memory.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemDomainState {
    /// F051/F052 — frames the buddy zone manages.
    pub managed_frames: u32,
    /// F052 — frames currently free.
    pub free_frames: u32,
    /// F070 — frames retired from service.
    pub quarantined: usize,
    /// F064 — watermarks armed from total RAM.
    pub watermarks: mm::Watermarks,
    /// F064 — level at the end of bring-up.
    pub water: mm::WaterLevel,
    /// F062 — swap backing configured.
    pub swap: mm::SwapBacking,
    /// F075 — (passed, failed) from the memory-chain self-test.
    pub self_test: (usize, usize),
}

impl MemDomainState {
    pub fn ok(&self) -> bool {
        self.managed_frames > 0 && self.self_test.1 == 0
    }

    pub fn free_bytes(&self) -> u64 {
        self.free_frames as u64 * PAGE_SIZE as u64
    }

    pub fn total_bytes(&self) -> u64 {
        self.managed_frames as u64 * PAGE_SIZE as u64
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("mem frames=");
        w.num(self.managed_frames as u64);
        w.str(" free=");
        w.num(self.free_frames as u64);
        w.str(" (");
        w.num(self.free_bytes() / (1024 * 1024));
        w.str("MiB) water=");
        w.str(match self.water {
            mm::WaterLevel::Critical => "CRITICAL",
            mm::WaterLevel::Low => "low",
            mm::WaterLevel::Normal => "normal",
            mm::WaterLevel::Comfortable => "ok",
        });
        w.str(" swap=");
        w.str(match self.swap {
            mm::SwapBacking::None => "off",
            mm::SwapBacking::File => "file",
            mm::SwapBacking::Partition => "part",
        });
        w.str(" bad=");
        w.num(self.quarantined as u64);
        w.str(" [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

/// F051~F075 bring-up.
///
/// The usable/reserved frame bitmaps come from AI-01's memory-map parser
/// (F015/F016); this domain turns them into working allocators.
pub fn init() -> MemDomainState {
    let mut st = MemDomainState::default();

    // 1. Translate the boot memory map into allocator inputs. The bitmaps
    //    live in static storage inside pmm: at 64 KiB apiece they must never
    //    touch the bootloader's small stack (boot hang, QEMU 2026-09-12).
    let total_frames = boot_bitmaps();

    // 2. F051 + F052 + F070 — the frame allocators.
    pmm::init();

    // 3. F053 + F054 + F055 — the slab heap and its leak journal.
    heap::init();

    // 4. F064 — arm the watermarks from total RAM.
    let total_bytes = total_frames as u64 * PAGE_SIZE as u64;
    watcher().lock().arm(total_bytes);

    // 5. F062 — without a swap device yet, swapping is off by policy: the
    //    framework is present and refuses cleanly instead of misbehaving.
    // SAFETY: single-threaded boot path.
    unsafe {
        mm::swap_mut().set_backing(mm::SwapBacking::None);
    }

    let (managed, free, quarantined) = {
        let z = pmm::zone().lock();
        (z.count(), z.free_frames(), pmm::quarantined())
    };
    st.managed_frames = managed;
    st.free_frames = free;
    st.quarantined = quarantined;
    st.watermarks = watcher().lock().marks();
    st.water = watcher().lock().sample(st.free_bytes(), total_bytes);
    st.swap = mm::swap().backing();

    // 7. F075 — self-test the whole chain.
    st.self_test = mm::run_memory_checks();

    crate::kinfo!(
        "mem: {} frames ({} MiB) {} free, watermarks {}/{}/{}, self-test {}/{}",
        st.managed_frames,
        st.total_bytes() / (1024 * 1024),
        st.free_frames,
        st.watermarks.min_bytes,
        st.watermarks.low_bytes,
        st.watermarks.high_bytes,
        st.self_test.0,
        st.self_test.0 + st.self_test.1
    );
    st
}

/// Render the domain HUD onto the console.
pub fn render_to_console(st: &MemDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = mm::mem_selftest().render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

/// Turn AI-01's memory map into usable/reserved bitmaps.
///
/// The bitmaps are pmm's static storage: each `FrameBitmap` is 64 KiB, and
/// returning them by value used to put 128 KiB on the bootloader's stack
/// (crash, QEMU 2026-09-12). Returns the usable-frame count.
fn boot_bitmaps() -> usize {
    let map = crate::memmap::init();
    let total = (pmm::MAX_FRAMES).min(u64::MAX as usize);
    let frames;
    {
        let mut usable = pmm::usable_map().lock();
        let mut reserved = pmm::reserved_map().lock();

        match map {
            Some(state) => {
                frames = (state.usable_bytes / PAGE_SIZE as u64) as usize;
                usable.reset(total);
                reserved.reset(total);
                // Everything above the reported usable window is reserved: the
                // allocator must never hand out a frame the firmware did not
                // describe as RAM.
                for f in 0..total {
                    usable.set(f, f < frames);
                }
                // The kernel image, framebuffer, ACPI tables and log ring are
                // already tracked by F016; mirror them as reserved frames here
                // so the frame allocators can never hand them out.
                let res = crate::memmap::reservations();
                for i in 0..res.len() {
                    if let Some(r) = res.get(i) {
                        if r.length == 0 {
                            continue;
                        }
                        let first = (r.base / PAGE_SIZE as u64) as usize;
                        let last = ((r.base + r.length) / PAGE_SIZE as u64) as usize;
                        for f in first..=last.min(total.saturating_sub(1)) {
                            reserved.set(f, true);
                        }
                    }
                }
                crate::kinfo!(
                    "mem::boot: {} usable frames, {} reserved ranges",
                    frames,
                    crate::memmap::reservations().len()
                );
            }
            None => {
                frames = 0;
                usable.reset(total);
                reserved.reset(total);
                crate::kwarn!("mem::boot: no memory map — allocator starts empty");
            }
        }
        // The first 1 MiB is never usable, whatever the firmware says.
        for f in 0..(1024 * 1024 / PAGE_SIZE) {
            reserved.set(f, true);
        }
    }
    frames
}

fn watcher() -> &'static crate::cpu::sync::SpinProtected<mm::WatermarkWatcher> {
    mm::watcher()
}

/// Watermarks as configured, for the self-test (F064 gate).
pub fn mm_watcher_marks() -> mm::Watermarks {
    watcher().lock().marks()
}
