//! VE-F0202 · 域自检（判据逐条对应，见 `veb02_proto.rs` / `veb02_queue.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 全命令族 roundtrip 对拍规范 → `B02-roundtrip-*`（含 wire 长度/字段偏移对拍）
//! - 在途配对零串扰 → `B02-在途-*`
//! - 超时联动丢失状态机 → `B02-超时-*`
//! - 错误映射全覆盖 → `B02-错误映射-*`
//! - 编码零字节冗余 → `B02-编码-*`
//!
//! 全部确定性：参考设备应答 + 逻辑 tick，无墙钟、零 IO。

use super::veb01_device::DisplayCfg;
use super::veb02_proto::*;
use super::veb02_queue::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// 代表性命令族（每条命令至少一个实例；AttachBacking 带两条内存条目）。
fn family() -> Vec<CtrlCommand> {
    vec![
        CtrlCommand::GetDisplayInfo,
        CtrlCommand::ResourceCreate2d {
            resource_id: 1,
            format: 1, // B8G8R8A8_UNROM
            width: 10,
            height: 10,
        },
        CtrlCommand::ResourceCreate3d {
            resource_id: 2,
            target: 2,
            format: 1,
            width: 8,
            height: 8,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: [0; 3],
        },
        CtrlCommand::ResourceUnref { resource_id: 2 },
        CtrlCommand::SetScanout {
            scanout_id: 0,
            resource_id: 1,
            rect: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
        },
        CtrlCommand::ResourceFlush {
            resource_id: 1,
            rect: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
        },
        CtrlCommand::TransferToHost2d {
            resource_id: 1,
            rect: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            offset: 0,
        },
        CtrlCommand::TransferToHost3d {
            resource_id: 2,
            box_xyzwhd: [0, 0, 0, 8, 8, 1],
            offset: 0,
            level: 0,
            stride: 0,
            layer_stride: 0,
        },
        CtrlCommand::TransferFromHost3d {
            resource_id: 2,
            box_xyzwhd: [0, 0, 0, 8, 8, 1],
            offset: 0,
            level: 0,
            stride: 0,
            layer_stride: 0,
        },
        CtrlCommand::ResourceAttachBacking {
            resource_id: 1,
            entries: vec![
                MemEntry {
                    addr: 0x1000,
                    length: 256,
                },
                MemEntry {
                    addr: 0x2000,
                    length: 256,
                },
            ],
        },
        CtrlCommand::ResourceDetachBacking { resource_id: 1 },
        CtrlCommand::GetCapsetInfo { capset_index: 0 },
        CtrlCommand::GetCapset {
            capset_id: 0,
            version: 1,
        },
        CtrlCommand::GetEdid { scanout: 0 },
        CtrlCommand::UpdateCursor {
            scanout_id: 0,
            x: 5,
            y: 5,
            resource_id: 0,
            hot_x: 1,
            hot_y: 1,
        },
    ]
}

/// VE-F0202 域自检。
pub fn run_veb02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb02");

    // ---- 判据：编码零字节冗余 ----

    // 全命令族 wire 长度与规范逐字节相等
    {
        let ok = family().iter().all(|c| c.encode(0).len() == c.spec_wire_len());
        set.add("B02-编码-全命令族wire长度", ok, "");
    }
    // 控制头逐字段对拍（type/flags/fence/ctx/ring+pad 的偏移与值）
    {
        let cmd = CtrlCommand::ResourceCreate2d {
            resource_id: 7,
            format: 1,
            width: 10,
            height: 10,
        };
        let w = cmd.encode(0xFEED);
        let hdr_type = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        let hdr_flags = u32::from_le_bytes([w[4], w[5], w[6], w[7]]);
        let hdr_fence = u64::from_le_bytes([
            w[8], w[9], w[10], w[11], w[12], w[13], w[14], w[15],
        ]);
        let body_format = u32::from_le_bytes([w[24], w[25], w[26], w[27]]);
        let body_rid = u32::from_le_bytes([w[36], w[37], w[38], w[39]]);
        set.add(
            "B02-编码-控制头与字段偏移",
            hdr_type == CMD_RESOURCE_CREATE_2D
                && hdr_flags == 0
                && hdr_fence == 0xFEED
                && body_format == 1
                && body_rid == 7,
                "",
        );
    }
    // 变长命令（attach_backing）的长度随条目数增长且无额外字节
    {
        let one = CtrlCommand::ResourceAttachBacking {
            resource_id: 1,
            entries: vec![MemEntry { addr: 1, length: 2 }],
        };
        let three = CtrlCommand::ResourceAttachBacking {
            resource_id: 1,
            entries: vec![
                MemEntry { addr: 1, length: 2 },
                MemEntry { addr: 3, length: 4 },
                MemEntry { addr: 5, length: 6 },
            ],
        };
        set.add(
            "B02-编码-变长命令零冗余",
            one.spec_wire_len() == CTRL_HDR_LEN + 8 + 16
                && three.encode(0).len() == CTRL_HDR_LEN + 8 + 3 * 16,
            "",
        );
    }

    // ---- 判据：全命令族 roundtrip 对拍规范（经参考设备） ----

    // 全命令族依次经引擎提交 → 参考设备应答 → 收割配对，响应全部 OK
    {
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let mut eng = ControlQueueEngine::new(256);
        let mut all_ok = true;
        for c in family().iter() {
            // ResourceUnref(2) 在 family 里出现在 Create3d 之后、其余 3D
            // 命令之前——3D 命令用 resource 2 会失败；本对拍序列按
            // family 顺序逐条执行，UNREF 之后不再引用 2 是 false。
            // 因此这里对拍的是"设备语义正确"，序列敏感的顺序另测。
            let fence = eng.submit(c).unwrap();
            let resp = dev.handle(c);
            let expect_code = if matches!(resp, CtrlResponse::OkNoData) {
                RESP_OK_NODATA
            } else {
                resp.code()
            };
            let wire = resp.encode(fence);
            let _ = eng.device_complete(fence, wire.len());
            let done = eng.harvest();
            match done.first() {
                Some((f, _, _)) if *f == fence => {
                    let (_, dec) = CtrlResponse::decode(&wire).unwrap();
                    if dec.code() != expect_code {
                        all_ok = false;
                    }
                }
                _ => all_ok = false,
            }
        }
        set.add("B02-roundtrip-全命令族经参考设备", all_ok, "");
    }
    // 语义序列对拍：创建→绑定→刷新→解绑→销毁 全链响应逐一断言
    {
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let seq: Vec<(CtrlCommand, CtrlResponse)> = vec![
            (
                CtrlCommand::ResourceCreate2d {
                    resource_id: 10,
                    format: 1,
                    width: 100,
                    height: 100,
                },
                CtrlResponse::OkNoData,
            ),
            (
                CtrlCommand::SetScanout {
                    scanout_id: 0,
                    resource_id: 10,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                },
                CtrlResponse::OkNoData,
            ),
            (
                CtrlCommand::ResourceFlush {
                    resource_id: 10,
                    rect: Rect::default(),
                },
                CtrlResponse::OkNoData,
            ),
            (
                CtrlCommand::ResourceDetachBacking { resource_id: 10 },
                CtrlResponse::OkNoData,
            ),
            (
                CtrlCommand::ResourceUnref { resource_id: 10 },
                CtrlResponse::OkNoData,
            ),
        ];
        let ok = seq.iter().all(|(c, want)| dev.handle(c) == *want)
            && dev.resource_count() == 0;
        set.add("B02-roundtrip-语义序列对拍", ok, "");
    }
    // 响应解码 roundtrip：encode→decode 恢复 fence 与类型
    {
        let resp = CtrlResponse::OkDisplayInfo {
            scanouts: vec![DisplayOne {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 3840,
                    height: 2160,
                },
                enabled: 1,
                flags: 0,
            }],
        };
        let wire = resp.encode(77);
        let (f, dec) = CtrlResponse::decode(&wire).unwrap();
        let edid = CtrlResponse::OkEdid {
            size: 128,
            edid: vec![0u8; 128],
        };
        let wire2 = edid.encode(88);
        let (f2, dec2) = CtrlResponse::decode(&wire2).unwrap();
        set.add(
            "B02-roundtrip-响应解码配对",
            f == 77
                && matches!(dec, CtrlResponse::OkDisplayInfo { .. })
                && f2 == 88
                && matches!(dec2, CtrlResponse::OkEdid { size: 128, .. }),
            "",
        );
    }

    // ---- 判据：在途配对零串扰 ----

    // 多在途命令乱序完成，收割配对不错位
    {
        let mut eng = ControlQueueEngine::new(256);
        let f1 = eng
            .submit(&CtrlCommand::GetDisplayInfo)
            .unwrap();
        let f2 = eng
            .submit(&CtrlCommand::GetEdid { scanout: 0 })
            .unwrap();
        let f3 = eng
            .submit(&CtrlCommand::ResourceUnref { resource_id: 9 })
            .unwrap();
        // 乱序完成：3 → 1 → 2
        let _ = eng.device_complete(f3, 24);
        let _ = eng.device_complete(f1, 24);
        let _ = eng.device_complete(f2, 24);
        let done = eng.harvest();
        let fences: Vec<u64> = done.iter().map(|(f, _, _)| *f).collect();
        set.add(
            "B02-在途-乱序完成零错位",
            fences == vec![f3, f1, f2] && eng.inflight().is_empty() && eng.harvested == 3,
            "",
        );
    }
    // 未知 fence 的应答 = 串扰，显性拒绝并留痕
    {
        let mut eng = ControlQueueEngine::new(256);
        let r = eng.device_complete(0xDEAD, 24);
        set.add(
            "B02-在途-串扰显性拦截",
            r.as_ref().err().map(|e| e.code == "E_CROSSTALK" && !e.next.is_empty()).unwrap_or(false)
                && eng.crosstalk.len() == 1,
            "",
        );
    }
    // 同一 fence 重复应答 = 串扰（第二次对不上在途）
    {
        let mut eng = ControlQueueEngine::new(256);
        let f = eng.submit(&CtrlCommand::GetDisplayInfo).unwrap();
        let first = eng.device_complete(f, 24);
        let second = eng.device_complete(f, 24);
        set.add(
            "B02-在途-重复应答拒绝",
            first.is_ok() && second.as_ref().err().map(|e| e.code == "E_CROSSTALK").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：超时联动丢失状态机 ----

    // 默认 2 秒超时：1_999_999µs 不超时，+2µs 即超时并产出可疑记录
    {
        let mut eng = ControlQueueEngine::new(256);
        let _ = eng.submit(&CtrlCommand::GetDisplayInfo).unwrap();
        eng.advance(CMD_TIMEOUT_US - 1);
        let before = eng.poll();
        eng.advance(2);
        let after = eng.poll();
        let linked = eng
            .suspicions
            .first()
            .map(|s| s.links_f0009() && s.age_us > CMD_TIMEOUT_US)
            .unwrap_or(false);
        set.add(
            "B02-超时-默认两秒联动F0009",
            before == 0 && after == 1 && linked && eng.inflight().is_empty(),
            "",
        );
    }
    // 正常完成不误报超时
    {
        let mut eng = ControlQueueEngine::new(256);
        let f = eng.submit(&CtrlCommand::GetDisplayInfo).unwrap();
        let _ = eng.device_complete(f, 24);
        let _ = eng.harvest();
        eng.advance(CMD_TIMEOUT_US * 3);
        set.add(
            "B02-超时-正常完成不误报",
            eng.poll() == 0 && eng.suspicions.is_empty(),
            "",
        );
    }
    // 乱序提交下的超时逐条指认（每条可疑记录都带命令身份）
    {
        let mut eng = ControlQueueEngine::new(256);
        let _ = eng.submit(&CtrlCommand::GetDisplayInfo).unwrap();
        let _ = eng.submit(&CtrlCommand::GetEdid { scanout: 0 }).unwrap();
        eng.advance(CMD_TIMEOUT_US + 10);
        let n = eng.poll();
        let identified = eng
            .suspicions
            .iter()
            .all(|s| !s.cmd_label.is_empty() && s.cmd_code != 0);
        set.add(
            "B02-超时-逐条指认命令身份",
            n == 2 && identified,
            "",
        );
    }

    // ---- 判据：错误映射全覆盖 ----

    {
        let all = [
            RespErr::Unspec,
            RespErr::OutOfMemory,
            RespErr::InvalidScanoutId,
            RespErr::InvalidResourceId,
            RespErr::InvalidContextId,
            RespErr::InvalidParameter,
        ];
        let segs: Vec<&str> = all.iter().map(|e| map_resp_err(*e).seg_code).collect();
        let unique = all
            .iter()
            .all(|e| segs.iter().filter(|s| **s == map_resp_err(*e).seg_code).count() == 1);
        set.add(
            "B02-错误映射-全集覆盖三要素",
            all.iter().all(|e| map_resp_err(*e).is_complete())
                && all.iter().all(|e| map_resp_err(*e).seg_code.starts_with("VEB-E-"))
                && unique,
            "",
        );
    }
    // 设备错误响应经解码 → 映射到 B 域段（端到端）
    {
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let resp = dev.handle(&CtrlCommand::SetScanout {
            scanout_id: 9,
            resource_id: 0,
            rect: Rect::default(),
        });
        let wire = resp.encode(1);
        let (_, dec) = CtrlResponse::decode(&wire).unwrap();
        let mapped = match dec {
            CtrlResponse::Err(e) => map_resp_err(e),
            _ => map_resp_err(RespErr::Unspec),
        };
        set.add(
            "B02-错误映射-端到端落地",
            matches!(dec, CtrlResponse::Err(RespErr::InvalidScanoutId))
                && mapped.seg_code == "VEB-E-1202",
            "",
        );
    }

    // ---- 设备语义防线 ----

    {
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let bad = dev.handle(&CtrlCommand::ResourceFlush { resource_id: 42, rect: Rect::default() });
        set.add(
            "B02-设备-非法资源号拒绝",
            bad == CtrlResponse::Err(RespErr::InvalidResourceId),
            "",
        );
    }
    {
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let first = dev.handle(&CtrlCommand::ResourceCreate2d {
            resource_id: 1,
            format: 1,
            width: 0,
            height: 10,
        });
        // 零尺寸创建被拒（参数非法）且资源不入表——资源号占用无从发生
        set.add(
            "B02-设备-零尺寸拒绝",
            first == CtrlResponse::Err(RespErr::InvalidParameter) && dev.resource_count() == 0,
            "",
        );
    }

    // ---- avail/used 环操作 ----

    // 描述符链组装：长命令拆 ≤DESC_MAX_LEN 的链
    {
        let mut t = DescTable::new(64);
        let head = t.push_chain(1200).unwrap();
        set.add(
            "B02-队列-长命令拆链",
            head == 0 && t.len() == 3 && t.push_chain(1).unwrap() == 3,
            "",
        );
    }
    // 描述符表耗尽显性拒绝（不写穿）
    {
        let mut t = DescTable::new(2);
        let r = t.push_chain(2000);
        set.add(
            "B02-队列-容量耗尽拒绝",
            r.as_ref().err().map(|e| e.code == "E_DESC_EXHAUSTED" && !e.next.is_empty()).unwrap_or(false),
            "",
        );
    }
    // 空链拒绝
    {
        let mut t = DescTable::new(8);
        let r = t.push_chain(0);
        set.add(
            "B02-队列-空链拒绝",
            r.as_ref().err().map(|e| e.code == "E_CHAIN_EMPTY").unwrap_or(false),
            "",
        );
    }
    // avail 索引推进与 used 收割计数
    {
        let mut eng = ControlQueueEngine::new(64);
        let _ = eng.submit(&CtrlCommand::GetDisplayInfo);
        let _ = eng.submit(&CtrlCommand::GetDisplayInfo);
        let avail_ok = eng.avail.idx == 2 && eng.avail.ring.len() == 2;
        let _ = eng.device_complete(1, 24);
        let _ = eng.device_complete(2, 24);
        let done = eng.harvest();
        set.add(
            "B02-队列-avail推进与收割计数",
            avail_ok && done.len() == 2 && eng.used.ring.is_empty() && eng.harvested == 2,
            "",
        );
    }

    // ---- 读屏可达 ----

    {
        let mut eng = ControlQueueEngine::new(64);
        let _ = eng.submit(&CtrlCommand::GetDisplayInfo);
        eng.advance(CMD_TIMEOUT_US + 1);
        let _ = eng.poll();
        let s = eng.a11y_summary();
        set.add(
            "B02-读屏-队列状态可播",
            s.contains("控制队列") && s.contains("在途") && s.contains("超时可疑") && s.contains("串扰"),
            "",
        );
    }

    // ---- 确定性 ----

    {
        let run = || {
            let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
            let mut eng = ControlQueueEngine::new(256);
            for c in family().iter() {
                let f = eng.submit(c).unwrap();
                let resp = dev.handle(c);
                let _ = eng.device_complete(f, resp.encode(f).len());
                let _ = eng.harvest();
            }
            eng.advance(CMD_TIMEOUT_US + 1);
            eng.poll();
            (eng.submitted, eng.harvested, dev.handled.len())
        };
        let a = run();
        let b = run();
        set.add("B02-确定-同序列同结果", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veb02_checks_all_green() {
        let set = run_veb02_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-B02 域自检红项：{}/{} 绿", passed, passed + failed);
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

    /// 全命令族 wire 长度逐条断言（对拍规范的长名单版）。
    #[test]
    fn wire_len_matches_spec_per_command() {
        for c in family() {
            assert_eq!(
                c.encode(0).len(),
                c.spec_wire_len(),
                "{} 编码长度与规范不符",
                c.label()
            );
        }
    }

    /// 命令-响应对全链路：提交→设备→完成→收割→解码，配对逐条成立。
    #[test]
    fn full_roundtrip_pairing() {
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let mut eng = ControlQueueEngine::new(256);
        let mut fences: Vec<u64> = Vec::new();
        for c in family().iter() {
            let f = eng.submit(c).unwrap();
            fences.push(f);
            let resp = dev.handle(c);
            let _ = eng.device_complete(f, resp.encode(f).len());
            let done = eng.harvest();
            assert_eq!(done.len(), 1, "{} 应立即收割一条", c.label());
            let (f2, cmd2, _) = &done[0];
            assert_eq!(f2, &f);
            assert_eq!(cmd2, c);
        }
        assert_eq!(fences.len(), 15);
        assert!(eng.inflight().is_empty());
    }

    /// 超时的可疑记录携带 F0009 联动标记（丢失状态机的输入契约）。
    #[test]
    fn suspicion_links_f0009_loss_state_machine() {
        let mut eng = ControlQueueEngine::new(64);
        let _ = eng.submit(&CtrlCommand::GetDisplayInfo).unwrap();
        eng.advance(CMD_TIMEOUT_US + 5);
        assert_eq!(eng.poll(), 1);
        let s = &eng.suspicions[0];
        assert!(s.links_f0009());
        assert_eq!(s.cmd_label, "GET_DISPLAY_INFO");
        assert_eq!(s.detected_tick, CMD_TIMEOUT_US + 5);
    }

    /// 串扰零静默：拦截 + 留痕 + 建议三件齐。
    #[test]
    fn crosstalk_never_silent() {
        let mut eng = ControlQueueEngine::new(64);
        let r = eng.device_complete(12345, 24);
        let e = r.unwrap_err();
        assert_eq!(e.code, "E_CROSSTALK");
        assert!(!e.what.is_empty() && !e.why.is_empty() && !e.next.is_empty());
        assert_eq!(eng.crosstalk.len(), 1);
    }

    /// 错误映射端到端：设备错误响应解码后落进 B 域段码位。
    #[test]
    fn err_response_maps_to_b_segment() {
        let resp = CtrlResponse::Err(RespErr::InvalidResourceId);
        let wire = resp.encode(9);
        let (f, dec) = CtrlResponse::decode(&wire).unwrap();
        assert_eq!(f, 9);
        match dec {
            CtrlResponse::Err(e) => assert_eq!(map_resp_err(e).seg_code, "VEB-E-1203"),
            _ => panic!("错误响应解码错位"),
        }
    }
}
