//! UNX-F48831 autorep 语义（REP PERIOD/DELAY 可调，默认对齐 Windows 版本锚）
//! UNX-F48832 修饰态快照与回放对账（组合终态快照可回放一致）
//!
//! ── 闸门次序 ──
//! G1 区间闸：delay∈[250,1000]ms、period∈[10,400]ms；越界不是「夹一下算了」，
//!    对外部设置必须回诊断码，只有内部「吸收式配置」才允许 clamped。
//! G2 首拍闸：第一次重复永远发生在按住后的 delay 末，不是第 0 拍；
//!    delay 未到就吐字符 = autorep 语义错。
//! G3 节拍闸：第 k 次重复时刻 = delay + (k-1)·period，逐拍闭式可比。
//! G4 改速闸：按住中途改周期只影响**后续**拍，已经发出去的拍不追认、不重排。
//! G5 快照闸：快照必须自带校验和；字段缺一个、多一个、值越域都拒绝。
//! G6 回放闸：mid 快照恢复后跑尾段，终态必须与不间断连续跑逐字段相等。
//!
//! ── 关键差异 ──
//! · 与 B01（原始码流）不同：本模块的输入是「按住/松开 + 毫秒时钟」，不碰扫描码。
//! · 与 B02 前面的修饰位机（unx48823）不同：那边算的是「当下有效掩码」，
//!   这边算的是「掩码 + 自动重复 + 死键挂起 + 布局」的**整体可回放状态**。
//! · 与 B03（输入法）不同：快照里没有预编辑串，只有已被接受的状态；预编辑归 B03。
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（B02 edge 段）。
pub const SEG_BASE: u32 = 0x703C0;

// ── 诊断码 ───────────────────────────────────────────────────
/// autorep / 快照族诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeCode {
    /// delay 小于 250ms
    DelayBelowMin = 1,
    /// delay 大于 1000ms
    DelayAboveMax = 2,
    /// period 小于 10ms
    PeriodBelowMin = 3,
    /// period 大于 400ms
    PeriodAboveMax = 4,
    /// 快照字段数不足
    SnapShort = 5,
    /// 快照字段解析失败
    SnapFieldBad = 6,
    /// 快照校验和不符
    SnapChecksum = 7,
    /// 回放终态与连续跑不一致
    ReplayDiverged = 8,
    /// 未按住却 tick 出重复
    RepNotArmed = 9,
    /// 双跑不一致
    NotReproducible = 10,
}

impl EdgeCode {
    /// 段内绝对码。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 文案。
    pub const fn detail(self) -> &'static str {
        match self {
            EdgeCode::DelayBelowMin => "重复延迟小于 250ms（Windows 锚下界）",
            EdgeCode::DelayAboveMax => "重复延迟大于 1000ms（Windows 锚上界）",
            EdgeCode::PeriodBelowMin => "重复周期小于 10ms（超 100Hz）",
            EdgeCode::PeriodAboveMax => "重复周期大于 400ms（低于 2.5Hz）",
            EdgeCode::SnapShort => "快照字段数不足",
            EdgeCode::SnapFieldBad => "快照字段不是合法十六进制",
            EdgeCode::SnapChecksum => "快照校验和不符（被截断或篡改）",
            EdgeCode::ReplayDiverged => "快照回放终态与连续跑不一致",
            EdgeCode::RepNotArmed => "未按住就吐自动重复",
            EdgeCode::NotReproducible => "双跑摘要不一致",
        }
    }

    /// 全码表。
    pub const ALL: [EdgeCode; 10] = [
        EdgeCode::DelayBelowMin,
        EdgeCode::DelayAboveMax,
        EdgeCode::PeriodBelowMin,
        EdgeCode::PeriodAboveMax,
        EdgeCode::SnapShort,
        EdgeCode::SnapFieldBad,
        EdgeCode::SnapChecksum,
        EdgeCode::ReplayDiverged,
        EdgeCode::RepNotArmed,
        EdgeCode::NotReproducible,
    ];
}

/// 末字符是否为十六进制位（校验和收尾自检）。
pub fn last_is_hex(t: &str) -> bool {
    match t.as_bytes().last() {
        Some(c) => match c {
            b'0'..=b'9' => true,
            b'a'..=b'f' => true,
            b'A'..=b'F' => true,
            _ => false,
        },
        None => false,
    }
}

/// 码 → 文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EdgeCode::ALL.len() {
        if let Some(c) = EdgeCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x703C0 段 autorep/快照闭集内"
}

// ── 折叠 ─────────────────────────────────────────────────────
/// 折一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折一个 32 位字。
pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let b = w.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 折一段文本。
pub fn fold_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 十六进制回写。
fn push_hex(s: &mut String, v: u32) {
    s.push_str("0x");
    let mut sh = 28u32;
    let mut started = false;
    loop {
        let nib = ((v >> sh) & 0xf) as u8;
        if nib != 0 || started || sh == 0 {
            started = true;
            let ch = match nib {
                0..=9 => (b'0' + nib) as char,
                _ => (b'a' + (nib - 10)) as char,
            };
            s.push(ch);
        }
        if sh == 0 {
            break;
        }
        sh -= 4;
    }
}

/// 十进制回写。
fn push_dec(s: &mut String, v: u32) {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = buf.len();
    loop {
        let d = (n % 10) as u8;
        n /= 10;
        if i == 0 {
            break;
        }
        i -= 1;
        if let Some(c) = buf.get_mut(i) {
            *c = b'0' + d;
        }
        if n == 0 {
            break;
        }
    }
    let mut j = i;
    while j < buf.len() {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
}

/// 十六进制解析。
fn parse_hex(t: &str) -> Option<u32> {
    let b = t.as_bytes();
    if b.len() < 3 {
        return None;
    }
    if b.first() != Some(&b'0') {
        return None;
    }
    let second = b.get(1)?;
    if *second != b'x' && *second != b'X' {
        return None;
    }
    let mut v = 0u32;
    let mut i = 2usize;
    while i < b.len() {
        let c = *b.get(i)?;
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d as u32)?;
        i += 1;
    }
    Some(v)
}

/// 固定种子掷骰。
pub struct Die {
    s: u32,
}

impl Die {
    /// 起骰。
    pub const fn new(seed: u32) -> Die {
        Die { s: seed ^ 0x4883_1B02 }
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.s ^= self.s >> 15;
        self.s
    }

    /// [0, bound)。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

// ── autorep ──────────────────────────────────────────────────
/// delay 下界（Windows 锚：慢速 1000ms，快速 250ms）。
pub const DELAY_MIN_MS: u32 = 250;
/// delay 上界。
pub const DELAY_MAX_MS: u32 = 1000;
/// period 下界（100Hz）。
pub const PERIOD_MIN_MS: u32 = 10;
/// period 上界（2.5Hz）。
pub const PERIOD_MAX_MS: u32 = 400;
/// Windows 默认重复延迟。
pub const DELAY_DEFAULT_MS: u32 = 500;
/// Windows 默认重复周期（31.25 字符/秒）。
pub const PERIOD_DEFAULT_MS: u32 = 32;

/// 重复配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepConfig {
    /// 首拍延迟（ms）
    pub delay_ms: u32,
    /// 后续周期（ms）
    pub period_ms: u32,
}

impl RepConfig {
    /// Windows 版本锚：500ms / 32ms。
    pub const fn windows_anchor() -> RepConfig {
        RepConfig { delay_ms: DELAY_DEFAULT_MS, period_ms: PERIOD_DEFAULT_MS }
    }

    /// 外部设置：越界必须报错（不夹）。
    pub fn check(&self) -> Result<(), u32> {
        if self.delay_ms < DELAY_MIN_MS {
            return Err(EdgeCode::DelayBelowMin.val());
        }
        if self.delay_ms > DELAY_MAX_MS {
            return Err(EdgeCode::DelayAboveMax.val());
        }
        if self.period_ms < PERIOD_MIN_MS {
            return Err(EdgeCode::PeriodBelowMin.val());
        }
        if self.period_ms > PERIOD_MAX_MS {
            return Err(EdgeCode::PeriodAboveMax.val());
        }
        Ok(())
    }

    /// 内部吸收式配置：先 clamps 再建。
    pub fn clamped(delay_ms: u32, period_ms: u32) -> RepConfig {
        RepConfig {
            delay_ms: clamp(delay_ms, DELAY_MIN_MS, DELAY_MAX_MS),
            period_ms: clamp(period_ms, PERIOD_MIN_MS, PERIOD_MAX_MS),
        }
    }

    /// 每秒字符数（period 换算）。
    pub fn chars_per_sec(&self) -> u32 {
        if self.period_ms == 0 {
            return 0;
        }
        1000 / self.period_ms
    }
}

impl Default for RepConfig {
    fn default() -> RepConfig {
        RepConfig::windows_anchor()
    }
}

/// 夹取。
pub fn clamp(v: u32, lo: u32, hi: u32) -> u32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 自动重复状态机（毫秒时钟，每 tick 调一次）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepState {
    /// 当前配置
    pub cfg: RepConfig,
    /// 是否按住
    pub armed: bool,
    /// 按住时刻
    pub since: u32,
    /// 下一拍时刻
    pub next: u32,
    /// 最近一拍时刻
    pub last: u32,
    /// 已发拍数
    pub repeats: u32,
    /// 未按住时的 tick 次数（必须为 0 才能通过 RepNotArmed）
    pub stray_ticks: u32,
}

impl RepState {
    /// 起机（松开态）。
    pub fn new(cfg: RepConfig) -> RepState {
        RepState {
            cfg,
            armed: false,
            since: 0,
            next: 0,
            last: 0,
            repeats: 0,
            stray_ticks: 0,
        }
    }

    /// 按下：首拍排在 since + delay，绝不排在第 0 拍。
    pub fn press(&mut self, t: u32, cfg: RepConfig) -> Result<(), u32> {
        cfg.check()?;
        self.cfg = cfg;
        self.armed = true;
        self.since = t;
        self.next = t.saturating_add(cfg.delay_ms);
        self.last = t;
        self.repeats = 0;
        Ok(())
    }

    /// 松开：清拍。
    pub fn release(&mut self) {
        self.armed = false;
        self.next = 0;
        self.repeats = 0;
    }

    /// 走一拍时钟：(t, 是否吐字, 第几拍)。
    pub fn tick(&mut self, t: u32) -> (u32, bool, bool) {
        if !self.armed {
            self.stray_ticks = self.stray_ticks.saturating_add(1);
            return (t, false, false);
        }
        if t >= self.next {
            self.last = self.next;
            self.next = self.next.saturating_add(self.cfg.period_ms);
            self.repeats = self.repeats.saturating_add(1);
            return (t, true, false);
        }
        (t, false, false)
    }

    /// 改速：只影响后续拍（已发拍不追认）；若 delay 已过则按「上一拍 + 新周期」重排。
    pub fn reconfigure(&mut self, cfg: RepConfig) -> Result<(), u32> {
        cfg.check()?;
        if self.armed {
            let base = self.last;
            self.next = base.saturating_add(cfg.period_ms);
        }
        self.cfg = cfg;
        Ok(())
    }
}

/// 一次按住产生的全部重复时刻（含首拍）。
pub fn autorep_ticks(cfg: RepConfig, hold_ms: u32) -> Result<Vec<u32>, u32> {
    cfg.check()?;
    let mut st = RepState::new(cfg);
    st.press(0, cfg)?;
    let mut v: Vec<u32> = Vec::new();
    let mut t = 1u32;
    while t <= hold_ms {
        let (_, fired, _) = st.tick(t);
        if fired {
            v.push(st.last);
        }
        t = t.saturating_add(1);
    }
    Ok(v)
}

/// 独立闭式臂：第 k 拍 = delay + (k-1)·period（k≥1）。
pub fn expect_ticks(cfg: RepConfig, hold_ms: u32) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    if cfg.period_ms == 0 || cfg.delay_ms > hold_ms {
        return v;
    }
    let mut t = cfg.delay_ms;
    let mut guard = 0u32;
    while t <= hold_ms && guard < 100_000u32 {
        v.push(t);
        t = t.saturating_add(cfg.period_ms);
        guard = guard.saturating_add(1);
    }
    v
}

/// 五种组合（判据 F48831 的 ×5）。
pub const REP_COMBOS: [(u32, u32); 5] = [
    (500, 32),
    (250, 32),
    (1000, 32),
    (500, 100),
    (750, 50),
];

/// 五组合节奏报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepReport {
    /// 组合数
    pub combos: u32,
    /// 全部拍数
    pub total: u32,
    /// 不一致拍数（必须 0）
    pub mismatches: u32,
    /// 首拍是否都不为 0
    pub first_after_delay: bool,
    /// 摘要
    pub digest: u64,
}

/// 五组合 × 2000ms 按住：逐拍与闭式臂一致。
pub fn rep_five() -> Result<RepReport, u32> {
    let mut total = 0u32;
    let mut mismatches = 0u32;
    let mut first_ok = true;
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < REP_COMBOS.len() {
        let spec = match REP_COMBOS.get(i) {
            Some(v) => *v,
            None => break,
        };
        let cfg = RepConfig { delay_ms: spec.0, period_ms: spec.1 };
        cfg.check()?;
        let got = autorep_ticks(cfg, 2000)?;
        let want = expect_ticks(cfg, 2000);
        if got.len() != want.len() {
            mismatches = mismatches.saturating_add((got.len() as u32).abs_diff(want.len() as u32));
        }
        let mut j = 0usize;
        while j < want.len() {
            let g = got.get(j).copied().unwrap_or(u32::MAX);
            let w = want.get(j).copied().unwrap_or(u32::MAX);
            digest = fold_word(digest, g);
            if g != w {
                mismatches = mismatches.saturating_add(1);
            }
            if j == 0 && g != cfg.delay_ms {
                first_ok = false;
            }
            j = j.saturating_add(1);
        }
        total = total.saturating_add(got.len() as u32);
        i = i.saturating_add(1);
    }
    Ok(RepReport {
        combos: REP_COMBOS.len() as u32,
        total,
        mismatches,
        first_after_delay: first_ok,
        digest,
    })
}

// ── 修饰位常量（与本段其它模块同义，此处自带一份以保持独立可探） ──
/// Shift
pub const B_SHIFT: u8 = 0x01;
/// Caps
pub const B_CAPS: u8 = 0x02;
/// Ctrl
pub const B_CTRL: u8 = 0x04;
/// Alt
pub const B_ALT: u8 = 0x08;
/// Meta
pub const B_META: u8 = 0x10;
/// Num
pub const B_NUM: u8 = 0x20;
/// AltGr / Group
pub const B_GRP: u8 = 0x40;

// ── 状态机 + 快照 ────────────────────────────────────────────
/// 一条状态操作（快照回放脚本用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// 按下修饰位
    Press(u8),
    /// 松开修饰位
    Release(u8),
    /// 翻转锁存位
    Lock(u8),
    /// 置闩锁位
    Latch(u8),
    /// 清闩锁位
    Unlatch(u8),
    /// 设组号（0..=3）
    Grp(u8),
    /// 挂起死键 keysym
    Dead(u32),
    /// 切布局（0=us, 1=de）
    Layout(u8),
    /// 改重复配置
    Rep(u32, u32),
    /// 空转一拍
    Tick,
}

/// 修饰状态机（可快照、可回放）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Machine {
    /// 瞬时按下位
    pub bits: u8,
    /// 锁存位
    pub locked: u8,
    /// 闩锁位
    pub latched: u8,
    /// 组号
    pub grp: u8,
    /// 挂起死键
    pub dead: u32,
    /// 重复配置
    pub rep: RepConfig,
    /// 布局
    pub layout: u8,
    /// 操作序号
    pub seq: u32,
}

impl Machine {
    /// 起机（全清，Windows 重复锚）。
    pub const fn new() -> Machine {
        Machine {
            bits: 0,
            locked: 0,
            latched: 0,
            grp: 0,
            dead: 0,
            rep: RepConfig { delay_ms: DELAY_DEFAULT_MS, period_ms: PERIOD_DEFAULT_MS },
            layout: 0,
            seq: 0,
        }
    }

    /// 有效掩码：瞬时 | 锁存 | 闩锁。
    pub fn effective(&self) -> u8 {
        self.bits | self.locked | self.latched
    }

    /// 跑一条操作。
    pub fn apply(&mut self, op: Op) {
        match op {
            Op::Press(b) => self.bits |= b & 0x7f,
            Op::Release(b) => self.bits &= !(b & 0x7f),
            Op::Lock(b) => self.locked ^= b & 0x7f,
            Op::Latch(b) => self.latched |= b & 0x7f,
            Op::Unlatch(b) => self.latched &= !(b & 0x7f),
            Op::Grp(g) => self.grp = g & 0x03,
            Op::Dead(k) => self.dead = k & 0x1f_ffff,
            Op::Layout(l) => self.layout = l & 0x01,
            Op::Rep(d, p) => self.rep = RepConfig::clamped(d, p),
            Op::Tick => {}
        }
        self.seq = self.seq.saturating_add(1);
    }

    /// 拍一张快照。
    pub fn snapshot(&self) -> Snap {
        Snap {
            bits: self.bits,
            locked: self.locked,
            latched: self.latched,
            grp: self.grp,
            dead: self.dead,
            delay_ms: self.rep.delay_ms,
            period_ms: self.rep.period_ms,
            layout: self.layout,
            seq: self.seq,
        }
    }

    /// 从快照恢复（新机，不留历史）。
    pub fn restore(s: Snap) -> Machine {
        Machine {
            bits: s.bits,
            locked: s.locked,
            latched: s.latched,
            grp: s.grp,
            dead: s.dead,
            rep: RepConfig { delay_ms: s.delay_ms, period_ms: s.period_ms },
            layout: s.layout,
            seq: s.seq,
        }
    }
}

impl Default for Machine {
    fn default() -> Machine {
        Machine::new()
    }
}

/// 状态快照（9 字段 + 校验和）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snap {
    /// 瞬时位
    pub bits: u8,
    /// 锁存位
    pub locked: u8,
    /// 闩锁位
    pub latched: u8,
    /// 组号
    pub grp: u8,
    /// 挂起死键
    pub dead: u32,
    /// 重复延迟
    pub delay_ms: u32,
    /// 重复周期
    pub period_ms: u32,
    /// 布局
    pub layout: u8,
    /// 序号
    pub seq: u32,
}

impl Snap {
    /// 有效掩码（快照自带，回放后必须与原机相等）。
    pub fn effective(&self) -> u8 {
        self.bits | self.locked | self.latched
    }

    /// 字段折叠（也是校验和的原料）。
    pub fn fold(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        h = fold_byte(h, self.bits);
        h = fold_byte(h, self.locked);
        h = fold_byte(h, self.latched);
        h = fold_byte(h, self.grp);
        h = fold_word(h, self.dead);
        h = fold_word(h, self.delay_ms);
        h = fold_word(h, self.period_ms);
        h = fold_byte(h, self.layout);
        h = fold_word(h, self.seq);
        h
    }
}

/// 快照字段数。
pub const SNAP_FIELDS: usize = 9;
/// 快照串前缀。
pub const SNAP_TAG: &str = "snap1";

/// 快照 → 文本（10 段：tag + 9 字段 + ck）。
pub fn serialize(s: Snap) -> String {
    let mut t = String::new();
    t.push_str(SNAP_TAG);
    t.push(':');
    push_hex(&mut t, s.bits as u32);
    t.push(':');
    push_hex(&mut t, s.locked as u32);
    t.push(':');
    push_hex(&mut t, s.latched as u32);
    t.push(':');
    push_hex(&mut t, s.grp as u32);
    t.push(':');
    push_hex(&mut t, s.dead);
    t.push(':');
    push_hex(&mut t, s.delay_ms);
    t.push(':');
    push_hex(&mut t, s.period_ms);
    t.push(':');
    push_hex(&mut t, s.layout as u32);
    t.push(':');
    push_hex(&mut t, s.seq);
    t.push(':');
    push_hex(&mut t, (s.fold() & 0xffff_ffff) as u32);
    t
}

/// 文本 → 快照（严格：段数、前缀、十六进制、区间、校验和全查）。
pub fn parse_snap(txt: &str) -> Result<Snap, u32> {
    let mut parts: Vec<&str> = Vec::new();
    for p in txt.split(':') {
        parts.push(p);
    }
    if parts.len() != SNAP_FIELDS + 2 {
        return Err(EdgeCode::SnapShort.val());
    }
    if parts.first().copied() != Some(SNAP_TAG) {
        return Err(EdgeCode::SnapFieldBad.val());
    }
    let mut vals: [u32; 10] = [0; 10];
    let mut i = 1usize;
    while i < parts.len() {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return Err(EdgeCode::SnapShort.val()),
        };
        let v = match parse_hex(p) {
            Some(v) => v,
            None => return Err(EdgeCode::SnapFieldBad.val()),
        };
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = v;
        }
        i += 1;
    }
    let f = |idx: usize| -> u32 { vals.get(idx).copied().unwrap_or(0) };
    let s = Snap {
        bits: (f(0) & 0x7f) as u8,
        locked: (f(1) & 0x7f) as u8,
        latched: (f(2) & 0x7f) as u8,
        grp: (f(3) & 0x03) as u8,
        dead: f(4) & 0x1f_ffff,
        delay_ms: f(5),
        period_ms: f(6),
        layout: (f(7) & 0x01) as u8,
        seq: f(8),
    };
    if s.delay_ms < DELAY_MIN_MS
        || s.delay_ms > DELAY_MAX_MS
        || s.period_ms < PERIOD_MIN_MS
        || s.period_ms > PERIOD_MAX_MS
    {
        return Err(EdgeCode::SnapFieldBad.val());
    }
    let want_ck = (s.fold() & 0xffff_ffff) as u32;
    if want_ck != f(9) {
        return Err(EdgeCode::SnapChecksum.val());
    }
    Ok(s)
}

/// 快照比对是否逐字段相等。
pub fn snap_eq(a: Snap, b: Snap) -> bool {
    a.bits == b.bits
        && a.locked == b.locked
        && a.latched == b.latched
        && a.grp == b.grp
        && a.dead == b.dead
        && a.delay_ms == b.delay_ms
        && a.period_ms == b.period_ms
        && a.layout == b.layout
        && a.seq == b.seq
}

/// 伪随机操作脚本。
pub fn random_ops(seed: u32, n: u32) -> Vec<Op> {
    let mut d = Die::new(seed);
    let mut v: Vec<Op> = Vec::new();
    let mut i = 0u32;
    while i < n {
        let pick = d.below(10);
        let op = match pick {
            0 => Op::Press((1u8 << d.below(7)) as u8),
            1 => Op::Release((1u8 << d.below(7)) as u8),
            2 => Op::Lock((1u8 << d.below(7)) as u8),
            3 => Op::Latch((1u8 << d.below(7)) as u8),
            4 => Op::Unlatch((1u8 << d.below(7)) as u8),
            5 => Op::Grp((d.below(4)) as u8),
            6 => Op::Dead(0xfe50u32.saturating_add(d.below(32))),
            7 => Op::Layout((d.below(2)) as u8),
            8 => Op::Rep(250 + d.below(751), 10 + d.below(391)),
            _ => Op::Tick,
        };
        v.push(op);
        i = i.saturating_add(1);
    }
    v
}

/// 跑一段脚本。
pub fn run_ops(m: &mut Machine, ops: &[Op]) {
    let mut i = 0usize;
    while i < ops.len() {
        if let Some(op) = ops.get(i) {
            m.apply(*op);
        }
        i += 1;
    }
}

/// 快照回放报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapReport {
    /// 轮数
    pub rounds: u32,
    /// 序列化往返成功次数
    pub roundtrips: u32,
    /// 回放终态不一致次数（必须 0）
    pub mismatches: u32,
    /// 有效掩码不一致次数（必须 0）
    pub mask_mismatch: u32,
    /// 摘要
    pub digest: u64,
}

/// 快照 ×rounds 回放：中点拍快照 → 文本 → 解析 → 新机 → 跑尾段 → 与连续跑比终态。
pub fn snapshot_100(rounds: u32) -> SnapReport {
    let mut rep = SnapReport {
        rounds: 0,
        roundtrips: 0,
        mismatches: 0,
        mask_mismatch: 0,
        digest: 0xcbf2_9ce4_8422_2325u64,
    };
    let mut r = 0u32;
    while r < rounds {
        let ops = random_ops(0x4883_2000 ^ r, 12);
        // 连续跑
        let mut a = Machine::new();
        run_ops(&mut a, ops.as_slice());
        // 分段跑：前 5 条 → 快照 → 新机 → 后 7 条
        let mut b = Machine::new();
        let mut i = 0usize;
        while i < 5 {
            if let Some(op) = ops.get(i) {
                b.apply(*op);
            }
            i += 1;
        }
        let snap = b.snapshot();
        let txt = serialize(snap);
        rep.digest = fold_text(rep.digest, txt.as_str());
        match parse_snap(txt.as_str()) {
            Ok(back) => {
                if snap_eq(back, snap) {
                    rep.roundtrips = rep.roundtrips.saturating_add(1);
                }
                let mut c = Machine::restore(back);
                let mut j = 5usize;
                while j < ops.len() {
                    if let Some(op) = ops.get(j) {
                        c.apply(*op);
                    }
                    j += 1;
                }
                if snap_eq(c.snapshot(), a.snapshot()) {
                    // 终态一致
                } else {
                    rep.mismatches = rep.mismatches.saturating_add(1);
                }
                if c.effective() != a.effective() {
                    rep.mask_mismatch = rep.mask_mismatch.saturating_add(1);
                }
                if Machine::restore(back).snapshot().effective() != snap.effective() {
                    rep.mask_mismatch = rep.mask_mismatch.saturating_add(1);
                }
            }
            Err(_) => {
                rep.mismatches = rep.mismatches.saturating_add(1);
            }
        }
        rep.rounds = rep.rounds.saturating_add(1);
        r = r.saturating_add(1);
    }
    rep
}

// ── 黄金跑 / 探针 ────────────────────────────────────────────
/// 黄金跑：(组合数, 回放轮数, 首拍合序标记)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let r = rep_five()?;
    if r.combos != 5 || r.mismatches != 0 || !r.first_after_delay {
        return Err(EdgeCode::DelayBelowMin.val());
    }
    let s = snapshot_100(100);
    if s.rounds != 100 || s.roundtrips != 100 || s.mismatches != 0 || s.mask_mismatch != 0 {
        return Err(EdgeCode::ReplayDiverged.val());
    }
    Ok((r.combos, s.rounds, 1))
}

/// 探针 A：锚点首拍 = 500ms。
pub fn probe_seq_a() -> Result<(), u32> {
    let cfg = RepConfig::windows_anchor();
    let v = autorep_ticks(cfg, 700)?;
    match v.first() {
        Some(500) => Ok(()),
        _ => Err(EdgeCode::DelayBelowMin.val()),
    }
}

/// 探针 B：快照往返。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = Machine::new();
    m.apply(Op::Press(B_SHIFT));
    m.apply(Op::Lock(B_CAPS));
    m.apply(Op::Dead(0xfe52));
    let txt = serialize(m.snapshot());
    match parse_snap(txt.as_str()) {
        Ok(s) => {
            if snap_eq(s, m.snapshot()) && s.effective() == (B_SHIFT | B_CAPS) {
                Ok(())
            } else {
                Err(EdgeCode::SnapChecksum.val())
            }
        }
        Err(c) => Err(c),
    }
}

/// 探针 C：越界配置必须拒绝。
pub fn probe_seq_c() -> Result<(), u32> {
    let cfg = RepConfig { delay_ms: 100, period_ms: 32 };
    match cfg.check() {
        Err(c) if c == EdgeCode::DelayBelowMin.val() => Ok(()),
        _ => Err(EdgeCode::DelayBelowMin.val()),
    }
}

/// 双跑守卫：摘要或总拍数不等即 NotReproducible。
pub fn repro_guard(a: &RepReport, b: &RepReport) -> Result<(), u32> {
    if a.digest == b.digest && a.total == b.total && a.combos == b.combos {
        Ok(())
    } else {
        Err(EdgeCode::NotReproducible.val())
    }
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EdgeCode::NotReproducible.val());
    }
    let r1 = rep_five()?;
    let r2 = rep_five()?;
    repro_guard(&r1, &r2)?;
    let s1 = snapshot_100(100);
    let s2 = snapshot_100(100);
    if s1.digest != s2.digest {
        return Err(EdgeCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < EdgeCode::ALL.len() {
        if let Some(c) = EdgeCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x20 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 注入账（十槽） ────────────────────────────────────────────
/// 十槽注入账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 期望码
    pub want: [u32; 10],
    /// 判定次数
    pub tries: [u32; 10],
    /// 命中次数
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 建账。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                EdgeCode::DelayBelowMin.val(),
                EdgeCode::DelayAboveMax.val(),
                EdgeCode::PeriodBelowMin.val(),
                EdgeCode::PeriodAboveMax.val(),
                EdgeCode::SnapShort.val(),
                EdgeCode::SnapFieldBad.val(),
                EdgeCode::SnapChecksum.val(),
                EdgeCode::ReplayDiverged.val(),
                EdgeCode::NotReproducible.val(),
                EdgeCode::RepNotArmed.val(),
            ],
            tries: [0; 10],
            caught: [0; 10],
        }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, got: u32) {
        if slot >= 10 {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        let want = self.want.get(slot).copied().unwrap_or(0);
        if got == want {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    /// 全槽命中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 10 {
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

impl Default for InjectLedger {
    fn default() -> InjectLedger {
        InjectLedger::new()
    }
}

/// 十槽注入。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0..3：四种越界。
        let c0 = RepConfig { delay_ms: 100, period_ms: 32 };
        led.record(0, match c0.check() {
            Err(c) => c,
            Ok(_) => 0,
        });
        let c1 = RepConfig { delay_ms: 1200, period_ms: 32 };
        led.record(1, match c1.check() {
            Err(c) => c,
            Ok(_) => 0,
        });
        let c2 = RepConfig { delay_ms: 500, period_ms: 5 };
        led.record(2, match c2.check() {
            Err(c) => c,
            Ok(_) => 0,
        });
        let c3 = RepConfig { delay_ms: 500, period_ms: 900 };
        led.record(3, match c3.check() {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 槽 4：快照段数不足。
        let mut m = Machine::new();
        m.apply(Op::Press(B_SHIFT));
        let txt = serialize(m.snapshot());
        led.record(4, match parse_snap("snap1:0x1:0x0") {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 槽 5：字段非法。
        led.record(5, match parse_snap("snap1:0x1:0x0:0x0:0x0:0x0:0x1f4:0x20:0x0:0x0:zz") {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 槽 6：校验和错（改最后一位）。
        let mut bad = String::new();
        let n = txt.len();
        let mut i = 0usize;
        while i < n {
            if let Some(ch) = txt.as_bytes().get(i) {
                if i + 1 == n {
                    bad.push(if *ch == b'0' { '1' } else { '0' });
                } else {
                    bad.push(*ch as char);
                }
            }
            i += 1;
        }
        led.record(6, match parse_snap(bad.as_str()) {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 槽 7：分叉检测器本身必须有效——走过不同路的终态要能被抓出来。
        let mut a7 = Machine::new();
        a7.apply(Op::Press(B_SHIFT));
        a7.apply(Op::Lock(B_CAPS));
        let snap7 = a7.snapshot();
        let mut b7 = Machine::restore(snap7);
        b7.apply(Op::Release(B_SHIFT));
        let diverged = !snap_eq(b7.snapshot(), a7.snapshot());
        led.record(7, if diverged { EdgeCode::ReplayDiverged.val() } else { 0 });
        // 槽 8：双跑守卫必须能抓出摘要差一。
        let a = rep_five()?;
        let mut b = rep_five()?;
        b.digest ^= 1;
        led.record(8, match repro_guard(&a, &b) {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 槽 9：未按住 tick。
        let mut st = RepState::new(RepConfig::windows_anchor());
        let _ = st.tick(10);
        led.record(9, if st.stray_ticks >= 1 { EdgeCode::RepNotArmed.val() } else { 0 });
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 判据 ─────────────────────────────────────────────────────
/// 十条判据。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1 五组合节奏一致
    let r = rep_five();
    let (ok1, d1) = match r {
        Ok(rep) => {
            let mut t = String::new();
            t.push_str("combos=");
            push_dec(&mut t, rep.combos);
            t.push_str(" ticks=");
            push_dec(&mut t, rep.total);
            t.push_str(" mismatch=");
            push_dec(&mut t, rep.mismatches);
            (rep.combos == 5 && rep.mismatches == 0, t)
        }
        Err(c) => {
            let mut t = String::new();
            t.push_str("err=");
            push_hex(&mut t, c);
            (false, t)
        }
    };
    v.push(("F48831-J1-autorep×5组合节奏断言一致", ok1, d1));

    // 2 默认锚
    let anchor = RepConfig::windows_anchor();
    let (ok2, d2) = {
        let mut t = String::new();
        t.push_str("delay=");
        push_dec(&mut t, anchor.delay_ms);
        t.push_str(" period=");
        push_dec(&mut t, anchor.period_ms);
        t.push_str(" cps=");
        push_dec(&mut t, anchor.chars_per_sec());
        (anchor.delay_ms == 500 && anchor.period_ms == 32 && anchor.chars_per_sec() == 31, t)
    };
    v.push(("F48831-J2-默认对齐Windows版本锚", ok2, d2));

    // 3 越界拒绝 + 吸收式夹取
    let mut rejects = 0u32;
    let specs = [(100u32, 32u32), (1200, 32), (500, 5), (500, 900)];
    let mut i = 0usize;
    while i < specs.len() {
        if let Some(s) = specs.get(i) {
            let c = RepConfig { delay_ms: s.0, period_ms: s.1 };
            if c.check().is_err() {
                rejects = rejects.saturating_add(1);
            }
        }
        i += 1;
    }
    let cl = RepConfig::clamped(0, 100_000);
    let (ok3, d3) = {
        let mut t = String::new();
        t.push_str("rejects=");
        push_dec(&mut t, rejects);
        t.push_str(" clamped=");
        push_hex(&mut t, cl.delay_ms);
        t.push('/');
        push_hex(&mut t, cl.period_ms);
        (
            rejects == 4 && cl.delay_ms == DELAY_MIN_MS && cl.period_ms == PERIOD_MAX_MS && cl.check().is_ok(),
            t,
        )
    };
    v.push(("F48831-J3-四向越界拒绝且夹取自洽", ok3, d3));

    // 4 首拍在 delay 末
    let mut first_ok = true;
    let mut k = 0usize;
    while k < REP_COMBOS.len() {
        if let Some(s) = REP_COMBOS.get(k) {
            let cfg = RepConfig { delay_ms: s.0, period_ms: s.1 };
            if let Ok(ticks) = autorep_ticks(cfg, 3000) {
                if ticks.first().copied() != Some(cfg.delay_ms) {
                    first_ok = false;
                }
            }
        }
        k += 1;
    }
    v.push(("F48831-J4-首拍落在delay末不在第0拍", first_ok, String::from("first=delay")));

    // 5 改速只影响后续拍
    let mut st = RepState::new(RepConfig::windows_anchor());
    let _ = st.press(0, RepConfig::windows_anchor());
    let mut fired_at_500 = false;
    let mut t = 1u32;
    while t <= 500 {
        let (_, fired, _) = st.tick(t);
        if fired {
            fired_at_500 = true;
        }
        t = t.saturating_add(1);
    }
    let _ = st.reconfigure(RepConfig { delay_ms: 500, period_ms: 100 });
    let mut next_fire = 0u32;
    let mut t = 501u32;
    while t <= 700 {
        let (_, fired, _) = st.tick(t);
        if fired && next_fire == 0 {
            next_fire = st.last;
        }
        t = t.saturating_add(1);
    }
    let ok5 = fired_at_500 && next_fire == 600 && st.repeats == 3;
    v.push(("F48831-J5-按住中途改速不追认已发拍", ok5, String::from("500 then 600/700")));

    // 6 快照百轮回放
    let s6 = snapshot_100(100);
    let (ok6, d6) = {
        let mut t = String::new();
        t.push_str("rounds=");
        push_dec(&mut t, s6.rounds);
        t.push_str(" roundtrip=");
        push_dec(&mut t, s6.roundtrips);
        t.push_str(" diff=");
        push_dec(&mut t, s6.mismatches);
        (s6.rounds == 100 && s6.roundtrips == 100 && s6.mismatches == 0, t)
    };
    v.push(("F48832-J1-快照×10^2回放一致", ok6, d6));

    // 7 序列化往返
    let mut rt = 0u32;
    let mut i = 0u32;
    while i < 20 {
        let ops = random_ops(0x7777 ^ i, 6);
        let mut m = Machine::new();
        run_ops(&mut m, ops.as_slice());
        let txt = serialize(m.snapshot());
        if let Ok(back) = parse_snap(txt.as_str()) {
            if snap_eq(back, m.snapshot()) {
                rt = rt.saturating_add(1);
            }
        }
        i += 1;
    }
    let (ok7, d7) = {
        let mut t = String::new();
        t.push_str("roundtrip=");
        push_dec(&mut t, rt);
        t.push_str("/20 fields=");
        push_dec(&mut t, SNAP_FIELDS as u32);
        (rt == 20, t)
    };
    v.push(("F48832-J2-九字段快照文本往返逐字段相等", ok7, d7));

    // 8 回放终态等于连续跑
    let s8 = snapshot_100(100);
    let ok8 = s8.mismatches == 0 && s8.mask_mismatch == 0;
    v.push(("F48832-J3-回放终态与连续跑逐字段相等", ok8, String::from("diff=0")));

    // 9 快照带校验和与有效掩码
    let mut m9 = Machine::new();
    m9.apply(Op::Press(B_ALT));
    m9.apply(Op::Lock(B_CAPS));
    m9.apply(Op::Latch(B_GRP));
    let sn = m9.snapshot();
    let txt9 = serialize(sn);
    let ok9 = sn.effective() == (B_ALT | B_CAPS | B_GRP)
        && parse_snap(txt9.as_str()).map(|b| snap_eq(b, sn)).unwrap_or(false)
        && last_is_hex(txt9.as_str());
    v.push(("F48832-J4-快照含校验和与有效掩码", ok9, String::from("ck+mask")));

    // 10 双跑一致
    let ok10 = repro_check().is_ok();
    v.push(("F48832-J5-同种子双跑摘要一致", ok10, String::from("repro ok")));

    v
}

// ── 检查集 ───────────────────────────────────────────────────
/// 运行本模块判据。
pub fn run_unx48831_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48831 autorep 节奏 / 快照回放");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        if let Some((name, passed, d)) = checks.get(i) {
            if d.is_empty() {
                s.add(name, *passed, "");
            } else {
                s.add_dyn(name, *passed, d.as_str());
            }
        }
        i += 1;
    }
    s
}

// ── 测试 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_anchor_defaults() {
        let c = RepConfig::windows_anchor();
        assert_eq!(c.delay_ms, 500);
        assert_eq!(c.period_ms, 32);
        assert_eq!(c.chars_per_sec(), 31);
        assert!(c.check().is_ok());
        assert_eq!(RepConfig::default(), c);
    }

    #[test]
    fn t_clamp_and_check() {
        assert_eq!(RepConfig::clamped(1, 1), RepConfig { delay_ms: 250, period_ms: 10 });
        assert_eq!(RepConfig::clamped(5000, 5000), RepConfig { delay_ms: 1000, period_ms: 400 });
        assert_eq!(
            RepConfig { delay_ms: 249, period_ms: 32 }.check(),
            Err(EdgeCode::DelayBelowMin.val())
        );
        assert_eq!(
            RepConfig { delay_ms: 1001, period_ms: 32 }.check(),
            Err(EdgeCode::DelayAboveMax.val())
        );
        assert_eq!(
            RepConfig { delay_ms: 500, period_ms: 9 }.check(),
            Err(EdgeCode::PeriodBelowMin.val())
        );
        assert_eq!(
            RepConfig { delay_ms: 500, period_ms: 401 }.check(),
            Err(EdgeCode::PeriodAboveMax.val())
        );
    }

    #[test]
    fn t_autorep_positions() {
        let cfg = RepConfig { delay_ms: 500, period_ms: 32 };
        let ticks = autorep_ticks(cfg, 600).expect("ticks");
        assert_eq!(ticks.first().copied(), Some(500));
        assert_eq!(ticks.len(), 4);
        let want = expect_ticks(cfg, 600);
        assert_eq!(ticks, want);
        // 首拍不在 0 拍
        assert_ne!(ticks.first().copied(), Some(0));
    }

    #[test]
    fn t_autorep_five_combos() {
        let r = rep_five().expect("five");
        assert_eq!(r.combos, 5);
        assert_eq!(r.mismatches, 0);
        assert!(r.first_after_delay);
        assert!(r.total > 0);
        assert_eq!(rep_five().expect("again").digest, r.digest);
    }

    #[test]
    fn t_reconfigure_keeps_history() {
        let mut st = RepState::new(RepConfig::windows_anchor());
        st.press(0, RepConfig::windows_anchor()).expect("press");
        let mut fired = Vec::new();
        let mut t = 1u32;
        while t <= 500 {
            let (_, f, _) = st.tick(t);
            if f {
                fired.push(t);
            }
            t += 1;
        }
        assert_eq!(fired, vec![500]);
        st.reconfigure(RepConfig { delay_ms: 500, period_ms: 100 }).expect("cfg");
        let mut later = Vec::new();
        let mut t = 501u32;
        while t <= 800 {
            let (_, f, _) = st.tick(t);
            if f {
                later.push(t);
            }
            t += 1;
        }
        assert_eq!(later.first().copied(), Some(600));
        assert_eq!(later, vec![600, 700, 800]);
        assert_eq!(st.repeats, 4);
    }

    #[test]
    fn t_rep_stray_tick() {
        let mut st = RepState::new(RepConfig::windows_anchor());
        let (_, f, _) = st.tick(1);
        assert!(!f);
        assert_eq!(st.stray_ticks, 1);
        // 松开后再 tick 仍然是 stray
        st.release();
        let (_, f2, _) = st.tick(2);
        assert!(!f2);
        assert_eq!(st.stray_ticks, 2);
    }

    #[test]
    fn t_snapshot_roundtrip() {
        let mut m = Machine::new();
        m.apply(Op::Press(B_SHIFT));
        m.apply(Op::Lock(B_CAPS));
        m.apply(Op::Latch(B_GRP));
        m.apply(Op::Dead(0xfe52));
        m.apply(Op::Layout(1));
        m.apply(Op::Rep(750, 50));
        let sn = m.snapshot();
        let txt = serialize(sn);
        assert_eq!(txt.matches(':').count(), SNAP_FIELDS + 1);
        let back = parse_snap(txt.as_str()).expect("parse");
        assert!(snap_eq(back, sn));
        assert_eq!(back.effective(), B_SHIFT | B_CAPS | B_GRP);
        assert_eq!(back.layout, 1);
        assert_eq!(back.dead, 0xfe52);
        assert_eq!(Machine::restore(back).snapshot(), sn);
    }

    #[test]
    fn t_snapshot_rejects() {
        assert_eq!(parse_snap("snap1:0x1"), Err(EdgeCode::SnapShort.val()));
        assert_eq!(
            parse_snap("snap2:0x1:0x0:0x0:0x0:0x0:0x1f4:0x20:0x0:0x0:0x1"),
            Err(EdgeCode::SnapFieldBad.val())
        );
        assert_eq!(
            parse_snap("snap1:0x1:0x0:0x0:0x0:0x0:0x1f4:0x20:0x0:0x0:0x0"),
            Err(EdgeCode::SnapChecksum.val())
        );
        // 区间外 delay
        let mut m = Machine::new();
        m.apply(Op::Dead(0xfe50));
        let txt = serialize(m.snapshot());
        let back = parse_snap(txt.as_str()).expect("ok");
        assert_eq!(back.delay_ms, 500);
    }

    #[test]
    fn t_snapshot_100_replay() {
        let s = snapshot_100(100);
        assert_eq!(s.rounds, 100);
        assert_eq!(s.roundtrips, 100);
        assert_eq!(s.mismatches, 0);
        assert_eq!(s.mask_mismatch, 0);
        assert_eq!(snapshot_100(100).digest, s.digest);
    }

    #[test]
    fn t_golden_probe_inject() {
        let g = golden_run().expect("golden");
        assert_eq!(g, (5, 100, 1));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail(0).contains("未知"));
        for c in EdgeCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x20);
        }
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48831_checks().all_passed());
    }
}
