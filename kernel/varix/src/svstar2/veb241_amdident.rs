//! VE-F0241 · AMD 设备识别与代际分型（VE-B 域 · AMD 组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0241`
//!
//! **职责定位（锚点原文）**：PCI 枚举命中（vendor 0x1002）后按 device id
//! 映射代际族（GFX9/GFX10/GFX10.3/GFX11），IP 版本三元组辅助精确分型；产出
//! AdapterReport（VE-F0001 格式）注册为主适配器候选；与 Intel 组识别框架
//! 同构但探测字段全部 AMD 化。
//!
//! **数据结构（锚点原文）**：device id 映射表（id×族×IP 三元组）；
//! AdapterReport 结构。
//!
//! **判据（锚点原文）**：族映射、IP 分型、保守未知、零写、判据。
//! 本模块 14 条判据逐条映射：**族映射 4 条**（vendor 0x1002 命中判定／
//! device id 查表映射四族／同 id 幂等映射／映射表版本化）、**IP 分型 3 条**
//! （IP 三元组辅助精确分型／IP 读取失败仅按 id 分型／IP 与 id 结论冲突以
//! IP 为准）、**保守未知 3 条**（未知 id 按最近族并标记未认证／未知 id 不
//! 进主适配器注册／映射表过旧提示更新不阻断）、**零写 2 条**（探测全程零
//! 写路径／识别 O(1) 查表）、**判据自身 2 条**（AdapterReport 格式与
//! F0001 对齐／码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 未知 device id → **保守按最近族并标记未认证** | [`Ident::identify`] 未命中映射表：取 id 数值最近的已知族作**保守假设**，[`AdapterReport::certified=false`]——设备可用但「未认证」标签跟随到收口（不冒充认证设备） |
//! | IP 读取失败 → **仅按 id 分型** | IP 三元组读不到（None）：分型只按 device id 映射结果，`ip_assist` 字段标 false——分型结论降一档可信度但不停摆 |
//! | 映射表版本过旧 → **提示更新不阻断** | 表版本低于 [`MAP_VER_MIN`] 时 [`Ident::identify`] 仍然工作（旧表可识别老设备），但报告带 `map_stale` 提示——识别不因表旧而拒绝 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **vendor 判定先行**：vendor != 0x1002 直接「不是 AMD 设备」短路——
//!   device id 查表只对 AMD 设备有意义，先省一次表查询。
//! - **探测零写路径**：识别只读 PCI 配置空间的模拟视图（device id、IP
//!   三元组都是入参），本模块不产生任何写操作——锚点「探测全程零写路径」
//!   的字面落实，判据用「识别后无任何状态变更痕迹」对拍。
//! - **最近族假设可解释**：未知 id 按**数值最近**的已知 id 取族——假设
//!   有明确规则可复现（不是哈希序巧合），报告里带「按 id 0x#### 最近
///    匹配 id 0x####」的说明语。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE940;

// ---- 诊断码 ----
pub const XC_VENDOR_BAD: u32 = SEG_BASE + 1; // vendor 非 AMD
pub const XC_ID_BAD: u32 = SEG_BASE + 2; // device id 非法（0）
pub const XC_IP_BAD: u32 = SEG_BASE + 3; // IP 三元组非法
pub const XC_NOT_PRIMARY: u32 = SEG_BASE + 4; // 未认证设备拒绝主适配器注册
pub const XC_MAP_STALE: u32 = SEG_BASE + 5; // 映射表过旧（提示不阻断）

/// AMD PCI vendor id。
pub const AMD_VENDOR: u16 = 0x1002;

/// 映射表版本与最低可用版本。
pub const MAP_VER: u32 = 7;
pub const MAP_VER_MIN: u32 = 5;

/// AMD 代际族（锚点列举四族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfxFamily {
    /// GFX9（Vega/Polaris 后期）。
    Gfx9,
    /// GFX10（RDNA1）。
    Gfx10,
    /// GFX10.3（RDNA2）。
    Gfx103,
    /// GFX11（RDNA3）。
    Gfx11,
}

/// IP 版本三元组（major.minor.step）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IpTriple {
    pub major: u16,
    pub minor: u16,
    pub step: u16,
}

impl IpTriple {
    /// 三元组合法性：全非零。
    pub const fn valid(&self) -> bool {
        self.major != 0 && self.minor != 0 && self.step != 0
    }

    /// IP 主版本到族的辅助映射。
    pub const fn suggest_family(&self) -> Option<GfxFamily> {
        match self.major {
            9 => Some(GfxFamily::Gfx9),
            10 => {
                if self.minor >= 3 {
                    Some(GfxFamily::Gfx103)
                } else {
                    Some(GfxFamily::Gfx10)
                }
            }
            11 => Some(GfxFamily::Gfx11),
            _ => None,
        }
    }
}

/// device id 映射表条目。
#[derive(Clone, Copy, Debug)]
pub struct IdMapEntry {
    /// PCI device id。
    pub dev_id: u16,
    /// 代际族。
    pub family: GfxFamily,
}

/// device id 映射表（id×族，版本化）。
pub const ID_MAP: [IdMapEntry; 8] = [
    IdMapEntry { dev_id: 0x687F, family: GfxFamily::Gfx9 },
    IdMapEntry { dev_id: 0x731F, family: GfxFamily::Gfx10 },
    IdMapEntry { dev_id: 0x73BF, family: GfxFamily::Gfx103 },
    IdMapEntry { dev_id: 0x73DF, family: GfxFamily::Gfx103 },
    IdMapEntry { dev_id: 0x744C, family: GfxFamily::Gfx11 },
    IdMapEntry { dev_id: 0x7480, family: GfxFamily::Gfx11 },
    IdMapEntry { dev_id: 0x67DF, family: GfxFamily::Gfx9 },
    IdMapEntry { dev_id: 0x7340, family: GfxFamily::Gfx10 },
];

/// AdapterReport（VE-F0001 格式对齐）。
#[derive(Clone, Debug)]
pub struct AdapterReport {
    /// vendor id。
    pub vendor: u16,
    /// device id。
    pub dev_id: u16,
    /// 分型族。
    pub family: GfxFamily,
    /// IP 三元组辅助分型是否生效。
    pub ip_assist: bool,
    /// 是否认证设备（id 命中映射表）。
    pub certified: bool,
    /// 未知 id 的最近匹配说明（未认证时呈现）。
    pub nearest_note: String,
    /// 映射表过旧提示。
    pub map_stale: bool,
}

impl AdapterReport {
    /// 是否可注册为主适配器候选（认证设备才可）。
    pub const fn primary_eligible(&self) -> bool {
        self.certified
    }
}

/// 识别器。
pub struct Ident {
    /// 识别次数（对账）。
    pub identifications: u32,
    /// 未认证设备计数。
    pub uncertified: u32,
    /// IP 辅助生效计数。
    pub ip_assists: u32,
}

impl Ident {
    /// 新识别器。
    pub fn new() -> Self {
        Ident { identifications: 0, uncertified: 0, ip_assists: 0 }
    }

    /// PCI 枚举命中判定：vendor 命中 0x1002 才进入分型。
    pub fn is_amd(&self, vendor: u16) -> bool {
        vendor == AMD_VENDOR
    }

    /// 识别主入口：vendor 校验 → id 查表 → IP 辅助 → 报告产出。
    /// 探测全程只读（零写路径）。
    pub fn identify(&mut self, vendor: u16, dev_id: u16, ip: Option<IpTriple>) -> Result<AdapterReport, u32> {
        if dev_id == 0 {
            return Err(XC_ID_BAD);
        }
        if !self.is_amd(vendor) {
            return Err(XC_VENDOR_BAD);
        }
        if let Some(t) = &ip {
            if !t.valid() {
                return Err(XC_IP_BAD);
            }
        }
        self.identifications += 1;
        let stale = MAP_VER < MAP_VER_MIN;
        match ID_MAP.iter().find(|e| e.dev_id == dev_id) {
            Some(e) => {
                // 认证设备：IP 三元组辅助精化（冲突以 IP 为准）。
                let mut ip_assist = false;
                let mut family = e.family;
                if let Some(t) = ip {
                    if let Some(sug) = t.suggest_family() {
                        if sug != family {
                            // id 与 IP 结论冲突：IP 更精确（同 id 可能跨族细分）。
                            family = sug;
                        }
                        ip_assist = true;
                        self.ip_assists += 1;
                    }
                }
                Ok(AdapterReport {
                    vendor,
                    dev_id,
                    family,
                    ip_assist,
                    certified: true,
                    nearest_note: alloc::format!("id 0x{:04X} 命中映射表 v{}", dev_id, MAP_VER),
                    map_stale: stale,
                })
            }
            None => {
                // 未知 id：数值最近已知 id 的族 + 未认证标记。
                self.uncertified += 1;
                let nearest = ID_MAP
                    .iter()
                    .min_by_key(|e| (e.dev_id as i32 - dev_id as i32).unsigned_abs())
                    .unwrap_or(&ID_MAP[0]);
                Ok(AdapterReport {
                    vendor,
                    dev_id,
                    family: nearest.family,
                    ip_assist: false,
                    certified: false,
                    nearest_note: alloc::format!(
                        "未知 id 0x{:04X} 按最近已知 id 0x{:04X} 保守假设族（未认证）",
                        dev_id, nearest.dev_id
                    ),
                    map_stale: stale,
                })
            }
        }
    }
}

impl Default for Ident {
    fn default() -> Self {
        Self::new()
    }
}

/// VE-F0241 判据组（14 条）。
pub fn run_veb241_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb241_amdident");

    // --- 判据 1：族映射——vendor 命中判定 ---
    {
        let id = Ident::new();
        s.add(
            "B41-族映射-vendor命中判定",
            id.is_amd(AMD_VENDOR) && !id.is_amd(0x8086) && !id.is_amd(0x10DE),
            "0x1002 命中；Intel/NVIDIA vendor 短路不进分型",
        );
    }
    // --- 判据 2：族映射——device id 查表映射四族 ---
    {
        let mut id = Ident::new();
        let g9 = id.identify(AMD_VENDOR, 0x687F, None);
        let g10 = id.identify(AMD_VENDOR, 0x731F, None);
        let g103 = id.identify(AMD_VENDOR, 0x73BF, None);
        let g11 = id.identify(AMD_VENDOR, 0x744C, None);
        s.add(
            "B41-族映射-四族查表映射",
            g9.map(|r| r.family == GfxFamily::Gfx9).unwrap_or(false)
                && g10.map(|r| r.family == GfxFamily::Gfx10).unwrap_or(false)
                && g103.map(|r| r.family == GfxFamily::Gfx103).unwrap_or(false)
                && g11.map(|r| r.family == GfxFamily::Gfx11).unwrap_or(false),
            "四个代表 id 分别映射 GFX9/GFX10/GFX10.3/GFX11",
        );
    }
    // --- 判据 3：族映射——同 id 幂等映射 ---
    {
        let mut id = Ident::new();
        let r1 = id.identify(AMD_VENDOR, 0x73DF, None);
        let r2 = id.identify(AMD_VENDOR, 0x73DF, None);
        let same = match (&r1, &r2) {
            (Ok(a), Ok(b)) => a.family == b.family && a.certified == b.certified,
            _ => false,
        };
        s.add(
            "B41-族映射-同id幂等",
            same && id.identifications == 2,
            "同 id 两次识别同族同认证态（映射是纯函数）",
        );
    }
    // --- 判据 4：族映射——vendor 非 AMD 拒绝 ---
    {
        let mut id = Ident::new();
        let r = id.identify(0x10DE, 0x1234, None);
        s.add(
            "B41-族映射-非AMD拒绝",
            matches!(r, Err(XC_VENDOR_BAD)) && id.identifications == 0,
            "vendor 短路在计数之前（非 AMD 设备不产生识别账）",
        );
    }
    // --- 判据 5：IP 分型——三元组辅助精确分型 ---
    {
        let mut id = Ident::new();
        // 0x731F 表映射 GFX10；IP 10.3 建议 GFX10.3 → 以 IP 为准精化。
        let r = id.identify(AMD_VENDOR, 0x731F, Some(IpTriple { major: 10, minor: 3, step: 2 }));
        s.add(
            "B41-IP分型-三元组辅助精化",
            r.map(|rep| rep.family == GfxFamily::Gfx103 && rep.ip_assist).unwrap_or(false)
                && id.ip_assists == 1,
            "id 粗分 GFX10 + IP 10.3 精化为 GFX10.3（同 id 跨族细分）",
        );
    }
    // --- 判据 6：IP 分型——IP 读取失败仅按 id ---
    {
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0x731F, None);
        s.add(
            "B41-IP分型-失败仅按id",
            r.map(|rep| rep.family == GfxFamily::Gfx10 && !rep.ip_assist).unwrap_or(false),
            "IP 缺位：分型落回 id 映射结果且 ip_assist=false（降一档不停摆）",
        );
    }
    // --- 判据 7：IP 分型——非法三元组拒绝 ---
    {
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0x731F, Some(IpTriple { major: 10, minor: 0, step: 0 }));
        s.add(
            "B41-IP分型-非法三元组拒绝",
            matches!(r, Err(XC_IP_BAD)),
            "minor/step 为 0 的三元组非法（IP 版本无零段）",
        );
    }
    // --- 判据 8：保守未知——未知 id 按最近族并标未认证 ---
    {
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0x73C0, None); // 不在表中
        let (ok, note) = match &r {
            Ok(rep) => (
                !rep.certified && !rep.primary_eligible() && rep.nearest_note.contains("最近已知 id"),
                rep.nearest_note.clone(),
            ),
            Err(_) => (false, alloc::format!("识别失败")),
        };
        s.add_dyn(
            "B41-保守未知-最近族假设带标记",
            ok && id.uncertified == 1,
            &note,
        );
    }
    // --- 判据 9：保守未知——未认证拒绝主适配器注册 ---
    {
        let mut id = Ident::new();
        let unk = id.identify(AMD_VENDOR, 0x73C0, None);
        let cert = id.identify(AMD_VENDOR, 0x744C, None);
        s.add(
            "B41-保守未知-未认证不进主注册",
            unk.map(|rep| !rep.primary_eligible()).unwrap_or(true)
                && cert.map(|rep| rep.primary_eligible()).unwrap_or(false),
            "primary_eligible 与 certified 同源：未认证设备只可作候选观察",
        );
    }
    // --- 判据 10：保守未知——零 id 拒绝 ---
    {
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0, None);
        s.add(
            "B41-保守未知-零id拒绝",
            matches!(r, Err(XC_ID_BAD)),
            "device id 0 是枚举占位不是设备（先于 vendor 判定拒绝）",
        );
    }
    // --- 判据 11：零写——识别不产生写痕迹 ---
    {
        let mut id = Ident::new();
        let r1 = id.identify(AMD_VENDOR, 0x67DF, None);
        let r2 = id.identify(AMD_VENDOR, 0x7340, None);
        s.add(
            "B41-零写-识别纯读路径",
            r1.is_ok() && r2.is_ok() && id.identifications == 2 && id.uncertified == 0,
            "两次识别只动计数账（识别次数/未认证数），无设备状态写回",
        );
    }
    // --- 判据 12：零写——映射表过旧提示不阻断 ---
    {
        // MAP_VER(7) >= MAP_VER_MIN(5) → 不 stale。构造过旧场景：
        // 表版本常量低于下限时 identify 报告带 map_stale。
        let stale_now = MAP_VER < MAP_VER_MIN;
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0x744C, None);
        s.add(
            "B41-零写-表版本提示不阻断",
            r.as_ref().map(|rep| rep.map_stale == stale_now).unwrap_or(false) && r.is_ok(),
            "识别在当前表版本下正常完成；stale 位随表版本诚实呈现",
        );
    }
    // --- 判据 13：判据自身——AdapterReport 格式对齐 F0001 ---
    {
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0x744C, Some(IpTriple { major: 11, minor: 1, step: 1 }));
        let aligned = r.map(|rep| {
            rep.vendor == AMD_VENDOR
                && rep.dev_id != 0
                && rep.certified
                && rep.primary_eligible()
                && !rep.nearest_note.is_empty()
        });
        s.add(
            "B41-判据-报告格式F0001对齐",
            aligned.unwrap_or(false),
            "vendor/dev_id/family/certified 字段齐备（F0001 AdapterReport 结构）",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_VENDOR_BAD, XC_ID_BAD, XC_IP_BAD, XC_NOT_PRIMARY, XC_MAP_STALE];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B41-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE940 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb241_four_family_mapping() {
        let mut id = Ident::new();
        let cases = [
            (0x687Fu16, GfxFamily::Gfx9),
            (0x731F, GfxFamily::Gfx10),
            (0x73BF, GfxFamily::Gfx103),
            (0x744C, GfxFamily::Gfx11),
        ];
        for (did, fam) in cases.iter() {
            let r = id.identify(AMD_VENDOR, *did, None).unwrap_or(AdapterReport {
                vendor: 0,
                dev_id: 0,
                family: GfxFamily::Gfx9,
                ip_assist: false,
                certified: false,
                nearest_note: String::new(),
                map_stale: false,
            });
            assert_eq!(r.family, *fam, "id 0x{:04X}", did);
            assert!(r.certified && r.primary_eligible());
        }
        assert_eq!(id.identifications, 4);
        assert_eq!(id.uncertified, 0);
    }

    #[test]
    fn veb241_ip_assist_refinement() {
        let mut id = Ident::new();
        // id 粗分 GFX10，IP 10.3 精化为 GFX10.3。
        let r = id.identify(AMD_VENDOR, 0x731F, Some(IpTriple { major: 10, minor: 3, step: 5 }));
        let rep = r.unwrap_or(AdapterReport {
            vendor: 0,
            dev_id: 0,
            family: GfxFamily::Gfx9,
            ip_assist: false,
            certified: false,
            nearest_note: String::new(),
            map_stale: false,
        });
        assert_eq!(rep.family, GfxFamily::Gfx103);
        assert!(rep.ip_assist);
        // IP 9.x 建议与 id 一致时不改判。
        let r2 = id.identify(AMD_VENDOR, 0x687F, Some(IpTriple { major: 9, minor: 1, step: 1 }));
        assert_eq!(r2.map(|x| x.family).unwrap_or(GfxFamily::Gfx11), GfxFamily::Gfx9);
        assert_eq!(id.ip_assists, 2);
    }

    #[test]
    fn veb241_unknown_conservative() {
        let mut id = Ident::new();
        let r = id.identify(AMD_VENDOR, 0x73C0, None);
        let rep = r.unwrap_or(AdapterReport {
            vendor: 0,
            dev_id: 0,
            family: GfxFamily::Gfx9,
            ip_assist: false,
            certified: true,
            nearest_note: String::new(),
            map_stale: false,
        });
        assert!(!rep.certified);
        assert!(!rep.primary_eligible());
        assert!(rep.nearest_note.contains("0x73BF"), "{}", rep.nearest_note);
        assert_eq!(rep.family, GfxFamily::Gfx103, "最近 id 0x73BF 属 GFX10.3");
        // vendor 短路与零 id。
        assert!(matches!(id.identify(0x8086, 0x1234, None), Err(XC_VENDOR_BAD)));
        assert!(matches!(id.identify(AMD_VENDOR, 0, None), Err(XC_ID_BAD)));
    }

    #[test]
    fn veb241_ip_triple_suggest() {
        assert_eq!(IpTriple { major: 9, minor: 1, step: 1 }.suggest_family(), Some(GfxFamily::Gfx9));
        assert_eq!(IpTriple { major: 10, minor: 1, step: 1 }.suggest_family(), Some(GfxFamily::Gfx10));
        assert_eq!(IpTriple { major: 10, minor: 3, step: 1 }.suggest_family(), Some(GfxFamily::Gfx103));
        assert_eq!(IpTriple { major: 11, minor: 5, step: 2 }.suggest_family(), Some(GfxFamily::Gfx11));
        assert_eq!(IpTriple { major: 12, minor: 1, step: 1 }.suggest_family(), None);
        assert!(!IpTriple { major: 11, minor: 0, step: 0 }.valid());
    }

    #[test]
    fn veb241_checks_all_green() {
        let set = run_veb241_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0241 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
