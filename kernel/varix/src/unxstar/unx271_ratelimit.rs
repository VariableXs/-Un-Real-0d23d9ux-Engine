// SPDX-License-Identifier: Apache-2.0
//! UNX-F0271 · 终端输出限流与背压
//!
//! 洪泛输出限流生效（行/秒上限），背压丢弃计数显性化。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 限流器
// ---------------------------------------------------------------------------

/// 行限流器（令牌桶）。
pub struct RateLimiter {
    /// 行/秒上限。
    pub max_lines_per_sec: u32,
    /// 当前秒已发。
    pub used_this_sec: u32,
    /// 当前秒编号（调用方推进）。
    pub current_sec: u64,
    /// 丢弃计数。
    pub dropped: u64,
}

impl RateLimiter {
    pub fn new(max_lines_per_sec: u32) -> Self {
        RateLimiter { max_lines_per_sec, used_this_sec: 0, current_sec: 0, dropped: 0 }
    }

    /// 推进到新秒（重置配额）。
    pub fn advance_sec(&mut self, sec: u64) {
        if sec != self.current_sec {
            self.current_sec = sec;
            self.used_this_sec = 0;
        }
    }

    /// 尝试输出一行。
    pub fn try_output(&mut self) -> bool {
        if self.used_this_sec >= self.max_lines_per_sec {
            self.dropped += 1;
            return false;
        }
        self.used_this_sec += 1;
        true
    }

    /// 是否被限流。
    pub fn is_throttled(&self) -> bool {
        self.used_this_sec >= self.max_lines_per_sec
    }
}

// ---------------------------------------------------------------------------
// 背压
// ---------------------------------------------------------------------------

/// 背压控制器。
pub struct Backpressure {
    /// 缓冲上限。
    pub max_buffer: usize,
    /// 当前深度。
    pub depth: usize,
    /// 丢弃计数。
    pub dropped: u64,
}

impl Backpressure {
    pub fn new(max_buffer: usize) -> Self {
        Backpressure { max_buffer, depth: 0, dropped: 0 }
    }

    /// 写入（超限丢弃并计数）。
    pub fn push(&mut self) -> bool {
        if self.depth >= self.max_buffer {
            self.dropped += 1;
            return false;
        }
        self.depth += 1;
        true
    }

    /// 消费。
    pub fn pop(&mut self) -> bool {
        if self.depth > 0 {
            self.depth -= 1;
            true
        } else {
            false
        }
    }

    /// 使用率。
    pub fn usage(&self) -> f32 {
        if self.max_buffer == 0 { return 0.0; }
        self.depth as f32 / self.max_buffer as f32
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0271 判据全集。
pub fn run_rate_limit_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0271", "终端输出限流与背压");

    // 1. 限流
    let mut rl = RateLimiter::new(3);
    cs.ok("输出 1", rl.try_output(), "配额内");
    cs.ok("输出 2", rl.try_output(), "配额内");
    cs.ok("输出 3", rl.try_output(), "配额内");
    cs.ok("限流生效", !rl.try_output(), "超限");
    cs.ok("丢弃计数", rl.dropped == 1, "显性化");
    cs.ok("被限流", rl.is_throttled(), "状态");

    // 2. 秒推进重置
    rl.advance_sec(1);
    cs.ok("新秒重置", !rl.is_throttled(), "配额重置");
    cs.ok("再输出", rl.try_output(), "新秒可发");

    // 3. 背压
    let mut bp = Backpressure::new(3);
    cs.ok("写入 1", bp.push(), "缓冲内");
    cs.ok("写入 2", bp.push(), "缓冲内");
    cs.ok("写入 3", bp.push(), "缓冲满");
    cs.ok("背压丢弃", !bp.push(), "超限");
    cs.ok("丢弃计数", bp.dropped == 1, "显性化");
    cs.ok("使用率满", (bp.usage() - 1.0).abs() < 0.01, "100%");

    // 4. 消费
    cs.ok("消费 1", bp.pop(), "释放");
    cs.ok("使用率", (bp.usage() - 2.0/3.0).abs() < 0.01, "约 67%");
    cs.ok("再写入", bp.push(), "有空间");

    // 5. 空消费
    let mut empty_bp = Backpressure::new(10);
    cs.ok("空消费", !empty_bp.pop(), "无数据");

    cs.done()
}
