//! VE-F0211 · 域自检（判据逐条对应，见 `veb11_irq.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 解耦入队 → `C211-解耦-*`（中断侧只置位/入队、解析不在中断侧、工作线程 drain 消费）
//! - 去重合并 → `C211-去重-*`（同键窗口内合并、异键不合并、过期不合并、O(1) 槽定位）
//! - 轮询兜底 → `C211-兜底-*`（默认关、开启后超时触发、兜底不自证恢复）
//! - 未知跳过 → `C211-未知-*`（计数公开、不入环、已知码正常反查）
//!
//! 门禁设计纪律（本域自检遵守）：
//! ① 每条判据都配「注入缺陷应变红」的验证——弱门禁（恒真断言）等价无门禁；
//! ② 表内元素验查表函数恒真，故去重槽验证用表外真实形态（跨槽碰撞构造）；
//! ③ 性能自检必须实测真实工作量（计数器覆盖缺陷发生层），不做 `n*CONST` 自证式算术；
//! ④ 边界判据用表外真实形态（0 窗口、非 3 倍数长度、越界扫描输出号）。

use super::veb11_irq::*;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

/// 构造一条 12 字节事件线格式记录（code / scanout / param 全 LE u32）。
fn rec(code: u32, scanout: u32, param: u32) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0..4].copy_from_slice(&code.to_le_bytes());
    b[4..8].copy_from_slice(&scanout.to_le_bytes());
    b[8..12].copy_from_slice(&param.to_le_bytes());
    b
}

/// 构造长度字段（u32 LE）。
fn len_field(n: usize) -> [u8; 4] {
    (n as u32).to_le_bytes()
}

/// VE-F0211 域自检。
pub fn run_veb11_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb11");

    // ==================================================================
    // 一、isr cfg 读取消除语义
    // ==================================================================

    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.raise_irq(ISR_CONFIG_CHANGE, 1);
        irq.raise_irq(ISR_QUEUE_CHANGE, 2);
        let first = irq.read_and_clear_isr();
        let second = irq.read_and_clear_isr();
        set.add(
            "C211-ISR-读后自清",
            first == (ISR_CONFIG_CHANGE | ISR_QUEUE_CHANGE) && second == 0,
            "",
        );
    }
    // 读取消除后位形不得残留（假绿防线：只看返回值不看内部位）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.raise_irq(ISR_CONFIG_CHANGE, 1);
        let _ = irq.read_and_clear_isr();
        let residue = irq.read_and_clear_isr();
        let again = irq.isr_has(ISR_CONFIG_CHANGE);
        set.add("C211-ISR-无位残留", residue == 0 && !again, "");
    }
    // 未知 ISR 位形登记而非静默（已知位形谓词）
    {
        set.add(
            "C211-ISR-位形判定",
            VirtGpuIrq::is_known_isr_bits(ISR_CONFIG_CHANGE | ISR_QUEUE_CHANGE)
                && !VirtGpuIrq::is_known_isr_bits(0b1000_0000),
            "",
        );
    }
    // 多位累积 OR 语义（置位不清零：raise 两次不同位互不覆盖）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.raise_irq(ISR_CONFIG_CHANGE, 5);
        irq.raise_irq(ISR_QUEUE_CHANGE, 6);
        irq.raise_irq(ISR_QUEUE_CHANGE, 7);
        let bits = irq.read_and_clear_isr();
        set.add(
            "C211-ISR-置位累积",
            bits == (ISR_CONFIG_CHANGE | ISR_QUEUE_CHANGE) && irq.last_irq_tick() == 7,
            "",
        );
    }

    // ==================================================================
    // 二、判据「解耦入队」
    // ==================================================================

    {
        let mut irq = VirtGpuIrq::new(8, 4);
        let o1 = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 1,
            tick: 1,
        });
        let o2 = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 3,
            param: 1,
            tick: 1,
        });
        let o3 = irq.enqueue_event(Event {
            kind: EventKind::Cursor,
            scanout: 0,
            param: 7,
            tick: 1,
        });
        let drained = irq.drain();
        let after = irq.drain();
        set.add(
            "C211-解耦-入队与drain",
            o1 == EventOutcome::Accepted
                && o2 == EventOutcome::Accepted
                && o3 == EventOutcome::Accepted
                && drained.len() == 3
                && after.is_empty()
                && drained[0].scanout == 0
                && drained[0].param == 1
                && drained[2].kind == EventKind::Cursor
                && drained[2].param == 7,
            "",
        );
    }
    // 参数维保真（判据数据结构「类型×输出×参数」三维齐）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        for p in 0..4u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: p % 4,
                param: 0xDEAD_0000 | p,
                tick: 100 + p as u64,
            });
        }
        let got: Vec<u32> = irq.drain().iter().map(|e| e.param).collect();
        set.add(
            "C211-解耦-参数保真",
            got == vec![0xDEAD_0000, 0xDEAD_0001, 0xDEAD_0002, 0xDEAD_0003],
            "",
        );
    }
    // 环满覆盖最旧 + 覆盖计数公开（不静默丢）
    {
        let mut irq = VirtGpuIrq::new(2, 8);
        for s in 0..3u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: s,
                param: 0,
                tick: 1 + s as u64,
            });
        }
        let drained = irq.drain();
        set.add(
            "C211-解耦-环满保最新",
            drained.len() == 2
                && drained[0].scanout == 1
                && drained[1].scanout == 2
                && irq.overwritten == 1
                && irq.accepted == 3,
            "",
        );
    }
    // FIFO 顺序（入队序 == 消费序，强断言而非 contains）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        for s in 0..4u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Cursor,
                scanout: s,
                param: 0,
                tick: 1 + s as u64,
            });
        }
        let ids: Vec<u32> = irq.drain().iter().map(|e| e.scanout).collect();
        set.add("C211-解耦-消费保序", ids == vec![0, 1, 2, 3], "");
    }
    // drain_with 逐条处理（工作线程零中间分配路径）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        for s in 0..3u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: s,
                param: s,
                tick: 1 + s as u64,
            });
        }
        let mut acc: Vec<u32> = Vec::new();
        let n = irq.drain_with(|e| acc.push(e.scanout));
        set.add(
            "C211-解耦-drain_with处理",
            n == 3 && acc == vec![0, 1, 2] && irq.ring_len() == 0,
            "",
        );
    }
    // backlog 面（工作线程调度判据）
    {
        // scanout_count 必须 ≥ 5，否则 scanout 4/5 会被越界拒收（那是另一条判据）。
        let mut irq = VirtGpuIrq::new(8, 8);
        for s in 0..5u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: s,
                param: 0,
                tick: 1 + s as u64,
            });
        }
        let b = irq.backlog();
        let _ = irq.drain();
        set.add(
            "C211-解耦-积压可见",
            b == 5 && irq.backlog() == 0 && irq.ring_cap() == 8 && irq.rejected_scanout == 0,
            "",
        );
    }

    // ==================================================================
    // 三、判据「去重合并」
    // ==================================================================

    // 同 (类型,输出) 窗口内合并
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        irq.set_window(4);
        let a = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let b = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 3,
        });
        let c = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 6,
        });
        set.add(
            "C211-去重-窗口内合并",
            a == EventOutcome::Accepted
                && b == EventOutcome::Deduped
                && c == EventOutcome::Accepted
                && irq.deduped == 1
                && irq.ring_len() == 2,
            "",
        );
    }
    // 异类型 / 异输出不合并（表外形态：不依赖槽位分布）
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        irq.set_window(8);
        let a = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let b = irq.enqueue_event(Event {
            kind: EventKind::Cursor,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let c = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 1,
            param: 0,
            tick: 1,
        });
        set.add(
            "C211-去重-异键不合并",
            a == EventOutcome::Accepted
                && b == EventOutcome::Accepted
                && c == EventOutcome::Accepted
                && irq.deduped == 0
                && irq.ring_len() == 3,
            "",
        );
    }
    // 窗口边界：恰好等于窗口宽度不再合并（< window 语义）
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        irq.set_window(4);
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 2,
            param: 0,
            tick: 10,
        });
        let at_edge = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 2,
            param: 0,
            tick: 14,
        });
        set.add(
            "C211-去重-窗口边界",
            at_edge == EventOutcome::Accepted && irq.deduped == 0,
            "",
        );
    }
    // tick 回退（乱序到达）不 panic、不误判合并——saturating 语义
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        irq.set_window(100);
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 3,
            param: 0,
            tick: 50,
        });
        let back = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 3,
            param: 0,
            tick: 10,
        });
        set.add("C211-去重-回退tick安全", back == EventOutcome::Deduped, "");
    }
    // 首条不被哨兵误判（tick=0 首条必须 Accepted——半成品曾用 0 当哨兵）
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        let first = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 0,
        });
        set.add(
            "C211-去重-零tick首条",
            first == EventOutcome::Accepted && irq.deduped == 0,
            "",
        );
    }
    // 槽定位 O(1) 且分布可分：不同 scanout 不应全落同一槽
    {
        // 前置：scanout_count=64 覆盖全部键；ring 128 容下 65 条（否则先撞环满覆盖）；
        // window=1 且 tick 递增（每 tick 1 条，远低于风暴阈值 16——否则先被限流吃掉，
        // 测的就不是去重而是限流）。
        let mut irq = VirtGpuIrq::new(128, 64);
        irq.set_window(1);
        let mut all_ok = true;
        for s in 0..64u32 {
            let o = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: s,
                param: 0,
                tick: 1 + s as u64,
            });
            if o != EventOutcome::Accepted {
                all_ok = false;
            }
        }
        let cursor_ok = irq.enqueue_event(Event {
            kind: EventKind::Cursor,
            scanout: 0,
            param: 0,
            tick: 65,
        });
        set.add(
            "C211-去重-跨槽不误合并",
            all_ok
                && cursor_ok == EventOutcome::Accepted
                && irq.deduped == 0
                && irq.ring_len() == 65
                && irq.rejected_scanout == 0
                && irq.rate_limited == 0,
            "",
        );
    }

    // 槽冲突下各键仍保有自己的合并窗口（防「后写者顶掉前驱窗口致漏合并」——
    // 直接映射的隐蔽缺陷：比较用完整元组故不误合并，但被顶掉的键会失去合并保护）
    {
        let mut irq = VirtGpuIrq::new(128, 16);
        irq.set_window(64);
        // 前置一：16 个 Display 键各落一窗（tick 10，恰在限流阈值 16 之内）
        let mut first_ok = true;
        for s in 0..16u32 {
            let o = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: s,
                param: 0,
                tick: 10,
            });
            if o != EventOutcome::Accepted {
                first_ok = false;
            }
        }
        // 前置二：16 个 Cursor 键另落一窗（tick 11，避开限流阈值）
        let mut first_ok2 = true;
        for s in 0..16u32 {
            let o = irq.enqueue_event(Event {
                kind: EventKind::Cursor,
                scanout: s,
                param: 0,
                tick: 11,
            });
            if o != EventOutcome::Accepted {
                first_ok2 = false;
            }
        }
        // 第二轮：窗口内（window=64），同 32 键必须全部被合并——若某键的窗口记录
        // 被槽冲突顶掉，它会重新 Accepted，此处即变红。
        let mut second_all_deduped = true;
        for s in 0..16u32 {
            let o = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: s,
                param: 0,
                tick: 12,
            });
            if o != EventOutcome::Deduped {
                second_all_deduped = false;
            }
        }
        for s in 0..16u32 {
            let o = irq.enqueue_event(Event {
                kind: EventKind::Cursor,
                scanout: s,
                param: 0,
                tick: 13,
            });
            if o != EventOutcome::Deduped {
                second_all_deduped = false;
            }
        }
        set.add(
            "C211-去重-槽冲突仍保窗口",
            first_ok
                && first_ok2
                && second_all_deduped
                && irq.accepted == 32
                && irq.deduped == 32
                && irq.rate_limited == 0
                && irq.ring_len() == 32,
            "",
        );
    }
    // 跨类别同扫描输出键并存（Display/Display 同输出不能互相顶掉）
    {
        let mut irq = VirtGpuIrq::new(64, 8);
        irq.set_window(50);
        let a = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 20,
        });
        let b = irq.enqueue_event(Event {
            kind: EventKind::Cursor,
            scanout: 0,
            param: 0,
            tick: 20,
        });
        let a2 = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 21,
        });
        let b2 = irq.enqueue_event(Event {
            kind: EventKind::Cursor,
            scanout: 0,
            param: 0,
            tick: 21,
        });
        set.add(
            "C211-去重-同类异类并存",
            a == EventOutcome::Accepted
                && b == EventOutcome::Accepted
                && a2 == EventOutcome::Deduped
                && b2 == EventOutcome::Deduped
                && irq.deduped == 2,
            "",
        );
    }

    // ==================================================================
    // 四、判据「轮询兜底」
    // ==================================================================

    // 默认关：超时也不动作
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        let a = irq.watchdog_tick(1000);
        set.add(
            "C211-兜底-默认关不动作",
            a == WatchdogAction::None
                && !irq.poll_fallback_enabled()
                && irq.poll_fallback_count == 0,
            "",
        );
    }
    // 开启后：未超时 None、超时 PollNow
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.set_watchdog_timeout(10);
        irq.enable_poll_fallback(true);
        irq.raise_irq(ISR_CONFIG_CHANGE, 100);
        let a = irq.watchdog_tick(105);
        let b = irq.watchdog_tick(200);
        set.add(
            "C211-兜底-超时触发",
            a == WatchdogAction::None
                && b == WatchdogAction::PollNow
                && irq.poll_fallback_count == 1,
            "",
        );
    }
    // 关键防线：兜底不得自证恢复（连续兜底每 tick 至多触发一次且基准推进）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.set_watchdog_timeout(5);
        irq.enable_poll_fallback(true);
        irq.raise_irq(ISR_CONFIG_CHANGE, 0);
        let first = irq.watchdog_tick(100);
        let second = irq.watchdog_tick(101);
        set.add(
            "C211-兜底-不自证恢复",
            first == WatchdogAction::PollNow && second == WatchdogAction::None,
            "",
        );
    }
    // 兜底真取到数据后显式记恢复，兜底停止
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.set_watchdog_timeout(5);
        irq.enable_poll_fallback(true);
        irq.raise_irq(ISR_CONFIG_CHANGE, 0);
        let _ = irq.watchdog_tick(100);
        let bytes = rec(0x0100, 0, 1);
        let batch = irq.poll_once(&bytes, 100);
        irq.note_poll_recovered(100);
        let after = irq.watchdog_tick(103);
        set.add(
            "C211-兜底-取数后停止",
            batch.enqueued == 1 && after == WatchdogAction::None && irq.ring_len() == 1,
            "",
        );
    }
    // 兜底路径不伪造中断（poll_once 不得改 last_irq_tick）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.enable_poll_fallback(true);
        irq.raise_irq(ISR_CONFIG_CHANGE, 42);
        let bytes = rec(0x0100, 1, 0);
        let _ = irq.poll_once(&bytes, 500);
        set.add("C211-兜底-不伪造中断", irq.last_irq_tick() == 42, "");
    }

    // ==================================================================
    // 五、判据「未知跳过」
    // ==================================================================

    {
        let mut irq = VirtGpuIrq::new(8, 4);
        let o1 = irq.enqueue_event(Event {
            kind: EventKind::of_code(0x0300),
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let o2 = irq.enqueue_event(Event {
            kind: EventKind::Unknown(0x9999),
            scanout: 0,
            param: 0,
            tick: 1,
        });
        set.add(
            "C211-未知-记录跳过",
            EventKind::of_code(0x0300) == EventKind::Unknown(0x0300)
                && o1 == EventOutcome::UnknownSkipped
                && o2 == EventOutcome::UnknownSkipped
                && irq.unknown_skipped == 2
                && irq.ring_len() == 0,
            "",
        );
    }
    // 未知码原值必须保留（不猜不吞）
    {
        let k = EventKind::of_code(0xDEAD_BEEF);
        set.add(
            "C211-未知-原值保留",
            matches!(k, EventKind::Unknown(c) if c == 0xDEAD_BEEF)
                && k.code() == 0xDEAD_BEEF
                && !k.is_known()
                && k.label() == "UNKNOWN",
            "",
        );
    }
    // 已知码反查与 label
    {
        set.add(
            "C211-未知-已知码正常",
            EventKind::of_code(0x0100) == EventKind::Display
                && EventKind::of_code(0x0101) == EventKind::Cursor
                && EventKind::Display.is_known()
                && EventKind::Cursor.is_known()
                && EventKind::Display.label() == "DISPLAY"
                && EventKind::Cursor.label() == "CURSOR"
                && EventKind::Display.code() == 0x0100
                && EventKind::Cursor.code() == 0x0101,
            "",
        );
    }

    // ==================================================================
    // 六、事件队列解析（display events 事件队列）
    // ==================================================================

    // 正常批：两段式解析
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        irq.set_window(1);
        let mut body = [0u8; 24];
        body[0..12].copy_from_slice(&rec(0x0100, 0, 0x11));
        body[12..24].copy_from_slice(&rec(0x0101, 1, 0x22));
        let lf = len_field(24);
        let batch = irq.ingest(Some(&lf), &body, 7);
        let got: Vec<(u32, u32)> = irq.drain().iter().map(|e| (e.scanout, e.param)).collect();
        set.add(
            "C211-解析-两段式批量",
            batch.parsed == 2
                && batch.enqueued == 2
                && batch.malformed == 0
                && got == vec![(0, 0x11), (1, 0x22)],
            "",
        );
    }
    // 畸形批：长度非 3 倍数 → 整批判废不 panic
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        let body = rec(0x0100, 0, 0);
        let lf = len_field(7);
        let batch = irq.ingest(Some(&lf), &body, 1);
        set.add(
            "C211-解析-长度非整除判废",
            batch.malformed == 1
                && batch.parsed == 0
                && batch.enqueued == 0
                && irq.malformed_batches == 1
                && irq.ring_len() == 0,
            "",
        );
    }
    // 长度越界（声明大于实际体）→ 判废
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        let body = rec(0x0100, 0, 0);
        let lf = len_field(240);
        let batch = irq.ingest(Some(&lf), &body, 1);
        set.add(
            "C211-解析-长度越界判废",
            batch.malformed == 1 && irq.malformed_batches == 1 && irq.ring_len() == 0,
            "",
        );
    }
    // 空读与缺长度字段必须分列：设备报「无事件」（长度字段为0）是正常态，
    // 记成畸形会让正常空读污染畸形计数、真畸形被淹没；长度字段缺失才是真畸形。
    // （旧版把两者塞进同一断言，是判据错——已按语义拆开。）
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        let lf0 = len_field(0);
        let empty_read = irq.ingest(Some(&lf0), &[], 1);
        let no_len = irq.ingest(None, &[], 1);
        set.add(
            "C211-解析-空读与缺长度分列",
            empty_read.malformed == 0
                && empty_read.empty == 1
                && empty_read.parsed == 0
                && no_len.malformed == 1
                && no_len.empty == 0
                && irq.malformed_batches == 1
                && irq.empty_reads == 1
                && irq.ring_len() == 0,
            "",
        );
    }
    // 反向门禁：真畸形（非 3 倍数长度）必须仍被判废——防止「空读不算畸形」
    // 被修成「什么都不算畸形」的弱门禁。
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        let body = rec(0x0100, 0, 0);
        let lf = len_field(7);
        let batch = irq.ingest(Some(&lf), &body, 1);
        set.add(
            "C211-解析-真畸形仍判废",
            batch.malformed == 1
                && batch.parsed == 0
                && batch.empty == 0
                && irq.malformed_batches == 1
                && irq.empty_reads == 0,
            "",
        );
    }
    // 兜底轮询空读（缺陷现场）：看门狗触发时队列无待读事件是**常态**，
    // 记成畸形会让每次正常兜底都往畸形计数掺水，真畸形被淹没。
    // （缺此项判据时，把 poll_once 空读改回记畸形不会有任何项变红——实测过。）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.enable_poll_fallback(true);
        let empty = irq.poll_once(&[], 50);
        set.add(
            "C211-兜底-空读不记畸形",
            empty.malformed == 0
                && empty.empty == 1
                && empty.parsed == 0
                && irq.malformed_batches == 0
                && irq.empty_reads == 1,
            "",
        );
    }
    // 兜底轮询真畸形（非 3 倍数）必须仍判废——空读修正不得放宽真畸形。
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.enable_poll_fallback(true);
        let odd = [0u8; 7];
        let bad = irq.poll_once(&odd, 50);
        set.add(
            "C211-兜底-非整除仍判废",
            bad.malformed == 1
                && bad.empty == 0
                && irq.malformed_batches == 1
                && irq.empty_reads == 0,
            "",
        );
    }
    {
        let mut irq = VirtGpuIrq::new(16, 4);
        irq.set_window(1);
        let mut body = [0u8; 24];
        body[0..12].copy_from_slice(&rec(0x0300, 0, 0));
        body[12..24].copy_from_slice(&rec(0x0100, 1, 0x33));
        let lf = len_field(24);
        let batch = irq.ingest(Some(&lf), &body, 3);
        let got = irq.drain();
        set.add(
            "C211-解析-混合批容错",
            batch.parsed == 2
                && batch.enqueued == 1
                && batch.unknown == 1
                && got.len() == 1
                && got[0].scanout == 1
                && got[0].param == 0x33,
            "",
        );
    }
    // 解析事件 tick 取摄入 tick（parse 单条路径）
    {
        let irq = VirtGpuIrq::new(8, 4);
        let e = irq.parse_event(&rec(0x0100, 2, 0x44));
        set.add(
            "C211-解析-单条字段对齐",
            e.kind == EventKind::Display && e.scanout == 2 && e.param == 0x44,
            "",
        );
    }

    // ==================================================================
    // 七、边界防护
    // ==================================================================

    // 扫描输出号越界拒收（cfg 声明 4 个输出）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        let o = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 4,
            param: 0,
            tick: 1,
        });
        set.add(
            "C211-边界-扫描输出越界",
            o == EventOutcome::RejectedBadScanout
                && irq.rejected_scanout == 1
                && irq.ring_len() == 0,
            "",
        );
    }
    // 越界号紧邻边界内号必须入队（防「一律拒收」的过门禁假绿）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        let ok = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 3,
            param: 0,
            tick: 1,
        });
        let bad = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 4,
            param: 0,
            tick: 1,
        });
        set.add(
            "C211-边界-临界内外分明",
            ok == EventOutcome::Accepted && bad == EventOutcome::RejectedBadScanout,
            "",
        );
    }
    // cfg 未声明数量（0）时不校验扫描输出号
    {
        let mut irq = VirtGpuIrq::new(8, 0);
        let o = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 99,
            param: 0,
            tick: 1,
        });
        set.add("C211-边界-零输出不校验", o == EventOutcome::Accepted, "");
    }
    // 窗口与超时下限钳制（0 → 1）
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.set_window(0);
        irq.set_watchdog_timeout(0);
        set.add(
            "C211-边界-阈值下限",
            irq.window() == 1 && irq.watchdog_timeout() == 1,
            "",
        );
    }
    // 零容量环视作 1（不留除零崩溃面）
    {
        let mut irq = VirtGpuIrq::new(0, 4);
        let o = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        set.add(
            "C211-边界-零容量环安全",
            o == EventOutcome::Accepted && irq.ring_cap() == 1,
            "",
        );
    }
    // 风暴限流：单位 tick 超阈值即丢弃并计数（表外形态：超 STORM 阈值）
    {
        let mut irq = VirtGpuIrq::new(128, 64);
        irq.set_window(1);
        let mut limited = 0u32;
        let mut accepted = 0u32;
        for i in 0..40u32 {
            let o = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: i,
                param: 0,
                tick: 9,
            });
            match o {
                EventOutcome::Accepted => accepted += 1,
                EventOutcome::RateLimited => limited += 1,
                _ => {}
            }
        }
        set.add(
            "C211-边界-风暴限流",
            accepted == 16 && limited == 24 && irq.rate_limited == 24,
            "",
        );
    }
    // 限流计数单位 tick 边界：跨 tick 重新计数（表外形态：两个 tick）
    {
        let mut irq = VirtGpuIrq::new(128, 64);
        irq.set_window(1);
        for i in 0..16u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: i,
                param: 0,
                tick: 1,
            });
        }
        let over = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 20,
            param: 0,
            tick: 1,
        });
        let next_tick = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 21,
            param: 0,
            tick: 2,
        });
        set.add(
            "C211-边界-限流跨tick重置",
            over == EventOutcome::RateLimited && next_tick == EventOutcome::Accepted,
            "",
        );
    }

    // ==================================================================
    // 八、计数公开（遥测口径：不静默丢）
    // ==================================================================

    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.set_window(2);
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 2,
        });
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Unknown(0x77),
            scanout: 0,
            param: 0,
            tick: 2,
        });
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 9,
            param: 0,
            tick: 2,
        });
        set.add(
            "C211-计数-四路口径",
            irq.accepted == 1
                && irq.deduped == 1
                && irq.unknown_skipped == 1
                && irq.rejected_scanout == 1,
            "",
        );
    }
    // 计数与实际留环一致（不虚报）
    {
        // scanout_count=8 覆盖 scanout 0..6；window=1 且 tick 递增避免误合并。
        let mut irq = VirtGpuIrq::new(8, 8);
        irq.set_window(1);
        for i in 0..6u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: i,
                param: 0,
                tick: 1 + i as u64,
            });
        }
        let before = irq.ring_len();
        let _ = irq.drain();
        set.add(
            "C211-计数-与留环一致",
            irq.accepted == 6
                && before == 6
                && irq.ring_len() == 0
                && irq.deduped == 0
                && irq.rejected_scanout == 0,
            "",
        );
    }

    // ==================================================================
    // 九、覆盖必释窗（环满覆盖与去重窗口的交互——静默丢事件防线）
    // ==================================================================

    // 被覆盖事件从未送达消费者 → 其去重窗口必须一并释放，否则同键后续事件
    // 会被误判「已见未处理」而合并丢弃，消费者永远感知不到该键变化。
    //
    // 环容量为 1，故第二条即覆盖第一条、第三条再覆盖第二条——`overwritten` 为 2
    // （两次覆盖各带走一条窗口）；`window_released` 为 3：两次覆盖释放 + 末尾
    // `drain` 把最终那条交付给消费者时再释放一条（交付即释窗，否则后续同键
    // 真实状态变化会被合并丢弃——事件静默丢失）。
    {
        let mut irq = VirtGpuIrq::new(1, 4);
        irq.set_window(10);
        let a = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let b = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 1,
            param: 0,
            tick: 2,
        }); // 覆盖 a
        let c = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 3,
        }); // a 的窗口若未释放，此处会被误合并
        let got = irq.drain();
        set.add(
            "C211-覆盖-必释窗不误合并",
            a == EventOutcome::Accepted
                && b == EventOutcome::Accepted
                && c == EventOutcome::Accepted
                && irq.overwritten == 2
                && irq.window_released == 3
                && got.len() == 1
                && got[0].scanout == 0,
            "",
        );
    }
    // 交付即释窗（drain 路径）：消费者处理完后，同键的真实后续状态变化必须
    // 能再次入环——否则「已处理」与「未处理」不可区分，变化被静默丢弃。
    {
        let mut irq = VirtGpuIrq::new(8, 4);
        irq.set_window(50);
        let first = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 1,
            tick: 10,
        });
        let got = irq.drain();
        let released_after_drain = irq.window_released;
        // 同键、仍在窗口内（tick 差 2 < 50）——必须 Accepted，不许 Deduped
        let second = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 2,
            tick: 12,
        });
        // 未交付时同键窗口内事件仍应合并（对照：证明不是把合并功能整体拆了）
        let mut irq2 = VirtGpuIrq::new(8, 4);
        irq2.set_window(50);
        let _ = irq2.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 1,
            param: 1,
            tick: 10,
        });
        let dup = irq2.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 1,
            param: 1,
            tick: 11,
        });
        set.add(
            "C211-交付-释窗不丢变化",
            first == EventOutcome::Accepted
                && got.len() == 1
                && released_after_drain == 1
                && second == EventOutcome::Accepted
                && dup == EventOutcome::Deduped,
            "",
        );
    }
    // 反向门禁：释窗不得把去重机制整体拆废——新落环的事件必须重新拥有
    // 生效窗口（否则「一律释窗」会让同键风暴全部 Accepted）。
    {
        let mut irq = VirtGpuIrq::new(1, 4);
        irq.set_window(10);
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        });
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 1,
            param: 0,
            tick: 2,
        }); // 覆盖上一条
        let fresh = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 3,
        }); // 窗口已释放 → 应收
        let repeat = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 4,
        }); // 新事件已落环 → 其窗口必须生效，必须合并
        set.add(
            "C211-覆盖-新窗仍生效",
            fresh == EventOutcome::Accepted
                && repeat == EventOutcome::Deduped
                && irq.deduped == 1
                && irq.window_released == 2,
            "",
        );
    }
    // 三元组纪律：窗口已被同键新事件刷新（tick 不同）时，覆盖旧事件
    // 不得把新事件的窗口一起作废。表外形态：先造 tick=1 的旧事件落环，
    // 再让 tick=5（同键出窗口后）刷新窗口，最后覆盖掉的是 tick=1 那条。
    // 若作废只按 (码,输出) 粗粒度，tick=5 的窗口会被误清 → probe 变 Accepted。
    //
    // scanout_count 必须 ≥ 8，否则光标 s=4..7 会被越界拒收、环填不满、
    // 覆盖根本不发生（此坑实测踩过：那版断言恒绿但什么都没验）。
    {
        let mut irq = VirtGpuIrq::new(8, 16);
        irq.set_window(2);
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 1,
        }); // 旧事件落环（将是被覆盖的那条）
        let _ = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 5,
        }); // 出窗口后同键重收 → 窗口刷新为 tick=5
        for s in 1..8u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Cursor,
                scanout: s,
                param: 0,
                tick: 6,
            });
        }
        // 环容量 8：前两条 + 7 条光标 = 9 → 必发生一次覆盖（覆盖 tick=1 那条）
        let probe = irq.enqueue_event(Event {
            kind: EventKind::Display,
            scanout: 0,
            param: 0,
            tick: 6,
        });
        set.add(
            "C211-覆盖-释窗三元组精确",
            irq.overwritten == 1
                && irq.window_released == 0
                && probe == EventOutcome::Deduped
                && irq.rejected_scanout == 0,
            "",
        );
    }

    // ==================================================================
    // 十、去重表满如实计数（配置错配暴露，不静默）
    // ==================================================================

    // 表满时放行但不落窗，且如实计数——不得静默夺他键窗口。
    {
        let mut irq = VirtGpuIrq::new(8192, 4096);
        irq.set_window(1000);
        // 灌入远超槽数的不同键，逼探测步数耗尽
        for i in 0..4000u32 {
            let _ = irq.enqueue_event(Event {
                kind: EventKind::Display,
                scanout: i,
                param: 0,
                tick: 1 + i as u64,
            });
        }
        // 耗尽必须被计数（表满是配置错配，如实暴露）
        let exhausted_seen = irq.probe_exhausted;
        set.add(
            "C211-去重-表满如实计数",
            exhausted_seen > 0 && irq.accepted == 4000 && irq.deduped == 0,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0211 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn veb11_checks_all_green() {
        let set = run_veb11_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "veb11 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
                .0
                .iter()
                .flatten()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }

    /// 判据计数下限闸：四条主判据（解耦入队/去重合并/轮询兜底/未知跳过）
    /// 每条至少两项自检——只有一项等于没门禁（改实现即可整体翻转而不被发现）。
    ///
    /// 覆盖与解析两族另设下限：它们承载「环满不静默丢事件」与「正常态不记畸形」
    /// 两条静默防线，各族过薄同样等于无门禁。
    #[test]
    fn veb11_judgement_families_present() {
        let set = run_veb11_checks();
        let (passed, _) = set.tally();
        let names: Vec<&str> = set.red_items().0.iter().flatten().map(|c| c.name).collect();
        for family in ["解耦", "去重", "兜底", "未知"] {
            let n = names.iter().filter(|m| m.contains(family)).count();
            assert!(n >= 2, "判据族 {} 仅{} 项自检，不足两道门禁", family, n);
        }
        for family in ["覆盖", "解析"] {
            let n = names.iter().filter(|m| m.contains(family)).count();
            assert!(n >= 3, "判据族 {} 仅 {} 项自检，不足三道门禁", family, n);
        }
        // 下限随判据落实密度上抬：48 项为当前实测总数，跌破 44 说明有判据被删。
        assert!(passed >= 44, "自检项数 {} 偏少，判据落实密度不足", passed);
    }
}
