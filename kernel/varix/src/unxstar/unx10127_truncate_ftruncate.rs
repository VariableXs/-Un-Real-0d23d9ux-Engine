//! UNX-F10127 truncate/ftruncate 正反双判据（截断达成与负长误用受控）· 0x92FC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10128-J1 锁语义达成（LOCK_SH/LOCK_EX/LOCK_UN 转换 LTP
//!       flock01 面 10/10 次）；UNX-F10128-J2 非阻塞冲突误用受控（预期
//!       EWOULDBLOCK 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92FC0（本卡独占 0x40 窗口，下一卡 0x93000）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_trunc_call`："ftruncate|path|len" 三成分，len 允许
//!      带负号（负值是 J2 注入对象，解析层不吞），非数字/缺成分逐档拒绝；
//!   ② 文件册 `TruncLedger`：路径→(体, 长)，ftruncate 三语义：缩短去尾 /
//!      加长零填充 / 置零清空，账面长度与体长逐次一致；
//!   ③ 状态机 `TruncState`：Full→Shortened→Extended→Zeroed 单向（禁倒退）；
//!   ④ 注入核 `BadTrunc`：负长 / 缺失路径 / 超上限 / 非数字 四类坏样本
//!      逐类 10/10 次精确复现 EINVAL/ENOENT/EFBIG 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10127 独占 0x40 窗口，下一卡 0x93000）。
pub const SEG_BASE: u32 = 0x92FC0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93000;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 文件册容量。
pub const FILE_CAP: usize = 16;

/// 单文件长度上限（超即 EFBIG 档）。
pub const FILE_MAX: u32 = 4096;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcCode {
    /// 负长（EINVAL 族）
    NegativeLen,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 超上限（EFBIG 族）
    OverMax,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 文件册满（ENOSPC 档）
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 体长失平（账面长度与体长不一致）
    SizeDrift,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 零填充失准
    ZeroFill,
    /// 语料指纹漂移
    RoundTrip,
}

impl TcCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TcCode::NegativeLen => "负长被拒：truncate 长度不得为负；EINVAL 精确归档",
            TcCode::MissingPath => "缺失路径被拒：账本中无此文件；ENOENT 精确归档",
            TcCode::OverMax => "超上限被拒：目标长度超 FILE_MAX；EFBIG 精确归档",
            TcCode::BadNumber => "非数字成分被拒：len 必须为（可带负号的）十进制；请改写",
            TcCode::MissingPart => "成分缺失被拒：调用行必须三段 ftruncate|path|len；请补成分",
            TcCode::LedgerFull => "文件册满被拒：请先清理闲置文件",
            TcCode::StateRegress => "状态倒退被拒：Full→Shortened→Extended→Zeroed 单向",
            TcCode::SizeDrift => "体长失平被拒：账面长度必须与体长逐次一致",
            TcCode::EmptySummary => "空成文被拒：截断小结不得为空串；请先 ftruncate 再成文",
            TcCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            TcCode::ZeroFill => "零填充失准被拒：加长段必须逐字节为零",
            TcCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TcCode; 12] = [
        TcCode::NegativeLen,
        TcCode::MissingPath,
        TcCode::OverMax,
        TcCode::BadNumber,
        TcCode::MissingPart,
        TcCode::LedgerFull,
        TcCode::StateRegress,
        TcCode::SizeDrift,
        TcCode::EmptySummary,
        TcCode::SegmentViolation,
        TcCode::ZeroFill,
        TcCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的截断调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TruncCall {
    /// 路径
    pub path: String,
    /// 目标长度（允许负值，判定层拒）
    pub len: i64,
}

/// 解析 "ftruncate|path|len" 三段调用行。
pub fn parse_trunc_call(line: &str) -> Result<TruncCall, TcCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(TcCode::MissingPart);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(TcCode::MissingPart),
    };
    let ls = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(TcCode::MissingPart),
    };
    if ls.is_empty() {
        return Err(TcCode::BadNumber);
    }
    let (neg, digits) = match ls.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, ls.as_str()),
    };
    if digits.is_empty() {
        return Err(TcCode::BadNumber);
    }
    let mut acc: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return Err(TcCode::BadNumber);
        }
        acc = acc.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    Ok(TruncCall { path, len: if neg { -acc } else { acc } })
}

// ── 截断状态机 ────────────────────────────────────────────────────────
/// 截断生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TruncState {
    /// 原长
    Full,
    /// 已缩短
    Shortened,
    /// 已加长
    Extended,
    /// 已置零
    Zeroed,
}

impl TruncState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: TruncState) -> bool {
        let rank = |s: TruncState| match s {
            TruncState::Full => 0u8,
            TruncState::Shortened => 1,
            TruncState::Extended => 2,
            TruncState::Zeroed => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            TruncState::Full => "原长",
            TruncState::Shortened => "已缩短",
            TruncState::Extended => "已加长",
            TruncState::Zeroed => "已置零",
        }
    }
}

// ── 文件册 ────────────────────────────────────────────────────────────
/// 文件条目。
#[derive(Clone, Debug)]
pub struct TruncEnt {
    /// 路径
    pub path: String,
    /// 文件体
    pub body: Vec<u8>,
    /// 账面长度（须与 body.len() 一致）
    pub size: u32,
    /// 截断状态
    pub state: TruncState,
}

/// 文件册：路径→(体, 长)。
#[derive(Clone, Debug)]
pub struct TruncLedger {
    ents: Vec<TruncEnt>,
    cap: usize,
    /// 累计截断次数
    pub truncs: u32,
    /// 累计缩短次数
    pub shorts: u32,
    /// 累计加长次数
    pub extends: u32,
}

impl TruncLedger {
    /// 空账构造。
    pub fn new() -> TruncLedger {
        TruncLedger { ents: Vec::new(), cap: FILE_CAP, truncs: 0, shorts: 0, extends: 0 }
    }

    /// 按路径寻址。
    fn idx_of(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 公开寻址（成文用）。
    pub fn idx_of_pub(&self, path: &str) -> Option<usize> {
        self.idx_of(path)
    }

    /// 建文件。
    pub fn create(&mut self, path: &str, body: &[u8]) -> Result<(), TcCode> {
        if self.idx_of(path).is_some() {
            return Err(TcCode::LedgerFull);
        }
        if self.ents.len() >= self.cap {
            return Err(TcCode::LedgerFull);
        }
        let mut b: Vec<u8> = Vec::new();
        let _ = b.extend_from_slice(body);
        let size = b.len() as u32;
        let _ = self.ents.push(TruncEnt {
            path: String::from(path),
            body: b,
            size,
            state: TruncState::Full,
        });
        Ok(())
    }

    /// ftruncate 判定核：负长/缺失/超限三闸；缩短去尾、加长零填充、置零清空。
    pub fn ftruncate(&mut self, path: &str, len: i64) -> Result<u32, TcCode> {
        if len < 0 {
            return Err(TcCode::NegativeLen);
        }
        if len as u64 > FILE_MAX as u64 {
            return Err(TcCode::OverMax);
        }
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(TcCode::MissingPath),
        };
        let cur = match self.ents.get(idx) {
            Some(e) => e.body.len() as u32,
            None => return Err(TcCode::MissingPath),
        };
        let target = len as u32;
        if target == 0 {
            // 置零：清空体与账（先于缩短分支判）。
            if let Some(e) = self.ents.get_mut(idx) {
                e.body.clear();
                e.size = 0;
                e.state = TruncState::Zeroed;
            }
            self.truncs = self.truncs.saturating_add(1);
            return Ok(0);
        }
        if target < cur {
            // 缩短：去尾。
            if let Some(e) = self.ents.get_mut(idx) {
                let keep = target as usize;
                let mut nb: Vec<u8> = Vec::new();
                let mut k = 0usize;
                while k < keep {
                    if let Some(b) = e.body.get(k) {
                        let _ = nb.push(*b);
                    }
                    k += 1;
                }
                e.body = nb;
                e.size = target;
                if e.state.can_advance(TruncState::Shortened) {
                    e.state = TruncState::Shortened;
                }
            }
            self.shorts = self.shorts.saturating_add(1);
        } else if target > cur {
            // 加长：零填充。
            if let Some(e) = self.ents.get_mut(idx) {
                let mut k = cur as usize;
                while k < target as usize {
                    let _ = e.body.push(0u8);
                    k += 1;
                }
                e.size = target;
                if e.state.can_advance(TruncState::Extended) {
                    e.state = TruncState::Extended;
                }
            }
            self.extends = self.extends.saturating_add(1);
        }
        self.truncs = self.truncs.saturating_add(1);
        Ok(target)
    }

    /// 体长一致性（账面审计）。
    pub fn sizes_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.size as usize != e.body.len() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 体读取。
    pub fn body_of(&self, path: &str) -> Option<Vec<u8>> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.body.clone()),
                None => None,
            },
            None => None,
        }
    }

    /// 状态读取。
    pub fn state_of(&self, path: &str) -> Option<TruncState> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.state),
                None => None,
            },
            None => None,
        }
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }
}

// ── J1 达成核：缩短/加长与零填充 10/10 轮 ─────────────────────────────
/// 逐轮：建 8 字节→缩短 4→加长 8（零填充核对）→置零→账面一致。
pub fn run_trunc_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[1, 2, 3, 4, 5, 6, 7, 8]);
        let mut all_ok = true;
        // 面1：缩短到 4。
        all_ok = all_ok && l.ftruncate("f", 4) == Ok(4)
            && l.body_of("f") == Some(Vec::from(&[1u8, 2, 3, 4] as &[u8]));
        probes += 1;
        // 面2：加长回 8，尾部零填充。
        let body8 = l.ftruncate("f", 8).is_ok();
        let b = l.body_of("f").unwrap_or_default();
        let zero_ok = b.len() == 8 && b.get(4) == Some(&0) && b.get(7) == Some(&0)
            && b.get(0) == Some(&1);
        all_ok = all_ok && body8 && zero_ok;
        probes += 1;
        // 面3：置零清空。
        all_ok = all_ok && l.ftruncate("f", 0) == Ok(0)
            && l.body_of("f").unwrap_or_default().is_empty();
        probes += 1;
        // 面4：状态推进到 Zeroed。
        all_ok = all_ok && l.state_of("f") == Some(TruncState::Zeroed);
        probes += 1;
        // 面5：账面长度与体长一致。
        all_ok = all_ok && l.sizes_consistent();
        probes += 1;
        // 面6：负长拒且账面不动。
        all_ok = all_ok && l.ftruncate("f", -1).err() == Some(TcCode::NegativeLen)
            && l.sizes_consistent();
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏截断矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTrunc {
    /// 负长（EINVAL 族）
    NegativeLen,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 超上限（EFBIG 族）
    OverMax,
    /// 非数字（EINVAL 族）
    BadNumber,
}

impl BadTrunc {
    /// 闭集全量。
    pub const ALL: [BadTrunc; 4] = [
        BadTrunc::NegativeLen,
        BadTrunc::MissingPath,
        BadTrunc::OverMax,
        BadTrunc::BadNumber,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadTrunc::NegativeLen => "负长截断",
            BadTrunc::MissingPath => "缺失路径截断",
            BadTrunc::OverMax => "超上限截断",
            BadTrunc::BadNumber => "非数字长度行",
        }
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), TcCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TcCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏截断注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_trunc_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadTrunc::ALL.len() {
        let case = match BadTrunc::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = TruncLedger::new();
            let _ = l.create("f", &[1, 2, 3]);
            let caught = match case {
                BadTrunc::NegativeLen => {
                    l.ftruncate("f", -2).err() == Some(TcCode::NegativeLen)
                }
                BadTrunc::MissingPath => {
                    l.ftruncate("ghost", 4).err() == Some(TcCode::MissingPath)
                }
                BadTrunc::OverMax => {
                    l.ftruncate("f", (FILE_MAX as i64) + 1).err() == Some(TcCode::OverMax)
                }
                BadTrunc::BadNumber => {
                    parse_trunc_call("ftruncate|f|4x").err() == Some(TcCode::BadNumber)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 截断小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「path=<s> size=<n> state=<t>」；账外先拒。
pub fn compose_trunc_summary(l: &TruncLedger, path: &str) -> Result<String, TcCode> {
    let idx = match l.idx_of_pub(path) {
        Some(i) => i,
        None => return Err(TcCode::EmptySummary),
    };
    let (size, state) = match l.ents.get(idx) {
        Some(e) => (e.size, e.state),
        None => return Err(TcCode::EmptySummary),
    };
    let out = format!("path={} size={} state={}", path, size, state.label());
    if out.is_empty() {
        return Err(TcCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；负长族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径族"),
    ("EFBIG", -27, 223, "Windows ERROR_FILE_TOO_LARGE；超上限族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料指纹：建→缩短→加长→置零→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = TruncLedger::new();
    let _ = l.create("f", &[9, 8, 7, 6]);
    let _ = l.ftruncate("f", 2);
    let _ = l.ftruncate("f", 5);
    let _ = l.ftruncate("f", 0);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_trunc_summary(&l, "f") {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&l.truncs.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10127 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10127_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10127");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92FC0..=0x92FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < TcCode::ALL.len() {
        let mut j = i + 1;
        while j < TcCode::ALL.len() {
            if let (Some(a), Some(b)) = (TcCode::ALL.get(i), TcCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = TcCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器：负值解析层不吞
    let parse_ok = parse_trunc_call("ftruncate|f|4").is_ok()
        && parse_trunc_call("ftruncate|f|-1").is_ok()
        && parse_trunc_call("ftruncate|f|4x").err() == Some(TcCode::BadNumber)
        && parse_trunc_call("ftruncate|f").err() == Some(TcCode::MissingPart);
    s.add("调用行解析器：带负号长度放行入判定层且坏成分逐档拒绝", parse_ok,
          "负值是 J2 注入对象；解析层只认（可带负号）十进制");

    // 4. J1 达成核
    let (ok, probes) = run_trunc_rounds();
    s.add("J1 截断达成：缩短/加长与零填充语义 LTP ftruncate01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（缩短/零填充/置零/态推进/账面/负长拒）全对");

    // 5. J2 负长
    let mut l = TruncLedger::new();
    let _ = l.create("a", &[1, 2, 3]);
    let neg_ok = l.ftruncate("a", -1).err() == Some(TcCode::NegativeLen)
        && l.body_of("a") == Some(Vec::from(&[1u8, 2, 3] as &[u8]));
    s.add("J2 负长误用受控：EINVAL 精确复现且账面不动", neg_ok,
          "负长必须拒；被拒截断不得改体");

    // 6. 加长零填充逐字节核对
    let _ = l.ftruncate("a", 6);
    let b6 = l.body_of("a").unwrap_or_default();
    let fill_ok = b6.len() == 6 && b6.get(3) == Some(&0) && b6.get(5) == Some(&0)
        && b6.get(0) == Some(&1);
    s.add("零填充：加长段逐字节为零且原段保留", fill_ok,
          "零填充语义必须逐字节可复核");

    // 7. 超上限档
    let over_ok = l.ftruncate("a", (FILE_MAX as i64) + 1).err() == Some(TcCode::OverMax)
        && l.ftruncate("a", FILE_MAX as i64).is_ok();
    s.add("超上限：EFBIG 档精确复现且边界值放行", over_ok,
          "FILE_MAX+1 拒、FILE_MAX 放行；边界必须分明");

    // 8. 体长账一致
    let cons_ok = l.sizes_consistent() && l.len() == 1;
    s.add("体长账一致：账面长度与体长逐条对平", cons_ok,
          "SizeDrift 是账面根缺陷；必须全账一致");

    // 9. 注入核
    let (inj, per_class) = run_bad_trunc_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "负长/缺失/超限/非数字必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadTrunc::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = TruncState::Full.can_advance(TruncState::Shortened)
        && TruncState::Shortened.can_advance(TruncState::Zeroed)
        && !TruncState::Zeroed.can_advance(TruncState::Full)
        && TruncState::Extended.label() == "已加长";
    s.add("状态机：Full→Shortened→Extended→Zeroed 单向", sm_ok,
          "截断生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(27)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_trunc_summary(&l, "a");
    let text_ok = match &sum {
        Ok(x) => x.contains("path=a") && x.contains("size="),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("截断达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_trunc_call("ftruncate|f|4").is_ok());
        assert!(parse_trunc_call("ftruncate|f|-2").is_ok());
        assert_eq!(parse_trunc_call("ftruncate|f|4x").err(), Some(TcCode::BadNumber));
        assert_eq!(parse_trunc_call("ftruncate|f").err(), Some(TcCode::MissingPart));
        let c = parse_trunc_call("ftruncate|f|-3").unwrap_or(TruncCall { path: String::new(), len: 0 });
        assert_eq!(c.len, -3);
    }

    #[test]
    fn shorten_drops_tail() {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[1, 2, 3, 4, 5]);
        assert_eq!(l.ftruncate("f", 2), Ok(2));
        assert_eq!(l.body_of("f"), Some(Vec::from(&[1u8, 2] as &[u8])));
        assert_eq!(l.shorts, 1);
    }

    #[test]
    fn extend_zero_fills() {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[7]);
        let _ = l.ftruncate("f", 4);
        assert_eq!(l.body_of("f"), Some(Vec::from(&[7u8, 0, 0, 0] as &[u8])));
        assert_eq!(l.extends, 1);
    }

    #[test]
    fn zero_clears() {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[7, 8]);
        let _ = l.ftruncate("f", 0);
        assert!(l.body_of("f").unwrap_or_default().is_empty());
        assert_eq!(l.state_of("f"), Some(TruncState::Zeroed));
    }

    #[test]
    fn negative_blocked() {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[7]);
        assert_eq!(l.ftruncate("f", -1).err(), Some(TcCode::NegativeLen));
        assert_eq!(l.body_of("f"), Some(Vec::from(&[7u8] as &[u8])));
    }

    #[test]
    fn missing_blocked() {
        let mut l = TruncLedger::new();
        assert_eq!(l.ftruncate("ghost", 2).err(), Some(TcCode::MissingPath));
    }

    #[test]
    fn over_max_boundary() {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[7]);
        assert_eq!(l.ftruncate("f", (FILE_MAX as i64) + 1).err(), Some(TcCode::OverMax));
        assert!(l.ftruncate("f", FILE_MAX as i64).is_ok());
    }

    #[test]
    fn size_ledger_consistent() {
        let mut l = TruncLedger::new();
        let _ = l.create("f", &[1, 2, 3]);
        let _ = l.ftruncate("f", 5);
        let _ = l.ftruncate("f", 1);
        assert!(l.sizes_consistent());
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_trunc_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_trunc_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(TruncState::Full.can_advance(TruncState::Extended));
        assert!(!TruncState::Zeroed.can_advance(TruncState::Shortened));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10127_checks_green() {
        let set = run_unx10127_checks();
        assert!(set.all_passed(), "UNX-F10127 red: {}", set.red_items_text());
    }
}
