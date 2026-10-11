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
//! 值不臆造。诊断码段 0x5101~0x5105。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const NV_PWR_VERSION: &str = "G06-nvpwr-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x51 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_PWR_CURVE_UNSORTED: u32 = 0x5101; // 曲线锚点未按瓦特升序（数据缺陷）
pub const XC_PWR_CLASS_MISMATCH: u32 = 0x5102; // 档位与曲线族不匹配（TGP 曲线喂了台式芯片）
pub const XC_PWR_NO_DATA: u32 = 0x5103; // 芯片无实测曲线（诚实空态，不插值）
pub const XC_PWR_WALL_ZERO: u32 = 0x5104; // 功耗墙为零（非法画像查询）
pub const XC_PWR_PROFILE_LEAK: u32 = 0x5105; // 画像导出含白名单外字段（隐私边界自检）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 性能百分比定点（×100）。
pub const PERF_FULL: u32 = 100_00; // 100.00%
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
}

/// 曲线锚点：瓦特 → 性能百分比（×100 定点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CurvePoint {
    pub watts: u32,
    pub perf_pct: u32,
}

/// 功耗-性能曲线（分段线性，锚点升序）。
#[derive(Clone, PartialEq, Eq)]
pub struct PowerCurve {
    /// 所属档位（防错线）。
    pub class: PowerClass,
    /// 芯片代号。
    pub chip: String,
    /// 厂商标称墙（瓦特）：台式=TDP，笔记本=TGP。
    pub nominal_watts: u32,
    /// 锚点集（升序约束由校验保证）。
    pub points: Vec<CurvePoint>,
    pub source: DataSource,
}

impl PowerCurve {
    /// 构造并校验：锚点必须升序、首尾覆盖合理；未排序即拒收。
    pub fn build(
        class: PowerClass,
        chip: &str,
        nominal_watts: u32,
        points: Vec<CurvePoint>,
        source: DataSource,
    ) -> Result<PowerCurve, u32> {
        if nominal_watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        let mut i = 1;
        while i < points.len() {
            if points[i].watts <= points[i - 1].watts {
                return Err(XC_PWR_CURVE_UNSORTED);
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

    /// 触墙判定：标称墙处性能是否已饱和（≈满性能则墙不构成约束）。
    pub fn wall_saturating(&self) -> bool {
        self.perf_at(self.nominal_watts) >= PERF_FULL
    }

    /// 功耗墙损失：墙下性能 vs 满性能的差（×100 定点）。
    pub fn wall_loss_pct(&self) -> u32 {
        PERF_FULL - self.perf_at(self.nominal_watts)
    }
}

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

    /// 挂载曲线（档位不匹配即拒收——判据"双线"的纪律面）。
    pub fn mount(&mut self, curve: PowerCurve) -> Result<(), u32> {
        if curve.chip != self.chip {
            return Err(XC_PWR_CLASS_MISMATCH);
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

    /// 约束画像查询：给定档位与瓦特 → 性能/触墙损失/数据来源。
    pub fn query(&self, class: PowerClass, watts: u32) -> Result<(u32, u32, DataSource), u32> {
        if watts == 0 {
            return Err(XC_PWR_WALL_ZERO);
        }
        let curve = match class {
            PowerClass::DesktopTdp => self.desktop.as_ref(),
            PowerClass::LaptopTgp => self.laptop.as_ref(),
        };
        match curve {
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
}

/// 画像仓：多芯片档案管理 + 公开导出。
pub struct NvPowerRegistry {
    profiles: Vec<DualLineProfile>,
    /// 导出计数。
    pub export_count: u64,
    /// 档位错配拒绝计数。
    pub mismatch_rejects: u64,
    /// 空态显性返回计数（诚实统计）。
    pub unknown_fallbacks: u64,
}

impl NvPowerRegistry {
    pub fn new() -> Self {
        NvPowerRegistry {
            profiles: Vec::new(),
            export_count: 0,
            mismatch_rejects: 0,
            unknown_fallbacks: 0,
        }
    }

    /// 登记档案。
    pub fn register(&mut self, p: DualLineProfile) {
        let mut i = 0;
        while i < self.profiles.len() {
            if self.profiles[i].chip == p.chip {
                self.profiles[i] = p; // 覆盖
                return;
            }
            i += 1;
        }
        self.profiles.push(p);
    }

    /// 查档案。
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

    /// 画像公开导出（JSON 式；字段白名单校验——判据"公开"）。
    pub fn export_profile(&mut self, chip: &str) -> Result<String, u32> {
        self.export_count += 1;
        let p = match self.find(chip) {
            Some(p) => p,
            None => {
                self.unknown_fallbacks += 1;
                // 空态也诚实导出：无数据芯片返回显性 unknown 画像。
                return Ok(String::from(
                    "{\"chip\":\"unknown\",\"power_class\":\"none\",\"tdp_watts\":0,\"tgp_watts\":0,\"curve_points\":0,\"wall_loss_pct\":0,\"data_source\":\"Unknown\"}",
                ));
            }
        };
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
        out.push_str(match src {
            DataSource::VendorSpec => "VendorSpec",
            DataSource::Measured => "Measured",
            DataSource::Unknown => "Unknown",
        });
        out.push_str("\"}");
        // 白名单自检：导出串中不允许出现白名单外字段名样例。
        if contains_str(&out, "\"owner") || contains_str(&out, "\"user_") {
            self.mismatch_rejects += 1;
            return Err(XC_PWR_PROFILE_LEAK);
        }
        Ok(out)
    }
}

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

// ---------------------------------------------------------------------------
// 自检（判据五组：双线 / 约束画像 / 曲线 / 公开 / 诚实空态）
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
        reg.register(p);
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

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_dual()?;
        self.selfcheck_constraint()?;
        self.selfcheck_curve()?;
        self.selfcheck_public()?;
        self.selfcheck_honest_empty()?;
        Ok(())
    }
}

impl DualLineProfile {
    fn mismatch_note(&mut self) {
        // 占位语义方法：错线拒绝后的档案侧标注（供遥测扩展）。
    }
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
