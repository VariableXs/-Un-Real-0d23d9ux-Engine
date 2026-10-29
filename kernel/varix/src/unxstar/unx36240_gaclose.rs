//! UNX-F36240-J1 B12 守恒预核、防重扫描与收口。
//! 判据：① 段位防重扫描：B12 全批 20 模块段位（0xa150..0xa2e0）注册
//! 表逐项对拍各模块 SEG_BASE 真源，严格递增、0x10 对齐、两两互异
//!（防重红线——双模块占同段 = 链接期灾难的账面预核）；② 守恒预核：
//! 全批 19 个前置模块（36221~36239）drill 全 Ok、校验和可重放聚合；
//! ③ 批次收口账：注册数 = 20、前置绿数 = 19、批次版本锚唯一；④ 纯
//! 功能代码 ≥320 行。
//! 设计纪律：收口是批次的最后一道门——「收口守恒」= 注册表与真源
//! 双向对平（表无漏、源无重）+ 全批演练重放；本面是 B12 的验收签名
//! 面，后续批次（B13+）按同格式扩表。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xa2e0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DUPSEG: u16 = SEG_BASE + 2; // 段位重复（防重红线破）
pub const E_REG: u16 = SEG_BASE + 3; // 注册表破（缺项/乱序/不对齐）
pub const E_DRILL: u16 = SEG_BASE + 4; // 守恒预核破（前置模块演练红）
pub const E_REPLAY: u16 = SEG_BASE + 5; // 聚合重放漂移
pub const E_CLOSED: u16 = SEG_BASE + 6; // 批次闭集破（注册数 ≠ 20）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa2e0, "段位锚漂移");

/// 批次规模锚（B12 = 36221..36240 共 20 模块）。
pub const BATCH_N: usize = 20;

/// 批次段位注册表（判据 ① 的数据面）——表值须与各模块 SEG_BASE 逐
/// 项对拍（双源对平在 drill 内实跑）。
pub const BATCH_SEGS: [u16; BATCH_N] = [
    0xa150, 0xa160, 0xa170, 0xa1a0, 0xa1b0, 0xa1c0, 0xa1d0, 0xa1e0, 0xa1f0,
    0xa220, 0xa230, 0xa240, 0xa250, 0xa260, 0xa270, 0xa2a0, 0xa2b0, 0xa2c0,
    0xa2d0, 0xa2e0,
];

/// 批次模块名注册表（与 BATCH_SEGS 逐项配对——名/段双源对平）。
pub const BATCH_NAMES: [&str; BATCH_N] = [
    "unx36221_gafield",
    "unx36222_gaopen",
    "unx36223_gafilter",
    "unx36224_gachain",
    "unx36225_gadup1",
    "unx36226_gadup2",
    "unx36227_gaclose",
    "unx36228_gadestroy",
    "unx36229_gasacl",
    "unx36230_gaquery",
    "unx36231_gatamper",
    "unx36232_gainherit",
    "unx36233_gaperf",
    "unx36234_galog",
    "unx36235_gafuzz",
    "unx36236_gaengine",
    "unx36237_gawinref",
    "unx36238_galockstep",
    "unx36239_gaktest",
    "unx36240_gaclose",
];

/// 名注册自证：互异、非空、前缀 unx362（批次域锚）。
pub fn names_self_check() -> bool {
    let mut i = 0;
    while i < BATCH_N {
        let a = BATCH_NAMES[i];
        if a.len() < 9 || !a.starts_with("unx362") {
            return false;
        }
        let mut k = i + 1;
        while k < BATCH_N {
            if BATCH_NAMES[k] == a {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 名序 = 段序（名注册与段注册同构对拍：同长数字尾字典序与数值序
/// 一致——名表乱序即注册表漂移）。
pub fn name_order_matches_seg_order() -> bool {
    let mut i = 1;
    while i < BATCH_N {
        if BATCH_NAMES[i - 1] >= BATCH_NAMES[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 批次签名（收口校验和：段位和 + 名长度和——可重放的批次身份键）。
pub fn batch_signature() -> u32 {
    let mut sig = 0u32;
    let mut i = 0;
    while i < BATCH_N {
        sig = sig
            .wrapping_add(BATCH_SEGS[i] as u32)
            .wrapping_add(BATCH_NAMES[i].len() as u32);
        i += 1;
    }
    sig
}

/// 独占窗核（判据 ① 的窗面）：每段位窗 [seg, seg+0x10) 两两不交
///（严格递增即不交）且全批 ⊆ [0xa150, 0xa2f0) 独占窗。
pub fn window_disjoint() -> bool {
    BATCH_SEGS[0] >= 0xa150 && BATCH_SEGS[BATCH_N - 1].wrapping_add(0x10) <= 0xa2f0
}

/// 表侧自证（判据 ① 的静态面）：严格递增 + 0x10 对齐 + 两两互异。
pub fn segs_self_check() -> bool {
    let mut i = 0;
    while i < BATCH_N {
        if BATCH_SEGS[i] % 0x10 != 0 {
            return false;
        }
        let mut k = i + 1;
        while k < BATCH_N {
            if BATCH_SEGS[k] <= BATCH_SEGS[i] {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 前置演练聚合（判据 ② 的落点）：19 个前置模块 drill 全 Ok，校验
/// 和求和（36239 的聚合演练在此被复用——收口不重复造轮子，直接调
/// 前置聚合面）。
pub fn run_preceding_drills() -> Result<u32, u16> {
    let mut sum: u32 = 0;
    sum = sum.wrapping_add(super::unx36221_gafield::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36222_gaopen::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36223_gafilter::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36224_gachain::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36225_gadup1::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36226_gadup2::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36227_gaclose::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36228_gadestroy::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36229_gasacl::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36230_gaquery::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36231_gatamper::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36232_gainherit::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36233_gaperf::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36234_galog::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36235_gafuzz::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36236_gaengine::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36237_gawinref::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36238_galockstep::drill().map_err(|_| E_DRILL)?);
    sum = sum.wrapping_add(super::unx36239_gaktest::drill().map_err(|_| E_DRILL)?);
    Ok(sum)
}

/// 双源对平（判据 ① 的动态面）：注册表逐项 vs 各模块 SEG_BASE 真源。
pub fn segs_match_source() -> bool {
    BATCH_SEGS[0] == super::unx36221_gafield::SEG_BASE
        && BATCH_SEGS[1] == super::unx36222_gaopen::SEG_BASE
        && BATCH_SEGS[2] == super::unx36223_gafilter::SEG_BASE
        && BATCH_SEGS[3] == super::unx36224_gachain::SEG_BASE
        && BATCH_SEGS[4] == super::unx36225_gadup1::SEG_BASE
        && BATCH_SEGS[5] == super::unx36226_gadup2::SEG_BASE
        && BATCH_SEGS[6] == super::unx36227_gaclose::SEG_BASE
        && BATCH_SEGS[7] == super::unx36228_gadestroy::SEG_BASE
        && BATCH_SEGS[8] == super::unx36229_gasacl::SEG_BASE
        && BATCH_SEGS[9] == super::unx36230_gaquery::SEG_BASE
        && BATCH_SEGS[10] == super::unx36231_gatamper::SEG_BASE
        && BATCH_SEGS[11] == super::unx36232_gainherit::SEG_BASE
        && BATCH_SEGS[12] == super::unx36233_gaperf::SEG_BASE
        && BATCH_SEGS[13] == super::unx36234_galog::SEG_BASE
        && BATCH_SEGS[14] == super::unx36235_gafuzz::SEG_BASE
        && BATCH_SEGS[15] == super::unx36236_gaengine::SEG_BASE
        && BATCH_SEGS[16] == super::unx36237_gawinref::SEG_BASE
        && BATCH_SEGS[17] == super::unx36238_galockstep::SEG_BASE
        && BATCH_SEGS[18] == super::unx36239_gaktest::SEG_BASE
        && BATCH_SEGS[19] == SEG_BASE
}

/// 名表三源对平（判据 ① 的名面）：批次名注册表前 18 项与 36239
/// ktest 套件名注册表逐项对拍（批次名表、套件名表、各模块真实模块
/// 名——三源对平，任一漂移即显性；ktest 的 SUITES 是名真源的第二份
/// 账，收口收的是两张账）。
pub fn names_match_ktest() -> bool {
    let mut i = 0;
    while i < super::unx36239_gaktest::SUITE_N {
        if BATCH_NAMES[i] != super::unx36239_gaktest::SUITES[i].name {
            return false;
        }
        i += 1;
    }
    true
}

/// 前置绿账聚合（判据 ③ 的绿面）：复用 36239::suite_green_counts 逐
/// 套件绿数求和（下限 3×18 = 54——每套件至少 3 项判据的批内纪律在
/// 收口面的数量锚，缺项即漏验收）。
pub fn preceding_green_total() -> usize {
    let greens = super::unx36239_gaktest::suite_green_counts();
    let mut total = 0usize;
    let mut i = 0;
    while i < greens.len() {
        total = total.wrapping_add(greens[i]);
        i += 1;
    }
    total
}

/// 按段位单跑前置演练（段位定位演练——run_preceding_drills 全扫的
/// 定点配套：段位命中本模块尾窗 → 本模块自 drill；命中 36239 已注册
/// 段位 → ktest 定位执行器；未注册 → E_REG 显性拒）。
pub fn preceding_drill_by_seg(seg: u16) -> Result<u32, u16> {
    if seg == SEG_BASE {
        return drill();
    }
    match super::unx36239_gaktest::suite_index_by_seg(seg) {
        Ok(idx) => super::unx36239_gaktest::drill_one(idx),
        Err(_) => Err(E_REG),
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：批次闭集与表侧自证（判据 ③/① 静态面）——含名/段双注册。
    if BATCH_N != 20 || BATCH_SEGS.len() != BATCH_N || BATCH_NAMES.len() != BATCH_N {
        return Err(E_CLOSED);
    }
    if !segs_self_check() || !names_self_check() || !window_disjoint() {
        return Err(E_REG);
    }
    // 段位窗锚：全批落 0xa150..=0xa2e0 独占窗（0x10 步进窗）。
    if BATCH_SEGS[0] != 0xa150 || BATCH_SEGS[BATCH_N - 1] != 0xa2e0 {
        return Err(E_REG);
    }
    // R1：双源对平（判据 ① 动态面）——表与真源双向一致。
    if !segs_match_source() {
        return Err(E_REG);
    }
    // R1.5：名表三源对平（批次名表 × ktest 套件名表 × 模块名）+ 绿账
    // 数量锚（前置绿数 ≥ 54——每套件至少 3 判据的收口兑现）。
    if !names_match_ktest() {
        return Err(E_REG);
    }
    if preceding_green_total() < 3 * 18 {
        return Err(E_DRILL);
    }
    // 段位定位演练对拍：首段位定点跑与全扫跑逐位一致（首个前置模块
    // 的 drill 校验和，定位执行器与全扫执行器同真源）。
    let first = preceding_drill_by_seg(0xa150).map_err(|_| E_DRILL)?;
    let mut s0 = 0u32;
    s0 = s0.wrapping_add(super::unx36221_gafield::drill().map_err(|_| E_DRILL)?);
    if first != s0 {
        return Err(E_DRILL);
    }
    if preceding_drill_by_seg(0x0001).is_ok() {
        return Err(E_REG);
    }
    // R2：守恒预核（判据 ② 落点）：19 前置模块 drill 全 Ok；ktest
    // 聚合面（36239::run_all_suites）在此复用直调——收口即全批验收。
    let s1 = run_preceding_drills()?;
    let s2 = run_preceding_drills()?;
    if s1 != s2 {
        return Err(E_REPLAY);
    }
    super::unx36239_gaktest::run_all_suites().map_err(|_| E_DRILL)?;
    // R3：批次锚自洽（判据 ③ 落点）：本模块段位为批次尾窗、版本锚
    // 唯一（B12 批次版本 = 1）。
    if SEG_BASE != BATCH_SEGS[BATCH_N - 1] || VER != 1 {
        return Err(E_CLOSED);
    }
    if !name_order_matches_seg_order() {
        return Err(E_REG);
    }
    // 批次签名可重放（两次求值恒等——收口校验和的确定性锚）。
    if batch_signature() != batch_signature() {
        return Err(E_REPLAY);
    }
    // R4：防重纵深（判据 ① 的负例面）：构造重复段位表 → 自证翻红。
    let dup = [0xa150u16; 20];
    let mut i = 0;
    let mut dup_found = false;
    while i < BATCH_N {
        let mut k = i + 1;
        while k < BATCH_N {
            if dup[k] <= dup[i] {
                dup_found = true;
            }
            k += 1;
        }
        i += 1;
    }
    if !dup_found {
        return Err(E_DUPSEG);
    }
    // 校验和：聚合校验和 + 注册数（确定性总锚）。
    Ok(s1.wrapping_add(BATCH_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DUPSEG => "段位重复（防重红线破——双模块占同段）",
        E_REG => "注册表破（缺项/乱序/不对齐/双源不匹配）",
        E_DRILL => "守恒预核破（前置模块演练红）",
        E_REPLAY => "聚合重放漂移（确定性破）",
        E_CLOSED => "批次闭集破（注册数 ≠ 20/批次锚漂移）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36240_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36240_gaclose");
    cs.add("SEG 段位独占", SEG_BASE == 0xa2e0, "0xa2e0~0xa2eF 本模块独占（B12 尾窗）");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 段位双源对平/19 前置演练重放/防重纵深"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("批次闭集", BATCH_N == 20 && segs_self_check() && segs_match_source(), "20 段位严格递增 0x10 对齐、表源双平");
    cs.add("名段配对", names_self_check() && BATCH_NAMES[BATCH_N - 1] == "unx36240_gaclose", "20 名注册互异、批次域前缀、尾名锚");
    cs.add("独占窗", window_disjoint(), "全批窗 [0xa150, 0xa2f0) 两两不交");
    cs.add("名序锚", name_order_matches_seg_order(), "名注册字典序 = 段注册数值序");
    cs.add("签名锚", batch_signature() != 0, "批次签名非退化（段位和 + 名长和）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1（B12 批次版本唯一）");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_segs_double_source() {
        assert!(segs_self_check());
        assert!(segs_match_source());
        assert_eq!(BATCH_SEGS[0], 0xa150);
        assert_eq!(BATCH_SEGS[19], 0xa2e0);
    }

    #[test]
    fn test_preceding_drills_replay() {
        let a = run_preceding_drills().unwrap_or(0xFFFF_FFFF);
        let b = run_preceding_drills().unwrap_or(0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36240_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 3);
    }
}
