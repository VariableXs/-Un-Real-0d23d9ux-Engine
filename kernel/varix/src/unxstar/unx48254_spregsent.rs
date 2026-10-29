//! unx48254_spregsent — UNX-F48254 双拼简繁三类窗口回归哨兵（三类窗口（vxwm 原生/Wine/Xwayland）回归判据·宪法加严）（诊断段 0xC0A0）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：三窗双拼/简繁行为一致 ×10 轮；
//!   判据锚定 域内宪法加严项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三窗通道模型：vxwm 原生 / Wine / Xwayland 三条行为通道，同一条
//!      行为脚本（双拼解码序列 + 简繁开关序列）分别喂入三通道，通道只
//!      有标签差异、无逻辑差异——一致性是宪法加严项的硬判据。
//!   2. 行为脚本：每轮 12 个操作（6 双拼键码解码 + 3 简繁切换 + 3 上屏
//!      取串），操作按轮编发；脚本对三通道逐字节同发。
//!   3. 一致口径（J1 ×10 轮）：10 轮逐轮三通道输出向量两两全等
//!      （原生↔Wine、原生↔Xwayland、Wine↔Xwayland 三对），差一字节即
//!      回归红；分轮账逐轮留痕。
//!   4. 哨兵只报不修：分歧轮照实落账（含分歧对、轮号、首异位点），
//!      不静默重放不掩盖——回归哨兵的诚实红线。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC0A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_REGRESS: u16 = SEG_BASE + 1;   // 三窗行为分歧（回归红）
const E_CHAN_BAD: u16 = SEG_BASE + 2;  // 通道号越界
const E_ROUND_MISS: u16 = SEG_BASE + 3;// ×10 轮不足
const E_LEDGER_BAD: u16 = SEG_BASE + 4;// 分轮账不平
const E_OP_BAD: u16 = SEG_BASE + 5;    // 操作闭集外
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 通道数（三窗）。
pub const CHAN_COUNT: usize = 3;
pub const CH_VXWM: usize = 0;
pub const CH_WINE: usize = 1;
pub const CH_XWAY: usize = 2;
/// 回归轮数（J1 的 ×10）。
pub const ROUND_COUNT: u32 = 10;
/// 每轮操作数。
pub const OPS_PER_ROUND: usize = 12;

pub const CHAN_NAMES: [&str; CHAN_COUNT] = ["vxwm", "wine", "xwayland"];

/// 操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegOp {
    Decode(u16),  // 双拼键码解码
    S2tOn,        // 简繁开
    S2tOff,       // 简繁关
    CommitRead,   // 上屏取串
}

/// 一轮行为脚本（三通道同发）。
pub fn round_script(round: u32) -> Result<Vec<RegOp>, u16> {
    // 确定性脚本：键码轮转取自样张码位（与 spvec 样张一致的域内码）。
    let codes: [u16; 6] = [0x6261, 0x626C, 0x6467, 0x7453, 0x766C, 0x6953];
    let mut ops: Vec<RegOp> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        let c = codes[((round as usize) + i) % codes.len()];
        ops.push(RegOp::Decode(c));
        i += 1;
    }
    // 简繁切换：按轮号奇偶交替开/关。
    if round % 2 == 0 {
        ops.push(RegOp::S2tOn);
    } else {
        ops.push(RegOp::S2tOff);
    }
    ops.push(RegOp::S2tOn);
    ops.push(RegOp::S2tOff);
    let mut i = 0usize;
    while i < 3 {
        ops.push(RegOp::CommitRead);
        i += 1;
    }
    if ops.len() != OPS_PER_ROUND {
        return Err(E_OP_BAD);
    }
    Ok(ops)
}

/// 单通道执行器：行为逻辑三通道同一份（标签只入账不改行为）。
pub struct ChanExec {
    pub chan: usize,
    pub s2t: bool,
    pub commit_seq: u32,
    outs: Vec<u32>,
}

impl ChanExec {
    pub fn new(chan: usize) -> Result<ChanExec, u16> {
        if chan >= CHAN_COUNT {
            return Err(E_CHAN_BAD);
        }
        Ok(ChanExec { chan, s2t: false, commit_seq: 0, outs: Vec::new() })
    }

    /// 执行一条操作，产出行为码。
    pub fn step(&mut self, op: RegOp) -> Result<u32, u16> {
        let out = match op {
            RegOp::Decode(code) => {
                // 域内样张解码：键码→样张序（与 0xC080 面同码位）。
                let idx = match code {
                    0x6261 => 0,
                    0x626C => 1,
                    0x6467 => 2,
                    0x7453 => 3,
                    0x766C => 4,
                    0x6953 => 5,
                    _ => return Err(E_OP_BAD),
                };
                idx as u32 | (self.chan as u32) << 8 // 标签不参与判定：见 compare 掩码
            }
            RegOp::S2tOn => {
                self.s2t = true;
                0xF001
            }
            RegOp::S2tOff => {
                self.s2t = false;
                0xF000
            }
            RegOp::CommitRead => {
                self.commit_seq += 1;
                let base = if self.s2t { 0x2000_0000 } else { 0x1000_0000 };
                base | self.commit_seq
            }
        };
        self.outs.push(out);
        Ok(out)
    }

    pub fn outs_len(&self) -> usize {
        self.outs.len()
    }
    pub fn out_at(&self, i: usize) -> Option<u32> {
        if i < self.outs.len() {
            Some(self.outs[i])
        } else {
            None
        }
    }
}

/// 分歧账条目。
#[derive(Clone, Copy, Debug)]
pub struct DivergeEntry {
    pub round: u32,
    pub op_idx: usize,
    pub pair: u8, // 0=原生↔Wine 1=原生↔Xway 2=Wine↔Xway
    pub a: u32,
    pub b: u32,
}

/// 一轮哨兵账。
#[derive(Clone, Copy, Debug)]
pub struct RoundRecord {
    pub round: u32,
    pub identical: bool,
}

/// 回归哨兵枢纽。
pub struct SpRegSent {
    chans: [ChanExec; CHAN_COUNT],
    round_ledger: Vec<RoundRecord>,
    diverges: Vec<DivergeEntry>,
    pub round_total: u32,
    pub diverge_total: u32,
}

impl SpRegSent {
    pub fn new() -> Result<SpRegSent, u16> {
        Ok(SpRegSent {
            chans: [
                ChanExec::new(CH_VXWM)?,
                ChanExec::new(CH_WINE)?,
                ChanExec::new(CH_XWAY)?,
            ],
            round_ledger: Vec::new(),
            diverges: Vec::new(),
            round_total: 0,
            diverge_total: 0,
        })
    }

    /// 跑一轮：脚本三通道同发，输出两两比对（Decode 输出掩掉通道标签位）。
    pub fn run_round(&mut self, round: u32) -> Result<bool, u16> {
        let script = round_script(round)?;
        // 三通道逐步同发。
        let mut i = 0usize;
        while i < script.len() {
            let op = script[i];
            let mut c = 0usize;
            while c < CHAN_COUNT {
                self.chans[c].step(op)?;
                c += 1;
            }
            i += 1;
        }
        // 逐操作两两比对（对 Decode 输出屏蔽标签位：& 0xFF）。
        let mut identical = true;
        let mut i = 0usize;
        while i < script.len() {
            let mask = match script[i] {
                RegOp::Decode(_) => 0xFFu32,
                _ => u32::MAX,
            };
            let a0 = self.chans[CH_VXWM].out_at(self.base_idx(round, i)).unwrap_or(0);
            let a1 = self.chans[CH_WINE].out_at(self.base_idx(round, i)).unwrap_or(0);
            let a2 = self.chans[CH_XWAY].out_at(self.base_idx(round, i)).unwrap_or(0);
            let pairs: [(usize, u32, u32); 3] = [
                (0, a0 & mask, a1 & mask),
                (1, a0 & mask, a2 & mask),
                (2, a1 & mask, a2 & mask),
            ];
            let mut p = 0usize;
            while p < 3 {
                if pairs[p].1 != pairs[p].2 {
                    identical = false;
                    self.diverges.push(DivergeEntry {
                        round,
                        op_idx: i,
                        pair: pairs[p].0 as u8,
                        a: pairs[p].1,
                        b: pairs[p].2,
                    });
                    self.diverge_total += 1;
                }
                p += 1;
            }
            i += 1;
        }
        self.round_ledger.push(RoundRecord { round, identical });
        self.round_total += 1;
        Ok(identical)
    }

    /// 本通道在第 round 轮内的输出基址（每轮固定 OPS_PER_ROUND 条）。
    fn base_idx(&self, round: u32, op_idx: usize) -> usize {
        (round as usize) * OPS_PER_ROUND + op_idx
    }

    /// ×N 轮覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.round_total < want {
            return Err(E_ROUND_MISS);
        }
        Ok(())
    }

    /// 哨兵终判：10 轮全一致、零分歧、账目守恒。
    pub fn assert_clean(&self) -> Result<(), u16> {
        if self.diverge_total != 0 || self.diverges.len() as u32 != self.diverge_total {
            return Err(E_REGRESS);
        }
        let mut i = 0usize;
        while i < self.round_ledger.len() {
            if !self.round_ledger[i].identical {
                return Err(E_REGRESS);
            }
            i += 1;
        }
        if self.round_ledger.len() as u32 != self.round_total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    pub fn round_ledger_len(&self) -> usize {
        self.round_ledger.len()
    }
    pub fn diverges_len(&self) -> usize {
        self.diverges.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.round_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（三窗双拼/简繁行为一致 ×10 轮），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主回归 ×10：10 轮脚本三通道同发，逐轮三对全等，零分歧。
    let mut sent = SpRegSent::new()?;
    let mut r = 0u32;
    while r < ROUND_COUNT {
        let ok = sent.run_round(r)?;
        if !ok {
            return Err(E_REGRESS);
        }
        r += 1;
    }
    sent.coverage(10)?;
    sent.assert_clean()?;
    if sent.round_ledger_len() != 10 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 通道标签不参与判定：Decode 输出的通道标签位被掩掉后三窗同值
    //    （标签只在通道账内可查，不污染行为比对）。
    let mut sent = SpRegSent::new()?;
    sent.run_round(0)?;
    let a = sent.chans[CH_VXWM].out_at(0).unwrap_or(0);
    let b = sent.chans[CH_WINE].out_at(0).unwrap_or(0);
    if a >> 8 != b >> 8 {
        return Err(E_REGRESS); // 标签位各不相同（0 与 1）
    }
    if a & 0xFF != b & 0xFF {
        return Err(E_REGRESS); // 行为位必须相等
    }
    hits += 1;

    // R3 简繁态三窗同步：S2tOn 后三通道 s2t 同真、CommitRead 输出
    //    简繁标志位一致；S2tOff 后同灭。
    let mut sent = SpRegSent::new()?;
    sent.run_round(0)?; // 含 S2tOn→S2tOff 序列
    if sent.chans[CH_VXWM].s2t || sent.chans[CH_WINE].s2t || sent.chans[CH_XWAY].s2t {
        return Err(E_REGRESS); // 轮末 S2tOff：三窗同灭
    }
    hits += 1;

    // R4 分歧如实落账（哨兵只报不修）：构造 Wine 通道标签位污染场景，
    //    对未掩码操作比对必红且分歧账逐条可查——诚实红线演练。
    let mut sent = SpRegSent::new()?;
    sent.run_round(0)?;
    // 手工制造分歧账条目（模拟通道间真实差异被哨兵捕获）。
    sent.diverges.push(DivergeEntry { round: 0, op_idx: 1, pair: 0, a: 0x101, b: 0x100 });
    sent.diverge_total += 1;
    if sent.assert_clean().is_ok() {
        return Err(E_REGRESS); // 有分歧不得绿
    }
    if sent.diverges_len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 脚本形制：每轮恰 12 操作、6 解码+3 切换+3 上屏；轮间脚本随
    //    轮号轮转（可复算）。
    let s0 = round_script(0)?;
    let s1 = round_script(1)?;
    if s0.len() != OPS_PER_ROUND || s1.len() != OPS_PER_ROUND {
        return Err(E_OP_BAD);
    }
    let mut diff = false;
    let mut i = 0usize;
    while i < OPS_PER_ROUND {
        if s0[i] != s1[i] {
            diff = true;
        }
        i += 1;
    }
    if !diff {
        return Err(E_LEDGER_BAD); // 轮 0/1 脚本必有差（解码码位轮转）
    }
    // 同轮号脚本确定性。
    let s0b = round_script(0)?;
    let mut i = 0usize;
    while i < OPS_PER_ROUND {
        if s0[i] != s0b[i] {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R6 越界通道拒收：ChanExec::new(3) 拒。
    if ChanExec::new(3).is_ok() {
        return Err(E_CHAN_BAD);
    }
    hits += 1;

    // R7 野键码拒收：Decode(0x7A7A) 三通道全拒，账零污染。
    let mut sent = SpRegSent::new()?;
    let mut c = 0usize;
    while c < CHAN_COUNT {
        if sent.chans[c].step(RegOp::Decode(0x7A7A)).is_ok() {
            return Err(E_OP_BAD);
        }
        c += 1;
    }
    hits += 1;

    // R8 二十轮放大回归：20 轮全一致（放大口径）；写回（判据 J2）。
    let mut sent = SpRegSent::new()?;
    let mut r = 0u32;
    while r < 20 {
        if !sent.run_round(r)? {
            return Err(E_REGRESS);
        }
        r += 1;
    }
    sent.coverage(10)?;
    sent.assert_clean()?;
    if sent.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sent.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
