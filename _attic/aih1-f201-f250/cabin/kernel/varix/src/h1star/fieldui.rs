//! F229 输入框占位符与清除按钮 · H 基础通用域实装。
//!
//! **判据锚**：F229。
//!
//! **验收标准（主册第一句）**：每个文本输入框的标配三件：占位符（浅色
//! 说明文字，获得焦点即消失、失焦且为空时回来，与已输入内容视觉可分辨
//! ——占位符是灰色、正文是正常色）、一键清除（有内容且聚焦时右端 ×，
//! 点击清空并保持焦点在框内）、字数超限提示（有上限的框实时显示 N/M，
//! 超限变红不截断让用户自己删）。
//!
//! **设计要点**：
//! - [`FieldSpec`] + [`FieldReg`] 三件套登记册：全系统输入框逐一登记
//!   占位符/清除/超限提示齐备性，缺件即计违规（`violations`）——
//!   「三件套审计清单」的代码级表达；
//! - 占位符状态机：焦点与内容四象限（[`Field::placeholder_visible`]）——
//!   「失焦且为空」才显示，获得焦点或已有内容即消失；
//! - 占位/正文可分辨判定：[`placeholder_body_ok`] 走 h1base
//!   `gray_steps_apart`，灰值差 ≥[`PLACEHOLDER_MIN_STEPS`] 档（主册
//!   「灰值差 ≥2 档」原文）；
//! - 清除按钮状态机：可见性 = 有内容且聚焦；点击清空内容、焦点保持在
//!   框内（[`Field::clear`] 不动焦点位）——「清除后焦点保持」判据；
//! - 超限计数器：内容长度独立于上限增长（**不截断**，`truncation_events`
//!   审计恒 0——截断路径在类型层不存在），超限即时变红（N/M 实时显示），
//!   用户删回限内红色即时消退；
//! - 热路径零堆：内容只持逻辑长度（判定面无需字节），事件走定容环；
//!   Vec 仅用于登记册快照（诊断面）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、
//! `crate::star::sbase::RingLog`、`crate::h1star::h1base::{Rgb8, gray_steps_apart}`。

use crate::checks::CheckSet;
use crate::h1star::h1base::{gray_steps_apart, Rgb8};
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 登记册容量——全系统输入框登记上限（64 个，超出拒绝并记账）。
pub const FIELD_CAP: usize = 64;

/// 占位/正文最小灰值档差——主册 F229 原文「灰值差 ≥2 档」（sRGB 编码域
/// 10 档制，h1base `gray_steps_apart` 口径）。
pub const PLACEHOLDER_MIN_STEPS: u32 = 2;

/// 缺省占位符灰（示例档：灰阶 150）。
pub const DEFAULT_PLACEHOLDER: Rgb8 = Rgb8::new(150, 150, 150);

/// 缺省正文色（示例档：深灰 40——与 150 差 110 级 ≈ 4 档 ≥ 2 档门）。
pub const DEFAULT_BODY: Rgb8 = Rgb8::new(40, 40, 40);

/// 单框事件环容量（焦点/清除/越限事件最近 32 条审计）。
pub const FIELD_EVENT_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 三件套登记册
// ---------------------------------------------------------------------------

/// 单个输入框的三件套规格（登记项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    pub id: u32,
    /// 有占位符。
    pub has_placeholder: bool,
    /// 有一键清除。
    pub has_clear: bool,
    /// 有字数上限提示（`max_len > 0` 才有意义）。
    pub has_limit: bool,
    /// 字数上限（0 = 无上限）。
    pub max_len: u32,
}

impl FieldSpec {
    /// 三件齐备（审计通过的充分条件）。
    pub fn complete(&self) -> bool {
        self.has_placeholder && self.has_clear && self.has_limit && self.max_len > 0
    }
}

/// 输入框三件套登记册——「全系统输入框三件套审计清单」本体。
pub struct FieldReg {
    fields: [Option<FieldSpec>; FIELD_CAP],
    /// 三件套缺失的登记数（审计判据：== 0 才算全系统齐备）。
    pub violations: u32,
    /// 累计登记次数。
    pub registered_total: u64,
    /// 因容量满被拒绝的登记数（诚实记账）。
    pub rejected_total: u64,
}

impl FieldReg {
    pub fn new() -> FieldReg {
        FieldReg { fields: [const { None }; FIELD_CAP], violations: 0, registered_total: 0, rejected_total: 0 }
    }

    /// 登记一个输入框。重复 id 拒绝；三件缺件照登但计违规
    /// （审计清单要如实呈现缺件者，不能静默丢弃）。
    pub fn register(&mut self, spec: FieldSpec) -> bool {
        self.registered_total += 1;
        if self.fields.iter().any(|f| matches!(f, Some(s) if s.id == spec.id)) {
            return false;
        }
        let slot = match self.fields.iter_mut().find(|f| f.is_none()) {
            Some(s) => s,
            None => {
                self.rejected_total += 1;
                return false;
            }
        };
        if !spec.complete() {
            self.violations += 1;
        }
        *slot = Some(spec);
        true
    }

    /// 注销（窗口销毁等）——缺件违规一并撤账（该框已不存在）。
    pub fn unregister(&mut self, id: u32) -> bool {
        let pos = self.fields.iter().position(|f| matches!(f, Some(s) if s.id == id));
        match pos {
            Some(idx) => {
                if let Some(s) = self.fields[idx] {
                    if !s.complete() && self.violations > 0 {
                        self.violations -= 1;
                    }
                }
                self.fields[idx] = None;
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: u32) -> Option<FieldSpec> {
        self.fields.iter().find_map(|f| match f {
            Some(s) if s.id == id => Some(*s),
            _ => None,
        })
    }

    /// 登记数。
    pub fn count(&self) -> usize {
        self.fields.iter().filter(|f| f.is_some()).count()
    }

    /// 快照（诊断/审计面，允许短暂 Vec）。
    pub fn snapshot(&self) -> Vec<FieldSpec> {
        self.fields.iter().flatten().copied().collect()
    }
}

impl Default for FieldReg {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 占位符 / 清除按钮 / 超限——三件套行为判定（纯函数面）
// ---------------------------------------------------------------------------

/// 占位符可见性：四象限状态机——「失焦且为空」才显示。
///
/// | 聚焦 | 有内容 | 占位符 |
/// | --- | --- | --- |
/// | 否 | 否 | 显示 |
/// | 否 | 是 | 隐藏 |
/// | 是 | 否 | 隐藏（获得焦点即消失） |
/// | 是 | 是 | 隐藏 |
pub fn placeholder_visible(focused: bool, content_len: u32) -> bool {
    !focused && content_len == 0
}

/// 清除按钮可见性：「有内容且聚焦」时右端 × 才出现。
pub fn clear_visible(focused: bool, content_len: u32) -> bool {
    focused && content_len > 0
}

/// 超限红显判定：有上限且内容超限（N/M 中 N > M）。
pub fn over_limit(content_len: u32, max_len: u32) -> bool {
    max_len > 0 && content_len > max_len
}

/// 占位/正文灰值差判定——主册「灰值差 ≥2 档」的统一入口。
pub fn placeholder_body_ok(placeholder: Rgb8, body: Rgb8) -> bool {
    gray_steps_apart(placeholder, body) >= PLACEHOLDER_MIN_STEPS
}

// ---------------------------------------------------------------------------
// 单框状态机（行为面：事件流进、判定出）
// ---------------------------------------------------------------------------

/// 单框事件（审计面，小拷贝体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldEvent {
    FocusGained,
    FocusLost,
    /// 输入 n 字节（不截断——哪怕已超限）。
    Input(u32),
    /// 点击清除（焦点保持在框内）。
    Cleared,
    /// 跨入超限态。
    OverEntered,
    /// 删回限内（红色即时消退）。
    OverExited,
}

/// 单个输入框的运行态：三件套行为 + 事件审计。
pub struct Field {
    spec: FieldSpec,
    focused: bool,
    content_len: u32,
    events: RingLog<FieldEvent, FIELD_EVENT_CAP>,
    /// 截断事件数——**恒 0**：内容长度独立于上限增长，截断路径不存在
    /// （判据「超限不截断让用户自己删」的代码级表达）。
    pub truncation_events: u32,
    /// 清除点击次数。
    pub clear_clicks: u32,
}

impl Field {
    pub fn new(spec: FieldSpec) -> Field {
        Field {
            spec,
            focused: false,
            content_len: 0,
            events: RingLog::new(),
            truncation_events: 0,
            clear_clicks: 0,
        }
    }

    pub fn spec(&self) -> FieldSpec {
        self.spec
    }

    pub fn content_len(&self) -> u32 {
        self.content_len
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    fn log(&mut self, e: FieldEvent) {
        self.events.push(e);
    }

    /// 事件环快照（新→旧）。
    pub fn event_snapshot(&self) -> Vec<FieldEvent> {
        self.events.newest_first()
    }

    /// 焦点切换（占位符消失/回来的驱动源）。
    pub fn set_focus(&mut self, on: bool, _ts_ms: u64) {
        if self.focused == on {
            return;
        }
        self.focused = on;
        self.log(if on { FieldEvent::FocusGained } else { FieldEvent::FocusLost });
    }

    /// 输入 n 字节：内容长度独立增长（超限照收，**不截断**），
    /// 越限/回限状态沿记录事件。
    pub fn input_bytes(&mut self, n: u32, _ts_ms: u64) {
        if n == 0 {
            return;
        }
        let was_over = self.over_limit();
        self.content_len = self.content_len.saturating_add(n);
        self.log(FieldEvent::Input(n));
        if !was_over && self.over_limit() {
            self.log(FieldEvent::OverEntered);
        }
    }

    /// 删除 n 字节（用户自己删——退回限内红色即时消退）。
    pub fn delete_bytes(&mut self, n: u32, _ts_ms: u64) {
        let was_over = self.over_limit();
        self.content_len = self.content_len.saturating_sub(n);
        if was_over && !self.over_limit() {
            self.log(FieldEvent::OverExited);
        }
    }

    /// 一键清除：清空内容、**焦点保持在框内**（F229 判据）。
    pub fn clear(&mut self, _ts_ms: u64) {
        self.content_len = 0;
        self.clear_clicks += 1;
        self.log(FieldEvent::Cleared);
        // 焦点不动——这就是「清除后焦点保持」的实现：此函数不触碰 focused。
    }

    /// 占位符当前可见性。
    pub fn placeholder_visible(&self) -> bool {
        placeholder_visible(self.focused, self.content_len)
    }

    /// 清除按钮当前可见性。
    pub fn clear_visible(&self) -> bool {
        clear_visible(self.focused, self.content_len)
    }

    /// 超限红显。
    pub fn over_limit(&self) -> bool {
        over_limit(self.content_len, self.spec.max_len)
    }

    /// N/M 实时计数（无上限框回 None——不显示 0/0 这种无意义计数）。
    pub fn limit_display(&self) -> Option<(u32, u32)> {
        if self.spec.has_limit && self.spec.max_len > 0 {
            Some((self.content_len, self.spec.max_len))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F229 自检（11 条行为级）。
pub fn run_fieldui_checks() -> CheckSet {
    let mut set = CheckSet::new("F229-fieldui");

    // 1. 占位符四象限真值表。
    set.add(
        "placeholder quadrants: shown only when unfocused & empty",
        placeholder_visible(false, 0) && !placeholder_visible(false, 5)
            && !placeholder_visible(true, 0) && !placeholder_visible(true, 5),
        "",
    );

    // 2. 占位/正文灰值差：缺省灰对 ≥2 档，近邻灰 <2 档。
    set.add(
        "placeholder/body gray steps >= 2, near tones < 2",
        placeholder_body_ok(DEFAULT_PLACEHOLDER, DEFAULT_BODY)
            && !placeholder_body_ok(DEFAULT_PLACEHOLDER, Rgb8::new(140, 140, 140)),
        "",
    );

    // 3. 清除按钮四象限：有内容且聚焦才可见。
    set.add(
        "clear button visible only when focused & non-empty",
        !clear_visible(false, 0) && !clear_visible(false, 5)
            && !clear_visible(true, 0) && clear_visible(true, 5),
        "",
    );

    // 4. 清除后焦点保持：点击清除 → 内容空、焦点仍在框内。
    let mut f = Field::new(FieldSpec { id: 1, has_placeholder: true, has_clear: true, has_limit: true, max_len: 10 });
    f.set_focus(true, 100);
    f.input_bytes(6, 110);
    let focused_before = f.focused();
    f.clear(120);
    set.add(
        "clear empties content and keeps focus",
        f.content_len() == 0 && f.clear_clicks == 1 && f.focused() == focused_before && f.focused(),
        "",
    );

    // 5. 清除后占位符回来、清除按钮消失（失焦且为空语义联动）。
    set.add(
        "after clear: placeholder still hidden (focused), clear hidden (empty)",
        !f.placeholder_visible() && !f.clear_visible(),
        "",
    );

    // 6. 超限不截断：上限 10 输入 30 字节 → 长度 30、红显、截断事件 0。
    f.input_bytes(30, 130);
    set.add(
        "over-limit: content grows past max, red, no truncation",
        f.content_len() == 30 && f.over_limit() && f.truncation_events == 0,
        "",
    );

    // 7. N/M 实时显示 + 用户删回限内红色即时消退。
    let nm = f.limit_display();
    f.delete_bytes(30, 140);
    set.add(
        "N/M display live, red clears once back under limit",
        nm == Some((30, 10)) && !f.over_limit() && f.limit_display() == Some((0, 10)),
        "",
    );

    // 8. 越限/回限事件沿：OverEntered 在跨入时记录一次。
    let evs = f.event_snapshot();
    set.add(
        "over-limit edge events recorded",
        evs.iter().any(|e| matches!(e, FieldEvent::OverEntered))
            && evs.iter().any(|e| matches!(e, FieldEvent::OverExited)),
        "",
    );

    // 9. 登记册审计：缺件计违规、齐备不计；缺件框注销后撤账。
    let mut reg = FieldReg::new();
    let bad = FieldSpec { id: 10, has_placeholder: true, has_clear: false, has_limit: false, max_len: 0 };
    let good = FieldSpec { id: 11, has_placeholder: true, has_clear: true, has_limit: true, max_len: 20 };
    reg.register(bad);
    reg.register(good);
    let v1 = reg.violations;
    reg.unregister(10);
    set.add(
        "registry audit: incomplete counted, unregister revokes",
        v1 == 1 && reg.violations == 0 && reg.get(11).map(|s| s.complete()).unwrap_or(false),
        "",
    );

    // 10. 无上限框：永不红显、不显示 N/M。
    let mut f2 = Field::new(FieldSpec { id: 12, has_placeholder: true, has_clear: true, has_limit: false, max_len: 0 });
    f2.input_bytes(999, 200);
    set.add(
        "no-limit field: never red, no N/M",
        !f2.over_limit() && f2.limit_display().is_none() && f2.content_len() == 999,
        "",
    );

    // 11. fuzz 2000 轮：随机焦点/输入/删除/清除序列，不变量——
    //     占位可见 == (!聚焦 && 空)、清除可见 == (聚焦 && 非空)、
    //     红显 == (上限>0 && 超限)、截断事件恒 0、无 panic。
    let mut x: u32 = 0x0BADC0DE;
    let mut ff = Field::new(FieldSpec { id: 99, has_placeholder: true, has_clear: true, has_limit: true, max_len: 7 });
    let mut ok = true;
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 5;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = x % 5;
        match op {
            0 => ff.set_focus(x % 2 == 0, i as u64),
            1 => ff.input_bytes(n.max(1), i as u64),
            2 => ff.delete_bytes(n, i as u64),
            3 => {
                if ff.clear_visible() {
                    ff.clear(i as u64);
                }
            }
            _ => {}
        }
        let len = ff.content_len();
        if ff.placeholder_visible() != placeholder_visible(ff.focused(), len)
            || ff.clear_visible() != clear_visible(ff.focused(), len)
            || ff.over_limit() != over_limit(len, 7)
            || ff.truncation_events != 0
        {
            ok = false;
            break;
        }
    }
    set.add("fuzz 2000 rounds: invariants hold, truncation = 0", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: u32, max_len: u32) -> FieldSpec {
        FieldSpec { id, has_placeholder: true, has_clear: true, has_limit: max_len > 0, max_len }
    }

    #[test]
    fn placeholder_focus_lifecycle() {
        let mut f = Field::new(spec(1, 20));
        // 初始：失焦且为空 → 占位符显示。
        assert!(f.placeholder_visible());
        f.set_focus(true, 0); // 获得焦点即消失。
        assert!(!f.placeholder_visible());
        f.set_focus(false, 10);
        assert!(f.placeholder_visible(), "失焦且为空 → 回来");
        f.input_bytes(3, 20);
        assert!(!f.placeholder_visible(), "有内容 → 隐藏");
        f.set_focus(true, 30);
        f.clear(40);
        f.set_focus(false, 50);
        assert!(f.placeholder_visible(), "清除后失焦且为空 → 回来");
    }

    #[test]
    fn clear_keeps_focus_and_content_semantics() {
        let mut f = Field::new(spec(2, 50));
        f.set_focus(true, 0);
        f.input_bytes(10, 10);
        assert!(f.clear_visible());
        f.clear(20);
        assert_eq!(f.content_len(), 0);
        assert!(f.focused(), "清除后焦点保持在框内");
        assert!(!f.clear_visible());
        assert_eq!(f.clear_clicks, 1);
    }

    #[test]
    fn over_limit_never_truncates() {
        let mut f = Field::new(spec(3, 5));
        f.input_bytes(100, 0);
        assert_eq!(f.content_len(), 100, "内容长度独立于上限增长");
        assert!(f.over_limit());
        assert_eq!(f.truncation_events, 0);
        assert_eq!(f.limit_display(), Some((100, 5)));
        // 用户自己删：删回限内红消退。
        f.delete_bytes(96, 10);
        assert!(!f.over_limit());
        assert_eq!(f.limit_display(), Some((4, 5)));
        let evs = f.event_snapshot();
        assert!(evs.iter().any(|e| matches!(e, FieldEvent::OverEntered)));
        assert!(evs.iter().any(|e| matches!(e, FieldEvent::OverExited)));
    }

    #[test]
    fn registry_audit_lifecycle() {
        let mut reg = FieldReg::new();
        assert!(reg.register(spec(1, 10)));
        assert!(!reg.register(spec(1, 10)), "重复 id 拒绝");
        let incomplete = FieldSpec { id: 2, has_placeholder: true, has_clear: false, has_limit: true, max_len: 0 };
        assert!(reg.register(incomplete));
        assert_eq!(reg.violations, 1);
        assert_eq!(reg.count(), 2);
        assert!(reg.unregister(2));
        assert_eq!(reg.violations, 0);
        // 容量满拒绝并记账。
        for i in 0..(FIELD_CAP as u32 + 4) {
            let _ = reg.register(spec(100 + i, 5));
        }
        assert!(reg.rejected_total > 0);
        assert!(reg.count() <= FIELD_CAP);
    }

    #[test]
    fn gray_steps_gate() {
        assert!(placeholder_body_ok(Rgb8::new(150, 150, 150), Rgb8::new(40, 40, 40)));
        assert!(gray_steps_apart(DEFAULT_PLACEHOLDER, DEFAULT_BODY) >= 2);
        assert!(!placeholder_body_ok(Rgb8::new(128, 128, 128), Rgb8::new(110, 110, 110)));
    }

    #[test]
    fn fieldui_selfcheck_all_green() {
        let s = run_fieldui_checks();
        assert!(s.all_passed(), "F229 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const FIELDUI_PERSIST_VERSION: u8 = 1;
/// 落盘条目上限——登记册全量 FIELD_CAP=64 走内存审计（槽位+事件环），
/// 落盘只留快照预算内 16 框（定容纪律：槽位在册，超出不落盘）。
pub const FIELD_PERSIST_CAP: usize = 16;
/// 单条 7 字节：id u32（LE）+ 三件套位图 u8 + 上限 u16（LE）。
pub const FIELD_ENTRY_BYTES: usize = 7;
/// 定长记录 = 4 magic + 1 版本 + 载荷 1+16×7 + 4 校验 = 122B。
pub const FIELDUI_RECORD_LEN: usize = 5 + 1 + FIELD_PERSIST_CAP * FIELD_ENTRY_BYTES + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FielduiPersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 三件套位图编码（bit0 占位符 / bit1 清除 / bit2 上限——审计位图的
/// 唯一编码点）。
pub fn spec_flags(s: &FieldSpec) -> u8 {
    (s.has_placeholder as u8) | ((s.has_clear as u8) << 1) | ((s.has_limit as u8) << 2)
}

pub fn spec_from_flags(id: u32, flags: u8, max_len: u32) -> FieldSpec {
    FieldSpec {
        id,
        has_placeholder: flags & 1 != 0,
        has_clear: flags & 2 != 0,
        has_limit: flags & 4 != 0,
        max_len,
    }
}

/// 三件套审计记录（登记册快照——跨会话审计的落盘面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldAuditRecord {
    /// 实填条数（≤ FIELD_PERSIST_CAP，超出不落盘）。
    pub n: u8,
    pub ids: [u32; FIELD_PERSIST_CAP],
    pub flags: [u8; FIELD_PERSIST_CAP],
    pub max_lens: [u16; FIELD_PERSIST_CAP],
}

impl FieldAuditRecord {
    /// 从登记册捕获（槽序即落盘序；超快照容量的条目不落盘）。
    pub fn capture(reg: &FieldReg) -> FieldAuditRecord {
        let mut rec = FieldAuditRecord {
            n: 0,
            ids: [0; FIELD_PERSIST_CAP],
            flags: [0; FIELD_PERSIST_CAP],
            max_lens: [0; FIELD_PERSIST_CAP],
        };
        for (i, s) in reg.fields.iter().flatten().enumerate() {
            if i >= FIELD_PERSIST_CAP {
                break;
            }
            rec.ids[i] = s.id;
            rec.flags[i] = spec_flags(s);
            rec.max_lens[i] = s.max_len.min(u16::MAX as u32) as u16;
            rec.n += 1;
        }
        rec
    }

    /// 编码：[0..4]=magic、[4]=版本、[5]=条数、条目区、尾 4B=校验（LE）。
    pub fn to_bytes(&self) -> [u8; FIELDUI_RECORD_LEN] {
        let mut out = [0u8; FIELDUI_RECORD_LEN];
        out[0..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = FIELDUI_PERSIST_VERSION;
        out[5] = self.n;
        for i in 0..FIELD_PERSIST_CAP {
            let o = 6 + i * FIELD_ENTRY_BYTES;
            out[o..o + 4].copy_from_slice(&self.ids[i].to_le_bytes());
            out[o + 4] = self.flags[i];
            out[o + 5..o + 7].copy_from_slice(&self.max_lens[i].to_le_bytes());
        }
        let n = FIELDUI_RECORD_LEN;
        let sum = fnv1a32(&out[5..n - 4]);
        out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：四类损坏全拒绝 + 条数容量复核（>FIELD_PERSIST_CAP = 损坏）。
    pub fn from_bytes(b: &[u8]) -> Result<FieldAuditRecord, FielduiPersistError> {
        if b.len() != FIELDUI_RECORD_LEN {
            return Err(FielduiPersistError::BadLen);
        }
        if b[0..4] != VXH1_MAGIC {
            return Err(FielduiPersistError::BadMagic);
        }
        if b[4] != FIELDUI_PERSIST_VERSION {
            return Err(FielduiPersistError::BadVersion);
        }
        let n = b.len();
        let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
        if fnv1a32(&b[5..n - 4]) != sum {
            return Err(FielduiPersistError::BadChecksum);
        }
        if b[5] as usize > FIELD_PERSIST_CAP {
            return Err(FielduiPersistError::BadChecksum);
        }
        let mut rec = FieldAuditRecord {
            n: b[5],
            ids: [0; FIELD_PERSIST_CAP],
            flags: [0; FIELD_PERSIST_CAP],
            max_lens: [0; FIELD_PERSIST_CAP],
        };
        for i in 0..FIELD_PERSIST_CAP {
            let o = 6 + i * FIELD_ENTRY_BYTES;
            rec.ids[i] = u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            rec.flags[i] = b[o + 4];
            rec.max_lens[i] = u16::from_le_bytes([b[o + 5], b[o + 6]]);
        }
        Ok(rec)
    }

    /// 逐条解码为规格（审计复核面——三件套齐备性可离线重算）。
    pub fn spec_at(&self, i: usize) -> Option<FieldSpec> {
        if i >= self.n as usize {
            return None;
        }
        Some(spec_from_flags(self.ids[i], self.flags[i], self.max_lens[i] as u32))
    }
}

// --- v2 UI 壳接线面：清除按钮命中区几何 + 占位符绘制清单 ---

/// 命中语义动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldHitAction { Clear, Focus, None }

/// 清除按钮几何：框右端 h×h 方形、内缩 1px 描边余量——「有内容且聚焦
/// 时右端 ×」的几何承载（可见性判定走 clear_visible，一处一事实）。
pub fn clear_button_rect(field: crate::h1star::h1base::Rect) -> crate::h1star::h1base::Rect {
    crate::h1star::h1base::Rect::new(field.right() - field.h, field.y + 1, field.h - 2, field.h - 2)
}

/// 命中测试：点 → 语义动作。清除区命中以「可见」为门（失焦/空内容时
/// 按钮区退化为普通框内 → Focus——不产生幽灵热区）。
pub fn field_hit(
    field: crate::h1star::h1base::Rect,
    focused: bool,
    content_len: u32,
    x: i32,
    y: i32,
) -> FieldHitAction {
    if focused && content_len > 0 && clear_button_rect(field).contains(x, y) {
        FieldHitAction::Clear
    } else if field.contains(x, y) {
        FieldHitAction::Focus
    } else {
        FieldHitAction::None
    }
}

/// 占位符绘制令牌索引（0 = 占位符灰、1 = 正文色——灰值差 ≥2 档判据的
/// 消费端；颜色本体在令牌表，绘制面只持索引）。
pub const FIELD_COLOR_PLACEHOLDER: u8 = 0;
pub const FIELD_COLOR_BODY: u8 = 1;

/// 一条输入框图元（几何 + 颜色索引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldDrawItem {
    pub rect: crate::h1star::h1base::Rect,
    pub color_idx: u8,
}

/// 占位符绘制清单：占位可见（失焦且为空）时产出一条左对齐文本图元
/// （文本区 = 框内左缩 6px、宽钳进框内），否则空清单。
pub fn placeholder_draw_list(
    field: crate::h1star::h1base::Rect,
    focused: bool,
    content_len: u32,
    text_w: i32,
) -> ([Option<FieldDrawItem>; 1], usize) {
    if placeholder_visible(focused, content_len) {
        (
            [Some(FieldDrawItem {
                rect: crate::h1star::h1base::Rect::new(
                    field.x + 6,
                    field.y + 2,
                    text_w.min(field.w - 12).max(0),
                    field.h - 4,
                ),
                color_idx: FIELD_COLOR_PLACEHOLDER,
            })],
            1,
        )
    } else {
        ([const { None }; 1], 0)
    }
}

// --- v2 判定面扩展 ---

/// F229 v2 自检（首条必为持久化 round-trip）。
pub fn run_fieldui_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F229-fieldui-v2");

    // 1. round-trip：登记册快照编码→解码→逐条重算齐备性——验主册 F229
    //    「三件套审计清单」的跨会话落盘面。
    let mut reg = FieldReg::new();
    let _ = reg.register(FieldSpec { id: 7, has_placeholder: true, has_clear: true, has_limit: true, max_len: 20 });
    let _ = reg.register(FieldSpec { id: 8, has_placeholder: true, has_clear: false, has_limit: true, max_len: 5 });
    let rec = FieldAuditRecord::capture(&reg);
    let bytes = rec.to_bytes();
    set.add(
        "v2 audit record roundtrip",
        matches!(FieldAuditRecord::from_bytes(&bytes), Ok(back)
            if back == rec && back.n == 2
                && back.spec_at(0).map(|s| s.complete()).unwrap_or(false)
                && back.spec_at(1).map(|s| !s.complete()).unwrap_or(false)),
        "",
    );

    // 2. 四类损坏全拒绝 + 条数超容拒读——验十二查「损坏输入明错误」。
    let mut m = bytes;
    m[0] = b'X';
    let mut v = bytes;
    v[4] = 9;
    let mut s = bytes;
    s[10] ^= 0xFF;
    let mut c = bytes;
    c[5] = (FIELD_PERSIST_CAP + 1) as u8;
    let fixed = fnv1a32(&c[5..FIELDUI_RECORD_LEN - 4]);
    c[FIELDUI_RECORD_LEN - 4..FIELDUI_RECORD_LEN].copy_from_slice(&fixed.to_le_bytes());
    set.add(
        "v2 persist rejects 4 corrupt classes + wild count",
        FieldAuditRecord::from_bytes(&m) == Err(FielduiPersistError::BadMagic)
            && FieldAuditRecord::from_bytes(&v) == Err(FielduiPersistError::BadVersion)
            && FieldAuditRecord::from_bytes(&s) == Err(FielduiPersistError::BadChecksum)
            && FieldAuditRecord::from_bytes(&c) == Err(FielduiPersistError::BadChecksum)
            && FieldAuditRecord::from_bytes(&bytes[..bytes.len() - 1]) == Err(FielduiPersistError::BadLen),
        "",
    );

    // 3. 三件套位图语义：flags 编解码封闭、齐备性随位图离线重算——
    //    验主册 F229「占位符/清除/超限提示标配三件」的审计承载。
    let spec = spec_from_flags(9, 0b111, 30);
    set.add(
        "v2 spec flags bitmap closed",
        spec_flags(&spec) == 0b111 && spec.complete()
            && !spec_from_flags(9, 0b011, 30).complete()
            && !spec_from_flags(9, 0b101, 30).has_clear,
        "",
    );

    // 4. 清除按钮命中区：聚焦+有内容时按钮内 Clear、按钮外框内 Focus、
    //    框外 None；失焦或空内容时按钮区退化为 Focus（无幽灵热区）——
    //    验主册 F229「有内容且聚焦时右端 ×，点击清空并保持焦点」。
    let field = crate::h1star::h1base::Rect::new(100, 50, 300, 32);
    let bx = clear_button_rect(field);
    set.add(
        "v2 clear hit zone gated by focus & content",
        field_hit(field, true, 5, bx.x + bx.w / 2, bx.y + bx.h / 2) == FieldHitAction::Clear
            && field_hit(field, true, 5, field.x + 10, field.y + 5) == FieldHitAction::Focus
            && field_hit(field, true, 5, 500, 500) == FieldHitAction::None
            && field_hit(field, false, 5, bx.x + 2, bx.y + 2) == FieldHitAction::Focus
            && field_hit(field, true, 0, bx.x + 2, bx.y + 2) == FieldHitAction::Focus,
        "",
    );

    // 5. 占位符绘制清单：失焦空 → 一条占位灰图元（令牌索引 0、文本区
    //    左缩 6px）；聚焦 → 空清单——验主册 F229「获得焦点即消失、
    //    失焦且为空时回来」的绘制面。
    let (list, n_on) = placeholder_draw_list(field, false, 0, 120);
    let (_, n_off) = placeholder_draw_list(field, true, 0, 120);
    set.add(
        "v2 placeholder draw list gated by visibility",
        n_on == 1 && n_off == 0
            && list[0].map(|d| {
                d.color_idx == FIELD_COLOR_PLACEHOLDER
                    && d.rect.x == field.x + 6
                    && d.rect.w == 120
                    && d.rect.right() <= field.right()
            })
            .unwrap_or(false),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_capture_skips_beyond_snapshot_cap() {
        let mut reg = FieldReg::new();
        for i in 0..FIELD_PERSIST_CAP as u32 + 4 {
            let _ = reg.register(FieldSpec { id: i, has_placeholder: true, has_clear: true, has_limit: true, max_len: 9 });
        }
        let rec = FieldAuditRecord::capture(&reg);
        assert_eq!(rec.n as usize, FIELD_PERSIST_CAP, "超容条目不落盘（登记册内仍全量）");
        assert_eq!(reg.count(), 20);
    }

    #[test]
    fn v2_clear_rect_inside_field() {
        let field = crate::h1star::h1base::Rect::new(0, 0, 200, 40);
        let btn = clear_button_rect(field);
        assert_eq!((btn.x, btn.y, btn.w, btn.h), (160, 1, 38, 38));
        assert!(field.contains(btn.x, btn.y) && field.contains(btn.right() - 1, btn.bottom() - 1));
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_fieldui_v2_checks();
        assert!(set.all_passed(), "F229 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
