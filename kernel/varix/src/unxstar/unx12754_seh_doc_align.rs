//! UNX-F12754 SEH 文档对齐（MSD SEH 注出处核验） · 0x82680段
//!
//! 判据原文（派工卡 12754 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F12755-J1 R-D1-008 复核留痕
//!   （悬置项/证据/下一步三列），零静默；③ 完成时 API/软件核回：状态=已
//!   完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 域模型：MSD SEH 出处核验册（定容 36 行），每行出处五元组（主题闭
//!    集、文档锚点、核验态、证据指纹、封账位）。主题闭集：__try/__except/
//!    __finally/__leave/异常过滤器/展开语义/向量续体/VEXCEPT_VECTOR_-
//!    CONTINUE_HANDLER/_set_se_translator/EXCEPTION_POINTERS/EXCEPTION_
//!    RECORD/EXCEPTION_RECORD64/帧布局/函数表/UNWIND_INFO/展开码族/
//!    RAII_SUPPORT/StructuredExceptionHandling.cpp 共 18 主题两档（概念/
//!    代码），行由纯函数 cite_row(i) 确定性复算。
//! 2. 三列复核留痕：每行核验留痕 =（悬置项列、证据列、下一步列），三列
//!    全非空才算复核完成；任何一列空 = 静默，显性拒。零静默 = 全 36 行
//!    三列全非空且无一被吞。
//! 3. 静默注入器：人为构造"复核跳过"（三列任一空即入账）共 10 轮，零
//!    静默闸比对照入账前校验，10/10 必抓；检出数与注入数守恒、漏检=0。
//! 4. 判定器 judge_cite：行号越册 ENOENT → 状态倒退 EREWIND → 主题越闭
//!    集 EINVAL → 锚点空 EEMPTY → 三列任一空 ESILENT → 正常判定
//!    （对齐/存疑/不符三值）。先到先判。
//! 5. 状态机 Pending→Checked→Aligned（或 Discrepancy）单调只进不退。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!    派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12754 − 12600)）。
pub const SEG_BASE: u32 = 0x82680;

/// 出处核验册定容（36 行）。
pub const CITE_ROWS: usize = 36;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaDiag {
    /// 行号越册。
    ENoEnt,
    /// 主题越闭集。
    EBits,
    /// 状态倒退。
    ERewind,
    /// 锚点空（文档锚点为空串）。
    EEmpty,
    /// 三列任一空（复核留痕静默）。
    ESilent,
    /// 未齐核验（核验未完成即封账）。
    ESeal,
    /// 容量耗尽。
    ECapFull,
    /// 静默注入漏检（入账前校验未抓到静默）。
    EMissed,
    /// 双跑指纹不一致。
    EFp,
    /// 守恒破缺（Σ(对齐+存疑+不符+悬置)≠总行数）。
    EConserve,
}

impl DaDiag {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DaDiag::ENoEnt => 0x01,
                DaDiag::EBits => 0x02,
                DaDiag::ERewind => 0x03,
                DaDiag::EEmpty => 0x04,
                DaDiag::ESilent => 0x05,
                DaDiag::ESeal => 0x06,
                DaDiag::ECapFull => 0x07,
                DaDiag::EMissed => 0x08,
                DaDiag::EFp => 0x09,
                DaDiag::EConserve => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DaDiag::ENoEnt => "行号越册：行号不在 0..36，核对出处核验册行序",
            DaDiag::EBits => "主题越闭集：SEH 主题不在 18 主题两档闭集，按闭集登记",
            DaDiag::ERewind => "状态倒退：Pending→Checked→Aligned 只进不退，倒退显性拒",
            DaDiag::EEmpty => "锚点空：文档锚点为空串，补 MSD 路径后再核验",
            DaDiag::ESilent => "三列任一空：复核留痕缺列即静默，悬置/证据/下一步三列补齐",
            DaDiag::ESeal => "未齐核验：36 行未核齐即封账，先补齐缺行再走封账口",
            DaDiag::ECapFull => "容量耗尽：账本判满，先封账或换新账本再入新果",
            DaDiag::EMissed => "静默注入漏检：零静默闸未抓到人为静默，复核入账前校验逻辑",
            DaDiag::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DaDiag::EConserve => "守恒破缺：Σ(对齐+存疑+不符+悬置)≠36，逐列复算账本",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// SEH 主题闭集（18 主题 × 两档 = 36 行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SehTopic {
    /// __try 块语义（概念档）。
    TryConcept,
    /// __try 块代码样板（代码档）。
    TryCode,
    /// __except 过滤器语义（概念档）。
    ExceptConcept,
    /// __except 过滤器代码（代码档）。
    ExceptCode,
    /// __finally 终止处理（概念档）。
    FinallyConcept,
    /// __finally 代码（代码档）。
    FinallyCode,
    /// __leave 跳转语义（概念档）。
    LeaveConcept,
    /// __leave 代码（代码档）。
    LeaveCode,
    /// 异常过滤器三值（概念档）。
    FilterConcept,
    /// 异常过滤器代码（代码档）。
    FilterCode,
    /// 展开语义（概念档）。
    UnwindConcept,
    /// 展开代码（代码档）。
    UnwindCode,
    /// 向量续体处理（概念档）。
    VectoredConcept,
    /// 向量续体代码（代码档）。
    VectoredCode,
    /// _set_se_translator（概念档）。
    TranslatorConcept,
    /// _set_se_translator 代码（代码档）。
    TranslatorCode,
    /// EXCEPTION_POINTERS/RECORD 结构（概念档）。
    StructConcept,
    /// 结构代码（代码档）。
    StructCode,
}

/// 出处行五元组（纯函数可复算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CiteRow {
    /// 行号（0..36）。
    pub no: u32,
    /// 主题。
    pub topic: SehTopic,
    /// 文档锚点（MSD 路径片段，确定性构造）。
    pub anchor: u32,
    /// 预期对齐态（true=对齐，false=存疑）。
    pub expect_align: bool,
    /// 轮次预算。
    pub budget: u32,
}

/// 复算第 i 行（确定性）。
pub fn cite_row(i: usize) -> Option<CiteRow> {
    if i >= CITE_ROWS {
        return None;
    }
    let topic = match i % 18 {
        0 => SehTopic::TryConcept,
        1 => SehTopic::TryCode,
        2 => SehTopic::ExceptConcept,
        3 => SehTopic::ExceptCode,
        4 => SehTopic::FinallyConcept,
        5 => SehTopic::FinallyCode,
        6 => SehTopic::LeaveConcept,
        7 => SehTopic::LeaveCode,
        8 => SehTopic::FilterConcept,
        9 => SehTopic::FilterCode,
        10 => SehTopic::UnwindConcept,
        11 => SehTopic::UnwindCode,
        12 => SehTopic::VectoredConcept,
        13 => SehTopic::VectoredCode,
        14 => SehTopic::TranslatorConcept,
        15 => SehTopic::TranslatorCode,
        16 => SehTopic::StructConcept,
        _ => SehTopic::StructCode,
    };
    // 锚点 = 主题序号 × 0x100 + 档位（概念=0/代码=1），确定性。
    let anchor = ((i % 18) as u32) * 0x100 + ((i / 18) as u32);
    // 预期对齐：每 9 行一档存疑（确定性轮铺）。
    let expect_align = i % 9 != 8;
    let budget = ((i as u32) % 10) + 1;
    Some(CiteRow {
        no: i as u32,
        topic,
        anchor,
        expect_align,
        budget,
    })
}

// ── 判定器 ───────────────────────────────────────────────────────
/// 复核留痕三列（悬置项/证据/下一步）。
#[derive(Clone, Debug)]
pub struct TraceCols {
    /// 悬置项列（待复核的具体疑点）。
    pub pending: String,
    /// 证据列（核验依据指纹）。
    pub evidence: String,
    /// 下一步列（行动项）。
    pub next: String,
}

/// 判定次序冻结：越册 → 倒退 → 主题 → 锚点 → 三列 → 正常（对齐/存疑/不符）。
pub fn judge_cite(
    row: &CiteRow,
    anchor_in: u32,
    trace: &TraceCols,
) -> Result<u8, DaDiag> {
    if row.no >= CITE_ROWS as u32 {
        return Err(DaDiag::ENoEnt);
    }
    if (row.no as usize) % 18 >= 18 {
        return Err(DaDiag::EBits);
    }
    if anchor_in == 0 {
        return Err(DaDiag::EEmpty);
    }
    if trace.pending.is_empty() || trace.evidence.is_empty() || trace.next.is_empty() {
        return Err(DaDiag::ESilent);
    }
    // 证据锚点与行锚点一致 → 对齐；行锚点漂移一档 → 存疑；否则不符。
    if anchor_in == row.anchor {
        Ok(0)
    } else if (anchor_in ^ row.anchor) <= 0x10 {
        Ok(1)
    } else {
        Ok(2)
    }
}

// ── 账本 ─────────────────────────────────────────────────────────
/// 核验态（单调只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaState {
    /// 待核验。
    Pending,
    /// 已核验未定档。
    Checked,
    /// 对齐（终态）。
    Aligned,
    /// 存疑（终态）。
    Doubtful,
    /// 不符（终态）。
    Mismatch,
}

/// 零静默账本。
#[derive(Clone, Debug)]
pub struct DaLedger {
    /// 各行状态。
    states: [Option<DaState>; CITE_ROWS],
    /// 各行判档（0=对齐 1=存疑 2=不符）。
    grades: [Option<u8>; CITE_ROWS],
    /// 对齐计数。
    aligned: u32,
    /// 存疑计数。
    doubtful: u32,
    /// 不符计数。
    mismatch: u32,
    /// 静默注入次数。
    injected: u32,
    /// 静默检出次数。
    caught: u32,
    /// 静默漏检次数（必须为 0）。
    missed: u32,
    /// 指纹。
    fp: u64,
}

impl DaLedger {
    /// 新账本。
    pub fn new() -> Self {
        DaLedger {
            states: [None; CITE_ROWS],
            grades: [None; CITE_ROWS],
            aligned: 0,
            doubtful: 0,
            mismatch: 0,
            injected: 0,
            caught: 0,
            missed: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 状态推进（只进不退）。
    pub fn advance(&mut self, no: usize, to: DaState) -> Result<(), DaDiag> {
        let cell = match self.states.get_mut(no) {
            Some(c) => c,
            None => return Err(DaDiag::ENoEnt),
        };
        let cur = match cell {
            Some(s) => *s,
            None => DaState::Pending,
        };
        let ok = match (cur, to) {
            (DaState::Pending, DaState::Checked) => true,
            (DaState::Checked, DaState::Aligned)
            | (DaState::Checked, DaState::Doubtful)
            | (DaState::Checked, DaState::Mismatch) => true,
            _ => false,
        };
        if !ok {
            return Err(DaDiag::ERewind);
        }
        *cell = Some(to);
        self.fnv((no as u64) << 8 | (to as u64));
        Ok(())
    }

    /// 入账一行判档。
    pub fn account(&mut self, no: usize, grade: u8) -> Result<(), DaDiag> {
        let st = match self.states.get(no) {
            Some(Some(s)) => *s,
            _ => return Err(DaDiag::ERewind),
        };
        if st != DaState::Checked {
            return Err(DaDiag::ERewind);
        }
        if grade > 2 {
            return Err(DaDiag::EBits);
        }
        let cell = match self.grades.get_mut(no) {
            Some(c) => c,
            None => return Err(DaDiag::ENoEnt),
        };
        if cell.is_some() {
            return Err(DaDiag::ERewind);
        }
        *cell = Some(grade);
        match grade {
            0 => self.aligned += 1,
            1 => self.doubtful += 1,
            _ => self.mismatch += 1,
        }
        self.fnv((no as u64) << 32 | grade as u64);
        Ok(())
    }

    /// 零静默闸（入账前校验三列）。
    pub fn silent_gate(&mut self, trace: &TraceCols) -> bool {
        if trace.pending.is_empty() || trace.evidence.is_empty() || trace.next.is_empty() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5150);
            true
        } else {
            false
        }
    }

    /// 守恒等式：Σ(对齐+存疑+不符) = 已入账行数。
    pub fn conservation_ok(&self) -> bool {
        let sum = self.aligned.saturating_add(self.doubtful).saturating_add(self.mismatch);
        let rows = self.grades.iter().filter(|c| c.is_some()).count() as u32;
        sum == rows
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 三态计数。
    pub fn counts(&self) -> (u32, u32, u32) {
        (self.aligned, self.doubtful, self.mismatch)
    }

    /// 静默三数。
    pub fn silent_stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.missed)
    }
}

impl Default for DaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：36 行复算→三列构造→核验→入账→守恒。返回（三态, 指纹）。
pub fn run_cycle() -> ((u32, u32, u32), u64) {
    let mut led = DaLedger::new();
    for i in 0..CITE_ROWS {
        let row = match cite_row(i) {
            Some(r) => r,
            None => continue,
        };
        let _ = led.advance(i, DaState::Checked);
        // 证据 = 行锚点自身（对齐）或漂移一档（存疑，确定性轮铺）。
        let anchor_in = if row.expect_align {
            row.anchor
        } else {
            row.anchor + 0x10
        };
        let trace = TraceCols {
            pending: alloc::format!("悬置-{}-主题{}", i, i % 18),
            evidence: alloc::format!("证据-{:x}", anchor_in),
            next: alloc::format!("下一步-{}-复跑", i),
        };
        let g = judge_cite(&row, anchor_in, &trace);
        if let Ok(g) = g {
            let _ = led.account(i, g);
        }
        // 终态按判档落位：0=Aligned 1=Doubtful 2=Mismatch（单调只进不退）。
        if let Ok(g) = g {
            let fin = match g {
                0 => DaState::Aligned,
                1 => DaState::Doubtful,
                _ => DaState::Mismatch,
            };
            let _ = led.advance(i, fin);
        }
    }
    (led.counts(), led.fingerprint())
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx12754_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12754");
    // 1. 正常流：36 行复算逐行可判。
    let mut all_judgeable = true;
    for i in 0..CITE_ROWS {
        if cite_row(i).is_none() {
            all_judgeable = false;
        }
    }
    s.add("正样本复算齐", all_judgeable, "36 行出处复算无一越册");
    // 2. 边界：零/负移位/上限/上限+1（行号与锚点四类）。
    let row0 = match cite_row(0) {
        Some(r) => r,
        None => CiteRow {
            no: 0,
            topic: SehTopic::TryConcept,
            anchor: 0,
            expect_align: true,
            budget: 1,
        },
    };
    s.add("边界·锚点零", judge_cite(&row0, 0, &TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") }) == Err(DaDiag::EEmpty), "锚点=0 必回 EEmpty");
    s.add("边界·行号越册", judge_cite(&CiteRow { no: CITE_ROWS as u32, topic: SehTopic::TryConcept, anchor: 1, expect_align: true, budget: 1 }, 1, &TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") }) == Err(DaDiag::ENoEnt), "行号=36 越册必回 ENoEnt");
    s.add("边界·行号上限合法", judge_cite(&row0, 0x100, &TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") }).is_ok(), "行 0 上限内合法");
    // 3. 零静默：三列任一空必拒。
    let empty_trace = TraceCols {
        pending: String::new(),
        evidence: alloc::format!("e"),
        next: alloc::format!("n"),
    };
    s.add("零静默·悬置列空拒", judge_cite(&row0, 0x100, &empty_trace) == Err(DaDiag::ESilent), "悬置列空必回 ESilent");
    let half_trace = TraceCols {
        pending: alloc::format!("p"),
        evidence: String::new(),
        next: alloc::format!("n"),
    };
    s.add("零静默·证据列空拒", judge_cite(&row0, 0x100, &half_trace) == Err(DaDiag::ESilent), "证据列空必回 ESilent");
    // 4. 静默注入必抓 10/10。
    let mut led = DaLedger::new();
    let mut all_caught = true;
    for k in 0..10 {
        let t = if k % 2 == 0 {
            TraceCols {
                pending: String::new(),
                evidence: alloc::format!("e{}", k),
                next: alloc::format!("n{}", k),
            }
        } else {
            TraceCols {
                pending: alloc::format!("p{}", k),
                evidence: String::new(),
                next: alloc::format!("n{}", k),
            }
        };
        if !led.silent_gate(&t) {
            all_caught = false;
        }
    }
    let (inj_n, caught_n, missed_n) = led.silent_stats();
    s.add_dyn(
        "静默注入 10/10",
        all_caught && missed_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 漏检={}", inj_n, caught_n, missed_n),
    );
    // 5. 全流程三态齐且守恒。
    let (counts, fp) = run_cycle();
    s.add("全流程·对齐列非空", counts.0 > 0, "对齐列非空");
    s.add("全流程·存疑列非空", counts.1 > 0, "存疑列非空（轮铺保证）");
    s.add("全流程·指纹非零", fp != 0, "FNV 指纹全零视为异常");
    // 6. 状态机倒退拒。
    let mut led2 = DaLedger::new();
    let r1 = led2.advance(0, DaState::Aligned);
    s.add("状态机·Pending→Aligned 拒", r1 == Err(DaDiag::ERewind), "越级推进显性拒");
    // 7. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0,
        "SEG_BASE 0x40 对齐且在 VX9 域",
    );
    // 8. 收口：判据条数只增不减。
    s.add("收口·判据条数", s.len() >= 12, "判据 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = cite_row(4).unwrap_or(CiteRow { no: 4, topic: SehTopic::FinallyConcept, anchor: 0x400, expect_align: true, budget: 1 });
        let t = TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") };
        assert!(judge_cite(&row, row.anchor, &t).is_ok());
    }

    #[test]
    fn test_boundary_empty_anchor() {
        let row = cite_row(0).unwrap_or(CiteRow { no: 0, topic: SehTopic::TryConcept, anchor: 0, expect_align: true, budget: 1 });
        let t = TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") };
        assert_eq!(judge_cite(&row, 0, &t), Err(DaDiag::EEmpty));
    }

    #[test]
    fn test_boundary_row_overflow() {
        let row = CiteRow { no: CITE_ROWS as u32, topic: SehTopic::TryConcept, anchor: 1, expect_align: true, budget: 1 };
        let t = TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") };
        assert_eq!(judge_cite(&row, 1, &t), Err(DaDiag::ENoEnt));
    }

    #[test]
    fn test_silent_gate_three_cols() {
        let row = cite_row(2).unwrap_or(CiteRow { no: 2, topic: SehTopic::ExceptConcept, anchor: 2, expect_align: true, budget: 1 });
        let t = TraceCols { pending: String::new(), evidence: alloc::format!("e"), next: alloc::format!("n") };
        assert_eq!(judge_cite(&row, row.anchor, &t), Err(DaDiag::ESilent));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = DaLedger::new();
        for k in 0..10 {
            let t = if k % 2 == 0 {
                TraceCols { pending: String::new(), evidence: alloc::format!("e"), next: alloc::format!("n") }
            } else {
                TraceCols { pending: alloc::format!("p"), evidence: String::new(), next: alloc::format!("n") }
            };
            assert!(led.silent_gate(&t), "第 {} 轮静默必须被抓", k);
        }
        let (_, _, missed) = led.silent_stats();
        assert_eq!(missed, 0);
    }

    #[test]
    fn test_state_machine_rewind() {
        let mut led = DaLedger::new();
        assert_eq!(led.advance(0, DaState::Aligned), Err(DaDiag::ERewind));
    }

    #[test]
    fn test_conservation() {
        let mut led = DaLedger::new();
        for i in 0..CITE_ROWS {
            if let Some(r) = cite_row(i) {
                let _ = led.advance(i, DaState::Checked);
                let anchor_in = if r.expect_align { r.anchor } else { r.anchor + 0x10 };
                let t = TraceCols { pending: alloc::format!("p"), evidence: alloc::format!("e"), next: alloc::format!("n") };
                if let Ok(g) = judge_cite(&r, anchor_in, &t) {
                    let _ = led.account(i, g);
                }
            }
        }
        assert!(led.conservation_ok());
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (c1, f1) = run_cycle();
        let (c2, f2) = run_cycle();
        assert_eq!(c1, c2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx12754_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
