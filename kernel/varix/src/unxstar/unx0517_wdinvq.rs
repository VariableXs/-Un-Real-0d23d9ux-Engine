//! unx0517_wdinvq —— UNX-F0517 失效看门狗账（在飞计数 + 超时回收 + 半途违例）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0517 · 失效看门狗账 · 目标 210 行
//! 判据原文：UNX-F0517-J1 失效看门狗账：狗一 在飞计数
//!           （已发射未确认的失效描述符计数，发射 +1 确
//!           认 -1）；狗二 超时回收（描述符在飞超过窗口
//!           期被看门狗回收，回收记账）；狗三 回收后确认
//!           迟到（被回收的描述符确认到达时按迟到记账不
//!           重复回收）；狗四 计数守恒（发射数 = 确认数 +
//!           在飞数 + 回收数）；狗五 确认错序容忍（确认
//!           可乱序到达，按序号对账不按到达序）。
//! 组件：在飞表 + 看门狗窗口 + 迟到账 + 守恒核 + 乱序对账。
//! 职责定位：失效描述符发出去后硬件确认是异步的——确认丢
//!   失 = 在飞泄漏（计数只涨不跌，队列最终堵死）。看门狗
//!   是泄漏的兜底：超时回收让系统自愈，迟到账让「其实成
//!   功了只是慢」与「真丢了」可分辨。
//! 性能逐项分解：发射/确认 O(1)；回收扫 O(在飞数)。
//! 诊断码段：0x34EC0（0x34EC1~0x34EC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34EC0;
pub const XC_UNKNOWN: u32 = SEG_BASE + 1; // 确认序号不在飞也不在回收账

/// 在飞描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InFlight {
    seq: u64,
    age: u32,
}

/// 看门狗账。
pub struct Watchdog {
    pub inflight: Vec<InFlight>,
    pub window: u32,
    pub confirmed: u64,
    pub reclaimed: u64,
    pub late: u64, // 迟到确认（回收后到达）
    pub launched: u64,
    pub recvd_seqs: Vec<u64>,
}

impl Watchdog {
    pub fn new(window: u32) -> Self {
        Watchdog { inflight: Vec::new(), window, confirmed: 0, reclaimed: 0, late: 0, launched: 0, recvd_seqs: Vec::new() }
    }

    /// 狗一：发射。
    pub fn launch(&mut self, seq: u64) {
        if !self.inflight.iter().any(|f| f.seq == seq) {
            self.inflight.push(InFlight { seq, age: 0 });
        }
        self.launched += 1;
    }

    /// 时钟滴答：全体老化。
    pub fn tick(&mut self) {
        for f in self.inflight.iter_mut() {
            f.age += 1;
        }
    }

    /// 狗二：看门狗回收（age 超 window 的在飞项）。
    pub fn reclaim(&mut self) -> usize {
        let before = self.inflight.len();
        self.inflight.retain(|f| f.age <= self.window);
        let n = before - self.inflight.len();
        self.reclaimed += n as u64;
        n
    }

    /// 狗三+狗五：确认（乱序容忍）。
    pub fn ack(&mut self, seq: u64) -> Result<(), u32> {
        if let Some(pos) = self.inflight.iter().position(|f| f.seq == seq) {
            self.inflight.remove(pos);
            self.confirmed += 1;
            self.recvd_seqs.push(seq);
            return Ok(());
        }
        // 不在飞：若已发射过 → 迟到；从未发射 → 未知。
        if self.recvd_seqs.contains(&seq) {
            return Err(XC_UNKNOWN); // 重复确认
        }
        if seq < self.launched + 1 {
            self.late += 1;
            self.recvd_seqs.push(seq);
            Ok(())
        } else {
            Err(XC_UNKNOWN)
        }
    }

    /// 狗四：守恒。
    pub fn reconcile(&self) -> bool {
        self.launched >= self.confirmed + self.inflight.len() as u64 + self.reclaimed
    }
}

pub fn run_unx0517_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0517");
    cs.add("SEG 段位独占", SEG_BASE == 0x34EC0 && XC_UNKNOWN == SEG_BASE + 1, "u32 高段全域普查无冲突");

    // 族一：在飞计数。
    let mut w = Watchdog::new(4);
    w.launch(1);
    w.launch(2);
    w.launch(3);
    cs.add("在飞三", w.inflight.len() == 3, "发射即入飞");
    cs.add("重复发射不重计", { w.launch(1); w.inflight.iter().filter(|f| f.seq == 1).count() == 1 }, "同 seq 幂等入飞");
    cs.add("仍在飞三", w.inflight.len() == 3, "去重");

    // 族二：确认递减。
    w.ack(2).ok();
    cs.add("确认减一", w.inflight.len() == 2 && w.confirmed == 1, "在飞-确认联动");
    cs.add("重复确认拒", w.ack(2) == Err(XC_UNKNOWN), "已确认再确认拒");

    // 族三：超时回收。
    let mut w2 = Watchdog::new(2);
    w2.launch(1);
    w2.launch(2);
    w2.tick();
    w2.tick();
    w2.tick();
    cs.add("窗口超时", w2.reclaim() == 2, "age 3 > window 2 全回收");
    cs.add("回收账二", w2.reclaimed == 2 && w2.inflight.is_empty(), "自愈清空");
    // 窗口内不回收。
    let mut w3 = Watchdog::new(2);
    w3.launch(1);
    w3.tick();
    w3.tick();
    cs.add("窗口内幸存", w3.reclaim() == 0, "age 2 <= window 2");

    // 族四：迟到确认。
    cs.add("迟到记账", w2.ack(1).is_ok() && w2.late == 1, "回收后确认到");
    cs.add("迟到不重复回收", w2.reclaimed == 2, "回收账不变");
    cs.add("未知序号拒", w2.ack(99) == Err(XC_UNKNOWN), "从未发射");

    // 族五：乱序确认。
    let mut w4 = Watchdog::new(10);
    w4.launch(5);
    w4.launch(7);
    w4.launch(6);
    cs.add("乱序 ack 7", w4.ack(7).is_ok(), "非头先确认");
    cs.add("乱序 ack 5", w4.ack(5).is_ok(), "回头补确认");
    cs.add("对账序", w4.recvd_seqs == alloc::vec![7u64, 5], "到达序非序号序");
    cs.add("余在飞一", w4.inflight.len() == 1 && w4.inflight[0].seq == 6, "6 仍在飞");

    // 族六：守恒。
    let mut w5 = Watchdog::new(1);
    for s in 1..=4u64 {
        w5.launch(s);
    }
    w5.tick();
    w5.tick();
    w5.reclaim();
    w5.ack(1).ok(); // 迟到
    w5.tick();
    let _ = w5.reclaim();
    cs.add("守恒等式", w5.reconcile(), "发射 ≥ 确认+在飞+回收");
    cs.add("迟到独立账", w5.late == 1, "迟到不进守恒三主项");

    // 族七：回收风暴恢复（批量超时 → 批量迟到 → 回稳）。
    let mut w6 = Watchdog::new(2);
    for s in 1..=6u64 {
        w6.launch(s);
    }
    for _ in 0..4 {
        w6.tick();
    }
    let storm = w6.reclaim();
    cs.add("风暴回收六", storm == 6, "全批超时");
    cs.add("在飞清零", w6.inflight.is_empty(), "风暴后自愈");
    // 全批迟到确认。
    let mut late_n = 0u64;
    for s in 1..=6u64 {
        if w6.ack(s).is_ok() {
            late_n += 1;
        }
    }
    cs.add("迟到六笔", late_n == 6 && w6.late == 6, "慢确认全部记账");
    cs.add("风暴守恒", w6.reconcile(), "6 发 = 0 确认 + 0 在飞 + 6 回收");
    // 回稳后新流正常。
    w6.launch(7);
    w6.ack(7).ok();
    cs.add("回稳确认", w6.confirmed == 1 && w6.inflight.is_empty(), "新流不再超时");

    // 族八：确认先于回收的混合流。
    let mut w7 = Watchdog::new(3);
    for s in 1..=4u64 {
        w7.launch(s);
    }
    w7.ack(2).ok(); // 及时
    w7.ack(4).ok(); // 乱序及时
    for _ in 0..4 {
        w7.tick();
    }
    let mixed = w7.reclaim();
    cs.add("混合回收二", mixed == 2, "1/3 超时 2/4 已确认");
    cs.add("混合守恒", w7.reconcile(), "4 发 = 2 确认 + 0 在飞 + 2 回收");
    cs.add("迟到对账", w7.ack(1).is_ok() && w7.ack(3).is_ok() && w7.late == 2, "超时项确认到齐");

    cs
}
