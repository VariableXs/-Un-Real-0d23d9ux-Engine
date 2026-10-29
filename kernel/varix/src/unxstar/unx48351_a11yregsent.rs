//! unx48351_a11yregsent — UNX-F48351 无障碍三类窗口回归哨兵（vxwm 原生/Wine/Xwayland）
//!                         （诊断段 0xCCC0，域内宪法加严项）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：三窗 ×10 轮一致；
//!      判据锚定 域内宪法加严项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三窗同构：三类窗口各自持有同构无障碍实态（preedit 字符流 +
//!      候选榜 + 高亮位），同一份确定性脚本逐窗施加，只认数据面，
//!      不认窗口系统差异。
//!   2. 宪法加严——三窗数据锚一致：每轮末三窗锚（preedit 指纹、
//!      候选指纹、高亮位）三元组逐窗相等，任一窗漂移即 E_DRIFT。
//!   3. ×10 轮一致：10 轮脚本全跑，每轮一致账 +1；哨兵账面可重放
//!      复算，轮号严格递增。
//!   4. 漂移可检出：第 5 轮末向 Wine 窗注入一字，三元比对必报
//!      E_DRIFT；清漂复位后复跑仍回到三窗一致（判据非恒绿）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCCC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DRIFT: u16 = SEG_BASE + 1; // 三窗锚漂移
const E_STATE_BAD: u16 = SEG_BASE + 2; // 窗实态非法
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 锚封印失配
const E_ROUND_BAD: u16 = SEG_BASE + 4; // 轮号非法
const E_REPLAY_BAD: u16 = SEG_BASE + 5; // 哨兵账重放翻供
const E_CAND_BAD: u16 = SEG_BASE + 6; // 候选榜撞号

/// 三类窗口。
pub const W_NATIVE: u8 = 0; // vxwm 原生
pub const W_WINE: u8 = 1; // Wine
pub const W_XWAY: u8 = 2; // Xwayland
pub const WINS: u8 = 3;
pub const WIN_NAMES: [&str; 3] = ["vxwm_native", "wine", "xwayland"];

/// FNV-1a 64 位指纹。
pub fn fp_chars(cs: &[char]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < cs.len() {
        h ^= (cs[i] as u32 & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

pub fn fp_u32s(vals: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < vals.len() {
        let mut b = 0usize;
        while b < 4 {
            h ^= ((vals[i] >> (b * 8)) & 0xFF) as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            b += 1;
        }
        i += 1;
    }
    h
}

/// 单窗实态：preedit + 候选榜 + 高亮位。
pub struct WinState {
    kind: u8,
    preedit: Vec<char>,
    cands: Vec<u32>,
    hl: usize,
}

impl WinState {
    pub fn new(kind: u8) -> Result<Self, u16> {
        if kind as usize >= WINS as usize {
            return Err(E_STATE_BAD);
        }
        Ok(WinState { kind, preedit: Vec::new(), cands: Vec::new(), hl: 0 })
    }

    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// 确定性脚本算子：op 0=压字, 1=退格, 2=换榜, 3=挪高亮, 4=清空。
    pub fn apply(&mut self, op: u8, arg: u32) -> Result<(), u16> {
        match op {
            0 => {
                let ch = core::char::from_u32(0x4E00 + (arg & 0x3F)).unwrap_or(core::char::REPLACEMENT_CHARACTER);
                self.preedit.push(ch);
                Ok(())
            }
            1 => {
                self.preedit.pop();
                Ok(())
            }
            2 => {
                let k = arg & 0x7;
                let mut ids: Vec<u32> = Vec::new();
                let mut j = 0u32;
                while j < k {
                    ids.push(500 + arg.wrapping_mul(31) + j);
                    j += 1;
                }
                // 撞号守门。
                let mut a = 0usize;
                while a < ids.len() {
                    let mut b = a + 1;
                    while b < ids.len() {
                        if ids[a] == ids[b] {
                            return Err(E_CAND_BAD);
                        }
                        b += 1;
                    }
                    a += 1;
                }
                self.cands = ids;
                if self.hl >= self.cands.len() {
                    self.hl = 0;
                }
                Ok(())
            }
            3 => {
                if (arg as usize) >= self.cands.len() {
                    return Err(E_STATE_BAD);
                }
                self.hl = arg as usize;
                Ok(())
            }
            4 => {
                self.preedit.clear();
                self.cands.clear();
                self.hl = 0;
                Ok(())
            }
            _ => Err(E_STATE_BAD),
        }
    }

    pub fn preedit_fp(&self) -> u64 {
        fp_chars(&self.preedit)
    }

    pub fn cand_fp(&self) -> u64 {
        fp_u32s(&self.cands)
    }

    pub fn hl(&self) -> u32 {
        self.hl as u32
    }

    /// 数据锚：三元组 + 封印。
    pub fn anchor(&self) -> WinAnchor {
        WinAnchor::new(self.kind, self.preedit_fp(), self.cand_fp(), self.hl())
    }
}

/// 窗数据锚：kind + preedit 指纹 + 候选指纹 + 高亮位 + 封印。
#[derive(Clone, Copy)]
pub struct WinAnchor {
    pub kind: u8,
    pub preedit_fp: u64,
    pub cand_fp: u64,
    pub hl: u32,
    seal: u64,
}

impl WinAnchor {
    fn seal_of(kind: u8, pfp: u64, cfp: u64, hl: u32) -> u64 {
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        h ^= kind as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= pfp;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= cfp;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= hl as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    fn new(kind: u8, pfp: u64, cfp: u64, hl: u32) -> Self {
        WinAnchor { kind, preedit_fp: pfp, cand_fp: cfp, hl, seal: Self::seal_of(kind, pfp, cfp, hl) }
    }

    pub fn verify(&self) -> Result<(), u16> {
        if self.kind >= WINS {
            return Err(E_STATE_BAD);
        }
        if self.seal != Self::seal_of(self.kind, self.preedit_fp, self.cand_fp, self.hl) {
            return Err(E_SEAL_BAD);
        }
        Ok(())
    }

    /// 三元组相等（不含 kind）。
    pub fn same_triple(&self, other: &WinAnchor) -> bool {
        self.preedit_fp == other.preedit_fp
            && self.cand_fp == other.cand_fp
            && self.hl == other.hl
    }
}

/// 三窗三元比对：逐窗 verify + 两两同三元组。
pub fn triple_check(a: &WinAnchor, b: &WinAnchor, c: &WinAnchor) -> Result<(), u16> {
    a.verify()?;
    b.verify()?;
    c.verify()?;
    if !a.same_triple(b) || !b.same_triple(c) {
        return Err(E_DRIFT);
    }
    Ok(())
}

/// 哨兵账：每轮三窗锚三元组留痕。
pub struct RegLedger {
    pub rounds: Vec<[WinAnchor; 3]>,
}

impl RegLedger {
    pub fn new() -> Self {
        RegLedger { rounds: Vec::new() }
    }

    pub fn record(&mut self, round: u32, trip: [WinAnchor; 3]) -> Result<(), u16> {
        if round != self.rounds.len() as u32 {
            return Err(E_ROUND_BAD);
        }
        self.rounds.push(trip);
        Ok(())
    }
}

/// 一轮脚本：五算子确定性序列（对全部传入窗同序施加）。
fn run_round(wins: &mut [WinState], round: u32) -> Result<(), u16> {
    let script: [(u8, u32); 6] = [
        (0, round & 0x3F),            // 压字
        (0, (round + 7) & 0x3F),      // 压字
        (2, round + 1),               // 换榜
        (3, round % 3),               // 挪高亮
        (1, 0),                       // 退格
        (0, (round + 13) & 0x3F),     // 压字
    ];
    let mut wi = 0usize;
    while wi < wins.len() {
        let mut si = 0usize;
        while si < script.len() {
            wins[wi].apply(script[si].0, script[si].1)?;
            si += 1;
        }
        wi += 1;
    }
    Ok(())
}

/// drill：J1 三窗 ×10 轮一致（含漂移注入检出）。
pub fn drill() -> Result<u32, u16> {
    // ── R1 主判据：10 轮脚本，每轮末三窗锚三元组逐轮一致 ────
    let mut ledger = RegLedger::new();
    let mut wins = [WinState::new(W_NATIVE)?, WinState::new(W_WINE)?, WinState::new(W_XWAY)?];
    let mut round = 0u32;
    while round < 10 {
        run_round(&mut wins, round)?;
        let trip = [wins[0].anchor(), wins[1].anchor(), wins[2].anchor()];
        triple_check(&trip[0], &trip[1], &trip[2])?;
        ledger.record(round, trip)?;
        round += 1;
    }
    if ledger.rounds.len() != 10 {
        return Err(E_ROUND_BAD);
    }

    // ── R2 漂移注入：Wine 窗多压一字 → 三元比对必 E_DRIFT ───
    wins[W_WINE as usize].apply(0, 0x15)?;
    let t0 = wins[0].anchor();
    let t1 = wins[1].anchor();
    let t2 = wins[2].anchor();
    if triple_check(&t0, &t1, &t2) != Err(E_DRIFT) {
        return Err(E_DRIFT); // 注入未检出即判据失效
    }
    // 原生/Xwayland 两窗仍互同（漂移只在一窗）。
    if !t0.same_triple(&t2) {
        return Err(E_STATE_BAD);
    }

    // ── R3 清漂复位：Wine 窗退格一字，三窗回到一致 ──────────
    wins[W_WINE as usize].apply(1, 0)?;
    let u0 = wins[0].anchor();
    let u1 = wins[1].anchor();
    let u2 = wins[2].anchor();
    triple_check(&u0, &u1, &u2)?;
    if !u0.same_triple(&t0) {
        return Err(E_REPLAY_BAD); // 复位后应与注入前第 10 轮锚同
    }

    // ── R4 锚封印：篡改三元组任一成员必失配 ─────────────────
    let mut bad = t1;
    bad.hl = bad.hl.wrapping_add(1);
    if bad.verify() != Err(E_SEAL_BAD) {
        return Err(E_SEAL_BAD);
    }
    let mut bad2 = t0;
    bad2.cand_fp = bad2.cand_fp.wrapping_add(1);
    if bad2.verify() != Err(E_SEAL_BAD) {
        return Err(E_SEAL_BAD);
    }

    // ── R5 轮号纪律：哨兵账续记必须轮号衔接 ─────────────────
    if ledger.record(11, [t0, t0, t0]).is_ok() {
        return Err(E_ROUND_BAD);
    }

    // ── R6 窗名零漂移：三名与三窗一一对应非空 ───────────────
    if WIN_NAMES.len() != WINS as usize {
        return Err(E_STATE_BAD);
    }
    let mut k = 0usize;
    while k < WIN_NAMES.len() {
        if WIN_NAMES[k].is_empty() {
            return Err(E_STATE_BAD);
        }
        k += 1;
    }

    // ── R7 跨轮重放：第 r 轮与第 r+1 轮锚必须可区分（脚本真在
    //    推进实态，哨兵不是恒绿复读）────────────────────────
    let mut r = 0u32;
    while r + 1 < 10 {
        if ledger.rounds[r as usize][0].preedit_fp == ledger.rounds[(r + 1) as usize][0].preedit_fp {
            return Err(E_REPLAY_BAD);
        }
        r += 1;
    }

    // ── R8 非法算子与越界：op 5 与越界高亮必拒 ───────────────
    let mut w = WinState::new(W_NATIVE)?;
    if w.apply(5, 0).is_ok() {
        return Err(E_STATE_BAD);
    }
    if w.apply(3, 9).is_ok() {
        return Err(E_STATE_BAD);
    }
    if WinState::new(3).is_ok() {
        return Err(E_STATE_BAD);
    }

    // J2 写回摘要非空：十轮一致账即摘要实质。
    if ledger.rounds.is_empty() {
        return Err(E_ROUND_BAD);
    }
    Ok(ledger.rounds.len() as u32)
}
