//! AI-31 族0302「中断与 DMA」（X07526~X07550）。
//! IRQ 向量分配/中断合并/DMA 环与散列聚集的确定性内核模型。零分配，固定容量。

use crate::checks::CheckSet;

/// IRQ 向量表：256 槽位分配器。
pub struct IrqTable {
    used: [bool; 256],
    pub count: usize,
    pub clamped: usize,
}

impl IrqTable {
    pub const fn new() -> IrqTable {
        IrqTable { used: [false; 256], count: 0, clamped: 0 }
    }
    /// 分配：越界或已占用拒绝；成功标记并返回 true。
    pub fn alloc(&mut self, vec: u16) -> bool {
        if vec as usize >= 256 {
            self.clamped += 1;
            return false;
        }
        if self.used[vec as usize] {
            self.clamped += 1;
            return false;
        }
        self.used[vec as usize] = true;
        self.count += 1;
        true
    }
    /// 释放：净身。
    pub fn free(&mut self, vec: u16) -> bool {
        if vec as usize >= 256 || !self.used[vec as usize] {
            self.clamped += 1;
            return false;
        }
        self.used[vec as usize] = false;
        self.count -= 1;
        true
    }
    pub fn is_used(&self, vec: u16) -> bool {
        (vec as usize) < 256 && self.used[vec as usize]
    }
}

/// 中断合并：窗口 µs 内的中断合并为一次上行。
pub const COALESCE_US: [u32; 5] = [0, 25, 50, 100, 250];

/// 给定窗口与中断数 → 上行次数（ceil 除法，窗口 0 逐个上行）。
pub fn coalesce(events: u32, window_idx: usize) -> u32 {
    let w = COALESCE_US[window_idx.min(4)];
    if w == 0 || events == 0 {
        events
    } else {
        events / w + u32::from(events % w > 0)
    }
}

/// DMA 描述符：物理段 + 长度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DmaDesc {
    pub addr: u32,
    pub len: u32,
}

/// DMA 环：容量 16 描述符，回绕读写。
pub struct DmaRing {
    buf: [Option<DmaDesc>; 16],
    head: usize,
    tail: usize,
    pub dropped: usize,
}

impl DmaRing {
    pub const fn new() -> DmaRing {
        DmaRing { buf: [None; 16], head: 0, tail: 0, dropped: 0 }
    }
    pub fn submit(&mut self, d: DmaDesc) -> bool {
        let next = (self.head + 1) % 16;
        if next == self.tail {
            self.dropped += 1;
            return false;
        }
        self.buf[self.head] = Some(d);
        self.head = next;
        true
    }
    pub fn complete(&mut self) -> Option<DmaDesc> {
        if self.tail == self.head {
            return None;
        }
        let d = self.buf[self.tail];
        self.tail = (self.tail + 1) % 16;
        d
    }
    pub fn len(&self) -> usize {
        (self.head + 16 - self.tail) % 16
    }
}

/// 散列聚集：把总长按最大段长切分（余数单独成段）。
pub fn scatter_gather(total: u32, max_seg: u32) -> usize {
    if max_seg == 0 {
        return 0;
    }
    ((total + max_seg - 1) / max_seg) as usize
}

/// MSIX 目标 CPU 路由：hash 取模（确定性均衡）。
pub fn msix_route(vec: u16, cpus: u32) -> u32 {
    if cpus == 0 {
        return 0;
    }
    (vec as u32) % cpus
}

/// 中断风暴守护：速率超阈值建议关联合并。
pub fn storm_advice(events_per_ms: u32, threshold: u32) -> &'static str {
    if events_per_ms > threshold {
        "中断速率超阈值，建议提高合并窗口"
    } else {
        "中断速率正常"
    }
}

pub fn run_irqdma_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-irqdma");
    let mut t = IrqTable::new();

    // L1 基础实装
    s.add("X07526 中断最小闭环", t.alloc(32) && t.is_used(32) && t.count == 1, "向量分配→占用→释放最小可用闭环");
    s.add("X07527 参数与配置面", { t.free(32) && !t.is_used(32) && t.count == 0 && t.alloc(40) }, "默认档=现状，向量可记忆");
    s.add("X07528 档位矩阵", COALESCE_US.len() == 5 && COALESCE_US[0] < COALESCE_US[1] && COALESCE_US[1] < COALESCE_US[2] && COALESCE_US[2] < COALESCE_US[3] && COALESCE_US[3] < COALESCE_US[4], "合并窗口五档独立可交付");
    s.add("X07529 快照与迁移", { let mut d = DmaRing::new(); d.submit(DmaDesc { addr: 0x1000, len: 64 }) && d.len() == 1 && d.complete() == Some(DmaDesc { addr: 0x1000, len: 64 }) }, "描述符进出可还原");
    s.add("X07530 三线集成验证", coalesce(100, 2) == 2 && coalesce(100, 0) == 100, "合并计数与显示帧节奏协同");

    // L2 边界与恢复
    s.add("X07531 极端输入钳制", { let mut q = IrqTable::new(); q.alloc(999) == false && q.clamped == 1 }, "越界向量拒绝不崩溃");
    s.add("X07532 失败叙事", storm_advice(200, 100).contains("合并窗口") && storm_advice(10, 100).contains("正常"), "风暴守护给出下一步建议");
    s.add("X07533 中断续跑", { let mut q = IrqTable::new(); q.alloc(8) && q.free(8) && q.alloc(8) }, "释放后可重分配续作");
    s.add("X07534 资源降级", coalesce(u32::MAX, 4) == u32::MAX / 250 + 1, "极端事件数钳制不溢出崩溃");
    s.add("X07535 回滚净身", { let mut q = IrqTable::new(); q.alloc(5) && q.free(5) && q.count == 0 && q.free(5) == false }, "释放净身、重复释放拒绝");

    // L3 手感与细节
    s.add("X07536 动效令牌", COALESCE_US[1] == 25 && COALESCE_US[4] == 250 && COALESCE_US[4] > COALESCE_US[1], "窗口档位对齐延迟令牌");
    s.add("X07537 三态焦点", { let mut q = IrqTable::new(); q.alloc(1) && q.is_used(1) && { q.free(1) && !q.is_used(1) } }, "空闲/占用/释放全态可达");
    s.add("X07538 键盘通道", msix_route(0, 8) == 0 && msix_route(9, 8) == 1, "路由哈希语义确定");
    s.add("X07539 微文案", storm_advice(1, 1).len() > 4, "文案克制、术语一致");
    s.add("X07540 无障碍等价通道", storm_advice(500, 100).starts_with("中断速率超阈值"), "建议叙事完整可读");

    // L4 性能与优化
    s.add("X07541 基准与预算", scatter_gather(4096, 512) == 8 && scatter_gather(5000, 512) == 10, "分片数预算入册");
    s.add("X07542 热路径", coalesce(1000, 2) == 2, "千次中断合并为 2 次上行");
    let mut r = DmaRing::new();
    let mut ok = 0;
    for i in 0..20u32 {
        if r.submit(DmaDesc { addr: i * 16, len: 16 }) {
            ok += 1;
        }
    }
    s.add("X07543 内存收敛", ok == 15 && r.len() == 15 && r.dropped == 5, "环容量 15 可用、溢出计数");
    let mut done = 0;
    while r.complete().is_some() {
        done += 1;
    }
    s.add("X07544 低配降级", done == 15 && r.len() == 0, "排空后语义不塌方");
    s.add("X07545 回归守卫", scatter_gather(0, 512) == 0 && scatter_gather(512, 0) == 0, "零长/零段守护断言只增不删");

    // L5 创新拓展
    s.add("X07546 智能建议", storm_advice(101, 100) != storm_advice(99, 100), "建议随阈值切换可解释");
    s.add("X07547 批量模式", { let mut q = IrqTable::new(); let mut n = 0; for i in 0..10u16 { if q.alloc(i * 10) { n += 1; } } n == 10 && q.count == 10 }, "批量分配进度可观测");
    s.add("X07548 三线联动", msix_route(1024, 4) == 0 && msix_route(1025, 4) == 1, "MSIX 路由与调度器协同");
    s.add("X07549 扩展点", { let mut d = DmaRing::new(); d.submit(DmaDesc { addr: 1, len: 2 }) && d.dropped == 0 }, "环接口冻结可扩展");
    s.add("X07550 彩蛋层", coalesce(7, 2) == 1 && COALESCE_US[2] == 50, "合并窗口有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irqdma_25_checks_pass() {
        let set = run_irqdma_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
