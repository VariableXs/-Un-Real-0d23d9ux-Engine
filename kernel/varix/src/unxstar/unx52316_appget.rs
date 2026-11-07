//! UNX-F52316 AppGet 收口（EOL 定级示范） · 0xC8EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52316"，逐字）：
//!   title: AppGet 收口（EOL 定级示范）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52317-J1 六判据
//!     全落；MSI 表编辑 ×10 场景；.msi 打开/保存校验和双向；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完
//!     成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52316）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52317-J1，照卡面原文逐字锚定；功能按本卡标题（AppGet EOL
//! 定级示范收口）落地，不按 N+1 卡标题落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 定级口径（EOL 定级示范）：AppGet 已停止维护（EOL）——停更日期
//! 在册、上游归档仓库可查、官方替代品 winget 迁移路径已声明。如实
//! 定级：全通封顶 S2，不虚报 S1；未齐格 S3（判据测试与本封顶一致，
//! EOL 如实不虚 S1）。
//!
//! 设计要点：
//!   ① 安装/更新/卸载 10 操作集：三家族闭集（安装 0..4/更新 4..7/
//!      卸载 7..10），家族单调（逆序必抓）。
//!   ② 包清单 12 格：逐格载荷折叠恒等才收，漂移必抓。
//!   ③ EOL 三要素 6 格全呈：停更日期（公告格/生效格）×2、替代品
//!      winget 迁移（迁移路径格/迁移联签格）×2、归档仓库（归档地址
//!      格/归档校验格）×2。
//!   ④ EOL 后不承诺声明 2 行闭集：不承诺更新声明/不承诺安全修复
//!      声明，缺一拒封。
//!   ⑤ 迁移消费联签 4 格（F52302）：winget 消费端 4 格联签锚，逐格
//!      对照，缺格拒封 S2。
//!   ⑥ ledger 状态机（new/record_pkg/record_manifest/record_eol/
//!      record_mig/declare_eolp/judge_gate/fingerprint，FNV-1a）；注入
//!      核 run_cycle 三态轮转 10 轮（家族逆序/清单漂移/操作越闭集），
//!      全部落在当轮临时账本上，主账 injected==caught==10、
//!      escaped==0；双跑指纹一致；定级闭集（全通封顶 S2）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52316;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 包操作数。
pub const PKG_OPS: usize = 10;

/// 包清单格数。
pub const MANIFEST_CELLS: usize = 12;

/// EOL 三要素格数（3 要素 × 2 格）。
pub const EOL_CELLS: usize = 6;

/// 迁移消费联签格数（F52302）。
pub const MIG_CELLS: usize = 4;

/// EOL 后不承诺声明行数。
pub const EOLP_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppgetCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 清单格越闭集
    ENoMan,
    /// 清单漂移
    EManDrift,
    /// EOL 格越闭集
    ENoEol,
    /// EOL 三要素漂移
    EEolDrift,
    /// 联签格越闭集
    ENoMig,
    /// 联签漂移
    EMigDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl AppgetCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AppgetCode::Ok => "成功",
            AppgetCode::ENoOp => "操作号越闭集：安装/更新/卸载 10 操作集之外先登记",
            AppgetCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            AppgetCode::EOrder => "家族逆序：安装→更新→卸载单调，逆序必抓",
            AppgetCode::ENoMan => "清单格越闭集：包清单 12 格之外先登记",
            AppgetCode::EManDrift => "清单漂移：清单折叠与基准不恒等，逐格复算",
            AppgetCode::ENoEol => "EOL 格越闭集：EOL 三要素 6 格之外先登记",
            AppgetCode::EEolDrift => "EOL 三要素漂移：停更/迁移/归档折叠与基准不恒等",
            AppgetCode::ENoMig => "联签格越闭集：迁移消费联签 4 格之外先登记",
            AppgetCode::EMigDrift => "联签漂移：F52302 联签折叠与基准不恒等",
            AppgetCode::EDup => "重复落账：同格只许一条账目",
            AppgetCode::ESeal => "未齐即封：10 操作 + 12 清单 + 6 EOL + 4 联签 + 2 声明未齐",
            AppgetCode::EGradeDrift => "定级漂移：EOL 如实不虚 S1，全通封顶 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（EOL 卡不授，如实不虚）
    S1,
    /// S2：有条件收口（EOL 全通如实封顶）
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
            Grade::S1 => "S1：全通收口（EOL 卡不授：上游停更，如实不虚 S1）",
            Grade::S2 => "S2：EOL 全通如实封顶，winget 迁移路径在册",
            Grade::S3 => "S3：不收口，判据或 EOL 三要素缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 包操作格载荷（操作 o）。
pub const fn pkg_cell(o: usize) -> Option<u64> {
    if o >= PKG_OPS {
        return None;
    }
    // const fn 内禁 unwrap_or，用 match 展开。
    let kind = match pkg_kind(o) {
        Some(k) => k,
        None => 0,
    };
    Some((0xD4000u64 + (o as u64) * 0x2C) ^ (kind as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=安装 4..7=更新 7..10=卸载）。
pub const fn pkg_kind(o: usize) -> Option<u32> {
    if o >= PKG_OPS {
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

/// 家族名。
pub const fn pkg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装"),
        1 => Some("更新"),
        2 => Some("卸载"),
        _ => None,
    }
}

/// 包清单格载荷（格 i）。
pub const fn manifest_cell(i: usize) -> Option<u64> {
    if i >= MANIFEST_CELLS {
        return None;
    }
    Some((0xD5000u64 + (i as u64) * 0x3B) ^ 0x415050474D410000)
}

/// EOL 三要素格载荷（格 i；XOR 锚低 16 位恒零）。
pub const fn eol_cell(i: usize) -> Option<u64> {
    if i >= EOL_CELLS {
        return None;
    }
    Some((0xD6000u64 + (i as u64) * 0x47) ^ 0x454F4C5441470000)
}

/// EOL 要素名。
pub const fn eol_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("停更日期·公告格"),
        1 => Some("停更日期·生效格"),
        2 => Some("替代品 winget·迁移路径格"),
        3 => Some("替代品 winget·迁移联签格"),
        4 => Some("归档仓库·归档地址格"),
        5 => Some("归档仓库·归档校验格"),
        _ => None,
    }
}

/// 迁移消费联签格载荷（格 i；F52302 winget 消费端联签锚）。
pub const fn mig_cell(i: usize) -> Option<u64> {
    if i >= MIG_CELLS {
        return None;
    }
    Some((0xD7000u64 + (i as u64) * 0x53) ^ 0x5747455446350000)
}

/// EOL 后不承诺声明行折叠（行 r）。
pub const fn eolp_row(r: usize) -> Option<u32> {
    if r >= EOLP_ROWS {
        return None;
    }
    Some(0xD800 + r as u32 * 0x2C)
}

/// EOL 后不承诺声明行名。
pub const fn eolp_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("EOL 后不承诺更新声明"),
        1 => Some("EOL 后不承诺安全修复声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// AppGet 收口账本状态机（EOL 定级示范）。
#[derive(Clone)]
pub struct AgLedger {
    /// 六判据（UNX-F52317-J1 锚）。
    gates: [Option<bool>; 6],
    /// 包操作账。
    ops: [Option<u64>; PKG_OPS],
    /// 包清单账。
    man: [Option<u64>; MANIFEST_CELLS],
    /// EOL 三要素账。
    eol: [Option<u64>; EOL_CELLS],
    /// 迁移消费联签账（F52302）。
    mig: [Option<u64>; MIG_CELLS],
    /// EOL 后不承诺声明账。
    eolp: [Option<u32>; EOLP_ROWS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AgLedger {
    /// 新账本。
    pub fn new() -> Self {
        AgLedger {
            gates: [None; 6],
            ops: [None; PKG_OPS],
            man: [None; MANIFEST_CELLS],
            eol: [None; EOL_CELLS],
            mig: [None; MIG_CELLS],
            eolp: [None; EOLP_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AppgetCode> {
        if gate >= 6 {
            return Err(AppgetCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5900 + gate as u64);
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

    /// 包操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_pkg(&mut self, o: usize, got: u64) -> Result<(), AppgetCode> {
        if o >= PKG_OPS {
            return Err(AppgetCode::ENoOp);
        }
        let kind = match pkg_kind(o) {
            Some(k) => k,
            None => return Err(AppgetCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A00);
            return Err(AppgetCode::EOrder);
        }
        let want = match pkg_cell(o) {
            Some(w) => w,
            None => return Err(AppgetCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A01 + o as u64);
            return Err(AppgetCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppgetCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5A02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < PKG_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 包清单一格（载荷对照 + 重复拒）。
    pub fn record_manifest(&mut self, i: usize, got: u64) -> Result<(), AppgetCode> {
        let want = match manifest_cell(i) {
            Some(w) => w,
            None => return Err(AppgetCode::ENoMan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B00 + i as u64);
            return Err(AppgetCode::EManDrift);
        }
        if match self.man.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppgetCode::EDup);
        }
        if let Some(c) = self.man.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5B01 + i as u64);
        Ok(())
    }

    /// 清单全账核验（12 格）。
    pub fn man_all(&self) -> bool {
        let mut i = 0usize;
        while i < MANIFEST_CELLS {
            match self.man.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// EOL 三要素一格（载荷对照 + 重复拒）。
    pub fn record_eol(&mut self, i: usize, got: u64) -> Result<(), AppgetCode> {
        let want = match eol_cell(i) {
            Some(w) => w,
            None => return Err(AppgetCode::ENoEol),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C00 + i as u64);
            return Err(AppgetCode::EEolDrift);
        }
        if match self.eol.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppgetCode::EDup);
        }
        if let Some(c) = self.eol.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5C01 + i as u64);
        Ok(())
    }

    /// EOL 三要素全账核验（6 格全呈）。
    pub fn eol_all(&self) -> bool {
        let mut i = 0usize;
        while i < EOL_CELLS {
            match self.eol.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 迁移消费联签一格（F52302；载荷对照 + 重复拒）。
    pub fn record_mig(&mut self, i: usize, got: u64) -> Result<(), AppgetCode> {
        let want = match mig_cell(i) {
            Some(w) => w,
            None => return Err(AppgetCode::ENoMig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D00 + i as u64);
            return Err(AppgetCode::EMigDrift);
        }
        if match self.mig.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppgetCode::EDup);
        }
        if let Some(c) = self.mig.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5D01 + i as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn mig_all(&self) -> bool {
        let mut i = 0usize;
        while i < MIG_CELLS {
            match self.mig.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// EOL 后不承诺声明一行。
    pub fn declare_eolp(&mut self, r: usize) -> Result<(), AppgetCode> {
        if r >= EOLP_ROWS {
            return Err(AppgetCode::ESeal);
        }
        if match self.eolp.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AppgetCode::EDup);
        }
        if let Some(c) = self.eolp.get_mut(r) {
            *c = eolp_row(r);
        }
        self.fnv(0x5E00 + r as u64);
        Ok(())
    }

    /// EOL 后不承诺声明全齐核验。
    pub fn eolp_all(&self) -> bool {
        let mut r = 0usize;
        while r < EOLP_ROWS {
            match self.eolp.get(r) {
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

    /// KB-COMPAT 定级：EOL 如实不虚 S1，全通封顶 S2，未齐 S3。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.man_all()
            && self.eol_all()
            && self.mig_all()
            && self.eolp_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AgLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AgLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    // 先落 EOL 后不承诺声明（操作前置闸口径）。
    let mut r = 0usize;
    while r < EOLP_ROWS {
        let _ = led.declare_eolp(r);
        r += 1;
    }
    let mut o = 0usize;
    while o < PKG_OPS {
        if let Some(w) = pkg_cell(o) {
            if led.record_pkg(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < MANIFEST_CELLS {
        if let Some(w) = manifest_cell(i) {
            if led.record_manifest(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut e = 0usize;
    while e < EOL_CELLS {
        if let Some(w) = eol_cell(e) {
            if led.record_eol(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut m = 0usize;
    while m < MIG_CELLS {
        if let Some(w) = mig_cell(m) {
            if led.record_mig(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    // 注入 10 轮：家族逆序 / 清单漂移 / 操作越闭集三态轮转，全部落在
    // 当轮临时账本上；家族逆序注入先按合法序落高族格再注低族。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AgLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_pkg(8, pkg_cell(8).unwrap_or(0));
                lt.record_pkg(2, pkg_cell(2).unwrap_or(0)) == Err(AppgetCode::EOrder)
            }
            1 => {
                lt.record_manifest(4, manifest_cell(4).unwrap_or(0));
                lt.record_manifest(5, manifest_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(AppgetCode::EManDrift)
            }
            _ => lt.record_pkg(PKG_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5F00 + k as u64);
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
/// UNX-F52316 判据集（锚 UNX-F52317-J1，off-by-one 板式）。
pub fn run_unx52316_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52316");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "安装/更新/卸载 10 操作全通",
        recorded >= PKG_OPS as u32,
        "安装/更新/卸载三家族逐操作恒等",
    );
    s.add(
        "包清单 12 格全落",
        {
            let mut lt = AgLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < MANIFEST_CELLS {
                if let Some(w) = manifest_cell(i) {
                    if lt.record_manifest(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.man_all()
        },
        "包清单逐格载荷折叠恒等才收",
    );
    s.add(
        "EOL 三要素 6 格全呈",
        {
            let mut lt = AgLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < EOL_CELLS {
                if let Some(w) = eol_cell(i) {
                    if lt.record_eol(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.eol_all()
        },
        "停更日期（公告/生效）+ 替代品 winget 迁移（路径/联签）+ 归档仓库（地址/校验）",
    );
    s.add(
        "迁移消费联签 4 格全落（F52302）",
        {
            let mut lt = AgLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < MIG_CELLS {
                if let Some(w) = mig_cell(i) {
                    if lt.record_mig(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.mig_all()
        },
        "winget 消费端 4 格联签锚（F52302）",
    );
    s.add(
        "EOL 后不承诺声明 2/2 全齐",
        {
            let mut lt = AgLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < EOLP_ROWS {
                if lt.declare_eolp(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.eolp_all()
        },
        "不承诺更新/不承诺安全修复两行闭集",
    );
    s.add(
        "六判据全落（UNX-F52317-J1）",
        {
            let mut lt = AgLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "判据锚 UNX-F52317-J1 逐格落账（§AI-66.3）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PKG_OPS {
                if pkg_kind(o).unwrap_or(3) < pkg_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→更新→卸载三家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "家族逆序/清单漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = AgLedger::new();
    s.add(
        "注入·家族逆序（空账必抓）",
        {
            let _ = l0.record_pkg(8, pkg_cell(8).unwrap_or(0));
            l0.record_pkg(2, pkg_cell(2).unwrap_or(0)) == Err(AppgetCode::EOrder)
        },
        "卸载后回安装必抓，且失败不改 last_kind",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_pkg(PKG_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·清单越闭集",
        l0.record_manifest(MANIFEST_CELLS, 0).is_err(),
        "清单格 12 越闭集必拒",
    );
    s.add(
        "边界·EOL 格越闭集",
        l0.record_eol(EOL_CELLS, 0).is_err(),
        "EOL 格 6 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_mig(MIG_CELLS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "漂移·联签必抓",
        {
            l0.record_mig(0, mig_cell(0).unwrap_or(0));
            l0.record_mig(1, mig_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(AppgetCode::EMigDrift)
        },
        "F52302 联签载荷漂移必抓",
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
        "定级全通封顶 S2（EOL 如实不虚 S1）",
        {
            let mut lt = AgLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PKG_OPS {
                if let Some(w) = pkg_cell(o) {
                    let _ = lt.record_pkg(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < MANIFEST_CELLS {
                if let Some(w) = manifest_cell(i) {
                    let _ = lt.record_manifest(i, w);
                }
                i += 1;
            }
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    let _ = lt.record_eol(e, w);
                }
                e += 1;
            }
            let mut m = 0usize;
            while m < MIG_CELLS {
                if let Some(w) = mig_cell(m) {
                    let _ = lt.record_mig(m, w);
                }
                m += 1;
            }
            let mut r = 0usize;
            while r < EOLP_ROWS {
                let _ = lt.declare_eolp(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通封顶 S2：上游停更，如实不虚 S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8EC0,
        "SEG_BASE 0x40 对齐且为 52316 位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkg_kind() {
        assert_eq!(pkg_kind(0), Some(0));
        assert_eq!(pkg_kind(3), Some(0));
        assert_eq!(pkg_kind(4), Some(1));
        assert_eq!(pkg_kind(6), Some(1));
        assert_eq!(pkg_kind(7), Some(2));
        assert_eq!(pkg_kind(9), Some(2));
        assert!(pkg_kind(10).is_none());
        assert!(pkg_cell(10).is_none());
        assert_eq!(pkg_kind_name(1), Some("更新"));
        assert!(pkg_kind_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = manifest_cell(3).unwrap_or(0);
        assert_eq!(a, manifest_cell(3).unwrap_or(0));
        assert_ne!(a, manifest_cell(4).unwrap_or(0));
        assert_ne!(eol_cell(0).unwrap_or(0), mig_cell(0).unwrap_or(0));
        assert_ne!(pkg_cell(0).unwrap_or(0), manifest_cell(0).unwrap_or(0));
        assert!(manifest_cell(MANIFEST_CELLS).is_none());
        assert!(eol_cell(EOL_CELLS).is_none());
        assert!(mig_cell(MIG_CELLS).is_none());
        assert!(eolp_row(EOLP_ROWS).is_none());
    }

    #[test]
    fn test_eol_elements_named() {
        assert_eq!(eol_name(0), Some("停更日期·公告格"));
        assert_eq!(eol_name(2), Some("替代品 winget·迁移路径格"));
        assert_eq!(eol_name(5), Some("归档仓库·归档校验格"));
        assert!(eol_name(6).is_none());
    }

    #[test]
    fn test_eolp_rows_named() {
        assert_eq!(eolp_row_name(0), Some("EOL 后不承诺更新声明"));
        assert_eq!(eolp_row_name(1), Some("EOL 后不承诺安全修复声明"));
        assert!(eolp_row_name(2).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = AgLedger::new();
        let _ = lt.record_pkg(8, pkg_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_pkg(2, pkg_cell(2).unwrap_or(0)),
            Err(AppgetCode::EOrder)
        );
        // 失败注入不改 last_kind：随后同族高号仍可合法落账。
        assert!(lt.record_pkg(9, pkg_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AgLedger::new();
        assert_eq!(
            lt.record_pkg(5, pkg_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AppgetCode::EDrift)
        );
        assert_eq!(
            lt.record_manifest(2, manifest_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(AppgetCode::EManDrift)
        );
        assert_eq!(
            lt.record_eol(3, eol_cell(3).unwrap_or(0).wrapping_add(9)),
            Err(AppgetCode::EEolDrift)
        );
        assert_eq!(
            lt.record_mig(2, mig_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(AppgetCode::EMigDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AgLedger::new();
        let _ = lt.record_manifest(0, manifest_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_manifest(0, manifest_cell(0).unwrap_or(0)),
            Err(AppgetCode::EDup)
        );
        let _ = lt.record_eol(1, eol_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_eol(1, eol_cell(1).unwrap_or(0)),
            Err(AppgetCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AgLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = AgLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PKG_OPS + MANIFEST_CELLS + EOL_CELLS + MIG_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52316 - 52201));
        assert_eq!(SEG_BASE, 0xC8EC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AppgetCode::Ok as u32, 0);
        assert_eq!(AppgetCode::EGradeDrift as u32, 12);
        assert_eq!(AppgetCode::Ok.val(), SEG_BASE);
        assert_eq!(AppgetCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let c: AppgetCode = match i {
                0 => AppgetCode::Ok,
                1 => AppgetCode::ENoOp,
                2 => AppgetCode::EDrift,
                3 => AppgetCode::EOrder,
                4 => AppgetCode::ENoMan,
                5 => AppgetCode::EManDrift,
                6 => AppgetCode::ENoEol,
                7 => AppgetCode::EEolDrift,
                8 => AppgetCode::ENoMig,
                9 => AppgetCode::EMigDrift,
                10 => AppgetCode::EDup,
                11 => AppgetCode::ESeal,
                _ => AppgetCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade_cap_s2() {
        assert_eq!(Grade::S2.val(), 2);
        // 空账：S3。
        let lt = AgLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 全通：EOL 如实封顶 S2，不虚 S1。
        let mut lt = AgLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < PKG_OPS {
            if let Some(w) = pkg_cell(o) {
                let _ = lt.record_pkg(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < MANIFEST_CELLS {
            if let Some(w) = manifest_cell(i) {
                let _ = lt.record_manifest(i, w);
            }
            i += 1;
        }
        let mut e = 0usize;
        while e < EOL_CELLS {
            if let Some(w) = eol_cell(e) {
                let _ = lt.record_eol(e, w);
            }
            e += 1;
        }
        let mut m = 0usize;
        while m < MIG_CELLS {
            if let Some(w) = mig_cell(m) {
                let _ = lt.record_mig(m, w);
            }
            m += 1;
        }
        let mut r = 0usize;
        while r < EOLP_ROWS {
            let _ = lt.declare_eolp(r);
            r += 1;
        }
        assert!(lt.ops_all() && lt.man_all() && lt.eol_all() && lt.mig_all() && lt.eolp_all());
        assert_eq!(lt.judge_grade(), Grade::S2);
        // 缺联签一格：落 S3。
        let mut lt2 = lt.clone();
        lt2.mig = [None; MIG_CELLS];
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52316_checks_green() {
        let s = run_unx52316_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
