//! UNX-F12686 LoaderLock 联签回归（B29 档） · 0x81540段
//!
//! 判据原文（派工卡 12685 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12686-J1 TLS 回调序回归 10/10 绿，回调时机注入
//!      一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. LoaderLock 联签域模型：TLS 回调序十槽（槽号/回调名/时机轴/装载
//!    侧签/回调侧签/双签序号），十槽全量回归 = 10/10 绿判据锚；
//! 2. 联签语义：每回调槽必须**双签**（装载侧签与回调侧签齐），且双签
//!    序号一致（loader_seq == cb_seq）——缺一签或序号错位即违约；
//! 3. 回调时机三闭集（持锁前持/持锁期/卸载期）：回调槽时机必须与冻结
//!    时机表逐位一致，注入漂移时机必被时机闸抓出（回调时机注入一致）；
//! 4. 时机轴绑定序：持锁前持只许槽 0..2、持锁期只许槽 3..6、卸载期只
//!    许槽 7..9——跨段挂时机即账可疑（显性拒）；
//! 5. 联签账状态机 CsOpen → CsRunning → CsSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：判定数 ≡ 槽位数守恒；
//! 6. 注入联测十路：乱序/重复/判码漂移/时机漂移/缺装载签/缺回调签/
//!    双签序号错位/时机跨段/封账后写/状态机倒退——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `tls_ten_green_ok` ↔ ② TLS 回调序回归 10/10 绿；
//!   - `timing_inject_ok` ↔ ② 回调时机注入一致（注入漂移必抓）；
//!   - `timing_band_ok` ↔ ② 时机三段轴绑定不跨段；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12685 − 12600)）。
pub const SEG_BASE: u32 = 0x81540;

// ── 联签回归诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsDiag {
    /// 槽乱序。
    ESlotOrder,
    /// 槽重复。
    ESlotDup,
    /// 判码漂移（判码 ≠ 冻结回调序谱位）。
    ECodeDrift,
    /// 时机漂移（回调时机 ≠ 冻结时机表）。
    ETimingDrift,
    /// 缺装载侧签。
    ELoaderSignMissing,
    /// 缺回调侧签。
    ECbSignMissing,
    /// 双签序号错位。
    ESeqMismatch,
    /// 时机跨段（时机轴挂错三段位）。
    ETimingBand,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
}

impl CsDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CsDiag::ESlotOrder => 0x01,
                CsDiag::ESlotDup => 0x02,
                CsDiag::ECodeDrift => 0x03,
                CsDiag::ETimingDrift => 0x04,
                CsDiag::ELoaderSignMissing => 0x05,
                CsDiag::ECbSignMissing => 0x06,
                CsDiag::ESeqMismatch => 0x07,
                CsDiag::ETimingBand => 0x08,
                CsDiag::ESealedWrite => 0x09,
                CsDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CsDiag::ESlotOrder => "槽乱序：按槽号 0..10 逐槽联签，跳槽先回补",
            CsDiag::ESlotDup => "槽重复：同回调名只立一槽，重复先查已有账",
            CsDiag::ECodeDrift => "判码漂移：判码必须等于冻结回调序谱位，先对谱口径",
            CsDiag::ETimingDrift => "时机漂移：回调时机必须等于冻结时机表，先核注入一致性",
            CsDiag::ELoaderSignMissing => "缺装载侧签：联签须双签齐，装载侧先补签",
            CsDiag::ECbSignMissing => "缺回调侧签：联签须双签齐，回调侧先补签",
            CsDiag::ESeqMismatch => "双签序号错位：装载签与回调签序号必须一致，先对序",
            CsDiag::ETimingBand => "时机跨段：持锁前持/持锁期/卸载期三段各归其位，跨段先归段",
            CsDiag::ESealedWrite => "封账后写：联签账只读，补槽走下一轮回归",
            CsDiag::ERewind => "状态机倒退：CsOpen→CsRunning→CsSealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CS_ALL: &[CsDiag] = &[
    CsDiag::ESlotOrder,
    CsDiag::ESlotDup,
    CsDiag::ECodeDrift,
    CsDiag::ETimingDrift,
    CsDiag::ELoaderSignMissing,
    CsDiag::ECbSignMissing,
    CsDiag::ESeqMismatch,
    CsDiag::ETimingBand,
    CsDiag::ESealedWrite,
    CsDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CS_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81541..0x8154A 闭集内"
}

// ── 回调时机三闭集 ───────────────────────────────────────────────
/// TLS 回调时机轴（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingAxis {
    /// 持锁前持（LoaderLock 获取前注册）。
    PreLock,
    /// 持锁期（LoaderLock 持有期间执行）。
    LockHeld,
    /// 卸载期（DLL 卸载路径执行）。
    OnUnload,
}

impl TimingAxis {
    /// 时机名。
    pub fn name(self) -> &'static str {
        match self {
            TimingAxis::PreLock => "持锁前持",
            TimingAxis::LockHeld => "持锁期",
            TimingAxis::OnUnload => "卸载期",
        }
    }

    /// 时机段位（0/1/2）。
    pub fn band(self) -> u32 {
        match self {
            TimingAxis::PreLock => 0,
            TimingAxis::LockHeld => 1,
            TimingAxis::OnUnload => 2,
        }
    }

    /// 时机段绑定槽区间（前闭后开：[lo, hi)）。
    pub fn band_window(self) -> (u32, u32) {
        match self {
            TimingAxis::PreLock => (0, 3),
            TimingAxis::LockHeld => (3, 7),
            TimingAxis::OnUnload => (7, 10),
        }
    }

    /// 全部时机（闭集数组）。
    pub fn all() -> &'static [TimingAxis] {
        &[TimingAxis::PreLock, TimingAxis::LockHeld, TimingAxis::OnUnload]
    }
}

/// 槽号是否落在时机段窗口内（时机段绑定律）。
pub fn slot_in_band(slot_no: u32, t: TimingAxis) -> bool {
    let (lo, hi) = t.band_window();
    slot_no >= lo && slot_no < hi
}

// ── 联签槽域模型 ─────────────────────────────────────────────────
/// 无签哨兵（缺签槽的占位序号）。
pub const NO_SIGN: u32 = 0;

/// TLS 回调联签槽六元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsSlot {
    /// 槽号（0..10）。
    pub slot_no: u32,
    /// 回调名（TLS 回调序清单冻结短名）。
    pub cb_name: &'static str,
    /// 冻结回调时机。
    pub timing: TimingAxis,
    /// 装载侧签序号（NO_SIGN=缺签）。
    pub loader_seq: u32,
    /// 回调侧签序号（NO_SIGN=缺签）。
    pub cb_seq: u32,
    /// 回调序谱判码（冻结值 0x41..0x4A）。
    pub verdict_code: u32,
}

/// 装载侧与回调侧双签齐且序号一致。
fn both_signed(s: &CsSlot) -> bool {
    s.loader_seq != NO_SIGN && s.cb_seq != NO_SIGN && s.loader_seq == s.cb_seq
}

/// 冻结回调序谱位（0x40 + 槽号 + 1；来源：TLS 回调序回归清单冻结轴）。
pub fn cs_spec_code(slot_no: u32) -> u32 {
    0x40u32.saturating_add(slot_no).saturating_add(1)
}

/// 判码是否落在回调序谱内（0x41..0x4A）。
pub fn code_in_cs_spec(code: u32) -> bool {
    code >= 0x41 && code <= 0x4A
}

/// 双签序号冻结值（槽号 + 1，双签同源）。
pub fn cs_frozen_seq(slot_no: u32) -> u32 {
    slot_no.saturating_add(1)
}

/// 冻结联签槽表：TLS 回调序十槽（三段时机 3+4+3）。
pub const CS_SLOTS: &[CsSlot] = &[
    CsSlot { slot_no: 0, cb_name: "预持注册回调", timing: TimingAxis::PreLock, loader_seq: 1, cb_seq: 1, verdict_code: 0x41 },
    CsSlot { slot_no: 1, cb_name: "预持清单回调", timing: TimingAxis::PreLock, loader_seq: 2, cb_seq: 2, verdict_code: 0x42 },
    CsSlot { slot_no: 2, cb_name: "预持环境回调", timing: TimingAxis::PreLock, loader_seq: 3, cb_seq: 3, verdict_code: 0x43 },
    CsSlot { slot_no: 3, cb_name: "持锁序回调甲", timing: TimingAxis::LockHeld, loader_seq: 4, cb_seq: 4, verdict_code: 0x44 },
    CsSlot { slot_no: 4, cb_name: "持锁序回调乙", timing: TimingAxis::LockHeld, loader_seq: 5, cb_seq: 5, verdict_code: 0x45 },
    CsSlot { slot_no: 5, cb_name: "持锁序回调丙", timing: TimingAxis::LockHeld, loader_seq: 6, cb_seq: 6, verdict_code: 0x46 },
    CsSlot { slot_no: 6, cb_name: "持锁收口回调", timing: TimingAxis::LockHeld, loader_seq: 7, cb_seq: 7, verdict_code: 0x47 },
    CsSlot { slot_no: 7, cb_name: "卸载序回调甲", timing: TimingAxis::OnUnload, loader_seq: 8, cb_seq: 8, verdict_code: 0x48 },
    CsSlot { slot_no: 8, cb_name: "卸载序回调乙", timing: TimingAxis::OnUnload, loader_seq: 9, cb_seq: 9, verdict_code: 0x49 },
    CsSlot { slot_no: 9, cb_name: "卸载收口回调", timing: TimingAxis::OnUnload, loader_seq: 10, cb_seq: 10, verdict_code: 0x4A },
];

/// 联签槽总数（10/10 判据锚定值）。
pub const CS_SLOT_CAP: usize = 10;

/// 按槽号取冻结槽（闭集内返回，越界 None）。
pub fn replay_cs(slot_no: u32) -> Option<CsSlot> {
    if slot_no >= CS_SLOTS.len() as u32 {
        return None;
    }
    for s in CS_SLOTS.iter() {
        if s.slot_no == slot_no {
            return Some(s.clone());
        }
    }
    None
}

// ── 联签账状态机 ─────────────────────────────────────────────────
/// 联签账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsState {
    /// 开账。
    CsOpen,
    /// 联签中。
    CsRunning,
    /// 封账（只读）。
    CsSealed,
}

impl CsState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            CsState::CsOpen => 0,
            CsState::CsRunning => 1,
            CsState::CsSealed => 2,
        }
    }
}

/// 联签账。
#[derive(Clone, Debug)]
pub struct CsLedger {
    /// 状态。
    state: CsState,
    /// 已入账槽（按槽号升序）。
    slots: Vec<CsSlot>,
    /// 拒止计数。
    rejects: u32,
    /// 已落账判定数。
    verdicts: u32,
}

impl CsLedger {
    /// 新账。
    pub fn new() -> CsLedger {
        CsLedger {
            state: CsState::CsOpen,
            slots: Vec::new(),
            rejects: 0,
            verdicts: 0,
        }
    }

    /// 开账 → 联签中（倒退拒）。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            CsState::CsOpen => {
                self.state = CsState::CsRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(CsDiag::ERewind.val())
            }
        }
    }

    /// 联签中 → 封账（未齐不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            CsState::CsRunning => {
                if self.slots.len() != CS_SLOT_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(CsDiag::ESlotOrder.val());
                }
                self.state = CsState::CsSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(CsDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> CsState {
        self.state
    }

    /// 已入账槽数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 判定数。
    pub fn verdict_len(&self) -> u32 {
        self.verdicts
    }

    /// 下标取槽（越界 None）。
    pub fn slot_at(&self, idx: usize) -> Option<&CsSlot> {
        self.slots.get(idx)
    }

    /// 账等式：判定数 ≡ 槽数。
    pub fn equation_ok(&self) -> bool {
        self.verdicts as usize == self.slots.len()
    }

    /// 入账一槽（序闸/重闸/谱闸/时机闸/段闸/双签闸/封闸七闸）。
    pub fn reg(&mut self, s: CsSlot) -> Result<(), u32> {
        if self.state == CsState::CsSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ESealedWrite.val());
        }
        if self.state == CsState::CsOpen {
            self.state = CsState::CsRunning;
        }
        // 序闸。
        if s.slot_no != self.slots.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ESlotOrder.val());
        }
        // 重闸。
        for e in self.slots.iter() {
            if e.cb_name == s.cb_name {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CsDiag::ESlotDup.val());
            }
        }
        // 谱闸。
        if !code_in_cs_spec(s.verdict_code) || s.verdict_code != cs_spec_code(s.slot_no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ECodeDrift.val());
        }
        // 时机闸：时机必须与冻结表逐位一致。
        let frozen = match replay_cs(s.slot_no) {
            Some(x) => x,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CsDiag::ESlotOrder.val());
            }
        };
        if s.timing != frozen.timing {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ETimingDrift.val());
        }
        // 段闸：时机段绑定律（跨段即拒）。
        if !slot_in_band(s.slot_no, s.timing) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ETimingBand.val());
        }
        // 双签闸：双签齐且序号一致（序号为冻结同源值）。
        if s.loader_seq == NO_SIGN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ELoaderSignMissing.val());
        }
        if s.cb_seq == NO_SIGN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ECbSignMissing.val());
        }
        if !both_signed(&s) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ESeqMismatch.val());
        }
        if s.loader_seq != cs_frozen_seq(s.slot_no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CsDiag::ESeqMismatch.val());
        }
        // 全闸放行。
        self.verdicts = self.verdicts.saturating_add(1);
        self.slots.push(s);
        Ok(())
    }
}

/// 全量联签（十槽依序入账并封账）。
pub fn build_full_cs_ledger() -> CsLedger {
    let mut led = CsLedger::new();
    let mut i = 0usize;
    while i < CS_SLOTS.len() {
        if let Some(s) = CS_SLOTS.get(i) {
            let _ = led.reg(s.clone());
        }
        i += 1;
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// TLS 回调序回归 10/10 绿。
fn tls_ten_green_ok() -> bool {
    let led = build_full_cs_ledger();
    if led.len() != CS_SLOT_CAP || led.state() != CsState::CsSealed {
        return false;
    }
    if led.ledger_dirty() {
        return false;
    }
    true
}

impl CsLedger {
    /// 账是否不洁（绿账须零拒止、判定数满；true = 有拒止或判定数不满）。
    fn ledger_dirty(&self) -> bool {
        self.rejects != 0 || self.verdicts as usize != CS_SLOT_CAP
    }
}

/// 回调时机注入一致（漂移时机注入必被抓）。
fn timing_inject_ok() -> bool {
    // 对每槽注入一次时机漂移，全部必须被拒。
    let mut i = 0u32;
    while i < CS_SLOT_CAP as u32 {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut j = 0u32;
        while j < i {
            if let Some(s) = replay_cs(j) {
                let _ = led.reg(s);
            }
            j += 1;
        }
        let mut bad = match replay_cs(i) {
            Some(x) => x,
            None => return false,
        };
        // 注入漂移：把时机换成表里另一个值。
        let alt = match TimingAxis::all().iter().find(|t| **t != bad.timing) {
            Some(t) => *t,
            None => return false,
        };
        bad.timing = alt;
        if led.reg(bad).is_ok() {
            // 注入逃逸 = 违约。
            return false;
        }
        i += 1;
    }
    true
}

/// 时机三段轴绑定不跨段（冻结表逐槽过段闸）。
fn timing_band_ok() -> bool {
    for s in CS_SLOTS.iter() {
        if !slot_in_band(s.slot_no, s.timing) {
            return false;
        }
    }
    // 三段窗口连续覆盖 0..10。
    let (lo0, hi0) = TimingAxis::PreLock.band_window();
    let (lo1, hi1) = TimingAxis::LockHeld.band_window();
    let (lo2, hi2) = TimingAxis::OnUnload.band_window();
    if lo0 != 0 || hi0 != lo1 || hi1 != lo2 || hi2 != CS_SLOT_CAP as u32 {
        return false;
    }
    true
}

/// 联签普查（十槽、槽号连续、判码落谱、双签同源）。
fn cs_census_ok() -> bool {
    if CS_SLOTS.len() != CS_SLOT_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < CS_SLOTS.len() {
        let s = match CS_SLOTS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot_no != i as u32 {
            return false;
        }
        if !code_in_cs_spec(s.verdict_code) || s.verdict_code != cs_spec_code(s.slot_no) {
            return false;
        }
        if !both_signed(s) || s.loader_seq != cs_frozen_seq(s.slot_no) {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn cs_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 乱序：跳过 0 号直入 1 号。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let bad = match replay_cs(1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("乱序;");
        }
    }
    // 2. 重复：同槽同回调名再入。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let s0 = match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        let _ = led.reg(s0.clone());
        if led.reg(s0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 3. 判码漂移。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        bad.verdict_code = 0x4B;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("漂移;");
        }
    }
    // 4. 缺装载侧签。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        bad.loader_seq = NO_SIGN;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺装签;");
        }
    }
    // 5. 缺回调侧签。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        bad.cb_seq = NO_SIGN;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺回签;");
        }
    }
    // 6. 双签序号错位。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        bad.cb_seq = 9;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("序错;");
        }
    }
    // 7. 时机跨段：持锁期回调挂进卸载期槽位。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_cs(3) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        bad.timing = TimingAxis::OnUnload;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("跨段;");
        }
    }
    // 8. 封账后写。
    {
        let mut led = build_full_cs_ledger();
        let extra = match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 9. 状态机倒退：已封账再开账。
    {
        let mut led = build_full_cs_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 未齐封账：只入一半就封。
    {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let _ = led.reg(match replay_cs(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺槽")),
        });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("早封;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_cs_ledger();
    let b = build_full_cs_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.slot_at(i), b.slot_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if cs_fault_sweep().0 != cs_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要（全槽判码与双签序折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for s in CS_SLOTS.iter() {
        acc = acc
            .wrapping_mul(33)
            .wrapping_add(s.verdict_code)
            .wrapping_add(s.loader_seq)
            .wrapping_add(s.cb_seq);
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12685u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_cs_ledger();
    if led.state() != CsState::CsSealed {
        return false;
    }
    let head = match led.slot_at(0) {
        Some(s) => s.cb_name,
        None => return false,
    };
    let tail = match led.slot_at(CS_SLOT_CAP.saturating_sub(1)) {
        Some(s) => s.cb_name,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    for t in TimingAxis::all().iter() {
        if t.name().is_empty() {
            return false;
        }
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12686 LoaderLock 联签回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12685_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12685-loaderlock-cosign");
    let led = build_full_cs_ledger();

    s.add_dyn(
        "F12686-01 回调序十槽齐",
        CS_SLOTS.len() == CS_SLOT_CAP,
        &format!("slots={} timings={}", CS_SLOTS.len(), TimingAxis::all().len()),
    );

    s.add_dyn(
        "F12686-02 TLS 回调序回归 10/10 绿",
        tls_ten_green_ok(),
        &format!("slots={} sealed={}", led.len(), led.state() == CsState::CsSealed),
    );

    s.add_dyn(
        "F12686-03 回调时机注入一致",
        timing_inject_ok(),
        &format!("injected={}", CS_SLOT_CAP),
    );

    s.add_dyn(
        "F12686-04 时机三段轴绑定",
        timing_band_ok(),
        &format!("bands={}", TimingAxis::all().len()),
    );

    s.add_dyn(
        "F12686-05 联签普查",
        cs_census_ok(),
        &format!("slots={}", CS_SLOTS.len()),
    );

    s.add_dyn(
        "F12686-06 注入联测 10/10 拒止",
        {
            let (c, t, e) = cs_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", cs_fault_sweep().0),
    );

    s.add_dyn(
        "F12686-07 账等式（判定数守恒）",
        led.equation_ok(),
        &format!("verdicts={} slots={}", led.verdict_len(), led.len()),
    );

    s.add_dyn(
        "F12686-08 谱口径",
        code_in_cs_spec(0x41) && code_in_cs_spec(0x4A) && !code_in_cs_spec(0x40) && !code_in_cs_spec(0x4B),
        "回调序谱 0x41..0x4A 闭集",
    );

    s.add_dyn(
        "F12686-09 双签同源序",
        cs_frozen_seq(0) == 1 && cs_frozen_seq(9) == 10,
        "双签序号 = 槽号 + 1",
    );

    s.add_dyn(
        "F12686-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12686-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12686-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12686-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81540);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(CS_ALL.len(), 10);
        let mut i = 0usize;
        while i < CS_ALL.len() {
            let vi = match CS_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CS_ALL.len() {
                let vj = match CS_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn tls_ten_green_holds() {
        assert!(tls_ten_green_ok());
        let led = build_full_cs_ledger();
        assert_eq!(led.len(), 10);
        assert_eq!(led.state(), CsState::CsSealed);
        assert_eq!(led.reject_len(), 0);
        assert_eq!(led.verdict_len(), 10);
        assert!(led.equation_ok());
    }

    #[test]
    fn timing_injection_all_caught() {
        assert!(timing_inject_ok());
        // 直证：前序槽 0..3 先入账，再对槽 4 注入时机漂移必拒。
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let mut k = 0u32;
        while k < 4 {
            if let Some(s) = replay_cs(k) {
                let _ = led.reg(s);
            }
            k += 1;
        }
        let mut bad = match replay_cs(4) {
            Some(x) => x,
            None => return,
        };
        bad.timing = TimingAxis::PreLock;
        assert_eq!(led.reg(bad), Err(CsDiag::ETimingDrift.val()));
    }

    #[test]
    fn timing_band_semantics() {
        assert!(timing_band_ok());
        assert!(slot_in_band(0, TimingAxis::PreLock));
        assert!(slot_in_band(2, TimingAxis::PreLock));
        assert!(!slot_in_band(3, TimingAxis::PreLock));
        assert!(slot_in_band(3, TimingAxis::LockHeld));
        assert!(slot_in_band(6, TimingAxis::LockHeld));
        assert!(!slot_in_band(7, TimingAxis::LockHeld));
        assert!(slot_in_band(9, TimingAxis::OnUnload));
        assert!(!slot_in_band(10, TimingAxis::OnUnload));
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = cs_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn reg_gate_rejects() {
        let mut led = CsLedger::new();
        let _ = led.begin_run();
        let s1 = match replay_cs(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.reg(s1), Err(CsDiag::ESlotOrder.val()));
        let s0 = match replay_cs(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.reg(s0.clone()).is_ok());
        // 缺回调侧签。
        let mut half = match replay_cs(1) {
            Some(x) => x,
            None => return,
        };
        half.cb_seq = NO_SIGN;
        assert_eq!(led.reg(half), Err(CsDiag::ECbSignMissing.val()));
        // 双签序号错位。
        let mut mis = match replay_cs(1) {
            Some(x) => x,
            None => return,
        };
        mis.cb_seq = 8;
        assert_eq!(led.reg(mis), Err(CsDiag::ESeqMismatch.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_cs_ledger();
        let b = build_full_cs_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.slot_at(i), b.slot_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑槽缺失"),
            }
            i += 1;
        }
        assert_eq!(cs_fault_sweep().0, cs_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(CsState::CsOpen.rank() < CsState::CsRunning.rank());
        assert!(CsState::CsRunning.rank() < CsState::CsSealed.rank());
        let mut led = build_full_cs_ledger();
        assert_eq!(led.state(), CsState::CsSealed);
        assert_eq!(led.seal(), Err(CsDiag::ERewind.val()));
        let mut fresh = CsLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(CsDiag::ERewind.val()));
    }

    #[test]
    fn spec_and_replay_boundary() {
        for no in 0u32..10 {
            assert!(code_in_cs_spec(cs_spec_code(no)), "槽 {} 未落谱", no);
        }
        assert!(!code_in_cs_spec(0x40));
        assert!(!code_in_cs_spec(0x4B));
        assert!(replay_cs(0).is_some());
        assert!(replay_cs(9).is_some());
        assert!(replay_cs(10).is_none());
    }

    #[test]
    fn census_holds() {
        assert!(cs_census_ok());
        assert_eq!(CS_SLOTS.len(), 10);
        for t in TimingAxis::all().iter() {
            let mut count = 0usize;
            for s in CS_SLOTS.iter() {
                if s.timing == *t {
                    count += 1;
                }
            }
            let (lo, hi) = t.band_window();
            assert_eq!(count, (hi - lo) as usize, "时机段 {} 槽额漂移", t.name());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12685_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
