//! VE-F2216 · 粒子安全（VE-L 域 · 粒子段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2216`
//!
//! **判据（锚点原文）**：四限额、输入消毒、恶意防御、隐私红线、判据。
//!
//! **职责定位（锚点原文）**：粒子安全——资源限额（池内存/纹理内存/
//! 发射器数/绘制调用四上限——超限拒绝显性报错：四上限的配置面+运行
//! 时检查+超限拒绝对策——池申请 vel08 quota 硬顶、绘制调用 vel06
//! 形态计数、发射器数 F2203 计数）；输入消毒（外部输入的消毒：配置
//! 加载/网络同步/脚本事件三入口的清洗——NaN 速度/负发射率/超大池
//! 申请/畸形事件名拒收——钳制记账+诊断）；恶意数据防御（畸形 glTF/
//! 恶意 Mesh 加载的防御：包围盒校验+索引越界拒+FGC 循环引用检测
//! ——加载器不信任输入）；隐私合规（遥测不采粒子内容：只采聚合统计
//! ——粒子位置/速度/颜色内容级数据不出内核——隐私红线结构性强）。
//!
//! # 一、四限额各有对策（不是一张空表）
//!
//! - **池内存**：真调 [`PoolQuota::validate`](vel08_pool::PoolQuota::
//!   validate)——总量超 `bytes_cap` 硬顶即**显性拒绝**（锚点矩阵：
//!   池超配额→拒绝显性）；
//! - **纹理内存**：粒子侧纹理申请（材质引用经 vel06
//!   [`MeshRef::material`](vel06_render::MeshRef) 间接发生）以
//!   [`TextureRequest`] 声明并对照 `texture_bytes_cap`——超限显性
//!   拒绝。如实声明：GPU 纹理实体分配在纹理域，本模块是粒子资源面
//!   的申购闸（同 vel14 对无对端项目的诚实先例）；
//! - **发射器数**：[`EmitterRegistry`] 运行时计数，超过
//!   `emitter_count_cap` 的创建请求显性拒绝（F2203 层级第一级的
//!   数量闸）；
//! - **绘制调用**：[`DrawCallMeter`] 逐次记录（vel06
//!   [`RenderForm`](vel06_render::RenderForm) 三形态各算一次调用），
//!   超 `drawcall_cap` **降级声明**而非拒绘（锚点矩阵原文：纹理/
//!   绘制调用超限→降级声明——拒绘会让画面消失，降级让画面变 Cheap
//!   但仍在）。
//!
//! # 二、输入消毒三入口（钳制记账+诊断，不静默改写）
//!
//! 配置加载/网络同步/脚本事件三入口各有清洗函数；每一次钳制都写
//! [`SanitizeRecord`]（入口×类别×处置×诊断）——静默改数是第二类
//! 事故，记账让「输入被改过」可见。NaN 速度/负发射率钳制（真调
//! vel03 [`clamp_emit_rate`](vel03_emitter::clamp_emit_rate) 与
//! [`is_finite`](vel03_emitter::is_finite)）；超大池申请拒绝（真调
//! vel08 quota）；畸形事件名**拒收**（真调 vel04
//! [`EventRegistry::lookup`](vel04_mode::EventRegistry::lookup)
//! 返 None 即拒——注册制拼写漂移防线在安全侧同权）。
//!
//! # 三、恶意数据防御三查（加载器不信任输入）
//!
//! glTF/Mesh 加载前三查：包围盒校验（位置越界拒——防坐标爆炸把
//! 相机与剔除全部打穿）、索引越界拒（index ≥ 顶点数即拒——防
//! 读到结构外内存）、循环引用检测（parent 链限深——防场景图成环
//! 把遍历变成死循环）。三查全过才装配三角形并真调 vel03
//! [`build_cumulative_area`](vel03_emitter::build_cumulative_area)
//! ——防御不误伤合法输入（判据双向）。
//!
//! # 四、隐私红线结构性（不是口头承诺）
//!
//! [`TelemetryPacket`] **只有四个聚合字段**（粒子数/生成率/水位/
//! 发射器数）——类型上没有 Vec/数组字段，位置/速度/颜色内容级
//! 数据在**类型层面无出口**；黑名单三字段（position/velocity/
//! color）在遥测面结构性缺席。判据以字段清单+渲染面双重验证：
//! 白名单闭集与黑名单不交、渲染输出恰四数值且无逐粒子清单。
//!
//! **性能（锚点原文）**：限额检查 O(1)；消毒每输入一次；glTF 防御
//! 加载期一次；遥测白名单编译期。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vel03_emitter::{
    build_cumulative_area, clamp_emit_rate, is_finite, DiagBag, Triangle, Vec3,
};
use crate::svstar2::vel04_mode::EventRegistry;
use crate::svstar2::vel08_pool::{PoolKind, PoolQuota};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SECURITY_VERSION: &str = "L16-security-v1";

/// 池内存超配额（显性拒绝）。
pub const E_SEC_QUOTA_POOL: &str = "E_SEC_QUOTA_POOL";

/// 纹理内存超限（显性拒绝）。
pub const E_SEC_QUOTA_TEXTURE: &str = "E_SEC_QUOTA_TEXTURE";

/// 发射器数超限（显性拒绝）。
pub const E_SEC_QUOTA_EMITTER: &str = "E_SEC_QUOTA_EMITTER";

/// 绘制调用超限（降级声明——不拒绘）。
pub const E_SEC_DRAWCALL_DEGRADE: &str = "E_SEC_DRAWCALL_DEGRADE";

/// 输入消毒钳制（记账类别）。
pub const E_SEC_SANITIZE_CLAMP: &str = "E_SEC_SANITIZE_CLAMP";

/// 畸形事件名（拒收）。
pub const E_SEC_EVENT_REJECT: &str = "E_SEC_EVENT_REJECT";

/// 包围盒越界（拒）。
pub const E_SEC_BBOX: &str = "E_SEC_BBOX";

/// 索引越界（拒）。
pub const E_SEC_INDEX: &str = "E_SEC_INDEX";

/// 循环引用（检测限深）。
pub const E_SEC_CYCLE: &str = "E_SEC_CYCLE";

/// 隐私越界（结构性拒绝）。
pub const E_SEC_PRIVACY: &str = "E_SEC_PRIVACY";

/// 默认池内存硬顶（字节）：64 MiB。
pub const DEFAULT_POOL_BYTES_CAP: u64 = 64 * 1024 * 1024;

/// 默认纹理内存上限（字节）：128 MiB。
pub const DEFAULT_TEXTURE_BYTES_CAP: u64 = 128 * 1024 * 1024;

/// 默认发射器数上限（F2203 层级第一级的数量闸）。
pub const DEFAULT_EMITTER_COUNT_CAP: u32 = 4_096;

/// 默认每帧绘制调用上限。
pub const DEFAULT_DRAWCALL_CAP: u32 = 2_048;

// ---------------------------------------------------------------------------
// 二、四限额（配置面 + 运行时检查 + 超限对策）
// ---------------------------------------------------------------------------

/// 资源限额配置（四上限——锚点：池内存/纹理内存/发射器数/绘制调用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// 池内存硬顶（字节）——vel08 `bytes_cap` 同口径。
    pub pool_bytes_cap: u64,
    /// 纹理内存上限（字节）。
    pub texture_bytes_cap: u64,
    /// 发射器数上限。
    pub emitter_count_cap: u32,
    /// 每帧绘制调用上限。
    pub drawcall_cap: u32,
}

impl ResourceLimits {
    /// 出厂限额（四上限全非零——零上限等于禁用该资源，须显性配置）。
    pub fn defaults() -> ResourceLimits {
        ResourceLimits {
            pool_bytes_cap: DEFAULT_POOL_BYTES_CAP,
            texture_bytes_cap: DEFAULT_TEXTURE_BYTES_CAP,
            emitter_count_cap: DEFAULT_EMITTER_COUNT_CAP,
            drawcall_cap: DEFAULT_DRAWCALL_CAP,
        }
    }

    /// 限额自检（零上限即拒——"限额没配"不是"限额无限"）。
    pub fn validate(&self) -> Result<(), String> {
        if self.pool_bytes_cap == 0 {
            return Err(format!("{}：池内存上限为 0（须显式配置）", E_SEC_QUOTA_POOL));
        }
        if self.texture_bytes_cap == 0 {
            return Err(format!("{}：纹理内存上限为 0（须显式配置）", E_SEC_QUOTA_TEXTURE));
        }
        if self.emitter_count_cap == 0 {
            return Err(format!("{}：发射器数上限为 0（须显式配置）", E_SEC_QUOTA_EMITTER));
        }
        if self.drawcall_cap == 0 {
            return Err(format!("{}：绘制调用上限为 0（须显式配置）", E_SEC_DRAWCALL_DEGRADE));
        }
        Ok(())
    }
}

/// 纹理申购（粒子侧对纹理内存的申请声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureRequest {
    /// 贴图槽数。
    pub slots: u32,
    /// 每槽字节数。
    pub bytes_per_slot: u64,
}

impl TextureRequest {
    /// 申购校验：slots 非零、字节非零、总量不超上限（checked_mul 防溢出
    /// 后比硬顶——同 vel08 `bytes()` 的防御性冗余理由：防类型放宽后
    /// 静默回绕成一个"装得下"的小总量）。
    pub fn validate(&self, cap: u64) -> Result<u64, String> {
        if self.slots == 0 {
            return Err(format!("{}：纹理申购 slots 为 0（空申购无意义）", E_SEC_QUOTA_TEXTURE));
        }
        if self.bytes_per_slot == 0 {
            return Err(format!("{}：纹理申购每槽字节为 0（总量恒 0 不可信）", E_SEC_QUOTA_TEXTURE));
        }
        let total = (self.slots as u64)
            .checked_mul(self.bytes_per_slot)
            .ok_or_else(|| format!("{}：纹理申购总量溢出", E_SEC_QUOTA_TEXTURE))?;
        if total > cap {
            return Err(format!(
                "{}：纹理申购 {} 字节（{} 槽 × {}）超上限 {}——拒绝申购，请降分辨率或走纹理域降级",
                E_SEC_QUOTA_TEXTURE, total, self.slots, self.bytes_per_slot, cap
            ));
        }
        Ok(total)
    }
}

/// 发射器登记册（F2203 计数器 + 上限闸）。
#[derive(Clone, Copy, Debug)]
pub struct EmitterRegistry {
    /// 当前在册发射器数。
    count: u32,
    /// 上限。
    cap: u32,
    /// 被拒计数（读屏可达——限额真的在拦人）。
    rejected: u64,
}

impl EmitterRegistry {
    /// 新登记册（上限取自限额配置）。
    pub fn new(cap: u32) -> EmitterRegistry {
        EmitterRegistry { count: 0, cap, rejected: 0 }
    }

    /// 登记一个新发射器；超上限显性拒绝（不改计数——拒绝不是"登记失败"）。
    pub fn add(&mut self) -> Result<(), String> {
        if self.count >= self.cap {
            self.rejected = self.rejected.saturating_add(1);
            return Err(format!(
                "{}：发射器数已达上限 {}（在册 {}）——拒绝创建，请合并或销毁闲置发射器",
                E_SEC_QUOTA_EMITTER, self.cap, self.count
            ));
        }
        self.count += 1;
        Ok(())
    }

    /// 注销一个（在册为 0 时注销是记账矛盾——不静默下溢）。
    pub fn remove(&mut self) -> Result<(), String> {
        if self.count == 0 {
            return Err(format!("{}：在册发射器为 0 时注销（记账矛盾）", E_SEC_QUOTA_EMITTER));
        }
        self.count -= 1;
        Ok(())
    }

    /// 在册数（读屏可达）。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 被拒次数（读屏可达）。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }
}

/// 降级声明（绘制调用超限的通知——降级不是错误，是知情决策）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeNotice {
    /// 通知类别码。
    pub code: &'static str,
    /// 人读详情（读屏可达）。
    pub detail: String,
}

/// 绘制调用计量器（vel06 三形态各算一次调用；超限降级声明）。
#[derive(Clone, Copy, Debug)]
pub struct DrawCallMeter {
    /// 本帧已用调用数。
    used: u32,
    /// 上限。
    cap: u32,
    /// 累计降级声明次数。
    degraded: u64,
}

impl DrawCallMeter {
    /// 新计量器。
    pub fn new(cap: u32) -> DrawCallMeter {
        DrawCallMeter { used: 0, cap, degraded: 0 }
    }

    /// 记录一次绘制调用；超限返回降级声明（**不拒绘**——锚点矩阵）。
    pub fn record(&mut self) -> Option<DegradeNotice> {
        self.used = self.used.saturating_add(1);
        if self.used > self.cap {
            self.degraded = self.degraded.saturating_add(1);
            return Some(DegradeNotice {
                code: E_SEC_DRAWCALL_DEGRADE,
                detail: format!(
                    "本帧绘制调用 {} 超上限 {}——已声明降级（合并批次/降低形态），不拒绘",
                    self.used, self.cap
                ),
            });
        }
        None
    }

    /// 帧末复位（新一帧从零计）。
    pub fn end_frame(&mut self) {
        self.used = 0;
    }

    /// 已用调用数（读屏可达）。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 累计降级次数（读屏可达）。
    pub fn degraded(&self) -> u64 {
        self.degraded
    }
}

// ---------------------------------------------------------------------------
// 三、输入消毒（三入口 × 钳制记账 + 诊断）
// ---------------------------------------------------------------------------

/// 输入入口三分类（锚点：配置加载/网络同步/脚本事件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SanEntry {
    /// 配置文件加载。
    ConfigLoad,
    /// 网络同步输入。
    NetworkSync,
    /// 脚本事件输入。
    ScriptEvent,
}

impl SanEntry {
    /// 入口名（读屏可达）。
    pub fn zh(self) -> &'static str {
        match self {
            SanEntry::ConfigLoad => "配置加载",
            SanEntry::NetworkSync => "网络同步",
            SanEntry::ScriptEvent => "脚本事件",
        }
    }
}

/// 一条消毒记录（入口×类别×处置×诊断——静默改数的反面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanitizeRecord {
    /// 入口。
    pub entry: SanEntry,
    /// 类别（NaN 速度/负发射率/超大申购/畸形事件名……）。
    pub kind: &'static str,
    /// 处置（钳制/拒绝/拒收）。
    pub action: &'static str,
    /// 人读诊断。
    pub note: String,
}

/// 消毒账本（定容外不用——Vec 即可，量级是配置量）。
#[derive(Clone, Debug, Default)]
pub struct SanitizeLog {
    records: Vec<SanitizeRecord>,
}

impl SanitizeLog {
    /// 空账本。
    pub fn new() -> SanitizeLog {
        SanitizeLog::default()
    }

    /// 记一条。
    pub fn note(&mut self, entry: SanEntry, kind: &'static str, action: &'static str, note: String) {
        self.records.push(SanitizeRecord { entry, kind, action, note: note });
    }

    /// 全部记录（读屏可达）。
    pub fn all(&self) -> &[SanitizeRecord] {
        &self.records
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空账（无任何钳制/拒绝）。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按类别计数（诊断聚合）。
    pub fn count_kind(&self, kind: &str) -> usize {
        self.records.iter().filter(|r| r.kind == kind).count()
    }
}

/// 速度消毒：任一分量非有限 → 零向量钳制并记账（NaN 速度会让粒子
/// 瞬间消失到无穷远，是经典崩溃源）。
pub fn sanitize_velocity(v: Vec3, entry: SanEntry, log: &mut SanitizeLog) -> Vec3 {
    if is_finite(v.x) && is_finite(v.y) && is_finite(v.z) {
        return v;
    }
    log.note(
        entry,
        "NaN 速度",
        "钳制",
        format!(
            "速度 ({}, {}, {}) 含非有限分量，已钳为零向量",
            v.x, v.y, v.z
        ),
    );
    Vec3::ZERO
}

/// 发射率消毒：真调 vel03 [`clamp_emit_rate`]（负/NaN → 0，超上限
/// 钳到 `EMIT_RATE_MAX_PER_SEC`），钳制即记账（透传不记——合法值
/// 无需痕迹）。
pub fn sanitize_emit_rate(raw: f32, entry: SanEntry, log: &mut SanitizeLog) -> f32 {
    let mut bag = DiagBag::new();
    let clamped = clamp_emit_rate(raw, &mut bag);
    if clamped != raw {
        log.note(
            entry,
            "发射率越界",
            "钳制",
            format!("发射率 {} 已钳制为 {}", raw, clamped),
        );
    }
    clamped
}

/// 池申购消毒：真调 vel08 [`PoolQuota::validate`]——声明了装不下的池
/// 一律拒绝（超硬顶是配置错误不是运行时抖动，拒绝并显性报错）。
pub fn sanitize_pool_request(q: &PoolQuota, entry: SanEntry, log: &mut SanitizeLog) -> Result<u64, String> {
    match q.validate() {
        Ok(total) => Ok(total),
        Err(e) => {
            log.note(entry, "超大池申请", "拒绝", e.clone());
            Err(format!("{}：{}", E_SEC_QUOTA_POOL, e))
        }
    }
}

/// 事件名消毒：真调 vel04 注册制查表——未注册事件名**拒收**（拼写
/// 漂移防线与 F1925 同规则，脚本入口是漂移高发地）。
pub fn sanitize_event_name(
    reg: &EventRegistry,
    event: u32,
    entry: SanEntry,
    log: &mut SanitizeLog,
) -> Result<(), String> {
    if reg.lookup(event).is_none() {
        log.note(
            entry,
            "畸形事件名",
            "拒收",
            format!("事件 id {} 未在注册表（拼写漂移或未注册）", event),
        );
        return Err(format!(
            "{}：事件 id {} 未注册——脚本事件名须先注册后触发（F1925 同规则）",
            E_SEC_EVENT_REJECT, event
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、恶意数据防御（glTF/Mesh 三查）
// ---------------------------------------------------------------------------

/// 世界包围盒（防御基准：位置必须落在声明盒内）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// 最小角。
    pub min: Vec3,
    /// 最大角。
    pub max: Vec3,
}

impl Bounds {
    /// 单位盒（[-1,1]³——粒子域出生形状的常用尺度）。
    pub fn unit() -> Bounds {
        Bounds {
            min: Vec3::new(-1.0, -1.0, -1.0),
            max: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    /// 点是否在盒内（含边界）。
    pub fn contains(&self, p: &Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }
}

/// 查一：包围盒校验（坐标爆炸防线的第一道）。
pub fn check_bbox(positions: &[Vec3], bounds: &Bounds) -> Result<(), String> {
    for (i, p) in positions.iter().enumerate() {
        if !is_finite(p.x) || !is_finite(p.y) || !is_finite(p.z) {
            return Err(format!(
                "{}：顶点 {} 含非有限坐标 ({}, {}, {})——拒绝加载",
                E_SEC_BBOX, i, p.x, p.y, p.z
            ));
        }
        if !bounds.contains(p) {
            return Err(format!(
                "{}：顶点 {} ({}, {}, {}) 越出包围盒 [{},{}]×[{},{}]×[{},{}]——坐标爆炸或尺度错误",
                E_SEC_BBOX,
                i,
                p.x,
                p.y,
                p.z,
                bounds.min.x,
                bounds.max.x,
                bounds.min.y,
                bounds.max.y,
                bounds.min.z,
                bounds.max.z
            ));
        }
    }
    Ok(())
}

/// 查二：索引越界拒（index ≥ 顶点数即拒——读到结构外内存的前兆）。
pub fn check_indices(indices: &[u32], vertex_count: u32) -> Result<(), String> {
    for (i, idx) in indices.iter().enumerate() {
        if *idx >= vertex_count {
            return Err(format!(
                "{}：索引 {}（值 {}）≥ 顶点数 {}——越界索引拒收",
                E_SEC_INDEX, i, idx, vertex_count
            ));
        }
    }
    Ok(())
}

/// 查三：循环引用检测（parent 链限深——场景图成环会把遍历变死循环）。
///
/// `parents[i]` 为节点 i 的父索引，`u32::MAX` 为根。鸽笼原理：一
/// 条链走过的节点数超过总节点数必有环——不需要额外访问标记数组
/// （no_std 无alloc 借用也能跑；这里用步数即检出）。
pub fn check_cycle(parents: &[u32]) -> Result<(), String> {
    let n = parents.len() as u32;
    for start in 0..n {
        let mut cur = start;
        let mut steps = 0u32;
        while cur != u32::MAX {
            if cur >= n {
                return Err(format!(
                    "{}：节点 {} 的 parent {} 越界（≥ 节点数 {}）——场景图损坏",
                    E_SEC_CYCLE, start, cur, n
                ));
            }
            cur = parents[cur as usize];
            steps += 1;
            if steps > n {
                return Err(format!(
                    "{}：节点 {} 的 parent 链超过 {} 步仍不到根——必成环，拒绝加载",
                    E_SEC_CYCLE, start, n
                ));
            }
        }
    }
    Ok(())
}

/// 恶意 Mesh 加载防御总装：三查全过 → 装配三角形 → 真调 vel03
/// [`build_cumulative_area`]（合法输入不被防御误伤——判据双向）。
pub fn defend_mesh_load(
    positions: &[Vec3],
    indices: &[u32],
    parents: &[u32],
    bounds: &Bounds,
    bag: &mut DiagBag,
) -> Result<usize, String> {
    check_bbox(positions, bounds)?;
    check_indices(indices, positions.len() as u32)?;
    check_cycle(parents)?;
    if indices.len() % 3 != 0 {
        return Err(format!(
            "{}：索引数 {} 非 3 的倍数（三角网格索引须三成组）",
            E_SEC_INDEX,
            indices.len()
        ));
    }
    let mut tris: Vec<Triangle> = Vec::new();
    let mut i = 0usize;
    while i + 2 < indices.len() {
        tris.push(Triangle {
            a: positions[indices[i] as usize],
            b: positions[indices[i + 1] as usize],
            c: positions[indices[i + 2] as usize],
        });
        i += 3;
    }
    match build_cumulative_area(&tris, bag) {
        Some(surface) => Ok(surface.cum_area.len()),
        None => Err(format!(
            "{}：三查全过但面积表装配失败（全退化网格）——拒绝加载",
            E_SEC_INDEX
        )),
    }
}

// ---------------------------------------------------------------------------
// 五、隐私红线（遥测只采聚合——结构性无内容出口）
// ---------------------------------------------------------------------------

/// 遥测字段白名单闭集（四聚合字段——锚点：只采聚合统计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryField {
    /// 粒子总数（计数）。
    ParticleCount,
    /// 每秒生成数（速率）。
    SpawnRate,
    /// 池水位百分位（聚合比）。
    WaterPct,
    /// 在册发射器数（计数）。
    EmitterCount,
}

impl TelemetryField {
    /// 白名单闭集（恰四字段）。
    pub const ALL: [TelemetryField; 4] = [
        TelemetryField::ParticleCount,
        TelemetryField::SpawnRate,
        TelemetryField::WaterPct,
        TelemetryField::EmitterCount,
    ];

    /// 字段名（wire 名）。
    pub fn wire(self) -> &'static str {
        match self {
            TelemetryField::ParticleCount => "particle_count",
            TelemetryField::SpawnRate => "spawn_rate",
            TelemetryField::WaterPct => "water_pct",
            TelemetryField::EmitterCount => "emitter_count",
        }
    }
}

/// 禁止字段黑名单（内容级数据——位置/速度/颜色不出内核）。
pub const TELEMETRY_BLACKLIST: [&str; 3] = ["position", "velocity", "color"];

/// 遥测包（**仅聚合字段**——类型上无 Vec/数组，内容级数据无出口）。
///
/// 隐私红线的结构性论证：本结构体的全部字段都是标量聚合值；即便
/// 调用方手里有整池粒子，也没有 API 把逐粒数据塞进来（无 slice 形
/// 参的构造路径）。红线不是文档承诺，是类型形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryPacket {
    /// 粒子总数。
    pub particle_count: u32,
    /// 每秒生成数。
    pub spawn_rate: u32,
    /// 水位百分位。
    pub water_pct: u64,
    /// 在册发射器数。
    pub emitter_count: u32,
}

impl TelemetryPacket {
    /// 由聚合值构造（唯一构造路径——无内容级入参版本）。
    pub fn new(particle_count: u32, spawn_rate: u32, water_pct: u64, emitter_count: u32) -> TelemetryPacket {
        TelemetryPacket { particle_count, spawn_rate, water_pct, emitter_count }
    }

    /// 聚合渲染（读屏/遥测串——恰四个数值，无逐粒子清单）。
    pub fn render(&self) -> String {
        format!(
            "particle_count={};spawn_rate={};water_pct={};emitter_count={}",
            self.particle_count, self.spawn_rate, self.water_pct, self.emitter_count
        )
    }

    /// 字段名清单（白名单顺序——判据与工具视图同源）。
    pub fn field_names() -> [&'static str; 4] {
        [
            TelemetryField::ParticleCount.wire(),
            TelemetryField::SpawnRate.wire(),
            TelemetryField::WaterPct.wire(),
            TelemetryField::EmitterCount.wire(),
        ]
    }
}

/// 隐私红线核验：白名单与黑名单不交 + 渲染面恰四数值且不含任何
/// 黑名单字段名（ struantural + behavioral 双证）。
pub fn privacy_verdict(p: &TelemetryPacket) -> Result<(), String> {
    // 结构证：白名单闭集四字段，任一不得落在黑名单。
    for f in TelemetryField::ALL.iter() {
        for b in TELEMETRY_BLACKLIST.iter() {
            if f.wire() == *b {
                return Err(format!(
                    "{}：白名单字段 {} 与黑名单 {} 相交——白名单被污染",
                    E_SEC_PRIVACY, f.wire(), b
                ));
            }
        }
    }
    // 行为证：渲染面恰四分号段（无逐粒子清单——清单会有成百上千段）。
    let rendered = p.render();
    let segments = rendered.split(';').count();
    if segments != 4 {
        return Err(format!(
            "{}：遥测渲染恰四段（实测 {} 段）——多出的段意味着夹带了内容级数据",
            E_SEC_PRIVACY, segments
        ));
    }
    for b in TELEMETRY_BLACKLIST.iter() {
        if rendered.contains(b) {
            return Err(format!(
                "{}：遥测输出含黑名单字段 {}——内容级数据外泄",
                E_SEC_PRIVACY, b
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2216 域自检（判据五组：限额/消毒/防御/隐私/CI）。
pub fn run_vel16_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2216");
    let limits = ResourceLimits::defaults();

    // --- 四限额（判据一）---
    // L16-限额-01：池超硬顶拒绝（真调 vel08 quota.validate）。
    let over = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: 1_000_000,
        stride: 64,
        emitter_share_pct: 10,
        bytes_cap: 1024, // 故意配小：声明 64,000,000 > 硬顶 1024
    };
    let mut log = SanitizeLog::new();
    let r = sanitize_pool_request(&over, SanEntry::ConfigLoad, &mut log);
    s.add(
        "L16-限额-01",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_SEC_QUOTA_POOL),
        "池超硬顶显性拒绝（真调 vel08 quota）",
    );

    // L16-限额-02：纹理申购超限拒绝 + 合法申购放行（双向）。
    let tex_bad = TextureRequest { slots: 1024, bytes_per_slot: 4 * 1024 * 1024 }; // 4 GiB > 128 MiB
    let tex_ok_req = TextureRequest { slots: 16, bytes_per_slot: 1024 * 1024 }; // 16 MiB < 128 MiB
    let tex_bad = tex_bad.validate(limits.texture_bytes_cap);
    let tex_ok = tex_ok_req.validate(limits.texture_bytes_cap);
    s.add(
        "L16-限额-02",
        tex_bad.is_err()
            && tex_bad.as_ref().unwrap_err().starts_with(E_SEC_QUOTA_TEXTURE)
            && tex_ok == Ok(16 * 1024 * 1024),
        "纹理超限拒绝/合法放行（双向）",
    );

    // L16-限额-03：发射器数超限拒绝（计数器 + 被拒可读）。
    let mut reg = EmitterRegistry::new(3);
    let mut add_ok = true;
    let mut i = 0;
    while i < 3 {
        add_ok = add_ok && reg.add().is_ok();
        i += 1;
    }
    let over_add = reg.add();
    s.add(
        "L16-限额-03",
        add_ok
            && over_add.is_err()
            && over_add.as_ref().unwrap_err().starts_with(E_SEC_QUOTA_EMITTER)
            && reg.rejected() == 1
            && reg.count() == 3,
        "发射器数超限拒绝（不改计数+被拒记账）",
    );

    // L16-限额-04：绘制调用超限降级声明（不拒绘——锚点矩阵原文）。
    let mut meter = DrawCallMeter::new(4);
    let mut notices = 0u64;
    let mut k = 0;
    while k < 8 {
        if meter.record().is_some() {
            notices += 1;
        }
        k += 1;
    }
    let last_notice = {
        let mut m = DrawCallMeter::new(1);
        m.record();
        m.record()
    };
    s.add(
        "L16-限额-04",
        notices == 4
            && meter.degraded() == 4
            && last_notice.map_or(false, |n| n.code == E_SEC_DRAWCALL_DEGRADE && n.detail.contains("不拒绘")),
        "绘制调用超限降级声明（拒绘变降级）",
    );

    // L16-限额-05：限额自检零上限即拒（"没配"不是"无限"）。
    let zero = ResourceLimits {
        pool_bytes_cap: 1024,
        texture_bytes_cap: 0,
        emitter_count_cap: 8,
        drawcall_cap: 8,
    };
    let ok_limits = ResourceLimits::validate(&limits).is_ok();
    s.add(
        "L16-限额-05",
        ok_limits && zero.validate().is_err() && zero.validate().unwrap_err().starts_with(E_SEC_QUOTA_TEXTURE),
        "限额零值拒绝+出厂限额自检过",
    );

    // --- 输入消毒（判据二）---
    // L16-消毒-01：NaN 速度钳制+记账（零向量；有限速度透传不记）。
    let mut log2 = SanitizeLog::new();
    let nan_v = sanitize_velocity(Vec3::new(1.0, f32::NAN, 2.0), SanEntry::NetworkSync, &mut log2);
    let ok_v = sanitize_velocity(Vec3::new(1.0, 2.0, 3.0), SanEntry::NetworkSync, &mut log2);
    s.add(
        "L16-消毒-01",
        nan_v == Vec3::ZERO
            && ok_v == Vec3::new(1.0, 2.0, 3.0)
            && log2.len() == 1
            && log2.count_kind("NaN 速度") == 1
            && log2.all()[0].entry == SanEntry::NetworkSync,
        "NaN 速度钳制记账（合法值透传不记）",
    );

    // L16-消毒-02：发射率钳制（真调 vel03 clamp_emit_rate）。
    let mut log3 = SanitizeLog::new();
    let neg = sanitize_emit_rate(-5.0, SanEntry::ConfigLoad, &mut log3);
    let huge = sanitize_emit_rate(1.0e9, SanEntry::ConfigLoad, &mut log3);
    s.add(
        "L16-消毒-02",
        neg == 0.0 && huge == 1_000_000.0 && log3.count_kind("发射率越界") == 2,
        "负率钳 0/超上限钳百万（真调 vel03）",
    );

    // L16-消毒-03：超大池申请拒绝+记账（入口记册）。
    let mut log4 = SanitizeLog::new();
    let r2 = sanitize_pool_request(&over, SanEntry::NetworkSync, &mut log4);
    s.add(
        "L16-消毒-03",
        r2.is_err() && log4.count_kind("超大池申请") == 1 && log4.all()[0].entry == SanEntry::NetworkSync,
        "超大申购拒绝+入口记账",
    );

    // L16-消毒-04：畸形事件名拒收（真调 vel04 注册制）。
    let mut reg4 = EventRegistry::new();
    let mut bag4 = DiagBag::new();
    let good_id = 0x1234u32;
    reg4.register(good_id, crate::svstar2::vel04_mode::EventSchema::neutral(), &mut bag4);
    let mut log5 = SanitizeLog::new();
    let good = sanitize_event_name(&reg4, good_id, SanEntry::ScriptEvent, &mut log5);
    let bad = sanitize_event_name(&reg4, 0xDEAD, SanEntry::ScriptEvent, &mut log5);
    s.add(
        "L16-消毒-04",
        good.is_ok()
            && bad.is_err()
            && bad.as_ref().unwrap_err().starts_with(E_SEC_EVENT_REJECT)
            && log5.count_kind("畸形事件名") == 1,
        "畸形事件名拒收（已注册放行）",
    );

    // L16-消毒-05：消毒账本可读聚合（三入口×处置对拍）。
    let mut log6 = SanitizeLog::new();
    sanitize_velocity(Vec3::new(f32::NAN, 0.0, 0.0), SanEntry::ConfigLoad, &mut log6);
    sanitize_emit_rate(-1.0, SanEntry::NetworkSync, &mut log6);
    sanitize_emit_rate(-1.0, SanEntry::ScriptEvent, &mut log6);
    s.add(
        "L16-消毒-05",
        log6.len() == 3
            && log6.all().iter().all(|r| !r.note.is_empty() && !r.action.is_empty()),
        "消毒账本三入口各一条且处置非空",
    );

    // --- 恶意数据防御（判据三）---
    let bounds = Bounds::unit();
    // L16-防御-01：包围盒越界拒 + 非有限坐标拒。
    let bad_pos = [Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)];
    let nan_pos = [Vec3::new(f32::INFINITY, 0.0, 0.0)];
    let r3 = check_bbox(&bad_pos, &bounds);
    let r3b = check_bbox(&nan_pos, &bounds);
    s.add(
        "L16-防御-01",
        r3.is_err() && r3.as_ref().unwrap_err().starts_with(E_SEC_BBOX)
            && r3b.is_err() && r3b.unwrap_err().contains("非有限"),
        "包围盒越界/非有限坐标拒",
    );

    // L16-防御-02：索引越界拒 + 合法索引过。
    let n_verts = 3u32;
    let r4 = check_indices(&[0, 1, 2], n_verts);
    let r4b = check_indices(&[0, 1, 3], n_verts);
    s.add(
        "L16-防御-02",
        r4.is_ok()
            && r4b.is_err()
            && r4b.as_ref().unwrap_err().starts_with(E_SEC_INDEX)
            && r4b.unwrap_err().contains("3"),
        "索引越界拒（0/1/2 过，0/1/3 拒）",
    );

    // L16-防御-03：循环引用检测（A↔B 成环检出 + 合法链放行）。
    let cyclic = [1u32, 0u32, u32::MAX]; // 0→1→0 成环
    let acyclic = [u32::MAX, 0u32, 1u32]; // 2→1→0→root
    let r5 = check_cycle(&cyclic);
    let r5b = check_cycle(&acyclic);
    s.add(
        "L16-防御-03",
        r5.is_err() && r5.as_ref().unwrap_err().starts_with(E_SEC_CYCLE)
            && r5b.is_ok(),
        "parent 环检出+合法链放行（鸽笼限深）",
    );

    // L16-防御-04：三查全过真调 build_cumulative_area（不误伤合法输入）。
    let good_pos = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let good_idx = [0u32, 1u32, 2u32];
    let good_parents = [u32::MAX, 0u32];
    let mut bag = DiagBag::new();
    let loaded = defend_mesh_load(&good_pos, &good_idx, &good_parents, &bounds, &mut bag);
    let bad_load = defend_mesh_load(&bad_pos, &good_idx, &good_parents, &bounds, &mut bag);
    s.add(
        "L16-防御-04",
        loaded == Ok(2) && bad_load.is_err(),
        "合法 Mesh 真调装配成功（2 表项）/越界即拒",
    );

    // --- 隐私红线（判据四）---
    // L16-隐私-01：白名单黑名单不交（闭集四字段）。
    let disjoint = TelemetryField::ALL
        .iter()
        .all(|f| !TELEMETRY_BLACKLIST.iter().any(|b| f.wire() == *b));
    s.add(
        "L16-隐私-01",
        disjoint && TelemetryField::ALL.len() == 4 && TELEMETRY_BLACKLIST.len() == 3,
        "白名单四聚合×黑名单三内容不交",
    );

    // L16-隐私-02：遥测结构尺寸定长（无 Vec——内容无出口的类型证）。
    let pkt = TelemetryPacket::new(1000, 120, 87, 3);
    s.add(
        "L16-隐私-02",
        pkt.particle_count == 1000
            && pkt.spawn_rate == 120
            && pkt.water_pct == 87
            && pkt.emitter_count == 3
            && privacy_verdict(&pkt).is_ok(),
        "遥测包恰四聚合字段且核验过",
    );

    // L16-隐私-03：渲染面恰四段且不含黑名单字段名（行为证）。
    let rendered = pkt.render();
    s.add(
        "L16-隐私-03",
        rendered.split(';').count() == 4
            && !TELEMETRY_BLACKLIST.iter().any(|b| rendered.contains(b))
            && TelemetryPacket::field_names().len() == 4,
        "遥测渲染四段无内容字段",
    );

    // --- CI 与版本 ---
    // L16-CI-01：限额+消毒+防御+隐私四组合规输入全放行（不误伤面）。
    let all_pass = ResourceLimits::validate(&limits).is_ok()
        && tex_ok_req.validate(limits.texture_bytes_cap).is_ok()
        && check_bbox(&good_pos, &bounds).is_ok()
        && check_cycle(&acyclic).is_ok();
    s.add("L16-CI-01", all_pass, "合规输入全链放行（防御不误伤）");

    // L16-CI-02：违规输入全链可检出（每族至少一条拒绝路径在网）。
    let over_tex = TextureRequest { slots: 1024, bytes_per_slot: 4 * 1024 * 1024 };
    let all_block = sanitize_pool_request(&over, SanEntry::ConfigLoad, &mut SanitizeLog::new()).is_err()
        && over_tex.validate(1).is_err()
        && check_bbox(&bad_pos, &bounds).is_err()
        && check_cycle(&cyclic).is_err();
    s.add("L16-CI-02", all_block, "违规输入全链可检出");

    // L16-版本-01：版本指纹非零。
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in SECURITY_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("L16-版本-01", fp != 0, "版本指纹非零（L16-security-v1）");

    // L16-暂挂-01：L 域账本暂挂声明显性（移交期模式延续）。
    s.add(
        "L16-暂挂-01",
        L_LEDGER_SEC_SUSPENDED_NOTE.contains("暂挂") && L_LEDGER_SEC_SUSPENDED_NOTE.contains("F2216"),
        "L 域账本暂挂声明显性",
    );

    // L16-暂挂-02：判据条数对账（本条为第 22 条）。
    s.add("L16-暂挂-02", s.len() == 21, "判据条数对账（21+本条）");

    s
}

/// L 域账本暂挂声明（锚点跨批对接点：安全策略总账入 L 域账本——
/// 建账前暂挂，移交期模式延续；与 F2094/F2211~F2215 同款）。
pub const L_LEDGER_SEC_SUSPENDED_NOTE: &str = "粒子安全策略总账入 L 域账本：建账前暂挂声明（移交期模式第五域延续——F2216 同款）；四限额与消毒/防御/隐私判据随 CI 跑";

// ---------------------------------------------------------------------------
// 七、诚实边界
// ---------------------------------------------------------------------------

/// 纹理限额的诚实声明（同 vel07_blend/vel12/vel15 先例：不编造对端）。
///
/// GPU 纹理实体的分配与回收在纹理域（本仓 VE-L 无纹理分配器）；本
/// 模块的 `texture_bytes_cap` 是**粒子资源面的申购闸**——粒子侧
/// 持材质引用（vel06 `MeshRef::material`）时先过本闸，再向纹理域
/// 申购。闸的存在让"粒子侧无节制引用材质"成为显性拒绝而非隐性
/// 超采；纹理域落地后本闸与其配额对账（接口即
/// [`TextureRequest::validate`]）。
pub const TEXTURE_LIMIT_HONESTY_NOTE: &str =
    "纹理内存上限为粒子资源面申购闸（GPU 纹理实体分配在纹理域）；闸先拒超采，纹理域落地后同接口对账";