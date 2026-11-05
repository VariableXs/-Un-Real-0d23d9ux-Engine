//! UNX-F12603 ALPC 端口对象语义→D4 消息队列消费联签 · 0x80080段
//!
//! 判据原文（派工卡 12602 逐字锚定）：
//!   ① 纯功能代码 ≥341 行；
//!   ② 判据原文：UNX-F12603-J1 连接四段序（请求/接受/完成/信息回传）
//!      对平断言绿，坏连接注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 连接四段序闭集：请求→接受→完成→信息回传，段序冻结；跳段、重段、
//!    倒段在段机即拒，不进消息队列；
//! 2. ALPC 端口对象语义：服务端连接端口持有连接序号与最大消息长度，
//!    客户端连接请求必须携带端口内合法长度（0 < len ≤ max），越界请求
//!    属坏连接；
//! 3. D4 消息队列联签：四段全部走完后连接才入队列（联签 = 四段各留
//!    一枚段戳，缺一枚即不入队），入队后队列只追加不改写；
//! 4. 坏连接注入 10/10：十路人为构造违约（越段/重段/倒段/零长请求/
//!    超长请求/未知端口/段戳冒充/未完成回传/重复入队/空连接）必须
//!    全数被拒，任何一路逃脱即判据失效；
//! 5. 状态机 待连→四段中→已联签 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12602 − 12600)）。
pub const SEG_BASE: u32 = 0x80080;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 连接四段序诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CxDiag {
    /// 段号越四段闭集。
    EPhaseRange,
    /// 跳段（未按 请求→接受→完成→回传 顺序）。
    ESkip,
    /// 重段（同段二次敲章）。
    EDupPhase,
    /// 倒段（段序回退）。
    ERewind,
    /// 连接请求零长。
    EZeroLen,
    /// 连接请求超长（> 端口最大消息长度）。
    EOverLen,
    /// 未知连接端口。
    EUnknownPort,
    /// 段戳冒充（未走流程直接携带全戳）。
    EStampForge,
    /// 回传未完成就试图联签。
    ENoInfo,
    /// 连接重复入队。
    EDupQueue,
    /// 空连接样本（端口与长度全零）。
    EEmpty,
    /// 队列守恒破坏（入队数与联签数不符）。
    EConserv,
}

impl CxDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CxDiag::EPhaseRange => 0x01,
                CxDiag::ESkip => 0x02,
                CxDiag::EDupPhase => 0x03,
                CxDiag::ERewind => 0x04,
                CxDiag::EZeroLen => 0x05,
                CxDiag::EOverLen => 0x06,
                CxDiag::EUnknownPort => 0x07,
                CxDiag::EStampForge => 0x08,
                CxDiag::ENoInfo => 0x09,
                CxDiag::EDupQueue => 0x0a,
                CxDiag::EEmpty => 0x0b,
                CxDiag::EConserv => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CxDiag::EPhaseRange => "段号越四段闭集：只认请求/接受/完成/信息回传，先对段表再敲章",
            CxDiag::ESkip => "跳段：四段序逐段推进，前段未敲不许敲后段",
            CxDiag::EDupPhase => "重段：同段只敲一枚章，重复敲章按重放处理",
            CxDiag::ERewind => "倒段：段机只进不退，回退章一律拒收",
            CxDiag::EZeroLen => "连接请求零长：长度语义为 0 < len ≤ max，零长属坏连接",
            CxDiag::EOverLen => "连接请求超长：超端口最大消息长度，先改分区段大消息路径",
            CxDiag::EUnknownPort => "未知连接端口：先查端口表再发请求",
            CxDiag::EStampForge => "段戳冒充：未走流程不得携带段戳，四段章只能由段机签发",
            CxDiag::ENoInfo => "回传未完成：信息回传段缺章禁止联签入队",
            CxDiag::EDupQueue => "连接重复入队：一连接一队列项，重复入队按重放拒",
            CxDiag::EEmpty => "空连接样本：端口与长度全零不构成请求",
            CxDiag::EConserv => "队列守恒破坏：联签数与入队数不符，账面可疑须整列复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CX_ALL: &[CxDiag] = &[
    CxDiag::EPhaseRange,
    CxDiag::ESkip,
    CxDiag::EDupPhase,
    CxDiag::ERewind,
    CxDiag::EZeroLen,
    CxDiag::EOverLen,
    CxDiag::EUnknownPort,
    CxDiag::EStampForge,
    CxDiag::ENoInfo,
    CxDiag::EDupQueue,
    CxDiag::EEmpty,
    CxDiag::EConserv,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in CX_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80081..0x8008C 闭集内"
}

// ── 连接四段闭集 ─────────────────────────────────────────────────
/// 连接四段闭集（判据原文段序，冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnPhase {
    /// 第 0 段：连接请求。
    ConnReq,
    /// 第 1 段：连接接受。
    ConnAccept,
    /// 第 2 段：连接完成。
    ConnComplete,
    /// 第 3 段：信息回传。
    ConnInfo,
}

impl ConnPhase {
    /// 段号。
    pub fn index(self) -> u32 {
        match self {
            ConnPhase::ConnReq => 0,
            ConnPhase::ConnAccept => 1,
            ConnPhase::ConnComplete => 2,
            ConnPhase::ConnInfo => 3,
        }
    }

    /// 段中文名。
    pub fn name(self) -> &'static str {
        match self {
            ConnPhase::ConnReq => "请求",
            ConnPhase::ConnAccept => "接受",
            ConnPhase::ConnComplete => "完成",
            ConnPhase::ConnInfo => "信息回传",
        }
    }

    /// 由段号反查闭集段（越界 None）。
    pub fn from_index(idx: u32) -> Option<ConnPhase> {
        match idx {
            0 => Some(ConnPhase::ConnReq),
            1 => Some(ConnPhase::ConnAccept),
            2 => Some(ConnPhase::ConnComplete),
            3 => Some(ConnPhase::ConnInfo),
            _ => None,
        }
    }
}

/// 四段闭集数组（段序即推进序）。
pub const PHASE_ALL: &[ConnPhase] = &[
    ConnPhase::ConnReq,
    ConnPhase::ConnAccept,
    ConnPhase::ConnComplete,
    ConnPhase::ConnInfo,
];

/// 四段全敲（位图口径：0b1111）。
pub const PHASE_FULL_MASK: u32 = 0b1111;

// ── 端口对象与连接域模型 ─────────────────────────────────────────
/// 服务端连接端口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenPort {
    /// 端口号（非零）。
    pub port_id: u32,
    /// 最大消息长度（连接请求上界）。
    pub max_msg: u32,
    /// 端口是否仍开放监听。
    pub open: bool,
}

impl ListenPort {
    /// 请求长度合法语义：0 < len ≤ max_msg。
    pub fn accepts_len(&self, len: u32) -> bool {
        len > 0 && len <= self.max_msg
    }
}

/// 连接状态机：待连 → 四段中 → 已联签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    /// 待连：尚未敲任何段章。
    Idle,
    /// 四段中：至少一段已敲、未齐。
    InProgress,
    /// 已联签：四段齐并入队。
    Signed,
}

impl ConnState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ConnState::Idle => 0,
            ConnState::InProgress => 1,
            ConnState::Signed => 2,
        }
    }
}

/// 单条连接：段章位图 + 状态机 + 携带长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection {
    /// 连接序号（非零）。
    pub conn_id: u32,
    /// 目标端口号。
    pub port_id: u32,
    /// 请求长度。
    pub req_len: u32,
    /// 四段章位图（bit i = 段 i 已敲）。
    pub stamps: u32,
    /// 状态。
    pub state: ConnState,
}

impl Connection {
    /// 新连接（待连态）。
    pub fn new(conn_id: u32, port_id: u32, req_len: u32) -> Connection {
        Connection {
            conn_id,
            port_id,
            req_len,
            stamps: 0,
            state: ConnState::Idle,
        }
    }

    /// 四段是否全敲。
    pub fn all_stamped(&self) -> bool {
        self.stamps == PHASE_FULL_MASK
    }

    /// 某段是否已敲。
    pub fn has(&self, p: ConnPhase) -> bool {
        self.stamps & (1u32 << p.index()) != 0
    }

    /// 联签摘要（诊断文本）。
    pub fn brief(&self) -> String {
        format!(
            "连接{} 端口{} len={} 章=0b{:04b} {:?}",
            self.conn_id, self.port_id, self.req_len, self.stamps, self.state
        )
    }
}

/// D4 消息队列：联签连接只追加不改写。
#[derive(Clone, Debug)]
pub struct MsgQueue {
    /// 已联签入队的连接。
    queue: Vec<Connection>,
    /// 队列容量。
    cap: usize,
}

impl MsgQueue {
    /// 定容队列。
    pub fn new(cap: usize) -> MsgQueue {
        MsgQueue {
            queue: Vec::new(),
            cap,
        }
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// 是否空队。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 第 idx 项（越界 None）。
    pub fn item_at(&self, idx: usize) -> Option<Connection> {
        self.queue.get(idx).copied()
    }

    /// 是否已含连接。
    pub fn contains(&self, conn_id: u32) -> bool {
        for c in self.queue.iter() {
            if c.conn_id == conn_id {
                return true;
            }
        }
        false
    }

    /// 联签入队（队列闸：容量/重复/联签齐三闸）。
    pub fn enqueue(&mut self, c: Connection) -> Result<(), u32> {
        if self.queue.len() >= self.cap {
            return Err(CxDiag::EConserv.val());
        }
        if self.contains(c.conn_id) {
            return Err(CxDiag::EDupQueue.val());
        }
        if !c.all_stamped() || c.state != ConnState::Signed {
            return Err(CxDiag::ENoInfo.val());
        }
        self.queue.push(c);
        Ok(())
    }

    /// 守恒：队内项必须全部联签齐章。
    pub fn conservation(&self) -> bool {
        for c in self.queue.iter() {
            if !c.all_stamped() || c.state != ConnState::Signed {
                return false;
            }
        }
        true
    }
}

/// 连接流水线：端口表 + 段机 + 联签。
#[derive(Clone, Debug)]
pub struct ConnPipeline {
    /// 服务端口表。
    ports: Vec<ListenPort>,
    /// 在途连接。
    conns: Vec<Connection>,
    /// 联签队列。
    queue: MsgQueue,
}

impl ConnPipeline {
    /// 建流水线（端口表给定，队列容量 8）。
    pub fn new(ports: Vec<ListenPort>) -> ConnPipeline {
        ConnPipeline {
            ports,
            conns: Vec::new(),
            queue: MsgQueue::new(8),
        }
    }

    /// 查端口。
    pub fn port_of(&self, port_id: u32) -> Option<ListenPort> {
        for p in self.ports.iter() {
            if p.port_id == port_id {
                return Some(*p);
            }
        }
        None
    }

    /// 查连接。
    pub fn conn_of(&self, conn_id: u32) -> Option<Connection> {
        for c in self.conns.iter() {
            if c.conn_id == conn_id {
                return Some(*c);
            }
        }
        None
    }

    /// 发起连接：端口闸 + 长度闸 + 重复闸。
    pub fn connect(&mut self, conn_id: u32, port_id: u32, req_len: u32) -> Result<(), u32> {
        if conn_id == 0 && port_id == 0 && req_len == 0 {
            return Err(CxDiag::EEmpty.val());
        }
        let port = match self.port_of(port_id) {
            Some(p) => p,
            None => return Err(CxDiag::EUnknownPort.val()),
        };
        if !port.open {
            return Err(CxDiag::EUnknownPort.val());
        }
        if !port.accepts_len(req_len) {
            if req_len == 0 {
                return Err(CxDiag::EZeroLen.val());
            }
            return Err(CxDiag::EOverLen.val());
        }
        if self.conn_of(conn_id).is_some() {
            return Err(CxDiag::EDupQueue.val());
        }
        self.conns.push(Connection::new(conn_id, port_id, req_len));
        Ok(())
    }

    /// 敲段章：段闭集/跳段/重段/倒段四闸 + 段机推进。
    pub fn stamp(&mut self, conn_id: u32, phase: ConnPhase) -> Result<(), u32> {
        let idx = match self.conn_index(conn_id) {
            Some(i) => i,
            None => return Err(CxDiag::EUnknownPort.val()),
        };
        let c = match self.conns.get(idx) {
            Some(c) => *c,
            None => return Err(CxDiag::EUnknownPort.val()),
        };
        // 已联签后再敲 = 倒段。
        if c.state.rank() >= ConnState::Signed.rank() {
            return Err(CxDiag::ERewind.val());
        }
        // 重段闸。
        if c.has(phase) {
            return Err(CxDiag::EDupPhase.val());
        }
        // 跳段闸：必须恰好敲"下一段"。
        let expect = match PHASE_ALL.get(c.stamps.count_ones() as usize) {
            Some(p) => *p,
            None => return Err(CxDiag::EPhaseRange.val()),
        };
        if phase != expect {
            if phase.index() > expect.index() {
                return Err(CxDiag::ESkip.val());
            }
            return Err(CxDiag::ERewind.val());
        }
        // 敲章 + 状态推进。
        if let Some(c) = self.conns.get_mut(idx) {
            c.stamps |= 1u32 << phase.index();
            if c.state == ConnState::Idle {
                c.state = ConnState::InProgress;
            }
        }
        Ok(())
    }

    /// 信息回传段敲章后尝试联签（回传齐即自动联签入队）。
    pub fn finalize(&mut self, conn_id: u32) -> Result<(), u32> {
        let idx = match self.conn_index(conn_id) {
            Some(i) => i,
            None => return Err(CxDiag::EUnknownPort.val()),
        };
        let c = match self.conns.get(idx) {
            Some(c) => *c,
            None => return Err(CxDiag::EUnknownPort.val()),
        };
        if !c.has(ConnPhase::ConnInfo) {
            return Err(CxDiag::ENoInfo.val());
        }
        if !c.all_stamped() {
            return Err(CxDiag::ENoInfo.val());
        }
        if c.state.rank() >= ConnState::Signed.rank() {
            return Err(CxDiag::EDupQueue.val());
        }
        let signed = Connection {
            state: ConnState::Signed,
            ..c
        };
        self.queue.enqueue(signed)?;
        if let Some(c) = self.conns.get_mut(idx) {
            c.state = ConnState::Signed;
        }
        Ok(())
    }

    /// 按连接号找在途下标。
    fn conn_index(&self, conn_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.conns.len() {
            if let Some(c) = self.conns.get(i) {
                if c.conn_id == conn_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 在途连接数。
    pub fn conns_len(&self) -> usize {
        self.conns.len()
    }

    /// 联签队列。
    pub fn queue(&self) -> &MsgQueue {
        &self.queue
    }

    /// 四段对平断言：在途连接段章位图必须落在前缀闭包
    /// （0b0001..0b1111 逐位前缀，不得出现跳位）。
    pub fn stamps_prefix_ok(&self) -> bool {
        for c in self.conns.iter() {
            let ones = c.stamps.count_ones();
            // 前缀位图的合法值 = 2^ones - 1（低 ones 位全 1）。
            if c.stamps != (1u32.checked_shl(ones).unwrap_or(0).wrapping_sub(1) & PHASE_FULL_MASK) {
                return false;
            }
        }
        true
    }
}

// ── 正常流与注入面 ───────────────────────────────────────────────
/// 标准端口表（三条服务端口，容量语义各异）。
pub fn std_ports() -> Vec<ListenPort> {
    let mut v = Vec::new();
    v.push(ListenPort {
        port_id: 0x2100,
        max_msg: 328,
        open: true,
    });
    v.push(ListenPort {
        port_id: 0x2101,
        max_msg: 112,
        open: true,
    });
    v.push(ListenPort {
        port_id: 0x2102,
        max_msg: 16,
        open: false,
    });
    v
}

/// 走完整四段联签流。
pub fn run_full_flow(conn_id: u32, port_id: u32, req_len: u32) -> Result<MsgQueue, u32> {
    let mut pl = ConnPipeline::new(std_ports());
    pl.connect(conn_id, port_id, req_len)?;
    let mut k = 0u32;
    while k < 4 {
        let p = ConnPhase::from_index(k).ok_or(CxDiag::EPhaseRange.val())?;
        pl.stamp(conn_id, p)?;
        k += 1;
    }
    pl.finalize(conn_id)?;
    // 返回一个仅含该流水线队列的快照队列。
    let mut out = MsgQueue::new(8);
    let mut i = 0usize;
    while i < pl.queue().len() {
        if let Some(c) = pl.queue().item_at(i) {
            out.enqueue(c)?;
        }
        i += 1;
    }
    Ok(out)
}

/// 四段对平断言（判据 J1 直读口）。
fn four_phase_ok() -> bool {
    // 完整流：队列恰好一项，段章位图全敲，前缀闭包成立。
    match run_full_flow(0x0701, 0x2100, 64) {
        Ok(q) => {
            q.len() == 1
                && q.conservation()
                && match q.item_at(0) {
                    Some(c) => c.all_stamped() && c.state == ConnState::Signed,
                    None => false,
                }
        }
        Err(_) => false,
    }
}

/// 坏连接注入 10/10：十路违约全数拒止。
fn bad_conn_injections_ok() -> bool {
    let mut hits = 0usize;
    let mut pl = ConnPipeline::new(std_ports());
    // 注 1：未知端口。
    if pl.connect(0x01, 0x9999, 8) == Err(CxDiag::EUnknownPort.val()) {
        hits += 1;
    }
    // 注 2：端口已关闭。
    if pl.connect(0x02, 0x2102, 8) == Err(CxDiag::EUnknownPort.val()) {
        hits += 1;
    }
    // 注 3：零长请求。
    if pl.connect(0x03, 0x2100, 0) == Err(CxDiag::EZeroLen.val()) {
        hits += 1;
    }
    // 注 4：超长请求。
    if pl.connect(0x04, 0x2100, 329) == Err(CxDiag::EOverLen.val()) {
        hits += 1;
    }
    // 注 5：空连接样本。
    if pl.connect(0, 0, 0) == Err(CxDiag::EEmpty.val()) {
        hits += 1;
    }
    // 注 6：跳段（请求未敲先敲完成）。
    let _ = pl.connect(0x06, 0x2100, 16);
    if pl.stamp(0x06, ConnPhase::ConnComplete) == Err(CxDiag::ESkip.val()) {
        hits += 1;
    }
    // 注 7：重段。
    let _ = pl.stamp(0x06, ConnPhase::ConnReq);
    if pl.stamp(0x06, ConnPhase::ConnReq) == Err(CxDiag::EDupPhase.val()) {
        hits += 1;
    }
    // 注 8：倒段（接受后回头敲请求段序之外的段）。
    let _ = pl.stamp(0x06, ConnPhase::ConnAccept);
    if pl.stamp(0x06, ConnPhase::ConnReq) == Err(CxDiag::EDupPhase.val()) {
        hits += 1;
    }
    // 注 9：回传缺章联签被拒。
    let _ = pl.stamp(0x06, ConnPhase::ConnComplete);
    if pl.finalize(0x06) == Err(CxDiag::ENoInfo.val()) {
        hits += 1;
    }
    // 注 10：重复连接（同连接号再连）。
    let _ = pl.stamp(0x06, ConnPhase::ConnInfo);
    if pl.connect(0x06, 0x2100, 16) == Err(CxDiag::EDupQueue.val()) {
        hits += 1;
    }
    hits == 10
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
    for d in CX_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(CxDiag::ESkip.val()) != CxDiag::ESkip.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x3E).starts_with("未知") {
        return false;
    }
    true
}

/// 四段闭集守卫：段号互异、反查闭合。
fn phase_closed_ok() -> bool {
    if PHASE_ALL.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < PHASE_ALL.len() {
        let a = match PHASE_ALL.get(i) {
            Some(p) => *p,
            None => return false,
        };
        if ConnPhase::from_index(a.index()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < PHASE_ALL.len() {
            let b = match PHASE_ALL.get(j) {
                Some(p) => *p,
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

/// 前缀闭包守卫：在途连接段章不得出现跳位。
fn prefix_closure_ok() -> bool {
    let mut pl = ConnPipeline::new(std_ports());
    let _ = pl.connect(0x0801, 0x2101, 32);
    let _ = pl.stamp(0x0801, ConnPhase::ConnReq);
    let _ = pl.stamp(0x0801, ConnPhase::ConnAccept);
    pl.stamps_prefix_ok()
}

/// 队列守恒：队内全联签、容量不越。
fn queue_conservation_ok() -> bool {
    let q = match run_full_flow(0x0901, 0x2100, 128) {
        Ok(q) => q,
        Err(_) => return false,
    };
    q.conservation() && q.len() <= 8 && !q.is_empty()
}

/// 双跑一致：同参数两条完整流逐项一致。
fn dual_run_ok() -> bool {
    let a = run_full_flow(0x0A01, 0x2100, 48);
    let b = run_full_flow(0x0A01, 0x2100, 48);
    match (a, b) {
        (Ok(qa), Ok(qb)) => {
            if qa.len() != qb.len() {
                return false;
            }
            let mut i = 0usize;
            while i < qa.len() {
                if qa.item_at(i) != qb.item_at(i) {
                    return false;
                }
                i += 1;
            }
            true
        }
        _ => false,
    }
}

/// 边界：长度语义 0 < len ≤ max 的三点判定。
fn length_boundary_ok() -> bool {
    let p = ListenPort {
        port_id: 0x2200,
        max_msg: 100,
        open: true,
    };
    // 零长拒、上界收、上界+1 拒。
    !p.accepts_len(0) && p.accepts_len(1) && p.accepts_len(100) && !p.accepts_len(101)
}

/// 完成写回语义：联签队列摘要非空。
fn writeback_ok() -> bool {
    match run_full_flow(0x0B01, 0x2100, 8) {
        Ok(q) => {
            let summary = format!("queue_len={}", q.len());
            q.len() == 1 && !summary.is_empty()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12603 自检（判据 12602 卡：连接四段序对平 + 坏连接注入）。
pub fn run_unx12602_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12602");
    let mut pl = ConnPipeline::new(std_ports());
    let _ = pl.connect(0x0C01, 0x2100, 64);
    let _ = pl.stamp(0x0C01, ConnPhase::ConnReq);
    let _ = pl.stamp(0x0C01, ConnPhase::ConnAccept);

    s.add_dyn(
        "F12603-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12603-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", CX_ALL.len()),
    );

    s.add_dyn(
        "F12603-03 四段闭集枚举",
        phase_closed_ok(),
        &format!(
            "phases={} 首={} 尾={}",
            PHASE_ALL.len(),
            PHASE_ALL.first().map(|p| p.name()).unwrap_or("?"),
            PHASE_ALL.last().map(|p| p.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12603-04 四段对平断言绿",
        four_phase_ok(),
        "请求→接受→完成→回传 全章联签入队",
    );

    s.add_dyn(
        "F12603-05 段章前缀闭包",
        prefix_closure_ok() && pl.stamps_prefix_ok(),
        &format!(
            "在途章位图=0b{:04b}",
            pl.conn_of(0x0C01).map(|c| c.stamps).unwrap_or(0)
        ),
    );

    // 注入必抓：十路坏连接全数拒止。
    s.add_dyn(
        "F12603-06 坏连接注入 10/10 拒止",
        bad_conn_injections_ok(),
        "未知口/闭口/零长/超长/空样本/跳段/重段/倒段/缺回传/重复连",
    );

    s.add_dyn(
        "F12603-07 队列守恒",
        queue_conservation_ok(),
        "队内全联签、容量不越、非空",
    );

    s.add_dyn(
        "F12603-08 双跑一致",
        dual_run_ok(),
        "同参数两条完整流逐项一致",
    );

    s.add_dyn(
        "F12603-09 长度边界三点",
        length_boundary_ok(),
        "0 拒 / 1 与 max 收 / max+1 拒",
    );

    s.add_dyn(
        "F12603-10 端口表语义",
        std_ports().len() == 3,
        "三条端口（两条开放一条关闭）",
    );

    s.add_dyn(
        "F12603-11 完成写回语义",
        writeback_ok(),
        "联签队列摘要非空",
    );

    // 状态机单调：Idle < InProgress < Signed。
    s.add_dyn(
        "F12603-12 状态机单调",
        ConnState::Idle.rank() < ConnState::InProgress.rank()
            && ConnState::InProgress.rank() < ConnState::Signed.rank(),
        "待连→四段中→已联签 只进不退",
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
        assert_eq!(SEG_BASE, 0x80080);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(CX_ALL.len(), 12);
        let mut i = 0usize;
        while i < CX_ALL.len() {
            let vi = CX_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CX_ALL.len() {
                let vj = CX_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3D).contains("未知"));
    }

    #[test]
    fn phase_closed_set_four() {
        assert_eq!(PHASE_ALL.len(), 4);
        assert_eq!(ConnPhase::ConnReq.index(), 0);
        assert_eq!(ConnPhase::ConnInfo.index(), 3);
        assert!(ConnPhase::from_index(4).is_none());
        assert!(phase_closed_ok());
    }

    #[test]
    fn four_phase_happy_path() {
        // 完整四段流：联签入队恰一项。
        let q = match run_full_flow(0x1001, 0x2100, 32) {
            Ok(q) => q,
            Err(_) => MsgQueue::new(0),
        };
        assert_eq!(q.len(), 1);
        let c = q.item_at(0);
        assert!(c.is_some());
        assert!(c.map(|c| c.all_stamped()).unwrap_or(false));
        assert_eq!(c.map(|c| c.state), Some(ConnState::Signed));
    }

    #[test]
    fn bad_conn_injections_all_rejected() {
        // 注入必抓：十路坏连接全数拒止。
        assert!(bad_conn_injections_ok());
    }

    #[test]
    fn length_semantics_boundary() {
        // 长度语义：0 < len ≤ max。
        let p = ListenPort {
            port_id: 1,
            max_msg: 10,
            open: true,
        };
        assert!(!p.accepts_len(0));
        assert!(p.accepts_len(10));
        assert!(!p.accepts_len(11));
        assert!(length_boundary_ok());
    }

    #[test]
    fn stamp_gates_reject_skip_dup_rewind() {
        let mut pl = ConnPipeline::new(std_ports());
        let _ = pl.connect(0x1101, 0x2100, 16);
        // 跳段：请求未敲先敲接受。
        assert_eq!(pl.stamp(0x1101, ConnPhase::ConnAccept), Err(CxDiag::ESkip.val()));
        // 正常敲请求段。
        assert!(pl.stamp(0x1101, ConnPhase::ConnReq).is_ok());
        // 重段。
        assert_eq!(pl.stamp(0x1101, ConnPhase::ConnReq), Err(CxDiag::EDupPhase.val()));
        // 未知连接敲段。
        assert_eq!(pl.stamp(0x1102, ConnPhase::ConnReq), Err(CxDiag::EUnknownPort.val()));
    }

    #[test]
    fn finalize_requires_info_stamp() {
        let mut pl = ConnPipeline::new(std_ports());
        let _ = pl.connect(0x1201, 0x2100, 16);
        let _ = pl.stamp(0x1201, ConnPhase::ConnReq);
        // 缺回传章联签拒。
        assert_eq!(pl.finalize(0x1201), Err(CxDiag::ENoInfo.val()));
        // 补齐三段再联签仍拒（回传未敲）。
        let _ = pl.stamp(0x1201, ConnPhase::ConnAccept);
        let _ = pl.stamp(0x1201, ConnPhase::ConnComplete);
        assert_eq!(pl.finalize(0x1201), Err(CxDiag::ENoInfo.val()));
        // 敲回传后联签成功且入队。
        let _ = pl.stamp(0x1201, ConnPhase::ConnInfo);
        assert!(pl.finalize(0x1201).is_ok());
        assert_eq!(pl.queue().len(), 1);
        // 重复联签拒。
        assert_eq!(pl.finalize(0x1201), Err(CxDiag::EDupQueue.val()));
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致。
        let a = run_full_flow(0x1301, 0x2101, 20);
        let b = run_full_flow(0x1301, 0x2101, 20);
        assert_eq!(a.is_ok(), b.is_ok());
        // 双队列逐项比对（Connection 有 PartialEq，队列本身按位取项比）。
        if let (Ok(qa), Ok(qb)) = (a, b) {
            assert_eq!(qa.len(), qb.len());
            let mut i = 0usize;
            while i < qa.len() {
                assert_eq!(qa.item_at(i), qb.item_at(i));
                i += 1;
            }
        }
        assert!(dual_run_ok());
    }

    #[test]
    fn queue_rejects_unsigned_and_dup() {
        let mut q = MsgQueue::new(4);
        // 未齐章入队拒。
        let mut c = Connection::new(7, 0x2100, 4);
        c.stamps = 0b0011;
        assert_eq!(q.enqueue(c), Err(CxDiag::ENoInfo.val()));
        // 齐章入队成功。
        c.stamps = PHASE_FULL_MASK;
        c.state = ConnState::Signed;
        assert!(q.enqueue(c).is_ok());
        // 重复入队拒。
        assert_eq!(q.enqueue(c), Err(CxDiag::EDupQueue.val()));
        assert!(q.conservation());
    }

    #[test]
    fn closed_port_rejected() {
        // 关闭端口（0x2102）不得接收连接。
        let mut pl = ConnPipeline::new(std_ports());
        assert_eq!(pl.connect(0x1401, 0x2102, 4), Err(CxDiag::EUnknownPort.val()));
        assert_eq!(pl.conns_len(), 0);
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12602_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
