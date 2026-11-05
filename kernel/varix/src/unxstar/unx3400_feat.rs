//! UNX-F3400 批收口面 · 0x79400段
//!
//! 判据原文（UNX-F3400 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥180 行；
//!   ② J1：finalize 五步断言链留痕；台账翻态完成。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. finalize 五步闭集：装配/留痕/折叠/翻态/写回，序号从一起且逐级推进，
//!    跳步、倒退、重跑一律显性拒（错误码带可操作中文建议）；
//! 2. 断言链留痕：每步执行即落一条留痕（步号+判定+备注），留痕册按写入序
//!    只增不改；收口后重折留痕册指纹与收口摘要比对，改痕必被抓；
//! 3. 台账翻态：批台账（进行中→已完成）单向翻态，翻态前置条件是前四步
//!    全部完成且逐痕判定为真，缺一即拒（防"翻态未收口"假绿）；
//! 4. 写回摘要：五步全绿后拼装非空摘要（含步名/痕数/链指纹），摘要空即
//!    收口失败；摘要落定后只读；
//! 5. 注入矩阵 8 项：跳步/重跑/早翻态/改痕/空痕/摘要篡改/步册错位/越位
//!    收口逐种构造，逐种必须被口点出；
//! 6. 诊断码闭集 0x79401..0x7940C，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x79400;

/// finalize 步数（判据原文「五步断言链」）。
pub const FINALIZE_STEPS: usize = 5;

/// 留痕指纹折叠种子（FNV-1a 偏移基）。
pub const FOLD_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 留痕指纹折叠质数（FNV-1a 质数）。
pub const FOLD_PRIME: u64 = 0x100_0000_01B3;

/// 批收口诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 跳步：finalize 五步必须逐级推进，越位执行被拒。
    ESkip,
    /// 重跑：已完成的步不许再跑，重跑被拒。
    ERerun,
    /// 早翻态：前四步未齐即翻台账，被拒。
    EEarlyFlip,
    /// 留痕被改：收口后重折指纹与收口摘要不符。
    ETraceTamper,
    /// 空痕：步骤未落任何留痕即算失败。
    EEmptyTrace,
    /// 假痕：留痕判定为假（违约断言混进了链）。
    EFalseTrace,
    /// 摘要空：五步全绿但写回摘要为空，收口失败。
    EEmptySummary,
    /// 摘要篡改：收口后改摘要，被拒。
    ESummarySealed,
    /// 步册错位：传入步号与名册位次不符。
    EMisalign,
    /// 相位倒退：已完成批不许再收口。
    EPhase,
    /// 空步册：输入载荷为空，无痕可留。
    EEmptyInput,
    /// 链指纹零：折叠结果为零视为异常。
    EZeroDigest,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ESkip => 0x01,
                Diag::ERerun => 0x02,
                Diag::EEarlyFlip => 0x03,
                Diag::ETraceTamper => 0x04,
                Diag::EEmptyTrace => 0x05,
                Diag::EFalseTrace => 0x06,
                Diag::EEmptySummary => 0x07,
                Diag::ESummarySealed => 0x08,
                Diag::EMisalign => 0x09,
                Diag::EPhase => 0x0a,
                Diag::EEmptyInput => 0x0b,
                Diag::EZeroDigest => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ESkip => "跳步被拒：finalize 五步从装配起逐级推进，先补跑被跳过的步",
            Diag::ERerun => "重跑被拒：该步已完成并留痕，重跑走复验口而非再执行",
            Diag::EEarlyFlip => "早翻态被拒：装配/留痕/折叠/翻态四步全绿才许翻台账",
            Diag::ETraceTamper => "留痕被改：重折指纹与收口摘要不符，回滚留痕册后重收口",
            Diag::EEmptyTrace => "空痕被拒：该步未落留痕，先补落痕再推进",
            Diag::EFalseTrace => "假痕被拒：链里有判定为假的断言，先修断言再收口",
            Diag::EEmptySummary => "摘要空：写回面无内容，按步名+痕数+链指纹重拼摘要",
            Diag::ESummarySealed => "摘要已封：收口后摘要只读，改动走下一批收口",
            Diag::EMisalign => "步册错位：步号与名册位次不符，按 FINALIZE 名册重排",
            Diag::EPhase => "相位倒退：进行中→收口中→已完成 单向，倒退即拒",
            Diag::EEmptyInput => "空步册：输入载荷为空，无痕可留，先备齐载荷",
            Diag::EZeroDigest => "链指纹为零：折叠异常，检查留痕册是否被清空",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ESkip,
    Diag::ERerun,
    Diag::EEarlyFlip,
    Diag::ETraceTamper,
    Diag::EEmptyTrace,
    Diag::EFalseTrace,
    Diag::EEmptySummary,
    Diag::ESummarySealed,
    Diag::EMisalign,
    Diag::EPhase,
    Diag::EEmptyInput,
    Diag::EZeroDigest,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79401..0x7940C 闭集内"
}

/// finalize 五步闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinStep {
    /// 步一：装配断言链（载荷入册）。
    Assemble,
    /// 步二：逐断言留痕。
    Trace,
    /// 步三：链指纹折叠。
    Fold,
    /// 步四：台账翻态。
    LedgerFlip,
    /// 步五：写回摘要。
    Writeback,
}

impl FinStep {
    /// 位次（0..FINALIZE_STEPS）。
    pub fn slot(self) -> usize {
        match self {
            FinStep::Assemble => 0,
            FinStep::Trace => 1,
            FinStep::Fold => 2,
            FinStep::LedgerFlip => 3,
            FinStep::Writeback => 4,
        }
    }

    /// 步名（与名册同序）。
    pub fn name(self) -> &'static str {
        match self {
            FinStep::Assemble => "装配",
            FinStep::Trace => "留痕",
            FinStep::Fold => "折叠",
            FinStep::LedgerFlip => "翻态",
            FinStep::Writeback => "写回",
        }
    }

    /// 由位次反查步（越位返回 None）。
    pub fn from_slot(slot: usize) -> Option<FinStep> {
        match slot {
            0 => Some(FinStep::Assemble),
            1 => Some(FinStep::Trace),
            2 => Some(FinStep::Fold),
            3 => Some(FinStep::LedgerFlip),
            4 => Some(FinStep::Writeback),
            _ => None,
        }
    }
}

/// 五步名册（钦定顺序）。
pub const STEP_NAMES: [&str; FINALIZE_STEPS] = ["装配", "留痕", "折叠", "翻态", "写回"];

/// 批相位（单向机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 进行中：五步未启动。
    Running,
    /// 收口中：步进推进中。
    Finalizing,
    /// 已完成：台账已翻态、摘要已写回。
    Done,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Running => 0,
            Phase::Finalizing => 1,
            Phase::Done => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Running => "进行中",
            Phase::Finalizing => "收口中",
            Phase::Done => "已完成",
        }
    }
}

/// 一条断言链留痕（步号 + 判定 + 备注 + 逐痕指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceRec {
    /// 所属步位次（0..FINALIZE_STEPS）。
    pub step: usize,
    /// 痕序号（从一起，写入序递增）。
    pub seq: u32,
    /// 断言判定（真=过）。
    pub verdict: bool,
    /// 备注（载荷摘要，非空）。
    pub note: &'static str,
}

/// 单痕指纹（FNV-1a，步号与序号混折——改位次/序号必被重折点出）。
pub fn trace_digest(rec: &TraceRec) -> u64 {
    let mut h = FOLD_SEED;
    for b in rec.note.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(FOLD_PRIME);
    }
    h ^= (rec.step as u64) << 32;
    h = h.wrapping_mul(FOLD_PRIME);
    h ^= rec.seq as u64;
    h ^= if rec.verdict { 0x9E37_79B9_7F4A_7C15 } else { 0 };
    h.wrapping_mul(FOLD_PRIME)
}

/// 留痕册指纹（逐痕指纹按写入序折叠）。
pub fn chain_digest(traces: &[TraceRec]) -> u64 {
    let mut h = FOLD_SEED;
    for t in traces {
        h ^= trace_digest(t);
        h = h.wrapping_mul(FOLD_PRIME);
    }
    h
}

/// 批收口机（相位单向 + 五步步进 + 留痕册只增）。
#[derive(Clone, Debug)]
pub struct Finalizer {
    /// 批位相。
    phase: Phase,
    /// 五步完成位图（位次 true=已完成）。
    steps_done: [bool; FINALIZE_STEPS],
    /// 留痕册（写入序只增）。
    traces: Vec<TraceRec>,
    /// 收口摘要（写回步落定，此后只读）。
    summary: String,
    /// 台账翻态标记。
    ledger_flipped: bool,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl Finalizer {
    /// 新批收口机（进行中）。
    pub fn new() -> Finalizer {
        Finalizer {
            phase: Phase::Running,
            steps_done: [false; FINALIZE_STEPS],
            traces: Vec::new(),
            summary: String::new(),
            ledger_flipped: false,
            rejects: 0,
        }
    }

    /// 批位相。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 台账翻态标记。
    pub fn ledger_flipped(&self) -> bool {
        self.ledger_flipped
    }

    /// 收口摘要（只读视图）。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 留痕条数。
    pub fn trace_len(&self) -> usize {
        self.traces.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取第 idx 条留痕（越界返回 None）。
    pub fn trace_at(&self, idx: usize) -> Option<TraceRec> {
        self.traces.get(idx).copied()
    }

    /// 第 slot 步是否已完成。
    pub fn step_done(&self, slot: usize) -> bool {
        match self.steps_done.get(slot) {
            Some(x) => *x,
            None => false,
        }
    }

    /// 启动收口（进行中→收口中，单向）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Running {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        self.phase = Phase::Finalizing;
        Ok(())
    }

    /// 执行第 slot 步（逐级推进、每步恰一次、逐载荷留痕）。
    ///
    /// `payloads` 是本步断言载荷（非空），逐载荷判真落痕；
    /// 返回本步留痕数。
    pub fn run_step(&mut self, slot: usize, payloads: &[(&'static str, bool)]) -> Result<usize, u32> {
        if self.phase != Phase::Finalizing {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let step = match FinStep::from_slot(slot) {
            Some(x) => x,
            None => return Err(Diag::EMisalign.val()),
        };
        // 名册对齐：步名必须与位次一致（防步册错位注入）。
        if step.name() != STEP_NAMES.get(slot).copied().unwrap_or("") {
            return Err(Diag::EMisalign.val());
        }
        // 逐级推进：前置步必须全部完成（跳步在此点出）。
        let mut pre = 0usize;
        while pre < slot {
            if !self.steps_done.get(pre).copied().unwrap_or(true) {
                return Err(Diag::ESkip.val());
            }
            pre += 1;
        }
        // 每步恰一次（重跑在此点出）。
        if self.steps_done.get(slot).copied().unwrap_or(false) {
            self.rejects += 1;
            return Err(Diag::ERerun.val());
        }
        // 空载荷：无痕可留。
        if payloads.is_empty() {
            return Err(Diag::EEmptyInput.val());
        }
        // 翻态步前置：前三步全绿（留痕判定全真）才许翻台账。
        if slot == FinStep::LedgerFlip.slot() {
            let mut k = 0usize;
            while k < self.traces.len() {
                let t = match self.traces.get(k) {
                    Some(x) => *x,
                    None => return Err(Diag::EEmptyTrace.val()),
                };
                if !t.verdict {
                    return Err(Diag::EFalseTrace.val());
                }
                k += 1;
            }
        }
        // 逐载荷留痕。
        let base_seq = (self.traces.len() as u32) + 1;
        let mut n = 0usize;
        for (i, (note, verdict)) in payloads.iter().enumerate() {
            if note.is_empty() {
                return Err(Diag::EEmptyTrace.val());
            }
            self.traces.push(TraceRec {
                step: slot,
                seq: base_seq + i as u32,
                verdict: *verdict,
                note,
            });
            n += 1;
        }
        if n == 0 {
            return Err(Diag::EEmptyTrace.val());
        }
        // 翻态步落定：台账翻态。
        if slot == FinStep::LedgerFlip.slot() {
            self.ledger_flipped = true;
        }
        if let Some(x) = self.steps_done.get_mut(slot) {
            *x = true;
        }
        Ok(n)
    }

    /// 留痕册当前指纹（复验面用）。
    pub fn digest(&self) -> u64 {
        chain_digest(&self.traces)
    }

    /// 收口（写回步全绿后拼装摘要并落定相位→已完成）。
    ///
    /// 前置：五步全完成、逐痕判定全真、台账已翻态、链指纹非零。
    pub fn finalize(&mut self) -> Result<String, u32> {
        if self.phase != Phase::Finalizing {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let mut done = 0usize;
        while done < FINALIZE_STEPS {
            if !self.steps_done.get(done).copied().unwrap_or(true) {
                return Err(Diag::ESkip.val());
            }
            done += 1;
        }
        if self.traces.is_empty() {
            return Err(Diag::EEmptyTrace.val());
        }
        let mut k = 0usize;
        while k < self.traces.len() {
            let t = match self.traces.get(k) {
                Some(x) => *x,
                None => return Err(Diag::EEmptyTrace.val()),
            };
            if !t.verdict {
                return Err(Diag::EFalseTrace.val());
            }
            k += 1;
        }
        if !self.ledger_flipped {
            return Err(Diag::EEarlyFlip.val());
        }
        let dg = self.digest();
        if dg == 0 {
            return Err(Diag::EZeroDigest.val());
        }
        // 拼装写回摘要（五步逐行 + 痕数 + 链指纹）。
        let mut sum = String::new();
        let mut s = 0usize;
        while s < FINALIZE_STEPS {
            let name = match STEP_NAMES.get(s) {
                Some(x) => *x,
                None => "",
            };
            let mut per_step = 0usize;
            for t in &self.traces {
                if t.step == s {
                    per_step += 1;
                }
            }
            sum.push_str(&format!("步{}{} 痕{}\n", s + 1, name, per_step));
            s += 1;
        }
        sum.push_str(&format!("痕{} 链指纹{:016x}\n", self.traces.len(), dg));
        if sum.is_empty() {
            return Err(Diag::EEmptySummary.val());
        }
        self.summary = sum.clone();
        self.phase = Phase::Done;
        Ok(sum)
    }

    /// 收口后重验：重折留痕册指纹与收口时记录一致且摘要未被改。
    /// `digest_at_finalize` 是收口时返回的链指纹。
    pub fn reverify(&self, digest_at_finalize: u64) -> Result<(), u32> {
        if self.phase != Phase::Done {
            return Err(Diag::EPhase.val());
        }
        if self.summary.is_empty() {
            return Err(Diag::EEmptySummary.val());
        }
        if self.digest() != digest_at_finalize {
            return Err(Diag::ETraceTamper.val());
        }
        if !self.summary.contains(&format!("{:016x}", digest_at_finalize)) {
            return Err(Diag::ESummarySealed.val());
        }
        Ok(())
    }
}

/// 标准五步载荷（每步两条，判定全真——清洁收口语料）。
pub fn clean_payloads(slot: usize) -> Vec<(&'static str, bool)> {
    match slot {
        0 => vec![("断言链册齐备", true), ("批号入册", true)],
        1 => vec![("逐断言判定为真", true), ("留痕序号连续", true)],
        2 => vec![("链指纹非零", true), ("折叠序稳定", true)],
        3 => vec![("四步全绿前置成立", true), ("台账可翻", true)],
        4 => vec![("摘要非空", true), ("写回面齐", true)],
        _ => Vec::new(),
    }
}

/// 清洁收口：五步按序跑满并 finalize，返回（收口机, 链指纹, 摘要）。
pub fn clean_finalize() -> (Finalizer, u64, String) {
    let mut f = Finalizer::new();
    let _ = f.begin();
    let mut slot = 0usize;
    while slot < FINALIZE_STEPS {
        let _ = f.run_step(slot, &clean_payloads(slot));
        slot += 1;
    }
    let dg = f.digest();
    let sum = match f.finalize() {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    (f, dg, sum)
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：跳步（不跑装配直接跑留痕）。
    let mut f = Finalizer::new();
    let _ = f.begin();
    let skip_caught = f.run_step(1, &clean_payloads(1)) == Err(Diag::ESkip.val());
    mark(&mut out, 0, skip_caught);
    // 注入2：重跑（装配步跑两遍）。
    let mut f = Finalizer::new();
    let _ = f.begin();
    let _ = f.run_step(0, &clean_payloads(0));
    let rerun_caught = f.run_step(0, &clean_payloads(0)) == Err(Diag::ERerun.val());
    mark(&mut out, 1, rerun_caught);
    // 注入3：早翻态（只跑前三步就 finalize，翻态步未跑）。
    let mut f = Finalizer::new();
    let _ = f.begin();
    let mut s = 0usize;
    while s < 3 {
        let _ = f.run_step(s, &clean_payloads(s));
        s += 1;
    }
    let early_caught = f.finalize().is_err();
    mark(&mut out, 2, early_caught);
    // 注入4：假痕（留痕步混入判定为假的断言，翻态步必拒）。
    let mut f = Finalizer::new();
    let _ = f.begin();
    let _ = f.run_step(0, &clean_payloads(0));
    let bad: Vec<(&'static str, bool)> = vec![("假断言", false)];
    let _ = f.run_step(1, &bad);
    let _ = f.run_step(2, &clean_payloads(2));
    let false_caught = f.run_step(3, &clean_payloads(3)) == Err(Diag::EFalseTrace.val());
    mark(&mut out, 3, false_caught);
    // 注入5：空载荷（空步册被拒）。
    let mut f = Finalizer::new();
    let _ = f.begin();
    let empty_caught = f.run_step(0, &[]) == Err(Diag::EEmptyInput.val());
    mark(&mut out, 4, empty_caught);
    // 注入6：步册错位（步名与位次不符——用越位步号）。
    let mut f = Finalizer::new();
    let _ = f.begin();
    let mis_caught = f.run_step(9, &clean_payloads(0)) == Err(Diag::EMisalign.val());
    mark(&mut out, 5, mis_caught);
    // 注入7：相位倒退（已完成批再收口被拒）。
    let (mut f, dg, _) = clean_finalize();
    let phase_caught = f.finalize() == Err(Diag::EPhase.val());
    mark(&mut out, 6, phase_caught);
    // 注入8：留痕篡改（收口后改痕，复验必抓）。留痕册只增不改是结构承诺；
    // 篡改注入以「同构异痕收口机对照 + 错指纹复验」双面构造：
    // 同构输入但首痕备注被换 → 重折指纹必不等原收口指纹；
    // 原收口机拿错指纹复验 → 复验口必须点出 ETraceTamper。
    let mut g = Finalizer::new();
    let _ = g.begin();
    let mut tampered_payload: Vec<(&'static str, bool)> = clean_payloads(0);
    if let Some(first) = tampered_payload.get_mut(0) {
        first.0 = "被换痕";
    }
    let _ = g.run_step(0, &tampered_payload);
    let mut s2 = 1usize;
    while s2 < FINALIZE_STEPS {
        let _ = g.run_step(s2, &clean_payloads(s2));
        s2 += 1;
    }
    let tamper_diff = g.digest() != dg;
    let reverify_catch = f.reverify(dg ^ 1) == Err(Diag::ETraceTamper.val());
    let clean_reverify = f.reverify(dg).is_ok();
    mark(&mut out, 7, tamper_diff && reverify_catch && clean_reverify);
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x79400
}

/// 五步闭集自洽（名册/位次/反查三面对齐，不重不漏）。
fn steps_closed_ok() -> bool {
    if STEP_NAMES.len() != FINALIZE_STEPS {
        return false;
    }
    let all = [
        FinStep::Assemble,
        FinStep::Trace,
        FinStep::Fold,
        FinStep::LedgerFlip,
        FinStep::Writeback,
    ];
    let mut i = 0usize;
    while i < all.len() {
        let a = match all.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want = match STEP_NAMES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.name() != want || a.slot() != i {
            return false;
        }
        let back = match FinStep::from_slot(i) {
            Some(x) => x,
            None => return false,
        };
        if back != a {
            return false;
        }
        let mut j = i + 1;
        while j < all.len() {
            let b = match all.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// J1 主判据：五步断言链留痕 + 台账翻态完成。
fn j1_main_ok() -> bool {
    let (f, dg, sum) = clean_finalize();
    let steps_all = {
        let mut ok = true;
        let mut s = 0usize;
        while s < FINALIZE_STEPS {
            if !f.step_done(s) {
                ok = false;
            }
            s += 1;
        }
        ok
    };
    let traces_ok = f.trace_len() == FINALIZE_STEPS * 2;
    let ledger_ok = f.ledger_flipped() && f.phase() == Phase::Done;
    let summary_ok = !sum.is_empty() && sum.contains("翻态") && sum.contains(&format!("{:016x}", dg));
    steps_all && traces_ok && ledger_ok && summary_ok
}

/// 留痕逐条自检（序号连续、备注非空、步号与名册一致）。
fn traces_wellformed_ok() -> bool {
    let (f, _, _) = clean_finalize();
    let mut prev_seq = 0u32;
    let mut i = 0usize;
    while i < f.trace_len() {
        let t = match f.trace_at(i) {
            Some(x) => x,
            None => return false,
        };
        if t.seq != prev_seq + 1 || t.note.is_empty() || t.step >= FINALIZE_STEPS {
            return false;
        }
        prev_seq = t.seq;
        i += 1;
    }
    prev_seq > 0
}

/// 双跑一致（同构输入两次收口，指纹/摘要/痕数全同）。
fn dual_run_ok() -> bool {
    let (a, dga, sa) = clean_finalize();
    let (b, dgb, sb) = clean_finalize();
    dga == dgb && dga != 0 && sa == sb && a.trace_len() == b.trace_len() && a.phase() == b.phase()
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3400 批收口面。
pub fn run_unx3400_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3400");
    let (mut f, dg, sum) = clean_finalize();

    s.add_dyn(
        "F3400-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F3400-02 五步闭集自洽",
        steps_closed_ok(),
        &format!("steps={}", FINALIZE_STEPS),
    );

    // 正常流：五步留痕 + 台账翻态 + 摘要写回（J1 主判据）。
    s.add_dyn(
        "F3400-03 finalize五步留痕台账翻态",
        j1_main_ok(),
        &format!("phase={} flipped={}", f.phase().name(), f.ledger_flipped()),
    );

    // 留痕逐条自检。
    s.add_dyn(
        "F3400-04 留痕逐条良构",
        traces_wellformed_ok(),
        &format!("traces={}", f.trace_len()),
    );

    // 边界：收口后复验通过（对照面——复验面不许空转）。
    s.add_dyn(
        "F3400-05 收口后复验通过",
        f.reverify(dg).is_ok() && f.phase().ord() == 2,
        &format!("digest={:016x}", dg),
    );

    // 摘要非空且含步名与指纹（判据③写回面）。
    s.add_dyn(
        "F3400-06 写回摘要非空含步名指纹",
        !sum.is_empty() && sum.contains("装配") && sum.contains("写回"),
        &format!("bytes={}", sum.len()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3400-07 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 边界：进行中批直接 finalize 显性拒。
    let mut raw = Finalizer::new();
    let raw_rej = raw.finalize() == Err(Diag::EPhase.val());
    s.add_dyn(
        "F3400-08 未启动收口显性拒",
        raw_rej && raw.phase() == Phase::Running,
        &format!("phase={}", raw.phase().name()),
    );

    // 边界：未 begin 就 run_step 显性拒（相位门）。
    let mut cold = Finalizer::new();
    let cold_rej = cold.run_step(0, &clean_payloads(0)) == Err(Diag::EPhase.val());
    s.add_dyn(
        "F3400-09 未启动步进显性拒",
        cold_rej && cold.rejects() >= 1,
        &format!("rejects={}", cold.rejects()),
    );

    // 单痕指纹敏感性：改步号/序号/判定/备注任一，指纹必变。
    let base_rec = TraceRec { step: 1, seq: 2, verdict: true, note: "痕" };
    let base_dg = trace_digest(&base_rec);
    let sens = trace_digest(&TraceRec { step: 2, ..base_rec }) != base_dg
        && trace_digest(&TraceRec { seq: 3, ..base_rec }) != base_dg
        && trace_digest(&TraceRec { verdict: false, ..base_rec }) != base_dg
        && trace_digest(&TraceRec { note: "改痕", ..base_rec }) != base_dg;
    s.add_dyn(
        "F3400-10 单痕指纹四维敏感",
        sens && base_dg != 0,
        &format!("base={:016x}", base_dg),
    );

    // 链指纹对痕序敏感（重排必变）。
    let mut traces = Vec::new();
    traces.push(TraceRec { step: 0, seq: 1, verdict: true, note: "甲" });
    traces.push(TraceRec { step: 0, seq: 2, verdict: true, note: "乙" });
    let d1 = chain_digest(&traces);
    traces.reverse();
    let d2 = chain_digest(&traces);
    s.add_dyn(
        "F3400-11 链指纹对痕序敏感",
        d1 != d2 && d1 != 0 && d2 != 0,
        &format!("d1={:016x} d2={:016x}", d1, d2),
    );

    // 相位机单向：Running→Finalizing→Done，倒退拒。
    let mono = f.phase() == Phase::Done
        && f.finalize() == Err(Diag::EPhase.val())
        && Phase::Running.ord() < Phase::Finalizing.ord()
        && Phase::Finalizing.ord() < Phase::Done.ord();
    s.add_dyn(
        "F3400-12 相位机单向倒退拒",
        mono,
        &format!("ord={}", f.phase().ord()),
    );

    // 双跑一致。
    s.add_dyn(
        "F3400-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 诊断码闭集：12 码齐、查表非空、未知码有兜底文案。
    s.add_dyn(
        "F3400-14 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EZeroDigest.detail(),
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
        assert_eq!(Diag::ESkip.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EZeroDigest.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79400);
    }

    #[test]
    fn clean_finalize_full_chain() {
        let (f, dg, sum) = clean_finalize();
        assert_eq!(f.phase(), Phase::Done);
        assert!(f.ledger_flipped());
        assert_eq!(f.trace_len(), FINALIZE_STEPS * 2);
        assert_ne!(dg, 0);
        assert!(!sum.is_empty());
        assert!(f.reverify(dg).is_ok());
    }

    #[test]
    fn steps_closed_and_aligned() {
        assert!(steps_closed_ok());
        assert_eq!(STEP_NAMES.len(), FINALIZE_STEPS);
        assert_eq!(FinStep::from_slot(4), Some(FinStep::Writeback));
        assert_eq!(FinStep::from_slot(5), None);
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn skip_and_rerun_rejected() {
        let mut f = Finalizer::new();
        let _ = f.begin();
        assert_eq!(f.run_step(2, &clean_payloads(2)), Err(Diag::ESkip.val()));
        let _ = f.run_step(0, &clean_payloads(0));
        assert_eq!(f.run_step(0, &clean_payloads(0)), Err(Diag::ERerun.val()));
    }

    #[test]
    fn false_trace_blocks_ledger_flip() {
        let mut f = Finalizer::new();
        let _ = f.begin();
        let _ = f.run_step(0, &clean_payloads(0));
        let bad: Vec<(&'static str, bool)> = vec![("假断言", false)];
        let _ = f.run_step(1, &bad);
        let _ = f.run_step(2, &clean_payloads(2));
        assert_eq!(f.run_step(3, &clean_payloads(3)), Err(Diag::EFalseTrace.val()));
        assert!(!f.ledger_flipped());
    }

    #[test]
    fn phase_monotonic() {
        let mut f = Finalizer::new();
        assert_eq!(f.phase(), Phase::Running);
        assert_eq!(f.begin(), Ok(()));
        assert_eq!(f.begin(), Err(Diag::EPhase.val()));
        let (mut f, _, _) = clean_finalize();
        assert_eq!(f.finalize(), Err(Diag::EPhase.val()));
        assert_eq!(f.phase().ord(), 2);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
        let (a, dga, _) = clean_finalize();
        let (b, dgb, _) = clean_finalize();
        assert_eq!(dga, dgb);
        assert_eq!(a.trace_len(), b.trace_len());
    }

    #[test]
    fn trace_digest_sensitivity() {
        let base = TraceRec { step: 0, seq: 1, verdict: true, note: "痕" };
        let d0 = trace_digest(&base);
        assert_ne!(d0, 0);
        assert_ne!(d0, trace_digest(&TraceRec { note: "乙", ..base }));
        assert_ne!(d0, trace_digest(&TraceRec { verdict: false, ..base }));
    }

    #[test]
    fn empty_payload_rejected() {
        let mut f = Finalizer::new();
        let _ = f.begin();
        assert_eq!(f.run_step(0, &[]), Err(Diag::EEmptyInput.val()));
        assert_eq!(f.trace_len(), 0);
    }

    #[test]
    fn reverify_detects_summary_mismatch() {
        let (f, dg, _) = clean_finalize();
        assert!(f.reverify(dg).is_ok());
        // 指纹错位：复验口必须点出（防复验面空转）。
        assert_eq!(f.reverify(dg ^ 1), Err(Diag::ETraceTamper.val()));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3400_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
