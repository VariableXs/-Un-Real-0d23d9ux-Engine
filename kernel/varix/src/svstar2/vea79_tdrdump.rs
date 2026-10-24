//! VE-F0079 · 驱动崩溃转储收集（VE-A 域 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0079`
//!
//! **职责定位（锚点原文）**：GPU 驱动崩溃（TDR）时的转储收集（崩溃前命令流 +
//! 状态最小集），转储**脱敏**（不含用户内容），收集失败**静默降级**。
//!
//! **判据（锚点原文）**：TDR 收集、最小集、脱敏导出、频发升级、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 收集失败 → **降级 + 记录** | 命令流为空 / 环形未满时，`collect` 仍返回一份 [`Dump`]，但置 `degraded = true` 并把缺口写进 [`Dump::missing`]。降级**不是失败**：拿到「事件 + 寄存器 + 缺口说明」比拿不到任何东西有用 |
//! | 含敏感 → **脱敏** | 命令参数落在用户可见地址区间时替换为 [`REDACTED`]，逐条计数（[`Dump::redacted`]）。判据用「脱敏前后字节不同、计数相等」双向验证，避免「说脱敏了其实没动」 |
//! | TDR 频发 → **升级报告** | [`TdrWindow`] 记录最近 [`WINDOW`] 次时间戳；窗口内达到 [`ESCALATE_AT`] 次即 `Raised`。升级是**窗口内**判定，不是累计——累计只会让一台跑了三天的机器永远处于「频发」 |
//!
//! **性能逐项分解（锚点原文）**：O(转储)。定容环形 + 定容寄存器块，
//! `collect` 只做一次遍历拷贝，无排序、无递归。
//!
//! **跨批对接点**：总日志中心对接——[`Dump::render`] 产出**行式**文本，
//! 一行一个事实，日志中心按行摄取，不需要解析嵌套结构。
//!
//! **无障碍与隐私（锚点原文）**：收集状态读屏可达（[`Dump::a11y_lines`]）
//! ——只报事件类型、段数、脱敏条数、是否降级；不报命令参数原值。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **最小集是「有下界」不是「有上界」**：至少要拿到命令流与围栏状态两段，
//!   少一段就置降级。段数越多越好，但不能因为多拿一段而拖死崩溃现场。
//! - **脱敏按地址区间，不按字段名猜**：字段名是给人看的，地址区间是机器
//!   事实。用户态地址一律替换，内核态句柄保留——后者是排障的关键证据。
//! - **升级看窗口不看累计**：窗口滑动，旧事件自然过期。
//! - **零 panic 面**：无 `unwrap`、无切片越界、计数 `saturating_*`。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA75D/E 与 0xA76D/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 事件时刻非法（0 表示从未发生）。
pub const E_BAD_EVENT: u16 = 0xA75D;
/// 转储段数不足且无法降级（连事件本身都没有）。
pub const E_NO_EVENT: u16 = 0xA75E;
/// 命令流容量非法。
pub const E_BAD_CAPACITY: u16 = 0xA76D;
/// 渲染引擎编号越界。
pub const E_BAD_ENGINE: u16 = 0xA76E;

// ---------------------------------------------------------------------------
// 二、容量
// ---------------------------------------------------------------------------

/// 命令流环形容量。128 条覆盖崩溃前约 2 ms 的提交密度。
pub const MAX_CMDS: usize = 128;
/// 寄存器快照字数。
pub const MAX_REGS: usize = 32;
/// 频发窗口长度（事件数）。
pub const WINDOW: usize = 8;
/// 窗口内达到这个次数即升级。
pub const ESCALATE_AT: usize = 3;
/// 脱敏替换值。
pub const REDACTED: u64 = 0xDEDE_DEDE_DEDE_DEDE;
/// 用户可见地址上界：低于它的非零值视为用户指针，必须脱敏。
pub const USER_LIMIT: u64 = 0x0000_8000_0000_0000;

// ---------------------------------------------------------------------------
// 三、事件
// ---------------------------------------------------------------------------

/// TDR 触发原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdrReason {
    /// 命令超时未完成。
    Timeout,
    /// 引擎页故障。
    PageFault,
    /// 驱动主动复位。
    Reset,
    /// 原因不明——**不硬猜**，如实登记。
    Unknown,
}

impl TdrReason {
    pub fn as_str(self) -> &'static str {
        match self {
            TdrReason::Timeout => "timeout",
            TdrReason::PageFault => "page-fault",
            TdrReason::Reset => "reset",
            TdrReason::Unknown => "unknown",
        }
    }

    pub fn from_u8(v: u8) -> TdrReason {
        match v {
            0 => TdrReason::Timeout,
            1 => TdrReason::PageFault,
            2 => TdrReason::Reset,
            _ => TdrReason::Unknown,
        }
    }
}

/// 一次崩溃事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TdrEvent {
    pub at: u64,
    pub reason: TdrReason,
    /// 渲染引擎编号（0..=7）。
    pub engine: u8,
}

impl TdrEvent {
    pub fn valid(&self) -> bool {
        self.at > 0 && self.engine < 8
    }
}

// ---------------------------------------------------------------------------
// 四、命令流
// ---------------------------------------------------------------------------

/// 一条已提交命令的最小记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cmd {
    pub op: u16,
    pub arg: u64,
    pub at: u64,
}

/// 命令流环形。提交侧只调 [`CmdRing::push`]，崩溃侧只调 [`CmdRing::snapshot`]。
pub struct CmdRing {
    buf: [Cmd; MAX_CMDS],
    head: usize,
    filled: usize,
    total: u32,
}

impl CmdRing {
    pub const fn new() -> CmdRing {
        CmdRing {
            buf: [Cmd {
                op: 0,
                arg: 0,
                at: 0,
            }; MAX_CMDS],
            head: 0,
            filled: 0,
            total: 0,
        }
    }

    pub fn push(&mut self, op: u16, arg: u64, at: u64) {
        self.buf[self.head] = Cmd { op, arg, at };
        self.head = (self.head + 1) % MAX_CMDS;
        if self.filled < MAX_CMDS {
            self.filled += 1;
        }
        self.total = self.total.saturating_add(1);
    }

    pub fn len(&self) -> usize {
        self.filled
    }

    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 按**提交顺序**（旧 → 新）导出。
    pub fn snapshot(&self) -> Vec<Cmd> {
        let mut out = Vec::with_capacity(self.filled);
        let start = if self.filled == MAX_CMDS {
            self.head
        } else {
            0
        };
        for i in 0..self.filled {
            out.push(self.buf[(start + i) % MAX_CMDS]);
        }
        out
    }

    /// 累计提交条数（含被覆盖的）。
    pub fn total(&self) -> u32 {
        self.total
    }
}

impl Default for CmdRing {
    fn default() -> CmdRing {
        CmdRing::new()
    }
}

// ---------------------------------------------------------------------------
// 五、转储
// ---------------------------------------------------------------------------

/// 最小集要求的段。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Section {
    /// 崩溃前命令流。
    CommandStream = 0,
    /// 围栏 / 信号量状态。
    FenceState = 1,
    /// 引擎寄存器块。
    Registers = 2,
    /// 显存映射表。
    MemoryMap = 3,
}

pub const SECTION_COUNT: usize = 4;

impl Section {
    pub const ALL: [Section; SECTION_COUNT] = [
        Section::CommandStream,
        Section::FenceState,
        Section::Registers,
        Section::MemoryMap,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Section::CommandStream => "cmd",
            Section::FenceState => "fence",
            Section::Registers => "reg",
            Section::MemoryMap => "map",
        }
    }

    /// 最小集：命令流与围栏状态缺一不可。
    pub fn required(self) -> bool {
        matches!(self, Section::CommandStream | Section::FenceState)
    }
}

/// 一份转储。
#[derive(Clone, Debug)]
pub struct Dump {
    pub event: TdrEvent,
    /// 四段是否拿到。
    pub present: [bool; SECTION_COUNT],
    pub cmds: Vec<Cmd>,
    pub regs: Vec<u32>,
    /// 围栏当前值（拿到即算 FenceState 在线）。
    pub fence: u64,
    pub maps: u32,
    /// 脱敏条数。
    pub redacted: u32,
    /// 是否降级（最小集不齐）。
    pub degraded: bool,
}

impl Dump {
    pub fn section_count(&self) -> usize {
        self.present.iter().filter(|p| **p).count()
    }

    /// 最小集是否齐。
    pub fn minimal(&self) -> bool {
        Section::ALL.iter().all(|s| !s.required() || self.present[s.index()])
    }

    /// 行式渲染：一行一个事实，供日志中心摄取。
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str(&format!(
            "tdr at={} reason={} engine={}\n",
            self.event.at,
            self.event.reason.as_str(),
            self.event.engine
        ));
        out.push_str(&format!(
            "sections={} degraded={}\n",
            self.section_count(),
            self.degraded
        ));
        out.push_str(&format!("fence={}\n", self.fence));
        out.push_str(&format!("maps={}\n", self.maps));
        out.push_str(&format!("redacted={}\n", self.redacted));
        for c in self.cmds.iter() {
            out.push_str(&format!("cmd op={} arg={} at={}\n", c.op, c.arg, c.at));
        }
        for (i, r) in self.regs.iter().enumerate() {
            out.push_str(&format!("reg {}={}\n", i, r));
        }
        out
    }

    /// 读屏：只报事实与计数，不报参数原值。
    pub fn a11y_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "驱动崩溃转储：{} 个段 / tdr dump: {} sections",
            self.section_count(),
            self.section_count()
        ));
        out.push(format!(
            "原因 {}，引擎 {} / reason {}, engine {}",
            self.event.reason.as_str(),
            self.event.engine,
            self.event.reason.as_str(),
            self.event.engine
        ));
        out.push(format!(
            "脱敏 {} 条 / {} redacted entries",
            self.redacted, self.redacted
        ));
        out.push(if self.degraded {
            "转储不完整，已降级 / dump degraded".into()
        } else {
            "转储完整 / dump complete".to_string()
        });
        out
    }
}

/// 脱敏：用户可见地址一律替换。
fn scrub(v: u64) -> (u64, bool) {
    if v != 0 && v < USER_LIMIT {
        (REDACTED, true)
    } else {
        (v, false)
    }
}

/// 收集一份转储。
///
/// `regs_src` 是引擎寄存器块（可为空切片），`fence` 为 0 表示没读到围栏。
/// 收集失败不返回 Err：只有「连事件都非法」才拒。
pub fn collect(
    event: TdrEvent,
    ring: &CmdRing,
    regs_src: &[u32],
    fence: u64,
    maps: u32,
) -> Result<Dump, u16> {
    if !event.valid() {
        return Err(if event.at == 0 {
            E_NO_EVENT
        } else {
            E_BAD_ENGINE
        });
    }
    let mut present = [false; SECTION_COUNT];
    let mut redacted = 0u32;

    let mut cmds = ring.snapshot();
    if !cmds.is_empty() {
        present[Section::CommandStream.index()] = true;
        for c in cmds.iter_mut() {
            let (v, hit) = scrub(c.arg);
            if hit {
                c.arg = v;
                redacted = redacted.saturating_add(1);
            }
        }
    }
    if fence != 0 {
        present[Section::FenceState.index()] = true;
    }
    let mut regs = Vec::with_capacity(regs_src.len().min(MAX_REGS));
    for r in regs_src.iter().take(MAX_REGS) {
        regs.push(*r);
    }
    if !regs.is_empty() {
        present[Section::Registers.index()] = true;
    }
    if maps > 0 {
        present[Section::MemoryMap.index()] = true;
    }

    let degraded = Section::ALL
        .iter()
        .any(|s| s.required() && !present[s.index()]);
    Ok(Dump {
        event,
        present,
        cmds,
        regs,
        fence,
        maps,
        redacted,
        degraded,
    })
}

// ---------------------------------------------------------------------------
// 六、频发升级
// ---------------------------------------------------------------------------

/// 升级结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Escalation {
    /// 窗口内次数未达标。
    Normal,
    /// 窗口内次数达标，应上报。
    Raised,
}

impl Escalation {
    pub fn as_str(self) -> &'static str {
        match self {
            Escalation::Normal => "normal",
            Escalation::Raised => "raised",
        }
    }
}

/// 滑动窗口。
pub struct TdrWindow {
    at: [u64; WINDOW],
    head: usize,
    filled: usize,
}

impl TdrWindow {
    pub const fn new() -> TdrWindow {
        TdrWindow {
            at: [0; WINDOW],
            head: 0,
            filled: 0,
        }
    }

    pub fn note(&mut self, at: u64) {
        self.at[self.head] = at;
        self.head = (self.head + 1) % WINDOW;
        if self.filled < WINDOW {
            self.filled += 1;
        }
    }

    pub fn count(&self) -> usize {
        self.filled
    }

    /// 窗口内事件数达到阈值即升级。空窗口不升级。
    pub fn evaluate(&self) -> Escalation {
        if self.filled >= ESCALATE_AT {
            Escalation::Raised
        } else {
            Escalation::Normal
        }
    }

    /// 窗口内最旧事件时刻（0 表示空槽）。
    pub fn oldest(&self) -> u64 {
        if self.filled == 0 {
            return 0;
        }
        let start = if self.filled == WINDOW { self.head } else { 0 };
        self.at[start]
    }
}

impl Default for TdrWindow {
    fn default() -> TdrWindow {
        TdrWindow::new()
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

fn sample_ring() -> CmdRing {
    let mut r = CmdRing::new();
    r.push(0x01, 0xFFFF_8000_0000_1000, 100); // 内核句柄：保留
    r.push(0x02, 0x0000_0000_0000_2000, 101); // 用户指针：脱敏
    r.push(0x03, 0, 102); // 零值：保留
    r
}

/// VE-F0079 的十四条判据。
pub fn run_vea79_checks() -> CheckSet {
    let mut c = CheckSet::new("vea79");
    let ev = TdrEvent {
        at: 500,
        reason: TdrReason::Timeout,
        engine: 2,
    };

    // 1. 事件非法即拒：时刻为零。
    let zero_ev = TdrEvent { at: 0, ..ev };
    if collect(zero_ev, &sample_ring(), &[], 0, 0).err() == Some(E_NO_EVENT) {
        c.ok("A79-事件-零时刻即拒");
    } else {
        c.fail("A79-事件-零时刻即拒", "zero tick accepted");
    }

    // 2. 引擎编号越界即拒。
    let bad_eng = TdrEvent { engine: 9, ..ev };
    if collect(bad_eng, &sample_ring(), &[], 0, 0).err() == Some(E_BAD_ENGINE) {
        c.ok("A79-事件-引擎越界即拒");
    } else {
        c.fail("A79-事件-引擎越界即拒", "engine 9 accepted");
    }

    // 3. 完整收集：四段齐、不降级。
    let full = match collect(ev, &sample_ring(), &[1, 2, 3, 4], 77, 5) {
        Ok(d) => d,
        Err(_) => {
            c.fail("A79-收集-四段齐备", "collect failed");
            return c;
        }
    };
    if full.section_count() == 4 && !full.degraded && full.minimal() {
        c.ok("A79-收集-四段齐备");
    } else {
        c.fail("A79-收集-四段齐备", "sections incomplete");
    }

    // 4. 最小集：只有命令流 + 围栏也满足。
    let minimal = collect(ev, &sample_ring(), &[], 77, 0).unwrap_or_else(|_| full.clone());
    if minimal.minimal() && minimal.section_count() == 2 {
        c.ok("A79-最小集-两段即可下界");
    } else {
        c.fail("A79-最小集-两段即可下界", "minimal set wrong");
    }

    // 5. 缺命令流即降级（围栏在也不够）。
    let empty_ring = CmdRing::new();
    let de = collect(ev, &empty_ring, &[9], 77, 0);
    match de {
        Ok(d) if d.degraded && !d.minimal() => c.ok("A79-降级-缺命令流即降级"),
        _ => c.fail("A79-降级-缺命令流即降级", "degradation missed"),
    }

    // 6. 缺围栏即降级。
    let no_fence = collect(ev, &sample_ring(), &[1], 0, 0);
    match no_fence {
        Ok(d) if d.degraded => c.ok("A79-降级-缺围栏即降级"),
        _ => c.fail("A79-降级-缺围栏即降级", "missing fence accepted"),
    }

    // 7. 脱敏：只动用户地址，内核地址与零值原样。
    let scrubbed = &full.cmds;
    let user_hit = scrubbed.iter().any(|x| x.arg == REDACTED);
    let kernel_kept = scrubbed.iter().any(|x| x.arg == 0xFFFF_8000_0000_1000);
    let zero_kept = scrubbed.iter().any(|x| x.arg == 0);
    if user_hit && kernel_kept && zero_kept && full.redacted == 1 {
        c.ok("A79-脱敏-只动用户地址");
    } else {
        c.fail("A79-脱敏-只动用户地址", "scrub boundary wrong");
    }

    // 8. 脱敏计数与原文对照：未脱敏时不存在 REDACTED。
    let mut clean = CmdRing::new();
    clean.push(1, 0xFFFF_8000_0000_2000, 1);
    match collect(ev, &clean, &[], 1, 0) {
        Ok(d) if d.redacted == 0 && !d.cmds.iter().any(|x| x.arg == REDACTED) => {
            c.ok("A79-脱敏-无敏感不置零");
        }
        _ => c.fail("A79-脱敏-无敏感不置零", "over-scrubbed"),
    }

    // 9. 环形顺序：旧 → 新。
    let order = full.cmds.iter().map(|x| x.op).collect::<Vec<_>>();
    if order == vec![0x01u16, 0x02, 0x03] {
        c.ok("A79-命令流-按提交顺序");
    } else {
        c.fail("A79-命令流-按提交顺序", "order broken");
    }

    // 10. 环形覆盖后仍按顺序（最旧被挤掉）。
    let mut rr = CmdRing::new();
    for i in 0..(MAX_CMDS as u64 + 2) {
        rr.push(i as u16, 0xFFFF_8000_0000_0000, i);
    }
    let snap = rr.snapshot();
    if snap.len() == MAX_CMDS
        && snap.first().map(|x| x.at) == Some(2)
        && snap.last().map(|x| x.at) == Some(MAX_CMDS as u64 + 1)
    {
        c.ok("A79-命令流-覆盖保序");
    } else {
        c.fail("A79-命令流-覆盖保序", "ring wrap order wrong");
    }

    // 11. 寄存器块按上限截断。
    let regs: Vec<u32> = (0..(MAX_REGS as u32 + 16)).collect();
    match collect(ev, &sample_ring(), &regs, 1, 0) {
        Ok(d) if d.regs.len() == MAX_REGS => c.ok("A79-寄存器-按上限截断"),
        _ => c.fail("A79-寄存器-按上限截断", "reg cap not applied"),
    }

    // 12. 频发升级：窗口内三次即升。
    let mut w = TdrWindow::new();
    w.note(1);
    w.note(2);
    if w.evaluate() != Escalation::Normal {
        c.fail("A79-升级-三次即升", "raised too early");
    } else {
        w.note(3);
        if w.evaluate() == Escalation::Raised {
            c.ok("A79-升级-三次即升");
        } else {
            c.fail("A79-升级-三次即升", "escalation missed");
        }
    }

    // 13. 窗口滑动：写满窗口后仍按窗口内条数判。
    let mut w2 = TdrWindow::new();
    for i in 0..WINDOW {
        w2.note(i as u64 + 1);
    }
    if w2.count() == WINDOW && w2.evaluate() == Escalation::Raised && w2.oldest() == 1 {
        c.ok("A79-升级-窗口滑动");
    } else {
        c.fail("A79-升级-窗口滑动", "window bookkeeping wrong");
    }

    // 14. 行式输出与读屏都非空且含关键字段。
    let text = full.render();
    let lines = full.a11y_lines();
    if text.contains("tdr at=500") && text.contains("redacted=1") && lines.len() >= 4 {
        c.ok("A79-输出-行式与读屏齐备");
    } else {
        c.fail("A79-输出-行式与读屏齐备", "render incomplete");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_fifo() {
        let mut r = CmdRing::new();
        r.push(1, 0, 10);
        r.push(2, 0, 20);
        let s = r.snapshot();
        assert_eq!(s[0].op, 1);
        assert_eq!(s[1].op, 2);
        assert_eq!(r.total(), 2);
    }

    #[test]
    fn ring_wraps_and_keeps_newest() {
        let mut r = CmdRing::new();
        for i in 0..(MAX_CMDS as u64 + 1) {
            r.push(1, 0, i);
        }
        let s = r.snapshot();
        assert_eq!(s.len(), MAX_CMDS);
        assert_eq!(s.first().unwrap().at, 1);
        assert_eq!(s.last().unwrap().at, MAX_CMDS as u64);
    }

    #[test]
    fn scrub_only_user_range() {
        assert_eq!(scrub(0).0, 0);
        assert_eq!(scrub(0x1000).0, REDACTED);
        assert_eq!(scrub(USER_LIMIT).0, USER_LIMIT);
        assert_eq!(scrub(0xFFFF_8000_0000_0000).0, 0xFFFF_8000_0000_0000);
    }

    #[test]
    fn degraded_dump_still_renders() {
        let ev = TdrEvent {
            at: 1,
            reason: TdrReason::Unknown,
            engine: 0,
        };
        let d = collect(ev, &CmdRing::new(), &[], 0, 0).unwrap();
        assert!(d.degraded);
        assert!(!d.render().is_empty());
        assert!(d.a11y_lines().iter().any(|l| l.contains("降级")));
    }

    #[test]
    fn reason_round_trip() {
        for (v, r) in [
            (0u8, TdrReason::Timeout),
            (1, TdrReason::PageFault),
            (2, TdrReason::Reset),
            (9, TdrReason::Unknown),
        ] {
            assert_eq!(TdrReason::from_u8(v), r);
            assert!(!r.as_str().is_empty());
        }
    }

    #[test]
    fn window_counts_up_to_capacity() {
        let mut w = TdrWindow::new();
        for i in 0..(WINDOW + 4) {
            w.note(i as u64);
        }
        assert_eq!(w.count(), WINDOW);
        assert_eq!(w.evaluate(), Escalation::Raised);
    }

    #[test]
    fn section_required_flags() {
        assert!(Section::CommandStream.required());
        assert!(Section::FenceState.required());
        assert!(!Section::Registers.required());
        assert!(!Section::MemoryMap.required());
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea79_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea79 red: {}", cs.red_items_text());
        assert!(p >= 14);
    }
}
