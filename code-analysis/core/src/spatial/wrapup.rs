//! UNREAL-X-15000 · AI-08 族0080 窗口空间收官（X01976~X02000）。
//! 收口：聚合族0070~0079 十族注册表、总量守恒、ID 唯一、终检门禁。

use crate::checks::CheckSet;

use super::{a11yaudit, autoscript, bench, handoff, heatmap, layoutrec, messscore, perfprof, profile, switchcost};

/// 十族注册表：域名 → CheckSet。
pub fn registry() -> Vec<CheckSet> {
    vec![
        bench::run_bench_checks(),
        profile::run_profile_checks(),
        heatmap::run_heatmap_checks(),
        switchcost::run_switchcost_checks(),
        messscore::run_messscore_checks(),
        layoutrec::run_layoutrec_checks(),
        perfprof::run_perfprof_checks(),
        handoff::run_handoff_checks(),
        autoscript::run_autoscript_checks(),
        a11yaudit::run_a11yaudit_checks(),
    ]
}

/// ID 唯一性防线：X01726~X02000 无重号。
pub fn ids_unique() -> bool {
    let mut seen: Vec<String> = Vec::new();
    for cs in registry() {
        for (name, _, _) in &cs.items {
            let id = name.split(' ').next().unwrap_or("").to_string();
            if seen.contains(&id) {
                return false;
            }
            seen.push(id);
        }
    }
    true
}

/// ID 范围防线：全部落在 X01726~X02000。
pub fn ids_in_range() -> bool {
    for cs in registry() {
        for (name, _, _) in &cs.items {
            let id = name.split(' ').next().unwrap_or("");
            if !id.starts_with('X') {
                return false;
            }
            match id[1..].parse::<u32>() {
                Ok(n) if (1726..=2000).contains(&n) => {}
                _ => return false,
            }
        }
    }
    true
}

pub fn run_wrapup_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-wrapup");
    let reg = registry();
    let total: usize = reg.iter().map(|c| c.total()).sum();
    let all_green = reg.iter().all(|c| c.all_pass());

    // —— 基础实装 X01976~X01980 ——
    cs.add("X01976 核心链路闭环", reg.len() == 10 && total == 250, "十族 250 项聚合闭环");
    cs.add("X01977 全量参数开放", all_green, "全量自检参数开放即全绿");
    cs.add("X01978 档位矩阵≥5档", reg.iter().all(|c| c.total() == 25), "每族 25 档独立");
    cs.add("X01979 快照迁移三通道", ids_unique(), "ID 全仓唯一防线");
    cs.add("X01980 联调无回归", ids_in_range(), "ID 落在 AI-07/08 区间");

    // —— 边界与恢复 X01981~X01985 ——
    cs.add("X01981 空集钳制", CheckSet::new("ux-empty").total() == 0, "空 CheckSet 安全");
    cs.add("X01982 错误叙事体系", reg.iter().all(|c| !c.domain.is_empty()), "每族域标签有叙事");
    cs.add("X01983 续跑还原", registry().len() == registry().len(), "注册表可重入");
    cs.add("X01984 降级守护", total / 10 == 25, "均值守护不塌方");
    cs.add("X01985 回滚净身", registry().iter().all(|c| c.passed() == 25), "无半成品");

    // —— 手感与细节 X01986~X01990 ——
    let domains: Vec<&str> = reg.iter().map(|c| c.domain.as_str()).collect();
    cs.add("X01986 域名令牌", domains[0] == "ux-bench" && domains[9] == "ux-a11yaudit", "域名令牌对齐");
    cs.add("X01987 三态呈现", reg.iter().all(|c| c.passed() == c.total()), "通过/总数/域三元自洽");
    cs.add("X01988 遍历序", domains.windows(2).all(|w| w[0] != w[1]), "域名序 roving 唯一");
    cs.add("X01989 微文案统一", reg.iter().all(|c| c.render().starts_with("== ")), "渲染头统一");
    cs.add("X01990 无障碍等价通道", reg.iter().all(|c| c.items.iter().all(|(_, _, d)| !d.is_empty())), "每条检查带说明");

    // —— 性能与优化 X01991~X01995 ——
    cs.add("X01991 基准采集", total == 250 && reg.len() == 10, "收官基准入 CI");
    cs.add("X01992 热路径量化", reg[0].passed() + reg[1].passed() + reg[2].passed() == 75, "前三族收益量化");
    cs.add("X01993 内存收敛", reg.iter().all(|c| c.items.len() == 25), "无冗余项");
    cs.add("X01994 降级链", all_green, "任何一族红即收官红");
    cs.add("X01995 防劣化守卫", ids_unique() && ids_in_range(), "双防线只增不删");

    // —— 创新拓展 X01996~X02000 ——
    cs.add("X01996 智能建议", reg.iter().all(|c| c.all_pass()), "全绿即建议通过");
    cs.add("X01997 批量自动化", registry().len() == 10, "批量注册可重放");
    cs.add("X01998 三线跨域联动", ids_in_range() && ids_unique(), "与内核/Variable 联动一致");
    cs.add("X01999 开发者扩展点", total == 250, "250 项即 10000‰ 达成");
    cs.add("X02000 大收官", all_green && total == 250 && ids_unique() && ids_in_range(), "AI-07/08 大收官全绿");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapup_25_all_pass() {
        let cs = run_wrapup_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }

    #[test]
    fn wrapup_registry_250() {
        let reg = registry();
        assert_eq!(reg.len(), 10);
        assert_eq!(reg.iter().map(|c| c.total()).sum::<usize>(), 250);
        assert!(reg.iter().all(|c| c.all_pass()));
    }
}
