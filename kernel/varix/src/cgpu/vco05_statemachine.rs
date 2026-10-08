//! CGPU-F2245 · 降级状态机（CGPU-O 域 · 降级链 · 状态机主题）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2245`
//!
//! 状态机：状态机（降级状态机（全域降级状态机（正常/降级中/降级档/恢复
//! ——状态机（状态机形式化复用 J03——形式化复用；迁移（迁移条件显性
//! ——迁移复用；测试（状态机/形式化/迁移三组）。
//!
//! ## 要点一：全域降级状态机四态闭集
//!
//! 正常/降级中/降级档/恢复——全域降级生命周期封闭枚举，表外不立态；
//! 每态一条中文标签（字面量冻结，判据独立对拍）。
//!
//! ## 要点二：状态机形式化复用 J03
//!
//! 形式化验证复用 J03（F1475）模式——迁移表全枚举 + 不可达态检查：
//! 合法迁移恰 6 条逐条显性（from/to/条件字面量三件齐）；从正常态沿
//! 迁移表可达全部四态（无不可达态）；复用不另立形式化语义。
//!
//! ## 要点三：迁移条件显性——迁移复用
//!
//! 迁移条件全部字面量显性（不隐式推断）；跳段/倒退/自迁移逐类显性
//! 拒绝；与 vco01 五段流水线（触发/决策/执行/恢复/观测）的分工显性
//! 声明——段=管线步进（一次性推进），态=生命周期状态（可驻留可回退）。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x5Bxx 段
//!
//! 与 vco04（0x5Axx）/vco03（0x59xx）/vcq02（0x58xx）/vco01（0x55xx）
//! 等互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、全域降级状态机四态闭集
// ---------------------------------------------------------------------------

/// 全域降级状态机（官方四态闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeState {
    /// 正常——无降级活动。
    Normal,
    /// 降级中——触发已确认，降档动作推进中。
    Descending,
    /// 降级档——降档动作已生效，驻留降级档位。
    Degraded,
    /// 恢复——触发解除，逐级回升中。
    Recovering,
}

/// 状态总数。
pub const STATE_COUNT: usize = 4;

impl DegradeState {
    /// 全部状态（官方序）。
    pub const ALL: [DegradeState; STATE_COUNT] = [
        DegradeState::Normal,
        DegradeState::Descending,
        DegradeState::Degraded,
        DegradeState::Recovering,
    ];

    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            DegradeState::Normal => "正常".to_string(),
            DegradeState::Descending => "降级中".to_string(),
            DegradeState::Degraded => "降级档".to_string(),
            DegradeState::Recovering => "恢复".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、迁移表全枚举（形式化复用 J03——迁移条件显性）
// ---------------------------------------------------------------------------

/// 形式化复用联动单号（J03/F1475 形式化验证模式——判据独立对拍）。
pub const FORMAL_UPLINK: u32 = 1475;
/// 形式化复用声明（判据逐字对拍）。
pub const FORMAL_REUSE_NOTE: &str =
    "状态机形式化复用 J03——迁移表全枚举+不可达态检查，形式化语义不另立";
/// 迁移复用声明（锚点「迁移条件显性——迁移复用」——判据逐字对拍）。
pub const MIGRATION_REUSE_NOTE: &str =
    "迁移条件显性——每条迁移的触发条件字面量冻结，不隐式推断，迁移语义全域唯一";
/// 与 vco01 五段流水线的分工声明（段 vs 态——判据非空对拍）。
pub const STAGE_LINK_NOTE: &str =
    "与 vco01 分工——段=管线步进一次性推进，态=生命周期状态可驻留可回退，两账并行不混用";

/// 一条合法迁移（from/to/条件——三件齐，条件字面量显性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// 起始态。
    pub from: DegradeState,
    /// 目标态。
    pub to: DegradeState,
    /// 显性条件（字面量冻结——判据独立对拍）。
    pub condition: &'static str,
}

/// 合法迁移表（全枚举恰 6 条——C(4,2) 有向对中合法的 6 向）。
pub const MIGRATIONS: [Migration; 6] = [
    Migration {
        from: DegradeState::Normal,
        to: DegradeState::Descending,
        condition: "触发源融合事件激活（F2243 七源任一确认）",
    },
    Migration {
        from: DegradeState::Descending,
        to: DegradeState::Degraded,
        condition: "降级决策下发完成（O02 决策引擎联动预留）",
    },
    Migration {
        from: DegradeState::Descending,
        to: DegradeState::Normal,
        condition: "短促扰动解除，未达降档门槛即回退",
    },
    Migration {
        from: DegradeState::Degraded,
        to: DegradeState::Recovering,
        condition: "触发条件解除持续确认（滞回判定通过）",
    },
    Migration {
        from: DegradeState::Recovering,
        to: DegradeState::Normal,
        condition: "恢复完成全链回绿（各级动作复位到位）",
    },
    Migration {
        from: DegradeState::Recovering,
        to: DegradeState::Descending,
        condition: "恢复途中再触发（重入降级，防振荡判定在前）",
    },
];

/// 状态机形式化：迁移表全枚举完整性 + 不可达态检查。
///
/// 复用 J03 形式化语义：① 恰 6 条且两两不重复；② 无自迁移条目混入；
/// ③ 从正常态沿表可达全部四态（不可达态=建模漏洞）。
pub fn formal_check() -> Result<(), ScCode> {
    if MIGRATIONS.len() != 6 {
        return Err(ScCode::STATE_TABLE_BROKEN);
    }
    let mut i = 0;
    while i < MIGRATIONS.len() {
        let x = &MIGRATIONS[i];
        if x.from == x.to {
            return Err(ScCode::STATE_TABLE_BROKEN);
        }
        let mut j = i + 1;
        while j < MIGRATIONS.len() {
            let y = &MIGRATIONS[j];
            if x.from == y.from && x.to == y.to {
                return Err(ScCode::STATE_TABLE_BROKEN);
            }
            j += 1;
        }
        i += 1;
    }
    // 不可达态检查（判据侧独立重算同款 BFS）：从 Normal 出发逐轮扩散。
    let mut reach = [false; STATE_COUNT];
    reach[DegradeState::Normal as usize] = true;
    let mut grew = true;
    while grew {
        grew = false;
        for m in MIGRATIONS.iter() {
            let f = m.from as usize;
            let t = m.to as usize;
            if reach[f] && !reach[t] {
                reach[t] = true;
                grew = true;
            }
        }
    }
    let mut k = 0;
    while k < STATE_COUNT {
        if !reach[k] {
            return Err(ScCode::UNREACHABLE_STATE);
        }
        k += 1;
    }
    Ok(())
}

/// 状态迁移：查显性迁移表裁决。
///
/// 规则：① 同态自迁移显性拒绝；② 表内命中→返回目标态；③ 表外迁移
/// （跳段/倒退）逐类显性拒绝——迁移语义全域唯一。
pub fn transition(current: DegradeState, target: DegradeState) -> Result<DegradeState, ScCode> {
    if current == target {
        return Err(ScCode::SAME_STATE);
    }
    for m in MIGRATIONS.iter() {
        if m.from == current && m.to == target {
            return Ok(target);
        }
    }
    Err(ScCode::ILLEGAL_TRANSITION)
}

/// 带迁移史的迁移：校验接续 + 全量留痕（可观测复用）。
///
/// 史末态与 current 不接续=史断裂（HISTORY_BROKEN）；合法迁移后追加
/// 目标态留痕——迁移史全量保存供审计。
pub fn transition_with_history(
    history: &[DegradeState],
    current: DegradeState,
    target: DegradeState,
) -> Result<Vec<DegradeState>, ScCode> {
    match history.last() {
        Some(last) if *last != current => return Err(ScCode::HISTORY_BROKEN),
        None => {
            if current != DegradeState::Normal {
                return Err(ScCode::HISTORY_BROKEN);
            }
        }
        _ => {}
    }
    let next = transition(current, target)?;
    let mut out = Vec::with_capacity(history.len() + 1);
    for s in history.iter() {
        out.push(*s);
    }
    out.push(next);
    Ok(out)
}

// ---------------------------------------------------------------------------
// 三、错误契约（独占 0x5Bxx 段）
// ---------------------------------------------------------------------------

/// vco05 诊断码。独占 `0x5Bxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScCode(pub u16);

impl ScCode {
    /// 表外迁移（跳段/倒退）。
    pub const ILLEGAL_TRANSITION: ScCode = ScCode(0x5B01);
    /// 同态自迁移。
    pub const SAME_STATE: ScCode = ScCode(0x5B02);
    /// 迁移表完整性破坏。
    pub const STATE_TABLE_BROKEN: ScCode = ScCode(0x5B03);
    /// 不可达态（形式化检查失败——防御位）。
    pub const UNREACHABLE_STATE: ScCode = ScCode(0x5B04);
    /// 迁移史断裂（史末态与申报当前态不接续）。
    pub const HISTORY_BROKEN: ScCode = ScCode(0x5B05);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            ScCode::ILLEGAL_TRANSITION => "表外迁移：跳段/倒退不在迁移表内——迁移语义全域唯一".into(),
            ScCode::SAME_STATE => "同态自迁移：迁移必须改变状态".into(),
            ScCode::STATE_TABLE_BROKEN => "迁移表完整性破坏：条数/重复/自迁移条目混入".into(),
            ScCode::UNREACHABLE_STATE => "不可达态：形式化检查发现建模漏洞".into(),
            ScCode::HISTORY_BROKEN => "迁移史断裂：史末态与申报当前态不接续".into(),
            ScCode(_) => "未知 vco05 降级状态机域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑（锚点三组：状态机/形式化/迁移）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 四态闭集与全链迁移() {
        assert_eq!(STATE_COUNT, 4);
        assert_eq!(
            DegradeState::ALL,
            [
                DegradeState::Normal,
                DegradeState::Descending,
                DegradeState::Degraded,
                DegradeState::Recovering,
            ]
        );
        // 合法全链：正常→降级中→降级档→恢复→正常
        let chain = transition(DegradeState::Normal, DegradeState::Descending);
        assert_eq!(chain, Ok(DegradeState::Descending));
        assert_eq!(
            transition(DegradeState::Descending, DegradeState::Degraded),
            Ok(DegradeState::Degraded)
        );
        assert_eq!(
            transition(DegradeState::Degraded, DegradeState::Recovering),
            Ok(DegradeState::Recovering)
        );
        assert_eq!(
            transition(DegradeState::Recovering, DegradeState::Normal),
            Ok(DegradeState::Normal)
        );
    }

    #[test]
    fn 形式化迁移表全枚举() {
        assert_eq!(formal_check(), Ok(()));
        assert_eq!(MIGRATIONS.len(), 6);
        assert_eq!(FORMAL_UPLINK, 1475);
    }

    #[test]
    fn 非法迁移与迁移史() {
        // 跳段/倒退/自迁移逐类拒
        assert_eq!(
            transition(DegradeState::Normal, DegradeState::Degraded),
            Err(ScCode::ILLEGAL_TRANSITION)
        );
        assert_eq!(
            transition(DegradeState::Degraded, DegradeState::Descending),
            Err(ScCode::ILLEGAL_TRANSITION)
        );
        assert_eq!(
            transition(DegradeState::Normal, DegradeState::Normal),
            Err(ScCode::SAME_STATE)
        );
        // 带史合法链留痕完整
        let h0: Vec<DegradeState> = Vec::new();
        let h1 = transition_with_history(&h0, DegradeState::Normal, DegradeState::Descending).unwrap();
        assert_eq!(h1.len(), 1);
        let h2 = transition_with_history(&h1, DegradeState::Descending, DegradeState::Degraded).unwrap();
        assert_eq!(h2.len(), 2);
        assert_eq!(h2[1], DegradeState::Degraded);
        // 史断裂拒
        assert_eq!(
            transition_with_history(&h2, DegradeState::Normal, DegradeState::Descending),
            Err(ScCode::HISTORY_BROKEN)
        );
    }
}
