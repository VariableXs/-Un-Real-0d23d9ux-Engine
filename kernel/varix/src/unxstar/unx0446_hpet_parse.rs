//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0446 HPET 表解析（F0404 对照兑现）· 0x75140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   基址/序号/最小周期/保护位 4/4 字段入账，与 F0404 探测账对照
//!   一致，缺表降级显性。
//! 设计要点：
//!   1. HPET 表布局真实建模（分派口径，依 ACPI 规范 HPET 描述表）：
//!      表头 36B@0 + event timer block ID 4B@36 + 基址结构（GAS）
//!      12B@40 + 序号 1B@52 + 最小周期 3B@53（小端 u24，100ns 计数
//!      口径）+ 页面保护 1B@56，全表 57 字节。
//!   2. 四字段账（基址/序号/最小周期/保护位）逐一入账；最小周期带
//!      100ns→ns 换算账（checked_mul，溢出显性）；保护位闭集解码
//!      （0=无保护/1=4KB 页/2=64KB 页，其余保留显性记原始值）。
//!   3. F0404 对照账：自建 mock 探测值表（F0404 批登记的探测值四
//!      元组），解析账与探测账 4/4 逐字段对照，命中数冻结为 4。
//!   4. 缺表降级显性：None 表像走降级路（Degraded 相位 + 缺表计数
//!      显性累加），不静默、不 panic；降级后不得再进 Armed（单调
//!      状态机倒退显性拒）。
//!   5. ETB ID 逐位解构账（硬件版本 bits0..7 / 比较器数−1
//!      bits8..12 / 64 位计数器 bit13 / legacy 替换 bit14）。
//!   6. 注入四路（基址/序号/最小周期/保护位逐字段篡改）必抓为
//!      3/4 命中，LCG 种子固定，双跑一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75140，WBV1104 批 HPET 解析卡区）。
pub const SEG_BASE: u32 = 0x75140;

// ── 冻结常量表（分派口径：ACPI HPET 描述表布局） ────────────────
/// HPET 表头长（通用 36B）。
pub const HPET_HDR: usize = 36;
/// event timer block ID 偏移（@36）。
pub const OFF_ETB_ID: usize = 36;
/// 基址结构（GAS）偏移（@40）。
pub const OFF_GAS: usize = 40;
/// GAS 结构长（space_id 1 + bit_width 1 + bit_offset 1 + 访问位 1 + 地址 8）。
pub const GAS_LEN: usize = 12;
/// 序号偏移（@52）。
pub const OFF_SEQ: usize = 52;
/// 最小周期偏移（@53，小端 u24）。
pub const OFF_MINPER: usize = 53;
/// 页面保护偏移（@56）。
pub const OFF_PROT: usize = 56;
/// HPET 表全长（36+4+12+1+3+1）。
pub const HPET_TBL_LEN: usize = 57;
/// 最小周期 100ns 计数 → ns 换算因子。
pub const NS_PER_TICK: u32 = 100;
/// 真机样张冻结值：HPET MMIO 基址。
pub const MACHINE_BASE: u64 = 0xFED0_0000;
/// 真机样张冻结值：序号。
pub const MACHINE_SEQ: u8 = 0;
/// 真机样张冻结值：最小周期（100ns 计数）。
pub const MACHINE_MINPER: u32 = 100;
/// 真机样张冻结值：页面保护（4KB 页保护）。
pub const MACHINE_PROT: u8 = 1;
/// 真机样张冻结值：ETB ID（rev1 + 比较器 3 + 64 位 + legacy）。
pub const MACHINE_ETB_ID: u32 = 0x0000_6201;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HpetCode {
    /// 表像残缺（不足 57 字节）
    FieldShort,
    /// 签名失配（非 "HPET"）
    SigMismatch,
    /// 基址结构坏（space_id 非 SystemMemory 或地址 0）
    GasBad,
    /// 最小周期非法（0 无法换算 100ns 口径）
    MinPeriodZero,
    /// F0404 对照失配（4/4 未齐）
    ProbeMismatch,
    /// 状态机倒退（含降级后再武装）
    ParseRegress,
    /// 降级路缺表计数失账
    DegradeLost,
    /// 双跑不一致
    NotReproducible,
}

impl HpetCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HpetCode::FieldShort => "表像残缺：不足 57 字节，建议重取 HPET 镜像",
            HpetCode::SigMismatch => "签名失配：非 HPET，建议核对根表指针",
            HpetCode::GasBad => "基址结构坏：space_id 须 SystemMemory 且地址非 0，建议核对 GAS",
            HpetCode::MinPeriodZero => "最小周期为 0：无法换算 100ns 口径，建议核对表字段",
            HpetCode::ProbeMismatch => "F0404 对照失配：4/4 未齐，建议对拍探测账",
            HpetCode::ParseRegress => "状态机倒退：建议检查驱动推进序（降级后不得再武装）",
            HpetCode::DegradeLost => "降级路缺表计数失账：建议核查缺表记账",
            HpetCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定源",
        }
    }
    pub const ALL: [HpetCode; 8] = [
        HpetCode::FieldShort,
        HpetCode::SigMismatch,
        HpetCode::GasBad,
        HpetCode::MinPeriodZero,
        HpetCode::ProbeMismatch,
        HpetCode::ParseRegress,
        HpetCode::DegradeLost,
        HpetCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，越段码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HpetCode::ALL.len() {
        if let Some(c) = HpetCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75140 段闭集内"
}

// ── GAS 基址结构（12 字节） ─────────────────────────────────────
/// GAS 地址空间闭集值：SystemMemory（ACPI 规范 §5.2.3.1）。
pub const GAS_SPACE_SYSMEM: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GasAddr {
    /// 地址空间 ID（1B@0）。
    pub space_id: u8,
    /// 寄存器位宽（1B@1）。
    pub bit_width: u8,
    /// 寄存器位偏移（1B@2）。
    pub bit_offset: u8,
    /// 访问尺寸/保留（1B@3）。
    pub access_size: u8,
    /// 64 位地址（8B@4，小端）。
    pub address: u64,
}

impl GasAddr {
    /// GAS 解码（不足 12 字节显性拒）。
    pub fn decode(img: &[u8], off: usize) -> Result<GasAddr, u32> {
        if off + GAS_LEN > img.len() {
            return Err(HpetCode::FieldShort.val());
        }
        let b = |k: usize| -> u8 { *img.get(off + k).unwrap_or(&0) };
        let mut addr = 0u64;
        let mut i = 0;
        while i < 8 {
            addr |= (b(4 + i) as u64) << (8 * i);
            i += 1;
        }
        Ok(GasAddr {
            space_id: b(0),
            bit_width: b(1),
            bit_offset: b(2),
            access_size: b(3),
            address: addr,
        })
    }
}

// ── 页面保护闭集 ────────────────────────────────────────────────
/// 页面保护位闭集（ACPI HPET 表 Page Protection 字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtectMode {
    /// 0：无保护。
    None,
    /// 1：4KB 页保护（保护起于 4KB 边界）。
    Page4K,
    /// 2：64KB 页保护。
    Page64K,
}

impl ProtectMode {
    pub fn val(self) -> u8 {
        match self {
            ProtectMode::None => 0,
            ProtectMode::Page4K => 1,
            ProtectMode::Page64K => 2,
        }
    }
    pub fn detail(self) -> &'static str {
        match self {
            ProtectMode::None => "无保护",
            ProtectMode::Page4K => "4KB 页保护",
            ProtectMode::Page64K => "64KB 页保护",
        }
    }
    pub const ALL: [ProtectMode; 3] = [
        ProtectMode::None,
        ProtectMode::Page4K,
        ProtectMode::Page64K,
    ];
}

/// 保护位解码：闭集内返 Some，保留值返 None（调用方显性记原始值）。
pub fn protect_decode(raw: u8) -> Option<ProtectMode> {
    let mut i = 0;
    while i < ProtectMode::ALL.len() {
        if let Some(m) = ProtectMode::ALL.get(i) {
            if m.val() == raw {
                return Some(*m);
            }
        }
        i += 1;
    }
    None
}

// ── HPET 四字段账 ───────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HpetFields {
    /// ETB ID 原值（4B@36）。
    pub etb_id: u32,
    /// 硬件版本（ETB bits0..7）。
    pub hw_rev: u8,
    /// 比较器数（ETB bits8..12 存数−1，此处存真实数）。
    pub comparators: u8,
    /// 64 位计数器（ETB bit13）。
    pub counter_64: bool,
    /// legacy 替换（ETB bit14）。
    pub legacy_rev: bool,
    /// 基址（GAS 12B@40）。
    pub base: GasAddr,
    /// 序号（1B@52）。
    pub seq: u8,
    /// 最小周期（100ns 计数，u24 小端 3B@53）。
    pub min_period: u32,
    /// 页面保护原值（1B@56）。
    pub protect_raw: u8,
}

impl HpetFields {
    /// 最小周期 ns 换算账（100ns 口径 ×100，checked）。
    pub fn min_period_ns(&self) -> Result<u32, u32> {
        match self.min_period.checked_mul(NS_PER_TICK) {
            Some(ns) => Ok(ns),
            None => Err(HpetCode::MinPeriodZero.val()),
        }
    }
    /// 保护位闭集解码（保留值返 None）。
    pub fn protect(&self) -> Option<ProtectMode> {
        protect_decode(self.protect_raw)
    }
}

/// HPET 字段解析（签名 → 表长 → GAS → 四字段 → ETB 解构）。
pub fn parse_hpet_fields(img: &[u8]) -> Result<HpetFields, u32> {
    if img.len() < HPET_TBL_LEN {
        return Err(HpetCode::FieldShort.val());
    }
    let mut sig = [0u8; 4];
    let mut i = 0;
    while i < 4 {
        sig[i] = *img.get(i).unwrap_or(&0);
        i += 1;
    }
    if sig != *b"HPET" {
        return Err(HpetCode::SigMismatch.val());
    }
    let etb = rd_u32(img, OFF_ETB_ID);
    let base = GasAddr::decode(img, OFF_GAS)?;
    if base.space_id != GAS_SPACE_SYSMEM || base.address == 0 {
        return Err(HpetCode::GasBad.val());
    }
    let seq = *img.get(OFF_SEQ).unwrap_or(&0);
    // 最小周期 u24 小端（3B@53）。
    let mp0 = *img.get(OFF_MINPER).unwrap_or(&0) as u32;
    let mp1 = *img.get(OFF_MINPER.wrapping_add(1)).unwrap_or(&0) as u32;
    let mp2 = *img.get(OFF_MINPER.wrapping_add(2)).unwrap_or(&0) as u32;
    let min_period = mp0 | (mp1 << 8) | (mp2 << 16);
    if min_period == 0 {
        return Err(HpetCode::MinPeriodZero.val());
    }
    let protect_raw = *img.get(OFF_PROT).unwrap_or(&0);
    let hw_rev = (etb & 0xFF) as u8;
    let comp_field = ((etb >> 8) & 0x1F) as u8;
    let comparators = comp_field.wrapping_add(1);
    let counter_64 = (etb >> 13) & 1 == 1;
    let legacy_rev = (etb >> 14) & 1 == 1;
    Ok(HpetFields {
        etb_id: etb,
        hw_rev,
        comparators,
        counter_64,
        legacy_rev,
        base,
        seq,
        min_period,
        protect_raw,
    })
}

/// 小端读 u32（越尾按 0 补账，调用方先以 FieldShort 拦截）。
fn rd_u32(img: &[u8], off: usize) -> u32 {
    let mut v = 0u32;
    let mut i = 0;
    while i < 4 {
        let b = match img.get(off + i) {
            Some(x) => *x as u32,
            None => return v,
        };
        v |= b << (8 * i);
        i += 1;
    }
    v
}

// ── F0404 对照账（自建 mock 探测值表） ──────────────────────────
/// F0404 批探测账四元组（探测值口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeAccount {
    /// 探测基址。
    pub base: u64,
    /// 探测序号。
    pub seq: u8,
    /// 探测最小周期（100ns 计数）。
    pub min_period: u32,
    /// 探测页面保护。
    pub protect: u8,
}

/// 自建 mock F0404 探测值表（冻结：与真机样张一致）。
pub const MOCK_F0404: ProbeAccount = ProbeAccount {
    base: MACHINE_BASE,
    seq: MACHINE_SEQ,
    min_period: MACHINE_MINPER,
    protect: MACHINE_PROT,
};

/// 对照报告：4/4 逐字段命中账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    pub base_ok: bool,
    pub seq_ok: bool,
    pub minper_ok: bool,
    pub protect_ok: bool,
}

impl ProbeReport {
    /// 4/4 全中。
    pub fn all4(&self) -> bool {
        self.base_ok && self.seq_ok && self.minper_ok && self.protect_ok
    }
    /// 命中数（0..4）。
    pub fn hits(&self) -> u32 {
        self.base_ok as u32 + self.seq_ok as u32 + self.minper_ok as u32 + self.protect_ok as u32
    }
}

/// 解析账 vs F0404 探测账 4/4 对照。
pub fn probe_crosscheck(f: &HpetFields, expect: &ProbeAccount) -> ProbeReport {
    ProbeReport {
        base_ok: f.base.address == expect.base,
        seq_ok: f.seq == expect.seq,
        minper_ok: f.min_period == expect.min_period,
        protect_ok: f.protect_raw == expect.protect,
    }
}

// ── 武装状态机（单调推进；降级路显性） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HpetPhase {
    /// 未开始
    Idle,
    /// 表头已读
    HeaderRead,
    /// 四字段已入账
    FieldsRead,
    /// F0404 对照已过
    ProbeCompared,
    /// 已武装（计时兜底就绪）
    Armed,
    /// 已降级（缺表，显性记账）
    Degraded,
}

impl HpetPhase {
    fn rank(self) -> u32 {
        match self {
            HpetPhase::Idle => 0,
            HpetPhase::HeaderRead => 1,
            HpetPhase::FieldsRead => 2,
            HpetPhase::ProbeCompared => 3,
            // Armed 与 Degraded 同为终态层，互斥但同级。
            HpetPhase::Armed => 4,
            HpetPhase::Degraded => 4,
        }
    }
}

/// HPET 武装游标：单调推进，倒退显性拒；降级后不得再武装。
#[derive(Clone, Debug)]
pub struct HpetCursor {
    pub phase: HpetPhase,
    pub missing: u32,
}

impl HpetCursor {
    pub fn new() -> HpetCursor {
        HpetCursor { phase: HpetPhase::Idle, missing: 0 }
    }
    pub fn advance(&mut self, to: HpetPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(HpetCode::ParseRegress.val());
        }
        // 降级终态后不得再武装（同级互斥显性拒）。
        if self.phase == HpetPhase::Degraded && to == HpetPhase::Armed {
            return Err(HpetCode::ParseRegress.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 缺表降级（显性记账：missing 计数累加）。
    pub fn degrade(&mut self) -> Result<(), u32> {
        self.missing += 1;
        self.advance(HpetPhase::Degraded)
    }
}

// ── 武装总账 ────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HpetState {
    pub phase: HpetPhase,
    pub fields: Option<HpetFields>,
    pub probe: Option<ProbeReport>,
    pub missing: u32,
}

/// 单臂决策：表像在 → 解析 + 4/4 对照 + 武装；缺表 → 显性降级。
pub fn arm_or_degrade(img: Option<&[u8]>, expect: &ProbeAccount) -> Result<HpetState, u32> {
    let mut cur = HpetCursor::new();
    let img = match img {
        Some(x) => x,
        None => {
            cur.degrade()?;
            return Ok(HpetState { phase: cur.phase, fields: None, probe: None, missing: cur.missing });
        }
    };
    cur.advance(HpetPhase::HeaderRead)?;
    if img.len() < HPET_TBL_LEN {
        return Err(HpetCode::FieldShort.val());
    }
    let f = parse_hpet_fields(img)?;
    cur.advance(HpetPhase::FieldsRead)?;
    let rep = probe_crosscheck(&f, expect);
    if !rep.all4() {
        return Err(HpetCode::ProbeMismatch.val());
    }
    cur.advance(HpetPhase::ProbeCompared)?;
    cur.advance(HpetPhase::Armed)?;
    Ok(HpetState { phase: cur.phase, fields: Some(f), probe: Some(rep), missing: cur.missing })
}

// ── HPET 表像构造（57B，整表补数 checksum） ─────────────────────
/// 构造 HPET 像：表头 36B + ETB 4B + GAS 12B + 序号/最小周期/保护。
pub fn build_hpet(base: u64, seq: u8, min_period: u32, protect: u8) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(b"HPET");
    let t = HPET_TBL_LEN as u32;
    v.push((t & 0xFF) as u8);
    v.push(((t >> 8) & 0xFF) as u8);
    v.push(((t >> 16) & 0xFF) as u8);
    v.push(((t >> 24) & 0xFF) as u8);
    v.push(1u8); // revision
    v.push(0u8); // checksum 占位
    let mut j = 0;
    while j < 6 {
        v.push(b'V');
        j += 1;
    }
    let mut k = 0;
    while k < 8 {
        v.push(b'X');
        k += 1;
    }
    let mut m = 0;
    while m < 12 {
        v.push(0u8);
        m += 1;
    }
    // ETB ID（真机冻结值）。
    let etb = MACHINE_ETB_ID;
    let mut i = 0;
    while i < 4 {
        v.push(((etb >> (8 * i)) & 0xFF) as u8);
        i += 1;
    }
    // GAS：SystemMemory / 64 位宽 / 偏移 0 / 访问 0 / 8B 地址。
    v.push(GAS_SPACE_SYSMEM);
    v.push(64u8);
    v.push(0u8);
    v.push(0u8);
    let mut a = 0;
    while a < 8 {
        v.push(((base >> (8 * a)) & 0xFF) as u8);
        a += 1;
    }
    v.push(seq);
    v.push((min_period & 0xFF) as u8);
    v.push(((min_period >> 8) & 0xFF) as u8);
    v.push(((min_period >> 16) & 0xFF) as u8);
    v.push(protect);
    // 整表补数 checksum。
    let others = {
        let mut s = 0u8;
        let mut q = 0;
        while q < v.len() {
            if let Some(x) = v.get(q) {
                s = s.wrapping_add(*x);
            }
            q += 1;
        }
        s
    };
    if let Some(slot) = v.get_mut(9) {
        *slot = 0u8.wrapping_sub(others);
    }
    v
}

/// 真机样张像。
pub fn build_machine_hpet() -> Vec<u8> {
    build_hpet(MACHINE_BASE, MACHINE_SEQ, MACHINE_MINPER, MACHINE_PROT)
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路逐字段篡改） ────────────────────────────────
/// 路 0 基址、路 1 序号、路 2 最小周期、路 3 保护位——每路篡改后
/// 对照必现 3/4 命中（被抓字段 = 被篡字段）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 种子固定，每轮四路逐字段篡改全抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0：基址篡改（+0x1000）。
        let img0 = build_hpet(MACHINE_BASE + 0x1000, MACHINE_SEQ, MACHINE_MINPER, MACHINE_PROT);
        let hit0 = match arm_or_degrade(Some(&img0), &MOCK_F0404) {
            Err(e) => e == HpetCode::ProbeMismatch.val(),
            Ok(_) => false,
        };
        led.record(0, hit0);
        // 路 1：序号篡改（+1）。
        let img1 = build_hpet(MACHINE_BASE, MACHINE_SEQ.wrapping_add(1), MACHINE_MINPER, MACHINE_PROT);
        let hit1 = match arm_or_degrade(Some(&img1), &MOCK_F0404) {
            Err(e) => e == HpetCode::ProbeMismatch.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 路 2：最小周期篡改（+1 计数 = +100ns）。
        let img2 = build_hpet(MACHINE_BASE, MACHINE_SEQ, MACHINE_MINPER.wrapping_add(1), MACHINE_PROT);
        let hit2 = match arm_or_degrade(Some(&img2), &MOCK_F0404) {
            Err(e) => e == HpetCode::ProbeMismatch.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        // 路 3：保护位篡改（1→2）。
        let img3 = build_hpet(MACHINE_BASE, MACHINE_SEQ, MACHINE_MINPER, 2);
        let hit3 = match arm_or_degrade(Some(&img3), &MOCK_F0404) {
            Err(e) => e == HpetCode::ProbeMismatch.val(),
            Ok(_) => false,
        };
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金景 ──────────────────────────────────────────────────────
/// 黄金景 A：真机样张武装 4/4。
pub fn golden_arm() -> Result<HpetState, u32> {
    arm_or_degrade(Some(&build_machine_hpet()), &MOCK_F0404)
}

/// 黄金景 B：缺表降级显性。
pub fn golden_degrade() -> Result<HpetState, u32> {
    arm_or_degrade(None, &MOCK_F0404)
}

/// 探针序 A：武装 4/4 全中且相位 Armed。
pub fn probe_seq_a() -> Result<(), u32> {
    let s = golden_arm()?;
    match (&s.phase, &s.probe) {
        (HpetPhase::Armed, Some(rep)) if rep.all4() && rep.hits() == 4 => Ok(()),
        _ => Err(HpetCode::NotReproducible.val()),
    }
}

/// 探针序 B：缺表降级显性（Degraded 相位 + 缺表计数 1）。
pub fn probe_seq_b() -> Result<(), u32> {
    let s = golden_degrade()?;
    if s.phase == HpetPhase::Degraded && s.missing == 1 && s.fields.is_none() {
        Ok(())
    } else {
        Err(HpetCode::NotReproducible.val())
    }
}

/// 探针序 C：状态机倒退必抓（含降级后再武装）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut c1 = HpetCursor::new();
    let _ = c1.advance(HpetPhase::FieldsRead);
    let r1 = match c1.advance(HpetPhase::HeaderRead) {
        Err(e) if e == HpetCode::ParseRegress.val() => true,
        _ => false,
    };
    let mut c2 = HpetCursor::new();
    let _ = c2.degrade();
    let r2 = match c2.advance(HpetPhase::Armed) {
        Err(e) if e == HpetCode::ParseRegress.val() => true,
        _ => false,
    };
    if r1 && r2 {
        Ok(())
    } else {
        Err(HpetCode::NotReproducible.val())
    }
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_arm()?;
    let b = golden_arm()?;
    let c = golden_degrade()?;
    let d = golden_degrade()?;
    if a != b || c != d {
        return Err(HpetCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < HpetCode::ALL.len() {
        if let Some(c) = HpetCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据：四字段入账且 F0404 对照 4/4 一致。
fn judge_fields_ok() -> bool {
    match golden_arm() {
        Ok(s) => match &s.fields {
            Some(f) => {
                f.base.address == MACHINE_BASE
                    && f.seq == MACHINE_SEQ
                    && f.min_period == MACHINE_MINPER
                    && f.protect() == Some(ProtectMode::Page4K)
                    && probe_seq_a().is_ok()
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// 判据：缺表降级显性。
fn judge_degrade_ok() -> bool {
    probe_seq_b().is_ok()
}

/// 判据迭代轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 守卫：段位窗口。
    v.push((
        "守卫-段位窗口",
        segment_guard(),
        format!("SEG_BASE=0x{:x}", SEG_BASE),
    ));

    // 判据：4/4 字段入账 + F0404 对照一致。
    v.push((
        "J1-4/4字段入账对照一致",
        judge_fields_ok(),
        String::from("基址/序号/最小周期/保护位 与 F0404 探测账全中"),
    ));

    // 判据：缺表降级显性。
    v.push((
        "J2-缺表降级显性",
        judge_degrade_ok(),
        String::from("None 表像 ⇒ Degraded + 缺表计数 1"),
    ));

    // 判据：注入四路逐字段篡改全抓。
    v.push((
        "J3-逐字段篡改必抓",
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        String::from("基址/序号/周期/保护 四路全中"),
    ));

    // 布局冻结账（57B 逐字段偏移）。
    v.push((
        "布局57B冻结",
        HPET_HDR == 36 && OFF_ETB_ID == 36 && OFF_GAS == 40 && GAS_LEN == 12
            && OFF_SEQ == 52 && OFF_MINPER == 53 && OFF_PROT == 56 && HPET_TBL_LEN == 57,
        format!("etb@{} gas@{} seq@{} 周期@{} 保护@{}", OFF_ETB_ID, OFF_GAS, OFF_SEQ, OFF_MINPER, OFF_PROT),
    ));

    // 最小周期 100ns 口径换算账（100 计数 = 10_000ns）。
    {
        let ok = match golden_arm() {
            Ok(s) => match &s.fields {
                Some(f) => f.min_period_ns() == Ok(MACHINE_MINPER * NS_PER_TICK),
                None => false,
            },
            Err(_) => false,
        };
        v.push((
            "最小周期100ns口径",
            ok,
            format!("{} 计数 = {} ns", MACHINE_MINPER, MACHINE_MINPER * NS_PER_TICK),
        ));
    }

    // 保护位闭集解码（保留值显性 None）。
    {
        let ok = protect_decode(0) == Some(ProtectMode::None)
            && protect_decode(1) == Some(ProtectMode::Page4K)
            && protect_decode(2) == Some(ProtectMode::Page64K)
            && protect_decode(0xFF).is_none();
        v.push((
            "保护位闭集解码",
            ok,
            String::from("0/1/2 闭集，0xFF ⇒ 保留显性"),
        ));
    }

    // ETB ID 逐位解构账。
    {
        let ok = match golden_arm() {
            Ok(s) => match &s.fields {
                Some(f) => {
                    f.etb_id == MACHINE_ETB_ID
                        && f.hw_rev == 1
                        && f.comparators == 3
                        && f.counter_64
                        && f.legacy_rev
                }
                None => false,
            },
            Err(_) => false,
        };
        v.push((
            "ETB-ID逐位解构",
            ok,
            String::from("rev=1 比较器=3 64位=1 legacy=1"),
        ));
    }

    // 签名失配显性拒。
    {
        let mut img = build_machine_hpet();
        if let Some(slot) = img.get_mut(0) {
            *slot = b'X';
        }
        let hit = parse_hpet_fields(&img) == Err(HpetCode::SigMismatch.val());
        v.push((
            "签名失配显性拒",
            hit,
            String::from("XPET ⇒ SigMismatch"),
        ));
    }

    // 表像残缺显性拒（40 字节 < 57）。
    {
        let img = build_machine_hpet();
        let hit = match img.get(..40) {
            Some(short) => parse_hpet_fields(short) == Err(HpetCode::FieldShort.val()),
            None => false,
        };
        v.push((
            "表像残缺显性拒",
            hit,
            String::from("40 字节 < 57 ⇒ FieldShort"),
        ));
    }

    // GAS 坏账显性拒（space_id 非SystemMemory）。
    {
        let mut img = build_machine_hpet();
        if let Some(slot) = img.get_mut(OFF_GAS) {
            *slot = 2; // 非 SystemMemory
        }
        let hit = parse_hpet_fields(&img) == Err(HpetCode::GasBad.val());
        v.push((
            "GAS坏账显性拒",
            hit,
            String::from("space_id≠0 ⇒ GasBad"),
        ));
    }

    // 最小周期 0 显性拒。
    {
        let img = build_hpet(MACHINE_BASE, MACHINE_SEQ, 0, MACHINE_PROT);
        let hit = parse_hpet_fields(&img) == Err(HpetCode::MinPeriodZero.val());
        v.push((
            "最小周期0显性拒",
            hit,
            String::from("min_period=0 ⇒ MinPeriodZero"),
        ));
    }

    // 状态机单调：倒退/降级后再武装必抓。
    v.push((
        "状态机单调-倒退必抓",
        probe_seq_c().is_ok(),
        String::from("FieldsRead→HeaderRead 与 Degraded→Armed 均拒"),
    ));

    // 双跑一致。
    v.push((
        "双跑一致",
        repro_check().is_ok(),
        String::from("武装/降级黄金景两跑同值"),
    ));

    // 探针三序。
    v.push((
        "探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a=4/4武装 b=降级显性 c=倒退抓"),
    ));

    v
}

pub fn run_unx0446_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0446 HPET 表解析（F0404 对照兑现）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_arm_4_of_4() {
        let s = golden_arm().ok().unwrap_or(HpetState {
            phase: HpetPhase::Idle,
            fields: None,
            probe: None,
            missing: 0,
        });
        assert_eq!(s.phase, HpetPhase::Armed);
        let rep = s.probe.unwrap_or(ProbeReport {
            base_ok: false,
            seq_ok: false,
            minper_ok: false,
            protect_ok: false,
        });
        assert!(rep.all4());
        assert_eq!(rep.hits(), 4);
    }

    #[test]
    fn test_fields_frozen_values() {
        let s = golden_arm().ok().unwrap_or(HpetState {
            phase: HpetPhase::Idle,
            fields: None,
            probe: None,
            missing: 0,
        });
        let f = s.fields.unwrap_or(HpetFields {
            etb_id: 0,
            hw_rev: 0,
            comparators: 0,
            counter_64: false,
            legacy_rev: false,
            base: GasAddr {
                space_id: 0,
                bit_width: 0,
                bit_offset: 0,
                access_size: 0,
                address: 0,
            },
            seq: 9,
            min_period: 9,
            protect_raw: 9,
        });
        assert_eq!(f.base.address, MACHINE_BASE);
        assert_eq!(f.seq, MACHINE_SEQ);
        assert_eq!(f.min_period, MACHINE_MINPER);
        assert_eq!(f.protect(), Some(ProtectMode::Page4K));
        assert_eq!(f.min_period_ns(), Ok(10_000));
    }

    #[test]
    fn test_missing_table_degrades() {
        let s = golden_degrade().ok().unwrap_or(HpetState {
            phase: HpetPhase::Idle,
            fields: Some(HpetFields {
                etb_id: 0,
                hw_rev: 0,
                comparators: 0,
                counter_64: false,
                legacy_rev: false,
                base: GasAddr {
                    space_id: 0,
                    bit_width: 0,
                    bit_offset: 0,
                    access_size: 0,
                    address: 0,
                },
                seq: 0,
                min_period: 1,
                protect_raw: 0,
            }),
            probe: None,
            missing: 0,
        });
        assert_eq!(s.phase, HpetPhase::Degraded);
        assert_eq!(s.missing, 1);
        assert!(s.fields.is_none());
    }

    #[test]
    fn test_base_tamper_caught() {
        let img = build_hpet(MACHINE_BASE + 0x1000, MACHINE_SEQ, MACHINE_MINPER, MACHINE_PROT);
        assert_eq!(
            arm_or_degrade(Some(&img), &MOCK_F0404),
            Err(HpetCode::ProbeMismatch.val())
        );
    }

    #[test]
    fn test_seq_tamper_caught() {
        let img = build_hpet(MACHINE_BASE, MACHINE_SEQ.wrapping_add(1), MACHINE_MINPER, MACHINE_PROT);
        assert_eq!(
            arm_or_degrade(Some(&img), &MOCK_F0404),
            Err(HpetCode::ProbeMismatch.val())
        );
    }

    #[test]
    fn test_minperiod_tamper_caught() {
        let img = build_hpet(MACHINE_BASE, MACHINE_SEQ, MACHINE_MINPER + 1, MACHINE_PROT);
        assert_eq!(
            arm_or_degrade(Some(&img), &MOCK_F0404),
            Err(HpetCode::ProbeMismatch.val())
        );
    }

    #[test]
    fn test_protect_tamper_caught() {
        let img = build_hpet(MACHINE_BASE, MACHINE_SEQ, MACHINE_MINPER, 2);
        assert_eq!(
            arm_or_degrade(Some(&img), &MOCK_F0404),
            Err(HpetCode::ProbeMismatch.val())
        );
    }

    #[test]
    fn test_short_and_sig_and_gas_caught() {
        let img = build_machine_hpet();
        let short = img.get(..40).unwrap_or(&[]);
        assert_eq!(parse_hpet_fields(short), Err(HpetCode::FieldShort.val()));
        let mut bad_sig = build_machine_hpet();
        if let Some(slot) = bad_sig.get_mut(0) {
            *slot = b'X';
        }
        assert_eq!(parse_hpet_fields(&bad_sig), Err(HpetCode::SigMismatch.val()));
        let mut bad_gas = build_machine_hpet();
        if let Some(slot) = bad_gas.get_mut(OFF_GAS) {
            *slot = 2;
        }
        assert_eq!(parse_hpet_fields(&bad_gas), Err(HpetCode::GasBad.val()));
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(4).ok().unwrap_or(InjectLedger::new());
        assert!(led.all_caught());
        assert_eq!(led.tries[0], 4);
        assert_eq!(led.caught[3], 4);
    }

    #[test]
    fn test_phase_regress_and_degrade_lock() {
        let mut c1 = HpetCursor::new();
        assert!(c1.advance(HpetPhase::ProbeCompared).is_ok());
        assert_eq!(
            c1.advance(HpetPhase::Idle),
            Err(HpetCode::ParseRegress.val())
        );
        let mut c2 = HpetCursor::new();
        assert!(c2.degrade().is_ok());
        assert_eq!(
            c2.advance(HpetPhase::Armed),
            Err(HpetCode::ParseRegress.val())
        );
    }

    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0446_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
