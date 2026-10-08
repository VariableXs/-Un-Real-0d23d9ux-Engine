//! VE-F3410 · 深空玻璃令牌基线 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3410`
//!
//! # 判据映射（锚点五条判据 + 纪律）
//!
//! - **审美令牌化**（8 条）→ 五级封闭全集/默认质感单源对拍（判据侧
//!   独立字面量表）/命名单源/O(1) 查询/基线零事件不变量/重级非零对照；
//! - **质感价签**（5 条）→ 价签公式判据侧独立重算逐级对拍/透明级
//!   基础价/blur 步价/预算常量钉死；
//! - **双标注**（6 条）→ 每令牌两半齐/缺签补齐记账/错签拒绝/先整改
//!   后定价/整改账/钳后价签跟整改后质感；
//! - **降档预告**（7 条）→ 默认基线恰只 Heavy 超标/预告三字段独立
//!   重算/应用核销/无预告拒/最低档无预告/重复扫描不重发/降档序变体；
//! - **判据与契约**（9 条）→ 七码唯一/三要素/阻断降级分清/码表冻结
//!   一致/每码真跑可达/契约冻结/联动校验/读屏/变体。
//!
//! # 判据设计纪律
//!
//! 1. 期望值由判据侧**独立字面量**给出（150/1590/1110/630/250 与
//!    五级质感三元组全部写死），不调被测 `derive_price`/`tier_texture`
//!    当期望——同源对拍是恒真门禁；
//! 2. 每条「必被抓」判据配变体（降档序反转/码表漂移/提示当阻断），
//!    变体只在目标维度不同；
//! 3. 七个诊断码逐个**真跑造出来**（有短码 ≠ 可达）；
//! 4. 判据区零 panic 面：越界一律 match/`.get()` 记红。

use crate::checks::CheckSet;
use crate::svstar2::ver01j_glass::*;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧常量（独立字面量，不从被测推导）
// ---------------------------------------------------------------------------

const EXPECT_CODES: usize = 7;
const EXPECT_TIERS: usize = 5;

/// 判据侧独立质感表：(blur_px, luma‰, opacity‰)——与锚点审美立场
/// 字面一致，被测 `tier_texture` 若被改即在此对拍转红。
const EXPECT_TEXTURE: [(u32, u32, u32); EXPECT_TIERS] = [
    (0, 200, 1000),  // Opaque
    (48, 800, 700),  // Heavy
    (32, 650, 550),  // Medium
    (16, 500, 400),  // Light
    (0, 350, 150),   // Clear
];

/// 判据侧独立价签表（‰）：基础价(150，Clear 250) + blur×30 逐级算死。
const EXPECT_STEPS: [u32; EXPECT_TIERS] = [150, 1590, 1110, 630, 250];

/// 判据侧独立路径名表。
const EXPECT_PATHS: [&str; EXPECT_TIERS] = [
    "glass.opaque",
    "glass.heavy",
    "glass.medium",
    "glass.light",
    "glass.clear",
];

fn tier_at(i: usize) -> Option<GlassTier> {
    match i {
        0 => Some(GlassTier::Opaque),
        1 => Some(GlassTier::Heavy),
        2 => Some(GlassTier::Medium),
        3 => Some(GlassTier::Light),
        4 => Some(GlassTier::Clear),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 一、审美令牌化（基线完整性 + 单源对拍）
// ---------------------------------------------------------------------------

fn chk_baseline(set: &mut CheckSet) {
    let b = GlassBaseline::new();

    // 五级封闭全集：数量、序、路径、双标注齐。
    let mut all_ok = b.tokens.len() == EXPECT_TIERS;
    for (i, tok) in b.tokens.iter().enumerate() {
        let t = match tier_at(i) {
            Some(t) => t,
            None => {
                all_ok = false;
                continue;
            }
        };
        if tok.tier != t
            || tok.path != EXPECT_PATHS[i]
            || tok.tag.steps_permille != EXPECT_STEPS[i]
        {
            all_ok = false;
        }
        let (eb, el, eo) = EXPECT_TEXTURE[i];
        if tok.spec.blur_px != eb
            || tok.spec.luma_permille != el
            || tok.spec.opacity_permille != eo
        {
            all_ok = false;
        }
    }
    set.add(
        "F3410-基线-五级封闭全集对拍",
        all_ok,
        "层级/路径/质感三元组/价签逐级与判据侧独立字面量表一致（审美令牌化：参数单源、无裸魔数漂移）",
    );

    // 重级非零对照（防恒零语料：若 Heavy blur=0，价签与降档判据全空转）。
    let heavy = match b.token_of(GlassTier::Heavy) {
        Some(t) => t,
        None => {
            set.fail("F3410-基线-重级非零对照", "查不到 Heavy 令牌");
            return;
        }
    };
    set.add(
        "F3410-基线-重级非零对照",
        heavy.spec.blur_px > 0 && heavy.tag.steps_permille > 1000,
        "重玻璃须真有模糊与高价签——基线非平凡，预算判据才有信息量",
    );

    // O(1) 查询：五级全可查。
    let q = GlassTier::ALL
        .iter()
        .all(|t| b.token_of(*t).is_some());
    set.add(
        "F3410-基线-层级直查",
        q,
        "五级逐一直查可得（封闭全集查询不依赖扫描序）",
    );

    // 基线零事件不变量：默认构造无补签无整改无预告。
    set.add(
        "F3410-基线-默认零事件",
        b.tag_backfilled.is_empty() && b.remediated.is_empty() && b.previews.is_empty(),
        "默认基线必须零整改零补签零预告——基线自身缺纪律，纪律就是空话",
    );

    // 基线读屏：含五级名与价签字样。
    let sp = baseline_spoken(&b);
    set.add(
        "F3410-基线-读屏可达",
        sp.contains("深空玻璃基线")
            && sp.contains("重玻璃")
            && sp.contains("透明")
            && sp.contains("1590"),
        "读屏须逐级给出质感与价签（含重玻璃价签字面，验证明细真在句里）",
    );

    // 层级序往返 + 降档方向语义：rank+1 恰是轻一级。
    let seq_ok = (0..EXPECT_TIERS).all(|i| {
        match GlassTier::of_rank(i) {
            Some(t) => t.rank() == i,
            None => false,
        }
    }) && GlassTier::of_rank(EXPECT_TIERS).is_none();
    set.add(
        "F3410-基线-层级序往返",
        seq_ok,
        "rank/of_rank 逐位往返且越界 None（降档方向=序号+1 的前提）",
    );
}

// ---------------------------------------------------------------------------
// 二、质感价签 + 双标注
// ---------------------------------------------------------------------------

fn chk_pricing(set: &mut CheckSet) {
    // 价签公式独立重算对拍：判据侧写死算式，不调 derive_price。
    let heavy_t = Texture {
        blur_px: 48,
        luma_permille: 0,
        opacity_permille: 0,
    };
    set.add(
        "F3410-价签-公式独立重算",
        derive_price(GlassTier::Heavy, heavy_t) == 150 + 48 * 30
            && derive_price(GlassTier::Clear, heavy_t) == 250 + 48 * 30,
        "价签=基础价+blur×步价逐位成立；透明级基础价更高（淡染合成成本不在 blur）",
    );

    // 缺签补齐：合法质感 + None → 补齐且值与判据侧重算一致。
    let ok_t = Texture {
        blur_px: 16,
        luma_permille: 500,
        opacity_permille: 400,
    };
    let backfill = build_token("glass.custom.a", GlassTier::Light, ok_t, None);
    set.add(
        "F3410-价签-缺签补齐",
        match backfill {
            Ok((tok, note)) => {
                note == Some(GlassCode::TagMissing)
                    && tok.tag.steps_permille == 150 + 16 * 30
            }
            Err(_) => false,
        },
        "缺价签按公式补齐并报 TagMissing（降级矩阵：价签缺失→标注补齐，非阻断）",
    );

    // 错签拒绝：Some(错价) → Err(TagInvalid)，不可钳（假账不可补救）。
    let badtag = build_token(
        "glass.custom.b",
        GlassTier::Light,
        ok_t,
        Some(PriceTag {
            steps_permille: 999,
        }),
    );
    set.add(
        "F3410-价签-错签拒绝",
        badtag.err() == Some(GlassCode::TagInvalid)
            && GlassCode::TagInvalid.blocking(),
        "价签与推导不符须拒——补齐解决没写，解决不了写错",
    );

    // 零事件构造：合法质感 + 恰推导价签 → Ok(None)。
    let exact = build_token(
        "glass.custom.c",
        GlassTier::Light,
        ok_t,
        Some(PriceTag {
            steps_permille: 150 + 16 * 30,
        }),
    );
    set.add(
        "F3410-价签-恰推导零事件",
        match exact {
            Ok((_, note)) => note.is_none(),
            Err(_) => false,
        },
        "价签恰与推导一致时零事件（None）——否则对拍口径漂移",
    );

    // 质感违例整改：越界钳回 + 报 TextureViolation + 价签跟整改后质感。
    let wild = Texture {
        blur_px: 200,
        luma_permille: 1500,
        opacity_permille: 700,
    };
    let fix = build_token("glass.custom.d", GlassTier::Heavy, wild, None);
    set.add(
        "F3410-价签-违例钳回整改",
        match fix {
            Ok((tok, note)) => {
                note == Some(GlassCode::TextureViolation)
                    && tok.spec.blur_px == BLUR_MAX_PX
                    && tok.spec.luma_permille == PERMILLE_MAX
                    && tok.spec.opacity_permille == 700
                    && tok.tag.steps_permille == 150 + BLUR_MAX_PX * 30
            }
            Err(_) => false,
        },
        "越界三向钳回（blur 到上限/光影到上限/透明度不动）且价签按整改后质感重推导（先整改后定价）",
    );

    // 违例与错签同发：整改优先（违例在先，错签按整改后口径判定）。
    let both = build_token(
        "glass.custom.e",
        GlassTier::Medium,
        Texture {
            blur_px: 999,
            luma_permille: 0,
            opacity_permille: 0,
        },
        Some(PriceTag {
            steps_permille: 150 + BLUR_MAX_PX * 30,
        }),
    );
    set.add(
        "F3410-价签-整改优先于错签",
        match both {
            Ok((tok, note)) => {
                note == Some(GlassCode::TextureViolation)
                    && tok.tag.steps_permille == 150 + BLUR_MAX_PX * 30
            }
            Err(_) => false,
        },
        "违例钳回后价签恰为整改口径——同发时按处置顺序报码，不静默吞错签",
    );

    // 路径非法拒（真跑造码 TokenEmpty）。
    let empty = build_token("", GlassTier::Light, ok_t, None);
    let long = build_token(&"x".repeat(PATH_MAX + 1), GlassTier::Light, ok_t, None);
    set.add(
        "F3410-价签-路径非法拒",
        empty.err() == Some(GlassCode::TokenEmpty)
            && long.err() == Some(GlassCode::TokenEmpty)
            && GlassCode::TokenEmpty.blocking(),
        "空/超长路径须拒且阻断",
    );
}

// ---------------------------------------------------------------------------
// 三、降档预告（预算扫描 + 对票应用）
// ---------------------------------------------------------------------------

fn chk_downshift(set: &mut CheckSet) {
    let mut b = GlassBaseline::new();

    // 默认基线预算扫描：判据侧按独立价签表推——恰只 Heavy(1590) 超预算。
    let over: Vec<usize> = EXPECT_STEPS
        .iter()
        .enumerate()
        .filter(|(_, s)| **s > BUDGET_STEPS)
        .map(|(i, _)| i)
        .collect();
    let previews = b.budget_scan(BUDGET_STEPS);
    set.add(
        "F3410-降档-默认恰只重级超标",
        over == [1] && previews.len() == 1,
        "判据侧独立推出恰只 Heavy 超预算——被测扫描须同判（否则价签表或扫描有漂移）",
    );

    // 预告四字段独立重算：身份=路径、Heavy→Medium、new_steps=Medium 默认价。
    let p = match previews.first() {
        Some(p) => p.clone(),
        None => {
            set.fail("F3410-降档-预告字段", "无预告可核");
            return;
        }
    };
    set.add(
        "F3410-降档-预告字段独立重算",
        p.path == "glass.heavy"
            && p.from == GlassTier::Heavy
            && p.to == GlassTier::Medium
            && p.new_steps == EXPECT_STEPS[2],
        "预告须给出令牌身份/从/到/降价后步价且与判据侧重算一致（降档预告：先出预告再落档）",
    );

    // 重复扫描不重发。
    let again = b.budget_scan(BUDGET_STEPS);
    set.add(
        "F3410-降档-重复扫描不重发",
        again.is_empty() && b.previews.len() == 1,
        "同一超标事实只发一张预告（预告是待办不是日志）",
    );

    // 变体：降档序反转（Heavy→Clear 跳两级）必与实测不同。
    let mutant = DownshiftPreview {
        path: String::from("glass.heavy"),
        from: GlassTier::Heavy,
        to: GlassTier::Clear,
        new_steps: EXPECT_STEPS[4],
    };
    set.add(
        "F3410-降档-降档序变体必被抓",
        p != mutant,
        "跳级降档须与逐级降档不同——相同则降档序判据恒真",
    );

    // 对票应用：持正确预告落档，核销预告，三半同步。
    let applied = b.apply_downshift(&p);
    set.add(
        "F3410-降档-对票应用核销",
        applied.is_ok()
            && b.previews.is_empty()
            && match b.token_of(GlassTier::Medium) {
                Some(t) => {
                    t.spec.blur_px == 32 && t.tag.steps_permille == EXPECT_STEPS[2]
                }
                None => false,
            },
        "应用后预告核销、Medium 级质感与价签同步到位（三半永不失配）",
    );

    // 应用后原级位势核查：Heavy 槽已空（换级不是复制）。
    set.add(
        "F3410-降档-换级不复制",
        b.token_of(GlassTier::Heavy).is_none(),
        "降档后 Heavy 槽须无令牌——只改不删会让同一令牌出现在两级",
    );

    // 无预告应用拒（真跑造码 PreviewMismatch）。
    let ghost = DownshiftPreview {
        path: String::from("glass.light"),
        from: GlassTier::Light,
        to: GlassTier::Clear,
        new_steps: EXPECT_STEPS[4],
    };
    let ghost_apply = b.apply_downshift(&ghost);
    set.add(
        "F3410-降档-无预告即拒",
        ghost_apply.err() == Some(GlassCode::PreviewMismatch)
            && GlassCode::PreviewMismatch.blocking(),
        "没有预告的降档不可应用（静默降档拦截，阻断级）",
    );

    // 变体预算：预算 500 时判据侧推出 Light(630)/Heavy(1590)/Medium(1110) 超，
    // Light 降 Clear、Heavy→Medium、Medium→Light；Opaque(150)/Clear(250) 不超。
    let mut b2 = GlassBaseline::new();
    let mut expect_over = 0usize;
    for s in EXPECT_STEPS.iter() {
        if *s > 500 {
            expect_over += 1;
        }
    }
    let pv2 = b2.budget_scan(500);
    set.add(
        "F3410-降档-紧预算多级预告",
        expect_over == 3 && pv2.len() == 3,
        "紧预算下判据侧独立推出三级超标，扫描须同判（预算参数化扫描不写死单一语料）",
    );

    // 最低档无预告：Clear 价签 250 > 0 预算时无预告可发（无可降）。
    // 可降四级预告须逐级相邻（to 恰为 from 序号+1），不跳级不越界。
    let mut b3 = GlassBaseline::new();
    let pv3 = b3.budget_scan(0);
    set.add(
        "F3410-降档-最低档如实无预告",
        pv3.len() == 4
            && pv3
                .iter()
                .all(|q| q.to.rank() == q.from.rank() + 1),
        "预算 0 时四级各降一级且逐级相邻、最低档 Clear 无从再降（如实不硬造预告）",
    );
}

// ---------------------------------------------------------------------------
// 四、V 域联动 + 判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 联动：默认关、合法过、越界拒（真跑造码 LinkageState）。
    let off = ColorLinkage::off();
    let ok_link = ColorLinkage {
        enabled: true,
        tint_permille: 1000,
    };
    let bad_link = ColorLinkage {
        enabled: true,
        tint_permille: 1001,
    };
    set.add(
        "F3410-联动-三态校验",
        off.validate().is_ok()
            && ok_link.validate().is_ok()
            && bad_link.err() == Some(GlassCode::LinkageState)
            && GlassCode::LinkageState.blocking(),
        "联动默认关闭、端点合法值（恰 1000）放行、越界即拒（V 域色彩联动对接点）",
    );

    // 七码唯一。
    let mut codes: Vec<&str> = GlassCode::ALL.iter().map(|c| c.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    set.add(
        "F3410-判据-七码唯一",
        codes.len() == EXPECT_CODES && GlassCode::ALL.len() == EXPECT_CODES,
        "错误码重复会让用户报号指不准",
    );

    // 三要素齐发。
    set.add(
        "F3410-判据-错误三要素齐发",
        GlassCode::ALL.iter().all(|c| {
            !c.code().is_empty() && !c.spoken().is_empty() && c.spoken().contains(c.code())
        }),
        "每个码须有短码与可读句子",
    );

    // 阻断/降级分清：阻断恰 4，降级恰 3。
    let blocking = GlassCode::ALL.iter().filter(|c| c.blocking()).count();
    let degradable = GlassCode::ALL.iter().filter(|c| c.degradable()).count();
    set.add(
        "F3410-判据-阻断降级分清",
        blocking == 4 && degradable == 3,
        "阻断 4（路径/错签/无预告/联动）与降级 3（违例/缺签/超标）三档不侵占",
    );

    // 码表与枚举一致（判据侧独立列出冻结表）。
    const EXPECT_WIRE: [&str; EXPECT_CODES] = [
        "E12-TOKEN-EMPTY",
        "E12-TAG-INVALID",
        "E12-PREVIEW-MISMATCH",
        "E12-LINKAGE-STATE",
        "E12-TEXTURE-VIOLATION",
        "E12-TAG-MISSING",
        "E12-BUDGET-OVER",
    ];
    let mut got: Vec<&str> = GlassCode::ALL.iter().map(|c| c.code()).collect();
    got.sort_unstable();
    let mut want: Vec<&str> = EXPECT_WIRE.to_vec();
    want.sort_unstable();
    set.add(
        "F3410-判据-码表与枚举一致",
        got == want,
        "ALL 须恰好列出 7 码且与冻结表逐字一致（E12 段独占）",
    );

    // 契约冻结（判据侧钉死具体值）。
    set.add(
        "F3410-判据-契约已冻结",
        GLASS_CONTRACT == "E12-glass-v1"
            && BLUR_MAX_PX == 64
            && PERMILLE_MAX == 1000
            && BLUR_COST_PER_PX == 30
            && BUDGET_STEPS == 1200
            && BASELINE_TOKENS == 5
            && PATH_MAX == 128,
        "契约号与关键上限钉死（改它们等于改契约）",
    );

    // 每码真实产生点：七个码逐个真跑造出来。
    let mut reached: Vec<GlassCode> = Vec::new();
    // TokenEmpty
    if build_token("", GlassTier::Light, Texture { blur_px: 0, luma_permille: 0, opacity_permille: 0 }, None)
        .err() == Some(GlassCode::TokenEmpty)
    {
        reached.push(GlassCode::TokenEmpty);
    }
    // TagInvalid
    if build_token(
        "c.a",
        GlassTier::Light,
        Texture { blur_px: 0, luma_permille: 0, opacity_permille: 0 },
        Some(PriceTag { steps_permille: 1 }),
    )
    .err() == Some(GlassCode::TagInvalid)
    {
        reached.push(GlassCode::TagInvalid);
    }
    // PreviewMismatch
    {
        let mut b = GlassBaseline::new();
        let ghost = DownshiftPreview {
            path: String::from("glass.light"),
            from: GlassTier::Light,
            to: GlassTier::Clear,
            new_steps: 250,
        };
        if b.apply_downshift(&ghost).err() == Some(GlassCode::PreviewMismatch) {
            reached.push(GlassCode::PreviewMismatch);
        }
    }
    // LinkageState
    {
        let bad = ColorLinkage {
            enabled: true,
            tint_permille: PERMILLE_MAX + 1,
        };
        if bad.validate().err() == Some(GlassCode::LinkageState) {
            reached.push(GlassCode::LinkageState);
        }
    }
    // TextureViolation
    if build_token(
        "c.b",
        GlassTier::Heavy,
        Texture { blur_px: BLUR_MAX_PX + 1, luma_permille: 0, opacity_permille: 0 },
        None,
    )
    .ok()
    .map(|(_, n)| n) == Some(Some(GlassCode::TextureViolation))
    {
        reached.push(GlassCode::TextureViolation);
    }
    // TagMissing
    if build_token(
        "c.c",
        GlassTier::Light,
        Texture { blur_px: 0, luma_permille: 0, opacity_permille: 0 },
        None,
    )
    .ok()
    .map(|(_, n)| n) == Some(Some(GlassCode::TagMissing))
    {
        reached.push(GlassCode::TagMissing);
    }
    // BudgetOver：默认基线在预算 1200 下必有超标预告（码的语义事实）
    {
        let mut b = GlassBaseline::new();
        if !b.budget_scan(BUDGET_STEPS).is_empty() {
            reached.push(GlassCode::BudgetOver);
        }
    }
    set.add(
        "F3410-判据-每码真实产生点",
        reached.len() == EXPECT_CODES,
        "码在 ALL/映射/句子里齐 ≠ 可达；7 码须逐个真跑造出来",
    );

    // 变体：码表漂移必被抓。
    let mutated = {
        let mut w = EXPECT_WIRE.to_vec();
        w[0] = "E12-MUTATED";
        let a: Vec<&str> = GlassCode::ALL.iter().map(|c| c.code()).collect();
        let mut sa = a.clone();
        sa.sort_unstable();
        let mut sw = w.to_vec();
        sw.sort_unstable();
        sa != sw
    };
    set.add(
        "F3410-判据-变体码表漂移必被抓",
        mutated,
        "冻结表被改时『码表一致』判据须转红",
    );

    // 变体：把降级码当阻断计须与实测不同。
    let cnt_if_violation_blocked = GlassCode::ALL
        .iter()
        .filter(|c| c.blocking() || **c == GlassCode::TextureViolation)
        .count();
    set.add(
        "F3410-判据-变体降级当阻断必被抓",
        cnt_if_violation_blocked != 4,
        "把降级码算进阻断会让三档计数漂移——判据须能区分",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：基线完整性 + 价签双标注。
pub fn run_ver01j_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01j-glass-a");
    chk_baseline(&mut set);
    chk_pricing(&mut set);
    set
}

/// B 批：降档预告 + 联动 + 契约。
pub fn run_ver01j_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01j-glass-b");
    chk_downshift(&mut set);
    chk_contract(&mut set);
    set
}
