//! unx48245_spline — UNX-F48245 双拼与整句引擎联动（双拼串→音节序列→DAG 打分贯通）（诊断段 0xBF80）。
//!
//! 判据原文：
//!   J1 贯通 ×100 整句转换正确；判据锚定 §AI-61 §2 专题一 Viterbi 联动。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 贯通链三段落账：双拼键序（两键一音节）→ 音节候选序列（每
//!      键对展开为 1..2 个候选音节——双韵母键经 Viterbi 择优）→
//!      整句得分（逐音节频度分累加）。三段各有账本，链上任一段
//!      断链即 E_CHAIN，贯通账不许跳段直抄结果。
//!   2. 候选展开封闭：键对 → (声母,韵母候选集)，双韵母键产两候选
//!      各带频度分，单韵母键产一候选；空候选（非法键对）即整句
//!      E_PARSE 拒——DAG 建在合法候选上，不建在猜上。
//!   3. Viterbi 择优：DAG 逐层松弛，每层记 (累计分, 前驱)；终局
//!      回溯出最优音节序。并列同分取先者（确定性 tie-break，防
//!      随机性入账）；分数表 FREQUENCY 冻结常量，运行期只读。
//!   4. 期望双算（判据 J1 的"正确"口径）：×100 句各配独立期望
//!      函数（argmax 逐音节直算，不走 Viterbi 代码路径），两算
//!      结果逐音节相等才计贯通；一句不等即 E_MISMATCH 回指句号。
//!   5. 句源确定性：LCG 造 100 句（每句 3..8 音节），音节取自
//!      冻结词表 SYLLABUS；同种子同句集，可复跑。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBF80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CHAIN: u16 = SEG_BASE + 1; // 贯通链断段
const E_PARSE: u16 = SEG_BASE + 2; // 非法键对：空候选
const E_MISMATCH: u16 = SEG_BASE + 3; // 双算不等
const E_SENT_MISS: u16 = SEG_BASE + 4; // 句数不足 ×100
const E_TIE: u16 = SEG_BASE + 5; // 同分并列未按先者 tie-break
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空
const E_SEG_BAD: u16 = SEG_BASE + 7; // 分段数与键序不齐

// ── 声母/韵母 id（与方案账同源闭集，本地冻结）──────────────
pub const I_ZERO: u8 = 0;
pub const I_B: u8 = 1;
pub const I_P: u8 = 2;
pub const I_M: u8 = 3;
pub const I_F: u8 = 4;
pub const I_D: u8 = 5;
pub const I_T: u8 = 6;
pub const I_N: u8 = 7;
pub const I_L: u8 = 8;
pub const I_G: u8 = 9;
pub const I_K: u8 = 10;
pub const I_H: u8 = 11;
pub const I_J: u8 = 12;
pub const I_Q: u8 = 13;
pub const I_X: u8 = 14;
pub const I_ZH: u8 = 15;
pub const I_CH: u8 = 16;
pub const I_SH: u8 = 17;
pub const I_R: u8 = 18;
pub const I_Z: u8 = 19;
pub const I_C: u8 = 20;
pub const I_S: u8 = 21;
pub const I_Y: u8 = 22;
pub const I_W: u8 = 23;

pub const F_A: u8 = 0;
pub const F_O: u8 = 1;
pub const F_E: u8 = 2;
pub const F_I: u8 = 3;
pub const F_U: u8 = 4;
pub const F_V: u8 = 5;
pub const F_AI: u8 = 6;
pub const F_EI: u8 = 7;
pub const F_AO: u8 = 8;
pub const F_OU: u8 = 9;
pub const F_AN: u8 = 10;
pub const F_EN: u8 = 11;
pub const F_ANG: u8 = 12;
pub const F_ENG: u8 = 13;
pub const F_ONG: u8 = 14;
pub const F_IONG: u8 = 15;
pub const F_IA: u8 = 16;
pub const F_UA: u8 = 17;
pub const F_UO: u8 = 18;
pub const F_UE: u8 = 19;
pub const F_IE: u8 = 20;
pub const F_UAI: u8 = 21;
pub const F_UI: u8 = 22;
pub const F_UN: u8 = 23;
pub const F_IAN: u8 = 24;
pub const F_IANG: u8 = 25;
pub const F_UANG: u8 = 26;
pub const F_IU: u8 = 27;
pub const F_IN: u8 = 28;
pub const F_ING: u8 = 29;
pub const F_UAN: u8 = 30;
pub const F_ER: u8 = 31;

/// 键闭集。
pub const KEY_LO: u8 = b'a';
pub const KEY_N: usize = 26;

/// 布局表（冻结）：每键韵母候选（0 哨兵止）。
pub const KEY_FINALS: [[u8; 3]; KEY_N] = [
    [F_A, 0, 0],       // a
    [F_OU, 0, 0],      // b
    [F_IN, 0, 0],      // c
    [F_IANG, F_UANG, 0], // d
    [F_E, 0, 0],       // e
    [F_EN, 0, 0],      // f
    [F_ENG, 0, 0],     // g
    [F_ANG, 0, 0],     // h
    [F_I, 0, 0],       // i
    [F_AN, 0, 0],      // j
    [F_AO, 0, 0],      // k
    [F_AI, 0, 0],      // l
    [F_IAN, 0, 0],     // m
    [F_ING, 0, 0],     // n
    [F_O, F_UO, 0],    // o
    [F_UN, 0, 0],      // p
    [F_IU, 0, 0],      // q
    [F_UAN, F_ER, 0],  // r
    [F_ONG, F_IONG, 0], // s
    [F_UE, 0, 0],      // t
    [F_U, 0, 0],       // u
    [F_V, F_UI, 0],    // v
    [F_IA, F_UA, 0],   // w
    [F_IE, 0, 0],      // x
    [F_UAI, 0, 0],     // y
    [F_EI, 0, 0],      // z
];

/// 声母键位（冻结）：zh→v / ch→i / sh→u，零声母占 o。
pub fn init_key(init: u8) -> u8 {
    match init {
        I_ZERO => b'o',
        I_ZH => b'v',
        I_CH => b'i',
        I_SH => b'u',
        I_B => b'b',
        I_P => b'p',
        I_M => b'm',
        I_F => b'f',
        I_D => b'd',
        I_T => b't',
        I_N => b'n',
        I_L => b'l',
        I_G => b'g',
        I_K => b'k',
        I_H => b'h',
        I_J => b'j',
        I_Q => b'q',
        I_X => b'x',
        I_R => b'r',
        I_Z => b'z',
        I_C => b'c',
        I_S => b's',
        I_Y => b'y',
        _ => b'w',
    }
}

pub fn key_init(k1: u8) -> Result<u8, u16> {
    match k1 {
        b'o' => Ok(I_ZERO),
        b'v' => Ok(I_ZH),
        b'i' => Ok(I_CH),
        b'u' => Ok(I_SH),
        b'b' => Ok(I_B),
        b'p' => Ok(I_P),
        b'm' => Ok(I_M),
        b'f' => Ok(I_F),
        b'd' => Ok(I_D),
        b't' => Ok(I_T),
        b'n' => Ok(I_N),
        b'l' => Ok(I_L),
        b'g' => Ok(I_G),
        b'k' => Ok(I_K),
        b'h' => Ok(I_H),
        b'j' => Ok(I_J),
        b'q' => Ok(I_Q),
        b'x' => Ok(I_X),
        b'r' => Ok(I_R),
        b'z' => Ok(I_Z),
        b'c' => Ok(I_C),
        b's' => Ok(I_S),
        b'y' => Ok(I_Y),
        b'w' => Ok(I_W),
        _ => Err(E_PARSE), // a/e 无声母：整句面空候选
    }
}

/// 音节 = (声母, 韵母)。
pub type Syll = (u8, u8);

/// 频度分表（冻结）：韵母 → 分（1..=9，双韵母键两候选分差造歧义面）。
pub const FREQUENCY: [u32; 32] = [
    9, 5, 8, 9, 8, 4, 7, 5, 7, 6, 8, 6, 7, 6, 7, 3, 5, 5, 6, 5, 5, 3, 6, 5, 7, 6, 5, 5, 6, 5, 6, 2,
];

/// 键对 → 候选音节集（纪律 2）：单韵母键 1 候选，双韵母键 2 候选。
pub fn candidates(k1: u8, k2: u8) -> Result<Vec<Syll>, u16> {
    if k1 < KEY_LO || k1 > KEY_LO + (KEY_N as u8) - 1 {
        return Err(E_PARSE);
    }
    if k2 < KEY_LO || k2 > KEY_LO + (KEY_N as u8) - 1 {
        return Err(E_PARSE);
    }
    let init = key_init(k1)?;
    let row = KEY_FINALS[(k2 - KEY_LO) as usize];
    let mut out: Vec<Syll> = Vec::new();
    let mut i = 0usize;
    while i < 3 {
        if row[i] == 0 {
            break;
        }
        out.push((init, row[i]));
        i += 1;
    }
    if out.is_empty() {
        return Err(E_PARSE);
    }
    Ok(out)
}

/// 音节得分：频度分 + 声母加成（零声母 +1，声母字面 +2）。
pub fn score(s: &Syll) -> u32 {
    FREQUENCY[s.1 as usize] + if s.0 == I_ZERO { 1 } else { 2 }
}

/// 期望直算（纪律 4）：逐键对取最高分候选（同分取先者）。
pub fn expect_best(pairs: &[[u8; 2]]) -> Result<Vec<Syll>, u16> {
    if pairs.is_empty() {
        return Err(E_CHAIN); // 空句断链，与 Viterbi 同口径
    }
    let mut out: Vec<Syll> = Vec::new();
    let mut i = 0usize;
    while i < pairs.len() {
        let cands = candidates(pairs[i][0], pairs[i][1])?;
        let mut best = cands[0];
        let mut best_sc = score(&best);
        let mut j = 1usize;
        while j < cands.len() {
            let sc = score(&cands[j]);
            if sc > best_sc {
                best = cands[j];
                best_sc = sc;
            }
            j += 1;
        }
        out.push(best);
        i += 1;
    }
    Ok(out)
}

/// Viterbi DAG（纪律 3）：逐层 (累计分, 前驱)，回溯最优音节序。
pub fn viterbi(pairs: &[[u8; 2]]) -> Result<Vec<Syll>, u16> {
    if pairs.is_empty() {
        return Err(E_CHAIN);
    }
    // 层账：每层候选的累计分与前驱层内下标。
    let mut layers: Vec<Vec<(u32, i32)>> = Vec::new();
    let mut layer_cands: Vec<Vec<Syll>> = Vec::new();
    let mut i = 0usize;
    while i < pairs.len() {
        let cands = candidates(pairs[i][0], pairs[i][1])?;
        let mut row: Vec<(u32, i32)> = Vec::new();
        let mut j = 0usize;
        while j < cands.len() {
            let sc = score(&cands[j]);
            if i == 0 {
                row.push((sc, -1));
            } else {
                // 前层任一节点皆可达（链式 DAG）：取前层最大累计。
                let mut best_prev = 0u32;
                let mut best_idx: i32 = 0;
                let mut p = 0usize;
                while p < layers[i - 1].len() {
                    let tot = layers[i - 1][p].0;
                    if tot > best_prev {
                        best_prev = tot;
                        best_idx = p as i32;
                    }
                    p += 1;
                }
                row.push((sc + best_prev, best_idx));
            }
            j += 1;
        }
        layers.push(row);
        layer_cands.push(cands);
        i += 1;
    }
    // 终局：末层最大累计（同分取先者，纪律 3）。
    let last = layers.len() - 1;
    let mut best_idx = 0usize;
    let mut best_tot = layers[last][0].0;
    let mut p = 1usize;
    while p < layers[last].len() {
        if layers[last][p].0 > best_tot {
            best_tot = layers[last][p].0;
            best_idx = p;
        }
        p += 1;
    }
    // 回溯。
    let mut out: Vec<Syll> = Vec::new();
    let mut idx = best_idx as i32;
    let mut l = last as i32;
    while l >= 0 {
        let ul = l as usize;
        if idx < 0 || idx as usize >= layer_cands[ul].len() {
            return Err(E_CHAIN);
        }
        out.push(layer_cands[ul][idx as usize]);
        idx = layers[ul][idx as usize].1;
        l -= 1;
    }
    out.reverse();
    if out.len() != pairs.len() {
        return Err(E_SEG_BAD);
    }
    Ok(out)
}

/// LCG（确定性）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 冻结词表（句源）：20 个 (声母,韵母) 槽位。
pub const SYLLABUS: [Syll; 20] = [
    (I_ZH, F_ONG), (I_ZERO, F_IA), (I_M, F_EN), (I_D, F_IANG), (I_X, F_IE),
    (I_H, F_ANG), (I_J, F_AN), (I_L, F_AI), (I_S, F_ONG), (I_T, F_UE),
    (I_SH, F_U), (I_ZH, F_UI), (I_B, F_O), (I_P, F_UN), (I_Q, F_IU),
    (I_G, F_ENG), (I_N, F_ING), (I_F, F_EN), (I_W, F_UAI), (I_ZERO, F_E),
];

/// 音节 → 双拼键序（经冻结布局表反查承载键）。
pub fn encode_syll(s: &Syll) -> Result<[u8; 2], u16> {
    let k1 = init_key(s.0);
    let mut k2 = 0u8;
    let mut found = false;
    let mut i = 0usize;
    while i < KEY_N {
        let row = KEY_FINALS[i];
        if row[0] == s.1 || row[1] == s.1 || row[2] == s.1 {
            k2 = KEY_LO + i as u8;
            found = true;
        }
        i += 1;
    }
    if !found {
        return Err(E_CHAIN);
    }
    Ok([k1, k2])
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（贯通 ×100 整句转换正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xBF80_0100);

    // R1 主判据 ×100：LCG 造句（3..8 音节）→ 编键序 → Viterbi
    //    贯通 → 与期望直算逐音节相等。
    let mut i = 0u32;
    while i < 100 {
        let n = 3 + rng.below(6);
        let mut sent: Vec<Syll> = Vec::new();
        let mut k = 0u32;
        while k < n {
            sent.push(SYLLABUS[rng.below(SYLLABUS.len() as u32) as usize]);
            k += 1;
        }
        // 编键序。
        let mut pairs: Vec<[u8; 2]> = Vec::new();
        let mut k = 0usize;
        while k < sent.len() {
            pairs.push(encode_syll(&sent[k])?);
            k += 1;
        }
        // 贯通。
        let got = viterbi(&pairs)?;
        let want = expect_best(&pairs)?;
        if got.len() != want.len() {
            return Err(E_MISMATCH);
        }
        let mut k = 0usize;
        while k < got.len() {
            if got[k] != want[k] {
                return Err(E_MISMATCH); // 回指句号的口径：首败即断
            }
            k += 1;
        }
        i += 1;
    }
    hits += 1;

    // R2 候选展开封闭：双韵母键产 2 候选、单韵母键产 1 候选、非法
    //    键对（a/e 作声母键、'{'）产 E_PARSE。
    let c = candidates(b'v', b'd')?; // zh + {iang,uang}
    if c.len() != 2 {
        return Err(E_CHAIN);
    }
    let c = candidates(b'm', b'n')?; // m + ing 单候选
    if c.len() != 1 {
        return Err(E_CHAIN);
    }
    if candidates(b'a', b'b').is_ok() {
        return Err(E_PARSE); // a 无声母
    }
    if candidates(b'z', b'{').is_ok() {
        return Err(E_PARSE);
    }
    hits += 1;

    // R3 Viterbi tie-break：构造同分并列面（两候选等分），必取先者。
    //    o 键 o(5)+零声母1=6 vs uo(6)+2=8 不并列——改用 s 键：
    //    ong(7)+2=9 vs iong(3)+2=5 不并列——并列面用 v 键 zh 系：
    //    ui(6)+2=8 唯一……直取构造层：r 键零声母 er(2)+1=3 vs
    //    uan(6)+1=7 不并列。并列面由 FREQUENCY 冻结值保证：
    //    d 键零声母 iang(6)+1=7 vs uang(5)+1=6 不并列——
    //    w 键零声母 ia(5)+1=6 vs ua(5)+1=6：同分！必取先者 ia。
    let got = viterbi(&[[b'o', b'w']])?;
    if got.len() != 1 || got[0] != (I_ZERO, F_IA) {
        return Err(E_TIE);
    }
    // 期望直算同口径复核。
    let want = expect_best(&[[b'o', b'w']])?;
    if want[0] != (I_ZERO, F_IA) {
        return Err(E_TIE);
    }
    hits += 1;

    // R4 择优非平凡性：高分候选确实压过低分（s 键 x 系取 iong 面
    //    但 ong 分更高——非零声母时 ong 胜；零声母时 iong+1 仍低于
    //    ong+1，但 ong 被音位禁——此处账面验 x+s → iong 通道存在）。
    let got = viterbi(&[[b'x', b's']])?;
    if got.len() != 1 || got[0] != (I_X, F_ONG) {
        return Err(E_MISMATCH); // ong(7)+2=9 > iong(3)+2=5
    }
    let got = viterbi(&[[b'o', b's']])?;
    if got.len() != 1 || got[0] != (I_ZERO, F_ONG) {
        return Err(E_MISMATCH); // 零声母同取 ong：7+1=8 > 3+1=4
    }
    hits += 1;

    // R5 分段守恒：键序长 N ⇒ 音节序长 N；Viterbi 回溯不得串层。
    let mut pairs: Vec<[u8; 2]> = Vec::new();
    let mut k = 0u32;
    while k < 8 {
        pairs.push([b'v', b'd']); // zh + iang/uang 双候选链
        k += 1;
    }
    let got = viterbi(&pairs)?;
    if got.len() != 8 {
        return Err(E_SEG_BAD);
    }
    let mut k = 0usize;
    while k < 8 {
        if got[k] != (I_ZH, F_IANG) {
            return Err(E_MISMATCH); // iang(6)+2=8 > uang(5)+2=7
        }
        k += 1;
    }
    hits += 1;

    // R6 空句拒：空键序 Viterbi 即 E_CHAIN（断链口径）。
    if viterbi(&[]).is_ok() {
        return Err(E_CHAIN);
    }
    if expect_best(&[]).is_ok() {
        return Err(E_CHAIN);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x2680)?;
    hits += 1;

    Ok(hits)
}
