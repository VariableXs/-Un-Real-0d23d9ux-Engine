//! unx0434_ifdisc —— UNX-F0434 IF 标志纪律账（cli/sti 配对）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0434 · IF 标志纪律账 · 目标 240 行
//! 判据原文：UNX-F0434-J1 IF 旗纪律 = cli（关中断）与 sti（开
//!           中断）严格配对栈式管理：cli 入栈保存旧 IF、sti
//!           出栈恢复旧 IF（而非无脑置 1——嵌套 cli 内层 sti
//!           不许提前开中断）；深嵌套计账（栈深上限 16，越界
//!           立案）；临界区账：cli→…→sti 为一段，段内不许再
//!           有无保护裸 sti（由「恢复」语义自动满足）；断言
//!           核：IF 位提取（RFLAGS 位 9）、保存-恢复往返恒等
//!           （save→restore 同 RFLAGS）；死区检测：栈深 > 0
//!           跨「调度点」立案（持锁调度是死锁温床）。
//! 组件：IF 栈（16 深）+ 嵌套 cli/sti 对 + RFLAGS 位 9 提取
//!       + 往返恒等核 + 临界区分段账 + 调度点死区检测。
//! 职责定位：IF 旗是「总闸」——无脑 sti 会把内层临界区提前
//!   曝光给中断（典型的竞态温床），栈式 save/restore 是唯一
//!   守得住嵌套的办法；持锁调度（栈非空时进入调度点）是内
//!   核死锁的头号来源，跨点立案让这类 bug 在演练期现形。
//! 性能逐项分解：cli/sti O(1)；位提取 O(1)；往返核 O(1)；
//!   死区检测 O(1)。
//! 诊断码段：0x30400（0x30401~0x30409 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const IF_VERSION: &str = "unx0434_ifdisc-v1";

/// 诊断段基址（0x30400 段 · 本模块独占）。
pub const IF_SEG_BASE: u32 = 0x30400;
pub const XC_STI_UNPAIRED: u32 = 0x30401; // sti 无配对 cli
pub const XC_NEST_DEEP: u32 = 0x30402; // 嵌套过深
pub const XC_SCHED_HELD: u32 = 0x30403; // 持锁调度
pub const XC_RESTORE: u32 = 0x30404; // 往返恒等破
pub const XC_IF_LOST: u32 = 0x30405; // IF 位账丢
pub const XC_MAGIC: u32 = 0x30409; // 账魔数破

/// RFLAGS IF 位。
pub const RFLAGS_IF: u64 = 1 << 9;
/// 栈深上限。
pub const NEST_CAP: usize = 16;

/// 提取 RFLAGS 位 9。
pub fn if_bit(rflags: u64) -> bool {
    rflags & RFLAGS_IF != 0
}

/// IF 栈账。
pub struct IfStack {
    /// 保存的旧 IF 值栈。
    pub saved: Vec<bool>,
    /// 当前 IF（软账）。
    pub cur_if: bool,
    /// 调度点持锁立案数。
    pub sched_violations: u32,
    /// 临界区段计数（完成配对次数）。
    pub sections: u32,
}

impl IfStack {
    pub fn new(initial_if: bool) -> Self {
        IfStack { saved: Vec::new(), cur_if: initial_if, sched_violations: 0, sections: 0 }
    }

    /// cli：保存旧 IF、关中断。
    pub fn cli(&mut self) -> Result<usize, u32> {
        if self.saved.len() >= NEST_CAP {
            return Err(XC_NEST_DEEP);
        }
        self.saved.push(self.cur_if);
        self.cur_if = false;
        Ok(self.saved.len())
    }

    /// sti：弹出旧 IF 恢复（非无脑置 1）。
    pub fn sti(&mut self) -> Result<bool, u32> {
        let old = match self.saved.pop() {
            Some(v) => v,
            None => return Err(XC_STI_UNPAIRED),
        };
        self.cur_if = old;
        self.sections += 1;
        Ok(old)
    }

    /// 栈深。
    pub fn depth(&self) -> usize {
        self.saved.len()
    }

    /// 嵌套语义核：内层 sti 恢复的是内层 cli 时的 IF（false），
    /// 即内层不许提前开中断。
    pub fn nested_no_early_open(&mut self) -> Result<bool, u32> {
        self.cli()?;          // 外层：true→关
        self.cli()?;          // 内层：false→关
        let inner = self.sti()?; // 内层恢复 false
        let outer = self.sti()?; // 外层恢复 true
        Ok(!inner && outer)
    }

    /// 往返恒等核：save（cli）→ restore（sti）后 IF 同初值。
    pub fn roundtrip(&mut self, initial: bool) -> Result<(), u32> {
        let mut probe = IfStack::new(initial);
        probe.cli()?;
        let back = probe.sti()?;
        if back != initial || probe.cur_if != initial {
            return Err(XC_RESTORE);
        }
        Ok(())
    }

    /// 调度点死区检测：栈非空进入调度点即立案。
    pub fn sched_point(&mut self) -> Result<(), u32> {
        if self.depth() > 0 {
            self.sched_violations += 1;
            return Err(XC_SCHED_HELD);
        }
        Ok(())
    }

    /// 临界区分段账：完整 cli…sti 配对计一段。
    pub fn section_ok(&self) -> bool {
        self.sections > 0 && self.saved.is_empty()
    }
}

/// RFLAGS 建模（夹具）。
pub fn mk_rflags(if_set: bool) -> u64 {
    if if_set {
        0x202 // IF=1 + 保留位 1
    } else {
        0x002
    }
}

/// 判据入口：UNX-F0434。
pub fn run_unx0434_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0434_ifdisc");
    // 族一：IF 位提取。
    cs.add("位 9 置", if_bit(mk_rflags(true)), "RFLAGS 0x202");
    cs.add("位 9 清", !if_bit(mk_rflags(false)), "RFLAGS 0x002");
    // 族二：cli/sti 配对。
    let mut s = IfStack::new(true);
    cs.add("初态开", s.cur_if, "IF=1");
    cs.add("cli 关", s.cli() == Ok(1) && !s.cur_if, "深 1");
    cs.add("sti 恢复开", s.sti() == Ok(true) && s.cur_if, "回 IF=1");
    cs.add("配对计段", s.sections == 1, "一段");
    // 族三：无配对 sti。
    let mut s2 = IfStack::new(false);
    cs.add("裸 sti 拒", s2.sti() == Err(XC_STI_UNPAIRED), "栈空");
    // 族四：嵌套语义。
    let mut s3 = IfStack::new(true);
    cs.add("嵌套不提前开", s3.nested_no_early_open() == Ok(true), "内层复 false");
    cs.add("嵌套计两段", s3.sections == 2, "内外各一");
    // 族五：深嵌套。
    let mut s4 = IfStack::new(true);
    let mut last = Ok(0usize);
    for _ in 0..20 {
        last = s4.cli();
        if last.is_err() {
            break;
        }
    }
    cs.add("16 深上限", s4.depth() == NEST_CAP, "前 16 全进");
    cs.add("17 层拒", last == Err(XC_NEST_DEEP), "越栈");
    // 逐层恢复清栈。
    let mut drained = true;
    for _ in 0..NEST_CAP {
        if s4.sti().is_err() {
            drained = false;
        }
    }
    cs.add("逐层清空", drained && s4.depth() == 0, "16 层回");
    // 族六：恢复语义非无脑。
    let mut s5 = IfStack::new(false); // 初态关
    let _ = s5.cli();
    cs.add("关中开恢复关", s5.sti() == Ok(false) && !s5.cur_if, "恢复≠置 1");
    // 族七：往返恒等。
    let mut s6 = IfStack::new(true);
    cs.add("真往返", s6.roundtrip(true).is_ok(), "true→true");
    cs.add("假往返", IfStack::new(false).roundtrip(false).is_ok(), "false→false");
    // 族八：调度点死区。
    let mut s7 = IfStack::new(true);
    cs.add("空栈调度过", s7.sched_point().is_ok(), "无锁");
    let _ = s7.cli();
    cs.add("持锁调度立案", s7.sched_point() == Err(XC_SCHED_HELD), "死区");
    cs.add("立案计数", s7.sched_violations == 1, "账面");
    // 族九：段账。
    let mut s8 = IfStack::new(true);
    let _ = s8.cli();
    let _ = s8.sti();
    cs.add("段账成", s8.section_ok(), "配对后栈空");
    let mut s9 = IfStack::new(true);
    let _ = s9.cli();
    cs.add("未配对段账败", !s9.section_ok(), "栈未空");
    // 族十：两层临界区串联。
    let mut s10 = IfStack::new(true);
    let _ = s10.cli();
    let _ = s10.sti();
    let _ = s10.cli();
    let _ = s10.sti();
    cs.add("两段串联", s10.sections == 2 && s10.section_ok(), "串行复用");
    cs
}
