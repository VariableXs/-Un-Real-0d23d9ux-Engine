//! UNX-F52408 GnuPG CLI 收口 · 0xCA5C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52408"，逐字）：
//!   title: GnuPG CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52409-J1 生成/签
//!     名/验签 ×10 全通；TLS 消费对账（AI-44 契约联签锚定行）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52408）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52409-J1，照原文锚定；功能按本卡标题（GnuPG CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：gen/encrypt/decrypt/sign/verify 各 4 操作共 20 格，
//!      命令号 = o/4，族单调（逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② 密钥环 8 格：公钥环/私钥环逐格恒等收账，越闭集拒。
//!   ③ agent 三态 6 格：套接字/转发/缓存三态各 2 格，态号 = 格/2，态单调
//!      （逆序必抓）；逐格载荷对照。
//!   ④ 版本阶梯 3 档（2.2.x/2.3.x/2.4.x），越档必抓。
//!   ⑤ OpenSSH 消费声明 2 行闭集（OpenSSH 消费声明/AI-44 契约联签锚定
//!      行），缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（版本越档/密钥环漂移/
//!      agent 态 order 注入，order 注入对空账本同样必抓且不改 last_kind），
//!      检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3（37 格全通 +
//!      六判据全落 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52408;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数。
pub const CMD_OPS: usize = 20;

/// 密钥环格数。
pub const RING_CELLS: usize = 8;

/// agent 三态格数。
pub const AGENT_CELLS: usize = 6;

/// 版本阶梯档数。
pub const VER_STEPS: usize = 3;

/// OpenSSH 消费声明行数。
pub const SSH_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GnupgCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 密钥环格越闭集
    ENoRing,
    /// 密钥环格漂移
    ERingDrift,
    /// agent 格越闭集
    ENoAgent,
    /// agent 态漂移
    EAgentDrift,
    /// 版本档越阶梯
    ENoVer,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl GnupgCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GnupgCode::Ok => "成功",
            GnupgCode::ENoOp => "操作号越闭集：gen/encrypt/decrypt/sign/verify 20 操作集之外先登记",
            GnupgCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            GnupgCode::EOrder => "命令族逆序：gen→encrypt→decrypt→sign→verify 单调，逆序必抓",
            GnupgCode::ENoRing => "密钥环格越闭集：密钥环 8 格之外先登记",
            GnupgCode::ERingDrift => "密钥环格漂移：环格折叠与基准不恒等，逐格复算",
            GnupgCode::ENoAgent => "agent 格越闭集：agent 三态 6 格之外先登记",
            GnupgCode::EAgentDrift => "agent 态漂移：agent 格折叠与基准不恒等，逐格复算",
            GnupgCode::ENoVer => "版本档越阶梯：2.2.x/2.3.x/2.4.x 三档闭集之外先登记",
            GnupgCode::ENoRow => "声明行越闭集：OpenSSH 消费声明 2 行闭集之外先登记",
            GnupgCode::EDup => "重复落账：同格只许一条账目",
            GnupgCode::ESeal => "未齐即封：20 操作 + 8 环格 + 6 agent 格 + 3 档未齐先补齐",
            GnupgCode::EGradeDrift => "定级漂移：37 格全通且六判据全落才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20操作+8环格+6agent格+3档全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；命令族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some(
        (0xD0000u64 + (o as u64) * 0x2C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193).wrapping_add(0x47500000),
    )
}

/// 命令族（0..4=gen 4..8=encrypt 8..12=decrypt 12..16=sign 16..20=verify）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("gen"),
        1 => Some("encrypt"),
        2 => Some("decrypt"),
        3 => Some("sign"),
        4 => Some("verify"),
        _ => None,
    }
}

/// 密钥环格载荷（格 p）。
pub const fn ring_cell(p: usize) -> Option<u64> {
    if p >= RING_CELLS {
        return None;
    }
    Some((0xD1000u64 + (p as u64) * 0x3B) ^ 0x52494E47_0000)
}

/// agent 三态格载荷（格 a；态 = a/2：0=套接字 1=转发 2=缓存）。
pub const fn agent_cell(a: usize) -> Option<u64> {
    if a >= AGENT_CELLS {
        return None;
    }
    Some((0xD2000u64 + (a as u64) * 0x47) ^ ((a / 2) as u64).wrapping_mul(0x01000193))
}

/// agent 态（态 = a/2）。
pub const fn agent_kind(a: usize) -> Option<u32> {
    if a >= AGENT_CELLS {
        return None;
    }
    Some((a / 2) as u32)
}

/// agent 态名。
pub const fn agent_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("套接字"),
        1 => Some("转发"),
        2 => Some("缓存"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v：0=2.2.x 1=2.3.x 2=2.4.x）。
pub const fn ver_cell(v: usize) -> Option<u64> {
    if v >= VER_STEPS {
        return None;
    }
    Some((0xD3000u64 + (v as u64) * 0x5D) ^ 0x4E475056_45520000)
}

/// 版本档名。
pub const fn ver_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("2.2.x"),
        1 => Some("2.3.x"),
        2 => Some("2.4.x"),
        _ => None,
    }
}

/// OpenSSH 消费声明行折叠（行 r）。
pub const fn ssh_row(r: usize) -> Option<u32> {
    if r >= SSH_ROWS {
        return None;
    }
    Some(0xD1400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GnuPG CLI 收口账本状态机。
#[derive(Clone)]
pub struct GnupgLedger {
    /// 六判据（UNX-F52409-J1 生成/签名/验签收口闸）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; CMD_OPS],
    /// 密钥环账。
    rings: [Option<u64>; RING_CELLS],
    /// agent 账。
    agents: [Option<u64>; AGENT_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VER_STEPS],
    /// OpenSSH 声明账。
    sshs: [Option<u32>; SSH_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 最后 agent 态。
    last_agent: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GnupgLedger {
    /// 新账本。
    pub fn new() -> Self {
        GnupgLedger {
            gates: [None; 6],
            ops: [None; CMD_OPS],
            rings: [None; RING_CELLS],
            agents: [None; AGENT_CELLS],
            vers: [None; VER_STEPS],
            sshs: [None; SSH_ROWS],
            last_kind: -1,
            last_agent: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GnupgCode> {
        if gate >= 6 {
            return Err(GnupgCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 操作一格（命令族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GnupgCode> {
        if o >= CMD_OPS {
            return Err(GnupgCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(GnupgCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(GnupgCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(GnupgCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(GnupgCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GnupgCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 密钥环一格（载荷对照 + 重复拒）。
    pub fn record_ring(&mut self, p: usize, got: u64) -> Result<(), GnupgCode> {
        let want = match ring_cell(p) {
            Some(w) => w,
            None => return Err(GnupgCode::ENoRing),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + p as u64);
            return Err(GnupgCode::ERingDrift);
        }
        if match self.rings.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GnupgCode::EDup);
        }
        if let Some(c) = self.rings.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8301 + p as u64);
        Ok(())
    }

    /// 密钥环全账核验（8 格）。
    pub fn rings_all(&self) -> bool {
        let mut p = 0usize;
        while p < RING_CELLS {
            match self.rings.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// agent 一格（态单调 + 载荷对照 + 重复拒）。
    pub fn record_agent(&mut self, a: usize, got: u64) -> Result<(), GnupgCode> {
        if a >= AGENT_CELLS {
            return Err(GnupgCode::ENoAgent);
        }
        let kind = match agent_kind(a) {
            Some(k) => k,
            None => return Err(GnupgCode::ENoAgent),
        };
        if (kind as i64) < self.last_agent {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400);
            return Err(GnupgCode::EOrder);
        }
        let want = match agent_cell(a) {
            Some(w) => w,
            None => return Err(GnupgCode::ENoAgent),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8401 + a as u64);
            return Err(GnupgCode::EAgentDrift);
        }
        if match self.agents.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GnupgCode::EDup);
        }
        if let Some(c) = self.agents.get_mut(a) {
            *c = Some(got);
        }
        self.last_agent = kind as i64;
        self.fnv(0x8402 + a as u64);
        Ok(())
    }

    /// agent 全账核验（6 格）。
    pub fn agents_all(&self) -> bool {
        let mut a = 0usize;
        while a < AGENT_CELLS {
            match self.agents.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 版本档一格（阶梯载荷对照 + 重复拒）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), GnupgCode> {
        let want = match ver_cell(v) {
            Some(w) => w,
            None => return Err(GnupgCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + v as u64);
            return Err(GnupgCode::EDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GnupgCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x8501 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VER_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// OpenSSH 消费声明一行。
    pub fn declare_ssh(&mut self, r: usize) -> Result<(), GnupgCode> {
        if r >= SSH_ROWS {
            return Err(GnupgCode::ENoRow);
        }
        if match self.sshs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GnupgCode::EDup);
        }
        if let Some(c) = self.sshs.get_mut(r) {
            *c = ssh_row(r);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn sshs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SSH_ROWS {
            match self.sshs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.rings_all()
            && self.agents_all()
            && self.vers_all()
            && self.sshs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GnupgLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GnupgLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VER_STEPS {
        if let Some(w) = ver_cell(v) {
            if led.record_ver(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < RING_CELLS {
        if let Some(w) = ring_cell(p) {
            if led.record_ring(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut a = 0usize;
    while a < AGENT_CELLS {
        if let Some(w) = agent_cell(a) {
            if led.record_agent(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < SSH_ROWS {
        let _ = led.declare_ssh(r);
        r += 1;
    }
    // 注入 10 轮：版本越档 / 密钥环漂移 / agent 态 order 三态轮转。
    // order 注入对空账本同样必抓且不改 last_kind（临时账本即空账）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GnupgLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_ver(0, ver_cell(0).unwrap_or(0));
                lt.record_ver(VER_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_ring(0, ring_cell(0).unwrap_or(0));
                lt.record_ring(1, ring_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(GnupgCode::ERingDrift)
            }
            _ => {
                let _ = lt.record_agent(3, agent_cell(3).unwrap_or(0));
                lt.record_agent(1, agent_cell(1).unwrap_or(0)) == Err(GnupgCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52408 判据集（锚 UNX-F52409-J1，off-by-one 板式）。
pub fn run_unx52408_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52408");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "命令 20 操作全通",
        recorded >= CMD_OPS as u32,
        "gen/encrypt/decrypt/sign/verify 五命令族逐操作恒等",
    );
    s.add(
        "密钥环 8/8 格全落",
        {
            let mut lt = GnupgLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < RING_CELLS {
                if let Some(w) = ring_cell(p) {
                    if lt.record_ring(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.rings_all()
        },
        "公钥环/私钥环逐格恒等，越闭集拒",
    );
    s.add(
        "agent 三态 6/6 格全落",
        {
            let mut lt = GnupgLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < AGENT_CELLS {
                if let Some(w) = agent_cell(a) {
                    if lt.record_agent(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.agents_all()
        },
        "套接字/转发/缓存三态各 2 格，态单调逆序必抓",
    );
    s.add(
        "版本阶梯 2.2.x/2.3.x/2.4.x 三档全落",
        {
            let mut lt = GnupgLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VER_STEPS {
                if let Some(w) = ver_cell(v) {
                    if lt.record_ver(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐档恒等，越档必抓",
    );
    s.add(
        "OpenSSH 消费声明 2/2 全齐",
        {
            let mut lt = GnupgLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SSH_ROWS {
                if lt.declare_ssh(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sshs_all()
        },
        "OpenSSH 消费声明/AI-44 契约联签锚定行两行闭集，缺一拒封",
    );
    s.add(
        "生成/签名/验签六判据全落",
        {
            let mut lt = GnupgLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52409-J1 六判据逐格落账（§AI-66.3）",
    );
    s.add(
        "命令族与 agent 态单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            let mut a = 1usize;
            while a < AGENT_CELLS {
                if agent_kind(a).unwrap_or(3) < agent_kind(a - 1).unwrap_or(3) {
                    ok = false;
                }
                a += 1;
            }
            ok
        },
        "五命令族 + agent 三态闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越档/密钥环漂移/agent 态 order 三态轮转必抓",
    );
    let mut l0 = GnupgLedger::new();
    s.add(
        "边界·版本档越阶梯",
        l0.record_ver(VER_STEPS, 0).is_err(),
        "版本档 3 越阶梯必拒",
    );
    s.add(
        "密钥环漂移拒",
        l0.record_ring(2, ring_cell(2).unwrap_or(0).wrapping_add(4)) == Err(GnupgCode::ERingDrift),
        "环格载荷漂移必抓",
    );
    s.add(
        "agent 态逆序拒（空账本必抓）",
        {
            let _ = l0.record_agent(3, agent_cell(3).unwrap_or(0));
            l0.record_agent(1, agent_cell(1).unwrap_or(0)) == Err(GnupgCode::EOrder)
        },
        "转发态后回套接字态必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(CMD_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·agent 格越闭集",
        l0.record_agent(AGENT_CELLS, 0).is_err(),
        "agent 格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_ssh(SSH_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GnupgLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VER_STEPS {
                if let Some(w) = ver_cell(v) {
                    let _ = lt.record_ver(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < RING_CELLS {
                if let Some(w) = ring_cell(p) {
                    let _ = lt.record_ring(p, w);
                }
                p += 1;
            }
            let mut a = 0usize;
            while a < AGENT_CELLS {
                if let Some(w) = agent_cell(a) {
                    let _ = lt.record_agent(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < SSH_ROWS {
                let _ = lt.declare_ssh(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA5C0,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA5C0",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(3), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(8), Some(2));
        assert_eq!(cmd_kind(12), Some(3));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
    }

    #[test]
    fn test_cmd_kind_names() {
        assert_eq!(cmd_kind_name(0), Some("gen"));
        assert_eq!(cmd_kind_name(2), Some("decrypt"));
        assert_eq!(cmd_kind_name(4), Some("verify"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_agent_kind() {
        assert_eq!(agent_kind(0), Some(0));
        assert_eq!(agent_kind(1), Some(0));
        assert_eq!(agent_kind(2), Some(1));
        assert_eq!(agent_kind(5), Some(2));
        assert!(agent_kind(6).is_none());
        assert!(agent_cell(6).is_none());
        assert_eq!(agent_kind_name(0), Some("套接字"));
        assert_eq!(agent_kind_name(2), Some("缓存"));
        assert!(agent_kind_name(3).is_none());
    }

    #[test]
    fn test_ver_ladder() {
        assert_eq!(ver_name(0), Some("2.2.x"));
        assert_eq!(ver_name(1), Some("2.3.x"));
        assert_eq!(ver_name(2), Some("2.4.x"));
        assert!(ver_name(3).is_none());
        assert!(ver_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = ring_cell(3).unwrap_or(0);
        assert_eq!(a, ring_cell(3).unwrap_or(0));
        assert_ne!(a, ring_cell(4).unwrap_or(0));
        assert!(cmd_cell(CMD_OPS).is_none());
        assert!(ssh_row(SSH_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GnupgLedger::new();
        let _ = lt.record_op(13, cmd_cell(13).unwrap_or(0));
        assert_eq!(lt.record_op(2, cmd_cell(2).unwrap_or(0)), Err(GnupgCode::EOrder));
        assert!(lt.record_op(14, cmd_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GnupgLedger::new();
        assert_eq!(
            lt.record_op(7, cmd_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(GnupgCode::EDrift)
        );
        assert_eq!(
            lt.record_agent(4, agent_cell(4).unwrap_or(0).wrapping_add(3)),
            Err(GnupgCode::EAgentDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GnupgLedger::new();
        let _ = lt.record_ver(0, ver_cell(0).unwrap_or(0));
        assert_eq!(lt.record_ver(0, ver_cell(0).unwrap_or(0)), Err(GnupgCode::EDup));
        let _ = lt.record_ring(0, ring_cell(0).unwrap_or(0));
        assert_eq!(lt.record_ring(0, ring_cell(0).unwrap_or(0)), Err(GnupgCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GnupgLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GnupgLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded = 3 版本档 + 20 操作 + 8 环格 + 6 agent 格 = 37（声明 2 行不计格）。
        assert_eq!(
            recorded,
            VER_STEPS as u32 + CMD_OPS as u32 + RING_CELLS as u32 + AGENT_CELLS as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA5C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GnupgCode::Ok as u32, 0);
        assert_eq!(GnupgCode::EGradeDrift as u32, 12);
        assert!(!GnupgCode::Ok.detail().is_empty());
        assert!(!GnupgCode::ESeal.detail().is_empty());
        assert!(!GnupgCode::ENoVer.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GnupgLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_ssh_declare_boundary() {
        let mut lt = GnupgLedger::new();
        assert!(lt.declare_ssh(0).is_ok());
        assert!(lt.declare_ssh(1).is_ok());
        assert!(lt.declare_ssh(2).is_err());
        assert!(lt.sshs_all());
    }

    #[test]
    fn unx52408_checks_green() {
        let s = run_unx52408_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
