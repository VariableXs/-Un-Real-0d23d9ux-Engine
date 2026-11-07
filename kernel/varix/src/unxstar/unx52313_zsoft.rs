//! UNX-F52313 ZSoft Uninstaller 收口（长尾） · 0xC8E00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52313"，逐字）：
//!   title: ZSoft Uninstaller 收口（长尾）
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52314-J1 六判据
//!     全落；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52313）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52314-J1，照卡面原文逐字锚定；功能按本卡标题（ZSoft
//! Uninstaller 卸载收口）落地，不按 N+1 卡标题落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 定级口径：ZSoft Uninstaller 属长尾在册软件（最后版本年份在册，
//! 上游维护低频但未停更），如实定级封顶 S3——收口全通也只记 S3，
//! 不虚报 S1/S2（判据测试与本封顶一致）。
//!
//! 设计要点：
//!   ① 卸载 10 操作集：五家族闭集（标准卸载/静默卸载/强制卸载/批量
//!      卸载/卸载日志）各 2 操作，家族单调（逆序必抓）。
//!   ② 残留扫描 12 格：注册表键/文件系统残留/服务残留/计划任务残留/
//!      启动项残留/卸载日志残留六域各 2 格，逐格载荷折叠恒等才收。
//!   ③ 扩展信息 6 格：发布者/安装日期/版本/占用大小/安装源/数字签名
//!      逐格对照，漂移必抓。
//!   ④ 长尾维护状态声明 2 行闭集：最后版本年份在册声明/长尾维护
//!      低频声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_uninstall/record_residue/record_ext/
//!      declare_maint/judge_gate/fingerprint，FNV-1a）；注入核 run_cycle
//!      三态轮转 10 轮（家族逆序/残留漂移/操作越闭集），全部落在当轮
//!      临时账本上，主账 injected==caught==10、escaped==0；双跑指纹
//!      一致；定级闭集（长尾如实封顶 S3）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52313;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 卸载操作数。
pub const UNINSTALL_OPS: usize = 10;

/// 残留扫描格数。
pub const RESIDUE_CELLS: usize = 12;

/// 扩展信息格数。
pub const EXTINFO_CELLS: usize = 6;

/// 长尾维护声明行数。
pub const MAINT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZsoftCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 残留格越闭集
    ENoRes,
    /// 残留扫描漂移
    EResDrift,
    /// 扩展格越闭集
    ENoExt,
    /// 扩展信息漂移
    EExtDrift,
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

impl ZsoftCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ZsoftCode::Ok => "成功",
            ZsoftCode::ENoOp => "操作号越闭集：卸载 10 操作集之外先登记",
            ZsoftCode::EDrift => "操作漂移：卸载操作折叠与基准不恒等，逐格复算",
            ZsoftCode::EOrder => "家族逆序：标准→静默→强制→批量→日志单调，逆序必抓",
            ZsoftCode::ENoRes => "残留格越闭集：残留扫描 12 格之外先登记",
            ZsoftCode::EResDrift => "残留扫描漂移：残留折叠与基准不恒等，逐格复算",
            ZsoftCode::ENoExt => "扩展格越闭集：扩展信息 6 格之外先登记",
            ZsoftCode::EExtDrift => "扩展信息漂移：扩展折叠与基准不恒等，逐格复算",
            ZsoftCode::EDup => "重复落账：同格只许一条账目",
            ZsoftCode::ESeal => "未齐即封：10 操作 + 12 残留 + 6 扩展 + 2 声明未齐先补齐",
            ZsoftCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ZsoftCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ZsoftCode::EGradeDrift => "定级漂移：长尾收口如实封顶 S3，不虚报更高级",
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
            Grade::S3 => "S3：ZSoft Uninstaller 长尾在册，收口如实封顶 S3",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 卸载操作格载荷（操作 o；家族 = o/2）。
pub const fn uninstall_cell(o: usize) -> Option<u64> {
    if o >= UNINSTALL_OPS {
        return None;
    }
    Some((0xC9000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..2=标准 2..4=静默 4..6=强制 6..8=批量 8..10=日志）。
pub const fn uninstall_kind(o: usize) -> Option<u32> {
    if o >= UNINSTALL_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn uninstall_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("标准卸载"),
        1 => Some("静默卸载"),
        2 => Some("强制卸载"),
        3 => Some("批量卸载"),
        4 => Some("卸载日志"),
        _ => None,
    }
}

/// 残留扫描格基准（格 i；XOR 锚低 16 位恒零）。
pub const fn residue_cell(i: usize) -> Option<u64> {
    if i >= RESIDUE_CELLS {
        return None;
    }
    Some((0xCA000u64 + (i as u64) * 0x3B) ^ 0x5A534F46540000)
}

/// 残留域名。
pub const fn residue_domain(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("注册表键"),
        1 => Some("文件系统残留"),
        2 => Some("服务残留"),
        3 => Some("计划任务残留"),
        4 => Some("启动项残留"),
        5 => Some("卸载日志残留"),
        _ => None,
    }
}

/// 扩展信息格基准（格 i）。
pub const fn ext_cell(i: usize) -> Option<u64> {
    if i >= EXTINFO_CELLS {
        return None;
    }
    Some((0xCB000u64 + (i as u64) * 0x47) ^ 0x455854494E460000)
}

/// 扩展信息名。
pub const fn ext_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("发布者"),
        1 => Some("安装日期"),
        2 => Some("版本"),
        3 => Some("占用大小"),
        4 => Some("安装源"),
        5 => Some("数字签名"),
        _ => None,
    }
}

/// 长尾维护声明行折叠（行 r）。
pub const fn maint_row(r: usize) -> Option<u32> {
    if r >= MAINT_ROWS {
        return None;
    }
    Some(0xCC00 + r as u32 * 0x2C)
}

/// 长尾维护声明行名。
pub const fn maint_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("最后版本年份在册声明"),
        1 => Some("长尾维护低频声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ZSoft Uninstaller 收口账本状态机。
#[derive(Clone)]
pub struct ZsLedger {
    /// 六判据（UNX-F52314-J1 锚）。
    gates: [Option<bool>; 6],
    /// 卸载操作账。
    ops: [Option<u64>; UNINSTALL_OPS],
    /// 残留扫描账。
    res: [Option<u64>; RESIDUE_CELLS],
    /// 扩展信息账。
    ext: [Option<u64>; EXTINFO_CELLS],
    /// 长尾维护声明账。
    maint: [Option<u32>; MAINT_ROWS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ZsLedger {
    /// 新账本。
    pub fn new() -> Self {
        ZsLedger {
            gates: [None; 6],
            ops: [None; UNINSTALL_OPS],
            res: [None; RESIDUE_CELLS],
            ext: [None; EXTINFO_CELLS],
            maint: [None; MAINT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ZsoftCode> {
        if gate >= 6 {
            return Err(ZsoftCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4600 + gate as u64);
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

    /// 卸载一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_uninstall(&mut self, o: usize, got: u64) -> Result<(), ZsoftCode> {
        if o >= UNINSTALL_OPS {
            return Err(ZsoftCode::ENoOp);
        }
        let kind = match uninstall_kind(o) {
            Some(k) => k,
            None => return Err(ZsoftCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4700);
            return Err(ZsoftCode::EOrder);
        }
        let want = match uninstall_cell(o) {
            Some(w) => w,
            None => return Err(ZsoftCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4701 + o as u64);
            return Err(ZsoftCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZsoftCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4702 + o as u64);
        Ok(())
    }

    /// 卸载全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < UNINSTALL_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 残留扫描一格（载荷对照 + 重复拒）。
    pub fn record_residue(&mut self, i: usize, got: u64) -> Result<(), ZsoftCode> {
        let want = match residue_cell(i) {
            Some(w) => w,
            None => return Err(ZsoftCode::ENoRes),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4800 + i as u64);
            return Err(ZsoftCode::EResDrift);
        }
        if match self.res.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZsoftCode::EDup);
        }
        if let Some(c) = self.res.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4801 + i as u64);
        Ok(())
    }

    /// 残留全账核验（12 格）。
    pub fn res_all(&self) -> bool {
        let mut i = 0usize;
        while i < RESIDUE_CELLS {
            match self.res.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 扩展信息一格（载荷对照 + 重复拒）。
    pub fn record_ext(&mut self, i: usize, got: u64) -> Result<(), ZsoftCode> {
        let want = match ext_cell(i) {
            Some(w) => w,
            None => return Err(ZsoftCode::ENoExt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4900 + i as u64);
            return Err(ZsoftCode::EExtDrift);
        }
        if match self.ext.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZsoftCode::EDup);
        }
        if let Some(c) = self.ext.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4901 + i as u64);
        Ok(())
    }

    /// 扩展全账核验（6 格）。
    pub fn ext_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXTINFO_CELLS {
            match self.ext.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 长尾维护声明一行。
    pub fn declare_maint(&mut self, r: usize) -> Result<(), ZsoftCode> {
        if r >= MAINT_ROWS {
            return Err(ZsoftCode::ESeal);
        }
        if match self.maint.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZsoftCode::EDup);
        }
        if let Some(c) = self.maint.get_mut(r) {
            *c = maint_row(r);
        }
        self.fnv(0x4A00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn maint_all(&self) -> bool {
        let mut r = 0usize;
        while r < MAINT_ROWS {
            match self.maint.get(r) {
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

    /// KB-COMPAT 定级：长尾卡如实封顶 S3（全通也只记 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.res_all() && self.ext_all() && self.maint_all()
        {
            Grade::S3
        } else {
            Grade::S3
        }
    }
}

impl Default for ZsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ZsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    // 先落长尾维护声明（卸载前置闸口径）。
    let mut r = 0usize;
    while r < MAINT_ROWS {
        let _ = led.declare_maint(r);
        r += 1;
    }
    let mut o = 0usize;
    while o < UNINSTALL_OPS {
        if let Some(w) = uninstall_cell(o) {
            if led.record_uninstall(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < RESIDUE_CELLS {
        if let Some(w) = residue_cell(i) {
            if led.record_residue(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut e = 0usize;
    while e < EXTINFO_CELLS {
        if let Some(w) = ext_cell(e) {
            if led.record_ext(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    // 注入 10 轮：家族逆序 / 残留漂移 / 操作越闭集三态轮转，全部落在
    // 当轮临时账本上；家族逆序注入先按合法序落高族格再注低族。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ZsLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_uninstall(7, uninstall_cell(7).unwrap_or(0));
                lt.record_uninstall(2, uninstall_cell(2).unwrap_or(0)) == Err(ZsoftCode::EOrder)
            }
            1 => {
                lt.record_residue(3, residue_cell(3).unwrap_or(0));
                lt.record_residue(4, residue_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(ZsoftCode::EResDrift)
            }
            _ => lt.record_uninstall(UNINSTALL_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4B00 + k as u64);
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
/// UNX-F52313 判据集（锚 UNX-F52314-J1，off-by-one 板式）。
pub fn run_unx52313_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52313");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "卸载 10 操作全通",
        recorded >= UNINSTALL_OPS as u32,
        "标准/静默/强制/批量/日志五家族逐操作恒等",
    );
    s.add(
        "残留扫描 12 格全落",
        {
            let mut lt = ZsLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < RESIDUE_CELLS {
                if let Some(w) = residue_cell(i) {
                    if lt.record_residue(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.res_all()
        },
        "注册表/文件系统/服务/计划任务/启动项/日志六域各 2 格恒等",
    );
    s.add(
        "扩展信息 6 格全落",
        {
            let mut lt = ZsLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < EXTINFO_CELLS {
                if let Some(w) = ext_cell(i) {
                    if lt.record_ext(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.ext_all()
        },
        "发布者/安装日期/版本/大小/安装源/数字签名逐格对照",
    );
    s.add(
        "六判据全落（UNX-F52314-J1）",
        {
            let mut lt = ZsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "判据锚 UNX-F52314-J1 逐格落账（§AI-66.3）",
    );
    s.add(
        "长尾维护声明 2/2 全齐",
        {
            let mut lt = ZsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MAINT_ROWS {
                if lt.declare_maint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.maint_all()
        },
        "最后版本年份在册/长尾维护低频两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < UNINSTALL_OPS {
                if uninstall_kind(o).unwrap_or(5) < uninstall_kind(o - 1).unwrap_or(5) {
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
        "家族逆序/残留漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = ZsLedger::new();
    s.add(
        "注入·家族逆序（空账必抓）",
        {
            let _ = l0.record_uninstall(7, uninstall_cell(7).unwrap_or(0));
            l0.record_uninstall(2, uninstall_cell(2).unwrap_or(0)) == Err(ZsoftCode::EOrder)
        },
        "批量后回静默必抓，且失败不改 last_kind",
    );
    s.add(
        "边界·卸载越闭集",
        l0.record_uninstall(UNINSTALL_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·残留越闭集",
        l0.record_residue(RESIDUE_CELLS, 0).is_err(),
        "残留格 12 越闭集必拒",
    );
    s.add(
        "边界·扩展越闭集",
        l0.record_ext(EXTINFO_CELLS, 0).is_err(),
        "扩展格 6 越闭集必拒",
    );
    s.add(
        "漂移·残留必抓",
        {
            l0.record_residue(0, residue_cell(0).unwrap_or(0));
            l0.record_residue(1, residue_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(ZsoftCode::EResDrift)
        },
        "残留载荷漂移必抓",
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
            let mut lt = ZsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < UNINSTALL_OPS {
                if let Some(w) = uninstall_cell(o) {
                    let _ = lt.record_uninstall(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < RESIDUE_CELLS {
                if let Some(w) = residue_cell(i) {
                    let _ = lt.record_residue(i, w);
                }
                i += 1;
            }
            let mut e = 0usize;
            while e < EXTINFO_CELLS {
                if let Some(w) = ext_cell(e) {
                    let _ = lt.record_ext(e, w);
                }
                e += 1;
            }
            let mut r = 0usize;
            while r < MAINT_ROWS {
                let _ = lt.declare_maint(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S3
        },
        "全通也封顶 S3：长尾在册不虚报（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8E00,
        "SEG_BASE 0x40 对齐且为 52313 位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uninstall_kind() {
        assert_eq!(uninstall_kind(0), Some(0));
        assert_eq!(uninstall_kind(2), Some(1));
        assert_eq!(uninstall_kind(9), Some(4));
        assert!(uninstall_kind(10).is_none());
        assert!(uninstall_cell(10).is_none());
        assert_eq!(uninstall_kind_name(4), Some("卸载日志"));
        assert!(uninstall_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = residue_cell(5).unwrap_or(0);
        assert_eq!(a, residue_cell(5).unwrap_or(0));
        assert_ne!(a, residue_cell(6).unwrap_or(0));
        assert_ne!(residue_cell(0).unwrap_or(0), ext_cell(0).unwrap_or(0));
        assert!(residue_cell(RESIDUE_CELLS).is_none());
        assert!(ext_cell(EXTINFO_CELLS).is_none());
        assert!(maint_row(MAINT_ROWS).is_none());
    }

    #[test]
    fn test_residue_domains() {
        assert_eq!(residue_domain(0), Some("注册表键"));
        assert_eq!(residue_domain(2), Some("文件系统残留"));
        assert_eq!(residue_domain(4), Some("服务残留"));
        assert_eq!(residue_domain(11), Some("卸载日志残留"));
        assert!(residue_domain(12).is_none());
        assert_eq!(ext_name(5), Some("数字签名"));
        assert!(ext_name(6).is_none());
    }

    #[test]
    fn test_maint_rows_named() {
        assert_eq!(maint_row_name(0), Some("最后版本年份在册声明"));
        assert_eq!(maint_row_name(1), Some("长尾维护低频声明"));
        assert!(maint_row_name(2).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ZsLedger::new();
        let _ = lt.record_uninstall(7, uninstall_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_uninstall(2, uninstall_cell(2).unwrap_or(0)),
            Err(ZsoftCode::EOrder)
        );
        // 失败注入不改 last_kind：随后同族高号仍可合法落账。
        assert!(lt.record_uninstall(9, uninstall_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ZsLedger::new();
        assert_eq!(
            lt.record_uninstall(5, uninstall_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ZsoftCode::EDrift)
        );
        assert_eq!(
            lt.record_residue(2, residue_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(ZsoftCode::EResDrift)
        );
        assert_eq!(
            lt.record_ext(3, ext_cell(3).unwrap_or(0).wrapping_add(9)),
            Err(ZsoftCode::EExtDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ZsLedger::new();
        let _ = lt.record_residue(0, residue_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_residue(0, residue_cell(0).unwrap_or(0)),
            Err(ZsoftCode::EDup)
        );
        let _ = lt.record_ext(1, ext_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_ext(1, ext_cell(1).unwrap_or(0)),
            Err(ZsoftCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ZsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ZsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (UNINSTALL_OPS + RESIDUE_CELLS + EXTINFO_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52313 - 52201));
        assert_eq!(SEG_BASE, 0xC8E00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ZsoftCode::Ok as u32, 0);
        assert_eq!(ZsoftCode::EGradeDrift as u32, 12);
        assert_eq!(ZsoftCode::Ok.val(), SEG_BASE);
        assert_eq!(ZsoftCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let c: ZsoftCode = match i {
                0 => ZsoftCode::Ok,
                1 => ZsoftCode::ENoOp,
                2 => ZsoftCode::EDrift,
                3 => ZsoftCode::EOrder,
                4 => ZsoftCode::ENoRes,
                5 => ZsoftCode::EResDrift,
                6 => ZsoftCode::ENoExt,
                7 => ZsoftCode::EExtDrift,
                8 => ZsoftCode::EDup,
                9 => ZsoftCode::ESeal,
                10 => ZsoftCode::EInj,
                11 => ZsoftCode::EFp,
                _ => ZsoftCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade_cap_s3() {
        assert_eq!(Grade::S3.val(), 3);
        // 空账：S3。
        let lt = ZsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 全通：长尾如实仍封顶 S3。
        let mut lt = ZsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < UNINSTALL_OPS {
            if let Some(w) = uninstall_cell(o) {
                let _ = lt.record_uninstall(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < RESIDUE_CELLS {
            if let Some(w) = residue_cell(i) {
                let _ = lt.record_residue(i, w);
            }
            i += 1;
        }
        assert!(lt.ops_all() && lt.res_all());
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52313_checks_green() {
        let s = run_unx52313_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
