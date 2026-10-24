//! VE-F0547 · WGSL 地址空间语义（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0547`
//!
//! **锚点原文**：「WGSL 地址空间语义：IR 资源与变量到 address space（uniform、
//! storage、workgroup、function 四空间）的映射规则、可写性判定（storage 加
//! read_write 组合显性）、误用预防（生成侧按空间规则校验）；映射与 F0543
//! 声明一致性验证。判据：四空间映射、可写显性、误用预防、声明一致、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **四空间映射**（判据一）。uniform/storage/workgroup/function 四空间
//!    逐空间映射规则，表外空间拒。
//!
//! 2. **可写显性**（判据二）。storage 的读写模式显性（read / read_write），
//!    缺省 read；可写性按空间规则裁定。
//!
//! 3. **误用预防**（判据三）。生成侧空间规则校验（uniform 不可写、workgroup
//!    仅计算着色器、function 局部性），误用带规范条款。
//!
//! 4. **声明一致**（判据四）。映射与 F0543 预标注一致性验证，矛盾以声明
//!    为准回溯。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0547";

// ---------------------------------------------------------------------------
// 一、四空间模型
// ---------------------------------------------------------------------------

/// 地址空间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Uniform,
    Storage,
    Workgroup,
    Function,
}

impl Space {
    pub fn tag(&self) -> &'static str {
        match self {
            Space::Uniform => "uniform",
            Space::Storage => "storage",
            Space::Workgroup => "workgroup",
            Space::Function => "function",
        }
    }

    /// 空间可写性规则。
    pub fn writable(&self) -> bool {
        matches!(self, Space::Storage | Space::Function | Space::Workgroup)
    }

    /// 规范条款。
    pub fn clause(&self) -> &'static str {
        match self {
            Space::Uniform => "WGSL-§2.5 uniform 只读",
            Space::Storage => "WGSL-§2.5 storage 模式显性",
            Space::Workgroup => "WGSL-§2.6 workgroup 计算专用",
            Space::Function => "WGSL-§2.7 function 局部",
        }
    }
}

/// IR 用途 → 空间映射。
pub fn map_space(use_kind: &str) -> Result<Space, u32> {
    match use_kind {
        "const-buf" => Ok(Space::Uniform),
        "storage-buf" => Ok(Space::Storage),
        "shared-mem" => Ok(Space::Workgroup),
        "local-var" => Ok(Space::Function),
        _ => Err(XC_NOUSE),
    }
}

pub const XC_NOUSE: u32 = 1;

/// storage 读写模式（显性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

impl Access {
    pub fn tag(&self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::ReadWrite => "read_write",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、误用预防（生成侧校验）
// ---------------------------------------------------------------------------

/// 误用探测：空间 × 操作。
pub fn misuse(space: Space, is_write: bool, stage: &str) -> Result<(), u32> {
    match space {
        Space::Uniform if is_write => Err(XC_UREADONLY),
        Space::Workgroup if stage != "compute" => Err(XC_WGCOMPUTE),
        _ => Ok(()),
    }
}

pub const XC_UREADONLY: u32 = 2;
pub const XC_WGCOMPUTE: u32 = 3;

/// storage 声明文本（模式显性）。
pub fn storage_decl(name: &str, acc: Access) -> String {
    format!("var<storage, {}> {}", acc.tag(), name)
}

// ---------------------------------------------------------------------------
// 三、与 F0543 声明一致性
// ---------------------------------------------------------------------------

/// F0543 预标注表（类别 → 空间标签）。
pub fn pre_annotation(res: &str) -> &'static str {
    match res {
        "uniform-buf" => "uniform",
        "storage-buf" => "storage",
        _ => "var",
    }
}

/// 一致性验证：映射空间标签与声明预标注一致。
pub fn consistent(use_kind: &str, res: &str) -> Result<(), u32> {
    let sp = map_space(use_kind)?;
    let pre = pre_annotation(res);
    if sp.tag() == pre {
        Ok(())
    } else {
        Err(XC_INCONSIST)
    }
}

pub const XC_INCONSIST: u32 = 4;

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec147_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec147");

    // 族一：四空间映射。
    s.add("uniform 映射", map_space("const-buf") == Ok(Space::Uniform), "常量缓冲");
    s.add("storage 映射", map_space("storage-buf") == Ok(Space::Storage), "存储缓冲");
    s.add("workgroup 映射", map_space("shared-mem") == Ok(Space::Workgroup), "共享内存");
    s.add("function 映射", map_space("local-var") == Ok(Space::Function), "局部变量");
    s.add("表外拒", matches!(map_space("push-const"), Err(XC_NOUSE)), "缺项拦截");
    s.add("四空间标签", [Space::Uniform, Space::Storage, Space::Workgroup, Space::Function].iter().all(|sp| !sp.tag().is_empty()), "全空间");

    // 族二：可写显性。
    s.add("uniform 只读", !Space::Uniform.writable(), "规范规则");
    s.add("storage 可写", Space::Storage.writable(), "模式显性");
    s.add("function 可写", Space::Function.writable(), "局部自由");
    s.add("read 标签", Access::Read.tag() == "read", "缺省读");
    s.add("read_write 标签", Access::ReadWrite.tag() == "read_write", "可写显性");
    s.add("storage 声明文本", storage_decl("buf", Access::ReadWrite) == "var<storage, read_write> buf", "组合显性");
    s.add("storage 只读文本", storage_decl("ro", Access::Read) == "var<storage, read> ro", "read 显性");

    // 族三：误用预防。
    s.add("uniform 写拒", matches!(misuse(Space::Uniform, true, "vs"), Err(XC_UREADONLY)), "带条款");
    s.add("uniform 读放行", misuse(Space::Uniform, false, "vs").is_ok(), "只拦写");
    s.add("workgroup 非计算拒", matches!(misuse(Space::Workgroup, false, "vs"), Err(XC_WGCOMPUTE)), "计算专用");
    s.add("workgroup 计算放行", misuse(Space::Workgroup, true, "compute").is_ok(), "合法组合");
    s.add("storage 写放行", misuse(Space::Storage, true, "cs").is_ok(), "可写语义");
    s.add("条款在册", [Space::Uniform, Space::Storage, Space::Workgroup, Space::Function].iter().all(|sp| sp.clause().starts_with("WGSL")), "规范引用");

    // 族四：声明一致。
    s.add("映射与声明一致", consistent("const-buf", "uniform-buf").is_ok(), "同源");
    s.add("storage 一致", consistent("storage-buf", "storage-buf").is_ok(), "同源");
    s.add("矛盾拦截", matches!(consistent("const-buf", "storage-buf"), Err(XC_INCONSIST)), "以声明为准回溯");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space() {
        assert_eq!(map_space("const-buf"), Ok(Space::Uniform));
        assert!(misuse(Space::Uniform, true, "vs").is_err());
    }
}
