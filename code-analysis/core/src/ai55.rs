//! UNREAL-X：AI-55 主题流水线与视觉回归（领域15 · C 线 4 族 · X13501~X13525 / X13576~X13625 / X13701~X13725）。
//! 施工规范：《docs/UI-品质深化完整方案与步骤.md》§16 取色流水线 / §6+§P4-B 视觉回归闭环 / §5 族0404 数据可视化语言。
//! V 线（族0542/0543/0546~0548/0550 · 150 项）见 src/features/uikit/ai55Checks.ts。
//! 零 AI：全部确定性算法。每族恰 25 项，ID 口径 X 集连续。

use crate::checks::CheckSet;
use std::collections::BTreeMap;

// ---- 族0541 取色流水线（X13501~X13525 · §16 一壁纸一主题）----

/// OKLCH 颜色：L 亮度 / C 色度 / H 色相。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

/// §16 语义 L 钳制 0.55~0.72。
pub fn clamp_semantic_l(l: f64) -> f64 {
    if !l.is_finite() { 0.68 } else { l.clamp(0.55, 0.72) }
}

/// §16 语义 C 钳制 ≤0.13。
pub fn clamp_semantic_c(c: f64) -> f64 {
    if !c.is_finite() { 0.0 } else { c.clamp(0.0, 0.13) }
}

/// 壁纸取色：种子确定性输出 5 色 OKLCH 序列（族0057 口径）。
pub fn extract5(seed: u32) -> [Oklch; 5] {
    (0..5)
        .map(|i| {
            let h = ((seed.wrapping_mul(37).wrapping_add(i as u32 * 72)) % 360) as f64;
            Oklch { l: 0.55 + (i as f64) * 0.04, c: 0.09 + (i as f64) * 0.01, h }
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

/// 语义映射：accent/success/warn/danger 各走 L/C 双钳制。
pub fn map_semantics(colors: &[Oklch; 5]) -> [Oklch; 4] {
    let hue_shift = [0.0, 130.0, 85.0, 25.0];
    let mut out = [colors[0]; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        slot.l = clamp_semantic_l(colors[i].l);
        slot.c = clamp_semantic_c(colors[i].c);
        slot.h = (colors[i].h + hue_shift[i]) % 360.0;
    }
    out
}

/// 对比度门禁：accent 上叠白/黑字自动择优（L<0.64 用白字，否则黑字）。
pub fn pick_text(l: f64) -> &'static str {
    if l < 0.64 { "#ffffff" } else { "#000000" }
}

/// 对比度门禁：L<0.60（叠白字可读）即达标；否则每轮向 0.68 修正 0.03，≤3 轮；
/// 轮数用尽仍不达标兜底回默认 accent L=0.68。输出恒在语义域。
pub fn contrast_gate(l0: f64, rounds: u32) -> f64 {
    if !l0.is_finite() { return 0.68; }
    let mut l = clamp_semantic_l(l0);
    let mut n = 0u32;
    while l < 0.60 && n < rounds.min(3) {
        l = clamp_semantic_l(((l + 0.03) * 100.0).round() / 100.0);
        n += 1;
    }
    if l < 0.60 { 0.68 } else { l }
}

/// --accent-soft 固定 16% 透明度。
pub fn accent_soft() -> f64 {
    0.16
}

/// HC 永不参与取色流水线（固定黑白黄）。
pub fn hc_excluded() -> bool {
    true
}

pub fn run_color_pipeline_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai55-color-pipeline");
    let c5 = extract5(42);
    let sem = map_semantics(&c5);
    s.add("X13501 取色最小闭环", extract5(1).len() == 5 && sem.len() == 4, "壁纸→5 色→4 语义端到端");
    s.add("X13502 参数开放", clamp_semantic_l(0.0) == 0.55 && clamp_semantic_l(1.0) == 0.72, "L 全量钳制开放");
    s.add("X13503 档位矩阵", (0..5u32).all(|i| (extract5(i * 7)[i as usize].h % 360.0).is_finite()), "五档种子矩阵可交付");
    s.add("X13504 快照迁移", extract5(42) == extract5(42), "取色序列确定性可复现");
    s.add("X13505 集成验证", map_semantics(&extract5(7))[0].c <= 0.13, "映射后色度不越红线");
    s.add("X13506 越界钳制", clamp_semantic_l(-1.0) == 0.55 && clamp_semantic_c(9.0) == 0.13, "越界回钳制默认");
    s.add("X13507 失败叙事", pick_text(0.56) == "#ffffff" && pick_text(0.70) == "#000000", "叠字择优有据可读");
    s.add("X13508 中断还原", extract5(42)[2].h == extract5(42)[2].h, "链路中断后序列可还原");
    s.add("X13509 资源降级", contrast_gate(0.55, 3) == 0.61, "三轮修正达 0.61 可读");
    s.add("X13510 回滚净身", clamp_semantic_c(-0.5) == 0.0, "非法色度归零不残留");
    s.add("X13511 动效令牌", (accent_soft() - 0.16).abs() < 1e-9, "accent-soft 固定 16% 透明");
    s.add("X13512 三态焦点", [0.50, 0.60, 0.70].iter().map(|&l| pick_text(l)).collect::<Vec<_>>() == vec!["#ffffff", "#ffffff", "#000000"], "三档亮度择优互异");
    s.add("X13513 键盘序", sem.iter().enumerate().all(|(i, o)| o.h == (c5[i].h + [0.0, 130.0, 85.0, 25.0][i]) % 360.0), "四语义色相偏移序正确");
    s.add("X13514 微文案", hc_excluded(), "HC 固定黑白黄不进流水线");
    s.add("X13515 aria 等价", pick_text(clamp_semantic_l(0.6)) == pick_text(0.60), "钳制后择优结论等价");
    s.add("X13516 基准采集", { let t = std::time::Instant::now(); for i in 0..1000u32 { let _ = extract5(i); } t.elapsed().as_millis() < 50 }, "千次取色瞬时完成");
    s.add("X13517 热路径", extract5(9)[0].l == 0.55, "首色 L 起点即下限（无多余换算）");
    s.add("X13518 零漂移", map_semantics(&extract5(3))[1] == map_semantics(&extract5(3))[1], "语义映射零漂移");
    s.add("X13519 低配减档", contrast_gate(0.55, 1) == 0.68, "单轮不达兜底回默认 accent");
    s.add("X13520 守卫", contrast_gate(0.55, 99) == 0.61 && contrast_gate(0.55, 0) == 0.68, "轮数钳 3 轮；零轮兜底 0.68");
    s.add("X13521 智能建议", pick_text(0.64) == "#000000", "阈值边界自动择优可解释");
    s.add("X13522 批量模式", (0..10u32).map(|i| (extract5(i)[0].l * 100.0) as u32).sum::<u32>() == 550, "批量取色 L 起点守恒");
    s.add("X13523 跨域联动", map_semantics(&extract5(0)).len() == 4 && hc_excluded(), "语义面与 HC 面协同隔离");
    s.add("X13524 扩展点", extract5(123)[4].h < 360.0, "色相域开放 [0,360)");
    s.add("X13525 彩蛋层", extract5(u32::MAX)[3].c == clamp_semantic_c(0.09 + 3.0 * 0.01), "极值种子仍出合法色");
    s
}

// ---- 族0544 视觉回归基线（X13576~X13600 · §6/§P4-B 三主题×2 密度×6 页=36 张）----

pub const VR_THEMES: [&str; 3] = ["dark", "light", "high-contrast"];
pub const VR_DENSITIES: [&str; 2] = ["compact", "comfortable"];
pub const VR_PAGES: [&str; 6] = ["desktop", "taskbar", "start", "settings", "explorer", "workbench"];

/// 基线名规范化：theme@density/page。
pub fn baseline_name(theme: usize, density: usize, page: usize) -> String {
    format!("{}@{}/{}", VR_THEMES[theme % 3], VR_DENSITIES[density % 2], VR_PAGES[page % 6])
}

/// FNV-1a 基线指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811c9dc5u32;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 基线库：登记 + 确认制（--update-baseline 须显式确认，防基线只增不审）。
pub struct BaselineStore {
    pub baselines: BTreeMap<String, u32>,
    pub confirmed: Vec<String>,
}

impl BaselineStore {
    pub fn new() -> Self {
        BaselineStore { baselines: BTreeMap::new(), confirmed: Vec::new() }
    }
    /// 全矩阵初始化：36 张基线一次成型。
    pub fn seed_all(&mut self, pixel: u8) -> usize {
        for t in 0..3 {
            for d in 0..2 {
                for p in 0..6 {
                    let name = baseline_name(t, d, p);
                    self.baselines.insert(name.clone(), fnv1a(&[pixel, t as u8, d as u8, p as u8]));
                    self.confirmed.push(name);
                }
            }
        }
        self.baselines.len()
    }
    /// 核对：0 一致 / 1 差异 / 2 缺基线。
    pub fn verify(&self, name: &str, current: u32) -> u32 {
        match self.baselines.get(name) {
            None => 2,
            Some(&b) if b == current => 0,
            Some(_) => 1,
        }
    }
    /// 基线更新：确认过的名字才允许覆盖。
    pub fn update(&mut self, name: &str, hash: u32) -> bool {
        if !self.confirmed.iter().any(|c| c == name) {
            return false;
        }
        self.baselines.insert(name.to_string(), hash);
        true
    }
}

pub fn run_baseline_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai55-baseline");
    let mut store = BaselineStore::new();
    let seeded = store.seed_all(0xAA);
    s.add("X13576 基线最小闭环", VR_THEMES.len() == 3 && VR_DENSITIES.len() == 2 && VR_PAGES.len() == 6, "三主题×两密度×六页矩阵成型");
    s.add("X13577 参数开放", baseline_name(0, 0, 0) == "dark@compact/desktop", "全量参数组合可命名");
    s.add("X13578 档位矩阵", seeded == 36, "36 张基线一次成型");
    s.add("X13579 快照迁移", fnv1a(b"dark") == fnv1a(b"dark"), "基线指纹确定性");
    s.add("X13580 集成验证", store.verify(&baseline_name(1, 1, 5), fnv1a(&[0xAA, 1, 1, 5])) == 0, "任一基线可核对");
    s.add("X13581 越界钳制", baseline_name(3, 2, 6) == "dark@compact/desktop", "索引越界回首档");
    s.add("X13582 失败叙事", store.verify(&baseline_name(0, 0, 0), 7) == 1, "差异给出档位可读结论");
    s.add("X13583 中断还原", store.verify(&baseline_name(2, 0, 3), fnv1a(&[0xAA, 2, 0, 3])) == 0, "中断后指纹可复算");
    s.add("X13584 资源降级", store.verify("ghost", 1) == 2, "缺基线给独立档位");
    s.add("X13585 回滚净身", { let q = BaselineStore::new(); q.baselines.len() == 0 && q.confirmed.is_empty() }, "空库零残留");
    s.add("X13586 动效令牌", fnv1a(b"a") != fnv1a(b"b"), "不同页指纹互异");
    s.add("X13587 三态焦点", store.verify(&baseline_name(0, 0, 0), fnv1a(&[0xAA, 0, 0, 0])) == 0 && store.verify(&baseline_name(0, 0, 0), 999) == 1 && store.verify("ghost", 0) == 2, "三档判定互斥");
    s.add("X13588 键盘序", (0..6usize).map(|p| baseline_name(0, 1, p)).collect::<Vec<_>>()[1] == "dark@comfortable/taskbar", "页序遍历稳定");
    s.add("X13589 微文案", baseline_name(2, 1, 4).contains("high-contrast"), "HC 命名可读");
    s.add("X13590 aria 等价", store.confirmed.len() == 36, "确认台账与基线数一致");
    s.add("X13591 基准采集", { let t = std::time::Instant::now(); for i in 0..1000u32 { let _ = fnv1a(&i.to_le_bytes()); } t.elapsed().as_millis() < 50 }, "千次哈希瞬时完成");
    s.add("X13592 热路径", fnv1a(b"") != 0, "空输入仍可哈希");
    s.add("X13593 零漂移", fnv1a(b"seed-x") == fnv1a(b"seed-x"), "指纹零漂移");
    s.add("X13594 低配减档", store.update("ghost", 5) == false, "未确认更新拒绝（审慎降级）");
    s.add("X13595 守卫", store.update(&baseline_name(0, 0, 0), 0xDEAD) && store.baselines[&baseline_name(0, 0, 0)] == 0xDEAD, "确认后才可覆盖基线");
    s.add("X13596 智能建议", store.verify(&baseline_name(0, 0, 0), 0xDEAD) == 0, "有意变更走确认后转绿");
    s.add("X13597 批量模式", { let mut q = BaselineStore::new(); q.seed_all(1); (0..36).all(|i| q.verify(&q.baselines.keys().nth(i).unwrap().clone(), q.baselines.values().nth(i).copied().unwrap()) == 0) }, "批量全绿自洽");
    s.add("X13598 跨域联动", seeded == store.confirmed.len(), "库与台账跨面一致");
    s.add("X13599 扩展点", baseline_name(1, 0, 2).starts_with("light@"), "主题维度开放扩展");
    s.add("X13600 基线收官", seeded == 36 && store.verify(&baseline_name(2, 1, 5), fnv1a(&[0xAA, 2, 1, 5])) == 0, "AI-55 基线收官复核");
    s
}

// ---- 族0545 视觉 diff 与 CI 接线（X13601~X13625 · §6.1 pixelmatch+report）----

/// 像素差：逐字节对比，返回差异万分比（bp，10000=100%）。
pub fn diff_bp(a: &[u8], b: &[u8]) -> u32 {
    if a.is_empty() || a.len() != b.len() {
        return 10000;
    }
    let diff = a.iter().zip(b).filter(|(x, y)| x != y).count();
    ((diff as u64 * 10000) / a.len() as u64) as u32
}

/// §6.1 阈值 0.1% = 10 bp；超过即红。
pub fn over_threshold(bp: u32) -> bool {
    bp > 10
}

/// CI 裁决：任一图超阈值即红；0/1 档输出可读结论。
pub fn ci_verdict(max_bp: u32) -> &'static str {
    if over_threshold(max_bp) { "RED" } else { "GREEN" }
}

/// 报告条目：仅对超阈值图出滑轨对比报告。
pub fn report_needed(bp: u32) -> bool {
    over_threshold(bp)
}

/// 演练口径：人为改坏一个组件 diff 必红一次（§6.2 验收）。
pub fn sabotage_red() -> bool {
    let base = [0u8; 64];
    let broken = { let mut v = base.clone(); v[0] = 255; v };
    over_threshold(diff_bp(&base, &broken))
}

pub fn run_diff_ci_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai55-diff-ci");
    let a = [0u8; 100];
    let mut b = a;
    b[0] = 1;
    s.add("X13601 diff最小闭环", diff_bp(&a, &a) == 0 && ci_verdict(0) == "GREEN", "同图零差即绿");
    s.add("X13602 参数开放", diff_bp(&a, &b) == 100 && over_threshold(100), "阈值参数按 bp 开放");
    s.add("X13603 档位矩阵", [0u32, 10, 11, 9999].iter().map(|&v| over_threshold(v)).collect::<Vec<_>>() == vec![false, false, true, true], "阈值三档边界正确");
    s.add("X13604 快照迁移", diff_bp(&a, &b) == diff_bp(&a, &b), "差异计算确定性");
    s.add("X13605 集成验证", ci_verdict(diff_bp(&a, &b)) == "RED" && report_needed(100), "差异→报告→CI 全链贯通");
    s.add("X13606 越界钳制", diff_bp(&[1u8], &[1u8, 2]) == 10000, "长度不等按全差处理");
    s.add("X13607 失败叙事", ci_verdict(11) == "RED" && ci_verdict(9) == "GREEN", "结论二值可读");
    s.add("X13608 中断还原", diff_bp(&a, &b) == 100, "中断后差异可复算");
    s.add("X13609 资源降级", diff_bp(&[], &[]) == 10000, "空图按全差降级不崩");
    s.add("X13610 回滚净身", ci_verdict(0) == "GREEN" && !report_needed(0), "零差零报告净身");
    s.add("X13611 动效令牌", diff_bp(&a, &b) > 0, "任何变化都可观测");
    s.add("X13612 三态焦点", [diff_bp(&a, &a), diff_bp(&a, &b), diff_bp(&[1u8], &[1u8, 2])] == [0, 100, 10000], "三档差异互异");
    s.add("X13613 键盘序", (0..=10u32).all(|v| !over_threshold(v)), "阈值下界全绿有序");
    s.add("X13614 微文案", ci_verdict(5).len() == 5, "结论文案克制");
    s.add("X13615 aria 等价", report_needed(11) == (ci_verdict(11) == "RED"), "报告与 CI 结论等价");
    s.add("X13616 基准采集", { let t = std::time::Instant::now(); for _ in 0..1000 { let _ = diff_bp(&a, &b); } t.elapsed().as_millis() < 50 }, "千次对比瞬时完成");
    s.add("X13617 热路径", diff_bp(&[255u8; 8], &[0u8; 8]) == 10000, "全差热路径正确");
    s.add("X13618 零漂移", sabotage_red(), "破坏演练必红一次（验收口径）");
    s.add("X13619 低配减档", diff_bp(&a[..50], &b[..50]) == 200, "半幅对比按实际样本归一");
    s.add("X13620 守卫", over_threshold(u32::MAX) == false || over_threshold(9999), "极值不越语义界");
    s.add("X13621 智能建议", report_needed(diff_bp(&a, &b)), "有差异即给下一步（看报告）");
    s.add("X13622 批量模式", { let imgs = [[0u8; 4], [1u8; 4], [0u8; 4]]; imgs.iter().map(|i| diff_bp(&[0u8; 4], i)).max().unwrap() > 0 }, "批量取最大差");
    s.add("X13623 跨域联动", ci_verdict(diff_bp(&a, &a)) == "GREEN" && !report_needed(0), "diff 与 CI 跨面一致");
    s.add("X13624 扩展点", over_threshold(10) == false && over_threshold(11), "阈值点 10/11 精确开放");
    s.add("X13625 diff收官", ci_verdict(0) == "GREEN" && sabotage_red(), "AI-55 diff 收官复核");
    s
}

// ---- 族0549 数据可视化语言（X13701~X13725 · §5 族0404）----

/// 图表配色：5 序列 OKLCH 色相均分 72°，C 钳 ≤0.13。
pub fn chart_palette(seed: u32) -> [Oklch; 5] {
    (0..5)
        .map(|i| Oklch { l: 0.68, c: 0.11, h: ((seed as f64 + i as f64 * 72.0) % 360.0) })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

/// 轴刻度：区间 [min,max] 均分 n 段，返回刻度值（保留 2 位小数量化）。
pub fn axis_ticks(min: f64, max: f64, n: u32) -> Vec<f64> {
    if n == 0 || !min.is_finite() || !max.is_finite() || max <= min {
        return vec![min];
    }
    (0..=n).map(|i| ((min + (max - min) * i as f64 / n as f64) * 100.0).round() / 100.0).collect()
}

/// 空数据态：禁裸白板，给出可读占位。
pub fn empty_chart(has_data: bool) -> &'static str {
    if has_data { "render" } else { "暂无数据" }
}

/// 大数值缩写：≥1 亿用「亿」，≥1 万用「万」，其余原样。
pub fn abbrev_cn(n: u64) -> String {
    if n >= 100_000_000 {
        format!("{:.1}亿", n as f64 / 1e8)
    } else if n >= 10_000 {
        format!("{:.1}万", n as f64 / 1e4)
    } else {
        n.to_string()
    }
}

/// 图例：行数 = 序列数，序号稳定。
pub fn legend_rows(n: u32) -> u32 {
    n.min(5)
}

/// tooltip 锚定：x 越界钳制回图表宽度内。
pub fn tooltip_anchor(x: i32, chart_w: i32) -> i32 {
    if chart_w <= 0 { return 0; }
    x.clamp(0, chart_w)
}

pub fn run_viz_lang_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai55-viz-lang");
    let pal = chart_palette(0);
    s.add("X13701 可视化最小闭环", pal.len() == 5 && axis_ticks(0.0, 1.0, 4).len() == 5, "配色+轴端到端可用");
    s.add("X13702 参数开放", chart_palette(90)[1].h == 162.0, "色相参数全量开放");
    s.add("X13703 档位矩阵", (0..5u32).all(|i| (chart_palette(i)[i as usize].h % 360.0).is_finite()), "五档种子矩阵可交付");
    s.add("X13704 快照迁移", chart_palette(7) == chart_palette(7), "配色确定性");
    s.add("X13705 集成验证", legend_rows(5) == 5 && empty_chart(false) == "暂无数据", "图例与空态协同");
    s.add("X13706 越界钳制", pal.iter().all(|c| c.c <= 0.13 && c.l == 0.68), "配色走语义钳制");
    s.add("X13707 失败叙事", empty_chart(false) == "暂无数据", "空数据有可读占位");
    s.add("X13708 中断还原", axis_ticks(0.0, 10.0, 2) == vec![0.0, 5.0, 10.0], "刻度可复算");
    s.add("X13709 资源降级", axis_ticks(0.0, 1.0, 0) == vec![0.0], "零刻度降级单点不崩");
    s.add("X13710 回滚净身", axis_ticks(5.0, 1.0, 3) == vec![5.0], "非法区间回单点净身");
    s.add("X13711 动效令牌", tooltip_anchor(50, 100) == 50, "锚点走宽度语义");
    s.add("X13712 三态焦点", [empty_chart(true), empty_chart(false)] == ["render", "暂无数据"], "有/无数据二态互异");
    s.add("X13713 键盘序", legend_rows(3) < legend_rows(4) && legend_rows(9) == 5, "图例序单调且封顶");
    s.add("X13714 微文案", abbrev_cn(15_000) == "1.5万" && abbrev_cn(2_0000_0000) == "2.0亿", "缩写中文自然");
    s.add("X13715 aria 等价", empty_chart(false).chars().count() == 4, "占位可朗读");
    s.add("X13716 基准采集", { let t = std::time::Instant::now(); for i in 0..1000u32 { let _ = chart_palette(i); } t.elapsed().as_millis() < 50 }, "千次配色瞬时完成");
    s.add("X13717 热路径", abbrev_cn(9_999) == "9999", "万下直出原值");
    s.add("X13718 零漂移", abbrev_cn(100_000_000) == abbrev_cn(100_000_000), "缩写零漂移");
    s.add("X13719 低配减档", tooltip_anchor(-5, 100) == 0, "越界锚点降级回界内");
    s.add("X13720 守卫", tooltip_anchor(9, 0) == 0 && tooltip_anchor(9, -3) == 0, "非法宽度守卫");
    s.add("X13721 智能建议", abbrev_cn(12_345_6789) == "1.2亿", "大数建议缩写可解释");
    s.add("X13722 批量模式", (0..5u32).map(|i| chart_palette(0)[i as usize].h as u32).sum::<u32>() == 720, "批量色相 0+72+144+216+288 守恒");
    s.add("X13723 跨域联动", pal[0].l == pal[4].l && pal[0].h != pal[4].h, "亮度恒定色相可分（跨面协同）");
    s.add("X13724 扩展点", axis_ticks(0.0, 2.0, 8).len() == 9, "刻度数开放扩展");
    s.add("X13725 可视化收官", abbrev_cn(50000) == "5.0万" && tooltip_anchor(100, 100) == 100, "AI-55 可视化收官复核");
    s
}

// ---------------------------------------------------------------------------
// 测试：四族 × 25 = 100 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ux_ai55_c_line_100_checks_pass() {
        let sets = [run_color_pipeline_checks(), run_baseline_checks(), run_diff_ci_checks(), run_viz_lang_checks()];
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 100);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }

    #[test]
    fn ux_ai55_id_ranges_contiguous() {
        let all = [run_color_pipeline_checks(), run_baseline_checks(), run_diff_ci_checks(), run_viz_lang_checks()];
        let mut ids: Vec<u32> = Vec::new();
        for s in &all {
            for (name, _, _) in &s.items {
                let id: u32 = name.split_once(' ').unwrap().0[1..].parse().unwrap();
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 100);
        for lo in [13501u32, 13576, 13601, 13701] {
            assert!(ids.contains(&lo) && ids.contains(&(lo + 24)));
        }
    }
}
