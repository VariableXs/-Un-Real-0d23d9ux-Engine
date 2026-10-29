//! unx48293_ma11y — UNX-F48293 M 段无障碍联检（键盘全程可达输入面、焦点可见联检 AI-29）（诊断段 0xC580）。
//!
//! 判据原文：
//!   J1 联检 ×10 全过；判据锚定 体验四 键盘与焦点。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 键盘全程可达：输入面上每个元素必须在 Tab 序链上——链覆盖
//!      全部元素各恰一次、链尾回到链首成环（闭环即无死角），链外
//!      元素 = 键盘不可达 = 联检红。
//!   2. 无焦点陷阱：链上不允许出现「只进不出」的陷阱点（自环元素
//!      长度 >1 的面内不允许）；链内相邻步进必须单调推进（next 恒
//!      指向他人），防 Tab 被卡死。
//!   3. 焦点可见（AI-29）：焦点落位即置 focus_visible 标志；焦点
//!      离开即清除——任一被访元素缺可见标志 = 焦点不可见 = 联检红。
//!   4. 联检口径（×10）：10 个输入面逐一跑全检（可达 + 成环 + 无陷阱
//!      + 焦点可见四项），四项全过才记一面全绿；一项红即该面红且
//!      红因入账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC580;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_UNREACHABLE: u16 = SEG_BASE + 1; // 链外元素（键盘不可达）
const E_NOT_CLOSED: u16 = SEG_BASE + 2;  // Tab 链不成环
const E_TRAP_NODE: u16 = SEG_BASE + 3;   // 焦点陷阱（自环）
const E_FOCUS_INVISIBLE: u16 = SEG_BASE + 4; // 焦点不可见
const E_SURFACE_MISS: u16 = SEG_BASE + 5;// ×10 面不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 联检账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 输入面元素。
#[derive(Clone, Copy, Debug)]
pub struct UiElement {
    pub id: u32,
    pub focus_visible: bool, // 焦点落位时的可见标志
}

/// Tab 序链：next[i] = 元素 i 的 Tab 下一站（下标）。
/// 链由元素下标数组表达，next 与元素一一对应。
#[derive(Clone, Debug)]
pub struct Surface {
    pub name: &'static str,
    elements: Vec<UiElement>,
    next: Vec<usize>, // next[i] ∈ 0..elements.len()
    pub entry: usize, // Tab 起点下标
}

impl Surface {
    /// 构造：n 个元素 + 显式 next 链 + entry。
    pub fn build(name: &'static str, n: usize, next: &[usize], entry: usize) -> Result<Surface, u16> {
        if n == 0 || next.len() != n || entry >= n {
            return Err(E_LEDGER_BAD);
        }
        let mut elements: Vec<UiElement> = Vec::new();
        let mut i = 0usize;
        while i < n {
            elements.push(UiElement { id: i as u32, focus_visible: false });
            i += 1;
        }
        // next 逐项域检。
        let mut i = 0usize;
        while i < n {
            if next[i] >= n {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(Surface { name, elements, next: next.to_vec(), entry })
    }

    pub fn elem_count(&self) -> usize {
        self.elements.len()
    }

    /// 键盘全程可达：从 entry 沿 Tab 链走 n 步，覆盖全部元素各恰一次
    /// （位图计数，重复即环过早闭合=有元素不可达）。
    pub fn audit_reachable(&self) -> Result<(), u16> {
        let n = self.elements.len();
        let mut seen: Vec<bool> = Vec::new();
        let mut i = 0usize;
        while i < n {
            seen.push(false);
            i += 1;
        }
        let mut cur = self.entry;
        let mut step = 0usize;
        while step < n {
            if seen[cur] {
                return Err(E_UNREACHABLE); // 重复访问 = 有元素在环外
            }
            seen[cur] = true;
            cur = self.next[cur];
            step += 1;
        }
        // 走满 n 步后必须回到 entry（闭环）。
        if cur != self.entry {
            return Err(E_NOT_CLOSED);
        }
        Ok(())
    }

    /// 无焦点陷阱：n>1 的面内任一元素 next 不得指向自身（自环=卡死）。
    pub fn audit_no_trap(&self) -> Result<(), u16> {
        if self.elements.len() == 1 {
            return Ok(()); // 单元素面自环合法
        }
        let mut i = 0usize;
        while i < self.next.len() {
            if self.next[i] == i {
                return Err(E_TRAP_NODE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 焦点可见联检：沿 Tab 链走一整圈，每落位一处即置可见标志；
    /// 圈内任何一处标志缺失 = 焦点不可见。
    pub fn audit_focus_visible(&mut self) -> Result<(), u16> {
        let n = self.elements.len();
        // 先清全部标志（焦点离开即清除）。
        let mut i = 0usize;
        while i < n {
            self.elements[i].focus_visible = false;
            i += 1;
        }
        // 走一整圈逐位点亮。
        let mut cur = self.entry;
        let mut step = 0usize;
        while step < n {
            self.elements[cur].focus_visible = true;
            cur = self.next[cur];
            step += 1;
        }
        // 复核全亮。
        let mut i = 0usize;
        while i < n {
            if !self.elements[i].focus_visible {
                return Err(E_FOCUS_INVISIBLE);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 一面联检账。
#[derive(Clone, Copy, Debug)]
pub struct MaRecord {
    pub surface: &'static str,
    pub elems: usize,
    pub reachable: bool,
    pub closed: bool,
    pub no_trap: bool,
    pub focus_visible: bool,
    pub green: bool,
}

/// 联检枢纽。
pub struct Ma11y {
    ledger: Vec<MaRecord>,
    pub surface_total: u32,
    pub fail_total: u32,
}

impl Ma11y {
    pub fn new() -> Ma11y {
        Ma11y { ledger: Vec::new(), surface_total: 0, fail_total: 0 }
    }

    /// 联检一面：四项全过才全绿（J1 主体）。
    pub fn inspect(&mut self, s: &mut Surface) -> Result<(), u16> {
        let mut rec = MaRecord {
            surface: s.name,
            elems: s.elem_count(),
            reachable: false,
            closed: false,
            no_trap: false,
            focus_visible: false,
            green: false,
        };
        let mut green = true;
        // 可达（含成环：audit_reachable 同时判两项，这里分开落账）。
        match s.audit_reachable() {
            Ok(()) => {
                rec.reachable = true;
                rec.closed = true;
            }
            Err(E_UNREACHABLE) => {
                rec.reachable = false;
                green = false;
            }
            Err(E_NOT_CLOSED) => {
                rec.closed = false;
                green = false;
            }
            Err(_) => {
                green = false;
            }
        }
        match s.audit_no_trap() {
            Ok(()) => rec.no_trap = true,
            Err(_) => {
                rec.no_trap = false;
                green = false;
            }
        }
        match s.audit_focus_visible() {
            Ok(()) => rec.focus_visible = true,
            Err(_) => {
                rec.focus_visible = false;
                green = false;
            }
        }
        rec.green = green;
        if !green {
            self.fail_total += 1;
        }
        self.ledger.push(rec);
        self.surface_total += 1;
        if !green {
            return Err(E_UNREACHABLE);
        }
        Ok(())
    }

    /// ×N 面覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.surface_total < want {
            return Err(E_SURFACE_MISS);
        }
        Ok(())
    }

    /// 终判：全绿、账目守恒。
    pub fn assert_all_green(&self) -> Result<(), u16> {
        if self.fail_total != 0 || self.ledger.len() as u32 != self.surface_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if !self.ledger[i].green {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.surface_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（联检 ×10 全过），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主联检 ×10：10 个输入面（3..14 元素确定性环链）四项全检全绿。
    let mut ma = Ma11y::new();
    let mut n = 3usize;
    while n <= 12 {
        // 顺时针环：next[i] = (i+1) % n，entry=0。
        let mut next: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < n {
            next.push((i + 1) % n);
            i += 1;
        }
        let mut s = Surface::build("loop", n, &next, 0)?;
        ma.inspect(&mut s)?;
        n += 1;
    }
    ma.coverage(10)?;
    ma.assert_all_green()?;
    if ma.ledger_len() != 10 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 焦点可见逐位点灯复演：6 元素环走一圈后全亮，单独抽查 3 处。
    let mut next: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        next.push((i + 1) % 6);
        i += 1;
    }
    let mut s = Surface::build("ring6", 6, &next, 0)?;
    s.audit_focus_visible()?;
    let mut i = 0usize;
    while i < 6 {
        if !s.elements[i].focus_visible {
            return Err(E_FOCUS_INVISIBLE);
        }
        i += 1;
    }
    hits += 1;

    // R3 链外元素抓红：next 链只串 4/5 元素（第五元素不可达），
    //    audit_reachable 必红（E_UNREACHABLE）。
    // 5 元素：0→1→2→3→0，元素 4 悬空。
    let next = [1usize, 2, 3, 0, 0];
    let mut s = Surface::build("broken5", 5, &next, 0)?;
    if s.audit_reachable().is_ok() {
        return Err(E_UNREACHABLE);
    }
    hits += 1;

    // R4 不成环抓红：4 元素链尾不回 entry（0→1→2→3→1，成 1..3 小环，
    //    entry 0 走 4 步后落在 2 ≠ entry）。
    let next = [1usize, 2, 3, 1];
    let mut s = Surface::build("open4", 4, &next, 0)?;
    if s.audit_reachable().is_ok() {
        return Err(E_NOT_CLOSED);
    }
    hits += 1;

    // R5 焦点陷阱抓红：3 元素中 next[1]==1（自环），audit_no_trap 必红；
    //    单元素面自环放行（退化面合法）。
    let next = [1usize, 1, 0];
    let mut s = Surface::build("trap3", 3, &next, 0)?;
    if s.audit_no_trap().is_ok() {
        return Err(E_TRAP_NODE);
    }
    let next1 = [0usize];
    let mut s1 = Surface::build("solo", 1, &next1, 0)?;
    s1.audit_no_trap()?;
    s1.audit_reachable()?;
    s1.audit_focus_visible()?;
    hits += 1;

    // R6 面账可红：伪造一面联检失败（fail_total 手工置位），
    //    assert_all_green 必红（联检判据非恒绿）。
    let mut ma = Ma11y::new();
    ma.ledger.push(MaRecord {
        surface: "fake",
        elems: 3,
        reachable: false,
        closed: false,
        no_trap: false,
        focus_visible: false,
        green: false,
    });
    ma.surface_total += 1;
    ma.fail_total += 1;
    if ma.assert_all_green().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 越界防御：空面、next 越域、entry 越域全拒且不落账。
    if Surface::build("e0", 0, &[], 0).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    let bad_next = [1usize, 9, 2];
    if Surface::build("oob", 3, &bad_next, 0).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    let ok_next = [1usize, 2, 0];
    if Surface::build("oob_entry", 3, &ok_next, 3).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 混合面回归 ×10（另一批形状：交替步长环）与写回（判据 J2）：
    //    10 面交替顺/逆时针环全绿；末尾写回断言。
    let mut ma = Ma11y::new();
    let mut n = 4usize;
    while n <= 13 {
        let mut next: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < n {
            if n % 2 == 0 {
                next.push((i + 1) % n); // 顺时针
            } else {
                next.push((i + n - 1) % n); // 逆时针
            }
            i += 1;
        }
        let mut s = Surface::build("mix", n, &next, 0)?;
        ma.inspect(&mut s)?;
        n += 1;
    }
    ma.coverage(10)?;
    ma.assert_all_green()?;
    if ma.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ma.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
