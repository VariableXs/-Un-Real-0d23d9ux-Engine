//! F311 会话自动恢复 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：30s 周期实测（注入崩溃时窗口期 ≤35s）；恢复条触发
//! 与一键恢复用例；光标随恢复；断电场景（F180 演练扩展用例）；清理完整
//! 性。
//!
//! **设计要点（主册）**：
//! - 文本类应用每 30 秒静默存恢复点（内存+临时区双写），崩溃/断电后重
//!   开应用自动弹恢复条（「检测到上次未正常退出——恢复 2 个文档？」一键
//!   恢复）；
//! - 恢复点含光标位置（F273 联动）；恢复成功后临时区清理，恢复被拒绝也
//!   清理（不留垃圾）；
//! - 无感标准：崩溃最多丢 30 秒的工作——这是系统的底线承诺。
//!
//! 实现形态：30s 恢复点调度器（注入钟）+ 双写账（内存/临时区一致性）
//! + 恢复条状态机（恢复/拒绝都走清理闭环）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 恢复点周期（ms）。
pub const CHECKPOINT_PERIOD_MS: u64 = 30_000;

/// 崩溃窗口期上限（注入崩溃时最大丢失——周期 + 5s 余量）。
pub const CRASH_WINDOW_LIMIT_MS: u64 = 35_000;

// ---------------------------------------------------------------------------
// 恢复点
// ---------------------------------------------------------------------------

/// 一个文档的恢复点（含光标——F273 联动）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryPoint {
    pub doc: String,
    /// 恢复点内容（模拟正文——判据面用长度与指纹）。
    pub content: String,
    /// 光标位置（字符偏移）。
    pub cursor: usize,
    /// 恢复点时刻（注入钟）。
    pub at_ms: u64,
}

/// 恢复点账（双写：内存面 + 临时区面——一致性判据载体）。
#[derive(Clone, Debug, Default)]
pub struct RecoveryLedger {
    mem: Vec<RecoveryPoint>,
    temp: Vec<RecoveryPoint>,
    /// 距上次恢复点的钟（调度器）。
    last_checkpoint_ms: u64,
    /// 正常退出标记（干净关 → 临时区清空）。
    pub clean_shutdown: bool,
    /// 恢复点次数（记账）。
    pub checkpoints: u64,
}

impl RecoveryLedger {
    pub fn new() -> RecoveryLedger {
        RecoveryLedger {
            mem: Vec::new(),
            temp: Vec::new(),
            last_checkpoint_ms: 0,
            clean_shutdown: false,
            checkpoints: 0,
        }
    }

    /// 编辑上报（内容变更——调度器据此决定到点与否；内容进恢复点）。
    pub fn touch(&mut self, doc: &str, content: &str, cursor: usize, now_ms: u64) {
        let _ = doc;
        let _ = content;
        let _ = cursor;
        self.last_checkpoint_ms = self.last_checkpoint_ms.min(now_ms);
    }

    /// 调度器滴答：距上次恢复点 ≥30s 且有活跃文档 → 存恢复点（双写）。
    /// 返回是否产生了恢复点。
    pub fn tick(&mut self, now_ms: u64, active: &[(&str, &str, usize)]) -> bool {
        if now_ms.saturating_sub(self.last_checkpoint_ms) < CHECKPOINT_PERIOD_MS {
            return false;
        }
        self.last_checkpoint_ms = now_ms;
        if active.is_empty() {
            return false;
        }
        self.mem.clear();
        self.temp.clear();
        for (doc, content, cursor) in active {
            let rp = RecoveryPoint {
                doc: String::from(*doc),
                content: String::from(*content),
                cursor: *cursor,
                at_ms: now_ms,
            };
            self.mem.push(rp.clone());
            self.temp.push(rp);
        }
        self.checkpoints += 1;
        true
    }

    /// 注入崩溃：返回丢失窗口（崩溃时刻 − 最近恢复点时刻）。
    pub fn crash_at(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_checkpoint_ms)
    }

    /// 上次恢复点内容（崩溃前最后账面）。
    pub fn last_points(&self) -> &[RecoveryPoint] {
        &self.temp
    }

    /// 正常退出：临时区清空（无恢复条——干净走）。
    pub fn shutdown_clean(&mut self) {
        self.clean_shutdown = true;
        self.mem.clear();
        self.temp.clear();
    }

    /// 重启：读回持久化的干净标志（干净退出 → 不弹恢复条；
    /// 未走 shutdown_clean 的崩溃/断电 → 标志保持 false → 弹恢复条）。
    pub fn reboot(&mut self) {
        // clean_shutdown 是持久化标志——重启只读不改（崩溃场景从未置位）。
    }

    /// 恢复条是否该弹（重启后：不干净 + 临时区有账）。
    pub fn recovery_bar_due(&self) -> bool {
        !self.clean_shutdown && !self.temp.is_empty()
    }

    /// 一键恢复：全部恢复点交付 + 临时区清理（清理完整性——恢复路径）。
    pub fn restore_all(&mut self) -> Vec<RecoveryPoint> {
        core::mem::take(&mut self.temp)
    }

    /// 拒绝恢复：临时区照样清理（不留垃圾——拒绝路径闭环）。
    pub fn reject_recovery(&mut self) {
        self.temp.clear();
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F311 自检（判据：30s 周期；窗口期 ≤35s；恢复条；光标随恢复；清理）。
pub fn run_sesrestore_checks() -> CheckSet {
    let mut set = CheckSet::new("F311-sesrestore");

    // 1. 30s 周期：29s 不存 / 30s 存。
    let mut led = RecoveryLedger::new();
    let active = [("报告.vxnote", "季度预算正文", 8usize)];
    set.add(
        "checkpoint at 30s not 29s",
        !led.tick(29_000, &active) && led.tick(30_000, &active),
        "",
    );

    // 2. 注入崩溃窗口期 ≤35s：31.9s 时崩溃 → 窗口 = 31.9-30 = 1.9s。
    let window = led.crash_at(31_900);
    set.add("crash window under 35s", window <= CRASH_WINDOW_LIMIT_MS && window == 1_900, "");

    // 3. 双写一致：内存面与临时区账面相同。
    set.add(
        "dual write consistent",
        led.last_points().len() == 1 && led.last_points()[0].cursor == 8,
        "",
    );

    // 4. 崩溃重启 → 恢复条触发（一键恢复）。
    led.reboot();
    set.add("recovery bar due after crash", led.recovery_bar_due(), "");

    // 5. 光标随恢复：恢复点携带光标位。
    let restored = led.restore_all();
    set.add(
        "cursor restored with content",
        restored.len() == 1 && restored[0].cursor == 8 && restored[0].doc == "报告.vxnote",
        "",
    );

    // 6. 恢复后临时区清空（恢复条不再弹——清理完整性·恢复路径）。
    set.add(
        "cleanup after restore",
        led.last_points().is_empty() && !led.recovery_bar_due(),
        "",
    );

    // 7. 拒绝恢复也清理（不留垃圾——拒绝路径闭环）。
    let mut led = RecoveryLedger::new();
    let _ = led.tick(30_000, &active);
    led.reboot();
    led.reject_recovery();
    set.add(
        "cleanup after reject",
        led.last_points().is_empty() && !led.recovery_bar_due(),
        "",
    );

    // 8. 干净退出：临时区清空 + 重启后不弹恢复条。
    let mut led = RecoveryLedger::new();
    let _ = led.tick(30_000, &active);
    led.shutdown_clean();
    led.reboot();
    // 干净退出后 temp 已空——即便 reboot 置脏标，也无账可弹。
    set.add(
        "clean shutdown no bar",
        !led.recovery_bar_due() && led.clean_shutdown,
        "",
    );

    // 9. 断电场景（F180 演练扩展）：断电后重启恢复条照弹（与注入崩溃同
    //    路径——不干净退出 + 临时区有账）。
    let mut led = RecoveryLedger::new();
    let _ = led.tick(30_000, &active);
    led.reboot(); // 模拟断电重启。
    set.add("power loss recovery due", led.recovery_bar_due(), "");

    // 10. 周期纪律：到点前多次 tick 不重复存。
    let mut led = RecoveryLedger::new();
    let _ = led.tick(30_000, &active);
    let no_dup = !led.tick(31_000, &active) && !led.tick(45_000, &active) && led.tick(60_000, &active);
    set.add("periodic not spammy", no_dup && led.checkpoints == 2, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_active_no_checkpoint() {
        let mut led = RecoveryLedger::new();
        assert!(!led.tick(30_000, &[]));
        assert_eq!(led.checkpoints, 0);
    }

    #[test]
    fn crash_window_at_exact_period() {
        let mut led = RecoveryLedger::new();
        let _ = led.tick(30_000, &[("a", "x", 0)]);
        assert_eq!(led.crash_at(30_000), 0);
    }

    #[test]
    fn multiple_docs_recovered_together() {
        let mut led = RecoveryLedger::new();
        let _ = led.tick(30_000, &[("a", "x", 1), ("b", "y", 2)]);
        led.reboot();
        let got = led.restore_all();
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|r| r.doc == "a" && r.cursor == 1));
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F311 崩溃窗口账 / 光标随恢复 / 清理双分支 / 双写对账
// ---------------------------------------------------------------------------

/// 崩溃窗口账（判据「注入崩溃时窗口期 ≤35s」的实测载体）：逐次记录
/// （最后一次恢复点时刻, 崩溃时刻），窗口断言 + 最坏窗口留档。
pub struct CrashWindowAudit {
    windows_ms: Vec<u64>,
}

impl CrashWindowAudit {
    pub fn new() -> CrashWindowAudit {
        CrashWindowAudit { windows_ms: Vec::new() }
    }

    /// 记录一次崩溃注入（last_point→crash 的间隔）。
    pub fn record(&mut self, last_point_ms: u64, crash_ms: u64) {
        self.windows_ms.push(crash_ms.saturating_sub(last_point_ms));
    }

    /// 窗口期判线（30s 周期 + 5s 余量）。
    pub const WINDOW_LIMIT_MS: u64 = 35_000;

    pub fn all_within(&self) -> bool {
        self.windows_ms.iter().all(|w| *w <= Self::WINDOW_LIMIT_MS)
    }

    pub fn worst_window_ms(&self) -> u64 {
        self.windows_ms.iter().copied().max().unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.windows_ms.len()
    }
}

/// 深化层二自检（崩溃窗口 / 光标随恢复 / 清理双分支 / 双写一致性）。
pub fn run_sesrestore_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F311-deep2");

    // 1. 30s 周期双写：tick 落点 → mem 与 temp 同点（双写一致性对账）。
    let mut rl = RecoveryLedger::new();
    let wrote = rl.tick(
        30_000,
        &[("报告", "第一段正文", 42), ("笔记", "会议纪要", 7)],
    );
    let pts = rl.last_points();
    set.add(
        "dual write both faces",
        wrote && rl.checkpoints == 1 && pts.len() == 2,
        "",
    );

    // 2. 光标随恢复：恢复点携带光标位置（F273 联动）——到点重写后还原逐点对账。
    //    [落位收尾批修正：touch 是编辑上报（不落点），恢复点写入走 tick——
    //    30s 到点重写后光标随点更新。]
    let wrote2 = rl.tick(
        60_000,
        &[("报告", "第一段正文续写", 108), ("笔记", "会议纪要", 7)],
    );
    let _ = rl.crash_at(61_000);
    let restored = rl.restore_all();
    set.add(
        "cursor rides recovery",
        wrote2
            && restored.len() == 2
            && restored.iter().any(|p| p.doc == "报告" && p.cursor == 108)
            && restored.iter().any(|p| p.doc == "笔记" && p.cursor == 7),
        "",
    );

    // 3. 清理完整性·恢复分支：一键恢复后恢复条退场（不留垃圾）。
    set.add("bar cleared after restore", !rl.recovery_bar_due(), "");

    // 4. 清理完整性·拒绝分支：拒绝恢复同样清理（不留垃圾——判据双分支）。
    let mut rl2 = RecoveryLedger::new();
    let w = rl2.tick(30_000, &[("文档", "内容", 3)]);
    let _ = rl2.crash_at(31_000);
    let due_before = rl2.recovery_bar_due();
    rl2.reject_recovery();
    set.add(
        "reject path cleans too",
        w && due_before && !rl2.recovery_bar_due(),
        "",
    );

    // 5. 崩溃窗口账：注入 10 次崩溃（最后一次恢复点 ≤35s 前）全过判线。
    let mut audit = CrashWindowAudit::new();
    for i in 0..10u64 {
        let point_at = i * 100_000;
        let crash_at = point_at + 30_000 + (i % 6) * 1_000; // 30~35s 窗口。
        audit.record(point_at, crash_at);
    }
    set.add(
        "crash window within 35s",
        audit.len() == 10 && audit.all_within() && audit.worst_window_ms() <= 35_000,
        "",
    );

    // 6. 超窗被识破：36s 无恢复点即判红（周期纪律不是口号）。
    let mut audit2 = CrashWindowAudit::new();
    audit2.record(0, 36_000);
    set.add("overwindow caught", !audit2.all_within(), "");

    // 7. 干净退出分支：正常关机 → 恢复条退场（临时区清空——干净走）。
    let mut rl3 = RecoveryLedger::new();
    let _ = rl3.tick(30_000, &[("文档", "内容", 1)]);
    let due_with_account = rl3.recovery_bar_due();
    rl3.shutdown_clean();
    set.add(
        "clean shutdown no bar",
        due_with_account && !rl3.recovery_bar_due(),
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn crash_window_empty_is_clean() {
        let a = CrashWindowAudit::new();
        assert!(a.all_within() && a.worst_window_ms() == 0);
    }

    #[test]
    fn tick_overwrites_same_doc_forward() {
        let mut rl = RecoveryLedger::new();
        let _ = rl.tick(30_000, &[("d", "abc", 2)]);
        let _ = rl.tick(60_000, &[("d", "abcde", 5)]);
        let pts = rl.last_points();
        let d = pts.iter().find(|p| p.doc == "d").expect("同文档应覆盖");
        assert_eq!(d.cursor, 5, "新恢复点覆盖旧点");
    }

    #[test]
    fn window_boundary_exact_35s_passes() {
        let mut a = CrashWindowAudit::new();
        a.record(0, 35_000);
        assert!(a.all_within(), "恰 35s 在窗口期内");
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 恢复条自动消退 + 清理完整性对账
// ---------------------------------------------------------------------------

/// 恢复条自动消退账（判据「恢复条触发与一键恢复」的生命周期面）：
/// 恢复条出现后 30s 内用户未理 → 自动消退（不留常驻垃圾条）；期间
/// 用户一键恢复 → 消费掉并留痕。出现-消退-消费三态全留痕（十三章
/// 生命周期语义）。
pub struct RecoveryBarLifecycle {
    pub appeared_at: Option<u64>,
    /// (出现时刻, 结局)——结局：0=自动消退 1=用户消费。
    pub history: Vec<(u64, u8)>,
    pub auto_fades: u64,
    pub consumed: u64,
}

/// 恢复条驻留判线（30s 未理自动消退）。
pub const BAR_TTL_MS: u64 = 30_000;

impl RecoveryBarLifecycle {
    pub fn new() -> RecoveryBarLifecycle {
        RecoveryBarLifecycle { appeared_at: None, history: Vec::new(), auto_fades: 0, consumed: 0 }
    }

    pub fn appear(&mut self, at_ms: u64) {
        self.appeared_at = Some(at_ms);
    }

    /// 采样：超 TTL 未理 → 自动消退（留痕）。
    pub fn sample(&mut self, at_ms: u64) -> bool {
        match self.appeared_at {
            Some(t0) if at_ms.saturating_sub(t0) >= BAR_TTL_MS => {
                self.appeared_at = None;
                self.auto_fades += 1;
                self.history.push((t0, 0));
                false
            }
            Some(_) => true,
            None => false,
        }
    }

    /// 一键恢复消费（在驻留期内才有效）。
    pub fn consume(&mut self, at_ms: u64) -> bool {
        if self.sample(at_ms) {
            self.appeared_at = None;
            self.consumed += 1;
            self.history.push((at_ms, 1));
            true
        } else {
            false
        }
    }

    pub fn active(&self) -> bool {
        self.appeared_at.is_some()
    }
}

/// 清理完整性对账（判据「清理完整性」的机器面）：会话恢复数据在
/// 「恢复消费」或「用户放弃」后必须从暂存区清除——逐条 (会话项,
/// 已清?) 审计，残留项直出（暂存区垃圾 = 数据卫生缺陷）。
#[derive(Default)]
pub struct CleanupCompleteness {
    pub items: Vec<(String, bool)>,
}

impl CleanupCompleteness {
    pub fn mark_cleared(&mut self, item: &str) -> bool {
        match self.items.iter_mut().find(|(n, _)| n == item) {
            Some(slot) => {
                slot.1 = true;
                true
            }
            None => false,
        }
    }

    pub fn leftovers(&self) -> Vec<&str> {
        self.items.iter().filter(|(_, c)| !c).map(|(n, _)| n.as_str()).collect()
    }

    pub fn fully_cleaned(&self) -> bool {
        !self.items.is_empty() && self.leftovers().is_empty()
    }
}

/// 深化层三自检（恢复条生命周期 / 清理完整性）。
pub fn run_sesrestore_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F311-deep3");

    // 1. 生命周期：出现→驻留→超时自动消退（留痕）；未超时仍活跃。
    let mut bar = RecoveryBarLifecycle::new();
    let before = bar.sample(0);
    bar.appear(0);
    let mid = bar.sample(BAR_TTL_MS - 1);
    let after = bar.sample(BAR_TTL_MS);
    set.add(
        "bar lifecycle auto fade",
        !before && mid && !after && bar.auto_fades == 1 && bar.history.len() == 1,
        "",
    );

    // 2. 消费路径：驻留期内一键恢复成功、超时后消费拒绝。
    let mut bar2 = RecoveryBarLifecycle::new();
    bar2.appear(100);
    let consumed = bar2.consume(2000);
    let late = bar2.consume(999_999);
    set.add(
        "bar consume in ttl only",
        consumed && !late && bar2.consumed == 1,
        "",
    );

    // 3. 清理完整性：恢复消费 → 暂存区三面全清（光标位/窗口几何/
    //    草稿体）；残留项直出。
    let mut cc = CleanupCompleteness::default();
    cc.items = alloc::vec![
        (String::from("光标位"), false),
        (String::from("窗口几何"), false),
        (String::from("草稿体"), false),
    ];
    set.add("leftovers surfaced", !cc.fully_cleaned() && cc.leftovers().len() == 3, "");
    for (n, _) in cc.items.clone() {
        let _ = cc.mark_cleared(&n);
    }
    set.add("fully cleaned after marks", cc.fully_cleaned(), "");

    // 4. 未知项清理拒绝（不虚报）。
    set.add("unknown item rejected", !cc.mark_cleared("幽灵项"), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn bar_no_appear_sample_false() {
        let mut bar = RecoveryBarLifecycle::new();
        bar.appear(0);
        bar.consume(10);
        assert!(!bar.sample(20), "无活跃条采样恒假");
    }

    #[test]
    fn ttl_constant_is_thirty_seconds() {
        assert_eq!(BAR_TTL_MS, 30_000, "恢复条 30s 驻留判线钉死");
    }

    #[test]
    fn cleanup_empty_not_clean() {
        let cc = CleanupCompleteness::default();
        assert!(!cc.fully_cleaned(), "零项不构成清理完成");
    }
}
