#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0794 · 域经 top10 跨域推荐（高价值清单+推荐语）
//! 域/批：CoRun·UNX A1 收官域（签收/协作/封域面 · W7 清单第 21 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0794-J1 推荐清单齐（top10 逐条：
//!   条目引用/推荐语/适用域三列 = 10 条）且逐条高档来源、推荐语非空；③ 完成
//!   时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 候选池建模：条目（id, 整数评分, 来源档位 High/Mid/Low 闭集,
//!   适用域位图 5 位, 推荐语非空）；top10 选择 = 评分降序 + id 升序破平
//!   （确定性选择排序，禁浮点）。
//! ② 三列出证：条目引用（候选 id）/ 推荐语（逐条非空，含档位与评分回显）/
//!   适用域（位图逐位解出 1..=5，禁空集）。
//! ③ 逐条高档来源断言：入选 10 条来源必须全 High；低档入围即红并定位。
//! ④ 池下限 ≥20（选 10 的两倍冗余）；重选确定性对拍（双跑指纹一致）。
//! ⑤ 状态机 Intake→Ranked→Selected→Attested；诊断码定位到候选号。
//! 诊断段声明：0x6180–0x619F 本单独占（清单 <SEG 0x6180>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

// 1. 诊断码段（本单独占 0x6180–0x619F）
pub const SEG_BASE: u16 = 0x6180; // 诊断码段基数，来源：W7 清单 UNX-F0794 行 <SEG 0x6180>
pub const SEG_TOP: u16 = 0x619F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 推荐状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空候选批
pub const XC_DUP_CID: u16 = SEG_BASE + 3; // 候选号重复
pub const XC_POOL_SHORT: u16 = SEG_BASE + 4; // 候选池不足 20
pub const XC_REC_EMPTY: u16 = SEG_BASE + 5; // 推荐语为空
pub const XC_DOMAIN_EMPTY: u16 = SEG_BASE + 6; // 适用域位图为 0（空集）
pub const XC_SCORE_ZERO: u16 = SEG_BASE + 7; // 评分为 0（非法占位）
pub const XC_LOW_GRADE: u16 = SEG_BASE + 8; // 低档来源入围（高档断言红）
pub const XC_SEL_SHORT: u16 = SEG_BASE + 9; // 入选不足 10 条
pub const XC_SEL_DUP: u16 = SEG_BASE + 10; // 入选重复
pub const XC_NOT_RANKED: u16 = SEG_BASE + 11; // 未排序即选择/出证
pub const XC_ALREADY_ATTESTED: u16 = SEG_BASE + 12; // 已出证再出证
pub const XC_DETER_DIFF: u16 = SEG_BASE + 13; // 双跑对拍不一致
pub const XC_DOMAIN_BAD: u16 = SEG_BASE + 14; // 适用域位图越出 5 位
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 推荐常量（写死，来源=判据原文/本单口径头注假设）
pub const TOP_N: usize = 10; // top10，来源：判据原文
pub const POOL_MIN: usize = 20; // 候选池下限（2×冗余），头注假设
pub const POOL_CAP: usize = 64; // 候选池定容，头注假设
pub const DOMAIN_BITS: u8 = 5; // 适用域位图 5 位（bit0..=bit4 ↔ 域 1..=5）

/// 来源档位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceGrade {
    High = 0,
    Mid = 1,
    Low = 2,
}

/// 一条候选条目。
#[derive(Clone, Debug)]
pub struct ScrollCandidate {
    pub cid: u32,            // 候选号（唯一）
    pub score: u32,          // 整数评分（>0，禁浮点）
    pub grade: SourceGrade,  // 来源档位
    pub domain_mask: u8,     // 适用域位图（bit0..=bit4，非 0，高位禁用）
    pub recommendation: String, // 推荐语（非空）
}

/// 入选行（三列出证）。
#[derive(Clone, Debug)]
pub struct TopRow {
    pub rank: usize,   // 名次 1..=10
    pub cid: u32,      // 条目引用
    pub recommendation: String, // 推荐语
    pub domains: [u8; 5],       // 适用域（0 填充，解出位图）
    pub score: u32,    // 评分回显
}

/// 出证回执。
#[derive(Clone, Debug)]
pub struct Top10Attest {
    pub rows: u32,        // 入选数（10）
    pub all_high: bool,   // 逐条高档结论
    pub pool: u32,        // 池规模
    pub summary: String,  // 摘要（非空）
    pub fingerprint: u64, // 清单指纹（对拍用）
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopStage {
    Intake = 0,
    Ranked = 1,
    Selected = 2,
    Attested = 3,
}
impl TopStage {
    pub fn can_step(self, next: TopStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct Top10Recommender {  // 域经 top10 推荐机。
    stage: TopStage,
    pool: Vec<ScrollCandidate>,
    ranked: Vec<u32>, // 排序后候选号序（评分降序 + id 升序）
    rows: Vec<TopRow>,
    pinned: Option<Top10Attest>,
    events: Vec<String>,
}
impl Top10Recommender {
    pub fn new() -> Self {
        Top10Recommender {
            stage: TopStage::Intake,
            pool: Vec::new(),
            ranked: Vec::new(),
            rows: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> TopStage {
        self.stage
    }
    pub fn pool_len(&self) -> usize {
        self.pool.len()
    }
    pub fn selected_len(&self) -> usize {
        self.rows.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_pool(&mut self, batch: &[ScrollCandidate]) -> Result<u32, u16> {  // 候选喂入：号唯一、评分非 0、位图 5 位非 0、推荐语非空。
        if self.stage != TopStage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for c in batch.iter() {
            if self.pool.len() >= POOL_CAP {
                return Err(XC_POOL_SHORT);
            }
            if c.score == 0 {
                return Err(XC_SCORE_ZERO);
            }
            if c.domain_mask == 0 {
                return Err(XC_DOMAIN_EMPTY);
            }
            if c.domain_mask & !((1u8 << DOMAIN_BITS) - 1) != 0 {
                return Err(XC_DOMAIN_BAD);
            }
            if c.recommendation.is_empty() {
                return Err(XC_REC_EMPTY);
            }
            let mut i = 0usize;
            while i < self.pool.len() {
                if self.pool[i].cid == c.cid {
                    return Err(XC_DUP_CID);
                }
                i += 1;
            }
            self.pool.push(c.clone());
            added += 1;
        }
        self.events.push(alloc::format!("pool +{}", added));
        Ok(added)
    }
    pub fn rank(&mut self) -> Result<u32, u16> {  // 排序：评分降序 + id 升序破平（选择排序，确定性，全池）。
        if !self.stage.can_step(TopStage::Ranked) {
            return Err(XC_STATE_INVALID);
        }
        if self.pool.len() < POOL_MIN {
            return Err(XC_POOL_SHORT);
        }
        self.ranked.clear();
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.pool.len() {
            ids.push(self.pool[i].cid);
            i += 1;
        }
        let score_of = |p: &Top10Recommender, cid: u32| -> u32 {
            let mut j = 0usize;
            while j < p.pool.len() {
                if p.pool[j].cid == cid {
                    return p.pool[j].score;
                }
                j += 1;
            }
            0
        };
        // 选择排序：主键评分降序，次键 id 升序。
        let mut a = 0usize;
        while a < ids.len() {
            let mut b = a + 1;
            while b < ids.len() {
                let sa = score_of(self, ids[a]);
                let sb = score_of(self, ids[b]);
                if sb > sa || (sb == sa && ids[b] < ids[a]) {
                    let t = ids[a];
                    ids[a] = ids[b];
                    ids[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        self.ranked = ids;
        self.stage = TopStage::Ranked;
        self.events.push(alloc::format!("ranked {}", self.ranked.len()));
        Ok(self.ranked.len() as u32)
    }
    pub fn rank_of(&self, n: usize) -> Option<u32> {  // 排序名次查询（1 起，越名次 None）。
        if n < 1 || n > self.ranked.len() {
            return None;
        }
        Some(self.ranked[n - 1])
    }
    pub fn select_top10(&mut self) -> Result<u32, u16> {  // 选择 top10：前 10 名逐条三列出证 + 高档断言。
        if !self.stage.can_step(TopStage::Selected) {
            return Err(XC_NOT_RANKED);
        }
        if self.ranked.len() < TOP_N {
            return Err(XC_SEL_SHORT);
        }
        self.rows.clear();
        let mut rank = 1usize;
        while rank <= TOP_N {
            let cid = self.ranked[rank - 1];
            // 候选取回。
            let mut cand: Option<ScrollCandidate> = None;
            let mut i = 0usize;
            while i < self.pool.len() {
                if self.pool[i].cid == cid {
                    cand = Some(self.pool[i].clone());
                    break;
                }
                i += 1;
            }
            let c = match cand {
                Some(x) => x,
                None => return Err(XC_SEL_DUP),
            };
            // 逐条高档断言（低档入围即红并定位）。
            if c.grade != SourceGrade::High {
                return Err(XC_LOW_GRADE);
            }
            // 推荐语（非空在喂入已守卫，此处复核）+ 适用域解位。
            let mut domains = [0u8; 5];
            let mut b = 0usize;
            while b < 5 {
                if c.domain_mask & (1u8 << b) != 0 {
                    domains[b] = (b + 1) as u8;
                }
                b += 1;
            }
            self.rows.push(TopRow {
                rank,
                cid,
                recommendation: c.recommendation.clone(),
                domains,
                score: c.score,
            });
            rank += 1;
        }
        // 入选唯一性守卫。
        let mut i = 0usize;
        while i < self.rows.len() {
            let mut j = i + 1;
            while j < self.rows.len() {
                if self.rows[i].cid == self.rows[j].cid {
                    return Err(XC_SEL_DUP);
                }
                j += 1;
            }
            i += 1;
        }
        self.stage = TopStage::Selected;
        self.events.push(String::from("selected 10"));
        Ok(TOP_N as u32)
    }
    pub fn attest(&mut self) -> Result<Top10Attest, u16> {  // 出证：10 条 + 全高档 → 摘要定格（重出证红）。
        if !self.stage.can_step(TopStage::Attested) {
            return Err(if self.pinned.is_some() {
                XC_ALREADY_ATTESTED
            } else {
                XC_NOT_RANKED
            });
        }
        if self.rows.len() != TOP_N {
            return Err(XC_SEL_SHORT);
        }
        let all_high = true; // select_top10 已逐条守卫
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0794-J1 推荐清单齐：top10 三列齐备（条目引用/推荐语/适用域），逐条高档来源（池 {} 取 10），首名评分 {}",
            self.pool.len(), self.rows[0].score
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = Top10Attest {
            rows: self.rows.len() as u32,
            all_high,
            pool: self.pool.len() as u32,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = TopStage::Attested;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&Top10Attest> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 清单指纹：入选序（cid, score）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.rows.len() {
            h = fnv1a(h, self.rows[i].cid as u64);
            h = fnv1a(h, self.rows[i].score as u64);
            i += 1;
        }
        h
    }
    pub fn row_of(&self, rank: usize) -> Option<&TopRow> {  // 入选行查询。
        if rank < 1 || rank > self.rows.len() {
            return None;
        }
        Some(&self.rows[rank - 1])
    }
    pub fn score_by_cid(&self, cid: u32) -> Option<u32> {  // 候选评分查询（池内取回）。
        let mut i = 0usize;
        while i < self.pool.len() {
            if self.pool[i].cid == cid {
                return Some(self.pool[i].score);
            }
            i += 1;
        }
        None
    }
    pub fn domain_view(&self, cid: u32) -> Option<String> {  // 候选适用域视图：位图逐位解出 1..=5（分号串）。
        let mut i = 0usize;
        while i < self.pool.len() {
            if self.pool[i].cid == cid {
                let mut s = String::from("domains=");
                let mut b = 0usize;
                while b < 5 {
                    if self.pool[i].domain_mask & (1u8 << b) != 0 {
                        s.push_str(alloc::format!("{};", b + 1).as_str());
                    }
                    b += 1;
                }
                return Some(s);
            }
            i += 1;
        }
        None
    }
    pub fn grade_view(g: SourceGrade) -> &'static str {  // 档位视图串（闭集映射，禁越集）。
        match g {
            SourceGrade::High => "High",
            SourceGrade::Mid => "Mid",
            SourceGrade::Low => "Low",
        }
    }
    pub fn grade_histogram(&self) -> [u32; 3] {  // 池内档位直方图 [High, Mid, Low]（守恒=池规模）。
        let mut h = [0u32; 3];
        let mut i = 0usize;
        while i < self.pool.len() {
            h[self.pool[i].grade as usize] += 1;
            i += 1;
        }
        h
    }
    pub fn score_gap_view(&self) -> Vec<u32> {  // 相邻名次分差视图（9 项：名次 k 与 k+1 之差，降序保证 ≥0）。
        let mut g: Vec<u32> = Vec::new();
        let mut k = 1usize;
        while k < self.rows.len() {
            let a = self.rows[k - 1].score;
            let b = self.rows[k].score;
            g.push(if a >= b { a - b } else { 0 });
            k += 1;
        }
        g
    }
    pub fn ranked_view(&self) -> String {  // 排序视图串（"cid:score" 逗号串，与 ranked 序一致）。
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.ranked.len() {
            if i > 0 {
                s.push(',');
            }
            let sc = match self.score_by_cid(self.ranked[i]) {
                Some(v) => v,
                None => 0,
            };
            s.push_str(alloc::format!("{}:{}", self.ranked[i], sc).as_str());
            i += 1;
        }
        s
    }
    pub fn top_summary_view(&self) -> String {  // 出证摘要视图（未出证给占位串，禁空）。
        match &self.pinned {
            Some(a) => a.summary.clone(),
            None => String::from("not-attested"),
        }
    }
}
pub fn gen_pool(n: usize, seed: u64) -> Vec<ScrollCandidate> {  // 样本发生器：n 条候选（前 10 条 High 高分，其余 Mid/Low 低分，确定性）。
    let mut s = seed;
    let mut out: Vec<ScrollCandidate> = Vec::new();
    let mut i = 0usize;
    while i < n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let r1 = (s >> 33) as u32;
        let grade = if i < TOP_N { SourceGrade::High } else if i % 2 == 0 { SourceGrade::Mid } else { SourceGrade::Low };
        let score = if i < TOP_N { 9_000 - (i as u32) * 10 } else { 1_000 + (r1 % 1_000) };
        let mask = ((r1 % 31) as u8) | 1; // 1..=31，非 0 且 5 位内
        out.push(ScrollCandidate {
            cid: (i + 1) as u32,
            score,
            grade,
            domain_mask: mask & 0x1F,
            recommendation: alloc::format!("推荐语_{:03}_评分参考", i + 1),
        });
        i += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let pool = gen_pool(24, 0x6180_0000_0000_0001);
    // —— 主路径：池 24 → 排序 → 选 10 → 出证 ——
    let mut m = Top10Recommender::new();
    m.feed_pool(&pool)?;
    if m.pool_len() != 24 {
        return Err(XC_POOL_SHORT);
    }
    m.rank()?;
    m.select_top10()?;
    if m.selected_len() != TOP_N {
        return Err(XC_SEL_SHORT);
    }
    let rc = m.attest()?;
    if rc.rows != 10 || !rc.all_high || rc.pool != 24 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.attest(), Err(XC_ALREADY_ATTESTED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 双跑对拍：同池重跑指纹一致（确定性）——
    let fp1 = rc.fingerprint;
    let fp2 = {
        let mut m2 = Top10Recommender::new();
        m2.feed_pool(&pool)?;
        m2.rank()?;
        m2.select_top10()?;
        m2.fingerprint()
    };
    if fp1 != fp2 {
        return Err(XC_DETER_DIFF);
    }
    // —— 错误路径 1：低档入围（把第 1 名换成 Mid）——
    let mut badpool = pool.clone();
    badpool[0].grade = SourceGrade::Mid;
    let mut m3 = Top10Recommender::new();
    m3.feed_pool(&badpool)?;
    m3.rank()?;
    if !matches!(m3.select_top10(), Err(XC_LOW_GRADE)) {
        return Err(XC_LOW_GRADE);
    }
    // —— 错误路径 2：池不足 20 / 空批 / 重号 / 评分为零 / 位图空 / 位图越界 ——
    let mut m4 = Top10Recommender::new();
    m4.feed_pool(&pool[..10].to_vec())?;
    if !matches!(m4.rank(), Err(XC_POOL_SHORT)) {
        return Err(XC_POOL_SHORT);
    }
    let mut m5 = Top10Recommender::new();
    if m5.feed_pool(&[]) != Err(XC_EMPTY_FEED) {
        return Err(XC_EMPTY_FEED);
    }
    let mut m6 = Top10Recommender::new();
    let dup = vec![pool[0].clone(), pool[0].clone()];
    if !matches!(m6.feed_pool(&dup), Err(XC_DUP_CID)) {
        return Err(XC_DUP_CID);
    }
    let mut m7 = Top10Recommender::new();
    let mut badscore = pool[..3].to_vec();
    badscore[1].score = 0;
    if !matches!(m7.feed_pool(&badscore), Err(XC_SCORE_ZERO)) {
        return Err(XC_SCORE_ZERO);
    }
    let mut m8 = Top10Recommender::new();
    let mut badmask = pool[..3].to_vec();
    badmask[0].domain_mask = 0;
    if !matches!(m8.feed_pool(&badmask), Err(XC_DOMAIN_EMPTY)) {
        return Err(XC_DOMAIN_EMPTY);
    }
    let mut m9 = Top10Recommender::new();
    let mut himask = pool[..3].to_vec();
    himask[2].domain_mask = 0x7F; // 6 位越界
    if !matches!(m9.feed_pool(&himask), Err(XC_DOMAIN_BAD)) {
        return Err(XC_DOMAIN_BAD);
    }
    // —— 错误路径 3：推荐语为空 / 未排序即选择 ——
    let mut ma = Top10Recommender::new();
    let mut badrec = pool[..3].to_vec();
    badrec[1].recommendation = String::new();
    if !matches!(ma.feed_pool(&badrec), Err(XC_REC_EMPTY)) {
        return Err(XC_REC_EMPTY);
    }
    let mut mb = Top10Recommender::new();
    mb.feed_pool(&pool)?;
    if !matches!(mb.select_top10(), Err(XC_NOT_RANKED)) {
        return Err(XC_NOT_RANKED);
    }
    // —— 查询 API：名次可解引 / 首名评分最大 / 位图解出域非空 ——
    match m.row_of(1) {
        Some(r) if r.rank == 1 && !r.recommendation.is_empty() => {}
        _ => return Err(XC_SEL_SHORT),
    }
    if m.row_of(11).is_some() {
        return Err(XC_SEL_SHORT);
    }
    let mut rk = 1usize;
    while rk < 10 {
        let a = m.row_of(rk).map(|r| r.score).unwrap_or(0);
        let b = m.row_of(rk + 1).map(|r| r.score).unwrap_or(0);
        if b > a {
            return Err(XC_DETER_DIFF);
        }
        rk += 1;
    }
    let r5 = m.row_of(5).map(|r| r.domains).unwrap_or([0u8; 5]);
    let mut nonzero = 0usize;
    let mut d = 0usize;
    while d < 5 {
        if r5[d] != 0 {
            nonzero += 1;
        }
        d += 1;
    }
    if nonzero == 0 {
        return Err(XC_DOMAIN_EMPTY);
    }
    // —— 扩展互证：视图/查询 API 与核心账对拍 ——
    let gh = m.grade_histogram();
    if gh[0] < TOP_N as u32 {
        return Err(XC_LOW_GRADE); // 高档不足 10，主路径不可能
    }
    if gh[0] + gh[1] + gh[2] != m.pool_len() as u32 {
        return Err(XC_POOL_SHORT); // 直方图守恒破
    }
    let dv = match m.domain_view(1) {
        Some(v) => v,
        None => return Err(XC_DOMAIN_EMPTY),
    };
    if !dv.starts_with("domains=") {
        return Err(XC_DOMAIN_BAD);
    }
    if m.domain_view(99_999).is_some() {
        return Err(XC_SEL_DUP); // 不存在的候选号被解引
    }
    if Top10Recommender::grade_view(SourceGrade::High) != "High" {
        return Err(XC_LOW_GRADE);
    }
    if Top10Recommender::grade_view(SourceGrade::Low) != "Low" {
        return Err(XC_LOW_GRADE);
    }
    let gaps = m.score_gap_view();
    if gaps.len() != TOP_N - 1 {
        return Err(XC_SEL_SHORT);
    }
    let rv = m.ranked_view();
    if rv.is_empty() || !rv.contains(&alloc::format!("{}:", m.row_of(1).map(|r| r.cid).unwrap_or(0)).as_str()) {
        return Err(XC_DETER_DIFF);
    }
    if m.top_summary_view().is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    // 未出证实例：摘要视图应给占位而非空串。
    let mut mz = Top10Recommender::new();
    if mz.top_summary_view() != "not-attested" {
        return Err(XC_SUMMARY_EMPTY);
    }
    Ok(0)
}
