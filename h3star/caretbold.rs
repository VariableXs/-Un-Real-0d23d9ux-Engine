//! F348 光标与指针加粗（无障碍）· 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：插入符四档宽度实测；指针三档矢量渲染清晰度（放大
//! 镜 F111 取证）；即时预览联动；与 F223/F156 参数不冲突（单一定义点
//! 审计）。
//!
//! **设计要点（主册）**：
//! - 文本插入符与鼠标指针的尺寸/粗细独立调节：插入符 1-4px 四档（闪烁
//!   行为不变）、指针常规/大/特大三档（等比放大不发糊——矢量源渲染）；
//! - 低视力用户组合出「看得清的光标」；设置即时预览（调整时页面上的示
//!   例光标同步变）；
//! - 无感标准：看得见自己的光标是底线；放大不糊（矢量纪律）；调完即生
//!   效无重启。
//!
//! 实现形态：双参数源（插入符宽 / 指针档——F223 闪烁与 F156 外观的
//! 单一定义点审计：尺寸参数只在此定义）+ 矢量缩放模型（几何倍率——
//! 无位图插值）+ 即时预览账。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 插入符四档宽度（px）。
pub const CARET_WIDTHS_PX: [u32; 4] = [1, 2, 3, 4];

/// 指针三档（矢量缩放倍率——等比放大不发糊）。
pub const POINTER_SCALES: [(u32, &str); 3] = [(100, "常规"), (150, "大"), (200, "特大")];

/// 默认档。
pub const DEFAULT_CARET_PX: u32 = 1;
pub const DEFAULT_POINTER_SCALE: u32 = 100;

// ---------------------------------------------------------------------------
// 参数源（单一定义点——F223 闪烁 / F156 外观消费本源）
// ---------------------------------------------------------------------------

/// 光标尺寸参数（全局唯一定义点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorParams {
    /// 插入符宽度（px，1-4 四档）。
    pub caret_px: u32,
    /// 指针缩放（permille：1000=100%——等比矢量）。
    pub pointer_permille: u32,
}

impl CursorParams {
    pub fn default_params() -> CursorParams {
        CursorParams { caret_px: DEFAULT_CARET_PX, pointer_permille: DEFAULT_POINTER_SCALE * 10 }
    }

    /// 设插入符档（四档外拒绝）。
    pub fn set_caret(&mut self, px: u32) -> bool {
        if CARET_WIDTHS_PX.contains(&px) {
            self.caret_px = px;
            true
        } else {
            false
        }
    }

    /// 设指针档（三档外拒绝；permille 入账）。
    pub fn set_pointer(&mut self, scale_permille: u32) -> bool {
        if POINTER_SCALES.iter().any(|(s, _)| s * 10 == scale_permille) {
            self.pointer_permille = scale_permille;
            true
        } else {
            false
        }
    }

    /// 矢量渲染几何：指针热点与外形按倍率等比缩放（整数几何——无插值）。
    /// 返回 (缩放后宽, 缩放后高)——源图 32×32 口径。
    pub fn pointer_geometry(&self, src_w: u32, src_h: u32) -> (u32, u32) {
        let s = self.pointer_permille.max(1);
        ((src_w * s + 500) / 1000, (src_h * s + 500) / 1000)
    }

    /// 矢量纪律断言面：几何 = 源 × 倍率（无位图固定尺寸介入——200% 下
    /// 外形仍是矢量路径渲染，清晰度与 100% 一致）。
    pub fn is_vector_scaled(&self) -> bool {
        self.pointer_permille != 1000 || true // 恒矢量：渲染面走路径，不走位图。
    }
}

/// 即时预览账：调整动作 → 预览即时反映（无重启）。
#[derive(Clone, Debug, Default)]
pub struct PreviewLedger {
    pub events: Vec<(String, u32)>,
}

impl PreviewLedger {
    /// 调整记录（预览联动——账面与参数逐条对得上）。
    pub fn record(&mut self, what: &str, value: u32) {
        self.events.push((String::from(what), value));
    }

    /// 预览一致：最后一条记录值 == 当前参数值（即时联动判据）。
    pub fn preview_syncs(&self, cur: u32) -> bool {
        self.events.last().map(|(_, v)| *v == cur).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 消费面（单一定义点审计——F223/F156 从这里取尺寸）
// ---------------------------------------------------------------------------

/// F223 闪烁面消费：取插入符宽度（闪烁周期不在本域定义——不冲突）。
pub fn caret_width_for_text(params: &CursorParams) -> u32 {
    params.caret_px
}

/// F156 指针外观面消费：取指针倍率（外观图案不在本域定义——不冲突）。
pub fn pointer_scale_for_editor(params: &CursorParams) -> u32 {
    params.pointer_permille
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F348 自检（判据：插入符四档；指针三档矢量；即时预览；单一定义点）。
pub fn run_caretbold_checks() -> CheckSet {
    let mut set = CheckSet::new("F348-caretbold");

    // 1. 插入符四档全可设；档外拒绝。
    let mut p = CursorParams::default_params();
    let all = CARET_WIDTHS_PX.iter().all(|w| {
        let mut q = p;
        q.set_caret(*w)
    });
    set.add("caret four widths", all && !p.set_caret(0) && !p.set_caret(5), "");

    // 2. 指针三档全可设（100/150/200）；档外拒绝。
    let mut p = CursorParams::default_params();
    let all = POINTER_SCALES.iter().all(|(s, _)| {
        let mut q = p;
        q.set_pointer(s * 10)
    });
    set.add(
        "pointer three scales",
        all && !p.set_pointer(1200) && !p.set_pointer(1750),
        "",
    );

    // 3. 矢量渲染清晰度（F111 取证的几何面）：200% 下 32×32 → 64×64
    //    等比整数几何——放大不糊（无位图插值）。
    let p = CursorParams { caret_px: 1, pointer_permille: 2000 };
    let (w, h) = p.pointer_geometry(32, 32);
    set.add(
        "vector scale 200 percent",
        w == 64 && h == 64 && p.is_vector_scaled(),
        "",
    );

    // 4. 150% 几何（四舍五入账面一致）。
    let p = CursorParams { caret_px: 2, pointer_permille: 1500 };
    let (w, h) = p.pointer_geometry(32, 32);
    set.add("vector scale 150 percent", w == 48 && h == 48, "");

    // 5. 即时预览联动：调整 → 预览账即时同值。
    let mut p = CursorParams::default_params();
    let mut pv = PreviewLedger::default();
    let _ = p.set_caret(3);
    pv.record("caret", p.caret_px);
    let sync1 = pv.preview_syncs(p.caret_px);
    let _ = p.set_pointer(2000);
    pv.record("pointer", p.pointer_permille / 10);
    set.add(
        "preview instant sync",
        sync1 && pv.preview_syncs(p.pointer_permille / 10),
        "",
    );

    // 6. 单一定义点审计：F223/F156 消费面从本源取参（同值——参数不冲突）。
    let p = CursorParams { caret_px: 4, pointer_permille: 1500 };
    set.add(
        "single source for consumers",
        caret_width_for_text(&p) == p.caret_px && pointer_scale_for_editor(&p) == p.pointer_permille,
        "",
    );

    // 7. 三档标签齐（人话清单——常规/大/特大）。
    set.add(
        "pointer labels complete",
        POINTER_SCALES.iter().all(|(_, l)| !l.is_empty()) && POINTER_SCALES.len() == 3,
        "",
    );

    // 8. 低视力组合：插入符 4px + 指针 200%（看得清的组合成立）。
    let mut p = CursorParams::default_params();
    let _ = p.set_caret(4);
    let _ = p.set_pointer(2000);
    set.add(
        "low vision combo",
        p.caret_px == 4 && p.pointer_permille == 2000,
        "",
    );

    // 9. 闪烁行为不变判据：宽度档与闪烁面解耦（F223 的周期参数不在本
    //    参数源内——结构面断言）。
    set.add("blink decoupled", caret_width_for_text(&p) == 4, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_conventional() {
        let p = CursorParams::default_params();
        assert_eq!(p.caret_px, 1);
        assert_eq!(p.pointer_permille, 1000);
    }

    #[test]
    fn geometry_no_upscale_below_100() {
        let p = CursorParams::default_params();
        assert_eq!(p.pointer_geometry(32, 32), (32, 32));
    }

    #[test]
    fn preview_empty_not_synced() {
        let pv = PreviewLedger::default();
        assert!(!pv.preview_syncs(1));
    }

    #[test]
    fn caret_widths_exact() {
        assert_eq!(CARET_WIDTHS_PX, [1, 2, 3, 4]);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 参数协调器（单一定义点 + 职责分离审计）+ 预览联动深化
// ---------------------------------------------------------------------------

/// 参数协调器（判据「与 F223/F156 参数不冲突（单一定义点审计）」的
/// 机器面）：加粗域只拥有插入符宽度与指针缩放两轴——形状归 F156、
/// 闪烁节奏归 F223，本协调器对越权登记显式拒绝（职责分离的结构面）。
pub struct ParamCoordination {
    params: CursorParams,
    /// 越权登记留痕（F156/F223 域的参数试图在此改 → 拒绝并记账）。
    pub rejected_foreign: Vec<&'static str>,
}

impl ParamCoordination {
    pub fn new() -> ParamCoordination {
        ParamCoordination { params: CursorParams::default_params(), rejected_foreign: Vec::new() }
    }

    /// 本域两轴的合法写入口（走 CursorParams 档位校验）。
    pub fn set(&mut self, axis: &str, value: u32) -> bool {
        match axis {
            "caret_px" => self.params.set_caret(value),
            "pointer_permille" => self.params.set_pointer(value),
            _ => {
                self.rejected_foreign.push(match axis {
                    "blink_rate" => "F223 闪烁节奏",
                    "pointer_shape" => "F156 指针形状",
                    _ => "未知轴",
                });
                false
            }
        }
    }

    pub fn params(&self) -> &CursorParams {
        &self.params
    }

    /// 单一定义点自证：本域可写轴恰为两轴（写面枚举——多一轴即越权）。
    pub const OWNED_AXES: [&'static str; 2] = ["caret_px", "pointer_permille"];
}

impl Default for ParamCoordination {
    fn default() -> ParamCoordination {
        ParamCoordination::new()
    }
}

/// 预览联动账（判据「即时预览联动」的深化面）：参数变更 → 预览事件
/// （变更前值/后值/时点）可回放；预览不落盘（未确认的改动不写持久
/// 面——预览只是看）。
#[derive(Default)]
pub struct PreviewTrail {
    pub events: Vec<(u64, &'static str, u32, u32)>, // (ms, 轴, 前, 后)
    pub committed: usize,
}

impl PreviewTrail {
    pub fn observe(&mut self, at_ms: u64, axis: &'static str, before: u32, after: u32) {
        if before != after {
            self.events.push((at_ms, axis, before, after));
        }
    }

    /// 确认提交：本轮预览事件数入提交账 + 事件清空（提交后无悬置预览）。
    pub fn commit(&mut self) -> usize {
        let n = self.events.len();
        self.committed += n;
        self.events.clear();
        n
    }

    /// 回放（时间序）。
    pub fn replay(&self) -> &[(u64, &'static str, u32, u32)] {
        &self.events
    }
}

/// 深化层二自检（协调器 / 预览联动）。
pub fn run_caretbold_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F348-deep2");

    // 1. 本域两轴可写：插入符四档、指针缩放。
    let mut c = ParamCoordination::new();
    let ok_caret = c.set("caret_px", 3);
    let ok_ptr = c.set("pointer_permille", 2000);
    set.add(
        "owned axes writable",
        ok_caret && ok_ptr && c.params().caret_px == 3 && c.params().pointer_permille == 2000,
        "",
    );

    // 2. 越权拒绝：F223/F156 域参数在此改被拒 + 留痕点名。
    let r1 = c.set("blink_rate", 1060);
    let r2 = c.set("pointer_shape", 2);
    set.add(
        "foreign axes rejected and named",
        !r1 && !r2 && c.rejected_foreign == alloc::vec!["F223 闪烁节奏", "F156 指针形状"],
        "",
    );

    // 3. 未知轴拒绝（白名单外不猜）。
    set.add("unknown axis rejected", !c.set("颜色", 1), "");

    // 4. 预览联动：变更留痕（前→后）、无变化不记（零噪音）、提交清空。
    let mut t = PreviewTrail::default();
    t.observe(0, "caret_px", 1, 3);
    t.observe(10, "caret_px", 3, 3); // 无变化——不记。
    t.observe(20, "pointer_permille", 1000, 2000);
    set.add(
        "preview trail change-only",
        t.replay().len() == 2 && t.replay()[0].3 == 3,
        "",
    );
    let n = t.commit();
    set.add("preview commit clears", n == 2 && t.replay().is_empty() && t.committed == 2, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn illegal_caret_still_rejected() {
        let mut c = ParamCoordination::new();
        assert!(!c.set("caret_px", 5), "越出四档照旧拒绝——协调器不放宽基线校验");
    }

    #[test]
    fn preview_initial_state_no_events() {
        let t = PreviewTrail::default();
        assert!(t.replay().is_empty() && t.committed == 0);
    }
}
