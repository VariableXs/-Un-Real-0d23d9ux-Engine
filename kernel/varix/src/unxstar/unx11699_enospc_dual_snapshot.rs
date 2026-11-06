//! UNX-F11699 ENOSPC 双机快照验证（B25 批收口卡）
//!
//! 本卡把 B25 批（UNX-F11681 ~ UNX-F11698）的观察点在两台机器（glibc 基准机与
//! musl 目标机）上各跑一遍，逐格比对：一致才记绿，任何不一致都必须以显式冲突
//! 的形式落在格里，不许被平均、不许被取其一。

use crate::alloc::string::String;

// ---------------------------------------------------------------------------
// 段基址与归因码
// ---------------------------------------------------------------------------

/// 本卡段基址。
pub const SEG_BASE: u32 = 0x6d430;

/// 槽位/用法错（结论码域内的小档位，域界 0xffff）。
pub const SN_SLOT: u32 = 0x0001;
/// 缺快照。
pub const SN_MISSING: u32 = 0x0002;
/// 结论码冲突未仲裁。
pub const SN_CONFLICT: u32 = 0x0003;
/// 观测值错档。
pub const SN_VALUE: u32 = 0x0004;

/// 通过结论码。
pub const VERDICT_PASS: u32 = 0;
/// 未判值。
pub const VERDICT_UNJUDGED: u32 = 99;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折入一个字（小端）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut n = w;
    let mut i = 0usize;
    while i < 8 {
        x = fold_byte(x, (n & 0xff) as u8);
        n >>= 8;
        i += 1;
    }
    x
}

/// 折入一段文本。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

/// 折入一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// 文本渲染
// ---------------------------------------------------------------------------

/// u32 → 固定宽度十六进制文本。
fn hex_text(v: u32, width: usize) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    let w = if width > 8 { 8 } else { width };
    let mut i = w;
    while i > 0 {
        let shift = (i - 1) * 4;
        let ndx = ((v >> shift) & 0xf) as usize;
        match digits.get(ndx) {
            Some(c) => s.push(*c as char),
            None => s.push('0'),
        }
        i -= 1;
    }
    s
}

/// u32 → 十进制文本。
pub fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        let d = (n % 10) as u8;
        n /= 10;
        i -= 1;
        buf[i] = b'0' + d;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s
}

/// 布尔 → 文本。
pub fn bool_text(v: bool) -> String {
    let mut s = String::new();
    s.push_str(if v { "true" } else { "false" });
    s
}

/// 段位文本：`0x6d430` 形态。
pub fn seg_text(v: u32) -> String {
    let mut s = String::new();
    s.push_str("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

// ---------------------------------------------------------------------------
// 闸门与注记
// ---------------------------------------------------------------------------

/// 本卡的六道闸。秩从 1 起算：用法 → 兜底 → 抓取 → 结论码 → 观测值 → 漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 秩 1：用法成立。
    Check,
    /// 秩 2：兜底档归属。
    Report,
    /// 秩 3：双机快照抓取。
    Capture,
    /// 秩 4：结论码冲突仲裁。
    Arbitrate,
    /// 秩 5：观测值比对。
    Compare,
    /// 秩 6：漂移归属与通过。
    Exit,
}

impl Stage {
    /// 秩（1..=6）。
    pub fn rank(&self) -> u32 {
        match self {
            Stage::Check => 1,
            Stage::Report => 2,
            Stage::Capture => 3,
            Stage::Arbitrate => 4,
            Stage::Compare => 5,
            Stage::Exit => 6,
        }
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Stage::Check => "check",
            Stage::Report => "report",
            Stage::Capture => "capture",
            Stage::Arbitrate => "arbitrate",
            Stage::Compare => "compare",
            Stage::Exit => "exit",
        }
    }
}

/// 收场注记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记。
    Nil,
    /// 缺快照。
    Missing,
    /// 用法错。
    Usage,
    /// 已仲裁的显式冲突。
    Held,
    /// 仲裁死锁。
    Conflict,
    /// 观测值错档。
    ValueOff,
    /// 两机全同。
    Same,
    /// 措辞漂移。
    Wording,
    /// 顺序漂移。
    Ordering,
}

impl Note {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Note::Nil => "nil",
            Note::Missing => "missing",
            Note::Usage => "usage",
            Note::Held => "held",
            Note::Conflict => "conflict",
            Note::ValueOff => "value-off",
            Note::Same => "same",
            Note::Wording => "wording",
            Note::Ordering => "ordering",
        }
    }
}

/// 缺陷维。申报对判的固定次序即枚举次序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 成败互报。
    Estate,
    /// 阶段不合。
    Stage,
    /// 归因码不合。
    Code,
    /// 观测值不合。
    Value,
    /// 注记不合。
    Note,
    /// 兜底档不合。
    Default,
    /// 无缺陷。
    Ok,
}

/// 文案漂移形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriftKind {
    /// 无漂移。
    None,
    /// 措辞漂移。
    Wording,
    /// 顺序漂移。
    Ordering,
}

impl DriftKind {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            DriftKind::None => "none",
            DriftKind::Wording => "wording",
            DriftKind::Ordering => "ordering",
        }
    }

    /// 是否真的漂了。
    pub fn is_drift(&self) -> bool {
        !matches!(self, DriftKind::None)
    }
}

/// 结论码不等时的仲裁去向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arbiter {
    /// 无人拍板。
    None,
    /// 按 glibc 基线和解。
    Glibc,
    /// 按 musl 侧和解。
    Musl,
    /// 仲裁死锁。
    Deadlock,
}

impl Arbiter {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Arbiter::None => "none",
            Arbiter::Glibc => "glibc",
            Arbiter::Musl => "musl",
            Arbiter::Deadlock => "deadlock",
        }
    }

    /// 是否已显式拍板。
    pub fn is_resolved(&self) -> bool {
        matches!(self, Arbiter::Glibc | Arbiter::Musl)
    }
}

/// 对照机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    /// glibc 基准机。
    Glibc,
    /// musl 目标机。
    Musl,
}

impl Machine {
    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self {
            Machine::Glibc => "glibc",
            Machine::Musl => "musl",
        }
    }
}

/// 判据卡（B25 批：F11681 ~ F11698，共 18 张）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    n: u32,
}

impl Card {
    /// 由卡号构造；越界即拒。
    pub const fn new(n: u32) -> Option<Card> {
        if n >= 11681 && n <= 11698 {
            Some(Card { n })
        } else {
            None
        }
    }

    /// 卡号。
    pub const fn num(&self) -> u32 {
        self.n
    }

    /// 短名。
    pub fn slug(&self) -> &'static str {
        match self.n {
            11681 => "f11681",
            11682 => "f11682",
            11683 => "f11683",
            11684 => "f11684",
            11685 => "f11685",
            11686 => "f11686",
            11687 => "f11687",
            11688 => "f11688",
            11689 => "f11689",
            11690 => "f11690",
            11691 => "f11691",
            11692 => "f11692",
            11693 => "f11693",
            11694 => "f11694",
            11695 => "f11695",
            11696 => "f11696",
            11697 => "f11697",
            _ => "f11698",
        }
    }
}

// ---------------------------------------------------------------------------
// 域模型：Side / Shape / Outcome / Claim / Case
// ---------------------------------------------------------------------------

/// 用法错时的观测值。
pub const USAGE_VALUE: u32 = 2;

/// 缺快照时的观测值。
pub const ABSENT_VALUE: u32 = 0;

/// 一台机器对一张卡的快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Side {
    /// 本机给出的结论码（0 = 通过）。
    pub code: u32,
    /// 本机观测到的值（errno / 块数等）。
    pub value: u32,
    /// 本机是否产出了快照。
    pub present: bool,
}

impl Side {
    /// 构造。
    pub const fn new(code: u32, value: u32, present: bool) -> Side {
        Side {
            code,
            value,
            present,
        }
    }

    /// 一台"全对"的机器。
    pub const fn clean(value: u32) -> Side {
        Side {
            code: VERDICT_PASS,
            value,
            present: true,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        fold_bool(h, self.present)
    }

    /// 一行呈现。
    pub fn render(&self) -> String {
        let mut s = String::from("code=");
        s.push_str(seg_text(self.code).as_str());
        s.push_str(" value=");
        s.push_str(dec_text(self.value).as_str());
        s.push_str(" present=");
        s.push_str(bool_text(self.present).as_str());
        s
    }
}

/// 一格双机现场。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 判据卡。
    pub card: Card,
    /// glibc 侧。
    pub glibc: Side,
    /// musl 侧。
    pub musl: Side,
    /// 文案漂移形态。
    pub drift: DriftKind,
    /// 结论码不等时的仲裁去向。
    pub arbiter: Arbiter,
    /// 命令行是否非法。
    pub bad_call: bool,
    /// 兜底档。
    pub default: bool,
}

impl Shape {
    /// 一格"双机全同"的现场。
    pub const fn pair(card: Card) -> Shape {
        Shape {
            card,
            glibc: Side::clean(0),
            musl: Side::clean(0),
            drift: DriftKind::None,
            arbiter: Arbiter::None,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"措辞漂移"的现场。
    pub const fn wording(card: Card) -> Shape {
        Shape {
            card,
            glibc: Side::clean(0),
            musl: Side::clean(0),
            drift: DriftKind::Wording,
            arbiter: Arbiter::None,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"顺序漂移"的现场。
    pub const fn ordering(card: Card) -> Shape {
        Shape {
            card,
            glibc: Side::clean(0),
            musl: Side::clean(0),
            drift: DriftKind::Ordering,
            arbiter: Arbiter::None,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"已仲裁的显式冲突"：musl 的结论码不同，按 glibc 基线和解。
    pub const fn held(card: Card) -> Shape {
        Shape {
            card,
            glibc: Side::clean(0),
            musl: Side::new(SN_VALUE, 0, true),
            drift: DriftKind::None,
            arbiter: Arbiter::Glibc,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"仲裁死锁"：两边码不同且无人拍板。
    pub const fn deadlock(card: Card) -> Shape {
        Shape {
            card,
            glibc: Side::new(SN_CONFLICT, 0, true),
            musl: Side::new(SN_MISSING, 0, true),
            drift: DriftKind::None,
            arbiter: Arbiter::Deadlock,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"观测值错档"：两边码同、值不同。
    pub const fn value_off(card: Card, glibc: u32, musl: u32) -> Shape {
        Shape {
            card,
            glibc: Side::new(VERDICT_PASS, glibc, true),
            musl: Side::new(VERDICT_PASS, musl, true),
            drift: DriftKind::None,
            arbiter: Arbiter::None,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"一边缺快照"。
    pub const fn missing(card: Card, which: Machine) -> Shape {
        let g = match which {
            Machine::Glibc => false,
            Machine::Musl => true,
        };
        let m = match which {
            Machine::Glibc => true,
            Machine::Musl => false,
        };
        Shape {
            card,
            glibc: Side::new(VERDICT_PASS, 0, g),
            musl: Side::new(VERDICT_PASS, 0, m),
            drift: DriftKind::None,
            arbiter: Arbiter::None,
            bad_call: false,
            default: false,
        }
    }

    /// 一格"兜底"。
    pub const fn unjudged(card: Card) -> Shape {
        let mut s = Shape::pair(card);
        s.default = true;
        s
    }

    /// 一格"用法错"。
    pub const fn usage(card: Card) -> Shape {
        let mut s = Shape::pair(card);
        s.bad_call = true;
        s
    }

    /// 该格的两边是否都到场。
    pub fn both_present(&self) -> bool {
        self.glibc.present && self.musl.present
    }

    /// 该格是否显式冲突（结论码不等）。
    pub fn is_conflict(&self) -> bool {
        self.glibc.code != self.musl.code
    }

    /// 该格是否真的漂了。
    pub fn drifted(&self) -> bool {
        self.drift.is_drift()
    }

    /// 域合法性。
    pub fn domain_ok(&self) -> bool {
        if self.card.num() < 11681 || self.card.num() > 11698 {
            return false;
        }
        if self.glibc.code > 0xffff || self.musl.code > 0xffff {
            return false;
        }
        if self.glibc.value > 1_000_000 || self.musl.value > 1_000_000 {
            return false;
        }
        true
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.card.num() as u64);
        h = fold_word(h, self.glibc.seal());
        h = fold_word(h, self.musl.seal());
        h = fold_str(h, self.drift.slug());
        h = fold_str(h, self.arbiter.slug());
        h = fold_bool(h, self.bad_call);
        fold_bool(h, self.default)
    }

    /// 一行呈现。
    pub fn render(&self) -> String {
        let mut s = String::from(self.card.slug());
        s.push(' ');
        s.push_str(self.glibc.render().as_str());
        s.push(' ');
        s.push_str(self.musl.render().as_str());
        s.push_str(" drift=");
        s.push_str(self.drift.slug());
        s.push_str(" arbiter=");
        s.push_str(self.arbiter.slug());
        s
    }
}

/// 一格的事实面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点阶段。
    pub stage: Stage,
    /// 归因码（0 = 无违诺）。
    pub code: u32,
    /// 观测值。
    pub value: u32,
    /// 收场注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 该格是否判对。
    pub ok: bool,
}

impl Outcome {
    /// 构造。
    pub const fn new(
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
        ok: bool,
    ) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default,
            ok,
        }
    }

    /// 成功收场。
    const fn pass(stage: Stage, value: u32, note: Note) -> Outcome {
        Outcome::new(stage, 0, value, note, false, true)
    }

    /// 违诺收场。
    const fn breach(stage: Stage, code: u32, value: u32, note: Note, default: bool) -> Outcome {
        Outcome::new(stage, code, value, note, default, false)
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok
    }

    /// 是否悬空（缺快照或未判）。
    pub fn dangling(&self) -> bool {
        self.default || self.note == Note::Nil || self.note == Note::Missing
    }

    /// 是否是显式冲突收场（含已仲裁与死锁）。
    pub fn conflicted(&self) -> bool {
        matches!(self.note, Note::Held | Note::Conflict)
    }

    /// 折叠六维。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.slug());
        h = fold_bool(h, self.default);
        fold_bool(h, self.ok)
    }

    /// 一行呈现。
    pub fn render(&self) -> String {
        let mut s = String::from("stage=");
        s.push_str(self.stage.slug());
        s.push_str(" code=");
        s.push_str(seg_text(self.code).as_str());
        s.push_str(" value=");
        s.push_str(dec_text(self.value).as_str());
        s.push_str(" note=");
        s.push_str(self.note.slug());
        s.push_str(" ok=");
        s.push_str(bool_text(self.ok).as_str());
        s
    }
}

/// 申报面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 槽标签。
    pub tag: &'static str,
    /// 申报的成败。
    pub success: bool,
    /// 申报的阶段。
    pub stage: Stage,
    /// 申报的归因码。
    pub code: u32,
    /// 申报的观测值。
    pub value: u32,
    /// 申报的注记。
    pub note: Note,
    /// 申报的兜底位。
    pub default: bool,
}

impl Claim {
    /// 构造。
    pub const fn new(
        tag: &'static str,
        success: bool,
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Claim {
        Claim {
            tag,
            success,
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 申报成败位。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 由事实面照抄。
    pub fn from_outcome(actual: &Outcome, success: bool, tag: &'static str) -> Claim {
        Claim::new(
            tag,
            success,
            actual.stage,
            actual.code,
            actual.value,
            actual.note,
            actual.default,
        )
    }

    /// 六维差异掩码。
    pub fn diff_mask(&self, actual: &Outcome) -> u32 {
        let mut m: u32 = 0;
        if actual.ok() != self.success {
            m |= 0x01;
        }
        if actual.stage != self.stage {
            m |= 0x02;
        }
        if actual.code != self.code {
            m |= 0x04;
        }
        if actual.value != self.value {
            m |= 0x08;
        }
        if actual.note != self.note {
            m |= 0x10;
        }
        if actual.default != self.default {
            m |= 0x20;
        }
        m
    }

    /// 折叠申报。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.slug());
        fold_bool(h, self.default)
    }

    /// 一行呈现。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tag);
        s.push_str(" ok=");
        s.push_str(bool_text(self.success).as_str());
        s.push(' ');
        s.push_str(self.stage.slug());
        s.push(' ');
        s.push_str(seg_text(self.code).as_str());
        s.push(' ');
        s.push_str(dec_text(self.value).as_str());
        s.push(' ');
        s.push_str(self.note.slug());
        s
    }
}

/// 一条语料。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 现场。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 构造。
    pub const fn new(shape: Shape, claim: Claim) -> Case {
        Case { shape, claim }
    }

    /// 真实结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }

    /// 判定。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否合格。
    pub fn ok(&self) -> bool {
        self.fault() == Fault::Ok
    }

    /// 语料指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.shape.seal());
        fold_word(h, self.claim.seal())
    }
}

// ---------------------------------------------------------------------------
// 本卡的不变量：一格的收场
// ---------------------------------------------------------------------------

/// 由现场推出事实面。
///
/// 五步，次序即闸门：用法 → 兜底 → 抓取 → 结论码 → 观测值 → 漂移。
/// 结论码先于观测值：一边说通过一边说失败，是冲突；两边都通过但值不同，
/// 是值差。冲突必须显式仲裁，值差轮不到仲裁粉饰。
pub fn define(s: &Shape) -> Outcome {
    // 闸 1 Check：用法错一律判死，且不碰快照。
    if s.bad_call {
        return Outcome::breach(Stage::Check, SN_SLOT, USAGE_VALUE, Note::Usage, false);
    }
    // 兜底档：可判定的卡片被丢进未判。
    if s.default {
        return Outcome::new(
            Stage::Report,
            0,
            VERDICT_UNJUDGED,
            Note::Nil,
            true,
            false,
        );
    }
    // 闸 2 Capture：两边都必须到场。
    if !s.both_present() {
        return Outcome::breach(Stage::Capture, SN_MISSING, ABSENT_VALUE, Note::Missing, false);
    }
    // 闸 5 Arbitrate：结论码不等，必须显式仲裁。
    if s.is_conflict() {
        if s.arbiter.is_resolved() {
            let v = match s.arbiter {
                Arbiter::Musl => s.musl.code,
                _ => s.glibc.code,
            };
            return Outcome::pass(Stage::Exit, v, Note::Held);
        }
        return Outcome::breach(Stage::Arbitrate, SN_CONFLICT, 0, Note::Conflict, false);
    }
    // 闸 4 Compare：值不等即红。
    if s.glibc.value != s.musl.value {
        return Outcome::breach(Stage::Compare, SN_VALUE, s.musl.value, Note::ValueOff, false);
    }
    // 漂移必须被记录：措辞与顺序各记各的，不许并成一档。
    match s.drift {
        DriftKind::None => Outcome::pass(Stage::Exit, s.glibc.value, Note::Same),
        DriftKind::Wording => Outcome::pass(Stage::Exit, s.glibc.value, Note::Wording),
        DriftKind::Ordering => Outcome::pass(Stage::Exit, s.glibc.value, Note::Ordering),
    }
}

/// 缺陷维 → 序位。
pub fn fault_ndx(f: Fault) -> usize {
    match f {
        Fault::Estate => 0,
        Fault::Stage => 1,
        Fault::Code => 2,
        Fault::Value => 3,
        Fault::Note => 4,
        Fault::Default => 5,
        Fault::Ok => 6,
    }
}

/// 序位 → 缺陷维。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0 => Some(Fault::Estate),
        1 => Some(Fault::Stage),
        2 => Some(Fault::Code),
        3 => Some(Fault::Value),
        4 => Some(Fault::Note),
        5 => Some(Fault::Default),
        6 => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = &c.claim;
    if a.ok() != k.success() {
        return Fault::Estate;
    }
    if a.stage != k.stage {
        return Fault::Stage;
    }
    if a.code != k.code {
        return Fault::Code;
    }
    if a.value != k.value {
        return Fault::Value;
    }
    if a.note != k.note {
        return Fault::Note;
    }
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}
