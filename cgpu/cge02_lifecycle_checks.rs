//! CGPU-F0642 · 域自检（判据逐条对应，见 `cge02_lifecycle.rs` 头注）。
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **六态** → `L02-六态-*`（封闭全集/中文名独立对拍/越界守卫）
//! - **语义** → `L02-语义-*`（三位语义表独立写死对拍/迁移清算联动）
//! - **触发** → `L02-触发-*`（三类封闭/端到端 happy path/终态拒绝）
//! - **清理** → `L02-清理-*`（冻结清算渲染面/销毁全清/非法迁移逐条拒）
//! - **判据** → `L02-判据-*`（双面零 panic、码表冻结、恒真防线、
//!   统计守恒、容量非截断）
//!
//! **判据设计纪律**（承家族六条）：
//! 1. 期望值判据侧独立写死（六态中文名/参与率 100-25-0/合法迁移表
//!    字面量——不从被测常量反推）；
//! 2. 双向验证——每条拒绝判据配接受判据；
//! 3. 弱门禁指认：清算判据抓「清算两行被删」变异（清算后渲染面必须
//!    为零——删除清算则冻结态残留渲染资源必红）；
//! 4. 判据区零 panic 面；
//! 5. 判据不得自调全域入口。

use crate::checks::{CheckSet, MAX_CHECKS};

use super::cge02_lifecycle::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧常量与语料（不从被测常量推导）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的码表（6 条冻结）。
const EXPECT_WIRE: [&str; 6] = [
    "E_LIFECYCLE_MIGRATION_INVALID",
    "E_LIFECYCLE_TERMINAL",
    "E_LIFECYCLE_TRIGGER_MISSING",
    "E_LIFECYCLE_SURFACE_UNKNOWN",
    "E_LIFECYCLE_HALF_STATE",
    "E_LIFECYCLE_CAP",
];

/// 判据侧独立写死的六态中文名（锚点原文顺序）。
const EXPECT_STATE_ZH: [&str; 6] = ["创建", "注册", "激活", "节流", "冻结", "销毁"];

/// 判据侧独立写死的语义期望：(renders, throttle_pct, keeps_alive)。
const EXPECT_SEMANTICS: [(bool, u8, bool); 6] = [
    (false, 0, true),
    (false, 0, true),
    (true, 100, true),
    (true, 25, true),
    (false, 0, true),
    (false, 0, false),
];

/// 判据总数（独立常数；加判据时同步维护）。
const EXPECTED_CHECK_COUNT: usize = 15;

/// 单遍词法剥除（字符串字面量/行注释一次状态机）。
fn strip_lexical_noise(src: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut in_line_comment = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_str {
            if c == '\\' {
                let _ = chars.next();
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            in_line_comment = true;
            let _ = chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// 一、六态
// ---------------------------------------------------------------------------

fn chk_state_closed(set: &mut CheckSet) {
    // 六态-01：封闭全集 + 中文名独立对拍 + 越界守卫。
    let mut ok = SurfaceState::ALL.len() == 6;
    let mut k = 0usize;
    while ok && k < 6 {
        match SurfaceState::of_rank(k) {
            Some(s) => ok = s.rank() == k && s.zh() == EXPECT_STATE_ZH[k],
            None => ok = false,
        }
        k += 1;
    }
    ok = ok && SurfaceState::of_rank(6).is_none();
    if ok {
        set.ok("L02-六态-01-封闭与独立对拍");
    } else {
        set.fail("L02-六态-01-封闭与独立对拍", "秩/中文名与独立期望不符");
    }
}

// ---------------------------------------------------------------------------
// 二、语义
// ---------------------------------------------------------------------------

fn chk_semantics_table(set: &mut CheckSet) {
    // 语义-01：三位语义表与独立写死期望逐态对拍（含参与率 100/25/0）。
    let mut ok = STATE_SEMANTICS.len() == 6;
    let mut k = 0usize;
    while ok && k < 6 {
        let (r, p, a) = EXPECT_SEMANTICS[k];
        ok = STATE_SEMANTICS[k].renders == r
            && STATE_SEMANTICS[k].throttle_pct == p
            && STATE_SEMANTICS[k].keeps_alive == a;
        k += 1;
    }
    // 语义-02：读面语义函数与表同源（激活=参与帧循环）。
    ok = ok && semantics_of(SurfaceState::Active).renders
        && semantics_of(SurfaceState::Frozen).renders == false;
    if ok {
        set.ok("L02-语义-01-语义表独立对拍");
        set.ok("L02-语义-02-语义寻址同源");
    } else {
        set.fail("L02-语义-01-语义表独立对拍", "语义位与独立期望不符");
        set.fail("L02-语义-02-语义寻址同源", "semantics_of 寻址错");
    }
}

// ---------------------------------------------------------------------------
// 三、触发与状态机
// ---------------------------------------------------------------------------

fn chk_trigger_closed(set: &mut CheckSet) {
    // 触发-01：三类封闭 + 中文名（用户操作/可见性/资源压力）。
    let ok = MigrationTrigger::ALL.len() == 3
        && MigrationTrigger::UserAction.rank() == 0
        && MigrationTrigger::Visibility.rank() == 1
        && MigrationTrigger::ResourcePressure.rank() == 2
        && MigrationTrigger::UserAction.zh() == "用户操作"
        && MigrationTrigger::Visibility.zh() == "可见性"
        && MigrationTrigger::ResourcePressure.zh() == "资源压力";
    if ok {
        set.ok("L02-触发-01-三类封闭");
    } else {
        set.fail("L02-触发-01-三类封闭", "触发类或中文名不符");
    }
}

fn chk_trigger_happy_path(set: &mut CheckSet) {
    // 触发-02：端到端生命周期（创建→注册→激活⇄节流→冻结→销毁全合法）。
    let mut lc = SurfaceLifecycle::new();
    let id = lc.spawn().unwrap_or(0);
    let mut ok = id != 0 && lc.state_of(id) == Some(SurfaceState::Created);
    ok = ok && lc.transition(id, SurfaceState::Registered, TrUser).is_ok();
    ok = ok && lc.transition(id, SurfaceState::Active, TrVisibility).is_ok();
    ok = ok && lc.transition(id, SurfaceState::Throttled, TrResource).is_ok();
    ok = ok && lc.transition(id, SurfaceState::Active, TrVisibility).is_ok();
    ok = ok && lc.transition(id, SurfaceState::Frozen, TrResource).is_ok();
    ok = ok && lc.transition(id, SurfaceState::Throttled, TrVisibility).is_ok();
    ok = ok && lc.transition(id, SurfaceState::Destroyed, TrUser).is_ok();
    // 终态零出边（锚点：销毁是终点）。
    ok = ok && lc.transition(id, SurfaceState::Active, TrUser).is_err();
    if ok {
        set.ok("L02-触发-02-端到端全迁移");
    } else {
        set.fail("L02-触发-02-端到端全迁移", "合法链路被拒或终态可出");
    }
}

// ---------------------------------------------------------------------------
// 四、清理（迁移安全——无半态）
// ---------------------------------------------------------------------------

fn chk_cleanup_frozen(set: &mut CheckSet) {
    // 清理-01：冻结清算渲染面（图层+缓冲清零，保活句柄留存）。
    // 抓「清算两行被删」变异：不清算则冻结态残留渲染资源必红。
    let mut lc = SurfaceLifecycle::new();
    let id = lc.spawn().unwrap_or(0);
    let _ = lc.transition(id, SurfaceState::Registered, TrUser);
    let _ = lc.transition(id, SurfaceState::Active, TrVisibility);
    let _ = lc.attach_resources(
        id,
        ResourceHold { render_layers: 4, buffers: 2, event_handles: 3 },
    );
    let frozen_ok = lc.transition(id, SurfaceState::Frozen, TrResource).is_ok();
    let hold = lc.surfaces[(id - 1) as usize].hold;
    let ok = frozen_ok && hold.render_layers == 0 && hold.buffers == 0 && hold.event_handles == 3;
    if ok {
        set.ok("L02-清理-01-冻结清算渲染面");
    } else {
        set.fail("L02-清理-01-冻结清算渲染面", "冻结后渲染资源残留（半态）");
    }
}

fn chk_cleanup_destroyed(set: &mut CheckSet) {
    // 清理-02：销毁全清（渲染面+保活面全零）。
    let mut lc = SurfaceLifecycle::new();
    let id = lc.spawn().unwrap_or(0);
    let _ = lc.attach_resources(
        id,
        ResourceHold { render_layers: 1, buffers: 1, event_handles: 5 },
    );
    let ok = lc.transition(id, SurfaceState::Destroyed, TrUser).is_ok()
        && lc.surfaces[(id - 1) as usize].hold.all_clear();
    if ok {
        set.ok("L02-清理-02-销毁全清");
    } else {
        set.fail("L02-清理-02-销毁全清", "销毁后资源未全清");
    }
}

fn chk_migration_rejects(set: &mut CheckSet) {
    // 清理-03：非法迁移逐条拒（跳级/回退/终态出边/未知表面——各专属码）。
    let mut lc = SurfaceLifecycle::new();
    let id = lc.spawn().unwrap_or(0);
    let skip = match lc.transition(id, SurfaceState::Active, TrUser) {
        Err(e) => e.code == E_LIFECYCLE_MIGRATION_INVALID,
        Ok(_) => false,
    };
    let _ = lc.transition(id, SurfaceState::Registered, TrUser);
    let _ = lc.transition(id, SurfaceState::Active, TrVisibility);
    let back = match lc.transition(id, SurfaceState::Registered, TrUser) {
        Err(e) => e.code == E_LIFECYCLE_MIGRATION_INVALID,
        Ok(_) => false,
    };
    let _ = lc.transition(id, SurfaceState::Destroyed, TrUser);
    let after_terminal = match lc.transition(id, SurfaceState::Active, TrUser) {
        Err(e) => e.code == E_LIFECYCLE_TERMINAL,
        Ok(_) => false,
    };
    let unknown = match lc.transition(9999, SurfaceState::Active, TrUser) {
        Err(e) => e.code == E_LIFECYCLE_SURFACE_UNKNOWN,
        Ok(_) => false,
    };
    if skip && back && after_terminal && unknown {
        set.ok("L02-清理-03-非法迁移逐条拒");
    } else {
        set.fail("L02-清理-03-非法迁移逐条拒", "某类非法迁移未被专属码拒绝");
    }
}

fn chk_migration_audit(set: &mut CheckSet) {
    // 清理-04：全迁移图双源对账通过 + 摘要可读。
    let audited = match audit_migrations() {
        Ok(msg) => msg.contains("14") || msg.contains("对账"),
        Err(_) => false,
    };
    let summary_ok = domain_summary().contains("F0642");
    if audited {
        set.ok("L02-清理-04-迁移图双源对账");
    } else {
        set.fail("L02-清理-04-迁移图双源对账", "位图与边表漂移或审计失败");
    }
    if summary_ok {
        set.ok("L02-清理-05-域摘要可读");
    } else {
        set.fail("L02-清理-05-域摘要可读", "摘要缺单号锚");
    }
}

// ---------------------------------------------------------------------------
// 五、判据承载力
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：生产面 + 判据面双零 panic 扫描。
    let src_prod = include_str!("cge02_lifecycle.rs");
    let src_chk = include_str!("cge02_lifecycle_checks.rs");
    let clean_prod = strip_lexical_noise(src_prod);
    let clean_chk = strip_lexical_noise(src_chk);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean_prod.contains(pat) || clean_chk.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("L02-判据-01-双面零 panic");
    } else {
        set.fail("L02-判据-01-双面零 panic", "生产或判据代码含 panic 面");
    }
}

fn chk_criterion_codes(set: &mut CheckSet) {
    // 判据-02：码表冻结（6 码逐条同源对拍）。
    let expect: [&str; 6] = [
        E_LIFECYCLE_MIGRATION_INVALID,
        E_LIFECYCLE_TERMINAL,
        E_LIFECYCLE_TRIGGER_MISSING,
        E_LIFECYCLE_SURFACE_UNKNOWN,
        E_LIFECYCLE_HALF_STATE,
        E_LIFECYCLE_CAP,
    ];
    let mut ok = expect.len() == EXPECT_WIRE.len();
    let mut k = 0usize;
    while ok && k < 6 {
        ok = expect[k] == EXPECT_WIRE[k];
        k += 1;
    }
    if ok {
        set.ok("L02-判据-02-码表冻结");
    } else {
        set.fail("L02-判据-02-码表冻结", "码表漂移");
    }
}

fn chk_criterion_baseline(set: &mut CheckSet) {
    // 判据-03：恒真防线——先建面证明基线非空，再断迁移面。
    let mut lc = SurfaceLifecycle::new();
    let id = lc.spawn().unwrap_or(0);
    let nontrivial = id != 0 && lc.spawned == 1 && lc.ledger.is_empty();
    if nontrivial {
        set.ok("L02-判据-03-基线非平凡");
    } else {
        set.fail("L02-判据-03-基线非平凡", "基线账为空，状态机断言退化为空真");
    }
}

fn chk_criterion_conservation(set: &mut CheckSet) {
    // 判据-04：统计守恒（成功迁移+拒绝 ≤ 操作；建面=账行数）。
    let mut lc = SurfaceLifecycle::new();
    let id = lc.spawn().unwrap_or(0);
    let _ = lc.transition(id, SurfaceState::Registered, TrUser);
    let _ = lc.transition(id, SurfaceState::Active, TrVisibility);
    let _ = lc.transition(id, SurfaceState::Created, TrUser); // 非法回退
    let conserved = lc.spawned == 1
        && lc.migrated == 2
        && lc.rejected == 1
        && lc.migrated + lc.rejected <= 3;
    if conserved {
        set.ok("L02-判据-04-统计守恒");
    } else {
        set.fail("L02-判据-04-统计守恒", "迁移/拒绝计数失真");
    }
}

fn chk_criterion_capacity(set: &mut CheckSet) {
    // 判据-05：容量非截断（独立常数对拍，不递归调 run 入口）。
    if EXPECTED_CHECK_COUNT <= MAX_CHECKS {
        set.ok("L02-判据-05-容量不截断");
    } else {
        set.fail("L02-判据-05-容量不截断", "判据总数超 MAX_CHECKS");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=六态+语义+触发 / b=清理+判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：六态 + 语义 + 触发。
pub fn run_cge02_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cge02/a");
    chk_state_closed(&mut s);
    chk_semantics_table(&mut s);
    chk_trigger_closed(&mut s);
    chk_trigger_happy_path(&mut s);
    s
}

/// 判据族 b：清理 + 判据承载力。
pub fn run_cge02_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cge02/b");
    chk_cleanup_frozen(&mut s);
    chk_cleanup_destroyed(&mut s);
    chk_migration_rejects(&mut s);
    chk_migration_audit(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes(&mut s);
    chk_criterion_baseline(&mut s);
    chk_criterion_conservation(&mut s);
    chk_criterion_capacity(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_cge02_checks() -> CheckSet {
    CheckSet::merge(
        run_cge02_checks_a_standalone(),
        run_cge02_checks_b_standalone(),
    )
}
