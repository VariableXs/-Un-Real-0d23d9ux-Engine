//! VE-F3994 · S 域真实走查
//!
//! 走查：五角色真实用户走查执行（F3975 剧本转正式 + 录证采集 + 走查表
//! 终填 + 发现分级（阻断 / 重要 / 建议））。
//! 数据结构：走查执行册；录证库；发现分级表。
//! 错误路径：阻断发现→回 F3972 链路+复走；不可达任务→最高级立案；同
//! 意缺→禁录。
//! 性能：执行 O(剧本)；录证 O(会话)；分级 O(发现)。
//! 跨批对接：F3975/F3986 上游；F4000 宣告证据。
//! 无障碍与隐私：走查即域本色终验（真实用户双维复核）；录证全程知情
//! 同意 + 脱敏。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x4740）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x4740;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T14WCode(pub u16);

impl T14WCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const BLOCKER_FOUND: Self = Self(SEG_BASE + 1);
    pub const TASK_UNREACHABLE: Self = Self(SEG_BASE + 2);
    pub const CONSENT_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BLOCKER_FOUND => "阻断发现→回链路+复走",
            Self::TASK_UNREACHABLE => "不可达任务→最高级立案",
            Self::CONSENT_MISS => "同意缺→禁录",
            _ => "未知",
        }
    }
}

// ── 五角色走查执行册 ─────────────────────────────────────────────────

pub const WALK_ROLES: [&str; 5] = ["视障", "听障", "运动", "认知", "老年"];

/// 剧本任务：预演（F3975）→ 正式。
#[derive(Debug, Clone)]
pub struct ScriptTask {
    /// 任务名。
    pub name: String,
    /// 可达。
    pub reachable: bool,
    /// 正式执行完成。
    pub done: bool,
}

/// 单角色走查会话。
#[derive(Debug, Clone)]
pub struct WalkSession {
    /// 角色。
    pub role: &'static str,
    /// 知情同意（录证前置）。
    pub consent: bool,
    /// 录证已采。
    pub recorded: bool,
    /// 剧本任务（正式版）。
    pub tasks: Vec<ScriptTask>,
}

/// 走查执行册。
#[derive(Debug, Clone)]
pub struct WalkExecutionBook {
    pub sessions: Vec<WalkSession>,
}

impl WalkExecutionBook {
    pub fn new() -> Self {
        Self { sessions: Vec::new() }
    }

    /// 剧本转正式：五角色各一套任务副本。
    pub fn promote_script(&mut self, tasks: &[&str]) -> bool {
        if !self.sessions.is_empty() {
            return false;
        }
        for role in WALK_ROLES.iter() {
            self.sessions.push(WalkSession {
                role,
                consent: false,
                recorded: false,
                tasks: tasks.iter().map(|t| ScriptTask {
                    name: String::from(*t),
                    reachable: true,
                    done: false,
                }).collect(),
            });
        }
        true
    }

    /// 知情同意登记。
    pub fn consent(&mut self, role: &str, granted: bool) -> bool {
        match self.sessions.iter_mut().find(|s| s.role == role) {
            Some(s) => {
                s.consent = granted;
                true
            }
            None => false,
        }
    }

    /// 录证采集：同意缺 → 禁录。
    pub fn record(&mut self, role: &str) -> T14WCode {
        match self.sessions.iter_mut().find(|s| s.role == role) {
            Some(s) => {
                if !s.consent {
                    return T14WCode::CONSENT_MISS;
                }
                s.recorded = true;
                T14WCode::OK
            }
            None => T14WCode::TASK_UNREACHABLE,
        }
    }

    /// 任务执行：不可达 → 最高级立案。
    pub fn run_task(&mut self, role: &str, task: &str, success: bool) -> T14WCode {
        let s = match self.sessions.iter_mut().find(|s| s.role == role) {
            Some(s) => s,
            None => return T14WCode::TASK_UNREACHABLE,
        };
        let t = match s.tasks.iter_mut().find(|t| t.name == task) {
            Some(t) => t,
            None => return T14WCode::TASK_UNREACHABLE,
        };
        if !t.reachable {
            return T14WCode::TASK_UNREACHABLE;
        }
        t.done = success;
        T14WCode::OK
    }

    /// 走查表终填资格：五角色齐 + 全同意 + 全录证。
    pub fn finalize_ready(&self) -> bool {
        self.sessions.len() == 5
            && self.sessions.iter().all(|s| s.consent && s.recorded)
    }
}

// ── 发现分级 ─────────────────────────────────────────────────────────

/// 发现三级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 阻断。
    Blocker,
    /// 重要。
    Major,
    /// 建议。
    Minor,
}

/// 发现条目。
#[derive(Debug, Clone)]
pub struct Finding {
    pub role: &'static str,
    pub desc: String,
    pub sev: Severity,
    /// 复走完成（阻断发现处置后）。
    pub rewalked: bool,
}

/// 发现分级表。
#[derive(Debug, Clone)]
pub struct FindingBook {
    pub items: Vec<Finding>,
}

impl FindingBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn file(&mut self, role: &'static str, desc: &str, sev: Severity) {
        self.items.push(Finding {
            role,
            desc: String::from(desc),
            sev,
            rewalked: false,
        });
    }

    /// 阻断发现（回 F3972 链路对象）。
    pub fn blockers(&self) -> Vec<&Finding> {
        self.items.iter().filter(|f| f.sev == Severity::Blocker && !f.rewalked).collect()
    }

    /// 复走闭环。
    pub fn rewalk(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(f) => {
                f.rewalked = true;
                true
            }
            None => false,
        }
    }

    /// 分级齐活：零未复走阻断。
    pub fn clean(&self) -> bool {
        self.blockers().is_empty()
    }

    /// 三级计数。
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.items.iter().filter(|f| f.sev == Severity::Blocker).count(),
            self.items.iter().filter(|f| f.sev == Severity::Major).count(),
            self.items.iter().filter(|f| f.sev == Severity::Minor).count(),
        )
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves248_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves248_s10walk");

    // C3994-01 诊断码段不越窗。
    chk(&mut set, "C3994-01",
        T14WCode::CONSENT_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x4740, "err");

    // C3994-02 五角色表齐备。
    chk(&mut set, "C3994-02",
        WALK_ROLES.len() == 5 && WALK_ROLES[0] == "视障" && WALK_ROLES[4] == "老年", "err");

    // C3994-03 剧本转正式：五角色各一套。
    let mut b = WalkExecutionBook::new();
    chk(&mut set, "C3994-03",
        b.promote_script(&["开启读屏", "切高对比", "完成任务甲"])
            && b.sessions.len() == 5
            && b.sessions[0].tasks.len() == 3, "err");

    // C3994-04 二次转正式拒绝。
    chk(&mut set, "C3994-04", !b.promote_script(&["x"]), "err");

    // C3994-05 同意缺 → 禁录。
    chk(&mut set, "C3994-05",
        b.record("视障") == T14WCode::CONSENT_MISS
            && T14WCode::CONSENT_MISS.explain().contains("禁录"), "err");

    // C3994-06 同意后录证过。
    b.consent("视障", true);
    chk(&mut set, "C3994-06", b.record("视障") == T14WCode::OK, "err");

    // C3994-07 撤回同意后再录 → 禁录。
    b.consent("视障", false);
    chk(&mut set, "C3994-07", b.record("视障") == T14WCode::CONSENT_MISS, "err");
    b.consent("视障", true);
    let _ = b.record("视障");

    // C3994-08 任务执行成功。
    chk(&mut set, "C3994-08",
        b.run_task("视障", "开启读屏", true) == T14WCode::OK, "err");

    // C3994-09 不可达任务 → 最高级立案。
    if let Some(s) = b.sessions.iter_mut().find(|s| s.role == "听障") {
        s.tasks[2].reachable = false;
    }
    chk(&mut set, "C3994-09",
        b.run_task("听障", "完成任务甲", false) == T14WCode::TASK_UNREACHABLE, "err");

    // C3994-10 越名角色/任务 → 不可达码。
    chk(&mut set, "C3994-10",
        b.run_task("外星", "x", true) == T14WCode::TASK_UNREACHABLE
            && b.run_task("视障", "幽灵任务", true) == T14WCode::TASK_UNREACHABLE, "err");

    // C3994-11 终填资格：缺同意/录证 → 不可终填。
    chk(&mut set, "C3994-11", !b.finalize_ready(), "err");

    // C3994-12 全角色同意+录证 → 终填资格过。
    for role in WALK_ROLES.iter() {
        b.consent(role, true);
        b.record(role);
    }
    chk(&mut set, "C3994-12", b.finalize_ready(), "err");

    // C3994-13 发现分级：三级入册。
    let mut f = FindingBook::new();
    f.file("视障", "焦点环丢失", Severity::Blocker);
    f.file("认知", "引导文案冗长", Severity::Major);
    f.file("老年", "字号偏小", Severity::Minor);
    chk(&mut set, "C3994-13", f.counts() == (1, 1, 1), "err");

    // C3994-14 阻断未复走 → 不 clean。
    chk(&mut set, "C3994-14", !f.clean() && f.blockers().len() == 1, "err");

    // C3994-15 复走闭环 → clean。
    chk(&mut set, "C3994-15", f.rewalk(0) && f.clean(), "err");

    // C3994-16 复走越界拒绝。
    chk(&mut set, "C3994-16", !f.rewalk(99), "err");

    // C3994-17 阻断码语义。
    chk(&mut set, "C3994-17", T14WCode::BLOCKER_FOUND.explain().contains("复走"), "err");

    // C3994-18 三级枚举互异。
    chk(&mut set, "C3994-18",
        Severity::Blocker != Severity::Major
            && Severity::Major != Severity::Minor
            && Severity::Blocker != Severity::Minor, "err");

    // C3994-19 录证与同意解耦（同意可先于剧本执行）。
    let mut b2 = WalkExecutionBook::new();
    b2.promote_script(&["任务"]);
    b2.consent("老年", true);
    chk(&mut set, "C3994-19",
        b2.sessions[4].consent && !b2.sessions[4].recorded, "err");

    // C3994-20 错误码互异。
    let codes = [T14WCode::OK.0, T14WCode::BLOCKER_FOUND.0, T14WCode::TASK_UNREACHABLE.0,
        T14WCode::CONSENT_MISS.0];
    chk(&mut set, "C3994-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_gates_recording() {
        let mut b = WalkExecutionBook::new();
        b.promote_script(&["t"]);
        assert_eq!(b.record("视障"), T14WCode::CONSENT_MISS);
        b.consent("视障", true);
        assert_eq!(b.record("视障"), T14WCode::OK);
    }

    #[test]
    fn blocker_needs_rewalk() {
        let mut f = FindingBook::new();
        f.file("视障", "x", Severity::Blocker);
        assert!(!f.clean());
        f.rewalk(0);
        assert!(f.clean());
    }
}
