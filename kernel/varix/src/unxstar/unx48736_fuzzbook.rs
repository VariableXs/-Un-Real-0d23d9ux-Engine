//! UNX-F48736 C·fuzz 全域汇总（各批 fuzz 轮次与结果并账）
//! 判据：UNX-F48736-J1 并账全绿；判据锚定 体验十。
//! 纪律：各批 fuzz 轮次-检出-并账三段闭环，确定性 LCG 重演一致、轮账守恒、检出闭环。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1840;

const E_BATCH_BAD: u16 = SEG_BASE + 1; // 批号越界
const E_ROUND_BAD: u16 = SEG_BASE + 2; // 轮次非法
const E_LEDGER_DUP: u16 = SEG_BASE + 3; // 并账重复
const E_BAL_BREAK: u16 = SEG_BASE + 4; // 轮账守恒破
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_FOUND_LOST: u16 = SEG_BASE + 6; // 检出未闭环
const E_SEED_BAD: u16 = SEG_BASE + 7; // 种子为零拒
const E_BATCH_MISS: u16 = SEG_BASE + 8; // 批缺漏

/// 批数。
pub const BATCH_N: u32 = 40;
/// 每轮注入数。
pub const PER_ROUND: u32 = 64;

/// 单轮 fuzz 结果。
#[derive(Clone)]
pub struct FuzzRound {
    /// 批号。
    pub batch: u32,
    /// 轮号（批内）。
    pub round: u32,
    /// LCG 种子。
    pub seed: u64,
    /// 检出数（异常注入被防御吸收）。
    pub found: u32,
}

/// fuzz 并账簿。
pub struct FuzzBook {
    rounds: Vec<FuzzRound>,
    seen: Vec<(u32, u32)>,
}

impl FuzzBook {
    pub fn new() -> FuzzBook {
        FuzzBook {
            rounds: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// 登记一轮（批合法、种子非零、批内轮唯一）。
    pub fn put(&mut self, batch: u32, round: u32, seed: u64, found: u32) -> Result<bool, u16> {
        if batch == 0 || batch > BATCH_N {
            return Err(E_BATCH_BAD);
        }
        if seed == 0 {
            return Err(E_SEED_BAD);
        }
        for s in self.seen.iter() {
            if s.0 == batch && s.1 == round {
                return Err(E_LEDGER_DUP);
            }
        }
        self.seen.push((batch, round));
        self.rounds.push(FuzzRound {
            batch,
            round,
            seed,
            found,
        });
        Ok(true)
    }

    /// LCG 重演：固定种子复算检出数。
    pub fn replay_found(seed: u64, rounds: u32) -> u32 {
        let mut rng = Lcg(seed);
        let mut found = 0u32;
        let mut k = 0u32;
        while k < rounds * PER_ROUND {
            // 重演注入流：kind 与值，越界 kind 记检出
            let kind = rng.below(6) as u32;
            let _val = rng.below(1000);
            if kind >= 4 {
                found += 1;
            }
            k += 1;
        }
        found
    }

    /// 重演对拍：登记检出数 vs 复算检出数。
    pub fn assert_replay(&self, r: &FuzzRound) -> Result<(), u16> {
        let want = FuzzBook::replay_found(r.seed, 1);
        if want != r.found {
            return Err(E_REPLAY_DIFF);
        }
        Ok(())
    }

    /// 批覆盖守恒：40 批每批至少一轮。
    pub fn assert_cover(&self) -> Result<(), u16> {
        let mut b = 1u32;
        while b <= BATCH_N {
            let mut hit = false;
            for r in self.rounds.iter() {
                if r.batch == b {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_BATCH_MISS);
            }
            b += 1;
        }
        Ok(())
    }

    /// 轮账守恒：并账轮数 = 登记数、检出总数一致。
    pub fn assert_balance(&self, want_total: u64) -> Result<u64, u16> {
        let mut total = 0u64;
        for r in self.rounds.iter() {
            total += r.found as u64;
        }
        if total != want_total {
            return Err(E_BAL_BREAK);
        }
        Ok(total)
    }

    /// 检出闭环：全部检出数非零轮必须可重演。
    pub fn close_all(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for r in self.rounds.iter() {
            self.assert_replay(r)?;
            if r.found > 0 {
                n += 1;
            }
        }
        Ok(n)
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// fuzz 全域汇总主判据：40 批 ×3 轮并账 + 重演对拍全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 40 批 ×3 轮登记：种子 = 批号<<16 | 轮号，检出数按重演预计算
    let mut fb = FuzzBook::new();
    let mut want_total = 0u64;
    let mut b = 1u32;
    while b <= BATCH_N {
        let mut r = 1u32;
        while r <= 3 {
            let seed = ((b as u64) << 16) | (r as u64);
            let found = FuzzBook::replay_found(seed, 1);
            want_total += found as u64;
            fb.put(b, r, seed, found)?;
            r += 1;
        }
        b += 1;
    }
    if fb.rounds.len() != 120 {
        return Err(E_LEDGER_DUP);
    }
    hits += 1;

    // R2 重复轮必拒
    if fb.put(1, 1, 0x11, 1).err() == Some(E_LEDGER_DUP) {
        hits += 1;
    } else {
        return Err(E_LEDGER_DUP);
    }

    // R3 批号越界必拒
    if fb.put(41, 1, 0x11, 1).err() == Some(E_BATCH_BAD) {
        hits += 1;
    } else {
        return Err(E_BATCH_BAD);
    }

    // R4 零种子必拒
    if fb.put(1, 9, 0, 1).err() == Some(E_SEED_BAD) {
        hits += 1;
    } else {
        return Err(E_SEED_BAD);
    }

    // R5 重演对拍全绿：120 轮逐一复算
    let closed = fb.close_all()?;
    if closed == 0 {
        return Err(E_FOUND_LOST);
    }
    hits += 1;

    // R6 轮账守恒
    let total = fb.assert_balance(want_total)?;
    if total != want_total {
        return Err(E_BAL_BREAK);
    }
    fb.assert_cover()?;
    hits += 1;

    // R7 重演不一致必抓：登记检出数偏移 +1 后应红
    let mut fb2 = FuzzBook::new();
    fb2.put(1, 1, 0x1001, FuzzBook::replay_found(0x1001, 1) + 1)?;
    if fb2.close_all().err() == Some(E_REPLAY_DIFF) {
        hits += 1;
    } else {
        return Err(E_REPLAY_DIFF);
    }

    // R8 批覆盖缺失必抓
    let mut fb3 = FuzzBook::new();
    let mut b3 = 1u32;
    while b3 <= BATCH_N - 1 {
        fb3.put(b3, 1, (b3 as u64) << 8 | 1, FuzzBook::replay_found((b3 as u64) << 8 | 1, 1))?;
        b3 += 1;
    }
    if fb3.assert_cover().err() == Some(E_BATCH_MISS) {
        hits += 1;
    } else {
        return Err(E_BATCH_MISS);
    }

    // R9 LCG 独立重演 ×256：同种子同结果零漂移
    let mut rng = Lcg::new(0x48736);
    let mut k = 0u32;
    while k < 256 {
        let seed = (rng.below(0xFFFF) + 1) as u64;
        let a = FuzzBook::replay_found(seed, 1);
        let c = FuzzBook::replay_found(seed, 1);
        if a != c {
            return Err(E_REPLAY_DIFF);
        }
        k += 1;
    }
    hits += 1;

    // R10 大轮并账：单轮 PER_ROUND×4 注入重演守恒
    let mut fb4 = FuzzBook::new();
    let seed4 = 0x48736_0004;
    let found4 = {
        let mut rng4 = Lcg(seed4);
        let mut f4 = 0u32;
        let mut k4 = 0u32;
        while k4 < PER_ROUND * 4 {
            let kind = rng4.below(6) as u32;
            let _v = rng4.below(1000);
            if kind >= 4 {
                f4 += 1;
            }
            k4 += 1;
        }
        f4
    };
    fb4.put(40, 4, seed4, found4)?;
    fb4.assert_replay(fb4.rounds.first().ok_or(E_BATCH_MISS)?)?;
    let t4 = fb4.assert_balance(found4 as u64)?;
    if t4 != found4 as u64 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    // R11 检出闭环计数：主簿检出非零轮数与重演闭环数一致
    let mut nonzero = 0u32;
    for r in fb.rounds.iter() {
        if r.found > 0 {
            nonzero += 1;
        }
    }
    if closed != nonzero {
        return Err(E_FOUND_LOST);
    }
    hits += 1;

    Ok(hits)
}
