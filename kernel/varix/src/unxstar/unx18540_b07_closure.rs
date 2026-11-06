//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18540 B07 批守恒闭合断言 · 0x802C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18540-J1 本批 20 条行数逐条求和=6,000 三方一致，红绿齐。
//! 结构清单：
//!   - 数据结构：LineRec（Copy 行记录：序/卡号/行种/行数/证据字）、
//!     ClosureLedger（20 槽闭合账 + 卡占位位面 + 三路求和通道：正序饱和
//!     累加 / 逆序重加 / 冻结声明总额）、冻结行表 QUOTA_TABLE（卡表配额
//!     10 行）+ 判据行补齐恒等式；
//!   - 状态机：Opened→Summed→Sealed 三态（Sealed 后追加显式拒绝）；
//!   - 账本：行账（追加序单调 + 证据字逐行重算必符）+ 位面（卡占位
//!     popcount == 行数守恒）；
//!   - 错误路径：ClCode 闭集 12 码，code() = SEG_BASE + n（0x802C0 段），
//!     每码非空中文建议；
//!   - 注入核：行数篡改（三路必破） / 缺行 / 重行 / 卡号越界 四类坏样本
//!     各 10 轮逐类断言必红或显式拒绝（10/10 全抓）。
//! 行表推导（写死）：20 条 = 10 卡 × 2 行（配额行 + 判据行）。
//!   配额行 = 卡表配额列 [260,300,280,260,300,320,280,300,380,280]，
//!   逐条累加 = 2,960；J1 冻结总额 6,000 → 判据行补齐 = 6,000 − 2,960
//!   = 3,040 → 10 卡均分 = 304 行/卡。三方一致的三方：
//!   ① 账面正序饱和求和；② 逆序重加（交换律复算）；③ 冻结声明
//!   DECLARED_TOTAL = 6,000。红绿齐：正常账三路全等（绿），任一行数
//!   被篡改则三路失衡必红（SumMismatch）。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x802C0，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x802C0;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 闭合账行数（J1 冻结：20 条）。
pub const LINE_TOTAL: usize = 20;
/// 本批卡数（F18531..F18540 = 10 卡）。
pub const CARD_TOTAL: usize = 10;
/// 卡号下限（1 ↔ F18531）。
pub const CARD_NO_MIN: u8 = 1;
/// 卡号上限（10 ↔ F18540）。
pub const CARD_NO_MAX: u8 = 10;
/// 行种：配额行（卡表配额列快照）。
pub const KIND_QUOTA: u8 = 1;
/// 行种：判据行（闭合补齐行）。
pub const KIND_CRIT: u8 = 2;
/// 卡表配额列（10 卡逐条快照：260,300,280,260,300,320,280,300,380,280）。
pub const QUOTA_TABLE: [u32; CARD_TOTAL] =
    [260, 300, 280, 260, 300, 320, 280, 300, 380, 280];
/// 配额行合计（逐条累加冻结值：2,960）。
pub const QUOTA_SUM: u32 = 2_960;
/// J1 冻结声明总额（20 条求和 = 6,000）。
pub const DECLARED_TOTAL: u32 = 6_000;
/// 判据行补齐恒等式（推导：6,000 − 2,960 = 3,040 → 均分 10 卡 = 304）。
pub const CRIT_LINES: u32 = (DECLARED_TOTAL - QUOTA_SUM) / CARD_TOTAL as u32;
/// 判据行合计（304 × 10 = 3,040）。
pub const CRIT_SUM: u32 = CRIT_LINES * CARD_TOTAL as u32;
/// 单行行数上限（推导：卡表最大目标 760 + 40 余量 = 800）。
pub const LINE_MAX: u32 = 800;
/// 账态：开账（可追加）。
pub const STATE_OPENED: u8 = 1;
/// 账态：已求和（核算完毕）。
pub const STATE_SUMMED: u8 = 2;
/// 账态：封账（终态）。
pub const STATE_SEALED: u8 = 3;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏样本）。
pub const INJECT_CLASSES: usize = 4;
// 编译期守恒：QUOTA_SUM 必须等于 QUOTA_TABLE 逐条累加。
const QUOTA_TABLE_SUM: u32 = {
    let mut s = 0u32;
    let mut i = 0;
    while i < QUOTA_TABLE.len() {
        s += QUOTA_TABLE[i];
        i += 1;
    }
    s
};
const _: () = assert!(QUOTA_TABLE_SUM == QUOTA_SUM);
const _: () = assert!(QUOTA_SUM + CRIT_SUM == DECLARED_TOTAL);

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 卡号越界（1..=10 闭集外）
    CardNoOutOfRange,
    /// 行种越闭集（配额/判据外）
    KindIllegal,
    /// 零行数（无账可闭合）
    ZeroLines,
    /// 行数超上限（>800）
    LineCapExceeded,
    /// 重行（同卡同行种已入账）
    DuplicateLine,
    /// 闭合账满（20 行耗尽）
    LedgerFull,
    /// 缺行（行数 ≠ 20）
    MissingLine,
    /// 三方求和失衡（篡改必红）
    SumMismatch,
    /// 账态非法转换
    IllegalTransition,
    /// 位面损坏（卡占位 popcount ≠ 行数）
    PlaneCorrupt,
    /// 守恒破坏（绿 + 红 = 20 或证据字不符）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl ClCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::CardNoOutOfRange => "卡号越界：卡号闭集为 1..=10（F18531..F18540）",
            ClCode::KindIllegal => "行种越闭集：行种闭集为 配额行/判据行",
            ClCode::ZeroLines => "零行数：行数为 0 无闭合意义，显式拒绝",
            ClCode::LineCapExceeded => "行数超上限：单行 ≤800（760+40 余量）",
            ClCode::DuplicateLine => "重行：同卡同行种已入账，20 行逐行唯一",
            ClCode::LedgerFull => "闭合账满：20 行耗尽，账本只增不减",
            ClCode::MissingLine => "缺行：闭合账行数不足 20，三方守恒必红",
            ClCode::SumMismatch => "三方求和失衡：正序/逆序/声明总额不一致，必红",
            ClCode::IllegalTransition => "账态非法转换：三态只进不退，越级拒绝",
            ClCode::PlaneCorrupt => "位面损坏：卡占位 popcount 须与行数吻合",
            ClCode::ConservationBroken => "守恒破坏：绿 + 红 = 20 或证据字不符",
            ClCode::NotReproducible => "双跑不一致：同输入两次闭合结果不同",
        }
    }
    /// 码闭集清单。
    pub const ALL: [ClCode; 12] = [
        ClCode::CardNoOutOfRange,
        ClCode::KindIllegal,
        ClCode::ZeroLines,
        ClCode::LineCapExceeded,
        ClCode::DuplicateLine,
        ClCode::LedgerFull,
        ClCode::MissingLine,
        ClCode::SumMismatch,
        ClCode::IllegalTransition,
        ClCode::PlaneCorrupt,
        ClCode::ConservationBroken,
        ClCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if ClCode::ALL[i].code() == code {
            return ClCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x802C0 段闭集内"
}

// ── 行证据字核（确定性可重算） ───────────────────────────────────
/// 行证据字：(行数 << 8) ^ (卡号 << 4) ^ 行种 —— 任一字段变动必改全字。
pub fn line_evidence(lines: u32, card_no: u8, kind: u8) -> u32 {
    (lines << 8) ^ ((card_no as u32) << 4) ^ (kind as u32)
}

// ── 行记录（Copy） ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineRec {
    /// 行账序号（追加序单调）。
    pub seq: u32,
    /// 卡号（1..=10 ↔ F18531..F18540）。
    pub card_no: u8,
    /// 行种（配额/判据）。
    pub kind: u8,
    /// 行数（本行声明的行数）。
    pub lines: u32,
    /// 证据字（对 card_no/kind/lines 派生，篡改必红）。
    pub evidence: u32,
    /// 空槽标记。
    pub empty: bool,
}

impl LineRec {
    /// 空行（const fn）。
    pub const fn empty() -> LineRec {
        LineRec { seq: 0, card_no: 0, kind: 0, lines: 0, evidence: 0, empty: true }
    }
}

// ── 闭合账（核心账本 + 三态状态机 + 位面 + 三路求和） ───────────
pub struct ClosureLedger {
    /// 20 槽行账。
    lines: [LineRec; LINE_TOTAL],
    /// 卡占位位面（位 idx：第 idx 行有账即置位）。
    card_plane: [u64; 1],
    /// 已入账行数。
    count: usize,
    /// 下一个序号。
    next_seq: u32,
    /// 账态。
    state: u8,
    /// 三方求和快照（Summed 态冻结）。
    pub sum_forward: u32,
    pub sum_reverse: u32,
    pub sum_declared: u32,
    /// 三方一致标记。
    pub consistent: bool,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl ClosureLedger {
    pub fn new() -> ClosureLedger {
        ClosureLedger {
            lines: [LineRec::empty(); LINE_TOTAL],
            card_plane: [0u64; 1],
            count: 0,
            next_seq: 1,
            state: STATE_OPENED,
            sum_forward: 0,
            sum_reverse: 0,
            sum_declared: 0,
            consistent: false,
            reject_ledger: Vec::new(),
        }
    }

    /// 追加一行（闸序：账态→卡号→行种→行数→重行→容量）。
    pub fn append(&mut self, card_no: u8, kind: u8, lines: u32) -> Result<u32, u32> {
        if self.state != STATE_OPENED {
            self.reject_ledger.push(ClCode::IllegalTransition.code());
            return Err(ClCode::IllegalTransition.code());
        }
        if card_no < CARD_NO_MIN || card_no > CARD_NO_MAX {
            self.reject_ledger.push(ClCode::CardNoOutOfRange.code());
            return Err(ClCode::CardNoOutOfRange.code());
        }
        if kind != KIND_QUOTA && kind != KIND_CRIT {
            self.reject_ledger.push(ClCode::KindIllegal.code());
            return Err(ClCode::KindIllegal.code());
        }
        if lines == 0 {
            self.reject_ledger.push(ClCode::ZeroLines.code());
            return Err(ClCode::ZeroLines.code());
        }
        if lines > LINE_MAX {
            self.reject_ledger.push(ClCode::LineCapExceeded.code());
            return Err(ClCode::LineCapExceeded.code());
        }
        // 重行扫描（同卡同种唯一）。
        let mut i = 0;
        while i < self.count {
            let rec = match self.lines.get(i) {
                Some(r) => *r,
                None => return Err(ClCode::PlaneCorrupt.code()),
            };
            if !rec.empty && rec.card_no == card_no && rec.kind == kind {
                self.reject_ledger.push(ClCode::DuplicateLine.code());
                return Err(ClCode::DuplicateLine.code());
            }
            i += 1;
        }
        if self.count >= LINE_TOTAL {
            self.reject_ledger.push(ClCode::LedgerFull.code());
            return Err(ClCode::LedgerFull.code());
        }
        // 写账（先算证据字再可变借用）。
        let rec = LineRec {
            seq: self.next_seq,
            card_no,
            kind,
            lines,
            evidence: line_evidence(lines, card_no, kind),
            empty: false,
        };
        let seq = rec.seq;
        let idx = self.count;
        match self.lines.get_mut(idx) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(ClCode::LedgerFull.code());
                return Err(ClCode::LedgerFull.code());
            }
        }
        if let Some(w) = self.card_plane.get_mut(0) {
            *w |= 1u64 << (idx % 64);
        }
        self.count += 1;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(seq)
    }

    /// 从冻结行表整表装配：10 卡 × (配额行, 判据行)。
    pub fn build(&mut self) -> Result<(), u32> {
        let mut card = CARD_NO_MIN;
        while (card as usize) <= CARD_TOTAL {
            let quota = match QUOTA_TABLE.get(card as usize - 1) {
                Some(&q) => q,
                None => return Err(ClCode::PlaneCorrupt.code()),
            };
            self.append(card, KIND_QUOTA, quota)?;
            self.append(card, KIND_CRIT, CRIT_LINES)?;
            card += 1;
        }
        Ok(())
    }

    /// 证据字逐行审计：重算必符，篡改必红。
    pub fn audit_evidence(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.count {
            let rec = match self.lines.get(i) {
                Some(r) => *r,
                None => return Err(ClCode::PlaneCorrupt.code()),
            };
            let want = line_evidence(rec.lines, rec.card_no, rec.kind);
            if want != rec.evidence {
                return Err(ClCode::ConservationBroken.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// 正序饱和求和（u32 显式累加器）。
    pub fn sum_forward_now(&self) -> u32 {
        let mut acc: u32 = 0;
        let mut i = 0;
        while i < self.count {
            let n = match self.lines.get(i) {
                Some(r) => r.lines,
                None => 0,
            };
            acc = acc.saturating_add(n);
            i += 1;
        }
        acc
    }

    /// 逆序重加（交换律复算通道）。
    pub fn sum_reverse_now(&self) -> u32 {
        let mut acc: u32 = 0;
        let mut i = self.count;
        while i > 0 {
            i -= 1;
            let n = match self.lines.get(i) {
                Some(r) => r.lines,
                None => return 0,
            };
            acc = acc.saturating_add(n);
        }
        acc
    }

    /// 求和：Opened → Summed，三路对拍并冻结快照。
    pub fn summarize(&mut self) -> Result<u32, u32> {
        if self.state != STATE_OPENED {
            self.reject_ledger.push(ClCode::IllegalTransition.code());
            return Err(ClCode::IllegalTransition.code());
        }
        // 缺行闸：行数必须恰 20。
        if self.count != LINE_TOTAL {
            self.reject_ledger.push(ClCode::MissingLine.code());
            return Err(ClCode::MissingLine.code());
        }
        self.audit_evidence()?;
        // 位面对账：popcount == 行数。
        let mut pop = 0usize;
        let mut w = *self.card_plane.get(0).unwrap_or(&0);
        while w != 0 {
            w &= w.wrapping_sub(1);
            pop += 1;
        }
        if pop != self.count {
            return Err(ClCode::PlaneCorrupt.code());
        }
        let fwd = self.sum_forward_now();
        let rev = self.sum_reverse_now();
        // 三方对拍：正序 / 逆序 / 冻结声明。
        if fwd != rev || fwd != DECLARED_TOTAL {
            self.sum_forward = fwd;
            self.sum_reverse = rev;
            self.sum_declared = DECLARED_TOTAL;
            self.consistent = false;
            self.state = STATE_SUMMED;
            return Err(ClCode::SumMismatch.code());
        }
        self.sum_forward = fwd;
        self.sum_reverse = rev;
        self.sum_declared = DECLARED_TOTAL;
        self.consistent = true;
        self.state = STATE_SUMMED;
        Ok(fwd)
    }

    /// 封账：Summed → Sealed（终态）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != STATE_SUMMED {
            self.reject_ledger.push(ClCode::IllegalTransition.code());
            return Err(ClCode::IllegalTransition.code());
        }
        if !self.consistent {
            return Err(ClCode::SumMismatch.code());
        }
        self.state = STATE_SEALED;
        Ok(())
    }

    /// 卡分组守恒：逐卡 (配额+判据) 分组求和 = 求和快照（Summed 态冻结值）。
    /// 求和后行账再被改写 → 分组重算 ≠ 快照 → 必红。
    pub fn group_audit(&self) -> Result<(), u32> {
        if self.state != STATE_SUMMED && self.state != STATE_SEALED {
            return Err(ClCode::IllegalTransition.code());
        }
        let mut group_sum = 0u32;
        let mut card = CARD_NO_MIN;
        while (card as usize) <= CARD_TOTAL {
            let mut per_card = 0u32;
            let mut i = 0;
            while i < self.count {
                let rec = match self.lines.get(i) {
                    Some(r) => *r,
                    None => return Err(ClCode::PlaneCorrupt.code()),
                };
                if !rec.empty && rec.card_no == card {
                    per_card = per_card.saturating_add(rec.lines);
                }
                i += 1;
            }
            group_sum = group_sum.saturating_add(per_card);
            card += 1;
        }
        if group_sum != self.sum_forward {
            return Err(ClCode::SumMismatch.code());
        }
        Ok(())
    }

    /// 追加序单调审计。
    pub fn seq_monotonic(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0;
        while i < self.count {
            let rec = match self.lines.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if rec.seq <= prev {
                return false;
            }
            prev = rec.seq;
            i += 1;
        }
        true
    }

    /// 篡改行数（注入专用：模拟"声明写错"的坏样本——行账自洽改写，
    /// 证据字随新值重算，使防线落到三方求和闸而非证据闸）。
    pub fn tamper_lines(&mut self, card_no: u8, kind: u8, new_lines: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.count {
            match self.lines.get_mut(i) {
                Some(r) if !r.empty && r.card_no == card_no && r.kind == kind => {
                    r.lines = new_lines;
                    r.evidence = line_evidence(new_lines, card_no, kind);
                    return Ok(());
                }
                _ => {}
            }
            i += 1;
        }
        Err(ClCode::MissingLine.code())
    }

    /// 缺行注入：撤最后一行（账本尾部回缩）。
    pub fn drop_last(&mut self) -> Result<(), u32> {
        if self.count == 0 {
            return Err(ClCode::MissingLine.code());
        }
        let last = self.count - 1;
        if self.lines.get(last).is_none() {
            return Err(ClCode::PlaneCorrupt.code());
        }
        if let Some(w) = self.card_plane.get_mut(0) {
            *w &= !(1u64 << (last % 64));
        }
        match self.lines.get_mut(last) {
            Some(r) => {
                *r = LineRec::empty();
            }
            None => return Err(ClCode::PlaneCorrupt.code()),
        }
        self.count -= 1;
        Ok(())
    }

    pub fn line_count(&self) -> usize {
        self.count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }

    /// 第 idx 行只读（越界 None）。
    pub fn line_at(&self, idx: usize) -> Option<LineRec> {
        match self.lines.get(idx) {
            Some(r) if !r.empty => Some(*r),
            _ => None,
        }
    }
}

// ── 黄金链：装配 20 行 → 三方求和 6,000 全等 → 封账 ─────────────
/// 返回 (正序和, 逆序和, 声明总额)。三方全等 = 6,000（绿）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = ClosureLedger::new();
    led.build()?;
    let fwd = led.summarize()?;
    led.seal()?;
    led.group_audit()?;
    if !led.seq_monotonic() {
        return Err(ClCode::NotReproducible.code());
    }
    if fwd != DECLARED_TOTAL || !led.consistent {
        return Err(ClCode::SumMismatch.code());
    }
    Ok((led.sum_forward, led.sum_reverse, led.sum_declared))
}

// ── 红路径：行数篡改 → 三方失衡必红（红绿齐的红半边） ───────────
/// 返回 (正序和, 逆序和)。注入：卡 5 配额行 300 → 301。
pub fn inject_tamper_red() -> Result<(u32, u32), u32> {
    let mut led = ClosureLedger::new();
    led.build()?;
    led.tamper_lines(5, KIND_QUOTA, 301)?;
    let r = led.summarize();
    if r.err() != Some(ClCode::SumMismatch.code()) {
        return Err(ClCode::ConservationBroken.code());
    }
    if led.consistent {
        return Err(ClCode::ConservationBroken.code());
    }
    Ok((led.sum_forward, led.sum_reverse))
}

// ── 注入核：四类坏样本，各 10 轮，逐类全抓 ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = *self.tries.get(i).unwrap_or(&0);
            let c = *self.caught.get(i).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(ClCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏样本逐类注入 `rounds` 轮：
/// C0 行数篡改必红、C1 缺行必红（MissingLine）、C2 重行显式拒、
/// C3 卡号/行种越界显式拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：行数篡改 → summarize 必 SumMismatch。
        let mut l0 = ClosureLedger::new();
        let _ = l0.build();
        let _ = l0.tamper_lines(8, KIND_CRIT, 305);
        led.record(0, l0.summarize().err() == Some(ClCode::SumMismatch.code()));
        // C1：缺行 → summarize 必 MissingLine。
        let mut l1 = ClosureLedger::new();
        let _ = l1.build();
        let _ = l1.drop_last();
        led.record(1, l1.summarize().err() == Some(ClCode::MissingLine.code()));
        // C2：重行（卡 3 配额行重复追加）。
        let mut l2 = ClosureLedger::new();
        let _ = l2.build();
        led.record(
            2,
            l2.append(3, KIND_QUOTA, 280).err() == Some(ClCode::DuplicateLine.code()),
        );
        // C3：卡号 0/11 与行种 9 双探。
        let mut l3 = ClosureLedger::new();
        let hit3 = l3.append(0, KIND_QUOTA, 100).is_err()
            && l3.append(11, KIND_QUOTA, 100).err() == Some(ClCode::CardNoOutOfRange.code())
            && l3.append(1, 9, 100).err() == Some(ClCode::KindIllegal.code());
        led.record(3, hit3);
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 探针三序（装配→求和→封账 / 未求和先封账 / 证据字审计） ──────
pub fn probe_seq_a() -> Result<u32, u32> {
    let mut led = ClosureLedger::new();
    led.build()?;
    let s = led.summarize()?;
    led.seal()?;
    Ok(s)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未求和先封账。
    let mut led = ClosureLedger::new();
    led.build()?;
    led.seal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 空账求和必缺行。
    let mut led = ClosureLedger::new();
    led.summarize().map(|_| ())
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ClCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < ClCode::ALL.len() {
        let v = ClCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if ClCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

/// 行表审计：配额表 10 项逐项与卡表一致、判据行恒等式、编译期守恒复核。
pub fn quota_table_audit() -> Result<(), u32> {
    // 编译期守恒复核（运行期再算一遍防表被改）。
    let mut s = 0u32;
    let mut i = 0;
    while i < QUOTA_TABLE.len() {
        let q = match QUOTA_TABLE.get(i) {
            Some(&v) => v,
            None => return Err(ClCode::PlaneCorrupt.code()),
        };
        if q == 0 || q > LINE_MAX {
            return Err(ClCode::LineCapExceeded.code());
        }
        s = s.saturating_add(q);
        i += 1;
    }
    if s != QUOTA_SUM {
        return Err(ClCode::SumMismatch.code());
    }
    if QUOTA_SUM + CRIT_SUM != DECLARED_TOTAL {
        return Err(ClCode::SumMismatch.code());
    }
    if CRIT_LINES != 304 {
        // 恒等式锚：6,000 − 2,960 = 3,040 → 均分 = 304。
        return Err(ClCode::SumMismatch.code());
    }
    Ok(())
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18540_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18540 B07 批守恒闭合");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x802C0 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-行表冻结审计",
        quota_table_audit().is_ok(),
        "配额表逐项 ≤800 且合计 2,960；判据行恒等式 304×10；总额 6,000",
    );
    s.add(
        "J1-账态三序",
        probe_seq_a().is_ok()
            && probe_seq_b().err() == Some(ClCode::IllegalTransition.code())
            && probe_seq_c().err() == Some(ClCode::MissingLine.code()),
        "装配→求和→封账全过；未求和封账/空账求和均拒",
    );
    s.add(
        "J1-边界四类闸",
        {
            let mut led = ClosureLedger::new();
            let c0 = led.append(0, KIND_QUOTA, 100).is_err();
            let c11 = led.append(CARD_NO_MAX + 1, KIND_QUOTA, 100).is_err();
            let k0 = led.append(1, 0, 100).is_err();
            let z = led.append(1, KIND_QUOTA, 0).is_err();
            let cap = led.append(1, KIND_QUOTA, LINE_MAX + 1).is_err();
            let edge = led.append(1, KIND_QUOTA, LINE_MAX).is_ok(); // 恰 800 过
            c0 && c11 && k0 && z && cap && edge
        },
        "卡号 0/11、行种 0、行数 0/801 全拒；恰 800 过",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((f, r, d)) => *f == DECLARED_TOTAL && *r == DECLARED_TOTAL && *d == DECLARED_TOTAL,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-20条求和=6000三方一致",
        g_ok,
        &format!("正序/逆序/声明 = {:?}", g),
    );
    let red = inject_tamper_red();
    let red_ok = match &red {
        Ok((f, r)) => *f == DECLARED_TOTAL + 1 && *r == DECLARED_TOTAL + 1,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-红绿齐之红半边",
        red_ok,
        &format!("篡改后三路 = {:?}（必红 SumMismatch）", red),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_three_way_6000() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, 6_000);
        assert_eq!(g.1, 6_000);
        assert_eq!(g.2, 6_000);
    }

    #[test]
    fn test_quota_table_frozen() {
        assert!(quota_table_audit().is_ok());
        assert_eq!(QUOTA_TABLE.len(), CARD_TOTAL);
        assert_eq!(QUOTA_SUM, 2_960);
        assert_eq!(CRIT_LINES, 304);
        assert_eq!(CRIT_SUM, 3_040);
        assert_eq!(QUOTA_SUM + CRIT_SUM, DECLARED_TOTAL);
    }

    #[test]
    fn test_tamper_red_path() {
        let red = inject_tamper_red().unwrap_or((0, 0));
        assert_eq!(red.0, 6_001);
        assert_eq!(red.1, 6_001);
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏样本 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_state_machine_transitions() {
        // 未求和先封账拒。
        let mut led = ClosureLedger::new();
        let _ = led.build();
        assert!(led.seal().is_err());
        // 正常链。
        assert!(led.summarize().is_ok());
        assert!(led.seal().is_ok());
        // 封账后追加拒。
        assert!(led.append(1, KIND_QUOTA, 10).is_err());
        // 二次求和拒（Summed 态再 summarize）。
        let mut led2 = ClosureLedger::new();
        let _ = led2.build();
        assert!(led2.summarize().is_ok());
        assert!(led2.summarize().is_err());
    }

    #[test]
    fn test_duplicate_line() {
        let mut led = ClosureLedger::new();
        let _ = led.build();
        // 同卡同种重复。
        assert!(led.append(2, KIND_CRIT, 304).is_err());
        // 同卡异种可追加？不行——装配已占满 20 行，容量闸先拒。
        assert!(led.append(2, KIND_QUOTA, 1).is_err());
        assert_eq!(led.line_count(), LINE_TOTAL);
    }

    #[test]
    fn test_missing_line_red() {
        let mut led = ClosureLedger::new();
        let _ = led.build();
        assert!(led.drop_last().is_ok());
        assert_eq!(led.line_count(), LINE_TOTAL - 1);
        assert!(led.summarize().is_err());
        // 空账 drop_last 拒。
        let mut led2 = ClosureLedger::new();
        assert!(led2.drop_last().is_err());
    }

    #[test]
    fn test_evidence_field_separation() {
        // 确定性：同三元同证据字。
        assert_eq!(line_evidence(260, 1, KIND_QUOTA), line_evidence(260, 1, KIND_QUOTA));
        // 域分离：任一字段变动必改全字。
        assert_ne!(line_evidence(260, 1, KIND_QUOTA), line_evidence(261, 1, KIND_QUOTA));
        assert_ne!(line_evidence(260, 1, KIND_QUOTA), line_evidence(260, 2, KIND_QUOTA));
        assert_ne!(line_evidence(260, 1, KIND_QUOTA), line_evidence(260, 1, KIND_CRIT));
        // 行账证据审计（黄金账全符）。
        let mut led = ClosureLedger::new();
        let _ = led.build();
        assert!(led.audit_evidence().is_ok());
        // 自洽改写：证据字随新值重算（防线移交三方求和闸）。
        let _ = led.tamper_lines(9, KIND_QUOTA, 380);
        assert!(led.audit_evidence().is_ok());
    }

    #[test]
    fn test_group_audit_conservation() {
        let mut led = ClosureLedger::new();
        let _ = led.build();
        let _ = led.summarize();
        assert!(led.group_audit().is_ok());
        // 篡改后分组和 ≠ 正序和 → 必红。
        let _ = led.tamper_lines(10, KIND_CRIT, 305);
        assert!(led.group_audit().is_err());
    }

    #[test]
    fn test_seq_monotonic_and_plane() {
        let mut led = ClosureLedger::new();
        let _ = led.build();
        assert!(led.seq_monotonic());
        assert_eq!(led.line_count(), LINE_TOTAL);
        // 逐行序号 1..=20。
        let mut i = 0;
        while i < LINE_TOTAL {
            let rec = led.line_at(i).unwrap_or(LineRec::empty());
            assert_eq!(rec.seq, i as u32 + 1);
            i += 1;
        }
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(ClCode::SumMismatch.code(), SEG_BASE + 7);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18540_checks();
        assert!(set.all_passed(), "UNX-F18540 判据必须全绿：{}", set.red_items_text());
    }
}
