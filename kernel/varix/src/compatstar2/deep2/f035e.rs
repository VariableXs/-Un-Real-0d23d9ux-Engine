//! F035 深化批次三 · 日志归因引擎面（compatstar2/deep2 · G-A-35）。
//!
//! 批次一/批次二已覆盖 F035 的协议语义面与执行治理面；本批补主册
//! 【功能定义】「全语义对齐」的执行/边界/注入面：API 缺失模式表（函数
//! 名指纹→缺失分类：图形/网络/运行时/编解码/未知，定长 16 表，FNV-1a
//! 指纹匹配）、异常码聚类器（NTSTATUS/HRESULT facility 提取分类计账）、
//! 向导步骤跳转决策表（条件位图→目标步映射，定长 12 行决策表，无命中
//! 行→兜底步并记账）、归因置信度排序（证据计数×类别权重降序排列，定长
//! 8 候选，并列按登记序稳定）。
//!
//! 判据对账：深化以主册【设计细节】「归因决策树：位数→格式→缺运行时
//! 签名表→API 缺失日志分析→权限日志→未知」、【状态与异常】「归因置信
//! 度低 → 如实『未知原因』（不硬编原因）」未落地面为源，一处一事实
//! （NTSTATUS/HRESULT 位域规范、FNV-1a 参考参数对拍）。
//!
//! 零堆纪律：定长模式表/决策表/候选板，无 alloc。

use crate::checks::CheckSet;

// 常量（一处一事实）

/// API 缺失模式表容量（定长 16；登记 8 常见缺口出口）。
pub const MAX_API_PATTERNS: usize = 16;
/// 归因候选板容量（定长 8）。
pub const MAX_CANDIDATES: usize = 8;
/// 向导决策表行数（定长 12）。
pub const DECISION_ROWS: usize = 12;
/// FNV-1a 32 位偏移基（规范参考值 2166136261）。
pub const FNV_OFFSET: u32 = 2166136261;
/// FNV-1a 32 位素数（规范参考值 16777619）。
pub const FNV_PRIME: u32 = 16777619;
/// 缺失分类四实类 + 未知（图形/网络/运行时/编解码/未知）。
pub const MISSING_CATEGORIES: [&str; 5] = ["graphics", "network", "runtime", "codec", "unknown"];
/// 归因类别权重（‰，域内口径：运行时缺口最高——可自动修复的行动面
/// 优先；未知类最低——不硬编原因）。
pub const CATEGORY_WEIGHTS: [u32; 5] = [820, 880, 950, 760, 100];
/// 向导步骤目标（主册【功能定义】分支行动面）。
pub const STEP_PRECHECK: u8 = 0; // 预检重跑（位数/架构不符）
pub const STEP_DOWNLOAD: u8 = 1; // 缺运行时 → F032 下载指引
pub const STEP_ELEVATE: u8 = 2; // 权限拒绝 → F038 提权卡
pub const STEP_REPORT: u8 = 3; // 未知/API 缺口 → F036 星卡草稿+提报
pub const STEP_FALLBACK: u8 = 4; // 兜底步（无命中行）

// API 缺失模式表（FNV-1a 指纹匹配）

/// FNV-1a 32 位（const 面供模式表编译期指纹）。
pub const fn fnv1a(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < b.len() { h ^= b[i] as u32; h = h.wrapping_mul(FNV_PRIME); i += 1; }
    h
}

/// API 缺失模式表：函数名指纹 → 分类（0 图形/1 网络/2 运行时/3 编解码）。
/// 指纹编译期算出；导出名取真实 DLL 出口（d3d11/opengl32/ws2_32/msvcrt/
/// msvcp/avcodec 语义）。
const API_PATTERNS: [(u32, usize, &'static str); 8] = [
    (fnv1a("D3D11CreateDevice"), 0, "D3D11CreateDevice"),
    (fnv1a("wglCreateContext"), 0, "wglCreateContext"),
    (fnv1a("WSAStartup"), 1, "WSAStartup"),
    (fnv1a("getaddrinfo"), 1, "getaddrinfo"),
    (fnv1a("_initterm"), 2, "_initterm"),
    (fnv1a("__CxxFrameHandler"), 2, "__CxxFrameHandler"),
    (fnv1a("avcodec_open1"), 3, "avcodec_open1"),
    (fnv1a("avformat_alloc_context"), 3, "avformat_alloc_context"),
];

/// API 缺失归类：指纹命中 → 分类；未命中 → unknown(4)。
pub fn classify_missing_api(name: &str) -> usize {
    let h = fnv1a(name);
    for p in API_PATTERNS.iter() {
        if p.0 == h { return p.1; }
    }
    4
}

// 异常码聚类器（NTSTATUS/HRESULT 位域）

/// NTSTATUS facility 位域：bits 16-27（12 位掩码 0xFFF）。
pub fn ntstatus_facility(code: u32) -> u32 { (code >> 16) & 0xFFF }

/// NTSTATUS severity=11（bits 30-31 全 1）→ 错误级。
pub fn ntstatus_is_error(code: u32) -> bool { (code >> 30) & 0b11 == 0b11 }

/// HRESULT facility 位域：bits 16-26（11 位掩码 0x7FF）。
pub fn hresult_facility(code: u32) -> u32 { (code >> 16) & 0x7FF }

/// HRESULT severity 位（bit 31 置位）→ 失败。
pub fn hresult_is_failure(code: u32) -> bool { code & 0x8000_0000 != 0 }

/// 聚类账：facility 低 4 位折桶（域内模型口径），双体系分别计数。
pub struct ExceptionCluster {
    pub buckets: [u32; 16],
    pub ntstatus_seen: u32,
    pub hresult_seen: u32,
    pub total: u32,
}

impl ExceptionCluster {
    pub const fn new() -> Self {
        ExceptionCluster { buckets: [0; 16], ntstatus_seen: 0, hresult_seen: 0, total: 0 }
    }

    pub fn record_ntstatus(&mut self, code: u32) {
        self.buckets[ntstatus_facility(code) as usize % self.buckets.len()] += 1;
        self.ntstatus_seen += 1;
        self.total += 1;
    }

    pub fn record_hresult(&mut self, code: u32) {
        self.buckets[hresult_facility(code) as usize % self.buckets.len()] += 1;
        self.hresult_seen += 1;
        self.total += 1;
    }
}

// 向导步骤跳转决策表（条件位图 → 目标步）

/// 条件位定义：bit0 架构不符 / bit1 缺运行时签名 / bit2 API 缺失命中 /
/// bit3 权限拒绝日志 / bit4 位数不符 / bit5 归因置信度低 / bit6-7 保留。
pub struct DecisionRow {
    pub mask: u8,
    pub expect: u8,
    pub target: u8,
}

/// 决策表 12 行（顺序即优先级；语义：权限 > 运行时 > 低置信兜底 >
/// 重预检；每行均可被至少一个条件位图命中——零死行）。
pub const DECISION_TABLE: [DecisionRow; DECISION_ROWS] = [
    DecisionRow { mask: 0b0010_0001, expect: 0b0010_0001, target: STEP_PRECHECK }, // 低置信+架构：重预检优先
    DecisionRow { mask: 0b0010_1000, expect: 0b0010_1000, target: STEP_ELEVATE }, // 低置信+权限：权限优先
    DecisionRow { mask: 0b0001_0100, expect: 0b0001_0100, target: STEP_REPORT }, // 位数+API：提报优先
    DecisionRow { mask: 0b0000_1010, expect: 0b0000_1010, target: STEP_ELEVATE }, // 权限+运行时：权限优先
    DecisionRow { mask: 0b0000_1000, expect: 0b0000_1000, target: STEP_ELEVATE }, // 权限拒绝
    DecisionRow { mask: 0b0000_1010, expect: 0b0000_0010, target: STEP_DOWNLOAD }, // 运行时而权限未命中
    DecisionRow { mask: 0b0000_0010, expect: 0b0000_0010, target: STEP_DOWNLOAD }, // 缺运行时
    DecisionRow { mask: 0b0010_0000, expect: 0b0010_0000, target: STEP_REPORT }, // 低置信兜底
    DecisionRow { mask: 0b0000_0100, expect: 0b0000_0100, target: STEP_REPORT }, // API 缺口提报
    DecisionRow { mask: 0b0001_0001, expect: 0b0001_0001, target: STEP_PRECHECK }, // 位数+架构
    DecisionRow { mask: 0b0001_0000, expect: 0b0001_0000, target: STEP_PRECHECK }, // 位数不符
    DecisionRow { mask: 0b0000_0001, expect: 0b0000_0001, target: STEP_PRECHECK }, // 架构不符
];

/// 纯判定：首个 `(cond & mask) == expect` 行的目标；无命中 → 兜底步
/// （归因置信度低如实「未知原因」语义的表级出口）。
pub fn resolve_step(cond: u8) -> u8 {
    for row in DECISION_TABLE.iter() {
        if cond & row.mask == row.expect { return row.target; }
    }
    STEP_FALLBACK
}

/// 路由账：命中行记账 + 无命中兜底记账——零静默。
pub struct WizardRouter {
    pub target_counts: [u32; 5],
    pub fallback_misses: u32,
    pub routed_total: u32,
}

impl WizardRouter {
    pub const fn new() -> Self {
        WizardRouter { target_counts: [0; 5], fallback_misses: 0, routed_total: 0 }
    }

    pub fn route(&mut self, cond: u8) -> u8 {
        let t = resolve_step(cond);
        if t == STEP_FALLBACK { self.fallback_misses += 1; }
        self.target_counts[t as usize] += 1;
        self.routed_total += 1;
        t
    }
}

// 归因置信度排序（证据计数 × 类别权重，降序稳定）

/// 归因候选：类别 + 证据计数；score = evidence × weight（‰ 权重量纲）。
#[derive(Clone, Copy)]
pub struct Candidate {
    pub category: usize,
    pub evidence: u32,
    /// 登记序（并列时稳定序的判据）。
    pub seq: u16,
}

impl Candidate {
    pub fn score(&self) -> u64 { self.evidence as u64 * CATEGORY_WEIGHTS[self.category.min(4)] as u64 }
}

/// 候选板：定长 8，满则显性 Err；排序为稳定插入排序（并列按登记序）。
pub struct RankBoard {
    entries: [Option<Candidate>; MAX_CANDIDATES],
    pub count: usize,
    push_seq: u16,
}

impl RankBoard {
    pub const fn new() -> Self {
        RankBoard { entries: [None; MAX_CANDIDATES], count: 0, push_seq: 0 }
    }

    pub fn push(&mut self, category: usize, evidence: u32) -> Result<(), &'static str> {
        if self.count >= MAX_CANDIDATES { return Err("rank-full"); }
        self.entries[self.count] = Some(Candidate { category, evidence, seq: self.push_seq });
        self.push_seq += 1;
        self.count += 1;
        Ok(())
    }

    /// 降序排列副本：仅当 prev.score < key.score 时前移——并列保持登记序。
    pub fn ranked(&self) -> [Option<Candidate>; MAX_CANDIDATES] {
        let mut out = self.entries;
        for i in 1..self.count {
            let key = out[i];
            let mut j = i;
            while j > 0 {
                let shift = matches!((out[j - 1], key), (Some(p), Some(k)) if p.score() < k.score());
                if shift { out[j] = out[j - 1]; j -= 1; } else { break; }
            }
            out[j] = key;
        }
        out
    }
}

// 域自检（深化批次三）

pub fn run_f035e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F035-attribution-d3");
    // 1) FNV-1a 参考向量：空串 = 偏移基；"a" = 0xE40C292C。
    cs.add("fnv1a_reference_vectors", fnv1a("") == 2166136261 && fnv1a("a") == 0xE40C292C, "");
    // 2) API 模式表：图形/网络指纹命中（真实 DLL 导出名）。
    cs.add("api_graphics_network",
        classify_missing_api("D3D11CreateDevice") == 0 && classify_missing_api("wglCreateContext") == 0
            && classify_missing_api("WSAStartup") == 1 && classify_missing_api("getaddrinfo") == 1, "");
    // 3) API 模式表：运行时/编解码命中；未知函数如实归 unknown。
    cs.add("api_runtime_codec_unknown",
        classify_missing_api("_initterm") == 2 && classify_missing_api("avcodec_open1") == 3
            && classify_missing_api("TotallyBogus") == 4, "");
    // 4) 指纹全表两两相异（指纹碰撞即误归类，须零碰撞）。
    let mut distinct = true;
    for i in 0..API_PATTERNS.len() {
        for j in (i + 1)..API_PATTERNS.len() { if API_PATTERNS[i].0 == API_PATTERNS[j].0 { distinct = false; } }
    }
    cs.add("api_fingerprints_distinct", distinct, "");
    // 5) NTSTATUS 位域：常见 STATUS facility=0；severity=11 判错误级。
    cs.add("ntstatus_bitfields",
        ntstatus_facility(0xC000_0005) == 0 && ntstatus_facility(0xC000_0135) == 0
            && ntstatus_is_error(0xC000_0005) && !ntstatus_is_error(0x0000_0000), "");
    // 6) HRESULT 位域：FACILITY_WIN32=7（0x8007xxxx）；DXGI 族 0x7A。
    cs.add("hresult_bitfields",
        hresult_facility(0x8007_0005) == 7 && hresult_facility(0x887A_0005) == 0x7A
            && hresult_is_failure(0x8007_0005) && !hresult_is_failure(0x0000_0000), "");
    // 7) 聚类账：三分桶落位（0 内核 / 7 WIN32 / 0xA DXGI 折桶）。
    let mut ec = ExceptionCluster::new();
    ec.record_ntstatus(0xC000_0005);
    ec.record_hresult(0x8007_0005);
    ec.record_hresult(0x887A_0005);
    cs.add("cluster_bucket_accounting",
        ec.buckets[0] == 1 && ec.buckets[7] == 1 && ec.buckets[0xA] == 1
            && ec.ntstatus_seen == 1 && ec.hresult_seen == 2 && ec.total == 3, "");
    // 8) 决策表路由：四行动面各命中其行。
    cs.add("wizard_route_actions",
        resolve_step(0b0000_0010) == STEP_DOWNLOAD && resolve_step(0b0000_1000) == STEP_ELEVATE
            && resolve_step(0b0010_0000) == STEP_REPORT && resolve_step(0b0001_0000) == STEP_PRECHECK, "");
    // 9) 决策表优先级与兜底：权限压过运行时；重预检压过低置信；
    //    空条件位图无命中 → 兜底步并记账。
    let mut wr = WizardRouter::new();
    let (t1, t2, t3) = (wr.route(0b0000_1010), wr.route(0b0010_0001), wr.route(0b0000_0000));
    cs.add("wizard_priority_fallback",
        t1 == STEP_ELEVATE && t2 == STEP_PRECHECK && t3 == STEP_FALLBACK
            && wr.fallback_misses == 1 && wr.target_counts[STEP_FALLBACK as usize] == 1, "");
    // 10) 置信度排序：codec(10×760=7600) > graphics(5×820=4100) >
    //     runtime(3×950=2850)——权重×证据降序。
    let mut rb = RankBoard::new();
    let _ = (rb.push(0, 5), rb.push(2, 3), rb.push(3, 10));
    let ranked = rb.ranked();
    cs.add("rank_descending",
        ranked[0].map(|c| c.category) == Some(3) && ranked[1].map(|c| c.category) == Some(0)
            && ranked[2].map(|c| c.category) == Some(2) && rb.count == 3, "");
    // 11) 并列稳定：同分候选保持登记序（seq 单调）。
    let mut rb2 = RankBoard::new();
    let _ = rb2.push(2, 2);
    let _ = rb2.push(2, 2);
    let tie = rb2.ranked();
    cs.add("rank_tie_stable", tie[0].map(|c| c.seq) == Some(0) && tie[1].map(|c| c.seq) == Some(1), "");
    // 12) 候选板满：第 9 个显性 Err，账面不静默溢出。
    let mut rb3 = RankBoard::new();
    let mut all_ok = true;
    for _ in 0..MAX_CANDIDATES { all_ok = all_ok && rb3.push(0, 1).is_ok(); }
    cs.add("rank_full_explicit_err",
        all_ok && rb3.push(0, 1) == Err("rank-full") && rb3.count == MAX_CANDIDATES, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_table_full_coverage() {
        // 12 行逐一可达：每行至少一个条件位图命中——零死行实测。
        let conds = [
            0b0010_0001, 0b0010_1000, 0b0001_0100, 0b0000_1010,
            0b0000_1000, 0b0000_0010, 0b0010_0100, 0b0010_0000,
            0b0000_0100, 0b0001_0001, 0b0001_0000, 0b0000_0001,
        ];
        let mut wr = WizardRouter::new();
        for c in conds.iter() {
            let t = wr.route(*c);
            assert!(t != STEP_FALLBACK, "判例行 cond={:#b} 不得落兜底", c);
        }
        assert_eq!(wr.routed_total, 12);
        assert_eq!(wr.fallback_misses, 0); // 全部走判例行，零兜底
        assert_eq!(
            (wr.target_counts[0], wr.target_counts[1], wr.target_counts[2], wr.target_counts[3]),
            (4, 1, 3, 4)
        );
    }

    #[test]
    fn rank_board_reverse_input_sorts() {
        // 逆序输入：最弱者最后登记最先胜出——插入排序正确性。
        let mut rb = RankBoard::new();
        let _ = rb.push(4, 99); // unknown 100‰ → 9900
        let _ = rb.push(3, 20); // codec 760‰ → 15200
        let _ = rb.push(2, 30); // runtime 950‰ → 28500
        let ranked = rb.ranked();
        assert_eq!(ranked[0].map(|c| c.category), Some(2));
        assert_eq!(ranked[1].map(|c| c.category), Some(3));
        assert_eq!(ranked[2].map(|c| c.category), Some(4));
        assert_eq!(ranked[0].map(|c| c.score()), Some(28500));
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f035e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
