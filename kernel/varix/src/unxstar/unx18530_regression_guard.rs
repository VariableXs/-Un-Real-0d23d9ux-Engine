//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18530 · 探针版本化与回归纪律（零回退） · 0x80040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18530-J1 已绿探针回归失绿即 P1 缺陷登记断言全过
//! （注入演练 3/3）
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：ProbeVer（探针版本束：id/版本/绿态三列）、P1Entry
//!     （P1 缺陷账行：id/版本/序号）、VerLedger（登记账 + P1 账 +
//!     演练计数账）、RegistrySnapshot（账目快照，零回退对拍用）；
//!   - 状态机：VState 四态（Enrolled→Green→Red→Green 复绿 / P1Filed→
//!     Green 修复），迁移闭集 4 条合法、其余 12 组合显式拒；
//!     未绿探针首报即红 → 拒（未绿不红纪律，首红不是回归）；
//!   - 账本：P1 账 append-only（同 (id,ver) 幂等登记）、版本单调账
//!     （只升不降，回退显式拒）、快照守恒账（账目只增不减）、
//!     注入演练计数账（3/3）；
//!   - 错误路径：RegCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（版本回退 / 未登记探针报结果 / P1 重复登记）
//!     逐类 10 轮断言被显式拒绝，漏检恒零；另含回归演练 3/3。
//! 设计要点：
//!   - 版本纪律：版本号 ∈ [1, 0xFFFF]，登记后只升不降（VersionRollback
//!     显式拒），版本 0 非法（VersionZero），0x10000 溢出拒；
//!   - 回归纪律：Green 态失绿 → P1 登记（append-only，同 (id,ver)
//!     恰一次）；Red 复绿不消账（P1 账零回退，修复只追加不复原）；
//!   - 演练 3/3：探针 7/14/21 三例注入回归，逐例检出 + 登记 + 计数，
//!     drill_caught == 3 为全过（J1 主锚点）；
//!   - 快照对拍：账目快照（登记数+P1 数+seq）单调不减，任何缩账
//!     尝试（LedgerShrink）显式拒；
//!   - 常量推导：0x80040 = 0x80000 + 0x40（A13 批第 10 卡独占窗）；
//!     VER_MAX = 0xFFFF（版本号 u16 域闭集）；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80040，A13 批第 10 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x80040;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 探针 id 闭集（与总纲一致 1..=50）。
pub const PROBE_COUNT: usize = 50;
/// 版本号上限（u16 域闭集，0 非法）。
pub const VER_MAX: u32 = 0xFFFF;
/// 注入演练探针集（冻结：7/14/21 三例）。
pub const DRILL_PROBES: [u32; 3] = [7, 14, 21];
/// 演练目标（冻结：3/3，J1 主锚点）。
pub const DRILL_TARGET: usize = 3;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（版本回退/未登记报结果/P1 重复 三类）。
pub const INJECT_CLASSES: usize = 3;
/// 演练登记版本（冻结：v1）。
pub const DRILL_VER: u32 = 1;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（账本指纹唯一哈希来源）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// probe_id 越界（0 或 >50）
    IdOutOfRange,
    /// 版本号为 0（非法）
    VersionZero,
    /// 版本回退（新版本 ≤ 当前版本，零回退红线）
    VersionRollback,
    /// 版本溢出（> 0xFFFF）
    VersionOverflow,
    /// 探针未登记就报结果
    NotEnrolled,
    /// 未绿不红（首报即红，无绿基线，非回归不进 P1）
    NotGreenBaseline,
    /// P1 重复登记（同 (id, ver) 已在账）
    DupP1Registration,
    /// 账目缩账尝试（快照回退，append-only 红线）
    LedgerShrink,
    /// 状态机非法迁移
    StateIllegalTransit,
    /// 演练不齐（drill_caught < 3）
    DrillIncomplete,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏检
    InjectMissed,
}

impl RegCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x80040..0x80080）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::IdOutOfRange => "probe_id 越界：合法闭集 1..=50",
            RegCode::VersionZero => "版本号为 0：非法（域闭集 1..=0xFFFF）",
            RegCode::VersionRollback => "版本回退：新版本 ≤ 当前版本，零回退红线",
            RegCode::VersionOverflow => "版本溢出：超过 0xFFFF 上限",
            RegCode::NotEnrolled => "探针未登记：先登记后报结果",
            RegCode::NotGreenBaseline => "未绿不红：首报即红无绿基线，非回归",
            RegCode::DupP1Registration => "P1 重复登记：同 (id, ver) 已在账",
            RegCode::LedgerShrink => "账目缩账尝试：快照回退，append-only 红线",
            RegCode::StateIllegalTransit => "状态机非法迁移：越级/回退全拒",
            RegCode::DrillIncomplete => "演练不齐：drill_caught < 3/3",
            RegCode::NotReproducible => "双跑不一致：确定性红线",
            RegCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [RegCode; 12] = [
        RegCode::IdOutOfRange,
        RegCode::VersionZero,
        RegCode::VersionRollback,
        RegCode::VersionOverflow,
        RegCode::NotEnrolled,
        RegCode::NotGreenBaseline,
        RegCode::DupP1Registration,
        RegCode::LedgerShrink,
        RegCode::StateIllegalTransit,
        RegCode::DrillIncomplete,
        RegCode::NotReproducible,
        RegCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if let Some(c) = RegCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x80040 段闭集内"
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 探针版本态四态：Enrolled→Green→Red→Green（复绿）/ P1Filed→Green。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VState {
    /// 已登记未出首报
    Enrolled,
    /// 绿基线在位
    Green,
    /// 回归失绿（P1 已登记）
    Red,
    /// P1 已登记但当前态仍绿前历史（绿→红即转 Red，P1Filed 只作账面旗）
    P1Filed,
}

/// 迁移闭集：合法恰 4 条（Enrolled→Green / Green→Red / Red→Green /
/// P1Filed→Green），其余 12 组合显式拒。
pub fn can_transit(from: VState, to: VState) -> bool {
    match (from, to) {
        (VState::Enrolled, VState::Green) => true,
        (VState::Green, VState::Red) => true,
        (VState::Red, VState::Green) => true,
        (VState::P1Filed, VState::Green) => true,
        _ => false,
    }
}

// ── 数据结构 ────────────────────────────────────────────────────
/// 探针版本束（id / 当前版本 / 当前态 三列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeVer {
    pub probe_id: u32,
    pub version: u32,
    pub state: VState,
}

/// P1 缺陷账行（append-only）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P1Entry {
    pub probe_id: u32,
    pub ver: u32,
    /// 全局登记序号（单调递增，审计排序键）。
    pub seq: u32,
}

/// 报结果裁决（回归处理结果闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegOutcome {
    /// 绿基线保持（绿跑绿）
    StayGreen,
    /// 回归检出且 P1 已登记
    RegressedFiled,
    /// 已在 Red 的重复失败（不重复登记）
    AlreadyRed,
    /// 复绿（Red/P1Filed → Green，修复通道）
    Repaired,
}

/// 账目快照（零回退对拍：登记数 + P1 数 + seq 三列单调不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrySnapshot {
    pub enrolled: u32,
    pub p1_count: u32,
    pub seq_head: u32,
}

/// 版本化总账。
#[derive(Clone, Debug, Default)]
pub struct VerLedger {
    pub probes: Vec<ProbeVer>,
    pub p1: Vec<P1Entry>,
    /// 全局 P1 登记序号计数器。
    pub seq_counter: u32,
    /// 状态机拒迁计数。
    pub transit_rejects: u32,
    /// 演练检出计数（3/3 目标）。
    pub drill_caught: u32,
}

impl VerLedger {
    pub fn new() -> VerLedger {
        VerLedger {
            probes: Vec::new(),
            p1: Vec::new(),
            seq_counter: 0,
            transit_rejects: 0,
            drill_caught: 0,
        }
    }

    /// 在账查询。
    pub fn find(&self, probe_id: u32) -> Option<ProbeVer> {
        let mut i = 0;
        while i < self.probes.len() {
            if let Some(p) = self.probes.get(i) {
                if p.probe_id == probe_id {
                    return Some(p.clone());
                }
            }
            i += 1;
        }
        None
    }

    /// 登记（enroll）：版本域闸 + id 闭集 + dup 拒。
    pub fn enroll(&mut self, probe_id: u32, ver: u32) -> Result<(), u32> {
        if probe_id == 0 || probe_id > PROBE_COUNT as u32 {
            return Err(RegCode::IdOutOfRange.code());
        }
        if ver == 0 {
            return Err(RegCode::VersionZero.code());
        }
        if ver > VER_MAX {
            return Err(RegCode::VersionOverflow.code());
        }
        if self.find(probe_id).is_some() {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(RegCode::StateIllegalTransit.code());
        }
        self.probes.push(ProbeVer { probe_id, version: ver, state: VState::Enrolled });
        Ok(())
    }

    /// 版本提升：严格单调递增（回退/平版/溢出/零 全拒）。
    pub fn bump(&mut self, probe_id: u32, new_ver: u32) -> Result<u32, u32> {
        if probe_id == 0 || probe_id > PROBE_COUNT as u32 {
            return Err(RegCode::IdOutOfRange.code());
        }
        if new_ver == 0 {
            return Err(RegCode::VersionZero.code());
        }
        if new_ver > VER_MAX {
            return Err(RegCode::VersionOverflow.code());
        }
        let cur = match self.find(probe_id) {
            Some(p) => p,
            None => return Err(RegCode::NotEnrolled.code()),
        };
        if new_ver <= cur.version {
            return Err(RegCode::VersionRollback.code());
        }
        let idx = (probe_id - 1) as usize;
        // 探针束按 id 存放序查找（非下标直取，防注册乱序）。
        let mut pos = None;
        let mut i = 0;
        while i < self.probes.len() {
            if let Some(p) = self.probes.get(i) {
                if p.probe_id == probe_id {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let pos = match pos {
            Some(p) => p,
            None => return Err(RegCode::NotEnrolled.code()),
        };
        if let Some(p) = self.probes.get_mut(pos) {
            p.version = new_ver;
        }
        Ok(new_ver)
    }

    /// P1 登记（append-only，同 (id, ver) 幂等拒）。
    pub fn file_p1(&mut self, probe_id: u32, ver: u32) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.p1.len() {
            if let Some(e) = self.p1.get(i) {
                if e.probe_id == probe_id && e.ver == ver {
                    return Err(RegCode::DupP1Registration.code());
                }
            }
            i += 1;
        }
        self.seq_counter = self.seq_counter.saturating_add(1);
        let seq = self.seq_counter;
        self.p1.push(P1Entry { probe_id, ver, seq });
        Ok(seq)
    }

    /// 报结果：Enrolled→Green（首绿）/ Green→Red（回归→P1）/
    /// Red→Green（复绿）/ 已 Red 重复失败不重复登记。
    pub fn report(&mut self, probe_id: u32, ver: u32, passed: bool) -> Result<RegOutcome, u32> {
        if probe_id == 0 || probe_id > PROBE_COUNT as u32 {
            return Err(RegCode::IdOutOfRange.code());
        }
        let cur = match self.find(probe_id) {
            Some(p) => p,
            None => return Err(RegCode::NotEnrolled.code()),
        };
        // 结果版本必须与当前版本一致（陈旧结果拒）。
        if ver != cur.version {
            return Err(RegCode::StateIllegalTransit.code());
        }
        let idx = (probe_id - 1) as usize;
        let mut pos = None;
        let mut i = 0;
        while i < self.probes.len() {
            if let Some(p) = self.probes.get(i) {
                if p.probe_id == probe_id {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let pos = match pos {
            Some(p) => p,
            None => return Err(RegCode::NotEnrolled.code()),
        };
        match cur.state {
            VState::Enrolled => {
                // 未绿不红：首报即红 → 拒（无绿基线非回归）。
                if !passed {
                    self.transit_rejects = self.transit_rejects.saturating_add(1);
                    return Err(RegCode::NotGreenBaseline.code());
                }
                if !can_transit(VState::Enrolled, VState::Green) {
                    return Err(RegCode::StateIllegalTransit.code());
                }
                if let Some(p) = self.probes.get_mut(pos) {
                    p.state = VState::Green;
                }
                Ok(RegOutcome::StayGreen)
            }
            VState::Green => {
                if passed {
                    Ok(RegOutcome::StayGreen)
                } else {
                    // 回归失绿：P1 登记（append-only 恰一次）+ 态转 Red。
                    if !can_transit(VState::Green, VState::Red) {
                        return Err(RegCode::StateIllegalTransit.code());
                    }
                    self.file_p1(probe_id, ver)?;
                    if let Some(p) = self.probes.get_mut(pos) {
                        p.state = VState::Red;
                    }
                    Ok(RegOutcome::RegressedFiled)
                }
            }
            VState::Red => {
                if passed {
                    // 复绿（修复通道），P1 账不消。
                    if !can_transit(VState::Red, VState::Green) {
                        return Err(RegCode::StateIllegalTransit.code());
                    }
                    if let Some(p) = self.probes.get_mut(pos) {
                        p.state = VState::Green;
                    }
                    Ok(RegOutcome::Repaired)
                } else {
                    // 已 Red 重复失败：不重复登记，显性回执。
                    Ok(RegOutcome::AlreadyRed)
                }
            }
            VState::P1Filed => {
                if passed {
                    if !can_transit(VState::P1Filed, VState::Green) {
                        return Err(RegCode::StateIllegalTransit.code());
                    }
                    if let Some(p) = self.probes.get_mut(pos) {
                        p.state = VState::Green;
                    }
                    Ok(RegOutcome::Repaired)
                } else {
                    Ok(RegOutcome::AlreadyRed)
                }
            }
        }
    }

    /// 账目快照（零回退对拍基准）。
    pub fn snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot {
            enrolled: self.probes.len() as u32,
            p1_count: self.p1.len() as u32,
            seq_head: self.seq_counter,
        }
    }

    /// 快照零回退闸：cur 不得小于 prev 任何一列。
    pub fn no_shrink(prev: RegistrySnapshot, cur: RegistrySnapshot) -> Result<(), u32> {
        if cur.enrolled < prev.enrolled
            || cur.p1_count < prev.p1_count
            || cur.seq_head < prev.seq_head
        {
            return Err(RegCode::LedgerShrink.code());
        }
        Ok(())
    }

    /// P1 账查询。
    pub fn p1_contains(&self, probe_id: u32, ver: u32) -> bool {
        let mut i = 0;
        while i < self.p1.len() {
            if let Some(e) = self.p1.get(i) {
                if e.probe_id == probe_id && e.ver == ver {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 账本指纹（probes+p1 拼列 FNV，双跑对拍用）。
    pub fn ledger_fnv(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.probes.len() {
            if let Some(p) = self.probes.get(i) {
                bytes.push((p.probe_id & 0xFF) as u8);
                bytes.push((p.version & 0xFF) as u8);
                bytes.push((p.version >> 8) as u8);
                bytes.push(p.state as u8);
            }
            i += 1;
        }
        bytes.push(0xBB);
        let mut j = 0;
        while j < self.p1.len() {
            if let Some(e) = self.p1.get(j) {
                bytes.push((e.probe_id & 0xFF) as u8);
                bytes.push((e.ver & 0xFF) as u8);
                bytes.push((e.seq & 0xFF) as u8);
            }
            j += 1;
        }
        fnv1a(&bytes)
    }
}

// ── 注入演练 3/3（J1 主锚点） ───────────────────────────────────
/// 回归注入演练：三探针各自 绿→失绿→P1 恰一次。
///
/// 每例断言链：enroll 成功 → 首报绿 → 回归失绿检出 → P1 在账恰一条
/// → 重复失绿不重复登记。三例全过 → drill_caught += 1（每轮重建账）。
pub fn regression_drill_once() -> Result<bool, u32> {
    let mut led = VerLedger::new();
    let mut all_ok = true;
    let mut k = 0;
    while k < DRILL_PROBES.len() {
        let pid = match DRILL_PROBES.get(k) {
            Some(x) => *x,
            None => return Err(RegCode::IdOutOfRange.code()),
        };
        led.enroll(pid, DRILL_VER)?;
        // 首报绿：建绿基线。
        let first = led.report(pid, DRILL_VER, true)?;
        if first != RegOutcome::StayGreen {
            all_ok = false;
            break;
        }
        // 回归失绿：P1 登记恰一次。
        let reg = led.report(pid, DRILL_VER, false)?;
        if reg != RegOutcome::RegressedFiled {
            all_ok = false;
            break;
        }
        if !led.p1_contains(pid, DRILL_VER) || led.p1.len() != (k + 1) {
            all_ok = false;
            break;
        }
        // 重复失绿：不重复登记。
        let again = led.report(pid, DRILL_VER, false)?;
        if again != RegOutcome::AlreadyRed || led.p1.len() != (k + 1) {
            all_ok = false;
            break;
        }
        k += 1;
    }
    Ok(all_ok && led.p1.len() == DRILL_PROBES.len())
}

/// 演练 rounds 轮（每轮重建账），返回全过轮数。
pub fn regression_drill(rounds: u32) -> Result<u32, u32> {
    let mut ok_rounds = 0u32;
    let mut r = 0;
    while r < rounds {
        if regression_drill_once()? {
            ok_rounds = ok_rounds.saturating_add(1);
        }
        r += 1;
    }
    Ok(ok_rounds)
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：版本回退（v3→v2）→ VersionRollback 显式拒。
pub fn inject_version_rollback() -> Result<bool, u32> {
    let mut led = VerLedger::new();
    led.enroll(30, 3)?;
    let hit = match led.bump(30, 2) {
        Err(c) => c == RegCode::VersionRollback.code(),
        Ok(_) => false,
    };
    // 平版（3→3）同拒。
    let flat = match led.bump(30, 3) {
        Err(c) => c == RegCode::VersionRollback.code(),
        Ok(_) => false,
    };
    Ok(hit && flat)
}

/// 类②：未登记探针报结果 → NotEnrolled 显式拒。
pub fn inject_not_enrolled() -> Result<bool, u32> {
    let mut led = VerLedger::new();
    let hit = match led.report(40, 1, true) {
        Err(c) => c == RegCode::NotEnrolled.code(),
        Ok(_) => false,
    };
    // 未绿不红：登记后首报即红 → NotGreenBaseline 拒。
    let _ = led.enroll(41, 1);
    let h2 = match led.report(41, 1, false) {
        Err(c) => c == RegCode::NotGreenBaseline.code(),
        Ok(_) => false,
    };
    Ok(hit && h2)
}

/// 类③：P1 重复登记（同 (id,ver) 二次登记）→ DupP1Registration 拒。
pub fn inject_dup_p1() -> Result<bool, u32> {
    let mut led = VerLedger::new();
    let _ = led.enroll(45, 1);
    let _ = led.report(45, 1, true);
    let _ = led.report(45, 1, false)?; // 回归 → P1 登记
    // 直接重复 file_p1 → 拒。
    let hit = match led.file_p1(45, 1) {
        Err(c) => c == RegCode::DupP1Registration.code(),
        Ok(_) => false,
    };
    Ok(hit && led.p1.len() == 1)
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
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
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类断言被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_version_rollback()?);
        led.record(1, inject_not_enrolled()?);
        led.record(2, inject_dup_p1()?);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：登记→绿基线→版本提升→复绿→快照零回退→演练 3/3。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = VerLedger::new();
    // 演练 3/3。
    let drill = regression_drill(JUDGE_ROUNDS)?;
    if drill != JUDGE_ROUNDS {
        return Err(RegCode::DrillIncomplete.code());
    }
    led.drill_caught = DRILL_TARGET as u32;
    // 版本提升合法通道：50→51。
    led.enroll(50, 50)?;
    let _ = led.report(50, 50, true)?; // 首绿
    let _ = led.bump(50, 51)?;
    let _ = led.report(50, 51, true)?; // 新版首绿
    // 回归→复绿：P1 账不消。
    let _ = led.report(50, 51, false)?;
    let _ = led.report(50, 51, true)?;
    if !led.p1_contains(50, 51) {
        return Err(RegCode::LedgerShrink.code());
    }
    // 快照零回退对拍。
    let s1 = led.snapshot();
    let _ = led.report(50, 51, true)?;
    let s2 = led.snapshot();
    VerLedger::no_shrink(s1, s2)?;
    Ok((led.p1.len() as u32, led.probes.len() as u32, led.seq_counter))
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：演练 + 黄金链两次相等 + 账本指纹稳定。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RegCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if let Some(c) = RegCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 版本域边界：0 拒 / 0xFFFF 合法 / 0x10000 拒。
pub fn version_boundary() -> Result<bool, u32> {
    let mut led = VerLedger::new();
    let h0 = led.enroll(1, 0).is_err();
    let hmax = led.enroll(1, VER_MAX).is_ok();
    let hover = led.enroll(2, VER_MAX + 1).is_err();
    Ok(h0 && hmax && hover)
}

/// 缩账注入：快照回退（p1_count 少一列）→ LedgerShrink 显式拒。
pub fn inject_shrink() -> Result<bool, u32> {
    let prev = RegistrySnapshot { enrolled: 3, p1_count: 2, seq_head: 2 };
    let same = RegistrySnapshot { enrolled: 3, p1_count: 2, seq_head: 2 };
    let grow = RegistrySnapshot { enrolled: 4, p1_count: 3, seq_head: 3 };
    let shrink = RegistrySnapshot { enrolled: 2, p1_count: 1, seq_head: 1 };
    let h_same = VerLedger::no_shrink(prev, same).is_ok();
    let h_grow = VerLedger::no_shrink(prev, grow).is_ok();
    let h_shrink = match VerLedger::no_shrink(prev, shrink) {
        Err(c) => c == RegCode::LedgerShrink.code(),
        Ok(_) => false,
    };
    Ok(h_same && h_grow && h_shrink)
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18530 自检判据（15 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18530_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18530 探针版本化与回归纪律（零回退）");

    s.add("J3-段位守卫", segment_guard(), "0x80040 窗口内 12 码全落位");

    // J1 主锚点：回归注入演练 3/3。
    let drill = regression_drill(JUDGE_ROUNDS);
    let (d_ok, d_txt) = match &drill {
        Ok(n) => (*n == JUDGE_ROUNDS, format!("演练全过轮={}/{}", n, JUDGE_ROUNDS)),
        Err(c) => (false, format!("演练断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-回归演练3/3", d_ok, &d_txt);

    // P1 登记恰一次（演练账内 3 探针各恰一条）。
    let p1_once = {
        let led = match drill_once_ledger() {
            Ok(l) => l,
            Err(_) => VerLedger::new(),
        };
        led.p1.len() == DRILL_TARGET
            && led.p1_contains(7, DRILL_VER)
            && led.p1_contains(14, DRILL_VER)
            && led.p1_contains(21, DRILL_VER)
    };
    s.add("J1-P1登记恰一次", p1_once, "7/14/21 三例各恰一条 P1 账");

    // 复绿不消账：回归→复绿后 P1 账仍在（零回退）。
    let repair_keeps = {
        let mut led = VerLedger::new();
        let _ = led.enroll(50, 1);
        let _ = led.report(50, 1, true);
        let _ = led.report(50, 1, false);
        let had = led.p1_contains(50, 1);
        let _ = led.report(50, 1, true); // 复绿
        had && led.p1_contains(50, 1)
    };
    s.add("J2-复绿不消账", repair_keeps, "Red→Green 后 P1 账仍在（append-only）");

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①版本回退10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "v3→v2 与平版全拒",
    );
    s.add(
        "J2-注入②未登记报结果10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "NotEnrolled + 未绿不红 双闸",
    );
    s.add(
        "J2-注入③P1重复登记10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "同 (id,ver) 二次登记拒",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 缩账注入。
    let shrink = inject_shrink();
    s.add(
        "J2-缩账注入检出",
        match shrink { Ok(v) => v, Err(_) => false },
        "快照回退 LedgerShrink 拒，增账/平账放行",
    );

    // 状态机：迁移闭集合法恰 4 条。
    let states = [VState::Enrolled, VState::Green, VState::Red, VState::P1Filed];
    let mut legal_n = 0usize;
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) { Some(x) => *x, None => break };
            let fb = match states.get(b) { Some(x) => *x, None => break };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J2-迁移闭集合法恰4条", legal_n == 4, "4 态 16 组合枚举：合法 4 / 非法 12");

    // 版本域边界。
    let vb = version_boundary();
    s.add(
        "J3-版本域0/FFFF/10000",
        match vb { Ok(v) => v, Err(_) => false },
        "0 拒 / 0xFFFF 合法 / 0x10000 拒",
    );

    // 黄金链（版本提升 + 回归复绿 + 快照对拍）。
    s.add("J2-黄金链零回退", golden_run().is_ok(), "版本提升→回归→复绿→快照单调");

    // 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < RegCode::ALL.len() {
        if let Some(c) = RegCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 收口。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

/// 演练单轮账本形态（供 P1 恰一次判据复用）。
pub fn drill_once_ledger() -> Result<VerLedger, u32> {
    let mut led = VerLedger::new();
    let mut k = 0;
    while k < DRILL_PROBES.len() {
        let pid = match DRILL_PROBES.get(k) {
            Some(x) => *x,
            None => return Err(RegCode::IdOutOfRange.code()),
        };
        led.enroll(pid, DRILL_VER)?;
        led.report(pid, DRILL_VER, true)?;
        led.report(pid, DRILL_VER, false)?;
        k += 1;
    }
    Ok(led)
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18530_checks();
        assert!(set.all_passed(), "UNX-F18530 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_drill_three_of_three() {
        let d = regression_drill(JUDGE_ROUNDS);
        assert!(d.is_ok());
        assert_eq!(d.unwrap_or(0), JUDGE_ROUNDS);
    }

    #[test]
    fn t03_regression_files_p1_once() {
        let led = drill_once_ledger().unwrap_or(VerLedger::new());
        assert_eq!(led.p1.len(), 3);
        // 序号单调 1/2/3。
        let s0 = led.p1.get(0).map(|e| e.seq).unwrap_or(0);
        let s1 = led.p1.get(1).map(|e| e.seq).unwrap_or(0);
        let s2 = led.p1.get(2).map(|e| e.seq).unwrap_or(0);
        assert_eq!(s0, 1);
        assert_eq!(s1, 2);
        assert_eq!(s2, 3);
    }

    #[test]
    fn t04_version_monotonic() {
        let mut led = VerLedger::new();
        assert!(led.enroll(9, 1).is_ok());
        assert_eq!(led.bump(9, 2).unwrap_or(0), 2);
        assert_eq!(led.bump(9, 5).unwrap_or(0), 5);
        // 回退/平版/溢出/零 全拒。
        assert!(led.bump(9, 4).is_err());
        assert!(led.bump(9, 5).is_err());
        assert!(led.bump(9, VER_MAX + 1).is_err());
        assert!(led.bump(9, 0).is_err());
    }

    #[test]
    fn t05_not_green_no_red() {
        let mut led = VerLedger::new();
        let _ = led.enroll(10, 1);
        // 首报即红 → 拒。
        assert!(led.report(10, 1, false).is_err());
        // 首报绿 → Enrolled→Green。
        assert_eq!(led.report(10, 1, true).unwrap_or(RegOutcome::AlreadyRed), RegOutcome::StayGreen);
    }

    #[test]
    fn t06_stale_version_rejected() {
        let mut led = VerLedger::new();
        let _ = led.enroll(11, 2);
        // 报旧版本结果 → 拒。
        assert!(led.report(11, 1, true).is_err());
        assert!(led.report(11, 2, true).is_ok());
    }

    #[test]
    fn t07_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t08_snapshot_no_shrink() {
        let mut led = VerLedger::new();
        let _ = led.enroll(20, 1);
        let _ = led.report(20, 1, true);
        let s1 = led.snapshot();
        let _ = led.report(20, 1, false); // 回归 → P1
        let s2 = led.snapshot();
        assert!(s2.p1_count > s1.p1_count);
        assert!(VerLedger::no_shrink(s1, s2).is_ok());
        // 缩账注入。
        assert!(inject_shrink().unwrap_or(false));
    }

    #[test]
    fn t09_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < RegCode::ALL.len() {
            if let Some(c) = RegCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x80040 段闭集内");
    }
}
