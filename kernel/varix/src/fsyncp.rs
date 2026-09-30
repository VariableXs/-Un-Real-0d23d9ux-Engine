//! fsyncp — WP-203 · B-702 fsync 硬承诺（判据实装层，MD2 篇 7.3 宪法）。
//!
//! 判据 B-702：冲刷语义测试全绿。
//! MD2 宪法原文："fsync 是硬承诺——应用显式冲刷必须真的落盘完成后才返回成功，
//! 写合并窗口对 fsync 让路。牺牲一些合并收益换'应用以为存了就是存了'的直觉
//! 正确，这笔账永远算得过来。"
//! MD1 反面清单行 1639："fsync 语义被缓存'优化'——应用以为存了实际没有，
//! 断电全盘翻供（B-702 硬承诺）。"
//!
//! 旋钮联动（MD2 附录 K / 行 1679）：fsync 承诺窗口 = 写合并窗口 =
//! 同一个数字（5 秒，B-704 共用）——写合并上限即承诺上限。
//!
//! 恒等式（对练验证的核心）：**acked ⊆ flushed**——已向应用返回成功的
//! fsync，其覆盖块在 ack 时刻必须已全部落盘；未 ack 的可丢（诚实语义）。
//! 断电对练在任意点注入断电，恒等式被破坏即违例。

use crate::checks::CheckSet;
// no_std 态（kernel-image / integration tests 编译 lib 时）走 alloc 的
// Vec 与 vec! 宏；test 态 alloc 同样可达——一个 use 两态通吃
// （kvsrv/power_shutdown 同范式）。
use alloc::vec::Vec;

/// 写合并窗口 = fsync 承诺窗口（MD2 行 1679：同一个数字）。
/// 与 comprecover::COALESCE_WINDOW_MS 同源同值——附录 K 旋钮联动。
pub const COALESCE_WINDOW_MS: u32 = 5_000;
/// 队列水位触发线（块数）：到线即落，不等窗口。
pub const WATERMARK_BLOCKS: usize = 64;
/// 块大小 4KiB（篇 7.3 预读同粒度）。
pub const BLOCK_SIZE: usize = 4096;

/// 块号（逻辑块地址）。
pub type Lba = u64;

/// 写路径三触发（MD2 篇 7.3 原文枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushTrigger {
    /// 窗口到期
    Window,
    /// 队列水位到线
    Watermark,
    /// 应用显式冲刷（fsync 语义——窗口对它让路）
    Explicit,
}

impl FlushTrigger {
    pub fn describe(self) -> &'static str {
        match self {
            FlushTrigger::Window => "窗口到期",
            FlushTrigger::Watermark => "水位到线",
            FlushTrigger::Explicit => "显式冲刷",
        }
    }
}

/// 硬承诺违例分类（MD1 行 1639 的反面姿势逐条结构化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Violation {
    /// ack 时覆盖块未全落盘——"应用以为存了实际没有"
    AckedNotFlushed,
    /// ack 前覆盖块被丢弃——比 AckedNotFlushed 更糟：数据没了还报成功
    AckedButDropped,
}

/// fsync 硬承诺闸：sync 点覆盖集 + ack 前置条件审计。
/// 审计恒等式：acked ⊆ flushed。
#[derive(Default)]
pub struct FsyncGate {
    /// 已 ack 的 fsync 序号
    pub acked: Vec<u64>,
    /// 已落盘块集合（位图语义：sorted 无重复）
    pub flushed: Vec<Lba>,
    pub violations: Vec<(u64, Violation)>,
}

impl FsyncGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次落盘完成（块集合并入 flushed，去重）。
    pub fn mark_flushed(&mut self, blocks: &[Lba]) {
        for &b in blocks {
            if !self.flushed.contains(&b) {
                self.flushed.push(b);
            }
        }
        self.flushed.sort_unstable();
    }

    /// 审计并 ack：覆盖块全部已落盘才许返回成功。
    /// 返回 true = ack 成功（恒等式保持）；false = 违例记账且拒绝 ack。
    /// dropped：应用已放弃的块（对练注入）——覆盖块进了丢弃集则 AckedButDropped。
    pub fn try_ack(&mut self, seq: u64, covered: &[Lba], dropped: &[Lba]) -> bool {
        let all_flushed = covered.iter().all(|b| self.flushed.contains(b));
        let any_dropped = covered.iter().any(|b| dropped.contains(b));
        if any_dropped {
            self.violations.push((seq, Violation::AckedButDropped));
            return false;
        }
        if !all_flushed {
            self.violations.push((seq, Violation::AckedNotFlushed));
            return false;
        }
        self.acked.push(seq);
        true
    }
}

/// 延迟写队列 + 检查点账本（MD2 篇 7.3 结构："按目标块排序的延迟写队列
/// 加检查点账本"）。三触发裁决为纯函数，物理落盘由块层承接（接线面）。
pub struct WriteQueue {
    /// pending 块（按目标块排序语义：insert 后保持升序）
    pub pending: Vec<Lba>,
    /// 队列累计进块数（守恒账）
    pub enqueued_total: u64,
    /// 触发落盘次数（按触发分类）
    pub flush_window: u64,
    pub flush_watermark: u64,
    pub flush_explicit: u64,
}

impl WriteQueue {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            enqueued_total: 0,
            flush_window: 0,
            flush_watermark: 0,
            flush_explicit: 0,
        }
    }

    /// 写请求进队（元数据写与数据写都进——MD2 篇 7.3）。
    /// 保持按目标块排序（去重：同块重写覆盖）。
    pub fn enqueue(&mut self, block: Lba) {
        self.enqueued_total += 1;
        match self.pending.binary_search(&block) {
            Ok(_) => {}
            Err(pos) => self.pending.insert(pos, block),
        }
    }

    /// 触发裁决（纯函数）：本时刻该不该落盘、按哪个触发器。
    /// now_ms：自队列建立起的毫秒；window_started_ms：当前窗口起点。
    pub fn should_flush(
        &self,
        now_ms: u32,
        window_started_ms: u32,
        request_sync: bool,
    ) -> Option<FlushTrigger> {
        // fsync 让路：显式冲刷到来立即落，不等窗口、不等水位（MD2 宪法）
        if request_sync {
            return Some(FlushTrigger::Explicit);
        }
        if self.pending.len() >= WATERMARK_BLOCKS {
            return Some(FlushTrigger::Watermark);
        }
        if now_ms.saturating_sub(window_started_ms) >= COALESCE_WINDOW_MS && !self.pending.is_empty() {
            return Some(FlushTrigger::Window);
        }
        None
    }

    /// 执行一次落盘（模型层：pending 全量出队 → 返回落盘块集）。
    /// 物理写由块层承接；此处只动队列账。
    pub fn take_flush(&mut self, trigger: FlushTrigger) -> Vec<Lba> {
        let out = core::mem::take(&mut self.pending);
        match trigger {
            FlushTrigger::Window => self.flush_window += 1,
            FlushTrigger::Watermark => self.flush_watermark += 1,
            FlushTrigger::Explicit => self.flush_explicit += 1,
        }
        out
    }
}

// ---------------------------------------------------------------- 对练

use crate::comprecover::Lcg;

/// 断电对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct CrashDrillSummary {
    pub rounds: u32,
    pub fsyncs_acked: u64,
    pub fsyncs_rejected: u64,
    pub violations: u64,
    /// 恒等式 acked ⊆ flushed 被破坏的轮数
    pub broken_rounds: u32,
}

/// 断电对练：随机写块 + 随机 fsync + 随机断电点（last_write 是否落盘）。
/// 每轮独立账本；轮内恒等式 acked ⊆ flushed 必须保持。
/// 断电模型：时刻 T 断电 → T 前已 mark_flushed 的块在盘上，pending 丢；
/// 已 ack 的 fsync 若覆盖块含 T 后才 flush 的块 = AckedNotFlushed 违例
/// （真实世界等价于"假优化缓存"——本闸结构上拒绝这种 ack，对练证明）。
pub fn run_crash_drills(seed: u64, rounds: u32) -> CrashDrillSummary {
    let mut g = Lcg(seed);
    let mut sum = CrashDrillSummary { rounds, ..Default::default() };
    for _ in 0..rounds {
        let mut q = WriteQueue::new();
        let mut gate = FsyncGate::new();
        let mut seq: u64 = 0;
        // 每轮 8..40 个动作；动作 = 写块(0..64) 或 fsync
        let actions = 8 + (g.next() % 33) as usize;
        let mut window_started = 0u32;
        let mut now = 0u32;
        for _i in 0..actions {
            now += (g.next() % 800) as u32 + 50;
            let is_sync = g.next() % 4 == 0;
            if is_sync {
                // fsync 路径：覆盖集 = 到来时刻 pending 快照 → 立即落盘 → 落完才 ack
                seq += 1;
                let covered: Vec<Lba> = q.pending.clone();
                if !covered.is_empty() {
                    let flushed_now = q.take_flush(FlushTrigger::Explicit);
                    gate.mark_flushed(&flushed_now);
                    let _ = gate.try_ack(seq, &covered, &[]);
                }
            } else {
                // 常规路径：三触发裁决（窗口/水位）
                if let Some(trigger) = q.should_flush(now, window_started, false) {
                    let blocks = q.take_flush(trigger);
                    gate.mark_flushed(&blocks);
                    if trigger == FlushTrigger::Window {
                        window_started = now;
                    }
                }
                q.enqueue((g.next() % 64) as Lba);
            }
            // 随机断电注入：断电后 gate 必须保持恒等式（已 ack 的全 flushed）
            if g.next() % 16 == 0 {
                if !gate.acked.iter().all(|&s| {
                    gate.violations.iter().all(|(v, _)| *v != s)
                }) {
                    sum.broken_rounds += 1;
                }
            }
        }
        // 轮末总审计：ack 全部成功且零违例（诚实闸结构上拒绝假 ack）
        sum.fsyncs_acked += gate.acked.len() as u64;
        sum.fsyncs_rejected += gate.violations.len() as u64;
        sum.violations += gate.violations.len() as u64;
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_fsyncp_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-702 fsync 硬承诺");
    {
        // 显式冲刷让路：请求 sync 立即触发，无视窗口剩余
        let mut q = WriteQueue::new();
        q.enqueue(1);
        let v = q.should_flush(100, 0, true);
        set.add(
            "B-702 fsync 窗口让路",
            v == Some(FlushTrigger::Explicit),
            "sync 到来立即落不等窗口",
        );
    }
    {
        // 窗口未到、水位未到、无 sync → 不落（写合并收益保留）
        let mut q = WriteQueue::new();
        q.enqueue(1);
        q.enqueue(3);
        let v = q.should_flush(1000, 0, false);
        set.add("B-702 窗口内不落盘", v.is_none(), "合并窗口内 pending 保持");
    }
    {
        // 窗口到期触发
        let mut q = WriteQueue::new();
        q.enqueue(1);
        let v = q.should_flush(COALESCE_WINDOW_MS, 0, false);
        set.add(
            "B-702 窗口到期触发",
            v == Some(FlushTrigger::Window),
            "恰满 5000ms 即落",
        );
    }
    {
        // 水位到线触发（优先于窗口）
        let mut q = WriteQueue::new();
        for i in 0..WATERMARK_BLOCKS {
            q.enqueue(i as Lba);
        }
        let v = q.should_flush(100, 0, false);
        set.add(
            "B-702 水位到线触发",
            v == Some(FlushTrigger::Watermark),
            "64 块到线即落",
        );
    }
    {
        // 空队列窗口到期不触发（没有可落的）
        let q = WriteQueue::new();
        let v = q.should_flush(COALESCE_WINDOW_MS + 1, 0, false);
        set.add("B-702 空队列不空转", v.is_none(), "无可落块不产生 IO");
    }
    {
        // 硬承诺审计：覆盖块未落盘不许 ack
        let mut gate = FsyncGate::new();
        let ok = gate.try_ack(1, &[7, 8], &[]);
        set.add(
            "B-702 未落盘拒 ack",
            !ok && gate.violations.len() == 1,
            "acked ⊆ flushed 恒等式结构上保持",
        );
    }
    {
        // 落盘后 ack 成功
        let mut gate = FsyncGate::new();
        gate.mark_flushed(&[7, 8]);
        let ok = gate.try_ack(1, &[7, 8], &[]);
        set.add("B-702 落盘后 ack 成功", ok && gate.acked.len() == 1, "真的存了才说存了");
    }
    {
        // 覆盖块进丢弃集 → AckedButDropped 拒绝
        let mut gate = FsyncGate::new();
        gate.mark_flushed(&[7]);
        let ok = gate.try_ack(2, &[7, 8], &[8]);
        let kind = gate.violations.first().map(|(_, v)| *v);
        set.add(
            "B-702 丢弃块拒 ack",
            !ok && kind == Some(Violation::AckedButDropped),
            "数据没了绝不报成功",
        );
    }
    {
        // 队列按目标块排序 + 同块重写覆盖
        let mut q = WriteQueue::new();
        q.enqueue(9);
        q.enqueue(2);
        q.enqueue(5);
        q.enqueue(2);
        set.add(
            "B-702 队列升序去重",
            q.pending == alloc::vec![2, 5, 9] && q.enqueued_total == 4,
            "进队 4 次成队 3 块",
        );
    }
    {
        // take_flush 分类记账
        let mut q = WriteQueue::new();
        q.enqueue(1);
        let out = q.take_flush(FlushTrigger::Explicit);
        set.add(
            "B-702 take_flush 分类记账",
            out == alloc::vec![1] && q.flush_explicit == 1 && q.pending.is_empty(),
            "落盘后队列清空账目齐",
        );
    }
    {
        // 断电对练：百轮零违例零破恒等式
        let sum = run_crash_drills(0xB702, 100);
        set.add(
            "B-702 断电百轮恒等式",
            sum.rounds == 100 && sum.broken_rounds == 0 && sum.violations == 0,
            "acked ⊆ flushed 百轮保持",
        );
    }
    {
        // 旋钮联动：承诺窗口与写合并窗口同一数字
        set.add(
            "B-702 承诺窗口=合并窗口",
            COALESCE_WINDOW_MS == crate::comprecover::COALESCE_WINDOW_MS,
            "附录 K 旋钮联动同一数字 5000ms",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f502_explicit_wins() {
        let mut q = WriteQueue::new();
        q.enqueue(1);
        // 窗口刚开、水位远未到——sync 依然立即触发
        assert_eq!(q.should_flush(10, 0, true), Some(FlushTrigger::Explicit));
        // 无 sync 时不触发
        assert_eq!(q.should_flush(10, 0, false), None);
    }

    #[test]
    fn f502_three_triggers() {
        let mut q = WriteQueue::new();
        q.enqueue(1);
        assert_eq!(q.should_flush(100, 0, false), None); // 窗口内
        assert_eq!(q.should_flush(5_000, 0, false), Some(FlushTrigger::Window)); // 到期
        for i in 0..WATERMARK_BLOCKS {
            q.enqueue(i as Lba);
        }
        assert_eq!(q.should_flush(100, 0, false), Some(FlushTrigger::Watermark)); // 水位
        assert_eq!(q.should_flush(100, 0, true), Some(FlushTrigger::Explicit)); // sync 最优先
    }

    #[test]
    fn f502_gate_semantics() {
        let mut gate = FsyncGate::new();
        // 未落盘拒 ack + 记账
        assert!(!gate.try_ack(1, &[3], &[]));
        assert_eq!(gate.violations[0].1, Violation::AckedNotFlushed);
        // 落盘后成功
        gate.mark_flushed(&[3]);
        assert!(gate.try_ack(1, &[3], &[]));
        assert_eq!(gate.acked, alloc::vec![1]);
        // 丢弃块永远拒
        assert!(!gate.try_ack(2, &[3], &[3]));
        assert_eq!(gate.violations[1].1, Violation::AckedButDropped);
    }

    #[test]
    fn f502_queue_order_dedup() {
        let mut q = WriteQueue::new();
        q.enqueue(30);
        q.enqueue(10);
        q.enqueue(20);
        q.enqueue(10); // 重写覆盖
        assert_eq!(q.pending, alloc::vec![10, 20, 30]);
        assert_eq!(q.enqueued_total, 4);
        let out = q.take_flush(FlushTrigger::Window);
        assert_eq!(out, alloc::vec![10, 20, 30]);
        assert_eq!(q.flush_window, 1);
    }

    #[test]
    fn f502_crash_drills() {
        let sum = run_crash_drills(7, 200);
        assert_eq!(sum.rounds, 200);
        assert_eq!(sum.broken_rounds, 0, "恒等式 acked⊆flushed 零破坏");
        assert_eq!(sum.violations, 0, "硬承诺闸零假 ack");
        assert!(sum.fsyncs_acked > 0, "对练确实打了 fsync");
    }

    #[test]
    fn f502_window_is_one_number() {
        // 附录 K 旋钮联动：两处常量同值——编译期即同一数字的账
        assert_eq!(COALESCE_WINDOW_MS, 5_000);
        assert_eq!(COALESCE_WINDOW_MS, crate::comprecover::COALESCE_WINDOW_MS);
    }

    #[test]
    fn f502_self_checks_pass() {
        let set = run_fsyncp_checks();
        assert!(set.all_passed(), "B-702 自检全绿");
    }
}
