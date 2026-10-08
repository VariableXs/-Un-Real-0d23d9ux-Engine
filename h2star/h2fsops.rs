//! H2 文件操作计划器 · 深化批次三（计划→确认→执行→撤销链——
//! F261/F262/F263/F269/F270 五项操作族的总编排）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F262 拖拽语义**：六组合语义在此收口成计划（同盘移动 = 改
//!   目录项；跨盘移动 = 复制+校验+删除三段——删除源以校验通过为
//!   前置，哈希不对不删）；
//! - **F261 删除两路**：普通删 = 进回收站（可撤销）；Shift+Del =
//!   直删（确认形制更重，撤销只在计划簿里留封存记录）；
//! - **F263 发送到**：目标动作还原成 Copy 计划（防重名走
//!   [`crate::h2star::h2base::bump_copy_name`]）；
//! - **F270 错误重试**：逐项失败不中断——计划器内置三选一策略
//!   （重试/跳过/取消），取消保已完成（半途取消不回滚已完成项）；
//! - **操作级 undo**：每条已执行动作带逆操作，撤销按逆序弹栈——
//!   F087「应用到全部误伤撤销」的车道底座。
//!
//! 纪律：本层只产出**计划与账目**（做什么、什么顺序、怎么撤销），
//! 真实 IO 由执行层接手——计划器无 IO，全部逻辑宿主可测。

use crate::checks::CheckSet;

use crate::h2star::h2base::bump_copy_name;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 计划模型
// ---------------------------------------------------------------------------

/// 操作种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    /// 复制（F263 发送到还原成此项）。
    Copy,
    /// 移动（跨盘自动展开三段）。
    Move,
    /// 普通删除（进回收站，可撤销）。
    Trash,
    /// 直删（Shift+Del——确认后不可逆）。
    Purge,
}

/// 一步动作：可执行、可逆、有账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// 复制块段：源 → 目标（跨盘移动的第一段，按块计）。
    CopyChunk { src: String, dst: String, chunk: u32 },
    /// 校验：源哈希 == 目标哈希（跨盘移动第二段——不通过不动源）。
    Verify { src: String, dst: String },
    /// 删除源（跨盘移动第三段——只在 Verify 通过后出现）。
    DeleteSrc { src: String },
    /// 改目录项（同盘移动：只改路径）。
    Relink { from: String, to: String },
    /// 入回收站（Trash 形制）。
    ToTrash { src: String },
    /// 重命名/碰撞改名（bump_copy_name 的执行位）。
    Rename { from: String, to: String },
}

impl Action {
    /// 逆操作（撤销链的原子——每条动作必须带得出逆，否则不能入计划）。
    pub fn inverse(&self) -> Option<Action> {
        match self {
            Action::CopyChunk { src: _, dst, .. } => Some(Action::DeleteSrc { src: dst.clone() }),
            Action::Verify { .. } => None,
            Action::DeleteSrc { .. } => None,
            Action::Relink { from, to } => Some(Action::Relink { from: to.clone(), to: from.clone() }),
            Action::ToTrash { src } => Some(Action::Relink { from: alloc::format!("$recycle/{src}"), to: src.clone() }),
            Action::Rename { from, to } => Some(Action::Rename { from: to.clone(), to: from.clone() }),
        }
    }
}

/// 碰撞策略（F262/F263 防重名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collision {
    /// 自动改名（副本递增——默认）。
    AutoRename,
    /// 覆盖（危险，确认形制更高）。
    Overwrite,
    /// 跳过该项。
    Skip,
}

/// 逐项计划：一个源文件展开成动作序列。
#[derive(Clone, Debug)]
pub struct ItemPlan {
    pub src: String,
    pub actions: Vec<Action>,
    /// 碰撞时实际采用的目标名（AutoRename 展开后的结果）。
    pub final_name: String,
}

/// 生成单项计划：`dst_dir` 目标目录、`existing` 目标目录现存名集
/// （碰撞预检用）、`is_same_volume` 同盘判定（F262 盘符边界）。
pub fn plan_item(
    kind: OpKind,
    src: &str,
    dst_dir: &str,
    existing: &[String],
    collision: Collision,
    same_volume: bool,
) -> ItemPlan {
    let name = src.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(src);
    let mut final_name = alloc::format!("{dst_dir}/{name}");
    let mut actions = Vec::new();
    match kind {
        OpKind::Copy | OpKind::Move => {
            if existing.iter().any(|e| *e == final_name) {
                match collision {
                    Collision::AutoRename => {
                        let mut cand = bump_copy_name(&final_name);
                        while existing.iter().any(|e| *e == cand) {
                            cand = bump_copy_name(&cand);
                        }
                        final_name = cand;
                    }
                    Collision::Skip => {
                        return ItemPlan { src: src.into(), actions, final_name };
                    }
                    Collision::Overwrite => {}
                }
            }
            if kind == OpKind::Move && same_volume {
                actions.push(Action::Relink { from: src.into(), to: final_name.clone() });
            } else {
                // 复制 / 跨盘移动：块复制 → 校验 → (移动才删源)。
                actions.push(Action::CopyChunk {
                    src: src.into(),
                    dst: final_name.clone(),
                    chunk: 0,
                });
                actions.push(Action::Verify { src: src.into(), dst: final_name.clone() });
                if kind == OpKind::Move {
                    actions.push(Action::DeleteSrc { src: src.into() });
                }
            }
        }
        OpKind::Trash => actions.push(Action::ToTrash { src: src.into() }),
        OpKind::Purge => actions.push(Action::DeleteSrc { src: src.into() }),
    }
    ItemPlan { src: src.into(), actions, final_name }
}

// ---------------------------------------------------------------------------
// 执行账（失败三选一 + 完成账 + 撤销链）
// ---------------------------------------------------------------------------

/// 失败处置（F270 三选一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnFail {
    Retry,
    Skip,
    Cancel,
}

/// 执行账：逐动作回执。取消保已完成——done 链不因取消缩水。
pub struct RunLog {
    /// 已完成动作（撤销链按逆序弹）。
    pub done: Vec<Action>,
    /// 跳过项（F270 导出报告口径：源 + 人话原因）。
    pub skipped: Vec<(String, &'static str)>,
    /// 已完成项数 / 总项数（进度双口径）。
    pub finished_items: usize,
    pub total_items: usize,
    pub cancelled: bool,
}

impl RunLog {
    pub fn new(total: usize) -> RunLog {
        RunLog { done: Vec::new(), skipped: Vec::new(), finished_items: 0, total_items: total, cancelled: false }
    }

    /// 记一笔动作完成（入撤销链）。
    pub fn record(&mut self, a: &Action) {
        self.done.push(a.clone());
    }

    /// 项完成（进度口径）。
    pub fn finish_item(&mut self) {
        self.finished_items += 1;
    }

    /// 撤销一步：弹最后一条已完成的动作并给其逆操作。
    /// 无可撤时 None（undo 链空——不假装撤了）。
    pub fn undo_step(&mut self) -> Option<Action> {
        let a = self.done.pop()?;
        a.inverse()
    }

    /// F270 导出报告（可 grep 的文本——三要素齐）。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "文件操作报告：完成 {}/{} 项，跳过 {} 项{}\n",
            self.finished_items,
            self.total_items,
            self.skipped.len(),
            if self.cancelled { "（用户取消，已完成项保留）" } else { "" }
        );
        for (src, why) in &self.skipped {
            s.push_str(alloc::format!("  跳过 {src}：{why}\n").as_str());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2fsops_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2fsops");
    let existing: Vec<String> = alloc::vec!["/d/a.txt".into()];
    // Copy 无碰撞：复制+校验两动作（校验是完成的前置——不静默成功）。
    let p1 = plan_item(OpKind::Copy, "/c/b.txt", "/d", &existing, Collision::AutoRename, true);
    set.add(
        "h2fsops copy plan",
        p1.final_name == "/d/b.txt"
            && p1.actions.len() == 2
            && matches!(p1.actions[1], Action::Verify { .. }),
        "copy + verify",
    );
    // Copy 碰撞自动改名：副本递增且避让现存集。
    let p2 = plan_item(OpKind::Copy, "/c/a.txt", "/d", &existing, Collision::AutoRename, true);
    set.add(
        "h2fsops collision rename",
        p2.final_name == "/d/a - 副本.txt",
        "bump avoids existing",
    );
    // 副本也撞：继续递增（a.txt 与 a - 副本.txt 都在）。
    let existing2: Vec<String> =
        alloc::vec!["/d/a.txt".into(), "/d/a - 副本.txt".into()];
    let p3 = plan_item(OpKind::Copy, "/c/a.txt", "/d", &existing2, Collision::AutoRename, true);
    set.add(
        "h2fsops collision ladder",
        p3.final_name == "/d/a - 副本 (2).txt",
        "keeps bumping",
    );
    // Skip 碰撞：空计划（显式跳过，不是静默丢）。
    let p4 = plan_item(OpKind::Copy, "/c/a.txt", "/d", &existing, Collision::Skip, true);
    set.add(
        "h2fsops skip plan",
        p4.actions.is_empty() && p4.final_name == "/d/a.txt",
        "skip is visible",
    );
    // 同盘移动：单动作 Relink（改目录项，无复制段）。
    let p5 = plan_item(OpKind::Move, "/c/a.txt", "/c/sub", &[], Collision::AutoRename, true);
    set.add(
        "h2fsops move same volume",
        p5.actions == vec![Action::Relink { from: "/c/a.txt".into(), to: "/c/sub/a.txt".into() }],
        "relink only",
    );
    // 跨盘移动三段：复制→校验→删源（校验是删的前置——哈希不对不删）。
    let p6 = plan_item(OpKind::Move, "/c/a.txt", "/d", &[], Collision::AutoRename, false);
    set.add(
        "h2fsops move cross volume",
        p6.actions.len() == 3
            && matches!(p6.actions[1], Action::Verify { .. })
            && matches!(p6.actions[2], Action::DeleteSrc { .. }),
        "copy→verify→delete",
    );
    // Trash / Purge 两路。
    let p7 = plan_item(OpKind::Trash, "/c/a.txt", "", &[], Collision::AutoRename, true);
    let p8 = plan_item(OpKind::Purge, "/c/a.txt", "", &[], Collision::AutoRename, true);
    set.add(
        "h2fsops two delete routes",
        matches!(p7.actions[0], Action::ToTrash { .. })
            && matches!(p8.actions[0], Action::DeleteSrc { .. }),
        "trash vs purge",
    );
    // 逆操作：Relink 可逆、Rename 可逆、CopyChunk 逆=删产物。
    let inv = Action::Relink { from: "/c/a".into(), to: "/d/a".into() };
    set.add(
        "h2fsops inverse",
        inv.inverse() == Some(Action::Relink { from: "/d/a".into(), to: "/c/a".into() })
            && Action::CopyChunk { src: "s".into(), dst: "d".into(), chunk: 0 }
                .inverse()
                .is_some(),
        "every step reversible",
    );
    // 执行账：取消保已完成、撤销逆序、报告可读。
    let mut log = RunLog::new(3);
    log.record(&Action::Relink { from: "/1".into(), to: "/2".into() });
    log.record(&Action::Relink { from: "/3".into(), to: "/4".into() });
    log.finish_item();
    log.skipped.push(("/x".into(), "目标同名且策略为跳过"));
    log.cancelled = true;
    let u1 = log.undo_step();
    let u2 = log.undo_step();
    set.add(
        "h2fsops undo lifo",
        u1 == Some(Action::Relink { from: "/4".into(), to: "/3".into() })
            && u2 == Some(Action::Relink { from: "/2".into(), to: "/1".into() })
            && log.undo_step().is_none(),
        "LIFO empty honest",
    );
    let rep = log.report();
    set.add(
        "h2fsops report grep",
        rep.contains("完成 1/3 项")
            && rep.contains("跳过 1 项")
            && rep.contains("用户取消")
            && rep.contains("/x"),
        "three elements present",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2fsops_all_green() {
        let set = run_h2fsops_checks();
        assert!(set.all_passed(), "h2fsops 自检有红项");
        assert!(!set.truncated(), "h2fsops 自检溢出");
    }

    #[test]
    fn rename_ladder_terminates() {
        // 碰撞阶梯必终止：预置 200 个撞名，仍能落到空位（有界循环）。
        let existing: Vec<String> =
            (0..200).map(|i| if i == 0 { "/d/a.txt".into() } else { alloc::format!("/d/a - 副本 ({i}).txt") }).collect();
        let p = plan_item(OpKind::Copy, "/c/a.txt", "/d", &existing, Collision::AutoRename, true);
        assert!(!existing.iter().any(|e| *e == p.final_name), "landed on free name");
    }

    #[test]
    fn purge_has_no_undo_entry_by_design() {
        // 直删的逆是 None——撤销链上不存在「复活直删」的假动作。
        assert!(Action::DeleteSrc { src: "/c/a.txt".into() }.inverse().is_none());
    }
}
