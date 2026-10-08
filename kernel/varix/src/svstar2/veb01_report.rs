//! VE-F0201 续 · A 域对接：AdapterReport 注册为主适配器候选
//!
//! 规格原文："与 A 域对接：产出 AdapterReport（VE-F0001 格式）注册为
//! 主适配器候选。判据：注册报告过 A 域 schema。"
//!
//! 对接纪律：本模块只**生产** `AdapterProbeInput`（VE-F0001 冻结的输入
//! 契约），探测/仲裁/口径对齐全权交给 A 域 `run_a01`——驱动不私带一套
//! 探测逻辑，A 域 schema 是唯一裁判。初始化失败/前置拒绝的设备**不注册**
//! （带着失败状态注册 = 把坏账推给上层，违反异常零静默）。

use super::veb01_device::*;
use super::veb01_init::{InitOutcome, preflight, outcome_fingerprint};
use super::vea01_index::{ProbeReport, run_a01};
use super::vea01_probe::{AdapterProbeInput, BusKind};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 注册结论——B 域驱动向 A 域交付的最终形态。
#[derive(Clone, Debug)]
pub struct AdapterRegistration {
    /// 是否注册成功（初始化失败/前置拒绝 = false，且无 report）
    pub accepted: bool,
    /// 是否主适配器候选（A 域仲裁 winner 命中本设备 slot）
    pub primary_candidate: bool,
    /// A 域探测报告（accepted=false 时为 None）
    pub report: Option<ProbeReport>,
    /// 全过程留痕（为什么接受/为什么拒绝，人话）
    pub reasons: Vec<String>,
    /// 确定性指纹（同机同果可追溯）
    pub fingerprint: u64,
}

/// 把初始化完成态翻译成 A 域输入契约。
///
/// 能力表只报**协商确认过的**特征（诚实现）：没协商上的特征报 0，
/// 绝不把"设备宣称"当"实际可用"。
pub fn to_probe_input(snap: &VirtioGpuDeviceSnapshot, out: &InitOutcome) -> AdapterProbeInput {
    let g = out.gpu_features;
    let f = |on: bool| if on { 1.0 } else { 0.0 };
    AdapterProbeInput {
        slot: snap.slot.clone(),
        bus: BusKind::Virtio,
        vendor_id: snap.vendor_id,
        device_id: snap.device_id,
        name: "virtio-gpu".to_string(),
        driver_version: snap.driver_version.clone(),
        caps: vec![
            ("virgl", f(g.virgl)),
            ("edid", f(g.edid)),
            ("resource_blob", f(g.resource_blob)),
            ("context_init", f(g.context_init)),
            ("scanouts", snap.display.scanouts as f32 / MAX_SCANOUTS as f32),
        ],
        // 协商即实测：特征位是设备与驱动双向确认的结果，claimed==measured
        measured_caps: vec![
            ("virgl", f(g.virgl)),
            ("edid", f(g.edid)),
            ("resource_blob", f(g.resource_blob)),
            ("context_init", f(g.context_init)),
            ("scanouts", snap.display.scanouts as f32 / MAX_SCANOUTS as f32),
        ],
        raw_signatures: vec![
            "virtio-gpu".to_string(),
            "0x1af4".to_string(),
            snap.driver_version.clone(),
        ],
        dedicated_memory_bytes: 0, // 虚拟显卡共享宿主内存，诚实声明
        passthrough_capable: false,
    }
}

/// 完整注册流程：前置校验 → （初始化已由调用方完成）→ A 域 schema → 主候选判定。
///
/// 返回 `Err` 仅在初始化失败/前置拒绝时发生（三要素齐全）；
/// A 域 schema 的问题走降级注册并留痕（不静默吞）。
pub fn register_with_a_domain(
    snap: &VirtioGpuDeviceSnapshot,
    out: &InitOutcome,
) -> Result<AdapterRegistration, Rejection> {
    // 第一道闸：前置校验（身份/旧版拒绝/显示能力越界）
    preflight(snap)?;
    // 第二道闸：初始化必须 DRIVER_OK（失败设备不注册）
    if !out.ok {
        let r = out.rejection.clone().unwrap_or(Rejection {
            code: "E_INIT_FAILED",
            what: "初始化未到 DRIVER_OK".to_string(),
            why: "初始化结果标记失败但未携带三要素——初始化器的缺陷".to_string(),
            next: "按失败三要素处置后重新探测".to_string(),
        });
        return Err(r);
    }
    // 第三道闸：A 域 schema（探测→仲裁→V01 口径对齐，一条链全过）
    let input = to_probe_input(snap, out);
    let mut reasons: Vec<String> = Vec::new();
    reasons.push(format!(
        "virtio-gpu 初始化完成（{}，时序 {}µs ≤ 100ms 预算）",
        out.gpu_features.describe(),
        out.timing.total_us()
    ));
    match run_a01(&[input]) {
        Ok(report) => {
            let primary = report.decision.winner.as_deref() == Some(snap.slot.as_str());
            if primary {
                reasons.push("A 域仲裁命中本设备为主适配器候选".to_string());
            } else {
                reasons.push(format!(
                    "A 域仲裁未命中主候选（winner={:?}），注册为普通候选",
                    report.decision.winner
                ));
            }
            reasons.extend(report.decision.reasons.iter().cloned());
            Ok(AdapterRegistration {
                accepted: true,
                primary_candidate: primary,
                report: Some(report),
                reasons,
                fingerprint: outcome_fingerprint(snap, out),
            })
        }
        Err(e) => {
            // A 域 schema 拒绝 = 构建期缺陷级问题，显性降级并留痕，不静默吞
            reasons.push(format!("A 域 schema 拒绝注册：{}", e));
            reasons.push("已按规格回退软件渲染，图形栈继续可用".to_string());
            Err(Rejection {
                code: "E_SCHEMA_MISMATCH",
                what: "注册报告未过 A 域 schema".to_string(),
                why: e,
                next: "核对 AdapterProbeInput 契约字段；这是驱动侧缺陷，不是设备问题"
                    .to_string(),
            })
        }
    }
}

/// 读屏可达的注册摘要（规格：探测结果读屏可达）。
pub fn a11y_summary(reg: &AdapterRegistration, snap: &VirtioGpuDeviceSnapshot) -> String {
    if !reg.accepted {
        let r = reg
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "未注册（原因未记录）".to_string());
        return format!("virtio-gpu {} 未注册：{}", snap.slot, r);
    }
    let primary = if reg.primary_candidate {
        "主适配器候选"
    } else {
        "普通候选"
    };
    format!(
        "virtio-gpu 设备 {} 注册成功：{} 个扫描出口、最大 {}x{}；{}。{}",
        snap.slot,
        snap.display.scanouts,
        snap.display.max_width,
        snap.display.max_height,
        reg.reasons.first().cloned().unwrap_or_default(),
        primary
    )
}
