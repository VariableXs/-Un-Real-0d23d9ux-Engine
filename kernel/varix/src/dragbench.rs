//! UNREAL-X-15000 · WP-201 · B-502 拖动帧率基准 harness（MD2 篇 5.2 × 判据表）。
//!
//! 定案（MD2 行 329）：一帧的生命周期分四步——收集、合成、后处理、提交。
//! **帧计时打点在第二（合成）与第四（提交）步各一枚**，vxbench 的拖动帧率
//! 判据（55fps）读的就是这两点的间隔分布。
//! 判据（MD2 行 358）：B-502 拖动帧率——**1080p ≥ 55fps，掉帧时输入不迟滞**。
//! 缓冲纪律联动：拖动场景走三缓冲（B-504 动画档），脏区拷贝严格限制在脏区
//! 并集内（MD2 行 329——软渲染下内存带宽是唯一稀缺资源）。
//! 零堆、整数运算、宿主全测。判据号 B-502 入 CheckSet 命名。

// ---------------------------------------------------------------------------
// 预算常量
// ---------------------------------------------------------------------------

/// 55fps 每帧预算（微秒）：1_000_000 / 55 = 18181。
pub const FRAME_BUDGET_US: u32 = 18_181;
/// 1080p ARGB 帧字节数：1920×1080×4。
pub const FRAME_1080P_BYTES: u32 = 1920 * 1080 * 4;
/// 拖动基准帧数（vxbench 采样窗口）。
pub const DRAG_FRAMES: usize = 600;
/// 掉帧判定：帧间隔 > 预算即记掉帧。
/// 输入不迟滞判定：输入事件从入队到消费 ≤ 1 帧。

/// 帧计时打点（第二/第四步各一枚，MD2 行 329）。
#[derive(Clone, Copy, Debug)]
pub struct FrameMark {
    /// 合成步打点（相对起点，微秒）。
    pub compose_tsc: u32,
    /// 提交步打点（相对起点，微秒）。
    pub present_tsc: u32,
}

/// 拖动场景帧序列模型：每帧给模拟合成耗时（微秒），harness 据此算分布。
#[derive(Clone, Copy, Debug)]
pub struct DragBench {
    marks: [Option<FrameMark>; DRAG_FRAMES],
    n: usize,
    /// 每帧输入事件消费延迟（帧数）——掉帧时输入不迟滞的账本。
    pub input_lag_frames: [u8; DRAG_FRAMES],
    pub cursor_lag_frames: [u8; DRAG_FRAMES],
    // —— 账本 ——
    pub total_frames: usize,
    pub dropped_frames: u64,
    pub max_lag: u8,
    pub sum_interval_us: u64,
}

impl DragBench {
    pub fn new() -> DragBench {
        DragBench {
            marks: [None; DRAG_FRAMES],
            n: 0,
            input_lag_frames: [0; DRAG_FRAMES],
            cursor_lag_frames: [0; DRAG_FRAMES],
            total_frames: 0,
            dropped_frames: 0,
            max_lag: 0,
            sum_interval_us: 0,
        }
    }

    /// 登记一帧：传入本帧合成耗时（us）与提交步耗时（us）。
    /// 返回帧间隔（0 = 首帧）。
    pub fn record(&mut self, compose_us: u32, present_us: u32) -> u32 {
        if self.n == 0 {
            self.marks[0] = Some(FrameMark { compose_tsc: compose_us, present_tsc: compose_us + present_us });
            self.n = 1;
            self.total_frames = 1;
            return 0;
        }
        let prev = self.marks[self.n - 1].unwrap_or(FrameMark { compose_tsc: 0, present_tsc: 0 });
        let start = prev.present_tsc;
        let mark = FrameMark {
            compose_tsc: start + compose_us,
            present_tsc: start + compose_us + present_us,
        };
        let interval = mark.present_tsc.saturating_sub(prev.present_tsc);
        self.sum_interval_us += interval as u64;
        if interval > FRAME_BUDGET_US {
            self.dropped_frames += 1;
        }
        self.marks[self.n] = Some(mark);
        self.n += 1;
        self.total_frames = self.n;
        interval
    }

    /// 登记输入事件消费延迟（帧数）——拖动中输入跟着帧走，掉帧不积压。
    pub fn record_input_lag(&mut self, frames: u8) {
        if self.n > 0 && self.n <= DRAG_FRAMES {
            self.input_lag_frames[self.n - 1] = frames;
            if frames > self.max_lag {
                self.max_lag = frames;
            }
        }
    }

    /// 帧间隔 p95（宿主整数近似：排序取第 95 百分位槽）。
    /// 零堆：复制到定长数组插入排序（DRAG_FRAMES=600 可承受）。
    pub fn p95_interval_us(&self) -> u32 {
        self.percentile_interval_us(95)
    }

    pub fn percentile_interval_us(&self, pct: u32) -> u32 {
        if self.n < 2 {
            return 0;
        }
        let mut tmp = [0u32; DRAG_FRAMES];
        let mut m = 0usize;
        for i in 1..self.n {
            if let (Some(a), Some(b)) = (self.marks[i - 1], self.marks[i]) {
                tmp[m] = b.present_tsc.saturating_sub(a.present_tsc);
                m += 1;
            }
        }
        // 插入排序（定长、零堆）
        for i in 1..m {
            let key = tmp[i];
            let mut j = i;
            while j > 0 && tmp[j - 1] > key {
                tmp[j] = tmp[j - 1];
                j -= 1;
            }
            tmp[j] = key;
        }
        let idx = ((m as u64) * (pct as u64 - 1) / 100) as usize;
        tmp[idx.min(m - 1)]
    }

    /// 平均帧率（fps × 100，整数）：1e8 / 平均间隔 us。
    pub fn fps_x100(&self) -> u32 {
        if self.n < 2 || self.sum_interval_us == 0 {
            return 0;
        }
        let avg_us = self.sum_interval_us / (self.n - 1) as u64;
        (100_000_000 / avg_us) as u32
    }

    /// 判据 B-502 第一条：p95 间隔 ≤ 55fps 预算。
    pub fn meets_55fps(&self) -> bool {
        self.n >= DRAG_FRAMES / 2 && self.p95_interval_us() <= FRAME_BUDGET_US
    }

    /// 判据 B-502 第二条：掉帧时输入不迟滞（最大消费延迟 ≤ 1 帧）。
    pub fn input_no_stall(&self) -> bool {
        self.max_lag <= 1
    }
}

// ---------------------------------------------------------------------------
// 标准拖动负载模型（1080p 脏区带宽下界）
// ---------------------------------------------------------------------------

/// 拖动帧的合成耗时模型（微秒）：脏区并集字节 / 有效带宽 + 固定开销。
/// 有效带宽以 MB/s 整数表达（软渲染纯内存搬运，MD2 行 329）。
pub fn drag_frame_cost_us(bandwidth_mbps: u32, overhead_us: u32) -> u32 {
    let bytes = FRAME_1080P_BYTES as u64;
    let bw = bandwidth_mbps as u64 * 1_000_000 / 1_000_000; // MB/s → B/us
    let copy_us = if bw > 0 { bytes / bw.max(1) } else { u32::MAX as u64 };
    (copy_us as u32).saturating_add(overhead_us)
}

/// 帧生命周期四步预算分解（收集/合成/后处理/提交，微秒）。
pub const BUDGET_COLLECT_US: u32 = 1_500;
pub const BUDGET_COMPOSE_US: u32 = 10_500;
pub const BUDGET_POST_US: u32 = 2_000;
pub const BUDGET_PRESENT_US: u32 = 4_000;
/// 四步合计 ≤ 帧预算。
pub fn budget_total_us() -> u32 {
    BUDGET_COLLECT_US + BUDGET_COMPOSE_US + BUDGET_POST_US + BUDGET_PRESENT_US
}

// ---------------------------------------------------------------------------
// 自检（判据号 B-502 入命名）
// ---------------------------------------------------------------------------

pub fn run_dragbench_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("vxwm-dragbench");

    // —— 标准拖动序列（模拟 16.5ms/帧：p95 ≈ 16500 ≤ 18181） ——
    let mut b = DragBench::new();
    for _ in 0..DRAG_FRAMES {
        let _ = b.record(13_000, 3_500); // 合成+提交 = 16500us
    }
    set.add(
        "B-502 标准拖动 p95 达 55fps",
        b.meets_55fps() && b.p95_interval_us() <= FRAME_BUDGET_US,
        "600 帧采样 p95 ≤ 18181us",
    );
    set.add(
        "B-502 帧率折算 ≥ 5500（55fps×100）",
        b.fps_x100() >= 5_500,
        "整数折算无浮点",
    );
    set.add(
        "B-502 采样窗口 600 帧齐",
        b.total_frames == DRAG_FRAMES,
        "vxbench 采样窗口完整",
    );

    // —— 打点位置：合成步与提交步各一枚（MD2 行 329） ——
    let m0 = b.percentile_interval_us(1);
    set.add(
        "B-502 双打点间隔分布可算",
        m0 > 0,
        "间隔 = 本次 present − 上次 present",
    );

    // —— 掉帧场景：单帧超标被记 ——
    let mut d = DragBench::new();
    for i in 0..100 {
        let _ = if i == 50 { d.record(25_000, 5_000) } else { d.record(13_000, 3_500) };
    }
    set.add(
        "B-502 掉帧计数如实",
        d.dropped_frames == 1,
        "第 50 帧超标被记 1 次",
    );

    // —— 输入不迟滞 ——
    let mut s = DragBench::new();
    for i in 0..DRAG_FRAMES {
        let _ = if i == 30 { s.record(25_000, 5_000) } else { s.record(13_000, 3_500) };
        s.record_input_lag(if i == 30 { 1 } else { 0 }); // 掉帧当帧事件下帧消化
    }
    set.add(
        "B-502 掉帧时输入不迟滞",
        s.input_no_stall() && s.max_lag <= 1,
        "输入消费延迟 ≤ 1 帧（B-502 达标线）",
    );
    let mut stall = DragBench::new();
    for _ in 0..100 {
        let _ = stall.record(13_000, 3_500);
    }
    stall.record_input_lag(5);
    set.add(
        "B-502 迟滞被账本抓获",
        !stall.input_no_stall() && stall.max_lag == 5,
        "积压 5 帧即判负——缺陷形态可现形",
    );

    // —— 带宽模型下界 ——
    let c1 = drag_frame_cost_us(600, 2_000); // 600MB/s：8.3MB 拷贝 ≈ 13890us + 2000
    set.add(
        "B-502 1080p 拷贝带宽下界",
        c1 > 10_000 && c1 < FRAME_BUDGET_US,
        "全屏脏区在 600MB/s 下入预算",
    );
    let c2 = drag_frame_cost_us(300, 2_000); // 300MB/s：≈27778us 超预算
    set.add(
        "B-502 带宽不足如实超预算",
        c2 > FRAME_BUDGET_US,
        "27.8ms > 18.2ms——硬件下界可解释",
    );

    // —— 四步预算分解 ——
    set.add(
        "B-502 四步预算合计 ≤ 帧预算",
        budget_total_us() <= FRAME_BUDGET_US,
        "收集+合成+后处理+提交 = 18000 ≤ 18181",
    );
    set.add(
        "B-502 合成步是最大份额",
        BUDGET_COMPOSE_US > BUDGET_COLLECT_US
            && BUDGET_COMPOSE_US > BUDGET_POST_US
            && BUDGET_COMPOSE_US > BUDGET_PRESENT_US,
        "带宽稀缺资源的预算倾斜",
    );

    // —— p95 排序正确性（已知序列） ——
    let mut k = DragBench::new();
    // 9 帧正常 16500 + 1 帧 30000（首帧无间隔，9 个间隔：8×16500 + 1×30000）
    for i in 0..10 {
        let _ = if i == 5 { k.record(25_000, 5_000) } else { k.record(13_000, 3_500) };
    }
    set.add(
        "B-502 百分位槽取值正确",
        k.percentile_interval_us(50) == 16_500,
        "p50 落在正常帧",
    );

    // —— 首帧零间隔不计 ——
    let mut f = DragBench::new();
    let first = f.record(13_000, 3_500);
    set.add(
        "B-502 首帧无间隔基准",
        first == 0 && f.sum_interval_us == 0,
        "间隔分布从第二帧起算",
    );

    // —— 净身 ——
    let mut z = DragBench::new();
    let _ = z.record(13_000, 3_500);
    z = DragBench::new();
    set.add(
        "B-502 重置净身",
        z.total_frames == 0 && z.dropped_frames == 0 && z.max_lag == 0,
        "账本归零",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragbench_steady_55fps() {
        let mut b = DragBench::new();
        for _ in 0..DRAG_FRAMES {
            let _ = b.record(13_000, 3_500);
        }
        assert!(b.meets_55fps());
        assert_eq!(b.dropped_frames, 0);
        assert!(b.fps_x100() >= 5_500, "fps_x100={}", b.fps_x100());
    }

    #[test]
    fn dragbench_drops_counted() {
        let mut b = DragBench::new();
        for i in 0..100 {
            let cost = if i % 10 == 9 { 20_000 } else { 16_000 };
            let _ = b.record(cost - 3_500, 3_500);
        }
        // 20_000 间隔 10 次全部超 18181
        assert_eq!(b.dropped_frames, 10);
    }

    #[test]
    fn dragbench_percentile_order() {
        let mut b = DragBench::new();
        // 间隔序列：9×16000 + 1×25000
        for i in 0..11 {
            let _ = if i == 10 { b.record(21_500, 3_500) } else { b.record(12_500, 3_500) };
        }
        assert_eq!(b.percentile_interval_us(50), 16_000);
        assert_eq!(b.percentile_interval_us(90), 16_000); // 90% 恰落在正常帧上界
        assert_eq!(b.percentile_interval_us(100), 25_000);
    }

    #[test]
    fn dragbench_input_no_stall_matrix() {
        let mut b = DragBench::new();
        for i in 0..50 {
            let _ = b.record(16_000, 200);
            b.record_input_lag(if i % 7 == 3 { 1 } else { 0 });
        }
        assert!(b.input_no_stall());
        let mut bad = DragBench::new();
        for _ in 0..50 {
            let _ = bad.record(16_000, 200);
        }
        bad.record_input_lag(2);
        assert!(!bad.input_no_stall());
    }

    #[test]
    fn dragbench_all_checks_pass() {
        let set = run_dragbench_checks();
        assert!(set.len() >= 14, "B-502 CheckSet 应≥14 项，实际 {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "B-502 check {} failed: {}", c.name, c.detail);
        }
    }
}
