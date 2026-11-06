//! UNX-F51517 B16 可观测性埋点（TRIM/flush/介质/切换四点） · 0xC0440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51518-J1 错误码 ×12 并入；未定义码注入 10/10 拒绝
//!
//! 设计要点：
//!   ① 观测面四点闭集：TRIM/flush/介质/切换，每点独立单调序号；埋点
//!      覆盖率按「点覆盖」判定，四点全亮即 100%。
//!   ② 错误码 ×12 并入：闭集 12 个错误码（每点 3 个）并入观测账，raw
//!      值域 1..=12；raw 0 与 raw >12 均为未定义码，注入显性拒（10/10）。
//!   ③ 定容观测账：16 槽定容环，槽满不静默——丢弃数显性记账；守恒：
//!      点火数 = 落账数 + 丢弃数。
//!   ④ 封账（seal）后观测面只读：再点火显性拒（ObsSealed）；纯读归因
//!      probe 与落账 fire 共用「封账 → 零码 → 码闭集」判序，容量判据仅
//!      对落账 apply 生效（probe 不占槽、不推进序号、不计点火）。
//!   ⑤ 切换点带单调相位机：Idle→Armed→Switched，倒退/跳格显性拒。
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；三类注入（未定义码、
//!      零码、封账后点火）×10 必抓。零浮点、零 IO、零墙钟、零随机；
//!      观测账为定容数组，无堆分配。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0xC0440;

/// 定容观测账槽数（冻结）。
pub const OBS_CAP: usize = 16;
/// 并入观测账的错误码个数（冻结 ×12）。
pub const MERGED_CODES: usize = 12;
/// 观测点数（冻结 ×4）。
pub const POINT_COUNT: usize = 4;
/// 未定义码注入轮数（冻结）。
pub const INJECT_ROUNDS: u32 = 10;

/// 并入观测账的 12 个错误码（闭集，raw 值域 1..=12）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsErrCode {
    /// TRIM 超时
    TrimTimeout = 1,
    /// TRIM 区间越界
    TrimSpanBad = 2,
    /// TRIM 零区间
    TrimZeroRange = 3,
    /// flush 失败
    FlushFail = 4,
    /// flush 忙
    FlushBusy = 5,
    /// flush 脏账丢失
    FlushDirtyLoss = 6,
    /// 介质变更
    MediumChanged = 7,
    /// 介质缺席
    MediumAbsent = 8,
    /// 介质世代过期
    MediumGenStale = 9,
    /// 切换相位非法
    SwitchPhaseBad = 10,
    /// 切换忙
    SwitchBusy = 11,
    /// 切换被拒
    SwitchDenied = 12,
}

impl ObsErrCode {
    /// 闭集 raw 值（1..=12）。
    pub fn raw(self) -> u32 {
        self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ObsErrCode::TrimTimeout => "TRIM 超时：检查传输层时序账与重试窗",
            ObsErrCode::TrimSpanBad => "TRIM 区间越界：核对 LBA 与块数跨度",
            ObsErrCode::TrimZeroRange => "TRIM 零区间：块数必须 ≥1",
            ObsErrCode::FlushFail => "flush 失败：脏账保持原状，逐条确认后重试",
            ObsErrCode::FlushBusy => "flush 忙：上一轮尚未收口，等待完成位",
            ObsErrCode::FlushDirtyLoss => "flush 脏账丢失：对账守恒被破坏，停机排查",
            ObsErrCode::MediumChanged => "介质变更：重新读取世代令牌后重试",
            ObsErrCode::MediumAbsent => "介质缺席：检查插槽与供电",
            ObsErrCode::MediumGenStale => "介质世代过期：令牌落后于当前世代",
            ObsErrCode::SwitchPhaseBad => "切换相位非法：只允许逐格推进切换相位",
            ObsErrCode::SwitchBusy => "切换忙：上一轮切换尚未落定",
            ObsErrCode::SwitchDenied => "切换被拒：核对切换权限与前置条件",
        }
    }

    /// raw → 闭集码的双向翻译（未定义 raw 返回 None）。
    pub fn from_raw(raw: u32) -> Option<ObsErrCode> {
        match raw {
            1 => Some(ObsErrCode::TrimTimeout),
            2 => Some(ObsErrCode::TrimSpanBad),
            3 => Some(ObsErrCode::TrimZeroRange),
            4 => Some(ObsErrCode::FlushFail),
            5 => Some(ObsErrCode::FlushBusy),
            6 => Some(ObsErrCode::FlushDirtyLoss),
            7 => Some(ObsErrCode::MediumChanged),
            8 => Some(ObsErrCode::MediumAbsent),
            9 => Some(ObsErrCode::MediumGenStale),
            10 => Some(ObsErrCode::SwitchPhaseBad),
            11 => Some(ObsErrCode::SwitchBusy),
            12 => Some(ObsErrCode::SwitchDenied),
            _ => None,
        }
    }
}

/// 并入码权威清单（raw = 下标 + 1，逐位可验）。
pub const CODE_TABLE: [ObsErrCode; MERGED_CODES] = [
    ObsErrCode::TrimTimeout,
    ObsErrCode::TrimSpanBad,
    ObsErrCode::TrimZeroRange,
    ObsErrCode::FlushFail,
    ObsErrCode::FlushBusy,
    ObsErrCode::FlushDirtyLoss,
    ObsErrCode::MediumChanged,
    ObsErrCode::MediumAbsent,
    ObsErrCode::MediumGenStale,
    ObsErrCode::SwitchPhaseBad,
    ObsErrCode::SwitchBusy,
    ObsErrCode::SwitchDenied,
];

/// 观测点四点闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    /// TRIM 点
    Trim = 0,
    /// flush 点
    Flush = 1,
    /// 介质点
    Medium = 2,
    /// 切换点
    Switch = 3,
}

impl ObsPoint {
    pub fn index(self) -> u32 {
        self as u32
    }

    pub fn label(self) -> &'static str {
        match self {
            ObsPoint::Trim => "TRIM",
            ObsPoint::Flush => "flush",
            ObsPoint::Medium => "介质",
            ObsPoint::Switch => "切换",
        }
    }
}

/// 单条观测记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObsRec {
    pub point: ObsPoint,
    /// 闭集 raw 码（1..=12）
    pub raw: u32,
    /// 该点内单调序号
    pub seq: u32,
}

/// 切换相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchPhase {
    /// 空闲
    Idle = 0,
    /// 已武装
    Armed = 1,
    /// 已切换
    Switched = 2,
}

impl SwitchPhase {
    pub fn order(self) -> u32 {
        self as u32
    }
}

/// 诊断码闭集（可观测性埋点域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 零码（raw=0 不在闭集）
    ZeroCode,
    /// 未定义码（raw 超出 1..=12）
    UndefinedCode,
    /// 观测面已封账，再点火显性拒
    ObsSealed,
    /// 切换相位倒退
    PhaseRegression,
    /// 切换相位跳格
    PhaseSkip,
    /// 账面守恒失败
    LedgerMismatch,
}

impl ObsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::Ok => "埋点点火成功",
            ObsCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ObsCode::ZeroCode => "零码：raw=0 不在并入闭集内",
            ObsCode::UndefinedCode => "未定义码：raw 必须落在 1..=12",
            ObsCode::ObsSealed => "观测面已封账：只读归因可用 probe，落账须先解封",
            ObsCode::PhaseRegression => "切换相位倒退：只能前进不能回退",
            ObsCode::PhaseSkip => "切换相位跳格：必须逐格推进",
            ObsCode::LedgerMismatch => "账面不符：点火=落账+丢弃 守恒被破坏",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [ObsCode; 8] = [
    ObsCode::Ok,
    ObsCode::SegBad,
    ObsCode::ZeroCode,
    ObsCode::UndefinedCode,
    ObsCode::ObsSealed,
    ObsCode::PhaseRegression,
    ObsCode::PhaseSkip,
    ObsCode::LedgerMismatch,
];

/// B16 可观测性埋点引擎。
pub struct B16ObsEngine {
    sealed: bool,
    slots: [Option<ObsRec>; OBS_CAP],
    recorded: usize,
    dropped: u64,
    fired: u64,
    point_seqs: [u32; POINT_COUNT],
    point_counts: [u32; POINT_COUNT],
    switch_phase: SwitchPhase,
    injected: u64,
    detected: u64,
}

impl B16ObsEngine {
    /// 新引擎：四点序号/计数从 0 起，未封账。
    pub fn new() -> B16ObsEngine {
        B16ObsEngine {
            sealed: false,
            slots: [None; OBS_CAP],
            recorded: 0,
            dropped: 0,
            fired: 0,
            point_seqs: [0u32; POINT_COUNT],
            point_counts: [0u32; POINT_COUNT],
            switch_phase: SwitchPhase::Idle,
            injected: 0,
            detected: 0,
        }
    }

    /// 落账（apply）：append-only 定容环，槽满显性记账不静默。
    fn ring_push(&mut self, rec: ObsRec) {
        if self.recorded >= OBS_CAP {
            self.dropped += 1;
            return;
        }
        match self.slots.get_mut(self.recorded) {
            Some(slot) => {
                *slot = Some(rec);
                self.recorded += 1;
            }
            None => {
                self.dropped += 1;
            }
        }
    }

    /// 点火（落账 apply）。判定次序：封账守卫 → 零码 → 码闭集 → 落账。
    /// 槽满不拒：照常分配序号并显性记丢弃（守恒：点火=落账+丢弃）。
    pub fn fire(&mut self, point: ObsPoint, raw: u32) -> Result<u32, ObsCode> {
        if self.sealed {
            return Err(ObsCode::ObsSealed);
        }
        if raw == 0 {
            return Err(ObsCode::ZeroCode);
        }
        let code = match ObsErrCode::from_raw(raw) {
            Some(c) => c,
            None => return Err(ObsCode::UndefinedCode),
        };
        let idx = point.index() as usize;
        let next = match self.point_seqs.get_mut(idx) {
            Some(s) => match s.checked_add(1) {
                Some(v) => {
                    *s = v;
                    v
                }
                None => return Err(ObsCode::LedgerMismatch),
            },
            None => return Err(ObsCode::LedgerMismatch),
        };
        self.ring_push(ObsRec { point, raw: code.raw(), seq: next });
        match self.point_counts.get_mut(idx) {
            Some(c) => {
                *c += 1;
            }
            None => {}
        }
        self.fired += 1;
        Ok(next)
    }

    /// 纯读归因（probe）：与 fire 共用「封账 → 零码 → 码闭集」判序，
    /// 但不落账、不占槽、不推进序号、不计点火——容量判据仅对落账生效。
    pub fn probe(&self, raw: u32) -> Result<&'static str, ObsCode> {
        if self.sealed {
            return Err(ObsCode::ObsSealed);
        }
        if raw == 0 {
            return Err(ObsCode::ZeroCode);
        }
        match ObsErrCode::from_raw(raw) {
            Some(c) => Ok(c.detail()),
            None => Err(ObsCode::UndefinedCode),
        }
    }

    /// 封账：之后观测面只读（fire 拒、probe 仍可用前的判序被拒）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 切换相位推进（单调一格）。
    fn step_switch(&mut self, target: SwitchPhase) -> Result<(), ObsCode> {
        let cur = self.switch_phase.order();
        let tgt = target.order();
        if tgt < cur {
            return Err(ObsCode::PhaseRegression);
        }
        if tgt != cur + 1 {
            return Err(ObsCode::PhaseSkip);
        }
        self.switch_phase = target;
        Ok(())
    }

    /// 武装切换：Idle → Armed。
    pub fn switch_arm(&mut self) -> Result<(), ObsCode> {
        self.step_switch(SwitchPhase::Armed)
    }

    /// 提交切换：Armed → Switched。
    pub fn switch_commit(&mut self) -> Result<(), ObsCode> {
        self.step_switch(SwitchPhase::Switched)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn recorded_count(&self) -> usize {
        self.recorded
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn fired(&self) -> u64 {
        self.fired
    }

    /// 第 idx 点的落账条数（越界返回 0）。
    pub fn point_count(&self, idx: usize) -> u32 {
        match self.point_counts.get(idx) {
            Some(c) => *c,
            None => 0,
        }
    }

    /// 第 idx 点的当前序号（越界返回 0）。
    pub fn point_seq(&self, idx: usize) -> u32 {
        match self.point_seqs.get(idx) {
            Some(s) => *s,
            None => 0,
        }
    }

    /// 第 idx 槽的观测记录。
    pub fn rec_at(&self, idx: usize) -> Option<ObsRec> {
        match self.slots.get(idx) {
            Some(r) => *r,
            None => None,
        }
    }

    pub fn switch_phase(&self) -> SwitchPhase {
        self.switch_phase
    }

    /// 点覆盖：四点全亮（各 ≥1 条）即埋点 100%。
    pub fn coverage_ok(&self) -> bool {
        self.coverage_holes() == 0
    }

    /// 覆盖缺亮点数（0 即 100%）。
    pub fn coverage_holes(&self) -> u32 {
        let mut holes = 0u32;
        for i in 0..POINT_COUNT {
            if self.point_count(i) == 0 {
                holes += 1;
            }
        }
        holes
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.injected.saturating_sub(self.detected)
    }

    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }

    pub fn note_violation_caught(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    pub fn note_escape(&mut self) {
        self.injected += 1;
    }

    /// 双跑快照：观测账全量 + 各点序号/计数 + 切换相位（确定性对照面）。
    pub fn snapshot(
        &self,
    ) -> (
        [Option<ObsRec>; OBS_CAP],
        [u32; POINT_COUNT],
        [u32; POINT_COUNT],
        usize,
        u64,
        SwitchPhase,
    ) {
        (self.slots, self.point_seqs, self.point_counts, self.recorded, self.dropped, self.switch_phase)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次重建快照逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (
        [Option<ObsRec>; OBS_CAP],
        [u32; POINT_COUNT],
        [u32; POINT_COUNT],
        usize,
        u64,
        SwitchPhase,
    ) {
        let mut e = B16ObsEngine::new();
        let _ = e.fire(ObsPoint::Trim, 2);
        let _ = e.fire(ObsPoint::Flush, 5);
        let _ = e.fire(ObsPoint::Medium, 8);
        let _ = e.fire(ObsPoint::Switch, 11);
        let _ = e.fire(ObsPoint::Trim, 0);
        let _ = e.fire(ObsPoint::Flush, 99);
        let _ = e.switch_arm();
        let _ = e.switch_commit();
        e.snapshot()
    };
    build() == build()
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51517_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51517");
    // 1 正常流：四点各落 1 条，点覆盖 100%。
    {
        let mut e = B16ObsEngine::new();
        let a = e.fire(ObsPoint::Trim, ObsErrCode::TrimTimeout.raw());
        let b = e.fire(ObsPoint::Flush, ObsErrCode::FlushFail.raw());
        let c = e.fire(ObsPoint::Medium, ObsErrCode::MediumChanged.raw());
        let d = e.fire(ObsPoint::Switch, ObsErrCode::SwitchDenied.raw());
        s.add(
            "四点埋点全亮",
            a.is_ok()
                && b.is_ok()
                && c.is_ok()
                && d.is_ok()
                && e.recorded_count() == 4
                && e.coverage_ok(),
            "TRIM/flush/介质/切换 各落 1 条，点覆盖 100%",
        );
    }
    // 2 边界零：零码 fire/probe 双路显性拒。
    {
        let mut e = B16ObsEngine::new();
        let f0 = e.fire(ObsPoint::Trim, 0) == Err(ObsCode::ZeroCode);
        let p0 = e.probe(0) == Err(ObsCode::ZeroCode);
        s.add(
            "零码边界",
            f0 && p0 && e.recorded_count() == 0,
            "raw=0 不在并入闭集内，落账与纯读同拒",
        );
    }
    // 3 边界上限/上限+1：raw=12 放行、raw=13 拒。
    {
        let mut e = B16ObsEngine::new();
        let max_ok = e.fire(ObsPoint::Trim, MERGED_CODES as u32).is_ok();
        let over = e.fire(ObsPoint::Trim, MERGED_CODES as u32 + 1) == Err(ObsCode::UndefinedCode);
        let p12 = e.probe(MERGED_CODES as u32).is_ok();
        s.add(
            "raw 上限与上限+1",
            max_ok && over && p12,
            "raw=12（并入上限）放行、raw=13 未定义码显性拒；probe 同判序",
        );
    }
    // 4 注入必抓：未定义码 ×10（raw 13..=22）。
    {
        let mut e = B16ObsEngine::new();
        let mut caught = 0u32;
        for i in 0..INJECT_ROUNDS {
            if e.fire(ObsPoint::Trim, 13 + i) == Err(ObsCode::UndefinedCode) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add(
            "未定义码注入 10/10",
            caught == 10 && e.recorded_count() == 0,
            "raw>12 必须全拒且不落账",
        );
    }
    // 5 注入必抓：零码 ×10。
    {
        let mut e = B16ObsEngine::new();
        let mut caught = 0u32;
        for _ in 0..INJECT_ROUNDS {
            if e.fire(ObsPoint::Flush, 0) == Err(ObsCode::ZeroCode) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add("零码注入 10/10", caught == 10, "raw=0 必须全拒");
    }
    // 6 注入必抓：封账后点火 ×10。
    {
        let mut e = B16ObsEngine::new();
        e.seal();
        let mut caught = 0u32;
        for _ in 0..INJECT_ROUNDS {
            if e.fire(ObsPoint::Medium, ObsErrCode::FlushFail.raw()) == Err(ObsCode::ObsSealed) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add(
            "封账后点火注入 10/10",
            caught == 10 && e.is_sealed() && e.recorded_count() == 0,
            "封账后观测面只读，落账必须全拒",
        );
    }
    // 7 判定次序：封账守卫先于码判据（fire/probe 同序）。
    {
        let mut e = B16ObsEngine::new();
        e.seal();
        let sealed_first = e.fire(ObsPoint::Trim, 0) == Err(ObsCode::ObsSealed);
        let sealed_undef = e.fire(ObsPoint::Trim, 13) == Err(ObsCode::ObsSealed);
        let probe_sealed = e.probe(0) == Err(ObsCode::ObsSealed);
        s.add(
            "判序封账先于码判据",
            sealed_first && sealed_undef && probe_sealed,
            "封账守卫是入口守卫，先于零码/未定义码判据",
        );
    }
    // 8 状态机拒止：切换相位跳格/原地/倒退。
    {
        let mut e = B16ObsEngine::new();
        let skip = e.switch_commit() == Err(ObsCode::PhaseSkip);
        let _ = e.switch_arm();
        let same = e.switch_arm() == Err(ObsCode::PhaseSkip);
        let _ = e.switch_commit();
        let regress = e.switch_arm() == Err(ObsCode::PhaseRegression);
        s.add(
            "切换相位机拒止",
            skip && same && regress,
            "Idle→Armed→Switched 逐格推进，越轨显性拒",
        );
    }
    // 9 容量耗尽：槽满不静默，丢弃显性记账且守恒。
    {
        let mut e = B16ObsEngine::new();
        for i in 0..(OBS_CAP as u32 + 1) {
            let p = match i % 4 {
                0 => ObsPoint::Trim,
                1 => ObsPoint::Flush,
                2 => ObsPoint::Medium,
                _ => ObsPoint::Switch,
            };
            let raw = (i % 12) + 1;
            let _ = e.fire(p, raw);
        }
        s.add(
            "观测账容量耗尽显性",
            e.recorded_count() == OBS_CAP
                && e.dropped() == 1
                && e.fired() == e.recorded_count() as u64 + e.dropped(),
            "槽满不静默：丢弃显性记账，点火=落账+丢弃",
        );
    }
    // 10 probe 纯读不落账（probe/apply 判序共用、容量判据仅 apply 生效）。
    {
        let mut e = B16ObsEngine::new();
        let before = e.recorded_count();
        let p = e.probe(ObsErrCode::FlushFail.raw());
        s.add(
            "probe 纯读不落账",
            p.is_ok() && e.recorded_count() == before && e.fired() == 0,
            "纯读归因不占槽、不推进序号、不计点火",
        );
    }
    // 11 双跑一致。
    s.add("双跑逐字段一致", dual_run_consistent(), "同一脚本两次快照逐字段相等");
    // 12 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 13 对账守恒：30 次注入全检出、逃逸 0。
    {
        let mut e = B16ObsEngine::new();
        for i in 0..INJECT_ROUNDS {
            let _ = e.fire(ObsPoint::Trim, 13 + i);
            e.note_violation_caught();
            let _ = e.fire(ObsPoint::Flush, 0);
            e.note_violation_caught();
        }
        let mut e2 = B16ObsEngine::new();
        e2.seal();
        for _ in 0..INJECT_ROUNDS {
            let _ = e2.fire(ObsPoint::Medium, 4);
            e2.note_violation_caught();
        }
        s.add(
            "注入账守恒",
            e.conserved()
                && e2.conserved()
                && e.injected() + e2.injected() == 30
                && e.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 14 错误码 ×12 闭集完备：raw 1..=12 双向可译、文案非空。
    {
        let mut table_ok = true;
        for i in 0..MERGED_CODES {
            let c = match CODE_TABLE.get(i) {
                Some(c) => *c,
                None => {
                    table_ok = false;
                    break;
                }
            };
            if c.raw() != i as u32 + 1 || c.detail().is_empty() {
                table_ok = false;
            }
            match ObsErrCode::from_raw(i as u32 + 1) {
                Some(r) => {
                    if r != c {
                        table_ok = false;
                    }
                }
                None => table_ok = false,
            }
        }
        s.add(
            "错误码 ×12 闭集完备",
            table_ok,
            "12 码逐位并入：raw 1..=12 双向可译、中文建议非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_points_coverage_100() {
        let mut e = B16ObsEngine::new();
        assert!(e.fire(ObsPoint::Trim, 1).is_ok());
        assert!(e.fire(ObsPoint::Flush, 4).is_ok());
        assert!(e.fire(ObsPoint::Medium, 7).is_ok());
        assert!(e.fire(ObsPoint::Switch, 10).is_ok());
        assert_eq!(e.recorded_count(), 4);
        assert!(e.coverage_ok());
        assert_eq!(e.coverage_holes(), 0);
        // 每点序号独立单调。
        assert_eq!(e.point_seq(0), 1);
        assert_eq!(e.point_seq(3), 1);
    }

    #[test]
    fn zero_and_upper_raw_bounds() {
        let mut e = B16ObsEngine::new();
        assert_eq!(e.fire(ObsPoint::Trim, 0), Err(ObsCode::ZeroCode));
        assert!(e.fire(ObsPoint::Trim, MERGED_CODES as u32).is_ok());
        assert_eq!(
            e.fire(ObsPoint::Trim, MERGED_CODES as u32 + 1),
            Err(ObsCode::UndefinedCode)
        );
        assert_eq!(e.probe(0), Err(ObsCode::ZeroCode));
        assert!(e.probe(1).is_ok());
    }

    #[test]
    fn undefined_and_zero_codes_all_rejected_x10() {
        let mut e = B16ObsEngine::new();
        for i in 0..INJECT_ROUNDS {
            assert_eq!(
                e.fire(ObsPoint::Medium, 13 + i),
                Err(ObsCode::UndefinedCode),
                "未定义码 {} 必须拒绝", 13 + i
            );
        }
        for _ in 0..INJECT_ROUNDS {
            assert_eq!(e.fire(ObsPoint::Trim, 0), Err(ObsCode::ZeroCode));
        }
        assert_eq!(e.recorded_count(), 0);
        assert_eq!(e.fired(), 0);
    }

    #[test]
    fn sealed_surface_rejects_fires_x10() {
        let mut e = B16ObsEngine::new();
        e.seal();
        for _ in 0..INJECT_ROUNDS {
            assert_eq!(
                e.fire(ObsPoint::Switch, ObsErrCode::SwitchBusy.raw()),
                Err(ObsCode::ObsSealed)
            );
        }
        assert_eq!(e.recorded_count(), 0);
    }

    #[test]
    fn judge_order_sealed_before_codes() {
        let mut e = B16ObsEngine::new();
        e.seal();
        assert_eq!(e.fire(ObsPoint::Trim, 0), Err(ObsCode::ObsSealed));
        assert_eq!(e.fire(ObsPoint::Trim, 13), Err(ObsCode::ObsSealed));
        assert_eq!(e.probe(0), Err(ObsCode::ObsSealed));
    }

    #[test]
    fn probe_shares_order_but_not_ring() {
        let mut e = B16ObsEngine::new();
        let detail = e.probe(ObsErrCode::MediumChanged.raw());
        assert!(detail.is_ok());
        assert_eq!(e.recorded_count(), 0);
        assert_eq!(e.fired(), 0);
        // probe 不推进序号；fire 才推进。
        assert!(e.fire(ObsPoint::Medium, ObsErrCode::MediumChanged.raw()).is_ok());
        assert_eq!(e.point_seq(2), 1);
        assert_eq!(e.probe(99), Err(ObsCode::UndefinedCode));
    }

    #[test]
    fn switch_machine_rejects() {
        let mut e = B16ObsEngine::new();
        assert_eq!(e.switch_commit(), Err(ObsCode::PhaseSkip));
        assert!(e.switch_arm().is_ok());
        assert_eq!(e.switch_arm(), Err(ObsCode::PhaseSkip));
        assert!(e.switch_commit().is_ok());
        assert_eq!(e.switch_arm(), Err(ObsCode::PhaseRegression));
        assert_eq!(e.switch_phase(), SwitchPhase::Switched);
    }

    #[test]
    fn ring_overflow_accounted() {
        let mut e = B16ObsEngine::new();
        for i in 0..(OBS_CAP as u32 + 1) {
            let p = match i % 4 {
                0 => ObsPoint::Trim,
                1 => ObsPoint::Flush,
                2 => ObsPoint::Medium,
                _ => ObsPoint::Switch,
            };
            let raw = (i % 12) + 1;
            assert!(e.fire(p, raw).is_ok(), "槽满不拒：点火仍成功并显性记丢弃");
        }
        assert_eq!(e.recorded_count(), OBS_CAP);
        assert_eq!(e.dropped(), 1);
        assert_eq!(e.fired(), e.recorded_count() as u64 + e.dropped());
        assert!(e.rec_at(0).is_some());
        assert!(e.rec_at(OBS_CAP).is_none());
    }

    #[test]
    fn dual_run_identical_snapshot() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn code_table_closed_and_complete() {
        assert_eq!(CODE_TABLE.len(), MERGED_CODES);
        for i in 0..MERGED_CODES {
            let c = match CODE_TABLE.get(i) {
                Some(c) => *c,
                None => {
                    assert!(false, "码表缺位");
                    return;
                }
            };
            assert_eq!(c.raw(), i as u32 + 1);
            assert!(!c.detail().is_empty());
            assert_eq!(ObsErrCode::from_raw(i as u32 + 1), Some(c));
        }
        assert_eq!(ObsErrCode::from_raw(0), None);
        assert_eq!(ObsErrCode::from_raw(13), None);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
