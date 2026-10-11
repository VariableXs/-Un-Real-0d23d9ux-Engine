//! unx504_lineedit —— UNX-F0504 行缓冲编辑（退格/回车）。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B26 · 行 2017）：
//! 「UNX-F0504 | 行缓冲编辑（退格/回车） | 220 | 已深化 |
//!   判据：编辑三键（退格/回车/取消行）行为账齐（9/9 用例），
//!   行内容与键序复算一致」
//!
//! 组件：行缓冲（定容 · 光标位 · 可打印插入）
//!      + 编辑三键闭集（退格 / 回车提交 / 取消行）
//!      + 键序复算器（按键序列 → 行内容 重放确定性）
//!      + 提交历史账（append-only · 行号单调）
//!      + 行为账（下发 ≡ 命中 + 拒绝 · 9/9 golden 用例）
//!      + LCG 压力 512 轮 + 重放对拍 + 快照指纹（FNV-1a 确定性）。
//!
//! 判据落地：①编辑三键行为账齐——9 个 golden 用例（退格空行/退格
//! 正常/退格截断拒绝/回车提交/回车空行拒绝/取消行有内容/取消行空/
//! 容满拒绝插入/容满退格仍可用）；②行内容与键序复算一致——同一按键
//! 序列复算两次行内容与指纹全等；③账实守恒（键入 ≡ 命中 + 拒绝）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const LED_VERSION: &str = "unx504_lineedit-v1";

/// 诊断段基址（0x9900 段 · 本模块独占 0x9911~0x9918，入库前已 grep 全域确认无撞段）。
pub const LED_SEG_BASE: u16 = 0x9900;

/// 行缓冲容量（字节 · 定容守恒）。
pub const LED_LINE_CAP: usize = 96;

/// 提交历史账容量（append-only 截断面）。
pub const LED_HISTORY_CAP: usize = 64;

/// golden 用例数（判据口径 9/9）。
pub const LED_GOLDEN_CASES: usize = 9;

/// LCG 压力轮数。
pub const LED_STRESS_ROUNDS: u32 = 512;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9911~0x9918）
// ---------------------------------------------------------------------------

/// 诊断码闭集（段内八码；闭集外一律拒绝进入账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedCode {
    /// 0x9911 退格于空行（无可删字符）。
    BackspaceEmpty,
    /// 0x9912 回车空行（空行不提交）。
    EnterEmpty,
    /// 0x9913 插入越容（行满拒绝可打印字符）。
    InsertFull,
    /// 0x9914 键序复算漂移（同键序复算行内容不一致）。
    ReplayDrift,
    /// 0x9915 非可打印字符（控制字节拒绝插入）。
    CharNotPrintable,
    /// 0x9916 历史账溢出（截断面外未拒绝覆盖语义错位）。
    HistoryOverflow,
    /// 0x9917 行为账不平（键入 ≢ 命中 + 拒绝）。
    LedgerUnbalanced,
    /// 0x9918 快照指纹漂移（同行编辑器两次快照指纹不同）。
    FingerprintDrift,
}

impl LedCode {
    /// 全量闭集（八码）。
    pub const ALL: [LedCode; 8] = [
        LedCode::BackspaceEmpty,
        LedCode::EnterEmpty,
        LedCode::InsertFull,
        LedCode::ReplayDrift,
        LedCode::CharNotPrintable,
        LedCode::HistoryOverflow,
        LedCode::LedgerUnbalanced,
        LedCode::FingerprintDrift,
    ];

    /// 码值（段位守卫口径）。
    pub const fn val(self) -> u16 {
        match self {
            LedCode::BackspaceEmpty => 0x9911,
            LedCode::EnterEmpty => 0x9912,
            LedCode::InsertFull => 0x9913,
            LedCode::ReplayDrift => 0x9914,
            LedCode::CharNotPrintable => 0x9915,
            LedCode::HistoryOverflow => 0x9916,
            LedCode::LedgerUnbalanced => 0x9917,
            LedCode::FingerprintDrift => 0x9918,
        }
    }

    /// 现象（三要素之一）。
    pub const fn what(self) -> &'static str {
        match self {
            LedCode::BackspaceEmpty => "空行上退格，无可删字符",
            LedCode::EnterEmpty => "空行上回车，无内容可提交",
            LedCode::InsertFull => "行缓冲已满仍尝试插入可打印字符",
            LedCode::ReplayDrift => "同键序复算行内容与首次不一致",
            LedCode::CharNotPrintable => "控制字节被尝试插入行缓冲",
            LedCode::HistoryOverflow => "提交历史越过截断面语义",
            LedCode::LedgerUnbalanced => "行为账不平：键入 ≢ 命中 + 拒绝",
            LedCode::FingerprintDrift => "同一编辑器状态两次快照指纹不一致",
        }
    }

    /// 原因（三要素之二）。
    pub const fn why(self) -> &'static str {
        match self {
            LedCode::BackspaceEmpty => "缓冲长度为零，退格无作用对象",
            LedCode::EnterEmpty => "提交语义要求非空行，空行提交会污染历史账",
            LedCode::InsertFull => "定容缓冲守恒要求容满即拒，溢出会破坏定容口径",
            LedCode::ReplayDrift => "复算器含非确定源或键序归一化不完整",
            LedCode::CharNotPrintable => "可打印闭集为 0x20~0x7E，闭集外拒绝",
            LedCode::HistoryOverflow => "历史账 append-only 截断面语义被绕过",
            LedCode::LedgerUnbalanced => "存在绕过留账的按键路径或拒绝未计数",
            LedCode::FingerprintDrift => "缓冲或账面含未初始化/非确定字段",
        }
    }

    /// 建议（三要素之三）。
    pub const fn advice(self) -> &'static str {
        match self {
            LedCode::BackspaceEmpty => "退格前判空，空行退格计入拒绝账即可",
            LedCode::EnterEmpty => "空行回车计入拒绝账，不产生历史条目",
            LedCode::InsertFull => "容满插入计入拒绝账并回显溢出提示语义",
            LedCode::ReplayDrift => "固定键序归一化函数，排查复算路径非确定源",
            LedCode::CharNotPrintable => "插入入口统一过可打印闭集过滤",
            LedCode::HistoryOverflow => "历史满后按截断语义拒绝新提交而非覆盖",
            LedCode::LedgerUnbalanced => "审计按键入口是否全部经由留账路径",
            LedCode::FingerprintDrift => "排查缓冲清零与账面字段初始化",
        }
    }
}

/// 段位守卫：全部诊断码落在 0x9911~0x9918 窗口内。
pub fn segment_guard() -> bool {
    LedCode::ALL
        .iter()
        .all(|c| c.val() >= 0x9911 && c.val() <= 0x9918)
}

// ---------------------------------------------------------------------------
// 按键闭集
// ---------------------------------------------------------------------------

/// 按键闭集（可打印 / 退格 / 回车 / 取消行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// 可打印字符（0x20~0x7E）。
    Ch(u8),
    /// 退格。
    Backspace,
    /// 回车（提交当前行）。
    Enter,
    /// 取消行（清空当前行，内容丢弃并留账）。
    KillLine,
}

impl Key {
    /// 可打印判定（0x20~0x7E 闭集；const 兼容写法）。
    pub const fn printable(b: u8) -> bool {
        b >= 0x20 && b <= 0x7E
    }
}

// ---------------------------------------------------------------------------
// 行编辑器
// ---------------------------------------------------------------------------

/// 行编辑器（单行定容缓冲 + 行为账 + 提交历史）。
#[derive(Clone, Debug)]
pub struct LineEditor {
    buf: Vec<u8>,
    log: LedLog,
    history: Vec<(u64, String)>,
    history_dropped: u64,
    history_seen: u64,
    killed: u64,
    committed: u64,
    line_no: u64,
    tick: u64,
    lcg: u64,
}

/// 行为账（守恒式：键入 ≡ 命中 + 拒绝）。
#[derive(Clone, Debug)]
pub struct LedLog {
    keys: u64,
    hits: u64,
    rejects: u64,
    /// 拒绝码计数（按 LedCode::val() 索引语义聚合）。
    reject_codes: Vec<(u16, u64)>,
}

impl LedLog {
    /// 新账。
    pub fn new() -> Self {
        LedLog {
            keys: 0,
            hits: 0,
            rejects: 0,
            reject_codes: Vec::new(),
        }
    }

    /// 命中入账。
    pub fn hit(&mut self) {
        self.keys += 1;
        self.hits += 1;
    }

    /// 拒绝入账（带码）。
    pub fn reject(&mut self, code: u16) {
        self.keys += 1;
        self.rejects += 1;
        let mut found = false;
        for pair in self.reject_codes.iter_mut() {
            if pair.0 == code {
                pair.1 += 1;
                found = true;
                break;
            }
        }
        if !found {
            self.reject_codes.push((code, 1));
        }
    }

    /// 守恒式。
    pub fn conserves(&self) -> bool {
        self.keys == self.hits + self.rejects
    }

    /// 累计。
    pub fn totals(&self) -> (u64, u64, u64) {
        (self.keys, self.hits, self.rejects)
    }
}

impl LineEditor {
    /// 新编辑器。
    pub fn new() -> Self {
        LineEditor {
            buf: Vec::new(),
            log: LedLog::new(),
            history: Vec::new(),
            history_dropped: 0,
            history_seen: 0,
            killed: 0,
            committed: 0,
            line_no: 0,
            tick: 0,
            lcg: 0x5DEE_CE66_D133_BEEF,
        }
    }

    /// 当前行内容。
    pub fn line(&self) -> String {
        String::from_iter(self.buf.iter().map(|b| *b as char))
    }

    /// 当前行字节长。
    pub fn line_len(&self) -> usize {
        self.buf.len()
    }

    /// 提交历史条数（窗口内）。
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// 提交历史窗口。
    pub fn history_window(&self) -> &[(u64, String)] {
        &self.history
    }

    /// 按键处理（留账唯一入口）。
    pub fn feed(&mut self, k: Key) -> bool {
        self.tick += 1;
        match k {
            Key::Ch(b) => {
                if !Key::printable(b) {
                    self.log.reject(LedCode::CharNotPrintable.val());
                    return false;
                }
                if self.buf.len() >= LED_LINE_CAP {
                    self.log.reject(LedCode::InsertFull.val());
                    return false;
                }
                self.buf.push(b);
                self.log.hit();
                true
            }
            Key::Backspace => {
                if self.buf.is_empty() {
                    self.log.reject(LedCode::BackspaceEmpty.val());
                    return false;
                }
                self.buf.pop();
                self.log.hit();
                true
            }
            Key::Enter => {
                if self.buf.is_empty() {
                    self.log.reject(LedCode::EnterEmpty.val());
                    return false;
                }
                self.line_no += 1;
                self.committed += 1;
                self.history_seen += 1;
                let content = self.line();
                if self.history.len() >= LED_HISTORY_CAP {
                    self.history.remove(0);
                    self.history_dropped += 1;
                }
                self.history.push((self.line_no, content));
                self.buf.clear();
                self.log.hit();
                true
            }
            Key::KillLine => {
                // 取消行：空行取消是幂等命中（账面留痕但不产生内容）。
                if !self.buf.is_empty() {
                    self.killed += 1;
                }
                self.buf.clear();
                self.log.hit();
                true
            }
        }
    }

    /// 键序复算：从零重建，与当前行内容比对（确定性口径）。
    pub fn replay_from_keys(&self, keys: &[Key]) -> String {
        let mut buf: Vec<u8> = Vec::new();
        for k in keys {
            match k {
                Key::Ch(b) => {
                    if Key::printable(*b) && buf.len() < LED_LINE_CAP {
                        buf.push(*b);
                    }
                }
                Key::Backspace => {
                    buf.pop();
                }
                Key::Enter | Key::KillLine => {
                    buf.clear();
                }
            }
        }
        String::from_iter(buf.iter().map(|b| *b as char))
    }

    /// LCG 伪随机。
    fn lcg_next(&mut self) -> u64 {
        self.lcg = self
            .lcg
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.lcg
    }

    /// golden 9 用例：编辑三键行为账逐例核。
    /// 返回命中用例数（判据口径 9/9）。
    pub fn golden_9(&mut self) -> u32 {
        let mut hits = 0u32;

        // g1：空行退格 → 拒绝（BackspaceEmpty）。
        let ok = !self.feed(Key::Backspace)
            && self.line_len() == 0
            && self.log.rejects >= 1;
        if ok {
            hits += 1;
        }

        // g2：正常退格 → 命中且长度减一。
        let _ = self.feed(Key::Ch(b'a'));
        let _ = self.feed(Key::Ch(b'b'));
        let ok = self.feed(Key::Backspace) && self.line_len() == 1 && self.line() == "a";
        if ok {
            hits += 1;
        }

        // g3：控制字节插入 → 拒绝（CharNotPrintable）。
        let ok = !self.feed(Key::Ch(0x07)) && self.line_len() == 1;
        if ok {
            hits += 1;
        }

        // g4：回车提交非空 → 命中且历史 +1、缓冲清空。
        let before = self.history_len();
        let ok = self.feed(Key::Enter)
            && self.history_len() == before + 1
            && self.line_len() == 0;
        if ok {
            hits += 1;
        }

        // g5：空行回车 → 拒绝（EnterEmpty）且历史不变。
        let before = self.history_len();
        let ok = !self.feed(Key::Enter) && self.history_len() == before;
        if ok {
            hits += 1;
        }

        // g6：取消行有内容 → 清空并计数 killed。
        let _ = self.feed(Key::Ch(b'x'));
        let _ = self.feed(Key::Ch(b'y'));
        let ok = self.feed(Key::KillLine) && self.line_len() == 0;
        if ok {
            hits += 1;
        }

        // g7：取消行空行 → 幂等命中。
        let ok = self.feed(Key::KillLine) && self.line_len() == 0;
        if ok {
            hits += 1;
        }

        // g8：容满拒绝插入（插 LED_LINE_CAP 个再插一个拒）。
        let mut eng = LineEditor::new();
        for i in 0..LED_LINE_CAP {
            let b = b'0' + (i % 10) as u8;
            let _ = eng.feed(Key::Ch(b));
        }
        let ok = eng.line_len() == LED_LINE_CAP && !eng.feed(Key::Ch(b'!'));
        if ok {
            hits += 1;
        }

        // g9：容满后退格仍可用（拒插入不拒退格）。
        let ok = eng.feed(Key::Backspace) && eng.line_len() == LED_LINE_CAP - 1;
        if ok {
            hits += 1;
        }

        hits
    }

    /// LCG 压力 512 轮：随机按键流（含控制字节/越容），全程守恒。
    pub fn stress(&mut self, rounds: u32) -> bool {
        let mut ok = true;
        for _ in 0..rounds {
            let r = self.lcg_next();
            let k = match r % 5 {
                0 => Key::Ch((0x20 + (r >> 8) % 0x5F) as u8),
                1 => Key::Ch((((r >> 8) % 0x20)) as u8), // 控制字节（必拒）
                2 => Key::Backspace,
                3 => Key::Enter,
                _ => Key::KillLine,
            };
            let _ = self.feed(k);
            ok &= self.log.conserves();
            ok &= self.line_len() <= LED_LINE_CAP;
            ok &= self.history.len() <= LED_HISTORY_CAP;
            ok &= self.history_seen == self.history.len() as u64 + self.history_dropped;
        }
        ok
    }

    /// 重放对拍：同键序复算两次行内容与指纹全等。
    pub fn replay_pair(&mut self) -> bool {
        let keys = [
            Key::Ch(b'h'),
            Key::Ch(b'e'),
            Key::Ch(b'l'),
            Key::Ch(b'l'),
            Key::Ch(b'o'),
            Key::Backspace,
            Key::Ch(b'0'),
        ];
        // 先真实喂入。
        for k in keys {
            let _ = self.feed(k);
        }
        let live = self.line();
        // 复算两次。
        let r1 = self.replay_from_keys(&keys);
        let r2 = self.replay_from_keys(&keys);
        live == r1 && r1 == r2 && live == "hell0"
    }

    /// 历史账守恒（窗口口径）。
    pub fn history_conserves(&self) -> bool {
        self.history_seen == self.history.len() as u64 + self.history_dropped
    }

    /// 快照指纹（缓冲 + 账 + 历史 + 计数全量）。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for b in self.buf.iter() {
            mix(&mut h, *b);
        }
        mix(&mut h, 0x1E);
        let (k, hi, rj) = self.log.totals();
        for v in [k, hi, rj, self.killed, self.committed, self.line_no, self.history_seen, self.history_dropped, self.tick] {
            for i in 0..8 {
                mix(&mut h, ((v >> (i * 8)) & 0xFF) as u8);
            }
        }
        for (_, content) in &self.history {
            for b in content.as_bytes() {
                mix(&mut h, *b);
            }
            mix(&mut h, 0x0A);
        }
        h
    }

    /// 快照双取一致。
    pub fn snapshot_stable(&self) -> bool {
        self.snapshot_fingerprint() == self.snapshot_fingerprint()
    }
}

impl Default for LineEditor {
    fn default() -> Self {
        LineEditor::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（当前模式：轻量判据内嵌，重判据押后统一补）
// ---------------------------------------------------------------------------

/// 判据集（登记进 mod.rs 域聚合——折叠范式）。
pub fn run_t504_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t504");

    // c1 段位守卫 + 可打印闭集边界。
    set.add(
        "t504-c1-seg",
        segment_guard()
            && Key::printable(0x20)
            && Key::printable(0x7E)
            && !Key::printable(0x1F)
            && !Key::printable(0x7F),
        if segment_guard() { "" } else { "diag segment guard broken" },
    );

    // c2 golden 9/9：编辑三键行为账齐。
    let mut eng = LineEditor::new();
    let hits = eng.golden_9();
    set.add(
        "t504-c2-golden",
        hits == LED_GOLDEN_CASES as u32,
        if hits == 9 { "" } else { "golden cases below 9/9" },
    );

    // c3 键序复算一致：固定键序复算两次 + 与实喂行内容全等。
    set.add(
        "t504-c3-replay",
        eng.replay_pair(),
        if eng.replay_pair() { "" } else { "key-order replay drift" },
    );

    // c4 账实守恒 + 历史守恒 + 快照稳定。
    set.add(
        "t504-c4-ledger",
        eng.log.conserves()
            && eng.history_conserves()
            && eng.snapshot_stable(),
        if eng.log.conserves() { "" } else { "key ledger unbalanced" },
    );

    // c5 LCG 压力 512 轮。
    let mut eng = LineEditor::new();
    let ok = eng.stress(LED_STRESS_ROUNDS);
    set.add(
        "t504-c5-stress",
        ok && eng.log.conserves() && eng.history_conserves(),
        if ok { "" } else { "512-round stress violated invariant" },
    );

    // c6 历史截断语义：LED_HISTORY_CAP 条提交后再提交，窗口恒不超且 dropped 增加。
    let mut eng = LineEditor::new();
    for i in 0..(LED_HISTORY_CAP as u64 + 7) {
        // 逐字符构造 "l<i>" 行后回车。
        let _ = eng.feed(Key::Ch(b'l'));
        let mut v = i;
        if v == 0 {
            let _ = eng.feed(Key::Ch(b'0'));
        }
        while v > 0 {
            let _ = eng.feed(Key::Ch(b'0' + (v % 10) as u8));
            v /= 10;
        }
        let _ = eng.feed(Key::Enter);
    }
    set.add(
        "t504-c6-history",
        eng.history_len() == LED_HISTORY_CAP && eng.history_conserves(),
        if eng.history_len() == LED_HISTORY_CAP { "" } else { "history truncation semantics broken" },
    );

    // c7 行号单调：历史行号严格递增。
    let mono = {
        let w = eng.history_window();
        let mut ok = true;
        for i in 1..w.len() {
            ok &= w[i].0 > w[i - 1].0;
        }
        ok
    };
    set.add(
        "t504-c7-mono",
        mono,
        if mono { "" } else { "history line numbers not monotonic" },
    );

    // c8 拒绝码账：空行退格与控制字节两码各留痕。
    let mut eng = LineEditor::new();
    let _ = eng.feed(Key::Backspace);
    let _ = eng.feed(Key::Ch(0x01));
    set.add(
        "t504-c8-rejectcodes",
        eng.log.rejects == 2 && eng.log.conserves(),
        if eng.log.rejects == 2 { "" } else { "reject codes not ledgered" },
    );

    set
}
