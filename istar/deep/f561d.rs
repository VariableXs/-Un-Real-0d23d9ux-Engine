//! 深化层 · F561 快捷方式参数编辑（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F561 节）：
//! ①三字段的**分级校验状态机**——黄提示 = 警告不阻断（目标缺失/目录
//!   缺失/参数未引号），红 = 阻断保存（目标为空/引号奇数吞尾）；
//!   字段级结论汇成总级，保存按钮的 enable 判据唯一源；
//! ②**参数引号/空格转义正确性引擎**——含空格参数须整体引号包裹、
//!   引号必须成对（奇数引号会吞掉命令行尾部）、目标+参数的追加合成；
//! ③**可执行性三分账**——目标存在 + 工作目录有效 + 参数合法三路独立
//!   结论（基础层 [`ExecCheck`] 只出单结论，深化层拆三路分级）；
//! ④**恢复默认的快照对账**——逐字段还原凭证：动了哪几笔、还原后是否
//!   与打开时刻快照逐字段一致且 dirty 清零。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::lnkparams::{ExecCheck, LnkParams, RunStyle};

// ---------------------------------------------------------------------------
// ② 转义正确性引擎
// ---------------------------------------------------------------------------

/// 参数是否需要引号包裹（含空格/制表——不包裹会被拆成多个参数）。
pub fn needs_quoting(arg: &str) -> bool {
    arg.bytes().any(|b| b == b' ' || b == b'\t')
}

/// 是否已整体引号包裹（首尾成对引号）。
pub fn is_quoted(arg: &str) -> bool {
    arg.len() >= 2 && arg.starts_with('"') && arg.ends_with('"')
}

/// 引号平衡：参数串内引号成对。奇数引号 = 红（会吞掉命令行尾部参数）。
pub fn quotes_balanced(args: &str) -> bool {
    args.bytes().filter(|&b| b == b'"').count() % 2 == 0
}

/// 目标 + 参数的追加合成（「给游戏加 -windowed」的落点；参数空则直落目标）。
pub fn compose(target: &str, args: &str) -> alloc::string::String {
    if args.is_empty() {
        return alloc::string::String::from(target);
    }
    let mut s = alloc::string::String::from(target);
    s.push(' ');
    s.push_str(args);
    s
}

// ---------------------------------------------------------------------------
// ① 分级校验状态机 + ③ 三分账
// ---------------------------------------------------------------------------

/// 校验分级：黄 = 警告不阻断，红 = 阻断保存。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Pass,
    Warn,
    Block,
}

impl Severity {
    fn rank(self) -> u8 {
        match self {
            Severity::Pass => 0,
            Severity::Warn => 1,
            Severity::Block => 2,
        }
    }
}

/// 三字段分级账（每字段一结论 + 总级汇总）。
pub struct FieldAudit {
    pub target_sev: Severity,
    pub args_sev: Severity,
    pub workdir_sev: Severity,
}

impl FieldAudit {
    /// 对编辑器现状做分级评估（存在性结论取基础层宿主注入口）。
    pub fn evaluate(p: &LnkParams) -> FieldAudit {
        // 目标：空 = 红（没有目标必阻断）；存在性缺失 = 黄提示。
        let target_sev = if p.target.is_empty() {
            Severity::Block
        } else {
            match p.exec_check() {
                ExecCheck::TargetMissing => Severity::Warn,
                _ => Severity::Pass,
            }
        };
        // 参数：引号奇数 = 红（吞尾）；含空格未包裹 = 黄。
        let args_sev = if !quotes_balanced(&p.args) {
            Severity::Block
        } else if needs_quoting(&p.args) && !is_quoted(&p.args) {
            Severity::Warn
        } else {
            Severity::Pass
        };
        // 工作目录：空 = 可缺省（用目标所在目录）；缺失 = 黄提示。
        let workdir_sev = if p.workdir.is_empty() {
            Severity::Pass
        } else {
            match p.exec_check() {
                ExecCheck::WorkdirMissing => Severity::Warn,
                _ => Severity::Pass,
            }
        };
        FieldAudit { target_sev, args_sev, workdir_sev }
    }

    /// 总级：三路取最重。
    pub fn worst(&self) -> Severity {
        let mut w = self.target_sev;
        if self.args_sev.rank() > w.rank() {
            w = self.args_sev;
        }
        if self.workdir_sev.rank() > w.rank() {
            w = self.workdir_sev;
        }
        w
    }

    /// 红 = 阻断保存（保存按钮 disable 判据）。
    pub fn blocking(&self) -> bool {
        self.worst() == Severity::Block
    }
}

// ---------------------------------------------------------------------------
// ④ 恢复默认快照对账
// ---------------------------------------------------------------------------

/// 恢复默认对账凭证（动了哪几笔 + 是否全量还原）。
pub struct RestoreAudit {
    pub target_changed: bool,
    pub args_changed: bool,
    pub workdir_changed: bool,
    pub style_changed: bool,
}

impl RestoreAudit {
    /// 捕获快照态与现态的字段差异。
    pub fn capture(before: &LnkParams, after: &LnkParams) -> RestoreAudit {
        RestoreAudit {
            target_changed: before.target != after.target,
            args_changed: before.args != after.args,
            workdir_changed: before.workdir != after.workdir,
            style_changed: before.style != after.style,
        }
    }

    pub fn changed_fields(&self) -> usize {
        self.target_changed as usize
            + self.args_changed as usize
            + self.workdir_changed as usize
            + self.style_changed as usize
    }

    /// 还原完备：还原后四字段与快照逐字段一致且 dirty 清零。
    pub fn restored(&self, snapshot: &LnkParams, after_restore: &LnkParams) -> bool {
        after_restore.target == snapshot.target
            && after_restore.args == snapshot.args
            && after_restore.workdir == snapshot.workdir
            && after_restore.style == snapshot.style
            && !after_restore.dirty()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f561_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 引号平衡：成对过、奇数拒（吞尾红线）。
    cs.add(
        "quotes balanced parity",
        quotes_balanced("-a \"x y\"") && !quotes_balanced("-a \"x"),
        "",
    );

    // 2) 转义判定：含空格须引号；已包裹不重复告。
    cs.add(
        "quoting detection",
        needs_quoting("path with space") && !needs_quoting("-w") && is_quoted("\"a b\""),
        "",
    );

    // 3) 追加合成：目标 + 参数；参数空直落目标。
    let c1 = compose("C:\\g.exe", "-windowed");
    let c2 = compose("C:\\g.exe", "");
    cs.add(
        "compose appends args",
        c1 == "C:\\g.exe -windowed" && c2 == "C:\\g.exe",
        "",
    );

    // 4) 红级①：目标为空 → Block（阻断保存）。
    let e1 = LnkParams::open("", "-w", "C:\\", RunStyle::Normal);
    let a1 = FieldAudit::evaluate(&e1);
    cs.add(
        "empty target blocks",
        a1.target_sev == Severity::Block && a1.blocking(),
        "",
    );

    // 5) 红级②：奇数引号 → 参数 Block。
    let e2 = LnkParams::open("C:\\g.exe", "-a \"x", "", RunStyle::Normal);
    let a2 = FieldAudit::evaluate(&e2);
    cs.add(
        "odd quotes block",
        a2.args_sev == Severity::Block && a2.blocking() && a2.target_sev == Severity::Pass,
        "",
    );

    // 6) 黄级①：目标缺失（宿主注入）→ Warn 不阻断——黄提示，保存照走。
    let mut e3 = LnkParams::open("C:\\gone.exe", "-w", "", RunStyle::Normal);
    e3.note_fs(false, true);
    let a3 = FieldAudit::evaluate(&e3);
    cs.add(
        "missing target warns not blocks",
        a3.target_sev == Severity::Warn && a3.worst() == Severity::Warn && !a3.blocking(),
        "",
    );

    // 7) 黄级②：工作目录缺失（目录非空 + 宿主注入缺失）→ Warn；目标 Pass。
    let mut e4 = LnkParams::open("C:\\g.exe", "-w", "D:\\nowhere", RunStyle::Normal);
    e4.note_fs(true, false);
    let a4 = FieldAudit::evaluate(&e4);
    cs.add(
        "missing workdir warns target passes",
        a4.workdir_sev == Severity::Warn && a4.target_sev == Severity::Pass && !a4.blocking(),
        "",
    );

    // 8) 恢复默认对账：三笔改动逐一入账；还原后与快照逐字段一致、dirty 清零。
    let mut e5 = LnkParams::open("a.exe", "-v", "C:\\w", RunStyle::Minimized);
    let snap = LnkParams::open("a.exe", "-v", "C:\\w", RunStyle::Minimized);
    e5.set_args("-v -new");
    e5.set_workdir("D:\\z");
    e5.set_style(RunStyle::Maximized);
    let audit = RestoreAudit::capture(&snap, &e5);
    let changed = audit.changed_fields() == 3;
    e5.restore_default();
    cs.add(
        "restore audit full roundtrip",
        changed && audit.args_changed && audit.workdir_changed && audit.style_changed
            && audit.restored(&snap, &e5),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_takes_heaviest() {
        let a = FieldAudit {
            target_sev: Severity::Warn,
            args_sev: Severity::Block,
            workdir_sev: Severity::Pass,
        };
        assert_eq!(a.worst(), Severity::Block);
        let b = FieldAudit {
            target_sev: Severity::Pass,
            args_sev: Severity::Warn,
            workdir_sev: Severity::Pass,
        };
        assert_eq!(b.worst(), Severity::Warn);
        assert!(!b.blocking());
    }

    #[test]
    fn compose_keeps_quoted_spaces() {
        let s = compose("C:\\g.exe", "\"C:\\my games\\run.dll\" /s");
        assert_eq!(s, "C:\\g.exe \"C:\\my games\\run.dll\" /s");
        assert!(quotes_balanced(&s));
    }

    #[test]
    fn restore_style_only_roundtrip() {
        let mut e = LnkParams::open("a.exe", "-v", "", RunStyle::Normal);
        let snap = LnkParams::open("a.exe", "-v", "", RunStyle::Normal);
        e.set_style(RunStyle::Maximized);
        let audit = RestoreAudit::capture(&snap, &e);
        assert_eq!(audit.changed_fields(), 1);
        assert!(audit.style_changed && !audit.args_changed);
        e.restore_default();
        assert!(audit.restored(&snap, &e));
    }
}
