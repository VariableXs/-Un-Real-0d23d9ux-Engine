//! UNX-F50003 黑点补偿开关语义（显式登记不静默）（0x42c80 段）
//!
//! 锚点：UNX-F50003-J1 开关 ×2 档输出差异显式落账；判据锚定 §AI-63 §2 专题十二。
//!
//! 数据段 `SEG_BASE = 0x42c80`（独占 0x20）。诊断码闭集 = 本段 `E_EMPTY..=E_UNBOUND`
//! （1..=11），`BpcError::code()` 恒为 `0x42c80 + n`。
//!
//! 为什么「开关」值得单独一张卡：黑点补偿（BPC）是印前最常见的静默源——算法里
//! 一句 `if bpc { ... }` 之后，没人知道这一页到底补偿了没有、补了多少、为什么没补。
//! 本模块把开关做成**有账的二值决策**：每一次「开」或「关」都留一条记录，每一次
//! 「开了但没补」都必须带拒绝原因；差异量只看账本就能复算（`paired_diff`）。
//!
//! 结构清单：
//! * `BpcError`：11 个显式错误，`code()` 落 0x42c80 段；
//! * `BpcSwitch` / `BpcAction` / `BpcRefusal`：开关二值 × 三种结局 × 三种拒绝原因；
//! * `BpcCurve`：黑点补偿曲线（源黑→目标黑、强度混合），带反解与单调校验；
//! * `BlackBook`：纸张实测黑注册表（容量 8，重名/越界显式拒绝）；
//! * `BpcLedger`：补偿账本（容量 96 = 30 色块 × 2 档 + 余量），守恒可查；
//! * `BpcKernel`：页驱动（预扫描 → 逐 patch 决策 → 落账），`paired_diff()` 从
//!   账本单独复算两档差异；
//! * `injection_core`：5 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{
    black_point_compensate, delta_e2000, rgb8_to_lab, IccProfile, Intent, Lab, Rgb8,
};

/// 诊断段：0x42c80 独占 0x20（同批其余模块从 0x42ca0 起顺序占用）。
pub const SEG_BASE: u32 = 0x42c80;

pub const E_EMPTY: u16 = 1;
pub const E_RANGE: u16 = 2;
pub const E_LEDGER_FULL: u16 = 3;
pub const E_NO_PAPER: u16 = 4;
pub const E_PAPER_EXISTS: u16 = 5;
pub const E_BOOK_FULL: u16 = 6;
pub const E_BAD_SWITCH: u16 = 7;
pub const E_INTENT_NO_BPC: u16 = 8;
pub const E_REFUSED: u16 = 9;
pub const E_PATCH_COUNT: u16 = 10;
pub const E_UNBOUND: u16 = 11;

/// 黑点补偿的显式错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpcError {
    /// 空批或不具名纸张。
    Empty,
    /// 数值越界（NaN / 负黑 / 黑点 >60 / 强度越界 / 目标黑低于源黑）。
    Range,
    /// 账本容量不足（容量 96）。
    LedgerFull,
    /// 纸张未在实测黑注册表里登记。
    NoPaper,
    /// 同名纸张重复登记。
    PaperExists,
    /// 注册表满（容量 8）。
    BookFull,
    /// 开关码越界（合法只有 0/1）。
    BadSwitch,
    /// 该意图档在语义上不做 BPC（感知/饱和档）。
    IntentNoBpc,
    /// 曲线可用但本次补偿被拒绝（如两端黑点相同，补了等于没补）。
    Refused,
    /// 单批 patch 数越界（1..=255）。
    PatchCount,
    /// 曲线尚未绑定（未构造）。
    Unbound,
}

impl BpcError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            BpcError::Empty => E_EMPTY,
            BpcError::Range => E_RANGE,
            BpcError::LedgerFull => E_LEDGER_FULL,
            BpcError::NoPaper => E_NO_PAPER,
            BpcError::PaperExists => E_PAPER_EXISTS,
            BpcError::BookFull => E_BOOK_FULL,
            BpcError::BadSwitch => E_BAD_SWITCH,
            BpcError::IntentNoBpc => E_INTENT_NO_BPC,
            BpcError::Refused => E_REFUSED,
            BpcError::PatchCount => E_PATCH_COUNT,
            BpcError::Unbound => E_UNBOUND,
        };
        SEG_BASE + n as u32
    }

    /// 一行原因（日志用）。
    pub fn message(self) -> &'static str {
        match self {
            BpcError::Empty => "empty batch or unnamed paper",
            BpcError::Range => "black point or strength outside legal range",
            BpcError::LedgerFull => "bpc ledger full",
            BpcError::NoPaper => "paper not in measured black book",
            BpcError::PaperExists => "paper already registered",
            BpcError::BookFull => "measured black book full",
            BpcError::BadSwitch => "bpc switch code must be 0 or 1",
            BpcError::IntentNoBpc => "rendering intent has no bpc semantics",
            BpcError::Refused => "bpc refused for this curve",
            BpcError::PatchCount => "patch count outside 1..=255",
            BpcError::Unbound => "curve not bound",
        }
    }
}

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 容许的黑点上限（L*）。印刷黑版在铜版纸上实测约 4~8，涂布差的纸可到 15；
/// 60 是「明显不是黑」的硬边界，超过它说明测量或数据源本身坏了。
pub const BLACK_MAX: f32 = 60.0;

// ---------------------------------------------------------------------------
// 1. 开关 / 结局 / 拒绝原因
// ---------------------------------------------------------------------------

/// 黑点补偿开关（二值）。只有这两档——没有「自动」档：自动档正是静默的温床。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpcSwitch {
    Off,
    On,
}

impl BpcSwitch {
    pub fn code(self) -> u8 {
        match self {
            BpcSwitch::Off => 0,
            BpcSwitch::On => 1,
        }
    }

    /// 开关码解析。0/1 之外一律 `BadSwitch`（不猜默认值）。
    pub fn from_code(c: u8) -> Result<BpcSwitch, BpcError> {
        match c {
            0 => Ok(BpcSwitch::Off),
            1 => Ok(BpcSwitch::On),
            _ => Err(BpcError::BadSwitch),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BpcSwitch::Off => "off",
            BpcSwitch::On => "on",
        }
    }

    /// 取反（判据对照用）。
    pub fn flip(self) -> BpcSwitch {
        match self {
            BpcSwitch::Off => BpcSwitch::On,
            BpcSwitch::On => BpcSwitch::Off,
        }
    }
}

/// 单 patch 的结局。三值闭集，守恒式 `records = applied + refused + disabled`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpcAction {
    /// 开关关闭：源 L 原样通过（逐位不改写）。
    Disabled,
    /// 开关开启且补偿已执行。
    Applied,
    /// 开关开启但补偿被拒——**必须**带原因，绝不留空。
    Refused,
}

impl BpcAction {
    pub fn code(self) -> u8 {
        match self {
            BpcAction::Disabled => 0,
            BpcAction::Applied => 1,
            BpcAction::Refused => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BpcAction::Disabled => "disabled",
            BpcAction::Applied => "applied",
            BpcAction::Refused => "refused",
        }
    }
}

/// 拒绝原因闭集。`Refused` 结局的账目里原因不得为 `None`（构造期即保证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpcRefusal {
    /// 意图档不做 BPC（感知/饱和）。
    IntentNoBpc,
    /// 曲线的源黑与目标黑相同：补了等于没补，记拒绝而不是假装补偿。
    NoGain,
    /// 抬升量超出物理上限（> BLACK_MAX）。
    LiftTooLarge,
    /// 纸张实测黑缺失。
    PaperMissing,
}

impl BpcRefusal {
    pub fn code(self) -> u8 {
        match self {
            BpcRefusal::IntentNoBpc => 1,
            BpcRefusal::NoGain => 2,
            BpcRefusal::LiftTooLarge => 3,
            BpcRefusal::PaperMissing => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BpcRefusal::IntentNoBpc => "intent-has-no-bpc",
            BpcRefusal::NoGain => "source-black-equals-target-black",
            BpcRefusal::LiftTooLarge => "lift-exceeds-physical-limit",
            BpcRefusal::PaperMissing => "paper-black-unmeasured",
        }
    }
}

// ---------------------------------------------------------------------------
// 2. 补偿曲线
// ---------------------------------------------------------------------------

/// 黑点补偿曲线：把源黑 `src_black` 映到目标黑 `dst_black`，白点固定在 100。
///
/// 形态：`L' = strength × BPC(L) + (1 - strength) × L`，其中 `BPC` 是
/// F50001 的线性黑点映射。凸组合保持单调（两条 affine 的斜率都为正），
/// 端点 `L'=dst_black`（strength=1）与 `L'=100` 精确成立。
///
/// 为什么允许 `strength`：全量补偿会把接近黑的细节一起压掉（暗部层次丢失），
/// 印前常按 50%~100% 折中；但折中值必须**记录**，不能藏在代码里的 0.5。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BpcCurve {
    src_black: f32,
    dst_black: f32,
    strength: f32,
    bound: bool,
}

impl BpcCurve {
    /// 构造并校验。`dst_black < src_black` 被拒——那是「压黑」不是「补黑」，
    /// 会越过目标设备的黑点，属于数据源错误。
    pub fn new(src_black: f32, dst_black: f32, strength: f32) -> Result<BpcCurve, BpcError> {
        if !src_black.is_finite() || !dst_black.is_finite() || !strength.is_finite() {
            return Err(BpcError::Range);
        }
        if src_black < 0.0 || src_black > BLACK_MAX {
            return Err(BpcError::Range);
        }
        if dst_black < 0.0 || dst_black > BLACK_MAX {
            return Err(BpcError::Range);
        }
        if dst_black < src_black {
            return Err(BpcError::Range);
        }
        if strength < 0.0 || strength > 1.0 {
            return Err(BpcError::Range);
        }
        Ok(BpcCurve {
            src_black,
            dst_black,
            strength,
            bound: true,
        })
    }

    /// 未绑定曲线（只用于显式报 `Unbound`，任何求值都失败）。
    pub fn unbound() -> BpcCurve {
        BpcCurve {
            src_black: 0.0,
            dst_black: 0.0,
            strength: 0.0,
            bound: false,
        }
    }

    /// 从 profile 取目标黑（矩阵模型恒给 0，真实喷墨要用实测值——
    /// 所以这条只是「有 profile 时的下限口径」，生产走 `BlackBook`）。
    pub fn from_profile(profile: &IccProfile, strength: f32) -> Result<BpcCurve, BpcError> {
        BpcCurve::new(0.0, clampf(profile.black_l(), 0.0, BLACK_MAX), strength)
    }

    pub fn src_black(&self) -> f32 {
        self.src_black
    }

    pub fn dst_black(&self) -> f32 {
        self.dst_black
    }

    pub fn strength(&self) -> f32 {
        self.strength
    }

    pub fn is_bound(&self) -> bool {
        self.bound
    }

    /// 曲线是否「有增益」：目标黑必须严格高于源黑，否则补偿是空操作。
    pub fn has_gain(&self) -> bool {
        self.dst_black > self.src_black + 1e-6
    }

    /// 求值（L → L'）。未绑定 → `Unbound`；越界 L 先夹到 0..100。
    pub fn eval(&self, l: f32) -> Result<f32, BpcError> {
        if !self.bound {
            return Err(BpcError::Unbound);
        }
        if !l.is_finite() {
            return Err(BpcError::Range);
        }
        let x = clampf(l, 0.0, 100.0);
        let full = black_point_compensate(x, self.src_black, self.dst_black);
        let k = self.strength;
        Ok(clampf(k * full + (1.0 - k) * x, 0.0, 100.0))
    }

    /// 斜率（解析式）：`k × (100-dst)/(100-src) + (1-k)`。恒 > 0 即单调。
    pub fn slope(&self) -> f32 {
        let denom = 100.0 - self.src_black;
        if denom <= 1e-6 {
            return 1.0;
        }
        let s = (100.0 - self.dst_black) / denom;
        clampf(self.strength * s + (1.0 - self.strength), 0.0, 4.0)
    }

    /// 反解：给定输出 L'，求输入 L。二分 40 轮（区间 100 → 精度 ~1e-10）。
    ///
    /// 为什么要反解：审片时只有「纸上的 L'」，要回答「这个 L' 是输入多少打出来的」
    /// 只能靠反解；也给判据一条可复算的往返性质。
    pub fn inverse(&self, target: f32) -> Result<f32, BpcError> {
        if !self.bound {
            return Err(BpcError::Unbound);
        }
        if !target.is_finite() {
            return Err(BpcError::Range);
        }
        let lo_out = self.eval(0.0)?;
        let hi_out = self.eval(100.0)?;
        if target < lo_out - 1e-4 || target > hi_out + 1e-4 {
            return Err(BpcError::Range);
        }
        let mut lo = 0.0_f32;
        let mut hi = 100.0_f32;
        let mut i = 0usize;
        while i < 40 {
            let mid = (lo + hi) * 0.5;
            let v = self.eval(mid)?;
            if v < target {
                lo = mid;
            } else {
                hi = mid;
            }
            i += 1;
        }
        Ok((lo + hi) * 0.5)
    }

    /// 单调性自检：`samples` 个等距点逐点非降。
    pub fn is_monotone(&self, samples: usize) -> bool {
        if !self.bound || samples < 2 {
            return false;
        }
        let mut prev = match self.eval(0.0) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut i = 1usize;
        while i < samples {
            let x = 100.0 * i as f32 / (samples - 1) as f32;
            let v = match self.eval(x) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if v + 1e-5 < prev {
                return false;
            }
            prev = v;
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 3. 纸张实测黑注册表
// ---------------------------------------------------------------------------

pub const BOOK_CAP: usize = 8;

/// 一种纸的实测黑（L*）：`asserted` 标记它是不是实测值——没实测过的纸不能
/// 拿来当补偿目标（那等于用猜的数做印前决策）。
#[derive(Clone, Debug, PartialEq)]
pub struct PaperBlack {
    pub paper: String,
    pub measured_l: f32,
    pub asserted: bool,
}

/// 实测黑注册表。容量 8 种纸，重名/越界/未实测都显式拒绝。
#[derive(Clone, Debug)]
pub struct BlackBook {
    entries: Vec<PaperBlack>,
}

impl BlackBook {
    pub fn new() -> BlackBook {
        BlackBook {
            entries: Vec::new(),
        }
    }

    /// 三张常用纸的内置档（数值取自实验室常见实测区间，单位 L*）。
    pub fn standard() -> BlackBook {
        let mut b = BlackBook::new();
        let _ = b.register("plain-80g", 6.5);
        let _ = b.register("coated-120g", 3.8);
        let _ = b.register("newsprint-45g", 14.0);
        b
    }

    /// 登记一种纸的实测黑（0..=BLACK_MAX）。空名 → `Empty`；NaN/越界 → `Range`；
    /// 重名 → `PaperExists`；满 → `BookFull`。
    pub fn register(&mut self, paper: &str, measured_l: f32) -> Result<(), BpcError> {
        if paper.is_empty() {
            return Err(BpcError::Empty);
        }
        if !measured_l.is_finite() || measured_l < 0.0 || measured_l > BLACK_MAX {
            return Err(BpcError::Range);
        }
        if self.entries.len() >= BOOK_CAP {
            return Err(BpcError::BookFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.paper.as_str() == paper {
                    return Err(BpcError::PaperExists);
                }
            }
            i += 1;
        }
        self.entries.push(PaperBlack {
            paper: paper.to_string(),
            measured_l,
            asserted: true,
        });
        Ok(())
    }

    /// 查一种纸的实测黑。缺失 → `NoPaper`（不是默认 0）。
    pub fn black_of(&self, paper: &str) -> Result<f32, BpcError> {
        if paper.is_empty() {
            return Err(BpcError::Empty);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.paper.as_str() == paper {
                    return Ok(e.measured_l);
                }
            }
            i += 1;
        }
        Err(BpcError::NoPaper)
    }

    /// 按纸构造曲线（源黑取 0：sRGB 的 PCS 黑就是 0）。
    pub fn curve_for(&self, paper: &str, strength: f32) -> Result<BpcCurve, BpcError> {
        let dst = self.black_of(paper)?;
        BpcCurve::new(0.0, dst, strength)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 4. 账本
// ---------------------------------------------------------------------------

pub const LEDGER_CAP: usize = 96;

/// 一条补偿账目。`reason` 与 `action` 的配对由构造点保证：
/// `Refused` 必带 `Some(reason)`，另外两种必须是 `None`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BpcRecord {
    pub page: u16,
    pub seq: u16,
    pub patch: u8,
    pub switch: BpcSwitch,
    pub intent: Intent,
    pub action: BpcAction,
    pub src_l: f32,
    pub out_l: f32,
    pub delta_e: f32,
    pub reason: Option<BpcRefusal>,
}

/// 一批的汇总。全部字段由记录算出，不含任何「猜的值」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BpcReport {
    pub page: u16,
    pub records: usize,
    pub applied: usize,
    pub refused: usize,
    pub disabled: usize,
    pub mean_de: f32,
    pub max_de: f32,
    pub lift: f32,
}

/// 两档差异（**只从账本复算**）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchDiff {
    pub n: usize,
    pub changed: usize,
    pub identical: usize,
    pub mean_de: f32,
    pub max_de: f32,
}

/// 补偿账本。容量 96 = 30 色块 × 2 档 + 36 余量。
#[derive(Clone, Debug)]
pub struct BpcLedger {
    recs: Vec<BpcRecord>,
    applied: u32,
    refused: u32,
    disabled: u32,
}

impl BpcLedger {
    pub fn new() -> BpcLedger {
        BpcLedger {
            recs: Vec::new(),
            applied: 0,
            refused: 0,
            disabled: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn capacity_left(&self) -> usize {
        if self.recs.len() >= LEDGER_CAP {
            0
        } else {
            LEDGER_CAP - self.recs.len()
        }
    }

    /// 守恒三元组 (applied, refused, disabled)。
    pub fn conservation(&self) -> (u32, u32, u32) {
        (self.applied, self.refused, self.disabled)
    }

    pub fn requests(&self) -> u32 {
        self.applied + self.refused + self.disabled
    }

    pub fn count_of(&self, action: BpcAction) -> u32 {
        match action {
            BpcAction::Applied => self.applied,
            BpcAction::Refused => self.refused,
            BpcAction::Disabled => self.disabled,
        }
    }

    pub fn get(&self, i: usize) -> Option<&BpcRecord> {
        self.recs.get(i)
    }

    /// 追加一条（满 → `LedgerFull`，条数不变）。
    pub fn push(&mut self, rec: BpcRecord) -> Result<(), BpcError> {
        if self.recs.len() >= LEDGER_CAP {
            return Err(BpcError::LedgerFull);
        }
        if rec.action == BpcAction::Refused && rec.reason.is_none() {
            // 拒绝必须带原因：这是本模块的语义地基，宁可拒收也不留空因账目。
            return Err(BpcError::Refused);
        }
        match rec.action {
            BpcAction::Applied => self.applied += 1,
            BpcAction::Refused => self.refused += 1,
            BpcAction::Disabled => self.disabled += 1,
        }
        self.recs.push(rec);
        Ok(())
    }

    /// 某页「关档 vs 开档」的逐 patch 差异——只读账本，不碰曲线与 profile。
    ///
    /// 配对规则：同 page 同 patch 下标，一条 `Off` 一条 `On`。缺任一侧的 patch
    /// 不计入（`n` 只数真正成对的），因此 `n == 0` 时调用方能立刻发现「没落账」。
    pub fn paired_diff(&self, page: u16) -> SwitchDiff {
        let mut n = 0usize;
        let mut changed = 0usize;
        let mut identical = 0usize;
        let mut sum = 0.0_f32;
        let mut max = 0.0_f32;
        let mut i = 0usize;
        while i < self.recs.len() {
            let a = match self.recs.get(i) {
                Some(v) => v,
                None => break,
            };
            if a.page != page || a.switch != BpcSwitch::Off {
                i += 1;
                continue;
            }
            let mut j = 0usize;
            let mut paired = false;
            while j < self.recs.len() {
                if let Some(b) = self.recs.get(j) {
                    if b.page == page
                        && b.switch == BpcSwitch::On
                        && b.patch == a.patch
                        && !paired
                    {
                        let d = delta_e2000(
                            Lab::new(a.out_l, 0.0, 0.0),
                            Lab::new(b.out_l, 0.0, 0.0),
                        );
                        sum += d;
                        if d > max {
                            max = d;
                        }
                        if d > 0.05 {
                            changed += 1;
                        } else {
                            identical += 1;
                        }
                        n += 1;
                        paired = true;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        SwitchDiff {
            n,
            changed,
            identical,
            mean_de: if n > 0 { sum / n as f32 } else { 0.0 },
            max_de: max,
        }
    }

    /// 按页统计施加次数（判据核对报告与账本一致性用）。
    pub fn applied_on_page(&self, page: u16) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.page == page && r.action == BpcAction::Applied {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 每条 `Refused` 都必须带原因——返回缺因的条数（应恒为 0）。
    pub fn unreasoned_refusals(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.action == BpcAction::Refused && r.reason.is_none() {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }
}

// ---------------------------------------------------------------------------
// 5. 页驱动
// ---------------------------------------------------------------------------

/// 30 级灰阶（0..=255 等距）。BPC 只动 L，用灰阶能把「补偿量」与「色度处理」
/// 彻底解耦——若用彩色块，量到的差异里会混进色相裁切。
pub fn gray30() -> Vec<Rgb8> {
    let mut out = Vec::with_capacity(30);
    let mut i = 0u16;
    while i < 30 {
        let v = (i * 255 / 29) as u8;
        out.push(Rgb8::new(v, v, v));
        i += 1;
    }
    out
}

/// 补偿内核：曲线 + 账本 + 页号。
#[derive(Clone, Debug)]
pub struct BpcKernel {
    curve: BpcCurve,
    ledger: BpcLedger,
    page: u16,
}

impl BpcKernel {
    pub fn new(curve: BpcCurve) -> BpcKernel {
        BpcKernel {
            curve,
            ledger: BpcLedger::new(),
            page: 1,
        }
    }

    pub fn curve(&self) -> BpcCurve {
        self.curve
    }

    pub fn ledger(&self) -> &BpcLedger {
        &self.ledger
    }

    pub fn ledger_mut(&mut self) -> &mut BpcLedger {
        &mut self.ledger
    }

    pub fn page(&self) -> u16 {
        self.page
    }

    /// 换下一页（溢出不回绕，停在 u16::MAX 并显式返回当前页）。
    pub fn advance(&mut self) -> u16 {
        if self.page < u16::MAX {
            self.page += 1;
        }
        self.page
    }

    /// 跑一批 patch。语义要点：
    ///
    /// 1. **预扫描**：先逐条查 L 的有限性，再落账——半批落账比整批拒绝难审计；
    /// 2. 开关 `Off` → `Disabled`，输出逐位等于源 L（不改写）；
    /// 3. 开关 `On` + 意图无 BPC 语义 → `Refused(IntentNoBpc)`；
    /// 4. 开关 `On` + 曲线无增益 → `Refused(NoGain)`；
    /// 5. 其余 → `Applied`，输出 `curve.eval(L)`。
    ///
    /// 每种结局都写一条账目，因此「开了但没补」在账本里是可数的。
    pub fn run(
        &mut self,
        patches: &[Rgb8],
        switch: BpcSwitch,
        intent: Intent,
    ) -> Result<BpcReport, BpcError> {
        if patches.is_empty() {
            return Err(BpcError::Empty);
        }
        if patches.len() > 255 {
            return Err(BpcError::PatchCount);
        }
        if patches.len() > self.ledger.capacity_left() {
            return Err(BpcError::LedgerFull);
        }
        if !self.curve.is_bound() {
            return Err(BpcError::Unbound);
        }
        // 预扫描：任一条 L 非有限即整批拒绝，账本不留半批。
        let mut k = 0usize;
        while k < patches.len() {
            if let Some(c) = patches.get(k) {
                let lab = rgb8_to_lab(*c);
                if !lab.l.is_finite() {
                    return Err(BpcError::Range);
                }
            }
            k += 1;
        }
        let page = self.page;
        let seq_base = self.ledger.len();
        let mut applied = 0usize;
        let mut refused = 0usize;
        let mut disabled = 0usize;
        let mut sum_de = 0.0_f32;
        let mut max_de = 0.0_f32;
        let mut sum_lift = 0.0_f32;
        let mut i = 0usize;
        while i < patches.len() {
            let src = match patches.get(i) {
                Some(c) => rgb8_to_lab(*c),
                None => return Err(BpcError::PatchCount),
            };
            let (action, out_l, reason) = match switch {
                BpcSwitch::Off => (BpcAction::Disabled, src.l, None),
                BpcSwitch::On => {
                    if !intent.supports_bpc() {
                        (BpcAction::Refused, src.l, Some(BpcRefusal::IntentNoBpc))
                    } else if !self.curve.has_gain() {
                        (BpcAction::Refused, src.l, Some(BpcRefusal::NoGain))
                    } else {
                        let v = self.curve.eval(src.l)?;
                        if v - src.l > BLACK_MAX {
                            (BpcAction::Refused, src.l, Some(BpcRefusal::LiftTooLarge))
                        } else {
                            (BpcAction::Applied, v, None)
                        }
                    }
                }
            };
            let de = delta_e2000(src, Lab::new(out_l, src.a, src.b));
            if de > max_de {
                max_de = de;
            }
            sum_de += de;
            sum_lift += out_l - src.l;
            match action {
                BpcAction::Applied => applied += 1,
                BpcAction::Refused => refused += 1,
                BpcAction::Disabled => disabled += 1,
            }
            let rec = BpcRecord {
                page,
                seq: (seq_base + i) as u16,
                patch: i as u8,
                switch,
                intent,
                action,
                src_l: src.l,
                out_l,
                delta_e: de,
                reason,
            };
            self.ledger.push(rec)?;
            i += 1;
        }
        let n = patches.len();
        Ok(BpcReport {
            page,
            records: n,
            applied,
            refused,
            disabled,
            mean_de: sum_de / n as f32,
            max_de,
            lift: sum_lift / n as f32,
        })
    }
}

// ---------------------------------------------------------------------------
// 6. 注入核：5 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 5 类坏样本 → 逐类断言显式拒绝。返回 (抓住数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 目标黑低于源黑（压黑，不是补黑）→ Range
    let a = BpcCurve::new(5.0, 2.0, 1.0).err() == Some(BpcError::Range);
    // 强度越界同样是 Range
    let b = BpcCurve::new(0.0, 5.0, 1.5).err() == Some(BpcError::Range);
    let c = BpcCurve::new(0.0, 5.0, -0.1).err() == Some(BpcError::Range);
    if a && b && c {
        caught += 1;
    }

    // 2) 开关码越界 → BadSwitch
    if BpcSwitch::from_code(2).err() == Some(BpcError::BadSwitch)
        && BpcSwitch::from_code(255).err() == Some(BpcError::BadSwitch)
    {
        caught += 1;
    }

    // 3) 未登记纸张 → NoPaper（不静默取 0）
    let book = BlackBook::standard();
    if book.black_of("tissue-12g").err() == Some(BpcError::NoPaper) {
        caught += 1;
    }

    // 4) 账本容量封口 → LedgerFull 且条数不变
    let mut lg = BpcLedger::new();
    let mut k = 0usize;
    while k < LEDGER_CAP {
        let rec = BpcRecord {
            page: 9,
            seq: k as u16,
            patch: 0,
            switch: BpcSwitch::Off,
            intent: Intent::Relative,
            action: BpcAction::Disabled,
            src_l: 50.0,
            out_l: 50.0,
            delta_e: 0.0,
            reason: None,
        };
        if lg.push(rec).is_err() {
            break;
        }
        k += 1;
    }
    let before = lg.len();
    let over = lg.push(BpcRecord {
        page: 9,
        seq: 999,
        patch: 0,
        switch: BpcSwitch::On,
        intent: Intent::Relative,
        action: BpcAction::Applied,
        src_l: 1.0,
        out_l: 2.0,
        delta_e: 1.0,
        reason: None,
    });
    if over.err() == Some(BpcError::LedgerFull) && lg.len() == before {
        caught += 1;
    }

    // 5) 未绑定曲线求值 → Unbound
    let ub = BpcCurve::unbound();
    if ub.eval(50.0).err() == Some(BpcError::Unbound) && !ub.is_bound() {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 7. 判据
// ---------------------------------------------------------------------------

/// UNX-F50003 判据集（16 条）。锚点：开关 ×2 档输出差异显式落账。
pub fn run_unx50003_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50003");
    // 6.5 L* 的普通 80g 纸：真实机型上最典型的「矩阵给不出、实测才有」的黑。
    let book = BlackBook::standard();
    let curve = match book.curve_for("plain-80g", 1.0) {
        Ok(c) => c,
        Err(_) => BpcCurve::unbound(),
    };
    let patches = gray30();

    // J1-1 两档 ×30 色块全部落账（锚点原文的「显式落账」面）
    let mut kernel = BpcKernel::new(curve);
    let off = kernel.run(&patches, BpcSwitch::Off, Intent::Relative);
    let on = kernel.run(&patches, BpcSwitch::On, Intent::Relative);
    let recs = kernel.ledger().len();
    let ok = off.is_ok() && on.is_ok() && recs == 60 && patches.len() == 30;
    let det = format!("records={} patches={}", recs, patches.len());
    s.add_dyn("50003-J1-两档全落账", ok, &det);

    // J1-2 每档结局字段正确：Off 全 Disabled、On 全 Applied
    let (ap, rf, di) = kernel.ledger().conservation();
    let ok = ap == 30 && rf == 0 && di == 30 && kernel.ledger().requests() == 60;
    let det = format!("applied={} refused={} disabled={}", ap, rf, di);
    s.add_dyn("50003-J1-结局字段", ok, &det);

    // J1-3 差异可从账本单独复算：29 个 patch 有差、1 个（纯白）无差
    let diff = kernel.ledger().paired_diff(1);
    let ok = diff.n == 30 && diff.changed == 29 && diff.identical == 1;
    let det = format!(
        "n={} changed={} identical={}",
        diff.n, diff.changed, diff.identical
    );
    s.add_dyn("50003-J1-差异可复算", ok, &det);

    // J1-4 差异量级：均值 > 1.0、最大 > 2.5（6.5 L* 纸的真实补偿量）
    let ok = diff.mean_de > 1.0 && diff.max_de > 2.5;
    let det = format!("mean={:.2} max={:.2}", diff.mean_de, diff.max_de);
    s.add_dyn("50003-J1-差异量级", ok, &det);

    // J1-5 关闭档逐位不改写：所有 Disabled 记录 out_l 与 src_l 位相等
    let mut intact = true;
    let mut i = 0usize;
    while i < kernel.ledger().len() {
        if let Some(r) = kernel.ledger().get(i) {
            if r.action == BpcAction::Disabled && r.out_l.to_bits() != r.src_l.to_bits() {
                intact = false;
            }
        }
        i += 1;
    }
    s.add("50003-J1-关闭零改写", intact, "关闭档仍改写了 L");

    // J1-6 曲线端点：eval(0) == 目标黑、eval(100) == 100
    let l0 = curve.eval(0.0).unwrap_or(-1.0);
    let l100 = curve.eval(100.0).unwrap_or(-1.0);
    let ok = fabsf(l0 - curve.dst_black()) < 1e-3 && fabsf(l100 - 100.0) < 1e-3;
    let det = format!("L0={:.3} L100={:.3}", l0, l100);
    s.add_dyn("50003-J1-曲线端点", ok, &det);

    // J1-7 单调：101 点非降 + 解析斜率 > 0
    let ok = curve.is_monotone(101) && curve.slope() > 0.0;
    s.add("50003-J1-曲线单调", ok, "曲线回折");

    // J1-8 反解往返：对 21 个目标值 eval(inverse(y)) ≈ y
    let mut worst = 0.0_f32;
    let mut j = 0usize;
    while j <= 20 {
        let y = l0 + (100.0 - l0) * j as f32 / 20.0;
        if let Ok(x) = curve.inverse(y) {
            if let Ok(back) = curve.eval(x) {
                let d = fabsf(back - y);
                if d > worst {
                    worst = d;
                }
            }
        }
        j += 1;
    }
    let ok = worst < 0.05;
    let det = format!("worst={:.4}", worst);
    s.add_dyn("50003-J1-反解往返", ok, &det);

    // J1-9 开档但意图无 BPC 语义 → 全批显式拒绝（30 条带原因），输出不改写
    let mut k2 = BpcKernel::new(curve);
    let rep = k2.run(&patches, BpcSwitch::On, Intent::Perceptual);
    let (ap2, rf2, di2) = k2.ledger().conservation();
    let mut reasoned = true;
    let mut m = 0usize;
    while m < k2.ledger().len() {
        if let Some(r) = k2.ledger().get(m) {
            if r.action == BpcAction::Refused
                && (r.reason != Some(BpcRefusal::IntentNoBpc) || r.out_l.to_bits() != r.src_l.to_bits())
            {
                reasoned = false;
            }
        }
        m += 1;
    }
    let ok = rep.is_ok() && ap2 == 0 && rf2 == 30 && di2 == 0 && reasoned;
    let det = format!("refused={} reason_ok={}", rf2, reasoned);
    s.add_dyn("50003-J1-感知拒补", ok, &det);

    // J1-10 守恒：requests == applied + refused + disabled（两本账都查）
    let (a1, r1, d1) = kernel.ledger().conservation();
    let (a2, r2, d2) = k2.ledger().conservation();
    let ok = a1 + r1 + d1 == kernel.ledger().requests()
        && kernel.ledger().requests() as usize == kernel.ledger().len()
        && a2 + r2 + d2 == k2.ledger().requests()
        && kernel.ledger().unreasoned_refusals() == 0
        && k2.ledger().unreasoned_refusals() == 0;
    s.add("50003-J1-账本守恒", ok, "守恒式破裂或拒绝缺因");

    // J1-11 报告与账本一致：report.applied == 账本按页施加数
    let rep_ok = match (&off, &on) {
        (Ok(r0), Ok(r1)) => {
            r0.applied == 0
                && r0.disabled == 30
                && r1.applied == 30
                && r1.records == 30
                && kernel.ledger().applied_on_page(1) == 30
                && r1.max_de > 0.0
        }
        _ => false,
    };
    s.add("50003-J1-报告账本一致", rep_ok, "报告与账本对不上");

    // J1-12 无增益曲线：On 档必须显式拒绝（NoGain），而不是假装补偿
    let flat = BpcCurve::new(6.0, 6.0, 1.0);
    let mut k3 = BpcKernel::new(match flat {
        Ok(c) => c,
        Err(_) => BpcCurve::unbound(),
    });
    let rep3 = k3.run(&patches, BpcSwitch::On, Intent::Relative);
    let (a3, r3, d3) = k3.ledger().conservation();
    let ok = rep3.is_ok() && a3 == 0 && r3 == 30 && d3 == 0 && k3.ledger().unreasoned_refusals() == 0;
    s.add("50003-J1-无增益必拒", ok, "无增益曲线被当成补偿执行");

    // J1-13 确定性：同输入两轮逐位一致
    let mut k4 = BpcKernel::new(curve);
    let _ = k4.run(&patches, BpcSwitch::On, Intent::Relative);
    let _ = k4.run(&patches, BpcSwitch::On, Intent::Relative);
    let mut bit_same = k4.ledger().len() == 60;
    let mut x = 0usize;
    while x < 30 {
        match (k4.ledger().get(x), k4.ledger().get(x + 30)) {
            (Some(pa), Some(pb)) => {
                if pa.out_l.to_bits() != pb.out_l.to_bits()
                    || pa.delta_e.to_bits() != pb.delta_e.to_bits()
                {
                    bit_same = false;
                }
            }
            _ => bit_same = false,
        }
        x += 1;
    }
    s.add("50003-J1-确定性", bit_same, "两轮结果不一致");

    // J1-14 补偿量随亮度递减：暗端补得多、亮端补得少、白点补 0。
    //
    // 为什么不做「再补偿一次收敛」：BPC 的定义域与值域都是 0..100 的 PCS L，
    // 对已补偿值再套一次同一曲线会**再次**抬黑（第二次的源黑仍是 0），
    // 那是不收敛的；真实流程里第二次补偿必须换曲线（源黑=目标黑），
    // 所以这里断言的是曲线本身的形状性质，而不是不成立的幂等。
    let lift0 = curve.eval(0.0).unwrap_or(0.0);
    let lift50 = curve.eval(50.0).unwrap_or(0.0) - 50.0;
    let lift100 = curve.eval(100.0).unwrap_or(0.0) - 100.0;
    let ok = lift0 > lift50 && lift50 > 0.0 && fabsf(lift100) < 1e-3;
    s.add("50003-J1-补量单调递减", ok, "补偿量未随亮度递减");

    // 曲线参数非法被拒（NaN / 黑点越界 / 目标低于源）
    let ok = BpcCurve::new(f32::NAN, 5.0, 1.0).err() == Some(BpcError::Range)
        && BpcCurve::new(0.0, 61.0, 1.0).err() == Some(BpcError::Range)
        && BpcCurve::new(8.0, 4.0, 1.0).err() == Some(BpcError::Range)
        && BpcCurve::new(0.0, 5.0, 0.5).is_ok();
    s.add("50003-J1-参数必校验", ok, "非法曲线参数被接受");

    // 纸张注册表：重名拒绝 + 容量封口 + 未登记查询显式失败
    let mut bk = BlackBook::new();
    let n1 = bk.register("p0", 5.0).is_ok();
    let n2 = bk.register("p0", 6.0).err() == Some(BpcError::PaperExists);
    let mut filled = 1usize;
    let mut t = 1usize;
    while t < BOOK_CAP {
        let name = format!("p{}", t);
        if bk.register(name.as_str(), 4.0).is_ok() {
            filled += 1;
        }
        t += 1;
    }
    let over = bk.register("p99", 4.0).err() == Some(BpcError::BookFull);
    let missing = bk.black_of("p404").err() == Some(BpcError::NoPaper);
    let ok = n1 && n2 && filled == BOOK_CAP && over && missing && bk.len() == BOOK_CAP;
    s.add("50003-J1-纸册封口", ok, "注册表容量或重名未封口");

    // 注入核：5 类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50003-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50003_checks_green() {
        let set = run_unx50003_checks();
        assert!(set.all_passed(), "UNX-F50003 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50003_switch_diff_is_visible_in_ledger_only() {
        let book = BlackBook::standard();
        let curve = book.curve_for("plain-80g", 1.0).unwrap();
        let mut k = BpcKernel::new(curve);
        let patches = gray30();
        k.run(&patches, BpcSwitch::Off, Intent::Relative).unwrap();
        k.run(&patches, BpcSwitch::On, Intent::Relative).unwrap();
        let d = k.ledger().paired_diff(1);
        assert_eq!(d.n, 30);
        assert_eq!(d.changed, 29);
        assert!(d.mean_de > 1.0);
    }

    #[test]
    fn unx50003_codes_stay_in_segment() {
        let all = [
            BpcError::Empty,
            BpcError::Range,
            BpcError::LedgerFull,
            BpcError::NoPaper,
            BpcError::PaperExists,
            BpcError::BookFull,
            BpcError::BadSwitch,
            BpcError::IntentNoBpc,
            BpcError::Refused,
            BpcError::PatchCount,
            BpcError::Unbound,
        ];
        for e in all {
            assert!(e.code() >= SEG_BASE && e.code() < SEG_BASE + 0x20);
            assert!(!e.message().is_empty());
        }
    }
}
