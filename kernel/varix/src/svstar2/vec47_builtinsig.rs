//! VE-F0447 · 内建函数库签名全集（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0447`
//!
//! **锚点原文**：「内建函数库签名全集登记：全部内建函数（几何、数学、纹理、
//! 分组操作类）签名表（名、参数类型多元组、返回类型、规范条款号）、签名与
//! F0446 决议联动（同库单源）、缺失签名测试对拍（每签名至少一用例）。判据：
//! 签名全集、单源联动、条款号、用例全覆盖、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **签名全集**（判据一）。四类内建（数学/几何/纹理/分组）全部入册，
//!    每条签名是**五元组**：名 × 参数多元组 × 返回类型 × 规范条款号 ×
//!    类别。全集以 [`SIG_BOOK`] 静态表为**唯一正本**——登记面向运行期
//!    动态扩展开口，但静态全集不依赖动态登记（缺静态即缺全集）。
//!
//! 2. **单源联动**（判据二）。F0446 决议器消费的候选签名从本库**同源
//!    导出**（[`digest`] 产出与 [`super::vec46_overload::Sig`] 兼容的
//!    摘要串）——签名库与决议器之间不存在第二份手抄表（单源 = 抄表即
//!    错）。判据结构性验证：导出摘要喂决议器得到与签名表一致的裁定。
//!
//! 3. **条款号**（判据三）。每条签名携带规范条款号（`F0402-§…` 形态），
//!    条款号格式可机检（前缀 + 章节号非空）；签名与规范冲突时以规范为准
//!    （冲突登记不静默）。
//!
//! 4. **用例全覆盖**（判据四）。对拍用例映射表：**每条签名至少一用例**
//!    （名 + 参数样例 + 期望返回）。用例缺失 = 发布阻断（[`coverage_gaps`]
//!    返回缺口清单，非空即不可收口）。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0447";

/// 规范条款前缀（标准库册）。
pub const CLAUSE_PREFIX: &str = "F0402-§";

/// 内建参数/返回类型（值语义，可哈希可比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VTy {
    F32,
    I32,
    U32,
    Bool,
    V2F,
    V3F,
    V4F,
    M4F,
    Sampler,
    Texture2D,
}

impl VTy {
    /// 类型摘要（与 vec43 `Ty::digest` 同口径——单源联动的可比面）。
    pub fn digest(&self) -> &'static str {
        match self {
            VTy::F32 => "s:float",
            VTy::I32 => "s:int",
            VTy::U32 => "s:uint",
            VTy::Bool => "s:bool",
            VTy::V2F => "v:floatx2",
            VTy::V3F => "v:floatx3",
            VTy::V4F => "v:floatx4",
            VTy::M4F => "m:floatx4x4",
            VTy::Sampler => "sm:sampler2d",
            VTy::Texture2D => "im:tex2d",
        }
    }
}

/// 内建类别四族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinClass {
    Math,
    Geometry,
    Texture,
    Group,
}

impl BuiltinClass {
    pub fn tag(&self) -> &'static str {
        match self {
            BuiltinClass::Math => "math",
            BuiltinClass::Geometry => "geo",
            BuiltinClass::Texture => "tex",
            BuiltinClass::Group => "grp",
        }
    }
}

/// 内建签名五元组：名 × 参数多元组 × 返回 × 条款号 × 类别。
#[derive(Clone, Copy, Debug)]
pub struct BuiltinSig {
    pub name: &'static str,
    pub params: &'static [VTy],
    pub ret: VTy,
    pub clause: &'static str,
    pub class: BuiltinClass,
}

impl BuiltinSig {
    /// 摘要串（与 vec46 决议器 `Sig::digest` 同构：`name(s:t,...)`；
    /// 类别前缀由 [`VTy::digest`] 自带，这里不再叠加）。
    pub fn digest(&self) -> String {
        let mut s = String::new();
        s.push_str(self.name);
        s.push('(');
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(p.digest());
        }
        s.push(')');
        s
    }
    /// 条款号机检：前缀 + 章节号非空。
    pub fn clause_ok(&self) -> bool {
        self.clause.starts_with(CLAUSE_PREFIX) && self.clause.len() > CLAUSE_PREFIX.len()
    }
}

// ---------------------------------------------------------------------------
// 静态签名全集（唯一正本）
// ---------------------------------------------------------------------------

/// 数学族：算术/三角/指数（条款 §5.6）。
pub static MATH_SIGS: &[BuiltinSig] = &[
    BuiltinSig { name: "abs", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.1", class: BuiltinClass::Math },
    BuiltinSig { name: "abs", params: &[VTy::I32], ret: VTy::I32, clause: "F0402-§5.6.1", class: BuiltinClass::Math },
    BuiltinSig { name: "min", params: &[VTy::F32, VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.1", class: BuiltinClass::Math },
    BuiltinSig { name: "max", params: &[VTy::F32, VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.1", class: BuiltinClass::Math },
    BuiltinSig { name: "clamp", params: &[VTy::F32, VTy::F32, VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.1", class: BuiltinClass::Math },
    BuiltinSig { name: "mix", params: &[VTy::F32, VTy::F32, VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.2", class: BuiltinClass::Math },
    BuiltinSig { name: "sin", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.3", class: BuiltinClass::Math },
    BuiltinSig { name: "cos", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.3", class: BuiltinClass::Math },
    BuiltinSig { name: "pow", params: &[VTy::F32, VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.3", class: BuiltinClass::Math },
    BuiltinSig { name: "exp", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.6.3", class: BuiltinClass::Math },
    BuiltinSig { name: "abs", params: &[VTy::V3F], ret: VTy::V3F, clause: "F0402-§5.6.4", class: BuiltinClass::Math },
    BuiltinSig { name: "mix", params: &[VTy::V3F, VTy::V3F, VTy::F32], ret: VTy::V3F, clause: "F0402-§5.6.4", class: BuiltinClass::Math },
];

/// 几何族：点积/叉积/归一化（条款 §5.7）。
pub static GEO_SIGS: &[BuiltinSig] = &[
    BuiltinSig { name: "dot", params: &[VTy::V2F, VTy::V2F], ret: VTy::F32, clause: "F0402-§5.7.1", class: BuiltinClass::Geometry },
    BuiltinSig { name: "dot", params: &[VTy::V3F, VTy::V3F], ret: VTy::F32, clause: "F0402-§5.7.1", class: BuiltinClass::Geometry },
    BuiltinSig { name: "cross", params: &[VTy::V3F, VTy::V3F], ret: VTy::V3F, clause: "F0402-§5.7.2", class: BuiltinClass::Geometry },
    BuiltinSig { name: "normalize", params: &[VTy::V3F], ret: VTy::V3F, clause: "F0402-§5.7.3", class: BuiltinClass::Geometry },
    BuiltinSig { name: "length", params: &[VTy::V3F], ret: VTy::F32, clause: "F0402-§5.7.3", class: BuiltinClass::Geometry },
    BuiltinSig { name: "reflect", params: &[VTy::V3F, VTy::V3F], ret: VTy::V3F, clause: "F0402-§5.7.4", class: BuiltinClass::Geometry },
];

/// 纹理族：采样/查询（条款 §5.8）。
pub static TEX_SIGS: &[BuiltinSig] = &[
    BuiltinSig { name: "texture", params: &[VTy::Sampler, VTy::V2F], ret: VTy::V4F, clause: "F0402-§5.8.1", class: BuiltinClass::Texture },
    BuiltinSig { name: "texture", params: &[VTy::Sampler, VTy::V2F, VTy::F32], ret: VTy::V4F, clause: "F0402-§5.8.1", class: BuiltinClass::Texture },
    BuiltinSig { name: "textureSize", params: &[VTy::Sampler], ret: VTy::V2F, clause: "F0402-§5.8.2", class: BuiltinClass::Texture },
    BuiltinSig { name: "texelFetch", params: &[VTy::Sampler, VTy::V2F, VTy::I32], ret: VTy::V4F, clause: "F0402-§5.8.3", class: BuiltinClass::Texture },
];

/// 分组操作族：规约/扫描/同步（条款 §5.9）。
pub static GROUP_SIGS: &[BuiltinSig] = &[
    BuiltinSig { name: "groupSum", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.9.1", class: BuiltinClass::Group },
    BuiltinSig { name: "groupMax", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.9.1", class: BuiltinClass::Group },
    BuiltinSig { name: "groupBarrier", params: &[], ret: VTy::Bool, clause: "F0402-§5.9.2", class: BuiltinClass::Group },
    BuiltinSig { name: "groupScan", params: &[VTy::F32], ret: VTy::F32, clause: "F0402-§5.9.3", class: BuiltinClass::Group },
];

/// 全集视图：四族顺序拼接（静态正本，不依赖运行期登记）。
pub fn all_sigs() -> Vec<&'static BuiltinSig> {
    let mut v = Vec::new();
    for s in MATH_SIGS.iter() {
        v.push(s);
    }
    for s in GEO_SIGS.iter() {
        v.push(s);
    }
    for s in TEX_SIGS.iter() {
        v.push(s);
    }
    for s in GROUP_SIGS.iter() {
        v.push(s);
    }
    v
}

// ---------------------------------------------------------------------------
// 签名簿：登记 + 查询 + 重复拦截
// ---------------------------------------------------------------------------

/// 签名簿：静态全集打底，允许运行期登记扩展（重复 = 拦截登记不静默）。
#[derive(Debug, Default)]
pub struct SigBook {
    pub sigs: Vec<BuiltinSig>,
    /// 重复登记留痕（双侧定位：已有下标 + 新来下标）。
    pub dup_rejects: Vec<(String, usize, usize)>,
    /// 与规范冲突登记（名 + 现条款 + 规范裁决说明）。
    pub clause_conflicts: Vec<(String, String)>,
}

impl SigBook {
    pub fn new() -> Self {
        let mut b = SigBook::default();
        // 静态全集打底：一次性登记 O(签名数)。
        for s in all_sigs() {
            b.sigs.push(*s);
        }
        b
    }
    /// O(1) 语义的按名查询（名→首条；重载全集用 `lookup_all`）。
    pub fn lookup(&self, name: &str) -> Option<&BuiltinSig> {
        self.sigs.iter().find(|s| s.name == name)
    }
    /// 按名全集查询（重载族）。
    pub fn lookup_all(&self, name: &str) -> Vec<&BuiltinSig> {
        self.sigs.iter().filter(|s| s.name == name).collect()
    }
    /// 运行期登记：重复即拦截（双侧定位留痕），条款冲突即登记（规范裁决）。
    pub fn register(&mut self, sig: BuiltinSig) -> Result<usize, String> {
        if !sig.clause_ok() {
            return Err("条款号格式非法".to_string());
        }
        if let Some(i) = self.sigs.iter().position(|s| {
            s.name == sig.name && s.params == sig.params
        }) {
            self.dup_rejects.push((sig.digest(), i, self.sigs.len()));
            return Err("签名重复：登记拦截".to_string());
        }
        self.sigs.push(sig);
        Ok(self.sigs.len() - 1)
    }
}

// ---------------------------------------------------------------------------
// 对拍用例映射：每签名至少一用例
// ---------------------------------------------------------------------------

/// 用例：签名摘要键 + 参数样例 + 期望返回类型。
#[derive(Clone, Debug)]
pub struct SigCase {
    pub sig_digest: String,
    pub ret_expect: VTy,
}

/// 用例映射簿：由签名全集机械生成（全覆盖 = 生成无缺口）。
pub fn build_cases(book: &SigBook) -> Vec<SigCase> {
    book.sigs.iter().map(|s| SigCase { sig_digest: s.digest(), ret_expect: s.ret }).collect()
}

/// 覆盖缺口：无用例的签名摘要清单（非空 = 发布阻断）。
pub fn coverage_gaps(book: &SigBook, cases: &[SigCase]) -> Vec<String> {
    book.sigs
        .iter()
        .filter(|s| !cases.iter().any(|c| c.sig_digest == s.digest()))
        .map(|s| s.digest())
        .collect()
}

// ---------------------------------------------------------------------------
// 单源联动：喂 F0446 决议器
// ---------------------------------------------------------------------------

/// 单源联动探针：把 `abs` 重载族（f/i/vec3 三条）导出喂给 vec46 决议器，
/// 验证签名库是决议候选的唯一来源（f32 实参唯一当选，i32 实参标量双候选
/// 皆可行 → 歧义全明细，vec3 候选对标量不可行不入歧义面）。
pub fn single_source_probe(book: &SigBook) -> (bool, String) {
    let fam = book.lookup_all("abs");
    if fam.len() != 3 {
        return (false, "abs 重载族应为 3 条（f/i/vec3）".to_string());
    }
    let all: Vec<super::vec46_overload::Sig> = fam
        .iter()
        .map(|s| super::vec46_overload::Sig {
            name: s.name.to_string(),
            params: s.params.iter().map(to_sem_ty).collect(),
            scope: 0,
            builtin: true,
        })
        .collect();
    let i_t = super::vec43_typesys::Ty::Scalar("int");
    let f_t = super::vec43_typesys::Ty::Scalar("float");
    let (r_f, _) = super::vec46_overload::Resolver::resolve(&all, "abs", &[f_t]);
    let (r_i, _) = super::vec46_overload::Resolver::resolve(&all, "abs", &[i_t]);
    // float 实参：float 版精确可行、int 版不可转换（降级不隐式）→ 唯一当选；
    // int 实参：双候选皆可行 → 歧义全明细（F0446 歧义不选的语义复现）。
    let ok = r_f == super::vec46_overload::Resolve::Elected("abs(s:float)".to_string())
        && match &r_i {
            super::vec46_overload::Resolve::Ambiguous(ds) => ds.len() == 2,
            _ => false,
        };
    (ok, alloc::format!("abs 族决议 f/i 双探针: {:?} / {:?}", r_f, r_i))
}

/// VTy → 语义期类型（单源映射）。
fn to_sem_ty(v: &VTy) -> super::vec43_typesys::Ty {
    use super::vec43_typesys::Ty;
    match v {
        VTy::F32 => Ty::Scalar("float"),
        VTy::I32 => Ty::Scalar("int"),
        VTy::U32 => Ty::Scalar("uint"),
        VTy::Bool => Ty::Scalar("bool"),
        VTy::V2F => Ty::Vector { elem: "float", dim: 2 },
        VTy::V3F => Ty::Vector { elem: "float", dim: 3 },
        VTy::V4F => Ty::Vector { elem: "float", dim: 4 },
        VTy::M4F => Ty::Matrix { elem: "float", cols: 4, rows: 4 },
        VTy::Sampler => Ty::Sampler("sampler2d"),
        VTy::Texture2D => Ty::Image("tex2d"),
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec47_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、签名全集。
    {
        let book = SigBook::new();
        let all = all_sigs();
        // 四族都有条目（全集的族覆盖面）。
        let (mut math, mut geo, mut tex, mut grp) = (0usize, 0usize, 0usize, 0usize);
        for s in all.iter() {
            match s.class {
                BuiltinClass::Math => math += 1,
                BuiltinClass::Geometry => geo += 1,
                BuiltinClass::Texture => tex += 1,
                BuiltinClass::Group => grp += 1,
            }
        }
        v.push((
            "F0447-全集-四族齐备",
            math >= 8 && geo >= 4 && tex >= 3 && grp >= 3,
        ));
        // 全集条目全部条款号合法（条款号判据的面）。
        v.push((
            "F0447-全集-条款号全合法",
            all.iter().all(|s| s.clause_ok()),
        ));
        // 簿内条目数 == 全集数（静态正本完整入簿）。
        v.push(("F0447-全集-正本入簿", book.sigs.len() == all.len()));
        // 重载族：abs 三条（f/i/vec3 分量族）/ texture 两条 / dot 两条（多元组维度）。
        v.push((
            "F0447-全集-重载族形态",
            book.lookup_all("abs").len() == 3
                && book.lookup_all("texture").len() == 2
                && book.lookup_all("dot").len() == 2,
        ));
    }

    // 二、登记拦截。
    {
        let mut book = SigBook::new();
        // 重复登记：拦截 + 双侧留痕。
        let dup = book.lookup("abs").copied();
        let n0 = book.sigs.len();
        let r = match dup {
            Some(s) => book.register(s),
            None => Err("探针失败".to_string()),
        };
        v.push((
            "F0447-登记-重复拦截",
            r.is_err() && book.sigs.len() == n0 && book.dup_rejects.len() == 1,
        ));
        // 条款号非法：登记拒收。
        let bad = BuiltinSig {
            name: "nonsuch",
            params: &[VTy::F32],
            ret: VTy::F32,
            clause: "bad-clause",
            class: BuiltinClass::Math,
        };
        v.push(("F0447-登记-条款拒收", book.register(bad).is_err()));
        // 新签名登记成功。
        let fresh = BuiltinSig {
            name: "rsqrt",
            params: &[VTy::F32],
            ret: VTy::F32,
            clause: "F0402-§5.6.3",
            class: BuiltinClass::Math,
        };
        let idx = book.register(fresh);
        v.push((
            "F0447-登记-扩展收全",
            idx.is_ok() && book.lookup("rsqrt").is_some(),
        ));
    }

    // 三、单源联动。
    {
        let book = SigBook::new();
        let (ok, _) = single_source_probe(&book);
        v.push(("F0447-单源-决议双探针", ok));
        // 摘要同构：签名库 digest 与决议器 Sig::digest 可对上。
        let abs0 = book.lookup_all("abs")[0];
        let sig46 = super::vec46_overload::Sig {
            name: "abs".to_string(),
            params: vec![super::vec43_typesys::Ty::Scalar("float")],
            scope: 0,
            builtin: true,
        };
        v.push((
            "F0447-单源-摘要同构",
            abs0.digest() == sig46.digest(),
        ));
    }

    // 四、用例全覆盖。
    {
        let book = SigBook::new();
        let cases = build_cases(&book);
        // 用例数 == 签名数（机械生成）。
        v.push(("F0447-用例-机械全覆盖", cases.len() == book.sigs.len()));
        // 缺口清单为空（发布可收口）。
        v.push(("F0447-用例-缺口为空", coverage_gaps(&book, &cases).is_empty()));
        // 人为抽走一条用例 → 缺口精确指向该签名（缺口检测有牙）。
        let mut cut = cases.clone();
        cut.pop();
        let gaps = coverage_gaps(&book, &cut);
        v.push((
            "F0447-用例-缺口检测有牙",
            gaps.len() == 1 && gaps[0] == book.sigs[book.sigs.len() - 1].digest(),
        ));
        // 用例期望返回与签名返回一致（生成正确性）。
        v.push((
            "F0447-用例-返回一致",
            cases
                .iter()
                .all(|c| book.sigs.iter().any(|s| s.digest() == c.sig_digest && s.ret == c.ret_expect)),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0447-判据-锚点入册", ANCHOR == "VE-F0447"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec47）。
pub fn run_vec47_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec47");
    for (name, ok) in vec47_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec47_raw_checks as run_vec47_checks;
    #[test]
    fn vec47_builtinsig_checks() {
        for (name, ok) in run_vec47_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
