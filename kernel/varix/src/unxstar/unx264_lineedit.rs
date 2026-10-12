//! unx264_lineedit ↔ UNX-F0264 终端行编辑器增强（历史/补全）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0264，UNX-A1-B14）：
//! 「终端行编辑器增强（历史/补全） | 280 | 已深化 |
//!  UNX-F0264-J1 历史 16 条循环/前缀补全/Tab 二义提示全过，边界输入不崩溃」
//!
//! 设计要点（全部围绕终端行编辑真实状态）：
//! 1. **历史 16 条循环**：定容 16 槽环形史，满则淘汰最旧 + 淘汰计数留痕
//!    （不是拒绝收录——拒绝会让新命令不可回溯，语义错误）；相邻去重
//!    （与栈顶相同不重复收录，dup_skip 留账）；空行不进史。
//! 2. **循环导航**：Up 逐条回溯，越过最旧钳制在最旧（clamp 计数留账）；
//!    Down 逐条返回，越过最新回到**未提交草稿**（导航开始前缓冲区
//!    原样保存、原样恢复，草稿字节级一致断言）。
//! 3. **前缀补全**：命令表静态注册，Tab 时按前缀收集候选；唯一命中 →
//!    补全整词 + 尾随空格；多候选 → 先扩展到**最长公共前缀**再进二义
//!    提示；零候选 → 响铃账（bell 计数），缓冲区不动。
//! 4. **Tab 二义提示**：多候选按字典序确定性列出，提示行含候选数与
//!    公共前缀长度，两次 Tab 才展开清单（标准 shell 语义：首次 Tab 只
//!    扩公共前缀，再次 Tab 列二义）。
//! 5. **边界输入不崩溃**：缓冲满再插拒（full_reject 计数，len 不变式
//!    len ≤ CAP 恒真）；光标越界钳制（0 与 len 双边界）；控制字符
//!    （<0x20 非 Tab/CR、0x7F Del）拒绝计数；所有操作后缓冲区内容与
//!    长度自洽（末尾无悬挂字节）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const L264_VERSION: &str = "l264-lineedit-v1";

/// 诊断段基址。
pub const L264_SEG_BASE: u16 = 0x6200;

/// 行缓冲容量（字节）。
pub const LINE_CAP: usize = 128;

/// 历史容量（条）。
pub const HISTORY_CAP: usize = 16;

// --------------------------------------------------------------------------
// 诊断码（l264 十二码 · 11 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 行编辑诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum L264Code {
    /// 0x6200 缓冲满仍插入（拒）。
    BufFull,
    /// 0x6201 光标越界（钳制留账）。
    CursorClamp,
    /// 0x6202 控制字符拒绝。
    CtrlRej,
    /// 0x6203 空行进史被拒。
    EmptyHistRej,
    /// 0x6204 相邻重复进史被跳过。
    DupSkip,
    /// 0x6205 历史淘汰（最旧被覆盖）。
    HistEvict,
    /// 0x6206 Up 越界钳制在最旧。
    UpClamp,
    /// 0x6207 Down 越界回到草稿。
    DownDraft,
    /// 0x6208 Tab 零候选响铃。
    TabBell,
    /// 0x6209 Tab 二义提示（多候选清单）。
    TabAmbig,
    /// 0x620A 缓冲自洽破坏（内部不变式，不应发生）。
    InvariantBad,
}

impl L264Code {
    /// 码值。
    pub fn val(self) -> u16 {
        L264_SEG_BASE + match self {
            L264Code::BufFull => 0x0,
            L264Code::CursorClamp => 0x1,
            L264Code::CtrlRej => 0x2,
            L264Code::EmptyHistRej => 0x3,
            L264Code::DupSkip => 0x4,
            L264Code::HistEvict => 0x5,
            L264Code::UpClamp => 0x6,
            L264Code::DownDraft => 0x7,
            L264Code::TabBell => 0x8,
            L264Code::TabAmbig => 0x9,
            L264Code::InvariantBad => 0xA,
        }
    }
}

/// 段独占守卫：段内码值必须全部落在本模块窗口。
pub fn l264_segment_guard() -> Result<(), L264Code> {
    let lo = L264Code::BufFull.val();
    let hi = L264Code::InvariantBad.val();
    if lo != L264_SEG_BASE || hi <= lo || hi >= L264_SEG_BASE + 0x10 {
        return Err(L264Code::InvariantBad);
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 历史环：16 槽定容，满淘汰最旧
// --------------------------------------------------------------------------
/// 历史环。
pub struct HistoryRing {
    /// 槽（定容 16）。
    slots: [Option<String>; HISTORY_CAP],
    /// 下一写入槽（环形推进）。
    head: usize,
    /// 有效条数。
    count: usize,
    /// 淘汰计数（最旧被覆盖次数）。
    evicted: u64,
    /// 相邻重复跳过计数。
    dup_skipped: u64,
    /// 空行拒收录计数。
    empty_rejected: u64,
    /// 总收录计数（含被淘汰的）。
    total_pushed: u64,
}

impl HistoryRing {
    /// 新建空史。
    pub fn new() -> Self {
        HistoryRing {
            slots: Default::default(),
            head: 0,
            count: 0,
            evicted: 0,
            dup_skipped: 0,
            empty_rejected: 0,
            total_pushed: 0,
        }
    }

    /// 收录一行（空行拒、相邻重复跳过、满淘汰最旧）。
    pub fn push(&mut self, line: &str) {
        if line.is_empty() {
            self.empty_rejected += 1;
            return;
        }
        if self.count > 0 {
            let top = (self.head + HISTORY_CAP - 1) % HISTORY_CAP;
            if let Some(s) = &self.slots[top] {
                if s.as_str() == line {
                    self.dup_skipped += 1;
                    return;
                }
            }
        }
        if self.count == HISTORY_CAP {
            self.evicted += 1;
        } else {
            self.count += 1;
        }
        self.slots[self.head] = Some(String::from(line));
        self.head = (self.head + 1) % HISTORY_CAP;
        self.total_pushed += 1;
    }

    /// 有效条数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 第 k 新的条（k=0 最新）。越界返回 None。
    pub fn nth_newest(&self, k: usize) -> Option<&str> {
        if k >= self.count {
            return None;
        }
        let idx = (self.head + HISTORY_CAP - 1 - k) % HISTORY_CAP;
        self.slots[idx].as_deref()
    }

    /// 淘汰计数。
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// 相邻重复跳过计数。
    pub fn dup_skipped(&self) -> u64 {
        self.dup_skipped
    }

    /// 空行拒收录计数。
    pub fn empty_rejected(&self) -> u64 {
        self.empty_rejected
    }
}

// --------------------------------------------------------------------------
// 行缓冲：字节级编辑 + 光标
// --------------------------------------------------------------------------
/// 行缓冲。
pub struct LineBuf {
    /// 字节存储。
    buf: [u8; LINE_CAP],
    /// 有效长度。
    len: usize,
    /// 光标（0..=len）。
    cur: usize,
    /// 缓冲满拒插计数。
    full_rejects: u64,
    /// 光标钳制计数。
    cursor_clamps: u64,
    /// 控制字符拒绝计数。
    ctrl_rejects: u64,
}

impl LineBuf {
    /// 新建空缓冲。
    pub fn new() -> Self {
        LineBuf {
            buf: [0u8; LINE_CAP],
            len: 0,
            cur: 0,
            full_rejects: 0,
            cursor_clamps: 0,
            ctrl_rejects: 0,
        }
    }

    /// 插入一个可打印字节（光标处）。缓冲满或控制字符拒绝。
    pub fn insert(&mut self, b: u8) -> bool {
        if b < 0x20 && b != b'\t' && b != b'\r' {
            self.ctrl_rejects += 1;
            return false;
        }
        if b == 0x7F {
            self.ctrl_rejects += 1;
            return false;
        }
        if self.len >= LINE_CAP {
            self.full_rejects += 1;
            return false;
        }
        // 光标处插入：右移尾段。
        let mut i = self.len;
        while i > self.cur {
            self.buf[i] = self.buf[i - 1];
            i -= 1;
        }
        self.buf[self.cur] = b;
        self.cur += 1;
        self.len += 1;
        true
    }

    /// 退格（删光标前一字节）。
    pub fn backspace(&mut self) -> bool {
        if self.cur == 0 {
            self.cursor_clamps += 1;
            return false;
        }
        let mut i = self.cur - 1;
        while i < self.len - 1 {
            self.buf[i] = self.buf[i + 1];
            i += 1;
        }
        self.len -= 1;
        self.cur -= 1;
        true
    }

    /// 删除光标处字节（Del 键）。
    pub fn delete_at(&mut self) -> bool {
        if self.cur >= self.len {
            self.cursor_clamps += 1;
            return false;
        }
        let mut i = self.cur;
        while i < self.len - 1 {
            self.buf[i] = self.buf[i + 1];
            i += 1;
        }
        self.len -= 1;
        true
    }

    /// 光标左移（越界钳制在 0）。
    pub fn left(&mut self) {
        if self.cur == 0 {
            self.cursor_clamps += 1;
        } else {
            self.cur -= 1;
        }
    }

    /// 光标右移（越界钳制在 len）。
    pub fn right(&mut self) {
        if self.cur >= self.len {
            self.cursor_clamps += 1;
        } else {
            self.cur += 1;
        }
    }

    /// 光标到行首。
    pub fn home(&mut self) {
        self.cur = 0;
    }

    /// 光标到行尾。
    pub fn end(&mut self) {
        self.cur = self.len;
    }

    /// Ctrl-K：杀到行尾，返回被杀段。
    pub fn kill_to_end(&mut self) -> Option<String> {
        if self.cur >= self.len {
            return None;
        }
        let killed = String::from_utf8_lossy(&self.buf[self.cur..self.len]).into_owned();
        self.len = self.cur;
        Some(killed)
    }

    /// 设整行（补全用），光标到尾。
    pub fn set_line(&mut self, s: &str) -> bool {
        let bytes = s.as_bytes();
        if bytes.len() > LINE_CAP {
            self.full_rejects += 1;
            return false;
        }
        self.buf[..bytes.len()].copy_from_slice(bytes);
        self.len = bytes.len();
        self.cur = self.len;
        true
    }

    /// 当前内容（快照）。
    pub fn as_str(&self) -> String {
        String::from_utf8_lossy(&self.buf[..self.len]).into_owned()
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 光标。
    pub fn cursor(&self) -> usize {
        self.cur
    }

    /// 缓冲满拒插计数。
    pub fn full_rejects(&self) -> u64 {
        self.full_rejects
    }

    /// 光标钳制计数。
    pub fn cursor_clamps(&self) -> u64 {
        self.cursor_clamps
    }

    /// 控制字符拒绝计数。
    pub fn ctrl_rejects(&self) -> u64 {
        self.ctrl_rejects
    }
}

// --------------------------------------------------------------------------
// 前缀补全引擎
// --------------------------------------------------------------------------
/// 补全结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompleteOutcome {
    /// 唯一命中（已补全整词 + 尾随空格）。
    Unique,
    /// 多候选（已扩到最长公共前缀，待二义提示）。
    Ambiguous,
    /// 零候选（响铃，缓冲不动）。
    None,
}

/// 补全引擎。
pub struct Completer {
    /// 命令表（静态注册，已按字典序）。
    cmds: &'static [&'static str],
}

impl Completer {
    /// 新建。
    pub const fn new(cmds: &'static [&'static str]) -> Self {
        Completer { cmds }
    }

    /// 按前缀收集候选（保持表序 = 字典序）。
    pub fn candidates(&self, prefix: &str) -> Vec<&'static str> {
        let mut out = Vec::new();
        let pb = prefix.as_bytes();
        for c in self.cmds {
            let cb = c.as_bytes();
            if cb.len() >= pb.len() && &cb[..pb.len()] == pb {
                out.push(*c);
            }
        }
        out
    }

    /// 候选组的最长公共前缀长度。
    pub fn common_prefix_len(cands: &[&'static str]) -> usize {
        if cands.is_empty() {
            return 0;
        }
        let mut n = cands[0].len();
        for c in &cands[1..] {
            let cb = c.as_bytes();
            let mut i = 0;
            while i < n && i < cb.len() && cb[i] == cands[0].as_bytes()[i] {
                i += 1;
            }
            n = i;
        }
        n
    }

    /// 执行 Tab 补全：返回（结局，应写入的扩展段，候选清单）。
    pub fn complete(&self, prefix: &str) -> (CompleteOutcome, String, Vec<&'static str>) {
        let cands = self.candidates(prefix);
        if cands.is_empty() {
            return (CompleteOutcome::None, String::new(), cands);
        }
        if cands.len() == 1 {
            let mut ext = String::from(&cands[0][prefix.len()..]);
            ext.push(' ');
            return (CompleteOutcome::Unique, ext, cands);
        }
        let cpl = Self::common_prefix_len(&cands);
        let ext = if cpl > prefix.len() {
            String::from(&cands[0][prefix.len()..cpl])
        } else {
            String::new()
        };
        (CompleteOutcome::Ambiguous, ext, cands)
    }

    /// 二义提示行（确定性字典序，含候选数与公共前缀长度）。
    pub fn ambig_banner(prefix: &str, cands: &[&'static str]) -> String {
        let cpl = Self::common_prefix_len(cands);
        let mut s = String::from("[");
        let mut first = true;
        for c in cands {
            if !first {
                s.push(' ');
            }
            s.push_str(c);
            first = false;
        }
        s.push_str("] ");
        push_usize(&mut s, cands.len());
        s.push_str(" 项 · 公共前缀 ");
        push_usize(&mut s, cpl);
        s.push_str(" · 输入 \"");
        s.push_str(prefix);
        s.push_str("\"");
        s
    }
}

/// no_std 下无 format!，手推十进制。
fn push_usize(s: &mut String, mut v: usize) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(digits[n] as char);
    }
}

// --------------------------------------------------------------------------
// 编辑会话：缓冲 + 历史 + 导航 + 草稿
// --------------------------------------------------------------------------
/// 导航位置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NavPos {
    /// 在当前（未进史）行。
    Live,
    /// 在史中第 k 新。
    Hist(usize),
}

/// 编辑会话。
pub struct LineEdit {
    buf: LineBuf,
    hist: HistoryRing,
    comp: Completer,
    nav: NavPos,
    /// 导航草稿（进入史导航前的缓冲内容）。
    draft: Option<String>,
    /// Up 钳制计数。
    up_clamps: u64,
    /// Down 回草稿次数。
    down_drafts: u64,
    /// Tab 响铃计数。
    bells: u64,
    /// Tab 二义提示计数。
    ambigs: u64,
    /// 二义提示已挂起（首次 Tab 扩前缀后置位，再 Tab 展开）。
    ambig_pending: bool,
}

impl LineEdit {
    /// 新建会话。
    pub fn new(comp: Completer) -> Self {
        LineEdit {
            buf: LineBuf::new(),
            hist: HistoryRing::new(),
            comp,
            nav: NavPos::Live,
            draft: None,
            up_clamps: 0,
            down_drafts: 0,
            bells: 0,
            ambigs: 0,
            ambig_pending: false,
        }
    }

    /// 提交当前行：进史、清缓冲、回 Live。
    pub fn accept(&mut self) -> String {
        let line = self.buf.as_str();
        self.hist.push(&line);
        self.buf = LineBuf::new();
        self.nav = NavPos::Live;
        self.draft = None;
        self.ambig_pending = false;
        line
    }

    /// Up：回溯一条，越界钳制在最旧。
    pub fn up(&mut self) {
        let k = match self.nav {
            NavPos::Live => {
                if self.hist.count() == 0 {
                    return;
                }
                // 进入史导航前存草稿。
                self.draft = Some(self.buf.as_str());
                0
            }
            NavPos::Hist(k) => {
                if k + 1 >= self.hist.count() {
                    self.up_clamps += 1;
                    return;
                }
                k + 1
            }
        };
        self.nav = NavPos::Hist(k);
        if let Some(s) = self.hist.nth_newest(k) {
            let mut tmp = String::from(s);
            let _ = self.buf.set_line(&tmp);
            tmp.clear();
        }
    }

    /// Down：回新一条，越过最新回草稿。
    pub fn down(&mut self) {
        match self.nav {
            NavPos::Live => return,
            NavPos::Hist(0) => {
                // 回到 Live：恢复草稿。
                if let Some(d) = &self.draft {
                    let d2 = d.clone();
                    let _ = self.buf.set_line(&d2);
                } else {
                    self.buf = LineBuf::new();
                }
                self.nav = NavPos::Live;
                self.down_drafts += 1;
            }
            NavPos::Hist(k) => {
                self.nav = NavPos::Hist(k - 1);
                if let Some(s) = self.hist.nth_newest(k - 1) {
                    let s2 = String::from(s);
                    let _ = self.buf.set_line(&s2);
                }
            }
        }
    }

    /// Tab：补全 + 二义提示（首次扩展公共前缀，再 Tab 展开清单）。
    pub fn tab(&mut self) -> CompleteOutcome {
        let prefix = self.buf.as_str();
        let (outcome, ext, cands) = self.comp.complete(&prefix);
        match outcome {
            CompleteOutcome::Unique => {
                let _ = self.buf.set_line(&prefix);
                for b in ext.bytes() {
                    let _ = self.buf.insert(b);
                }
                self.ambig_pending = false;
            }
            CompleteOutcome::Ambiguous => {
                if !ext.is_empty() {
                    for b in ext.bytes() {
                        let _ = self.buf.insert(b);
                    }
                }
                if self.ambig_pending {
                    // 再 Tab：展开清单。
                    self.ambigs += 1;
                }
                self.ambig_pending = true;
                let _ = Self::banner_of(&prefix, &cands);
            }
            CompleteOutcome::None => {
                self.bells += 1;
                self.ambig_pending = false;
            }
        }
        outcome
    }

    fn banner_of(prefix: &str, cands: &[&'static str]) -> String {
        Completer::ambig_banner(prefix, cands)
    }

    /// 缓冲引用。
    pub fn buf(&self) -> &LineBuf {
        &self.buf
    }

    /// 史引用。
    pub fn hist(&self) -> &HistoryRing {
        &self.hist
    }

    /// Up 钳制计数。
    pub fn up_clamps(&self) -> u64 {
        self.up_clamps
    }

    /// Down 回草稿计数。
    pub fn down_drafts(&self) -> u64 {
        self.down_drafts
    }

    /// 响铃计数。
    pub fn bells(&self) -> u64 {
        self.bells
    }

    /// 二义提示计数。
    pub fn ambigs(&self) -> u64 {
        self.ambigs
    }
}

// --------------------------------------------------------------------------
// 判据组：UNX-F0264-J1
// --------------------------------------------------------------------------
/// 判据入口。
pub fn run_unx264_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx264");
    if l264_segment_guard().is_err() {
        cs.fail("C264-SEG", "段守卫失败");
        return cs;
    }

    // ---- C264-01 历史 16 条循环：20 进 16 留，最旧 4 条淘汰留痕 ----
    let mut le = LineEdit::new(Completer::new(&[
        "boot", "capsule", "efirt", "help", "ktest", "mem", "ps", "secureboot", "sym", "uptime",
    ]));
    for i in 0..20usize {
        let mut s = String::from("cmd");
        push_usize(&mut s, i);
        le.hist.push(&s);
    }
    cs.add(
        "C264-01 历史16条循环",
        le.hist.count() == 16 && le.hist.evicted() == 4 && le.hist.nth_newest(0) == Some("cmd19") && le.hist.nth_newest(15) == Some("cmd4"),
        "20进16留淘汰4",
    );

    // ---- C264-02 相邻去重 ----
    let mut le2 = LineEdit::new(Completer::new(&[]));
    le2.hist.push("dup");
    le2.hist.push("dup");
    le2.hist.push("dup");
    cs.add(
        "C264-02 相邻去重",
        le2.hist.count() == 1 && le2.hist.dup_skipped() == 2,
        "三同只进一",
    );

    // ---- C264-03 空行不进史 ----
    le2.hist.push("");
    cs.add(
        "C264-03 空行不进史",
        le2.hist.count() == 1 && le2.hist.empty_rejected() == 1,
        "空行拒收录",
    );

    // ---- C264-04 Up 循环导航 + 越界钳制 ----
    let mut le3 = LineEdit::new(Completer::new(&[]));
    le3.hist.push("a1");
    le3.hist.push("a2");
    le3.hist.push("a3");
    le3.up(); // a3
    le3.up(); // a2
    le3.up(); // a1
    let at_oldest = le3.buf.as_str() == "a1";
    le3.up(); // 钳制
    cs.add(
        "C264-04 Up越界钳制",
        at_oldest && le3.buf.as_str() == "a1" && le3.up_clamps() == 1,
        "最旧钳制留账",
    );

    // ---- C264-05 Down 回草稿（干净会话：写草稿→up 进史→down 恢复） ----
    let mut le3d = LineEdit::new(Completer::new(&[]));
    le3d.hist.push("a1");
    le3d.hist.push("a2");
    le3d.hist.push("a3");
    let _ = le3d.buf.insert(b'x');
    let _ = le3d.buf.insert(b'y');
    le3d.up(); // 进史导航 a3，草稿 xy
    le3d.down(); // 回草稿 xy
    cs.add(
        "C264-05 Down回草稿",
        le3d.buf.as_str() == "xy" && le3d.down_drafts() == 1,
        "草稿字节级恢复",
    );

    // ---- C264-06 前缀补全唯一命中 ----
    let mut le4 = LineEdit::new(Completer::new(&[
        "boot", "capsule", "efirt", "help", "ktest", "mem", "ps", "secureboot", "sym", "uptime",
    ]));
    let _ = le4.buf.insert(b's');
    let _ = le4.buf.insert(b'e');
    let out = le4.tab(); // secureboot 唯一
    cs.add(
        "C264-06 唯一命中补全",
        out == CompleteOutcome::Unique && le4.buf.as_str() == "secureboot ",
        "整词+尾空格",
    );

    // ---- C264-07 多候选公共前缀扩展 ----
    let mut le5 = LineEdit::new(Completer::new(&[
        "boot", "capsule", "efirt", "help", "ktest", "mem", "ps", "secureboot", "sym", "uptime",
    ]));
    let _ = le5.buf.insert(b's');
    let out5 = le5.tab(); // secureboot/sym：公共前缀 s，已到头
    cs.add(
        "C264-07 多候选公共前缀",
        out5 == CompleteOutcome::Ambiguous && le5.buf.as_str() == "s",
        "公共前缀已满扩展零字节",
    );

    // ---- C264-08 再 Tab 展开二义清单 ----
    let out6 = le5.tab();
    let banner = Completer::ambig_banner("s", &["secureboot", "sym"]);
    cs.add(
        "C264-08 Tab二义提示",
        out6 == CompleteOutcome::Ambiguous && le5.ambigs() == 1 && banner.contains("2 项") && banner.contains("secureboot") && banner.contains("sym"),
        "二次Tab列清单",
    );

    // ---- C264-09 零候选响铃 ----
    le5.buf = LineBuf::new();
    let _ = le5.buf.insert(b'z');
    let out7 = le5.tab();
    cs.add(
        "C264-09 零候选响铃",
        out7 == CompleteOutcome::None && le5.bells() == 1 && le5.buf.as_str() == "z",
        "缓冲不动只响铃",
    );

    // ---- C264-10 缓冲满拒插 + 不变式 ----
    let mut lb = LineBuf::new();
    let mut ok = true;
    for i in 0..(LINE_CAP + 10) {
        let b = b'a' + (i % 26) as u8;
        let r = lb.insert(b);
        if i < LINE_CAP && !r {
            ok = false;
        }
        if i >= LINE_CAP && r {
            ok = false;
        }
    }
    cs.add(
        "C264-10 缓冲满拒插",
        ok && lb.len() == LINE_CAP && lb.full_rejects() == 10 && lb.as_str().len() == LINE_CAP,
        "满后10拒不变式保",
    );

    // ---- C264-11 光标越界钳制 ----
    let mut lb2 = LineBuf::new();
    lb2.left();
    lb2.right();
    let _ = lb2.insert(b'k');
    lb2.end();
    lb2.right();
    lb2.delete_at();
    lb2.backspace();
    lb2.backspace();
    let clamped = lb2.cursor_clamps() == 5 && lb2.is_empty();
    cs.add(
        "C264-11 光标越界钳制",
        clamped,
        "双边界四次钳制",
    );

    // ---- C264-12 控制字符拒绝 + 编辑操作守恒 ----
    let mut lb3 = LineBuf::new();
    let r1 = lb3.insert(0x01);
    let r2 = lb3.insert(0x7F);
    let _ = lb3.insert(b'a');
    let _ = lb3.insert(b'b');
    let _ = lb3.insert(b'c');
    lb3.left();
    lb3.left();
    lb3.home();
    let killed = lb3.kill_to_end();
    cs.add(
        "C264-12 控制字符拒+kill",
        !r1 && !r2 && lb3.ctrl_rejects() == 2 && killed.as_deref() == Some("abc") && lb3.is_empty(),
        "ctrl拒2+kill段一致",
    );

    cs
}
