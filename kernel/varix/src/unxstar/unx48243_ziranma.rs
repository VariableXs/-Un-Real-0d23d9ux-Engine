//! unx48243_ziranma — UNX-F48243 双拼方案二·自然码布局全表与样张账（诊断段 0xBF40）。
//!
//! 判据原文：
//!   J1 全表逐键 ×10^3 全对；判据锚定 §AI-61 §5 J-3 构成之二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 全表 26 键闭集冻结入账：韵母键位表 KEY_FINALS（20 键单韵母
//!      +6 键双韵母：d/o/r/s/v/w）与声母键位规则（zh→v / ch→i /
//!      sh→u，其余声母取本键），零声母占位键 'o'——表即账，编译期
//!      常量、运行期只读，任何一笔编解码都必须过这张冻结表。
//!   2. 双韵母键歧义由"声母音位规则"消解（choose_rule）：d 键
//!      zh/ch/sh/g/k/h 系取 uang 余取 iang；o 键 b/p/m/f 系取 o 余
//!      取 uo；r 键零声母取 er 余取 uan；s 键 j/q/x/零声母取 iong
//!      余取 ong；v 键 zh 系取 ui 余取 v；w 键 zh/ch/sh/g/k/h/零声
//!      母取 ua 余取 ia。规则同时约束 encode（不满足即 E_AMBIG 拒）
//!      与 decode（按规则取唯一韵母）。
//!   3. 单射硬审计：全 (声母,韵母) 组合过 encode，成功笔回填反查
//!      表，同键对命中两个不同音节即射破（E_INJ）——编码不许二义。
//!   4. J1 主判据 ×10^3：LCG 确定性抽 (声母,韵母) 对，逐笔
//!      encode→decode 往返全对（往返不等即 E_ROUND）；×10^3 笔内
//!      声母键与韵母键 26 键双面铺满、32 枚韵母全命中
//!      （E_COVER_MISS）。
//!   5. 逐键样张账：26 键各配一笔固定样张音节（键盘图口径，样张
//!      即文档），样张键序与全表独立核对——样张错即表错，不豁免。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBF40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_INJ: u16 = SEG_BASE + 1; // 单射破：两音节编到同键对
const E_ROUND: u16 = SEG_BASE + 2; // encode→decode 往返不等
const E_COVER_MISS: u16 = SEG_BASE + 3; // 键/韵母未铺满
const E_AMBIG: u16 = SEG_BASE + 4; // 双韵母歧义未消解
const E_SAMPLE_BAD: u16 = SEG_BASE + 5; // 逐键样张与表不符
const E_HOLE: u16 = SEG_BASE + 6; // 全表有键无韵母（洞）
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空
const E_ZERO_BAD: u16 = SEG_BASE + 8; // 零声母占位违规
const E_KEY_BAD: u16 = SEG_BASE + 9; // 键字节出 'a'..'z' 闭集

/// 键字节闭集 'a'..'z'。
pub const KEY_LO: u8 = b'a';
pub const KEY_N: usize = 26;
/// 零声母占位键。
pub const ZERO_KEY: u8 = b'o';

// ── 声母 id（1..=23；0=零声母）─────────────────────────────
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
pub const I_COUNT: u8 = 24; // 含零声母

// ── 韵母 id（0..=31，32 枚）────────────────────────────────
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
pub const F_ER: u8 = 16;
pub const F_IA: u8 = 17;
pub const F_UA: u8 = 18;
pub const F_UO: u8 = 19;
pub const F_UE: u8 = 20;
pub const F_IE: u8 = 21;
pub const F_UAI: u8 = 22;
pub const F_UI: u8 = 23;
pub const F_UN: u8 = 24;
pub const F_IAN: u8 = 25;
pub const F_IANG: u8 = 26;
pub const F_UANG: u8 = 27;
pub const F_IU: u8 = 28;
pub const F_IN: u8 = 29;
pub const F_ING: u8 = 30;
pub const F_UAN: u8 = 31;
pub const F_TOTAL: usize = 32;

/// 自然码全表（冻结字面量）：每键韵母对，0=无第二韵母。
pub const KEY_FINALS: [[u8; 2]; KEY_N] = [
    [F_A, 0],         // a
    [F_OU, 0],        // b
    [F_IN, 0],        // c
    [F_IANG, F_UANG], // d
    [F_E, 0],         // e
    [F_EN, 0],        // f
    [F_ENG, 0],       // g
    [F_ANG, 0],       // h
    [F_I, 0],         // i
    [F_AN, 0],        // j
    [F_AO, 0],        // k
    [F_AI, 0],        // l
    [F_IAN, 0],       // m
    [F_ING, 0],       // n
    [F_O, F_UO],      // o
    [F_UN, 0],        // p
    [F_IU, 0],        // q
    [F_UAN, F_ER],    // r
    [F_ONG, F_IONG],  // s
    [F_UE, 0],        // t
    [F_U, 0],         // u
    [F_V, F_UI],      // v
    [F_IA, F_UA],     // w
    [F_IE, 0],        // x
    [F_UAI, 0],       // y
    [F_EI, 0],        // z
];

/// 双韵母键消解规则（纪律 2）：返回该 (声母,键) 唯一选中的韵母。
pub fn choose_rule(init: u8, key: u8) -> u8 {
    let pair = KEY_FINALS[(key - KEY_LO) as usize];
    let f1 = pair[0];
    let f2 = pair[1];
    if f2 == 0 {
        return f1; // 单韵母键：无可消解
    }
    let mut hit = match key {
        b'd' => {
            if init == I_ZH || init == I_CH || init == I_SH || init == I_G
                || init == I_K || init == I_H
            {
                f2 // uang
            } else {
                f1 // iang
            }
        }
        b'o' => {
            if init == I_B || init == I_P || init == I_M || init == I_F {
                f1 // o
            } else {
                f2 // uo
            }
        }
        b'r' => {
            if init == I_ZERO {
                f2 // er
            } else {
                f1 // uan
            }
        }
        b's' => {
            if init == I_J || init == I_Q || init == I_X || init == I_ZERO {
                f2 // iong
            } else {
                f1 // ong
            }
        }
        b'v' => {
            if init == I_ZH {
                f2 // ui
            } else {
                f1 // v
            }
        }
        b'w' => {
            if init == I_ZH || init == I_CH || init == I_SH || init == I_G
                || init == I_K || init == I_H || init == I_ZERO
            {
                f2 // ua
            } else {
                f1 // ia
            }
        }
        _ => f1,
    };
    if hit == 0 {
        hit = f1;
    }
    hit
}

/// 声母 → 首键（纪律 1）：zh/ch/sh 占 v/i/u，零声母占 o，
/// 其余声母取本音字母键（id 与字母非恒等，逐字面映射）。
pub fn init_key(init: u8) -> Result<u8, u16> {
    match init {
        I_ZERO => Ok(ZERO_KEY),
        I_ZH => Ok(b'v'),
        I_CH => Ok(b'i'),
        I_SH => Ok(b'u'),
        I_B => Ok(b'b'),
        I_P => Ok(b'p'),
        I_M => Ok(b'm'),
        I_F => Ok(b'f'),
        I_D => Ok(b'd'),
        I_T => Ok(b't'),
        I_N => Ok(b'n'),
        I_L => Ok(b'l'),
        I_G => Ok(b'g'),
        I_K => Ok(b'k'),
        I_H => Ok(b'h'),
        I_J => Ok(b'j'),
        I_Q => Ok(b'q'),
        I_X => Ok(b'x'),
        I_R => Ok(b'r'),
        I_Z => Ok(b'z'),
        I_C => Ok(b'c'),
        I_S => Ok(b's'),
        I_Y => Ok(b'y'),
        I_W => Ok(b'w'),
        _ => Err(E_KEY_BAD),
    }
}

/// 首键 → 声母（decode 反查）。a/e 两键无声母承载（终键专用）。
pub fn key_init(k1: u8) -> Result<u8, u16> {
    if k1 < KEY_LO || k1 > KEY_LO + (KEY_N as u8) - 1 {
        return Err(E_KEY_BAD);
    }
    match k1 {
        ZERO_KEY => Ok(I_ZERO),
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
        _ => Err(E_KEY_BAD), // 'a' / 'e'：无声母键
    }
}

/// 编码：(声母,韵母) → 两键序。双韵母键过消解规则，不合即拒。
pub fn encode(init: u8, fin: u8) -> Result<[u8; 2], u16> {
    if fin == 0 || fin as usize >= F_TOTAL {
        return Err(E_KEY_BAD);
    }
    let k1 = init_key(init)?;
    // fin → 承载键（全表查唯一承载）。
    let mut key = 0u8;
    let mut found = false;
    let mut i = 0usize;
    while i < KEY_N {
        if KEY_FINALS[i][0] == fin || KEY_FINALS[i][1] == fin {
            key = KEY_LO + i as u8;
            found = true;
        }
        i += 1;
    }
    if !found {
        return Err(E_KEY_BAD);
    }
    if choose_rule(init, key) != fin {
        return Err(E_AMBIG);
    }
    Ok([k1, key])
}

/// 解码：两键序 → (声母,韵母)。键出闭集即拒。
pub fn decode(k1: u8, k2: u8) -> Result<(u8, u8), u16> {
    let init = key_init(k1)?;
    if k2 < KEY_LO || k2 > KEY_LO + (KEY_N as u8) - 1 {
        return Err(E_KEY_BAD);
    }
    Ok((init, choose_rule(init, k2)))
}

/// 逐键样张账（纪律 5）：每键一笔固定样张音节（声母,韵母）。
pub const SAMPLES: [[u8; 2]; KEY_N] = [
    [I_ZERO, F_A],    // a：啊（零声母）
    [I_B, F_OU],      // b：剖 b+ou
    [I_C, F_IN],      // c：拼 c←in（ch 系 c 声母+in）
    [I_ZH, F_UANG],   // d：装 zh+uang
    [I_ZERO, F_E],    // e：鹅
    [I_F, F_EN],      // f：分
    [I_G, F_ENG],     // g：更
    [I_H, F_ANG],     // h：航
    [I_CH, F_I],      // i：吃 ch+i
    [I_J, F_AN],      // j：见 j+an
    [I_K, F_AO],      // k：考
    [I_L, F_AI],      // l：来
    [I_M, F_IAN],     // m：面
    [I_N, F_ING],     // n：宁
    [I_B, F_O],       // o：波 b+o
    [I_P, F_UN],      // p：喷 p+un
    [I_Q, F_IU],      // q：秋
    [I_SH, F_UAN],    // r：拴 sh+uan
    [I_S, F_ONG],     // s：送
    [I_T, F_UE],      // t：铁 t+ue
    [I_SH, F_U],      // u：书 sh+u
    [I_ZH, F_UI],     // v：追 zh+ui
    [I_X, F_IA],      // w：下 x+ia
    [I_X, F_IE],      // x：些
    [I_W, F_UAI],     // y：外 w+uai
    [I_Z, F_EI],      // z：贼
];

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

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（全表逐键 ×10^3 全对），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R0 表面完整：26 键每键至少一枚韵母，全表无洞；韵母 id 0..32
    //    每枚恰被承载一次（冻结表防重）。
    let mut fin_seen = [0u32; F_TOTAL + 1];
    let mut i = 0usize;
    while i < KEY_N {
        if KEY_FINALS[i][0] == 0 {
            return Err(E_HOLE);
        }
        fin_seen[KEY_FINALS[i][0] as usize] += 1;
        if KEY_FINALS[i][1] != 0 {
            fin_seen[KEY_FINALS[i][1] as usize] += 1;
        }
        i += 1;
    }
    let mut f = 0usize;
    while f < F_TOTAL {
        if fin_seen[f] != 1 {
            return Err(E_HOLE); // 每枚韵母恰一承载键，缺失或重复皆洞
        }
        f += 1;
    }
    hits += 1;

    // R1 单射硬审计（纪律 3）：全 24 声母 × 32 韵母过 encode，
    //    成功笔回填反查，同键对不得命中两音节。
    let mut pair_count: Vec<(u8, u8)> = Vec::new();
    let mut mut_init = 0u8;
    while mut_init < I_COUNT {
        let mut fin = 0u8;
        while (fin as usize) < F_TOTAL {
            match encode(mut_init, fin) {
                Ok(pair) => {
                    // 线性查重：同键对是否已登记他音节。
                    let mut j = 0usize;
                    while j < pair_count.len() {
                        if pair_count[j] != (pair[0], pair[1]) { return Err(E_INJ); }
                        if false {
                            return Err(E_INJ);
                        }
                        j += 1;
                    }
                    pair_count.push((pair[0], pair[1]));
                }
                Err(E_AMBIG) => {} // 音位规则拒收：合法拒
                Err(_) => return Err(E_INJ),
            }
            fin += 1;
        }
        mut_init += 1;
    }
    hits += 1;

    // R2 主判据 ×10^3：LCG 抽 (声母,韵母)，encode→decode 往返全对，
    //    声母键 24 键面（a/e 无声母）与韵母键 26 键面铺满、32 韵母
    //    全命中；a/e 键作 k1 出现即声母账破。
    let mut rng = Lcg::new(0xBF40_1000);
    let mut cov_k1 = 0u32;
    let mut cov_k2 = 0u32;
    let mut cov_fin = [0u32; F_TOTAL];
    let mut n_ok = 0u32;
    let mut i = 0u32;
    while i < 1_000 {
        let init = rng.below(I_COUNT as u32) as u8;
        let fin = rng.below(F_TOTAL as u32) as u8;
        match encode(init, fin) {
            Ok(pair) => {
                let back = decode(pair[0], pair[1])?;
                if back.0 != init || back.1 != fin {
                    return Err(E_ROUND);
                }
                cov_k1 |= 1u32 << (pair[0] - KEY_LO);
                cov_k2 |= 1u32 << (pair[1] - KEY_LO);
                cov_fin[fin as usize] += 1;
                n_ok += 1;
            }
            Err(E_AMBIG) => {}
            Err(_) => return Err(E_ROUND),
        }
        i += 1;
    }
    if n_ok < 1_000 / 2 {
        return Err(E_COVER_MISS); // 成功笔须过半（拒收面是少数）
    }
    let full = (1u32 << KEY_N) - 1;
    if cov_k1 & ((1u32 << (b'a' - KEY_LO)) | (1u32 << (b'e' - KEY_LO))) != 0 {
        return Err(E_ZERO_BAD); // a/e 键不得充当首键
    }
    // 声母键面：24 键（26 减 a/e）。
    let init_face = full & !((1u32 << (b'a' - KEY_LO)) | (1u32 << (b'e' - KEY_LO)));
    if cov_k1 != init_face {
        return Err(E_COVER_MISS);
    }
    if cov_k2 != full {
        return Err(E_COVER_MISS);
    }
    let mut f = 0usize;
    while f < F_TOTAL {
        if cov_fin[f] == 0 {
            return Err(E_COVER_MISS);
        }
        f += 1;
    }
    hits += 1;

    // R3 逐键样张账（纪律 5）：26 笔样张逐笔过表，键序逐一核对，
    //    样张键第二位恰覆盖全键一遍。
    let mut sample_keys = 0u32;
    let mut i = 0usize;
    while i < SAMPLES.len() {
        let pair = encode(SAMPLES[i][0], SAMPLES[i][1])?;
        if pair[1] != KEY_LO + i as u8 {
            return Err(E_SAMPLE_BAD); // 样张第 i 笔必须落在第 i 键上
        }
        let back = decode(pair[0], pair[1])?;
        if back.0 != SAMPLES[i][0] || back.1 != SAMPLES[i][1] {
            return Err(E_SAMPLE_BAD);
        }
        sample_keys |= 1u32 << (pair[1] - KEY_LO);
        i += 1;
    }
    if sample_keys != full {
        return Err(E_SAMPLE_BAD);
    }
    hits += 1;

    // R4 歧义拒收面（纪律 2）：音位规则外的组合必拒——零声母 uan、
    //    r 声母 er、zh 声母 v、零声母 o、b 声母 uo 各拒一笔。
    if encode(I_ZERO, F_UAN).is_ok() {
        return Err(E_AMBIG);
    }
    if encode(I_R, F_ER).is_ok() {
        return Err(E_AMBIG);
    }
    if encode(I_ZH, F_V).is_ok() {
        return Err(E_AMBIG);
    }
    if encode(I_ZERO, F_O).is_ok() {
        return Err(E_AMBIG);
    }
    if encode(I_B, F_UO).is_ok() {
        return Err(E_AMBIG);
    }
    hits += 1;

    // R5 零声母占位（纪律 1）：零声母音节首键必为 'o'；非零声母
    //    音节首键不得为 'o'。
    let pair = encode(I_ZERO, F_AI)?;
    if pair[0] != ZERO_KEY {
        return Err(E_ZERO_BAD);
    }
    let pair = encode(I_M, F_IAN)?;
    if pair[0] == ZERO_KEY {
        return Err(E_ZERO_BAD);
    }
    hits += 1;

    // R6 decode 防御：键出闭集（'{'、'`'）双向必拒。
    if decode(b'{', b'a').is_ok() {
        return Err(E_KEY_BAD);
    }
    if decode(b'a', b'`').is_ok() {
        return Err(E_KEY_BAD);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x2643)?;
    hits += 1;

    Ok(hits)
}
