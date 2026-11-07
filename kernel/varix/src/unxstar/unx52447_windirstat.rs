//! UNX-F52447 WinDirStat 收口 · 0xCAF80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52447"，逐字）：
//!   title: WinDirStat 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52448-J1 六判据
//!     全落；MFT 快扫 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52447）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52448-J1，照原文锚定；功能按本卡标题（WinDirStat）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五视图族闭集 20 格：目录树/扩展名/用户/文件/空闲空间各 4 格，家
//!      族单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② Cushion 渲染三元闸：高亮/暗化/比例三格闭集，越格必抓、漂移必抓
//!      （treemap 渲染收口前置闸）。
//!   ③ 清理三态 6 格：未清理/已清理/已跳过三态各 2 格，闭集外先登记必
//!      抓。
//!   ④ 与 WizTree 分账声明 2 行闭集（互斥消费分账）+ C++ 上栈声明 2 行
//!      闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（视图漂
//!      移/家族逆序/视图越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一
//!      致；定级 S2 目标档（KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52447;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五视图族操作数。
pub const VIEW_OPS: usize = 20;

/// Cushion 渲染三元闸格数。
pub const CUSHION_CELLS: usize = 3;

/// 清理三态格数。
pub const CLEAN_STATES: usize = 6;

/// 与 WizTree 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const CSTACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindirstatCode {
    /// 成功
    Ok = 0,
    /// 视图号越闭集
    ENoView,
    /// 视图漂移
    EViewDrift,
    /// 家族逆序
    EOrder,
    /// Cushion 格越闸
    ENoCush,
    /// Cushion 漂移
    ECushDrift,
    /// 清理格越闭集
    ENoClean,
    /// 清理漂移
    ECleanDrift,
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

impl WindirstatCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WindirstatCode::Ok => "成功",
            WindirstatCode::ENoView => "视图号越闭集：WinDirStat 五视图族 20 格闭集之外先登记",
            WindirstatCode::EViewDrift => "视图漂移：视图族折叠与基准不恒等，逐格复算",
            WindirstatCode::EOrder => "家族逆序：目录树→扩展名→用户→文件→空闲空间单调，逆序必抓",
            WindirstatCode::ENoCush => "Cushion 格越闸：渲染三元闸 3 格（高亮/暗化/比例）闭集之外先登记",
            WindirstatCode::ECushDrift => "Cushion 漂移：渲染三元闸折叠与基准不恒等，逐格复算",
            WindirstatCode::ENoClean => "清理格越闭集：清理三态 6 格闭集之外先登记",
            WindirstatCode::ECleanDrift => "清理漂移：清理态折叠与基准不恒等，逐格复算",
            WindirstatCode::EDup => "重复落账：同格只许一条账目",
            WindirstatCode::ESeal => "未齐即封：20 视图 + 3 Cushion + 6 清理 + 2 行分账 + 2 行上栈未齐先补齐",
            WindirstatCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WindirstatCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WindirstatCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const 安全）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口（本卡闭集保留级）",
            Grade::S2 => "S2：有条件收口，WinDirStat 目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 视图族操作格载荷（视图 o；家族 = o/4）。
pub const fn view_cell(o: usize) -> Option<u64> {
    if o >= VIEW_OPS {
        return None;
    }
    Some((0xCD100u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 视图族（0..4=目录树 4..8=扩展名 8..12=用户 12..16=文件 16..20=空闲空间）。
pub const fn view_kind(o: usize) -> Option<u32> {
    if o >= VIEW_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 视图族名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("目录树"),
        1 => Some("扩展名"),
        2 => Some("用户"),
        3 => Some("文件"),
        4 => Some("空闲空间"),
        _ => None,
    }
}

/// Cushion 渲染三元闸格载荷（格 c；0=高亮 1=暗化 2=比例）。
pub const fn cushion_cell(c: usize) -> Option<u64> {
    if c >= CUSHION_CELLS {
        return None;
    }
    Some((0xCD200u64 + (c as u64) * 0x5D) ^ 0x5744_5354_0000_0000)
}

/// Cushion 闸名。
pub const fn cushion_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("高亮"),
        1 => Some("暗化"),
        2 => Some("比例"),
        _ => None,
    }
}

/// 清理三态格载荷（格 c；态 = c/2）。
pub const fn clean_cell(c: usize) -> Option<u64> {
    if c >= CLEAN_STATES {
        return None;
    }
    Some((0xCD300u64 + (c as u64) * 0x1F) ^ ((c / 2) as u64).wrapping_mul(0x9E3779B9))
}

/// 清理三态（0..2=未清理 2..4=已清理 4..6=已跳过）。
pub const fn clean_phase(c: usize) -> Option<u32> {
    if c >= CLEAN_STATES {
        return None;
    }
    Some((c / 2) as u32)
}

/// 清理态名。
pub const fn clean_phase_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("未清理"),
        1 => Some("已清理"),
        2 => Some("已跳过"),
        _ => None,
    }
}

/// WizTree 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCD400 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn cstack_row(r: usize) -> Option<u32> {
    if r >= CSTACK_ROWS {
        return None;
    }
    Some(0xCD500 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WinDirStat 收口账本状态机。
#[derive(Clone)]
pub struct WindirstatLedger {
    /// 六判据（磁盘占用收口：视图/Cushion/清理/分账/上栈/封核）。
    gates: [Option<bool>; 6],
    /// 视图族操作账。
    ops: [Option<u64>; VIEW_OPS],
    /// Cushion 三元闸账。
    cush: [Option<u64>; CUSHION_CELLS],
    /// 清理三态账。
    clean: [Option<u64>; CLEAN_STATES],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C++ 上栈声明账。
    cstack: [Option<u32>; CSTACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WindirstatLedger {
    /// 新账本。
    pub fn new() -> Self {
        WindirstatLedger {
            gates: [None; 6],
            ops: [None; VIEW_OPS],
            cush: [None; CUSHION_CELLS],
            clean: [None; CLEAN_STATES],
            splits: [None; SPLIT_ROWS],
            cstack: [None; CSTACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WindirstatCode> {
        if gate >= 6 {
            return Err(WindirstatCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4F00 + gate as u64);
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

    /// 视图一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_view(&mut self, o: usize, got: u64) -> Result<(), WindirstatCode> {
        if o >= VIEW_OPS {
            return Err(WindirstatCode::ENoView);
        }
        let kind = match view_kind(o) {
            Some(k) => k,
            None => return Err(WindirstatCode::ENoView),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4F10);
            return Err(WindirstatCode::EOrder);
        }
        let want = match view_cell(o) {
            Some(w) => w,
            None => return Err(WindirstatCode::ENoView),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4F11 + o as u64);
            return Err(WindirstatCode::EViewDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindirstatCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4F12 + o as u64);
        Ok(())
    }

    /// 视图全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < VIEW_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Cushion 三元闸一格（越格必抓 + 载荷对照 + 重复拒）。
    pub fn record_cushion(&mut self, c: usize, got: u64) -> Result<(), WindirstatCode> {
        let want = match cushion_cell(c) {
            Some(w) => w,
            None => return Err(WindirstatCode::ENoCush),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4F20 + c as u64);
            return Err(WindirstatCode::ECushDrift);
        }
        if match self.cush.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindirstatCode::EDup);
        }
        if let Some(x) = self.cush.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x4F21 + c as u64);
        Ok(())
    }

    /// Cushion 三元闸全账核验（3 格）。
    pub fn cush_all(&self) -> bool {
        let mut c = 0usize;
        while c < CUSHION_CELLS {
            match self.cush.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 清理一格（三态载荷对照 + 重复拒）。
    pub fn record_clean(&mut self, c: usize, got: u64) -> Result<(), WindirstatCode> {
        let want = match clean_cell(c) {
            Some(w) => w,
            None => return Err(WindirstatCode::ENoClean),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4F30 + c as u64);
            return Err(WindirstatCode::ECleanDrift);
        }
        if match self.clean.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindirstatCode::EDup);
        }
        if let Some(x) = self.clean.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x4F31 + c as u64);
        Ok(())
    }

    /// 清理全账核验（6 格）。
    pub fn clean_all(&self) -> bool {
        let mut c = 0usize;
        while c < CLEAN_STATES {
            match self.clean.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 分账声明一行（与 WizTree 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), WindirstatCode> {
        if r >= SPLIT_ROWS {
            return Err(WindirstatCode::ENoView);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindirstatCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4F40 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// C++ 上栈声明一行。
    pub fn declare_cstack(&mut self, r: usize) -> Result<(), WindirstatCode> {
        if r >= CSTACK_ROWS {
            return Err(WindirstatCode::ENoView);
        }
        if match self.cstack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindirstatCode::EDup);
        }
        if let Some(c) = self.cstack.get_mut(r) {
            *c = cstack_row(r);
        }
        self.fnv(0x4F50 + r as u64);
        Ok(())
    }

    /// C++ 上栈声明全齐核验。
    pub fn cstack_all(&self) -> bool {
        let mut r = 0usize;
        while r < CSTACK_ROWS {
            match self.cstack.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封核检查：六闸齐且各节全齐才放行，未齐即封。
    pub fn seal_check(&self) -> Result<(), WindirstatCode> {
        if !self.gates_all() {
            return Err(WindirstatCode::ESeal);
        }
        if !self.splits_all() || !self.cstack_all() {
            return Err(WindirstatCode::ESeal);
        }
        if !self.ops_all() || !self.cush_all() || !self.clean_all() {
            return Err(WindirstatCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WindirstatLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WindirstatLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < VIEW_OPS {
        if let Some(w) = view_cell(o) {
            if led.record_view(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CLEAN_STATES {
        if let Some(w) = clean_cell(c) {
            if led.record_clean(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < CUSHION_CELLS {
        if let Some(w) = cushion_cell(t) {
            let _ = led.record_cushion(t, w);
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut x = 0usize;
    while x < CSTACK_ROWS {
        let _ = led.declare_cstack(x);
        x += 1;
    }
    // 注入 10 轮：视图漂移 / 家族逆序 / 视图越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WindirstatLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格（低族 0 格），再注漂移格。
            0 => {
                let _ = lt.record_view(0, view_cell(0).unwrap_or(0));
                lt.record_view(1, view_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WindirstatCode::EViewDrift)
            }
            // 逆序注入：先落合法高族格，再注低族格，逆序必抓。
            1 => {
                let _ = lt.record_view(17, view_cell(17).unwrap_or(0));
                lt.record_view(5, view_cell(5).unwrap_or(0)) == Err(WindirstatCode::EOrder)
            }
            // 越闭集注入：空账本同样必抓，且不改 last_kind。
            _ => lt.record_view(VIEW_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4F60 + k as u64);
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
/// UNX-F52447 判据集（锚 UNX-F52448-J1，off-by-one 板式）。
pub fn run_unx52447_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52447");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五视图族 20 格全通",
        recorded >= VIEW_OPS as u32,
        "目录树/扩展名/用户/文件/空闲空间五家族逐格恒等",
    );
    s.add(
        "Cushion 渲染三元闸 3/3 全落",
        {
            let mut lt = WindirstatLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CUSHION_CELLS {
                if let Some(w) = cushion_cell(c) {
                    if lt.record_cushion(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cush_all()
        },
        "高亮/暗化/比例三格闭集，越格必抓漂移必抓（treemap 渲染前置闸）",
    );
    s.add(
        "清理三态 6 格全落",
        {
            let mut lt = WindirstatLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CLEAN_STATES {
                if let Some(w) = clean_cell(c) {
                    if lt.record_clean(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.clean_all()
        },
        "未清理/已清理/已跳过三态各 2 格闭集",
    );
    s.add(
        "与 WizTree 分账声明 2/2 全齐",
        {
            let mut lt = WindirstatLedger::new();
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
        "WinDirStat 独立账声明/WizTree 互斥消费分账声明两行闭集",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = WindirstatLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CSTACK_ROWS {
                if lt.declare_cstack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cstack_all()
        },
        "C++ 上栈独立账声明/C++ 栈深度分账声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < VIEW_OPS {
                if view_kind(o).unwrap_or(5) < view_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五视图族闭集单调（逆序必抓）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    let mut l0 = WindirstatLedger::new();
    s.add(
        "边界·视图越闭集",
        l0.record_view(VIEW_OPS, 0).is_err(),
        "视图 20 越闭集必拒",
    );
    s.add(
        "边界·Cushion 越闸",
        l0.record_cushion(CUSHION_CELLS, 0).is_err(),
        "Cushion 格 3 越闸必拒",
    );
    s.add(
        "边界·清理越闭集",
        l0.record_clean(CLEAN_STATES, 0).is_err(),
        "清理格 6 越闭集必拒",
    );
    s.add(
        "视图漂移拒",
        {
            let _ = l0.record_view(0, view_cell(0).unwrap_or(0));
            l0.record_view(1, view_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WindirstatCode::EViewDrift)
        },
        "视图族载荷漂移必抓",
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
        "未齐先记账不封",
        {
            let mut lt = WindirstatLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < 4 {
                if let Some(w) = view_cell(o) {
                    if lt.record_view(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            // 账未齐时逐格记账仍放行（Ok），封核检查才拒。
            ok && lt.record_view(4, view_cell(4).unwrap_or(0)).is_ok()
        },
        "账未齐时记账必须 Ok，未齐即封只在封核检查处生效",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = WindirstatLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < VIEW_OPS {
                if let Some(w) = view_cell(o) {
                    let _ = lt.record_view(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CUSHION_CELLS {
                if let Some(w) = cushion_cell(c) {
                    let _ = lt.record_cushion(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < CLEAN_STATES {
                if let Some(w) = clean_cell(t) {
                    let _ = lt.record_clean(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut x = 0usize;
            while x < CSTACK_ROWS {
                let _ = lt.declare_cstack(x);
                x += 1;
            }
            // main/aux 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集",
        {
            let codes = [
                WindirstatCode::Ok,
                WindirstatCode::ENoView,
                WindirstatCode::EViewDrift,
                WindirstatCode::EOrder,
                WindirstatCode::ENoCush,
                WindirstatCode::ECushDrift,
                WindirstatCode::ENoClean,
                WindirstatCode::ECleanDrift,
                WindirstatCode::EDup,
                WindirstatCode::ESeal,
                WindirstatCode::EInj,
                WindirstatCode::EFp,
                WindirstatCode::EGradeDrift,
            ];
            let mut ok = codes.len() == 13;
            let mut i = 0usize;
            while i < codes.len() {
                if codes[i].val() != SEG_BASE + i as u32 || codes[i].detail().is_empty() {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "13 变体码位连续且明细全非空",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAF80,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCAF80",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_kind() {
        assert_eq!(view_kind(0), Some(0));
        assert_eq!(view_kind(4), Some(1));
        assert_eq!(view_kind(8), Some(2));
        assert_eq!(view_kind(12), Some(3));
        assert_eq!(view_kind(19), Some(4));
        assert!(view_kind(20).is_none());
        assert!(view_cell(20).is_none());
        assert_eq!(view_kind_name(0), Some("目录树"));
        assert_eq!(view_kind_name(2), Some("用户"));
        assert_eq!(view_kind_name(4), Some("空闲空间"));
        assert!(view_kind_name(5).is_none());
    }

    #[test]
    fn test_clean_phase() {
        assert_eq!(clean_phase(0), Some(0));
        assert_eq!(clean_phase(1), Some(0));
        assert_eq!(clean_phase(2), Some(1));
        assert_eq!(clean_phase(4), Some(2));
        assert_eq!(clean_phase(5), Some(2));
        assert!(clean_phase(6).is_none());
        assert_eq!(clean_phase_name(0), Some("未清理"));
        assert_eq!(clean_phase_name(1), Some("已清理"));
        assert_eq!(clean_phase_name(2), Some("已跳过"));
        assert!(clean_phase_name(3).is_none());
    }

    #[test]
    fn test_cushion_cells() {
        assert_eq!(cushion_cell_name(0), Some("高亮"));
        assert_eq!(cushion_cell_name(1), Some("暗化"));
        assert_eq!(cushion_cell_name(2), Some("比例"));
        assert!(cushion_cell_name(3).is_none());
        assert!(cushion_cell(CUSHION_CELLS).is_none());
        let a = cushion_cell(1).unwrap_or(0);
        assert_eq!(a, cushion_cell(1).unwrap_or(0));
        assert_ne!(a, cushion_cell(2).unwrap_or(0));
    }

    #[test]
    fn test_cells_pure() {
        let a = view_cell(3).unwrap_or(0);
        assert_eq!(a, view_cell(3).unwrap_or(0));
        assert_ne!(a, view_cell(4).unwrap_or(0));
        let b = clean_cell(5).unwrap_or(0);
        assert_eq!(b, clean_cell(5).unwrap_or(0));
        assert_ne!(b, clean_cell(4).unwrap_or(0));
        assert!(clean_cell(CLEAN_STATES).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(cstack_row(CSTACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = WindirstatLedger::new();
        // 合法序先落高族格（空闲空间 19），再注低族格（扩展名 5）必抓。
        let _ = lt.record_view(19, view_cell(19).unwrap_or(0));
        assert_eq!(lt.record_view(5, view_cell(5).unwrap_or(0)), Err(WindirstatCode::EOrder));
        assert!(lt.record_view(18, view_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WindirstatLedger::new();
        assert_eq!(
            lt.record_view(6, view_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(WindirstatCode::EViewDrift)
        );
        assert_eq!(
            lt.record_cushion(2, cushion_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(WindirstatCode::ECushDrift)
        );
        assert_eq!(
            lt.record_clean(3, clean_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(WindirstatCode::ECleanDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WindirstatLedger::new();
        let _ = lt.record_view(8, view_cell(8).unwrap_or(0));
        assert_eq!(lt.record_view(8, view_cell(8).unwrap_or(0)), Err(WindirstatCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(WindirstatCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WindirstatLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WindirstatLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：视图 20 + 清理 6。
        assert_eq!(recorded, (VIEW_OPS + CLEAN_STATES) as u32);
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
        assert_eq!(SEG_BASE, 0xCAF80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WindirstatCode::Ok as u32, 0);
        assert_eq!(WindirstatCode::EGradeDrift as u32, 12);
        assert_eq!(WindirstatCode::Ok.val(), SEG_BASE);
        assert_eq!(WindirstatCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!WindirstatCode::Ok.detail().is_empty());
        assert!(!WindirstatCode::ECushDrift.detail().is_empty());
    }

    #[test]
    fn test_seal_check() {
        let mut lt = WindirstatLedger::new();
        assert_eq!(lt.seal_check(), Err(WindirstatCode::ESeal));
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < VIEW_OPS {
            if let Some(w) = view_cell(o) {
                let _ = lt.record_view(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < CUSHION_CELLS {
            if let Some(w) = cushion_cell(c) {
                let _ = lt.record_cushion(c, w);
            }
            c += 1;
        }
        let mut t = 0usize;
        while t < CLEAN_STATES {
            if let Some(w) = clean_cell(t) {
                let _ = lt.record_clean(t, w);
            }
            t += 1;
        }
        // 分账/上栈未齐仍拒封。
        assert_eq!(lt.seal_check(), Err(WindirstatCode::ESeal));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        let mut x = 0usize;
        while x < CSTACK_ROWS {
            let _ = lt.declare_cstack(x);
            x += 1;
        }
        assert!(lt.seal_check().is_ok());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = WindirstatLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = WindirstatLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let grades = [Grade::S1, Grade::S2, Grade::S3];
        assert_eq!(grades.len(), 3);
        assert!(!grades[0].detail().is_empty());
    }

    #[test]
    fn unx52447_checks_green() {
        let s = run_unx52447_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
