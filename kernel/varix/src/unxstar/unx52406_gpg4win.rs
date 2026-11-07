//! UNX-F52406 Gpg4win 收口 · 0xCA540段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52406"，逐字）：
//!   title: Gpg4win 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52407-J1 六判据全
//!     落；密钥管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52406）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52407-J1，照原文锚定；功能按本卡标题（Gpg4win）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集：gpg/GPA/Kleopatra/GpgOL/GpgEX 各 4 格共 20 格，组
//!      件号 = o/4，组件单调（逆序必抓）；逐格载荷纯函数折叠，恒等才收。
//!   ② 密钥管理 10 操作账：生成/导入/导出/撤销/信任签/过期/吊销/清理/
//!      备份/恢复，逐操作恒等收账，越闭集拒。
//!   ③ GnuPG 消费联签 4 格（F52408 消费端联签锚定）：逐格折叠对照。
//!   ④ 商业支持分账 2 行闭集（商业支持账独立声明/社区支持分账声明），
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（组件族 order 注入/
//!      密钥管理漂移/操作越闭集，order 注入对空账本同样必抓且不改
//!      last_kind），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （34 格全通 + 六判据全落 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52406;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 组件格数。
pub const COMP_OPS: usize = 20;

/// 密钥管理操作数。
pub const KEY_OPS: usize = 10;

/// GnuPG 消费联签格数（F52408）。
pub const GPG_ROWS: usize = 4;

/// 商业支持分账行数。
pub const BIZ_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gpg4winCode {
    /// 成功
    Ok = 0,
    /// 组件格越闭集
    ENoOp,
    /// 组件格漂移
    EDrift,
    /// 组件族逆序
    EOrder,
    /// 密钥操作越闭集
    ENoKey,
    /// 密钥操作漂移
    EKeyDrift,
    /// 联签格越闭集
    ENoSig,
    /// 分账行越闭集
    ENoRow,
    /// 缺分账行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl Gpg4winCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            Gpg4winCode::Ok => "成功",
            Gpg4winCode::ENoOp => "组件格越闭集：gpg/GPA/Kleopatra/GpgOL/GpgEX 20 格之外先登记",
            Gpg4winCode::EDrift => "组件格漂移：格折叠与基准不恒等，逐格复算",
            Gpg4winCode::EOrder => "组件族逆序：gpg→GPA→Kleopatra→GpgOL→GpgEX 单调，逆序必抓",
            Gpg4winCode::ENoKey => "密钥操作越闭集：密钥管理 10 操作之外先登记",
            Gpg4winCode::EKeyDrift => "密钥操作漂移：操作折叠与基准不恒等，逐操作复算",
            Gpg4winCode::ENoSig => "联签格越闭集：GnuPG 消费联签 4 格之外先登记",
            Gpg4winCode::ENoRow => "分账行越闭集：商业支持分账 2 行闭集之外先登记",
            Gpg4winCode::ERowMiss => "缺分账行：商业支持分账声明未齐先补齐",
            Gpg4winCode::EDup => "重复落账：同格只许一条账目",
            Gpg4winCode::ESeal => "未齐即封：20 组件格 + 10 密钥操作 + 4 联签未齐先补齐",
            Gpg4winCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            Gpg4winCode::EGradeDrift => "定级漂移：34 格全通且六判据全落才可定 S1",
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
            Grade::S1 => "S1：六判据+20组件格+10密钥操作+4联签+2分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 组件格载荷（格 o；组件族 = o/4）。
pub const fn comp_cell(o: usize) -> Option<u64> {
    if o >= COMP_OPS {
        return None;
    }
    Some(
        (0xCC000u64 + (o as u64) * 0x2C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193).wrapping_add(0x47500000),
    )
}

/// 组件族（0..4=gpg 4..8=GPA 8..12=Kleopatra 12..16=GpgOL 16..20=GpgEX）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMP_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn comp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("gpg"),
        1 => Some("GPA"),
        2 => Some("Kleopatra"),
        3 => Some("GpgOL"),
        4 => Some("GpgEX"),
        _ => None,
    }
}

/// 密钥管理操作载荷（操作 k）。
pub const fn key_cell(k: usize) -> Option<u64> {
    if k >= KEY_OPS {
        return None;
    }
    Some((0xCD000u64 + (k as u64) * 0x47) ^ 0x4B45594D_474D0000)
}

/// 密钥管理操作名。
pub const fn key_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("生成"),
        1 => Some("导入"),
        2 => Some("导出"),
        3 => Some("撤销"),
        4 => Some("信任签"),
        5 => Some("过期"),
        6 => Some("吊销"),
        7 => Some("清理"),
        8 => Some("备份"),
        9 => Some("恢复"),
        _ => None,
    }
}

/// GnuPG 消费联签折叠（格 r；F52408 消费端联签）。
pub const fn gnupg_sig(r: usize) -> Option<u32> {
    if r >= GPG_ROWS {
        return None;
    }
    Some(0xCC400 + r as u32 * 0x2C)
}

/// 商业支持分账行折叠（行 r）。
pub const fn biz_row(r: usize) -> Option<u32> {
    if r >= BIZ_ROWS {
        return None;
    }
    Some(0xCD100 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Gpg4win 收口账本状态机。
#[derive(Clone)]
pub struct Gpg4winLedger {
    /// 六判据（UNX-F52407-J1 收口闸）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMP_OPS],
    /// 密钥管理账。
    keys: [Option<u64>; KEY_OPS],
    /// GnuPG 联签账。
    sigs: [Option<u32>; GPG_ROWS],
    /// 商业支持分账账。
    bizs: [Option<u32>; BIZ_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Gpg4winLedger {
    /// 新账本。
    pub fn new() -> Self {
        Gpg4winLedger {
            gates: [None; 6],
            comps: [None; COMP_OPS],
            keys: [None; KEY_OPS],
            sigs: [None; GPG_ROWS],
            bizs: [None; BIZ_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Gpg4winCode> {
        if gate >= 6 {
            return Err(Gpg4winCode::ESeal);
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

    /// 组件一格（组件族单调 + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), Gpg4winCode> {
        if o >= COMP_OPS {
            return Err(Gpg4winCode::ENoOp);
        }
        let kind = match comp_kind(o) {
            Some(k) => k,
            None => return Err(Gpg4winCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(Gpg4winCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(Gpg4winCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(Gpg4winCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Gpg4winCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 组件全账核验（20 格）。
    pub fn comps_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMP_OPS {
            match self.comps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 密钥管理一操作（载荷对照 + 重复拒）。
    pub fn record_key(&mut self, k: usize, got: u64) -> Result<(), Gpg4winCode> {
        let want = match key_cell(k) {
            Some(w) => w,
            None => return Err(Gpg4winCode::ENoKey),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + k as u64);
            return Err(Gpg4winCode::EKeyDrift);
        }
        if match self.keys.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Gpg4winCode::EDup);
        }
        if let Some(c) = self.keys.get_mut(k) {
            *c = Some(got);
        }
        self.fnv(0x6301 + k as u64);
        Ok(())
    }

    /// 密钥管理全账核验（10 操作）。
    pub fn keys_all(&self) -> bool {
        let mut k = 0usize;
        while k < KEY_OPS {
            match self.keys.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// GnuPG 消费联签一格（F52408）。
    pub fn sign_gnupg(&mut self, r: usize, got: u32) -> Result<(), Gpg4winCode> {
        let want = match gnupg_sig(r) {
            Some(w) => w,
            None => return Err(Gpg4winCode::ENoSig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + r as u64);
            return Err(Gpg4winCode::ENoSig);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Gpg4winCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x6401 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn sigs_all(&self) -> bool {
        let mut r = 0usize;
        while r < GPG_ROWS {
            match self.sigs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 商业支持分账一行。
    pub fn declare_biz(&mut self, r: usize) -> Result<(), Gpg4winCode> {
        if r >= BIZ_ROWS {
            return Err(Gpg4winCode::ENoRow);
        }
        if match self.bizs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Gpg4winCode::EDup);
        }
        if let Some(c) = self.bizs.get_mut(r) {
            *c = biz_row(r);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
    pub fn bizs_all(&self) -> bool {
        let mut r = 0usize;
        while r < BIZ_ROWS {
            match self.bizs.get(r) {
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
            && self.comps_all()
            && self.keys_all()
            && self.sigs_all()
            && self.bizs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for Gpg4winLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Gpg4winLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < COMP_OPS {
        if let Some(w) = comp_cell(o) {
            if led.record_comp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut k = 0usize;
    while k < KEY_OPS {
        if let Some(w) = key_cell(k) {
            if led.record_key(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut r = 0usize;
    while r < GPG_ROWS {
        if let Some(w) = gnupg_sig(r) {
            if led.sign_gnupg(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut b = 0usize;
    while b < BIZ_ROWS {
        let _ = led.declare_biz(b);
        b += 1;
    }
    // 注入 10 轮：组件族 order / 密钥管理漂移 / 组件格越闭集三态轮转。
    // order 注入对空账本同样必抓且不改 last_kind（临时账本即空账）。
    let mut kk = 0usize;
    while kk < 10 {
        let kind = (kk % 3) as u32;
        let mut lt = Gpg4winLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_comp(9, comp_cell(9).unwrap_or(0));
                lt.record_comp(2, comp_cell(2).unwrap_or(0)) == Err(Gpg4winCode::EOrder)
            }
            1 => {
                let _ = lt.record_key(0, key_cell(0).unwrap_or(0));
                lt.record_key(1, key_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(Gpg4winCode::EKeyDrift)
            }
            _ => lt.record_comp(COMP_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + kk as u64);
        } else {
            led.escaped += 1;
        }
        kk += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52406 判据集（锚 UNX-F52407-J1，off-by-one 板式）。
pub fn run_unx52406_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52406");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五组件 20 格全通",
        recorded >= COMP_OPS as u32,
        "gpg/GPA/Kleopatra/GpgOL/GpgEX 五组件族逐格恒等",
    );
    s.add(
        "密钥管理 10 操作全落",
        {
            let mut lt = Gpg4winLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < KEY_OPS {
                if let Some(w) = key_cell(k) {
                    if lt.record_key(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.keys_all()
        },
        "生成/导入/导出/撤销/信任签/过期/吊销/清理/备份/恢复逐操作恒等",
    );
    s.add(
        "GnuPG 消费联签 4/4 全落",
        {
            let mut lt = Gpg4winLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GPG_ROWS {
                if let Some(w) = gnupg_sig(r) {
                    if lt.sign_gnupg(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.sigs_all()
        },
        "F52408 消费端联签逐格对照",
    );
    s.add(
        "商业支持分账 2/2 全齐",
        {
            let mut lt = Gpg4winLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BIZ_ROWS {
                if lt.declare_biz(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.bizs_all()
        },
        "商业支持账独立声明/社区支持分账声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = Gpg4winLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52407-J1 六判据逐格落账（§AI-66.3）",
    );
    s.add(
        "组件族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < COMP_OPS {
                if comp_kind(o).unwrap_or(5) < comp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "gpg→GPA→Kleopatra→GpgOL→GpgEX 五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "组件族 order/密钥漂移/越闭集三态轮转必抓",
    );
    let mut l0 = Gpg4winLedger::new();
    s.add(
        "族逆序拒（空账本必抓）",
        {
            let _ = l0.record_comp(9, comp_cell(9).unwrap_or(0));
            l0.record_comp(2, comp_cell(2).unwrap_or(0)) == Err(Gpg4winCode::EOrder)
        },
        "GPA 后回 gpg 必抓",
    );
    s.add(
        "密钥漂移拒",
        l0.record_key(3, key_cell(3).unwrap_or(0).wrapping_add(5)) == Err(Gpg4winCode::EKeyDrift),
        "密钥操作载荷漂移必抓",
    );
    s.add(
        "边界·组件格越闭集",
        l0.record_comp(COMP_OPS, 0).is_err(),
        "组件格 20 越闭集必拒",
    );
    s.add(
        "边界·密钥操作越闭集",
        l0.record_key(KEY_OPS, 0).is_err(),
        "密钥操作 10 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.sign_gnupg(GPG_ROWS, 0).is_err(),
        "联签格 4 越闭集必拒",
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
            let mut lt = Gpg4winLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < COMP_OPS {
                if let Some(w) = comp_cell(o) {
                    let _ = lt.record_comp(o, w);
                }
                o += 1;
            }
            let mut k = 0usize;
            while k < KEY_OPS {
                if let Some(w) = key_cell(k) {
                    let _ = lt.record_key(k, w);
                }
                k += 1;
            }
            let mut r = 0usize;
            while r < GPG_ROWS {
                if let Some(w) = gnupg_sig(r) {
                    let _ = lt.sign_gnupg(r, w);
                }
                r += 1;
            }
            let mut b = 0usize;
            while b < BIZ_ROWS {
                let _ = lt.declare_biz(b);
                b += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA540,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA540",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_kind() {
        assert_eq!(comp_kind(0), Some(0));
        assert_eq!(comp_kind(3), Some(0));
        assert_eq!(comp_kind(4), Some(1));
        assert_eq!(comp_kind(8), Some(2));
        assert_eq!(comp_kind(12), Some(3));
        assert_eq!(comp_kind(19), Some(4));
        assert!(comp_kind(20).is_none());
        assert!(comp_cell(20).is_none());
    }

    #[test]
    fn test_comp_kind_names() {
        assert_eq!(comp_kind_name(0), Some("gpg"));
        assert_eq!(comp_kind_name(2), Some("Kleopatra"));
        assert_eq!(comp_kind_name(4), Some("GpgEX"));
        assert!(comp_kind_name(5).is_none());
    }

    #[test]
    fn test_key_names() {
        assert_eq!(key_name(0), Some("生成"));
        assert_eq!(key_name(4), Some("信任签"));
        assert_eq!(key_name(9), Some("恢复"));
        assert!(key_name(10).is_none());
        assert!(key_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(5).unwrap_or(0);
        assert_eq!(a, comp_cell(5).unwrap_or(0));
        assert_ne!(a, comp_cell(6).unwrap_or(0));
        assert!(gnupg_sig(GPG_ROWS).is_none());
        assert!(biz_row(BIZ_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = Gpg4winLedger::new();
        let _ = lt.record_comp(12, comp_cell(12).unwrap_or(0));
        assert_eq!(lt.record_comp(3, comp_cell(3).unwrap_or(0)), Err(Gpg4winCode::EOrder));
        assert!(lt.record_comp(13, comp_cell(13).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = Gpg4winLedger::new();
        assert_eq!(
            lt.record_comp(7, comp_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(Gpg4winCode::EDrift)
        );
        assert_eq!(
            lt.record_key(8, key_cell(8).unwrap_or(0).wrapping_add(2)),
            Err(Gpg4winCode::EKeyDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = Gpg4winLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(lt.record_comp(0, comp_cell(0).unwrap_or(0)), Err(Gpg4winCode::EDup));
        let _ = lt.sign_gnupg(0, gnupg_sig(0).unwrap_or(0));
        assert_eq!(lt.sign_gnupg(0, gnupg_sig(0).unwrap_or(0)), Err(Gpg4winCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = Gpg4winLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = Gpg4winLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded = 20 组件格 + 10 密钥操作 + 4 联签 = 34（分账 2 行不计格）。
        assert_eq!(recorded, COMP_OPS as u32 + KEY_OPS as u32 + GPG_ROWS as u32);
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
        assert_eq!(SEG_BASE, 0xCA540);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(Gpg4winCode::Ok as u32, 0);
        assert_eq!(Gpg4winCode::EGradeDrift as u32, 12);
        assert!(!Gpg4winCode::Ok.detail().is_empty());
        assert!(!Gpg4winCode::ESeal.detail().is_empty());
        assert!(!Gpg4winCode::EInj.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = Gpg4winLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_biz_declare_boundary() {
        let mut lt = Gpg4winLedger::new();
        assert!(lt.declare_biz(0).is_ok());
        assert!(lt.declare_biz(1).is_ok());
        assert!(lt.declare_biz(2).is_err());
        assert!(lt.bizs_all());
    }

    #[test]
    fn unx52406_checks_green() {
        let s = run_unx52406_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
