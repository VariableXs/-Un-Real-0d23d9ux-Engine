//! focring — WP-202 · B-907 焦点环（MD2 篇 9.4）。
//!
//! 判据 B-907：全控件基类默认渲染，审计零例外。
//! MD2 原文（9.4）："焦点纪律的落地：焦点变更事件与输入投递同源（合成器
//! 裁决，篇 5.5），Tab 序按窗口声明的控件序，焦点环渲染由 vx-SDK 控件基
//! 类统一实现（应用想丢焦点环都难——宪章第四章的物理强制思路与浮层出路
//! 同构）。"
//!
//! 宿主可测形态：控件基类**焦点环默认开且类型面无关闭构造**（构造唯一、
//! focus_ring 恒 true——"应用想丢焦点环都难"的物理强制，B-803 同族防线）
//! + Tab 序环形遍历（按窗口声明控件序，focus_next/focus_prev 环形）+
//! 焦点唯一（合成器裁决同源——任意时刻恰一个焦点持有者）+ 审计零例外
//! （全控件枚举 focus_ring 全 true 的审计谓词）。

use crate::checks::CheckSet;

/// 窗口控件容量。
pub const CONTROL_CAP: usize = 16;

/// 控件基类（vx-SDK 控件基类的宿主模型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ControlBase {
    pub id: u16,
    /// **焦点环渲染标志——恒 true**。
    /// 类型面防线：字段私有，构造唯一（`new`），不存在任何置 false 的
    /// 路径——应用想丢焦点环都难（宪章第四章物理强制思路）。
    focus_ring: bool,
}

impl ControlBase {
    pub const fn new(id: u16) -> ControlBase {
        ControlBase { id, focus_ring: true }
    }

    /// 审计读取面（唯一出口——恒 true，类型上无法返回 false）。
    pub const fn focus_ring_on(&self) -> bool {
        self.focus_ring
    }
}

/// 窗口：控件 Tab 序（声明序）+ 焦点裁决（与输入投递同源）。
pub struct FocusWindow {
    pub controls: [Option<ControlBase>; CONTROL_CAP],
    pub n: usize,
    /// 当前焦点（合成器裁决——任意时刻至多一个）。
    pub focus: Option<usize>,
}

impl FocusWindow {
    pub const fn new() -> FocusWindow {
        FocusWindow { controls: [None; CONTROL_CAP], n: 0, focus: None }
    }

    /// 声明控件（Tab 序即声明序）。
    pub fn declare(&mut self, id: u16) -> bool {
        if self.n >= CONTROL_CAP {
            return false;
        }
        self.controls[self.n] = Some(ControlBase::new(id));
        self.n += 1;
        true
    }

    /// Tab：焦点前进（环形——尾后回首，首前回尾）。
    pub fn focus_next(&mut self) -> Option<usize> {
        if self.n == 0 {
            return None;
        }
        self.focus = match self.focus {
            Some(i) => Some((i + 1) % self.n),
            None => Some(0),
        };
        self.focus
    }

    /// Shift+Tab：焦点后退（环形）。
    pub fn focus_prev(&mut self) -> Option<usize> {
        if self.n == 0 {
            return None;
        }
        self.focus = match self.focus {
            Some(0) => Some(self.n - 1),
            Some(i) => Some(i - 1),
            None => Some(self.n - 1),
        };
        self.focus
    }

    /// 审计谓词：全控件焦点环全开（零例外的执行器）。
    pub fn audit_all_rings(&self) -> bool {
        (0..self.n).all(|i| self.controls[i].map(|c| c.focus_ring_on()).unwrap_or(false))
    }

    /// 焦点唯一性（合成器裁决同源——至多一个）。
    pub fn focus_unique(&self) -> bool {
        self.focus.map(|i| i < self.n).unwrap_or(true)
    }
}

// ---------------------------------------------------------------- 对练

/// 焦点环对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct FocDrillSummary {
    pub rounds: u32,
    pub tab_steps: u64,
    /// Tab 环形完整（一圈恰遍历全部控件一次）
    pub ring_complete: bool,
    /// 焦点唯一
    pub focus_unique: bool,
    /// 审计零例外（全控件焦点环全开）
    pub audit_zero_exception: bool,
}

/// 随机窗口 × Tab 遍历对练：环形完整性 + 唯一性 + 审计。
pub fn run_foc_drills(seed: u64, rounds: u32) -> FocDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = FocDrillSummary::default();
    sum.rounds = rounds;
    sum.ring_complete = true;
    sum.focus_unique = true;
    sum.audit_zero_exception = true;
    for _ in 0..rounds {
        let n_ctrl = 2 + (g.next() % (CONTROL_CAP as u64 - 1)) as usize; // 2..=16
        let mut w = FocusWindow::new();
        for i in 0..n_ctrl {
            if !w.declare(100 + i as u16) {
                sum.audit_zero_exception = false;
            }
        }
        if !w.audit_all_rings() || !w.focus_unique() {
            sum.audit_zero_exception = false;
        }
        // Tab 一整圈：从任意起点出发 n 步后回到起点（环形完整）
        let start = (g.next() % n_ctrl as u64) as usize;
        w.focus = Some(start);
        let mut ok = true;
        for _ in 0..n_ctrl {
            match w.focus_next() {
                Some(_) => sum.tab_steps += 1,
                None => ok = false,
            }
            if !w.focus_unique() {
                ok = false;
            }
        }
        if w.focus != Some(start) {
            ok = false; // n 步后回到起点 = 环形
        }
        // 反向一圈
        for _ in 0..n_ctrl {
            let _ = w.focus_prev();
        }
        if w.focus != Some(start) {
            ok = false;
        }
        if !ok {
            sum.ring_complete = false;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_focring_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-907 焦点环");
    {
        // 类型面防线：构造唯一且恒开
        let c = ControlBase::new(1);
        set.add(
            "B-907 焦点环类型面恒开",
            c.focus_ring_on(),
            "focus_ring 私有恒 true——应用想丢焦点环都难（物理强制思路）",
        );
    }
    {
        // Tab 序按声明序
        let mut w = FocusWindow::new();
        for i in 0..3u16 {
            let _ = w.declare(i);
        }
        w.focus = Some(0);
        let n1 = w.focus_next().unwrap();
        let n2 = w.focus_next().unwrap();
        set.add(
            "B-907 Tab 序按声明序",
            n1 == 1 && n2 == 2,
            "Tab 走窗口声明控件序（MD2 9.4）",
        );
    }
    {
        // 环形：尾后回首
        let mut w = FocusWindow::new();
        for i in 0..3u16 {
            let _ = w.declare(i);
        }
        w.focus = Some(2);
        let n = w.focus_next().unwrap();
        set.add(
            "B-907 环形回首",
            n == 0,
            "Tab 序环形（尾后回首，Shift+Tab 首前回尾）",
        );
    }
    {
        // 反向环形
        let mut w = FocusWindow::new();
        for i in 0..3u16 {
            let _ = w.declare(i);
        }
        w.focus = Some(0);
        let p = w.focus_prev().unwrap();
        set.add(
            "B-907 反向环形回尾",
            p == 2,
            "Shift+Tab 首前回尾",
        );
    }
    {
        // 焦点唯一（合成器裁决同源）
        let mut w = FocusWindow::new();
        for i in 0..4u16 {
            let _ = w.declare(i);
        }
        let a = w.focus_next();
        let b = w.focus_next();
        set.add(
            "B-907 焦点唯一",
            w.focus_unique() && a.is_some() && b.is_some() && a != b,
            "焦点变更与输入投递同源——任意时刻恰一个焦点",
        );
    }
    {
        // 审计零例外（多控件窗口）
        let mut w = FocusWindow::new();
        for i in 0..CONTROL_CAP as u16 {
            let _ = w.declare(i);
        }
        set.add(
            "B-907 审计零例外（满窗）",
            w.audit_all_rings(),
            "全控件基类默认渲染——审计谓词全 true",
        );
    }
    {
        // 空窗口焦点安全
        let mut w = FocusWindow::new();
        set.add(
            "B-907 空窗焦点安全",
            w.focus_next().is_none() && w.focus_prev().is_none() && w.audit_all_rings(),
            "零控件窗口 Tab 无 panic、审计恒过",
        );
    }
    {
        // 焦点环对练
        let sum = run_foc_drills(0xB907, 60);
        set.add(
            "B-907 焦点环对练",
            sum.rounds == 60 && sum.ring_complete && sum.focus_unique && sum.audit_zero_exception,
            "全控件基类默认渲染，审计零例外（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f807_ring_always_on() {
        // 全部构造路径恒开（唯一构造）
        for id in 0..100u16 {
            assert!(ControlBase::new(id).focus_ring_on());
        }
    }

    #[test]
    fn f807_tab_cycle() {
        let mut w = FocusWindow::new();
        for i in 0..4u16 {
            let _ = w.declare(i);
        }
        w.focus = Some(0);
        let seq: [usize; 4] = [
            w.focus_next().unwrap(),
            w.focus_next().unwrap(),
            w.focus_next().unwrap(),
            w.focus_next().unwrap(),
        ];
        assert_eq!(seq, [1, 2, 3, 0], "4 控件一圈回首");
        assert_eq!(w.focus, Some(0));
    }

    #[test]
    fn f807_shift_tab_cycle() {
        let mut w = FocusWindow::new();
        for i in 0..4u16 {
            let _ = w.declare(i);
        }
        w.focus = Some(0);
        assert_eq!(w.focus_prev(), Some(3));
        assert_eq!(w.focus_prev(), Some(2));
        assert_eq!(w.focus_prev(), Some(1));
        assert_eq!(w.focus_prev(), Some(0));
    }

    #[test]
    fn f807_drill_deterministic() {
        let a = run_foc_drills(17, 30);
        let b = run_foc_drills(17, 30);
        assert_eq!(a, b);
        assert!(a.ring_complete && a.focus_unique && a.audit_zero_exception);
        assert!(a.tab_steps > 0);
    }
}
