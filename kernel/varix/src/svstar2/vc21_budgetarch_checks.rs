//! CGPU-F0321 域自检（CGPU-C 域 · 帧预算仲裁总架构判据层）
//!
//! 判据侧**独立写死**期望（12.5ms/80fps、预算守恒恒等式、八账页序、
//! 绑定面四名、码段 0x21 四码），不复用实现侧常量。聚合防自调：族内
//! 判据只调另一族 standalone + 进行中 set 自身 tally。

use crate::checks::CheckSet;

use super::vc21_budgetarch as ba;
use super::vc21_budgetarch::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_FRAME_US: u32 = 12_500;
const EXPECT_FPS: u32 = 80;
const EXPECT_CODES: [u16; 4] = [0x2100, 0x2101, 0x2102, 0x2103];
const EXPECT_HANDOFF_COUNT: usize = 4;
const EXPECT_MODULE_COUNT: usize = 8;

// ---------------------------------------------------------------------------
// 一、规格（八模块 / 预算守恒 / 仲裁拒绝 / 超时与交易 / 再平衡 / 移交核验）
// ---------------------------------------------------------------------------

fn chk_spec_eight_modules(set: &mut CheckSet) {
    // 规格-01：八模块封闭全集——八账页序往返 + 越界 None。
    let mut rt = true;
    let mut i = 0usize;
    while i < EXPECT_MODULE_COUNT {
        match BudgetModule::of_ordinal(i) {
            Some(m) => {
                if m.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    let out_of_range = BudgetModule::of_ordinal(8).is_none();
    if rt && out_of_range && BudgetModule::ALL.len() == EXPECT_MODULE_COUNT {
        set.ok("EC21-规格-01-八模块封闭往返");
    } else {
        set.fail("EC21-规格-01-八模块封闭往返", "账页序漂移或越界未拒");
    }
    // 规格-02：80 帧承诺——帧预算与帧率互洽（判据侧独立写死对拍）。
    if FRAME_BUDGET_US == EXPECT_FRAME_US
        && FRAME_PROMISE_FPS == EXPECT_FPS
        && EXPECT_FRAME_US * EXPECT_FPS == 1_000_000
    {
        set.ok("EC21-规格-02-80帧承诺互洽");
    } else {
        set.fail("EC21-规格-02-80帧承诺互洽", "预算/帧率与 80 帧承诺失配");
    }
    // 规格-03：八模块开帧全零账（new_frame 八账清零）。
    let a = Arbiter::new_frame();
    let mut all_zero = true;
    let mut i = 0usize;
    while i < 8 {
        let l = a.ledger_of(BudgetModule::of_ordinal(i).unwrap_or(BudgetModule::Model));
        if !(l.granted_us == 0 && l.used_us == 0 && l.grants == 0) {
            all_zero = false;
        }
        i += 1;
    }
    if all_zero && a.frame_remaining() == EXPECT_FRAME_US as i64 {
        set.ok("EC21-规格-03-开帧八账全零");
    } else {
        set.fail("EC21-规格-03-开帧八账全零", "开帧账未清或预算未回满");
    }
}

fn chk_spec_grant_and_reject(set: &mut CheckSet) {
    // 规格-04：仲裁授予——授予后账页与全局剩余同步（精确分配可核对）。
    let mut a = Arbiter::new_frame();
    let req = BudgetRequest { module: BudgetModule::Arbitrate, amount_us: 3_000, priority: 0 };
    let g = a.grant(&req);
    let ok = g == Ok(3_000)
        && a.frame_remaining() == (EXPECT_FRAME_US - 3_000) as i64
        && a.ledger_of(BudgetModule::Arbitrate).granted_us == 3_000;
    if ok {
        set.ok("EC21-规格-04-授予同步全局剩余");
    } else {
        set.fail("EC21-规格-04-授予同步全局剩余", "授予后账页/全局剩余失守");
    }
    // 规格-05：超预算显式拒绝——申请超过剩余被拒且拒绝计数（不静默给满）。
    let mut a = Arbiter::new_frame();
    let big = BudgetRequest { module: BudgetModule::Model, amount_us: EXPECT_FRAME_US + 1, priority: 0 };
    let rejected = a.grant(&big);
    let ok = rejected == Err(E_B021_OVER_BUDGET)
        && a.rejections == 1
        && a.frame_remaining() == EXPECT_FRAME_US as i64;
    if ok {
        set.ok("EC21-规格-05-超预算显式拒绝");
    } else {
        set.fail("EC21-规格-05-超预算显式拒绝", "超预算未拒或静默给满");
    }
    // 规格-06：重复授予拒绝——同帧同模块二次授予 0x2103。
    let mut a = Arbiter::new_frame();
    let req = BudgetRequest { module: BudgetModule::Trade, amount_us: 1_000, priority: 1 };
    let first = a.grant(&req);
    let second = a.grant(&req);
    let ok = first.is_ok() && second == Err(E_B021_DOUBLE_GRANT);
    if ok {
        set.ok("EC21-规格-06-重复授予拒绝");
    } else {
        set.fail("EC21-规格-06-重复授予拒绝", "同模块重复授予未拒");
    }
}

fn chk_spec_timeout_trade(set: &mut CheckSet) {
    // 规格-07：超时处置——回收未用授予、全局剩余回补、计数递增。
    let mut a = Arbiter::new_frame();
    let _ = a.grant(&BudgetRequest { module: BudgetModule::Timeout, amount_us: 2_000, priority: 0 });
    a.consume(BudgetModule::Timeout, 500);
    let reclaimed = a.timeout_reclaim(BudgetModule::Timeout);
    let ok = reclaimed == 1_500
        && a.ledger_of(BudgetModule::Timeout).reclaimed_us == 1_500
        && a.frame_remaining() == (EXPECT_FRAME_US - 500) as i64
        && a.timeout_reclaims == 1;
    if ok {
        set.ok("EC21-规格-07-超时回收回补");
    } else {
        set.fail("EC21-规格-07-超时回收回补", "回收额/回补/计数失守");
    }
    // 规格-08：交易——转出方剩余足额才成、双账页净额镜像、拒绝计数。
    let mut a = Arbiter::new_frame();
    let _ = a.grant(&BudgetRequest { module: BudgetModule::Trade, amount_us: 2_000, priority: 0 });
    let ok_trade = a.trade(BudgetModule::Trade, BudgetModule::Rebalance, 800);
    let mirror = a.ledger_of(BudgetModule::Trade).traded_net_us == -800
        && a.ledger_of(BudgetModule::Rebalance).traded_net_us == 800;
    let over = a.trade(BudgetModule::Trade, BudgetModule::Rebalance, 99_999);
    let ok = ok_trade.is_ok() && mirror && over == Err(E_B021_TRADE_OVER);
    if ok {
        set.ok("EC21-规格-08-交易镜像与足额闸");
    } else {
        set.fail("EC21-规格-08-交易镜像与足额闸", "交易未镜像或足额闸失效");
    }
    // 规格-09：再平衡——回收池整体注入缺口模块；无可回收则 0（不变出预算）。
    let mut a = Arbiter::new_frame();
    let zero = a.rebalance(BudgetModule::Request);
    let _ = a.grant(&BudgetRequest { module: BudgetModule::Timeout, amount_us: 1_000, priority: 0 });
    let _ = a.timeout_reclaim(BudgetModule::Timeout);
    let pool = a.rebalance(BudgetModule::Request);
    let ok = zero == 0
        && pool == 1_000
        && a.ledger_of(BudgetModule::Request).traded_net_us == 1_000
        && a.rebalances == 1;
    if ok {
        set.ok("EC21-规格-09-再平衡回收注入");
    } else {
        set.fail("EC21-规格-09-再平衡回收注入", "再平衡注入额或零回收语义漂移");
    }
}

fn chk_spec_handoff_and_service(set: &mut CheckSet) {
    // 规格-10：B10 移交核验就位——四面对接全核验，名册与核验账逐位对拍。
    let checks = handoff_checks();
    let mut all_ok = checks.len() == EXPECT_HANDOFF_COUNT;
    let mut i = 0usize;
    while i < checks.len() && i < HANDOFF_SURFACE.len() {
        if !checks[i].verified || checks[i].surface != HANDOFF_SURFACE[i] {
            all_ok = false;
        }
        i += 1;
    }
    if all_ok {
        set.ok("EC21-规格-10-移交核验就位");
    } else {
        set.fail("EC21-规格-10-移交核验就位", "对接面缺失或核验未过");
    }
    // 规格-11：服务化纪律——仲裁在用户态、内核态唯一动作是到期强制让出，
    // 阈值内不动（内核面最小化：无仲裁逻辑）。
    let below = enforcement_point(EXPECT_FRAME_US - 1);
    let at = enforcement_point(EXPECT_FRAME_US);
    let service_ok = SERVICE_MODE == "userland-arbiter";
    let enforce_ok = below.is_none()
        && matches!(at, Some(EnforcementAction::YieldOnExpiry));
    if service_ok && enforce_ok {
        set.ok("EC21-规格-11-服务化纪律内核面最小");
    } else {
        set.fail("EC21-规格-11-服务化纪律内核面最小", "执行点越权仲裁或阈值翻面失效");
    }
}

// ---------------------------------------------------------------------------
// 二、幂等 / 确定性
// ---------------------------------------------------------------------------

fn chk_idem_determinism(set: &mut CheckSet) {
    // 幂等-01：预算守恒恒等式——授予 + 剩余恒等于帧预算（任何操作序列后）。
    let mut a = Arbiter::new_frame();
    let _ = a.grant(&BudgetRequest { module: BudgetModule::Model, amount_us: 2_000, priority: 0 });
    let _ = a.grant(&BudgetRequest { module: BudgetModule::Trade, amount_us: 1_500, priority: 1 });
    a.consume(BudgetModule::Model, 1_200);
    let _ = a.timeout_reclaim(BudgetModule::Trade);
    let _ = a.trade(BudgetModule::Model, BudgetModule::Request, 300);
    let mut granted_total: i64 = 0;
    let mut i = 0usize;
    while i < 8 {
        let l = a.ledger_of(BudgetModule::of_ordinal(i).unwrap_or(BudgetModule::Model));
        granted_total += l.granted_us as i64 + l.traded_net_us as i64 - l.reclaimed_us as i64;
        i += 1;
    }
    let conserved = granted_total + a.frame_remaining() == EXPECT_FRAME_US as i64;
    if conserved {
        set.ok("EC21-幂等-01-预算守恒恒等式");
    } else {
        set.fail("EC21-幂等-01-预算守恒恒等式", "操作序列后预算失守");
    }
    // 幂等-02：仲裁确定性——同申请序列对独立帧产生同账页快照。
    let run = || {
        let mut a = Arbiter::new_frame();
        let _ = a.grant(&BudgetRequest { module: BudgetModule::Model, amount_us: 2_000, priority: 0 });
        let _ = a.grant(&BudgetRequest { module: BudgetModule::Trade, amount_us: 1_500, priority: 1 });
        a.consume(BudgetModule::Model, 500);
        a
    };
    let a1 = run();
    let a2 = run();
    let same = a1.ledger_of(BudgetModule::Model) == a2.ledger_of(BudgetModule::Model)
        && a1.frame_remaining() == a2.frame_remaining();
    if same {
        set.ok("EC21-幂等-02-仲裁确定性");
    } else {
        set.fail("EC21-幂等-02-仲裁确定性", "同序列不同账");
    }
    // 幂等-03：未知模块防御面——of_ordinal 越界 None 已查（规格-01），
    // 此处断 grant 拒绝路径计数独立（拒绝不改账页）。
    let mut a = Arbiter::new_frame();
    let _ = a.grant(&BudgetRequest { module: BudgetModule::Model, amount_us: 99_999, priority: 0 });
    let untouched = a.ledger_of(BudgetModule::Model).grants == 0
        && a.ledger_of(BudgetModule::Model).granted_us == 0;
    if untouched {
        set.ok("EC21-幂等-03-拒绝不改账");
    } else {
        set.fail("EC21-幂等-03-拒绝不改账", "被拒申请污染账页");
    }
}

// ---------------------------------------------------------------------------
// 三、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vc21_budgetarch_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EC21-判据-01-判据面零 panic");
    } else {
        set.fail("EC21-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-12：生产面零 panic（扫实现文件）。
    let src = include_str!("vc21_budgetarch.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EC21-规格-12-生产面零 panic");
    } else {
        set.fail("EC21-规格-12-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——四码皆 0x21 细分段、互异、与写死值逐位等。
    let got = [E_B021_OVER_BUDGET, E_B021_UNKNOWN_MODULE, E_B021_TRADE_OVER, E_B021_DOUBLE_GRANT];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x2100 || got[i] != EXPECT_CODES[i] {
            ok = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("EC21-判据-02-码段独占独立复核");
    } else {
        set.fail("EC21-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 14 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vc21_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 14;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EC21-判据-03-聚合守恒防自调");
    } else {
        set.fail("EC21-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+幂等 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 幂等。
pub fn run_vc21_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vc21/a");
    chk_spec_eight_modules(&mut s);
    chk_spec_grant_and_reject(&mut s);
    chk_spec_timeout_trade(&mut s);
    chk_spec_handoff_and_service(&mut s);
    chk_idem_determinism(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vc21_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vc21/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vc21_checks() -> CheckSet {
    CheckSet::merge(run_vc21_checks_a_standalone(), run_vc21_checks_b_standalone())
}
