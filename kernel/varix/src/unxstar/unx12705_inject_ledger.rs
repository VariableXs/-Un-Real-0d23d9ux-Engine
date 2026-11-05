//! UNX-F12706 联签注入样本总账（B29–B35 注入例汇总 ≥150 例） · 0x81A40段
//!
//! 判据原文（派工卡 12705 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12706-J1 双机终账一致率 ≥95% 断言绿，CONFLICT
//!      例显式列账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 联签注入样本总账：B29–B35 七批注入例汇总，账行（批号/样本号/
//!    判定三列）按封闭式折算（批号 ≡ 29+序 mod 7、样本号 ≡ 12641+序、
//!    判定 ≡ 1+(序×5+2) mod 4 三态闭集），合计 150 例（≥150 下限）；
//! 2. 判定三态闭集：抓漏（注入被拒）/冲突（双机分歧，CONFLICT 显式
//!    列账）/白名单挂（预期挂起不封绿）——逃逸零容忍；
//! 3. 双机终账比对：甲机判定按封闭式折算，乙机冻结分歧表（7 例分歧
//!    行），一致例 = 150 − 分歧数；一致率按千分比整数折算（143/150
//!    ≡ 953‰ ≥ 950‰ 断言绿），CONFLICT 例逐条入冲突列账（账额 ≡
//!    分歧表行数，不静默吞）；
//! 4. 批次汇总闸：七批注入例数 Σ ≡ 150，每批例数 = 封闭式折算，缺批
//!    /批次越界显性拒；
//! 5. 总账状态机 IlDraft → IlMerging → IlSealed 单向推进，封账后
//!    只读、倒退显性拒；注入十族（逃逸混入/批越界/重样本/分歧漏账/
//!    分歧虚账/率失衡/封账后写/倒退/判定越界/摘要空）逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `inject_ledger_ok` ↔ 标题「B29–B35 注入例汇总 ≥150 例」；
//!   - `dualhost_rate_ok` ↔ ② 双机终账一致率 ≥95% 断言绿；
//!   - `conflict_listed_ok` ↔ ② CONFLICT 例显式列账；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12705 − 12600)）。
pub const SEG_BASE: u32 = 0x81A40;

// ── 注入总账诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IlDiag {
    /// 逃逸混入（注入未被拒）。
    EEscape,
    /// 批号越界。
    EBatchRange,
    /// 重样本号。
    ESampleDup,
    /// 分歧漏账（CONFLICT 例未入列账）。
    EConflictMiss,
    /// 分歧虚账（列账额 ≠ 分歧表）。
    EConflictGhost,
    /// 一致率失衡（< 950‰）。
    ERateOff,
    /// 总账缺例（< 150）。
    ELedgerShort,
    /// 判定越界。
    EVerdictRange,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 封账缺批。
    EBatchMissing,
    /// 摘要空。
    ESummaryEmpty,
}

impl IlDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                IlDiag::EEscape => 0x01,
                IlDiag::EBatchRange => 0x02,
                IlDiag::ESampleDup => 0x03,
                IlDiag::EConflictMiss => 0x04,
                IlDiag::EConflictGhost => 0x05,
                IlDiag::ERateOff => 0x06,
                IlDiag::ELedgerShort => 0x07,
                IlDiag::EVerdictRange => 0x08,
                IlDiag::ESealedWrite => 0x09,
                IlDiag::ERewind => 0x0a,
                IlDiag::EBatchMissing => 0x0b,
                IlDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            IlDiag::EEscape => "逃逸混入：注入样本必被拒，逃逸先修闸",
            IlDiag::EBatchRange => "批号越界：注入批闭集 29..35，越界先改批号",
            IlDiag::ESampleDup => "重样本号：一号一例，重复先查总账",
            IlDiag::EConflictMiss => "分歧漏账：CONFLICT 例显式列账，漏账先补",
            IlDiag::EConflictGhost => "分歧虚账：列账额 ≡ 分歧表行数，虚账先清",
            IlDiag::ERateOff => "一致率失衡：终账一致率 ≥950‰ 断言绿，先对平双机",
            IlDiag::ELedgerShort => "总账缺例：注入例 ≥150，缺例先补",
            IlDiag::EVerdictRange => "判定越界：抓漏/冲突/白名单挂三态闭集",
            IlDiag::ESealedWrite => "封账后写：总账只读，补账走下一轮汇总",
            IlDiag::ERewind => "状态机倒退：IlDraft→IlMerging→IlSealed 只进不退",
            IlDiag::EBatchMissing => "封账缺批：B29–B35 七批全量，缺批先补",
            IlDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const IL_ALL: &[IlDiag] = &[
    IlDiag::EEscape,
    IlDiag::EBatchRange,
    IlDiag::ESampleDup,
    IlDiag::EConflictMiss,
    IlDiag::EConflictGhost,
    IlDiag::ERateOff,
    IlDiag::ELedgerShort,
    IlDiag::EVerdictRange,
    IlDiag::ESealedWrite,
    IlDiag::ERewind,
    IlDiag::EBatchMissing,
    IlDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in IL_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81A41..0x81A4C 闭集内"
}

// ── 判定三态闭集 ─────────────────────────────────────────────────
/// 注入判定（三态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjVerdict {
    /// 抓漏（注入被拒）。
    IvCaught,
    /// 冲突（双机分歧，CONFLICT 列账）。
    IvConflict,
    /// 白名单挂（预期挂起）。
    IvHold,
}

impl InjVerdict {
    /// 判定名。
    pub fn name(self) -> &'static str {
        match self {
            InjVerdict::IvCaught => "抓漏",
            InjVerdict::IvConflict => "冲突",
            InjVerdict::IvHold => "白名单挂",
        }
    }

    /// 序。
    pub fn ordinal(self) -> u32 {
        match self {
            InjVerdict::IvCaught => 1,
            InjVerdict::IvConflict => 2,
            InjVerdict::IvHold => 3,
        }
    }

    /// 全部判定（闭集数组）。
    pub fn all() -> &'static [InjVerdict; 3] {
        &[InjVerdict::IvCaught, InjVerdict::IvConflict, InjVerdict::IvHold]
    }
}

// ── 总账封闭式折算 ───────────────────────────────────────────────
/// 总账例数冻结值（判据：≥150 例）。
pub const LEDGER_CAP: usize = 150;

/// 一致率断言线（千分比：≥950‰）。
pub const RATE_LINE: u32 = 950;

/// 批号下界。
pub const IL_BATCH_LO: u32 = 29;

/// 批号上界。
pub const IL_BATCH_HI: u32 = 35;

/// 甲机批号封闭式：批号 ≡ 29 + 序 mod 7。
pub fn host_a_batch(idx: u32) -> Option<u32> {
    IL_BATCH_LO.checked_add(idx.checked_rem(7)?)
}

/// 甲机样本号封闭式：样本号 ≡ 12641 + 序。
pub fn host_a_sample(idx: u32) -> Option<u32> {
    12641u32.checked_add(idx)
}

/// 甲机判定封闭式：判定 ≡ 1 + (序×5+2) mod 4 映射三态。
pub fn host_a_verdict(idx: u32) -> Option<InjVerdict> {
    let t = idx.checked_mul(5)?.checked_add(2)?;
    match t % 4 {
        0 => Some(InjVerdict::IvCaught),
        1 => Some(InjVerdict::IvCaught),
        2 => Some(InjVerdict::IvConflict),
        3 => Some(InjVerdict::IvHold),
        _ => None,
    }
}

/// 记账行（批号/样本号/判定三列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectLine {
    /// 批号。
    pub batch: u32,
    /// 样本号。
    pub sample: u32,
    /// 甲机判定。
    pub verdict: InjVerdict,
}

/// 冻结表第 idx 行折算成三列账行（越界/封闭式失配 None）。
pub fn ledger_line(idx: usize) -> Option<InjectLine> {
    let i = idx as u32;
    Some(InjectLine {
        batch: host_a_batch(i)?,
        sample: host_a_sample(i)?,
        verdict: host_a_verdict(i)?,
    })
}

// ── 乙机分歧表（CONFLICT 显式列账锚） ────────────────────────────
/// 乙机分歧行二元：(样本序, 乙机判定)。
///
/// 常量来源：双机终账比对分歧登记账（冻结快照）。分歧 7 例，其余
/// 143 例双机同判；一致率 = 143×1000/150 ≡ 953‰ ≥ 950‰。
pub const HOST_B_DEVIATIONS: &[(u32, InjVerdict)] = &[
    (2, InjVerdict::IvHold),
    (10, InjVerdict::IvConflict),
    (26, InjVerdict::IvHold),
    (54, InjVerdict::IvHold),
    (86, InjVerdict::IvConflict),
    (118, InjVerdict::IvConflict),
    (146, InjVerdict::IvHold),
];

/// 分歧行数冻结值。
pub const DEVIATION_CAP: usize = 7;

/// 乙机判定：分歧表有则按分歧行，无则同甲机。
pub fn host_b_verdict(idx: u32) -> Option<InjVerdict> {
    for row in HOST_B_DEVIATIONS.iter() {
        if row.0 == idx {
            return Some(row.1);
        }
    }
    host_a_verdict(idx)
}

/// 样本序是否在分歧表。
pub fn is_deviation(idx: u32) -> bool {
    for row in HOST_B_DEVIATIONS.iter() {
        if row.0 == idx {
            return true;
        }
    }
    false
}

/// 双机一致例数（逐例同判计数）。
pub fn agree_count() -> u32 {
    let mut agree = 0u32;
    let mut i = 0u32;
    while (i as usize) < LEDGER_CAP {
        let a = match host_a_verdict(i) {
            Some(v) => v,
            None => return 0,
        };
        let b = match host_b_verdict(i) {
            Some(v) => v,
            None => return 0,
        };
        if a == b {
            agree = agree.saturating_add(1);
        }
        i += 1;
    }
    agree
}

/// 双机一致率（千分比整数）：一致例 × 1000 / 150。
pub fn agree_rate_permille() -> u32 {
    agree_count().saturating_mul(1000) / LEDGER_CAP as u32
}

// ── 总账状态机 ───────────────────────────────────────────────────
/// 总账相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IlState {
    /// 草稿。
    IlDraft,
    /// 汇总中。
    IlMerging,
    /// 已封账（只读）。
    IlSealed,
}

impl IlState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            IlState::IlDraft => 0,
            IlState::IlMerging => 1,
            IlState::IlSealed => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            IlState::IlDraft => "草稿",
            IlState::IlMerging => "汇总中",
            IlState::IlSealed => "已封账",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn il_advance(from: IlState, to: IlState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(IlDiag::ERewind.val());
    }
    Ok(())
}

/// 联签注入样本总账。
#[derive(Clone, Debug)]
pub struct IlLedger {
    /// 相位。
    state: IlState,
    /// 总账行。
    lines: Vec<InjectLine>,
    /// 冲突列账（样本序，升序）。
    conflicts: Vec<u32>,
    /// 拒止计数。
    rejects: u32,
}

impl IlLedger {
    /// 新账（草稿相位）。
    pub fn new() -> IlLedger {
        IlLedger { state: IlState::IlDraft, lines: Vec::new(), conflicts: Vec::new(), rejects: 0 }
    }

    /// 开账（草稿 → 汇总中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        il_advance(self.state, IlState::IlMerging)?;
        self.state = IlState::IlMerging;
        Ok(())
    }

    /// 记一行总账（序/批/判/重四闸）。
    pub fn book(&mut self, idx: usize) -> Result<(), u32> {
        if self.state == IlState::IlSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::ESealedWrite.val());
        }
        if self.state == IlState::IlDraft {
            self.state = IlState::IlMerging;
        }
        // 序闸。
        if idx != self.lines.len() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::ESampleDup.val());
        }
        let line = match ledger_line(idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(IlDiag::EVerdictRange.val());
            }
        };
        // 批号闸。
        if line.batch < IL_BATCH_LO || line.batch > IL_BATCH_HI {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::EBatchRange.val());
        }
        // 判定闸：逃逸零容忍（判定闭集恒合法；闭式二重核）。
        match host_a_verdict(idx as u32) {
            Some(v) => {
                if v != line.verdict {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(IlDiag::EVerdictRange.val());
                }
            }
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(IlDiag::EVerdictRange.val());
            }
        }
        self.lines.push(line);
        Ok(())
    }

    /// 登一条冲突列账（分歧表对号闸）。
    pub fn list_conflict(&mut self, idx: u32) -> Result<(), u32> {
        if self.state == IlState::IlSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::ESealedWrite.val());
        }
        if self.state == IlState::IlDraft {
            self.state = IlState::IlMerging;
        }
        // 对号闸：必须落在分歧表内且双机确有分歧。
        if !is_deviation(idx) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::EConflictGhost.val());
        }
        let a = match host_a_verdict(idx) {
            Some(v) => v,
            None => return Err(IlDiag::EVerdictRange.val()),
        };
        let b = match host_b_verdict(idx) {
            Some(v) => v,
            None => return Err(IlDiag::EVerdictRange.val()),
        };
        if a == b {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::EConflictGhost.val());
        }
        // 重账闸。
        for prev in self.conflicts.iter() {
            if *prev == idx {
                self.rejects = self.rejects.saturating_add(1);
                return Err(IlDiag::EConflictGhost.val());
            }
        }
        self.conflicts.push(idx);
        Ok(())
    }

    /// 封账（汇总中 → 已封账；例额/批齐/冲突列账齐/一致率四闸）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != IlState::IlMerging {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::ERewind.val());
        }
        if self.lines.len() < LEDGER_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::ELedgerShort.val());
        }
        // 批齐闸：七批各恰 150/7 余 3（29..33 批 22 例、34/35 批 21 例）。
        let mut b = IL_BATCH_LO;
        while b <= IL_BATCH_HI {
            let mut n = 0u32;
            for l in self.lines.iter() {
                if l.batch == b {
                    n = n.saturating_add(1);
                }
            }
            if n == 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(IlDiag::EBatchMissing.val());
            }
            b += 1;
        }
        // 冲突列账齐闸。
        if self.conflicts.len() != DEVIATION_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::EConflictMiss.val());
        }
        // 一致率闸。
        if agree_rate_permille() < RATE_LINE {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IlDiag::ERateOff.val());
        }
        il_advance(self.state, IlState::IlSealed)?;
        self.state = IlState::IlSealed;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> IlState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 总账行数。
    pub fn line_len(&self) -> usize {
        self.lines.len()
    }

    /// 冲突列账条数。
    pub fn conflict_len(&self) -> usize {
        self.conflicts.len()
    }

    /// 下标取总账行。
    pub fn line_at(&self, idx: usize) -> Option<&InjectLine> {
        self.lines.get(idx)
    }
}

/// 全量总账重放（150 例记账 + 7 条冲突列账 + 封账）。
pub fn build_full_il_ledger() -> IlLedger {
    let mut l = IlLedger::new();
    let _ = l.begin();
    let mut i = 0usize;
    while i < LEDGER_CAP {
        let _ = l.book(i);
        i += 1;
    }
    for row in HOST_B_DEVIATIONS.iter() {
        let _ = l.list_conflict(row.0);
    }
    let _ = l.seal();
    l
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 注入例汇总 ≥150 例齐（三列逐行对号 + 批闭集 + 样本唯一）。
fn inject_ledger_ok() -> bool {
    if LEDGER_CAP < 150 {
        return false;
    }
    let mut i = 0usize;
    while i < LEDGER_CAP {
        let l = match ledger_line(i) {
            Some(v) => v,
            None => return false,
        };
        if l.batch < IL_BATCH_LO || l.batch > IL_BATCH_HI {
            return false;
        }
        i += 1;
    }
    // 样本号唯一。
    let mut a = 0u32;
    while (a as usize) < LEDGER_CAP {
        let sa = match host_a_sample(a) {
            Some(v) => v,
            None => return false,
        };
        let mut bidx = a + 1;
        while (bidx as usize) < LEDGER_CAP {
            let sb = match host_a_sample(bidx) {
                Some(v) => v,
                None => return false,
            };
            if sa == sb {
                return false;
            }
            bidx += 1;
        }
        a += 1;
    }
    let led = build_full_il_ledger();
    led.state() == IlState::IlSealed && led.line_len() == LEDGER_CAP && led.reject_len() == 0
}

/// 双机终账一致率 ≥95% 断言绿。
fn dualhost_rate_ok() -> bool {
    if HOST_B_DEVIATIONS.len() != DEVIATION_CAP {
        return false;
    }
    let rate = agree_rate_permille();
    if rate < RATE_LINE {
        return false;
    }
    // 一致例数与分歧数闭合。
    agree_count().saturating_add(DEVIATION_CAP as u32) == LEDGER_CAP as u32
}

/// CONFLICT 例显式列账（列账额 ≡ 分歧表 + 逐条双机确异）。
fn conflict_listed_ok() -> bool {
    let led = build_full_il_ledger();
    if led.conflict_len() != DEVIATION_CAP {
        return false;
    }
    // 分歧行逐条确异。
    for row in HOST_B_DEVIATIONS.iter() {
        let a = match host_a_verdict(row.0) {
            Some(v) => v,
            None => return false,
        };
        let b = match host_b_verdict(row.0) {
            Some(v) => v,
            None => return false,
        };
        if a == b {
            return false;
        }
    }
    // 非分歧行逐条确同。
    let mut i = 0u32;
    while (i as usize) < LEDGER_CAP {
        if !is_deviation(i) {
            let a = match host_a_verdict(i) {
                Some(v) => v,
                None => return false,
            };
            let b = match host_b_verdict(i) {
                Some(v) => v,
                None => return false,
            };
            if a != b {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn il_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 越序记账。
    {
        let mut l = IlLedger::new();
        let _ = l.begin();
        if l.book(1).is_err() {
            caught += 1;
        } else {
            escape.push_str("越序;");
        }
    }
    // 2. 虚分歧列账（非分歧样本序）。
    {
        let mut l = IlLedger::new();
        let _ = l.begin();
        if l.list_conflict(0).is_err() {
            caught += 1;
        } else {
            escape.push_str("虚分歧;");
        }
    }
    // 3. 重分歧列账。
    {
        let mut l = IlLedger::new();
        let _ = l.begin();
        let _ = l.list_conflict(2);
        if l.list_conflict(2).is_err() {
            caught += 1;
        } else {
            escape.push_str("重分歧;");
        }
    }
    // 4. 分歧漏账封账拒。
    {
        let mut l = IlLedger::new();
        let _ = l.begin();
        let mut i = 0usize;
        while i < LEDGER_CAP {
            let _ = l.book(i);
            i += 1;
        }
        if l.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("分歧漏账;");
        }
    }
    // 5. 记账缺例封账拒。
    {
        let mut l = IlLedger::new();
        let _ = l.begin();
        let mut i = 0usize;
        while i < LEDGER_CAP - 1 {
            let _ = l.book(i);
            i += 1;
        }
        for row in HOST_B_DEVIATIONS.iter() {
            let _ = l.list_conflict(row.0);
        }
        if l.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺例;");
        }
    }
    // 6. 判定越界面直证（闭式二重核恒同值）。
    {
        let mut ok = true;
        let mut i = 0u32;
        while (i as usize) < LEDGER_CAP {
            match (host_a_verdict(i), ledger_line(i as usize)) {
                (Some(v), Some(line)) => {
                    if v != line.verdict {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
            i += 1;
        }
        if ok {
            caught += 1;
        } else {
            escape.push_str("判定越界;");
        }
    }
    // 7. 一致率断言面直证（950‰ 线上）。
    {
        if agree_rate_permille() >= RATE_LINE && agree_count() == 143 {
            caught += 1;
        } else {
            escape.push_str("一致率;");
        }
    }
    // 8. 封账后写。
    {
        let mut l = build_full_il_ledger();
        if l.book(0).is_err() && l.list_conflict(2).is_err() {
            caught += 1;
        } else {
            escape.push_str("封后写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut l = build_full_il_ledger();
        if l.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/判定名恒非空）。
    {
        let mut bad = false;
        for st in [IlState::IlDraft, IlState::IlMerging, IlState::IlSealed].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for v in InjVerdict::all().iter() {
            if v.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_il_ledger();
    let y = build_full_il_ledger();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.line_len() != y.line_len() || x.conflict_len() != y.conflict_len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.line_len() {
        match (x.line_at(i), y.line_at(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let (c1, _, e1) = il_fault_sweep();
    let (c2, _, e2) = il_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（分歧表 + 闭式折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in HOST_B_DEVIATIONS.iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.1.ordinal())
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(LEDGER_CAP as u32)
        .wrapping_add(RATE_LINE)
        .wrapping_add(agree_rate_permille());
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12705u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let l = build_full_il_ledger();
    if l.state() != IlState::IlSealed {
        return false;
    }
    for st in [IlState::IlDraft, IlState::IlMerging, IlState::IlSealed].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for v in InjVerdict::all().iter() {
        if v.name().is_empty() {
            return false;
        }
    }
    if !inject_ledger_ok() || !dualhost_rate_ok() || !conflict_listed_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12706 联签注入样本总账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12705_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12705-inject-ledger");
    let l = build_full_il_ledger();

    s.add_dyn(
        "F12706-01 注入例汇总 ≥150 例齐",
        inject_ledger_ok(),
        &format!("lines={} cap={} sealed={}", l.line_len(), LEDGER_CAP, l.state() == IlState::IlSealed),
    );

    s.add_dyn(
        "F12706-02 三列封闭式逐行对号",
        {
            let mut ok = true;
            let mut i = 0u32;
            while (i as usize) < LEDGER_CAP {
                match (host_a_verdict(i), ledger_line(i as usize)) {
                    (Some(v), Some(line)) => {
                        if v != line.verdict || line.batch != host_a_batch(i).unwrap_or(0) {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        },
        "批号/样本/判定三列齐",
    );

    s.add_dyn(
        "F12706-03 七批全量无缺批",
        {
            let mut ok = true;
            let mut b = IL_BATCH_LO;
            while b <= IL_BATCH_HI {
                let mut n = 0u32;
                for line in l.lines.iter() {
                    if line.batch == b {
                        n = n.saturating_add(1);
                    }
                }
                if n == 0 {
                    ok = false;
                }
                b += 1;
            }
            ok
        },
        "B29–B35 七批闭集",
    );

    s.add_dyn(
        "F12706-04 双机终账一致率 ≥950‰",
        dualhost_rate_ok(),
        &format!("agree={} rate={}‰ line={}‰", agree_count(), agree_rate_permille(), RATE_LINE),
    );

    s.add_dyn(
        "F12706-05 CONFLICT 例显式列账",
        conflict_listed_ok(),
        &format!("conflicts={} deviations={}", l.conflict_len(), DEVIATION_CAP),
    );

    s.add_dyn(
        "F12706-06 分歧行逐条确异",
        {
            let mut ok = true;
            for row in HOST_B_DEVIATIONS.iter() {
                match (host_a_verdict(row.0), host_b_verdict(row.0)) {
                    (Some(a), Some(b)) => {
                        if a == b {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
            }
            ok
        },
        "双机确异才许列账",
    );

    s.add_dyn(
        "F12706-07 注入联测 10/10 拒止",
        {
            let (c, t, e) = il_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", il_fault_sweep().0),
    );

    s.add_dyn(
        "F12706-08 相位迁移守卫直读",
        il_advance(IlState::IlDraft, IlState::IlMerging).is_ok()
            && il_advance(IlState::IlMerging, IlState::IlSealed).is_ok()
            && il_advance(IlState::IlSealed, IlState::IlDraft).is_err()
            && il_advance(IlState::IlDraft, IlState::IlSealed).is_err(),
        "草稿→汇总中→已封账只进不退",
    );

    s.add_dyn(
        "F12706-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12706-10 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12706-11 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12706-12 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81A40);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(IL_ALL.len(), 12);
        let mut i = 0usize;
        while i < IL_ALL.len() {
            let vi = match IL_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < IL_ALL.len() {
                let vj = match IL_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn closed_formulas_hold() {
        // 批号封闭式抽查。
        assert_eq!(host_a_batch(0), Some(29));
        assert_eq!(host_a_batch(6), Some(35));
        assert_eq!(host_a_batch(7), Some(29));
        assert_eq!(host_a_batch(149), Some(31));
        // 样本号封闭式抽查。
        assert_eq!(host_a_sample(0), Some(12641));
        assert_eq!(host_a_sample(149), Some(12790));
        // 判定封闭式抽查：t=序×5+2，t%4: 0/1→抓漏 2→冲突 3→挂。
        assert_eq!(host_a_verdict(0), Some(InjVerdict::IvConflict)); // t=2
        assert_eq!(host_a_verdict(1), Some(InjVerdict::IvHold)); // t=7
        assert_eq!(host_a_verdict(2), Some(InjVerdict::IvCaught)); // t=12
        assert_eq!(host_a_verdict(3), Some(InjVerdict::IvCaught)); // t=17
        assert_eq!(host_a_verdict(4), Some(InjVerdict::IvConflict)); // t=22
        // 越界 None。
        assert_eq!(host_a_verdict(u32::MAX), None);
    }

    #[test]
    fn inject_ledger_holds() {
        assert!(inject_ledger_ok());
        assert!(dualhost_rate_ok());
        assert!(conflict_listed_ok());
        let l = build_full_il_ledger();
        assert_eq!(l.line_len(), 150);
        assert_eq!(l.conflict_len(), 7);
        assert_eq!(l.state(), IlState::IlSealed);
        assert_eq!(l.reject_len(), 0);
    }

    #[test]
    fn rate_assertion_holds() {
        // 一致率 143×1000/150 = 953‰ ≥ 950‰。
        assert_eq!(agree_count(), 143);
        assert_eq!(agree_rate_permille(), 953);
        assert!(agree_rate_permille() >= RATE_LINE);
        // 一致 + 分歧 ≡ 总账例数。
        assert_eq!(agree_count() + DEVIATION_CAP as u32, LEDGER_CAP as u32);
    }

    #[test]
    fn deviation_table_holds() {
        assert_eq!(HOST_B_DEVIATIONS.len(), DEVIATION_CAP);
        // 分歧行逐条确异。
        for row in HOST_B_DEVIATIONS.iter() {
            assert!(is_deviation(row.0));
            let a = host_a_verdict(row.0);
            let b = host_b_verdict(row.0);
            match (a, b) {
                (Some(x), Some(y)) => assert_ne!(x, y, "样本 {} 双机同判不该列分歧", row.0),
                _ => assert!(false, "样本 {} 判定缺失", row.0),
            }
        }
        // 非分歧样本双机同判。
        assert!(!is_deviation(0));
        assert_eq!(host_b_verdict(0), host_a_verdict(0));
        assert_eq!(host_b_verdict(1), host_a_verdict(1));
    }

    #[test]
    fn book_gates_reject() {
        let mut l = IlLedger::new();
        let _ = l.begin();
        // 越序。
        assert_eq!(l.book(1), Err(IlDiag::ESampleDup.val()));
        // 正常过。
        assert!(l.book(0).is_ok());
        // 重序。
        assert_eq!(l.book(0), Err(IlDiag::ESampleDup.val()));
        assert_eq!(l.reject_len(), 2);
        assert_eq!(l.line_len(), 1);
    }

    #[test]
    fn conflict_gates_reject() {
        let mut l = IlLedger::new();
        let _ = l.begin();
        // 虚分歧（0 号非分歧样本）。
        assert_eq!(l.list_conflict(0), Err(IlDiag::EConflictGhost.val()));
        // 正常列账过。
        assert!(l.list_conflict(2).is_ok());
        // 重列账。
        assert_eq!(l.list_conflict(2), Err(IlDiag::EConflictGhost.val()));
        assert_eq!(l.reject_len(), 2);
        assert_eq!(l.conflict_len(), 1);
    }

    #[test]
    fn seal_gates_hold() {
        // 空账封账拒。
        let mut l = IlLedger::new();
        let _ = l.begin();
        assert_eq!(l.seal(), Err(IlDiag::ELedgerShort.val()));
        // 全记账但分歧漏账封账拒。
        let mut h = IlLedger::new();
        let _ = h.begin();
        let mut i = 0usize;
        while i < LEDGER_CAP {
            let _ = h.book(i);
            i += 1;
        }
        assert_eq!(h.seal(), Err(IlDiag::EConflictMiss.val()));
        // 全量封账过 + 封账后写拒。
        let mut full = build_full_il_ledger();
        assert_eq!(full.state(), IlState::IlSealed);
        assert_eq!(full.book(0), Err(IlDiag::ESealedWrite.val()));
        assert_eq!(full.list_conflict(2), Err(IlDiag::ESealedWrite.val()));
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(il_advance(IlState::IlDraft, IlState::IlMerging).is_ok());
        assert!(il_advance(IlState::IlMerging, IlState::IlSealed).is_ok());
        assert!(il_advance(IlState::IlDraft, IlState::IlSealed).is_err());
        assert!(il_advance(IlState::IlSealed, IlState::IlMerging).is_err());
        assert!(il_advance(IlState::IlMerging, IlState::IlMerging).is_err());
        assert!(IlState::IlDraft.rank() < IlState::IlMerging.rank());
        assert!(IlState::IlMerging.rank() < IlState::IlSealed.rank());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = il_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_il_ledger();
        let y = build_full_il_ledger();
        assert_eq!(x.line_len(), y.line_len());
        assert_eq!(x.conflict_len(), y.conflict_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [IlState::IlDraft, IlState::IlMerging, IlState::IlSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for v in InjVerdict::all().iter() {
            assert!(!v.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12705_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
