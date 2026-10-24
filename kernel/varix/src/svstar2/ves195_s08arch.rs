#![allow(dead_code)]
//! VE-F3941 · S08 开工与测试工具总纲
//!
//! S08 定位：无障碍测试工具组——S 域全部无障碍能力的测试工具统一 + DX。
//! 工具家族复用声明：F3342 统一框架 + S08 测试工具 = 工具生态闭环（闭环声明）。
//! DX 五标准复用执法（快/稳/可 diff/可集成/可扩展——五标准复述 S 版）。
//! 范围：十六项映射（调度/审查/仿真/语料/回归/看板/参考/索引/地图/扩展/
//! 接入/矩阵/总闸/走查/资产/演进）。
//! 错误路径：工具不达五标准→缺陷立案（复述红线实测）；闭环断（工具与家族
//! 脱节）→对齐（复述）；范围缺→补（复述）。
//! 性能：映射 O(1)；审计 O(标准)；对齐 O(协议)；补 O(缺项)。
//! 跨批对接：F3342 框架单源；S01-S07 调试器归集；F3952 扩展前向。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0980）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0980;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8ACode(pub u16);

impl S8ACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const DX_FAIL_CASE: Self = Self(SEG_BASE + 1);
    pub const LOOP_BREAK: Self = Self(SEG_BASE + 2);
    pub const SCOPE_MISSING: Self = Self(SEG_BASE + 3);
    pub const FAMILY_UNBOUND: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::DX_FAIL_CASE => "工具不达五标准→缺陷立案",
            Self::LOOP_BREAK => "闭环断→对齐",
            Self::SCOPE_MISSING => "范围缺→补",
            Self::FAMILY_UNBOUND => "家族脱节→对齐复述",
            _ => "未知",
        }
    }
}

// ── 闭环声明（F3342 框架 + S08 工具 = 生态闭环）──────────────────────

/// 工具家族成员：每个 S08 工具必须声明其复用的家族框架单源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilySource {
    /// F3342 统一测试框架（单源）。
    F3342Framework,
    /// S01-S07 调试器归集（家族对端）。
    S01S07Debugger,
    /// S08 自有工具（新登记，须挂框架）。
    S08Owned,
}

/// 闭环声明记录：工具 × 家族单源 × 绑定状态。
#[derive(Debug, Clone)]
pub struct LoopDecl {
    /// 工具名（十六项之一）。
    pub tool: &'static str,
    /// 复用的家族单源。
    pub source: FamilySource,
    /// 绑定是否成立（脱节 = 闭环断）。
    pub bound: bool,
}

impl LoopDecl {
    pub fn new(tool: &'static str, source: FamilySource, bound: bool) -> Self {
        Self { tool, source, bound }
    }
}

/// 工具生态闭环账：所有十六项都绑定家族单源才算闭环。
#[derive(Debug, Clone)]
pub struct LoopLedger {
    pub decls: Vec<LoopDecl>,
}

impl LoopLedger {
    pub fn new() -> Self {
        Self { decls: Vec::new() }
    }

    pub fn declare(&mut self, d: LoopDecl) {
        self.decls.push(d);
    }

    /// 闭环断检出：任一声明未绑定即断，返回断点工具名。
    pub fn find_break(&self) -> Option<&'static str> {
        self.decls.iter().find(|d| !d.bound).map(|d| d.tool)
    }

    /// 对齐动作：闭环断→对齐（把脱节工具重新挂到家族单源）。
    pub fn align(&mut self, tool: &'static str) -> S8ACode {
        match self.decls.iter_mut().find(|d| d.tool == tool) {
            Some(d) if !d.bound => {
                d.bound = true;
                S8ACode::OK
            }
            Some(_) => S8ACode::OK,
            None => S8ACode::FAMILY_UNBOUND,
        }
    }

    pub fn closed(&self) -> bool {
        self.find_break().is_none() && self.decls.len() == 16
    }
}

impl Default for LoopLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── DX 五标准复用执法 ────────────────────────────────────────────────

/// DX 五标准（复述 S 版）：快/稳/可 diff/可集成/可扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxStd {
    /// 快：单工具冷启动预算内。
    Fast,
    /// 稳：注入下不误报不崩溃。
    Stable,
    /// 可 diff：输出可逐字节对比。
    Diffable,
    /// 可集成：可挂 CI 与全域调度。
    Integratable,
    /// 可扩展：五点三段插槽可挂插件。
    Extendable,
}

pub const DX_STANDARDS: [DxStd; 5] = [
    DxStd::Fast,
    DxStd::Stable,
    DxStd::Diffable,
    DxStd::Integratable,
    DxStd::Extendable,
];

/// 单工具 DX 审计：五标准逐项达标记录。
#[derive(Debug, Clone)]
pub struct DxAudit {
    pub tool: &'static str,
    /// 五项逐一对齐 DX_STANDARDS 下标。
    pub pass: [bool; 5],
}

impl DxAudit {
    pub fn new(tool: &'static str, pass: [bool; 5]) -> Self {
        Self { tool, pass }
    }

    /// 执法：任一标准不达 → 缺陷立案（返回不达标标准）。
    pub fn enforce(&self) -> Option<DxStd> {
        for (i, std) in DX_STANDARDS.iter().enumerate() {
            if !self.pass[i] {
                return Some(*std);
            }
        }
        None
    }

    /// 达标率（五分之几）。
    pub fn ratio(&self) -> u8 {
        self.pass.iter().filter(|p| **p).count() as u8
    }
}

// ── 十六项映射 ───────────────────────────────────────────────────────

/// S08 十六项范围（F3960 十六族同源）。
pub const SIXTEEN_SCOPE: [&str; 16] = [
    "调度", "审查", "仿真", "语料", "回归", "看板", "参考", "索引",
    "地图", "扩展", "接入", "矩阵", "总闸", "走查", "资产", "演进",
];

/// 十六项映射表：范围项 → 对应卡号（映射 O(1)）。
#[derive(Debug, Clone)]
pub struct ScopeMap {
    /// 每项挂一张卡（下标对齐 SIXTEEN_SCOPE）。
    pub anchors: [u32; 16],
}

impl ScopeMap {
    /// 标准映射：F3942-F3959 逐项挂卡（F3941 总纲自身 + F3960 收口）。
    pub fn standard() -> Self {
        // 调度 F3942、审查 F3943、仿真 F3944、语料 F3945、回归 F3946、
        // 看板 F3948、参考 F3949、索引 F3950、地图 F3951、扩展 F3952、
        // 接入 F3953、矩阵 F3954、总闸 F3955、走查 F3956、资产 F3957、
        // 演进 F3958。
        Self {
            anchors: [
                3942, 3943, 3944, 3945, 3946, 3948, 3949, 3950,
                3951, 3952, 3953, 3954, 3955, 3956, 3957, 3958,
            ],
        }
    }

    /// O(1) 取映射。
    pub fn anchor_of(&self, name: &str) -> Option<u32> {
        SIXTEEN_SCOPE
            .iter()
            .position(|s| *s == name)
            .map(|i| self.anchors[i])
    }

    /// 范围缺→补：返回未挂卡（0）的下标集合。
    pub fn missing(&self) -> Vec<usize> {
        (0..16).filter(|i| self.anchors[*i] == 0).collect()
    }

    /// 补齐动作。
    pub fn fill(&mut self, idx: usize, card: u32) -> S8ACode {
        if idx >= 16 || card == 0 {
            return S8ACode::SCOPE_MISSING;
        }
        self.anchors[idx] = card;
        S8ACode::OK
    }

    pub fn complete(&self) -> bool {
        self.missing().is_empty()
    }
}

// ── 家族复用对账（S01-S07 调试器归集）────────────────────────────────

/// 家族对端登记：S01-S07 各组调试器挂到本总纲归集。
#[derive(Debug, Clone)]
pub struct FamilyHub {
    /// 已归集的组号（1-7）。
    pub joined: [bool; 7],
}

impl FamilyHub {
    pub fn new() -> Self {
        Self { joined: [false; 7] }
    }

    pub fn join(&mut self, group: usize) -> S8ACode {
        if group < 1 || group > 7 {
            return S8ACode::FAMILY_UNBOUND;
        }
        self.joined[group - 1] = true;
        S8ACode::OK
    }

    pub fn all_joined(&self) -> bool {
        self.joined.iter().all(|j| *j)
    }

    /// 对齐协议成本：O(协议) = 未归集组数。
    pub fn align_cost(&self) -> usize {
        self.joined.iter().filter(|j| !**j).count()
    }
}

impl Default for FamilyHub {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves195_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves195_s08arch");

    // 01 十六项映射完备
    {
        let m = ScopeMap::standard();
        chk(&mut set, "C3941-01", m.complete() && m.missing().is_empty(), "err");
    }
    // 02 映射 O(1) 取值正确
    {
        let m = ScopeMap::standard();
        chk(&mut set, "C3941-02", m.anchor_of("调度") == Some(3942) && m.anchor_of("演进") == Some(3958), "err");
    }
    // 03 范围缺→补闭环
    {
        let mut m = ScopeMap::standard();
        m.anchors[3] = 0;
        let miss = m.missing();
        chk(&mut set, "C3941-03", miss == vec![3] && m.fill(3, 3945) == S8ACode::OK && m.complete(), "err");
    }
    // 04 闭环生态：全绑定即闭环
    {
        let mut led = LoopLedger::new();
        for name in SIXTEEN_SCOPE.iter() {
            led.declare(LoopDecl::new(name, FamilySource::F3342Framework, true));
        }
        chk(&mut set, "C3941-04", led.closed(), "err");
    }
    // 05 闭环断→对齐复述实测
    {
        let mut led = LoopLedger::new();
        for name in SIXTEEN_SCOPE.iter() {
            let bound = *name != "仿真";
            led.declare(LoopDecl::new(name, FamilySource::F3342Framework, bound));
        }
        let brk = led.find_break();
        let aligned = led.align("仿真");
        chk(&mut set, "C3941-05", brk == Some("仿真") && aligned == S8ACode::OK && led.closed(), "err");
    }
    // 06 家族脱节错误码语义
    {
        let mut led = LoopLedger::new();
        chk(&mut set, "C3941-06", led.align("不存在") == S8ACode::FAMILY_UNBOUND, "err");
    }
    // 07 DX 五标准执法全过
    {
        let a = DxAudit::new("调度", [true, true, true, true, true]);
        chk(&mut set, "C3941-07", a.enforce().is_none() && a.ratio() == 5, "err");
    }
    // 08 DX 不达标→立案实测（复述红线）
    {
        let a = DxAudit::new("仿真", [true, true, false, true, true]);
        chk(&mut set, "C3941-08", a.enforce() == Some(DxStd::Diffable) && a.ratio() == 4, "err");
    }
    // 09 五标准清单守恒
    {
        chk(&mut set, "C3941-09", DX_STANDARDS.len() == 5 && DX_STANDARDS[0] == DxStd::Fast && DX_STANDARDS[4] == DxStd::Extendable, "err");
    }
    // 10 家族归集七组全挂
    {
        let mut hub = FamilyHub::new();
        for g in 1..=7 {
            hub.join(g);
        }
        chk(&mut set, "C3941-10", hub.all_joined() && hub.align_cost() == 0, "err");
    }
    // 11 家族对端越界拒收
    {
        let mut hub = FamilyHub::new();
        chk(&mut set, "C3941-11", hub.join(0) == S8ACode::FAMILY_UNBOUND && hub.join(8) == S8ACode::FAMILY_UNBOUND && !hub.all_joined(), "err");
    }
    // 12 对齐成本 O(协议)
    {
        let mut hub = FamilyHub::new();
        hub.join(2);
        hub.join(5);
        chk(&mut set, "C3941-12", hub.align_cost() == 5, "err");
    }
    // 13 诊断码段自守（无越窗）
    {
        chk(&mut set, "C3941-13", S8ACode::FAMILY_UNBOUND.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释非空
    {
        let all = [S8ACode::OK, S8ACode::DX_FAIL_CASE, S8ACode::LOOP_BREAK, S8ACode::SCOPE_MISSING, S8ACode::FAMILY_UNBOUND];
        chk(&mut set, "C3941-14", all.iter().all(|c| !c.explain().is_empty() && c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_arch_all_green() {
        let s = run_ves195_checks();
        assert!(s.all_passed(), "ves195 checks red: {}", s.red_items_text());
        assert_eq!(s.len(), 14);
    }

    #[test]
    fn test_scope_map_o1() {
        let m = ScopeMap::standard();
        assert_eq!(m.anchor_of("总闸"), Some(3955));
        assert_eq!(m.anchor_of("不存在项"), None);
    }

    #[test]
    fn test_loop_break_align_cycle() {
        let mut led = LoopLedger::new();
        led.declare(LoopDecl::new("走查", FamilySource::S01S07Debugger, false));
        assert_eq!(led.find_break(), Some("走查"));
        led.align("走查");
        assert!(led.decls[0].bound);
    }

    #[test]
    fn test_dx_enforce_redline() {
        let a = DxAudit::new("看板", [false, false, false, false, false]);
        assert_eq!(a.enforce(), Some(DxStd::Fast));
        assert_eq!(a.ratio(), 0);
    }
}
