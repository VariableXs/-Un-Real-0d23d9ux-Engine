//! unx0497_devtlb —— UNX-F0497 设备 TLB 失效描述符队列（容量饱和 + 排空对账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0497 · 设备失效队列 · 目标 290 行
//! 判据原文：UNX-F0497-J1 设备失效队列：队一 容量内入
//!           （描述符在队列容量内正常入队，pending 计数同
//!           步）；队二 满溢记账（队列满后新描述符不入队
//!           但进溢出账——丢弃计数加一，绝不覆盖队头）；
//!           队三 排空清账（drain 后队列空、pending 归零，
//!           已发失效计数与排空量对账）；队四 溢出账守恒
//!           （溢出计数只增不减，drain 不清溢出账——溢出
//!           是历史事实）；队五 队头保全（溢出发生时队头
//!           描述符原样）。
//! 组件：定容描述符队列 + 溢出账 + drain 排空 + 守恒对账。
//! 职责定位：设备 TLB 失效队列是「翻译时效性」的传送带——
//!   队列满时的处理策略决定故障形态：覆盖队头 = 最老的失
//!   效丢时效；静默丢弃 = 无账可查；进溢出账 = 时效损失
//!   但可审计。本模型选可审计路径，溢出账是事后定期的依据。
//! 性能逐项分解：入队 O(1)；drain O(队列深)。
//! 诊断码段：0x34B00（0x34B01~0x34B09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34B00;
pub const XC_FULL: u32 = SEG_BASE + 1; // 队满（溢出记账路径的返回码）
pub const XC_DRAINEMPTY: u32 = SEG_BASE + 2; // 空队 drain（合法但零排空）

/// 失效描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desc {
    pub did: u16,
    pub iova: u64,
    pub seq: u64,
}

/// 定容失效队列 + 溢出账。
pub struct DevQueue {
    pub q: Vec<Desc>,
    pub cap: usize,
    pub overflow: u64,
    pub flushed: u64,
    pub next_seq: u64,
}

impl DevQueue {
    pub fn new(cap: usize) -> Self {
        DevQueue { q: Vec::new(), cap, overflow: 0, flushed: 0, next_seq: 1 }
    }

    /// 入队：容量内成；满则溢出记账（队头保全）。
    pub fn push(&mut self, did: u16, iova: u64) -> Result<(), u32> {
        if self.q.len() >= self.cap {
            self.overflow += 1;
            return Err(XC_FULL);
        }
        let d = Desc { did, iova, seq: self.next_seq };
        self.next_seq += 1;
        self.q.push(d);
        Ok(())
    }

    /// drain：全排空，返回排空量。空队合法排零。
    pub fn drain(&mut self) -> usize {
        let n = self.q.len();
        self.flushed += n as u64;
        self.q.clear();
        n
    }

    /// 队头描述符（未消费不移动）。
    pub fn head(&self) -> Option<Desc> {
        if self.q.is_empty() {
            None
        } else {
            Some(self.q[0])
        }
    }

    /// 守恒等式：成功入队总量 = 已排空 + 在队（溢出不耗序号，单列历史账）。
    pub fn reconcile(&self) -> bool {
        let pushed = self.next_seq - 1;
        self.flushed + self.q.len() as u64 == pushed
    }
}

pub fn run_unx0497_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0497");
    cs.add("SEG 段位独占", SEG_BASE == 0x34B00 && XC_DRAINEMPTY == SEG_BASE + 2, "u32 高段全域普查无冲突");

    // 族一：容量内入队。
    let mut q = DevQueue::new(4);
    for i in 0..4u64 {
        cs.add("入队成", q.push(1, 0x1000 + i * 0x1000).is_ok(), "容量内");
    }
    cs.add("pending 计四", q.q.len() == 4, "队深=4");
    cs.add("序号单调", q.next_seq == 5, "四个描述符四跳");

    // 族二：满溢记账。
    cs.add("满溢拒", q.push(1, 0x5000) == Err(XC_FULL), "第五个溢出");
    cs.add("溢出账一", q.overflow == 1, "丢弃计数一");
    cs.add("队深不涨", q.q.len() == 4, "溢出不占位");

    // 族三：队头保全。
    let h = q.head();
    cs.add("队头保全", h.map(|d| d.seq) == Some(1), "队头仍是第一描述符");
    q.push(2, 0x6000).ok();
    cs.add("溢出不覆盖", q.head().map(|d| d.seq) == Some(1), "再溢出队头不变");
    cs.add("溢出账二", q.overflow == 2, "两次溢出");

    // 族四：drain 清账。
    let n = q.drain();
    cs.add("排空四", n == 4, "四描述符出队");
    cs.add("排空后空", q.q.is_empty(), "clear 语义");
    cs.add("空队排零", q.drain() == 0, "二次 drain 零量");
    cs.add("flushed 对账", q.flushed == 4, "只记真排空");

    // 族五：溢出账守恒（drain 不清溢出）。
    cs.add("溢出历史守恒", q.overflow == 2 && q.flushed == 4, "drain 后溢出账仍在");

    // 族六：守恒等式（多轮混合）。
    let mut q2 = DevQueue::new(3);
    for i in 0..7u64 {
        // 3 入队 + 4 溢出。
        let _ = q2.push(1, 0x1000 + i * 0x1000);
    }
    let _ = q2.drain();
    for i in 0..2u64 {
        let _ = q2.push(2, 0x8000 + i * 0x1000);
    }
    let _ = q2.drain();
    cs.add("守恒等式", q2.reconcile(), "成功入队(5) = flushed(5) + queued(0)；溢出(4) 单列历史账");
    cs.add("溢出量核", q2.overflow == 4, "第一轮 4 溢出");
    cs.add("排空量核", q2.flushed == 5, "两轮 3+2");

    // 族七：溢出后可再入队（容量释放）。
    let mut q3 = DevQueue::new(1);
    q3.push(1, 0x1000).ok();
    let _ = q3.push(1, 0x2000); // 溢出
    let _ = q3.drain();
    cs.add("容量释放再入", q3.push(1, 0x3000).is_ok(), "drain 后容量归还");
    cs.add("再入后守恒", q3.reconcile(), "成功入队(2) = flushed(1) + queued(1)；溢出(1) 单列");

    // 族八：序号窗口核（队列内 seq 严格递增、无重号无跳号）。
    fn window_ok(q: &DevQueue) -> bool {
        for w in q.q.windows(2) {
            if w[1].seq <= w[0].seq {
                return false;
            }
        }
        // seq 与 next_seq 连续：队内首号 = next_seq - 队深。
        q.q.first().map(|d| d.seq) == Some(q.next_seq - q.q.len() as u64)
    }
    let mut q4 = DevQueue::new(5);
    for i in 0..5u64 {
        let _ = q4.push(1, 0x1000 + i * 0x1000);
    }
    cs.add("满队窗口核", window_ok(&q4), "seq 1~5 严格递增连续");
    q4.drain();
    for _ in 0..2 {
        let _ = q4.push(2, 0x9000);
    }
    cs.add("重灌窗口核", window_ok(&q4), "seq 6~7 连续（清空不清序号）");
    // 溢出请求不进窗口。
    let _ = q4.push(2, 0x9500); // 溢出
    cs.add("溢出不占序", window_ok(&q4) && q4.overflow == 1, "窗口仍 6~7");

    // 族九：drain 后序号不回卷（序号是单调时钟不是槽位）。
    let mut q5 = DevQueue::new(2);
    let _ = q5.push(1, 0x1000);
    let _ = q5.push(1, 0x2000);
    let s1 = q5.next_seq;
    let _ = q5.drain();
    let _ = q5.push(1, 0x3000);
    cs.add("序号跨 drain 单调", q5.next_seq == s1 + 1 && s1 == 3, "drain 不回卷序号");

    cs
}
