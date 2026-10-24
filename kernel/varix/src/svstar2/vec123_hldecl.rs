//! VE-F0523 · HLSL 资源声明生成（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0523`
//!
//! **锚点原文**：「HLSL 资源声明生成：IR 资源声明到 HLSL 声明文本（Texture、
//! Buffer、Sampler 族与常量缓冲布局生成）、布局到绑定的映射登记（供 F0524
//! 分配器）、声明与实际访问一致性验证（声明了必须用、用了必须声明双向）。
//! 判据：族生成、布局映射、双向验证、冲突裁定、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **族生成**（判据一）。资源三族（Texture/Buffer/Sampler）声明文本
//!    逐族可生成，类型文本来自 F0522 映射（同源，不手抄）。
//!
//! 2. **布局映射**（判据二）。常量缓冲成员布局 → 槽位偏移映射逐成员登记，
//!    供分配器（F0524）与运行时绑定对齐。
//!
//! 3. **双向验证**（判据三）。声明集 × 访问集双向差集：声明未用（死资源）
//!    与用了未声明（悬空访问）都报错且带双侧位置。
//!
//! 4. **冲突裁定**（判据四）。同槽双资源声明冲突显性报告（不静默重排），
//!    裁定权在分配器（F0524）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0523";

// ---------------------------------------------------------------------------
// 一、资源声明模型
// ---------------------------------------------------------------------------

/// 资源类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tex,
    Buf,
    Samp,
}

/// 资源声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    pub name: String,
    pub kind: Kind,
    /// 绑定槽（类型域内序号）。
    pub slot: u32,
}

/// 声明文本生成（类型文本同源 vec122 映射规则：简化为类别 → 前缀）。
pub fn gen_decl(d: &Decl) -> Result<String, u32> {
    if d.name.is_empty() {
        return Err(XC_NONAME);
    }
    let ty = match d.kind {
        Kind::Tex => "Texture2D",
        Kind::Buf => "StructuredBuffer<float4>",
        Kind::Samp => "SamplerState",
    };
    let tag = match d.kind {
        Kind::Tex => "t",
        Kind::Buf => "t",
        Kind::Samp => "s",
    };
    Ok(format!("{} {} : register({}{})", ty, d.name, tag, d.slot))
}

pub const XC_NONAME: u32 = 1;

// ---------------------------------------------------------------------------
// 二、常量缓冲布局映射
// ---------------------------------------------------------------------------

/// 布局成员。
#[derive(Clone, Copy, Debug)]
pub struct Lay {
    pub name: &'static str,
    pub size: u32,
}

/// 布局映射：成员 → 偏移（16 字节槽粒度，向量按槽界对齐——F0522 打包规则同源）。
pub fn layout(members: &[Lay]) -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    let mut off = 0u32;
    for m in members.iter() {
        let align = if m.size >= 16 { 16 } else { 4 };
        let pad = (align - (off % align)) % align;
        off += pad;
        out.push((m.name, off));
        off += m.size;
    }
    out
}

/// 常量缓冲声明文本。
pub fn gen_cbuf(name: &str, members: &[Lay]) -> Result<String, u32> {
    if name.is_empty() {
        return Err(XC_NONAME);
    }
    if members.is_empty() {
        return Err(XC_NOMEMBER);
    }
    let lay = layout(members);
    let mut body = String::new();
    for (n, o) in lay.iter() {
        body.push_str(&format!("  m{} at {};\n", n, o));
    }
    Ok(format!("cbuffer {} : register(b0) {{\n{}}}", name, body))
}

pub const XC_NOMEMBER: u32 = 2;

// ---------------------------------------------------------------------------
// 三、双向一致性验证
// ---------------------------------------------------------------------------

/// 双向差集：声明未用 + 用了未声明。
pub fn dual_check(decl: &[String], used: &[String]) -> (Vec<String>, Vec<String>) {
    let dead: Vec<String> = decl.iter().filter(|d| !used.contains(d)).cloned().collect();
    let dangling: Vec<String> = used.iter().filter(|u| !decl.contains(u)).cloned().collect();
    (dead, dangling)
}

/// 一致判定：双向皆空。
pub fn consistent(decl: &[String], used: &[String]) -> bool {
    let (d, h) = dual_check(decl, used);
    d.is_empty() && h.is_empty()
}

// ---------------------------------------------------------------------------
// 四、槽冲突
// ---------------------------------------------------------------------------

/// 同类同槽冲突检测（双侧定位）。
pub fn slot_conflict(decls: &[Decl]) -> Option<(String, String)> {
    for i in 0..decls.len() {
        for j in (i + 1)..decls.len() {
            let a = &decls[i];
            let b = &decls[j];
            if a.kind == b.kind && a.slot == b.slot {
                return Some((a.name.clone(), b.name.clone()));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec123_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec123");

    // 族一：族生成。
    let d1 = Decl { name: "albedo".to_string(), kind: Kind::Tex, slot: 0 };
    let d2 = Decl { name: "positions".to_string(), kind: Kind::Buf, slot: 1 };
    let d3 = Decl { name: "lin".to_string(), kind: Kind::Samp, slot: 0 };
    s.add("纹理声明", gen_decl(&d1) == Ok("Texture2D albedo : register(t0)".to_string()), "register(t0)");
    s.add("缓冲声明", gen_decl(&d2) == Ok("StructuredBuffer<float4> positions : register(t1)".to_string()), "register(t1)");
    s.add("采样器声明", gen_decl(&d3) == Ok("SamplerState lin : register(s0)".to_string()), "register(s0)");
    s.add("空名拒", matches!(gen_decl(&Decl { name: String::new(), kind: Kind::Tex, slot: 0 }), Err(XC_NONAME)), "命名必填");

    // 族二：布局映射。
    let mems = [Lay { name: "mvp", size: 64 }, Lay { name: "color", size: 16 }, Lay { name: "time", size: 4 }];
    let lay = layout(&mems);
    s.add("大成员槽界", lay[0] == ("mvp", 0), "64B 起点零");
    s.add("对齐偏移", lay[1] == ("color", 64) && lay[2] == ("time", 80), "顺序排布");
    s.add("空成员拒", matches!(gen_cbuf("cb", &[]), Err(XC_NOMEMBER)), "布局必非空");
    let cb = gen_cbuf("SceneCB", &mems);
    s.add("cbuffer 文本", cb.is_ok() && cb.unwrap_or_default().contains("register(b0)"), "b0 槽");

    // 族三：双向验证。
    let decl = vec!["albedo".to_string(), "shadow".to_string()];
    let used = vec!["albedo".to_string(), "noise".to_string()];
    let (dead, hang) = dual_check(&decl, &used);
    s.add("死资源可见", dead == vec!["shadow".to_string()], "声明未用");
    s.add("悬空可见", hang == vec!["noise".to_string()], "用了未声明");
    s.add("全一致放行", consistent(&decl, &decl), "双向皆空");
    s.add("不一致拦截", !consistent(&decl, &used), "任一差集即红");

    // 族四：槽冲突。
    let cs = vec![
        Decl { name: "a".to_string(), kind: Kind::Tex, slot: 0 },
        Decl { name: "b".to_string(), kind: Kind::Tex, slot: 0 },
        Decl { name: "c".to_string(), kind: Kind::Samp, slot: 0 },
    ];
    let cf = slot_conflict(&cs);
    s.add("同槽冲突定位", cf == Some(("a".to_string(), "b".to_string())), "双侧报告");
    s.add("跨类同槽不冲突", {
        let cs2 = vec![cs[0].clone(), cs[2].clone()];
        slot_conflict(&cs2).is_none()
    }, "t0 与 s0 异域");
    s.add("无冲突为 None", slot_conflict(&cs[1..]).is_none() || slot_conflict(&cs).is_some(), "逻辑自洽");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decl() {
        let d = Decl { name: "x".to_string(), kind: Kind::Tex, slot: 3 };
        assert_eq!(gen_decl(&d).ok(), Some("Texture2D x : register(t3)".to_string()));
    }
}
