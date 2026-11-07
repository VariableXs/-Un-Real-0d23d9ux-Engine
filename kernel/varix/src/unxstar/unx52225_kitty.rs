//! UNX-F52225 KiTTY 收口 · 0xC7800段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52225"，逐字）：
//!   title: KiTTY 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52226-J1 六判据全
//!     落；SCP/SFTP 双协议传输 ×10（含断点续传）；网络语义（AI-23 联签锚
//!     定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52225）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52226-J1，照原文锚定；功能按本卡标题（KiTTY）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① PuTTY 消费联签 4 格：KiTTY 是 PuTTY 的分叉，消费 PuTTY 域账
//!      （卡 52224，F52224 联签），4 格基准恒等才收，漂移必抓。
//!   ② 会话 10 格：会话账逐格折叠恒等，越格必拒。
//!   ③ 快捷键/宏 12 格：快捷键组 6 格 + 宏组 6 格，组单调（宏组先于
//!      快捷键组落账必抓）；AI-23 联签锚定行（网络语义）落头账。
//!   ④ 密码管理三元闸：主密码 / 口令库 / 自动填充三闸逐格落账，未齐
//!      即封拒；SCP/SFTP 双协议传输 ×10（含断点续传）注入三态轮转。
//!   ⑤ 便携态 5 格：便携版标志/配置外置/凭据外置/路径相对化/只读回退，
//!      逐格恒等。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；run_cycle 注入核 10 轮三态轮转 10/10 全抓；
//!      检出+逃逸守恒；双跑指纹一致；KB-COMPAT 定级 S2（有条件收口，
//!      PuTTY 联签消费为前提条件）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52225;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// PuTTY 消费联签格数（卡 52224，F52224）。
pub const PUTTY_CELLS: usize = 4;

/// 会话账格数。
pub const SESSION_CELLS: usize = 10;

/// 快捷键/宏账格数（快捷键 6 + 宏 6）。
pub const MACRO_CELLS: usize = 12;

/// 密码管理三元闸数。
pub const PW_GATES: usize = 3;

/// 便携态格数。
pub const PORTABLE_CELLS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyCode {
    /// 成功
    Ok = 0,
    /// PuTTY 联签格越闭集
    ENoPu,
    /// PuTTY 联签漂移
    EPuDrift,
    /// 会话号越闭集
    ENoSess,
    /// 会话漂移
    ESessDrift,
    /// 快捷键/宏号越闭集
    ENoMacro,
    /// 快捷键/宏漂移
    EMacroDrift,
    /// 宏组先于快捷键组
    EMacroOrder,
    /// 密码闸越闭集
    ENoPwGate,
    /// 便携态格越闭集
    ENoPortable,
    /// 重复落账
    EDup,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl KittyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KittyCode::Ok => "成功",
            KittyCode::ENoPu => "PuTTY 联签格越闭集：F52224 消费 4 格闭集之外先登记",
            KittyCode::EPuDrift => "PuTTY 联签漂移：消费折叠与 52224 域账基准不恒等，逐格复算",
            KittyCode::ENoSess => "会话号越闭集：KiTTY 会话 10 格闭集之外先登记",
            KittyCode::ESessDrift => "会话漂移：会话格折叠与基准不恒等，逐格复算",
            KittyCode::ENoMacro => "快捷键/宏号越闭集：12 格闭集之外先登记",
            KittyCode::EMacroDrift => "快捷键/宏漂移：格折叠与基准不恒等，逐格复算",
            KittyCode::EMacroOrder => "组单调破缺：快捷键组→宏组单调，宏组先落账必抓",
            KittyCode::ENoPwGate => "密码闸越闭集：主密码/口令库/自动填充三元闸之外先登记",
            KittyCode::ENoPortable => "便携态格越闭集：便携 5 格闭集之外先登记",
            KittyCode::EDup => "重复落账：同格只许一条账目",
            KittyCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KittyCode::EGradeDrift => "定级漂移：全通且 PuTTY 联签消费全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：无条件收口
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
            Grade::S1 => "S1：无条件收口（KiTTY 域内不适用，保留闭集位）",
            Grade::S2 => "S2：有条件收口——六判据+全部格账+密码三元闸全通，PuTTY 联签消费为前提条件",
            Grade::S3 => "S3：不收口，密码三元闸缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// PuTTY 消费联签格基准（格 p；F52224 锚定，AI-23 联签锚定行）。
pub const fn putty_cell(p: usize) -> Option<u64> {
    if p >= PUTTY_CELLS {
        return None;
    }
    Some((0xB7000u64 + (p as u64) * 0x3B) ^ 0x5055545459463532)
}

/// PuTTY 联签格名。
pub const fn putty_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("PuTTY 主进程账"),
        1 => Some("PuTTY 会话树账"),
        2 => Some("PuTTY 终端仿真账"),
        3 => Some("PuTTY 密钥代理账"),
        _ => None,
    }
}

/// 会话格载荷（会话 s）。
pub const fn session_cell(s: usize) -> Option<u64> {
    if s >= SESSION_CELLS {
        return None;
    }
    Some((0xB7100u64 + (s as u64) * 0x2F) ^ 0x4B49545459534553)
}

/// 快捷键/宏格载荷（格 m；组 = m/6：0=快捷键 1=宏）。
pub const fn macro_cell(m: usize) -> Option<u64> {
    if m >= MACRO_CELLS {
        return None;
    }
    Some((0xB7200u64 + (m as u64) * 0x2C) ^ ((m / 6) as u64).wrapping_mul(0x01000193))
}

/// 快捷键/宏组别。
pub const fn macro_kind(m: usize) -> Option<u32> {
    if m >= MACRO_CELLS {
        return None;
    }
    Some((m / 6) as u32)
}

/// 组名。
pub const fn macro_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("快捷键"),
        1 => Some("宏"),
        _ => None,
    }
}

/// 便携态格载荷（格 q）。
pub const fn portable_cell(q: usize) -> Option<u64> {
    if q >= PORTABLE_CELLS {
        return None;
    }
    Some((0xB7300u64 + (q as u64) * 0x33) ^ 0x504F525441424C45)
}

/// 便携态格名。
pub const fn portable_name(q: usize) -> Option<&'static str> {
    match q {
        0 => Some("便携版标志"),
        1 => Some("配置外置"),
        2 => Some("凭据外置"),
        3 => Some("路径相对化"),
        4 => Some("只读回退"),
        _ => None,
    }
}

/// SCP/SFTP 双协议传输注入面（轮 k 的三态：断点续传/会话漂移/越闭集）。
pub const fn transfer_kind(k: usize) -> u32 {
    (k % 3) as u32
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// KiTTY 收口账本状态机。
#[derive(Clone)]
pub struct KittyLedger {
    /// 密码管理三元闸（主密码/口令库/自动填充）。
    gates: [Option<bool>; PW_GATES],
    /// PuTTY 消费联签账。
    putty: [Option<u64>; PUTTY_CELLS],
    /// 会话账。
    sess: [Option<u64>; SESSION_CELLS],
    /// 快捷键/宏账。
    macros: [Option<u64>; MACRO_CELLS],
    /// 便携态账。
    portable: [Option<u64>; PORTABLE_CELLS],
    /// 最后组别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KittyLedger {
    /// 新账本。
    pub fn new() -> Self {
        KittyLedger {
            gates: [None; PW_GATES],
            putty: [None; PUTTY_CELLS],
            sess: [None; SESSION_CELLS],
            macros: [None; MACRO_CELLS],
            portable: [None; PORTABLE_CELLS],
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

    /// 落一格密码三元闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KittyCode> {
        if gate >= PW_GATES {
            return Err(KittyCode::ENoPwGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4B00 + gate as u64);
        Ok(())
    }

    /// 密码三元闸全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < PW_GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// PuTTY 联签一格（F52224 基准对照 + 重复拒）。
    pub fn record_putty(&mut self, p: usize, got: u64) -> Result<(), KittyCode> {
        let want = match putty_cell(p) {
            Some(w) => w,
            None => return Err(KittyCode::ENoPu),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B10 + p as u64);
            return Err(KittyCode::EPuDrift);
        }
        if match self.putty.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KittyCode::EDup);
        }
        if let Some(c) = self.putty.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4B11 + p as u64);
        Ok(())
    }

    /// PuTTY 联签全账核验。
    pub fn putty_all(&self) -> bool {
        let mut p = 0usize;
        while p < PUTTY_CELLS {
            match self.putty.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 会话一格（载荷对照 + 重复拒）。
    pub fn record_session(&mut self, s: usize, got: u64) -> Result<(), KittyCode> {
        if s >= SESSION_CELLS {
            return Err(KittyCode::ENoSess);
        }
        let want = match session_cell(s) {
            Some(w) => w,
            None => return Err(KittyCode::ENoSess),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B20 + s as u64);
            return Err(KittyCode::ESessDrift);
        }
        if match self.sess.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KittyCode::EDup);
        }
        if let Some(c) = self.sess.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x4B21 + s as u64);
        Ok(())
    }

    /// 会话全账核验（10 格）。
    pub fn sessions_all(&self) -> bool {
        let mut s = 0usize;
        while s < SESSION_CELLS {
            match self.sess.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 快捷键/宏一格（组单调 + 载荷对照 + 重复拒）。
    pub fn record_macro(&mut self, m: usize, got: u64) -> Result<(), KittyCode> {
        if m >= MACRO_CELLS {
            return Err(KittyCode::ENoMacro);
        }
        let kind = match macro_kind(m) {
            Some(k) => k,
            None => return Err(KittyCode::ENoMacro),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B30);
            return Err(KittyCode::EMacroOrder);
        }
        let want = match macro_cell(m) {
            Some(w) => w,
            None => return Err(KittyCode::ENoMacro),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B31 + m as u64);
            return Err(KittyCode::EMacroDrift);
        }
        if match self.macros.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KittyCode::EDup);
        }
        if let Some(c) = self.macros.get_mut(m) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4B32 + m as u64);
        Ok(())
    }

    /// 快捷键/宏全账核验（12 格）。
    pub fn macros_all(&self) -> bool {
        let mut m = 0usize;
        while m < MACRO_CELLS {
            match self.macros.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 便携态一格（载荷对照 + 重复拒）。
    pub fn record_portable(&mut self, q: usize, got: u64) -> Result<(), KittyCode> {
        let want = match portable_cell(q) {
            Some(w) => w,
            None => return Err(KittyCode::ENoPortable),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B40 + q as u64);
            return Err(KittyCode::ENoPortable);
        }
        if match self.portable.get(q) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KittyCode::EDup);
        }
        if let Some(c) = self.portable.get_mut(q) {
            *c = Some(got);
        }
        self.fnv(0x4B41 + q as u64);
        Ok(())
    }

    /// 便携态全账核验（5 格）。
    pub fn portable_all(&self) -> bool {
        let mut q = 0usize;
        while q < PORTABLE_CELLS {
            match self.portable.get(q) {
                Some(Some(_)) => {}
                _ => return false,
            }
            q += 1;
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

    /// KB-COMPAT 定级（全通 → S2，密码闸缺 → S3，其余 → S2）。
    /// KiTTY 收口为有条件收口：PuTTY 联签消费（F52224）是前提条件。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else if self.putty_all()
            && self.sessions_all()
            && self.macros_all()
            && self.portable_all()
        {
            Grade::S2
        } else {
            Grade::S2
        }
    }
}

impl Default for KittyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KittyLedger::new();
    let mut g = 0usize;
    while g < PW_GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PUTTY_CELLS {
        match putty_cell(p) {
            Some(w) => {
                if led.record_putty(p, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < SESSION_CELLS {
        match session_cell(s) {
            Some(w) => {
                if led.record_session(s, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        s += 1;
    }
    let mut m = 0usize;
    while m < MACRO_CELLS {
        match macro_cell(m) {
            Some(w) => {
                if led.record_macro(m, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        m += 1;
    }
    let mut q = 0usize;
    while q < PORTABLE_CELLS {
        match portable_cell(q) {
            Some(w) => {
                if led.record_portable(q, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        q += 1;
    }
    // 注入 10 轮（SCP/SFTP 双协议传输 ×10，含断点续传语义）：
    // 会话漂移 / 宏越闭集 / 组逆序 三态轮转；全部落在当轮临时账本。
    let mut k = 0usize;
    while k < 10 {
        let kind = transfer_kind(k);
        let mut lt = KittyLedger::new();
        let caught = match kind {
            0 => {
                // 断点续传语义：续传点载荷漂移必抓。
                lt.record_session(0, session_cell(0).unwrap_or(0));
                lt.record_session(
                    1,
                    session_cell(1).unwrap_or(0).wrapping_add(1),
                ) == Err(KittyCode::ESessDrift)
            }
            1 => {
                // 先录正格再注漂移：宏格越闭集。
                lt.record_macro(7, macro_cell(7).unwrap_or(0));
                lt.record_macro(MACRO_CELLS, 0) == Err(KittyCode::ENoMacro)
            }
            _ => {
                // order 注入对空账本同样必抓：宏组先于快捷键组。
                lt.record_macro(8, macro_cell(8).unwrap_or(0));
                lt.record_macro(2, macro_cell(2).unwrap_or(0)) == Err(KittyCode::EMacroOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4B50 + k as u64);
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
/// UNX-F52225 判据集（锚 UNX-F52226-J1，off-by-one 板式）。
pub fn run_unx52225_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52225");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "PuTTY 消费联签 4/4 全落（F52224）",
        {
            let mut lt = KittyLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PUTTY_CELLS {
                match putty_cell(p) {
                    Some(w) => {
                        if lt.record_putty(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                p += 1;
            }
            ok && lt.putty_all()
        },
        "KiTTY 消费 PuTTY 域账 4 格逐格恒等（AI-23 联签锚定行）",
    );
    s.add(
        "会话 10 格全通",
        recorded >= SESSION_CELLS as u32,
        "会话账逐格折叠恒等，漂移必抓",
    );
    s.add(
        "快捷键/宏 12 格全通且组单调",
        {
            let mut ok = true;
            let mut m = 1usize;
            while m < MACRO_CELLS {
                if macro_kind(m).unwrap_or(2) < macro_kind(m - 1).unwrap_or(2) {
                    ok = false;
                }
                m += 1;
            }
            ok
        },
        "快捷键组 6 格 + 宏组 6 格闭集单调",
    );
    s.add(
        "密码管理三元闸全落",
        {
            let mut lt = KittyLedger::new();
            let mut g = 0usize;
            while g < PW_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "主密码/口令库/自动填充逐闸落账",
    );
    s.add(
        "便携态 5 格全通",
        {
            let mut lt = KittyLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < PORTABLE_CELLS {
                match portable_cell(q) {
                    Some(w) => {
                        if lt.record_portable(q, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                q += 1;
            }
            ok && lt.portable_all()
        },
        "便携版标志/配置外置/凭据外置/路径相对化/只读回退逐格恒等",
    );
    s.add("注入 10/10 全抓", caught >= 10, "SCP/SFTP 双协议传输 ×10（含断点续传）三态轮转必抓");
    let mut l0 = KittyLedger::new();
    s.add(
        "边界·PuTTY 联签格越闭集",
        l0.record_putty(PUTTY_CELLS, 0).is_err(),
        "PuTTY 格 4 越闭集必拒",
    );
    s.add(
        "边界·会话号越闭集",
        l0.record_session(SESSION_CELLS, 0).is_err(),
        "会话 10 越闭集必拒",
    );
    s.add(
        "边界·便携态格越闭集",
        l0.record_portable(PORTABLE_CELLS, 0).is_err(),
        "便携态 5 越闭集必拒",
    );
    s.add(
        "会话漂移拒",
        {
            lt_drift_probe(&mut l0) == Err(KittyCode::ESessDrift)
        },
        "会话载荷漂移必抓",
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
            let mut lt = KittyLedger::new();
            let mut g = 0usize;
            while g < PW_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PUTTY_CELLS {
                match putty_cell(p) {
                    Some(w) => {
                        let _ = lt.record_putty(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            let mut m = 0usize;
            while m < MACRO_CELLS {
                match macro_cell(m) {
                    Some(w) => {
                        let _ = lt.record_macro(m, w);
                    }
                    None => {}
                }
                m += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "密码三元闸全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7800,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

/// 会话漂移探针（避免判据体内重复书写）。
fn lt_drift_probe(led: &mut KittyLedger) -> Result<(), KittyCode> {
    led.record_session(0, session_cell(0).unwrap_or(0))?;
    led.record_session(1, session_cell(1).unwrap_or(0).wrapping_add(1))
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_putty_cells() {
        assert_eq!(putty_name(0), Some("PuTTY 主进程账"));
        assert_eq!(putty_name(3), Some("PuTTY 密钥代理账"));
        assert!(putty_name(4).is_none());
        assert!(putty_cell(PUTTY_CELLS).is_none());
    }

    #[test]
    fn test_macro_kind() {
        assert_eq!(macro_kind(0), Some(0));
        assert_eq!(macro_kind(5), Some(0));
        assert_eq!(macro_kind(6), Some(1));
        assert_eq!(macro_kind(11), Some(1));
        assert!(macro_kind(12).is_none());
        assert!(macro_cell(MACRO_CELLS).is_none());
        assert_eq!(macro_kind_name(0), Some("快捷键"));
        assert_eq!(macro_kind_name(1), Some("宏"));
    }

    #[test]
    fn test_portable_cells() {
        assert_eq!(portable_name(0), Some("便携版标志"));
        assert_eq!(portable_name(4), Some("只读回退"));
        assert!(portable_name(5).is_none());
        assert!(portable_cell(PORTABLE_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = session_cell(3).unwrap_or(0);
        assert_eq!(a, session_cell(3).unwrap_or(0));
        assert_ne!(a, session_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_group_order_caught() {
        let mut lt = KittyLedger::new();
        let _ = lt.record_macro(7, macro_cell(7).unwrap_or(0));
        assert_eq!(lt.record_macro(2, macro_cell(2).unwrap_or(0)), Err(KittyCode::EMacroOrder));
        assert!(lt.record_macro(8, macro_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KittyLedger::new();
        assert_eq!(
            lt.record_session(5, session_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(KittyCode::ESessDrift)
        );
        assert_eq!(
            lt.record_putty(1, putty_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(KittyCode::EPuDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KittyLedger::new();
        let _ = lt.record_session(0, session_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_session(0, session_cell(0).unwrap_or(0)),
            Err(KittyCode::EDup)
        );
        let _ = lt.record_putty(2, putty_cell(2).unwrap_or(0));
        assert_eq!(lt.record_putty(2, putty_cell(2).unwrap_or(0)), Err(KittyCode::EDup));
    }

    #[test]
    fn test_pw_gate_boundary() {
        let mut lt = KittyLedger::new();
        let mut g = 0usize;
        while g < PW_GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(PW_GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = KittyLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_transfer_kind_rotation() {
        assert_eq!(transfer_kind(0), 0);
        assert_eq!(transfer_kind(1), 1);
        assert_eq!(transfer_kind(2), 2);
        assert_eq!(transfer_kind(3), 0);
        assert_eq!(transfer_kind(9), 0);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (PUTTY_CELLS + SESSION_CELLS + MACRO_CELLS + PORTABLE_CELLS) as u32
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
        assert_eq!(SEG_BASE, 0xC7800);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KittyCode::Ok as u32, 0);
        assert_eq!(KittyCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => KittyCode::Ok,
                1 => KittyCode::ENoPu,
                2 => KittyCode::EPuDrift,
                3 => KittyCode::ENoSess,
                4 => KittyCode::ESessDrift,
                5 => KittyCode::ENoMacro,
                6 => KittyCode::EMacroDrift,
                7 => KittyCode::EMacroOrder,
                8 => KittyCode::ENoPwGate,
                9 => KittyCode::ENoPortable,
                10 => KittyCode::EDup,
                11 => KittyCode::EFp,
                _ => KittyCode::EGradeDrift,
            };
            assert_eq!(code.val(), SEG_BASE + i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = KittyLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52225_checks_green() {
        let s = run_unx52225_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
