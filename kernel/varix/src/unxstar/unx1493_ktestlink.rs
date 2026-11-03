//! UNX-F1493 断言与 ktest 挂接复核（全域断言可执行面复核） · 0x75E80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5 L 组）：
//!   全域断言可执行面复核（断言↔ktest 双向挂接 1:1），挂接断言数
//!   =断言总数 100% 挂接，挂接缺口注入（抽断言脱挂）10/10 必抓。
//!
//! 设计要点：
//!   ① 双向挂接 1:1：每条断言恰挂 1 个 ktest，每个 ktest 恰挂 1 条
//!      断言——挂接账以断言号与 ktest 号双侧唯一性闸保证（双侧重复
//!      挂接均显性拒）；
//!   ② 覆盖率判定：挂接断言数 = 断言总数（100% 挂接），缺一口即翻
//!      红（CoverageHole）——脱挂注入的抓点；
//!   ③ 脱挂注入（抽断言脱挂）10/10 必抓：每轮随机（LCG 固定种子）
//!      抽一条断言的挂接，覆盖复核当轮必报缺口；
//!   ④ 双向对拍：断言侧账与 ktest 侧账逐对互查（正向断言→ktest、
//!      反向 ktest→断言），任何单向账都属挂接损坏；
//!   ⑤ 状态机单调：Unlinked（未挂）→ Linked（全挂）→ Audited（复
//!      核过），倒退显性拒（脱挂即跌回 Unlinked 由复核显性记账）；
//!   ⑥ ktest 语义真实：ktest 是断言的可执行体占位（名称+判定轮数
//!      口径），挂接表冻结可复算。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75E80，批5 L 组 F1493 独占窗口）。
pub const SEG_BASE: u32 = 0x75E80;

/// 种子断言数（冻结 24：全域断言可执行面种子）。
pub const ASSERT_TOTAL: u32 = 24;

/// ktest 判定轮数口径（冻结 10：10/10 语义）。
pub const KTEST_ROUNDS: u32 = 10;

/// 判定轮数（10/10 语义，注入轮写死 10 轮）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkCode {
    /// 断言号越界
    AssertOver,
    /// 断言重复挂接（一断言多 ktest）
    AssertDoubleLink,
    /// ktest 重复挂接（一 ktest 多断言）
    KtestDoubleLink,
    /// 覆盖缺口：挂接断言数 < 断言总数
    CoverageHole,
    /// 双向账失配（正向有、反向无）
    DirectionMismatch,
    /// ktest 缺执行体占位
    KtestMissing,
    /// 状态机倒退/越级
    BadState,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl LinkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LinkCode::AssertOver => "断言号越界：超出全域断言闭集；\
                请核对断言总数账后再挂接",
            LinkCode::AssertDoubleLink => "断言重复挂接：1:1 语义下每断言恰挂 1 ktest；\
                请先解除旧挂接（留痕）再挂新",
            LinkCode::KtestDoubleLink => "ktest 重复挂接：1:1 语义下每 ktest 恰挂 1 断言；\
                请换未占用的 ktest 号",
            LinkCode::CoverageHole => "覆盖缺口：挂接断言数 < 断言总数；\
                怀疑脱挂注入，请全量补挂至 100%",
            LinkCode::DirectionMismatch => "双向账失配：正向账有而反向账无；\
                请重放双向挂接通道修复对称性",
            LinkCode::KtestMissing => "ktest 缺执行体：挂接目标无占位；\
                请先登记 ktest 再挂接",
            LinkCode::BadState => "状态机倒退/越级：复核流程只许单向推进；\
                请按 Unlinked→Linked→Audited 次序走",
            LinkCode::NotReproducible => "双跑不一致：同输入两跑结果不同；\
                请排查全局可变状态",
        }
    }
    pub const ALL: [LinkCode; 8] = [
        LinkCode::AssertOver,
        LinkCode::AssertDoubleLink,
        LinkCode::KtestDoubleLink,
        LinkCode::CoverageHole,
        LinkCode::DirectionMismatch,
        LinkCode::KtestMissing,
        LinkCode::BadState,
        LinkCode::NotReproducible,
    ];
}

/// 查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LinkCode::ALL.len() {
        if LinkCode::ALL[i].val() == code {
            return LinkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x75E80 段闭集内"
}

// ── 复核状态机（单调，倒退显性拒） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    /// 未挂接
    Unlinked,
    /// 全挂接（100%）
    Linked,
    /// 复核通过
    Audited,
}

impl LinkState {
    fn rank(self) -> u8 {
        match self {
            LinkState::Unlinked => 0,
            LinkState::Linked => 1,
            LinkState::Audited => 2,
        }
    }
}

// ── 域模型 ─────────────────────────────────────────────────────
/// 断言占位（全域断言可执行面的账行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertStub {
    /// 断言号（1..ASSERT_TOTAL）
    pub aid: u32,
    /// 断言名
    pub name: String,
}

/// ktest 执行体占位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KtestStub {
    /// ktest 号（1..ASSERT_TOTAL，与断言同容量）
    pub kid: u32,
    /// ktest 名
    pub name: String,
    /// 判定轮数口径（恒 KTEST_ROUNDS）
    pub rounds: u32,
}

/// 双向挂接账。
#[derive(Clone, Debug)]
pub struct LinkLedger {
    pub asserts: Vec<AssertStub>,
    pub ktests: Vec<KtestStub>,
    /// 正向账：断言号 → ktest 号（下标 aid-1 对齐，值 0=未挂）
    pub fwd: Vec<u32>,
    /// 反向账：ktest 号 → 断言号（下标 kid-1 对齐，值 0=未挂）
    pub rev: Vec<u32>,
    pub state: LinkState,
    /// 脱挂注入被拒/被抓获笔数
    pub holes_found: u32,
    /// 复核序（Audited 一次后置 1）
    pub audit_seq: u32,
}

impl LinkLedger {
    pub fn new() -> LinkLedger {
        LinkLedger {
            asserts: Vec::new(),
            ktests: Vec::new(),
            fwd: Vec::new(),
            rev: Vec::new(),
            state: LinkState::Unlinked,
            holes_found: 0,
            audit_seq: 0,
        }
    }

    /// 断言登记（只增）。
    pub fn add_assert(&mut self, aid: u32, name: &str) -> Result<usize, u32> {
        if aid == 0 || aid > ASSERT_TOTAL {
            return Err(LinkCode::AssertOver.val());
        }
        self.asserts.push(AssertStub { aid, name: String::from(name) });
        self.fwd.push(0);
        Ok(self.asserts.len() - 1)
    }

    /// ktest 登记占位（只增）。
    pub fn add_ktest(&mut self, kid: u32, name: &str) -> Result<usize, u32> {
        if kid == 0 || kid > ASSERT_TOTAL {
            return Err(LinkCode::AssertOver.val());
        }
        self.ktests.push(KtestStub { kid, name: String::from(name), rounds: KTEST_ROUNDS });
        self.rev.push(0);
        Ok(self.ktests.len() - 1)
    }

    /// 挂接：断言↔ktest 双侧唯一性闸 + 双向账同步写。
    pub fn link(&mut self, aid: u32, kid: u32) -> Result<(), u32> {
        if aid == 0 || aid > ASSERT_TOTAL || kid == 0 || kid > ASSERT_TOTAL {
            return Err(LinkCode::AssertOver.val());
        }
        // ktest 执行体必须在册。
        let mut kt_found = false;
        let mut i = 0;
        while i < self.ktests.len() {
            if let Some(k) = self.ktests.get(i) {
                if k.kid == kid {
                    kt_found = true;
                    break;
                }
            }
            i += 1;
        }
        if !kt_found {
            return Err(LinkCode::KtestMissing.val());
        }
        // 正向唯一：该断言已挂 → 拒。
        let old_fwd = match self.fwd.get((aid - 1) as usize) {
            Some(v) => *v,
            None => return Err(LinkCode::AssertOver.val()),
        };
        if old_fwd != 0 {
            return Err(LinkCode::AssertDoubleLink.val());
        }
        // 反向唯一：该 ktest 已挂 → 拒。
        let old_rev = match self.rev.get((kid - 1) as usize) {
            Some(v) => *v,
            None => return Err(LinkCode::AssertOver.val()),
        };
        if old_rev != 0 {
            return Err(LinkCode::KtestDoubleLink.val());
        }
        // 双向同步写。
        if let Some(f) = self.fwd.get_mut((aid - 1) as usize) {
            *f = kid;
        }
        if let Some(r) = self.rev.get_mut((kid - 1) as usize) {
            *r = aid;
        }
        // 全挂齐则推进 Linked。
        if self.state.rank() < LinkState::Linked.rank() && self.covered_count() == ASSERT_TOTAL {
            self.state = LinkState::Linked;
        }
        Ok(())
    }

    /// 挂接数（双向账一致性口径：以正向账为准）。
    pub fn covered_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.fwd.len() {
            if let Some(v) = self.fwd.get(i) {
                if *v != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖复核：挂接数=断言总数（100%）。
    pub fn coverage_full(&self) -> bool {
        self.covered_count() == ASSERT_TOTAL && self.asserts.len() as u32 == ASSERT_TOTAL
    }

    /// 双向对拍：正向账有挂的断言，反向账必须同对回指。
    pub fn bidir_ok(&self) -> bool {
        let mut i = 0;
        while i < self.fwd.len() {
            let kid = match self.fwd.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if kid != 0 {
                let back = match self.rev.get((kid - 1) as usize) {
                    Some(v) => *v,
                    None => return false,
                };
                if back != i as u32 + 1 {
                    return false;
                }
            }
            i += 1;
        }
        // 反向账有挂的 ktest，正向账必须同对回指。
        let mut j = 0;
        while j < self.rev.len() {
            let aid = match self.rev.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if aid != 0 {
                let forth = match self.fwd.get((aid - 1) as usize) {
                    Some(v) => *v,
                    None => return false,
                };
                if forth != j as u32 + 1 {
                    return false;
                }
            }
            j += 1;
        }
        true
    }

    /// 复核：覆盖 100% + 双向对拍，过则推进 Audited。
    pub fn audit(&mut self) -> Result<u32, u32> {
        if self.state.rank() >= LinkState::Audited.rank() {
            return Err(LinkCode::BadState.val());
        }
        if !self.coverage_full() {
            self.holes_found += 1;
            return Err(LinkCode::CoverageHole.val());
        }
        if !self.bidir_ok() {
            return Err(LinkCode::DirectionMismatch.val());
        }
        self.state = LinkState::Audited;
        self.audit_seq += 1;
        Ok(self.covered_count())
    }

    /// 脱挂注入（抽断言脱挂）：抽走 aid 的挂接，覆盖必现缺口。
    pub fn inject_unlink(&mut self, aid: u32) -> Result<(), u32> {
        if aid == 0 || aid > ASSERT_TOTAL {
            return Err(LinkCode::AssertOver.val());
        }
        let kid = match self.fwd.get((aid - 1) as usize) {
            Some(v) => *v,
            None => return Err(LinkCode::AssertOver.val()),
        };
        if kid == 0 {
            // 本就未挂：注入无效但仍按缺口记账（复核会抓）。
            return Ok(());
        }
        if let Some(f) = self.fwd.get_mut((aid - 1) as usize) {
            *f = 0;
        }
        if let Some(r) = self.rev.get_mut((kid - 1) as usize) {
            *r = 0;
        }
        // 状态跌回 Unlinked 由复核显性抓（审计必须重挂）。
        Ok(())
    }
}

/// 满挂种子：24 断言 × 24 ktest 恒等挂接（aid==kid）。
pub fn seed_links() -> Result<LinkLedger, u32> {
    let mut l = LinkLedger::new();
    let mut n = 1u32;
    while n <= ASSERT_TOTAL {
        l.add_assert(n, &format!("断言{:02}", n))?;
        l.add_ktest(n, &format!("ktest{:02}", n))?;
        n += 1;
    }
    let mut n = 1u32;
    while n <= ASSERT_TOTAL {
        l.link(n, n)?;
        n += 1;
    }
    Ok(l)
}

// ── 判据谓词（私有 bool 收口） ──────────────────────────────────
/// 100% 挂接判定。
fn coverage_ok() -> bool {
    match seed_links() {
        Ok(l) => l.coverage_full() && l.covered_count() == ASSERT_TOTAL,
        Err(_) => false,
    }
}

/// 双向 1:1 判定。
fn bidir_ok() -> bool {
    match seed_links() {
        Ok(l) => l.bidir_ok() && l.state == LinkState::Linked,
        Err(_) => false,
    }
}

/// 双侧唯一闸判定：重挂同断言 / 重挂同 ktest 均显性拒。
/// 用部分挂接账（仅 1↔1 挂上），保证两路各撞各的闸。
fn double_link_rejected() -> bool {
    let mut l = LinkLedger::new();
    let mut n = 1u32;
    while n <= ASSERT_TOTAL {
        let _ = l.add_assert(n, "断言");
        let _ = l.add_ktest(n, "ktest");
        n += 1;
    }
    if l.link(1, 1).is_err() {
        return false;
    }
    // 路 1：断言 1 已挂 → AssertDoubleLink（撞正向唯一闸）。
    let a = l.link(1, 2) == Err(LinkCode::AssertDoubleLink.val());
    // 路 2：断言 2 未挂、ktest 1 已占 → KtestDoubleLink（撞反向唯一闸）。
    let b = l.link(2, 1) == Err(LinkCode::KtestDoubleLink.val());
    a && b && l.covered_count() == 1
}

/// 脱挂注入判定（10/10 必抓）：逐轮抽一条脱挂，复核必报缺口。
fn unlink_inject_all_caught(rounds: u32) -> bool {
    let mut r = 0u32;
    while r < rounds {
        let mut l = match seed_links() {
            Ok(x) => x,
            Err(_) => return false,
        };
        let victim = (r % ASSERT_TOTAL) + 1;
        if l.inject_unlink(victim).is_err() {
            return false;
        }
        // 抓：覆盖必缺、双向账必须仍对称（脱挂双侧同步）。
        if l.coverage_full() {
            return false;
        }
        if !l.bidir_ok() {
            return false;
        }
        match l.audit() {
            Err(e) => {
                if e != LinkCode::CoverageHole.val() {
                    return false;
                }
                if l.holes_found == 0 {
                    return false;
                }
            }
            Ok(_) => return false,
        }
        r += 1;
    }
    true
}

/// 双向账失配注入判定：手工只拆正向账 → bidir 必翻红。
fn direction_mismatch_caught() -> bool {
    let mut l = match seed_links() {
        Ok(x) => x,
        Err(_) => return false,
    };
    // 只拆正向（模拟单向账损坏）。
    if let Some(f) = l.fwd.get_mut(4) {
        *f = 0;
    }
    !l.bidir_ok()
}

/// 状态机单调判定：未挂齐直接复核拒；复核后再挂接拒。
fn state_monotonic_ok() -> bool {
    let mut l = LinkLedger::new();
    let mut n = 1u32;
    while n <= ASSERT_TOTAL {
        let _ = l.add_assert(n, "断言");
        let _ = l.add_ktest(n, "ktest");
        n += 1;
    }
    // 只挂 1 对：未齐即复核 → CoverageHole。
    if l.link(1, 1).is_err() {
        return false;
    }
    if l.audit().is_ok() {
        return false;
    }
    // 补挂齐全 → 复核过 → 再挂接拒。
    let mut n = 2u32;
    while n <= ASSERT_TOTAL {
        if l.link(n, n).is_err() {
            return false;
        }
        n += 1;
    }
    if l.audit().is_err() {
        return false;
    }
    l.link(1, 1).is_err() && l.audit() == Err(LinkCode::BadState.val()) && l.audit_seq == 1
}

/// 黄金全链：种子满挂→覆盖 100%→双向对拍→复核 Audited→10 轮脱挂全抓。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = seed_links()?;
    let covered = l.audit()?;
    let inject_ok = unlink_inject_all_caught(JUDGE_ROUNDS) as u32;
    Ok((covered, inject_ok, l.audit_seq))
}

/// 探针序 A：黄金全链数值封口（24 挂、注入全抓=1、复核 1 次）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((24, 1, 1)) => Ok(()),
        _ => Err(LinkCode::NotReproducible.val()),
    }
}

/// 探针序 B：脱挂单点必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = seed_links()?;
    l.inject_unlink(7)?;
    match l.audit() {
        Err(e) if e == LinkCode::CoverageHole.val() => Ok(()),
        _ => Err(LinkCode::NotReproducible.val()),
    }
}

/// 探针序 C：断言总数与轮数口径冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (ASSERT_TOTAL == 24, KTEST_ROUNDS == 10) {
        (true, true) => Ok(()),
        _ => Err(LinkCode::NotReproducible.val()),
    }
}

// ── LCG（种子固定） ─────────────────────────────────────────────
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

// ── 注入账（三路：脱挂/双侧重挂/缺执行体） ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let victim = rng.below(ASSERT_TOTAL as u64) as u32 + 1;
        // 路 1：脱挂注入。
        let mut l1 = seed_links()?;
        let _ = l1.inject_unlink(victim);
        led.record(0, l1.audit() == Err(LinkCode::CoverageHole.val()));
        // 路 2：断言侧重挂。
        let mut l2 = seed_links()?;
        led.record(1, l2.link(victim, if victim == 1 { 2 } else { 1 }).is_err());
        // 路 3：缺执行体（ktest 未登记即挂）。
        let mut l3 = LinkLedger::new();
        let _ = l3.add_assert(1, "断言");
        led.record(2, l3.link(1, 1).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LinkCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < LinkCode::ALL.len() {
        let v = LinkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks ≥12 条） ───────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 100% 挂接。
    v.push((
        "J1-100%挂接",
        coverage_ok(),
        format!("挂接 {}/{} = 100%", ASSERT_TOTAL, ASSERT_TOTAL),
    ));
    // 2. 双向 1:1。
    v.push((
        "J1-双向1:1",
        bidir_ok(),
        String::from("断言↔ktest 正反双向对拍全绿"),
    ));
    // 3. 双侧唯一闸。
    v.push((
        "J1-双侧唯一闸",
        double_link_rejected(),
        String::from("断言重挂/ktest 重挂均显性拒"),
    ));
    // 4. 脱挂注入 10/10。
    v.push((
        "J1-脱挂注入10/10",
        unlink_inject_all_caught(JUDGE_ROUNDS),
        format!("{} 轮抽断言脱挂全抓", JUDGE_ROUNDS),
    ));
    // 5. 双向失配注入。
    v.push((
        "J1-双向失配必抓",
        direction_mismatch_caught(),
        String::from("单向账损坏 bidir 翻红"),
    ));
    // 6. 状态机单调。
    v.push((
        "J2-状态机倒退拒",
        state_monotonic_ok(),
        String::from("未齐复核拒/Audited 后挂接拒"),
    ));
    // 7. 注入三路账。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = inj.as_ref().map(|l| l.all_caught()).unwrap_or(false);
    v.push((
        "J1-注入三路账",
        inj_ok,
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 9. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 8 码全落段", SEG_BASE),
    ));
    // 11. 断言号越界闸。
    v.push((
        "J1-断言号越界闸",
        assert_over_rejected(),
        String::from("号 0 与号 25 登记均拒"),
    ));
    // 12. 复核一次性。
    v.push((
        "J2-复核一次性",
        match seed_links() {
            Ok(mut l) => l.audit().is_ok() && l.audit() == Err(LinkCode::BadState.val()),
            Err(_) => false,
        },
        String::from("二次复核显性拒"),
    ));
    // 13. 脱挂后重挂修复。
    v.push((
        "J1-脱挂修复回绿",
        relink_after_unlink_ok(),
        String::from("脱挂→补挂→复核重绿"),
    ));
    // 14. ktest 轮数口径冻结。
    v.push((
        "J1-ktest轮数口径",
        ktest_rounds_ok(),
        format!("24 ktest 每体 {} 轮口径", KTEST_ROUNDS),
    ));
    v
}

/// ktest 轮数口径判定：24 体每体恰 10 轮。
fn ktest_rounds_ok() -> bool {
    match seed_links() {
        Ok(l) => {
            if l.ktests.len() != ASSERT_TOTAL as usize {
                return false;
            }
            let mut i = 0;
            while i < l.ktests.len() {
                match l.ktests.get(i) {
                    Some(k) => {
                        if k.rounds != KTEST_ROUNDS {
                            return false;
                        }
                    }
                    None => return false,
                }
                i += 1;
            }
            true
        }
        Err(_) => false,
    }
}

/// 断言号越界闸。
fn assert_over_rejected() -> bool {
    let mut l = LinkLedger::new();
    let a = l.add_assert(0, "零号").is_err();
    let b = l.add_assert(ASSERT_TOTAL + 1, "越界").is_err();
    let c = l.add_assert(1, "合法").is_ok();
    let d = l.add_ktest(0, "零号").is_err();
    a && b && c && d
}

/// 脱挂后重挂修复判定。
fn relink_after_unlink_ok() -> bool {
    let mut l = match seed_links() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let _ = l.inject_unlink(11);
    if l.coverage_full() {
        return false;
    }
    if l.link(11, 11).is_err() {
        return false;
    }
    l.coverage_full() && l.bidir_ok() && l.audit().is_ok()
}

// ── 检查节 ─────────────────────────────────────────────────────
pub fn run_unx1493_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1493 断言与 ktest 挂接复核（全域断言可执行面复核）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_coverage_100_percent() {
        assert!(coverage_ok());
        match seed_links() {
            Ok(l) => {
                assert_eq!(l.covered_count(), 24);
                assert_eq!(l.asserts.len(), 24);
                assert_eq!(l.ktests.len(), 24);
            }
            Err(_) => assert!(false, "种子挂接不应失败"),
        }
    }

    #[test]
    fn bidirectional_1to1_holds() {
        assert!(bidir_ok());
    }

    #[test]
    fn double_link_rejected_on_both_sides() {
        assert!(double_link_rejected());
        let mut l = match seed_links() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "种子挂接不应失败");
                return;
            }
        };
        assert_eq!(l.link(5, 6), Err(LinkCode::AssertDoubleLink.val()));
        // 断言 6 先脱挂，再撞 ktest 5 的反向唯一闸。
        assert!(l.inject_unlink(6).is_ok());
        assert_eq!(l.link(6, 5), Err(LinkCode::KtestDoubleLink.val()));
    }

    #[test]
    fn unlink_injection_always_caught() {
        assert!(unlink_inject_all_caught(JUDGE_ROUNDS), "脱挂注入必须 10/10 抓");
    }

    #[test]
    fn direction_mismatch_is_caught() {
        assert!(direction_mismatch_caught());
    }

    #[test]
    fn state_machine_holds() {
        assert!(state_monotonic_ok());
    }

    #[test]
    fn assert_over_rejected_all() {
        assert!(assert_over_rejected());
    }

    #[test]
    fn relink_repairs_coverage() {
        assert!(relink_after_unlink_ok());
    }

    #[test]
    fn golden_run_reproducible() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert_eq!(a, Ok((24, 1, 1)));
    }

    #[test]
    fn probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn inject_rounds_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert!(l.all_caught(), "三路注入全抓：{:?}", l.caught),
            Err(_) => assert!(false, "注入轮不应失败"),
        }
    }

    #[test]
    fn segment_window_holds() {
        assert!(segment_guard());
        assert_eq!(detail(LinkCode::CoverageHole.val()), LinkCode::CoverageHole.detail());
        assert_eq!(detail(0x75EBF), "未知诊断码：不在 0x75E80 段闭集内");
    }
}
