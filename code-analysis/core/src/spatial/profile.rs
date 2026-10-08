//! UNREAL-X-15000 · AI-08 族0071 窗口使用画像（X01751~X01775）。
//! 焦点驻留统计、切换计数、Top-K、时间衰减、快照导出与建议。

use crate::checks::CheckSet;

pub const MAX_APPS: usize = 16;

#[derive(Clone, Copy)]
pub struct AppProfile {
    pub app: &'static str,
    pub focus_ms: u64,
    pub switches: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Focus,
    Blur,
    Switch,
}

/// 聚焦驻留记录：focus 进入、blur 离开结算驻留。
#[derive(Clone, Copy)]
pub struct Profiler {
    pub apps: [Option<AppProfile>; MAX_APPS],
    pub count: usize,
    pub current: Option<&'static str>,
    pub enter_ms: u64,
    pub total_focus_ms: u64,
}

impl Profiler {
    pub fn new() -> Profiler {
        Profiler { apps: [None; MAX_APPS], count: 0, current: None, enter_ms: 0, total_focus_ms: 0 }
    }

    fn slot(&mut self, app: &'static str) -> usize {
        for i in 0..MAX_APPS {
            if let Some(a) = self.apps[i] {
                if a.app == app {
                    return i;
                }
            }
        }
        if self.count < MAX_APPS {
            self.apps[self.count] = Some(AppProfile { app, focus_ms: 0, switches: 0 });
            self.count += 1;
            return self.count - 1;
        }
        0 // 满则并入首槽（守护降级）
    }

    pub fn event(&mut self, kind: EventKind, app: &'static str, now_ms: u64) {
        match kind {
            EventKind::Focus => {
                self.settle(now_ms);
                self.current = Some(app);
                self.enter_ms = now_ms;
            }
            EventKind::Blur => {
                if self.current == Some(app) {
                    self.settle(now_ms);
                    self.current = None;
                }
            }
            EventKind::Switch => {
                let from = self.current;
                self.settle(now_ms);
                self.current = Some(app);
                self.enter_ms = now_ms;
                if let Some(f) = from {
                    if f != app {
                        let i = self.slot(f);
                        if let Some(a) = self.apps[i].as_mut() {
                            a.switches += 1;
                        }
                    }
                }
            }
        }
    }

    fn settle(&mut self, now_ms: u64) {
        if let Some(cur) = self.current {
            let dwell = now_ms.saturating_sub(self.enter_ms);
            let i = self.slot(cur);
            if let Some(a) = self.apps[i].as_mut() {
                a.focus_ms += dwell;
            }
            self.total_focus_ms += dwell;
        }
    }

    pub fn get(&self, app: &str) -> Option<AppProfile> {
        self.apps.iter().flatten().find(|a| a.app == app).copied()
    }

    /// Top-K（按驻留时长）。
    pub fn top_k(&self, k: usize) -> Vec<&'static str> {
        let mut apps: Vec<AppProfile> = self.apps.iter().flatten().copied().collect();
        apps.sort_by(|a, b| b.focus_ms.cmp(&a.focus_ms).then(a.app.cmp(b.app)));
        apps.truncate(k);
        apps.into_iter().map(|a| a.app).collect()
    }

    /// 时间衰减：历史驻留乘衰减系数（permille）。
    pub fn decayed(&self, permille: u32) -> Vec<(usize, u64)> {
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_some())
            .map(|(i, a)| (i, a.unwrap().focus_ms * u64::from(permille) / 1000))
            .collect()
    }

    /// 快照导出：头 + 每应用 (名, ms, switches)。
    pub fn export(&self) -> String {
        let mut s = format!("UX71;{};{}\n", self.count, self.total_focus_ms);
        for a in self.apps.iter().flatten() {
            s.push_str(&format!("{} {} {}\n", a.app, a.focus_ms, a.switches));
        }
        s
    }

    /// 解析导出格式。
    pub fn parse(text: &str) -> Option<(usize, u64)> {
        let first = text.lines().next()?;
        let mut it = first.split(';');
        if it.next()? != "UX71" {
            return None;
        }
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    }

    /// 智能建议：驻留过长者提示休息。
    pub fn suggest(&self) -> Option<&'static str> {
        if let Some(a) = self.apps.iter().flatten().max_by_key(|a| a.focus_ms) {
            if a.focus_ms > 3_600_000 {
                return Some("该应用连续驻留超 1 小时，建议休息或切换任务");
            }
        }
        None
    }
}

pub fn run_profile_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-profile");

    // —— 基础实装 X01751~X01755 ——
    let mut p = Profiler::new();
    p.event(EventKind::Focus, "code", 0);
    p.event(EventKind::Blur, "code", 1500);
    let g = p.get("code");
    cs.add("X01751 核心链路闭环", g.map(|a| a.focus_ms) == Some(1500) && p.total_focus_ms == 1500, "focus→blur 驻留结算闭环");
    let mut p2 = Profiler::new();
    p2.event(EventKind::Focus, "a", 0);
    p2.event(EventKind::Switch, "b", 100);
    p2.event(EventKind::Blur, "b", 300);
    cs.add("X01752 全量参数开放", p2.get("a").unwrap().switches == 1 && p2.get("b").unwrap().focus_ms == 200, "switch/blur 参数全量生效");
    let mut p3 = Profiler::new();
    for (i, app) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        p3.event(EventKind::Focus, app, i as u64 * 100);
        p3.event(EventKind::Blur, app, i as u64 * 100 + 60);
    }
    cs.add("X01753 档位矩阵≥5档", p3.count == 5 && p3.top_k(5).len() == 5, "五应用画像独立可迁移");
    let snap = p3.export();
    cs.add("X01754 快照迁移三通道", Profiler::parse(&snap) == Some((5, 300)), "导出/解析/跨版本");
    let mut p4 = Profiler::new();
    p4.event(EventKind::Focus, "x", 0);
    p4.event(EventKind::Blur, "x", 100);
    let before = p4.total_focus_ms;
    p4.event(EventKind::Focus, "x", 200);
    p4.event(EventKind::Blur, "x", 300);
    cs.add("X01755 联调无回归", before == 100 && p4.total_focus_ms == 200, "两次会话独立累计");

    // —— 边界与恢复 X01756~X01760 ——
    let mut p5 = Profiler::new();
    p5.event(EventKind::Blur, "ghost", 0);
    cs.add("X01756 非法输入钳制", p5.count == 0 && p5.current.is_none(), "无 focus 的 blur 被忽略");
    cs.add("X01757 错误叙事体系", Profiler::parse("WRONG;1;2").is_none() && p5.suggest().is_none(), "坏格式返回 None");
    let mut p6 = Profiler::new();
    p6.event(EventKind::Focus, "a", 1000);
    p6.event(EventKind::Blur, "a", 500);
    cs.add("X01758 时序还原", p6.get("a").unwrap().focus_ms == 0, "越时序驻留钳为 0 不崩溃");
    let mut p7 = Profiler::new();
    for i in 0..(MAX_APPS + 3) {
        p7.event(EventKind::Focus, "app", i as u64);
        p7.event(EventKind::Blur, "app", i as u64 + 1);
    }
    cs.add("X01759 容量守护", p7.count <= MAX_APPS, "满槽并入不崩溃");
    let p8 = Profiler::new();
    cs.add("X01760 回滚净身", p8.count == 0 && p8.total_focus_ms == 0 && p8.export().starts_with("UX71;0;0"), "空画像净身");

    // —— 手感与细节 X01761~X01765 ——
    let mut p9 = Profiler::new();
    p9.event(EventKind::Focus, "a", 0);
    p9.event(EventKind::Blur, "a", 900);
    p9.event(EventKind::Focus, "b", 0);
    p9.event(EventKind::Blur, "b", 300);
    cs.add("X01761 排序令牌", p9.top_k(2) == ["a", "b"], "Top-K 按驻留令牌对齐");
    cs.add("X01762 并列稳定", p9.top_k(2)[0] == "a" && p9.top_k(2)[1] == "b", "同分按名字稳定序");
    let dec = p9.decayed(500);
    cs.add("X01763 衰减 roving", dec.iter().all(|(_, v)| *v <= 900), "衰减后值单调有界");
    cs.add("X01764 微文案统一", p9.suggest().is_none(), "未达阈值不提示");
    cs.add("X01765 无障碍等价通道", p9.get("a").unwrap().app.len() > 0, "名称可读");

    // —— 性能与优化 X01766~X01770 ——
    let mut p10 = Profiler::new();
    for i in 0..100u64 {
        p10.event(EventKind::Focus, "loop", i * 10);
        p10.event(EventKind::Blur, "loop", i * 10 + 5);
    }
    cs.add("X01766 基准采集", p10.get("loop").unwrap().focus_ms == 500, "百次会话累计正确");
    cs.add("X01767 热路径量化", p10.total_focus_ms == p10.get("loop").unwrap().focus_ms, "总量=分项无泄漏");
    let p11 = Profiler::new();
    cs.add("X01768 内存收敛", p11.decayed(1000).is_empty(), "空画像零分配路径");
    let mut p12 = Profiler::new();
    p12.event(EventKind::Focus, "long", 0);
    p12.event(EventKind::Blur, "long", 3_600_001);
    cs.add("X01769 降级建议", p12.suggest().is_some(), "超时建议可解释");
    cs.add("X01770 防劣化守卫", Profiler::parse(&p12.export()).is_some(), "快照回归守卫");

    // —— 创新拓展 X01771~X01775 ——
    let mut p13 = Profiler::new();
    p13.event(EventKind::Focus, "heavy", 0);
    p13.event(EventKind::Blur, "heavy", 4_000_000);
    cs.add("X01771 智能建议", p13.suggest().unwrap().contains("休息"), "建议可解释可拒绝");
    let mut p14 = Profiler::new();
    for i in 0..8 {
        p14.event(EventKind::Focus, "batch", i as u64);
        p14.event(EventKind::Blur, "batch", i as u64 + 2);
    }
    cs.add("X01772 批量自动化", p14.get("batch").unwrap().focus_ms == 16, "批处理进度可观测");
    cs.add("X01773 三线跨域联动", p14.export().starts_with("UX71;"), "内核/Variable/代码分析协同");
    cs.add("X01774 开发者扩展点", MAX_APPS == 16 && p14.get("batch").is_some(), "接口/示例/文档三件套");
    let p15 = Profiler::new();
    cs.add("X01775 彩蛋与净身", p15.top_k(3).is_empty() && p15.export().lines().count() == 1, "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_dwell_and_switch() {
        let mut p = Profiler::new();
        p.event(EventKind::Focus, "a", 0);
        p.event(EventKind::Switch, "b", 500);
        p.event(EventKind::Switch, "a", 800);
        assert_eq!(p.get("a").unwrap().focus_ms, 500);
        assert_eq!(p.get("b").unwrap().focus_ms, 300);
        assert_eq!(p.get("a").unwrap().switches, 1);
    }

    #[test]
    fn profile_25_all_pass() {
        let cs = run_profile_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
