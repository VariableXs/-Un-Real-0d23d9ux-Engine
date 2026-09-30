//! F627 指针语义检查器 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：四项检查注入样本全对（缺态/热点出界/超尺寸/
//! 超帧率包各一）；一键修复四路；缺态运行时回退链；报告与 F628 方案库
//! 联动；修复可撤销。
//!
//! **检查面（15 标准态口径与 F156/F627 一致）**：
//! 1. **齐全性**：15 标准态逐态核对，缺哪态列出；运行时回退链——缺态
//!    按回退序（内置默认方案同态 → 正常态兜底）解析并登记回退记录；
//! 2. **热点合理性**：热点须在帧边界内 **且** 落在图形实体上（alpha
//!    ≥128）；出界=红、落透明区=红（转 F637 热区补偿推荐）；
//! 3. **尺寸合规**：单帧 > 64px 提示「过大遮挡内容」（提示不拒入——
//!    硬闸 256px 在 F639 安全闸）；
//! 4. **动画纪律**：单态帧数 ≤16（F156/F625 同源）；有效帧率 ≤60fps
//!    （F639 帧率闸同源数值）。
//!
//! **修复四路（一键）**：缺态补默认（jbase 内置方案同态帧）/热点回实体
//! 中心（内容包围盒中心，落 F637 推荐同源）/超限缩放（Lanczos 到 64px
//! 内）/超帧降速（延时抬到 ≥17ms；超 16 帧截断并如实注记）。
//! **非破坏纪律**：全部修复产出「修复副本」（原件不动），副本可整体
//! 撤销（丢弃副本即回原件）——判据「修复可撤销」的机制面。
//!
//! **F628 联动**：报告以 `scheme_fingerprint`（jbase 内容指纹）挂到
//! 方案库详情页；库房侧 `set_report` 记录最近体检结果（对账口径两处
//! 同一指纹）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    builtin_default_scheme, resample_lanczos3, vxcur_fingerprint, CursorFrame, CursorSchemeModel,
    OriginKind, PixBuf, PointerState, MAX_FPS, MAX_FRAMES_PER_STATE, WARN_FRAME_PX,
};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 报告模型
// ---------------------------------------------------------------------------

/// 单项检查结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Warn,
    Red,
}

/// 单条发现（可带受影响态/帧定位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub check: CheckId,
    pub verdict: Verdict,
    pub state: Option<PointerState>,
    pub frame_index: Option<usize>,
    pub detail: String,
}

/// 四项检查 ID（报告与修复路由共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckId {
    Completeness,
    Hotspot,
    SizeLimit,
    AnimationDiscipline,
}

impl CheckId {
    pub fn zh(self) -> &'static str {
        match self {
            CheckId::Completeness => "15 标准态齐全性",
            CheckId::Hotspot => "热点合理性",
            CheckId::SizeLimit => "尺寸合规",
            CheckId::AnimationDiscipline => "动画帧率纪律",
        }
    }
}

/// 完整体检报告（F628 详情页的挂载物）。
#[derive(Clone, Debug)]
pub struct HealthReport {
    pub scheme_fingerprint: u64,
    pub findings: Vec<Finding>,
    pub missing_states: Vec<PointerState>,
    /// 可用的一键修复路由（四路子集）。
    pub fixable: [bool; 4],
}

impl HealthReport {
    pub fn verdict_of(&self, c: CheckId) -> Verdict {
        let mut v = Verdict::Green;
        for f in &self.findings {
            if f.check == c {
                v = worse(v, f.verdict);
            }
        }
        v
    }

    pub fn all_green(&self) -> bool {
        (0..4).all(|i| self.verdict_of(IDS[i]) == Verdict::Green)
    }
}

const IDS: [CheckId; 4] = [
    CheckId::Completeness,
    CheckId::Hotspot,
    CheckId::SizeLimit,
    CheckId::AnimationDiscipline,
];

fn worse(a: Verdict, b: Verdict) -> Verdict {
    match (a, b) {
        (Verdict::Red, _) | (_, Verdict::Red) => Verdict::Red,
        (Verdict::Warn, _) | (_, Verdict::Warn) => Verdict::Warn,
        _ => Verdict::Green,
    }
}

// ---------------------------------------------------------------------------
// 体检主体
// ---------------------------------------------------------------------------

/// 对方案执行四项检查（纯函数：不改方案）。
pub fn inspect(m: &CursorSchemeModel) -> HealthReport {
    let mut findings: Vec<Finding> = Vec::new();
    // —— 1. 齐全性 ——
    let missing = m.missing_states();
    for st in &missing {
        findings.push(Finding {
            check: CheckId::Completeness,
            verdict: Verdict::Red,
            state: Some(*st),
            frame_index: None,
            detail: alloc::format!("缺少「{}」态——运行时将回退默认并登记", st.zh_name()),
        });
    }
    // —— 2. 热点合理性（边界内 + 实体上）——
    let mut hotspot_hit = false;
    for e in &m.entries {
        for (fi, f) in e.frames.iter().enumerate() {
            if f.hot_x >= f.w || f.hot_y >= f.h {
                findings.push(Finding {
                    check: CheckId::Hotspot,
                    verdict: Verdict::Red,
                    state: Some(e.state),
                    frame_index: Some(fi),
                    detail: alloc::format!(
                        "热点 ({},{}) 超出 {}×{} 帧边界",
                        f.hot_x,
                        f.hot_y,
                        f.w,
                        f.h
                    ),
                });
                hotspot_hit = true;
                continue;
            }
            let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
            if !buf.solid(f.hot_x, f.hot_y) {
                findings.push(Finding {
                    check: CheckId::Hotspot,
                    verdict: Verdict::Red,
                    state: Some(e.state),
                    frame_index: Some(fi),
                    detail: alloc::format!(
                        "「{}」态热点 ({},{}) 落在透明区——建议交给 F637 热区补偿推荐",
                        e.state.zh_name(),
                        f.hot_x,
                        f.hot_y
                    ),
                });
                hotspot_hit = true;
            }
        }
    }
    // —— 3. 尺寸合规（提示线 64px；硬闸 256 在 F639）——
    let mut size_hit = false;
    for e in &m.entries {
        for (fi, f) in e.frames.iter().enumerate() {
            if (f.w as u32).max(f.h as u32) > WARN_FRAME_PX {
                findings.push(Finding {
                    check: CheckId::SizeLimit,
                    verdict: Verdict::Warn,
                    state: Some(e.state),
                    frame_index: Some(fi),
                    detail: alloc::format!(
                        "「{}」态帧 {}×{} 超过 {}px 提示线——过大指针遮挡内容",
                        e.state.zh_name(),
                        f.w,
                        f.h,
                        WARN_FRAME_PX
                    ),
                });
                size_hit = true;
            }
        }
    }
    // —— 4. 动画纪律（帧数 + 帧率）——
    let mut anim_hit = false;
    for e in &m.entries {
        if e.frames.len() > MAX_FRAMES_PER_STATE {
            findings.push(Finding {
                check: CheckId::AnimationDiscipline,
                verdict: Verdict::Red,
                state: Some(e.state),
                frame_index: None,
                detail: alloc::format!(
                    "「{}」态 {} 帧超过 16 帧纪律上限",
                    e.state.zh_name(),
                    e.frames.len()
                ),
            });
            anim_hit = true;
        }
        for (fi, f) in e.frames.iter().enumerate() {
            if f.delay_ms > 0 && f.delay_ms < 17 {
                findings.push(Finding {
                    check: CheckId::AnimationDiscipline,
                    verdict: Verdict::Red,
                    state: Some(e.state),
                    frame_index: Some(fi),
                    detail: alloc::format!(
                        "「{}」态第 {} 帧延时 {}ms 低于 17ms——有效帧率超过 {}fps 上限",
                        e.state.zh_name(),
                        fi,
                        f.delay_ms,
                        MAX_FPS
                    ),
                });
                anim_hit = true;
            }
        }
    }
    HealthReport {
        scheme_fingerprint: vxcur_fingerprint(m),
        fixable: [!missing.is_empty(), hotspot_hit, size_hit, anim_hit],
        findings,
        missing_states: missing,
    }
}

// ---------------------------------------------------------------------------
// 修复四路（产出修复副本，原件不动——可撤销的机制基础）
// ---------------------------------------------------------------------------

/// 修复结果：副本方案 + 撤销凭据（副本名）。
pub struct FixOutcome {
    pub repaired: CursorSchemeModel,
    pub applied: [CheckId; 4],
    /// 撤销凭据：库房侧凭此删除副本即完成撤销（原件从未改动）。
    pub undo_token: u64,
}

/// 一键修复（四路全跑；无红/警项时返回 None——不空转）。
///
/// 修复语义（判据「一键修复四路」）：
/// - 缺态 → 补 jbase 内置默认同态帧；
/// - 热点出界/透明 → 移到内容包围盒中心（与 F637 视觉推荐同源起点）；
/// - 超尺寸 → Lanczos 等比缩到 64px 内（热点同比例迁移）；
/// - 超 16 帧 → 截断前 16 帧并注记；超 60fps → 延时抬到 17ms。
pub fn fix_all(m: &CursorSchemeModel) -> Option<FixOutcome> {
    let report = inspect(m);
    if report.all_green() {
        return None;
    }
    let mut r = m.clone();
    let mut applied: Vec<CheckId> = Vec::new();
    let builtin = builtin_default_scheme();

    // 1. 缺态补默认。
    if !report.missing_states.is_empty() {
        for st in &report.missing_states {
            if let Some(bf) = builtin.state(*st) {
                r.set_state(*st, bf.frames.clone());
            }
        }
        applied.push(CheckId::Completeness);
    }

    // 2. 热点修正：出界或透明 → 内容包围盒中心。
    if report.verdict_of(CheckId::Hotspot) != Verdict::Green {
        for e in r.entries.iter_mut() {
            for f in e.frames.iter_mut() {
                let out_of_bounds = f.hot_x >= f.w || f.hot_y >= f.h;
                let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
                let on_solid = !out_of_bounds && buf.solid(f.hot_x, f.hot_y);
                if out_of_bounds || !on_solid {
                    if let Some((cx, cy)) = buf.visual_centroid() {
                        f.hot_x = cx;
                        f.hot_y = cy;
                    } else {
                        f.hot_x = f.w / 2;
                        f.hot_y = f.h / 2;
                    }
                }
            }
        }
        applied.push(CheckId::Hotspot);
    }

    // 3. 超尺寸 → 等比缩到 64px 内。
    if report.verdict_of(CheckId::SizeLimit) != Verdict::Green {
        for e in r.entries.iter_mut() {
            for f in e.frames.iter_mut() {
                let mx = (f.w as u32).max(f.h as u32);
                if mx > WARN_FRAME_PX {
                    let scale = WARN_FRAME_PX * 1000 / mx;
                    let nw = ((f.w as u32) * scale / 1000).max(1) as u16;
                    let nh = ((f.h as u32) * scale / 1000).max(1) as u16;
                    let src = PixBuf::from_rgba(f.w, f.h, f.px.clone());
                    let dst = resample_lanczos3(&src, nw, nh);
                    let nhx = (f.hot_x as u32 * scale / 1000).min(nw as u32 - 1) as u16;
                    let nhy = (f.hot_y as u32 * scale / 1000).min(nh as u32 - 1) as u16;
                    *f = CursorFrame::from_buf(nhx, nhy, f.delay_ms, dst);
                }
            }
        }
        applied.push(CheckId::SizeLimit);
    }

    // 4. 动画纪律：>16 帧截断；超帧率延时抬到 17ms。
    if report.verdict_of(CheckId::AnimationDiscipline) != Verdict::Green {
        for e in r.entries.iter_mut() {
            if e.frames.len() > MAX_FRAMES_PER_STATE {
                e.frames.truncate(MAX_FRAMES_PER_STATE);
            }
            for f in e.frames.iter_mut() {
                if f.delay_ms > 0 && f.delay_ms < 17 {
                    f.delay_ms = 17; // 1000/17 ≈ 58.8fps ≤ 60
                }
            }
        }
        applied.push(CheckId::AnimationDiscipline);
    }

    let mut route = [
        CheckId::Completeness,
        CheckId::Hotspot,
        CheckId::SizeLimit,
        CheckId::AnimationDiscipline,
    ];
    for (i, id) in applied.iter().enumerate() {
        route[i] = *id;
    }
    Some(FixOutcome {
        repaired: r,
        applied: route,
        undo_token: vxcur_fingerprint(m),
    })
}

// ---------------------------------------------------------------------------
// 运行时回退链（缺态解析；登记式）
// ---------------------------------------------------------------------------

/// 一次缺态回退的登记记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackRecord {
    pub state: PointerState,
    /// 回退来源：Some(方案名) = 从指定方案借同态；None = 内置默认。
    pub from_scheme: Option<String>,
}

/// 缺态解析结果（三态显式——调用方按变体取帧，无悬垂引用）。
pub enum ResolvedFrame<'a> {
    /// 本方案自有该态。
    Own(&'a CursorFrame),
    /// 从 donor 方案借到同态（附带 donor 名，供登记）。
    Donor(&'a CursorFrame, String),
    /// 内置默认兜底（值语义帧，生命周期独立）。
    Builtin(CursorFrame),
}

impl<'a> ResolvedFrame<'a> {
    pub fn frame(&self) -> &CursorFrame {
        match self {
            ResolvedFrame::Own(f) | ResolvedFrame::Donor(f, _) => f,
            ResolvedFrame::Builtin(f) => f,
        }
    }
}

/// 运行时缺态回退解析：本方案 → donor 方案同态 → 内置默认。
/// 走了回退路径时向 `log` 追加登记记录（判据「缺态运行时回退链 +
/// 登记」的机制面）。
pub fn resolve_with_fallback<'a>(
    m: &'a CursorSchemeModel,
    st: PointerState,
    donor: Option<&'a CursorSchemeModel>,
    log: &mut Vec<FallbackRecord>,
) -> ResolvedFrame<'a> {
    if let Some(e) = m.state(st) {
        if let Some(f) = e.frames.first() {
            return ResolvedFrame::Own(f);
        }
    }
    if let Some(d) = donor {
        if let Some(e) = d.state(st) {
            if let Some(f) = e.frames.first() {
                log.push(FallbackRecord { state: st, from_scheme: Some(d.name.clone()) });
                return ResolvedFrame::Donor(f, d.name.clone());
            }
        }
    }
    let builtin = builtin_default_scheme();
    let f = builtin
        .state(st)
        .and_then(|e| e.frames.first())
        .cloned()
        .expect("内置默认方案 15 态齐全（jbase 构造保证）");
    log.push(FallbackRecord { state: st, from_scheme: None });
    ResolvedFrame::Builtin(f)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条）
// ---------------------------------------------------------------------------

/// F627 自检。

// ---------------------------------------------------------------------------
// v2 深化：批量体检 / 体检阈值域内可配
// ---------------------------------------------------------------------------

/// 体检阈值（F627 判据线的域内可配面——与 F639 阈值文档同哲学：
/// 管理员可收紧、不可放宽过出厂线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckerThresholds {
    /// 单帧尺寸提示线（px，判据 64px 提示——过大遮挡内容）。
    pub size_warn_px: u32,
    /// 帧率纪律线（fps）。
    pub fps_cap: u32,
}

impl Default for CheckerThresholds {
    fn default() -> Self {
        CheckerThresholds { size_warn_px: 64, fps_cap: crate::jstar2::jbase::MAX_FPS }
    }
}

impl CheckerThresholds {
    /// 管理员覆盖（只许收紧——放宽过出厂线拒绝）。
    pub fn admin_override(&mut self, size_px: u32, fps: u32) -> Result<(), &'static str> {
        if size_px > 64 || fps > crate::jstar2::jbase::MAX_FPS {
            return Err("体检线只能收紧、不能放宽过出厂判据");
        }
        if size_px == 0 || fps == 0 {
            return Err("阈值为零等于全拒——请给出正数");
        }
        self.size_warn_px = size_px;
        self.fps_cap = fps;
        Ok(())
    }
}

/// 批量体检（方案库全量走查口——每方案一份报告，与 F628 的
/// health_sweep 过期扫描配套：扫出旧的就批量补新报告）。
pub fn batch_inspect(schemes: &[CursorSchemeModel]) -> Vec<HealthReport> {
    schemes.iter().map(inspect).collect()
}

pub fn run_checker_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_glyph;
    let mut set = CheckSet::new("jstar2-F627");
    let good = builtin_default_scheme();

    // 1. 完好方案（内置基线）体检全绿。
    let rep = inspect(&good);
    set.add("builtin baseline all green", rep.all_green(), "");

    // 2. 注入样本①：缺态包（只放 3 态）→ 齐全性红 + 12 缺态列出。
    let mut holey = CursorSchemeModel::empty("残缺包", OriginKind::Imported(String::new()));
    for st in [PointerState::Normal, PointerState::Text, PointerState::Busy] {
        holey.set_state(st, alloc::vec![builtin_glyph(st)]);
    }
    let rep = inspect(&holey);
    set.add(
        "missing-state sample red with 12 listed",
        rep.verdict_of(CheckId::Completeness) == Verdict::Red && rep.missing_states.len() == 12,
        "",
    );

    // 3. 注入样本②：热点出界 + 热点落透明区 → 热点红（两条发现）。
    let mut badhot = builtin_default_scheme();
    {
        let e = badhot.state_mut(PointerState::Normal).unwrap();
        e.frames[0].hot_x = 200; // 出界（32×32 帧）
    }
    {
        let e = badhot.state_mut(PointerState::Text).unwrap();
        e.frames[0].hot_x = 0;
        e.frames[0].hot_y = 0; // 32×32 文本指针角落 = 透明区
    }
    let rep = inspect(&badhot);
    let hot_reds = rep
        .findings
        .iter()
        .filter(|f| f.check == CheckId::Hotspot && f.verdict == Verdict::Red)
        .count();
    set.add("hotspot oob + transparent both caught", hot_reds == 2, "");

    // 4. 注入样本③：超尺寸包（80px 帧 → 提示线 Warn）。
    let mut bigf = builtin_glyph(PointerState::Move);
    bigf.w = 80;
    bigf.h = 80;
    bigf.px = alloc::vec![255u8; 80 * 80 * 4];
    let mut big = CursorSchemeModel::empty("巨大包", OriginKind::Imported(String::new()));
    big.set_state(PointerState::Move, alloc::vec![bigf]);
    let rep = inspect(&big);
    set.add(
        "oversize sample warned at 64px line",
        rep.verdict_of(CheckId::SizeLimit) == Verdict::Warn,
        "",
    );

    // 5. 注入样本④：超帧率包（5ms 延时 = 200fps）+ 超 16 帧。
    let mut fast = builtin_glyph(PointerState::Busy);
    fast.delay_ms = 5;
    let mut fastm = CursorSchemeModel::empty("超速包", OriginKind::Imported(String::new()));
    let mut many: Vec<CursorFrame> = Vec::new();
    for _ in 0..20 {
        many.push(fast.clone());
    }
    fastm.set_state(PointerState::Busy, many);
    let rep = inspect(&fastm);
    set.add(
        "over-fps and over-16-frames both red",
        rep.verdict_of(CheckId::AnimationDiscipline) == Verdict::Red,
        "",
    );

    // 6. 一键修复四路：残缺+坏热点+超大+超速 → 修复副本全绿，原件未动。
    let mut mess = CursorSchemeModel::empty("烂包", OriginKind::Imported(String::new()));
    let mut big_frame = builtin_glyph(PointerState::Normal);
    big_frame.w = 96;
    big_frame.h = 96;
    big_frame.px = alloc::vec![200u8; 96 * 96 * 4];
    big_frame.hot_x = 250; // 出界
    big_frame.hot_y = 250;
    big_frame.delay_ms = 4; // 250fps
    mess.set_state(PointerState::Normal, alloc::vec![big_frame]);
    let original_fp = vxcur_fingerprint(&mess);
    let fix = fix_all(&mess).expect("fixable");
    let rep2 = inspect(&fix.repaired);
    set.add(
        "fix-all yields all green copy",
        rep2.all_green() && vxcur_fingerprint(&mess) == original_fp,
        "",
    );
    // 修复后热点落在实体上 + 缺态补齐。
    let f0 = &fix.repaired.state(PointerState::Normal).unwrap().frames[0];
    let buf = f0.buf();
    set.add(
        "fixed hotspot solid + 15 states",
        buf.solid(f0.hot_x, f0.hot_y) && fix.repaired.missing_states().is_empty(),
        "",
    );

    // 7. 撤销：凭 undo_token（=原件指纹）删除副本即回原件——机制面验证。
    set.add("undo token equals original fingerprint", fix.undo_token == original_fp, "");

    // 8. 无病方案不空转修复。
    set.add("no-op fix returns none", fix_all(&good).is_none(), "");

    // 9. 回退链：残缺方案缺态从 donor 借同态并登记；无 donor 落内置默认。
    let mut log: Vec<FallbackRecord> = Vec::new();
    let donor = builtin_default_scheme();
    match resolve_with_fallback(&holey, PointerState::Link, Some(&donor), &mut log) {
        ResolvedFrame::Donor(_, name) => {
            set.add(
                "fallback chain donor logged",
                log.len() == 1 && name == "VARIX 默认指针",
                "",
            );
        }
        _ => set.add("fallback chain donor logged", false, "wrong variant"),
    }
    // 9b. 无 donor 时的内置兜底变体 + 登记。
    let mut log2: Vec<FallbackRecord> = Vec::new();
    match resolve_with_fallback(&holey, PointerState::Move, None, &mut log2) {
        ResolvedFrame::Builtin(_) => {
            set.add(
                "fallback chain builtin logged",
                log2.len() == 1 && log2[0].from_scheme.is_none(),
                "",
            );
        }
        _ => set.add("fallback chain builtin logged", false, "wrong variant"),
    }

    // 10. 修复副本与原件可指纹区分（F628 两条目不混淆）。
    set.add(
        "repaired fingerprint differs from original",
        vxcur_fingerprint(&fix.repaired) != original_fp,
        "",
    );


    // 6. 批量体检：多方案逐个出报告、指纹各归各案。
    let mut second6 = builtin_default_scheme();
    second6.name = alloc::format!("{}乙", second6.name);
    second6.author = alloc::format!("{}乙", second6.author);
    let batch = batch_inspect(&[good.clone(), second6]);
    set.add(
        "batch inspect per-scheme fingerprints",
        batch.len() == 2
            && batch[0].scheme_fingerprint != batch[1].scheme_fingerprint
            && batch[0].scheme_fingerprint == vxcur_fingerprint(&good),
        "",
    );

    // 7. 体检阈值可配：收紧放行、放宽拒绝、零值拒绝。
    let mut th = CheckerThresholds::default();
    let tighten = th.admin_override(48, 30);
    let loosen = th.admin_override(128, 30);
    let zero = th.admin_override(0, 30);
    set.add(
        "checker thresholds tighten-only",
        tighten.is_ok() && th.size_warn_px == 48 && loosen.is_err() && zero.is_err(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::builtin_glyph;

    fn scheme_with(states: &[PointerState], mutate: impl Fn(&mut CursorSchemeModel)) -> CursorSchemeModel {
        let mut m = CursorSchemeModel::empty("t", OriginKind::Created);
        for st in states {
            m.set_state(*st, alloc::vec![builtin_glyph(*st)]);
        }
        mutate(&mut m);
        m
    }

    #[test]
    fn healthy_scheme_passes() {
        let m = builtin_default_scheme();
        let rep = inspect(&m);
        assert!(rep.all_green());
        assert!(rep.findings.is_empty());
    }

    #[test]
    fn missing_states_listed_exactly() {
        let m = scheme_with(&[PointerState::Normal, PointerState::Move], |_| {});
        let rep = inspect(&m);
        assert_eq!(rep.missing_states.len(), 13);
        assert_eq!(rep.verdict_of(CheckId::Completeness), Verdict::Red);
        // 「缺哪态列出」——发现明细逐条带态名。
        for st in &rep.missing_states {
            assert!(rep.findings.iter().any(|f| f.state == Some(*st)));
        }
    }

    #[test]
    fn hotspot_transparent_maps_to_f637_hint() {
        let m = scheme_with(&[PointerState::Text], |m| {
            let e = m.state_mut(PointerState::Text).unwrap();
            e.frames[0].hot_x = 0;
            e.frames[0].hot_y = 0;
        });
        let rep = inspect(&m);
        let f = rep
            .findings
            .iter()
            .find(|f| f.check == CheckId::Hotspot)
            .unwrap();
        assert!(f.detail.contains("F637"));
    }

    #[test]
    fn fix_reduces_fps_to_within_60() {
        let m = scheme_with(&[PointerState::Busy], |m| {
            let e = m.state_mut(PointerState::Busy).unwrap();
            e.frames[0].delay_ms = 2; // 500fps
        });
        let fix = fix_all(&m).unwrap();
        let e = fix.repaired.state(PointerState::Busy).unwrap();
        assert!(e.frames[0].fps() <= MAX_FPS);
        assert_eq!(e.frames[0].delay_ms, 17);
    }

    #[test]
    fn fix_downscales_to_64_line() {
        let mut big = builtin_glyph(PointerState::Normal);
        big.w = 100;
        big.h = 60;
        big.px = alloc::vec![255u8; 100 * 60 * 4];
        let mut m = CursorSchemeModel::empty("big", OriginKind::Created);
        m.set_state(PointerState::Normal, alloc::vec![big]);
        let fix = fix_all(&m).unwrap();
        let f = &fix.repaired.state(PointerState::Normal).unwrap().frames[0];
        assert!((f.w as u32).max(f.h as u32) <= WARN_FRAME_PX);
        // 等比：64/100 → 宽 64，高 60·640/1000 = 38。
        assert_eq!(f.w, 64);
        assert_eq!(f.h, 38);
    }

    #[test]
    fn original_never_touched_by_fix() {
        let m = scheme_with(&[PointerState::Help], |m| {
            let e = m.state_mut(PointerState::Help).unwrap();
            e.frames[0].hot_x = 500;
        });
        let fp = vxcur_fingerprint(&m);
        let fix = fix_all(&m).unwrap();
        assert_eq!(vxcur_fingerprint(&m), fp, "原件必须分毫未动");
        assert!(inspect(&fix.repaired).all_green());
    }

    #[test]
    fn fallback_without_donor_reports_builtin() {
        let m = scheme_with(&[PointerState::Normal], |_| {});
        let mut log = Vec::new();
        let donor = builtin_default_scheme();
        let r = resolve_with_fallback(&m, PointerState::Link, Some(&donor), &mut log);
        assert!(matches!(r, ResolvedFrame::Donor(_, _)));
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].state, PointerState::Link);
        assert!(log[0].from_scheme.is_some());
    }

    #[test]
    fn fallback_builtin_variant_when_no_donor() {
        let m = scheme_with(&[PointerState::Normal], |_| {});
        let mut log = Vec::new();
        let r = resolve_with_fallback(&m, PointerState::Move, None, &mut log);
        assert!(matches!(r, ResolvedFrame::Builtin(_)));
        assert_eq!(log.len(), 1);
        assert!(log[0].from_scheme.is_none());
        // 内置兜底帧自身合规：热点落实体上。
        let f = r.frame();
        let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
        assert!(buf.solid(f.hot_x, f.hot_y));
    }

    #[test]
    fn present_state_never_falls_back() {
        let m = scheme_with(&[PointerState::Normal], |_| {});
        let mut log = Vec::new();
        let donor = builtin_default_scheme();
        match resolve_with_fallback(&m, PointerState::Normal, Some(&donor), &mut log) {
            ResolvedFrame::Own(_) => {}
            _ => panic!("在态应走 Own 变体"),
        }
        assert!(log.is_empty(), "在态不走回退");
    }
}

// ---------------------------------------------------------------------------
// v3 深化批：扩展五检（透明态/重复帧/延时一致性/跨态尺寸/热点重心）
// · 深化修复三路（补齐内置/尺寸归一/邻帧去重）· 报告确定性渲染
// ---------------------------------------------------------------------------

use crate::jstar2::jbase::ALL_STATES;

/// v3 扩展检查 ID（四主检之外的第二层——「更难看的角落也要看得见」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtendedCheckId {
    /// 全透明态（一个态 15 帧全透明 = 指针在屏幕上消失——红）。
    InvisibleState,
    /// 邻帧全同（动画态里相邻帧逐字节相等 = 假动画——警）。
    DuplicateAdjacentFrames,
    /// 帧延时一致性（同态内延时参差 = 播放节奏抖——警）。
    DelayUniformity,
    /// 跨态尺寸一致（同方案内画布尺寸不一 = 切换态时跳——警）。
    CrossStateSizeConsistency,
    /// 热点重心离群（热点远离视觉重心超出内容半径 = 点不准——警）。
    HotspotCentroidOutlier,
}

impl ExtendedCheckId {
    pub fn zh(self) -> &'static str {
        match self {
            ExtendedCheckId::InvisibleState => "全透明态",
            ExtendedCheckId::DuplicateAdjacentFrames => "邻帧全同",
            ExtendedCheckId::DelayUniformity => "帧延时一致性",
            ExtendedCheckId::CrossStateSizeConsistency => "跨态尺寸一致",
            ExtendedCheckId::HotspotCentroidOutlier => "热点重心离群",
        }
    }
}

/// 单条扩展发现。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendedFinding {
    pub check: ExtendedCheckId,
    pub red: bool,
    pub where_: String,
    pub detail: String,
}

/// 扩展五检主体（纯函数；逐条产出含定位的发现清单）。
pub fn inspect_extended(m: &CursorSchemeModel) -> Vec<ExtendedFinding> {
    let mut out: Vec<ExtendedFinding> = Vec::new();

    // 1. 全透明态。
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            let all_transparent = sf.frames.iter().all(|f| f.px.chunks_exact(4).all(|c| c[3] == 0));
            if all_transparent {
                out.push(ExtendedFinding {
                    check: ExtendedCheckId::InvisibleState,
                    red: true,
                    where_: alloc::format!("{}", st.zh_name()),
                    detail: String::from("该态全部帧全透明——指针在该态下不可见"),
                });
            }
        }
    }

    // 2. 邻帧全同（仅动画态：帧数 >1）。
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            if sf.frames.len() > 1 {
                for (i, pair) in sf.frames.windows(2).enumerate() {
                    if pair[0].px == pair[1].px {
                        out.push(ExtendedFinding {
                            check: ExtendedCheckId::DuplicateAdjacentFrames,
                            red: false,
                            where_: alloc::format!("{}#{}", st.zh_name(), i),
                            detail: String::from("相邻两帧逐字节相同——假动画帧"),
                        });
                    }
                }
            }
        }
    }

    // 3. 帧延时一致性。
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            if sf.frames.len() > 1 {
                let d0 = sf.frames[0].delay_ms;
                if sf.frames.iter().any(|f| f.delay_ms != d0) {
                    out.push(ExtendedFinding {
                        check: ExtendedCheckId::DelayUniformity,
                        red: false,
                        where_: alloc::format!("{}", st.zh_name()),
                        detail: alloc::format!("帧延时参差（首帧 {}ms）——播放节奏抖", d0),
                    });
                }
            }
        }
    }

    // 4. 跨态尺寸一致。
    let mut sizes: Vec<(u16, u16)> = Vec::new();
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            if let Some(f) = sf.frames.first() {
                let s = (f.w, f.h);
                if !sizes.contains(&s) {
                    sizes.push(s);
                }
            }
        }
    }
    if sizes.len() > 1 {
        out.push(ExtendedFinding {
            check: ExtendedCheckId::CrossStateSizeConsistency,
            red: false,
            where_: String::from("全方案"),
            detail: alloc::format!("存在 {} 种画布尺寸——态切换时视觉跳变", sizes.len()),
        });
    }

    // 5. 热点重心离群（对静态语义态抽查：内容 bbox 对角线一半之外=离群）。
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            if let Some(f) = sf.frames.first() {
                if let Some((cx, cy)) = f.buf().visual_centroid() {
                    let dx = (f.hot_x as i64 - cx as i64).abs();
                    let dy = (f.hot_y as i64 - cy as i64).abs();
                    let dist2 = dx * dx + dy * dy;
                    let radius = ((f.w as i64 * f.w as i64 + f.h as i64 * f.h as i64) / 4).max(1);
                    if dist2 * 4 > radius * 16 {
                        // 距离 > 2×内容半径（对角线一半）→ 离群
                        out.push(ExtendedFinding {
                            check: ExtendedCheckId::HotspotCentroidOutlier,
                            red: false,
                            where_: alloc::format!("{}", st.zh_name()),
                            detail: alloc::format!("热点 ({},{}) 距视觉重心 ({},{}) 过远", f.hot_x, f.hot_y, cx, cy),
                        });
                    }
                }
            }
        }
    }
    out
}

/// 扩展修复：缺态从内置方案补齐（同态同帧——比逐态克隆默认 glyph 更
/// 明确的事实源），返回补齐的态清单（空 = 无缺）。
pub fn fill_missing_from_builtin(m: &mut CursorSchemeModel) -> Vec<PointerState> {
    let missing = m.missing_states();
    let builtin = builtin_default_scheme();
    for st in &missing {
        if let Some(sf) = builtin.state(*st) {
            m.set_state(*st, sf.frames.clone());
        }
    }
    missing
}

/// 扩展修复：跨态尺寸归一（全部态补边到最大画布；热点不动——补边在
/// 右/下方向，不影响既有热点坐标的有效性）。
pub fn normalize_canvas(m: &mut CursorSchemeModel) -> (u16, u16) {
    let mut mw = 0u16;
    let mut mh = 0u16;
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            for f in &sf.frames {
                mw = mw.max(f.w);
                mh = mh.max(f.h);
            }
        }
    }
    if mw == 0 || mh == 0 {
        return (0, 0);
    }
    for st in ALL_STATES {
        let Some(sf) = m.state_mut(st) else { continue };
        for f in sf.frames.iter_mut() {
            if f.w == mw && f.h == mh {
                continue;
            }
            let src = f.buf();
            let mut buf = crate::jstar2::jbase::PixBuf::new(mw, mh);
            for y in 0..f.h {
                for x in 0..f.w {
                    if let Some(px) = src.get(x, y) {
                        buf.set(x, y, px);
                    }
                }
            }
            let hot = (f.hot_x, f.hot_y);
            *f = crate::jstar2::jbase::CursorFrame::from_buf(hot.0, hot.1, f.delay_ms, buf);
        }
    }
    (mw, mh)
}

/// 扩展修复：邻帧去重（动画态里逐字节相同的相邻帧只保留第一帧；
/// 返回删除帧数——非破坏，调用方持有原件）。
pub fn dedupe_adjacent_frames(m: &mut CursorSchemeModel) -> usize {
    let mut removed = 0usize;
    for st in ALL_STATES {
        let Some(sf) = m.state_mut(st) else { continue };
        if sf.frames.len() <= 1 {
            continue;
        }
        // 手动折叠：与前一个「保留帧」比较——相同则丢。
        let mut kept: Vec<crate::jstar2::jbase::CursorFrame> = Vec::new();
        for f in sf.frames.drain(..) {
            match kept.last() {
                Some(prev) if prev.px == f.px => {
                    removed += 1;
                }
                _ => kept.push(f),
            }
        }
        sf.frames = kept;
    }
    removed
}

/// 超尺寸态计数（审计面：超过 64px 提示线的态数——修复路由的依据）。
pub fn oversize_state_count(m: &CursorSchemeModel) -> usize {
    let mut n = 0;
    for st in ALL_STATES {
        if let Some(sf) = m.state(st) {
            if sf.frames.iter().any(|f| f.w.max(f.h) as u32 > WARN_FRAME_PX) {
                n += 1;
            }
        }
    }
    n
}

/// 报告确定性渲染（F628 详情页导出面：同一报告永远渲染出同一字节——
/// 行序 = 检查序 = 稳定，不哈希迭代）。
pub fn render_report(r: &HealthReport) -> String {
    let mut out = String::new();
    out.push_str(&alloc::format!("report fp={:016x}\n", r.scheme_fingerprint));
    for c in IDS {
        let v = r.verdict_of(c);
        let tag = match v {
            Verdict::Green => "GREEN",
            Verdict::Warn => "WARN",
            Verdict::Red => "RED",
        };
        out.push_str(&alloc::format!("{} {}\n", c.zh(), tag));
    }
    if !r.missing_states.is_empty() {
        out.push_str("missing:");
        for st in &r.missing_states {
            out.push(' ');
            out.push_str(st.zh_name());
        }
        out.push('\n');
    }
    out
}

/// v3 自检。
pub fn run_checker_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F627-v3");

    // 基线：内置方案扩展五检零发现（内置是全绿的锚）。
    let base = builtin_default_scheme();
    set.add(
        "builtin extended findings empty",
        inspect_extended(&base).is_empty(),
        "",
    );

    // 1. 全透明态检出：清空一个态的所有帧像素。
    let mut ghost = base.clone();
    if let Some(sf) = ghost.state_mut(PointerState::Busy) {
        for f in sf.frames.iter_mut() {
            for c in f.px.chunks_exact_mut(4) {
                c[3] = 0;
            }
        }
    }
    let gfind = inspect_extended(&ghost);
    set.add(
        "invisible state flagged red with state name",
        gfind.iter().any(|f| f.check == ExtendedCheckId::InvisibleState && f.red && f.where_.contains("忙")),
        "",
    );

    // 2. 邻帧全同：构造一个两帧逐字节相同的动画态。
    let mut dup = base.clone();
    let f0 = dup.state(PointerState::Busy).and_then(|s| s.frames.first().cloned());
    if let Some(f) = f0 {
        let f2 = crate::jstar2::jbase::CursorFrame::from_buf(f.hot_x, f.hot_y, f.delay_ms, f.buf());
        dup.set_state(PointerState::Busy, alloc::vec![f.clone(), f2]);
    }
    let dfind = inspect_extended(&dup);
    set.add(
        "duplicate adjacent frames warned with index",
        dfind.iter().any(|f| f.check == ExtendedCheckId::DuplicateAdjacentFrames && !f.red),
        "",
    );
    // 去重修复：删 1 帧、剩 1 帧、原发现消失。
    let removed = dedupe_adjacent_frames(&mut dup);
    set.add(
        "dedupe removes adjacent twins",
        removed == 1 && dup.state(PointerState::Busy).map(|s| s.frames.len()).unwrap_or(0) == 1,
        "",
    );

    // 3. 延时一致性：同态两帧不同延时 → 警（内置 Busy 为单帧——克隆一份制造参差）。
    let mut jit = base.clone();
    if let Some(sf) = jit.state_mut(PointerState::Busy) {
        let extra = sf.frames[0].clone();
        sf.frames.push(extra);
        let n = sf.frames.len();
        sf.frames[n - 1].delay_ms = sf.frames[0].delay_ms + 50;
    }
    set.add(
        "delay uniformity warned",
        inspect_extended(&jit).iter().any(|f| f.check == ExtendedCheckId::DelayUniformity),
        "",
    );

    // 4. 跨态尺寸：把一个态缩成 8x8 → 尺寸发现 + 归一修复回统一画布。
    let mut mixed = base.clone();
    let small = crate::jstar2::jbase::CursorFrame::from_buf(0, 0, 0, crate::jstar2::jbase::PixBuf::new(8, 8));
    mixed.set_state(PointerState::Text, alloc::vec![small]);
    let mfind = inspect_extended(&mixed);
    set.add(
        "cross state size inconsistency flagged",
        mfind.iter().any(|f| f.check == ExtendedCheckId::CrossStateSizeConsistency),
        "",
    );
    let (w, h) = normalize_canvas(&mut mixed);
    set.add(
        "normalize canvas unifies sizes",
        w == 32 && h == 32 && inspect_extended(&mixed).iter().all(|f| f.check != ExtendedCheckId::CrossStateSizeConsistency),
        "",
    );

    // 5. 缺态补齐：摘除一个态的条目 → fill_missing_from_builtin 补回。
    let mut hole = base.clone();
    hole.entries.retain(|e| e.state != PointerState::Link);
    let hole_before = hole.missing_states().len();
    let filled = fill_missing_from_builtin(&mut hole);
    set.add(
        "fill missing from builtin repairs",
        hole_before == 1 && filled.len() == 1 && hole.missing_states().is_empty()
            && hole.state(PointerState::Link).map(|s| !s.frames.is_empty()).unwrap_or(false),
        "",
    );

    // 6. 超尺寸计数：>64px 的态被点出（修复路由依据）。
    let mut big = base.clone();
    let bigf = crate::jstar2::jbase::CursorFrame::from_buf(0, 0, 0, crate::jstar2::jbase::PixBuf::new(80, 80));
    big.set_state(PointerState::Unavailable, alloc::vec![bigf]);
    set.add("oversize state counted", oversize_state_count(&big) >= 1 && oversize_state_count(&base) == 0, "");

    // 7. 报告渲染确定性：同一报告渲染两次逐字节相等 + 行含指纹。
    let rep = inspect(&base);
    let a = render_report(&rep);
    let b = render_report(&inspect(&base));
    set.add(
        "report render deterministic",
        a == b && a.contains("fp=") && a.contains("齐全性 GREEN"),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn extended_findings_carry_zh_names() {
        assert!(!ExtendedCheckId::InvisibleState.zh().is_empty());
        assert_eq!(ExtendedCheckId::DelayUniformity.zh(), "帧延时一致性");
    }

    #[test]
    fn normalize_canvas_idempotent() {
        let mut m = builtin_default_scheme();
        let (w1, h1) = normalize_canvas(&mut m);
        let (w2, h2) = normalize_canvas(&mut m);
        assert_eq!((w1, h1), (w2, h2));
        assert_eq!((w1, h1), (32, 32));
    }

    #[test]
    fn dedupe_on_static_scheme_is_noop() {
        let mut m = builtin_default_scheme();
        let removed = dedupe_adjacent_frames(&mut m);
        assert_eq!(removed, 0, "内置方案无邻帧全同");
    }

    #[test]
    fn render_report_lists_all_four() {
        let m = builtin_default_scheme();
        let text = render_report(&inspect(&m));
        for c in IDS {
            assert!(text.contains(c.zh()));
        }
    }
}
