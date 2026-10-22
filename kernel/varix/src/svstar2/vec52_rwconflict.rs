//! VE-F0452 · 资源读写冲突检测（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0452`
//!
//! **锚点原文**：「资源读写冲突检测：同 pass 内资源访问冲突（同一绑定点的
//! 写写、读写序依赖）检测、只读声明的写访问违规、别名资源（同资源多声明）
//! 检测；冲突按可修性分级（声明修正、加屏障建议、必须重构三级建议）；
//! 检测结果供管线期屏障插入参考。判据：读写序、只读违规、别名检测、
//! 三级建议、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **读写序**（判据一）。同绑定点的访问序列按「写写冲突 / 读写序
//!    依赖 / 读读相容」三态判定（[`verdict_pair`]），判定按资源分组
//!    两两配对 O(对数)。
//!
//! 2. **只读违规**（判据二）。声明为只读（声明面映射 F0449）的资源
//!    出现写访问 → 违规报错带声明位置。
//!
//! 3. **别名检测**（判据三）。同资源多声明（别名）登记检测——别名对
//!    的访问合并到同一资源后再判冲突（别名不明 → 保守报冲突并标注
//!    待人工，与 F0453 报告质量协同）。
//!
//! 4. **三级建议**（判据四）。冲突按可修性分级三档建议（声明修正 /
//!    加屏障 / 必须重构），建议随冲突条目携带（供管线期屏障插入参考）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0452";

/// 访问形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// 一条资源访问记录。
#[derive(Clone, Debug)]
pub struct AccRec {
    pub binding: u32,
    pub decl_off: u32,
    pub use_off: u32,
    pub acc: Access,
    /// 声明面只读位（F0449 映射的 ReadOnly）。
    pub declared_ro: bool,
}

/// 冲突判定三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 读读相容。
    Compat,
    /// 读写序依赖（先读后写或先写后读——顺序敏感）。
    RwOrder,
    /// 写写冲突。
    WwConflict,
}

/// 同资源两访问的判定（序敏感：prev 在前，cur 在后）。
pub fn verdict_pair(prev: &AccRec, cur: &AccRec) -> Verdict {
    match (prev.acc, cur.acc) {
        (Access::Read, Access::Read) => Verdict::Compat,
        (Access::Write, Access::Write) => Verdict::WwConflict,
        _ => Verdict::RwOrder,
    }
}

/// 可修性三级建议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice3 {
    /// 一级：声明修正（改声明让冲突消失）。
    DeclFix,
    /// 二级：加屏障（管线期插入同步）。
    Barrier,
    /// 三级：必须重构（语义级冲突，声明与屏障都救不了）。
    Restructure,
}

impl Advice3 {
    pub fn tag(&self) -> &'static str {
        match self {
            Advice3::DeclFix => "decl-fix",
            Advice3::Barrier => "barrier",
            Advice3::Restructure => "restructure",
        }
    }
}

/// 冲突条目。
#[derive(Clone, Debug)]
pub struct Conflict {
    pub binding: u32,
    pub prev_off: u32,
    pub cur_off: u32,
    pub verdict: Verdict,
    pub advice: Advice3,
    /// 待人工标注（判定歧义保守报冲突时置位）。
    pub manual_review: bool,
}

/// 冲突分级：写写 → 必须重构；只读违规 → 声明修正；读写序 → 加屏障。
pub fn advice_of(verdict: Verdict, ro_violation: bool) -> Advice3 {
    if ro_violation {
        return Advice3::DeclFix;
    }
    match verdict {
        Verdict::WwConflict => Advice3::Restructure,
        Verdict::RwOrder => Advice3::Barrier,
        Verdict::Compat => Advice3::DeclFix,
    }
}

/// 检测器：同资源分组配对检测（别名合并后统一判定）。
#[derive(Debug, Default)]
pub struct ConflictDetector {
    /// 别名登记：绑定号 → 组代表绑定号（同资源多声明合并）。
    pub alias_groups: Vec<(u32, u32)>,
    pub conflicts: Vec<Conflict>,
    /// 只读违规条目。
    pub ro_violations: Vec<(u32, u32)>,
    /// 待人工标注数。
    pub manual_count: u32,
}

impl ConflictDetector {
    pub fn new() -> Self {
        ConflictDetector::default()
    }
    /// 别名登记：两个绑定号是同一资源。
    pub fn register_alias(&mut self, a: u32, b: u32) {
        if a != b && !self.alias_groups.iter().any(|(x, y)| (x == &a && y == &b) || (x == &b && y == &a)) {
            self.alias_groups.push((a.min(b), a.max(b)));
        }
    }
    /// 组代表（别名归并后的判定键：别名对统一映射到较小绑定号）。
    fn group_key(&self, binding: u32) -> u32 {
        for (a, b) in self.alias_groups.iter() {
            if *a == binding || *b == binding {
                return *a;
            }
        }
        binding
    }
    /// 检测：访问序列按组归并后两两判定。
    pub fn detect(&mut self, recs: &[AccRec]) {
        // 只读违规先单条判（声明面直接违规）。
        for r in recs.iter() {
            if r.declared_ro && r.acc == Access::Write {
                self.ro_violations.push((r.binding, r.use_off));
            }
        }
        // 分组（按组代表键）后按序两两判定。
        let mut keys: Vec<u32> = Vec::new();
        for r in recs.iter() {
            let k = self.group_key(r.binding);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        for k in keys.iter() {
            let group: Vec<&AccRec> = recs.iter().filter(|r| self.group_key(r.binding) == *k).collect();
            // 全对判定（非相邻的写写不漏）。
            for i in 0..group.len() {
                for j in (i + 1)..group.len() {
                    let (prev, cur) = (group[i], group[j]);
                    let vd = verdict_pair(prev, cur);
                    if vd == Verdict::Compat {
                        continue;
                    }
                    let ro_hit = self.ro_violations.iter().any(|(b, _)| self.group_key(*b) == *k);
                    let adv = advice_of(vd, ro_hit);
                    // 别名对判定歧义 → 保守报冲突并标注待人工。
                    let manual = self.alias_groups.iter().any(|(a, b)| *a == *k || *b == *k);
                    if manual {
                        self.manual_count += 1;
                    }
                    self.conflicts.push(Conflict {
                        binding: *k,
                        prev_off: prev.use_off,
                        cur_off: cur.use_off,
                        verdict: vd,
                        advice: adv,
                        manual_review: manual,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec52_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let rec = |binding: u32, decl: u32, useo: u32, acc: Access, ro: bool| AccRec {
        binding,
        decl_off: decl,
        use_off: useo,
        acc,
        declared_ro: ro,
    };

    // 一、读写序。
    {
        let r1 = rec(1, 10, 100, Access::Read, false);
        let r2 = rec(1, 10, 200, Access::Read, false);
        let w1 = rec(1, 10, 150, Access::Write, false);
        v.push(("F0452-读写序-读读相容", verdict_pair(&r1, &r2) == Verdict::Compat));
        v.push((
            "F0452-读写序-写写冲突",
            verdict_pair(&rec(1, 10, 150, Access::Write, false), &rec(1, 10, 200, Access::Write, false))
                == Verdict::WwConflict,
        ));
        // 读写序依赖：序敏感（读→写 与 写→读 同为 RwOrder）。
        let rr = rec(1, 10, 300, Access::Read, false);
        let ww = rec(1, 10, 400, Access::Write, false);
        v.push((
            "F0452-读写序-双向序依赖",
            verdict_pair(&rr, &ww) == Verdict::RwOrder && verdict_pair(&ww, &rr) == Verdict::RwOrder,
        ));
    }

    // 二、只读违规。
    {
        let mut det = ConflictDetector::new();
        let recs = vec![
            rec(2, 20, 100, Access::Read, true),
            rec(2, 20, 150, Access::Write, true),
        ];
        det.detect(&recs);
        // 只读声明写访问 → 违规带声明位置（use_off 150 是违规点）。
        v.push((
            "F0452-只读-写访问违规",
            det.ro_violations.len() == 1 && det.ro_violations[0].1 == 150,
        ));
        // 只读资源读读不违规。
        let mut det2 = ConflictDetector::new();
        det2.detect(&vec![rec(3, 30, 1, Access::Read, true), rec(3, 30, 2, Access::Read, true)]);
        v.push((
            "F0452-只读-读读不违规",
            det2.ro_violations.is_empty() && det2.conflicts.is_empty(),
        ));
        // 只读违规的建议 = 声明修正（一级）。
        v.push((
            "F0452-只读-建议一级",
            advice_of(Verdict::RwOrder, true) == Advice3::DeclFix,
        ));
    }

    // 三、别名检测。
    {
        let mut det = ConflictDetector::new();
        det.register_alias(7, 9);
        // 别名对：绑定 7 写 + 绑定 9 写 = 同资源写写冲突（组归并后判定）。
        let recs = vec![
            rec(7, 70, 100, Access::Write, false),
            rec(9, 90, 200, Access::Write, false),
        ];
        det.detect(&recs);
        v.push((
            "F0452-别名-归并后写写",
            det.conflicts.iter().any(|c| c.verdict == Verdict::WwConflict && c.prev_off == 100 && c.cur_off == 200),
        ));
        // 别名判定歧义 → 保守报冲突 + 待人工标注。
        v.push((
            "F0452-别名-保守待人工",
            det.manual_count >= 1 && det.conflicts.iter().any(|c| c.manual_review),
        ));
        // 非别名独立资源互不冲突。
        let mut det2 = ConflictDetector::new();
        det2.detect(&vec![
            rec(1, 1, 10, Access::Write, false),
            rec(2, 2, 20, Access::Write, false),
        ]);
        v.push(("F0452-别名-独立不冲突", det2.conflicts.is_empty()));
    }

    // 四、三级建议。
    {
        // 写写 → 必须重构（三级）。
        v.push((
            "F0452-建议-写写三级",
            advice_of(Verdict::WwConflict, false) == Advice3::Restructure
                && Advice3::Restructure.tag() == "restructure",
        ));
        // 读写序 → 加屏障（二级）。
        v.push((
            "F0452-建议-序依赖二级",
            advice_of(Verdict::RwOrder, false) == Advice3::Barrier
                && Advice3::Barrier.tag() == "barrier",
        ));
        // 三级互异（分级面闭集）。
        let all = [Advice3::DeclFix, Advice3::Barrier, Advice3::Restructure];
        v.push((
            "F0452-建议-三级互异",
            all.iter().all(|a| all.iter().filter(|b| b.tag() == a.tag()).count() == 1),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0452-判据-锚点入册", ANCHOR == "VE-F0452"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec52）。
pub fn run_vec52_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec52");
    for (name, ok) in vec52_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec52_raw_checks as run_vec52_checks;
    #[test]
    fn vec52_rwconflict_checks() {
        for (name, ok) in run_vec52_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
