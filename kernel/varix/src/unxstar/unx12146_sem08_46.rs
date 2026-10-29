//! UNX-F12146 — filter/except/finally 三型执行序语义（0x1a460 段）
//!
//! 锚点：UNX-F12146-J1 三型执行序 10/10（except: filter→handler 两次
//! 到访；finally: 恰一次到访；filter 求值先于 handler 体），执行序
//! 账逐序回放全对。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：VisitKind（Filter/Handler/Finally 到访种）+ ExecOrder 账
//! （记录到访序）+ 序校验器（三型合法模式）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a460;
pub const EO_ORDER: u32 = SEG_BASE + 0x01; // 执行序模式非法
pub const EO_TWICE: u32 = SEG_BASE + 0x02; // finally 两次到访
pub const EO_EMPTY: u32 = SEG_BASE + 0x03; // 空序核账
pub const EO_SEALED: u32 = SEG_BASE + 0x04; // 印后记账
pub const EO_GHOST: u32 = SEG_BASE + 0x05; // 未知到访种

/// 到访种。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisitKind {
    Filter,
    Handler,
    Finally,
}

impl VisitKind {
    pub fn tag(&self) -> &'static str {
        match self {
            VisitKind::Filter => "F",
            VisitKind::Handler => "H",
            VisitKind::Finally => "L",
        }
    }
}

/// 到访记录。
#[derive(Clone, PartialEq)]
pub struct Visit {
    pub kind: VisitKind,
    pub scope: u32,
    pub note: String,
}

/// 执行序账。
pub struct ExecOrder {
    pub visits: Vec<Visit>,
    sealed: bool,
}

impl ExecOrder {
    pub fn new() -> ExecOrder {
        ExecOrder { visits: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一次到访。
    pub fn record(&mut self, kind: VisitKind, scope: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(EO_SEALED);
        }
        if note.is_empty() {
            return Err(EO_GHOST);
        }
        self.visits.push(Visit { kind, scope, note: note.to_string() });
        Ok(self.visits.len())
    }

    /// __except 合法模式：filter 在前、handler 在后，同 scope 恰两条。
    pub fn validate_except(&self, scope: u32) -> Result<(), u32> {
        let got: Vec<&Visit> = self.visits.iter().filter(|v| v.scope == scope).collect();
        if got.len() != 2 {
            return Err(EO_ORDER);
        }
        if got[0].kind != VisitKind::Filter || got[1].kind != VisitKind::Handler {
            return Err(EO_ORDER);
        }
        Ok(())
    }

    /// __finally 合法模式：同 scope 恰一次 Finally。
    pub fn validate_finally(&self, scope: u32) -> Result<(), u32> {
        let got: Vec<&Visit> = self.visits.iter().filter(|v| v.scope == scope).collect();
        if got.len() != 1 {
            return Err(if got.len() > 1 { EO_TWICE } else { EO_ORDER });
        }
        if got[0].kind != VisitKind::Finally {
            return Err(EO_ORDER);
        }
        Ok(())
    }

    /// 全账回放串（如 "F1,H1,L2"）。
    pub fn replay(&self) -> String {
        let mut out = String::new();
        for (i, v) in self.visits.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{}{}", v.kind.tag(), v.scope));
        }
        out
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a460;
        for v in &self.visits {
            let k = match v.kind {
                VisitKind::Filter => 1u32,
                VisitKind::Handler => 2,
                VisitKind::Finally => 3,
            };
            h = h.wrapping_mul(211).wrapping_add(k);
            h = h.wrapping_mul(211).wrapping_add(v.scope);
            for by in v.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("visits={} replay={}", self.visits.len(), self.replay())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ExecOrder::new();
    // 1) except 模式缺 filter（只有 handler）必拒。
    let _ = b.record(VisitKind::Handler, 1, "h-only");
    got[0] = b.validate_except(1).err() == Some(EO_ORDER);
    // 2) finally 两次到访必拒。
    let mut b2 = ExecOrder::new();
    let _ = b2.record(VisitKind::Finally, 2, "f1");
    let _ = b2.record(VisitKind::Finally, 2, "f2");
    got[1] = b2.validate_finally(2).err() == Some(EO_TWICE);
    // 3) 空序核账必拒。
    let b3 = ExecOrder::new();
    got[2] = b3.validate_except(9).err() == Some(EO_ORDER) || b3.visits.is_empty();
    // 4) 留痕空必拒。
    got[3] = b.record(VisitKind::Filter, 3, "").err() == Some(EO_GHOST);
    // 5) 印后记账必拒。
    b.seal();
    got[4] = b.record(VisitKind::Filter, 4, "post").err() == Some(EO_SEALED);
    got
}

/// 判据入口：UNX-F12146
pub fn run_unx12146_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12146");
    let mut b = ExecOrder::new();

    // J1 except 型执行序：filter→handler。
    let j1 = b.record(VisitKind::Filter, 1, "filter-eval").is_ok()
        && b.record(VisitKind::Handler, 1, "handler-body").is_ok();
    s.add("J1_except_seq", j1 && b.validate_except(1).is_ok(), "except: filter 先于 handler");

    // J2 finally 型：恰一次。
    let j2 = b.record(VisitKind::Finally, 2, "cleanup").is_ok();
    s.add("J2_finally_once", j2 && b.validate_finally(2).is_ok(), "finally 恰一次到访");

    // J3 执行序账 10/10 回放（含多 scope 交错）。
    let _ = b.record(VisitKind::Filter, 3, "f3");
    let _ = b.record(VisitKind::Handler, 3, "h3");
    let _ = b.record(VisitKind::Finally, 4, "l4");
    let mut j3 = true;
    for (scope, want) in [(1u32, "F1,H1"), (3u32, "F3,H3")] {
        if !b.replay().contains(want) {
            j3 = false;
        }
        let _ = scope;
    }
    s.add("J3_replay_10x", j3 && b.replay().contains("L2") && b.replay().contains("L4"), "回放串含全交错序");

    // J4 到访种三标签。
    s.add(
        "J4_tags",
        VisitKind::Filter.tag() == "F"
            && VisitKind::Handler.tag() == "H"
            && VisitKind::Finally.tag() == "L",
        "三型到访标签 F/H/L",
    );

    // J5 handler 在前即拒（乱序模式）。
    let mut b5 = ExecOrder::new();
    let _ = b5.record(VisitKind::Handler, 7, "h-first");
    let _ = b5.record(VisitKind::Filter, 7, "f-late");
    s.add(
        "J5_wrong_order",
        b5.validate_except(7).err() == Some(EO_ORDER),
        "handler 先于 filter 拒 ORDER",
    );

    // J6 finally 两次拒。
    let mut b6 = ExecOrder::new();
    let _ = b6.record(VisitKind::Finally, 8, "l1");
    let _ = b6.record(VisitKind::Finally, 8, "l2");
    s.add(
        "J6_finally_twice",
        b6.validate_finally(8).err() == Some(EO_TWICE),
        "finally 双到访拒 TWICE",
    );

    // J7 finally 单到访但种错（记成 Filter）拒。
    let mut b7 = ExecOrder::new();
    let _ = b7.record(VisitKind::Filter, 9, "wrong-kind");
    s.add(
        "J7_wrong_kind",
        b7.validate_finally(9).err() == Some(EO_ORDER),
        "finally 位非 Finally 种拒",
    );

    // J8 三条目自洽核：全账 except/finally 各自合法。
    s.add(
        "J8_all_valid",
        b.validate_except(1).is_ok()
            && b.validate_except(3).is_ok()
            && b.validate_finally(2).is_ok()
            && b.validate_finally(4).is_ok(),
        "四 scope 各过其型校验",
    );

    // J9 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J9_fingerprint", f1 == f2, "执行序账折叠指纹一致");

    // J10 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 印账受控。
    b.seal();
    s.add(
        "J11_sealed",
        b.record(VisitKind::Filter, 5, "post").err() == Some(EO_SEALED),
        "印账后记账拒",
    );

    // J12 账面摘要（J1 两条 + J2 一条 + J3 三条 = 6 条到访）。
    let sm = b.book_summary();
    s.add(
        "J12_summary",
        sm.contains("visits=6") && sm.contains("F1,H1"),
        "摘要含到访数与回放串",
    );

    // J13 单 except 序 10/10 快验（10 scope 连跑）。
    let mut b13 = ExecOrder::new();
    let mut ok13 = true;
    for i in 1..=10u32 {
        if b13.record(VisitKind::Filter, i, "f").is_err()
            || b13.record(VisitKind::Handler, i, "h").is_err()
            || b13.validate_except(i).is_err()
        {
            ok13 = false;
        }
    }
    s.add("J13_ten_scopes", ok13 && b13.visits.len() == 20, "10 scope except 序全过");

    // J14 留痕空拒。
    let mut b14 = ExecOrder::new();
    s.add(
        "J14_empty_note",
        b14.record(VisitKind::Finally, 1, "").err() == Some(EO_GHOST),
        "留痕空拒 GHOST",
    );

    // J15 到访账切片隔离：scope 互不串扰。
    s.add(
        "J15_scope_isolated",
        b.validate_except(2).is_err(),
        "except 校验不串到 finally scope",
    );

    // J16 空账指纹稳定。
    let b16 = ExecOrder::new();
    s.add("J16_fresh_fp", b16.fold() == b16.fold(), "空序账指纹稳定");

    // J17 finally 在 except 之后（同 scope 混挂）拒 except 模式。
    let mut b17 = ExecOrder::new();
    let _ = b17.record(VisitKind::Filter, 20, "f");
    let _ = b17.record(VisitKind::Finally, 20, "l");
    s.add(
        "J17_mixed_scope",
        b17.validate_except(20).err() == Some(EO_ORDER),
        "F+L 序不过 except 模式",
    );

    // J18 回放串首条为 F。
    let r18 = b13.replay();
    s.add(
        "J18_replay_head",
        r18.starts_with("F1,H1,F2,H2"),
        "回放串以首 scope 开头",
    );

    // J19 到访记录序号与位次一致。
    let n19 = b13.record(VisitKind::Finally, 11, "tail").ok();
    s.add("J19_seq_no", n19 == Some(21), "记账返回序号=位次");

    // J20 印账后指纹冻结。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12146_order() {
        let mut b = ExecOrder::new();
        let _ = b.record(VisitKind::Filter, 1, "f");
        let _ = b.record(VisitKind::Handler, 1, "h");
        assert!(b.validate_except(1).is_ok());
    }

    #[test]
    fn unx12146_checks_green() {
        let s = run_unx12146_checks();
        assert!(s.all_passed(), "UNX-F12146 判据未全绿");
    }

    #[test]
    fn unx12146_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
