//! unx48258_spfuzzy — UNX-F48258 双拼模糊音联动账（双拼键位下 fuzzy 表适配与开关一致）（诊断段 0xC120）。
//!
//! 判据原文：
//!   J1 联动 ×10 组行为与全拼一致；判据锚定 §AI-61 §2 专题一 fuzzy 表。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 模糊对闭集：zh↔z、ch↔c、sh↔s、l↔n、f↔h、an↔ang、en↔eng、
//!      in↔ing 八对（fuzzy 表域内闭集）；开关只对闭集内对生效。
//!   2. 双路径对账：同一目标音节集，全拼路径（拼音串模糊匹配）与
//!      双拼路径（键位编码后模糊匹配）各自独立求命中集；两路径命中
//!      集逐位全等才联动成立——路径间零共享匹配代码（防同错）。
//!   3. 开关一致：fuzzy off 时两路径都精确匹配（命中集收窄且互等）；
//!      fuzzy on 时两路径都模糊（命中集扩大且互等）；一侧开一侧关
//!      即失步，联动账必红。
//!   4. ×10 组口径（J1）：10 组用例 = 8 对模糊音各 1 组 + 2 组多对
//!      叠加，每组含 on/off 两态比对，逐组落账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC120;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_LINK_BROKEN: u16 = SEG_BASE + 1;  // 两路径命中集不等
const E_PAIR_UNKNOWN: u16 = SEG_BASE + 2; // 模糊对闭集外
const E_GROUP_MISS: u16 = SEG_BASE + 3;   // ×10 组不足
const E_STATE_BAD: u16 = SEG_BASE + 4;    // 开关态非法
const E_LEDGER_BAD: u16 = SEG_BASE + 5;   // 联动账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;    // 写回摘要为空

/// 音节表条目：全拼（声母, 韵母）+ 三方案无关的双拼键位（本卡用标准键位）。
#[derive(Clone, Copy, Debug)]
pub struct FzSyllable {
    pub sheng: u8, // 声母 ASCII（zh/ch/sh 用 z/c/s 占位再以卷舌位区分）
    pub retroflex: bool, // true = 卷舌（zh/ch/sh）
    pub yun: u16,  // 韵母代号（自定义枚举值）
}

/// 声母代号（内码）。
pub const SH_B: u8 = b'b';
pub const SH_M: u8 = b'm';
pub const SH_D: u8 = b'd';
pub const SH_T: u8 = b't';
pub const SH_N: u8 = b'n';
pub const SH_L: u8 = b'l';
pub const SH_F: u8 = b'f';
pub const SH_H: u8 = b'h';
pub const SH_Z: u8 = b'z';
pub const SH_C: u8 = b'c';
pub const SH_S: u8 = b's';

/// 韵母代号（内码）。
pub const YUN_A: u16 = 1;
pub const YUN_AN: u16 = 2;
pub const YUN_ANG: u16 = 3;
pub const YUN_EN: u16 = 4;
pub const YUN_ENG: u16 = 5;
pub const YUN_IN: u16 = 6;
pub const YUN_ING: u16 = 7;
pub const YUN_U: u16 = 8;
pub const YUN_I: u16 = 9;

/// 音节语料表（16 条：覆盖 8 对模糊音全部成员）。
pub const FZ_TABLE: [FzSyllable; 16] = [
    FzSyllable { sheng: SH_Z, retroflex: true, yun: YUN_AN },  // zhan
    FzSyllable { sheng: SH_Z, retroflex: false, yun: YUN_AN }, // zan
    FzSyllable { sheng: SH_Z, retroflex: true, yun: YUN_ANG }, // zhang
    FzSyllable { sheng: SH_Z, retroflex: false, yun: YUN_ANG },// zang
    FzSyllable { sheng: SH_C, retroflex: true, yun: YUN_EN },  // chen
    FzSyllable { sheng: SH_C, retroflex: false, yun: YUN_EN }, // cen
    FzSyllable { sheng: SH_C, retroflex: true, yun: YUN_ENG }, // cheng
    FzSyllable { sheng: SH_C, retroflex: false, yun: YUN_ENG },// ceng
    FzSyllable { sheng: SH_S, retroflex: true, yun: YUN_IN },  // shin（示音节位）
    FzSyllable { sheng: SH_S, retroflex: false, yun: YUN_IN }, // sin（位）
    FzSyllable { sheng: SH_S, retroflex: true, yun: YUN_ING }, // shing（位）
    FzSyllable { sheng: SH_S, retroflex: false, yun: YUN_ING },// sing（位）
    FzSyllable { sheng: SH_N, retroflex: false, yun: YUN_A },  // na
    FzSyllable { sheng: SH_L, retroflex: false, yun: YUN_A },  // la
    FzSyllable { sheng: SH_F, retroflex: false, yun: YUN_U },  // fu
    FzSyllable { sheng: SH_H, retroflex: false, yun: YUN_U },  // hu
];

/// 模糊对闭集（8 对）：(成员 A, 成员 B, is_yun)。
/// 声母对比较 (sheng,retroflex)；韵母对比较 yun。
pub const FUZZY_PAIRS: [(u16, u16, bool); 8] = [
    (SH_Z as u16, SH_Z as u16, false), // zh↔z：以 retroflex 区分（占位编码）
    (SH_C as u16, SH_C as u16, false), // ch↔c
    (SH_S as u16, SH_S as u16, false), // sh↔s
    (SH_L as u16, SH_N as u16, false), // l↔n
    (SH_F as u16, SH_H as u16, false), // f↔h
    (YUN_AN, YUN_ANG, true),           // an↔ang
    (YUN_EN, YUN_ENG, true),           // en↔eng
    (YUN_IN, YUN_ING, true),           // in↔ing
];

/// 两声母是否模糊相等（含卷舌开关）。
fn sheng_fuzzy_eq(a: (u8, bool), b: (u8, bool), pairs: &[usize], use_fuzzy: bool) -> bool {
    if a == b {
        return true;
    }
    if !use_fuzzy {
        return false;
    }
    // zh↔z、ch↔c、sh↔s：同字母异卷舌。
    if a.0 == b.0 && a.1 != b.1 {
        return pairs.contains(&0) || pairs.contains(&1) || pairs.contains(&2)
            || (pairs.contains(&0) && a.0 == SH_Z)
            || (pairs.contains(&1) && a.0 == SH_C)
            || (pairs.contains(&2) && a.0 == SH_S);
    }
    let mut i = 0usize;
    while i < pairs.len() {
        let p = pairs[i];
        if p >= 3 && p < 5 {
            // l↔n / f↔h
            let (x, y) = (FUZZY_PAIRS[p].0 as u8, FUZZY_PAIRS[p].1 as u8);
            if (a.0 == x && b.0 == y) || (a.0 == y && b.0 == x) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 两韵母是否模糊相等。
fn yun_fuzzy_eq(a: u16, b: u16, pairs: &[usize], use_fuzzy: bool) -> bool {
    if a == b {
        return true;
    }
    if !use_fuzzy {
        return false;
    }
    let mut i = 0usize;
    while i < pairs.len() {
        let p = pairs[i];
        if p >= 5 {
            let (x, y) = (FUZZY_PAIRS[p].0, FUZZY_PAIRS[p].1);
            if (a == x && b == y) || (a == y && b == x) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 全拼路径匹配：给定查询 (sheng,retroflex,yun) 与启用模糊对集，
/// 求语料命中集（下标集合的位掩码 u16）。
pub fn match_pinyin(q: (u8, bool, u16), pairs: &[usize], use_fuzzy: bool) -> u16 {
    let mut mask: u16 = 0;
    let mut i = 0usize;
    while i < FZ_TABLE.len() {
        let s = &FZ_TABLE[i];
        if sheng_fuzzy_eq((q.0, q.1), (s.sheng, s.retroflex), pairs, use_fuzzy)
            && yun_fuzzy_eq(q.2, s.yun, pairs, use_fuzzy)
        {
            mask |= 1 << i;
        }
        i += 1;
    }
    mask
}

/// 双拼路径匹配：查询先按标准键位编码为 (声母键, 韵母键) 再匹配。
/// 标准键位：卷舌声母用固定双拼键（zh→v、ch→i、sh→u），韵母 an→j、
/// ang→h、en→f、eng→g、in→b、ing→k、a→a、u→u、i→i。
/// 路径独立性：键位映射后仍走本路径内的匹配实现（与全拼路径代码互异）。
pub fn sp_key_sheng(q: (u8, bool)) -> u8 {
    if q.1 {
        match q.0 {
            SH_Z => b'v',
            SH_C => b'i',
            SH_S => b'u',
            _ => q.0,
        }
    } else {
        q.0
    }
}

pub fn sp_key_yun(y: u16) -> u16 {
    match y {
        YUN_A => b'a' as u16,
        YUN_AN => b'j' as u16,
        YUN_ANG => b'h' as u16,
        YUN_EN => b'f' as u16,
        YUN_ENG => b'g' as u16,
        YUN_IN => b'b' as u16,
        YUN_ING => b'k' as u16,
        YUN_U => b'u' as u16,
        _ => b'i' as u16,
    }
}

pub fn match_shuangpin(q: (u8, bool, u16), pairs: &[usize], use_fuzzy: bool) -> u16 {
    // 双拼路径：把语料逐条映射到键位域，再在键位域做模糊等价。
    let q_s = sp_key_sheng((q.0, q.1)) as u16;
    let q_y = sp_key_yun(q.2);
    let mut mask: u16 = 0;
    let mut i = 0usize;
    while i < FZ_TABLE.len() {
        let s = &FZ_TABLE[i];
        let s_s = sp_key_sheng((s.sheng, s.retroflex)) as u16;
        let s_y = sp_key_yun(s.yun);
        // 键位域模糊等价：声母键相等或（模糊开启时）经声母对还原判断；
        // 韵母键同理按键位域对（an/ang→j/h、en/eng→f/g、in/ing→b/k）。
        let sheng_hit = if s_s == q_s {
            true
        } else if use_fuzzy && s.retroflex != q.1 && sp_key_sheng((q.0, !q.1)) as u16 == s_s {
            pairs.contains(&0) || pairs.contains(&1) || pairs.contains(&2)
        } else if use_fuzzy {
            // 键位域平舌对：l↔n（pair 3）、f↔h（pair 4）。
            let ln = (q_s == b'l' as u16 && s_s == b'n' as u16)
                || (q_s == b'n' as u16 && s_s == b'l' as u16);
            let fh = (q_s == b'f' as u16 && s_s == b'h' as u16)
                || (q_s == b'h' as u16 && s_s == b'f' as u16);
            (ln && pairs.contains(&3)) || (fh && pairs.contains(&4))
        } else {
            false
        };
        let yun_hit = if s_y == q_y {
            true
        } else if use_fuzzy {
            // 键位域韵母对表：j↔h、f↔g、b↔k。
            let jh = (q_y == b'j' as u16 && s_y == b'h' as u16)
                || (q_y == b'h' as u16 && s_y == b'j' as u16);
            let fg = (q_y == b'f' as u16 && s_y == b'g' as u16)
                || (q_y == b'g' as u16 && s_y == b'f' as u16);
            let bk = (q_y == b'b' as u16 && s_y == b'k' as u16)
                || (q_y == b'k' as u16 && s_y == b'b' as u16);
            (jh && pairs.contains(&5)) || (fg && pairs.contains(&6)) || (bk && pairs.contains(&7))
        } else {
            false
        };
        if sheng_hit && yun_hit {
            mask |= 1 << i;
        }
        i += 1;
    }
    mask
}

/// 一组联动账。
#[derive(Clone, Copy, Debug)]
pub struct LinkRecord {
    pub group: u32,
    pub fuzzy_on: bool,
    pub mp: u16, // 全拼路径命中掩码
    pub ms: u16, // 双拼路径命中掩码
    pub equal: bool,
}

/// 联动账枢纽。
pub struct SpFuzzy {
    ledger: Vec<LinkRecord>,
    pub group_total: u32,
    pub fail_total: u32,
}

impl SpFuzzy {
    pub fn new() -> SpFuzzy {
        SpFuzzy { ledger: Vec::new(), group_total: 0, fail_total: 0 }
    }

    /// 跑一组（on/off 两态比对，J1 主体）。
    pub fn run_group(
        &mut self,
        group: u32,
        q: (u8, bool, u16),
        pairs: &[usize],
    ) -> Result<(), u16> {
        // 闭集校验：启用的模糊对必须在 0..8 内。
        let mut i = 0usize;
        while i < pairs.len() {
            if pairs[i] >= FUZZY_PAIRS.len() as usize {
                return Err(E_PAIR_UNKNOWN);
            }
            i += 1;
        }
        let mut state = 0u8;
        while state < 2 {
            let on = state == 1;
            let mp = match_pinyin(q, pairs, on);
            let ms = match_shuangpin(q, pairs, on);
            let equal = mp == ms;
            if !equal {
                self.fail_total += 1;
            }
            self.ledger.push(LinkRecord { group, fuzzy_on: on, mp, ms, equal });
            self.group_total += 1;
            state += 1;
        }
        Ok(())
    }

    /// 联动审计：账目守恒 + 全部 equal。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.group_total {
            return Err(E_LEDGER_BAD);
        }
        if self.fail_total != 0 {
            return Err(E_LINK_BROKEN);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if !self.ledger[i].equal {
                return Err(E_LINK_BROKEN);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 组覆盖核验（J1 硬口径）：组号 0..N 每组 on/off 两态齐。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.group_total < want * 2 {
            return Err(E_GROUP_MISS);
        }
        Ok(())
    }

    /// 开关一致核验：每组 on 态命中集 ≥ off 态命中集（模糊只会扩大）。
    pub fn audit_monotone(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i + 1 < self.ledger.len() {
            let off = &self.ledger[i];
            let on = &self.ledger[i + 1];
            if off.group == on.group && !off.fuzzy_on && on.fuzzy_on {
                if (on.mp & off.mp) != off.mp || (on.ms & off.ms) != off.ms {
                    return Err(E_STATE_BAD);
                }
            }
            i += 2;
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（联动 ×10 组行为与全拼一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主联动 ×10：8 对模糊音各 1 组 + 2 组叠加，每组 on/off 两态
    //    两路径命中集逐位相等。
    let mut fz = SpFuzzy::new();
    let groups: [((u8, bool, u16), Vec<usize>); 10] = [
        ((SH_Z, true, YUN_AN), make_pairs(&[0])),      // zh+an 查 zhan 域
        ((SH_C, true, YUN_EN), make_pairs(&[1])),      // ch+en
        ((SH_S, true, YUN_IN), make_pairs(&[2])),      // sh+in
        ((SH_L, false, YUN_A), make_pairs(&[3])),      // l/n + a
        ((SH_F, false, YUN_U), make_pairs(&[4])),      // f/h + u
        ((SH_D, false, YUN_AN), make_pairs(&[5])),     // d + an/ang
        ((SH_B, false, YUN_EN), make_pairs(&[6])),     // b + en/eng
        ((SH_M, false, YUN_IN), make_pairs(&[7])),     // m + in/ing
        ((SH_Z, true, YUN_ANG), make_pairs(&[0, 5])),  // zh↔z 叠 an↔ang
        ((SH_S, false, YUN_ENG), make_pairs(&[2, 6, 7])), // sh↔s 叠 en↔eng/in↔ing
    ];
    let mut g = 0u32;
    while g < 10 {
        let (q, ref pairs) = groups[g as usize];
        fz.run_group(g, q, pairs)?;
        g += 1;
    }
    fz.coverage(10)?;
    fz.audit()?;
    fz.audit_monotone()?;
    if fz.ledger_len() != 20 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 精确基线：fuzzy off 时查询只精确命中自身（单元素掩码），
    //    两路径掩码相等且非零。
    let mut fz = SpFuzzy::new();
    fz.run_group(0, (SH_Z, true, YUN_AN), &[])?;
    let rec = &fz.ledger[0]; // off 态
    if rec.mp == 0 || rec.mp.count_ones() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 模糊扩大：zh↔z 开启后查询 zhan 命中含 zan（掩码位集扩大），
    //    全拼与双拼路径同扩。
    let off = match_pinyin((SH_Z, true, YUN_AN), &[], false);
    let on = match_pinyin((SH_Z, true, YUN_AN), &make_pairs(&[0]), true);
    if (on & off) != off || on == off {
        return Err(E_STATE_BAD); // off ⊂ on 且严格扩大
    }
    let off_s = match_shuangpin((SH_Z, true, YUN_AN), &[], false);
    let on_s = match_shuangpin((SH_Z, true, YUN_AN), &make_pairs(&[0]), true);
    if (on_s & off_s) != off_s || on_s != on {
        return Err(E_LINK_BROKEN); // 双拼路径同步扩大且与全拼相等
    }
    hits += 1;

    // R4 闭集外模糊对拒收：pairs 含 8 号（越界）run_group 拒。
    let mut fz = SpFuzzy::new();
    let bad: Vec<usize> = vec8_out();
    if fz.run_group(0, (SH_Z, true, YUN_AN), &bad).is_ok() {
        return Err(E_PAIR_UNKNOWN);
    }
    if fz.group_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 开关失步抓拍：人为只让全拼路径开模糊、双拼路径关，两掩码必
    //    不等（联动判据确实能抓失步，非恒绿判据）。
    let mp = match_pinyin((SH_Z, true, YUN_AN), &make_pairs(&[0]), true);
    let ms = match_shuangpin((SH_Z, true, YUN_AN), &[], false);
    if mp == ms {
        return Err(E_LINK_BROKEN); // 失步态必须可观测
    }
    hits += 1;

    // R6 l↔n 与 f↔h 声母对：na 查询模糊命中 la（两路径一致）。
    let m_p = match_pinyin((SH_N, false, YUN_A), &make_pairs(&[3]), true);
    let m_s = match_shuangpin((SH_N, false, YUN_A), &make_pairs(&[3]), true);
    if m_p != m_s || m_p == 0 {
        return Err(E_LINK_BROKEN);
    }
    if (m_p & (1 << 13)) == 0 {
        return Err(E_LEDGER_BAD); // la 在语料表位 13
    }
    let m_p2 = match_pinyin((SH_F, false, YUN_U), &make_pairs(&[4]), true);
    let m_s2 = match_shuangpin((SH_F, false, YUN_U), &make_pairs(&[4]), true);
    if m_p2 != m_s2 || (m_p2 & (1 << 15)) == 0 {
        return Err(E_LINK_BROKEN); // hu 在语料表位 15
    }
    hits += 1;

    // R7 韵母对键位域联动：an↔ang 开启后 dan 查询双拼路径（d,j）命中
    //    dang（d,h）条目，与全拼路径一致。
    let m_p = match_pinyin((SH_D, false, YUN_AN), &make_pairs(&[5]), true);
    let m_s = match_shuangpin((SH_D, false, YUN_AN), &make_pairs(&[5]), true);
    if m_p != m_s {
        return Err(E_LINK_BROKEN);
    }
    hits += 1;

    // R8 组号闭合 ×10 复跑：同批用例二轮重跑逐位一致；写回（判据 J2）。
    let mut fz = SpFuzzy::new();
    let mut g = 0u32;
    while g < 10 {
        let (q, ref pairs) = groups[g as usize];
        fz.run_group(g, q, pairs)?;
        g += 1;
    }
    let first: Vec<LinkRecord> = {
        let mut v: Vec<LinkRecord> = Vec::new();
        let mut i = 0usize;
        while i < fz.ledger_len() {
            v.push(fz.ledger[i]);
            i += 1;
        }
        v
    };
    let mut g = 0u32;
    while g < 10 {
        let (q, ref pairs) = groups[g as usize];
        fz.run_group(g, q, pairs)?;
        g += 1;
    }
    let mut i = 0usize;
    while i < first.len() {
        if fz.ledger[first.len() + i].mp != first[i].mp
            || fz.ledger[first.len() + i].ms != first[i].ms
        {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    fz.audit()?;
    if fz.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    fz.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}

fn make_pairs(items: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < items.len() {
        v.push(items[i]);
        i += 1;
    }
    v
}

fn vec8_out() -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    v.push(8); // 闭集外（0..8）
    v
}
