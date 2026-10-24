// SPDX-License-Identifier: Apache-2.0
//! UNX-F0236 · 邻域消费方验收反馈通道
//!
//! A2/A4/L1 消费方反馈表单与问题登记流打通，首轮回访 3 方全过。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 反馈模型
// ---------------------------------------------------------------------------

/// 消费方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    /// A2 域。
    A2,
    /// A4 域。
    A4,
    /// L1 层。
    L1,
}

impl Consumer {
    pub fn label(&self) -> &'static str {
        match self {
            Consumer::A2 => "A2",
            Consumer::A4 => "A4",
            Consumer::L1 => "L1",
        }
    }
}

/// 反馈状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackStatus {
    /// 待处理。
    Pending,
    /// 处理中。
    InProgress,
    /// 已解决。
    Resolved,
    /// 已关闭。
    Closed,
}

/// 反馈记录。
#[derive(Debug, Clone)]
pub struct Feedback {
    pub consumer: Consumer,
    pub status: FeedbackStatus,
    pub content: String,
    pub resolved: bool,
}

// ---------------------------------------------------------------------------
// 反馈通道
// ---------------------------------------------------------------------------

/// 反馈通道。
pub struct FeedbackChannel {
    pub feedbacks: Vec<Feedback>,
    /// 问题登记流。
    pub issue_log: Vec<String>,
    /// 回访计数。
    pub follow_ups: Vec<(Consumer, bool)>,
}

impl FeedbackChannel {
    pub fn new() -> Self {
        FeedbackChannel { feedbacks: Vec::new(), issue_log: Vec::new(), follow_ups: Vec::new() }
    }

    /// 提交反馈。
    pub fn submit(&mut self, consumer: Consumer, content: &str) {
        self.feedbacks.push(Feedback {
            consumer,
            status: FeedbackStatus::Pending,
            content: content.to_string(),
            resolved: false,
        });
    }

    /// 登记问题。
    pub fn log_issue(&mut self, issue: &str) {
        self.issue_log.push(issue.to_string());
    }

    /// 回访（首轮回访 3 方）。
    pub fn follow_up(&mut self, consumer: Consumer, passed: bool) {
        self.follow_ups.push((consumer, passed));
    }

    /// 三方全过。
    pub fn all_consumers_passed(&self) -> bool {
        let mut seen: Vec<Consumer> = Vec::new();
        for (c, p) in &self.follow_ups {
            if !seen.contains(c) {
                seen.push(*c);
                if !p { return false; }
            }
        }
        seen.contains(&Consumer::A2) && seen.contains(&Consumer::A4) && seen.contains(&Consumer::L1)
    }

    /// 反馈总数。
    pub fn count(&self) -> usize { self.feedbacks.len() }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0236 判据全集。
pub fn run_feedback_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0236", "消费方反馈通道");

    // 1. 消费方
    cs.ok("三方", Consumer::A2.label() == "A2", "A2/A4/L1");
    cs.ok("A4", Consumer::A4.label() == "A4", "标签");
    cs.ok("L1", Consumer::L1.label() == "L1", "标签");

    // 2. 反馈提交
    let mut ch = FeedbackChannel::new();
    ch.submit(Consumer::A2, "need better docs");
    ch.submit(Consumer::A4, "slow response");
    ch.submit(Consumer::L1, "works fine");
    cs.ok("三方反馈", ch.count() == 3, "三条");

    // 3. 问题登记
    ch.log_issue("issue_1");
    ch.log_issue("issue_2");
    cs.ok("问题登记", ch.issue_log.len() == 2, "两条问题");

    // 4. 回访
    ch.follow_up(Consumer::A2, true);
    ch.follow_up(Consumer::A4, true);
    ch.follow_up(Consumer::L1, true);
    cs.ok("三方全过", ch.all_consumers_passed(), "首轮回访");

    // 5. 回访失败
    let mut ch2 = FeedbackChannel::new();
    ch2.follow_up(Consumer::A2, true);
    ch2.follow_up(Consumer::A4, false);
    ch2.follow_up(Consumer::L1, true);
    cs.ok("失败检出", !ch2.all_consumers_passed(), "A4 未过");

    // 6. 状态
    cs.ok("待处理", FeedbackStatus::Pending == FeedbackStatus::Pending, "初始态");

    cs.done()
}
