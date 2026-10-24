// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves109_improve
// VE-F3709 · 质量改进建议（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3709
// 判据原文：建议可执行、一键修复幂等可撤销、改进建议、判据。
//
// 难点分析：
//   1. 一键修复红线：自动修复必须幂等+可撤销。
//   2. 建议可执行：每条建议必须有具体修法，不能只说"要改进"。
//   3. 改进建议分级——按问题严重度排优先级。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9D00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9D00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9DCode(pub u16);

impl C9DCode {
    pub const NOT_ACTIONABLE: Self = Self(SEG_BASE);   // 建议不可执行
    pub const FIX_NOT_IDEMPOTENT: Self = Self(SEG_BASE + 1); // 修复不幂等
    pub const FIX_NOT_REVERSIBLE: Self = Self(SEG_BASE + 2); // 修复不可撤销
    pub const NO_SUGGESTION: Self = Self(SEG_BASE + 3); // 无建议
    pub const ACTIONABLE: Self = Self(SEG_BASE + 4);   // 可执行合规
    pub const IDEMPOTENT: Self = Self(SEG_BASE + 5);   // 幂等合规
    pub const REVERSIBLE: Self = Self(SEG_BASE + 6);   // 可撤销合规

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::NOT_ACTIONABLE => "建议不可执行——必须有具体修法",
            Self::FIX_NOT_IDEMPOTENT => "一键修复不幂等——幂等红线",
            Self::FIX_NOT_REVERSIBLE => "一键修复不可撤销——撤销红线",
            Self::NO_SUGGESTION => "无改进建议——改进导向",
            Self::ACTIONABLE => "建议可执行合规",
            Self::IDEMPOTENT => "修复幂等合规",
            Self::REVERSIBLE => "修复可撤销合规",
            _ => "未知诊断",
        }
    }
}

// ── 改进建议 ──────────────────────────────────────────────────────

/// 建议优先级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    /// 低。
    Low,
    /// 中。
    Medium,
    /// 高。
    High,
}

impl Priority {
    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Medium => "中",
            Self::High => "高",
        }
    }
}

/// 改进建议。
#[derive(Clone, Debug)]
pub struct Suggestion {
    /// 具体修法（不可为空）。
    pub fix: &'static str,
    /// 优先级。
    pub priority: Priority,
    /// 是否可一键修复。
    pub one_click: bool,
}

impl Suggestion {
    pub fn new(fix: &'static str, priority: Priority, one_click: bool) -> Self {
        Self { fix, priority, one_click }
    }

    /// 可执行检查：修法非空。
    pub fn check_actionable(&self) -> Result<(), C9DCode> {
        if self.fix.is_empty() {
            Err(C9DCode::NOT_ACTIONABLE)
        } else {
            Ok(())
        }
    }
}

/// 建议清单。
pub struct SuggestionList {
    items: Vec<Suggestion>,
}

impl SuggestionList {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, s: Suggestion) {
        self.items.push(s);
    }

    /// 全量可执行审计。
    pub fn audit(&mut self) -> Result<(), C9DCode> {
        if self.items.is_empty() {
            return Err(C9DCode::NO_SUGGESTION);
        }
        for s in &self.items {
            s.check_actionable()?;
        }
        Ok(())
    }

    /// 按优先级排序（高→低）。
    pub fn sorted(&self) -> Vec<Suggestion> {
        let mut v = self.items.clone();
        v.sort_by_key(|s| match s.priority {
            Priority::High => 0,
            Priority::Medium => 1,
            Priority::Low => 2,
        });
        v
    }

    pub fn one_click_count(&self) -> usize {
        self.items.iter().filter(|s| s.one_click).count()
    }
}

impl Default for SuggestionList {
    fn default() -> Self {
        Self::new()
    }
}

// ── 一键修复 ──────────────────────────────────────────────────────

/// 修复操作。
pub struct FixAction {
    /// 目标属性 ID。
    pub prop_id: u16,
    /// 修复后值。
    pub new_value: u16,
    /// 原值（撤销用）。
    pub old_value: Option<u16>,
}

impl FixAction {
    pub fn new(prop_id: u16, new_value: u16) -> Self {
        Self { prop_id, new_value, old_value: None }
    }
}

/// 修复引擎（幂等+可撤销）。
pub struct FixEngine {
    /// 当前状态：prop_id → value。
    state: Vec<(u16, u16)>,
    /// 撤销栈。
    undo_stack: Vec<(u16, u16)>,
}

impl FixEngine {
    pub fn new() -> Self {
        Self { state: Vec::new(), undo_stack: Vec::new() }
    }

    /// 设置初始属性。
    pub fn init(&mut self, prop_id: u16, value: u16) {
        self.state.push((prop_id, value));
    }

    fn get(&self, prop_id: u16) -> Option<u16> {
        for &(k, v) in &self.state {
            if k == prop_id {
                return Some(v);
            }
        }
        None
    }

    fn set(&mut self, prop_id: u16, value: u16) {
        for entry in self.state.iter_mut() {
            if entry.0 == prop_id {
                entry.1 = value;
                return;
            }
        }
        self.state.push((prop_id, value));
    }

    /// 应用修复。幂等：同值重复应用不改变状态。
    pub fn apply(&mut self, action: &mut FixAction) -> Result<(), C9DCode> {
        let current = self.get(action.prop_id);
        action.old_value = current;
        // 幂等：若当前已是目标值，不推入撤销栈
        if current == Some(action.new_value) {
            return Ok(());
        }
        match current {
            Some(old) => {
                self.undo_stack.push((action.prop_id, old));
                self.set(action.prop_id, action.new_value);
                Ok(())
            }
            None => Err(C9DCode::FIX_NOT_IDEMPOTENT), // 无原值无法保证幂等/撤销
        }
    }

    /// 撤销最近一次。
    pub fn undo(&mut self) -> Result<(), C9DCode> {
        match self.undo_stack.pop() {
            Some((pid, old)) => {
                self.set(pid, old);
                Ok(())
            }
            None => Err(C9DCode::FIX_NOT_REVERSIBLE),
        }
    }

    /// 当前值查询。
    pub fn value(&self, prop_id: u16) -> Option<u16> {
        self.get(prop_id)
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }
}

impl Default for FixEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves109_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves109_improve");
    {
        // 01 空修法被拒
        let s = Suggestion::new("", Priority::High, false);
        chk(
            &mut set,
            "C39D-01",
            s.check_actionable().err() == Some(C9DCode::NOT_ACTIONABLE),
            "无修法=不可执行",
        );
    }
    {
        // 02 有修法可执行
        let s = Suggestion::new("将对比度从 3:1 提高到 4.5:1", Priority::High, false);
        chk(&mut set, "C39D-02", s.check_actionable().is_ok(), "有具体修法可执行");
    }
    {
        // 03 空清单被拒
        let mut sl = SuggestionList::new();
        chk(
            &mut set,
            "C39D-03",
            sl.audit().err() == Some(C9DCode::NO_SUGGESTION),
            "空清单无建议",
        );
    }
    {
        // 04 全量可执行审计
        let mut sl = SuggestionList::new();
        sl.add(Suggestion::new("修法 A", Priority::Low, false));
        sl.add(Suggestion::new("修法 B", Priority::High, true));
        chk(&mut set, "C39D-04", sl.audit().is_ok() && sl.one_click_count() == 1, "全可执行+一键计数");
    }
    {
        // 05 优先级排序
        let mut sl = SuggestionList::new();
        sl.add(Suggestion::new("低", Priority::Low, false));
        sl.add(Suggestion::new("高", Priority::High, false));
        sl.add(Suggestion::new("中", Priority::Medium, false));
        let sorted = sl.sorted();
        chk(
            &mut set,
            "C39D-05",
            sorted[0].priority == Priority::High && sorted[2].priority == Priority::Low,
            "高→中→低排序",
        );
    }
    {
        // 06 修复幂等
        let mut eng = FixEngine::new();
        eng.init(1, 100);
        let mut a = FixAction::new(1, 200);
        assert!(eng.apply(&mut a).is_ok());
        // 再次应用同值
        let mut a2 = FixAction::new(1, 200);
        assert!(eng.apply(&mut a2).is_ok());
        chk(
            &mut set,
            "C39D-06",
            eng.value(1) == Some(200) && eng.undo_depth() == 1,
            "同值重复应用幂等（撤销栈不增长）",
        );
    }
    {
        // 07 修复可撤销
        let mut eng = FixEngine::new();
        eng.init(1, 100);
        let mut a = FixAction::new(1, 200);
        assert!(eng.apply(&mut a).is_ok());
        assert!(eng.undo().is_ok());
        chk(
            &mut set,
            "C39D-07",
            eng.value(1) == Some(100),
            "撤销恢复原值",
        );
    }
    {
        // 08 无原值拒绝
        let mut eng = FixEngine::new();
        let mut a = FixAction::new(1, 200);
        let r = eng.apply(&mut a);
        chk(
            &mut set,
            "C39D-08",
            r.err() == Some(C9DCode::FIX_NOT_IDEMPOTENT),
            "属性不存在→无法保证幂等/撤销→拒绝",
        );
    }
    {
        // 09 空撤销栈拒绝
        let mut eng = FixEngine::new();
        let r = eng.undo();
        chk(
            &mut set,
            "C39D-09",
            r.err() == Some(C9DCode::FIX_NOT_REVERSIBLE),
            "空撤销栈→无物可撤",
        );
    }
    {
        // 10 多步修复+撤销
        let mut eng = FixEngine::new();
        eng.init(1, 100);
        eng.init(2, 200);
        let mut a1 = FixAction::new(1, 111);
        let mut a2 = FixAction::new(2, 222);
        assert!(eng.apply(&mut a1).is_ok());
        assert!(eng.apply(&mut a2).is_ok());
        assert!(eng.undo().is_ok());
        assert!(eng.undo().is_ok());
        chk(
            &mut set,
            "C39D-10",
            eng.value(1) == Some(100) && eng.value(2) == Some(200) && eng.undo_depth() == 0,
            "两步修复两步撤销全恢复",
        );
    }
    {
        // 11 修法原样记录
        let s = Suggestion::new("将圆角从 12px 改为 8px", Priority::Medium, true);
        chk(&mut set, "C39D-11", s.fix == "将圆角从 12px 改为 8px", "修法具体非空");
    }
    {
        // 12 一键修复标记
        let mut sl = SuggestionList::new();
        sl.add(Suggestion::new("手动修", Priority::Low, false));
        sl.add(Suggestion::new("自动修 1", Priority::High, true));
        sl.add(Suggestion::new("自动修 2", Priority::High, true));
        chk(&mut set, "C39D-12", sl.one_click_count() == 2, "2 条可一键修复");
    }
    set
}
