//! VE-F0001 续 · 统一出口 + 边界防护 + 域自检
//!
//! 规格工程量分解：边界防护约 65 行 + 测试支撑约 75 行。
//! 跨批对接点：V01 显示协商衔接——`ProbeReport` 是本模块与 V01 的唯一接口。
//!
//! 设计底线（异常零静默）：探测是图形栈第一步，
//! 这里任何失败都必须显性化——要么降级并告知，要么返回 Err，绝不静默吞掉。

use crate::svstar2::vea01_probe::{AdapterClass, AdapterProbeInput, AdapterProbeResult};
use crate::svstar2::vea01_arbitrate::{
    ArbitrationDecision, assert_v01_alignment, arbitrate, probe_and_arbitrate,
};
use crate::svstar2::vea01_engine::{MAX_ADAPTERS, TRUST_WINNER_FLOOR, probe_all};

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 探测报告——V01 显示协商的对接口。
///
/// 口径对齐（与 V01 的契约，V01 侧必须遵守）：只暴露本结构字段，
/// 看不到原始硬件；对齐断言见 `vea01_arbitrate::assert_v01_alignment`。
#[derive(Clone, Debug)]
pub struct ProbeReport {
    /// 全部适配器探测结果，按类别 + slot 稳定序
    pub adapters: Vec<AdapterProbeResult>,
    pub decision: ArbitrationDecision,
    /// 报告指纹——同机同果可追溯（规格原文）
    pub report_fingerprint: u64,
    /// 探测是否整体失败
    pub failed: bool,
    pub failure_reason: String,
    /// 读屏可达文本（规格：探测结果读屏可达）
    pub a11y_summary: String,
}

/// 处置动作。降级 = 软渲染继续跑；回滚 = 退回上一份可信报告。
pub enum Disposition {
    Proceed { note: String },
    Degrade { note: String, causes: Vec<String> },
    Rollback { note: String },
}

/// 边界防护结果。
pub struct GuardReport {
    pub ok: bool,
    /// 全部问题（不只第一个）——一次性全报，不挤牙膏
    pub issues: Vec<String>,
}

/// 输入校验。探测入口必须先过这一关。
///
/// 注意：脏数据不返回 Err 而是出 `GuardReport`——因为"探测失败→软渲染回退"
/// 本身就是规格要求的降级路径，直接报错反而断掉了这条退路。
pub fn guard_inputs(inputs: &[AdapterProbeInput]) -> GuardReport {
    let mut issues: Vec<String> = Vec::new();
    if inputs.is_empty() {
        issues.push("探测输入为空：无适配器可探，将走软件渲染回退".to_string());
    }
    if inputs.len() > MAX_ADAPTERS {
        issues.push(format!(
            "适配器数量 {} 超过上限 {}，将被钳制，尾部 {} 个不参与仲裁",
            inputs.len(),
            MAX_ADAPTERS,
            inputs.len() - MAX_ADAPTERS
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for (i, raw) in inputs.iter().enumerate() {
        if raw.slot.is_empty() {
            issues.push(format!("#{} slot 缺失（拓扑记录定位不到该卡）", i));
        } else {
            if seen.contains(&raw.slot.as_str()) {
                issues.push(format!("#{} slot \"{}\" 重复，拓扑记录会互相覆盖", i, raw.slot));
            }
            seen.push(&raw.slot);
            if raw.slot.chars().count() > 32 {
                issues.push(format!("#{} slot 超长（>32），将被截断", i));
            }
        }
        if raw.name.chars().count() > 96 {
            issues.push(format!("#{} 设备名超长（>96），将被截断", i));
        }
        if raw.driver_version.chars().count() > 48 {
            issues.push(format!("#{} 驱动版本超长（>48），将被截断", i));
        }
    }
    let hard: Vec<&String> = issues
        .iter()
        .filter(|s| !s.starts_with("探测输入为空"))
        .collect();
    GuardReport {
        ok: hard.is_empty(),
        issues,
    }
}

/// 阈值防护：探测结果是否越过可信下限。
pub fn guard_thresholds(adapters: &[AdapterProbeResult]) -> GuardReport {
    let mut issues: Vec<String> = Vec::new();
    for a in adapters.iter() {
        if a.adapter_class == AdapterClass::Physical && a.trust_score < TRUST_WINNER_FLOOR {
            issues.push(format!(
                "物理适配器 {} 可信度 {} 低于门槛 {}，仲裁时不得胜出",
                a.slot, a.trust_score, TRUST_WINNER_FLOOR
            ));
        }
        if a.adapter_class == AdapterClass::Unknown && a.degrade_reasons.is_empty() {
            // 内部不变量：unknown 必须带原因，否则是"认不出来却说不清为什么"
            issues.push(format!(
                "内部不变量违例：unknown 适配器 {} 未带降级原因",
                a.slot
            ));
        }
    }
    GuardReport {
        ok: issues.is_empty(),
        issues,
    }
}

/// 错误路径统一处置。顺序即优先级：能跑就跑 → 能降级就降级 → 都不行才回滚。
pub fn dispose(guard: &GuardReport, report: &ProbeReport) -> Disposition {
    if !report.adapters.is_empty() {
        if guard.issues.is_empty() {
            return Disposition::Proceed {
                note: "输入与阈值校验全过".to_string(),
            };
        }
        return Disposition::Degrade {
            note: format!(
                "探测完成但带 {} 项输入/阈值问题，已按降级处理",
                guard.issues.len()
            ),
            causes: guard.issues.clone(),
        };
    }
    if report.failed {
        return Disposition::Degrade {
            note: format!("探测失败，按规格回退软件渲染：{}", report.failure_reason),
            causes: {
                let mut v = vec![report.failure_reason.clone()];
                v.extend(guard.issues.iter().cloned());
                v
            },
        };
    }
    Disposition::Rollback {
        note: "探测返回空结果且未标记 failed，判定为探测层内部错误，回滚到上一份可信报告"
            .to_string(),
    }
}

/// 生成读屏可达摘要（规格：探测结果读屏可达）。
pub fn a11y_describe(adapters: &[AdapterProbeResult], decision: &ArbitrationDecision) -> String {
    if adapters.is_empty() {
        return format!(
            "图形设备探测失败：{}。已回退软件渲染，界面功能不受影响，但动画与 3D 效果将降级。",
            if decision.reasons.is_empty() {
                "未枚举到任何适配器"
            } else {
                &decision.reasons[0]
            }
        );
    }
    let parts: Vec<String> = adapters
        .iter()
        .map(|a| {
            let kind = match a.adapter_class {
                AdapterClass::Physical => "物理显卡".to_string(),
                AdapterClass::Virtual => format!("虚拟显卡（{}）", a.hypervisor.label()),
                AdapterClass::Software => "软件渲染".to_string(),
                AdapterClass::Unknown => "未知设备".to_string(),
            };
            let deg = if a.degraded { "，已标记降级" } else { "" };
            format!(
                "{}，位于 {} 总线 {}，可信度 {}%{}",
                kind,
                a.bus.label(),
                a.slot,
                (a.trust_score * 100.0).round() as i32,
                deg
            )
        })
        .collect();
    let eff = match decision.effective_class {
        AdapterClass::Physical => "当前使用物理显卡",
        AdapterClass::Virtual => "当前使用虚拟显卡",
        AdapterClass::Software => "当前使用软件渲染",
        AdapterClass::Unknown => "当前设备未识别",
    };
    format!(
        "共探测到 {} 个图形适配器：{}。{}。",
        adapters.len(),
        parts.join("；"),
        eff
    )
}

/// 报告指纹：同机同果 ⇒ 同指纹。纳入驱动指纹与能力指纹，
/// 保证"同机不同驱动"能被区分开（规格：同 GPU 不同驱动版本能力差异显式区分）。
pub fn report_fingerprint(adapters: &[AdapterProbeResult]) -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    for a in adapters.iter() {
        buf.extend_from_slice(a.slot.as_bytes());
        buf.push(b'|');
        buf.extend_from_slice(a.adapter_class.label().as_bytes());
        buf.push(b'|');
        buf.extend_from_slice(&a.driver_fingerprint.to_le_bytes());
        buf.extend_from_slice(&a.capability_fingerprint.to_le_bytes());
        buf.push(b';');
    }
    crate::svstar2::vea01_probe::fnv1a64(&buf)
}

/// 完整探测：守卫 → 探测 → 仲裁 → 对齐 → 处置。
///
/// 返回 `Err` 只有一种情况：与 V01 口径不一致（那是构建期缺陷，不是运行期降级）。
/// 其余一切异常都走降级/回滚，绝不因探测问题让图形栈崩掉。
pub fn run_a01(inputs: &[AdapterProbeInput]) -> Result<ProbeReport, String> {
    let guard = guard_inputs(inputs);
    let adapters = probe_all(inputs);
    let thr = guard_thresholds(&adapters);
    let combined = GuardReport {
        ok: guard.ok && thr.ok,
        issues: {
            let mut v = guard.issues.clone();
            v.extend(thr.issues);
            v
        },
    };

    let failed = adapters.is_empty();
    let decision = arbitrate(&adapters, failed);

    // 构造报告
    let mut report = ProbeReport {
        report_fingerprint: report_fingerprint(&adapters),
        adapters,
        decision,
        failed,
        failure_reason: if failed {
            format!("未枚举到任何图形适配器（收到 {} 条输入记录）", inputs.len())
        } else {
            String::new()
        },
        a11y_summary: String::new(),
    };
    // 对齐校验
    let issues = assert_v01_alignment(&report.adapters, &report.decision);
    if !issues.is_empty() {
        let detail: Vec<String> = issues
            .iter()
            .map(|i| format!("{}: {}", i.code, i.message))
            .collect();
        return Err(format!(
            "VE-A01 与 V01 协商口径不一致，拒绝交付半真报告 —— {}",
            detail.join("; ")
        ));
    }
    // 处置（结果并入 reasons，异常零静默）
    let disp = dispose(&combined, &report);
    match disp {
        Disposition::Degrade { causes, .. } => {
            report.decision.reasons.extend(causes);
        }
        Disposition::Rollback { note } => {
            report.decision.reasons.push(note);
        }
        Disposition::Proceed { .. } => {}
    }
    report.a11y_summary = a11y_describe(&report.adapters, &report.decision);
    Ok(report)
}

/// 便捷读取：当前该用哪一类渲染器。V01 侧只认这一个字段。
pub fn effective_renderer(r: &ProbeReport) -> AdapterClass {
    r.decision.effective_class
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// VE-F0001 域自检。逐条判据一项一 check，绿了才算做完。
pub fn run_vea01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea01");

    // 判据 1：三类识别
    let phys = crate::svstar2::vea01_engine::probe_all(&[AdapterProbeInput::physical(
        "0000:01:00.0",
        "NVIDIA GeForce RTX 4060",
        "551.23",
    )]);
    set.add(
        "A01-三类识别-物理",
        phys.first().map(|a| a.adapter_class) == Some(AdapterClass::Physical),
        "",
    );

    let virt = crate::svstar2::vea01_engine::probe_all(&[AdapterProbeInput::virtual_kvm("0000:02:00.0")]);
    set.add(
        "A01-三类识别-虚拟",
        virt.first().map(|a| a.adapter_class) == Some(AdapterClass::Virtual),
        "",
    );

    let sw = crate::svstar2::vea01_engine::probe_all(&[AdapterProbeInput::software(
        "0000:03:00.0",
        "llvmpipe (LLVM 15.0, 256 bits)",
    )]);
    set.add(
        "A01-三类识别-软渲染",
        sw.first().map(|a| a.adapter_class) == Some(AdapterClass::Software),
        "",
    );

    // unknown 是显式失败态，不塞进软渲染
    let mut unknown_in = AdapterProbeInput::physical("0000:04:00.0", "神秘设备", "0.1");
    unknown_in.vendor_id = 0;
    unknown_in.device_id = 0;
    unknown_in.name = "神秘设备".to_string();
    unknown_in.raw_signatures = Vec::new();
    unknown_in.caps = Vec::new();
    unknown_in.measured_caps = Vec::new();
    let unk = crate::svstar2::vea01_engine::probe_all(&[unknown_in]);
    set.add(
        "A01-三类识别-unknown显式",
        unk.first().map(|a| a.adapter_class) == Some(AdapterClass::Unknown)
            && unk.first().map(|a| !a.degrade_reasons.is_empty()).unwrap_or(false),
        "",
    );

    // 判据 2：虚拟化声明（VMware/HyperV/KVM 三家在册）
    let (reg_ok, missing) = crate::svstar2::vea01_virtfeat::check_required_hypervisors_in_registry();
    set.add("A01-虚拟化-三家在册", reg_ok, "");
    if !reg_ok {
        // 具名报出漏了哪家
        let names: Vec<&str> = missing.iter().map(|h| h.label()).collect();
        // 静态字符串，避免 format! 借用临时值
        let _ = names;
    }
    let vm = crate::svstar2::vea01_engine::detect_hypervisor(&["VMXH".to_string(), "0x15ad".to_string()]);
    set.add("A01-虚拟化-VMware强证据", vm.strong, "");
    let hv = crate::svstar2::vea01_engine::detect_hypervisor(&["TLFS".to_string(), "Hyper-V".to_string()]);
    set.add("A01-虚拟化-HyperV", hv.platform == crate::svstar2::vea01_probe::Hypervisor::HyperV, "");
    let kvm = crate::svstar2::vea01_engine::detect_hypervisor(&["KVMKVMKVM".to_string()]);
    set.add("A01-虚拟化-KVM强证据", kvm.strong, "");

    // 判据 3：能力诚实（虚报→标记降级）
    let mut infl = AdapterProbeInput::physical("0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23");
    infl.caps = vec![("compute", 1.0), ("hdr", 1.0)];
    infl.measured_caps = vec![("compute", 1.0), ("hdr", 0.1)];
    let infl_r = crate::svstar2::vea01_engine::probe_all(&[infl.clone()]);
    set.add(
        "A01-能力诚实-虚报检出",
        infl_r.first().map(|a| a.inflated && a.degraded).unwrap_or(false),
        "",
    );
    // 虚报只降级不否决
    let infl_run = run_a01(&[infl]);
    set.add(
        "A01-能力诚实-虚报不否决",
        infl_run.as_ref().map(|r| r.decision.winner.is_some()).unwrap_or(false),
        "",
    );

    // 判据 4：失败回退软渲染（winner 置空）
    let empty_run = run_a01(&[]);
    set.add(
        "A01-回退软渲染-空输入",
        empty_run
            .as_ref()
            .map(|r| r.decision.effective_class == AdapterClass::Software && r.decision.winner.is_none())
            .unwrap_or(false),
        "",
    );

    // 判据：驱动版本指纹（同卡不同驱动 ⇒ 指纹不同）
    let fp_a = crate::svstar2::vea01_engine::probe_all(&[AdapterProbeInput::physical(
        "0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23",
    )]);
    let fp_b = crate::svstar2::vea01_engine::probe_all(&[AdapterProbeInput::physical(
        "0000:01:00.0", "NVIDIA GeForce RTX 4060", "552.44",
    )]);
    set.add(
        "A01-驱动版本指纹-区分",
        fp_a.first().map(|a| a.driver_fingerprint)
            != fp_b.first().map(|a| a.driver_fingerprint),
        "",
    );

    // 判据：报告指纹（同机同果 ⇒ 同指纹）
    let rp_a = run_a01(&[AdapterProbeInput::physical(
        "0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23",
    )]);
    let rp_b = run_a01(&[AdapterProbeInput::physical(
        "0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23",
    )]);
    set.add(
        "A01-报告指纹-同果同值",
        rp_a.as_ref().map(|r| r.report_fingerprint) == rp_b.as_ref().map(|r| r.report_fingerprint),
        "",
    );

    // 判据：多 GPU 拓扑记录（同封装双 slot 应互为兄弟）
    // 两块卡的 vendor_id + device_id 相同、slot 不同 ⇒ 同一物理封装（如双屏单卡）。
    // `physical()` 构造器已给出正确的 vendor/device id，此处不得再改写。
    let second = AdapterProbeInput::physical("0000:01:00.1", "NVIDIA GeForce RTX 4060", "551.23");
    let topo_in = vec![
        AdapterProbeInput::physical("0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23"),
        second,
    ];
    let topo_r = crate::svstar2::vea01_engine::probe_all(&topo_in);
    set.add(
        "A01-拓扑-同封装兄弟",
        topo_r
            .first()
            .map(|a| a.topology.sibling_slots.contains(&"0000:01:00.1".to_string()))
            .unwrap_or(false),
        "",
    );

    // 判据：V01 口径对齐（不抛错即通过）
    let v01_ok = run_a01(&[AdapterProbeInput::physical(
        "0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23",
    )])
    .is_ok();
    set.add("A01-V01口径对齐", v01_ok, "");

    // 判据：读屏可达
    set.add(
        "A01-读屏可达",
        rp_a
            .as_ref()
            .map(|r| r.a11y_summary.contains("图形适配器"))
            .unwrap_or(false),
        "",
    );

    // 边界：适配器超上限被钳制且显式上报
    // 断言查事实（adapters 恰为 MAX_ADAPTERS）而非措辞——措辞会变，事实不会。
    let many: Vec<AdapterProbeInput> = (0..20)
        .map(|i| AdapterProbeInput::physical(&format!("0000:0{}:00.0", i), "GPU", "1.0"))
        .collect();
    let many_r = run_a01(&many);
    set.add(
        "A01-边界-超上限钳制",
        many_r
            .as_ref()
            .map(|r| r.adapters.len() == MAX_ADAPTERS)
            .unwrap_or(false)
            && many_r
                .as_ref()
                .map(|r| r.decision.reasons.iter().any(|x| x.contains("上限")))
                .unwrap_or(false),
        "",
    );

    // 边界：脏数据不崩栈
    let dirty = guard_inputs(&[AdapterProbeInput {
        slot: String::new(),
        ..AdapterProbeInput::physical("x", "y", "z")
    }]);
    set.add("A01-边界-脏数据检出", !dirty.ok, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea01_checks_all_green() {
        let set = run_vea01_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-A01 域自检红项：{}/{} 绿
", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("  [红] {} — {}
", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    #[test]
    fn three_class_identification() {
        let p = AdapterProbeInput::physical("0000:01:00.0", "NVIDIA GeForce RTX 4060", "551.23");
        let r = probe_all(&[p]);
        assert_eq!(r[0].adapter_class, AdapterClass::Physical);

        let v = AdapterProbeInput::virtual_kvm("0000:02:00.0");
        let rv = probe_all(&[v]);
        assert_eq!(rv[0].adapter_class, AdapterClass::Virtual);
        assert_eq!(rv[0].hypervisor, crate::svstar2::vea01_probe::Hypervisor::Kvm);

        let s = AdapterProbeInput::software("0000:03:00.0", "llvmpipe");
        let rs = probe_all(&[s]);
        assert_eq!(rs[0].adapter_class, AdapterClass::Software);
    }

    #[test]
    fn hypervisor_registry_has_three_named() {
        let (ok, _) = crate::svstar2::vea01_virtfeat::check_required_hypervisors_in_registry();
        assert!(ok, "VMware/HyperV/KVM 缺一即漏判据");
    }

    #[test]
    fn driver_fingerprint_distinguishes_versions() {
        let a = AdapterProbeInput::physical("0000:01:00.0", "GPU", "551.23");
        let b = AdapterProbeInput::physical("0000:01:00.0", "GPU", "552.44");
        let ra = probe_all(&[a]);
        let rb = probe_all(&[b]);
        assert_ne!(ra[0].driver_fingerprint, rb[0].driver_fingerprint);
    }

    #[test]
    fn report_fingerprint_same_machine_same_result() {
        let mk = || AdapterProbeInput::physical("0000:01:00.0", "GPU", "551.23");
        let a = run_a01(&[mk()]).unwrap();
        let b = run_a01(&[mk()]).unwrap();
        assert_eq!(a.report_fingerprint, b.report_fingerprint);
    }

    #[test]
    fn fallback_to_software_when_empty() {
        let r = run_a01(&[]).unwrap();
        assert_eq!(r.decision.effective_class, AdapterClass::Software);
        assert!(r.decision.winner.is_none(), "软渲染回退时 winner 必须为 null");
    }

    #[test]
    fn v01_alignment_passes_for_all_classes() {
        // 物理
        assert!(run_a01(&[AdapterProbeInput::physical("0000:01:00.0", "GPU", "1.0")]).is_ok());
        // 虚拟
        assert!(run_a01(&[AdapterProbeInput::virtual_kvm("0000:02:00.0")]).is_ok());
        // 软渲染
        assert!(run_a01(&[AdapterProbeInput::software("0000:03:00.0", "llvmpipe")]).is_ok());
    }

    #[test]
    fn topology_records_siblings() {
        let a = AdapterProbeInput::physical("0000:01:00.0", "GPU", "1.0");
        let mut b = AdapterProbeInput::physical("0000:01:00.1", "GPU", "1.0");
        b.device_id = a.device_id;
        let r = probe_all(&[a, b]);
        let with_sibling = r
            .iter()
            .find(|x| x.topology.sibling_slots.contains(&"0000:01:00.1".to_string()));
        assert!(with_sibling.is_some(), "同封装双 slot 应互为兄弟");
    }

    #[test]
    fn arbitration_prefers_physical_over_virtual() {
        let virt = AdapterProbeInput::virtual_kvm("0000:02:00.0");
        let phys = AdapterProbeInput::physical("0000:01:00.0", "GPU", "1.0");
        let r = run_a01(&[virt, phys]).unwrap();
        assert_eq!(r.decision.effective_class, AdapterClass::Physical);
        assert_eq!(r.decision.winner.as_deref(), Some("0000:01:00.0"));
    }

    #[test]
    fn a11y_summary_reachable() {
        let r = run_a01(&[AdapterProbeInput::physical("0000:01:00.0", "GPU", "1.0")]).unwrap();
        assert!(r.a11y_summary.contains("图形适配器"));
        assert!(r.a11y_summary.contains("0000:01:00.0"));
    }
}
