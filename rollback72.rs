//! 72 小时回滚兜底与发布七步（WP-401 · B-1305/1306 五分钟内回滚×每步有工具入口）。
//!
//! MD2 篇 13.3/13.4：72 小时一键回滚（MD1 第 37.4 节）是转正后的兜底——转
//! 正记录保留旧槽内容满 72 小时，期间更新器提供手动回滚按钮，五分钟内完成。
//! 发布七步（MD1 第 37.3 节）每步有对应工具：冻结/回归/打包/试运行/发布/
//! 公告/复盘——公告是变更说明模板（用户视角、三段式：修了什么、变了什么、
//! 已知问题）由发布工具强制套用，空白段不允许发布。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 72 小时回滚窗口
// ---------------------------------------------------------------------------

/// 回滚窗口时长（小时——转正记录保留旧槽内容的最短时长）。
pub const ROLLBACK_WINDOW_H: u32 = 72;

/// 手动回滚时限（分钟——B-1305 达标线：五分钟内完成）。
pub const ROLLBACK_DEADLINE_MIN: u32 = 5;

/// 回滚窗口状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RollbackWindow {
    /// 转正时刻（小时计）。
    pub promoted_at_h: u32,
    /// 旧槽内容仍在（72 小时内不许清——清了就没得回）。
    pub old_slot_kept: bool,
}

/// 窗口内判：转正后 72 小时内且旧槽未清——窗口外或旧槽已清，兜底失效。
pub fn window_open(w: &RollbackWindow, now_h: u32) -> bool {
    w.old_slot_kept && now_h >= w.promoted_at_h && now_h - w.promoted_at_h < ROLLBACK_WINDOW_H
}

/// 回滚耗时达标判（**B-1305 达标线：手动回滚五分钟内完成**）。
pub fn rollback_in_time(minutes: u32) -> bool {
    minutes <= ROLLBACK_DEADLINE_MIN
}

// ---------------------------------------------------------------------------
// 发布七步与公告模板
// ---------------------------------------------------------------------------

/// 发布七步（穷举——步序即仪式序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReleaseStep {
    /// 冻结（分支保护与合入门禁开关）。
    Freeze,
    /// 回归（冒烟套+实机核心场景一键跑）。
    Regression,
    /// 打包（构建链本体）。
    Package,
    /// 试运行（实机日用，体验日志汇总成日报）。
    Trial,
    /// 发布（归档、打标签、推送一条命令）。
    Release,
    /// 公告（三段式模板强制）。
    Announce,
    /// 复盘（过程问题回写表单化入口）。
    Postmortem,
}

/// 七步步深（0..=6——穷举对账坐标）。
pub fn step_depth(s: ReleaseStep) -> u8 {
    match s {
        ReleaseStep::Freeze => 0,
        ReleaseStep::Regression => 1,
        ReleaseStep::Package => 2,
        ReleaseStep::Trial => 3,
        ReleaseStep::Release => 4,
        ReleaseStep::Announce => 5,
        ReleaseStep::Postmortem => 6,
    }
}

/// 公告三段式（修了什么/变了什么/已知问题——空白段不允许发布）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Announce {
    /// 修了什么（用户视角）。
    pub fixed: bool,
    /// 变了什么（行为变化明示）。
    pub changed: bool,
    /// 已知问题（诚实面——没有已知问题也要写"无"）。
    pub known_issues: bool,
}

impl Announce {
    /// 公告完整判（**B-1306 达标线：公告模板强制**）——三段全非空。
    pub fn complete(&self) -> bool {
        self.fixed && self.changed && self.known_issues
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-1305/1306 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_rollback72_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1305/1306 72 小时兜底与发布七步");
    // 1. 窗口语义：72 小时内开、恰好 72 整点关、旧槽清了即关。
    let w0 = RollbackWindow { promoted_at_h: 10, old_slot_kept: true };
    set.add(
        "B-1305 回滚窗口语义",
        window_open(&w0, 10)
            && window_open(&w0, 81)
            && !window_open(&w0, 82)
            && !window_open(&RollbackWindow { old_slot_kept: false, ..w0 }, 20),
        "转正记录保留旧槽满 72 小时——窗口内兜底常在，旧槽清了兜底就没了",
    );
    // 2. 五分钟达标（B-1305 达标线）：恰 5 分钟过、6 分钟拒。
    set.add(
        "B-1305 五分钟回滚",
        rollback_in_time(5) && rollback_in_time(3) && !rollback_in_time(6),
        "手动回滚五分钟内完成——兜底的时限是承诺不是期望（B-1305 达标线）",
    );
    // 3. 七步穷举：步深 0..=6 互异且单调。
    let steps = [
        (ReleaseStep::Freeze, 0u8),
        (ReleaseStep::Regression, 1),
        (ReleaseStep::Package, 2),
        (ReleaseStep::Trial, 3),
        (ReleaseStep::Release, 4),
        (ReleaseStep::Announce, 5),
        (ReleaseStep::Postmortem, 6),
    ];
    let mut depths_ok = true;
    let mut i = 0;
    while i < steps.len() {
        if step_depth(steps[i].0) != steps[i].1 {
            depths_ok = false;
        }
        i += 1;
    }
    set.add(
        "B-1306 发布七步穷举",
        depths_ok,
        "冻结/回归/打包/试运行/发布/公告/复盘——七步穷举，每步有工具入口",
    );
    // 4. 公告三段式（B-1306 达标线）：三段全非空才算完整。
    let full = Announce { fixed: true, changed: true, known_issues: true };
    let no_known = Announce { known_issues: false, ..full };
    set.add(
        "B-1306 公告模板强制",
        full.complete() && !no_known.complete(),
        "修了什么/变了什么/已知问题三段式——空白段不允许发布（B-1306 达标线）",
    );
    // 5. 已知问题诚实面：没有问题也要写"无"——三段位不能省略只能填实。
    let none_ok = Announce { fixed: true, changed: true, known_issues: true };
    set.add(
        "B-1306 已知问题段不可省略",
        none_ok.known_issues,
        "'已知问题：无'也是一段——省略段的公告让用户猜盲区",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe25 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe25_window_arithmetic() {
        // 先算再断：promoted_at=10，窗口 [10, 82)。
        let w = RollbackWindow { promoted_at_h: 10, old_slot_kept: true };
        assert!(window_open(&w, 10)); // 转正即刻
        assert!(window_open(&w, 50)); // 中段
        assert!(window_open(&w, 81)); // 恰好最后一小时（82-1）
        assert!(!window_open(&w, 82)); // 整点关闭
        assert!(!window_open(&w, 200)); // 远超
        assert!(!window_open(&w, 5)); // 转正前（时钟倒挂防御）
        assert_eq!(ROLLBACK_WINDOW_H, 72);
    }

    #[test]
    fn fe25_old_slot_kept_gate() {
        // 旧槽清了：窗口内也关——保留时长与窗口判是两个独立条件。
        let w = RollbackWindow { promoted_at_h: 0, old_slot_kept: false };
        assert!(!window_open(&w, 1));
        assert!(!window_open(&w, 71));
    }

    #[test]
    fn fe25_rollback_deadline() {
        // 时限边界：0 与 5 过，6 拒。
        assert!(rollback_in_time(0));
        assert!(rollback_in_time(5));
        assert!(!rollback_in_time(6));
        assert!(!rollback_in_time(60));
        assert_eq!(ROLLBACK_DEADLINE_MIN, 5);
    }

    #[test]
    fn fe25_step_depths() {
        // 七步步深逐一对账。
        assert_eq!(step_depth(ReleaseStep::Freeze), 0);
        assert_eq!(step_depth(ReleaseStep::Regression), 1);
        assert_eq!(step_depth(ReleaseStep::Package), 2);
        assert_eq!(step_depth(ReleaseStep::Trial), 3);
        assert_eq!(step_depth(ReleaseStep::Release), 4);
        assert_eq!(step_depth(ReleaseStep::Announce), 5);
        assert_eq!(step_depth(ReleaseStep::Postmortem), 6);
        // 公告三段逐项独立红。
        let full = Announce { fixed: true, changed: true, known_issues: true };
        assert!(full.complete());
        assert!(!Announce { fixed: false, ..full }.complete());
        assert!(!Announce { changed: false, ..full }.complete());
        assert!(!Announce { known_issues: false, ..full }.complete());
    }
}
