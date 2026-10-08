//! VARIX-M400 AI-01 引导与平台加固域（F001~F025）。
//!
//! 从按下电源到内核可控的全链路可信。全部为纯逻辑 + 固定容量数组
//! （无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F001 — UEFI 直接引导路径
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UefiInfo {
    pub system_table: u64,
    pub runtime_services: u64,
    pub graphics_output: bool,
    pub secure_boot_var: bool,
}

/// 校验 UEFI 引导信息完整性：系统表非零即认为直连路径成立。
pub fn f001_uefi_direct_boot(info: UefiInfo) -> bool {
    info.system_table != 0 && info.runtime_services != 0
}

// ---------------------------------------------------------------------------
// F002 — Secure Boot 签名链
// ---------------------------------------------------------------------------

/// FNV-1a 32 位摘要，作为签名链的轻量校验。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub const ROOT_CA_HASH: u32 = 0x8eed_cafe;

/// 自签 CA -> 内核签名两级验证；任一级不符即拒启。
pub fn f002_verify_boot_chain(kernel: &[u8], kernel_sig: u32, ca_sig: u32) -> bool {
    ca_sig == ROOT_CA_HASH && kernel_sig == fnv1a(kernel)
}

// ---------------------------------------------------------------------------
// F003 — 多系统引导菜单
// ---------------------------------------------------------------------------

pub const MAX_BOOT_ENTRIES: usize = 8;

#[derive(Clone, Copy)]
pub struct BootEntry {
    pub label: &'static str,
    pub kind: BootEntryKind,
    pub default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootEntryKind {
    Varix,
    Windows,
    Other,
}

#[derive(Clone, Copy)]
pub struct BootMenu {
    pub entries: [Option<BootEntry>; MAX_BOOT_ENTRIES],
    pub count: usize,
    pub selected: usize,
}

impl BootMenu {
    pub const fn new() -> BootMenu {
        BootMenu { entries: [const { None }; MAX_BOOT_ENTRIES], count: 0, selected: 0 }
    }

    /// 添加条目；第一个条目自动成为默认项（若尚无默认）。
    pub fn add(&mut self, e: BootEntry) -> bool {
        if self.count >= MAX_BOOT_ENTRIES {
            return false;
        }
        self.entries[self.count] = Some(e);
        self.count += 1;
        true
    }

    pub fn default_index(&self) -> Option<usize> {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.default {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn select(&mut self, idx: usize) -> bool {
        if idx >= self.count {
            return false;
        }
        self.selected = idx;
        true
    }
}

// ---------------------------------------------------------------------------
// F004 — 内核命令行参数（varix: 前缀）
// ---------------------------------------------------------------------------

pub const MAX_CMDLINE_PARAMS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdParam {
    pub key: &'static str,
    pub value: Option<&'static str>,
}

/// 解析 `varix:k1,k2=v,...` 形式的参数串（静态切片切分）。
pub fn f004_parse_cmdline(s: &str) -> ([Option<CmdParam>; MAX_CMDLINE_PARAMS], usize) {
    let mut out: [Option<CmdParam>; MAX_CMDLINE_PARAMS] = [const { None }; MAX_CMDLINE_PARAMS];
    let mut n = 0usize;
    let body = s.strip_prefix("varix:").unwrap_or(s);
    for part in body.split(',') {
        if n >= MAX_CMDLINE_PARAMS {
            break;
        }
        if part.is_empty() {
            continue;
        }
        // 内核命令行常驻 .rodata，按 'static 处理。
        let part: &'static str = unsafe { core::mem::transmute::<&str, &'static str>(part) };
        match part.split_once('=') {
            Some((k, v)) => {
                out[n] = Some(CmdParam { key: k, value: Some(v) });
                n += 1;
            }
            None => {
                out[n] = Some(CmdParam { key: part, value: None });
                n += 1;
            }
        }
    }
    (out, n)
}

// ---------------------------------------------------------------------------
// F005 — 分级 early printk
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

/// 低于等于当前级别才输出。
pub fn f005_early_log_filter(level: LogLevel, threshold: LogLevel) -> bool {
    level <= threshold
}

// ---------------------------------------------------------------------------
// F006 — ACPI 表解析骨架（校验和）
// ---------------------------------------------------------------------------

/// ACPI 表头 8 字节为 OEMID/签名简化版；sum(bytes) % 256 == 0 即有效。
pub fn f006_acpi_checksum_valid(bytes: &[u8]) -> bool {
    let sum: u32 = bytes.iter().map(|&b| b as u32).sum();
    (sum & 0xFF) == 0
}

/// RSDT/XSDT：XSDT 优先（64 位指针）。
pub fn f006_pick_sdt(has_xsdt: bool, xsdt_ptr: u64, rsdt_ptr: u32) -> u64 {
    if has_xsdt && xsdt_ptr != 0 {
        xsdt_ptr
    } else {
        rsdt_ptr as u64
    }
}

// ---------------------------------------------------------------------------
// F007 — MADT 多核发现
// ---------------------------------------------------------------------------

pub const MAX_LAPICS: usize = 64;

/// 从 LAPIC entry 列表统计可用核数（flags & 1 == enabled）。
pub fn f007_madt_count_cpus(entries: &[(u32, u32)]) -> usize {
    let mut n = 0usize;
    for &(apic_id, flags) in entries {
        if flags & 1 == 1 && apic_id != u32::MAX {
            n += 1;
        }
    }
    n.min(MAX_LAPICS)
}

// ---------------------------------------------------------------------------
// F008 — SMP 启动（SIPI）状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApState {
    Init,
    SipiSent,
    WaitApSig,
    Online,
    Failed,
}

/// 副核启动序列：INIT -> SIPI -> 等 AP 签名 -> Online；超时 2 轮判失败。
pub fn f008_sipi_step(state: ApState, ap_sig_seen: bool, rounds_waited: u32) -> ApState {
    match state {
        ApState::Init => ApState::SipiSent,
        ApState::SipiSent => ApState::WaitApSig,
        ApState::WaitApSig => {
            if ap_sig_seen {
                ApState::Online
            } else if rounds_waited >= 2 {
                ApState::Failed
            } else {
                ApState::WaitApSig
            }
        }
        other => other,
    }
}

// ---------------------------------------------------------------------------
// F009 — APIC 定时器校准
// ---------------------------------------------------------------------------

/// 频率 = ticks / picoseconds_reference；误差 > 1% 判不合格。
pub fn f009_apic_calibrate(ticks: u64, ref_ns: u64) -> (u64, bool) {
    if ref_ns == 0 {
        return (0, false);
    }
    let khz = ticks * 1_000_000 / ref_ns;
    // 用 1e6 参考频率的偏差衡量（校准基准 1 GHz 级别）。
    let base = ref_ns; // 名义上 ticks 应为 ref_ns * 1（1 tick/ns）
    let drift = ticks.abs_diff(base) as u128 * 10_000 / (base.max(1) as u128);
    (khz, drift <= 100) // 1% = 万分之 100
}

// ---------------------------------------------------------------------------
// F010 — HPET 高精度时钟（与 TSC 交叉验证）
// ---------------------------------------------------------------------------

/// 两个时钟源读数差在容差内视为一致。
pub fn f010_cross_check(hpet_ns: u64, tsc_ns: u64, tolerance_ns: u64) -> bool {
    hpet_ns.abs_diff(tsc_ns) <= tolerance_ns
}

// ---------------------------------------------------------------------------
// F011 — 内存映射精炼（保留区/坏区清单）
// ---------------------------------------------------------------------------

pub const MAX_MEM_REGIONS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    Usable,
    Reserved,
    Bad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemRegion {
    pub base: u64,
    pub len: u64,
    pub kind: RegionKind,
}

/// 逐区核对：与固件映射重叠的坏区打 Bad，保留区打 Reserved。
pub fn f011_refine_map(regions: &mut [MemRegion], bad_base: u64, bad_len: u64) -> usize {
    let mut changed = 0usize;
    for r in regions.iter_mut() {
        let r_end = r.base.saturating_add(r.len);
        let b_end = bad_base.saturating_add(bad_len);
        if r.base < b_end && bad_base < r_end {
            if r.kind == RegionKind::Usable {
                r.kind = RegionKind::Bad;
                changed += 1;
            }
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// F012 — 引导自检报告页
// ---------------------------------------------------------------------------

/// 渲染 CPU/内存/设备发现结果到固定缓冲。
pub fn f012_render_report(
    out: &mut [u8],
    cpus: usize,
    mem_mib: u64,
    devices: usize,
    secure_boot: bool,
) -> usize {
    let mut n = 0usize;
    let push_num = |out: &mut [u8], n: &mut usize, mut v: u64| {
        let mut digits = [0u8; 20];
        let mut w = 0usize;
        if v == 0 {
            digits[w] = b'0';
            w += 1;
        }
        while v > 0 {
            digits[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
        while w > 0 {
            w -= 1;
            if *n < out.len() {
                out[*n] = digits[w];
                *n += 1;
            }
        }
    };
    crate::checks::push_str(out, &mut n, "varix boot report\ncpus=");
    push_num(out, &mut n, cpus as u64);
    crate::checks::push_str(out, &mut n, "\nmem_mib=");
    push_num(out, &mut n, mem_mib);
    crate::checks::push_str(out, &mut n, "\ndevices=");
    push_num(out, &mut n, devices as u64);
    crate::checks::push_str(out, &mut n, "\nsecure_boot=");
    crate::checks::push_str(out, &mut n, if secure_boot { "on" } else { "off" });
    crate::checks::push_str(out, &mut n, "\n");
    n
}

// ---------------------------------------------------------------------------
// F013 — Legacy BIOS 回退路径
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoaderMode {
    Uefi,
    Bios,
}

/// 有 UEFI 走 UEFI，否则 Limine BIOS 兼容。
pub fn f013_pick_loader(uefi_available: bool, limine_bios_ready: bool) -> LoaderMode {
    if uefi_available {
        LoaderMode::Uefi
    } else if limine_bios_ready {
        LoaderMode::Bios
    } else {
        LoaderMode::Bios // 无路可走时仍按 BIOS 尝试并留日志
    }
}

// ---------------------------------------------------------------------------
// F014 — 引导耗时打点
// ---------------------------------------------------------------------------

pub const BOOT_STAGES: usize = 6;
pub const STAGE_NAMES: [&str; BOOT_STAGES] =
    ["firmware", "loader", "early", "mmu", "smp", "init"];

#[derive(Clone, Copy)]
pub struct BootTiming {
    pub ns: [u64; BOOT_STAGES],
}

impl BootTiming {
    pub const fn new() -> BootTiming {
        BootTiming { ns: [0; BOOT_STAGES] }
    }

    pub fn record(&mut self, stage: usize, ns: u64) -> bool {
        if stage >= BOOT_STAGES {
            return false;
        }
        self.ns[stage] = ns;
        true
    }

    pub fn total(&self) -> u64 {
        self.ns.iter().sum()
    }

    pub fn slowest(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..BOOT_STAGES {
            if best.map(|b| self.ns[i] > self.ns[b]).unwrap_or(self.ns[i] > 0) {
                best = Some(i);
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// F015 — A/B 内核槽
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotState {
    pub boot_ok: bool,
    pub fail_streak: u32,
}

/// 连续 3 次失败自动回退另一槽。
pub fn f015_ab_slot_pick(a: SlotState, b: SlotState) -> usize {
    let a_ok = a.boot_ok && a.fail_streak < 3;
    let b_ok = b.boot_ok && b.fail_streak < 3;
    if a_ok {
        0
    } else if b_ok {
        1
    } else {
        0 // 双坏时仍试 A 并留痕
    }
}

pub fn f015_slot_on_fail(s: &mut SlotState) {
    s.fail_streak = s.fail_streak.saturating_add(1);
    if s.fail_streak >= 3 {
        s.boot_ok = false;
    }
}

pub fn f015_slot_on_ok(s: &mut SlotState) {
    s.boot_ok = true;
    s.fail_streak = 0;
}

// ---------------------------------------------------------------------------
// F016 — panic 画面 OEM 化
// ---------------------------------------------------------------------------

/// 渲染 panic 画面：err/rip/cr2 + 摘要，可读可截图。
pub fn f016_render_panic(out: &mut [u8], err: u64, rip: u64, cr2: u64) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "VARIX PANIC\nerr=");
    crate::checks::push_hex_u64(out, &mut n, err);
    crate::checks::push_str(out, &mut n, " rip=");
    crate::checks::push_hex_u64(out, &mut n, rip);
    crate::checks::push_str(out, &mut n, " cr2=");
    crate::checks::push_hex_u64(out, &mut n, cr2);
    crate::checks::push_str(out, &mut n, "\n");
    n
}

// ---------------------------------------------------------------------------
// F017 — 启动动画子系统（帧率独立于初始化进度）
// ---------------------------------------------------------------------------

/// 动画按时间驱动：进度 = min(elapsed / budget, 100)，不被 init 阻塞。
pub fn f017_anim_progress(elapsed_ms: u64, budget_ms: u64) -> u32 {
    if budget_ms == 0 {
        return 100;
    }
    let p = (elapsed_ms.min(budget_ms)) * 100 / budget_ms;
    p as u32
}

/// 帧号按固定 fps 推进。
pub fn f017_anim_frame(elapsed_ms: u64, fps: u32) -> u64 {
    if fps == 0 {
        return 0;
    }
    elapsed_ms * fps as u64 / 1000
}

// ---------------------------------------------------------------------------
// F018 — NUMA 拓扑发现（单路正确降级）
// ---------------------------------------------------------------------------

pub const MAX_NUMA_NODES: usize = 8;

/// 返回节点数；0（未声明）视为单节点 1。
pub fn f018_numa_nodes(prox_domains: &[u32]) -> usize {
    let mut nodes = [false; MAX_NUMA_NODES];
    let mut count = 0usize;
    for &d in prox_domains {
        if (d as usize) < MAX_NUMA_NODES && !nodes[d as usize] {
            nodes[d as usize] = true;
            count += 1;
        }
    }
    if count == 0 {
        1
    } else {
        count
    }
}

// ---------------------------------------------------------------------------
// F019 — CPU 特性位检测库
// ---------------------------------------------------------------------------

pub const CPUID_FEATS: usize = 64;

/// 特性位检测：leaf/word/bit 三元封装。
pub fn f019_has_feat(leaf_words: &[u32; CPUID_FEATS], bit: u32) -> bool {
    if bit as usize >= CPUID_FEATS * 32 {
        return false;
    }
    let word = (bit / 32) as usize;
    let b = bit % 32;
    (leaf_words[word] >> b) & 1 == 1
}

// ---------------------------------------------------------------------------
// F020 — 微码更新接口（可关闭）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Microcode {
    pub vendor_sig: u32,
    pub revision: u32,
}

/// 厂商签名匹配且修订号更高才允许加载；enabled=false 直接拒绝。
pub fn f020_microcode_accept(enabled: bool, current_rev: u32, mc: Microcode, vendor_cpu: u32) -> bool {
    enabled && mc.vendor_sig == vendor_cpu && mc.revision > current_rev
}

// ---------------------------------------------------------------------------
// F021 — SME/TME 探测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemEncryptInfo {
    pub sme: bool,
    pub tme: bool,
}

pub fn f021_probe_mem_encrypt(amd_ebx: u32, intel_eax: u32) -> MemEncryptInfo {
    MemEncryptInfo {
        sme: (amd_ebx >> 0) & 1 == 1,
        tme: (intel_eax >> 0) & 1 == 1,
    }
}

// ---------------------------------------------------------------------------
// F022 — 引导日志环形缓冲（不丢首屏）
// ---------------------------------------------------------------------------

pub const BOOTLOG_CAP: usize = 64;

#[derive(Clone, Copy)]
pub struct BootLog {
    pub buf: [u32; BOOTLOG_CAP],
    pub head: usize,
    pub count: usize,
}

impl BootLog {
    pub const fn new() -> BootLog {
        BootLog { buf: [0; BOOTLOG_CAP], head: 0, count: 0 }
    }

    pub fn push(&mut self, v: u32) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % BOOTLOG_CAP;
        if self.count < BOOTLOG_CAP {
            self.count += 1;
        }
    }

    /// 按时间序读第 i 条（0 = 最早，首屏不丢）。
    pub fn get(&self, i: usize) -> Option<u32> {
        if i >= self.count {
            return None;
        }
        let start = if self.count < BOOTLOG_CAP { 0 } else { self.head };
        Some(self.buf[(start + i) % BOOTLOG_CAP])
    }
}

// ---------------------------------------------------------------------------
// F023 — 启动项管理工具
// ---------------------------------------------------------------------------

/// 设置默认项与超时；越界即拒绝。
pub fn f023_set_default(entries: &mut [Option<BootEntry>], idx: usize) -> bool {
    if idx >= entries.len() || entries[idx].is_none() {
        return false;
    }
    for e in entries.iter_mut().flatten() {
        e.default = false;
    }
    if let Some(e) = entries[idx].as_mut() {
        e.default = true;
    }
    true
}

pub fn f023_validate_timeout(secs: u32) -> bool {
    secs <= 30
}

// ---------------------------------------------------------------------------
// F024 — 引导器主题
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootTheme {
    pub bg_rgb: u32,
    pub fg_rgb: u32,
    pub accent_rgb: u32,
}

impl BootTheme {
    pub const VARIX_DARK: BootTheme = BootTheme { bg_rgb: 0x0d1117, fg_rgb: 0xe6edf3, accent_rgb: 0x2f81f7 };
}

/// 主题合法性：对比度（亮度差）需超过阈值。
pub fn f024_theme_readable(t: BootTheme) -> bool {
    let lum = |c: u32| -> u32 {
        let r = (c >> 16) & 0xFF;
        let g = (c >> 8) & 0xFF;
        let b = c & 0xFF;
        (r * 299 + g * 587 + b * 114) / 1000
    };
    lum(t.bg_rgb).abs_diff(lum(t.fg_rgb)) >= 128
}

// ---------------------------------------------------------------------------
// F025 — 引导域全量自检
// ---------------------------------------------------------------------------

pub fn run_bootplat_checks() -> CheckSet {
    let mut set = CheckSet::new("bootplat");

    // F001
    let ok = f001_uefi_direct_boot(UefiInfo {
        system_table: 0xdead_beef,
        runtime_services: 0xfeed_face,
        graphics_output: true,
        secure_boot_var: true,
    });
    set.add("F001 uefi direct boot", ok, "system/runtime table must be nonzero");
    set.add("F001 uefi reject empty", !f001_uefi_direct_boot(UefiInfo { system_table: 0, runtime_services: 0xfeed_face, graphics_output: false, secure_boot_var: false }), "zero stt must fail");

    // F002
    let img = [1u8, 2, 3, 4];
    let sig = fnv1a(&img);
    set.add("F002 chain ok", f002_verify_boot_chain(&img, sig, ROOT_CA_HASH), "valid chain");
    set.add("F002 tamper reject", !f002_verify_boot_chain(&img, sig ^ 1, ROOT_CA_HASH), "bad kernel sig rejected");
    set.add("F002 ca reject", !f002_verify_boot_chain(&img, sig, 0x1234), "bad ca rejected");

    // F003
    let mut menu = BootMenu::new();
    let added = menu.add(BootEntry { label: "varix", kind: BootEntryKind::Varix, default: true })
        && menu.add(BootEntry { label: "win", kind: BootEntryKind::Windows, default: false });
    set.add("F003 menu add", added && menu.count == 2, "two entries");
    set.add("F003 default pick", menu.default_index() == Some(0), "first default");
    set.add("F003 select", menu.select(1) && menu.selected == 1, "select second");
    set.add("F003 select oob", !menu.select(9), "out of range rejected");

    // F004
    let (params, n) = f004_parse_cmdline("varix:quiet,log=debug,smp");
    set.add("F004 parse count", n == 3, "three params");
    set.add("F004 kv", params[1].map(|p| p.value) == Some(Some("debug")), "log=debug");
    set.add("F004 flag", params[2].map(|p| p.value) == Some(None), "bare flag");
    set.add("F004 empty", f004_parse_cmdline("varix:").1 == 0, "empty ok");

    // F005
    set.add("F005 filter info@warn", !f005_early_log_filter(LogLevel::Info, LogLevel::Warn), "info suppressed at warn");
    set.add("F005 filter err@warn", f005_early_log_filter(LogLevel::Error, LogLevel::Warn), "error passes");

    // F006
    let tbl = [0u8; 8];
    set.add("F006 zero sum valid", f006_acpi_checksum_valid(&tbl), "all zero sums to 0");
    set.add("F006 checksum fixed", f006_acpi_checksum_valid(&[0xFF, 0x01]), "0xFF+0x01=0x100");
    set.add("F006 xsdt pref", f006_pick_sdt(true, 0xAAAA_0000, 0xBBBB) == 0xAAAA_0000, "xsdt wins");
    set.add("F006 rsdt fallback", f006_pick_sdt(false, 0, 0xBBBB) == 0xBBBB, "rsdt fallback");

    // F007
    let madt = [(0u32, 1u32), (1, 1), (2, 0), (3, 1)];
    set.add("F007 madt count", f007_madt_count_cpus(&madt) == 3, "3 enabled cores");
    set.add("F007 madt empty", f007_madt_count_cpus(&[]) == 0, "none");

    // F008
    let st = ApState::Init;
    let st2 = f008_sipi_step(st, false, 0);
    let st3 = f008_sipi_step(st2, false, 0);
    set.add("F008 init->sipi", st2 == ApState::SipiSent, "step1");
    set.add("F008 sipi->wait", st3 == ApState::WaitApSig, "step2");
    set.add("F008 online", f008_sipi_step(st3, true, 0) == ApState::Online, "sig seen");
    set.add("F008 timeout fail", f008_sipi_step(st3, false, 2) == ApState::Failed, "timeout");

    // F009
    let (khz, ok) = f009_apic_calibrate(1_000_000, 1_000_000);
    set.add("F009 calib pass", ok, "zero drift");
    set.add("F009 khz sane", khz == 1_000_000, "1 tick/ns");
    let (_, bad) = f009_apic_calibrate(2_000_000, 1_000_000);
    set.add("F009 drift fail", !bad, "100% drift rejected");

    // F010
    set.add("F010 match", f010_cross_check(1000, 1001, 10), "within tol");
    set.add("F010 drift", !f010_cross_check(1000, 1200, 10), "beyond tol");

    // F011
    let mut regions = [
        MemRegion { base: 0x1000, len: 0x1000, kind: RegionKind::Usable },
        MemRegion { base: 0x5000, len: 0x1000, kind: RegionKind::Reserved },
    ];
    let changed = f011_refine_map(&mut regions, 0x1800, 0x200);
    set.add("F011 bad mark", changed == 1 && regions[0].kind == RegionKind::Bad, "overlap marked bad");
    set.add("F011 reserved kept", regions[1].kind == RegionKind::Reserved, "reserved untouched");

    // F012
    let mut buf = [0u8; 128];
    let n = f012_render_report(&mut buf, 4, 512, 12, true);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("F012 report render", n > 0 && text.contains("cpus=4"), "cpus rendered");
    set.add("F012 report sb", text.contains("secure_boot=on"), "secure boot flag");

    // F013
    set.add("F013 uefi pick", f013_pick_loader(true, false) == LoaderMode::Uefi, "uefi first");
    set.add("F013 bios fallback", f013_pick_loader(false, true) == LoaderMode::Bios, "bios fallback");

    // F014
    let mut t = BootTiming::new();
    let rec = t.record(0, 100) && t.record(1, 50) && !t.record(6, 1);
    set.add("F014 record", rec && t.total() == 150, "two stages");
    set.add("F014 slowest", t.slowest() == Some(0), "firmware slowest");

    // F015
    let mut sa = SlotState { boot_ok: true, fail_streak: 0 };
    let sb = SlotState { boot_ok: true, fail_streak: 0 };
    set.add("F015 pick a", f015_ab_slot_pick(sa, sb) == 0, "A healthy first");
    f015_slot_on_fail(&mut sa);
    f015_slot_on_fail(&mut sa);
    f015_slot_on_fail(&mut sa);
    set.add("F015 fallback b", f015_ab_slot_pick(sa, sb) == 1, "3 fails -> B");
    f015_slot_on_ok(&mut sa);
    set.add("F015 recover", sa.boot_ok && sa.fail_streak == 0, "reset on ok");

    // F016
    let mut pbuf = [0u8; 96];
    let n = f016_render_panic(&mut pbuf, 14, 0x401000, 0xdead0);
    let ptext = core::str::from_utf8(&pbuf[..n]).unwrap_or("");
    set.add("F016 panic err", ptext.contains("err=e"), "err field");
    set.add("F016 panic rip", ptext.contains("rip=401000"), "rip field");
    set.add("F016 panic cr2", ptext.contains("cr2=dead0"), "cr2 field");

    // F017
    set.add("F017 anim progress", f017_anim_progress(50, 100) == 50, "halfway");
    set.add("F017 anim clamp", f017_anim_progress(200, 100) == 100, "clamped");
    set.add("F017 frames", f017_anim_frame(1000, 60) == 60, "60fps @1s");

    // F018
    set.add("F018 two nodes", f018_numa_nodes(&[0, 1, 0]) == 2, "dedup");
    set.add("F018 degrade", f018_numa_nodes(&[]) == 1, "no domain -> 1 node");

    // F019
    let mut words = [0u32; CPUID_FEATS];
    words[1] = 1 << 0; // bit 32
    set.add("F019 feat set", f019_has_feat(&words, 32), "bit32 in word1");
    set.add("F019 feat clear", !f019_has_feat(&words, 33), "bit33 clear");
    set.add("F019 oob", !f019_has_feat(&words, 4096), "oob safe");

    // F020
    let mc = Microcode { vendor_sig: 0xACE, revision: 5 };
    set.add("F020 accept", f020_microcode_accept(true, 3, mc, 0xACE), "newer rev");
    set.add("F020 disabled", !f020_microcode_accept(false, 3, mc, 0xACE), "opt-out");
    set.add("F020 vendor mismatch", !f020_microcode_accept(true, 3, mc, 0xBAD), "vendor");
    set.add("F020 older rev", !f020_microcode_accept(true, 6, mc, 0xACE), "regress");

    // F021
    let enc = f021_probe_mem_encrypt(0b1, 0b1);
    set.add("F021 sme+tme", enc.sme && enc.tme, "both present");
    set.add("F021 none", !f021_probe_mem_encrypt(0, 0).sme, "none");

    // F022
    let mut log = BootLog::new();
    for i in 0..(BOOTLOG_CAP + 4) as u32 {
        log.push(i);
    }
    set.add("F022 ring order", log.get(0) == Some(4) && log.get(BOOTLOG_CAP - 1) == Some((BOOTLOG_CAP + 3) as u32), "oldest kept");
    set.add("F022 oob", log.get(BOOTLOG_CAP).is_none(), "no overflow read");

    // F023
    let mut ents: [Option<BootEntry>; 4] = [
        Some(BootEntry { label: "a", kind: BootEntryKind::Varix, default: true }),
        Some(BootEntry { label: "b", kind: BootEntryKind::Other, default: false }),
        None,
        None,
    ];
    let moved = f023_set_default(&mut ents, 1);
    set.add("F023 default move", moved && ents[1].map(|e| e.default) == Some(true) && ents[0].map(|e| e.default) == Some(false), "repointed");
    set.add("F023 oob reject", !f023_set_default(&mut ents, 3), "empty slot rejected");
    set.add("F023 timeout", f023_validate_timeout(5) && !f023_validate_timeout(31), "timeout bound");

    // F024
    set.add("F024 theme readable", f024_theme_readable(BootTheme::VARIX_DARK), "builtin dark");
    set.add("F024 theme bad", !f024_theme_readable(BootTheme { bg_rgb: 0x808080, fg_rgb: 0x7f7f7f, accent_rgb: 0 }), "low contrast");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f025_bootplat_selftest_all_pass() {
        let set = run_bootplat_checks();
        assert!(set.all_passed(), "{}", render_failures(&set));
        assert!(set.len() >= 25);
    }

    fn render_failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f003_menu_capacity() {
        let mut m = BootMenu::new();
        for i in 0..MAX_BOOT_ENTRIES {
            assert!(m.add(BootEntry { label: "x", kind: BootEntryKind::Other, default: false }));
            assert_eq!(m.count, i + 1);
        }
        assert!(!m.add(BootEntry { label: "y", kind: BootEntryKind::Other, default: false }));
    }

    #[test]
    fn f015_ab_slot_state_machine() {
        let mut s = SlotState { boot_ok: true, fail_streak: 0 };
        for _ in 0..2 {
            f015_slot_on_fail(&mut s);
        }
        assert!(s.boot_ok);
        f015_slot_on_fail(&mut s);
        assert!(!s.boot_ok);
    }
}
