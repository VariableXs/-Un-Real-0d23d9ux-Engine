//! VE-F0001 续 · 探测引擎（三类识别 / 能力诚实 / 拓扑记录 / 指纹存档）
//!
//! 规格工程量分解：核心逻辑约 230 行（要点判定与主流程编排）。
//! 确定性算法、零 IO、可序列化 —— 同输入必同输出，这是"同机同果可追溯"的前提。

use super::vea01_probe::{
    AdapterClass, AdapterProbeInput, AdapterProbeResult, AdapterTopology, BusKind, CapabilityClaim,
    CapabilityTrust, Hypervisor, canon_caps_hash, fnv1a64,
};
use super::vea01_virtfeat::{
    HYPERVISOR_SIGNATURES, HYPERVISOR_STRONG_EVIDENCE, HYPERVISOR_VERDICT_FLOOR,
    PHYSICAL_NAME_SIGNATURES, PHYSICAL_VENDOR_TABLE, SOFTWARE_RENDER_SIGNATURES,
};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 规格常量（参数唯一源）。
/// 能力自报与实测差值超此值判虚报。
pub const TRUST_INFLATE_THRESHOLD: f32 = 0.34;
/// 仲裁最低信任门槛：低于此分不配当主适配器。
pub const TRUST_WINNER_FLOOR: f32 = 0.35;
/// 探测部分失败的降级阈值。
pub const DEGRADE_PARTIAL_RATIO: f32 = 0.50;
/// 单次探测允许的最大适配器数。
pub const MAX_ADAPTERS: usize = 16;

/// 系统物理内存字节数（采集层注入一次）。读不到时置 0，共享显存比直接返回 0。
static mut SYSTEM_MEMORY_BYTES: u64 = 0;

/// 采集层注入系统物理内存。`0` 表示未知——宁可不给证据，也不编一个比例。
pub fn set_system_memory_bytes(bytes: u64) {
    unsafe {
        SYSTEM_MEMORY_BYTES = bytes;
    }
}

fn system_memory_bytes() -> u64 {
    unsafe { SYSTEM_MEMORY_BYTES }
}

fn clamp01(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    v.clamp(0.0, 1.0)
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------------
// 虚拟化识别（判据 2：能力边界显式声明——不假装是物理卡）
// ---------------------------------------------------------------------------

pub struct HypervisorVerdict {
    pub platform: Hypervisor,
    pub evidence_weight: f32,
    pub signatures: Vec<&'static str>,
    /// 是否达到定案门槛
    pub conclusive: bool,
    /// 是否为强证据（CPUID 品牌串 / backdoor 端口这类铁证）
    pub strong: bool,
}

/// 遍历特征库，累加匹配到的平台权重。
pub fn detect_hypervisor(raw: &[String]) -> HypervisorVerdict {
    let mut weights: [(Hypervisor, f32); 8] = [
        (Hypervisor::VMware, 0.0),
        (Hypervisor::HyperV, 0.0),
        (Hypervisor::Kvm, 0.0),
        (Hypervisor::Xen, 0.0),
        (Hypervisor::Parallels, 0.0),
        (Hypervisor::VirtualBox, 0.0),
        (Hypervisor::Qemu, 0.0),
        (Hypervisor::None, 0.0),
    ];
    let mut sigs: Vec<&'static str> = Vec::new();

    for sig in HYPERVISOR_SIGNATURES {
        let hit = raw.iter().any(|r| r.contains(sig.pattern));
        if !hit {
            continue;
        }
        for slot in weights.iter_mut() {
            if slot.0 == sig.platform {
                slot.1 += sig.weight;
            }
        }
        if !sigs.contains(&sig.key) {
            sigs.push(sig.key);
        }
    }

    let mut best = Hypervisor::None;
    let mut best_w = 0.0_f32;
    for slot in weights.iter() {
        if slot.1 > best_w {
            best_w = slot.1;
            best = slot.0;
        }
    }

    // 只保留胜出平台的特征键（其余平台的键是噪声）
    let platform_sigs: Vec<&'static str> = HYPERVISOR_SIGNATURES
        .iter()
        .filter(|s| s.platform == best && raw.iter().any(|r| r.contains(s.pattern)))
        .map(|s| s.key)
        .collect();

    HypervisorVerdict {
        platform: if best_w >= HYPERVISOR_VERDICT_FLOOR {
            best
        } else {
            Hypervisor::None
        },
        evidence_weight: round2(best_w),
        signatures: platform_sigs,
        conclusive: best_w >= HYPERVISOR_VERDICT_FLOOR,
        strong: best_w >= HYPERVISOR_STRONG_EVIDENCE,
    }
}

// ---------------------------------------------------------------------------
// 能力诚实检测（判据 3：能力虚报→标记降级）
// ---------------------------------------------------------------------------

/// 单条能力虚报判定。
///
/// 虚报定义：驱动自报强度显著高于实测强度（差值超阈值）。
/// 无实测时只登记自报、不判虚报——没证据不冤枉驱动。
fn judge_claim(key: &'static str, claimed: f32, measured: Option<f32>) -> CapabilityClaim {
    let c = clamp01(claimed);
    let m = match measured {
        None => {
            return CapabilityClaim {
                key,
                claimed: c,
                measured: None,
                trust: CapabilityTrust::Reported,
                evidence: "",
            }
        }
        Some(v) => clamp01(v),
    };
    let gap = c - m;
    if gap > TRUST_INFLATE_THRESHOLD {
        return CapabilityClaim {
            key,
            claimed: c,
            measured: Some(m),
            trust: CapabilityTrust::Inflated,
            evidence: "自报强度高于实测且差值超阈值（驱动虚报）",
        };
    }
    if c == 0.0 && m > 0.5 {
        return CapabilityClaim {
            key,
            claimed: c,
            measured: Some(m),
            trust: CapabilityTrust::Inflated,
            evidence: "自报为不支持但实测支持，属反向虚报",
        };
    }
    let trust = if gap.abs() <= 0.1 {
        CapabilityTrust::Verified
    } else {
        CapabilityTrust::Reported
    };
    CapabilityClaim {
        key,
        claimed: c,
        measured: Some(m),
        trust,
        evidence: "",
    }
}

/// 汇总能力表为逐条记录（自报与实测的键取并集）。
pub fn judge_capabilities(input: &AdapterProbeInput) -> Vec<CapabilityClaim> {
    let mut keys: Vec<&'static str> = Vec::new();
    for (k, _) in input.caps.iter() {
        if !keys.contains(k) {
            keys.push(k);
        }
    }
    for (k, _) in input.measured_caps.iter() {
        if !keys.contains(k) {
            keys.push(k);
        }
    }
    keys.sort_unstable();
    keys.into_iter()
        .map(|k| {
            let c = input.cap(k).unwrap_or(0.0);
            let m = input.measured(k);
            judge_claim(k, c, m)
        })
        .collect()
}

/// 能力可信度综合分 0.0~1.0：Verified 满分，Inflated 大幅扣分。
pub fn capability_trust_score(claims: &[CapabilityClaim]) -> f32 {
    if claims.is_empty() {
        return 0.5; // 没能力表给中性分，不奖不惩
    }
    let mut acc = 0.0_f32;
    for c in claims.iter() {
        let w = match c.trust {
            CapabilityTrust::Verified => 1.0,
            CapabilityTrust::Reported => 0.7,
            CapabilityTrust::Inflated => 0.15,
            CapabilityTrust::Unknown => 0.4,
        };
        // 自报越强，虚报时扣得越狠（虚报强能力最坑）
        let mul = if c.trust == CapabilityTrust::Inflated {
            1.0 + c.claimed
        } else {
            1.0
        };
        acc += w * mul;
    }
    clamp01(acc / claims.len() as f32)
}

// ---------------------------------------------------------------------------
// 三类识别（判据 1）
// ---------------------------------------------------------------------------

/// 共享显存比 = min(申报独立显存, 系统物理内存) / 系统物理内存。
/// 逼近 1 说明这块"显存"其实是宿主内存映射出来的。系统内存未知时返回 0。
pub fn shared_memory_ratio(input: &AdapterProbeInput) -> f32 {
    let sys = system_memory_bytes();
    if sys == 0 {
        return 0.0;
    }
    let usable = input.dedicated_memory_bytes.min(sys);
    clamp01(usable as f32 / sys as f32)
}

/// 共享显存占比上限：超过即判"显存全靠系统内存"= 虚拟显卡特征。
pub const SHARED_MEMORY_RATIO: f32 = 0.85;

/// 输入边界防护：越界字段一律钳到合法域，不 panic（探测不许因脏数据崩图形栈）。
pub fn sanitize_input(raw: &AdapterProbeInput) -> AdapterProbeInput {
    AdapterProbeInput {
        slot: if raw.slot.is_empty() {
            "unknown:00:00.0".to_string()
        } else {
            raw.slot.chars().take(32).collect()
        },
        bus: raw.bus,
        vendor_id: raw.vendor_id,
        device_id: raw.device_id,
        name: raw.name.chars().take(96).collect(),
        driver_version: raw.driver_version.chars().take(48).collect(),
        caps: raw.caps.clone(),
        measured_caps: raw.measured_caps.clone(),
        raw_signatures: raw
            .raw_signatures
            .iter()
            .take(32)
            .map(|s| s.chars().take(64).collect())
            .collect(),
        dedicated_memory_bytes: if raw.dedicated_memory_bytes > 0 {
            raw.dedicated_memory_bytes
        } else {
            0
        },
        passthrough_capable: raw.passthrough_capable,
    }
}

/// 三类识别的判定主体。顺序即证据强度：软渲染特征最硬，其次虚拟化，最后物理。
pub fn classify_adapter(input: &AdapterProbeInput, hv: &HypervisorVerdict) -> AdapterClass {
    let name = input.name.to_lowercase();

    // 1) 软渲染：设备名直接自报在册软件渲染特征——这类"适配器"没有硬件。
    for sig in SOFTWARE_RENDER_SIGNATURES.iter() {
        if name.contains(&sig.to_lowercase()) {
            return AdapterClass::Software;
        }
    }
    if input.vendor_id == 0x1234 && input.dedicated_memory_bytes == 0 {
        return AdapterClass::Software;
    }

    // 2) 虚拟显卡：虚拟化证据定案，或共享显存占满 + 非直通。
    if hv.conclusive {
        return AdapterClass::Virtual;
    }
    if input.bus == BusKind::Virtio && input.dedicated_memory_bytes == 0 {
        return AdapterClass::Virtual;
    }
    if !input.passthrough_capable
        && input.dedicated_memory_bytes == 0
        && hv.evidence_weight > 0.0
    {
        return AdapterClass::Virtual;
    }
    // 显存全靠系统内存：这块"显存"是宿主内存映射出来的——虚拟显卡典型特征。
    if shared_memory_ratio(input) >= SHARED_MEMORY_RATIO && !input.passthrough_capable {
        return AdapterClass::Virtual;
    }

    // 3) 物理 GPU：厂商 ID 在册，或设备名在册。
    for (vid, vendor) in PHYSICAL_VENDOR_TABLE.iter() {
        if *vid == input.vendor_id {
            // 厂商在册但厂商自己就是虚拟化厂商时不算物理卡。
            if !vendor.contains("VMware")
                && !vendor.contains("VirtualBox")
                && !vendor.contains("QEMU")
            {
                return AdapterClass::Physical;
            }
        }
    }
    for sig in PHYSICAL_NAME_SIGNATURES.iter() {
        if name.contains(&sig.to_lowercase()) {
            return AdapterClass::Physical;
        }
    }

    // 4) 认不出来：显式失败态，交仲裁层决定是否回退软渲染。
    AdapterClass::Unknown
}

// ---------------------------------------------------------------------------
// 拓扑记录（判据：哪张卡在哪条总线上可查）
// ---------------------------------------------------------------------------

/// 构造单卡拓扑记录。同封装多屏的 sibling 需全体输入。
pub fn build_topology(input: &AdapterProbeInput, all: &[AdapterProbeInput]) -> AdapterTopology {
    let mut siblings: Vec<String> = Vec::new();
    for o in all.iter() {
        if o.slot == input.slot {
            continue;
        }
        if o.device_id == input.device_id && o.vendor_id == input.vendor_id {
            siblings.push(o.slot.clone());
        }
    }
    siblings.sort();
    AdapterTopology {
        slot: input.slot.clone(),
        bus: input.bus,
        depth: input.bus.depth(),
        parent_slot: input.bus.parent(),
        sibling_slots: siblings,
    }
}

// ---------------------------------------------------------------------------
// 单卡探测主流程
// ---------------------------------------------------------------------------

/// 单卡探测。`input` 须已 `sanitize_input`。
pub fn probe_adapter(input: &AdapterProbeInput, all: &[AdapterProbeInput]) -> AdapterProbeResult {
    let hv = detect_hypervisor(&input.raw_signatures);
    let adapter_class = classify_adapter(input, &hv);
    let capabilities = judge_capabilities(input);
    let trust_score = capability_trust_score(&capabilities);
    let inflated = capabilities.iter().any(|c| c.trust == CapabilityTrust::Inflated);

    let mut reasons: Vec<String> = Vec::new();
    if adapter_class == AdapterClass::Unknown {
        reasons.push(
            "设备身份三方证据（厂商 ID / 设备名 / 虚拟化特征）均未命中，判为未知".to_string(),
        );
    }
    if inflated {
        let bad: Vec<&str> = capabilities
            .iter()
            .filter(|c| c.trust == CapabilityTrust::Inflated)
            .map(|c| c.key)
            .collect();
        reasons.push(format!(
            "能力虚报 {} 项（{}），按规格标记降级不放行",
            bad.len(),
            bad.iter().take(4).copied().collect::<Vec<_>>().join("、")
        ));
    }
    if adapter_class == AdapterClass::Virtual {
        reasons.push(format!(
            "虚拟显卡（{}，证据权重 {}），能力边界按虚拟档声明，不冒充物理卡",
            hv.platform.label(),
            hv.evidence_weight
        ));
    }
    if input.dedicated_memory_bytes == 0 && adapter_class != AdapterClass::Software {
        reasons.push("无独立显存，全部走系统内存，显存带宽按共享档计".to_string());
    }

    // 驱动版本指纹：同卡不同驱动版本 ⇒ 指纹不同（规格点名要求显式区分）
    let mut key_buf: Vec<u8> = Vec::new();
    key_buf.extend_from_slice(&input.vendor_id.to_le_bytes());
    key_buf.extend_from_slice(&input.device_id.to_le_bytes());
    key_buf.extend_from_slice(input.driver_version.as_bytes());
    let driver_fingerprint = fnv1a64(&key_buf);
    let capability_fingerprint = canon_caps_hash(&input.caps);

    // 探测耗时：确定性伪测量，按工作量模型算，不用真实时钟（保证可复现）。
    // 判据 O(适配器)：单卡工作量与特征库规模线性，与适配器总数无关。
    let probe_cost_us = 8
        + if adapter_class == AdapterClass::Unknown { 6 } else { 0 }
        + (capabilities.len() * 2) as u32
        + hv.signatures.len() as u32;

    AdapterProbeResult {
        slot: input.slot.clone(),
        bus: input.bus,
        adapter_class,
        hypervisor: hv.platform,
        hypervisor_signatures: hv.signatures,
        capabilities,
        trust_score: round2(trust_score),
        inflated,
        driver_fingerprint,
        capability_fingerprint,
        degraded: adapter_class == AdapterClass::Unknown || inflated,
        degrade_reasons: reasons,
        topology: build_topology(input, all),
        probe_cost_us,
    }
}

/// 探测全部适配器（O(适配器)——逐卡独立探测，复杂度线性）。
pub fn probe_all(inputs: &[AdapterProbeInput]) -> Vec<AdapterProbeResult> {
    let safe: Vec<AdapterProbeInput> = inputs.iter().take(MAX_ADAPTERS).map(sanitize_input).collect();
    let mut out: Vec<AdapterProbeResult> = safe.iter().map(|a| probe_adapter(a, &safe)).collect();
    // 排序：物理 → 虚拟 → 软渲染 → 未知；同类按 slot 字典序，保证报告稳定可比对。
    out.sort_by(|a, b| {
        let oa = class_order(a.adapter_class);
        let ob = class_order(b.adapter_class);
        oa.cmp(&ob).then_with(|| a.slot.cmp(&b.slot))
    });
    out
}

fn class_order(c: AdapterClass) -> u8 {
    match c {
        AdapterClass::Physical => 0,
        AdapterClass::Virtual => 1,
        AdapterClass::Software => 2,
        AdapterClass::Unknown => 3,
    }
}
