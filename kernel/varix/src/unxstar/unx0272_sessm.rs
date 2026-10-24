//! unx0272_sessm —— UNX-F0272 终端会话状态机（空闲/命令/浏览/回看）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0272 · 会话状态机 · 目标 260 行
//! 判据原文：UNX-F0272-J1 会话四态闭集迁移合法矩阵逐条判定：
//!           非法迁移拒且态不变（迁移原子性）；回看模式只读
//!           （历史缓冲不可被当前命令污染）；状态机事件可重放
//!           （同事件序列重放终态一致）。
//! 组件：四态枚举 + 迁移矩阵（合法迁移表）+ 会话机（step 核 +
//!       历史缓冲 + 回看只读闸）+ 重放核（事件序列确定性）。
//! 职责定位：状态机是「终端交互的骨架」——迁移必须原子（拒绝
//!   = 态不动，半迁移是最恶心的 bug 源）；回看是历史缓冲的
//!   只读窗口，浏览态下跑命令会污染历史视图。重放确定性让
//!   远端复现会话问题成为可能。
//! 性能逐项分解：step O(1)；重放 O(事件数)；历史 O(深)。
//! 诊断码段：0x20340（0x20341~0x20349 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SM_VERSION: &str = "unx0272_sessm-v1";

/// 诊断段基址（0x20340 段 · 本模块独占）。
pub const SM_SEG_BASE: u32 = 0x20340;
pub const XC_ILLEGAL: u32 = 0x20341; // 非法迁移
pub const XC_RO_VIEW: u32 = 0x20342; // 回看只读闸
pub const EVT_OOR: u32 = 0x20343; // 事件越域
pub const HIST_FULL: u32 = 0x20344; // 历史满
pub const STATE_OOR: u32 = 0x20345; // 态越域
pub const REPLAY_DRIFT: u32 = 0x20346; // 重放漂移
pub const EVT_EMPTY: u32 = 0x20347; // 空事件拒
pub const HIST_POLLUTE: u32 = 0x20348; // 历史污染检出
pub const SEQ_LEN: u32 = 0x20349; // 序列长越域

/// 四态。
pub const S_IDLE: u8 = 0; // 空闲
pub const S_CMD: u8 = 1; // 命令执行中
pub const S_BROWSE: u8 = 2; // 浏览（当前输出滚动）
pub const S_REVIEW: u8 = 3; // 回看（历史只读窗口）

/// 事件闭集。
pub const E_CMD_START: u8 = 1; // 开始命令
pub const E_CMD_DONE: u8 = 2; // 命令完成
pub const E_SCROLL: u8 = 3; // 滚动浏览
pub const E_REVIEW_OPEN: u8 = 4; // 打开回看
pub const E_REVIEW_CLOSE: u8 = 5; // 关闭回看
pub const E_RESET: u8 = 6; // 复位

/// 合法迁移矩阵：FROM → [可到态集]。
pub fn legal_next(from: u8) -> &'static [u8] {
    match from {
        S_IDLE => &[S_CMD, S_BROWSE, S_REVIEW],
        S_CMD => &[S_IDLE, S_BROWSE],
        S_BROWSE => &[S_IDLE, S_REVIEW],
        S_REVIEW => &[S_IDLE, S_BROWSE],
        _ => &[],
    }
}

/// 迁移合法性核。
pub fn transition_legal(from: u8, to: u8) -> Result<(), u32> {
    if from > S_REVIEW || to > S_REVIEW {
        return Err(STATE_OOR);
    }
    if legal_next(from).contains(&to) {
        Ok(())
    } else {
        Err(XC_ILLEGAL)
    }
}

/// 终端会话机。
pub struct TermSession {
    state: u8,
    /// 历史缓冲（命令完成时落账的输出行号）。
    history: Vec<u32>,
    hist_cap: usize,
}

impl TermSession {
    pub fn new(hist_cap: usize) -> TermSession {
        TermSession { state: S_IDLE, history: Vec::new(), hist_cap }
    }

    /// 当前态。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 单步迁移：非法 = 拒且态不变（原子性锚）。
    pub fn step(&mut self, ev: u8) -> Result<u8, u32> {
        if ev < E_CMD_START || ev > E_RESET {
            return Err(EVT_OOR);
        }
        let to = match ev {
            E_CMD_START => S_CMD,
            E_CMD_DONE => S_IDLE,
            E_SCROLL => S_BROWSE,
            E_REVIEW_OPEN => S_REVIEW,
            E_REVIEW_CLOSE => S_IDLE,
            E_RESET => S_IDLE,
            _ => return Err(EVT_OOR),
        };
        transition_legal(self.state, to)?;
        // 迁移副作用：命令完成 → 输出行落历史。
        if ev == E_CMD_DONE && self.state == S_CMD {
            self.history.push((self.history.len() + 1) as u32);
            if self.history.len() > self.hist_cap {
                self.history.remove(0);
                return Err(HIST_FULL); // 满账立案（旧行已逐出）
            }
        }
        self.state = to;
        Ok(to)
    }

    /// 回看只读闸：回看态下禁写历史（污染检出）。
    pub fn history_write(&mut self, line: u32) -> Result<(), u32> {
        if self.state == S_REVIEW {
            return Err(XC_RO_VIEW);
        }
        if self.history.len() >= self.hist_cap {
            return Err(HIST_FULL);
        }
        self.history.push(line);
        Ok(())
    }

    /// 历史快照。
    pub fn history(&self) -> &[u32] {
        &self.history
    }
}

/// 重放核：同事件序列重放两次，终态必须一致（确定性锚）。
pub fn replay_deterministic(events: &[u8], hist_cap: usize) -> Result<(), u32> {
    if events.is_empty() {
        return Err(EVT_EMPTY);
    }
    if events.len() > 512 {
        return Err(SEQ_LEN);
    }
    let mut a = TermSession::new(hist_cap);
    let mut b = TermSession::new(hist_cap);
    for &e in events.iter() {
        let _ = a.step(e);
        let _ = b.step(e);
    }
    if a.state() == b.state() && a.history() == b.history() {
        Ok(())
    } else {
        Err(REPLAY_DRIFT)
    }
}

pub fn run_unx0272_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0272_sessm");
    // 族一：迁移矩阵。
    cs.add("空闲→命令", transition_legal(S_IDLE, S_CMD).is_ok(), "合法");
    cs.add("空闲→回看", transition_legal(S_IDLE, S_REVIEW).is_ok(), "合法");
    cs.add("命令→回看拒", transition_legal(S_CMD, S_REVIEW) == Err(XC_ILLEGAL), "执行中禁回看");
    cs.add("回看→命令拒", transition_legal(S_REVIEW, S_CMD) == Err(XC_ILLEGAL), "回看只读");
    cs.add("浏览→回看", transition_legal(S_BROWSE, S_REVIEW).is_ok(), "合法");
    cs.add("态越域拒", transition_legal(9, S_IDLE).is_err(), "STATE_OOR");
    // 族二：step 原子性。
    let mut sess = TermSession::new(8);
    cs.add("初态空闲", sess.state() == S_IDLE, "上电态");
    cs.add("启动命令", sess.step(E_CMD_START) == Ok(S_CMD), "迁入命令");
    cs.add("非法拒保持", sess.step(E_REVIEW_OPEN) == Err(XC_ILLEGAL) && sess.state() == S_CMD,
           "拒后态不变");
    cs.add("完成落账", sess.step(E_CMD_DONE) == Ok(S_IDLE) && sess.history() == &[1u32],
           "输出行 1 入史");
    // 族三：回看只读。
    let _ = sess.step(E_REVIEW_OPEN);
    cs.add("回看态", sess.state() == S_REVIEW, "打开");
    cs.add("回看禁写", sess.history_write(99) == Err(XC_RO_VIEW), "历史不被污染");
    cs.add("回看历史不变", sess.history() == &[1u32], "快照冻结");
    let _ = sess.step(E_REVIEW_CLOSE);
    cs.add("闭回看可写", sess.history_write(50).is_ok(), "回空闲可写");
    // 族四：历史满。
    let mut small = TermSession::new(2);
    let _ = small.step(E_CMD_START);
    let _ = small.step(E_CMD_DONE);
    let _ = small.step(E_CMD_START);
    let _ = small.step(E_CMD_DONE);
    let _ = small.step(E_CMD_START);
    cs.add("满账立案", small.step(E_CMD_DONE) == Err(HIST_FULL), "第 3 行逐出");
    // 族五：重放确定性。
    let evs = [E_CMD_START, E_CMD_DONE, E_REVIEW_OPEN, E_REVIEW_CLOSE, E_SCROLL];
    cs.add("重放一致", replay_deterministic(&evs, 8).is_ok(), "终态双同");
    cs.add("空序列拒", replay_deterministic(&[], 8).is_err(), "EVT_EMPTY");
    cs.add("事件越域拒", sess.step(9) == Err(EVT_OOR), "闭集外");
    cs
}
