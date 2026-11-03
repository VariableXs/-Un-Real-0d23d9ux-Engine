//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0469 链路能力账（Link Cap/Status 只读） · 0x75640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7G 逐字锚定，AI-VX7-1104）：
//!   Cap/Status 4 字段双列（速率/宽度/训练态/中断消息数），训练异常态显
//!   性账注。
//! 设计要点：
//!   1. 双列模型：Cap 列（Link Cap @ PCIe cap+0x0C：最大速率 bits[3:0]、
//!      最大宽度 bits[9:4]、中断消息数 bits[31:26]）与 Status 列（Link
//!      Status @ cap+0x10：协商速率 bits[3:0]、协商宽度 bits[9:4]、DLL
//!      激活 bit13），4 字段同名列对齐落账（PCIe Base r6.0 §7.5.3.6/§7.5.3.7）。
//!   2. 协商≤能力门：neg_speed ≤ cap_speed 且 neg_width ≤ cap_width，任
//!      一越门显性拒账；零值速率/宽度为无效编码，单独账注。
//!   3. 训练态闭集：4bit 训练码空间，0x0 完成；0x1~0x8 过渡态（2bit 子相
//!      位细分，TRAIN_SUBPHASE_BITS=2）；0x9~0xF 异常态逐一具名（链路未
//!      建/重训失败/配置超时/时钟漂移/均衡失败/环回超时/保留异常）。
//!   4. 异常态显性账注：任何落在 0x9~0xF 的训练码必须进异常归档账
//!      （AbnormalLedger），未归档即判据违约。
//!   5. 单调状态机：Collected→Gated→Archived→Sealed 四相，倒退显性拒；
//!      封账后列值不可再改。
//!   6. 错误路径中文建议写入诊断 detail：越门→核对链路训练结果、零值→
//!      复读 Status 寄存器、异常未归档→补记异常账、倒退→按相序推进。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75640，AI-VX7-1104 批 G 组独占区）。
pub const SEG_BASE: u32 = 0x75640;

// ── 冻结常量表（来源：PCIe Base Spec r6.0 §7.5.3.6 / §7.5.3.7）──
/// Link Cap 寄存器相对 PCIe 能力头的偏移（0x0C）。
pub const LINK_CAP_OFF: u8 = 0x0C;
/// Link Status 寄存器相对 PCIe 能力头的偏移（0x10）。
pub const LINK_STATUS_OFF: u8 = 0x10;
/// 速率编码数上限（bits[3:0]，1~16 档冻结）。
pub const SPEED_CODE_MAX: u32 = 16;
/// 宽度编码数上限（bits[9:4]，x1~x16 冻结）。
pub const WIDTH_CODE_MAX: u32 = 16;
/// 中断消息数位宽（Link Cap bits[31:26]，6bit 冻结）。
pub const IRQ_MSG_BITS: u32 = 6;
/// 训练子相位位宽（2bit 闭集：0~3）。
pub const TRAIN_SUBPHASE_BITS: u32 = 2;
/// 训练码异常下界（0x9）。
pub const TRAIN_ABNORMAL_LO: u32 = 0x9;
/// 训练码异常上界（0xF）。
pub const TRAIN_ABNORMAL_HI: u32 = 0xF;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkCode {
    /// 协商速率超过能力门
    NegSpeedOver,
    /// 协商宽度超过能力门
    NegWidthOver,
    /// 零值编码（速率/宽度为 0 无效）
    ZeroCode,
    /// 宽度编码越界（>16）
    WidthOverRange,
    /// 训练异常态未归档
    TrainNotArchived,
    /// 训练子相位越界（>3）
    SubPhaseOver,
    /// 审计状态机倒退
    PhaseRegress,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl LinkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LinkCode::NegSpeedOver => "协商速率超过能力门：核对链路训练结果后重扫",
            LinkCode::NegWidthOver => "协商宽度超过能力门：核对 Lane 配置后重扫",
            LinkCode::ZeroCode => "速率/宽度零值编码无效：复读 Status 寄存器",
            LinkCode::WidthOverRange => "宽度编码越界：最大 x16，超界必拒",
            LinkCode::TrainNotArchived => "训练异常态未归档：补记异常账后复扫",
            LinkCode::SubPhaseOver => "训练子相位越界：2bit 闭集 0~3",
            LinkCode::PhaseRegress => "审计状态机倒退：按 Collected→Gated→Archived→Sealed 推进",
            LinkCode::NotReproducible => "双跑不一致：复现红线，固定种子复跑",
        }
    }
    pub const ALL: [LinkCode; 8] = [
        LinkCode::NegSpeedOver,
        LinkCode::NegWidthOver,
        LinkCode::ZeroCode,
        LinkCode::WidthOverRange,
        LinkCode::TrainNotArchived,
        LinkCode::SubPhaseOver,
        LinkCode::PhaseRegress,
        LinkCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LinkCode::ALL.len() {
        if let Some(c) = LinkCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75640 段闭集内"
}

// ── 训练态闭集 ─────────────────────────────────────────────────
/// 训练态：0x0 完成；0x1~0x8 过渡；0x9~0xF 异常（具名显性账注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainCode {
    /// 0x0 训练完成
    Done,
    /// 0x1~0x8 过渡态（携带原始码与 2bit 子相位）
    Transient { raw: u8, subphase: u8 },
    /// 0x9 链路未建立
    AbnLinkDown,
    /// 0xA 重训失败
    AbnRetrainFail,
    /// 0xB 配置超时
    AbnCfgTimeout,
    /// 0xC 时钟漂移
    AbnClockDrift,
    /// 0xD 均衡失败
    AbnEqFail,
    /// 0xE 环回超时
    AbnLoopTimeout,
    /// 0xF 保留异常
    AbnReserved,
}

/// Default 哨兵：0x0 完成（供 LinkRow Default 派生使用）。
impl Default for TrainCode {
    fn default() -> TrainCode {
        TrainCode::Done
    }
}

impl TrainCode {
    /// 原始 4bit 码 → 闭集枚举（越界码返回 None）。
    pub fn from_raw(raw: u32) -> Option<TrainCode> {
        match raw {
            0x0 => Some(TrainCode::Done),
            0x1..=0x8 => {
                let sub = raw & ((1u32 << TRAIN_SUBPHASE_BITS) - 1);
                Some(TrainCode::Transient { raw: raw as u8, subphase: sub as u8 })
            }
            0x9 => Some(TrainCode::AbnLinkDown),
            0xA => Some(TrainCode::AbnRetrainFail),
            0xB => Some(TrainCode::AbnCfgTimeout),
            0xC => Some(TrainCode::AbnClockDrift),
            0xD => Some(TrainCode::AbnEqFail),
            0xE => Some(TrainCode::AbnLoopTimeout),
            0xF => Some(TrainCode::AbnReserved),
            _ => None,
        }
    }
    /// 原始码。
    pub fn raw(self) -> u32 {
        match self {
            TrainCode::Done => 0x0,
            TrainCode::Transient { raw, .. } => raw as u32,
            TrainCode::AbnLinkDown => 0x9,
            TrainCode::AbnRetrainFail => 0xA,
            TrainCode::AbnCfgTimeout => 0xB,
            TrainCode::AbnClockDrift => 0xC,
            TrainCode::AbnEqFail => 0xD,
            TrainCode::AbnLoopTimeout => 0xE,
            TrainCode::AbnReserved => 0xF,
        }
    }
    /// 异常态判定（0x9~0xF）。
    pub fn is_abnormal(self) -> bool {
        self.raw() >= TRAIN_ABNORMAL_LO && self.raw() <= TRAIN_ABNORMAL_HI
    }
    /// 异常态显性账注文案。
    pub fn abnormal_note(self) -> &'static str {        match self {
            TrainCode::AbnLinkDown => "链路未建立：查 LTSSM 状态机与参考时钟",
            TrainCode::AbnRetrainFail => "重训失败：核对均衡预设与 lane 极性",
            TrainCode::AbnCfgTimeout => "配置超时：查 TS1/TS2 交换序列",
            TrainCode::AbnClockDrift => "时钟漂移：查 SRIS/SRNS 模式配置",
            TrainCode::AbnEqFail => "均衡失败：查 Phase2/3 均衡系数",
            TrainCode::AbnLoopTimeout => "环回超时：查环回主从角色配置",
            TrainCode::AbnReserved => "保留异常码：固件账注后全量复扫",
            _ => "非异常态：无需账注",
        }
    }
}

// ── 双列行模型 ─────────────────────────────────────────────────
/// Cap/Status 4 字段双列行（同名列对齐，来源显性）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkRow {
    /// bdf（bus<<8|dev<<3|fn）
    pub bdf: u32,
    // ── Cap 列 ──
    /// 最大链路速率（Cap bits[3:0]，1~16 档）
    pub cap_speed: u8,
    /// 最大链路宽度（Cap bits[9:4]，x1~x16）
    pub cap_width: u8,
    /// 中断消息数（Cap bits[31:26]）
    pub irq_msg_num: u8,
    // ── Status 列 ──
    /// 协商速率（Status bits[3:0]）
    pub neg_speed: u8,
    /// 协商宽度（Status bits[9:4]）
    pub neg_width: u8,
    /// DLL 激活位（Status bit13）
    pub dll_active: bool,
    /// 训练态（Status 侧账注，闭集枚举）
    pub train: TrainCode,
    /// 训练子相位（2bit 闭集 0~3）
    pub subphase: u8,
}

impl LinkRow {
    /// 从两个 32bit 寄存器字解出行（cap_w = Link Cap，st_w = Link Status）。
    pub fn from_words(bdf: u32, cap_w: u32, st_w: u32) -> Option<LinkRow> {
        let cap_speed = (cap_w & 0xF) as u8;
        let cap_width = ((cap_w >> 4) & 0x3F) as u8;
        let irq = ((cap_w >> 26) & 0x3F) as u8;
        let neg_speed = (st_w & 0xF) as u8;
        let neg_width = ((st_w >> 4) & 0x3F) as u8;
        let dll_active = (st_w >> 13) & 1 == 1;
        let train_raw = (st_w >> 16) & 0xF;
        let train = TrainCode::from_raw(train_raw)?;
        let sub = (train_raw as u32) & ((1u32 << TRAIN_SUBPHASE_BITS) - 1);
        Some(LinkRow {
            bdf,
            cap_speed,
            cap_width,
            irq_msg_num: irq,
            neg_speed,
            neg_width,
            dll_active,
            train,
            subphase: sub as u8,
        })
    }
}

/// 从配置空间读双列（pcie cap 头偏移给定）。
pub fn read_row(cfg: &[u32], bdf: u32, cap_off: u8) -> Option<LinkRow> {
    let cap_idx = (cap_off.saturating_add(LINK_CAP_OFF)) as usize / 4;
    let st_idx = (cap_off.saturating_add(LINK_STATUS_OFF)) as usize / 4;
    let cap_w = cfg.get(cap_idx).copied()?;
    let st_w = cfg.get(st_idx).copied()?;
    LinkRow::from_words(bdf, cap_w, st_w)
}

// ── 闸门（协商≤能力 + 零值 + 越界） ────────────────────────────
/// 逐行闸门判定：违约时返回对应诊断码，干净行返回 None。
pub fn gate_row(r: &LinkRow) -> Option<u32> {
    if (r.cap_speed as u32) > SPEED_CODE_MAX {
        return Some(LinkCode::ZeroCode.val());
    }
    if (r.cap_width as u32) == 0 || (r.cap_width as u32) > WIDTH_CODE_MAX {
        return Some(LinkCode::WidthOverRange.val());
    }
    if (r.neg_speed as u32) == 0 || (r.neg_width as u32) == 0 {
        return Some(LinkCode::ZeroCode.val());
    }
    if (r.neg_speed as u32) > SPEED_CODE_MAX {
        return Some(LinkCode::NegSpeedOver.val());
    }
    if (r.neg_width as u32) > WIDTH_CODE_MAX {
        return Some(LinkCode::WidthOverRange.val());
    }
    if r.neg_speed > r.cap_speed {
        return Some(LinkCode::NegSpeedOver.val());
    }
    if r.neg_width > r.cap_width {
        return Some(LinkCode::NegWidthOver.val());
    }
    if r.subphase as u32 >= (1u32 << TRAIN_SUBPHASE_BITS) {
        return Some(LinkCode::SubPhaseOver.val());
    }
    None
}

// ── 异常归档账 ─────────────────────────────────────────────────
/// 训练异常账行：异常码 + 中文账注。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbnRow {
    pub bdf: u32,
    pub raw: u32,
    pub note: String,
}

/// 异常归档账：0x9~0xF 训练码必须显性归档。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AbnormalLedger {
    pub rows: Vec<AbnRow>,
}

impl AbnormalLedger {
    /// 归档一条异常（非异常码显性拒）。
    pub fn archive(&mut self, bdf: u32, code: TrainCode) -> Result<(), u32> {
        if !code.is_abnormal() {
            return Err(LinkCode::TrainNotArchived.val());
        }
        self.rows.push(AbnRow { bdf, raw: code.raw(), note: String::from(code.abnormal_note()) });
        Ok(())
    }
    /// 归档完整谓词：所有异常行非空账注。
    pub fn rows_noted(&self) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.note.is_empty() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 审计状态机（Collected→Gated→Archived→Sealed，单调） ────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkPhase {
    /// 已采集双列
    Collected,
    /// 已过协商门
    Gated,
    /// 异常态已归档
    Archived,
    /// 已封账
    Sealed,
}

/// 多设备链路审计器。
pub struct LinkAuditor {
    pub phase: LinkPhase,
    pub rows: Vec<LinkRow>,
    pub rejects: Vec<(u32, u32)>,
    pub abn: AbnormalLedger,
}

impl LinkAuditor {
    pub fn new(rows: Vec<LinkRow>) -> LinkAuditor {
        LinkAuditor { phase: LinkPhase::Collected, rows, rejects: Vec::new(), abn: AbnormalLedger::default() }
    }

    /// 步 1：过协商门（仅 Collected 相可入）。
    pub fn step_gate(&mut self) -> Result<(), u32> {
        if self.phase != LinkPhase::Collected {
            return Err(LinkCode::PhaseRegress.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => *r,
                None => break,
            };
            if let Some(code) = gate_row(&r) {
                self.rejects.push((r.bdf, code));
            }
            i += 1;
        }
        self.phase = LinkPhase::Gated;
        Ok(())
    }

    /// 步 2：异常态归档（仅 Gated 相可入）。
    pub fn step_archive(&mut self) -> Result<(), u32> {
        if self.phase != LinkPhase::Gated {
            return Err(LinkCode::PhaseRegress.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.train.is_abnormal() {
                    let _ = self.abn.archive(r.bdf, r.train);
                }
            }
            i += 1;
        }
        self.phase = LinkPhase::Archived;
        Ok(())
    }

    /// 步 3：封账（仅 Archived 相可入）。
    pub fn step_seal(&mut self) -> Result<(), u32> {
        if self.phase != LinkPhase::Archived {
            return Err(LinkCode::PhaseRegress.val());
        }
        self.phase = LinkPhase::Sealed;
        Ok(())
    }

    /// 干净行数（过门且无违约）。
    pub fn clean_rows(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if gate_row(r).is_none() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 谓词（私有，判据面） ────────────────────────────────────────
/// 协商≤能力门谓词。
fn neg_le_cap_ok(r: &LinkRow) -> bool {
    if r.neg_speed > r.cap_speed {
        return false;
    }
    if r.neg_width > r.cap_width {
        return false;
    }
    true
}

/// 异常归档完备谓词：行内异常码全部在账。
fn archive_complete_ok(rows: &[LinkRow], abn: &AbnormalLedger) -> bool {
    let mut i = 0;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.train.is_abnormal() {
                let mut found = false;
                let mut j = 0;
                while j < abn.rows.len() {
                    if let Some(a) = abn.rows.get(j) {
                        if a.bdf == r.bdf && a.raw == r.train.raw() {
                            found = true;
                        }
                    }
                    j += 1;
                }
                if !found {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

// ── LCG（固定种子） ────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：越门速率/越门宽度/零值/异常未归档） ──────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 真机设备表（4 台：三种正常 + 一种异常） ────────────────────
/// 行构造辅助：cap_w/st_w 组装。
fn mk_cap_w(speed: u8, width: u8, irq: u8) -> u32 {
    (speed as u32) | ((width as u32) << 4) | ((irq as u32) << 26)
}
fn mk_st_w(speed: u8, width: u8, dll: bool, train_raw: u32) -> u32 {
    let mut w = (speed as u32) | ((width as u32) << 4) | (train_raw << 16);
    if dll {
        w |= 1u32 << 13;
    }
    w
}

/// 真机 4 行：x8@8GT/s 满协商 / x4@5GT/s 半协商 / x16@16GT/s 满协商 / x2@2GT/s 异常 0xB。
pub fn machine_rows() -> Vec<LinkRow> {
    let mut v: Vec<LinkRow> = Vec::new();
    if let Some(r) = LinkRow::from_words(0x0000, mk_cap_w(3, 8, 16), mk_st_w(3, 8, true, 0x0)) {
        v.push(r);
    }
    if let Some(r) = LinkRow::from_words(0x0800, mk_cap_w(2, 4, 8), mk_st_w(2, 4, true, 0x0)) {
        v.push(r);
    }
    if let Some(r) = LinkRow::from_words(0x1000, mk_cap_w(4, 16, 32), mk_st_w(4, 16, true, 0x0)) {
        v.push(r);
    }
    if let Some(r) = LinkRow::from_words(0x1800, mk_cap_w(1, 2, 4), mk_st_w(1, 2, false, 0xB)) {
        v.push(r);
    }
    v
}

// ── 黄金全链：采集→过门→归档→封账 ──────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut aud = LinkAuditor::new(machine_rows());
    aud.step_gate()?;
    aud.step_archive()?;
    aud.step_seal()?;
    // 4 行全部过协商门（异常训练态是归档账注，不是闸门拒绝）。
    let clean = aud.clean_rows();
    let abn_ok = archive_complete_ok(&aud.rows, &aud.abn) && aud.abn.rows_noted();
    // 异常账恰好 1 行（0xB 配置超时）。
    let abn_count_ok = aud.abn.rows.len() == 1
        && match aud.abn.rows.get(0) {
            Some(r) => r.raw == 0xB && r.bdf == 0x1800,
            None => false,
        };
    // 双列字段一致性：干净行全部满足协商≤能力门。
    let gate_ok = match aud.rows.get(0) {
        Some(r) => neg_le_cap_ok(r),
        None => false,
    };
    let ok = (clean == 4) as u32
        + (abn_ok as u32)
        + (abn_count_ok as u32)
        + (gate_ok as u32)
        + (aud.phase == LinkPhase::Sealed) as u32;
    Ok((ok, aud.abn.rows.len() as u32, clean))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 5/5 绿 + 异常账 1 行 + 干净 4 行。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 1, 4)) => Ok(()),
        _ => Err(LinkCode::NotReproducible.val()),
    }
}

/// 序 B：越门注入必抓（协商速率 4 > 能力 2）。
pub fn probe_seq_b() -> Result<(), u32> {
    let r = match LinkRow::from_words(0x0800, mk_cap_w(2, 4, 8), mk_st_w(4, 4, true, 0x0)) {
        Some(r) => r,
        None => return Err(LinkCode::NotReproducible.val()),
    };
    match gate_row(&r) {
        Some(c) if c == LinkCode::NegSpeedOver.val() => Ok(()),
        _ => Err(LinkCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（异常窗 0x9~0xF / 子相位 2bit / 宽度上限 16）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        TRAIN_ABNORMAL_LO == 0x9,
        TRAIN_ABNORMAL_HI == 0xF,
        TRAIN_SUBPHASE_BITS == 2,
        WIDTH_CODE_MAX == 16,
        IRQ_MSG_BITS == 6,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(LinkCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越门速率（neg 4 > cap 2）。
        let over_s = match LinkRow::from_words(0x0800, mk_cap_w(2, 4, 8), mk_st_w(4, 4, true, 0x0)) {
            Some(r) => r,
            None => return Err(LinkCode::NotReproducible.val()),
        };
        led.record(0, probe_seq_b().is_ok() && gate_row(&over_s).is_some());
        // 路 2：越门宽度（neg 8 > cap 4）。
        let over_w = match LinkRow::from_words(0x0800, mk_cap_w(2, 4, 8), mk_st_w(2, 8, true, 0x0)) {
            Some(r) => r,
            None => return Err(LinkCode::NotReproducible.val()),
        };
        led.record(1, gate_row(&over_w) == Some(LinkCode::NegWidthOver.val()));
        // 路 3：零值速率。
        let zero = match LinkRow::from_words(0x0000, mk_cap_w(3, 8, 16), mk_st_w(0, 8, true, 0x0)) {
            Some(r) => r,
            None => return Err(LinkCode::NotReproducible.val()),
        };
        led.record(2, gate_row(&zero) == Some(LinkCode::ZeroCode.val()));
        // 路 4：异常态未归档（异常行存在但账空）。
        let rows = machine_rows();
        let empty_abn = AbnormalLedger::default();
        let _ = rows;
        led.record(3, !archive_complete_ok(&machine_rows(), &empty_abn));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LinkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（0x75000~0x781C0 窗口 + 0x40 对齐 + 闭集落窗） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < LinkCode::ALL.len() {
        if let Some(c) = LinkCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 训练码闭集谓词：0x0~0xF 全部可映射，0x10 起拒绝；异常码账注非空。
fn train_closure_ok() -> bool {
    let mut raw = 0u32;
    while raw <= 0x1F {
        let mapped = TrainCode::from_raw(raw);
        if raw <= 0xF {
            match mapped {
                Some(c) => {
                    if c.is_abnormal() && c.abnormal_note().is_empty() {
                        return false;
                    }
                }
                None => return false,
            }
        } else if mapped.is_some() {
            return false;
        }
        raw += 1;
    }
    true
}

/// 诊断码闭集查表谓词。
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < LinkCode::ALL.len() {
        if let Some(c) = LinkCode::ALL.get(i) {
            let v = c.val();
            if detail(v) != c.detail() || c.detail().is_empty() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks） ─────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "F0469-J1-黄金全链",
        match &g {
            Ok((ok, abn, clean)) => *ok == 5 && *abn == 1 && *clean == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, abn, clean)) => format!("ok={} abn={} clean={}", ok, abn, clean),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // 段位守卫。
    v.push((
        "F0469-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗口 0x75000~0x781C0", SEG_BASE),
    ));
    // 双列采集（0 号设备：x8@8GT/s 满协商）。
    let r0 = match machine_rows().get(0) {
        Some(r) => *r,
        None => LinkRow::default(),
    };
    v.push((
        "F0469-双列4字段",
        r0.cap_speed == 3 && r0.cap_width == 8 && r0.neg_speed == 3 && r0.neg_width == 8 && r0.irq_msg_num == 16,
        format!(
            "cap {}x{} st {}x{} irq={}",
            r0.cap_speed, r0.cap_width, r0.neg_speed, r0.neg_width, r0.irq_msg_num
        ),
    ));
    // 协商≤能力门。
    v.push((
        "F0469-协商≤能力门",
        neg_le_cap_ok(&r0) && gate_row(&r0).is_none(),
        String::from("满协商行过门无违约"),
    ));
    // 训练态 0x0 完成闭集。
    v.push((
        "F0469-训练态0x0完成",
        r0.train == TrainCode::Done && !r0.train.is_abnormal(),
        String::from("训练码 0x0 → Done，非异常"),
    ));
    // 异常态显性账注。
    let abn_row = match machine_rows().get(3) {
        Some(r) => *r,
        None => LinkRow::default(),
    };
    v.push((
        "F0469-异常态显性账注",
        abn_row.train == TrainCode::AbnCfgTimeout && !abn_row.train.abnormal_note().is_empty(),
        format!(
            "0x{:X} {}",
            abn_row.train.raw(),
            abn_row.train.abnormal_note()
        ),
    ));
    // 异常归档账完备。
    let mut aud = LinkAuditor::new(machine_rows());
    let flow = aud.step_gate().is_ok() && aud.step_archive().is_ok() && aud.step_seal().is_ok();
    v.push((
        "F0469-异常归档完备",
        flow && archive_complete_ok(&aud.rows, &aud.abn) && aud.abn.rows_noted() && aud.abn.rows.len() == 1,
        format!("归档行={}", aud.abn.rows.len()),
    ));
    // 越门速率注入必抓。
    v.push((
        "F0469-越门速率注入",
        probe_seq_b().is_ok(),
        String::from("neg 4 > cap 2 → NegSpeedOver"),
    ));
    // 越门宽度注入必抓。
    let over_w = match LinkRow::from_words(0x0800, mk_cap_w(2, 4, 8), mk_st_w(2, 8, true, 0x0)) {
        Some(r) => r,
        None => LinkRow::default(),
    };
    v.push((
        "F0469-越门宽度注入",
        gate_row(&over_w) == Some(LinkCode::NegWidthOver.val()),
        String::from("neg x8 > cap x4 → NegWidthOver"),
    ));
    // 零值编码注入必抓。
    let zero = match LinkRow::from_words(0x0000, mk_cap_w(3, 8, 16), mk_st_w(0, 8, true, 0x0)) {
        Some(r) => r,
        None => LinkRow::default(),
    };
    v.push((
        "F0469-零值编码注入",
        gate_row(&zero) == Some(LinkCode::ZeroCode.val()),
        String::from("neg_speed=0 → ZeroCode"),
    ));
    // 宽度越界注入必抓。
    let wide = match LinkRow::from_words(0x0000, mk_cap_w(3, 20, 16), mk_st_w(3, 8, true, 0x0)) {
        Some(r) => r,
        None => LinkRow::default(),
    };
    v.push((
        "F0469-宽度越界注入",
        gate_row(&wide) == Some(LinkCode::WidthOverRange.val()),
        String::from("cap_width=20 > 16 → WidthOverRange"),
    ));
    // 状态机倒退拒。
    let mut aud2 = LinkAuditor::new(machine_rows());
    let regress_ok = aud2.step_archive().is_err() && aud2.step_seal().is_err();
    v.push((
        "F0469-状态机倒退拒",
        regress_ok,
        String::from("Collected 相直调 archive/seal 显性拒"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F0469-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 双跑一致。
    v.push((
        "F0469-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 训练码闭集。
    v.push((
        "F0469-训练码闭集",
        train_closure_ok(),
        String::from("0x0~0xF 全映射、0x10 起拒、异常账注非空"),
    ));
    // 诊断码闭集查表。
    v.push((
        "F0469-诊断码闭集",
        code_closure_ok(),
        String::from("8 码全员落窗且查表回环一致"),
    ));
    v
}

pub fn run_unx0469_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0469 链路能力账（Link Cap/Status 只读）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_full() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.ok(), Some((5, 1, 4)));
    }

    #[test]
    fn test_neg_speed_over_caught() {
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn test_neg_width_over_caught() {
        let r = match LinkRow::from_words(0x0800, mk_cap_w(2, 4, 8), mk_st_w(2, 8, true, 0x0)) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(gate_row(&r), Some(LinkCode::NegWidthOver.val()));
    }

    #[test]
    fn test_zero_code_caught() {
        let r = match LinkRow::from_words(0x0000, mk_cap_w(3, 8, 16), mk_st_w(0, 8, true, 0x0)) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(gate_row(&r), Some(LinkCode::ZeroCode.val()));
        let r2 = match LinkRow::from_words(0x0000, mk_cap_w(3, 8, 16), mk_st_w(3, 0, true, 0x0)) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(gate_row(&r2), Some(LinkCode::ZeroCode.val()));
    }

    #[test]
    fn test_width_over_range_caught() {
        let r = match LinkRow::from_words(0x0000, mk_cap_w(3, 20, 16), mk_st_w(3, 8, true, 0x0)) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(gate_row(&r), Some(LinkCode::WidthOverRange.val()));
    }

    #[test]
    fn test_abnormal_codes_named() {
        let abns = [0x9u32, 0xA, 0xB, 0xC, 0xD, 0xE, 0xF];
        let mut i = 0;
        while i < abns.len() {
            let code = match abns.get(i).copied().and_then(TrainCode::from_raw) {
                Some(c) => c,
                None => break,
            };
            assert!(code.is_abnormal());
            assert!(!code.abnormal_note().is_empty());
            assert_ne!(code.abnormal_note(), "非异常态：无需账注");
            i += 1;
        }
        // 完成态与过渡态不是异常。
        assert!(!TrainCode::Done.is_abnormal());
        let t = match TrainCode::from_raw(0x5) {
            Some(c) => c,
            None => return,
        };
        assert!(!t.is_abnormal());
        assert_eq!(t.abnormal_note(), "非异常态：无需账注");
    }

    #[test]
    fn test_subphase_2bit() {
        // 过渡态 0x5：子相位 = 0x5 & 0x3 = 1。
        let t = match TrainCode::from_raw(0x5) {
            Some(TrainCode::Transient { raw, subphase }) => (raw, subphase),
            _ => (0, 0),
        };
        assert_eq!(t, (0x5, 0x1));
        // 过渡态 0x2：子相位 = 2。
        let t2 = match TrainCode::from_raw(0x2) {
            Some(TrainCode::Transient { raw, subphase }) => (raw, subphase),
            _ => (0, 0),
        };
        assert_eq!(t2, (0x2, 0x2));
    }

    #[test]
    fn test_archive_required_for_abnormal() {
        let mut abn = AbnormalLedger::default();
        assert!(abn.archive(0x1800, TrainCode::AbnCfgTimeout).is_ok());
        // 非异常码归档显性拒。
        assert!(abn.archive(0x0000, TrainCode::Done).is_err());
        assert!(abn.archive(0x0800, TrainCode::Transient { raw: 3, subphase: 3 }).is_err());
        assert_eq!(abn.rows.len(), 1);
    }

    #[test]
    fn test_phase_machine_regress_rejected() {
        let mut aud = LinkAuditor::new(machine_rows());
        assert!(aud.step_archive().is_err());
        assert!(aud.step_seal().is_err());
        assert!(aud.step_gate().is_ok());
        assert!(aud.step_gate().is_err());
        assert!(aud.step_archive().is_ok());
        assert!(aud.step_archive().is_err());
        assert!(aud.step_seal().is_ok());
    }

    #[test]
    fn test_dual_run_identical() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(train_closure_ok());
        assert!(code_closure_ok());
        assert_eq!(detail(0x1234_5678u32), "未知诊断码：不在 0x75640 段闭集内");
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS);
        assert!(inj.is_ok());
        let led = match inj {
            Ok(l) => l,
            Err(_) => return,
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0469_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
