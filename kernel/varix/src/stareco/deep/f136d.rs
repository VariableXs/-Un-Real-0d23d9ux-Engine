//! 深化层 · F136 示例应用仓库（2026-09-26 回炉补深化）。
//!
//! 补深：学习时长预估模型、知识递进图谱（示例间知识点账）、CI 夜跑
//! 排程器、冒烟判据（示例窗口出现即绿的判定核）、README 索引生成。

use crate::checks::CheckSet;
use crate::stareco::examples::{Example, EXAMPLE_COUNT};

// ---------------------------------------------------------------------------
// 学习时长预估（README「预计学习时长」的估算核）
// ---------------------------------------------------------------------------

/// 基础时长（分钟）随示例阶号递增，注释密度高酌减（扶手多学得快）。
pub fn estimate_minutes(no: usize, lines: usize, comment_lines: usize) -> u32 {
    let base = 20u32 + (no as u32) * 15;
    let density = if lines > 0 { comment_lines * 100 / lines } else { 0 };
    let relief = if density >= 30 { base / 5 } else { 0 };
    base - relief
}

// ---------------------------------------------------------------------------
// 知识递进图谱：每示例引入的新知识点
// ---------------------------------------------------------------------------

/// 五示例各自引入的知识点（②用①的知识+一点新——坡度可验）。
pub const KNOWLEDGE_LADDER: [&[&str]; 5] = [
    &["窗口", "事件循环"],
    &["窗口", "事件循环", "布局"],
    &["窗口", "事件循环", "布局", "文件IO"],
    &["窗口", "事件循环", "布局", "文件IO", "多窗口", "菜单"],
    &["窗口", "事件循环", "布局", "文件IO", "多窗口", "菜单", "完整应用组装"],
];

/// 坡度校验：第 N 阶必须包含第 N-1 阶的全部知识点（阶梯不倒退）。
pub fn ladder_monotonic() -> bool {
    for i in 1..EXAMPLE_COUNT {
        for k in KNOWLEDGE_LADDER[i - 1].iter() {
            if k.is_empty() {
                continue;
            }
            if !KNOWLEDGE_LADDER[i].contains(k) {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// CI 夜跑排程器 + 冒烟判据
// ---------------------------------------------------------------------------

/// 夜跑窗口：23:00 起，每示例 5 分钟slot，五个示例串行。
pub const NIGHT_START_HOUR: u32 = 23;
pub const SLOT_MINUTES: u32 = 5;

pub fn night_schedule() -> [(usize, u32); 5] {
    let mut out = [(0usize, 0u32); 5];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = (i + 1, NIGHT_START_HOUR * 60 + (i as u32) * SLOT_MINUTES);
    }
    out
}

/// 冒烟判据核：示例窗口出现（标题渲染 + 首帧绘制）即绿——
/// 缺一即红（截图基线比对的前置条件）。
pub fn smoke_verdict(window_shown: bool, first_frame: bool, title_rendered: bool) -> Result<(), &'static str> {
    if !window_shown {
        return Err("冒烟失败：示例窗口未出现");
    }
    if !first_frame {
        return Err("冒烟失败：首帧未绘制");
    }
    if !title_rendered {
        return Err("冒烟失败：标题未渲染");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// README 索引生成
// ---------------------------------------------------------------------------

/// 生成一条 README 索引行（一句话/构建命令/预计学习时长）。
pub fn readme_line(ex: &Example) -> alloc::string::String {
    let mins = estimate_minutes(ex.no, ex.lines, ex.comment_lines);
    alloc::format!(
        "examples/{:02} — {} — vxapp run examples/{:02} — 约 {} 分钟",
        ex.no,
        ex.title,
        ex.no,
        mins
    )
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F136D_TAG: &str = "stareco-F136-deep";

pub fn run_f136_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F136D_TAG);

    // 时长预估：扶手多的更省时
    let plain = estimate_minutes(2, 100, 20);
    let guided = estimate_minutes(2, 100, 45);
    set.add(
        "f136d guided cheaper",
        guided < plain && (plain - guided) == 10,
        "注释密度 30%+ 减 20%",
    );

    // 递进图谱坡度
    set.add("f136d knowledge ladder monotonic", ladder_monotonic(), "②含①全部知识点");
    set.add("f136d ladder depth", KNOWLEDGE_LADDER[4].contains(&"完整应用组装"), "⑤完整应用");

    // 夜跑排程
    let sched = night_schedule();
    set.add(
        "f136d night slots",
        sched[0] == (1, 23 * 60) && sched[4] == (5, 23 * 60 + 20),
        "23:00 起五槽串行",
    );

    // 冒烟判据
    set.add(
        "f136d smoke full green",
        smoke_verdict(true, true, true).is_ok(),
        "窗口+首帧+标题",
    );
    set.add(
        "f136d smoke failure reasons",
        smoke_verdict(false, true, true).is_err() && smoke_verdict(true, false, true).is_err(),
        "缺一即红带原因",
    );

    // README 索引行
    let ex = Example {
        no: 1,
        title: "hello",
        lines: 100,
        comment_lines: 40,
        builds_on: None,
        readme_ok: true,
    };
    let line = readme_line(&ex);
    set.add(
        "f136d readme line shape",
        line.contains("vxapp run examples/01") && line.contains("hello"),
        "一句话+命令+时长",
    );

    // 与主仓账联动：注册表完整时五行索引
    set.add("f136d index covers five", night_schedule().len() == EXAMPLE_COUNT, "五示例全排");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn estimate_no_negative() {
        assert!(estimate_minutes(1, 10, 10) > 0);
    }
}
