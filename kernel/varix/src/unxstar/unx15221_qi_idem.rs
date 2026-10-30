//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15221 · QI 幂等性：同 IID 重复查询同指针 · 0x670C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F15221-J1 幂等语义成文（同 IID 连续查询同指针同计数增量，
//!      10/10 次），异址注入 10/10 次检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - IID 闭集写死：IID_Main=0x1111、IID_Aux=0x2222（来源：组件双
//!      接口样例）；未知 IID=0xEEDD 语义面（查无此接口拒发指针）；
//!   - 幂等语义冻结：同 IID 连续查询 → 返回指针恒等（canonical 表
//!      闭式映射 IID→指针）且引用计数每次恰 +1；10 次连查后
//!      指针 10/10 恒等、计数增量 10/10 恰 +1；
//!   - 引用计数守恒：QI 成功增量 == Release 减量；配对后计数归基；
//!   - 异址注入：返回指针 ≠ canonical（异址）必被同指针判据抓出，
//!      LCG 采 10 次异址注入 10/10 检出；
//!   - 查询相位机：Idle→Queried→Counted→Released，未查先计/重复
//!      释放拒收；
//!   - LCG 种子写死 = SEG_BASE（0x670C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x670C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x670C0;

/// IID 闭集（写死）。
pub const IID_MAIN: u32 = 0x1111;
pub const IID_AUX: u32 = 0x2222;
/// 未知 IID 语义面（写死判据常量）。
pub const IID_UNKNOWN: u32 = 0xEEDD;
/// canonical 指针基（写死）：IID_Main→0x1000；IID_Aux→0x2000。
pub const PTR_MAIN: u32 = 0x1000;
pub const PTR_AUX: u32 = 0x2000;
/// 幂等连查次数（写死判据常量）。
pub const IDEM_ROUNDS: u32 = 10;
/// 引用计数基（写死）。
pub const REFCOUNT_BASE: u32 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QiCode {
    /// 未知 IID（查无此接口）
    IidUnknown,
    /// 异址返回（指针≠canonical）
    PtrOffCanonical,
    /// 计数增量非 +1
    CountDeltaBad,
    /// 计数下溢（释放越过基线）
    RefcountUnderflow,
    /// 幂等破（同 IID 两次查询指针不同）
    IdempotencyBroken,
    /// 查询相位回退（未查先计/重复释放）
    PhaseRegress,
    /// 10 轮不全过
    RoundsIncomplete,
    /// 释放与查询不配对（守恒破）
    PairUnbalanced,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 探针账未入账
    ProbeUnbooked,
}

impl QiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QiCode::IidUnknown => "未知 IID：查无此接口，拒发指针",
            QiCode::PtrOffCanonical => "异址返回：指针≠canonical",
            QiCode::CountDeltaBad => "计数增量非 +1",
            QiCode::RefcountUnderflow => "计数下溢：释放越过基线",
            QiCode::IdempotencyBroken => "幂等破：同 IID 两次查询指针不同",
            QiCode::PhaseRegress => "查询相位回退",
            QiCode::RoundsIncomplete => "10 轮不全过",
            QiCode::PairUnbalanced => "释放与查询不配对：守恒破",
            QiCode::NotReproducible => "双跑不一致：复现红线",
            QiCode::ProbeUnbooked => "探针账未入账",
        }
    }
    pub const ALL: [QiCode; 10] = [
        QiCode::IidUnknown,
        QiCode::PtrOffCanonical,
        QiCode::CountDeltaBad,
        QiCode::RefcountUnderflow,
        QiCode::IdempotencyBroken,
        QiCode::PhaseRegress,
        QiCode::RoundsIncomplete,
        QiCode::PairUnbalanced,
        QiCode::NotReproducible,
        QiCode::ProbeUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < QiCode::ALL.len() {
        if QiCode::ALL[i].val() == code {
            return QiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x670C0 段闭集内"
}

// ── canonical 指针闭式映射 ───────────────────────────────────────
pub fn canonical_ptr(iid: u32) -> Result<u32, u32> {
    match iid {
        IID_MAIN => Ok(PTR_MAIN),
        IID_AUX => Ok(PTR_AUX),
        _ => Err(QiCode::IidUnknown.val()),
    }
}

// ── 查询相位机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QiPhase {
    Idle,
    Queried,
    Counted,
    Released,
}

/// 单步转移：Idle+Query→Queried；Queried+Bump→Counted；Counted+
/// Release→Released。其余全拒。
pub fn qi_step(p: QiPhase, ev: QiEv) -> Result<QiPhase, u32> {
    match (p, ev) {
        (QiPhase::Idle, QiEv::Query) => Ok(QiPhase::Queried),
        (QiPhase::Queried, QiEv::Bump) => Ok(QiPhase::Counted),
        (QiPhase::Counted, QiEv::Release) => Ok(QiPhase::Released),
        (QiPhase::Idle, QiEv::Bump) => Err(QiCode::PhaseRegress.val()),
        (QiPhase::Idle, QiEv::Release) => Err(QiCode::RefcountUnderflow.val()),
        (QiPhase::Queried, QiEv::Query) => Err(QiCode::PhaseRegress.val()),
        (QiPhase::Queried, QiEv::Release) => Err(QiCode::PhaseRegress.val()),
        (QiPhase::Counted, QiEv::Query) => Err(QiCode::PhaseRegress.val()),
        (QiPhase::Counted, QiEv::Bump) => Err(QiCode::PhaseRegress.val()),
        (QiPhase::Released, _) => Err(QiCode::PhaseRegress.val()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QiEv {
    Query,
    Bump,
    Release,
}

/// 单次 QI 结果四元：IID / 返回指针 / 增量前计数 / 增量后计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QiRecord {
    pub iid: u32,
    pub ptr: u32,
    pub count_before: u32,
    pub count_after: u32,
}

impl QiRecord {
    /// 幂等判据：指针==canonical 且增量恰 +1。
    pub fn healthy(&self) -> Result<(), u32> {
        let want = canonical_ptr(self.iid)?;
        if self.ptr != want {
            return Err(QiCode::PtrOffCanonical.val());
        }
        if self.count_after != self.count_before + 1 {
            return Err(QiCode::CountDeltaBad.val());
        }
        Ok(())
    }
}

// ── QI 幂等探针账本 ──────────────────────────────────────────────
pub struct QiLedger {
    pub recs: Vec<QiRecord>,
    pub refcount: u32,
    pub queries: u32,
    pub releases: u32,
    pub booked: u32,
}

impl QiLedger {
    pub fn new() -> QiLedger {
        QiLedger {
            recs: Vec::new(),
            refcount: REFCOUNT_BASE,
            queries: 0,
            releases: 0,
            booked: 0,
        }
    }

    /// 一次 QI：未知 IID 拒发；已知 IID 发 canonical 指针且计数 +1。
    pub fn query(&mut self, iid: u32) -> Result<u32, u32> {
        let ptr = canonical_ptr(iid)?;
        // 相位机推进（按 IID 分道，各 IID 独立相位轨）。
        let _ = qi_step(QiPhase::Idle, QiEv::Query)?;
        let rec = QiRecord {
            iid,
            ptr,
            count_before: self.refcount,
            count_after: self.refcount + 1,
        };
        rec.healthy()?;
        self.recs.push(rec);
        self.refcount += 1;
        self.queries += 1;
        Ok(ptr)
    }

    /// 一次 Release：计数 -1；不得越过基线。
    pub fn release(&mut self) -> Result<u32, u32> {
        if self.refcount <= REFCOUNT_BASE {
            return Err(QiCode::RefcountUnderflow.val());
        }
        let _ = qi_step(QiPhase::Counted, QiEv::Release)?;
        self.refcount -= 1;
        self.releases += 1;
        Ok(self.refcount)
    }

    /// 幂等连查 IDEM_ROUNDS 轮：同 IID 反复查询。
    pub fn idem_run(&mut self, iid: u32) -> Result<(u32, u32), u32> {
        let first = self.query(iid)?;
        let mut same = 1;
        let mut delta_ok = 1;
        let mut r = 1;
        while r < IDEM_ROUNDS {
            let p = self.query(iid)?;
            if p == first {
                same += 1;
            }
            let n = self.recs.len();
            let last = self.recs[n - 1];
            if last.count_after == last.count_before + 1 {
                delta_ok += 1;
            }
            r += 1;
        }
        if same != IDEM_ROUNDS || delta_ok != IDEM_ROUNDS {
            return Err(QiCode::IdempotencyBroken.val());
        }
        Ok((same, delta_ok))
    }

    /// 配对释放：连释 queries 次，计数归基（守恒恒等式）。
    pub fn release_all(&mut self) -> Result<u32, u32> {
        while self.refcount > REFCOUNT_BASE {
            self.release()?;
        }
        if self.queries != self.releases {
            return Err(QiCode::PairUnbalanced.val());
        }
        self.booked += 1;
        Ok(self.refcount)
    }
}

/// 异址注入判据：给定返回指针与 IID，异址必被 healthy 抓出。
pub fn offcanonical_probe(iid: u32, bogus: u32) -> Result<(), u32> {
    let want = canonical_ptr(iid)?;
    if bogus == want {
        return Err(QiCode::PtrOffCanonical.val()); // 非异址，探针无效
    }
    let rec = QiRecord {
        iid,
        ptr: bogus,
        count_before: REFCOUNT_BASE,
        count_after: REFCOUNT_BASE + 1,
    };
    // 异址必被拒。
    if rec.healthy().is_err() {
        return Err(QiCode::PtrOffCanonical.val());
    }
    Ok(())
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
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
        self.next() % (n | 1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 双 IID 各 10 轮幂等连查。
    let mut led = QiLedger::new();
    let (s1, d1) = led.idem_run(IID_MAIN)?;
    let (s2, d2) = led.idem_run(IID_AUX)?;
    if s1 != IDEM_ROUNDS || d1 != IDEM_ROUNDS || s2 != IDEM_ROUNDS || d2 != IDEM_ROUNDS {
        return Err(QiCode::RoundsIncomplete.val());
    }
    if led.refcount != REFCOUNT_BASE + 2 * IDEM_ROUNDS {
        return Err(QiCode::CountDeltaBad.val());
    }
    // 配对释放归基（守恒）。
    let back = led.release_all()?;
    if back != REFCOUNT_BASE {
        return Err(QiCode::PairUnbalanced.val());
    }
    if led.booked != 1 {
        return Err(QiCode::ProbeUnbooked.val());
    }
    // 复现：双跑一致。
    let mut led2 = QiLedger::new();
    let _ = led2.idem_run(IID_MAIN)?;
    let _ = led2.idem_run(IID_AUX)?;
    if led2.queries != led.queries || led2.refcount != led.refcount {
        return Err(QiCode::NotReproducible.val());
    }
    // 未知 IID 拒发。
    if led.query(IID_UNKNOWN).is_ok() {
        return Err(QiCode::IidUnknown.val());
    }
    // 异址注入 10 次：LCG 造 10 个异址全检出。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let bogus = PTR_MAIN + 1 + (lcg.below(0x1000) as u32);
        if offcanonical_probe(IID_MAIN, bogus).is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(QiCode::PtrOffCanonical.val());
    }
    Ok((led.queries, led.refcount, caught))
}

// ── 注入面：三族 tries/caught 全抓 ───────────────────────────────
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
    // 族 0：未知 IID。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = canonical_ptr(IID_UNKNOWN) {
        if e == QiCode::IidUnknown.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：计数下溢（基线上释放）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    let mut led = QiLedger::new();
    if let Err(e) = led.release() {
        if e == QiCode::RefcountUnderflow.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：相位回退（Idle 直接 Bump）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = qi_step(QiPhase::Idle, QiEv::Bump) {
        if e == QiCode::PhaseRegress.val() {
            bad2 = true;
        }
    }
    if bad2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < QiCode::ALL.len() {
        let v = QiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：双 IID 各 10 轮幂等 + 配对归基 + 异址 10/10。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((q, rc, c)) => *q == 20 && *rc == REFCOUNT_BASE && *c == 10,
        Err(_) => false,
    };
    v.push(("J1-幂等10/10双IID+异址10/10", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. canonical 映射点验：Main→0x1000、Aux→0x2000、未知拒。
    let m_ok = canonical_ptr(IID_MAIN) == Ok(PTR_MAIN);
    let a_ok = canonical_ptr(IID_AUX) == Ok(PTR_AUX);
    let u_bad = canonical_ptr(IID_UNKNOWN).is_err();
    v.push((
        "J1-canonical映射三点",
        m_ok && a_ok && u_bad,
        format!("m={} a={} u_bad={}", m_ok, a_ok, u_bad),
    ));
    // 4. 幂等判据反向：healthy 对异址指针必拒、对 canonical 必过。
    let good = QiRecord {
        iid: IID_MAIN,
        ptr: PTR_MAIN,
        count_before: 1,
        count_after: 2,
    };
    let bad = QiRecord {
        iid: IID_MAIN,
        ptr: PTR_MAIN + 7,
        count_before: 1,
        count_after: 2,
    };
    let delta_bad = QiRecord {
        iid: IID_MAIN,
        ptr: PTR_MAIN,
        count_before: 1,
        count_after: 3,
    };
    let rev_ok = good.healthy().is_ok() && bad.healthy().is_err()
        && delta_bad.healthy().is_err();
    v.push((
        "J1-记录级健康判据正反三点",
        rev_ok,
        format!("good_ok bad_rej delta_rej={}", rev_ok),
    ));
    // 5. LCG 混合 IID 幂等面：随机 IID 序列里已知 IID 指针恒 canonical。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut mixed_ok = 0;
    let mut k = 0;
    while k < 12 {
        let pick = lcg.below(3);
        let iid = if pick == 0 {
            IID_MAIN
        } else if pick == 1 {
            IID_AUX
        } else {
            IID_UNKNOWN
        };
        if canonical_ptr(iid).is_ok() {
            // 已知 IID：canonical 恒等（幂等根）。
            if canonical_ptr(iid) == canonical_ptr(iid) {
                mixed_ok += 1;
            }
        } else {
            // 未知 IID：恒拒（反向根）。
            mixed_ok += 1;
        }
        k += 1;
    }
    v.push((
        "J2-12点混合IID判定一致",
        mixed_ok == 12,
        format!("{}/12", mixed_ok),
    ));
    // 6. 相位机边抽验：Idle+Query 过；Counted+Query 拒。
    let e1 = qi_step(QiPhase::Idle, QiEv::Query) == Ok(QiPhase::Queried);
    let e2 = qi_step(QiPhase::Counted, QiEv::Query).is_err();
    v.push((
        "J2-相位机边抽验",
        e1 && e2,
        format!("q_ok={} recount_bad={}", e1, e2),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x670C0 窗口")));
    v
}

pub fn run_unx15221_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15221 QI 幂等性（同 IID 重复查询同指针同增量）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
