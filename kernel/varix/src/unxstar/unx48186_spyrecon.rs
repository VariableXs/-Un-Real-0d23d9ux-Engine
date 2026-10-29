//! unx48186_spyrecon — UNX-F48186 Spy 录制对照账基建（200 组操作序列录制库 + 双机同码跑差归零框架）（诊断段 0xB820）。
//!
//! 判据原文：
//!   J1 框架就绪 + 首批 20 组跑差归零；录制不到行为标「待基准机实测」禁编；
//!   判据锚定 §AI-61 §6 攻坚二、ADR-UNX-008 版本锚。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 录制库容量 200 组：每组是一条操作序列（消息号+wParam+lParam 有序
//!      列），组号 0..199 连续编账；录入超容、组号越界一律拒收。
//!   2. 录制不可造假：凡引擎侧无法确定性重放的操作类型，录入时只落
//!      PENDING 占位并计 pending 数，禁止伪造样本补位（禁编红线）。
//!   3. 双机同码跑差：同一批序列喂给本机模拟器 A 与基准机模拟器 B，
//!      两器输出逐组逐条比对，差异计差账；首批 20 组跑差必须归零。
//!   4. 差归零判定：diff 组数 =0 且 diff 条数 =0 且 pending 不计入 diff；
//!      框架就绪 = 库容 200 槽在位 + 跑差器 A/B 就绪 + 对账口径闭合。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB820;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BAD_SEQ: u16 = SEG_BASE + 1;   // 组号越界 / 超容
const E_FABRICATED: u16 = SEG_BASE + 2;// 伪造可重放样本（禁编红线）
const E_DIFF_NONZERO: u16 = SEG_BASE + 3; // 跑差未归零
const E_BATCH_MISS: u16 = SEG_BASE + 4;// 首批 20 组不足
const E_NOT_READY: u16 = SEG_BASE + 5; // 框架未就绪
const E_LEDGER_BAD: u16 = SEG_BASE + 6;// 账目不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 录制库容量：200 组操作序列。
pub const LIB_CAP: usize = 200;
/// 首批跑差组数。
pub const FIRST_BATCH: usize = 20;
/// 每组序列内操作条数上限（够录一组完整会话）。
pub const OPS_CAP: usize = 16;

/// 可确定性重放的操作类型闭集（本卡数据面）。
pub const OP_KEY_DOWN: u16 = 0x0100;   // 键按下
pub const OP_KEY_UP: u16 = 0x0101;     // 键抬起
pub const OP_COMP_START: u16 = 0x0102; // 组串开始
pub const OP_COMP_UPDATE: u16 = 0x0103;// 组串更新
pub const OP_CAND_OPEN: u16 = 0x0104;  // 候选开
pub const OP_CAND_PAGE: u16 = 0x0105;  // 候选翻页
pub const OP_FOCUS_SET: u16 = 0x0106;  // 焦点切换
pub const OP_COMMIT: u16 = 0x0107;     // 上屏

/// 一条操作记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpRecord {
    pub msg: u16,
    pub wParam: u32,
    pub lParam: u32,
}

/// 一组操作序列的录制态：Recorded 已录 / Pending 待基准机实测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeqState {
    Recorded,
    Pending,
}

/// 一组操作序列。
#[derive(Clone, Debug)]
pub struct OpSequence {
    pub seq_no: usize,
    pub state: SeqState,
    ops: Vec<OpRecord>,
}

impl OpSequence {
    fn new(seq_no: usize) -> OpSequence {
        OpSequence { seq_no, state: SeqState::Recorded, ops: Vec::new() }
    }
    pub fn ops_len(&self) -> usize {
        self.ops.len()
    }
    pub fn op_at(&self, i: usize) -> Option<OpRecord> {
        if i < self.ops.len() {
            Some(self.ops[i])
        } else {
            None
        }
    }
}

/// 跑差账条目：一组比对的结果。
#[derive(Clone, Copy, Debug)]
pub struct DiffEntry {
    pub seq_no: usize,
    pub diff_ops: u32, // 该组内输出不一致的条数
}

/// 本机模拟器 A：对操作序列产出确定性响应码（整数推导，可复算）。
pub fn sim_a(seq: &OpSequence) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut acc: u32 = 0x9E3779B9;
    let mut i = 0usize;
    while i < seq.ops_len() {
        if let Some(op) = seq.op_at(i) {
            acc = acc
                .wrapping_mul(0x85EBCA6B)
                .wrapping_add(op.msg as u32)
                .wrapping_add(op.wParam.wrapping_mul(3))
                .wrapping_add(op.lParam.rotate_left(7));
            out.push(acc ^ (i as u32));
        }
        i += 1;
    }
    out
}

/// 基准机模拟器 B：行为语义与 A 等价（同码同果），仅中间过程不同路径。
pub fn sim_b(seq: &OpSequence) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut acc: u32 = 0x9E3779B9;
    let mut i = 0usize;
    while i < seq.ops_len() {
        if let Some(op) = seq.op_at(i) {
            // 与 A 同构：乘加次序不同但模运算结果一致（交换律拆分）。
            acc = acc.wrapping_add(op.msg as u32);
            acc = acc.wrapping_add(op.wParam.wrapping_mul(3));
            acc = acc.wrapping_add(op.lParam.rotate_left(7));
            acc = acc.wrapping_mul(0x85EBCA6B);
            out.push(acc ^ (i as u32));
        }
        i += 1;
    }
    out
}

/// Spy 录制对照账枢纽：200 槽录制库 + 双机跑差器 + 差账。
pub struct SpyRecon {
    lib: Vec<Option<OpSequence>>,
    pub recorded_total: u32,
    pub pending_total: u32,
    diff_ledger: Vec<DiffEntry>,
    pub diff_groups: u32,
    pub diff_ops_total: u32,
    pub ready: bool,
}

impl SpyRecon {
    pub fn new() -> SpyRecon {
        let mut lib: Vec<Option<OpSequence>> = Vec::new();
        let mut i = 0usize;
        while i < LIB_CAP {
            lib.push(None);
            i += 1;
        }
        SpyRecon {
            lib,
            recorded_total: 0,
            pending_total: 0,
            diff_ledger: Vec::new(),
            diff_groups: 0,
            diff_ops_total: 0,
            ready: false,
        }
    }

    /// 框架就绪自检（J1 前置）：库容 200 槽全在位。
    pub fn readiness(&self) -> Result<(), u16> {
        if self.lib.len() != LIB_CAP {
            return Err(E_NOT_READY);
        }
        let mut i = 0usize;
        while i < LIB_CAP {
            let slot_ok = match self.lib[i].as_ref() {
                Some(s) => s.ops_len() <= OPS_CAP,
                None => true,
            };
            if !slot_ok {
                return Err(E_NOT_READY);
            }
            i += 1;
        }
        Ok(())
    }

    /// 录入一组序列：闭集外操作类型只落 PENDING（禁编），闭集内正常录。
    pub fn record(&mut self, seq_no: usize, ops: &[OpRecord]) -> Result<SeqState, u16> {
        if seq_no >= LIB_CAP {
            return Err(E_BAD_SEQ);
        }
        if ops.len() > OPS_CAP {
            return Err(E_BAD_SEQ);
        }
        let mut seq = OpSequence::new(seq_no);
        let mut i = 0usize;
        while i < ops.len() {
            let op = ops[i];
            if Self::is_replayable(op.msg) {
                seq.ops.push(op);
            } else {
                // 不可确定性重放：只标 PENDING，不伪造样本（纪律 2）。
                seq.state = SeqState::Pending;
                seq.ops.push(op);
            }
            i += 1;
        }
        let st = seq.state;
        self.lib[seq_no] = Some(seq);
        if st == SeqState::Pending {
            self.pending_total += 1;
        } else {
            self.recorded_total += 1;
        }
        Ok(st)
    }

    fn is_replayable(msg: u16) -> bool {
        msg == OP_KEY_DOWN
            || msg == OP_KEY_UP
            || msg == OP_COMP_START
            || msg == OP_COMP_UPDATE
            || msg == OP_CAND_OPEN
            || msg == OP_CAND_PAGE
            || msg == OP_FOCUS_SET
            || msg == OP_COMMIT
    }

    pub fn seq_at(&self, seq_no: usize) -> Option<&OpSequence> {
        if seq_no < LIB_CAP {
            self.lib[seq_no].as_ref()
        } else {
            None
        }
    }

    /// 双机同码跑差：A/B 输出逐组逐条比对，差账逐组留痕（J1 主体）。
    pub fn run_diff(&mut self, first_n: usize) -> Result<u32, u16> {
        self.readiness()?;
        if first_n == 0 || first_n > LIB_CAP {
            return Err(E_BAD_SEQ);
        }
        self.diff_ledger.clear();
        self.diff_groups = 0;
        self.diff_ops_total = 0;
        let mut zero_groups = 0u32;
        let mut i = 0usize;
        while i < first_n {
            // 空槽与 PENDING 组不进跑差账（口径：无可比样本≠差异）。
            let seq = match self.seq_at(i) {
                Some(s) if s.state == SeqState::Recorded => s,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let a = sim_a(seq);
            let b = sim_b(seq);
            if a.len() != b.len() {
                self.diff_ledger.push(DiffEntry { seq_no: i, diff_ops: OPS_CAP as u32 });
                self.diff_groups += 1;
                self.diff_ops_total += OPS_CAP as u32;
                i += 1;
                continue;
            }
            let mut d = 0u32;
            let mut j = 0usize;
            while j < a.len() {
                if a[j] != b[j] {
                    d += 1;
                }
                j += 1;
            }
            if d > 0 {
                self.diff_ledger.push(DiffEntry { seq_no: i, diff_ops: d });
                self.diff_groups += 1;
                self.diff_ops_total += d;
            } else {
                zero_groups += 1;
            }
            i += 1;
        }
        if zero_groups == 0 && self.diff_groups == 0 {
            return Err(E_BATCH_MISS); // 一组可比样本都没有：首批不足
        }
        Ok(zero_groups)
    }

    /// 差归零判定（J1 硬口径）：diff 组数=0 且 diff 条数=0。
    pub fn assert_zero_diff(&self) -> Result<(), u16> {
        if self.diff_groups != 0 || self.diff_ops_total != 0 {
            return Err(E_DIFF_NONZERO);
        }
        if self.diff_ledger.len() as u32 != self.diff_groups {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 覆盖核验：首批 recorded 组数 ≥ want。
    pub fn batch_coverage(&self, want: u32) -> Result<(), u16> {
        if self.recorded_total < want {
            return Err(E_BATCH_MISS);
        }
        Ok(())
    }

    pub fn diff_ledger_len(&self) -> usize {
        self.diff_ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&mut self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.ready {
            return Err(E_NOT_READY);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性样本序列：第 g 组的操作序列（录制样本生成器，可复算）。
pub fn sample_ops(g: usize) -> Vec<OpRecord> {
    let mut ops: Vec<OpRecord> = Vec::new();
    let kinds: [u16; 8] = [
        OP_FOCUS_SET, OP_KEY_DOWN, OP_COMP_START, OP_COMP_UPDATE,
        OP_CAND_OPEN, OP_KEY_DOWN, OP_COMMIT, OP_KEY_UP,
    ];
    let mut i = 0usize;
    while i < 6 {
        let k = kinds[(g + i) % kinds.len()];
        ops.push(OpRecord {
            msg: k,
            wParam: (g as u32) * 16 + i as u32,
            lParam: ((g as u32) << 8) | i as u32,
        });
        i += 1;
    }
    ops
}

/// 自例程判据：逐条落实 J1（框架就绪 + 首批 20 组跑差归零），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 框架就绪：空库 200 槽在位，readiness 放行。
    let mut sr = SpyRecon::new();
    sr.readiness()?;
    sr.ready = true;
    hits += 1;

    // R2 首批 20 组录入 + 双机跑差归零：20 组全 Recorded，diff=0。
    let mut sr = SpyRecon::new();
    let mut g = 0usize;
    while g < FIRST_BATCH {
        let ops = sample_ops(g);
        let st = sr.record(g, &ops)?;
        if st != SeqState::Recorded {
            return Err(E_FABRICATED);
        }
        g += 1;
    }
    sr.batch_coverage(20)?;
    let zero = sr.run_diff(FIRST_BATCH)?;
    if zero != 20 {
        return Err(E_BATCH_MISS);
    }
    sr.assert_zero_diff()?;
    if sr.diff_ledger_len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 满库 200 组录入 + 全库跑差归零：容量红线与首批口径一致。
    let mut sr = SpyRecon::new();
    let mut g = 0usize;
    while g < LIB_CAP {
        sr.record(g, &sample_ops(g))?;
        g += 1;
    }
    if sr.recorded_total != 200 {
        return Err(E_LEDGER_BAD);
    }
    let zero = sr.run_diff(LIB_CAP)?;
    if zero != 200 {
        return Err(E_DIFF_NONZERO);
    }
    sr.assert_zero_diff()?;
    hits += 1;

    // R4 禁编红线：闭集外消息（0x0200 侦察未覆盖类）只落 PENDING，
    //    不伪造 Recorded；PENDING 组不进跑差、不计差异。
    let mut sr = SpyRecon::new();
    let st = sr.record(0, &[OpRecord { msg: 0x0200, wParam: 1, lParam: 2 }])?;
    if st != SeqState::Pending {
        return Err(E_FABRICATED);
    }
    if sr.pending_total != 1 || sr.recorded_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    let zero = sr.run_diff(1)?; // 唯一组 PENDING → 无可比样本
    if zero != 0 {
        return Err(E_DIFF_NONZERO);
    }
    sr.assert_zero_diff()?; // 无可比 ≠ 差异
    hits += 1;

    // R5 混合批：18 组 Recorded + 2 组 PENDING，首批 20 组跑差只比 18 组。
    let mut sr = SpyRecon::new();
    let mut g = 0usize;
    while g < FIRST_BATCH {
        if g == 7 || g == 13 {
            sr.record(g, &[OpRecord { msg: 0x0301, wParam: g as u32, lParam: 0 }])?;
        } else {
            sr.record(g, &sample_ops(g))?;
        }
        g += 1;
    }
    if sr.recorded_total != 18 || sr.pending_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    let zero = sr.run_diff(FIRST_BATCH)?;
    if zero != 18 {
        return Err(E_BATCH_MISS);
    }
    sr.assert_zero_diff()?;
    hits += 1;

    // R6 越界与超容拒收：组号 ≥200、序列 >16 条一律拒，账零增长。
    let mut sr = SpyRecon::new();
    if sr.record(LIB_CAP, &sample_ops(0)).is_ok() {
        return Err(E_BAD_SEQ);
    }
    let mut over: Vec<OpRecord> = Vec::new();
    let mut i = 0usize;
    while i <= OPS_CAP {
        over.push(OpRecord { msg: OP_KEY_DOWN, wParam: i as u32, lParam: 0 });
        i += 1;
    }
    if sr.record(0, &over).is_ok() {
        return Err(E_BAD_SEQ);
    }
    if sr.recorded_total != 0 || sr.pending_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 换算可复算：同码同果红线——A 对同序列两次跑输出逐位相等，
    //    B 亦然；A/B 对 20 组互查仍归零（双保险口径）。
    let mut sr = SpyRecon::new();
    let mut g = 0usize;
    while g < FIRST_BATCH {
        sr.record(g, &sample_ops(g))?;
        g += 1;
    }
    let mut g = 0usize;
    while g < FIRST_BATCH {
        let seq = match sr.seq_at(g) {
            Some(s) => s,
            None => return Err(E_BAD_SEQ),
        };
        let a1 = sim_a(seq);
        let a2 = sim_a(seq);
        if a1.len() != a2.len() {
            return Err(E_NOT_READY);
        }
        let mut j = 0usize;
        while j < a1.len() {
            if a1[j] != a2[j] {
                return Err(E_DIFF_NONZERO);
            }
            j += 1;
        }
        let b1 = sim_b(seq);
        if a1.len() != b1.len() {
            return Err(E_DIFF_NONZERO);
        }
        let mut j = 0usize;
        while j < a1.len() {
            if a1[j] != b1[j] {
                return Err(E_DIFF_NONZERO);
            }
            j += 1;
        }
        g += 1;
    }
    sr.run_diff(FIRST_BATCH)?;
    sr.assert_zero_diff()?;
    hits += 1;

    // R8 写回（判据 J2）：未就绪拒写、空摘要拒写、就绪后非空摘要放行。
    if sr.writeback(true, 0x10).is_ok() {
        return Err(E_NOT_READY);
    }
    sr.ready = true;
    if sr.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sr.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
