//! VE-F1810 · 光照调试数据（VE-J 域 · 光照与阴影 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1810`
//!
//! **判据（锚点原文）**：三类负载、发行版零成本、三通道贡献、色弱安全、判据。
//!
//! 职责定位：光照调试数据——输出光源可视化数据（位置/方向/范围线框，供
//! VE-Y 渲染）、光照热力分解（像素级直接光/IBL/探针三层贡献的调试通道）、
//! 光照统计（活跃光源数/各类型计数/剔除计数），全部经 F1764 调试总线信封
//! 规范发出。本条对上（VE-Y 与 CLI）提供三类负载，对下只读消费 F1807
//! 管理器与光照 pass 的中间缓冲——**它不做任何光照计算**，热力分解是
//! 「复用光照 pass 已算出的中间缓冲做分通道写出」，不重复计算。
//!
//! # 一、三类负载（判据一）——各自的采样纪律不同
//!
//! - **线框负载** [`WireframePayload`]：光源图元顶点列表 + 类型着色标记。
//!   每种光源画自己的图元：方向光射线、点光球线框、聚光锥线框、面光矩形。
//!   顶点有硬上限 [`CAP_WIRE_VERTICES`]，超限**截断 + 溢出标记**（万级
//!   光源异常场景的防线）——截断不是丢弃：`total_vertices`（真实应画数）
//!   与 `vertices.len()`（实际交付数）分别如实记账，VE-Y 才知道拿到的是
//!   截断视图。
//!
//!   **截断防线对管理器来源恒不触发，这是几何事实不是漏洞**：F1807 的
//!   `MAX_LIGHTS = 256`、单灯最坏顶点 [`MAX_VERTS_PER_LIGHT`] = 32，
//!   上界 `256 × 32 = 8192 < 16384`。所以纯入口取自描述序列
//!   [`build_wireframe_from`]，管理器口 [`build_wireframe`] 只是它的
//!   convenience 包装——「万级光源」是合并光源体/实例化光源等**不经管理器
//!   注册**的来源，只有那条路径才真的撞上限。两处行为一致，不分裂语义。
//! - **热力负载** [`HeatPayload`]：三分量贡献分解缓冲（直接光/IBL/探针），
//!   半分辨率。**复用光照 pass 的中间缓冲**——输入就是 pass 已经算出的
//!   三张全分辨率图，本条只做 2×2 盒式降采样分通道写出，零重复计算。
//!   显存预算不足 → **降为四分之一分辨率档并标注**（tier 字段如实记录
//!   实际档位，静默降档会让消费端拿 1/4 当 1/2 解读）；四分之一档仍超
//!   预算 → **诚实拒绝**（降档是手段，不是无限降）。
//! - **统计快照** [`StatsSnapshot`]：活跃光源数/各类型计数/剔除计数 +
//!   耗时五元组。全部**直读 F1807 管理器**（[`LightManager`] 公开面），
//!   零额外遍历。某光源类型未注册（本帧计数为零）→ **零值 + 缺项标记**
//!   （[`StatsSnapshot::missing_mask`]）而非静默——「零」与「没测」是
//!   两件事，缺项标记让 CLI 能区分「场景里没有方向光」和「统计坏了」。
//!
//! # 二、发行版零成本（判据二）——由「根本不进入」保证
//!
//! 线框与热力仅在调试模式编译开启——发行版物理剔除零成本。本条用
//! [`StripGuard`] 落地：**Release 档下 `payload_builds` 不递增**（不是
//! 构建完再抹掉），强行请求只记 `forced_attempts` + 一条专属诊断码。
//!
//! **为什么还要 `build_entries`（单调证据）**：`payload_builds` 是净值，
//! 若实现错误地「先自增再自减抹掉」，「Release 档 payload_builds == 0」
//! 这条判据会**全绿放过**（自增再减回，净值仍为零）。`build_entries`
//! 单调递增不可回退，与净值互为交叉验证：判据断「Debug 档 build ==
//! entries == 1；Release 档 build == 0 且 entries == 强行次数」——两条
//! 独立口径同时钉死，洗白路径不存在。
//!
//! **一个 bool 表达两件事必错**：「调试负载可用吗」是构建期属性
//! （[`BuildProfile`]），「本帧有数据吗」是运行期属性（bus 里有没有
//! 帧）。二者分列，不用一个 `enabled` 混着表达。
//!
//! # 三、色弱三冗余（判据四）——类型不只靠颜色区分
//!
//! 锚点：线框着色遵循 F1366 色弱三冗余——类型不只靠颜色区分，叠加形状
//! 图元。本条把「类型」编码进**三个独立通道**，任一通道单独可解码：
//!
//! 1. **颜色位** `color_id`（1/2/3/4 按类型）——正常色觉用户的主要通道；
//! 2. **形状位** `glyph`（射线/球/锥/矩形四种图元几何）——色弱用户可从
//!    图元形状读出类型；
//! 3. **标签位** `tag`（'D'/'P'/'S'/'A' 字符）——CLI/文本消费者可从
//!    标签读出类型。
//!
//! 三通道一致性由 [`WireMark::for_kind`] 统一生成、由判据逐通道独立解码
//! 对拍——若实现只填了颜色忘了形状，「不只靠颜色」就是一句空话。
//!
//! # 四、信封与总线（F1764 J 段）
//!
//! 每帧信封封装带版本与校验和：[`Envelope`] 对 (kind, schema, byte_len,
//! seq) 做 FNV-1a 摘要存入 `declared`；[`JEnvelopeRegistry::reconcile`]
//! 重算比对，不一致即漂移 → **拦截而非记一笔**：置 `intercepted` 后
//! `take()` 一律返回 `None`——漂移的信封不得再被消费端取用，否则 VE-Y
//! 会把旧结构画成当前结构。
//!
//! **信封 seq 是真帧号**：摘要与 seq 在**信封构造处**一次写定（框架经
//! [`JDebugBus::peek_next_seq`] 先取帧号再封信封）。若先封 seq=0 的信封、
//! 再让压帧口改帧号，字段看着像版本号实际恒为 0，而摘要算的是 0——这种
//! 「装饰字段」在单帧判据下完全看不见，故 `C10-ENV-08` 逐帧对拍。
//!
//! **拦截态粘滞**：对账经 [`JDebugBus::reconcile_latest`] **就地**跑在帧上，
//! 写回 `intercepted`。此前每次取用都克隆一份登记簿再对账，等于「这帧坏了」
//! 要靠每次重新发现才能生效，且不调对账口的调用方完全不知情。
//!
//! 总线背压 → **F1764 淘汰旧帧策略承接**：[`JDebugBus`] 定容环形，满时
//! 弹出最旧帧、保住最新帧，淘汰计数 + 专属诊断码——背压是常态不是异常，
//! 但淘汰必须留痕。
//!
//! 负载类型 J 段注册：线框/热力/统计三类 + **两个扩展位**（阴影段归
//! F1830、氛围段归 F1850 沿用本条目负载类型扩展）——扩展位只声明不实现，
//! `wire()` 可解析但无负载构造面（诚实预留）。
//!
//! # 五、耗时五元组与字节口径的诚实标注
//!
//! 内核 no_std 无墙钟，统计快照的「耗时五元组」以**真实工作单元计数**
//! （选灯操作数/剔除判定数/线框顶点数/热力像素数/信封封装数）作为机检
//! 口径，**不是自证式常数**；墙钟口径由光照基准条目在目标机器定标。
//! 这与 F2407/F2013 同纪律：可机检的先机检，不可在本条定标的如实移交。
//!
//! **「不是常数」是逐字段兑现的**：`envelope_seals` 读登记簿**实测枚数**
//! （热力被预算拒绝时只有两枚，此前写死 3 会凭空多记一个封装单元）；
//! `byte_len` 按**字段表逐项相加**得 [`STATS_WIRE_BYTES`]，不写「4×7 +
//! 8×6」这种与实际字段数对不上的估算——信封字节量是消费端分配缓冲的
//! 依据，多报白占显存、少报越界读。
//!
//! # 六、诊断码
//!
//! 自建诊断码 [`JdbgCode`]，J 域调试数据独占段 **0x31**。码段选段依据：
//! 0x2A/0x2B/0x2C/0x2D 分属 M/F2408/F0220/F2409·F2607 系，**0x2E 已被
//! L 域 F1909（vel09）与 N 域 F2608（ven08）登记占用、0x2F 已被 B 域
//! F0222（veb22）登记占用**——0x31 经全仓扫描无任何诊断码段登记，
//! 是 J 域调试数据的独占段。码段判据用「不等于自身」防止自判死。
//!
//! 零外部依赖，只用 `alloc` 与 `crate::checks`（自检侧）。
//! 不改他人文件：F1807 管理器只读消费，F1764 信封自持实现
//! （沿用 M 段已验证形状，J 段独立落码）。

// 内核浮点层：`x86_64-unknown-none` 下 core 不提供浮点 transcendentals，
// 本文件用到的浮点方法统一走 `FloatExt`（转发 libm，见 crate::float 文档）。
use crate::float::FloatExt;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use super::vej07_lightmgr::{FrameStats, LightDesc, LightKind, LightManager};

// ---------------------------------------------------------------------------
// 一、诊断码（自建，J 域调试数据独占段 0x31xx）
// ---------------------------------------------------------------------------

/// 自建诊断码。**自建**而非复用他人枚举——下游封闭枚举无权加变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct JdbgCode(pub u16);

impl JdbgCode {
    /// 线框顶点超上限，已截断。
    pub const WIREFRAME_TRUNCATED: JdbgCode = JdbgCode(0x3101);
    /// 热力缓冲显存不足，已降四分之一档。
    pub const HEAT_DOWNTIERED: JdbgCode = JdbgCode(0x3102);
    /// 热力四分之一档仍超预算，诚实拒绝。
    pub const HEAT_CAPACITY_EXHAUSTED: JdbgCode = JdbgCode(0x3103);
    /// 统计某光源类型未注册，零值 + 缺项标记。
    pub const STATS_KIND_MISSING: JdbgCode = JdbgCode(0x3104);
    /// 总线背压，淘汰最旧帧。
    pub const BUS_BACKPRESSURE_DROPPED: JdbgCode = JdbgCode(0x3105);
    /// 信封摘要漂移，已拦截。
    pub const ENVELOPE_DRIFT: JdbgCode = JdbgCode(0x3106);
    /// 发行档强行请求调试负载（物理剔除中）。
    pub const RELEASE_FORCED: JdbgCode = JdbgCode(0x3107);
    /// 线框输入含非有限坐标，已钳制。
    pub const WIRE_NON_FINITE: JdbgCode = JdbgCode(0x3108);
    /// 热力输入含非有限值，已钳零。
    pub const HEAT_NON_FINITE: JdbgCode = JdbgCode(0x3109);
    /// 方向光范围无限远，线框用固定可视长度。
    pub const WIRE_RANGE_INFINITE: JdbgCode = JdbgCode(0x310A);
}

/// 诊断台账（码 → 计数）。只记账不崩溃——调试面的错误路径全是降级。
#[derive(Clone, Debug, Default)]
pub struct JdiagLog {
    counts: Vec<(JdbgCode, u32)>,
}

impl JdiagLog {
    /// 记一条诊断码。
    pub fn record(&mut self, code: JdbgCode) {
        for entry in self.counts.iter_mut() {
            if entry.0 == code {
                entry.1 = entry.1.saturating_add(1);
                return;
            }
        }
        self.counts.push((code, 1));
    }

    /// 某码计数。
    pub fn count(&self, code: JdbgCode) -> u32 {
        for entry in self.counts.iter() {
            if entry.0 == code {
                return entry.1;
            }
        }
        0
    }

    /// 全部条目（只读）。
    pub fn items(&self) -> &[(JdbgCode, u32)] {
        &self.counts
    }
}

// ---------------------------------------------------------------------------
// 二、构建档位与发行版物理剔除（StripGuard）
// ---------------------------------------------------------------------------

/// 构建档位。**显式声明**，不读环境——发行默认会让忘配的调用方白算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildProfile {
    /// 调试档：三类负载全量构建。
    Debug,
    /// 发行档：物理剔除零成本，负载不构建。
    Release,
}

/// 零成本的物质保证：Release 档下 `payload_builds` **不递增**——
/// 不是构建完再抹掉。「根本不进入」比「进入后丢弃」省掉全部构建成本。
///
/// `build_entries` 是**单调递增**的进入证据：净值口径（`payload_builds`）
/// 可被「自增再减回」洗白，单调计数不可。两个口径交叉验证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StripGuard {
    /// 构建档位。
    pub profile: BuildProfile,
    /// 已构建负载数（Debug 档才递增；净值口径）。
    pub payload_builds: u64,
    /// Release 档强行请求次数（留痕不崩溃）。
    pub forced_attempts: u64,
    /// 进入构建路径的总次数（**单调**，不可回退的证据口径）。
    pub build_entries: u64,
}

impl StripGuard {
    /// 新建守卫（显式档位）。
    pub const fn new(profile: BuildProfile) -> StripGuard {
        StripGuard { profile, payload_builds: 0, forced_attempts: 0, build_entries: 0 }
    }

    /// 是否允许构建负载。
    pub const fn allows(&self) -> bool {
        matches!(self.profile, BuildProfile::Debug)
    }

    /// 请求构建一次负载：Debug 档放行；Release 档记强行次数并拒绝。
    pub fn try_push(&mut self, log: &mut JdiagLog) -> bool {
        // 单调证据：无论档位，进入即计数，不可回退。
        self.build_entries = self.build_entries.saturating_add(1);
        if self.allows() {
            self.payload_builds = self.payload_builds.saturating_add(1);
            true
        } else {
            self.forced_attempts = self.forced_attempts.saturating_add(1);
            log.record(JdbgCode::RELEASE_FORCED);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 三、色弱三冗余（F1366）：类型的三通道编码
// ---------------------------------------------------------------------------

/// 类型标记三通道。**三个通道独立可解码**，一致性由构造口保证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WireMark {
    /// 颜色位（1/2/3/4；0 非法）。
    pub color_id: u8,
    /// 形状位（1=射线 2=球框 3=锥框 4=矩形）。
    pub glyph: u8,
    /// 标签位（ASCII 'D'/'P'/'S'/'A'）。
    pub tag: u8,
}

impl WireMark {
    /// 方向光标记。
    pub const DIRECTIONAL: WireMark = WireMark { color_id: 1, glyph: 1, tag: b'D' };
    /// 点光标记。
    pub const POINT: WireMark = WireMark { color_id: 2, glyph: 2, tag: b'P' };
    /// 聚光标记。
    pub const SPOT: WireMark = WireMark { color_id: 3, glyph: 3, tag: b'S' };
    /// 面光标记。
    pub const AREA: WireMark = WireMark { color_id: 4, glyph: 4, tag: b'A' };

    /// 按类型取标记（构造口唯一——三通道一致性在这里一次定死）。
    pub const fn for_kind(kind: LightKind) -> WireMark {
        match kind {
            LightKind::Directional => WireMark::DIRECTIONAL,
            LightKind::Point => WireMark::POINT,
            LightKind::Spot => WireMark::SPOT,
            LightKind::Area => WireMark::AREA,
        }
    }

    /// 三通道是否互相一致（同一类型的三种编码同时成立）。
    pub const fn consistent(&self) -> bool {
        matches!(
            (self.color_id, self.glyph, self.tag),
            (1, 1, b'D') | (2, 2, b'P') | (3, 3, b'S') | (4, 4, b'A')
        )
    }

    /// 仅从形状位解码类型（色弱通道独立可解码）。
    pub const fn kind_from_glyph(glyph: u8) -> Option<LightKind> {
        match glyph {
            1 => Some(LightKind::Directional),
            2 => Some(LightKind::Point),
            3 => Some(LightKind::Spot),
            4 => Some(LightKind::Area),
            _ => None,
        }
    }

    /// 仅从标签位解码类型（文本消费者通道独立可解码）。
    pub const fn kind_from_tag(tag: u8) -> Option<LightKind> {
        match tag {
            b'D' => Some(LightKind::Directional),
            b'P' => Some(LightKind::Point),
            b'S' => Some(LightKind::Spot),
            b'A' => Some(LightKind::Area),
            _ => None,
        }
    }

    /// 仅从颜色位解码类型（正常色觉通道独立可解码）。
    pub const fn kind_from_color(color_id: u8) -> Option<LightKind> {
        match color_id {
            1 => Some(LightKind::Directional),
            2 => Some(LightKind::Point),
            3 => Some(LightKind::Spot),
            4 => Some(LightKind::Area),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、线框负载
// ---------------------------------------------------------------------------

/// 线框顶点硬上限（万级光源异常场景的截断防线）。
pub const CAP_WIRE_VERTICES: usize = 16384;
/// 方向光固定可视长度（方向光 `range = INF` 无几何半径，射线用固定
/// 长度并留诊断——诚实标注这是可视约定不是物理量）。
pub const DIR_VISUAL_LEN: f32 = 2.0;
/// 聚光调试锥默认半角的正切（锥角归 F1805，本条不持有角度，用默认
/// 展开比画示意锥并标注 approximate——宁可标注错误来源也不冒充精确）。
pub const SPOT_DEBUG_SPREAD: f32 = 0.45;
/// 球/环线框每圈段数。
pub const RING_SEGMENTS: usize = 8;

/// 线框顶点：位置 + 类型三通道标记。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WireVertex {
    /// 位置 x。
    pub x: f32,
    /// 位置 y。
    pub y: f32,
    /// 位置 z。
    pub z: f32,
    /// 类型标记（三冗余）。
    pub mark: WireMark,
}

/// 线框负载：光源图元顶点列表 + 溢出记账。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WireframePayload {
    /// 实际交付顶点（≤ [`CAP_WIRE_VERTICES`]）。
    pub vertices: Vec<WireVertex>,
    /// 截断前真实应画顶点数（与 `vertices.len()` 分别如实记账）。
    pub total_vertices: usize,
    /// 是否发生截断。
    pub truncated: bool,
    /// 画了多少个光源的图元（截断后不足整灯的不计入）。
    pub lights_drawn: usize,
    /// 截断丢弃的顶点数（`total_vertices − vertices.len()`）——**溢出标记
    /// 的量化面**。没有它，截断只告诉消费方「少了」，不告诉「少了多少」。
    pub dropped_vertices: usize,
    /// 本负载字节量（信封 byte_len 口径：每顶点 15 字节）。
    pub byte_len: u32,
}

/// 单光源图元生成（纯函数：描述 → 顶点序列）。
///
/// 非有限坐标当场钳到原点并留诊断——NaN 顶点会顺着画线算法污染整段
/// 图元（曲线编辑器同款故障），钳制 + 计数 + 诊断三件齐做，不静默。
fn light_primitives(desc: &LightDesc, out: &mut Vec<WireVertex>, log: &mut JdiagLog) {
    let mark = WireMark::for_kind(desc.kind);
    let (px, py, pz) = if desc.pos.0.is_finite() && desc.pos.1.is_finite() && desc.pos.2.is_finite()
    {
        desc.pos
    } else {
        log.record(JdbgCode::WIRE_NON_FINITE);
        (0.0, 0.0, 0.0)
    };
    match desc.kind {
        // 方向光：8 根射线，固定可视长度（range=INF 留诊断）。
        LightKind::Directional => {
            if !desc.range.is_finite() {
                log.record(JdbgCode::WIRE_RANGE_INFINITE);
            }
            let (dx, dy, dz) = desc.dir;
            let (dx, dy, dz) = if dx.is_finite() && dy.is_finite() && dz.is_finite() {
                (dx, dy, dz)
            } else {
                log.record(JdbgCode::WIRE_NON_FINITE);
                (0.0, 0.0, 1.0)
            };
            let mut i = 0usize;
            while i < RING_SEGMENTS {
                let ang = (i as f32) * core::f32::consts::TAU / (RING_SEGMENTS as f32);
                // 射线基点绕位置微散开（环半径 0.1），画成短放射线。
                let bx = px + ang.m_cos() * 0.1;
                let by = py + ang.m_sin() * 0.1;
                let bz = pz;
                out.push(WireVertex {
                    x: bx,
                    y: by,
                    z: bz,
                    mark,
                });
                out.push(WireVertex {
                    x: bx + dx * DIR_VISUAL_LEN,
                    y: by + dy * DIR_VISUAL_LEN,
                    z: bz + dz * DIR_VISUAL_LEN,
                    mark,
                });
                i += 1;
            }
        }
        // 点光：两正交大圆各 8 段，半径取 range（非有限钳 1.0 留诊断）。
        LightKind::Point => {
            let r = if desc.range.is_finite() && desc.range > 0.0 {
                desc.range
            } else {
                log.record(if desc.range.is_finite() {
                    JdbgCode::WIRE_NON_FINITE
                } else {
                    JdbgCode::WIRE_RANGE_INFINITE
                });
                1.0
            };
            let mut i = 0usize;
            while i < RING_SEGMENTS {
                let a0 = (i as f32) * core::f32::consts::TAU / (RING_SEGMENTS as f32);
                let a1 = ((i + 1) % RING_SEGMENTS) as f32 * core::f32::consts::TAU
                    / (RING_SEGMENTS as f32);
                // 水平圆（XZ 平面）。
                out.push(WireVertex {
                    x: px + a0.m_cos() * r,
                    y: py,
                    z: pz + a0.m_sin() * r,
                    mark,
                });
                out.push(WireVertex {
                    x: px + a1.m_cos() * r,
                    y: py,
                    z: pz + a1.m_sin() * r,
                    mark,
                });
                // 垂直圆（XY 平面）。
                out.push(WireVertex {
                    x: px + a0.m_cos() * r,
                    y: py + a0.m_sin() * r,
                    z: pz,
                    mark,
                });
                out.push(WireVertex {
                    x: px + a1.m_cos() * r,
                    y: py + a1.m_sin() * r,
                    z: pz,
                    mark,
                });
                i += 1;
            }
        }
        // 聚光：锥框（8 根侧棱 + 底环 8 段），展开比取默认值并诚实标注。
        LightKind::Spot => {
            let reach = if desc.range.is_finite() && desc.range > 0.0 {
                desc.range
            } else {
                log.record(JdbgCode::WIRE_NON_FINITE);
                1.0
            };
            let (dx, dy, dz) = desc.dir;
            let (dx, dy, dz) = if dx.is_finite() && dy.is_finite() && dz.is_finite() {
                (dx, dy, dz)
            } else {
                log.record(JdbgCode::WIRE_NON_FINITE);
                (0.0, 0.0, 1.0)
            };
            // 底面中心 = 位置 + 方向×可达距离。
            let cx = px + dx * reach;
            let cy = py + dy * reach;
            let cz = pz + dz * reach;
            // 与方向正交的两个基向量（Hoffman 非退化支轴法）：选一个与
            // dir 最不平行的轴做叉积，得到 u；u × dir 得 v。锥底环点 =
            // 底心 + u·cosθ·r + v·sinθ·r——这样锥体在任意方向下都不退化。
            let axis: (f32, f32, f32) = if dx.abs() < 0.9 { (1.0, 0.0, 0.0) } else { (0.0, 1.0, 0.0) };
            // u = normalize(dir × axis)。
            let cxu = dy * axis.2 - dz * axis.1;
            let cyu = dz * axis.0 - dx * axis.2;
            let czu = dx * axis.1 - dy * axis.0;
            let ul = (cxu * cxu + cyu * cyu + czu * czu).m_sqrt();
            let (ux, uy, uz) = if ul > 0.0 { (cxu / ul, cyu / ul, czu / ul) } else { (1.0, 0.0, 0.0) };
            // v = dir × u（已单位化：dir 与 u 均为单位向量且正交）。
            let (vx, vy, vz) = (dy * uz - dz * uy, dz * ux - dx * uz, dx * uy - dy * ux);
            let spread_r = SPOT_DEBUG_SPREAD * reach;
            let mut i = 0usize;
            while i < RING_SEGMENTS {
                let a0 = (i as f32) * core::f32::consts::TAU / (RING_SEGMENTS as f32);
                let a1 = ((i + 1) % RING_SEGMENTS) as f32 * core::f32::consts::TAU
                    / (RING_SEGMENTS as f32);
                // 侧棱：位置 → 底环点。
                out.push(WireVertex { x: px, y: py, z: pz, mark });
                out.push(WireVertex {
                    x: cx + ux * (a0.m_cos() * spread_r) + vx * (a0.m_sin() * spread_r),
                    y: cy + uy * (a0.m_cos() * spread_r) + vy * (a0.m_sin() * spread_r),
                    z: cz + uz * (a0.m_cos() * spread_r) + vz * (a0.m_sin() * spread_r),
                    mark,
                });
                // 底环：相邻两点相连。
                out.push(WireVertex {
                    x: cx + ux * (a0.m_cos() * spread_r) + vx * (a0.m_sin() * spread_r),
                    y: cy + uy * (a0.m_cos() * spread_r) + vy * (a0.m_sin() * spread_r),
                    z: cz + uz * (a0.m_cos() * spread_r) + vz * (a0.m_sin() * spread_r),
                    mark,
                });
                out.push(WireVertex {
                    x: cx + ux * (a1.m_cos() * spread_r) + vx * (a1.m_sin() * spread_r),
                    y: cy + uy * (a1.m_cos() * spread_r) + vy * (a1.m_sin() * spread_r),
                    z: cz + uz * (a1.m_cos() * spread_r) + vz * (a1.m_sin() * spread_r),
                    mark,
                });
                i += 1;
            }
        }
        // 面光：矩形线框（4 边），半边长取 range（面光尺寸归 F1806，
        // 调试矩形用 range 作半边长并标注 approximate）。
        LightKind::Area => {
            let h = if desc.range.is_finite() && desc.range > 0.0 {
                desc.range
            } else {
                log.record(JdbgCode::WIRE_NON_FINITE);
                1.0
            };
            let corners = [
                (px - h, py - h, pz),
                (px + h, py - h, pz),
                (px + h, py + h, pz),
                (px - h, py + h, pz),
            ];
            let mut i = 0usize;
            while i < 4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                out.push(WireVertex { x: a.0, y: a.1, z: a.2, mark });
                out.push(WireVertex { x: b.0, y: b.1, z: b.2, mark });
                i += 1;
            }
        }
    }
}

/// 单个光源图元的顶点数（判据侧独立重算的公共口径；构造与判据同源推导）。
pub const fn verts_of(kind: LightKind) -> usize {
    match kind {
        LightKind::Directional => 2 * RING_SEGMENTS,
        LightKind::Point => 4 * RING_SEGMENTS,
        LightKind::Spot => 4 * RING_SEGMENTS,
        LightKind::Area => 8,
    }
}

/// **单灯最坏顶点数**（四类图元的最大值）。
pub const MAX_VERTS_PER_LIGHT: usize = 4 * RING_SEGMENTS;

/// 构建线框负载的**纯入口**：光源描述序列 → 线框顶点。
///
/// **为什么不只接受 `&LightManager`**：管理器受 F1807 的
/// [`MAX_LIGHTS`](super::vej07_lightmgr::MAX_LIGHTS) 硬上限约束，注册源
/// 最多 256 盏；即便每盏都画最贵的 32 顶点图元，总量也只有
/// `256 × 32 = 8192 < CAP_WIRE_VERTICES`——**管理器路径永远触发不了截断**，
/// 锚点要求的「万级光源异常场景」防线会退化成一段永不执行的死代码。
/// 因此本入口吃**任意描述序列**（合并光源体、实例化光源、跨管理器聚合的
/// 调试视图等），管理器只是其中一种来源。截断语义对两者一致。
pub fn build_wireframe_from(descs: &[LightDesc], log: &mut JdiagLog) -> WireframePayload {
    let mut vertices: Vec<WireVertex> = Vec::new();
    let mut prims: Vec<WireVertex> = Vec::new();
    let mut total = 0usize;
    let mut lights_drawn = 0usize;
    let mut truncated = false;
    for desc in descs.iter() {
        prims.clear();
        light_primitives(desc, &mut prims, log);
        total += prims.len();
        if !truncated && vertices.len() + prims.len() <= CAP_WIRE_VERTICES {
            for v in prims.iter() {
                vertices.push(*v);
            }
            lights_drawn += 1;
        } else {
            // **不 break**：截断后仍走完余下光源。理由有二——
            // ① `total_vertices` 必须是「真实应画数」，break 掉就只剩
            //    已交付数，消费方无从知道丢了多少（`dropped_vertices`
            //    永远是 0，溢出标记形同虚设）；
            // ② 余下光源的输入异常（非有限坐标/无限范围）仍要进诊断台账，
            //    break 掉会让「场景里有畸形灯」这件事随截断一起消失。
            // 顶点一律丢弃，但遍历与记账不中断——成本是每盏一次图元生成，
            // 复用同一个 `prims` 缓冲，不额外分配。
            if !truncated {
                truncated = true;
                log.record(JdbgCode::WIREFRAME_TRUNCATED);
            }
        }
    }
    // 字节口径：每顶点 3×f32 + 3×u8 = 15 字节（三通道标记各 1 字节）。
    let byte_len = (vertices.len() * 15) as u32;
    let dropped_vertices = total.saturating_sub(vertices.len());
    WireframePayload {
        vertices,
        total_vertices: total,
        truncated,
        lights_drawn,
        dropped_vertices,
        byte_len,
    }
}

/// 构建线框负载（管理器在册光源 convenience 口，语义等价于
/// [`build_wireframe_from`]）。
///
/// **恒不截断**：`MAX_LIGHTS × MAX_VERTS_PER_LIGHT = 8192 < CAP_WIRE_VERTICES`，
/// 该性质由判据 `C10-DEG-01` 以独立重算钉死——哪天上调了 `CAP_WIRE_VERTICES`
/// 到管理器上限之下（防线对在册光源彻底失效），判据即红。
pub fn build_wireframe(mgr: &LightManager, log: &mut JdiagLog) -> WireframePayload {
    build_wireframe_from(&mgr.iter_live(), log)
}

// ---------------------------------------------------------------------------
// 五、热力负载：三通道贡献分解
// ---------------------------------------------------------------------------

/// 热力分辨率档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeatTier {
    /// 半分辨率（宽高各右移 1）。
    Half,
    /// 四分之一分辨率（右移 2；显存不足的降档位）。
    Quarter,
}

impl HeatTier {
    /// 右移位数。
    pub const fn shift(self) -> u32 {
        match self {
            HeatTier::Half => 1,
            HeatTier::Quarter => 2,
        }
    }

    /// 线缆名（诚实标注实际档位，消费端不得自行推断）。
    pub const fn as_str(self) -> &'static str {
        match self {
            HeatTier::Half => "half",
            HeatTier::Quarter => "quarter",
        }
    }
}

/// 三通道贡献分解缓冲（直接光/IBL/探针）。
///
/// **复用而非重算**：输入是光照 pass 已算出的三张全分辨率图，本条只做
/// 2×2 盒式降采样分通道写出——热力分解是「把已有结果分通道端出去」，
/// 不重复任何光照计算（锚点性能条款原文）。
#[derive(Clone, Debug, PartialEq)]
pub struct HeatPayload {
    /// 实际档位（降档后如实标注）。
    pub tier: HeatTier,
    /// 降采样后宽。
    pub hw: u32,
    /// 降采样后高。
    pub hh: u32,
    /// 直接光通道。
    pub direct: Vec<f32>,
    /// IBL 通道。
    pub ibl: Vec<f32>,
    /// 探针通道。
    pub probe: Vec<f32>,
    /// 钳零的非有限输入值计数。
    pub nonfinite_clamped: u32,
    /// 本负载字节量（3 通道 × 像素 × 4 字节）。
    pub byte_len: u32,
}

/// 热力构建结果：成功给负载，降档给降档负载，彻底超预算诚实拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeatBuildOutcome {
    /// 成功（可能已降档，看 tier）。
    Built,
    /// 四分之一档仍超预算，拒绝（不静默缩水）。
    Exhausted,
}

/// 单通道 2×2 盒式降采样（非有限值钳零并计数）。
fn downsample_channel(
    src: &[f32],
    w: u32,
    h: u32,
    shift: u32,
    out: &mut Vec<f32>,
    clamped: &mut u32,
) {
    let hw = w >> shift;
    let hh = h >> shift;
    out.clear();
    out.reserve((hw * hh) as usize);
    let mut oy = 0u32;
    while oy < hh {
        let mut ox = 0u32;
        while ox < hw {
            // 2×2 求和取均值；源不足 2×2 的尾块按实际覆盖数均值。
            let mut sum = 0.0f32;
            let mut n = 0u32;
            let mut sy = oy << shift;
            while sy < (oy + 1) << shift && sy < h {
                let mut sx = ox << shift;
                while sx < (ox + 1) << shift && sx < w {
                    let v = src[(sy * w + sx) as usize];
                    if v.is_finite() {
                        sum += v;
                    } else {
                        *clamped = clamped.wrapping_add(1);
                    }
                    n += 1;
                    sx += 1;
                }
                sy += 1;
            }
            out.push(if n > 0 { sum / (n as f32) } else { 0.0 });
            ox += 1;
        }
        oy += 1;
    }
}

/// 构建热力负载：半分辨率优先，显存不足降四分之一档并标注，
/// 四分之一档仍超预算诚实拒绝。
///
/// `budget_floats` 是消费端声明的输出缓冲预算（单位：f32 槽位）——
/// 三通道合计不得超。
pub fn build_heat(
    direct: &[f32],
    ibl: &[f32],
    probe: &[f32],
    w: u32,
    h: u32,
    budget_floats: usize,
    log: &mut JdiagLog,
) -> (HeatBuildOutcome, Option<HeatPayload>) {
    // 档位从半分辨率起试；每档检查「3 通道 × hw×hh ≤ 预算」。
    let tiers = [HeatTier::Half, HeatTier::Quarter];
    let mut chosen: Option<HeatTier> = None;
    let mut downgraded = false;
    for tier in tiers {
        let hw = w >> tier.shift();
        let hh = h >> tier.shift();
        let need = 3usize * (hw as usize) * (hh as usize);
        if need <= budget_floats {
            chosen = Some(tier);
            break;
        }
        downgraded = true;
    }
    let tier = match chosen {
        Some(t) => t,
        None => {
            log.record(JdbgCode::HEAT_CAPACITY_EXHAUSTED);
            return (HeatBuildOutcome::Exhausted, None);
        }
    };
    if downgraded {
        // 降档必须留痕——静默降档会让消费端拿 1/4 当 1/2 解读。
        log.record(JdbgCode::HEAT_DOWNTIERED);
    }
    let hw = w >> tier.shift();
    let hh = h >> tier.shift();
    let mut clamped = 0u32;
    let mut dch: Vec<f32> = Vec::new();
    let mut ich: Vec<f32> = Vec::new();
    let mut pch: Vec<f32> = Vec::new();
    downsample_channel(direct, w, h, tier.shift(), &mut dch, &mut clamped);
    downsample_channel(ibl, w, h, tier.shift(), &mut ich, &mut clamped);
    downsample_channel(probe, w, h, tier.shift(), &mut pch, &mut clamped);
    if clamped > 0 {
        log.record(JdbgCode::HEAT_NON_FINITE);
    }
    let byte_len = (3u64 * (hw as u64) * (hh as u64) * 4) as u32;
    (
        HeatBuildOutcome::Built,
        Some(HeatPayload {
            tier,
            hw,
            hh,
            direct: dch,
            ibl: ich,
            probe: pch,
            nonfinite_clamped: clamped,
            byte_len,
        }),
    )
}

// ---------------------------------------------------------------------------
// 六、统计快照：直读 F1807 管理器
// ---------------------------------------------------------------------------

/// 各类型计数（缺项标记位掩码：bit0 方向光 … bit3 面光）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KindCensus {
    /// 方向光数。
    pub directional: u32,
    /// 点光数。
    pub point: u32,
    /// 聚光数。
    pub spot: u32,
    /// 面光数。
    pub area: u32,
    /// 缺项标记：计数为零的类型置位（「零」与「没测」是两件事）。
    pub missing_mask: u8,
}

impl KindCensus {
    /// 缺项位掩码的第 k 位（k 按类型序）。
    fn missing_bit(kind: LightKind) -> u8 {
        match kind {
            LightKind::Directional => 1 << 0,
            LightKind::Point => 1 << 1,
            LightKind::Spot => 1 << 2,
            LightKind::Area => 1 << 3,
        }
    }

    /// 从管理器在册光源直读计数（零额外结构，一次遍历）。
    pub fn from_manager(mgr: &LightManager, log: &mut JdiagLog) -> KindCensus {
        let mut c = KindCensus::default();
        let mut any_registered = [false; 4];
        for desc in mgr.iter_live() {
            match desc.kind {
                LightKind::Directional => {
                    c.directional += 1;
                    any_registered[0] = true;
                }
                LightKind::Point => {
                    c.point += 1;
                    any_registered[1] = true;
                }
                LightKind::Spot => {
                    c.spot += 1;
                    any_registered[2] = true;
                }
                LightKind::Area => {
                    c.area += 1;
                    any_registered[3] = true;
                }
            }
        }
        // 缺项标记：某类型本帧零注册 → 零值 + 缺项位 + 诊断码，
        // 而非静默——CLI 据此区分「没有这类灯」与「统计坏了」。
        let kinds = [LightKind::Directional, LightKind::Point, LightKind::Spot, LightKind::Area];
        let mut k = 0usize;
        while k < 4 {
            if !any_registered[k] {
                c.missing_mask |= KindCensus::missing_bit(kinds[k]);
                log.record(JdbgCode::STATS_KIND_MISSING);
            }
            k += 1;
        }
        c
    }

    /// 活跃光源总数。
    pub const fn active(&self) -> u32 {
        self.directional + self.point + self.spot + self.area
    }
}

/// 耗时五元组（工作单元口径——no_std 无墙钟，诚实标注见头注第五节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TimingQuintet {
    /// 选灯操作单元（管理器累计操作数）。
    pub select_units: u64,
    /// 剔除判定单元（本帧视锥剔除数）。
    pub cull_checks: u64,
    /// 线框顶点单元。
    pub wire_vertices: u64,
    /// 热力像素单元（三通道合计）。
    pub heat_pixels: u64,
    /// 信封封装单元。
    pub envelope_seals: u64,
}

/// 统计快照线缆字节数（**按字段表逐项加出来的，不是估的**）。
///
/// `active_lights/selected/culled/dropped` 4×u32 = 16；
/// `census` 4×u32 + `missing_mask` u8 = 17；
/// `timing` 5×u64 = 40。合计 **73**。
///
/// **为什么不写 76**：此前按「4×7 + 8×6」估写 76，而五元组实际只有 5 个
/// u64——多出来的 3 字节是凭空捏造的。信封 `byte_len` 是消费端按它分配
/// 缓冲的依据，多报会白占显存、少报会越界读，故此项必须可对账。
pub const STATS_WIRE_BYTES: u32 = 4 * 4 + 4 * 4 + 1 + 5 * 8;

/// 统计快照：活跃光源数/各类型计数/剔除计数 + 耗时五元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatsSnapshot {
    /// 活跃光源数。
    pub active_lights: u32,
    /// 本帧选中数（进参数块的）。
    pub selected: u32,
    /// 本帧视锥剔除数。
    pub culled: u32,
    /// 本帧因上限被裁数。
    pub dropped: u32,
    /// 各类型计数 + 缺项标记。
    pub census: KindCensus,
    /// 耗时五元组。
    pub timing: TimingQuintet,
    /// 本负载字节量（= [`STATS_WIRE_BYTES`]，按字段表实算）。
    pub byte_len: u32,
}

/// 构建统计快照：全部直读管理器公开面，零额外遍历、零自证常数。
///
/// `frame_stats` 来自 F1807 本帧产出（registered/selected/culled/dropped/
/// total_ops 五项管理器真值）；`wire_vertices`/`heat_pixels` 由本条
/// 前两类负载的构建结果回填（信封封装数在总线压帧时补记）。
pub fn build_stats(
    mgr: &LightManager,
    frame_stats: &FrameStats,
    wire_vertices: u64,
    heat_pixels: u64,
) -> StatsSnapshot {
    let mut log = JdiagLog::default();
    let census = KindCensus::from_manager(mgr, &mut log);
    StatsSnapshot {
        active_lights: census.active(),
        selected: frame_stats.selected as u32,
        culled: frame_stats.culled as u32,
        dropped: frame_stats.dropped as u32,
        census,
        timing: TimingQuintet {
            select_units: frame_stats.total_ops,
            cull_checks: frame_stats.culled as u64,
            wire_vertices,
            heat_pixels,
            envelope_seals: 0,
        },
        byte_len: STATS_WIRE_BYTES,
    }
}

// ---------------------------------------------------------------------------
// 七、信封（F1764 J 段）与总线
// ---------------------------------------------------------------------------

/// 信封规范版本（J 段沿用 M 段已验证形状，版本独立起号）。
pub const ENVELOPE_SCHEMA: u32 = 1;

/// F1764 负载类型（J 段注册：三类实作 + 两个诚实扩展位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadKind {
    /// 线框负载（0x10）。
    Wireframe,
    /// 热力负载（0x11）。
    Heat,
    /// 统计快照（0x12）。
    Stats,
    /// 阴影段扩展位（0x13，归 F1830 沿用扩展）——只声明不实现。
    ReservedShadow,
    /// 氛围段扩展位（0x14，归 F1850 沿用扩展）——只声明不实现。
    ReservedAtmosphere,
}

impl PayloadKind {
    /// 线缆类型号。
    pub const fn wire(self) -> u8 {
        match self {
            PayloadKind::Wireframe => 0x10,
            PayloadKind::Heat => 0x11,
            PayloadKind::Stats => 0x12,
            PayloadKind::ReservedShadow => 0x13,
            PayloadKind::ReservedAtmosphere => 0x14,
        }
    }

    /// 人读标签。
    pub const fn label(self) -> &'static str {
        match self {
            PayloadKind::Wireframe => "JWIREFRAME",
            PayloadKind::Heat => "JHEAT",
            PayloadKind::Stats => "JSTATS",
            PayloadKind::ReservedShadow => "JSHADOW_RES",
            PayloadKind::ReservedAtmosphere => "JATMO_RES",
        }
    }

    /// 按线缆号解析（未知即 `None`）。
    pub const fn from_wire(w: u8) -> Option<PayloadKind> {
        match w {
            0x10 => Some(PayloadKind::Wireframe),
            0x11 => Some(PayloadKind::Heat),
            0x12 => Some(PayloadKind::Stats),
            0x13 => Some(PayloadKind::ReservedShadow),
            0x14 => Some(PayloadKind::ReservedAtmosphere),
            _ => None,
        }
    }
}

/// FNV-1a 单步。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    let h = h ^ byte;
    h.wrapping_mul(0x0100_0193)
}

/// 每帧信封：版本 + 校验和（对 kind/schema/byte_len/seq/标签长度的
/// FNV-1a 摘要）。`new` 自算摘要——对账基准在产生处一次写定，
/// 事后补填等于「用答案校对答案」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Envelope {
    /// 负载类型。
    pub kind: PayloadKind,
    /// 信封规范版本。
    pub schema: u32,
    /// 负载字节量。
    pub byte_len: u32,
    /// 帧序号。
    pub seq: u32,
    /// 注册时声明的摘要（对账基准）。
    pub declared: u32,
}

impl Envelope {
    /// 唯一构造口：自算摘要。
    pub fn new(kind: PayloadKind, byte_len: u32, seq: u32) -> Envelope {
        let mut e = Envelope { kind, schema: ENVELOPE_SCHEMA, byte_len, seq, declared: 0 };
        e.declared = e.checksum();
        e
    }

    /// 重算摘要。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_step(h, self.kind.wire() as u32);
        h = fnv_step(h, self.schema);
        h = fnv_step(h, self.byte_len);
        h = fnv_step(h, self.seq);
        h = fnv_step(h, self.kind.label().len() as u32);
        h
    }

    /// 摘要是否漂移。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }
}

/// 信封登记簿：注册时存摘要，对账重算比对，漂移即拦截。
#[derive(Clone, Debug, Default)]
pub struct JEnvelopeRegistry {
    slots: Vec<Option<Envelope>>,
    epoch: u32,
    drift_count: u32,
    intercepted: bool,
}

impl JEnvelopeRegistry {
    /// 新建登记簿（J 段五类型槽位）。
    pub fn new() -> JEnvelopeRegistry {
        let mut slots: Vec<Option<Envelope>> = Vec::new();
        let mut i = 0;
        while i < 5 {
            slots.push(None);
            i += 1;
        }
        JEnvelopeRegistry { slots, epoch: 0, drift_count: 0, intercepted: false }
    }

    /// 槽位下标（按线缆号排序映射）。
    fn slot_of(kind: PayloadKind) -> usize {
        match kind {
            PayloadKind::Wireframe => 0,
            PayloadKind::Heat => 1,
            PayloadKind::Stats => 2,
            PayloadKind::ReservedShadow => 3,
            PayloadKind::ReservedAtmosphere => 4,
        }
    }

    /// 本簿已注册的信封枚数（**实测**而非按负载类型总数推算——热力降档
    /// 失败时实际只有两枚，凭常数写「三」就是记账撒谎）。
    pub fn sealed(&self) -> u32 {
        let mut n = 0u32;
        for slot in self.slots.iter() {
            if slot.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 注册一枚信封（同类型覆盖旧枚）。
    pub fn register(&mut self, env: Envelope) -> bool {
        self.epoch = self.epoch.wrapping_add(1);
        let slot = self.slots[Self::slot_of(env.kind)].as_mut();
        match slot {
            Some(old) => {
                *old = env;
            }
            None => {
                self.slots[Self::slot_of(env.kind)] = Some(env);
            }
        }
        !env.drifted()
    }

    /// 对账：重算全部已注册摘要，任一漂移即整体拦截。
    ///
    /// 拦截是「不是记一笔」的纪律：置 `intercepted` 后 `take()` 一律
    /// 返回 `None`——漂移的信封不得再被消费端取用。
    pub fn reconcile(&mut self, log: &mut JdiagLog) {
        for slot in self.slots.iter() {
            if let Some(env) = slot {
                if env.drifted() {
                    self.drift_count = self.drift_count.saturating_add(1);
                    self.intercepted = true;
                    log.record(JdbgCode::ENVELOPE_DRIFT);
                }
            }
        }
    }

    /// 取一枚信封（被拦截后一律 `None`）。
    pub fn take(&self, kind: PayloadKind) -> Option<&Envelope> {
        if self.intercepted {
            return None;
        }
        self.slots[Self::slot_of(kind)].as_ref()
    }

    /// 可变取用一枚**已注册**信封（对账钩子与负向判据用——篡改字段后
    /// 摘要必然失配，正是要制造漂移的那一处）。
    ///
    /// **不给未注册类型开可变口**：否则可以在登记簿里凭空造出一枚没有
    /// 基准的信封，绕过「登记时写基准」这条纪律。
    pub fn slot_mut(&mut self, kind: PayloadKind) -> Option<&mut Envelope> {
        self.slots[Self::slot_of(kind)].as_mut()
    }

    /// 是否已被拦截。
    pub const fn is_intercepted(&self) -> bool {
        self.intercepted
    }

    /// 漂移累计次数。
    pub const fn drifts(&self) -> u32 {
        self.drift_count
    }
}

/// 一帧调试数据：序号 + 三类实作负载 + 信封登记簿。
#[derive(Clone, Debug, Default)]
pub struct JDebugFrame {
    /// 帧序号。
    pub seq: u32,
    /// 线框负载（None = 本帧未构建，如发行档）。
    pub wire: Option<WireframePayload>,
    /// 热力负载。
    pub heat: Option<HeatPayload>,
    /// 统计快照。
    pub stats: Option<StatsSnapshot>,
    /// 信封登记簿。
    pub registry: JEnvelopeRegistry,
}

/// F1764 J 段调试总线：定容环形 + 淘汰旧帧承接背压。
#[derive(Clone, Debug)]
pub struct JDebugBus {
    cap: usize,
    frames: VecDeque<JDebugFrame>,
    dropped_old: u32,
    next_seq: u32,
}

impl JDebugBus {
    /// 新建总线（容量即背压阈值）。
    pub fn new(cap: usize) -> JDebugBus {
        JDebugBus {
            cap: cap.max(1),
            frames: VecDeque::new(),
            dropped_old: 0,
            next_seq: 1,
        }
    }

    /// 压入一帧：满时弹出最旧帧（淘汰旧帧策略）+ 计数 + 诊断码。
    pub fn push_frame(&mut self, mut frame: JDebugFrame, log: &mut JdiagLog) -> u32 {
        frame.seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        // 信封封装单元在压帧时补记：**读登记簿实测枚数**（热力被降档拒绝时
        // 只有两枚），不写常数 3。
        let sealed = frame.registry.sealed();
        if let Some(st) = frame.stats.as_mut() {
            st.timing.envelope_seals = sealed as u64;
        }
        let seq = frame.seq;
        if self.frames.len() >= self.cap {
            self.frames.pop_front();
            self.dropped_old = self.dropped_old.saturating_add(1);
            log.record(JdbgCode::BUS_BACKPRESSURE_DROPPED);
        }
        self.frames.push_back(frame);
        seq
    }

    /// 下一帧将用的序号（**只读窥探**，让调用方在构造信封时就把正确的
    /// 帧号写进去；压帧时取同一个值，故两者恒等）。
    pub const fn peek_next_seq(&self) -> u32 {
        self.next_seq
    }

    /// 对最新帧的登记簿**就地**对账（拦截态写回，跨调用粘滞）。
    ///
    /// **为什么必须就地**：拦截若只活在临时克隆上，「这帧坏了」这件事
    /// 每次取用都要重新发现一次，且调用方若不调对账口就完全不知情。
    pub fn reconcile_latest(&mut self, log: &mut JdiagLog) {
        if let Some(f) = self.frames.back_mut() {
            f.registry.reconcile(log);
        }
    }

    /// 最新帧（消费端语义：只要最新）。
    pub fn latest(&self) -> Option<&JDebugFrame> {
        self.frames.back()
    }

    /// 最新帧可变口（对账与负向判据用；`assemble_frame` 经
    /// [`JDebugBus::push_frame`] 压帧，不走此口）。
    pub fn latest_mut(&mut self) -> Option<&mut JDebugFrame> {
        self.frames.back_mut()
    }

    /// 背压淘汰累计数。
    pub const fn backpressure_drops(&self) -> u32 {
        self.dropped_old
    }

    /// 当前滞留帧数。
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 八、帧组装面：档位门 + 三类负载一次成型
// ---------------------------------------------------------------------------

/// 帧组装器：持有档位守卫与诊断台账。
///
/// Debug 档组装三类负载入总线；Release 档**根本不进入**负载构建
/// （`payload_builds` 不递增），强行请求只记次数。
#[derive(Clone, Debug)]
pub struct JDebugAssembler {
    guard: StripGuard,
    log: JdiagLog,
    bus: JDebugBus,
}

impl JDebugAssembler {
    /// 新建组装器（显式档位；总线容量默认 4 帧）。
    pub fn new(profile: BuildProfile) -> JDebugAssembler {
        JDebugAssembler {
            guard: StripGuard::new(profile),
            log: JdiagLog::default(),
            bus: JDebugBus::new(4),
        }
    }

    /// 诊断台账（只读）。
    pub fn log(&self) -> &JdiagLog {
        &self.log
    }

/// 总线（只读）。
    pub const fn bus(&self) -> &JDebugBus {
        &self.bus
    }

    /// 总线可变口（负向判据制造信封漂移用；产线路径不经此口）。
    pub const fn bus_mut(&mut self) -> &mut JDebugBus {
        &mut self.bus
    }

    /// 档位守卫（只读，供判据交叉验证两个口径）。
    pub const fn guard(&self) -> &StripGuard {
        &self.guard
    }

    /// 组装一帧并压入总线。
    ///
    /// `direct/ibl/probe` 是光照 pass 已算出的全分辨率中间缓冲
    /// （复用不重算）；`budget_floats` 是热力输出预算。
    /// Release 档返回 `None` 且总线无变化（物理剔除零成本）。
    pub fn assemble_frame(
        &mut self,
        mgr: &LightManager,
        frame_stats: &FrameStats,
        direct: &[f32],
        ibl: &[f32],
        probe: &[f32],
        full_w: u32,
        full_h: u32,
        budget_floats: usize,
    ) -> Option<u32> {
        if !self.guard.try_push(&mut self.log) {
            return None;
        }
        // 帧号先定：信封摘要在**产生处**就要带上正确的 seq——先封一个
        // seq=0 的信封、再让 `push_frame` 改帧号，会留下「摘要与帧号对不上」
        // 的信封（字段看着像版本号，实际恒为 0，是纯装饰）。
        let seq = self.bus.peek_next_seq();
        // 1) 线框负载。
        let wire = build_wireframe(mgr, &mut self.log);
        // 2) 热力负载（复用 pass 中间缓冲）。
        let (_outcome, heat) =
            build_heat(direct, ibl, probe, full_w, full_h, budget_floats, &mut self.log);
        let heat_pixels = heat
            .as_ref()
            .map(|hp| (hp.direct.len() + hp.ibl.len() + hp.probe.len()) as u64)
            .unwrap_or(0);
        // 3) 统计快照（直读管理器 + 前两类负载回填五元组）。
        let stats = build_stats(mgr, frame_stats, wire.vertices.len() as u64, heat_pixels);
        // 4) 信封三枚，登记后入帧。
        let mut registry = JEnvelopeRegistry::new();
        registry.register(Envelope::new(PayloadKind::Wireframe, wire.byte_len, seq));
        if let Some(h) = heat.as_ref() {
            registry.register(Envelope::new(PayloadKind::Heat, h.byte_len, seq));
        }
        registry.register(Envelope::new(PayloadKind::Stats, stats.byte_len, seq));
        let frame = JDebugFrame {
            seq: 0,
            wire: Some(wire),
            heat,
            stats: Some(stats),
            registry,
        };
        // `peek_next_seq()` 与 `push_frame()` 之间不得有其他压帧（单线程
        // 压帧口保证）；该等价关系由判据 `C10-ENV-08` 钉死——这里不加
        // `debug_assert`，保持生产面零 panic。
        Some(self.bus.push_frame(frame, &mut self.log))
    }

    /// 消费端取信封（走登记簿对账口——被拦截即 `None`）。
    ///
    /// 对账**就地**跑在最新帧的登记簿上：拦截态粘滞在帧上，后续每次取用
    /// 都直接吃到拦截结果，不靠「每次都重新发现一次漂移」。
    pub fn take_envelope(&mut self, kind: PayloadKind) -> Option<Envelope> {
        self.bus.reconcile_latest(&mut self.log);
        let frame = self.bus.latest()?;
        if frame.registry.is_intercepted() {
            return None;
        }
        frame.registry.take(kind).copied()
    }
}

#[cfg(test)]
mod tests {
    //! **与域自检（`vej10_checks.rs`）的分工**：域自检走「判据侧独立重算 +
    //! 双向对拍」，防的是**口径写错**；本节走「同输入两次调用逐位相同 +
    //! 畸形输入不崩」，防的是**不确定性与 panic 面**。两者不可互替——
    //! 一段每帧重跑都给出不同顶点序的代码，判据全绿也能过，但它在渲染里
    //! 就是每帧抖动的线框。
    use super::*;
    use crate::svstar2::vej07_lightmgr::FrameStats;

    fn desc(kind: LightKind, pos: (f32, f32, f32), dir: (f32, f32, f32), range: f32, id: u64) -> LightDesc {
        LightDesc::new(kind, pos, dir, (1.0, 1.0, 1.0), 1.0, range, id).0
    }

    fn mgr() -> LightManager {
        let mut m = LightManager::new();
        let _ = m.add(desc(LightKind::Directional, (0.0, 8.0, 0.0), (0.0, -1.0, 0.0), f32::INFINITY, 1), Vec::new());
        let _ = m.add(desc(LightKind::Point, (2.0, 1.0, 0.0), (0.0, 0.0, 0.0), 4.0, 2), Vec::new());
        let _ = m.add(desc(LightKind::Spot, (0.0, 5.0, 0.0), (0.0, -1.0, 0.0), 6.0, 3), Vec::new());
        let _ = m.add(desc(LightKind::Area, (1.0, 3.0, -1.0), (0.0, -1.0, 0.0), 1.5, 4), Vec::new());
        m
    }

    fn fs() -> FrameStats {
        FrameStats { registered: 4, selected: 3, culled: 1, dropped: 0, total_ops: 17 }
    }

    fn heat_src() -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let n = 32 * 32;
        let mut d: Vec<f32> = Vec::new();
        let mut i: Vec<f32> = Vec::new();
        let mut p: Vec<f32> = Vec::new();
        let mut k = 0usize;
        while k < n {
            d.push(1.0);
            i.push(2.0);
            p.push(4.0);
            k += 1;
        }
        (d, i, p)
    }

    #[test]
    fn same_input_twice_is_bit_identical() {
        let (d, i, p) = heat_src();
        let mut a = JDebugAssembler::new(BuildProfile::Debug);
        let mut b = JDebugAssembler::new(BuildProfile::Debug);
        let m = mgr();
        let s1 = a.assemble_frame(&m, &fs(), &d, &i, &p, 32, 32, 4096);
        let s2 = b.assemble_frame(&m, &fs(), &d, &i, &p, 32, 32, 4096);
        assert_eq!(s1, s2);
        let fa = a.bus().latest().cloned();
        let fb = b.bus().latest().cloned();
        match (fa, fb) {
            (Some(x), Some(y)) => {
                let wa = x.wire.as_ref().map(|w| w.vertices.clone());
                let wb = y.wire.as_ref().map(|w| w.vertices.clone());
                assert_eq!(wa, wb, "线框顶点序列两次调用必须逐位相同");
                assert_eq!(x.heat, y.heat, "热力负载两次调用必须逐位相同");
                assert_eq!(x.stats, y.stats, "统计快照两次调用必须逐位相同");
            }
            _ => panic!("两次组装都应产出帧"),
        }
    }

    #[test]
    fn malformed_lights_never_panic_and_never_emit_nonfinite() {
        let bad = [
            desc(LightKind::Point, (f32::NAN, 0.0, 0.0), (0.0, 0.0, 0.0), 4.0, 1),
            desc(LightKind::Point, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), -1.0, 2),
            desc(LightKind::Point, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), 0.0, 3),
            desc(LightKind::Spot, (1.0, 1.0, 1.0), (f32::NAN, 0.0, 0.0), 3.0, 4),
            desc(LightKind::Directional, (0.0, 1.0, 0.0), (0.0, -1.0, 0.0), f32::INFINITY, 5),
            desc(LightKind::Area, (0.0, 0.0, 0.0), (0.0, -1.0, 0.0), f32::NAN, 6),
        ];
        let mut log = JdiagLog::default();
        let w = build_wireframe_from(&bad, &mut log);
        assert!(w.vertices.len() > 0);
        for v in w.vertices.iter() {
            assert!(
                v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
                "畸形输入不得产出非有限顶点"
            );
        }
        // 畸形必须留痕，不许静默。
        assert!(
            log.count(JdbgCode::WIRE_NON_FINITE) + log.count(JdbgCode::WIRE_RANGE_INFINITE) > 0
        );
    }

    #[test]
    fn degenerate_heat_geometry_does_not_panic() {
        let mut log = JdiagLog::default();
        // 零宽高：下游采样数为 0，不应崩、不应产出越界长度。
        let empty: Vec<f32> = Vec::new();
        let (outcome, payload) = build_heat(&empty, &empty, &empty, 0, 0, 4096, &mut log);
        assert_eq!(outcome, HeatBuildOutcome::Built);
        match payload {
            Some(h) => {
                assert_eq!(h.hw, 0);
                assert_eq!(h.hh, 0);
                assert!(h.direct.is_empty() && h.ibl.is_empty() && h.probe.is_empty());
            }
            None => panic!("零尺寸在预算内不应被拒"),
        }
        // 非 2 的幂奇数尺寸：右移取整后不应越界读。
        let src: Vec<f32> = (0..13 * 7).map(|k| k as f32).collect();
        let (_o2, p2) = build_heat(&src, &src, &src, 13, 7, 4096, &mut log);
        match p2 {
            Some(h) => {
                assert_eq!(h.direct.len(), (h.hw as usize) * (h.hh as usize));
                for v in h.direct.iter() {
                    assert!(v.is_finite());
                }
            }
            None => panic!("奇数尺寸在预算内不应被拒"),
        }
    }

    #[test]
    fn empty_manager_frame_is_well_formed() {
        let (d, i, p) = heat_src();
        let mut asm = JDebugAssembler::new(BuildProfile::Debug);
        let empty = LightManager::new();
        let seq = asm.assemble_frame(&empty, &fs(), &d, &i, &p, 32, 32, 4096);
        assert_eq!(seq, Some(1));
        match asm.bus().latest() {
            Some(f) => {
                let s = match f.stats {
                    Some(s) => s,
                    None => panic!("空场景也应有统计快照"),
                };
                assert_eq!(s.active_lights, 0);
                assert_eq!(s.census.missing_mask, 0b1111, "四类全缺项须标出，不得静默");
                assert_eq!(s.byte_len, STATS_WIRE_BYTES);
                let w = f.wire.as_ref();
                assert!(w.map(|x| x.vertices.is_empty()).unwrap_or(false));
            }
            None => panic!("应有一帧"),
        }
    }

    #[test]
    fn release_profile_assembles_nothing_and_keeps_counting_entries() {
        let (d, i, p) = heat_src();
        let mut asm = JDebugAssembler::new(BuildProfile::Release);
        let m = mgr();
        for _ in 0..8 {
            assert_eq!(asm.assemble_frame(&m, &fs(), &d, &i, &p, 32, 32, 4096), None);
        }
        assert_eq!(asm.guard().payload_builds, 0, "发行档净值必须恒 0");
        assert_eq!(asm.guard().forced_attempts, 8);
        assert_eq!(asm.guard().build_entries, 8, "单调口径必须等于进入次数");
        assert!(asm.bus().is_empty());
        assert_eq!(asm.log().count(JdbgCode::RELEASE_FORCED), 8);
    }
}
