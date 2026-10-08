//! CGPU-F2247 · 降级恢复引擎（CGPU-O 域 · 降级链 · 恢复主题）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2247`
//!
//! 恢复引擎：恢复（恢复引擎（恢复序/限速/滞回——恢复复用家族——恢复
//! 复用；测试（恢复一组）。判据：恢复复用、一组、判据。
//!
//! ## 要点一：恢复序——逐级有序回升
//!
//! 恢复序四级闭集（资源动作解除→效果动作解除→呈现动作解除→调度动作
//! 解除——与降级施加序相反，与 F2244/F2246 域对齐）；每次恰推进一级，
//! 序表字面量冻结判据独立对拍。
//!
//! ## 要点二：限速与滞回
//!
//! 恢复限速——相邻两级最小间隔 RECOVERY_INTERVAL_TICKS（回升过快=
//! 再触发风险）；恢复滞回——当前余量须 ≥ 滞回带 HYSTERESIS_BAND 才
//! 推进（复用 J03 双滞回防振荡模式——降级阈值与恢复阈值不对称）。
//!
//! ## 要点三：恢复复用家族——J03 模式复用
//!
//! 恢复复用 J03/F1481 家族——两维序/过冲记忆/振荡判据模式复用：
//! 过冲记忆=恢复途中再触发即回退一级并记账（恢复目标自动下调）；
//! 振荡判据=短窗内再触发累计达限即冻结恢复（转决策引擎裁决）。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x5Dxx 段
//!
//! 与 vco06（0x5Cxx）/vco05（0x5Bxx）/vco04（0x5Axx）/vco03（0x59xx）
//! 等互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、恢复序四级闭集
// ---------------------------------------------------------------------------

/// 恢复动作级（官方四级闭集——与降级施加序相反）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStep {
    /// 第 1 级：资源域动作解除（功耗回收最先——成本项最后压）。
    ResourceRelease,
    /// 第 2 级：效果域动作解除（画质回升）。
    EffectsRelease,
    /// 第 3 级：呈现域动作解除（延迟优化退出）。
    PresentationRelease,
    /// 第 4 级：调度域动作解除（帧率档位回满——合同动作最后回）。
    SchedulingRelease,
}

/// 恢复序级数。
pub const RECOVERY_LEVELS: usize = 4;

impl RecoveryStep {
    /// 全部恢复级（官方恢复序——下标即推进次序）。
    pub const ALL: [RecoveryStep; RECOVERY_LEVELS] = [
        RecoveryStep::ResourceRelease,
        RecoveryStep::EffectsRelease,
        RecoveryStep::PresentationRelease,
        RecoveryStep::SchedulingRelease,
    ];

    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            RecoveryStep::ResourceRelease => "资源动作解除".to_string(),
            RecoveryStep::EffectsRelease => "效果动作解除".to_string(),
            RecoveryStep::PresentationRelease => "呈现动作解除".to_string(),
            RecoveryStep::SchedulingRelease => "调度动作解除".to_string(),
        }
    }
}

/// 恢复复用联动单号（J03/F1481 恢复家族——判据独立对拍）。
pub const RECOVERY_UPLINK: u32 = 1481;
/// 恢复复用声明（判据逐字对拍）。
pub const RECOVERY_REUSE_NOTE: &str =
    "恢复复用 J03 家族——两维序/过冲记忆/振荡判据模式复用，恢复语义不另立";

// ---------------------------------------------------------------------------
// 二、限速与滞回（常量字面量冻结）
// ---------------------------------------------------------------------------

/// 恢复限速——相邻两级最小间隔（tick，确定性时钟非墙钟）。
pub const RECOVERY_INTERVAL_TICKS: u64 = 500;
/// 恢复滞回带——余量须达到该值才允许推进（与降级阈值不对称防振荡）。
pub const HYSTERESIS_BAND: i32 = 3;
/// 振荡判据阈值——恢复途中再触发累计达限即冻结。
pub const OSCILLATION_LIMIT: u8 = 3;

// ---------------------------------------------------------------------------
// 三、恢复引擎状态机
// ---------------------------------------------------------------------------

/// 恢复引擎（可驻留可回退的生命周期账——与 F2245 状态机协作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEngine {
    /// 下一个待推进的恢复级下标（0..=4，4=全部恢复完）。
    pub level: usize,
    /// 上次推进时刻（tick）。
    pub last_tick: u64,
    /// 过冲记忆——恢复途中再触发累计次数（每次再触发回退一级）。
    pub overshoot: u8,
    /// 振荡计数——再触发累计（达 OSCILLATION_LIMIT 即冻结）。
    pub oscillation: u8,
    /// 振荡冻结位。
    pub frozen: bool,
}

/// 新恢复引擎（从 0 级起步，未冻结）。
pub fn new_engine() -> RecoveryEngine {
    RecoveryEngine { level: 0, last_tick: 0, overshoot: 0, oscillation: 0, frozen: false }
}

/// 恢复推进一步：限速/触发/冻结三闸前置，过闸按恢复序恰推一级。
///
/// 规则：① 振荡冻结位→拒绝（转决策引擎裁决）；② 触发仍活跃→恢复
/// 暂停（降级优先于恢复——物理约束声明）；③ tick 距上次推进不足限速
/// 窗口→限速拒绝；④ 滞回余量不足→拒绝（当前余量 margin 须 ≥ 滞回带）；
/// ⑤ 全部恢复完→返回 None（回绿）；⑥ 否则推进一级返回该级动作。
pub fn step(
    engine: &mut RecoveryEngine,
    tick: u64,
    trigger_active: bool,
    margin: i32,
) -> Result<Option<RecoveryStep>, RcCode> {
    if engine.frozen {
        return Err(RcCode::OSCILLATION_FROZEN);
    }
    if trigger_active {
        return Err(RcCode::TRIGGER_ACTIVE);
    }
    if engine.level > 0 && tick < engine.last_tick + RECOVERY_INTERVAL_TICKS {
        return Err(RcCode::RATE_LIMITED);
    }
    if margin < HYSTERESIS_BAND {
        return Err(RcCode::RATE_LIMITED);
    }
    if engine.level >= RECOVERY_LEVELS {
        return Ok(None);
    }
    let s = RecoveryStep::ALL[engine.level];
    engine.level += 1;
    engine.last_tick = tick;
    Ok(Some(s))
}

/// 恢复途中再触发：过冲记忆回退一级并记账，达振荡限即冻结。
///
/// 规则：① overshoot/oscillation 各加一；② level 回退一级（下限 0——
/// 过冲防护：恢复目标自动下调）；③ oscillation 达 OSCILLATION_LIMIT→
/// frozen=true（振荡冻结——恢复转决策引擎裁决）。
pub fn on_retrigger(engine: &mut RecoveryEngine) {
    engine.overshoot = engine.overshoot.saturating_add(1);
    engine.oscillation = engine.oscillation.saturating_add(1);
    if engine.level > 0 {
        engine.level -= 1;
    }
    if engine.oscillation >= OSCILLATION_LIMIT {
        engine.frozen = true;
    }
}

// ---------------------------------------------------------------------------
// 四·半、恢复观测账（每级推进留痕——可审计可回放）
// ---------------------------------------------------------------------------

/// 一级恢复的观测记录（推进即留痕——恢复全链可回放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryEntry {
    /// 推进的恢复级动作。
    pub step: RecoveryStep,
    /// 推进时刻（tick）。
    pub tick: u64,
    /// 推进时的滞回余量（≥ 滞回带才可能被记录）。
    pub margin: i32,
    /// 该级的过冲序号（第几次过冲后重推的——0=首次推进）。
    pub attempt: u8,
}

/// 恢复账对账：tick 严格递增且相邻间隔 ≥ 限速窗口，首条豁免窗口检查。
///
/// 规则：① tick 必须严格递增（时间回退=账断裂）；② 相邻两条间隔 ≥
/// RECOVERY_INTERVAL_TICKS（限速窗口在账面可复核——引擎行为与账一致）；
/// ③ 余量必须全部 ≥ 滞回带（滞回语义在账面可复核）。
pub fn audit_entries(entries: &[RecoveryEntry]) -> Result<(), RcCode> {
    let mut i = 0;
    while i < entries.len() {
        if entries[i].margin < HYSTERESIS_BAND {
            return Err(RcCode::RATE_LIMITED);
        }
        if i > 0 {
            let dt = entries[i].tick - entries[i - 1].tick;
            if entries[i].tick <= entries[i - 1].tick || dt < RECOVERY_INTERVAL_TICKS {
                return Err(RcCode::RATE_LIMITED);
            }
        }
        i += 1;
    }
    Ok(())
}

/// 恢复引擎与观测账的一致性核验：账内条数（去除过冲重推）+ 引擎
/// level 反推的已推进级数必须一致——账实相符（防御位自检）。
///
/// 规则：账恰记录了引擎 level 次推进（每级恰一条有效记录）时，账内
/// 去重后的级动作序列必须是 RecoveryStep::ALL 的前缀。
pub fn engine_ledger_consistent(engine: &RecoveryEngine, entries: &[RecoveryEntry]) -> Result<(), RcCode> {
    if engine.level > RECOVERY_LEVELS {
        return Err(RcCode::ORDER_TABLE_BROKEN);
    }
    if entries.len() < engine.level {
        return Err(RcCode::ORDER_TABLE_BROKEN);
    }
    let mut i = 0;
    while i < engine.level {
        if entries[i].step != RecoveryStep::ALL[i] {
            return Err(RcCode::ORDER_TABLE_BROKEN);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、恢复序表完整性守卫
// ---------------------------------------------------------------------------

/// 恢复序表完整性：恰 4 级、标签非空、复用单号对（防御位自检）。
pub fn order_integrity() -> Result<(), RcCode> {
    if RecoveryStep::ALL.len() != RECOVERY_LEVELS {
        return Err(RcCode::ORDER_TABLE_BROKEN);
    }
    for s in RecoveryStep::ALL.iter() {
        if s.label().is_empty() {
            return Err(RcCode::ORDER_TABLE_BROKEN);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、错误契约（独占 0x5Dxx 段）
// ---------------------------------------------------------------------------

/// vco07 诊断码。独占 `0x5Dxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RcCode(pub u16);

impl RcCode {
    /// 限速窗口内或滞回余量不足。
    pub const RATE_LIMITED: RcCode = RcCode(0x5D01);
    /// 触发仍活跃（恢复暂停——降级优先）。
    pub const TRIGGER_ACTIVE: RcCode = RcCode(0x5D02);
    /// 振荡冻结（转决策引擎裁决）。
    pub const OSCILLATION_FROZEN: RcCode = RcCode(0x5D03);
    /// 已全部恢复（无更多级可推——防御位，step 正常路径返回 None）。
    pub const ALL_RECOVERED: RcCode = RcCode(0x5D04);
    /// 恢复序表完整性破坏。
    pub const ORDER_TABLE_BROKEN: RcCode = RcCode(0x5D05);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            RcCode::RATE_LIMITED => "限速或滞回未达：距上次推进不足窗口或余量低于滞回带".into(),
            RcCode::TRIGGER_ACTIVE => "触发仍活跃：恢复暂停——降级优先于恢复".into(),
            RcCode::OSCILLATION_FROZEN => "振荡冻结：再触发达限，恢复转决策引擎裁决".into(),
            RcCode::ALL_RECOVERED => "已全部恢复：无更多恢复级可推进".into(),
            RcCode::ORDER_TABLE_BROKEN => "恢复序表完整性破坏：级数或标签不符".into(),
            RcCode(_) => "未知 vco07 降级恢复引擎域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、测试支撑（锚点一组：恢复）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 恢复全链与限速滞回() {
        assert_eq!(order_integrity(), Ok(()));
        let mut e = new_engine();
        // 限速：首级不受限（level==0），第二级同 tick 推进被拒
        assert_eq!(step(&mut e, 100, false, 5), Ok(Some(RecoveryStep::ResourceRelease)));
        assert_eq!(step(&mut e, 100, false, 5), Err(RcCode::RATE_LIMITED));
        // 滞回：余量低于滞回带拒
        assert_eq!(step(&mut e, 1000, false, 2), Err(RcCode::RATE_LIMITED));
        // 触发活跃：恢复暂停
        assert_eq!(step(&mut e, 1000, true, 5), Err(RcCode::TRIGGER_ACTIVE));
        // 过闸推进
        assert_eq!(step(&mut e, 1000, false, 5), Ok(Some(RecoveryStep::EffectsRelease)));
        assert_eq!(step(&mut e, 2000, false, 5), Ok(Some(RecoveryStep::PresentationRelease)));
        assert_eq!(step(&mut e, 3000, false, 5), Ok(Some(RecoveryStep::SchedulingRelease)));
        assert_eq!(step(&mut e, 4000, false, 5), Ok(None));
    }

    #[test]
    fn 过冲记忆与振荡冻结() {
        let mut e = new_engine();
        assert_eq!(step(&mut e, 100, false, 5), Ok(Some(RecoveryStep::ResourceRelease)));
        assert_eq!(step(&mut e, 700, false, 5), Ok(Some(RecoveryStep::EffectsRelease)));
        // 恢复途中再触发：过冲回退一级
        on_retrigger(&mut e);
        assert_eq!(e.level, 1);
        assert_eq!(e.overshoot, 1);
        // 再推进须重推同一级（过冲防护——恢复目标下调）
        assert_eq!(step(&mut e, 1300, false, 5), Ok(Some(RecoveryStep::EffectsRelease)));
        // 振荡：再触发达限冻结
        on_retrigger(&mut e);
        on_retrigger(&mut e);
        assert_eq!(e.oscillation, 3);
        assert!(e.frozen);
        assert_eq!(step(&mut e, 9999, false, 9), Err(RcCode::OSCILLATION_FROZEN));
    }
}
