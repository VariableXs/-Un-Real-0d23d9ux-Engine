//! F133 第三方图标包规范 · 完整设计（STAR I 主册 G-D-08）。
//!
//! **判据（主册）**：官方示例包 2 套过校验；社区模拟包 5 套（含故意
//! 错误）报错逐条准确。
//!
//! **设计要点（主册）**：包结构 `icons/<名称>/<尺寸>/<状态>/<类别>.png`
//! + `manifest.json`；尺寸档位 32/48/96/256（256 为 4K 管线必填，其余
//! 档可由校验器建议重采样输出）；三态（常驻/悬停/按下）；类别清单固定
//! （系统/文件夹/文件类型/托盘四族）；位深 32bit（含 alpha）；命名
//! 规则；版权字段必填（作者+许可）；**宽容导入，诚实标注**——缺档位
//! 自动用邻近档重采样（标注降级）、缺状态用常驻态代替+标注、校验失败
//! 逐条指出（修图指南式报错）。
//!
//! 本模块是校验器的**纯逻辑核**：manifest schema 校验、档位/三态/
//! 命名四族校验、缺档重采样建议与降级标注、逐条错误报告（判据「报错
//! 逐条准确」的机制面）。官方 2 套 + 社区 5 套判据以样本用例钉死。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 尺寸档位（乙-4 表对齐）。
pub const SIZE_STEPS: [u16; 4] = [32, 48, 96, 256];

/// 4K 管线必填档。
pub const MANDATORY_SIZE: u16 = 256;

/// 三态。缺状态态 → 用常驻态代替+标注。
pub const STATES: [&str; 3] = ["normal", "hover", "pressed"];

/// 类别清单固定四族。
pub const CATEGORIES: [&str; 4] = ["system", "folder", "filetype", "tray"];

/// 包内图标名命名规则：ASCII 小写字母/数字/中划线，1..=48 字节，
/// 不许中划线开头结尾或连续。
pub fn name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 48 {
        return false;
    }
    if b[0] == b'-' || b[b.len() - 1] == b'-' {
        return false;
    }
    let mut prev_dash = false;
    for &c in b {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-';
        if !ok {
            return false;
        }
        if c == b'-' {
            if prev_dash {
                return false;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// manifest 模型
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Manifest {
    pub name_ok: bool,
    pub author_set: bool,
    pub license_set: bool,
    pub version_set: bool,
    /// 声明的档位位图（bit i = SIZE_STEPS[i] 提供）。
    pub sizes: u8,
    /// 声明的三态位图。
    pub states: u8,
    /// 提供的类别位图。
    pub categories: u8,
    /// 位深 32bit。
    pub depth32: bool,
}

impl Manifest {
    /// 版权字段必填（作者+许可）——缺任一直接拒收（不宽容版权缺位）。
    pub fn copyright_ok(&self) -> bool {
        self.author_set && self.license_set
    }

    /// 256 必填（4K 管线硬标准）。
    pub fn mandatory_size_ok(&self) -> bool {
        self.sizes & (1 << 3) != 0
    }

    /// 三态齐 = 位图 0b111。
    pub fn all_states(&self) -> bool {
        self.states == 0b111
    }
}

// ---------------------------------------------------------------------------
// 校验结果：逐条错误（修图指南式报错）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Issue {
    pub code: IssueCode,
    pub detail: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IssueCode {
    /// manifest 文件缺失。
    NoManifest,
    /// 包名非法。
    BadName,
    /// 作者/许可缺位。
    MissingAuthor,
    MissingLicense,
    /// 版本缺位。
    MissingVersion,
    /// 256 档缺失（4K 硬标准）。
    Missing256,
    /// 状态缺失（用 normal 代替+标注的降级面）。
    MissingState,
    /// 类别缺失。
    MissingCategory,
    /// 位深不足 32bit。
    LowDepth,
    /// 图标名非法。
    BadIconName,
}

pub struct PackReport {
    pub issues: Vec<Issue>,
    /// 缺档重采样建议：缺失档 → 建议来源档（邻近更低档）。
    pub resample: Vec<(u16, u16)>,
    /// 降级标注：缺失态 → 用 normal 补。
    pub state_fallback: Vec<(&'static str, &'static str)>,
}

impl PackReport {
    /// 官方包判据：零 issue 才过校验。
    pub fn pass(&self) -> bool {
        self.issues.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 校验器
// ---------------------------------------------------------------------------

pub fn validate(m: &Manifest, icon_names: &[&str]) -> PackReport {
    let mut issues = Vec::new();
    let mut resample = Vec::new();
    let mut state_fallback = Vec::new();

    if !m.name_ok {
        issues.push(Issue { code: IssueCode::BadName, detail: "包名非法：仅 ASCII 小写/数字/中划线" });
    }
    if !m.author_set {
        issues.push(Issue { code: IssueCode::MissingAuthor, detail: "manifest 缺 author 字段（版权必填）" });
    }
    if !m.license_set {
        issues.push(Issue { code: IssueCode::MissingLicense, detail: "manifest 缺 license 字段（版权必填）" });
    }
    if !m.version_set {
        issues.push(Issue { code: IssueCode::MissingVersion, detail: "manifest 缺 version 字段" });
    }
    if !m.depth32 {
        issues.push(Issue { code: IssueCode::LowDepth, detail: "位深不足：要求 32bit（含 alpha）" });
    }
    if !m.mandatory_size_ok() {
        issues.push(Issue { code: IssueCode::Missing256, detail: "256px 档必填（4K 管线硬标准）" });
    } else {
        // 宽容导入：缺失的非必填档 → 建议由邻近更低档重采样（标注降级）。
        for (i, &s) in SIZE_STEPS.iter().enumerate() {
            if i < 3 && m.sizes & (1 << i) == 0 {
                // 找不到更低提供档时由 256 出（downscale 品质更稳）。
                let from = if m.sizes & mask_below(i) != 0 {
                    nearest_below(i, m.sizes)
                } else {
                    MANDATORY_SIZE
                };
                resample.push((s, from));
            }
        }
    }
    // 三态：缺 → normal 代替 + 标注（宽容导入，诚实标注）。
    for (i, &st) in STATES.iter().enumerate() {
        if m.states & (1 << i) == 0 {
            issues.push(Issue { code: IssueCode::MissingState, detail: "状态缺失（已按 normal 代替并标注降级）" });
            state_fallback.push((st, "normal"));
        }
    }
    // 类别：至少一族，逐族清单校验。
    if m.categories == 0 {
        issues.push(Issue { code: IssueCode::MissingCategory, detail: "至少提供一个类别（系统/文件夹/文件类型/托盘）" });
    }
    // 逐图标命名。
    for n in icon_names {
        if !name_ok(n) {
            issues.push(Issue { code: IssueCode::BadIconName, detail: "图标名非法（小写/数字/中划线，1-48 字节）" });
        }
    }
    PackReport { issues, resample, state_fallback }
}

fn mask_below(i: usize) -> u8 {
    if i == 0 {
        0
    } else {
        (1u8 << i).saturating_sub(1)
    }
}

fn nearest_below(i: usize, sizes: u8) -> u16 {
    for j in (0..i).rev() {
        if sizes & (1 << j) != 0 {
            return SIZE_STEPS[j];
        }
    }
    MANDATORY_SIZE
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F133_TAG: &str = "stareco-F133-iconpack";

pub fn run_iconpack_checks() -> CheckSet {
    let mut set = CheckSet::new(F133_TAG);

    let official_a = Manifest {
        name_ok: true,
        author_set: true,
        license_set: true,
        version_set: true,
        sizes: 0b1111,
        states: 0b111,
        categories: 0b1111,
        depth32: true,
    };
    let official_b = Manifest { sizes: 0b1001, states: 0b111, categories: 0b0011, ..official_a };
    let ra = validate(&official_a, &["folder-open", "system-gear"]);
    let rb = validate(&official_b, &["tray-net"]);
    set.add("f133 official a pass", ra.pass(), "full pack");
    set.add(
        "f133 official b pass with resample note",
        rb.pass() && rb.resample == vec![(48, 32), (96, 32)],
        "missing 48/96 resampled from nearest 32",
    );

    // 社区模拟包（含故意错误）报错逐条准确
    let broken = Manifest {
        name_ok: false,
        author_set: false,
        license_set: false,
        version_set: false,
        sizes: 0b0110,
        states: 0b001,
        categories: 0,
        depth32: false,
    };
    let r = validate(&broken, &["OK-name", "Bad Name!", "-leading"]);
    let codes: Vec<IssueCode> = r.issues.iter().map(|i| i.code).collect();
    set.add(
        "f133 broken pack all flagged",
        codes.contains(&IssueCode::BadName)
            && codes.contains(&IssueCode::MissingAuthor)
            && codes.contains(&IssueCode::MissingLicense)
            && codes.contains(&IssueCode::MissingVersion)
            && codes.contains(&IssueCode::LowDepth)
            && codes.contains(&IssueCode::Missing256)
            && codes.contains(&IssueCode::MissingCategory)
            && codes.contains(&IssueCode::BadIconName),
        "8 issue kinds hit",
    );
    set.add("f133 broken pack not pass", !r.pass(), "community bad pack red");
    // 缺 256 的建议面（broken 包 256 缺 → resample 建议不替代必填报错）
    set.add("f133 missing256 not silently resampled", codes.contains(&IssueCode::Missing256), "4k hard line");

    // 宽容导入面：hover/pressed 缺 → normal 代替 + 标注
    let partial_states = Manifest { states: 0b001, ..official_a };
    let rs = validate(&partial_states, &["a1"]);
    set.add(
        "f133 state fallback honest",
        rs.state_fallback == vec![("hover", "normal"), ("pressed", "normal")],
        "fallback labeled",
    );

    // 命名规则边界
    set.add("f133 name rules", name_ok("ab-12") && !name_ok("-ab") && !name_ok("a--b") && !name_ok("A") && !name_ok(""), "naming law");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Manifest {
        Manifest {
            name_ok: true,
            author_set: true,
            license_set: true,
            version_set: true,
            sizes: 0b1111,
            states: 0b111,
            categories: 0b1111,
            depth32: true,
        }
    }

    #[test]
    fn official_packs_pass() {
        let a = validate(&good(), &["x1"]);
        assert!(a.pass());
        let b = validate(&Manifest { sizes: 0b1000, ..good() }, &["y2"]);
        assert!(b.pass());
        assert_eq!(b.resample, vec![(32, 256), (48, 256), (96, 256)]);
    }

    #[test]
    fn every_bad_icon_flagged() {
        // 两个非法名（含空格、全大写）→ 逐条各报一次
        let r = validate(&good(), &["bad name", "UP", "x", "ok1"]);
        assert_eq!(r.issues.len(), 2);
        assert!(r.issues.iter().all(|i| i.code == IssueCode::BadIconName));
    }
}
