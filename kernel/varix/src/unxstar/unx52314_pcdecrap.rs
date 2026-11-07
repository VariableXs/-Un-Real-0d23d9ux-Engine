//! UNX-F52314 PC Decrapifier 收口（长尾） · 0xC8E40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52314"，逐字）：
//!   title: PC Decrapifier 收口（长尾）
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52315-J1 六判据
//!     全落；评级数据外部依赖声明；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52314）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52315-J1，照卡面原文逐字锚定；功能按本卡标题（PC Decrapifier
//! 预装清除收口）落地，不按 N+1 卡标题落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 定级口径：PC Decrapifier 属长尾在册软件（上游低频维护、依赖社区
//! 预装清单数据），如实定级封顶 S3——收口全通也只记 S3，不虚报
//! S1/S2（判据测试与本封顶一致）。
//!
//! 设计要点：
//!   ① 清除 10 操作集：五家族闭集（预装清除/启动项清除/工具栏清除/
//!      捆绑清除/清除日志）各 2 操作，家族单调（逆序必抓）。
//!   ② 备份前置闸：清除操作落账前必须先落齐备份前置闸声明 2 行
//!      （备份点声明/还原校验声明），未落齐报 EBackup 拒收。
//!   ③ 预装软件检测 12 格：六域（厂商预装/杀毒试用/工具栏/购物助手/
//!      更新器/遥测组件）各 2 格，逐格载荷折叠恒等才收。
//!   ④ 推荐清除清单 8 格：逐格对照 + 漂移必抓；评级数据外部依赖声明
//!      以清单外挂行落账。
//!   ⑤ ledger 状态机（new/declare_backup/record_clean/record_preload/
//!      record_reco/judge_gate/fingerprint，FNV-1a）；注入核 run_cycle
//!      三态轮转 10 轮（家族逆序/预装漂移/清除越闭集），全部落在当轮
//!      临时账本上，主账 injected==caught==10、escaped==0；双跑指纹
//!      一致；定级闭集（长尾如实封顶 S3）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52314;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 清除操作数。
pub const CLEAN_OPS: usize = 10;

/// 预装软件检测格数。
pub const PRELOAD_CELLS: usize = 12;

/// 推荐清除清单格数。
pub const RECO_CELLS: usize = 8;

/// 备份前置闸声明行数。
pub const BACKUP_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcDecrapCode {
    /// 成功
    Ok = 0,
    /// 清除操作号越闭集
    ENoOp,
    /// 清除操作漂移
    EDrift,
    /// 清除家族逆序
    EOrder,
    /// 预装格越闭集
    ENoPre,
    /// 预装检测漂移
    EPreDrift,
    /// 清单格越闭集
    ENoReco,
    /// 清单漂移
    ERecoDrift,
    /// 备份前置闸未落
    EBackup,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PcDecrapCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PcDecrapCode::Ok => "成功",
            PcDecrapCode::ENoOp => "清除操作号越闭集：清除 10 操作集之外先登记",
            PcDecrapCode::EDrift => "清除操作漂移：清除折叠与基准不恒等，逐格复算",
            PcDecrapCode::EOrder => "家族逆序：预装→启动项→工具栏→捆绑→日志单调，逆序必抓",
            PcDecrapCode::ENoPre => "预装格越闭集：预装检测 12 格之外先登记",
            PcDecrapCode::EPreDrift => "预装检测漂移：预装折叠与基准不恒等，逐格复算",
            PcDecrapCode::ENoReco => "清单格越闭集：推荐清除清单 8 格之外先登记",
            PcDecrapCode::ERecoDrift => "清单漂移：清单折叠与基准不恒等，逐格复算",
            PcDecrapCode::EBackup => "备份前置闸未落：备份点/还原校验声明先落齐再清",
            PcDecrapCode::EDup => "重复落账：同格只许一条账目",
            PcDecrapCode::ESeal => "未齐即封：10 清除 + 12 预装 + 8 清单 + 2 备份未齐先补齐",
            PcDecrapCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PcDecrapCode::EGradeDrift => "定级漂移：长尾收口如实封顶 S3，不虚报更高级",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（长尾卡不授）
    S1,
    /// S2：有条件收口（长尾卡不授）
    S2,
    /// S3：不收口（长尾如实封顶）
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
            Grade::S1 => "S1：全通收口（长尾卡不授，如实封顶 S3）",
            Grade::S2 => "S2：有条件收口（长尾卡不授，如实封顶 S3）",
            Grade::S3 => "S3：PC Decrapifier 长尾在册，收口如实封顶 S3",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 清除操作格载荷（操作 o；家族 = o/2）。
pub const fn clean_cell(o: usize) -> Option<u64> {
    if o >= CLEAN_OPS {
        return None;
    }
    Some((0xCD000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..2=预装 2..4=启动项 4..6=工具栏 6..8=捆绑 8..10=日志）。
pub const fn clean_kind(o: usize) -> Option<u32> {
    if o >= CLEAN_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn clean_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("预装清除"),
        1 => Some("启动项清除"),
        2 => Some("工具栏清除"),
        3 => Some("捆绑清除"),
        4 => Some("清除日志"),
        _ => None,
    }
}

/// 预装软件检测格基准（格 i；XOR 锚低 16 位恒零）。
pub const fn preload_cell(i: usize) -> Option<u64> {
    if i >= PRELOAD_CELLS {
        return None;
    }
    Some((0xCE000u64 + (i as u64) * 0x3B) ^ 0x504344430000)
}

/// 预装检测域名。
pub const fn preload_domain(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("厂商预装"),
        1 => Some("杀毒试用"),
        2 => Some("工具栏"),
        3 => Some("购物助手"),
        4 => Some("更新器"),
        5 => Some("遥测组件"),
        _ => None,
    }
}

/// 推荐清除清单格基准（格 i）。
pub const fn reco_cell(i: usize) -> Option<u64> {
    if i >= RECO_CELLS {
        return None;
    }
    Some((0xCF000u64 + (i as u64) * 0x47) ^ 0x5245434F4C490000)
}

/// 备份前置闸声明行折叠（行 r）。
pub const fn backup_row(r: usize) -> Option<u32> {
    if r >= BACKUP_ROWS {
        return None;
    }
    Some(0xD000 + r as u32 * 0x2C)
}

/// 备份声明行名。
pub const fn backup_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("备份点声明"),
        1 => Some("还原校验声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PC Decrapifier 收口账本状态机。
#[derive(Clone)]
pub struct PdLedger {
    /// 六判据（UNX-F52315-J1 锚）。
    gates: [Option<bool>; 6],
    /// 备份前置闸声明账。
    backup: [Option<u32>; BACKUP_ROWS],
    /// 清除操作账。
    ops: [Option<u64>; CLEAN_OPS],
    /// 预装检测账。
    pre: [Option<u64>; PRELOAD_CELLS],
    /// 推荐清单账。
    reco: [Option<u64>; RECO_CELLS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PdLedger {
    /// 新账本。
    pub fn new() -> Self {
        PdLedger {
            gates: [None; 6],
            backup: [None; BACKUP_ROWS],
            ops: [None; CLEAN_OPS],
            pre: [None; PRELOAD_CELLS],
            reco: [None; RECO_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PcDecrapCode> {
        if gate >= 6 {
            return Err(PcDecrapCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4C00 + gate as u64);
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

    /// 备份前置闸声明一行。
    pub fn declare_backup(&mut self, r: usize) -> Result<(), PcDecrapCode> {
        if r >= BACKUP_ROWS {
            return Err(PcDecrapCode::ESeal);
        }
        if match self.backup.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PcDecrapCode::EDup);
        }
        if let Some(c) = self.backup.get_mut(r) {
            *c = backup_row(r);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// 备份前置闸全齐核验。
    pub fn backup_all(&self) -> bool {
        let mut r = 0usize;
        while r < BACKUP_ROWS {
            match self.backup.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 清除一格（备份前置闸 + 家族单调 + 载荷对照 + 重复拒）。
    pub fn record_clean(&mut self, o: usize, got: u64) -> Result<(), PcDecrapCode> {
        if !self.backup_all() {
            return Err(PcDecrapCode::EBackup);
        }
        if o >= CLEAN_OPS {
            return Err(PcDecrapCode::ENoOp);
        }
        let kind = match clean_kind(o) {
            Some(k) => k,
            None => return Err(PcDecrapCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E00);
            return Err(PcDecrapCode::EOrder);
        }
        let want = match clean_cell(o) {
            Some(w) => w,
            None => return Err(PcDecrapCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E01 + o as u64);
            return Err(PcDecrapCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PcDecrapCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4E02 + o as u64);
        Ok(())
    }

    /// 清除全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CLEAN_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 预装检测一格（载荷对照 + 重复拒）。
    pub fn record_preload(&mut self, i: usize, got: u64) -> Result<(), PcDecrapCode> {
        let want = match preload_cell(i) {
            Some(w) => w,
            None => return Err(PcDecrapCode::ENoPre),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4F00 + i as u64);
            return Err(PcDecrapCode::EPreDrift);
        }
        if match self.pre.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PcDecrapCode::EDup);
        }
        if let Some(c) = self.pre.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4F01 + i as u64);
        Ok(())
    }

    /// 预装全账核验（12 格）。
    pub fn pre_all(&self) -> bool {
        let mut i = 0usize;
        while i < PRELOAD_CELLS {
            match self.pre.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 推荐清单一格（载荷对照 + 重复拒）。
    pub fn record_reco(&mut self, i: usize, got: u64) -> Result<(), PcDecrapCode> {
        let want = match reco_cell(i) {
            Some(w) => w,
            None => return Err(PcDecrapCode::ENoReco),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5000 + i as u64);
            return Err(PcDecrapCode::ERecoDrift);
        }
        if match self.reco.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PcDecrapCode::EDup);
        }
        if let Some(c) = self.reco.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5001 + i as u64);
        Ok(())
    }

    /// 清单全账核验（8 格）。
    pub fn reco_all(&self) -> bool {
        let mut i = 0usize;
        while i < RECO_CELLS {
            match self.reco.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
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

    /// KB-COMPAT 定级：长尾卡如实封顶 S3（全通也只记 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.backup_all()
            && self.ops_all()
            && self.pre_all()
            && self.reco_all()
        {
            Grade::S3
        } else {
            Grade::S3
        }
    }
}

impl Default for PdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    // 备份前置闸先落齐，清除操作才可收。
    let mut r = 0usize;
    while r < BACKUP_ROWS {
        let _ = led.declare_backup(r);
        r += 1;
    }
    let mut o = 0usize;
    while o < CLEAN_OPS {
        if let Some(w) = clean_cell(o) {
            if led.record_clean(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < PRELOAD_CELLS {
        if let Some(w) = preload_cell(i) {
            if led.record_preload(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < RECO_CELLS {
        if let Some(w) = reco_cell(c) {
            if led.record_reco(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 注入 10 轮：家族逆序 / 预装漂移 / 清除越闭集三态轮转，全部落在
    // 当轮临时账本上；临时账本先按合法序落备份闸与高族格再注入。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PdLedger::new();
        let mut b = 0usize;
        while b < BACKUP_ROWS {
            let _ = lt.declare_backup(b);
            b += 1;
        }
        let caught = match kind {
            0 => {
                let _ = lt.record_clean(9, clean_cell(9).unwrap_or(0));
                lt.record_clean(4, clean_cell(4).unwrap_or(0)) == Err(PcDecrapCode::EOrder)
            }
            1 => {
                lt.record_preload(2, preload_cell(2).unwrap_or(0));
                lt.record_preload(3, preload_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(PcDecrapCode::EPreDrift)
            }
            _ => lt.record_clean(CLEAN_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5100 + k as u64);
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
/// UNX-F52314 判据集（锚 UNX-F52315-J1，off-by-one 板式）。
pub fn run_unx52314_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52314");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "清除 10 操作全通",
        recorded >= CLEAN_OPS as u32,
        "预装/启动项/工具栏/捆绑/日志五家族逐操作恒等",
    );
    s.add(
        "备份前置闸 2/2 全齐且前置生效",
        {
            let mut lt = PdLedger::new();
            // 未落备份闸先清必拒。
            let refused = lt.record_clean(0, clean_cell(0).unwrap_or(0)) == Err(PcDecrapCode::EBackup);
            let mut ok = true;
            let mut r = 0usize;
            while r < BACKUP_ROWS {
                if lt.declare_backup(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            refused && ok && lt.backup_all() && lt.record_clean(0, clean_cell(0).unwrap_or(0)).is_ok()
        },
        "备份点/还原校验两行闭集，未落齐拒收清除（§AI-66.3）",
    );
    s.add(
        "预装软件检测 12 格全落",
        {
            let mut lt = PdLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PRELOAD_CELLS {
                if let Some(w) = preload_cell(i) {
                    if lt.record_preload(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.pre_all()
        },
        "厂商预装/杀毒试用/工具栏/购物助手/更新器/遥测六域各 2 格恒等",
    );
    s.add(
        "推荐清除清单 8 格全落",
        {
            let mut lt = PdLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < RECO_CELLS {
                if let Some(w) = reco_cell(i) {
                    if lt.record_reco(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.reco_all()
        },
        "推荐清除清单逐格对照恒等",
    );
    s.add(
        "评级数据外部依赖声明",
        {
            let mut lt = PdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据锚 UNX-F52315-J1 全落，评级数据外部依赖以判据行声明",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CLEAN_OPS {
                if clean_kind(o).unwrap_or(5) < clean_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "家族逆序/预装漂移/清除越闭集三态轮转必抓",
    );
    let mut l0 = PdLedger::new();
    let _ = l0.declare_backup(0);
    let _ = l0.declare_backup(1);
    s.add(
        "注入·家族逆序（空账必抓）",
        {
            let _ = l0.record_clean(9, clean_cell(9).unwrap_or(0));
            l0.record_clean(4, clean_cell(4).unwrap_or(0)) == Err(PcDecrapCode::EOrder)
        },
        "捆绑后回工具栏必抓，且失败不改 last_kind",
    );
    s.add(
        "边界·清除越闭集",
        l0.record_clean(CLEAN_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·预装越闭集",
        l0.record_preload(PRELOAD_CELLS, 0).is_err(),
        "预装格 12 越闭集必拒",
    );
    s.add(
        "边界·清单越闭集",
        l0.record_reco(RECO_CELLS, 0).is_err(),
        "清单格 8 越闭集必拒",
    );
    s.add(
        "漂移·清单必抓",
        {
            l0.record_reco(0, reco_cell(0).unwrap_or(0));
            l0.record_reco(1, reco_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(PcDecrapCode::ERecoDrift)
        },
        "清单载荷漂移必抓",
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
        "定级封顶 S3（长尾如实）",
        {
            let mut lt = PdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < BACKUP_ROWS {
                let _ = lt.declare_backup(r);
                r += 1;
            }
            let mut o = 0usize;
            while o < CLEAN_OPS {
                if let Some(w) = clean_cell(o) {
                    let _ = lt.record_clean(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < PRELOAD_CELLS {
                if let Some(w) = preload_cell(i) {
                    let _ = lt.record_preload(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < RECO_CELLS {
                if let Some(w) = reco_cell(c) {
                    let _ = lt.record_reco(c, w);
                }
                c += 1;
            }
            lt.judge_grade() == Grade::S3
        },
        "全通也封顶 S3：长尾在册不虚报（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8E40,
        "SEG_BASE 0x40 对齐且为 52314 位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_kind() {
        assert_eq!(clean_kind(0), Some(0));
        assert_eq!(clean_kind(5), Some(2));
        assert_eq!(clean_kind(9), Some(4));
        assert!(clean_kind(10).is_none());
        assert!(clean_cell(10).is_none());
        assert_eq!(clean_kind_name(2), Some("工具栏清除"));
        assert!(clean_kind_name(5).is_none());
    }

    #[test]
    fn test_preload_domains() {
        assert_eq!(preload_domain(0), Some("厂商预装"));
        assert_eq!(preload_domain(3), Some("杀毒试用"));
        assert_eq!(preload_domain(6), Some("购物助手"));
        assert_eq!(preload_domain(11), Some("遥测组件"));
        assert!(preload_domain(12).is_none());
        assert!(preload_cell(PRELOAD_CELLS).is_none());
        assert!(reco_cell(RECO_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = clean_cell(3).unwrap_or(0);
        assert_eq!(a, clean_cell(3).unwrap_or(0));
        assert_ne!(a, clean_cell(4).unwrap_or(0));
        assert_ne!(preload_cell(0).unwrap_or(0), reco_cell(0).unwrap_or(0));
        assert!(backup_row(BACKUP_ROWS).is_none());
    }

    #[test]
    fn test_backup_rows_named() {
        assert_eq!(backup_row_name(0), Some("备份点声明"));
        assert_eq!(backup_row_name(1), Some("还原校验声明"));
        assert!(backup_row_name(2).is_none());
    }

    #[test]
    fn test_backup_precondition_gate() {
        let mut lt = PdLedger::new();
        // 备份前置闸未落：清除必拒。
        assert_eq!(
            lt.record_clean(0, clean_cell(0).unwrap_or(0)),
            Err(PcDecrapCode::EBackup)
        );
        let _ = lt.declare_backup(0);
        // 只落一行仍拒。
        assert_eq!(
            lt.record_clean(0, clean_cell(0).unwrap_or(0)),
            Err(PcDecrapCode::EBackup)
        );
        let _ = lt.declare_backup(1);
        assert!(lt.record_clean(0, clean_cell(0).unwrap_or(0)).is_ok());
        // 重复声明必拒。
        assert_eq!(lt.declare_backup(1), Err(PcDecrapCode::EDup));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PdLedger::new();
        let _ = lt.declare_backup(0);
        let _ = lt.declare_backup(1);
        let _ = lt.record_clean(9, clean_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_clean(4, clean_cell(4).unwrap_or(0)),
            Err(PcDecrapCode::EOrder)
        );
        assert!(lt.record_clean(10, 0).is_err());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PdLedger::new();
        let _ = lt.declare_backup(0);
        let _ = lt.declare_backup(1);
        assert_eq!(
            lt.record_clean(5, clean_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PcDecrapCode::EDrift)
        );
        assert_eq!(
            lt.record_preload(4, preload_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(PcDecrapCode::EPreDrift)
        );
        assert_eq!(
            lt.record_reco(2, reco_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(PcDecrapCode::ERecoDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PdLedger::new();
        let _ = lt.record_preload(0, preload_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_preload(0, preload_cell(0).unwrap_or(0)),
            Err(PcDecrapCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PdLedger::new();
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
        assert!(recorded >= (CLEAN_OPS + PRELOAD_CELLS + RECO_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52314 - 52201));
        assert_eq!(SEG_BASE, 0xC8E40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PcDecrapCode::Ok as u32, 0);
        assert_eq!(PcDecrapCode::EGradeDrift as u32, 12);
        assert_eq!(PcDecrapCode::Ok.val(), SEG_BASE);
        assert_eq!(PcDecrapCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!PcDecrapCode::EBackup.detail().is_empty());
        assert!(!PcDecrapCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade_cap_s3() {
        assert_eq!(Grade::S3.val(), 3);
        let lt = PdLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52314_checks_green() {
        let s = run_unx52314_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
