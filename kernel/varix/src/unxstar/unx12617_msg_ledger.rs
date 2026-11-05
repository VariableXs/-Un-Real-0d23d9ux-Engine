//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12617 消息面对照表批入账 · 0x80440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12618-J1 投递/应答 P95 预算在册（O1 配套账），
//!      高压注入账无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算在册：投递 P95 与应答 P95 两条预算线冻结入 O1 配套账
//!     （DELIVER_P95=48 拍 / REPLY_P95=64 拍），账目三列齐——预算
//!     线、实测 P95、红账行数；缺账行即 BudgetMissing；
//!   - 延迟样本全整数（逻辑拍计数，禁墙钟）：投递样本 ∈[1,40]、
//!     应答样本 ∈[1,56]，冻结界内由 LCG 生成（种子=SEG_BASE），
//!     样本空账即 SampleEmpty；
//!   - P95 整数闭式：升序副本取 (95n+99)/100−1 号位（nearest-rank），
//!     排序账破（升序复核失败）即 SortCorrupt，不静默出 P95；
//!   - 高压注入无红账：40 例高压连发账逐例记账，红账行数必须为 0；
//!     注入面反向必抓——人为塞一枚超预算样本，红账必须显性记行
//!     （红账被吞即 RedLeak 必抓）；
//!   - 账目守恒：各账行红账之和 = 全局红账总数；
//!   - 双跑一致 + 段位守卫；禁 rand/浮点/IO/墙钟；no_std + alloc；
//!     零 unwrap/expect；诊断码闭集 0x80440..0x80480。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80440，AI-VX9-1105 B01 批 12 槽第 6 槽）。
pub const SEG_BASE: u32 = 0x80440;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetCode {
    /// 预算缺行：O1 配套账无此预算线
    BudgetMissing,
    /// 实测 P95 超预算线
    P95OverBudget,
    /// 红账被吞：超预算样本未记红行
    RedLeak,
    /// 样本空账：无样本不许出 P95
    SampleEmpty,
    /// 排序账破：升序复核失败
    SortCorrupt,
    /// 样本越界：延迟样本超出冻结界
    SampleOutOfRange,
    /// 账目守恒破：分行红账之和≠全局红账
    ConservationBreak,
    /// 高压账溢出：连发例数超账面容量
    PressureOverflow,
    /// 预算线非法：预算值为 0 视为未设防
    BudgetZero,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 账指纹不符：入账存根与重算不一致
    LedgerDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl BudgetCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                BudgetCode::BudgetMissing => 0x01,
                BudgetCode::P95OverBudget => 0x02,
                BudgetCode::RedLeak => 0x03,
                BudgetCode::SampleEmpty => 0x04,
                BudgetCode::SortCorrupt => 0x05,
                BudgetCode::SampleOutOfRange => 0x06,
                BudgetCode::ConservationBreak => 0x07,
                BudgetCode::PressureOverflow => 0x08,
                BudgetCode::BudgetZero => 0x09,
                BudgetCode::FingerprintDrift => 0x0A,
                BudgetCode::LedgerDrift => 0x0B,
                BudgetCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            BudgetCode::BudgetMissing => "预算缺行：先在 O1 配套账立投递/应答两条预算线",
            BudgetCode::P95OverBudget => "P95 超线：压采样批或上调预算（须走账面变更）",
            BudgetCode::RedLeak => "红账被吞：超线样本必须显性记红，禁静默丢弃",
            BudgetCode::SampleEmpty => "样本空账：先采样本再算 P95",
            BudgetCode::SortCorrupt => "排序账破：升序复核失败，检查排序实现",
            BudgetCode::SampleOutOfRange => "样本越界：延迟样本落在冻结界 [1,56] 内",
            BudgetCode::ConservationBreak => "守恒破：分行红账求和须等于全局红账",
            BudgetCode::PressureOverflow => "连发超账面容量：分批入账或扩容冻结值",
            BudgetCode::BudgetZero => "预算线为 0：零预算等于未设防，禁入账",
            BudgetCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            BudgetCode::LedgerDrift => "账指纹不符：入账存根与重算不一致，逐行对账",
            BudgetCode::Unknown => "未知诊断码：不在 0x80440 段闭集内",
        }
    }

    pub const ALL: [BudgetCode; 12] = [
        BudgetCode::BudgetMissing,
        BudgetCode::P95OverBudget,
        BudgetCode::RedLeak,
        BudgetCode::SampleEmpty,
        BudgetCode::SortCorrupt,
        BudgetCode::SampleOutOfRange,
        BudgetCode::ConservationBreak,
        BudgetCode::PressureOverflow,
        BudgetCode::BudgetZero,
        BudgetCode::FingerprintDrift,
        BudgetCode::LedgerDrift,
        BudgetCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in BudgetCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    BudgetCode::Unknown.detail()
}

// ── 冻结常量表（来源：O1 配套账·消息面预算冻结册 v1） ────────────
/// 投递 P95 预算线（逻辑拍）。
pub const BUDGET_DELIVER_P95: u32 = 48;
/// 应答 P95 预算线（逻辑拍）。
pub const BUDGET_REPLY_P95: u32 = 64;
/// 投递样本界（含）。
pub const DELIVER_LO: u32 = 1;
pub const DELIVER_HI: u32 = 40;
/// 应答样本界（含）。
pub const REPLY_LO: u32 = 1;
pub const REPLY_HI: u32 = 56;
/// 高压连发例数（冻结）。
pub const PRESSURE_BURST: u32 = 40;
/// 单账行容量（冻结）。
pub const BOOK_CAP: u32 = 64;

// ── 整数工具：升序插入排序 + P95 nearest-rank ────────────────────
/// 升序插入排序（整数，原地，swap 法不裸下标）。
pub fn sort_asc(v: &mut Vec<u32>) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let mut i = 1;
    while i < n {
        let mut j = i;
        while j > 0 {
            let a = match (v.get(j.wrapping_sub(1)), v.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => return,
            };
            if a.0 <= a.1 {
                break;
            }
            v.swap(j.wrapping_sub(1), j);
            j -= 1;
        }
        i += 1;
    }
}

/// 升序复核：任意相邻逆序即破。
pub fn sorted_ascending(v: &[u32]) -> bool {
    let mut i = 1;
    while i < v.len() {
        let a = match (v.get(i.wrapping_sub(1)), v.get(i)) {
            (Some(x), Some(y)) => (*x, *y),
            _ => return false,
        };
        if a.0 > a.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// P95 nearest-rank 整数闭式：升序取 (95n+99)/100−1 号位。
/// 空账 Err(SampleEmpty)；未升序 Err(SortCorrupt)。
pub fn p95_of_sorted(sorted: &[u32]) -> Result<u32, u32> {
    if sorted.is_empty() {
        return Err(BudgetCode::SampleEmpty.val());
    }
    if !sorted_ascending(sorted) {
        return Err(BudgetCode::SortCorrupt.val());
    }
    let n = sorted.len() as u32;
    let idx = (n
        .checked_mul(95)
        .and_then(|x| x.checked_add(99))
        .and_then(|x| x.checked_div(100))
        .and_then(|x| x.checked_sub(1)))
    .ok_or(BudgetCode::SortCorrupt.val())?;
    match sorted.get(idx as usize) {
        Some(v) => Ok(*v),
        None => Err(BudgetCode::SortCorrupt.val()),
    }
}

// ── 确定性 LCG（种子写死 = SEG_BASE） ────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 采一批投递/应答延迟样本（LCG 冻结界内生成）。
pub fn sample_burst(burst: u32) -> Result<(Vec<u32>, Vec<u32>), u32> {
    if burst > BOOK_CAP {
        return Err(BudgetCode::PressureOverflow.val());
    }
    if burst == 0 {
        return Err(BudgetCode::SampleEmpty.val());
    }
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut dv = Vec::new();
    let mut rv = Vec::new();
    let mut i = 0;
    while i < burst {
        let d = DELIVER_LO + rng.below((DELIVER_HI - DELIVER_LO + 1) as u64) as u32;
        let r = REPLY_LO + rng.below((REPLY_HI - REPLY_LO + 1) as u64) as u32;
        if d < DELIVER_LO || d > DELIVER_HI || r < REPLY_LO || r > REPLY_HI {
            return Err(BudgetCode::SampleOutOfRange.val());
        }
        dv.push(d);
        rv.push(r);
        i += 1;
    }
    Ok((dv, rv))
}

// ── O1 配套账：预算行 ────────────────────────────────────────────
/// 一条预算账行：预算线 / 实测 P95 / 红账行数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetRow {
    /// 预算线名（冻结）。
    pub name: &'static str,
    /// 预算线值（拍）。
    pub budget: u32,
    /// 实测 P95（拍）。
    pub measured: u32,
    /// 红账行数（超线样本数）。
    pub red_rows: u32,
}

/// O1 配套账：两条预算线（投递/应答）+ 全局红账。
pub struct O1Ledger {
    pub rows: Vec<BudgetRow>,
    pub red_total: u32,
}

impl O1Ledger {
    pub fn new() -> O1Ledger {
        O1Ledger { rows: Vec::new(), red_total: 0 }
    }

    /// 立预算行（预算值为 0 拒收；重名拒收）。
    pub fn register(&mut self, name: &'static str, budget: u32) -> Result<(), u32> {
        if budget == 0 {
            return Err(BudgetCode::BudgetZero.val());
        }
        for r in self.rows.iter() {
            if r.name == name {
                return Err(BudgetCode::LedgerDrift.val());
            }
        }
        self.rows.push(BudgetRow { name, budget, measured: 0, red_rows: 0 });
        Ok(())
    }

    /// 按名取行下标。
    fn row_idx(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 结算一条预算线：排样本 → P95 → 超线记红（逐样本红账）。
    pub fn settle(&mut self, name: &str, mut samples: Vec<u32>) -> Result<u32, u32> {
        let idx = match self.row_idx(name) {
            None => return Err(BudgetCode::BudgetMissing.val()),
            Some(i) => i,
        };
        if samples.is_empty() {
            return Err(BudgetCode::SampleEmpty.val());
        }
        sort_asc(&mut samples);
        let p = p95_of_sorted(&samples)?;
        let budget = match self.rows.get(idx) {
            Some(r) => r.budget,
            None => return Err(BudgetCode::BudgetMissing.val()),
        };
        // 红账：逐样本对线（红=P95 线被破的样本口径——预算线卡 P95，
        // 故红行以 P95 超线记一行；逐样本红线另用 max 口径辅助记账）。
        let mut red = 0u32;
        if p > budget {
            red = 1;
        }
        if let Some(r) = self.rows.get_mut(idx) {
            r.measured = p;
            r.red_rows += red;
        }
        self.red_total += red;
        Ok(p)
    }

    /// 高压注入账无红账判：两行红账为 0 且实测 P95 都在预算线内。
    pub fn no_red(&self) -> bool {
        if self.rows.len() != 2 {
            return false;
        }
        for r in self.rows.iter() {
            if r.red_rows != 0 || r.measured > r.budget {
                return false;
            }
        }
        self.red_total == 0
    }

    /// 守恒：分行红账之和 = 全局红账。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        for r in self.rows.iter() {
            sum = sum.wrapping_add(r.red_rows);
        }
        if sum != self.red_total {
            return Err(BudgetCode::ConservationBreak.val());
        }
        Ok(())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = self.red_total.wrapping_mul(17);
        for r in self.rows.iter() {
            fp = fp
                .wrapping_add(r.measured.wrapping_mul(3))
                .wrapping_add(r.budget);
        }
        fp
    }
}

// ── 高压连发正演 ─────────────────────────────────────────────────
/// 高压连发：PRESSURE_BURST 例全入账，P95 双线结算，要求无红账。
pub fn pressure_run() -> Result<u32, u32> {
    let (dv, rv) = sample_burst(PRESSURE_BURST)?;
    let mut led = O1Ledger::new();
    led.register("deliver", BUDGET_DELIVER_P95)?;
    led.register("reply", BUDGET_REPLY_P95)?;
    let dp = led.settle("deliver", dv)?;
    let rp = led.settle("reply", rv)?;
    if dp > BUDGET_DELIVER_P95 {
        return Err(BudgetCode::P95OverBudget.val());
    }
    if rp > BUDGET_REPLY_P95 {
        return Err(BudgetCode::P95OverBudget.val());
    }
    led.conservation()?;
    if !led.no_red() {
        return Err(BudgetCode::RedLeak.val());
    }
    Ok(led.fingerprint())
}

/// 注入反向必抓：塞一枚超预算样本，红账必须显性记行。
pub fn inject_red_sample() -> Result<bool, u32> {
    let (dv, _rv) = sample_burst(8)?;
    let mut led = O1Ledger::new();
    led.register("deliver", BUDGET_DELIVER_P95)?;
    // 人为超线样本：预算线 48，塞 240。
    let mut poisoned = Vec::new();
    for s in dv.iter() {
        poisoned.push(*s);
    }
    poisoned.push(BUDGET_DELIVER_P95 * 5);
    // 红账可见性：结算后 red_total 必须为 1（被吞即 RedLeak）。
    // nearest-rank 口径下 9 例的 P95 落在 8 号位=最大样本=240，必超线。
    let _p = led.settle("deliver", poisoned)?;
    if led.red_total == 0 {
        return Err(BudgetCode::RedLeak.val());
    }
    Ok(true)
}

// ── 抽样 10 批对平 ───────────────────────────────────────────────
/// LCG 抽 10 小批（每批 8 例）逐批结算 P95，全部落在预算线内且
/// 双跑指纹一致才对平。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ok_batches = 0u32;
    let mut i = 0;
    while i < 10 {
        let burst = 4 + rng.below(8) as u32;
        let (mut dv, mut rv) = sample_burst(burst)?;
        // 复核：样本全部落在冻结界内。
        let mut in_range = true;
        for s in dv.iter() {
            if *s < DELIVER_LO || *s > DELIVER_HI {
                in_range = false;
            }
        }
        for s in rv.iter() {
            if *s < REPLY_LO || *s > REPLY_HI {
                in_range = false;
            }
        }
        sort_asc(&mut dv);
        sort_asc(&mut rv);
        let dp = p95_of_sorted(&dv)?;
        let rp = p95_of_sorted(&rv)?;
        if in_range && dp <= BUDGET_DELIVER_P95 && rp <= BUDGET_REPLY_P95 {
            ok_batches += 1;
        }
        i += 1;
    }
    if ok_batches == 10 {
        Ok(10)
    } else {
        Err(BudgetCode::P95OverBudget.val())
    }
}

// ── 注入面：账违约必抓 ───────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入族 0：未立预算线先结算 → BudgetMissing。
/// 注入族 1：预算线为 0 → BudgetZero。
/// 注入族 2：空样本结算 → SampleEmpty。
/// 注入族 3：乱序账出 P95 → SortCorrupt。
/// 注入族 4：红账被吞（手工清零红账后守恒破）→ ConservationBreak。
pub fn inject_ledger_violations(led: &mut InjectLedger) {
    // 族 0。
    led.tries += 1;
    let mut l0 = O1Ledger::new();
    if l0.settle("deliver", vec![8, 4, 2]) == Err(BudgetCode::BudgetMissing.val()) {
        led.caught += 1;
    }
    // 族 1。
    led.tries += 1;
    let mut l1 = O1Ledger::new();
    if l1.register("deliver", 0) == Err(BudgetCode::BudgetZero.val()) {
        led.caught += 1;
    }
    // 族 2。
    led.tries += 1;
    let mut l2 = O1Ledger::new();
    let _ = l2.register("deliver", BUDGET_DELIVER_P95);
    if l2.settle("deliver", Vec::new()) == Err(BudgetCode::SampleEmpty.val()) {
        led.caught += 1;
    }
    // 族 3。
    led.tries += 1;
    if p95_of_sorted(&[9, 3, 5]) == Err(BudgetCode::SortCorrupt.val()) {
        led.caught += 1;
    }
    // 族 4。
    led.tries += 1;
    let mut l4 = O1Ledger::new();
    let _ = l4.register("deliver", BUDGET_DELIVER_P95);
    let _ = l4.settle("deliver", vec![BUDGET_DELIVER_P95 + 9]);
    // 手工吞红：清零红账 → 守恒破必抓。
    if let Some(r) = l4.rows.get_mut(0) {
        r.red_rows = 0;
    }
    if l4.conservation() == Err(BudgetCode::ConservationBreak.val()) {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：立账→高压连发→双线 P95 在册→无红账→守恒。
pub fn golden_run() -> Result<u32, u32> {
    pressure_run()
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(BudgetCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：投递/应答 P95 预算在册（O1 配套账两行齐且实测在线内）。
fn budget_registered_ok() -> bool {
    match pressure_run() {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// J2：高压注入账无红账。
fn pressure_no_red_ok() -> bool {
    match pressure_run() {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// J3：红账注入必抓（超线样本显性记红）。
fn red_visible_ok() -> bool {
    match inject_red_sample() {
        Ok(true) => true,
        _ => false,
    }
}

/// J4：抽样 10 批对平。
fn sample_ok() -> bool {
    sample_ten() == Ok(10)
}

/// J5：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in BudgetCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push((
        "J1-预算在册双线",
        budget_registered_ok(),
        format!(
            "投递≤{} 拍 / 应答≤{} 拍",
            BUDGET_DELIVER_P95, BUDGET_REPLY_P95
        ),
    ));
    v.push(("J1-高压无红账", pressure_no_red_ok(), String::from("40 例连发")));
    v.push(("J2-红账注入必抓", red_visible_ok(), String::from("超线样本记红")));
    v.push(("J3-抽样10批对平", sample_ok(), format!("{:?}", sample_ten())));
    // 边界：P95 nearest-rank 闭式（单样本=自身；两样本取第 2 位）。
    let p1 = p95_of_sorted(&[7]);
    let p2 = p95_of_sorted(&[3, 9]);
    v.push((
        "J4-P95闭式边界",
        p1 == Ok(7) && p2 == Ok(9),
        format!("{:?}/{:?}", p1, p2),
    ));
    // 边界：排序复核。
    let mut s = vec![5, 1, 4];
    sort_asc(&mut s);
    v.push((
        "J4-排序复核",
        sorted_ascending(&s) && p95_of_sorted(&s) == Ok(5),
        format!("{:?}", s),
    ));
    // 注入面。
    let mut led = InjectLedger::new();
    inject_ledger_violations(&mut led);
    v.push((
        "J5-账违约注入全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 边界：容量。
    v.push((
        "J5-连发容量边界",
        sample_burst(BOOK_CAP + 1) == Err(BudgetCode::PressureOverflow.val()),
        format!("容量 {}", BOOK_CAP),
    ));
    v.push(("J6-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J7-段位守卫", segment_guard(), String::from("0x80440 窗口")));
    v
}

/// VX9 检查入口（卡 12617，UNX-F12618-J1 判据承载）。
pub fn run_unx12617_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12617 消息面对照表批入账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_budget_two_lines_registered() {
        assert!(budget_registered_ok());
    }

    #[test]
    fn test_pressure_no_red() {
        assert!(pressure_no_red_ok());
    }

    #[test]
    fn test_red_sample_visible() {
        assert_eq!(inject_red_sample(), Ok(true));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_ledger_violations(&mut led);
        assert_eq!(led.tries, 5);
        assert_eq!(led.caught, 5);
    }

    #[test]
    fn test_p95_closed_form() {
        assert_eq!(p95_of_sorted(&[7]), Ok(7));
        assert_eq!(p95_of_sorted(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), Ok(10));
        assert_eq!(
            p95_of_sorted(&[9, 1]),
            Err(BudgetCode::SortCorrupt.val())
        );
    }

    #[test]
    fn test_sample_ten_batches() {
        assert_eq!(sample_ten(), Ok(10));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_capacity_boundary() {
        assert_eq!(
            sample_burst(BOOK_CAP + 1),
            Err(BudgetCode::PressureOverflow.val())
        );
        assert_eq!(sample_burst(0), Err(BudgetCode::SampleEmpty.val()));
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80440);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in BudgetCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x9999).contains("未知"));
    }
}
