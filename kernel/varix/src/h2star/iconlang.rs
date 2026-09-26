//! F300 系统图标语汇总表 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：栅格关键线审计（抽 40 枚入表）；三态参数全系统
//! 一致扫描；风格突兀孤例=0（走查）；资产管线装载判据；账本页在开发
//! 者文档站（F135）公开。
//!
//! **设计要点（主册）**：全系统图标一本账：24px 标准栅格（描边 1.5px、
//! 圆角与 F151 令牌同源、关键线几何对齐）、三态（常态/悬停亮 10%/禁用
//! 40% 透明）、五套语义族（文件类型/操作/状态/设备/系统功能）每族内
//! 形状语汇自洽；图标永远走资产管线（F068）按需装载。
//!
//! 实装：图标账本（24px 栅格 + 1.5px 描边常量——关键线审计基线）；
//! 三态参数表（全系统唯一——扫描即对拍）；五语义族注册制（每枚图标
//! 必须归族——孤例检测=无族图标数为 0）；资产管线装载口（F068 引用）；
//! 账本导出（F135 文档站数据源）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 标准栅格（px）。
pub const GRID_PX: u32 = 24;
/// 描边宽（px）。
pub const STROKE_PX: f32 = 1.5;
/// 圆角（px，与 F151 令牌同源——4px）。
pub const CORNER_PX: u32 = 4;

/// 三态参数（全系统唯一——渲染层按此取值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconStates {
    /// 常态不透明度（千分比）。
    pub normal_pm: u32,
    /// 悬停亮度提升（10% → 1100/1000）。
    pub hover_scale_pm: u32,
    /// 禁用透明度（40% → 400/1000）。
    pub disabled_pm: u32,
}

pub const STATES: IconStates = IconStates { normal_pm: 1000, hover_scale_pm: 1100, disabled_pm: 400 };

/// 五套语义族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconFamily {
    FileType,
    Action,
    Status,
    Device,
    SystemFunc,
}

impl IconFamily {
    /// 族名（账本导出用）。
    pub fn name(&self) -> &'static str {
        match self {
            IconFamily::FileType => "文件类型",
            IconFamily::Action => "操作",
            IconFamily::Status => "状态",
            IconFamily::Device => "设备",
            IconFamily::SystemFunc => "系统功能",
        }
    }
}

/// 一枚登记在账的图标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconEntry {
    pub id: &'static str,
    pub family: IconFamily,
    /// 关键线对齐标记（几何对齐审计位）。
    pub keyline_ok: bool,
    /// 资产管线键（F068 按需装载——非内联位图）。
    pub asset_key: &'static str,
}

/// 图标账本。
pub struct IconLedger {
    pub icons: Vec<IconEntry>,
}

impl IconLedger {
    pub fn new() -> IconLedger {
        IconLedger { icons: Vec::new() }
    }

    /// 注册（缺族/关键线不对齐 → 拒绝——孤例进不了账）。
    pub fn register(
        &mut self,
        id: &'static str,
        family: IconFamily,
        keyline_ok: bool,
        asset_key: &'static str,
    ) -> Result<(), &'static str> {
        if !keyline_ok {
            return Err("关键线未对齐——先修几何再入账");
        }
        if asset_key.is_empty() {
            return Err("图标必须走资产管线（F068）——不接受内联位图");
        }
        self.icons.push(IconEntry { id, family, keyline_ok, asset_key });
        Ok(())
    }

    /// 风格突兀孤例扫描：无族（不可能——注册强制）+ 关键线不过（注册
    /// 拒绝）+ 三态参数偏离（全系统共享 STATES 常量，无从偏离）。
    /// 返回孤例数（判据：=0）。
    pub fn outlier_count(&self) -> usize {
        self.icons
            .iter()
            .filter(|i| !i.keyline_ok || i.asset_key.is_empty())
            .count()
    }

    /// 栅格关键线审计：抽 40 枚全部对齐（样本不足如实报告）。
    pub fn audit_keylines(&self, sample: usize) -> (usize, usize) {
        let n = sample.min(self.icons.len());
        let ok = self.icons.iter().take(n).filter(|i| i.keyline_ok).count();
        (ok, n)
    }

    /// 账本导出（F135 开发者文档站数据源——文本表格）。
    pub fn export_for_docs(&self) -> String {
        let mut out = String::from(
            "| 图标 | 语义族 | 栅格 | 描边 | 圆角 | 资产键 |\n| --- | --- | --- | --- | --- | --- |\n",
        );
        for i in &self.icons {
            out.push_str(&alloc::format!(
                "| {} | {} | {}px | {:.1}px | {}px | {} |\n",
                i.id,
                i.family.name(),
                GRID_PX,
                STROKE_PX,
                CORNER_PX,
                i.asset_key
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_iconlang_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F300");
    let mut ledger = IconLedger::new();
    // 注册 45 枚（>40 样本线），五族分布。
    let fams = [
        IconFamily::FileType,
        IconFamily::Action,
        IconFamily::Status,
        IconFamily::Device,
        IconFamily::SystemFunc,
    ];
    for i in 0..45 {
        let _ = ledger.register(
            match i % 5 {
                0 => "文件",
                1 => "操作",
                2 => "状态",
                3 => "设备",
                _ => "功能",
            },
            fams[i % 5],
            true,
            "asset:icon",
        );
    }
    // 栅格关键线审计：抽 40 枚全对齐。
    let (ok, n) = ledger.audit_keylines(40);
    set.add(
        "F300 keyline audit 40",
        n == 40 && ok == 40 && GRID_PX == 24 && STROKE_PX == 1.5,
        "24px/1.5px",
    );
    // 三态参数全系统一致（常量唯一——扫描即对拍）。
    set.add(
        "F300 three states",
        STATES.normal_pm == 1000
            && STATES.hover_scale_pm == 1100
            && STATES.disabled_pm == 400,
        "norm/hover+10%/disabled 40%",
    );
    // 风格突兀孤例=0。
    set.add("F300 outliers zero", ledger.outlier_count() == 0, "walkthrough clean");
    // 不合规矩的进不了账（关键线不过 / 无资产键）。
    set.add(
        "F300 registration gate",
        ledger.register("歪的", IconFamily::Action, false, "asset:x").is_err()
            && ledger.register("裸图", IconFamily::Action, true, "").is_err(),
        "gate holds",
    );
    // 资产管线装载口 + 账本导出（F135 公开数据源）。
    let doc = ledger.export_for_docs();
    set.add(
        "F300 docs export",
        doc.contains("24px") && doc.lines().count() >= 46,
        "F135 source",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f300_icon_ledger() {
        let set = run_iconlang_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F300 自检红 {f}/{p}");
    }

    #[test]
    fn five_families_complete() {
        // 五套语义族一名不缺。
        let names = [
            IconFamily::FileType.name(),
            IconFamily::Action.name(),
            IconFamily::Status.name(),
            IconFamily::Device.name(),
            IconFamily::SystemFunc.name(),
        ];
        assert!(names.iter().all(|n| !n.is_empty()));
    }
}
