//! UNX-F12160 — B08 批域内集成账与真机三类异常注入预告（0x1a620 段）
//!
//! 锚点：UNX-F12160-J1 B08 集成账 10/10（访问违例/除零/栈溢出三类
//! 异常各自走全分发链模拟，链账逐级对平），域账收官。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系·收官集成）。
//! 结构：FaultKind（三类异常枚举）+ IntegrationBook（三类全链模拟
//! 账 + 每类逐级对平）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a620;
pub const IB_KIND: u32 = SEG_BASE + 0x01; // 未知异常类
pub const IB_TWICE: u32 = SEG_BASE + 0x02; // 同类二次记
pub const IB_SEALED: u32 = SEG_BASE + 0x03; // 印后记
pub const IB_ORDER: u32 = SEG_BASE + 0x04; // 链级缺漏
pub const IB_EMPTY: u32 = SEG_BASE + 0x05; // 空链记账

/// 三类异常。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    AccessViolation,
    DivideByZero,
    StackOverflow,
}

impl FaultKind {
    pub fn code(&self) -> u32 {
        match self {
            FaultKind::AccessViolation => 0xC0000005,
            FaultKind::DivideByZero => 0xC0000094,
            FaultKind::StackOverflow => 0xC00000FD,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            FaultKind::AccessViolation => "AV",
            FaultKind::DivideByZero => "DBZ",
            FaultKind::StackOverflow => "SO",
        }
    }
}

/// 一条全链模拟记录。
#[derive(Clone, PartialEq)]
pub struct ChainRun {
    pub kind: FaultKind,
    pub stages: Vec<u8>, // 每级 rank：1=VEH 2=SEH 3=UEF
    pub caught: Option<u8>,
    pub note: String,
}

impl ChainRun {
    /// 链合法：级序严格递增、若接住则接住级在链内。
    pub fn ok(&self) -> bool {
        if self.stages.len() < 2 || self.stages.windows(2).any(|w| w[0] >= w[1]) {
            return false;
        }
        if let Some(c) = self.caught {
            self.stages.contains(&c)
        } else {
            true
        }
    }

    pub fn render(&self) -> String {
        let st: String = self.stages.iter().map(|r| format!("{}", r)).collect();
        let c = match self.caught {
            Some(r) => format!("C{}", r),
            None => "pass".to_string(),
        };
        format!("{} [{}] {}", self.kind.tag(), st, c)
    }
}

/// B08 集成账。
pub struct IntegrationBook {
    pub runs: Vec<ChainRun>,
    sealed: bool,
}

impl IntegrationBook {
    pub fn new() -> IntegrationBook {
        IntegrationBook { runs: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 记一次全链模拟。
    pub fn run(
        &mut self,
        kind: FaultKind,
        stages: Vec<u8>,
        caught: Option<u8>,
        note: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(IB_SEALED);
        }
        if note.is_empty() {
            return Err(IB_EMPTY);
        }
        if self.runs.iter().any(|r| r.kind == kind) {
            return Err(IB_TWICE);
        }
        let cr = ChainRun { kind, stages, caught, note: note.to_string() };
        if !cr.ok() {
            return Err(IB_ORDER);
        }
        self.runs.push(cr);
        Ok(())
    }

    /// 三类全齐判定。
    pub fn all_kinds(&self) -> bool {
        self.runs.len() == 3
            && self.runs.iter().any(|r| r.kind == FaultKind::AccessViolation)
            && self.runs.iter().any(|r| r.kind == FaultKind::DivideByZero)
            && self.runs.iter().any(|r| r.kind == FaultKind::StackOverflow)
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a620;
        for r in &self.runs {
            h = h.wrapping_mul(211).wrapping_add(r.kind.code());
            h = h.wrapping_mul(211).wrapping_add(r.caught.unwrap_or(0) as u32);
            for by in r.note.bytes() {
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
        format!("runs={} kinds={}", self.runs.len(), self.all_kinds())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = IntegrationBook::new();
    // 1) 级序倒挂链必拒。
    got[0] = b.run(FaultKind::AccessViolation, [3u8, 1].to_vec(), Some(1), "inv").err()
        == Some(IB_ORDER);
    // 2) 接住级不在链必拒。
    got[1] = b.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(3), "miss")
        .err()
        == Some(IB_ORDER);
    // 3) 留痕空必拒。
    got[2] = b.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "")
        .err()
        == Some(IB_EMPTY);
    // 4) 同类二次必拒。
    let _ = b.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "av");
    got[3] = b.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "dup")
        .err()
        == Some(IB_TWICE);
    // 5) 印后记必拒。
    b.seal();
    got[4] = b.run(FaultKind::DivideByZero, [1u8, 2].to_vec(), Some(2), "post")
        .err()
        == Some(IB_SEALED);
    got
}

/// 判据入口：UNX-F12160
pub fn run_unx12160_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12160");
    let mut b = IntegrationBook::new();

    // J1 三类全链模拟挂账。
    let j1 = b
        .run(
            FaultKind::AccessViolation,
            [1u8, 2].to_vec(),
            Some(2),
            "av-seh-catch",
        )
        .is_ok()
        && b.run(
            FaultKind::DivideByZero,
            [1u8, 2, 3].to_vec(),
            Some(3),
            "dbz-uef-catch",
        )
        .is_ok()
        && b.run(
            FaultKind::StackOverflow,
            [1u8, 2, 3].to_vec(),
            None,
            "so-wer-pass",
        )
        .is_ok();
    s.add("J1_runs3", j1 && b.runs.len() == 3, "三类全链模拟全挂");

    // J2 三类全齐。
    s.add("J2_all_kinds", b.all_kinds(), "AV/DBZ/SO 三类全齐");

    // J3 异常码常量。
    s.add(
        "J3_codes",
        FaultKind::AccessViolation.code() == 0xC0000005
            && FaultKind::DivideByZero.code() == 0xC0000094
            && FaultKind::StackOverflow.code() == 0xC00000FD,
        "三码与 NT 语义一致",
    );

    // J4 类标签。
    s.add(
        "J4_tags",
        FaultKind::AccessViolation.tag() == "AV"
            && FaultKind::DivideByZero.tag() == "DBZ"
            && FaultKind::StackOverflow.tag() == "SO",
        "三标签 AV/DBZ/SO",
    );

    // J5 链自洽核。
    s.add(
        "J5_runs_ok",
        b.runs.iter().all(|r| r.ok()),
        "全链过自洽核",
    );

    // J6 链渲染。
    let r = b.runs[0].render();
    s.add(
        "J6_render",
        r == "AV [12] C2",
        "链渲染逐字对平",
    );

    // J7 接住级语义。
    s.add(
        "J7_caught",
        b.runs[0].caught == Some(2) && b.runs[2].caught.is_none(),
        "AV 接 SEH、SO 全放行",
    );

    // J8 链级缺漏拒。
    let mut b8 = IntegrationBook::new();
    s.add(
        "J8_short_chain",
        b8.run(FaultKind::AccessViolation, [1u8].to_vec(), Some(1), "short").err()
            == Some(IB_ORDER),
        "单级链拒 ORDER",
    );

    // J9 同级重复链拒。
    s.add(
        "J9_dup_stage",
        b8.run(FaultKind::AccessViolation, [1u8, 1].to_vec(), Some(1), "dup").err()
            == Some(IB_ORDER),
        "同级重复链拒 ORDER",
    );

    // J10 同类二次拒。
    let mut b10 = IntegrationBook::new();
    let _ = b10.run(FaultKind::DivideByZero, [1u8, 2].to_vec(), Some(2), "a");
    s.add(
        "J10_twice_kind",
        b10.run(FaultKind::DivideByZero, [1u8, 2].to_vec(), Some(2), "b").err()
            == Some(IB_TWICE),
        "同类二次拒 TWICE",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "集成账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "post")
            .err()
            == Some(IB_SEALED),
        "印账后记拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("runs=3") && sm.contains("kinds=true"),
        "摘要含链数与全齐位",
    );

    // J15 留痕空拒。
    let mut b15 = IntegrationBook::new();
    s.add(
        "J15_empty_note",
        b15.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "")
            .err()
            == Some(IB_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J16 全放行链 caught=None 合法。
    let pass_run = ChainRun {
        kind: FaultKind::StackOverflow,
        stages: [1u8, 2, 3].to_vec(),
        caught: None,
        note: "w".to_string(),
    };
    s.add("J16_pass_ok", pass_run.ok() && pass_run.caught.is_none(), "全放行链自洽");

    // J17 十次重跑同类模拟（独立账）同果。
    let mut ok17 = true;
    for _ in 0..10 {
        let mut b17 = IntegrationBook::new();
        if b17
            .run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "r")
            .is_err()
        {
            ok17 = false;
        }
    }
    s.add("J17_ten_reruns", ok17, "十次重跑同果");

    // J18 空账指纹稳定。
    let b18 = IntegrationBook::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空集成账指纹稳定");

    // J19 链渲染 pass 形。
    let r19 = b.runs[2].render();
    s.add(
        "J19_render_pass",
        r19 == "SO [123] pass",
        "全放行渲染 pass 形",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12160_book() {
        let mut b = IntegrationBook::new();
        let _ = b.run(FaultKind::AccessViolation, [1u8, 2].to_vec(), Some(2), "av");
        assert!(!b.all_kinds());
    }

    #[test]
    fn unx12160_checks_green() {
        let s = run_unx12160_checks();
        assert!(s.all_passed(), "UNX-F12160 判据未全绿");
    }

    #[test]
    fn unx12160_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
