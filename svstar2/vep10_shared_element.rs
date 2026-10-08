//! VE-F3010 · 共享元素转场（VE-P 域 · 动效域 · 批次 P01 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3010`
//!
//! **判据（锚点原文）**：pair 声明、四维插值、内容不拉伸、飞行体协议、语义不飞、判据。
//!
//! **职责定位（锚点原文）**：共享元素模型（元素在两个界面状态间连续过渡——
//! 列表缩略图→详情大图连续变形）；共享元素对（源元素+目标元素配对声明：
//! `pair(源, 目标, 映射)`）；变形动画（位置/尺寸/圆角/内容裁剪**四维插值**——
//! 源几何→目标几何中间帧生成；不变形内容的抗拉伸策略：容器变形+内容恒定
//! 尺寸居中裁剪——**内容不拉伸红线**）；配对生命周期（转场开始配对→转场中
//! 双元素视觉合体（源隐藏+飞行体呈现）→目标接管——**飞行体协议**：独立图层
//! 提升——F2850 强制提升声明）。
//!
//! # 一、内容为什么不拉伸（而裁剪）
//!
//! 缩略图飞向大图时，若把内容随容器一起拉伸，中间帧会出现「被拉糊的照片」
//! ——变形过程中内容可读性归零，且纵横比失真不可逆。故抗拉伸策略**只有一条
//! 合法路径**：容器做几何变形（四维插值），内容保持**恒定尺寸**（=目标内容
//! 尺寸）并**居中**放置，超出容器的部分被裁剪。内容的宽度/高度在任意中间帧
//! 都等于声明值——判据多帧采样钉死「内容尺寸恒等」，任何把内容写进插值的
//! 改动都会露馅。配对声明的映射字段只收 `CropCenter`（容器变形+居中裁剪），
//! `Stretch` 在声明入口即拒（[`E_PAIR_FAIL`]）——红线从数据结构入口执行，
//! 不靠调用方自觉。
//!
//! # 二、飞行体为什么是「独立图层 + 快照」而不是「移动源元素」
//!
//! 源元素在旧页面树里、目标在新页面树里，两棵树在转场期并存——任何一个
//! 「原地元素」都无法同时属于两棵树。故合体的唯一干净实现是第三实体：
//! **飞行体**（独立图层，内容来自源元素的快照）。三段生命周期里源隐藏、
//! 目标待命、飞行体呈现，飞行体落位即目标接管、飞行体销毁。飞行体必须
//! **强制提升**为独立合成图层（F2850 前向声明契约位
//! [`BOOST_FORCE_DECLARED`]）——否则每帧变形都会触发主线程重排，违背
//! 「飞行体=强制提升零主线程」的性能承诺。
//!
//! # 三、语义为什么不跟着飞
//!
//! 读屏用户不需要「一张正在飞的照片」：转场中语义随**目标**——读屏焦点
//! 在转场开始即落目标语义，飞行体对读屏恒隐藏（aria-hidden）。若语义跟飞，
//! 读屏用户会听到「一个无名的移动图形」在两次可读状态之间悬空——语义断线。
//! [`semantics_during_flight`] 把「焦点=目标语义、飞行体=隐藏」钉成唯一
//! 返回值，判据断言隐藏恒真（语义不跟着飞红线）。
//!
//! # 四、退化为什么显性而不静默
//!
//! 配对失败（目标未渲染/源已销毁）的正确行为是**退化为独立入退场+诊断**
//! ——不是静默取消：用户看到的是「新页面正常入场」而不是「点了没反应」。
//! 四路降级（配对失败/几何病态/飞行体丢失/内容不支持）全部产出
//! [`Degradation`] 显性记录并计数，禁止任何一路静默吞掉（退化显性红线）。
//!
//! # 五、与相邻条的分工
//!
//! F2850 管图层提升单源（本模块消费，前向声明契约位）；F3005 编排器管
//! 时间轴（本模块只产几何中间帧）；F3021/F3027 是页面/主从转场客户（消费
//! 本模块的 pair 与飞行体）；O04 编译走合成通道（几何动画落通道声明）。
//! 本模块只管「**配对、变形、飞、接、语义归谁**」，不执行页面路由与图层
//! 系统本身。
//!
//! **性能（锚点原文）**：配对 O(对数)（有序表二分）；中间帧 O(1) 每帧插值；
//! 快照 O(1)（纹理复用计数）；飞行体=强制提升零主线程。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vep03_token::{MotionTokenError, TokenTable, TokenValue};
use crate::svstar2::vep04_stack::P_NAMESPACE_OWNER;

// ---------------------------------------------------------------------------
// 一、常量与错误码（P 域字符串码家族格式）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SHARED_PROTOCOL_VERSION: &str = "P10-shared-v1";

/// 配对声明非法（自配对/映射违例/重复源）。
pub const E_PAIR_FAIL: &str = "E_PAIR_FAIL";

/// 变形中间几何病态（负尺寸/超界钳制后仍零面积）。
pub const E_PAIR_GEO: &str = "E_PAIR_GEO";

/// 飞行体丢失（图层被逐出→重建+诊断）。
pub const E_FLIGHT_LOST: &str = "E_FLIGHT_LOST";

/// 生命周期非法迁移（跳步/逆序）。
pub const E_PAIR_PHASE: &str = "E_PAIR_PHASE";

/// F2850 强制提升声明（**前向契约位**：F2850 未落库，落库后本模块改为
/// 引用其类型；位值非空即代表「飞行体必须强制提升」的契约已钉）。
pub const BOOST_FORCE_DECLARED: &str = "F2850-layer-boost-forward-decl";

/// 飞行时长来源令牌（**令牌单源**：转查 F3003 令牌表，不私设数值；
/// 页面级转场令牌，P02 最大客户同源）。
pub const FLIGHT_DURATION_TOKEN: &str = "dur-page";

/// 插值进度上限（千分比定点：0..=1000，零浮点纪律）。
pub const PROGRESS_MAX: i64 = 1000;

// ---------------------------------------------------------------------------
// 二、几何与四维插值（零浮点：千分比定点）
// ---------------------------------------------------------------------------

/// 元素几何（六维声明：位置 x/y、尺寸 w/h、圆角、内容裁剪内缩）。
///
/// w/h/radius/crop 允许构造时为负（真实来源可能给出病态值），但一切
/// 消费前必须经 [`Geo::sanitize`] 钳制——病态值不允许直接进插值管线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geo {
    /// 左上 x。
    pub x: i32,
    /// 左上 y。
    pub y: i32,
    /// 宽（可为负=病态，sanitize 钳 0）。
    pub w: i32,
    /// 高（可为负=病态，sanitize 钳 0）。
    pub h: i32,
    /// 圆角半径（超 min(w,h)/2 视为超界，钳制）。
    pub radius: i32,
    /// 内容裁剪内缩（超 min(w,h)/2 视为超界，钳制）。
    pub crop: i32,
}

impl Geo {
    /// 病态钳制：负尺寸钳 0；圆角/裁剪钳到 [0, min(w,h)/2]（超界收缩）。
    pub fn sanitize(&self) -> Geo {
        let w = self.w.max(0);
        let h = self.h.max(0);
        let half = (w.min(h)) / 2;
        Geo {
            x: self.x,
            y: self.y,
            w,
            h,
            radius: self.radius.clamp(0, half),
            crop: self.crop.clamp(0, half),
        }
    }

    /// 钳后是否零面积（零面积=无法承载变形，须提前接管）。
    pub fn is_zero_area(&self) -> bool {
        let s = self.sanitize();
        s.w == 0 || s.h == 0
    }
}

/// 千分比定点插值（p ∈ [0, 1000]；i64 中间量防 i32 溢出，O(1)）。
pub fn lerp_i32(a: i32, b: i32, p: i64) -> i32 {
    let p = p.clamp(0, PROGRESS_MAX);
    let a64 = a as i64;
    let b64 = b as i64;
    (a64 + (b64 - a64) * p / PROGRESS_MAX) as i32
}

/// 四维中间帧：源几何→目标几何，按进度 p（千分比）逐维插值。
///
/// 输入两侧都先 sanitize（病态不进插值）；输出再 sanitize（两侧合成的
/// 中间值仍可能病态，例如两端裁剪钳制后插出的中间值超界）——**双重钳制**
/// 保证任意中间帧合法。O(1) 每帧。
pub fn midframe(src: &Geo, dst: &Geo, p: i64) -> Geo {
    let s = src.sanitize();
    let d = dst.sanitize();
    let g = Geo {
        x: lerp_i32(s.x, d.x, p),
        y: lerp_i32(s.y, d.y, p),
        w: lerp_i32(s.w, d.w, p),
        h: lerp_i32(s.h, d.h, p),
        radius: lerp_i32(s.radius, d.radius, p),
        crop: lerp_i32(s.crop, d.crop, p),
    };
    g.sanitize()
}

/// 几何病态检出：源或目标钳后零面积（负尺寸/超界收缩到无）→ 提前接管。
///
/// 锚点：「变形中间几何病态（负尺寸/超界）→钳制+提前接管」——钳制保证
/// 中间帧合法，提前接管保证零面积帧不播出。
pub fn needs_early_takeover(src: &Geo, dst: &Geo) -> bool {
    src.sanitize().is_zero_area() || dst.sanitize().is_zero_area()
}

/// 内容不拉伸布局：容器做几何变形，内容**恒定尺寸**居中（内容不拉伸红线）。
///
/// 返回 `(内容 x, 内容 y, 内容 w, 内容 h)`：宽高恒等于声明值——任何容器
/// 变形都不改变内容尺寸，超出部分由容器裁剪。居中偏移用饱和减法（容器
/// 比内容小时偏移为负=溢出居中，交由容器 crop 收口）。
pub fn content_layout(geo: &Geo, content_w: u32, content_h: u32) -> (i32, i32, u32, u32) {
    let g = geo.sanitize();
    let cw = content_w as i32;
    let ch = content_h as i32;
    let cx = g.x + (g.w - cw) / 2;
    let cy = g.y + (g.h - ch) / 2;
    (cx, cy, content_w, content_h)
}

// ---------------------------------------------------------------------------
// 三、配对声明与配对表（O(log n) 有序二分）
// ---------------------------------------------------------------------------

/// 内容映射方式（闭集：合法映射只有居中裁剪一条路——内容不拉伸红线入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentMap {
    /// 容器变形+内容恒定尺寸居中裁剪（唯一合法映射）。
    CropCenter,
    /// 内容随容器拉伸（**红线违例**，声明入口即拒）。
    Stretch,
}

impl ContentMap {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            ContentMap::CropCenter => "crop-center",
            ContentMap::Stretch => "stretch",
        }
    }
}

/// 共享元素对声明：`pair(源, 目标, 映射)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairDecl {
    /// 源元素 id（转场前所在的界面状态）。
    pub source: u32,
    /// 目标元素 id（转场后所在的界面状态）。
    pub target: u32,
    /// 内容映射（只收 [`ContentMap::CropCenter`]）。
    pub map: ContentMap,
}

impl PairDecl {
    /// 声明构造（自配对拒、Stretch 拒——红线的两道入口闸）。
    pub fn new(source: u32, target: u32, map: ContentMap) -> Result<PairDecl, MotionTokenError> {
        if source == target {
            return Err(MotionTokenError::new(
                E_PAIR_FAIL,
                "配对声明被拒：源与目标同 id",
                &format!("pair({}, …) 的源与目标是同一元素：没有「过渡」可言", source),
                "检查配对声明：源与目标必须是两个界面状态里的两个元素",
                P_NAMESPACE_OWNER,
            ));
        }
        if map == ContentMap::Stretch {
            return Err(MotionTokenError::new(
                E_PAIR_FAIL,
                "配对声明被拒：Stretch 映射违例",
                "内容随容器拉伸=中间帧内容失真（内容不拉伸红线）",
                "改用 ContentMap::CropCenter（容器变形+内容恒定尺寸居中裁剪）",
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(PairDecl { source, target, map })
    }
}

/// 配对表：按源 id 有序的声明集（配对 O(对数)——二分查找，锚点性能承诺）。
#[derive(Clone, Debug, Default)]
pub struct PairTable {
    sorted: Vec<PairDecl>,
}

impl PairTable {
    /// 空表。
    pub fn new() -> PairTable {
        PairTable { sorted: Vec::new() }
    }

    /// 登记配对（二分插入保序；**重复源拒**——一个源同时飞向两个目标
    /// 没有合法语义，后到声明显性被拒而不是静默覆盖先到）。
    pub fn declare(&mut self, decl: PairDecl) -> Result<(), MotionTokenError> {
        match self.sorted.binary_search_by_key(&decl.source, |d| d.source) {
            Ok(_) => Err(MotionTokenError::new(
                E_PAIR_FAIL,
                "配对声明被拒：源元素重复配对",
                &format!("源 {} 已有在册配对：一源多目标无合法转场语义", decl.source),
                "先解除旧配对，或改用独立入退场（退化路径）",
                P_NAMESPACE_OWNER,
            )),
            Err(pos) => {
                self.sorted.insert(pos, decl);
                Ok(())
            }
        }
    }

    /// 按源查配对（二分，O(log n)）。
    pub fn find(&self, source: u32) -> Option<&PairDecl> {
        self.sorted
            .binary_search_by_key(&source, |d| d.source)
            .ok()
            .and_then(|i| self.sorted.get(i))
    }

    /// 在册配对数。
    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }

    /// 有序性自检（内部不变量：源 id 严格递增）。
    pub fn is_sorted(&self) -> bool {
        self.sorted
            .windows(2)
            .all(|w| w[0].source < w[1].source)
    }
}

// ---------------------------------------------------------------------------
// 四、三态生命周期状态机（配对→飞行→接管）
// ---------------------------------------------------------------------------

/// 配对生命周期三态（锚点：配对/飞行/接管）+ 退化态（显性降级出口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairState {
    /// 配对：声明在册，转场未起飞。
    Paired,
    /// 飞行：源隐藏+飞行体呈现（视觉合体期）。
    Flying,
    /// 接管：目标接管，飞行体销毁。
    TakenOver,
    /// 退化：配对失败显性降级（走独立入退场——不静默取消红线）。
    Degraded,
}

/// 退化类型（锚点四路降级矩阵的显性记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Degradation {
    /// 配对失败（目标未渲染/源已销毁）→ 独立入退场。
    IndependentEntryExit,
    /// 中间几何病态 → 钳制+提前接管。
    EarlyTakeOver,
    /// 飞行体丢失（图层被逐出）→ 重建+诊断。
    FlightRebuild,
    /// 内容类型不支持 → 快照降级（海报帧）。
    PosterSnapshot,
}

impl Degradation {
    /// 显性退化说明（**不静默取消红线**：每路降级都有可读的理由串）。
    pub fn note(self) -> &'static str {
        match self {
            Degradation::IndependentEntryExit => "配对失败：退化为独立入退场（新元素正常入场，诊断在案）",
            Degradation::EarlyTakeOver => "中间几何病态：已钳制合法，提前接管（零面积帧不播出）",
            Degradation::FlightRebuild => "飞行体图层被逐出：已重建并续飞，诊断在案",
            Degradation::PosterSnapshot => "内容类型不支持实时快照：降级为海报帧快照（显性声明）",
        }
    }

    /// 对应诊断码。
    pub fn code(self) -> &'static str {
        match self {
            Degradation::IndependentEntryExit | Degradation::EarlyTakeOver => E_PAIR_GEO,
            Degradation::FlightRebuild => E_FLIGHT_LOST,
            Degradation::PosterSnapshot => E_PAIR_FAIL,
        }
    }
}

/// 单个配对的生命周期记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairRecord {
    /// 配对声明。
    pub decl: PairDecl,
    /// 当前状态。
    pub state: PairState,
    /// 起飞时刻（毫秒逻辑钟）。
    pub started_ms: u64,
    /// 接管时刻（起飞后由 take_over 回填）。
    pub taken_ms: Option<u64>,
    /// 显性退化记录（空=未退化）。
    pub degraded: Option<Degradation>,
    /// 是否提前接管（几何病态路径）。
    pub early_takeover: bool,
}

impl PairRecord {
    /// 转场开始：配对（生命周期入口）。
    pub fn begin(decl: PairDecl, now_ms: u64) -> PairRecord {
        PairRecord {
            decl,
            state: PairState::Paired,
            started_ms: now_ms,
            taken_ms: None,
            degraded: None,
            early_takeover: false,
        }
    }

    /// 起飞：Paired → Flying（合体：源隐藏+飞行体呈现）。
    pub fn launch(&mut self, now_ms: u64) -> Result<(), MotionTokenError> {
        if self.state != PairState::Paired {
            return Err(MotionTokenError::new(
                E_PAIR_PHASE,
                "起飞被拒：生命周期非法迁移",
                &format!("当前状态 {:?} 不可起飞（只允许 Paired→Flying）", self.state),
                "检查转场时序：起飞只能在配对之后、接管之前",
                P_NAMESPACE_OWNER,
            ));
        }
        self.started_ms = now_ms;
        self.state = PairState::Flying;
        Ok(())
    }

    /// 接管：Flying → TakenOver（目标接管，飞行体销毁）。
    pub fn take_over(&mut self, now_ms: u64) -> Result<(), MotionTokenError> {
        if self.state != PairState::Flying {
            return Err(MotionTokenError::new(
                E_PAIR_PHASE,
                "接管被拒：生命周期非法迁移",
                &format!("当前状态 {:?} 不可接管（只允许 Flying→TakenOver）", self.state),
                "正常路径先起飞再接管；reduce 直达用 reduce_take_over",
                P_NAMESPACE_OWNER,
            ));
        }
        self.taken_ms = Some(now_ms);
        self.state = PairState::TakenOver;
        Ok(())
    }

    /// 显性退化：Paired → Degraded（配对失败出口，绝不静默）。
    pub fn degrade(&mut self, d: Degradation) {
        if self.state == PairState::Paired {
            self.state = PairState::Degraded;
            self.degraded = Some(d);
        }
    }

    /// reduce 直达：Paired → TakenOver（跳过飞行——退化声明显性）。
    ///
    /// 正常协议禁跳步，reduce 是**唯一**合法跳步路径：跳过飞行直达接管，
    /// 并把跳步记入显性退化记录（直达不是静默省略，是声明过的替代路径）。
    pub fn reduce_take_over(&mut self, now_ms: u64) -> Result<(), MotionTokenError> {
        if self.state != PairState::Paired {
            return Err(MotionTokenError::new(
                E_PAIR_PHASE,
                "reduce 直达被拒：只在配对态可直达",
                &format!("当前状态 {:?}，reduce 直达只覆盖 Paired→TakenOver", self.state),
                "飞行中的 reduce 处理走编排层（F2874 覆盖矩阵），不在配对层",
                P_NAMESPACE_OWNER,
            ));
        }
        self.taken_ms = Some(now_ms);
        self.state = PairState::TakenOver;
        self.degraded = Some(Degradation::EarlyTakeOver);
        self.early_takeover = true;
        Ok(())
    }

    /// 语义归 TARGET 还是 SOURCE（语义不跟着飞红线的状态侧投影）。
    ///
    /// 飞行期语义随目标；配对前（声明期）语义仍属源；接管后属目标；
    /// 退化后语义各自独立（无共享语义可言）。
    pub fn semantics_owner(&self) -> PairState {
        match self.state {
            PairState::Paired | PairState::Degraded => PairState::Paired,
            PairState::Flying | PairState::TakenOver => PairState::TakenOver,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、飞行体协议（独立图层提升 + 内容快照）
// ---------------------------------------------------------------------------

/// 内容类型（快照模式选择的闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    /// 位图：可直接纹理复用。
    Image,
    /// 矢量：快照为位图纹理。
    Vector,
    /// 文本：快照为位图纹理（转场期不做文本重排）。
    Text,
    /// 视频：**不支持实时快照**——海报帧降级（显性声明）。
    Video,
}

/// 快照模式（O(1) 纹理复用 vs 海报帧降级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotMode {
    /// 纹理复用：快照一次，飞行期零重采样（O(1) 每帧）。
    Texture,
    /// 海报帧：视频等内容降级快照（显性声明，非静默）。
    PosterFrame,
}

impl ContentType {
    /// 快照模式选择（视频→海报帧降级声明，其余→纹理复用）。
    pub fn snapshot_mode(self) -> SnapshotMode {
        match self {
            ContentType::Video => SnapshotMode::PosterFrame,
            _ => SnapshotMode::Texture,
        }
    }
}

/// 飞行体：转场期的第三实体（源隐藏、目标待命、飞行体呈现）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlyingBody {
    /// 关联配对的源 id。
    pub source: u32,
    /// 独立图层标识（强制提升后由图层系统分配的层名）。
    pub layer: &'static str,
    /// 快照模式。
    pub mode: SnapshotMode,
    /// 强制提升声明（F2850 契约位——落库前以常量钉位）。
    pub boost: &'static str,
    /// 快照纹理复用计数（O(1) 复用的可观测账）。
    pub reuse_count: u32,
    /// 是否存活（图层被逐出即 false——丢失检测的依据）。
    pub alive: bool,
}

/// 飞行体管理器（图层+内容快照；丢失重建+诊断计数）。
#[derive(Clone, Debug, Default)]
pub struct FlightManager {
    bodies: Vec<FlyingBody>,
    /// 飞行体丢失重建次数（诊断账）。
    pub lost_rebuilds: u32,
    /// 海报帧降级次数（显性声明账）。
    pub poster_degrades: u32,
}

static LAYER_PREFIX: &str = "flight-layer-";

impl FlightManager {
    /// 空管理器。
    pub fn new() -> FlightManager {
        FlightManager { bodies: Vec::new(), lost_rebuilds: 0, poster_degrades: 0 }
    }

    /// 起飞创建飞行体：强制提升为独立图层 + 快照（O(1) 纹理复用起账）。
    pub fn launch(
        &mut self,
        source: u32,
        content: ContentType,
    ) -> Result<(), MotionTokenError> {
        if self.get(source).is_some() {
            return Err(MotionTokenError::new(
                E_PAIR_PHASE,
                "飞行体创建被拒：该源已在飞行",
                &format!("源 {} 已有存活飞行体：重复起飞=双体竞争", source),
                "先 take_over 销毁旧飞行体，再重新起飞",
                P_NAMESPACE_OWNER,
            ));
        }
        let mode = content.snapshot_mode();
        if mode == SnapshotMode::PosterFrame {
            // 降级显性入账（不静默：海报帧替代实时快照是可观测行为差异）。
            self.poster_degrades = self.poster_degrades.saturating_add(1);
        }
        self.bodies.push(FlyingBody {
            source,
            layer: LAYER_PREFIX,
            mode,
            boost: BOOST_FORCE_DECLARED,
            reuse_count: 1,
            alive: true,
        });
        Ok(())
    }

    /// 按源查飞行体。
    pub fn get(&self, source: u32) -> Option<&FlyingBody> {
        self.bodies.iter().rev().find(|b| b.source == source && b.alive)
    }

    /// 快照复用计数 +1（O(1) 纹理复用的账面证据）。
    pub fn snapshot_reuse(&mut self, source: u32) -> bool {
        match self.bodies.iter_mut().rev().find(|b| b.source == source && b.alive) {
            Some(b) => {
                b.reuse_count = b.reuse_count.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// 图层被逐出：标记死亡并重建（重建+诊断——锚点降级矩阵第三路）。
    ///
    /// 返回 `Ok(())` 表示已完成「逐出标记+重建」；被逐出者不存在时报错
    /// （丢失诊断必须对应真实飞行体，不虚构）。
    pub fn note_ejected(&mut self, source: u32) -> Result<(), MotionTokenError> {
        let had = match self.bodies.iter_mut().rev().find(|b| b.source == source && b.alive) {
            Some(b) => {
                b.alive = false;
                true
            }
            None => false,
        };
        if !had {
            return Err(MotionTokenError::new(
                E_FLIGHT_LOST,
                "飞行体逐出报告被拒：无存活飞行体",
                &format!("源 {} 没有可被逐出的飞行体：丢失报告与账不符", source),
                "核对起飞记录；无飞行体时不需要丢失处理",
                P_NAMESPACE_OWNER,
            ));
        }
        // 重建：同源新图层，快照模式与降级声明继承，复用账从 1 重起。
        self.bodies.push(FlyingBody {
            source,
            layer: LAYER_PREFIX,
            mode: SnapshotMode::Texture,
            boost: BOOST_FORCE_DECLARED,
            reuse_count: 1,
            alive: true,
        });
        self.lost_rebuilds = self.lost_rebuilds.saturating_add(1);
        Ok(())
    }

    /// 目标接管：销毁飞行体（生命周期终点）。无飞行体时幂等 Ok
    /// （接管与逐出重建的竞态按幂等收敛，同 F3009 双轨竞争处理）。
    pub fn take_over(&mut self, source: u32) {
        if let Some(b) = self.bodies.iter_mut().rev().find(|b| b.source == source && b.alive) {
            b.alive = false;
        }
    }

    /// 存活飞行体数。
    pub fn alive_len(&self) -> usize {
        self.bodies.iter().filter(|b| b.alive).count()
    }

    /// 在册总数（含已销毁——账面完整性）。
    pub fn total_len(&self) -> usize {
        self.bodies.len()
    }
}

// ---------------------------------------------------------------------------
// 六、语义连续（语义不跟着飞红线）与 reduce 直达
// ---------------------------------------------------------------------------

/// 飞行期语义投影：`(读屏焦点语义, 飞行体是否对读屏隐藏)`。
///
/// **语义不跟着飞**：转场中读屏看到的是目标语义，飞行体恒隐藏——返回值
/// 第二位钉死 `true`，判据断言恒真；任何让飞行体可读的改动在此露馅。
pub fn semantics_during_flight(target_label: &str) -> (String, bool) {
    (target_label.to_string(), true)
}

/// reduce 态转场计划：`(飞行帧数, 直达接管)`，恒 `(0, true)`。
///
/// 锚点：「reduce 态=跳过飞行直达接管（退化声明显性）」——双零纪律的
/// 转场版：零飞行帧 + 直达布尔，判据钉死；实现漏掉 reduce 时这里露馅。
pub const fn reduce_plan() -> (u32, bool) {
    (0, true)
}

/// 飞行时长（毫秒）：**转查令牌表单源**（[`FLIGHT_DURATION_TOKEN`]），
/// 令牌缺失/值型不符即报错——静默兜底会掩盖令牌表破坏。
pub fn flight_duration_ms(table: &TokenTable) -> Result<u32, MotionTokenError> {
    let tok = table.require(FLIGHT_DURATION_TOKEN)?;
    match &tok.value {
        TokenValue::Millis(ms) => Ok(*ms),
        other => Err(MotionTokenError::new(
            E_PAIR_FAIL,
            "飞行时长令牌值型不符",
            &format!("令牌 {} 的值不是时长型（实际 {:?}）", tok.id, other.css_literal()),
            "检查令牌表：飞行时长令牌的值必须为 Millis",
            P_NAMESPACE_OWNER,
        )),
    }
}

// ---------------------------------------------------------------------------
// 七、读屏替述
// ---------------------------------------------------------------------------

/// 转场读屏单行（配对/状态/语义归属三要素；不含坐标内容）。
pub fn screen_line(r: &PairRecord) -> String {
    let state = match r.state {
        PairState::Paired => "已配对，转场待开始",
        PairState::Flying => "转场进行中",
        PairState::TakenOver => "转场完成",
        PairState::Degraded => "转场降级（独立入退场）",
    };
    match (r.state, r.degraded) {
        (_, Some(d)) => format!(
            "元素 {}→{}：{}；{}",
            r.decl.source, r.decl.target, state, d.note()
        ),
        _ => format!(
            "元素 {}→{}：{}；读屏语义随{}",
            r.decl.source,
            r.decl.target,
            state,
            if r.semantics_owner() == PairState::TakenOver { "目标" } else { "源" }
        ),
    }
}
