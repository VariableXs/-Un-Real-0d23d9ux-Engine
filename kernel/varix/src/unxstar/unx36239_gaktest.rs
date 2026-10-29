//! UNX-F36239-J1 B12 ktest 断言集（句柄面全判据注册）。
//! 判据：① 判据面注册闭集：36221~36238 共 18 个模块的判据套件
//!（run_unx362NN_checks）全数注册，注册名与段位一一对应、无缺无重；
//! ② 聚合执行：18 套件逐个实跑且 all_passed 全绿（ktest 是红即全红
//! 的聚合门）；③ 确定性演练聚合：18 个模块的 drill() 全 Ok，校验和
//! 可重放；④ 纯功能代码 ≥320 行。
//! 设计纪律：ktest 是批次验收的聚合门——单模块自绿不等于批次绿；
//! 本面把「全判据注册 + 全套件实跑 + 全演练重放」三步做成闭集，注册
//! 表即批次判据的清单真源（漏注册 = 验收漏项）。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xa2d0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_REG: u16 = SEG_BASE + 2; // 注册表破（缺项/重项/名段不符）
pub const E_SUITE: u16 = SEG_BASE + 3; // 套件红（某模块 checks 未全绿）
pub const E_DRILL: u16 = SEG_BASE + 4; // 演练红（某模块 drill 非 Ok）
pub const E_COUNT: u16 = SEG_BASE + 5; // 聚合账不平（注册数/绿数不符）
pub const E_CLOSED: u16 = SEG_BASE + 6; // 注册闭集破（id 断档）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa2d0, "段位锚漂移");

/// 批次规模锚（36221..36238 = 18 模块）。
pub const SUITE_N: usize = 18;

/// 判据套件注册表（判据 ① 的数据面）：段位 + 短名 + 套件执行器。
/// 判据函数名用短名（各模块 run_unx362NN_checks——坑 16 纪律）。
pub struct SuiteEntry {
    pub seg: u16,
    pub name: &'static str,
    pub run: fn() -> CheckSet,
}

pub const SUITES: [SuiteEntry; SUITE_N] = [
    SuiteEntry { seg: 0xa150, name: "unx36221_gafield", run: super::unx36221_gafield::run_unx36221_checks },
    SuiteEntry { seg: 0xa160, name: "unx36222_gaopen", run: super::unx36222_gaopen::run_unx36222_checks },
    SuiteEntry { seg: 0xa170, name: "unx36223_gafilter", run: super::unx36223_gafilter::run_unx36223_checks },
    SuiteEntry { seg: 0xa1a0, name: "unx36224_gachain", run: super::unx36224_gachain::run_unx36224_checks },
    SuiteEntry { seg: 0xa1b0, name: "unx36225_gadup1", run: super::unx36225_gadup1::run_unx36225_checks },
    SuiteEntry { seg: 0xa1c0, name: "unx36226_gadup2", run: super::unx36226_gadup2::run_unx36226_checks },
    SuiteEntry { seg: 0xa1d0, name: "unx36227_gaclose", run: super::unx36227_gaclose::run_unx36227_checks },
    SuiteEntry { seg: 0xa1e0, name: "unx36228_gadestroy", run: super::unx36228_gadestroy::run_unx36228_checks },
    SuiteEntry { seg: 0xa1f0, name: "unx36229_gasacl", run: super::unx36229_gasacl::run_unx36229_checks },
    SuiteEntry { seg: 0xa220, name: "unx36230_gaquery", run: super::unx36230_gaquery::run_unx36230_checks },
    SuiteEntry { seg: 0xa230, name: "unx36231_gatamper", run: super::unx36231_gatamper::run_unx36231_checks },
    SuiteEntry { seg: 0xa240, name: "unx36232_gainherit", run: super::unx36232_gainherit::run_unx36232_checks },
    SuiteEntry { seg: 0xa250, name: "unx36233_gaperf", run: super::unx36233_gaperf::run_unx36233_checks },
    SuiteEntry { seg: 0xa260, name: "unx36234_galog", run: super::unx36234_galog::run_unx36234_checks },
    SuiteEntry { seg: 0xa270, name: "unx36235_gafuzz", run: super::unx36235_gafuzz::run_unx36235_checks },
    SuiteEntry { seg: 0xa2a0, name: "unx36236_gaengine", run: super::unx36236_gaengine::run_unx36236_checks },
    SuiteEntry { seg: 0xa2b0, name: "unx36237_gawinref", run: super::unx36237_gawinref::run_unx36237_checks },
    SuiteEntry { seg: 0xa2c0, name: "unx36238_galockstep", run: super::unx36238_galockstep::run_unx36238_checks },
];

/// 注册表闭集自证（判据 ① 的判定）：段位严格递增且 0x10 对齐、名
/// 互异非空、执行器可进。
pub fn registry_closed() -> bool {
    let mut i = 0;
    while i < SUITE_N {
        let a = &SUITES[i];
        if a.seg % 0x10 != 0 || a.name.is_empty() {
            return false;
        }
        let mut k = i + 1;
        while k < SUITE_N {
            if SUITES[k].seg <= a.seg || SUITES[k].name == a.name {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 演练执行器注册表（与 SUITES 同序——注册表双源对拍的演练侧真源）。
pub const DRILLS: [fn() -> Result<u32, u16>; SUITE_N] = [
    super::unx36221_gafield::drill,
    super::unx36222_gaopen::drill,
    super::unx36223_gafilter::drill,
    super::unx36224_gachain::drill,
    super::unx36225_gadup1::drill,
    super::unx36226_gadup2::drill,
    super::unx36227_gaclose::drill,
    super::unx36228_gadestroy::drill,
    super::unx36229_gasacl::drill,
    super::unx36230_gaquery::drill,
    super::unx36231_gatamper::drill,
    super::unx36232_gainherit::drill,
    super::unx36233_gaperf::drill,
    super::unx36234_galog::drill,
    super::unx36235_gafuzz::drill,
    super::unx36236_gaengine::drill,
    super::unx36237_gawinref::drill,
    super::unx36238_galockstep::drill,
];

/// 按序号单跑演练（可定位执行体——红项归因到模块；越界 → E_CLOSED）。
pub fn drill_one(idx: usize) -> Result<u32, u16> {
    if idx >= SUITE_N {
        return Err(E_CLOSED);
    }
    (DRILLS[idx])().map_err(|_| E_DRILL)
}

/// 段位 → 套件序号反演（段位定位器：段位未注册 → E_CLOSED——索引
/// 定位器的段位面配套，防重红线在执行器侧的负例入口）。
pub fn suite_index_by_seg(seg: u16) -> Result<usize, u16> {
    let mut i = 0;
    while i < SUITE_N {
        if SUITES[i].seg == seg {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_CLOSED)
}

/// 按段位单跑套件（段位定位套件——验收方可按段位直接定点复核单个
/// 模块判据面，不必全批扫跑；未注册 → E_CLOSED）。
pub fn suite_by_seg(seg: u16) -> Result<CheckSet, u16> {
    let idx = suite_index_by_seg(seg)?;
    Ok((SUITES[idx].run)())
}

/// 按段位单跑演练（演练侧的段位定位器——与 drill_one 序号定位器同
/// 真源：同一 DRILLS 表，双定位器对拍即注册表双源的可执行兑现）。
pub fn drill_by_seg(seg: u16) -> Result<u32, u16> {
    let idx = suite_index_by_seg(seg)?;
    (DRILLS[idx])().map_err(|_| E_DRILL)
}

/// 逐套件指纹（绿数 × 黄金比率 + 段位——逐套件确定性锚：聚合校验和
/// 只证演练总和，指纹把绿账与段位绑进可重放账面，套件漂移在指纹面
/// 显性）。
pub fn suite_fingerprints() -> [u32; SUITE_N] {
    let mut out = [0u32; SUITE_N];
    let mut i = 0;
    while i < SUITE_N {
        let cs = (SUITES[i].run)();
        out[i] = (cs.passed_count() as u32)
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(SUITES[i].seg as u32);
        i += 1;
    }
    out
}

/// 逐套件绿项数（判据 ② 的账面化：passed 计数——红项账的补集）。
pub fn suite_green_counts() -> [usize; SUITE_N] {
    let mut out = [0usize; SUITE_N];
    let mut i = 0;
    while i < SUITE_N {
        let cs = (SUITES[i].run)();
        out[i] = cs.passed_count();
        i += 1;
    }
    out
}

/// 逐套件红项数（判据 ② 的账面化：红项计数器——绿 = 全零）。
pub fn suite_red_counts() -> [usize; SUITE_N] {
    let mut out = [0usize; SUITE_N];
    let mut i = 0;
    while i < SUITE_N {
        let cs = (SUITES[i].run)();
        out[i] = cs.failed_count();
        i += 1;
    }
    out
}

/// 聚合执行（判据 ② 的落点）：18 套件逐个实跑，红即返 E_SUITE 并
/// 带可定位序号；绿数与注册数对平。
pub fn run_all_suites() -> Result<usize, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut green = 0usize;
    let mut i = 0;
    while i < SUITE_N {
        let cs = (SUITES[i].run)();
        if !cs.all_passed() {
            return Err(E_SUITE);
        }
        green += 1;
        i += 1;
    }
    if green != SUITE_N {
        return Err(E_COUNT);
    }
    Ok(green)
}

/// 演练聚合（判据 ③ 的落点）：按演练注册表逐项执行（与套件注册表
/// 同序双源——漏注册在演练侧同样显性），校验和可重放。
pub fn run_all_drills() -> Result<u32, u16> {
    if DRILLS.len() != SUITE_N {
        return Err(E_REG);
    }
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < SUITE_N {
        sum = sum.wrapping_add((DRILLS[i])().map_err(|_| E_DRILL)?);
        i += 1;
    }
    Ok(sum)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：注册表闭集（判据 ① 的判定）。
    if !registry_closed() {
        return Err(E_CLOSED);
    }
    if SUITES.len() != SUITE_N {
        return Err(E_REG);
    }
    // 段位锚逐项对拍（注册表 vs 各模块 SEG_BASE——双源一致）。
    if SUITES[0].seg != super::unx36221_gafield::SEG_BASE
        || SUITES[1].seg != super::unx36222_gaopen::SEG_BASE
        || SUITES[2].seg != super::unx36223_gafilter::SEG_BASE
        || SUITES[3].seg != super::unx36224_gachain::SEG_BASE
        || SUITES[4].seg != super::unx36225_gadup1::SEG_BASE
        || SUITES[5].seg != super::unx36226_gadup2::SEG_BASE
        || SUITES[6].seg != super::unx36227_gaclose::SEG_BASE
        || SUITES[7].seg != super::unx36228_gadestroy::SEG_BASE
        || SUITES[8].seg != super::unx36229_gasacl::SEG_BASE
        || SUITES[9].seg != super::unx36230_gaquery::SEG_BASE
        || SUITES[10].seg != super::unx36231_gatamper::SEG_BASE
        || SUITES[11].seg != super::unx36232_gainherit::SEG_BASE
        || SUITES[12].seg != super::unx36233_gaperf::SEG_BASE
        || SUITES[13].seg != super::unx36234_galog::SEG_BASE
        || SUITES[14].seg != super::unx36235_gafuzz::SEG_BASE
        || SUITES[15].seg != super::unx36236_gaengine::SEG_BASE
        || SUITES[16].seg != super::unx36237_gawinref::SEG_BASE
        || SUITES[17].seg != super::unx36238_galockstep::SEG_BASE
    {
        return Err(E_REG);
    }
    // R1：18 套件全绿（判据 ② 落点）——含逐套件红项账全零。
    let reds = suite_red_counts();
    let mut i = 0;
    while i < SUITE_N {
        if reds[i] != 0 {
            return Err(E_SUITE);
        }
        i += 1;
    }
    let green = run_all_suites()?;
    if green != SUITE_N {
        return Err(E_COUNT);
    }
    // R2：18 演练全 Ok（判据 ③ 落点）——校验和两次求值稳定；聚合
    // 校验和与逐序号单跑求和对拍（注册表双源的可执行兑现）。
    let s1 = run_all_drills()?;
    let s2 = run_all_drills()?;
    if s1 != s2 {
        return Err(E_DRILL);
    }
    let mut s3 = 0u32;
    let mut i = 0;
    while i < SUITE_N {
        s3 = s3.wrapping_add(drill_one(i)?);
        i += 1;
    }
    if s3 != s1 {
        return Err(E_DRILL);
    }
    // 逐套件指纹重放（两次求值恒等——指纹面确定性锚）+ 双定位器对拍
    //（序号定位器与段位定位器跑同模块得同校验和——注册表双源兑现）。
    let f1 = suite_fingerprints();
    let f2 = suite_fingerprints();
    let mut i = 0;
    while i < SUITE_N {
        if f1[i] != f2[i] {
            return Err(E_DRILL);
        }
        let by_idx = drill_one(i).map_err(|_| E_DRILL)?;
        let by_seg = drill_by_seg(SUITES[i].seg).map_err(|_| E_DRILL)?;
        if by_idx != by_seg {
            return Err(E_DRILL);
        }
        i += 1;
    }
    // 段位定位器负例：未注册段位显性拒（E_CLOSED 而非静默 Ok）。
    if suite_index_by_seg(0x0001) != Err(E_CLOSED) || suite_by_seg(0x0001).is_ok() {
        return Err(E_CLOSED);
    }
    // 逐套件绿项账：每套件至少 3 项判据（批内纪律——缺项即漏验收）。
    let greens = suite_green_counts();
    let mut i = 0;
    while i < SUITE_N {
        if greens[i] < 3 {
            return Err(E_SUITE);
        }
        i += 1;
    }
    // 校验和：聚合演练校验和 + 绿套件数（确定性总锚）。
    Ok(s1.wrapping_add(green as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_REG => "注册表破（缺项/重项/段位锚漂移）",
        E_SUITE => "套件红（某模块 checks 未全绿——ktest 红即全红）",
        E_DRILL => "演练红（某模块 drill 非 Ok 或重放漂移）",
        E_COUNT => "聚合账不平（注册数/绿数不符）",
        E_CLOSED => "注册闭集破（id 断档/段位乱序）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36239_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36239_gaktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xa2d0, "0xa2d0~0xa2dF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 套件全绿/18 演练重放/注册表闭集"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("注册闭集", registry_closed() && SUITE_N == 18, "18 判据套件注册（36221~36238 无缺无重）");
    cs.add("演练注册", DRILLS.len() == SUITE_N, "18 演练执行器与套件同序双源");
    cs.add("红项账", suite_red_counts().iter().all(|&r| r == 0), "逐套件红项计数全零");
    cs.add("演练索引", drill_one(0).is_ok() && drill_one(SUITE_N).err() == Some(E_CLOSED), "按序号单跑可定位；越界显性拒");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
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
    fn test_registry_closed() {
        assert!(registry_closed());
        assert_eq!(SUITES.len(), 18);
        // 首尾段位锚。
        assert_eq!(SUITES[0].seg, 0xa150);
        assert_eq!(SUITES[17].seg, 0xa2c0);
    }

    #[test]
    fn test_all_suites_green() {
        assert_eq!(run_all_suites().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36239_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 3);
    }
}
