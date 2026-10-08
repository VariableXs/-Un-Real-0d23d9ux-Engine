//! VE-F0203 · 域自检（判据逐条对应，见 `veb03_resource.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 2D/3D 创建路径全测 → `B03-创建-*`
//! - backing 部分重挂正确 → `B03-backing-*`
//! - 两步销毁封装 → `B03-销毁-*`
//! - 导出句柄可被导入 → `B03-导出-*`
//! - 泄漏压测归零 → `B03-泄漏-*`

use super::veb01_device::DisplayCfg;
use super::veb02_proto::{CtrlCommand, MemEntry, ReferenceDevice, RespErr, CtrlResponse};
use super::veb03_resource::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

fn mem(addr: u64, len: u32) -> MemEntry {
    MemEntry { addr, length: len }
}

/// VE-F0203 域自检。
pub fn run_veb03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb03");

    // ---- 判据：2D/3D 创建路径全测 ----

    // 2D：合法创建 / 重复 id / 零尺寸 / 未登记格式 全覆盖
    {
        let mut t = ResourceTable::new();
        let ok = t
            .create_2d(&Create2d {
                id: 1,
                format: FMT_B8G8R8A8_UNORM,
                width: 3840,
                height: 2160,
            })
            .is_ok();
        let dup = t
            .create_2d(&Create2d {
                id: 1,
                format: 1,
                width: 10,
                height: 10,
            })
            .as_ref()
            .err()
            .map(|e| e.code == "E_DUP_ID")
            .unwrap_or(false);
        let zero = t
            .create_2d(&Create2d {
                id: 2,
                format: 1,
                width: 0,
                height: 10,
            })
            .as_ref()
            .err()
            .map(|e| e.code == "E_INVALID_DIMS")
            .unwrap_or(false);
        let fmt = t
            .create_2d(&Create2d {
                id: 3,
                format: 0xFFFF,
                width: 10,
                height: 10,
            })
            .as_ref()
            .err()
            .map(|e| e.code == "E_FORMAT_UNKNOWN")
            .unwrap_or(false);
        set.add("B03-创建-2D全路径", ok && dup && zero && fmt, "");
    }
    // 3D 参数路径：全字段入账 / depth 0 拒绝
    {
        let mut t = ResourceTable::new();
        let ok = t
            .create_3d_params(&Create3dParams {
                id: 5,
                target: 2,
                format: FMT_B8G8R8A8_UNORM,
                width: 1024,
                height: 1024,
                depth: 4,
                array_size: 2,
                last_level: 10,
                nr_samples: 4,
                flags: [1, 2, 3],
            })
            .is_ok();
        let full = t.find(5).map(|r| {
            r.is_3d && r.depth == 4 && r.array_size == 2 && r.last_level == 10
                && r.nr_samples == 4 && r.flags == [1, 2, 3]
        });
        let zero_depth = t
            .create_3d_params(&Create3dParams {
                id: 6,
                target: 2,
                format: 1,
                width: 8,
                height: 8,
                depth: 0,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: [0; 3],
            })
            .as_ref()
            .err()
            .map(|e| e.code == "E_INVALID_DIMS")
            .unwrap_or(false);
        set.add(
            "B03-创建-3D参数路径全字段",
            ok && full == Some(true) && zero_depth,
            "",
        );
    }
    // blob 路径显性指界 VE-F0209（不装实现、不静默）
    {
        let t = ResourceTable::new();
        let e = t.create_3d_blob(7);
        set.add(
            "B03-创建-blob路径显性指界",
            e.code == "E_BLOB_OUT_OF_SCOPE"
                && e.is_complete()
                && e.next.contains("VE-F0209"),
            "",
        );
    }

    // ---- 判据：backing 部分重挂正确 ----

    // 页对齐纪律：非对齐地址/长度拒绝
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let bad_addr = t
            .attach_backing(1, &[mem(0x1001, PAGE_SIZE)])
            .as_ref()
            .err()
            .map(|e| e.code == "E_PAGE_UNALIGNED")
            .unwrap_or(false);
        let bad_len = t
            .attach_backing(1, &[mem(0x1000, PAGE_SIZE + 512)])
            .as_ref()
            .err()
            .map(|e| e.code == "E_PAGE_UNALIGNED")
            .unwrap_or(false);
        set.add("B03-backing-页对齐纪律", bad_addr && bad_len, "");
    }
    // 挂接 + 页数记账 + 重复挂接拒绝
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let pages = t
            .attach_backing(1, &[mem(0x1000, PAGE_SIZE), mem(0x2000, PAGE_SIZE * 2)])
            .unwrap_or(0);
        let dup = t
            .attach_backing(1, &[mem(0x1000, PAGE_SIZE)])
            .as_ref()
            .err()
            .map(|e| e.code == "E_BACKING_DUP")
            .unwrap_or(false);
        set.add(
            "B03-backing-挂接页数记账",
            pages == 3 && t.total_backing_pages() == 3 && dup,
            "",
        );
    }
    // 部分重挂：只换目标条目，其余原样，审计留痕
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let _ = t.attach_backing(
            1,
            &[mem(0x1000, PAGE_SIZE), mem(0x2000, PAGE_SIZE), mem(0x3000, PAGE_SIZE)],
        );
        let ok = t.reattach_backing(1, 1, &mem(0x9000, PAGE_SIZE)).is_ok();
        let r = t.find(1).unwrap();
        let list = r.backing.as_ref().unwrap();
        let only_target_changed = list[0].addr == 0x1000
            && list[1].addr == 0x9000
            && list[2].addr == 0x3000;
        let audited = r.reattach_count == 1 && r.audit == vec![(1, 1)];
        let out_of_range = t
            .reattach_backing(1, 5, &mem(0xA000, PAGE_SIZE))
            .as_ref()
            .err()
            .map(|e| e.code == "E_REATTACH_RANGE")
            .unwrap_or(false);
        set.add(
            "B03-backing-部分重挂正确",
            ok && only_target_changed && audited && out_of_range,
            "",
        );
    }

    // ---- 判据：两步销毁封装 ----

    // destroy 固定 DETACH→UNREF 顺序
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let _ = t.attach_backing(1, &[mem(0x1000, PAGE_SIZE)]);
        let seq = t.destroy(1).unwrap();
        set.add(
            "B03-销毁-两步封装顺序",
            matches!(seq[0], CtrlCommand::ResourceDetachBacking { resource_id: 1 })
                && matches!(seq[1], CtrlCommand::ResourceUnref { resource_id: 1 })
                && t.is_empty(),
            "",
        );
    }
    // 顺序错误显性拒绝：带 backing 裸 UNREF 被拦并引导 destroy()
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let _ = t.attach_backing(1, &[mem(0x1000, PAGE_SIZE)]);
        let e = t.unref(1);
        set.add(
            "B03-销毁-顺序错误显性拒绝",
            e.as_ref().err().map(|e| {
                e.code == "E_DESTROY_ORDER" && e.next.contains("destroy")
                    && e.is_complete()
            }).unwrap_or(false)
                && t.find(1).is_some()
                && t.refused_unref_with_backing == 1,
            "",
        );
    }
    // 无 backing 资源可裸 UNREF（合法路径不挡）
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 2, format: 1, width: 64, height: 64 });
        set.add(
            "B03-销毁-无backing可裸unref",
            t.unref(2).is_ok() && t.is_empty() && t.destroyed_total == 1,
            "",
        );
    }

    // ---- 判据：导出句柄可被导入 ----

    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d {
            id: 9,
            format: FMT_B8G8R8A8_UNORM,
            width: 3840,
            height: 2160,
        });
        let mut reg = ExportRegistry::new();
        let h = reg.export(&t, 9).unwrap();
        // stride 页对齐：3840*4=15360 → 4 页 = 16384
        let stride_ok = h.stride == 16384 && h.bpp == 4;
        let imported = reg.import(&h, 1).is_ok() && reg.active_imports() == 1;
        set.add("B03-导出-句柄可导入stride页对齐", stride_ok && imported && h.is_intact(), "");
    }
    // 句柄防篡改
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let mut reg = ExportRegistry::new();
        let mut h = reg.export(&t, 1).unwrap();
        h.width += 1; // 篡改字段
        set.add(
            "B03-导出-防篡改",
            !h.is_intact()
                && reg.import(&h, 1).as_ref().err().map(|e| e.code == "E_HANDLE_TAMPERED").unwrap_or(false),
            "",
        );
    }
    // 销毁即吊销：吊销后导入必拒（悬垂防护）
    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let mut reg = ExportRegistry::new();
        let h = reg.export(&t, 1).unwrap();
        let _ = reg.import(&h, 1);
        let _ = t.destroy(1);
        reg.revoke(h.nonce);
        set.add(
            "B03-导出-销毁即吊销",
            reg.import(&h, 2).as_ref().err().map(|e| e.code == "E_HANDLE_REVOKED").unwrap_or(false),
            "",
        );
    }
    // 未知资源导出拒绝
    {
        let t = ResourceTable::new();
        let mut reg = ExportRegistry::new();
        set.add(
            "B03-导出-未知资源拒绝",
            reg.export(&t, 99).as_ref().err().map(|e| e.code == "E_NO_RESOURCE").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：泄漏压测归零 ----

    {
        let rep = leak_stress(200);
        set.add(
            "B03-泄漏-压测归零",
            rep.zero_leak() && rep.rounds == 200 && rep.revoked == 200,
            "",
        );
    }
    // 混合 2D/3D 压测同样归零
    {
        let mut t = ResourceTable::new();
        let mut reg = ExportRegistry::new();
        for i in 0..100u32 {
            let id = i + 1;
            if i % 2 == 0 {
                let _ = t.create_2d(&Create2d { id, format: 1, width: 32, height: 32 });
            } else {
                let _ = t.create_3d_params(&Create3dParams {
                    id,
                    target: 2,
                    format: 1,
                    width: 32,
                    height: 32,
                    depth: 1,
                    array_size: 1,
                    last_level: 0,
                    nr_samples: 0,
                    flags: [0; 3],
                });
            }
            let _ = t.attach_backing(id, &[mem(0x1000, PAGE_SIZE)]);
            if let Ok(h) = reg.export(&t, id) {
                let _ = reg.import(&h, i);
                reg.revoke(h.nonce);
            }
            let _ = t.destroy(id);
        }
        set.add(
            "B03-泄漏-混合2D3D归零",
            t.is_empty() && t.total_backing_pages() == 0 && t.created_total == 100 && t.destroyed_total == 100,
            "",
        );
    }

    // ---- 设备侧对拍（与 F0202 参考设备联动） ----

    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: FMT_B8G8R8A8_UNORM, width: 64, height: 64 });
        let mut reg = ExportRegistry::new();
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let replay_ok = replay_against_device(&mut t, &mut reg, &mut dev, 1).is_ok();
        // 经设备：创建 → backing → 挂 scanout → 两步销毁，设备账面同步归零
        let c1 = dev.handle(&CtrlCommand::ResourceCreate2d {
            resource_id: 2,
            format: FMT_B8G8R8A8_UNORM,
            width: 32,
            height: 32,
        });
        let _ = dev.handle(&CtrlCommand::ResourceAttachBacking {
            resource_id: 2,
            entries: vec![mem(0x1000, PAGE_SIZE)],
        });
        // 驱动侧没有 2 号资源——销毁须拒绝
        let destroy_refused = t.destroy(2).is_err();
        let _ = dev.handle(&CtrlCommand::ResourceDetachBacking { resource_id: 2 });
        let un = dev.handle(&CtrlCommand::ResourceUnref { resource_id: 2 });
        // 清理 replay 灌入的资源 1（设备侧无 backing，可直接 UNREF）
        let un1 = dev.handle(&CtrlCommand::ResourceUnref { resource_id: 1 });
        set.add(
            "B03-联动-设备侧对拍全通",
            replay_ok
                && c1 == CtrlResponse::OkNoData
                && destroy_refused
                && un == CtrlResponse::OkNoData
                && un1 == CtrlResponse::OkNoData
                && dev.resource_count() == 0,
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let _ = t.attach_backing(1, &[mem(0x1000, PAGE_SIZE)]);
        let _ = t.unref(1);
        let s = t.a11y_summary();
        set.add(
            "B03-读屏-账面可播",
            s.contains("资源账面") && s.contains("backing") && s.contains("拒绝裸销毁"),
            "",
        );
    }
    {
        let mut a = ResourceTable::new();
        let mut b = ResourceTable::new();
        for t in [&mut a, &mut b] {
            let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
            let _ = t.attach_backing(1, &[mem(0x1000, PAGE_SIZE)]);
            let _ = t.reattach_backing(1, 0, &mem(0x9000, PAGE_SIZE));
        }
        set.add(
            "B03-确定-同操作同账面",
            a.a11y_summary() == b.a11y_summary()
                && a.find(1).map(|r| r.audit.clone()) == b.find(1).map(|r| r.audit.clone()),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veb03_checks_all_green() {
        let set = run_veb03_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-B03 域自检红项：{}/{} 绿", passed, passed + failed);
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

    /// 两步销毁的命令序列灌进参考设备必须全通（设备账面同步归零）。
    #[test]
    fn destroy_sequence_replays_clean_on_device() {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: FMT_B8G8R8A8_UNORM, width: 64, height: 64 });
        let _ = t.attach_backing(1, &[mem(0x1000, PAGE_SIZE)]);
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        assert!(
            dev.handle(&CtrlCommand::ResourceCreate2d {
                resource_id: 1,
                format: FMT_B8G8R8A8_UNORM,
                width: 64,
                height: 64,
            }) == CtrlResponse::OkNoData
        );
        assert!(dev.handle(&CtrlCommand::ResourceAttachBacking {
            resource_id: 1,
            entries: vec![mem(0x1000, PAGE_SIZE)],
        }) == CtrlResponse::OkNoData);
        let seq = t.destroy(1).unwrap();
        for c in seq.iter() {
            assert_eq!(dev.handle(c), CtrlResponse::OkNoData, "序列步 {:?}", c);
        }
        assert_eq!(dev.resource_count(), 0, "设备侧账面必须同步归零");
    }

    /// 泄漏压测：创建数 == 销毁数，账面归零。
    #[test]
    fn leak_stress_zeroes_ledger() {
        let rep = leak_stress(500);
        assert!(rep.zero_leak(), "压测报告 {:?}", rep);
    }

    /// 句柄是能力凭证：篡改与吊销双闸都在。
    #[test]
    fn handle_is_capability() {
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 1, format: 1, width: 64, height: 64 });
        let mut reg = ExportRegistry::new();
        let mut h = reg.export(&t, 1).unwrap();
        h.size_bytes += 1;
        assert!(!h.is_intact());
        assert!(reg.import(&h, 1).is_err());
    }
}
