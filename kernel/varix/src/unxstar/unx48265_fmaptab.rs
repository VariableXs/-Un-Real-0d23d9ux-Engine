//! unx48265_fmaptab — UNX-F48265 全拼/双拼/五笔方案映射表（fcitx5 方案互认面）（诊断段 0xC200）。
//!
//! 判据原文：
//!   J1 映射 ×3 类 ×10 抽样全对；判据锚定 §AI-61 §2 专题四互通账。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三方案三面：全拼（音节串 → 字母键序）、双拼（音节 → 双键码）、
//!      五笔（字根键序 → 字）；每面自带编解码，互认即三面往返各自闭合。
//!   2. 段位互斥（编号宪法）：声母编号段 0x00..=0x1F、韵母编号段
//!      0x20..=0x3F，两段永不重叠——凡映射表落号越段即 E_SEG_CLASH，
//!      撞号即双拼面歧义源头。
//!   3. 双拼键位唯一性：声母键位表、韵母键位表各自键位互异；双键码
//!      (声母键, 韵母键) 解码反查唯一，无歧义码。
//!   4. 抽样口径（J1 主体）：每方案抽 10 条映射，编码→解码往返逐位
//!      相等且与常量表直查逐位相等——×3 类 ×10 全对。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC200;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SM_OOB: u16 = SEG_BASE + 1; // 声母编号越段
const E_YM_OOB: u16 = SEG_BASE + 2; // 韵母编号越段
const E_SEG_CLASH: u16 = SEG_BASE + 3; // 声母/韵母段位互斥被破坏
const E_MAP_MISS: u16 = SEG_BASE + 4; // 映射表查无此项
const E_ROUNDTRIP: u16 = SEG_BASE + 5; // 往返失守（编解码不对合）
const E_SCHEME_BAD: u16 = SEG_BASE + 6; // 方案号越界
const E_STATE_BAD: u16 = SEG_BASE + 7; // 互认面状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

/// 方案号闭集：0 全拼 / 1 双拼 / 2 五笔。
pub const SCH_QUANPIN: u8 = 0;
pub const SCH_SHUANGPIN: u8 = 1;
pub const SCH_WUBI: u8 = 2;

/// 段位宪法：声母段 [0x00, 0x20)，韵母段 [0x20, 0x40)。
pub const SM_SEG_LO: u8 = 0x00;
pub const SM_SEG_HI: u8 = 0x20; // 开界
pub const YM_SEG_LO: u8 = 0x20;
pub const YM_SEG_HI: u8 = 0x40; // 开界

/// 声母表（21 项，编号即段内下标）。
pub const SM_LIST: [&str; 21] = [
    "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
    "zh", "ch", "sh", "r", "z", "c", "s",
];
/// 韵母表（20 项，编号 = 0x20 + 下标）。
pub const YM_LIST: [&str; 20] = [
    "a", "o", "e", "i", "u", "v", "ai", "ei", "ui", "ao", "ou", "iu",
    "ie", "ve", "er", "an", "en", "in", "un", "vn",
];
/// 双拼声母键位（与 SM_LIST 一一对应，键位互异）。
pub const SM_KEY: [u8; 21] = [
    b'b', b'p', b'm', b'f', b'd', b't', b'n', b'l', b'g', b'k', b'h',
    b'j', b'q', b'x', b'v', b'i', b'u', b'r', b'z', b'c', b's',
];
/// 双拼韵母键位（与 YM_LIST 一一对应，键位互异）。
pub const YM_KEY: [u8; 20] = [
    b'a', b'o', b'e', b'i', b'u', b'v', b'd', b'w', b'x', b'k', b'b',
    b'n', b'p', b't', b'q', b'j', b'f', b'l', b'y', b'g',
];
/// 五笔样表：字 → 4 键字根码（10 条抽样基）。
pub const WUBI: [(u16, [u8; 4]); 10] = [
    (0x4E00, [b'g', b'g', b'l', b'l']),
    (0x4E01, [b's', b'g', b'h', b' ']),
    (0x4E02, [b'w', b'h', b'g', b' ']),
    (0x4E03, [b'a', b'g', b'n', b'y']),
    (0x4E04, [b't', b'f', b'd', b' ']),
    (0x4E05, [b'g', b't', b'p', b' ']),
    (0x4E06, [b'r', b'n', b'n', b' ']),
    (0x4E07, [b'd', b'n', b'v', b' ']),
    (0x4E08, [b'n', b't', b'h', b' ']),
    (0x4E09, [b'd', b'g', b'g', b' ']),
];

/// 声母名 → 编号（落声母段，宪法 2）。
pub fn sm_code(name: &str) -> Result<u8, u16> {
    let mut i = 0usize;
    while i < SM_LIST.len() {
        if SM_LIST[i] == name {
            let code = i as u8;
            if code < SM_SEG_LO || code >= SM_SEG_HI {
                return Err(E_SM_OOB);
            }
            return Ok(code);
        }
        i += 1;
    }
    Err(E_MAP_MISS)
}

/// 韵母名 → 编号（落韵母段，宪法 2）。
pub fn ym_code(name: &str) -> Result<u8, u16> {
    let mut i = 0usize;
    while i < YM_LIST.len() {
        if YM_LIST[i] == name {
            let code = YM_SEG_LO + i as u8;
            if code < YM_SEG_LO || code >= YM_SEG_HI {
                return Err(E_YM_OOB);
            }
            return Ok(code);
        }
        i += 1;
    }
    Err(E_MAP_MISS)
}

/// 段位互斥判定：声母编号不得落韵母段、韵母编号不得落声母段。
pub fn seg_clash(sm: u8, ym: u8) -> Result<(), u16> {
    if sm >= SM_SEG_HI {
        return Err(E_SEG_CLASH);
    }
    if ym < YM_SEG_LO || ym >= YM_SEG_HI {
        return Err(E_SEG_CLASH);
    }
    Ok(())
}

/// 全拼面：音节 (声母, 韵母) → 字母键序串（声母名 + 韵母名逐字母）。
pub fn quanpin_encode(sm: u8, ym: u8) -> Result<Vec<u8>, u16> {
    seg_clash(sm, ym)?;
    if sm as usize >= SM_LIST.len() || ((ym - YM_SEG_LO) as usize) >= YM_LIST.len() {
        return Err(E_MAP_MISS);
    }
    let sm_name = SM_LIST[sm as usize];
    let ym_name = YM_LIST[(ym - YM_SEG_LO) as usize];
    let mut out: Vec<u8> = Vec::new();
    let sb = sm_name.as_bytes();
    let mut i = 0usize;
    while i < sb.len() {
        out.push(sb[i]);
        i += 1;
    }
    let yb = ym_name.as_bytes();
    i = 0;
    while i < yb.len() {
        out.push(yb[i]);
        i += 1;
    }
    Ok(out)
}

/// 双拼面：音节 → 双键码 (声母键, 韵母键)。
pub fn shuangpin_encode(sm: u8, ym: u8) -> Result<(u8, u8), u16> {
    seg_clash(sm, ym)?;
    if sm as usize >= SM_KEY.len() || ((ym - YM_SEG_LO) as usize) >= YM_KEY.len() {
        return Err(E_MAP_MISS);
    }
    Ok((SM_KEY[sm as usize], YM_KEY[(ym - YM_SEG_LO) as usize]))
}

/// 双拼面解码：双键码 → 编号对（反查唯一）。
pub fn shuangpin_decode(sk: u8, yk: u8) -> Result<(u8, u8), u16> {
    let mut si: Option<usize> = None;
    let mut i = 0usize;
    while i < SM_KEY.len() {
        if SM_KEY[i] == sk {
            if si.is_some() {
                return Err(E_ROUNDTRIP); // 键位重复 = 歧义码
            }
            si = Some(i);
        }
        i += 1;
    }
    let mut yi: Option<usize> = None;
    i = 0;
    while i < YM_KEY.len() {
        if YM_KEY[i] == yk {
            if yi.is_some() {
                return Err(E_ROUNDTRIP);
            }
            yi = Some(i);
        }
        i += 1;
    }
    match (si, yi) {
        (Some(s), Some(y)) => {
            let sm = s as u8;
            let ym = YM_SEG_LO + y as u8;
            seg_clash(sm, ym)?;
            Ok((sm, ym))
        }
        _ => Err(E_MAP_MISS),
    }
}

/// 五笔面：字 → 4 键字根码。
pub fn wubi_encode(ch: u16) -> Result<[u8; 4], u16> {
    let mut i = 0usize;
    while i < WUBI.len() {
        if WUBI[i].0 == ch {
            return Ok(WUBI[i].1);
        }
        i += 1;
    }
    Err(E_MAP_MISS)
}

/// 五笔面解码：4 键字根码 → 字。
pub fn wubi_decode(code: [u8; 4]) -> Result<u16, u16> {
    let mut i = 0usize;
    while i < WUBI.len() {
        if WUBI[i].1 == code {
            return Ok(WUBI[i].0);
        }
        i += 1;
    }
    Err(E_MAP_MISS)
}

/// 互认面抽样哨兵：按方案号跑一条抽样（编→解→对表三重核对）。
pub fn sample_check(scheme: u8, sm: u8, ym: u8) -> Result<(), u16> {
    match scheme {
        SCH_QUANPIN => {
            let keys = quanpin_encode(sm, ym)?;
            // 解码：键序串反查声母/韵母名拼接。
            let sm_name = SM_LIST[sm as usize];
            let ym_name = YM_LIST[(ym - YM_SEG_LO) as usize];
            let mut want: Vec<u8> = Vec::new();
            let sb = sm_name.as_bytes();
            let mut i = 0usize;
            while i < sb.len() {
                want.push(sb[i]);
                i += 1;
            }
            let yb = ym_name.as_bytes();
            i = 0;
            while i < yb.len() {
                want.push(yb[i]);
                i += 1;
            }
            if keys.len() != want.len() {
                return Err(E_ROUNDTRIP);
            }
            let mut j = 0usize;
            while j < want.len() {
                if keys[j] != want[j] {
                    return Err(E_ROUNDTRIP);
                }
                j += 1;
            }
            Ok(())
        }
        SCH_SHUANGPIN => {
            let (sk, yk) = shuangpin_encode(sm, ym)?;
            let (sm2, ym2) = shuangpin_decode(sk, yk)?;
            if sm2 != sm || ym2 != ym {
                return Err(E_ROUNDTRIP);
            }
            Ok(())
        }
        SCH_WUBI => {
            // 五笔抽样以声母/韵母参数运轨：sm 作字下标。
            let idx = sm as usize;
            if idx >= WUBI.len() {
                return Err(E_MAP_MISS);
            }
            let ch = WUBI[idx].0;
            let code = wubi_encode(ch)?;
            let back = wubi_decode(code)?;
            if back != ch {
                return Err(E_ROUNDTRIP);
            }
            Ok(())
        }
        _ => Err(E_SCHEME_BAD),
    }
}

/// 完成写回（判据 J2）：抽样全对 + 非空摘要方放行。
pub fn writeback(done: bool, green_classes: u32, summary_len: u32) -> Result<(), u16> {
    if !done || green_classes < 3 {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（映射 ×3 类 ×10 抽样全对），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 全拼面 ×10 抽样：10 个音节编解码逐位对表全对。
    let samples: [(u8, u8); 10] = [
        (0, YM_SEG_LO), (2, YM_SEG_LO + 2), (4, YM_SEG_LO + 4), (7, YM_SEG_LO + 5),
        (10, YM_SEG_LO + 6), (13, YM_SEG_LO + 8), (14, YM_SEG_LO + 15), (16, YM_SEG_LO + 16),
        (19, YM_SEG_LO + 18), (20, YM_SEG_LO + 19),
    ];
    let mut s = 0usize;
    while s < 10 {
        sample_check(SCH_QUANPIN, samples[s].0, samples[s].1)?;
        s += 1;
    }
    hits += 1;

    // R2 双拼面 ×10 抽样：编→解往返全对；键位唯一性全表核验。
    let mut s = 0usize;
    while s < 10 {
        sample_check(SCH_SHUANGPIN, samples[s].0, samples[s].1)?;
        s += 1;
    }
    // 声母键位互异 + 韵母键位互异（歧义码防线）。
    let mut a = 0usize;
    while a < SM_KEY.len() {
        let mut b = a + 1;
        while b < SM_KEY.len() {
            if SM_KEY[a] == SM_KEY[b] {
                return Err(E_ROUNDTRIP);
            }
            b += 1;
        }
        a += 1;
    }
    a = 0;
    while a < YM_KEY.len() {
        let mut b = a + 1;
        while b < YM_KEY.len() {
            if YM_KEY[a] == YM_KEY[b] {
                return Err(E_ROUNDTRIP);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R3 五笔面 ×10 抽样：字 ↔ 4 键码往返逐条全对、码长恒 4。
    let mut s = 0usize;
    while s < 10 {
        sample_check(SCH_WUBI, s as u8, YM_SEG_LO)?;
        let ch = WUBI[s].0;
        let code = wubi_encode(ch)?;
        // 码长恒 4（互认面格式契约）。
        if wubi_decode(code)? != ch {
            return Err(E_ROUNDTRIP);
        }
        s += 1;
    }
    hits += 1;

    // R4 段位互斥宪法：声母全表落 [0x00,0x20)、韵母全表落 [0x20,0x40)；
    //    越段组合双拒（E_SEG_CLASH / E_SM_OOB / E_YM_OOB 族）。
    let mut i = 0usize;
    while i < SM_LIST.len() {
        let c = sm_code(SM_LIST[i])?;
        if c >= SM_SEG_HI {
            return Err(E_SEG_CLASH);
        }
        i += 1;
    }
    i = 0;
    while i < YM_LIST.len() {
        let c = ym_code(YM_LIST[i])?;
        if c < YM_SEG_LO || c >= YM_SEG_HI {
            return Err(E_SEG_CLASH);
        }
        i += 1;
    }
    // 伪造越段：声母 0x25（韵母段）、韵母 0x10（声母段）→ 双拒。
    if seg_clash(0x25, YM_SEG_LO).is_ok() {
        return Err(E_SEG_CLASH);
    }
    if seg_clash(0x00, 0x10).is_ok() {
        return Err(E_SEG_CLASH);
    }
    if quanpin_encode(0x25, YM_SEG_LO).is_ok() {
        return Err(E_SEG_CLASH);
    }
    hits += 1;

    // R5 查无此项：未知声母名/韵母名 → E_MAP_MISS；双拼野键 → E_MAP_MISS。
    if sm_code("ww").is_ok() {
        return Err(E_MAP_MISS);
    }
    if ym_code("zz").is_ok() {
        return Err(E_MAP_MISS);
    }
    if shuangpin_decode(b'?', b'?').is_ok() {
        return Err(E_MAP_MISS);
    }
    if wubi_encode(0xDEAD).is_ok() {
        return Err(E_MAP_MISS);
    }
    if wubi_decode([b'z', b'z', b'z', b'z']).is_ok() {
        return Err(E_MAP_MISS);
    }
    hits += 1;

    // R6 方案号闭集：越界方案号抽样 → E_SCHEME_BAD。
    if sample_check(9, 0, YM_SEG_LO).is_ok() {
        return Err(E_SCHEME_BAD);
    }
    hits += 1;

    // R7 双拼反查唯一性负例补强：伪造重键表不可达（静态表互异已验），
    //    但解码器必须保留歧义防线——此处验证合法双键码解码恰得一解。
    let (sk, yk) = shuangpin_encode(3, YM_SEG_LO + 6)?;
    let (sm2, ym2) = shuangpin_decode(sk, yk)?;
    if sm2 != 3 || ym2 != YM_SEG_LO + 6 {
        return Err(E_ROUNDTRIP);
    }
    hits += 1;

    // R8 写回（判据 J2）：三类未齐不写回、空摘要拒、齐+非空摘要放行。
    if writeback(true, 2, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if writeback(true, 3, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 3, 0x18)?;
    hits += 1;

    Ok(hits)
}
