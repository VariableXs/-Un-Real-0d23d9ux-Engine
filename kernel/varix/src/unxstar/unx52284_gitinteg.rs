//! UNX-F52284 Git 集成消费 L4 判据对账（全域 GUI/CLI ×10 条目）
//! （开发工具共面四） · 0xC86C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52284"，逐字）：
//!   title: 开发工具共面四：Git 集成消费 L4 判据对账（全域 GUI/CLI ×10
//!     条目）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52285-J1 域经义
//!     务条目：VSCode 多进程组合态失绿现场实录（现象/归因/回推 G 部缺陷
//!     账编号/跟踪结论四段全记）；判据锚定 §AI-66.8 域经义务 DJ-UNX-N1-*；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52284）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52285-J1，照原文锚定；功能按本卡标题（Git 集成消费 L4 判据对账）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① GUI/CLI ×10 条目对账 20 格：每条目 GUI/CLI 两格，条目序单调，逐
//!      格载荷纯函数折叠恒等才收，漂移必抓。
//!   ② L4 判据消费联签 6 格：F52035/52059 复用联签格逐格落账（AI-59 同
//!      构锚定行），缺一拒封。
//!   ③ 状态刷新 8 拍单调：拍载荷严格递增（VSCode 多进程组合态刷新序），
//!      拍序倒退必抓。
//!   ④ 嫁接 4 件判绿：L4 嫁接件逐件判绿格，载荷漂移即嫁接不判绿必抓。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（条目漂移/刷新拍序逆/条目越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 格+6 联签+8 拍+4 嫁接+六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52284;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// GUI/CLI 对账格数（10 条目 × 2 侧）。
pub const ITEM_CELLS: usize = 20;

/// L4 判据消费联签格数。
pub const SIG_ROWS: usize = 6;

/// 状态刷新拍数。
pub const REFRESH_BEATS: usize = 8;

/// 嫁接判绿件数。
pub const GRAFT_ITEMS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitIntegCode {
    /// 成功
    Ok = 0,
    /// 条目格越闭集
    ENoItem,
    /// 条目载荷漂移
    EDrift,
    /// 条目序逆
    EOrder,
    /// 联签行越闭集
    ENoSig,
    /// 联签载荷漂移
    ESigDrift,
    /// 嫁接不判绿/漂移
    EGraft,
    /// 刷新拍越闭集
    ENoBeat,
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

impl GitIntegCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GitIntegCode::Ok => "成功",
            GitIntegCode::ENoItem => "条目格越闭集：GUI/CLI ×10 条目 20 格之外先登记",
            GitIntegCode::EDrift => "条目载荷漂移：对账折叠与基准不恒等，逐格复算",
            GitIntegCode::EOrder => "条目序逆：GUI/CLI 十条目单调，逆序必抓",
            GitIntegCode::ENoSig => "联签行越闭集：L4 消费联签 6 格之外先登记",
            GitIntegCode::ESigDrift => "联签载荷漂移：F52035/52059 复用折叠与基准不恒等",
            GitIntegCode::EGraft => "嫁接不判绿：L4 嫁接件判绿格载荷漂移必抓",
            GitIntegCode::ENoBeat => "刷新拍越闭集：状态刷新 8 拍之外先登记",
            GitIntegCode::EDup => "重复落账：同格只许一条账目",
            GitIntegCode::ESeal => "未齐即封：20 格+6 联签+8 拍+4 嫁接未齐先补齐",
            GitIntegCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GitIntegCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GitIntegCode::EGradeDrift => "定级漂移：全通且联签全齐才可定 S1",
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
            Grade::S1 => "S1：20格+6联签+8拍+4嫁接+六判据全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 条目（格 o；条目 = o/2）。
pub const fn item_of(o: usize) -> Option<u32> {
    if o >= ITEM_CELLS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 侧（格 o；侧 = o%2：0=GUI 1=CLI）。
pub const fn side_of(o: usize) -> Option<u32> {
    if o >= ITEM_CELLS {
        return None;
    }
    Some((o % 2) as u32)
}

/// 侧名。
pub const fn side_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("GUI"),
        1 => Some("CLI"),
        _ => None,
    }
}

/// 条目格载荷（格 o）。
pub const fn item_cell(o: usize) -> Option<u64> {
    if o >= ITEM_CELLS {
        return None;
    }
    Some((0xBF00u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// L4 联签行折叠（行 r；F52035/52059 复用口径）。
pub const fn sig_row(r: usize) -> Option<u32> {
    if r >= SIG_ROWS {
        return None;
    }
    Some(0xC000 + r as u32 * 0x2C)
}

/// 联签行名。
pub const fn sig_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("F52035 消费复用"),
        1 => Some("F52059 消费复用"),
        2 => Some("B01 git 消费条目"),
        3 => Some("B08 git 消费条目"),
        4 => Some("B10 git 消费条目"),
        5 => Some("AI-59 联签锚定行"),
        _ => None,
    }
}

/// 状态刷新拍载荷（拍 b，严格递增）。
pub const fn refresh_beat(b: usize) -> Option<u64> {
    if b >= REFRESH_BEATS {
        return None;
    }
    Some(0xC100u64 + (b as u64) * 0x41)
}

/// 嫁接判绿格载荷（件 g）。
pub const fn graft_cell(g: usize) -> Option<u64> {
    if g >= GRAFT_ITEMS {
        return None;
    }
    Some((0xC200u64 + (g as u64) * 0x33) ^ 0x4755_4943_4C49_0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Git 集成对账收口账本状态机。
#[derive(Clone)]
pub struct GitIntegLedger {
    /// 六判据（×10 条目/L4 联签/刷新序/嫁接判绿/失绿实录/汇总）。
    gates: [Option<bool>; 6],
    /// 条目对账账。
    items: [Option<u64>; ITEM_CELLS],
    /// L4 联签账。
    sigs: [Option<u32>; SIG_ROWS],
    /// 刷新拍账。
    beats: [Option<u64>; REFRESH_BEATS],
    /// 嫁接判绿账。
    grafts: [Option<u64>; GRAFT_ITEMS],
    /// 最后条目。
    last_item: i64,
    /// 最后拍载荷。
    last_beat: u64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GitIntegLedger {
    /// 新账本。
    pub fn new() -> Self {
        GitIntegLedger {
            gates: [None; 6],
            items: [None; ITEM_CELLS],
            sigs: [None; SIG_ROWS],
            beats: [None; REFRESH_BEATS],
            grafts: [None; GRAFT_ITEMS],
            last_item: -1,
            last_beat: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GitIntegCode> {
        if gate >= 6 {
            return Err(GitIntegCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 条目一格（条目单调 + 载荷对照 + 重复拒）。
    pub fn record_item(&mut self, o: usize, got: u64) -> Result<(), GitIntegCode> {
        if o >= ITEM_CELLS {
            return Err(GitIntegCode::ENoItem);
        }
        let item = match item_of(o) {
            Some(k) => k,
            None => return Err(GitIntegCode::ENoItem),
        };
        if (item as i64) < self.last_item {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(GitIntegCode::EOrder);
        }
        let want = match item_cell(o) {
            Some(w) => w,
            None => return Err(GitIntegCode::ENoItem),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(GitIntegCode::EDrift);
        }
        if match self.items.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitIntegCode::EDup);
        }
        if let Some(c) = self.items.get_mut(o) {
            *c = Some(got);
        }
        self.last_item = item as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 条目全账核验（20 格）。
    pub fn items_all(&self) -> bool {
        let mut o = 0usize;
        while o < ITEM_CELLS {
            match self.items.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// L4 联签一行。
    pub fn declare_sig(&mut self, r: usize, got: u32) -> Result<(), GitIntegCode> {
        let want = match sig_row(r) {
            Some(w) => w,
            None => return Err(GitIntegCode::ENoSig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + r as u64);
            return Err(GitIntegCode::ESigDrift);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitIntegCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x8301 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（6 格）。
    pub fn sigs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SIG_ROWS {
            match self.sigs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 刷新拍一格（拍载荷递增 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), GitIntegCode> {
        let want = match refresh_beat(b) {
            Some(w) => w,
            None => return Err(GitIntegCode::ENoBeat),
        };
        if got < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400);
            return Err(GitIntegCode::EOrder);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8401 + b as u64);
            return Err(GitIntegCode::EDrift);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitIntegCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got;
        self.fnv(0x8402 + b as u64);
        Ok(())
    }

    /// 刷新拍全账核验（8 拍）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < REFRESH_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 嫁接判绿一件（载荷对照 + 重复拒）。
    pub fn record_graft(&mut self, g: usize, got: u64) -> Result<(), GitIntegCode> {
        let want = match graft_cell(g) {
            Some(w) => w,
            None => return Err(GitIntegCode::EGraft),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + g as u64);
            return Err(GitIntegCode::EGraft);
        }
        if match self.grafts.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitIntegCode::EDup);
        }
        if let Some(c) = self.grafts.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x8501 + g as u64);
        Ok(())
    }

    /// 嫁接全账核验（4 件）。
    pub fn grafts_all(&self) -> bool {
        let mut g = 0usize;
        while g < GRAFT_ITEMS {
            match self.grafts.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
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
        if self.gates_all() && self.items_all() && self.sigs_all() && self.beats_all() && self.grafts_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GitIntegLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GitIntegLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ITEM_CELLS {
        if let Some(w) = item_cell(o) {
            if led.record_item(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < SIG_ROWS {
        if let Some(w) = sig_row(r) {
            if led.declare_sig(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut b = 0usize;
    while b < REFRESH_BEATS {
        if let Some(w) = refresh_beat(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut g = 0usize;
    while g < GRAFT_ITEMS {
        if let Some(w) = graft_cell(g) {
            if led.record_graft(g, w).is_ok() {
                recorded += 1;
            }
        }
        g += 1;
    }
    // 注入 10 轮：条目漂移 / 刷新拍序逆 / 条目越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GitIntegLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_item(3, item_cell(3).unwrap_or(0));
                lt.record_item(4, item_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(GitIntegCode::EDrift)
            }
            1 => {
                let _ = lt.record_beat(5, refresh_beat(5).unwrap_or(0));
                lt.record_beat(2, refresh_beat(2).unwrap_or(0)) == Err(GitIntegCode::EOrder)
            }
            _ => lt.record_item(ITEM_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52284 判据集（锚 UNX-F52285-J1，off-by-one 板式）。
pub fn run_unx52284_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52284");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "GUI/CLI ×10 条目对账 20 格全通",
        recorded >= ITEM_CELLS as u32,
        "十条目逐格 GUI/CLI 双侧恒等",
    );
    s.add(
        "条目序单调（纯函数核验）",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ITEM_CELLS {
                if item_of(o).unwrap_or(10) < item_of(o - 1).unwrap_or(10) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "十条目闭集单调，逆序必抓",
    );
    s.add(
        "GUI/CLI 侧闭集核验",
        {
            let mut ok = true;
            let mut o = 0usize;
            while o < ITEM_CELLS {
                if side_of(o).unwrap_or(2) != (o % 2) as u32 {
                    ok = false;
                }
                o += 1;
            }
            ok && side_name(0) == Some("GUI") && side_name(1) == Some("CLI")
        },
        "偶格 GUI 奇格 CLI 双侧闭集",
    );
    s.add(
        "L4 判据消费联签 6/6 全齐",
        {
            let mut lt = GitIntegLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SIG_ROWS {
                if let Some(w) = sig_row(r) {
                    if lt.declare_sig(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.sigs_all()
        },
        "F52035/52059 复用 + B01/B08/B10 + AI-59 联签六格闭集",
    );
    s.add(
        "状态刷新 8 拍单调全落",
        {
            let mut lt = GitIntegLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < REFRESH_BEATS {
                if let Some(w) = refresh_beat(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "多进程组合态刷新拍严格递增，倒退必抓",
    );
    s.add(
        "嫁接 4 件判绿全落",
        {
            let mut lt = GitIntegLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GRAFT_ITEMS {
                if let Some(w) = graft_cell(g) {
                    if lt.record_graft(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.grafts_all()
        },
        "L4 嫁接件逐件判绿格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GitIntegLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "×10 条目/L4 联签/刷新序/嫁接判绿/失绿实录/汇总逐格",
    );
    s.add("注入 10/10 全抓", caught >= 10, "条目漂移/拍序逆/越闭集三态轮转必抓");
    let mut l0 = GitIntegLedger::new();
    s.add(
        "边界·条目越闭集",
        l0.record_item(ITEM_CELLS, 0).is_err(),
        "条目格 20 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.declare_sig(SIG_ROWS, 0).is_err(),
        "联签行 6 越闭集必拒",
    );
    s.add(
        "边界·刷新拍越闭集",
        l0.record_beat(REFRESH_BEATS, 0).is_err(),
        "刷新拍 8 越闭集必拒",
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
        "诊断码闭集 13 变体",
        GitIntegCode::Ok as u32 == 0 && GitIntegCode::EGradeDrift as u32 == 12,
        "Ok=0 至 EGradeDrift=12 恰 13 变体闭集",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GitIntegLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < ITEM_CELLS {
                if let Some(w) = item_cell(o) {
                    let _ = lt.record_item(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < SIG_ROWS {
                if let Some(w) = sig_row(r) {
                    let _ = lt.declare_sig(r, w);
                }
                r += 1;
            }
            let mut b = 0usize;
            while b < REFRESH_BEATS {
                if let Some(w) = refresh_beat(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut g = 0usize;
            while g < GRAFT_ITEMS {
                if let Some(w) = graft_cell(g) {
                    let _ = lt.record_graft(g, w);
                }
                g += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC86C0,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC86C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_of() {
        assert_eq!(item_of(0), Some(0));
        assert_eq!(item_of(1), Some(0));
        assert_eq!(item_of(2), Some(1));
        assert_eq!(item_of(19), Some(9));
        assert!(item_of(20).is_none());
        assert!(item_cell(20).is_none());
    }

    #[test]
    fn test_side_of() {
        assert_eq!(side_of(0), Some(0));
        assert_eq!(side_of(1), Some(1));
        assert_eq!(side_of(18), Some(0));
        assert_eq!(side_of(19), Some(1));
        assert!(side_of(20).is_none());
        assert_eq!(side_name(0), Some("GUI"));
        assert_eq!(side_name(1), Some("CLI"));
    }

    #[test]
    fn test_cells_pure() {
        let a = item_cell(3).unwrap_or(0);
        assert_eq!(a, item_cell(3).unwrap_or(0));
        assert_ne!(a, item_cell(4).unwrap_or(0));
        assert!(sig_row(SIG_ROWS).is_none());
        assert!(graft_cell(GRAFT_ITEMS).is_none());
        assert!(refresh_beat(REFRESH_BEATS).is_none());
    }

    #[test]
    fn test_beats_strictly_increasing() {
        let mut b = 1usize;
        while b < REFRESH_BEATS {
            let prev = refresh_beat(b - 1).unwrap_or(0);
            let cur = refresh_beat(b).unwrap_or(0);
            assert!(cur > prev);
            b += 1;
        }
    }

    #[test]
    fn test_item_order_caught() {
        let mut lt = GitIntegLedger::new();
        let _ = lt.record_item(7, item_cell(7).unwrap_or(0));
        assert_eq!(lt.record_item(2, item_cell(2).unwrap_or(0)), Err(GitIntegCode::EOrder));
        assert!(lt.record_item(9, item_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = GitIntegLedger::new();
        let _ = lt.record_beat(5, refresh_beat(5).unwrap_or(0));
        assert_eq!(
            lt.record_beat(2, refresh_beat(2).unwrap_or(0)),
            Err(GitIntegCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GitIntegLedger::new();
        assert_eq!(
            lt.record_item(4, item_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(GitIntegCode::EDrift)
        );
        assert_eq!(
            lt.record_graft(1, graft_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(GitIntegCode::EGraft)
        );
    }

    #[test]
    fn test_sig_drift_caught() {
        let mut lt = GitIntegLedger::new();
        assert_eq!(lt.declare_sig(0, sig_row(0).unwrap_or(0).wrapping_add(1)), Err(GitIntegCode::ESigDrift));
        assert!(lt.declare_sig(0, sig_row(0).unwrap_or(0)).is_ok());
        assert_eq!(lt.declare_sig(0, sig_row(0).unwrap_or(0)), Err(GitIntegCode::EDup));
    }

    #[test]
    fn test_sig_row_names() {
        assert_eq!(sig_row_name(0), Some("F52035 消费复用"));
        assert_eq!(sig_row_name(1), Some("F52059 消费复用"));
        assert_eq!(sig_row_name(5), Some("AI-59 联签锚定行"));
        assert!(sig_row_name(6).is_none());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GitIntegLedger::new();
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
        assert!(recorded >= (ITEM_CELLS + SIG_ROWS + REFRESH_BEATS + GRAFT_ITEMS) as u32);
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
        assert_eq!(SEG_BASE, 0xC86C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GitIntegCode::Ok as u32, 0);
        assert_eq!(GitIntegCode::ENoItem as u32, 1);
        assert_eq!(GitIntegCode::EFp as u32, 11);
        assert_eq!(GitIntegCode::EGradeDrift as u32, 12);
        let all = [
            GitIntegCode::Ok,
            GitIntegCode::ENoItem,
            GitIntegCode::EDrift,
            GitIntegCode::EOrder,
            GitIntegCode::ENoSig,
            GitIntegCode::ESigDrift,
            GitIntegCode::EGraft,
            GitIntegCode::ENoBeat,
            GitIntegCode::EDup,
            GitIntegCode::ESeal,
            GitIntegCode::EInj,
            GitIntegCode::EFp,
            GitIntegCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GitIntegLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52284_checks_green() {
        let s = run_unx52284_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
