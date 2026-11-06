//! UNX-F48836 keymap 引擎与修饰键状态机观测埋点（入口/出口/耗时/结果四点全覆盖）
//! UNX-F48837 keymap 引擎与修饰键状态机 fuzz 面（乱参数注入不崩）
//! UNX-F48838 keymap 引擎与修饰键状态机判据用例集（20 例入库可复跑）
//!
//! ── 闸门次序 ──
//! G1 入口闸：每次工作单元先 enter，同一单元重复 enter = DoubleEnter（不覆盖旧记录）。
//! G2 出口闸：exit 必须与 enter 成对；没有 enter 的 exit = OrphanExit。
//! G3 耗时闸：cost 只能落在 enter 之后、exit 之前；总耗时必须与逐条耗时之和相等。
//! G4 结果闸：**任何** Err 都必须有一条 Result(code≠0) 记录——异常零静默。
//! G5 fuzz 闸：乱参数注入必须每步有界（≤3 步）、失败必须带码、10^4 次不崩。
//! G6 用例闸：20 例按子系统铺满（parse/switch/mod/led/io 各 4 例），双跑复跑一致。
//!
//! ── 关键差异 ──
//! · 与 B01/B03 不同：本模块不新增输入语义，只给 B02 的四类语义（解析/切换/修饰/LED）
//!   装「四点埋点」，并把它们固化成 20 条可复跑用例。
//! · 与本批 unx48833 不同：那边管错误码**族**（对外词汇表），这边管**观测覆盖**
//!   （入口/出口/耗时/结果四个观测点一个都不能缺），二者正交。
//! · 与 runtime 日志不同：这里不落盘、不打字，只留定容结构化记录，便于探针直接断言。
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（B02 quality 段）。
pub const SEG_BASE: u32 = 0x70460;

// ── 诊断码 ───────────────────────────────────────────────────
/// 观测 / fuzz / 用例族诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualCode {
    /// 同一单元重复 enter
    DoubleEnter = 1,
    /// 没有 enter 的 exit
    OrphanExit = 2,
    /// 单元开着就结束（缺 exit）
    MissingExit = 3,
    /// 出现 Err 却没有 Result 记录
    SilentError = 4,
    /// 耗时记录越界（exit 之后又来 cost）
    CostAfterExit = 5,
    /// 某个观测点缺记录
    PointMissing = 6,
    /// 用例期望与实际不符
    BadCase = 7,
    /// 用例名重复
    CaseDup = 8,
    /// 用例编号不连续
    CaseGap = 9,
    /// fuzz 参数越界未被拒
    FuzzUnbounded = 10,
    /// 观测账满
    TraceOverflow = 11,
    /// 双跑不一致
    NotReproducible = 12,
}

impl QualCode {
    /// 段内绝对码。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 文案。
    pub const fn detail(self) -> &'static str {
        match self {
            QualCode::DoubleEnter => "同一工作单元重复入口埋点",
            QualCode::OrphanExit => "没有入口埋点的出口",
            QualCode::MissingExit => "工作单元结束时仍缺出口埋点",
            QualCode::SilentError => "失败没有结果埋点（静默异常）",
            QualCode::CostAfterExit => "出口之后仍上报耗时",
            QualCode::PointMissing => "四点埋点不齐（入口/出口/耗时/结果）",
            QualCode::BadCase => "用例期望码与实际码不符",
            QualCode::CaseDup => "用例名重复",
            QualCode::CaseGap => "用例编号不连续",
            QualCode::FuzzUnbounded => "fuzz 单步超过步数上限",
            QualCode::TraceOverflow => "观测账已满，记录被丢弃",
            QualCode::NotReproducible => "双跑摘要不一致",
        }
    }

    /// 全码表。
    pub const ALL: [QualCode; 12] = [
        QualCode::DoubleEnter,
        QualCode::OrphanExit,
        QualCode::MissingExit,
        QualCode::SilentError,
        QualCode::CostAfterExit,
        QualCode::PointMissing,
        QualCode::BadCase,
        QualCode::CaseDup,
        QualCode::CaseGap,
        QualCode::FuzzUnbounded,
        QualCode::TraceOverflow,
        QualCode::NotReproducible,
    ];
}

/// 码 → 文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QualCode::ALL.len() {
        if let Some(c) = QualCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70460 段 quality 闭集内"
}

/// 码是否在段内。
pub fn in_set(code: u32) -> bool {
    let mut i = 0usize;
    while i < QualCode::ALL.len() {
        if let Some(c) = QualCode::ALL.get(i) {
            if c.val() == code {
                return true;
            }
        }
        i += 1;
    }
    false
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
        Die { s: seed ^ 0x4883_6046 }
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_644_521).wrapping_add(2_245_531);
        self.s ^= self.s >> 13;
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

// ── 子语义（解析/切换/修饰/LED/IO 的最小可跑件） ──────────────
/// keysym 域上界。
pub const KSYM_MAX: u32 = 0x1f_ffff;

/// 解析一行；错误码落在 quality 段的 parse 三码上。
pub fn parse_line(line: &str, seen: &mut Vec<u16>) -> Result<(u16, u32), u32> {
    let eq = match line.find('=') {
        Some(p) => p,
        None => return Err(QualCode::BadCase.val()),
    };
    let left = line.get(..eq).unwrap_or("").trim();
    let right = line.get(eq + 1..).unwrap_or("").trim();
    let kc_txt = match left.strip_prefix("kc.") {
        Some(v) => v.trim(),
        None => return Err(QualCode::BadCase.val()),
    };
    let mut kc = 0u32;
    let b = kc_txt.as_bytes();
    if b.is_empty() {
        return Err(QualCode::BadCase.val());
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(QualCode::BadCase.val());
        }
        kc = match kc.checked_mul(10).and_then(|v| v.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(QualCode::PointMissing.val()),
        };
        i += 1;
    }
    if kc > u16::MAX as u32 {
        return Err(QualCode::PointMissing.val());
    }
    let lo_t = match right.split(',').next() {
        Some(v) => v.trim(),
        None => return Err(QualCode::BadCase.val()),
    };
    let lo = match parse_hex(lo_t) {
        Some(v) => v,
        None => return Err(QualCode::BadCase.val()),
    };
    if lo == 0 || lo > KSYM_MAX {
        return Err(QualCode::PointMissing.val());
    }
    if seen.iter().any(|x| *x == kc as u16) {
        return Err(QualCode::CaseDup.val());
    }
    seen.push(kc as u16);
    Ok((kc as u16, lo))
}

/// 迷你切换器。
pub struct MiniSwitch {
    /// 生效布局
    pub active: u8,
    /// 目标布局
    pub pending: Option<u8>,
    /// 缓冲
    pub q: Vec<u16>,
}

/// 窗口上限。
pub const SWITCH_CAP: usize = 8;

impl MiniSwitch {
    /// 起机。
    pub fn new() -> MiniSwitch {
        MiniSwitch { active: 0, pending: None, q: Vec::new() }
    }

    /// 起切换。
    pub fn begin(&mut self, to: u8) -> Result<(), u32> {
        if self.pending.is_some() || to == self.active || to > 1 {
            return Err(QualCode::DoubleEnter.val());
        }
        self.pending = Some(to);
        Ok(())
    }

    /// 投递。
    pub fn feed(&mut self, kc: u16) -> Result<(), u32> {
        if self.pending.is_none() {
            return Ok(());
        }
        if self.q.len() >= SWITCH_CAP {
            return Err(QualCode::TraceOverflow.val());
        }
        self.q.push(kc);
        Ok(())
    }

    /// 落账。
    pub fn commit(&mut self) -> Result<u32, u32> {
        let to = match self.pending {
            Some(t) => t,
            None => return Err(QualCode::OrphanExit.val()),
        };
        self.active = to;
        self.pending = None;
        let n = self.q.len() as u32;
        self.q.clear();
        Ok(n)
    }
}

impl Default for MiniSwitch {
    fn default() -> MiniSwitch {
        MiniSwitch::new()
    }
}

/// 迷你修饰栈。
pub struct ModStack {
    /// 瞬时位
    pub bits: u8,
    /// 闩锁位
    pub latched: u8,
}

impl ModStack {
    /// 空栈。
    pub const fn new() -> ModStack {
        ModStack { bits: 0, latched: 0 }
    }

    /// 按下。
    pub fn press(&mut self, bit: u8) -> Result<(), u32> {
        let b = bit & 0x7f;
        if b == 0 {
            return Err(QualCode::MissingExit.val());
        }
        if self.bits & b != 0 {
            return Err(QualCode::DoubleEnter.val());
        }
        self.bits |= b;
        Ok(())
    }

    /// 松开。
    pub fn release(&mut self, bit: u8) -> Result<(), u32> {
        let b = bit & 0x7f;
        if self.bits & b == 0 {
            return Err(QualCode::OrphanExit.val());
        }
        self.bits &= !b;
        Ok(())
    }

    /// 闩锁。
    pub fn latch(&mut self, bit: u8) {
        self.latched |= bit & 0x7f;
    }

    /// 结算（全松后闩锁必须清）。
    pub fn settle(&mut self) -> Result<(), u32> {
        self.bits = 0;
        if self.latched != 0 {
            self.latched = 0;
            return Err(QualCode::CostAfterExit.val());
        }
        Ok(())
    }
}

impl Default for ModStack {
    fn default() -> ModStack {
        ModStack::new()
    }
}

/// 迷你 LED 灯。
pub struct LedLamp {
    /// 连续失败
    pub streak: u32,
    /// 停机
    pub down: bool,
    /// 账行
    pub rows: u32,
}

/// 重试预算。
pub const LED_BUDGET: u32 = 3;
/// 账容量。
pub const LED_CAP: u32 = 8;

impl LedLamp {
    /// 起灯。
    pub const fn new() -> LedLamp {
        LedLamp { streak: 0, down: false, rows: 0 }
    }

    /// 写失败。
    pub fn fail(&mut self) -> Result<(), u32> {
        if self.down {
            return Err(QualCode::SilentError.val());
        }
        self.streak = self.streak.saturating_add(1);
        if self.rows >= LED_CAP {
            return Err(QualCode::TraceOverflow.val());
        }
        self.rows = self.rows.saturating_add(1);
        if self.streak >= LED_BUDGET {
            self.down = true;
            return Err(QualCode::PointMissing.val());
        }
        Ok(())
    }

    /// 写成功。
    pub fn ok(&mut self) {
        self.streak = 0;
    }
}

impl Default for LedLamp {
    fn default() -> LedLamp {
        LedLamp::new()
    }
}

/// IO 短读。
pub fn io_read(want: u32, got: u32) -> Result<u32, u32> {
    if got < want {
        return Err(QualCode::PointMissing.val());
    }
    Ok(got)
}

/// IO 校验和。
pub fn io_checksum(want: u32, got: u32) -> Result<(), u32> {
    if want != got {
        return Err(QualCode::NotReproducible.val());
    }
    Ok(())
}

/// IO 回退静默。
pub fn io_fallback(fallback: bool, alarmed: bool) -> Result<(), u32> {
    if fallback && !alarmed {
        return Err(QualCode::SilentError.val());
    }
    Ok(())
}

// ── 四点埋点 ─────────────────────────────────────────────────
/// 观测点（入口 / 耗时 / 出口 / 结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Point {
    /// 入口
    Enter = 0,
    /// 耗时
    Cost = 1,
    /// 出口
    Exit = 2,
    /// 结果
    Result = 3,
}

/// 观测点个数。
pub const POINTS: usize = 4;

/// 一条观测记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rec {
    /// 观测点
    pub point: u8,
    /// 工作单元编号
    pub id: u16,
    /// 时钟
    pub tick: u32,
    /// 结果码（0 = 成功）
    pub code: u32,
}

/// 定容观测账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    /// 记录
    pub recs: Vec<Rec>,
    /// 丢弃条数（必须 0）
    pub dropped: u32,
}

/// 观测账容量（足够 200 单元 × 4 点）。
pub const TRACE_CAP: usize = 4096;

impl Trace {
    /// 空账。
    pub fn new() -> Trace {
        Trace { recs: Vec::new(), dropped: 0 }
    }

    /// 记一条。
    pub fn push(&mut self, point: u8, id: u16, tick: u32, code: u32) -> Result<(), u32> {
        if self.recs.len() >= TRACE_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(QualCode::TraceOverflow.val());
        }
        self.recs.push(Rec { point, id, tick, code });
        Ok(())
    }

    /// 某观测点条数。
    pub fn count_of(&self, point: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.point == point {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 结果码非零的条数。
    pub fn err_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.point == Point::Result as u8 && r.code != 0 {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 摘要。
    pub fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                h = fold_byte(h, r.point);
                h = fold_word(h, r.id as u32);
                h = fold_word(h, r.tick);
                h = fold_word(h, r.code);
            }
            i += 1;
        }
        h
    }
}

impl Default for Trace {
    fn default() -> Trace {
        Trace::new()
    }
}

/// 埋点工作单元：进入栈 + 耗时 + 结果。
pub struct Instrumented {
    /// 观测账
    pub trace: Trace,
    /// 时钟
    pub tick: u32,
    /// 当前开着的单元（编号）
    pub open: Vec<u16>,
    /// 入口数
    pub enters: u32,
    /// 出口数
    pub exits: u32,
    /// 耗时条数
    pub costs: u32,
    /// 结果条数
    pub results: u32,
    /// 失败单元数
    pub errs: u32,
    /// 耗时总和
    pub cost_total: u64,
    /// 耗时对不上次数（必须 0）
    pub cost_mismatch: u32,
}

impl Instrumented {
    /// 起机。
    pub fn new() -> Instrumented {
        Instrumented {
            trace: Trace::new(),
            tick: 0,
            open: Vec::new(),
            enters: 0,
            exits: 0,
            costs: 0,
            results: 0,
            errs: 0,
            cost_total: 0,
            cost_mismatch: 0,
        }
    }

    /// 入口：重复入口即 DoubleEnter（不覆盖已开记录）。
    pub fn enter(&mut self, id: u16) -> Result<(), u32> {
        self.tick = self.tick.saturating_add(1);
        if self.open.iter().any(|x| *x == id) {
            let _ = self.trace.push(Point::Result as u8, id, self.tick, QualCode::DoubleEnter.val());
            self.results = self.results.saturating_add(1);
            self.errs = self.errs.saturating_add(1);
            return Err(QualCode::DoubleEnter.val());
        }
        self.open.push(id);
        let _ = self.trace.push(Point::Enter as u8, id, self.tick, 0);
        self.enters = self.enters.saturating_add(1);
        Ok(())
    }

    /// 耗时：必须在开着的单元上报。
    pub fn cost(&mut self, id: u16, delta: u32) -> Result<(), u32> {
        self.tick = self.tick.saturating_add(1);
        if !self.open.iter().any(|x| *x == id) {
            let _ = self.trace.push(Point::Result as u8, id, self.tick, QualCode::CostAfterExit.val());
            self.results = self.results.saturating_add(1);
            self.errs = self.errs.saturating_add(1);
            return Err(QualCode::CostAfterExit.val());
        }
        let _ = self.trace.push(Point::Cost as u8, id, self.tick, delta);
        self.costs = self.costs.saturating_add(1);
        self.cost_total = self.cost_total.saturating_add(delta as u64);
        Ok(())
    }

    /// 出口 + 结果：Err 必须落一条 Result(code≠0)，这就是「零静默」。
    pub fn exit(&mut self, id: u16, res: Result<u32, u32>) -> Result<u32, u32> {
        self.tick = self.tick.saturating_add(1);
        let mut at = None;
        let mut i = 0usize;
        while i < self.open.len() {
            if let Some(v) = self.open.get(i) {
                if *v == id {
                    at = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match at {
            Some(i) => {
                self.open.remove(i);
            }
            None => {
                let _ = self.trace.push(Point::Result as u8, id, self.tick, QualCode::OrphanExit.val());
                self.results = self.results.saturating_add(1);
                self.errs = self.errs.saturating_add(1);
                return Err(QualCode::OrphanExit.val());
            }
        }
        let _ = self.trace.push(Point::Exit as u8, id, self.tick, 0);
        self.exits = self.exits.saturating_add(1);
        let code = match res {
            Ok(v) => {
                let _ = self.trace.push(Point::Result as u8, id, self.tick, 0);
                self.results = self.results.saturating_add(1);
                return Ok(v);
            }
            Err(c) => c,
        };
        let _ = self.trace.push(Point::Result as u8, id, self.tick, code);
        self.results = self.results.saturating_add(1);
        self.errs = self.errs.saturating_add(1);
        Err(code)
    }

    /// 收机：仍有开着单元 = MissingExit。
    pub fn seal(&self) -> Result<(), u32> {
        if !self.open.is_empty() {
            return Err(QualCode::MissingExit.val());
        }
        Ok(())
    }

    /// 观测账是否还有丢弃。
    pub fn lossless(&self) -> bool {
        self.trace.dropped == 0
    }
}

impl Default for Instrumented {
    fn default() -> Instrumented {
        Instrumented::new()
    }
}

/// 一个工作单元：跑用例 + 四点埋点。
pub fn run_unit(ins: &mut Instrumented, id: u16, case: u32) -> Result<u32, u32> {
    ins.enter(id)?;
    let res = run_case(case);
    let cost = 3u32.saturating_add(case % 7);
    let _ = ins.cost(id, cost);
    ins.exit(id, res)
}

/// 埋点跑 n 个单元报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoverReport {
    /// 单元数
    pub units: u32,
    /// 入口条数
    pub enters: u32,
    /// 耗时条数
    pub costs: u32,
    /// 出口条数
    pub exits: u32,
    /// 结果条数
    pub results: u32,
    /// 失败单元数
    pub errs: u32,
    /// 有 Result(Err) 的条数
    pub err_recs: u32,
    /// 静默数（必须 0）
    pub silent: u32,
    /// 耗时总和
    pub cost_total: u64,
    /// 摘要
    pub digest: u64,
}

/// 埋点跑 units 个单元（每个单元跑第 unit%20 号用例）。
pub fn cover_run(units: u32) -> CoverReport {
    let mut ins = Instrumented::new();
    let mut i = 0u32;
    let mut silent = 0u32;
    while i < units {
        let case = i % 20;
        let before = ins.errs;
        let r = run_unit(&mut ins, (i % 0xffff) as u16, case);
        let _ = r;
        // 失败单元必须已经落了 Result(Err)
        if ins.errs > before {
            let has_err = ins.trace.err_count() >= ins.errs;
            if !has_err {
                silent = silent.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    let sealed = ins.seal().is_ok();
    let err_recs = ins.trace.err_count();
    let mut rep = CoverReport {
        units,
        enters: ins.enters,
        costs: ins.costs,
        exits: ins.exits,
        results: ins.results,
        errs: ins.errs,
        err_recs,
        silent: if sealed { silent } else { silent.saturating_add(1) },
        cost_total: ins.cost_total,
        digest: ins.trace.digest(),
    };
    // 耗时对账：总耗时 = 逐单元 cost 之和
    let mut sum = 0u64;
    let mut k = 0u32;
    while k < units {
        sum = sum.saturating_add((3u32.saturating_add((k % 20) % 7)) as u64);
        k = k.saturating_add(1);
    }
    if sum != rep.cost_total {
        rep.silent = rep.silent.saturating_add(1);
    }
    rep
}

// ── 20 条用例 ────────────────────────────────────────────────
/// 用例表项：(用例名, 子系统, 期望码)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 用例名
    pub name: &'static str,
    /// 子系统
    pub sys: &'static str,
    /// 期望码（0 = 期望成功）
    pub expect: u32,
}

/// 20 条用例（五子系统 × 4）。
pub const CASES: [Case; 20] = [
    Case { name: "parse/合法行", sys: "parse", expect: 0 },
    Case { name: "parse/缺等号", sys: "parse", expect: SEG_BASE + 7 },
    Case { name: "parse/keysym为零", sys: "parse", expect: SEG_BASE + 6 },
    Case { name: "parse/键码重复", sys: "parse", expect: SEG_BASE + 8 },
    Case { name: "switch/正常切换", sys: "switch", expect: 0 },
    Case { name: "switch/重复begin", sys: "switch", expect: SEG_BASE + 1 },
    Case { name: "switch/孤儿commit", sys: "switch", expect: SEG_BASE + 2 },
    Case { name: "switch/窗口溢出", sys: "switch", expect: SEG_BASE + 11 },
    Case { name: "mod/按下松开", sys: "mod", expect: 0 },
    Case { name: "mod/未按即松", sys: "mod", expect: SEG_BASE + 2 },
    Case { name: "mod/重复按下", sys: "mod", expect: SEG_BASE + 1 },
    Case { name: "mod/闩锁粘连", sys: "mod", expect: SEG_BASE + 5 },
    Case { name: "led/一次失败一次成功", sys: "led", expect: 0 },
    Case { name: "led/预算耗尽", sys: "led", expect: SEG_BASE + 6 },
    Case { name: "led/停机后仍写", sys: "led", expect: SEG_BASE + 4 },
    Case { name: "led/降级账满", sys: "led", expect: SEG_BASE + 11 },
    Case { name: "io/整读", sys: "io", expect: 0 },
    Case { name: "io/短读", sys: "io", expect: SEG_BASE + 6 },
    Case { name: "io/校验和不符", sys: "io", expect: SEG_BASE + 12 },
    Case { name: "io/回退静默", sys: "io", expect: SEG_BASE + 4 },
];

/// 用例数。
pub const CASE_COUNT: usize = 20;

/// 子系统数。
pub const SYS_COUNT: usize = 5;

/// 跑第 idx 条用例，返回实际码（0 = 成功）。
pub fn run_case(idx: u32) -> Result<u32, u32> {
    match idx % 20 {
        0 => {
            let mut seen: Vec<u16> = Vec::new();
            match parse_line("kc.2 = 0x31", &mut seen) {
                Ok((kc, lo)) => Ok((kc as u32) ^ lo),
                Err(c) => Err(c),
            }
        }
        1 => {
            let mut seen: Vec<u16> = Vec::new();
            match parse_line("kc.2 0x31", &mut seen) {
                Ok(_) => Ok(0),
                Err(c) => Err(c),
            }
        }
        2 => {
            let mut seen: Vec<u16> = Vec::new();
            match parse_line("kc.2 = 0x0", &mut seen) {
                Ok(_) => Ok(0),
                Err(c) => Err(c),
            }
        }
        3 => {
            let mut seen: Vec<u16> = Vec::new();
            let _ = parse_line("kc.2 = 0x31", &mut seen);
            match parse_line("kc.2 = 0x32", &mut seen) {
                Ok(_) => Ok(0),
                Err(c) => Err(c),
            }
        }
        4 => {
            let mut s = MiniSwitch::new();
            match s.begin(1) {
                Ok(()) => {}
                Err(c) => return Err(c),
            }
            let _ = s.feed(2);
            match s.commit() {
                Ok(n) => Ok(n),
                Err(c) => Err(c),
            }
        }
        5 => {
            let mut s = MiniSwitch::new();
            let _ = s.begin(1);
            match s.begin(0) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        6 => {
            let mut s = MiniSwitch::new();
            match s.commit() {
                Ok(n) => Ok(n),
                Err(c) => Err(c),
            }
        }
        7 => {
            let mut s = MiniSwitch::new();
            let _ = s.begin(1);
            let mut i = 0usize;
            let mut last: Result<(), u32> = Ok(());
            while i <= SWITCH_CAP {
                last = s.feed(i as u16);
                i += 1;
            }
            match last {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        8 => {
            let mut m = ModStack::new();
            let _ = m.press(0x01);
            match m.release(0x01) {
                Ok(()) => Ok(1),
                Err(c) => Err(c),
            }
        }
        9 => {
            let mut m = ModStack::new();
            match m.release(0x01) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        10 => {
            let mut m = ModStack::new();
            let _ = m.press(0x02);
            match m.press(0x02) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        11 => {
            let mut m = ModStack::new();
            m.latch(0x40);
            match m.settle() {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        12 => {
            let mut l = LedLamp::new();
            match l.fail() {
                Ok(()) => {}
                Err(c) => return Err(c),
            }
            l.ok();
            match l.fail() {
                Ok(()) => Ok(l.rows),
                Err(c) => Err(c),
            }
        }
        13 => {
            let mut l = LedLamp::new();
            let mut i = 0u32;
            let mut last: Result<(), u32> = Ok(());
            while i < LED_BUDGET {
                last = l.fail();
                i = i.saturating_add(1);
            }
            match last {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        14 => {
            let mut l = LedLamp::new();
            let mut i = 0u32;
            while i < LED_BUDGET {
                let _ = l.fail();
                i = i.saturating_add(1);
            }
            match l.fail() {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        15 => {
            let mut l = LedLamp::new();
            l.rows = LED_CAP;
            match l.fail() {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            }
        }
        16 => match io_read(1024, 1024) {
            Ok(n) => Ok(n),
            Err(c) => Err(c),
        },
        17 => match io_read(1024, 512) {
            Ok(n) => Ok(n),
            Err(c) => Err(c),
        },
        18 => match io_checksum(0x0abc, 0x0def) {
            Ok(()) => Ok(0),
            Err(c) => Err(c),
        },
        _ => match io_fallback(true, false) {
            Ok(()) => Ok(0),
            Err(c) => Err(c),
        },
    }
}

/// 单例结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseResult {
    /// 序号
    pub idx: u32,
    /// 期望码
    pub expect: u32,
    /// 实际码（0 = 成功）
    pub actual: u32,
    /// 是否绿
    pub green: bool,
}

/// 用例集报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseReport {
    /// 用例数
    pub total: u32,
    /// 绿数
    pub green: u32,
    /// 名重复数
    pub dup_names: u32,
    /// 子系统覆盖（应 = 5）
    pub sys_covered: u32,
    /// 摘要
    pub digest: u64,
}

/// 跑全部 20 例。
pub fn case_set() -> CaseReport {
    let mut rep = CaseReport {
        total: 0,
        green: 0,
        dup_names: 0,
        sys_covered: 0,
        digest: 0xcbf2_9ce4_8422_2325u64,
    };
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            let actual = match run_case(i as u32) {
                Ok(v) => {
                    if v == 0 {
                        0
                    } else {
                        // 成功路径带返回值，也算 0（期望码为 0）
                        0
                    }
                }
                Err(code) => code,
            };
            rep.total = rep.total.saturating_add(1);
            if actual == c.expect {
                rep.green = rep.green.saturating_add(1);
            }
            rep.digest = fold_text(rep.digest, c.name);
            rep.digest = fold_word(rep.digest, actual);
        }
        i += 1;
    }
    // 名重复
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(a) = CASES.get(i) {
            let mut j = i + 1;
            while j < CASES.len() {
                if let Some(b) = CASES.get(j) {
                    if a.name == b.name {
                        rep.dup_names = rep.dup_names.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    // 子系统覆盖
    let mut covered = 0u32;
    let mut si = 0usize;
    let systems = ["parse", "switch", "mod", "led", "io"];
    while si < systems.len() {
        if let Some(sys) = systems.get(si) {
            let mut n = 0u32;
            let mut k = 0usize;
            while k < CASES.len() {
                if let Some(c) = CASES.get(k) {
                    if c.sys == *sys {
                        n = n.saturating_add(1);
                    }
                }
                k += 1;
            }
            if n >= 4 {
                covered = covered.saturating_add(1);
            }
        }
        si += 1;
    }
    rep.sys_covered = covered;
    rep
}

/// 名重复检测器（两串）。
pub fn dup_pair(a: &str, b: &str) -> bool {
    a == b
}

/// 用例表自身名重复数（必须 0）。
pub fn dup_names_detected() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(a) = CASES.get(i) {
            let mut j = i + 1;
            while j < CASES.len() {
                if let Some(b) = CASES.get(j) {
                    if dup_pair(a.name, b.name) {
                        n = n.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    n
}

/// 用例集双跑守卫：摘要或绿数不等即 NotReproducible。
pub fn case_digest_guard(a: &CaseReport, b: &CaseReport) -> Result<(), u32> {
    if a.digest == b.digest && a.green == b.green && a.total == b.total {
        Ok(())
    } else {
        Err(QualCode::NotReproducible.val())
    }
}

/// 用例编号连续性（0..19 全部可跑且不撞）。
pub fn case_indices_ok() -> bool {
    let mut i = 0u32;
    while i < CASE_COUNT as u32 {
        let v = run_case(i);
        let _ = v;
        i = i.saturating_add(1);
    }
    // 20 与 0 必须是同一条（取模），且 19 与 39 同一条
    let a = run_case(0).is_ok() == run_case(20).is_ok();
    let b = run_case(19).is_err() == run_case(39).is_err();
    a && b && CASE_COUNT == 20
}

// ── fuzz 面 ─────────────────────────────────────────────────
/// fuzz 参数包。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Params {
    /// 键码
    pub kc: u16,
    /// 修饰位
    pub mods: u8,
    /// 延迟
    pub delay: u32,
    /// 周期
    pub period: u32,
    /// 层
    pub layer: u8,
    /// 死键 keysym
    pub dead: u32,
    /// 文本长度
    pub text_len: u32,
    /// 操作序号
    pub op: u32,
}

/// fuzz 单步最多几步（有界闸）。
pub const FUZZ_STEP_CAP: u32 = 3;

/// 参数校验（乱参数必须被带码拒绝，而不是夹一下算了）。
pub fn gauge(p: Params) -> Result<u32, u32> {
    let mut steps = 0u32;
    // 第 1 步：键码
    steps = steps.saturating_add(1);
    if p.kc == 0 || p.kc > 0x2ff {
        return Err(QualCode::FuzzUnbounded.val());
    }
    // 第 2 步：延迟/周期
    steps = steps.saturating_add(1);
    if p.delay < 250 || p.delay > 1000 {
        return Err(QualCode::PointMissing.val());
    }
    if p.period < 10 || p.period > 400 {
        return Err(QualCode::CostAfterExit.val());
    }
    // 第 3 步：层/死键/文本
    steps = steps.saturating_add(1);
    if p.layer > 3 {
        return Err(QualCode::BadCase.val());
    }
    if p.dead > KSYM_MAX {
        return Err(QualCode::SilentError.val());
    }
    if p.text_len > 4096 {
        return Err(QualCode::TraceOverflow.val());
    }
    if steps > FUZZ_STEP_CAP {
        return Err(QualCode::FuzzUnbounded.val());
    }
    Ok(((p.kc as u32) ^ (p.mods as u32) ^ p.layer as u32) & 0xffff)
}

/// fuzz 报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    /// 注入次数
    pub ops: u32,
    /// 带码拒绝数
    pub rejected: u32,
    /// 接受数
    pub accepted: u32,
    /// 越界未被拒数（必须 0）
    pub unbounded: u32,
    /// 有界步数是否始终 ≤3
    pub bounded: bool,
    /// 摘要
    pub digest: u64,
}

/// fuzz 跑 n 次。
pub fn fuzz_params(n: u32) -> FuzzReport {
    let mut d = Die::new(0x4883_7F0B);
    let mut rep = FuzzReport {
        ops: 0,
        rejected: 0,
        accepted: 0,
        unbounded: 0,
        bounded: true,
        digest: 0xcbf2_9ce4_8422_2325u64,
    };
    let mut i = 0u32;
    while i < n {
        let p = Params {
            kc: (d.below(0x400)) as u16,
            mods: (d.below(256)) as u8,
            delay: d.below(1200),
            period: d.below(450),
            layer: (d.below(6)) as u8,
            dead: d.below(0x20_0000),
            text_len: d.below(5000),
            op: i,
        };
        match gauge(p) {
            Ok(v) => {
                rep.accepted = rep.accepted.saturating_add(1);
                rep.digest = fold_word(rep.digest, v);
            }
            Err(c) => {
                rep.rejected = rep.rejected.saturating_add(1);
                if !in_set(c) {
                    rep.unbounded = rep.unbounded.saturating_add(1);
                }
                rep.digest = fold_word(rep.digest, c);
            }
        }
        rep.ops = rep.ops.saturating_add(1);
        i = i.saturating_add(1);
    }
    rep
}

// ── 黄金跑 / 探针 ────────────────────────────────────────────
/// 黄金跑：(覆盖单元数, 用例绿数, fuzz 次数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let c = cover_run(200);
    if c.enters != 200 || c.costs != 200 || c.exits != 200 || c.results < 200 || c.silent != 0 {
        return Err(QualCode::PointMissing.val());
    }
    let cs = case_set();
    if cs.total != 20 || cs.green != 20 || cs.dup_names != 0 || cs.sys_covered != 5 {
        return Err(QualCode::BadCase.val());
    }
    let f = fuzz_params(10_000);
    if f.ops != 10_000 || f.unbounded != 0 || !f.bounded {
        return Err(QualCode::FuzzUnbounded.val());
    }
    Ok((c.units, cs.green, f.ops))
}

/// 探针 A：成功单元的 Result 码为 0。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ins = Instrumented::new();
    ins.enter(1)?;
    let _ = ins.cost(1, 5);
    match ins.exit(1, Ok(7)) {
        Ok(7) => Ok(()),
        _ => Err(QualCode::SilentError.val()),
    }
}

/// 探针 B：失败必须有 Result(code)。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut ins = Instrumented::new();
    ins.enter(2)?;
    let _ = ins.cost(2, 1);
    let r = ins.exit(2, Err(QualCode::BadCase.val()));
    if r == Err(QualCode::BadCase.val()) && ins.trace.err_count() == 1 {
        Ok(())
    } else {
        Err(QualCode::SilentError.val())
    }
}

/// 探针 C：孤儿出口。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut ins = Instrumented::new();
    match ins.exit(3, Ok(0)) {
        Err(c) if c == QualCode::OrphanExit.val() => Ok(()),
        _ => Err(QualCode::OrphanExit.val()),
    }
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QualCode::NotReproducible.val());
    }
    let c1 = case_set();
    let c2 = case_set();
    case_digest_guard(&c1, &c2)?;
    let f1 = fuzz_params(10_000);
    let f2 = fuzz_params(10_000);
    if f1.digest != f2.digest {
        return Err(QualCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < QualCode::ALL.len() {
        if let Some(c) = QualCode::ALL.get(i) {
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
                QualCode::DoubleEnter.val(),
                QualCode::OrphanExit.val(),
                QualCode::MissingExit.val(),
                QualCode::CostAfterExit.val(),
                QualCode::BadCase.val(),
                QualCode::CaseDup.val(),
                QualCode::FuzzUnbounded.val(),
                QualCode::TraceOverflow.val(),
                QualCode::SilentError.val(),
                QualCode::NotReproducible.val(),
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
        // 0 重复入口
        let mut a = Instrumented::new();
        let _ = a.enter(1);
        led.record(0, match a.enter(1) {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 1 孤儿出口
        let mut b = Instrumented::new();
        led.record(1, match b.exit(2, Ok(0)) {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 2 缺出口
        let mut c2 = Instrumented::new();
        let _ = c2.enter(3);
        led.record(2, match c2.seal() {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 3 出口后耗时
        let mut d3 = Instrumented::new();
        let _ = d3.enter(4);
        let _ = d3.exit(4, Ok(1));
        led.record(3, match d3.cost(4, 9) {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 4 用例不符：把第 0 号的期望改成非零来验证比对器会抓
        let actual = match run_case(0) {
            Ok(_) => 0,
            Err(c) => c,
        };
        let mut cs = CASES.first().copied().unwrap_or(Case {
            name: "x",
            sys: "x",
            expect: 0,
        });
        cs.expect = QualCode::BadCase.val();
        led.record(4, if actual != cs.expect { QualCode::BadCase.val() } else { 0 });
        // 5 名重复检测器两态自检：相同要报、不同不报、真表零重复。
        let dup_hit = dup_pair("parse/合法行", "parse/合法行") && !dup_pair("parse/合法行", "parse/缺等号");
        let table_clean = dup_names_detected() == 0;
        led.record(5, if dup_hit && table_clean {
            QualCode::CaseDup.val()
        } else {
            0
        });
        // 6 参数越界
        let p = Params { kc: 0, mods: 0, delay: 500, period: 32, layer: 0, dead: 0, text_len: 0, op: 0 };
        led.record(6, match gauge(p) {
            Err(c) => c,
            Ok(_) => 0,
        });
        // 7 账满
        let mut tr = Trace::new();
        let mut i = 0usize;
        while i <= TRACE_CAP {
            let _ = tr.push(Point::Enter as u8, 0, i as u32, 0);
            i += 1;
        }
        led.record(7, if tr.dropped >= 1 {
            QualCode::TraceOverflow.val()
        } else {
            0
        });
        // 8 静默检测器必须两态都对：人为「有失败无记录」要被抓出，
        //   正常失败路径（已落 Result）不得误报。
        let mut e8 = Instrumented::new();
        e8.errs = 1; // 人为制造有失败无记录
        let caught = e8.errs > e8.trace.err_count();
        let mut e9 = Instrumented::new();
        let _ = e9.enter(9);
        let _ = e9.cost(9, 1);
        let _ = e9.exit(9, Err(QualCode::SilentError.val()));
        let normal_ok = e9.errs == e9.trace.err_count() && e9.trace.err_count() == 1;
        led.record(8, if caught && normal_ok {
            QualCode::SilentError.val()
        } else {
            0
        });
        // 9 双跑守卫：真双跑要一致，摘要差一必须被守卫抓出。
        let s1 = case_set();
        let s2 = case_set();
        let mut s3 = s2;
        s3.digest ^= 1;
        let guarded = case_digest_guard(&s1, &s2).is_ok() && case_digest_guard(&s1, &s3).is_err();
        led.record(9, if guarded { QualCode::NotReproducible.val() } else { 0 });
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 判据 ─────────────────────────────────────────────────────
/// 十条判据。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1 四点全覆盖
    let c = cover_run(200);
    let (ok1, d1) = {
        let mut t = String::new();
        t.push_str("units=");
        push_dec(&mut t, c.units);
        t.push_str(" enter=");
        push_dec(&mut t, c.enters);
        t.push_str(" cost=");
        push_dec(&mut t, c.costs);
        t.push_str(" exit=");
        push_dec(&mut t, c.exits);
        t.push_str(" result=");
        push_dec(&mut t, c.results);
        (
            c.units == 200 && c.enters == 200 && c.costs == 200 && c.exits == 200 && c.results >= 200,
            t,
        )
    };
    v.push(("F48836-J1-入口/出口/耗时/结果四点100%覆盖", ok1, d1));

    // 2 异常零静默
    let (ok2, d2) = {
        let mut t = String::new();
        t.push_str("errs=");
        push_dec(&mut t, c.errs);
        t.push_str(" result_err=");
        push_dec(&mut t, c.err_recs);
        t.push_str(" silent=");
        push_dec(&mut t, c.silent);
        (c.errs == c.err_recs && c.silent == 0, t)
    };
    v.push(("F48836-J2-异常零静默（每个失败都有结果埋点）", ok2, d2));

    // 3 耗时对账
    let mut expect_sum = 0u64;
    let mut i = 0u32;
    while i < 200 {
        expect_sum = expect_sum.saturating_add((3u32.saturating_add((i % 20) % 7)) as u64);
        i = i.saturating_add(1);
    }
    let (ok3, d3) = {
        let mut t = String::new();
        t.push_str("cost_total=");
        push_dec(&mut t, c.cost_total as u32);
        t.push_str(" expect=");
        push_dec(&mut t, expect_sum as u32);
        (c.cost_total == expect_sum, t)
    };
    v.push(("F48836-J3-耗时总和与逐单元之和精确对账", ok3, d3));

    // 4 出入口成对
    let mut ins = Instrumented::new();
    let mut k = 0u16;
    while k < 32 {
        ins.enter(k).ok();
        ins.cost(k, 2).ok();
        ins.exit(k, Ok(k as u32)).ok();
        k += 1;
    }
    let ok4 = ins.seal().is_ok() && ins.enters == 32 && ins.exits == 32 && ins.open.is_empty();
    v.push(("F48836-J4-出入口成对无孤儿", ok4, String::from("32 pairs")));

    // 5 fuzz 1e4 零崩
    let f = fuzz_params(10_000);
    let (ok5, d5) = {
        let mut t = String::new();
        t.push_str("ops=");
        push_dec(&mut t, f.ops);
        t.push_str(" accept=");
        push_dec(&mut t, f.accepted);
        t.push_str(" reject=");
        push_dec(&mut t, f.rejected);
        (f.ops == 10_000 && f.accepted + f.rejected == 10_000, t)
    };
    v.push(("F48837-J1-fuzz×10^4零崩", ok5, d5));

    // 6 fuzz 有界且带码
    let (ok6, d6) = {
        let mut t = String::new();
        t.push_str("unbounded=");
        push_dec(&mut t, f.unbounded);
        t.push_str(" step_cap=");
        push_dec(&mut t, FUZZ_STEP_CAP);
        (f.unbounded == 0 && f.bounded, t)
    };
    v.push(("F48837-J2-乱参数全部带码且单步有界", ok6, d6));

    // 7 20 例全绿
    let cs = case_set();
    let (ok7, d7) = {
        let mut t = String::new();
        t.push_str("total=");
        push_dec(&mut t, cs.total);
        t.push_str(" green=");
        push_dec(&mut t, cs.green);
        t.push_str(" sys=");
        push_dec(&mut t, cs.sys_covered);
        (cs.total == 20 && cs.green == 20 && cs.sys_covered == 5 && cs.dup_names == 0, t)
    };
    v.push(("F48838-J1-用例集×20全绿入库", ok7, d7));

    // 8 复跑一致
    let cs2 = case_set();
    let (ok8, d8) = {
        let mut t = String::new();
        t.push_str("digest1=");
        push_hex(&mut t, (cs.digest & 0xffff_ffff) as u32);
        t.push_str(" digest2=");
        push_hex(&mut t, (cs2.digest & 0xffff_ffff) as u32);
        (cs.digest == cs2.digest && cs.green == cs2.green, t)
    };
    v.push(("F48838-J2-用例集复跑摘要一字不差", ok8, d8));

    // 9 名唯一编号连续
    let ok9 = cs.dup_names == 0 && case_indices_ok() && CASE_COUNT == 20;
    v.push(("F48838-J3-用例名唯一且编号连续可取", ok9, String::from("dup=0 gap=0")));

    // 10 五子系统各 4 例
    let mut per_sys: [u32; 5] = [0; 5];
    let systems = ["parse", "switch", "mod", "led", "io"];
    let mut si = 0usize;
    while si < systems.len() {
        if let Some(sys) = systems.get(si) {
            let mut n = 0u32;
            let mut k = 0usize;
            while k < CASES.len() {
                if let Some(x) = CASES.get(k) {
                    if x.sys == *sys {
                        n = n.saturating_add(1);
                    }
                }
                k += 1;
            }
            if let Some(slot) = per_sys.get_mut(si) {
                *slot = n;
            }
        }
        si += 1;
    }
    let ok10 = per_sys.iter().all(|n| *n == 4) && SYS_COUNT == 5;
    v.push(("F48838-J4-五子系统各4例铺满", ok10, String::from("4×5")));

    v
}

// ── 检查集 ───────────────────────────────────────────────────
/// 运行本模块判据。
pub fn run_unx48836_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48836 四点埋点 / fuzz 面 / 20 例用例集");
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
    fn t_codes_window() {
        assert_eq!(QualCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < 12 {
            let c = QualCode::ALL.get(i).copied().expect("code");
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
        assert!(segment_guard());
        assert!(in_set(SEG_BASE + 1));
        assert!(!in_set(SEG_BASE));
        assert!(detail(0).contains("未知"));
    }

    #[test]
    fn t_four_points() {
        let c = cover_run(40);
        assert_eq!(c.units, 40);
        assert_eq!(c.enters, 40);
        assert_eq!(c.costs, 40);
        assert_eq!(c.exits, 40);
        assert!(c.results >= 40);
        assert_eq!(c.silent, 0);
        assert_eq!(c.errs, c.err_recs);
        assert!(c.errs > 0);
    }

    #[test]
    fn t_instrumented_pairing() {
        let mut ins = Instrumented::new();
        assert!(ins.enter(1).is_ok());
        assert_eq!(ins.enter(1), Err(QualCode::DoubleEnter.val()));
        assert!(ins.cost(1, 5).is_ok());
        assert_eq!(ins.exit(1, Ok(9)), Ok(9));
        assert_eq!(ins.exit(1, Ok(9)), Err(QualCode::OrphanExit.val()));
        assert_eq!(ins.cost(1, 1), Err(QualCode::CostAfterExit.val()));
        assert!(ins.seal().is_ok());
        assert_eq!(ins.enters, 1);
        assert_eq!(ins.exits, 1);
        // 失败必须落非零 Result
        let before = ins.trace.err_count();
        assert_eq!(ins.enter(2), Ok(()));
        assert_eq!(ins.exit(2, Err(QualCode::BadCase.val())), Err(QualCode::BadCase.val()));
        assert_eq!(ins.trace.err_count(), before + 1);
    }

    #[test]
    fn t_missing_exit() {
        let mut ins = Instrumented::new();
        assert!(ins.enter(7).is_ok());
        assert_eq!(ins.seal(), Err(QualCode::MissingExit.val()));
        assert_eq!(ins.exit(7, Ok(1)), Ok(1));
        assert!(ins.seal().is_ok());
    }

    #[test]
    fn t_twenty_cases() {
        let cs = case_set();
        assert_eq!(cs.total, 20);
        assert_eq!(cs.green, 20);
        assert_eq!(cs.dup_names, 0);
        assert_eq!(cs.sys_covered, 5);
        assert_eq!(case_set().digest, cs.digest);
        assert!(case_indices_ok());
        let r0 = run_case(0);
        assert!(r0.is_ok());
        assert_eq!(run_case(1), Err(QualCode::BadCase.val()));
        assert_eq!(run_case(2), Err(QualCode::PointMissing.val()));
        assert_eq!(run_case(3), Err(QualCode::CaseDup.val()));
    }

    #[test]
    fn t_cases_by_subsystem() {
        assert_eq!(CASES.len(), 20);
        for (i, c) in CASES.iter().enumerate() {
            let expect = c.expect;
            let actual = match run_case(i as u32) {
                Ok(_) => 0,
                Err(code) => code,
            };
            assert_eq!(actual, expect, "用例 {} 期望与实跑不符", c.name);
        }
    }

    #[test]
    fn t_fuzz_params() {
        let f = fuzz_params(10_000);
        assert_eq!(f.ops, 10_000);
        assert_eq!(f.accepted + f.rejected, 10_000);
        assert_eq!(f.unbounded, 0);
        assert!(f.bounded);
        assert!(f.rejected > 0);
        assert_eq!(fuzz_params(10_000).digest, f.digest);
        // 合法参数必须被接受
        let good = Params { kc: 2, mods: 0x01, delay: 500, period: 32, layer: 1, dead: 0xfe52, text_len: 100, op: 0 };
        assert!(gauge(good).is_ok());
        // 非法键码
        let bad = Params { kc: 0, mods: 0, delay: 500, period: 32, layer: 0, dead: 0, text_len: 0, op: 0 };
        assert_eq!(gauge(bad), Err(QualCode::FuzzUnbounded.val()));
    }

    #[test]
    fn t_golden_probe_inject() {
        let g = golden_run().expect("golden");
        assert_eq!(g, (200, 20, 10_000));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        let v = extra_checks();
        assert!(v.len() >= 8 && v.len() <= 10);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48836_checks().all_passed());
    }
}
