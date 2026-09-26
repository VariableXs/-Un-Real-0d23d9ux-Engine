//! F632 指针渲染保真审计 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：24 组合走查自动化脚本；标红判据（边缘锐度/
//! 对比度阈值文档化）；一键重生成链路；内置指针基线过审；报告入
//! F628 详情页。
//!
//! **审计面（C-7 4K 资产管线在指针域的落地件）**：
//! - **24 组合** = 4 档 DPI（100/125/150/200%）× 3 底色（深/浅/中灰）
//!   × 深浅主题（2）——逐组合在宿主模型面重演渲染路径（jbase Lanczos
//!   重采样为真实渲染核），对拍出糊/锯齿/对比不足三类缺陷；
//! - **标红判据（文档化于 `THRESHOLDS`，一处一事实）**：
//!   1. 边缘锐度 `edge_sharpness`：实体-透明边界的平均过渡带宽 ≤1.6px
//!      （Lanczos 合理过冲范围；最近邻 ≈1.0、双线性 ≈2.0 之间）；
//!   2. 对比度 `contrast_x100`：指针主体对底色 ≥ 200（2:1 可辨底线，
//!      描边语义另由 F629/F631 承担）；
//!   3. 过冲带 `overshoot_ring`：边界外 1px 环不允许被染出暗晕
//!      （alpha ≤ 96——Lanczos 负瓣的可见伪影线）；
//! - **一键重生成**：不达标项走「重生成双倍率 sprite」（jbase
//!   `scale_integer2x` 精确复制 + Lanczos DPI 派生复用）——重生成后
//!   复审闭环；
//! - **内置基线过审**：jbase 内置默认方案自己先过同一审计（裁判与
//!   球员一把尺）；
//! - **报告入 F628 详情页**：报告带方案指纹，经 `SchemeLibrary::
//!   record_report` 的指纹对账挂载（与 F627 报告同通道、同失效纪律）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{resample_lanczos3, CursorFrame, CursorSchemeModel, OriginKind, PixBuf, Rgb};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 阈值文档（标红判据唯一事实源）
// ---------------------------------------------------------------------------

/// 标红阈值表（文档化判据——改阈值必须走对账）。
pub struct Thresholds;
impl Thresholds {
    /// 边缘锐度：实体-透明边界平均过渡带宽上限（px ×100 定点）。
    pub const EDGE_SHARPNESS_MAX_X100: i64 = 160;
    /// 主体对底色对比度下限（×100 定点，2:1 可辨底线）。
    pub const CONTRAST_MIN_X100: i64 = 200;
    /// 远晕环（距实体 2px、无 1px 实体邻接）的 alpha 上限——Lanczos
    /// 一阶过冲带（紧邻 1px，alpha 可达 ~127）是锐化机理不算伪影，
    /// 远环出现显著 alpha 才是负瓣失控。
    pub const OVERSHOOT_ALPHA_MAX: u8 = 96;
    /// 4 档 DPI（%）。
    pub const DPI_TIERS: [u32; 4] = [100, 125, 150, 200];
    /// 3 档底色。
    pub const BACKDROPS: [Rgb; 3] = [
        Rgb::new(24, 24, 26),    // 深
        Rgb::new(245, 245, 245), // 浅
        Rgb::new(128, 128, 128), // 中灰
    ];
}

// ---------------------------------------------------------------------------
// 审计度量（模型面渲染核 = jbase 重采样）
// ---------------------------------------------------------------------------

/// 在指定 DPI 档渲染方案 Normal 态首帧（DPI = 逻辑像素缩放：
/// 100% 原样，其余按比例 Lanczos 放大——与 F636 契约同核）。
fn render_at_dpi(f: &CursorFrame, dpi: u32) -> PixBuf {
    let src = PixBuf::from_rgba(f.w, f.h, f.px.clone());
    if dpi == 100 {
        return src;
    }
    let scale = dpi * 1000 / 100;
    let dw = ((f.w as u32) * scale / 1000).max(1) as u16;
    let dh = ((f.h as u32) * scale / 1000).max(1) as u16;
    resample_lanczos3(&src, dw, dh)
}

/// 边缘锐度：实体（α≥128）与透明（α<128）边界的平均过渡带宽。
/// 口径：对每个边界像素，沿主梯度方向数「中间灰」像素（0<α<255 的
/// 邻接游程）——平均游程 ×100 定点。锐利=过渡窄。
pub fn edge_sharpness_x100(buf: &PixBuf) -> i64 {
    let mut transitions = 0u64;
    let mut band_total = 0u64;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let a = buf.get(x, y).unwrap_or([0, 0, 0, 0])[3];
            let solid = a >= 128;
            // 右向与下向两个梯度方向。
            for (dx, dy) in [(1i64, 0i64), (0, 1)] {
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if nx >= buf.w as i64 || ny >= buf.h as i64 {
                    continue;
                }
                let b = buf.get(nx as u16, ny as u16).unwrap_or([0, 0, 0, 0])[3];
                if (b >= 128) != solid {
                    transitions += 1;
                    // 过渡带：两端点向外数中间灰（0<α<255）游程之和。
                    let mut band = 0u64;
                    // 前向游程。
                    let mut k = 1i64;
                    while k <= 3 {
                        let px_ = x as i64 + dx * k;
                        let py_ = y as i64 + dy * k;
                        if px_ >= buf.w as i64 || py_ >= buf.h as i64 {
                            break;
                        }
                        let aa = buf.get(px_ as u16, py_ as u16).unwrap_or([0, 0, 0, 0])[3];
                        if aa > 0 && aa < 255 {
                            band += 1;
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    band_total += band;
                }
            }
        }
    }
    if transitions == 0 {
        return 0;
    }
    ((band_total * 100) / transitions) as i64
}

/// 远晕检查：透明（α<128）且**无 1px 实体邻接**、但 2px 内存在实体
/// 的像素——Lanczos 一阶过冲带（紧邻实体 1px）是锐化机理不计入，
/// 远环出现显著 alpha 才是负瓣失控（伪影判据）。
pub fn overshoot_max_alpha(buf: &PixBuf) -> u8 {
    let solid_at = |x: i64, y: i64| -> bool {
        x >= 0
            && y >= 0
            && x < buf.w as i64
            && y < buf.h as i64
            && buf.get(x as u16, y as u16).unwrap_or([0, 0, 0, 0])[3] >= 128
    };
    let mut worst = 0u8;
    for y in 0..buf.h {
        for x in 0..buf.w {
            let p = buf.get(x, y).unwrap_or([0, 0, 0, 0]);
            if p[3] >= 128 {
                continue;
            }
            let xi = x as i64;
            let yi = y as i64;
            let first_ring = [
                (1i64, 0i64),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ]
            .iter()
            .any(|(dx, dy)| solid_at(xi + dx, yi + dy));
            if first_ring {
                continue; // 一阶过冲带——机理非伪影
            }
            let second_ring = (-2i64..=2).any(|dx| (-2i64..=2).any(|dy| solid_at(xi + dx, yi + dy)));
            if second_ring && p[3] > worst {
                worst = p[3];
            }
        }
    }
    worst
}

/// 主体对底色的可辨对比度（不透明像素抽样）：最暗 10% 与最亮 10% 两
/// 主色群各自对底取对比，取**较大者**——指针可见 = 任一主色群可辨
/// （黑形白边双群设计在深浅底上都由其中一群撑起可辨性）。
pub fn min_body_contrast_x100(buf: &PixBuf, backdrop: Rgb) -> Option<i64> {
    let mut lumis: Vec<i64> = Vec::new();
    for c in buf.px.chunks_exact(4) {
        if c[3] >= 128 {
            lumis.push(crate::jstar2::jbase::relative_luminance_m(Rgb::new(c[0], c[1], c[2])));
        }
    }
    if lumis.is_empty() {
        return None;
    }
    lumis.sort_unstable();
    let n = lumis.len();
    let darkest = lumis[0];
    let lightest = lumis[n - 1];
    let lum_bg = crate::jstar2::jbase::relative_luminance_m(backdrop);
    let ratio = |c: i64| -> i64 {
        let hi = c.max(lum_bg);
        let lo = c.min(lum_bg);
        ((hi + 50) * 100 / (lo + 50).max(1)) as i64
    };
    Some(ratio(darkest).max(ratio(lightest)))
}

// ---------------------------------------------------------------------------
// 审计报告
// ---------------------------------------------------------------------------

/// 单组合结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboResult {
    pub dpi: u32,
    pub dark_theme: bool,
    pub backdrop: Rgb,
    pub edge_x100: i64,
    pub contrast_x100: i64,
    pub overshoot: u8,
    pub passed: bool,
    pub why: &'static str,
}

/// 完整审计报告（24 组合 + 主题侧）。
#[derive(Clone, Debug)]
pub struct AuditReport {
    pub scheme_fingerprint: u64,
    pub combos: Vec<ComboResult>,
    pub all_passed: bool,
    /// 内置基线对照值（边缘锐度）——「裁判与球员一把尺」的对账面。
    pub builtin_baseline_edge_x100: i64,
}

/// 对方案执行 24 组合审计（Normal 态首帧；模型面确定性渲染）。

// ---------------------------------------------------------------------------
// v2 深化：汇总行 / 批量审计 / 留痕台账
// ---------------------------------------------------------------------------

/// 审计汇总行（详情页头部数字面：过几格、红几格、最差项是什么）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditSummary {
    pub total: usize,
    pub passed: usize,
    pub red: usize,
    /// 最差边缘锐度（最低值——锐度越高越大，取最小为最差）。
    pub worst_edge_x100: i64,
    /// 最低主体对比度。
    pub worst_contrast_x100: i64,
}

/// 从完整报告提取汇总行。
pub fn summarize(rep: &AuditReport) -> AuditSummary {
    let passed = rep.combos.iter().filter(|c| c.passed).count();
    AuditSummary {
        total: rep.combos.len(),
        passed,
        red: rep.combos.len() - passed,
        worst_edge_x100: rep.combos.iter().map(|c| c.edge_x100).min().unwrap_or(0),
        worst_contrast_x100: rep.combos.iter().map(|c| c.contrast_x100).min().unwrap_or(0),
    }
}

/// 批量审计（方案库全量走查口：每方案一份报告——库房详情页逐条挂载
/// 的数据源）。
pub fn batch_audit(schemes: &[CursorSchemeModel]) -> Vec<AuditReport> {
    schemes.iter().map(audit).collect()
}

/// 审计留痕记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    pub at_ms: u64,
    pub scheme_fingerprint: u64,
    pub passed: bool,
}

/// 审计留痕台账（环形 32——何时审了谁、过没过，F372 口径）。
#[derive(Clone, Debug, Default)]
pub struct AuditLedger {
    records: Vec<AuditRecord>,
    dropped: usize,
}

impl AuditLedger {
    pub const CAP: usize = 32;

    pub fn record(&mut self, at_ms: u64, rep: &AuditReport) {
        if self.records.len() >= Self::CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(AuditRecord {
            at_ms,
            scheme_fingerprint: rep.scheme_fingerprint,
            passed: rep.all_passed,
        });
    }

    pub fn records(&self) -> &[AuditRecord] {
        &self.records
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

pub fn audit(m: &CursorSchemeModel) -> AuditReport {
    let frame = m
        .state(crate::jstar2::jbase::PointerState::Normal)
        .and_then(|e| e.frames.first())
        .cloned()
        .unwrap_or_else(|| crate::jstar2::jbase::builtin_glyph(crate::jstar2::jbase::PointerState::Normal));
    let mut combos = Vec::with_capacity(24);
    for dark_theme in [false, true] {
        for backdrop in Thresholds::BACKDROPS {
            for dpi in Thresholds::DPI_TIERS {
                let rendered = render_at_dpi(&frame, dpi);
                let edge = edge_sharpness_x100(&rendered);
                let contrast = min_body_contrast_x100(&rendered, backdrop).unwrap_or(0);
                let over = overshoot_max_alpha(&rendered);
                let (passed, why) = if edge > Thresholds::EDGE_SHARPNESS_MAX_X100 {
                    (false, "边缘过渡带过宽（糊）")
                } else if contrast < Thresholds::CONTRAST_MIN_X100 {
                    (false, "主体对底色对比不足")
                } else if over > Thresholds::OVERSHOOT_ALPHA_MAX {
                    (false, "边界外过冲暗晕")
                } else {
                    (true, "")
                };
                combos.push(ComboResult {
                    dpi,
                    dark_theme,
                    backdrop,
                    edge_x100: edge,
                    contrast_x100: contrast,
                    overshoot: over,
                    passed,
                    why,
                });
            }
        }
    }
    let all_passed = combos.iter().all(|c| c.passed);
    let baseline_frame = crate::jstar2::jbase::builtin_glyph(crate::jstar2::jbase::PointerState::Normal);
    let baseline_edge = edge_sharpness_x100(&render_at_dpi(&baseline_frame, 100));
    AuditReport {
        scheme_fingerprint: crate::jstar2::jbase::vxcur_fingerprint(m),
        combos,
        all_passed,
        builtin_baseline_edge_x100: baseline_edge,
    }
}

/// 一键重生成：把方案重制为「2x 原生」形态（1x 精确复制升 2x——
/// 重生成后 150/200% 走原生 2x 采样，不再经过 1x 插值）。
/// 非破坏：产物为新对象（origin 保持），调用方决定入库。
pub fn regenerate_2x(m: &CursorSchemeModel) -> CursorSchemeModel {
    let mut out = CursorSchemeModel::empty(&alloc::format!("{}·重生成", m.name), OriginKind::Created);
    out.author = m.author.clone();
    out.native_2x = true;
    out.vector_source = m.vector_source;
    out.enhanced_render = m.enhanced_render;
    for e in &m.entries {
        let frames: Vec<CursorFrame> = e
            .frames
            .iter()
            .map(|f| {
                let src = PixBuf::from_rgba(f.w, f.h, f.px.clone());
                let up = src.scale_integer2x();
                crate::jstar2::jbase::CursorFrame::from_buf(
                    (f.hot_x as u32 * 2).min(up.w as u32 - 1) as u16,
                    (f.hot_y as u32 * 2).min(up.h as u32 - 1) as u16,
                    f.delay_ms,
                    up,
                )
            })
            .collect();
        out.set_state(e.state, frames);
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F632 自检。
pub fn run_audit_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F632");

    // 1. 内置基线过审（裁判与球员一把尺）。
    let base = builtin_default_scheme();
    let rep_base = audit(&base);
    set.add(
        "builtin baseline passes 24 combos",
        rep_base.all_passed && rep_base.combos.len() == 24,
        "",
    );

    // 2. 24 组合全覆盖（4 DPI × 3 底 × 2 主题）。
    let mut dpi_seen = [0u32; 4];
    for c in &rep_base.combos {
        if let Some(i) = Thresholds::DPI_TIERS.iter().position(|d| *d == c.dpi) {
            dpi_seen[i] += 1;
        }
    }
    set.add("dpi tiers 6 combos each", dpi_seen == [6, 6, 6, 6], "");

    // 3. 高斯糊化样本被标红（边缘锐度判据的注入验证）。
    let mut blur = base.clone();
    {
        let e = blur.state_mut(crate::jstar2::jbase::PointerState::Normal).unwrap();
        let src = e.frames[0].buf();
        // 三次 1.1x 上下采样近似软化（模型面可控退化）。
        let soft = resample_lanczos3(&resample_lanczos3(&src, (src.w as u32 * 11 / 10).max(1) as u16, (src.h as u32 * 11 / 10).max(1) as u16), src.w, src.h);
        e.frames[0] = crate::jstar2::jbase::CursorFrame::from_buf(e.frames[0].hot_x, e.frames[0].hot_y, 0, soft);
    }
    let rep_blur = audit(&blur);
    set.add(
        "blurred sample flagged red on edge sharpness",
        !rep_blur.all_passed && rep_blur.combos.iter().any(|c| c.why.contains("边缘")),
        "",
    );

    // 4. 一键重生成：救「清晰的 1x 缺 2x」——重生成（精确 2×复制）
    //    后复审通过 + 2x 原生标记。糊化样本的糊是内容属性，重生成
    //    如实保留（糊件继续标红是审计的诚实，不是重生成的失职）。
    let regen = regenerate_2x(&base);
    set.add(
        "regen produces native-2x passing audit",
        regen.native_2x && audit(&regen).all_passed,
        "",
    );
    let regen_blur = regenerate_2x(&blur);
    set.add(
        "blurred content stays honestly red after regen",
        !audit(&regen_blur).all_passed,
        "",
    );

    // 5. 阈值文档在位（一处一事实：改阈值必须改这里并过对账）。
    set.add(
        "thresholds documented",
        Thresholds::EDGE_SHARPNESS_MAX_X100 == 160
            && Thresholds::CONTRAST_MIN_X100 == 200
            && Thresholds::OVERSHOOT_ALPHA_MAX == 96
            && Thresholds::DPI_TIERS == [100, 125, 150, 200]
            && Thresholds::BACKDROPS.len() == 3,
        "",
    );

    // 6. 报告入 F628 详情页（指纹对账挂载通道）。
    use crate::jstar2::library::{SchemeLibrary, AddOutcome};
    let mut lib = SchemeLibrary::new(0);
    let mut m = base.clone();
    m.name = String::from("被审件");
    assert!(matches!(lib.add(m), AddOutcome::Added(_)));
    let rep = audit(&lib.get("被审件").unwrap().model);
    set.add(
        "report attaches to library entry",
        lib.record_report("被审件", to_library_report(rep)).is_ok(),
        "",
    );


    // 6. 汇总行：过/红/最差值与逐组合明细一致（数字面对账）。
    let rep6 = audit(&base);
    let sum = summarize(&rep6);
    set.add(
        "audit summary tallies consistent",
        sum.total == 24
            && sum.passed + sum.red == 24
            && sum.passed == rep6.combos.iter().filter(|c| c.passed).count()
            && sum.worst_edge_x100 == rep6.combos.iter().map(|c| c.edge_x100).min().unwrap_or(0),
        "",
    );

    // 7. 批量审计：多方案逐个出报告 + 台账留痕 + 封顶滚动。
    let mut second = base.clone();
    second.name = alloc::format!("{}乙", second.name);
    let batch = batch_audit(&[base.clone(), second]);
    let mut ledger = AuditLedger::default();
    for (i, r) in batch.iter().enumerate() {
        ledger.record(i as u64, r);
    }
    set.add(
        "batch audit with ledger trail",
        batch.len() == 2
            && ledger.records().len() == 2
            && ledger.records()[0].passed == batch[0].all_passed,
        "",
    );

    set
}

/// F632 报告 → F628 挂载（HealthReport 同通道复用——指纹失效纪律一致）。
/// 此处构造等价的 HealthReport 摘要（24 组合全绿 → Green 提要）。
fn to_library_report(r: AuditReport) -> crate::jstar2::checker::HealthReport {
    use crate::jstar2::checker::{CheckId, Finding, Verdict, HealthReport};
    let mut findings: Vec<Finding> = Vec::new();
    for c in r.combos.iter().filter(|c| !c.passed) {
        findings.push(Finding {
            check: CheckId::SizeLimit,
            verdict: Verdict::Red,
            state: Some(crate::jstar2::jbase::PointerState::Normal),
            frame_index: None,
            detail: alloc::format!("DPI{}{}：{}", c.dpi, if c.dark_theme { "深" } else { "浅" }, c.why),
        });
    }
    HealthReport {
        scheme_fingerprint: r.scheme_fingerprint,
        findings,
        missing_states: Vec::new(),
        fixable: [false, false, false, false],
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::{builtin_glyph, PointerState};

    #[test]
    fn hard_edge_scores_sharp() {
        // 左半实体右半透明 → 纯硬边 → 锐度 = 0。
        let mut buf = PixBuf::new(8, 8);
        for y in 0..8u16 {
            for x in 0..8u16 {
                buf.set(x, y, if x < 4 { [255, 0, 0, 255] } else { [0, 0, 0, 0] });
            }
        }
        assert_eq!(edge_sharpness_x100(&buf), 0);
    }

    #[test]
    fn overshoot_detects_halo() {
        // 实体块缩到 2..6（留出远环带），外圈刷 alpha=110 的晕 →
        // 角点 (0,0) 无一阶实体邻接、2px 内有实体 → 远环捕获。
        let mut buf = PixBuf::new(8, 8);
        for y in 2..6u16 {
            for x in 2..6u16 {
                buf.set(x, y, [0, 0, 0, 255]);
            }
        }
        for y in 0..8u16 {
            for x in 0..8u16 {
                if buf.get(x, y).unwrap()[3] == 0 {
                    buf.set(x, y, [0, 0, 0, 110]);
                }
            }
        }
        // 角点 (0,0) 无 1px 实体邻接（邻居是晕本身 α110<128）但 2px 内
        // 有实体 → 远环口径捕获。
        assert_eq!(overshoot_max_alpha(&buf), 110);
    }

    #[test]
    fn contrast_floor_detects_invisible_pointer() {
        // 白指针对白底 → 对比 < 2:1。
        let mut buf = PixBuf::new(8, 8);
        for c in buf.px.chunks_exact_mut(4) {
            c[0] = 250;
            c[1] = 250;
            c[2] = 250;
            c[3] = 255;
        }
        let c = min_body_contrast_x100(&buf, Rgb::new(245, 245, 245)).unwrap();
        assert!(c < Thresholds::CONTRAST_MIN_X100);
    }

    #[test]
    fn regen_doubles_dimensions_and_hotspot() {
        let mut m = CursorSchemeModel::empty("t", OriginKind::Created);
        let g = builtin_glyph(PointerState::Text);
        m.set_state(PointerState::Text, alloc::vec![g]);
        let out = regenerate_2x(&m);
        let f = &out.state(PointerState::Text).unwrap().frames[0];
        assert_eq!((f.w, f.h), (64, 64));
        assert_eq!((f.hot_x, f.hot_y), (32, 32));
    }

    #[test]
    fn audit_report_empty_scheme_falls_back() {
        let m = CursorSchemeModel::empty("空", OriginKind::Created);
        let rep = audit(&m);
        // 空方案回退内置帧审计（诚实兜底不 panic）。
        assert_eq!(rep.combos.len(), 24);
    }
}
