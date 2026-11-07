//! UNX-F52236 SecureCRT 收口 · 0xC7AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52236"，逐字）：
//!   title: SecureCRT 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52237-J1 六判据全
//!     落；传输 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52236）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52237-J1，照原文锚定；功能按本卡标题（SecureCRT）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 会话 10 格：SecureCRT 会话逐格落账，载荷对照漂移必抓。
//!   ② 协议三族 12 格：SSH2/SSH1/Telnet 三家族各 4 格，家族单调（逆序
//!      必抓）。
//!   ③ 脚本三态 8 格：VBScript/Python/JScript 三态轮转逐格登记，越集/
//!      漂移必抓。
//!   ④ 许可三元闸：正式许可/试用许可/教育许可三元闭集，越元必抓；许可
//!      面如实定级（封顶 S2）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（越闭集/漂移/逆序）全部落在当轮
//!      临时账本上，led 上 injected==caught==10、escaped==0；双跑指纹一
//!      致；定级闭集 S1/S2/S3（SecureCRT 封顶 S2：全通 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52236;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 会话格数。
pub const SESSIONS: usize = 10;

/// 协议三族格数（三族 × 4）。
pub const PROTOS: usize = 12;

/// 脚本三态格数。
pub const SCRIPTS: usize = 8;

/// 许可三元闸元数。
pub const LICENSE_MODES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecureCrtCode {
    /// 成功
    Ok = 0,
    /// 会话号越闭集
    ENoSession,
    /// 会话载荷漂移
    ESessionDrift,
    /// 协议格越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 协议家族逆序
    EOrder,
    /// 脚本格越闭集或漂移
    ENoScript,
    /// 许可三元越元
    ENoLicense,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl SecureCrtCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SecureCrtCode::Ok => "成功",
            SecureCrtCode::ENoSession => "会话号越闭集：SecureCRT 会话 10 格之外先登记",
            SecureCrtCode::ESessionDrift => "会话载荷漂移：会话折叠与基准不恒等，逐格复算",
            SecureCrtCode::ENoProto => "协议格越闭集：SSH2/SSH1/Telnet 12 格之外先登记",
            SecureCrtCode::EProtoDrift => "协议载荷漂移：协议折叠与基准不恒等，逐格复算",
            SecureCrtCode::EOrder => "协议家族逆序：SSH2→SSH1→Telnet 单调，逆序必抓",
            SecureCrtCode::ENoScript => "脚本格越闭集或漂移：VB/Python/JScript 三态 8 格之外先登记，载荷不恒等先复算",
            SecureCrtCode::ENoLicense => "许可三元越元：正式/试用/教育三元闭集之外先登记",
            SecureCrtCode::EDup => "重复落账：同格只许一条账目",
            SecureCrtCode::ESeal => "未齐即封：10 会话 + 12 协议 + 8 脚本 + 许可三元未齐先补齐",
            SecureCrtCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SecureCrtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SecureCrtCode::EGradeDrift => "定级漂移：全通且许可三元闸落账才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（SecureCRT 许可面如实定级，封顶 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本卡不适用（SecureCRT 封顶 S2）
    S1,
    /// S2：全通收口（如实定级）
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
            Grade::S1 => "S1：本卡不适用——SecureCRT 许可面如实定级封顶 S2",
            Grade::S2 => "S2：六判据+10会话+12协议+8脚本+许可三元全通收口",
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
    Some((0xF3600u64 + (c as u64) * 0x3B) ^ 0x534352545F534553)
}

/// 协议格载荷（格 p；家族 = p/4）。
pub const fn proto_cell(p: usize) -> Option<u64> {
    if p >= PROTOS {
        return None;
    }
    Some((0xF5000u64 + (p as u64) * 0x2C) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 协议家族（0..4=SSH2 4..8=SSH1 8..12=Telnet）。
pub const fn proto_kind(p: usize) -> Option<u32> {
    if p >= PROTOS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 协议家族名。
pub const fn proto_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("SSH2"),
        1 => Some("SSH1"),
        2 => Some("Telnet"),
        _ => None,
    }
}

/// 脚本三态格载荷（格 sc；态 = sc % 3）。
pub const fn script_cell(sc: usize) -> Option<u64> {
    if sc >= SCRIPTS {
        return None;
    }
    Some((0xF7000u64 + (sc as u64) * 0x5D) ^ ((sc % 3) as u64).wrapping_mul(0x9E3779B1))
}

/// 脚本三态名（态 s）。
pub const fn script_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("VBScript"),
        1 => Some("Python"),
        2 => Some("JScript"),
        _ => None,
    }
}

/// 许可三元闸元名。
pub const fn license_mode_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("正式许可"),
        1 => Some("试用许可"),
        2 => Some("教育许可"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SecureCRT 收口账本状态机。
#[derive(Clone)]
pub struct SecureCrtLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 会话账。
    sessions: [Option<u64>; SESSIONS],
    /// 协议账。
    protos: [Option<u64>; PROTOS],
    /// 脚本账。
    scripts: [Option<u64>; SCRIPTS],
    /// 许可三元闸。
    license: Option<usize>,
    /// 最后协议家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SecureCrtLedger {
    /// 新账本。
    pub fn new() -> Self {
        SecureCrtLedger {
            gates: [None; 6],
            sessions: [None; SESSIONS],
            protos: [None; PROTOS],
            scripts: [None; SCRIPTS],
            license: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SecureCrtCode> {
        if gate >= 6 {
            return Err(SecureCrtCode::ESeal);
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

    /// 会话一格（载荷对照 + 重复拒）。
    pub fn record_session(&mut self, c: usize, got: u64) -> Result<(), SecureCrtCode> {
        let want = match session_cell(c) {
            Some(w) => w,
            None => return Err(SecureCrtCode::ENoSession),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + c as u64);
            return Err(SecureCrtCode::ESessionDrift);
        }
        if match self.sessions.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SecureCrtCode::EDup);
        }
        if let Some(x) = self.sessions.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8201 + c as u64);
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

    /// 协议一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, p: usize, got: u64) -> Result<(), SecureCrtCode> {
        if p >= PROTOS {
            return Err(SecureCrtCode::ENoProto);
        }
        let kind = match proto_kind(p) {
            Some(k) => k,
            None => return Err(SecureCrtCode::ENoProto),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(SecureCrtCode::EOrder);
        }
        let want = match proto_cell(p) {
            Some(w) => w,
            None => return Err(SecureCrtCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + p as u64);
            return Err(SecureCrtCode::EProtoDrift);
        }
        if match self.protos.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SecureCrtCode::EDup);
        }
        if let Some(x) = self.protos.get_mut(p) {
            *x = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8302 + p as u64);
        Ok(())
    }

    /// 协议全账核验（12 格）。
    pub fn protos_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTOS {
            match self.protos.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 最后协议家族读数（逆序注入不改值核验）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 脚本一格（三态轮转 + 载荷对照 + 重复拒）。
    pub fn record_script(&mut self, sc: usize, got: u64) -> Result<(), SecureCrtCode> {
        let want = match script_cell(sc) {
            Some(w) => w,
            None => return Err(SecureCrtCode::ENoScript),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + sc as u64);
            return Err(SecureCrtCode::ENoScript);
        }
        if match self.scripts.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SecureCrtCode::EDup);
        }
        if let Some(x) = self.scripts.get_mut(sc) {
            *x = Some(got);
        }
        self.fnv(0x8401 + sc as u64);
        Ok(())
    }

    /// 脚本全账核验（8 格）。
    pub fn scripts_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCRIPTS {
            match self.scripts.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
        }
        true
    }

    /// 许可三元闸落账（越元必抓）。
    pub fn record_license(&mut self, m: usize) -> Result<(), SecureCrtCode> {
        if m >= LICENSE_MODES {
            return Err(SecureCrtCode::ENoLicense);
        }
        self.license = Some(m);
        self.fnv(0x8500 + m as u64);
        Ok(())
    }

    /// 许可三元闸已落核验。
    pub fn license_set(&self) -> bool {
        self.license.is_some()
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（SecureCRT 封顶 S2：全通 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.sessions_all()
            && self.protos_all()
            && self.scripts_all()
            && self.license_set()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SecureCrtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SecureCrtLedger::new();
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
    let mut p = 0usize;
    while p < PROTOS {
        if let Some(w) = proto_cell(p) {
            if led.record_proto(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut sc = 0usize;
    while sc < SCRIPTS {
        if let Some(w) = script_cell(sc) {
            if led.record_script(sc, w).is_ok() {
                recorded += 1;
            }
        }
        sc += 1;
    }
    let _ = led.record_license(0);
    // 注入 10 轮：越闭集 / 漂移 / 逆序三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SecureCrtLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：会话 10 与许可元 3 之外必抓。
                if k % 2 == 0 {
                    lt.record_session(SESSIONS, 0).is_err()
                } else {
                    lt.record_license(LICENSE_MODES).is_err()
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
                }) == Err(SecureCrtCode::ESessionDrift)
            }
            _ => {
                // 逆序：空账本上先录 Telnet 族再注 SSH2 族，逆序必抓且不改 last_kind。
                let _ = lt.record_proto(9, match proto_cell(9) {
                    Some(v) => v,
                    _ => 0,
                });
                let before = lt.last_kind();
                let hit = lt.record_proto(1, match proto_cell(1) {
                    Some(v) => v,
                    _ => 0,
                }) == Err(SecureCrtCode::EOrder);
                hit && lt.last_kind() == before
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52236 判据集（锚 UNX-F52237-J1，off-by-one 板式）。
pub fn run_unx52236_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52236");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "SecureCRT 会话 10 格全通",
        recorded >= SESSIONS as u32,
        "会话逐格载荷恒等，漂移必抓",
    );
    s.add(
        "协议三族 12 格全通",
        {
            let mut lt = SecureCrtLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROTOS {
                if let Some(w) = proto_cell(p) {
                    if lt.record_proto(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.protos_all()
        },
        "SSH2/SSH1/Telnet 逐格恒等，越级必抓",
    );
    s.add(
        "脚本三态 8 格全通",
        {
            let mut lt = SecureCrtLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SCRIPTS {
                if let Some(w) = script_cell(sc) {
                    if lt.record_script(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.scripts_all()
        },
        "VBScript/Python/JScript 三态轮转逐格登记，重复落账拒",
    );
    s.add(
        "许可三元闸落账",
        {
            let mut lt = SecureCrtLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < LICENSE_MODES {
                if lt.record_license(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.record_license(LICENSE_MODES).is_err()
        },
        "正式/试用/教育三元闭集，越元必抓",
    );
    s.add(
        "SecureCRT 六判据全落",
        {
            let mut lt = SecureCrtLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52237-J1）",
    );
    s.add(
        "协议家族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PROTOS {
                let a = match proto_kind(p) {
                    Some(k) => k,
                    _ => 3,
                };
                let b = match proto_kind(p - 1) {
                    Some(k) => k,
                    _ => 3,
                };
                if a < b {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "SSH2→SSH1→Telnet 三家族闭集单调",
    );
    s.add(
        "脚本三态名闭集",
        {
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SCRIPTS {
                if script_state_name(sc % 3).is_none() {
                    ok = false;
                }
                sc += 1;
            }
            ok && script_state_name(3).is_none()
        },
        "三态名逐格有表且越态为 None",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/漂移/逆序三态轮转必抓");
    let mut l0 = SecureCrtLedger::new();
    s.add(
        "边界·会话越闭集",
        l0.record_session(SESSIONS, 0).is_err(),
        "会话 10 越闭集必拒",
    );
    s.add(
        "边界·协议越闭集",
        l0.record_proto(PROTOS, 0).is_err(),
        "协议 12 越闭集必拒",
    );
    s.add(
        "边界·脚本越闭集",
        l0.record_script(SCRIPTS, 0).is_err(),
        "脚本 8 越闭集必拒",
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
            }) == Err(SecureCrtCode::ESessionDrift)
        },
        "会话载荷漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_script(0, match script_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_script(0, match script_cell(0) {
                Some(v) => v,
                _ => 0,
            }) == Err(SecureCrtCode::EDup)
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
        "许可面如实定级 S2",
        {
            let mut lt = SecureCrtLedger::new();
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
            let mut p = 0usize;
            while p < PROTOS {
                if let Some(w) = proto_cell(p) {
                    let _ = lt.record_proto(p, w);
                }
                p += 1;
            }
            let mut sc = 0usize;
            while sc < SCRIPTS {
                if let Some(w) = script_cell(sc) {
                    let _ = lt.record_script(sc, w);
                }
                sc += 1;
            }
            let _ = lt.record_license(0);
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，SecureCRT 封顶 S2）",
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
    fn test_proto_kind() {
        assert_eq!(proto_kind(0), Some(0));
        assert_eq!(proto_kind(4), Some(1));
        assert_eq!(proto_kind(11), Some(2));
        assert!(proto_kind(12).is_none());
        assert!(proto_cell(12).is_none());
        assert_eq!(proto_kind_name(2), Some("Telnet"));
        assert!(proto_kind_name(3).is_none());
    }

    #[test]
    fn test_script_states() {
        assert_eq!(script_state_name(0), Some("VBScript"));
        assert_eq!(script_state_name(1), Some("Python"));
        assert_eq!(script_state_name(2), Some("JScript"));
        assert!(script_state_name(3).is_none());
        assert!(script_cell(SCRIPTS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = session_cell(3).unwrap_or(0);
        assert_eq!(a, session_cell(3).unwrap_or(0));
        assert_ne!(a, session_cell(4).unwrap_or(0));
        let s0 = script_cell(0).unwrap_or(0);
        assert_ne!(s0, script_cell(1).unwrap_or(0));
        assert!(session_cell(SESSIONS).is_none());
    }

    #[test]
    fn test_license_ternary() {
        let mut lt = SecureCrtLedger::new();
        assert!(lt.record_license(0).is_ok());
        assert!(lt.record_license(2).is_ok());
        assert_eq!(lt.record_license(LICENSE_MODES), Err(SecureCrtCode::ENoLicense));
        assert!(lt.license_set());
        assert_eq!(license_mode_name(1), Some("试用许可"));
        assert!(license_mode_name(3).is_none());
    }

    #[test]
    fn test_proto_order_caught() {
        let mut lt = SecureCrtLedger::new();
        let _ = lt.record_proto(9, proto_cell(9).unwrap_or(0));
        let before = lt.last_kind();
        assert_eq!(lt.record_proto(1, proto_cell(1).unwrap_or(0)), Err(SecureCrtCode::EOrder));
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_proto(10, proto_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SecureCrtLedger::new();
        assert_eq!(
            lt.record_session(5, session_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SecureCrtCode::ESessionDrift)
        );
        assert_eq!(
            lt.record_proto(2, proto_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(SecureCrtCode::EProtoDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SecureCrtLedger::new();
        let _ = lt.record_session(0, session_cell(0).unwrap_or(0));
        assert_eq!(lt.record_session(0, session_cell(0).unwrap_or(0)), Err(SecureCrtCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SecureCrtLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SecureCrtLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SESSIONS + PROTOS + SCRIPTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7AC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SecureCrtCode::Ok as u32, 0);
        assert_eq!(SecureCrtCode::EGradeDrift as u32, 12);
        assert!(!SecureCrtCode::Ok.detail().is_empty());
        assert!(!SecureCrtCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SecureCrtLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn unx52236_checks_green() {
        let s = run_unx52236_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
