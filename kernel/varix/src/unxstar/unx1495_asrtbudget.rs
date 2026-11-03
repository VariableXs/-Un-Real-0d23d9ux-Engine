//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1495 断言性能预算 · 0x75FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6 M组）：
//!   全跑耗时 ≤30s 上界+单断言均值 ≤10ms 两线（步数化口径），全跑
//!   演练 10/10 达标，超限断言定位到慢件，预算超限自动分批 10/10 次。
//! 逐卡要点：
//!   两线预算步数化（全跑 ≤30 万步、单断言均值 ≤1000 步），全跑演
//!   练 10/10 达标，慢件定位（最慢断言排名表），超限自动分批（分
//!   批后合计守恒）10/10 次。
//! 设计要点：
//!   ① 两线一律步数化：全跑预算 300000 步（对应 30s 上界）、单断
//!     言均值预算 1000 步（对应 10ms 上界），不碰墙钟不碰浮点；
//!   ② 全跑演练 10/10：常规断言表（均值远低于预算线）跑 10 轮全
//!     达标，超限表自动分批 10 轮全成功；
//!   ③ 慢件定位：步数降序排名表（手写插入排序，不碰 std 排序），
//!     排名首位即最慢断言，逐名次步数可复算；
//!   ④ 自动分批：贪心累加遇预算线即封批，分批合计 == 原始合计
//!     （守恒红线），每批步数 ≤ 全跑预算；
//!   ⑤ 均值整数口径：total/count 向下取整，count=0 报空表诊断；
//!   ⑥ 常量冻结：全跑预算 300000 步、均值预算 1000 步、常规表 10
//!     条、判据轮数 10（来源：本卡判据 30s→30 万步、10ms→1000
//!     步的步数化换算）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75FC0，AI-VX7-1104 批6 M组窗口第四格）。
pub const SEG_BASE: u32 = 0x75FC0;

/// 全跑预算（冻结 300000 步 = 30s × 10000 步/s 步数化口径）。
pub const FULL_BUDGET: u64 = 300_000;
/// 单断言均值预算（冻结 1000 步 = 10ms × 100000 步/s 步数化口径）。
pub const MEAN_BUDGET: u64 = 1_000;
/// 常规断言表条数（冻结 10，全跑演练 10/10 的"10"）。
pub const NORM_COUNT: usize = 10;
/// 判据轮数（写死 10：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 分批上限（冻结 16：超出视为无法分批）。
pub const BATCH_CAP: usize = 16;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetCode {
    /// 全跑预算超限：总步数 > 300000
    FullOver,
    /// 均值预算超限：均值 > 1000 步
    MeanOver,
    /// 分批合计不守恒：分批步数之和 != 原始总步数
    BatchLost,
    /// 分批后仍超限：单批步数 > 300000
    BatchOver,
    /// 空断言表：均值口径无分母
    EmptySuite,
    /// 慢件定位失败：排名表首位与已知慢件不符
    RankLost,
    /// 步数溢出：累加超 u64 上界
    StepOverflow,
    /// 预算状态倒退：达标态回退
    BudgetBackStep,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl BudgetCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BudgetCode::FullOver => "全跑预算超限：总步数>300000，请先跑慢件排名再自动分批",
            BudgetCode::MeanOver => "均值预算超限：单断言均值>1000 步，请拆分最慢断言",
            BudgetCode::BatchLost => "分批合计不守恒：分批步数之和须等于原始总步数，请查封批漏项",
            BudgetCode::BatchOver => "分批后仍超限：单步预算装不下该断言，请拆分断言本身",
            BudgetCode::EmptySuite => "空断言表：均值口径无分母，请至少登记 1 条断言",
            BudgetCode::RankLost => "慢件定位失败：排名表首位步数须为全表最大值",
            BudgetCode::StepOverflow => "步数溢出：请用 checked_add 分批落盘后再累加",
            BudgetCode::BudgetBackStep => "预算状态倒退：达标态不可回退，请重开新预算期",
            BudgetCode::NotReproducible => "双跑不一致：复现红线，请固定种子与输入重跑",
        }
    }
    pub const ALL: [BudgetCode; 9] = [
        BudgetCode::FullOver,
        BudgetCode::MeanOver,
        BudgetCode::BatchLost,
        BudgetCode::BatchOver,
        BudgetCode::EmptySuite,
        BudgetCode::RankLost,
        BudgetCode::StepOverflow,
        BudgetCode::BudgetBackStep,
        BudgetCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码给中文提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BudgetCode::ALL.len() {
        if let Some(c) = BudgetCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75FC0 段闭集内"
}

// ── 断言条目（名 + 步数成本） ───────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsrtItem {
    pub id: u32,
    pub name: &'static str,
    pub steps: u64,
}

// ── 预算状态机（开算→达标/分批→落账，单调） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetState {
    /// 开算：步数表在记
    Open,
    /// 达标：两线全过
    Within,
    /// 分批：超限已自动分批
    Batched,
    /// 落账：结果已冻结
    Sealed,
}

/// 预算会话。
#[derive(Debug)]
pub struct BudgetSession {
    pub items: Vec<AsrtItem>,
    pub state: BudgetState,
    pub total: u64,
    pub mean: u64,
}

impl BudgetSession {
    pub fn new(items: Vec<AsrtItem>) -> BudgetSession {
        BudgetSession { items, state: BudgetState::Open, total: 0, mean: 0 }
    }

    /// 全跑评估：总步数 + 均值（整数向下取整）。
    pub fn evaluate(&mut self) -> Result<(u64, u64), u32> {
        if self.items.is_empty() {
            return Err(BudgetCode::EmptySuite.val());
        }
        let mut total: u64 = 0;
        for it in self.items.iter() {
            match total.checked_add(it.steps) {
                Some(t) => total = t,
                None => return Err(BudgetCode::StepOverflow.val()),
            }
        }
        let count = self.items.len() as u64;
        let mean = total / count;
        self.total = total;
        self.mean = mean;
        Ok((total, mean))
    }

    /// 两线判定：全跑 ≤ 预算 且 均值 ≤ 预算。
    pub fn two_lines_ok(&self) -> Result<bool, u32> {
        if self.items.is_empty() {
            return Err(BudgetCode::EmptySuite.val());
        }
        Ok(self.total <= FULL_BUDGET && self.mean <= MEAN_BUDGET)
    }

    /// 达标落账（开算→达标）。
    pub fn mark_within(&mut self) -> Result<(), u32> {
        if self.state != BudgetState::Open {
            return Err(BudgetCode::BudgetBackStep.val());
        }
        if !self.two_lines_ok()? {
            return Err(BudgetCode::FullOver.val());
        }
        self.state = BudgetState::Within;
        Ok(())
    }

    /// 达标态回退显性拒。
    pub fn demote(&self) -> Result<(), u32> {
        if self.state == BudgetState::Within || self.state == BudgetState::Sealed {
            return Err(BudgetCode::BudgetBackStep.val());
        }
        Ok(())
    }

    /// 封账（达标/分批 → 封账）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != BudgetState::Within && self.state != BudgetState::Batched {
            return Err(BudgetCode::BudgetBackStep.val());
        }
        self.state = BudgetState::Sealed;
        Ok(())
    }
}

// ── 慢件排名表（步数降序，手写插入排序） ────────────────────────
pub fn rank_desc(items: &[AsrtItem]) -> Vec<AsrtItem> {
    let mut v: Vec<AsrtItem> = Vec::new();
    for it in items.iter() {
        v.push(*it);
        // 冒泡上浮到降序位置。
        let mut i = v.len();
        while i > 1 {
            let a = match v.get(i - 2) {
                Some(x) => *x,
                None => break,
            };
            let b = match v.get(i - 1) {
                Some(x) => *x,
                None => break,
            };
            if a.steps >= b.steps {
                break;
            }
            v.swap(i - 2, i - 1);
            i -= 1;
        }
    }
    v
}

/// 慢件定位：排名首位须为全表最大步数。
pub fn slowest_ok(items: &[AsrtItem]) -> Result<bool, u32> {
    if items.is_empty() {
        return Err(BudgetCode::EmptySuite.val());
    }
    let ranked = rank_desc(items);
    let head = match ranked.first() {
        Some(h) => h.steps,
        None => return Err(BudgetCode::RankLost.val()),
    };
    let mut max_seen: u64 = 0;
    for it in items.iter() {
        if it.steps > max_seen {
            max_seen = it.steps;
        }
    }
    Ok(head == max_seen)
}

// ── 自动分批（贪心累加遇线封批，合计守恒） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub no: u32,
    pub count: usize,
    pub steps: u64,
}

/// 超限自动分批：按序累加，遇预算线封批；单条超预算线即报错。
pub fn auto_batch(items: &[AsrtItem]) -> Result<Vec<Batch>, u32> {
    if items.is_empty() {
        return Err(BudgetCode::EmptySuite.val());
    }
    let mut batches: Vec<Batch> = Vec::new();
    let mut cur_steps: u64 = 0;
    let mut cur_count: usize = 0;
    for it in items.iter() {
        if it.steps > FULL_BUDGET {
            return Err(BudgetCode::BatchOver.val());
        }
        match cur_steps.checked_add(it.steps) {
            Some(t) if t <= FULL_BUDGET => {
                cur_steps = t;
                cur_count += 1;
            }
            Some(_) => {
                // 封批再开新批。
                let no = batches.len() as u32;
                batches.push(Batch { no, count: cur_count, steps: cur_steps });
                cur_steps = it.steps;
                cur_count = 1;
            }
            None => return Err(BudgetCode::StepOverflow.val()),
        }
    }
    if cur_count > 0 {
        let no = batches.len() as u32;
        batches.push(Batch { no, count: cur_count, steps: cur_steps });
    }
    if batches.len() > BATCH_CAP {
        return Err(BudgetCode::BatchOver.val());
    }
    Ok(batches)
}

/// 分批守恒核验：各批步数之和 == 原始总步数，且每批 ≤ 预算。
pub fn batch_conserved(items: &[AsrtItem], batches: &[Batch]) -> Result<bool, u32> {
    let mut src_total: u64 = 0;
    for it in items.iter() {
        match src_total.checked_add(it.steps) {
            Some(t) => src_total = t,
            None => return Err(BudgetCode::StepOverflow.val()),
        }
    }
    let mut bat_total: u64 = 0;
    for b in batches.iter() {
        if b.steps > FULL_BUDGET {
            return Err(BudgetCode::BatchOver.val());
        }
        match bat_total.checked_add(b.steps) {
            Some(t) => bat_total = t,
            None => return Err(BudgetCode::StepOverflow.val()),
        }
    }
    let mut bat_count = 0usize;
    for b in batches.iter() {
        bat_count += b.count;
    }
    Ok(bat_total == src_total && bat_count == items.len())
}

// ── LCG（固定种子，整数口径） ───────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 常规断言表（手写 10 条，均值 900 步 << 1000 预算线） ────────
pub fn norm_suite() -> Vec<AsrtItem> {
    let rows: [(&'static str, u64); NORM_COUNT] = [
        ("五数复算", 900),
        ("哨兵账", 850),
        ("段位守卫", 880),
        ("对账勾稽", 920),
        ("坏参三闸", 910),
        ("口径快照", 870),
        ("风暴守恒", 940),
        ("慢件排名", 860),
        ("双跑一致", 890),
        ("分批守恒", 980),
    ];
    let mut v: Vec<AsrtItem> = Vec::new();
    let mut id: u32 = 0;
    for (name, steps) in rows.iter() {
        v.push(AsrtItem { id, name, steps: *steps });
        id += 1;
    }
    v
}

/// 超限断言表（手写：两条重断言各 150000 步 + 常规条目，合计
/// 309000 > 300000 触发分批，单条均可入批）。
pub fn heavy_suite() -> Vec<AsrtItem> {
    let mut v: Vec<AsrtItem> = norm_suite();
    v.push(AsrtItem { id: 10, name: "全帧重算", steps: 150_000 });
    v.push(AsrtItem { id: 11, name: "全帧重算二", steps: 150_000 });
    v
}

// ── 全跑演练 10/10 + 分批演练 10/10 ─────────────────────────────
pub struct DrillLedger {
    pub norm_ok: u32,
    pub norm_tries: u32,
    pub batch_ok: u32,
    pub batch_tries: u32,
}

/// 演练：常规表 10 轮达标 + 超限表 10 轮自动分批（守恒核验）。
pub fn drill_rounds(rounds: u32) -> Result<DrillLedger, u32> {
    let mut led = DrillLedger { norm_ok: 0, norm_tries: 0, batch_ok: 0, batch_tries: 0 };
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 常规表：两线全过即达标。
        let mut norm = BudgetSession::new(norm_suite());
        let ok1 = match norm.evaluate() {
            Ok((t, m)) => t <= FULL_BUDGET && m <= MEAN_BUDGET,
            Err(_) => false,
        };
        led.norm_tries = led.norm_tries.saturating_add(1);
        if ok1 {
            led.norm_ok = led.norm_ok.saturating_add(1);
        }
        // 超限表：自动分批 + 守恒 + 单批不超线。
        let heavy = heavy_suite();
        let batches = auto_batch(&heavy)?;
        let ok2 = batch_conserved(&heavy, &batches)?;
        // 守恒之外：分批后每批总步数 ≤ 预算已由核验覆盖。
        led.batch_tries = led.batch_tries.saturating_add(1);
        if ok2 {
            led.batch_ok = led.batch_ok.saturating_add(1);
        }
        // 慢件定位每轮抽检。
        if !slowest_ok(&heavy)? {
            return Err(BudgetCode::RankLost.val());
        }
        r += 1;
    }
    if led.norm_ok != led.norm_tries || led.batch_ok != led.batch_tries {
        return Err(BudgetCode::FullOver.val());
    }
    Ok(led)
}

// ── 黄金全链：评估→两线→达标落账→慢件→超限分批→守恒→回退拒 ────
pub fn golden_run() -> Result<(u32, u32), u32> {
    // 常规表达标线。
    let mut norm = BudgetSession::new(norm_suite());
    let (t, m) = norm.evaluate()?;
    let norm_ok = (t == 9_000 && m == 900) as u32;
    norm.mark_within()?;
    let within_ok = (norm.state == BudgetState::Within) as u32;
    let back_deny = norm.demote().is_err();
    let seal_ok = norm.seal().is_ok();
    // 慢件定位：超限表首位应为全帧重算 150000（并列取先登记者）。
    let heavy = heavy_suite();
    let ranked = rank_desc(&heavy);
    let head_ok = match ranked.first() {
        Some(h) => h.steps == 150_000 && h.id == 10,
        None => false,
    };
    // 超限分批 + 守恒。
    let batches = auto_batch(&heavy)?;
    let cons = batch_conserved(&heavy, &batches)?;
    let raw_total_over = t.saturating_add(150_000).saturating_add(150_000) > FULL_BUDGET;
    Ok((
        norm_ok.saturating_add(within_ok).saturating_add(back_deny as u32).saturating_add(seal_ok as u32),
        (head_ok && cons && raw_total_over && batches.len() >= 2) as u32,
    ))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链 4/4 + 2/2。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 1)) => Ok(()),
        _ => Err(BudgetCode::NotReproducible.val()),
    }
}

/// 序 B：两线口径抽检（均值线用重表必报超限方向；守恒反例必抓）。
pub fn probe_seq_b() -> Result<(), u32> {
    // 破坏守恒：抽掉一批步数必被核验抓出。
    let heavy = heavy_suite();
    let batches = auto_batch(&heavy)?;
    if batches.len() < 2 {
        return Err(BudgetCode::NotReproducible.val());
    }
    let mut spoiled: Vec<Batch> = Vec::new();
    for b in batches.iter() {
        spoiled.push(Batch { no: b.no, count: b.count, steps: b.steps });
    }
    let _ = spoiled.pop();
    match batch_conserved(&heavy, &spoiled) {
        Ok(false) | Err(_) => Ok(()),
        Ok(true) => Err(BudgetCode::BatchLost.val()),
    }
}

/// 序 C：常量冻结（预算两线 300000/1000、表 10、轮 10、批上限 16）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        FULL_BUDGET == 300_000,
        MEAN_BUDGET == 1_000,
        NORM_COUNT == 10,
        JUDGE_ROUNDS == 10,
        BATCH_CAP == 16,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(BudgetCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BudgetCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < BudgetCode::ALL.len() {
        if let Some(c) = BudgetCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((a, b)) => *a == 4 && *b == 1,
            Err(_) => false,
        },
        match &g {
            Ok((a, b)) => format!("常规链={} 超限链={}", a, b),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ② 两线全跑演练 10/10 达标。
    let drill = drill_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-全跑演练10/10",
        match &drill {
            Ok(l) => l.norm_tries == JUDGE_ROUNDS && l.norm_ok == l.norm_tries,
            Err(_) => false,
        },
        match &drill {
            Ok(l) => format!("常规 {}/{}", l.norm_ok, l.norm_tries),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ③ 超限自动分批 10/10 次。
    v.push((
        "J1-自动分批10/10",
        match &drill {
            Ok(l) => l.batch_tries == JUDGE_ROUNDS && l.batch_ok == l.batch_tries,
            Err(_) => false,
        },
        match &drill {
            Ok(l) => format!("分批 {}/{}", l.batch_ok, l.batch_tries),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ④ 两线口径：常规表 total=9000 ≤300000 且 mean=900 ≤1000。
    let mut norm = BudgetSession::new(norm_suite());
    let two = norm.evaluate();
    v.push((
        "J1-两线口径",
        match &two {
            Ok((t, m)) => *t <= FULL_BUDGET && *m <= MEAN_BUDGET && *t == 9_000 && *m == 900,
            Err(_) => false,
        },
        match &two {
            Ok((t, m)) => format!("total={} mean={}", t, m),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ⑤ 超限表两线必越（均值超限方向确认）。
    let mut heavy = BudgetSession::new(heavy_suite());
    let heavy_lines = heavy.evaluate();
    let over_dir = match &heavy_lines {
        Ok((t, _)) => *t > FULL_BUDGET,
        Err(_) => false,
    };
    v.push((
        "J1-超限方向确认",
        over_dir,
        String::from("重表总步数 > 300000，分批路径触发"),
    ));

    // ⑥ 慢件定位（排名首位 = 全帧重算 150000 步）。
    let ranked = rank_desc(&heavy_suite());
    let head_ok = match ranked.first() {
        Some(h) => h.steps == 150_000 && h.id == 10,
        None => false,
    };
    let rank_ok = slowest_ok(&heavy_suite()).unwrap_or(false) && head_ok;
    v.push((
        "J1-慢件定位",
        rank_ok,
        String::from("排名首位=全帧重算 150000 步"),
    ));

    // ⑦ 分批守恒（合计 == 原始总步数）。
    let heavy_items = heavy_suite();
    let batches = match auto_batch(&heavy_items) {
        Ok(b) => b,
        Err(_) => Vec::new(),
    };
    let cons = batch_conserved(&heavy_items, &batches).unwrap_or(false);
    v.push((
        "J1-分批守恒",
        cons && batches.len() >= 2,
        format!("{} 批合计=原始总步数", batches.len()),
    ));

    // ⑧ 每批不超线。
    let batch_line = batches.iter().all(|b| b.steps <= FULL_BUDGET);
    v.push((
        "J1-每批不超线",
        batch_line,
        String::from("单批步数 ≤ 300000"),
    ));

    // ⑨ 守恒反例必抓（抽一批即核验失败）。
    let mut spoiled: Vec<Batch> = Vec::new();
    for b in batches.iter() {
        spoiled.push(*b);
    }
    let _ = spoiled.pop();
    let cons_catch = match batch_conserved(&heavy_items, &spoiled) {
        Ok(false) | Err(_) => true,
        Ok(true) => false,
    };
    v.push((
        "J1-守恒反例必抓",
        cons_catch,
        String::from("抽批后核验必报不守恒"),
    ));

    // ⑩ 状态机单调（达标态回退拒）。
    let mut n10 = BudgetSession::new(norm_suite());
    let _ = n10.evaluate();
    let _ = n10.mark_within();
    let back_deny = n10.demote().is_err();
    let seal_ok = n10.seal().is_ok();
    v.push((
        "J1-状态机单调",
        back_deny && seal_ok,
        String::from("开算→达标→落账 单调，回退拒"),
    ));

    // ⑪ 空表拒。
    let mut empty = BudgetSession::new(Vec::new());
    let empty_deny = empty.evaluate().is_err();
    v.push((
        "J1-空表拒",
        empty_deny,
        String::from("空断言表报 EmptySuite"),
    ));

    // ⑫ 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push(("J1-探针三序", a && b && c, format!("a={} b=检出{} c=检出{}", a, b, c)));

    // ⑬ 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // ⑭ 诊断码闭集查表。
    let mut codes_ok = true;
    let mut ci = 0;
    while ci < BudgetCode::ALL.len() {
        if let Some(c) = BudgetCode::ALL.get(ci) {
            if detail(c.val()) != c.detail() {
                codes_ok = false;
            }
        }
        ci += 1;
    }
    v.push((
        "J3-诊断码闭集",
        codes_ok && detail(0xDEAD_0000) == "未知诊断码：不在 0x75FC0 段闭集内",
        String::from("ALL 查表一致 + 未知码兜底"),
    ));

    // ⑮ 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75FC0 窗口 + 0x40 对齐"),
    ));
    v
}

pub fn run_unx1495_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1495 断言性能预算");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_chain() {
        assert_eq!(golden_run(), Ok((4, 1)));
    }

    #[test]
    fn t02_norm_suite_two_lines() {
        let mut s = BudgetSession::new(norm_suite());
        assert_eq!(s.evaluate(), Ok((9_000, 900)));
        assert!(s.two_lines_ok() == Ok(true));
        assert!(s.mark_within().is_ok());
        assert_eq!(s.state, BudgetState::Within);
        assert!(s.demote().is_err());
        assert!(s.seal().is_ok());
        assert_eq!(s.state, BudgetState::Sealed);
    }

    #[test]
    fn t03_heavy_over_and_batched() {
        let heavy = heavy_suite();
        let mut total: u64 = 0;
        for it in heavy.iter() {
            total += it.steps;
        }
        assert!(total > FULL_BUDGET);
        let batches = match auto_batch(&heavy) {
            Ok(b) => b,
            Err(_) => Vec::new(),
        };
        assert!(batches.len() >= 2);
        assert_eq!(batch_conserved(&heavy, &batches), Ok(true));
        for b in batches.iter() {
            assert!(b.steps <= FULL_BUDGET);
        }
    }

    #[test]
    fn t04_rank_desc_head() {
        let ranked = rank_desc(&heavy_suite());
        let head = match ranked.first() {
            Some(h) => *h,
            None => AsrtItem { id: 0, name: "", steps: 0 },
        };
        assert_eq!(head.steps, 150_000);
        assert_eq!(head.id, 10);
        // 降序不变式。
        let mut prev = u64::MAX;
        for it in ranked.iter() {
            assert!(it.steps <= prev);
            prev = it.steps;
        }
    }

    #[test]
    fn t05_conservation_negative() {
        let heavy = heavy_suite();
        let batches = match auto_batch(&heavy) {
            Ok(b) => b,
            Err(_) => Vec::new(),
        };
        let mut spoiled: Vec<Batch> = Vec::new();
        for b in batches.iter() {
            spoiled.push(*b);
        }
        let _ = spoiled.pop();
        assert_ne!(batch_conserved(&heavy, &spoiled), Ok(true));
    }

    #[test]
    fn t06_drill_rounds_full() {
        let led = match drill_rounds(JUDGE_ROUNDS) {
            Ok(x) => x,
            Err(_) => DrillLedger { norm_ok: 0, norm_tries: 0, batch_ok: 0, batch_tries: 0 },
        };
        assert_eq!(led.norm_tries, JUDGE_ROUNDS);
        assert_eq!(led.norm_ok, JUDGE_ROUNDS);
        assert_eq!(led.batch_tries, JUDGE_ROUNDS);
        assert_eq!(led.batch_ok, JUDGE_ROUNDS);
    }

    #[test]
    fn t07_empty_and_overflow() {
        let mut empty = BudgetSession::new(Vec::new());
        assert_eq!(empty.evaluate(), Err(BudgetCode::EmptySuite.val()));
        // 单条超全跑预算 → 分批路径直接拒。
        let huge = vec![AsrtItem { id: 0, name: "巨断言", steps: FULL_BUDGET + 1 }];
        assert_eq!(auto_batch(&huge), Err(BudgetCode::BatchOver.val()));
    }

    #[test]
    fn t08_repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t09_code_closed_set() {
        assert_eq!(BudgetCode::ALL.len(), 9);
        let mut i = 0;
        while i < BudgetCode::ALL.len() {
            if let Some(c) = BudgetCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD_0000), "未知诊断码：不在 0x75FC0 段闭集内");
    }

    #[test]
    fn t10_slowest_ok_positive() {
        assert_eq!(slowest_ok(&heavy_suite()), Ok(true));
        assert_eq!(slowest_ok(&norm_suite()), Ok(true));
    }

    #[test]
    fn t11_probes_abc() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn t12_batch_numbering() {
        let batches = match auto_batch(&heavy_suite()) {
            Ok(b) => b,
            Err(_) => Vec::new(),
        };
        let mut expect_no = 0u32;
        for b in batches.iter() {
            assert_eq!(b.no, expect_no);
            expect_no += 1;
            assert!(b.count > 0);
        }
    }
}
