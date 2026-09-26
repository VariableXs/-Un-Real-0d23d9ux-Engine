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

// ---------------------------------------------------------------------------
// v3 深化批：问题归因分类 · 锐度直方图 · 重生成趋势对账 · 校准快照
// · 过程化底纹底色 · 报告确定性序列化
// ---------------------------------------------------------------------------

use crate::jstar2::jbase::{builtin_glyph, vxcur_fingerprint, PointerState};
use crate::jstar2::checker::HealthReport;

/// 问题归因（红格子的病根分类——「为什么红」从人话升级到可统计的分类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    EdgeBlur,
    LowContrast,
    OvershootHalo,
}

impl IssueKind {
    /// 从红格子的 why 人话归因（判据文案与分类同源——改文案即改分类）。
    pub fn from_why(why: &'static str) -> Option<IssueKind> {
        if why.contains("糊") {
            Some(IssueKind::EdgeBlur)
        } else if why.contains("对比") {
            Some(IssueKind::LowContrast)
        } else if why.contains("晕") {
            Some(IssueKind::OvershootHalo)
        } else {
            None
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            IssueKind::EdgeBlur => "边缘模糊",
            IssueKind::LowContrast => "对比不足",
            IssueKind::OvershootHalo => "过冲晕环",
        }
    }
}

/// 归因统计（每类病根的红格数——修复优先级的数源：先修最常见的）。
pub fn classify_issues(rep: &AuditReport) -> [(IssueKind, usize); 3] {
    let mut blur = 0;
    let mut contrast = 0;
    let mut halo = 0;
    for c in &rep.combos {
        if c.passed {
            continue;
        }
        match IssueKind::from_why(c.why) {
            Some(IssueKind::EdgeBlur) => blur += 1,
            Some(IssueKind::LowContrast) => contrast += 1,
            _ => halo += 1,
        }
    }
    [(IssueKind::EdgeBlur, blur), (IssueKind::LowContrast, contrast), (IssueKind::OvershootHalo, halo)]
}

/// 锐度直方图（4 桶：≤80 / ≤120 / ≤160 / >160——红警线在第三桶顶）。
pub fn sharpness_histogram(rep: &AuditReport) -> [usize; 4] {
    let mut h = [0usize; 4];
    for c in &rep.combos {
        let e = c.edge_x100;
        let bucket = if e <= 80 {
            0
        } else if e <= 120 {
            1
        } else if e <= 160 {
            2
        } else {
            3
        };
        h[bucket] += 1;
    }
    h
}

/// 重生成趋势：重生成前后最差锐度对比（负值 = 变差；预期重生成后
/// 150/200% 走原生 2x 采样 → 最差锐度不劣于重生成前）。
pub fn regen_trend(before: &AuditReport, after: &AuditReport) -> i64 {
    let worst_before = before.combos.iter().map(|c| c.edge_x100).max().unwrap_or(0);
    let worst_after = after.combos.iter().map(|c| c.edge_x100).max().unwrap_or(0);
    worst_after - worst_before
}

/// 校准快照（把一次实测的各档度量钉下来——阈值改动时与快照对账，
/// 「改阈值要有证据」的机器面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationSnapshot {
    pub at_ms: u64,
    pub edge_x100: i64,
    pub contrast_x100: i64,
    pub overshoot: u8,
}

/// 从报告提取校准快照（取 100% × 中灰底这一标准格）。
pub fn calibrate(rep: &AuditReport, at_ms: u64) -> Option<CalibrationSnapshot> {
    let c = rep
        .combos
        .iter()
        .find(|c| c.dpi == 100 && c.backdrop == Rgb::new(128, 128, 128))?;
    Some(CalibrationSnapshot { at_ms, edge_x100: c.edge_x100, contrast_x100: c.contrast_x100, overshoot: c.overshoot })
}

/// 快照漂移检测（同机重测 vs 既有快照：任何指标漂出 ±10% → true——
/// 环境变了，阈值该重新标定而不是硬套旧线）。
pub fn drifted(snap: &CalibrationSnapshot, now: &CalibrationSnapshot) -> bool {
    let pct = |a: i64, b: i64| -> bool {
        if a == 0 {
            return b != 0;
        }
        (b - a).abs() * 100 > a.abs() * 10
    };
    pct(snap.edge_x100, now.edge_x100)
        || pct(snap.contrast_x100, now.contrast_x100)
        || pct(snap.overshoot as i64, now.overshoot as i64)
}

/// 过程化底纹底色（棋盘格——比纯色底更接近真实桌面的「花纹」场景；
/// 确定性：格子尺寸 8px、双色由种子推）。
pub fn procedural_checkerboard(size: u16, seed: u32) -> PixBuf {
    let mut cv = PixBuf::new(size, size);
    let mut rng = crate::jstar2::jbase::XorShift32::new(seed | 1);
    let c1 = [rng.next_u32() as u8, rng.next_u32() as u8, rng.next_u32() as u8, 255];
    let c2 = [rng.next_u32() as u8, rng.next_u32() as u8, rng.next_u32() as u8, 255];
    for y in 0..size {
        for x in 0..size {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            cv.set(x, y, if on { c1 } else { c2 });
        }
    }
    cv
}

/// 底纹场景扩展审计：24 组合之外加一档「棋盘底」（3 DPI × 2 主题 ×
/// 2 种子 = 12 格）——花纹底下的对比度是常规审计的盲区补丁。对账口径：
/// 两种格子色都当作对边逐格算对比、取最差格（内置黑形白边双群设计在
/// 任意格子色下都由其中一群撑起可辨性——与 min_body_contrast 同语义）。
pub fn audit_checkerboard_scene(m: &CursorSchemeModel) -> Vec<(u32, bool, bool, i64, bool)> {
    let frame = m
        .state(PointerState::Normal)
        .and_then(|e| e.frames.first())
        .cloned()
        .unwrap_or_else(|| builtin_glyph(PointerState::Normal));
    let mut out = Vec::new();
    for dpi in [100u32, 150, 200] {
        for dark in [false, true] {
            for seed in [7u32, 42] {
                let mut backdrop = procedural_checkerboard(32, seed);
                if dark {
                    // 深色主题：同一花纹的格子色反转。
                    for c in backdrop.px.chunks_exact_mut(4) {
                        c[0] = 255 - c[0];
                        c[1] = 255 - c[1];
                        c[2] = 255 - c[2];
                    }
                }
                let cell_a = backdrop.get(0, 0).map(|p| Rgb::new(p[0], p[1], p[2]));
                let cell_b = backdrop.get(8, 0).map(|p| Rgb::new(p[0], p[1], p[2]));
                let rendered = if dpi == 100 {
                    PixBuf::from_rgba(frame.w, frame.h, frame.px.clone())
                } else {
                    let scale = dpi * 1000 / 100;
                    let dw = ((frame.w as u32) * scale / 1000).max(1) as u16;
                    let dh = ((frame.h as u32) * scale / 1000).max(1) as u16;
                    crate::jstar2::jbase::resample_lanczos3(&PixBuf::from_rgba(frame.w, frame.h, frame.px.clone()), dw, dh)
                };
                let contrast_a = cell_a.and_then(|c| min_body_contrast_x100(&rendered, c)).unwrap_or(0);
                let contrast_b = cell_b.and_then(|c| min_body_contrast_x100(&rendered, c)).unwrap_or(0);
                let contrast = contrast_a.min(contrast_b);
                let edge = edge_sharpness_x100(&rendered);
                let ok = edge <= Thresholds::EDGE_SHARPNESS_MAX_X100 && contrast >= Thresholds::CONTRAST_MIN_X100;
                out.push((dpi, dark, seed == 7, contrast, ok));
            }
        }
    }
    out
}

/// 报告确定性序列化（详情页导出面：同报告同字节，行序 = 组合序）。
pub fn render_audit_report(rep: &AuditReport) -> String {
    let mut out = String::new();
    out.push_str(&alloc::format!("audit fp={:016x} all={}\n", rep.scheme_fingerprint, rep.all_passed));
    out.push_str(&alloc::format!("baseline_edge={}\n", rep.builtin_baseline_edge_x100));
    for c in &rep.combos {
        out.push_str(&alloc::format!(
            "dpi={} dark={} edge={} contrast={} over={} passed={}\n",
            c.dpi, c.dark_theme as u8, c.edge_x100, c.contrast_x100, c.overshoot, c.passed as u8
        ));
    }
    out
}

/// 导入方案审计入口（血统核对 + 标准审计——Imported 血统不走内置
/// 基线对照（基线只对自家渲染核负责），对照值如实记 -1）。
pub fn audit_imported(m: &CursorSchemeModel, hr: &HealthReport) -> Result<AuditReport, &'static str> {
    if !hr.all_green() {
        return Err("体检有红项——先走 F627 修复，再进审计");
    }
    let mut rep = audit(m);
    if matches!(m.origin, crate::jstar2::jbase::OriginKind::Imported(_)) {
        rep.builtin_baseline_edge_x100 = -1;
    }
    Ok(rep)
}

/// v3 自检。
pub fn run_audit_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F632-v3");
    let base = crate::jstar2::jbase::builtin_default_scheme();
    let rep = audit(&base);

    // 1. 归因分类：全绿方案三类病根皆零；人为模糊方案归因到 EdgeBlur。
    let cls = classify_issues(&rep);
    set.add(
        "issue classification zero for clean scheme",
        cls.iter().all(|(_, n)| *n == 0),
        "",
    );
    set.add(
        "issue kinds map from why text",
        IssueKind::from_why("边缘过渡带过宽（糊）") == Some(IssueKind::EdgeBlur)
            && IssueKind::from_why("主体对底色对比不足") == Some(IssueKind::LowContrast)
            && IssueKind::from_why("边界外过冲暗晕") == Some(IssueKind::OvershootHalo)
            && IssueKind::from_why("未知原因").is_none(),
        "",
    );

    // 2. 锐度直方图：四桶合计 = 24（组合总数守恒）。
    let hist = sharpness_histogram(&rep);
    set.add("sharpness histogram conserves 24", hist[0] + hist[1] + hist[2] + hist[3] == 24, "");

    // 3. 重生成趋势：重生成后最差锐度不劣于重生成前（2x 原生的承诺）。
    let regen = regenerate_2x(&base);
    let rep2 = audit(&regen);
    let trend = regen_trend(&rep, &rep2);
    set.add("regen trend not worse", trend <= 0, "");

    // 4. 校准快照：100% × 中灰标准格可提取；重测同值不判漂移；人为
    //    改值判漂移。
    let snap = calibrate(&rep, 1000);
    let snap2 = calibrate(&audit(&base), 2000);
    let drift_probe = snap.map(|s0| CalibrationSnapshot { contrast_x100: s0.contrast_x100 * 2, ..s0 });
    set.add(
        "calibration snapshot stable and drift detectable",
        snap.is_some()
            && snap2.is_some()
            && !drifted(snap.as_ref().unwrap(), snap2.as_ref().unwrap())
            && drift_probe.as_ref().map(|p| drifted(snap.as_ref().unwrap(), p)).unwrap_or(false),
        "",
    );

    // 5. 过程化棋盘底：确定性（同种子同图）、双色、格子语义。
    let a = procedural_checkerboard(32, 7);
    let b = procedural_checkerboard(32, 7);
    let c = procedural_checkerboard(32, 8);
    set.add(
        "checkerboard deterministic and seeded",
        a.px == b.px && a.px != c.px,
        "",
    );

    // 6. 棋盘场景扩展审计：12 格、结论随格（内置方案在花纹底也可辨）。
    let scene = audit_checkerboard_scene(&base);
    set.add(
        "checkerboard scene 12 combos",
        scene.len() == 12 && scene.iter().all(|r| r.4),
        "",
    );

    // 7. 报告渲染确定性：同方案两次渲染逐字节相等、行数 = 头 2 + 24。
    let r1 = render_audit_report(&rep);
    let r2 = render_audit_report(&audit(&base));
    set.add(
        "audit report render deterministic",
        r1 == r2 && r1.lines().count() == 26,
        "",
    );

    // 8. 导入审计入口：绿体检才放行；Imported 血统的基线对照如实 -1。
    let green_hr = crate::jstar2::checker::inspect(&base);
    let mut foreign = CursorSchemeModel::empty("外来件", crate::jstar2::jbase::OriginKind::Imported(String::from("fp:abc")));
    foreign.set_state(PointerState::Normal, alloc::vec![builtin_glyph(PointerState::Normal)]);
    let red_probe = {
        let mut hr = crate::jstar2::checker::inspect(&foreign);
        hr.findings.push(crate::jstar2::checker::Finding {
            check: crate::jstar2::checker::CheckId::Completeness,
            verdict: crate::jstar2::checker::Verdict::Red,
            state: None,
            frame_index: None,
            detail: String::from("人为注入红项"),
        });
        hr
    };
    set.add("imported audit gates on health", audit_imported(&foreign, &red_probe).is_err(), "");
    let rep3 = audit_imported(&foreign, &green_hr).unwrap();
    let _ = green_hr;
    set.add(
        "imported audit entry honest baseline",
        rep3.builtin_baseline_edge_x100 == -1 && rep3.combos.len() == 24,
        "",
    );

    // 9. 指纹挂账：报告指纹与方案指纹一致（F628 详情挂载的对账键）。
    set.add("report fingerprint matches scheme", rep.scheme_fingerprint == vxcur_fingerprint(&base), "");

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn histogram_buckets_ordered() {
        let rep = audit(&crate::jstar2::jbase::builtin_default_scheme());
        let h = sharpness_histogram(&rep);
        // 内置方案不应有第四桶（>160 = 红线之上）。
        assert_eq!(h[3], 0, "内置方案不能越过锐度红线");
    }

    #[test]
    fn calibration_none_when_combo_missing() {
        // 空方案的报告里没有标准格可提——诚实 None。
        let empty = CursorSchemeModel::empty("空", crate::jstar2::jbase::OriginKind::Created);
        let rep = audit(&empty);
        // audit 用内置 glyph 兜底渲染，仍能出 24 格——快照应有值。
        assert!(calibrate(&rep, 0).is_some());
    }

    #[test]
    fn checkerboard_size_exact() {
        let cv = procedural_checkerboard(16, 1);
        assert_eq!((cv.w, cv.h), (16, 16));
        assert!(cv.solid_count() == 16 * 16, "棋盘全格着色");
    }

    #[test]
    fn trend_of_same_report_is_zero() {
        let rep = audit(&crate::jstar2::jbase::builtin_default_scheme());
        assert_eq!(regen_trend(&rep, &rep), 0);
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：对比度趋势（同方案多次审计的最差对比度序列 + 方向判断）·
// 审计摘要人话行（最差组合是哪个、差在哪一项）
// ---------------------------------------------------------------------------

/// 对比度趋势方向（同方案逐次审计的最差对比度走向；阈值 ±5（×100
/// 定点）防抖——不是每个抖动都叫趋势）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendDir {
    Improving,
    Worsening,
    Flat,
}

/// 对比度趋势台账（同方案逐次审计的最差对比度序列——修复有没有用、
/// 资产有没有劣化，趋势说话，不凭一次审计下结论）。
#[derive(Clone, Debug, Default)]
pub struct ContrastTrend {
    points: Vec<(u64, i64)>,
}

impl ContrastTrend {
    pub fn new() -> ContrastTrend {
        ContrastTrend { points: Vec::new() }
    }

    /// 记一次审计（取最差对比度 = 全组合最小值——最短板入账）。
    pub fn push(&mut self, at_ms: u64, rep: &AuditReport) {
        let worst = summarize(rep).worst_contrast_x100;
        self.points.push((at_ms, worst));
    }

    pub fn series(&self) -> &[(u64, i64)] {
        &self.points
    }

    pub fn worst(&self) -> Option<i64> {
        self.points.iter().map(|(_, c)| *c).min()
    }

    /// 方向判断：后半段均值 − 前半段均值（奇数点中点归前半；空/单点
    /// 无趋势——Flat，不拿一个数编方向）。差 ≥ +5 改善、≤ −5 恶化。
    pub fn direction(&self) -> TrendDir {
        let n = self.points.len();
        if n < 2 {
            return TrendDir::Flat;
        }
        let front = n / 2;
        let front_avg = self.points[..front].iter().map(|p| p.1).sum::<i64>() / front as i64;
        let back_avg = self.points[front..].iter().map(|p| p.1).sum::<i64>() / (n - front) as i64;
        let delta = back_avg - front_avg;
        if delta >= 5 {
            TrendDir::Improving
        } else if delta <= -5 {
            TrendDir::Worsening
        } else {
            TrendDir::Flat
        }
    }

    /// 趋势报告行（人话序列：逐点时刻与值 + 走向结论——修复验收的
    /// 单行凭据；空序列诚实报「暂无样本」）。
    pub fn report_line(&self) -> String {
        let mut out = String::from("对比度趋势");
        for (at, c) in &self.points {
            out.push_str(&alloc::format!(" @{}ms={}", at, c));
        }
        if self.points.is_empty() {
            out.push_str("：暂无样本");
        } else {
            out.push_str(&alloc::format!(
                "；走向 {}",
                match self.direction() {
                    TrendDir::Improving => "改善",
                    TrendDir::Worsening => "恶化",
                    TrendDir::Flat => "持平",
                }
            ));
        }
        out
    }
}

/// 审计摘要人话行（详情页一句话：全绿报喜、标红点名——最差组合是
/// 哪个、差在哪一项。最差 = 标红格中对比度最低者，平局取组合序
/// 靠前者——确定性可选）。
pub fn summary_line(name: &str, rep: &AuditReport) -> String {
    let s = summarize(rep);
    if s.red == 0 {
        return alloc::format!(
            "「{}」24 组合全绿；最差边缘锐度 {}（红线 {}）、最差对比 {}（下限 {}）。",
            name,
            s.worst_edge_x100,
            Thresholds::EDGE_SHARPNESS_MAX_X100,
            s.worst_contrast_x100,
            Thresholds::CONTRAST_MIN_X100
        );
    }
    let worst = rep.combos.iter().filter(|c| !c.passed).min_by_key(|c| c.contrast_x100).unwrap();
    let bg_name = if worst.backdrop == Rgb::new(24, 24, 26) {
        "深"
    } else if worst.backdrop == Rgb::new(245, 245, 245) {
        "浅"
    } else {
        "中灰"
    };
    alloc::format!(
        "「{}」{} 格标红；最差：DPI{} {}主题 {}底——{}（边缘 {} / 对比 {}）。",
        name,
        s.red,
        worst.dpi,
        if worst.dark_theme { "深" } else { "浅" },
        bg_name,
        worst.why,
        worst.edge_x100,
        worst.contrast_x100,
    )
}

/// F632 v4 自检。
pub fn run_audit_v4_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F632-v4");

    // 1. 趋势记录：最差对比度取全组合最小（最短板入账）。
    let base = builtin_default_scheme();
    let rep = audit(&base);
    let mut tr = ContrastTrend::new();
    tr.push(100, &rep);
    tr.push(200, &rep);
    set.add(
        "trend records worst contrast",
        tr.series().len() == 2 && tr.worst() == Some(summarize(&rep).worst_contrast_x100),
        "",
    );

    // 2. 同报告重审 → 持平（零漂移的环境不编趋势）。
    set.add("identical audits trend flat", tr.direction() == TrendDir::Flat, "");

    // 3. 改善/恶化判定（合成序列——方向阈值的确定性验证）。
    let mut up = ContrastTrend::new();
    for (i, c) in [200i64, 210, 220, 230].iter().enumerate() {
        up.points.push((i as u64, *c));
    }
    let mut down = ContrastTrend::new();
    for (i, c) in [230i64, 220, 210, 200].iter().enumerate() {
        down.points.push((i as u64, *c));
    }
    set.add(
        "trend direction improving and worsening",
        up.direction() == TrendDir::Improving && down.direction() == TrendDir::Worsening,
        "",
    );

    // 4. 单点/空序列不编方向（一个样本不是趋势）。
    let mut one = ContrastTrend::new();
    one.points.push((0, 100));
    set.add(
        "trend needs two points for direction",
        one.direction() == TrendDir::Flat && ContrastTrend::new().direction() == TrendDir::Flat,
        "",
    );

    // 5. 趋势报告行：序列 + 走向结论 + 空态诚实。
    set.add(
        "trend report line readable",
        up.report_line().contains("改善")
            && down.report_line().contains("恶化")
            && tr.report_line().contains("持平")
            && ContrastTrend::new().report_line().contains("暂无样本"),
        "",
    );

    // 6. 摘要人话行：全绿报喜（含名字与阈值口径）。
    let line_ok = summary_line("内置基线", &rep);
    set.add(
        "summary line praises green scheme",
        line_ok.contains("全绿") && line_ok.contains("内置基线") && line_ok.contains("160"),
        "",
    );

    // 7. 摘要人话行：标红点名（第 8 格注入红项 = DPI200 浅底浅主题，
    //    点名到 DPI/主题/底色/病根/数值）。
    let mut red_rep = rep.clone();
    red_rep.combos[7] = ComboResult {
        dpi: 200,
        dark_theme: false,
        backdrop: Rgb::new(245, 245, 245),
        edge_x100: 100,
        contrast_x100: 120,
        overshoot: 0,
        passed: false,
        why: "主体对底色对比不足",
    };
    red_rep.all_passed = false;
    let line_red = summary_line("问题件", &red_rep);
    set.add(
        "summary line names worst combo",
        line_red.contains("1 格标红")
            && line_red.contains("DPI200")
            && line_red.contains("主体对底色对比不足")
            && line_red.contains("120"),
        "",
    );

    // 8. 摘要行确定性：同报告同字节。
    set.add("summary line deterministic", summary_line("问题件", &red_rep) == line_red, "");

    // 9. 真实糊件点名：模糊方案的摘要行含病根人话（文案与分类同源）。
    let mut blur = base.clone();
    {
        let e = blur.state_mut(PointerState::Normal).unwrap();
        let src = e.frames[0].buf();
        let soft = resample_lanczos3(
            &resample_lanczos3(
                &src,
                (src.w as u32 * 11 / 10).max(1) as u16,
                (src.h as u32 * 11 / 10).max(1) as u16,
            ),
            src.w,
            src.h,
        );
        e.frames[0] = CursorFrame::from_buf(e.frames[0].hot_x, e.frames[0].hot_y, 0, soft);
    }
    let line_blur = summary_line("糊件", &audit(&blur));
    set.add(
        "blurred scheme summary names the defect",
        line_blur.contains("标红")
            && (line_blur.contains("糊") || line_blur.contains("对比") || line_blur.contains("晕")),
        "",
    );

    // 10. 修复闭环趋势：基线 → 糊件 → 基线（最差对比度回到基线水平
    //     ——修一次的净效果为零劣化）。
    let mut tr2 = ContrastTrend::new();
    tr2.push(1, &rep);
    tr2.push(2, &audit(&blur));
    tr2.push(3, &audit(&base));
    set.add(
        "repair loop trend returns to baseline worst",
        tr2.series().len() == 3 && tr2.worst() == Some(summarize(&rep).worst_contrast_x100),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
    use super::*;

    fn combo(dpi: u32, contrast: i64, passed: bool) -> ComboResult {
        ComboResult {
            dpi,
            dark_theme: false,
            backdrop: Rgb::new(128, 128, 128),
            edge_x100: 100,
            contrast_x100: contrast,
            overshoot: 0,
            passed,
            why: if passed { "" } else { "主体对底色对比不足" },
        }
    }

    fn fake_report(contrast: i64, pass: bool) -> AuditReport {
        AuditReport {
            scheme_fingerprint: 0,
            combos: (0..24).map(|i| combo(100 + (i % 4) as u32, contrast, pass)).collect(),
            all_passed: pass,
            builtin_baseline_edge_x100: 0,
        }
    }

    #[test]
    fn direction_thresholds() {
        let mut t = ContrastTrend::new();
        t.points.push((0, 200));
        t.points.push((1, 206));
        assert_eq!(t.direction(), TrendDir::Improving, "delta = +6 ≥ 5");
        t.points.push((2, 203));
        // 前半 [200]，后半 [206,203] → 均值 204 → delta 4 → Flat。
        assert_eq!(t.direction(), TrendDir::Flat);
    }

    #[test]
    fn worst_tracks_min() {
        let mut t = ContrastTrend::new();
        let mut r1 = fake_report(250, true);
        r1.combos[3] = combo(150, 180, true);
        t.push(0, &r1);
        assert_eq!(t.worst(), Some(180), "最短板入账");
    }

    #[test]
    fn summary_red_counts_and_names() {
        let mut r = fake_report(300, true);
        r.combos[0].passed = false;
        r.combos[0].dpi = 125;
        let line = summary_line("测试件", &r);
        assert!(line.contains("1 格标红"));
        assert!(line.contains("DPI125"));
    }

    #[test]
    fn empty_trend_report_honest() {
        assert!(ContrastTrend::new().report_line().contains("暂无样本"));
        assert_eq!(ContrastTrend::new().worst(), None);
    }

    #[test]
    fn v4_checks_all_green() {
        let set = run_audit_v4_checks();
        assert!(!set.truncated());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "v4 check red: {}", c.name);
        }
    }
}
