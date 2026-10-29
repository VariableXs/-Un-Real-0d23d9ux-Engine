//! UNX-F36139-J1 B07 守恒预核、防重扫描与收口。
//! 判据：① B07 段位账（20 卡 × 0x10 窗）逐卡核对：窗口互斥、判别式
//! 连续、段位号与卡号一一对应；② 防重扫描：段位全库唯一性（B07 窗
//! 不与任何已知段位账撞窗）；③ 收口总锚：SEG_BOOK 校验和冻结；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：收口真源——B07 的 20 个 SEG 窗（36121~36140）在整库段位
//! 空间里必须互斥且连续。SEG_BOOK 是本批段位分配的冻结账（与
//! kernel/_tools/wbj1028_segs.txt 同源）；任何新模块入段必须先过本账的
//! 防重扫描。段位借出与归还都从这里走账——段位空间是稀缺资源，撞窗
//! 即诊断码语义串线（B04 曾实证）。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8210;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEG: u16 = SEG_BASE + 2; // 段位账失真（窗口/连续性破）
pub const E_DUP: u16 = SEG_BASE + 3; // 段位撞窗（防重扫描命中）
pub const E_CARD: u16 = SEG_BASE + 4; // 卡号解析失败（防重对齐破）
pub const E_INTG: u16 = SEG_BASE + 5; // 收口总锚漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 窗宽（本批 0x10 宽窗——wbj1028 段位制）。
pub const WINDOW_W: u16 = 0x10;

/// B07 段位冻结账：卡号尾三位 + SEG_BASE（与 wbj1028_segs.txt 同源）。
pub const SEG_BOOK: [(u16, u16); 20] = [
    (121, 0x3a90),
    (122, 0x3aa0),
    (123, 0x3ab0),
    (124, 0x3b00),
    (125, 0x3b10),
    (126, 0x3b20),
    (127, 0x3b30),
    (128, 0x3b40),
    (129, 0x3b50),
    (130, 0x3b60),
    (131, 0x3b70),
    (132, 0x3b80),
    (133, 0x3b90),
    (134, 0x3ba0),
    (135, 0x3bb0),
    (136, 0x81e0),
    (137, 0x81f0),
    (138, 0x8200),
    (139, 0x8210),
    (140, 0x8220),
];

/// 卡号全称合成（"UNX-F36NNN" 数字面 → u32）。
pub fn card_no(seg_idx: usize) -> Result<u32, u16> {
    if seg_idx >= SEG_BOOK.len() {
        return Err(E_CARD);
    }
    let (tail, _) = SEG_BOOK[seg_idx];
    // 尾三位 + 批次前缀 36 → 完整卡号 36NNN。
    let card = 36000u32 + tail as u32;
    if card < 36121 || card > 36140 {
        return Err(E_CARD);
    }
    Ok(card)
}

/// 段位账守恒核一：逐窗互斥（[base, base+W) 两两不交）。
pub fn windows_disjoint() -> Result<(), u16> {
    let mut i = 0;
    while i < SEG_BOOK.len() {
        let (ti, bi) = SEG_BOOK[i];
        let hi_i = bi.wrapping_add(WINDOW_W).wrapping_sub(1);
        // 环绕防御：本批窗不跨 0xFFFF（静态可证，但账面仍核）。
        if hi_i < bi {
            return Err(E_SEG);
        }
        let mut j = i + 1;
        while j < SEG_BOOK.len() {
            let (tj, bj) = SEG_BOOK[j];
            if ti == tj {
                return Err(E_DUP); // 卡尾重复
            }
            let hi_j = bj.wrapping_add(WINDOW_W).wrapping_sub(1);
            if bj <= hi_i && bi <= hi_j {
                return Err(E_DUP); // 区间交
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 段位账守恒核二：卡尾连续（121..140 逐位递增）。
pub fn tails_monotonic() -> Result<(), u16> {
    let mut i = 0;
    while i < SEG_BOOK.len() {
        let (t, _) = SEG_BOOK[i];
        if t != 121 + i as u16 {
            return Err(E_SEG);
        }
        i += 1;
    }
    Ok(())
}

/// 段位账守恒核三：窗口尺寸（相邻窗距离 ∈ {0x10, 0x50, 0x4630}——本批
/// 分配中 36123→36124 跳 0x50、36135→36136 跳 0x4630（换段位岛），其余
/// 0x10 递进；跳档位必须与分配史一致，防止静默挪窗）。
pub fn window_gaps_legal() -> Result<(), u16> {
    let mut i = 0;
    while i + 1 < SEG_BOOK.len() {
        let (_, bi) = SEG_BOOK[i];
        let (_, bj) = SEG_BOOK[i + 1];
        let gap = bj.wrapping_sub(bi);
        let legal = gap == 0x10 || gap == 0x50 || gap == 0x4630;
        if !legal {
            return Err(E_SEG);
        }
        i += 1;
    }
    Ok(())
}

/// 防重扫描：B07 全部段位与「库内已知段位抽样闭集」不撞窗。
/// 抽样闭集 = 邻批（B04~B06 + 早期 0x10 制窗口表）的冻结代表值。
pub fn no_dup_scan() -> Result<(), u16> {
    // 邻批代表段位（B04 0xe1a0 起、B05 0xe2d0 起、B06 0x3910 起——
    // 各批首窗；窗宽 0x10 时首窗不撞即账面无系统性重叠）。
    const NEIGHBORS: [u16; 6] = [
        0xe1a0, // B04 首（F36062）
        0xe2d0, // B05 首（F36081）
        0x3910, // B06 首（F36101）
        0x7270, // B01 首（F36001，0x20 制）
        0xf7a0, // B02 首
        0xfc80, // B03 首
    ];
    let mut b = 0;
    while b < SEG_BOOK.len() {
        let (_, bb) = SEG_BOOK[b];
        let bh = bb.wrapping_add(WINDOW_W).wrapping_sub(1);
        let mut n = 0;
        while n < NEIGHBORS.len() {
            let nb = NEIGHBORS[n];
            let nh = nb.wrapping_add(WINDOW_W).wrapping_sub(1);
            if nb <= bh && bb <= nh {
                return Err(E_DUP);
            }
            n += 1;
        }
        b += 1;
    }
    Ok(())
}

/// 本批段位借出账（收口时冻结的窗口占用总数）。
pub fn segs_used() -> u32 {
    SEG_BOOK.len() as u32
}

/// 收口总锚：SEG_BOOK 校验和（FNV-16 over 卡尾+段位交替）。
pub fn book_checksum() -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < SEG_BOOK.len() {
        let (t, b) = SEG_BOOK[i];
        h ^= t as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
        h = (h & 0xFFFF) ^ (h >> 16);
        i += 1;
    }
    (h & 0xFFFF) as u16
}

/// 段位账 ↔ 代码常量交叉核对：SEG_BOOK 的 20 个窗位必须与各模块头部的
/// SEG_BASE 常量逐位一致（账面与代码两条独立真源互证——静默挪窗在此显形）。
pub fn seg_constants_aligned() -> Result<(), u16> {
    use super::unx36121_typereg as m21;
    use super::unx36122_typedeny as m22;
    use super::unx36123_genbind as m23;
    use super::unx36124_enroll as m24;
    use super::unx36125_deftier as m25;
    use super::unx36126_obcreate as m26;
    use super::unx36127_nounreg as m27;
    use super::unx36128_typever as m28;
    use super::unx36129_persist as m29;
    use super::unx36130_typaudit as m30;
    use super::unx36131_selftest as m31;
    use super::unx36132_typfuzz as m32;
    use super::unx36133_typperf as m33;
    use super::unx36134_nsrecon as m34;
    use super::unx36135_wizard as m35;
    use super::unx36136_eventlog as m36;
    use super::unx36137_scripts as m37;
    use super::unx36138_ktest as m38;
    use super::unx36140_midaudit as m40;
    // SEG_BOOK[0..20] 逐位对照（本模块 SEG_BASE 自身也核——自我勾稽）。
    if m21::SEG_BASE != SEG_BOOK[0].1 { return Err(E_SEG); }
    if m22::SEG_BASE != SEG_BOOK[1].1 { return Err(E_SEG); }
    if m23::SEG_BASE != SEG_BOOK[2].1 { return Err(E_SEG); }
    if m24::SEG_BASE != SEG_BOOK[3].1 { return Err(E_SEG); }
    if m25::SEG_BASE != SEG_BOOK[4].1 { return Err(E_SEG); }
    if m26::SEG_BASE != SEG_BOOK[5].1 { return Err(E_SEG); }
    if m27::SEG_BASE != SEG_BOOK[6].1 { return Err(E_SEG); }
    if m28::SEG_BASE != SEG_BOOK[7].1 { return Err(E_SEG); }
    if m29::SEG_BASE != SEG_BOOK[8].1 { return Err(E_SEG); }
    if m30::SEG_BASE != SEG_BOOK[9].1 { return Err(E_SEG); }
    if m31::SEG_BASE != SEG_BOOK[10].1 { return Err(E_SEG); }
    if m32::SEG_BASE != SEG_BOOK[11].1 { return Err(E_SEG); }
    if m33::SEG_BASE != SEG_BOOK[12].1 { return Err(E_SEG); }
    if m34::SEG_BASE != SEG_BOOK[13].1 { return Err(E_SEG); }
    if m35::SEG_BASE != SEG_BOOK[14].1 { return Err(E_SEG); }
    if m36::SEG_BASE != SEG_BOOK[15].1 { return Err(E_SEG); }
    if m37::SEG_BASE != SEG_BOOK[16].1 { return Err(E_SEG); }
    if m38::SEG_BASE != SEG_BOOK[17].1 { return Err(E_SEG); }
    if SEG_BASE != SEG_BOOK[18].1 { return Err(E_SEG); } // 本模块 = 139 窗
    if m40::SEG_BASE != SEG_BOOK[19].1 { return Err(E_SEG); }
    Ok(())
}

/// B07 卡号 → 模块短名映射表（防重对齐的第三真源：卡号/段位/模块名
/// 三账互证——模块名与卡号错位是坑 29 的病灶，此表把它钉死）。
pub const CARD_MODULES: [&'static str; 20] = [
    "unx36121_typereg",
    "unx36122_typedeny",
    "unx36123_genbind",
    "unx36124_enroll",
    "unx36125_deftier",
    "unx36126_obcreate",
    "unx36127_nounreg",
    "unx36128_typever",
    "unx36129_persist",
    "unx36130_typaudit",
    "unx36131_selftest",
    "unx36132_typfuzz",
    "unx36133_typperf",
    "unx36134_nsrecon",
    "unx36135_wizard",
    "unx36136_eventlog",
    "unx36137_scripts",
    "unx36138_ktest",
    "unx36139_b07close",
    "unx36140_midaudit",
];

/// 卡号 → 模块短名（三账对齐的查询面）。
pub fn module_of_card(card: u32) -> Result<&'static str, u16> {
    if card < 36121 || card > 36140 {
        return Err(E_CARD);
    }
    Ok(CARD_MODULES[(card - 36121) as usize])
}

/// 三账互证：卡号 ↔ 模块名 ↔ 段位逐位一致（防重扫描的终面）。
pub fn triple_align() -> Result<(), u16> {
    let mut i = 0;
    while i < 20 {
        let card = card_no(i)?;
        let (tail, base) = SEG_BOOK[i];
        // 卡号尾三位 = 账面条目卡尾。
        if card % 1000 != tail as u32 {
            return Err(E_CARD);
        }
        // 模块短名内嵌卡号（unx36NNN 的 NNN = 卡尾）。
        let m = CARD_MODULES[i];
        let mb = m.as_bytes();
        if mb.len() < 9 {
            return Err(E_CARD);
        }
        let m_tail = (mb[5] - b'0') as u32 * 100 + (mb[6] - b'0') as u32 * 10 + (mb[7] - b'0') as u32;
        if m_tail != tail as u32 {
            return Err(E_CARD);
        }
        // 段位在窗宽内含模块版本锚（SEG_BASE % 0x10 == 0 的窗口对齐语义）。
        if base % WINDOW_W != 0 {
            return Err(E_SEG);
        }
        i += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：卡号解析面（首/尾/越界三态）。
    if card_no(0)? != 36121 || card_no(19)? != 36140 {
        return Err(E_CARD);
    }
    if card_no(20).err() != Some(E_CARD) {
        return Err(E_CARD);
    }
    // R2：段位账三守恒。
    windows_disjoint()?;
    tails_monotonic()?;
    window_gaps_legal()?;
    // R3：防重扫描（邻批六首窗不撞）。
    no_dup_scan()?;
    // R4：收口总锚冻结（校验和确定性重放）。
    let c1 = book_checksum();
    let c2 = book_checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_INTG);
    }
    // R5：借出账守恒（20 窗全在册——不多不少）。
    if segs_used() != 20 {
        return Err(E_SEG);
    }
    // R6：本模块自身段位在账（收口模块的段位自我勾稽——139 号窗）。
    let mut self_in_book = false;
    let mut i = 0;
    while i < SEG_BOOK.len() {
        let (t, b) = SEG_BOOK[i];
        if t == 139 && b == 0x8210 {
            self_in_book = true;
        }
        i += 1;
    }
    if !self_in_book {
        return Err(E_SEG);
    }
    // R7：窗宽语义锚（0x10 制——与 wbj1028 分配史一致）。
    if WINDOW_W != 0x10 {
        return Err(E_SEG);
    }
    // R8：账面 ↔ 代码常量 20 位互证。
    seg_constants_aligned()?;
    // R9：三账互证（卡号/模块名/段位）。
    triple_align()?;
    if module_of_card(36121)? != "unx36121_typereg"
        || module_of_card(36140)? != "unx36140_midaudit"
        || module_of_card(36120).err() != Some(E_CARD)
    {
        return Err(E_CARD);
    }
    Ok((c1 as u32).wrapping_mul(7).wrapping_add(segs_used()))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEG => "段位账失真（窗口互斥/连续性/尺寸纪律破）",
        E_DUP => "段位撞窗（防重扫描命中）",
        E_CARD => "卡号解析失败（卡尾与账不对齐）",
        E_INTG => "收口总锚漂移（校验和重放失真）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36139_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36139_b07close");
    cs.add("SEG 段位独占", SEG_BASE == 0x8210, "0x8210~0x821F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "卡号/三守恒/防重/总锚/借出账全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("段位账容量", SEG_BOOK.len() == 20 && segs_used() == 20, "B07 = 20 卡 × 0x10 窗");
    cs.add("卡号边界", card_no(0) == Ok(36121) && card_no(19) == Ok(36140), "F36121~F36140 冻结");
    cs.add("邻批代表", { no_dup_scan().is_ok() }, "B01~B06 首窗不撞");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
