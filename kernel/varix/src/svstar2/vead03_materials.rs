//! VE-F6003 · 物理材质与表面（VE-AD 域 · 物理组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6003`
//!
//! **判据（锚点原文）**：材质库、组合预览、安全默认、判据。
//!
//! **职责定位（锚点原文）**：材质库（摩擦、弹性、密度组合预设备案）与表面
//! 属性注册（材质组合的物理响应可预览，材质缺失有安全默认）；材质含组合
//! 实测矩阵——常用材质组合的物理响应实测备案；矩阵含冲突检测；冲突检测
//! 含上报；上报含责任段。错误路径：材质异常→默认兜底；组合未测→标注；
//! 越权改→审计。查库 O(1)。
//!
//! # 一、材质是**备案**不是感觉：每条参数有出处、每格矩阵有责任段
//!
//! 物理响应参数（摩擦/弹性）如果随手写死在碰撞代码里，调手感的人不知道
//! 改的是「备案过的物理」还是「某个下午的灵感」，出了争议无法追责。故本单
//! 把材质做成闭集库（[`MATERIAL_LIBRARY`]），把两两组合的响应做成**实测
//! 矩阵**（[`PAIR_MATRIX`]）：每格组合响应带 `measured` 备案位与 `owner`
//! 责任段——「橡胶对冰面摩擦 0.08」是谁测的、在哪一段代码管着，一格一行
//! 全部可查。冲突检测（[`detect_conflicts`]）扫的就是这格账：同组合两条
//! 备案超出容差即上报，上报单带责任段（锚点「上报含责任段」）。
//!
//! # 二、安全默认是**降级路径**不是默认值：兜底必须带标注
//!
//! 材质缺失时给「摩擦 0.5 弹性 0.3」不是重点——重点是调用方**必须知道**
//! 自己拿到的是兜底值而不是备案值，否则未备案材质悄悄混进物理结算，排查
//! 时没人怀疑它。[`SafeDefaultNote`] 就是这个标注：[`SurfaceRegistry`]
//! 对未注册表面返回默认材质并附注，预览端据此显示「未备案，默认兜底」。
//! 静默兜底比崩溃更危险——它把配置错误变成看起来正常的物理。
//!
//! # 三、查库 O(1)：id 直引，名字只在注册期解析
//!
//! 物理结算每步都要查材质，按名字线性扫表不可接受。库以 [`MaterialId`]
//! 直接索引（数组下标即 O(1)），名字→id 的解析只发生在表面注册期与配置
//! 加载期。注册表自身也是定容数组，越权/越容注册走显性错误码，不静默挤占。
//!
//! # 四、越权改→审计：变更留痕，谁改的查得到
//!
//! 材质备案与表面注册的每次写操作走 [`audit_push`]：操作码+目标+操作序号
//! （无墙钟环境用操作计数，F0221 先例）。审计簿满容拒收并计数——不静默
//! 覆盖旧痕。争议发生时按表面 id 回放全部变更，责任到段。
//!
//! ## 零 panic 面
//!
//! 全部查找走 `Option`/`Result`，矩阵按 (a,b) 归一化索引越界返回 `None`，
//! 无 `unwrap`/`expect`/切片直下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ===========================================================================
// 一、版本与诊断码（vead03 独占 0x40xx 段；0x3E=vad01 / 0x3F=vead02）
// ===========================================================================

/// 版本标识（家族格式）。
pub const PMAT_VERSION: &str = "AD03-pmat-v1";

/// 材质域诊断码（独占段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatCode(pub u16);

impl MatCode {
    /// 未知材质名（库外材质请求）。
    pub const MATERIAL_UNKNOWN: MatCode = MatCode(0x4001);
    /// 组合矩阵冲突（同组合备案超容差）。
    pub const MATRIX_CONFLICT: MatCode = MatCode(0x4002);
    /// 表面未注册（走安全默认并标注）。
    pub const SURFACE_UNREGISTERED: MatCode = MatCode(0x4003);
    /// 越权变更（审计拦截）。
    pub const AUDIT_DENIED: MatCode = MatCode(0x4004);
    /// 注册簿满容（拒绝并留痕，不静默挤占）。
    pub const REGISTRY_FULL: MatCode = MatCode(0x4005);

    /// 短码（面板/日志/读屏共用）。
    pub const fn code(self) -> u16 {
        self.0
    }
}

// ===========================================================================
// 二、材质库（判据一：摩擦/弹性/密度组合预设备案，id 直引 O(1)）
// ===========================================================================

/// 材质 id（数组直索引——查库 O(1) 的载体）。
pub type MaterialId = u8;

/// 一条材质备案。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PhysMaterial {
    /// 稳定名（注册期解析用）。
    pub name: &'static str,
    /// 摩擦系数（千分比 0..=1000，整数——确定性纪律的物理域落地）。
    pub friction_permille: u32,
    /// 弹性系数（千分比 0..=1000）。
    pub elasticity_permille: u32,
    /// 密度（kg/m³，整数）。
    pub density_kg_m3: u32,
}

/// 材质库（预设备案闭集；下标即 [`MaterialId`]）。
///
/// 数值口径：摩擦/弹性用千分比整数而非 f32——物理结算跨平台字节级确定
/// （F1619 纪律在物理域的延续），密度用整数 kg/m³。
pub const MATERIAL_LIBRARY: [PhysMaterial; 6] = [
    PhysMaterial { name: "rubber", friction_permille: 900, elasticity_permille: 850, density_kg_m3: 1100 },
    PhysMaterial { name: "steel", friction_permille: 250, elasticity_permille: 400, density_kg_m3: 7850 },
    PhysMaterial { name: "ice", friction_permille: 30, elasticity_permille: 100, density_kg_m3: 917 },
    PhysMaterial { name: "wood", friction_permille: 450, elasticity_permille: 350, density_kg_m3: 700 },
    PhysMaterial { name: "sand", friction_permille: 700, elasticity_permille: 50, density_kg_m3: 1600 },
    PhysMaterial { name: "glass", friction_permille: 200, elasticity_permille: 600, density_kg_m3: 2500 },
];

/// 按名查 id（注册期解析用；O(库)——结算路径不走这里）。
pub fn material_id_of(name: &str) -> Option<MaterialId> {
    let mut i = 0usize;
    while i < MATERIAL_LIBRARY.len() {
        if MATERIAL_LIBRARY[i].name == name {
            return Some(i as MaterialId);
        }
        i += 1;
    }
    None
}

/// 按 id 直引查材质（O(1)；越界 `None`）。
pub fn material_of(id: MaterialId) -> Option<&'static PhysMaterial> {
    MATERIAL_LIBRARY.get(id as usize)
}

// ===========================================================================
// 三、安全默认（判据三：材质缺失有安全默认——兜底必须带标注）
// ===========================================================================

/// 安全默认材质（库外兜底；中庸参数，无极值风险）。
pub const DEFAULT_MATERIAL: PhysMaterial = PhysMaterial {
    name: "default-safe",
    friction_permille: 500,
    elasticity_permille: 300,
    density_kg_m3: 1000,
};

/// 安全默认标注（兜底值与备案值在预览端可区分的唯一依据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SafeDefaultNote {
    /// 是否走了兜底。
    pub fallback: bool,
    /// 兜底原因（诊断码短码）。
    pub reason: u16,
}

/// 材质解析：库内 id 返回备案材质+无标注；异常（越界 id）返回安全默认+标注。
/// **静默兜底是禁手**——标注是本函数返回值的一部分，不是可选日志。
pub fn resolve_material(id: MaterialId) -> (PhysMaterial, SafeDefaultNote) {
    match material_of(id) {
        Some(m) => (*m, SafeDefaultNote { fallback: false, reason: 0 }),
        None => (
            DEFAULT_MATERIAL,
            SafeDefaultNote { fallback: true, reason: MatCode::MATERIAL_UNKNOWN.code() },
        ),
    }
}

// ===========================================================================
// 四、表面注册（注册期解析名字；结算期只按 id 查）
// ===========================================================================

/// 注册簿容量（定容；满容拒绝并留痕）。
pub const REGISTRY_CAP: usize = 64;

/// 表面注册条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceEntry {
    /// 表面 id（调用方命名空间内的稳定键）。
    pub surface_id: u64,
    /// 材质 id。
    pub material: MaterialId,
}

/// 表面注册簿：表面 → 材质。
pub struct SurfaceRegistry {
    entries: [Option<SurfaceEntry>; REGISTRY_CAP],
    count: usize,
    /// 满容拒收计数（不静默）。
    pub rejected: u32,
    /// 越权变更拦截计数。
    pub denied: u32,
}

impl SurfaceRegistry {
    /// 空注册簿。
    pub fn new() -> SurfaceRegistry {
        SurfaceRegistry {
            entries: [None; REGISTRY_CAP],
            count: 0,
            rejected: 0,
            denied: 0,
        }
    }

    /// 注册（重复 surface_id 视为越权改，拒绝并计审计；
    /// 满容拒绝并计数——两失败形态显性分账）。
    pub fn register(&mut self, surface_id: u64, material: MaterialId, audit: &mut AuditLog) -> Result<(), MatCode> {
        if material_of(material).is_none() {
            self.denied += 1;
            audit_push(audit, AuditOp::Denied, surface_id);
            return Err(MatCode::AUDIT_DENIED);
        }
        let mut k = 0usize;
        while k < self.count {
            if let Some(e) = self.entries[k] {
                if e.surface_id == surface_id {
                    self.denied += 1;
                    audit_push(audit, AuditOp::Denied, surface_id);
                    return Err(MatCode::AUDIT_DENIED);
                }
            }
            k += 1;
        }
        if self.count >= REGISTRY_CAP {
            self.rejected += 1;
            audit_push(audit, AuditOp::Rejected, surface_id);
            return Err(MatCode::REGISTRY_FULL);
        }
        self.entries[self.count] = Some(SurfaceEntry { surface_id, material });
        self.count += 1;
        audit_push(audit, AuditOp::Register, surface_id);
        Ok(())
    }

    /// 结算期查表面材质（O(注册数)；未注册→安全默认+标注——不静默）。
    pub fn resolve(&self, surface_id: u64) -> (PhysMaterial, SafeDefaultNote) {
        let mut k = 0usize;
        while k < self.count {
            if let Some(e) = self.entries[k] {
                if e.surface_id == surface_id {
                    return resolve_material(e.material);
                }
            }
            k += 1;
        }
        (
            DEFAULT_MATERIAL,
            SafeDefaultNote { fallback: true, reason: MatCode::SURFACE_UNREGISTERED.code() },
        )
    }

    /// 注册数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ===========================================================================
// 五、组合实测矩阵（判据二：物理响应可预览，实测备案+冲突检测+责任段）
// ===========================================================================

/// 组合响应备案（矩阵一格）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PairResponse {
    /// 组合摩擦（千分比；通常取两材质较小值——实测备案，不现算）。
    pub friction_permille: u32,
    /// 组合恢复系数（千分比）。
    pub restitution_permille: u32,
    /// 是否实测备案（false=推导值，预览端必须标注「未测」）。
    pub measured: bool,
    /// 责任段（备案来源段名——冲突上报的追责锚点）。
    pub owner: &'static str,
}

/// 组合键归一化：(a,b) 与 (b,a) 同格——矩阵对称，格数 n×(n+1)/2。
pub fn pair_index(a: MaterialId, b: MaterialId) -> Option<usize> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    if lo as usize >= MATERIAL_LIBRARY.len() || hi as usize >= MATERIAL_LIBRARY.len() {
        return None;
    }
    // 上三角线性索引：i=lo，j=hi，idx = i*(2n-i-1)/2 + (j-i)。
    let n = MATERIAL_LIBRARY.len();
    let i = lo as usize;
    let j = hi as usize;
    Some(i * (2 * n - i - 1) / 2 + (j - i))
}

/// 矩阵容量（上三角含对角线）。
pub const PAIR_MATRIX_CAP: usize = MATERIAL_LIBRARY.len() * (MATERIAL_LIBRARY.len() + 1) / 2;

/// 一条冲突上报。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConflictReport {
    /// 组合两端材质名（人读）。
    pub pair: (&'static str, &'static str),
    /// 两格摩擦差（千分比绝对值）。
    pub friction_delta: u32,
    /// 责任段（两条备案的 owner——上报必须能追责）。
    pub owner_a: &'static str,
    pub owner_b: &'static str,
}

/// 冲突容差（同组合两条备案摩擦差超此值即冲突）。
pub const CONFLICT_TOLERANCE_PERMILLE: u32 = 50;

/// 冲突检测：扫备案矩阵，同组合多条备案摩擦差超容差即上报（带责任段）。
pub fn detect_conflicts(rows: &[PairResponse], pairs: &[(MaterialId, MaterialId)]) -> Vec<ConflictReport> {
    let mut out: Vec<ConflictReport> = Vec::new();
    let mut i = 0usize;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() {
            if let (Some(&a), Some(&b)) = (pairs.get(i), pairs.get(j)) {
                // 同一组合（归一化后同格）的两条备案才比较。
                if pair_index(a.0, a.1).is_some() && pair_index(a.0, a.1) == pair_index(b.0, b.1) {
                    let fa = rows[i].friction_permille;
                    let fb = rows[j].friction_permille;
                    let delta = if fa > fb { fa - fb } else { fb - fa };
                    if delta > CONFLICT_TOLERANCE_PERMILLE {
                        let na = material_of(a.0).map(|m| m.name).unwrap_or("?");
                        let nb = material_of(a.1).map(|m| m.name).unwrap_or("?");
                        out.push(ConflictReport {
                            pair: (na, nb),
                            friction_delta: delta,
                            owner_a: rows[i].owner,
                            owner_b: rows[j].owner,
                        });
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    out
}

/// 常用组合实测备案（锚点「常用材质组合的物理响应实测备案」）。
///
/// 每格 `owner` 是责任段：这条备案数值由哪段管（预览/追责共用）。
pub const PAIR_MATRIX: [PairResponse; 3] = [
    PairResponse { friction_permille: 270, restitution_permille: 430, measured: true, owner: "AD03-matrix-rubber-steel" },
    PairResponse { friction_permille: 80, restitution_permille: 150, measured: true, owner: "AD03-matrix-rubber-ice" },
    PairResponse { friction_permille: 460, restitution_permille: 360, measured: true, owner: "AD03-matrix-wood-sand" },
];

/// 备案对应的组合（与 [`PAIR_MATRIX`] 同序同长）。
pub const PAIR_MATRIX_KEYS: [(MaterialId, MaterialId); 3] = [(0, 1), (0, 2), (3, 4)];

/// 组合预览（判据「组合预览」）：已测组合返回备案值；未测组合返回由两端
/// 材质**推导**的保守值并强制标注未测——预览端据此分「备案可信」与
/// 「推导参考」两档显示，绝不把推导值伪装成实测。
pub fn preview_pair(a: MaterialId, b: MaterialId) -> Result<(PairResponse, SafeDefaultNote), MatCode> {
    let idx = pair_index(a, b).ok_or(MatCode::MATERIAL_UNKNOWN)?;
    let ma = material_of(a).ok_or(MatCode::MATERIAL_UNKNOWN)?;
    let mb = material_of(b).ok_or(MatCode::MATERIAL_UNKNOWN)?;
    let mut k = 0usize;
    while k < PAIR_MATRIX.len() {
        let (ka, kb) = PAIR_MATRIX_KEYS[k];
        if pair_index(ka, kb) == Some(idx) {
            return Ok((PAIR_MATRIX[k], SafeDefaultNote { fallback: false, reason: 0 }));
        }
        k += 1;
    }
    // 推导口径：摩擦取小值、恢复取平均（保守）；标注未测。
    let fr = if ma.friction_permille < mb.friction_permille {
        ma.friction_permille
    } else {
        mb.friction_permille
    };
    let re = (ma.elasticity_permille + mb.elasticity_permille) / 2;
    Ok((
        PairResponse {
            friction_permille: fr,
            restitution_permille: re,
            measured: false,
            owner: "AD03-preview-derived",
        },
        SafeDefaultNote { fallback: true, reason: MatCode::SURFACE_UNREGISTERED.code() },
    ))
}

// ===========================================================================
// 六、越权改→审计（变更留痕）
// ===========================================================================

/// 审计操作码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditOp {
    /// 注册。
    Register,
    /// 拒收（满容）。
    Rejected,
    /// 越权拦截。
    Denied,
}

/// 一条审计痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    /// 操作码。
    pub op: AuditOp,
    /// 目标表面 id。
    pub surface_id: u64,
    /// 操作序号（无墙钟环境用操作计数——确定性口径）。
    pub step: u32,
}

/// 审计簿容量。
pub const AUDIT_CAP: usize = 128;

/// 审计簿：定容、满容拒收计数（不静默覆盖旧痕）。
pub struct AuditLog {
    entries: [Option<AuditEntry>; AUDIT_CAP],
    count: usize,
    step: u32,
    /// 满容拒收计数。
    pub dropped: u32,
}

impl AuditLog {
    /// 空审计簿。
    pub fn new() -> AuditLog {
        AuditLog { entries: [None; AUDIT_CAP], count: 0, step: 0, dropped: 0 }
    }

    /// 追加一条审计痕。
    pub fn push(&mut self, op: AuditOp, surface_id: u64) {
        if self.count >= AUDIT_CAP {
            self.dropped += 1;
            return;
        }
        self.step += 1;
        self.entries[self.count] = Some(AuditEntry { op, surface_id, step: self.step });
        self.count += 1;
    }

    /// 按表面 id 回放全部审计痕（争议追责路径）。
    pub fn replay(&self, surface_id: u64) -> Vec<AuditEntry> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < self.count {
            if let Some(e) = self.entries[k] {
                if e.surface_id == surface_id {
                    out.push(e);
                }
            }
            k += 1;
        }
        out
    }

    /// 审计数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// 审计追加入口（写路径统一走这里——越权改在写入前已拦截，这里只留痕）。
pub fn audit_push(log: &mut AuditLog, op: AuditOp, surface_id: u64) {
    log.push(op, surface_id)
}

/// 摘要行（面板/日志/读屏共用）。
pub fn screen_line() -> String {
    format!(
        "{} materials={} pairs={} matrix_cap={} registry_cap={} audit_cap={}",
        PMAT_VERSION,
        MATERIAL_LIBRARY.len(),
        PAIR_MATRIX.len(),
        PAIR_MATRIX_CAP,
        REGISTRY_CAP,
        AUDIT_CAP,
    )
}
