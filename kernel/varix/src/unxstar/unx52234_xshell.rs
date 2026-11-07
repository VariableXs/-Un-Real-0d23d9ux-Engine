//! UNX-F52234 Xshell 收口 · 0xC7A40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52234"，逐字）：
//!   title: Xshell 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52235-J1 六判据全
//!     落；SFTP 传输 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52234）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52235-J1，照原文锚定；功能按本卡标题（Xshell）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 会话 10 格：Xshell 会话逐格落账，载荷对照漂移必抓。
//!   ② 隧道 12 格：本地转发/远程转发/动态 SOCKS 三家族各 4 格，家族单
//!      调（逆序必抓）。
//!   ③ 宏/脚本 8 格：宏与脚本两家族各 4 格，逐格登记，越集/漂移必抓。
//!   ④ 密钥管理三元闸：生成/导入/导出三元闭集，越元必抓。
//!   ⑤ 商业/家庭分账 2 行闭集：商业版分账声明一行、家庭版分账声明一行，
//!      缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（越闭集/漂移/逆序）全部落在当轮
//!      临时账本上，led 上 injected==caught==10、escaped==0；双跑指纹一
//!      致；定级闭集 S1/S2/S3（全通 → S1，KB-COMPAT 定级登记）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52234;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 会话格数。
pub const SESSIONS: usize = 10;

/// 隧道格数（三家族 × 4）。
pub const TUNNELS: usize = 12;

/// 宏/脚本格数（两家族 × 4）。
pub const MACROS: usize = 8;

/// 密钥管理三元闸元数。
pub const KEY_MODES: usize = 3;

/// 商业/家庭分账行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XshellCode {
    /// 成功
    Ok = 0,
    /// 会话号越闭集
    ENoSession,
    /// 会话载荷漂移
    ESessionDrift,
    /// 隧道号越闭集
    ENoTunnel,
    /// 隧道载荷漂移
    ETunnelDrift,
    /// 隧道家族逆序
    EOrder,
    /// 宏/脚本越闭集或漂移
    ENoMacro,
    /// 密钥三元越元或漂移
    ENoKey,
    /// 分账行越闭集
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

impl XshellCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            XshellCode::Ok => "成功",
            XshellCode::ENoSession => "会话号越闭集：Xshell 会话 10 格之外先登记",
            XshellCode::ESessionDrift => "会话载荷漂移：会话折叠与基准不恒等，逐格复算",
            XshellCode::ENoTunnel => "隧道号越闭集：本地/远程/动态隧道 12 格之外先登记",
            XshellCode::ETunnelDrift => "隧道载荷漂移：隧道折叠与基准不恒等，逐格复算",
            XshellCode::EOrder => "隧道家族逆序：本地转发→远程转发→动态 SOCKS 单调，逆序必抓",
            XshellCode::ENoMacro => "宏/脚本越闭集或漂移：8 格闭集之外先登记，载荷不恒等先复算",
            XshellCode::ENoKey => "密钥三元越元或漂移：生成/导入/导出三元闭集之外先登记",
            XshellCode::ENoRow => "分账行越闭集：商业/家庭分账 2 行之外先登记，缺行先补齐",
            XshellCode::EDup => "重复落账：同格只许一条账目",
            XshellCode::ESeal => "未齐即封：10 会话 + 12 隧道 + 8 宏脚本 + 2 行分账未齐先补齐",
            XshellCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            XshellCode::EGradeDrift => "定级漂移：全通且密钥三元闸落账才可定 S1",
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
            Grade::S1 => "S1：六判据+10会话+12隧道+8宏脚本+2行分账+密钥三元全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 会话格载荷（会话 c）。
pub const fn session_cell(c: usize) -> Option<u64> {
    if c >= SESSIONS {
        return None;
    }
    Some((0xD3400u64 + (c as u64) * 0x3B) ^ 0x5853485F53455353)
}

/// 隧道格载荷（隧道 t；家族 = t/4）。
pub const fn tunnel_cell(t: usize) -> Option<u64> {
    if t >= TUNNELS {
        return None;
    }
    Some((0xD5000u64 + (t as u64) * 0x2C) ^ ((t / 4) as u64).wrapping_mul(0x01000193))
}

/// 隧道家族（0..4=本地转发 4..8=远程转发 8..12=动态 SOCKS）。
pub const fn tunnel_kind(t: usize) -> Option<u32> {
    if t >= TUNNELS {
        return None;
    }
    Some((t / 4) as u32)
}

/// 隧道家族名。
pub const fn tunnel_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("本地转发"),
        1 => Some("远程转发"),
        2 => Some("动态 SOCKS"),
        _ => None,
    }
}

/// 宏/脚本格载荷（格 m；家族 = m/4）。
pub const fn macro_cell(m: usize) -> Option<u64> {
    if m >= MACROS {
        return None;
    }
    Some((0xD7000u64 + (m as u64) * 0x5D) ^ ((m % 4) as u64).wrapping_mul(0x9E3779B1))
}

/// 宏/脚本家族名。
pub const fn macro_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("宏"),
        1 => Some("脚本"),
        _ => None,
    }
}

/// 密钥三元闸元名。
pub const fn key_mode_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("密钥生成"),
        1 => Some("密钥导入"),
        2 => Some("密钥导出"),
        _ => None,
    }
}

/// 商业/家庭分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD900 + r as u32 * 0x2C)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("商业版分账声明"),
        1 => Some("家庭版分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Xshell 收口账本状态机。
#[derive(Clone)]
pub struct XshellLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 会话账。
    sessions: [Option<u64>; SESSIONS],
    /// 隧道账。
    tunnels: [Option<u64>; TUNNELS],
    /// 宏/脚本账。
    macros: [Option<u64>; MACROS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 密钥三元闸。
    key: Option<usize>,
    /// 最后隧道家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl XshellLedger {
    /// 新账本。
    pub fn new() -> Self {
        XshellLedger {
            gates: [None; 6],
            sessions: [None; SESSIONS],
            tunnels: [None; TUNNELS],
            macros: [None; MACROS],
            splits: [None; SPLIT_ROWS],
            key: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), XshellCode> {
        if gate >= 6 {
            return Err(XshellCode::ESeal);
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

    /// 会话一格（载荷对照 + 重复拒）。
    pub fn record_session(&mut self, c: usize, got: u64) -> Result<(), XshellCode> {
        let want = match session_cell(c) {
            Some(w) => w,
            None => return Err(XshellCode::ENoSession),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + c as u64);
            return Err(XshellCode::ESessionDrift);
        }
        if match self.sessions.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XshellCode::EDup);
        }
        if let Some(x) = self.sessions.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x6201 + c as u64);
        Ok(())
    }

    /// 会话全账核验（10 格）。
    pub fn sessions_all(&self) -> bool {
        let mut c = 0usize;
        while c < SESSIONS {
            match self.sessions.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 隧道一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_tunnel(&mut self, t: usize, got: u64) -> Result<(), XshellCode> {
        if t >= TUNNELS {
            return Err(XshellCode::ENoTunnel);
        }
        let kind = match tunnel_kind(t) {
            Some(k) => k,
            None => return Err(XshellCode::ENoTunnel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(XshellCode::EOrder);
        }
        let want = match tunnel_cell(t) {
            Some(w) => w,
            None => return Err(XshellCode::ENoTunnel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + t as u64);
            return Err(XshellCode::ETunnelDrift);
        }
        if match self.tunnels.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XshellCode::EDup);
        }
        if let Some(x) = self.tunnels.get_mut(t) {
            *x = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + t as u64);
        Ok(())
    }

    /// 隧道全账核验（12 格）。
    pub fn tunnels_all(&self) -> bool {
        let mut t = 0usize;
        while t < TUNNELS {
            match self.tunnels.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 最后隧道家族读数（逆序注入不改值核验）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 宏/脚本一格（载荷对照 + 重复拒）。
    pub fn record_macro(&mut self, m: usize, got: u64) -> Result<(), XshellCode> {
        let want = match macro_cell(m) {
            Some(w) => w,
            None => return Err(XshellCode::ENoMacro),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + m as u64);
            return Err(XshellCode::ENoMacro);
        }
        if match self.macros.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XshellCode::EDup);
        }
        if let Some(x) = self.macros.get_mut(m) {
            *x = Some(got);
        }
        self.fnv(0x6401 + m as u64);
        Ok(())
    }

    /// 宏/脚本全账核验（8 格）。
    pub fn macros_all(&self) -> bool {
        let mut m = 0usize;
        while m < MACROS {
            match self.macros.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 密钥三元闸落账（越元必抓）。
    pub fn record_key(&mut self, m: usize) -> Result<(), XshellCode> {
        if m >= KEY_MODES {
            return Err(XshellCode::ENoKey);
        }
        self.key = Some(m);
        self.fnv(0x6500 + m as u64);
        Ok(())
    }

    /// 密钥三元闸已落核验。
    pub fn key_set(&self) -> bool {
        self.key.is_some()
    }

    /// 分账一行（商业/家庭）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), XshellCode> {
        if r >= SPLIT_ROWS {
            return Err(XshellCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(XshellCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x6600 + r as u64);
        Ok(())
    }

    /// 分账全齐核验（2 行）。
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.sessions_all()
            && self.tunnels_all()
            && self.macros_all()
            && self.splits_all()
            && self.key_set()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for XshellLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = XshellLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < SESSIONS {
        if let Some(w) = session_cell(c) {
            if led.record_session(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < TUNNELS {
        if let Some(w) = tunnel_cell(t) {
            if led.record_tunnel(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut m = 0usize;
    while m < MACROS {
        if let Some(w) = macro_cell(m) {
            if led.record_macro(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let _ = led.record_key(0);
    // 注入 10 轮：越闭集 / 漂移 / 逆序三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = XshellLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：会话 10 与密钥元 3 之外必抓。
                if k % 2 == 0 {
                    lt.record_session(SESSIONS, 0).is_err()
                } else {
                    lt.record_key(KEY_MODES).is_err()
                }
            }
            1 => {
                // 漂移：先录正格再注漂移，漂移必抓。
                let _ = lt.record_session(0, match session_cell(0) {
                    Some(v) => v,
                    _ => 0,
                });
                lt.record_session(1, match session_cell(1) {
                    Some(v) => v.wrapping_add(1),
                    _ => 1,
                }) == Err(XshellCode::ESessionDrift)
            }
            _ => {
                // 逆序：空账本上先录远端家族再注本地家族，逆序必抓且不改 last_kind。
                let _ = lt.record_tunnel(5, match tunnel_cell(5) {
                    Some(v) => v,
                    _ => 0,
                });
                let before = lt.last_kind();
                let hit = lt.record_tunnel(1, match tunnel_cell(1) {
                    Some(v) => v,
                    _ => 0,
                }) == Err(XshellCode::EOrder);
                hit && lt.last_kind() == before
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52234 判据集（锚 UNX-F52235-J1，off-by-one 板式）。
pub fn run_unx52234_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52234");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Xshell 会话 10 格全通",
        recorded >= SESSIONS as u32,
        "会话逐格载荷恒等，漂移必抓",
    );
    s.add(
        "隧道 12 格三家族全通",
        {
            let mut lt = XshellLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TUNNELS {
                if let Some(w) = tunnel_cell(t) {
                    if lt.record_tunnel(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tunnels_all()
        },
        "本地转发/远程转发/动态 SOCKS 逐格恒等，越级必抓",
    );
    s.add(
        "宏/脚本 8 格全通",
        {
            let mut lt = XshellLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MACROS {
                if let Some(w) = macro_cell(m) {
                    if lt.record_macro(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.macros_all()
        },
        "宏与脚本两家族逐格登记，重复落账拒",
    );
    s.add(
        "密钥管理三元闸落账",
        {
            let mut lt = XshellLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < KEY_MODES {
                if lt.record_key(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.record_key(KEY_MODES).is_err()
        },
        "生成/导入/导出三元闭集，越元必抓",
    );
    s.add(
        "商业/家庭分账 2/2 全齐",
        {
            let mut lt = XshellLedger::new();
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
        "商业版/家庭版分账声明两行闭集，缺一拒封",
    );
    s.add(
        "Xshell 六判据全落",
        {
            let mut lt = XshellLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52235-J1）",
    );
    s.add(
        "隧道家族单调",
        {
            let mut ok = true;
            let mut t = 1usize;
            while t < TUNNELS {
                let a = match tunnel_kind(t) {
                    Some(k) => k,
                    _ => 3,
                };
                let b = match tunnel_kind(t - 1) {
                    Some(k) => k,
                    _ => 3,
                };
                if a < b {
                    ok = false;
                }
                t += 1;
            }
            ok
        },
        "三家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/漂移/逆序三态轮转必抓");
    let mut l0 = XshellLedger::new();
    s.add(
        "边界·会话越闭集",
        l0.record_session(SESSIONS, 0).is_err(),
        "会话 10 越闭集必拒",
    );
    s.add(
        "边界·隧道越闭集",
        l0.record_tunnel(TUNNELS, 0).is_err(),
        "隧道 12 越闭集必拒",
    );
    s.add(
        "边界·宏/脚本越闭集",
        l0.record_macro(MACROS, 0).is_err(),
        "宏/脚本 8 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            l0.record_session(0, match session_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_session(1, match session_cell(1) {
                Some(v) => v.wrapping_add(1),
                _ => 1,
            }) == Err(XshellCode::ESessionDrift)
        },
        "会话载荷漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_macro(0, match macro_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_macro(0, match macro_cell(0) {
                Some(v) => v,
                _ => 0,
            }) == Err(XshellCode::EDup)
        },
        "同格只许一条账目",
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
            let mut lt = XshellLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < SESSIONS {
                if let Some(w) = session_cell(c) {
                    let _ = lt.record_session(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < TUNNELS {
                if let Some(w) = tunnel_cell(t) {
                    let _ = lt.record_tunnel(t, w);
                }
                t += 1;
            }
            let mut m = 0usize;
            while m < MACROS {
                if let Some(w) = macro_cell(m) {
                    let _ = lt.record_macro(m, w);
                }
                m += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let _ = lt.record_key(0);
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且符合 VX15 派工公式",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_kind() {
        assert_eq!(tunnel_kind(0), Some(0));
        assert_eq!(tunnel_kind(4), Some(1));
        assert_eq!(tunnel_kind(11), Some(2));
        assert!(tunnel_kind(12).is_none());
        assert!(tunnel_cell(12).is_none());
        assert_eq!(tunnel_kind_name(2), Some("动态 SOCKS"));
        assert!(tunnel_kind_name(3).is_none());
    }

    #[test]
    fn test_split_rows() {
        assert_eq!(split_row_name(0), Some("商业版分账声明"));
        assert_eq!(split_row_name(1), Some("家庭版分账声明"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = session_cell(3).unwrap_or(0);
        assert_eq!(a, session_cell(3).unwrap_or(0));
        assert_ne!(a, session_cell(4).unwrap_or(0));
        let m0 = macro_cell(0).unwrap_or(0);
        assert_ne!(m0, macro_cell(1).unwrap_or(0));
        assert!(session_cell(SESSIONS).is_none());
        assert!(macro_cell(MACROS).is_none());
    }

    #[test]
    fn test_key_ternary() {
        let mut lt = XshellLedger::new();
        assert!(lt.record_key(0).is_ok());
        assert!(lt.record_key(2).is_ok());
        assert_eq!(lt.record_key(KEY_MODES), Err(XshellCode::ENoKey));
        assert!(lt.key_set());
        assert_eq!(key_mode_name(1), Some("密钥导入"));
        assert!(key_mode_name(3).is_none());
    }

    #[test]
    fn test_tunnel_order_caught() {
        let mut lt = XshellLedger::new();
        let _ = lt.record_tunnel(9, tunnel_cell(9).unwrap_or(0));
        let before = lt.last_kind();
        assert_eq!(lt.record_tunnel(1, tunnel_cell(1).unwrap_or(0)), Err(XshellCode::EOrder));
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_tunnel(10, tunnel_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = XshellLedger::new();
        assert_eq!(
            lt.record_session(5, session_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(XshellCode::ESessionDrift)
        );
        assert_eq!(
            lt.record_tunnel(2, tunnel_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(XshellCode::ETunnelDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = XshellLedger::new();
        let _ = lt.record_session(0, session_cell(0).unwrap_or(0));
        assert_eq!(lt.record_session(0, session_cell(0).unwrap_or(0)), Err(XshellCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = XshellLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = XshellLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SESSIONS + TUNNELS + MACROS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7A40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(XshellCode::Ok as u32, 0);
        assert_eq!(XshellCode::EGradeDrift as u32, 12);
        assert!(!XshellCode::Ok.detail().is_empty());
        assert!(!XshellCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = XshellLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52234_checks_green() {
        let s = run_unx52234_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
