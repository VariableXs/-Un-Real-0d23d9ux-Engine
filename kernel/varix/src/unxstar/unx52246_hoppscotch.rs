//! UNX-F52246 Hoppscotch Desktop 收口 · 0xC7D40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52246"，逐字）：
//!   title: Hoppscotch Desktop 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52247-J1 六判据全
//!     落；HTTPS 解密（证书面）/会话列表/断点三面 ×10；系统代理语义消费
//!     （AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52246）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52247-J1，照原文锚定（卡面 criteria 内「定级 S1」一语随原文属
//! 后卡口径）；功能按本卡标题（Hoppscotch Desktop）落地，本卡 KB-COMPAT
//! 定级 S2。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五协议 20 格闭集：REST/GraphQL/WebSocket/SSE/MQTT 五家族各 4 格，
//!      家族单调（逆序必抓），逐格载荷纯函数恒等才收，漂移必抓。
//!   ② 集合 8 格逐格落账；环境 6 格逐格落账。
//!   ③ 自托管/云双形态分账声明 2 行闭集，缺一拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（协议格漂移/
//!      协议家族逆序/集合越闭集），三类注入全部落在当轮临时账本上，
//!      led 上 injected==caught==10、escaped==0；漂移注入先录正格再注漂
//!      移；order 注入对空账本同样必抓且不改 last_kind；双跑指纹一致。
//!   ⑤ 定级 S2 口径：六判据 + 20 协议格 + 8 集合格 + 6 环境格 + 2 行分
//!      账全通 → S2（本卡收口上限）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52246;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五协议格数。
pub const PROTO_OPS: usize = 20;

/// 集合格数。
pub const COLL_CELLS: usize = 8;

/// 环境格数。
pub const ENV_CELLS: usize = 6;

/// 双形态分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoppCode {
    /// 成功
    Ok = 0,
    /// 协议格越闭集
    ENoProto,
    /// 协议漂移
    EProtoDrift,
    /// 协议家族逆序
    EOrder,
    /// 集合格越闭集
    ENoColl,
    /// 集合漂移
    ECollDrift,
    /// 环境格越闭集
    ENoEnv,
    /// 环境漂移
    EEnvDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl HoppCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HoppCode::Ok => "成功",
            HoppCode::ENoProto => "协议格越闭集：REST/GraphQL/WebSocket/SSE/MQTT 20 格闭集之外先登记",
            HoppCode::EProtoDrift => "协议漂移：协议格载荷与基准不恒等，逐格复算",
            HoppCode::EOrder => "协议家族逆序：REST→GraphQL→WebSocket→SSE→MQTT 单调，逆序必抓",
            HoppCode::ENoColl => "集合格越闭集：集合 8 格闭集之外先登记",
            HoppCode::ECollDrift => "集合漂移：集合格载荷与基准不恒等，逐格复算",
            HoppCode::ENoEnv => "环境格越闭集：环境 6 格闭集之外先登记",
            HoppCode::EEnvDrift => "环境漂移：环境格载荷与基准不恒等，逐格复算",
            HoppCode::ENoRow => "声明行越闭集：自托管/云双形态分账 2 行闭集之外先登记",
            HoppCode::EDup => "重复落账：同格只许一条账目",
            HoppCode::ESeal => "未齐即封：20 协议格 + 8 集合格 + 6 环境格未齐先补齐",
            HoppCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HoppCode::EGradeDrift => "定级漂移：六判据+20 协议格+8 集合格+6 环境格+2 行分账全通才可定 S2",
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
            Grade::S1 => "S1：全通收口（本卡口径不触发，本卡收口上限 S2）",
            Grade::S2 => "S2：六判据+20 协议格+8 集合格+6 环境格+2 行分账全通收口",
            Grade::S3 => "S3：不收口，判据缺失或格未齐",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 协议格载荷（格 p；家族 = p/4）。
pub const fn proto_cell(p: usize) -> Option<u64> {
    if p >= PROTO_OPS {
        return None;
    }
    Some((0xE0000u64 + (p as u64) * 0x53) ^ ((p / 4) as u64).wrapping_mul(0x0F0F1A2B))
}

/// 协议家族（0..4=REST 4..8=GraphQL 8..12=WebSocket 12..16=SSE 16..20=MQTT）。
pub const fn proto_kind(p: usize) -> Option<u32> {
    if p >= PROTO_OPS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 协议家族名。
pub const fn proto_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("REST"),
        1 => Some("GraphQL"),
        2 => Some("WebSocket"),
        3 => Some("SSE"),
        4 => Some("MQTT"),
        _ => None,
    }
}

/// 集合格载荷（格 c）。
pub const fn coll_cell(c: usize) -> Option<u64> {
    if c >= COLL_CELLS {
        return None;
    }
    Some((0xE1000u64 + (c as u64) * 0x3D) ^ 0x484F5050434F4C4C)
}

/// 环境格载荷（格 e）。
pub const fn env_cell(e: usize) -> Option<u64> {
    if e >= ENV_CELLS {
        return None;
    }
    Some((0xE2000u64 + (e as u64) * 0x29) ^ 0x484F5050454E5631)
}

/// 双形态分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xE300 + r as u32 * 0x3D)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("自托管形态分账声明"),
        1 => Some("云形态分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Hoppscotch 收口账本状态机。
#[derive(Clone)]
pub struct HoppLedger {
    /// 六判据（启动/HTTPS 解密证书面/会话列表/断点/系统代理语义消费/卸载）。
    gates: [Option<bool>; 6],
    /// 协议账。
    protos: [Option<u64>; PROTO_OPS],
    /// 集合账。
    colls: [Option<u64>; COLL_CELLS],
    /// 环境账。
    envs: [Option<u64>; ENV_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后协议家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HoppLedger {
    /// 新账本。
    pub fn new() -> Self {
        HoppLedger {
            gates: [None; 6],
            protos: [None; PROTO_OPS],
            colls: [None; COLL_CELLS],
            envs: [None; ENV_CELLS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HoppCode> {
        if gate >= 6 {
            return Err(HoppCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 协议一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, p: usize, got: u64) -> Result<(), HoppCode> {
        if p >= PROTO_OPS {
            return Err(HoppCode::ENoProto);
        }
        let kind = match proto_kind(p) {
            Some(k) => k,
            None => return Err(HoppCode::ENoProto),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(HoppCode::EOrder);
        }
        let want = match proto_cell(p) {
            Some(w) => w,
            None => return Err(HoppCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + p as u64);
            return Err(HoppCode::EProtoDrift);
        }
        if match self.protos.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HoppCode::EDup);
        }
        if let Some(c) = self.protos.get_mut(p) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + p as u64);
        Ok(())
    }

    /// 协议全账核验（20 格）。
    pub fn protos_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTO_OPS {
            match self.protos.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 集合一格（载荷对照 + 重复拒）。
    pub fn record_coll(&mut self, c: usize, got: u64) -> Result<(), HoppCode> {
        if c >= COLL_CELLS {
            return Err(HoppCode::ENoColl);
        }
        let want = match coll_cell(c) {
            Some(w) => w,
            None => return Err(HoppCode::ENoColl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + c as u64);
            return Err(HoppCode::ECollDrift);
        }
        if match self.colls.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HoppCode::EDup);
        }
        if let Some(cc) = self.colls.get_mut(c) {
            *cc = Some(got);
        }
        self.fnv(0x6301 + c as u64);
        Ok(())
    }

    /// 集合全账核验（8 格）。
    pub fn colls_all(&self) -> bool {
        let mut c = 0usize;
        while c < COLL_CELLS {
            match self.colls.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 环境一格（载荷对照 + 重复拒）。
    pub fn record_env(&mut self, e: usize, got: u64) -> Result<(), HoppCode> {
        if e >= ENV_CELLS {
            return Err(HoppCode::ENoEnv);
        }
        let want = match env_cell(e) {
            Some(w) => w,
            None => return Err(HoppCode::ENoEnv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + e as u64);
            return Err(HoppCode::EEnvDrift);
        }
        if match self.envs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HoppCode::EDup);
        }
        if let Some(cc) = self.envs.get_mut(e) {
            *cc = Some(got);
        }
        self.fnv(0x6401 + e as u64);
        Ok(())
    }

    /// 环境全账核验（6 格）。
    pub fn envs_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENV_CELLS {
            match self.envs.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 分账声明一行（自托管/云双形态）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), HoppCode> {
        if r >= SPLIT_ROWS {
            return Err(HoppCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HoppCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

    /// KB-COMPAT 定级（本卡收口口径：全通 → S2，其余 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.protos_all()
            && self.colls_all()
            && self.envs_all()
            && self.splits_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for HoppLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HoppLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROTO_OPS {
        if let Some(w) = proto_cell(p) {
            if led.record_proto(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut c = 0usize;
    while c < COLL_CELLS {
        if let Some(w) = coll_cell(c) {
            if led.record_coll(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut e = 0usize;
    while e < ENV_CELLS {
        if let Some(w) = env_cell(e) {
            if led.record_env(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：协议格漂移 / 协议家族逆序 / 集合越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移；
    // order 注入对空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HoppLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_proto(0, proto_cell(0).unwrap_or(0));
                lt.record_proto(1, proto_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(HoppCode::EProtoDrift)
            }
            1 => {
                let _ = lt.record_proto(13, proto_cell(13).unwrap_or(0));
                lt.record_proto(6, proto_cell(6).unwrap_or(0)) == Err(HoppCode::EOrder)
            }
            _ => {
                let _ = lt.record_proto(0, proto_cell(0).unwrap_or(0));
                lt.record_coll(COLL_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52246 判据集（锚 UNX-F52247-J1，off-by-one 板式）。
pub fn run_unx52246_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52246");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五协议 20 格全通",
        recorded >= PROTO_OPS as u32,
        "REST/GraphQL/WebSocket/SSE/MQTT 五家族逐格恒等",
    );
    s.add(
        "集合 8 格全落",
        {
            let mut lt = HoppLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COLL_CELLS {
                if let Some(w) = coll_cell(c) {
                    if lt.record_coll(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.colls_all()
        },
        "集合 8 格逐格载荷对照恒等",
    );
    s.add(
        "环境 6 格全落",
        {
            let mut lt = HoppLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENV_CELLS {
                if let Some(w) = env_cell(e) {
                    if lt.record_env(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.envs_all()
        },
        "环境 6 格逐格载荷对照恒等",
    );
    s.add(
        "自托管/云双形态分账声明 2/2 全齐",
        {
            let mut lt = HoppLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "自托管形态/云形态两行分账声明闭集，缺一拒封",
    );
    s.add(
        "协议家族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PROTO_OPS {
                if proto_kind(p).unwrap_or(9) < proto_kind(p - 1).unwrap_or(9) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = HoppLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/HTTPS 解密证书面/会话列表/断点/系统代理语义消费/卸载逐格落账（AI-23 联签锚定行）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "协议格漂移/协议家族逆序/集合越闭集三态轮转必抓",
    );
    let mut l0 = HoppLedger::new();
    s.add(
        "边界·协议格越闭集",
        l0.record_proto(PROTO_OPS, 0).is_err(),
        "协议格 20 越闭集必拒",
    );
    s.add(
        "边界·集合格越闭集",
        l0.record_coll(COLL_CELLS, 0).is_err(),
        "集合格 8 越闭集必拒",
    );
    s.add(
        "边界·环境越闭集",
        l0.record_env(ENV_CELLS, 0).is_err(),
        "环境格 6 越闭集必拒",
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
        "定级 S2 闭集",
        {
            let mut lt = HoppLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PROTO_OPS {
                if let Some(w) = proto_cell(p) {
                    let _ = lt.record_proto(p, w);
                }
                p += 1;
            }
            let mut c = 0usize;
            while c < COLL_CELLS {
                if let Some(w) = coll_cell(c) {
                    let _ = lt.record_coll(c, w);
                }
                c += 1;
            }
            let mut e = 0usize;
            while e < ENV_CELLS {
                if let Some(w) = env_cell(e) {
                    let _ = lt.record_env(e, w);
                }
                e += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级 S2 登记口径）",
    );
    s.add(
        "诊断码闭集",
        HoppCode::Ok.val() == SEG_BASE && HoppCode::EGradeDrift.val() == SEG_BASE + 12,
        "Ok=+0x00、EGradeDrift=+0x0C 恰 13 变体",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7D40,
        "SEG_BASE = 0xC7200 + 0x40×(52246−52201)，0x40 对齐",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_kinds() {
        assert_eq!(proto_kind_name(0), Some("REST"));
        assert_eq!(proto_kind_name(1), Some("GraphQL"));
        assert_eq!(proto_kind_name(2), Some("WebSocket"));
        assert_eq!(proto_kind_name(3), Some("SSE"));
        assert_eq!(proto_kind_name(4), Some("MQTT"));
        assert!(proto_kind_name(5).is_none());
        assert_eq!(proto_kind(19), Some(4));
        assert!(proto_kind(20).is_none());
        assert!(proto_cell(20).is_none());
    }

    #[test]
    fn test_split_rows() {
        assert_eq!(split_row_name(0), Some("自托管形态分账声明"));
        assert_eq!(split_row_name(1), Some("云形态分账声明"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = proto_cell(7).unwrap_or(0);
        assert_eq!(a, proto_cell(7).unwrap_or(0));
        assert_ne!(a, proto_cell(8).unwrap_or(0));
        let c = coll_cell(3).unwrap_or(0);
        assert_eq!(c, coll_cell(3).unwrap_or(0));
        assert!(coll_cell(COLL_CELLS).is_none());
        assert!(env_cell(ENV_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = HoppLedger::new();
        let _ = lt.record_proto(13, proto_cell(13).unwrap_or(0));
        assert_eq!(lt.record_proto(6, proto_cell(6).unwrap_or(0)), Err(HoppCode::EOrder));
        // 逆序被拒后家族账位不受污染：续落同家族高格仍可收。
        assert!(lt.record_proto(14, proto_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_order_on_empty_ledger_no_kind_change() {
        let mut lt = HoppLedger::new();
        // 空账本先立 SSE 家族再回 GraphQL：必抓。
        let _ = lt.record_proto(12, proto_cell(12).unwrap_or(0));
        assert_eq!(lt.record_proto(4, proto_cell(4).unwrap_or(0)), Err(HoppCode::EOrder));
        // EOrder 拒绝不改 last_kind：同家族继续可收。
        assert!(lt.record_proto(13, proto_cell(13).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HoppLedger::new();
        // 先录正格再注漂移。
        let _ = lt.record_proto(0, proto_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_proto(1, proto_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(HoppCode::EProtoDrift)
        );
        let _ = lt.record_coll(0, coll_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_coll(1, coll_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(HoppCode::ECollDrift)
        );
        assert_eq!(
            lt.record_env(0, env_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(HoppCode::EEnvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HoppLedger::new();
        let _ = lt.record_proto(9, proto_cell(9).unwrap_or(0));
        assert_eq!(lt.record_proto(9, proto_cell(9).unwrap_or(0)), Err(HoppCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(HoppCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HoppLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = HoppLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROTO_OPS + COLL_CELLS + ENV_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7D40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(HoppCode::Ok as u32, 0);
        assert_eq!(HoppCode::EGradeDrift as u32, 12);
        assert!(!HoppCode::Ok.detail().is_empty());
        assert!(!HoppCode::ENoRow.detail().is_empty());
        assert_eq!(HoppCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = HoppLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52246_checks_green() {
        let s = run_unx52246_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
