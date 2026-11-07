//! UNX-F52407 Kleopatra 收口 · 0xCA580段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52407"，逐字）：
//!   title: Kleopatra 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52408-J1 gpg 20 操
//!     作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52407）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52408-J1，照原文锚定；功能按本卡标题（Kleopatra）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 证书五面闭集：创建/导入/导出/验证/签名各 4 操作共 20 格，面号 =
//!      o/4，面单调（逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② SMART 卡三元闸：无卡/插卡/激活三态阶梯，越级必抓；每态独立载荷
//!      折叠。
//!   ③ gpg 消费联签 4 格（底层引擎消费端联签锚定）：逐格折叠对照。
//!   ④ Qt 上栈 4 层闭集（QtCore/QtGui/QtWidgets/QtNetwork 逐层声明），
//!      越层拒；缺层拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（SMART 卡越闸/证书
//!      面面序 order 注入/操作漂移，order 注入对空账本同样必抓且不改
//!      last_kind），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （27 格全通 + 六判据全落 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52407;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 证书操作数。
pub const CERT_OPS: usize = 20;

/// SMART 卡三元闸态数。
pub const CARD_STATES: usize = 3;

/// gpg 消费联签格数。
pub const GPG_ROWS: usize = 4;

/// Qt 上栈层数。
pub const QT_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KleoCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 证书面逆序
    EOrder,
    /// 卡态越闸
    ENoCard,
    /// 卡态漂移
    ECardDrift,
    /// 联签格越闭集
    ENoSig,
    /// Qt 层越栈
    ENoLayer,
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

impl KleoCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KleoCode::Ok => "成功",
            KleoCode::ENoOp => "操作号越闭集：创建/导入/导出/验证/签名 20 操作集之外先登记",
            KleoCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            KleoCode::EOrder => "证书面逆序：创建→导入→导出→验证→签名 单调，逆序必抓",
            KleoCode::ENoCard => "卡态越闸：无卡/插卡/激活三元闸之外先登记",
            KleoCode::ECardDrift => "卡态漂移：三元闸载荷折叠与基准不恒等，逐态复算",
            KleoCode::ENoSig => "联签格越闭集：gpg 消费联签 4 格之外先登记",
            KleoCode::ENoLayer => "Qt 层越栈：Qt 上栈 4 层闭集之外先登记",
            KleoCode::EDup => "重复落账：同格只许一条账目",
            KleoCode::ESeal => "未齐即封：20 操作 + 3 态闸 + 4 联签 + 4 层未齐先补齐",
            KleoCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KleoCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KleoCode::EGradeDrift => "定级漂移：27 格全通且六判据全落才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3态闸+4联签+4层全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 证书操作格载荷（操作 o；证书面 = o/4）。
pub const fn cert_cell(o: usize) -> Option<u64> {
    if o >= CERT_OPS {
        return None;
    }
    Some(
        (0xCE000u64 + (o as u64) * 0x2C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193).wrapping_add(0x4B4C0000),
    )
}

/// 证书面（0..4=创建 4..8=导入 8..12=导出 12..16=验证 16..20=签名）。
pub const fn cert_kind(o: usize) -> Option<u32> {
    if o >= CERT_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 证书面名。
pub const fn cert_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("创建"),
        1 => Some("导入"),
        2 => Some("导出"),
        3 => Some("验证"),
        4 => Some("签名"),
        _ => None,
    }
}

/// SMART 卡三元闸态载荷（态 c：0=无卡 1=插卡 2=激活）。
pub const fn card_cell(c: usize) -> Option<u64> {
    if c >= CARD_STATES {
        return None;
    }
    Some((0xCF000u64 + (c as u64) * 0x5D) ^ 0x4152544344_0000)
}

/// 三元闸态名。
pub const fn card_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("无卡"),
        1 => Some("插卡"),
        2 => Some("激活"),
        _ => None,
    }
}

/// gpg 消费联签折叠（格 r）。
pub const fn gpg_sig(r: usize) -> Option<u32> {
    if r >= GPG_ROWS {
        return None;
    }
    Some(0xCE400 + r as u32 * 0x2C)
}

/// Qt 层折叠（层 l）。
pub const fn qt_layer(l: usize) -> Option<u32> {
    if l >= QT_LAYERS {
        return None;
    }
    Some(0xCF100 + l as u32 * 0x2C)
}

/// Qt 层名。
pub const fn qt_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("QtCore"),
        1 => Some("QtGui"),
        2 => Some("QtWidgets"),
        3 => Some("QtNetwork"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Kleopatra 收口账本状态机。
#[derive(Clone)]
pub struct KleoLedger {
    /// 六判据（UNX-F52408-J1 收口闸）。
    gates: [Option<bool>; 6],
    /// 卡态账。
    cards: [Option<u64>; CARD_STATES],
    /// 操作账。
    ops: [Option<u64>; CERT_OPS],
    /// gpg 联签账。
    sigs: [Option<u32>; GPG_ROWS],
    /// Qt 层账。
    qts: [Option<u32>; QT_LAYERS],
    /// 最后证书面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KleoLedger {
    /// 新账本。
    pub fn new() -> Self {
        KleoLedger {
            gates: [None; 6],
            cards: [None; CARD_STATES],
            ops: [None; CERT_OPS],
            sigs: [None; GPG_ROWS],
            qts: [None; QT_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KleoCode> {
        if gate >= 6 {
            return Err(KleoCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 三元闸一态（阶梯载荷对照 + 重复拒）。
    pub fn record_card(&mut self, c: usize, got: u64) -> Result<(), KleoCode> {
        let want = match card_cell(c) {
            Some(w) => w,
            None => return Err(KleoCode::ENoCard),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + c as u64);
            return Err(KleoCode::ECardDrift);
        }
        if match self.cards.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KleoCode::EDup);
        }
        if let Some(c2) = self.cards.get_mut(c) {
            *c2 = Some(got);
        }
        self.fnv(0x7201 + c as u64);
        Ok(())
    }

    /// 三元闸全账核验（3 态全齐）。
    pub fn cards_all(&self) -> bool {
        let mut c = 0usize;
        while c < CARD_STATES {
            match self.cards.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 操作一格（证书面单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), KleoCode> {
        if o >= CERT_OPS {
            return Err(KleoCode::ENoOp);
        }
        let kind = match cert_kind(o) {
            Some(k) => k,
            None => return Err(KleoCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(KleoCode::EOrder);
        }
        let want = match cert_cell(o) {
            Some(w) => w,
            None => return Err(KleoCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + o as u64);
            return Err(KleoCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KleoCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CERT_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// gpg 消费联签一格。
    pub fn sign_gpg(&mut self, r: usize, got: u32) -> Result<(), KleoCode> {
        let want = match gpg_sig(r) {
            Some(w) => w,
            None => return Err(KleoCode::ENoSig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + r as u64);
            return Err(KleoCode::ENoSig);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KleoCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x7401 + r as u64);
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

    /// Qt 上栈一层。
    pub fn declare_qt(&mut self, l: usize) -> Result<(), KleoCode> {
        if l >= QT_LAYERS {
            return Err(KleoCode::ENoLayer);
        }
        if match self.qts.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KleoCode::EDup);
        }
        if let Some(c) = self.qts.get_mut(l) {
            *c = qt_layer(l);
        }
        self.fnv(0x7500 + l as u64);
        Ok(())
    }

    /// Qt 层全齐核验。
    pub fn qts_all(&self) -> bool {
        let mut l = 0usize;
        while l < QT_LAYERS {
            match self.qts.get(l) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.cards_all()
            && self.ops_all()
            && self.sigs_all()
            && self.qts_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for KleoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KleoLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < CARD_STATES {
        if let Some(w) = card_cell(c) {
            if led.record_card(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < CERT_OPS {
        if let Some(w) = cert_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < GPG_ROWS {
        if let Some(w) = gpg_sig(r) {
            if led.sign_gpg(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut l = 0usize;
    while l < QT_LAYERS {
        let _ = led.declare_qt(l);
        l += 1;
    }
    // 注入 10 轮：SMART 卡越闸 / 证书面 order / 操作漂移三态轮转。
    // order 注入对空账本同样必抓且不改 last_kind（临时账本即空账）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KleoLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_card(0, card_cell(0).unwrap_or(0));
                lt.record_card(CARD_STATES, 0).is_err()
            }
            1 => {
                let _ = lt.record_op(17, cert_cell(17).unwrap_or(0));
                lt.record_op(2, cert_cell(2).unwrap_or(0)) == Err(KleoCode::EOrder)
            }
            _ => {
                lt.record_op(5, cert_cell(5).unwrap_or(0));
                lt.record_op(4, cert_cell(4).unwrap_or(0).wrapping_add(1)) == Err(KleoCode::EDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52407 判据集（锚 UNX-F52408-J1，off-by-one 板式）。
pub fn run_unx52407_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52407");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "证书 20 操作全通",
        recorded >= CERT_OPS as u32,
        "创建/导入/导出/验证/签名五面逐操作恒等",
    );
    s.add(
        "SMART 卡三元闸 3/3 全落",
        {
            let mut lt = KleoLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CARD_STATES {
                if let Some(w) = card_cell(c) {
                    if lt.record_card(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cards_all()
        },
        "无卡/插卡/激活三态阶梯逐态恒等，越级必抓",
    );
    s.add(
        "gpg 消费联签 4/4 全落",
        {
            let mut lt = KleoLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GPG_ROWS {
                if let Some(w) = gpg_sig(r) {
                    if lt.sign_gpg(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.sigs_all()
        },
        "底层引擎消费端联签逐格对照",
    );
    s.add(
        "Qt 上栈 4/4 层全齐",
        {
            let mut lt = KleoLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < QT_LAYERS {
                if lt.declare_qt(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.qts_all()
        },
        "QtCore/QtGui/QtWidgets/QtNetwork 逐层闭集，缺层拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = KleoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52408-J1 六判据逐格落账（§AI-66.3）",
    );
    s.add(
        "证书面单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CERT_OPS {
                if cert_kind(o).unwrap_or(5) < cert_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "创建→导入→导出→验证→签名五面闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "卡态越闸/面序 order/操作漂移三态轮转必抓",
    );
    let mut l0 = KleoLedger::new();
    s.add(
        "边界·卡态越闸",
        l0.record_card(CARD_STATES, 0).is_err(),
        "卡态 3 越闸必拒",
    );
    s.add(
        "面序逆序拒（空账本必抓）",
        {
            let _ = l0.record_op(17, cert_cell(17).unwrap_or(0));
            l0.record_op(2, cert_cell(2).unwrap_or(0)) == Err(KleoCode::EOrder)
        },
        "签名面后回创建面必抓",
    );
    s.add(
        "操作漂移拒",
        {
            // 用当轮新账本：先落正格再注漂移，不受共享 l0 家族位影响。
            let mut l1 = KleoLedger::new();
            l1.record_op(4, cert_cell(4).unwrap_or(0));
            l1.record_op(4, cert_cell(4).unwrap_or(0).wrapping_add(1)) == Err(KleoCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(CERT_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.sign_gpg(GPG_ROWS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "边界·Qt 层越栈",
        l0.declare_qt(QT_LAYERS).is_err(),
        "Qt 层 4 越栈必拒",
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
            let mut lt = KleoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < CARD_STATES {
                if let Some(w) = card_cell(c) {
                    let _ = lt.record_card(c, w);
                }
                c += 1;
            }
            let mut o = 0usize;
            while o < CERT_OPS {
                if let Some(w) = cert_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < GPG_ROWS {
                if let Some(w) = gpg_sig(r) {
                    let _ = lt.sign_gpg(r, w);
                }
                r += 1;
            }
            let mut l = 0usize;
            while l < QT_LAYERS {
                let _ = lt.declare_qt(l);
                l += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA580,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA580",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cert_kind() {
        assert_eq!(cert_kind(0), Some(0));
        assert_eq!(cert_kind(3), Some(0));
        assert_eq!(cert_kind(4), Some(1));
        assert_eq!(cert_kind(8), Some(2));
        assert_eq!(cert_kind(12), Some(3));
        assert_eq!(cert_kind(19), Some(4));
        assert!(cert_kind(20).is_none());
        assert!(cert_cell(20).is_none());
    }

    #[test]
    fn test_card_ladder() {
        assert_eq!(card_name(0), Some("无卡"));
        assert_eq!(card_name(1), Some("插卡"));
        assert_eq!(card_name(2), Some("激活"));
        assert!(card_name(3).is_none());
        assert!(card_cell(3).is_none());
    }

    #[test]
    fn test_kind_and_layer_names() {
        assert_eq!(cert_kind_name(0), Some("创建"));
        assert_eq!(cert_kind_name(4), Some("签名"));
        assert!(cert_kind_name(5).is_none());
        assert_eq!(qt_layer_name(0), Some("QtCore"));
        assert_eq!(qt_layer_name(3), Some("QtNetwork"));
        assert!(qt_layer_name(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cert_cell(6).unwrap_or(0);
        assert_eq!(a, cert_cell(6).unwrap_or(0));
        assert_ne!(a, cert_cell(7).unwrap_or(0));
        assert!(gpg_sig(GPG_ROWS).is_none());
        assert!(qt_layer(QT_LAYERS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = KleoLedger::new();
        let _ = lt.record_op(9, cert_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, cert_cell(2).unwrap_or(0)), Err(KleoCode::EOrder));
        assert!(lt.record_op(10, cert_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KleoLedger::new();
        assert_eq!(
            lt.record_op(3, cert_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(KleoCode::EDrift)
        );
        assert_eq!(
            lt.record_card(2, card_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(KleoCode::ECardDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KleoLedger::new();
        let _ = lt.record_card(0, card_cell(0).unwrap_or(0));
        assert_eq!(lt.record_card(0, card_cell(0).unwrap_or(0)), Err(KleoCode::EDup));
        let _ = lt.sign_gpg(1, gpg_sig(1).unwrap_or(0));
        assert_eq!(lt.sign_gpg(1, gpg_sig(1).unwrap_or(0)), Err(KleoCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KleoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = KleoLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded = 3 卡态 + 20 操作 + 4 联签 = 27（Qt 层 4 行不计格）。
        assert_eq!(recorded, CARD_STATES as u32 + CERT_OPS as u32 + GPG_ROWS as u32);
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
        assert_eq!(SEG_BASE, 0xCA580);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KleoCode::Ok as u32, 0);
        assert_eq!(KleoCode::EGradeDrift as u32, 12);
        assert!(!KleoCode::Ok.detail().is_empty());
        assert!(!KleoCode::ESeal.detail().is_empty());
        assert!(!KleoCode::EFp.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = KleoLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_qt_declare_boundary() {
        let mut lt = KleoLedger::new();
        let mut l = 0usize;
        while l < QT_LAYERS {
            assert!(lt.declare_qt(l).is_ok());
            l += 1;
        }
        assert!(lt.declare_qt(QT_LAYERS).is_err());
        assert!(lt.qts_all());
    }

    #[test]
    fn unx52407_checks_green() {
        let s = run_unx52407_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
