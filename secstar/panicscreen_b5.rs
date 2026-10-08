//! F173 panic 画面设计 · 批次五深化（secstar · G-G-03）。
//!
//! 批次五功能面（与 b3「版面与倒计时」、b4「码与统计」互补，本批管
//! 「恢复建议与风暴闸」）：
//! - [`recovery_advice`]：恢复建议生成——模块 → 分步建议（panic 后
//!   不是死屏：给用户可走的路，逐模块真出路）；
//! - [`PanicBurstGate`]：连续 panic 风暴闸——连续 ≥2 次 → 跳过倒计时
//!   直进安全模式询问（主册 CONSECUTIVE_SAFE_MODE_AT 的触发流本体）；
//! - [`boot_health_reset`]：健康启动复位——成功进入系统后清连续计数
//!   （风暴闸的复位面：一次好启动就翻篇）。
//!
//! 零堆纪律：状态字段 + 定长建议，无 alloc。

use super::panicscreen::CONSECUTIVE_SAFE_MODE_AT;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 恢复建议（逐模块真出路）
// ---------------------------------------------------------------------------

/// 模块 → 分步建议（三步以内，每步可操作）。
pub fn recovery_advice(module: &str) -> [(&'static str, bool); 3] {
    // bool = 该步是否需要重启。
    match module {
        "GFX" => [
            ("等待系统回退到基础显示模式", false),
            ("进入系统后打开「显示设置」检查刷新率", false),
            ("若反复出现，运行图形驱动重置（需重启）", true),
        ],
        "MEM" => [
            ("系统已转安全模式，数据已保护", false),
            ("打开「内存诊断」跑一次完整检测", false),
            ("确认内存故障后联系检修（需关机）", true),
        ],
        "STORE" => [
            ("文件系统已回滚到一致状态", false),
            ("打开「检查此卷」修复损坏区块", false),
            ("修复后重启完成校验（需重启）", true),
        ],
        "SCHED" => [
            ("调度器已回退到保守策略", false),
            ("进入系统后查看「性能」页有无异常占用", false),
            ("若反复出现，卸载最近安装的应用", false),
        ],
        _ => [
            ("系统已保存诊断快照", false),
            ("查看帮助篇 panic-general 了解详情", false),
            ("重启后如再现请提交反馈", true),
        ],
    }
}

/// 建议行数守恒：所有模块三步齐（不缺步——第 9 章三要素的步骤化）。
pub fn advice_complete() -> bool {
    ["GFX", "MEM", "STORE", "SCHED", "NET", "INPUT", "POWER", "BOOT"]
        .iter()
        .all(|m| recovery_advice(m).iter().all(|(text, _)| !text.is_empty()))
}

// ---------------------------------------------------------------------------
// 连续 panic 风暴闸
// ---------------------------------------------------------------------------

/// 风暴闸状态机：连续计数 + 阈值触发 + 成功启动复位。
pub struct PanicBurstGate {
    consecutive: u32,
    /// 已触发安全模式询问（一次性——询问只弹一次）。
    pub asked: bool,
}

impl PanicBurstGate {
    pub const fn new() -> PanicBurstGate {
        PanicBurstGate { consecutive: 0, asked: false }
    }

    /// 又一次 panic：计数 +1；达到阈值 → 触发询问（恰好一次）。
    pub fn on_panic(&mut self) -> bool {
        self.consecutive += 1;
        if self.consecutive >= CONSECUTIVE_SAFE_MODE_AT && !self.asked {
            self.asked = true;
            return true; // 直进安全模式询问
        }
        false
    }

    /// 成功启动进系统 → 清零（一次好启动就翻篇）。
    pub fn on_healthy_boot(&mut self) {
        self.consecutive = 0;
        self.asked = false;
    }

    pub fn consecutive(&self) -> u32 {
        self.consecutive
    }
}

/// 风暴闸与主层账本联动：PanicLedger 连续计数与闸门同源语义。
pub fn gate_matches_ledger(ledger_consecutive: u32, gate: &PanicBurstGate) -> bool {
    ledger_consecutive == gate.consecutive()
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_panicscreen_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F173-b5");

    // 1) 建议逐步：GFX 三步、第一步不需重启、第三步需重启（步骤语义）。
    let gfx = recovery_advice("GFX");
    cs.add(
        "advice_gfx_steps",
        gfx[0].1 == false && gfx[1].1 == false && gfx[2].1 == true && gfx.iter().all(|(t, _)| !t.is_empty()),
        "",
    );

    // 2) 建议全模块齐：八模块 × 3 步全非空（不缺步）。
    cs.add("advice_complete", advice_complete(), "");

    // 3) 未知模块兜底：不在表内也三步齐（兜底不缺位）。
    let unk = recovery_advice("WHATEVER");
    cs.add("advice_fallback", unk.iter().all(|(t, _)| !t.is_empty()), "");

    // 4) 风暴闸：第 1 次 panic 不触发、第 2 次触发询问（阈值恰点）。
    let mut g = PanicBurstGate::new();
    let first = !g.on_panic();
    let second = g.on_panic();
    cs.add(
        "burst_gate_threshold",
        first && second && g.asked && g.consecutive() == 2,
        "",
    );

    // 5) 询问一次性：触发后再 panic 不重复弹（第 3、4 次不再问）。
    let third = !g.on_panic();
    let fourth = !g.on_panic();
    cs.add("burst_ask_once", third && fourth && g.consecutive() == 4, "");

    // 6) 健康启动复位：清计数 + 复位询问旗（翻篇语义）。
    g.on_healthy_boot();
    cs.add(
        "burst_reset",
        g.consecutive() == 0 && !g.asked && !g.on_panic(),
        "",
    );

    // 7) 复位后重新累计：第二次风暴再次触发（复位不是禁用）。
    let again = g.on_panic();
    cs.add("burst_rearm", again && g.asked, "");

    // 8) 阈值常量贯通：CONSECUTIVE_SAFE_MODE_AT == 2 一处一事实。
    cs.add("consts_aligned", CONSECUTIVE_SAFE_MODE_AT == 2, "");

    // 9) 主层持久帧联动：PanicPersist.consecutive 经帧 round-trip 与
    //     闸门同源（一处一事实消费面——连续计数的权威在持久帧）。
    let persist = super::panicscreen::PanicPersist {
        addr: 0x1000,
        total_count: 2,
        consecutive: 2,
        code: *b"VX-PANIC-GFX",
    };
    let mut frame = [0u8; super::panicscreen::PERSIST_FRAME_LEN];
    super::panicscreen::encode_persist(&persist, &mut frame);
    let dec = super::panicscreen::decode_persist(&frame).unwrap();
    let mut g3 = PanicBurstGate::new();
    g3.on_panic();
    g3.on_panic();
    cs.add("ledger_gate_sync", dec.consecutive == 2 && gate_matches_ledger(dec.consecutive, &g3), "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn burst_long_storm_one_ask() {
        // 长风暴：连崩 10 次只问一次（询问不骚扰——触发即定格）。
        let mut g = PanicBurstGate::new();
        let mut asks = 0;
        for _ in 0..10 {
            if g.on_panic() {
                asks += 1;
            }
        }
        assert_eq!(asks, 1);
        assert_eq!(g.consecutive(), 10);
    }

    #[test]
    fn advice_restart_flags_sane() {
        // 重启旗分布：兜底建议只有末步需重启（前两步是可立即做的）。
        let a = recovery_advice("NET");
        assert!(!a[0].1 && !a[1].1 && a[2].1);
    }

    #[test]
    fn ledger_reset_on_repair() {
        // 主层账本与闸门同步复位路径：连续计数双向一致。
        let mut g = PanicBurstGate::new();
        g.on_panic();
        g.on_panic();
        g.on_healthy_boot();
        assert!(gate_matches_ledger(0, &g));
    }
}
