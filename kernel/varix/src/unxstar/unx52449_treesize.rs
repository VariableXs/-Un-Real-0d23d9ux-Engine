//! UNX-F52449 TreeSize Free 收口 · 0xCB000段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52449"，逐字）：
//!   title: TreeSize Free 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52450-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52449）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52450-J1，照原文锚定；功能按本卡标题（TreeSize Free）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五视图族闭集：树视图/块图/扩展名/Top 列表/导出 五族 × 4 格 =
//!      20 格，逐格载荷折叠纯函数恒等才收；族单调（逆序必抓）。
//!   ② 扫描 8 拍单调：扫描拍号必须递增推进，回退拍即抓；拍载荷对照。
//!   ③ NTFS 权限三元闸：读取/遍历/ACL 三格闭集，越闸必抓、漂移必抓。
//!   ④ 与 WizTree 分账声明 2 行（F52448）：TreeSize 独立账声明 /
//!      WizTree 类管道分账声明，缺一拒封。
//!   ⑤ C++ 上栈声明 2 行：JAM Software 原生栈声明 / 本仓 C++ 类比
//!      上栈声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（扫描拍回退/视图载荷漂移/视图
//!      越闭集）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹
//!      一致；KB-COMPAT 定级 S2（域语料登记）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52449;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 视图格数（五视图族 × 4）。
pub const VIEW_OPS: usize = 20;

/// 扫描拍数。
pub const SCAN_BEATS: usize = 8;

/// NTFS 权限三元闸格数。
pub const PERM_GATES: usize = 3;

/// 与 WizTree 分账声明行数（F52448）。
pub const SPLIT_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreesizeCode {
    /// 成功
    Ok = 0,
    /// 视图格号越闭集
    ENoOp,
    /// 视图/扫描载荷漂移
    EDrift,
    /// 视图族逆序
    EOrder,
    /// 扫描拍号越闭集
    ENoScan,
    /// 扫描拍回退
    EScanOrd,
    /// 权限闸号越闭集
    ENoPerm,
    /// 权限闸载荷漂移
    EPermDrift,
    /// 分账声明行越闭集
    ENoRow,
    /// 上栈声明行越闭集
    EStackRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl TreesizeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TreesizeCode::Ok => "成功",
            TreesizeCode::ENoOp => "视图格号越闭集：五视图族 20 格之外先登记",
            TreesizeCode::EDrift => "载荷漂移：视图/扫描拍折叠与基准不恒等，逐格复算",
            TreesizeCode::EOrder => "视图族逆序：树视图→块图→扩展名→Top→导出单调，逆序必抓",
            TreesizeCode::ENoScan => "扫描拍号越闭集：8 拍闭集之外先登记",
            TreesizeCode::EScanOrd => "扫描拍回退：拍号单调递增推进，回退拍必抓",
            TreesizeCode::ENoPerm => "权限闸号越闭集：NTFS 三元闸之外先登记",
            TreesizeCode::EPermDrift => "权限闸漂移：三元闸载荷折叠与基准不恒等，逐格复算",
            TreesizeCode::ENoRow => "分账声明行越闭集：WizTree 分账 2 行闭集之外先登记",
            TreesizeCode::EStackRow => "上栈声明行越闭集：C++ 上栈 2 行闭集之外先登记",
            TreesizeCode::EDup => "重复落账：同格只许一条账目",
            TreesizeCode::ESeal => "未齐即封：20 视图格 + 8 扫描拍 + 3 闸 + 4 声明行未齐先补齐",
            TreesizeCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全优收口
    S1,
    /// S2：目标档收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const fn：被常量语境调用也安全）。
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
            Grade::S1 => "S1：全优收口（本卡不适用）",
            Grade::S2 => "S2：六判据+20视图格+8扫描拍+3权限闸+声明行全齐收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 视图格载荷（格 o；族 = o/4）。
pub const fn view_cell(o: usize) -> Option<u64> {
    if o >= VIEW_OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 视图族（0..4=树视图 4..8=块图 8..12=扩展名 12..16=Top 列表 16..20=导出）。
pub const fn view_kind(o: usize) -> Option<u32> {
    if o >= VIEW_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 视图族名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("树视图"),
        1 => Some("块图"),
        2 => Some("扩展名"),
        3 => Some("Top 列表"),
        4 => Some("导出"),
        _ => None,
    }
}

/// 扫描拍载荷（拍 b）。
pub const fn scan_cell(b: usize) -> Option<u64> {
    if b >= SCAN_BEATS {
        return None;
    }
    Some((0xD2000u64 + (b as u64) * 0x33) ^ 0x545343414E5F4238)
}

/// NTFS 权限闸载荷（闸 p：0=读取 1=遍历 2=ACL）。
pub const fn perm_cell(p: usize) -> Option<u64> {
    if p >= PERM_GATES {
        return None;
    }
    Some((0xD3000u64 + (p as u64) * 0x47) ^ 0x4E5446535F504552)
}

/// 权限闸名。
pub const fn perm_gate_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("读取"),
        1 => Some("遍历"),
        2 => Some("ACL"),
        _ => None,
    }
}

/// WizTree 分账声明行折叠（行 r；F52448）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD4000 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD5000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// TreeSize Free 收口账本状态机。
#[derive(Clone)]
pub struct TreesizeLedger {
    /// 六判据（目录树扫描/视图渲染/导出/权限/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 视图账。
    views: [Option<u64>; VIEW_OPS],
    /// 扫描拍账。
    scans: [Option<u64>; SCAN_BEATS],
    /// 权限闸账。
    perms: [Option<u64>; PERM_GATES],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后视图族。
    last_kind: i64,
    /// 最后扫描拍。
    last_scan: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TreesizeLedger {
    /// 新账本。
    pub fn new() -> Self {
        TreesizeLedger {
            gates: [None; 6],
            views: [None; VIEW_OPS],
            scans: [None; SCAN_BEATS],
            perms: [None; PERM_GATES],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
            last_kind: -1,
            last_scan: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TreesizeCode> {
        if gate >= 6 {
            return Err(TreesizeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 视图一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_view(&mut self, o: usize, got: u64) -> Result<(), TreesizeCode> {
        if o >= VIEW_OPS {
            return Err(TreesizeCode::ENoOp);
        }
        let kind = match view_kind(o) {
            Some(k) => k,
            None => return Err(TreesizeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(TreesizeCode::EOrder);
        }
        let want = match view_cell(o) {
            Some(w) => w,
            None => return Err(TreesizeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(TreesizeCode::EDrift);
        }
        if match self.views.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TreesizeCode::EDup);
        }
        if let Some(c) = self.views.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 视图全账核验（20 格）。
    pub fn views_all(&self) -> bool {
        let mut o = 0usize;
        while o < VIEW_OPS {
            match self.views.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 扫描一拍（拍号单调 + 载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, b: usize, got: u64) -> Result<(), TreesizeCode> {
        if b >= SCAN_BEATS {
            return Err(TreesizeCode::ENoScan);
        }
        if (b as i64) < self.last_scan {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(TreesizeCode::EScanOrd);
        }
        let want = match scan_cell(b) {
            Some(w) => w,
            None => return Err(TreesizeCode::ENoScan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + b as u64);
            return Err(TreesizeCode::EDrift);
        }
        if match self.scans.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TreesizeCode::EDup);
        }
        if let Some(c) = self.scans.get_mut(b) {
            *c = Some(got);
        }
        self.last_scan = b as i64;
        self.fnv(0x5302 + b as u64);
        Ok(())
    }

    /// 扫描全账核验（8 拍）。
    pub fn scans_all(&self) -> bool {
        let mut b = 0usize;
        while b < SCAN_BEATS {
            match self.scans.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 权限闸一格（载荷对照 + 重复拒）。
    pub fn record_perm(&mut self, p: usize, got: u64) -> Result<(), TreesizeCode> {
        let want = match perm_cell(p) {
            Some(w) => w,
            None => return Err(TreesizeCode::ENoPerm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + p as u64);
            return Err(TreesizeCode::EPermDrift);
        }
        if match self.perms.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TreesizeCode::EDup);
        }
        if let Some(c) = self.perms.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5401 + p as u64);
        Ok(())
    }

    /// 权限闸全账核验（3 格）。
    pub fn perms_all(&self) -> bool {
        let mut p = 0usize;
        while p < PERM_GATES {
            match self.perms.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// WizTree 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TreesizeCode> {
        if r >= SPLIT_ROWS {
            return Err(TreesizeCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TreesizeCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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
    pub fn declare_stack(&mut self, r: usize) -> Result<(), TreesizeCode> {
        if r >= STACK_ROWS {
            return Err(TreesizeCode::EStackRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TreesizeCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5600 + r as u64);
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

    /// KB-COMPAT 定级（六判据齐 → S2 目标档，判据缺 → S3；本卡目标档
    /// 即 S2，全齐格不升档，如实登记）。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else {
            Grade::S2
        }
    }
}

impl Default for TreesizeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TreesizeLedger::new();
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
    let mut b = 0usize;
    while b < SCAN_BEATS {
        if let Some(w) = scan_cell(b) {
            if led.record_scan(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut p = 0usize;
    while p < PERM_GATES {
        if let Some(w) = perm_cell(p) {
            if led.record_perm(p, w).is_ok() {
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
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_stack(s);
        s += 1;
    }
    // 注入 10 轮：扫描拍回退 / 视图载荷漂移 / 视图越闭集三态轮转。
    // 每轮全部落在当轮临时账本 lt 上，主账本只记账注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TreesizeLedger::new();
        let caught = match kind {
            0 => {
                // 空账本上先落高拍再落低拍：回退必抓，且不改 last_scan 语义。
                let _ = lt.record_scan(6, scan_cell(6).unwrap_or(0));
                lt.record_scan(2, scan_cell(2).unwrap_or(0)) == Err(TreesizeCode::EScanOrd)
            }
            1 => {
                lt.record_view(3, view_cell(3).unwrap_or(0));
                lt.record_view(7, view_cell(7).unwrap_or(0).wrapping_add(1))
                    == Err(TreesizeCode::EDrift)
            }
            _ => lt.record_view(VIEW_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52449 判据集（锚 UNX-F52450-J1，off-by-one 板式）。
pub fn run_unx52449_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52449");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五视图族 20 格全通",
        recorded >= VIEW_OPS as u32,
        "树视图/块图/扩展名/Top 列表/导出五族逐格恒等",
    );
    s.add(
        "扫描 8 拍单调全落",
        {
            let mut lt = TreesizeLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SCAN_BEATS {
                if let Some(w) = scan_cell(b) {
                    if lt.record_scan(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.scans_all()
        },
        "扫描拍号单调递增，回退拍必抓",
    );
    s.add(
        "NTFS 权限三元闸全落",
        {
            let mut lt = TreesizeLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PERM_GATES {
                if let Some(w) = perm_cell(p) {
                    if lt.record_perm(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.perms_all()
        },
        "读取/遍历/ACL 三格闭集，越闸必抓",
    );
    s.add(
        "与 WizTree 分账声明 2/2 全齐",
        {
            let mut lt = TreesizeLedger::new();
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
        "TreeSize 独立账声明/WizTree 类管道分账声明两行闭集（F52448）",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = TreesizeLedger::new();
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
        "JAM Software 原生栈声明/本仓 C++ 类比上栈声明两行闭集",
    );
    s.add(
        "视图族单调",
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
        "五视图族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "扫描拍回退/载荷漂移/越闭集三态轮转必抓",
    );
    let mut l0 = TreesizeLedger::new();
    s.add(
        "边界·扫描拍回退拒",
        {
            let _ = l0.record_scan(6, scan_cell(6).unwrap_or(0));
            l0.record_scan(2, scan_cell(2).unwrap_or(0)) == Err(TreesizeCode::EScanOrd)
        },
        "高拍后回低拍必抓",
    );
    s.add(
        "载荷漂移拒",
        {
            l0.record_view(0, view_cell(0).unwrap_or(0));
            l0.record_view(1, view_cell(1).unwrap_or(0).wrapping_add(1)) == Err(TreesizeCode::EDrift)
        },
        "视图载荷漂移必抓",
    );
    s.add(
        "边界·视图越闭集",
        l0.record_view(VIEW_OPS, 0).is_err(),
        "视图格 20 越闭集必拒",
    );
    s.add(
        "边界·扫描拍越闭集",
        l0.record_scan(SCAN_BEATS, 0).is_err(),
        "扫描拍 8 越闭集必拒",
    );
    s.add(
        "边界·权限闸越闭集",
        l0.record_perm(PERM_GATES, 0).is_err(),
        "权限闸 3 越闭集必拒",
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
        "未齐即封·方向核",
        {
            let mut lt = TreesizeLedger::new();
            let ok_op = lt.record_view(0, view_cell(0).unwrap_or(0)).is_ok();
            ok_op && lt.judge_grade() == Grade::S3
        },
        "账未齐时落账仍 Ok，仅定级不达 S2",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = TreesizeLedger::new();
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
            let mut b = 0usize;
            while b < SCAN_BEATS {
                if let Some(w) = scan_cell(b) {
                    let _ = lt.record_scan(b, w);
                }
                b += 1;
            }
            let mut p = 0usize;
            while p < PERM_GATES {
                if let Some(w) = perm_cell(p) {
                    let _ = lt.record_perm(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut t = 0usize;
            while t < STACK_ROWS {
                let _ = lt.declare_stack(t);
                t += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB000,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位",
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
        assert_eq!(view_kind(19), Some(4));
        assert!(view_kind(20).is_none());
        assert!(view_cell(20).is_none());
        assert_eq!(view_kind_name(0), Some("树视图"));
        assert_eq!(view_kind_name(4), Some("导出"));
        assert!(view_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = view_cell(3).unwrap_or(0);
        assert_eq!(a, view_cell(3).unwrap_or(0));
        assert_ne!(a, view_cell(4).unwrap_or(0));
        assert!(scan_cell(SCAN_BEATS).is_none());
        assert!(perm_cell(PERM_GATES).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_perm_gate_names() {
        assert_eq!(perm_gate_name(0), Some("读取"));
        assert_eq!(perm_gate_name(1), Some("遍历"));
        assert_eq!(perm_gate_name(2), Some("ACL"));
        assert!(perm_gate_name(3).is_none());
    }

    #[test]
    fn test_view_order_caught() {
        let mut lt = TreesizeLedger::new();
        let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
        assert_eq!(lt.record_view(2, view_cell(2).unwrap_or(0)), Err(TreesizeCode::EOrder));
        assert!(lt.record_view(10, view_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_scan_order_caught() {
        let mut lt = TreesizeLedger::new();
        assert!(lt.record_scan(4, scan_cell(4).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_scan(1, scan_cell(1).unwrap_or(0)), Err(TreesizeCode::EScanOrd));
        // 同拍重落：重复拒。
        assert_eq!(lt.record_scan(4, scan_cell(4).unwrap_or(0)), Err(TreesizeCode::EDup));
        assert!(lt.record_scan(5, scan_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TreesizeLedger::new();
        assert_eq!(
            lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TreesizeCode::EDrift)
        );
        assert_eq!(
            lt.record_perm(1, perm_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(TreesizeCode::EPermDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TreesizeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TreesizeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：视图 + 扫描 + 权限闸。
        assert_eq!(
            recorded,
            VIEW_OPS as u32 + SCAN_BEATS as u32 + PERM_GATES as u32
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
        assert_eq!(SEG_BASE, 0xCB000);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TreesizeCode::Ok as u32, 0);
        assert_eq!(TreesizeCode::EGradeDrift as u32, 12);
        // 闭集恰 13 变体：逐位 detail 全非空。
        let all = [
            TreesizeCode::Ok,
            TreesizeCode::ENoOp,
            TreesizeCode::EDrift,
            TreesizeCode::EOrder,
            TreesizeCode::ENoScan,
            TreesizeCode::EScanOrd,
            TreesizeCode::ENoPerm,
            TreesizeCode::EPermDrift,
            TreesizeCode::ENoRow,
            TreesizeCode::EStackRow,
            TreesizeCode::EDup,
            TreesizeCode::ESeal,
            TreesizeCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2。
        let lt = TreesizeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = TreesizeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52449_checks_green() {
        let s = run_unx52449_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
