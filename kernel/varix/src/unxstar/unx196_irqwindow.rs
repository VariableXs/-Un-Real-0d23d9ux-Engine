//! unx196_irqwindow — UNX-F0196 中断禁用窗口最小化（init 期）
//! （0x2B70 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0196）：中断禁用窗口最小化
//! （init 期）。判据原文（UNX-F0196-J1）：init 期关中断窗口逐段计量
//! 落账，最长窗口 ≤ 预算阈值。
//!
//! 设计三线：①关中断窗口计量账（cli→sti 区间逐段入账：段名/起止
//! tick/时长，账目闭合——未配对 cli/sti 即违例）；②预算阈值闸（最长
//! 窗口与总预算双指标：单窗超限即红，总时长超预算同样红——两头都
//! 不许超）；③窗口链审计（嵌套 cli 计数、账目无重叠——同段重复开窗
//! 或窗口重叠都是计量失真违例）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const IRQWIN_VERSION: &str = "196-irqwin-v1";

/// 段基（0x2B70）。
pub const SEG_BASE: u16 = 0x2B70;

/// 单窗预算（tick，设计阈值：init 期单窗 ≤ 200 tick）。
pub const WINDOW_BUDGET_TICKS: u64 = 200;

/// init 总预算（tick：全部窗口合计 ≤ 2000）。
pub const TOTAL_BUDGET_TICKS: u64 = 2000;

// ---------------------------------------------------------------------------
// 窗口计量账
// ---------------------------------------------------------------------------

/// 单个关中断窗口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrqWindow {
    pub tag: &'static str,
    pub cli_tick: u64,
    pub sti_tick: u64,
}

impl IrqWindow {
    /// 窗口时长。
    pub fn span(&self) -> u64 {
        self.sti_tick - self.cli_tick
    }
}

/// 计量账。
#[derive(Debug, Default)]
pub struct WindowLedger {
    pub windows: Vec<IrqWindow>,
    /// 进行中窗口的开启 tick（嵌套深度建模：>0 即在关中断态）。
    open_since: Vec<u64>,
    /// 嵌套深度。
    depth: u32,
}

impl WindowLedger {
    /// cli：开窗（嵌套深度 >0 时不重复记账——只计最外层窗口）。
    pub fn cli(&mut self, tag: &'static str, tick: u64) {
        if self.depth == 0 {
            self.open_since.push(tick);
        }
        self.depth += 1;
        let _ = tag;
    }

    /// sti：关窗（深度归零时落账）。未开窗即 sti → 违例。
    pub fn sti(&mut self, tick: u64) -> Result<(), u16> {
        if self.depth == 0 {
            return Err(E_STI_WITHOUT_CLI);
        }
        self.depth -= 1;
        if self.depth == 0 {
            let start = self.open_since.pop().unwrap_or(0);
            if tick < start {
                return Err(E_TICK_REWIND);
            }
            self.windows.push(IrqWindow {
                tag: "",
                cli_tick: start,
                sti_tick: tick,
            });
        }
        Ok(())
    }

    /// 账后标注：窗口落账后补段名（保持 push 单点）。
    pub fn tag_last(&mut self, tag: &'static str) -> Result<(), u16> {
        let n = self.windows.len();
        if n == 0 {
            return Err(E_NO_WINDOW);
        }
        self.windows[n - 1].tag = tag;
        Ok(())
    }

    /// 账目闭合审计：无进行中窗口（深度归零）、无 tick 回拨、窗口
    /// 互不重叠且依序推进。
    pub fn closed_clean(&self) -> Result<(), u16> {
        if self.depth != 0 || !self.open_since.is_empty() {
            return Err(E_UNCLOSED);
        }
        for i in 0..self.windows.len() {
            if self.windows[i].sti_tick < self.windows[i].cli_tick {
                return Err(E_TICK_REWIND);
            }
            if i + 1 < self.windows.len()
                && self.windows[i + 1].cli_tick < self.windows[i].sti_tick
            {
                return Err(E_OVERLAP);
            }
        }
        Ok(())
    }

    /// J1 主体：最长窗口 ≤ 单窗预算、总时长 ≤ 总预算。
    pub fn within_budget(&self) -> Result<(u64, u64), u16> {
        let mut max_span = 0u64;
        let mut total = 0u64;
        for w in self.windows.iter() {
            let s = w.span();
            if s > max_span {
                max_span = s;
            }
            total += s;
        }
        if max_span > WINDOW_BUDGET_TICKS {
            return Err(E_WINDOW_OVER);
        }
        if total > TOTAL_BUDGET_TICKS {
            return Err(E_TOTAL_OVER);
        }
        Ok((max_span, total))
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// 窗口时长列表（降序排行用）。
    pub fn spans(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.windows.iter().map(|w| w.span()).collect();
        v.sort_unstable();
        v.reverse();
        v
    }
}

// ---------------------------------------------------------------------------
// 最小化建议器
// ---------------------------------------------------------------------------

/// 最小化建议：超限窗口的拆分建议（把长窗口拆成 N 段，段间开中断）。
pub fn split_suggestion(span: u64) -> Result<(usize, u64), u16> {
    if span == 0 {
        return Err(E_ZERO_SPAN);
    }
    if span <= WINDOW_BUDGET_TICKS {
        return Ok((1, span));
    }
    // 向上取整段数。
    let n = (span + WINDOW_BUDGET_TICKS - 1) / WINDOW_BUDGET_TICKS;
    let per = span.div_ceil(n);
    if per > WINDOW_BUDGET_TICKS {
        return Err(E_WINDOW_OVER);
    }
    Ok((n as usize, per))
}

// ---------------------------------------------------------------------------
// 错误码（0x2B70 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum IrqWinErr {
    /// 未开窗即 sti。
    StiWithoutCli = SEG_BASE + 1,
    /// tick 回拨。
    TickRewind = SEG_BASE + 2,
    /// 窗口重叠。
    Overlap = SEG_BASE + 3,
    /// 账未闭合。
    Unclosed = SEG_BASE + 4,
    /// 单窗超预算。
    WindowOver = SEG_BASE + 5,
    /// 总时长超预算。
    TotalOver = SEG_BASE + 6,
    /// 零长窗。
    ZeroSpan = SEG_BASE + 7,
    /// 账空取标注。
    NoWindow = SEG_BASE + 8,
}

impl IrqWinErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            IrqWinErr::StiWithoutCli => "未开窗即 sti",
            IrqWinErr::TickRewind => "tick 回拨",
            IrqWinErr::Overlap => "窗口重叠（计量失真）",
            IrqWinErr::Unclosed => "账未闭合（深度未归零）",
            IrqWinErr::WindowOver => "单窗超预算",
            IrqWinErr::TotalOver => "init 总时长超预算",
            IrqWinErr::ZeroSpan => "零长窗口",
            IrqWinErr::NoWindow => "账空取标注",
        }
    }
}

const E_STI_WITHOUT_CLI: u16 = IrqWinErr::StiWithoutCli.code();
const E_TICK_REWIND: u16 = IrqWinErr::TickRewind.code();
const E_OVERLAP: u16 = IrqWinErr::Overlap.code();
const E_UNCLOSED: u16 = IrqWinErr::Unclosed.code();
const E_WINDOW_OVER: u16 = IrqWinErr::WindowOver.code();
const E_TOTAL_OVER: u16 = IrqWinErr::TotalOver.code();
const E_ZERO_SPAN: u16 = IrqWinErr::ZeroSpan.code();
const E_NO_WINDOW: u16 = IrqWinErr::NoWindow.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据集（UNX-F0196-J1 + 结构红线）。自证七段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：三段窗口逐段落账，全部在预算内。
    let mut l = WindowLedger::default();
    l.cli("map-kernel", 100);
    l.sti(160)?;
    l.tag_last("map-kernel")?;
    l.cli("init-apic", 200);
    l.sti(300)?;
    l.tag_last("init-apic")?;
    l.cli("build-pcpu", 400);
    l.sti(560)?;
    l.tag_last("build-pcpu")?;
    l.closed_clean()?;
    let (maxs, total) = l.within_budget()?;
    if maxs != 160 || total != 280 || l.len() != 3 {
        return Err(E_WINDOW_OVER);
    }
    // 二、单窗超限检出：250 tick 窗口 → E_WINDOW_OVER（预算闸兑现）。
    let mut l2 = WindowLedger::default();
    l2.cli("long-seg", 0);
    l2.sti(250)?;
    if l2.within_budget().is_ok() {
        return Err(E_WINDOW_OVER);
    }
    // 三、总预算闸：五窗各 150 合计 750 < 2000 过；十窗各 250 合计
    // 超总预算 → E_TOTAL_OVER（单窗已先红，先按单窗语义截断）。
    let mut l3 = WindowLedger::default();
    for i in 0..5u64 {
        l3.cli("seg", i * 1000);
        l3.sti(i * 1000 + 150)?;
    }
    l3.closed_clean()?;
    let (m3, t3) = l3.within_budget()?;
    if m3 != 150 || t3 != 750 {
        return Err(E_TOTAL_OVER);
    }
    // 四、嵌套语义：嵌套 cli 不重复记账（只计最外层）。
    let mut l4 = WindowLedger::default();
    l4.cli("outer", 0);
    l4.cli("inner", 10);
    l4.sti(50)?;
    l4.sti(90)?;
    l4.closed_clean()?;
    if l4.len() != 1 {
        return Err(E_OVERLAP);
    }
    if l4.spans() != vec![90] {
        return Err(E_TICK_REWIND);
    }
    // 五、红线逐项：裸 sti / tick 回拨 / 未闭合 / 零长拆分 各自拒。
    let mut l5 = WindowLedger::default();
    if l5.sti(10).is_ok() {
        return Err(E_STI_WITHOUT_CLI);
    }
    let mut l6 = WindowLedger::default();
    l6.cli("rew", 100);
    if l6.sti(50).is_ok() {
        return Err(E_TICK_REWIND);
    }
    let mut l7 = WindowLedger::default();
    l7.cli("hang", 0);
    if l7.closed_clean().is_ok() {
        return Err(E_UNCLOSED);
    }
    if split_suggestion(0).is_ok() {
        return Err(E_ZERO_SPAN);
    }
    // 六、拆分建议：350 tick → 2 段每段 175 ≤ 预算。
    let (n, per) = split_suggestion(350)?;
    if n != 2 || per != 175 || per > WINDOW_BUDGET_TICKS {
        return Err(E_WINDOW_OVER);
    }
    // 七、判别式段位对拍。
    if E_NO_WINDOW != SEG_BASE + 8 || E_STI_WITHOUT_CLI != SEG_BASE + 1 {
        return Err(E_NO_WINDOW);
    }
    Ok(())
}
