# -*- coding: utf-8 -*-
"""任务20：lib.rs 挂载 drivers + nvme probe 入口 + main.rs 接线。"""
from pathlib import Path

ROOT = Path("kernel/varix/src")

# ---- 1) lib.rs 挂载 drivers（此前整个 drivers/ 从未编译）----
lib = ROOT / "lib.rs"
s = lib.read_text(encoding="utf-8")
if "pub mod drivers;" not in s:
    anchor = "pub mod display;"
    assert anchor in s, "lib.rs anchor"
    s = s.replace(anchor, anchor + "\npub mod drivers;", 1)
    lib.write_text(s, encoding="utf-8")
assert "pub mod drivers;" in lib.read_text(encoding="utf-8")
print("lib.rs wired")

# ---- 2) nvme.rs target mod 追加 probe_and_selftest ----
p = ROOT / "drivers" / "nvme.rs"
s = p.read_text(encoding="utf-8")
if "pub fn probe_and_selftest" not in s:
    anchor = """    impl DmaMem for DmaBuckets {
        fn alloc_frame(&mut self) -> Option<u64> {
            let f = crate::mem::pmm::alloc_page()?;
            self.frames.iter_mut().find(|s| s.is_none()).map(|s| *s = Some(f))?;
            Some(f)
        }
        fn free_frame(&mut self, phys: u64) {
            if let Some(slot) = self.frames.iter_mut().find(|s| **s == Some(phys)) {
                *slot = None;
                crate::mem::pmm::free_order(phys, 0);
            }
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            let virt = phys + off + self.hhdm;
            // SAFETY: phys 为 PMM 持有帧，off+len ≤4KiB。
            for (i, &b) in data.iter().enumerate() {
                unsafe { core::ptr::write_volatile((virt + i as u64) as *mut u8, b) };
            }
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            let virt = phys + off + self.hhdm;
            for (i, slot) in out.iter_mut().enumerate() {
                // SAFETY: 同 write_bytes。
                *slot = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
            }
        }
    }
}"""
    assert anchor in s, "DmaBuckets anchor"
    probe = anchor[:-1] + """
    /// 任务16 实机入口：ACPI→MCFG→ECAM 扫描→BAR 映射→初始化→回环 ×1000。
    /// 无 NVMe 控制器/无 MCFG 时优雅跳过（镜像在其他验收配置下照常工作）。
    pub fn probe_and_selftest() {
        let Some(rsdp) = crate::limine::rsdp_address() else {
            crate::kinfo!("blk: no RSDP - block stack skipped");
            return;
        };
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        let Some(mcfg_phys) = super::pci::target::find_mcfg_phys(rsdp, hhdm) else {
            crate::kinfo!("blk: no MCFG table - block stack skipped");
            return;
        };
        let Some(seg) = super::pci::target::read_first_segment(mcfg_phys, hhdm) else {
            crate::kinfo!("blk: MCFG has no usable segment - skipped");
            return;
        };
        crate::kinfo!(
            "pci: ECAM base={:#x} bus {}..={}",
            seg.base,
            seg.start_bus,
            seg.end_bus
        );
        let mut ecam = super::pci::target::EcamMmio::new(seg);
        let Some(hit) = super::pci::scan_nvme(&mut ecam, &seg) else {
            crate::kinfo!("blk: no NVMe controller - block selftest skipped (graceful)");
            return;
        };
        crate::kinfo!(
            "pci: NVMe controller at {:#x}:{:#x}.{} bar0={:#x} ecam_pages={}",
            hit.bus,
            hit.dev,
            hit.func,
            hit.bar0,
            super::pci::target::ECAM_PAGES_MAPPED.load(core::sync::atomic::Ordering::Relaxed)
        );
        let Some(bar) = BarMmio::map(hit.bar0, 4) else {
            crate::kwarn!("blk: BAR0 map failed - skipped");
            return;
        };
        let buckets = DmaBuckets::new();
        match NvmeCtrl::init_with_recovery(bar, buckets, now_ns, 3_000_000_000) {
            Ok(mut ctrl) => {
                let (bs, n) = ctrl.geometry();
                crate::kinfo!(
                    "nvme: init ok resets={} block_size={} blocks={} ({} MiB)",
                    ctrl.resets,
                    bs,
                    n,
                    n * bs as u64 / (1 << 20)
                );
                let mut buf = [0u8; 4096];
                let rep = super::blk::loopback_probe(&mut ctrl, 1000, 8, &mut buf);
                crate::kinfo!(
                    "nvme: loopback x1000 passed={} rounds={} write_sum={:#018x} read_sum={:#018x} err={:?}",
                    rep.passed,
                    rep.rounds,
                    rep.write_sum,
                    rep.read_sum,
                    rep.err
                );
                if !rep.passed {
                    crate::kwarn!("nvme: loopback FAILED - see err above");
                }
            }
            Err(e) => crate::kwarn!("nvme: init failed {:?} - selftest skipped", e),
        }
    }
}"""
    s = s.replace(anchor, probe, 1)
    p.write_text(s, encoding="utf-8")
assert "pub fn probe_and_selftest" in p.read_text(encoding="utf-8")
print("nvme probe added")

# ---- 3) main.rs 接线（storage 段后） ----
m = ROOT / "main.rs"
s = m.read_text(encoding="utf-8")
if "probe_and_selftest" not in s:
    anchor = """    let storage_state = varix::storage::init();
    varix::storage::render_to_console(&storage_state);
"""
    assert anchor in s, "main.rs anchor"
    s = s.replace(
        anchor,
        anchor
        + """
    // --- block stack probe（任务16：块设备抽象 + NVMe 最小栈）------------------------
    varix::drivers::nvme::target::probe_and_selftest();
""",
        1,
    )
    m.write_text(s, encoding="utf-8")
assert "probe_and_selftest" in m.read_text(encoding="utf-8")
print("main.rs wired")
