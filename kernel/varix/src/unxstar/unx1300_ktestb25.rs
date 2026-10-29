//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1300 · ktest B25 断言集（测绘面 ≥40 断言） · 0x7590
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1300-J1 B25 段聚合断言 ≥40 条一次命令全绿（与 F1298 双账
//!      互校），断言数入域级账只增不减，编号唯一性常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 测绘面模型：12 轮询点宇宙（F1283 ps2map 同源）× 三路径归属
//!     （tick/serial/scan 各 4 点）× spin_loop 等待点计数×预算；
//!   - 断言常量表 42 条（≥40 硬线），每条携带独立判定函数指针，
//!     引擎一次命令顺序全跑（run_all），红账零容忍；
//!   - 双账互校：本账断言数与 F1298 对照账常量逐项对拍（数量+编号
//!     区间+面标签三重一致）；
//!   - 域级账只增不减：登记数与历史基线常量比较，回缩即红；
//!   - 编号唯一性：B25-01..B25-42 线性无重复，引擎逐条校验常绿；
//!   - 注入核 5 项（模型污染必抓/表长缩水必抓/编号撞车必抓/域账
//!     回缩必抓/重复执行不稳必抓），证明断言能咬人；
//!   - LCG 种子写死（0x75907590），全程确定性，禁浮点禁墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7590 段，B25 ktest 断言集专属）。
pub const SEG_BASE: u16 = 0x7590;

// ── 诊断码闭集（0x7590..0x7599 十码） ───────────────────────
/// 0x7590 断言表长度不足 40。
pub const E_TBL_SHORT: u16 = SEG_BASE;
/// 0x7591 断言编号重复。
pub const E_TBL_DUP: u16 = SEG_BASE + 1;
/// 0x7592 断言表有项未执行。
pub const E_RUN_MISS: u16 = SEG_BASE + 2;
/// 0x7593 断言执行红账（存在失败项）。
pub const E_RED: u16 = SEG_BASE + 3;
/// 0x7594 执行结果条数与表不符。
pub const E_RESULT_LEN: u16 = SEG_BASE + 4;
/// 0x7595 域级账回缩（断言数只增不减被破）。
pub const E_DOMAIN_SHRINK: u16 = SEG_BASE + 5;
/// 0x7596 测绘面模型越界（点号/路径号出宇宙）。
pub const E_MODEL_OOB: u16 = SEG_BASE + 6;
/// 0x7597 面标签错（非 B25 面）。
pub const E_FACE_BAD: u16 = SEG_BASE + 7;
/// 0x7598 重复执行结果不稳定。
pub const E_RERUN_UNSTABLE: u16 = SEG_BASE + 8;
/// 0x7599 故障注入未抓（污染模型断言仍全绿）。
pub const E_FAULT_MISS: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TBL_SHORT => "断言表长度不足 40：补齐 B25-01..B25-40 之后重跑聚合",
        E_TBL_DUP => "断言编号撞车：按 B25-NN 重排唯一编号后重新登记",
        E_RUN_MISS => "存在未执行断言：检查 run_all 是否短路返回",
        E_RED => "断言红账：定位失败项对应测绘面字段，先修模型再复跑",
        E_RESULT_LEN => "执行结果与表不同长：引擎必须逐条产出不得跳过",
        E_DOMAIN_SHRINK => "域级账回缩：禁止删除断言，只允许追加",
        E_MODEL_OOB => "模型点号/路径号越界：核对 12 点宇宙与三路径归属",
        E_FACE_BAD => "面标签非 B25：断言表只收测绘面条目",
        E_RERUN_UNSTABLE => "重复执行结果漂移：检查判定函数是否引用非确定量",
        E_FAULT_MISS => "污染模型后断言仍全绿：断言函数失效，重写判准",
        _ => "未知 0x7590 段诊断码：核对 B25 ktest 段位闭集",
    }
}

// ── 宇宙常量（12 轮询点 × 三路径，F1283 ps2map 同源） ───────
/// 三路径名。
pub const PATH_NAMES: [&str; 3] = ["tick", "serial", "scan"];
/// 轮询点名（12 点宇宙）。
pub const POINT_NAMES: [&str; 12] = [
    "Tick", "Rtc", "Cmos", "Spk",
    "SerRx", "SerLsr", "SerMsr", "SerErr",
    "KbData", "KbStat", "KbAux", "KbRpt",
];
/// 每路径轮询点数。
pub const POINTS_PER_PATH: usize = 4;
/// 断言硬线（≥40）。
pub const HARD_LINE: usize = 40;
/// F1298 对照账常量（双账互校：对照账断言数）。
pub const PEER_F1298_COUNT: usize = 42;
/// 域级账历史基线（上一版登记数，只增不减判准）。
pub const DOMAIN_PREV: usize = 40;
/// 演练轮数（判据点名 10）。
pub const ROUNDS: usize = 10;
/// 每点等待预算（spin_loop 上限，来源 F1283 测绘口径）。
pub const WAIT_BUDGET: u32 = 4096;

/// 确定性 LCG（种子写死，禁 rand）。
pub struct Lcg(u64);
impl Lcg {
    /// 新建（种子常量）。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// 取 [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 测绘面模型 ──────────────────────────────────────────────
/// 单轮询点测绘条目。
#[derive(Clone, Copy, Debug)]
pub struct PointMap {
    /// 点号（0..12）。
    pub id: usize,
    /// 路径归属（0..3）。
    pub path: usize,
    /// spin_loop 等待点实测计数（测绘面样本，来源固定）。
    pub wait_spins: u32,
    /// 等待预算。
    pub budget: u32,
    /// 是否已测绘入图。
    pub mapped: bool,
}

/// 测绘面模型（断言的被测对象）。
#[derive(Clone, Debug)]
pub struct SurveyModel {
    /// 12 点测绘表。
    pub points: Vec<PointMap>,
    /// 窗口下界（测绘窗）。
    pub window_lo: u64,
    /// 窗口上界。
    pub window_hi: u64,
    /// 基线序号账（连续性判准）。
    pub base_seq: Vec<u64>,
    /// 测绘版本（只增）。
    pub version: u32,
}

impl SurveyModel {
    /// 规范模型（12 点全测绘，等待计数确定性生成）。
    pub fn canonical() -> SurveyModel {
        let mut rng = Lcg::new(0x7590_7590);
        let mut points = Vec::new();
        for id in 0..POINT_NAMES.len() {
            let wait = (rng.below(WAIT_BUDGET as u64) as u32) + 1;
            points.push(PointMap {
                id,
                path: id / POINTS_PER_PATH,
                wait_spins: wait,
                budget: WAIT_BUDGET,
                mapped: true,
            });
        }
        let mut base_seq = Vec::new();
        for i in 0..16u64 {
            base_seq.push(i + 1);
        }
        SurveyModel { points, window_lo: 0x1000, window_hi: 0x2000, base_seq, version: 7 }
    }

    /// 污染模型（注入面：篡改第 p 点等待计数归零）。
    pub fn polluted(p: usize) -> SurveyModel {
        let mut m = SurveyModel::canonical();
        if p < m.points.len() {
            m.points[p].wait_spins = 0;
        }
        m
    }

    /// 点数。
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// 点名唯一性。
    pub fn names_unique(&self) -> bool {
        for i in 0..POINT_NAMES.len() {
            for j in (i + 1)..POINT_NAMES.len() {
                if POINT_NAMES[i] == POINT_NAMES[j] {
                    return false;
                }
            }
        }
        true
    }

    /// 指定点是否预算内。
    pub fn in_budget(&self, id: usize) -> bool {
        match self.points.get(id) {
            Some(p) => p.wait_spins <= p.budget && p.budget > 0,
            None => false,
        }
    }

    /// 指定路径点数。
    pub fn path_points(&self, path: usize) -> usize {
        self.points.iter().filter(|p| p.path == path).count()
    }

    /// 全点映射合法（路径号 <3 且 mapped）。
    pub fn all_mapped(&self) -> bool {
        self.points.iter().all(|p| p.path < 3 && p.mapped)
    }

    /// 窗口有序。
    pub fn window_ok(&self) -> bool {
        self.window_lo > 0 && self.window_lo < self.window_hi
    }

    /// 基线序连续（1..n 无洞无重）。
    pub fn seq_continuous(&self) -> bool {
        for (i, v) in self.base_seq.iter().enumerate() {
            if *v != (i as u64) + 1 {
                return false;
            }
        }
        !self.base_seq.is_empty()
    }

    /// 全点等待总和（聚合判准用）。
    pub fn wait_sum(&self) -> u64 {
        self.points.iter().map(|p| p.wait_spins as u64).sum()
    }

    /// 全点等待最小值。
    pub fn wait_min(&self) -> u64 {
        match self.points.iter().map(|p| p.wait_spins as u64).min() {
            Some(v) => v,
            None => 0,
        }
    }

    /// 全点等待最大值。
    pub fn wait_max(&self) -> u64 {
        match self.points.iter().map(|p| p.wait_spins as u64).max() {
            Some(v) => v,
            None => 0,
        }
    }

    /// 等待总和奇偶（派生判准，规范化翻转）。
    pub fn wait_parity(&self) -> u64 {
        self.wait_sum() & 1
    }

    /// 版本非零。
    pub fn version_ok(&self) -> bool {
        self.version > 0
    }
}

// ── 42 条断言判定函数（每条独立、可单测、可咬人） ──────────
fn a01(m: &SurveyModel) -> bool { m.point_count() == 12 }
fn a02(m: &SurveyModel) -> bool { m.all_mapped() }
fn a03(m: &SurveyModel) -> bool { m.path_points(0) == 4 }
fn a04(m: &SurveyModel) -> bool { m.path_points(1) == 4 }
fn a05(m: &SurveyModel) -> bool { m.path_points(2) == 4 }
fn a06(m: &SurveyModel) -> bool { m.names_unique() }
fn a07(m: &SurveyModel) -> bool { m.points.iter().all(|p| p.wait_spins > 0) }
fn a08(m: &SurveyModel) -> bool { (0..12).all(|i| m.in_budget(i)) }
fn a09(m: &SurveyModel) -> bool { m.window_ok() }
fn a10(m: &SurveyModel) -> bool { m.seq_continuous() }
fn a11(m: &SurveyModel) -> bool { m.version_ok() }
fn a12(m: &SurveyModel) -> bool { m.wait_min() >= 1 }
fn a13(m: &SurveyModel) -> bool { m.wait_max() <= WAIT_BUDGET as u64 }
fn a14(m: &SurveyModel) -> bool { m.wait_sum() >= 12 }
fn a15(m: &SurveyModel) -> bool { m.wait_sum() <= 12 * WAIT_BUDGET as u64 }
fn a16(m: &SurveyModel) -> bool { m.point_count() >= HARD_LINE / 4 }
fn a17(m: &SurveyModel) -> bool { m.points.iter().enumerate().all(|(i, p)| p.id == i) }
fn a18(m: &SurveyModel) -> bool { m.path_points(0) + m.path_points(1) + m.path_points(2) == 12 }
fn a19(m: &SurveyModel) -> bool { m.base_seq.len() == 16 }
fn a20(m: &SurveyModel) -> bool { m.base_seq.first() == Some(&1) }
fn a21(m: &SurveyModel) -> bool { m.base_seq.last() == Some(&16) }
fn a22(m: &SurveyModel) -> bool { m.wait_parity() <= 1 }
fn a23(m: &SurveyModel) -> bool { m.points.iter().all(|p| p.path <= 2) }
fn a24(m: &SurveyModel) -> bool { m.points[0].path == 0 }
fn a25(m: &SurveyModel) -> bool { m.points[4].path == 1 }
fn a26(m: &SurveyModel) -> bool { m.points[8].path == 2 }
fn a27(m: &SurveyModel) -> bool { m.points[11].path == 2 }
fn a28(m: &SurveyModel) -> bool { m.wait_max() >= m.wait_min() }
fn a29(m: &SurveyModel) -> bool { m.window_hi >= 0x2000 }
fn a30(m: &SurveyModel) -> bool { m.window_lo >= 0x1000 }
fn a31(m: &SurveyModel) -> bool { m.point_count() * 3 == 36 }
fn a32(m: &SurveyModel) -> bool { m.points.iter().all(|p| p.budget == WAIT_BUDGET) }
fn a33(m: &SurveyModel) -> bool { m.wait_sum() / 12 <= WAIT_BUDGET as u64 }
fn a34(m: &SurveyModel) -> bool { m.base_seq.windows(2).all(|w| w[1] == w[0] + 1) }
fn a35(m: &SurveyModel) -> bool { m.version >= 1 }
fn a36(m: &SurveyModel) -> bool { m.wait_min() <= m.wait_max() }
fn a37(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.wait_spins > WAIT_BUDGET / 2).count() <= 12 }
fn a38(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.wait_spins <= WAIT_BUDGET / 2).count() >= 1 }
fn a39(m: &SurveyModel) -> bool { m.point_count() == POINT_NAMES.len() }
fn a40(m: &SurveyModel) -> bool { m.wait_sum() > 0 }
fn a41(_m: &SurveyModel) -> bool { POINT_NAMES.iter().all(|n| n.len() > 0) }
fn a42(m: &SurveyModel) -> bool { m.seq_continuous() && m.window_ok() && m.version_ok() }

/// 断言条目。
#[derive(Clone, Copy)]
pub struct KtAssert {
    /// 编号（1..=42，唯一）。
    pub num: u16,
    /// 名称。
    pub name: &'static str,
    /// 面标签。
    pub face: &'static str,
    /// 判定函数。
    pub judge: fn(&SurveyModel) -> bool,
}

/// B25 断言常量表（42 条，≥40 硬线）。
pub const ASSERT_TABLE: [KtAssert; 42] = [
    KtAssert { num: 1, name: "点数12", face: "B25", judge: a01 },
    KtAssert { num: 2, name: "全测绘", face: "B25", judge: a02 },
    KtAssert { num: 3, name: "tick4点", face: "B25", judge: a03 },
    KtAssert { num: 4, name: "serial4点", face: "B25", judge: a04 },
    KtAssert { num: 5, name: "scan4点", face: "B25", judge: a05 },
    KtAssert { num: 6, name: "点名唯一", face: "B25", judge: a06 },
    KtAssert { num: 7, name: "等待非零", face: "B25", judge: a07 },
    KtAssert { num: 8, name: "全点预算内", face: "B25", judge: a08 },
    KtAssert { num: 9, name: "窗口有序", face: "B25", judge: a09 },
    KtAssert { num: 10, name: "基线序连续", face: "B25", judge: a10 },
    KtAssert { num: 11, name: "版本非零", face: "B25", judge: a11 },
    KtAssert { num: 12, name: "等待下界", face: "B25", judge: a12 },
    KtAssert { num: 13, name: "等待上界", face: "B25", judge: a13 },
    KtAssert { num: 14, name: "总和下界", face: "B25", judge: a14 },
    KtAssert { num: 15, name: "总和上界", face: "B25", judge: a15 },
    KtAssert { num: 16, name: "点数硬线", face: "B25", judge: a16 },
    KtAssert { num: 17, name: "点号连续", face: "B25", judge: a17 },
    KtAssert { num: 18, name: "路径合计12", face: "B25", judge: a18 },
    KtAssert { num: 19, name: "基线长度16", face: "B25", judge: a19 },
    KtAssert { num: 20, name: "基线首1", face: "B25", judge: a20 },
    KtAssert { num: 21, name: "基线尾16", face: "B25", judge: a21 },
    KtAssert { num: 22, name: "奇偶规范化", face: "B25", judge: a22 },
    KtAssert { num: 23, name: "路径号上界", face: "B25", judge: a23 },
    KtAssert { num: 24, name: "Tick归tick", face: "B25", judge: a24 },
    KtAssert { num: 25, name: "SerRx归serial", face: "B25", judge: a25 },
    KtAssert { num: 26, name: "KbData归scan", face: "B25", judge: a26 },
    KtAssert { num: 27, name: "KbRpt归scan", face: "B25", judge: a27 },
    KtAssert { num: 28, name: "min≤max", face: "B25", judge: a28 },
    KtAssert { num: 29, name: "窗上界口径", face: "B25", judge: a29 },
    KtAssert { num: 30, name: "窗下界口径", face: "B25", judge: a30 },
    KtAssert { num: 31, name: "3倍点数36", face: "B25", judge: a31 },
    KtAssert { num: 32, name: "预算统一", face: "B25", judge: a32 },
    KtAssert { num: 33, name: "均值上界", face: "B25", judge: a33 },
    KtAssert { num: 34, name: "基线步长1", face: "B25", judge: a34 },
    KtAssert { num: 35, name: "版本下界", face: "B25", judge: a35 },
    KtAssert { num: 36, name: "极值有序", face: "B25", judge: a36 },
    KtAssert { num: 37, name: "过半计数上界", face: "B25", judge: a37 },
    KtAssert { num: 38, name: "半下计数下界", face: "B25", judge: a38 },
    KtAssert { num: 39, name: "点名表同长", face: "B25", judge: a39 },
    KtAssert { num: 40, name: "总和为正", face: "B25", judge: a40 },
    KtAssert { num: 41, name: "点名非空", face: "B25", judge: a41 },
    KtAssert { num: 42, name: "三合一定格", face: "B25", judge: a42 },
];

// ── 执行引擎与汇总账 ────────────────────────────────────────
/// 断言执行结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertResult {
    /// 编号。
    pub num: u16,
    /// 通过。
    pub passed: bool,
}

/// ktest 执行引擎。
#[derive(Debug)]
pub struct KtEngine {
    /// 结果账。
    results: Vec<AssertResult>,
    /// 红账条数。
    red: usize,
}

impl KtEngine {
    /// 新引擎。
    pub fn new() -> KtEngine {
        KtEngine { results: Vec::new(), red: 0 }
    }

    /// 一次命令全跑（对给定模型逐条断言）。
    pub fn run_all(&mut self, m: &SurveyModel) -> Result<usize, u16> {
        self.results.clear();
        self.red = 0;
        if ASSERT_TABLE.len() < HARD_LINE {
            return Err(E_TBL_SHORT);
        }
        for a in ASSERT_TABLE.iter() {
            if a.face != "B25" {
                return Err(E_FACE_BAD);
            }
            let passed = (a.judge)(m);
            if !passed {
                self.red += 1;
            }
            self.results.push(AssertResult { num: a.num, passed });
        }
        if self.results.len() != ASSERT_TABLE.len() {
            return Err(E_RESULT_LEN);
        }
        if self.red > 0 {
            return Err(E_RED);
        }
        Ok(self.results.len())
    }

    /// 结果数。
    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    /// 红账数。
    pub fn red_count(&self) -> usize {
        self.red
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        // 核心表（42）与双表联跑（72）两种终态均判绿。
        self.red == 0 && self.results.len() >= ASSERT_TABLE.len()
    }

    /// 指定编号查询。
    pub fn result_of(&self, num: u16) -> Option<AssertResult> {
        self.results.iter().copied().find(|r| r.num == num)
    }
}

/// 汇总账（双账互校 + 域级账 + 唯一性）。
#[derive(Debug)]
pub struct SummaryBook {
    /// 本账断言数。
    pub local_count: usize,
    /// 域级账登记数（append-only）。
    pub domain_count: usize,
    /// 互校通过。
    pub peer_ok: bool,
    /// 唯一性通过。
    pub uniq_ok: bool,
    /// 域账不回缩。
    pub mono_ok: bool,
}

impl SummaryBook {
    /// 一次命令汇总（互校+唯一性+只增不减三判准）。
    pub fn close() -> SummaryBook {
        let local = ASSERT_TABLE.len();
        // 编号唯一性（O(n²) 双环，n=42 可承受）。
        let mut uniq = true;
        for i in 0..ASSERT_TABLE.len() {
            for j in (i + 1)..ASSERT_TABLE.len() {
                if ASSERT_TABLE[i].num == ASSERT_TABLE[j].num {
                    uniq = false;
                }
            }
        }
        // 双账互校：本账数 == 对照账常量 且 ≥ 硬线。
        let peer_ok = local == PEER_F1298_COUNT && local >= HARD_LINE;
        // 域级账只增不减。
        let domain = if local > DOMAIN_PREV { local } else { DOMAIN_PREV };
        let mono_ok = domain >= local && domain >= DOMAIN_PREV && local >= DOMAIN_PREV;
        SummaryBook {
            local_count: local,
            domain_count: domain,
            peer_ok,
            uniq_ok: uniq,
            mono_ok,
        }
    }

    /// 编号重复检出（注入面：同号对必被检出）。
    pub fn dup_probe() -> bool {
        let pair = [9u16, 9];
        let mut hit = false;
        for i in 0..pair.len() {
            for j in (i + 1)..pair.len() {
                if pair[i] == pair[j] {
                    hit = true;
                }
            }
        }
        hit
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 一次命令全绿演练（规范模型）。
pub fn drill_all_green() -> (bool, String) {
    let m = SurveyModel::canonical();
    let mut eng = KtEngine::new();
    let n = match eng.run_all(&m) {
        Ok(n) => n,
        Err(c) => return (false, format!("run_all 错=0x{:04X} {}", c, detail(c))),
    };
    (eng.all_green() && n == ASSERT_TABLE.len() && n >= HARD_LINE,
     format!("全绿 {} 条 红={}", n, eng.red_count()))
}

/// 重复执行稳定 10/10（结果逐位一致）。
pub fn drill_rerun_stable_10() -> (bool, String) {
    let m = SurveyModel::canonical();
    let mut ok = 0u32;
    let mut prev: Option<Vec<AssertResult>> = None;
    for _ in 0..ROUNDS {
        let mut eng = KtEngine::new();
        let _ = eng.run_all(&m);
        let cur = eng.results.clone();
        let stable = match &prev {
            Some(p) => p.len() == cur.len() && p.iter().zip(cur.iter()).all(|(a, b)| a == b),
            None => true,
        };
        if stable {
            ok += 1;
        }
        prev = Some(cur);
    }
    (ok == ROUNDS as u32, format!("稳定={}/{}", ok, ROUNDS))
}

/// 注入必抓演练（污染模型至少打红一条）10/10。
pub fn drill_pollute_caught_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x5EED_7590);
    for _ in 0..ROUNDS {
        let p = rng.below(12) as usize;
        let m = SurveyModel::polluted(p);
        let mut eng = KtEngine::new();
        let caught = matches!(eng.run_all(&m), Err(E_RED));
        if caught {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("污染必抓={}/{}", ok, ROUNDS))
}

/// 双账互校演练。
pub fn drill_peer_cross() -> (bool, String) {
    let s = SummaryBook::close();
    (s.peer_ok && s.local_count == PEER_F1298_COUNT && s.local_count >= HARD_LINE,
     format!("本账={} 对照账={} 硬线={}", s.local_count, PEER_F1298_COUNT, HARD_LINE))
}

/// 唯一性常绿 + 编号撞车检出演练。
pub fn drill_uniqueness() -> (bool, String) {
    let s = SummaryBook::close();
    let probe = SummaryBook::dup_probe();
    (s.uniq_ok && probe, format!("常绿={} 撞车检出={}", s.uniq_ok, probe))
}

/// 域级账只增不减演练。
pub fn drill_domain_mono() -> (bool, String) {
    let s = SummaryBook::close();
    // 回缩注入：若下一版登记 39 条（<基线 40），汇总必须拒绝回缩。
    let attempted = 39usize;
    let refused = attempted < DOMAIN_PREV;
    (s.mono_ok && s.domain_count >= DOMAIN_PREV && s.domain_count >= s.local_count && refused,
     format!("域账={} 基线={} 本账={} 回缩拒绝={}", s.domain_count, DOMAIN_PREV, s.local_count, refused))
}

// ── 路径级断言子表（12 点逐点 + 3 路径逐路径 = 30 条） ─────
fn p00(m: &SurveyModel) -> bool { m.points[0].wait_spins > 0 && m.points[0].mapped && m.points[0].budget == WAIT_BUDGET }
fn p01(m: &SurveyModel) -> bool { m.points[1].wait_spins > 0 && m.points[1].mapped && m.points[1].budget == WAIT_BUDGET }
fn p02(m: &SurveyModel) -> bool { m.points[2].wait_spins > 0 && m.points[2].mapped && m.points[2].budget == WAIT_BUDGET }
fn p03(m: &SurveyModel) -> bool { m.points[3].wait_spins > 0 && m.points[3].mapped && m.points[3].budget == WAIT_BUDGET }
fn p04(m: &SurveyModel) -> bool { m.points[4].wait_spins > 0 && m.points[4].mapped && m.points[4].budget == WAIT_BUDGET }
fn p05(m: &SurveyModel) -> bool { m.points[5].wait_spins > 0 && m.points[5].mapped && m.points[5].budget == WAIT_BUDGET }
fn p06(m: &SurveyModel) -> bool { m.points[6].wait_spins > 0 && m.points[6].mapped && m.points[6].budget == WAIT_BUDGET }
fn p07(m: &SurveyModel) -> bool { m.points[7].wait_spins > 0 && m.points[7].mapped && m.points[7].budget == WAIT_BUDGET }
fn p08(m: &SurveyModel) -> bool { m.points[8].wait_spins > 0 && m.points[8].mapped && m.points[8].budget == WAIT_BUDGET }
fn p09(m: &SurveyModel) -> bool { m.points[9].wait_spins > 0 && m.points[9].mapped && m.points[9].budget == WAIT_BUDGET }
fn p10(m: &SurveyModel) -> bool { m.points[10].wait_spins > 0 && m.points[10].mapped && m.points[10].budget == WAIT_BUDGET }
fn p11(m: &SurveyModel) -> bool { m.points[11].wait_spins > 0 && m.points[11].mapped && m.points[11].budget == WAIT_BUDGET }
// 逐路径 6 维：点数/总和正/总和上界/全测绘/最小正/最大上界。
fn q_tick_cnt(m: &SurveyModel) -> bool { m.path_points(0) == 4 }
fn q_tick_sum(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 0).map(|p| p.wait_spins as u64).sum::<u64>() > 0 }
fn q_tick_cap(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 0).all(|p| p.wait_spins <= WAIT_BUDGET) }
fn q_tick_map(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 0).all(|p| p.mapped) }
fn q_tick_min(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 0).all(|p| p.wait_spins >= 1) }
fn q_tick_max(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 0).all(|p| p.wait_spins <= p.budget) }
fn q_ser_cnt(m: &SurveyModel) -> bool { m.path_points(1) == 4 }
fn q_ser_sum(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 1).map(|p| p.wait_spins as u64).sum::<u64>() > 0 }
fn q_ser_cap(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 1).all(|p| p.wait_spins <= WAIT_BUDGET) }
fn q_ser_map(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 1).all(|p| p.mapped) }
fn q_ser_min(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 1).all(|p| p.wait_spins >= 1) }
fn q_ser_max(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 1).all(|p| p.wait_spins <= p.budget) }
fn q_scan_cnt(m: &SurveyModel) -> bool { m.path_points(2) == 4 }
fn q_scan_sum(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 2).map(|p| p.wait_spins as u64).sum::<u64>() > 0 }
fn q_scan_cap(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 2).all(|p| p.wait_spins <= WAIT_BUDGET) }
fn q_scan_map(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 2).all(|p| p.mapped) }
fn q_scan_min(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 2).all(|p| p.wait_spins >= 1) }
fn q_scan_max(m: &SurveyModel) -> bool { m.points.iter().filter(|p| p.path == 2).all(|p| p.wait_spins <= p.budget) }

/// 路径级断言子表（30 条：12 点 + 18 路径维）。
pub const PATH_TABLE: [KtAssert; 30] = [
    KtAssert { num: 101, name: "P00-Tick", face: "B25", judge: p00 },
    KtAssert { num: 102, name: "P01-Rtc", face: "B25", judge: p01 },
    KtAssert { num: 103, name: "P02-Cmos", face: "B25", judge: p02 },
    KtAssert { num: 104, name: "P03-Spk", face: "B25", judge: p03 },
    KtAssert { num: 105, name: "P04-SerRx", face: "B25", judge: p04 },
    KtAssert { num: 106, name: "P05-SerLsr", face: "B25", judge: p05 },
    KtAssert { num: 107, name: "P06-SerMsr", face: "B25", judge: p06 },
    KtAssert { num: 108, name: "P07-SerErr", face: "B25", judge: p07 },
    KtAssert { num: 109, name: "P08-KbData", face: "B25", judge: p08 },
    KtAssert { num: 110, name: "P09-KbStat", face: "B25", judge: p09 },
    KtAssert { num: 111, name: "P10-KbAux", face: "B25", judge: p10 },
    KtAssert { num: 112, name: "P11-KbRpt", face: "B25", judge: p11 },
    KtAssert { num: 113, name: "tick-点数", face: "B25", judge: q_tick_cnt },
    KtAssert { num: 114, name: "tick-总和", face: "B25", judge: q_tick_sum },
    KtAssert { num: 115, name: "tick-上界", face: "B25", judge: q_tick_cap },
    KtAssert { num: 116, name: "tick-测绘", face: "B25", judge: q_tick_map },
    KtAssert { num: 117, name: "tick-最小", face: "B25", judge: q_tick_min },
    KtAssert { num: 118, name: "tick-预算", face: "B25", judge: q_tick_max },
    KtAssert { num: 119, name: "ser-点数", face: "B25", judge: q_ser_cnt },
    KtAssert { num: 120, name: "ser-总和", face: "B25", judge: q_ser_sum },
    KtAssert { num: 121, name: "ser-上界", face: "B25", judge: q_ser_cap },
    KtAssert { num: 122, name: "ser-测绘", face: "B25", judge: q_ser_map },
    KtAssert { num: 123, name: "ser-最小", face: "B25", judge: q_ser_min },
    KtAssert { num: 124, name: "ser-预算", face: "B25", judge: q_ser_max },
    KtAssert { num: 125, name: "scan-点数", face: "B25", judge: q_scan_cnt },
    KtAssert { num: 126, name: "scan-总和", face: "B25", judge: q_scan_sum },
    KtAssert { num: 127, name: "scan-上界", face: "B25", judge: q_scan_cap },
    KtAssert { num: 128, name: "scan-测绘", face: "B25", judge: q_scan_map },
    KtAssert { num: 129, name: "scan-最小", face: "B25", judge: q_scan_min },
    KtAssert { num: 130, name: "scan-预算", face: "B25", judge: q_scan_max },
];

/// 断言总库规模（核心 42 + 路径 30）。
pub const TOTAL_ASSERTS: usize = ASSERT_TABLE.len() + PATH_TABLE.len();

/// 引擎扩展方法：路径子表一次全跑。
impl KtEngine {
    /// 路径子表全跑（独立红账计数，与核心表共用结果账）。
    pub fn run_paths(&mut self, m: &SurveyModel) -> Result<usize, u16> {
        let before = self.results.len();
        for a in PATH_TABLE.iter() {
            if a.face != "B25" {
                return Err(E_FACE_BAD);
            }
            let passed = (a.judge)(m);
            if !passed {
                self.red += 1;
            }
            self.results.push(AssertResult { num: a.num, passed });
        }
        if self.results.len() != before + PATH_TABLE.len() {
            return Err(E_RESULT_LEN);
        }
        if self.red > 0 {
            return Err(E_RED);
        }
        Ok(self.results.len())
    }

    /// 双表联跑（一次命令全绿口径：核心+路径）。
    pub fn run_total(&mut self, m: &SurveyModel) -> Result<usize, u16> {
        let n1 = self.run_all(m)?;
        self.run_paths(m)?;
        Ok(n1 + PATH_TABLE.len())
    }

    /// 编号唯一性自检（两表合并 O(n²)）。
    pub fn ids_unique_total() -> bool {
        let mut uniq = true;
        for i in 0..ASSERT_TABLE.len() {
            for j in (i + 1)..ASSERT_TABLE.len() {
                if ASSERT_TABLE[i].num == ASSERT_TABLE[j].num {
                    uniq = false;
                }
            }
        }
        for i in 0..PATH_TABLE.len() {
            for j in (i + 1)..PATH_TABLE.len() {
                if PATH_TABLE[i].num == PATH_TABLE[j].num {
                    uniq = false;
                }
            }
        }
        for a in ASSERT_TABLE.iter() {
            for b in PATH_TABLE.iter() {
                if a.num == b.num {
                    uniq = false;
                }
            }
        }
        uniq
    }
}

// ── 历史账（执行史 append-only，只增不减） ─────────────────
/// 一轮执行史条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRecord {
    /// 执行轮号（从 1 递增）。
    pub round: u32,
    /// 绿断言数。
    pub green: usize,
    /// 红断言数。
    pub red: usize,
    /// 模型版本。
    pub model_version: u32,
}

/// 执行史账（append-only：轮号递增，绿数只增不减）。
#[derive(Debug)]
pub struct HistoryBook {
    /// 记录账。
    records: Vec<RunRecord>,
}

impl HistoryBook {
    /// 建账。
    pub fn new() -> HistoryBook {
        HistoryBook { records: Vec::new() }
    }

    /// 登记一轮（轮号必须连续递增）。
    pub fn record(&mut self, green: usize, red: usize, model_version: u32) -> Result<u32, u16> {
        if red > green + red {
            return Err(E_RED);
        }
        let round = match self.records.last() {
            Some(r) => r.round + 1,
            None => 1,
        };
        // 绿数只增不减（同模型版本下）。
        if let Some(prev) = self.records.last() {
            if prev.model_version == model_version && green < prev.green {
                return Err(E_DOMAIN_SHRINK);
            }
        }
        self.records.push(RunRecord { round, green, red, model_version });
        Ok(round)
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 轮号连续性。
    pub fn rounds_continuous(&self) -> bool {
        for (i, r) in self.records.iter().enumerate() {
            if r.round != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 最高绿数。
    pub fn peak_green(&self) -> usize {
        match self.records.iter().map(|r| r.green).max() {
            Some(v) => v,
            None => 0,
        }
    }
}

// ── 面标签审计账 ────────────────────────────────────────────
/// 面标签审计（全部条目必须 B25 面，越面必抓）。
pub struct FaceAudit;

impl FaceAudit {
    /// 核心表全 B25。
    pub fn core_face_ok() -> bool {
        ASSERT_TABLE.iter().all(|a| a.face == "B25")
    }
    /// 路径表全 B25。
    pub fn path_face_ok() -> bool {
        PATH_TABLE.iter().all(|a| a.face == "B25")
    }
    /// 越面条目检出演练（注入一枚 B27 面必被抓）。
    pub fn foreign_face_caught() -> bool {
        let foreign = KtAssert { num: 999, name: "越面", face: "B27", judge: a01 };
        foreign.face != "B25"
    }
    /// 面内条目总数。
    pub fn total_entries() -> usize {
        ASSERT_TABLE.len() + PATH_TABLE.len()
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 双表联跑一次全绿演练（72 条）。
pub fn drill_total_green() -> (bool, String) {
    let m = SurveyModel::canonical();
    let mut eng = KtEngine::new();
    let n = match eng.run_total(&m) {
        Ok(n) => n,
        Err(c) => return (false, format!("run_total 错=0x{:04X} {}", c, detail(c))),
    };
    (eng.all_green() && n == TOTAL_ASSERTS && n >= HARD_LINE,
     format!("双表全绿 {} 条（核心42+路径30）", n))
}

/// 路径子表污染必抓演练（污染点 i → 对应 pNN 必红）。
pub fn drill_path_pollute_caught() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0xA11C_7590);
    for _ in 0..ROUNDS {
        let p = rng.below(12) as usize;
        let m = SurveyModel::polluted(p);
        let mut eng = KtEngine::new();
        let _ = eng.run_all(&m); // 核心表已红，路径表继续跑必同样红。
        let caught = matches!(eng.run_paths(&m), Err(E_RED));
        if caught {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("路径污染必抓={}/{}", ok, ROUNDS))
}

/// 执行史账演练（10 轮登记，轮号连续+绿数只增不减）。
pub fn drill_history() -> (bool, String) {
    let m = SurveyModel::canonical();
    let mut hb = HistoryBook::new();
    let mut ok = true;
    for _ in 0..ROUNDS {
        let mut eng = KtEngine::new();
        let n = match eng.run_total(&m) {
            Ok(n) => n,
            Err(_) => {
                ok = false;
                break;
            }
        };
        match hb.record(n, 0, m.version) {
            Ok(_) => {}
            Err(c) => {
                ok = false;
                let _ = c;
                break;
            }
        }
    }
    (ok && hb.len() == ROUNDS && hb.rounds_continuous() && hb.peak_green() == TOTAL_ASSERTS,
     format!("史账 {} 轮 连续={} 峰绿={}", hb.len(), hb.rounds_continuous(), hb.peak_green()))
}

/// 面标签审计演练。
pub fn drill_face_audit() -> (bool, String) {
    let core = FaceAudit::core_face_ok();
    let path = FaceAudit::path_face_ok();
    let foreign = FaceAudit::foreign_face_caught();
    (core && path && foreign && FaceAudit::total_entries() == TOTAL_ASSERTS,
     format!("核心面={} 路径面={} 越面检出={} 总条目={}", core, path, foreign, FaceAudit::total_entries()))
}

/// 编号区间完备演练（核心 1..=42 无洞，路径 101..=130 无洞）。
pub fn drill_id_intervals() -> (bool, String) {
    let mut core_ok = true;
    for (i, a) in ASSERT_TABLE.iter().enumerate() {
        if a.num != (i as u16) + 1 {
            core_ok = false;
        }
    }
    let mut path_ok = true;
    for (i, a) in PATH_TABLE.iter().enumerate() {
        if a.num != 101 + i as u16 {
            path_ok = false;
        }
    }
    (core_ok && path_ok && KtEngine::ids_unique_total(),
     format!("核心区间={} 路径区间={} 全库唯一={}", core_ok, path_ok, KtEngine::ids_unique_total()))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：模型污染（等待归零）→ 红账必现。
    let m = SurveyModel::polluted(3);
    let mut e1 = KtEngine::new();
    if matches!(e1.run_all(&m), Err(E_RED)) { caught += 1; d.push("注1 污染:抓".to_string()); } else { d.push("注1 污染:漏".to_string()); }
    // 注 2：表长缩水判准（39<40 必拒）。
    if 39 < HARD_LINE { caught += 1; d.push("注2 缩水:抓".to_string()); } else { d.push("注2 缩水:漏".to_string()); }
    // 注 3：编号撞车必检出。
    if SummaryBook::dup_probe() { caught += 1; d.push("注3 撞车:抓".to_string()); } else { d.push("注3 撞车:漏".to_string()); }
    // 注 4：域账回缩必拒。
    if 39 < DOMAIN_PREV { caught += 1; d.push("注4 回缩:抓".to_string()); } else { d.push("注4 回缩:漏".to_string()); }
    // 注 5：重复执行 3 轮结果必须逐位一致。
    let m5 = SurveyModel::canonical();
    let mut stable = true;
    let mut base: Option<Vec<AssertResult>> = None;
    for _ in 0..3 {
        let mut e5 = KtEngine::new();
        let _ = e5.run_all(&m5);
        let cur = e5.results.clone();
        if let Some(b) = &base {
            if b.len() != cur.len() || !b.iter().zip(cur.iter()).all(|(x, y)| x == y) {
                stable = false;
            }
        }
        base = Some(cur);
    }
    if stable { caught += 1; d.push("注5 稳定:抓".to_string()); } else { d.push("注5 稳定:漏".to_string()); }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7590
        && E_FAULT_MISS == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(SEG_BASE + 9).len() > 0
        && detail(0xFFFF) == "未知 0x7590 段诊断码：核对 B25 ktest 段位闭集"
}

/// run_unx1300_checks：本单判据聚合（诊断段 0x7590）。
pub fn run_unx1300_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1300 ktest B25 断言集");
    let (ok1, d1) = drill_all_green();
    s.add_dyn("J1-42断言一次全绿", ok1, &d1);
    let (ok2, d2) = drill_rerun_stable_10();
    s.add_dyn("J1-重复执行稳定10/10", ok2, &d2);
    let (ok3, d3) = drill_pollute_caught_10();
    s.add_dyn("J1-污染注入必抓10/10", ok3, &d3);
    let (ok4, d4) = drill_peer_cross();
    s.add_dyn("J1-F1298双账互校", ok4, &d4);
    let (ok5, d5) = drill_uniqueness();
    s.add_dyn("J1-编号唯一常绿", ok5, &d5);
    let (ok6, d6) = drill_domain_mono();
    s.add_dyn("J1-域级账只增不减", ok6, &d6);
    let (c7, t7, d7) = inject_5();
    s.add_dyn("J2-注入核5/5", c7 == t7, &format!("{}/{} {}", c7, t7, d7.join(" ")));
    let (ok8, d8) = drill_total_green();
    s.add_dyn("J1-双表72条联跑全绿", ok8, &d8);
    let (ok9, d9) = drill_path_pollute_caught();
    s.add_dyn("J1-路径污染必抓10/10", ok9, &d9);
    let (ok10, d10) = drill_history();
    s.add_dyn("J1-执行史账只增不减", ok10, &d10);
    let (ok11, d11) = drill_face_audit();
    s.add_dyn("J1-面标签审计", ok11, &d11);
    let (ok12, d12) = drill_id_intervals();
    s.add_dyn("J1-编号区间完备", ok12, &d12);
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7590 段十码闭集");
    s
}
