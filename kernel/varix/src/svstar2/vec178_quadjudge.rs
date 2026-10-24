//! VE-F0578 · 四后端总对拍
//!
//! F0557 框架的预留槽位兑现——SPIR-V 第四路适配器接入：四层比对在二进制产物的
//! 适配实现（词法层=ID 与名称映射经反汇编回溯、结构层=反汇编快照控制流同构、
//! 数值层=常量序列一致、声明层=接口反射等价 F0568）；四路差异汇总矩阵与发布
//! 门槛（四路全绿或白名单放行）；分工显性：F0506/F0529/F0548/F0569 是各后端
//! 纵深对拍，本条是跨后端横宽汇总，仲裁结论回写差异表 F0558 的 SPIR-V 列。
//!
//! 跨批对接：上游 F0557 框架、F0568 反射、F0569 反汇编；下游 F0580 组收口、发布管线。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 四路后端
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Road {
    Glsl,
    Hlsl,
    Wgsl,
    Spirv,
}

/// 四层比对
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpLayer {
    /// 词法：ID 与名称映射经反汇编回溯
    Lexical,
    /// 结构：反汇编快照控制流同构
    Structural,
    /// 数值：常量序列一致
    Numeric,
    /// 声明：接口反射等价（F0568）
    Declaration,
}

/// SPIR-V 第四路适配器（兑现 F0557 预留槽位）
#[derive(Clone, Debug)]
pub struct SpirvAdapter {
    /// 反汇编回溯表：二进制 ID → 规范名
    pub id_names: Vec<(u32, String)>,
    /// 反汇编控制流指纹
    pub cf_fp: u64,
    /// 常量序列
    pub consts: Vec<i64>,
    /// 接口反射等价键（F0568）
    pub iface_key: String,
}

impl SpirvAdapter {
    /// 词法层：ID → 名称回溯
    pub fn lex_backtrace(&self, id: u32) -> Option<&str> {
        self.id_names.iter().find(|(i, _)| *i == id).map(|(_, n)| n.as_str())
    }

    pub fn fp(&self) -> u64 {
        self.cf_fp
    }
}

/// 单层判定
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub layer: CmpLayer,
    pub road: Road,
    pub same: bool,
}

/// 四层比对（第四路对任意文本路参考实现）
pub fn compare_road4(
    spirv: &SpirvAdapter,
    ref_names: &[&str],
    ref_fp: u64,
    ref_consts: &[i64],
    ref_iface: &str,
) -> [Verdict; 4] {
    let lex_same = spirv
        .id_names
        .iter()
        .all(|(_, n)| ref_names.contains(&n.as_str()));
    [
        Verdict { layer: CmpLayer::Lexical, road: Road::Spirv, same: lex_same },
        Verdict { layer: CmpLayer::Structural, road: Road::Spirv, same: spirv.fp() == ref_fp },
        Verdict { layer: CmpLayer::Numeric, road: Road::Spirv, same: spirv.consts == ref_consts },
        Verdict { layer: CmpLayer::Declaration, road: Road::Spirv, same: spirv.iface_key == ref_iface },
    ]
}

/// 四路汇总矩阵
#[derive(Clone, Debug, Default)]
pub struct QuadMatrix {
    /// (变体名, 路, 全层同) 
    pub cells: Vec<(String, Road, bool)>,
}

impl QuadMatrix {
    pub fn record(&mut self, variant: &str, road: Road, same: bool) {
        self.cells.push((variant.to_string(), road, same));
    }

    /// 单路失败：阻断该变体并定位归因
    pub fn blocked_variants(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (v, _, same) in &self.cells {
            if !same && !out.contains(v) {
                out.push(v.clone());
            }
        }
        out
    }

    /// 四路到齐校验
    pub fn roads_complete(&self, variant: &str) -> bool {
        [Road::Glsl, Road::Hlsl, Road::Wgsl, Road::Spirv]
            .iter()
            .all(|r| self.cells.iter().any(|(v, rr, _)| v == variant && rr == r))
    }
}

/// 发布门槛判定器：四路全绿或白名单放行
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateVerdict {
    /// 四路全绿
    Pass,
    /// 白名单放行
    Whitelisted,
    /// 阻断
    Blocked,
}

/// 发布门槛
pub fn release_gate(quad_green: bool, whitelisted: bool, side_failed: bool) -> GateVerdict {
    if side_failed {
        return GateVerdict::Blocked;
    }
    if quad_green {
        GateVerdict::Pass
    } else if whitelisted {
        GateVerdict::Whitelisted
    } else {
        GateVerdict::Blocked
    }
}

/// 白名单（跨后端格式性差异）
#[derive(Clone, Debug, Default)]
pub struct Wl {
    pub entries: Vec<String>,
}

impl Wl {
    pub fn add(&mut self, e: &str) -> bool {
        if e.is_empty() || self.entries.iter().any(|x| x == e) {
            return false;
        }
        self.entries.push(e.to_string());
        true
    }

    pub fn allows(&self, e: &str) -> bool {
        self.entries.iter().any(|x| x == e)
    }
}

/// 仲裁结论回写：差异表 F0558 的 SPIR-V 列
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WriteBack {
    pub feature: String,
    /// 回写支持态（0 原生 1 需变换 2 不支持）
    pub support: u32,
    pub note: String,
}

impl WriteBack {
    pub fn valid(&self) -> bool {
        !self.feature.is_empty() && self.support <= 2
    }
}

/// 分工显性：纵深对拍（各后端单后端）vs 横宽汇总（本条）
pub fn division_of_labors(depth_owners: &[&str]) -> bool {
    depth_owners.contains(&"F0506") && depth_owners.contains(&"F0529") && depth_owners.contains(&"F0548") && depth_owners.contains(&"F0569")
}

pub fn run_vec178_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec178");
    // 族一：第四路适配器
    let ad = SpirvAdapter {
        id_names: vec![(1, "pos".to_string()), (2, "uv".to_string())],
        cf_fp: 777,
        consts: vec![1, 2, 3],
        iface_key: "set0:b0-uniform".to_string(),
    };
    s.add("适配-反汇编回溯", ad.lex_backtrace(1) == Some("pos"), "lex");
    s.add("适配-未命中", ad.lex_backtrace(9).is_none(), "miss");
    s.add("适配-指纹", ad.fp() == 777, "fp");
    // 族二：四层比对
    let v = compare_road4(&ad, &["pos", "uv"], 777, &[1, 2, 3], "set0:b0-uniform");
    s.add("四层-全同", v.iter().all(|x| x.same), "all");
    s.add("四层-层序", v[0].layer == CmpLayer::Lexical && v[3].layer == CmpLayer::Declaration, "order");
    s.add("四层-路标记Spirv", v.iter().all(|x| x.road == Road::Spirv), "road");
    let v2 = compare_road4(&ad, &["pos", "uv"], 999, &[1, 2, 3], "set0:b0-uniform");
    s.add("四层-结构失配检出", !v2[1].same && v2[0].same, "struct");
    let v3 = compare_road4(&ad, &["pos", "uv"], 777, &[1, 2, 4], "set0:b0-uniform");
    s.add("四层-数值失配检出", !v3[2].same, "num");
    let v4 = compare_road4(&ad, &["pos", "other"], 777, &[1, 2, 3], "x");
    s.add("四层-词法与声明失配", !v4[0].same && !v4[3].same, "lexdecl");
    // 族三：四路汇总矩阵
    let mut m = QuadMatrix::default();
    m.record("v1", Road::Glsl, true);
    m.record("v1", Road::Hlsl, true);
    m.record("v1", Road::Wgsl, true);
    m.record("v1", Road::Spirv, true);
    s.add("矩阵-四路到齐", m.roads_complete("v1"), "quad");
    s.add("矩阵-无阻断", m.blocked_variants().is_empty(), "clean");
    m.record("v2", Road::Spirv, false);
    s.add("矩阵-单路失败定位", m.blocked_variants() == vec!["v2".to_string()], "block");
    s.add("矩阵-缺路未齐", !m.roads_complete("v2"), "incomplete");
    // 族四：发布门槛
    s.add("门槛-全绿通过", release_gate(true, false, false) == GateVerdict::Pass, "pass");
    s.add("门槛-白名单放行", release_gate(false, true, false) == GateVerdict::Whitelisted, "wl");
    s.add("门槛-未登记阻断", release_gate(false, false, false) == GateVerdict::Blocked, "blk");
    s.add("门槛-单侧失败优先阻断", release_gate(true, true, true) == GateVerdict::Blocked, "side");
    // 族五：白名单
    let mut wl = Wl::default();
    s.add("白名单-登记", wl.add("id-order-cosmetic"), "add");
    s.add("白名单-重复拒绝", !wl.add("id-order-cosmetic"), "dup");
    s.add("白名单-查询", wl.allows("id-order-cosmetic"), "q");
    // 族六：回写差异表
    let wb = WriteBack { feature: "structured-cf".to_string(), support: 0, note: "四路全绿".to_string() };
    s.add("回写-有效", wb.valid(), "ok");
    s.add("回写-越界无效", !WriteBack { feature: "x".to_string(), support: 3, note: String::new() }.valid(), "oob");
    // 族七：分工显性
    s.add("分工-纵深四主在册", division_of_labors(&["F0506", "F0529", "F0548", "F0569"]), "div");
    s.add("分工-缺主不成立", !division_of_labors(&["F0506"]), "nodiv");
    s
}
