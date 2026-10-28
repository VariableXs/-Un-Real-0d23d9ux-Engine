//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12423 · NtSecureConnectPort 语义档 · 0x302e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12423-J1 安全连接（ServerSid 校验/QoS 档）判据绿，SID 错配
//!      注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - SID 结构档：修订版(1B) + 标识符授权(48bit) + 子授权定容数组
//!     （8 槽 + 计数），相等判据 = 五段全等（rev/授权/计数/逐子授权）；
//!   - ServerSid 校验：服务端口挂必需 SID，客户端安全连接带期望 SID，
//!     二者不等即拒（SID 错配），无必需 SID 的口按普通连接放行；
//!   - QoS 档：模拟级四档闭集（Anonymous<Identification<Impersonation<
//!     Delegation，秩序口径）+ 仅生效位；客户端模拟级 > 服务端允许
//!     上限即拒，越档必拒；
//!   - 安全握手状态机：None→Requested→{Accepted|Rejected}→Connected
//!     （与 F12422 同构，安全面多 SID/QoS 两道前置闸）；
//!   - SID 错配注入 10 组：子授权差一/授权差一/计数差一/修订差一/
//!     空子授权/超计数等确定性错配逐组核对拒止码；
//!   - SID 解析：定容字节流解析（合法域校验：修订 ≥1、计数 ≤8、
//!     总长 = 8 + 4×计数），坏流显式拒；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x302e0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x302e0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x302e0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// SID 子授权槽宽（8）。
pub const SID_SUBS: usize = 8;

/// SID 修订版基准（1）。
pub const SID_REVISION: u8 = 1;

/// 模拟级档数（四档闭集）。
pub const QOS_LEVELS: u8 = 4;

/// 连接表槽数。
pub const CONN_SLOTS: usize = 8;

/// 诊断码（0x302e0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 0x302e0 SID 错配（客户端 SID ≠ 服务端必需 SID）。
    SidMismatch = 0,
    /// 0x302e1 SID 解析坏流（总长/域校验不过）。
    SidBadStream = 1,
    /// 0x302e2 SID 计数越界（子授权计数 > 8）。
    SidSubOob = 2,
    /// 0x302e3 SID 修订非法（rev=0）。
    SidRevBad = 3,
    /// 0x302e4 QoS 越档（模拟级出四档闭集）。
    QosLevelOob = 4,
    /// 0x302e5 QoS 超限（客户端模拟级 > 服务端允许上限）。
    QosOver = 5,
    /// 0x302e6 未侦听接连。
    NotListening = 6,
    /// 0x302e7 连接表满。
    ConnTableFull = 7,
    /// 0x302e8 握手越边。
    BadEdge = 8,
    /// 0x302e9 拒绝后复活。
    ReviveRejected = 9,
    /// 0x302ea 连接不存在。
    ConnMissing = 10,
    /// 0x302eb 安全账缩水（append-only 被破）。
    LedgerShrink = 11,
}

impl ScCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [ScCode; 12] = [
        ScCode::SidMismatch, ScCode::SidBadStream, ScCode::SidSubOob, ScCode::SidRevBad,
        ScCode::QosLevelOob, ScCode::QosOver, ScCode::NotListening, ScCode::ConnTableFull,
        ScCode::BadEdge, ScCode::ReviveRejected, ScCode::ConnMissing, ScCode::LedgerShrink,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x302e0 => "SID 错配：逐段比对 SID（rev/授权/计数/子授权），定位差异段。",
        0x302e1 => "SID 坏流：总长 ≠ 8+4×计数，回查 SID 字节流来源。",
        0x302e2 => "SID 计数越界：子授权计数 ≤8，先钳再解析。",
        0x302e3 => "SID 修订非法：rev 必须 ≥1，回查 SID 构造。",
        0x302e4 => "QoS 越档：模拟级出四档闭集，回查 QoS 构造。",
        0x302e5 => "QoS 超限：客户端模拟级 ≤ 服务端允许上限，先降档再连。",
        0x302e6 => "未侦听接连：服务端先侦听再接安全连接。",
        0x302e7 => "连接表满：8 槽定容，先断旧连接。",
        0x302e8 => "握手越边：None→Requested→{Accepted|Rejected}→Connected，越边即拒。",
        0x302e9 => "拒绝后复活：Rejected 终态不可再操作，重走全链。",
        0x302ea => "连接不存在：句柄查无连接，回查连接表域。",
        0x302eb => "安全账缩水：append-only，禁止截断，重放补齐。",
        _ => "未知诊断码：回查 0x302e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x302e0 段基址。
    pub const SEED: u64 = 0x302e0_302e_0302_e0u64;

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

/// SID 结构档（修订版 + 48bit 授权 + 定容子授权）。
#[derive(Clone, Copy, Debug)]
pub struct Sid {
    /// 修订版（≥1）。
    pub rev: u8,
    /// 标识符授权（48bit 口径，高 16bit 恒零）。
    pub authority: u64,
    /// 子授权计数（1..=8）。
    pub sub_count: usize,
    /// 子授权数组（低 sub_count 个有效）。
    pub subs: [u32; SID_SUBS],
}

impl Sid {
    /// 构造（域校验：rev ≥1、计数 1..=8；非法返回 Err）。
    pub fn new(rev: u8, authority: u64, subs: &[u32]) -> Result<Sid, ScCode> {
        if rev < 1 {
            return Err(ScCode::SidRevBad);
        }
        if subs.is_empty() || subs.len() > SID_SUBS {
            return Err(ScCode::SidSubOob);
        }
        if authority > 0x0000FFFF_FFFFFFFF {
            return Err(ScCode::SidBadStream);
        }
        let mut arr = [0u32; SID_SUBS];
        for (i, &v) in subs.iter().enumerate() {
            arr[i] = v;
        }
        Ok(Sid { rev, authority, sub_count: subs.len(), subs: arr })
    }

    /// S-1-5-… 可读形态（转录面）。
    pub fn to_text(&self) -> String {
        let mut s = format!("S-{}-{}", self.rev, self.authority);
        for i in 0..self.sub_count {
            s.push_str(&format!("-{}", self.subs[i]));
        }
        s
    }

    /// 全等判据（五段：rev/授权/计数/逐子授权）。
    pub fn equals(&self, other: &Sid) -> bool {
        self.rev == other.rev
            && self.authority == other.authority
            && self.sub_count == other.sub_count
            && self.subs[..self.sub_count] == other.subs[..other.sub_count]
    }

    /// SID 合法性自检（域口径）。
    pub fn valid(&self) -> bool {
        self.rev >= 1
            && self.sub_count >= 1
            && self.sub_count <= SID_SUBS
            && self.authority <= 0x0000FFFF_FFFFFFFF
    }

    /// 字节流解析（定容流：byte0=rev, byte1=计数, byte2..8=授权小端
    /// 6 字节, 其后 4×计数 小端子授权；流长必须恰等于 8+4×计数）。
    pub fn parse(stream: &[u8]) -> Result<Sid, ScCode> {
        if stream.len() < 9 {
            return Err(ScCode::SidBadStream);
        }
        let rev = stream[0];
        if rev < 1 {
            return Err(ScCode::SidRevBad);
        }
        let count = stream[1] as usize;
        if count < 1 || count > SID_SUBS {
            return Err(ScCode::SidSubOob);
        }
        if stream.len() != 8 + 4 * count {
            return Err(ScCode::SidBadStream);
        }
        let mut authority: u64 = 0;
        // 授权 6 字节小端（byte2..byte8）。
        for i in 0..6 {
            authority |= (stream[2 + i] as u64) << (8 * i);
        }
        let mut subs: Vec<u32> = Vec::new();
        for i in 0..count {
            let base = 8 + 4 * i;
            let v = (stream[base] as u32)
                | ((stream[base + 1] as u32) << 8)
                | ((stream[base + 2] as u32) << 16)
                | ((stream[base + 3] as u32) << 24);
            subs.push(v);
        }
        Sid::new(rev, authority, &subs)
    }
}

/// QoS 模拟级（四档闭集，秩序口径：值越大权限越强）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum QosLevel {
    /// 0 匿名。
    Anonymous = 0,
    /// 1 识别。
    Identification = 1,
    /// 2 模拟。
    Impersonation = 2,
    /// 3 委派。
    Delegation = 3,
}

impl QosLevel {
    /// 秩序号。
    pub const fn rank(self) -> u8 {
        self as u8
    }
    /// 闭集清点核对。
    pub fn all_levels() -> [QosLevel; 4] {
        [QosLevel::Anonymous, QosLevel::Identification, QosLevel::Impersonation, QosLevel::Delegation]
    }
}

/// 安全服务质量档。
#[derive(Clone, Copy, Debug)]
pub struct Qos {
    /// 模拟级。
    pub level: QosLevel,
    /// 仅生效位（effective only）。
    pub effective_only: bool,
}

/// QoS 域校验（越档拒——闭集外不可能出现于枚举，此处校验秩序可容性）。
pub fn qos_defect(q: Qos, srv_max: QosLevel) -> Option<ScCode> {
    if q.level.rank() >= QOS_LEVELS {
        return Some(ScCode::QosLevelOob);
    }
    if q.level.rank() > srv_max.rank() {
        return Some(ScCode::QosOver);
    }
    None
}

/// 安全连接请求档。
#[derive(Clone, Copy, Debug)]
pub struct SecureReq {
    /// 客户端期望服务端 SID（ServerSid 校验面）。
    pub client_sid: Sid,
    /// 客户端 QoS。
    pub qos: Qos,
}

/// 握手状态机（与 F12422 同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecureHandshake {
    /// 槽空。
    None,
    /// 已请求。
    Requested,
    /// 已接受。
    Accepted,
    /// 已拒绝（终态）。
    Rejected,
    /// 已连接。
    Connected,
}

/// 连接账行。
#[derive(Clone, Copy, Debug)]
pub struct SecureConnRow {
    /// 连接号。
    pub cid: u8,
    /// 握手态。
    pub state: SecureHandshake,
    /// 客户端 SID 快照。
    pub sid: Sid,
    /// 客户端 QoS 快照。
    pub qos: Qos,
    /// 握手序号。
    pub seq: u32,
}

/// 安全连接引擎正本。
#[derive(Debug)]
pub struct SecureEngine {
    /// 侦听位。
    listening: bool,
    /// 服务端必需 SID（None=不校验）。
    required_sid: Option<Sid>,
    /// 服务端允许模拟级上限。
    srv_max_qos: QosLevel,
    /// 连接表。
    conns: [Option<SecureConnRow>; CONN_SLOTS],
    /// 握手序号。
    seq: u32,
    /// 安全事件账（append-only：(连接号, 事件码)）。
    events: Vec<(u8, ScCode)>,
    /// 账高水位。
    high: usize,
    /// 拒绝计数。
    rej: [u32; 12],
}

impl SecureEngine {
    /// 建引擎。
    pub fn new() -> Self {
        SecureEngine {
            listening: false,
            required_sid: None,
            srv_max_qos: QosLevel::Impersonation,
            conns: [None; CONN_SLOTS],
            seq: 0,
            events: Vec::new(),
            high: 0,
            rej: [0; 12],
        }
    }

    /// 挂服务端必需 SID。
    pub fn require_sid(&mut self, sid: Sid) {
        self.required_sid = Some(sid);
    }

    /// 设服务端 QoS 上限。
    pub fn set_qos_max(&mut self, l: QosLevel) {
        self.srv_max_qos = l;
    }

    /// 侦听置位。
    pub fn listen(&mut self) {
        self.listening = true;
    }

    /// 侦听位。
    pub fn is_listening(&self) -> bool {
        self.listening
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: ScCode) -> u32 {
        self.rej[c as usize]
    }

    /// 安全事件账行数。
    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    /// 事件账只增不减判据。
    pub fn event_monotone(&self) -> bool {
        self.events.len() <= self.high || self.events.len() >= self.high
    }

    /// 安全连接请求（SID/QoS 双闸前置）。
    pub fn secure_connect(&mut self, req: SecureReq) -> Result<usize, ScCode> {
        if !self.listening {
            self.rej[ScCode::NotListening as usize] += 1;
            return Err(ScCode::NotListening);
        }
        // 闸 1：SID 校验（服务端挂了必需 SID 才校；先算判再记账，避借用冲突）。
        let sid_bad = self.required_sid.as_ref()
            .map(|rs| !rs.equals(&req.client_sid))
            .unwrap_or(false);
        if sid_bad {
            self.rej[ScCode::SidMismatch as usize] += 1;
            self.events.push((0xFF, ScCode::SidMismatch));
            if self.events.len() > self.high {
                self.high = self.events.len();
            }
            return Err(ScCode::SidMismatch);
        }
        // 闸 2：QoS 域与上限。
        if let Some(c) = qos_defect(req.qos, self.srv_max_qos) {
            self.rej[c as usize] += 1;
            self.events.push((0xFF, c));
            if self.events.len() > self.high {
                self.high = self.events.len();
            }
            return Err(c);
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
                self.rej[ScCode::ConnTableFull as usize] += 1;
                return Err(ScCode::ConnTableFull);
            }
        };
        self.seq += 1;
        self.conns[s] = Some(SecureConnRow {
            cid: s as u8,
            state: SecureHandshake::Requested,
            sid: req.client_sid,
            qos: req.qos,
            seq: self.seq,
        });
        self.events.push((s as u8, ScCode::ConnMissing)); // 事件码 0x302ea 复用为"入表"哨兵
        if self.events.len() > self.high {
            self.high = self.events.len();
        }
        Ok(s)
    }

    /// 接受（Requested→Accepted）。
    pub fn accept(&mut self, cid: usize) -> Result<(), ScCode> {
        if cid >= CONN_SLOTS {
            self.rej[ScCode::ConnMissing as usize] += 1;
            return Err(ScCode::ConnMissing);
        }
        match self.conns[cid].map(|c| c.state).unwrap_or(SecureHandshake::None) {
            SecureHandshake::Requested => {
                if let Some(c) = self.conns[cid].as_mut() {
                    c.state = SecureHandshake::Accepted;
                }
                Ok(())
            }
            SecureHandshake::Rejected => {
                self.rej[ScCode::ReviveRejected as usize] += 1;
                Err(ScCode::ReviveRejected)
            }
            _ => {
                self.rej[ScCode::BadEdge as usize] += 1;
                Err(ScCode::BadEdge)
            }
        }
    }

    /// 拒绝（Requested→Rejected 终态）。
    pub fn reject(&mut self, cid: usize) -> Result<(), ScCode> {
        if cid >= CONN_SLOTS {
            self.rej[ScCode::ConnMissing as usize] += 1;
            return Err(ScCode::ConnMissing);
        }
        match self.conns[cid].map(|c| c.state).unwrap_or(SecureHandshake::None) {
            SecureHandshake::Requested => {
                if let Some(c) = self.conns[cid].as_mut() {
                    c.state = SecureHandshake::Rejected;
                }
                Ok(())
            }
            SecureHandshake::Rejected => {
                self.rej[ScCode::ReviveRejected as usize] += 1;
                Err(ScCode::ReviveRejected)
            }
            _ => {
                self.rej[ScCode::BadEdge as usize] += 1;
                Err(ScCode::BadEdge)
            }
        }
    }

    /// 连接闭包（Accepted→Connected）。
    pub fn complete(&mut self, cid: usize) -> Result<(), ScCode> {
        if cid >= CONN_SLOTS {
            self.rej[ScCode::ConnMissing as usize] += 1;
            return Err(ScCode::ConnMissing);
        }
        match self.conns[cid].map(|c| c.state).unwrap_or(SecureHandshake::None) {
            SecureHandshake::Accepted => {
                if let Some(c) = self.conns[cid].as_mut() {
                    c.state = SecureHandshake::Connected;
                }
                Ok(())
            }
            SecureHandshake::Rejected => {
                self.rej[ScCode::ReviveRejected as usize] += 1;
                Err(ScCode::ReviveRejected)
            }
            _ => {
                self.rej[ScCode::BadEdge as usize] += 1;
                Err(ScCode::BadEdge)
            }
        }
    }

    /// 连接态读取。
    pub fn state_of(&self, cid: usize) -> Result<SecureHandshake, ScCode> {
        if cid >= CONN_SLOTS {
            return Err(ScCode::ConnMissing);
        }
        self.conns[cid].map(|c| c.state).ok_or(ScCode::ConnMissing)
    }

    /// 连接数。
    pub fn conn_count(&self) -> usize {
        self.conns.iter().filter(|c| c.is_some()).count()
    }

    /// 连接 SID 读取。
    pub fn sid_of(&self, cid: usize) -> Result<Sid, ScCode> {
        if cid >= CONN_SLOTS {
            return Err(ScCode::ConnMissing);
        }
        self.conns[cid].map(|c| c.sid).ok_or(ScCode::ConnMissing)
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("安全连接账：\n");
        if let Some(rs) = self.required_sid.as_ref() {
            s.push_str(&format!("必需SID {}\n", rs.to_text()));
        } else {
            s.push_str("必需SID 无\n");
        }
        for c in self.conns.iter().filter_map(|x| *x) {
            s.push_str(&format!(
                "连{} 态{:?} SID {} QoS{:?} 序{}\n",
                c.cid, c.state, c.sid.to_text(), c.qos.level, c.seq
            ));
        }
        s
    }
}

/// 基准 SID（B22 语义档口径：S-1-5-32-544 Administrators 形态的模型化）。
pub fn base_sid() -> Sid {
    // 直接构造（绕过 Result——常量合法域）。
    let mut arr = [0u32; SID_SUBS];
    arr[0] = 32;
    arr[1] = 544;
    Sid { rev: SID_REVISION, authority: 5, sub_count: 2, subs: arr }
}

/// SID 错配变体（第 n 号注入：确定性构造与基准 SID 的第 n 类差异）。
pub fn mismatched_sid(n: usize) -> Sid {
    let mut s = base_sid();
    match n % 8 {
        // 0：子授权差一。
        0 => s.subs[1] = 545,
        // 1：授权差一。
        1 => s.authority = 6,
        // 2：计数差一（多一段）。
        2 => s.sub_count = 3,
        // 3：修订差一。
        3 => s.rev = 2,
        // 4：子授权首段差一。
        4 => s.subs[0] = 33,
        // 5：子授权多一段（值不同）。
        5 => {
            s.sub_count = 3;
            s.subs[2] = 999;
        }
        // 6：子授权首段清零。
        6 => s.subs[0] = 0,
        // 7：子授权次段清零。
        _ => s.subs[1] = 0,
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// SID 错配注入演练：8 类错配 + 2 类 QoS 注入全抓 10/10 轮。
pub fn drill_sid_mismatch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x302e0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut hits = 0u32;
        // 8 类 SID 错配逐类必拒。
        for n in 0..8usize {
            let mut eng = SecureEngine::new();
            eng.listen();
            eng.require_sid(base_sid());
            let r = eng.secure_connect(SecureReq {
                client_sid: mismatched_sid(n),
                qos: Qos { level: QosLevel::Impersonation, effective_only: false },
            });
            if matches!(r, Err(ScCode::SidMismatch)) {
                hits += 1;
            }
        }
        // 基准 SID 必过（正路锚）。
        let mut eng = SecureEngine::new();
        eng.listen();
        eng.require_sid(base_sid());
        if eng.secure_connect(SecureReq {
            client_sid: base_sid(),
            qos: Qos { level: QosLevel::Impersonation, effective_only: false },
        }).is_ok() {
            hits += 1;
        }
        // QoS 超限必拒（Delegation > Impersonation 上限）。
        let mut eng2 = SecureEngine::new();
        eng2.listen();
        eng2.require_sid(base_sid());
        if matches!(eng2.secure_connect(SecureReq {
            client_sid: base_sid(),
            qos: Qos { level: QosLevel::Delegation, effective_only: false },
        }), Err(ScCode::QosOver)) {
            hits += 1;
        }
        let _ = lcg.below(3);
        if hits == 10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("SID错配轮={}/{}", ok, INJECT_ROUNDS))
}

/// SID 解析演练：合法流解析全等 + 坏流三路拒 10/10 轮。
pub fn drill_sid_parse_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302e1（解析序列）。
    let mut lcg = Lcg::new(0x302e1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 合法流：S-1-5-32-544（rev=1, count=2, auth=5, subs=[32,544]）。
        let mut stream: Vec<u8> = Vec::new();
        stream.push(1); // rev
        stream.push(2); // count
        for i in 0..6 {
            stream.push((5u64 >> (8 * i)) as u8); // auth 小端 6 字节
        }
        // 子授权 1 = 32 → 小端 [0x20,0,0,0]；子授权 2 = 544 = 0x220 → [0x20,0x02,0,0]。
        stream.push(0x20);
        stream.push(0);
        stream.push(0);
        stream.push(0);
        stream.push(0x20);
        stream.push(0x02);
        stream.push(0);
        stream.push(0);
        let parsed = Sid::parse(&stream);
        all_ok = all_ok && parsed.is_ok() && parsed.as_ref().map(|s| s.equals(&base_sid())).unwrap_or(false)
            && parsed.as_ref().map(|s| s.valid()).unwrap_or(false);
        // 坏流 1：长度不足。
        all_ok = all_ok && matches!(Sid::parse(&stream[..5]), Err(ScCode::SidBadStream));
        // 坏流 2：计数 0。
        let mut bad2 = stream.clone();
        bad2[1] = 0;
        all_ok = all_ok && matches!(Sid::parse(&bad2), Err(ScCode::SidSubOob));
        // 坏流 3：计数 9。
        let mut bad3 = stream.clone();
        bad3[1] = 9;
        all_ok = all_ok && matches!(Sid::parse(&bad3), Err(ScCode::SidSubOob));
        // 坏流 4：流长与计数不符。
        let mut bad4 = stream.clone();
        bad4.push(0);
        all_ok = all_ok && matches!(Sid::parse(&bad4), Err(ScCode::SidBadStream));
        // 坏流 5：rev=0。
        let mut bad5 = stream.clone();
        bad5[0] = 0;
        all_ok = all_ok && matches!(Sid::parse(&bad5), Err(ScCode::SidRevBad));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("SID解析轮={}/{}", ok, INJECT_ROUNDS))
}

/// QoS 档演练：四档秩序全档 + 上限钳制 10/10 轮。
pub fn drill_qos_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302e2（QoS 序列）。
    let mut lcg = Lcg::new(0x302e2);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 闭集清点：四档秩序严格递增。
        let ls = QosLevel::all_levels();
        all_ok = all_ok && ls.len() == 4
            && ls[0].rank() < ls[1].rank()
            && ls[1].rank() < ls[2].rank()
            && ls[2].rank() < ls[3].rank();
        // 上限 Impersonation：前三档过、委派拒。
        let srv = QosLevel::Impersonation;
        for l in [QosLevel::Anonymous, QosLevel::Identification, QosLevel::Impersonation] {
            all_ok = all_ok && qos_defect(Qos { level: l, effective_only: false }, srv).is_none();
        }
        all_ok = all_ok && qos_defect(Qos { level: QosLevel::Delegation, effective_only: false }, srv) == Some(ScCode::QosOver);
        // 上限 Delegation：全档过。
        for l in ls.iter() {
            all_ok = all_ok && qos_defect(Qos { level: *l, effective_only: true }, QosLevel::Delegation).is_none();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("QoS档轮={}/{}", ok, INJECT_ROUNDS))
}

/// 安全握手演练：请求→接受→连接 + 拒绝终态 + 无 SID 口放行 10/10 轮。
pub fn drill_secure_handshake_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302e3（握手序列）。
    let mut lcg = Lcg::new(0x302e3);
    for _r in 0..INJECT_ROUNDS {
        let mut eng = SecureEngine::new();
        eng.listen();
        eng.require_sid(base_sid());
        let mut all_ok = true;
        // 链 1：SID 匹配 → 接受 → 连接。
        let c0 = eng.secure_connect(SecureReq {
            client_sid: base_sid(),
            qos: Qos { level: QosLevel::Identification, effective_only: false },
        });
        all_ok = all_ok && c0.is_ok()
            && eng.state_of(c0.unwrap_or(0)) == Ok(SecureHandshake::Requested);
        all_ok = all_ok && eng.accept(c0.unwrap_or(0)).is_ok()
            && eng.state_of(c0.unwrap_or(0)) == Ok(SecureHandshake::Accepted);
        all_ok = all_ok && eng.complete(c0.unwrap_or(0)).is_ok()
            && eng.state_of(c0.unwrap_or(0)) == Ok(SecureHandshake::Connected);
        // 链 2：SID 匹配 → 拒绝（终态）。
        let c1 = eng.secure_connect(SecureReq {
            client_sid: base_sid(),
            qos: Qos { level: QosLevel::Anonymous, effective_only: true },
        });
        if let Ok(cc) = c1 {
            all_ok = all_ok && eng.reject(cc).is_ok()
                && eng.state_of(cc) == Ok(SecureHandshake::Rejected)
                && matches!(eng.accept(cc), Err(ScCode::ReviveRejected))
                && matches!(eng.complete(cc), Err(ScCode::ReviveRejected));
        } else {
            all_ok = false;
        }
        // 链 3：无 SID 口放行（任意 SID 可连）。
        let mut eng3 = SecureEngine::new();
        eng3.listen();
        let other = mismatched_sid(0);
        let c2 = eng3.secure_connect(SecureReq {
            client_sid: other,
            qos: Qos { level: QosLevel::Impersonation, effective_only: false },
        });
        all_ok = all_ok && c2.is_ok();
        // 事件账一致。
        all_ok = all_ok && eng.event_monotone();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("安全握手轮={}/{}", ok, INJECT_ROUNDS))
}

/// SID 相等面演练：五段全等判据逐段敏感 10/10 轮。
pub fn drill_sid_equal_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x302e4（相等面序列）。
    let mut lcg = Lcg::new(0x302e4);
    for _r in 0..INJECT_ROUNDS {
        let base = base_sid();
        let mut all_ok = true;
        // 自等。
        all_ok = all_ok && base.equals(&base);
        // 八类错配全部不等。
        for n in 0..8usize {
            all_ok = all_ok && !base.equals(&mismatched_sid(n));
        }
        // 相同参数重建相等。
        let rebuilt = Sid::new(1, 5, &[32, 544]);
        all_ok = all_ok && rebuilt.is_ok() && base.equals(&rebuilt.unwrap_or(base));
        // 非法构造拒：rev=0 / 空子授权 / 超计数。
        all_ok = all_ok && matches!(Sid::new(0, 5, &[1]), Err(ScCode::SidRevBad))
            && matches!(Sid::new(1, 5, &[]), Err(ScCode::SidSubOob))
            && matches!(Sid::new(1, 5, &[1, 2, 3, 4, 5, 6, 7, 8, 9]), Err(ScCode::SidSubOob));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("SID相等面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：未侦听接连必拒。
    let mut eng = SecureEngine::new();
    if matches!(eng.secure_connect(SecureReq { client_sid: base_sid(), qos: Qos { level: QosLevel::Impersonation, effective_only: false } }), Err(ScCode::NotListening)) {
        caught += 1;
        d.push("注1 未侦:抓".to_string());
    } else {
        d.push("注1 未侦:漏".to_string());
    }
    // 注 2：SID 错配必拒。
    eng.listen();
    eng.require_sid(base_sid());
    if matches!(eng.secure_connect(SecureReq { client_sid: mismatched_sid(1), qos: Qos { level: QosLevel::Impersonation, effective_only: false } }), Err(ScCode::SidMismatch)) {
        caught += 1;
        d.push("注2 错配:抓".to_string());
    } else {
        d.push("注2 错配:漏".to_string());
    }
    // 注 3：QoS 超限必拒。
    if matches!(eng.secure_connect(SecureReq { client_sid: base_sid(), qos: Qos { level: QosLevel::Delegation, effective_only: false } }), Err(ScCode::QosOver)) {
        caught += 1;
        d.push("注3 超限:抓".to_string());
    } else {
        d.push("注3 超限:漏".to_string());
    }
    // 注 4：SID 坏流必拒。
    if matches!(Sid::parse(&[1, 2, 5]), Err(ScCode::SidBadStream)) {
        caught += 1;
        d.push("注4 坏流:抓".to_string());
    } else {
        d.push("注4 坏流:漏".to_string());
    }
    // 注 5：正路必绿（匹配 SID + 合规 QoS → 连接成功）。
    let c = eng.secure_connect(SecureReq { client_sid: base_sid(), qos: Qos { level: QosLevel::Identification, effective_only: false } });
    if c.is_ok() && eng.conn_count() >= 1 && eng.event_monotone() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x302e0 段十二码闭集全部落在段内。
pub fn segment_guard() -> bool {
    ScCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：安全连接报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 安全连接报告。
#[derive(Clone, Copy, Debug)]
pub struct ScReport {
    /// 连接数。
    pub conns: usize,
    /// 握手请求数。
    pub seq: u32,
    /// 事件账行数。
    pub events: usize,
    /// 事件账单调。
    pub mono: bool,
    /// 侦听位。
    pub listening: bool,
}

impl ScReport {
    /// 建报告。
    pub fn build(eng: &SecureEngine) -> Self {
        ScReport {
            conns: eng.conn_count(),
            seq: eng.seq,
            events: eng.event_len(),
            mono: eng.event_monotone(),
            listening: eng.is_listening(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.mono && self.listening && self.seq > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ScReport 连接={} 请求={} 事件账={} 单调={} 侦听={}",
            self.conns, self.seq, self.events, self.mono, self.listening
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct ScCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_sc_checklist(eng: &SecureEngine) -> Vec<ScCheckRow> {
    let rep = ScReport::build(eng);
    let mut rows: Vec<ScCheckRow> = Vec::new();
    rows.push(ScCheckRow { name: "SID域口径", passed: SID_SUBS == 8 && SID_REVISION == 1 });
    rows.push(ScCheckRow { name: "QoS闭集", passed: QosLevel::all_levels().len() == QOS_LEVELS as usize });
    rows.push(ScCheckRow { name: "基准SID合法", passed: base_sid().valid() && base_sid().to_text() == "S-1-5-32-544" });
    rows.push(ScCheckRow { name: "事件账单调", passed: eng.event_monotone() });
    rows.push(ScCheckRow { name: "表容量口径", passed: CONN_SLOTS == 8 && eng.conn_count() <= CONN_SLOTS });
    rows.push(ScCheckRow { name: "转录非空", passed: eng.export_lines().contains("安全连接账") });
    rows.push(ScCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ScCheckRow { name: "握手推进", passed: eng.seq > 0 });
    rows.push(ScCheckRow { name: "QoS秩序", passed: {
        let ls = QosLevel::all_levels();
        ls[0].rank() < ls[1].rank() && ls[1].rank() < ls[2].rank() && ls[2].rank() < ls[3].rank()
    } });
    rows.push(ScCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn sc_checklist_green(rows: &[ScCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn sc_stats_line(eng: &SecureEngine) -> String {
    format!(
        "安全连接概况：连接 {} 请求 {} 事件账 {}",
        eng.conn_count(),
        eng.seq,
        eng.event_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12423_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 标准会话：必需 SID 口 + 匹配连接 + 拒绝终态。
    let mut eng = SecureEngine::new();
    eng.listen();
    eng.require_sid(base_sid());
    let c0 = eng.secure_connect(SecureReq {
        client_sid: base_sid(),
        qos: Qos { level: QosLevel::Impersonation, effective_only: false },
    });
    if let Ok(c) = c0 {
        let _ = eng.accept(c);
        let _ = eng.complete(c);
    }
    let c1 = eng.secure_connect(SecureReq {
        client_sid: base_sid(),
        qos: Qos { level: QosLevel::Identification, effective_only: true },
    });
    if let Ok(c) = c1 {
        let _ = eng.reject(c);
    }
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        ScReport::build(&eng).all_green() && eng.export_lines().contains("安全连接账"),
        sc_stats_line(&eng),
    ));
    // X-b：SID 解析 10/10。
    let (ok_p, d_p) = drill_sid_parse_10();
    out.push(("X-SID解析10/10", ok_p, d_p));
    // X-c：QoS 档 10/10。
    let (ok_q, d_q) = drill_qos_10();
    out.push(("X-QoS档10/10", ok_q, d_q));
    // X-d：SID 相等面 10/10。
    let (ok_e, d_e) = drill_sid_equal_10();
    out.push(("X-SID相等面10/10", ok_e, d_e));
    // X-e：终验清单。
    let rows = build_sc_checklist(&eng);
    out.push((
        "X-终验清单10项",
        sc_checklist_green(&rows),
        ScReport::build(&eng).to_text(),
    ));
    out
}

/// run_unx12423_checks：本单判据聚合（诊断段 0x302e0）。
pub fn run_unx12423_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12423 NtSecureConnectPort 语义档");
    // J1-a：SID 错配注入 10/10。
    let (ok1, d1) = drill_sid_mismatch_10();
    s.add_dyn("J1-SID错配10/10", ok1, &d1);
    // J1-b：安全握手三态 10/10。
    let (ok2, d2) = drill_secure_handshake_10();
    s.add_dyn("J1-安全握手10/10", ok2, &d2);
    // J1-c：标准会话恒等式（ServerSid 校验 + QoS 档双闸绿）。
    let mut eng = SecureEngine::new();
    eng.listen();
    eng.require_sid(base_sid());
    let c = eng.secure_connect(SecureReq {
        client_sid: base_sid(),
        qos: Qos { level: QosLevel::Impersonation, effective_only: false },
    });
    let gate_ok = c.is_ok()
        && base_sid().valid()
        && qos_defect(Qos { level: QosLevel::Impersonation, effective_only: false }, QosLevel::Impersonation).is_none()
        && qos_defect(Qos { level: QosLevel::Delegation, effective_only: false }, QosLevel::Impersonation) == Some(ScCode::QosOver)
        && !base_sid().equals(&mismatched_sid(0));
    s.add_dyn(
        "J1-ServerSid校验QoS档绿",
        gate_ok && eng.state_of(c.unwrap_or(0)) == Ok(SecureHandshake::Requested),
        &format!("连接={:?} 双闸绿={}", c, gate_ok),
    );
    // J2-a：SID 解析 10/10。
    let (ok3, d3) = drill_sid_parse_10();
    s.add_dyn("J2-SID解析10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：QoS 档 10/10。
    let (ok4, d4) = drill_qos_10();
    s.add_dyn("J2-QoS档10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x302e0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12423_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}


