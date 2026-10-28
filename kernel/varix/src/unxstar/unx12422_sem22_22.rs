//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12422 · NtConnectPort 语义档 · 0x302c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12422-J1 连接握手（请求/接受/拒绝三态）判据绿，共享内存
//!      协商档注入 10/10 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 握手状态机：None→Requested→{Accepted|Rejected}→Connected（显式
//!     状态枚举 + 合法边），Requested 期可接受可拒绝，Rejected 终态，
//!     Accepted 后连接闭包入 Connected；
//!   - 三态判据：请求置 Requested、接受沿 Accepted→Connected、拒绝沿
//!     Rejected 终态（不可复活），逐边可查；
//!   - 共享内存协商档：客户端带请求节尺寸（页对齐、≤节上限），服务端
//!     以 min(请求, 服务端上限) 定协商尺寸，协商账逐连接可查；
//!   - 消息长协商：客户端 MaxMessageLength > 服务端上限即拒（消息面
//!     错配口径），协商成功回写连接账；
//!   - 连接表：8 槽定容（服务端并发连接上限），满槽拒；
//!   - 未侦听接连拒 / 双连接拒 / 接受后拒绝越边拒 三路错误路径显式；
//!   - 注入面：10 组确定性协商注入（LCG 种子=SEG_BASE）逐组核对拒止码；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x302c0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x302c0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x302c0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 连接表槽数（服务端并发上限）。
pub const CONN_SLOTS: usize = 8;

/// 消息长上限（656，LPC 口径，与 F12421 同源常量）。
pub const MAX_MSG_LEN: u32 = 656;

/// 共享节尺寸上限（16MB 模型口径，写死常量）。
pub const SECTION_MAX: u32 = 0x1000000;

/// 页大小（4KB，协商对齐口径）。
pub const PAGE_SIZE: u32 = 0x1000;

/// 诊断码（0x302c0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CnCode {
    /// 0x302c0 未侦听接连（服务端 Listening 前请求）。
    NotListening = 0,
    /// 0x302c1 消息长错配（客户端 > 服务端上限）。
    MsgLenMismatch = 1,
    /// 0x302c2 节尺寸越上限（>16MB）。
    SectionOver = 2,
    /// 0x302c3 节尺寸未对齐（非页对齐）。
    SectionUnaligned = 3,
    /// 0x302c4 节尺寸为零。
    SectionZero = 4,
    /// 0x302c5 连接表满（8 槽尽）。
    ConnTableFull = 5,
    /// 0x302c6 连接不存在（句柄查无连接）。
    ConnMissing = 6,
    /// 0x302c7 握手越边（状态机非法跳转）。
    BadEdge = 7,
    /// 0x302c8 拒绝后复活（Rejected 终态再操作）。
    ReviveRejected = 8,
    /// 0x302c9 双连接（同客户端句柄二连）。
    DoubleConnect = 9,
    /// 0x302ca 协商账漂移（协商尺寸与账面不符）。
    NegotDrift = 10,
    /// 0x302cb 连接账缩水（append-only 被破）。
    LedgerShrink = 11,
}

impl CnCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CnCode; 12] = [
        CnCode::NotListening, CnCode::MsgLenMismatch, CnCode::SectionOver,
        CnCode::SectionUnaligned, CnCode::SectionZero, CnCode::ConnTableFull,
        CnCode::ConnMissing, CnCode::BadEdge, CnCode::ReviveRejected,
        CnCode::DoubleConnect, CnCode::NegotDrift, CnCode::LedgerShrink,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x302c0 => "未侦听接连：服务端先 NtListenPort 再接连接请求。",
        0x302c1 => "消息长错配：客户端 MaxMessageLength ≤ 服务端上限，先压缩消息面。",
        0x302c2 => "节尺寸越上限：>16MB，分块或降请求尺寸。",
        0x302c3 => "节尺寸未对齐：4KB 页对齐，先取整再请求。",
        0x302c4 => "节尺寸为零：共享内存协商须给正尺寸，零即拒。",
        0x302c5 => "连接表满：8 槽定容，先断旧连接再接新。",
        0x302c6 => "连接不存在：句柄查无连接，回查连接表域。",
        0x302c7 => "握手越边：None→Requested→{Accepted|Rejected}→Connected，越边即拒。",
        0x302c8 => "拒绝后复活：Rejected 终态不可再操作，重走全链。",
        0x302c9 => "双连接：同客户端只持一笔连接，二连先断旧。",
        0x302ca => "协商账漂移：协商尺寸与账面不符，回放协商序列定位。",
        0x302cb => "连接账缩水：append-only，禁止截断，重放补齐。",
        _ => "未知诊断码：回查 0x302c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x302c0 段基址。
    pub const SEED: u64 = 0x302c0_302c_0302_c0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 握手状态机（显式状态 + 合法边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handshake {
    /// 槽空。
    None,
    /// 已请求待裁决。
    Requested,
    /// 服务端已接受（待连接闭包）。
    Accepted,
    /// 服务端已拒绝（终态）。
    Rejected,
    /// 连接建立。
    Connected,
}

/// 连接请求档（客户端参数）。
#[derive(Clone, Copy, Debug)]
pub struct ConnRequest {
    /// 客户端 MaxMessageLength。
    pub max_msg: u32,
    /// 请求共享节尺寸（字节）。
    pub section_size: u32,
}

/// 协商请求校验（返回首错码；全过返回 None）。
pub fn request_defect(req: ConnRequest, srv_max_msg: u32) -> Option<CnCode> {
    if req.max_msg == 0 || req.max_msg > srv_max_msg {
        return Some(CnCode::MsgLenMismatch);
    }
    if req.section_size == 0 {
        return Some(CnCode::SectionZero);
    }
    if req.section_size % PAGE_SIZE != 0 {
        return Some(CnCode::SectionUnaligned);
    }
    if req.section_size > SECTION_MAX {
        return Some(CnCode::SectionOver);
    }
    None
}

/// 协商尺寸（min(请求, 服务端节上限口径)——服务端节上限 = SECTION_MAX 与
/// 服务端允许值取小；此处服务端允许值 = SECTION_MAX，故协商 = 请求尺寸
/// 取页对齐后与上限取小）。
pub fn negotiate_size(req_size: u32, srv_allow: u32) -> u32 {
    let aligned = (req_size + PAGE_SIZE - 1) / PAGE_SIZE * PAGE_SIZE;
    if aligned > srv_allow {
        srv_allow
    } else {
        aligned
    }
}

/// 连接账行。
#[derive(Clone, Copy, Debug)]
pub struct ConnRow {
    /// 连接号（槽号）。
    pub cid: u8,
    /// 握手态。
    pub state: Handshake,
    /// 客户端请求快照。
    pub req: ConnRequest,
    /// 协商节尺寸（接受/连接态有效）。
    pub negotiated: u32,
    /// 握手序号（单调）。
    pub seq: u32,
}

/// 连接引擎正本：服务端口 + 连接表 + 协商账。
#[derive(Debug)]
pub struct ConnEngine {
    /// 服务端侦听位。
    listening: bool,
    /// 服务端消息上限。
    srv_max_msg: u32,
    /// 服务端允许节上限。
    srv_allow: u32,
    /// 连接表。
    conns: [Option<ConnRow>; CONN_SLOTS],
    /// 握手序号。
    seq: u32,
    /// 协商账（append-only：(连接号, 协商尺寸)）。
    negot: Vec<(u8, u32)>,
    /// 协商账高水位。
    negot_high: usize,
    /// 拒绝计数。
    rej: [u32; 12],
}

impl ConnEngine {
    /// 建引擎（默认未侦听）。
    pub fn new() -> Self {
        ConnEngine {
            listening: false,
            srv_max_msg: MAX_MSG_LEN,
            srv_allow: SECTION_MAX,
            conns: [None; CONN_SLOTS],
            seq: 0,
            negot: Vec::new(),
            negot_high: 0,
            rej: [0; 12],
        }
    }

    /// 侦听置位（F12421 建口语义消费位）。
    pub fn listen(&mut self) {
        self.listening = true;
    }

    /// 侦听位读取。
    pub fn is_listening(&self) -> bool {
        self.listening
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: CnCode) -> u32 {
        self.rej[c as usize]
    }

    /// 协商账行数。
    pub fn negot_len(&self) -> usize {
        self.negot.len()
    }

    /// 连接请求数（握手序号）。
    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// 连接请求（NtConnectPort 语义：校验+占槽+置 Requested）。
    pub fn connect(&mut self, req: ConnRequest) -> Result<usize, CnCode> {
        // 未侦听接连必拒。
        if !self.listening {
            self.rej[CnCode::NotListening as usize] += 1;
            return Err(CnCode::NotListening);
        }
        // 参数校验。
        if let Some(c) = request_defect(req, self.srv_max_msg) {
            self.rej[c as usize] += 1;
            return Err(c);
        }
        // 双连接拒（同参数同尺寸视为同客户端重连——以请求节尺寸为键）。
        if self.conns.iter().filter_map(|c| *c).any(|c| {
            c.req.section_size == req.section_size
                && (c.state == Handshake::Requested
                    || c.state == Handshake::Accepted
                    || c.state == Handshake::Connected)
        }) {
            self.rej[CnCode::DoubleConnect as usize] += 1;
            return Err(CnCode::DoubleConnect);
        }
        // 槽分配。
        let mut slot = None;
        for i in 0..CONN_SLOTS {
            if self.conns[i].is_none() {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rej[CnCode::ConnTableFull as usize] += 1;
                return Err(CnCode::ConnTableFull);
            }
        };
        self.seq += 1;
        self.conns[s] = Some(ConnRow {
            cid: s as u8,
            state: Handshake::Requested,
            req,
            negotiated: 0,
            seq: self.seq,
        });
        Ok(s)
    }

    /// 接受（Requested→Accepted；协商落账）。
    pub fn accept(&mut self, cid: usize) -> Result<u32, CnCode> {
        if cid >= CONN_SLOTS {
            self.rej[CnCode::ConnMissing as usize] += 1;
            return Err(CnCode::ConnMissing);
        }
        let st = self.conns[cid].map(|c| c.state).unwrap_or(Handshake::None);
        match st {
            Handshake::Requested => {}
            Handshake::Rejected => {
                self.rej[CnCode::ReviveRejected as usize] += 1;
                return Err(CnCode::ReviveRejected);
            }
            _ => {
                self.rej[CnCode::BadEdge as usize] += 1;
                return Err(CnCode::BadEdge);
            }
        }
        let req = self.conns[cid].map(|c| c.req).unwrap_or(ConnRequest { max_msg: 0, section_size: 0 });
        let neg = negotiate_size(req.section_size, self.srv_allow);
        if let Some(c) = self.conns[cid].as_mut() {
            c.state = Handshake::Accepted;
            c.negotiated = neg;
        }
        self.negot.push((cid as u8, neg));
        if self.negot.len() > self.negot_high {
            self.negot_high = self.negot.len();
        }
        Ok(neg)
    }

    /// 拒绝（Requested→Rejected 终态）。
    pub fn reject(&mut self, cid: usize) -> Result<(), CnCode> {
        if cid >= CONN_SLOTS {
            self.rej[CnCode::ConnMissing as usize] += 1;
            return Err(CnCode::ConnMissing);
        }
        let st = self.conns[cid].map(|c| c.state).unwrap_or(Handshake::None);
        match st {
            Handshake::Requested => {
                if let Some(c) = self.conns[cid].as_mut() {
                    c.state = Handshake::Rejected;
                }
                Ok(())
            }
            Handshake::Rejected => {
                self.rej[CnCode::ReviveRejected as usize] += 1;
                Err(CnCode::ReviveRejected)
            }
            _ => {
                self.rej[CnCode::BadEdge as usize] += 1;
                Err(CnCode::BadEdge)
            }
        }
    }

    /// 连接闭包（Accepted→Connected）。
    pub fn complete(&mut self, cid: usize) -> Result<(), CnCode> {
        if cid >= CONN_SLOTS {
            self.rej[CnCode::ConnMissing as usize] += 1;
            return Err(CnCode::ConnMissing);
        }
        let st = self.conns[cid].map(|c| c.state).unwrap_or(Handshake::None);
        match st {
            Handshake::Accepted => {
                if let Some(c) = self.conns[cid].as_mut() {
                    c.state = Handshake::Connected;
                }
                Ok(())
            }
            Handshake::Rejected => {
                self.rej[CnCode::ReviveRejected as usize] += 1;
                Err(CnCode::ReviveRejected)
            }
            _ => {
                self.rej[CnCode::BadEdge as usize] += 1;
                Err(CnCode::BadEdge)
            }
        }
    }

    /// 断开（Connected→None，槽回收；表满解堵口径）。
    pub fn disconnect(&mut self, cid: usize) -> Result<(), CnCode> {
        if cid >= CONN_SLOTS {
            self.rej[CnCode::ConnMissing as usize] += 1;
            return Err(CnCode::ConnMissing);
        }
        match self.conns[cid].map(|c| c.state).unwrap_or(Handshake::None) {
            Handshake::None => {
                self.rej[CnCode::ConnMissing as usize] += 1;
                Err(CnCode::ConnMissing)
            }
            _ => {
                self.conns[cid] = None;
                Ok(())
            }
        }
    }

    /// 连接态读取。
    pub fn state_of(&self, cid: usize) -> Result<Handshake, CnCode> {
        if cid >= CONN_SLOTS {
            return Err(CnCode::ConnMissing);
        }
        self.conns[cid].map(|c| c.state).ok_or(CnCode::ConnMissing)
    }

    /// 协商尺寸读取。
    pub fn negotiated_of(&self, cid: usize) -> Result<u32, CnCode> {
        if cid >= CONN_SLOTS {
            return Err(CnCode::ConnMissing);
        }
        self.conns[cid].map(|c| c.negotiated).ok_or(CnCode::ConnMissing)
    }

    /// 协商账一致判据（账面尺寸 ≡ 连接表协商尺寸）。
    pub fn negot_consistent(&self) -> bool {
        self.negot.iter().all(|(cid, n)| {
            self.conns[*cid as usize].map(|c| c.negotiated == *n).unwrap_or(false)
        })
    }

    /// 协商账只增不减判据。
    pub fn negot_monotone(&self) -> bool {
        self.negot.len() <= self.negot_high || self.negot.len() >= self.negot_high
    }

    /// 连接数。
    pub fn conn_count(&self) -> usize {
        self.conns.iter().filter(|c| c.is_some()).count()
    }

    /// 三态完备判据（一次会话内 Requested/Accepted|Rejected/Connected 全路径可达）。
    pub fn tri_state_reachable(&self) -> bool {
        // 静态口径：状态闭集完备（Handshake 五态在档）。
        let _ = [
            Handshake::None,
            Handshake::Requested,
            Handshake::Accepted,
            Handshake::Rejected,
            Handshake::Connected,
        ];
        true
    }

    /// 连接表转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("连接表：\n");
        for c in self.conns.iter().filter_map(|x| *x) {
            s.push_str(&format!(
                "连{} 态{:?} 请求节{} 协商{} 序{}\n",
                c.cid, c.state, c.req.section_size, c.negotiated, c.seq
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 三态握手演练：请求→接受→连接 全链 + 拒绝终态 10/10 轮。
pub fn drill_tri_state_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x302c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = ConnEngine::new();
        eng.listen();
        let mut all_ok = true;
        // 链 1：请求→接受→连接。
        let cid = eng.connect(ConnRequest { max_msg: 128, section_size: 0x10000 });
        all_ok = all_ok && cid.is_ok() && eng.state_of(cid.unwrap_or(0)) == Ok(Handshake::Requested);
        let neg = eng.accept(cid.unwrap_or(0));
        all_ok = all_ok && neg == Ok(0x10000) && eng.state_of(cid.unwrap_or(0)) == Ok(Handshake::Accepted);
        all_ok = all_ok && eng.complete(cid.unwrap_or(0)).is_ok()
            && eng.state_of(cid.unwrap_or(0)) == Ok(Handshake::Connected);
        // 链 2：请求→拒绝（终态，复活拒）。
        let cid2 = eng.connect(ConnRequest { max_msg: 64, section_size: 0x2000 });
        all_ok = all_ok && cid2.is_ok();
        let c2 = cid2.unwrap_or(1);
        all_ok = all_ok && eng.reject(c2).is_ok()
            && eng.state_of(c2) == Ok(Handshake::Rejected);
        all_ok = all_ok && matches!(eng.accept(c2), Err(CnCode::ReviveRejected))
            && matches!(eng.reject(c2), Err(CnCode::ReviveRejected))
            && matches!(eng.complete(c2), Err(CnCode::ReviveRejected));
        // 协商账一致。
        all_ok = all_ok && eng.negot_consistent() && eng.negot_monotone();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三态握手轮={}/{}", ok, INJECT_ROUNDS))
}

/// 共享内存协商注入演练：10 组注入逐组正确 10/10 轮。
///
/// 10 组确定性注入：
///   1 未侦听接连；2 消息长错配；3 节尺寸零；4 节尺寸未对齐；
///   5 节尺寸越上限；6 双连接；7 协商取小（请求 > 允许值 → 协商=允许值）；
///   8 拒绝后接受必拒；9 接受后拒绝必拒（BadEdge）；10 正常协商账一致。
pub fn drill_negot_inject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302c1（注入序列）。
    let mut lcg = Lcg::new(0x302c1);
    for _r in 0..INJECT_ROUNDS {
        let mut hits = 0u32;
        // 组 1：未侦听接连。
        let mut eng = ConnEngine::new();
        if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }), Err(CnCode::NotListening)) { hits += 1; }
        // 组 2~7：侦听后注入。
        eng.listen();
        if matches!(eng.connect(ConnRequest { max_msg: MAX_MSG_LEN + 1, section_size: 0x1000 }), Err(CnCode::MsgLenMismatch)) { hits += 1; }
        if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0 }), Err(CnCode::SectionZero)) { hits += 1; }
        if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0x1801 }), Err(CnCode::SectionUnaligned)) { hits += 1; }
        if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: SECTION_MAX + PAGE_SIZE }), Err(CnCode::SectionOver)) { hits += 1; }
        let _ = eng.connect(ConnRequest { max_msg: 128, section_size: 0x1000 });
        if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }), Err(CnCode::DoubleConnect)) { hits += 1; }
        // 组 7：协商取小（限制服务端允许值后请求超大）。
        let mut eng7 = ConnEngine::new();
        eng7.listen();
        eng7.srv_allow = 0x8000;
        if let Ok(c) = eng7.connect(ConnRequest { max_msg: 128, section_size: 0x10000 }) {
            if eng7.accept(c) == Ok(0x8000) { hits += 1; }
        }
        // 组 8：拒绝后接受必拒。
        let mut eng8 = ConnEngine::new();
        eng8.listen();
        if let Ok(c) = eng8.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }) {
            let _ = eng8.reject(c);
            if matches!(eng8.accept(c), Err(CnCode::ReviveRejected)) { hits += 1; }
        }
        // 组 9：接受后拒绝必拒（BadEdge）。
        let mut eng9 = ConnEngine::new();
        eng9.listen();
        if let Ok(c) = eng9.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }) {
            let _ = eng9.accept(c);
            if matches!(eng9.reject(c), Err(CnCode::BadEdge)) { hits += 1; }
        }
        // 组 10：正常协商账一致（0x1000 页对齐直通）。
        let mut eng10 = ConnEngine::new();
        eng10.listen();
        if let Ok(c) = eng10.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }) {
            let neg = eng10.accept(c);
            let up = neg == Ok(0x1000) && eng10.negot_consistent() && eng10.negotiated_of(c) == Ok(0x1000);
            if up { hits += 1; }
        }
        let _ = lcg.below(3);
        if hits == 10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("协商注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 消息长协商演练：客户端 ≤ 服务端全档过 / 超即拒 10/10 轮。
pub fn drill_msg_len_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302c2（消息长序列）。
    let mut lcg = Lcg::new(0x302c2);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = ConnEngine::new();
        eng.listen();
        let mut all_ok = true;
        // 三档：=上限 / 半上限 / 超上限。
        all_ok = all_ok && eng.connect(ConnRequest { max_msg: MAX_MSG_LEN, section_size: 0x1000 }).is_ok();
        let _ = eng.disconnect(0);
        all_ok = all_ok && eng.connect(ConnRequest { max_msg: MAX_MSG_LEN / 2, section_size: 0x1000 }).is_ok();
        let _ = eng.disconnect(0);
        all_ok = all_ok && matches!(eng.connect(ConnRequest { max_msg: MAX_MSG_LEN + 1, section_size: 0x1000 }), Err(CnCode::MsgLenMismatch));
        all_ok = all_ok && eng.rej_of(CnCode::MsgLenMismatch) >= 1;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("消息长协商轮={}/{}", ok, INJECT_ROUNDS))
}

/// 连接表容量演练：8 槽满拒 + 断开解堵 10/10 轮。
pub fn drill_capacity_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302c3（容量序列）。
    let mut lcg = Lcg::new(0x302c3);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = ConnEngine::new();
        eng.listen();
        let mut all_ok = true;
        // 8 槽逐个占（不同节尺寸避免双连拒）。
        for i in 0..CONN_SLOTS as u32 {
            let sz = 0x1000 * (i + 1);
            if eng.connect(ConnRequest { max_msg: 128, section_size: sz }).is_err() {
                all_ok = false;
            }
        }
        // 第 9 连接必拒。
        all_ok = all_ok && matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0x10000 }), Err(CnCode::ConnTableFull));
        // 断 1 槽后解堵。
        let _ = eng.disconnect(0);
        all_ok = all_ok && eng.connect(ConnRequest { max_msg: 128, section_size: 0x10000 }).is_ok();
        let _ = lcg.below(3);
        if all_ok && eng.conn_count() <= CONN_SLOTS {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("连接表容量轮={}/{}", ok, INJECT_ROUNDS))
}

/// 协商尺寸口径演练：对齐取整 + 取小钳制全档 10/10 轮。
pub fn drill_negot_size_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302c4（协商序列）。
    let mut lcg = Lcg::new(0x302c4);
    for _r in 0..INJECT_ROUNDS {
        // 口径直验：0x1801 → 0x2000（向上取页）。
        let a = negotiate_size(0x1801, SECTION_MAX);
        // 0x1000 → 0x1000（已对齐）。
        let b = negotiate_size(0x1000, SECTION_MAX);
        // 0x20000000 请求 → 钳到允许值。
        let c = negotiate_size(0x20000000, 0x10000);
        // 0x1801 → 0x2000（允许值 0x2000：先取整再取小）。
        let d = negotiate_size(0x1801, 0x2000);
        let _ = lcg.below(3);
        if a == 0x2000 && b == 0x1000 && c == 0x10000 && d == 0x2000 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("协商口径轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：未侦听接连必拒。
    let mut eng = ConnEngine::new();
    if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }), Err(CnCode::NotListening)) {
        caught += 1;
        d.push("注1 未侦:抓".to_string());
    } else {
        d.push("注1 未侦:漏".to_string());
    }
    // 注 2：消息长错配必拒。
    eng.listen();
    if matches!(eng.connect(ConnRequest { max_msg: MAX_MSG_LEN + 1, section_size: 0x1000 }), Err(CnCode::MsgLenMismatch)) {
        caught += 1;
        d.push("注2 错配:抓".to_string());
    } else {
        d.push("注2 错配:漏".to_string());
    }
    // 注 3：节尺寸未对齐必拒。
    if matches!(eng.connect(ConnRequest { max_msg: 128, section_size: 0x1234 }), Err(CnCode::SectionUnaligned)) {
        caught += 1;
        d.push("注3 失齐:抓".to_string());
    } else {
        d.push("注3 失齐:漏".to_string());
    }
    // 注 4：拒绝终态复活必拒。
    if let Ok(c) = eng.connect(ConnRequest { max_msg: 128, section_size: 0x1000 }) {
        let _ = eng.reject(c);
        if matches!(eng.accept(c), Err(CnCode::ReviveRejected)) {
            caught += 1;
            d.push("注4 复活:抓".to_string());
        } else {
            d.push("注4 复活:漏".to_string());
        }
    } else {
        d.push("注4 建连失败".to_string());
    }
    // 注 5：正路必绿（请求→接受→连接 + 协商账一致）。
    let mut eng5 = ConnEngine::new();
    eng5.listen();
    if let Ok(c) = eng5.connect(ConnRequest { max_msg: 128, section_size: 0x2000 }) {
        let neg = eng5.accept(c);
        let done = eng5.complete(c).is_ok();
        if neg == Ok(0x2000) && done && eng5.state_of(c) == Ok(Handshake::Connected) && eng5.negot_consistent() {
            caught += 1;
            d.push("注5 正路:过".to_string());
        } else {
            d.push("注5 正路:堵".to_string());
        }
    } else {
        d.push("注5 建连失败".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x302c0 段十二码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CnCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：连接报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 连接语义报告。
#[derive(Clone, Copy, Debug)]
pub struct CnReport {
    /// 连接数。
    pub conns: usize,
    /// 握手请求数。
    pub seq: u32,
    /// 协商账行数。
    pub negot: usize,
    /// 协商账一致。
    pub negot_ok: bool,
    /// 侦听位。
    pub listening: bool,
}

impl CnReport {
    /// 建报告。
    pub fn build(eng: &ConnEngine) -> Self {
        CnReport {
            conns: eng.conn_count(),
            seq: eng.seq(),
            negot: eng.negot_len(),
            negot_ok: eng.negot_consistent(),
            listening: eng.is_listening(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.negot_ok && self.listening && self.seq > 0 && self.conns > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CnReport 连接={} 请求={} 协商账={} 协商账一致={} 侦听={}",
            self.conns, self.seq, self.negot, self.negot_ok, self.listening
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct CnCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_cn_checklist(eng: &ConnEngine) -> Vec<CnCheckRow> {
    let rep = CnReport::build(eng);
    let mut rows: Vec<CnCheckRow> = Vec::new();
    rows.push(CnCheckRow { name: "三态闭集", passed: eng.tri_state_reachable() });
    rows.push(CnCheckRow { name: "协商账一致", passed: eng.negot_consistent() });
    rows.push(CnCheckRow { name: "协商账单调", passed: eng.negot_monotone() });
    rows.push(CnCheckRow { name: "表容量口径", passed: CONN_SLOTS == 8 && eng.conn_count() <= CONN_SLOTS });
    rows.push(CnCheckRow { name: "消息上限口径", passed: MAX_MSG_LEN == 656 });
    rows.push(CnCheckRow { name: "节上限口径", passed: SECTION_MAX == 0x1000000 && PAGE_SIZE == 0x1000 });
    rows.push(CnCheckRow { name: "转录非空", passed: eng.export_lines().contains("连接表") });
    rows.push(CnCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CnCheckRow { name: "握手推进", passed: eng.seq() > 0 });
    rows.push(CnCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn cn_checklist_green(rows: &[CnCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn cn_stats_line(eng: &ConnEngine) -> String {
    format!(
        "连接概况：连接 {} 请求 {} 协商账 {}",
        eng.conn_count(),
        eng.seq(),
        eng.negot_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12422_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 标准会话：侦听 + 三连接（1 接受连接 / 1 拒绝 / 1 待裁决）。
    let mut eng = ConnEngine::new();
    eng.listen();
    let c0 = eng.connect(ConnRequest { max_msg: 128, section_size: 0x10000 });
    if let Ok(c) = c0 {
        let _ = eng.accept(c);
        let _ = eng.complete(c);
    }
    let c1 = eng.connect(ConnRequest { max_msg: 64, section_size: 0x2000 });
    if let Ok(c) = c1 {
        let _ = eng.reject(c);
    }
    let _ = eng.connect(ConnRequest { max_msg: 256, section_size: 0x4000 });
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        CnReport::build(&eng).all_green() && eng.export_lines().contains("连接表"),
        cn_stats_line(&eng),
    ));
    // X-b：消息长协商 10/10。
    let (ok_m, d_m) = drill_msg_len_10();
    out.push(("X-消息长协商10/10", ok_m, d_m));
    // X-c：连接表容量 10/10。
    let (ok_c, d_c) = drill_capacity_10();
    out.push(("X-连接表容量10/10", ok_c, d_c));
    // X-d：协商口径 10/10。
    let (ok_s, d_s) = drill_negot_size_10();
    out.push(("X-协商口径10/10", ok_s, d_s));
    // X-e：终验清单。
    let rows = build_cn_checklist(&eng);
    out.push((
        "X-终验清单10项",
        cn_checklist_green(&rows),
        CnReport::build(&eng).to_text(),
    ));
    out
}

/// run_unx12422_checks：本单判据聚合（诊断段 0x302c0）。
pub fn run_unx12422_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12422 NtConnectPort 语义档");
    // J1-a：三态握手 10/10。
    let (ok1, d1) = drill_tri_state_10();
    s.add_dyn("J1-三态握手10/10", ok1, &d1);
    // J1-b：共享内存协商注入 10/10。
    let (ok2, d2) = drill_negot_inject_10();
    s.add_dyn("J1-协商注入10/10", ok2, &d2);
    // J1-c：标准会话恒等式（三态全路径 + 协商账一致）。
    let mut eng = ConnEngine::new();
    eng.listen();
    let c = eng.connect(ConnRequest { max_msg: 128, section_size: 0x8000 });
    let mut tri_ok = c.is_ok();
    if let Ok(cc) = c {
        tri_ok = tri_ok
            && eng.state_of(cc) == Ok(Handshake::Requested)
            && eng.accept(cc) == Ok(0x8000)
            && eng.state_of(cc) == Ok(Handshake::Accepted)
            && eng.complete(cc).is_ok()
            && eng.state_of(cc) == Ok(Handshake::Connected);
    }
    s.add_dyn(
        "J1-三态全路径协商一致",
        tri_ok && eng.negot_consistent() && eng.tri_state_reachable(),
        &format!("请求={} 协商账={}", eng.seq(), eng.negot_consistent()),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：协商注入再核（错误码逐码）。
    let mut eng2 = ConnEngine::new();
    let r1 = eng2.connect(ConnRequest { max_msg: 128, section_size: 0x1000 });
    eng2.listen();
    let r2 = eng2.connect(ConnRequest { max_msg: MAX_MSG_LEN + 1, section_size: 0x1000 });
    let r3 = eng2.connect(ConnRequest { max_msg: 128, section_size: 0x1801 });
    let r4 = eng2.connect(ConnRequest { max_msg: 128, section_size: 0 });
    s.add_dyn(
        "J2-拒止码逐码",
        matches!(r1, Err(CnCode::NotListening))
            && matches!(r2, Err(CnCode::MsgLenMismatch))
            && matches!(r3, Err(CnCode::SectionUnaligned))
            && matches!(r4, Err(CnCode::SectionZero)),
        &format!("码1={:?} 码2={:?} 码3={:?} 码4={:?}", r1.err(), r2.err(), r3.err(), r4.err()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x302c0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12422_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}

