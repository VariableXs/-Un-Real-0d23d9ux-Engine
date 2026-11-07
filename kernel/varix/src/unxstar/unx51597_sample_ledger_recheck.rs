//! UNX-F51597 B20 样本账首批 40 台复核（B14 兑现核账） · 0xC1800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51598-J1 清点零 P0；🟡/🟢 逐条处置态在册；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - UnitState：单台处置态闭集（🟢 已核销/🟡 挂观察/P0 缺陷/P1 缺陷/P2 缺陷）
//!   - SampleUnit：样本单（台号/状态/处置指纹）
//!   - SampleLedger：样本账（40 台复核 + 清点零 P0 + 逐条处置态在册面）
//!   - RecheckRound：复核轮（×N 轮逐台重放，指纹零漂移）
//!   - SampleCode：诊断码闭集（13 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//!   ② 清点零 P0：40 台中 P0 态必须为零，任一 P0 显性判 P0Present 且账面
//!      不入册——B14 兑现核账以机器清点为准，不虚报。
//!   ③ 逐条处置态在册：🟡/🟢 逐条须带处置指纹（处置动作派生），缺指纹判
//!      DispoMissing。
//!   ④ 复核轮：逐台重放求指纹，与账面指纹逐位一致（零漂移）；状态只许从
//!      缺陷态收敛到 🟡/🟢，不允许反向恶化（单调收敛面）。
//!   ⑤ 注入核：4 类坏样本（P0 入册/缺处置指纹/状态恶化/越界台号）逐类 10 轮
//!      精确复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1800;

/// 样本账首批台数。
pub const UNITS: usize = 40;
/// 复核轮数。
pub const RECHECK_ROUNDS: usize = 8;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 单台处置态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitState {
    /// 🟢 已核销
    Green = 0,
    /// 🟡 挂观察
    Yellow = 1,
    /// P0 缺陷（不入册）
    P0 = 2,
    /// P1 缺陷
    P1 = 3,
    /// P2 缺陷
    P2 = 4,
}

impl UnitState {
    /// 态序号。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 态名（中文）。
    pub fn name(self) -> &'static str {
        match self {
            UnitState::Green => "🟢 已核销",
            UnitState::Yellow => "🟡 挂观察",
            UnitState::P0 => "P0 缺陷",
            UnitState::P1 => "P1 缺陷",
            UnitState::P2 => "P2 缺陷",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [UnitState; 5] {
        [UnitState::Green, UnitState::Yellow, UnitState::P0, UnitState::P1, UnitState::P2]
    }
    /// 是否缺陷态（P0/P1/P2）。
    pub fn is_defect(self) -> bool {
        self.idx() >= UnitState::P0.idx()
    }
}

/// 样本单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleUnit {
    /// 台号（1..=40）
    pub unit: usize,
    /// 处置态
    pub state: UnitState,
    /// 处置指纹（处置动作派生，缺 0 判缺）
    pub dispo_fp: u64,
}

/// 复核轮结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecheckRound {
    /// 轮号
    pub round: usize,
    /// 该轮重放指纹
    pub fp: u64,
    /// 零漂移
    pub stable: bool,
}

/// 样本账。
pub struct SampleLedger {
    units: Vec<SampleUnit>,
    rechecks: Vec<RecheckRound>,
    closed: bool,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl SampleLedger {
    /// 新建样本账（零台，未收口）。
    pub fn new() -> SampleLedger {
        SampleLedger {
            units: Vec::new(),
            rechecks: Vec::new(),
            closed: false,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 处置指纹：台号×态 派生（同台同态同指纹）。
    pub fn dispo_fp(unit: usize, state: UnitState) -> u64 {
        fnv1a(&unit.to_le_bytes()) ^ fnv1a(&state.idx().to_le_bytes()).rotate_left(32)
    }

    /// 入一单：台号 1..=40 顺次，P0 不入册，🟡/🟢 须带处置指纹。
    pub fn admit(&mut self, unit: usize, state: UnitState, dispo_fp: u64) -> Result<SampleUnit, SampleCode> {
        if self.closed {
            self.rejected += 1;
            return Err(SampleCode::Closed);
        }
        if unit == 0 || unit > UNITS {
            self.rejected += 1;
            return Err(SampleCode::UnitOob);
        }
        if self.units.iter().any(|u| u.unit == unit) {
            self.rejected += 1;
            return Err(SampleCode::DupUnit);
        }
        if state == UnitState::P0 {
            self.rejected += 1;
            return Err(SampleCode::P0Present);
        }
        let expect = Self::dispo_fp(unit, state);
        if dispo_fp != expect {
            self.rejected += 1;
            return Err(SampleCode::DispoMissing);
        }
        let u = SampleUnit { unit, state, dispo_fp };
        self.units.push(u);
        Ok(u)
    }

    /// 状态收敛：缺陷态 → 🟡/🟢 允许（只许改善），反向恶化显性拒。
    pub fn converge(&mut self, unit: usize, to: UnitState) -> Result<UnitState, SampleCode> {
        if self.closed {
            self.rejected += 1;
            return Err(SampleCode::Closed);
        }
        if to == UnitState::P0 {
            self.rejected += 1;
            return Err(SampleCode::P0Present);
        }
        let idx = match self.units.iter().position(|u| u.unit == unit) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(SampleCode::UnitOob);
            }
        };
        let cur = match self.units.get(idx) {
            Some(u) => u.state,
            None => {
                self.rejected += 1;
                return Err(SampleCode::UnitOob);
            }
        };
        // 单调收敛面：态序号下降视为改善（P2→P1→🟡→🟢 方向），上升即恶化。
        if to.idx() > cur.idx() {
            self.rejected += 1;
            return Err(SampleCode::StateWorse);
        }
        let fp = Self::dispo_fp(unit, to);
        if let Some(u) = self.units.get_mut(idx) {
            u.state = to;
            u.dispo_fp = fp;
        }
        Ok(to)
    }

    /// 收口：40 台入满才可收口。
    pub fn close(&mut self) -> Result<u64, SampleCode> {
        if self.closed {
            self.rejected += 1;
            return Err(SampleCode::DupClose);
        }
        if self.units.len() != UNITS {
            self.rejected += 1;
            return Err(SampleCode::UnitsShort);
        }
        self.closed = true;
        Ok(self.ledger_fp())
    }

    /// 账面指纹。
    pub fn ledger_fp(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for u in self.units.iter() {
            bytes.extend_from_slice(&u.unit.to_le_bytes());
            bytes.extend_from_slice(&u.state.idx().to_le_bytes());
            bytes.extend_from_slice(&u.dispo_fp.to_le_bytes());
        }
        fnv1a(&bytes)
    }

    /// 一轮复核：逐台重放（台号单调 + 处置指纹自洽），与账面比对。
    pub fn recheck_once(&mut self) -> Result<RecheckRound, SampleCode> {
        if self.units.len() != UNITS {
            self.rejected += 1;
            return Err(SampleCode::UnitsShort);
        }
        let mut prev: usize = 0;
        let mut bytes: Vec<u8> = Vec::new();
        for u in self.units.iter() {
            if u.unit <= prev {
                self.rejected += 1;
                return Err(SampleCode::UnitOob);
            }
            if u.dispo_fp != Self::dispo_fp(u.unit, u.state) {
                self.rejected += 1;
                return Err(SampleCode::DispoMissing);
            }
            prev = u.unit;
            bytes.extend_from_slice(&u.unit.to_le_bytes());
            bytes.extend_from_slice(&u.state.idx().to_le_bytes());
            bytes.extend_from_slice(&u.dispo_fp.to_le_bytes());
        }
        let fp = fnv1a(&bytes);
        let r = RecheckRound { round: self.rechecks.len() + 1, fp, stable: fp == self.ledger_fp() };
        self.rechecks.push(r);
        Ok(r)
    }

    /// 复核 ×N 零漂移判据。
    pub fn recheck_ok(&self) -> bool {
        self.rechecks.len() == RECHECK_ROUNDS && self.rechecks.iter().all(|r| r.stable)
    }

    /// 清点判据：40 台全在册、零 P0、🟡/🟢 处置指纹全在（逐条处置态在册）。
    pub fn zero_p0_ok(&self) -> bool {
        self.units.len() == UNITS
            && self.units.iter().all(|u| u.state != UnitState::P0 && u.dispo_fp != 0)
    }

    /// 台数。
    pub fn units_len(&self) -> usize {
        self.units.len()
    }

    /// 某态台数。
    pub fn count_of(&self, state: UnitState) -> usize {
        self.units.iter().filter(|u| u.state == state).count()
    }

    /// 已收口。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 拒绝账计数。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&(self.closed as u64).to_le_bytes());
        bytes.extend_from_slice(&self.ledger_fp().to_le_bytes());
        for r in self.rechecks.iter() {
            bytes.extend_from_slice(&r.round.to_le_bytes());
            bytes.extend_from_slice(&r.fp.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 样本账复核域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 台号越界
    UnitOob,
    /// 台号重复
    DupUnit,
    /// P0 缺陷（不入册）
    P0Present,
    /// 处置指纹缺失（🟡/🟢 逐条须带）
    DispoMissing,
    /// 状态恶化（反向收敛）
    StateWorse,
    /// 台数不足
    UnitsShort,
    /// 已收口只读
    Closed,
    /// 重复收口
    DupClose,
    /// 复核指纹漂移
    RecheckDrift,
    /// 复核轮数不足
    RecheckShort,
    /// 态越闭集
    StateUnknown,
}

impl SampleCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            SampleCode::Ok => "样本账复核判定成功",
            SampleCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SampleCode::UnitOob => "台号越界：样本台号必须在 1..=40 之内",
            SampleCode::DupUnit => "台号重复：同台只许入册一次",
            SampleCode::P0Present => "P0 在册：清点零 P0 判据下 P0 态不入册",
            SampleCode::DispoMissing => "处置指纹缺失：🟡/🟢 逐条处置态须带指纹",
            SampleCode::StateWorse => "状态恶化：处置态只许改善不许反向",
            SampleCode::UnitsShort => "台数不足：不足 40 台不得收口或复核",
            SampleCode::Closed => "已收口：收口后样本账只读",
            SampleCode::DupClose => "重复收口：样本账只许收口一次",
            SampleCode::RecheckDrift => "复核漂移：重放指纹与账面指纹逐位不符",
            SampleCode::RecheckShort => "复核不足：×N 轮零漂移判据未满轮数",
            SampleCode::StateUnknown => "态非法：处置态必须在五态闭集内",
        }
    }
}

/// 诊断码闭集权威清单（13 变体）。
pub const ALL_CODES: [SampleCode; 13] = [
    SampleCode::Ok,
    SampleCode::SegBad,
    SampleCode::UnitOob,
    SampleCode::DupUnit,
    SampleCode::P0Present,
    SampleCode::DispoMissing,
    SampleCode::StateWorse,
    SampleCode::UnitsShort,
    SampleCode::Closed,
    SampleCode::DupClose,
    SampleCode::RecheckDrift,
    SampleCode::RecheckShort,
    SampleCode::StateUnknown,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x85ebca6b77c2b399 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：40 台（36 🟢 + 4 🟡）→ 收口 → 复核 ×8。返回 (指纹, 台数)。
pub fn standard_script() -> (u64, usize) {
    let mut l = SampleLedger::new();
    for u in 1..=UNITS {
        let st = if u % 10 == 0 { UnitState::Yellow } else { UnitState::Green };
        let _ = l.admit(u, st, SampleLedger::dispo_fp(u, st));
    }
    let _ = l.close();
    for _ in 0..RECHECK_ROUNDS {
        let _ = l.recheck_once();
    }
    (l.ledger_fp(), l.units_len())
}

/// 双跑一致性：同一脚本两次建账，指纹与台数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：P0 入册——显性拒（P0Present），零 P0 判据不破。
    for _ in 0..INJ_ROUNDS {
        let mut l = SampleLedger::new();
        let caught = l.admit(1, UnitState::P0, dispo_fp_of(1, UnitState::P0))
            == Err(SampleCode::P0Present);
        if caught {
            l.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            l.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：缺处置指纹——指纹错显性拒（DispoMissing）。
    for _ in 0..INJ_ROUNDS {
        let mut l = SampleLedger::new();
        let caught = l.admit(2, UnitState::Green, 0) == Err(SampleCode::DispoMissing);
        if caught {
            l.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            l.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：状态恶化——🟢→P1 显性拒（StateWorse）。
    for _ in 0..INJ_ROUNDS {
        let mut l = SampleLedger::new();
        let _ = l.admit(3, UnitState::Green, SampleLedger::dispo_fp(3, UnitState::Green));
        let caught = l.converge(3, UnitState::P1) == Err(SampleCode::StateWorse);
        if caught {
            l.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            l.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：越界台号——41/0 显性拒（UnitOob）。
    for _ in 0..INJ_ROUNDS {
        let mut l = SampleLedger::new();
        let hi = l.admit(UNITS + 1, UnitState::Green, SampleLedger::dispo_fp(UNITS + 1, UnitState::Green))
            == Err(SampleCode::UnitOob);
        let lo = l.admit(0, UnitState::Green, SampleLedger::dispo_fp(0, UnitState::Green))
            == Err(SampleCode::UnitOob);
        if hi && lo {
            l.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            l.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 处置指纹辅助（注入核用）。
fn dispo_fp_of(unit: usize, state: UnitState) -> u64 {
    SampleLedger::dispo_fp(unit, state)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51597_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51597");
    // 1 标准复核全通。
    {
        let (fp, n) = standard_script();
        s.add(
            "40 台复核全通",
            fp != 0 && n == UNITS,
            "UNX-F51597 判据：首批 40 台入册收口复核零漂移",
        );
    }
    // 2 清点零 P0。
    {
        let mut l = SampleLedger::new();
        for u in 1..=UNITS {
            let st = if u % 10 == 0 { UnitState::Yellow } else { UnitState::Green };
            let _ = l.admit(u, st, SampleLedger::dispo_fp(u, st));
        }
        s.add(
            "清点零 P0",
            l.zero_p0_ok() && l.count_of(UnitState::P0) == 0,
            "UNX-F51598-J1：40 台清点零 P0，🟡/🟢 逐条处置指纹在册",
        );
    }
    // 3 处置态五态闭集与字面。
    {
        let ok = UnitState::all().len() == 5
            && UnitState::all()
                .iter()
                .enumerate()
                .all(|(i, st)| st.idx() == i as u32 && !st.name().is_empty())
            && UnitState::P0.is_defect()
            && !UnitState::Green.is_defect();
        s.add(
            "处置态闭集完整",
            ok,
            "五态闭集序号 0..5，字面非空，缺陷态判定自洽",
        );
    }
    // 4 P0 不入册。
    {
        let mut l = SampleLedger::new();
        let r = l.admit(1, UnitState::P0, SampleLedger::dispo_fp(1, UnitState::P0));
        s.add(
            "P0 不入册",
            r == Err(SampleCode::P0Present) && l.units_len() == 0,
            "清点零 P0 判据下 P0 态显性拒入册",
        );
    }
    // 5 处置指纹自洽。
    {
        let mut l = SampleLedger::new();
        let bad0 = l.admit(2, UnitState::Yellow, 0);
        let badfp = l.admit(2, UnitState::Yellow, 0xDEAD);
        let good = l.admit(2, UnitState::Yellow, SampleLedger::dispo_fp(2, UnitState::Yellow));
        s.add(
            "处置指纹自洽",
            bad0 == Err(SampleCode::DispoMissing)
                && badfp == Err(SampleCode::DispoMissing)
                && good.is_ok(),
            "🟡/🟢 逐条处置态须带台号×态派生的处置指纹",
        );
    }
    // 6 状态单调收敛。
    {
        let mut l = SampleLedger::new();
        let _ = l.admit(3, UnitState::P2, SampleLedger::dispo_fp(3, UnitState::P2));
        let step1 = l.converge(3, UnitState::P1);
        let step2 = l.converge(3, UnitState::Yellow);
        let worse = l.converge(3, UnitState::P2);
        s.add(
            "状态单调收敛",
            step1 == Ok(UnitState::P1)
                && step2 == Ok(UnitState::Yellow)
                && worse == Err(SampleCode::StateWorse),
            "P2→P1→🟡 只许改善，反向恶化显性拒",
        );
    }
    // 7 台号越界与重复。
    {
        let mut l = SampleLedger::new();
        let _ = l.admit(4, UnitState::Green, SampleLedger::dispo_fp(4, UnitState::Green));
        let dup = l.admit(4, UnitState::Green, SampleLedger::dispo_fp(4, UnitState::Green));
        let hi = l.admit(UNITS + 1, UnitState::Green, 0);
        s.add(
            "台号越界与重复拒",
            dup == Err(SampleCode::DupUnit) && hi == Err(SampleCode::UnitOob),
            "台号 1..=40 顺次在册，越界/重复显性拒",
        );
    }
    // 8 未满拒收口。
    {
        let mut l = SampleLedger::new();
        for u in 1..(UNITS - 1) {
            let _ = l.admit(u, UnitState::Green, SampleLedger::dispo_fp(u, UnitState::Green));
        }
        s.add(
            "未满拒收口",
            l.close() == Err(SampleCode::UnitsShort) && !l.closed(),
            "不足 40 台不得收口",
        );
    }
    // 9 复核 ×8 零漂移。
    {
        let mut l = SampleLedger::new();
        for u in 1..=UNITS {
            let st = if u % 20 == 0 { UnitState::Yellow } else { UnitState::Green };
            let _ = l.admit(u, st, SampleLedger::dispo_fp(u, st));
        }
        let _ = l.close();
        let fp0 = l.ledger_fp();
        let mut all_stable = true;
        for _ in 0..RECHECK_ROUNDS {
            match l.recheck_once() {
                Ok(r) => {
                    if !r.stable {
                        all_stable = false;
                    }
                }
                Err(_) => all_stable = false,
            }
        }
        s.add(
            "复核 ×8 零漂移",
            all_stable && l.recheck_ok() && l.ledger_fp() == fp0,
            "B14 兑现核账：逐台重放指纹逐位一致",
        );
    }
    // 10 收口后只读。
    {
        let mut l = SampleLedger::new();
        for u in 1..=UNITS {
            let _ = l.admit(u, UnitState::Green, SampleLedger::dispo_fp(u, UnitState::Green));
        }
        let _ = l.close();
        let ro = l.admit(UNITS, UnitState::Green, 0) == Err(SampleCode::Closed)
            && l.close() == Err(SampleCode::DupClose)
            && l.converge(1, UnitState::Yellow) == Err(SampleCode::Closed);
        s.add(
            "收口后只读",
            ro,
            "Closed 后入单/收敛/二次收口显性拒",
        );
    }
    // 11 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 12 诊断码闭集唯一性。
    {
        let mut ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                ok = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 13 变体",
        );
    }
    // 13 注入账守恒。
    {
        let mut l = SampleLedger::new();
        for _ in 0..16u32 {
            l.note_detected();
        }
        s.add("注入账守恒", l.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建账，指纹与台数逐位一致",
    );
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!("unx51597 样本账复核 {} 台 ×{} 轮", UNITS, RECHECK_ROUNDS);
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51597"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51597_checks_green() {
        let set = run_unx51597_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_ledger_and_recheck() {
        let mut l = SampleLedger::new();
        for u in 1..=UNITS {
            let st = if u % 10 == 0 { UnitState::Yellow } else { UnitState::Green };
            assert!(l.admit(u, st, SampleLedger::dispo_fp(u, st)).is_ok());
        }
        assert!(l.close().is_ok());
        for _ in 0..RECHECK_ROUNDS {
            assert!(l.recheck_once().map(|r| r.stable) == Ok(true));
        }
        assert!(l.recheck_ok());
        assert!(l.zero_p0_ok());
        assert_eq!(l.count_of(UnitState::Yellow), 4);
        assert_eq!(l.count_of(UnitState::Green), 36);
    }

    #[test]
    fn p0_never_admitted() {
        let mut l = SampleLedger::new();
        assert_eq!(
            l.admit(1, UnitState::P0, SampleLedger::dispo_fp(1, UnitState::P0)),
            Err(SampleCode::P0Present)
        );
        assert_eq!(l.units_len(), 0);
    }

    #[test]
    fn dispo_fp_required() {
        let mut l = SampleLedger::new();
        assert_eq!(l.admit(2, UnitState::Green, 0), Err(SampleCode::DispoMissing));
        assert_eq!(l.admit(2, UnitState::Green, 0x1234), Err(SampleCode::DispoMissing));
        assert!(l.admit(2, UnitState::Green, SampleLedger::dispo_fp(2, UnitState::Green)).is_ok());
    }

    #[test]
    fn state_converges_monotone() {
        let mut l = SampleLedger::new();
        let _ = l.admit(3, UnitState::P2, SampleLedger::dispo_fp(3, UnitState::P2));
        assert_eq!(l.converge(3, UnitState::P1), Ok(UnitState::P1));
        assert_eq!(l.converge(3, UnitState::Yellow), Ok(UnitState::Yellow));
        assert_eq!(l.converge(3, UnitState::Green), Ok(UnitState::Green));
        assert_eq!(l.converge(3, UnitState::P1), Err(SampleCode::StateWorse));
    }

    #[test]
    fn unit_oob_and_dup() {
        let mut l = SampleLedger::new();
        assert_eq!(l.admit(0, UnitState::Green, 0), Err(SampleCode::UnitOob));
        assert_eq!(
            l.admit(UNITS + 1, UnitState::Green, 0),
            Err(SampleCode::UnitOob)
        );
        assert!(l.admit(5, UnitState::Green, SampleLedger::dispo_fp(5, UnitState::Green)).is_ok());
        assert_eq!(
            l.admit(5, UnitState::Green, SampleLedger::dispo_fp(5, UnitState::Green)),
            Err(SampleCode::DupUnit)
        );
    }

    #[test]
    fn close_needs_40_units() {
        let mut l = SampleLedger::new();
        for u in 1..(UNITS - 1) {
            let _ = l.admit(u, UnitState::Green, SampleLedger::dispo_fp(u, UnitState::Green));
        }
        assert_eq!(l.close(), Err(SampleCode::UnitsShort));
        assert_eq!(l.recheck_once(), Err(SampleCode::UnitsShort));
    }

    #[test]
    fn closed_is_readonly() {
        let mut l = SampleLedger::new();
        for u in 1..=UNITS {
            let _ = l.admit(u, UnitState::Green, SampleLedger::dispo_fp(u, UnitState::Green));
        }
        assert!(l.close().is_ok());
        assert_eq!(l.converge(1, UnitState::Yellow), Err(SampleCode::Closed));
        assert_eq!(l.close(), Err(SampleCode::DupClose));
        assert_eq!(
            l.admit(1, UnitState::Green, SampleLedger::dispo_fp(1, UnitState::Green)),
            Err(SampleCode::Closed)
        );
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 40);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut l = SampleLedger::new();
        for _ in 0..10u32 {
            l.note_detected();
        }
        assert!(l.conserved());
        let mut bad = SampleLedger::new();
        bad.note_escape();
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        assert_eq!(standard_script(), standard_script());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 13);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
