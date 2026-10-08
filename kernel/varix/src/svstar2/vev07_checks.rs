//! VE-F4407 · 域自检（判据逐条对应，见 `vev07_multisync.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **派生联动**（主屏校准派生副屏，三态派生 + 失配重算留痕） →
//!   `C07-派生-01` ~ `C07-派生-07`
//! - **漂移监测**（周期采样对拍，只采参数不采内容） →
//!   `C07-监测-01` ~ `C07-监测-06`
//! - **独立豁免**（创作屏豁免并标注 + 频次提示） →
//!   `C07-豁免-01` ~ `C07-豁免-05`
//! - **告警三通道**（读屏行 + 台账立案 + 校准建议同一事实三呈现） →
//!   `C07-告警-01` ~ `C07-告警-03`、`C07-主流程-01`
//! - **判据**（五条映射齐备 + 条数对账） → `H07-判据-条数对账`
//! - 主流程编排（豁免/重算屏跳过采样 + 记账） → `C07-主流程-01` ~ `05`
//!
//! **判据设计硬规矩**：容差 15 与频次限 3 与 gamma 域 800..=1200 与
//! 短码四域判据侧字面量写死（不引用被测常量自比自身）；阈值边界
//! 15/16 对拍（防 `>` ⇄ `>=` 等价变异）；豁免反复横跳**真实触发**
//! 频次提示（反恒假门禁——先证明该状态真会出现）；主流程两读数源
//! 分立（登记值/物理采样）——共用一源则告警路径不可达。

use crate::checks::CheckSet;
use crate::svstar2::vev07_multisync as ms;
use alloc::string::{String, ToString};
use alloc::vec;

// ---------------------------------------------------------------------------
// 判据侧独立真值区（字面量写死——被测常量改了这里必红）
// ---------------------------------------------------------------------------

/// 判据侧独立漂移容差（与锚点口径一致：千分位/gamma）。
const REF_DRIFT_TOL: i64 = 15;
/// 判据侧独立频次提示阈值。
const REF_FLIP_FLOP: u32 = 3;
/// 判据侧独立 gamma 合法域（F4405 校准节口径）。
const REF_GAMMA_MIN: i64 = 800;
const REF_GAMMA_MAX: i64 = 1200;
/// 判据侧独立短码四域（F4403 引用域）。
const REF_PROFILES: [&str; 4] = ["srgb", "p3", "adobe", "custom"];

/// 判据侧独立漂移判定（与被测 `DriftMonitor::sample` 逐字对齐）。
fn ref_drift_exceeds(gamma: i64, baseline: i64) -> bool {
    (gamma - baseline).abs() > REF_DRIFT_TOL
}

// ---------------------------------------------------------------------------
// 判据主体
// ---------------------------------------------------------------------------

/// F4407 多屏色彩同步判据（五条映射 27 项六组）。
pub fn run_vev07_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4407");

    // =====================================================================
    // 一、派生联动（判据一：主屏校准派生副屏）
    // =====================================================================

    // -- C07-派生-01：副屏与派生值一致 → InSync（无需重算）。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["sec-a".to_string()],
            gamma_per_mille: 1000,
            color_profile: "srgb".to_string(),
        };
        let cur = |id: &str| -> Option<i64> {
            if id == "sec-a" { Some(1000) } else { None }
        };
        let r = ms::derive_all(&cfg, &[], &cur);
        let passed = matches!(
            r.as_deref(),
            Ok([(_, ms::DeriveOutcome::InSync)])
        );
        s.add(
            "C07-派生-01 同值即 InSync",
            passed,
            if passed { "副屏 gamma 与主屏一致 → InSync 不重算" } else { "同值副屏未判 InSync" },
        );
    }

    // -- C07-派生-02：派生失配 → Recalced 且 old/new 两值留痕。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["sec-b".to_string()],
            gamma_per_mille: 1000,
            color_profile: "p3".to_string(),
        };
        let cur = |id: &str| -> Option<i64> {
            if id == "sec-b" { Some(970) } else { None }
        };
        let r = ms::derive_all(&cfg, &[], &cur);
        let passed = matches!(
            r.as_deref(),
            Ok([(
                _,
                ms::DeriveOutcome::Recalced { old_gamma: 970, new_gamma: 1000 }
            )])
        );
        s.add(
            "C07-派生-02 失配重算留痕",
            passed,
            if passed { "970→1000 旧新两值皆在案（降级矩阵第一条）" } else { "重算结果或留痕不符" },
        );
    }

    // -- C07-派生-03：豁免屏 → Exempted（跳过派生并标注）。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["art".to_string(), "sec-c".to_string()],
            gamma_per_mille: 1000,
            color_profile: "adobe".to_string(),
        };
        let cur = |id: &str| -> Option<i64> {
            if id == "art" { Some(1100) } else if id == "sec-c" { Some(1000) } else { None }
        };
        let exempt = ["art".to_string()];
        let r = ms::derive_all(&cfg, &exempt, &cur);
        let passed = matches!(
            r.as_deref(),
            Ok([
                (_, ms::DeriveOutcome::Exempted),
                (_, ms::DeriveOutcome::InSync),
            ])
        );
        s.add(
            "C07-派生-03 豁免屏跳过派生",
            passed,
            if passed { "创作屏 1100 不被派生覆盖且标注 Exempted" } else { "豁免屏未跳过或标注缺失" },
        );
    }

    // -- C07-派生-04：空主屏 → E_SYNC_INPUT（空基准不派生）。
    {
        let cfg = ms::SyncConfig {
            primary: String::new(),
            secondaries: vec!["sec-d".to_string()],
            gamma_per_mille: 1000,
            color_profile: "srgb".to_string(),
        };
        let cur = |_: &str| -> Option<i64> { None };
        let r = ms::derive_all(&cfg, &[], &cur);
        let passed = r == Err(ms::E_SYNC_INPUT);
        s.add(
            "C07-派生-04 空主屏拒绝",
            passed,
            if passed { "空基准 → E_SYNC_INPUT 不派生" } else { "空主屏未被拒绝" },
        );
    }

    // -- C07-派生-05：gamma 域下界外（799，判据侧独立域 800..=1200）拒绝。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec![],
            gamma_per_mille: REF_GAMMA_MIN - 1,
            color_profile: "srgb".to_string(),
        };
        let cur = |_: &str| -> Option<i64> { None };
        let r = ms::derive_all(&cfg, &[], &cur);
        let passed = r == Err(ms::E_SYNC_INPUT);
        s.add(
            "C07-派生-05 gamma 越下界拒",
            passed,
            if passed { "799 出独立域 800..=1200 → E_SYNC_INPUT" } else { "下界外未拒（域判定放水）" },
        );
    }

    // -- C07-派生-06：gamma 域上界外（1201）拒绝；且边界内 800/1200 合法。
    {
        let mk = |g: i64| ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec![],
            gamma_per_mille: g,
            color_profile: "custom".to_string(),
        };
        let cur = |_: &str| -> Option<i64> { None };
        let over = ms::derive_all(&mk(REF_GAMMA_MAX + 1), &[], &cur);
        let lo_ok = ms::derive_all(&mk(REF_GAMMA_MIN), &[], &cur).is_ok();
        let hi_ok = ms::derive_all(&mk(REF_GAMMA_MAX), &[], &cur).is_ok();
        let passed = over == Err(ms::E_SYNC_INPUT) && lo_ok && hi_ok;
        s.add(
            "C07-派生-06 gamma 边界对拍",
            passed,
            if passed { "1201 拒而 800/1200 收（防 >⇄>= 等价变异）" } else { "边界判定不等价于 800..=1200" },
        );
    }

    // -- C07-派生-07：副屏无登记值 → Recalced{old:0}（缺值视作全新派生）。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["new-screen".to_string()],
            gamma_per_mille: 950,
            color_profile: "srgb".to_string(),
        };
        let cur = |_: &str| -> Option<i64> { None };
        let r = ms::derive_all(&cfg, &[], &cur);
        let passed = matches!(
            r.as_deref(),
            Ok([(_, ms::DeriveOutcome::Recalced { old_gamma: 0, new_gamma: 950 })])
        );
        s.add(
            "C07-派生-07 缺值全新派生",
            passed,
            if passed { "无登记值 → old=0/new=主屏 逐屏重算路径" } else { "缺值派生路径不符" },
        );
    }

    // =====================================================================
    // 二、漂移监测（判据二：周期采样对拍，只采参数不采内容）
    // =====================================================================

    // -- C07-监测-01：容差常量与判据侧独立字面量对账（15）。
    {
        let passed = ms::DRIFT_TOL_PER_MILLE == REF_DRIFT_TOL;
        s.add(
            "C07-监测-01 容差字面量对账",
            passed,
            if passed { "DRIFT_TOL_PER_MILLE 与判据侧独立 15 一致" } else { "容差常量被改（判据侧 15 写死）" },
        );
    }

    // -- C07-监测-02：阈内（漂移恰为 15）不告警（None，记账不告警）。
    {
        let mut m = ms::DriftMonitor::default();
        let r = m.sample("sec", 1000 - REF_DRIFT_TOL, 1000, 1);
        let passed = r.is_none();
        s.add(
            "C07-监测-02 阈内不告警",
            passed,
            if passed { "漂移恰为容差 15 → None（> 不含等号）" } else { "阈内误告警（>= 放水）" },
        );
    }

    // -- C07-监测-03：超阈（漂移 16）告警且返回漂移量本身。
    {
        let mut m = ms::DriftMonitor::default();
        let r = m.sample("sec", 1000 - (REF_DRIFT_TOL + 1), 1000, 2);
        let passed = r == Some(REF_DRIFT_TOL + 1);
        s.add(
            "C07-监测-03 超阈返回漂移量",
            passed,
            if passed { "漂移 16 → Some(16) 边界对拍 15/16 双向" } else { "超阈判定或漂移量回传不符" },
        );
    }

    // -- C07-监测-04：负向漂移取绝对值（低于基准同样超阈）。
    {
        let mut m = ms::DriftMonitor::default();
        let over = m.sample("sec-lo", 1000 - (REF_DRIFT_TOL + 5), 1000, 3);
        let mut m2 = ms::DriftMonitor::default();
        let under = m2.sample("sec-hi", 1000 + (REF_DRIFT_TOL + 5), 1000, 3);
        let passed = over == Some(REF_DRIFT_TOL + 5) && under == Some(REF_DRIFT_TOL + 5);
        s.add(
            "C07-监测-04 双向漂移绝对值",
            passed,
            if passed { "偏高/偏低同判：|γ-基准| 对称超阈" } else { "单向漂移漏判" },
        );
    }

    // -- C07-监测-05：超阈告警计数留痕、阈内不计数；采样流只追加可回放。
    {
        let mut m = ms::DriftMonitor::default();
        let _ = m.sample("a", 1000 + REF_DRIFT_TOL + 1, 1000, 1); // 超阈
        let _ = m.sample("b", 1000 + REF_DRIFT_TOL, 1000, 2); // 阈内
        let passed = m.alerts == 1 && m.stream.len() == 2
            && m.stream.get(1).map(|sp| sp.tick == 2).unwrap_or(false);
        s.add(
            "C07-监测-05 告警计数留痕",
            passed,
            if passed { "alerts=1 流长=2（超阈 1 计、阈内 0 计、流可回放）" } else { "计数或采样流记账不符" },
        );
    }

    // -- C07-监测-06：Sample 结构只含校准参数三键（identity/gamma/tick），
    //    无内容字段（锚点隐私口径，结构可断言）。
    {
        let sp = ms::Sample {
            identity: "sec".to_string(),
            gamma_per_mille: 990,
            tick: 7,
        };
        let dbg = alloc::format!("{:?}", sp);
        let three_keys = dbg.contains("identity")
            && dbg.contains("gamma_per_mille")
            && dbg.contains("tick");
        let no_content = !dbg.contains("pixel")
            && !dbg.contains("content")
            && !dbg.contains("frame");
        let passed = three_keys && no_content
            && sp.identity == "sec" && sp.gamma_per_mille == 990 && sp.tick == 7;
        s.add(
            "C07-监测-06 采样只含参数",
            passed,
            if passed { "Sample 三键参数、无 pixel/content/frame 内容字段" } else { "采样结构含内容字段（隐私口径破口）" },
        );
    }

    // =====================================================================
    // 三、告警三通道（判据四：同一事实三种呈现）
    // =====================================================================

    // -- C07-告警-01：三通道字段齐备且立案码为台账码字面量。
    {
        let a = ms::drift_alert("sec-x", 23);
        let passed = !a.screen_line.is_empty()
            && !a.advice.is_empty()
            && a.case_code == "E_SYNC_DRIFT";
        s.add(
            "C07-告警-01 三通道齐备",
            passed,
            if passed { "读屏行+立案码 E_SYNC_DRIFT+建议 三字段齐发" } else { "告警三通道缺一（单通道必丢）" },
        );
    }

    // -- C07-告警-02：读屏行人话可播报（含屏身份与漂移数值）。
    {
        let a = ms::drift_alert("创作屏B", 23);
        let passed = a.screen_line.contains("创作屏B") && a.screen_line.contains("23");
        s.add(
            "C07-告警-02 读屏行含事实",
            passed,
            if passed { "screen_line 同时含身份与漂移量（可独立播报）" } else { "读屏行缺关键事实" },
        );
    }

    // -- C07-告警-03：建议通道给出下一步（重校准 + 容差依据）。
    {
        let a = ms::drift_alert("sec-y", 40);
        let passed = a.advice.contains("sec-y")
            && a.advice.contains("校准")
            && a.advice.contains("15");
        s.add(
            "C07-告警-03 建议含下一步",
            passed,
            if passed { "advice 含身份+重校准动作+容差 15 依据" } else { "建议通道无头绪（废话建议）" },
        );
    }

    // =====================================================================
    // 四、独立豁免（判据三：创作屏豁免并标注 + 频次提示）
    // =====================================================================

    // -- C07-豁免-01：toggle 登记后 is_exempt 且计数 1。
    {
        let mut b = ms::ExemptionBook::default();
        b.toggle("art", "调色创作屏");
        let rec = b.entries.iter().find(|e| e.identity == "art");
        let passed = b.is_exempt("art")
            && rec.map(|e| e.active && e.toggle_count == 1).unwrap_or(false)
            && rec.map(|e| e.reason == "调色创作屏").unwrap_or(false);
        s.add(
            "C07-豁免-01 登记生效",
            passed,
            if passed { "登记后 active/计数 1/原因留档 三者齐" } else { "登记路径不符" },
        );
    }

    // -- C07-豁免-02：撤销翻转 active 但记录与计数保留（计数不归一）。
    {
        let mut b = ms::ExemptionBook::default();
        b.toggle("art", "调色创作屏");
        b.toggle("art", "");
        let passed = !b.is_exempt("art")
            && b.entries.len() == 1
            && b.entries.first().map(|e| !e.active && e.toggle_count == 2).unwrap_or(false);
        s.add(
            "C07-豁免-02 撤销留痕不删",
            passed,
            if passed { "撤销只翻 active、计数 2 保留（频次检测不失效）" } else { "撤销删记录致计数归一（恒假路径回潮）" },
        );
    }

    // -- C07-豁免-03：频次限常量与判据侧独立字面量对账（3）。
    {
        let passed = ms::FLIP_FLOP_LIMIT == REF_FLIP_FLOP;
        s.add(
            "C07-豁免-03 频次限对账",
            passed,
            if passed { "FLIP_FLOP_LIMIT 与判据侧独立 3 一致" } else { "频次限被改（判据侧 3 写死）" },
        );
    }

    // -- C07-豁免-04：反复横跳真实触发——前 3 次无提示、第 4 次提示
    //    （反恒假门禁：证明频次提示状态真会出现）。
    {
        let mut b = ms::ExemptionBook::default();
        let h1 = b.toggle("art", "r"); // count=1
        let h2 = b.toggle("art", ""); // count=2
        let h3 = b.toggle("art", "r"); // count=3
        let h4 = b.toggle("art", ""); // count=4 > 3 → 提示
        let passed = h1.is_none() && h2.is_none() && h3.is_none()
            && h4 == Some(ms::E_SYNC_EXEMPT);
        s.add(
            "C07-豁免-04 横跳真实触发",
            passed,
            if passed { "计数 1..3 静默、第 4 次 → E_SYNC_EXEMPT（限值=3 实测可达）" } else { "频次提示不可达（恒假）或阈值错位" },
        );
    }

    // -- C07-豁免-05：提示计数 hints 留痕；生效豁免清单只含 active。
    {
        let mut b = ms::ExemptionBook::default();
        for _ in 0..(REF_FLIP_FLOP + 1) {
            let _ = b.toggle("art", "r");
        }
        let mut ids = b.exempt_identities();
        ids.sort();
        // 4 次 toggle：奇数次 → active（登记态）。
        let passed = b.hints == 1 && ids == vec!["art".to_string()]
            && !b.is_exempt("off") && {
            let _ = b.toggle("off", "x");
            b.exempt_identities().iter().any(|i| i == "off")
        };
        s.add(
            "C07-豁免-05 提示留痕清单",
            passed,
            if passed { "hints=1、exempt_identities 只含 active 态" } else { "提示计数或生效清单口径不符" },
        );
    }

    // =====================================================================
    // 五、主流程编排（一 tick 三件 + 记账）
    // =====================================================================

    // -- C07-主流程-01：InSync 屏物理采样超阈 → 告警三通道产出。
    //    （两读数源分立：登记值同基线 → InSync；物理读数漂移 16 → 告警）
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["sec".to_string()],
            gamma_per_mille: 1000,
            color_profile: "srgb".to_string(),
        };
        let mut eng = ms::SyncEngine::default();
        let cur = |_: &str| -> Option<i64> { Some(1000) };
        let smp = |_: &str| -> Option<i64> { Some(1000 - (REF_DRIFT_TOL + 1)) };
        let r = eng.sync_tick(&cfg, &cur, &smp, 1);
        let passed = match r {
            Ok(alerts) => {
                alerts.len() == 1
                    && alerts.first().map(|a| {
                        a.case_code == "E_SYNC_DRIFT"
                            && !a.screen_line.is_empty()
                            && !a.advice.is_empty()
                    }).unwrap_or(false)
            }
            Err(_) => false,
        };
        s.add(
            "C07-主流程-01 超阈告警产出",
            passed,
            if passed { "InSync 屏物理漂移 16 → 单告警三通道齐备" } else { "主流程告警路径不可达或三通道缺" },
        );
    }

    // -- C07-主流程-02：豁免屏整 tick 跳过（不采样不告警——采样流不动）。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["art".to_string(), "sec".to_string()],
            gamma_per_mille: 1000,
            color_profile: "p3".to_string(),
        };
        let mut eng = ms::SyncEngine::default();
        let _ = eng.book.toggle("art", "调色创作屏");
        let cur = |id: &str| -> Option<i64> {
            // art 物理值大幅偏离也绝不采样；sec 同值 InSync。
            if id == "art" { Some(500) } else { Some(1000) }
        };
        let smp = |id: &str| -> Option<i64> {
            if id == "art" { Some(500) } else { Some(1000) }
        };
        let r = eng.sync_tick(&cfg, &cur, &smp, 1);
        let passed = match r {
            Ok(alerts) => alerts.is_empty() && eng.monitor.stream.len() == 1,
            Err(_) => false,
        };
        s.add(
            "C07-主流程-02 豁免屏不采样",
            passed,
            if passed { "art 偏 500 仍零采样零告警；仅 sec 采 1 点" } else { "豁免屏被采样（独立校准被侵犯）" },
        );
    }

    // -- C07-主流程-03：刚重算对齐的屏本 tick 跳过采样（重算即对齐）。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["sec".to_string()],
            gamma_per_mille: 1000,
            color_profile: "srgb".to_string(),
        };
        let mut eng = ms::SyncEngine::default();
        let cur = |_: &str| -> Option<i64> { Some(900) }; // 失配 → 重算
        let smp = |_: &str| -> Option<i64> { Some(900) };
        let r = eng.sync_tick(&cfg, &cur, &smp, 1);
        let passed = match r {
            Ok(alerts) => alerts.is_empty()
                && eng.monitor.stream.is_empty()
                && eng.last_recalced == 1,
            Err(_) => false,
        };
        s.add(
            "C07-主流程-03 重算屏跳采样",
            passed,
            if passed { "失配重算屏零采样零告警、last_recalced=1" } else { "重算屏被重复对拍（记账不符）" },
        );
    }

    // -- C07-主流程-04：非法配置整 tick 拒绝且引擎状态不被污染。
    {
        let cfg = ms::SyncConfig {
            primary: String::new(),
            secondaries: vec!["sec".to_string()],
            gamma_per_mille: 1000,
            color_profile: "srgb".to_string(),
        };
        let mut eng = ms::SyncEngine::default();
        let cur = |_: &str| -> Option<i64> { Some(1000) };
        let smp = |_: &str| -> Option<i64> { Some(1000) };
        let r = eng.sync_tick(&cfg, &cur, &smp, 1);
        let passed = r == Err(ms::E_SYNC_INPUT) && eng.ticks == 1
            && eng.monitor.stream.is_empty() && eng.last_recalced == 0;
        s.add(
            "C07-主流程-04 非法输入不污染",
            passed,
            if passed { "E_SYNC_INPUT 拒绝；tick 计数在案、流与重算零残留" } else { "失败 tick 污染引擎状态" },
        );
    }

    // -- C07-主流程-05：多 tick 记账与读屏摘要（逻辑时钟零墙钟）。
    {
        let cfg = ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec!["sec".to_string()],
            gamma_per_mille: 1000,
            color_profile: "srgb".to_string(),
        };
        let mut eng = ms::SyncEngine::default();
        let cur = |_: &str| -> Option<i64> { Some(1000) };
        let smp = |_: &str| -> Option<i64> { Some(995) }; // 阈内记账
        let _ = eng.sync_tick(&cfg, &cur, &smp, 1);
        let _ = eng.sync_tick(&cfg, &cur, &smp, 2);
        let sum = eng.screen_summary(&cfg);
        let passed = eng.ticks == 2
            && eng.monitor.stream.len() == 2
            && eng.monitor.alerts == 0
            && sum.contains("main")
            && sum.contains("1000");
        s.add(
            "C07-主流程-05 多tick记账摘要",
            passed,
            if passed { "ticks=2 流=2 告警 0；摘要含主屏身份与 gamma 可读屏" } else { "多 tick 记账或摘要口径不符" },
        );
    }

    // =====================================================================
    // 六、判据（锚点判据五：契约常量域 + 条数离账自证）
    // =====================================================================

    // -- H07-判据-契约-01：短码四域字面量写死（legal 域判定独立对拍）。
    {
        let mk = |p: &str| ms::SyncConfig {
            primary: "main".to_string(),
            secondaries: vec![],
            gamma_per_mille: 1000,
            color_profile: p.to_string(),
        };
        let cur = |_: &str| -> Option<i64> { None };
        let mut all_ok = true;
        for p in REF_PROFILES {
            all_ok = all_ok && ms::derive_all(&mk(p), &[], &cur).is_ok();
        }
        let bad = ms::derive_all(&mk("prophoto"), &[], &cur);
        let passed = all_ok && bad == Err(ms::E_SYNC_INPUT);
        s.add(
            "H07-判据-契约-01 短码四域",
            passed,
            if passed { "srgb/p3/adobe/custom 收、表外 prophoto 拒（白名单独立对拍）" } else { "短码域判定与四短码不符" },
        );
    }

    // -- H07-判据-契约-02：版本号在案（诊断面板可溯源）。
    {
        let passed = ms::SYNC_ENGINE_VERSION == "V07-sync-v1";
        s.add(
            "H07-判据-契约-02 版本在案",
            passed,
            if passed { "SYNC_ENGINE_VERSION=V07-sync-v1 溯源键稳定" } else { "版本键漂移" },
        );
    }

    // -- H07-判据-条数对账：判据侧离账自证（本条为第 27 项）。
    {
        let passed = s.len() == 26;
        s.add(
            "H07-判据-条数对账",
            passed,
            if passed { "判据 27 项离账：对账点前 26 项与设计清单一一对应" } else { "判据条数与设计清单不符（漏项/多项）" },
        );
    }

    s
}
