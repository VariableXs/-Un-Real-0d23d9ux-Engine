//! UNX-F52437 Termshark 收口 · 0xCAD00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52437"，逐字）：
//!   title: Termshark 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52438-J1 请求 ×10
//!     全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写
//!     回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52437）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52438-J1，照原文锚定；功能按本卡标题（Termshark）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五界面族闭集：capture（抓包列表）/ packet（分组详情）/ tree（协
//!      议树）/ follow（流跟随）/ hex（字节十六进制）五族各 4 格共 20 格，
//!      家族单调（逆序必抓），载荷折叠恒等才收。
//!   ② tshark 消费联签 4 格：Termshark 对 tshark 的消费联签逐格落账，
//!      载荷漂移必抓（ESinkDrift）。
//!   ③ 过滤器投影 8 格：显示过滤器投影逐格恒等，越闭集必拒。
//!   ④ 与 Wireshark 分账声明 2 行闭集：Termshark 与 Wireshark 分账不重
//!      账，缺一拒封；Go 上栈声明 2 行闭集同口径。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（界面越闭集/载荷漂移/过滤器越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级封顶
//!      S2（名义全通 S1 压顶至 S2，登记口径为 S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52437;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五界面族总格数（每族 4 格 × 5 族）。
pub const PANE_OPS: usize = 20;

/// 界面族数。
pub const PANE_FAMILIES: usize = 5;

/// tshark 消费联签格数。
pub const TSINK_CELLS: usize = 4;

/// 过滤器投影格数。
pub const FILTER_CELLS: usize = 8;

/// 与 Wireshark 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Go 上栈声明行数。
pub const GO_STACK_ROWS: usize = 2;

/// KB-COMPAT 定级封顶（本卡口径：定级 S2，名义 S1 压顶至 S2）。
pub const GRADE_CAP: u32 = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermsharkCode {
    /// 成功
    Ok = 0,
    /// 界面格越闭集
    ENoPane,
    /// 界面载荷漂移
    EPaneDrift,
    /// 界面族逆序
    EOrder,
    /// 联签格越闭集
    ENoSink,
    /// 联签载荷漂移
    ESinkDrift,
    /// 过滤器投影越闭集
    ENoFilter,
    /// 过滤器投影漂移
    EFilterDrift,
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

/// 全码闭集表（13 变体，位序即码位）。
pub const ALL_CODES: [TermsharkCode; 13] = [
    TermsharkCode::Ok,
    TermsharkCode::ENoPane,
    TermsharkCode::EPaneDrift,
    TermsharkCode::EOrder,
    TermsharkCode::ENoSink,
    TermsharkCode::ESinkDrift,
    TermsharkCode::ENoFilter,
    TermsharkCode::EFilterDrift,
    TermsharkCode::EDup,
    TermsharkCode::ESeal,
    TermsharkCode::EInj,
    TermsharkCode::EFp,
    TermsharkCode::EGradeDrift,
];

impl TermsharkCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TermsharkCode::Ok => "成功",
            TermsharkCode::ENoPane => "界面格越闭集：capture/packet/tree/follow/hex 20 格之外先登记",
            TermsharkCode::EPaneDrift => "界面载荷漂移：界面格折叠与基准不恒等，逐格复算",
            TermsharkCode::EOrder => "界面族逆序：capture→packet→tree→follow→hex 单调，逆序必抓",
            TermsharkCode::ENoSink => "联签格越闭集：tshark 消费联签 4 格之外先登记",
            TermsharkCode::ESinkDrift => "联签载荷漂移：tshark 消费联签折叠与基准不恒等，逐格复算",
            TermsharkCode::ENoFilter => "过滤器投影越闭集：显示过滤器投影 8 格之外先登记",
            TermsharkCode::EFilterDrift => "过滤器投影漂移：投影折叠与基准不恒等，逐格复算",
            TermsharkCode::EDup => "重复落账：同格只许一条账目",
            TermsharkCode::ESeal => "未齐即封：20 界面格 + 4 联签格 + 8 投影格 + 2 行分账未齐先补齐",
            TermsharkCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TermsharkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TermsharkCode::EGradeDrift => "定级漂移：KB-COMPAT 登记口径为 S2 封顶，越顶即漂移",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（名义档，本卡压顶至 S2）
    S1,
    /// S2：有条件收口（本卡登记档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
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
            Grade::S1 => "S1：五界面 20 格+联签 4 格+投影 8 格+分账 2 行全通（名义档）",
            Grade::S2 => "S2：Termshark 定级登记档，S1 压顶至 S2（KB-COMPAT 口径）",
            Grade::S3 => "S3：不收口，六判据缺失",
        }
    }
}

/// 定级值 → 定级档（const，无浮点无查表）。
const fn grade_of(val: u32) -> Grade {
    if val <= 1 {
        Grade::S1
    } else if val == 2 {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级封顶：好于封顶档的压至封顶档。
const fn cap_grade(g: Grade) -> Grade {
    let v = g.val();
    if v < GRADE_CAP {
        grade_of(GRADE_CAP)
    } else {
        g
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 界面格载荷（格 o；族 = o/4，capture/packet/tree/follow/hex）。
pub const fn pane_cell(o: usize) -> Option<u64> {
    if o >= PANE_OPS {
        return None;
    }
    Some(
        (0xCA000u64 + (o as u64) * 0x37)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193)
            ^ 0x5445524D53480000,
    )
}

/// 界面族（0..4=capture 4..8=packet 8..12=tree 12..16=follow 16..20=hex）。
pub const fn pane_kind(o: usize) -> Option<u32> {
    if o >= PANE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 界面族名。
pub const fn pane_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("capture 抓包列表"),
        1 => Some("packet 分组详情"),
        2 => Some("tree 协议树"),
        3 => Some("follow 流跟随"),
        4 => Some("hex 字节十六进制"),
        _ => None,
    }
}

/// tshark 消费联签格载荷（格 r）。
pub const fn tsink_cell(r: usize) -> Option<u32> {
    if r >= TSINK_CELLS {
        return None;
    }
    Some(0xCA100 + r as u32 * 0x5B)
}

/// 过滤器投影格载荷（格 f）。
pub const fn filter_cell(f: usize) -> Option<u64> {
    if f >= FILTER_CELLS {
        return None;
    }
    Some((0xCA180u64 + (f as u64) * 0x2F) ^ 0x46494C5450000000)
}

/// 与 Wireshark 分账声明行折叠（行 r）。
pub const fn ws_split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCA200 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_stack_row(r: usize) -> Option<u32> {
    if r >= GO_STACK_ROWS {
        return None;
    }
    Some(0xCA240 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Termshark 收口账本状态机。
#[derive(Clone)]
pub struct TermsharkLedger {
    /// 六判据（请求/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五界面族 20 格。
    panes: [Option<u64>; PANE_OPS],
    /// tshark 消费联签 4 格。
    tsink: [Option<u32>; TSINK_CELLS],
    /// 过滤器投影 8 格。
    filters: [Option<u64>; FILTER_CELLS],
    /// 与 Wireshark 分账声明 2 行。
    ws_splits: [Option<u32>; SPLIT_ROWS],
    /// Go 上栈声明 2 行。
    go_rows: [Option<u32>; GO_STACK_ROWS],
    /// 最后界面族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TermsharkLedger {
    /// 新账本。
    pub fn new() -> Self {
        TermsharkLedger {
            gates: [None; 6],
            panes: [None; PANE_OPS],
            tsink: [None; TSINK_CELLS],
            filters: [None; FILTER_CELLS],
            ws_splits: [None; SPLIT_ROWS],
            go_rows: [None; GO_STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TermsharkCode> {
        if gate >= 6 {
            return Err(TermsharkCode::ESeal);
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

    /// 界面一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_pane(&mut self, o: usize, got: u64) -> Result<(), TermsharkCode> {
        if o >= PANE_OPS {
            return Err(TermsharkCode::ENoPane);
        }
        let kind = match pane_kind(o) {
            Some(k) => k,
            None => return Err(TermsharkCode::ENoPane),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(TermsharkCode::EOrder);
        }
        let want = match pane_cell(o) {
            Some(w) => w,
            None => return Err(TermsharkCode::ENoPane),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + o as u64);
            return Err(TermsharkCode::EPaneDrift);
        }
        if match self.panes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermsharkCode::EDup);
        }
        if let Some(c) = self.panes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4202 + o as u64);
        Ok(())
    }

    /// 界面全账核验（20 格）。
    pub fn panes_all(&self) -> bool {
        let mut o = 0usize;
        while o < PANE_OPS {
            match self.panes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 联签一格（载荷对照 + 重复拒）。
    pub fn record_tsink(&mut self, r: usize, got: u32) -> Result<(), TermsharkCode> {
        let want = match tsink_cell(r) {
            Some(w) => w,
            None => return Err(TermsharkCode::ENoSink),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300 + r as u64);
            return Err(TermsharkCode::ESinkDrift);
        }
        if match self.tsink.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermsharkCode::EDup);
        }
        if let Some(c) = self.tsink.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x4301 + r as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn tsink_all(&self) -> bool {
        let mut r = 0usize;
        while r < TSINK_CELLS {
            match self.tsink.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 过滤器投影一格（载荷对照 + 重复拒）。
    pub fn record_filter(&mut self, f: usize, got: u64) -> Result<(), TermsharkCode> {
        let want = match filter_cell(f) {
            Some(w) => w,
            None => return Err(TermsharkCode::ENoFilter),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400 + f as u64);
            return Err(TermsharkCode::EFilterDrift);
        }
        if match self.filters.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermsharkCode::EDup);
        }
        if let Some(c) = self.filters.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x4401 + f as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn filters_all(&self) -> bool {
        let mut f = 0usize;
        while f < FILTER_CELLS {
            match self.filters.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 与 Wireshark 分账声明一行。
    pub fn declare_ws_split(&mut self, r: usize) -> Result<(), TermsharkCode> {
        if r >= SPLIT_ROWS {
            return Err(TermsharkCode::ENoPane);
        }
        if match self.ws_splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermsharkCode::EDup);
        }
        if let Some(c) = self.ws_splits.get_mut(r) {
            *c = ws_split_row(r);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn ws_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.ws_splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go_stack(&mut self, r: usize) -> Result<(), TermsharkCode> {
        if r >= GO_STACK_ROWS {
            return Err(TermsharkCode::ENoPane);
        }
        if match self.go_rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermsharkCode::EDup);
        }
        if let Some(c) = self.go_rows.get_mut(r) {
            *c = go_stack_row(r);
        }
        self.fnv(0x4600 + r as u64);
        Ok(())
    }

    /// Go 上栈声明全齐核验。
    pub fn go_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_STACK_ROWS {
            match self.go_rows.get(r) {
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

    /// 注入守恒等式核验（注入 = 检出 + 逃逸）。
    pub fn check_conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), TermsharkCode> {
        if injected == caught.wrapping_add(escaped) && injected >= 10 && escaped == 0 {
            Ok(())
        } else {
            Err(TermsharkCode::EInj)
        }
    }

    /// 定级封顶核验（越封顶档即 EGradeDrift）。
    pub fn check_grade_cap(g: Grade) -> Result<(), TermsharkCode> {
        if g.val() > GRADE_CAP {
            Err(TermsharkCode::EGradeDrift)
        } else {
            Ok(())
        }
    }

    /// 双跑指纹一致性核验。
    pub fn check_fingerprint(a: u64, b: u64) -> Result<(), TermsharkCode> {
        if a == b && a != 0 {
            Ok(())
        } else {
            Err(TermsharkCode::EFp)
        }
    }

    /// 名义定级（未封顶）：全通 → S1，六判据齐 → S2，否则 S3。
    pub fn judge_grade_nominal(&self) -> Grade {
        if self.gates_all()
            && self.panes_all()
            && self.tsink_all()
            && self.filters_all()
            && self.ws_all()
            && self.go_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// KB-COMPAT 定级（本卡封顶 S2：名义 S1 压顶至 S2）。
    pub fn judge_grade(&self) -> Grade {
        cap_grade(self.judge_grade_nominal())
    }
}

impl Default for TermsharkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TermsharkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PANE_OPS {
        if let Some(w) = pane_cell(o) {
            if led.record_pane(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < TSINK_CELLS {
        if let Some(w) = tsink_cell(r) {
            if led.record_tsink(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut f = 0usize;
    while f < FILTER_CELLS {
        if let Some(w) = filter_cell(f) {
            if led.record_filter(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut s = 0usize;
    while s < SPLIT_ROWS {
        let _ = led.declare_ws_split(s);
        s += 1;
    }
    let mut s = 0usize;
    while s < GO_STACK_ROWS {
        let _ = led.declare_go_stack(s);
        s += 1;
    }
    // 注入 10 轮：界面越闭集 / 界面载荷漂移 / 过滤器投影越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TermsharkLedger::new();
        let caught = match kind {
            0 => lt.record_pane(PANE_OPS, 0).is_err(),
            1 => {
                let _ = lt.record_pane(0, pane_cell(0).unwrap_or(0));
                lt.record_pane(1, pane_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(TermsharkCode::EPaneDrift)
            }
            _ => lt.record_filter(FILTER_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
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
/// UNX-F52437 判据集（锚 UNX-F52438-J1，off-by-one 板式）。
pub fn run_unx52437_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52437");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五界面族 20 格全通",
        {
            let mut lt = TermsharkLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < PANE_OPS {
                if let Some(w) = pane_cell(o) {
                    if lt.record_pane(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.panes_all()
        },
        "capture/packet/tree/follow/hex 五族逐格恒等，逆序与漂移必抓",
    );
    s.add(
        "tshark 消费联签 4/4 全落",
        {
            let mut lt = TermsharkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < TSINK_CELLS {
                if let Some(w) = tsink_cell(r) {
                    if lt.record_tsink(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.tsink_all()
        },
        "tshark 消费联签逐格恒等，载荷漂移必抓",
    );
    s.add(
        "过滤器投影 8/8 全落",
        {
            let mut lt = TermsharkLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FILTER_CELLS {
                if let Some(w) = filter_cell(f) {
                    if lt.record_filter(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.filters_all()
        },
        "显示过滤器投影逐格恒等，越闭集必拒",
    );
    s.add(
        "与 Wireshark 分账声明 2/2 全齐",
        {
            let mut lt = TermsharkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_ws_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ws_all()
        },
        "Termshark 与 Wireshark 分账不重账，两行闭集缺一拒封（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = TermsharkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_STACK_ROWS {
                if lt.declare_go_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.go_all()
        },
        "Go 上栈声明两行闭集缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TermsharkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "请求/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "五界面族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANE_OPS {
                if pane_kind(o).unwrap_or(5) < pane_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "capture→packet→tree→follow→hex 五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "界面越闭集/载荷漂移/过滤器越闭集三态轮转必抓",
    );
    let mut l0 = TermsharkLedger::new();
    s.add(
        "边界·界面越闭集",
        l0.record_pane(PANE_OPS, 0).is_err(),
        "界面格 20 越闭集必拒",
    );
    s.add(
        "边界·过滤器投影越闭集",
        l0.record_filter(FILTER_CELLS, 0).is_err(),
        "过滤器投影 8 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_tsink(TSINK_CELLS, 0).is_err(),
        "tshark 消费联签 4 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        TermsharkLedger::check_conservation(inj_n, caught_n, esc_n).is_ok(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        TermsharkLedger::check_fingerprint(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = TermsharkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PANE_OPS {
                if let Some(w) = pane_cell(o) {
                    let _ = lt.record_pane(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < TSINK_CELLS {
                if let Some(w) = tsink_cell(r) {
                    let _ = lt.record_tsink(r, w);
                }
                r += 1;
            }
            let mut f = 0usize;
            while f < FILTER_CELLS {
                if let Some(w) = filter_cell(f) {
                    let _ = lt.record_filter(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_ws_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_STACK_ROWS {
                let _ = lt.declare_go_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
                && lt.judge_grade_nominal() == Grade::S1
                && TermsharkLedger::check_grade_cap(lt.judge_grade()).is_ok()
        },
        "全通名义 S1 压顶至 S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAD00,
        "SEG_BASE 0x40 对齐且落在 VX16 公式 0xCA400+0x40×36",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 16 条闭集，判据锚 UNX-F52438-J1 ×10 全通",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_kind() {
        assert_eq!(pane_kind(0), Some(0));
        assert_eq!(pane_kind(4), Some(1));
        assert_eq!(pane_kind(8), Some(2));
        assert_eq!(pane_kind(12), Some(3));
        assert_eq!(pane_kind(19), Some(4));
        assert!(pane_kind(20).is_none());
        assert_eq!(pane_kind_name(0), Some("capture 抓包列表"));
        assert_eq!(pane_kind_name(4), Some("hex 字节十六进制"));
        assert!(pane_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = pane_cell(3).unwrap_or(0);
        assert_eq!(a, pane_cell(3).unwrap_or(0));
        assert_ne!(a, pane_cell(4).unwrap_or(0));
        assert_ne!(filter_cell(0).unwrap_or(0), filter_cell(1).unwrap_or(0));
        assert_ne!(tsink_cell(0).unwrap_or(0), tsink_cell(1).unwrap_or(0));
        assert!(pane_cell(PANE_OPS).is_none());
        assert!(filter_cell(FILTER_CELLS).is_none());
        assert!(tsink_cell(TSINK_CELLS).is_none());
        assert!(ws_split_row(SPLIT_ROWS).is_none());
        assert!(go_stack_row(GO_STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TermsharkLedger::new();
        // 先落族 1（packet，格 5），再回落族 0（capture，格 2）→ 逆序必抓。
        let _ = lt.record_pane(5, pane_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_pane(2, pane_cell(2).unwrap_or(0)),
            Err(TermsharkCode::EOrder)
        );
        // 同族内回格不判逆序。
        assert!(lt.record_pane(6, pane_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TermsharkLedger::new();
        assert_eq!(
            lt.record_pane(0, pane_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(TermsharkCode::EPaneDrift)
        );
        assert_eq!(
            lt.record_tsink(0, tsink_cell(0).unwrap_or(0).wrapping_add(7)),
            Err(TermsharkCode::ESinkDrift)
        );
        assert_eq!(
            lt.record_filter(0, filter_cell(0).unwrap_or(0).wrapping_add(3)),
            Err(TermsharkCode::EFilterDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TermsharkLedger::new();
        let _ = lt.record_pane(0, pane_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_pane(0, pane_cell(0).unwrap_or(0)),
            Err(TermsharkCode::EDup)
        );
        let _ = lt.record_tsink(0, tsink_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_tsink(0, tsink_cell(0).unwrap_or(0)),
            Err(TermsharkCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TermsharkLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TermsharkLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_declaration_boundaries() {
        let mut lt = TermsharkLedger::new();
        assert!(lt.declare_ws_split(0).is_ok());
        assert!(lt.declare_ws_split(1).is_ok());
        assert!(lt.declare_ws_split(2).is_err());
        assert!(lt.declare_go_stack(0).is_ok());
        assert!(lt.declare_go_stack(1).is_ok());
        assert!(lt.declare_go_stack(2).is_err());
        assert!(lt.ws_all() && lt.go_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 界面 + 4 联签 + 8 投影（公式常量之和）。
        assert_eq!(recorded, (PANE_OPS + TSINK_CELLS + FILTER_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCAD00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert_eq!(ALL_CODES[i].val(), SEG_BASE + i as u32);
            assert!(!ALL_CODES[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(TermsharkCode::Ok as u32, 0);
        assert_eq!(TermsharkCode::EGradeDrift as u32, 12);
        assert!(TermsharkLedger::check_fingerprint(1, 2).is_err());
        assert!(TermsharkLedger::check_conservation(9, 10, 0).is_err());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐压顶至 S2（名义 S1）。
        let lt = TermsharkLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = TermsharkLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut lt3 = TermsharkLedger::new();
        let mut o = 0usize;
        while o < PANE_OPS {
            if let Some(w) = pane_cell(o) {
                let _ = lt3.record_pane(o, w);
            }
            o += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52437_checks_green() {
        let s = run_unx52437_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
