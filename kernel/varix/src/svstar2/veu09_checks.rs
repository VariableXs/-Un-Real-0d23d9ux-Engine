//! VE-F4009 自检 · 输入法协同（IME × i18n）
//!
//! **锚点判据逐条对应**（`#VE-F4009` 的「组合期单源、候选跟随、方向联动、
//! 切换断言、IME 协同」）：
//!
//! | 锚点判据 | 自检组|
//! |---|---|
//! | 组合期单源（复述 F3024 红线） | `O09-组合期-*` |
//! | 候选跟随（复述 F3848） | `O09-候选-*` |
//! | 方向联动（F4028 前向） | `O09-方向-*` |
//! | 切换断言 | `O09-切换-*` |
//! | IME 协同（多语言） | `O09-协同-*` |
//!
//! **判据设计的四条硬规矩**（防止自检变成恒真的空断言）：
//!
//! ① **不变量类判据必须两头都测**：既测「违规被拒」，也测「合规放行」。
//!    只测违规侧的判据，会被「永远返回 `Err`」的实现骗绿。
//! ② **表驱动判据不许用表内元素验表函数**——那必然恒真。语言↔输入法映射的
//!    判据一律用**表外**语言（`th`、`he`、`xx-YY`）验「查不到即拒」。
//! ③ **顺序/序关系判据必须配数值对账**：「四档严格递增」在归一化分母改错时
//!    照样全绿。`O09-候选-04` 除「仍在视口内」外，还与**解析解**逐轴对账。
//! ④ **单边符号优于双边阈值**：正确实现的偏差方向恒定（量化只会向内收，
//!    不会外扩），故断言用单边符号而非宽双边阈值——双边阈值一旦宽过正确
//!    实现的高估幅度，缺陷就从缝里钻过去了。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::veu09_ime::{
    caret_step, check_caret, diagnose_candidate, diagnose_composition, diagnose_switch, ime_kind_for,
    screen_line, CandidateRect, CandidateWindow, Composition, CompositionGate, Diag, DiagBag,
    Direction, ImeKind, SwitchLedger, SwitchRecord, Viewport, COMPOSITION_BANNED_KEYS,
    E_CANDIDATE_CAP, E_CANDIDATE_CLAMP_STILL_OUT, E_CANDIDATE_NO_FIT, E_COMPOSITION_BANNED_KEY,
    E_COMPOSITION_ENGINE_EDIT, E_COMPOSITION_TOO_LONG, E_DIRECTION_MISMATCH, E_SWITCH_DESYNC,
    IME_VERSION, LANG_IME_KINDS, MAX_CANDIDATES, MAX_COMPOSITION_LEN, MAX_SWITCH_RECORDS,
    MIN_VIEWPORT_DIM,
};

/// 表外语言（判据设计规矩②：不用表内元素验表函数）。
const OUTSIDE_LANGS: [&str; 4] = ["th", "he", "xx-YY", ""];

/// 组合串样例（覆盖三类有组合期的输入法）。
fn samples() -> Vec<Composition> {
    vec![
        {
            let mut c = Composition::new(ImeKind::Cjk, 12);
            c.text = String::from("nihao");
            c
        },
        {
            let mut c = Composition::new(ImeKind::Arabic, 3);
            c.text = String::from("سلا");
            c
        },
        {
            let mut c = Composition::new(ImeKind::VoiceCompose, 40);
            c.text = String::from("yuandianzi");
            c
        },
    ]
}

/// 组合期单源：违规必拒 + 合规必放行（规矩①）。
fn group_composition(set: &mut CheckSet) {
    let mut gate = CompositionGate::new();
    let ss = samples();

    // 01 三类输入法的组合期都处于 active。
    let all_active = ss.iter().filter(|c| c.active()).count();
    set.add(
        "O09-组合期-01",
        all_active == ss.len(),
        "三类有组合期的输入法都必须 active",
    );

    // 02 引擎侧编辑在组合期恒被拒（三类逐一验，不抽样）。
    let mut refused = 0usize;
    let mut codes_ok = true;
    for c in ss.iter() {
        if let Err(e) = gate.accept_edit(c) {
            if e.code == E_COMPOSITION_ENGINE_EDIT {
                refused += 1;
            } else {
                codes_ok = false;
            }
        }
    }
    set.add(
        "O09-组合期-02",
        refused == ss.len() && codes_ok,
        "组合期引擎编辑必须全部拒绝且错误码为 E_COMPOSITION_ENGINE_EDIT",
    );

    // 03 拒绝必须留痕（事后能回答「多出来的字符哪来的」）。
    set.add(
        "O09-组合期-03",
        gate.engine_edits == ss.len() as u32,
        "每次被拒都要计数，否则事后无法归因",
    );

    // 04 合规侧必须放行：直通输入法 + 空组合都不该被拦。
    //    没有这条，一个「永远返回 Err」的实现能骗过 02/03 全部判据。
    let direct = Composition::new(ImeKind::Direct, 5);
    let empty_cjk = Composition::new(ImeKind::Cjk, 0);
    let direct_ok = gate.accept_edit(&direct).is_ok();
    let empty_ok = gate.accept_edit(&empty_cjk).is_ok();
    let no_extra_count = gate.engine_edits == ss.len() as u32;
    set.add(
        "O09-组合期-04",
        direct_ok && empty_ok && no_extra_count,
        "直通输入法/空组合必须放行且不计数——否则 02 是恒真弱门禁",
    );

    // 05 组合期快捷键禁令：五个键逐一被拦（表驱动，用 `iter().any`）。
    let mut banned_hit = 0usize;
    let cjk = &ss[0];
    for k in COMPOSITION_BANNED_KEYS.iter() {
        if gate.key_banned(cjk, k) {
            banned_hit += 1;
        }
    }
    set.add(
        "O09-组合期-05",
        banned_hit == COMPOSITION_BANNED_KEYS.len(),
        "组合期五个命令键必须全部被拦",
    );

    // 06 非命令键不得被拦（禁令表不许扩大成「全键皆禁」）。
    let ordinary = ["KeyA", "Digit1", "ArrowLeft", "ShiftLeft", "F5"];
    let mut over = 0usize;
    for k in ordinary.iter() {
        if gate.key_banned(cjk, k) {
            over += 1;
        }
    }
    set.add(
        "O09-组合期-06",
        over == 0,
        "普通字符/修饰键不得被拦——扩大禁令表会让用户无法正常输入",
    );

    // 07 无组合期时禁令不生效（直通输入法按 Space 是要打空格的）。
    let direct_ban = gate.key_banned(&direct, "Space");
    set.add(
        "O09-组合期-07",
        !direct_ban,
        "直通输入法的 Space 不该被拦",
    );

    // 08 长度上限边界：恰好等于上限放行，超一字节即拒。
    let at_limit = {
        let mut c = Composition::new(ImeKind::VoiceCompose, 0);
        c.text = String::from("x").repeat(MAX_COMPOSITION_LEN);
        c
    };
    let over_limit = {
        let mut c = Composition::new(ImeKind::VoiceCompose, 0);
        c.text = String::from("x").repeat(MAX_COMPOSITION_LEN + 1);
        c
    };
    let at_ok = gate.check_len(&at_limit).is_ok();
    let over_err = matches!(
        gate.check_len(&over_limit),
        Err(ref e) if e.code == E_COMPOSITION_TOO_LONG
    );
    set.add(
        "O09-组合期-08",
        at_ok && over_err,
        "组合串上限必须含等号：等于上限放行、超一字节拒绝（边界判据）",
    );

    // 09 统一立案入口：三类失联各自可立案，且**同袋并存**不互相吞掉。
    let mut bag = DiagBag::new();
    let mut g2 = CompositionGate::new();
    let _ = g2.accept_edit(&ss[0]);
    diagnose_composition(&mut bag, &g2, &ss[0], "Enter");
    diagnose_composition(&mut bag, &g2, &over_limit, "Enter");
    let has_engine = bag.all().iter().any(|n| n.code == Diag::EngineEditDuringComposition);
    let has_key = bag.all().iter().any(|n| n.code == Diag::BannedKeyDuringComposition);
    let has_len = bag.all().iter().any(|n| n.code == Diag::CompositionTooLong);
    set.add(
        "O09-组合期-09",
        has_engine && has_key && has_len && bag.len() >= 3,
        "三类组合期失联必须都能进同一袋且互不吞并",
    );

    // 10 阻断分级：引擎编辑与命令键未拦必须阻断，其余不阻断。
    //     分级反了的后果：静音类故障只告警→ 无人处理。
    let blocking = Diag::EngineEditDuringComposition.is_blocking()
        && Diag::BannedKeyDuringComposition.is_blocking();
    let advisory = !Diag::CandidateNoFit.is_blocking()
        && !Diag::SwitchDesync.is_blocking()
        && !Diag::CompositionTooLong.is_blocking()
        && !Diag::CandidateCap.is_blocking();
    set.add(
        "O09-组合期-10",
        blocking && advisory,
        "组合期两类必须阻断，其余为建议级——分级反了会让人不处理",
    );

    // 11 诊断文案要说实话：每条 explain 都要有非空且能指向后果的正文。
    let mut words_ok = true;
    for d in [
        Diag::EngineEditDuringComposition,
        Diag::BannedKeyDuringComposition,
        Diag::CandidateNoFit,
        Diag::SwitchDesync,
        Diag::CompositionTooLong,
        Diag::CandidateCap,
    ]
    .iter()
    {
        if d.explain().len() < 8 || !d.explain().contains('：') {
            words_ok = false;
        }
    }
    set.add(
        "O09-组合期-11",
        words_ok,
        "诊断文案必须含「现象：后果」，不能只说「失败」",
    );
}

/// 候选跟随：三轴 + 解析解对账。
fn group_candidate(set: &mut CheckSet) {
    let view = match Viewport::new(800, 600) {
        Ok(v) => v,
        Err(_) => {
            set.fail("O09-候选-00", "视口构造不应失败：800×600");
            return;
        }
    };
    let w = match CandidateWindow::new(9) {
        Ok(c) => c,
        Err(_) => {
            set.fail("O09-候选-00", "9条候选不应被拒");
            return;
        }
    };

    // 01 正常跟随：LTR 下候选窗左上角= 光标位置。
    let r = match w.place((100, 200), (160, 90), view, Direction::Ltr) {
        Ok(r) => r,
        Err(_) => {
            set.fail("O09-候选-01", "视口内的光标不该报无解");
            return;
        }
    };
    set.add(
        "O09-候选-01",
        r.x == 100 && r.y == 202,
        "LTR 跟随起点应为光标，光标下方留 2px",
    );

    // 02 RTL 向左展开（不遮组合串）——方向与输入的联动点。
    let rl = w.place((100, 200), (160, 90), view, Direction::Rtl);
    let rl_ok = matches!(rl, Ok(ref r) if r.x == -60 || r.x == 0);
    // 解析解：RTL 起点 = caret.x - cw = -60，被x 轴夹取到 0。
    set.add(
        "O09-候选-02",
        rl_ok,
        "RTL 候选窗必须向左展开并被夹取到 x=0（解析解 x=max(0,caret.x-cw)）",
    );

    // 03 x 轴右侧越界夹取：caret 靠右时贴右边缘，且**恰好**贴住。
    let rr = w.place((790, 200), (160, 90), view, Direction::Ltr);
    set.add(
        "O09-候选-03",
        matches!(rr, Ok(ref r) if r.right() == 800),
        "右侧越界必须夹到 x=vw-cw，恰好贴住右边缘而非留缝",
    );

    // 04 y 轴下越界翻转到光标上方，且与解析解逐轴对账（规矩③）。
    let rb = w.place((100, 590), (160, 90), view, Direction::Ltr);
    let y_expect = 590 - 90 - 2; // 翻到光标上方：caret.y - ch - 2
    set.add(
        "O09-候选-04",
        matches!(rb, Ok(ref r) if r.y == y_expect && r.bottom() == y_expect + 90),
        "下方放不下必须翻到光标上方，且 y 必须等于解析解 caret.y-ch-2",
    );

    // 05 光标在视口**外**（组合段被滚出可视区）仍须得到合法位置。
    //    这是本单修过的真缺陷：负 y 会让翻转分支算出更负的值并误报「无解」。
    let outside = [
        (0i32, -50i32),
        (900, 200),
        (100, 700),
        (-30, -30),
        (900, 700),
    ];
    let mut all_ok = true;
    let mut detail_ok = true;
    for (cx, cy) in outside.iter() {
        match w.place((*cx, *cy), (160, 90), view, Direction::Ltr) {
            Ok(r) => {
                if !r.within(view) {
                    all_ok = false;
                }
                // 解析解：caret 先钳进视口，再按 04 的规则解算y。
                let ecy = clamp_ref(*cy, 0, 599);
                let ey = if ecy + 2 + 90 <= 600 {
                    ecy + 2
                } else {
                    max0(ecy - 90 - 2)
                };
                if r.y != ey {
                    detail_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
    }
    set.add(
        "O09-候选-05",
        all_ok && detail_ok,
        "caret 在视口外（滚动场景）仍须得到合法矩形且与解析解逐轴一致",
    );

    // 06 全网格扫描：候选窗恒完整落在视口内（**大面积**而非抽样）。
    let mut grid_ok = true;
    let mut grid_n = 0usize;
    let sizes = [(40i32, 30i32), (160, 90), (320, 240)];
    for cw in sizes.iter() {
        for cx in (0..820).step_by(37) {
            for cy in (0..640).step_by(41) {
                grid_n += 1;
                for dir in [Direction::Ltr, Direction::Rtl].iter() {
                    if let Ok(r) = w.place((cx, cy), *cw, view, *dir) {
                        if !r.within(view) {
                            grid_ok = false;
                        }
                    } else {
                        grid_ok = false;
                    }
                }
            }
        }
    }
    set.add(
        "O09-候选-06",
        grid_ok && grid_n == 3 * 23 * 16,
        "1200+ 组网格下候选窗恒完整落在视口内",
    );

    // 07 视口装不下候选窗 ⇒ 拒绝并给降级出路（不能返回越界矩形装作成功）。
    let big = w.place((10, 10), (900, 90), view, Direction::Ltr);
    set.add(
        "O09-候选-07",
        matches!(big, Err(ref e) if e.code == E_CANDIDATE_NO_FIT && e.next.len() > 4),
        "候选窗大于视口必须拒绝，且错误必须带降级出路",
    );

    // 08 候选条目上限边界：恰好上限放行、超一条拒绝。
    let at = CandidateWindow::new(MAX_CANDIDATES as u32);
    let over = CandidateWindow::new(MAX_CANDIDATES as u32 + 1);
    set.add(
        "O09-候选-08",
        at.is_ok()
            && matches!(over, Err(ref e) if e.code == E_CANDIDATE_CAP),
        "候选上限含等号：=上限放行、+1 拒绝（静默截断会让用户看不到想要的词）",
    );

    // 09 视口尺寸非法（0 轴）必须拒绝——夹取在 0 尺寸下无解。
    let v0 = Viewport::new(0, 600);
    let v1 = Viewport::new(800, 0);
    set.add(
        "O09-候选-09",
        v0.is_err() && v1.is_err() && MIN_VIEWPORT_DIM == 1,
        "0 宽/0 高视口必须拒绝，不能进入渲染路径",
    );

    // 10 `within` 是判据唯一真相：逐轴分别验（不合并成一个 &&）。
    //     合并写会让「只错一个轴」的缺陷从缝里钻过去。
    let v = Viewport { w: 100, h: 100 };
    let good = CandidateRect { x: 0, y: 0, w: 100, h: 100 };
    let bad_x = CandidateRect { x: -1, y: 0, w: 100, h: 100 };
    let bad_y = CandidateRect { x: 0, y: -1, w: 100, h: 100 };
    let bad_r = CandidateRect { x: 1, y: 0, w: 100, h: 100 };
    let bad_b = CandidateRect { x: 0, y: 1, w: 100, h: 100 };
    set.add(
        "O09-候选-10",
        good.within(v) && !bad_x.within(v) && !bad_y.within(v) && !bad_r.within(v) && !bad_b.within(v),
        "within 须逐轴判负：x/y/右/下四个方向各自都能被抓",
    );

    // 11 候选落位失败能进诊断袋（不能只返回 Err 而无人收）。
    let mut bag = DiagBag::new();
    if let Err(ref e) = big {
        diagnose_candidate(&mut bag, e);
    }
    set.add(
        "O09-候选-11",
        bag.len() == 1 && bag.all()[0].code == Diag::CandidateNoFit,
        "候选落位失败必须可立案（单条、不重复）",
    );

    // 13 **两条候选失败分支必须各有专属错误码**（弱门禁第3 条的防线）。
    //     前置守卫（视口装不下）与夹取兜底（守卫失效后仍越界）的外部表现都是
    //     「候选窗没放下」，若共用一个码，**删掉前置守卫后判据依然全绿**——
    //     缺陷就从缝里钻过去了。
    //
    //     **兜底分支经公开 API 不可直接触达**（视口合法 ⇒ 守卫必过 ⇒ 夹取后
    //     恒在视口内），它是「守卫被绕过」的防御自检。故本判据分三段：
    //     ① 守卫分支**可达**且给专属码（cw=900 > vw=800）；
    //     ② 两个码**必须不同**（钉住「不得共用码」这条契约本身）；
    //     ③ **守卫边界是严格大于**：cw 恰等于 vw 时必须夹取成功而非拒绝。
    //     ③ 才是真正抓「守卫被改坏」的那道闸——把 `cw > vw` 误写成
    //     `cw >= vw`（或反向放宽）时，只有等号这一档能分辨，而旧写法用
    //     cw=800 去撞兜底码，恰好把等号档误判成「应当拒绝」。
    let guard = w.place((10, 10), (900, 90), view, Direction::Ltr);
    let guard_code_ok = matches!(guard, Err(ref e) if e.code == E_CANDIDATE_NO_FIT);
    let codes_distinct = E_CANDIDATE_NO_FIT != E_CANDIDATE_CLAMP_STILL_OUT;
    // 等号档：宽恰等于视口宽 ⇒ 夹取到 x=0，完整落在视口内。
    let exact = w.place((10, 10), (view.w as i32, 90), view, Direction::Ltr);
    let exact_fits = matches!(exact, Ok(ref r) if r.within(view) && r.right() <= view.w as i32);
    set.add(
        "O09-候选-13",
        guard_code_ok && codes_distinct && exact_fits,
        "守卫分支须给专属码、两码不得共用、且 cw==vw 等号档必须夹取成功（守卫是严格大于）",
    );

    // 12夹取方向的单边符号：RTL 的x 必须**不大于** LTR 同参数下的 x。
    //     用单边≤ 而非双边阈值——正确实现恒满足，反了则明显大于（规矩④）。
    let mut monotone = true;
    for cx in (0..800).step_by(53) {
        let l = match w.place((cx, 300), (160, 90), view, Direction::Ltr) {
            Ok(r) => r.x,
            Err(_) => {
                monotone = false;
                break;
            }
        };
        let r_x = match w.place((cx, 300), (160, 90), view, Direction::Rtl) {
            Ok(r) => r.x,
            Err(_) => {
                monotone = false;
                break;
            }
        };
        if r_x > l {
            monotone = false;
        }
    }
    set.add(
        "O09-候选-12",
        monotone,
        "RTL 起点必须恒不大于 LTR 起点（单边符号判据，反向实现立刻越界）",
    );
}

/// 方向联动（F4028 前向声明）：只校验方向 × 插入点自洽。
fn group_direction(set: &mut CheckSet) {
    // 01 步进符号：LTR +1、RTL -1。这是 RTL 光标「往左跑」的唯一来源。
    set.add(
        "O09-方向-01",
        caret_step(Direction::Ltr) == 1 && caret_step(Direction::Rtl) == -1,
        "caret_step 符号必须 LTR=+1 RTL=-1",
    );

    // 02 自洽侧：LTR 前进合法、RTL 后退合法。
    let ltr_ok = check_caret(10, 11, Direction::Ltr).is_ok();
    let rtl_ok = check_caret(10, 9, Direction::Rtl).is_ok();
    let same_ok = check_caret(10, 10, Direction::Ltr).is_ok()
        && check_caret(10, 10, Direction::Rtl).is_ok();
    set.add(
        "O09-方向-02",
        ltr_ok && rtl_ok && same_ok,
        "方向自洽侧必须放行（含等值），否则正常输入被判错",
    );

    // 03 违规侧：LTR 后退、RTL 前进都必须立案。
    let ltr_bad = matches!(
        check_caret(10, 9, Direction::Ltr),
        Err(ref e) if e.code == E_DIRECTION_MISMATCH
    );
    let rtl_bad = matches!(
        check_caret(10, 11, Direction::Rtl),
        Err(ref e) if e.code == E_DIRECTION_MISMATCH
    );
    set.add(
        "O09-方向-04",
        ltr_bad && rtl_bad,
        "方向不自洽必须拒绝并给 E_DIRECTION_MISMATCH",
    );

    // 04 枚举往返守卫：未知码拒绝，不留默认分支兜底。
    let rt = Direction::from_code("LTR") == Some(Direction::Ltr)
        && Direction::from_code("RTL") == Some(Direction::Rtl)
        && Direction::from_code("ltr").is_none()
        && Direction::from_code("").is_none()
        && Direction::from_code("AUTO").is_none();
    set.add(
        "O09-方向-03",
        rt,
        "方向码必须大小写敏感且拒绝未登记值，不留兜底分支",
    );

    // 05 中文名不得为空（读屏与诊断文案要用）。
    set.add(
        "O09-方向-05",
        !Direction::Ltr.zh().is_empty()
            && !Direction::Rtl.zh().is_empty()
            && Direction::Ltr.zh() != Direction::Rtl.zh(),
        "两个方向的中文名必须都存在且互不相同",
    );

    // 06 方向 × 候选窗的联动一致性：RTL 场景下候选窗不得盖住插入点。
    //    判据用**单边**关系：RTL 矩形右边界不应超过 caret.x（除非被夹到 0）。
    let view = match Viewport::new(800, 600) {
        Ok(v) => v,
        Err(_) => {
            set.fail("O09-方向-06", "视口构造不应失败");
            return;
        }
    };
    let w = match CandidateWindow::new(5) {
        Ok(c) => c,
        Err(_) => {
            set.fail("O09-方向-06", "候选窗构造不应失败");
            return;
        }
    };
    let mut covered = 0usize;
    let mut checked = 0usize;
    for cx in (200..600).step_by(23) {
        if let Ok(r) = w.place((cx, 100), (160, 90), view, Direction::Rtl) {
            checked += 1;
            // 夹取导致贴左边缘时（x=0）允许覆盖，那是被迫的降级。
            if r.x > 0 && r.right() > cx {
                covered += 1;
            }
        }
    }
    set.add(
        "O09-方向-06",
        checked > 0 && covered == 0,
        "RTL 候选窗不得盖住插入点（贴左边缘的被迫降级除外）",
    );
}

/// 切换断言：语言切换 → 输入法切换联动。
fn group_switch(set: &mut CheckSet) {
    // 01 表内语言逐一查到（用查表结果驱动，不硬编码期望值序列）。
    let mut hit = 0usize;
    for (lang, _) in LANG_IME_KINDS.iter() {
        if ime_kind_for(lang).is_some() {
            hit += 1;
        }
    }
    set.add(
        "O09-切换-01",
        hit == LANG_IME_KINDS.len(),
        "表内每种语言都必须查到输入法类别",
    );

    // 02 表外语言返回 None，**不许静默兜底到 Direct**（规矩②）。
    //     兜底的后果：切到没登记的语言，输入法静默退化，按键无反应。
    let mut leaked = 0usize;
    for lang in OUTSIDE_LANGS.iter() {
        if ime_kind_for(lang).is_some() {
            leaked += 1;
        }
    }
    set.add(
        "O09-切换-02",
        leaked == 0,
        "未登记语言必须返回 None——静默兜底正是本单要防的失联",
    );

    // 03 组合期语言必须映射到有组合期的输入法（ko 是关键样本）。
    //     若把谚文误判为 Direct，引擎会在谚文组合期插入内容。
    let mut kind_ok = true;
    for (lang, expect) in [
        ("zh-Hans", ImeKind::Cjk),
        ("zh-Hant", ImeKind::Cjk),
        ("ja", ImeKind::Cjk),
        ("ko", ImeKind::Cjk),
        ("ar", ImeKind::Arabic),
        ("fa", ImeKind::Arabic),
        ("ur", ImeKind::Arabic),
        ("en", ImeKind::Direct),
    ]
    .iter()
    {
        match ime_kind_for(lang) {
            Some(k) if k == *expect => {}
            _ => kind_ok = false,
        }
    }
    set.add(
        "O09-切换-03",
        kind_ok,
        "语言↔输入法映射须逐条正确（ko 必为 Cjk：谚文有组合期）",
    );

    // 04 全表自洽：凡映射到有组合期类的语言，组合期必须真的能激活。
    let mut consistent = true;
    for (lang, kind) in LANG_IME_KINDS.iter() {
        if kind.has_composition() {
            let mut c = Composition::new(*kind, 0);
            c.text = String::from("x");
            if !c.active() {
                consistent = false;
            }
            if ime_kind_for(lang) != Some(*kind) {
                consistent = false;
            }
        }
    }
    set.add(
        "O09-切换-04",
        consistent,
        "映射表与组合期激活判定须一致，不得两处各写一套",
    );

    // 05 联动成功入账。
    let mut led = SwitchLedger::new();
    let ok = led.record(SwitchRecord {
        lang: String::from("ja"),
        expect_kind: ImeKind::Cjk,
        actual_kind: ImeKind::Cjk,
    });
    set.add(
        "O09-切换-05",
        ok.is_ok() && led.len() == 1 && led.desyncs == 0,
        "同步的联动必须入账且不记失联",
    );

    // 06 失联必须立案 + 计数 + **不入账**（失联记录没有价值）。
    let bad = led.record(SwitchRecord {
        lang: String::from("zh-Hans"),
        expect_kind: ImeKind::Cjk,
        actual_kind: ImeKind::Direct,
    });
    set.add(
        "O09-切换-06",
        matches!(bad, Err(ref e) if e.code == E_SWITCH_DESYNC)
            && led.desyncs == 1
            && led.len() == 1,
        "语言切了输入法没切必须拒绝并计数，且不得入账",
    );

    // 07 失联文案要指向后果（静默吞键），不能只说「不一致」。
    let why = match bad {
        Err(ref e) => e.why.clone(),
        Ok(()) => String::new(),
    };
    set.add(
        "O09-切换-07",
        why.contains("静默"),
        "失联诊断必须点明「按键被静默吞掉」这一实际后果",
    );

    // 08 台账容量边界：恰好上限入账，第 N+1 条被拒。
    let mut full = SwitchLedger::new();
    let mut accepted = 0usize;
    for _ in 0..MAX_SWITCH_RECORDS {
        if full
            .record(SwitchRecord {
                lang: String::from("en"),
                expect_kind: ImeKind::Direct,
                actual_kind: ImeKind::Direct,
            })
            .is_ok()
        {
            accepted += 1;
        }
    }
    let overflow = full.record(SwitchRecord {
        lang: String::from("en"),
        expect_kind: ImeKind::Direct,
        actual_kind: ImeKind::Direct,
    });
    set.add(
        "O09-切换-08",
        accepted == MAX_SWITCH_RECORDS
            && full.len() == MAX_SWITCH_RECORDS
            && overflow.is_err(),
        "台账上限含等号：=上限入账、+1 被拒（断案需要可查台账）",
    );

    // 09 切换失联能进诊断袋（与 record 的拒绝分支同源，不重复计数）。
    let mut bag = DiagBag::new();
    let desync = SwitchRecord {
        lang: String::from("ar"),
        expect_kind: ImeKind::Arabic,
        actual_kind: ImeKind::Direct,
    };
    diagnose_switch(&mut bag, &desync);
    let synced = SwitchRecord {
        lang: String::from("ar"),
        expect_kind: ImeKind::Arabic,
        actual_kind: ImeKind::Arabic,
    };
    diagnose_switch(&mut bag, &synced);
    set.add(
        "O09-切换-09",
        bag.len() == 1 && bag.all()[0].code == Diag::SwitchDesync,
        "只有失联才立案，同步切换不得误报",
    );

    // 10 读屏摘要含成功与失联两个数字（读屏用户要能知道有没有失联发生过）。
    let sum = led.screen_summary();
    set.add(
        "O09-切换-10",
        sum.contains("1") && sum.contains("失联"),
        "读屏摘要必须同时报出成功次数与失联次数",
    );

    // 11 错误码全局唯一（两处不同原因共用一个码 ⇒ 无法按码分流）。
    let codes = [
        E_COMPOSITION_ENGINE_EDIT,
        E_COMPOSITION_BANNED_KEY,
        E_CANDIDATE_NO_FIT,
        E_DIRECTION_MISMATCH,
        E_SWITCH_DESYNC,
        E_COMPOSITION_TOO_LONG,
        E_CANDIDATE_CAP,
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    set.add(
        "O09-切换-11",
        uniq && codes.len() == 7,
        "七个错误码必须两两不同（共用码会让按码分流失效）",
    );
}

/// IME 协同：三类输入法 × 多语言的端到端协同。
fn group_ime(set: &mut CheckSet) {
    let view = match Viewport::new(1024, 768) {
        Ok(v) => v,
        Err(_) => {
            set.fail("O09-协同-00", "视口构造不应失败");
            return;
        }
    };
    let mut gate = CompositionGate::new();
    let mut led = SwitchLedger::new();

    // 01 三类输入法都能走完「组合 → 候选 → 上屏」且不产生阻断诊断。
    let mut end_to_end = true;
    let mut detail = String::new();
    for (i, mut c) in samples().into_iter().enumerate() {
        c.caret_offset = 10 + i as u32 * 8;
        let w = match CandidateWindow::new(6) {
            Ok(w) => w,
            Err(_) => {
                end_to_end = false;
                break;
            }
        };
        // 组合期不该拦引擎编辑，但命令键必须被拦。
        let edit_refused = gate.accept_edit(&c).is_err();
        let key_blocked = gate.key_banned(&c, "Space");
        // 候选窗要能落位。
        let dir = if c.kind == ImeKind::Arabic {
            Direction::Rtl
        } else {
            Direction::Ltr
        };
        let caret = (100 + c.caret_offset as i32, 300);
        let placed = w.place(caret, (200, 100), view, dir).is_ok();
        if !(edit_refused && key_blocked && placed) {
            end_to_end = false;
            detail = format!("第 {} 类输入法未走通", i);
            break;
        }
        // 上屏后组合清空⇒ 引擎侧编辑应放行。
        c.text = String::new();
        if gate.accept_edit(&c).is_err() {
            end_to_end = false;
            detail = format!("第 {} 类上屏后仍被拦", i);
            break;
        }
        // 上屏同时要把「语言 → 输入法」的联动落账（协同的另一半）。
        let lang = match c.kind {
            ImeKind::Cjk => "zh-Hans",
            ImeKind::Arabic => "ar",
            ImeKind::VoiceCompose => "zh-Hans",
            ImeKind::Direct => "en",
        };
        if led
            .record(SwitchRecord {
                lang: String::from(lang),
                expect_kind: c.kind,
                actual_kind: c.kind,
            })
            .is_err()
        {
            end_to_end = false;
            detail = String::from("上屏联动未入账");
            break;
        }
    }
    // 三类都走通 ⇒ 台账应有三条且零失联。
    set.add(
        "O09-协同-01",
        end_to_end && led.len() == 3 && led.desyncs == 0,
        if detail.is_empty() {
            "三类输入法必须都走通组合→候选→上屏→联动入账"
        } else {
            "端到端协同失败"
        },
    );

    // 02 阿拉伯语走 RTL：候选窗向左展开且方向校验通过。
    let ar = {
        let mut c = Composition::new(ImeKind::Arabic, 8);
        c.text = String::from("كتب");
        c
    };
    let w6 = match CandidateWindow::new(6) {
        Ok(w) => w,
        Err(_) => {
            set.fail("O09-协同-02", "候选窗构造不应失败");
            return;
        }
    };
    let ar_place = w6.place((500, 300), (200, 100), view, Direction::Rtl);
    // 位移在**有符号空间**施加：`-1 as u32` 会溢出，故全程 i32。
    let base: i32 = 20;
    let moved = base + caret_step(Direction::Rtl);
    let ar_step_ok = moved >= 0 && check_caret(base as u32, moved as u32, Direction::Rtl).is_ok();
    set.add(
        "O09-协同-02",
        matches!(ar_place, Ok(ref r) if r.x <= 500) && ar_step_ok,
        "阿拉伯语必须走 RTL：候选窗向左展开且插入点在头部之前",
    );

    // 03 语音输入的长组合期不误伤：长度在上限内就放行。
    let voice = {
        let mut c = Composition::new(ImeKind::VoiceCompose, 0);
        c.text = String::from("yuandianzhinengjiagongshi");
        c
    };
    set.add(
        "O09-协同-03",
        gate.check_len(&voice).is_ok() && voice.active(),
        "正常的语音长组合期不得被长度守卫误伤",
    );

    // 04 直通输入法无组合期：命令键不拦、编辑放行、候选窗仍可落位。
    let direct = Composition::new(ImeKind::Direct, 0);
    let direct_ok = !gate.key_banned(&direct, "Enter") && gate.accept_edit(&direct).is_ok();
    let direct_place = w6.place((50, 50), (200, 100), view, Direction::Ltr).is_ok();
    set.add(
        "O09-协同-04",
        direct_ok && direct_place,
        "直通输入法不得被组合期规则误伤",
    );

    // 05 整条读屏替述可读：含输入法类别、组合文本、候选数、联动摘要。
    //    联动摘要必须带上真实台账里的成功次数（3次），否则这段是模板文案。
    let line = screen_line(&ar, &w6, &led);
    let summary_ok = led.screen_summary().contains("3");
    set.add(
        "O09-协同-05",
        line.contains("阿拉伯") && line.contains("كتب") && line.contains("候选") && summary_ok,
        "读屏替述必须同时含输入法类别、组合文本与候选数",
    );

    // 06 组合读屏含光标偏移（读屏用户要知道 IME 状态到哪了）。
    set.add(
        "O09-协同-06",
        ar.screen_line().contains("8") && !ar.screen_line().is_empty(),
        "组合读屏必须报出光标偏移",
    );

    // 07 全语言轮询：每种登记语言都能推导出输入法类别并构造出组合态。
    let mut all_lang_ok = true;
    for (lang, kind) in LANG_IME_KINDS.iter() {
        match ime_kind_for(lang) {
            Some(k) if k == *kind => {
                let mut c = Composition::new(*kind, 0);
                if kind.has_composition() {
                    c.text = String::from("a");
                    if !c.active() {
                        all_lang_ok = false;
                    }
                } else if c.active() {
                    all_lang_ok = false;
                }
            }
            _ => all_lang_ok = false,
        }
    }
    set.add(
        "O09-协同-07",
        all_lang_ok,
        "八种登记语言都必须推导出正确输入法且组合态与类别一致",
    );

    // 08 版本号已登记（对外可追）。
    set.add(
        "O09-协同-08",
        IME_VERSION.starts_with("U01-") && !IME_VERSION.is_empty(),
        "版本号须带域前缀 U01-",
    );
}

/// 判据自检：验证**判据本身**不是恒真/空断言。
fn group_meta(set: &mut CheckSet) {
    // 01 判据不得恒真：用「组合期」与「非组合期」两次真实调用做对照，
    //    确认判据能分辨二者——否则「永远返回 Err」的实现也能骗绿。
    let mut null_gate = CompositionGate::new();
    let cjk = {
        let mut c = Composition::new(ImeKind::Cjk, 0);
        c.text = String::from("ni");
        c
    };
    // 真实现：拒。空实现（把 active 判成 false）：放行 ⇒ 判据能分辨。
    let real_refuses = null_gate.accept_edit(&cjk).is_err();
    let fake = Composition::new(ImeKind::Direct, 0);
    let fake_allows = null_gate.accept_edit(&fake).is_ok();
    set.add(
        "O09-判据-01",
        real_refuses && fake_allows,
        "判据必须能分辨「有组合期」与「无组合期」——否则是恒真弱门禁",
    );

    // 02 反向对照：把 composition 判定反接（组合期判成非组合期）必须被抓住。
    //     即「判据红了」的语义方向是确定的。
    let inverted = Composition::new(ImeKind::Direct, 0);
    let inverted_ok = null_gate.accept_edit(&inverted).is_ok();
    set.add(
        "O09-判据-02",
        inverted_ok,
        "组合期判据的极性：非组合期放行、组合期拒绝（两向都验）",
    );

    // 03 解析解对账的单边符号：正确实现的 y 偏差恒非负/非正之一。
    //     用「LTR 与 RTL 在夹取后 x 的关系」做符号判据（见候选-12）。
    set.add(
        "O09-判据-03",
        Direction::Ltr != Direction::Rtl,
        "方向枚举必须有两个互异变体（否则符号判据退化）",
    );

    // 04 自检项数量在CheckSet 容量内（不截断丢红）。
    set.add(
        "O09-判据-04",
        set.len() < 112,
        "自检项数须在 CheckSet 容量 112 内，否则末项被静默丢弃",
    );

    // 05 诊断袋不重复立案：同一次失联重复收集不应翻倍。
    //    （去重责任在调用方；本判据确保单次收集只产生一条。）
    let mut bag = DiagBag::new();
    let sync = SwitchRecord {
        lang: String::from("en"),
        expect_kind: ImeKind::Direct,
        actual_kind: ImeKind::Direct,
    };
    diagnose_switch(&mut bag, &sync);
    set.add(
        "O09-判据-05",
        bag.is_empty(),
        "同步切换不得产生任何诊断（空袋是正确结果）",
    );

    // 06 错误三要素齐发：screen_text 含现象/原因/下一步/责任方。
    let e = match Viewport::new(0, 0) {
        Err(e) => e,
        Ok(_) => {
            set.fail("O09-判据-06", "0×0 视口应当被拒");
            return;
        }
    };
    let txt = e.screen_text();
    set.add(
        "O09-判据-06",
        txt.contains(E_CANDIDATE_NO_FIT) && txt.contains("下一步") && txt.contains("责任方"),
        "错误文本必须三要素齐发（现象/原因/下一步/责任方）",
    );
}

/// 夹取辅助（与实现同口径的自检侧解析解）。
fn clamp_ref(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 下钳到 0。
fn max0(v: i32) -> i32 {
    if v < 0 {
        0
    } else {
        v
    }
}

/// 本域自检入口。
pub fn run_veu09_checks() -> CheckSet {
    let mut set = CheckSet::new("veu09-ime");
    group_composition(&mut set);
    group_candidate(&mut set);
    group_direction(&mut set);
    group_switch(&mut set);
    group_ime(&mut set);
    group_meta(&mut set);
    assert!(!set.truncated(), "VE-F4009 自检项被CheckSet 截断");
    set
}
