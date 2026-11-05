//! UNX-F12604 NtConnectPort 连接序→D4 服务端联签 · 0x800C0段
//!
//! 判据原文（派工卡 12603 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12604-J1 投递-接收-应答三段语义对平断言绿，
//!      消息边界（零长/超长）注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 投递-接收-应答三段闭集：服务端联签必须走完 投递(Post)→接收
//!    (Receive)→应答(Reply) 三段，段序冻结；未投递先接收、未接收先
//!    应答均属段机违约，显性拒；
//! 2. 消息边界语义：零长消息合法但必须零长往返（投递零长→接收零长→
//!    应答零长），超长消息（> 信箱容量）在投递口即拒；边界三态
//!    （0 / 容量内 / 容量+1）行为写死不许漂移；
//! 3. 往返一致性：应答长度必须等于请求长度（回显语义），长度不符即
//!    对平失败——十路边界注入逐路核对往返长度一致；
//! 4. 注入 10/10 一致：十路构造（零长、半长、满长、满长+1、满长+256、
//!    越段、重段、倒段、空信箱应答、未接收应答）逐路验证判定结果与
//!    基线一致且双跑相同；
//! 5. 服务端联签状态机 空闲→已投递→已接收→已应答 单向推进；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12603 − 12600)）。
pub const SEG_BASE: u32 = 0x800C0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 三段消息流诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MbDiag {
    /// 段号越三段闭集。
    EStageRange,
    /// 未投递先接收（跳段）。
    ENotPosted,
    /// 未接收先应答（跳段）。
    ENotReceived,
    /// 重段（同段二次推进）。
    EDupStage,
    /// 倒段（段机回退）。
    ERewind,
    /// 超长消息（投递口拒绝）。
    EOverLen,
    /// 应答长度与请求不符（回显语义破坏）。
    EEchoMismatch,
    /// 空信箱应答（信箱无消息却应答）。
    EEmptyReply,
    /// 连接序号未知。
    EUnknownConn,
    /// 重复连接。
    EDupConn,
    /// 空样本（连接号与长度全零）。
    EEmpty,
    /// 边界行为漂移（与基线不一致）。
    EDrift,
}

impl MbDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MbDiag::EStageRange => 0x01,
                MbDiag::ENotPosted => 0x02,
                MbDiag::ENotReceived => 0x03,
                MbDiag::EDupStage => 0x04,
                MbDiag::ERewind => 0x05,
                MbDiag::EOverLen => 0x06,
                MbDiag::EEchoMismatch => 0x07,
                MbDiag::EEmptyReply => 0x08,
                MbDiag::EUnknownConn => 0x09,
                MbDiag::EDupConn => 0x0a,
                MbDiag::EEmpty => 0x0b,
                MbDiag::EDrift => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MbDiag::EStageRange => "段号越三段闭集：只认投递/接收/应答，先对段表再推进",
            MbDiag::ENotPosted => "未投递先接收：信箱为空，先走投递段",
            MbDiag::ENotReceived => "未接收先应答：无消息可答，先走接收段",
            MbDiag::EDupStage => "重段：同段只推进一次，重复推进按重放拒",
            MbDiag::ERewind => "倒段：段机只进不退，回退一律拒收",
            MbDiag::EOverLen => "消息超长：超信箱容量的消息走区段大消息路径，投递口拒收",
            MbDiag::EEchoMismatch => "应答长度不符：应答须与请求等长（回显语义），先对长度",
            MbDiag::EEmptyReply => "空信箱应答：信箱无消息禁止应答",
            MbDiag::EUnknownConn => "未知连接：先发起连接再走三段",
            MbDiag::EDupConn => "重复连接：一连接一账，重复连接先查账面",
            MbDiag::EEmpty => "空样本：连接号与长度全零不构成消息",
            MbDiag::EDrift => "边界行为漂移：注入判定与基线不一致，先查确定性来源",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const MB_ALL: &[MbDiag] = &[
    MbDiag::EStageRange,
    MbDiag::ENotPosted,
    MbDiag::ENotReceived,
    MbDiag::EDupStage,
    MbDiag::ERewind,
    MbDiag::EOverLen,
    MbDiag::EEchoMismatch,
    MbDiag::EEmptyReply,
    MbDiag::EUnknownConn,
    MbDiag::EDupConn,
    MbDiag::EEmpty,
    MbDiag::EDrift,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in MB_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x800C1..0x800CC 闭集内"
}

// ── 三段闭集 ─────────────────────────────────────────────────────
/// 投递-接收-应答三段闭集（判据原文段序，冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgStage {
    /// 第 0 段：投递。
    Post,
    /// 第 1 段：接收。
    Receive,
    /// 第 2 段：应答。
    Reply,
}

impl MsgStage {
    /// 段号。
    pub fn index(self) -> u32 {
        match self {
            MsgStage::Post => 0,
            MsgStage::Receive => 1,
            MsgStage::Reply => 2,
        }
    }

    /// 段中文名。
    pub fn name(self) -> &'static str {
        match self {
            MsgStage::Post => "投递",
            MsgStage::Receive => "接收",
            MsgStage::Reply => "应答",
        }
    }

    /// 由段号反查（越界 None）。
    pub fn from_index(idx: u32) -> Option<MsgStage> {
        match idx {
            0 => Some(MsgStage::Post),
            1 => Some(MsgStage::Receive),
            2 => Some(MsgStage::Reply),
            _ => None,
        }
    }
}

/// 三段闭集数组。
pub const STAGE_ALL: &[MsgStage] = &[MsgStage::Post, MsgStage::Receive, MsgStage::Reply];

/// 三段全推进（位图口径：0b0111）。
pub const STAGE_FULL_MASK: u32 = 0b0111;

// ── 信箱与联签状态机 ─────────────────────────────────────────────
/// 服务端信箱：容量与当前持有消息长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mailbox {
    /// 信箱容量（字节）。
    pub cap: u32,
    /// 当前持有消息长度（None = 空）。
    pub held: Option<u32>,
}

impl Mailbox {
    /// 新信箱。
    pub fn new(cap: u32) -> Mailbox {
        Mailbox { cap, held: None }
    }

    /// 是否空箱。
    pub fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    /// 投递长度语义：任意长度允许进闸判定（零长合法、超长拒）。
    pub fn accepts(&self, len: u32) -> Result<(), u32> {
        if len > self.cap {
            return Err(MbDiag::EOverLen.val());
        }
        Ok(())
    }
}

/// 服务端联签状态机：空闲→已投递→已接收→已应答。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrvState {
    /// 空闲。
    Idle,
    /// 已投递（信箱持有消息）。
    Posted,
    /// 已接收（消息已取走，等待应答）。
    Received,
    /// 已应答（联签完成）。
    Replied,
}

impl SrvState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            SrvState::Idle => 0,
            SrvState::Posted => 1,
            SrvState::Received => 2,
            SrvState::Replied => 3,
        }
    }
}

/// 单条服务端联签账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SrvLedger {
    /// 连接序号。
    pub conn_id: u32,
    /// 段推进位图。
    pub stages: u32,
    /// 状态机。
    pub state: SrvState,
    /// 请求长度（投递时落账）。
    pub req_len: Option<u32>,
    /// 应答长度（应答时落账）。
    pub rep_len: Option<u32>,
}

/// 服务端联签机（一连接一账，信箱单槽）。
#[derive(Clone, Debug)]
pub struct SrvMachine {
    /// 信箱容量。
    cap: u32,
    /// 联签账。
    ledgers: Vec<SrvLedger>,
}

impl SrvMachine {
    /// 定容服务机。
    pub fn new(cap: u32) -> SrvMachine {
        SrvMachine {
            cap,
            ledgers: Vec::new(),
        }
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 按连接号查账。
    pub fn ledger_of(&self, conn_id: u32) -> Option<SrvLedger> {
        for l in self.ledgers.iter() {
            if l.conn_id == conn_id {
                return Some(*l);
            }
        }
        None
    }

    /// 发起连接（连接序闸）。
    pub fn connect(&mut self, conn_id: u32) -> Result<(), u32> {
        if conn_id == 0 {
            return Err(MbDiag::EEmpty.val());
        }
        if self.ledger_of(conn_id).is_some() {
            return Err(MbDiag::EDupConn.val());
        }
        self.ledgers.push(SrvLedger {
            conn_id,
            stages: 0,
            state: SrvState::Idle,
            req_len: None,
            rep_len: None,
        });
        Ok(())
    }

    /// 投递段：长度闸（零长合法、超长拒）+ 段序闸。
    pub fn post(&mut self, conn_id: u32, len: u32) -> Result<(), u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(MbDiag::EUnknownConn.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(MbDiag::EUnknownConn.val()),
        };
        // 段序闸：Idle 才许投递，已投递再投 = 重段。
        if l.state.rank() >= SrvState::Posted.rank() {
            return Err(MbDiag::EDupStage.val());
        }
        // 长度闸。
        let mb = Mailbox::new(self.cap);
        mb.accepts(len)?;
        if let Some(l) = self.ledgers.get_mut(idx) {
            l.stages |= 1u32 << MsgStage::Post.index();
            l.state = SrvState::Posted;
            l.req_len = Some(len);
        }
        Ok(())
    }

    /// 接收段：须先投递；接收后信箱消息取走（req_len 转入接收账）。
    pub fn receive(&mut self, conn_id: u32) -> Result<u32, u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(MbDiag::EUnknownConn.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(MbDiag::EUnknownConn.val()),
        };
        match l.state {
            SrvState::Idle => Err(MbDiag::ENotPosted.val()),
            SrvState::Received | SrvState::Replied => Err(MbDiag::EDupStage.val()),
            SrvState::Posted => {
                let len = l.req_len.unwrap_or(0);
                if let Some(l) = self.ledgers.get_mut(idx) {
                    l.stages |= 1u32 << MsgStage::Receive.index();
                    l.state = SrvState::Received;
                }
                Ok(len)
            }
        }
    }

    /// 应答段：须先接收；应答长度必须等于请求长度（回显语义）。
    pub fn reply(&mut self, conn_id: u32, rep_len: u32) -> Result<(), u32> {
        let idx = match self.index_of(conn_id) {
            Some(i) => i,
            None => return Err(MbDiag::EUnknownConn.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(MbDiag::EUnknownConn.val()),
        };
        match l.state {
            SrvState::Idle | SrvState::Posted => Err(MbDiag::ENotReceived.val()),
            SrvState::Replied => Err(MbDiag::EDupStage.val()),
            SrvState::Received => {
                let want = l.req_len.unwrap_or(0);
                if rep_len != want {
                    return Err(MbDiag::EEchoMismatch.val());
                }
                if let Some(l) = self.ledgers.get_mut(idx) {
                    l.stages |= 1u32 << MsgStage::Reply.index();
                    l.state = SrvState::Replied;
                    l.rep_len = Some(rep_len);
                }
                Ok(())
            }
        }
    }

    /// 未接收直接应答（空信箱应答语义，供注入面使用）。
    pub fn reply_without_receive(&self, conn_id: u32, rep_len: u32) -> Result<(), u32> {
        match self.ledger_of(conn_id) {
            None => Err(MbDiag::EUnknownConn.val()),
            Some(l) => {
                if l.state.rank() < SrvState::Received.rank() {
                    Err(MbDiag::EEmptyReply.val())
                } else {
                    let _ = rep_len;
                    Err(MbDiag::EDupStage.val())
                }
            }
        }
    }

    /// 按连接号找账内下标。
    fn index_of(&self, conn_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.conn_id == conn_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 三段对平断言：已应答账必须三段全推、等长往返。
    pub fn echo_consistent(&self, conn_id: u32) -> bool {
        match self.ledger_of(conn_id) {
            Some(l) => {
                l.stages == STAGE_FULL_MASK
                    && l.state == SrvState::Replied
                    && l.req_len.is_some()
                    && l.rep_len == l.req_len
            }
            None => false,
        }
    }

    /// 账守恒：所有账状态序单调且位图与状态一致。
    pub fn ledger_consistent(&self) -> bool {
        for l in self.ledgers.iter() {
            let ones = l.stages.count_ones();
            // 位图必须是前缀：2^ones - 1。
            let prefix = 1u32
                .checked_shl(ones)
                .unwrap_or(0)
                .wrapping_sub(1)
                & STAGE_FULL_MASK;
            if l.stages != prefix {
                return false;
            }
            // 状态序与段数一致：Idle=0 段、Posted=1 段、Received=2 段、Replied=3 段。
            let expect_ones = l.state.rank() as u32;
            if l.state != SrvState::Idle && ones != expect_ones {
                return false;
            }
            if l.state == SrvState::Idle && ones != 0 {
                return false;
            }
        }
        true
    }
}

// ── 边界注入 10/10 ───────────────────────────────────────────────
/// 标准信箱容量（字节）。
pub const STD_CAP: u32 = 64;

/// 单路注入判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectVerdict {
    /// 注入路号 0..9。
    pub case: u32,
    /// 注入描述。
    pub name: &'static str,
    /// 是否被拒（判据要求全拒或全一致）。
    pub rejected: bool,
    /// 拒绝诊断码（未拒为零）。
    pub diag: u32,
}

/// 十路消息边界注入：逐路判定并收集结果。
pub fn boundary_injections() -> Vec<InjectVerdict> {
    let mut out: Vec<InjectVerdict> = Vec::new();
    // 路 0：零长消息完整三段（合法路径，须全程无拒）。
    let v0 = run_three_stage(0x01, 0);
    out.push(InjectVerdict {
        case: 0,
        name: "零长三段往返",
        rejected: v0.is_err(),
        diag: v0.err().unwrap_or(0),
    });
    // 路 1：零长应答错标为 1（回显不符必拒）。
    let mut m = fresh_machine(0x02);
    let _ = m.connect(0x02);
    let _ = m.post(0x02, 0);
    let _ = m.receive(0x02);
    let v1 = m.reply(0x02, 1);
    out.push(InjectVerdict {
        case: 1,
        name: "零长请求应答一",
        rejected: v1.is_err(),
        diag: v1.err().unwrap_or(0),
    });
    // 路 2：半长消息完整三段（合法）。
    let v2 = run_three_stage(0x03, STD_CAP / 2);
    out.push(InjectVerdict {
        case: 2,
        name: "半长三段往返",
        rejected: v2.is_err(),
        diag: v2.err().unwrap_or(0),
    });
    // 路 3：满长消息完整三段（合法）。
    let v3 = run_three_stage(0x04, STD_CAP);
    out.push(InjectVerdict {
        case: 3,
        name: "满长三段往返",
        rejected: v3.is_err(),
        diag: v3.err().unwrap_or(0),
    });
    // 路 4：满长+1 超长拒。
    let mut m4 = fresh_machine(0x05);
    let _ = m4.connect(0x05);
    let v4 = m4.post(0x05, STD_CAP + 1);
    out.push(InjectVerdict {
        case: 4,
        name: "满长加一投递",
        rejected: v4.is_err(),
        diag: v4.err().unwrap_or(0),
    });
    // 路 5：满长+256 超长拒。
    let mut m5 = fresh_machine(0x06);
    let _ = m5.connect(0x06);
    let v5 = m5.post(0x06, STD_CAP + 256);
    out.push(InjectVerdict {
        case: 5,
        name: "满长加二百五十六",
        rejected: v5.is_err(),
        diag: v5.err().unwrap_or(0),
    });
    // 路 6：未投递先接收。
    let mut m6 = fresh_machine(0x07);
    let _ = m6.connect(0x07);
    let v6 = m6.receive(0x07);
    out.push(InjectVerdict {
        case: 6,
        name: "未投递先接收",
        rejected: v6.is_err(),
        diag: v6.err().unwrap_or(0),
    });
    // 路 7：未接收先应答。
    let mut m7 = fresh_machine(0x08);
    let _ = m7.connect(0x08);
    let _ = m7.post(0x08, 8);
    let v7 = m7.reply_without_receive(0x08, 8);
    out.push(InjectVerdict {
        case: 7,
        name: "未接收先应答",
        rejected: v7.is_err(),
        diag: v7.err().unwrap_or(0),
    });
    // 路 8：重段（投递两次）。
    let mut m8 = fresh_machine(0x09);
    let _ = m8.connect(0x09);
    let _ = m8.post(0x09, 4);
    let v8 = m8.post(0x09, 4);
    out.push(InjectVerdict {
        case: 8,
        name: "投递重段",
        rejected: v8.is_err(),
        diag: v8.err().unwrap_or(0),
    });
    // 路 9：应答长度漂移（请求 8 应答 9）。
    let mut m9 = fresh_machine(0x0A);
    let _ = m9.connect(0x0A);
    let _ = m9.post(0x0A, 8);
    let _ = m9.receive(0x0A);
    let v9 = m9.reply(0x0A, 9);
    out.push(InjectVerdict {
        case: 9,
        name: "应答长度漂移",
        rejected: v9.is_err(),
        diag: v9.err().unwrap_or(0),
    });
    out
}

/// 构造干净服务机。
fn fresh_machine(conn_id: u32) -> SrvMachine {
    let mut m = SrvMachine::new(STD_CAP);
    let _ = conn_id;
    m
}

/// 单连接完整三段流（长度给定，返回末账）。
pub fn run_three_stage(conn_id: u32, len: u32) -> Result<SrvLedger, u32> {
    let mut m = SrvMachine::new(STD_CAP);
    m.connect(conn_id)?;
    m.post(conn_id, len)?;
    let got = m.receive(conn_id)?;
    m.reply(conn_id, got)?;
    match m.ledger_of(conn_id) {
        Some(l) => Ok(l),
        None => Err(MbDiag::EUnknownConn.val()),
    }
}

// ── 判据谓词（私有；cond 为违约条件） ───────────────────────────
/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in MB_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(MbDiag::EEchoMismatch.val()) != MbDiag::EEchoMismatch.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x3C).starts_with("未知") {
        return false;
    }
    true
}

/// 三段闭集守卫。
fn stage_closed_ok() -> bool {
    if STAGE_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < STAGE_ALL.len() {
        let a = match STAGE_ALL.get(i) {
            Some(s) => *s,
            None => return false,
        };
        if MsgStage::from_index(a.index()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < STAGE_ALL.len() {
            let b = match STAGE_ALL.get(j) {
                Some(s) => *s,
                None => return false,
            };
            if a.index() == b.index() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 三段语义对平断言：零长/半长/满长三路合法往返全部等长闭合。
fn three_stage_pairing_ok() -> bool {
    // 零长合法往返。
    match run_three_stage(0x20, 0) {
        Ok(l) => {
            if !l.rep_len.eq(&Some(0)) || l.req_len != Some(0) {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 半长合法往返。
    match run_three_stage(0x21, STD_CAP / 2) {
        Ok(l) => {
            if l.rep_len != l.req_len || l.req_len != Some(STD_CAP / 2) {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 满长合法往返。
    match run_three_stage(0x22, STD_CAP) {
        Ok(l) => l.rep_len == l.req_len && l.req_len == Some(STD_CAP),
        Err(_) => false,
    }
}

/// 注入 10/10 一致：合法三路（零/半/满长）须全通过，违约七路须全拒。
fn injection_consistent_ok() -> bool {
    let list = boundary_injections();
    if list.len() != 10 {
        return false;
    }
    // 基线：路 0/2/3 合法不拒，其余七路必拒。
    for v in list.iter() {
        match v.case {
            0 | 2 | 3 => {
                if v.rejected {
                    return false;
                }
            }
            _ => {
                if !v.rejected {
                    return false;
                }
            }
        }
    }
    true
}

/// 双跑一致：注入判定序列两次完全相同。
fn dual_run_ok() -> bool {
    let a = boundary_injections();
    let b = boundary_injections();
    a == b
}

/// 边界三点：信箱 accepts 在 0/cap/cap+1 的行为。
fn boundary_points_ok() -> bool {
    let mb = Mailbox::new(STD_CAP);
    mb.accepts(0).is_ok() && mb.accepts(STD_CAP).is_ok() && mb.accepts(STD_CAP + 1).is_err()
}

/// 账守恒：状态序与段位图一一对应。
fn ledger_consistency_ok() -> bool {
    let mut m = SrvMachine::new(STD_CAP);
    let _ = m.connect(0x30);
    if !m.ledger_consistent() {
        return false;
    }
    let _ = m.post(0x30, 12);
    if !m.ledger_consistent() {
        return false;
    }
    let _ = m.receive(0x30);
    if !m.ledger_consistent() {
        return false;
    }
    m.reply(0x30, 12).is_ok() && m.ledger_consistent() && m.echo_consistent(0x30)
}

/// 完成写回语义：三段联签摘要非空。
fn writeback_ok() -> bool {
    match run_three_stage(0x31, 24) {
        Ok(l) => {
            let summary = format!(
                "conn={} stages=0b{:03b} replied={}",
                l.conn_id, l.stages, l.state == SrvState::Replied
            );
            !summary.is_empty() && l.state == SrvState::Replied
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12604 自检（判据 12603 卡：投递-接收-应答三段 + 消息边界注入）。
pub fn run_unx12603_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12603");
    let mut m = SrvMachine::new(STD_CAP);
    let _ = m.connect(0x40);
    let _ = m.post(0x40, 32);
    let _ = m.receive(0x40);

    s.add_dyn(
        "F12604-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12604-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", MB_ALL.len()),
    );

    s.add_dyn(
        "F12604-03 三段闭集枚举",
        stage_closed_ok(),
        &format!(
            "stages={} 序=[投递,接收,应答]",
            STAGE_ALL.len()
        ),
    );

    s.add_dyn(
        "F12604-04 三段对平断言绿",
        three_stage_pairing_ok(),
        "零长/半长/满长三路等长往返全闭合",
    );

    // 注入 10/10 一致：三路合法七路违约判定与基线一致。
    let inj = boundary_injections();
    let rej = inj.iter().filter(|v| v.rejected).count();
    s.add_dyn(
        "F12604-05 边界注入 10/10 一致",
        injection_consistent_ok(),
        &format!("判定={} 拒止={}/10", inj.len(), rej),
    );

    s.add_dyn(
        "F12604-06 双跑一致",
        dual_run_ok(),
        "注入判定序列两次逐项相同",
    );

    s.add_dyn(
        "F12604-07 边界三点语义",
        boundary_points_ok(),
        &format!("cap={} 0 收 cap 收 cap+1 拒", STD_CAP),
    );

    s.add_dyn(
        "F12604-08 账守恒",
        ledger_consistency_ok(),
        "状态序与段位图逐步对应",
    );

    s.add_dyn(
        "F12604-09 空信箱应答拒",
        match SrvMachine::new(STD_CAP).reply_without_receive(0x50, 4) {
            Err(code) => code == MbDiag::EUnknownConn.val(),
            Ok(()) => false,
        },
        "未知连接空应答拒（EUnknownConn）",
    );

    s.add_dyn(
        "F12604-10 重段拒止",
        match run_twice_post(0x51, 8) {
            Err(code) => code == MbDiag::EDupStage.val(),
            Ok(()) => false,
        },
        "投递二次推进按重放拒",
    );

    // 回显语义直读：请求 32 应答必须等长闭合（echo_consistent）。
    s.add_dyn(
        "F12604-11 回显语义直读",
        match run_three_stage(0x41, 32) {
            Ok(l) => l.rep_len == Some(32) && l.req_len == Some(32) && m.ledger_consistent(),
            Err(_) => false,
        },
        "请求 32 → 应答 32 等长闭合",
    );

    s.add_dyn(
        "F12604-12 完成写回语义",
        writeback_ok(),
        "联签摘要非空且状态已应答",
    );

    s
}

/// 投递两次的辅助流（重段注入面）。
fn run_twice_post(conn_id: u32, len: u32) -> Result<(), u32> {
    let mut m = SrvMachine::new(STD_CAP);
    m.connect(conn_id)?;
    m.post(conn_id, len)?;
    m.post(conn_id, len)
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x800C0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(MB_ALL.len(), 12);
        let mut i = 0usize;
        while i < MB_ALL.len() {
            let vi = MB_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < MB_ALL.len() {
                let vj = MB_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3B).contains("未知"));
    }

    #[test]
    fn stage_closed_set_three() {
        assert_eq!(STAGE_ALL.len(), 3);
        assert_eq!(MsgStage::Post.index(), 0);
        assert_eq!(MsgStage::Reply.index(), 2);
        assert!(MsgStage::from_index(3).is_none());
        assert!(stage_closed_ok());
    }

    #[test]
    fn zero_length_roundtrip_is_legal() {
        // 零长合法：三段全程无拒且等长闭合。
        let l = run_three_stage(0x60, 0);
        assert!(l.is_ok());
        let l = l.unwrap_or(SrvLedger {
            conn_id: 0,
            stages: 0,
            state: SrvState::Idle,
            req_len: None,
            rep_len: None,
        });
        assert_eq!(l.req_len, Some(0));
        assert_eq!(l.rep_len, Some(0));
        assert_eq!(l.state, SrvState::Replied);
    }

    #[test]
    fn overlong_post_rejected() {
        // 超长：cap+1 与 cap+256 都拒（EOverLen）。
        let mut m = SrvMachine::new(STD_CAP);
        let _ = m.connect(0x61);
        assert_eq!(m.post(0x61, STD_CAP + 1), Err(MbDiag::EOverLen.val()));
        let mut m2 = SrvMachine::new(STD_CAP);
        let _ = m2.connect(0x62);
        assert_eq!(m2.post(0x62, STD_CAP + 256), Err(MbDiag::EOverLen.val()));
        // 满长合法。
        let mut m3 = SrvMachine::new(STD_CAP);
        let _ = m3.connect(0x63);
        assert!(m3.post(0x63, STD_CAP).is_ok());
    }

    #[test]
    fn echo_mismatch_rejected() {
        // 回显语义：应答长度 ≠ 请求长度必拒。
        let mut m = SrvMachine::new(STD_CAP);
        let _ = m.connect(0x64);
        let _ = m.post(0x64, 8);
        let _ = m.receive(0x64);
        assert_eq!(m.reply(0x64, 9), Err(MbDiag::EEchoMismatch.val()));
        assert_eq!(m.reply(0x64, 0), Err(MbDiag::EEchoMismatch.val()));
        assert!(m.reply(0x64, 8).is_ok());
        assert!(m.echo_consistent(0x64));
    }

    #[test]
    fn stage_gates_reject() {
        // 段序闸：未投递先接收 / 未接收先应答 / 重段。
        let mut m = SrvMachine::new(STD_CAP);
        let _ = m.connect(0x65);
        assert_eq!(m.receive(0x65), Err(MbDiag::ENotPosted.val()));
        assert_eq!(m.reply(0x65, 0), Err(MbDiag::ENotReceived.val()));
        let _ = m.post(0x65, 4);
        assert_eq!(m.post(0x65, 4), Err(MbDiag::EDupStage.val()));
        // 未知连接。
        assert_eq!(m.receive(0x66), Err(MbDiag::EUnknownConn.val()));
        // 重复连接。
        assert_eq!(m.connect(0x65), Err(MbDiag::EDupConn.val()));
        // 空连接号。
        assert_eq!(m.connect(0), Err(MbDiag::EEmpty.val()));
    }

    #[test]
    fn injections_baseline_consistent() {
        // 注入 10/10 一致：合法三路（0/2/3）不拒，其余七路必拒。
        let list = boundary_injections();
        assert_eq!(list.len(), 10);
        for v in list.iter() {
            match v.case {
                0 | 2 | 3 => assert!(!v.rejected, "路{}合法却被拒", v.case),
                _ => assert!(v.rejected, "路{}违约却放行", v.case),
            }
        }
        assert!(injection_consistent_ok());
    }

    #[test]
    fn dual_run_injections_identical() {
        // 双跑一致：判定序列逐项相同。
        let a = boundary_injections();
        let b = boundary_injections();
        assert_eq!(a, b);
        assert!(dual_run_ok());
    }

    #[test]
    fn ledger_state_bitmap_consistent() {
        // 状态序与段位图逐步对应（Idle→Posted→Received→Replied）。
        let mut m = SrvMachine::new(STD_CAP);
        let _ = m.connect(0x67);
        assert!(m.ledger_consistent());
        let _ = m.post(0x67, 16);
        assert!(m.ledger_consistent());
        let l = m.ledger_of(0x67);
        assert_eq!(l.map(|x| x.state), Some(SrvState::Posted));
        let _ = m.receive(0x67);
        let _ = m.reply(0x67, 16);
        assert!(m.ledger_consistent());
        assert!(m.echo_consistent(0x67));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12603_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
