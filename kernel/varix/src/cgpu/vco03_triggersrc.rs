//! CGPU-F2243 · 降级触发源汇总（CGPU-O 域 · 降级链 · 触发源主题）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2243`
//!
//! 触发汇总：全域触发源汇总（帧超时/热档/续航档/CGPU 档位/弱网/资源
//! 紧张/场景切换——七类触发源——源表）；源注册（可扩展——扩展复用）；
//! 源融合（多源并发——融合规则复用 F1459）；测试（源表/扩展/融合三组）。
//!
//! ## 要点一：七类触发源闭集
//!
//! 帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换——全域降级触
//! 发源封闭枚举，表外不立触发源；每源一条中文标签（字面量冻结，判据
//! 独立对拍）。
//!
//! ## 要点二：源注册可扩展
//!
//! 注册表从空表起步逐源注册（追加式扩展）；同源重复注册拒绝；注册序
//! 即优先级序（确定性——平局裁决的依据）；新源追加不改旧源语义（扩展
//! 复用）。
//!
//! ## 要点三：源融合复用 F1459
//!
//! 多源并发时融合裁决：主源=严重度最高者；严重度平局取注册序最早
//! （确定性，不掷骰子）；贡献源账按注册序升序全量留痕。融合规则复用
//! F1459 不另立——融合语义分叉则全域降级行为不可预测。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x59xx 段
//!
//! 与 vcq02（0x58xx）/cgr01（0x57xx）/vcq01（0x56xx）/vco01（0x55xx）
//! 等互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、七类触发源闭集
// ---------------------------------------------------------------------------

/// 全域降级触发源（官方七类闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    /// 帧超时。
    FrameTimeout,
    /// 热档。
    ThermalStep,
    /// 续航档。
    BatteryStep,
    /// CGPU 档位。
    GpuGear,
    /// 弱网。
    WeakNet,
    /// 资源紧张。
    ResourceTight,
    /// 场景切换。
    SceneSwitch,
}

/// 触发源总数。
pub const TRIGGER_COUNT: usize = 7;

impl TriggerKind {
    /// 全部触发源（官方序）。
    pub const ALL: [TriggerKind; TRIGGER_COUNT] = [
        TriggerKind::FrameTimeout,
        TriggerKind::ThermalStep,
        TriggerKind::BatteryStep,
        TriggerKind::GpuGear,
        TriggerKind::WeakNet,
        TriggerKind::ResourceTight,
        TriggerKind::SceneSwitch,
    ];

    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            TriggerKind::FrameTimeout => "帧超时".to_string(),
            TriggerKind::ThermalStep => "热档".to_string(),
            TriggerKind::BatteryStep => "续航档".to_string(),
            TriggerKind::GpuGear => "CGPU 档位".to_string(),
            TriggerKind::WeakNet => "弱网".to_string(),
            TriggerKind::ResourceTight => "资源紧张".to_string(),
            TriggerKind::SceneSwitch => "场景切换".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、源表与可扩展注册
// ---------------------------------------------------------------------------

/// 已注册触发源（注册序即优先级序——确定性平局裁决依据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerSource {
    /// 触发源类别。
    pub kind: TriggerKind,
    /// 注册序（0 起连续——即融合平局时的优先级）。
    pub priority: u8,
    /// 是否启用（注册即可用；禁用语义留给决策引擎）。
    pub enabled: bool,
}

/// 触发源注册表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRegistry {
    /// 已注册源（按注册序排列）。
    pub sources: Vec<TriggerSource>,
}

/// 扩展复用声明（锚点「可扩展」——判据逐字对拍）。
pub const EXTENSIBLE_NOTE: &str = "源注册可扩展——新触发源追加注册，不改旧源语义";

/// 空注册表（扩展起步形态——从空表逐源注册）。
pub fn new_registry() -> SourceRegistry {
    SourceRegistry { sources: Vec::new() }
}

/// 注册一个触发源（追加式扩展：新源 priority=当前长度，旧源不动）。
///
/// 同类源重复注册拒绝（DUPLICATE_SOURCE）。
pub fn register_source(registry: &mut SourceRegistry, kind: TriggerKind) -> Result<u8, OtCode> {
    for s in registry.sources.iter() {
        if s.kind == kind {
            return Err(OtCode::DUPLICATE_SOURCE);
        }
    }
    let priority = registry.sources.len() as u8;
    registry.sources.push(TriggerSource { kind, priority, enabled: true });
    Ok(priority)
}

/// 预注册默认源表（七源全量，priority 0..6——字面量钉死）。
pub fn default_registry() -> SourceRegistry {
    let mut r = new_registry();
    let mut i = 0;
    while i < TRIGGER_COUNT {
        let _ = register_source(&mut r, TriggerKind::ALL[i]);
        i += 1;
    }
    r
}

// ---------------------------------------------------------------------------
// 三、源融合（多源并发——复用 F1459）
// ---------------------------------------------------------------------------

/// 融合规则复用声明（锚点「融合规则复用 F1459」——判据独立对拍）。
pub const FUSION_UPLINK: u32 = 1459;
/// 联动人话。
pub const FUSION_UPLINK_NOTE: &str = "多源并发融合规则复用 F1459——融合语义不另立";

/// 触发事件（一次触发源上报）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerEvent {
    /// 触发源类别。
    pub kind: TriggerKind,
    /// 上报时间戳（确定性 tick，非墙钟）。
    pub tick: u64,
    /// 严重度（0..=9，越界拒绝）。
    pub severity: u8,
}

/// 严重度上限（域外拒绝——语义显性）。
pub const MAX_SEVERITY: u8 = 9;

/// 融合结果（主源裁决 + 贡献源全量账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusedOutcome {
    /// 主源（严重度最高；平局取注册序最早——确定性）。
    pub dominant: TriggerKind,
    /// 贡献源账（去重，按注册序升序）。
    pub contributors: Vec<TriggerKind>,
}

/// 多源并发融合：主源裁决 + 贡献源留痕。
///
/// 规则（复用 F1459）：① 空事件表拒绝；② severity 域外拒绝；③ 未注册
/// 源拒绝（账实相符优先于静默吞并）；④ 主源=severity 最高，平局取
/// priority 最小（注册序最早）；⑤ 贡献源去重升序留痕。
pub fn fuse(events: &[TriggerEvent], registry: &SourceRegistry) -> Result<FusedOutcome, OtCode> {
    if events.is_empty() {
        return Err(OtCode::EMPTY_EVENTS);
    }
    // 校验：severity 域外 + 未注册源
    for e in events.iter() {
        if e.severity > MAX_SEVERITY {
            return Err(OtCode::SEVERITY_OUT_OF_RANGE);
        }
        let mut registered = false;
        for s in registry.sources.iter() {
            if s.kind == e.kind && s.enabled {
                registered = true;
            }
        }
        if !registered {
            return Err(OtCode::UNREGISTERED_SOURCE);
        }
    }
    // 主源裁决：severity 最高；平局取 priority 最小（确定性）
    let mut best_idx = 0usize;
    let mut i = 1;
    while i < events.len() {
        let a = &events[best_idx];
        let b = &events[i];
        let pa = prio_of(registry, a.kind);
        let pb = prio_of(registry, b.kind);
        if b.severity > a.severity || (b.severity == a.severity && pb < pa) {
            best_idx = i;
        }
        i += 1;
    }
    // 贡献源账：去重，按注册序升序
    let mut contributors: Vec<TriggerKind> = Vec::new();
    let mut order = 0u8;
    while (order as usize) < registry.sources.len() {
        for s in registry.sources.iter() {
            if s.priority == order {
                for e in events.iter() {
                    if e.kind == s.kind && !contributors.contains(&e.kind) {
                        contributors.push(e.kind);
                    }
                }
            }
        }
        order += 1;
    }
    Ok(FusedOutcome { dominant: events[best_idx].kind, contributors })
}

/// 查某源的注册序（未注册返回 u8::MAX——调用前已核注册性，不会取到）。
fn prio_of(registry: &SourceRegistry, kind: TriggerKind) -> u8 {
    for s in registry.sources.iter() {
        if s.kind == kind {
            return s.priority;
        }
    }
    u8::MAX
}

// ---------------------------------------------------------------------------
// 四、错误契约（独占 0x59xx 段）
// ---------------------------------------------------------------------------

/// vco03 诊断码。独占 `0x59xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtCode(pub u16);

impl OtCode {
    /// 同类源重复注册。
    pub const DUPLICATE_SOURCE: OtCode = OtCode(0x5901);
    /// 空事件表融合。
    pub const EMPTY_EVENTS: OtCode = OtCode(0x5902);
    /// 严重度域外（>9）。
    pub const SEVERITY_OUT_OF_RANGE: OtCode = OtCode(0x5903);
    /// 未注册源上报。
    pub const UNREGISTERED_SOURCE: OtCode = OtCode(0x5904);
    /// 融合产出空账（防御位——注册表与事件表不一致时兜底）。
    pub const FUSION_CONFLICT: OtCode = OtCode(0x5905);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            OtCode::DUPLICATE_SOURCE => "同类源重复注册：扩展是追加不是覆盖".into(),
            OtCode::EMPTY_EVENTS => "空事件表融合：无触发不裁决".into(),
            OtCode::SEVERITY_OUT_OF_RANGE => "严重度域外：severity 必须落在 0..=9".into(),
            OtCode::UNREGISTERED_SOURCE => "未注册源上报：先注册再触发".into(),
            OtCode::FUSION_CONFLICT => "融合产出空账：注册表与事件表不一致".into(),
            OtCode(_) => "未知 vco03 触发源汇总域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑（回归三组：源表/扩展/融合）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 七类闭集与默认注册表() {
        let r = default_registry();
        assert_eq!(r.sources.len(), 7);
        for i in 0..7 {
            assert_eq!(r.sources[i].kind, TriggerKind::ALL[i]);
            assert_eq!(r.sources[i].priority, i as u8);
        }
    }

    #[test]
    fn 重复注册拒绝() {
        let mut r = new_registry();
        assert!(register_source(&mut r, TriggerKind::FrameTimeout).is_ok());
        assert_eq!(
            register_source(&mut r, TriggerKind::FrameTimeout),
            Err(OtCode::DUPLICATE_SOURCE)
        );
    }

    #[test]
    fn 多源并发融合确定性() {
        let r = default_registry();
        let evs = [
            TriggerEvent { kind: TriggerKind::FrameTimeout, tick: 1, severity: 3 },
            TriggerEvent { kind: TriggerKind::ThermalStep, tick: 2, severity: 5 },
        ];
        let out = fuse(&evs, &r).unwrap();
        assert_eq!(out.dominant, TriggerKind::ThermalStep);
        assert_eq!(out.contributors.len(), 2);
        // 平局：severity 相同 → 注册序最早（帧超时）胜
        let tie = [
            TriggerEvent { kind: TriggerKind::ThermalStep, tick: 1, severity: 4 },
            TriggerEvent { kind: TriggerKind::FrameTimeout, tick: 2, severity: 4 },
        ];
        let out2 = fuse(&tie, &r).unwrap();
        assert_eq!(out2.dominant, TriggerKind::FrameTimeout);
    }
}
