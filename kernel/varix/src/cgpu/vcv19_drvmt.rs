//! CGPU-F3379 · 驱动版本矩阵（vcv19_drvmt）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3379`
//!
//! **判据（锚点原文）**：代次、一组、判据。
//!
//! **职责定位（锚点原文）**：驱动：驱动版本矩阵（驱动代次覆盖——
//! 驱动实现；测试（驱动一组）。
//!
//! # 一、代次模型（版本 ≠ 代次）
//!
//! 版本号是连续流（1.0.3、1.0.4……），代次是**验收关心的粗粒度**：
//! 同代次内的小版本差异不构成验收结论的作用域差别，跨代次则构成。
//! 本模块钉死四代次（与 vcv03 驱动轴同轴）：当前（[`Gen::Current`]）、
//! 上一代（[`Gen::Prev`]）、仍支持老代次（[`Gen::Legacy`]）、beta 通
//! 道（[`Gen::Beta`]）。代次覆盖判据：验收矩阵里每个**已取证**的
//! 驱动坐标必须能映射到某个代次；无代次映射的版本号不允许出现在
//! 验收记录里（[`E_VV_NOGEN`]）。
//!
//! # 二、版本→代次归类
//!
//! [`GenBook`] 维护版本号前缀→代次的归类表：归类规则是**最长前缀
//! 优先**（"1.2" 归当前时 "1.2.9" 也归当前），归类冲突（同前缀两
//! 代次）即拒。beta 通道特殊：beta 版本永不替代同号正式版——验
//! 收结论只对「beta 通道」成立，不外溢到正式通道（隔离纪律）。
//!
//! # 三、驱动一组（判据）
//!
//! 判据组：四代次闭集 + 归类最长前缀 + 前缀冲突拒收 + beta 隔离
//! + 覆盖清点（每代次在册数）。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DRVMATRIX_VERSION: &str = "V19-drvmatrix-v1";

/// 版本号无代次归类（不允许出现在验收记录里）。
pub const E_VV_NOGEN: &str = "E_VV_NOGEN";

/// 同前缀两代次（归类冲突）。
pub const E_VV_GENCLASH: &str = "E_VV_GENCLASH";

/// 驱动代次四代（与 vcv03 驱动轴同轴：0=当前 1=上一代 2=老代次 3=beta）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gen {
    /// 当前代（0）。
    Current,
    /// 上一代（1）。
    Prev,
    /// 仍支持老代次（2）。
    Legacy,
    /// beta 通道（3，隔离不外溢）。
    Beta,
}

impl Gen {
    /// 四代闭集。
    pub const ALL: [Gen; 4] = [Gen::Current, Gen::Prev, Gen::Legacy, Gen::Beta];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Gen::Current => "当前代",
            Gen::Prev => "上一代",
            Gen::Legacy => "老代次",
            Gen::Beta => "beta 通道",
        }
    }

    /// 对应 vcv03 驱动轴取值号。
    pub const fn axis(self) -> u8 {
        match self {
            Gen::Current => 0,
            Gen::Prev => 1,
            Gen::Legacy => 2,
            Gen::Beta => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、归类表
// ---------------------------------------------------------------------------

/// 归类条目（版本前缀 → 代次）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenRule {
    /// 版本号前缀（如 "1.2"）。
    pub prefix: String,
    /// 归属代次。
    pub gen: Gen,
}

/// 驱动代次归类表。
pub struct GenBook {
    rules: Vec<GenRule>,
}

impl GenBook {
    /// 空表。
    pub fn new() -> GenBook {
        GenBook { rules: Vec::new() }
    }

    /// 加规则：同前缀两代次即拒（归类冲突）。
    pub fn add(&mut self, prefix: &str, gen: Gen) -> Result<(), String> {
        if self.rules.iter().any(|r| r.prefix == prefix && r.gen != gen) {
            return Err(format!(
                "{}：前缀 {} 已归 {}——同前缀不许跨代次",
                E_VV_GENCLASH, prefix, gen.zh()
            ));
        }
        if let Some(pos) = self.rules.iter().position(|r| r.prefix == prefix) {
            self.rules[pos].gen = gen;
        } else {
            self.rules.push(GenRule { prefix: prefix.to_string(), gen });
        }
        Ok(())
    }

    /// 归类：最长前缀优先；无命中即拒（E_VV_NOGEN）。
    pub fn classify(&self, version: &str) -> Result<Gen, String> {
        let mut best: Option<&GenRule> = None;
        for r in self.rules.iter() {
            if version.starts_with(&r.prefix) {
                match best {
                    None => best = Some(r),
                    Some(b) if r.prefix.len() > b.prefix.len() => best = Some(r),
                    _ => {}
                }
            }
        }
        match best {
            Some(r) => Ok(r.gen),
            None => Err(format!(
                "{}：版本 {} 无代次归类——不许进验收记录",
                E_VV_NOGEN, version
            )),
        }
    }

    /// 规则数。
    pub fn len(&self) -> usize {
        self.rules.len()
    }
}

/// beta 隔离核验：beta 归类结论不外溢（验收记录必须带通道标注）。
pub fn beta_isolated(version: &str, book: &GenBook) -> Result<bool, String> {
    let g = book.classify(version)?;
    Ok(matches!(g, Gen::Beta))
}

/// 覆盖清点：按代次清点版本在册数（覆盖率分母口径）。
pub fn count_gen(book: &GenBook, versions: &[&str], g: Gen) -> usize {
    versions
        .iter()
        .filter(|v| book.classify(v).map(|x| x == g).unwrap_or(false))
        .count()
}

/// 驱动矩阵声明（非空即判据）。
pub const DRVMATRIX_DECLARATION: &str =
    "驱动四代次：当前/上一代/老代次/beta，版本归类最长前缀优先，beta 隔离不外溢";

// ---------------------------------------------------------------------------
// 三、驱动一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3379 域自检：代次、一组、判据。
pub fn run_vcv19_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv19_drvmatrix");

    // --- 四代闭集 + 轴号互异 ---
    let axes: Vec<u8> = Gen::ALL.iter().map(|g| g.axis()).collect();
    let mut distinct = true;
    for i in 0..axes.len() {
        for j in i + 1..axes.len() {
            if axes[i] == axes[j] {
                distinct = false;
            }
        }
    }
    cs.add(
        "四代闭集轴号互异",
        Gen::ALL.len() == 4 && distinct && axes == vec![0, 1, 2, 3],
        "接 vcv03 驱动轴 0..4",
    );

    // --- 归类：最长前缀优先 ---
    let mut bk = GenBook::new();
    bk.add("1", Gen::Prev).expect("rule1");
    bk.add("1.2", Gen::Current).expect("rule2");
    bk.add("2.0b", Gen::Beta).expect("rule3");
    cs.add(
        "最长前缀归类",
        bk.classify("1.0.3") == Ok(Gen::Prev)
            && bk.classify("1.2") == Ok(Gen::Current)
            && bk.classify("1.2.9") == Ok(Gen::Current)
            && bk.classify("1.9.1") == Ok(Gen::Prev),
        "1.2.x 归当前，其余 1.x 归上一代",
    );

    // --- 无归类拒收 ---
    let r = bk.classify("3.0.0");
    cs.add(
        "无归类版本拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_NOGEN),
        "没有代次的版本不许进验收记录",
    );

    // --- 前缀冲突拒收 ---
    let mut bk2 = GenBook::new();
    bk2.add("1.2", Gen::Current).expect("first");
    let r = bk2.add("1.2", Gen::Beta);
    cs.add(
        "同前缀跨代次拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_GENCLASH),
        "归类冲突即拒",
    );

    // --- beta 隔离 ---
    cs.add(
        "beta 归类可判",
        beta_isolated("2.0b1", &bk) == Ok(true) && beta_isolated("1.2.0", &bk) == Ok(false),
        "beta 通道标注不外溢正式通道",
    );

    // --- 覆盖清点 ---
    let vers = ["1.0.3", "1.2.0", "1.2.4", "2.0b1", "3.0.0"];
    cs.add(
        "按代次清点",
        count_gen(&bk, &vers, Gen::Current) == 2
            && count_gen(&bk, &vers, Gen::Prev) == 1
            && count_gen(&bk, &vers, Gen::Beta) == 1
            && count_gen(&bk, &vers, Gen::Legacy) == 0,
        "3.0.0 无归类不计入",
    );

    // --- 声明非空 ---
    cs.add(
        "驱动声明非空",
        !DRVMATRIX_DECLARATION.is_empty() && DRVMATRIX_DECLARATION.contains("beta"),
        "锚点代次声明",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in DRVMATRIX_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, DRVMATRIX_VERSION);

    cs
}
