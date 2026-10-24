//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0333 · 多核启动失败分类（未达/超时/三次失败）· 诊断段 0x3ec0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0333-J1 三类失败分类正确，三次失败进隔离流程（B18
//!   F0347 对接）。
//! 难点分析：
//!   AP 启动失败最难的不是「失败了」而是「失败到哪一步」：SIPI
//!   发了没响应 = 未达（芯片级嫌疑）；有响应但 init 没完成 = 超时
//!   （软件级嫌疑）。分类错，归因就错，隔离流程就会把好核关掉或
//!   把坏核放回来。本模块把「未达/超时/三次失败」做成状态机：
//!   每次尝试落一行账，状态迁移有判据，三次失败硬性进隔离队列。
//! 纪律约束：
//!   - 分类互斥：一次尝试只归一类；
//!   - 三振出局：第三次失败立即入隔离队列，不允许第四次尝试；
//!   - 隔离账可查：谁、何时、哪类、第几次；
//!   - 成功重置：中间成功一次则计数清零（间歇性故障不冤枉）。
//! 成本记账：分类 O(1)；隔离 O(1)；审计 O(AP 数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3ec0;

/// 尝试结果分类（互斥三态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailClass {
    /// SIPI 发出后 INIT-SIPI-SIPI 序列无任何响应。
    Unreached,
    /// 有响应但 init 例程在窗口内未完成。
    Timeout,
    /// 本次成功（用于重置计数）。
    Success,
}

impl FailClass {
    pub fn label(self) -> &'static str {
        match self {
            FailClass::Unreached => "未达",
            FailClass::Timeout => "超时",
            FailClass::Success => "成功",
        }
    }
}

/// 单 AP 的尝试账与状态机。
#[derive(Debug)]
pub struct ApBootSm {
    pub ap_id: u32,
    /// 连续失败计数（成功清零）。
    fail_streak: u32,
    /// 尝试历史（最近 8 次）。
    pub history: Vec<FailClass>,
    /// 已入隔离队列。
    pub isolated: bool,
    /// 状态机闭合：隔离后不允许再尝试。
    locked: bool,
}

/// 隔离队例行（B18 F0347 对接接口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsolateRow {
    pub ap_id: u32,
    pub fail_class: FailClass,
    pub attempt_no: u32,
}

/// 全局启动监督器。
#[derive(Debug)]
pub struct BootSupervisor {
    sms: Vec<ApBootSm>,
    pub quarantine: Vec<IsolateRow>,
}

/// 单次尝试的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 继续（成功或前两次失败）。
    Continue,
    /// 三振出局，进入隔离。
    Quarantined,
    /// 状态机已锁（隔离后再试 = 编程错误）。
    Locked,
}

impl BootSupervisor {
    pub fn new(aps: &[u32]) -> Self {
        BootSupervisor {
            sms: aps
                .iter()
                .map(|&id| ApBootSm { ap_id: id, fail_streak: 0, history: Vec::new(), isolated: false, locked: false })
                .collect(),
            quarantine: Vec::new(),
        }
    }

    /// 记录一次尝试并给结论。
    pub fn attempt(&mut self, ap_id: u32, class: FailClass) -> Result<Verdict, &'static str> {
        let sm = self.sms.iter_mut().find(|s| s.ap_id == ap_id).ok_or("未知 AP")?;
        if sm.locked {
            return Ok(Verdict::Locked);
        }
        if sm.history.len() >= 8 {
            sm.history.remove(0); // 环形上限
        }
        sm.history.push(class);
        match class {
            FailClass::Success => {
                sm.fail_streak = 0;
                Ok(Verdict::Continue)
            }
            FailClass::Unreached | FailClass::Timeout => {
                sm.fail_streak += 1;
                if sm.fail_streak >= 3 {
                    sm.isolated = true;
                    sm.locked = true;
                    self.quarantine.push(IsolateRow { ap_id, fail_class: class, attempt_no: sm.fail_streak });
                    Ok(Verdict::Quarantined)
                } else {
                    Ok(Verdict::Continue)
                }
            }
        }
    }

    /// 未达 AP 清单（芯片级归因视图）。
    pub fn unreached_aps(&self) -> Vec<u32> {
        self.sms
            .iter()
            .filter(|s| s.history.last() == Some(&FailClass::Unreached))
            .map(|s| s.ap_id)
            .collect()
    }

    /// 超时 AP 清单。
    pub fn timeout_aps(&self) -> Vec<u32> {
        self.sms
            .iter()
            .filter(|s| s.history.last() == Some(&FailClass::Timeout) && !s.isolated)
            .map(|s| s.ap_id)
            .collect()
    }

    /// 审计闭合：隔离者 streak==3 且 history 非空；未隔离者 streak<3。
    pub fn audit(&self) -> bool {
        self.sms.iter().all(|s| {
            if s.isolated {
                s.fail_streak == 3 && s.locked
            } else {
                s.fail_streak < 3 && !s.locked
            }
        })
    }
}

/// 黄金夹具：AP1 两次未达后成功；AP2 三次超时出局；AP3 一次未达两次超时出局。
pub fn golden_supervisor() -> BootSupervisor {
    let mut sup = BootSupervisor::new(&[1, 2, 3]);
    let _ = sup.attempt(1, FailClass::Unreached);
    let _ = sup.attempt(1, FailClass::Unreached);
    let _ = sup.attempt(1, FailClass::Success);
    let _ = sup.attempt(2, FailClass::Timeout);
    let _ = sup.attempt(2, FailClass::Timeout);
    let _ = sup.attempt(2, FailClass::Timeout);
    let _ = sup.attempt(3, FailClass::Unreached);
    let _ = sup.attempt(3, FailClass::Timeout);
    let _ = sup.attempt(3, FailClass::Timeout);
    sup
}

/// UNX-F0333 判据（J1 三类分类正确 + 三振入隔离）。
pub fn run_unx333_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0333 多核启动失败分类");

    // J1 主线
    let sup = golden_supervisor();
    cs.add("J1 AP2 三振隔离", sup.sms.iter().find(|s| s.ap_id == 2).map(|s| s.isolated).unwrap_or(false), "三次超时");
    cs.add("J1 AP3 三振隔离", sup.sms.iter().find(|s| s.ap_id == 3).map(|s| s.isolated).unwrap_or(false), "混类三振");
    cs.add("J1 AP1 成功存活", !sup.sms.iter().find(|s| s.ap_id == 1).map(|s| s.isolated).unwrap_or(true), "两败一成");
    cs.add("J1 隔离账 2 行", sup.quarantine.len() == 2, "AP2+AP3");
    cs.add("J1 隔离账行完整", sup.quarantine[0] == IsolateRow { ap_id: 2, fail_class: FailClass::Timeout, attempt_no: 3 }, "三要素");
    cs.add("J1 审计闭合", sup.audit(), "streak/locked 一致");

    // 分类视图
    cs.add("未达视图", sup.unreached_aps().is_empty(), "末次皆非未达");
    cs.add("超时视图排除已隔离", sup.timeout_aps().is_empty(), "AP2/3 已隔离");

    // 注入 1：成功重置计数（间歇故障不冤枉）
    let mut r1 = BootSupervisor::new(&[7]);
    let _ = r1.attempt(7, FailClass::Unreached);
    let _ = r1.attempt(7, FailClass::Timeout);
    let v = r1.attempt(7, FailClass::Success);
    let v2 = r1.attempt(7, FailClass::Unreached);
    cs.add("注入1 成功重置", v == Ok(Verdict::Continue) && v2 == Ok(Verdict::Continue), "streak 归零");
    cs.add("注入1 重置后未隔离", !r1.sms[0].isolated, "第四败才出局");

    // 注入 2：隔离后再试 → Locked（防第四次尝试）
    let mut r2 = BootSupervisor::new(&[8]);
    let _ = r2.attempt(8, FailClass::Unreached);
    let _ = r2.attempt(8, FailClass::Unreached);
    let v3 = r2.attempt(8, FailClass::Unreached);
    let v4 = r2.attempt(8, FailClass::Unreached);
    cs.add("注入2 三振结论 Quarantined", v3 == Ok(Verdict::Quarantined), "硬门槛");
    cs.add("注入2 四试 Locked", v4 == Ok(Verdict::Locked), "状态机闭合");

    // 注入 3：未知 AP 拒收
    let mut r3 = BootSupervisor::new(&[1]);
    cs.add("注入3 未知 AP 拒收", r3.attempt(99, FailClass::Timeout).is_err(), "不在名册");

    // 注入 4：隔离账分类保真（未达出局也入账）
    let mut r4 = BootSupervisor::new(&[5]);
    let _ = r4.attempt(5, FailClass::Timeout);
    let _ = r4.attempt(5, FailClass::Timeout);
    let _ = r4.attempt(5, FailClass::Unreached);
    cs.add("注入4 末振分类保真", r4.quarantine[0].fail_class == FailClass::Unreached, "按第三败归类");
    cs.add("注入4 尝试历史环形", r4.sms[0].history.len() == 3, "未触上限");

    cs
}
