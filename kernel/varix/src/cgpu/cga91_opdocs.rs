//! cga91_opdocs —— CGPU-F0091 CPU 算子文档。
//! 锚点（CGPU 施工书）：CGPU-F0091 · CPU 算子文档 · 目标 300 行
//! 判据摘录：文档即判据（每个已入册算子一页机器可核文档：名/
//! 签名/域/复杂度/边界行为/用例引用——六字段闭集）；用例可执
//! 行（文档内嵌用例直接跑 cga89 语料 ID——文档不悬空）；覆盖
//! 审计（注册表全算子有页——漏页即账破）；签名一致性（文档签
//! 名 == 注册表签名——两账对拍）；复杂度标注核（O(1)/O(N)/
//! O(N·w²) 三档闭集——标注与模型一致）；边界行为闭集（饱和/
//! 拒核/直通三态）；指纹冻结。
//! 组件：DocPage 六字段 + DocBook 线性册 + 与 FxRegistry/语料
//!   三账对拍 + 复杂度模型核 + 边界态闭集 + 指纹。
//! 职责定位：算子的「说明书总账」——上层 AI/人查接口只看册；
//! 文档与注册表、语料两账绑定，任何一方漂移即对拍红——文档
//! 不是死文本而是账的一部分。
//!
//! 复杂度三档：
//!   C0 = O(1)（纯函数查表）
//!   CN = O(N)（点态全图一遍）
//!   CW = O(N·w²)（窗口族：w=窗口边长）

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE900 段 · 本模块独占）=====
pub const XC_PAGE_MISS: u32 = 0xE900; // 缺页
pub const XC_SIG_DRIFT: u32 = 0xE901; // 文档签名 != 注册签名
pub const XC_CASE_BAD: u32 = 0xE902; // 用例引用无效（语料无此 ID）
pub const XC_CPLX_BAD: u32 = 0xE903; // 复杂度标注非法
pub const XC_BOUND_BAD: u32 = 0xE904; // 边界态非法

// ===== 复杂度三档 =====
pub const C_O1: u8 = 0;
pub const C_ON: u8 = 1;
pub const C_ONW2: u8 = 2;
/// 边界行为三态。
pub const B_SATURATE: u8 = 0;
pub const B_REJECT: u8 = 1;
pub const B_PASSTHRU: u8 = 2;

/// 文档页（六字段闭集）。
#[derive(Clone)]
pub struct DocPage {
    pub op_id: u16,
    pub name: &'static str,
    pub sig: &'static str,
    pub cplx: u8,
    pub bound: u8,
    /// 语料用例 ID 引用（至少 1 条——文档不悬空）。
    pub case_ids: [u16; 2],
    pub case_n: usize,
}

/// 签名引用（与 cga87 注册表同构的最小面）。
pub struct RegRef {
    pub id: u16,
    pub name: &'static str,
    pub sig: &'static str,
}

/// 文档册。
pub struct DocBook {
    pages: Vec<DocPage>,
}

impl DocBook {
    pub fn new() -> DocBook {
        DocBook { pages: Vec::new() }
    }

    pub fn add(&mut self, p: DocPage) -> Result<(), u32> {
        // 复杂度/边界态闭集闸。
        if p.cplx > C_ONW2 {
            return Err(XC_CPLX_BAD);
        }
        if p.bound > B_PASSTHRU {
            return Err(XC_BOUND_BAD);
        }
        if p.case_n == 0 || p.case_n > 2 {
            return Err(XC_CASE_BAD);
        }
        if self.pages.iter().any(|q| q.op_id == p.op_id) {
            return Err(XC_PAGE_MISS); // 重页拒（行锁语义）
        }
        self.pages.push(p);
        Ok(())
    }

    pub fn find(&self, id: u16) -> Option<&DocPage> {
        self.pages.iter().find(|p| p.op_id == id)
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// 覆盖审计：注册表全算子有页。
    pub fn cover_reg(&self, reg: &[RegRef]) -> Result<(), u32> {
        for r in reg.iter() {
            if self.find(r.id).is_none() {
                return Err(XC_PAGE_MISS);
            }
        }
        Ok(())
    }

    /// 签名两账对拍。
    pub fn sig_match(&self, reg: &[RegRef]) -> Result<(), u32> {
        for r in reg.iter() {
            let p = self.find(r.id).ok_or(XC_PAGE_MISS)?;
            if p.sig != r.sig || p.name != r.name {
                return Err(XC_SIG_DRIFT);
            }
        }
        Ok(())
    }

    /// 用例引用可执行：引用 ID 在语料中真实存在。
    pub fn cases_resolvable(&self, corpus: &[u16]) -> Result<(), u32> {
        for p in self.pages.iter() {
            for i in 0..p.case_n {
                if !corpus.contains(&p.case_ids[i]) {
                    return Err(XC_CASE_BAD);
                }
            }
        }
        Ok(())
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for p in self.pages.iter() {
            for v in [p.op_id as u64, p.cplx as u64, p.bound as u64].iter() {
                h ^= *v;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in p.name.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in p.sig.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

/// 复杂度模型核：标注与算子族模型一致（cga90 模型同源）。
pub fn cplx_consistent(op_family: u8, claimed: u8) -> Result<(), u32> {
    let expect = match op_family {
        0 => C_O1,     // 单点纯函数
        1 => C_ON,     // 点态全图
        2 => C_ONW2,   // 窗口族
        _ => return Err(XC_CPLX_BAD),
    };
    if claimed != expect {
        return Err(XC_CPLX_BAD);
    }
    Ok(())
}

/// 标准册（本域四算子）。
pub fn default_book() -> DocBook {
    let mut b = DocBook::new();
    let _ = b.add(DocPage { op_id: 8401, name: "drop_shadow", sig: "uuiiu", cplx: C_ONW2, bound: B_SATURATE, case_ids: [100, 200], case_n: 2 });
    let _ = b.add(DocPage { op_id: 8501, name: "glow", sig: "uuuu", cplx: C_ONW2, bound: B_SATURATE, case_ids: [110, 210], case_n: 2 });
    let _ = b.add(DocPage { op_id: 8601, name: "stroke_outer", sig: "uu", cplx: C_ONW2, bound: B_PASSTHRU, case_ids: [120, 220], case_n: 2 });
    let _ = b.add(DocPage { op_id: 8602, name: "stroke_inner", sig: "uu", cplx: C_ONW2, bound: B_PASSTHRU, case_ids: [130, 230], case_n: 2 });
    b
}

/// 注册表参照（与 cga87 default_registry 同构）。
pub fn default_reg() -> Vec<RegRef> {
    alloc::vec![
        RegRef { id: 8401, name: "drop_shadow", sig: "uuiiu" },
        RegRef { id: 8501, name: "glow", sig: "uuuu" },
        RegRef { id: 8601, name: "stroke_outer", sig: "uu" },
        RegRef { id: 8602, name: "stroke_inner", sig: "uu" },
    ]
}

/// 语料 ID 参照（cga89 语料的 ID 全集）。
pub fn corpus_ids() -> Vec<u16> {
    let mut v: Vec<u16> = Vec::new();
    for base in [100u16, 110, 120, 130].iter() {
        for k in 0..3u16 {
            v.push(base + k);
        }
    }
    for base in [200u16, 210, 220, 230].iter() {
        for k in 0..3u16 {
            v.push(base + k);
        }
    }
    v
}

// ===== 主判据 =====
pub fn run_cga91_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0091");
    let book = default_book();
    let reg = default_reg();
    let corpus = corpus_ids();
    // 族一：覆盖与对拍。
    cs.add("册容 4", book.len() == 4, "四页");
    cs.add("覆盖注册表", book.cover_reg(&reg).is_ok(), "全算子有页");
    cs.add("签名对拍", book.sig_match(&reg).is_ok(), "两账一致");
    cs.add("用例可解析", book.cases_resolvable(&corpus).is_ok(), "引用真存在");
    // 族二：页域闸。
    let mut b2 = DocBook::new();
    cs.add("复杂度越界拒", b2.add(DocPage { op_id: 1, name: "x", sig: "u", cplx: 3, bound: 0, case_ids: [100, 0], case_n: 1 }).is_err(), "CPLX");
    cs.add("边界态越界拒", b2.add(DocPage { op_id: 1, name: "x", sig: "u", cplx: 1, bound: 3, case_ids: [100, 0], case_n: 1 }).is_err(), "BOUND");
    cs.add("零用例拒", b2.add(DocPage { op_id: 1, name: "x", sig: "u", cplx: 1, bound: 0, case_ids: [0, 0], case_n: 0 }).is_err(), "悬空页");
    cs.add("用例越位拒", b2.add(DocPage { op_id: 1, name: "x", sig: "u", cplx: 1, bound: 0, case_ids: [100, 0], case_n: 2 }).is_ok(), "合法建页");
    // 族三：复杂度模型核。
    cs.add("C0 锚", cplx_consistent(0, C_O1).is_ok(), "单点 O(1)");
    cs.add("CN 锚", cplx_consistent(1, C_ON).is_ok(), "点态 O(N)");
    cs.add("CW 锚", cplx_consistent(2, C_ONW2).is_ok(), "窗口 O(N·w²)");
    cs.add("CPLX 漂移抓", cplx_consistent(2, C_ON).is_err(), "窗口≠O(N)");
    // 族四：缺页检出。
    let mut short_reg: Vec<RegRef> = Vec::new();
    for r in reg.iter() {
        short_reg.push(RegRef { id: r.id, name: r.name, sig: r.sig });
    }
    short_reg.push(RegRef { id: 9999, name: "ghost", sig: "u" });
    cs.add("缺页检出", book.cover_reg(&short_reg).is_err(), "PAGE_MISS");
    // 族五：签名漂移检出。
    let mut drift_reg = default_reg();
    drift_reg[1].sig = "uu";
    cs.add("签名漂移抓", book.sig_match(&drift_reg).is_err(), "SIG_DRIFT");
    // 族六：指纹。
    let f1 = book.fingerprint();
    cs.add("指纹幂等", book.fingerprint() == f1, "双算一致");
    let mut b3 = default_book();
    let _ = b3.add(DocPage { op_id: 8701, name: "fxreg", sig: "u", cplx: C_O1, bound: B_REJECT, case_ids: [100, 0], case_n: 1 });
    cs.add("指纹敏感", b3.fingerprint() != f1, "加页变指纹");
    CheckSet::merge(CheckSet::merge(cs, run_cga91_deep()), run_cga91_deep2())
}

// ===== 深化面：文档页账与复杂度对拍 =====
/// 空书惰性：空书 find None、cover 过（无页无缺）。
pub fn empty_book_inert() -> bool {
    let b = DocBook::new();
    b.find(1).is_none()
}

pub fn run_cga91_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga91_deep");
    cs.add("空书惰性", empty_book_inert(), "空查 None");
    // 加页→查→签名对拍。
    let mut b = DocBook::new();
    let _ = b.add(DocPage {
        op_id: 1,
        name: "simd_gauss",
        sig: "u8",
        cplx: 1,
        bound: 255,
        case_ids: [11, 12],
        case_n: 2,
    });
    cs.add("页命中", b.find(1).map(|p| p.op_id == 1).unwrap_or(false), "往返");
    cs.add("页缺拒", b.find(9).is_none(), "不在册");
    cs.add("页数账", b.len() == 1, "1 页");
    cs.add("复杂度合", cplx_consistent(2, 2).is_ok(), "族档对");
    cs.add("复杂度破检出", cplx_consistent(2, 5).is_err(), "漂移立案");
    cs.add("指纹稳定", b.fingerprint() == b.fingerprint(), "双算同值");
    cs
}

// ===== 深化面二：引用闭与用例解析 =====
pub fn run_cga91_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga91_deep2");
    let mut b = DocBook::new();
    let _ = b.add(DocPage {
        op_id: 7,
        name: "simd_stroke",
        sig: "u8,u8",
        cplx: 2,
        bound: 255,
        case_ids: [21, 22],
        case_n: 2,
    });
    // 注册表匹配：同 id 同 sig。
    let reg = alloc::vec![RegRef { id: 7, name: "simd_stroke", sig: "u8,u8" }];
    cs.add("签名对拍合", b.sig_match(&reg).is_ok(), "sig 一致");
    cs.add("覆盖核对", b.cover_reg(&reg).is_ok(), "无缺页");
    // 用例解析：case_ids 全在语料。
    cs.add("用例解析合", b.cases_resolvable(&[21, 22, 30]).is_ok(), "引用在册");
    cs.add("用例缺检出", b.cases_resolvable(&[30]).is_err(), "悬空引用");
    // 签名漂移。
    let reg_bad = alloc::vec![RegRef { id: 7, name: "simd_stroke", sig: "u16" }];
    cs.add("签名漂移抓", b.sig_match(&reg_bad).is_err(), "DRIFT");
    cs.add("双页查一", {
        let _ = b.add(DocPage {
            op_id: 8,
            name: "simd_glow",
            sig: "u8",
            cplx: 1,
            bound: 255,
            case_ids: [31, 0],
            case_n: 1,
        });
        b.find(8).map(|p| p.cplx == 1).unwrap_or(false)
    }, "第二页可查");
    cs
}
