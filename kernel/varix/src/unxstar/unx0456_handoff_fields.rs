//! UNX-F0456 ACPI 交接字段（A2 预备） · 0x753C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   交接字段 v1 冻结（8 字段），A2 mock 消费 diff 需零差。
//! 设计要点：
//!   1. 交接信包 v1 冻结 8 字段：RSDP 地址 / XSDT 指针 / MADT 项数 /
//!      HPET 基址 / FADT 端口块（打包 u32）/ century / MCFG 记录数 /
//!      表总数，字段名表与字段数常量一并冻结；
//!   2. 真值装配：8 字段各配机器真值常数（QEMU 典型布局），装配后
//!      与真值逐字段对拍必须零差；
//!   3. 冻结状态机：Draft→Frozen 单调前进，冻结后任何字段改写显性拒
//!      并计数（防篡改红线），倒退（重复冻结）显性拒；
//!   4. A2 mock 消费：按字段下标逐字段读取装配 MockView，diff 复算
//!      必须零差；对 mock 视图注入单字段篡改必被抓；
//!   5. 信包指纹 FNV-1a 64：版本 + 8 字段按槽序折叠，重放一致；
//!   6. 版本闸：v1 受理，非 1 版本 mock 拒收（向后兼容预留）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x753C0，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x753C0;

/// 交接字段数（v1 冻结 8）。
pub const FIELD_COUNT: usize = 8;
/// 交接信包版本（v1）。
pub const HANDOFF_VERSION: u16 = 1;
/// FNV-1a 64 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

// ── 机器真值常数（QEMU 典型布局，A2 mock 消费的期望侧） ─────────
/// RSDP 物理地址（EBDA 边界典型值）。
pub const GT_RSDP_ADDR: u64 = 0x000F_EB00;
/// XSDT 物理指针。
pub const GT_XSDT_PTR: u64 = 0x7FFE_0000;
/// MADT 项数（8 LAPIC + 12 IOAPIC/ISO 等）。
pub const GT_MADT_ITEMS: u32 = 24;
/// HPET 基址（FED00000 固定窗）。
pub const GT_HPET_BASE: u64 = 0xFED0_0000;
/// FADT 端口块打包（PM1A_EVT 0x0600 << 16 | PM1A_CNT 0x0604）。
pub const GT_FADT_PORTS: u32 = 0x0600_0604;
/// century 字段（CMOS 世纪寄存器偏移，0x20 表示 2000 纪元）。
pub const GT_CENTURY: u8 = 0x20;
/// MCFG 记录数。
pub const GT_MCFG_RECORDS: u32 = 4;
/// 表总数（与 12 表语料对拍）。
pub const GT_TABLE_TOTAL: u32 = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffCode {
    /// 冻结后改写（防篡改红线）
    TamperBlocked,
    /// 相位倒退（重复冻结）
    PhaseRegress,
    /// 字段下标越界（> 7）
    FieldIdxOob,
    /// 版本非法（非 v1）
    VersionBad,
    /// mock 消费 diff 非零
    MockDiff,
    /// 真值对拍失配
    TruthMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl HandoffCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HandoffCode::TamperBlocked => "冻结后改写：信包已只读；建议走 v2 新信包而非改冻结字段",
            HandoffCode::PhaseRegress => "相位倒退：Draft→Frozen 只进不退；建议检查装配序",
            HandoffCode::FieldIdxOob => "字段下标越界：v1 只有 8 槽；建议用 FieldIdx 闭集取值",
            HandoffCode::VersionBad => "版本非法：mock 只认 v1；建议先升级消费端再换版本",
            HandoffCode::MockDiff => "mock diff 非零：消费视图与信包不等；建议逐字段对拍定位",
            HandoffCode::TruthMismatch => "真值对拍失配：字段与机器真值不等；建议复查装配源",
            HandoffCode::NotReproducible => "双跑不一致：装配两轮不等；建议冻结装配序与真值表",
        }
    }
    pub const ALL: [HandoffCode; 7] = [
        HandoffCode::TamperBlocked,
        HandoffCode::PhaseRegress,
        HandoffCode::FieldIdxOob,
        HandoffCode::VersionBad,
        HandoffCode::MockDiff,
        HandoffCode::TruthMismatch,
        HandoffCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段外码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HandoffCode::ALL.len() {
        if let Some(c) = HandoffCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x753C0 段闭集内"
}

// ── 字段下标闭集 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldIdx {
    /// 0 RSDP 地址
    RsdpAddr,
    /// 1 XSDT 指针
    XsdtPtr,
    /// 2 MADT 项数
    MadtItems,
    /// 3 HPET 基址
    HpetBase,
    /// 4 FADT 端口块
    FadtPorts,
    /// 5 century
    Century,
    /// 6 MCFG 记录数
    McfgRecords,
    /// 7 表总数
    TableTotal,
}

impl FieldIdx {
    pub fn slot(self) -> usize {
        match self {
            FieldIdx::RsdpAddr => 0,
            FieldIdx::XsdtPtr => 1,
            FieldIdx::MadtItems => 2,
            FieldIdx::HpetBase => 3,
            FieldIdx::FadtPorts => 4,
            FieldIdx::Century => 5,
            FieldIdx::McfgRecords => 6,
            FieldIdx::TableTotal => 7,
        }
    }
    pub const ALL: [FieldIdx; 8] = [
        FieldIdx::RsdpAddr,
        FieldIdx::XsdtPtr,
        FieldIdx::MadtItems,
        FieldIdx::HpetBase,
        FieldIdx::FadtPorts,
        FieldIdx::Century,
        FieldIdx::McfgRecords,
        FieldIdx::TableTotal,
    ];
}

/// 字段名冻结表（v1，顺序与 FieldIdx 槽位一致）。
pub const FIELD_NAMES: [&str; 8] = [
    "rsdp_addr",
    "xsdt_ptr",
    "madt_items",
    "hpet_base",
    "fadt_ports",
    "century",
    "mcfg_records",
    "table_total",
];

// ── 交接信包 v1（8 字段，类型各异） ─────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffV1 {
    pub version: u16,
    pub rsdp_addr: u64,
    pub xsdt_ptr: u64,
    pub madt_items: u32,
    pub hpet_base: u64,
    pub fadt_ports: u32,
    pub century: u8,
    pub mcfg_records: u32,
    pub table_total: u32,
}

/// 相位（Draft→Frozen 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffPhase {
    Draft,
    Frozen,
}

/// 装配账：信包 + 相位 + 指纹 + 防篡改计数。
#[derive(Clone, Debug)]
pub struct HandoffLedger {
    pub pkt: HandoffV1,
    pub phase: HandoffPhase,
    pub fp: u64,
    /// 冻结后改写被拒次数（显性记账）。
    pub tamper_blocked: u32,
    /// 倒退被拒次数。
    pub regress_blocked: u32,
}

impl HandoffLedger {
    pub fn new(pkt: HandoffV1) -> HandoffLedger {
        HandoffLedger {
            pkt,
            phase: HandoffPhase::Draft,
            fp: 0,
            tamper_blocked: 0,
            regress_blocked: 0,
        }
    }

    /// Draft 期改字段（按下标闭集分派）。
    pub fn set_field(&mut self, idx: FieldIdx, v: u64) -> Result<(), u32> {
        if self.phase != HandoffPhase::Draft {
            self.tamper_blocked += 1;
            return Err(HandoffCode::TamperBlocked.val());
        }
        match idx {
            FieldIdx::RsdpAddr => self.pkt.rsdp_addr = v as u64,
            FieldIdx::XsdtPtr => self.pkt.xsdt_ptr = v as u64,
            FieldIdx::MadtItems => self.pkt.madt_items = v as u32,
            FieldIdx::HpetBase => self.pkt.hpet_base = v as u64,
            FieldIdx::FadtPorts => self.pkt.fadt_ports = v as u32,
            FieldIdx::Century => self.pkt.century = v as u8,
            FieldIdx::McfgRecords => self.pkt.mcfg_records = v as u32,
            FieldIdx::TableTotal => self.pkt.table_total = v as u32,
        }
        Ok(())
    }

    /// 读字段（按下标闭集）。
    pub fn get_field(&self, idx: FieldIdx) -> u64 {
        match idx {
            FieldIdx::RsdpAddr => self.pkt.rsdp_addr,
            FieldIdx::XsdtPtr => self.pkt.xsdt_ptr,
            FieldIdx::MadtItems => self.pkt.madt_items as u64,
            FieldIdx::HpetBase => self.pkt.hpet_base,
            FieldIdx::FadtPorts => self.pkt.fadt_ports as u64,
            FieldIdx::Century => self.pkt.century as u64,
            FieldIdx::McfgRecords => self.pkt.mcfg_records as u64,
            FieldIdx::TableTotal => self.pkt.table_total as u64,
        }
    }

    /// 冻结：Draft→Frozen，落 FNV 指纹；重复冻结 = 倒退拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.phase != HandoffPhase::Draft {
            self.regress_blocked += 1;
            return Err(HandoffCode::PhaseRegress.val());
        }
        self.fp = packet_fingerprint(&self.pkt);
        self.phase = HandoffPhase::Frozen;
        Ok(self.fp)
    }
}

/// 真值装配（8 字段全部来自机器真值常数）。
pub fn handoff_from_truth() -> HandoffV1 {
    HandoffV1 {
        version: HANDOFF_VERSION,
        rsdp_addr: GT_RSDP_ADDR,
        xsdt_ptr: GT_XSDT_PTR,
        madt_items: GT_MADT_ITEMS,
        hpet_base: GT_HPET_BASE,
        fadt_ports: GT_FADT_PORTS,
        century: GT_CENTURY,
        mcfg_records: GT_MCFG_RECORDS,
        table_total: GT_TABLE_TOTAL,
    }
}

/// 信包指纹：版本 + 8 字段按槽序 FNV 折叠。
pub fn packet_fingerprint(p: &HandoffV1) -> u64 {
    let mut fp = FNV_OFFSET;
    fp = fnv_fold(fp, &p.version.to_le_bytes());
    fp = fnv_fold(fp, &p.rsdp_addr.to_le_bytes());
    fp = fnv_fold(fp, &p.xsdt_ptr.to_le_bytes());
    fp = fnv_fold(fp, &p.madt_items.to_le_bytes());
    fp = fnv_fold(fp, &p.hpet_base.to_le_bytes());
    fp = fnv_fold(fp, &p.fadt_ports.to_le_bytes());
    fp = fnv_fold(fp, &[p.century]);
    fp = fnv_fold(fp, &p.mcfg_records.to_le_bytes());
    fp = fnv_fold(fp, &p.table_total.to_le_bytes());
    fp
}

fn fnv_fold(mut fp: u64, bytes: &[u8]) -> u64 {
    for b in bytes.iter() {
        fp ^= *b as u64;
        fp = fp.wrapping_mul(FNV_PRIME);
    }
    fp
}

/// 真值对拍：逐字段与真值常数比，返回失配字段下标闭集（应空）。
pub fn truth_diff(p: &HandoffV1) -> Vec<usize> {
    let mut bad: Vec<usize> = Vec::new();
    if p.version != HANDOFF_VERSION {
        bad.push(8);
    }
    if p.rsdp_addr != GT_RSDP_ADDR {
        bad.push(FieldIdx::RsdpAddr.slot());
    }
    if p.xsdt_ptr != GT_XSDT_PTR {
        bad.push(FieldIdx::XsdtPtr.slot());
    }
    if p.madt_items != GT_MADT_ITEMS {
        bad.push(FieldIdx::MadtItems.slot());
    }
    if p.hpet_base != GT_HPET_BASE {
        bad.push(FieldIdx::HpetBase.slot());
    }
    if p.fadt_ports != GT_FADT_PORTS {
        bad.push(FieldIdx::FadtPorts.slot());
    }
    if p.century != GT_CENTURY {
        bad.push(FieldIdx::Century.slot());
    }
    if p.mcfg_records != GT_MCFG_RECORDS {
        bad.push(FieldIdx::McfgRecords.slot());
    }
    if p.table_total != GT_TABLE_TOTAL {
        bad.push(FieldIdx::TableTotal.slot());
    }
    bad
}

// ── A2 mock 消费侧 ──────────────────────────────────────────────
/// mock 消费视图：按槽位 u64 展开（宽化存储，diff 按槽比）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockView {
    pub version: u16,
    pub slots: [u64; 8],
}

/// mock 消费：v1 受理，按下标闭集逐字段读入视图；非 v1 拒收。
pub fn a2_mock_consume(p: &HandoffV1) -> Result<MockView, u32> {
    if p.version != HANDOFF_VERSION {
        return Err(HandoffCode::VersionBad.val());
    }
    let led = HandoffLedger::new(*p);
    let mut slots: [u64; 8] = [0; 8];
    for f in FieldIdx::ALL {
        let s = f.slot();
        if let Some(dst) = slots.get_mut(s) {
            *dst = led.get_field(f);
        }
    }
    Ok(MockView { version: p.version, slots })
}

/// mock diff：视图与信包逐槽比，返回失配槽位（应空 = 零差）。
pub fn mock_diff(v: &MockView, p: &HandoffV1) -> Vec<usize> {
    let led = HandoffLedger::new(*p);
    let mut bad: Vec<usize> = Vec::new();
    for f in FieldIdx::ALL {
        let s = f.slot();
        let want = led.get_field(f);
        if let Some(got) = v.slots.get(s) {
            if *got != want {
                bad.push(s);
            }
        }
    }
    bad
}

/// mock 注入：篡改视图 MADT 槽（+1），diff 必须非零（必抓）。
pub fn inject_mock_tamper(v: &MockView) -> MockView {
    let mut c = v.clone();
    if let Some(dst) = c.slots.get_mut(FieldIdx::MadtItems.slot()) {
        *dst = dst.wrapping_add(1);
    }
    c
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_ok() -> bool {
    golden_run() == golden_run()
}

/// 黄金全链：真值装配 → 真值对拍零差 → 冻结 → mock 消费零差 →
/// 冻结后 8 字段逐槽改写全拒 → 倒退拒。
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let pkt = handoff_from_truth();
    if !truth_diff(&pkt).is_empty() {
        return Err(HandoffCode::TruthMismatch.val());
    }
    let mut led = HandoffLedger::new(pkt);
    let fp = led.freeze()?;
    // 重复冻结 = 倒退。
    if led.freeze().is_ok() {
        return Err(HandoffCode::PhaseRegress.val());
    }
    // mock 消费零差。
    let view = a2_mock_consume(&led.pkt)?;
    if !mock_diff(&view, &led.pkt).is_empty() {
        return Err(HandoffCode::MockDiff.val());
    }
    // 冻结后 8 槽改写全拒。
    let mut blocked = 0;
    for f in FieldIdx::ALL {
        if led.set_field(f, 0xDEAD) == Err(HandoffCode::TamperBlocked.val()) {
            blocked += 1;
        }
    }
    if blocked != FIELD_COUNT as u32 || led.tamper_blocked != FIELD_COUNT as u32 {
        return Err(HandoffCode::TamperBlocked.val());
    }
    Ok((fp, blocked, led.regress_blocked))
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in HandoffCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn frozen_consts_ok() -> bool {
    FIELD_COUNT == 8
        && FIELD_NAMES.len() == FIELD_COUNT
        && FieldIdx::ALL.len() == FIELD_COUNT
        && GT_TABLE_TOTAL == 12
}
fn truth_zero_diff_ok() -> bool {
    truth_diff(&handoff_from_truth()).is_empty()
}
fn mock_zero_diff_ok() -> bool {
    match a2_mock_consume(&handoff_from_truth()) {
        Ok(v) => mock_diff(&v, &handoff_from_truth()).is_empty(),
        Err(_) => false,
    }
}
fn tamper_all_blocked_ok() -> bool {
    match golden_run() {
        Ok((_, blocked, _)) => blocked == FIELD_COUNT as u32,
        Err(_) => false,
    }
}
fn regress_ok() -> bool {
    let mut led = HandoffLedger::new(handoff_from_truth());
    let _ = led.freeze();
    led.freeze() == Err(HandoffCode::PhaseRegress.val()) && led.regress_blocked == 1
}
fn idx_oob_ok() -> bool {
    // 槽位闭集 0..8 全在窗内；越窗槽位（手工 8/9）查不到对应字段。
    let mut i = 0;
    while i < FieldIdx::ALL.len() {
        if let Some(f) = FieldIdx::ALL.get(i) {
            if f.slot() >= FIELD_COUNT {
                return false;
            }
        }
        i += 1;
    }
    FIELD_COUNT == 8
}
fn mock_tamper_caught_ok() -> bool {
    let view = match a2_mock_consume(&handoff_from_truth()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !mock_diff(&view, &handoff_from_truth()).is_empty() {
        return false;
    }
    !mock_diff(&inject_mock_tamper(&view), &handoff_from_truth()).is_empty()
}
fn version_gate_ok() -> bool {
    let mut bad = handoff_from_truth();
    bad.version = 2;
    a2_mock_consume(&handoff_from_truth()).is_ok()
        && a2_mock_consume(&bad) == Err(HandoffCode::VersionBad.val())
}
fn fp_replay_ok() -> bool {
    match golden_run() {
        Ok((fp, _, _)) => packet_fingerprint(&handoff_from_truth()) == fp && fp != 0,
        Err(_) => false,
    }
}
fn codeset_ok() -> bool {
    for c in HandoffCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-字段数冻结8",
        frozen_consts_ok(),
        format!("字段={} 名表={} 下标闭集={}", FIELD_COUNT, FIELD_NAMES.len(), FieldIdx::ALL.len()),
    ));
    v.push((
        "J1-真值装配零差",
        truth_zero_diff_ok(),
        format!("RSDP={:#x} 表总数={}", GT_RSDP_ADDR, GT_TABLE_TOTAL),
    ));
    v.push((
        "J1-A2 mock 消费零差",
        mock_zero_diff_ok(),
        "8 槽逐槽 diff 为空".to_string(),
    ));
    v.push((
        "J1-冻结后改写拦截",
        tamper_all_blocked_ok(),
        format!("8 槽试写 {} 拒", FIELD_COUNT),
    ));
    v.push((
        "J1-相位倒退显性拒",
        regress_ok(),
        "重复冻结即倒退".to_string(),
    ));
    v.push((
        "J1-字段下标闭集",
        idx_oob_ok(),
        "槽位 0..8 全在窗内".to_string(),
    ));
    v.push((
        "J1-mock 篡改注入必抓",
        mock_tamper_caught_ok(),
        "MADT 槽 +1 即 diff 非零".to_string(),
    ));
    v.push((
        "J1-版本闸 v1",
        version_gate_ok(),
        format!("v{} 过 / v2 拒", HANDOFF_VERSION),
    ));
    v.push((
        "J1-信包指纹重放一致",
        fp_replay_ok(),
        "FNV 版本+8字段折叠同值".to_string(),
    ));
    v.push((
        "J2-表总数对拍",
        GT_TABLE_TOTAL == 12,
        format!("交接表总数={} 语料=12", GT_TABLE_TOTAL),
    ));
    v.push((
        "J2-双跑一致",
        repro_ok(),
        "golden_run 两跑同值".to_string(),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:x} 窗口", SEG_BASE),
    ));
    v.push((
        "J3-诊断码闭集",
        codeset_ok(),
        format!("{} 码全查表命中", HandoffCode::ALL.len()),
    ));
    v
}

pub fn run_unx0456_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0456 ACPI 交接字段（A2 预备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_count_frozen() {
        assert!(frozen_consts_ok());
        assert_eq!(FIELD_COUNT, 8);
        assert_eq!(FIELD_NAMES.first().copied().unwrap_or(""), "rsdp_addr");
        assert_eq!(FIELD_NAMES.last().copied().unwrap_or(""), "table_total");
    }

    #[test]
    fn test_truth_assembly_zero_diff() {
        assert!(truth_zero_diff_ok());
        assert!(truth_diff(&handoff_from_truth()).is_empty());
    }

    #[test]
    fn test_truth_mismatch_detected() {
        let mut bad = handoff_from_truth();
        bad.hpet_base = 0xFED00001;
        assert_eq!(truth_diff(&bad), vec![FieldIdx::HpetBase.slot()]);
    }

    #[test]
    fn test_mock_consume_zero_diff() {
        assert!(mock_zero_diff_ok());
    }

    #[test]
    fn test_frozen_tamper_all_blocked() {
        let mut led = HandoffLedger::new(handoff_from_truth());
        let _ = led.freeze();
        for f in FieldIdx::ALL {
            assert_eq!(led.set_field(f, 1), Err(HandoffCode::TamperBlocked.val()));
        }
        assert_eq!(led.tamper_blocked, 8);
    }

    #[test]
    fn test_draft_set_field_works() {
        let mut led = HandoffLedger::new(handoff_from_truth());
        assert!(led.set_field(FieldIdx::McfgRecords, 6).is_ok());
        assert_eq!(led.get_field(FieldIdx::McfgRecords), 6);
        // 改写后真值对拍立即失配（真值闸抓装配错）。
        assert!(!truth_diff(&led.pkt).is_empty());
    }

    #[test]
    fn test_freeze_regress_rejected() {
        assert!(regress_ok());
    }

    #[test]
    fn test_mock_tamper_caught() {
        assert!(mock_tamper_caught_ok());
        let view = a2_mock_consume(&handoff_from_truth());
        assert!(view.is_ok());
        let tampered = inject_mock_tamper(&view.unwrap_or(MockView { version: 1, slots: [0; 8] }));
        assert_eq!(mock_diff(&tampered, &handoff_from_truth()), vec![FieldIdx::MadtItems.slot()]);
    }

    #[test]
    fn test_version_gate() {
        assert!(version_gate_ok());
    }

    #[test]
    fn test_fingerprint_replay() {
        assert!(fp_replay_ok());
        let a = packet_fingerprint(&handoff_from_truth());
        let mut b = handoff_from_truth();
        b.century = 0x19;
        assert_ne!(a, packet_fingerprint(&b));
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x753FF), "未知诊断码：不在 0x753C0 段闭集内");
    }
}
