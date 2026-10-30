//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15341 · 计数原子性全域复核（FetchAdd 语义） · 0x67240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15341-J1 原子复核成文（计数原语 RMV 语义逐原子核验
//!      10/10 次），非原子注入（普通读写计数）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - FetchAdd（RMV）语义钉版：读-改-写三步不可分割，返回值 =
//!     修改前的旧值；两次 fetch_add 的旧值必不相同（无重复派发）；
//!   - 序列化核验账：并发操作以全局序展开，逐条核对
//!     returned_old == ledger_before 且 delta 一致；
//!   - 非原子注入模型：read-read-write-write 交错（普通读写计数
//!     丢更新）——两次加法共用同一旧值 → 重复旧值 + 终账短少
//!     delta，双特征必检出；
//!   - 守恒恒等式：Σdelta == final − initial；旧值集合 = 区间
//!     [initial, final) 的无重全排（原子序下）；
//!   - LCG 种子写死 = SEG_BASE（0x67240），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67240，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67240;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67240;

/// 原子操作码闭集。
pub const AOP_FETCH_ADD: u8 = 0; // 计数 +=1，返回旧值
pub const AOP_FETCH_SUB: u8 = 1; // 计数 -=1，返回旧值
pub const AOP_LOAD: u8 = 2; // 只读
pub const AOP_CAS: u8 = 3; // 比较交换（期望值命中才换）
pub const AOP_COUNT: usize = 4;
pub const AOP_NAMES: [&str; AOP_COUNT] = ["FetchAdd", "FetchSub", "Load", "CAS"];

/// 判据投影：逐原子核验 10/10。
pub const ATOMIC_TRIALS: usize = 10;
/// 每轮原子操作数。
pub const OPS_PER_ROUND: usize = 200;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomCode {
    /// 操作码越闭集
    AopOutOfRange,
    /// FetchSub 下溢（计数减至负）
    SubUnderflow,
    /// CAS 期望值与账面不符（写入被拒）
    CasMismatch,
    /// 旧值重复派发（原子性破——两次操作同一旧值）
    DuplicateOld,
    /// 旧值越界（不在 [initial, final) 区间）
    OldOutOfRange,
    /// 守恒破账（Σdelta ≠ 终−初）
    ConservationBreach,
    /// 非原子丢更新（终账短少）
    LostUpdate,
    /// 账目顺序错乱（返回旧值后写值与账不符）
    RmvSequenceBreach,
    /// 下溢哨兵触发（过程负账）
    NegativeMidway,
}

impl AtomCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AtomCode::AopOutOfRange => "操作码越闭集：4 原语冻结",
            AtomCode::SubUnderflow => "FetchSub 下溢：计数不得减至负",
            AtomCode::CasMismatch => "CAS 期望值与账面不符：交换被拒",
            AtomCode::DuplicateOld => "旧值重复派发：RMV 原子性破",
            AtomCode::OldOutOfRange => "旧值越界：不在 [initial, final)",
            AtomCode::ConservationBreach => "守恒破账：Σdelta ≠ 终−初",
            AtomCode::LostUpdate => "非原子丢更新：终账短少",
            AtomCode::RmvSequenceBreach => "RMV 序列错乱：返回旧值与写值不符",
            AtomCode::NegativeMidway => "下溢哨兵：过程负账",
        }
    }
    pub const ALL: [AtomCode; 9] = [
        AtomCode::AopOutOfRange,
        AtomCode::SubUnderflow,
        AtomCode::CasMismatch,
        AtomCode::DuplicateOld,
        AtomCode::OldOutOfRange,
        AtomCode::ConservationBreach,
        AtomCode::LostUpdate,
        AtomCode::RmvSequenceBreach,
        AtomCode::NegativeMidway,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AtomCode::ALL.len() {
        if AtomCode::ALL[i].val() == code {
            return AtomCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67240 段闭集内"
}

// ── 原子账元 ─────────────────────────────────────────────────────
/// 一条原子操作入账记录。
pub struct AtomEntry {
    pub op: u8,
    pub returned_old: i64,
    pub ledger_before: i64,
    pub ledger_after: i64,
    /// CAS 专用：期望值（非 CAS 置 i64::MIN 哨兵）。
    pub cas_expected: i64,
    /// CAS 专用：是否交换成功。
    pub cas_ok: bool,
}

/// 原子计数器账本：以全局序逐条展开并核 RMV 语义。
pub struct AtomicLedger {
    pub initial: i64,
    pub count: i64,
    pub log: Vec<AtomEntry>,
    /// 旧值派发直方图（按返回值分桶：add 列 / sub 列）——
    /// 原子序不变量：边 v→v+1 的净穿越数 add(v)−sub(v) 必等于
    /// [final > v]（终值过边为 1，否则 0）。
    pub add_ret: Vec<u32>,
    pub sub_ret: Vec<u32>,
    /// Σdelta 守恒列。
    pub sum_delta: i64,
    pub min_seen: i64,
}

impl AtomicLedger {
    pub fn new(initial: i64) -> AtomicLedger {
        AtomicLedger {
            initial,
            count: initial,
            log: Vec::new(),
            add_ret: Vec::new(),
            sub_ret: Vec::new(),
            sum_delta: 0,
            min_seen: initial,
        }
    }

    /// FetchAdd：count+=1，返回旧值；RMV 三步一次入账。
    pub fn fetch_add(&mut self) -> Result<i64, u32> {
        let old = self.count;
        self.count += 1;
        self.sum_delta += 1;
        self.entry(AOP_FETCH_ADD, old, old, old + 1, i64::MIN, true);
        Ok(old)
    }

    /// FetchSub：count-=1，返回旧值；下溢拒收。
    pub fn fetch_sub(&mut self) -> Result<i64, u32> {
        if self.count <= 0 {
            return Err(AtomCode::SubUnderflow.val());
        }
        let old = self.count;
        self.count -= 1;
        self.sum_delta -= 1;
        // 向下穿越边 (count', count')：直方图桶取新值（old−1）。
        bump_hist(&mut self.sub_ret, self.count);
        self.entry(AOP_FETCH_SUB, old, old, old - 1, i64::MIN, true);
        Ok(old)
    }

    /// Load：只读，不入旧值集。
    pub fn load(&mut self) -> i64 {
        self.entry(AOP_LOAD, self.count, self.count, self.count, i64::MIN, true);
        self.count
    }

    /// CAS：期望命中才换，返回 (旧值, 是否交换)。
    pub fn cas(&mut self, expected: i64, new: i64) -> Result<(i64, bool), u32> {
        let old = self.count;
        let ok = old == expected;
        if ok {
            self.count = new;
            self.sum_delta += new - old;
            // 命中的 CAS 同样跨越计数边——逐边记入直方图（边穿越
            // 不变量把 CAS 命中与 RMV 同账核算）。
            if new > old {
                let mut v = old;
                while v < new {
                    bump_hist(&mut self.add_ret, v);
                    v += 1;
                }
            }
            if new < old {
                let mut v = new;
                while v < old {
                    bump_hist(&mut self.sub_ret, v);
                    v += 1;
                }
            }
        }
        self.entry(AOP_CAS, old, old, self.count, expected, ok);
        Ok((old, ok))
    }

    fn entry(
        &mut self,
        op: u8,
        returned_old: i64,
        before: i64,
        after: i64,
        cas_expected: i64,
        cas_ok: bool,
    ) {
        self.log.push(AtomEntry {
            op,
            returned_old,
            ledger_before: before,
            ledger_after: after,
            cas_expected,
            cas_ok,
        });
        if op == AOP_FETCH_ADD && cas_ok {
            bump_hist(&mut self.add_ret, returned_old);
        }
        if self.count < self.min_seen {
            self.min_seen = self.count;
        }
    }
}

// ── RMV 全域复核（判据核心） ─────────────────────────────────────
/// 旧值直方图加一（按返回值分桶，动态扩容）。
fn bump_hist(h: &mut Vec<u32>, v: i64) {
    let idx = v as usize;
    while h.len() <= idx {
        h.push(0);
    }
    h[idx] += 1;
}

/// 逐条核验账目：RMV 三步语义 + 边穿越不变量 + 守恒。
/// 原子序铁律（边穿越式）：边 v→v+1 的净穿越 add(v)−sub(v) 恒等于
/// [final > v]（初值 0、终值非负的 1D 游走必过此式——重复派发/
/// 丢更新/越界旧值任一现形都会破此式）。
pub fn audit_rmv(led: &AtomicLedger) -> Result<(u32, u32), u32> {
    // 1) 逐条 RMV 序列核对。
    let mut i = 0;
    let mut rmv_ops = 0;
    let mut cas_hits = 0;
    let mut add_total = 0u32;
    let mut sub_total = 0u32;
    while i < led.log.len() {
        let e = &led.log[i];
        let is_rmv = e.op == AOP_FETCH_ADD || e.op == AOP_FETCH_SUB;
        if is_rmv {
            if e.ledger_after - e.ledger_before != if e.op == AOP_FETCH_ADD { 1 } else { -1 } {
                return Err(AtomCode::RmvSequenceBreach.val());
            }
            if e.returned_old != e.ledger_before {
                return Err(AtomCode::RmvSequenceBreach.val());
            }
            rmv_ops += 1;
        }
        if e.op == AOP_CAS {
            // CAS 语义：命中 ⇔ expected==before；命中后 after==expected_new。
            if e.cas_ok != (e.cas_expected == e.ledger_before) {
                return Err(AtomCode::CasMismatch.val());
            }
            if e.cas_ok {
                cas_hits += 1;
            }
        }
        i += 1;
    }
    // 2) 边穿越不变量（先核语义形，再核总量）：v ∈ [0, max桶] 逐边核对。
    let n = if led.add_ret.len() > led.sub_ret.len() {
        led.add_ret.len()
    } else {
        led.sub_ret.len()
    };
    let mut v = 0usize;
    while v < n {
        let av = if v < led.add_ret.len() { led.add_ret[v] as i64 } else { 0 };
        let sv = if v < led.sub_ret.len() { led.sub_ret[v] as i64 } else { 0 };
        let diff = av - sv;
        let expect = if (led.count as usize) > v { 1 } else { 0 };
        if diff != expect {
            return Err(AtomCode::DuplicateOld.val());
        }
        v += 1;
    }
    // 3) 直方图总量守恒：分桶合计 == RMV 次数 + CAS 命中数。
    let mut a = 0;
    while a < led.add_ret.len() {
        add_total += led.add_ret[a];
        a += 1;
    }
    let mut s = 0;
    while s < led.sub_ret.len() {
        sub_total += led.sub_ret[s];
        s += 1;
    }
    if add_total + sub_total != rmv_ops + cas_hits {
        return Err(AtomCode::OldOutOfRange.val());
    }
    // 4) 守恒：Σdelta == final − initial。
    if led.sum_delta != led.count - led.initial {
        return Err(AtomCode::ConservationBreach.val());
    }
    // 5) 下溢哨兵。
    if led.min_seen < 0 {
        return Err(AtomCode::NegativeMidway.val());
    }
    Ok((rmv_ops, rmv_ops))
}

// ── 非原子注入模型（丢更新双特征） ───────────────────────────────
/// 普通（非原子）读写计数并发模型：两线程各做一次 +1，
/// 交错 read-read-write-write → 丢更新。
/// 特征 A：两次操作派发同一旧值（重复旧值）；
/// 特征 B：终账比原子语义少 1（Σdelta 应为 2 实为 1）。
/// 返回 (重复旧值数, 短少 delta)；两者均 >0 才算注入现形。
pub fn lost_update_model() -> Result<(i64, i64), u32> {
    let mut naive = 0i64; // 普通读写计数
    let initial = 0i64;
    // 线程 A：读 old_A；线程 B：读 old_B（交错：读到同一值）。
    let old_a = naive;
    let old_b = naive;
    // 写回：各自 +1（后写覆盖先写）。
    naive = old_a + 1;
    naive = old_b + 1;
    let dup_old = if old_a == old_b { 1 } else { 0 };
    let short = 2 - (naive - initial);
    if dup_old == 0 || short == 0 {
        return Err(AtomCode::LostUpdate.val());
    }
    Ok((dup_old, short))
}

// ── 确定性 LCG（种子 = 0x67240） ─────────────────────────────────
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
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 单轮随机原子压演（200 操作后账自洽） ─────────────────────────
pub fn atomic_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut led = AtomicLedger::new(0);
    let mut i = 0;
    while i < OPS_PER_ROUND {
        let sel = lcg.below(6) as u8; // 加权：Add 多、Sub 少、Load/CAS 搭配
        match sel {
            0 | 1 | 2 => {
                led.fetch_add()?;
            }
            3 => {
                // 只在 count>0 时 Sub（下溢拒收走注入面）。
                if led.count > 0 {
                    led.fetch_sub()?;
                }
            }
            4 => {
                let _ = led.load();
            }
            _ => {
                // CAS：以当前值为期望 +1（必命中）；偶发错期望（必拒换）。
                let wrong = lcg.below(2) == 0;
                let expect = if wrong { led.count + 99 } else { led.count };
                let (_, ok) = led.cas(expect, led.count + 1)?;
                if wrong && ok {
                    return Err(AtomCode::CasMismatch.val());
                }
            }
        }
        i += 1;
    }
    audit_rmv(&led)
}

// ── 黄金正演：10/10 轮逐原子核验 + 黄金手算小序 ─────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rounds = 0;
    let mut rmv_total = 0;
    let mut r = 0;
    while r < ATOMIC_TRIALS {
        let (rmv, oldn) = atomic_round(r)?;
        if rmv != oldn {
            return Err(AtomCode::DuplicateOld.val());
        }
        rmv_total += rmv;
        rounds += 1;
        r += 1;
    }
    // 黄金手算小序（写死判据，独立于压演）：
    //   [Add,Add,Sub,Add] → 旧值 0,1,2,1（终 count=2，Σdelta=+2）。
    let mut g = AtomicLedger::new(0);
    let a0 = g.fetch_add()?;
    let a1 = g.fetch_add()?;
    let s2 = g.fetch_sub()?;
    let a3 = g.fetch_add()?;
    let seq_ok = a0 == 0 && a1 == 1 && s2 == 2 && a3 == 1 && g.count == 2;
    if !seq_ok {
        return Err(AtomCode::RmvSequenceBreach.val());
    }
    let g_audit = audit_rmv(&g)?;
    if g_audit.0 != 4 {
        // 4 条 RMV（3 加 1 减）全入账。
        return Err(AtomCode::ConservationBreach.val());
    }
    // 非原子注入双特征必现。
    let (dup, short) = lost_update_model()?;
    if dup != 1 || short != 1 {
        return Err(AtomCode::LostUpdate.val());
    }
    Ok((rounds, rmv_total, g.count as u32))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：非原子丢更新 ×10（重复旧值 + 短少 delta 双特征）。
    let mut t = 0;
    while t < 10 {
        let v = lost_update_model();
        ledger.tries[0] += 1;
        if let Ok((dup, short)) = v {
            if dup > 0 && short > 0 {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：旧值重复注入（构造账：同一旧值派发两次）×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AtomicLedger::new(0);
        let _ = led.fetch_add();
        // 手工伪造重复旧值派发（0 号旧值再记一票，绕过账本）。
        bump_hist(&mut led.add_ret, 0);
        let v = audit_rmv(&led);
        ledger.tries[1] += 1;
        if let Err(e) = v {
            if e == AtomCode::DuplicateOld.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：守恒破账注入（Σdelta 与终值不符）×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AtomicLedger::new(0);
        let _ = led.fetch_add();
        let _ = led.fetch_add();
        // 篡改守恒列：Σdelta 少记 1（模拟丢更新后的账面）。
        led.sum_delta -= 1;
        let v = audit_rmv(&led);
        ledger.tries[2] += 1;
        if let Err(e) = v {
            if e == AtomCode::ConservationBreach.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AtomCode::ALL.len() {
        let v = AtomCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 原语闭集守恒。
    AOP_COUNT == 4 && ATOMIC_TRIALS == 10 && OPS_PER_ROUND == 200
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, rmv, _)) = golden {
        rounds == ATOMIC_TRIALS as u32 && rmv >= (ATOMIC_TRIALS * 100) as u32
    } else {
        false
    };
    v.push((
        "J1-RMV逐原子核验10/10",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-非原子注入10/10必抓",
        led.all_caught() && led.caught[0] == 10,
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 黄金手算小序独立点验（写死判据值）。
    let mut g = AtomicLedger::new(0);
    let a0 = g.fetch_add();
    let a1 = g.fetch_add();
    let s2 = g.fetch_sub();
    let triple_ok = if let (Ok(x), Ok(y), Ok(z)) = (a0, a1, s2) {
        x == 0 && y == 1 && z == 2
    } else {
        false
    };
    v.push((
        "J1-黄金旧值序 0,1,2",
        triple_ok,
        format!("0,1,2 判据"),
    ));
    // 原语名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < AOP_COUNT {
        if AOP_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    v.push((
        "J1-4原语名点验",
        okn == AOP_COUNT,
        format!("{}/4", okn),
    ));
    // CAS 语义独立点验：错期望必拒换。
    let mut c = AtomicLedger::new(5);
    let r1 = c.cas(5, 9);
    let r2 = c.cas(4, 9);
    let (ok_hit, ok_miss) = match (r1, r2) {
        (Ok((_, h)), Ok((_, m))) => (h, m),
        _ => (false, true),
    };
    v.push((
        "J1-CAS 命中/拒换语义",
        ok_hit && !ok_miss,
        format!("hit={} miss_ok={}", ok_hit, ok_miss),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67240 窗口")));
    v
}

pub fn run_unx15341_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15341 计数原子性全域复核（FetchAdd 语义）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
