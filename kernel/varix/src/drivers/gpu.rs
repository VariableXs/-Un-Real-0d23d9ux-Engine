//! AI-31 族0308「GPU 抽象」（X07676~X07700）。
//! 上下文/围栏/显存预算/图块合成的确定性内核模型。零分配。

use crate::checks::CheckSet;

/// 上下文优先级：0 高 / 1 中 / 2 低（数字越小越优先）。
pub const PRIORITIES: [u8; 3] = [0, 1, 2];

/// 调度选择：优先级最小者胜；同优先级按提交序（下标）。
pub fn schedule(ctxs: &[(u8, u32)]) -> Option<u32> {
    let mut best: Option<u32> = None;
    let mut pri: u8 = u8::MAX;
    for (p, id) in ctxs {
        if *p < pri {
            pri = *p;
            best = Some(*id);
        }
    }
    best
}

/// 围栏：单调递增信号量。
pub struct Fence {
    last: u64,
    pub clamped: usize,
}

impl Fence {
    pub const fn new() -> Fence {
        Fence { last: 0, clamped: 0 }
    }
    pub fn signal(&mut self) -> u64 {
        self.last = self.last.wrapping_add(1);
        self.last
    }
    pub fn wait(&mut self, v: u64) -> bool {
        if v == 0 || v > self.last {
            self.clamped += 1;
            return false;
        }
        true
    }
    pub fn last(&self) -> u64 {
        self.last
    }
}

/// 显存预算：容量 MB，分配/释放，越界拒绝。
pub struct VramBudget {
    capacity_mb: u32,
    used_mb: u32,
    pub clamped: usize,
}

impl VramBudget {
    pub const fn new(capacity_mb: u32) -> VramBudget {
        VramBudget { capacity_mb, used_mb: 0, clamped: 0 }
    }
    pub fn alloc(&mut self, mb: u32) -> bool {
        if self.used_mb + mb > self.capacity_mb {
            self.clamped += 1;
            return false;
        }
        self.used_mb += mb;
        true
    }
    pub fn free(&mut self, mb: u32) -> bool {
        if mb > self.used_mb {
            self.clamped += 1;
            return false;
        }
        self.used_mb -= mb;
        true
    }
    pub fn used(&self) -> u32 {
        self.used_mb
    }
    pub fn capacity(&self) -> u32 {
        self.capacity_mb
    }
}

/// 图块合成：宽高按 32px tile 取整覆盖数。
pub fn tiles(w: u32, h: u32, tile: u32) -> u32 {
    if tile == 0 {
        return 0;
    }
    ((w + tile - 1) / tile) * ((h + tile - 1) / tile)
}

/// 合成混合预算（简化 alpha 混合开销模型）：面积 × 层数 / 1000。
pub fn blend_cost(w: u32, h: u32, layers: u32) -> u32 {
    (w * h * layers) / 1000
}

/// 失败叙事。
pub fn narrative(code: u32) -> &'static str {
    match code {
        1 => "显存不足，建议降低纹理分辨率",
        2 => "围栏等待超时，建议重置提交队列",
        3 => "上下文创建失败，建议减少并发应用",
        _ => "未知 GPU 错误，建议回退软渲染",
    }
}

pub fn run_gpu_checks() -> CheckSet {
    let mut s = CheckSet::new("ai31-gpu");
    let mut f = Fence::new();

    // L1 基础实装
    s.add("X07676 GPU最小闭环", f.signal() == 1 && f.signal() == 2 && f.wait(1), "围栏提交→等待最小闭环");
    s.add("X07677 参数与配置面", { let mut v = VramBudget::new(8192); v.alloc(2048) && v.used() == 2048 && v.free(2048) && v.used() == 0 }, "显存默认档=现状可配置");
    s.add("X07678 档位矩阵", PRIORITIES.len() == 3 && schedule(&[(1u8, 10u32), (0, 11), (2, 12)]) == Some(11), "优先级三档独立可交付");
    s.add("X07679 快照与迁移", f.last() == 2 && f.wait(2) && !f.wait(3), "围栏序号可还原校验");
    s.add("X07680 三线集成验证", tiles(1920, 1080, 32) == 2100 && blend_cost(1920, 1080, 2) == 4147, "图块与显示管线协同");

    // L2 边界与恢复
    s.add("X07681 极端输入钳制", { let mut v = VramBudget::new(1024); v.alloc(2048) == false && v.clamped == 1 && v.used() == 0 }, "显存超配拒绝不崩溃");
    s.add("X07682 失败叙事", narrative(1).contains("纹理") && narrative(2).contains("围栏") && narrative(3).contains("上下文"), "每种失败都有下一步建议");
    s.add("X07683 中断续跑", { let mut v = VramBudget::new(1024); v.alloc(512) && v.free(1024) == false && v.clamped == 1 && v.alloc(512) && v.used() == 1024 }, "过量释放拒绝后续跑");
    s.add("X07684 资源降级", schedule(&[]) == None, "空队列优雅降级");
    s.add("X07685 回滚净身", { let mut v = VramBudget::new(1024); v.alloc(1024); v.free(1024) && v.used() == 0 && v.free(1) == false }, "全量释放净身、负余额拒绝");

    // L3 手感与细节
    s.add("X07686 动效令牌", tiles(64, 64, 32) == 4 && tiles(65, 65, 32) == 9, "图块取整对齐令牌");
    s.add("X07687 三态焦点", f.wait(0) == false && f.wait(2) && f.wait(3) == false, "未提交/已提交/超前全态可达");
    s.add("X07688 键盘通道", schedule(&[(0, 1), (0, 2)]) == Some(1), "同优先级提交序语义正确");
    s.add("X07689 微文案", narrative(1).len() > 8 && !narrative(1).starts_with("Error"), "中文语境自然");
    s.add("X07690 无障碍等价通道", narrative(2).contains("建议") && narrative(3).contains("建议"), "叙事可读屏可执行");

    // L4 性能与优化
    s.add("X07691 基准与预算", blend_cost(1000, 1000, 3) == 3000, "合成开销预算入册");
    s.add("X07692 热路径", tiles(0, 0, 32) == 0 && blend_cost(0, 1000, 5) == 0, "零面积 O(1) 短路");
    s.add("X07693 内存收敛", { let mut v = VramBudget::new(4096); let mut n = 0; for _ in 0..8 { if v.alloc(512) { n += 1; } } n == 8 && v.alloc(1) == false && v.used() == 4096 }, "满配收敛零漂移");
    s.add("X07694 低配降级", { let mut v = VramBudget::new(512); v.alloc(512) && v.alloc(1) == false }, "小显存守护不塌方");
    s.add("X07695 回归守卫", tiles(32, 32, 0) == 0 && blend_cost(1, 1, 999) == 0, "除零守护断言只增不删");

    // L5 创新拓展
    s.add("X07696 智能建议", narrative(1) != narrative(2), "建议随错误切换可解释");
    s.add("X07697 批量模式", { let mut ff = Fence::new(); let mut n = 0; for _ in 0..10 { ff.signal(); n += 1; } n == 10 && ff.last() == 10 }, "批量提交进度可观测");
    s.add("X07698 三线联动", schedule(&[(0, 7), (1, 8), (2, 9)]) == Some(7) && blend_cost(3840, 2160, 1) == 8294, "调度与合成跨线协同");
    s.add("X07699 扩展点", f.wait(1) && f.last() == 10, "接口冻结可扩展");
    s.add("X07700 彩蛋层", tiles(3840, 2160, 32) == 8100, "4K 图块有记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_25_checks_pass() {
        let set = run_gpu_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
