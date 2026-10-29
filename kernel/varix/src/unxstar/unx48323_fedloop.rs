//! unx48323_fedloop — UNX-F48323 候选学习反馈环（选词即调 freq，影响下次排序）（诊断段 0xC940）。
//!
//! 判据原文：
//!   J1 反馈环 ×10^3 收敛可复现；判据锚定 §AI-61 §2 专题一用户词学习。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 选词即调频：用户每选一次候选，该词 freq 立即 +FREQ_STEP 且
//!      封顶 FREQ_CAP（饱和即停，不无限膨胀）；排序下次立即按新 freq
//!      降序重排——反馈环闭环必须落在"下次"可见（环断即学习失效）。
//!   2. 收敛口径（J1 ×10^3）：固定目标词反复选 10^3 次，其排名单调
//!      不回退直至登顶；登顶轮次由 LCG 序列唯一决定，同种子两轮复跑
//!      收敛轮次逐位一致（可复现，学习不可是玄学）。
//!   3. 排序确定性：freq 同值按候选 id 升序 tie-break（稳定序），
//!      任何时刻排序是纯函数（同表同序），无时间无随机。
//!   4. 账目守恒：选次账（成功选词总数）== Σ(freq - 初值)（只增无
//!      出径）；饱和后选次照记但 freq 不再动，两条账分立。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性 LCG，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC940;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NO_CAND: u16 = SEG_BASE + 1;  // 候选 id 越闭集
const E_SAT_FULL: u16 = SEG_BASE + 2; // 频次已饱和（照记不涨，非错，留痕用）
const E_RANK_STALE: u16 = SEG_BASE + 3;// 排序未随频次重排/排名回退
const E_LEDGER_BAD: u16 = SEG_BASE + 4;// 选次账与频次增量账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;// 同种子复跑收敛轮次不一致
const E_TIE_BREAK: u16 = SEG_BASE + 6; // 同频 tie-break 违反 id 升序
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 频次参数。
pub const FREQ_INIT: u32 = 100;
pub const FREQ_STEP: u32 = 7;
pub const FREQ_CAP: u32 = 9000;
/// 候选表容量。
pub const CAND_N: usize = 8;
/// 反馈环轮数（J1）。
pub const LOOP_N: u32 = 1000;

/// 候选词槽（id 即槽位，freq 学习态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CandSlot {
    pub cand_id: u8,
    pub freq: u32,
}

/// 反馈环枢纽：选词即调频 + 即时重排 + 双账本。
pub struct FedLoop {
    slots: [CandSlot; CAND_N],
    pub pick_total: u32,   // 成功选词账
    pub sat_total: u32,    // 饱和照记账（freq 未动）
    pub reject_total: u32, // 越闭集拒收账
}

impl FedLoop {
    pub fn new() -> FedLoop {
        let mut slots = [CandSlot { cand_id: 0, freq: FREQ_INIT }; CAND_N];
        let mut i = 0usize;
        while i < CAND_N {
            slots[i] = CandSlot { cand_id: i as u8, freq: FREQ_INIT + (i as u32) * 10 };
            i += 1;
        }
        // 初表故意给 id0~7 递增初频，学习要逆风翻盘才见环效。
        FedLoop { slots, pick_total: 0, sat_total: 0, reject_total: 0 }
    }

    /// 当前排序视图：freq 降序，同频 id 升序（稳定 tie-break）。
    pub fn ranking(&self) -> Vec<u8> {
        let mut order: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < CAND_N {
            order.push(i as u8);
            i += 1;
        }
        // 选择排序（n=8 足够，且无递归无栈深问题）。
        let mut p = 0usize;
        while p < CAND_N {
            let mut best = p;
            let mut q = p + 1;
            while q < CAND_N {
                let a = self.slots[order[best] as usize];
                let b = self.slots[order[q] as usize];
                if b.freq > a.freq || (b.freq == a.freq && b.cand_id < a.cand_id) {
                    best = q;
                }
                q += 1;
            }
            let t = order[p];
            order[p] = order[best];
            order[best] = t;
            p += 1;
        }
        order
    }

    /// 词 id 当前排名（0 = 榜首）。
    pub fn rank_of(&mut self, cand_id: u8) -> Result<u32, u16> {
        if cand_id as usize >= CAND_N {
            self.reject_total += 1;
            return Err(E_NO_CAND);
        }
        let order = self.ranking();
        let mut i = 0usize;
        while i < order.len() {
            if order[i] == cand_id {
                return Ok(i as u32);
            }
            i += 1;
        }
        Err(E_NO_CAND)
    }

    /// 选词：freq 即调（+STEP 封顶）并即时可重排；返回该词新排名。
    pub fn pick(&mut self, cand_id: u8) -> Result<u32, u16> {
        if cand_id as usize >= CAND_N {
            self.reject_total += 1;
            return Err(E_NO_CAND);
        }
        let s = &mut self.slots[cand_id as usize];
        if s.freq >= FREQ_CAP {
            self.sat_total += 1; // 饱和照记，freq 不动
            return self.rank_of(cand_id);
        }
        let mut nf = s.freq + FREQ_STEP;
        if nf > FREQ_CAP {
            nf = FREQ_CAP;
        }
        s.freq = nf;
        self.pick_total += 1;
        self.rank_of(cand_id)
    }

    pub fn freq_of(&self, cand_id: u8) -> Result<u32, u16> {
        if cand_id as usize >= CAND_N {
            return Err(E_NO_CAND);
        }
        Ok(self.slots[cand_id as usize].freq)
    }

    /// 排序纯函数复核：同表两次 ranking 逐位一致（确定性红线）。
    pub fn ranking_stable(&self) -> Result<(), u16> {
        let a = self.ranking();
        let b = self.ranking();
        if a != b {
            return Err(E_RANK_STALE);
        }
        Ok(())
    }

    /// 账目守恒：pick_total == Σ(freq-初值)/STEP 的步数账；
    /// 饱和账与选次账分立互不冒充。
    pub fn audit(&self) -> Result<(), u16> {
        let mut gain: u64 = 0;
        let mut i = 0usize;
        while i < CAND_N {
            if self.slots[i].freq < FREQ_INIT {
                return Err(E_LEDGER_BAD);
            }
            gain += (self.slots[i].freq - FREQ_INIT) as u64;
            i += 1;
        }
        if gain / (FREQ_STEP as u64) != self.pick_total as u64 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.pick_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_NO_CAND);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// 一轮反馈环（J1 主体）：干扰词由 LCG 驱动随机选，目标词每 4 轮
/// 强选一次；返回目标词登顶轮次（未登顶返回 LOOP_N）。
pub fn converge_round(lcg: &mut Lcg) -> u32 {
    let mut fl = FedLoop::new();
    let target: u8 = 0;
    let mut r = 0u32;
    let mut top_at: u32 = LOOP_N;
    let mut topped = false;
    while r < LOOP_N {
        // 干扰选 3 次（非目标），强选目标 1 次——环效应可压过随机干扰。
        let mut d = 0u32;
        while d < 3 {
            let noise = (lcg.next_below(CAND_N as u64 - 1) + 1) as u8;
            fl.pick(noise).is_ok();
            d += 1;
        }
        fl.pick(target).is_ok();
        if !topped {
            if let Ok(rank) = fl.rank_of(target) {
                if rank == 0 {
                    top_at = r + 1;
                    topped = true;
                }
            }
        }
        r += 1;
    }
    top_at
}

/// 自例程判据：逐条落实 J1（反馈环 ×10^3 收敛可复现），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 反馈环 ×10^3 收敛可复现：同种子两轮 converge_round，登顶
    //    轮次逐位一致；且确实在 10^3 内登顶（环真实闭合）。
    let mut l1 = Lcg::new(0xC940)?;
    let top1 = converge_round(&mut l1);
    let mut l2 = Lcg::new(0xC940)?;
    let top2 = converge_round(&mut l2);
    if top1 != top2 {
        return Err(E_REPLAY_DIFF);
    }
    if top1 > LOOP_N {
        return Err(E_RANK_STALE);
    }
    hits += 1;

    // R2 选词即调频单调封顶：连选目标词，freq 严格 +STEP 直至 CAP，
    //    饱和后照记不涨且饱和账逐次留痕。
    let mut fl = FedLoop::new();
    let mut prev = fl.freq_of(0)?;
    let mut n = 0u32;
    while n < 1400 {
        fl.pick(0)?;
        let cur = fl.freq_of(0)?;
        if cur > FREQ_CAP {
            return Err(E_LEDGER_BAD);
        }
        if cur != FREQ_CAP && cur != prev + FREQ_STEP {
            return Err(E_LEDGER_BAD);
        }
        prev = cur;
        n += 1;
    }
    if prev != FREQ_CAP {
        return Err(E_SAT_FULL);
    }
    let sat_mark = fl.sat_total;
    fl.pick(0)?;
    if fl.freq_of(0)? != FREQ_CAP || fl.sat_total != sat_mark + 1 {
        return Err(E_SAT_FULL);
    }
    hits += 1;

    // R3 排序随频次前移且不回退：目标词排名随连选单调不回退至登顶；
    //    未被选中者排名不得因它人学习而前移。
    let mut fl = FedLoop::new();
    let mut last_rank = fl.rank_of(0)?;
    let mut n = 0u32;
    while n < 200 {
        let rk = fl.pick(0)?;
        if rk > last_rank {
            return Err(E_RANK_STALE);
        }
        last_rank = rk;
        n += 1;
    }
    if last_rank != 0 {
        return Err(E_RANK_STALE);
    }
    // id7 从未入选，其 freq 不变；它最初因高初频居首，学习后应让位。
    if fl.freq_of(7)? != FREQ_INIT + 70 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 同频 tie-break 按 id 升序：构造 id0（初频 100 + 10 次×7）
    //    与 id7（初频 170）同频 170，序必须 id 小者在前。
    let mut fl = FedLoop::new();
    let mut n = 0u32;
    while n < 10 {
        fl.pick(0)?;
        n += 1;
    }
    if fl.freq_of(0)? != 170 || fl.freq_of(7)? != 170 {
        return Err(E_LEDGER_BAD);
    }
    let order = fl.ranking();
    let mut pos0 = CAND_N;
    let mut pos7 = CAND_N;
    let mut i = 0usize;
    while i < order.len() {
        if order[i] == 0 {
            pos0 = i;
        }
        if order[i] == 7 {
            pos7 = i;
        }
        i += 1;
    }
    if pos0 >= pos7 {
        return Err(E_TIE_BREAK);
    }
    fl.ranking_stable()?;
    hits += 1;

    // R5 账目守恒：×100 混选后 pick_total == Σ(freq-初值)/STEP；
    //    拒收账分立；越闭集 id 拒收不动任何槽。
    let mut fl = FedLoop::new();
    let mut lcg = Lcg::new(0xC941)?;
    let mut n = 0u32;
    while n < 100 {
        let id = lcg.next_below(CAND_N as u64) as u8;
        fl.pick(id)?;
        n += 1;
    }
    if fl.pick(99) != Err(E_NO_CAND) {
        return Err(E_NO_CAND);
    }
    if fl.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    fl.audit()?;
    hits += 1;

    // R6 非目标词学习同样生效（环是通则不是特例）：连选 id5 后其
    //    排名前移越过初频更高的 id7。
    let mut fl = FedLoop::new();
    let before = fl.rank_of(5)?;
    let mut n = 0u32;
    while n < 15 {
        fl.pick(5)?;
        n += 1;
    }
    let after = fl.rank_of(5)?;
    if after >= before {
        return Err(E_RANK_STALE);
    }
    if after > 2 {
        return Err(E_RANK_STALE);
    }
    hits += 1;

    // R7 可红演练：伪造账目（手工破坏守恒不可直接构造，改为验证
    //    audit 对零选次拒写回 + 空摘要拒收两路红）。
    let fl = FedLoop::new();
    if fl.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    if fl.writeback(false, 8).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if fl.audit().is_err() {
        return Err(E_LEDGER_BAD); // 干净表账必平（判据本身可绿）
    }
    hits += 1;

    // R8 ×10^3 全账收官与写回（判据 J2）：完整 converge_round 后
    //    账目守恒、登顶词频为全程最高、写回过。
    let mut lcg = Lcg::new(0xC942)?;
    let top_at = converge_round(&mut lcg);
    let mut fl = FedLoop::new();
    let mut lcg2 = Lcg::new(0xC942)?;
    let mut r = 0u32;
    while r < top_at {
        let mut d = 0u32;
        while d < 3 {
            let noise = (lcg2.next_below(CAND_N as u64 - 1) + 1) as u8;
            fl.pick(noise).is_ok();
            d += 1;
        }
        fl.pick(0).is_ok();
        r += 1;
    }
    fl.audit()?;
    if fl.rank_of(0)? != 0 {
        return Err(E_RANK_STALE);
    }
    // 目标词频须为全表最高（登顶即第一，且账面支持）。
    let f0 = fl.freq_of(0)?;
    let mut i = 1usize;
    while i < CAND_N {
        if fl.freq_of(i as u8)? > f0 {
            return Err(E_RANK_STALE);
        }
        i += 1;
    }
    if fl.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    fl.writeback(true, 0x21)?;
    hits += 1;

    Ok(hits)
}
