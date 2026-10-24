//! VE-F0260 · AMD PCI 识别（VE-B 域 · AMD 组 · 目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0260`
//!
//! **职责定位（锚点原文）**：PCI 枚举命中（vendor 0x1002）后按 device id
//! 映射代际族（GFX9/GFX10/GFX10.3/GFX11），IP 版本三元组辅助精确分型；
//! 产出 AdapterReport（VE-F0001 格式）注册为主适配器候选；与 Intel 组识
//! 别框架同构但探测字段全部 AMD 化。
//!
//! **数据结构（锚点原文）**：device id 映射表（id×族×IP 三元组）。
//!
//! **判据（锚点原文）**：族映射、IP 分型、保守未知、零写。
//! 本模块 13 条判据逐条映射：**族映射 4 条**（vendor 0x1002 命中才继续／
//! 非 0x1002 显式拒绝／device id 命中映射表得族／映射表外 id 未知保守）、
//! **IP 分型 3 条**（IP 三元组（major.minor.rev）辅助精确分型／三元组缺
//! 失降级仅 device id 分型／三元组与映射冲突以三元组为准）、**保守未知
//! 3 条**（未知 id 不冒充分型结果／未知族报告含「未知」标记／未知候选不
//! 注册主适配器）、**零写 2 条**（识别阶段零硬件写入／探测失败零副作用
//! 状态不变）、**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 未知 id → **保守未知** | [`AmdIdent::identify`] 返回未知态 + 报告带标记——不猜测族 |
//! | 三元组缺失 → **降级 id 分型** | 只有 device id 可用时仍可分型，精度标注降低 |
//! | vendor 不符 → **零写拒绝** | 非 AMD 设备在识别层就拒绝，无任何后续探测副作用 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **识别是纯读过程**：PCI 配置空间只读，identify 全程零写——识别
//!    失败的设备不应该因为「被识别过」而状态漂移。零写由结构保证：
//!    identify 只消费 ProbeCtx 快照，不持有可变设备句柄。
//! 2. **三元组 > device id**：同一 device id 在不同 IP 步进下行为差异
//!    常见，三元组（IP 版本 major.minor.rev）是更精确的分型依据——冲
//!    突时以细粒度为准。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC440 独占 0x10）。
pub const SEG_BASE: u32 = 0xC440;
pub const XC_VENDOR: u32 = SEG_BASE + 1; // vendor 非 AMD
pub const XC_UNKNOWN_ID: u32 = SEG_BASE + 2; // 未知 device id
pub const XC_REPORT_BAD: u32 = SEG_BASE + 3; // 报告格式破缺

/// AMD PCI vendor id。
pub const VENDOR_AMD: u16 = 0x1002;

/// 代际族。
pub const F_GFX9: u8 = 1;
pub const F_GFX10: u8 = 2;
pub const F_GFX103: u8 = 3;
pub const F_GFX11: u8 = 4;

/// 族名。
pub fn family_name(f: u8) -> &'static str {
    match f {
        F_GFX9 => "GFX9",
        F_GFX10 => "GFX10",
        F_GFX103 => "GFX10.3",
        F_GFX11 => "GFX11",
        _ => "未知",
    }
}

/// IP 版本三元组。
#[derive(Clone, Copy, PartialEq)]
pub struct IpTriplet {
    pub major: u8,
    pub minor: u8,
    pub rev: u8,
}

impl IpTriplet {
    /// 三元组 → 族（IP 分型核心表：AMD 化字段）。
    pub fn family(self) -> u8 {
        match (self.major, self.minor) {
            (9, _) => F_GFX9,
            (10, 0..=2) => F_GFX10,
            (10, 3..=5) => F_GFX103,
            (11, _) => F_GFX11,
            _ => 0,
        }
    }
}

/// device id 映射表条目（id×族×IP 辅助）。
#[derive(Clone)]
pub struct IdEntry {
    pub device: u16,
    pub family: u8,
    pub ip_hint: IpTriplet,
}

/// AdapterReport（VE-F0001 格式：vendor×device×族×精度标记）。
#[derive(Clone, PartialEq)]
pub struct AdapterReport {
    pub vendor: u16,
    pub device: u16,
    pub family: u8,
    pub family_name: String,
    pub precision: u8, // 0=id分型 1=三元组分型
    pub known: bool,   // 保守未知标记
}

/// 探测上下文快照（识别阶段零写：identify 只消费快照）。
#[derive(Clone)]
pub struct ProbeCtx {
    pub vendor: u16,
    pub device: u16,
    pub ip: Option<IpTriplet>,
}

/// AMD 识别器。
pub struct AmdIdent {
    pub table: Vec<IdEntry>,
    pub reports: Vec<AdapterReport>,
}

impl AmdIdent {
    pub fn new() -> Self {
        // device id 映射表（AMD 化：真实命名的代表 id + 区段示意）
        AmdIdent {
            table: vec![
                IdEntry { device: 0x687F, family: F_GFX9, ip_hint: IpTriplet { major: 9, minor: 4, rev: 1 } },  // VEGA10
                IdEntry { device: 0x731F, family: F_GFX10, ip_hint: IpTriplet { major: 10, minor: 1, rev: 0 } }, // NAVI10
                IdEntry { device: 0x73BF, family: F_GFX103, ip_hint: IpTriplet { major: 10, minor: 3, rev: 2 } },// NAVI21
                IdEntry { device: 0x744C, family: F_GFX11, ip_hint: IpTriplet { major: 11, minor: 0, rev: 1 } }, // NAVI31
            ],
            reports: Vec::new(),
        }
    }

    /// 识别核：vendor 命中 → id 映射 → 三元组仲裁 → 报告。
    /// 全程零写（只消费快照）。未知 id 保守返回。
    pub fn identify(&mut self, ctx: &ProbeCtx) -> Result<AdapterReport, u32> {
        if ctx.vendor != VENDOR_AMD {
            return Err(XC_VENDOR); // 非 AMD：零写拒绝，无后续探测
        }
        let Some(e) = self.table.iter().find(|e| e.device == ctx.device) else {
            // 未知 id：保守未知报告（不冒充分型、不注册候选）
            let r = AdapterReport {
                vendor: ctx.vendor,
                device: ctx.device,
                family: 0,
                family_name: "未知".into(),
                precision: 0,
                known: false,
            };
            self.reports.push(r.clone());
            return Err(XC_UNKNOWN_ID);
        };
        // 三元组仲裁：有且可分型 → 以三元组为准；否则降级 id 分型
        let (family, precision) = match ctx.ip {
            Some(ip) if ip.family() != 0 => (ip.family(), 1),
            _ => (e.family, 0),
        };
        let r = AdapterReport {
            vendor: ctx.vendor,
            device: ctx.device,
            family,
            family_name: family_name(family).into(),
            precision,
            known: true,
        };
        self.reports.push(r.clone());
        Ok(r)
    }

    /// 注册主适配器候选：仅已知报告可注册（保守未知不注册）。
    pub fn register_primary(&mut self) -> Option<&AdapterReport> {
        let idx = self.reports.iter().rposition(|r| r.known)?;
        Some(&self.reports[idx])
    }

    /// 报告渲染。
    pub fn render(&self) -> String {
        let mut out = format!("AMD 识别报告 表项={} 候选={}\n", self.table.len(), {
            self.reports.iter().filter(|r| r.known).count()
        });
        for r in &self.reports {
            let line = format!(
                "  {:04x}:{:04x} 族={} 精度={} 已知={}\n",
                r.vendor,
                r.device,
                r.family_name,
                if r.precision == 1 { "三元组" } else { "id" },
                r.known
            );
            out.push_str(&line);
        }
        out
    }
}

impl Default for AmdIdent {
    fn default() -> Self {
        Self::new()
    }
}

/// 场景核：全序识别一条龙。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut id = AmdIdent::new();
    // 1) 三元组精确分型（GFX10.3）
    let r = id.identify(&ProbeCtx {
        vendor: VENDOR_AMD,
        device: 0x73BF,
        ip: Some(IpTriplet { major: 10, minor: 3, rev: 2 }),
    });
    if r.as_ref().map(|x| x.family == F_GFX103 && x.precision == 1).unwrap_or(false) {
        caught += 1;
    }
    // 2) 三元组缺失降级 id 分型
    let r = id.identify(&ProbeCtx { vendor: VENDOR_AMD, device: 0x731F, ip: None });
    if r.as_ref().map(|x| x.family == F_GFX10 && x.precision == 0).unwrap_or(false) {
        caught += 1;
    }
    // 3) 未知 id 保守
    if id.identify(&ProbeCtx { vendor: VENDOR_AMD, device: 0x9999, ip: None }) == Err(XC_UNKNOWN_ID) {
        caught += 1;
    }
    // 4) 非 AMD 零写拒绝
    if id.identify(&ProbeCtx { vendor: 0x8086, device: 0x46A6, ip: None }) == Err(XC_VENDOR) {
        caught += 1;
    }
    (caught, 4)
}

/// 判据入口。
pub fn run_veb260_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0260");
    // J1-1 vendor 0x1002 命中
    let mut id = AmdIdent::new();
    let r = id.identify(&ProbeCtx {
        vendor: VENDOR_AMD,
        device: 0x687F,
        ip: Some(IpTriplet { major: 9, minor: 4, rev: 1 }),
    });
    let ok = r.is_ok();
    s.add("AMD-260-01-vendor命中", ok, if ok { "" } else { "vendor 破缺" });
    // J1-2 非 0x1002 显式拒绝（零写）
    let ok = id.identify(&ProbeCtx { vendor: 0x10DE, device: 0x2684, ip: None }) == Err(XC_VENDOR);
    s.add("AMD-260-02-非AMD拒绝", ok, if ok { "" } else { "vendor 放水" });
    // J1-3 device id 映射族（四代齐测）
    let mut id2 = AmdIdent::new();
    let mut all_ok = true;
    for (dev, fam) in [(0x687Fu16, F_GFX9), (0x731F, F_GFX10), (0x73BF, F_GFX103), (0x744C, F_GFX11)] {
        let r = id2.identify(&ProbeCtx { vendor: VENDOR_AMD, device: dev, ip: None });
        all_ok &= r.as_ref().map(|x| x.family == fam && x.precision == 0).unwrap_or(false);
    }
    s.add("AMD-260-03-族映射四代", all_ok, if all_ok { "" } else { "映射表破缺" });
    // J1-4 IP 三元组精确分型
    let r = id2.identify(&ProbeCtx {
        vendor: VENDOR_AMD,
        device: 0x731F,
        ip: Some(IpTriplet { major: 10, minor: 3, rev: 0 }),
    });
    let ok = r.as_ref().map(|x| x.family == F_GFX103 && x.precision == 1).unwrap_or(false);
    s.add("AMD-260-04-三元组分型", ok, if ok { "" } else { "三元组失效" });
    // J1-5 三元组与 id 映射冲突以三元组为准
    // 0x731F 映射 GFX10，但三元组 10.3 → GFX10.3
    let ok = r.as_ref().map(|x| x.family == F_GFX103).unwrap_or(false)
        && id2.table.iter().find(|e| e.device == 0x731F).map(|e| e.family) == Some(F_GFX10);
    s.add("AMD-260-05-冲突三元组准", ok, if ok { "" } else { "仲裁破缺" });
    // J1-6 三元组缺失降级 id 分型
    let r = id2.identify(&ProbeCtx { vendor: VENDOR_AMD, device: 0x744C, ip: None });
    let ok = r.as_ref().map(|x| x.precision == 0).unwrap_or(false);
    s.add("AMD-260-06-降级id分型", ok, if ok { "" } else { "降级破缺" });
    // J1-7 未知 id 保守（不冒充分型）
    let ok = id2.identify(&ProbeCtx { vendor: VENDOR_AMD, device: 0x9999, ip: None })
        == Err(XC_UNKNOWN_ID);
    s.add("AMD-260-07-未知保守", ok, if ok { "" } else { "未知冒充" });
    // J1-8 未知报告含未知标记
    let ok = id2.reports.last().map(|r| !r.known && r.family_name == "未知").unwrap_or(false);
    s.add("AMD-260-08-未知标记", ok, if ok { "" } else { "未知不可查" });
    // J1-9 未知候选不注册主适配器
    let mut id3 = AmdIdent::new();
    let _ = id3.identify(&ProbeCtx { vendor: VENDOR_AMD, device: 0x9999, ip: None });
    let ok = id3.register_primary().is_none();
    s.add("AMD-260-09-未知不注册", ok, if ok { "" } else { "未知候选渗入" });
    // J1-10 已知候选注册（VE-F0001 格式字段齐）
    let _ = id3.identify(&ProbeCtx {
        vendor: VENDOR_AMD,
        device: 0x687F,
        ip: Some(IpTriplet { major: 9, minor: 4, rev: 1 }),
    });
    let p = id3.register_primary();
    let ok = p.map(|r| r.vendor == VENDOR_AMD && r.device == 0x687F && r.known).unwrap_or(false);
    s.add("AMD-260-10-候选注册", ok, if ok { "" } else { "注册破缺" });
    // J1-11 零写（识别失败前后 reports 表只增不改——纯读语义）
    let before = id3.reports.len();
    let _ = id3.identify(&ProbeCtx { vendor: 0x8086, device: 0x46A6, ip: None });
    let ok = id3.reports.len() == before; // 非 AMD 拒绝无副作用
    s.add("AMD-260-11-零写零副作用", ok, if ok { "" } else { "识别有副作用" });
    // J1-12 场景核 4/4
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-260-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_VENDOR != XC_UNKNOWN_ID && XC_UNKNOWN_ID != XC_REPORT_BAD && SEG_BASE % 0x10 == 0;
    s.add("AMD-260-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb260_checks_all_green() {
        let set = run_veb260_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb260_triplet_overrides_id() {
        // 三元组 10.3 覆盖 id 映射 GFX10（冲突细粒度准）
        let mut id = AmdIdent::new();
        let r = id
            .identify(&ProbeCtx {
                vendor: VENDOR_AMD,
                device: 0x731F,
                ip: Some(IpTriplet { major: 10, minor: 3, rev: 9 }),
            })
            .ok()
            .unwrap();
        assert_eq!(r.family, F_GFX103);
        assert_eq!(r.precision, 1);
    }
}
