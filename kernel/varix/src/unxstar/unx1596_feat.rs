//! UNX-F1596 域级体验收官账（体验信号全闭环终证） · 0x79300段
//!
//! 判据原文（UNX-F1596-J1 摘录）：「体验收官账通过（F1558 信号全闭环复验：
//! 闭环率 100%），新信号注入 10/10 入流，终账归档」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 全闭环硬口径：闭环率按整数口径（闭环数×100/开流数），收官前置为
//!    闭环率恰 100——开流数非零且闭环数等于开流数，欠一笔都不收官；
//! 2. F1558 信号全闭环复验：基线八信号（F1558 钦定）逐笔开流逐笔闭环，
//!    复验闭环率 100% 才转收官相位；
//! 3. 新信号注入 10/10 入流：十笔新信号逐笔注入，逐笔必须被开流口收进
//!    流转面（入流零拒收），防体验面在收官前静默丢新信号；
//! 4. 相位机 流转中→已收官→已归档 单向，倒退显性拒；已收官后开流全拒
//!    （收官账封口），已归档后读写全拒；
//! 5. 终账归档：逐信号一行（号/类/态）+ 闭环率汇总，归档文本非空；
//! 6. 诊断码闭集 0x79301..0x7930C，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x79300;

/// F1558 基线信号数（体验收官账钦定）。
pub const BASELINE_COUNT: usize = 8;

/// 新信号注入种数（判据原文 10/10）。
pub const INFLOW_PROBES: usize = 10;

/// 信号种类名册（体验面钦定四类）。
pub const SIGNAL_KINDS: [&str; 4] = ["路径类", "延迟类", "反馈类", "卡顿类"];

/// 闭环率整数口径分母（百分制）。
pub const RATE_BASE: u64 = 100;

/// 留痕指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 留痕指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 体验收官账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 相位倒退（流转中→已收官→已归档 单向）。
    EPhase,
    /// 重复开流（信号号已用）。
    EDupId,
    /// 信号种类册外。
    EKind,
    /// 未开流先闭环。
    ENoEntry,
    /// 重复闭环。
    EDupClose,
    /// 已收官后开流拒。
    ESealed,
    /// 已归档后写拒。
    EArchived,
    /// 未全闭环先收官。
    ENotFull,
    /// 终账缺失（空文本不归档）。
    ENoReport,
    /// 空账收官拒。
    ENoSig,
    /// 闭环率口径非法（开流数为零时查率）。
    ERate,
    /// 终账行数与信号数不符（落账面失真）。
    ERows,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::EDupId => 0x02,
                Diag::EKind => 0x03,
                Diag::ENoEntry => 0x04,
                Diag::EDupClose => 0x05,
                Diag::ESealed => 0x06,
                Diag::EArchived => 0x07,
                Diag::ENotFull => 0x08,
                Diag::ENoReport => 0x09,
                Diag::ENoSig => 0x0a,
                Diag::ERate => 0x0b,
                Diag::ERows => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "相位倒退：流转中→已收官→已归档 单向，倒退即拒",
            Diag::EDupId => "重复开流：信号号已用，先查账面再开流",
            Diag::EKind => "信号种类册外：体验信号按名册四类入流，册外不收",
            Diag::ENoEntry => "未开流先闭环：信号号不在账面，先开流再闭环",
            Diag::EDupClose => "重复闭环：该信号已闭环，勿重复落账",
            Diag::ESealed => "已收官后开流拒：收官账已封口，新信号走下一轮体验账",
            Diag::EArchived => "已归档后写拒：终账已归档，改动走下一轮收官",
            Diag::ENotFull => "未全闭环先收官：闭环率必须 100%，逐笔闭环再收官",
            Diag::ENoReport => "终账缺失：先出逐信号终账再归档，空文本不收",
            Diag::ENoSig => "空账收官拒：无信号不构成体验账，先开流入流",
            Diag::ERate => "闭环率口径非法：开流数为零不构成率，先入流再查率",
            Diag::ERows => "终账行数失真：逐信号一行对齐账面，先修行数再归档",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::EDupId,
    Diag::EKind,
    Diag::ENoEntry,
    Diag::EDupClose,
    Diag::ESealed,
    Diag::EArchived,
    Diag::ENotFull,
    Diag::ENoReport,
    Diag::ENoSig,
    Diag::ERate,
    Diag::ERows,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79301..0x7930C 闭集内"
}

/// 体验账相位（流转中→已收官→已归档 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpPhase {
    /// 流转中：信号开流/闭环。
    Flowing,
    /// 已收官：闭环率 100%，账面封口。
    Closed,
    /// 已归档：终账落档封存。
    Archived,
}

impl ExpPhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            ExpPhase::Flowing => 0,
            ExpPhase::Closed => 1,
            ExpPhase::Archived => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ExpPhase::Flowing => "流转中",
            ExpPhase::Closed => "已收官",
            ExpPhase::Archived => "已归档",
        }
    }
}

/// 一笔体验信号（号 + 类 + 开流/闭环态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signal {
    /// 信号号（账内唯一）。
    pub id: u32,
    /// 信号种类（名册内）。
    pub kind: &'static str,
    /// 已闭环。
    pub closed: bool,
}

/// 体验收官账。
#[derive(Clone, Debug)]
pub struct ExperienceLedger {
    phase: ExpPhase,
    signals: Vec<Signal>,
    /// 结构性拒绝计数。
    rejects: u32,
    /// 累计入流笔数（新信号注入记账）。
    inflow: u32,
}

impl ExperienceLedger {
    /// 新体验账（流转中）。
    pub fn new() -> ExperienceLedger {
        ExperienceLedger {
            phase: ExpPhase::Flowing,
            signals: Vec::new(),
            rejects: 0,
            inflow: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> ExpPhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 累计入流笔数。
    pub fn inflow(&self) -> u32 {
        self.inflow
    }

    /// 账内信号数。
    pub fn len(&self) -> usize {
        self.signals.len()
    }

    /// 开流（种类册外/重号逐门拒；已收官/已归档后全拒）。
    pub fn open(&mut self, id: u32, kind: &'static str) -> Result<(), u32> {
        if self.phase == ExpPhase::Archived {
            self.rejects += 1;
            return Err(Diag::EArchived.val());
        }
        if self.phase == ExpPhase::Closed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if !SIGNAL_KINDS.iter().any(|k| *k == kind) {
            self.rejects += 1;
            return Err(Diag::EKind.val());
        }
        if self.signals.iter().any(|s| s.id == id) {
            self.rejects += 1;
            return Err(Diag::EDupId.val());
        }
        self.signals.push(Signal {
            id,
            kind,
            closed: false,
        });
        self.inflow += 1;
        Ok(())
    }

    /// 按号取信号。
    pub fn signal_of(&self, id: u32) -> Option<&Signal> {
        self.signals.iter().find(|s| s.id == id)
    }

    /// 闭环（未开流/重复闭环逐门拒）。
    pub fn close(&mut self, id: u32) -> Result<(), u32> {
        if self.phase == ExpPhase::Archived {
            self.rejects += 1;
            return Err(Diag::EArchived.val());
        }
        let slot = match self.signals.iter_mut().find(|s| s.id == id) {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(Diag::ENoEntry.val());
            }
        };
        if slot.closed {
            self.rejects += 1;
            return Err(Diag::EDupClose.val());
        }
        slot.closed = true;
        Ok(())
    }

    /// 已闭环笔数。
    pub fn closed_count(&self) -> usize {
        self.signals.iter().filter(|s| s.closed).count()
    }

    /// 闭环率（整数百分制；开流数为零显性拒——零账不构成率）。
    pub fn closure_rate(&self) -> Result<u64, u32> {
        if self.signals.is_empty() {
            return Err(Diag::ERate.val());
        }
        Ok((self.closed_count() as u64 * RATE_BASE) / self.len() as u64)
    }

    /// 收官（闭环率恰 100 才转已收官；空账/欠账显性拒）。
    pub fn finalize(&mut self) -> Result<(), u32> {
        if self.phase != ExpPhase::Flowing {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.signals.is_empty() {
            return Err(Diag::ENoSig.val());
        }
        match self.closure_rate() {
            Ok(rate) if rate == RATE_BASE => {}
            _ => return Err(Diag::ENotFull.val()),
        }
        self.phase = ExpPhase::Closed;
        Ok(())
    }

    /// 终账（逐信号一行 + 闭环率汇总；已收官后生成）。
    pub fn terminal(&self) -> Result<String, u32> {
        if self.phase == ExpPhase::Flowing {
            return Err(Diag::EPhase.val());
        }
        let mut out = String::new();
        for s in &self.signals {
            out.push_str(&format!(
                "信号{} {} {}\n",
                s.id,
                s.kind,
                if s.closed { "已闭环" } else { "未闭环" }
            ));
        }
        let rate = self.closure_rate().unwrap_or(0);
        out.push_str(&format!("闭环率{}/100 信号{}\n", rate, self.len()));
        Ok(out)
    }

    /// 归档（已收官才归档；终账非空且行数对齐才落档）。
    pub fn archive(&mut self) -> Result<String, u32> {
        if self.phase != ExpPhase::Closed {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let text = self.terminal()?;
        if text.is_empty() {
            return Err(Diag::ENoReport.val());
        }
        // 行数对齐：信号行数 + 汇总行。
        if text.matches('\n').count() != self.len() + 1 {
            return Err(Diag::ERows.val());
        }
        self.phase = ExpPhase::Archived;
        Ok(text)
    }
}

/// 信号留痕指纹（FNV-1a 整数折叠）。
pub fn signal_digest(id: u32, kind: &str) -> u64 {
    let mut h = DIGEST_SEED;
    for b in kind.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    h ^= id as u64;
    h.wrapping_mul(DIGEST_PRIME)
}

/// F1558 基线八信号名册（按号序开流闭环）。
pub const BASELINE_KINDS: [&str; BASELINE_COUNT] = [
    "路径类", "延迟类", "反馈类", "卡顿类",
    "路径类", "延迟类", "反馈类", "卡顿类",
];

/// 标准体验账：F1558 八信号开流并全闭环（流转中相位）。
pub fn build_ledger() -> ExperienceLedger {
    let mut ledger = ExperienceLedger::new();
    let mut i = 0usize;
    while i < BASELINE_COUNT {
        let kind = match BASELINE_KINDS.get(i) {
            Some(x) => *x,
            None => break,
        };
        let _ = ledger.open((i + 1) as u32, kind);
        i += 1;
    }
    let mut i = 1u32;
    while i <= BASELINE_COUNT as u32 {
        let _ = ledger.close(i);
        i += 1;
    }
    ledger
}

/// 新信号注入 10/10：十笔新信号逐笔注入标准账前置的空账，
/// 逐笔必须被开流口收进（入流零拒收且计数递增）。
/// 返回十布尔（true=该笔入流成功）。
pub fn inflow_matrix() -> [bool; INFLOW_PROBES] {
    let mut ledger = ExperienceLedger::new();
    let mut out = [false; INFLOW_PROBES];
    let mut i = 0usize;
    while i < INFLOW_PROBES {
        let kind = match SIGNAL_KINDS.get(i % SIGNAL_KINDS.len()) {
            Some(x) => *x,
            None => "",
        };
        let opened = ledger.open((100 + i) as u32, kind) == Ok(());
        let counted = ledger.inflow() == (i + 1) as u32;
        if let Some(cell) = out.get_mut(i) {
            *cell = opened && counted;
        }
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x79300
}

/// F1558 全闭环复验（八信号齐且闭环率恰 100）。
fn baseline_closed_ok(ledger: &ExperienceLedger) -> bool {
    if ledger.len() != BASELINE_COUNT {
        return false;
    }
    let mut i = 1u32;
    while i <= BASELINE_COUNT as u32 {
        let s = match ledger.signal_of(i) {
            Some(s) => s,
            None => return false,
        };
        if !s.closed {
            return false;
        }
        i += 1;
    }
    match ledger.closure_rate() {
        Ok(rate) => rate == RATE_BASE,
        Err(_) => false,
    }
}

/// 新信号注入 10/10 入流。
fn inflow_all_ok() -> bool {
    inflow_matrix() == [true; INFLOW_PROBES]
}

/// 双跑一致（同构输入两次构建终账一致且非空）。
fn dual_run_ok() -> bool {
    let mut a = build_ledger();
    let mut b = build_ledger();
    let _ = a.finalize();
    let ta = a.terminal();
    let _ = b.finalize();
    let tb = b.terminal();
    ta == tb && ta.is_ok()
}

/// 体验收官账判据：UNX-F1596 域级体验收官账。
pub fn run_unx1596_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1596");
    let mut ledger = build_ledger();

    s.add_dyn(
        "F1596-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // F1558 全闭环复验：闭环率恰 100。
    let rate = match ledger.closure_rate() {
        Ok(r) => r,
        Err(_) => 0,
    };
    s.add_dyn(
        "F1596-02 F1558 八信号全闭环",
        baseline_closed_ok(&ledger),
        &format!("rate={}/100 signals={}", rate, ledger.len()),
    );

    // 开流门：种类册外/重号逐门拒。
    let mut g = ExperienceLedger::new();
    let bad_kind = g.open(1, "册外类") == Err(Diag::EKind.val());
    let dup = g.open(1, "路径类") == Ok(())
        && g.open(1, "延迟类") == Err(Diag::EDupId.val());
    s.add_dyn(
        "F1596-03 开流册外与重号逐门拒",
        bad_kind && dup,
        &format!("rejects={}", g.rejects()),
    );

    // 闭环门：未开流先闭环/重复闭环逐门拒。
    let no_entry = g.close(99) == Err(Diag::ENoEntry.val());
    let _ = g.close(1);
    let dup_close = g.close(1) == Err(Diag::EDupClose.val());
    s.add_dyn(
        "F1596-04 闭环未开流与重闭逐门拒",
        no_entry && dup_close,
        &format!("rejects={}", g.rejects()),
    );

    // 新信号注入 10/10 入流。
    s.add_dyn(
        "F1596-05 新信号十笔全入流",
        inflow_all_ok(),
        &format!("matrix={:?}", inflow_matrix()),
    );

    // 欠账收官显性拒（闭环率不足 100）。
    let mut partial = ExperienceLedger::new();
    let _ = partial.open(1, "路径类");
    let _ = partial.open(2, "延迟类");
    let _ = partial.close(1);
    let partial_rej = partial.finalize() == Err(Diag::ENotFull.val())
        && partial.phase() == ExpPhase::Flowing;
    s.add_dyn(
        "F1596-06 欠账收官显性拒",
        partial_rej,
        &format!("rate={}", partial.closure_rate().unwrap_or(0)),
    );

    // 空账收官显性拒、零账查率显性拒。
    let mut empty = ExperienceLedger::new();
    let empty_rej = empty.finalize() == Err(Diag::ENoSig.val());
    let rate_rej = empty.closure_rate() == Err(Diag::ERate.val());
    s.add_dyn(
        "F1596-07 空账收官与零账查率显性拒",
        empty_rej && rate_rej,
        &format!("empty_phase={}", empty.phase().name()),
    );

    // 收官：闭环率 100 转已收官；已收官后开流显性拒。
    let finalize_ok = ledger.finalize() == Ok(()) && ledger.phase() == ExpPhase::Closed;
    let sealed = ledger.open(200, "路径类") == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1596-08 收官转相与封口开流拒",
        finalize_ok && sealed && ledger.phase().ord() == 1,
        &format!("phase={}", ledger.phase().name()),
    );

    // 终账：已收官后生成非空且行数对齐（八行 + 汇总行）。
    let term = ledger.terminal();
    let term_ok = match &term {
        Ok(t) => !t.is_empty() && t.contains("已闭环") && t.contains("闭环率100/100"),
        Err(_) => false,
    };
    s.add_dyn(
        "F1596-09 终账逐信号落账非空",
        term_ok,
        &format!("bytes={}", term.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 归档：终账行数对齐才落档；已归档后写全拒。
    let archived = ledger.archive();
    let arch_ok = match &archived {
        Ok(t) => !t.is_empty(),
        Err(_) => false,
    };
    let after_open = ledger.open(300, "路径类") == Err(Diag::EArchived.val());
    let after_close = ledger.close(1) == Err(Diag::EArchived.val());
    s.add_dyn(
        "F1596-10 终账归档与已归档写全拒",
        arch_ok && after_open && after_close && ledger.phase().ord() == 2,
        &format!("phase_ord={}", ledger.phase().ord()),
    );

    // 相位倒退显性拒（归档后再收官）。
    let back = ledger.finalize() == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1596-11 相位倒退显性拒",
        back,
        &format!("back_rejected={}", back),
    );

    // 留痕指纹稳定（同号同类指纹一致、异类指纹异）。
    let d1 = signal_digest(1, "路径类");
    let d2 = signal_digest(1, "路径类");
    let d3 = signal_digest(1, "延迟类");
    s.add_dyn(
        "F1596-12 留痕指纹稳定非零",
        d1 == d2 && d1 != d3 && d1 != 0,
        &format!("digest_nonzero={}", d1 != 0),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1596-13 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::ERows.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ERows.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79300);
    }

    #[test]
    fn open_gates() {
        let mut l = ExperienceLedger::new();
        assert_eq!(l.open(1, "册外类"), Err(Diag::EKind.val()));
        assert_eq!(l.open(1, "路径类"), Ok(()));
        assert_eq!(l.open(1, "延迟类"), Err(Diag::EDupId.val()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.inflow(), 1);
    }

    #[test]
    fn close_gates() {
        let mut l = ExperienceLedger::new();
        assert_eq!(l.close(7), Err(Diag::ENoEntry.val()));
        let _ = l.open(7, "反馈类");
        assert_eq!(l.close(7), Ok(()));
        assert_eq!(l.close(7), Err(Diag::EDupClose.val()));
        assert_eq!(l.closed_count(), 1);
    }

    #[test]
    fn closure_rate_integer() {
        let mut l = ExperienceLedger::new();
        assert_eq!(l.closure_rate(), Err(Diag::ERate.val()));
        let _ = l.open(1, "路径类");
        let _ = l.open(2, "延迟类");
        assert_eq!(l.closure_rate(), Ok(0));
        let _ = l.close(1);
        assert_eq!(l.closure_rate(), Ok(50));
        let _ = l.close(2);
        assert_eq!(l.closure_rate(), Ok(100));
    }

    #[test]
    fn inflow_ten_of_ten() {
        assert_eq!(inflow_matrix(), [true; INFLOW_PROBES]);
    }

    #[test]
    fn baseline_f1558_all_closed() {
        let l = build_ledger();
        assert_eq!(l.len(), BASELINE_COUNT);
        assert_eq!(l.closed_count(), BASELINE_COUNT);
        assert!(baseline_closed_ok(&l));
    }

    #[test]
    fn finalize_requires_full_closure() {
        let mut partial = ExperienceLedger::new();
        let _ = partial.open(1, "路径类");
        let _ = partial.open(2, "延迟类");
        let _ = partial.close(1);
        assert_eq!(partial.finalize(), Err(Diag::ENotFull.val()));
        let _ = partial.close(2);
        assert_eq!(partial.finalize(), Ok(()));
        assert_eq!(partial.phase(), ExpPhase::Closed);
    }

    #[test]
    fn empty_gates() {
        let mut l = ExperienceLedger::new();
        assert_eq!(l.finalize(), Err(Diag::ENoSig.val()));
        assert_eq!(l.closure_rate(), Err(Diag::ERate.val()));
        assert_eq!(l.phase(), ExpPhase::Flowing);
    }

    #[test]
    fn phase_monotonic_and_sealed() {
        let mut l = build_ledger();
        assert_eq!(l.phase(), ExpPhase::Flowing);
        assert_eq!(l.finalize(), Ok(()));
        assert_eq!(l.phase(), ExpPhase::Closed);
        assert_eq!(l.open(200, "路径类"), Err(Diag::ESealed.val()));
        assert_eq!(l.finalize(), Err(Diag::EPhase.val()));
        assert!(l.archive().is_ok());
        assert_eq!(l.phase(), ExpPhase::Archived);
        assert_eq!(l.phase().ord(), 2);
    }

    #[test]
    fn archive_rows_aligned() {
        let mut l = build_ledger();
        let _ = l.finalize();
        let term = l.terminal();
        assert!(term.is_ok());
        let archived = l.archive();
        assert!(archived.is_ok());
        assert_eq!(l.phase(), ExpPhase::Archived);
        assert_eq!(l.open(300, "路径类"), Err(Diag::EArchived.val()));
        assert_eq!(l.close(1), Err(Diag::EArchived.val()));
        assert_eq!(l.finalize(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn terminal_before_close_rejected() {
        let l = build_ledger();
        assert_eq!(l.terminal(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn signal_digest_stable() {
        let a = signal_digest(3, "卡顿类");
        let b = signal_digest(3, "卡顿类");
        let c = signal_digest(4, "卡顿类");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1596_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
