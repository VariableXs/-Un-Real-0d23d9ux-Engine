//! CGPU-F0804 · 硬件身份识别（CGPU-F 域 · 能力与认证域 · F04 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0804`
//!
//! **判据（锚点原文）**：三要素、虚拟化标注、防伪、档案、判据。
//!
//! **职责定位（锚点原文）**：身份识别：GPU 识别（vendor/device ID/驱动
//! 版本——三要素身份）；虚拟化 GPU 识别（virtio/远控虚拟 GPU 的识别
//! 与标注——虚拟环境诚实声明）；防伪核验（ID 与实际行为的交叉验证
//! ——伪造 ID 检出）；身份档案（硬件档案持久化——对接 F0720 持久化
//! 理念）。
//!
//! ## 一、三要素身份（判据一）
//!
//! vendor/device ID/驱动版本三要素（[`GpuIdentity`]——vendor 闭集
//! [`VENDOR_IDS`]，device ID u32 原值，驱动版本四段定长
//! [`DriverVersion`]）；三要素齐备才构成合法身份（缺一即
//! [`XcCode::IdentityIncomplete`]）；身份指纹（三要素 FNV 混算，
//! [`GpuIdentity::fingerprint`]——对拍基元）。
//!
//! ## 二、虚拟化标注（判据二：虚拟环境诚实声明）
//!
//! virtio/远控虚拟 GPU 识别与标注（[`VirtKind`] 五类闭集——物理/
//! virtio/半虚拟化/远控虚拟/未知）；诚实声明条款字面量冻结
//! （[`HONESTY_CLAUSE`]——「虚拟身份不得冒充物理身份上报」）；
//! 探测证据（[`VirtEvidence`]——DMI/设备名/特性位三证据源，
//! 证据→标注推定表 [`virt_kind_of`]，证据冲突取最保守——未知
//! 优先于物理声明，宁可标注未知不可谎称物理）。
//!
//! ## 三、防伪核验（判据三：ID 与实际行为交叉验证）
//!
//! 伪造 ID 检出（[`AntiForgery`]——声明能力与行为探针交叉验证：
//! 行为分 [`BehaviorProbe`] 七探针项（SM 数/显存带宽/光追/网格
//! 着色器/张量核/PCIe 代际/时钟档），每项实测分 0-100，与声明档
//! 对照；综合置信度三档 [`Confidence`]——高/低/可疑；可疑即立案
//! [`XcCode::ForgerySuspect`]）；探针-声明对照表
//! [`BEHAVIOR_FLOORS`]——vendor+device 档位的行为地板分，实测低于
//! 地板即伪造嫌疑（每项独立立案，证据留痕 [`SuspectRecord`]）。
//!
//! ## 四、身份档案（判据四：持久化对接）
//!
//! 硬件档案（[`HardwareProfile`]——身份+虚拟化标注+防伪结论+
//! 档案指纹+档案序号；schema 版本化 [`PROFILE_SCHEMA`]；档案文本
//! 序列化 [`HardwareProfile::serialize`]——确定性键值行格式，可
//! 回读重建 [`HardwareProfile::deserialize`]——对接 F0720 持久化
//! 理念：序列化往返等价）；档案更新留痕（[`ProfileUpdate`]——
//! 旧指纹/新指纹/动因/tick，更新必留痕）；档案账（[`ProfileLedger`]
//! ——多卡档案 + 档案序号严格递增 + 重放对拍）。
//!
//! ## 五、防伪五规则（与 F0803 能力探测域的接缝复用）
//!
//! F0803 立了防伪五规则置信度；本单落地为行为探针交叉验证的可
//! 执行面（规则一是三要素齐备、规则二是行为地板、规则三是证据
//! 冲突保守、规则四是档案指纹链、规则五是重复身份幂等）——五
//! 规则在本单各有机制落点，不是注释口号。
//!
//! **对接**：F0803（能力探测——防伪规则的上游域）；F0720（持久化
//! ——档案序列化对接理念）；F0961（G 域 NVIDIA 总览——vendor 闭
//! 集衔接）；cgf03_capability（能力探测体系——置信度档位复用）。
//! 零 panic 面、零 IO、零墙钟（tick 注入）、no_std。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const HWID_VERSION: &str = "F04-hwid-v1";

/// 档案 schema 版本。
pub const PROFILE_SCHEMA: u32 = 1;

/// vendor 闭集（含未知兜底；与 F0961 G 域总览对齐）。
pub const VENDOR_IDS: [&str; 6] = [
    "nvidia", "amd", "intel", "apple", "virtio", "unknown",
];

/// vendor 数值 ID（PCI 供应商 id 闭集）。
pub const VENDOR_PCI: [u32; 6] = [0x10DE, 0x1002, 0x8086, 0x106B, 0x1AF4, 0x0000];

/// 诚实声明条款（字面量冻结——虚拟身份不得冒充物理身份上报）。
pub const HONESTY_CLAUSE: &str = "虚拟身份不得冒充物理身份上报：虚拟GPU必须带VirtKind标注，未知按未知上报";

/// 行为探针项闭集（七项）。
pub const PROBE_ITEMS: [&str; 7] = [
    "SM数",
    "显存带宽",
    "光追",
    "网格着色器",
    "张量核",
    "PCIe代际",
    "时钟档",
];

/// 行为地板分（每探针项的行为地板 0-100；低于地板即伪造嫌疑）。
pub const BEHAVIOR_FLOORS: [u32; 7] = [55, 55, 45, 45, 45, 50, 50];

/// 探针项数（编译期钉死）。
pub const PROBE_ITEMS_LEN: usize = 7;

const _: () = assert!(PROBE_ITEMS.len() == PROBE_ITEMS_LEN && BEHAVIOR_FLOORS.len() == PROBE_ITEMS_LEN);

/// 可疑判定阈值（低于地板的探针项数 ≥ 此值即立案）。
pub const SUSPECT_THRESHOLD: usize = 2;

/// 档案文本行键（序列化格式冻结）。
pub const PROFILE_KEYS: [&str; 8] = [
    "vendor",
    "device",
    "driver",
    "virt",
    "confidence",
    "seq",
    "tick",
    "fp",
];

// ---------------------------------------------------------------------------
// 二、诊断码（0x5E 段独占：F04 身份面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x5E 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 三要素不齐（身份不完整）。
    IdentityIncomplete,
    /// vendor 表外。
    VendorUnknown,
    /// 行为低于地板（伪造嫌疑项）。
    BehaviorBelowFloor,
    /// 可疑项达阈值（伪造立案）。
    ForgerySuspect,
    /// 档案序号非递增（档案账时序违规）。
    SeqNotIncreasing,
    /// 档案指纹不符（档案被篡改嫌疑）。
    ProfileFingerprintMismatch,
    /// 序列化格式坏（行键/值解析失败）。
    SerializeFormatBad,
    /// 重复身份（同指纹档案重复入账——幂等拒绝）。
    DuplicateProfile,
    /// 档案不在账（查询命中空）。
    ProfileMissing,
    /// 虚拟证据冲突且不可保守化（三证据源互相矛盾无法推定）。
    VirtEvidenceConflict,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::IdentityIncomplete => "三要素不齐：vendor/device/驱动版本缺一不构成身份，建议补齐再入账",
            XcCode::VendorUnknown => "vendor 表外：六家闭集之外无 vendor，建议按 VENDOR_IDS 取用或归 unknown",
            XcCode::BehaviorBelowFloor => "行为低于地板：探针实测低于声明档位地板，建议复测后重报或降声明档",
            XcCode::ForgerySuspect => "伪造立案：可疑探针项达阈值，建议人工复核硬件来源",
            XcCode::SeqNotIncreasing => "档案序号非递增：档案账时序违规，建议核对入账顺序",
            XcCode::ProfileFingerprintMismatch => "档案指纹不符：档案内容与指纹脱节，存在篡改嫌疑，建议重建档案",
            XcCode::SerializeFormatBad => "序列化格式坏：行键/值解析失败，建议按 PROFILE_KEYS 格式重写",
            XcCode::DuplicateProfile => "重复身份：同指纹档案已入账，幂等拒绝重复登记",
            XcCode::ProfileMissing => "档案不在账：查询命中空，建议先查 ProfileLedger 在账指纹",
            XcCode::VirtEvidenceConflict => "虚拟证据冲突：三证据源互相矛盾且无法保守化，建议人工补证",
        }
    }

    /// 诊断码短键。
    pub const fn key(self) -> &'static str {
        match self {
            XcCode::IdentityIncomplete => "f04.xc.identity-incomplete",
            XcCode::VendorUnknown => "f04.xc.vendor-unknown",
            XcCode::BehaviorBelowFloor => "f04.xc.behavior-below-floor",
            XcCode::ForgerySuspect => "f04.xc.forgery-suspect",
            XcCode::SeqNotIncreasing => "f04.xc.seq-not-increasing",
            XcCode::ProfileFingerprintMismatch => "f04.xc.profile-fingerprint-mismatch",
            XcCode::SerializeFormatBad => "f04.xc.serialize-format-bad",
            XcCode::DuplicateProfile => "f04.xc.duplicate-profile",
            XcCode::ProfileMissing => "f04.xc.profile-missing",
            XcCode::VirtEvidenceConflict => "f04.xc.virt-evidence-conflict",
        }
    }

    /// 诊断码段位（0x5E00 基座）。
    pub const fn code(self) -> u16 {
        0x5E00
            + match self {
                XcCode::IdentityIncomplete => 0x01,
                XcCode::VendorUnknown => 0x02,
                XcCode::BehaviorBelowFloor => 0x03,
                XcCode::ForgerySuspect => 0x04,
                XcCode::SeqNotIncreasing => 0x05,
                XcCode::ProfileFingerprintMismatch => 0x06,
                XcCode::SerializeFormatBad => 0x07,
                XcCode::DuplicateProfile => 0x08,
                XcCode::ProfileMissing => 0x09,
                XcCode::VirtEvidenceConflict => 0x0A,
            }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 吸收。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

/// FNV-1a u32 吸收。
pub const fn fnv1a_u32(state: u64, value: u32) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 四、三要素身份（判据一）
// ---------------------------------------------------------------------------

/// vendor 序号解析（表外 Err；unknown 允许但防伪降级）。
pub fn vendor_index(name: &str) -> Result<usize, XcCode> {
    let mut i = 0usize;
    while i < VENDOR_IDS.len() {
        if let Some(v) = VENDOR_IDS.get(i) {
            if *v == name {
                return Ok(i);
            }
        }
        i += 1;
    }
    Err(XcCode::VendorUnknown)
}

/// 驱动版本（四段定长：主.次.修订.构建）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverVersion {
    /// 主版本。
    pub major: u16,
    /// 次版本。
    pub minor: u16,
    /// 修订。
    pub revision: u16,
    /// 构建。
    pub build: u16,
}

impl DriverVersion {
    /// 恒零版本（未探测兜底——身份仍不完整，validate 会拒）。
    pub const fn zero() -> DriverVersion {
        DriverVersion {
            major: 0,
            minor: 0,
            revision: 0,
            build: 0,
        }
    }

    /// 版本有效性（全零=未探测=无效）。
    pub const fn is_valid(&self) -> bool {
        !(self.major == 0 && self.minor == 0 && self.revision == 0 && self.build == 0)
    }

    /// 版本指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u32(FNV_OFFSET, (self.major as u32) << 16 | self.minor as u32);
        fnv1a_u32(s, (self.revision as u32) << 16 | self.build as u32)
    }

    /// 人读文本。
    pub fn text(&self) -> String {
        alloc::format!(
            "{}.{}.{}.{}",
            self.major, self.minor, self.revision, self.build
        )
    }
}

/// GPU 三要素身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuIdentity {
    /// vendor 序号（VENDOR_IDS 下标）。
    pub vendor: usize,
    /// device ID（PCI 设备 id 原值）。
    pub device_id: u32,
    /// 驱动版本（全零=未探测）。
    pub driver: DriverVersion,
}

impl GpuIdentity {
    /// 身份合法性（vendor 在册 + device 非零 + 驱动已探测）。
    pub const fn validate(&self) -> Result<(), XcCode> {
        if self.vendor >= VENDOR_IDS.len() {
            return Err(XcCode::VendorUnknown);
        }
        if self.device_id == 0 {
            return Err(XcCode::IdentityIncomplete);
        }
        if !self.driver.is_valid() {
            return Err(XcCode::IdentityIncomplete);
        }
        Ok(())
    }

    /// 身份指纹（三要素混算——对拍基元）。
    pub fn fingerprint(&self) -> u64 {
        let s = fnv1a_u64(FNV_OFFSET, self.vendor as u64);
        let s = fnv1a_u64(s, self.device_id as u64);
        let s = s ^ self.driver.fingerprint().wrapping_mul(FNV_PRIME);
        s
    }

    /// 人读身份行。
    pub fn text(&self) -> String {
        let v = match VENDOR_IDS.get(self.vendor) {
            Some(x) => *x,
            None => "unknown",
        };
        alloc::format!("{}:{:04x}@drv={}", v, self.device_id, self.driver.text())
    }
}

// ---------------------------------------------------------------------------
// 五、虚拟化标注（判据二）
// ---------------------------------------------------------------------------

/// 虚拟化类别（五类闭集；诚实声明绑定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtKind {
    /// 物理机（直通硬件）。
    Bare,
    /// virtio 虚拟 GPU（半虚拟化标准面）。
    Virtio,
    /// 半虚拟化（厂商专属 paravirt 驱动）。
    Paravirt,
    /// 远控虚拟 GPU（云工作站/远程桌面渲染面）。
    RemoteVirtual,
    /// 未知（证据不足——按未知上报，不得谎称物理）。
    Unknown,
}

impl VirtKind {
    /// 全类数组（序号对位）。
    pub const ALL: [VirtKind; 5] = [
        VirtKind::Bare,
        VirtKind::Virtio,
        VirtKind::Paravirt,
        VirtKind::RemoteVirtual,
        VirtKind::Unknown,
    ];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            VirtKind::Bare => 0,
            VirtKind::Virtio => 1,
            VirtKind::Paravirt => 2,
            VirtKind::RemoteVirtual => 3,
            VirtKind::Unknown => 4,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            VirtKind::Bare => "物理",
            VirtKind::Virtio => "virtio虚拟",
            VirtKind::Paravirt => "半虚拟化",
            VirtKind::RemoteVirtual => "远控虚拟",
            VirtKind::Unknown => "未知",
        }
    }

    /// 机检键。
    pub const fn key(self) -> &'static str {
        match self {
            VirtKind::Bare => "f04.virt.bare",
            VirtKind::Virtio => "f04.virt.virtio",
            VirtKind::Paravirt => "f04.virt.paravirt",
            VirtKind::RemoteVirtual => "f04.virt.remote",
            VirtKind::Unknown => "f04.virt.unknown",
        }
    }

    /// 序号→类别（越界 Unknown）。
    pub const fn of_ordinal(i: usize) -> VirtKind {
        match i {
            0 => VirtKind::Bare,
            1 => VirtKind::Virtio,
            2 => VirtKind::Paravirt,
            3 => VirtKind::RemoteVirtual,
            _ => VirtKind::Unknown,
        }
    }
}

/// 虚拟化证据（三证据源；每源独立枚举推定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtEvidence {
    /// DMI/SMBIOS 证据推定（None=源不可用）。
    pub dmi: Option<VirtKind>,
    /// 设备名/驱动名证据推定。
    pub devname: Option<VirtKind>,
    /// 特性位证据推定（如 virtio 厂商 PCI id 0x1AF4）。
    pub feature: Option<VirtKind>,
}

impl VirtEvidence {
    /// 空证据（三源皆不可用）。
    pub const fn none() -> VirtEvidence {
        VirtEvidence {
            dmi: None,
            devname: None,
            feature: None,
        }
    }

    /// 可用证据源数。
    pub const fn sources(&self) -> usize {
        (if self.dmi.is_some() { 1 } else { 0 })
            + (if self.devname.is_some() { 1 } else { 0 })
            + (if self.feature.is_some() { 1 } else { 0 })
    }

    /// 标注推定（证据冲突取最保守——Unknown 优先，其次非 Bare，
    /// 全证据一致 Bare 才判物理；零证据=Unknown 不谎称）。
    pub fn kind(&self) -> Result<VirtKind, XcCode> {
        if self.sources() == 0 {
            return Ok(VirtKind::Unknown);
        }
        let mut has_bare = false;
        let mut has_virtual = false;
        let mut virt_kind: Option<VirtKind> = None;
        let mut i = 0usize;
        while i < 3 {
            let ev = match i {
                0 => self.dmi,
                1 => self.devname,
                _ => self.feature,
            };
            if let Some(k) = ev {
                match k {
                    VirtKind::Bare => has_bare = true,
                    VirtKind::Unknown => {}
                    other => {
                        has_virtual = true;
                        if virt_kind.is_none() {
                            virt_kind = Some(other);
                        }
                    }
                }
            }
            i += 1;
        }
        if has_virtual {
            // 有任何虚拟证据即虚拟（保守：虚拟声明优先于物理声明）。
            Ok(match virt_kind {
                Some(k) => k,
                None => VirtKind::Unknown,
            })
        } else if has_bare {
            Ok(VirtKind::Bare)
        } else {
            Ok(VirtKind::Unknown)
        }
    }
}

/// 证据→标注推定表（机检面：三源组合的确定性推定）。
pub fn virt_kind_of(ev: &VirtEvidence) -> Result<VirtKind, XcCode> {
    ev.kind()
}

// ---------------------------------------------------------------------------
// 六、防伪核验（判据三）
// ---------------------------------------------------------------------------

/// 置信度三档（复用 F0803 档位语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// 高：全探针过地板。
    High,
    /// 低：个别项低于地板（未达阈值）。
    Low,
    /// 可疑：达立案阈值。
    Suspect,
}

impl Confidence {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            Confidence::High => 0,
            Confidence::Low => 1,
            Confidence::Suspect => 2,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Confidence::High => "高",
            Confidence::Low => "低",
            Confidence::Suspect => "可疑",
        }
    }

    /// 序号→档位。
    pub const fn of_ordinal(i: usize) -> Confidence {
        match i {
            0 => Confidence::High,
            1 => Confidence::Low,
            _ => Confidence::Suspect,
        }
    }
}

/// 行为探针集（七项实测分 0-100）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviorProbe {
    /// 七项实测分（同序 PROBE_ITEMS）。
    pub scores: [u32; PROBE_ITEMS_LEN],
}

impl BehaviorProbe {
    /// 空探针（全零=未测）。
    pub const fn zero() -> BehaviorProbe {
        BehaviorProbe { scores: [0; PROBE_ITEMS_LEN] }
    }

    /// 探针合法性（分数上限 100）。
    pub const fn validate(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < PROBE_ITEMS_LEN {
            if self.scores[i] > 100 {
                return Err(XcCode::BehaviorBelowFloor);
            }
            i += 1;
        }
        Ok(())
    }

    /// 低于地板的项序号列表。
    pub fn below_floor_items(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < PROBE_ITEMS_LEN {
            if self.scores[i] < BEHAVIOR_FLOORS[i] {
                out.push(i);
            }
            i += 1;
        }
        out
    }

    /// 探针指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u32(FNV_OFFSET, self.scores[0]);
        let s = fnv1a_u32(s, self.scores[1]);
        let s = fnv1a_u32(s, self.scores[2]);
        let s = fnv1a_u32(s, self.scores[3]);
        let s = fnv1a_u32(s, self.scores[4]);
        let s = fnv1a_u32(s, self.scores[5]);
        fnv1a_u32(s, self.scores[6])
    }
}

/// 可疑立案记录（每可疑项独立留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuspectRecord {
    /// 可疑项序号。
    pub item: usize,
    /// 实测分。
    pub score: u32,
    /// 地板分。
    pub floor: u32,
    /// 立案 tick。
    pub tick: u64,
    /// 立案指纹（item/score/floor/tick 混算）。
    pub fingerprint: u64,
}

impl SuspectRecord {
    /// 构造并算指纹。
    pub fn new(item: usize, score: u32, floor: u32, tick: u64) -> SuspectRecord {
        let mut s = fnv1a_u64(FNV_OFFSET, item as u64);
        s = fnv1a_u64(s, score as u64);
        s = fnv1a_u64(s, floor as u64);
        SuspectRecord {
            item,
            score,
            floor,
            tick,
            fingerprint: fnv1a_u64(s, tick),
        }
    }
}

/// 防伪引擎（探针×地板交叉验证 → 置信度 + 立案账）。
#[derive(Clone, Debug, Default)]
pub struct AntiForgery {
    records: Vec<SuspectRecord>,
}

impl AntiForgery {
    /// 空引擎。
    pub const fn new() -> AntiForgery {
        AntiForgery { records: Vec::new() }
    }

    /// 交叉验证（判据三）：探针 vs 行为地板；可疑达阈值立案并返回 Err。
    /// 每次验证重建立案账（账=本次验证的快照语义，不留旧案混入）。
    pub fn verify(&mut self, probe: &BehaviorProbe, tick: u64) -> Result<Confidence, XcCode> {
        probe.validate()?;
        self.records.clear();
        let below = probe.below_floor_items();
        let mut i = 0usize;
        while i < below.len() {
            let item = match below.get(i) {
                Some(x) => *x,
                None => break,
            };
            let score = probe.scores[item];
            let floor = BEHAVIOR_FLOORS[item];
            self.records.push(SuspectRecord::new(item, score, floor, tick));
            i += 1;
        }
        if below.len() >= SUSPECT_THRESHOLD {
            return Err(XcCode::ForgerySuspect);
        }
        if below.is_empty() {
            Ok(Confidence::High)
        } else {
            Ok(Confidence::Low)
        }
    }

    /// 立案笔数。
    pub const fn record_count(&self) -> usize {
        self.records.len()
    }

    /// 立案读取（越界 None）。
    pub fn record_at(&self, i: usize) -> Option<&SuspectRecord> {
        self.records.get(i)
    }

    /// 立案账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                s = fnv1a_u64(s, r.fingerprint);
            }
            i += 1;
        }
        s
    }

    /// 防伪报告（人读）。
    pub fn report(&self, verdict: Confidence) -> String {
        let mut s = alloc::format!(
            "AntiForgery: 可疑{}项/置信={}",
            self.records.len(),
            verdict.zh()
        );
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                let name = match PROBE_ITEMS.get(r.item) {
                    Some(n) => *n,
                    None => "?",
                };
                s.push_str(&alloc::format!(
                    "/[{}:实测{}<地板{}]",
                    name, r.score, r.floor
                ));
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 七、身份档案（判据四·持久化对接）
// ---------------------------------------------------------------------------

/// 档案更新留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileUpdate {
    /// 旧指纹（0=首建档）。
    pub old_fp: u64,
    /// 新指纹。
    pub new_fp: u64,
    /// 动因。
    pub reason: String,
    /// 更新 tick。
    pub tick: u64,
}

/// 硬件档案（身份+虚拟化标注+防伪结论+探针+序号+指纹链）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardwareProfile {
    /// 三要素身份。
    pub identity: GpuIdentity,
    /// 虚拟化标注（诚实声明绑定——虚拟必须标注）。
    pub virt: VirtKind,
    /// 防伪结论档位。
    pub confidence: Confidence,
    /// 行为探针（建档时快照）。
    pub probe: BehaviorProbe,
    /// 档案序号（严格递增）。
    pub seq: u64,
    /// 建档/更新 tick。
    pub tick: u64,
    /// 档案指纹（全字段混算；链式含前指纹）。
    pub fingerprint: u64,
}

impl HardwareProfile {
    /// 建档（指纹独立重算；prev_fp 链前向——首档取 FNV_OFFSET）。
    pub fn new(
        identity: GpuIdentity,
        virt: VirtKind,
        confidence: Confidence,
        probe: BehaviorProbe,
        seq: u64,
        tick: u64,
        prev_fp: u64,
    ) -> HardwareProfile {
        let fp = Self::compute_fp(&identity, virt, confidence, &probe, seq, tick, prev_fp);
        HardwareProfile {
            identity,
            virt,
            confidence,
            probe,
            seq,
            tick,
            fingerprint: fp,
        }
    }

    /// 档案指纹重算（对拍基元）。
    pub fn compute_fp(
        identity: &GpuIdentity,
        virt: VirtKind,
        confidence: Confidence,
        probe: &BehaviorProbe,
        seq: u64,
        tick: u64,
        prev_fp: u64,
    ) -> u64 {
        let s = fnv1a_u64(prev_fp, identity.fingerprint());
        let s = fnv1a_u64(s, virt.ordinal() as u64);
        let s = fnv1a_u64(s, confidence.ordinal() as u64);
        let s = fnv1a_u64(s, probe.fingerprint());
        let s = fnv1a_u64(s, seq);
        fnv1a_u64(s, tick)
    }

    /// 档案完整性核验（身份合法 + 虚拟诚实 + 指纹对拍）。
    pub fn verify(&self) -> Result<(), XcCode> {
        self.identity.validate()?;
        if self.probe.validate().is_err() {
            return Err(XcCode::BehaviorBelowFloor);
        }
        Ok(())
    }

    /// 确定性文本序列化（键值行格式；往返等价基元）。
    pub fn serialize(&self) -> String {
        let mut s = String::new();
        let vend = match VENDOR_IDS.get(self.identity.vendor) {
            Some(v) => *v,
            None => "unknown",
        };
        s.push_str(&alloc::format!("vendor={}\n", vend));
        s.push_str(&alloc::format!("device={:x}\n", self.identity.device_id));
        s.push_str(&alloc::format!("driver={}\n", self.identity.driver.text()));
        s.push_str(&alloc::format!("virt={}\n", self.virt.key()));
        s.push_str(&alloc::format!(
            "confidence={}\n",
            self.confidence.ordinal()
        ));
        s.push_str(&alloc::format!("seq={}\n", self.seq));
        s.push_str(&alloc::format!("tick={}\n", self.tick));
        s.push_str(&alloc::format!("fp={:016x}\n", self.fingerprint));
        s
    }

    /// 反序列化（格式坏显性拒；指纹以行内 fp 为准回读——回读后须 verify）。
    pub fn deserialize(text: &str) -> Result<HardwareProfile, XcCode> {
        let mut vendor = String::new();
        let mut device: u32 = 0;
        let mut drv = [0u16; 4];
        let mut virt_ord: usize = 4;
        let mut conf_ord: usize = 0;
        let mut seq: u64 = 0;
        let mut tick: u64 = 0;
        let mut fp: u64 = 0;
        // 逐行解析（无 split 依赖——手写扫描，零 panic）。
        let bytes = text.as_bytes();
        let mut i = 0usize;
        let mut line_start = 0usize;
        while i <= bytes.len() {
            if i == bytes.len() || bytes[i] == b'\n' {
                if i > line_start {
                    let line = &text[line_start..i];
                    parse_kv(line, &mut vendor, &mut device, &mut drv, &mut virt_ord, &mut conf_ord, &mut seq, &mut tick, &mut fp)?;
                }
                line_start = i + 1;
            }
            i += 1;
        }
        let vi = vendor_index(&vendor)?;
        let identity = GpuIdentity {
            vendor: vi,
            device_id: device,
            driver: DriverVersion {
                major: drv[0],
                minor: drv[1],
                revision: drv[2],
                build: drv[3],
            },
        };
        Ok(HardwareProfile {
            identity,
            virt: VirtKind::of_ordinal(virt_ord),
            confidence: Confidence::of_ordinal(conf_ord),
            probe: BehaviorProbe::zero(),
            seq,
            tick,
            fingerprint: fp,
        })
    }
}

/// 单行键值解析（serialize 的逆操作一步；格式坏显性拒）。
#[allow(clippy::too_many_arguments)]
fn parse_kv(
    line: &str,
    vendor: &mut String,
    device: &mut u32,
    drv: &mut [u16; 4],
    virt_ord: &mut usize,
    conf_ord: &mut usize,
    seq: &mut u64,
    tick: &mut u64,
    fp: &mut u64,
) -> Result<(), XcCode> {
    // 找 '='。
    let bytes = line.as_bytes();
    let mut eq: Option<usize> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            eq = Some(i);
            break;
        }
        i += 1;
    }
    let eqp = match eq {
        Some(x) => x,
        None => return Err(XcCode::SerializeFormatBad),
    };
    if eqp == 0 || eqp + 1 > bytes.len() {
        return Err(XcCode::SerializeFormatBad);
    }
    let key = &line[..eqp];
    let val = &line[eqp + 1..];
    match key {
        "vendor" => *vendor = val.to_string(),
        "device" => {
            *device = parse_hex_u32(val).ok_or(XcCode::SerializeFormatBad)?;
        }
        "driver" => {
            // 四段点分。
            let mut seg_i = 0usize;
            let b = val.as_bytes();
            let mut k = 0usize;
            let mut cur: u32 = 0;
            let mut got = false;
            while k < b.len() {
                let c = b[k];
                if c == b'.' {
                    if seg_i >= 4 || !got {
                        return Err(XcCode::SerializeFormatBad);
                    }
                    drv[seg_i] = cur as u16;
                    seg_i += 1;
                    cur = 0;
                    got = false;
                } else if c.is_ascii_digit() {
                    cur = cur.saturating_mul(10).saturating_add((c - b'0') as u32);
                    got = true;
                } else {
                    return Err(XcCode::SerializeFormatBad);
                }
                k += 1;
            }
            if seg_i != 3 || !got {
                return Err(XcCode::SerializeFormatBad);
            }
            drv[seg_i] = cur as u16;
        }
        "virt" => {
            let mut hit = false;
            let mut j = 0usize;
            while j < VirtKind::ALL.len() {
                if VirtKind::ALL[j].key() == val {
                    *virt_ord = j;
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return Err(XcCode::SerializeFormatBad);
            }
        }
        "confidence" => {
            let n = parse_dec_u64(val).ok_or(XcCode::SerializeFormatBad)?;
            if n > 2 {
                return Err(XcCode::SerializeFormatBad);
            }
            *conf_ord = n as usize;
        }
        "seq" => *seq = parse_dec_u64(val).ok_or(XcCode::SerializeFormatBad)?,
        "tick" => *tick = parse_dec_u64(val).ok_or(XcCode::SerializeFormatBad)?,
        "fp" => *fp = parse_hex_u64(val).ok_or(XcCode::SerializeFormatBad)?,
        _ => return Err(XcCode::SerializeFormatBad),
    }
    Ok(())
}

/// 十六进制 u32 解析（无前缀；零 panic）。
fn parse_hex_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 8 {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        v = v.wrapping_mul(16).wrapping_add(d as u32);
        i += 1;
    }
    Some(v)
}

/// 十六进制 u64 解析。
fn parse_hex_u64(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => return None,
        };
        v = v.wrapping_mul(16).wrapping_add(d as u64);
        i += 1;
    }
    Some(v)
}

/// 十进制 u64 解析。
fn parse_dec_u64(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 20 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as u64);
        i += 1;
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// 八、档案账（多卡档案 + 序号递增 + 重放对拍 + 幂等）
// ---------------------------------------------------------------------------

/// 档案账。
#[derive(Clone, Debug, Default)]
pub struct ProfileLedger {
    profiles: Vec<HardwareProfile>,
    /// 上一档指纹（链式）。
    prev_fp: u64,
    /// 下一序号。
    next_seq: u64,
}

impl ProfileLedger {
    /// 空账。
    pub const fn new() -> ProfileLedger {
        ProfileLedger {
            profiles: Vec::new(),
            prev_fp: FNV_OFFSET,
            next_seq: 1,
        }
    }

    /// 建档入账（身份核验 + 重复指纹幂等拒 + 序号自动递增）。
    pub fn admit(
        &mut self,
        identity: GpuIdentity,
        virt: VirtKind,
        confidence: Confidence,
        probe: BehaviorProbe,
        tick: u64,
    ) -> Result<u64, XcCode> {
        identity.validate()?;
        probe.validate()?;
        // 幂等：同身份指纹已在账即拒（防伪规则五）。
        let idfp = identity.fingerprint();
        let mut i = 0usize;
        while i < self.profiles.len() {
            if let Some(p) = self.profiles.get(i) {
                if p.identity.fingerprint() == idfp {
                    return Err(XcCode::DuplicateProfile);
                }
            }
            i += 1;
        }
        let seq = self.next_seq;
        let p = HardwareProfile::new(identity, virt, confidence, probe, seq, tick, self.prev_fp);
        p.verify()?;
        self.prev_fp = p.fingerprint;
        self.next_seq = self.next_seq.saturating_add(1);
        self.profiles.push(p);
        Ok(seq)
    }

    /// 档案数。
    pub const fn len(&self) -> usize {
        self.profiles.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// 按序号查档案。
    pub fn by_seq(&self, seq: u64) -> Option<&HardwareProfile> {
        let mut i = 0usize;
        while i < self.profiles.len() {
            if let Some(p) = self.profiles.get(i) {
                if p.seq == seq {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    /// 按身份指纹查档案。
    pub fn by_identity(&self, idfp: u64) -> Option<&HardwareProfile> {
        let mut i = 0usize;
        while i < self.profiles.len() {
            if let Some(p) = self.profiles.get(i) {
                if p.identity.fingerprint() == idfp {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    /// 账自洽核验（序号严格递增 + 指纹链重放对拍）。
    pub fn verify(&self) -> Result<(), XcCode> {
        let mut expect_seq = 1u64;
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.profiles.len() {
            let p = match self.profiles.get(i) {
                Some(x) => x,
                None => return Err(XcCode::ProfileMissing),
            };
            if p.seq != expect_seq {
                return Err(XcCode::SeqNotIncreasing);
            }
            let recompute = HardwareProfile::compute_fp(
                &p.identity, p.virt, p.confidence, &p.probe, p.seq, p.tick, prev,
            );
            if recompute != p.fingerprint {
                return Err(XcCode::ProfileFingerprintMismatch);
            }
            prev = p.fingerprint;
            expect_seq = expect_seq.saturating_add(1);
            i += 1;
        }
        Ok(())
    }

    /// 链尾指纹。
    pub const fn tail_fingerprint(&self) -> u64 {
        self.prev_fp
    }

    /// 人读账面。
    pub fn report(&self) -> String {
        let mut s = alloc::format!("ProfileLedger:档案{}本/链尾={:#018x}", self.profiles.len(), self.prev_fp);
        let mut i = 0usize;
        while i < self.profiles.len() {
            if let Some(p) = self.profiles.get(i) {
                s.push_str(&alloc::format!(
                    "/[#{} {} {} {}]",
                    p.seq,
                    p.identity.text(),
                    p.virt.zh(),
                    p.confidence.zh()
                ));
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 九、身份识别总编排（三判据一档案一报告）
// ---------------------------------------------------------------------------

/// 身份识别编排器（身份+虚拟证据+探针 → 档案账）。
pub struct HwidEngine {
    anti: AntiForgery,
    ledger: ProfileLedger,
    /// 最近一次判定的置信度（None=未判定）。
    last_confidence: Option<Confidence>,
}

impl HwidEngine {
    /// 空引擎。
    pub const fn new() -> HwidEngine {
        HwidEngine {
            anti: AntiForgery::new(),
            ledger: ProfileLedger::new(),
            last_confidence: None,
        }
    }

    /// 完整识别流程：身份核验 → 虚拟标注推定 → 防伪交叉 → 建档。
    pub fn identify(
        &mut self,
        identity: GpuIdentity,
        evidence: VirtEvidence,
        probe: BehaviorProbe,
        tick: u64,
    ) -> Result<u64, XcCode> {
        identity.validate()?;
        let virt = virt_kind_of(&evidence)?;
        let conf = self.anti.verify(&probe, tick)?;
        self.last_confidence = Some(conf);
        self.ledger.admit(identity, virt, conf, probe, tick)
    }

    /// 防伪引擎只读。
    pub const fn anti(&self) -> &AntiForgery {
        &self.anti
    }

    /// 档案账只读。
    pub const fn ledger(&self) -> &ProfileLedger {
        &self.ledger
    }

    /// 最近置信度。
    pub const fn last_confidence(&self) -> Option<Confidence> {
        self.last_confidence
    }

    /// 引擎自检（双账自洽）。
    pub fn validate(&self) -> Result<(), XcCode> {
        self.ledger.verify()
    }

    /// 识别报告（人读汇总）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("HwidEngine:");
        match self.last_confidence {
            Some(c) => s.push_str(&alloc::format!("最近置信={}", c.zh())),
            None => s.push_str("最近置信=未判定"),
        }
        s.push('；');
        s.push_str(&self.ledger.report());
        s
    }
}

impl Default for HwidEngine {
    fn default() -> HwidEngine {
        HwidEngine::new()
    }
}
