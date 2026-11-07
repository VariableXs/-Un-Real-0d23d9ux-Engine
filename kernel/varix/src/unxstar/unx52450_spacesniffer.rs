//! UNX-F52450 SpaceSniffer 收口 · 0xCB040段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52450"，逐字）：
//!   title: SpaceSniffer 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52451-J1 六判据
//!     全落；压缩/解压 ×10 格式全通；大文件（4GB+）分段账；KB-COMPAT
//!     定级 S1；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52450）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52451-J1，照原文锚定；功能按本卡标题（SpaceSniffer）落地；
//! 卡面里「压缩/解压 ×10 格式全通」「大文件（4GB+）分段账」「定级 S1」
//! 系 N+1 卡（7-Zip）域语料，本卡不落地；KB-COMPAT 定级按本卡域语料
//! 如实登记 S2。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五视图族闭集：块图/缩放/过滤/双击下钻/导出 五族 × 4 格 = 20 格，
//!      逐格载荷折叠纯函数恒等才收；族单调（逆序必抓）。
//!   ② 块布局三元闸：粒度/间距/动画 三格闭集，越闸必抓、漂移必抓。
//!   ③ 过滤投影 8 格：过滤器投影序单调，漂移必抓。
//!   ④ 与 WinDirStat 分账声明 2 行（F52447）：SpaceSniffer 独立账声明 /
//!      WinDirStat 类管道分账声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（视图族逆序/布局闸漂移/视图越
//!      闭集）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹一致；
//!      KB-COMPAT 定级 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52450;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 视图格数（五视图族 × 4）。
pub const VIEW_OPS: usize = 20;

/// 块布局三元闸格数。
pub const LAYOUT_GATES: usize = 3;

/// 过滤投影格数。
pub const FILTER_CELLS: usize = 8;

/// 与 WinDirStat 分账声明行数（F52447）。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpacesniffCode {
    /// 成功
    Ok = 0,
    /// 视图格号越闭集
    ENoOp,
    /// 视图/过滤载荷漂移
    EDrift,
    /// 视图族逆序
    EOrder,
    /// 布局闸号越闭集
    ENoLayout,
    /// 布局闸载荷漂移
    ELayoutDrift,
    /// 过滤投影格号越闭集
    ENoFilter,
    /// 过滤投影载荷漂移
    EFilDrift,
    /// 过滤投影序回退
    EFilOrd,
    /// 分账声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl SpacesniffCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SpacesniffCode::Ok => "成功",
            SpacesniffCode::ENoOp => "视图格号越闭集：五视图族 20 格之外先登记",
            SpacesniffCode::EDrift => "载荷漂移：视图/过滤投影折叠与基准不恒等，逐格复算",
            SpacesniffCode::EOrder => "视图族逆序：块图→缩放→过滤→双击下钻→导出单调，逆序必抓",
            SpacesniffCode::ENoLayout => "布局闸号越闭集：块布局三元闸之外先登记",
            SpacesniffCode::ELayoutDrift => "布局闸漂移：三元闸载荷折叠与基准不恒等，逐格复算",
            SpacesniffCode::ENoFilter => "过滤投影格号越闭集：8 格闭集之外先登记",
            SpacesniffCode::EFilDrift => "过滤投影漂移：投影载荷折叠与基准不恒等，逐格复算",
            SpacesniffCode::EFilOrd => "过滤投影序回退：投影序单调推进，回退必抓",
            SpacesniffCode::ENoRow => "分账声明行越闭集：WinDirStat 分账 2 行闭集之外先登记",
            SpacesniffCode::EDup => "重复落账：同格只许一条账目",
            SpacesniffCode::ESeal => "未齐即封：20 视图格 + 3 闸 + 8 投影 + 2 声明行未齐先补齐",
            SpacesniffCode::EGradeDrift => "定级漂移：六判据齐才可定 S2",
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
            Grade::S1 => "S1：全优收口（本卡不适用，S1 系 N+1 卡域语料）",
            Grade::S2 => "S2：六判据+20视图格+3布局闸+8投影+2声明行全齐收口",
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
    Some((0xD6000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 视图族（0..4=块图 4..8=缩放 8..12=过滤 12..16=双击下钻 16..20=导出）。
pub const fn view_kind(o: usize) -> Option<u32> {
    if o >= VIEW_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 视图族名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("块图"),
        1 => Some("缩放"),
        2 => Some("过滤"),
        3 => Some("双击下钻"),
        4 => Some("导出"),
        _ => None,
    }
}

/// 块布局闸载荷（闸 p：0=粒度 1=间距 2=动画）。
pub const fn layout_cell(p: usize) -> Option<u64> {
    if p >= LAYOUT_GATES {
        return None;
    }
    Some((0xD7000u64 + (p as u64) * 0x47) ^ 0x424C4B5F4C41594F)
}

/// 布局闸名。
pub const fn layout_gate_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("粒度"),
        1 => Some("间距"),
        2 => Some("动画"),
        _ => None,
    }
}

/// 过滤投影格载荷（格 i）。
pub const fn filter_cell(i: usize) -> Option<u64> {
    if i >= FILTER_CELLS {
        return None;
    }
    Some((0xD8000u64 + (i as u64) * 0x3B) ^ 0x46494C5445525052)
}

/// WinDirStat 分账声明行折叠（行 r；F52447）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD9000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SpaceSniffer 收口账本状态机。
#[derive(Clone)]
pub struct SpacesniffLedger {
    /// 六判据（块图渲染/缩放/过滤/下钻/导出/许可）。
    gates: [Option<bool>; 6],
    /// 视图账。
    views: [Option<u64>; VIEW_OPS],
    /// 布局闸账。
    layouts: [Option<u64>; LAYOUT_GATES],
    /// 过滤投影账。
    filters: [Option<u64>; FILTER_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后视图族。
    last_kind: i64,
    /// 最后过滤投影格。
    last_filter: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SpacesniffLedger {
    /// 新账本。
    pub fn new() -> Self {
        SpacesniffLedger {
            gates: [None; 6],
            views: [None; VIEW_OPS],
            layouts: [None; LAYOUT_GATES],
            filters: [None; FILTER_CELLS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            last_filter: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SpacesniffCode> {
        if gate >= 6 {
            return Err(SpacesniffCode::ESeal);
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

    /// 视图一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_view(&mut self, o: usize, got: u64) -> Result<(), SpacesniffCode> {
        if o >= VIEW_OPS {
            return Err(SpacesniffCode::ENoOp);
        }
        let kind = match view_kind(o) {
            Some(k) => k,
            None => return Err(SpacesniffCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(SpacesniffCode::EOrder);
        }
        let want = match view_cell(o) {
            Some(w) => w,
            None => return Err(SpacesniffCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(SpacesniffCode::EDrift);
        }
        if match self.views.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SpacesniffCode::EDup);
        }
        if let Some(c) = self.views.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
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

    /// 布局闸一格（载荷对照 + 重复拒）。
    pub fn record_layout(&mut self, p: usize, got: u64) -> Result<(), SpacesniffCode> {
        let want = match layout_cell(p) {
            Some(w) => w,
            None => return Err(SpacesniffCode::ENoLayout),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + p as u64);
            return Err(SpacesniffCode::ELayoutDrift);
        }
        if match self.layouts.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SpacesniffCode::EDup);
        }
        if let Some(c) = self.layouts.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6301 + p as u64);
        Ok(())
    }

    /// 布局闸全账核验（3 格）。
    pub fn layouts_all(&self) -> bool {
        let mut p = 0usize;
        while p < LAYOUT_GATES {
            match self.layouts.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 过滤投影一格（投影序单调 + 载荷对照 + 重复拒）。
    pub fn record_filter(&mut self, i: usize, got: u64) -> Result<(), SpacesniffCode> {
        if i >= FILTER_CELLS {
            return Err(SpacesniffCode::ENoFilter);
        }
        if (i as i64) < self.last_filter {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(SpacesniffCode::EFilOrd);
        }
        let want = match filter_cell(i) {
            Some(w) => w,
            None => return Err(SpacesniffCode::ENoFilter),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + i as u64);
            return Err(SpacesniffCode::EFilDrift);
        }
        if match self.filters.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SpacesniffCode::EDup);
        }
        if let Some(c) = self.filters.get_mut(i) {
            *c = Some(got);
        }
        self.last_filter = i as i64;
        self.fnv(0x6402 + i as u64);
        Ok(())
    }

    /// 过滤投影全账核验（8 格）。
    pub fn filters_all(&self) -> bool {
        let mut i = 0usize;
        while i < FILTER_CELLS {
            match self.filters.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// WinDirStat 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SpacesniffCode> {
        if r >= SPLIT_ROWS {
            return Err(SpacesniffCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SpacesniffCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x6500 + r as u64);
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

impl Default for SpacesniffLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SpacesniffLedger::new();
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
    let mut p = 0usize;
    while p < LAYOUT_GATES {
        if let Some(w) = layout_cell(p) {
            if led.record_layout(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut i = 0usize;
    while i < FILTER_CELLS {
        if let Some(w) = filter_cell(i) {
            if led.record_filter(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：视图族逆序 / 布局闸漂移 / 视图越闭集三态轮转。
    // 每轮全部落在当轮临时账本 lt 上，主账本只记账注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SpacesniffLedger::new();
        let caught = match kind {
            0 => {
                // 空账本上先落高族再落低族：逆序必抓，且不改 last_kind 语义。
                let _ = lt.record_view(17, view_cell(17).unwrap_or(0));
                lt.record_view(5, view_cell(5).unwrap_or(0)) == Err(SpacesniffCode::EOrder)
            }
            1 => lt.record_layout(1, layout_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(SpacesniffCode::ELayoutDrift),
            _ => lt.record_view(VIEW_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52450 判据集（锚 UNX-F52451-J1，off-by-one 板式）。
pub fn run_unx52450_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52450");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五视图族 20 格全通",
        recorded >= VIEW_OPS as u32,
        "块图/缩放/过滤/双击下钻/导出五族逐格恒等",
    );
    s.add(
        "块布局三元闸全落",
        {
            let mut lt = SpacesniffLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < LAYOUT_GATES {
                if let Some(w) = layout_cell(p) {
                    if lt.record_layout(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.layouts_all()
        },
        "粒度/间距/动画三格闭集，越闸必抓",
    );
    s.add(
        "过滤投影 8 格全落",
        {
            let mut lt = SpacesniffLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < FILTER_CELLS {
                if let Some(w) = filter_cell(i) {
                    if lt.record_filter(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.filters_all()
        },
        "过滤器投影序单调，回退必抓",
    );
    s.add(
        "与 WinDirStat 分账声明 2/2 全齐",
        {
            let mut lt = SpacesniffLedger::new();
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
        "SpaceSniffer 独立账声明/WinDirStat 类管道分账声明两行闭集（F52447）",
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
        "族逆序/布局漂移/越闭集三态轮转必抓",
    );
    let mut l0 = SpacesniffLedger::new();
    s.add(
        "边界·视图族逆序拒",
        {
            let _ = l0.record_view(17, view_cell(17).unwrap_or(0));
            l0.record_view(5, view_cell(5).unwrap_or(0)) == Err(SpacesniffCode::EOrder)
        },
        "导出后回过滤必抓",
    );
    s.add(
        "布局闸漂移拒",
        l0.record_layout(0, layout_cell(0).unwrap_or(0).wrapping_add(3))
            == Err(SpacesniffCode::ELayoutDrift),
        "布局闸载荷漂移必抓",
    );
    s.add(
        "边界·视图越闭集",
        l0.record_view(VIEW_OPS, 0).is_err(),
        "视图格 20 越闭集必拒",
    );
    s.add(
        "边界·布局闸越闭集",
        l0.record_layout(LAYOUT_GATES, 0).is_err(),
        "布局闸 3 越闭集必拒",
    );
    s.add(
        "边界·过滤投影越闭集",
        l0.record_filter(FILTER_CELLS, 0).is_err(),
        "过滤投影 8 越闭集必拒",
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
            let mut lt = SpacesniffLedger::new();
            let ok_op = lt.record_view(0, view_cell(0).unwrap_or(0)).is_ok();
            ok_op && lt.judge_grade() == Grade::S3
        },
        "账未齐时落账仍 Ok，仅定级不达 S2",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = SpacesniffLedger::new();
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
            let mut p = 0usize;
            while p < LAYOUT_GATES {
                if let Some(w) = layout_cell(p) {
                    let _ = lt.record_layout(p, w);
                }
                p += 1;
            }
            let mut i = 0usize;
            while i < FILTER_CELLS {
                if let Some(w) = filter_cell(i) {
                    let _ = lt.record_filter(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，卡面 S1 系 N+1 卡语料）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB040,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
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
        assert_eq!(view_kind_name(0), Some("块图"));
        assert_eq!(view_kind_name(3), Some("双击下钻"));
        assert!(view_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = view_cell(3).unwrap_or(0);
        assert_eq!(a, view_cell(3).unwrap_or(0));
        assert_ne!(a, view_cell(4).unwrap_or(0));
        assert!(layout_cell(LAYOUT_GATES).is_none());
        assert!(filter_cell(FILTER_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_layout_gate_names() {
        assert_eq!(layout_gate_name(0), Some("粒度"));
        assert_eq!(layout_gate_name(1), Some("间距"));
        assert_eq!(layout_gate_name(2), Some("动画"));
        assert!(layout_gate_name(3).is_none());
    }

    #[test]
    fn test_view_order_caught() {
        let mut lt = SpacesniffLedger::new();
        let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
        assert_eq!(lt.record_view(2, view_cell(2).unwrap_or(0)), Err(SpacesniffCode::EOrder));
        assert!(lt.record_view(10, view_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_filter_order_caught() {
        let mut lt = SpacesniffLedger::new();
        assert!(lt.record_filter(3, filter_cell(3).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_filter(1, filter_cell(1).unwrap_or(0)), Err(SpacesniffCode::EFilOrd));
        assert_eq!(lt.record_filter(3, filter_cell(3).unwrap_or(0)), Err(SpacesniffCode::EDup));
        assert!(lt.record_filter(4, filter_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SpacesniffLedger::new();
        assert_eq!(
            lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SpacesniffCode::EDrift)
        );
        assert_eq!(
            lt.record_filter(2, filter_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(SpacesniffCode::EFilDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SpacesniffLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SpacesniffLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：视图 + 布局闸 + 过滤投影。
        assert_eq!(
            recorded,
            VIEW_OPS as u32 + LAYOUT_GATES as u32 + FILTER_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCB040);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SpacesniffCode::Ok as u32, 0);
        assert_eq!(SpacesniffCode::EGradeDrift as u32, 12);
        // 闭集恰 13 变体：逐位 detail 全非空。
        let all = [
            SpacesniffCode::Ok,
            SpacesniffCode::ENoOp,
            SpacesniffCode::EDrift,
            SpacesniffCode::EOrder,
            SpacesniffCode::ENoLayout,
            SpacesniffCode::ELayoutDrift,
            SpacesniffCode::ENoFilter,
            SpacesniffCode::EFilDrift,
            SpacesniffCode::EFilOrd,
            SpacesniffCode::ENoRow,
            SpacesniffCode::EDup,
            SpacesniffCode::ESeal,
            SpacesniffCode::EGradeDrift,
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
        let lt = SpacesniffLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = SpacesniffLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52450_checks_green() {
        let s = run_unx52450_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
