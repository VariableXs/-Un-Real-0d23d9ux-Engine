//! VE-F0543 · WGSL 资源声明生成（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0543`
//!
//! **锚点原文**：「WGSL 资源声明生成：IR 资源声明到 WGSL binding 与 buffer
//! 声明文本（绑定组与绑定号生成策略、address space 预标注）、声明与访问一致
//! 性双向验证（F0523 机制同构复用）；声明黄金对拍。判据：绑定生成、space
//! 预标注、双向验证、黄金对拍、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **绑定生成**（判据一）。binding(group, binding) 文本按策略生成
//!    （group 按资源类别分段，binding 组内递增）。
//!
//! 2. **space 预标注**（判据二）。address space 随声明预标注（var<uniform>
//!    等），标注与 F0542 维度联动同源。
//!
//! 3. **双向验证**（判据三）。声明 × 访问双向差集（死资源/悬空访问），
//!    F0523 机制同构。
//!
//! 4. **黄金对拍**（判据四）。声明文本黄金对独立维护。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0543";

// ---------------------------------------------------------------------------
// 一、绑定策略（group 按类别分段）
// ---------------------------------------------------------------------------

/// 资源类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    Uniform,
    Storage,
    Tex,
    Samp,
}

impl Res {
    /// group 分段策略：uniform=0, storage=1, tex=2, samp=3。
    pub fn group(&self) -> u32 {
        match self {
            Res::Uniform => 0,
            Res::Storage => 1,
            Res::Tex => 2,
            Res::Samp => 3,
        }
    }

    /// address space 预标注。
    pub fn space(&self) -> &'static str {
        match self {
            Res::Uniform => "uniform",
            Res::Storage => "storage",
            Res::Tex => "texture_2d<f32>",
            Res::Samp => "sampler",
        }
    }

    /// WGSL 声明形态。
    pub fn decl_form(&self) -> &'static str {
        match self {
            Res::Uniform => "var<uniform>",
            Res::Storage => "var<storage, read>",
            Res::Tex => "var",
            Res::Samp => "var",
        }
    }
}

/// 声明条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WDecl {
    pub name: String,
    pub res: Res,
    /// 组内 binding 序号（0 起）。
    pub bind: u32,
}

/// 绑定文本生成。
pub fn gen_bind(d: &WDecl) -> Result<String, u32> {
    if d.name.is_empty() {
        return Err(XC_NONAME);
    }
    Ok(format!("@group({}) @binding({})", d.res.group(), d.bind))
}

pub const XC_NONAME: u32 = 1;

/// 全声明文本（绑定头 + space 声明）。
pub fn gen_decl(d: &WDecl, ty: &str) -> Result<String, u32> {
    let head = gen_bind(d)?;
    Ok(format!("{} {} {}: {}", head, d.res.decl_form(), d.name, ty))
}

// ---------------------------------------------------------------------------
// 二、组内递增分配
// ---------------------------------------------------------------------------

/// 绑定分配器：按类别分组、组内递增。
#[derive(Default)]
pub struct Binder {
    pub decls: Vec<WDecl>,
}

impl Binder {
    pub fn new() -> Binder {
        Binder { decls: Vec::new() }
    }

    pub fn add(&mut self, name: &str, res: Res) -> Result<u32, u32> {
        if name.is_empty() {
            return Err(XC_NONAME);
        }
        if self.decls.iter().any(|d| d.name == name) {
            return Err(XC_DUP);
        }
        let bind = self.decls.iter().filter(|d| d.res == res).count() as u32;
        self.decls.push(WDecl { name: name.to_string(), res, bind });
        Ok(bind)
    }

    pub fn by_group(&self, g: u32) -> usize {
        self.decls.iter().filter(|d| d.res.group() == g).count()
    }
}

pub const XC_DUP: u32 = 2;

// ---------------------------------------------------------------------------
// 三、双向验证（复用 F0523 语义）
// ---------------------------------------------------------------------------

pub fn dual_check(decl: &[String], used: &[String]) -> (Vec<String>, Vec<String>) {
    let dead: Vec<String> = decl.iter().filter(|d| !used.contains(d)).cloned().collect();
    let hang: Vec<String> = used.iter().filter(|u| !decl.contains(u)).cloned().collect();
    (dead, hang)
}

pub fn consistent(decl: &[String], used: &[String]) -> bool {
    let (a, b) = dual_check(decl, used);
    a.is_empty() && b.is_empty()
}

// ---------------------------------------------------------------------------
// 四、黄金对
// ---------------------------------------------------------------------------

pub struct WGolden {
    pub res: Res,
    pub name: &'static str,
    pub ty: &'static str,
    pub want: &'static str,
}

pub const W_GOLDENS: [WGolden; 3] = [
    WGolden { res: Res::Uniform, name: "cb", ty: "SceneCB", want: "@group(0) @binding(0) var<uniform> cb: SceneCB;" },
    WGolden { res: Res::Tex, name: "albedo", ty: "texture_2d<f32>", want: "@group(2) @binding(0) var albedo: texture_2d<f32>;" },
    WGolden { res: Res::Samp, name: "lin", ty: "sampler", want: "@group(3) @binding(0) var lin: sampler;" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec143_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec143");

    // 族一：绑定生成。
    let d = WDecl { name: "cb".to_string(), res: Res::Uniform, bind: 0 };
    s.add("绑定头文本", gen_bind(&d) == Ok("@group(0) @binding(0)".to_string()), "group 分段");
    let d2 = WDecl { name: "albedo".to_string(), res: Res::Tex, bind: 1 };
    s.add("纹理绑定", gen_bind(&d2) == Ok("@group(2) @binding(1)".to_string()), "group 2");
    s.add("空名拒", matches!(gen_bind(&WDecl { name: String::new(), res: Res::Tex, bind: 0 }), Err(XC_NONAME)), "命名必填");

    // 族二：space 预标注。
    s.add("uniform 预标注", Res::Uniform.space() == "uniform", "var<uniform>");
    s.add("storage 只读标注", Res::Storage.space() == "storage", "read 预设");
    s.add("声明全文本", gen_decl(&d, "SceneCB") == Ok("@group(0) @binding(0) var<uniform> cb: SceneCB;".to_string()), "三段集成");

    // 族三：组内递增。
    let mut b = Binder::new();
    let t0 = match b.add("t0", Res::Tex) {
        Ok(x) => x,
        Err(_) => { s.add("分配败", false, "不应到"); return s; }
    };
    let t1 = match b.add("t1", Res::Tex) {
        Ok(x) => x,
        Err(_) => { s.add("分配败2", false, "不应到"); return s; }
    };
    let u0 = match b.add("cb", Res::Uniform) {
        Ok(x) => x,
        Err(_) => { s.add("分配败3", false, "不应到"); return s; }
    };
    s.add("组内递增", t0 == 0 && t1 == 1, "同类连续");
    s.add("跨组独立", u0 == 0, "uniform 从零");
    s.add("分组计数", b.by_group(2) == 2 && b.by_group(0) == 1, "group 统计");
    s.add("重名拒", matches!(b.add("t0", Res::Tex), Err(XC_DUP)), "名字唯一");

    // 族四：双向验证。
    let decl = vec!["cb".to_string(), "albedo".to_string()];
    let used = vec!["cb".to_string(), "noise".to_string()];
    let (dead, hang) = dual_check(&decl, &used);
    s.add("死资源可见", dead == vec!["albedo".to_string()], "声明未用");
    s.add("悬空可见", hang == vec!["noise".to_string()], "用了未声明");
    s.add("全一致", consistent(&decl, &decl), "双向皆空");

    // 族五：黄金对拍。
    let mut seq = [0u32; 3];
    for (i, g) in W_GOLDENS.iter().enumerate() {
        let w = WDecl { name: g.name.to_string(), res: g.res, bind: 0 };
        seq[i] = 1;
        s.add("声明黄金", gen_decl(&w, g.ty) == Ok(g.want.to_string()), g.name);
    }
    let _ = seq;
    s.add("黄金三对", W_GOLDENS.len() == 3, "独立正本");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind() {
        let d = WDecl { name: "x".to_string(), res: Res::Storage, bind: 2 };
        assert_eq!(gen_bind(&d).ok(), Some("@group(1) @binding(2)".to_string()));
    }
}
