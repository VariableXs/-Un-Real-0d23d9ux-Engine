//! cgm07_pacemodel_checks — CGPU-F0486 域自检（三模式/权衡声明/适配/探测/编排）。
//!
//! 判据纪律：预期值判据侧手算写死；零 panic 面。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::vec::Vec;

use super::cgm07_pacemodel::{
    AdaptiveVsync, GapPredictor, ModeProbe, MultiMonPace, PaceVerifier, PresentMode,
    ProbeSignals, WindowMode, XcCode, PACE_MODEL_VERSION, PROMISE_GAP_ADAPT_US,
};

/// F0486 判据总入口。
pub fn run_cgm07_checks() -> CheckSet {
    let mut checks = CheckSet::new("cgpu-f0486");

    // ---- 三模式族（模式-01..03）----
    // 模式-01 闭集三值互异。
    let distinct = PresentMode::VSync != PresentMode::Unsynced
        && PresentMode::Unsynced != PresentMode::Composer
        && PresentMode::VSync != PresentMode::Composer;
    checks.add("模式-01-三值互异", distinct, "VSync/Unsynced/Composer 两两互异");

    // 模式-02 ordinal 连续 0/1/2。
    let ord_ok = PresentMode::VSync.ordinal() == 0
        && PresentMode::Unsynced.ordinal() == 1
        && PresentMode::Composer.ordinal() == 2
        && PresentMode::ALL.len() == 3;
    checks.add("模式-02-序连续", ord_ok, "ordinal 0/1/2 与 ALL 覆盖闭集");

    // 模式-03 版本冻结标识。
    checks.add(
        "模式-03-版本冻结",
        PACE_MODEL_VERSION == "M07-pacemodel-v1",
        "模型版本号即冻结标识",
    );

    // ---- 权衡族（权衡-01..04）----
    // 权衡-01 撕裂风险表：VSync0/Unsynced3/Composer0（判据侧手算）。
    checks.add(
        "权衡-01-撕裂表",
        PresentMode::VSync.tear_risk() == 0
            && PresentMode::Unsynced.tear_risk() == 3
            && PresentMode::Composer.tear_risk() == 0,
        "无同步撕裂风险最高，两同步模式零撕裂",
    );

    // 权衡-02 延迟成本表：VSync2/Unsynced0/Composer1。
    checks.add(
        "权衡-02-延迟表",
        PresentMode::VSync.latency_cost() == 2
            && PresentMode::Unsynced.latency_cost() == 0
            && PresentMode::Composer.latency_cost() == 1,
        "VSync 延迟最高（等空区），无同步最低",
    );

    // 权衡-03 抖动成本表：VSync0/Unsynced2/Composer1。
    checks.add(
        "权衡-03-抖动表",
        PresentMode::VSync.jitter_cost() == 0
            && PresentMode::Unsynced.jitter_cost() == 2
            && PresentMode::Composer.jitter_cost() == 1,
        "VSync 恒定周期零抖动",
    );

    // 权衡-04 对照表与逐项一致 + 策略文本非空。
    let t = PaceVerifier::tradeoff_table();
    let table_ok = t[0] == [0, 2, 0] && t[1] == [3, 0, 2] && t[2] == [0, 1, 1]
        && !PresentMode::VSync.pace_policy().is_empty()
        && !PresentMode::Unsynced.pace_policy().is_empty()
        && !PresentMode::Composer.pace_policy().is_empty();
    checks.add(
        "权衡-04-对照表",
        table_ok,
        "tradeoff_table 三行与逐项评分一致且策略文本闭集非空",
    );

    // ---- 适配族（适配-01..04）----
    // 适配-01 全屏独占三模式皆可。
    let fe_ok = WindowMode::FullscreenExclusive.allows(PresentMode::VSync)
        && WindowMode::FullscreenExclusive.allows(PresentMode::Unsynced)
        && WindowMode::FullscreenExclusive.allows(PresentMode::Composer);
    checks.add("适配-01-独占直通", fe_ok, "全屏独占绕过合成器三模式直通");

    // 适配-02 无边框禁 VSync（VSync 语义由 D02 抑制联动接管）。
    let bl_ok = !WindowMode::Borderless.allows(PresentMode::VSync)
        && WindowMode::Borderless.allows(PresentMode::Composer)
        && WindowMode::Borderless.allows(PresentMode::Unsynced);
    checks.add("适配-02-无边框禁VSync", bl_ok, "无边框必经合成器：VSync 语义不成立");

    // 适配-03 窗口化同无边框。
    let w_ok = !WindowMode::Windowed.allows(PresentMode::VSync)
        && WindowMode::Windowed.allows(PresentMode::Composer);
    checks.add("适配-03-窗口化同律", w_ok, "窗口化必经合成器同无边框纪律");

    // 适配-04 适配说明含 D02 联动声明。
    checks.add(
        "适配-04-D02声明",
        WindowMode::Borderless.adapt_note().contains("D02"),
        "无边框适配说明含 D02 抑制联动（跨批对接显性）",
    );

    // ---- 探测族（探测-01..05）----
    // 探测-01 锁相+低抖+无合成器 → VSync：8×16667（抖动 0）。
    let sig_v = ProbeSignals {
        gaps_us: vec![16_667; 8],
        refresh_us: 16_667,
        composer_active: false,
    };
    checks.add(
        "探测-01-VSync识别",
        ModeProbe::detect(&sig_v) == Ok(PresentMode::VSync),
        "锁相偏差 0 + 抖动 0 + 无合成器 → VSync",
    );

    // 探测-02 锁相+低抖+合成器活跃 → Composer。
    let sig_c = ProbeSignals {
        gaps_us: vec![16_667; 8],
        refresh_us: 16_667,
        composer_active: true,
    };
    checks.add(
        "探测-02-Composer识别",
        ModeProbe::detect(&sig_c) == Ok(PresentMode::Composer),
        "同信号合成器活跃 → Composer（二分律）",
    );

    // 探测-03 大抖不锁相 → Unsynced：16667/40000/25000/16667（抖 23333>2ms，
    // 中位数 (16667+25000)/2 排序后 [16667,16667,25000,40000] med=idx2=25000，
    // 偏差 = 8333×1e6/16667 = 499970ppm >100000 不锁相）。
    let sig_u = ProbeSignals {
        gaps_us: vec![16_667, 40_000, 25_000, 16_667],
        refresh_us: 16_667,
        composer_active: false,
    };
    checks.add(
        "探测-03-Unsynced识别",
        ModeProbe::detect(&sig_u) == Ok(PresentMode::Unsynced),
        "抖 23333µs>2ms 且偏差 499970ppm>10% → Unsynced",
    );

    // 探测-04 歧义显性拒：锁相但抖动 1.5ms（介于 1ms 与 2ms 之间）。
    let sig_a = ProbeSignals {
        gaps_us: vec![16_667, 18_167],
        refresh_us: 16_667,
        composer_active: false,
    };
    checks.add(
        "探测-04-歧义拒",
        ModeProbe::detect(&sig_a) == Err(XcCode::PmProbeAmbiguous),
        "抖 1500µs 落在灰区：拒绝武断结论",
    );

    // 探测-05 空信号与零间隔拒。
    let sig_e = ProbeSignals { gaps_us: Vec::new(), refresh_us: 16_667, composer_active: false };
    let sig_z = ProbeSignals {
        gaps_us: vec![0],
        refresh_us: 16_667,
        composer_active: false,
    };
    checks.add(
        "探测-05-非法拒",
        ModeProbe::detect(&sig_e) == Err(XcCode::PmProbeAmbiguous)
            && ModeProbe::detect(&sig_z) == Err(XcCode::PmGapZero),
        "空信号歧义拒、零间隔显性拒",
    );

    // ---- 编排族（编排-01..07）----
    // 编排-01 适配矩阵拒：Borderless+VSync 构造即拒。
    checks.add(
        "编排-01-组合拒",
        matches!(
            PaceVerifier::new(PresentMode::VSync, WindowMode::Borderless, 16_667),
            Err(XcCode::PmComboUnsupported)
        ),
        "无边框+VSync 组合适配矩阵直接拒",
    );

    // 编排-02 VSync 平间隔 verify Ok：16667±200 抖 200 ≤1000。
    let mut pv = match PaceVerifier::new(PresentMode::VSync, WindowMode::FullscreenExclusive, 16_667) {
        Ok(p) => p,
        Err(_) => {
            checks.add("编排-02-平间隔过", false, "构造失败");
            // 兜底返回（零 panic 面）：构造失败直接出后续判据红。
            return checks;
        }
    };
    let mut i = 0;
    while i < 8 {
        let _ = pv.feed(16_667 + (i % 2) * 200);
        i += 1;
    }
    checks.add(
        "编排-02-平间隔过",
        pv.verify() == Ok(()) && pv.cases == 0,
        "抖 200µs ≤ VSync 上界 1ms：过",
    );

    // 编排-03 VSync 大抖立案：喂 16667 与 20000（抖 3333>1000）。
    let mut pv2 = match PaceVerifier::new(PresentMode::VSync, WindowMode::FullscreenExclusive, 16_667) {
        Ok(p) => p,
        Err(_) => {
            checks.add("编排-03-大抖立案", false, "构造失败");
            return checks;
        }
    };
    let _ = pv2.feed(16_667);
    let _ = pv2.feed(20_000);
    checks.add(
        "编排-03-大抖立案",
        pv2.verify() == Err(XcCode::PmJitterOver) && pv2.cases == 1,
        "抖 3333µs > VSync 上界：行为漂移立案",
    );

    // 编排-04 Unsynced 不判抖动：同款大抖过（撕裂风险已在权衡声明显性接受）。
    let mut pv3 = match PaceVerifier::new(PresentMode::Unsynced, WindowMode::FullscreenExclusive, 16_667) {
        Ok(p) => p,
        Err(_) => {
            checks.add("编排-04-无同步豁免", false, "构造失败");
            return checks;
        }
    };
    let _ = pv3.feed(16_667);
    let _ = pv3.feed(20_000);
    checks.add(
        "编排-04-无同步豁免",
        pv3.verify() == Ok(()),
        "Unsynced 抖动上界 ∞：不判抖动",
    );

    // 编排-05 多屏分频：主 120 从 60 → 2；非整数倍偏差留痕（75Hz：
    // 120/75=1 → eff=75 偏差 375000ppm）；零值防御 1。
    checks.add(
        "编排-05-多屏分频",
        MultiMonPace::divider(120, 60) == 2
            && MultiMonPace::divider(60, 120) == 1
            && MultiMonPace::divider(0, 60) == 1
            && MultiMonPace::misalign_ppm(120, 60) == 0
            && MultiMonPace::misalign_ppm(120, 75) == 375_000,
        "整数倍分频偏差 0；非整数倍 375000ppm 留痕（判据侧手算）",
    );

    // 编排-06 间隔预测器 EWMA：feed 8×16667 → predict=16667；
    // submit_early(16667, 1000)=false（不早于）；feed 变快后 ewma 下降。
    let mut gp = GapPredictor::new();
    let mut i = 0;
    while i < 8 {
        gp.feed(16_667);
        i += 1;
    }
    // 稳态（整数除逐次精确手算）：首样 16667，每次迭代 ewma =
    // floor(3x/4)+floor(x/4)：x≡3→16666、x≡2→16665、x≡1→16664、
    // x=16664 mod 4=0 无损耗稳定。8 次 feed → 16664。
    let ewma_stable = gp.predict_us() == 16_664;
    let early_no = !gp.submit_early(16_667, 1_000);
    gp.feed(8_000);
    gp.feed(8_000);
    // 两次 feed（整数除精确手算）：feed1: floor(16664×3/4)=12498 +
    // floor(8000/4)=2000 → 14498；feed2: floor(14498×3/4)=10873 + 2000
    // → 12873。
    let ewma_drop = gp.predict_us() == 12_873;
    checks.add(
        "编排-06-EWMA预测",
        ewma_stable && early_no && ewma_drop,
        "稳态 ewma=16664（整数除三次损耗后 mod4=0 稳定）；两次 8000 后 ewma=12873",
    );

    // 编排-07 自适应 VSync 迟滞：超 15625 立即降 Unsynced（降档即时）；
    // 8 好帧回 VSync（升档迟滞）；好帧计数对账。
    let mut av = AdaptiveVsync::new();
    let d1 = av.on_frame(20_000);
    let drop_now = d1 == PresentMode::Unsynced;
    let mut cur = PresentMode::Unsynced;
    let mut i = 0;
    while i < 7 {
        cur = av.on_frame(12_000);
        i += 1;
    }
    let still_unsynced = cur == PresentMode::Unsynced && av.good_run() == 7;
    let back = av.on_frame(12_000);
    checks.add(
        "编排-07-迟滞策略",
        drop_now && still_unsynced && back == PresentMode::VSync,
        "降档即时、升档需 8 连好帧（防横跳迟滞）且容带 15625=125%×12500",
    );

    // 容带常量独立对拍。
    checks.add(
        "编排-08-容带对拍",
        PROMISE_GAP_ADAPT_US == 15_625,
        "贴线判据 = 125%×12500µs = 15625µs（判据侧手算）",
    );

    checks
}
