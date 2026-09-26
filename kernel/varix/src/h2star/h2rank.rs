//! H2 域排序评级引擎 · 深化批次一（决策层纵深——骨架→器官）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F257 打开方式选择器**：选择器排序判据——可处理该格式的应用按
//!   「默认关联 > 兼容评级（星图 A2 判例工厂供数）> 最近使用」排序；
//!   「始终使用」勾选才写关联表（不勾不写）；关联变更可追溯（审计位）；
//! - **F274 「所有应用」列表**：中英混排按拼音/字母**二序列**排序；
//!   索引条按字母跳段（首应用可见）；
//! - **F299 开始菜单推荐区**：三来源混合（最近安装 / 近期文件 /
//!   推荐打开方式），× 掉的同源项降权，Top6 数量稳定，广告位=0
//!   （数据源枚举无第四成员——结构保证）；
//! - **F291 耗电排行**：Top10，异常判定（高于同应用历史均值 3 倍），
//!   归因文案覆盖率 100%（Top10 每条有说法），只展示不越权。
//!
//! 排序纪律：所有排序**稳定**（同键保持插入序——分组稳定性与推荐区
//! 「数量稳定」的机制保证）；全引擎纯函数，注入偏好，不读全局状态。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F257 打开方式评级排序
// ---------------------------------------------------------------------------

/// 兼容评级（星图 A2 判例工厂五档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompatGrade {
    /// 判例全绿。
    Gold = 4,
    /// 主功能绿。
    Silver = 3,
    /// 可用有瑕疵。
    Bronze = 2,
    /// 未判例。
    Unknown = 1,
    /// 判例红。
    Poor = 0,
}

/// 选择器里的一个候选应用。
#[derive(Clone, Debug)]
pub struct OpenWithCandidate {
    pub app: String,
    pub grade: CompatGrade,
    /// 最近一次用该应用打开此格式的分钟戳（None=从未）。
    pub last_used_min: Option<u64>,
    /// 当前默认关联（每格式至多一个）。
    pub is_default: bool,
}

/// 排序键：默认关联永远第一；其后评级降序、最近使用降序、未用垫底；
/// 全同键保持输入序（稳定排序——判据「排序规则入册」的可复现保证）。
pub fn rank_open_with(cands: &[OpenWithCandidate], now_min: u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..cands.len()).collect();
    idx.sort_by(|&a, &b| {
        let (x, y) = (&cands[a], &cands[b]);
        y.is_default
            .cmp(&x.is_default)
            .then_with(|| y.grade.cmp(&x.grade))
            .then_with(|| {
                // 最近使用降序：从未使用视为最旧（0 时刻），同刻保序。
                let lx = x.last_used_min.unwrap_or(0);
                let ly = y.last_used_min.unwrap_or(0);
                // now_min 距离越近越前——直接比时间戳降序即可（时间不倒流）。
                ly.cmp(&lx)
            })
            .then_with(|| a.cmp(&b))
    });
    idx
}

/// 「始终使用」勾选写入：返回新关联表（每格式一条审计记录——
/// 「不勾不写」由调用契约保证：未勾选时本函数不被调用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssocAudit {
    pub ext: String,
    pub app: String,
    pub at_min: u64,
}

// ---------------------------------------------------------------------------
// F274 「所有应用」混排二序列
// ---------------------------------------------------------------------------

/// 一个「所有应用」条目。
#[derive(Clone, Debug)]
pub struct AppEntry {
    /// 显示名。
    pub name: String,
    /// 排序键第一序列：中文取拼音首字（系统拼音引擎 F326 供数注入，
    /// 非中文字符取自身小写——「二序列」规则的单一定义点）。
    pub sort_key: String,
    pub is_system: bool,
}

/// 混排排序：`sort_key` 升序（A-Z / 拼音序同流），同键比显示名
/// （字素级——大小写不敏感），仍同比插入序。系统组件照常在列。
pub fn sort_all_apps(entries: &[AppEntry]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        x.sort_key.cmp(&y.sort_key).then_with(|| {
            x.name.to_lowercase().cmp(&y.name.to_lowercase()).then_with(|| a.cmp(&b))
        })
    });
    idx
}

/// 索引条：字母 → 排序后列表中该字母**首个**条目的位置（判据
/// 「索引跳转精度（首应用可见）」）。无该字母时返回 None（条目置灰）。
pub fn index_jumps(entries: &[AppEntry], order: &[usize]) -> Vec<(char, usize)> {
    let mut out: Vec<(char, usize)> = Vec::new();
    for (pos, &i) in order.iter().enumerate() {
        let key = entries[i].sort_key.clone();
        let first = key.chars().next().unwrap_or('#');
        let letter = first.to_ascii_uppercase();
        if !out.iter().any(|(c, _)| *c == letter) {
            out.push((letter, pos));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// F299 推荐区三来源混合
// ---------------------------------------------------------------------------

/// 推荐来源（枚举无第四成员——「广告位=0」的结构保证：想塞广告
/// 编译不过）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoSource {
    /// 最近安装（徽标 7 天）。
    FreshInstall,
    /// 近期文件（F072 引擎 Top6 跨应用聚合）。
    RecentFile,
    /// 推荐打开方式（上下文智能——插 U 盘推「打开 S:」）。
    ContextHint,
}

impl RecoSource {
    /// 混合权重（排序规则入册的唯一源）：
    /// 上下文提示 > 近期文件 > 最近安装（「接着干活」优先于营销位）。
    fn weight(self) -> u8 {
        match self {
            RecoSource::ContextHint => 3,
            RecoSource::RecentFile => 2,
            RecoSource::FreshInstall => 1,
        }
    }
}

/// 一条推荐。
#[derive(Clone, Debug)]
pub struct RecoItem {
    pub source: RecoSource,
    pub label: String,
    /// 来源内新鲜度分钟戳（越大越新——同来源内降序）。
    pub fresh_min: u64,
    /// 新装是否仍在 7 天徽标期内（渲染层徽标位）。
    pub badged: bool,
    /// 用户 × 掉的降权（持久化态注入）。
    pub dismissed: bool,
}

/// 混合排序：来源权重降序 → 新鲜度降序 → ×掉项沉底 → 稳定序。
/// `top_n` 截取（Top6 判据）——截断前先排定，保「数量稳定」。
pub fn blend_reco(items: &[RecoItem], top_n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    idx.sort_by(|&a, &b| {
        let (x, y) = (&items[a], &items[b]);
        x.dismissed
            .cmp(&y.dismissed)
            .then_with(|| y.source.weight().cmp(&x.source.weight()))
            .then_with(|| y.fresh_min.cmp(&x.fresh_min))
            .then_with(|| a.cmp(&b))
    });
    idx.truncate(top_n);
    idx
}

/// 徽标判定：装于 `installed_min`，徽标期 `days`（主册 7 天）。
pub fn fresh_badge(now_min: u64, installed_min: u64, days: u64) -> bool {
    now_min.saturating_sub(installed_min) < days * 1440
}

// ---------------------------------------------------------------------------
// F291 耗电排行
// ---------------------------------------------------------------------------

/// 一行耗电账（对账 F060 电量账本——此处只做用户面呈现判定）。
#[derive(Clone, Debug)]
pub struct PowerRow {
    pub app: String,
    /// 当前功率（mW）。
    pub mw_now: u32,
    /// 近 1 小时累计（mWh）。
    pub mwh_1h: u32,
    /// 同应用历史均值（mWh）——异常判定的基准。
    pub mwh_hist_avg: u32,
    /// 归因文案（Top10 每条必须有——None 判红）。
    pub attribution: Option<&'static str>,
}

/// 排行：mWh 降序 TopN；异常行标黄（≥3×历史均值——判据定值）。
/// 返回 (行索引, 异常标记)——只展示，不携带任何杀进程语义。
pub fn rank_power(rows: &[PowerRow], top_n: usize) -> Vec<(usize, bool)> {
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    idx.sort_by(|&a, &b| {
        rows[b]
            .mwh_1h
            .cmp(&rows[a].mwh_1h)
            .then_with(|| a.cmp(&b))
    });
    idx.truncate(top_n);
    idx.iter()
        .map(|&i| {
            let r = &rows[i];
            let anomalous = r.mwh_hist_avg > 0 && r.mwh_1h >= r.mwh_hist_avg * 3;
            (i, anomalous)
        })
        .collect()
}

/// 归因覆盖率审计：TopN 里每行都有归因文案（判据「覆盖率 Top10 每条
/// 有说法」的机制实现——少一条即红，不许静默缺文案）。
pub fn attribution_coverage(rows: &[PowerRow], ranked: &[(usize, bool)]) -> bool {
    ranked.iter().all(|(i, _)| rows[*i].attribution.is_some())
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2rank_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2rank");
    // --- F257 排序：默认第一 > 评级 > 最近使用；稳定序。 ---
    let cands = alloc::vec![
        OpenWithCandidate { app: String::from("丙·金"), grade: CompatGrade::Gold, last_used_min: Some(100), is_default: false },
        OpenWithCandidate { app: String::from("甲·默认"), grade: CompatGrade::Bronze, last_used_min: Some(10), is_default: true },
        OpenWithCandidate { app: String::from("乙·银"), grade: CompatGrade::Silver, last_used_min: Some(500), is_default: false },
    ];
    let ranked = rank_open_with(&cands, 600);
    set.add(
        "h2rank F257 default first",
        cands[ranked[0]].is_default
            && cands[ranked[1]].grade == CompatGrade::Gold
            && cands[ranked[2]].grade == CompatGrade::Silver,
        "default>grade>recency",
    );
    // 未知评级垫在银后金前？——不：金(4)>银(3)>未知(1)>铜？错序检查：
    // Bronze=2 > Unknown=1，显式钉住。
    let mix = alloc::vec![
        OpenWithCandidate { app: String::from("u"), grade: CompatGrade::Unknown, last_used_min: None, is_default: false },
        OpenWithCandidate { app: String::from("b"), grade: CompatGrade::Bronze, last_used_min: None, is_default: false },
    ];
    let r2 = rank_open_with(&mix, 0);
    set.add("h2rank F257 bronze>unknown", mix[r2[0]].grade == CompatGrade::Bronze, "grade order");
    // --- F274 二序列：拼音键注入后中英同流；索引跳段。 ---
    let apps = alloc::vec![
        AppEntry { name: String::from("记事本"), sort_key: String::from("jishiben"), is_system: true },
        AppEntry { name: String::from("Edge"), sort_key: String::from("edge"), is_system: false },
        AppEntry { name: String::from("计算器"), sort_key: String::from("jisuanqi"), is_system: true },
        AppEntry { name: String::from("终端"), sort_key: String::from("zhongduan"), is_system: true },
    ];
    let order = sort_all_apps(&apps);
    set.add(
        "h2rank F274 mixed sort",
        apps[order[0]].name == "Edge"
            && apps[order[1]].name == "记事本"
            && apps[order[2]].name == "计算器",
        // 拼音键 jishiben(记) < jisuanqi(计)——字母序 h<u，记事本在前。
        "pinyin+latin one lane",
    );
    let jumps = index_jumps(&apps, &order);
    let edge_pos = jumps.iter().find(|(c, _)| *c == 'E').map(|(_, p)| *p);
    set.add("h2rank F274 index first visible", edge_pos == Some(0), "E→first");
    // --- F299 三来源混合：上下文 > 近期 > 新装；× 沉底；Top6 稳定。 ---
    let recos = alloc::vec![
        RecoItem { source: RecoSource::FreshInstall, label: String::from("新装A"), fresh_min: 900, badged: true, dismissed: false },
        RecoItem { source: RecoSource::RecentFile, label: String::from("文件B"), fresh_min: 800, badged: false, dismissed: false },
        RecoItem { source: RecoSource::ContextHint, label: String::from("打开S:"), fresh_min: 100, badged: false, dismissed: false },
        RecoItem { source: RecoSource::RecentFile, label: String::from("文件C"), fresh_min: 850, badged: false, dismissed: false },
        RecoItem { source: RecoSource::RecentFile, label: String::from("文件D"), fresh_min: 700, badged: false, dismissed: false },
        RecoItem { source: RecoSource::RecentFile, label: String::from("文件E"), fresh_min: 600, badged: false, dismissed: false },
        RecoItem { source: RecoSource::FreshInstall, label: String::from("新装F"), fresh_min: 880, badged: true, dismissed: false },
        RecoItem { source: RecoSource::RecentFile, label: String::from("文件G"), fresh_min: 500, badged: false, dismissed: true },
    ];
    let top6 = blend_reco(&recos, 6);
    set.add("h2rank F299 top6 size", top6.len() == 6, "stable size");
    set.add(
        "h2rank F299 context first",
        recos[top6[0]].source == RecoSource::ContextHint,
        "context > files > fresh",
    );
    set.add(
        "h2rank F299 dismissed sinks",
        !top6.iter().any(|&i| recos[i].dismissed),
        "x-ed sinks",
    );
    set.add(
        "h2rank F299 fresh after files",
        recos[top6[top6.iter().position(|&i| recos[i].source == RecoSource::FreshInstall).unwrap()]]
            .fresh_min
            == 900,
        // 两条新装同权重——新鲜度降序，900(新装A) 先见。
        "newer install first",
    );
    // 7 天徽标：6 天内 true、8 天外 false。
    set.add(
        "h2rank F299 badge window",
        fresh_badge(6 * 1440, 0, 7) && !fresh_badge(8 * 1440, 0, 7),
        "7d window",
    );
    // --- F291 耗电排行：3 倍异常、TopN、归因覆盖。 ---
    let rows = alloc::vec![
        PowerRow { app: String::from("A"), mw_now: 3_000, mwh_1h: 900, mwh_hist_avg: 100, attribution: Some("后台持续联网——可在应用内关闭同步") },
        PowerRow { app: String::from("B"), mw_now: 1_000, mwh_1h: 300, mwh_hist_avg: 100, attribution: Some("正常范围") },
        PowerRow { app: String::from("C"), mw_now: 2_000, mwh_1h: 600, mwh_hist_avg: 0, attribution: Some("新应用无历史基准") },
    ];
    let pr = rank_power(&rows, 10);
    set.add(
        "h2rank F291 order+anomaly",
        pr[0].0 == 0 && pr[0].1 && !pr[1].1,
        "3x flag",
    );
    set.add("h2rank F291 coverage", attribution_coverage(&rows, &pr), "top10 all said");
    // 零历史基准（hist=0）不判异常（防除零误报——诚实）。
    let c_idx = pr.iter().find(|(i, _)| *i == 2).unwrap();
    set.add("h2rank F291 no-hist honest", !c_idx.1, "no false flag");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2rank_all_green() {
        let set = run_h2rank_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2rank 自检红 {f}/{p}");
    }

    #[test]
    fn sorts_are_stable() {
        // 全同键保持输入序——稳定性的回归锚（排序不许翻供）。
        let apps = alloc::vec![
            AppEntry { name: String::from("同键1"), sort_key: String::from("same"), is_system: false },
            AppEntry { name: String::from("同键2"), sort_key: String::from("same"), is_system: false },
            AppEntry { name: String::from("同键3"), sort_key: String::from("same"), is_system: false },
        ];
        let order = sort_all_apps(&apps);
        assert_eq!(order, alloc::vec![0, 1, 2]);
    }

    #[test]
    fn reco_never_exceeds_top_n() {
        let many: Vec<RecoItem> = (0..50)
            .map(|i| RecoItem {
                source: RecoSource::RecentFile,
                label: alloc::format!("f{i}"),
                fresh_min: i as u64,
                badged: false,
                dismissed: false,
            })
            .collect();
        assert_eq!(blend_reco(&many, 6).len(), 6);
    }
}
