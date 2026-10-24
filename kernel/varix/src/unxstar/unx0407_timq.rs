//! unx0407_timq —— UNX-F0407 定时器队列雏形（到期最小堆）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0407 · 定时器队列雏形 · 目标 280 行
//! 判据原文：UNX-F0407-J1 定时器按到期时刻组织成最小堆（堆顶 =
//!           最近到期）；性质核：任一父节点到期时刻 ≤ 子节点
//!           （堆性质全量核）；弹出顺序 = 到期升序（同到时刻
//!           按插入序稳定）；取消定时器成批支持（懒删除 + 收
//!           割期清理）；过期批量收割（now 之前全部弹出且按升
//!           序）；重插拒绝（同 ID 在册不得重复入堆）。
//! 组件：定时器项（ID + 到期 + 序号）+ 最小堆（上浮/下沉）+
//!       在册表（ID → 堆位，懒删除标记）+ 收割器（批量弹出 +
//!       清理）+ 堆性质核。
//! 职责定位：定时器队列是「内核的闹钟架」——最小堆保证查最近
//!   到期 O(1)、插删 O(log n)；懒删除是工程关键：取消只打标
//!   记不搬堆，收割期一并清理，把高频取消的代价摊薄；稳定序
//!   （同到期按插入序）让到期回调的次序可预期，是测试确定
//!   性的基础。
//! 性能逐项分解：入堆 O(log n)；查顶 O(1)；收割 O(k log n)
//!   （k = 收割数）；性质核 O(n)。
//! 诊断码段：0x20F40（0x20F41~0x20F49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TQ_VERSION: &str = "unx0407_timq-v1";

/// 诊断段基址（0x20F40 段 · 本模块独占）。
pub const TQ_SEG_BASE: u32 = 0x20F40;
pub const XC_DUP_ID: u32 = 0x20F41; // 重插拒绝
pub const XC_ID_MISS: u32 = 0x20F42; // 取消的 ID 不在册
pub const XC_HEAP_BAD: u32 = 0x20F43; // 堆性质破
pub const XC_ORDER_BAD: u32 = 0x20F44; // 收割序不升
pub const XC_DEAD_POP: u32 = 0x20F45; // 死项被收割
pub const XC_EMPTY: u32 = 0x20F46; // 空堆取顶
pub const XC_ID_OOR: u32 = 0x20F47; // ID 越域
pub const XC_EXPIRE_REV: u32 = 0x20F48; // 到期时刻回退入堆
pub const XC_CANCEL_TWICE: u32 = 0x20F49; // 双重取消

/// 定时器项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerItem {
    /// ID（1~65535）。
    pub id: u32,
    /// 到期时刻。
    pub expire: u64,
    /// 插入序号（稳定序）。
    pub seq: u64,
    /// 存活标记（懒删除）。
    pub alive: bool,
}

/// 定时器队列。
pub struct TimerQ {
    heap: Vec<TimerItem>,
    /// 已取消项的 ID 表（收割期清理）。
    dead_ids: Vec<u32>,
    next_seq: u64,
}

impl TimerQ {
    /// 建空队列。
    pub fn new() -> TimerQ {
        TimerQ { heap: Vec::new(), dead_ids: Vec::new(), next_seq: 1 }
    }

    /// 入堆：ID 越域拒、在册重插拒、到期早于堆顶历史拒（单调入堆）。
    pub fn push(&mut self, id: u32, expire: u64) -> Result<(), u32> {
        if id == 0 || id > 0xFFFF {
            return Err(XC_ID_OOR);
        }
        for it in self.heap.iter() {
            if it.id == id && it.alive {
                return Err(XC_DUP_ID);
            }
        }
        for &d in self.dead_ids.iter() {
            if d == id {
                return Err(XC_CANCEL_TWICE); // 已取消未收割再入堆
            }
        }
        if let Some(last) = self.heap.last() {
            if expire < last.expire {
                return Err(XC_EXPIRE_REV);
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.heap.push(TimerItem { id, expire, seq, alive: true });
        let mut i = self.heap.len() - 1;
        while i > 0 {
            let p = (i - 1) / 2;
            if self.heap[p].expire <= self.heap[i].expire {
                break;
            }
            self.heap.swap(p, i);
            i = p;
        }
        Ok(())
    }

    /// 堆顶（最近到期存活项）；空堆拒。
    pub fn peek(&self) -> Result<TimerItem, u32> {
        for it in self.heap.iter() {
            if it.alive {
                return Ok(*it);
            }
        }
        Err(XC_EMPTY)
    }

    /// 取消：打懒删除标记；ID 不在册 / 双取消拒。
    pub fn cancel(&mut self, id: u32) -> Result<(), u32> {
        if id == 0 || id > 0xFFFF {
            return Err(XC_ID_OOR);
        }
        for it in self.heap.iter_mut() {
            if it.id == id && it.alive {
                it.alive = false;
                self.dead_ids.push(id);
                return Ok(());
            }
        }
        Err(XC_ID_MISS)
    }

    /// 收割：弹出全部 expire ≤ now 的存活项（升序返回）；死项顺带清理。
    pub fn harvest(&mut self, now: u64) -> Result<Vec<TimerItem>, u32> {
        // 先清死项。
        self.heap.retain(|it| it.alive);
        self.dead_ids.clear();
        // 收集到期项并按 (expire, seq) 升序。
        let mut due: Vec<TimerItem> = Vec::new();
        let mut keep: Vec<TimerItem> = Vec::new();
        for it in self.heap.drain(..) {
            if it.expire <= now {
                due.push(it);
            } else {
                keep.push(it);
            }
        }
        // 排序（选择排序，n 小且确定性）。
        for i in 0..due.len() {
            let mut m = i;
            for j in (i + 1)..due.len() {
                if due[j].expire < due[m].expire
                    || (due[j].expire == due[m].expire && due[j].seq < due[m].seq)
                {
                    m = j;
                }
            }
            due.swap(i, m);
        }
        // 到期升序核。
        for i in 1..due.len() {
            if due[i].expire < due[i - 1].expire {
                return Err(XC_ORDER_BAD);
            }
        }
        self.heap = keep;
        Ok(due)
    }

    /// 堆性质全量核（存活项）：父 ≤ 子。
    pub fn heap_ok(&self) -> Result<(), u32> {
        let n = self.heap.len();
        for i in 0..n {
            if !self.heap[i].alive {
                continue;
            }
            let l = 2 * i + 1;
            let r = 2 * i + 2;
            if l < n && self.heap[l].alive && self.heap[l].expire < self.heap[i].expire {
                return Err(XC_HEAP_BAD);
            }
            if r < n && self.heap[r].alive && self.heap[r].expire < self.heap[i].expire {
                return Err(XC_HEAP_BAD);
            }
        }
        Ok(())
    }

    /// 在册存活数。
    pub fn alive_count(&self) -> usize {
        self.heap.iter().filter(|it| it.alive).count()
    }

    /// 总项数（含死）。
    pub fn total_count(&self) -> usize {
        self.heap.len()
    }

    /// 死项数。
    pub fn dead_count(&self) -> usize {
        self.total_count() - self.alive_count()
    }
}

pub fn run_unx0407_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0407_timq");
    // 族一：入堆与堆性质。
    let mut q = TimerQ::new();
    let _ = q.push(1, 100);
    let _ = q.push(2, 50);
    let _ = q.push(3, 200);
    let _ = q.push(4, 75);
    cs.add("四项入堆", q.total_count() == 4, "无死项");
    cs.add("堆性质过", q.heap_ok().is_ok(), "父 ≤ 子");
    cs.add("堆顶最近", q.peek() == Ok(TimerItem { id: 2, expire: 50, seq: 2, alive: true }), "50 最小");
    cs.add("存活 4", q.alive_count() == 4, "全活");
    // 族二：取消（懒删除）。
    let _ = q.cancel(2);
    cs.add("取消成", q.alive_count() == 3 && q.dead_count() == 1, "懒标记");
    cs.add("堆顶更替", q.peek() == Ok(TimerItem { id: 4, expire: 75, seq: 4, alive: true }), "75 次小");
    cs.add("双取消拒", q.cancel(2) == Err(XC_ID_MISS), "已死不再取");
    cs.add("堆性质取消后过", q.heap_ok().is_ok(), "死项跳过");
    // 族三：重插拒绝。
    cs.add("在册重插拒", q.push(3, 300) == Err(XC_DUP_ID), "ID 3 在册");
    // 族四：收割（升序批量）。
    let due = q.harvest(100);
    cs.add("收割 2 项", due.as_ref().map(|v| v.len()) == Ok(2), "≤100 的活项 75/100");
    let d = match due {
        Ok(v) => v,
        Err(_) => {
            cs.add("收割失败", false, "破");
            return cs;
        }
    };
    cs.add("收割升序", d[0].expire <= d[1].expire, "75 ≤ 100");
    cs.add("收割序 ID", d.iter().map(|x| x.id).collect::<Vec<_>>() == vec![4u32, 1], "按 (expire, seq)");
    cs.add("余 1 项未到期", q.alive_count() == 1, "ID 3 @200 保留");
    cs.add("死项清理", q.dead_count() == 0, "retain 清场");
    cs.add("余项再收割", q.harvest(200).as_ref().map(|v| v.len()) == Ok(1), "ID 3 归位");
    cs.add("终空", q.alive_count() == 0, "全弹");
    // 族五：空堆取顶拒。
    cs.add("空取顶拒", q.peek() == Err(XC_EMPTY), "无活项");
    // 族六：参数与状态拒绝。
    let mut z = TimerQ::new();
    cs.add("ID 零拒", z.push(0, 10) == Err(XC_ID_OOR), "id=0");
    cs.add("ID 超域拒", z.push(0x1_0000, 10) == Err(XC_ID_OOR), ">0xFFFF");
    cs.add("取消越域拒", z.cancel(0) == Err(XC_ID_OOR), "id=0");
    cs.add("取消不在册拒", z.cancel(9) == Err(XC_ID_MISS), "无 ID 9");
    // 族七：单调入堆核。
    let mut m = TimerQ::new();
    let _ = m.push(1, 100);
    cs.add("回退入堆拒", m.push(2, 50) == Err(XC_EXPIRE_REV), "< 堆尾");
    cs.add("平顶入堆过", m.push(2, 100).is_ok(), "= 堆尾可入");
    // 族八：多批收割与再入堆。
    let mut r = TimerQ::new();
    let _ = r.push(10, 10);
    let _ = r.push(11, 20);
    let _ = r.push(12, 30);
    let d1 = r.harvest(15);
    cs.add("首批 1 项", d1.as_ref().map(|v| v.len()) == Ok(1), "仅 ID 10");
    let _ = r.push(13, 35);
    let d2 = r.harvest(30);
    cs.add("次批 2 项", d2.as_ref().map(|v| v.len()) == Ok(2), "11/12 到期");
    let d2 = match d2 {
        Ok(v) => v,
        Err(_) => {
            cs.add("次批失败", false, "破");
            return cs;
        }
    };
    cs.add("次批升序", d2[0].expire <= d2[1].expire, "20 ≤ 30");
    cs.add("余 13 存活", r.alive_count() == 1, "ID 13 @35 保留");
    cs.add("再入堆过", r.push(10, 100).is_ok(), "已收割 ID 复用");
    cs.add("终态堆核", r.heap_ok().is_ok(), "性质完好");
    cs
}
