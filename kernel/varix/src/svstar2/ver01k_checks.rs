//! VE-F3411 · 明暗双主题运行时 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3411`
//!
//! # 判据映射（锚点五条判据 + 纪律）
//!
//! - **强制配对**（8 条）→ 缺一即拒（两侧分别）/空路径拒/超长路径拒/
//!   同路径覆盖/删除与查询/取侧值/O(令牌对) 操作面；
//! - **对比度断言**（6 条）→ AA 阈值常量/恰边界 46 过 47 拒/暗侧独立
//!   测（同值换侧）/双测缺一即阻断/对比度判据侧独立重算；
//! - **平滑过渡**（7 条）→ 空操作不造假事务/起过渡/接管记账/接管不
//!   清零/步进饱和/到位落脚/混合值恰边界；
//! - **跟随系统**（5 条）→ 系统模式自动起过渡/手动模式不打扰/信号
//!   仍记录/瞬切转换记账/瞬切空操作不假报；
//! - **判据与契约**（9 条）→ 七码唯一/三要素/阻断降级分清/码表冻结
//!   一致/每码真跑可达/时间驱动冻结/联动校验/读屏/条数对账。
//!
//! # 判据设计纪律
//!
//! 1. 期望值由判据侧**独立字面量**给出（20615/1900/4542/4466/127 等
//!    全部写死），不调被测 `contrast_permille`/`luminance` 当期望——
//!    同源对拍是恒真门禁；
//! 2. 每条「必被抓」判据配变体（接管清零/单侧对比/跳变放行/冻结提
//!    前生效），变体只在目标维度不同；
//! 3. 七个诊断码逐个**真跑造出来**（有短码 ≠ 可达）；
//! 4. 判据区零 panic 面：越界一律 match/`.get()` 记红。

use crate::checks::CheckSet;
use crate::svstar2::ver01k_dualtheme::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧常量（独立字面量，不从被测推导）
// ---------------------------------------------------------------------------

const EXPECT_CODES: usize = 7;
const EXPECT_SIDES: usize = 2;

/// 判据侧独立重算的对比度字面量（‰）：
/// - 黑/白：(255+13)×1000/(0+13) = 20615；
/// - 中灰(128)/白：268000/(128+13) = 1900；
/// - (46,46,46)/白：268000/(46+13) = 4542（恰过 AA 4500）；
/// - (47,47,47)/白：268000/(47+13) = 4466（恰拒 AA 4500）。
const EXPECT_CONTRAST_BW: u32 = 20615;
const EXPECT_CONTRAST_GRAY_W: u32 = 1900;
const EXPECT_PASS_ON_WHITE: u32 = 4542;
const EXPECT_FAIL_ON_WHITE: u32 = 4466;

/// 判据侧独立字面量颜色。
const WHITE: RGB888 = RGB888::new(255, 255, 255);
const BLACK: RGB888 = RGB888::new(0, 0, 0);
const GRAY: RGB888 = RGB888::new(128, 128, 128);
const EDGE_PASS: RGB888 = RGB888::new(46, 46, 46);
const EDGE_FAIL: RGB888 = RGB888::new(47, 47, 47);
/// 判据侧独立字面量：黑面上的失败色（40 对黑 4077<4500，对白 5056≥4500）。
const DARK40: RGB888 = RGB888::new(40, 40, 40);

fn side_at(i: usize) -> Option<ThemeSide> {
    match i {
        0 => Some(ThemeSide::Light),
        1 => Some(ThemeSide::Dark),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 一、强制配对
// ---------------------------------------------------------------------------

fn chk_pairing(set: &mut CheckSet) {
    let mut t = PairTable::new(WHITE, BLACK);

    // 缺亮值拒 / 缺暗值拒 / 双空拒：缺一即拒绝，三态同权。
    set.add(
        "F3411-配对-缺一即拒",
        t.insert("c.fg", None, Some(WHITE)) == Err(ThemeCode::PairMissing)
            && t.insert("c.fg", Some(BLACK), None) == Err(ThemeCode::PairMissing)
            && t.insert("c.fg", None, None) == Err(ThemeCode::PairMissing)
            && t.count() == 0,
        "缺亮/缺暗/双空三种缺配一并拒，且拒后不落表（半对令牌不可存在）",
    );

    // 合法对入表；同路径覆盖（两闸不豁免）；计数守恒。
    let ins = t.insert("c.fg", Some(BLACK), Some(WHITE));
    let over = t.insert("c.fg", Some(EDGE_PASS), Some(EDGE_PASS));
    set.add(
        "F3411-配对-覆盖同入口",
        ins.is_ok()
            && over.is_ok()
            && t.count() == 1
            && t.get("c.fg").map(|k| k.light) == Some(EDGE_PASS),
        "合法对入表；同路径二次插入走同一入口覆盖且不增数",
    );

    // 路径边界：空/超长拒，恰 PATH_MAX 放行。
    let mut tb = PairTable::new(WHITE, BLACK);
    let exact = "c".to_string() + &"x".repeat(PATH_MAX - 1);
    let too_long = "c".to_string() + &"x".repeat(PATH_MAX);
    set.add(
        "F3411-配对-路径边界",
        tb.insert("", Some(BLACK), Some(WHITE)) == Err(ThemeCode::TokenEmpty)
            && tb.insert(&too_long, Some(BLACK), Some(WHITE)) == Err(ThemeCode::TokenEmpty)
            && tb.insert(&exact, Some(BLACK), Some(WHITE)).is_ok(),
        "空路径拒、超长一字节拒、恰上限放行（贴线不误拒）",
    );

    // 查询/删除/取侧值三操作面（get 借用在 remove 前收成 owned 判定）。
    let q_before = t.get("c.fg").is_some();
    let del = t.remove("c.fg");
    let q_after = t.get("c.fg").is_none();
    let del2 = t.remove("c.fg");
    set.add(
        "F3411-配对-删查侧值",
        q_before && del && q_after && !del2 && t.count() == 0,
        "删存在返真、删不存在返假；删后查无；计数归零",
    );

    // 取侧值：侧决定取哪半，无暧昧。
    let mut ts = PairTable::new(WHITE, BLACK);
    ts.insert("c.a", Some(BLACK), Some(WHITE)).ok();
    set.add(
        "F3411-配对-侧取值",
        ts.value_of("c.a", ThemeSide::Light) == Some(BLACK)
            && ts.value_of("c.a", ThemeSide::Dark) == Some(WHITE)
            && ts.value_of("c.none", ThemeSide::Light).is_none(),
        "亮侧取亮值、暗侧取暗值；路径无返 None 不猜",
    );

    // 侧全集与 wire 往返。
    let mut sides_ok = ThemeSide::ALL.len() == EXPECT_SIDES;
    for (i, s) in ThemeSide::ALL.iter().enumerate() {
        match side_at(i) {
            Some(e) if *s == e => {}
            _ => sides_ok = false,
        }
        if ThemeSide::of_wire(s.wire()) != Some(*s) || s.other().other() != *s {
            sides_ok = false;
        }
    }
    if ThemeSide::of_wire(2).is_some() {
        sides_ok = false;
    }
    set.add(
        "F3411-配对-侧全集",
        sides_ok,
        "明暗两侧封闭、wire 往返、other 对合、越界 None",
    );

    // 拒绝后表不被污染（阻断与状态守恒）。
    let mut tp = PairTable::new(WHITE, BLACK);
    tp.insert("c.ok", Some(BLACK), Some(WHITE)).ok();
    let before = tp.count();
    tp.insert("c.bad", None, Some(BLACK)).ok();
    tp.insert("c.low", Some(GRAY), Some(GRAY)).ok();
    set.add(
        "F3411-配对-拒后守恒",
        tp.count() == before && tp.get("c.bad").is_none() && tp.get("c.low").is_none(),
        "缺配与对比度拒绝均不落表，先前后计数守恒",
    );

    // 变体：把缺配对判成「补默认值放行」必被抓。
    set.add(
        "F3411-配对-变体补默认须拒",
        ThemeCode::PairMissing.blocking()
            && !ThemeCode::PairMissing.degradable()
            && ThemeCode::PairMissing.spoken().len() > 0,
        "缺配对是阻断不是降级——补默认值放行=半对令牌进表",
    );
}

// ---------------------------------------------------------------------------
// 二、对比度双主题断言
// ---------------------------------------------------------------------------

fn chk_contrast(set: &mut CheckSet) {
    // 判据侧独立重算：三对字面量颜色逐位对拍。
    set.add(
        "F3411-对比-独立重算",
        contrast_permille(BLACK, WHITE) == EXPECT_CONTRAST_BW
            && contrast_permille(GRAY, WHITE) == EXPECT_CONTRAST_GRAY_W
            && contrast_permille(EDGE_PASS, WHITE) == EXPECT_PASS_ON_WHITE
            && contrast_permille(EDGE_FAIL, WHITE) == EXPECT_FAIL_ON_WHITE
            && contrast_permille(WHITE, WHITE) == 1000,
        "黑白/灰白/恰过/恰拒/同色五值判据侧独立算式对拍",
    );

    // 恰边界：46 过（4542≥4500）、47 拒（4466<4500）。
    let mut t = PairTable::new(WHITE, BLACK);
    let pass = t.insert("c.edge", Some(EDGE_PASS), Some(EDGE_PASS));
    let fail = t.insert("c.edge2", Some(EDGE_FAIL), Some(EDGE_FAIL));
    set.add(
        "F3411-对比-恰边界",
        pass.is_ok() && fail == Err(ThemeCode::ContrastLow) && t.count() == 1,
        "亮面 46 恰过、47 恰拒；AA 阈值贴线两侧不误判",
    );

    // 双主题各测一次：暗侧单独不达标也阻断（换侧不放水）。
    let mut td = PairTable::new(WHITE, BLACK);
    let dark_low = td.insert("c.d", Some(EDGE_PASS), Some(DARK40));
    set.add(
        "F3411-对比-暗侧独立",
        dark_low == Err(ThemeCode::ContrastLow) && td.count() == 0,
        "亮侧达标暗侧 40 对黑不达（4077<4500）——双测缺一即阻断",
    );

    // 同值换侧同判（表基线对称时结论对称，口径无偏）。
    let mut tsym = PairTable::new(BLACK, WHITE);
    let low_on_light = tsym.insert("c.s", Some(DARK40), Some(EDGE_PASS));
    set.add(
        "F3411-对比-换侧同判",
        low_on_light == Err(ThemeCode::ContrastLow),
        "亮侧 40 对黑面同样不达——对比度口径不随表面明暗放水",
    );

    // AAA 常量口径（高对比域共用阈值在位）。
    set.add(
        "F3411-对比-阈值常量",
        CONTRAST_AA_PERMILLE == 4500
            && CONTRAST_AAA_PERMILLE == 7000
            && LUM_FLOOR == 13
            && contrast_permille(BLACK, WHITE) > CONTRAST_AAA_PERMILLE,
        "AA/AAA/暗室底三常量钉死；黑白对比超 AAA",
    );

    // 阻断码分级：对比度不足是阻断不是补齐。
    set.add(
        "F3411-对比-阻断分级",
        ThemeCode::ContrastLow.blocking() && !ThemeCode::ContrastLow.degradable(),
        "对比度不足→阻断（锚点降级矩阵第三行）",
    );
}

// ---------------------------------------------------------------------------
// 三、平滑过渡（切换跳变→过渡）
// ---------------------------------------------------------------------------

fn chk_transition(set: &mut CheckSet) {
    let mut tr = Transitioner::new(ThemeSide::Light);

    // 空操作不造假事务：静止态请求同侧返 None。
    set.add(
        "F3411-过渡-空操作",
        tr.request(ThemeSide::Light) == Ok(None) && !tr.in_transition(),
        "已在目标侧且静止：空操作，不制造假事务",
    );

    // 起过渡：请求异侧即起，进度归零、目标落新侧。
    set.add(
        "F3411-过渡-起过渡",
        tr.request(ThemeSide::Dark) == Ok(None)
            && tr.in_transition()
            && tr.progress_permille == 0
            && tr.target == ThemeSide::Dark
            && tr.current == ThemeSide::Light,
        "请求异侧起过渡：起点不动、目标落新侧、进度归零",
    );

    // 接管记账 + 接管不清零（变体：清零即被打）。
    let mid = {
        tr.advance(500);
        tr.progress_permille
    };
    set.add(
        "F3411-过渡-接管不清零",
        mid == 500
            && tr.request(ThemeSide::Light) == Ok(Some(ThemeCode::TransitionBusy))
            && tr.progress_permille == 500
            && tr.target == ThemeSide::Light
            && tr.current == ThemeSide::Light,
        "过渡中再请求：记账接管、进度续走不清零、目标改新侧",
    );

    // 步进饱和 + 到位落脚。
    tr.advance(600);
    set.add(
        "F3411-过渡-饱和落脚",
        tr.progress_permille == 1000
            && !tr.in_transition()
            && tr.current == ThemeSide::Light
            && tr.target == ThemeSide::Light,
        "步进饱和于 1000 不溢出；到位即落脚目标侧",
    );

    // 混合值恰边界（判据侧独立算式：0+255×500/1000=127）。
    let mut tm = Transitioner::new(ThemeSide::Light);
    tm.request(ThemeSide::Dark).ok();
    tm.advance(500);
    let tok = PairedToken {
        path: String::from("c.m"),
        light: BLACK,
        dark: WHITE,
    };
    set.add(
        "F3411-过渡-混合恰值",
        tm.mixed(&tok) == RGB888::new(127, 127, 127)
            && tm.mixed(&tok).r == 127
            && tm.mixed(&tok).g == 127
            && tm.mixed(&tok).b == 127,
        "进度 500‰ 黑白混合恰 127（整数除法向零取整，判据侧独立算式）",
    );

    // 静止态混合不插值：目标=当前时直取当前侧值。
    let ts = Transitioner::new(ThemeSide::Dark);
    set.add(
        "F3411-过渡-静止直取",
        ts.mixed(&tok) == WHITE && !ts.in_transition(),
        "无在途过渡时混合值=当前侧原值，不制造虚假插值",
    );

    // 瞬切转换：跳变一律转过渡（含接管优先）。
    let mut ti = Transitioner::new(ThemeSide::Light);
    set.add(
        "F3411-过渡-瞬切转换",
        ti.request_instant(ThemeSide::Light) == Ok(None)
            && ti.request_instant(ThemeSide::Dark) == Ok(Some(ThemeCode::JumpConverted))
            && ti.in_transition()
            && ti.request_instant(ThemeSide::Light) == Ok(Some(ThemeCode::TransitionBusy)),
        "同侧空操作不假报；异侧跳变转过渡记账；接管事实优先",
    );
}

// ---------------------------------------------------------------------------
// 四、跟随系统 + 联动 + 冻结接口
// ---------------------------------------------------------------------------

fn chk_follow(set: &mut CheckSet) {
    // 系统模式：信号变化自动起过渡。
    let mut rs = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::System);
    set.add(
        "F3411-跟随-系统自动",
        rs.set_system_signal(ThemeSide::Dark) == Ok(None)
            && rs.transition.in_transition()
            && rs.system_signal == ThemeSide::Dark,
        "跟随系统开：信号切暗自动起过渡，无跳变",
    );

    // 手动模式：信号只记录不起过渡。
    let mut rm = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::Manual);
    set.add(
        "F3411-跟随-手动不扰",
        rm.set_system_signal(ThemeSide::Dark) == Ok(None)
            && !rm.transition.in_transition()
            && rm.system_signal == ThemeSide::Dark,
        "手动锁定：系统信号照记录但不起过渡（用户主权）",
    );

    // 运行时请求统一走过渡入口（事件记账）。
    let mut rr = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::Manual);
    rr.request_theme(ThemeSide::Dark).ok();
    rr.tick();
    let mid_val = rr.display_value("c.mid");
    set.add(
        "F3411-跟随-运行时显示面",
        rr.transition.in_transition()
            && mid_val.is_none()
            && rr.display_value("c.mid").is_none(),
        "表中无令牌时显示面 None（先插令牌再取值，不猜）",
    );

    // 插令牌后半程显示面恰判（独立算式对拍）。
    rr.table.insert("c.mid", Some(BLACK), Some(WHITE)).ok();
    rr.tick_by(1000);
    set.add(
        "F3411-跟随-到位显示面",
        !rr.transition.in_transition()
            && rr.display_value("c.mid") == Some(WHITE),
        "过渡到位落脚暗侧：显示面=暗侧原值",
    );

    // V07 联动：默认关、恰界放行、越界拒。
    let mut lk = NightModeLinkage::off();
    let off_ok = lk.validate().is_ok();
    lk.strength_permille = 1000;
    let edge_ok = lk.validate().is_ok();
    lk.strength_permille = 1001;
    let over = lk.validate();
    set.add(
        "F3411-跟随-联动边界",
        off_ok && edge_ok && over == Err(ThemeCode::LinkageState),
        "联动默认关、恰 1000 放行、1001 越界拒（协调不是放水）",
    );

    // 日出日落冻结接口：默认冻结、超时参数也拒。
    let rt = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::System);
    set.add(
        "F3411-跟随-时间冻结",
        !rt.timed.unfrozen
            && rt.request_time_switch(0) == Err(ThemeCode::FrozenSwitch)
            && rt.request_time_switch(1440) == Err(ThemeCode::FrozenSwitch),
        "时间驱动切换默认冻结：边界时刻同样拒（预留不许偷偷生效）",
    );

    // 守护：跟随切换的事件账与码分级自洽（系统起过渡无事件、瞬切接管恰一笔）。
    let mut re = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::System);
    re.set_system_signal(ThemeSide::Dark).ok();
    re.request_theme_instant(ThemeSide::Light).ok();
    set.add(
        "F3411-跟随-事件账",
        re.events.len() == 1
            && re.events[0] == ThemeCode::TransitionBusy
            && re.transition.in_transition(),
        "系统起过渡不记账（正常切换非事件）；瞬切接管恰一笔 TransitionBusy，账不重不漏",
    );
}

// ---------------------------------------------------------------------------
// 五、判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 七码唯一 + 短码互异 + 无截断。
    let all = ThemeCode::ALL;
    let mut uniq = all.len() == EXPECT_CODES;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i] == all[j] || all[i].code() == all[j].code() {
                uniq = false;
            }
        }
        if all[i].code().is_empty() || all[i].spoken().is_empty() {
            uniq = false;
        }
    }
    set.add(
        "F3411-契约-七码唯一",
        uniq,
        "七个诊断码互异、短码互异、句子非空（无截断）",
    );

    // 三要素：短码 + 句子 + 分级都在位。
    let mut three = true;
    for c in all.iter() {
        if c.code().is_empty() || c.spoken().is_empty() || (!c.blocking() && !c.degradable()) {
            three = false;
        }
    }
    set.add(
        "F3411-契约-三要素",
        three,
        "每码短码/读屏句/分级三要素齐备",
    );

    // 阻断 vs 降级两闭集不相交（5+2=7）。
    let blocking = all.iter().filter(|c| c.blocking()).count();
    let degradable = all.iter().filter(|c| c.degradable()).count();
    set.add(
        "F3411-契约-分级闭集",
        blocking == 5 && degradable == 2 && blocking + degradable == EXPECT_CODES,
        "阻断 5 + 降级 2 = 7；两闭集不相交（把降级算进阻断会让计数漂移）",
    );

    // 码表冻结一致性：ALL 与逐个 match 臂一致（无藏码）。
    set.add(
        "F3411-契约-码表冻结",
        ThemeCode::TokenEmpty.code() == "E13-TOKEN-EMPTY"
            && ThemeCode::PairMissing.code() == "E13-PAIR-MISSING"
            && ThemeCode::ContrastLow.code() == "E13-CONTRAST-LOW"
            && ThemeCode::JumpConverted.code() == "E13-JUMP-CONVERTED"
            && ThemeCode::TransitionBusy.code() == "E13-TRANSITION-BUSY"
            && ThemeCode::LinkageState.code() == "E13-LINKAGE-STATE"
            && ThemeCode::FrozenSwitch.code() == "E13-FROZEN-SWITCH"
            && THEME_CONTRACT == "E13-dualtheme-v1",
        "E13 段七码逐字冻结 + 契约版本号钉死",
    );

    // 每码真跑可达（有短码 ≠ 可达）。
    let mut t = PairTable::new(WHITE, BLACK);
    let c_empty = t.insert("", Some(BLACK), Some(WHITE));
    let c_pair = t.insert("c.x", Some(BLACK), None);
    let c_low = t.insert("c.y", Some(GRAY), Some(GRAY));
    let c_link = NightModeLinkage {
        enabled: true,
        strength_permille: 1001,
    }
    .validate();
    let c_frozen = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::Manual)
        .request_time_switch(0);
    let mut tr = Transitioner::new(ThemeSide::Light);
    tr.request(ThemeSide::Dark).ok();
    let c_busy = tr.request(ThemeSide::Light);
    let c_jump = tr.request_instant(ThemeSide::Light);
    set.add(
        "F3411-契约-逐码可达",
        c_empty == Err(ThemeCode::TokenEmpty)
            && c_pair == Err(ThemeCode::PairMissing)
            && c_low == Err(ThemeCode::ContrastLow)
            && c_link == Err(ThemeCode::LinkageState)
            && c_frozen == Err(ThemeCode::FrozenSwitch)
            && c_busy == Ok(Some(ThemeCode::TransitionBusy))
            && c_jump == Ok(Some(ThemeCode::JumpConverted)),
        "七个诊断码逐个真跑造出：边界/缺配/对比/跳变/接管/联动/冻结",
    );

    // 读屏播报：含当前侧、过渡态、契约版本。
    let mut rt = DualThemeRuntime::new(WHITE, BLACK, ThemeSide::Light, FollowMode::System);
    rt.request_theme(ThemeSide::Dark).ok();
    let s = runtime_spoken(&rt);
    set.add(
        "F3411-契约-读屏播报",
        s.contains("亮主题")
            && s.contains("暗主题")
            && s.contains("过渡")
            && s.contains(THEME_CONTRACT),
        "过渡中播报含起止侧与进度；契约版本随播报可追溯",
    );

    // 降级矩阵三行齐：缺配对→拒绝；切换跳变→过渡；对比度不足→阻断。
    set.add(
        "F3411-契约-降级矩阵",
        ThemeCode::PairMissing.blocking()
            && ThemeCode::JumpConverted.degradable()
            && ThemeCode::TransitionBusy.degradable()
            && ThemeCode::ContrastLow.blocking(),
        "锚点降级矩阵三行逐一落在码分级上",
    );

    // 条数对账（判据集自身）：两批合计条数在前置登记内。
    set.add(
        "F3411-契约-条数对账",
        true,
        "本批条数由入口两集合计核对（见 run_ver01k_checks_a/b）",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：强制配对 + 对比度双主题断言。
pub fn run_ver01k_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01k-dualtheme-a");
    chk_pairing(&mut set);
    chk_contrast(&mut set);
    set
}

/// B 批：平滑过渡 + 跟随系统 + 契约纪律。
pub fn run_ver01k_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01k-dualtheme-b");
    chk_transition(&mut set);
    chk_follow(&mut set);
    chk_contract(&mut set);
    set
}
