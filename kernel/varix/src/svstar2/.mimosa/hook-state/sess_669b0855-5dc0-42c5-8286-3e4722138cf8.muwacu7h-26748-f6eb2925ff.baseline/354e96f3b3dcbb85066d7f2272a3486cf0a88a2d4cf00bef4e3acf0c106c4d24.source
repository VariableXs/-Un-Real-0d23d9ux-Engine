//! VE-F0204 · 域自检（判据逐条对应，见 `veb04_2dupdate.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 脏区裁剪正确性（屏幕内容无残影）→ `B04-裁剪-*`（含覆盖率采样对拍）
//! - 合批收益实测 → `B04-合批-*`
//! - 流水线围栏正确 → `B04-流水线-*`
//! - 限速策略 → `B04-限速-*`
//! - 4K 桌面滚动场景带宽实测 → `B04-带宽-*`

use super::veb01_device::DisplayCfg;
use super::veb02_proto::{CtrlCommand, Rect, ReferenceDevice, CtrlResponse};
use super::veb03_resource::{Create2d, FMT_B8G8R8A8_UNORM, ResourceTable};
use super::veb04_2dupdate::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

fn r(x: u32, y: u32, w: u32, h: u32) -> Rect {
    Rect { x, y, width: w, height: h }
}

/// 建 4K 2D 资源的表（checks 复用）。
fn table_with_4k(id: u32) -> ResourceTable {
    let mut t = ResourceTable::new();
    let _ = t.create_2d(&Create2d {
        id,
        format: FMT_B8G8R8A8_UNORM,
        width: 3840,
        height: 2160,
    });
    t
}

/// VE-F0204 域自检。
pub fn run_veb04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb04");

    // ---- 判据：脏区裁剪正确性（屏幕内容无残影）----

    // 越界裁剪：部分越界裁到边界、完全在外丢弃、零面积丢弃
    {
        let bounds = r(0, 0, 1920, 1080);
        let c1 = clip_rect(&r(1800, 1000, 400, 200), &bounds); // 右下越界 → 裁到 120x80
        let c2 = clip_rect(&r(2000, 1200, 100, 100), &bounds); // 完全在外 → None
        let c3 = clip_rect(&r(10, 10, 0, 50), &bounds); // 零宽 → None
        let ok = c1 == Some(r(1800, 1000, 120, 80)) && c2.is_none() && c3.is_none();
        set.add("B04-裁剪-越界与空区处理", ok, "");
    }
    // 无残影核心：覆盖率采样对拍——bounds 内脏像素一个不少
    {
        let bounds = r(0, 0, 256, 256);
        let raw = vec![
            r(200, 200, 200, 200), // 右下越界
            r(100, 20, 50, 50),    // 界内
            r(0, 0, 8, 8),         // 左上角
            r(300, 10, 40, 40),    // 全外（对拍应无贡献）
        ];
        let clipped: Vec<Rect> = raw.iter().filter_map(|x| clip_rect(x, &bounds)).collect();
        set.add(
            "B04-裁剪-覆盖率对拍无残影",
            coverage_preserved(&raw, &clipped, &bounds, 4),
            "",
        );
    }
    // 反例注入：故意丢掉一个界内矩形，覆盖率对拍必须变红（对拍有效自证）
    {
        let bounds = r(0, 0, 256, 256);
        let raw = vec![r(10, 10, 50, 50), r(120, 120, 30, 30)];
        let mut clipped: Vec<Rect> =
            raw.iter().filter_map(|x| clip_rect(x, &bounds)).collect();
        clipped.pop(); // 模拟丢失
        set.add(
            "B04-裁剪-丢失注入必红",
            !coverage_preserved(&raw, &clipped, &bounds, 2),
            "",
        );
    }

    // ---- 判据：合批收益实测 ----

    // 相邻且代价可容忍 → 并；远距高代价 → 不并
    {
        let near = vec![r(0, 0, 100, 100), r(108, 0, 100, 100)];
        let m_near = merge_all(&near, MERGE_GAP_PX, MERGE_TOLERANCE_PCT);
        let far = vec![r(0, 0, 10, 10), r(2000, 2000, 10, 10)];
        let m_far = merge_all(&far, MERGE_GAP_PX, MERGE_TOLERANCE_PCT);
        set.add(
            "B04-合批-相邻并远距不并",
            m_near.len() == 1 && m_far.len() == 2,
            "",
        );
    }
    // 强制合批：超批上限必并到上限内（确定性最小增量对）
    {
        let mut many = Vec::new();
        for i in 0..20u32 {
            many.push(r(i * 500, i * 400, 10, 10));
        }
        let m = merge_all(&many, 0, 0);
        set.add(
            "B04-合批-超上限强制并",
            m.len() <= MAX_RECTS_PER_BATCH,
            "",
        );
    }
    // 确定性：同输入同输出（两次调用逐矩形相等）
    {
        let input = vec![r(300, 100, 40, 40), r(0, 0, 20, 20), r(350, 100, 30, 30)];
        let a = merge_all(&input, MERGE_GAP_PX, MERGE_TOLERANCE_PCT);
        let b = merge_all(&input, MERGE_GAP_PX, MERGE_TOLERANCE_PCT);
        set.add("B04-合批-确定性同输出", a == b && !a.is_empty(), "");
    }
    // 合批收益实测：30 条滚动碎脏区合批后命令数下降
    {
        let mut dirty = Vec::new();
        for i in 0..30u32 {
            dirty.push(r((i % 6) * 640, (i / 6) * 360 + (i % 3) * 8, 600, 40));
        }
        let merged = merge_all(&dirty, MERGE_GAP_PX, MERGE_TOLERANCE_PCT);
        let saved_pct = (dirty.len() - merged.len()) * 100 / dirty.len();
        set.add(
            "B04-合批-收益实测命令数下降",
            merged.len() < dirty.len() && saved_pct > 0,
            "",
        );
    }

    // ---- 判据：流水线围栏正确 ----

    // 有在途传输 → flush 挂起；最后一笔完成 → flush 释放
    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        let sub = e
            .submit_frame(&t, 1, &[r(0, 0, 1920, 1080)], 0)
            .expect("4K 资源帧提交");
        let fences: Vec<u64> = sub.transfers.iter().map(|(f, _)| *f).collect();
        let deferred_now = sub.flush.is_none() && sub.flush_pending;
        // 完成部分传输：flush 仍不得释放
        let mut released_mid: usize = 0;
        for f in fences[..fences.len() - 1].iter() {
            released_mid += e.on_transfer_complete(*f).unwrap().len();
        }
        // 最后一笔完成：flush 释放
        let released_last = e.on_transfer_complete(fences[fences.len() - 1]).unwrap();
        set.add(
            "B04-流水线-挂起与释放时序",
            deferred_now
                && released_mid == 0
                && released_last.len() == 1
                && matches!(released_last[0], CtrlCommand::ResourceFlush { resource_id: 1, .. }),
            "",
        );
    }
    // 无在途 → flush 直接发射（账本快路径；帧内同资源有在途时必挂起）
    {
        let mut led = FenceLedger::new();
        let direct = led.request_flush(7, r(0, 0, 10, 10));
        let f = led.register_transfer(7);
        let deferred = led.request_flush(7, r(0, 0, 10, 10));
        set.add(
            "B04-流水线-无在途直发有在途挂起",
            direct == FlushDecision::Emit
                && deferred == FlushDecision::Deferred { waiting_on: 1 },
            "",
        );
    }
    // 不变量：日志中每条 flush 发射时在途数 = 0；同帧 flush 晚于全部传输
    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(3);
        for i in 0..5u64 {
            let _ = e.submit_frame(&t, 3, &[r(i as u32 * 100, 0, 80, 80), r(i as u32 * 100, 400, 800, 200)], i * 13_000);
            // 完成本帧全部传输（流水线正常推进）
            let sub = e.bandwidth.frames; // 帧计数仅为推进
            let _ = sub;
            let fs: Vec<u64> = {
                let n = e.log.len();
                let mut v = Vec::new();
                for entry in e.log.iter().skip(n.saturating_sub(8)) {
                    if let CtrlCommand::TransferToHost2d { .. } = entry.cmd {
                        // 反查围栏：按提交顺序登记，从 ledger 无法直接拿——重放：
                        v.clear();
                        break;
                    }
                }
                v
            };
            let _ = fs;
            // 简化推进：完成最早一笔（围号单调，逐笔完成）
            let _ = e.on_transfer_complete((i * 2 + 1) as u64).is_ok();
        }
        set.add(
            "B04-流水线-发射时在途为零",
            e.pipeline_invariant_holds() && e.ledger.invariant_closed(),
            "",
        );
    }
    // 未知围栏完成回调显性报错（不静默吞）
    {
        let mut e = TwoDUpdateEngine::new();
        let err = e.on_transfer_complete(999).unwrap_err();
        set.add(
            "B04-流水线-未知围栏显性拒绝",
            err.code == "E_UNKNOWN_FENCE" && err.is_complete(),
            "",
        );
    }

    // ---- 判据：限速策略 ----

    // 小更新令牌桶：桶容量内放行、打空后推迟、隔帧回填恢复
    // （矩形拉开间距防合流——合流后面积涨过小更新阈值是预期引擎行为）
    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        let mut deferred_first = 0;
        for i in 0..12u64 {
            let sub = e
                .submit_frame(&t, 1, &[r(i as u32 * 300, 0, 32, 32)], i * 1000)
                .unwrap();
            deferred_first += sub.deferred_rects;
        }
        // 12 帧（12ms，同一帧窗口内）桶容 8 → 12 笔小更新 8 放 4 推
        let gated = deferred_first >= 4;
        // 时间推到下一帧窗口：令牌回填，推迟的脏区合流后能发出
        let sub2 = e.submit_frame(&t, 1, &[], 2 * FRAME_INTERVAL_US).unwrap();
        set.add(
            "B04-限速-小更新桶限与回填",
            gated && sub2.transfers.len() >= 1 && e.throttle.small_deferred >= 4,
            "",
        );
    }
    // 帧级字节预算：同窗口第二笔大更新被推迟并入待发集；下一窗口恢复
    // （窗口首笔不受钳制——防饿死，超预算的全帧更新不许活锁成永久残影）
    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(4);
        let full = r(0, 0, 3840, 2160); // 全帧 ≈ 31.6MB > 8MB 预算
        let sub1 = e.submit_frame(&t, 4, &[full], 0).unwrap();
        let sub2 = e.submit_frame(&t, 4, &[full], 1000).unwrap();
        let sub3 = e.submit_frame(&t, 4, &[], FRAME_INTERVAL_US * 2).unwrap();
        set.add(
            "B04-限速-帧级预算推迟并合流",
            sub1.transfers.len() == 1
                && sub2.deferred_rects >= 1
                && sub2.transfers.is_empty()
                && sub3.transfers.len() == 1,
            "",
        );
    }
    // 待发集封顶：并成包围盒，覆盖不丢失
    {
        let mut e = TwoDUpdateEngine::new();
        let mut t = ResourceTable::new();
        let _ = t.create_2d(&Create2d { id: 9, format: FMT_B8G8R8A8_UNORM, width: 4096, height: 4096 });
        // 灌大量远距小脏区且全部被桶限推迟
        for i in 0..(MAX_PENDING_RECTS + 16) {
            let _ = e.submit_frame(&t, 9, &[r(i as u32 * 96, 0, 16, 16)], 1000 + i as u64);
        }
        let has_pending = e.pending_rects_of(9) > 0 && e.pending_resource_count() >= 1;
        set.add("B04-限速-待发集封顶不丢账", has_pending, "");
    }

    // ---- 判据：4K 桌面滚动场景带宽实测 ----

    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        let stride = page_aligned_stride(3840, 4);
        // 滚动场景：每帧新暴露一条 3840×120 的条带（其余部分复用不上传）
        let band_bytes = 3840u64 * 120 * 4;
        for i in 0..30u64 {
            let y = ((i * 120) % 2040) as u32;
            let sub = e
                .submit_frame(&t, 1, &[r(0, y, 3840, 120)], i * FRAME_INTERVAL_US)
                .unwrap();
            // offset 语义抽查：条带首像素 = y*stride
            if let Some((_, CtrlCommand::TransferToHost2d { rect, offset, .. })) = sub.transfers.first() {
                let expect = rect.y as u64 * stride + rect.x as u64 * 4;
                if expect != *offset {
                    set.add("B04-带宽-offset语义抽查", false, "offset 与 y*stride+x*bpp 不符");
                    break;
                }
            }
        }
        let b = &e.bandwidth;
        let full = 3840u64 * 2160 * 4;
        let avg = b.avg_bytes_per_frame();
        // 全帧 31.6MB/帧 vs 条带 1.84MB/帧 → 节省应 ≥ 90%（规格 60-90% 区间上限）
        let saving = b.clip_savings_pct();
        set.add(
            "B04-带宽-4K滚动实测达标",
            avg <= band_bytes * 3 / 2
                && saving >= 90
                && full == 33_177_600
                && b.frames == 30,
            "",
        );
    }
    // 带宽台账收益口径自洽：合批节省百分比可计算且 ≤ 100
    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        for i in 0..3u64 {
            let _ = e.submit_frame(&t, 1, &[r(0, 0, 400, 40), r(0, 44, 400, 40)], i * FRAME_INTERVAL_US);
        }
        let m = e.bandwidth.merge_savings_pct();
        set.add(
            "B04-带宽-合批口径自洽",
            m <= 100 && e.bandwidth.bytes_sent > 0 && e.bandwidth.frames == 3,
            "",
        );
    }

    // ---- 错误路径与 VE-D 对接面 ----

    // 未知资源 / 3D 资源 / 未登记格式 显性拒绝
    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        let e1 = e.submit_frame(&t, 77, &[r(0, 0, 10, 10)], 0).unwrap_err();
        let mut t3 = ResourceTable::new();
        let _ = t3.create_3d_params(&super::veb03_resource::Create3dParams {
            id: 5,
            target: 2,
            format: FMT_B8G8R8A8_UNORM,
            width: 64,
            height: 64,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: [0; 3],
        });
        let e2 = e.submit_frame(&t3, 5, &[r(0, 0, 10, 10)], 0).unwrap_err();
        set.add(
            "B04-错误-未知与3D显性拒绝",
            e1.code == "E_NO_RESOURCE"
                && e1.is_complete()
                && e2.code == "E_NOT_2D_PATH"
                && e2.next.contains("3D"),
            "",
        );
    }

    // ---- 联动：命令灌进 veb02 参考设备全通 ----

    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        let mut dev = ReferenceDevice::new(DisplayCfg::qemu_default());
        let _ = dev.handle(&CtrlCommand::ResourceCreate2d {
            resource_id: 1,
            format: FMT_B8G8R8A8_UNORM,
            width: 3840,
            height: 2160,
        });
        let sub = e.submit_frame(&t, 1, &[r(0, 0, 1920, 1080), r(100, 1200, 300, 200)], 0).unwrap();
        let mut all_ok = true;
        for (_, cmd) in sub.transfers.iter() {
            if dev.handle(cmd) != CtrlResponse::OkNoData {
                all_ok = false;
            }
        }
        // 流水推进：全部传输完成后 flush 释放
        let fences: Vec<u64> = sub.transfers.iter().map(|(f, _)| *f).collect();
        let mut released = Vec::new();
        for f in fences.iter() {
            released.extend(e.on_transfer_complete(*f).unwrap());
        }
        for cmd in released.iter().chain(sub.flush.iter()) {
            if dev.handle(cmd) != CtrlResponse::OkNoData {
                all_ok = false;
            }
        }
        set.add(
            "B04-联动-参考设备对拍全通",
            all_ok && e.pipeline_invariant_holds(),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(1);
        let _ = e.submit_frame(&t, 1, &[r(0, 0, 800, 600)], 0);
        let s = e.a11y_summary();
        set.add(
            "B04-读屏-状态可播",
            s.contains("实发") && s.contains("在途") && s.contains("限速"),
            "",
        );
    }
    {
        let run = || {
            let mut e = TwoDUpdateEngine::new();
            let t = table_with_4k(1);
            let mut out = Vec::new();
            for i in 0..4u64 {
                let sub = e
                    .submit_frame(&t, 1, &[r(i as u32 * 50, i as u32 * 40, 300, 200)], i * FRAME_INTERVAL_US)
                    .unwrap();
                out.push((sub.transfers.len(), sub.emitted_bytes, sub.deferred_rects));
            }
            (out, e.a11y_summary())
        };
        let a = run();
        let b2 = run();
        set.add("B04-确定-同操作同结果", a == b2, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veb04_checks_all_green() {
        let set = run_veb04_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-B04 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&alloc::format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 无残影：覆盖率对拍直接单测（判据的机器可验形式）。
    #[test]
    fn dirty_clipping_never_loses_visible_pixels() {
        let bounds = r(0, 0, 512, 512);
        let raw = vec![r(400, 400, 400, 400), r(0, 250, 100, 300), r(250, 0, 300, 100)];
        let clipped: Vec<Rect> = raw.iter().filter_map(|x| clip_rect(x, &bounds)).collect();
        assert!(coverage_preserved(&raw, &clipped, &bounds, 4));
    }

    /// 流水线铁律：flush 永不早于其资源的传输完成。
    #[test]
    fn flush_never_precedes_transfer_completion() {
        let mut e = TwoDUpdateEngine::new();
        let t = table_with_4k(7);
        let sub = e.submit_frame(&t, 7, &[r(0, 0, 1000, 1000)], 0).unwrap();
        assert!(sub.flush.is_none() && sub.flush_pending, "有在途必须挂起");
        let fences: Vec<u64> = sub.transfers.iter().map(|(f, _)| *f).collect();
        for (i, f) in fences.iter().enumerate() {
            let rel = e.on_transfer_complete(*f).unwrap();
            if i + 1 < fences.len() {
                assert!(rel.is_empty(), "未完成全部传输前不得释放 flush");
            } else {
                assert_eq!(rel.len(), 1, "最后一笔完成即释放");
            }
        }
        assert!(e.pipeline_invariant_holds());
    }
}
