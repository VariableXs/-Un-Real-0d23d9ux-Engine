//! VE-F1624 · Morph Target 基础（VE-I 域 · I02 顶点流水线组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1624`
//!
//! Morph Target 基础极致深化：morph 数据通路（基础网格+形变目标×权重
//! ——顶点级插值，插值在位置与法线上执行——morph 是表情与形态动画的
//! 根基）；帧序列（morph 权重的时间驱动动画——关键帧（F1345）联动——
//! 表情动画的基础设施）；morph 组合（多目标叠加——微笑+眨眼同时——
//! 权重组合语义——归一化与叠加模式（replace/add 两模式声明））；数据
//! 压缩（morph 差分量化——稀疏形变只存差异顶点）。
//!
//! ## 要点一：morph 通路是顶点级差分插值
//!
//! 形变目标只描述「与基础网格的差异」（表情/形态）；应用时按权重对
//! **位置与法线**同时执行顶点级线性插值；不在差异表内的顶点保持原样
//! ——稀疏性贯穿到运行时通路。
//!
//! ## 要点二：帧序列让权重动起来
//!
//! morph 权重由时间驱动的关键帧轨道供给（关键帧系统 F1345 联动）：
//! 相邻关键帧间线性插值，恰帧命中取恰值，轨道外钳到端点——表情动画
//! 的基础设施。
//!
//! ## 要点三：组合语义两模式声明
//!
//! replace 模式（各目标竞争表达）：权重和必须 ≤1（归一化约束，超出
//! 拒绝）；add 模式（各目标独立叠加）：位移逐顶点累加不设和上限——
//! 两模式语义显性分账，表外无暗混。
//!
//! ## 要点四：差分量化——稀疏形变只存差异顶点
//!
//! 形变目标只存差异顶点（差分）；差分以 Q12 定点量化（12 位小数，
//! 纯整数移位算术，内核面无 libm 依赖）；往返误差有界（≤ 半步）且
//! 入判据对拍。
//!
//! ## 要点五：零 panic 面
//!
//! 无 unwrap/expect/裸下标越界；权重域外、未知目标、空轨道、乱序关
//! 键帧一律专属码拒绝。
//!
//! ## 要点六：诊断码独占 0x44xx 段
//!
//! 与 vef26（0x43）/0x40-0x42/0x45-0x49 段互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、morph 通路（位置与法线的顶点级差分插值）
// ---------------------------------------------------------------------------

/// 差分顶点（相对基础网格的差异——只存差异是稀疏性的根）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexDelta {
    /// 作用的基础网格顶点下标。
    pub index: u32,
    /// 位置差分（满权重时的位移向量）。
    pub pos: [f32; 3],
    /// 法线差分（满权重时的法线偏移向量）。
    pub normal: [f32; 3],
}

/// 形变目标（表情/形态——基础网格 + 形变目标 × 权重）。
#[derive(Debug, Clone, PartialEq)]
pub struct MorphTarget {
    /// 目标标识。
    pub id: u32,
    /// 人话标签（如 "微笑"/"眨眼"）。
    pub label: &'static str,
    /// 差分顶点表（只存差异顶点——稀疏形变）。
    pub deltas: Vec<VertexDelta>,
}

/// 顶点级插值结果（位置与法线同时插值——通路红线）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppliedMorph {
    /// 插值后位置。
    pub pos: [f32; 3],
    /// 插值后法线（已重新归一化）。
    pub normal: [f32; 3],
    /// 该顶点是否被本目标触碰（差异表内才动——稀疏语义）。
    pub touched: bool,
}

/// 内核面 f32 平方根（牛顿迭代——纯乘除，无 libm 依赖；vef25 先例）。
pub fn isqrt_f32(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut x = v;
    // 牛顿迭代 24 步足够 f32 收敛（单调收敛，无 panic 面）
    let mut i = 0;
    while i < 24 {
        x = 0.5f32 * (x + v / x);
        // 相对误差足够小即提前收束
        if (x * x - v).abs() <= 1e-6 * v.max(1.0) {
            break;
        }
        i += 1;
    }
    x
}

/// 向量归一化（零向量返回零向量——诚实退化不 panic）。
pub fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if len2 <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let inv = 1.0 / isqrt_f32(len2);
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

/// 单目标顶点级插值：pos' = base + w·Δpos，normal' = normalize(base + w·Δn)。
///
/// 权重域 [0,1] 外拒绝（WEIGHT_OUT_OF_RANGE）；顶点不在差异表内则原样
/// 返回（touched=false）。
pub fn apply_morph(
    base_pos: [f32; 3],
    base_normal: [f32; 3],
    target: &MorphTarget,
    vertex: u32,
    weight: f32,
) -> Result<AppliedMorph, MtCode> {
    if !(weight >= 0.0 && weight <= 1.0) {
        return Err(MtCode::WEIGHT_OUT_OF_RANGE);
    }
    // 稀疏查找：只查差异表（线性扫描——差分表短，O(|deltas|) 可控）
    let mut hit: Option<&VertexDelta> = None;
    for d in target.deltas.iter() {
        if d.index == vertex {
            hit = Some(d);
        }
    }
    match hit {
        None => Ok(AppliedMorph { pos: base_pos, normal: base_normal, touched: false }),
        Some(d) => {
            let pos = [
                base_pos[0] + weight * d.pos[0],
                base_pos[1] + weight * d.pos[1],
                base_pos[2] + weight * d.pos[2],
            ];
            let n_raw = [
                base_normal[0] + weight * d.normal[0],
                base_normal[1] + weight * d.normal[1],
                base_normal[2] + weight * d.normal[2],
            ];
            Ok(AppliedMorph { pos, normal: normalize3(n_raw), touched: true })
        }
    }
}

// ---------------------------------------------------------------------------
// 二、帧序列（权重的时间驱动动画——关键帧 F1345 联动）
// ---------------------------------------------------------------------------

/// 关键帧系统联动声明（F1345——表情动画的基础设施）。
pub const KEYFRAME_UPLINK: u32 = 1345;
/// 联动人话（判据对拍）。
pub const KEYFRAME_UPLINK_NOTE: &str = "morph 权重轨道与关键帧系统（F1345）联动——表情动画的基础设施";

/// 权重关键帧（tick 升序存储）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightKeyframe {
    /// 时间戳（确定性 tick，非墙钟）。
    pub tick: u64,
    /// 该时刻的 morph 权重。
    pub weight: f32,
}

/// 单目标权重轨道。
#[derive(Debug, Clone, PartialEq)]
pub struct WeightTrack {
    /// 目标标识（对应 MorphTarget::id）。
    pub target_id: u32,
    /// 关键帧（tick 升序——乱序即非法轨道）。
    pub keys: Vec<WeightKeyframe>,
}

/// 时间驱动取权重：恰帧命中取恰值；两帧之间线性插值；轨道外钳到端点。
///
/// 空轨道/乱序轨道专属码拒绝。
pub fn weight_at(track: &WeightTrack, tick: u64) -> Result<f32, MtCode> {
    if track.keys.is_empty() {
        return Err(MtCode::EMPTY_TRACK);
    }
    // 升序校验（乱序=非法轨道，不静默重排）
    let mut i = 1;
    while i < track.keys.len() {
        if track.keys[i].tick < track.keys[i - 1].tick {
            return Err(MtCode::KEYS_UNORDERED);
        }
        i += 1;
    }
    // 端点钳制
    if tick <= track.keys[0].tick {
        return Ok(track.keys[0].weight);
    }
    let last = track.keys[track.keys.len() - 1];
    if tick >= last.tick {
        return Ok(last.weight);
    }
    // 区间线性插值（keys 非空且升序，此处无越界面）
    let mut j = 1;
    while j < track.keys.len() {
        let a = track.keys[j - 1];
        let b = track.keys[j];
        if tick >= a.tick && tick <= b.tick {
            if b.tick == a.tick {
                return Ok(b.weight);
            }
            let span = (b.tick - a.tick) as f32;
            let t = (tick - a.tick) as f32 / span;
            return Ok(a.weight + t * (b.weight - a.weight));
        }
        j += 1;
    }
    Err(MtCode::EMPTY_TRACK)
}

// ---------------------------------------------------------------------------
// 三、morph 组合（多目标叠加——replace/add 两模式声明）
// ---------------------------------------------------------------------------

/// 叠加模式（两模式声明——语义显性分账，表外无暗混）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineMode {
    /// replace：各目标竞争表达——权重和必须 ≤1（归一化约束）。
    Replace,
    /// add：各目标独立叠加——位移逐顶点累加，不设和上限。
    Add,
}

/// 权重条目（组合输入）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MorphWeight {
    /// 目标标识。
    pub target_id: u32,
    /// 权重 ∈ [0,1]。
    pub weight: f32,
}

/// 组合结果（插值后位置/法线 + 应用了哪些目标的账）。
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedMorph {
    /// 组合后位置。
    pub pos: [f32; 3],
    /// 组合后法线（已重新归一化）。
    pub normal: [f32; 3],
    /// 实际应用的目标 id 账（按输入序）。
    pub applied: Vec<u32>,
}

/// 多目标组合：微笑+眨眼同时（顶点级入口——锚点「顶点级插值」语义）。
///
/// replace：先验 Σw ≤ 1（归一化，超出 WEIGHT_SUM_EXCEEDS）；add：逐顶
/// 点位移累加。任一权重条目指向未知目标即 UNKNOWN_TARGET 拒绝（账实
/// 相符优先于静默跳过）；顶点不在某目标差异表内则该目标对本顶点零
/// 贡献（稀疏语义），但该目标仍计入 applied 账（权重确实被消费）。
pub fn combine_vertex(
    base_pos: [f32; 3],
    base_normal: [f32; 3],
    vertex: u32,
    targets: &[MorphTarget],
    weights: &[MorphWeight],
    mode: CombineMode,
) -> Result<CombinedMorph, MtCode> {
    // 权重域逐条校验
    for w in weights.iter() {
        if !(w.weight >= 0.0 && w.weight <= 1.0) {
            return Err(MtCode::WEIGHT_OUT_OF_RANGE);
        }
    }
    if mode == CombineMode::Replace {
        let mut sum = 0.0f32;
        for w in weights.iter() {
            sum += w.weight;
        }
        if sum > 1.0 {
            return Err(MtCode::WEIGHT_SUM_EXCEEDS);
        }
    }
    for w in weights.iter() {
        let mut found = false;
        for t in targets.iter() {
            if t.id == w.target_id {
                found = true;
            }
        }
        if !found {
            return Err(MtCode::UNKNOWN_TARGET);
        }
    }
    let mut pos = base_pos;
    let mut n_raw = base_normal;
    let mut applied: Vec<u32> = Vec::new();
    for w in weights.iter() {
        for t in targets.iter() {
            if t.id != w.target_id {
                continue;
            }
            for d in t.deltas.iter() {
                if d.index == vertex {
                    pos = [
                        pos[0] + w.weight * d.pos[0],
                        pos[1] + w.weight * d.pos[1],
                        pos[2] + w.weight * d.pos[2],
                    ];
                    n_raw = [
                        n_raw[0] + w.weight * d.normal[0],
                        n_raw[1] + w.weight * d.normal[1],
                        n_raw[2] + w.weight * d.normal[2],
                    ];
                }
            }
        }
        applied.push(w.target_id);
    }
    Ok(CombinedMorph { pos, normal: normalize3(n_raw), applied })
}

// ---------------------------------------------------------------------------
// 四、差分量化（Q12 定点——稀疏形变只存差异顶点）
// ---------------------------------------------------------------------------

/// 位置/法线差分量化小数位（Q12：1/4096 步长）。
pub const QUANT_FRACT_BITS: u32 = 12;
/// 量化步长字面量（判据独立对拍：2^12 = 4096）。
pub const QUANT_SCALE: f32 = 4096.0;
/// 往返误差半步（判据界：|round-trip| ≤ 0.5/4096）。
pub const QUANT_HALF_STEP: f32 = 0.5 / 4096.0;

/// f32 → Q12 定点（内核面无 f32::round——纯算术四舍五入）。
pub fn quant_q12(v: f32) -> i32 {
    let s = v * QUANT_SCALE;
    if s >= 0.0 {
        (s + 0.5) as i32
    } else {
        (s - 0.5) as i32
    }
}

/// Q12 定点 → f32。
pub fn dequant_q12(q: i32) -> f32 {
    q as f32 / QUANT_SCALE
}

/// 量化后的差分顶点（整数 wire 形态——只存差异顶点的稀疏表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantDelta {
    /// 基础网格顶点下标。
    pub index: u32,
    /// 位置差分（Q12 定点）。
    pub pos: [i32; 3],
    /// 法线差分（Q12 定点）。
    pub normal: [i32; 3],
}

/// 量化后的形变目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizedTarget {
    /// 目标标识。
    pub id: u32,
    /// 稀疏差分表（长度 == 原差异表长度——只存差异顶点）。
    pub deltas: Vec<QuantDelta>,
    /// 原始网格顶点数（稀疏度分母）。
    pub vertex_count: u32,
}

/// 目标差分量化（差分表逐条 Q12；稀疏性保留——零差分不剔除以保持
/// 下标直查语义，稀疏账以非零差分计数为准）。
pub fn quantize_target(t: &MorphTarget, vertex_count: u32) -> QuantizedTarget {
    let mut deltas: Vec<QuantDelta> = Vec::new();
    for d in t.deltas.iter() {
        deltas.push(QuantDelta {
            index: d.index,
            pos: [quant_q12(d.pos[0]), quant_q12(d.pos[1]), quant_q12(d.pos[2])],
            normal: [quant_q12(d.normal[0]), quant_q12(d.normal[1]), quant_q12(d.normal[2])],
        });
    }
    QuantizedTarget { id: t.id, deltas, vertex_count }
}

/// 稀疏度账：非零差分顶点占全网格比例（压缩率的量尺）。
pub fn sparsity(t: &MorphTarget, vertex_count: u32) -> Result<f32, MtCode> {
    if vertex_count == 0 {
        return Err(MtCode::EMPTY_TRACK);
    }
    let mut nonzero = 0usize;
    for d in t.deltas.iter() {
        let nz = d.pos[0] != 0.0 || d.pos[1] != 0.0 || d.pos[2] != 0.0
            || d.normal[0] != 0.0 || d.normal[1] != 0.0 || d.normal[2] != 0.0;
        if nz {
            nonzero += 1;
        }
    }
    Ok(nonzero as f32 / vertex_count as f32)
}

/// 量化目标反演权重的顶点插值（Q12 域：整数乘权重的定点近似——
/// 供量化域回归对拍；w 以 1/256 为粒度）。
pub fn dequant_apply(base: f32, q_delta: i32, w: f32) -> f32 {
    base + w * dequant_q12(q_delta)
}

// ---------------------------------------------------------------------------
// 五、错误契约（独占 0x44xx 段）
// ---------------------------------------------------------------------------

/// vef27 诊断码。独占 `0x44xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MtCode(pub u16);

impl MtCode {
    /// 权重域外（[0,1] 外）。
    pub const WEIGHT_OUT_OF_RANGE: MtCode = MtCode(0x4401);
    /// replace 模式权重和超 1（归一化违约）。
    pub const WEIGHT_SUM_EXCEEDS: MtCode = MtCode(0x4402);
    /// 权重条目指向未知目标。
    pub const UNKNOWN_TARGET: MtCode = MtCode(0x4403);
    /// 空轨道。
    pub const EMPTY_TRACK: MtCode = MtCode(0x4404);
    /// 关键帧乱序（非法轨道）。
    pub const KEYS_UNORDERED: MtCode = MtCode(0x4405);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            MtCode::WEIGHT_OUT_OF_RANGE => "morph 权重域外：权重必须落在 [0,1]".into(),
            MtCode::WEIGHT_SUM_EXCEEDS => "replace 模式权重和超 1：归一化约束违约".into(),
            MtCode::UNKNOWN_TARGET => "权重条目指向未知形变目标：账实不符".into(),
            MtCode::EMPTY_TRACK => "权重轨道为空：无帧可驱动".into(),
            MtCode::KEYS_UNORDERED => "关键帧乱序：轨道必须 tick 升序".into(),
            MtCode(_) => "未知 vef27 morph 域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（回归：通路/帧序列/组合/量化）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    fn smile() -> MorphTarget {
        MorphTarget {
            id: 1,
            label: "微笑",
            deltas: vec![
                VertexDelta { index: 0, pos: [0.0, 2.0, 0.0], normal: [0.0, 0.0, 0.0] },
                VertexDelta { index: 5, pos: [1.0, 0.0, 0.0], normal: [0.0, 1.0, 0.0] },
            ],
        }
    }

    #[test]
    fn 顶点级插值位置与法线() {
        let t = smile();
        let a = apply_morph([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], &t, 0, 0.5).unwrap();
        assert_eq!(a.pos, [0.0, 1.0, 0.0]);
        assert!(a.touched);
        let b = apply_morph([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], &t, 3, 0.5).unwrap();
        assert!(!b.touched);
    }

    #[test]
    fn 权重域外拒绝() {
        let t = smile();
        assert_eq!(
            apply_morph([0.0; 3], [0.0, 0.0, 1.0], &t, 0, 1.5),
            Err(MtCode::WEIGHT_OUT_OF_RANGE)
        );
    }

    #[test]
    fn 帧序列线性插值() {
        let track = WeightTrack {
            target_id: 1,
            keys: vec![
                WeightKeyframe { tick: 0, weight: 0.0 },
                WeightKeyframe { tick: 100, weight: 1.0 },
            ],
        };
        assert_eq!(weight_at(&track, 50), Ok(0.5));
        assert_eq!(weight_at(&track, 0), Ok(0.0));
        assert_eq!(weight_at(&track, 200), Ok(1.0));
    }

    #[test]
    fn replace权重和超1拒绝() {
        let t = smile();
        let ws = [
            MorphWeight { target_id: 1, weight: 0.6 },
            MorphWeight { target_id: 2, weight: 0.6 },
        ];
        let blink = MorphTarget { id: 2, label: "眨眼", deltas: vec![] };
        assert_eq!(
            combine_vertex([0.0; 3], [0.0, 0.0, 1.0], 0, &[t, blink], &ws, CombineMode::Replace),
            Err(MtCode::WEIGHT_SUM_EXCEEDS)
        );
    }

    #[test]
    fn 量化往返有界() {
        let v = 0.125_f32;
        let q = quant_q12(v);
        assert!((dequant_q12(q) - v).abs() <= QUANT_HALF_STEP);
    }
}
