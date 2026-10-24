//! unx0236_feedback —— UNX-F0236 邻域消费方验收反馈通道。
//! 锚点（CoRun Unxreal 主册）：UNX-F0236 · 邻域反馈通道 · 目标 200 行
//! 判据原文：UNX-F0236-J1 邻域消费方对 A1 交付物的验收反馈可入账：
//!           接受/拒绝二元结论 + 拒绝必有因码；重复反馈幂等去重；
//!           拒绝项进返工队列不无声消化。
//! 组件：反馈记录（消费方 + 批次 + 结论 + 因码）+ 反馈账（去重入账
//!       + 分批聚合 + 返工队列）+ 幂等核（同消费方同批同结论重复
//!       提交只记一次）。
//! 职责定位：反馈通道是「域间契约的回执联」——拒绝不可无声：每条
//!   拒绝必须带因码且进返工队列，直到对账时队列清空才算闭环。
//!   幂等保证网络层重发不产生重复账（同键同值 = 同一笔）。
//! 性能逐项分解：入账 O(账深)（线性查重）；聚合 O(账深)；队列 O(账深)。
//! 诊断码段：0x20080（0x20081~0x20089 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FB_VERSION: &str = "unx0236_feedback-v1";

/// 诊断段基址（0x20080 段 · 本模块独占）。
pub const FB_SEG_BASE: u32 = 0x20080;
pub const XC_DUP_FB: u32 = 0x20081; // 重复反馈（异值同键）
pub const XC_NO_REASON: u32 = 0x20082; // 拒绝无因码
pub const XC_EMPTY_FB: u32 = 0x20083; // 空反馈拒
pub const XC_BATCH_OOR: u32 = 0x20084; // 批号越域
pub const XC_VERDICT_OOR: u32 = 0x20085; // 结论态越域
pub const XC_QUEUE_STUCK: u32 = 0x20086; // 返工队列未清
pub const XC_CONSUMER_OOR: u32 = 0x20087; // 消费方越域
pub const XC_IDEMPOTENT: u32 = 0x20088; // 幂等冲突
pub const XC_CLOSED: u32 = 0x20089; // 通道已闭账拒新反馈

/// 验收结论。
pub const V_ACCEPT: u8 = 1;
pub const V_REJECT: u8 = 2;

/// 单条验收反馈。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Feedback {
    /// 消费方域号（1~8 邻域闭集）。
    pub consumer: u32,
    /// 批次号（1~40）。
    pub batch: u32,
    pub verdict: u8,
    /// 拒绝因码（接受时必须 0）。
    pub reason: u32,
}

impl Feedback {
    pub fn new(consumer: u32, batch: u32, verdict: u8, reason: u32) -> Result<Feedback, u32> {
        if consumer == 0 || consumer > 8 {
            return Err(XC_CONSUMER_OOR);
        }
        if batch == 0 || batch > 40 {
            return Err(XC_BATCH_OOR);
        }
        if verdict != V_ACCEPT && verdict != V_REJECT {
            return Err(XC_VERDICT_OOR);
        }
        if verdict == V_REJECT && reason == 0 {
            return Err(XC_NO_REASON);
        }
        if verdict == V_ACCEPT && reason != 0 {
            return Err(XC_VERDICT_OOR); // 接受不携带因码（语义闭合）
        }
        Ok(Feedback { consumer, batch, verdict, reason })
    }

    /// 幂等键：消费方×批次（同键只允许同值重复）。
    pub fn key(&self) -> u64 {
        ((self.consumer as u64) << 32) | self.batch as u64
    }
}

/// 邻域反馈账。
pub struct FeedbackBook {
    entries: Vec<Feedback>,
    closed: bool,
}

impl FeedbackBook {
    pub fn new() -> FeedbackBook {
        FeedbackBook { entries: Vec::new(), closed: false }
    }

    /// 入账：幂等去重——同键同值重发静默吸收返回 Ok(false)；
    /// 同键异值是账面冲突显式拒；拒绝无因码在 Feedback::new 已拦。
    pub fn record(&mut self, fb: Feedback) -> Result<bool, u32> {
        if self.closed {
            return Err(XC_CLOSED);
        }
        if let Some(old) = self.entries.iter().find(|e| e.key() == fb.key()) {
            if *old == fb {
                return Ok(false); // 幂等吸收
            }
            return Err(XC_IDEMPOTENT); // 同键翻供 = 冲突立案
        }
        self.entries.push(fb);
        Ok(true)
    }

    /// 闭账：闭后拒新反馈（回执联封存）。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 分批聚合：批号 → (接受数, 拒绝数)。
    pub fn tally(&self, batch: u32) -> (u32, u32) {
        let mut acc = 0;
        let mut rej = 0;
        for e in self.entries.iter() {
            if e.batch == batch {
                if e.verdict == V_ACCEPT {
                    acc += 1;
                } else {
                    rej += 1;
                }
            }
        }
        (acc, rej)
    }

    /// 返工队列：所有拒绝的（消费方, 批次）键清单。
    pub fn rework_queue(&self) -> Vec<u64> {
        let mut q = Vec::new();
        for e in self.entries.iter() {
            if e.verdict == V_REJECT && !q.contains(&e.key()) {
                q.push(e.key());
            }
        }
        q
    }

    /// 账深。
    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }
}

pub fn run_unx0236_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0236_feedback");
    // 族一：合法反馈入账。
    let mut book = FeedbackBook::new();
    let f1 = Feedback::new(1, 5, V_ACCEPT, 0).unwrap_or(Feedback { consumer: 1, batch: 5, verdict: V_ACCEPT, reason: 0 });
    let f2 = Feedback::new(2, 5, V_REJECT, 0x21).unwrap_or(Feedback { consumer: 2, batch: 5, verdict: V_REJECT, reason: 0x21 });
    cs.add("接受入账", book.record(f1) == Ok(true), "首记新账");
    cs.add("拒绝入账", book.record(f2) == Ok(true), "带因码 0x21");
    cs.add("账深", book.len() == 2, "两条");
    // 族二：幂等。
    cs.add("重发吸收", book.record(f1) == Ok(false), "同键同值幂等");
    cs.add("翻供拒", book.record(Feedback::new(2, 5, V_ACCEPT, 0).unwrap_or(f2)).is_err(),
           "同键异值冲突");
    // 族三：分批聚合。
    let _ = book.record(Feedback::new(3, 5, V_ACCEPT, 0).unwrap_or(f1));
    let (acc, rej) = book.tally(5);
    cs.add("聚合接受", acc == 2, "1+3 号");
    cs.add("聚合拒绝", rej == 1, "2 号");
    cs.add("空批聚合", book.tally(39) == (0, 0), "无反馈批零");
    // 族四：返工队列。
    let q = book.rework_queue();
    cs.add("队列深度", q.len() == 1, "一条拒绝");
    cs.add("队列键", q.first().map(|k| *k == ((2u64) << 32) | 5).unwrap_or(false), "2×5");
    // 族五：构造拒。
    cs.add("拒绝无因码拒", Feedback::new(1, 1, V_REJECT, 0).is_err(), "NO_REASON");
    cs.add("接受带因码拒", Feedback::new(1, 1, V_ACCEPT, 7).is_err(), "语义闭合");
    cs.add("消费方越域拒", Feedback::new(9, 1, V_ACCEPT, 0).is_err(), "CONSUMER");
    cs.add("批号越域拒", Feedback::new(1, 41, V_ACCEPT, 0).is_err(), "BATCH");
    cs.add("结论越域拒", Feedback::new(1, 1, 7, 0).is_err(), "VERDICT");
    // 族六：闭账。
    book.close();
    cs.add("闭后拒新", matches!(book.record(f1), Err(XC_CLOSED)), "回执联封存");
    cs.add("闭账聚合仍可读", book.tally(5) == (2, 1), "只读放行");
    cs
}
