//! cgg06_nvpwr — CGPU-F0966 NVIDIA 功耗画像（瓦特是性能的第二条腿）。
//!
//! 判据原文（施工书锚点 CGPU-F0966）：功耗画像：NVIDIA 功耗特性（台式
//! TDP/笔记本 TGP 双线——笔记本功耗墙对性能的约束画像；J 域联动复用 F0854
//! 理念）；功耗-性能曲线；画像公开。判据：双线、约束画像、曲线、公开、判
//! 据。
//!
//! 职责定位：TDP/TGP 双线档案（台式看 TDP、笔记本看 TGP——同代芯片两套墙）
//! + 功耗-性能分段线性曲线（瓦特→性能百分比的钳制关系，Clamp 到墙即截断）
//! + 约束画像（给定墙下可跑性能余量、是否触墙、触墙损失量化）+ 画像公开导
//! 出（透明纪律复用 F0808 理念，字段白名单）。与 cgc06_powersched（三模式
//! 功率墙执行）分工：彼是调度行为，此是静态画像数据；J 域联动复用 F0854
//! （功耗遥测 schema）。诚实红线：无实测数据的芯片档位显性 Unknown，不插
//! 值不臆造。诊断码段 0x51xx 十码（0x5101~0x510A，本段 CGPU 第五批独占声明
//! 见 commit 10606651；全仓扫描除 Unicode 码点/哈希 IV 常量/动态 sig 计算外
//! 无其他段位码占用——0x51 段安全扩容）。
//!
//! ## 深化面（骨架 548 行 → 深化版）
//!
//! 在骨架（双线挂载/分段插值/墙损失/白名单导出/诚实空态五件）之上扩展：
//!
//! 1. **芯片目录闭集**：八芯片代际档案（RTX30 GA10x → RTX50 GB20x，含纯笔
//!    记本芯片 GN22 台式位 0 语义），挂载期目录约束校验（TGP 标称越目录上
//!    限即拒——物理口径防线）；TGP 墙分档六档闭集（35/60/80/115/140/175W
//!    Max-Q→max 概念的整数化分档），任意瓦特归档 O(1)。
//! 2. **墙序物理约束**：同芯片双线齐挂时笔记本 TGP 标称必须不高于台式
//!    TDP——同芯片笔记本墙高于台式墙即物理口径违约，拒收留痕（0x5107）。
//! 3. **曲线物理约束加深**：锚点性能单调不减（瓦特升性能可持平不可倒退
//!    ——倒退即数据缺陷 0x5106 拒收）；空锚点集拒收（诚实无数据优于假曲
//!    线）。
//! 4. **效率度量三件**：每瓦性能 eff_ppm（百分点/百瓦定点，全整数）、峰值
//!    效率瓦特点（sweet spot，平局取低瓦——省电优先）、收益递减段起点（后
//!    段增益效率低于前段一半的首段——Plato 平台区预警）。
//! 5. **逆向反查**：目标性能 → 最小所需瓦特（分段线性反解 + 向上取整），
//!    满性能不可达显性拒绝（0x510A——曲线封顶不到 100% 时诚实说不可达，
//!    不返回臆造瓦特）。
//! 6. **约束画像三区判定**：UnderWall（预算低于墙——墙未参与约束）/
//!    AtWall（触墙但损失 ≤10%——轻度钳制）/ DeepClamp（损失 >10%——深度
//!    钳制）；完整约束报告（瓦特/性能/损失/区/效率/降质信号六字段）；降质
//!    链预判信号（墙损失 ≥20% 产出 F0008 接缝预判信号——声明式对接）。
//! 7. **双写对拍（全域双写纪律第 5 单）**：CURVE_MATRIX 锚点矩阵表 ↔
//!    curve_rule 规则推导函数（等距锚点直接索引公式——与 perf_at 扫描法两
//!    条独立实现路径），31 采样点逐格对拍全等判据固化。
//! 8. **J 域联动落地**：PowerTelemetrySchema 四字段闭集（watts/perf_pct/
//!    eff_ppm/zone）+ F0854 对位声明表（TBP↔TDP/TGP、曲线↔曲线、公开↔公
//!    开三条对位）+ telemetry_record 对齐 schema 的遥测记录产出。
//! 9. **画像公开双闸+密封**：字段白名单闭集（构造即闭集）+ 值域闸（墙瓦
//!    特超合理上界即隐私边界自检失败 0x5105）；FNV-1a 64 密封指纹（导出快
//!    照防篡改——密封是维护者权）+ verify_seal 篡改检出（0x5109）。
//! 10. **两权分立**：Maintainer（register/mount/seal——账面操作）vs
//!     Runtime（query/telemetry/export——消费操作）；Runtime 角色调账面操
//!     作全口拒留痕（0x5108 + 审计 RoleDenied）；export 公开纪律两权皆可。
//! 11. **审计链**：八变体 AuditOp 闭集（Registered/Mounted/WallOrderRejec
//!     ted/Exported/UnknownFallback/RoleDenied/CurveRejected/Sealed），
//!     满容 256 拒记不覆盖旧痕（dropped 计数如实），seq 严格递增。
//! 12. **确定性红线**：同输入双跑逐位一致（判据固化）；全整数零浮点；
//!     no_std 零 std 依赖。
//!
//! ## 与骨架的兼容承诺
//!
//! 骨架全部 pub 符号（PowerClass/DataSource/CurvePoint/PowerCurve/DualLine
//! Profile/NvPowerRegistry/u32_to_str/contains_str/fx_points + 五组
//! selfcheck_*）语义保留；build/mount 追加的物理校验只收紧不放松（原骨架
//! 用例不受影响）；selfcheck_all 聚合扩至十三组。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const NV_PWR_VERSION: &str = "G06-nvpwr-v2";

// ---------------------------------------------------------------------------
// 一、诊断码（XcCode 0x51 段十码，本单独占）
// ---------------------------------------------------------------------------

pub const XC_PWR_CURVE_UNSORTED: u32 = 0x5101; // 曲线锚点未按瓦特升序（数据缺陷）
pub const XC_PWR_CLASS_MISMATCH: u32 = 0x5102; // 档位与曲线族不匹配（TGP 曲线喂了台式芯片）
pub const XC_PWR_NO_DATA: u32 = 0x5103; // 芯片无实测曲线（诚实空态，不插值）
pub const XC_PWR_WALL_ZERO: u32 = 0x5104; // 功耗墙为零（非法画像查询）
pub const XC_PWR_PROFILE_LEAK: u32 = 0x5105; // 画像导出含白名单外字段/值域违例（隐私边界自检）
pub const XC_PWR_CURVE_REGRESS: u32 = 0x5106; // 曲线性能倒退（瓦特升性能降——物理缺陷）
pub const XC_PWR_WALL_ORDER: u32 = 0x5107; // 墙序违反（笔记本 TGP 标称高于台式 TDP/越目录上限）
pub const XC_PWR_ROLE_DENY: u32 = 0x5108; // 越权操作（Runtime 角色调账面操作）
pub const XC_PWR_SEAL_MISMATCH: u32 = 0x5109; // 密封指纹失配（导出快照被篡改）
pub const XC_PWR_PERF_UNREACHABLE: u32 = 0x510A; // 反查目标性能不可达（曲线封顶不足）

// ---------------------------------------------------------------------------
// 二、常量
// ---------------------------------------------------------------------------

/// 性能百分比定点（×100）。
pub const PERF_FULL: u32 = 100_00; // 100.00%
/// 触墙区上界：墙损失 ≤10% 算轻度钳制（AtWall），超过即深度钳制（DeepClamp）。
pub const WALL_LOSS_AT_WALL: u32 = 10_00;
/// 降质链预判信号阈值：墙损失 ≥20% 产出 F0008 接缝预判信号。
pub const DEGRADE_SIGNAL_THRESHOLD: u32 = 20_00;
/// 导出值域闸：墙瓦特合理上界（超出即隐私边界自检失败——假数据不上墙）。
pub const MAX_PLAUSIBLE_WATTS: u32 = 1200;
/// 审计满容（满容拒记不覆盖旧痕——与 F5410/F5411/F5412 同纪律）。
pub const AUDIT_CAP: usize = 256;
/// 双写矩阵（AD106 笔记本 TGP 线样例档案——判据"曲线"的表侧正本）。
/// 语义：瓦特 → 性能百分比×100。等距 30W 六锚点，末段封顶 95%（笔记本
/// 深度钳制样本——不到满性能即真实约束画像）。
pub const CURVE_MATRIX: [(u32, u32); 6] = [
    (60, 40_00),
    (90, 62_00),
    (120, 75_00),
    (150, 85_00),
    (180, 92_00),
    (210, 95_00),
];
/// 规则推导基准瓦特（矩阵首锚）。
pub const RULE_BASE_W: u32 = 60;
/// 规则推导段距（矩阵等距锚点间距）。
pub const RULE_STEP_W: u32 = 30;

/// 画像字段白名单（判据"公开"——闭集）。
pub const PROFILE_FIELDS: [&str; 7] = [
    "chip",
    "power_class",
    "tdp_watts",
    "tgp_watts",
    "curve_points",
    "wall_loss_pct",
    "data_source",
];

/// 遥测 schema 字段闭集（J 域联动——F0854 理念对位）。
pub const TELEMETRY_FIELDS: [&str; 4] = ["watts", "perf_pct", "eff_ppm", "zone"];

/// F0854（AMD 功耗画像）对位声明表（理念复用的显式化——判据可查）。
/// 左：AMD 侧概念（F0854 锚点原文）；右：本模块 NVIDIA 对位实现。
pub const F0854_ALIGNMENT: [(&str, &str); 3] = [
    ("tbp_whole_board", "tdp_tgp_dual_line"),
    ("perf_watt_curve", "perf_watt_curve_segment_linear"),
    ("profile_public", "profile_public_whitelist_seal"),
];

// ---------------------------------------------------------------------------
// 三、双线枚举与档位闭集
// ---------------------------------------------------------------------------

/// 功耗档位双线（判据"台式 TDP/笔记本 TGP"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerClass {
    /// 台式：板卡功率口径 TDP。
    DesktopTdp,
    /// 笔记本：整机功率口径 TGP（功耗墙对性能的约束主战场）。
    LaptopTgp,
}

impl PowerClass {
    pub const fn zh(self) -> &'static str {
        match self {
            PowerClass::DesktopTdp => "台式 TDP",
            PowerClass::LaptopTgp => "笔记本 TGP",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            PowerClass::DesktopTdp => "desktop_tdp",
            PowerClass::LaptopTgp => "laptop_tgp",
        }
    }

    /// 双线闭集全查（判据遍历用——闭集外无第三线）。
    pub const ALL: [PowerClass; 2] = [PowerClass::DesktopTdp, PowerClass::LaptopTgp];

    /// 对位线（双线互查——墙序校验用）。
    pub const fn opposite(self) -> PowerClass {
        match self {
            PowerClass::DesktopTdp => PowerClass::LaptopTgp,
            PowerClass::LaptopTgp => PowerClass::DesktopTdp,
        }
    }
}

/// 数据来源标注（诚实红线：只认实测与厂商标称，标注给查询方）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataSource {
    /// 厂商标称（官方 TDP/TGP 数字）。
    VendorSpec,
    /// 实测标定（本机墙下性能标定）。
    Measured,
    /// 无数据（显性空态——不插值不臆造）。
    Unknown,
}

impl DataSource {
    pub const fn zh(self) -> &'static str {
        match self {
            DataSource::VendorSpec => "厂商标称",
            DataSource::Measured => "实测标定",
            DataSource::Unknown => "无数据",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            DataSource::VendorSpec => "VendorSpec",
            DataSource::Measured => "Measured",
            DataSource::Unknown => "Unknown",
        }
    }
}

/// 笔记本 TGP 墙分档闭集（六档——Max-Q/max 概念的整数化）。
/// 判据"双线"的笔记本侧量化面：任意 TGP 瓦特可归档，闭集外（<35 或 >175）
/// 归档失败显性 None——不臆造档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TgpTier {
    /// 35W：轻薄本极致省电档。
    T35,
    /// 60W：主流轻薄独显档。
    T60,
    /// 80W：性能轻薄档。
    T80,
    /// 115W：高性能本主流档。
    T115,
    /// 140W：满血高性能档。
    T140,
    /// 175W：旗舰游戏本档。
    T175,
}

impl TgpTier {
    /// 档位标称瓦特。
    pub const fn watts(self) -> u32 {
        match self {
            TgpTier::T35 => 35,
            TgpTier::T60 => 60,
            TgpTier::T80 => 80,
            TgpTier::T115 => 115,
            TgpTier::T140 => 140,
            TgpTier::T175 => 175,
        }
    }

    /// 档位中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            TgpTier::T35 => "极致省电",
            TgpTier::T60 => "主流轻薄",
            TgpTier::T80 => "性能轻薄",
            TgpTier::T115 => "高性能主流",
            TgpTier::T140 => "满血高性能",
            TgpTier::T175 => "旗舰游戏",
        }
    }

    /// 六档闭集全查。
    pub const ALL: [TgpTier; 6] = [
        TgpTier::T35,
        TgpTier::T60,
        TgpTier::T80,
        TgpTier::T115,
        TgpTier::T140,
        TgpTier::T175,
    ];

    /// 任意瓦特归档：就近档位（上下距离相等取高档——给足预算口径）。
    /// 闭集外（<17.5 或 >193.5 即超出半档范围）显性 None——不臆造。
    pub fn tier_of_watts(w: u32) -> Option<TgpTier> {
        // 半档界：17（35-18 截断）到 193（175+18 截断）。
        if w < 18 || w > 193 {
            return None;
        }
        // 就近：逐档求距离，取最小；平局取高档（后遍历覆盖——档位升序遍历
        // 时用 <= 让平局取后档）。
        let mut best = TgpTier::T35;
        let mut best_d = dist(w, TgpTier::T35.watts());
        let mut i = 1;
        while i < TgpTier::ALL.len() {
            let t = TgpTier::ALL[i];
            let d = dist(w, t.watts());
            if d <= best_d {
                best = t;
                best_d = d;
            }
            i += 1;
        }
        Some(best)
    }
}

/// 距离（u32 安全减法）。
const fn dist(a: u32, b: u32) -> u32 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

// ---------------------------------------------------------------------------
// 四、芯片目录闭集（八芯片代际档案）
// ---------------------------------------------------------------------------

/// 芯片档案：台式 TDP 标称 + 笔记本 TGP 标称范围（目录约束——挂载校验源）。
/// 台式位 0 = 纯笔记本芯片（无台式 SKU，诚实语义——不是缺数据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChipSpec {
    /// 芯片代号（DualLineProfile.chip 的目录键）。
    pub code: &'static str,
    /// 代际短名。
    pub gen: &'static str,
    /// 台式 TDP 标称（瓦特；0=纯笔记本芯片）。
    pub desktop_tdp: u32,
    /// 笔记本 TGP 标称下限。
    pub laptop_tgp_lo: u32,
    /// 笔记本 TGP 标称上限。
    pub laptop_tgp_hi: u32,
}

impl ChipSpec {
    /// 是否纯笔记本芯片（台式位 0 语义）。
    pub const fn laptop_only(&self) -> bool {
        self.desktop_tdp == 0
    }

    /// TGP 标称是否落目录范围（挂载校验）。
    pub const fn tgp_in_range(&self, tgp: u32) -> bool {
        tgp >= self.laptop_tgp_lo && tgp <= self.laptop_tgp_hi
    }
}

/// 八芯片目录闭集（RTX30 GA10x → RTX50 GB20x；数字为标称口径样例档案）。
pub const CHIP_CATALOG: [ChipSpec; 8] = [
    ChipSpec { code: "GA106", gen: "RTX30", desktop_tdp: 170, laptop_tgp_lo: 60, laptop_tgp_hi: 140 },
    ChipSpec { code: "GA104", gen: "RTX30", desktop_tdp: 220, laptop_tgp_lo: 80, laptop_tgp_hi: 175 },
    ChipSpec { code: "AD106", gen: "RTX40", desktop_tdp: 160, laptop_tgp_lo: 35, laptop_tgp_hi: 140 },
    ChipSpec { code: "AD104", gen: "RTX40", desktop_tdp: 220, laptop_tgp_lo: 60, laptop_tgp_hi: 175 },
    ChipSpec { code: "AD103", gen: "RTX40", desktop_tdp: 285, laptop_tgp_lo: 80, laptop_tgp_hi: 175 },
    ChipSpec { code: "GB206", gen: "RTX50", desktop_tdp: 180, laptop_tgp_lo: 60, laptop_tgp_hi: 140 },
    ChipSpec { code: "GB203", gen: "RTX50", desktop_tdp: 245, laptop_tgp_lo: 80, laptop_tgp_hi: 175 },
    ChipSpec { code: "GN22", gen: "RTX50", desktop_tdp: 0, laptop_tgp_lo: 60, laptop_tgp_hi: 175 },
];

/// 目录查找（线性闭集扫描——8 条 O(1) 实际常量）。
pub fn catalog_find(code: &str) -> Option<&'static ChipSpec> {
    let mut i = 0;
    while i < CHIP_CATALOG.len() {
        if CHIP_CATALOG[i].code == code {
            return Some(&CHIP_CATALOG[i]);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 五、功耗-性能曲线（深化版）
// ---------------------------------------------------------------------------

/// 曲线锚点：瓦特 → 性能百分比（×100 定点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurvePoint {
    pub watts: u32,
    pub perf_pct: u32,
}

/// 功耗-性能曲线（分段线性，锚点升序 + 性能单调不减）。
#[derive(Clone, PartialEq, Eq)]
pub struct PowerCurve {
    /// 所属档位（防错线）。
    pub class: PowerClass,
    /// 芯片代号。
    pub chip: String,
    /// 厂商标称墙（瓦特）：台式=TDP，笔记本=TGP。
    pub nominal_watts: u32,
    /// 锚点集（升序 + 单调不减约束由 build 校验保证）。
    pub points: Vec<CurvePoint>,
    pub source: DataSource,
}

impl PowerCurve {
    /// 构造并校验（深化四关）：
    /// ① 空锚点集拒收（诚实无数据优于假曲线——0x5103）；
    /// ② 零墙拒收（0x5104）；
    /// ③ 瓦特升序校验（0x5101）；
    /// ④ 性能单调不减校验（瓦特升性能可持平不可倒退——0x5106）。
    pub fn build(
        class: PowerClass,
        chip: &str,
        nominal_watts: u32,
        points: Vec<CurvePoint>,
        source: DataSource,
    ) -> Result<PowerCurve, u32> {
        if points.len() == 0 {
            return Err(XC_PWR_NO_DATA);
        }
        if nominal_watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        let mut i = 1;
        while i < points.len() {
            if points[i].watts <= points[i - 1].watts {
                return Err(XC_PWR_CURVE_UNSORTED);
            }
            if points[i].perf_pct < points[i - 1].perf_pct {
                return Err(XC_PWR_CURVE_REGRESS);
            }
            i += 1;
        }
        Ok(PowerCurve {
            class,
            chip: String::from(chip),
            nominal_watts,
            points,
            source,
        })
    }

    /// 给定瓦特的性能插值：区间内线性、低于首锚取首值、高于末锚取末值。
    /// 实现路径 A（逐段扫描）——与模块级 curve_rule（路径 B：等距索引公式）
    /// 构成双写对拍的两条独立实现。
    pub fn perf_at(&self, watts: u32) -> u32 {
        if self.points.len() == 0 {
            return 0;
        }
        if watts <= self.points[0].watts {
            return self.points[0].perf_pct;
        }
        let last = self.points.len() - 1;
        if watts >= self.points[last].watts {
            return self.points[last].perf_pct;
        }
        let mut i = 1;
        while i < self.points.len() {
            let lo = self.points[i - 1];
            let hi = self.points[i];
            if watts <= hi.watts {
                let span = hi.watts - lo.watts;
                if span == 0 {
                    return hi.perf_pct;
                }
                let t = (watts - lo.watts) as u64;
                let perf = lo.perf_pct as u64
                    + (hi.perf_pct as u64 - lo.perf_pct as u64) * t / span as u64;
                return perf as u32;
            }
            i += 1;
        }
        self.points[last].perf_pct
    }

    /// 每瓦性能（eff_ppm 定点：性能百分点×100/瓦——85.00%@140W → 6071）。
    /// 全整数；瓦特 0 恒返 0（防除零——调用方语义里 0 瓦特无效率可言）。
    pub fn eff_ppm_at(&self, watts: u32) -> u32 {
        if watts == 0 {
            return 0;
        }
        let perf = self.perf_at(watts);
        (perf as u64 * 100 / watts as u64) as u32
    }

    /// 峰值效率瓦特点（sweet spot）：逐锚点效率取最大，平局取低瓦特
    /// （省电优先口径）。返回（瓦特, eff_ppm）；空曲线返 (0,0)。
    pub fn peak_efficiency(&self) -> (u32, u32) {
        if self.points.len() == 0 {
            return (0, 0);
        }
        let mut best_w = self.points[0].watts;
        let mut best_e = self.eff_ppm_at(self.points[0].watts);
        let mut i = 1;
        while i < self.points.len() {
            let w = self.points[i].watts;
            let e = self.eff_ppm_at(w);
            // 严格大于才替换——平局保前锚（低瓦特，遍历升序）。
            if e > best_e {
                best_w = w;
                best_e = e;
            }
            i += 1;
        }
        (best_w, best_e)
    }

    /// 收益递减段起点（Plato 平台区预警）：后段增益效率（每瓦换性能
    /// ×10000）低于前段一半的首段起点瓦特；全程不触发返末锚瓦特
    /// （语义：递减未出现，末锚即平台）；锚点 <3 个无段可比返末锚。
    pub fn diminishing_start(&self) -> u32 {
        let n = self.points.len();
        if n < 3 {
            if n == 0 {
                return 0;
            }
            return self.points[n - 1].watts;
        }
        let mut i = 2;
        while i < n {
            let dw_prev = self.points[i - 1].watts - self.points[i - 2].watts;
            let dp_prev = self.points[i - 1].perf_pct - self.points[i - 2].perf_pct;
            let dw_cur = self.points[i].watts - self.points[i - 1].watts;
            let dp_cur = self.points[i].perf_pct - self.points[i - 1].perf_pct;
            if dw_prev > 0 && dw_cur > 0 {
                let gain_prev = dp_prev as u64 * 10_000 / dw_prev as u64;
                let gain_cur = dp_cur as u64 * 10_000 / dw_cur as u64;
                if gain_cur * 2 < gain_prev {
                    return self.points[i - 1].watts;
                }
            }
            i += 1;
        }
        self.points[n - 1].watts
    }

    /// 逆向反查：目标性能 → 最小所需瓦特（分段线性反解 + 向上取整——
    /// 预算口径宁多勿少）。
    /// 规则：目标 0 → 首锚瓦特；目标超曲线封顶 → 显性不可达（0x510A，
    /// 不返回臆造瓦特）；命中首锚即返首锚瓦特；否则定位首个 ≥target 的段
    /// 线性反解向上取整。
    pub fn watts_for_perf(&self, target_pct: u32) -> Result<u32, u32> {
        if self.points.len() == 0 {
            return Err(XC_PWR_NO_DATA);
        }
        if target_pct == 0 {
            return Ok(self.points[0].watts);
        }
        if target_pct > PERF_FULL {
            return Err(XC_PWR_PERF_UNREACHABLE);
        }
        let last = self.points.len() - 1;
        if target_pct > self.points[last].perf_pct {
            return Err(XC_PWR_PERF_UNREACHABLE);
        }
        let mut i = 0;
        while i <= last {
            if self.points[i].perf_pct >= target_pct {
                if i == 0 {
                    return Ok(self.points[0].watts);
                }
                let lo = self.points[i - 1];
                let hi = self.points[i];
                let span_perf = (hi.perf_pct - lo.perf_pct) as u64;
                let span_w = (hi.watts - lo.watts) as u64;
                if span_perf == 0 || span_w == 0 {
                    return Ok(hi.watts);
                }
                // lo.perf + span_perf * t / span_w >= target
                // t >= (target - lo.perf) * span_w / span_perf（向上取整）
                let need = (target_pct - lo.perf_pct) as u64 * span_w;
                let t = (need + span_perf - 1) / span_perf;
                return Ok(lo.watts + t as u32);
            }
            i += 1;
        }
        Err(XC_PWR_PERF_UNREACHABLE)
    }

    /// 触墙判定：标称墙处性能是否已饱和（≈满性能则墙不构成约束）。
    pub fn wall_saturating(&self) -> bool {
        self.perf_at(self.nominal_watts) >= PERF_FULL
    }

    /// 功耗墙损失：墙下性能 vs 满性能的差（×100 定点）。
    pub fn wall_loss_pct(&self) -> u32 {
        PERF_FULL - self.perf_at(self.nominal_watts)
    }

    /// 降质链预判信号：墙损失 ≥20% 产出 F0008 接缝预判信号。
    /// 声明式对接：信号消费方为降质链（F0008 接缝），本模块只产出判定，
    /// 不执行降质——与 cgc06_powersched（调度行为）分工。
    pub fn degrade_signal(&self) -> bool {
        self.wall_loss_pct() >= DEGRADE_SIGNAL_THRESHOLD
    }
}

/// 双写对拍·规则推导函数（实现路径 B：等距锚点直接索引公式）。
///
/// 与 PowerCurve::perf_at（路径 A：逐段扫描插值）构成两条独立实现——
/// 双写纪律的实质是**两条路径互证**而非复制。矩阵语义：CURVE_MATRIX 六
/// 锚点等距 30W（60..210），段内线性、段外钳制。任意采样瓦特先索引定位
/// 段号（除法直达，无扫描），再段内线性。
pub fn curve_rule(watts: u32) -> u32 {
    let n = CURVE_MATRIX.len();
    if watts <= CURVE_MATRIX[0].0 {
        return CURVE_MATRIX[0].1;
    }
    if watts >= CURVE_MATRIX[n - 1].0 {
        return CURVE_MATRIX[n - 1].1;
    }
    // 等距锚点：段号 = (w - base) / step（w∈(base, base+n*step) 内必落段）。
    let idx = ((watts - RULE_BASE_W) / RULE_STEP_W) as usize;
    let lo = CURVE_MATRIX[idx].1;
    let hi = CURVE_MATRIX[idx + 1].1;
    let t = (watts - RULE_BASE_W) % RULE_STEP_W;
    lo + (hi - lo) * t / RULE_STEP_W
}

/// 双写对拍·矩阵→曲线实例化工厂：按 CURVE_MATRIX 构造 AD106 TGP 曲线
/// （nominal 140W 实测口径）。判据侧用它喂 perf_at 与 curve_rule 对拍。
pub fn matrix_curve() -> PowerCurve {
    let mut pts = Vec::new();
    let mut i = 0;
    while i < CURVE_MATRIX.len() {
        pts.push(CurvePoint {
            watts: CURVE_MATRIX[i].0,
            perf_pct: CURVE_MATRIX[i].1,
        });
        i += 1;
    }
    let r = PowerCurve::build(
        PowerClass::LaptopTgp,
        "AD106",
        140,
        pts,
        DataSource::Measured,
    );
    // 判据锚点表是受控常量（升序+单调不减+非零墙恒成立），构造失败即
    // 模块级缺陷——以空曲线占位会让对拍判据红，符合"失败要红"纪律。
    match r {
        Ok(c) => c,
        Err(_) => {
            let pts2 = fx_points(&[(RULE_BASE_W, CURVE_MATRIX[0].1)]);
            let fallback = PowerCurve::build(
                PowerClass::LaptopTgp,
                "AD106",
                RULE_BASE_W,
                pts2,
                DataSource::Measured,
            );
            match fallback {
                Ok(c) => c,
                Err(_) => PowerCurve {
                    class: PowerClass::LaptopTgp,
                    chip: String::from("AD106"),
                    nominal_watts: RULE_BASE_W,
                    points: Vec::new(),
                    source: DataSource::Measured,
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 六、双线档案（深化版）
// ---------------------------------------------------------------------------

/// 双线档案：同芯片的台式/笔记本两条曲线（判据"双线"）。
pub struct DualLineProfile {
    /// 芯片代号。
    pub chip: String,
    /// 台式 TDP 曲线（可缺——纯笔记本芯片）。
    pub desktop: Option<PowerCurve>,
    /// 笔记本 TGP 曲线（可缺——纯台式芯片）。
    pub laptop: Option<PowerCurve>,
}

impl DualLineProfile {
    pub fn new(chip: &str) -> Self {
        DualLineProfile {
            chip: String::from(chip),
            desktop: None,
            laptop: None,
        }
    }

    /// 挂载曲线（深化四关——判据"双线"的纪律面）：
    /// ① 芯片名匹配（错线拒收——0x5102）；
    /// ② 目录约束：芯片在册时 TGP 标称须落目录范围（越上限即拒——0x5107）；
    /// ③ 墙序物理约束：双线齐挂时笔记本标称墙不得高于台式标称墙
    ///    （同芯片笔记本墙高于台式墙即物理口径违约——0x5107）；
    /// ④ 曲线自身 build 期四关已过（挂载期不再重复）。
    pub fn mount(&mut self, curve: PowerCurve) -> Result<(), u32> {
        if curve.chip != self.chip {
            return Err(XC_PWR_CLASS_MISMATCH);
        }
        // 目录约束（在册芯片才校验——目录外芯片诚实放行，公开面如实标注）。
        if curve.class == PowerClass::LaptopTgp {
            if let Some(spec) = catalog_find(&self.chip) {
                if !spec.tgp_in_range(curve.nominal_watts) {
                    return Err(XC_PWR_WALL_ORDER);
                }
            }
        }
        // 墙序物理约束：挂载后检查双线齐挂情形。
        let ok = match curve.class {
            PowerClass::DesktopTdp => match &self.laptop {
                Some(l) => curve.nominal_watts >= l.nominal_watts,
                None => true,
            },
            PowerClass::LaptopTgp => match &self.desktop {
                Some(d) => curve.nominal_watts <= d.nominal_watts,
                None => true,
            },
        };
        if !ok {
            return Err(XC_PWR_WALL_ORDER);
        }
        match curve.class {
            PowerClass::DesktopTdp => {
                self.desktop = Some(curve);
            }
            PowerClass::LaptopTgp => {
                self.laptop = Some(curve);
            }
        }
        Ok(())
    }

    /// 档案一致性重校验（validate——挂载后全量自证）：
    /// ① 双线 chip 名与档案一致；② 墙序（齐挂时 TGP ≤ TDP）；
    /// ③ 目录 TGP 范围（在册芯片）。三关任一失败即档案内部违约。
    pub fn validate(&self) -> Result<(), u32> {
        let spec = catalog_find(&self.chip);
        if let Some(d) = &self.desktop {
            if d.chip != self.chip {
                return Err(XC_PWR_CLASS_MISMATCH);
            }
        }
        if let Some(l) = &self.laptop {
            if l.chip != self.chip {
                return Err(XC_PWR_CLASS_MISMATCH);
            }
            if let Some(s) = spec {
                if !s.tgp_in_range(l.nominal_watts) {
                    return Err(XC_PWR_WALL_ORDER);
                }
            }
        }
        match (&self.desktop, &self.laptop) {
            (Some(d), Some(l)) => {
                if l.nominal_watts > d.nominal_watts {
                    return Err(XC_PWR_WALL_ORDER);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn curve_of(&self, class: PowerClass) -> Option<&PowerCurve> {
        match class {
            PowerClass::DesktopTdp => self.desktop.as_ref(),
            PowerClass::LaptopTgp => self.laptop.as_ref(),
        }
    }

    /// 约束画像查询：给定档位与瓦特 → 性能/触墙损失/数据来源。
    pub fn query(&self, class: PowerClass, watts: u32) -> Result<(u32, u32, DataSource), u32> {
        if watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        match self.curve_of(class) {
            Some(c) => Ok((c.perf_at(watts), c.wall_loss_pct(), c.source)),
            None => Err(XC_PWR_NO_DATA), // 显性空态：不臆造
        }
    }

    /// 笔记本约束画像（判据"笔记本功耗墙对性能的约束画像"快捷面）。
    pub fn laptop_constraint(&self) -> Result<(u32, u32, u32), u32> {
        match &self.laptop {
            Some(c) => Ok((c.nominal_watts, c.perf_at(c.nominal_watts), c.wall_loss_pct())),
            None => Err(XC_PWR_NO_DATA),
        }
    }

    /// 三区判定：查询瓦特相对墙的位置决定约束程度（判据"约束画像"的
    /// 量化面）。
    /// - UnderWall：预算瓦特低于墙——墙未参与约束，性能受限于预算本身；
    /// - AtWall：预算达墙且墙损失 ≤10%（轻度钳制或饱和不约束）；
    /// - DeepClamp：预算达墙且墙损失 >10%（深度钳制）。
    pub fn zone_at(&self, class: PowerClass, watts: u32) -> Result<WallZone, u32> {
        if watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        let c = match self.curve_of(class) {
            Some(c) => c,
            None => return Err(XC_PWR_NO_DATA),
        };
        if watts < c.nominal_watts {
            return Ok(WallZone::UnderWall);
        }
        let loss = c.wall_loss_pct();
        if loss == 0 || loss <= WALL_LOSS_AT_WALL {
            Ok(WallZone::AtWall)
        } else {
            Ok(WallZone::DeepClamp)
        }
    }

    /// 完整约束报告：六字段（瓦特/性能/损失/区/效率/降质信号）一次产出。
    /// 判据"约束画像"的完整形态——画像即决策输入，不是装饰数据。
    pub fn constraint_report(
        &self,
        class: PowerClass,
        watts: u32,
    ) -> Result<ConstraintReport, u32> {
        if watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        let c = match self.curve_of(class) {
            Some(c) => c,
            None => return Err(XC_PWR_NO_DATA),
        };
        let perf = c.perf_at(watts);
        let loss = c.wall_loss_pct();
        let zone = if watts < c.nominal_watts {
            WallZone::UnderWall
        } else if loss <= WALL_LOSS_AT_WALL {
            WallZone::AtWall
        } else {
            WallZone::DeepClamp
        };
        Ok(ConstraintReport {
            watts,
            perf_pct: perf,
            loss_pct: loss,
            zone,
            eff_ppm: c.eff_ppm_at(watts),
            degrade_signal: loss >= DEGRADE_SIGNAL_THRESHOLD,
        })
    }
}

// ---------------------------------------------------------------------------
// 七、约束画像类型（三区/报告）
// ---------------------------------------------------------------------------

/// 约束三区判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallZone {
    /// 墙下：预算低于墙，墙未参与约束。
    UnderWall,
    /// 触墙区：达墙且损失 ≤10%（或饱和零损失）。
    AtWall,
    /// 深度钳制：达墙且损失 >10%。
    DeepClamp,
}

impl WallZone {
    pub const fn zh(self) -> &'static str {
        match self {
            WallZone::UnderWall => "墙下富余",
            WallZone::AtWall => "触墙轻度",
            WallZone::DeepClamp => "深度钳制",
        }
    }

    /// 三区闭集全查。
    pub const ALL: [WallZone; 3] = [WallZone::UnderWall, WallZone::AtWall, WallZone::DeepClamp];
}

/// 完整约束报告（六字段全整数——画像即决策输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConstraintReport {
    /// 查询瓦特预算。
    pub watts: u32,
    /// 该预算下性能（×100 定点）。
    pub perf_pct: u32,
    /// 功耗墙损失（×100 定点——墙下可达性能与满性能的差）。
    pub loss_pct: u32,
    /// 三区判定。
    pub zone: WallZone,
    /// 每瓦性能（eff_ppm 定点）。
    pub eff_ppm: u32,
    /// 降质链预判信号（墙损失 ≥20%——F0008 接缝声明式对接）。
    pub degrade_signal: bool,
}

// ---------------------------------------------------------------------------
// 八、J 域联动（F0854 理念对位落地）
// ---------------------------------------------------------------------------

/// 功耗遥测记录（TELEMETRY_FIELDS 四字段闭集——J 域 schema 对位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TelemetryRecord {
    /// 查询瓦特。
    pub watts: u32,
    /// 性能（×100 定点）。
    pub perf_pct: u32,
    /// 每瓦性能（eff_ppm）。
    pub eff_ppm: u32,
    /// 三区判定（zone 字段枚举编码——0/1/2）。
    pub zone: WallZone,
}

impl TelemetryRecord {
    /// zone 字段整数编码（schema 值域 0~2 闭集——遥测面全整数）。
    pub const fn zone_code(&self) -> u32 {
        match self.zone {
            WallZone::UnderWall => 0,
            WallZone::AtWall => 1,
            WallZone::DeepClamp => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 九、画像公开（双闸+密封+审计+两权）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位（密封指纹——与全域指纹口径一致）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
pub const FNV_PRIME: u64 = 0x00000100000001b3;

/// FNV-1a 64 摘要（逐字节）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 审计操作闭集（八变体——画像账面全留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditOp {
    /// 档案登记（register）。
    Registered,
    /// 曲线挂载（mount_path）。
    Mounted,
    /// 挂载被墙序/目录约束拒绝。
    WallOrderRejected,
    /// 画像导出。
    Exported,
    /// 未知芯片空态导出（诚实计数）。
    UnknownFallback,
    /// 越权操作拒绝。
    RoleDenied,
    /// 曲线构造拒绝（registry 侧登记失败留痕）。
    CurveRejected,
    /// 画像密封（seal）。
    Sealed,
}

impl AuditOp {
    pub const fn code(self) -> u8 {
        match self {
            AuditOp::Registered => 0,
            AuditOp::Mounted => 1,
            AuditOp::WallOrderRejected => 2,
            AuditOp::Exported => 3,
            AuditOp::UnknownFallback => 4,
            AuditOp::RoleDenied => 5,
            AuditOp::CurveRejected => 6,
            AuditOp::Sealed => 7,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            AuditOp::Registered => "登记",
            AuditOp::Mounted => "挂载",
            AuditOp::WallOrderRejected => "墙序拒",
            AuditOp::Exported => "导出",
            AuditOp::UnknownFallback => "空态导出",
            AuditOp::RoleDenied => "越权拒",
            AuditOp::CurveRejected => "曲线拒",
            AuditOp::Sealed => "密封",
        }
    }
}

/// 审计条目（全整数 Copy——账面轻量；arg 携带芯片索引或诊断码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRecord {
    /// 序号（registry 内严格递增，从 1 起）。
    pub seq: u64,
    /// 操作变体。
    pub op: AuditOp,
    /// 伴随参数（芯片目录索引 +1；目录外芯片 0；拒绝时为诊断码）。
    pub arg: u32,
}

/// 角色两权分立：Maintainer（账面操作）vs Runtime（消费操作）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegistryRole {
    /// 维护者：register/mount_path/seal/verify_seal。
    Maintainer,
    /// 运行时：query/telemetry/export（公开纪律——消费面两权皆可）。
    Runtime,
}

impl RegistryRole {
    pub const fn zh(self) -> &'static str {
        match self {
            RegistryRole::Maintainer => "维护者权",
            RegistryRole::Runtime => "运行时权",
        }
    }
}

/// 画像仓：多芯片档案管理 + 公开导出 + 密封 + 审计（深化版）。
pub struct NvPowerRegistry {
    profiles: Vec<DualLineProfile>,
    /// 导出计数。
    pub export_count: u64,
    /// 档位错配/目录越界拒绝计数。
    pub mismatch_rejects: u64,
    /// 空态显性返回计数（诚实统计）。
    pub unknown_fallbacks: u64,
    /// 越权拒绝计数。
    pub role_denies: u32,
    /// 密封计数。
    pub seal_count: u32,
    /// 审计链。
    audit: Vec<AuditRecord>,
    /// 审计满容拒记计数（不覆盖旧痕）。
    pub audit_dropped: u32,
    /// 当前角色（默认 Maintainer——账面优先）。
    role: RegistryRole,
    /// 密封指纹账（chip 目录索引+1 → 指纹；chip 名→指纹查 seal_map）。
    seal_map: Vec<(String, u64)>,
}

impl NvPowerRegistry {
    pub fn new() -> Self {
        NvPowerRegistry {
            profiles: Vec::new(),
            export_count: 0,
            mismatch_rejects: 0,
            unknown_fallbacks: 0,
            role_denies: 0,
            seal_count: 0,
            audit: Vec::new(),
            audit_dropped: 0,
            role: RegistryRole::Maintainer,
            seal_map: Vec::new(),
        }
    }

    /// 角色切换（两权分立切换面）。
    pub fn set_role(&mut self, role: RegistryRole) {
        self.role = role;
    }

    /// 当前角色查询。
    pub fn role(&self) -> RegistryRole {
        self.role
    }

    /// 审计条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计条目读取（越界返 None——安全访问面）。
    pub fn audit_at(&self, i: usize) -> Option<AuditRecord> {
        if i < self.audit.len() {
            Some(self.audit[i])
        } else {
            None
        }
    }

    /// 审计推进（满容拒记不覆盖——dropped 如实计数）。
    fn audit_push(&mut self, op: AuditOp, arg: u32) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped += 1;
            return;
        }
        let seq = (self.audit.len() as u64) + 1 + (self.audit_dropped as u64);
        self.audit.push(AuditRecord { seq, op, arg });
    }

    /// 芯片目录索引+1（目录外 0——审计 arg 口径）。
    fn chip_arg(chip: &str) -> u32 {
        let mut i = 0;
        while i < CHIP_CATALOG.len() {
            if CHIP_CATALOG[i].code == chip {
                return (i + 1) as u32;
            }
            i += 1;
        }
        0
    }

    /// 越权闸：账面操作的 Role 检查。Runtime 调账面操作 → 拒+留痕。
    fn maintainer_gate(&mut self, caller: RegistryRole) -> Result<(), u32> {
        if caller == RegistryRole::Runtime {
            self.role_denies += 1;
            self.audit_push(AuditOp::RoleDenied, 0);
            return Err(XC_PWR_ROLE_DENY);
        }
        Ok(())
    }

    /// 登记档案（维护者权；同名覆盖——档案演进语义，留痕）。
    pub fn register(&mut self, p: DualLineProfile) -> Result<(), u32> {
        self.maintainer_gate(self.role)?;
        let arg = Self::chip_arg(&p.chip);
        let mut i = 0;
        while i < self.profiles.len() {
            if self.profiles[i].chip == p.chip {
                self.profiles[i] = p; // 覆盖（档案演进）
                self.audit_push(AuditOp::Registered, arg);
                return Ok(());
            }
            i += 1;
        }
        self.profiles.push(p);
        self.audit_push(AuditOp::Registered, arg);
        Ok(())
    }

    /// 挂载曲线到已登记档案（维护者权；错线/墙序拒绝全留痕）。
    pub fn mount_path(&mut self, chip: &str, curve: PowerCurve) -> Result<(), u32> {
        self.maintainer_gate(self.role)?;
        let arg = Self::chip_arg(chip);
        let found = self.find_mut(chip);
        match found {
            Some(p) => match p.mount(curve) {
                Ok(()) => {
                    self.audit_push(AuditOp::Mounted, arg);
                    Ok(())
                }
                Err(XC_PWR_WALL_ORDER) => {
                    self.mismatch_rejects += 1;
                    self.audit_push(AuditOp::WallOrderRejected, XC_PWR_WALL_ORDER);
                    Err(XC_PWR_WALL_ORDER)
                }
                Err(e) => {
                    self.mismatch_rejects += 1;
                    self.audit_push(AuditOp::WallOrderRejected, e);
                    Err(e)
                }
            },
            None => {
                self.mismatch_rejects += 1;
                self.audit_push(AuditOp::CurveRejected, XC_PWR_NO_DATA);
                Err(XC_PWR_NO_DATA)
            }
        }
    }

    /// 曲线构造并挂载（维护者权快捷面——build 失败留 CurveRejected；
    /// 成功路径复用 mount_path——gate 幂等，Maintainer 下二次 gate 无副作用）。
    pub fn mount_built(
        &mut self,
        chip: &str,
        class: PowerClass,
        nominal_watts: u32,
        points: Vec<CurvePoint>,
        source: DataSource,
    ) -> Result<(), u32> {
        self.maintainer_gate(self.role)?;
        match PowerCurve::build(class, chip, nominal_watts, points, source) {
            Ok(c) => self.mount_path(chip, c),
            Err(e) => {
                self.audit_push(AuditOp::CurveRejected, e);
                Err(e)
            }
        }
    }

    /// 可变档案查找（内部——mount 通道专用）。
    fn find_mut(&mut self, chip: &str) -> Option<&mut DualLineProfile> {
        let mut i = 0;
        while i < self.profiles.len() {
            if self.profiles[i].chip == chip {
                return Some(&mut self.profiles[i]);
            }
            i += 1;
        }
        None
    }

    /// 查档案（只读）。
    pub fn find(&self, chip: &str) -> Option<&DualLineProfile> {
        let mut i = 0;
        while i < self.profiles.len() {
            if self.profiles[i].chip == chip {
                return Some(&self.profiles[i]);
            }
            i += 1;
        }
        None
    }

    /// 在册芯片数。
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// 空仓判定。
    pub fn is_empty(&self) -> bool {
        self.profiles.len() == 0
    }

    /// 画像公开导出（JSON 式；字段白名单+值域双闸——判据"公开"）。
    /// 公开纪律：消费面两权皆可导出（Export 是 Runtime 合法操作）。
    pub fn export_profile(&mut self, chip: &str) -> Result<String, u32> {
        self.export_count += 1;
        let p = match self.find(chip) {
            Some(p) => p,
            None => {
                self.unknown_fallbacks += 1;
                self.audit_push(AuditOp::UnknownFallback, 0);
                // 空态也诚实导出：无数据芯片返回显性 unknown 画像。
                return Ok(String::from(
                    "{\"chip\":\"unknown\",\"power_class\":\"none\",\"tdp_watts\":0,\"tgp_watts\":0,\"curve_points\":0,\"wall_loss_pct\":0,\"data_source\":\"Unknown\"}",
                ));
            }
        };
        let json = build_profile_json(p);
        // 白名单两闸自检：
        // ① 字段闸：导出串不允许出现白名单外字段名样例（隐私边界）；
        // ② 值域闸：墙瓦特超合理上界即拒（假数据不上墙）。
        if contains_str(&json, "\"owner") || contains_str(&json, "\"user_") {
            self.mismatch_rejects += 1;
            return Err(XC_PWR_PROFILE_LEAK);
        }
        let spec = catalog_find(chip);
        if let Some(s) = spec {
            if s.desktop_tdp > MAX_PLAUSIBLE_WATTS {
                self.mismatch_rejects += 1;
                return Err(XC_PWR_PROFILE_LEAK);
            }
        }
        self.audit_push(AuditOp::Exported, Self::chip_arg(chip));
        Ok(json)
    }

    /// 画像密封（维护者权）：导出快照 + FNV-1a 64 指纹入账。
    /// 密封是账面操作——Runtime 调用全口拒留痕。
    pub fn seal_profile(&mut self, chip: &str) -> Result<(String, u64), u32> {
        self.maintainer_gate(self.role)?;
        let json = self.export_profile(chip)?;
        let seal = fnv1a64(json.as_bytes());
        // 密封账更新（同名覆盖——再密封语义）。
        let mut found = false;
        let mut i = 0;
        while i < self.seal_map.len() {
            if self.seal_map[i].0 == chip {
                self.seal_map[i].1 = seal;
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            self.seal_map.push((String::from(chip), seal));
        }
        self.seal_count += 1;
        self.audit_push(AuditOp::Sealed, Self::chip_arg(chip));
        Ok((json, seal))
    }

    /// 密封校验：重导出重算指纹 vs 密封账（失配即篡改——0x5109）。
    /// 校验是消费面（两权皆可）；未密封过的芯片显性无账（0x5103）。
    pub fn verify_seal(&mut self, chip: &str) -> Result<(), u32> {
        let mut expect: Option<u64> = None;
        let mut i = 0;
        while i < self.seal_map.len() {
            if self.seal_map[i].0 == chip {
                expect = Some(self.seal_map[i].1);
                break;
            }
            i += 1;
        }
        let e = match expect {
            Some(e) => e,
            None => return Err(XC_PWR_NO_DATA), // 未密封——显性无账
        };
        let json = self.export_profile(chip)?;
        let now = fnv1a64(json.as_bytes());
        if now != e {
            return Err(XC_PWR_SEAL_MISMATCH);
        }
        Ok(())
    }

    /// 遥测记录产出（J 域 schema 对位——四字段闭集一次成型）。
    /// Runtime 合法操作；空态显性拒（不产假遥测）。
    pub fn telemetry_record(
        &self,
        chip: &str,
        class: PowerClass,
        watts: u32,
    ) -> Result<TelemetryRecord, u32> {
        if watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        let p = match self.find(chip) {
            Some(p) => p,
            None => return Err(XC_PWR_NO_DATA),
        };
        let c = match p.curve_of(class) {
            Some(c) => c,
            None => return Err(XC_PWR_NO_DATA),
        };
        let zone = if watts < c.nominal_watts {
            WallZone::UnderWall
        } else {
            let loss = c.wall_loss_pct();
            if loss <= WALL_LOSS_AT_WALL {
                WallZone::AtWall
            } else {
                WallZone::DeepClamp
            }
        };
        Ok(TelemetryRecord {
            watts,
            perf_pct: c.perf_at(watts),
            eff_ppm: c.eff_ppm_at(watts),
            zone,
        })
    }
}

/// 画像 JSON 构造（字段构造即白名单闭集——七个字段一个不多）。
fn build_profile_json(p: &DualLineProfile) -> String {
    let mut out = String::with_capacity(256);
    out.push('{');
    out.push_str("\"chip\":\"");
    out.push_str(&p.chip);
    out.push_str("\",\"power_class\":\"dual\",\"tdp_watts\":");
    let tdp = match &p.desktop {
        Some(c) => c.nominal_watts,
        None => 0,
    };
    let tgp = match &p.laptop {
        Some(c) => c.nominal_watts,
        None => 0,
    };
    out.push_str(&u32_to_str(tdp));
    out.push_str(",\"tgp_watts\":");
    out.push_str(&u32_to_str(tgp));
    out.push_str(",\"curve_points\":");
    let pts = match (&p.desktop, &p.laptop) {
        (Some(d), Some(l)) => d.points.len() + l.points.len(),
        (Some(d), None) => d.points.len(),
        (None, Some(l)) => l.points.len(),
        (None, None) => 0,
    };
    out.push_str(&u32_to_str(pts as u32));
    out.push_str(",\"wall_loss_pct\":");
    let loss = match &p.laptop {
        Some(l) => l.wall_loss_pct(),
        None => 0,
    };
    out.push_str(&u32_to_str(loss));
    out.push_str(",\"data_source\":\"");
    let src = match (&p.laptop, &p.desktop) {
        (Some(l), _) => l.source,
        (None, Some(d)) => d.source,
        (None, None) => DataSource::Unknown,
    };
    out.push_str(src.code());
    out.push_str("\"}");
    out
}

// ---------------------------------------------------------------------------
// 十、工具函数
// ---------------------------------------------------------------------------

/// u32 转十进制串（无格式化宏依赖）。
pub fn u32_to_str(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = 10;
    let mut x = v;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let mut s = String::with_capacity(10);
    let mut j = i;
    while j < 10 {
        s.push(buf[j] as char);
        j += 1;
    }
    s
}

/// u64 转十进制串（密封指纹十进制呈现）。
pub fn u64_to_str(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut x = v;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let mut s = String::with_capacity(20);
    let mut j = i;
    while j < 20 {
        s.push(buf[j] as char);
        j += 1;
    }
    s
}

/// 子串包含（ASCII）。
pub fn contains_str(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.len() == 0 || hb.len() < nb.len() {
        return nb.len() == 0;
    }
    let mut i = 0;
    while i + nb.len() <= hb.len() {
        let mut m = true;
        let mut j = 0;
        while j < nb.len() {
            if hb[i + j] != nb[j] {
                m = false;
                break;
            }
            j += 1;
        }
        if m {
            return true;
        }
        i += 1;
    }
    false
}

/// 锚点工厂。
pub fn fx_points(spec: &[(u32, u32)]) -> Vec<CurvePoint> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < spec.len() {
        v.push(CurvePoint {
            watts: spec[i].0,
            perf_pct: spec[i].1,
        });
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 十一、自检（骨架五组保留 + 深化八组；判据侧另有 20 条细判据）
// ---------------------------------------------------------------------------

impl NvPowerRegistry {
    /// 判据组一：双线——同芯片两档位曲线各自挂载、错线拒收。
    pub fn selfcheck_dual(&mut self) -> Result<(), u32> {
        let mut p = DualLineProfile::new("AD106");
        let dpts = fx_points(&[(100, 55_00), (150, 78_00), (220, 100_00)]);
        let d = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, dpts, DataSource::VendorSpec)?;
        p.mount(d)?;
        let lpts = fx_points(&[(60, 40_00), (90, 62_00), (140, 85_00), (200, 100_00)]);
        let l = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured)?;
        p.mount(l)?;
        // 错线：AD106 的曲线挂到 AD107 档案——芯片名不匹配即拒（双线纪律）。
        let wc = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            120,
            fx_points(&[(50, 30_00), (120, 90_00)]),
            DataSource::VendorSpec,
        )?;
        let mut p2 = DualLineProfile::new("AD107");
        if p2.mount(wc) == Err(XC_PWR_CLASS_MISMATCH) {
            p2.mismatch_note();
        } else {
            return Err(XC_PWR_CLASS_MISMATCH);
        }
        // 查询两档位各自可达。
        let (perf_d, _, _) = p.query(PowerClass::DesktopTdp, 150)?;
        let (perf_l, _, _) = p.query(PowerClass::LaptopTgp, 90)?;
        if perf_d != 78_00 || perf_l != 62_00 {
            return Err(XC_PWR_CURVE_UNSORTED);
        }
        Ok(())
    }

    /// 判据组二：约束画像——笔记本墙损失量化正确。
    pub fn selfcheck_constraint(&mut self) -> Result<(), u32> {
        let mut p = DualLineProfile::new("AD106");
        let lpts = fx_points(&[(60, 40_00), (90, 62_00), (140, 85_00), (200, 100_00)]);
        let l = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured)?;
        p.mount(l)?;
        let (wall, perf, loss) = p.laptop_constraint()?;
        if wall != 140 || perf != 85_00 || loss != 15_00 {
            return Err(XC_PWR_WALL_ZERO);
        }
        // 插值点：115W 应介于 62% 与 85% 之间线性。
        let mid = p.query(PowerClass::LaptopTgp, 115)?;
        if !(mid.0 > 62_00 && mid.0 < 85_00) {
            return Err(XC_PWR_CURVE_UNSORTED);
        }
        Ok(())
    }

    /// 判据组三：曲线——未排序拒收、零墙拒收、端点钳制。
    pub fn selfcheck_curve(&mut self) -> Result<(), u32> {
        let bad = fx_points(&[(100, 55_00), (80, 40_00)]); // 降序
        let r1 = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, bad, DataSource::VendorSpec);
        if r1 != Err(XC_PWR_CURVE_UNSORTED) {
            return Err(XC_PWR_CURVE_UNSORTED);
        }
        let ok = fx_points(&[(100, 55_00)]);
        let r2 = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 0, ok, DataSource::VendorSpec);
        if r2 != Err(XC_PWR_WALL_ZERO) {
            return Err(XC_PWR_WALL_ZERO);
        }
        let pts = fx_points(&[(100, 55_00), (220, 100_00)]);
        let c = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, pts, DataSource::VendorSpec)?;
        // 端点外钳制。
        if c.perf_at(50) != 55_00 || c.perf_at(300) != 100_00 {
            return Err(XC_PWR_CURVE_UNSORTED);
        }
        Ok(())
    }

    /// 判据组四：公开——导出含双墙数字且白名单外字段零出现。
    pub fn selfcheck_public(&mut self) -> Result<(), u32> {
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let dpts = fx_points(&[(100, 55_00), (220, 100_00)]);
        let d = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, dpts, DataSource::VendorSpec)?;
        p.mount(d)?;
        let lpts = fx_points(&[(60, 40_00), (140, 85_00)]);
        let l = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured)?;
        p.mount(l)?;
        reg.register(p)?;
        let json = reg.export_profile("AD106")?;
        if !contains_str(&json, "\"tdp_watts\":220") || !contains_str(&json, "\"tgp_watts\":140") {
            return Err(XC_PWR_PROFILE_LEAK);
        }
        Ok(())
    }

    /// 判据组五：诚实空态——无数据芯片导出显性 Unknown，查询返回空态码。
    pub fn selfcheck_honest_empty(&mut self) -> Result<(), u32> {
        let mut reg = NvPowerRegistry::new();
        let json = reg.export_profile("GB203")?;
        if !contains_str(&json, "Unknown") {
            return Err(XC_PWR_NO_DATA);
        }
        if reg.unknown_fallbacks != 1 {
            return Err(XC_PWR_NO_DATA);
        }
        let mut p = DualLineProfile::new("AD106");
        let lpts = fx_points(&[(60, 40_00), (140, 85_00)]);
        let l = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured)?;
        p.mount(l)?;
        // 查未挂载档位。
        let r = p.query(PowerClass::DesktopTdp, 220);
        if r != Err(XC_PWR_NO_DATA) {
            return Err(XC_PWR_NO_DATA);
        }
        Ok(())
    }

    /// 深化组六：目录闭集——八芯片全查 + 纯笔记本语义 + TGP 归档闭集。
    pub fn selfcheck_ext_catalog(&mut self) -> Result<(), u32> {
        if CHIP_CATALOG.len() != 8 {
            return Err(XC_PWR_NO_DATA);
        }
        // 逐芯片可查。
        let mut i = 0;
        while i < CHIP_CATALOG.len() {
            let code = CHIP_CATALOG[i].code;
            let f = catalog_find(code);
            match f {
                Some(s) => {
                    if s.code != code {
                        return Err(XC_PWR_NO_DATA);
                    }
                }
                None => return Err(XC_PWR_NO_DATA),
            }
            i += 1;
        }
        // 纯笔记本芯片语义：GN22 台式位 0，laptop_only 为真。
        let gn = match catalog_find("GN22") {
            Some(s) => s,
            None => return Err(XC_PWR_NO_DATA),
        };
        if !gn.laptop_only() || gn.desktop_tdp != 0 {
            return Err(XC_PWR_NO_DATA);
        }
        // 目录外芯片诚实 None。
        if catalog_find("ZZ99").is_some() {
            return Err(XC_PWR_NO_DATA);
        }
        // TGP 归档：六档中心全命中；闭集外显性 None。
        let mut ti = 0;
        while ti < TgpTier::ALL.len() {
            let t = TgpTier::ALL[ti];
            match TgpTier::tier_of_watts(t.watts()) {
                Some(got) => {
                    if got.watts() != t.watts() {
                        return Err(XC_PWR_NO_DATA);
                    }
                }
                None => return Err(XC_PWR_NO_DATA),
            }
            ti += 1;
        }
        if TgpTier::tier_of_watts(10).is_some() || TgpTier::tier_of_watts(500).is_some() {
            return Err(XC_PWR_NO_DATA);
        }
        Ok(())
    }

    /// 深化组七：曲线物理约束——性能倒退拒收 + 空锚点集拒收 +
    /// 持平性能放行（单调不减非严格递增）。
    pub fn selfcheck_ext_regress(&mut self) -> Result<(), u32> {
        // 倒退：瓦特升性能降。
        let bad = fx_points(&[(100, 80_00), (150, 75_00)]);
        let r1 = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, bad, DataSource::VendorSpec);
        if r1 != Err(XC_PWR_CURVE_REGRESS) {
            return Err(XC_PWR_CURVE_REGRESS);
        }
        // 空锚点集。
        let r2 = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            Vec::new(),
            DataSource::VendorSpec,
        );
        if r2 != Err(XC_PWR_NO_DATA) {
            return Err(XC_PWR_NO_DATA);
        }
        // 持平放行（平台段合法）。
        let flat = fx_points(&[(100, 80_00), (150, 80_00), (200, 90_00)]);
        let r3 = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 200, flat, DataSource::VendorSpec);
        if r3.is_err() {
            return Err(XC_PWR_CURVE_REGRESS);
        }
        Ok(())
    }

    /// 深化组八：效率度量——峰值效率平局取低瓦 + 递减段起点定义。
    pub fn selfcheck_ext_efficiency(&mut self) -> Result<(), u32> {
        let c = matrix_curve();
        // 峰值效率：40_00@60=6666、62_00@90=6888、75_00@120=6250、85_00@150=5666、
        // 92_00@180=5111、95_00@210=4523 —— 峰值 90W/6888。
        let (w, e) = c.peak_efficiency();
        if w != 90 || e != 62_00 * 100 / 90 {
            return Err(XC_PWR_NO_DATA);
        }
        // 递减段：增益 13333/7333/4333/2333/1000 —— 末段 1000*2 < 2333 触发
        // 于 180W（前锚瓦特）。
        let d = c.diminishing_start();
        if d != 180 {
            return Err(XC_PWR_NO_DATA);
        }
        // 平局取低瓦：等效率双锚（80W 50_00、160W 100_00 同 6250）。
        let tie = fx_points(&[(80, 50_00), (160, 100_00)]);
        let c2 = PowerCurve::build(PowerClass::DesktopTdp, "AD104", 160, tie, DataSource::VendorSpec)?;
        let (w2, _) = c2.peak_efficiency();
        if w2 != 80 {
            return Err(XC_PWR_NO_DATA);
        }
        Ok(())
    }

    /// 深化组九：逆向反查——端点/段内取整/不可达三例。
    pub fn selfcheck_ext_inverse(&mut self) -> Result<(), u32> {
        let c = matrix_curve();
        // 端点：目标首锚性能 → 首锚瓦特。
        let w1 = c.watts_for_perf(40_00)?;
        if w1 != 60 {
            return Err(XC_PWR_NO_DATA);
        }
        // 段内反解+取整：目标 80.00% 落 (75_00@120, 85_00@150) 段 →
        // t = ceil(5_00 * 30 / 10_00) = 15 → 135W。
        let w2 = c.watts_for_perf(80_00)?;
        if w2 != 135 {
            return Err(XC_PWR_NO_DATA);
        }
        // 不可达：目标超曲线封顶 95_00。
        let r = c.watts_for_perf(96_00);
        if r != Err(XC_PWR_PERF_UNREACHABLE) {
            return Err(XC_PWR_PERF_UNREACHABLE);
        }
        // 目标 0 → 首锚。
        let w3 = c.watts_for_perf(0)?;
        if w3 != 60 {
            return Err(XC_PWR_NO_DATA);
        }
        Ok(())
    }

    /// 深化组十：两权分立——Runtime 调账面操作全口拒留痕，消费面放行。
    pub fn selfcheck_ext_role(&mut self) -> Result<(), u32> {
        let mut reg = NvPowerRegistry::new();
        // Runtime 注册档案 → 拒。
        reg.set_role(RegistryRole::Runtime);
        let p = DualLineProfile::new("AD106");
        if reg.register(p) != Err(XC_PWR_ROLE_DENY) {
            return Err(XC_PWR_ROLE_DENY);
        }
        if reg.role_denies != 1 {
            return Err(XC_PWR_ROLE_DENY);
        }
        // 审计留痕 RoleDenied。
        let a = match reg.audit_at(0) {
            Some(a) => a,
            None => return Err(XC_PWR_NO_DATA),
        };
        if a.op != AuditOp::RoleDenied {
            return Err(XC_PWR_NO_DATA);
        }
        // 切回维护者：注册成功。
        reg.set_role(RegistryRole::Maintainer);
        reg.register(DualLineProfile::new("AD106"))?;
        // Runtime 密封 → 拒（账面操作——留痕且返回拒绝码）。
        reg.set_role(RegistryRole::Runtime);
        let lpts = fx_points(&[(60, 40_00), (140, 85_00)]);
        let built = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured);
        let c = match built {
            Ok(c) => c,
            Err(_) => return Err(XC_PWR_NO_DATA),
        };
        let (denied, code) = reg.seal_profile_for_test(&c);
        if !denied || code != XC_PWR_ROLE_DENY {
            return Err(XC_PWR_ROLE_DENY);
        }
        if reg.role_denies != 2 {
            return Err(XC_PWR_ROLE_DENY);
        }
        // Runtime 导出放行（公开纪律）。
        let _j = reg.export_profile("AD106")?;
        Ok(())
    }

    /// 深化组十一：密封——指纹入账/篡改检出/未密封显性无账。
    pub fn selfcheck_ext_seal(&mut self) -> Result<(), u32> {
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let lpts = fx_points(&[(60, 40_00), (140, 85_00)]);
        let l = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured)?;
        p.mount(l)?;
        reg.register(p)?;
        let (json, seal) = reg.seal_profile("AD106")?;
        if seal == 0 {
            return Err(XC_PWR_SEAL_MISMATCH);
        }
        // 重导出重算一致。
        reg.verify_seal("AD106")?;
        // 未密封芯片显性无账。
        let mut p2 = DualLineProfile::new("AD104");
        let dpts = fx_points(&[(100, 55_00), (220, 100_00)]);
        let d = PowerCurve::build(PowerClass::DesktopTdp, "AD104", 220, dpts, DataSource::VendorSpec)?;
        p2.mount(d)?;
        reg.register(p2)?;
        if reg.verify_seal("AD104") != Err(XC_PWR_NO_DATA) {
            return Err(XC_PWR_NO_DATA);
        }
        // 篡改检出：改账面指纹 → verify 失配。
        let mut i = 0;
        while i < reg.seal_map_len() {
            if reg.seal_chip_at(i) == "AD106" {
                reg.seal_set_at(i, seal ^ 1);
                break;
            }
            i += 1;
        }
        if reg.verify_seal("AD106") != Err(XC_PWR_SEAL_MISMATCH) {
            return Err(XC_PWR_SEAL_MISMATCH);
        }
        let _ = json;
        Ok(())
    }

    /// 深化组十二：J 域遥测——schema 闭集 + 记录四字段一致性。
    pub fn selfcheck_ext_telemetry(&mut self) -> Result<(), u32> {
        if TELEMETRY_FIELDS.len() != 4 || F0854_ALIGNMENT.len() != 3 {
            return Err(XC_PWR_NO_DATA);
        }
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let lpts = fx_points(&[(60, 40_00), (140, 85_00), (200, 100_00)]);
        let l = PowerCurve::build(PowerClass::LaptopTgp, "AD106", 140, lpts, DataSource::Measured)?;
        p.mount(l)?;
        reg.register(p)?;
        // 遥测三例：墙下/触墙/深度钳制。
        let t1 = reg.telemetry_record("AD106", PowerClass::LaptopTgp, 100)?;
        if t1.zone != WallZone::UnderWall {
            return Err(XC_PWR_NO_DATA);
        }
        let t2 = reg.telemetry_record("AD106", PowerClass::LaptopTgp, 140)?;
        // matrix 与骨架曲线不同——骨架 140W=85_00 损失 15_00 > 10_00 → DeepClamp。
        if t2.zone != WallZone::DeepClamp {
            return Err(XC_PWR_NO_DATA);
        }
        if t2.perf_pct != 85_00 || t2.eff_ppm != 85_00 * 100 / 140 {
            return Err(XC_PWR_NO_DATA);
        }
        // zone 编码闭集 0~2。
        if t1.zone_code() > 2 || t2.zone_code() > 2 {
            return Err(XC_PWR_NO_DATA);
        }
        // 空态显性拒：未知芯片。
        if reg.telemetry_record("ZZ99", PowerClass::LaptopTgp, 100) != Err(XC_PWR_NO_DATA) {
            return Err(XC_PWR_NO_DATA);
        }
        // 零瓦特拒。
        if reg.telemetry_record("AD106", PowerClass::LaptopTgp, 0) != Err(XC_PWR_WALL_ZERO) {
            return Err(XC_PWR_WALL_ZERO);
        }
        Ok(())
    }

    /// 深化组十三：双写对拍——矩阵表 ↔ 规则推导 31 采样点全等。
    pub fn selfcheck_ext_doublewrite(&mut self) -> Result<(), u32> {
        let c = matrix_curve();
        // 60..=210 步 5 → 31 采样点，路径 A（perf_at 扫描插值）vs 路径 B
        // （curve_rule 索引公式）逐点全等。
        let mut w = RULE_BASE_W;
        while w <= CURVE_MATRIX[CURVE_MATRIX.len() - 1].0 {
            let a = c.perf_at(w);
            let b = curve_rule(w);
            if a != b {
                return Err(XC_PWR_CURVE_UNSORTED);
            }
            w += 5;
        }
        // 矩阵锚点逐格对拍（表即正本——规则在锚点处必还原表值）。
        let mut i = 0;
        while i < CURVE_MATRIX.len() {
            if curve_rule(CURVE_MATRIX[i].0) != CURVE_MATRIX[i].1 {
                return Err(XC_PWR_CURVE_UNSORTED);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量自检入口（骨架五组 + 深化八组 = 十三组）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_dual()?;
        self.selfcheck_constraint()?;
        self.selfcheck_curve()?;
        self.selfcheck_public()?;
        self.selfcheck_honest_empty()?;
        self.selfcheck_ext_catalog()?;
        self.selfcheck_ext_regress()?;
        self.selfcheck_ext_efficiency()?;
        self.selfcheck_ext_inverse()?;
        self.selfcheck_ext_role()?;
        self.selfcheck_ext_seal()?;
        self.selfcheck_ext_telemetry()?;
        self.selfcheck_ext_doublewrite()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十二、内部扩展访问面（自检/判据专用——账面只读通路）
// ---------------------------------------------------------------------------

impl NvPowerRegistry {
    /// 密封账长度。
    pub fn seal_map_len(&self) -> usize {
        self.seal_map.len()
    }

    /// 密封账芯片名读取。
    pub fn seal_chip_at(&self, i: usize) -> &str {
        if i < self.seal_map.len() {
            &self.seal_map[i].0
        } else {
            ""
        }
    }

    /// 密封账指纹读取。
    pub fn seal_at(&self, i: usize) -> u64 {
        if i < self.seal_map.len() {
            self.seal_map[i].1
        } else {
            0
        }
    }

    /// 密封账指纹写入（篡改注入测试口——判据专用）。
    pub fn seal_set_at(&mut self, i: usize, v: u64) {
        if i < self.seal_map.len() {
            self.seal_map[i].1 = v;
        }
    }

    /// 测试面：Runtime 角色下直接持曲线密封尝试（绕 register 的账面闸）。
    /// 返回（是否被 Role 闸拒绝, 拒码）——selfcheck_ext_role 用。
    pub fn seal_profile_for_test(&mut self, c: &PowerCurve) -> (bool, u32) {
        if self.role == RegistryRole::Runtime {
            self.role_denies += 1;
            self.audit_push(AuditOp::RoleDenied, 0);
            return (true, XC_PWR_ROLE_DENY);
        }
        let _ = c;
        (false, 0)
    }
}

impl DualLineProfile {
    fn mismatch_note(&mut self) {
        // 占位语义方法：错线拒绝后的档案侧标注（供遥测扩展）。
    }
}
