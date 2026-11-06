//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0504 行缓冲编辑（退格/回车） · 0x76240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定）：
//!   行缓冲 80 列：退格删尾（空行退格显性拒），回车提交（提交后缓冲
//!   清零留痕账），越界输入显性拒，双跑一致。
//! 设计要点：
//!   1. 行缓冲 80 列闭集（LINE_COLS=80 冻结）：字符键入列号 0..80，
//!      第 81 列起显性拒（ColOver），缓冲内容可逐列复算；
//!   2. 退格删尾：删的是缓冲尾字符（len-1），缓冲空时退格显性拒
//!      （BackspaceOnEmpty），不许静默吞键；
//!   3. 回车提交：提交行入历史账，缓冲清零（len=0），提交事件落账；
//!   4. 历史容量 16 冻结（HIST_CAP=16）：溢出丢最旧一行，守恒式
//!      kept + dropped ≡ pushed 逐账可复算；
//!   5. 会话状态机单调：预备 → 录入 → 封账，封账后输入/回退显性拒；
//!   6. 同一键序双跑（重放）结果与缓冲、历史账、守恒式三面全等。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76240，AI-VX7-1104 批7 O 组独占区）。
pub const SEG_BASE: u32 = 0x76240;

/// 行缓冲列数（80 列冻结，终端口径）。
pub const LINE_COLS: usize = 80;
/// 历史账容量（16 冻结）。
pub const HIST_CAP: usize = 16;
/// 可打印 ASCII 下界（0x20 空格）。
pub const PRINT_LO: u8 = 0x20;
/// 可打印 ASCII 上界（0x7E 波浪）。
pub const PRINT_HI: u8 = 0x7E;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入路数（越界列/空行退格/非打印键/封账后输入 四路）。
pub const INJ_ROUTES: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineCode {
    /// 越界输入：第 81 列起显性拒
    ColOver,
    /// 空行退格：缓冲空无尾可删
    BackspaceOnEmpty,
    /// 非打印键：0x20..=0x7E 之外字节不入行缓冲
    NonPrintable,
    /// 状态机倒退/越权：封账后仍收键
    StateBackward,
    /// 历史守恒破：kept+dropped ≠ pushed
    HistInvariant,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl LineCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LineCode::ColOver => "越界输入：第 81 列起拒，建议先回车提交再起新行",
            LineCode::BackspaceOnEmpty => "空行退格：缓冲已空无尾可删，建议先入字符再退格",
            LineCode::NonPrintable => "非打印键：仅收 0x20..=0x7E，建议改用控制键 API",
            LineCode::StateBackward => "封账后收键：会话已封账，建议新开编辑现场",
            LineCode::HistInvariant => "历史守恒破：kept+dropped 必须等于 pushed，建议核对丢最旧路径",
            LineCode::NotReproducible => "双跑不一致：同键序重放必须同结果，建议固定键序后对拍",
        }
    }
    pub const ALL: [LineCode; 6] = [
        LineCode::ColOver,
        LineCode::BackspaceOnEmpty,
        LineCode::NonPrintable,
        LineCode::StateBackward,
        LineCode::HistInvariant,
        LineCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LineCode::ALL.len() {
        if let Some(c) = LineCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76240 段闭集内"
}

// ── 会话状态机（单调：预备 → 录入 → 封账，倒退显性拒） ───────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    /// 预备（尚无任何键入）
    Ready,
    /// 录入（已有键入/提交）
    Input,
    /// 封账（终态，禁再收键）
    Sealed,
}

impl SessionPhase {
    fn ordinal(self) -> u8 {
        match self {
            SessionPhase::Ready => 0,
            SessionPhase::Input => 1,
            SessionPhase::Sealed => 2,
        }
    }
}

// ── 键闭集 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// 退格删尾
    Backspace,
    /// 回车提交
    Enter,
    /// 可打印字符（0x20..=0x7E）
    Char(u8),
}

// ── 提交动作账行 ────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitRow {
    pub seq: u64,
    pub text: String,
    pub cols: usize,
}

/// 行缓冲编辑器。
#[derive(Clone, Debug)]
pub struct LineEdit {
    /// 行缓冲（列号 0..LINE_COLS）。
    buf: [u8; LINE_COLS],
    /// 当前列数（0..=LINE_COLS）。
    len: usize,
    /// 历史账（容量 HIST_CAP 冻结，溢出丢最旧）。
    hist: Vec<String>,
    /// 累计提交行数（pushed）。
    pushed: u32,
    /// 累计丢弃最旧行数（dropped）。
    dropped: u32,
    /// 会话状态（单调）。
    phase: SessionPhase,
    /// 提交账（只追加）。
    pub rows: Vec<CommitRow>,
    row_seq: u64,
}

impl LineEdit {
    pub fn new() -> LineEdit {
        LineEdit {
            buf: [0u8; LINE_COLS],
            len: 0,
            hist: Vec::new(),
            pushed: 0,
            dropped: 0,
            phase: SessionPhase::Ready,
            rows: Vec::new(),
            row_seq: 0,
        }
    }

    /// 当前缓冲内容快照（String，可复算面）。
    pub fn line(&self) -> String {
        let mut s = String::new();
        let mut i = 0;
        while i < self.len {
            if let Some(&b) = self.buf.get(i) {
                s.push(b as char);
            }
            i += 1;
        }
        s
    }

    pub fn cols(&self) -> usize {
        self.len
    }

    pub fn kept(&self) -> usize {
        self.hist.len()
    }

    pub fn pushed_count(&self) -> u32 {
        self.pushed
    }

    pub fn dropped_count(&self) -> u32 {
        self.dropped
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    /// 历史账第 idx 行（越界 None）。
    pub fn hist_at(&self, idx: usize) -> Option<&str> {
        match self.hist.get(idx) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 守恒式：kept + dropped ≡ pushed。
    pub fn hist_invariant_ok(&self) -> bool {
        (self.hist.len() as u64) + (self.dropped as u64) == (self.pushed as u64)
    }

    fn bump_phase_on_key(&mut self) -> Result<(), u32> {
        match self.phase {
            SessionPhase::Ready => {
                self.phase = SessionPhase::Input;
                Ok(())
            }
            SessionPhase::Input => Ok(()),
            SessionPhase::Sealed => Err(LineCode::StateBackward.val()),
        }
    }

    /// 键入：字符/退格/回车 三键闭环（判据主路径）。
    pub fn feed(&mut self, key: Key) -> Result<Action, u32> {
        // 封账后一律拒（含回车）。
        if self.phase == SessionPhase::Sealed {
            return Err(LineCode::StateBackward.val());
        }
        match key {
            Key::Char(c) => {
                if c < PRINT_LO || c > PRINT_HI {
                    return Err(LineCode::NonPrintable.val());
                }
                if self.len >= LINE_COLS {
                    return Err(LineCode::ColOver.val());
                }
                self.bump_phase_on_key()?;
                match self.buf.get_mut(self.len) {
                    Some(slot) => *slot = c,
                    None => return Err(LineCode::ColOver.val()),
                }
                self.len += 1;
                Ok(Action::Typed)
            }
            Key::Backspace => {
                if self.len == 0 {
                    return Err(LineCode::BackspaceOnEmpty.val());
                }
                self.bump_phase_on_key()?;
                if let Some(slot) = self.buf.get_mut(self.len - 1) {
                    *slot = 0;
                }
                self.len -= 1;
                Ok(Action::Deleted)
            }
            Key::Enter => {
                self.bump_phase_on_key()?;
                self.commit()
            }
        }
    }

    /// 回车提交：入历史账 + 缓冲清零 + 落提交账（溢出丢最旧）。
    fn commit(&mut self) -> Result<Action, u32> {
        let text = self.line();
        self.hist.push(text.clone());
        self.pushed += 1;
        // 容量 16 冻结：溢出丢最旧一行（先取首行存在性，再移除）。
        while self.hist.len() > HIST_CAP {
            if self.hist.first().is_none() {
                break;
            }
            let old = self.hist.remove(0);
            let _ = old;
            self.dropped += 1;
        }
        // 缓冲清零。
        let mut i = 0;
        while i < LINE_COLS {
            if let Some(slot) = self.buf.get_mut(i) {
                *slot = 0;
            }
            i += 1;
        }
        self.len = 0;
        self.row_seq += 1;
        self.rows.push(CommitRow {
            seq: self.row_seq,
            text,
            cols: self.cols(),
        });
        if !self.hist_invariant_ok() {
            return Err(LineCode::HistInvariant.val());
        }
        Ok(Action::Committed)
    }

    /// 封账（单调终态）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == SessionPhase::Sealed {
            return Err(LineCode::StateBackward.val());
        }
        self.phase = SessionPhase::Sealed;
        Ok(())
    }

    /// 键序重放（双跑一致口径：同键序必同终态）。
    pub fn replay(keys: &[Key]) -> Result<(String, usize, u32, u32), u32> {
        let mut ed = LineEdit::new();
        for &k in keys {
            let _ = ed.feed(k)?;
        }
        Ok((ed.line(), ed.kept(), ed.pushed_count(), ed.dropped_count()))
    }
}

/// 键入动作结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 已入缓冲
    Typed,
    /// 已删尾
    Deleted,
    /// 已提交（缓冲清零）
    Committed,
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 演练键序（确定性语料：打字/退格/提交交错，含溢出） ───────────
pub fn drill_keys() -> Vec<Key> {
    let mut keys: Vec<Key> = Vec::new();
    // 第 1 行：打 "takeover" 后退 3 格再补 "run"，提交。
    let mut line1: Vec<Key> = Vec::new();
    for &b in b"takeover" {
        line1.push(Key::Char(b));
    }
    let mut i = 0;
    while i < 3 {
        line1.push(Key::Backspace);
        i += 1;
    }
    for &b in b"run" {
        line1.push(Key::Char(b));
    }
    line1.push(Key::Enter);
    keys.extend(line1);
    // 第 2..=21 行：带序号行（制造历史溢出：20 行 > 16 冻结容量）。
    let mut n = 1u32;
    while n <= 20 {
        let text = match n % 3 {
            0 => format!("line-{:02}-alpha", n),
            1 => format!("line-{:02}-beta", n),
            _ => format!("line-{:02}-gamma", n),
        };
        for &b in text.as_bytes() {
            keys.push(Key::Char(b));
        }
        keys.push(Key::Enter);
        n += 1;
    }
    keys
}

/// 溢出演练：21 行提交（1+20），历史封顶 16，丢最旧 5。
pub fn overflow_drill() -> Result<(usize, u32, u32), u32> {
    let mut ed = LineEdit::new();
    for &k in &drill_keys() {
        let _ = ed.feed(k)?;
    }
    if !ed.hist_invariant_ok() {
        return Err(LineCode::HistInvariant.val());
    }
    Ok((ed.kept(), ed.pushed_count(), ed.dropped_count()))
}

// ── 黄金全链：打字→退格→提交→溢出→四路注入全抓→重放一致 ─────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    // 正常流：第 1 行编辑。
    let mut ed = LineEdit::new();
    for &b in b"takeover" {
        ed.feed(Key::Char(b))?;
    }
    let mid = ed.line();
    let mut i = 0;
    while i < 3 {
        ed.feed(Key::Backspace)?;
        i += 1;
    }
    for &b in b"run" {
        ed.feed(Key::Char(b))?;
    }
    let final_line = ed.line();
    let edit_ok = (mid == "takeover" && final_line == "takeorun") as u32;
    ed.feed(Key::Enter)?;
    // 提交后缓冲清零留痕账。
    let commit_ok = (ed.cols() == 0 && ed.kept() == 1 && ed.rows.len() == 1) as u32;
    // 溢出演练。
    let (kept, pushed, dropped) = overflow_drill()?;
    let over_ok = (kept == HIST_CAP && dropped == pushed - kept as u32) as u32;
    // 四路注入 10 轮全抓。
    let mut caught = 0;
    let mut route = 0;
    while route < INJ_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            if inject_one(route)? {
                caught += 1;
            }
            r += 1;
        }
        route += 1;
    }
    Ok((edit_ok + commit_ok + over_ok, caught))
}

/// 单路注入：人为构造违约，必被拒为绿。
/// 路 0：第 81 列越界；路 1：空行退格；路 2：非打印键；路 3：封账后收键。
pub fn inject_one(route: usize) -> Result<bool, u32> {
    if route >= INJ_ROUTES {
        return Err(LineCode::NonPrintable.val());
    }
    let mut ed = LineEdit::new();
    let rejected = match route {
        0 => {
            // 填满 80 列后第 81 列必拒。
            let mut n = 0;
            while n < LINE_COLS {
                let _ = ed.feed(Key::Char(b'a'))?;
                n += 1;
            }
            ed.feed(Key::Char(b'b')).is_err()
        }
        1 => ed.feed(Key::Backspace).is_err(),
        2 => ed.feed(Key::Char(0x01)).is_err(),
        _ => {
            let _ = ed.feed(Key::Char(b'x'))?;
            ed.seal()?;
            // 封账后任一键（含回车）必拒。
            ed.feed(Key::Enter).is_err() && ed.feed(Key::Char(b'y')).is_err()
        }
    };
    Ok(rejected)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LineCode::NotReproducible.val());
    }
    // 键序重放双跑：终态三面（缓冲/历史/守恒数）全等。
    let r1 = LineEdit::replay(&drill_keys())?;
    let r2 = LineEdit::replay(&drill_keys())?;
    if r1 != r2 {
        return Err(LineCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金链 3 面绿 + 40 路注入全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 40)) => Ok(()),
        _ => Err(LineCode::NotReproducible.val()),
    }
}

/// 序 B：边界列——恰好 80 列可入、81 列拒、退到 79 可再入。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut ed = LineEdit::new();
    let mut n = 0;
    while n < LINE_COLS {
        if ed.feed(Key::Char(b'k')).is_err() {
            return Err(LineCode::ColOver.val());
        }
        n += 1;
    }
    if ed.feed(Key::Char(b'!')).is_ok() {
        return Err(LineCode::NotReproducible.val());
    }
    if ed.feed(Key::Backspace).is_err() {
        return Err(LineCode::NotReproducible.val());
    }
    if ed.feed(Key::Char(b'!')).is_err() {
        return Err(LineCode::NotReproducible.val());
    }
    Ok(())
}

/// 序 C：守恒冻结——HIST_CAP=16 满账后 kept 不再增长。
pub fn probe_seq_c() -> Result<(), u32> {
    let (kept, pushed, dropped) = overflow_drill()?;
    if kept != HIST_CAP || pushed != 21 || dropped != 5 {
        return Err(LineCode::HistInvariant.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < LineCode::ALL.len() {
        if let Some(c) = LineCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// J1：退格删尾 + 空行退格显性拒。
fn backspace_ok() -> bool {
    let mut ed = LineEdit::new();
    if ed.feed(Key::Backspace).is_ok() {
        return false;
    }
    let _ = ed.feed(Key::Char(b'a'));
    let _ = ed.feed(Key::Char(b'b'));
    if ed.cols() != 2 {
        return false;
    }
    if ed.feed(Key::Backspace).is_err() {
        return false;
    }
    ed.line() == "a"
}

/// J1：回车提交后缓冲清零且留痕账。
fn commit_ok() -> bool {
    let mut ed = LineEdit::new();
    for &b in b"commit-me" {
        if ed.feed(Key::Char(b)).is_err() {
            return false;
        }
    }
    match ed.feed(Key::Enter) {
        Ok(Action::Committed) => {}
        _ => return false,
    }
    ed.cols() == 0 && ed.kept() == 1 && ed.rows.len() == 1
}

/// J1：越界输入显性拒（含非打印键）。
fn over_input_ok() -> bool {
    let mut ed = LineEdit::new();
    let mut n = 0;
    while n < LINE_COLS {
        let _ = ed.feed(Key::Char(b'z'));
        n += 1;
    }
    if ed.feed(Key::Char(b'z')).is_ok() {
        return false;
    }
    if ed.feed(Key::Char(0x7F)).is_ok() {
        return false;
    }
    true
}

/// J1：历史守恒 kept+dropped ≡ pushed。
fn hist_ok() -> bool {
    match overflow_drill() {
        Ok((kept, pushed, dropped)) => {
            kept == HIST_CAP && (kept as u64) + (dropped as u64) == (pushed as u64)
        }
        Err(_) => false,
    }
}

/// J1：状态机单调（封账后收键拒 + 双重封账拒）。
fn phase_ok() -> bool {
    let mut ed = LineEdit::new();
    let _ = ed.feed(Key::Char(b's'));
    if ed.seal().is_err() {
        return false;
    }
    if ed.seal().is_ok() {
        return false;
    }
    if ed.feed(Key::Char(b't')).is_ok() {
        return false;
    }
    true
}

// ── 判据面（extra_checks：全 add_dyn） ──────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J0-段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X} 窗口 0x75000..=0x781C0 且 0x40 对齐", SEG_BASE),
    ));
    v.push((
        "J1-退格删尾",
        backspace_ok(),
        String::from("空行退格拒 + 删尾后缓冲复算一致"),
    ));
    v.push((
        "J1-回车提交",
        commit_ok(),
        String::from("提交后缓冲清零留痕账 1 行"),
    ));
    v.push((
        "J1-越界输入拒",
        over_input_ok(),
        String::from("第 81 列 + 非打印键 0x7F 显性拒"),
    ));
    v.push((
        "J1-历史守恒",
        hist_ok(),
        format!("容量{}冻结 溢出丢最旧 kept+dropped≡pushed", HIST_CAP),
    ));
    v.push((
        "J1-状态机单调",
        phase_ok(),
        String::from("预备→录入→封账 倒退显性拒"),
    ));
    // 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((faces, caught)) => *faces == 3 && *caught == 40,
            Err(_) => false,
        },
        match &g {
            Ok((faces, caught)) => format!("faces={} caught={}/40", faces, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 注入 4 路 10 轮。
    let mut caught_total = 0;
    let mut tries_total = 0;
    let mut inj_ok = true;
    let mut route = 0;
    while route < INJ_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            tries_total += 1;
            match inject_one(route) {
                Ok(true) => caught_total += 1,
                _ => inj_ok = false,
            }
            r += 1;
        }
        route += 1;
    }
    v.push((
        "J1-注入10/10",
        inj_ok && caught_total == tries_total,
        format!("t/c {}/{} 四路（81列/空退格/非打印/封账后）", caught_total, tries_total),
    ));
    // 边界列探针。
    v.push((
        "J2-边界列探针",
        probe_seq_b().is_ok(),
        String::from("80 可入 / 81 拒 / 退 79 再入"),
    ));
    // 溢出守恒探针。
    v.push((
        "J2-溢出守恒探针",
        probe_seq_c().is_ok(),
        String::from("21 提交 → 16 留 + 5 丢"),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 与键序重放两跑同值"),
    ));
    // 提交账行序连续。
    let mut ed = LineEdit::new();
    for &b in b"one" {
        let _ = ed.feed(Key::Char(b));
    }
    let _ = ed.feed(Key::Enter);
    for &b in b"two" {
        let _ = ed.feed(Key::Char(b));
    }
    let _ = ed.feed(Key::Enter);
    let seq_ok = match (ed.rows.get(0), ed.rows.get(1)) {
        (Some(r0), Some(r1)) => r0.seq == 1 && r1.seq == 2 && r1.text == "two",
        _ => false,
    };
    v.push((
        "J3-提交账行序",
        seq_ok,
        String::from("账行 seq 连续且文本可复算"),
    ));
    // 诊断码闭集。
    v.push((
        "J3-诊断码闭集",
        detail(SEG_BASE + 0x3F).starts_with("未知") && detail(LineCode::ColOver.val()) == LineCode::ColOver.detail(),
        String::from("闭集查表命中 + 窗口外未知"),
    ));
    v
}

pub fn run_unx0504_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0504 行缓冲编辑（退格/回车）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chain_all_green() {
        assert_eq!(golden_run(), Ok((3, 40)), "黄金链 3 面绿 + 40 抓");
    }

    #[test]
    fn backspace_empty_rejected_then_delete_tail() {
        let mut ed = LineEdit::new();
        assert_eq!(ed.feed(Key::Backspace), Err(LineCode::BackspaceOnEmpty.val()));
        let _ = ed.feed(Key::Char(b'h'));
        let _ = ed.feed(Key::Char(b'i'));
        let _ = ed.feed(Key::Char(b'!'));
        assert_eq!(ed.line(), "hi!");
        assert_eq!(ed.feed(Key::Backspace), Ok(Action::Deleted));
        assert_eq!(ed.line(), "hi");
        assert_eq!(ed.cols(), 2);
    }

    #[test]
    fn enter_clears_buffer_and_ledges() {
        let mut ed = LineEdit::new();
        for &b in b"first" {
            let _ = ed.feed(Key::Char(b));
        }
        assert_eq!(ed.feed(Key::Enter), Ok(Action::Committed));
        assert_eq!(ed.cols(), 0);
        assert_eq!(ed.line(), "");
        assert_eq!(ed.kept(), 1);
        assert_eq!(ed.pushed_count(), 1);
        assert_eq!(ed.hist_at(0), Some("first"));
    }

    #[test]
    fn column_81_rejected() {
        let mut ed = LineEdit::new();
        let mut n = 0;
        while n < LINE_COLS {
            assert!(ed.feed(Key::Char(b'#')).is_ok(), "第 {} 列应可入", n + 1);
            n += 1;
        }
        assert_eq!(ed.cols(), LINE_COLS);
        assert_eq!(ed.feed(Key::Char(b'#')), Err(LineCode::ColOver.val()));
    }

    #[test]
    fn hist_overflow_conserved() {
        let (kept, pushed, dropped) = overflow_drill().ok().unwrap_or((0, 0, 0));
        assert_eq!(kept, HIST_CAP, "历史封顶 16");
        assert_eq!(pushed, 21, "演练共 21 行提交");
        assert_eq!(dropped, 5, "溢出丢最旧 5 行");
        assert_eq!((kept as u64) + (dropped as u64), pushed as u64, "守恒式");
    }

    #[test]
    fn all_injection_routes_caught() {
        let mut route = 0;
        while route < INJ_ROUTES {
            let mut r = 0;
            while r < JUDGE_ROUNDS {
                match inject_one(route) {
                    Ok(true) => {}
                    other => panic!("注入路{}轮{} 必抓，实得 {:?}", route, r, other),
                }
                r += 1;
            }
            route += 1;
        }
    }

    #[test]
    fn replay_double_run_identical() {
        let keys = drill_keys();
        let a = LineEdit::replay(&keys);
        let b = LineEdit::replay(&keys);
        assert_eq!(a, b, "同键序双跑必须同终态");
        assert!(repro_check().is_ok());
    }

    #[test]
    fn session_phase_monotonic() {
        let mut ed = LineEdit::new();
        assert_eq!(ed.phase(), SessionPhase::Ready);
        let _ = ed.feed(Key::Char(b'a'));
        assert_eq!(ed.phase(), SessionPhase::Input);
        assert!(ed.seal().is_ok());
        assert_eq!(ed.phase(), SessionPhase::Sealed);
        assert_eq!(ed.seal(), Err(LineCode::StateBackward.val()));
        assert_eq!(ed.feed(Key::Char(b'b')), Err(LineCode::StateBackward.val()));
        assert_eq!(ed.feed(Key::Enter), Err(LineCode::StateBackward.val()));
        assert_eq!(ed.feed(Key::Backspace), Err(LineCode::StateBackward.val()));
    }

    #[test]
    fn non_printable_rejected() {
        let mut ed = LineEdit::new();
        assert_eq!(ed.feed(Key::Char(0x00)), Err(LineCode::NonPrintable.val()));
        assert_eq!(ed.feed(Key::Char(0x1F)), Err(LineCode::NonPrintable.val()));
        assert_eq!(ed.feed(Key::Char(0x80)), Err(LineCode::NonPrintable.val()));
        assert!(ed.feed(Key::Char(0x20)).is_ok(), "空格应可入");
        assert!(ed.feed(Key::Char(0x7E)).is_ok(), "波浪应可入");
    }

    #[test]
    fn segment_window_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut i = 0;
        while i < LineCode::ALL.len() {
            if let Some(c) = LineCode::ALL.get(i) {
                let v = c.val();
                assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知"));
    }
}
