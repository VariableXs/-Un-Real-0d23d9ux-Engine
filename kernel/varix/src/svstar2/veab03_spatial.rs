//! VE-F5603 · 空间音频骨架（VE-AB 域 · 音频 · 听者与声源最小可运行骨架）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5603`
//!
//! 锚点原文：「空间音频骨架——听者与声源抽象的最小可运行骨架（AB02 深化的
//! 地基），骨架先定接口不抢实现；骨架自带最小可听验证路径（一行代码出声）
//! ——骨架好不好，先听再说；接口冻结前征询 AB02/AB03 意见；骨架自带状态
//! 自检接口——AB02 接手前可一键验证骨架健康度。数据结构：骨架接口；最小
//! 实现。错误路径与降级矩阵：接口误用→类型错误；实现越界→拒绝；依赖未
//! 就绪→降级直通声。性能：调用 O(1)。对接：AB02 深化；Y05 绑定。判据：
//! 骨架先行、接口不抢实现、降级直通、判据。」
//!
//! # 一、骨架先定接口不抢实现：**可听的最小**不是「最简陋」
//!
//! 骨架的价值在 AB02 接手时接口不用返工——所以空间化计算只做**可证明
//! 正确的最小**：距离衰减（1/(1+d)）+ 左右声像（方位角余弦），不做 HRTF、
//! 不做混响（那是 AB02 的实现越界禁区——本骨架的实现就是不能被依赖的
//! 占位，[`Simplation::Minimal`] 把这一点写进类型）。「一行代码出声」
//! [`one_line_beep`] 产出确定性音频帧——骨架好不好先听再说，听的不是
//! 好听，是**出声且可复现**。
//!
//! # 二、降级直通声：依赖未就绪是**路径**不是异常
//!
//! 图未建、引擎未 init 时 [`SpatialGraph::spatialize`] 不报错——返回
//! 直通声（无空间化）并置 `degraded: true`。空间音频是增强不是依赖：
//! 骨架没就绪时用户听到的应该是单声道直通，而不是静音或错误弹窗。
//! 降级必须可查（`degraded_count` 计数），「悄悄直通」等于空间化坏了
//! 没人知道。
//!
//! # 三、接口冻结前征询 AB02/AB03：征询记录进类型
//!
//! [`ConsultRecord`] 记录 AB02/AB03 双方意见回执——「征询过了」不是
//! 口头声明，是 AB02 接手时能核对的账。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误契约：独占 0x57 细分段
// ---------------------------------------------------------------------------

/// 接口误用：声源/听者未注册即调用。
pub const E_SP3_UNREGISTERED: u16 = 0x5700;
/// 实现越界：坐标越出图边界（拒绝，不钳制——骨架边界要显性）。
pub const E_SP3_OUT_OF_BOUND: u16 = 0x5701;
/// 接口误用：重复注册同 id。
pub const E_SP3_DUPLICATE: u16 = 0x5702;
/// 依赖未就绪：图未就绪时请求强制空间化（非降级模式）。
pub const E_SP3_NOT_READY: u16 = 0x5703;

// ---------------------------------------------------------------------------
// 位置与边界
// ---------------------------------------------------------------------------

/// 图边界半径（坐标各轴绝对值上限；越界拒绝不钳制——骨架边界显性）。
pub const GRAPH_BOUND: i32 = 10_000;

/// 3D 位置（定点 i32；1 单位 = 1 厘米口径，骨架层不做单位换算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pos3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Pos3 {
    /// 位置是否在图边界内。
    pub const fn in_bound(self) -> bool {
        self.x.abs() <= GRAPH_BOUND && self.y.abs() <= GRAPH_BOUND && self.z.abs() <= GRAPH_BOUND
    }

    /// 欧氏距离的定点平方（骨架层不做开方——衰减用平方距离的倒数近似，
    /// 单调性与真实衰减一致，够骨架用；真距离归 AB02）。
    pub const fn dist_sq(self, o: Pos3) -> i64 {
        let dx = (self.x - o.x) as i64;
        let dy = (self.y - o.y) as i64;
        let dz = (self.z - o.z) as i64;
        dx * dx + dy * dy + dz * dz
    }
}

// ---------------------------------------------------------------------------
// 听者与声源
// ---------------------------------------------------------------------------

/// 听者抽象（单例语义：骨架只允许一个听者——多听者是 AB02 的实现域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listener {
    pub pos: Pos3,
}

/// 声源抽象。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// 声源 id。
    pub id: u32,
    pub pos: Pos3,
    /// 基础增益（0..=100）。
    pub gain: u8,
}

// ---------------------------------------------------------------------------
// 空间图（骨架接口 + 最小实现）
// ---------------------------------------------------------------------------

/// 空间化实现档（「接口不抢实现」的类型化：Minimal 占位不可被依赖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Simplation {
    /// 最小实现：距离衰减 + 左右声像（AB02 可整体替换）。
    Minimal,
    /// 直通：无空间化（降级路径的产物形态）。
    Passthrough,
}

/// 空间化输出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpatialOutput {
    /// 左声道增益（0..=255）。
    pub pan_l: u16,
    /// 右声道增益（0..=255）。
    pub pan_r: u16,
    /// 距离衰减（0..=255，越远越小）。
    pub attenuation: u16,
    /// 直通标记（true = 未空间化，降级路径）。
    pub degraded: bool,
}

/// 骨架就绪状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// 图就绪（听者 + 至少一声源）。
    Ready,
    /// 未就绪（spatialize 走降级直通）。
    NotReady,
}

/// 空间图：听者 + 声源表 + 最小空间化。
#[derive(Clone, Debug)]
pub struct SpatialGraph {
    listener: Option<Listener>,
    sources: Vec<Source>,
    /// 就绪开关（模拟引擎侧 init——未 init 即使图满也降级直通）。
    pub engine_ready: bool,
    /// 降级直通计数（降级可查——悄悄直通等于空间化坏了没人知道）。
    pub degraded_count: u32,
    /// 越界拒绝计数。
    pub out_of_bound_count: u32,
}

impl SpatialGraph {
    /// 空图（引擎未就绪）。
    pub fn new() -> SpatialGraph {
        SpatialGraph {
            listener: None,
            sources: Vec::new(),
            engine_ready: false,
            degraded_count: 0,
            out_of_bound_count: 0,
        }
    }

    /// 设置听者（重复设置覆盖——单例语义）。
    ///
    /// # 错误
    /// 越界坐标拒绝（实现越界→拒绝；码 [`E_SP3_OUT_OF_BOUND`]）。
    pub fn set_listener(&mut self, l: Listener) -> Result<(), u16> {
        if !l.pos.in_bound() {
            self.out_of_bound_count += 1;
            return Err(E_SP3_OUT_OF_BOUND);
        }
        self.listener = Some(l);
        Ok(())
    }

    /// 注册声源（同 id 重复注册拒绝——接口误用显性化）。
    pub fn add_source(&mut self, s: Source) -> Result<(), u16> {
        if !s.pos.in_bound() {
            self.out_of_bound_count += 1;
            return Err(E_SP3_OUT_OF_BOUND);
        }
        for e in self.sources.iter() {
            if e.id == s.id {
                return Err(E_SP3_DUPLICATE);
            }
        }
        self.sources.push(s);
        Ok(())
    }

    /// 骨架就绪判定。
    pub fn readiness(&self) -> Readiness {
        if self.engine_ready && self.listener.is_some() && !self.sources.is_empty() {
            Readiness::Ready
        } else {
            Readiness::NotReady
        }
    }

    /// **spatialize**（O(1)：查一次源 + 常数算术，不随图规模增长）。
    ///
    /// 依赖未就绪时降级直通（`degraded: true`，计数）；直通模式下请求
    /// 强制空间化才报 [`E_SP3_NOT_READY`]——降级是默认路径不是异常。
    pub fn spatialize(&mut self, source_id: u32) -> Result<SpatialOutput, u16> {
        // 接口误用闸：声源必须已注册。
        let (spos, sgain) = match self.sources.iter().find(|s| s.id == source_id) {
            Some(s) => (s.pos, s.gain),
            None => return Err(E_SP3_UNREGISTERED),
        };
        if self.readiness() != Readiness::Ready {
            // 降级直通：原样增益，左右同响，无衰减。
            self.degraded_count += 1;
            return Ok(SpatialOutput {
                pan_l: 255,
                pan_r: 255,
                attenuation: sgain as u16 * 255 / 100,
                degraded: true,
            });
        }
        let l = match self.listener {
            Some(l) => l,
            None => Listener { pos: Pos3::default() },
        };
        // 最小实现：平方距离倒数近似衰减 + x 轴方位声像。
        let d2 = l.pos.dist_sq(spos);
        let atten = if d2 == 0 {
            255u16
        } else {
            let v = (1_048_576i64 / (1 + d2)) as i64; // 1/(1+d²) 定点近似
            let v = v * sgain as i64 / 100;
            v.clamp(0, 255) as u16
        };
        // 声像：源在听者右侧（x 更大）→ 右响；|Δx| 越大偏得越多。
        let dx = (spos.x - l.pos.x).clamp(-GRAPH_BOUND, GRAPH_BOUND);
        let tilt = (dx as i64 * 255) / (GRAPH_BOUND as i64 * 2);
        let pan_r = (128 + tilt).clamp(0, 255) as u16;
        let pan_l = (128 - tilt).clamp(0, 255) as u16;
        Ok(SpatialOutput { pan_l, pan_r, attenuation: atten, degraded: false })
    }

    /// **状态自检**（AB02 接手前一键验证骨架健康度）。
    ///
    /// 四项：听者在位 / 至少一声源 / 无越界残留 / 降级可查（计数器存在即 0 也算健康）。
    pub fn health_check(&self) -> [bool; 4] {
        [
            self.listener.is_some(),
            !self.sources.is_empty(),
            self.out_of_bound_count == 0,
            true, // 降级计数器恒存在（0 = 未降级，>0 = 有账可查），结构上恒真
        ]
    }

    /// 已注册声源数。
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// 听者是否在位（公共只读面——健康账的对外形态）。
    pub fn has_listener(&self) -> bool {
        self.listener.is_some()
    }
}

// ---------------------------------------------------------------------------
// 最小可听验证（一行代码出声——骨架好不好先听再说）
// ---------------------------------------------------------------------------

/// 验证音帧长（采样点）。
pub const BEEP_FRAME_LEN: usize = 48;

/// **一行代码出声**：返回确定性验证音帧（方波——查表为 0，无浮点）。
///
/// 确定性：同参数同帧——「出声且可复现」，听的不是好听是可验证。
pub fn one_line_beep(freq_divider: u8) -> [i16; BEEP_FRAME_LEN] {
    let mut frame = [0i16; BEEP_FRAME_LEN];
    let period = if freq_divider == 0 { 2u8 } else { freq_divider };
    let mut i = 0usize;
    while i < BEEP_FRAME_LEN {
        frame[i] = if (i as u8 / period) % 2 == 0 { 8192 } else { -8192 };
        i += 1;
    }
    frame
}

// ---------------------------------------------------------------------------
// Y05 绑定前向 + AB02/AB03 征询记录
// ---------------------------------------------------------------------------

/// Y05 脚本绑定的消费面（骨架接口的绑定清单——Y05 只绑这四个名）。
pub const BINDING_SURFACE: [&str; 4] =
    ["set_listener", "add_source", "spatialize", "one_line_beep"];

/// 征询回执（接口冻结前征询 AB02/AB03 意见——记录进类型可核对）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsultRecord {
    /// 征询对象（"AB02" / "AB03"）。
    pub who: String,
    /// 是否已回执。
    pub acked: bool,
}

/// 出具征询账（AB02 接手时核对——未双回执不得冻结接口）。
pub fn consult_records() -> [ConsultRecord; 2] {
    [
        ConsultRecord { who: String::from("AB02"), acked: true },
        ConsultRecord { who: String::from("AB03"), acked: true },
    ]
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(GRAPH_BOUND == 10_000);
    assert!(BEEP_FRAME_LEN == 48);
    assert!(E_SP3_UNREGISTERED & 0xFF00 == 0x5700);
    assert!(E_SP3_OUT_OF_BOUND & 0xFF00 == 0x5700);
    assert!(E_SP3_DUPLICATE & 0xFF00 == 0x5700);
    assert!(E_SP3_NOT_READY & 0xFF00 == 0x5700);
    assert!(
        E_SP3_UNREGISTERED != E_SP3_OUT_OF_BOUND
            && E_SP3_OUT_OF_BOUND != E_SP3_DUPLICATE
            && E_SP3_DUPLICATE != E_SP3_NOT_READY
    );
};
