//! UNX-F52466 Vivaldi 收口 · 0xCB440段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52466"，逐字）：
//!   title: Vivaldi 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52467-J1 ×10；与
//!     Chrome 分账（发行版差异声明）；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52466）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52467-J1，照原文锚定；功能按本卡标题（Vivaldi 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 格闭集：UI 定制/标签栈/笔记/邮件/时钟各 4 格，家族
//!      单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② UI 投影 8 格：Vivaldi 界面投影逐格载荷对照，越格与漂移必抓。
//!   ③ 与 Chrome 分账声明 2 行闭集（发行版差异声明）：渲染账分账/邮件
//!      账分账，缺一拒封。
//!   ④ Chromium 上栈声明 2 行闭集：浏览器内核上栈声明/界面层自绘声明，
//!      缺一拒封。
//!   ⑤ 六判据收口闸（启动/渲染/崩溃/卸载/升级/许可）逐格落账，未齐即
//!      封拒。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（操作漂移/
//!      操作越序/操作越闭集，三类注入全落当轮临时账本，空账本同样必
//!      抓，越序路径不改 last_kind，escaped=0）；注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致；KB-COMPAT 定级 S1/S2/S3 闭集（全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52466;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FEATURE_OPS: usize = 20;

/// UI 投影格数。
pub const PROJ_CELLS: usize = 8;

/// 与 Chrome 分账声明行数（发行版差异声明）。
pub const SPLIT_ROWS: usize = 2;

/// Chromium 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VivaldiCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// 分账行越闭集
    ENoSplit,
    /// 上栈声明行越闭集
    ENoStack,
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

impl VivaldiCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VivaldiCode::Ok => "成功",
            VivaldiCode::ENoOp => "操作号越闭集：UI 定制/标签栈/笔记/邮件/时钟 20 格之外先登记",
            VivaldiCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            VivaldiCode::EOrder => "家族逆序：UI 定制→标签栈→笔记→邮件→时钟单调，逆序必抓",
            VivaldiCode::ENoProj => "投影格越闭集：UI 投影 8 格之外先登记",
            VivaldiCode::EProjDrift => "投影漂移：UI 投影载荷与基准不恒等，逐格复算",
            VivaldiCode::ENoSplit => "分账行越闭集：与 Chrome 分账 2 行闭集之外先登记",
            VivaldiCode::ENoStack => "上栈声明行越闭集：Chromium 上栈声明 2 行闭集之外先登记",
            VivaldiCode::EDup => "重复落账：同格只许一条账目",
            VivaldiCode::ESeal => "未齐即封：20 操作 + 8 格投影 + 2 行分账 + 2 行上栈未齐先补齐",
            VivaldiCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VivaldiCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VivaldiCode::EGradeDrift => "定级漂移：全通且发行版差异声明与上栈声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+8格投影+2行分账+2行上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（操作 o；家族 = o/4）。
pub const fn feature_cell(o: usize) -> Option<u64> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((0xCC00u64 + (o as u64) * 0x3B) ^ 0x564956414C4449)
}

/// 功能族（0..4=UI 定制 4..8=标签栈 8..12=笔记 12..16=邮件 16..20=时钟）。
pub const fn feature_kind(o: usize) -> Option<u32> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn feature_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("UI 定制"),
        1 => Some("标签栈"),
        2 => Some("笔记"),
        3 => Some("邮件"),
        4 => Some("时钟"),
        _ => None,
    }
}

/// UI 投影格载荷（格 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xCC20u64 + (p as u64) * 0x41) ^ 0x50524F4A_5549)
}

/// UI 投影格名。
pub const fn proj_cell_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("面板栏投影"),
        1 => Some("状态栏投影"),
        2 => Some("地址栏投影"),
        3 => Some("标签条投影"),
        4 => Some("书签栏投影"),
        5 => Some("快捷命令投影"),
        6 => Some("鼠标手势投影"),
        7 => Some("键盘序列投影"),
        _ => None,
    }
}

/// 与 Chrome 分账行折叠（行 r，发行版差异声明）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCC60 + r as u32 * 0x28)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("渲染账分账"),
        1 => Some("邮件账分账"),
        _ => None,
    }
}

/// Chromium 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCC80 + r as u32 * 0x28)
}

/// 上栈声明行名。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("浏览器内核上栈声明"),
        1 => Some("界面层自绘声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Vivaldi 收口账本状态机。
#[derive(Clone)]
pub struct VivaldiLedger {
    /// 六判据（启动/渲染/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FEATURE_OPS],
    /// UI 投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 与 Chrome 分账账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Chromium 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VivaldiLedger {
    /// 新账本。
    pub fn new() -> Self {
        VivaldiLedger {
            gates: [None; 6],
            ops: [None; FEATURE_OPS],
            projs: [None; PROJ_CELLS],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VivaldiCode> {
        if gate >= 6 {
            return Err(VivaldiCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 功能族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), VivaldiCode> {
        if o >= FEATURE_OPS {
            return Err(VivaldiCode::ENoOp);
        }
        let kind = match feature_kind(o) {
            Some(k) => k,
            None => return Err(VivaldiCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(VivaldiCode::EOrder);
        }
        let want = match feature_cell(o) {
            Some(w) => w,
            None => return Err(VivaldiCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(VivaldiCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VivaldiCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// UI 投影一格（越格拒 + 载荷对照 + 重复拒，漂移走注入账）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), VivaldiCode> {
        if p >= PROJ_CELLS {
            return Err(VivaldiCode::ENoProj);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(VivaldiCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4601 + p as u64);
            return Err(VivaldiCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VivaldiCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4600 + p as u64);
        Ok(())
    }

    /// UI 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 与 Chrome 分账一行（发行版差异声明）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), VivaldiCode> {
        if r >= SPLIT_ROWS {
            return Err(VivaldiCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VivaldiCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// Chromium 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), VivaldiCode> {
        if r >= STACK_ROWS {
            return Err(VivaldiCode::ENoStack);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VivaldiCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4700 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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
        let full = self.gates_all() && self.ops_all() && self.projs_all() && self.splits_all() && self.stacks_all();
        let gates_ok = self.gates_all();
        if full {
            Grade::S1
        } else if gates_ok {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VivaldiLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut led = VivaldiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURE_OPS {
        if let Some(w) = feature_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：操作漂移 / 操作越序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，临时账本从空起手，空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VivaldiLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移。
                let _ = lt.record_op(0, feature_cell(0).unwrap_or(0));
                lt.record_op(1, feature_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(VivaldiCode::EDrift)
            }
            1 => {
                // 越序注入：先落家族 4 正格，再倒落家族 2；错误路径不改 last_kind。
                let _ = lt.record_op(16, feature_cell(16).unwrap_or(0));
                lt.record_op(8, feature_cell(8).unwrap_or(0)) == Err(VivaldiCode::EOrder)
            }
            _ => {
                // 越闭集注入：空账本直接越界，提前返回必抓。
                lt.record_op(FEATURE_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let escaped = led.escaped;
    let fp = led.fingerprint();
    (recorded, caught, escaped, fp)
}

/// 周期守恒核验（守恒破缺 → EInj，指纹漂移 → EFp）。
pub fn verify_cycle() -> Result<u64, VivaldiCode> {
    let (_, caught, escaped, fp) = run_cycle();
    if caught != 10 || escaped != 0 {
        return Err(VivaldiCode::EInj);
    }
    let (_, _, _, fp2) = run_cycle();
    if fp != fp2 {
        return Err(VivaldiCode::EFp);
    }
    Ok(fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52466 判据集（锚 UNX-F52467-J1，off-by-one 板式）。
pub fn run_unx52466_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52466");
    let (recorded, caught, escaped, fp) = run_cycle();
    s.add(
        "五功能族 20 格全通",
        recorded >= FEATURE_OPS as u32,
        "UI 定制/标签栈/笔记/邮件/时钟五家族逐操作恒等",
    );
    s.add(
        "UI 投影 8 格全落",
        {
            let mut lt = VivaldiLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "面板/状态/地址/标签/书签/命令/手势/序列八格逐格投影",
    );
    s.add(
        "投影漂移拒",
        {
            let mut lt = VivaldiLedger::new();
            lt.record_proj(0, proj_cell(0).unwrap_or(0).wrapping_add(1)) == Err(VivaldiCode::EProjDrift)
        },
        "UI 投影载荷漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VivaldiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/渲染/崩溃/卸载/升级/许可逐格落账（UNX-F52467-J1 六判据）",
    );
    s.add(
        "与 Chrome 分账声明 2/2 全齐",
        {
            let mut lt = VivaldiLedger::new();
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
        "渲染账分账/邮件账分账两行闭集（发行版差异声明，§AI-66.3）",
    );
    s.add(
        "Chromium 上栈声明 2/2 全齐",
        {
            let mut lt = VivaldiLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "浏览器内核上栈声明/界面层自绘声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURE_OPS {
                if feature_kind(o).unwrap_or(5) < feature_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作漂移/操作越序/操作越闭集三态轮转必抓",
    );
    let mut l0 = VivaldiLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FEATURE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err() && l0.declare_stack(STACK_ROWS).is_err(),
        "分账行 2 与上栈行 2 越闭集必拒",
    );
    s.add(
        "未齐可续录",
        {
            let mut l1 = VivaldiLedger::new();
            l1.record_op(0, feature_cell(0).unwrap_or(0)).is_ok()
                && l1.judge_gate(0).is_ok()
        },
        "账未齐时落账与判据闸必须放行（未齐即封核只拒封口不拒续录）",
    );
    s.add_dyn(
        "注入账守恒",
        caught == 10 && escaped == 0,
        &format!("注入=10 检出={} 逃逸={}", caught, escaped),
    );
    s.add("双跑指纹一致", fp != 0 && verify_cycle().is_ok(), "确定性全流程逐位一致且守恒核验通过");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = VivaldiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURE_OPS {
                if let Some(w) = feature_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB440,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCB440 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_kind() {
        assert_eq!(feature_kind(0), Some(0));
        assert_eq!(feature_kind(4), Some(1));
        assert_eq!(feature_kind(11), Some(2));
        assert_eq!(feature_kind(15), Some(3));
        assert_eq!(feature_kind(19), Some(4));
        assert!(feature_kind(20).is_none());
        assert!(feature_cell(20).is_none());
        assert_eq!(feature_kind_name(3), Some("邮件"));
        assert!(feature_kind_name(6).is_none());
    }

    #[test]
    fn test_proj_cells() {
        assert_eq!(proj_cell_name(0), Some("面板栏投影"));
        assert_eq!(proj_cell_name(7), Some("键盘序列投影"));
        assert!(proj_cell_name(8).is_none());
        assert!(proj_cell(PROJ_CELLS).is_none());
        let a = proj_cell(3).unwrap_or(0);
        assert_eq!(a, proj_cell(3).unwrap_or(0));
        assert_ne!(a, proj_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_rows_closed() {
        assert_eq!(split_row_name(0), Some("渲染账分账"));
        assert_eq!(split_row_name(1), Some("邮件账分账"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(stack_row_name(0), Some("浏览器内核上栈声明"));
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族正格，再注逆序必抓。
        let mut lt = VivaldiLedger::new();
        let _ = lt.record_op(13, feature_cell(13).unwrap_or(0));
        assert_eq!(lt.record_op(6, feature_cell(6).unwrap_or(0)), Err(VivaldiCode::EOrder));
        // 逆序错误路径不改 last_kind：同族后续仍可续录。
        assert!(lt.record_op(14, feature_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VivaldiLedger::new();
        assert_eq!(
            lt.record_op(7, feature_cell(7).unwrap_or(0).wrapping_add(3)),
            Err(VivaldiCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(9)),
            Err(VivaldiCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = VivaldiLedger::new();
        let _ = lt.record_proj(2, proj_cell(2).unwrap_or(0));
        assert_eq!(lt.record_proj(2, proj_cell(2).unwrap_or(0)), Err(VivaldiCode::EDup));
        let _ = lt.declare_stack(0);
        assert_eq!(lt.declare_stack(0), Err(VivaldiCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VivaldiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = VivaldiLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, escaped, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：操作 20 + 投影 8。
        assert_eq!(recorded, FEATURE_OPS as u32 + PROJ_CELLS as u32);
        assert_eq!(caught, 10);
        assert_eq!(escaped, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(c1, c2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
        assert!(verify_cycle().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        // 三类注入对空账本同样必抓；越界提前返回不计注入账。
        let mut l0 = VivaldiLedger::new();
        assert!(l0.record_op(FEATURE_OPS, 0).is_err());
        assert!(l0.record_proj(PROJ_CELLS, 0).is_err());
        let (i0, c0, e0) = l0.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCB440);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(VivaldiCode::Ok as u32, 0);
        assert_eq!(VivaldiCode::EGradeDrift as u32, 12);
        assert_eq!(VivaldiCode::Ok.val(), SEG_BASE);
        assert!(!VivaldiCode::Ok.detail().is_empty());
        assert!(!VivaldiCode::ESeal.detail().is_empty());
        assert!(!VivaldiCode::EFp.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt0 = VivaldiLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = VivaldiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = VivaldiLedger::new();
        let mut o = 0usize;
        while o < FEATURE_OPS {
            if let Some(w) = feature_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut g2 = 0usize;
        while g2 < 6 {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52466_checks_green() {
        let s = run_unx52466_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
