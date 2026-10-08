//! VE-F0205 · 域自检（判据逐条对应，见 `veb05_virgl.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 协商四态正确 → `B05-协商-*`
//! - 能力映射与 host 实测一致 → `B05-映射-*`
//! - 超能力请求创建期拒绝 → `B05-拒绝-*`
//! - 降级链联动 → `B05-降级-*`
//! - 协商耗时 ≤50ms → `B05-耗时-*`

use super::vea07_caps::StandardBitmap;
use super::veb02_proto::CTRL_HDR_LEN;
use super::veb05_virgl::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// 构造一份 virgl capset 载荷（checks 复用）。
fn capset_payload(gl_major: u32, gl_minor: u32, max_tex: u32, max_samples: u32, fmt_mask: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&CAPSET_VIRGL_ID.to_le_bytes());
    p.extend_from_slice(&1u32.to_le_bytes());
    p.extend_from_slice(&gl_major.to_le_bytes());
    p.extend_from_slice(&gl_minor.to_le_bytes());
    p.extend_from_slice(&max_tex.to_le_bytes());
    p.extend_from_slice(&max_samples.to_le_bytes());
    p.extend_from_slice(&fmt_mask.to_le_bytes());
    p
}

/// 就绪探针（GL 4.5、纹理 16384、采样 8、格式 0..3）。
fn ready_probe(version: u32) -> HostProbe {
    HostProbe {
        device_alive: true,
        capsets: Some(vec![(CAPSET_VIRGL_ID, version, 4096)]),
        payload: Some(capset_payload(4, 5, 16384, 8, 0b1111)),
    }
}

/// VE-F0205 域自检。
pub fn run_veb05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb05");

    // ---- 判据：协商四态正确 ----

    {
        let mut neg = VirglNegotiator::new();
        let n1 = neg.negotiate(&ready_probe(1));
        let n2 = { let mut n = VirglNegotiator::new(); n.negotiate(&ready_probe(2)) };
        let n3 = {
            let mut n = VirglNegotiator::new();
            n.negotiate(&HostProbe {
                device_alive: true,
                capsets: Some(vec![(CAPSET_VENUS_ID, 1, 4096)]),
                payload: None,
            })
        };
        let n4 = {
            let mut n = VirglNegotiator::new();
            n.negotiate(&HostProbe { device_alive: false, capsets: None, payload: None })
        };
        let n5 = {
            let mut n = VirglNegotiator::new();
            n.negotiate(&HostProbe {
                device_alive: true,
                capsets: Some(vec![(CAPSET_VIRGL_ID, 1, 4096)]),
                payload: Some(vec![0u8; 8]), // 短包
            })
        };
        set.add(
            "B05-协商-四态判定",
            n1.state == VirglState::Virgl1
                && n2.state == VirglState::Virgl2
                && n3.state == VirglState::NoVirgl
                && n4.state == VirglState::Unavailable
                && n5.state == VirglState::Unavailable,
            "",
        );
    }
    // 协商失败 ≠ 无 virgl：Unavailable 可重试语义显性（通知文案区分）
    {
        let un = DegradeNotice::for_state(VirglState::Unavailable).unwrap();
        let no = DegradeNotice::for_state(VirglState::NoVirgl).unwrap();
        set.add(
            "B05-协商-失败与缺失语义分立",
            un.next.contains("重试") && !no.next.contains("重试协商成功后"),
            "",
        );
    }
    // 就绪态才建上下文：ctx_id 就绪非零、未就绪为零
    {
        let mut neg = VirglNegotiator::new();
        let ok = neg.negotiate(&ready_probe(1));
        let mut neg2 = VirglNegotiator::new();
        let bad = neg2.negotiate(&HostProbe { device_alive: false, capsets: None, payload: None });
        set.add(
            "B05-协商-就绪才建上下文",
            ok.ctx_id == 1 && bad.ctx_id == 0,
            "",
        );
    }

    // ---- 判据：能力映射与 host 实测一致 ----

    {
        let mut neg = VirglNegotiator::new();
        let n = neg.negotiate(&ready_probe(1));
        let gl45: Vec<&str> = MAPPING_RULES
            .iter()
            .filter(|r| n.bitmap.get(r.key) == Some(true))
            .map(|r| r.key)
            .collect();
        // GL 4.5 → 全部四条映射置位
        let all_on = gl45.len() == MAPPING_RULES.len();
        // GL 2.1 → 只有 raster3d
        let mut neg2 = VirglNegotiator::new();
        let low = neg2.negotiate(&HostProbe {
            device_alive: true,
            capsets: Some(vec![(CAPSET_VIRGL_ID, 1, 4096)]),
            payload: Some(capset_payload(2, 1, 8192, 4, 0b11)),
        });
        let only_raster = low.bitmap.get("raster3d") == Some(true)
            && low.bitmap.get("tessellation") == Some(false)
            && low.bitmap.get("compute") == Some(false)
            && low.bitmap.get("color_mgmt") == Some(false);
        set.add(
            "B05-映射-GL版本逐项一致",
            all_on && only_raster && n.caps.map(|c| c.gl_version()).unwrap_or(0) == 405,
            "",
        );
    }
    // 厂商位隔离纪律（F0007 同源）：映射产物高位恒零
    {
        let caps = parse_host_caps(&capset_payload(4, 5, 16384, 8, 0b1111)).unwrap();
        let b: StandardBitmap = map_to_bitmap(&caps);
        set.add("B05-映射-厂商位隔离", b.vendor_free(), "");
    }

    // ---- 判据：超能力请求创建期拒绝 ----

    {
        let caps = parse_host_caps(&capset_payload(4, 5, 8192, 4, 0b0111)).unwrap();
        let over_tex = Create3dRequest { width: 16384, height: 16, nr_samples: 1, format_bit: 0 }
            .admit(&caps)
            .as_ref()
            .err()
            .map(|e| e.code == "E_EXCEED_MAX_TEX" && e.is_complete())
            .unwrap_or(false);
        let over_samples = Create3dRequest { width: 1024, height: 1024, nr_samples: 16, format_bit: 0 }
            .admit(&caps)
            .as_ref()
            .err()
            .map(|e| e.code == "E_EXCEED_SAMPLES")
            .unwrap_or(false);
        let bad_fmt = Create3dRequest { width: 1024, height: 1024, nr_samples: 1, format_bit: 40 }
            .admit(&caps)
            .as_ref()
            .err()
            .map(|e| e.code == "E_FORMAT_UNSUPPORTED")
            .unwrap_or(false);
        let within = Create3dRequest { width: 8192, height: 8192, nr_samples: 4, format_bit: 2 }
            .admit(&caps)
            .is_ok();
        set.add(
            "B05-拒绝-超能力创建期三路",
            over_tex && over_samples && bad_fmt && within,
            "",
        );
    }
    // 边界值：恰好等于上限放行（边界不许误拒）
    {
        let caps = parse_host_caps(&capset_payload(4, 5, 4096, 8, 1)).unwrap();
        let edge = Create3dRequest { width: 4096, height: 4096, nr_samples: 8, format_bit: 0 }
            .admit(&caps)
            .is_ok();
        set.add("B05-拒绝-边界值放行", edge, "");
    }

    // ---- 判据：降级链联动 ----

    {
        let mut neg = VirglNegotiator::new();
        let n = neg.negotiate(&HostProbe {
            device_alive: true,
            capsets: Some(vec![(CAPSET_VENUS_ID, 1, 4096)]),
            payload: None,
        });
        let notice = n.notice.as_ref().expect("NoVirgl 必有降级通知");
        set.add(
            "B05-降级-无virgl指向软渲F0013",
            notice.target.contains("VE-F0013")
                && notice.is_complete()
                && notice.a11y_text().contains("3D")
                && !n.state.ready(),
            "",
        );
    }
    {
        let mut neg = VirglNegotiator::new();
        let n = neg.negotiate(&ready_probe(1));
        set.add(
            "B05-降级-就绪态无降级通知",
            n.notice.is_none() && n.state.ready(),
            "",
        );
    }

    // ---- 判据：协商耗时 ≤50ms ----

    {
        let mut neg = VirglNegotiator::new();
        let n = neg.negotiate(&ready_probe(2));
        let sum_ok = n.total_us == n.steps.iter().map(|s| s.cost_us).sum::<u64>();
        set.add(
            "B05-耗时-全流程在预算内",
            n.within_budget
                && n.total_us <= NEGOTIATION_BUDGET_US
                && sum_ok
                && n.steps.len() >= 5,
            "",
        );
    }
    {
        let mut neg = VirglNegotiator::new();
        let n = neg.negotiate(&HostProbe { device_alive: false, capsets: None, payload: None });
        set.add(
            "B05-耗时-早退路径也在预算内",
            n.within_budget && n.steps.len() == 1,
            "",
        );
    }

    // ---- 上下文命令编码（wire 对拍）----

    {
        let create = encode_ctx_create(3, "ve-3d-main");
        let destroy = encode_ctx_destroy(3);
        let attach = encode_ctx_resource(CMD_CTX_ATTACH_RESOURCE, 3, 9).unwrap();
        let detach = encode_ctx_resource(CMD_CTX_DETACH_RESOURCE, 3, 9).unwrap();
        let bad = encode_ctx_resource(0x9999, 3, 9);
        let len_ok = create.len() == ctx_wire_len(CMD_CTX_CREATE, 12 + "ve-3d-main".len())
            && destroy.len() == CTRL_HDR_LEN + 8
            && attach.len() == CTRL_HDR_LEN + 12
            && detach.len() == CTRL_HDR_LEN + 12;
        // 头部命令码可回读（小端逐字节）
        let code_ok = u32::from_le_bytes([create[0], create[1], create[2], create[3]])
            == CMD_CTX_CREATE;
        set.add(
            "B05-wire-上下文命令族",
            len_ok && code_ok && bad.is_err(),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut neg = VirglNegotiator::new();
        let n = neg.negotiate(&ready_probe(1));
        let s = negotiation_summary(&n);
        set.add(
            "B05-读屏-协商摘要可播",
            s.contains("virgl") && s.contains("上下文") && s.contains("耗时"),
            "",
        );
    }
    {
        let run = || {
            let mut neg = VirglNegotiator::new();
            let n = neg.negotiate(&ready_probe(2));
            (n.state, n.total_us, n.ctx_id, n.bitmap.0, negotiation_summary(&n))
        };
        let a = run();
        let b = run();
        set.add("B05-确定-同探针同结果", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veb05_checks_all_green() {
        let set = run_veb05_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-B05 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 四态判定的直接单测。
    #[test]
    fn four_state_matrix() {
        let mut neg = VirglNegotiator::new();
        assert_eq!(neg.negotiate(&ready_probe(1)).state, VirglState::Virgl1);
        let mut neg = VirglNegotiator::new();
        assert_eq!(neg.negotiate(&ready_probe(2)).state, VirglState::Virgl2);
        let mut neg = VirglNegotiator::new();
        let dead = HostProbe { device_alive: false, capsets: None, payload: None };
        assert_eq!(neg.negotiate(&dead).state, VirglState::Unavailable);
    }

    /// 超能力请求必须在创建期拒绝，不许拖到运行期。
    #[test]
    fn over_capability_rejected_at_creation() {
        let caps = parse_host_caps(&capset_payload(4, 5, 8192, 4, 0b111)).unwrap();
        assert!(Create3dRequest { width: 8193, height: 8, nr_samples: 1, format_bit: 0 }
            .admit(&caps)
            .is_err());
        assert!(Create3dRequest { width: 100, height: 100, nr_samples: 5, format_bit: 0 }
            .admit(&caps)
            .is_err());
        assert!(Create3dRequest { width: 100, height: 100, nr_samples: 1, format_bit: 50 }
            .admit(&caps)
            .is_err());
    }
}
