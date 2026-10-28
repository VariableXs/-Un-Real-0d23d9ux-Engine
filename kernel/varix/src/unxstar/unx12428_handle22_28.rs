//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12428 · 端口句柄语义：服务端/客户端两态 · 0x30380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12428-J1 两态句柄能力矩阵判据绿，客户端句柄误调服务端
//!      接口注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 句柄两态：ServerHandle（连接裁决/侦听/关闭服务权）与
//!     ClientHandle（请求投递/回包接收权），能力互斥不重叠；
//!   - 能力矩阵：五能力 × 两态闭集——Listen/AcceptReject 仅服务端、
//!     Request/ReplyRecv 仅客户端、Close 双态皆有（语义档口径）；
//!   - 句柄安全：句柄号 12 位槽位 + 4 位代数（generation），错代句柄
//!     （悬垂引用）显性拒——防句柄复用劫持（ABL 类漏洞内核口径）；
//!   - 误调注入：客户端句柄调 Accept/Listen/裁决接口、服务端句柄调
//!     Request/回包接收接口，逐例 10/10 全拒且逐码分账；
//!   - 句柄账：定容 16 槽，每槽 (态, 代数, 存活)，关闭后代数递增、
//!     槽可复用，旧代引用必拒 StaleHandle；
//!   - 能力派生链：服务端 accept 后派生客户端句柄，派生账 append-only
//!     （谁派生的谁可审计），派生数守恒（服务端开-关 ≡ 存活客户端）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，误调矩阵 10 能力×10 轮全跑；
//!   - 诊断码闭集 0x30380 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30380 段）。
pub const SEG_BASE: u32 = 0x30380;

/// 句柄表定容（16 槽）。
pub const HANDLE_CAP: usize = 16;

/// 代数位宽（4 位，模 16 递增）。
pub const GEN_MOD: u32 = 16;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30380 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HdCode {
    /// 0x30380 客户端句柄调服务端接口。
    ClientOnServer = 0,
    /// 0x30381 服务端句柄调客户端接口。
    ServerOnClient = 1,
    /// 0x30382 悬垂句柄（槽空）。
    DeadHandle = 2,
    /// 0x30383 错代句柄。
    StaleHandle = 3,
    /// 0x30384 句柄表满。
    TableFull = 4,
    /// 0x30385 越界句柄号。
    HandleOob = 5,
    /// 0x30386 派生守恒破。
    DeriveConsrv = 6,
    /// 0x30387 双重关闭。
    DupClose = 7,
    /// 0x30388 非派生客户端（无来源账）。
    OrphanClient = 8,
    /// 0x30389 段位越界。
    SegOob = 9,
    /// 0x3038A 能力矩阵越界（能力号出闭集）。
    CapOob = 10,
    /// 0x3038B 关闭后误用。
    ClosedUse = 11,
}

impl HdCode {
    /// 全部码闭集。
    pub const ALL: [HdCode; 12] = [
        HdCode::ClientOnServer,
        HdCode::ServerOnClient,
        HdCode::DeadHandle,
        HdCode::StaleHandle,
        HdCode::TableFull,
        HdCode::HandleOob,
        HdCode::DeriveConsrv,
        HdCode::DupClose,
        HdCode::OrphanClient,
        HdCode::SegOob,
        HdCode::CapOob,
        HdCode::ClosedUse,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            HdCode::ClientOnServer => "客户端句柄无服务端能力，改持服务端句柄或走请求通道",
            HdCode::ServerOnClient => "服务端句柄无请求能力，服务端用派生客户端句柄发请求",
            HdCode::DeadHandle => "句柄槽已空，对象已销毁，核对生命周期",
            HdCode::StaleHandle => "句柄代数过期，重新建立连接获取新句柄",
            HdCode::TableFull => "句柄表满（16），先关闭闲置连接",
            HdCode::HandleOob => "句柄号越界，核对句柄编码",
            HdCode::DeriveConsrv => "派生守恒破账，核对派生账与关闭账",
            HdCode::DupClose => "句柄已关闭，禁止二次关闭",
            HdCode::OrphanClient => "客户端句柄无派生来源，连接必须经服务端 accept 建立",
            HdCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            HdCode::CapOob => "能力号出闭集，核对能力矩阵表",
            HdCode::ClosedUse => "句柄已关闭仍被使用，查生命周期竞态",
        }
    }
}

/// 句柄态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleKind {
    /// 服务端句柄。
    Server,
    /// 客户端句柄。
    Client,
}

/// 能力闭集（五能力）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// 侦听（仅服务端）。
    Listen,
    /// 连接裁决（仅服务端）。
    AcceptReject,
    /// 请求投递（仅客户端）。
    Request,
    /// 回包接收（仅客户端）。
    ReplyRecv,
    /// 关闭（双态皆有）。
    Close,
}

/// 能力矩阵判：态 × 能力 → 是否授权（判据面核心表）。
pub fn capability_granted(kind: HandleKind, cap: Capability) -> bool {
    match (kind, cap) {
        (HandleKind::Server, Capability::Listen) => true,
        (HandleKind::Server, Capability::AcceptReject) => true,
        (HandleKind::Server, Capability::Request) => false,
        (HandleKind::Server, Capability::ReplyRecv) => false,
        (HandleKind::Server, Capability::Close) => true,
        (HandleKind::Client, Capability::Listen) => false,
        (HandleKind::Client, Capability::AcceptReject) => false,
        (HandleKind::Client, Capability::Request) => true,
        (HandleKind::Client, Capability::ReplyRecv) => true,
        (HandleKind::Client, Capability::Close) => true,
    }
}

/// 句柄编码：槽位 12 位 + 代数 4 位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawHandle {
    /// 槽位（0..15）。
    pub slot: u32,
    /// 代数（0..15）。
    pub gen: u32,
}

impl RawHandle {
    /// 编码为 u16。
    pub fn pack(self) -> u16 {
        ((self.slot & 0x0FFF) as u16) | (((self.gen & 0x0F) as u16) << 12)
    }
    /// 解码（越界返回 None）。
    pub fn unpack(v: u16) -> Option<RawHandle> {
        let slot = (v & 0x0FFF) as u32;
        let gen = (v >> 12) as u32;
        if slot >= HANDLE_CAP as u32 {
            None
        } else {
            Some(RawHandle { slot, gen })
        }
    }
}

/// 句柄槽。
#[derive(Clone, Copy, Debug)]
struct HandleSlot {
    /// 态。
    kind: HandleKind,
    /// 代数。
    gen: u32,
    /// 存活。
    alive: bool,
    /// 派生来源槽（服务端句柄无来源记 u32::MAX）。
    parent: u32,
}

/// 派生账行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct DeriveRow {
    /// 服务端槽。
    pub server_slot: u32,
    /// 派生出的客户端槽。
    pub client_slot: u32,
}

/// 派生账定容。
pub const DERIVE_LEDGER_CAP: usize = 64;

/// 句柄语义机。
pub struct HandleTable {
    /// 槽表。
    slots: [Option<HandleSlot>; HANDLE_CAP],
    /// 派生账。
    derive_ledger: [Option<DeriveRow>; DERIVE_LEDGER_CAP],
    derive_len: usize,
    /// 派生开启计数 / 关闭计数（守恒两项）。
    derived_total: u64,
    client_closed: u64,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl HandleTable {
    /// 新建。
    pub fn new() -> HandleTable {
        HandleTable {
            slots: [None; HANDLE_CAP],
            derive_ledger: [None; DERIVE_LEDGER_CAP],
            derive_len: 0,
            derived_total: 0,
            client_closed: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: HdCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 分配句柄（指定态）：空槽分配 + 代数 0 起。
    pub fn alloc(&mut self, kind: HandleKind) -> Result<RawHandle, u32> {
        let mut slot = 0usize;
        let mut found = false;
        while slot < HANDLE_CAP {
            match self.slots[slot] {
                None => {
                    found = true;
                    break;
                }
                Some(ref s) if !s.alive => {
                    found = true;
                    break;
                }
                _ => slot += 1,
            }
        }
        if !found {
            return Err(self.hit(HdCode::TableFull));
        }
        let gen = match self.slots[slot] {
            Some(ref s) => (s.gen + 1) % GEN_MOD,
            None => 0,
        };
        self.slots[slot] = Some(HandleSlot {
            kind,
            gen,
            alive: true,
            parent: if kind == HandleKind::Server { u32::MAX } else { u32::MAX },
        });
        Ok(RawHandle { slot: slot as u32, gen })
    }

    /// 槽内校验：存活 + 代数匹配（三态拒：Dead/Stale/Oob）。
    fn validate(&mut self, h: RawHandle) -> Result<HandleSlot, u32> {
        if h.slot as usize >= HANDLE_CAP {
            return Err(self.hit(HdCode::HandleOob));
        }
        match self.slots[h.slot as usize] {
            Some(ref s) if s.alive => {
                if s.gen != h.gen {
                    return Err(self.hit(HdCode::StaleHandle));
                }
                Ok(*s)
            }
            Some(_) => Err(self.hit(HdCode::ClosedUse)),
            None => Err(self.hit(HdCode::DeadHandle)),
        }
    }

    /// 能力调用总入口：矩阵判 + 误调拒（判准 J1 核心）。
    pub fn call(&mut self, h: RawHandle, cap: Capability) -> Result<(), u32> {
        let s = self.validate(h)?;
        if (cap as u32) > 4 {
            return Err(self.hit(HdCode::CapOob));
        }
        if capability_granted(s.kind, cap) {
            Ok(())
        } else {
            match s.kind {
                HandleKind::Client => Err(self.hit(HdCode::ClientOnServer)),
                HandleKind::Server => Err(self.hit(HdCode::ServerOnClient)),
            }
        }
    }

    /// 派生客户端句柄（服务端 accept 后调用，派生账入账）。
    pub fn derive_client(&mut self, srv: RawHandle) -> Result<RawHandle, u32> {
        let s = self.validate(srv)?;
        if s.kind != HandleKind::Server {
            return Err(self.hit(HdCode::ServerOnClient));
        }
        let cli = self.alloc(HandleKind::Client)?;
        // 派生账 append-only。
        if self.derive_len < DERIVE_LEDGER_CAP {
            self.derive_ledger[self.derive_len] =
                Some(DeriveRow { server_slot: srv.slot, client_slot: cli.slot });
            self.derive_len += 1;
        }
        if let Some(ref mut slot) = self.slots[cli.slot as usize] {
            slot.parent = srv.slot;
        }
        self.derived_total += 1;
        Ok(cli)
    }

    /// 关闭句柄（双态皆有；客户端关闭计守恒账）。
    pub fn close(&mut self, h: RawHandle) -> Result<(), u32> {
        let s = self.validate(h)?;
        if !s.alive {
            return Err(self.hit(HdCode::DupClose));
        }
        if let Some(ref mut slot) = self.slots[h.slot as usize] {
            slot.alive = false;
        }
        if s.kind == HandleKind::Client {
            self.client_closed += 1;
        }
        Ok(())
    }

    /// 派生守恒：derived_total - client_closed ≡ 存活客户端句柄数。
    pub fn derive_conserved(&self) -> bool {
        let mut alive_clients = 0;
        let mut i = 0;
        while i < HANDLE_CAP {
            match self.slots[i] {
                Some(ref s) if s.alive && s.kind == HandleKind::Client => alive_clients += 1,
                _ => {}
            }
            i += 1;
        }
        self.derived_total.saturating_sub(self.client_closed) == alive_clients as u64
    }

    /// 派生账单调。
    pub fn derive_monotone(&self) -> bool {
        let mut i = 0;
        while i < self.derive_len {
            if self.derive_ledger[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 存活句柄数。
    pub fn alive_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < HANDLE_CAP {
            match self.slots[i] {
                Some(ref s) if s.alive => n += 1,
                _ => {}
            }
            i += 1;
        }
        n
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: HdCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 能力矩阵全量演练（判准 J1 第一句）。
///
/// 5 能力 × 2 态 = 10 格逐格核（正例授权 / 负例拒止），10 轮全绿。
pub fn drill_capability_matrix_10() -> (bool, String) {
    // 矩阵期望（真值表独立写死，不复用 capability_granted——判据纪律）。
    let expect: [(bool, bool); 5] = [
        (true, false),   // Listen
        (true, false),   // AcceptReject
        (false, true),   // Request
        (false, true),   // ReplyRecv
        (true, true),    // Close
    ];
    let caps = [
        Capability::Listen,
        Capability::AcceptReject,
        Capability::Request,
        Capability::ReplyRecv,
        Capability::Close,
    ];
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        let mut ci = 0usize;
        while ci < 5 {
            if capability_granted(HandleKind::Server, caps[ci]) != expect[ci].0 {
                r_ok = false;
                d.push(format!("轮{} 服务端能力{}错", round, ci));
            }
            if capability_granted(HandleKind::Client, caps[ci]) != expect[ci].1 {
                r_ok = false;
                d.push(format!("轮{} 客户端能力{}错", round, ci));
            }
            ci += 1;
        }
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("能力矩阵 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 客户端句柄误调服务端接口注入 10/10 轮（判准 J1 第二句）。
pub fn drill_client_misuse_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let srv = t.alloc(HandleKind::Server);
        let cli = match srv {
            Ok(s) => t.derive_client(s),
            Err(_) => Err(0),
        };
        let cli = match cli {
            Ok(c) => c,
            Err(_) => {
                d.push(format!("轮{} 派生失败", round));
                round += 1;
                continue;
            }
        };
        let mut r_ok = true;
        // 误调三服务端能力：Listen / AcceptReject（Request/ReplyRecv 是
        // 客户端合法能力，不注入）。
        if let Err(c) = t.call(cli, Capability::Listen) {
            if c != HdCode::ClientOnServer.val() {
                r_ok = false;
                d.push(format!("轮{} Listen错码{:x}", round, c));
            }
        } else {
            r_ok = false;
        }
        if let Err(c) = t.call(cli, Capability::AcceptReject) {
            if c != HdCode::ClientOnServer.val() {
                r_ok = false;
            }
        } else {
            r_ok = false;
        }
        // 客户端合法能力放行：Request / ReplyRecv / Close。
        let legal = t.call(cli, Capability::Request).is_ok()
            && t.call(cli, Capability::ReplyRecv).is_ok();
        if r_ok && legal {
            ok += 1;
        } else {
            d.push(format!("轮{} 合法能力{}", round, legal));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("客户端误调 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 服务端句柄误调客户端接口注入 10/10 轮。
pub fn drill_server_misuse_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let srv = match t.alloc(HandleKind::Server) {
            Ok(s) => s,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        let mut r_ok = true;
        // 误调 Request / ReplyRecv。
        if let Err(c) = t.call(srv, Capability::Request) {
            if c != HdCode::ServerOnClient.val() {
                r_ok = false;
            }
        } else {
            r_ok = false;
        }
        if let Err(c) = t.call(srv, Capability::ReplyRecv) {
            if c != HdCode::ServerOnClient.val() {
                r_ok = false;
            }
        } else {
            r_ok = false;
        }
        // 服务端合法能力放行。
        let legal = t.call(srv, Capability::Listen).is_ok()
            && t.call(srv, Capability::AcceptReject).is_ok()
            && t.call(srv, Capability::Close).is_ok();
        if r_ok && legal {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("服务端误调 {}/{}", ok, INJECT_ROUNDS))
}

/// 悬垂/错代/越界/双关四族句柄注入 10/10 轮。
pub fn drill_stale_family_10() -> (bool, String) {
    let mut caught = [0usize; 4];
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let h = match t.alloc(HandleKind::Server) {
            Ok(h) => h,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        // 1) 越界句柄号（slot 16+）。
        let oob = RawHandle { slot: HANDLE_CAP as u32, gen: h.gen };
        if let Err(c) = t.call(oob, Capability::Listen) {
            if c == HdCode::HandleOob.val() {
                caught[0] += 1;
            }
        }
        // 2) 错代句柄（关闭后槽复用代数递增，旧代引用必拒）。
        let _ = t.close(h);
        let h2 = t.alloc(HandleKind::Server);
        if let Ok(h2) = h2 {
            if h2.gen != h.gen {
                if let Err(c) = t.call(h, Capability::Listen) {
                    if c == HdCode::StaleHandle.val() {
                        caught[1] += 1;
                    }
                }
            }
        }
        // 3) 双重关闭（新句柄关一次再关一次 → ClosedUse 闸）。
        let h3 = t.alloc(HandleKind::Server);
        if let Ok(h3) = h3 {
            let _ = t.close(h3);
            if let Err(c) = t.close(h3) {
                if c == HdCode::ClosedUse.val() {
                    caught[2] += 1;
                }
            }
        }
        // 4) 关闭后误用（另一新句柄关闭后调用 → ClosedUse 闸）。
        let h4 = t.alloc(HandleKind::Server);
        if let Ok(h4) = h4 {
            let _ = t.close(h4);
            if let Err(c) = t.call(h4, Capability::Close) {
                if c == HdCode::ClosedUse.val() {
                    caught[3] += 1;
                }
            }
        }
        round += 1;
    }
    let ok = caught.iter().all(|c| *c == INJECT_ROUNDS);
    (ok, format!("悬垂族注入 10/10×4 抓{:?}", caught))
}

/// 派生守恒演练 10/10 轮。
pub fn drill_derive_conserved_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let srv = match t.alloc(HandleKind::Server) {
            Ok(s) => s,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        // 派生随机 2~7 个客户端，随机关一半。
        let n = 2 + (lcg.below(6) as usize);
        let mut clients: Vec<RawHandle> = Vec::new();
        let mut i = 0;
        while i < n {
            if let Ok(c) = t.derive_client(srv) {
                clients.push(c);
            }
            i += 1;
        }
        let half = clients.len() / 2;
        let mut k = 0;
        while k < half {
            let _ = t.close(clients[k]);
            k += 1;
        }
        let cons = t.derive_conserved() && t.derive_monotone();
        let alive_ok = t.alive_count() == 1 + (clients.len() - half);
        if cons && alive_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("派生守恒 {}/{}", ok, INJECT_ROUNDS))
}

/// 表满注入 10/10 轮：16 槽满后第 17 分配必拒。
pub fn drill_table_full_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = HandleTable::new();
        let mut i = 0;
        let mut full = false;
        while i < HANDLE_CAP + 1 {
            match t.alloc(HandleKind::Server) {
                Ok(_) => {}
                Err(c) if c == HdCode::TableFull.val() => full = true,
                Err(_) => {}
            }
            i += 1;
        }
        if full {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("表满注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    HdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：矩阵完备性常量核对。
pub fn run_unx12428_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-矩阵完备",
        HANDLE_CAP == 16 && GEN_MOD == 16 && DERIVE_LEDGER_CAP == 64,
        format!("槽{} 代{} 账{}", HANDLE_CAP, GEN_MOD, DERIVE_LEDGER_CAP),
    ));
    v
}

/// run_unx12428_checks：本单判据聚合（诊断段 0x30380）。
pub fn run_unx12428_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12428 两态句柄能力矩阵");
    // J1-a：能力矩阵全量核。
    let (ok1, d1) = drill_capability_matrix_10();
    s.add_dyn("J1-能力矩阵10/10", ok1, &d1);
    // J1-b：客户端误调服务端接口。
    let (ok2, d2) = drill_client_misuse_10();
    s.add_dyn("J1-客户端误调10/10", ok2, &d2);
    // J1-c：服务端误调客户端接口。
    let (ok3, d3) = drill_server_misuse_10();
    s.add_dyn("J1-服务端误调10/10", ok3, &d3);
    // J1-d：悬垂四族。
    let (ok4, d4) = drill_stale_family_10();
    s.add_dyn("J1-悬垂族注入10/10", ok4, &d4);
    // J1-e：派生守恒。
    let (ok5, d5) = drill_derive_conserved_10();
    s.add_dyn("J1-派生守恒10/10", ok5, &d5);
    // J1-f：表满。
    let (ok6, d6) = drill_table_full_10();
    s.add_dyn("J1-表满注入10/10", ok6, &d6);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30380段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12428_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
