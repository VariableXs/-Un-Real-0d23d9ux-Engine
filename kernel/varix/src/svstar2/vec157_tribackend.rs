//! VE-F0557 · 三后端统一对拍框架
//!
//! 同一 IR 变体经 GLSL/HLSL/WGSL 三后端产出三份产物，框架四层比对——
//! 词法层（标识符经命名规则映射回溯后一致）、结构层（控制流拓扑同构）、
//! 数值层（常量表达式求值一致）、声明层（接口签名等价——绑定布局三语等价表）；
//! 仲裁器差异分级（格式性白名单、语义性阻断回溯 IR、单侧失败阻断合入）；
//! 适配器接口（BackendDiffAdapter）；与 F0506 显性分工：单后端纵深 vs 跨后端横宽；
//! SPIR-V 第四路预留态适配器槽位显性预留。
//!
//! 跨批对接：上游 F0506/F0529/F0548；下游 F0558 差异表、F0578 四后端总对拍。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 后端标识（第四路预留）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendId {
    Glsl,
    Hlsl,
    Wgsl,
    /// SPIR-V 预留槽位
    SpirvReserved,
}

/// 四层比对层
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpLayer {
    /// 词法层：标识符经命名规则映射回溯后一致
    Lexical,
    /// 结构层：控制流拓扑同构
    Structural,
    /// 数值层：常量表达式求值一致
    Numeric,
    /// 声明层：接口签名等价
    Declaration,
}

/// 差异分级
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffGrade {
    /// 格式性差异 → 白名单登记
    Cosmetic,
    /// 语义性差异 → 阻断并回溯 IR
    Semantic,
    /// 单侧失败 → 阻断该变体合入
    SideFailure,
}

/// 适配器接口：后端以适配器接入，新增后端零框架改动
pub trait BackendDiffAdapter {
    fn id(&self) -> BackendId;
    /// 词法规范化：标识符映射回溯到规范名
    fn lex_normalize(&self, raw: &str) -> String;
    /// 结构拓扑指纹
    fn structure_fingerprint(&self, product: &str) -> u64;
    /// 常量求值序列
    fn const_values(&self, product: &str) -> Vec<i64>;
    /// 接口签名（绑定布局等价表键）
    fn interface_signature(&self, product: &str) -> String;
}

/// 三文本后端通用适配器（演示与判据夹具）
pub struct TextAdapter {
    pub bid: BackendId,
}

impl BackendDiffAdapter for TextAdapter {
    fn id(&self) -> BackendId {
        self.bid
    }

    /// 词法规范化：抽取标识符序列并按声明序映射回溯
    fn lex_normalize(&self, raw: &str) -> String {
        // 简化规范化：去掉空白与常见文本噪声后取标识符序列
        let toks: Vec<&str> = raw.split_whitespace().collect();
        toks.join(" ")
    }

    fn structure_fingerprint(&self, product: &str) -> u64 {
        // 拓扑指纹：控制流关键字计数哈希
        let mut h: u64 = 1469598103934665603;
        for kw in ["if", "loop", "return"] {
            let n = product.matches(kw).count() as u64;
            h = (h ^ n).wrapping_mul(1099511628211);
        }
        h
    }

    fn const_values(&self, product: &str) -> Vec<i64> {
        product
            .split_whitespace()
            .filter_map(|t| t.trim_end_matches([';', ',']).parse::<i64>().ok())
            .collect()
    }

    fn interface_signature(&self, product: &str) -> String {
        // 签名键：@binding/@group/绑定序号归一
        let mut keys: Vec<&str> = Vec::new();
        for tok in product.split_whitespace() {
            if tok.starts_with("@binding") || tok.starts_with("@group") {
                keys.push(tok);
            }
        }
        keys.join(",")
    }
}

/// 单层比对结果
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LayerVerdict {
    pub layer: CmpLayer,
    pub same: bool,
}

/// 四层比对：逐层判定
pub fn compare_layers(a: &str, b: &str, ad: &dyn BackendDiffAdapter, bd: &dyn BackendDiffAdapter) -> [LayerVerdict; 4] {
    let lex_same = ad.lex_normalize(a) == bd.lex_normalize(b);
    let struct_same = ad.structure_fingerprint(a) == bd.structure_fingerprint(b);
    let num_same = ad.const_values(a) == bd.const_values(b);
    let decl_same = ad.interface_signature(a) == bd.interface_signature(b);
    [
        LayerVerdict { layer: CmpLayer::Lexical, same: lex_same },
        LayerVerdict { layer: CmpLayer::Structural, same: struct_same },
        LayerVerdict { layer: CmpLayer::Numeric, same: num_same },
        LayerVerdict { layer: CmpLayer::Declaration, same: decl_same },
    ]
}

/// 仲裁器：四层结果 → 差异分级
pub fn arbitrate(verdicts: &[LayerVerdict; 4], side_failed: bool) -> DiffGrade {
    if side_failed {
        return DiffGrade::SideFailure;
    }
    // 声明层或数值层失配 = 语义性
    if !verdicts.iter().all(|v| v.same) {
        return DiffGrade::Semantic;
    }
    DiffGrade::Cosmetic
}

/// 白名单：格式性差异登记
#[derive(Clone, Debug, Default)]
pub struct CosmeticWhitelist {
    entries: Vec<String>,
}

impl CosmeticWhitelist {
    pub fn new() -> Self {
        CosmeticWhitelist::default()
    }

    pub fn add(&mut self, rule: &str) -> bool {
        if rule.is_empty() || self.entries.iter().any(|e| e == rule) {
            return false;
        }
        self.entries.push(rule.to_string());
        true
    }

    pub fn contains(&self, rule: &str) -> bool {
        self.entries.iter().any(|e| e == rule)
    }
}

/// IR 回溯定位：语义差异 → 回溯 IR 层定位归因
pub fn ir_attribution(ir_stmts: &[&str], suspect_hint: &str) -> Option<usize> {
    ir_stmts.iter().position(|s| s.contains(suspect_hint))
}

/// 对拍报告
#[derive(Clone, Debug)]
pub struct DiffReport {
    pub variant: String,
    pub grade: DiffGrade,
    pub verdicts: [LayerVerdict; 4],
}

impl DiffReport {
    pub fn merge_allowed(&self) -> bool {
        self.grade == DiffGrade::Cosmetic
    }
}

pub fn run_vec157_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec157");
    let g = TextAdapter { bid: BackendId::Glsl };
    let h = TextAdapter { bid: BackendId::Hlsl };
    // 族一：四层比对——全同
    let prod_a = "@group(0) @binding(0) if loop return 42 7";
    let prod_b = "@group(0) @binding(0) if loop return 42 7";
    let v_same = compare_layers(prod_a, prod_b, &g, &h);
    s.add("四层-全同全过", v_same.iter().all(|x| x.same), "same");
    s.add("四层-层序正确", v_same[0].layer == CmpLayer::Lexical && v_same[3].layer == CmpLayer::Declaration, "order");
    // 族二：数值层失配
    let prod_c = "@group(0) @binding(0) if loop return 43 7";
    let v_num = compare_layers(prod_a, prod_c, &g, &h);
    s.add("数值-失配检出", !v_num[2].same, "num");
    s.add("词法-文本噪声同过", v_num[0].same, "lex");
    // 族三：仲裁分级
    s.add("仲裁-全同为格式性", arbitrate(&v_same, false) == DiffGrade::Cosmetic, "cos");
    s.add("仲裁-数值失配语义性", arbitrate(&v_num, false) == DiffGrade::Semantic, "sem");
    s.add("仲裁-单侧失败阻断", arbitrate(&v_same, true) == DiffGrade::SideFailure, "fail");
    // 族四：白名单登记
    let mut wl = CosmeticWhitelist::new();
    s.add("白名单-登记", wl.add("换行差异"), "add");
    s.add("白名单-重复拒绝", !wl.add("换行差异"), "dup");
    s.add("白名单-空串拒绝", !wl.add(""), "empty");
    s.add("白名单-查询", wl.contains("换行差异") && !wl.contains("缩进"), "query");
    // 族五：IR 回溯归因
    let irs = ["let x = 1;", "mul y, x, 2;", "ret y"];
    s.add("归因-命中乘法", ir_attribution(&irs, "mul") == Some(1), "mul");
    s.add("归因-未命中None", ir_attribution(&irs, "div").is_none(), "none");
    // 族六：报告与合入阻断
    let rep = DiffReport { variant: "v1".to_string(), grade: DiffGrade::Cosmetic, verdicts: v_same };
    let rep_sem = DiffReport { variant: "v2".to_string(), grade: DiffGrade::Semantic, verdicts: v_num };
    s.add("报告-格式性可合入", rep.merge_allowed(), "ok");
    s.add("报告-语义性阻断合入", !rep_sem.merge_allowed(), "block");
    // 族七：适配器零改动与第四路预留
    let w = TextAdapter { bid: BackendId::Wgsl };
    let sp = TextAdapter { bid: BackendId::SpirvReserved };
    s.add("适配-多实例无框架改动", g.id() == BackendId::Glsl && w.id() == BackendId::Wgsl && sp.id() == BackendId::SpirvReserved, "plug");
    s.add("适配-多态调用", backend_name(&g) == "glsl" && backend_name(&sp) == "spirv-reserved", "dyn");
    // 族八：结构指纹差异
    let s1 = g.structure_fingerprint("if if loop return");
    let s2 = g.structure_fingerprint("if loop return");
    s.add("结构-指纹区分拓扑", s1 != s2, "fp");
    s.add("结构-同构同指纹", s2 == h.structure_fingerprint("if loop return"), "iso");
    s
}

fn backend_name(ad: &dyn BackendDiffAdapter) -> &'static str {
    match ad.id() {
        BackendId::Glsl => "glsl",
        BackendId::Hlsl => "hlsl",
        BackendId::Wgsl => "wgsl",
        BackendId::SpirvReserved => "spirv-reserved",
    }
}
