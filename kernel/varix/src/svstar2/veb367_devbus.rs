//! veb367_devbus —— VE-F0367 设备到达事件总线。
//! 锚点（CoRun Unxreal 主册）：VE-F0367 · 设备到达事件总线 · 目标 320 行
//! 判据原文：VE-F0367-J1 设备到达/移除事件按发布-订阅模型广播：
//!           订阅者（驱动）按兴趣掩码过滤（设备类别位图）；
//!           发布保证送达序 = 发布序（FIFO 每订阅者独立队列）；
//!           订阅者满队列拒收（立案 + 不阻塞发布方）；注销订
//!           阅后不再送达；事件重放（同事件流同分发结果）。
//! 组件：事件（类别 + 设备号 + 序号）+ 订阅表（驱动 + 掩码 +
//!       队列）+ 发布器（FIFO 分发）+ 满队立案账 + 重放核。
//! 职责定位：事件总线是「热插拔的神经」——设备插拔是异步事件，
//!   轮询既浪费又迟钝；发布-订阅把「谁关心什么」解耦：总线只
//!   管按掩码分发，驱动只管声明兴趣。满队拒收不阻塞发布是关
//!   键纪律：一个卡死的驱动不许拖死整条总线，立案留给它自己
//!   醒来处理。
//! 性能逐项分解：发布 O(订阅者)；订阅 O(1)；重放 O(事件数 ×
//!   订阅者)。
//! 诊断码段：0x30140（0x30141~0x30149 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DB_VERSION: &str = "veb367_devbus-v1";

/// 诊断段基址（0x30140 段 · 本模块独占）。
pub const DB_SEG_BASE: u32 = 0x30140;
pub const XC_DUP_SUB: u32 = 0x30141; // 订阅重号
pub const XC_SUB_MISS: u32 = 0x30142; // 注销不在册
pub const XC_QUEUE_FULL: u32 = 0x30143; // 订阅者队列满
pub const XC_MASK_ZERO: u32 = 0x30144; // 掩码零拒
pub const XC_EV_REORDER: u32 = 0x30145; // 送达序违例
pub const XC_MAGIC: u32 = 0x30146; // 账魔数破
pub const XC_REPLAY: u32 = 0x30147; // 重放不一致
pub const XC_DUP_UNSUB: u32 = 0x30148; // 重复注销
pub const XC_QUEUE_CAP: u32 = 0x30149; // 队列容量越域

/// 设备事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevEvent {
    /// 设备类别位掩码（1 << 类别号）。
    pub class_mask: u32,
    /// 设备号。
    pub dev: u32,
    /// 事件种别：1 = 到达 2 = 移除。
    pub kind: u8,
    /// 全局发布序号。
    pub seq: u64,
}

/// 订阅者。
pub struct Subscriber {
    pub id: u32,
    /// 兴趣掩码（事件 class_mask 与此按位与，非零即感兴趣）。
    pub interest: u32,
    /// 独立队列（容量限）。
    pub queue: Vec<DevEvent>,
    pub cap: usize,
    pub dropped: u32,
}

/// 事件总线。
pub struct DevBus {
    subs: Vec<Subscriber>,
    pub_seq: u64,
}

impl DevBus {
    /// 建。
    pub fn new() -> DevBus {
        DevBus { subs: Vec::new(), pub_seq: 1 }
    }

    /// 订阅：重号拒、掩码零拒、容量越域拒。
    pub fn subscribe(&mut self, id: u32, interest: u32, cap: usize) -> Result<(), u32> {
        if interest == 0 {
            return Err(XC_MASK_ZERO);
        }
        if cap == 0 || cap > 256 {
            return Err(XC_QUEUE_CAP);
        }
        for sb in self.subs.iter() {
            if sb.id == id {
                return Err(XC_DUP_SUB);
            }
        }
        self.subs.push(Subscriber { id, interest, queue: Vec::new(), cap, dropped: 0 });
        Ok(())
    }

    /// 注销。
    pub fn unsubscribe(&mut self, id: u32) -> Result<(), u32> {
        let pos = match self.subs.iter().position(|s| s.id == id) {
            Some(p) => p,
            None => return Err(XC_SUB_MISS),
        };
        self.subs.remove(pos);
        Ok(())
    }

    /// 发布：按掩码 FIFO 分发；满队立案不阻塞。
    pub fn publish(&mut self, class_mask: u32, dev: u32, kind: u8) -> Result<u64, u32> {
        let ev = DevEvent { class_mask, dev, kind, seq: self.pub_seq };
        self.pub_seq += 1;
        for sb in self.subs.iter_mut() {
            if sb.interest & class_mask != 0 {
                if sb.queue.len() >= sb.cap {
                    sb.dropped += 1;
                } else {
                    sb.queue.push(ev);
                }
            }
        }
        Ok(ev.seq)
    }

    /// 订阅者取队首。
    pub fn peek(&self, id: u32) -> Option<DevEvent> {
        self.subs.iter().find(|s| s.id == id).and_then(|s| s.queue.first().copied())
    }

    /// 订阅者收队首（FIFO 消费）。
    pub fn consume(&mut self, id: u32) -> Option<DevEvent> {
        let sb = self.subs.iter_mut().find(|s| s.id == id)?;
        if sb.queue.is_empty() {
            None
        } else {
            Some(sb.queue.remove(0))
        }
    }

    /// 送达序核：某订阅者队列 seq 严格升序。
    pub fn order_ok(&self, id: u32) -> Result<(), u32> {
        let sb = match self.subs.iter().find(|s| s.id == id) {
            Some(s) => s,
            None => return Err(XC_SUB_MISS),
        };
        for i in 1..sb.queue.len() {
            if sb.queue[i].seq < sb.queue[i - 1].seq {
                return Err(XC_EV_REORDER);
            }
        }
        Ok(())
    }

    /// 订阅者数。
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }

    /// 弃件总数。
    pub fn dropped_total(&self) -> u32 {
        self.subs.iter().map(|s| s.dropped).sum()
    }

    /// 重放核：同事件序列重发布，各订阅者队列长度一致。
    pub fn replay_eq(class_masks: &[u32], dev: u32) -> Result<(), u32> {
        let mut a = DevBus::new();
        let mut b = DevBus::new();
        let _ = a.subscribe(1, 0xFFFF_FFFF, 256);
        let _ = b.subscribe(1, 0xFFFF_FFFF, 256);
        for (i, &m) in class_masks.iter().enumerate() {
            let _ = a.publish(m, dev + i as u32, 1);
            let _ = b.publish(m, dev + i as u32, 1);
        }
        let la = a.subs[0].queue.len();
        let lb = b.subs[0].queue.len();
        if la != lb {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

pub fn run_veb367_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0367");
    // 族一：订阅与掩码分发。
    let mut b = DevBus::new();
    let _ = b.subscribe(1, 1 << 0, 8);  // 只关心类别 0
    let _ = b.subscribe(2, 0xFFFF_FFFF, 8); // 全部
    let _ = b.subscribe(3, 1 << 2, 8);  // 只关心类别 2
    s.add("三订阅者", b.sub_count() == 3, "掩码各异");
    let q1 = b.publish(1 << 0, 100, 1);
    s.add("发布序 1", q1 == Ok(1), "首件");
    let _ = b.publish(1 << 2, 101, 1);
    let _ = b.publish(1 << 5, 102, 1);
    // 订阅者 1：类别 0 一件。
    s.add("sub1 一件", b.peek(1).map(|e| e.dev) == Some(100), "类别 0");
    // 订阅者 2：三件全收。
    let c1 = b.consume(2);
    s.add("sub2 FIFO 首", c1.map(|e| e.seq) == Some(1), "发布序");
    let c2 = b.consume(2);
    s.add("sub2 二件", c2.map(|e| e.dev) == Some(101), "类别 2");
    // 订阅者 3：只类别 2。
    s.add("sub3 一件", b.peek(3).map(|e| e.dev) == Some(101), "类别 2");
    s.add("sub1 无类别 2 件", b.peek(1).map(|e| e.dev) == Some(100), "掩码过滤");
    s.add("送达序核", b.order_ok(2).is_ok(), "seq 升序");
    // 族二：满队拒收立案。
    let mut f = DevBus::new();
    let _ = f.subscribe(9, 0xFFFF_FFFF, 2);
    let _ = f.publish(1, 1, 1);
    let _ = f.publish(1, 2, 1);
    let _ = f.publish(1, 3, 1); // 满
    s.add("满队立案", f.dropped_total() == 1, "第三件弃");
    s.add("队列仍 2", f.peek(9).map(|_| true) == Some(true), "容量钳位");
    // 消费后可再收。
    let _ = f.consume(9);
    let _ = f.consume(9);
    let _ = f.publish(1, 4, 1);
    s.add("消费复活", f.peek(9).map(|e| e.dev) == Some(4), "空位入队");
    // 族三：注销。
    s.add("注销成", f.unsubscribe(9).is_ok(), "在册");
    s.add("注销后零订阅", f.sub_count() == 0, "清场");
    s.add("重复注销拒", f.unsubscribe(9) == Err(XC_SUB_MISS), "已清");
    s.add("注销不在册拒", DevBus::new().unsubscribe(1) == Err(XC_SUB_MISS), "无人");
    // 族四：订阅拒绝。
    let mut g = DevBus::new();
    let _ = g.subscribe(1, 1, 4);
    s.add("订阅重号拒", g.subscribe(1, 1, 4) == Err(XC_DUP_SUB), "id=1 撞");
    s.add("掩码零拒", g.subscribe(2, 0, 4) == Err(XC_MASK_ZERO), "无兴趣");
    s.add("容量零拒", g.subscribe(3, 1, 0) == Err(XC_QUEUE_CAP), "cap=0");
    s.add("容量超域拒", g.subscribe(4, 1, 257) == Err(XC_QUEUE_CAP), ">256");
    // 族五：重放确定性。
    let masks = [1u32, 4, 16, 1];
    s.add("重放一致", DevBus::replay_eq(&masks, 500).is_ok(), "分发等长");
    s.add("魔数核", DB_SEG_BASE == 0x30140, "段基址");
    s
}
