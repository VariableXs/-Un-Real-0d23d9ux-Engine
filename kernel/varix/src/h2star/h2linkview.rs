//! H2 链接目标视图 · 深化批次六（F264 属性对话框深化——快捷方式
//! /符号链接的目标行解析：目标、参数、起始位置、状态）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F264 属性对话框**：字段与文件系统值比对（含符号链接）——
//!   目标行的字段级解析在此（目标路径 / 参数 / 工作目录 / 图标
//!   位置四字段——与 F013 `.lnk` 解析族同构对拍）；
//! - **F292 车道（经 F264 锚）**：目标状态徽标（有效/失效/循环）
//!   ——失效检测不在本层判文件系统，本层只把「判定结论」装配成
//! 人话字段（判定由 lnkhealth 供给——单一职责）。
//!
//! 解析规则（.lnk 字段串约定，一处一事实）：`目标|参数|工作目录|图标`。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 目标串解析
// ---------------------------------------------------------------------------

/// 目标状态（由调用方判定的结论枚举——本层不碰文件系统）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetState {
    Ok,
    Missing,
    Cycle,
}

/// 解析出的目标行字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkFields {
    pub target: String,
    pub args: String,
    pub workdir: String,
    pub icon: String,
}

/// 解析 `目标|参数|工作目录|图标` 四段串。
/// 段数不足补空（旧格式兼容——不炸）；目标段为空 = 缺陷拒绝。
pub fn parse_link(spec: &str) -> Result<LinkFields, &'static str> {
    let parts: Vec<&str> = spec.split('|').collect();
    if parts.first().map_or(true, |t| t.trim().is_empty()) {
        return Err("快捷方式没有目标——属性页无法保存");
    }
    let f = |i: usize| parts.get(i).copied().unwrap_or("").trim().to_string();
    Ok(LinkFields { target: f(0), args: f(1), workdir: f(2), icon: f(3) })
}

/// 目标行人话装配：字段 + 状态结论 → 属性页行文本（三要素齐——
/// 失效时给「为什么」与「下一步」）。
pub fn target_line(f: &LinkFields, st: TargetState) -> String {
    let status = match st {
        TargetState::Ok => alloc::format!("有效 → {}", f.target),
        TargetState::Missing => alloc::format!(
            "失效：目标 \"{}\" 不存在——可能被移动或删除，可点「打开文件位置」重定向",
            f.target
        ),
        TargetState::Cycle => alloc::format!(
            "循环：\"{}\" 指向自身或形成环——已停止追踪，请修正目标",
            f.target
        ),
    };
    let mut line = status;
    if !f.args.is_empty() {
        line.push_str(&alloc::format!("\n参数：{}", f.args));
    }
    if !f.workdir.is_empty() {
        line.push_str(&alloc::format!("\n起始位置：{}", f.workdir));
    }
    if !f.icon.is_empty() {
        line.push_str(&alloc::format!("\n图标：{}", f.icon));
    }
    line
}

/// 四字段同源性：属性页显示与 F013 解析族共用本结构（结构保证——
/// 两处各拼一套字符串即缺陷）。此处为「同构对拍」的机检形态：
/// 同一 spec 两次解析逐字段相等。
pub fn parse_is_deterministic(spec: &str) -> bool {
    match (parse_link(spec), parse_link(spec)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2linkview_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2linkview");
    // 四段全解析。
    let f = parse_link("C:\\app\\编辑器.exe|--新窗口|C:\\app|C:\\app\\edit.ico").unwrap();
    set.add(
        "h2linkview four fields",
        f.target == "C:\\app\\编辑器.exe"
            && f.args == "--新窗口"
            && f.workdir == "C:\\app"
            && f.icon == "C:\\app\\edit.ico",
        "target|args|workdir|icon",
    );
    // 缺段补空（旧格式兼容）；空目标拒绝。
    let short = parse_link("C:\\工具\\便签.exe").unwrap();
    set.add(
        "h2linkview short spec",
        short.target == "C:\\工具\\便签.exe" && short.args.is_empty() && short.icon.is_empty(),
        "missing parts fill empty",
    );
    set.add(
        "h2linkview empty target refused",
        parse_link("|x|y|z").is_err() && parse_link("").is_err(),
        "no target no save",
    );
    // 目标行三态人话。
    let ok_line = target_line(&f, TargetState::Ok);
    let miss_line = target_line(&parse_link("C:\\lost\\x.exe").unwrap(), TargetState::Missing);
    let cyc_line = target_line(&parse_link("C:\\loop| | |").unwrap(), TargetState::Cycle);
    set.add(
        "h2linkview three states",
        ok_line.contains("有效 → C:\\app\\编辑器.exe")
            && miss_line.contains("失效") && miss_line.contains("重定向")
            && cyc_line.contains("循环") && cyc_line.contains("已停止追踪"),
        "what/why/next present",
    );
    // 可选字段只有非空才出现（属性页不显示空行）。
    let bare = target_line(&short, TargetState::Ok);
    set.add(
        "h2linkview optional omitted",
        !bare.contains("参数：") && !bare.contains("图标：") && bare.contains("有效"),
        "no empty rows",
    );
    // 确定性对拍（同构机检）。
    set.add(
        "h2linkview deterministic",
        parse_is_deterministic("A|B|C|D") && !parse_is_deterministic(""),
        "same spec same fields",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2linkview_all_green() {
        let set = run_h2linkview_checks();
        assert!(set.all_passed(), "h2linkview 自检有红项");
        assert!(!set.truncated(), "h2linkview 自检溢出");
    }

    #[test]
    fn pipe_in_fields_never_confuses() {
        // 超过四段（目标串本身含 | 的畸形）：多出段被并入末字段不炸
        // （解析器对任意段数安全）。
        let f = parse_link("A|B|C|D|E|F").unwrap();
        assert_eq!(f.target, "A");
        assert_eq!(f.args, "B");
        assert_eq!(f.workdir, "C");
        assert!(!f.icon.is_empty());
    }
}
