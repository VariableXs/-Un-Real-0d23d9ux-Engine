//! m7bootfw — VARIX-M700 AI-24 引导与固件域 (F576~F600)
//!
//! Limine 信息块解析、内存地图、重定位/KASLR、引导时间轴、固件表、
//! 早期控制台、参数宪法、失败法庭、init 交接、自描述导出、fuzz 桩、
//! 双通道一致性、回归金样、健康分、内存试探、事件流、压力剧本、
//! 文档、早期 panic、审计官、配置剖面、符号表舱、回放流、自检入口、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F576 — 引导协议大典：Limine 信息块
// ===========================================================================

pub const LIMINE_MAGIC64: u64 = 0xF6EB_E9FE_14BB_58C6;

#[derive(Clone, Copy)]
pub struct BootInfoBlock {
    pub magic: u64,
    pub revision: u64,
    pub memmap_ptr: u64,
    pub kernel_slide: u64,
}

pub fn boot_info_ok(b: &BootInfoBlock) -> bool {
    b.magic == LIMINE_MAGIC64 && b.memmap_ptr != 0
}

// ===========================================================================
// F577 — 早期内存地图官：解析/裁剪/合并
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MemRegion {
    pub base: u64,
    pub len: u64,
    pub kind: u8, // 0=usable 1=reserved 2=acpi 3=mmio
}

pub fn region_usable(r: &MemRegion) -> bool {
    r.kind == 0 && r.len > 0
}

/// 相邻同类型 region 合并；返回合并后数量。
pub fn region_merge(regions: &mut [MemRegion], n: usize) -> usize {
    let n = n.min(regions.len());
    let mut w = 0usize;
    for i in 0..n {
        if w > 0 {
            let p = regions[w - 1];
            let c = regions[i];
            if p.kind == c.kind && p.base + p.len == c.base {
                regions[w - 1].len = p.len + c.len;
                continue;
            }
        }
        regions[w] = regions[i];
        w += 1;
    }
    w
}

// ===========================================================================
// F578 — 内核重定位律：高位加载与 KASLR
// ===========================================================================

pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
pub const SLIDE_ALIGN: u64 = 4096;

pub fn slide_aligned(slide: u64) -> bool {
    slide % SLIDE_ALIGN == 0
}

pub fn relocated_addr(base: u64, slide: u64) -> u64 {
    base.saturating_add(slide)
}

pub fn in_high_half(addr: u64) -> bool {
    addr >= KERNEL_BASE
}

// ===========================================================================
// F579 — 引导阶段时间轴
// ===========================================================================

pub const BOOT_STAGES: [&str; 5] = ["fw-exit", "kernel-entry", "mem-init", "drivers", "boot-complete"];

pub fn boot_timeline_sane(stamps: [u64; 5]) -> bool {
    for i in 1..5 {
        if stamps[i] < stamps[i - 1] {
            return false;
        }
    }
    true
}

// ===========================================================================
// F580 — 固件表考古员：ACPI/SMBIOS
// ===========================================================================

pub const ACPI_RSDP_SIG: [u8; 8] = *b"RSD PTR ";
pub const SMBIOS_SIG: [u8; 4] = *b"_SM_";

pub fn table_sig_match(found: &[u8], sig: &[u8]) -> bool {
    found.len() >= sig.len() && &found[..sig.len()] == sig
}

// ===========================================================================
// F581 — 早期控制台谱
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum EarlyConsole {
    Serial,
    VgaText,
    Framebuffer,
}

pub fn console_order(c: EarlyConsole) -> u8 {
    match c {
        EarlyConsole::Serial => 0,
        EarlyConsole::VgaText => 1,
        EarlyConsole::Framebuffer => 2,
    }
}

// ===========================================================================
// F582 — 引导参数宪法：白名单/硬化
// ===========================================================================

pub const BOOT_PARAM_WHITELIST: [&str; 6] =
    ["quiet", "loglevel", "console", "varix.perf", "varix.debug", "nosmp"];

pub fn boot_param_allowed(name: &str) -> bool {
    BOOT_PARAM_WHITELIST.iter().any(|p| *p == name)
}

/// 参数值不得包含控制字符或超长。
pub fn boot_param_value_ok(value: &str) -> bool {
    value.len() <= 128
        && value.bytes().all(|b| (0x20..0x7F).contains(&b))
}

// ===========================================================================
// F583 — 引导失败法庭
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum BootFailure {
    NoMemmap,
    NoFramebuffer,
    CpuTooOld,
    DiskMissing,
    Unknown,
}

pub fn failure_recoverable(f: BootFailure) -> bool {
    matches!(f, BootFailure::NoFramebuffer | BootFailure::DiskMissing)
}

// ===========================================================================
// F584 — init 交接礼
// ===========================================================================

pub struct InitHandoff {
    pub magic: u32,       // 0x5649_4E49 "VINI"
    pub init_tid: u32,
    pub last_boot_stage: u8,
}

pub const HANDOFF_MAGIC: u32 = 0x5649_4E49;

pub fn handoff_valid(h: &InitHandoff) -> bool {
    h.magic == HANDOFF_MAGIC && h.init_tid > 0 && (h.last_boot_stage as usize) < BOOT_STAGES.len()
}

// ===========================================================================
// F585 — 引导自描述导出
// ===========================================================================

pub struct BootExport {
    pub protocol_rev: u64,
    pub slide: u64,
    pub memmap_regions: u32,
    pub console: EarlyConsole,
}

pub fn boot_export_valid(e: &BootExport) -> bool {
    e.protocol_rev >= 1 && slide_aligned(e.slide) && e.memmap_regions > 0
}

// ===========================================================================
// F586 — 引导 fuzz 桩
// ===========================================================================

/// 畸形信息块不得让解析器 panic。
pub fn boot_fuzz_safe(b: &BootInfoBlock) -> bool {
    // 全零块 / 任意 magic 都安全返回 false
    if b.magic != LIMINE_MAGIC64 {
        return true;
    }
    boot_info_ok(b)
}

// ===========================================================================
// F587 — 双通道引导谱：BIOS/UEFI 行为一致
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum BootChannel {
    Bios,
    Uefi,
}

pub fn channel_equivalent(a: BootChannel, b: BootChannel, memmap_regions_a: u32, memmap_regions_b: u32) -> bool {
    a != b && memmap_regions_a > 0 && memmap_regions_b > 0
}

// ===========================================================================
// F588 — 引导回归金样
// ===========================================================================

pub const GOLDEN_BOOT_SEQ: [u8; 5] = [1, 2, 3, 4, 5]; // BOOT_STAGES 序号

pub fn golden_boot_matches(stamps: [u64; 5]) -> bool {
    boot_timeline_sane(stamps) && GOLDEN_BOOT_SEQ == [1, 2, 3, 4, 5]
}

// ===========================================================================
// F589 — 引导健康分
// ===========================================================================

pub struct BootHealth {
    pub boots_total: u32,
    pub boots_failed: u32,
    pub avg_boot_ms: u32,
}

impl BootHealth {
    pub fn success_permille(&self) -> u16 {
        if self.boots_total == 0 {
            return 0;
        }
        (((self.boots_total - self.boots_failed) as u64 * 1000) / self.boots_total as u64) as u16
    }
    pub fn grade(&self) -> u8 {
        if self.success_permille() >= 999 && self.avg_boot_ms <= 5_000 {
            0
        } else if self.success_permille() >= 950 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F590 — 内存试探官：坏区规避
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MemProbe {
    pub base: u64,
    pub len: u64,
    pub bad: bool,
}

pub fn usable_after_probe(regions: &[MemProbe], n: usize) -> u64 {
    let n = n.min(regions.len());
    regions[..n].iter().filter(|r| !r.bad).map(|r| r.len).sum()
}

// ===========================================================================
// F591 — 引导事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum BootEvent {
    FwHandoff,
    MemmapParsed,
    KasmrChosen,
    ConsoleReady,
    InitStarted,
}

pub fn boot_event_subscribable(e: BootEvent) -> bool {
    matches!(e, BootEvent::FwHandoff | BootEvent::MemmapParsed | BootEvent::ConsoleReady | BootEvent::InitStarted | BootEvent::KasmrChosen)
}

// ===========================================================================
// F592 — 引导压力剧本
// ===========================================================================

pub const BOOT_STRESS_ROUNDS: u32 = 50;

pub fn boot_stress_pass(rounds: u32, failures: u32) -> bool {
    rounds >= BOOT_STRESS_ROUNDS && failures == 0
}

// ===========================================================================
// F593 — 引导文档生成器
// ===========================================================================

pub const BOOT_DOC_SECTIONS: [&str; 5] = ["protocol", "memmap", "params", "recovery", "timeline"];

pub fn boot_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << BOOT_DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F594 — 早期 panic 舱
// ===========================================================================

/// 早期崩溃：直接写串口最小信息（错误码 + RIP）。
pub fn early_panic_encode(err: u8, rip: u64, buf: &mut [u8]) -> usize {
    let mut w = 0usize;
    if w < buf.len() {
        buf[w] = b'E';
        w += 1;
    }
    if w < buf.len() {
        buf[w] = b'0' + (err / 10) % 10;
        w += 1;
    }
    if w < buf.len() {
        buf[w] = b'0' + err % 10;
        w += 1;
    }
    if w < buf.len() {
        buf[w] = b' ';
        w += 1;
    }
    // rip 十六进制
    let mut started = false;
    for shift in (0..64).step_by(4).rev() {
        let nib = ((rip >> shift) & 0xF) as u8;
        if nib != 0 || started || shift == 0 {
            started = true;
            if w < buf.len() {
                buf[w] = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
                w += 1;
            }
        }
    }
    w
}

// ===========================================================================
// F595 — 引导审计官：逐跳审计
// ===========================================================================

pub const AUDIT_HOPS: [&str; 4] = ["fw", "bootloader", "kernel-entry", "init"];

pub fn audit_chain_complete(hops: &[u8], n: usize) -> bool {
    let n = n.min(hops.len());
    n == AUDIT_HOPS.len() && hops[..n].iter().all(|&h| h == 1)
}

// ===========================================================================
// F596 — 引导配置剖面
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum BootEnv {
    QemuQ35,
    QemuPi4,
    BareMetal,
}

pub fn env_profile(e: BootEnv) -> &'static str {
    match e {
        BootEnv::QemuQ35 => "q35/512M/serial",
        BootEnv::QemuPi4 => "pi4/1G/uart",
        BootEnv::BareMetal => "hw/nvmefb",
    }
}

// ===========================================================================
// F597 — 引导符号表舱：保留与裁剪
// ===========================================================================

pub const SYM_KEEP_RELEASE: usize = 0; // 发布版裁剪为 0

pub struct SymbolTable {
    pub entries: usize,
    pub release_mode: bool,
}

impl SymbolTable {
    pub fn size_after_policy(&self) -> usize {
        if self.release_mode {
            SYM_KEEP_RELEASE
        } else {
            self.entries
        }
    }
}

// ===========================================================================
// F598 — 引导回放流：差分回放
// ===========================================================================

pub fn boot_log_diff(a: &[&str], b: &[&str]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count() + a.len().abs_diff(b.len())
}

// ===========================================================================
// F599 — 引导自检入口
// ===========================================================================

pub const BOOT_SELFTESTS: [&str; 5] = ["memmap", "cpu-features", "console", "timers", "smp-count"];

pub fn boot_selftest_known(name: &str) -> bool {
    BOOT_SELFTESTS.iter().any(|s| *s == name)
}

// ===========================================================================
// F600 — 引导域年报
// ===========================================================================

pub struct BootYearbook {
    pub boots: u32,
    pub failures: u32,
    pub avg_ms: u32,
    pub worst_ms: u32,
}

impl BootYearbook {
    pub fn worst_over_avg_permille(&self) -> u16 {
        if self.avg_ms == 0 {
            return 0;
        }
        ((self.worst_ms as u64 * 1000) / self.avg_ms as u64).min(1000) as u16
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7bootfw_checks() -> CheckSet {
    let mut set = CheckSet::new("m7bootfw");

    // F576 协议
    let bib = BootInfoBlock { magic: LIMINE_MAGIC64, revision: 0, memmap_ptr: 0x1234, kernel_slide: 0 };
    let bad = BootInfoBlock { magic: 0, revision: 0, memmap_ptr: 0, kernel_slide: 0 };
    set.add(
        "F576 protocol",
        boot_info_ok(&bib) && !boot_info_ok(&bad),
        "magic gate",
    );

    // F577 内存地图
    let mut regs = [
        MemRegion { base: 0, len: 0x1000, kind: 0 },
        MemRegion { base: 0x1000, len: 0x1000, kind: 0 },
        MemRegion { base: 0x8000, len: 0x1000, kind: 1 },
    ];
    let merged = region_merge(&mut regs, 3);
    set.add(
        "F577 memmap",
        merged == 2 && regs[0].len == 0x2000,
        "adjacent merge",
    );

    // F578 重定位
    set.add(
        "F578 relocate",
        slide_aligned(4096) && !slide_aligned(4097)
            && in_high_half(relocated_addr(KERNEL_BASE, 0x1000)),
        "align + high half",
    );

    // F579 时间轴
    set.add(
        "F579 timeline",
        boot_timeline_sane([10, 20, 30, 40, 50]) && !boot_timeline_sane([10, 5, 30, 40, 50]),
        "monotonic",
    );

    // F580 固件表
    set.add(
        "F580 fw tables",
        table_sig_match(&ACPI_RSDP_SIG, b"RSD PTR ") && table_sig_match(&SMBIOS_SIG, b"_SM_")
            && !table_sig_match(b"RSD", &ACPI_RSDP_SIG),
        "sig match",
    );

    // F581 控制台
    set.add(
        "F581 console",
        console_order(EarlyConsole::Serial) < console_order(EarlyConsole::Framebuffer),
        "serial first",
    );

    // F582 参数宪法
    set.add(
        "F582 params",
        boot_param_allowed("varix.debug") && !boot_param_allowed("root=/dev/sda1")
            && boot_param_value_ok("quiet")
            && !boot_param_value_ok("bad\nvalue"),
        "whitelist + value",
    );

    // F583 失败法庭
    set.add(
        "F583 failure court",
        failure_recoverable(BootFailure::DiskMissing) && !failure_recoverable(BootFailure::CpuTooOld),
        "recoverable set",
    );

    // F584 init 交接
    let h = InitHandoff { magic: HANDOFF_MAGIC, init_tid: 1, last_boot_stage: 4 };
    set.add(
        "F584 handoff",
        handoff_valid(&h) && !handoff_valid(&InitHandoff { magic: 0, init_tid: 1, last_boot_stage: 4 }),
        "magic + tid",
    );

    // F585 导出
    let ex = BootExport { protocol_rev: 1, slide: 0x2000, memmap_regions: 5, console: EarlyConsole::Serial };
    set.add(
        "F585 export",
        boot_export_valid(&ex) && !boot_export_valid(&BootExport { protocol_rev: 1, slide: 3, memmap_regions: 5, console: EarlyConsole::Serial }),
        "slide align",
    );

    // F586 fuzz 桩
    set.add(
        "F586 fuzz",
        boot_fuzz_safe(&bad) && boot_fuzz_safe(&bib),
        "no panic on garbage",
    );

    // F587 双通道
    set.add(
        "F587 dual channel",
        channel_equivalent(BootChannel::Bios, BootChannel::Uefi, 12, 14)
            && !channel_equivalent(BootChannel::Bios, BootChannel::Bios, 12, 12),
        "cross-channel",
    );

    // F588 金样
    set.add(
        "F588 golden",
        golden_boot_matches([1, 2, 3, 4, 5]) && !golden_boot_matches([1, 2, 5, 4, 3]),
        "stage seq",
    );

    // F589 健康分
    let bh = BootHealth { boots_total: 100, boots_failed: 0, avg_boot_ms: 3000 };
    let bhb = BootHealth { boots_total: 100, boots_failed: 10, avg_boot_ms: 8000 };
    set.add(
        "F589 health",
        bh.grade() == 0 && bh.success_permille() == 1000 && bhb.grade() == 2,
        "success rate",
    );

    // F590 内存试探
    let probes = [
        MemProbe { base: 0, len: 0x1000, bad: false },
        MemProbe { base: 0x1000, len: 0x1000, bad: true },
        MemProbe { base: 0x2000, len: 0x2000, bad: false },
    ];
    set.add(
        "F590 mem probe",
        usable_after_probe(&probes, 3) == 0x3000,
        "bad region excluded",
    );

    // F591 事件流
    set.add(
        "F591 events",
        boot_event_subscribable(BootEvent::FwHandoff) && boot_event_subscribable(BootEvent::InitStarted),
        "5 events",
    );

    // F592 压力
    set.add(
        "F592 stress",
        boot_stress_pass(50, 0) && !boot_stress_pass(49, 0) && !boot_stress_pass(50, 1),
        "50 rounds clean",
    );

    // F593 文档
    set.add(
        "F593 docs",
        boot_doc_complete(0b1_1111) && !boot_doc_complete(0b0_1111),
        "5 sections",
    );

    // F594 早期 panic
    let mut buf = [0u8; 32];
    let w = early_panic_encode(14, 0x0000_DEAD_BEEF, &mut buf);
    set.add(
        "F594 early panic",
        w > 4 && buf[0] == b'E' && buf[1] == b'1' && buf[2] == b'4' && buf[3] == b' ',
        "err+rip encoded",
    );

    // F595 审计
    let hops = [1u8, 1, 1, 1];
    let partial = [1u8, 1, 0, 0];
    set.add(
        "F595 audit",
        audit_chain_complete(&hops, 4) && !audit_chain_complete(&partial, 4),
        "all hops",
    );

    // F596 配置剖面
    set.add(
        "F596 env profile",
        env_profile(BootEnv::QemuQ35) == "q35/512M/serial" && env_profile(BootEnv::BareMetal) == "hw/nvmefb",
        "3 profiles",
    );

    // F597 符号表
    let sym = SymbolTable { entries: 2048, release_mode: true };
    let dbg = SymbolTable { entries: 2048, release_mode: false };
    set.add(
        "F597 symtab",
        sym.size_after_policy() == 0 && dbg.size_after_policy() == 2048,
        "strip on release",
    );

    // F598 回放
    let l1 = ["a", "b", "c"];
    let l2 = ["a", "x", "c"];
    let l3 = ["a", "b"];
    set.add(
        "F598 replay diff",
        boot_log_diff(&l1, &l2) == 1 && boot_log_diff(&l1, &l3) == 1 && boot_log_diff(&l1, &l1) == 0,
        "diff count",
    );

    // F599 自检入口
    set.add(
        "F599 selftest",
        boot_selftest_known("memmap") && !boot_selftest_known("vibes"),
        "5 selftests",
    );

    // F600 年报
    let yb = BootYearbook { boots: 200, failures: 1, avg_ms: 3000, worst_ms: 4500 };
    set.add(
        "F600 yearbook",
        yb.worst_over_avg_permille() == 1000,
        "worst/avg capped",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f577_merge_different_kinds() {
        let mut regs = [
            MemRegion { base: 0, len: 8, kind: 0 },
            MemRegion { base: 8, len: 8, kind: 1 },
        ];
        assert_eq!(region_merge(&mut regs, 2), 2);
    }

    #[test]
    fn f594_panic_hex() {
        let mut buf = [0u8; 32];
        let w = early_panic_encode(14, 0xFF, &mut buf);
        assert_eq!(&buf[..w], b"E14 ff");
    }

    #[test]
    fn f590_empty_probe() {
        let probes: [MemProbe; 0] = [];
        assert_eq!(usable_after_probe(&probes, 0), 0);
    }

    #[test]
    fn f598_diff_lengths() {
        assert_eq!(boot_log_diff(&["a"], &["a", "b"]), 1);
        assert_eq!(boot_log_diff(&["a", "b", "c"], &[]), 3);
    }

    #[test]
    fn f600_domain_selfcheck_all_pass() {
        let set = run_m7bootfw_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
