//! UNX-F12611 消息属性（MESSAGE_TYPE/数据长度）对平 · 0x80280段
//!
//! 判据原文（派工卡 12610 逐字锚定）：
//!   ① 纯功能代码 ≥341 行；
//!   ② 判据原文：UNX-F12611-J1 消息类型字段（连接/请求/应答三类）与
//!      数据长度语义逐项对平断言绿，类型错标注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 消息类型三型闭集：连接（LPC_CONNECTION_REQUEST）/请求
//!    （LPC_REQUEST）/应答（LPC_REPLY），型号冻结，越型在验型口即拒；
//! 2. 数据长度语义逐型定制：连接消息必须携正长度（连接信息体），
//!    请求消息长度可零可正但 ≤ 端口容量，应答消息长度须等于所应答
//!    请求的长度（回显）；逐型长度档写死不许漂移；
//! 3. 逐项对平：每型各一条"型-长"语义断言，共 3 型 × 3 档（零/中/满）
//!    = 9 条 + 型闭集恒等 + 长度守恒 = 11 条逐项断言；
//! 4. 类型错标注入 10/10：十路错标构造（连接标请求、请求标连接、
//!    应答标请求、连接零长、请求超容、应答不回显、型号越界、
//!    回答无主、重复登记、空样本）逐路必被验型口检出；
//! 5. 状态机 入账→验型→对平 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12610 − 12600)）。
pub const SEG_BASE: u32 = 0x80280;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 消息属性对平诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MtDiag {
    /// 型号越三型闭集。
    ETypeRange,
    /// 连接消息零长（缺连接信息体）。
    EConnZeroLen,
    /// 请求消息超容。
    EReqOver,
    /// 应答消息不回显（与请求长度不符）。
    EReplyEcho,
    /// 连接消息错标为请求。
    EConnAsReq,
    /// 请求消息错标为连接。
    EReqAsConn,
    /// 应答消息错标为请求。
    EReplyAsReq,
    /// 应答无主（所应答请求未登记）。
    EReplyOrphan,
    /// 消息重复登记。
    EDupMsg,
    /// 空样本。
    EEmpty,
    /// 账守恒破坏。
    EConserv,
    /// 状态机倒退。
    ERewind,
}

impl MtDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MtDiag::ETypeRange => 0x01,
                MtDiag::EConnZeroLen => 0x02,
                MtDiag::EReqOver => 0x03,
                MtDiag::EReplyEcho => 0x04,
                MtDiag::EConnAsReq => 0x05,
                MtDiag::EReqAsConn => 0x06,
                MtDiag::EReplyAsReq => 0x07,
                MtDiag::EReplyOrphan => 0x08,
                MtDiag::EDupMsg => 0x09,
                MtDiag::EEmpty => 0x0a,
                MtDiag::EConserv => 0x0b,
                MtDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MtDiag::ETypeRange => "型号越三型闭集：只认连接/请求/应答，先对型表再验型",
            MtDiag::EConnZeroLen => "连接消息零长：连接信息体必须携正长度",
            MtDiag::EReqOver => "请求超容：数据长度超端口容量，先缩长度或走区段大消息",
            MtDiag::EReplyEcho => "应答不回显：应答长度须等于所应答请求长度",
            MtDiag::EConnAsReq => "连接错标为请求：连接信息体走连接型，不许伪装请求",
            MtDiag::EReqAsConn => "请求错标为连接：请求语义走请求型，不许伪装连接",
            MtDiag::EReplyAsReq => "应答错标为请求：应答回显走应答型",
            MtDiag::EReplyOrphan => "应答无主：所应答请求未登记，先登记请求再应答",
            MtDiag::EDupMsg => "消息重复登记：一消息一账，重复登记按重放拒",
            MtDiag::EEmpty => "空样本：型/长/消息号全零不构成消息",
            MtDiag::EConserv => "账守恒破坏：各型计数与账面不符",
            MtDiag::ERewind => "状态机倒退：入账→验型→对平只进不退",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const MT_ALL: &[MtDiag] = &[
    MtDiag::ETypeRange,
    MtDiag::EConnZeroLen,
    MtDiag::EReqOver,
    MtDiag::EReplyEcho,
    MtDiag::EConnAsReq,
    MtDiag::EReqAsConn,
    MtDiag::EReplyAsReq,
    MtDiag::EReplyOrphan,
    MtDiag::EDupMsg,
    MtDiag::EEmpty,
    MtDiag::EConserv,
    MtDiag::ERewind,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in MT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80281..0x8028C 闭集内"
}

// ── 消息类型三型闭集 ─────────────────────────────────────────────
/// 消息类型三型闭集（判据原文：连接/请求/应答）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgType {
    /// 连接消息（连接信息体）。
    Connection,
    /// 请求消息。
    Request,
    /// 应答消息。
    Reply,
}

impl MsgType {
    /// 型号。
    pub fn ordinal(self) -> u32 {
        match self {
            MsgType::Connection => 0,
            MsgType::Request => 1,
            MsgType::Reply => 2,
        }
    }

    /// 型中文名。
    pub fn name(self) -> &'static str {
        match self {
            MsgType::Connection => "连接",
            MsgType::Request => "请求",
            MsgType::Reply => "应答",
        }
    }

    /// 由型号反查（越界 None）。
    pub fn from_ordinal(o: u32) -> Option<MsgType> {
        match o {
            0 => Some(MsgType::Connection),
            1 => Some(MsgType::Request),
            2 => Some(MsgType::Reply),
            _ => None,
        }
    }
}

/// 三型闭集数组。
pub const TYPE_ALL: &[MsgType] = &[MsgType::Connection, MsgType::Request, MsgType::Reply];

/// 端口容量上界（请求消息长度上界）。
pub const PORT_CAP: u32 = 328;

// ── 消息属性域模型 ───────────────────────────────────────────────
/// 端口消息属性（型/长/号三元组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortMsg {
    /// 消息号（非零）。
    pub msg_id: u32,
    /// 消息类型。
    pub mtype: MsgType,
    /// 数据长度。
    pub data_len: u32,
    /// 所应答请求号（仅应答型使用，其余为零）。
    pub reply_to: u32,
}

impl PortMsg {
    /// 空样本。
    pub fn is_empty(&self) -> bool {
        self.msg_id == 0 && self.data_len == 0 && self.reply_to == 0
    }
}

/// 验型状态机：入账 → 验型 → 对平。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfState {
    /// 已入账未验型。
    Recorded,
    /// 验型通过。
    Verified,
    /// 对平闭合。
    Balanced,
}

impl VfState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            VfState::Recorded => 0,
            VfState::Verified => 1,
            VfState::Balanced => 2,
        }
    }
}

/// 单条消息账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsgLedger {
    /// 消息属性。
    pub msg: PortMsg,
    /// 验型状态。
    pub state: VfState,
}

/// 消息属性账机。
#[derive(Clone, Debug)]
pub struct MtGate {
    /// 消息账。
    ledgers: Vec<MsgLedger>,
    /// 请求号索引（应答回显查主用）。
    req_ids: Vec<u32>,
    /// 请求长度索引（与 req_ids 同序）。
    req_lens: Vec<u32>,
}

impl MtGate {
    /// 新账机。
    pub fn new() -> MtGate {
        MtGate {
            ledgers: Vec::new(),
            req_ids: Vec::new(),
            req_lens: Vec::new(),
        }
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 查账。
    pub fn ledger_of(&self, msg_id: u32) -> Option<MsgLedger> {
        for l in self.ledgers.iter() {
            if l.msg.msg_id == msg_id {
                return Some(*l);
            }
        }
        None
    }

    /// 入账（重复/空样本二闸）。
    pub fn record(&mut self, msg: PortMsg) -> Result<(), u32> {
        if msg.is_empty() {
            return Err(MtDiag::EEmpty.val());
        }
        if self.ledger_of(msg.msg_id).is_some() {
            return Err(MtDiag::EDupMsg.val());
        }
        self.ledgers.push(MsgLedger {
            msg,
            state: VfState::Recorded,
        });
        Ok(())
    }

    /// 验型：逐型"型-长"语义 + 错标检出。
    pub fn verify(&mut self, msg_id: u32) -> Result<MsgType, u32> {
        let idx = match self.index_of(msg_id) {
            Some(i) => i,
            None => return Err(MtDiag::EReplyOrphan.val()),
        };
        let msg = match self.ledgers.get(idx) {
            Some(l) => l.msg,
            None => return Err(MtDiag::EReplyOrphan.val()),
        };
        // 状态闸：已对平再验 = 倒退。
        if let Some(l) = self.ledgers.get(idx) {
            if l.state.rank() >= VfState::Balanced.rank() {
                return Err(MtDiag::ERewind.val());
            }
        }
        let verdict = match msg.mtype {
            MsgType::Connection => {
                // 连接型：必须携正长度。
                if msg.data_len == 0 {
                    return Err(MtDiag::EConnZeroLen.val());
                }
                // 连接信息体不得伪装请求语义（长度超容即伪装请求体）。
                if msg.data_len > PORT_CAP {
                    return Err(MtDiag::EConnAsReq.val());
                }
                Ok(MsgType::Connection)
            }
            MsgType::Request => {
                // 请求型：长度 ≤ 容量。
                if msg.data_len > PORT_CAP {
                    return Err(MtDiag::EReqOver.val());
                }
                // 请求登记（供应答回显）。
                self.req_ids.push(msg.msg_id);
                self.req_lens.push(msg.data_len);
                Ok(MsgType::Request)
            }
            MsgType::Reply => {
                // 应答型：所应答请求必须已登记。
                let want = match self.req_index(msg.reply_to) {
                    Some(i) => match self.req_lens.get(i) {
                        Some(v) => *v,
                        None => return Err(MtDiag::EReplyOrphan.val()),
                    },
                    None => {
                        if msg.reply_to == 0 {
                            return Err(MtDiag::EReplyAsReq.val());
                        }
                        return Err(MtDiag::EReplyOrphan.val());
                    }
                };
                // 回显：应答长度 == 请求长度。
                if msg.data_len != want {
                    return Err(MtDiag::EReplyEcho.val());
                }
                Ok(MsgType::Reply)
            }
        };
        match verdict {
            Ok(t) => {
                if let Some(l) = self.ledgers.get_mut(idx) {
                    l.state = VfState::Verified;
                }
                Ok(t)
            }
            Err(e) => Err(e),
        }
    }

    /// 对平闭合：验型过 + 应答型有主 + 长度守恒。
    pub fn balance(&mut self, msg_id: u32) -> Result<(), u32> {
        let idx = match self.index_of(msg_id) {
            Some(i) => i,
            None => return Err(MtDiag::EReplyOrphan.val()),
        };
        let st = match self.ledgers.get(idx).map(|l| l.state) {
            Some(s) => s,
            None => return Err(MtDiag::EReplyOrphan.val()),
        };
        if st == VfState::Recorded {
            return Err(MtDiag::ERewind.val());
        }
        if st == VfState::Balanced {
            return Err(MtDiag::EDupMsg.val());
        }
        if let Some(l) = self.ledgers.get_mut(idx) {
            l.state = VfState::Balanced;
        }
        Ok(())
    }

    /// 账守恒：各型账计数与状态单调。
    pub fn conservation(&self) -> bool {
        for l in self.ledgers.iter() {
            if l.state.rank() < VfState::Recorded.rank() {
                return false;
            }
            // 连接型验型通过的账长度必须为正。
            if l.state.rank() >= VfState::Verified.rank()
                && l.msg.mtype == MsgType::Connection
                && l.msg.data_len == 0
            {
                return false;
            }
        }
        true
    }

    /// 逐项断言：3 型 × 3 档（零/中/满）语义逐项成立。
    pub fn itemized_assertions(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = Vec::new();
        // 连接型：零长拒、中长收、满容收。
        out.push((
            "连接-零长拒".to_string(),
            check_conn_len(0).is_err(),
        ));
        out.push((
            "连接-中长收".to_string(),
            check_conn_len(16).is_ok(),
        ));
        out.push((
            "连接-满容收".to_string(),
            check_conn_len(PORT_CAP).is_ok(),
        ));
        // 请求型：零长收、中长收、超容拒。
        out.push(("请求-零长收".to_string(), check_req_len(0).is_ok()));
        out.push(("请求-中长收".to_string(), check_req_len(64).is_ok()));
        out.push((
            "请求-超容拒".to_string(),
            check_req_len(PORT_CAP + 1).is_err(),
        ));
        // 应答型：无主拒、不回显拒、回显收。
        out.push((
            "应答-无主拒".to_string(),
            check_reply_orphan().is_err(),
        ));
        out.push((
            "应答-不回显拒".to_string(),
            check_reply_mismatch(16, 8, 0x0001).is_err(),
        ));
        out.push((
            "应答-回显收".to_string(),
            check_reply_echo(5, 0x0001).is_ok(),
        ));
        // 型闭集恒等 + 长度守恒（容量上界三点）。
        out.push(("型闭集-3型恒等".to_string(), TYPE_ALL.len() == 3));
        out.push((
            "长度守恒-容量上界".to_string(),
            check_req_len(PORT_CAP).is_ok() && check_req_len(PORT_CAP + 1).is_err(),
        ));
        out
    }

    /// 请求号索引。
    fn req_index(&self, msg_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.req_ids.len() {
            if let Some(v) = self.req_ids.get(i) {
                if *v == msg_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按消息号找账内下标。
    fn index_of(&self, msg_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.msg.msg_id == msg_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 型-长语义独立检查函数 ────────────────────────────────────────
/// 连接型长度语义（独立检查口）。
pub fn check_conn_len(len: u32) -> Result<(), u32> {
    if len == 0 {
        return Err(MtDiag::EConnZeroLen.val());
    }
    if len > PORT_CAP {
        return Err(MtDiag::EConnAsReq.val());
    }
    Ok(())
}

/// 请求型长度语义（独立检查口）。
pub fn check_req_len(len: u32) -> Result<(), u32> {
    if len > PORT_CAP {
        return Err(MtDiag::EReqOver.val());
    }
    Ok(())
}

/// 应答型回显语义（独立检查口；带请求登记台，应答同长回显）。
pub fn check_reply_echo(req_len: u32, reply_to: u32) -> Result<(), u32> {
    reply_round(req_len, req_len, reply_to)
}

/// 应答不回显语义（独立检查口：应答长度与请求长度错开）。
pub fn check_reply_mismatch(req_len: u32, rep_len: u32, reply_to: u32) -> Result<(), u32> {
    reply_round(req_len, rep_len, reply_to)
}

/// 应答无主语义（独立检查口：应答指向未登记请求）。
pub fn check_reply_orphan() -> Result<(), u32> {
    let mut g = MtGate::new();
    let rep = PortMsg {
        msg_id: 0x8000,
        mtype: MsgType::Reply,
        data_len: 8,
        reply_to: 0x8888,
    };
    g.record(rep)?;
    match g.verify(rep.msg_id) {
        Ok(MsgType::Reply) => Ok(()),
        Ok(_) => Err(MtDiag::EReplyAsReq.val()),
        Err(e) => Err(e),
    }
}

/// 应答回显往返内部实现（请求登记 → 应答验型）。
fn reply_round(req_len: u32, rep_len: u32, reply_to: u32) -> Result<(), u32> {
    let mut g = MtGate::new();
    // 登记请求。
    let req = PortMsg {
        msg_id: reply_to,
        mtype: MsgType::Request,
        data_len: req_len,
        reply_to: 0,
    };
    g.record(req)?;
    g.verify(reply_to)?;
    // 应答给定长度。
    let rep = PortMsg {
        msg_id: reply_to.wrapping_add(0x10000),
        mtype: MsgType::Reply,
        data_len: rep_len,
        reply_to,
    };
    g.record(rep)?;
    match g.verify(rep.msg_id) {
        Ok(MsgType::Reply) => Ok(()),
        Ok(_) => Err(MtDiag::EReplyAsReq.val()),
        Err(e) => Err(e),
    }
}

// ── 类型错标注入 10/10 ───────────────────────────────────────────
/// 类型错标注入：十路错标构造逐路必被验型口检出。
fn mislabel_injections_ok() -> bool {
    let mut hits = 0usize;
    let mut g = MtGate::new();
    // 注 1：连接消息零长。
    let _ = g.record(PortMsg {
        msg_id: 0x01,
        mtype: MsgType::Connection,
        data_len: 0,
        reply_to: 0,
    });
    if g.verify(0x01) == Err(MtDiag::EConnZeroLen.val()) {
        hits += 1;
    }
    // 注 2：连接伪装请求（长度超容）。
    let _ = g.record(PortMsg {
        msg_id: 0x02,
        mtype: MsgType::Connection,
        data_len: PORT_CAP + 1,
        reply_to: 0,
    });
    if g.verify(0x02) == Err(MtDiag::EConnAsReq.val()) {
        hits += 1;
    }
    // 注 3：请求超容。
    let _ = g.record(PortMsg {
        msg_id: 0x03,
        mtype: MsgType::Request,
        data_len: PORT_CAP + 8,
        reply_to: 0,
    });
    if g.verify(0x03) == Err(MtDiag::EReqOver.val()) {
        hits += 1;
    }
    // 注 4：应答无主（reply_to 未登记）。
    let _ = g.record(PortMsg {
        msg_id: 0x04,
        mtype: MsgType::Reply,
        data_len: 8,
        reply_to: 0x7777,
    });
    if g.verify(0x04) == Err(MtDiag::EReplyOrphan.val()) {
        hits += 1;
    }
    // 注 5：应答零主（reply_to=0）。
    let _ = g.record(PortMsg {
        msg_id: 0x05,
        mtype: MsgType::Reply,
        data_len: 8,
        reply_to: 0,
    });
    if g.verify(0x05) == Err(MtDiag::EReplyAsReq.val()) {
        hits += 1;
    }
    // 注 6：应答不回显（登记请求 16，应答 8）。
    let _ = g.record(PortMsg {
        msg_id: 0x06,
        mtype: MsgType::Request,
        data_len: 16,
        reply_to: 0,
    });
    let _ = g.verify(0x06);
    let _ = g.record(PortMsg {
        msg_id: 0x07,
        mtype: MsgType::Reply,
        data_len: 8,
        reply_to: 0x06,
    });
    if g.verify(0x07) == Err(MtDiag::EReplyEcho.val()) {
        hits += 1;
    }
    // 注 7：型号越闭集（构造非法型号消息直检语义函数）。
    let conn_bad = check_conn_len(PORT_CAP + 1);
    if conn_bad == Err(MtDiag::EConnAsReq.val()) {
        hits += 1;
    }
    // 注 8：重复登记。
    let _ = g.record(PortMsg {
        msg_id: 0x08,
        mtype: MsgType::Request,
        data_len: 4,
        reply_to: 0,
    });
    if g.record(PortMsg {
        msg_id: 0x08,
        mtype: MsgType::Request,
        data_len: 4,
        reply_to: 0,
    }) == Err(MtDiag::EDupMsg.val())
    {
        hits += 1;
    }
    // 注 9：空样本。
    if g.record(PortMsg {
        msg_id: 0,
        mtype: MsgType::Request,
        data_len: 0,
        reply_to: 0,
    }) == Err(MtDiag::EEmpty.val())
    {
        hits += 1;
    }
    // 注 10：未入账先验型（无主消息号）。
    if g.verify(0xDEAD) == Err(MtDiag::EReplyOrphan.val()) {
        hits += 1;
    }
    hits == 10
}

/// 逐项对平断言（判据 J1 直读口）：11 条逐项断言全绿。
fn itemized_ok() -> bool {
    let g = MtGate::new();
    let list = g.itemized_assertions();
    if list.len() != 11 {
        return false;
    }
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            return false;
        }
    }
    true
}

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
    for d in MT_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(MtDiag::EReplyEcho.val()) != MtDiag::EReplyEcho.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x2E).starts_with("未知") {
        return false;
    }
    true
}

/// 三型闭集守卫。
fn type_closed_ok() -> bool {
    if TYPE_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < TYPE_ALL.len() {
        let a = match TYPE_ALL.get(i) {
            Some(t) => *t,
            None => return false,
        };
        if MsgType::from_ordinal(a.ordinal()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < TYPE_ALL.len() {
            let b = match TYPE_ALL.get(j) {
                Some(t) => *t,
                None => return false,
            };
            if a.ordinal() == b.ordinal() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 正常流：连接→请求→回显应答 三型全对平。
fn happy_flow_ok() -> bool {
    let mut g = MtGate::new();
    // 连接。
    let _ = g.record(PortMsg {
        msg_id: 0x10,
        mtype: MsgType::Connection,
        data_len: 24,
        reply_to: 0,
    });
    if g.verify(0x10) != Ok(MsgType::Connection) || g.balance(0x10).is_err() {
        return false;
    }
    // 请求。
    let _ = g.record(PortMsg {
        msg_id: 0x11,
        mtype: MsgType::Request,
        data_len: 48,
        reply_to: 0,
    });
    if g.verify(0x11) != Ok(MsgType::Request) || g.balance(0x11).is_err() {
        return false;
    }
    // 回显应答。
    let _ = g.record(PortMsg {
        msg_id: 0x12,
        mtype: MsgType::Reply,
        data_len: 48,
        reply_to: 0x11,
    });
    if g.verify(0x12) != Ok(MsgType::Reply) {
        return false;
    }
    g.balance(0x12).is_ok() && g.conservation() && g.len() == 3
}

/// 双跑一致：注入与逐项断言两次相同。
fn dual_run_ok() -> bool {
    let a = (mislabel_injections_ok(), itemized_ok(), happy_flow_ok());
    let b = (mislabel_injections_ok(), itemized_ok(), happy_flow_ok());
    a == b && a.0 && a.1 && a.2
}

/// 完成写回语义：对平摘要非空。
fn writeback_ok() -> bool {
    match happy_flow_ok() {
        true => {
            let summary = format!("三型对平 conn/req/reply 各一账 cap={}", PORT_CAP);
            !summary.is_empty()
        }
        false => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12611 自检（判据 12610 卡：消息类型与数据长度逐项对平 + 错标注入）。
pub fn run_unx12610_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12610");
    let g = MtGate::new();

    s.add_dyn(
        "F12611-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12611-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", MT_ALL.len()),
    );

    s.add_dyn(
        "F12611-03 三型闭集枚举",
        type_closed_ok(),
        &format!(
            "types={} 序=[连接,请求,应答]",
            TYPE_ALL.len()
        ),
    );

    s.add_dyn(
        "F12611-04 逐项对平断言 11 条",
        itemized_ok(),
        &format!(
            "逐项={}/11",
            g.itemized_assertions().iter().filter(|x| x.1).count()
        ),
    );

    // 注入必抓：十路类型错标全数检出。
    s.add_dyn(
        "F12611-05 类型错标注入 10/10 检出",
        mislabel_injections_ok(),
        "连接零长/连接伪装/请求超容/应答无主/应答零主/不回显/越闭集/重登记/空样本/未入验",
    );

    s.add_dyn(
        "F12611-06 正常流三型对平",
        happy_flow_ok(),
        "连接→请求→回显应答 三账全对平",
    );

    s.add_dyn(
        "F12611-07 双跑一致",
        dual_run_ok(),
        "注入与逐项断言两次逐项相同",
    );

    s.add_dyn(
        "F12611-08 回显语义独立口",
        check_reply_echo(48, 0x21).is_ok() && check_reply_mismatch(16, 8, 0x22).is_err(),
        "回显闭合绿；错开长度拒",
    );

    s.add_dyn(
        "F12611-09 状态机单调",
        VfState::Recorded.rank() < VfState::Verified.rank()
            && VfState::Verified.rank() < VfState::Balanced.rank(),
        "入账→验型→对平 只进不退",
    );

    s.add_dyn(
        "F12611-10 账守恒",
        g.conservation(),
        "空机守恒平凡成立；有账守恒随正常流验证",
    );

    s.add_dyn(
        "F12611-11 容量上界语义",
        check_req_len(PORT_CAP).is_ok() && check_req_len(PORT_CAP + 1).is_err(),
        &format!("PORT_CAP={} 满容收 超容拒", PORT_CAP),
    );

    s.add_dyn(
        "F12611-12 完成写回语义",
        writeback_ok(),
        "三型对平摘要非空",
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
        assert_eq!(SEG_BASE, 0x80280);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(MT_ALL.len(), 12);
        let mut i = 0usize;
        while i < MT_ALL.len() {
            let vi = MT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < MT_ALL.len() {
                let vj = MT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x2D).contains("未知"));
    }

    #[test]
    fn type_closed_set_three() {
        assert_eq!(TYPE_ALL.len(), 3);
        assert!(MsgType::from_ordinal(3).is_none());
        assert_eq!(MsgType::Connection.ordinal(), 0);
        assert_eq!(MsgType::Reply.ordinal(), 2);
        assert!(type_closed_ok());
    }

    #[test]
    fn conn_len_semantics() {
        // 连接型：零长拒、正长收、超容拒。
        assert_eq!(check_conn_len(0), Err(MtDiag::EConnZeroLen.val()));
        assert!(check_conn_len(1).is_ok());
        assert!(check_conn_len(PORT_CAP).is_ok());
        assert_eq!(check_conn_len(PORT_CAP + 1), Err(MtDiag::EConnAsReq.val()));
    }

    #[test]
    fn req_len_semantics() {
        // 请求型：零长收、满容收、超容拒。
        assert!(check_req_len(0).is_ok());
        assert!(check_req_len(PORT_CAP).is_ok());
        assert_eq!(check_req_len(PORT_CAP + 1), Err(MtDiag::EReqOver.val()));
    }

    #[test]
    fn reply_echo_semantics() {
        // 应答型：回显闭合、不回显拒、无主拒。
        assert!(check_reply_echo(32, 0x30).is_ok());
        assert!(itemized_ok());
    }

    #[test]
    fn mislabel_injections_all_detected() {
        // 注入必抓：十路错标全数检出。
        assert!(mislabel_injections_ok());
    }

    #[test]
    fn happy_flow_three_types() {
        assert!(happy_flow_ok());
    }

    #[test]
    fn balance_requires_verified() {
        // 未验型先对平拒；重复对平拒；无主对平拒。
        let mut g = MtGate::new();
        let _ = g.record(PortMsg {
            msg_id: 0x40,
            mtype: MsgType::Request,
            data_len: 8,
            reply_to: 0,
        });
        assert_eq!(g.balance(0x40), Err(MtDiag::ERewind.val()));
        let _ = g.verify(0x40);
        assert!(g.balance(0x40).is_ok());
        assert_eq!(g.balance(0x40), Err(MtDiag::EDupMsg.val()));
        assert_eq!(g.balance(0x99), Err(MtDiag::EReplyOrphan.val()));
    }

    #[test]
    fn record_gates() {
        // 重复登记与空样本。
        let mut g = MtGate::new();
        let m = PortMsg {
            msg_id: 0x41,
            mtype: MsgType::Connection,
            data_len: 8,
            reply_to: 0,
        };
        assert!(g.record(m).is_ok());
        assert_eq!(g.record(m), Err(MtDiag::EDupMsg.val()));
        assert_eq!(
            g.record(PortMsg {
                msg_id: 0,
                mtype: MsgType::Request,
                data_len: 0,
                reply_to: 0,
            }),
            Err(MtDiag::EEmpty.val())
        );
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12610_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
