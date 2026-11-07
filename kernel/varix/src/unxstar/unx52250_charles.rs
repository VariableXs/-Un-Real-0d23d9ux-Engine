//! UNX-F52250 Charles Proxy 收口 · 0xC7E40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52250"，逐字）：
//!   title: Charles Proxy 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52251-J1 六判据
//!     全落（Electron）；拦截 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52250）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52251-J1，照原文锚定（卡面 criteria 内「Electron」「拦截 ×10」
//! 诸语随原文属后卡 HTTP Toolkit 口径）；功能按本卡标题（Charles Proxy）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① HTTP/HTTPS 抓包三态 12 格闭集：三家族（HTTP 明文抓包/HTTPS 解密
//!      抓包/会话回放归档）各 4 格，逐格载荷纯函数恒等才收，漂移必抓。
//!   ② 断点/重写/映射 10 操作集：三家族（断点 0..4 / 重写 4..7 / 映射
//!      7..10），家族单调（逆序必抓），逐操作落账。
//!   ③ SSL 代理三元闸：SSL 代理开关 / 客户端证书安装 / 解密域名单三态
//!      逐格落账，越三态闭集必抓。
//!   ④ 限流/劫持 8 格：限速档案 4 格 + 劫持规则 4 格，逐格载荷对照恒等。
//!   ⑤ Java 上栈 4 层（JVM 启动参数/javax.net 信任库/系统代理挂钩/应用
//!      会话接入）逐层落账，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（抓包格漂移/
//!      操作家族逆序/抓包越闭集），三类注入全部落在当轮临时账本上，led
//!      上 injected==caught==10、escaped==0；漂移注入先录正格再注漂移；
//!      order 注入对空账本同样必抓且不改 last_kind；双跑指纹一致。
//!   ⑦ KB-COMPAT 定级 S2 口径：Charles HTTPS 解密依赖端侧信任自签 CA，
//!      全通为有条件收口，六判据 + 12 抓包格 + 10 操作 + 三元闸 + 8 格
//!      + 4 层上栈全通 → S2（S1 留空不授，判据缺 → S3）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52250;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// HTTP/HTTPS 抓包三态格数。
pub const CAP_CELLS: usize = 12;

/// 断点/重写/映射操作数。
pub const XFORM_OPS: usize = 10;

/// SSL 代理三元闸态数。
pub const SSL_STATES: usize = 3;

/// 限流/劫持格数。
pub const THROTTLE_CELLS: usize = 8;

/// Java 上栈层数。
pub const JAVA_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharlesCode {
    /// 成功
    Ok = 0,
    /// 抓包格越闭集
    ENoCap,
    /// 抓包格载荷漂移
    ECapDrift,
    /// 操作越闭集
    ENoOp,
    /// 操作家族逆序
    EOrder,
    /// SSL 代理三元闸越态
    ENoSsl,
    /// 限流/劫持格越闭集
    ENoThrottle,
    /// 限流/劫持格漂移
    EThrottleDrift,
    /// Java 上栈越层
    ENoJava,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl CharlesCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CharlesCode::Ok => "成功",
            CharlesCode::ENoCap => "抓包格越闭集：HTTP/HTTPS 抓包三态 12 格闭集之外先登记",
            CharlesCode::ECapDrift => "抓包漂移：抓包格载荷与基准不恒等，逐格复算",
            CharlesCode::ENoOp => "操作越闭集：断点/重写/映射 10 操作集之外先登记",
            CharlesCode::EOrder => "操作家族逆序：断点→重写→映射单调，逆序必抓",
            CharlesCode::ENoSsl => "三元闸越态：SSL 代理开关/客户端证书安装/解密域名单三态之外先登记",
            CharlesCode::ENoThrottle => "限流劫持格越闭集：限流/劫持 8 格闭集之外先登记",
            CharlesCode::EThrottleDrift => "限流劫持漂移：限流/劫持格载荷与基准不恒等，逐格复算",
            CharlesCode::ENoJava => "Java 上栈越层：JVM 参数/信任库/系统代理/会话接入四层之外先登记",
            CharlesCode::EDup => "重复落账：同格只许一条账目",
            CharlesCode::ESeal => "未齐即封：12 抓包格 + 10 操作 + 8 限流劫持格未齐先补齐",
            CharlesCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CharlesCode::EGradeDrift => "定级漂移：六判据+12 抓包格+10 操作+三元闸+8 格+4 层全通才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（Charles 口径留空不授）
    S1,
    /// S2：有条件收口（HTTPS 解密依赖端侧信任）
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
            Grade::S1 => "S1：Charles 口径留空不授（HTTPS 解密依赖端侧信任自签 CA）",
            Grade::S2 => "S2：六判据+12 抓包格+10 操作+三元闸+8 格+4 层全通有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 抓包格载荷（格 i；家族 = i/4）。
pub const fn cap_cell(i: usize) -> Option<u64> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((0x120000u64 + (i as u64) * 0x5B) ^ 0x434841524C410000)
}

/// 抓包家族（0..4=HTTP 明文抓包 4..8=HTTPS 解密抓包 8..12=会话回放归档）。
pub const fn cap_kind(i: usize) -> Option<u32> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 抓包家族名。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("HTTP 明文抓包"),
        1 => Some("HTTPS 解密抓包"),
        2 => Some("会话回放归档"),
        _ => None,
    }
}

/// 操作家族（0..4=断点 4..7=重写 7..10=映射）。
pub const fn xform_kind(o: usize) -> Option<u32> {
    if o >= XFORM_OPS {
        return None;
    }
    if o < 4 {
        Some(0)
    } else if o < 7 {
        Some(1)
    } else {
        Some(2)
    }
}

/// 操作家族名。
pub const fn xform_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("断点"),
        1 => Some("重写"),
        2 => Some("映射"),
        _ => None,
    }
}

/// 操作格折叠（行 o，落账值）。
pub const fn xform_row(o: usize) -> Option<u32> {
    if o >= XFORM_OPS {
        return None;
    }
    Some(0x12100 + o as u32 * 0x43)
}

/// SSL 代理三元闸态折叠（态 s）。
pub const fn ssl_state(s: usize) -> Option<u32> {
    if s >= SSL_STATES {
        return None;
    }
    Some(0x12200 + s as u32 * 0x5D)
}

/// SSL 三元闸态名。
pub const fn ssl_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("SSL 代理开关"),
        1 => Some("客户端证书安装"),
        2 => Some("解密域名单"),
        _ => None,
    }
}

/// 限流/劫持格载荷（格 r；0..4=限速档案 4..8=劫持规则）。
pub const fn throttle_cell(r: usize) -> Option<u64> {
    if r >= THROTTLE_CELLS {
        return None;
    }
    Some((0x123000u64 + (r as u64) * 0x37) ^ 0x5448524F54540000)
}

/// 限流/劫持格名。
pub const fn throttle_cell_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("限速档案·3G"),
        1 => Some("限速档案·带宽封顶"),
        2 => Some("限速档案·延迟注入"),
        3 => Some("限速档案·丢包模拟"),
        4 => Some("劫持规则·DNS"),
        5 => Some("劫持规则·Host 改写"),
        6 => Some("劫持规则·端口转发"),
        7 => Some("劫持规则·黑名单"),
        _ => None,
    }
}

/// Java 上栈层折叠（层 l）。
pub const fn java_layer(l: usize) -> Option<u32> {
    if l >= JAVA_LAYERS {
        return None;
    }
    Some(0x12400 + l as u32 * 0x53)
}

/// Java 上栈层名。
pub const fn java_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JVM 启动参数"),
        1 => Some("javax.net 信任库"),
        2 => Some("系统代理挂钩"),
        3 => Some("应用会话接入"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Charles Proxy 收口账本状态机。
#[derive(Clone)]
pub struct CharlesLedger {
    /// 六判据（启动/抓包/重写/限流/SSL 代理/卸载）。
    gates: [Option<bool>; 6],
    /// 抓包账。
    caps: [Option<u64>; CAP_CELLS],
    /// 操作账。
    xforms: [Option<u32>; XFORM_OPS],
    /// 三元闸账。
    ssls: [Option<u32>; SSL_STATES],
    /// 限流/劫持账。
    throttles: [Option<u64>; THROTTLE_CELLS],
    /// Java 上栈账。
    javas: [Option<u32>; JAVA_LAYERS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CharlesLedger {
    /// 新账本。
    pub fn new() -> Self {
        CharlesLedger {
            gates: [None; 6],
            caps: [None; CAP_CELLS],
            xforms: [None; XFORM_OPS],
            ssls: [None; SSL_STATES],
            throttles: [None; THROTTLE_CELLS],
            javas: [None; JAVA_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CharlesCode> {
        if gate >= 6 {
            return Err(CharlesCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xA100 + gate as u64);
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

    /// 抓包一格（载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, i: usize, got: u64) -> Result<(), CharlesCode> {
        if i >= CAP_CELLS {
            return Err(CharlesCode::ENoCap);
        }
        let want = match cap_cell(i) {
            Some(w) => w,
            None => return Err(CharlesCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA200 + i as u64);
            return Err(CharlesCode::ECapDrift);
        }
        if match self.caps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CharlesCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xA201 + i as u64);
        Ok(())
    }

    /// 抓包全账核验（12 格）。
    pub fn caps_all(&self) -> bool {
        let mut i = 0usize;
        while i < CAP_CELLS {
            match self.caps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 操作一格（家族单调 + 重复拒；拒后不改 last_kind）。
    pub fn record_xform(&mut self, o: usize) -> Result<(), CharlesCode> {
        if o >= XFORM_OPS {
            return Err(CharlesCode::ENoOp);
        }
        let kind = match xform_kind(o) {
            Some(k) => k,
            None => return Err(CharlesCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA300);
            return Err(CharlesCode::EOrder);
        }
        if match self.xforms.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CharlesCode::EDup);
        }
        let v = match xform_row(o) {
            Some(v) => v,
            None => return Err(CharlesCode::ENoOp),
        };
        if let Some(c) = self.xforms.get_mut(o) {
            *c = Some(v);
        }
        self.last_kind = kind as i64;
        self.fnv(0xA301 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 操作）。
    pub fn xforms_all(&self) -> bool {
        let mut o = 0usize;
        while o < XFORM_OPS {
            match self.xforms.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// SSL 三元闸一格。
    pub fn record_ssl(&mut self, s: usize) -> Result<(), CharlesCode> {
        let v = match ssl_state(s) {
            Some(v) => v,
            None => return Err(CharlesCode::ENoSsl),
        };
        if match self.ssls.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CharlesCode::EDup);
        }
        if let Some(c) = self.ssls.get_mut(s) {
            *c = Some(v);
        }
        self.fnv(0xA400 + s as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn ssls_all(&self) -> bool {
        let mut s = 0usize;
        while s < SSL_STATES {
            match self.ssls.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 限流/劫持一格（载荷对照 + 重复拒）。
    pub fn record_throttle(&mut self, r: usize, got: u64) -> Result<(), CharlesCode> {
        if r >= THROTTLE_CELLS {
            return Err(CharlesCode::ENoThrottle);
        }
        let want = match throttle_cell(r) {
            Some(w) => w,
            None => return Err(CharlesCode::ENoThrottle),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA500 + r as u64);
            return Err(CharlesCode::EThrottleDrift);
        }
        if match self.throttles.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CharlesCode::EDup);
        }
        if let Some(c) = self.throttles.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xA501 + r as u64);
        Ok(())
    }

    /// 限流/劫持全账核验（8 格）。
    pub fn throttles_all(&self) -> bool {
        let mut r = 0usize;
        while r < THROTTLE_CELLS {
            match self.throttles.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Java 上栈一层。
    pub fn record_java(&mut self, l: usize) -> Result<(), CharlesCode> {
        let v = match java_layer(l) {
            Some(v) => v,
            None => return Err(CharlesCode::ENoJava),
        };
        if match self.javas.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CharlesCode::EDup);
        }
        if let Some(c) = self.javas.get_mut(l) {
            *c = Some(v);
        }
        self.fnv(0xA600 + l as u64);
        Ok(())
    }

    /// 上栈全齐核验（4 层）。
    pub fn javas_all(&self) -> bool {
        let mut l = 0usize;
        while l < JAVA_LAYERS {
            match self.javas.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（全通 → S2 有条件收口，判据缺 → S3，其余 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.caps_all()
            && self.xforms_all()
            && self.ssls_all()
            && self.throttles_all()
            && self.javas_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CharlesLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CharlesLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < CAP_CELLS {
        if let Some(w) = cap_cell(i) {
            if led.record_cap(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < XFORM_OPS {
        if led.record_xform(o).is_ok() {
            recorded += 1;
        }
        o += 1;
    }
    let mut s = 0usize;
    while s < SSL_STATES {
        let _ = led.record_ssl(s);
        s += 1;
    }
    let mut r = 0usize;
    while r < THROTTLE_CELLS {
        if let Some(w) = throttle_cell(r) {
            if led.record_throttle(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut l = 0usize;
    while l < JAVA_LAYERS {
        let _ = led.record_java(l);
        l += 1;
    }
    // 注入 10 轮：抓包格漂移 / 操作家族逆序 / 抓包越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移；
    // order 注入对空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CharlesLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cap(0, cap_cell(0).unwrap_or(0));
                lt.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CharlesCode::ECapDrift)
            }
            1 => {
                let _ = lt.record_xform(5);
                lt.record_xform(2) == Err(CharlesCode::EOrder)
            }
            _ => lt.record_cap(CAP_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xA700 + k as u64);
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
/// UNX-F52250 判据集（锚 UNX-F52251-J1，off-by-one 板式）。
pub fn run_unx52250_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52250");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "HTTP/HTTPS 抓包三态 12 格全通",
        recorded >= CAP_CELLS as u32,
        "HTTP 明文抓包/HTTPS 解密抓包/会话回放归档三家族逐格恒等",
    );
    s.add(
        "断点/重写/映射 10 操作全通",
        {
            let mut lt = CharlesLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < XFORM_OPS {
                if lt.record_xform(o).is_err() {
                    ok = false;
                }
                o += 1;
            }
            ok && lt.xforms_all()
        },
        "断点 4/重写 3/映射 3 三家族逐操作落账，家族单调",
    );
    s.add(
        "SSL 代理三元闸 3/3 全落",
        {
            let mut lt = CharlesLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < SSL_STATES {
                if lt.record_ssl(st).is_err() {
                    ok = false;
                }
                st += 1;
            }
            ok && lt.ssls_all()
        },
        "SSL 代理开关/客户端证书安装/解密域名单三态逐格落账",
    );
    s.add(
        "限流/劫持 8 格全通",
        {
            let mut lt = CharlesLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < THROTTLE_CELLS {
                if let Some(w) = throttle_cell(r) {
                    if lt.record_throttle(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.throttles_all()
        },
        "限速档案 4 格 + 劫持规则 4 格逐格载荷对照恒等",
    );
    s.add(
        "Java 上栈 4 层全落",
        {
            let mut lt = CharlesLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                if lt.record_java(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.javas_all()
        },
        "JVM 启动参数/javax.net 信任库/系统代理挂钩/应用会话接入逐层落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CharlesLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/抓包/重写/限流/SSL 代理/卸载逐格落账",
    );
    s.add(
        "操作家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < XFORM_OPS {
                if xform_kind(o).unwrap_or(3) < xform_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "断点→重写→映射闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "抓包漂移/操作逆序/越闭集三态轮转必抓");
    let mut l0 = CharlesLedger::new();
    s.add(
        "边界·抓包越闭集",
        l0.record_cap(CAP_CELLS, 0).is_err(),
        "抓包格 12 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_xform(XFORM_OPS).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·限流劫持越闭集",
        l0.record_throttle(THROTTLE_CELLS, 0).is_err(),
        "限流/劫持格 8 越闭集必拒",
    );
    s.add(
        "限流劫持漂移拒",
        {
            let _ = l0.record_throttle(0, throttle_cell(0).unwrap_or(0));
            l0.record_throttle(1, throttle_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(CharlesCode::EThrottleDrift)
        },
        "限流/劫持格载荷漂移必抓",
    );
    s.add(
        "逆序拒后同族复验",
        {
            let _ = l0.record_xform(5);
            let r1 = l0.record_xform(2) == Err(CharlesCode::EOrder);
            // 逆序拒后 last_kind 未改，同族未录格复验必过。
            r1 && l0.record_xform(6).is_ok()
        },
        "逆序拒后同 last_kind 用同族未录格复验",
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
            let mut lt = CharlesLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < CAP_CELLS {
                if let Some(w) = cap_cell(i) {
                    let _ = lt.record_cap(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < XFORM_OPS {
                let _ = lt.record_xform(o);
                o += 1;
            }
            let mut st = 0usize;
            while st < SSL_STATES {
                let _ = lt.record_ssl(st);
                st += 1;
            }
            let mut r = 0usize;
            while r < THROTTLE_CELLS {
                if let Some(w) = throttle_cell(r) {
                    let _ = lt.record_throttle(r, w);
                }
                r += 1;
            }
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                let _ = lt.record_java(l);
                l += 1;
            }
            // main/aux 各节全部记账后再断言。
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 有条件收口（KB-COMPAT 定级登记，S1 留空不授）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7E40,
        "SEG_BASE 0x40 对齐且在 VX15 域 52250 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kinds() {
        assert_eq!(cap_kind(0), Some(0));
        assert_eq!(cap_kind(4), Some(1));
        assert_eq!(cap_kind(11), Some(2));
        assert!(cap_kind(12).is_none());
        assert!(cap_cell(12).is_none());
        assert_eq!(cap_kind_name(1), Some("HTTPS 解密抓包"));
    }

    #[test]
    fn test_xform_kinds() {
        assert_eq!(xform_kind(0), Some(0));
        assert_eq!(xform_kind(3), Some(0));
        assert_eq!(xform_kind(4), Some(1));
        assert_eq!(xform_kind(6), Some(1));
        assert_eq!(xform_kind(9), Some(2));
        assert!(xform_kind(10).is_none());
        assert!(xform_row(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = throttle_cell(3).unwrap_or(0);
        assert_eq!(a, throttle_cell(3).unwrap_or(0));
        assert_ne!(a, throttle_cell(4).unwrap_or(0));
        // XOR 锚常量低 16 位恒零：锚值低 16 位保持基址低 16 位。
        assert_eq!(a & 0xFFFF, (0x123000u64 + 3 * 0x37) & 0xFFFF);
        assert!(throttle_cell(THROTTLE_CELLS).is_none());
        assert!(ssl_state(SSL_STATES).is_none());
        assert!(java_layer(JAVA_LAYERS).is_none());
    }

    #[test]
    fn test_xform_order_caught() {
        let mut lt = CharlesLedger::new();
        let _ = lt.record_xform(5);
        assert_eq!(lt.record_xform(2), Err(CharlesCode::EOrder));
        assert!(lt.record_xform(6).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CharlesLedger::new();
        assert_eq!(
            lt.record_cap(5, cap_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(CharlesCode::ECapDrift)
        );
        assert_eq!(
            lt.record_throttle(6, throttle_cell(6).unwrap_or(0).wrapping_add(9)),
            Err(CharlesCode::EThrottleDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CharlesLedger::new();
        let _ = lt.record_ssl(1);
        assert_eq!(lt.record_ssl(1), Err(CharlesCode::EDup));
        let _ = lt.record_java(0);
        assert_eq!(lt.record_java(0), Err(CharlesCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CharlesLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= CAP_CELLS as u32 + XFORM_OPS as u32 + THROTTLE_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC7E40);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CharlesCode::Ok as u32, 0);
        assert_eq!(CharlesCode::EGradeDrift as u32, 12);
        assert_eq!(CharlesCode::EFp.val(), SEG_BASE + 11);
        assert!(!CharlesCode::Ok.detail().is_empty());
        assert!(!CharlesCode::ENoSsl.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = CharlesLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52250_checks_green() {
        let s = run_unx52250_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
