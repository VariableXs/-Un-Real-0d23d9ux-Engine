//! F139 反馈闭环通道 · 完整设计（STAR I 主册 G-D-14）。
//!
//! **判据（主册）**：端到端：导出-提交-状态更新-修复回链全流程演练；
//! 脱敏三查在提交路径强制。
//!
//! **设计要点（主册）**：报告模板结构化（现象/复现/环境三段必填）；
//! 状态五态与 F129 复用同引擎（一处一事实——本模块直接用
//! ebase::State5）；脱敏失败 → 提交拦截（敏感模式硬查）；重复报告
//! → 相似哈希聚类合并；恶意刷量 → 限频（ebase::RateGate）；编号
//! 全局唯一（日期+序号）；修复回链自动匹配（版本指纹）；季度最佳
//! 报告致谢。
//!
//! 本模块是反馈闭环的**纯逻辑核**：报告模型（三段必填）、脱敏硬门
//! （提交路径上强制——绕不过去）、提交队列、五态流转、回链匹配。

use crate::checks::CheckSet;
use crate::stareco::ebase::{fnv1a64, RateGate, State5, StateTrack, TraceId};

// ---------------------------------------------------------------------------
// 报告模型（现象/复现/环境三段必填）
// ---------------------------------------------------------------------------

pub struct FeedbackReport {
    /// 现象（人话描述）。
    pub symptom: &'static str,
    /// 复现步骤。
    pub repro: &'static str,
    /// 环境（版本指纹）。
    pub environment_fp: u64,
    /// 用户自附附件摘要（脱敏对象）。
    pub attachment: &'static str,
}

impl FeedbackReport {
    /// 模板校验：三段必填（现象/复现/环境——环境以指纹在案为准）。
    pub fn template_ok(&self) -> bool {
        !self.symptom.is_empty() && !self.repro.is_empty() && self.environment_fp != 0
    }
}

// ---------------------------------------------------------------------------
// 脱敏硬门（三查在提交路径强制）
// ---------------------------------------------------------------------------

/// 脱敏三查（复用 F120 三查口径的提交路径面）：
/// ①路径用户段 ②账号/用户名 ③序列号/密钥样本。
/// 注入敏感样本必须被拦——查到即拒，不清洗不猜测。
pub fn sanitize_clear(text: &str) -> bool {
    // ①路径用户段：C:\Users\<name> / /home/<name>
    const NEEDLES_PATH: [&str; 2] = ["C:\\Users\\", "/home/"];
    // ②账号/用户名标记
    const NEEDLES_ACCOUNT: [&str; 2] = ["user:", "account="];
    // ③序列号/密钥样本形态
    const NEEDLES_SECRET: [&str; 2] = ["SN-", "BEGIN PRIVATE KEY"];
    for n in NEEDLES_PATH.iter() {
        if text.contains(n) {
            return false;
        }
    }
    for n in NEEDLES_ACCOUNT.iter() {
        if text.contains(n) {
            return false;
        }
    }
    for n in NEEDLES_SECRET.iter() {
        if text.contains(n) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 闭环通道
// ---------------------------------------------------------------------------

/// 一条在册报告：五态轨迹 + 相似聚类哈希 + 回链。
#[derive(Clone, Copy, Debug)]
pub struct TrackedReport {
    pub id: TraceId,
    pub track: StateTrack,
    /// 相似哈希（现象+环境）——重复报告聚类键。
    pub sim_hash: u64,
    /// 修复回链：修复版指纹（0 = 未回链）。
    pub fix_fp: u64,
}

impl TrackedReport {
    /// 当前五态（查询页数据源）。
    pub fn state(&self) -> State5 {
        self.track.state
    }
}

pub struct FeedbackLoop {
    reports: [Option<TrackedReport>; 16],
    count: usize,
    seq: crate::stareco::ebase::SeqAlloc,
    gate: RateGate,
    /// 聚类合并计数（重复报告不新增，只并簇）。
    pub merged_dupes: u32,
}

impl FeedbackLoop {
    pub fn new() -> FeedbackLoop {
        FeedbackLoop {
            reports: [None; 16],
            count: 0,
            seq: crate::stareco::ebase::SeqAlloc::new(),
            gate: RateGate::new(3_600_000, 5), // 每小时 5 条防刷
            merged_dupes: 0,
        }
    }

    /// 提交：模板三段必填 → 脱敏三查（正文+附件都查）→ 限频 → 聚类。
    /// 任何一关不过 = 拒收且返回原因（三要素给调用方渲染）。
    pub fn submit(&mut self, day: u32, now_ms: u64, r: &FeedbackReport) -> Result<TraceId, &'static str> {
        if !r.template_ok() {
            return Err("三段必填：现象/复现/环境缺一不可");
        }
        if !sanitize_clear(r.symptom) || !sanitize_clear(r.repro) || !sanitize_clear(r.attachment) {
            return Err("脱敏三查未过：提交内容含路径用户段/账号/序列号样本");
        }
        if !self.gate.admit(now_ms) {
            return Err("触发防刷限频：请稍后再试");
        }
        let sim = fnv1a64(r.symptom.as_bytes()) ^ r.environment_fp;
        // 重复报告聚类：同簇只并计数，不占新编号
        let mut hit: Option<usize> = None;
        for (i, slot) in self.reports[..self.count].iter().enumerate() {
            if slot.as_ref().map(|s| s.sim_hash == sim) == Some(true) {
                hit = Some(i);
                break;
            }
        }
        if let Some(i) = hit {
            self.merged_dupes += 1;
            let slot = self.reports[i].as_mut().expect("hit slot");
            // 聚类到已有报告 = 该报告确认度上升，推进一态
            let _ = slot.track.advance(day);
            return Ok(slot.id);
        }
        let s = self.seq.take(day);
        if s == 0 {
            return Err("当日编号额度耗尽");
        }
        let id = TraceId::new("FB", day, s);
        if !id.is_valid() {
            return Err("编号生成失败");
        }
        if self.count >= 16 {
            return Err("队列满");
        }
        self.reports[self.count] = Some(TrackedReport {
            id,
            track: StateTrack::new(day),
            sim_hash: sim,
            fix_fp: 0,
        });
        self.count += 1;
        Ok(id)
    }

    /// 状态更新（确认/处理/解决/关闭——五态单步前进）。
    pub fn update_state(&mut self, id: TraceId, day: u32) -> Result<State5, &'static str> {
        let slot = self
            .reports
            .iter_mut()
            .flatten()
            .find(|s| s.id == id)
            .ok_or("unknown report id")?;
        slot.track.advance(day).map_err(|_| "already closed")
    }

    /// 修复回链：修复版指纹挂回报告（端到端最后一环）。
    pub fn link_fix(&mut self, id: TraceId, fix_fp: u64) -> Result<(), &'static str> {
        let slot = self
            .reports
            .iter_mut()
            .flatten()
            .find(|s| s.id == id)
            .ok_or("unknown report id")?;
        if fix_fp == 0 {
            return Err("empty fix fingerprint");
        }
        slot.fix_fp = fix_fp;
        Ok(())
    }

    /// 回链查询：修复版指纹 → 关联报告编号（修复说明@报告编号的机制面）。
    pub fn reports_fixed_by(&self, fix_fp: u64) -> usize {
        self.reports[..self.count].iter().flatten().filter(|s| s.fix_fp == fix_fp).count()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 编号 → 在册报告只读视图（查询页数据源）。
    pub fn report_view(&self, id: TraceId) -> Option<&TrackedReport> {
        self.reports[..self.count].iter().flatten().find(|s| s.id == id)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F139_TAG: &str = "stareco-F139-feedback";

pub fn run_feedbackloop_checks() -> CheckSet {
    let mut set = CheckSet::new(F139_TAG);

    // 模板三段必填
    let good = FeedbackReport { symptom: "窗口标题闪烁", repro: "打开 explorer 输入长路径", environment_fp: 0xABCD, attachment: "" };
    let no_repro = FeedbackReport { symptom: "闪", repro: "", environment_fp: 1, attachment: "" };
    set.add("f139 template ok", good.template_ok(), "three sections");
    set.add("f139 template missing repro", !no_repro.template_ok(), "must fill all three");

    let mut loop_ = FeedbackLoop::new();
    let day = 20260926;

    // 脱敏三查：注入敏感样本必须被拦（提交路径强制）
    set.add("f139 sanitize clean passes", sanitize_clear("普通描述无敏感物"), "clean text");
    set.add(
        "f139 sanitize path segment",
        !sanitize_clear("日志在 C:\\Users\\alice\\x.txt"),
        "user path segment",
    );
    set.add("f139 sanitize home", !sanitize_clear("/home/bob/core.dump"), "home dir");
    set.add("f139 sanitize account", !sanitize_clear("user: admin 登录失败"), "account field");
    set.add("f139 sanitize serial", !sanitize_clear("SN-123456789"), "serial sample");
    set.add("f139 sanitize key", !sanitize_clear("BEGIN PRIVATE KEY-----"), "key sample");

    // 正常提交 → 编号可查 → 状态流转 → 回链（端到端演练）
    let id = loop_.submit(day, 0, &good).expect("submit");
    set.add("f139 submit issues trace id", id.is_valid(), "day+seq id");
    set.add(
        "f139 five-state ladder",
        loop_.update_state(id, day + 1) == Ok(State5::Confirmed)
            && loop_.update_state(id, day + 2) == Ok(State5::Investigating)
            && loop_.update_state(id, day + 3) == Ok(State5::Resolved)
            && loop_.update_state(id, day + 4) == Ok(State5::Closed),
        "state engine (shared with F129)",
    );
    set.add("f139 closed is terminal", loop_.update_state(id, day + 5).is_err(), "no state after closed");

    // 回链
    assert!(loop_.link_fix(id, 0x7777).is_ok());
    set.add("f139 fix linked & queried", loop_.reports_fixed_by(0x7777) == 1, "fix fingerprint match");
    set.add("f139 empty fix fp rejected", loop_.link_fix(id, 0).is_err(), "honest link");

    // 重复报告聚类
    let dup = FeedbackReport { symptom: "窗口标题闪烁", repro: "打开 explorer 输入长路径", environment_fp: 0xABCD, attachment: "" };
    let dup_id = loop_.submit(day, 1000, &dup).expect("dup routes to cluster");
    set.add(
        "f139 duplicate clustered",
        dup_id == id && loop_.merged_dupes == 1 && loop_.len() == 1,
        "no new id for same cluster",
    );

    // 敏感样本提交被拦（在路径上，不是事后）
    let dirty = FeedbackReport { symptom: "报错位于 C:\\Users\\me\\log", repro: "r", environment_fp: 2, attachment: "" };
    set.add("f139 dirty submit blocked", loop_.submit(day, 2000, &dirty).is_err(), "sanitize gate on submit path");

    // 限频
    let mut flood = FeedbackLoop::new();
    let mut admitted = 0;
    for i in 0..8u64 {
        if flood
            .submit(20260101, i * 60_000, &FeedbackReport { symptom: "s", repro: "r", environment_fp: i + 1, attachment: "" })
            .is_ok()
        {
            admitted += 1;
        }
    }
    set.add("f139 flood capped at 5/h", admitted == 5, "rate gate engaged");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_drill() {
        let mut fb = FeedbackLoop::new();
        let r = FeedbackReport { symptom: "a", repro: "b", environment_fp: 9, attachment: "" };
        let id = fb.submit(20260101, 0, &r).unwrap();
        fb.update_state(id, 20260102).unwrap();
        fb.update_state(id, 20260103).unwrap();
        fb.link_fix(id, 42).unwrap();
        assert_eq!(fb.reports_fixed_by(42), 1);
    }
}
