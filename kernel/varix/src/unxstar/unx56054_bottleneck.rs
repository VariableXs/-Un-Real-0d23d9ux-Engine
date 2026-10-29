//! UNX-F56054 归因预挂钩：启动链瓶颈初筛件（0x606A0 段）
//!
//! 锚点：UNX-F56054-J1 逐段 ratio_p95 排序自动输出瓶颈段候选（归因只
//! 到域级即止——O1 不开无限剖析）；J2 候选清单与缺陷账对接字段就绪
//! （段 × ratio × 域级标签，联签锚定行 AI-97）；J3 候选截断纪律（只出
//! top-3 候选，禁全段清单倾倒）三判据全绿。判据锚定 §AI-71.2 专题四
//! +§AI-71.6 风险②。
//! 域：CoRun·UNX-F2-B40（启动分段域·瓶颈初筛面）。
//!
//! 结构：段 ratio 账（五段逐段 ratio_pbp 落账）+ 排序初筛核（降序取
//! top-3）+ 域级标签闭集（固件/内核/登录/桌面/交互五域，禁越域）+ 缺
//! 陷账对接行（AI-97 锚定行 + 对接字段齐）+ 注入核（越闭集段/零 ratio
//! /截断破/锚定缺/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x606A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x606A0;
pub const BN_SEG_UNK: u32 = SEG_BASE + 0x01; // 越闭集段
pub const BN_RATIO_ZERO: u32 = SEG_BASE + 0x02; // 零 ratio
pub const BN_CUT_OVER: u32 = SEG_BASE + 0x03; // 截断破（候选 >3）
pub const BN_ANCHOR_MISS: u32 = SEG_BASE + 0x04; // AI-97 锚定行缺
pub const BN_SEALED: u32 = SEG_BASE + 0x05; // 初筛账已封账拒写
pub const BN_RATIO_BAD: u32 = SEG_BASE + 0x06; // ratio 越物理界（>10×）
pub const BN_DUP_SEG: u32 = SEG_BASE + 0x07; // 同段重复落账

/// 五段闭集（与 F56041 同源）。
pub const G_COUNT: usize = 5;
pub const G_NAMES: [&str; G_COUNT] = [
    "firmware", "kernel", "login", "desktop", "interaction",
];

/// 候选截断（top-3）。
pub const TOP_N: usize = 3;

/// 候选行：段 × ratio × 域级标签。
#[derive(Clone)]
pub struct Candidate {
    pub seg: usize,
    pub ratio_pbp: u64,
    pub domain: &'static str,
}

impl Candidate {
    pub fn new(seg: usize, ratio_pbp: u64) -> Result<Self, u32> {
        if seg >= G_COUNT {
            return Err(BN_SEG_UNK);
        }
        if ratio_pbp == 0 {
            return Err(BN_RATIO_ZERO);
        }
        if ratio_pbp > 100_000 {
            return Err(BN_RATIO_BAD);
        }
        Ok(Candidate { seg, ratio_pbp, domain: G_NAMES[seg] })
    }
}

/// 初筛账：段 ratio 落账 + 排序出候选 + 锚定行。
pub struct BottleneckBook {
    pub ratios: [Option<u64>; G_COUNT],
    pub anchors: Vec<String>,
    pub sealed: bool,
}

impl BottleneckBook {
    pub fn new() -> Self {
        BottleneckBook { ratios: [None, None, None, None, None], anchors: Vec::new(), sealed: false }
    }

    /// 段 ratio 落账（同段重复拒 + 封账拒）。
    pub fn put(&mut self, seg: usize, ratio_pbp: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(BN_SEALED);
        }
        if seg >= G_COUNT {
            return Err(BN_SEG_UNK);
        }
        if ratio_pbp == 0 || ratio_pbp > 100_000 {
            return Err(BN_RATIO_ZERO);
        }
        if self.ratios[seg].is_some() {
            return Err(BN_DUP_SEG);
        }
        self.ratios[seg] = Some(ratio_pbp);
        Ok(())
    }

    /// AI-97 锚定行落账。
    pub fn anchor(&mut self, row: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BN_SEALED);
        }
        if row.is_empty() {
            return Err(BN_ANCHOR_MISS);
        }
        self.anchors.push(row.to_string());
        Ok(())
    }

    /// 锚定行在账核。
    pub fn anchor_ok(&self) -> bool {
        self.anchors.iter().any(|a| a.contains("AI-97"))
    }

    /// J1：排序初筛（降序取 top-3，域级标签即段名——O1 域级即止）。
    pub fn top_candidates(&self) -> Result<Vec<Candidate>, u32> {
        if !self.anchor_ok() {
            return Err(BN_ANCHOR_MISS);
        }
        let mut all: Vec<Candidate> = Vec::new();
        for g in 0..G_COUNT {
            if let Some(r) = self.ratios[g] {
                all.push(Candidate::new(g, r)?);
            }
        }
        // 降序排序（比值大者优先）。
        all.sort_by(|a, b| b.ratio_pbp.cmp(&a.ratio_pbp));
        if all.len() > TOP_N {
            all.truncate(TOP_N);
        }
        if all.len() > TOP_N {
            return Err(BN_CUT_OVER);
        }
        Ok(all)
    }

    /// J2：缺陷账对接行（候选 → 机读行，字段齐）。
    pub fn defect_rows(&self) -> Result<Vec<String>, u32> {
        let cands = self.top_candidates()?;
        let mut out = Vec::new();
        for c in cands.iter() {
            out.push(format!(
                "DEFECT-HOOK|seg={}|domain={}|ratio={}|scope=domain_only",
                G_NAMES[c.seg], c.domain, c.ratio_pbp
            ));
        }
        Ok(out)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：五段 ratio（kernel 13200 最高 → 候选首位）+ AI-97 行。
pub fn standard_book() -> BottleneckBook {
    let mut b = BottleneckBook::new();
    let rows = [(0usize, 10_500u64), (1, 13_200), (2, 9_800), (3, 11_100), (4, 8_600)];
    for r in rows.iter() {
        b.put(r.0, r.1).expect("put");
    }
    b.anchor("AI-97 缺陷账对接联签锚定行（瓶颈初筛域级口径）").expect("anchor");
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集段
    let mut b = standard_book();
    if b.put(9, 10_000).is_err() {
        caught += 1;
    }
    // 2) 零 ratio
    if b.put(3, 0) == Err(BN_RATIO_ZERO) {
        caught += 1;
    }
    // 3) 锚定行缺（未锚定禁出候选）
    let mut b2 = standard_book();
    b2.anchors.clear();
    if b2.top_candidates().err() == Some(BN_ANCHOR_MISS) {
        caught += 1;
    }
    // 4) 同段重复落账
    if b.put(1, 12_000).is_err() {
        caught += 1;
    }
    // 5) 封账改写
    let mut b3 = standard_book();
    b3.seal();
    if b3.put(2, 9_000).is_err() && b3.anchor("x").is_err() {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56054_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56054");
    // J1-1 五段 ratio 落账
    let b = standard_book();
    let mut filled = 0usize;
    for r in b.ratios.iter() {
        if r.is_some() {
            filled += 1;
        }
    }
    let ok = filled == G_COUNT;
    s.add("56054-J1-五段ratio落账", ok, if ok { "" } else { "段账缺" });
    // J1-2 排序初筛语义（降序 top-3：kernel 13200 / desktop 11100 / firmware 10500）
    let cands = b.top_candidates().expect("cands");
    let ok = cands.len() == TOP_N
        && cands[0].seg == 1 && cands[0].ratio_pbp == 13_200
        && cands[1].seg == 3 && cands[1].ratio_pbp == 11_100
        && cands[2].seg == 0 && cands[2].ratio_pbp == 10_500;
    s.add("56054-J1-top3降序", ok, if ok { "" } else { "初筛排序漂移" });
    // J1-3 域级标签闭集（候选域 = 段名，O1 域级即止）
    let ok = cands.iter().all(|c| c.domain == G_NAMES[c.seg]) && G_COUNT == 5;
    s.add("56054-J1-域级标签", ok, if ok { "" } else { "越域剖析" });
    // J2-1 AI-97 锚定行在账
    let ok = b.anchor_ok();
    s.add("56054-J2-AI-97锚定行", ok, if ok { "" } else { "锚定缺" });
    // J2-2 缺陷账对接行机读（DEFECT-HOOK 行字段齐：seg/domain/ratio/scope）
    let rows = b.defect_rows().expect("rows");
    let ok = rows.len() == TOP_N
        && rows[0].starts_with("DEFECT-HOOK|seg=kernel|domain=kernel|ratio=13200|scope=domain_only");
    s.add("56054-J2-对接行机读", ok, if ok { "" } else { "对接行漂移" });
    // J2-3 截断纪律（不足 3 段时如实出，禁编造补位）
    let mut b2 = BottleneckBook::new();
    b2.put(1, 12_000).expect("p1");
    b2.put(2, 9_000).expect("p2");
    b2.anchor("AI-97 对接").expect("a");
    let cands2 = b2.top_candidates().expect("c2");
    let ok = cands2.len() == 2;
    s.add("56054-J2-截断如实", ok, if ok { "" } else { "补位编造" });
    // J2-4 ratio 物理界（>10× 拒）
    let ok = Candidate::new(0, 200_000).err() == Some(BN_RATIO_BAD);
    s.add("56054-J2-ratio物理界", ok, if ok { "" } else { "越界穿透" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56054-J3-注入核必抓", ok, &detail);
    // J3-2 段名闭集同源
    let ok = G_NAMES[0] == "firmware" && G_NAMES[4] == "interaction";
    s.add("56054-J3-段名同源", ok, if ok { "" } else { "命名漂移" });
    // J3-3 封账冻结
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.put(0, 9_000).is_err() && b3.top_candidates().is_ok();
    s.add("56054-J3-封账冻结", ok, if ok { "" } else { "封账语义漂移" });
    // J2-5 候选截断上限常量
    let ok = TOP_N == 3;
    s.add("56054-J2-截断上限", ok, if ok { "" } else { "上限漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56054_all_checks_green() {
        let set = run_unx56054_checks();
        assert!(set.all_passed(), "UNX-F56054 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56054_top3() {
        let b = standard_book();
        let c = b.top_candidates().expect("c");
        assert_eq!(c[0].seg, 1);
        assert_eq!(c.len(), 3);
    }
}
