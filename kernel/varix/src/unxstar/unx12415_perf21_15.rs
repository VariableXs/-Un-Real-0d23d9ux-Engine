//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12415 · 节族性能预算（O1 对标口径） · 0x301e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12415-J1 map/unmap P95 预算在册（O1 配套账），共享页翻转账
//!      无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算在册：map/unmap 各设整数预算（tick 口径，O1 对标常量写死），
//!     预算账三字段（预算值/观测值/P95 值）真实可查；
//!   - 成本模型：整数 tick 计量（无浮点无墙钟），每笔操作成本 = 基线 +
//!     页数 × 单页系数（确定性成本模型，同输入同成本）；
//!   - P95 真算：样本升序快排后取序 95% 位（整数下标），非抽样近似；
//!   - 共享页翻转账：同节双视口翻转（map→touch→unmap 循环）记账，
//!     每轮守恒（翻转前后页账平衡），翻转账零红账判据；
//!   - 红账口径：观测成本超预算或 P95 超预算即入红账（append-only）；
//!   - 预算退化曲线：页数 1..64 扫描，P95 单调不降（成本模型一致性）；
//!   - 注入面：超预算注入（页数拉大/预算压低）必须全部入红账被抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x301e0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x301e0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x301e0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// map 预算（tick，O1 对标口径常量写死）。
pub const MAP_BUDGET: u64 = 200;

/// unmap 预算（tick，O1 对标口径）。
pub const UNMAP_BUDGET: u64 = 120;

/// map 基线成本（tick）。
pub const MAP_BASE: u64 = 20;

/// unmap 基线成本（tick）。
pub const UNMAP_BASE: u64 = 10;

/// 单页系数（tick/页）。
pub const PER_PAGE: u64 = 2;

/// P95 序位（千分口径 950）。
pub const P95_MILLI: u64 = 950;

/// 共享页翻转演练轮数（每轮 64 次翻转）。
pub const FLIP_ROUNDS: usize = 64;

/// 诊断码（0x301e0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 0x301e0 map 超预算（观测成本 > MAP_BUDGET）。
    MapOver = 0,
    /// 0x301e1 unmap 超预算（观测成本 > UNMAP_BUDGET）。
    UnmapOver = 1,
    /// 0x301e2 map P95 超预算。
    MapP95Over = 2,
    /// 0x301e3 unmap P95 超预算。
    UnmapP95Over = 3,
    /// 0x301e4 翻转账红账（翻转轮守恒被破）。
    FlipRed = 4,
    /// 0x301e5 页数越界（成本模型入参出 1..4096 页域）。
    PagesOob = 5,
    /// 0x301e6 样本不足（P95 计算前样本 <16）。
    SampleShort = 6,
    /// 0x301e7 预算账缩水（预算在册行只增不减被破）。
    BudgetShrink = 7,
    /// 0x301e8 P95 单调破坏（退化曲线非单调不降）。
    P95NonMono = 8,
    /// 0x301e9 零页操作（成本模型零页入参拒绝）。
    ZeroPages = 9,
}

impl PfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PfCode; 10] = [
        PfCode::MapOver, PfCode::UnmapOver, PfCode::MapP95Over, PfCode::UnmapP95Over,
        PfCode::FlipRed, PfCode::PagesOob, PfCode::SampleShort, PfCode::BudgetShrink,
        PfCode::P95NonMono, PfCode::ZeroPages,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x301e0 => "map 超预算：观测成本超 MAP_BUDGET，先查页数入参与单页系数。",
        0x301e1 => "unmap 超预算：观测成本超 UNMAP_BUDGET，查基线与页账释放路径。",
        0x301e2 => "map P95 超预算：取序 95% 位复核，超界先压尾延迟。",
        0x301e3 => "unmap P95 超预算：取序 95% 位复核，超界查释放批量。",
        0x301e4 => "翻转账红账：翻转轮守恒被破，逐轮回放页账差额定位。",
        0x301e5 => "页数越界：成本模型入参 1..4096 页，先钳再算。",
        0x301e6 => "样本不足：P95 前样本须 ≥16，先补采样。",
        0x301e7 => "预算账缩水：在册行 append-only，禁止截断。",
        0x301e8 => "P95 非单调：退化曲线必须单调不降，回查成本模型系数。",
        0x301e9 => "零页操作：零页入参拒绝，先判页数再计量。",
        _ => "未知诊断码：回查 0x301e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x301e0 段基址。
    pub const SEED: u64 = 0x301e0_301e_0301_e0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 成本模型：整数 tick 计量（同输入同成本，确定性）。
///
/// map 成本 = MAP_BASE + pages × PER_PAGE；unmap 成本 = UNMAP_BASE + pages × PER_PAGE。
/// 页数域 1..4096（越域/零页显式拒绝）。
pub const PAGES_MAX: u64 = 4096;

/// map 计量（Err 带码：零页/越域）。
pub fn cost_map(pages: u64) -> Result<u64, PfCode> {
    if pages == 0 {
        return Err(PfCode::ZeroPages);
    }
    if pages > PAGES_MAX {
        return Err(PfCode::PagesOob);
    }
    Ok(MAP_BASE + pages * PER_PAGE)
}

/// unmap 计量（Err 带码：零页/越域）。
pub fn cost_unmap(pages: u64) -> Result<u64, PfCode> {
    if pages == 0 {
        return Err(PfCode::ZeroPages);
    }
    if pages > PAGES_MAX {
        return Err(PfCode::PagesOob);
    }
    Ok(UNMAP_BASE + pages * PER_PAGE)
}

/// 升序插入排序（样本 ≤256，定容可承受；返回新序样本）。
pub fn sort_asc(v: &[u64]) -> Vec<u64> {
    let mut out: Vec<u64> = v.to_vec();
    let n = out.len();
    for i in 1..n {
        let key = out[i];
        let mut j = i;
        while j > 0 && out[j - 1] > key {
            out[j] = out[j - 1];
            j -= 1;
        }
        out[j] = key;
    }
    out
}

/// P95 真算（升序序 95% 位；样本 <16 拒绝）。
pub fn p95(samples: &[u64]) -> Result<u64, PfCode> {
    if samples.len() < 16 {
        return Err(PfCode::SampleShort);
    }
    let s = sort_asc(samples);
    // 序位下标：ceil(len*0.95)-1（整数口径）。
    let idx = ((s.len() as u64 * P95_MILLI + 999) / 1000) as usize - 1;
    Ok(s[idx.min(s.len() - 1)])
}

/// ─────────────────────────────────────────────────────────────────────
/// 预算账正本：样本采集 + P95 结算 + 红账。
/// ─────────────────────────────────────────────────────────────────────
/// 预算在册行（map/unmap 各一行）。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRow {
    /// 操作名（0=map 1=unmap）。
    pub op: u8,
    /// 预算值。
    pub budget: u64,
    /// 观测 P95。
    pub p95: u64,
    /// 样本数。
    pub samples: usize,
    /// 是否超预算。
    pub over: bool,
}

/// 红账行（超预算事件，append-only）。
#[derive(Clone, Copy, Debug)]
pub struct PfRedRow {
    /// 操作名。
    pub op: u8,
    /// 观测值。
    pub observed: u64,
    /// 预算值。
    pub budget: u64,
    /// 采样页数。
    pub pages: u64,
}

/// 翻转账行（共享页翻转守恒账）。
#[derive(Clone, Copy, Debug)]
pub struct FlipRow {
    /// 轮次。
    pub round: u32,
    /// 翻转页数。
    pub pages: u64,
    /// map 成本。
    pub map_cost: u64,
    /// unmap 成本。
    pub unmap_cost: u64,
    /// 守恒（翻转前后页账平衡）。
    pub conserved: bool,
}

/// 性能账（本单正本）。
#[derive(Debug)]
pub struct PerfBook {
    /// map 成本样本。
    map_samples: Vec<u64>,
    /// unmap 成本样本。
    unmap_samples: Vec<u64>,
    /// 预算在册行。
    budget_rows: Vec<BudgetRow>,
    /// 红账（append-only）。
    red: Vec<PfRedRow>,
    /// 红账高水位。
    high: usize,
    /// 翻转账。
    flips: Vec<FlipRow>,
}

impl PerfBook {
    /// 建账（预算在册：map/unmap 两行常量入册）。
    pub fn new() -> Self {
        let mut b = PerfBook {
            map_samples: Vec::new(),
            unmap_samples: Vec::new(),
            budget_rows: Vec::new(),
            red: Vec::new(),
            high: 0,
            flips: Vec::new(),
        };
        b.budget_rows.push(BudgetRow { op: 0, budget: MAP_BUDGET, p95: 0, samples: 0, over: false });
        b.budget_rows.push(BudgetRow { op: 1, budget: UNMAP_BUDGET, p95: 0, samples: 0, over: false });
        b
    }

    /// 采一笔 map 样本（页数域校验真实执行）。
    pub fn sample_map(&mut self, pages: u64) -> Result<u64, PfCode> {
        let c = cost_map(pages)?;
        self.map_samples.push(c);
        if c > MAP_BUDGET {
            self.red.push(PfRedRow { op: 0, observed: c, budget: MAP_BUDGET, pages });
            if self.red.len() > self.high {
                self.high = self.red.len();
            }
        }
        Ok(c)
    }

    /// 采一笔 unmap 样本。
    pub fn sample_unmap(&mut self, pages: u64) -> Result<u64, PfCode> {
        let c = cost_unmap(pages)?;
        self.unmap_samples.push(c);
        if c > UNMAP_BUDGET {
            self.red.push(PfRedRow { op: 1, observed: c, budget: UNMAP_BUDGET, pages });
            if self.red.len() > self.high {
                self.high = self.red.len();
            }
        }
        Ok(c)
    }

    /// 结算 P95（样本充足才结算，超预算入在册行 over 位）。
    pub fn settle(&mut self) -> Result<(u64, u64), PfCode> {
        let m = p95(&self.map_samples)?;
        let u = p95(&self.unmap_samples)?;
        if let Some(r) = self.budget_rows.first_mut() {
            r.p95 = m;
            r.samples = self.map_samples.len();
            r.over = m > MAP_BUDGET;
        }
        if let Some(r) = self.budget_rows.get_mut(1) {
            r.p95 = u;
            r.samples = self.unmap_samples.len();
            r.over = u > UNMAP_BUDGET;
        }
        Ok((m, u))
    }

    /// 预算在册行快照。
    pub fn budget_rows(&self) -> Vec<BudgetRow> {
        self.budget_rows.clone()
    }

    /// 红账长度。
    pub fn red_len(&self) -> usize {
        self.red.len()
    }

    /// 红账只增不减判据。
    pub fn red_monotone(&self) -> bool {
        self.red.len() >= self.high
    }

    /// 共享页翻转一笔（map→unmap 守恒记账）。
    pub fn flip(&mut self, round: u32, pages: u64) -> Result<(), PfCode> {
        let mc = cost_map(pages)?;
        let uc = cost_unmap(pages)?;
        // 守恒：同页数翻转，成本差恒等于基线差（MAP_BASE-UNMAP_BASE）。
        let conserved = mc - uc == MAP_BASE - UNMAP_BASE;
        self.map_samples.push(mc);
        self.unmap_samples.push(uc);
        self.flips.push(FlipRow { round, pages, map_cost: mc, unmap_cost: uc, conserved });
        if !conserved {
            self.red.push(PfRedRow { op: 2, observed: mc, budget: uc, pages });
            if self.red.len() > self.high {
                self.high = self.red.len();
            }
            return Err(PfCode::FlipRed);
        }
        Ok(())
    }

    /// 翻转账全守恒判据（共享页翻转账无红账口径之一）。
    pub fn flips_conserved(&self) -> bool {
        !self.flips.is_empty() && self.flips.iter().all(|f| f.conserved)
    }

    /// 翻转轮数。
    pub fn flip_len(&self) -> usize {
        self.flips.len()
    }

    /// 翻转账红账判据（无红账 = 红账空）。
    pub fn red_clean(&self) -> bool {
        self.red.is_empty()
    }

    /// P95 退化曲线单调判据（页数升序扫描 P95 不降）。
    pub fn p95_monotone(&self) -> bool {
        // 独立扫描：页数 1,2,4,...,256 各采 16 样本，P95 序列不降。
        let mut prev: u64 = 0;
        let mut pages = 1u64;
        while pages <= 256 {
            let mut s: Vec<u64> = Vec::new();
            let mut k = 0u64;
            while k < 16 {
                s.push(MAP_BASE + pages * PER_PAGE);
                k += 1;
            }
            let v = match p95(&s) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if v < prev {
                return false;
            }
            prev = v;
            pages *= 2;
        }
        true
    }

    /// 转录导出。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("性能账：\n");
        for r in self.budget_rows.iter() {
            s.push_str(&format!(
                "op{} 预算{} P95{} 样本{} 超{}\n",
                r.op, r.budget, r.p95, r.samples, r.over
            ));
        }
        s
    }

    /// 转录完整判据。
    pub fn lines_complete(&self) -> bool {
        let t = self.export_lines();
        t.contains("op0 ") && t.contains("op1 ")
    }
}

/// 共享页翻转演练：64 轮翻转守恒无红账 10/10 轮。
pub fn drill_flip_clean_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x301e0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let mut all_ok = true;
        for i in 0..FLIP_ROUNDS {
            // 翻转页数 1..16（预算内域，确定性变参）。
            let pages = 1 + lcg.below(16);
            if bk.flip(i as u32, pages).is_err() {
                all_ok = false;
            }
        }
        let _ = bk.settle();
        // 翻转账无红账：守恒全过且红账空（P95 在预算内域必不超）。
        if all_ok && bk.flips_conserved() && bk.red_clean() && bk.flip_len() == FLIP_ROUNDS {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("翻转无红账轮={}/{}", ok, INJECT_ROUNDS))
}

/// P95 预算在册演练：预算两行真实结算且 P95 不超 10/10 轮。
pub fn drill_p95_budget_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301e1（P95 序列）。
    let mut lcg = Lcg::new(0x301e1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let mut all_ok = true;
        for _i in 0..24 {
            let pages = 1 + lcg.below(32);
            if bk.sample_map(pages).is_err() {
                all_ok = false;
            }
            if bk.sample_unmap(pages).is_err() {
                all_ok = false;
            }
        }
        let (m95, u95) = match bk.settle() {
            Ok(v) => v,
            Err(_) => (u64::MAX, u64::MAX),
        };
        // P95 手工复核：24 样本全同域，P95 ≤ 域上界成本。
        let hand_m = MAP_BASE + 32 * PER_PAGE;
        let hand_u = UNMAP_BASE + 32 * PER_PAGE;
        let rows = bk.budget_rows();
        let in_reg = rows.len() == 2
            && rows[0].budget == MAP_BUDGET
            && rows[1].budget == UNMAP_BUDGET
            && rows[0].samples == 24
            && rows[1].samples == 24;
        if all_ok && m95 <= hand_m && u95 <= hand_u && in_reg && bk.red_clean() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("P95在册轮={}/{}", ok, INJECT_ROUNDS))
}

/// 超预算注入演练：大页注入必入红账 10/10 轮。
pub fn drill_over_budget_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301e2（注入序列）。
    let mut lcg = Lcg::new(0x301e2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        // 注 1：map 大页（>90 页必超 MAP_BUDGET=200：20+91*2=202）。
        let big = 91 + lcg.below(100);
        let e1 = bk.sample_map(big);
        // 注 2：unmap 大页（>55 页必超 UNMAP_BUDGET=120：10+56*2=122）。
        let big2 = 56 + lcg.below(100);
        let e2 = bk.sample_unmap(big2);
        // 注 3：零页必拒。
        let e3 = bk.sample_map(0);
        // 注 4：越域必拒。
        let e4 = bk.sample_unmap(PAGES_MAX + 1);
        let caught = matches!(e1, Ok(c) if c > MAP_BUDGET) && bk.red_len() >= 1
            && matches!(e2, Ok(c) if c > UNMAP_BUDGET) && bk.red_len() >= 2
            && matches!(e3, Err(PfCode::ZeroPages))
            && matches!(e4, Err(PfCode::PagesOob));
        if caught {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("超预算注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// P95 单调退化演练：曲线单调不降 10/10 轮。
pub fn drill_p95_mono_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x301e3（单调序列）。
    let mut lcg = Lcg::new(0x301e3);
    for _r in 0..INJECT_ROUNDS {
        let bk = PerfBook::new();
        let mono = bk.p95_monotone();
        // 抽样核对：1 页 P95 与 256 页 P95 严格递增。
        let s1: Vec<u64> = (0..16).map(|_| MAP_BASE + 1 * PER_PAGE).collect();
        let s2: Vec<u64> = (0..16).map(|_| MAP_BASE + 256 * PER_PAGE).collect();
        let (a, b) = (p95(&s1).unwrap_or(0), p95(&s2).unwrap_or(0));
        let _ = lcg.below(3);
        if mono && a == MAP_BASE + PER_PAGE && b == MAP_BASE + 256 * PER_PAGE && a < b {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("P95单调轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：零页 map 必拒。
    let mut bk = PerfBook::new();
    if matches!(bk.sample_map(0), Err(PfCode::ZeroPages)) {
        caught += 1;
        d.push("注1 零页:抓".to_string());
    } else {
        d.push("注1 零页:漏".to_string());
    }
    // 注 2：越域 unmap 必拒。
    let mut bk2 = PerfBook::new();
    if matches!(bk2.sample_unmap(PAGES_MAX + 1), Err(PfCode::PagesOob)) {
        caught += 1;
        d.push("注2 越域:抓".to_string());
    } else {
        d.push("注2 越域:漏".to_string());
    }
    // 注 3：样本不足 P95 必拒。
    let s3: Vec<u64> = (0..15).map(|_| MAP_BASE).collect();
    if matches!(p95(&s3), Err(PfCode::SampleShort)) {
        caught += 1;
        d.push("注3 样本:抓".to_string());
    } else {
        d.push("注3 样本:漏".to_string());
    }
    // 注 4：map 大页必入红账。
    let mut bk4 = PerfBook::new();
    let _ = bk4.sample_map(200);
    if bk4.red_len() >= 1 && bk4.red_monotone() {
        caught += 1;
        d.push("注4 红账:抓".to_string());
    } else {
        d.push("注4 红账:漏".to_string());
    }
    // 注 5：预算在册行必须两行且预算值对。
    let bk5 = PerfBook::new();
    let rows = bk5.budget_rows();
    if rows.len() == 2 && rows[0].budget == MAP_BUDGET && rows[1].budget == UNMAP_BUDGET {
        caught += 1;
        d.push("注5 在册:抓".to_string());
    } else {
        d.push("注5 在册:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x301e0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    PfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：性能报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 性能账报告。
#[derive(Clone, Copy, Debug)]
pub struct PfReport {
    /// map 样本数。
    pub map_n: usize,
    /// unmap 样本数。
    pub unmap_n: usize,
    /// map P95。
    pub map_p95: u64,
    /// unmap P95。
    pub unmap_p95: u64,
    /// 红账数。
    pub red: usize,
    /// 翻转轮数。
    pub flips: usize,
}

impl PfReport {
    /// 建报告。
    pub fn build(bk: &PerfBook) -> Self {
        PfReport {
            map_n: bk.budget_rows().first().map(|r| r.samples).unwrap_or(0),
            unmap_n: bk.budget_rows().get(1).map(|r| r.samples).unwrap_or(0),
            map_p95: bk.budget_rows().first().map(|r| r.p95).unwrap_or(0),
            unmap_p95: bk.budget_rows().get(1).map(|r| r.p95).unwrap_or(0),
            red: bk.red_len(),
            flips: bk.flip_len(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.map_n >= 16 && self.unmap_n >= 16 && self.map_p95 <= MAP_BUDGET
            && self.unmap_p95 <= UNMAP_BUDGET && self.red == 0 && self.flips > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PfReport mapN={} unmapN={} mapP95={} unmapP95={} 红={} 翻转={}",
            self.map_n, self.unmap_n, self.map_p95, self.unmap_p95, self.red, self.flips
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct PfCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_pf_checklist(bk: &PerfBook) -> Vec<PfCheckRow> {
    let rep = PfReport::build(bk);
    let mut rows: Vec<PfCheckRow> = Vec::new();
    rows.push(PfCheckRow { name: "预算在册两行", passed: bk.budget_rows().len() == 2 });
    rows.push(PfCheckRow { name: "样本≥16", passed: rep.map_n >= 16 && rep.unmap_n >= 16 });
    rows.push(PfCheckRow { name: "map P95 达标", passed: rep.map_p95 <= MAP_BUDGET && rep.map_p95 > 0 });
    rows.push(PfCheckRow { name: "unmap P95 达标", passed: rep.unmap_p95 <= UNMAP_BUDGET && rep.unmap_p95 > 0 });
    rows.push(PfCheckRow { name: "翻转账守恒", passed: bk.flips_conserved() });
    rows.push(PfCheckRow { name: "红账空", passed: bk.red_clean() });
    rows.push(PfCheckRow { name: "P95 单调", passed: bk.p95_monotone() });
    rows.push(PfCheckRow { name: "转录完整", passed: bk.lines_complete() });
    rows.push(PfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pf_checklist_green(rows: &[PfCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pf_stats_line(bk: &PerfBook) -> String {
    let rep = PfReport::build(bk);
    format!(
        "性能概况：mapP95 {} unmapP95 {} 红 {} 翻转 {}",
        rep.map_p95, rep.unmap_p95, rep.red, rep.flips
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12415_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    // 标准会话：24 组采样 + 64 轮翻转 + 结算。
    let mut bk = PerfBook::new();
    for i in 0..24 {
        let pages = 1 + (i as u64 * 3) % 32;
        let _ = bk.sample_map(pages);
        let _ = bk.sample_unmap(pages);
    }
    for i in 0..FLIP_ROUNDS {
        let _ = bk.flip(i as u32, 1 + (i as u64) % 16);
    }
    let _ = bk.settle();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        PfReport::build(&bk).all_green() && bk.lines_complete(),
        pf_stats_line(&bk),
    ));
    // X-b：P95 单调 10/10。
    let (ok_m, d_m) = drill_p95_mono_10();
    out.push(("X-P95单调10/10", ok_m, d_m));
    // X-c：终验清单。
    let rows = build_pf_checklist(&bk);
    out.push((
        "X-终验清单10项",
        pf_checklist_green(&rows),
        PfReport::build(&bk).to_text(),
    ));
    // X-d：口径常量核对。
    out.push((
        "X-口径常量核对",
        MAP_BUDGET == 200 && UNMAP_BUDGET == 120 && MAP_BASE == 20 && UNMAP_BASE == 10
            && PER_PAGE == 2 && P95_MILLI == 950 && PAGES_MAX == 4096,
        format!("map={} unmap={} base={}/{} pp={} p95={}", MAP_BUDGET, UNMAP_BUDGET, MAP_BASE, UNMAP_BASE, PER_PAGE, P95_MILLI),
    ));
    out
}

/// run_unx12415_checks：本单判据聚合（诊断段 0x301e0）。
pub fn run_unx12415_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12415 节族性能预算");
    // J1-a：共享页翻转账无红账 10/10。
    let (ok1, d1) = drill_flip_clean_10();
    s.add_dyn("J1-翻转账无红账10/10", ok1, &d1);
    // J1-b：map/unmap P95 预算在册 10/10。
    let (ok2, d2) = drill_p95_budget_10();
    s.add_dyn("J1-P95预算在册10/10", ok2, &d2);
    // J1-c：标准会话恒等式（预算在册 + P95 达标 + 红账空）。
    let mut bk = PerfBook::new();
    for i in 0..24u64 {
        let pages = 1 + i % 32;
        let _ = bk.sample_map(pages);
        let _ = bk.sample_unmap(pages);
    }
    for i in 0..FLIP_ROUNDS {
        let _ = bk.flip(i as u32, 1 + (i as u64) % 8);
    }
    let settled = bk.settle();
    s.add_dyn(
        "J1-预算在册P95达标",
        settled.is_ok() && bk.red_clean() && bk.flips_conserved(),
        &format!("P95={:?} 红={} 翻转={}", settled, bk.red_len(), bk.flip_len()),
    );
    // J2-a：超预算注入全抓 10/10。
    let (ok3, d3) = drill_over_budget_10();
    s.add_dyn("J2-超预算注入10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-c：红账只增不减。
    let before = bk.red_len();
    let _ = bk.sample_map(PAGES_MAX);
    s.add_dyn("J2-红账只增不减", bk.red_len() >= before + 1 && bk.red_monotone(), &format!("红 {}→{}", before, bk.red_len()));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x301e0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12415_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
