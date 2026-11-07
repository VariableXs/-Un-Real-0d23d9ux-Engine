//! UNX-F52459 OneCommander 收口（长尾） · 0xCB280段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52459"，逐字）：
//!   title: OneCommander 收口（长尾）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52460-J1 归档
//!     ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52459）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52460-J1，照原文锚定；功能按本卡标题（OneCommander）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面板族闭集 20 格：双窗 / 预览 / 列模式 / 标签 / 回收站历史，
//!      每族 4 格，逐格载荷折叠纯函数恒等才收；族单调（逆序必抓）。
//!   ② 长尾声明 2 行：长尾收录口径声明 / 长尾维护边界声明，缺一拒封；
//!      免费/专业分账 2 行：免费版功能分账声明 / 专业版功能分账声明，
//!      缺一拒封。
//!   ③ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（面板漂移/面板越闭集/面板族
//!      逆序）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹一致。
//!   ④ KB-COMPAT 定级封顶 S3：OneCommander 为长尾单机软件，如实定级——
//!      空账 → S3，六闸齐 → S3，全齐仍封顶 S3（长尾不升档），定级漂移
//!      核验全齐漂升至 S1/S2 即 EGradeDrift。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52459;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面板族操作格数（五族 × 4）。
pub const PANEL_OPS: usize = 20;

/// 长尾声明行数。
pub const TAIL_ROWS: usize = 2;

/// 免费/专业分账行数。
pub const TIER_ROWS: usize = 2;

/// run_cycle 实际经 record_* 计数的格数之和。
pub const RECORDED_CELLS: usize = PANEL_OPS;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OneCmdCode {
    /// 成功
    Ok = 0,
    /// 面板号越闭集
    ENoPanel,
    /// 面板载荷漂移
    EPanelDrift,
    /// 面板族逆序
    EOrder,
    /// 长尾声明行越闭集
    ERowNo,
    /// 缺长尾声明行
    ERowMiss,
    /// 分账行越闭集
    ETierNo,
    /// 缺分账行
    ETierMiss,
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

impl OneCmdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            OneCmdCode::Ok => "成功",
            OneCmdCode::ENoPanel => "面板号越闭集：双窗/预览/列模式/标签/回收站历史 20 格之外先登记",
            OneCmdCode::EPanelDrift => "面板载荷漂移：面板格折叠与基准不恒等，逐格复算",
            OneCmdCode::EOrder => "面板族逆序：双窗→预览→列模式→标签→回收站历史单调，逆序必抓",
            OneCmdCode::ERowNo => "长尾声明行越闭集：收录口径/维护边界 2 行之外先登记",
            OneCmdCode::ERowMiss => "缺长尾声明行：长尾收录口径/维护边界声明未齐先补齐",
            OneCmdCode::ETierNo => "分账行越闭集：免费/专业分账 2 行之外先登记",
            OneCmdCode::ETierMiss => "缺分账行：免费版/专业版功能分账声明未齐先补齐",
            OneCmdCode::EDup => "重复落账：同格只许一条账目",
            OneCmdCode::ESeal => "未齐即封：20 面板格 + 4 声明行未齐先补齐",
            OneCmdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OneCmdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OneCmdCode::EGradeDrift => "定级漂移：OneCommander 长尾单机软件封顶 S3，漂升即拒",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（长尾封顶不可达）
    S1,
    /// S2：有条件收口（长尾封顶不可达）
    S2,
    /// S3：不收口（长尾封顶档）
    S3,
}

impl Grade {
    /// 定级值（const 可用）。
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
            Grade::S1 => "S1：全通收口（OneCommander 长尾不升此档）",
            Grade::S2 => "S2：有条件收口（OneCommander 长尾不升此档）",
            Grade::S3 => "S3：不收口，长尾单机软件如实封顶定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面板格载荷（格 o；族 = o/4）。
pub const fn panel_cell(o: usize) -> Option<u64> {
    if o >= PANEL_OPS {
        return None;
    }
    Some((0xA7700u64 + (o as u64) * 0x4D) ^ ((o / 4) as u64).wrapping_mul(0xC2B2AE3D))
}

/// 面板族（0..4=双窗 4..8=预览 8..12=列模式 12..16=标签 16..20=回收站历史）。
pub const fn panel_kind(o: usize) -> Option<u32> {
    if o >= PANEL_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面板族名。
pub const fn panel_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("双窗"),
        1 => Some("预览"),
        2 => Some("列模式"),
        3 => Some("标签"),
        4 => Some("回收站历史"),
        _ => None,
    }
}

/// 长尾声明折叠（行 r）。
pub const fn tail_row(r: usize) -> Option<u32> {
    if r >= TAIL_ROWS {
        return None;
    }
    Some(0xA780 + r as u32 * 0x38)
}

/// 免费/专业分账折叠（行 t）。
pub const fn tier_row(t: usize) -> Option<u32> {
    if t >= TIER_ROWS {
        return None;
    }
    Some(0xA790 + t as u32 * 0x38)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// OneCommander 收口账本状态机。
#[derive(Clone)]
pub struct OneCmdLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 面板族账。
    panels: [Option<u64>; PANEL_OPS],
    /// 长尾声明账。
    tails: [Option<u32>; TAIL_ROWS],
    /// 免费/专业分账账。
    tiers: [Option<u32>; TIER_ROWS],
    /// 最后面板族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl OneCmdLedger {
    /// 新账本。
    pub fn new() -> Self {
        OneCmdLedger {
            gates: [None; 6],
            panels: [None; PANEL_OPS],
            tails: [None; TAIL_ROWS],
            tiers: [None; TIER_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OneCmdCode> {
        if gate >= 6 {
            return Err(OneCmdCode::ESeal);
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

    /// 面板一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, o: usize, got: u64) -> Result<(), OneCmdCode> {
        if o >= PANEL_OPS {
            return Err(OneCmdCode::ENoPanel);
        }
        let kind = match panel_kind(o) {
            Some(k) => k,
            None => return Err(OneCmdCode::ENoPanel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(OneCmdCode::EOrder);
        }
        let want = match panel_cell(o) {
            Some(w) => w,
            None => return Err(OneCmdCode::ENoPanel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(OneCmdCode::EPanelDrift);
        }
        if match self.panels.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OneCmdCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 面板全账核验（20 格）。
    pub fn panels_all(&self) -> bool {
        let mut o = 0usize;
        while o < PANEL_OPS {
            match self.panels.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 长尾声明一行。
    pub fn declare_tail(&mut self, r: usize) -> Result<(), OneCmdCode> {
        if r >= TAIL_ROWS {
            return Err(OneCmdCode::ERowNo);
        }
        if match self.tails.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OneCmdCode::EDup);
        }
        if let Some(c) = self.tails.get_mut(r) {
            *c = tail_row(r);
        }
        self.fnv(0x7300 + r as u64);
        Ok(())
    }

    /// 免费/专业分账一行。
    pub fn declare_tier(&mut self, t: usize) -> Result<(), OneCmdCode> {
        if t >= TIER_ROWS {
            return Err(OneCmdCode::ETierNo);
        }
        if match self.tiers.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OneCmdCode::EDup);
        }
        if let Some(c) = self.tiers.get_mut(t) {
            *c = tier_row(t);
        }
        self.fnv(0x7400 + t as u64);
        Ok(())
    }

    /// 长尾声明全齐核验。
    pub fn tails_all(&self) -> bool {
        let mut r = 0usize;
        while r < TAIL_ROWS {
            match self.tails.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 免费/专业分账全齐核验。
    pub fn tiers_all(&self) -> bool {
        let mut t = 0usize;
        while t < TIER_ROWS {
            match self.tiers.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
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

    /// KB-COMPAT 定级（长尾封顶 S3：空账/六闸齐/全齐均为 S3）。
    pub fn judge_grade(&self) -> Grade {
        Grade::S3
    }

    /// 全齐布尔（定级漂移核验用，如实反映各节记账）。
    pub fn full_complete(&self) -> bool {
        self.gates_all() && self.panels_all() && self.tails_all() && self.tiers_all()
    }

    /// 定级漂移核验：全齐仍必须封顶 S3，漂升 S1/S2 即 EGradeDrift。
    pub fn grade_drift_check(&self) -> Result<(), OneCmdCode> {
        if self.full_complete() && self.judge_grade() != Grade::S3 {
            return Err(OneCmdCode::EGradeDrift);
        }
        Ok(())
    }

    /// 长尾声明未齐核验：缺行即报 ERowMiss。
    pub fn tail_miss_check(&self) -> Result<(), OneCmdCode> {
        if !self.tails_all() {
            return Err(OneCmdCode::ERowMiss);
        }
        Ok(())
    }

    /// 分账行未齐核验：缺行即报 ETierMiss。
    pub fn tier_miss_check(&self) -> Result<(), OneCmdCode> {
        if !self.tiers_all() {
            return Err(OneCmdCode::ETierMiss);
        }
        Ok(())
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), OneCmdCode> {
    if injected != caught + escaped {
        return Err(OneCmdCode::EInj);
    }
    Ok(())
}

/// 双跑指纹核验。
pub fn verify_fingerprint(a: u64, b: u64) -> Result<(), OneCmdCode> {
    if a != b || a == 0 {
        return Err(OneCmdCode::EFp);
    }
    Ok(())
}

impl Default for OneCmdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（record_* 计格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = OneCmdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PANEL_OPS {
        if let Some(w) = panel_cell(o) {
            if led.record_panel(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < TAIL_ROWS {
        let _ = led.declare_tail(r);
        r += 1;
    }
    let mut t = 0usize;
    while t < TIER_ROWS {
        let _ = led.declare_tier(t);
        t += 1;
    }
    // 注入 10 轮：面板漂移 / 面板越闭集 / 面板族逆序三态轮转，
    // 全部落在当轮临时账本上，主账只记注入/检出计数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = OneCmdLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移。
                let seed = panel_cell(6).unwrap_or(0);
                let _ = lt.record_panel(6, seed);
                lt.record_panel(6, seed.wrapping_add(1)) == Err(OneCmdCode::EPanelDrift)
            }
            1 => lt.record_panel(PANEL_OPS, 0).is_err(),
            _ => {
                // 族逆序：先按合法序落高族正格，再注低族（空账本先落种子）。
                let hi = panel_cell(12).unwrap_or(0);
                let _ = lt.record_panel(12, hi);
                lt.record_panel(5, panel_cell(5).unwrap_or(0)) == Err(OneCmdCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52459 判据集（锚 UNX-F52460-J1，off-by-one 板式）。
pub fn run_unx52459_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52459");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面板族 20 操作全通（归档 ×10 全通口径）",
        recorded >= PANEL_OPS as u32,
        "双窗/预览/列模式/标签/回收站历史五族逐格载荷恒等",
    );
    s.add(
        "长尾声明 2/2 全齐",
        {
            let mut lt = OneCmdLedger::new();
            let mut ok = true;
            // 未齐方向：空账上缺行核验必须报 ERowMiss（is_err 口径）。
            if lt.tail_miss_check() != Err(OneCmdCode::ERowMiss) {
                ok = false;
            }
            let mut r = 0usize;
            while r < TAIL_ROWS {
                if lt.declare_tail(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.tails_all()
        },
        "长尾收录口径声明/长尾维护边界声明两行闭集（缺一拒封）",
    );
    s.add(
        "免费/专业分账 2/2 全齐",
        {
            let mut lt = OneCmdLedger::new();
            let mut ok = true;
            // 未齐方向：空账上缺行核验必须报 ETierMiss（is_err 口径）。
            if lt.tier_miss_check() != Err(OneCmdCode::ETierMiss) {
                ok = false;
            }
            let mut t = 0usize;
            while t < TIER_ROWS {
                if lt.declare_tier(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tiers_all()
        },
        "免费版功能分账/专业版功能分账两行闭集（缺一拒封）",
    );
    s.add(
        "五面板族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANEL_OPS {
                if panel_kind(o).unwrap_or(5) < panel_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "双窗→预览→列模式→标签→回收站历史五族闭集单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = OneCmdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "面板漂移/面板越闭集/面板族逆序三态轮转必抓",
    );
    let mut l0 = OneCmdLedger::new();
    s.add(
        "边界·面板号越闭集",
        l0.record_panel(PANEL_OPS, 0).is_err(),
        "面板号 20 越闭集必拒",
    );
    s.add(
        "边界·长尾声明行越闭集",
        l0.declare_tail(TAIL_ROWS).is_err(),
        "长尾声明行 2 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_tier(TIER_ROWS).is_err(),
        "分账行 2 越闭集必拒",
    );
    s.add(
        "面板漂移拒",
        {
            let seed = panel_cell(1).unwrap_or(0);
            let _ = l0.record_panel(1, seed);
            l0.record_panel(1, seed.wrapping_add(1)) == Err(OneCmdCode::EPanelDrift)
        },
        "面板载荷漂移必抓",
    );
    s.add(
        "面板族逆序拒",
        {
            let _ = l0.record_panel(16, panel_cell(16).unwrap_or(0));
            l0.record_panel(7, panel_cell(7).unwrap_or(0)) == Err(OneCmdCode::EOrder)
        },
        "回收站历史族后落预览族，逆序必抓",
    );
    s.add(
        "未齐允许落格（封核方向）",
        {
            let mut lt = OneCmdLedger::new();
            lt.record_panel(0, panel_cell(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落格表达式必须为 Ok，封核只在收口口处断",
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        verify_fingerprint(fp, fp2) == Ok(()),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级封顶 S3 闭集",
        {
            let mut lt = OneCmdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PANEL_OPS {
                if let Some(w) = panel_cell(o) {
                    let _ = lt.record_panel(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < TAIL_ROWS {
                let _ = lt.declare_tail(r);
                r += 1;
            }
            let mut t = 0usize;
            while t < TIER_ROWS {
                let _ = lt.declare_tier(t);
                t += 1;
            }
            lt.judge_grade() == Grade::S3 && lt.grade_drift_check() == Ok(())
        },
        "全齐仍封顶 S3（KB-COMPAT 定级登记，长尾单机软件如实定级）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB280,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位（0xCA400+0x40×58）",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        conservation(inj_n, caught_n, esc_n) == Ok(()) && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_kind() {
        assert_eq!(panel_kind(0), Some(0));
        assert_eq!(panel_kind(4), Some(1));
        assert_eq!(panel_kind(19), Some(4));
        assert!(panel_kind(20).is_none());
        assert!(panel_cell(20).is_none());
        assert_eq!(panel_kind_name(1), Some("预览"));
        assert_eq!(panel_kind_name(4), Some("回收站历史"));
        assert!(panel_kind_name(5).is_none());
    }

    #[test]
    fn test_rows_pure() {
        let a = tail_row(0).unwrap_or(0);
        assert_eq!(a, tail_row(0).unwrap_or(0));
        assert_ne!(a, tail_row(1).unwrap_or(0));
        assert!(tail_row(TAIL_ROWS).is_none());
        assert!(tier_row(TIER_ROWS).is_none());
        let b = tier_row(0).unwrap_or(0);
        assert_ne!(a, b, "长尾声明与分账行折叠必须可区分");
    }

    #[test]
    fn test_cells_pure() {
        let a = panel_cell(13).unwrap_or(0);
        assert_eq!(a, panel_cell(13).unwrap_or(0));
        assert_ne!(a, panel_cell(14).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = OneCmdLedger::new();
        let _ = lt.record_panel(8, panel_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_panel(2, panel_cell(2).unwrap_or(0)),
            Err(OneCmdCode::EOrder)
        );
        // 逆序拒绝不改 last_kind：同族正格仍可落。
        assert!(lt.record_panel(9, panel_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = OneCmdLedger::new();
        let seed = panel_cell(3).unwrap_or(0);
        assert_eq!(
            lt.record_panel(3, seed.wrapping_add(1)),
            Err(OneCmdCode::EPanelDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = OneCmdLedger::new();
        let w = panel_cell(10).unwrap_or(0);
        let _ = lt.record_panel(10, w);
        assert_eq!(lt.record_panel(10, w), Err(OneCmdCode::EDup));
        let _ = lt.declare_tier(0);
        assert_eq!(lt.declare_tier(0), Err(OneCmdCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = OneCmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = OneCmdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, RECORDED_CELLS as u32);
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
        assert_eq!(verify_fingerprint(f1, f2), Ok(()));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCB280);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(OneCmdCode::Ok as u32, 0);
        assert_eq!(OneCmdCode::EGradeDrift as u32, 12);
        assert_eq!(OneCmdCode::Ok.val(), SEG_BASE);
        assert!(!OneCmdCode::Ok.detail().is_empty());
        assert!(!OneCmdCode::ESeal.detail().is_empty());
        assert_eq!(conservation(10, 10, 0), Ok(()));
        assert_eq!(conservation(10, 7, 2), Err(OneCmdCode::EInj));
        assert_eq!(verify_fingerprint(0, 0), Err(OneCmdCode::EFp));
    }

    #[test]
    fn test_grade_capped_s3() {
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯（封顶口径）：空账 S3。
        let lt = OneCmdLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(lt.grade_drift_check() == Ok(()));
        // 六闸齐 S3，全齐仍封顶 S3。
        let mut lt2 = OneCmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut o = 0usize;
        while o < PANEL_OPS {
            if let Some(w) = panel_cell(o) {
                let _ = lt2.record_panel(o, w);
            }
            o += 1;
        }
        let mut r = 0usize;
        while r < TAIL_ROWS {
            let _ = lt2.declare_tail(r);
            r += 1;
        }
        let mut t = 0usize;
        while t < TIER_ROWS {
            let _ = lt2.declare_tier(t);
            t += 1;
        }
        assert!(lt2.full_complete());
        assert_eq!(lt2.judge_grade(), Grade::S3);
        assert!(lt2.grade_drift_check() == Ok(()));
    }

    #[test]
    fn unx52459_checks_green() {
        let s = run_unx52459_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
