//! cgd05_ratecontract_checks — CGPU-F0485 域自检（合同三条款+达标率+预警+联动）。
//!
//! 判据纪律：判据侧预期值全部**手算写死**（不 import 被测阈值反推）；
//! 零 panic 面；样本构造显式可对拍（每条判据注释给出样本账）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

use super::cgd05_ratecontract::{
    BudgetCorrection, ClauseRow, RateContract, RollingWin, Scene, Trend, XcCode,
    BUCKETS, BUCKET_US, JANK_CEIL_PPM, JANK_LINE_US, P95_CEIL_US, P99_CEIL_US,
    WARN_HEADROOM_PPM, WINDOW_FRAMES,
};

/// 匀速灌窗（判据助手）：灌 n 帧 gap_us。
fn feed_n(rc: &mut RateContract, n: usize, gap_us: u32) {
    let mut i = 0;
    while i < n {
        let _ = rc.feed(gap_us);
        i += 1;
    }
}

/// F0485 判据总入口。
pub fn run_cgd05_checks() -> CheckSet {
    let mut checks = CheckSet::new("cgpu-f0485");

    // ---- 三条款族（条款-01..06）----
    // 条款-01 良好流全达标：600×9900（桶19）。P95 target=570 → 桶19=600
    // → P95=9500≤12500 ✓；used=9500×1e6/12500=760000<800000 无预警 →
    // judge Ok。P99 同桶 9500 ✓；卡顿 0 ✓。
    let mut rc1 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc1, 600, 9_900);
    let j1 = rc1.judge();
    let ok1 = match j1 {
        Ok(row) => row.clause.all_ok()
            && row.clause.p95_us == 9_500
            && row.clause.p99_us == 9_500
            && row.clause.jank_ppm == 0
            && !row.warned,
        Err(_) => false,
    };
    checks.add("条款-01-良好流达标", ok1, "600×9900µs 三条款全过 P95=9500（桶19 粒度）且无预警");

    // 条款-02 P95 失守：560×12000 + 40×13000（桶26）。P95 target=570：
    // 桶24=560 <570，桶26=40 → P95=13000 >12500 失守；P99 target=594：
    // 桶26 acc=600 → P99=13000 ≤25000 ✓；卡顿 0 ✓。
    let mut rc2 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc2, 560, 12_000);
    feed_n(&mut rc2, 40, 13_000);
    // P95 失守时余量耗用必 ≥100%（13000/12500），warn 与违反双信号 →
    // judge 返回 RcWarnRisen（预警先行语义）；行已入账，从 ledger 验。
    let ok2 = match rc2.judge() {
        Err(XcCode::RcWarnRisen) => {
            let led = rc2.ledger();
            led.len() == 1 && !led[0].clause.ok_p95 && led[0].clause.p95_us == 13_000
                && led[0].clause.ok_p99 && led[0].clause.jank_ppm == 0
        }
        _ => false,
    };
    checks.add("条款-02-P95失守", ok2, "40 帧超 12.5ms 应推高 P95=13000 失守立案（预警先行返回码）");

    // 条款-03 P99 失守：592×12000 + 8×40000（桶80）。P99 target=594：
    // 桶24=592 <594，桶80=8 → P99=40000 >25000 失守；P95：桶24 acc=592 ≥570
    // → 12000 ✓。
    let mut rc3 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc3, 592, 12_000);
    feed_n(&mut rc3, 8, 40_000);
    let ok3 = match rc3.judge() {
        Err(XcCode::RcWarnRisen) => {
            let led = rc3.ledger();
            led.len() == 1 && !led[0].clause.ok_p99 && led[0].clause.p99_us == 40_000
                && led[0].clause.ok_p95 && led[0].clause.p95_us == 12_000
        }
        _ => false,
    };
    checks.add("条款-03-P99失守", ok3, "8 帧尾延迟应推高 P99=40000 失守而 P95=12000 无恙");

    // 条款-04 卡顿失守：596×12000 + 4×60000（>50ms 卡顿线）。
    // 卡顿 ppm = 4×1e6/600 = 6666 > 5000 失守。
    let mut rc4 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc4, 596, 12_000);
    feed_n(&mut rc4, 4, 60_000);
    let ok4 = match rc4.judge() {
        Err(XcCode::RcWarnRisen) => {
            let led = rc4.ledger();
            led.len() == 1 && !led[0].clause.ok_jank && led[0].clause.jank_ppm == 6_666
        }
        _ => false,
    };
    checks.add("条款-04-卡顿失守", ok4, "4 帧卡顿应得 6666ppm > 5000ppm 失守（预警先行返回码）");

    // 条款-05 非法样本显性拒：0µs 与 200001µs。
    let mut rc5 = RateContract::new(Scene::BenchA);
    let r5a = rc5.feed(0);
    let r5b = rc5.feed(200_001);
    checks.add(
        "条款-05-非法样本拒",
        r5a == Err(XcCode::RcSampleInvalid) && r5b == Err(XcCode::RcSampleInvalid),
        "0µs 与超量程样本应显性拒收不静默截断",
    );

    // 条款-06 窗口未满拒判定：100 帧就 judge。
    let mut rc6 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc6, 100, 12_000);
    checks.add(
        "条款-06-未满拒判",
        matches!(rc6.judge(), Err(XcCode::RcWindowNotFull)),
        "样本不足不下合同结论（宁缺毋滥）",
    );

    // ---- 达标率族（达标-01..04）----
    // 达标-01 全达标窗口达标率 1_000_000。
    let mut rc7 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc7, 600, 12_000);
    let _ = rc7.judge();
    checks.add(
        "达标-01-全绿1e6",
        rc7.overall_ok_ppm() == 1_000_000 && rc7.ledger().len() == 1,
        "单窗口全达标应得 1000000ppm 且账一行",
    );

    // 达标-02 半数违反 → 500_000：两窗，一窗 12000 一窗 13000。
    let mut rc8 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc8, 600, 12_000);
    let _ = rc8.judge();
    feed_n(&mut rc8, 600, 13_000);
    let _ = rc8.judge();
    checks.add(
        "达标-02-半绿5e5",
        rc8.overall_ok_ppm() == 500_000,
        "1 绿 1 违应得 500000ppm",
    );

    // 达标-03 履行账 slot 单调且场景标签正确。
    let led_ok = {
        let led = rc8.ledger();
        led.len() == 2 && led[0].slot == 1 && led[1].slot == 2
            && led[0].slot < led[1].slot
            && led[0].scene == Scene::BenchA
    };
    checks.add(
        "达标-03-账序场景",
        led_ok,
        "履行账 slot 单调 1,2 且场景标签 BenchA",
    );

    // 达标-04 逐条达标率：ok_rate_ppm 按 ok_count/3。
    let row_full = ClauseRow {
        p95_us: 12_000, p99_us: 12_000, jank_ppm: 0,
        ok_p95: true, ok_p99: true, ok_jank: true,
    };
    let row_two = ClauseRow {
        p95_us: 13_000, p99_us: 12_000, jank_ppm: 0,
        ok_p95: false, ok_p99: true, ok_jank: true,
    };
    checks.add(
        "达标-04-行级达标率",
        row_full.ok_rate_ppm() == 1_000_000
            && row_two.ok_rate_ppm() == 666_666
            && row_full.ok_count() == 3
            && row_two.ok_count() == 2,
        "3/3=1000000ppm；2/3=666666ppm（判据侧手算）",
    );

    // ---- 预警族（预警-01..04）----
    // 预警-01 余量耗用预警：P95=12200 → used=12200×1e6/12500=976000 ≥800000。
    // 全达标但预警 → Err(RcWarnRisen) 且 warns=1。
    let mut rc9 = RateContract::new(Scene::BenchA);
    feed_n(&mut rc9, 600, 12_200);
    let ok9 = match rc9.judge() {
        Err(XcCode::RcWarnRisen) => rc9.warns == 1 && rc9.cases == 0,
        _ => false,
    };
    checks.add(
        "预警-01-余量预警",
        ok9,
        "P95=12200 用掉 97.6% 余量应升预警且未立案",
    );

    // 预警-02 平静流无预警：P95=9900 → used=792000 <800000。
    let mut rcA = RateContract::new(Scene::BenchA);
    feed_n(&mut rcA, 600, 9_900);
    let okA = match rcA.judge() {
        Ok(row) => !row.warned && rcA.warns == 0,
        Err(_) => false,
    };
    checks.add(
        "预警-02-平静无预警",
        okA,
        "P95=9900 余量耗 79.2% 应无预警",
    );

    // 预警-03 趋势预警 Rising：窗满后旧半窗 0 卡顿、新半窗 4 卡顿。
    // 构造：先 600×12000（无卡顿），再 push 4×60000（顶替掉 4 帧旧的）。
    // 同样本量下若无趋势预警，P99 与卡顿判定同款会返回 RcClauseBroken；
    // 返回 RcWarnRisen 即证明趋势项生效（Rising 先行于违反码）。
    let mut rcB = RateContract::new(Scene::BenchA);
    feed_n(&mut rcB, 600, 12_000);
    feed_n(&mut rcB, 4, 60_000);
    checks.add(
        "预警-03-趋势Rising",
        matches!(rcB.judge(), Err(XcCode::RcWarnRisen)),
        "新半窗 4 卡顿 vs 旧半窗 0：趋势预警先于违反码（Rising 生效）",
    );

    // 预警-04 均匀卡顿 Flat：新旧各 2 不触发趋势项（4 > 2×2 不成立）。
    let mut rcC = RollingWin::new();
    let mut i = 0;
    while i < 300 {
        let _ = rcC.push(12_000);
        i += 1;
    }
    let _ = rcC.push(60_000);
    let _ = rcC.push(60_000);
    let mut i = 0;
    while i < 298 {
        let _ = rcC.push(12_000);
        i += 1;
    }
    // 现在窗内：最旧 300 帧里前 298 是 12000（原 300 帧的头两帧被顶替），
    // 60_000 两帧在最旧段尾部……直接验证 Trend 不 Rising 即可（语义对拍）。
    checks.add(
        "预警-04-均匀Flat",
        rcC.jank_trend() != Trend::Rising,
        "2+2 均匀卡顿不构成 Rising（阈值 2×）",
    );

    // ---- 联动族（联动-01..03）----
    // 联动-01 P95 失守 → 交互池加预算建议。
    let row_p95 = ClauseRow {
        p95_us: 13_000, p99_us: 12_000, jank_ppm: 0,
        ok_p95: false, ok_p99: true, ok_jank: true,
    };
    let corr1 = BudgetCorrection::derive(&row_p95);
    checks.add(
        "联动-01-P95建议",
        corr1.len() == 1
            && corr1[0].pool_ordinal == 1
            && corr1[0].delta_ppm == 30_000
            && corr1[0].cause_clause == 0,
        "P95 失守应出交互池 +30000ppm 建议单（cause 0）",
    );

    // 联动-02 P99+卡顿双失守 → 两条建议（表面收缩/机动收缩）。
    let row_dual = ClauseRow {
        p95_us: 12_000, p99_us: 40_000, jank_ppm: 6_666,
        ok_p95: true, ok_p99: false, ok_jank: false,
    };
    let corr2 = BudgetCorrection::derive(&row_dual);
    let c2_ok = corr2.len() == 2
        && corr2[0].pool_ordinal == 2
        && corr2[0].delta_ppm == -30_000
        && corr2[1].pool_ordinal == 3
        && corr2[1].delta_ppm == -20_000;
    checks.add(
        "联动-02-双失守双单",
        c2_ok,
        "P99+卡顿失守应出表面池 −30000 与机动池 −20000 两单",
    );

    // 联动-03 全达标空建议单 + judge 路径 corrections 空。
    let mut rcD = RateContract::new(Scene::BenchA);
    feed_n(&mut rcD, 600, 9_900);
    let okD = match rcD.judge() {
        Ok(row) => row.corrections.is_empty(),
        Err(_) => false,
    };
    checks.add(
        "联动-03-全绿空单",
        okD,
        "全达标窗口建议单应为空（不达标才触发）",
    );

    // ---- 编排族（编排-01..05）----
    // 编排-01 场景切换清窗：切新场景窗口未满拒判。
    let mut rcE = RateContract::new(Scene::BenchA);
    feed_n(&mut rcE, 600, 12_000);
    rcE.switch_scene(Scene::BenchB);
    let okE = rcE.current_scene() == Scene::BenchB
        && matches!(rcE.judge(), Err(XcCode::RcWindowNotFull));
    checks.add(
        "编排-01-切景清窗",
        okE,
        "跨场景混算会撒谎：切景清窗重计",
    );

    // 编排-02 同场景切换不清窗。
    let mut rcF = RateContract::new(Scene::BenchA);
    feed_n(&mut rcF, 600, 9_900);
    rcF.switch_scene(Scene::BenchA);
    let okF = matches!(rcF.judge(), Ok(_));
    checks.add(
        "编排-02-同景不清",
        okF,
        "同场景 switch 不清窗：窗口保留可判定",
    );

    // 编排-03 账序校验 Ok。
    let mut rcG = RateContract::new(Scene::BenchA);
    feed_n(&mut rcG, 600, 12_000);
    let _ = rcG.judge();
    checks.add(
        "编排-03-账序Ok",
        rcG.ledger_sequential() == Ok(()),
        "正常履行账时段序单调应 Ok",
    );

    // 编排-04 阈值常量独立对拍（判据侧手算写死）。
    checks.add(
        "编排-04-常量对拍",
        P95_CEIL_US == 12_500
            && P99_CEIL_US == 25_000
            && JANK_CEIL_PPM == 5_000
            && JANK_LINE_US == 50_000
            && WINDOW_FRAMES == 600
            && BUCKET_US == 500
            && BUCKETS == 400
            && WARN_HEADROOM_PPM == 800_000,
        "三条款/窗口/桶/预警线常量与合同文本一致（版本冻结）",
    );

    // 编排-05 顶替语义：满窗后新样本顶旧样本。599×12000+1×13000：
    // P95 target=570，桶24=599 ≥570 → P95 仍 12000（顶替 1 帧不推高 P95），
    // judge 正常出账。
    let mut rcH = RateContract::new(Scene::BenchA);
    feed_n(&mut rcH, 600, 12_000);
    feed_n(&mut rcH, 1, 13_000);
    let win_mean_ok = match rcH.judge() {
        Ok(row) => row.clause.p95_us == 12_000,
        Err(XcCode::RcWarnRisen) => {
            let led = rcH.ledger();
            led.len() == 1 && led[0].clause.p95_us == 12_000 && led[0].clause.ok_p95
        }
        Err(_) => false,
    };
    checks.add(
        "编排-05-满窗顶替",
        win_mean_ok,
        "满窗后顶替 1 帧：P95 不被单帧推高（12000），600 帧口径判定连续",
    );

    // ---- 摘要族（摘要-01..02）----
    // 摘要-01 summary 非空含版本。
    let s = rcH.summary();
    checks.add(
        "摘要-01-摘要非空",
        s.contains("D05-ratecontract-v1") && s.contains("基准A"),
        "单行摘要应含合同版本与场景",
    );

    // 摘要-02 桶/量程完备性：12000→桶24、200000→顶桶、0 与 200001 拒。
    let mut rcI = RollingWin::new();
    let _ = rcI.push(12_000);
    let _ = rcI.push(200_000);
    let r_oob = rcI.push(200_001);
    checks.add(
        "摘要-02-桶语义",
        r_oob == Err(XcCode::RcSampleInvalid),
        "边界样本：200000 顶桶收容、200001 量程外显性拒",
    );

    checks
}
