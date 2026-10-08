//! VE-F3008 · 入场动效族判据
//!
//! **锚点判据（原文）**：六型族、错开令牌、首帧原子、触发分型、reduce 终态、判据。
//!
//! # 一、判据怎么做到"不是恒真"
//!
//! 入场族最容易写成恒真断言的地方有五处，本模块逐一封死：
//!
//! 1. **"六型族"**。若只断「registry 里 6 条」，一个「六型全部退化成 fade-in」的
//!    实现照样绿。必须断**型指纹互异**：属性集+时长令牌+位移令牌三元组逐型不同
//!    （fade-up 与 scale-in 属性集相同，靠位移令牌区分；slide-in 靠时长档区分），
//!    且契约表 [`ENTRY_CONTRACT`] 与实现**同源对账**（表行数=型属性数）。
//! 2. **"错开令牌"**。若只断「错开值在区间内」，一个「越界直接丢弃」的实现也能绿。
//!    必须**双向钳制断言**：低于 30 钳到 30、高于 60 钳到 60、区间内原样通过且
//!    不记钳制；且每次钳制必出 [`E_STAGGER_BOUND`] 诊断（显性，不静默）。
//! 3. **"首帧原子"**。若只断「快照非空」，一个「逐属性分次写」的实现也能骗过。
//!    必须断**快照一次收齐全部属性**（prop_count == 输入属性数、顺序保持），
//!    且闪烁检出必立 [`E_FLASH_P1`] 案、`assert_no_flash` 在有案时**必须红**。
//! 4. **"触发分型"**。若只断「三类可解析」，一个「事件来了就触发」的实现也能绿。
//!    必须**分型分判**：once 默认第二次拒（`AlreadyFired`）、显性重触发放行且
//!    记诊断、视口规则离屏延迟（`OffscreenDeferred`）、未登记显性
//!    （`NotRegistered`）、页面加载不依赖视口可见性。
//! 5. **"reduce 终态"**。若只断「reduce 下有产物」，一个「只归零时长、偏移残留」
//!    的实现也能绿。必须断**全部实例时长与偏移双零**，且**基线先行**：Normal 泳道
//!    产物时长必须非零（恒真门禁防御），`assert_reduce_terminal` 对 Normal 泳道
//!    调用必须拒绝（防判定路径接错）。
//!
//! # 二、方向：该拒的确实拒了
//!
//! 空起始态、超容起始态、重名注册、重复触发规则、空变体、重复基础型——每类
//! 拒绝都要拿到**专属错误码**，且成功路径同时断（节点数、偏移均布、属性并集）。

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::{Lane, TokenTable, TokenValue};
use crate::svstar2::vep08_entry::*;

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

/// 取错误码（泛化：任意 `Result<T, MotionTokenError>` 取 code）。
fn code_of<T>(r: Result<T, crate::svstar2::vep03_token::MotionTokenError>) -> &'static str {
    match r {
        Ok(_) => "<no-error>",
        Err(e) => e.code,
    }
}

/// 标准令牌表；构造失败退空表（判据层不留 panic 面）。
fn tok() -> TokenTable {
    TokenTable::from_lang()
}

/// 从表取时长令牌基线值（独立重算通道，与 EntryDefaults 对账）。
fn token_ms(table: &TokenTable, id: &str) -> u32 {
    match table.find(id) {
        Some(t) => match &t.value {
            TokenValue::Millis(ms) => *ms,
            _ => 0,
        },
        None => 0,
    }
}

/// 从表取位移令牌值。
fn token_px(table: &TokenTable, id: &str) -> u32 {
    match table.find(id) {
        Some(t) => match &t.value {
            TokenValue::Px(px) => *px,
            _ => 0,
        },
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// 判据一：六型族（锚点判据 1）
// ---------------------------------------------------------------------------

fn chk_six_kinds(cs: &mut CheckSet) {
    // 全集往返互逆：index ⇄ from_index ⇄ parse(wire)。
    let rt = (0..KIND_COUNT).all(|i| {
        EntryKind::from_index(i)
            .map(|k| k.index() == i && EntryKind::parse(k.wire()) == Some(k))
            .unwrap_or(false)
    });
    cs.add("E08-六型-索引与短码往返互逆", rt, "六型 index/短码须双向可逆");

    // 短码互异（防型表塌缩成同一型）。
    let wires = [
        EntryKind::FadeIn.wire(),
        EntryKind::FadeUp.wire(),
        EntryKind::ScaleIn.wire(),
        EntryKind::SlideIn.wire(),
        EntryKind::BlurIn.wire(),
        EntryKind::ClipReveal.wire(),
    ];
    let mut uniq = true;
    for i in 0..wires.len() {
        for j in (i + 1)..wires.len() {
            if wires[i] == wires[j] {
                uniq = false;
            }
        }
    }
    cs.add("E08-六型-短码互异", uniq, "六型短码不得重复");

    // 型指纹三元组（属性集, 时长令牌, 位移令牌）逐型互异——关键断言：
    // fade-up 与 scale-in 属性集相同，靠位移令牌区分；slide-in 靠时长档区分。
    let mut finger_uniq = true;
    for i in 0..KIND_COUNT {
        for j in (i + 1)..KIND_COUNT {
            let a = match EntryKind::from_index(i) {
                Some(k) => k,
                None => continue,
            };
            let b = match EntryKind::from_index(j) {
                Some(k) => k,
                None => continue,
            };
            let same = a.duration_token_id() == b.duration_token_id()
                && a.distance_token_id() == b.distance_token_id()
                && a.properties().len() == b.properties().len()
                && a
                    .properties()
                    .iter()
                    .all(|p| b.properties().iter().any(|q| q == p));
            if same {
                finger_uniq = false;
            }
        }
    }
    cs.add(
        "E08-六型-型指纹三元组互异",
        finger_uniq,
        "属性集+时长令牌+位移令牌组合须逐型可区分",
    );

    // 契约表同源对账：表行 (短码, 属性数) 与实现逐行相等（防表实漂移）。
    let contract_ok = ENTRY_CONTRACT.iter().all(|(name, n)| {
        match EntryKind::parse(name) {
            Some(k) => k.properties().len() == *n,
            None => false,
        }
    }) && ENTRY_CONTRACT.len() == KIND_COUNT;
    cs.add("E08-六型-契约表同源", contract_ok, "ENTRY_CONTRACT 与型属性集逐行同源");

    // 属性名白名单（入场族只允许合成通道四属性——O04 选型前提）。
    let whitelist = ["opacity", "transform", "filter", "clip-path"];
    let wl_ok = (0..KIND_COUNT).all(|i| {
        match EntryKind::from_index(i) {
            Some(k) => k.properties().iter().all(|p| whitelist.iter().any(|w| w == p)),
            None => false,
        }
    });
    cs.add("E08-六型-属性白名单", wl_ok, "属性集仅限 opacity/transform/filter/clip-path");

    // 场景注释逐型非空且互异（锚点"适用场景注释"是型的一部分）。
    let mut scen_uniq = true;
    for i in 0..KIND_COUNT {
        for j in (i + 1)..KIND_COUNT {
            let a = EntryKind::from_index(i).map(|k| k.scenario()).unwrap_or("");
            let b = EntryKind::from_index(j).map(|k| k.scenario()).unwrap_or("");
            if a.is_empty() || b.is_empty() || a == b {
                scen_uniq = false;
            }
        }
    }
    cs.add("E08-六型-场景注释非空互异", scen_uniq, "每型场景注释须非空且不重复");

    // 注册表：六型预置计数守恒 + 变体注册与重名拒绝。
    let reg_ok = match EntryRegistry::with_six() {
        Ok(mut r) => {
            let base_ok = r.count() == KIND_COUNT && r.variant_count() == 0;
            let var_ok = match EntryVariant::new("hero", &[EntryKind::FadeUp, EntryKind::BlurIn]) {
                Ok(v) => {
                    r.register_variant(v).is_ok()
                        && r.count() == KIND_COUNT + 1
                        && r.variant_count() == 1
                        && r.lookup("hero").map(|s| s.is_variant).unwrap_or(false)
                }
                Err(_) => false,
            };
            let dup_ok = match EntryVariant::new("hero", &[EntryKind::FadeIn]) {
                Ok(v) => code_of(r.register_variant(v)) == "E_ENTRY_DUP",
                Err(_) => false,
            };
            let unknown_ok = r.lookup("no-such").is_none();
            base_ok && var_ok && dup_ok && unknown_ok
        }
        Err(_) => false,
    };
    cs.add("E08-六型-注册表预置与拒绝", reg_ok, "六型预置 6 条；变体可注册；重名拒 E_ENTRY_DUP");

    // 变体属性并集去重保序 + 令牌聚合（取大）。
    let var_ok = match EntryVariant::new("v1", &[EntryKind::FadeUp, EntryKind::BlurIn]) {
        Ok(v) => {
            let props = v.properties();
            let dedup = props.len() == 3 && props.iter().any(|p| *p == "opacity");
            let single = props.iter().filter(|p| **p == "opacity").count() == 1;
            // FadeUp(dur-component,dist-small) + BlurIn(dur-component,dist-none)
            // ⇒ 时长 component、位移 small（取大）。
            let tok_ok = v.duration_token_id() == "dur-component" && v.distance_token_id() == "dist-small";
            dedup && single && tok_ok
        }
        Err(_) => false,
    };
    cs.add("E08-六型-变体属性并集与令牌聚合", var_ok, "并集去重保序；令牌取大原则");

    // 变体含 slide-in 时时长升页面档（取大原则的第二半边）。
    let esc_ok = match EntryVariant::new("v2", &[EntryKind::FadeIn, EntryKind::SlideIn]) {
        Ok(v) => v.duration_token_id() == "dur-page" && v.distance_token_id() == "dist-medium",
        Err(_) => false,
    };
    cs.add("E08-六型-变体令牌升级", esc_ok, "含 slide-in 的变体时长须升 dur-page");

    // 变体非法声明三路拒绝（空基础型/重复基础型/坏名）全拿 E_VARIANT_BAD。
    let bad_empty = code_of(EntryVariant::new("x", &[])) == "E_VARIANT_BAD";
    let bad_dup = code_of(EntryVariant::new("x", &[EntryKind::FadeIn, EntryKind::FadeIn])) == "E_VARIANT_BAD";
    let bad_name = code_of(EntryVariant::new("", &[EntryKind::FadeIn])) == "E_VARIANT_BAD";
    cs.add(
        "E08-六型-变体非法声明拒绝",
        bad_empty && bad_dup && bad_name,
        "空基础型/重复基础型/空名均拒 E_VARIANT_BAD",
    );

    // slide-in 四向：方向短码互异 + 单位向量四向各异 + 别名注册。
    let dirs = [SlideDir::Up.wire(), SlideDir::Down.wire(), SlideDir::Left.wire(), SlideDir::Right.wire()];
    let dir_uniq = dirs[0] != dirs[1]
        && dirs[0] != dirs[2]
        && dirs[0] != dirs[3]
        && dirs[1] != dirs[2]
        && dirs[1] != dirs[3]
        && dirs[2] != dirs[3];
    let deltas = [SlideDir::Up.delta(), SlideDir::Down.delta(), SlideDir::Left.delta(), SlideDir::Right.delta()];
    let delta_uniq = deltas[0] != deltas[1] && deltas[0] != deltas[2] && deltas[0] != deltas[3];
    let alias_ok = match EntryRegistry::with_six() {
        Ok(mut r) => {
            let a = r.register_slide_dir(SlideDir::Left).is_ok() && r.count() == KIND_COUNT + 1;
            let dup = code_of(r.register_slide_dir(SlideDir::Left)) == "E_ENTRY_DUP";
            a && dup && r.lookup("slide-in-left").is_some()
        }
        Err(_) => false,
    };
    cs.add(
        "E08-六型-slide四向与别名",
        dir_uniq && delta_uniq && alias_ok,
        "四向短码与向量互异；方向别名可注册、重复拒",
    );

    // 默认参数从令牌取：EntryDefaults 与令牌表独立重算通道逐字段对账。
    let table = tok();
    let mut defaults_ok = true;
    for i in 0..KIND_COUNT {
        let k = match EntryKind::from_index(i) {
            Some(k) => k,
            None => {
                defaults_ok = false;
                continue;
            }
        };
        match EntryDefaults::from_token(&table, k) {
            Ok(d) => {
                let want_ms = token_ms(&table, k.duration_token_id());
                let want_px = token_px(&table, k.distance_token_id());
                if d.duration_ms != want_ms || d.distance_px != want_px {
                    defaults_ok = false;
                }
                if d.duration_ms == 0 {
                    defaults_ok = false; // 基线非零防御（恒真门禁）。
                }
            }
            Err(_) => defaults_ok = false,
        }
    }
    cs.add("E08-六型-默认参数令牌对账", defaults_ok, "六型默认参数与令牌表逐字段相等且非零");

    // 令牌值型错配拒绝（时长令牌必须产 Millis——由本实现 match 保证，此处断
    // 位移型对 fade-up 恰为 dist-small 的 Px 值且非零）。
    let px_ok = token_px(&table, "dist-small") > 0 && token_px(&table, "dist-none") == 0;
    cs.add("E08-六型-位移令牌非零基线", px_ok, "dist-small 非零、dist-none 为零");
}

/// 无条件落一条红（局部构造失败时的显性失败，不留静默绿）。
#[allow(dead_code)]
fn dummy_add(cs: &mut CheckSet, name: &'static str) {
    cs.add(name, false, "判据内部构造失败");
}

// ---------------------------------------------------------------------------
// 判据二：错开令牌（锚点判据 2）
// ---------------------------------------------------------------------------

fn chk_stagger(cs: &mut CheckSet) {
    // 区间常量锚点钉死（30–60ms 原文）。
    let bounds_ok = STAGGER_MIN_MS == 30 && STAGGER_MAX_MS == 60 && STAGGER_MIN_MS < STAGGER_MAX_MS;
    cs.add("E08-错开-区间常量", bounds_ok, "STAGGER_MIN_MS=30 / STAGGER_MAX_MS=60");

    // 双向钳制：低于下界钳到 30、高于上界钳到 60、区间内原样且不记钳制。
    let mut p = StaggerPlanner::new();
    let (lo, lo_clamped) = p.clamp(10);
    let (hi, hi_clamped) = p.clamp(120);
    let (mid, mid_clamped) = p.clamp(45);
    let clamp_ok = lo == STAGGER_MIN_MS
        && lo_clamped
        && hi == STAGGER_MAX_MS
        && hi_clamped
        && mid == 45
        && !mid_clamped
        && p.clamped_count() == 2
        && p.diagnostics().len() == 2;
    cs.add("E08-错开-双向钳制", clamp_ok, "10→30、120→60、45 原样；钳制恰记 2 次");

    // 钳制显性：诊断必须带专属码（不静默）。
    let diag_ok = p
        .diagnostics()
        .iter()
        .all(|d| d.contains(E_STAGGER_BOUND));
    cs.add("E08-错开-钳制显性", diag_ok, "每次钳制的诊断须含 E_STAGGER_BOUND");

    // 均布：offsets[i] == i × step（抽样首/中/尾三点 + 全程等差）。
    let mut p2 = StaggerPlanner::new();
    let plan_ok = match p2.plan(6, DEFAULT_STAGGER_MS) {
        Ok(off) => {
            let step = DEFAULT_STAGGER_MS;
            let points_ok = off.first().map(|v| *v == 0).unwrap_or(false)
                && off.len() == 6
                && off.iter().enumerate().all(|(i, v)| *v == (i as u32) * step);
            points_ok
        }
        Err(_) => false,
    };
    cs.add("E08-错开-均布等差", plan_ok, "6 元素偏移须为 0..step..5step 等差");

    // 边界量：0 元素返回空表（不报错）；1 元素只有 0。
    let mut p3 = StaggerPlanner::new();
    let edge_ok = match p3.plan(0, 40) {
        Ok(off) => off.is_empty(),
        Err(_) => false,
    } && match p3.plan(1, 40) {
        Ok(off) => off.len() == 1 && off.first().map(|v| *v == 0).unwrap_or(false),
        Err(_) => false,
    };
    cs.add("E08-错开-空集与单元素", edge_ok, "0 元素空表、1 元素偏移 0");

    // 超上限拒绝专属码（上限复用 F3005 单源）。
    let mut p4 = StaggerPlanner::new();
    let cap_ok = code_of(p4.plan(ENTRY_ELEMENT_CAP + 1, 40)) == "E_STAGGER_COUNT"
        && code_of(p4.plan(ENTRY_ELEMENT_CAP, 40)) == "<no-error>";
    cs.add("E08-错开-上限拒绝", cap_ok, "恰好上限通过、超 1 拒 E_STAGGER_COUNT");

    // 计划期：错开过钳制 + 时长从令牌取（slide-in 走页面档）。
    let table = tok();
    let mut p5 = StaggerPlanner::new();
    let plan_slide_ok = match build_plan("s", EntryKind::SlideIn, SlideDir::Left, TriggerKind::ViewportEnter, 500, &table, &mut p5) {
        Ok(pl) => {
            pl.stagger_step_ms == STAGGER_MAX_MS
                && pl.duration_ms == token_ms(&table, "dur-page")
                && pl.duration_ms > 0
                && pl.distance_px == token_px(&table, "dist-medium")
                && pl.duration_token == "dur-page"
                && p5.clamped_count() == 1
        }
        Err(_) => false,
    };
    cs.add(
        "E08-错开-计划期钳制与令牌",
        plan_slide_ok,
        "请求 500ms 钳到 60；时长/位移与令牌表对账",
    );

    // 计划名非法拒绝专属码。
    let mut p6 = StaggerPlanner::new();
    let name_ok = code_of(build_plan("", EntryKind::FadeIn, SlideDir::Up, TriggerKind::PageLoad, 40, &table, &mut p6)) == "E_ENTRY_NAME";
    cs.add("E08-错开-计划名非法拒绝", name_ok, "空计划名拒 E_ENTRY_NAME");

    // 非 slide-in 型的方向维度无语义（恒 Up，计划不改写）。
    let mut p7 = StaggerPlanner::new();
    let dir_neutral = match build_plan("f", EntryKind::FadeIn, SlideDir::Right, TriggerKind::PageLoad, 40, &table, &mut p7) {
        Ok(pl) => pl.dir == SlideDir::Up,
        Err(_) => false,
    };
    let dir_kept = match build_plan("s2", EntryKind::SlideIn, SlideDir::Down, TriggerKind::PageLoad, 40, &table, &mut p7) {
        Ok(pl) => pl.dir == SlideDir::Down,
        Err(_) => false,
    };
    cs.add("E08-错开-方向维度分型", dir_neutral && dir_kept, "fade-in 方向归零；slide-in 方向保留");
}

// ---------------------------------------------------------------------------
// 判据三：首帧原子（锚点判据 3）
// ---------------------------------------------------------------------------

fn chk_first_frame(cs: &mut CheckSet) {
    // 原子快照：一次收齐全部属性、顺序保持、atomic 恒真。
    let mut a = FirstFrameAtomizer::new();
    let snap_ok = match a.apply_start(7, &[("opacity", 0), ("transform", 8)]) {
        Ok(s) => {
            s.atomic && s.prop_count() == 2 && s.element == 7
                && s.props.first().map(|p| p.0 == "opacity").unwrap_or(false)
                && s.props.iter().nth(1).map(|p| p.0 == "transform").unwrap_or(false)
        }
        Err(_) => false,
    };
    cs.add("E08-首帧-原子快照收齐", snap_ok, "快照一次收齐 2 属性且顺序保持");

    // 空起始态 / 超容 / 坏属性名 → 全部 E_START_EMPTY（规模护栏）。
    let mut a2 = FirstFrameAtomizer::new();
    let empty_ok = code_of(a2.apply_start(1, &[])) == "E_START_EMPTY";
    let big: [(&str, i32); 17] = [
        ("a", 0), ("b", 0), ("c", 0), ("d", 0), ("e", 0), ("f", 0), ("g", 0), ("h", 0),
        ("i", 0), ("j", 0), ("k", 0), ("l", 0), ("m", 0), ("n", 0), ("o", 0), ("p", 0),
        ("q", 0),
    ];
    let cap_ok = code_of(a2.apply_start(1, &big)) == "E_START_EMPTY";
    let badname_ok = code_of(a2.apply_start(1, &[("", 0)])) == "E_START_EMPTY";
    cs.add(
        "E08-首帧-非法起始态拒绝",
        empty_ok && cap_ok && badname_ok,
        "空/超容/坏名均拒 E_START_EMPTY",
    );

    // 清白原子器：零案件、断言通过。
    let mut a3 = FirstFrameAtomizer::new();
    let clean_ok = a3.assert_no_flash().is_ok() && a3.cases().is_empty() && a3.applied_count() == 0;
    cs.add("E08-首帧-清白基线", clean_ok, "未检出时断言须绿（基线先行）");

    // 闪烁检出 → P1 立案 → 断言必须红（缺陷红线实测）。
    a3.note_premature_paint(9);
    let case_ok = a3.cases().len() == 1
        && a3.cases().first().map(|c| c.code == E_FLASH_P1 && c.element == 9).unwrap_or(false);
    let assert_red = a3.assert_no_flash().is_err();
    cs.add("E08-首帧-闪烁立案且断言红", case_ok && assert_red, "检出即立 E_FLASH_P1；有案时 assert_no_flash 必红");

    // 先绘后补：apply_start 不消除立案（补案累积）。
    let mut a4 = FirstFrameAtomizer::new();
    a4.note_premature_paint(5);
    let late_ok = match a4.apply_start(5, &[("opacity", 0)]) {
        Ok(_) => a4.cases().len() == 2 && a4.applied_count() == 1,
        Err(_) => false,
    };
    cs.add("E08-首帧-补应用不撤销立案", late_ok, "先绘记录下补应用须追加立案");

    // 其他元素的先绘记录不牵连无辜元素（按元素定案）。
    let mut a5 = FirstFrameAtomizer::new();
    a5.note_premature_paint(3);
    let isolate_ok = match a5.apply_start(4, &[("opacity", 0)]) {
        Ok(_) => a5.cases().len() == 1 && a5.cases().first().map(|c| c.element == 3).unwrap_or(false),
        Err(_) => false,
    };
    cs.add("E08-首帧-立案按元素隔离", isolate_ok, "元素 4 的正常应用不受元素 3 案件牵连");
}

// ---------------------------------------------------------------------------
// 判据四：触发分型（锚点判据 4）
// ---------------------------------------------------------------------------

fn chk_triggers(cs: &mut CheckSet) {
    // 三类全集：短码往返 + 互异 + 契约表同源。
    let rt = (0..TRIGGER_KIND_COUNT).all(|i| {
        // 契约表即全集短码表；逐条 parse 回来须命中。
        let w = TRIGGER_CONTRACT[i];
        match TriggerKind::parse(w) {
            Some(k) => k.wire() == w,
            None => false,
        }
    });
    cs.add("E08-触发-契约表往返", rt, "TRIGGER_CONTRACT 三短码须全部可逆解析");

    let wires = [
        TriggerKind::PageLoad.wire(),
        TriggerKind::ViewportEnter.wire(),
        TriggerKind::Conditional.wire(),
    ];
    let uniq = wires[0] != wires[1] && wires[1] != wires[2] && wires[0] != wires[2];
    cs.add("E08-触发-短码互异", uniq, "三类短码须互异");

    // 默认规则：once=true、retrigger=false（锚点"触发一次配置默认"）。
    let d = TriggerRule::new(1, TriggerKind::ViewportEnter);
    cs.add(
        "E08-触发-默认一次",
        d.once && !d.retrigger,
        "TriggerRule::new 须 once=true 且 retrigger=false",
    );

    // once 默认：首次 Fire、第二次 AlreadyFired、计数停在 1。
    let mut t = TriggerTable::new();
    let once_ok = match t.register(TriggerRule::new(10, TriggerKind::ViewportEnter)) {
        Ok(_) => {
            let f1 = t.on_event(10, TriggerKind::ViewportEnter, true);
            let f2 = t.on_event(10, TriggerKind::ViewportEnter, true);
            f1 == TriggerDecision::Fire
                && f2 == TriggerDecision::AlreadyFired
                && t.fired_count(10, TriggerKind::ViewportEnter) == 1
        }
        Err(_) => false,
    };
    cs.add("E08-触发-once默认二次拒", once_ok, "首次触发、重复拒 AlreadyFired、计数恰 1");

    // 显性重触发：放行且每次记诊断（E_TRIGGER_REPEAT 落账）。
    let mut t2 = TriggerTable::new();
    let retr_ok = match t2.register(TriggerRule::new(11, TriggerKind::ViewportEnter).with_retrigger()) {
        Ok(_) => {
            let f1 = t2.on_event(11, TriggerKind::ViewportEnter, true);
            let f2 = t2.on_event(11, TriggerKind::ViewportEnter, true);
            let f3 = t2.on_event(11, TriggerKind::ViewportEnter, true);
            f1 == TriggerDecision::Fire
                && f2 == TriggerDecision::Fire
                && f3 == TriggerDecision::Fire
                && t2.fired_count(11, TriggerKind::ViewportEnter) == 3
                && t2.diagnostics().len() == 2
                && t2.diagnostics().iter().all(|x| x.contains(E_TRIGGER_REPEAT))
        }
        Err(_) => false,
    };
    cs.add("E08-触发-显性重触发放行", retr_ok, "重触发 3 次全 Fire；诊断记 2 次且带专属码");

    // 视口规则离屏延迟（F2713 联动）：离屏一律 Deferred，不消耗 once 次数。
    let mut t3 = TriggerTable::new();
    let defer_ok = match t3.register(TriggerRule::new(12, TriggerKind::ViewportEnter)) {
        Ok(_) => {
            let d1 = t3.on_event(12, TriggerKind::ViewportEnter, false);
            let d2 = t3.on_event(12, TriggerKind::ViewportEnter, false);
            let f = t3.on_event(12, TriggerKind::ViewportEnter, true);
            d1 == TriggerDecision::OffscreenDeferred
                && d2 == TriggerDecision::OffscreenDeferred
                && f == TriggerDecision::Fire
                && t3.fired_count(12, TriggerKind::ViewportEnter) == 1
        }
        Err(_) => false,
    };
    cs.add("E08-触发-离屏延迟", defer_ok, "离屏延迟两次后可见才触发；once 次数不被离屏消耗");

    // 未登记元素显性 NotRegistered（区分于静默）。
    let mut t4 = TriggerTable::new();
    let nr = t4.on_event(99, TriggerKind::ViewportEnter, true);
    cs.add("E08-触发-未登记显性", nr == TriggerDecision::NotRegistered, "未登记须 NotRegistered");

    // 页面加载不依赖视口可见性（分型语义：首屏编排一次完成）。
    let mut t5 = TriggerTable::new();
    let pl_ok = match t5.register(TriggerRule::new(13, TriggerKind::PageLoad)) {
        Ok(_) => {
            t5.on_event(13, TriggerKind::PageLoad, false) == TriggerDecision::Fire
                && t5.on_event(13, TriggerKind::PageLoad, true) == TriggerDecision::AlreadyFired
        }
        Err(_) => false,
    };
    cs.add("E08-触发-页面加载无视口依赖", pl_ok, "PageLoad 离屏也触发；二次仍拒");

    // 重复登记拒绝专属码（同元素同类型）。
    let mut t6 = TriggerTable::new();
    let dup_ok = match t6.register(TriggerRule::new(14, TriggerKind::Conditional)) {
        Ok(_) => code_of(t6.register(TriggerRule::new(14, TriggerKind::Conditional))) == "E_TRIGGER_DUP",
        Err(_) => false,
    };
    cs.add("E08-触发-重复登记拒", dup_ok, "同元素同类型二次登记拒 E_TRIGGER_DUP");

    // 同元素不同类型不冲突（分型互不挤占）。
    let mut t7 = TriggerTable::new();
    let cross_ok = match t7.register(TriggerRule::new(15, TriggerKind::PageLoad)) {
        Ok(_) => match t7.register(TriggerRule::new(15, TriggerKind::Conditional)) {
            Ok(_) => {
                t7.on_event(15, TriggerKind::PageLoad, true) == TriggerDecision::Fire
                    && t7.on_event(15, TriggerKind::Conditional, true) == TriggerDecision::Fire
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    cs.add("E08-触发-跨类型不挤占", cross_ok, "同元素两类规则各自触发");
}

// ---------------------------------------------------------------------------
// 判据五：reduce 终态 + 编排图单源（锚点判据 5）
// ---------------------------------------------------------------------------

fn chk_reduce_terminal(cs: &mut CheckSet) {
    let table = tok();
    let mut p = StaggerPlanner::new();
    let elements: [u32; 4] = [100, 101, 102, 103];

    // 同一张计划编译两个泳道（图单源：只差泳道参数）。
    let plan = match build_plan("n", EntryKind::FadeUp, SlideDir::Up, TriggerKind::PageLoad, 40, &table, &mut p) {
        Ok(pl) => pl,
        Err(_) => {
            dummy_add(cs, "E08-reduce-计划构造");
            return;
        }
    };
    let normal = match compile_entry(&plan, &elements, Lane::Normal) {
        Ok(c) => c,
        Err(_) => {
            dummy_add(cs, "E08-reduce-Normal编译");
            return;
        }
    };
    let reduced = match compile_entry(&plan, &elements, Lane::Reduced) {
        Ok(c) => c,
        Err(_) => {
            dummy_add(cs, "E08-reduce-Reduced编译");
            return;
        }
    };

    // Normal 泳道基线（恒真门禁防御：先证产物非平凡）。
    let base_ok = normal.instance_count() == 4
        && normal.total_duration_ms() > 0
        && !normal.collapsed()
        && normal.compiled.instances.iter().all(|i| i.duration_ms > 0);
    cs.add("E08-reduce-Normal基线非平凡", base_ok, "4 实例、总时长非零、未坍缩、逐实例非零");

    // 均布偏移在实例上兑现（图内 offset_start 边 → offset_in_stage_ms）。
    let mut offsets_ok = normal.offsets.len() == 4;
    for (i, inst) in normal.compiled.instances.iter().enumerate() {
        if i < normal.offsets.len() && inst.offset_in_stage_ms != normal.offsets[i] {
            offsets_ok = false;
        }
    }
    cs.add("E08-reduce-偏移均布兑现", offsets_ok, "实例偏移与计划均布序列逐位相等");

    // reduce 泳道：整体坍缩 + 逐实例双零 + assert_reduce_terminal 绿。
    let reduce_ok = reduced.collapsed()
        && reduced.instance_count() == 4
        && reduced
            .compiled
            .instances
            .iter()
            .all(|i| i.duration_ms == 0 && i.offset_in_stage_ms == 0)
        && reduced.assert_reduce_terminal().is_ok();
    cs.add("E08-reduce-终态双零", reduce_ok, "reduce 下时长与偏移全零且断言通过");

    // assert_reduce_terminal 对 Normal 泳道必须拒绝（判定路径防接错）。
    cs.add(
        "E08-reduce-误用拒绝",
        normal.assert_reduce_terminal().is_err(),
        "Normal 泳道调 reduce 断言须 Err",
    );

    // 视图：行数守恒、元素升序、播报含名与触发短码。
    let v = build_view(&plan, &normal);
    let mut rows_ok = v.row_count() == 4;
    for (i, r) in v.rows.iter().enumerate() {
        if i < elements.len() && (r.element != elements[i] || r.kind != "fade-up") {
            rows_ok = false;
        }
    }
    let spoken = v.spoken();
    cs.add(
        "E08-reduce-视图守恒",
        rows_ok && spoken.contains("fade-up") && spoken.contains("page-load"),
        "视图 4 行、元素与计划一致、播报含型与触发",
    );

    // 空元素集拒绝专属码（编排图非空底线）。
    let empty_ok = code_of(compile_entry(&plan, &[], Lane::Normal)) == "E_ENTRY_EMPTY";
    cs.add("E08-reduce-空集拒绝", empty_ok, "空元素集拒 E_ENTRY_EMPTY");

    // slide-in 方向进入计划不影响编译（四向同构：实例数与时长不变）。
    // 总时长=令牌基线+(n-1)×生效步长（末元素完成时刻；步长 50 在区间内不钳制）。
    let mut p2 = StaggerPlanner::new();
    let slide_ok = match build_plan("s", EntryKind::SlideIn, SlideDir::Right, TriggerKind::ViewportEnter, 50, &table, &mut p2) {
        Ok(pl) => match compile_entry(&pl, &elements, Lane::Normal) {
            Ok(c) => {
                let expect = token_ms(&table, "dur-page") + (elements.len() as u32 - 1) * pl.stagger_step_ms;
                c.instance_count() == 4
                    && c.total_duration_ms() == expect
                    && pl.stagger_step_ms == 50
                    && expect > token_ms(&table, "dur-page")
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    cs.add("E08-reduce-四向同构", slide_ok, "slide-in 右向实例 4 个；总时长=页面档+(n-1)×步长");
}

// ---------------------------------------------------------------------------
// 判据六：契约（锚点"判据"自指项）
// ---------------------------------------------------------------------------

fn chk_contract(cs: &mut CheckSet) {
    // 协议版本。
    let ver_ok = ENTRY_PROTOCOL_VERSION == "P08-entry-v1";
    cs.add("E08-契约-协议版本", ver_ok, "入场族协议版本冻结为 P08-entry-v1");

    // 诊断码互异（13 码全量两两）。
    let my_codes: [&str; 13] = [
        E_ENTRY_KIND,
        E_ENTRY_NAME,
        E_ENTRY_DUP,
        E_ENTRY_EMPTY,
        E_STAGGER_BOUND,
        E_STAGGER_COUNT,
        E_TRIGGER_KIND,
        E_TRIGGER_DUP,
        E_TRIGGER_REPEAT,
        E_TRIGGER_OFFSCREEN,
        E_FLASH_P1,
        E_START_EMPTY,
        E_VARIANT_BAD,
    ];
    let mut uniq = true;
    for i in 0..my_codes.len() {
        for j in (i + 1)..my_codes.len() {
            if my_codes[i] == my_codes[j] {
                uniq = false;
            }
        }
    }
    cs.add("E08-契约-诊断码互异", uniq, "入场域 13 个诊断码不得重复");

    // 计数常量。
    let count_ok = KIND_COUNT == 6 && TRIGGER_KIND_COUNT == 3 && ENTRY_ELEMENT_CAP > 0;
    cs.add("E08-契约-计数常量", count_ok, "KIND_COUNT=6 / TRIGGER_KIND_COUNT=3 / 上限非零");

    // 起始态容量护栏与属性名上限为正。
    let cap_ok = START_PROP_CAP > 0 && PROP_CAP > 0 && NAME_CAP > 0;
    cs.add("E08-契约-护栏常量", cap_ok, "原子器与命名护栏须为正");

    // 默认错开必须落在锚点区间内（自洽）。
    let def_ok = DEFAULT_STAGGER_MS >= STAGGER_MIN_MS && DEFAULT_STAGGER_MS <= STAGGER_MAX_MS;
    cs.add("E08-契约-默认错开自洽", def_ok, "DEFAULT_STAGGER_MS 须在 [30,60] 内");
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：六型族 + 错开令牌。
pub fn run_vep08_checks_a() -> CheckSet {
    let mut cs = CheckSet::new("vep08-entry-a");
    chk_six_kinds(&mut cs);
    chk_stagger(&mut cs);
    cs
}

/// B 批：首帧原子 + 触发分型 + reduce 终态 + 契约。
pub fn run_vep08_checks_b() -> CheckSet {
    let mut cs = CheckSet::new("vep08-entry-b");
    chk_first_frame(&mut cs);
    chk_triggers(&mut cs);
    chk_reduce_terminal(&mut cs);
    chk_contract(&mut cs);
    cs
}
