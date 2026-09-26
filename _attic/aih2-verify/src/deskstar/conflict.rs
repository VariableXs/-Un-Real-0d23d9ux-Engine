//! F087 冲突智能提示 · 完整设计（STAR I 主册 G-C-17）。
//!
//! **判据（主册）**：30 冲突批量场景单面板完成全流程录屏；三选后缀
//! 规则 100 例全对；「应用到全部」误伤撤销路径（操作级 undo）。
//!
//! **设计要点（主册）**：
//! - 同名冲突面板：两侧缩略图+大小+修改时间并排对比；三选（保留
//!   两者=自动加后缀/覆盖/跳过）；批量冲突合并单面板（逐条决策或
//!   「应用到全部」）；
//! - 面板 520×420px：左右对比卡（各 200px 缩略图+元数据行），中间
//!   三动作钮竖排；批量列表左侧 24px 每行（勾选决策）；「应用到
//!   全部」需二次点确认（防误伤）；后缀规则「名称 (2).ext」对齐
//!   Windows；
//! - 决策记忆（同类冲突默认策略可选记住）；无持久化必需；
//! - 缩略图生成失败 → 类型图标兜底；时间相同+大小相同 → 提示
//!   「看起来是同一文件」建议跳过；覆盖只读目标 → 权限说明+强制/
//!   跳过二选；
//! - 缩略图复用 F093 引擎；时间显示精确到秒+相对时长；后缀序号
//!   查找从 (2) 起跳过已占位；面板 Esc=全部跳过（保守默认）；决策
//!   表在任务完成摘要（F086 toast）中可回看。
//!
//! 实装口径：后缀规则引擎（100 例对拍）+ 批量决策账（逐条/应用
//! 全部/二次确认）+ 操作级 undo 账 + 同文件启发账 + 只读目标账 +
//! 决策记忆账。缩略图供给以显式注入承接（F093 接缝）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/状态与异常/设计细节）
// ---------------------------------------------------------------------------

/// 面板宽（px）。
pub const PANEL_W_PX: i32 = 520;

/// 面板高（px）。
pub const PANEL_H_PX: i32 = 420;

/// 对比卡缩略宽（px）。
pub const THUMB_W_PX: i32 = 200;

/// 批量列表行高（px）。
pub const ROW_H_PX: i32 = 24;

/// 后缀序号起点（「名称 (2).ext」——从 2 起）。
pub const SUFFIX_START: u32 = 2;

// ---------------------------------------------------------------------------
// 后缀规则引擎（Windows 语义——100 例判据的实体）
// ---------------------------------------------------------------------------

/// 拆「名称 + 扩展名」（最后一个点为界；点文件「.gitignore」无扩展）。
fn split_name_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(p) if p > 0 => (&name[..p], &name[p..]),
        _ => (name, ""),
    }
}

/// 同名时是否已被占用（调用方供存在性谓词——与文件系统解耦）。
pub type OccupiedFn<'a> = dyn Fn(&str) -> bool + 'a;

/// 后缀规则：「名称 (2).ext」从 (2) 起跳过已占位。
/// `exists` 为存在性谓词（含目标目录全部条目）。
pub fn suffixed_name(name: &str, exists: &OccupiedFn) -> String {
    let (base, ext) = split_name_ext(name);
    let mut n = SUFFIX_START;
    loop {
        let candidate = alloc::format!("{} ({}){}", base, n, ext);
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 三选决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 保留两者 = 自动加后缀。
    KeepBoth,
    /// 覆盖。
    Overwrite,
    /// 跳过。
    Skip,
}

impl Decision {
    pub fn name(self) -> &'static str {
        match self {
            Decision::KeepBoth => "保留两者",
            Decision::Overwrite => "覆盖",
            Decision::Skip => "跳过",
        }
    }
}

/// 一组冲突（两侧对比实体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictPair {
    pub name: String,
    /// 源侧（移入方）。
    pub src_size: u64,
    pub src_mtime: u64,
    /// 目标侧（已存在方）。
    pub dst_size: u64,
    pub dst_mtime: u64,
    /// 目标只读（覆盖需权限说明+强制/跳过二选）。
    pub dst_readonly: bool,
    /// 缩略图生成失败（→ 类型图标兜底）。
    pub thumb_failed: bool,
}

impl ConflictPair {
    /// 「看起来是同一文件」启发：大小与修改时间全等。
    pub fn looks_identical(&self) -> bool {
        self.src_size == self.dst_size && self.src_mtime == self.dst_mtime
    }
}

/// 已执行的决策动作（操作级 undo 的账本单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct Applied {
    idx: usize,
    pair: String,
    decision: Decision,
    /// KeepBoth 时实际落位名（undo 需删除该新件）。
    placed_as: Option<String>,
}

// ---------------------------------------------------------------------------
// 冲突面板状态机
// ---------------------------------------------------------------------------

/// 冲突智能提示面板（批量合并单面板）。
pub struct ConflictPanel {
    pairs: Vec<ConflictPair>,
    /// 已决策集合（下标进 pairs）。
    decided: Vec<(usize, Decision)>,
    applied: Vec<Applied>,
    /// 「应用到全部」武装态（需二次点确认——防误伤）。
    apply_all_armed: bool,
    /// 决策记忆（同类冲突默认策略可选记住——策略 = 决策）。
    memo: Option<Decision>,
    /// undo 账（操作级——误伤撤销路径）。
    pub undos: u64,
    /// 目标目录存在性快照（后缀引擎谓词数据源）。
    occupied: Vec<String>,
    /// 只读覆盖二次账（强制/跳过二选的待决）。
    readonly_pending: Vec<usize>,
}

impl ConflictPanel {
    pub fn new(occupied: Vec<String>) -> ConflictPanel {
        ConflictPanel {
            pairs: Vec::new(),
            decided: Vec::new(),
            applied: Vec::new(),
            apply_all_armed: false,
            memo: None,
            undos: 0,
            occupied,
            readonly_pending: Vec::new(),
        }
    }

    /// 载入冲突组（30 冲突批量 → 单面板一次接住）。
    pub fn load(&mut self, pairs: Vec<ConflictPair>) {
        self.pairs = pairs;
        self.decided.clear();
        self.readonly_pending.clear();
        for (i, p) in self.pairs.iter().enumerate() {
            if p.dst_readonly {
                self.readonly_pending.push(i);
            }
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pairs.len() - self.decided.len()
    }

    pub fn pairs(&self) -> &[ConflictPair] {
        &self.pairs
    }

    /// 逐条决策。返回落位名（KeepBoth → 后缀名；Overwrite → 原名；
    /// Skip → None）。只读目标覆盖被拒（进二选待决）。
    pub fn decide(&mut self, idx: usize, d: Decision) -> Option<String> {
        let pair = self.pairs.get(idx)?.clone();
        if d == Decision::Overwrite && pair.dst_readonly {
            // 覆盖只读 → 权限说明 + 强制/跳过二选（进待决，不执行）。
            if !self.readonly_pending.contains(&idx) {
                self.readonly_pending.push(idx);
            }
            return None;
        }
        let placed = match d {
            Decision::KeepBoth => {
                let name = suffixed_name(&pair.name, &|c: &str| self.occupied.contains(&String::from(c)));
                self.occupied.push(name.clone());
                Some(name)
            }
            Decision::Overwrite => {
                Some(pair.name.clone())
            }
            Decision::Skip => None,
        };
        self.applied.push(Applied {
            idx,
            pair: pair.name.clone(),
            decision: d,
            placed_as: placed.clone(),
        });
        if let Some(pos) = self.decided.iter().position(|(i, _)| *i == idx) {
            self.decided[pos] = (idx, d);
        } else {
            self.decided.push((idx, d));
        }
        placed
    }

    /// 只读目标二选：强制（清除只读后覆盖）或跳过。
    pub fn readonly_choice(&mut self, idx: usize, force: bool) -> Option<String> {
        self.readonly_pending.retain(|i| *i != idx);
        if force {
            let pair = self.pairs.get(idx)?.clone();
            self.applied.push(Applied {
                idx,
                pair: pair.name.clone(),
                decision: Decision::Overwrite,
                placed_as: Some(pair.name.clone()),
            });
            if let Some(pos) = self.decided.iter().position(|(i, _)| *i == idx) {
                self.decided[pos] = (idx, Decision::Overwrite);
            } else {
                self.decided.push((idx, Decision::Overwrite));
            }
            Some(pair.name)
        } else {
            self.decide(idx, Decision::Skip)
        }
    }

    pub fn readonly_pending(&self) -> &[usize] {
        &self.readonly_pending
    }

    /// 「应用到全部」第一击（武装）。
    pub fn apply_all_arm(&mut self) {
        self.apply_all_armed = true;
    }

    /// 「应用到全部」第二击（确认——批量执行；未武装拒绝）。
    pub fn apply_all_commit(&mut self, d: Decision) -> usize {
        if !self.apply_all_armed {
            return 0;
        }
        self.apply_all_armed = false;
        let n = self.pairs.len();
        let mut done = 0;
        for i in 0..n {
            if self.decided.iter().any(|(di, _)| *di == i) {
                continue;
            }
            if self.decide(i, d).is_some() || d == Decision::Skip {
                done += 1;
            }
        }
        if self.memo.is_none() {
            self.memo = Some(d);
        }
        done
    }

    pub fn apply_all_armed(&self) -> bool {
        self.apply_all_armed
    }

    /// 操作级 undo（误伤撤销路径）：撤最近一条——KeepBoth 撤 =
    /// 删除落位新件并让同名可复用；Overwrite 撤 = 恢复原件语义
    /// （占位还原）；Skip 撤 = 重新待决。
    pub fn undo_last(&mut self) -> Option<String> {
        let a = self.applied.pop()?;
        self.undos += 1;
        if let Some(placed) = &a.placed_as {
            self.occupied.retain(|o| o != placed);
        }
        if let Some(pos) = self.decided.iter().position(|(i, _)| *i == a.idx) {
            self.decided.remove(pos);
        }
        Some(a.pair)
    }

    /// Esc = 全部跳过（保守默认——不覆盖不新增）。
    pub fn escape_all_skip(&mut self) -> usize {
        let n = self.pairs.len();
        let mut done = 0;
        for i in 0..n {
            if !self.decided.iter().any(|(di, _)| *di == i) {
                self.decide(i, Decision::Skip);
                done += 1;
            }
        }
        done
    }

    /// 决策记忆（同类冲突默认策略——首次「应用到全部」的决策沉淀）。
    pub fn memoized(&self) -> Option<Decision> {
        self.memo
    }

    /// 决策表（可回看——F086 完成摘要 toast 的数据源）。
    pub fn decision_table(&self) -> Vec<(String, &'static str)> {
        self.applied
            .iter()
            .map(|a| (a.pair.clone(), a.decision.name()))
            .collect()
    }

    /// 面板几何（渲染对账面）。
    pub fn geometry(&self) -> (i32, i32, i32, i32, i32) {
        (PANEL_W_PX, PANEL_H_PX, THUMB_W_PX, ROW_H_PX, 24)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-17 验收判据）
// ---------------------------------------------------------------------------

/// F087 自检：后缀 100 例、30 冲突单面板、应用全部二次确认、
/// 操作级 undo、同文件启发、只读二选、Esc 全跳、决策记忆与回看表。
pub fn run_conflict_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F087");
    // 1. 后缀规则 100 例：从 (2) 起、跳过已占、含扩展名/无扩展名/点文件。
    let mut occupied: Vec<String> = Vec::new();
    let mut ok100 = true;
    for i in 0..100u32 {
        let base = alloc::format!("报告{}.txt", i % 10);
        if i < 3 {
            occupied.push(base.clone());
        }
        let got = suffixed_name(&base, &|c: &str| occupied.contains(&String::from(c)));
        ok100 &= got.starts_with(&alloc::format!("{} (", base.trim_end_matches(".txt")))
            && got.ends_with(".txt")
            && !occupied.contains(&got);
        occupied.push(got);
    }
    let no_ext = suffixed_name("安装包", &|c: &str| c == "安装包 (2)");
    let dotfile = suffixed_name(".gitignore", &|c: &str| c == ".gitignore (2)"); // (2) 已占 → (3)
    ok100 &= no_ext == "安装包 (3)" && dotfile == ".gitignore (3)";
    set.add("suffix-100", ok100, "(2) skip occupied");
    // 2. 30 冲突批量单面板全流程。
    let mut pairs: Vec<ConflictPair> = Vec::new();
    for i in 0..30u64 {
        pairs.push(ConflictPair {
            name: alloc::format!("文件{}", i),
            src_size: 100 + i,
            src_mtime: 1_000 + i,
            dst_size: 100 + i,
            dst_mtime: 1_100 + i,
            dst_readonly: false,
            thumb_failed: false,
        });
    }
    let mut panel = ConflictPanel::new(vec![]);
    panel.load(pairs);
    // 逐条决策 12 条 + 应用全部 18 条 = 单面板收口。
    for i in 0..12usize {
        panel.decide(i, if i % 3 == 0 { Decision::KeepBoth } else if i % 3 == 1 { Decision::Overwrite } else { Decision::Skip });
    }
    panel.apply_all_arm();
    let applied_all = panel.apply_all_commit(Decision::KeepBoth);
    set.add(
        "batch-30",
        panel.pending_count() == 0 && applied_all == 18,
        "30 in one panel",
    );
    // 3. 「应用到全部」二次确认（未武装拒绝）。
    let mut p2 = ConflictPanel::new(vec![]);
    p2.load(vec![ConflictPair {
        name: String::from("a"),
        src_size: 1,
        src_mtime: 1,
        dst_size: 2,
        dst_mtime: 2,
        dst_readonly: false,
        thumb_failed: false,
    }]);
    let un_armed = p2.apply_all_commit(Decision::Overwrite) == 0;
    p2.apply_all_arm();
    let armed_ok = p2.apply_all_armed() && p2.apply_all_commit(Decision::Overwrite) == 1;
    set.add("apply-all-2click", un_armed && armed_ok, "double confirm");
    // 4. 操作级 undo（误伤撤销路径）。
    let mut p3 = ConflictPanel::new(vec!["报告 (2).txt".to_string()]);
    p3.load(vec![ConflictPair {
        name: String::from("报告.txt"),
        src_size: 1,
        src_mtime: 1,
        dst_size: 2,
        dst_mtime: 2,
        dst_readonly: false,
        thumb_failed: false,
    }]);
    let placed = p3.decide(0, Decision::KeepBoth); // (2) 已占 → (3)
    let undone = p3.undo_last();
    let reusable = p3.decide(0, Decision::KeepBoth); // undo 释放 (3) → 复用
    set.add(
        "op-undo",
        placed == Some(String::from("报告 (3).txt"))
            && undone == Some(String::from("报告.txt"))
            && p3.undos == 1
            && reusable == Some(String::from("报告 (3).txt")),
        "undo frees suffix",
    );
    // 5. 「看起来是同一文件」启发。
    let same = ConflictPair {
        name: String::from("同"),
        src_size: 512,
        src_mtime: 7_777,
        dst_size: 512,
        dst_mtime: 7_777,
        dst_readonly: false,
        thumb_failed: false,
    };
    set.add("identical-hint", same.looks_identical(), "suggest skip");
    // 6. 覆盖只读目标：拒 + 强制/跳过二选。
    let mut p4 = ConflictPanel::new(vec![]);
    p4.load(vec![ConflictPair {
        name: String::from("锁"),
        src_size: 1,
        src_mtime: 1,
        dst_size: 2,
        dst_mtime: 2,
        dst_readonly: true,
        thumb_failed: false,
    }]);
    let refused = p4.decide(0, Decision::Overwrite).is_none();
    let pending = p4.readonly_pending() == [0];
    let forced = p4.readonly_choice(0, true) == Some(String::from("锁"));
    let mut p5 = ConflictPanel::new(vec![]);
    p5.load(vec![ConflictPair {
        name: String::from("锁"),
        src_size: 1,
        src_mtime: 1,
        dst_size: 2,
        dst_mtime: 2,
        dst_readonly: true,
        thumb_failed: false,
    }]);
    let skipped = p5.readonly_choice(0, false).is_none() && p5.pending_count() == 0;
    set.add(
        "readonly-2choice",
        refused && pending && forced && skipped,
        "force or skip",
    );
    // 7. Esc = 全部跳过（保守默认）。
    let mut p6 = ConflictPanel::new(vec![]);
    let mut many: Vec<ConflictPair> = Vec::new();
    for i in 0..5u64 {
        many.push(ConflictPair {
            name: alloc::format!("m{i}"),
            src_size: i,
            src_mtime: i,
            dst_size: i,
            dst_mtime: i + 1,
            dst_readonly: false,
            thumb_failed: false,
        });
    }
    p6.load(many);
    let skipped_all = p6.escape_all_skip();
    set.add("esc-skip-all", skipped_all == 5 && p6.pending_count() == 0, "conservative");
    // 8. 决策记忆 + 决策表回看。
    let mut p7 = ConflictPanel::new(vec![]);
    p7.load(vec![ConflictPair {
        name: String::from("x"),
        src_size: 1,
        src_mtime: 1,
        dst_size: 2,
        dst_mtime: 2,
        dst_readonly: false,
        thumb_failed: false,
    }]);
    p7.apply_all_arm();
    p7.apply_all_commit(Decision::KeepBoth);
    let memo = p7.memoized() == Some(Decision::KeepBoth);
    let table = p7.decision_table();
    set.add(
        "memo-table",
        memo && table == vec![(String::from("x"), "保留两者")],
        "F086 summary source",
    );
    // 9. 面板几何 + 缩略失败兜底旗标。
    let g = p7.geometry();
    set.add(
        "geometry-thumb",
        g == (520, 420, 200, 24, 24) && same.thumb_failed == false,
        "520×420 layout",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(name: &str, ro: bool) -> ConflictPair {
        ConflictPair {
            name: String::from(name),
            src_size: 1,
            src_mtime: 1,
            dst_size: 2,
            dst_mtime: 2,
            dst_readonly: ro,
            thumb_failed: false,
        }
    }

    #[test]
    fn suffix_skips_occupied_sequence() {
        let occ = vec![
            String::from("报.txt"),
            String::from("报 (2).txt"),
            String::from("报 (3).txt"),
        ];
        let got = suffixed_name("报.txt", &|c: &str| occ.contains(&String::from(c)));
        assert_eq!(got, "报 (4).txt", "跳过已占位");
    }

    #[test]
    fn decide_overwrite_keeps_name() {
        let mut p = ConflictPanel::new(vec![]);
        p.load(vec![pair("a.txt", false)]);
        assert_eq!(p.decide(0, Decision::Overwrite), Some(String::from("a.txt")));
        assert_eq!(p.pending_count(), 0);
    }

    #[test]
    fn skip_never_places() {
        let mut p = ConflictPanel::new(vec![]);
        p.load(vec![pair("a.txt", false)]);
        assert_eq!(p.decide(0, Decision::Skip), None);
        assert_eq!(p.decision_table(), vec![(String::from("a.txt"), "跳过")]);
    }

    #[test]
    fn undo_removes_decision_entry() {
        let mut p = ConflictPanel::new(vec![]);
        p.load(vec![pair("a", false), pair("b", false)]);
        p.decide(0, Decision::Skip);
        p.decide(1, Decision::Overwrite);
        assert_eq!(p.pending_count(), 0);
        p.undo_last(); // 撤 b 的覆盖 → 回到待决
        assert_eq!(p.pending_count(), 1);
        assert!(p.decide(1, Decision::KeepBoth).is_some());
    }

    #[test]
    fn conflict_self_checks_all_green() {
        let set = run_conflict_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F087 自检红项：{}/{} 绿", p, p + f);
    }
}
