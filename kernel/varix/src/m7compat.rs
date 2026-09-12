//! m7compat — VARIX-M700 AI-27 兼容与移植域 (F651~F675)
//!
//! ABI 冻结、多平台抽象、汇编最小化、字节序中立、对齐、移植侦察、
//! QEMU 档案、特性探测、回归矩阵、cfg 礼仪、老硬件降级、ABI 金样、
//! 兼容 fuzz、统计分账、事件流、移植健康分、剧本、自描述导出、
//! 压力剧本、回归走廊、文档、差异词典、预算官、自检入口、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F651 — ABI 冻结宪法
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum AbiState {
    Frozen,
    Draft,
    Deprecated,
}

pub fn abi_call_stable(s: AbiState) -> bool {
    matches!(s, AbiState::Frozen | AbiState::Deprecated)
}

// ===========================================================================
// F652 — 多平台抽象舱：arch 相关/无关分层
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum Platform {
    X86_64,
    Aarch64,
    Riscv64,
}

pub fn arch_specific_call_allowed(p: Platform, sym: &str) -> bool {
    match p {
        Platform::X86_64 => sym.starts_with("x86_"),
        Platform::Aarch64 => sym.starts_with("a64_"),
        Platform::Riscv64 => sym.starts_with("rv64_"),
    }
}

// ===========================================================================
// F653 — 汇编最小化律
// ===========================================================================

pub const ASM_SITES_MAX: usize = 12;

pub struct AsmInventory {
    pub sites: usize,
}

impl AsmInventory {
    pub fn within_budget(&self) -> bool {
        self.sites <= ASM_SITES_MAX
    }
}

// ===========================================================================
// F654 — 字节序中立官
// ===========================================================================

pub fn to_be32(v: u32) -> u32 {
    v.to_be()
}

pub fn from_be32(v: u32) -> u32 {
    u32::from_be(v)
}

pub fn little_endian_store(buf: &mut [u8], off: usize, v: u32) -> bool {
    if off + 4 > buf.len() {
        return false;
    }
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    true
}

pub fn little_endian_load(buf: &[u8], off: usize) -> Option<u32> {
    if off + 4 > buf.len() {
        return None;
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[off..off + 4]);
    Some(u32::from_le_bytes(b))
}

// ===========================================================================
// F655 — 对齐自觉官
// ===========================================================================

pub fn aligned(addr: u64, align: u64) -> bool {
    align.is_power_of_two() && addr & (align - 1) == 0
}

pub fn align_up(addr: u64, align: u64) -> u64 {
    if !align.is_power_of_two() {
        return addr;
    }
    (addr + align - 1) & !(align - 1)
}

// ===========================================================================
// F656 — 移植侦察包：差距清单
// ===========================================================================

pub const PORT_GAPS: [&str; 6] =
    ["context-switch-asm", "page-table-fmt", "boot-protocol", "uart-regs", "timer-regs", "tlb-ops"];

pub fn port_gap_known(name: &str) -> bool {
    PORT_GAPS.iter().any(|g| *g == name)
}

pub fn port_readiness(closed_gaps: u8) -> u16 {
    ((closed_gaps as u32 * 1000) / PORT_GAPS.len() as u32) as u16
}

// ===========================================================================
// F657 — QEMU 多机档案
// ===========================================================================

pub const QEMU_MACHINES: [&str; 4] = ["q35", "microvm", "virt-aarch64", "virt-riscv64"];

pub fn qemu_machine_known(name: &str) -> bool {
    QEMU_MACHINES.iter().any(|m| *m == name)
}

// ===========================================================================
// F658 — 特性探测官：运行时分派
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum CpuFeature {
    Sse4_2,
    Avx2,
    Smap,
    Rdtsc,
}

pub fn feature_bit(f: CpuFeature) -> u32 {
    match f {
        CpuFeature::Rdtsc => 1 << 0,
        CpuFeature::Sse4_2 => 1 << 1,
        CpuFeature::Avx2 => 1 << 2,
        CpuFeature::Smap => 1 << 3,
    }
}

pub fn has_feature(present_mask: u32, f: CpuFeature) -> bool {
    present_mask & feature_bit(f) != 0
}

/// 分派：有 AVX2 走快路径，否则基础路径。
pub fn dispatch_simd(present_mask: u32) -> &'static str {
    if has_feature(present_mask, CpuFeature::Avx2) {
        "avx2-path"
    } else if has_feature(present_mask, CpuFeature::Sse4_2) {
        "sse42-path"
    } else {
        "scalar-path"
    }
}

// ===========================================================================
// F659 — 兼容回归矩阵
// ===========================================================================

pub const CONFIG_MATRIX: [&str; 4] = ["debug", "release", "lto", "no-fp"];

pub fn matrix_cells(machines: usize, configs: usize) -> usize {
    machines * configs
}

pub fn matrix_complete(passed: usize, total: usize) -> bool {
    total > 0 && passed >= total
}

// ===========================================================================
// F660 — 条件编译礼仪
// ===========================================================================

pub const CFG_ALLOWED_KEYS: [&str; 5] = ["target_arch", "target_os", "feature", "test", "cfg"];

pub fn cfg_key_allowed(key: &str) -> bool {
    CFG_ALLOWED_KEYS.iter().any(|k| *k == key)
}

pub fn cfg_audit(count_per_fn: usize) -> bool {
    count_per_fn <= 3 // 单函数内 cfg 块上限
}

// ===========================================================================
// F661 — 老硬件降级谱
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum HardwareClass {
    Modern,
    Legacy,
    Ancient,
}

pub fn degradation_level(hw: HardwareClass, feat: CpuFeature, present_mask: u32) -> u8 {
    match hw {
        HardwareClass::Modern => if has_feature(present_mask, feat) { 0 } else { 1 },
        HardwareClass::Legacy => 2,
        HardwareClass::Ancient => 3,
    }
}

// ===========================================================================
// F662 — ABI 金样本
// ===========================================================================

pub struct AbiSample {
    pub call_no: u32,
    pub arg_layout_hash: u64,
}

pub fn abi_sample_matches(a: &AbiSample, golden: &AbiSample) -> bool {
    a.call_no == golden.call_no && a.arg_layout_hash == golden.arg_layout_hash
}

// ===========================================================================
// F663 — 兼容 fuzz 桩：跨配置
// ===========================================================================

pub fn compat_fuzz_ok(config_id: u8, seed: u64) -> bool {
    (config_id as usize) < CONFIG_MATRIX.len() && seed != 0
}

// ===========================================================================
// F664 — 兼容统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ConfigStat {
    pub config: u8,
    pub passed: u32,
    pub failed: u32,
}

impl ConfigStat {
    pub fn pass_permille(&self) -> u16 {
        let t = self.passed + self.failed;
        if t == 0 {
            return 0;
        }
        ((self.passed as u64 * 1000) / t as u64) as u16
    }
}

// ===========================================================================
// F665 — 兼容事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum CompatEvent {
    ConfigPass,
    ConfigFail,
    FeatureDetected,
    Degraded,
}

pub fn compat_event_loggable(e: CompatEvent) -> bool {
    matches!(e, CompatEvent::ConfigPass | CompatEvent::ConfigFail | CompatEvent::FeatureDetected | CompatEvent::Degraded)
}

// ===========================================================================
// F666 — 移植健康分
// ===========================================================================

pub struct PortHealth {
    pub gaps_closed: u8,
    pub qemu_machines_pass: u8,
}

impl PortHealth {
    pub fn grade(&self) -> u8 {
        let g = self.gaps_closed as usize;
        let m = self.qemu_machines_pass as usize;
        if g == PORT_GAPS.len() && m >= QEMU_MACHINES.len() {
            0
        } else if g >= PORT_GAPS.len() / 2 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F667 — 移植剧本
// ===========================================================================

pub const PORT_STEPS: [&str; 6] =
    ["audit-gaps", "asm-minimize", "boot-bringup", "mmu-bringup", "timer-bringup", "regression-matrix"];

pub fn port_step_known(name: &str) -> bool {
    PORT_STEPS.iter().any(|s| *s == name)
}

// ===========================================================================
// F668 — 兼容自描述导出
// ===========================================================================

pub struct CompatExport {
    pub platform: Platform,
    pub feature_mask: u32,
    pub config_id: u8,
}

pub fn compat_export_valid(e: &CompatExport) -> bool {
    (e.config_id as usize) < CONFIG_MATRIX.len() && e.feature_mask != 0
}

// ===========================================================================
// F669 — 兼容压力剧本
// ===========================================================================

pub const COMPAT_STRESS_PLAYS: [&str; 4] = ["matrix-4x4", "feature-sweep", "align-chaos", "endianness-flip"];

pub fn compat_stress_known(name: &str) -> bool {
    COMPAT_STRESS_PLAYS.iter().any(|p| *p == name)
}

// ===========================================================================
// F670 — 兼容回归走廊
// ===========================================================================

pub const COMPAT_CORRIDOR_CASES: [&str; 5] =
    ["abi-frozen-calls", "endian-roundtrip", "align-guarantees", "feature-dispatch", "cfg-audit"];

pub fn compat_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F671 — 兼容文档生成器
// ===========================================================================

pub const PORT_DOC_SECTIONS: [&str; 5] = ["gap-list", "asm-notes", "bringup-order", "qemu-map", "checklist"];

pub fn port_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << PORT_DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F672 — 平台差异词典
// ===========================================================================

pub const PLATFORM_DIFFS: [&str; 5] =
    ["page-size-4k-vs-16k", "syscall-calling-conv", "tls-register", "timer-frequency", "mmu-format"];

pub fn platform_diff_documented(name: &str) -> bool {
    PLATFORM_DIFFS.iter().any(|d| *d == name)
}

// ===========================================================================
// F673 — 兼容预算官：抽象层开销
// ===========================================================================

pub const ABSTRACTION_OVERHEAD_CAP_PERMILLE: u16 = 20; // 2%

pub fn abstraction_overhead_ok(direct_ns: u32, via_layer_ns: u32) -> bool {
    if direct_ns == 0 {
        return true;
    }
    via_layer_ns as u64 * 1000 <= direct_ns as u64 * (1000 + ABSTRACTION_OVERHEAD_CAP_PERMILLE as u64)
}

// ===========================================================================
// F674 — 兼容自检入口
// ===========================================================================

pub const COMPAT_SELFTESTS: [&str; 4] = ["endian", "align", "abi-hash", "feature-mask"];

pub fn compat_selftest_known(name: &str) -> bool {
    COMPAT_SELFTESTS.iter().any(|s| *s == name)
}

// ===========================================================================
// F675 — 兼容域年报
// ===========================================================================

pub struct CompatYearbook {
    pub configs_tested: u32,
    pub machines_tested: u32,
    pub abi_breaks: u32,
}

impl CompatYearbook {
    pub fn clean(&self) -> bool {
        self.abi_breaks == 0
    }
    pub fn coverage_cells(&self) -> usize {
        (self.configs_tested as usize) * (self.machines_tested as usize)
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7compat_checks() -> CheckSet {
    let mut set = CheckSet::new("m7compat");

    // F651 ABI
    set.add(
        "F651 abi",
        abi_call_stable(AbiState::Frozen) && abi_call_stable(AbiState::Deprecated)
            && !abi_call_stable(AbiState::Draft),
        "frozen/deprecated only",
    );

    // F652 抽象舱
    set.add(
        "F652 arch layering",
        arch_specific_call_allowed(Platform::X86_64, "x86_wrmsr")
            && !arch_specific_call_allowed(Platform::X86_64, "a64_wrmsr"),
        "prefix discipline",
    );

    // F653 汇编
    set.add(
        "F653 asm minimal",
        AsmInventory { sites: 8 }.within_budget() && !AsmInventory { sites: 13 }.within_budget(),
        "12 site cap",
    );

    // F654 字节序
    let mut buf = [0u8; 8];
    little_endian_store(&mut buf, 0, 0x1234_5678);
    set.add(
        "F654 endian",
        to_be32(1) != 1
            && from_be32(to_be32(0xABCD)) == 0xABCD
            && little_endian_load(&buf, 0) == Some(0x1234_5678)
            && little_endian_load(&buf, 5).is_none(),
        "roundtrip + bounds",
    );

    // F655 对齐
    set.add(
        "F655 align",
        aligned(4096, 4096) && !aligned(4097, 4096)
            && align_up(4097, 4096) == 8192 && align_up(4096, 4096) == 4096
            && !aligned(5, 3), // 非幂次对齐非法
        "pow2 rules",
    );

    // F656 侦察包
    set.add(
        "F656 port recon",
        port_gap_known("context-switch-asm") && port_readiness(3) == 500,
        "6 gaps",
    );

    // F657 QEMU 档案
    set.add(
        "F657 qemu map",
        qemu_machine_known("q35") && qemu_machine_known("virt-riscv64") && !qemu_machine_known("pc98"),
        "4 machines",
    );

    // F658 特性探测
    let mask = feature_bit(CpuFeature::Rdtsc) | feature_bit(CpuFeature::Sse4_2);
    set.add(
        "F658 features",
        has_feature(mask, CpuFeature::Sse4_2) && !has_feature(mask, CpuFeature::Avx2)
            && dispatch_simd(mask) == "sse42-path"
            && dispatch_simd(0) == "scalar-path",
        "dispatch ladder",
    );

    // F659 矩阵
    set.add(
        "F659 matrix",
        matrix_cells(4, 4) == 16 && matrix_complete(16, 16) && !matrix_complete(15, 16),
        "4x4 cells",
    );

    // F660 cfg 礼仪
    set.add(
        "F660 cfg",
        cfg_key_allowed("target_arch") && !cfg_key_allowed("rustc_version")
            && cfg_audit(3) && !cfg_audit(4),
        "keys + count",
    );

    // F661 降级谱
    set.add(
        "F661 degrade",
        degradation_level(HardwareClass::Modern, CpuFeature::Avx2, feature_bit(CpuFeature::Avx2)) == 0
            && degradation_level(HardwareClass::Modern, CpuFeature::Avx2, 0) == 1
            && degradation_level(HardwareClass::Ancient, CpuFeature::Avx2, feature_bit(CpuFeature::Avx2)) == 3,
        "class ladder",
    );

    // F662 ABI 金样
    let g = AbiSample { call_no: 7, arg_layout_hash: 0xDEAD };
    set.add(
        "F662 abi golden",
        abi_sample_matches(&g, &g)
            && !abi_sample_matches(&g, &AbiSample { call_no: 7, arg_layout_hash: 0xBEEF }),
        "layout hash",
    );

    // F663 fuzz 桩
    set.add(
        "F663 compat fuzz",
        compat_fuzz_ok(0, 42) && !compat_fuzz_ok(9, 42) && !compat_fuzz_ok(0, 0),
        "config + seed",
    );

    // F664 分账
    let st = ConfigStat { config: 1, passed: 95, failed: 5 };
    set.add("F664 stats", st.pass_permille() == 950, "pass rate");

    // F665 事件流
    set.add(
        "F665 events",
        compat_event_loggable(CompatEvent::Degraded) && compat_event_loggable(CompatEvent::ConfigFail),
        "4 events",
    );

    // F666 移植健康
    let h = PortHealth { gaps_closed: 6, qemu_machines_pass: 4 };
    let hb = PortHealth { gaps_closed: 2, qemu_machines_pass: 1 };
    set.add("F666 health", h.grade() == 0 && hb.grade() == 2, "gap/machine grade");

    // F667 剧本
    set.add(
        "F667 steps",
        port_step_known("mmu-bringup") && !port_step_known("wing-it"),
        "6 steps",
    );

    // F668 导出
    let ex = CompatExport { platform: Platform::X86_64, feature_mask: 0xF, config_id: 2 };
    set.add(
        "F668 export",
        compat_export_valid(&ex) && !compat_export_valid(&CompatExport { platform: Platform::X86_64, feature_mask: 0, config_id: 2 }),
        "mask + config",
    );

    // F669 压力
    set.add(
        "F669 stress",
        compat_stress_known("endianness-flip") && !compat_stress_known("random"),
        "4 plays",
    );

    // F670 走廊
    set.add(
        "F670 corridor",
        compat_corridor_pass(&[true; 5]) && !compat_corridor_pass(&[true, true, false, true, true]),
        "5 cases",
    );

    // F671 文档
    set.add(
        "F671 docs",
        port_doc_complete(0b1_1111) && !port_doc_complete(0b1_1101),
        "5 sections",
    );

    // F672 差异词典
    set.add(
        "F672 diffs",
        platform_diff_documented("page-size-4k-vs-16k") && !platform_diff_documented("magic"),
        "5 diffs",
    );

    // F673 预算
    set.add(
        "F673 overhead budget",
        abstraction_overhead_ok(100, 101) && !abstraction_overhead_ok(100, 103),
        "2% cap",
    );

    // F674 自检
    set.add(
        "F674 selftest",
        compat_selftest_known("endian") && compat_selftest_known("abi-hash"),
        "4 selftests",
    );

    // F675 年报
    let yb = CompatYearbook { configs_tested: 4, machines_tested: 4, abi_breaks: 0 };
    set.add(
        "F675 yearbook",
        yb.clean() && yb.coverage_cells() == 16,
        "clean + cells",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f654_le_roundtrip_many() {
        for v in [0u32, 1, 0x8000_0000, 0xFFFF_FFFF] {
            let mut b = [0u8; 4];
            assert!(little_endian_store(&mut b, 0, v));
            assert_eq!(little_endian_load(&b, 0), Some(v));
        }
    }

    #[test]
    fn f655_align_up_edges() {
        assert_eq!(align_up(0, 8), 0);
        assert_eq!(align_up(1, 8), 8);
        assert_eq!(align_up(9, 8), 16);
    }

    #[test]
    fn f658_all_features_dispatch() {
        let all = 0xF;
        assert_eq!(dispatch_simd(all), "avx2-path");
    }

    #[test]
    fn f675_domain_selfcheck_all_pass() {
        let set = run_m7compat_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
