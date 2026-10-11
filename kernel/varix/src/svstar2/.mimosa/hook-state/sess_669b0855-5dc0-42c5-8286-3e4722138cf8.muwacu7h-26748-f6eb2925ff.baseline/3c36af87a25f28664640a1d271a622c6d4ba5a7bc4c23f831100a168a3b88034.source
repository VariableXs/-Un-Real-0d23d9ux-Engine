//! VE-F0012 · 域自检（判据逐条对应，见 `vea12_probe.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 硬件时间戳统一封装 → `A12-时间戳-唯一入口`
//! - 开销显式 → `A12-开销-声明入账`、`A12-开销-未声明拒绝启用`
//! - 零探针零开销断言（运行期） → `A12-零开销-禁用零查询`
//! - 零开销断言（编译期验证） → `A12-零开销-编译期零大小`
//! - 时间戳含频率校准（漂移补偿） → `A12-校准-漂移补偿换算`
//! - 时间戳失真→校准 → `A12-失真-超容限检出重校准`
//! - 探针含分组开关 → `A12-分组-只开关心的段`
//! - 开销超标→告警 → `A12-开销-超标告警`
//! - 零开销违例→修复 → `A12-违例-显性化修复`
//! - X04 数据契约显式 → `A12-契约-版本与行形状冻结`
//! - 探针面板读屏可达 → `A12-读屏-面板可达`
//!
//! 逻辑时钟注入、零墙钟，回归可复现。

use super::vea12_probe::*;
use crate::checks::CheckSet;

/// VE-F0012 域自检。
pub fn run_vea12_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea12");

    // ---- 硬件时间戳：统一封装 ----

    // 判据：一切测量走唯一入口——开启组的一对 begin/end 恰好消耗 2 次时钟
    // 查询（可证伪：任何绕开 GpuClock::query 的测量都会打破这个计数）。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 1 },
            OverheadProfile::declared_default(),
        );
        let q0 = p.clock_query_count();
        p.begin(0);
        let ended = p.end();
        let used = p.clock_query_count() - q0;
        let unique_entry = ended && used == 2;
        set.add("A12-时间戳-唯一入口", unique_entry, "");
    }

    // ---- 开销显式 ----

    // 判据：开启探针的代价数字入账（声明 12 tick/对，两对 = 24）。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 0b11 },
            OverheadProfile::declared_default(),
        );
        p.begin(0);
        p.end();
        p.begin(1);
        p.end();
        let explicit = p.overhead_ticks() == 24 && p.overhead_profile().declared;
        set.add("A12-开销-声明入账", explicit, "");
    }

    // 判据：开销未声明却启用探针 = 显性违例（不静默给默认）。
    {
        let p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 1 },
            OverheadProfile::undeclared(),
        );
        let rejected = p.errors().iter().any(|(_, c, _)| *c == "E_OVERHEAD_UNDECLARED");
        set.add("A12-开销-未声明拒绝启用", rejected, "");
    }

    // ---- 零探针零开销（运行期） ----

    // 判据：掩码 0 时 begin/end 不发起任何时间戳查询、零记录零开销。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig::disabled(),
            OverheadProfile::declared_default(),
        );
        let q0 = p.clock_query_count();
        p.begin(0);
        let _ = p.end();
        let zero = p.clock_query_count() == q0
            && p.overhead_ticks() == 0
            && p.records_empty()
            && !p.violation();
        set.add("A12-零开销-禁用零查询", zero, "");
    }

    // ---- 零开销断言（编译期验证） ----

    {
        let proof = zero_probe_compile_proof();
        set.add("A12-零开销-编译期零大小", proof, "");
    }

    // ---- 频率校准：漂移补偿 ----

    // 判据：GPU 快 0.1% 时校准斜率 = 1_000_000/1_001_000，换算还原真值。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 1 },
            OverheadProfile::declared_default(),
        );
        let calib = p.calibrate(&[(0, 0), (1_000_000, 1_001_000)]);
        let ok = calib.calibrated
            && calib.num == 1_000_000
            && calib.den == 1_001_000
            && calib.to_ref_ns(1_001_000, 1_000_000_000) == 1_000_000;
        set.add("A12-校准-漂移补偿换算", ok, "");
    }

    // 判据：时间戳失真→校准（超千分比容限检出；重校准后消除）。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 1 },
            OverheadProfile::declared_default(),
        );
        p.calibrate(&[(0, 0), (1_000_000, 1_001_000)]);
        let drifting = p.detect_drift(1_000_000, 1_010_000);
        p.calibrate(&[(0, 0), (1_000_000, 1_010_000)]);
        let healed = !p.detect_drift(1_000_000, 1_010_000);
        set.add("A12-失真-超容限检出重校准", drifting && healed, "");
    }

    // 判据：校准样本不足显性拒绝（不猜斜率）。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 1 },
            OverheadProfile::declared_default(),
        );
        let c = p.calibrate(&[(0, 0)]);
        let refused = !c.calibrated && p.errors().iter().any(|(_, code, _)| *code == "E_CALIBRATION_SAMPLES");
        set.add("A12-校准-样本不足显性拒绝", refused, "");
    }

    // ---- 分组开关 ----

    // 判据：只开关心的段——关组零查询零记录，开组正常采数。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 0b0000_0010 }, // 只开组 1
            OverheadProfile::declared_default(),
        );
        let q0 = p.clock_query_count();
        p.begin(0);
        let _ = p.end(); // 组 0 关：零路径
        let off_cost = p.clock_query_count() - q0;
        p.begin(1);
        let _ = p.end(); // 组 1 开：正常记录
        let ok = off_cost == 0 && p.records_len() == 1 && p.records_group(0) == 1;
        set.add("A12-分组-只开关心的段", ok, "");
    }

    // ---- 开销超标→告警 ----

    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 0xFFFF },
            OverheadProfile::declared_default(),
        );
        p.set_overhead_budget(10); // 故意压低预算：一对 24 tick 必超。
        p.begin(0);
        let _ = p.end();
        let fp = p.finish_frame();
        let warned = fp.warnings.iter().any(|w| w.contains("W_OVERHEAD_BUDGET"));
        set.add("A12-开销-超标告警", warned, "");
    }

    // ---- 零开销违例→修复 ----

    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig::disabled(),
            OverheadProfile::declared_default(),
        );
        p.force_record_while_disabled(2, 50);
        let flagged = p.violation()
            && p.errors().iter().any(|(_, c, _)| *c == "E_ZERO_PROBE_VIOLATION");
        p.repair();
        let repaired = !p.violation() && p.records_empty();
        set.add("A12-违例-显性化修复", flagged && repaired, "");
    }

    // ---- X04 数据契约 ----

    // 判据：契约版本显式 + 行形状冻结（帧号/组号/ns 端点齐全且单调）。
    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 500_500),
            ProbeConfig { group_mask: 1 },
            OverheadProfile::declared_default(),
        );
        p.calibrate(&[(0, 0), (1_000_000, 1_001_000)]);
        p.begin(0);
        let _ = p.end();
        let fp = p.finish_frame();
        let rows = p.to_x04_rows(&fp);
        let shape_ok = rows.len() == 1
            && rows[0].frame == fp.frame
            && rows[0].group == 0
            && rows[0].end_ns >= rows[0].start_ns;
        set.add(
            "A12-契约-版本与行形状冻结",
            X04_CONTRACT_VERSION >= 1 && shape_ok,
            "",
        );
    }

    // ---- 嵌套/乱序防护（边界防护判据） ----

    {
        let mut p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig { group_mask: 0xF },
            OverheadProfile::declared_default(),
        );
        p.begin(0);
        p.begin(1); // 嵌套 → 显性拒绝
        let nested_rejected = p.errors().iter().any(|(_, c, _)| *c == "E_ALREADY_OPEN");
        let _ = p.end();
        let stray = p.end(); // 多余 end → 显性拒绝
        let stray_rejected = !stray && p.errors().iter().any(|(_, c, _)| *c == "E_END_WITHOUT_BEGIN");
        p.begin(GROUP_COUNT); // 越界 → 显性拒绝不 panic
        let range_rejected = p.errors().iter().any(|(_, c, _)| *c == "E_GROUP_OUT_OF_RANGE");
        set.add(
            "A12-防护-嵌套乱序越界显性",
            nested_rejected && stray_rejected && range_rejected,
            "",
        );
    }

    // ---- 读屏可达 ----

    {
        let p = RenderProbe::new(
            GpuClock::new(1_000_000_000, 10),
            ProbeConfig::disabled(),
            OverheadProfile::declared_default(),
        );
        let t = p.panel_text();
        set.add(
            "A12-读屏-面板可达",
            t.contains("渲染探针面板") && t.contains("校准"),
            "",
        );
    }

    set
}
