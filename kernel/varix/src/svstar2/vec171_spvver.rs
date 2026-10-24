//! VE-F0571 · SPIR-V 版本与扩展策略
//!
//! 版本矩阵（SPIR-V 1.0 至 1.6 乘 Vulkan 1.0 至 1.3 对应表——模块头版本号与
//! Capability 联动校验）；策略：默认目标最低可用版本（1.0 基线最大化兼容），
//! 特性按需提升并显性登记版本提升点；扩展最小启用原则与 F0562 Capability
//! 最小化同哲学（仅在使用其指令时启用）；降级路径：高版本特性在低版本目标的
//! 替代实现登记，不可降级显性不支持。
//!
//! 跨批对接：上游 F0562 骨架、F0563 能力；下游 F0577 Vulkan 远景、F0572 套件。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// SPIR-V 版本
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SpvVer {
    V10,
    V11,
    V12,
    V13,
    V14,
    V15,
    V16,
}

impl SpvVer {
    pub fn encoded(self) -> u32 {
        let minor = match self {
            SpvVer::V10 => 0,
            SpvVer::V11 => 1,
            SpvVer::V12 => 2,
            SpvVer::V13 => 3,
            SpvVer::V14 => 4,
            SpvVer::V15 => 5,
            SpvVer::V16 => 6,
        };
        (1u32 << 16) | (minor << 8)
    }
}

/// Vulkan 版本
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum VkVer {
    Vk10,
    Vk11,
    Vk12,
    Vk13,
}

/// 版本矩阵：SPIR-V 版本 × Vulkan 版本 → 最低 SPIR-V 要求
/// Vulkan 1.0 → 1.0；1.1 → 1.3；1.2 → 1.5；1.3 → 1.6
pub fn min_spv_for_vk(vk: VkVer) -> SpvVer {
    match vk {
        VkVer::Vk10 => SpvVer::V10,
        VkVer::Vk11 => SpvVer::V13,
        VkVer::Vk12 => SpvVer::V15,
        VkVer::Vk13 => SpvVer::V16,
    }
}

/// 版本与 Capability 联动校验
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapRequirement {
    /// Shader：1.0 即可
    Shader,
    /// 物理存储缓冲区指针：1.3+
    PhysicalStorageBuffer,
    /// 简易着色：1.4+
    ShaderClock,
    /// 需 1.6 特性
    Newest,
}

impl CapRequirement {
    pub fn min_ver(self) -> SpvVer {
        match self {
            CapRequirement::Shader => SpvVer::V10,
            CapRequirement::PhysicalStorageBuffer => SpvVer::V13,
            CapRequirement::ShaderClock => SpvVer::V14,
            CapRequirement::Newest => SpvVer::V16,
        }
    }
}

/// 编译期拒绝：版本低于能力要求
pub fn ver_cap_conflict(target: SpvVer, reqs: &[CapRequirement]) -> bool {
    reqs.iter().any(|r| r.min_ver() > target)
}

/// 版本提升账本：特性按需提升并显性登记提升点
#[derive(Clone, Debug, Default)]
pub struct LiftLedger {
    pub lifts: Vec<(String, SpvVer)>,
}

impl LiftLedger {
    pub fn new() -> Self {
        LiftLedger::default()
    }

    /// 显性登记版本提升点
    pub fn record(&mut self, feature: &str, ver: SpvVer) -> bool {
        if feature.is_empty() {
            return false;
        }
        if self.lifts.iter().any(|(f, _)| f == feature) {
            return false;
        }
        self.lifts.push((feature.to_string(), ver));
        true
    }

    /// 目标版本 = max(基线, 各提升点)
    pub fn effective_target(&self, baseline: SpvVer) -> SpvVer {
        self.lifts.iter().map(|(_, v)| *v).fold(baseline, |a, b| if b > a { b } else { a })
    }

    pub fn len(&self) -> usize {
        self.lifts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lifts.is_empty()
    }
}

/// 扩展启用账本：最小启用原则（仅在使用其指令时启用）
#[derive(Clone, Debug, Default)]
pub struct ExtLedger {
    /// (扩展名, 使用的指令标识)
    pub enabled: Vec<(String, String)>,
    /// 审计告警数
    pub audits: u32,
}

impl ExtLedger {
    pub fn new() -> Self {
        ExtLedger::default()
    }

    pub fn enable(&mut self, ext: &str, inst: &str) -> bool {
        if ext.is_empty() || self.enabled.iter().any(|(e, _)| e == ext) {
            return false;
        }
        self.enabled.push((ext.to_string(), inst.to_string()));
        true
    }

    /// 扩展滥用审计：未绑定任何指令的启用 → 告警
    pub fn audit_unused(&mut self) -> u32 {
        let n = self.enabled.iter().filter(|(_, i)| i.is_empty()).count();
        self.audits += n as u32;
        n as u32
    }

    pub fn has(&self, ext: &str) -> bool {
        self.enabled.iter().any(|(e, _)| e == ext)
    }
}

/// 降级替代表：高版本特性在低版本目标的替代实现登记
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Downgrade {
    /// 有替代实现（登记替代路径名）
    Alt(&'static str),
    /// 不可降级：显性不支持
    Unsupported,
}

/// 降级查询：高版本特性 → 低版本目标的替代
pub fn downgrade(cap: CapRequirement, target: SpvVer) -> Option<Downgrade> {
    if cap.min_ver() <= target {
        return None; // 无需降级
    }
    match cap {
        CapRequirement::Shader => None,
        CapRequirement::PhysicalStorageBuffer => Some(Downgrade::Alt("uniform-buffer-fallback")),
        CapRequirement::ShaderClock => Some(Downgrade::Alt("cpu-timing-fallback")),
        CapRequirement::Newest => Some(Downgrade::Unsupported),
    }
}

pub fn run_vec171_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec171");
    // 族一：版本矩阵
    s.add("矩阵-Vk10对应1.0", min_spv_for_vk(VkVer::Vk10) == SpvVer::V10, "m0");
    s.add("矩阵-Vk11对应1.3", min_spv_for_vk(VkVer::Vk11) == SpvVer::V13, "m1");
    s.add("矩阵-Vk12对应1.5", min_spv_for_vk(VkVer::Vk12) == SpvVer::V15, "m2");
    s.add("矩阵-Vk13对应1.6", min_spv_for_vk(VkVer::Vk13) == SpvVer::V16, "m3");
    s.add("矩阵-单调", min_spv_for_vk(VkVer::Vk10) < min_spv_for_vk(VkVer::Vk13), "mono");
    // 族二：编码
    s.add("编码-1.0基线", SpvVer::V10.encoded() == 0x0001_0000, "e0");
    s.add("编码-1.3", SpvVer::V13.encoded() == 0x0001_0300, "e3");
    // 族三：版本与 Capability 联动校验
    s.add("联动-Shader最低1.0", CapRequirement::Shader.min_ver() == SpvVer::V10, "c0");
    s.add("联动-指针1.3", CapRequirement::PhysicalStorageBuffer.min_ver() == SpvVer::V13, "c1");
    s.add("联动-冲突检出", ver_cap_conflict(SpvVer::V10, &[CapRequirement::PhysicalStorageBuffer]), "conf");
    s.add("联动-满足无冲突", !ver_cap_conflict(SpvVer::V13, &[CapRequirement::PhysicalStorageBuffer]), "ok");
    s.add("联动-多要求任一冲突", ver_cap_conflict(SpvVer::V15, &[CapRequirement::Shader, CapRequirement::Newest]), "multi");
    // 族四：最低基线策略
    let mut led = LiftLedger::new();
    s.add("基线-默认1.0", led.effective_target(SpvVer::V10) == SpvVer::V10, "base");
    s.add("提升-登记生效", led.record("ptr-buffer", SpvVer::V13), "lift");
    s.add("提升-目标抬升", led.effective_target(SpvVer::V10) == SpvVer::V13, "eff");
    s.add("提升-重复拒绝", !led.record("ptr-buffer", SpvVer::V14), "dup");
    s.add("提升-空名拒绝", !led.record("", SpvVer::V12), "empty");
    s.add("提升-取最高点", led.effective_target(SpvVer::V14) == SpvVer::V14, "max");
    // 族五：扩展最小启用
    let mut ext = ExtLedger::new();
    s.add("扩展-启用", ext.enable("SPV_KHR_8bit", "op-8bit"), "en");
    s.add("扩展-重复拒绝", !ext.enable("SPV_KHR_8bit", "x"), "dup");
    s.add("扩展-在册查询", ext.has("SPV_KHR_8bit") && !ext.has("SPV_KHR_16bit"), "has");
    ext.enable("SPV_EXT_unused", "");
    s.add("扩展-滥用审计", ext.audit_unused() == 1 && ext.audits == 1, "audit");
    // 族六：降级路径
    let d1 = downgrade(CapRequirement::PhysicalStorageBuffer, SpvVer::V10);
    s.add("降级-指针有替代", d1 == Some(Downgrade::Alt("uniform-buffer-fallback")), "alt");
    let d2 = downgrade(CapRequirement::Newest, SpvVer::V10);
    s.add("降级-不可降级显性", d2 == Some(Downgrade::Unsupported), "unsup");
    s.add("降级-无需降级", downgrade(CapRequirement::Shader, SpvVer::V10).is_none(), "noneed");
    let d3 = downgrade(CapRequirement::ShaderClock, SpvVer::V13);
    s.add("降级-时钟替代", d3 == Some(Downgrade::Alt("cpu-timing-fallback")), "clk");
    s
}
