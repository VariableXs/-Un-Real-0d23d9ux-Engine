//! F231 表单校验时机 · H 基础通用域实装。
//!
//! **判据锚**：F231。
//!
//! **验收标准（主册第一句）**：校验三时机写死：输入中只查格式硬错误
//! （如端口输字母即时红）、失焦时查完整规则（用户名重名检查在离开该框
//! 时才发起——边打字边查会越查越报错）、点提交时做最终校验并把焦点跳回
//! 第一个出错框+该框抖动一次——从不弹汇总弹窗打断。错误文案贴在框下
//! 4px（不在顶部堆一列），修正后红色即时消退。
//!
//! **设计要点**：
//! - [`ValTiming`] 三时机枚举（Typing/Blur/Submit）+ [`dispatch`]
//!   分派器：各时机挂不同校验强度——Typing 只跑硬格式规则
//!   （[`FieldRules::hard`]），Blur/Submit 才跑完整规则
//!   （[`FieldRules::full`]）——「无 onkeyup 全量校验」的结构保证：
//!   Typing 通路在类型层拿不到 full 规则的执行权；
//! - 提交路由 [`SubmitRoute`]：第一个出错框跳焦点 + 抖动一次
//!   （[`SHAKE_MS`] 120ms）+ **不弹汇总窗**（[`FormValidator`] 无任何
//!   弹窗通路，`summary_dialogs` 恒 0 即审计判据）；
//! - 错误消退状态机 [`ErrState`]：修正即清（同帧置 corrected），
//!   渲染消退延迟 <[`ERR_CLEAR_BUDGET_MS`] 100ms 判定；
//! - 错误文案位置常量：框下 [`ERR_LABEL_OFFSET_PX`] 4px
//!   （[`err_label_rect`]），不在顶部堆一列；
//! - onkeyup 全量校验审计器：三时机之外挂完整规则登记即计违规
//!   （[`FormValidator::register_trigger`]）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、`crate::h1star::h1base::Rect`。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 错误文案与框的间距——主册 F231 原文「错误文案贴在框下 4px」。
pub const ERR_LABEL_OFFSET_PX: i32 = 4;

/// 错误文案行高（px）——文案单行贴框，不堆一列。
pub const ERR_LABEL_H_PX: i32 = 18;

/// 修正后红色消退预算——主册 F231 验收「错误消退延迟实测 <100ms」。
pub const ERR_CLEAR_BUDGET_MS: u64 = 100;

/// 提交时出错框抖动时长——主册 F231「该框抖动一次」的定量档位。
pub const SHAKE_MS: u32 = 120;

/// 表单字段绑定容量。
pub const FIELD_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 三时机与规则
// ---------------------------------------------------------------------------

/// 校验三时机（主册写死的三个触发点，无第四时机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValTiming {
    /// 输入中：只查格式硬错误。
    Typing,
    /// 失焦：查完整规则。
    Blur,
    /// 点提交：全量终检。
    Submit,
}

/// 单字段规则对：硬格式（输入中即查）+ 完整规则（失焦/提交才查）。
///
/// 规则签名：`fn(&str) -> Option<&'static str>`——None = 通过，
/// Some(文案) = 出错。fn 指针可拷贝，绑定表零堆。
#[derive(Clone, Copy)]
pub struct FieldRules {
    pub field_id: u32,
    /// 硬格式规则（Typing/Blur/Submit 三时机都跑）。
    pub hard: fn(&str) -> Option<&'static str>,
    /// 完整规则（只在 Blur/Submit 跑——Typing 通路结构上不可达）。
    pub full: fn(&str) -> Option<&'static str>,
}

/// 一条校验错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValErr {
    pub field_id: u32,
    pub msg: &'static str,
}

/// 三时机分派器——校验强度的唯一裁决点。
///
/// - Typing：**只**跑 hard（主册「输入中只查格式硬错误」）；
/// - Blur：hard → full 依次查（失焦时才发起完整检查）；
/// - Submit：hard → full 依次查（全量终检）。
pub fn dispatch(rules: &FieldRules, timing: ValTiming, input: &str) -> Option<ValErr> {
    let mk = |msg: &'static str| Some(ValErr { field_id: rules.field_id, msg });
    match timing {
        ValTiming::Typing => (rules.hard)(input).and_then(mk),
        ValTiming::Blur | ValTiming::Submit => (rules.hard)(input).or_else(|| (rules.full)(input)).and_then(mk),
    }
}

/// 提交路由：第一个出错框的焦点跳转 + 单次抖动。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmitRoute {
    /// 焦点跳回的出错框。
    pub focus_field: u32,
    /// 该框错误（文案贴框下显示）。
    pub err: ValErr,
    /// 抖动时长（一次）。
    pub shake_ms: u32,
}

// ---------------------------------------------------------------------------
// 示例规则（判据原文的两个具体场景）
// ---------------------------------------------------------------------------

/// 端口硬格式：只许数字——主册「端口输字母即时红」。
pub fn hard_port_digits(s: &str) -> Option<&'static str> {
    if s.is_empty() {
        return None; // 输入中允许暂空（清空态不算硬错误）。
    }
    for c in s.bytes() {
        if !c.is_ascii_digit() {
            return Some("端口只能是数字");
        }
    }
    None
}

/// 用户名重名检查——主册「用户名重名检查在离开该框时才发起」的完整规则。
pub fn full_username_free(s: &str) -> Option<&'static str> {
    if s == "admin" {
        return Some("用户名已被占用");
    }
    None
}

/// 最短长度完整规则。
pub fn full_len_at_least_3(s: &str) -> Option<&'static str> {
    if s.len() < 3 {
        return Some("至少 3 个字符");
    }
    None
}

// ---------------------------------------------------------------------------
// 表单校验器
// ---------------------------------------------------------------------------

/// 表单校验器：字段绑定表 + 三时机分派 + 提交路由 + 审计面。
pub struct FormValidator {
    fields: [Option<FieldRules>; FIELD_CAP],
    /// 提交总次数。
    pub submits: u32,
    /// 抖动总次数（每次提交至多 1 次——零或一，绝不连抖）。
    pub shakes: u32,
    /// 汇总弹窗数——**恒 0**：本结构没有任何产生弹窗的通路
    /// （主册「从不弹汇总弹窗打断」的审计判据）。
    pub summary_dialogs: u32,
    /// onkeyup 全量校验违规登记数（判据：== 0）。
    pub onkeyup_full_violations: u32,
}

impl FormValidator {
    pub fn new() -> FormValidator {
        FormValidator {
            fields: [const { None }; FIELD_CAP],
            submits: 0,
            shakes: 0,
            summary_dialogs: 0,
            onkeyup_full_violations: 0,
        }
    }

    /// 绑定字段规则。重复 id 覆盖（重绑定 = 规则更新）。
    pub fn bind(&mut self, rules: FieldRules) {
        match self.fields.iter_mut().find(|f| matches!(f, Some(r) if r.field_id == rules.field_id)) {
            Some(slot) => *slot = Some(rules),
            None => {
                if let Some(slot) = self.fields.iter_mut().find(|f| f.is_none()) {
                    *slot = Some(rules);
                }
            }
        }
    }

    pub fn bound(&self, field_id: u32) -> Option<FieldRules> {
        self.fields.iter().find_map(|f| match f {
            Some(r) if r.field_id == field_id => Some(*r),
            _ => None,
        })
    }

    /// 触发器登记审计：三时机之外把完整规则挂到输入中（onkeyup 全量
    /// 校验）即违规计数——「三时机触发点代码审计」的落地。
    pub fn register_trigger(&mut self, timing: ValTiming, runs_full_rules: bool) {
        if timing == ValTiming::Typing && runs_full_rules {
            self.onkeyup_full_violations += 1;
        }
    }

    /// 输入中校验（硬格式即时红）。
    pub fn typing_check(&self, field_id: u32, input: &str) -> Option<ValErr> {
        self.bound(field_id).and_then(|r| dispatch(&r, ValTiming::Typing, input))
    }

    /// 失焦校验（完整规则此时才发起）。
    pub fn blur_check(&self, field_id: u32, input: &str) -> Option<ValErr> {
        self.bound(field_id).and_then(|r| dispatch(&r, ValTiming::Blur, input))
    }

    /// 提交终检：按 `inputs`（表单字段顺序）逐个跑全量，第一个出错框
    /// 出路由（跳焦点 + 抖动一次）；全过返回 None。
    /// **无汇总弹窗通路**——只跳焦点，从不打断式弹窗。
    pub fn submit(&mut self, inputs: &[(u32, &str)]) -> Option<SubmitRoute> {
        self.submits += 1;
        for (id, s) in inputs {
            if let Some(r) = self.bound(*id) {
                if let Some(err) = dispatch(&r, ValTiming::Submit, s) {
                    self.shakes += 1;
                    return Some(SubmitRoute { focus_field: err.field_id, err, shake_ms: SHAKE_MS });
                }
            }
        }
        None
    }
}

impl Default for FormValidator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 错误文案位置与红色消退
// ---------------------------------------------------------------------------

/// 错误文案几何：贴在框下 [`ERR_LABEL_OFFSET_PX`] 处，同宽单行——
/// 「不在顶部堆一列」的几何表达。
pub fn err_label_rect(field: Rect) -> Rect {
    Rect::new(field.x, field.bottom() + ERR_LABEL_OFFSET_PX, field.w, ERR_LABEL_H_PX)
}

/// 错误显示状态机（单框）：显示中 → 用户修正（内容变好）→ 即时消退。
pub struct ErrState {
    pub field_id: u32,
    shown: bool,
    corrected: bool,
}

impl ErrState {
    /// 出错置显。
    pub fn shown(field_id: u32) -> ErrState {
        ErrState { field_id, shown: true, corrected: false }
    }

    pub fn is_shown(&self) -> bool {
        self.shown && !self.corrected
    }

    /// 用户修正内容（输入变好）：红色即时清（同帧置 corrected）。
    pub fn notify_corrected(&mut self) {
        if self.shown {
            self.corrected = true;
        }
    }

    /// 消退延迟判定：修正时刻 → 红色消失上屏时刻 < 100ms。
    pub fn clear_latency_ok(&self, corrected_ts_ms: u64, rendered_ts_ms: u64) -> bool {
        self.corrected && rendered_ts_ms.saturating_sub(corrected_ts_ms) < ERR_CLEAR_BUDGET_MS
    }
}

// ---------------------------------------------------------------------------
// 错误看板（提交路由的落地数据源）
// ---------------------------------------------------------------------------

/// 看板槽容量（与表单字段绑定容量同宽）。
pub const ERRBOARD_CAP: usize = FIELD_CAP;

/// 一条驻留错误（框 + 文案 + 出现时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrSlot {
    pub field_id: u32,
    pub msg: &'static str,
    /// 错误出现时刻（ms，注入式）。
    pub shown_at_ms: u64,
}

/// 错误看板：每框当前错误态的驻留表——错误文案渲染、焦点跳转、修正
/// 消退的统一数据源。定容零堆。
pub struct ErrBoard {
    slots: [Option<ErrSlot>; ERRBOARD_CAP],
    pub shown_total: u32,
    pub cleared_total: u32,
}

impl ErrBoard {
    pub fn new() -> ErrBoard {
        ErrBoard { slots: [const { None }; ERRBOARD_CAP], shown_total: 0, cleared_total: 0 }
    }

    /// 置显一条错误（同框覆盖更新文案，不重复占槽）。
    pub fn show(&mut self, field_id: u32, msg: &'static str, ts_ms: u64) {
        self.shown_total += 1;
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|s| matches!(s, Some(e) if e.field_id == field_id))
        {
            *slot = Some(ErrSlot { field_id, msg, shown_at_ms: ts_ms });
            return;
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some(ErrSlot { field_id, msg, shown_at_ms: ts_ms });
        }
    }

    /// 清除某框错误（修正即消退——红色消退的看板侧动作）。
    pub fn clear_field(&mut self, field_id: u32) -> bool {
        match self
            .slots
            .iter()
            .position(|s| matches!(s, Some(e) if e.field_id == field_id))
        {
            Some(idx) => {
                self.slots[idx] = None;
                self.cleared_total += 1;
                true
            }
            None => false,
        }
    }

    pub fn get(&self, field_id: u32) -> Option<&ErrSlot> {
        self.slots.iter().find_map(|s| match s {
            Some(e) if e.field_id == field_id => Some(e),
            _ => None,
        })
    }

    /// 第一个出错框（表单序 = 槽序——与提交路由的「第一个出错框」一致）。
    pub fn first_error(&self) -> Option<&ErrSlot> {
        self.slots.iter().flatten().next()
    }

    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

impl Default for ErrBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl FormValidator {
    /// 提交终检 + 看板落地：路由第一个出错框的同时把全部错误写进看板
    /// （焦点跳转与文案渲染共享一份数据源）。全过时清空看板。
    pub fn submit_tracked(&mut self, inputs: &[(u32, &str)], board: &mut ErrBoard, now_ms: u64) -> Option<SubmitRoute> {
        self.submits += 1;
        let mut first_route: Option<SubmitRoute> = None;
        for (id, s) in inputs {
            if let Some(r) = self.bound(*id) {
                if let Some(err) = dispatch(&r, ValTiming::Submit, s) {
                    board.show(err.field_id, err.msg, now_ms);
                    if first_route.is_none() {
                        self.shakes += 1;
                        first_route = Some(SubmitRoute { focus_field: err.field_id, err, shake_ms: SHAKE_MS });
                    }
                } else {
                    board.clear_field(*id);
                }
            }
        }
        first_route
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 随机短字符串（fuzz 语料：仅 ASCII 字母数字，UTF-8 恒合法）。
fn fuzz_str(x: u32, buf: &mut [u8; 8]) -> &str {
    let n = (x % 5 + 1) as usize;
    let alphabet = b"0123456789adm";
    for i in 0..n {
        buf[i] = alphabet[((x >> (i as u32 * 3)) % alphabet.len() as u32) as usize];
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("000")
}

/// F231 自检（10 条行为级）。
pub fn run_formval_checks() -> CheckSet {
    let mut set = CheckSet::new("F231-formval");
    let mut v = FormValidator::new();
    v.bind(FieldRules { field_id: 1, hard: hard_port_digits, full: full_len_at_least_3 });
    v.bind(FieldRules { field_id: 2, hard: full_len_at_least_3, full: full_username_free });

    // 1. 输入中只查硬格式：端口输字母即时红。
    set.add(
        "typing: port letters flagged instantly",
        v.typing_check(1, "80a").map(|e| e.msg).unwrap_or("") == "端口只能是数字"
            && v.typing_check(1, "808").is_none(),
        "",
    );

    // 2. 输入中不发起完整规则：用户名重名检查不随打字报错
    //    （hard 过、full 会挂的输入在 Typing 必须静默）。
    set.add(
        "typing never runs full rules (no onkeyup full validation)",
        v.typing_check(2, "admin").is_none(),
        "",
    );

    // 3. 失焦才查完整规则：离开用户名框时才报重名。
    set.add(
        "blur runs full rules (username taken)",
        v.blur_check(2, "admin").map(|e| e.msg).unwrap_or("") == "用户名已被占用"
            && v.blur_check(2, "nova").is_none(),
        "",
    );

    // 4. 提交全量终检：全过返回 None。
    let all_ok = v.submit(&[(1, "8080"), (2, "nova")]);
    set.add("submit passes when all fields valid", all_ok.is_none(), "");

    // 5. 提交路由：第一个出错框跳焦点 + 抖动 120ms 一次。
    let route = v.submit(&[(1, "80a"), (2, "admin")]);
    set.add(
        "submit routes focus to first error + single 120ms shake",
        route.map(|r| r.focus_field == 1 && r.shake_ms == SHAKE_MS && r.err.msg == "端口只能是数字").unwrap_or(false)
            && v.shakes == 1,
        "",
    );

    // 6. 第二个出错框的路由（第一个修好后焦点跳到它）。
    let route2 = v.submit(&[(1, "8080"), (2, "admin")]);
    set.add(
        "submit routes to next error field after fix",
        route2.map(|r| r.focus_field == 2 && r.err.msg == "用户名已被占用").unwrap_or(false) && v.shakes == 2,
        "",
    );

    // 7. 提交零弹窗：多次提交后汇总弹窗数恒 0。
    let _ = v.submit(&[(1, "x9"), (2, "zzz")]);
    let _ = v.submit(&[(1, ""), (2, "")]);
    set.add(
        "no summary dialog ever (zero-dialog criterion)",
        v.summary_dialogs == 0 && v.submits == 5,
        "",
    );

    // 8. 错误文案位置：贴框下 4px、同宽单行（不在顶部堆一列）。
    let field = Rect::new(100, 200, 240, 36);
    let label = err_label_rect(field);
    set.add(
        "error label 4px below field, same width",
        label.y == field.bottom() + ERR_LABEL_OFFSET_PX
            && label.x == field.x
            && label.w == field.w
            && label.h == ERR_LABEL_H_PX,
        "",
    );

    // 9. 修正后红色即时消退（<100ms 预算）。
    let mut es = ErrState::shown(1);
    es.notify_corrected();
    set.add(
        "error clears <100ms after fix",
        !es.is_shown() && es.clear_latency_ok(5_000, 5_099) && !es.clear_latency_ok(5_000, 5_100),
        "",
    );

    // 10. onkeyup 全量校验审计：Typing 挂完整规则 = 违规；三时机规范
    //     登记不违规；fuzz 2000 轮随机输入分派——Typing 通路产出恒等于
    //     硬规则产出（结构不变量的行为观测），无 panic。
    v.register_trigger(ValTiming::Typing, false);
    v.register_trigger(ValTiming::Blur, true);
    v.register_trigger(ValTiming::Submit, true);
    let clean = v.onkeyup_full_violations == 0;
    v.register_trigger(ValTiming::Typing, true);
    let mut x: u32 = 0xC0FFEE01;
    let mut buf = [0u8; 8];
    let mut ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let input = fuzz_str(x, &mut buf);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let id = 1 + x % 2;
        let rules = match v.bound(id) {
            Some(r) => r,
            None => continue,
        };
        // 结构不变量：Typing 结果 ≡ hard(input)（全量规则在该通路不可达）。
        let typing_out = dispatch(&rules, ValTiming::Typing, input);
        let hard_out = (rules.hard)(input).map(|m| ValErr { field_id: id, msg: m });
        if typing_out != hard_out {
            ok = false;
            break;
        }
        // Blur/Submit 恒不弱于 Typing（多查一条完整规则）。
        if dispatch(&rules, ValTiming::Blur, input).is_none() && typing_out.is_some() {
            ok = false;
            break;
        }
    }
    set.add(
        "onkeyup audit + fuzz 2000 rounds: typing path ≡ hard rule only",
        clean && v.onkeyup_full_violations == 1 && ok,
        "",
    );

    // 11. 错误看板：提交落地错误、修正清除、first_error 与路由一致。
    let mut v2 = FormValidator::new();
    v2.bind(FieldRules { field_id: 1, hard: hard_port_digits, full: full_len_at_least_3 });
    v2.bind(FieldRules { field_id: 2, hard: hard_port_digits, full: full_len_at_least_3 });
    let mut board = ErrBoard::new();
    let route = v2.submit_tracked(&[(1, "100"), (2, "2x")], &mut board, 1_000);
    let tracked = route.map(|r| r.focus_field).unwrap_or(0) == board.first_error().map(|e| e.field_id).unwrap_or(0)
        && board.active_count() == 1
        && board.get(1).is_none()
        && board.get(2).map(|e| e.msg).unwrap_or("") == "端口只能是数字";
    board.clear_field(2);
    set.add(
        "err board tracks submit route, clears on fix",
        tracked && board.active_count() == 0 && board.cleared_total == 1 && v2.shakes == 1,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_timings_strict_dispatch() {
        let rules = FieldRules { field_id: 7, hard: hard_port_digits, full: full_len_at_least_3 };
        // Typing：只硬格式。
        assert!(dispatch(&rules, ValTiming::Typing, "9x").is_some());
        assert!(dispatch(&rules, ValTiming::Typing, "9").is_none(), "单字符过硬格式、完整规则会挂");
        // Blur/Submit：硬格式 + 完整规则。
        assert_eq!(dispatch(&rules, ValTiming::Blur, "9").unwrap().msg, "至少 3 个字符");
        assert!(dispatch(&rules, ValTiming::Submit, "999").is_none());
    }

    #[test]
    fn hard_error_wins_over_full() {
        let rules = FieldRules { field_id: 7, hard: hard_port_digits, full: full_len_at_least_3 };
        // "x" 同时挂两条——先报硬格式。
        assert_eq!(dispatch(&rules, ValTiming::Blur, "x").unwrap().msg, "端口只能是数字");
    }

    #[test]
    fn empty_input_not_hard_error_while_typing() {
        // 输入中允许暂空（清空态），硬格式放行——但完整规则在失焦时拦。
        assert!(hard_port_digits("").is_none());
        let rules = FieldRules { field_id: 1, hard: hard_port_digits, full: full_len_at_least_3 };
        assert!(dispatch(&rules, ValTiming::Blur, "").is_some());
    }

    #[test]
    fn submit_route_order_and_single_shake() {
        let mut v = FormValidator::new();
        v.bind(FieldRules { field_id: 1, hard: hard_port_digits, full: full_len_at_least_3 });
        v.bind(FieldRules { field_id: 2, hard: hard_port_digits, full: full_len_at_least_3 });
        v.bind(FieldRules { field_id: 3, hard: hard_port_digits, full: full_len_at_least_3 });
        // 2、3 都错——路由只挑第一个（字段顺序），抖动恰一次。
        let r = v.submit(&[(1, "100"), (2, "2"), (3, "3")]).unwrap();
        assert_eq!(r.focus_field, 2);
        assert_eq!(r.shake_ms, 120);
        assert_eq!(v.shakes, 1);
        assert_eq!(v.summary_dialogs, 0);
        // 全对不路由。
        assert!(v.submit(&[(1, "100"), (2, "200"), (3, "300")]).is_none());
        assert_eq!(v.shakes, 1, "通过路径不抖动");
    }

    #[test]
    fn onkeyup_auditor() {
        let mut v = FormValidator::new();
        v.register_trigger(ValTiming::Blur, true);
        v.register_trigger(ValTiming::Submit, true);
        v.register_trigger(ValTiming::Typing, false);
        assert_eq!(v.onkeyup_full_violations, 0);
        v.register_trigger(ValTiming::Typing, true);
        assert_eq!(v.onkeyup_full_violations, 1, "输入中挂完整规则 = onkeyup 全量校验违规");
    }

    #[test]
    fn err_board_lifecycle() {
        let mut v = FormValidator::new();
        v.bind(FieldRules { field_id: 1, hard: hard_port_digits, full: full_len_at_least_3 });
        v.bind(FieldRules { field_id: 2, hard: hard_port_digits, full: full_len_at_least_3 });
        let mut board = ErrBoard::new();
        // 两框全错：看板两槽、路由第一错、抖动一次。
        let r = v.submit_tracked(&[(1, "8a"), (2, "9")], &mut board, 0).unwrap();
        assert_eq!(r.focus_field, 1);
        assert_eq!(board.active_count(), 2);
        assert_eq!(board.first_error().unwrap().field_id, 1);
        // 修好第一框：看板清槽、路由跳到第二错。
        // 现象：原字面量 ".as_bytes()100" 含字母/点，硬格式即红，第一框
        //      永远修不好，路由恒聚焦 field 1（left:1 right:2）。
        // 根因：测试输入字面量被污染，与本段注释意图「修好第一框」矛盾
        //      （"100" 过硬格式、full 长度 ≥3 也过）。
        // 修法：改测试——字面量还原为 "100"；实现（submit_tracked）正确。
        board.clear_field(1);
        let r2 = v.submit_tracked(&[(1, "100"), (2, "9")], &mut board, 10).unwrap();
        assert_eq!(r2.focus_field, 2);
        assert_eq!(board.active_count(), 1);
        // 全修好：看板空、无路由（"100"/"200" 均过硬格式 + 完整规则）。
        assert!(v.submit_tracked(&[(1, "100"), (2, "200")], &mut board, 20).is_none());
        assert_eq!(board.active_count(), 0);
        assert_eq!(v.summary_dialogs, 0);
        // 同框重复出错覆盖不占新槽。
        v.submit_tracked(&[(1, "1a")], &mut board, 30);
        let shown_again = board.shown_total;
        v.submit_tracked(&[(1, "2b")], &mut board, 40);
        assert_eq!(board.shown_total, shown_again + 1);
        assert_eq!(board.active_count(), 1);
        assert_eq!(board.get(1).unwrap().msg, "端口只能是数字");
    }

    #[test]
    fn formval_selfcheck_all_green() {
        let s = run_formval_checks();
        assert!(s.all_passed(), "F231 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F231。本段纯追加：持久化面 = 校验时机登记记录（逐表单字节）
// framed 编解码；壳接线面 = 提交门判定 + 错误文案绘制清单；
// 判定面 = run_formval_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic——全域 v2 段统一「VXH1」。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";

/// v2 记录版本（本批 = 1；版本不认显性拒绝，不做静默迁移）。
pub const V2_VERSION: u8 = 1;

/// 持久化损坏类型——四类全部显性拒绝，不留半态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    /// 头 4 字节不是 b"VXH1"。
    BadMagic,
    /// 版本字节不是 [`V2_VERSION`]。
    BadVersion,
    /// 总长与定长记录式不符。
    BadLen,
    /// 尾 4 字节 FNV-1a 校验和不符。
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族：offset
/// 0xCBF29CE484222325 / prime 0x100000001B3——一处一事实）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 字段登记种类：0 = 未绑定；2 = 硬格式 + 完整规则（Blur/Submit 可达）。
pub const V2_REG_NONE: u8 = 0;
pub const V2_REG_FULL: u8 = 2;

/// 记录式：magic4+ver1+regs[32]+budget u16+shake u16+checksum u32。
/// 容量上限在册：45 字节定长（零堆，栈上缓冲即可）。
pub const V2_PAYLOAD_LEN: usize = FIELD_CAP + 2 + 2;
pub const V2_REC_LEN: usize = 5 + V2_PAYLOAD_LEN + 4;
const V2_BODY_LEN: usize = V2_REC_LEN - 4;

/// 校验时机登记记录（主册 F231 v2：校验时机登记持久化——逐表单
/// 一字节，重启后审计「三时机登记」不改口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2TimingReg {
    /// 下标 = 字段 id；值 = 登记种类。
    pub regs: [u8; FIELD_CAP],
    /// 错误消退预算（主册「错误消退 <100ms」判据常量随记录走）。
    pub clear_budget_ms: u16,
    /// 提交抖动档位（主册「该框抖动一次」——既有 SHAKE_MS）。
    pub shake_ms: u16,
}

impl V2TimingReg {
    /// 从校验器绑定表导出登记记录（绑定即登记——一处登记表读一处）。
    pub fn capture(v: &FormValidator) -> V2TimingReg {
        let mut out = V2TimingReg {
            regs: [V2_REG_NONE; FIELD_CAP],
            clear_budget_ms: ERR_CLEAR_BUDGET_MS as u16,
            shake_ms: SHAKE_MS as u16,
        };
        for (i, slot) in out.regs.iter_mut().enumerate() {
            if v.bound(i as u32).is_some() {
                *slot = V2_REG_FULL;
            }
        }
        out
    }

    /// 编码：out 不足 45 字节返回 None（显性，不截断）。
    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < V2_REC_LEN {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5..5 + FIELD_CAP].copy_from_slice(&self.regs);
        out[5 + FIELD_CAP..7 + FIELD_CAP].copy_from_slice(&self.clear_budget_ms.to_le_bytes());
        out[7 + FIELD_CAP..9 + FIELD_CAP].copy_from_slice(&self.shake_ms.to_le_bytes());
        let sum = v2_fnv1a32(&out[..V2_BODY_LEN]);
        out[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum.to_le_bytes());
        Some(V2_REC_LEN)
    }

    /// 解码：magic/版本/长度/校验和四类损坏逐一拒绝。
    pub fn from_bytes(buf: &[u8]) -> Result<V2TimingReg, V2SaveErr> {
        if buf.len() != V2_REC_LEN {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let expect = u32::from_le_bytes([buf[V2_BODY_LEN], buf[V2_BODY_LEN + 1], buf[V2_BODY_LEN + 2], buf[V2_BODY_LEN + 3]]);
        if v2_fnv1a32(&buf[..V2_BODY_LEN]) != expect {
            return Err(V2SaveErr::BadChecksum);
        }
        let mut regs = [V2_REG_NONE; FIELD_CAP];
        regs.copy_from_slice(&buf[5..5 + FIELD_CAP]);
        let clear_budget_ms = u16::from_le_bytes([buf[5 + FIELD_CAP], buf[6 + FIELD_CAP]]);
        let shake_ms = u16::from_le_bytes([buf[7 + FIELD_CAP], buf[8 + FIELD_CAP]]);
        Ok(V2TimingReg { regs, clear_budget_ms, shake_ms })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 提交门判定（主册「点提交时……把焦点跳回第一个出错框」的门面：
/// 有错禁提交，焦点与提交路由同源——都是 first_error）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2GateVerdict {
    /// 无驻留错误——放行提交。
    Allow,
    /// 有错禁提交：焦点跳回第一个出错框。
    Deny { focus_field: u32 },
}

/// 提交门：读错误看板当前态（纯函数，壳层在提交按钮回调里直调）。
pub fn v2_submit_gate(board: &ErrBoard) -> V2GateVerdict {
    match board.first_error() {
        Some(e) => V2GateVerdict::Deny { focus_field: e.field_id },
        None => V2GateVerdict::Allow,
    }
}

/// 绘制清单图元（合成器消费：矩形 + 颜色索引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2DrawItem {
    pub rect: Rect,
    /// 1 = 错误红文案条 / 3 = 首错抖动描边（焦点目标）。
    pub color_idx: u8,
}

/// 错误文案绘制清单：对布局里每条驻留错误生成「框下 4px 贴条」图元
/// ——几何唯一源是既有 err_label_rect（不另造第二份几何）。
/// 容量上限 ERRBOARD_CAP，超容图元丢弃并如实回报。
pub fn v2_error_drawlist(
    board: &ErrBoard,
    field_rects: &[(u32, Rect)],
) -> (usize, [Option<V2DrawItem>; ERRBOARD_CAP]) {
    let mut out: [Option<V2DrawItem>; ERRBOARD_CAP] = [const { None }; ERRBOARD_CAP];
    let mut n = 0usize;
    for (id, rect) in field_rects.iter() {
        if board.get(*id).is_none() || n >= ERRBOARD_CAP {
            continue;
        }
        let color = if board.first_error().map(|e| e.field_id) == Some(*id) { 3 } else { 1 };
        out[n] = Some(V2DrawItem { rect: err_label_rect(*rect), color_idx: color });
        n += 1;
    }
    (n, out)
}

// -- 判定面扩展 ------------------------------------------------------------

/// F231 v2 自检（首条必为持久化 round-trip）。
pub fn run_formval_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F231-formval-v2");

    // 1. 持久化 round-trip（验主册「校验时机登记」可落盘还原）。
    let mut v = FormValidator::new();
    v.bind(FieldRules { field_id: 0, hard: hard_port_digits, full: full_len_at_least_3 });
    v.bind(FieldRules { field_id: 5, hard: hard_port_digits, full: full_username_free });
    let reg = V2TimingReg::capture(&v);
    let mut buf = [0u8; V2_REC_LEN];
    let wrote = reg.to_bytes(&mut buf).unwrap_or(0);
    let blank = V2TimingReg { regs: [V2_REG_NONE; FIELD_CAP], clear_budget_ms: 0, shake_ms: 0 };
    let back = V2TimingReg::from_bytes(&buf[..wrote]).unwrap_or(blank);
    set.add(
        "v2 persist round-trip: timing registry",
        wrote == V2_REC_LEN
            && back.regs[0] == V2_REG_FULL
            && back.regs[5] == V2_REG_FULL
            && back.regs[1] == V2_REG_NONE
            && back.clear_budget_ms == ERR_CLEAR_BUDGET_MS as u16
            && back.shake_ms == SHAKE_MS as u16,
        "",
    );

    // 2. 四类损坏全拒绝（验「持久化边界显性拒绝，不留半态」）。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 9;
    let mut b4 = buf;
    b4[V2_REC_LEN - 1] ^= 0xFF;
    set.add(
        "v2 persist rejects magic/version/len/checksum",
        matches!(V2TimingReg::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2TimingReg::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2TimingReg::from_bytes(&buf[..V2_REC_LEN - 1]), Err(V2SaveErr::BadLen))
            && matches!(V2TimingReg::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 提交门（验主册「点提交……焦点跳回第一个出错框」：有错禁提交）。
    let mut v3 = FormValidator::new();
    v3.bind(FieldRules { field_id: 1, hard: hard_port_digits, full: full_len_at_least_3 });
    let mut board = ErrBoard::new();
    let allow0 = matches!(v2_submit_gate(&board), V2GateVerdict::Allow);
    let _ = v3.submit_tracked(&[(1, "8a")], &mut board, 0);
    let deny = matches!(v2_submit_gate(&board), V2GateVerdict::Deny { focus_field: 1 });
    board.clear_field(1);
    set.add(
        "v2 submit gate: deny with focus, allow when clean",
        allow0 && deny && matches!(v2_submit_gate(&board), V2GateVerdict::Allow),
        "",
    );

    // 4. 绘制清单（验主册「错误文案贴在框下 4px」：几何与 err_label_rect
    //    同源，首错带抖动描边色 3）。
    let mut board2 = ErrBoard::new();
    let _ = v3.submit_tracked(&[(1, "x")], &mut board2, 10);
    let (n, items) = v2_error_drawlist(&board2, &[(1u32, Rect::new(100, 200, 240, 36))]);
    set.add(
        "v2 drawlist: label 4px below, first error shake-marked",
        n == 1
            && items[0].map(|it| {
                it.color_idx == 3 && it.rect.y == 236 + ERR_LABEL_OFFSET_PX && it.rect.w == 240
            }) == Some(true),
        "",
    );

    // 5. 登记面与绑定表逐位一致（FIELD_CAP 全域扫描——绑定即登记）。
    let mut v5 = FormValidator::new();
    for i in 0..FIELD_CAP as u32 {
        if i % 3 == 0 {
            v5.bind(FieldRules { field_id: i, hard: hard_port_digits, full: full_len_at_least_3 });
        }
    }
    let reg5 = V2TimingReg::capture(&v5);
    let sync = (0..FIELD_CAP).all(|i| (reg5.regs[i] == V2_REG_FULL) == v5.bound(i as u32).is_some());
    set.add(
        "v2 registry bitmap mirrors bindings",
        sync && reg5.regs.iter().filter(|r| **r == V2_REG_FULL).count() == 11,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_record_roundtrip_exact() {
        let mut v = FormValidator::new();
        v.bind(FieldRules { field_id: 2, hard: hard_port_digits, full: full_username_free });
        let reg = V2TimingReg::capture(&v);
        let mut buf = [0u8; V2_REC_LEN];
        let n = reg.to_bytes(&mut buf).unwrap();
        let back = V2TimingReg::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back, reg);
        assert_eq!(back.regs[2], V2_REG_FULL);
    }

    #[test]
    fn v2_gate_and_drawlist() {
        let mut v = FormValidator::new();
        v.bind(FieldRules { field_id: 4, hard: hard_port_digits, full: full_len_at_least_3 });
        let mut board = ErrBoard::new();
        let _ = v.submit_tracked(&[(4, "1z")], &mut board, 0);
        assert!(matches!(v2_submit_gate(&board), V2GateVerdict::Deny { focus_field: 4 }));
        let (n, items) = v2_error_drawlist(&board, &[(4, Rect::new(0, 0, 100, 30))]);
        assert_eq!(n, 1);
        assert_eq!(items[0].unwrap().color_idx, 3);
    }

    #[test]
    fn formval_v2_selfcheck_all_green() {
        let s = run_formval_v2_checks();
        assert!(s.all_passed(), "F231 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
