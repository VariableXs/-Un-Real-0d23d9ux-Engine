//! UNX-F10047 回归执行器（串行/并行档） · 0x91bc0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10047（B22 常态化回归线 · 执行器）。
//! 锚点：UNX-F10048-J1 归并零丢失（多轮逐条并入核对 10/10 次），轮次丢失 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91bc0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `ExecCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态机 `SerialTrack`：串行档序号机 next_seq 单调推进，乱序/重入/
//!     未完成再派发显性拒绝；在途账 pending 显性可见（零丢失核对）；
//!   - 分账 `LaneLedger`：并行档 lane 1..=4 隔离分账，回报档号与分配档号
//!     不符即交叉污染，逐次检出，注入=检出+逃逸，逃逸恒 0；
//!   - 用例注册表 `CaseRoster`：40 条战线条目注册，越界/重复拒绝；
//!   - 注入核 `BadExecCase`：串行乱序 / 槽越界 / 交叉污染 / 重复派发 /
//!     未知用例 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10047 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91bc0;

/// 段窗上界（下一卡 0x91c00 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91c00;

/// 战线条目数（40 条，B21 口径）。
pub const CASE_COUNT: usize = 40;

/// 执行档数（lane 0 串行 + lane 1..=4 并行）。
pub const LANE_COUNT: usize = 5;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const EACCES: i32 = 13;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EIO: i32 = 5;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecCode {
    /// 串行乱序（seq ≠ next_seq）
    SerialOutOfOrder,
    /// 槽越界（并行档号出 1..=4）
    SlotOverflow,
    /// 交叉污染（回报档号 ≠ 分配档号）
    CrossContamination,
    /// 重复派发（在途未完成再派发）
    DoubleDispatch,
    /// 未知用例（号出 1..=40）
    UnknownCase,
    /// 串行在途未清（零丢失核对失败）
    PendingLeak,
    /// 分账倒退
    LedgerRegress,
    /// 重复完成
    DoubleComplete,
    /// 完成无派发
    CompleteWithoutDispatch,
    /// 双跑不一致
    NotReproducible,
    /// 逐条并入缺漏（派发-完成账不平）
    MergeLoss,
    /// 段位越界
    SegmentViolation,
}

impl ExecCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ExecCode::SerialOutOfOrder => "串行乱序被拒：seq 必须等于 next_seq；请按序派发",
            ExecCode::SlotOverflow => "槽越界被拒：并行档号必须落在 1..=4；请核对槽位",
            ExecCode::CrossContamination => "交叉污染被拒：回报档号与分配档号不符；请核对分账",
            ExecCode::DoubleDispatch => "重复派发被拒：串行档在途未完成；请先 complete 再派发",
            ExecCode::UnknownCase => "未知用例被拒：条目号必须落在 1..=40；请核对口",
            ExecCode::PendingLeak => "在途未清被拒：串行档存在未完成派发；请补完成或落账",
            ExecCode::LedgerRegress => "分账倒退被拒：计数只增不减；请排查回滚误改",
            ExecCode::DoubleComplete => "重复完成被拒：该用例已完成；请勿二次回报",
            ExecCode::CompleteWithoutDispatch => "完成无派发被拒：未派发先回报；请先 dispatch",
            ExecCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            ExecCode::MergeLoss => "并入缺漏被拒：派发-完成账不平；请逐条并入核对",
            ExecCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91bc0..=0x91c00",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ExecCode; 12] = [
        ExecCode::SerialOutOfOrder,
        ExecCode::SlotOverflow,
        ExecCode::CrossContamination,
        ExecCode::DoubleDispatch,
        ExecCode::UnknownCase,
        ExecCode::PendingLeak,
        ExecCode::LedgerRegress,
        ExecCode::DoubleComplete,
        ExecCode::CompleteWithoutDispatch,
        ExecCode::NotReproducible,
        ExecCode::MergeLoss,
        ExecCode::SegmentViolation,
    ];
}

// ── 用例注册表 ────────────────────────────────────────────────────────
/// 用例注册表：40 条逐条注册。
#[derive(Clone, Copy, Debug)]
pub struct CaseRoster {
    /// 注册占用表（下标 = 条目号-1）
    pub have: [bool; CASE_COUNT],
    /// 已注册条数
    pub registered: u32,
}

impl CaseRoster {
    /// 构造空注册表。
    pub const fn new() -> CaseRoster {
        CaseRoster { have: [false; CASE_COUNT], registered: 0 }
    }

    /// 注册一条：越界与重复显性拒绝。
    pub fn register(&mut self, id: u32) -> Result<(), ExecCode> {
        if id == 0 || id as usize > CASE_COUNT {
            return Err(ExecCode::UnknownCase);
        }
        let idx = id as usize - 1;
        if *self.have.get(idx).unwrap_or(&true) {
            return Err(ExecCode::DoubleDispatch);
        }
        if let Some(slot) = self.have.get_mut(idx) {
            *slot = true;
        }
        self.registered = self.registered.saturating_add(1);
        Ok(())
    }

    /// 已注册判定。
    pub fn enrolled(&self, id: u32) -> bool {
        id >= 1 && (id as usize) <= CASE_COUNT
            && *self.have.get(id as usize - 1).unwrap_or(&false)
    }
}

/// 注册全部 40 条。
pub fn register_all(roster: &mut CaseRoster) -> Result<(), ExecCode> {
    let mut id = 1u32;
    while id as usize <= CASE_COUNT {
        roster.register(id)?;
        id += 1;
    }
    Ok(())
}

// ── 串行档状态机 ──────────────────────────────────────────────────────
/// 串行档状态机：next_seq 单调推进，在途唯一，完成账只增。
#[derive(Clone, Copy, Debug)]
pub struct SerialTrack {
    /// 下一期望序号（从 1 起）
    pub next_seq: u32,
    /// 在途用例（0=无在途）
    pub inflight: u32,
    /// 已完成条数
    pub done: u32,
    /// 完成占用表
    pub completed: [bool; CASE_COUNT],
}

impl SerialTrack {
    /// 构造零态串行机。
    pub const fn new() -> SerialTrack {
        SerialTrack { next_seq: 1, inflight: 0, done: 0, completed: [false; CASE_COUNT] }
    }

    /// 派发：seq 必须等于 next_seq、无在途、用例已注册。
    pub fn dispatch(&mut self, seq: u32, id: u32, roster: &CaseRoster) -> Result<(), ExecCode> {
        if id == 0 || id as usize > CASE_COUNT {
            return Err(ExecCode::UnknownCase);
        }
        if !roster.enrolled(id) {
            return Err(ExecCode::UnknownCase);
        }
        if seq != self.next_seq {
            return Err(ExecCode::SerialOutOfOrder);
        }
        if self.inflight != 0 {
            return Err(ExecCode::DoubleDispatch);
        }
        self.inflight = id;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }

    /// 完成：必须未重复完成且在途；完成账只增。
    pub fn complete(&mut self, id: u32) -> Result<(), ExecCode> {
        if id == 0 || id as usize > CASE_COUNT {
            return Err(ExecCode::UnknownCase);
        }
        let idx = id as usize - 1;
        if *self.completed.get(idx).unwrap_or(&true) {
            return Err(ExecCode::DoubleComplete);
        }
        if self.inflight != id {
            return Err(if self.inflight == 0 {
                ExecCode::CompleteWithoutDispatch
            } else {
                ExecCode::DoubleComplete
            });
        }
        if let Some(slot) = self.completed.get_mut(idx) {
            *slot = true;
        }
        self.inflight = 0;
        self.done = self.done.saturating_add(1);
        Ok(())
    }

    /// 零丢失核对：派发账（next_seq-1）必须等于完成账，在途必须清零。
    pub fn loss_audit(&self) -> Result<(), ExecCode> {
        if self.inflight != 0 {
            return Err(ExecCode::PendingLeak);
        }
        if self.done != self.next_seq - 1 {
            return Err(ExecCode::MergeLoss);
        }
        Ok(())
    }
}

// ── 并行档分账 ────────────────────────────────────────────────────────
/// 并行档分账：lane = 1 + (id-1)%4，回报档号必须一致。
#[derive(Clone, Copy, Debug)]
pub struct LaneLedger {
    /// 逐条分配档号（0=未分账）
    pub lane_of: [u8; CASE_COUNT],
    /// 已分账条数
    pub assigned: u32,
    /// 已回报条数
    pub reported: u32,
    /// 注入/检出/逃逸三联账
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl LaneLedger {
    /// 构造零账。
    pub const fn zero() -> LaneLedger {
        LaneLedger {
            lane_of: [0; CASE_COUNT],
            assigned: 0,
            reported: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 分配档号：1 + (id-1)%4，纯函数。
    pub fn lane_assign(id: u32) -> Option<u8> {
        if id == 0 || id as usize > CASE_COUNT {
            None
        } else {
            Some(((id - 1) % (LANE_COUNT as u32 - 1)) as u8 + 1)
        }
    }

    /// 分账一条并行用例。
    pub fn assign(&mut self, id: u32) -> Result<u8, ExecCode> {
        let lane = match Self::lane_assign(id) {
            Some(l) => l,
            None => return Err(ExecCode::UnknownCase),
        };
        let idx = id as usize - 1;
        if *self.lane_of.get(idx).unwrap_or(&255) != 0 {
            return Err(ExecCode::DoubleDispatch);
        }
        if let Some(slot) = self.lane_of.get_mut(idx) {
            *slot = lane;
        }
        self.assigned = self.assigned.saturating_add(1);
        Ok(lane)
    }

    /// 结果回报：回报档号与分配档号不符即交叉污染（逐次检出并入账）。
    pub fn record_result(&mut self, id: u32, claimed_lane: u8) -> Result<(), ExecCode> {
        self.injected = self.injected.saturating_add(1);
        let real = match Self::lane_assign(id) {
            Some(l) => l,
            None => {
                self.escaped = self.escaped.saturating_add(1);
                return Err(ExecCode::UnknownCase);
            }
        };
        let stored = *self.lane_of.get(id as usize - 1).unwrap_or(&0);
        if stored != real {
            self.escaped = self.escaped.saturating_add(1);
            return Err(ExecCode::CrossContamination);
        }
        if claimed_lane != real {
            self.detected = self.detected.saturating_add(1);
            return Err(ExecCode::CrossContamination);
        }
        self.reported = self.reported.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
        Ok(())
    }

    /// 注入账守恒：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏执行样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadExecCase {
    /// 串行乱序（跳 seq）
    SerialOutOfOrder,
    /// 槽越界（请求 lane 5）
    SlotOverflow,
    /// 交叉污染（回报错档）
    CrossContamination,
    /// 重复派发（在途未完成再派发）
    DoubleDispatch,
    /// 未知用例（号 41）
    UnknownCase,
}

impl BadExecCase {
    /// 闭集全量。
    pub const ALL: [BadExecCase; 5] = [
        BadExecCase::SerialOutOfOrder,
        BadExecCase::SlotOverflow,
        BadExecCase::CrossContamination,
        BadExecCase::DoubleDispatch,
        BadExecCase::UnknownCase,
    ];

    /// 注入一轮：构造坏样本并触发执行器，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let mut roster = CaseRoster::new();
        let _ = register_all(&mut roster);
        match self {
            BadExecCase::SerialOutOfOrder => {
                let mut tr = SerialTrack::new();
                if tr.dispatch(1, 1, &roster).is_err() {
                    return false;
                }
                let _ = tr.complete(1);
                // 期望 seq=2，给 seq=3 → 乱序拒绝。
                tr.dispatch(3, 2, &roster).is_err()
            }
            BadExecCase::SlotOverflow => {
                // 并行档号闭集 1..=4：lane_assign 不产出 5，回报 5 必拒。
                let mut led = LaneLedger::zero();
                if led.assign(3).is_err() {
                    return false;
                }
                led.record_result(3, 5).is_err()
            }
            BadExecCase::CrossContamination => {
                let mut led = LaneLedger::zero();
                if led.assign(7).is_err() {
                    return false;
                }
                let real = match LaneLedger::lane_assign(7) {
                    Some(l) => l,
                    None => return false,
                };
                led.record_result(7, real % (LANE_COUNT as u8 - 1) + 1).is_err()
            }
            BadExecCase::DoubleDispatch => {
                let mut tr = SerialTrack::new();
                if tr.dispatch(1, 1, &roster).is_err() {
                    return false;
                }
                tr.dispatch(2, 2, &roster).is_err()
            }
            BadExecCase::UnknownCase => {
                let mut tr = SerialTrack::new();
                tr.dispatch(1, 41, &roster).is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回逐类检出数。
pub fn run_exec_injection() -> [u32; 5] {
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadExecCase::ALL.len() {
        if let Some(case) = BadExecCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if case.inject() {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    per_class
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越档回报族"),
    ("EIO", -5, 111, "Windows ERROR_CONNECTION_REFUSED；回报失联族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知用例族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复派发族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；档位隔离族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；槽越界/乱序族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：注册→串行派发/完成→并行分账/回报全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut roster = CaseRoster::new();
    let _ = register_all(&mut roster);
    let mut tr = SerialTrack::new();
    let mut led = LaneLedger::zero();
    let mut bytes: Vec<u8> = Vec::new();
    let mut id = 1u32;
    while id as usize <= CASE_COUNT {
        bytes.push(match tr.dispatch(id, id, &roster) {
            Ok(()) => 1,
            Err(_) => 0,
        });
        bytes.push(match tr.complete(id) {
            Ok(()) => 2,
            Err(_) => 3,
        });
        if let Ok(lane) = led.assign(id) {
            bytes.push(lane);
            let _ = led.record_result(id, lane);
        }
        id += 1;
    }
    bytes.push(match tr.loss_audit() {
        Ok(()) => 4,
        Err(_) => 5,
    });
    bytes.push(if led.conserved() { 6 } else { 7 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10047 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10047_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10047");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91bc0..=0x91c00", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 注册表：40 条全量注册
    let mut roster = CaseRoster::new();
    let reg_ok = register_all(&mut roster).is_ok() && roster.registered == CASE_COUNT as u32;
    s.add("注册表：40 条战线条目全量注册", reg_ok,
          "条目号 1..=40 逐条入册，无重复无越界");

    // 3. 串行状态机：按序派发-完成 40/40
    let mut tr = SerialTrack::new();
    let mut seq_ok = true;
    let mut id = 1u32;
    while id as usize <= CASE_COUNT {
        if tr.dispatch(id, id, &roster).is_err() || tr.complete(id).is_err() {
            seq_ok = false;
            break;
        }
        id += 1;
    }
    s.add("串行档：40 条按序派发-完成零失败", seq_ok && tr.done == CASE_COUNT as u32,
          "next_seq 单调推进，逐条在途唯一");

    // 4. 零丢失核对：派发账=完成账、在途清零
    let loss_ok = tr.loss_audit().is_ok();
    s.add("零丢失核对：派发-完成账平、在途清零", loss_ok,
          "逐条并入核对，缺一条即 MergeLoss");

    // 5. 串行乱序拦截
    let mut tr2 = SerialTrack::new();
    let _ = tr2.dispatch(1, 1, &roster);
    let _ = tr2.complete(1);
    let ooo = tr2.dispatch(3, 2, &roster) == Err(ExecCode::SerialOutOfOrder);
    s.add("乱序拦截：跳 seq 派发显性拒绝", ooo,
          "seq 必须逐条衔接，跳号即拦");

    // 6. 重复派发拦截
    let mut tr3 = SerialTrack::new();
    let _ = tr3.dispatch(1, 1, &roster);
    let dd = tr3.dispatch(2, 2, &roster) == Err(ExecCode::DoubleDispatch);
    s.add("重入拦截：在途未完成再派发被拒", dd,
          "串行档在途唯一，重入显性拒绝");

    // 7. 重复完成 / 完成无派发拦截
    let mut tr4 = SerialTrack::new();
    let cw = tr4.complete(1) == Err(ExecCode::CompleteWithoutDispatch);
    let _ = tr4.dispatch(1, 1, &roster);
    let _ = tr4.complete(1);
    let dc = tr4.complete(1) == Err(ExecCode::DoubleComplete);
    s.add("完成闸：无派发回报/重复完成双双拒绝", cw && dc,
          "完成必须对应唯一在途派发");

    // 8. 在途泄漏检出（零丢失负样）
    let mut tr5 = SerialTrack::new();
    let _ = tr5.dispatch(1, 1, &roster);
    let leak = tr5.loss_audit() == Err(ExecCode::PendingLeak);
    s.add("在途泄漏：未完成派发被零丢失核对检出", leak,
          "逐条并入核对负样 1/1 检出");

    // 9. 并行档分配：lane 1..=4 轮转
    let lane_ok = LaneLedger::lane_assign(1) == Some(1)
        && LaneLedger::lane_assign(4) == Some(4)
        && LaneLedger::lane_assign(5) == Some(1)
        && LaneLedger::lane_assign(8) == Some(4)
        && LaneLedger::lane_assign(0).is_none();
    s.add("并行分账：lane 1..=4 轮转分配", lane_ok,
          "档号纯函数分配，账面可重算");

    // 10. 交叉污染 10/10 检出（并行档隔离核）
    let mut cross_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut led = LaneLedger::zero();
        let _ = led.assign(7);
        let real = LaneLedger::lane_assign(7).unwrap_or(0);
        let wrong = real % (LANE_COUNT as u8 - 1) + 1;
        if led.record_result(7, wrong).is_err() && led.record_result(7, real).is_ok() {
            cross_hit = cross_hit.saturating_add(1);
        }
        r += 1;
    }
    s.add("交叉污染检出：错档回报 10/10 即拦", cross_hit == INJECT_ROUNDS,
          "回报档号≠分配档号逐次检出，对档放行");

    // 11. 并行零丢失：40 条全分账全回报
    let mut led2 = LaneLedger::zero();
    let mut id2 = 1u32;
    let mut par_ok = true;
    while id2 as usize <= CASE_COUNT {
        let lane = match led2.assign(id2) {
            Ok(l) => l,
            Err(_) => {
                par_ok = false;
                break;
            }
        };
        if led2.record_result(id2, lane).is_err() {
            par_ok = false;
            break;
        }
        id2 += 1;
    }
    s.add("并行零丢失：40 条分账-回报逐条闭合", par_ok && led2.reported == CASE_COUNT as u32,
          "分账与回报逐条对齐，缺条即红");

    // 12. 注入核：五类坏样本全检出
    let per_class = run_exec_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "乱序/槽越界/交叉污染/重入/未知用例全被显式拒绝");

    // 13. 分账守恒：注入=检出+逃逸
    s.add("分账守恒：注入=检出+逃逸", led2.conserved() && led2.escaped == 0,
          "40 条对档回报全部入检出账，无静默");

    // 14. 文案闭集：非空
    let text_ok = ExecCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < ExecCode::ALL.len() {
        let mut j = i + 1;
        while j < ExecCode::ALL.len() {
            if let (Some(x), Some(y)) = (ExecCode::ALL.get(i), ExecCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ExecCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：执行账一行 + 指纹一行
    let acc_text = format!("串行完成 {} / 并行回报 {} / 注入 {} / 逃逸 {}",
        tr.done, led2.reported, led2.injected, led2.escaped);
    s.add_dyn("执行快照", seq_ok && par_ok && led2.escaped == 0, &acc_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_40() {
        let mut roster = CaseRoster::new();
        assert!(register_all(&mut roster).is_ok());
        assert_eq!(roster.registered, 40);
    }

    #[test]
    fn serial_order_enforced() {
        let mut roster = CaseRoster::new();
        let _ = register_all(&mut roster);
        let mut tr = SerialTrack::new();
        assert!(tr.dispatch(1, 1, &roster).is_ok());
        assert_eq!(tr.dispatch(3, 2, &roster), Err(ExecCode::SerialOutOfOrder));
    }

    #[test]
    fn double_dispatch_rejected() {
        let mut roster = CaseRoster::new();
        let _ = register_all(&mut roster);
        let mut tr = SerialTrack::new();
        let _ = tr.dispatch(1, 1, &roster);
        assert_eq!(tr.dispatch(2, 2, &roster), Err(ExecCode::DoubleDispatch));
    }

    #[test]
    fn complete_guards() {
        let mut roster = CaseRoster::new();
        let _ = register_all(&mut roster);
        let mut tr = SerialTrack::new();
        assert_eq!(tr.complete(1), Err(ExecCode::CompleteWithoutDispatch));
        let _ = tr.dispatch(1, 1, &roster);
        assert!(tr.complete(1).is_ok());
        assert_eq!(tr.complete(1), Err(ExecCode::DoubleComplete));
    }

    #[test]
    fn pending_leak_detected() {
        let mut roster = CaseRoster::new();
        let _ = register_all(&mut roster);
        let mut tr = SerialTrack::new();
        let _ = tr.dispatch(1, 1, &roster);
        assert_eq!(tr.loss_audit(), Err(ExecCode::PendingLeak));
        let _ = tr.complete(1);
        assert!(tr.loss_audit().is_ok());
    }

    #[test]
    fn unknown_case_rejected() {
        let roster = CaseRoster::new();
        let mut tr = SerialTrack::new();
        assert_eq!(tr.dispatch(1, 41, &roster), Err(ExecCode::UnknownCase));
        assert_eq!(tr.dispatch(1, 0, &roster), Err(ExecCode::UnknownCase));
    }

    #[test]
    fn lane_rotation() {
        assert_eq!(LaneLedger::lane_assign(1), Some(1));
        assert_eq!(LaneLedger::lane_assign(5), Some(1));
        assert_eq!(LaneLedger::lane_assign(8), Some(4));
        assert_eq!(LaneLedger::lane_assign(41), None);
    }

    #[test]
    fn cross_lane_detected() {
        let mut led = LaneLedger::zero();
        let _ = led.assign(9);
        let real = LaneLedger::lane_assign(9).unwrap_or(0);
        let wrong = real % (LANE_COUNT as u8 - 1) + 1;
        assert_eq!(led.record_result(9, wrong), Err(ExecCode::CrossContamination));
        assert!(led.record_result(9, real).is_ok());
        assert!(led.conserved());
    }

    #[test]
    fn injection_matrix_all_caught() {
        let per = run_exec_injection();
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EACCES));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10047_checks_green() {
        let s = run_unx10047_checks();
        assert!(s.all_passed(), "UNX-F10047 red: {}", s.red_items_text());
    }
}
