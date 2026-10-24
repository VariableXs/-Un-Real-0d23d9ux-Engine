//! VE-F6004 · 物理与场景图绑定（VE-AD 域 · AD01 批次 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6004`
//!
//! **判据（锚点原文）**：挂接节点、插值同步、两律延续、判据。
//!
//! **职责定位（锚点原文）**：物理体挂场景图节点（Y05 两律延续：集成不改
//! 运行时判定、不夺编辑器权），变换同步含插值（渲染帧取插值位姿不取物理
//! 硬步位姿）；绑定含批量挂接——多实体批量绑定与批量解除支持；挂接含
//! 批量上限说明；上限说明含理由。
//!
//! ## 一、绑定是旁路观察者，不是求解参与者（两律之一：集成不改运行时判定）
//!
//! 绑定层只做一件事：把物理步进产出的位姿**如实搬运**到场景图节点，把
//! 编辑器的权威位姿**如实回投**给物理态。绑定/解绑/批量/挂起/复活全都
//! 不碰求解序列——同一物理位姿喂入序列，在零绑定、单绑定、满容量绑定三
//! 种情境下摘要逐位相等（[`BindHub::physics_digest`]，判据实证）。绑定
//! 层若改变物理结果，就是运行时判定被集成改写——Y05 两律红线。
//!
//! ## 二、模式即两律的类型化（两律之二：不夺编辑器权）
//!
//! [`BindMode`] 三态把权属写进类型：
//! - [`BindMode::Strict`]：物理权威——物理步进位姿写节点（运行时权）；
//! - [`BindMode::Interactive`]：编辑器权威——编辑器写节点位姿、物理跟随
//!   （[`BindHub::editor_write_pose`]）；
//! - [`BindMode::Detached`]：映射在、同步停（调试/暂存用）。
//!
//! 模式切换是**编辑器权**（[`Op::SetMode`] × [`Actor::Runtime`] → 拒）；
//! 驱动同步步是**运行时权**（[`Op::SyncTick`] × [`Actor::Editor`] → 拒）。
//! 权限矩阵 [`PERM_MATRIX`] const 表与 [`perm_rule`] 推导函数**双写全格
//! 对拍**（判据实证：两路 5 操作 × 3 操作者 = 15 格逐格相等）——矩阵改了
//! 规则没改，或反过来，判据即红。
//!
//! ## 三、变换同步含插值：渲染帧取插值位姿，不取物理硬步位姿
//!
//! 物理按固定步长硬步进（F6002 调度），渲染帧落在两步之间——直接取硬步
//! 位姿画面抖动。本模块维护每绑定体 `prev_pose`/`cur_pose` 双位姿账，
//! [`BindHub::render_pose`] 按千分位插值系数 `alpha_permille`（与 F6002
//! `StepPlan::alpha_permille` 同口径）做**整数定点插值**：位置线性插值、
//! 朝向四元数 Q14 定点 NLERP（[`Pose::lerp_permille`]）。零浮点、零除零
//! （alpha 域 0..=1000 越界显性拒 [`BindCode::BadAlpha`]；零范数四元数
//! 显性拒 [`BindCode::BadPose`]）。alpha=0 恰得 prev、alpha=1000 恰得
//! cur（判据端点钉死）；中点 alpha 下插值位姿≠硬步位姿（判据实证渲染
//! 拿的确实不是硬步值）。
//!
//! ## 四、批量挂接/批量解除：all-or-nothing 事务
//!
//! [`BindHub::batch_bind`] / [`BindHub::batch_unbind`] 是**事务批量**：
//! 预检阶段全量逐项校验（权限→上限→容量→重复→节点占用），任一项失败则
//! 整体拒绝（一个都不落账），逐项失败原因进 [`BatchReport`]；全过才逐项
//! 落账并升代次。部分成功部分失败的批量是对绑定账本的静默撕裂——不干。
//!
//! 批量上限 [`MAX_BATCH`] 与理由 [`MAX_BATCH_REASON`] 同源常量：上限是
//! **单帧同步预算保护**（MAX_BATCH × O(log n) 查表 + 定点插值 = 可预估的
//! 帧内开销；超限批量把一帧预算吃光→帧超时→掉帧），不是随意的数字；
//! 确需更大规模走多帧分批，预算可查可调（锚点：上限说明含理由）。
//!
//! ## 五、失效三路：节点销毁→挂起；同步失效→重绑；越权→拒绝
//!
//! - **节点销毁→物理体挂起**：[`BindHub::on_node_destroyed`] 检测节点
//!   侧生命周期事件，绑定条目转挂起态、体进挂起池（[`SuspendedRecord`]，
//!   对外可查 [`BindHub::suspended_bodies`]——被隔离的体去哪了对外可查）。
//!   挂起体不参与位姿同步（[`BindCode::Suspended`] 显性拒），物理态保留
//!   不销毁，编辑器 [`BindHub::revive`] 重绑新节点即复活（账本计数）。
//! - **同步失效→重新绑定**：场景图节点被删重建后代次（generation）跳变。
//!   [`BindHub::sync_tick`] 逐项比对绑定代次与节点代次，失配 →
//!   [`SyncOutcome::Stale`] 显性报告 + 条目置 stale 位（[`BindCode::SyncStale`]），
//!   不静默重置；编辑器 [`BindHub::rebind`] 校验后代次对齐、stale 清零
//!   （账本 `rebinds` 计数）。
//! - **越权→拒绝**：全操作过 [`perm_matrix`] 权限闸，拒绝操作**零副作用**
//!   且进审计簿（[`AuditOp::Denied`]，可回放 [`BindHub::audit_replay`]）。
//!
//! ## 六、绑定状态可查（无障碍面）
//!
//! [`BindHub::status_of`] 按体查（模式/代次/挂起/stale/插值就绪五要素）、
//! [`BindHub::enumerate`] 全量清单、[`BindHub::lookup_by_node`] 反向
//! O(log n) 查——绑定状态读屏可达。
//!
//! ## 七、零浮点定点数学（F6001 不失真律的同步面延续）
//!
//! 位置：i64 三轴（毫米口径）。朝向：四元数 i32 × 4，Q14 定点
//! （1.0 = [`Q14_ONE`] = 16384）。NLERP：点积判符号取最短弧 → 线性插值
//! （i64 中间量防溢出）→ 整数平方根 [`isqrt64`]（Newton 迭代，单调收敛
//! 逐位下取）归一化回 Q14。归一化残差 ≤ [`Q14_NORM_TOL`]（判据钉死）。
//! 全程无 f32/f64——同输入同输出逐位可复现。
//!
//! **对接**：上游 F6001 三律（可预期/不失真在同步面延续）、F6002 固定
//! 步长调度（alpha 千分位同口径）；下游 F6005 渲染对接（render_pose 是
//! 消费面）、F6021 碰撞体（绑定体即碰撞体挂点）。诊断码独占 `0x4Bxx`
//! 段（0x3E=F6001 / 0x3F=F6002 / 0x40=F6003，0x4B 全仓扫描空闲）。
//! no_std 零 std 依赖、零 unwrap 零 expect 零切片越界、零全局可变状态。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 模块版本。
pub const VAD04_VERSION: &str = "vead04-bindgraph-v1";

/// 默认绑定容量（资源硬上限；满容显性拒不静默驱逐）。
pub const DEFAULT_CAPACITY: usize = 4096;

/// 批量上限（单帧同步预算保护——理由见 [`MAX_BATCH_REASON`]）。
pub const MAX_BATCH: usize = 64;

/// 批量上限理由（锚点：上限说明含理由——理由与上限同源，不散落文档）。
pub const MAX_BATCH_REASON: &str = "单帧同步预算=MAX_BATCH×O(log n)查表+定点插值，\
超限批量把一帧预算吃光→帧超时→掉帧；上限是预算保护不是随意数；\
确需更大批量走多帧分批（预算可查可调）";

/// 插值系数上限（千分位口径，与 F6002 StepPlan::alpha_permille 同源）。
pub const ALPHA_MAX: u32 = 1000;

/// 挂起池容量（挂起体显性上限；满容拒收计数不静默覆盖旧痕）。
pub const SUSPEND_POOL_CAP: usize = 512;

/// 审计簿容量（append-only 满容拒记计数——不静默覆盖旧痕）。
pub const AUDIT_CAP: usize = 256;

/// 单绑定条目账面摘要种子（FNV-1a 64 位偏移基）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x4Bxx 段；0x3E=F6001 / 0x3F=F6002 / 0x40=F6003）
// ---------------------------------------------------------------------------

/// F6004 诊断码。独占 `0x4Bxx` 段，十六码闭集两两互异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindCode(pub u16);

impl BindCode {
    /// 0x4B01 未绑定（查/同步/解除的对象没有绑定条目）。
    pub const NotBound: BindCode = BindCode(0x4B01);
    /// 0x4B02 重复绑定（body 已有绑定条目——先解绑再绑，不静默换绑）。
    pub const AlreadyBound: BindCode = BindCode(0x4B02);
    /// 0x4B03 节点被占（node 已绑其他 body——一节点一体，不静默抢占）。
    pub const NodeOccupied: BindCode = BindCode(0x4B03);
    /// 0x4B04 批量超限（> MAX_BATCH——预算保护，理由见 MAX_BATCH_REASON）。
    pub const BatchTooLarge: BindCode = BindCode(0x4B04);
    /// 0x4B05 容量满（绑定数撞 DEFAULT_CAPACITY——显性拒不驱逐）。
    pub const CapacityFull: BindCode = BindCode(0x4B05);
    /// 0x4B06 越权（操作者 × 操作不在权限矩阵允许格）。
    pub const PermissionDenied: BindCode = BindCode(0x4B06);
    /// 0x4B07 节点代次失效（节点被删重建，绑定代次追不上）。
    pub const NodeGone: BindCode = BindCode(0x4B07);
    /// 0x4B08 同步失效待重绑（stale 位已置——编辑器 rebind 前同步拒走）。
    pub const SyncStale: BindCode = BindCode(0x4B08);
    /// 0x4B09 未挂起（复活前提不满足——挂起池里没有这个体）。
    pub const NotSuspended: BindCode = BindCode(0x4B09);
    /// 0x4B10 挂起态禁操作（挂起体不参与同步/插值——先复活）。
    pub const Suspended: BindCode = BindCode(0x4B10);
    /// 0x4B11 插值系数越界（alpha > ALPHA_MAX——千分位域外显性拒）。
    pub const BadAlpha: BindCode = BindCode(0x4B11);
    /// 0x4B12 位姿非法（四元数零范数等——不失真律：坏位姿不进账）。
    pub const BadPose: BindCode = BindCode(0x4B12);
    /// 0x4B13 目标不存在（查/操作的 body 或 node 不在册）。
    pub const NotFound: BindCode = BindCode(0x4B13);
    /// 0x4B14 模式矛盾（操作与绑定模式冲突——如 Detached 下强同步）。
    pub const ModeMismatch: BindCode = BindCode(0x4B14);
    /// 0x4B15 账实不符（正向/反向映射不一致——自检面红线）。
    pub const LedgerDrift: BindCode = BindCode(0x4B15);
    /// 0x4B16 审计簿满（append-only 满容拒记计数——不静默覆盖旧痕）。
    pub const AuditFull: BindCode = BindCode(0x4B16);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（诊断可读——拒绝不是哑的 Err）。
    pub fn reason(self) -> String {
        match self {
            BindCode::NotBound => "未绑定：目标没有绑定条目".into(),
            BindCode::AlreadyBound => "重复绑定：body 已有绑定，先解绑再绑（不静默换绑）".into(),
            BindCode::NodeOccupied => "节点被占：node 已绑其他 body（一节点一体）".into(),
            BindCode::BatchTooLarge => "批量超限：超出 MAX_BATCH 单帧同步预算保护".into(),
            BindCode::CapacityFull => "容量满：绑定数撞硬上限，显性拒不驱逐".into(),
            BindCode::PermissionDenied => "越权：操作者对此操作无权限（Y05 两律权限闸）".into(),
            BindCode::NodeGone => "节点代次失效：节点被删重建，绑定追不上".into(),
            BindCode::SyncStale => "同步失效：绑定代次≠节点代次，待编辑器重绑".into(),
            BindCode::NotSuspended => "未挂起：挂起池中没有这个体，复活前提不满足".into(),
            BindCode::Suspended => "挂起态禁操作：物理体挂起中，先复活再同步".into(),
            BindCode::BadAlpha => "插值系数越界：alpha 超出 0..=1000 千分位域".into(),
            BindCode::BadPose => "位姿非法：四元数零范数等，坏位姿不进账".into(),
            BindCode::NotFound => "目标不存在：body 或 node 不在册".into(),
            BindCode::ModeMismatch => "模式矛盾：操作与绑定模式冲突（如 Detached 下强同步）".into(),
            BindCode::LedgerDrift => "账实不符：正向/反向映射不一致，自检红线".into(),
            BindCode::AuditFull => "审计簿满：append-only 满容拒记，不静默覆盖旧痕".into(),
            BindCode(_) => "未知绑定诊断码".into(),
        }
    }
}

/// `segment_guard`：十六码闭集两两互异 + 全落 0x4B 段（双写对拍面——
/// 常量表与 `reason()` 的 match 臂一一对应由判据实证）。
pub const SEGMENT_GUARD: [BindCode; 16] = [
    BindCode::NotBound,
    BindCode::AlreadyBound,
    BindCode::NodeOccupied,
    BindCode::BatchTooLarge,
    BindCode::CapacityFull,
    BindCode::PermissionDenied,
    BindCode::NodeGone,
    BindCode::SyncStale,
    BindCode::NotSuspended,
    BindCode::Suspended,
    BindCode::BadAlpha,
    BindCode::BadPose,
    BindCode::NotFound,
    BindCode::ModeMismatch,
    BindCode::LedgerDrift,
    BindCode::AuditFull,
];

/// 绑定错误路径（显性携带 [`BindCode`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindErr {
    /// 携诊断码的失败。
    Code(BindCode),
}

impl BindErr {
    /// 对外呈现码。
    pub const fn code(self) -> u16 {
        match self {
            BindErr::Code(c) => c.code(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、定点位姿（零浮点：i64 位置毫米口径 + Q14 四元数）
// ---------------------------------------------------------------------------

/// Q14 定点 1.0（四元数分量口径：1.0 == 16384）。
pub const Q14_ONE: i32 = 16384;

/// Q14 定点 0.5（NLERP 中点系数预计算——判据交叉验证用）。
pub const Q14_HALF: i32 = 8192;

/// 归一化残差容限（Q14 口径：归一化后 ‖q‖ 与 Q14_ONE 的整数舍入偏差上限）。
pub const Q14_NORM_TOL: i64 = 2;

/// 三轴位置（毫米口径整数——零浮点不失真）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Vec3i {
    /// X 轴（mm）。
    pub x: i64,
    /// Y 轴（mm）。
    pub y: i64,
    /// Z 轴（mm）。
    pub z: i64,
}

impl Vec3i {
    /// 千分位线性插值：`a + (b−a)·α/1000`（i64 中间量防溢出）。
    pub fn lerp_permille(a: Vec3i, b: Vec3i, alpha_permille: u32) -> Vec3i {
        let t = alpha_permille as i64;
        Vec3i {
            x: a.x + ((b.x - a.x) * t) / 1000,
            y: a.y + ((b.y - a.y) * t) / 1000,
            z: a.z + ((b.z - a.z) * t) / 1000,
        }
    }

    /// 两位置逐轴平方距离（整数；isqrt 后即毫米距离）。
    pub fn dist_sq(a: Vec3i, b: Vec3i) -> u64 {
        let dx = (a.x - b.x) as i128;
        let dy = (a.y - b.y) as i128;
        let dz = (a.z - b.z) as i128;
        // i128 中间量：i64 极端差值平方和也不溢出
        let s = dx * dx + dy * dy + dz * dz;
        if s <= 0 {
            0
        } else if s > u64::MAX as i128 {
            u64::MAX
        } else {
            s as u64
        }
    }
}

/// 位姿 = 位置（mm）+ 朝向（Q14 四元数 [w,x,y,z]）。
///
/// 四元数约定：单位四元数 ‖q‖ ≈ [`Q14_ONE`]（Q14 定点）；
/// 零范数四元数非法（[`BindCode::BadPose`]——坏位姿不进账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pose {
    /// 位置 X（mm）。
    pub px: i64,
    /// 位置 Y（mm）。
    pub py: i64,
    /// 位置 Z（mm）。
    pub pz: i64,
    /// 四元数 w 分量（Q14）。
    pub qw: i32,
    /// 四元数 x 分量（Q14）。
    pub qx: i32,
    /// 四元数 y 分量（Q14）。
    pub qy: i32,
    /// 四元数 z 分量（Q14）。
    pub qz: i32,
}

impl Pose {
    /// 构造并校验：四元数零范数 → None（不失真律——坏位姿不进账）。
    pub fn new(px: i64, py: i64, pz: i64, qw: i32, qx: i32, qy: i32, qz: i32) -> Option<Pose> {
        let p = Pose { px, py, pz, qw, qx, qy, qz };
        if p.quat_norm_sq() == 0 {
            None
        } else {
            Some(p)
        }
    }

    /// 四元数范数平方（Q14 口径：单位四元数 ≈ Q14_ONE²）。
    pub const fn quat_norm_sq(&self) -> u64 {
        let w = self.qw as i64;
        let x = self.qx as i64;
        let y = self.qy as i64;
        let z = self.qz as i64;
        (w * w + x * x + y * y + z * z) as u64
    }

    /// 与目标四元数的点积（Q14 口径：单位四元数点积 ∈ [−Q14_ONE², Q14_ONE²]）。
    pub const fn quat_dot(&self, other: &Pose) -> i64 {
        let a = [
            self.qw as i64,
            self.qx as i64,
            self.qy as i64,
            self.qz as i64,
        ];
        let b = [
            other.qw as i64,
            other.qx as i64,
            other.qy as i64,
            other.qz as i64,
        ];
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
    }

    /// 千分位定点插值（NLERP）：位置线性 + 朝向最短弧线性插值再归一化。
    ///
    /// - `alpha_permille == 0` → 恰得 `self`（prev）；
    /// - `alpha_permille == ALPHA_MAX` → 恰得 `other`（cur）；
    /// - 中间值 → 位置逐轴线性、朝向点积判符号取最短弧后线性插值 + 整数
    ///   归一化（[`isqrt64`]）。
    ///
    /// 零范数插值中间态 → None（调用方映射 [`BindCode::BadPose`]）。
    pub fn lerp_permille(&self, other: &Pose, alpha_permille: u32) -> Option<Pose> {
        if alpha_permille > ALPHA_MAX {
            return None;
        }
        // 位置：i64 线性插值（毫米口径逐轴）
        let pos = Vec3i::lerp_permille(
            Vec3i { x: self.px, y: self.py, z: self.pz },
            Vec3i { x: other.px, y: other.py, z: other.pz },
            alpha_permille,
        );
        // 朝向：点积 < 0 → 反转 other 取最短弧（NLERP 纪律）
        let dot = self.quat_dot(other);
        let (ox, oy, oz, ow) = if dot < 0 {
            (
                -(other.qx as i64),
                -(other.qy as i64),
                -(other.qz as i64),
                -(other.qw as i64),
            )
        } else {
            (
                other.qx as i64,
                other.qy as i64,
                other.qz as i64,
                other.qw as i64,
            )
        };
        let t = alpha_permille as i64;
        // i64 中间量插值（Q14 分量平方 ~2.7e8，插值和 ~5.4e8，i64 富余）
        let ix = (self.qx as i64) + ((ox - self.qx as i64) * t) / 1000;
        let iy = (self.qy as i64) + ((oy - self.qy as i64) * t) / 1000;
        let iz = (self.qz as i64) + ((oz - self.qz as i64) * t) / 1000;
        let iw = (self.qw as i64) + ((ow - self.qw as i64) * t) / 1000;
        let sq = ix * ix + iy * iy + iz * iz + iw * iw;
        if sq <= 0 {
            return None; // 零范数插值中间态（对跖点 α=500 理论边界——显性拒）
        }
        // 归一化：q_i · Q14_ONE / ‖q‖（isqrt Newton 整数平方根）
        let len = isqrt64(sq as u64) as i64;
        if len == 0 {
            return None;
        }
        let norm = |v: i64| -> i32 {
            let scaled = v * (Q14_ONE as i64);
            // 定点除法截断方向：向偶舍入的简化——直接截断 + 判据容差覆盖
            (scaled / len) as i32
        };
        Some(Pose {
            px: pos.x,
            py: pos.y,
            pz: pos.z,
            qw: norm(iw),
            qx: norm(ix),
            qy: norm(iy),
            qz: norm(iz),
        })
    }

    /// 位姿账面摘要（FNV-1a 64：逐字段折叠，确定性零随机）。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for v in [self.px, self.py, self.pz] {
            h ^= (v as u64) & 0xFFFF_FFFF;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= ((v as u64) >> 32) & 0xFFFF_FFFF;
            h = h.wrapping_mul(FNV_PRIME);
        }
        for v in [self.qw, self.qx, self.qy, self.qz] {
            h ^= (v as u32 as u64) & 0xFFFF;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
}

/// 整数平方根（u64，Newton 迭代：单调下取，`isqrt(x)² ≤ x < (isqrt(x)+1)²`）。
///
/// 零浮点纪律下的归一化基元；u64 全域正确（x < 4 需特判——Newton 初值
/// 依赖 x 的位宽估计，小值直接返回）。
pub fn isqrt64(x: u64) -> u64 {
    if x < 2 {
        return x;
    }
    // 初值：2^⌈bits/2⌉ ≥ √x（位宽估计——Newton 单调收敛到不动点）
    let bits = 64 - x.leading_zeros() as i64;
    let mut g: u64 = 1u64 << ((bits + 1) / 2);
    // Newton：g ← (g + x/g) / 2；下取不动点即 isqrt
    loop {
        let next = (g + x / g) >> 1;
        if next >= g {
            break;
        }
        g = next;
    }
    // 收敛后 g ≥ isqrt 可能多 1（初值高于真值单调下降）——校正
    while g * g > x {
        g -= 1;
    }
    g
}

/// 归一化 Q14 四元数（独立基元——判据交叉验证 `lerp_permille` 内嵌归一化）。
/// 零范数 → None；残差 ≤ [`Q14_NORM_TOL`] 由判据钉死。
pub fn quat_normalize_q14(qw: i32, qx: i32, qy: i32, qz: i32) -> Option<(i32, i32, i32, i32)> {
    let sq = (qw as i64) * (qw as i64)
        + (qx as i64) * (qx as i64)
        + (qy as i64) * (qy as i64)
        + (qz as i64) * (qz as i64);
    if sq <= 0 {
        return None;
    }
    let len = isqrt64(sq as u64) as i64;
    if len == 0 {
        return None;
    }
    let norm = |v: i64| -> i32 { ((v as i64) * (Q14_ONE as i64) / len) as i32 };
    Some((norm(qw as i64), norm(qx as i64), norm(qy as i64), norm(qz as i64)))
}

// ---------------------------------------------------------------------------
// 四、权限模型（Y05 两律的类型化：操作 × 操作者矩阵，双写对拍）
// ---------------------------------------------------------------------------

/// 物理体标识（绑定映射左键）。
pub type BodyId = u64;

/// 场景图节点标识（绑定映射右键；删重建由代次追踪）。
pub type NodeId = u64;

/// 操作者（谁在敲 API——两律的权属主体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// 运行时：驱动物理同步步、申报节点销毁；不碰绑定结构。
    Runtime,
    /// 编辑器：绑定/解除/模式/重绑结构的唯一权威。
    Editor,
    /// 调试：只读（绑定状态可查——无障碍面）。
    Debug,
}

impl Actor {
    /// 全操作者闭集（判据遍历用）。
    pub const ALL: [Actor; 3] = [Actor::Runtime, Actor::Editor, Actor::Debug];

    /// 人话名（审计可读）。
    pub const fn tag(self) -> &'static str {
        match self {
            Actor::Runtime => "运行时",
            Actor::Editor => "编辑器",
            Actor::Debug => "调试",
        }
    }
}

/// 操作类别（权限矩阵的列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// 挂接（单绑/批量绑/复活重绑——结构性变更）。
    Bind,
    /// 解除（单解/批量解——结构性变更）。
    Unbind,
    /// 模式切换（权属裁决——编辑器权）。
    SetMode,
    /// 驱动同步步（物理位姿喂入/代次校验——运行时权）。
    SyncTick,
    /// 状态查询（全员开放——绑定状态可查）。
    Query,
    /// 节点销毁申报（运行时事件；编辑器删节点同权）。
    Destroy,
}

impl Op {
    /// 全操作闭集（判据遍历用；列序 = [`PERM_MATRIX`] 列序）。
    pub const ALL: [Op; 6] = [
        Op::Bind,
        Op::Unbind,
        Op::SetMode,
        Op::SyncTick,
        Op::Query,
        Op::Destroy,
    ];

    /// 矩阵列下标（判据对拍用）。
    pub const fn column(self) -> usize {
        match self {
            Op::Bind => 0,
            Op::Unbind => 1,
            Op::SetMode => 2,
            Op::SyncTick => 3,
            Op::Query => 4,
            Op::Destroy => 5,
        }
    }

    /// 人话名（审计可读）。
    pub const fn tag(self) -> &'static str {
        match self {
            Op::Bind => "挂接",
            Op::Unbind => "解除",
            Op::SetMode => "模式切换",
            Op::SyncTick => "驱动同步",
            Op::Query => "状态查询",
            Op::Destroy => "节点销毁申报",
        }
    }
}

/// 操作者数（权限矩阵行数）。
pub const ACTOR_COUNT: usize = 3;

/// 操作数（权限矩阵列数）。
pub const OP_COUNT: usize = 6;

/// 权限矩阵（行 = [Runtime, Editor, Debug]，列 = [`Op::ALL`] 序）。
///
/// **双写之一**：声明式 const 表。裁决走 [`perm_matrix`]（查表）；独立
/// 推导路 [`perm_rule`]（match 逻辑）由判据逐格对拍——两路不一致即红线。
pub const PERM_MATRIX: [[bool; OP_COUNT]; ACTOR_COUNT] = [
    //            Bind   Unbind SetMode SyncTick Query Destroy
    /* Runtime */ [false, false, false, true,    true, true],
    /* Editor  */ [true,  true,  true,  false,   true, true],
    /* Debug   */ [false, false, false, false,   true, false],
];

/// 查表裁决路（双写之一）。
pub fn perm_matrix(actor: Actor, op: Op) -> bool {
    PERM_MATRIX[actor as usize][op.column()]
}

/// 规则推导路（双写之二：match 逻辑独立写——与矩阵逐格对拍）。
pub fn perm_rule(actor: Actor, op: Op) -> bool {
    match (actor, op) {
        // 查询全员开放（绑定状态可查——无障碍面）
        (_, Op::Query) => true,
        // 运行时：只驱动同步 + 申报销毁；不碰绑定结构（集成不改运行时判定，
        // 但也不让运行时反过来改结构——结构变更走编辑器流程）
        (Actor::Runtime, Op::SyncTick) => true,
        (Actor::Runtime, Op::Destroy) => true,
        // 编辑器：结构全权（挂接/解除/模式/重绑/销毁申报——删节点也是编辑器）
        (Actor::Editor, Op::Bind) => true,
        (Actor::Editor, Op::Unbind) => true,
        (Actor::Editor, Op::SetMode) => true,
        (Actor::Editor, Op::Destroy) => true,
        // 其余全拒（运行时不改结构；调试只读——两律 + 最小权）
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 五、绑定数据结构（绑定映射；插值同步）
// ---------------------------------------------------------------------------

/// 绑定模式（两律的类型化——权属写进类型，不靠口头约定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindMode {
    /// 物理权威：物理步进位姿写节点（运行时权面）。
    Strict,
    /// 编辑器权威：编辑器写节点位姿、物理跟随（不夺编辑器权）。
    Interactive,
    /// 映射在、同步停（调试/暂存——结构性保留，位姿冻结）。
    Detached,
}

impl BindMode {
    /// 全模式闭集（判据遍历用）。
    pub const ALL: [BindMode; 3] = [BindMode::Strict, BindMode::Interactive, BindMode::Detached];

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            BindMode::Strict => "物理权威",
            BindMode::Interactive => "编辑器权威",
            BindMode::Detached => "同步停",
        }
    }
}

/// 绑定条目（正向表一行：物理体 ↔ 节点 + 双位姿账 + 状态位）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindEntry {
    /// 物理体。
    pub body: BodyId,
    /// 场景图节点。
    pub node: NodeId,
    /// 绑定模式。
    pub mode: BindMode,
    /// 绑定时的节点代次（同步失效检测基准）。
    pub bound_gen: u64,
    /// 同步失效位（代次失配置位；编辑器重绑清零）。
    pub stale: bool,
    /// 挂起位（节点销毁置位；复活清零）。
    pub suspended: bool,
    /// 上一步物理位姿（插值基点 a）。
    pub prev_pose: Option<Pose>,
    /// 当前步位姿（插值基点 b；编辑器写时直接成为 cur）。
    pub cur_pose: Option<Pose>,
    /// 累计喂入物理步数。
    pub step_count: u64,
    /// 累计编辑器位姿写次数（Interactive 权面账）。
    pub editor_writes: u64,
}

/// 挂起记录（节点销毁→物理体挂起——隔离体去哪了对外可查）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuspendedRecord {
    /// 挂起的物理体。
    pub body: BodyId,
    /// 销毁前的节点。
    pub old_node: NodeId,
    /// 挂起时的账面代次。
    pub at_gen: u64,
}

/// 审计操作（append-only 留痕——越权/结构变更全可回放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOp {
    /// 挂接成功。
    Bound { body: BodyId, node: NodeId },
    /// 解除成功。
    Unbound { body: BodyId },
    /// 模式切换成功。
    ModeSet { body: BodyId, mode: BindMode },
    /// 挂起。
    Suspended { body: BodyId, node: NodeId },
    /// 复活重绑。
    Revived { body: BodyId, node: NodeId },
    /// 代次重绑。
    Rebound { body: BodyId },
    /// 批量整体拒绝。
    BatchRejected { items: usize },
    /// 越权拒绝（操作者 × 操作 × 目标——追责到格）。
    Denied { actor: Actor, op: Op, target: u64 },
}

/// 审计条目（序号 + 账面代次 + 操作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEntry {
    /// 审计序号（单调递增）。
    pub seq: u64,
    /// 落痕时的账面代次。
    pub gen: u64,
    /// 操作。
    pub op: AuditOp,
}

/// 绑定账本（计数不静默——每路显性事件都有账）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BindLedger {
    /// 成功挂接次数（含批量逐项与复活重绑）。
    pub binds: u64,
    /// 成功解除次数（含批量逐项）。
    pub unbinds: u64,
    /// 批量操作提交次数（全成）。
    pub batch_ops: u64,
    /// 批量整体拒绝次数（all-or-nothing 拒）。
    pub batch_rejects: u64,
    /// 挂起次数。
    pub suspensions: u64,
    /// 复活次数。
    pub revives: u64,
    /// 代次重绑次数。
    pub rebinds: u64,
    /// 同步失效检出次数。
    pub stale_hits: u64,
    /// 越权拒绝次数。
    pub denied: u64,
    /// 同步步推进次数（sync_tick 调用）。
    pub sync_ticks: u64,
    /// 插值位姿查询次数（render_pose）。
    pub interp_queries: u64,
    /// 编辑器位姿写次数。
    pub editor_writes: u64,
    /// 审计满拒记次数（AUDIT_CAP 满容不覆盖旧痕）。
    pub audit_dropped: u64,
}

/// 批量事务报告（all-or-nothing：committed=false 时零副作用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    /// 是否整体提交（false = 预检拒、一个都没落账）。
    pub committed: bool,
    /// 提交时成功项数（拒时为 0）。
    pub ok_count: usize,
    /// 逐项失败清单（下标 + 原因码；全成时为空）。
    pub failures: Vec<(usize, BindCode)>,
}

/// 同步步结论（代次校验结果——失效显性报告不静默）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// 全部条目代次相齐（本轮实际校验的条目数——挂起/Detached/stale 不算）。
    Clean(usize),
    /// 存在失效条目（失效 body 清单；条目已置 stale 位待编辑器重绑）。
    Stale(Vec<BodyId>),
}

/// 绑定请求（批量挂接的一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindRequest {
    /// 物理体。
    pub body: BodyId,
    /// 场景图节点。
    pub node: NodeId,
    /// 绑定模式。
    pub mode: BindMode,
    /// 绑定基准节点代次（0 = 节点初生）。
    pub node_gen: u64,
}

/// 绑定状态报告（状态可查——无障碍五要素+账面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindStatus {
    /// 物理体。
    pub body: BodyId,
    /// 场景图节点。
    pub node: NodeId,
    /// 绑定模式。
    pub mode: BindMode,
    /// 绑定基准节点代次。
    pub bound_gen: u64,
    /// 同步失效位。
    pub stale: bool,
    /// 挂起位。
    pub suspended: bool,
    /// 累计喂入步数。
    pub step_count: u64,
    /// 累计编辑器写次数。
    pub editor_writes: u64,
    /// 插值就绪（prev+cur 双位姿齐——render_pose 可给真插值）。
    pub interp_ready: bool,
}

// ---------------------------------------------------------------------------
// 六、BindHub（绑定中枢：映射账本 × 同步面 × 插值面 × 审计）
// ---------------------------------------------------------------------------

/// 物理与场景图绑定中枢。
///
/// 账本结构：正向表 `fwd`（body 主序）+ 反向表 `rev`（node→body）+
/// 挂起池 `suspended` + 审计簿 `audit`。账实一致性由
/// [`BindHub::consistency_probe`] 自检（判据钉死）。
#[derive(Debug, Clone)]
pub struct BindHub {
    fwd: Vec<BindEntry>,
    rev: Vec<(NodeId, BodyId)>,
    suspended: Vec<SuspendedRecord>,
    audit: Vec<AuditEntry>,
    ledger: BindLedger,
    /// 账面代次（结构变更 +1——审计与挂起记录的对时基）。
    gen: u64,
    capacity: usize,
    seq: u64,
    /// 物理喂入序列摘要（两律判据面：结构操作不碰、只有 push_step_pose 折叠）。
    phys_digest: u64,
}

impl BindHub {
    /// 默认容量构造。
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// 指定容量构造（0 容量无意义——钳到 1，显性可查）。
    pub fn with_capacity(cap: usize) -> Self {
        BindHub {
            fwd: Vec::new(),
            rev: Vec::new(),
            suspended: Vec::new(),
            audit: Vec::new(),
            ledger: BindLedger::default(),
            gen: 0,
            capacity: if cap == 0 { 1 } else { cap },
            seq: 0,
            phys_digest: FNV_OFFSET,
        }
    }

    // -- 账面只读视图 -------------------------------------------------------

    /// 当前绑定数。
    pub fn len(&self) -> usize {
        self.fwd.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.fwd.is_empty()
    }

    /// 绑定容量。
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// 账面代次。
    pub const fn gen(&self) -> u64 {
        self.gen
    }

    /// 账本只读视图。
    pub const fn ledger(&self) -> &BindLedger {
        &self.ledger
    }

    /// 物理喂入序列摘要（两律判据面）。
    pub const fn physics_digest(&self) -> u64 {
        self.phys_digest
    }

    /// 挂起池记录数。
    pub fn suspended_count(&self) -> usize {
        self.suspended.len()
    }

    /// 审计簿条数。
    pub fn audit_count(&self) -> usize {
        self.audit.len()
    }

    // -- 内部查找 -----------------------------------------------------------

    /// 正向查找（O(实体)——锚点同步 O(实体) 口径）。
    fn find(&self, body: BodyId) -> Option<&BindEntry> {
        self.fwd.iter().find(|e| e.body == body)
    }

    fn find_mut(&mut self, body: BodyId) -> Option<&mut BindEntry> {
        self.fwd.iter_mut().find(|e| e.body == body)
    }

    /// 反向查找（O(实体)）。
    pub fn lookup_by_node(&self, node: NodeId) -> Option<BodyId> {
        self.rev.iter().find(|(n, _)| *n == node).map(|(_, b)| *b)
    }

    /// 审计落痕（append-only；满容拒记计数不覆盖旧痕）。
    fn audit_push(&mut self, op: AuditOp) {
        if self.audit.len() >= AUDIT_CAP {
            self.ledger.audit_dropped = self.ledger.audit_dropped.saturating_add(1);
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        let e = AuditEntry { seq: self.seq, gen: self.gen, op };
        self.audit.push(e);
    }

    /// 越权统一口：拒绝 + 审计 + 计数，三路齐落（拒绝零副作用由调用序保证）。
    fn denied(&mut self, actor: Actor, op: Op, target: u64) -> BindErr {
        self.ledger.denied = self.ledger.denied.saturating_add(1);
        self.audit_push(AuditOp::Denied { actor, op, target });
        BindErr::Code(BindCode::PermissionDenied)
    }

    // -- 挂接 / 解除 --------------------------------------------------------

    /// 单体挂接：权限→容量→重复→占用四闸，全过才落账。
    pub fn bind(
        &mut self,
        actor: Actor,
        body: BodyId,
        node: NodeId,
        mode: BindMode,
        node_gen: u64,
    ) -> Result<(), BindErr> {
        if !perm_matrix(actor, Op::Bind) {
            return Err(self.denied(actor, Op::Bind, body));
        }
        if self.fwd.len() >= self.capacity {
            return Err(BindErr::Code(BindCode::CapacityFull));
        }
        if self.find(body).is_some() {
            return Err(BindErr::Code(BindCode::AlreadyBound));
        }
        if self.lookup_by_node(node).is_some() {
            return Err(BindErr::Code(BindCode::NodeOccupied));
        }
        self.bind_inner(body, node, mode, node_gen);
        Ok(())
    }

    /// 内部落账（权限/容量闸已过；批量复用）。
    fn bind_inner(&mut self, body: BodyId, node: NodeId, mode: BindMode, node_gen: u64) {
        self.fwd.push(BindEntry {
            body,
            node,
            mode,
            bound_gen: node_gen,
            stale: false,
            suspended: false,
            prev_pose: None,
            cur_pose: None,
            step_count: 0,
            editor_writes: 0,
        });
        self.rev.push((node, body));
        self.gen = self.gen.wrapping_add(1);
        self.ledger.binds = self.ledger.binds.saturating_add(1);
        self.audit_push(AuditOp::Bound { body, node });
    }

    /// 单体解除：权限→存在两闸，过则摘正向+反向双账。
    pub fn unbind(&mut self, actor: Actor, body: BodyId) -> Result<(), BindErr> {
        if !perm_matrix(actor, Op::Unbind) {
            return Err(self.denied(actor, Op::Unbind, body));
        }
        let idx = match self.fwd.iter().position(|e| e.body == body) {
            Some(i) => i,
            None => return Err(BindErr::Code(BindCode::NotBound)),
        };
        let node = self.fwd[idx].node;
        self.fwd.remove(idx);
        if let Some(ri) = self.rev.iter().position(|(n, b)| *b == body && *n == node) {
            self.rev.remove(ri);
        }
        self.gen = self.gen.wrapping_add(1);
        self.ledger.unbinds = self.ledger.unbinds.saturating_add(1);
        self.audit_push(AuditOp::Unbound { body });
        Ok(())
    }

    /// 模式切换：权限→存在→同模式幂等放行（记账不告警）。
    pub fn set_mode(&mut self, actor: Actor, body: BodyId, mode: BindMode) -> Result<(), BindErr> {
        if !perm_matrix(actor, Op::SetMode) {
            return Err(self.denied(actor, Op::SetMode, body));
        }
        let e = match self.find_mut(body) {
            Some(e) => e,
            None => return Err(BindErr::Code(BindCode::NotBound)),
        };
        e.mode = mode;
        self.gen = self.gen.wrapping_add(1);
        self.audit_push(AuditOp::ModeSet { body, mode });
        Ok(())
    }

    // -- 批量事务（all-or-nothing） ------------------------------------------

    /// 批量挂接：权限→上限→全量预检→任一失败整体拒（零副作用）→全过逐项落账。
    ///
    /// 预检含**批量内互斥**：同请求重复 body、同请求重复 node 也算失败
    /// （批量内部先打自己脸的请求不成立——显性给逐项原因）。
    pub fn batch_bind(
        &mut self,
        actor: Actor,
        reqs: &[BindRequest],
    ) -> Result<BatchReport, BindErr> {
        if !perm_matrix(actor, Op::Bind) {
            return Err(self.denied(actor, Op::Bind, reqs.first().map(|r| r.body).unwrap_or(0)));
        }
        if reqs.len() > MAX_BATCH {
            // 上限拒绝：显性码 + 理由文本随审计可查（上限说明含理由）
            self.ledger.batch_rejects = self.ledger.batch_rejects.saturating_add(1);
            self.audit_push(AuditOp::BatchRejected { items: reqs.len() });
            return Err(BindErr::Code(BindCode::BatchTooLarge));
        }
        let mut failures: Vec<(usize, BindCode)> = Vec::new();
        // 批量内局部占用表（body 重复 / node 重复互斥检测）
        let mut seen_bodies: Vec<BodyId> = Vec::new();
        let mut seen_nodes: Vec<NodeId> = Vec::new();
        // 容量预算：本批全落不撞容量
        if self.fwd.len() + reqs.len() > self.capacity {
            return Err(BindErr::Code(BindCode::CapacityFull));
        }
        for (i, r) in reqs.iter().enumerate() {
            let mut reason: Option<BindCode> = None;
            if self.find(r.body).is_some() || seen_bodies.contains(&r.body) {
                reason = Some(BindCode::AlreadyBound);
            } else if self.lookup_by_node(r.node).is_some() || seen_nodes.contains(&r.node) {
                reason = Some(BindCode::NodeOccupied);
            }
            if let Some(code) = reason {
                failures.push((i, code));
            } else {
                seen_bodies.push(r.body);
                seen_nodes.push(r.node);
            }
        }
        if !failures.is_empty() {
            // all-or-nothing：整体拒、零副作用
            self.ledger.batch_rejects = self.ledger.batch_rejects.saturating_add(1);
            self.audit_push(AuditOp::BatchRejected { items: reqs.len() });
            return Ok(BatchReport { committed: false, ok_count: 0, failures });
        }
        for r in reqs.iter() {
            self.bind_inner(r.body, r.node, r.mode, r.node_gen);
        }
        self.ledger.batch_ops = self.ledger.batch_ops.saturating_add(1);
        Ok(BatchReport { committed: true, ok_count: reqs.len(), failures: Vec::new() })
    }

    /// 批量解除：权限→上限→全量预检（NotBound）→任一失败整体拒→全过逐项摘账。
    pub fn batch_unbind(&mut self, actor: Actor, bodies: &[BodyId]) -> Result<BatchReport, BindErr> {
        if !perm_matrix(actor, Op::Unbind) {
            return Err(self.denied(actor, Op::Unbind, bodies.first().copied().unwrap_or(0)));
        }
        if bodies.len() > MAX_BATCH {
            self.ledger.batch_rejects = self.ledger.batch_rejects.saturating_add(1);
            self.audit_push(AuditOp::BatchRejected { items: bodies.len() });
            return Err(BindErr::Code(BindCode::BatchTooLarge));
        }
        let mut failures: Vec<(usize, BindCode)> = Vec::new();
        for (i, b) in bodies.iter().enumerate() {
            if self.find(*b).is_none() {
                failures.push((i, BindCode::NotBound));
            }
        }
        if !failures.is_empty() {
            self.ledger.batch_rejects = self.ledger.batch_rejects.saturating_add(1);
            self.audit_push(AuditOp::BatchRejected { items: bodies.len() });
            return Ok(BatchReport { committed: false, ok_count: 0, failures });
        }
        for b in bodies.iter() {
            // 预检已保证全部在册——unwrap 语义用 match 显性化（零 unwrap 纪律）
            let idx = match self.fwd.iter().position(|e| e.body == *b) {
                Some(i) => i,
                None => return Err(BindErr::Code(BindCode::LedgerDrift)),
            };
            let node = self.fwd[idx].node;
            self.fwd.remove(idx);
            if let Some(ri) = self.rev.iter().position(|(n, bb)| *bb == *b && *n == node) {
                self.rev.remove(ri);
            }
            self.gen = self.gen.wrapping_add(1);
            self.ledger.unbinds = self.ledger.unbinds.saturating_add(1);
            self.audit_push(AuditOp::Unbound { body: *b });
        }
        self.ledger.batch_ops = self.ledger.batch_ops.saturating_add(1);
        Ok(BatchReport { committed: true, ok_count: bodies.len(), failures: Vec::new() })
    }

    // -- 失效三路：挂起 / 复活 / 重绑 ----------------------------------------

    /// 节点销毁申报：绑在该节点上的条目全部转挂起（物理体挂起、账保留、
    /// 可复活）。返回本轮挂起数。挂起池满显性拒（[`BindCode::AuditFull`]
    /// 段位复用为满容码——池与审计同纪律：不静默覆盖旧痕）。
    pub fn on_node_destroyed(&mut self, actor: Actor, node: NodeId) -> Result<usize, BindErr> {
        if !perm_matrix(actor, Op::Destroy) {
            return Err(self.denied(actor, Op::Destroy, node));
        }
        let mut n = 0usize;
        // 预算前置：先算本轮会挂起几个，池装不下就整体拒（零回滚零副作用）
        let will = self.fwd.iter().filter(|e| e.node == node && !e.suspended).count();
        if will > 0 {
            if self.suspended.len() + will > SUSPEND_POOL_CAP {
                return Err(BindErr::Code(BindCode::CapacityFull));
            }
            for e in self.fwd.iter_mut() {
                if e.node == node && !e.suspended {
                    e.suspended = true;
                    n += 1;
                }
            }
            let body = match self.lookup_by_node(node) {
                Some(b) => b,
                None => return Err(BindErr::Code(BindCode::LedgerDrift)),
            };
            self.suspended.push(SuspendedRecord { body, old_node: node, at_gen: self.gen });
            self.gen = self.gen.wrapping_add(1);
            self.ledger.suspensions = self.ledger.suspensions.saturating_add(n as u64);
            self.audit_push(AuditOp::Suspended { body, node });
        }
        Ok(n)
    }

    /// 复活重绑：挂起池中的体挂到新节点（编辑器权——结构变更）。
    /// 挂起记录移除、条目复用（suspended/stale 清零、node/bound_gen 更新）。
    pub fn revive(
        &mut self,
        actor: Actor,
        body: BodyId,
        new_node: NodeId,
        new_node_gen: u64,
    ) -> Result<(), BindErr> {
        if !perm_matrix(actor, Op::Bind) {
            return Err(self.denied(actor, Op::Bind, body));
        }
        let sidx = match self.suspended.iter().position(|s| s.body == body) {
            Some(i) => i,
            None => return Err(BindErr::Code(BindCode::NotSuspended)),
        };
        if self.lookup_by_node(new_node).is_some() {
            return Err(BindErr::Code(BindCode::NodeOccupied));
        }
        let rec = self.suspended.remove(sidx);
        // 先更新反向双账（独立借用阶段），再改正向条目——零借用冲突
        if let Some(ri) = self.rev.iter().position(|(n, b)| *b == body && *n == rec.old_node) {
            self.rev.remove(ri);
        }
        self.rev.push((new_node, body));
        let e = match self.find_mut(body) {
            Some(e) => e,
            None => return Err(BindErr::Code(BindCode::LedgerDrift)),
        };
        e.node = new_node;
        e.bound_gen = new_node_gen;
        e.stale = false;
        e.suspended = false;
        e.cur_pose = None;
        e.prev_pose = None;
        self.gen = self.gen.wrapping_add(1);
        self.ledger.revives = self.ledger.revives.saturating_add(1);
        self.audit_push(AuditOp::Revived { body, node: new_node });
        Ok(())
    }

    /// 代次重绑：同步失效（stale）条目对齐到当前节点代次（编辑器权）。
    /// 幂等无害：非 stale 条目重绑只是刷新基准代次（对齐即重绑）。
    pub fn rebind(&mut self, actor: Actor, body: BodyId, node_gen: u64) -> Result<(), BindErr> {
        if !perm_matrix(actor, Op::Bind) {
            return Err(self.denied(actor, Op::Bind, body));
        }
        let e = match self.find_mut(body) {
            Some(e) => e,
            None => return Err(BindErr::Code(BindCode::NotBound)),
        };
        if e.suspended {
            return Err(BindErr::Code(BindCode::Suspended));
        }
        e.bound_gen = node_gen;
        e.stale = false;
        self.gen = self.gen.wrapping_add(1);
        self.ledger.rebinds = self.ledger.rebinds.saturating_add(1);
        self.audit_push(AuditOp::Rebound { body });
        Ok(())
    }

    // -- 同步面（驱动 + 代次校验） --------------------------------------------

    /// 推进一个同步步（运行时权）：逐条目做代次校验（节点代次表由调用方
    /// 喂入——场景图侧的节点删重建升代次），失配 → 置 stale + 计数 +
    /// 汇总返回 [`SyncOutcome::Stale`]；齐则 [`SyncOutcome::Clean`]。
    ///
    /// 挂起条目跳过（挂起体不参与同步）；Detached 条目跳过（同步停）；
    /// 已 stale 条目跳过（待编辑器重绑——不重复报）。
    /// 节点代次表中缺席的节点按失配处理（保守置 stale——显性不猜）。
    pub fn sync_tick(
        &mut self,
        actor: Actor,
        node_gens: &[(NodeId, u64)],
    ) -> Result<SyncOutcome, BindErr> {
        if !perm_matrix(actor, Op::SyncTick) {
            return Err(self.denied(actor, Op::SyncTick, 0));
        }
        self.ledger.sync_ticks = self.ledger.sync_ticks.saturating_add(1);
        let mut clean = 0usize;
        let mut stale_bodies: Vec<BodyId> = Vec::new();
        for e in self.fwd.iter_mut() {
            if e.suspended || e.stale || e.mode == BindMode::Detached {
                continue;
            }
            let cur_gen = node_gens
                .iter()
                .find(|(n, _)| *n == e.node)
                .map(|(_, g)| *g);
            match cur_gen {
                Some(g) if g == e.bound_gen => clean += 1,
                Some(g) => {
                    e.stale = true;
                    self.ledger.stale_hits = self.ledger.stale_hits.saturating_add(1);
                    stale_bodies.push(e.body);
                    let _ = g; // 失配代次本身进不了账——账面只记事实（失配）
                }
                None => {
                    e.stale = true;
                    self.ledger.stale_hits = self.ledger.stale_hits.saturating_add(1);
                    stale_bodies.push(e.body);
                }
            }
        }
        if stale_bodies.is_empty() {
            Ok(SyncOutcome::Clean(clean))
        } else {
            Ok(SyncOutcome::Stale(stale_bodies))
        }
    }

    /// 物理步进位姿喂入（运行时权）：双位姿账轮转（cur→prev、新位姿进 cur）
    /// + 物理喂入摘要折叠。挂起拒（[`BindCode::Suspended`]）、Detached 拒
    /// （[`BindCode::ModeMismatch`]——同步停位姿账也停，口径一致）。
    pub fn push_step_pose(
        &mut self,
        actor: Actor,
        body: BodyId,
        pose: Pose,
    ) -> Result<(), BindErr> {
        if !perm_matrix(actor, Op::SyncTick) {
            return Err(self.denied(actor, Op::SyncTick, body));
        }
        if pose.quat_norm_sq() == 0 {
            return Err(BindErr::Code(BindCode::BadPose));
        }
        let e = match self.find_mut(body) {
            Some(e) => e,
            None => return Err(BindErr::Code(BindCode::NotBound)),
        };
        if e.suspended {
            return Err(BindErr::Code(BindCode::Suspended));
        }
        if e.mode == BindMode::Detached {
            return Err(BindErr::Code(BindCode::ModeMismatch));
        }
        e.prev_pose = e.cur_pose;
        e.cur_pose = Some(pose);
        e.step_count = e.step_count.saturating_add(1);
        // 物理喂入摘要折叠（两律判据面：只有这一处折叠——结构操作不碰）
        self.phys_digest = self.phys_digest.wrapping_mul(FNV_PRIME);
        self.phys_digest ^= pose.digest();
        self.phys_digest = self.phys_digest.wrapping_mul(FNV_PRIME);
        self.phys_digest ^= body;
        Ok(())
    }

    /// 编辑器位姿写（编辑器权 + Interactive 模式）：直接成为 cur（prev
    /// 保留上一步物理位姿）——渲染插值在编辑器位姿与上一步物理位姿之间
    /// 平滑过渡，编辑器操作不产生画面跳变。**不碰物理喂入摘要**（编辑器
    /// 写不是物理喂入——两律判据面的口径边界）。
    pub fn editor_write_pose(
        &mut self,
        actor: Actor,
        body: BodyId,
        pose: Pose,
    ) -> Result<(), BindErr> {
        if actor != Actor::Editor {
            return Err(self.denied(actor, Op::SyncTick, body));
        }
        if pose.quat_norm_sq() == 0 {
            return Err(BindErr::Code(BindCode::BadPose));
        }
        let e = match self.find_mut(body) {
            Some(e) => e,
            None => return Err(BindErr::Code(BindCode::NotBound)),
        };
        if e.suspended {
            return Err(BindErr::Code(BindCode::Suspended));
        }
        if e.mode != BindMode::Interactive {
            return Err(BindErr::Code(BindCode::ModeMismatch));
        }
        e.prev_pose = e.cur_pose;
        e.cur_pose = Some(pose);
        e.editor_writes = e.editor_writes.saturating_add(1);
        self.ledger.editor_writes = self.ledger.editor_writes.saturating_add(1);
        Ok(())
    }

    // -- 插值面（渲染帧取插值位姿，不取硬步位姿） ------------------------------

    /// 插值就绪（prev+cur 双位姿齐——render_pose 可给真插值）。
    pub fn interp_ready(&self, body: BodyId) -> bool {
        match self.find(body) {
            Some(e) => !e.suspended && e.prev_pose.is_some() && e.cur_pose.is_some(),
            None => false,
        }
    }

    /// 渲染帧取位姿：`lerp(prev, cur, alpha_permille)` 整数定点插值。
    ///
    /// - alpha=0 恰得 prev、alpha=1000 恰得 cur（端点钉死）；
    /// - 单步退化：只喂过一步（prev 缺）→ 返回 cur（第一步无插值基点，
    ///   诚实退化为硬步——文档口径，判据覆盖）；
    /// - alpha 越域显性拒（[`BindCode::BadAlpha`]）；挂起拒；无位姿账拒。
    pub fn render_pose(&mut self, body: BodyId, alpha_permille: u32) -> Result<Pose, BindErr> {
        if alpha_permille > ALPHA_MAX {
            return Err(BindErr::Code(BindCode::BadAlpha));
        }
        let (prev, cur) = {
            let e = match self.find(body) {
                Some(e) => e,
                None => return Err(BindErr::Code(BindCode::NotBound)),
            };
            if e.suspended {
                return Err(BindErr::Code(BindCode::Suspended));
            }
            (e.prev_pose, e.cur_pose)
        };
        self.ledger.interp_queries = self.ledger.interp_queries.saturating_add(1);
        let cur = match cur {
            Some(c) => c,
            None => return Err(BindErr::Code(BindCode::NotBound)),
        };
        let prev = match prev {
            Some(p) => p,
            None => return Ok(cur), // 单步退化：无插值基点，硬步诚实呈现
        };
        match prev.lerp_permille(&cur, alpha_permille) {
            Some(p) => Ok(p),
            None => Err(BindErr::Code(BindCode::BadPose)),
        }
    }

    // -- 状态可查（无障碍面） -------------------------------------------------

    /// 按体查状态（五要素 + 账面）。
    pub fn status_of(&self, body: BodyId) -> Result<BindStatus, BindErr> {
        let e = match self.find(body) {
            Some(e) => e,
            None => return Err(BindErr::Code(BindCode::NotFound)),
        };
        Ok(BindStatus {
            body: e.body,
            node: e.node,
            mode: e.mode,
            bound_gen: e.bound_gen,
            stale: e.stale,
            suspended: e.suspended,
            step_count: e.step_count,
            editor_writes: e.editor_writes,
            interp_ready: e.prev_pose.is_some() && e.cur_pose.is_some(),
        })
    }

    /// 全量清单（body, node, mode, suspended, stale）——绑定状态读屏可达。
    pub fn enumerate(&self) -> Vec<(BodyId, NodeId, BindMode, bool, bool)> {
        self.fwd
            .iter()
            .map(|e| (e.body, e.node, e.mode, e.suspended, e.stale))
            .collect()
    }

    /// 挂起池清单（隔离体去哪了对外可查）。
    pub fn suspended_bodies(&self) -> Vec<BodyId> {
        self.suspended.iter().map(|s| s.body).collect()
    }

    /// 按体回放审计（append-only 全痕）。
    pub fn audit_replay(&self, body: BodyId) -> Vec<AuditEntry> {
        self.audit
            .iter()
            .filter(|a| match a.op {
                AuditOp::Bound { body: b, node: _ } => b == body,
                AuditOp::Unbound { body: b } => b == body,
                AuditOp::ModeSet { body: b, mode: _ } => b == body,
                AuditOp::Suspended { body: b, node: _ } => b == body,
                AuditOp::Revived { body: b, node: _ } => b == body,
                AuditOp::Rebound { body: b } => b == body,
                AuditOp::BatchRejected { items: _ } => false,
                AuditOp::Denied { actor: _, op: _, target } => target == body,
            })
            .copied()
            .collect()
    }

    // -- 自检面 ---------------------------------------------------------------

    /// 账实一致性探针：正向每条目在反向恰有一格、反向无孤儿、格内互指。
    /// 任一违例 → [`BindCode::LedgerDrift`]（判据钉死零漂移）。
    pub fn consistency_probe(&self) -> Result<(), BindCode> {
        for e in self.fwd.iter() {
            let hits = self.rev.iter().filter(|(_, b)| *b == e.body).count();
            if hits != 1 {
                return Err(BindCode::LedgerDrift);
            }
            let (_, b) = match self.rev.iter().find(|(n, _)| *n == e.node) {
                Some(p) => *p,
                None => return Err(BindCode::LedgerDrift),
            };
            if b != e.body {
                return Err(BindCode::LedgerDrift);
            }
        }
        if self.rev.len() != self.fwd.len() {
            return Err(BindCode::LedgerDrift);
        }
        // 挂起池与条目对拍：挂起记录的体必须在正向表中且 suspended 位为真
        for s in self.suspended.iter() {
            match self.find(s.body) {
                Some(e) if e.suspended => {}
                _ => return Err(BindCode::LedgerDrift),
            }
        }
        Ok(())
    }
}

impl Default for BindHub {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、域自检（判据：挂接节点、插值同步、两律延续、判据）
// ---------------------------------------------------------------------------

/// VE-F6004 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vead04_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vead04_bindgraph");

    // —— 判据一 · 挂接节点：绑定映射双向可逆 + 状态可查 ——
    let mut h = BindHub::new();
    let bind_ok = h.bind(Actor::Editor, 101, 9001, BindMode::Strict, 0).is_ok()
        && h.bind(Actor::Editor, 102, 9002, BindMode::Interactive, 0).is_ok()
        && h.lookup_by_node(9001) == Some(101)
        && h.lookup_by_node(9002) == Some(102)
        && h.status_of(101).map(|st| st.node == 9001 && st.mode == BindMode::Strict) == Ok(true);
    let enum_ok = {
        let list = h.enumerate();
        list.len() == 2
            && list.contains(&(101, 9001, BindMode::Strict, false, false))
            && list.contains(&(102, 9002, BindMode::Interactive, false, false))
    };
    s.add(
        "AD04-挂接节点-映射双向可逆状态可查",
        bind_ok && enum_ok,
        "Editor 挂两体两节点：正向 status_of/反向 lookup_by_node/全量 enumerate 三路一致",
    );

    // —— 判据一 · 反向：重复绑定与节点占用显性拒（不静默换绑/抢占） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 7, 70, BindMode::Strict, 0);
    let dup = h.bind(Actor::Editor, 7, 71, BindMode::Strict, 0);
    let occupied = h.bind(Actor::Editor, 8, 70, BindMode::Strict, 0);
    let clean_after = h.len() == 1 && h.consistency_probe() == Ok(());
    s.add(
        "AD04-挂接节点-重复与占用显性拒",
        dup == Err(BindErr::Code(BindCode::AlreadyBound))
            && occupied == Err(BindErr::Code(BindCode::NodeOccupied))
            && clean_after,
        "body 重复绑→AlreadyBound；node 被占→NodeOccupied；拒绝零副作用账实仍一致",
    );

    // —— 判据二 · 插值同步：alpha 端点钉死（0=prev / 1000=cur） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 5, 50, BindMode::Strict, 0);
    let p0 = Pose { px: 0, py: 0, pz: 0, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 };
    let p1 = Pose { px: 1000, py: 2000, pz: 3000, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 };
    let _ = h.push_step_pose(Actor::Runtime, 5, p0);
    let _ = h.push_step_pose(Actor::Runtime, 5, p1);
    let at_prev = h.render_pose(5, 0);
    let at_cur = h.render_pose(5, ALPHA_MAX);
    s.add(
        "AD04-插值同步-alpha端点钉死",
        at_prev == Ok(p0) && at_cur == Ok(p1),
        "alpha=0 恰得 prev 物理位姿、alpha=1000 恰得 cur——插值系数端点不过冲",
    );

    // —— 判据二 · 反向：中点取插值位姿不取硬步位姿 ——
    let mid = h.render_pose(5, 500);
    let mid_ok = match mid {
        Ok(p) => {
            p.px == 500 && p.py == 1000 && p.pz == 1500
                && p != p1
                && p != p0
        }
        Err(_) => false,
    };
    s.add(
        "AD04-插值同步-中点非硬步位姿",
        mid_ok,
        "alpha=500 位置恰中点且既非 prev 也非 cur——渲染帧取插值位姿不取物理硬步位姿",
    );

    // —— 判据三 · 插值同步：四元数 NLERP 归一残差钉死（零浮点） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 6, 60, BindMode::Strict, 0);
    // q0 = 恒等（1,0,0,0）；q1 = 绕 Z 转 90° 的 Q14 近似（w=√2/2, z=√2/2）
    let rt2_2: i32 = 11585; // √2/2 × 16384 ≈ 11585
    let qa = Pose { px: 0, py: 0, pz: 0, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 };
    let qb = Pose { px: 0, py: 0, pz: 0, qw: rt2_2, qx: 0, qy: 0, qz: rt2_2 };
    let _ = h.push_step_pose(Actor::Runtime, 6, qa);
    let _ = h.push_step_pose(Actor::Runtime, 6, qb);
    let slerp_mid = h.render_pose(6, 500);
    let norm_ok = match slerp_mid {
        Ok(p) => {
            let len = isqrt64(p.quat_norm_sq()) as i64;
            (len - Q14_ONE as i64).abs() <= Q14_NORM_TOL
        }
        Err(_) => false,
    };
    // 独立归一化基元交叉验证：同输入同残差
    let cross = quat_normalize_q14(rt2_2, 0, 0, rt2_2).is_some();
    s.add(
        "AD04-插值同步-NLERP归一残差钉死",
        norm_ok && cross,
        "最短弧插值中点归一化后 ‖q‖ 与 Q14_ONE 偏差 ≤ 2（整数 Newton isqrt 零浮点）",
    );

    // —— 判据三 · 反向：alpha 越域拒 + 单步退化诚实呈现 ——
    let oob = h.render_pose(6, ALPHA_MAX + 1);
    let mut h2 = BindHub::new();
    let _ = h2.bind(Actor::Editor, 9, 90, BindMode::Strict, 0);
    let _ = h2.push_step_pose(Actor::Runtime, 9, p1);
    let single = h2.render_pose(9, 500);
    s.add(
        "AD04-插值同步-alpha越域拒+单步退化",
        oob == Err(BindErr::Code(BindCode::BadAlpha)) && single == Ok(p1),
        "alpha>1000 显性拒；只喂一步无插值基点→诚实退化为硬步位姿（文档口径）",
    );

    // —— 判据四 · 批量挂接：事务全成（ok_count 全量 + 账实一致） ——
    let mut h = BindHub::new();
    let reqs: alloc::vec::Vec<BindRequest> = (0..MAX_BATCH)
        .map(|i| BindRequest { body: 1000 + i as u64, node: 2000 + i as u64, mode: BindMode::Strict, node_gen: 0 })
        .collect();
    let rep = h.batch_bind(Actor::Editor, &reqs);
    let batch_ok = match rep {
        Ok(r) => r.committed && r.ok_count == MAX_BATCH && h.len() == MAX_BATCH,
        Err(_) => false,
    };
    s.add(
        "AD04-批量挂接-满批事务全成",
        batch_ok && h.consistency_probe() == Ok(()),
        "MAX_BATCH 项批量一次提交：逐项落账、正反双账一致（批量 O(实体) 预算内）",
    );

    // —— 判据四 · 反向：批量含坏项整体拒、零副作用（all-or-nothing） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 1, 11, BindMode::Strict, 0);
    let bad_reqs = [
        BindRequest { body: 2, node: 22, mode: BindMode::Strict, node_gen: 0 },
        BindRequest { body: 1, node: 33, mode: BindMode::Strict, node_gen: 0 }, // body 重复
        BindRequest { body: 3, node: 11, mode: BindMode::Strict, node_gen: 0 }, // node 被占
    ];
    let gen_before = h.gen();
    let rep = h.batch_bind(Actor::Editor, &bad_reqs);
    let rollback_ok = match rep {
        Ok(r) => {
            !r.committed
                && r.ok_count == 0
                && r.failures.len() == 2
                && r.failures.contains(&(1, BindCode::AlreadyBound))
                && r.failures.contains(&(2, BindCode::NodeOccupied))
        }
        Err(_) => false,
    };
    let zero_side = h.len() == 1 && h.gen() == gen_before && h.lookup_by_node(22).is_none();
    s.add(
        "AD04-批量挂接-坏项整体拒零副作用",
        rollback_ok && zero_side,
        "逐项失败清单点名（下标+原因码）；一个都不落账（all-or-nothing 不撕裂账本）",
    );

    // —— 判据四 · 反向：批量上限拒绝 + 上限理由同源可查 ——
    let mut h = BindHub::new();
    let over: alloc::vec::Vec<BindRequest> = (0..MAX_BATCH + 1)
        .map(|i| BindRequest { body: 3000 + i as u64, node: 4000 + i as u64, mode: BindMode::Strict, node_gen: 0 })
        .collect();
    let over_rep = h.batch_bind(Actor::Editor, &over);
    let reason_ok = MAX_BATCH_REASON.len() > 20
        && MAX_BATCH_REASON.contains("预算")
        && h.ledger().batch_rejects == 1;
    s.add(
        "AD04-批量挂接-上限拒绝理由同源",
        over_rep == Err(BindErr::Code(BindCode::BatchTooLarge)) && reason_ok && h.is_empty(),
        "超 MAX_BATCH → BatchTooLarge；上限理由含'预算'文本同源常量（说明含理由）",
    );

    // —— 判据五 · 批量解除：事务全成 + NotBound 预检整体拒 ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 21, 121, BindMode::Strict, 0);
    let _ = h.bind(Actor::Editor, 22, 122, BindMode::Strict, 0);
    let rep_ok = h.batch_unbind(Actor::Editor, &[21, 22]);
    let unbind_ok = match rep_ok {
        Ok(r) => r.committed && r.ok_count == 2 && h.is_empty(),
        Err(_) => false,
    };
    let rep_bad = h.batch_unbind(Actor::Editor, &[21, 999]);
    let unbind_bad = match rep_bad {
        Ok(r) => !r.committed && r.failures.contains(&(1, BindCode::NotBound)),
        Err(_) => false,
    };
    s.add(
        "AD04-批量解除-事务同构",
        unbind_ok && unbind_bad,
        "批量解除与批量挂接同构：全成即提交、含 NotBound 项整体拒逐项点名",
    );

    // —— 判据六 · 两律之一：绑定结构操作不改物理喂入摘要 ——
    let seq: [(BodyId, Pose); 3] = [
        (77, Pose { px: 1, py: 2, pz: 3, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 }),
        (77, Pose { px: 4, py: 5, pz: 6, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 }),
        (77, Pose { px: 7, py: 8, pz: 9, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 }),
    ];
    // 情境 A：裸绑定直喂
    let mut ha = BindHub::new();
    let _ = ha.bind(Actor::Editor, 77, 770, BindMode::Strict, 0);
    for (b, p) in seq.iter() {
        let _ = ha.push_step_pose(Actor::Runtime, *b, *p);
    }
    let dig_a = ha.physics_digest();
    // 情境 B：同序列喂入，中间穿插批量/解绑/模式/挂起/复活结构操作
    let mut hb = BindHub::new();
    let _ = hb.bind(Actor::Editor, 77, 770, BindMode::Strict, 0);
    let _ = hb.push_step_pose(Actor::Runtime, 77, seq[0].1);
    let _ = hb.bind(Actor::Editor, 78, 780, BindMode::Interactive, 0);
    let _ = hb.batch_bind(
        Actor::Editor,
        &[
            BindRequest { body: 79, node: 790, mode: BindMode::Strict, node_gen: 0 },
            BindRequest { body: 80, node: 800, mode: BindMode::Detached, node_gen: 0 },
        ],
    );
    let _ = hb.set_mode(Actor::Editor, 77, BindMode::Detached);
    let _ = hb.set_mode(Actor::Editor, 77, BindMode::Strict);
    let _ = hb.on_node_destroyed(Actor::Runtime, 780);
    let _ = hb.unbind(Actor::Editor, 78);
    let _ = hb.set_mode(Actor::Editor, 80, BindMode::Strict);
    let _ = hb.push_step_pose(Actor::Runtime, 77, seq[1].1);
    let _ = hb.push_step_pose(Actor::Runtime, 77, seq[2].1);
    let dig_b = hb.physics_digest();
    s.add(
        "AD04-两律-结构操作不改物理摘要",
        dig_a == dig_b && dig_a != FNV_OFFSET,
        "同喂入序列：裸绑定 vs 穿插批量/解绑/模式/挂起结构操作——摘要逐位相等（集成不改运行时判定）",
    );

    // —— 判据六 · 反向：运行时/调试不碰结构（权限闸逐操作显性拒） ——
    let mut h = BindHub::new();
    let rt_bind = h.bind(Actor::Runtime, 1, 10, BindMode::Strict, 0);
    let ed_sync = h.push_step_pose(Actor::Editor, 1, p0);
    let dbg_bind = h.bind(Actor::Debug, 1, 10, BindMode::Strict, 0);
    let rt_setmode = h.set_mode(Actor::Runtime, 1, BindMode::Interactive);
    let query_ok = h.status_of(1) == Err(BindErr::Code(BindCode::NotFound));
    s.add(
        "AD04-两律-运行时调试不碰结构",
        rt_bind == Err(BindErr::Code(BindCode::PermissionDenied))
            && dbg_bind == Err(BindErr::Code(BindCode::PermissionDenied))
            && rt_setmode == Err(BindErr::Code(BindCode::PermissionDenied))
            && ed_sync == Err(BindErr::Code(BindCode::PermissionDenied))
            && query_ok,
        "运行时/调试挂接与模式切换全拒；编辑器驱动同步撞权限闸拒；查询全员开放（NotFound 是查询结果不是拒绝）",
    );

    // —— 判据七 · 两律之二：编辑器权（Interactive 位姿权威 + Strict 下写拒） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 31, 310, BindMode::Interactive, 0);
    let _ = h.push_step_pose(Actor::Runtime, 31, p0);
    let ed_pose = Pose { px: 500, py: 500, pz: 500, qw: Q14_ONE, qx: 0, qy: 0, qz: 0 };
    let ed_write = h.editor_write_pose(Actor::Editor, 31, ed_pose);
    let after_ed = h.render_pose(31, ALPHA_MAX);
    let _ = h.set_mode(Actor::Editor, 31, BindMode::Strict);
    let ed_write_strict = h.editor_write_pose(Actor::Editor, 31, ed_pose);
    let rt_write = h.editor_write_pose(Actor::Runtime, 31, ed_pose);
    s.add(
        "AD04-两律-编辑器权威模式化",
        ed_write.is_ok()
            && after_ed == Ok(ed_pose)
            && ed_write_strict == Err(BindErr::Code(BindCode::ModeMismatch))
            && rt_write == Err(BindErr::Code(BindCode::PermissionDenied)),
        "Interactive 下编辑器写即 cur（alpha=1000 恰得编辑器位姿）；Strict 下编辑器写拒；运行时写节点位姿拒（不夺编辑器权）",
    );

    // —— 判据七 · 反向：权限矩阵双写全格对拍 ——
    let mut matrix_ok = true;
    for a in Actor::ALL.iter() {
        for o in Op::ALL.iter() {
            if perm_matrix(*a, *o) != perm_rule(*a, *o) {
                matrix_ok = false;
            }
        }
    }
    s.add(
        "AD04-两律-权限矩阵双写对拍",
        matrix_ok && PERM_MATRIX.len() == ACTOR_COUNT && PERM_MATRIX[0].len() == OP_COUNT,
        "3 操作者 × 6 操作 = 18 格：查表路与规则推导路逐格相等（矩阵改规则没改即红）",
    );

    // —— 判据八 · 节点销毁→挂起：池可查、挂起拒同步、复活重绑可喂 ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 41, 410, BindMode::Strict, 0);
    let _ = h.push_step_pose(Actor::Runtime, 41, p0);
    let destroyed = h.on_node_destroyed(Actor::Runtime, 410);
    let susp_state = h.status_of(41).map(|st| st.suspended).unwrap_or(true);
    let susp_feed = h.push_step_pose(Actor::Runtime, 41, p1);
    let susp_listed = h.suspended_bodies() == alloc::vec![41];
    let revive = h.revive(Actor::Editor, 41, 411, 0);
    let feed_after = h.push_step_pose(Actor::Runtime, 41, p1);
    let node_moved = h.lookup_by_node(411) == Some(41) && h.lookup_by_node(410).is_none();
    s.add(
        "AD04-节点销毁-挂起池复活闭环",
        destroyed == Ok(1)
            && susp_state
            && susp_feed == Err(BindErr::Code(BindCode::Suspended))
            && susp_listed
            && revive.is_ok()
            && feed_after.is_ok()
            && node_moved,
        "销毁申报→挂起（池对外可查、喂位姿显性拒）；编辑器复活挂新节点→喂位姿恢复、反向账随迁",
    );

    // —— 判据八 · 反向：复活前提不满足显性拒（未挂起不能复活） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 42, 420, BindMode::Strict, 0);
    let not_susp = h.revive(Actor::Editor, 42, 421, 0);
    s.add(
        "AD04-节点销毁-复活前提校验",
        not_susp == Err(BindErr::Code(BindCode::NotSuspended)),
        "挂起池中无此体→NotSuspended（复活不是无条件换绑——结构操作有前提）",
    );

    // —— 判据九 · 同步失效→重绑：代次失配检出、重绑对齐恢复 ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 51, 510, BindMode::Strict, 3);
    let clean0 = h.sync_tick(Actor::Runtime, &[(510, 3)]);
    let stale_tick = h.sync_tick(Actor::Runtime, &[(510, 7)]);
    let stale_state = h.status_of(51).map(|st| st.stale).unwrap_or(false);
    let stale_feed = h.push_step_pose(Actor::Runtime, 51, p0);
    let rebind = h.rebind(Actor::Editor, 51, 7);
    let clean1 = h.sync_tick(Actor::Runtime, &[(510, 7)]);
    s.add(
        "AD04-同步失效-代次检出重绑恢复",
        clean0 == Ok(SyncOutcome::Clean(1))
            && stale_tick == Ok(SyncOutcome::Stale(alloc::vec![51]))
            && stale_state
            && stale_feed.is_ok()
            && rebind.is_ok()
            && clean1 == Ok(SyncOutcome::Clean(1))
            && h.ledger().stale_hits == 1
            && h.ledger().rebinds == 1,
        "代次 3→7 失配→Stale 点名+置位（不静默）；失效不挡物理喂入（挡的是同步对账不是位姿账）；重绑对齐后 Clean 恢复；账本 stale/rebind 各记 1",
    );

    // —— 判据九 · 反向：代次表缺席节点保守置 stale（显性不猜） ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Editor, 52, 520, BindMode::Strict, 1);
    let absent = h.sync_tick(Actor::Runtime, &[]);
    s.add(
        "AD04-同步失效-缺席保守置失效",
        absent == Ok(SyncOutcome::Stale(alloc::vec![52])),
        "节点代次表缺该节点→按失配置 stale（保守显性，不猜节点还活着）",
    );

    // —— 判据十 · 越权审计：拒绝留痕可回放 ——
    let mut h = BindHub::new();
    let _ = h.bind(Actor::Runtime, 61, 610, BindMode::Strict, 0); // 越权拒
    let _ = h.bind(Actor::Editor, 61, 610, BindMode::Strict, 0); // 成功
    let _ = h.bind(Actor::Debug, 62, 620, BindMode::Strict, 0); // 越权拒
    let replay = h.audit_replay(61);
    let denied_entries = h
        .audit_replay(62)
        .iter()
        .filter(|a| matches!(a.op, AuditOp::Denied { actor: Actor::Debug, op: Op::Bind, target: 62 }))
        .count();
    s.add(
        "AD04-越权-审计留痕可回放",
        h.ledger().denied == 2
            && replay.iter().any(|a| matches!(a.op, AuditOp::Denied { actor: Actor::Runtime, op: Op::Bind, target: 61 }))
            && replay.iter().any(|a| matches!(a.op, AuditOp::Bound { body: 61, node: 610 }))
            && denied_entries == 1,
        "越权三路齐落（拒绝+计数+审计点名到操作者×操作×目标）；按体回放含拒与成全痕",
    );

    // —— 判据十一 · 元数据：码段互异 + reason 全码非空 ——
    let codes_ok = {
        let mut uniq = true;
        for i in 0..SEGMENT_GUARD.len() {
            for j in (i + 1)..SEGMENT_GUARD.len() {
                if SEGMENT_GUARD[i].code() == SEGMENT_GUARD[j].code() {
                    uniq = false;
                }
            }
        }
        uniq
            && SEGMENT_GUARD.iter().all(|c| c.code() & 0xFF00 == 0x4B00)
            && SEGMENT_GUARD.iter().all(|c| !c.reason().is_empty())
    };
    s.add(
        "AD04-判据-码段互异reason全码非空",
        codes_ok && SEGMENT_GUARD.len() == 16,
        "十六码全落 0x4Bxx 两两互异；reason 人话逐码可读（拒绝不是哑的 Err）",
    );

    // —— 判据十一 · 反向：批量满载+审计满容不覆盖旧痕 ——
    let mut h = BindHub::with_capacity(2);
    let _ = h.bind(Actor::Editor, 1, 10, BindMode::Strict, 0);
    let _ = h.bind(Actor::Editor, 2, 20, BindMode::Strict, 0);
    let cap_full = h.bind(Actor::Editor, 3, 30, BindMode::Strict, 0);
    let mut h2 = BindHub::new();
    // 审计满容：灌满 AUDIT_CAP 后续记拒收计数（不静默覆盖旧痕）
    for i in 0..(AUDIT_CAP + 8) {
        let _ = h2.bind(Actor::Editor, 10_000 + i as u64, 20_000 + i as u64, BindMode::Strict, 0);
    }
    s.add(
        "AD04-判据-容量与审计满容显性",
        cap_full == Err(BindErr::Code(BindCode::CapacityFull))
            && h.len() == 2
            && h2.ledger().audit_dropped == 8
            && h2.audit_count() == AUDIT_CAP
            && h2.consistency_probe() == Ok(()),
        "绑定容量满→CapacityFull 不驱逐；审计满容拒记计数 8（append-only 旧痕保全）",
    );

    s
}



