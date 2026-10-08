//! CGPU-F0482 · 全帧精确计时器域自检（判据五条逐条映射 + 反向语料钉门禁）。
//!
//! **判据（锚点原文）**：四段、双源、校准、开销、判据。
//!
//! 自检纪律：判据区零 panic 面；判据侧独立重算（四段时长/标定映射/开销
//! 比手工验算值写死对账）；反向语料证明门禁不恒绿（未闭段 None、倒序
//! 异常、非单调标定拒绝、漂移超阈标注、开销超标标注）。

use super::cgd02_frametimer::{
    drift_check, overhead_line, overhead_report, ClockAlign, DriftCheck, FrameStage, FrameTimer,
    FRAME_BUDGET_NS, FRAME_STAGES, FRAME_TIMER_VERSION, OVERHEAD_LIMIT_PPM, TIMESTAMP_SOURCES,
};

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = ["四段", "双源", "校准", "开销", "判据"];

/// CGPU-F0482 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgd02_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgd02_frametimer");

    // —— 判据一 · 四段：全链分段开合 + 时长 + 反向（未闭/倒序） ——
    let mut t = FrameTimer::new(7);
    t.open(FrameStage::InputSampling, 100);
    t.close(FrameStage::InputSampling, 300);
    t.open(FrameStage::Submit, 300);
    t.close(FrameStage::Submit, 700);
    t.open(FrameStage::GpuExecute, 700);
    t.close(FrameStage::GpuExecute, 8_200);
    t.open(FrameStage::Present, 8_200);
    // Present 未闭（反向：未闭段不报时长）。
    let d0 = t.duration_ns(FrameStage::InputSampling);
    let d2 = t.duration_ns(FrameStage::GpuExecute);
    let d3 = t.duration_ns(FrameStage::Present);
    let total_partial = t.total_ns();
    let reversed_probe = t.reversed_count();
    s.add(
        "D02-四段-开合时长与反向门禁",
        FRAME_STAGES.len() == 4
            && d0 == Some(200)
            && d2 == Some(7_500)
            && d3.is_none()
            && total_partial.is_none()
            && reversed_probe == 0
            && t.anomalies() == 0,
        "四段闭三段出时长（200/7500ns 判据侧手算对账）；未闭段与全帧合计如实 None（不报幻觉数字）",
    );

    // —— 重叠 + 倒序异常检测（分段账本能抓记账错误） ——
    // 重叠帧：Input [100,400] 与 Submit [300,600] 区间相交。
    let mut t2 = FrameTimer::new(8);
    t2.open(FrameStage::InputSampling, 100);
    t2.close(FrameStage::InputSampling, 400);
    t2.open(FrameStage::Submit, 300);
    t2.close(FrameStage::Submit, 600);
    // 倒序帧：闭点早于开点。
    let mut t4 = FrameTimer::new(10);
    t4.open(FrameStage::Submit, 500);
    t4.close(FrameStage::Submit, 400);
    // 正常帧：四段顺序衔接，总时长判据侧手算 100+400+7400+3900=11800。
    let mut t3 = FrameTimer::new(9);
    t3.open(FrameStage::InputSampling, 100);
    t3.close(FrameStage::InputSampling, 200);
    t3.open(FrameStage::Submit, 200);
    t3.close(FrameStage::Submit, 600);
    t3.open(FrameStage::GpuExecute, 600);
    t3.close(FrameStage::GpuExecute, 8_000);
    t3.open(FrameStage::Present, 8_000);
    t3.close(FrameStage::Present, 11_900);
    s.add(
        "D02-四段-重叠倒序异常检测",
        t2.has_overlap()
            && t2.anomalies() == 0
            && t4.duration_ns(FrameStage::Submit).is_none()
            && t4.reversed_count() == 1
            && t4.anomalies() == 1
            && !t3.has_overlap()
            && t3.anomalies() == 0
            && t3.total_ns() == Some(11_800),
        "相邻闭段区间相交即重叠（100-400∩300-600）；倒序段不出时长+异常计数；正常四段总时长判据侧手算 11800ns 对账",
    );

    // —— 判据二 · 双源：闭集齐备 + 漂移控制 ——
    let d_ok = drift_check(7_500_000, 7_510_000);
    let d_bad = drift_check(7_500_000, 7_600_000);
    let sources_ok = TIMESTAMP_SOURCES.len() == 2
        && TIMESTAMP_SOURCES.get(0).map(|x| x.label()) == Some("CPU-QPC")
        && TIMESTAMP_SOURCES.get(1).map(|x| x.label()) == Some("GPU时间戳");
    s.add(
        "D02-双源-闭集齐备+漂移控制",
        sources_ok
            && d_ok == DriftCheck {
                cpu_span_ns: 7_500_000,
                gpu_span_ns: 7_510_000,
                drift_ns: 10_000,
                drifted: false,
            }
            && d_bad.drifted
            && d_bad.drift_ns == 100_000,
        "QPC/GPU 双源闭集冻结；同段双源对账——漂移 10ns 在容差内、100ns 超容差标注（漂移控制可判定）",
    );

    // —— 判据三 · 校准：线性标定 + 换算误差逐点可测 + 反向 ——
    let al = ClockAlign::calibrate((1_000_000, 500_000), (2_000_000, 1_400_000));
    let cal_ok = match al {
        Some(a) => {
            // ratio = (1_400_000-500_000)*1e6/(2_000_000-1_000_000) = 900_000ppm
            // to_cpu_ns(3_000_000) = 500_000 + 2_000_000*0.9 = 2_300_000
            a.ratio_ppm == 900_000
                && a.to_cpu_ns(3_000_000) == 2_300_000
                && a.residual_ns(3_000_000, 2_300_000) == 0
                && a.residual_ns(3_000_000, 2_350_000) == 50_000
                && a.residual_ok(3_000_000, 2_350_000)
                && !a.residual_ok(3_000_000, 2_500_000)
        }
        None => false,
    };
    let bad_cal = ClockAlign::calibrate((5_000, 100), (4_000, 200));
    let bad_cal2 = ClockAlign::calibrate((1_000, 100), (1_000, 900));
    s.add(
        "D02-校准-线性标定+残差逐点可测",
        cal_ok
            && bad_cal.is_none()
            && bad_cal2.is_none(),
        "两对样本完全确定线性映射（比率90万ppm判据侧手算对账）；换算残差逐点给账且超容差标注（换算误差可测）；非单调/零间隔样本拒绝标定",
    );

    // —— 判据四 · 开销：<0.1% 帧预算实测核算 ——
    let good = overhead_report(8, 1_000); // 8×1000=8000ns / 12.5ms = 640ppm
    let bad = overhead_report(8, 100_000); // 800_000ns = 64_000ppm 超标
    let line = overhead_line(&good);
    s.add(
        "D02-开销-实测核算+红线标注",
        FRAME_BUDGET_NS == 12_500_000
            && OVERHEAD_LIMIT_PPM == 1_000
            && good.ratio_ppm == 640
            && good.within_budget
            && !bad.within_budget
            && bad.ratio_ppm == 64_000
            && line.contains("达标"),
        "8 次/帧×1µs=640ppm 达标（判据侧手算对账）；×100µs=64000ppm 超标标注——实测值注入核算不拍脑袋宣称",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["四段", "双源", "校准", "开销", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "D02-判据stamp-五条独立重排全等",
        stamp_ok
            && FRAME_TIMER_VERSION.starts_with("D01-")
            && FRAME_STAGES.get(0).map(|x| x.label()) == Some("输入采样")
            && FRAME_STAGES.get(3).map(|x| x.label()) == Some("呈现上屏"),
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）；四段首尾标签钉死",
    );

    s
}
