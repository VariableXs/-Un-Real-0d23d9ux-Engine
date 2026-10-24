//! VE-F0780 九宫格组收口
//! 职责：F0761~F0779 二十单的组级收口——能力矩阵、跨模块交叉验证
//!       （元格式×装配×缓存联动）、遗留风险挂账、组完成判定。
//! 契约：矩阵二十行全 Done；交叉验证三例全绿；风险挂账远景。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};

/// 能力三态
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NCapState {
    Done,
    Missing,
}

/// 能力矩阵行
#[derive(Clone, Debug, PartialEq)]
pub struct NCapRow {
    pub task: &'static str,
    pub module: &'static str,
    pub state: NCapState,
}

/// 二十单能力矩阵
pub fn capability_matrix() -> Vec<NCapRow> {
    alloc::vec![
        NCapRow { task: "VE-F0761", module: "vec361_ninecore", state: NCapState::Done },
        NCapRow { task: "VE-F0762", module: "vec362_ninepolicy", state: NCapState::Done },
        NCapRow { task: "VE-F0763", module: "vec363_nineround", state: NCapState::Done },
        NCapRow { task: "VE-F0764", module: "vec364_ninemeta", state: NCapState::Done },
        NCapRow { task: "VE-F0765", module: "vec365_ninemin", state: NCapState::Done },
        NCapRow { task: "VE-F0766", module: "vec366_ninegpu", state: NCapState::Done },
        NCapRow { task: "VE-F0767", module: "vec367_ninecpu", state: NCapState::Done },
        NCapRow { task: "VE-F0768", module: "vec368_ninelayer", state: NCapState::Done },
        NCapRow { task: "VE-F0769", module: "vec369_nine4k", state: NCapState::Done },
        NCapRow { task: "VE-F0770", module: "vec370_ninedyn", state: NCapState::Done },
        NCapRow { task: "VE-F0771", module: "vec371_ninestroke", state: NCapState::Done },
        NCapRow { task: "VE-F0772", module: "vec372_ninebubble", state: NCapState::Done },
        NCapRow { task: "VE-F0773", module: "vec373_nineduel", state: NCapState::Done },
        NCapRow { task: "VE-F0774", module: "vec374_ninecache", state: NCapState::Done },
        NCapRow { task: "VE-F0775", module: "vec375_ninefuzz", state: NCapState::Done },
        NCapRow { task: "VE-F0776", module: "vec376_ninedocs", state: NCapState::Done },
        NCapRow { task: "VE-F0777", module: "vec377_ninebench", state: NCapState::Done },
        NCapRow { task: "VE-F0778", module: "vec378_ninedet", state: NCapState::Done },
        NCapRow { task: "VE-F0779", module: "vec379_nineasset", state: NCapState::Done },
        NCapRow { task: "VE-F0780", module: "vec380_nclose", state: NCapState::Done },
    ]
}

pub fn matrix_complete(m: &[NCapRow]) -> bool {
    m.len() == 20 && m.iter().all(|r| r.state == NCapState::Done)
}

/// 交叉验证一：元格式 → 装配 → 布局（全链路）
pub fn xval_meta_layout(text: &str, w: f64, h: f64) -> Option<[Cell; 9]> {
    let m = super::vec364_ninemeta::parse(text).ok()?;
    let sl = Slice::build(m.tex_w, m.tex_h, Insets { left: m.left, top: m.top, right: m.right, bottom: m.bottom })?;
    layout(&sl, 0.0, 0.0, w, h)
}

/// 交叉验证二：fuzz × 布局缓存——fuzz 样本经缓存往返仍一致
pub fn xval_fuzz_cache(seed: u64, rounds: usize) -> bool {
    let mut rng = super::vec335_cfuzz::Rng::new(seed);
    let mut cache = super::vec374_ninecache::LayoutCache::new(8);
    for _ in 0..rounds {
        let s = super::vec375_ninefuzz::gen_slice(&mut rng);
        let (w, h) = super::vec375_ninefuzz::gen_target(&mut rng, &s);
        let (via_cache, direct) = match (
            cache.get_or_compute(&s, w, h),
            layout(&s, 0.0, 0.0, w, h),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => return false,
        };
        if via_cache != direct {
            return false;
        }
    }
    true
}

/// 交叉验证三：4K 精度 × GPU 顶点——4K 尺寸下顶点路径不越画布
pub fn xval_4k_gpu() -> bool {
    let sl = match Slice::build(64.0, 64.0, Insets::uniform(8.0)) {
        Some(s) => s,
        None => return false,
    };
    let v = match super::vec366_ninegpu::emit_vertices(
        &sl,
        0.0,
        0.0,
        3840.0,
        2160.0,
        super::vec366_ninegpu::EmitCfg { half_pixel: true },
    ) {
        Some(v) => v,
        None => return false,
    };
    v.iter().all(|p| p.x >= 0.0 && p.x <= 3840.0 && p.y >= 0.0 && p.y <= 2160.0)
}

/// 遗留风险
pub fn leftover_risks() -> Vec<&'static str> {
    alloc::vec![
        "九宫格仅支持轴对齐矩形——旋转九宫格挂账 D06 后续",
        "气泡箭头当前为三角近似——样条箭头挂账曲线域扩展",
        "素材装配为同步解析——大图集异步加载挂账 D09",
    ]
}

/// 组完成判定
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NCloseInput {
    pub matrix_done: bool,
    pub xval_pass: bool,
    pub risks_filed: usize,
}

pub fn group_done(c: NCloseInput) -> bool {
    c.matrix_done && c.xval_pass && c.risks_filed >= 1
}

pub fn run_vec380_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec380");
    // 族一：能力矩阵
    let m = capability_matrix();
    s.add("矩阵-二十行", m.len() == 20, "20 rows");
    s.add("矩阵-全完成", matrix_complete(&m), "all Done");
    s.add("矩阵-任务号连续", m.windows(2).all(|w| {
        let a = w[0].task.trim_start_matches("VE-F");
        let b = w[1].task.trim_start_matches("VE-F");
        b.parse::<u32>().unwrap_or(0) == a.parse::<u32>().unwrap_or(0) + 1
    }), "consecutive");
    // 族二：交叉验证一（元格式→布局全链路）
    let text = "tex_w=64\ntex_h=64\ninset_l=8\ninset_t=8\ninset_r=8\ninset_b=8\n";
    let cells = xval_meta_layout(text, 100.0, 80.0);
    s.add("交叉-全链路", cells.is_some(), "meta → layout");
    if let Some(c) = cells {
        s.add("交叉-角区", c[0].w == 8.0 && c[0].h == 8.0, "corners 8");
    }
    // 非法元数据链路断
    s.add("交叉-非法断链", xval_meta_layout("garbage", 100.0, 80.0).is_none(), "bad meta none");
    // 族三：交叉验证二（fuzz×缓存）
    s.add("交叉-fuzz缓存", xval_fuzz_cache(2024, 16), "cache roundtrip");
    // 族四：交叉验证三（4K×GPU）
    s.add("交叉-4K顶点", xval_4k_gpu(), "4K verts in canvas");
    // 族五：风险挂账
    let risks = leftover_risks();
    s.add("风险-三条", risks.len() == 3, "3 risks");
    s.add("风险-挂账域", risks.iter().any(|r| r.contains("D06")) && risks.iter().any(|r| r.contains("D09")), "linked domains");
    // 族六：组判定
    s.add("判定-全绿", group_done(NCloseInput { matrix_done: true, xval_pass: true, risks_filed: 3 }), "group done");
    s.add("判定-矩阵缺", !group_done(NCloseInput { matrix_done: false, xval_pass: true, risks_filed: 3 }), "matrix gate");
    s.add("判定-交叉缺", !group_done(NCloseInput { matrix_done: true, xval_pass: false, risks_filed: 3 }), "xval gate");
    // 族七：前置组联动
    s.add("联动-布尔组", super::vec360_bclose::matrix_complete(&super::vec360_bclose::capability_matrix()), "bool group intact");
    s
}
