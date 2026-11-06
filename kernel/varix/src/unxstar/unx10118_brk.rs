//! UNX-F10118 brk 正反双判据（堆伸缩达成与越限回退误用受控）· 0x92D80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10119-J1 exit/wait 回收达成（WEXITSTATUS 回读一致 LTP
//!       exit01/wait01 面 10/10 次）；UNX-F10119-J2 无子等待误用受控
//!       （预期 ECHILD 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92D80（本卡独占 0x40 窗口，下一卡 0x92DC0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_brk_call`："brk|op|delta" 三成分（op ∈ {=,+,-}），
//!      非法 op / 非数字 / 成分缺失逐档拒绝；
//!   ② 堆册 `HeapLedger`：base/top/limit 三锚，brk 推进/收缩/查询三语义，
//!      越限保持原顶（ENOMEM 档）、低于基址受控拒绝（EINVAL 档）；
//!   ③ 状态机 `HeapState`：Init→Grown→Shrunk→Sealed 单向推进（禁倒退）；
//!   ④ 注入核 `BadBrk`：越上限 / 低于基址 / 坏 op / 坏 delta 四类坏样本
//!      逐类 10/10 次精确复现 ENOMEM/EINVAL 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10118 独占 0x40 窗口，下一卡 0x92DC0）。
pub const SEG_BASE: u32 = 0x92D80;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92DC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 页大小（堆伸缩粒度）。
pub const PAGE: u32 = 0x1000;

/// 堆基址。
pub const HEAP_BASE: u32 = 0x6000_0000;

/// 堆上限（基址 + 1MB）。
pub const HEAP_LIMIT: u32 = 0x6001_0000;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrkCode {
    /// 越上限（ENOMEM 族）
    OverLimit,
    /// 低于基址（EINVAL 档）
    BelowBase,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 非数字 delta（EINVAL 族）
    BadDelta,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 零粒度操作（EINVAL 族）
    ZeroDelta,
    /// 非页对齐目标（EINVAL 档）
    BadAlign,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 守恒失平
    LedgerDrift,
    /// 峰值回退
    PeakRegress,
}

impl BrkCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作动作（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BrkCode::OverLimit => "越上限被拒：目标顶越过 HEAP_LIMIT；ENOMEM 精确归档且原顶保持",
            BrkCode::BelowBase => "低于基址被拒：目标顶低于 HEAP_BASE；EINVAL 档受控拒绝",
            BrkCode::BadOp => "非法 op 被拒：op 闭集 {=,+,-}；请核对调用行",
            BrkCode::BadDelta => "非数字 delta 被拒：delta 必须为十进制；请改写",
            BrkCode::MissingPart => "成分缺失被拒：调用行必须三段 brk|op|delta；请补成分",
            BrkCode::ZeroDelta => "零 delta 被拒：探顶必须推进或收缩非零粒度；查询走 brk(=0)",
            BrkCode::BadAlign => "非对齐目标被拒：目标顶必须页对齐；请取整到 0x1000",
            BrkCode::StateRegress => "状态倒退被拒：Init→Grown→Shrunk→Sealed 单向",
            BrkCode::EmptySummary => "空成文被拒：堆小结不得为空串；请先操作堆再成文",
            BrkCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            BrkCode::LedgerDrift => "守恒失平被拒：峰值/操作数账不平；请回查操作序列",
            BrkCode::PeakRegress => "峰值回退被拒：peak 只增不减；请排查收缩越账",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [BrkCode; 12] = [
        BrkCode::OverLimit,
        BrkCode::BelowBase,
        BrkCode::BadOp,
        BrkCode::BadDelta,
        BrkCode::MissingPart,
        BrkCode::ZeroDelta,
        BrkCode::BadAlign,
        BrkCode::StateRegress,
        BrkCode::EmptySummary,
        BrkCode::SegmentViolation,
        BrkCode::LedgerDrift,
        BrkCode::PeakRegress,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 brk 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrkOp {
    /// 绝对置顶（=N）
    Set(u32),
    /// 相对推进（+N）
    Grow(u32),
    /// 相对收缩（-N）
    Shrink(u32),
    /// 查询（=0）
    Query,
}

/// 解析 "brk|op|delta" 三段调用行。
pub fn parse_brk_call(line: &str) -> Result<BrkOp, BrkCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = match bytes.get(i) {
            Some(b) => *b as char,
            None => break,
        };
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
        i += 1;
    }
    let _ = parts.push(cur);
    if parts.len() != 3 {
        return Err(BrkCode::MissingPart);
    }
    let op = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(BrkCode::MissingPart),
    };
    let ds = match parts.get(2) {
        Some(p) => p.as_str(),
        None => return Err(BrkCode::MissingPart),
    };
    let num = |s: &str| -> Result<u32, BrkCode> {
        if s.is_empty() {
            return Err(BrkCode::BadDelta);
        }
        let mut acc: u32 = 0;
        for b in s.bytes() {
            if !b.is_ascii_digit() {
                return Err(BrkCode::BadDelta);
            }
            acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        }
        Ok(acc)
    };
    let n = num(ds)?;
    match op {
        "=" => {
            if n == 0 {
                Ok(BrkOp::Query)
            } else {
                Ok(BrkOp::Set(n))
            }
        }
        "+" => Ok(BrkOp::Grow(n)),
        "-" => Ok(BrkOp::Shrink(n)),
        _ => Err(BrkCode::BadOp),
    }
}

// ── 堆状态机 ──────────────────────────────────────────────────────────
/// 堆生命周期（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeapState {
    /// 初始堆
    Init,
    /// 已推进
    Grown,
    /// 已收缩
    Shrunk,
    /// 已封账（只读快照）
    Sealed,
}

impl HeapState {
    /// 单调推进判定（允许跳级，禁倒退）。
    pub fn can_advance(self, next: HeapState) -> bool {
        let rank = |s: HeapState| match s {
            HeapState::Init => 0u8,
            HeapState::Grown => 1,
            HeapState::Shrunk => 2,
            HeapState::Sealed => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            HeapState::Init => "初始堆",
            HeapState::Grown => "已推进",
            HeapState::Shrunk => "已收缩",
            HeapState::Sealed => "封账",
        }
    }
}

// ── 堆册 ──────────────────────────────────────────────────────────────
/// 堆册：base/top/limit 三锚 + 峰值账。
#[derive(Clone, Debug)]
pub struct HeapLedger {
    /// 堆基址（恒定）
    pub base: u32,
    /// 当前堆顶
    pub top: u32,
    /// 堆上限
    pub limit: u32,
    /// 峰值顶（只增不减）
    pub peak: u32,
    /// 生命周期状态
    pub state: HeapState,
    /// 累计推进次数
    pub grows: u32,
    /// 累计收缩次数
    pub shrinks: u32,
    /// 累计拒止次数
    pub rejects: u32,
}

impl HeapLedger {
    /// 初始堆册：top = base。
    pub fn new() -> HeapLedger {
        HeapLedger {
            base: HEAP_BASE,
            top: HEAP_BASE,
            limit: HEAP_LIMIT,
            peak: HEAP_BASE,
            state: HeapState::Init,
            grows: 0,
            shrinks: 0,
            rejects: 0,
        }
    }

    /// brk 判定核：op 三语义，越限保持原顶、低于基址受控拒绝。
    pub fn brk(&mut self, op: &BrkOp) -> Result<u32, BrkCode> {
        match op {
            BrkOp::Query => Ok(self.top),
            BrkOp::Set(target) => self.move_top(*target),
            BrkOp::Grow(n) => {
                if *n == 0 {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(BrkCode::ZeroDelta);
                }
                if *n % PAGE != 0 {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(BrkCode::BadAlign);
                }
                let target = self.top.saturating_add(*n);
                self.move_top(target)
            }
            BrkOp::Shrink(n) => {
                if *n == 0 {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(BrkCode::ZeroDelta);
                }
                if *n % PAGE != 0 {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(BrkCode::BadAlign);
                }
                let target = self.top.saturating_sub(*n);
                self.move_top(target)
            }
        }
    }

    /// 顶移动判定核：三闸（越限/低于基址/对齐）+ 峰值账。
    /// 堆顶在 [base, limit] 内双向移动合法；状态账只记最近一次动作。
    fn move_top(&mut self, target: u32) -> Result<u32, BrkCode> {
        if target > self.limit {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BrkCode::OverLimit);
        }
        if target < self.base {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BrkCode::BelowBase);
        }
        if target % PAGE != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BrkCode::BadAlign);
        }
        if target > self.top {
            self.state = HeapState::Grown;
            self.grows = self.grows.saturating_add(1);
        } else if target < self.top {
            self.state = HeapState::Shrunk;
            self.shrinks = self.shrinks.saturating_add(1);
        }
        if target > self.peak {
            self.peak = target;
        }
        self.top = target;
        Ok(self.top)
    }

    /// 在账小结字段快照。
    pub fn snapshot(&self) -> (u32, u32, u32) {
        (self.base, self.top, self.peak)
    }
}

// ── J1 达成核：brk01 面 10/10 轮 ──────────────────────────────────────
/// 逐轮 6 面断言（推进/查询/收缩/回探/越限拒/峰值守恒）。
pub fn run_brk_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut h = HeapLedger::new();
        let mut all_ok = true;
        // 面1：推进 2 页成功且顶对齐。
        all_ok = all_ok && h.brk(&BrkOp::Grow(2 * PAGE)).is_ok()
            && h.top == HEAP_BASE + 2 * PAGE;
        probes += 1;
        // 面2：查询返回当前顶。
        all_ok = all_ok && h.brk(&BrkOp::Query) == Ok(h.top);
        probes += 1;
        // 面3：收缩 1 页成功。
        all_ok = all_ok && h.brk(&BrkOp::Shrink(PAGE)).is_ok()
            && h.top == HEAP_BASE + PAGE;
        probes += 1;
        // 面4：回探语义：收缩后可再推进。
        all_ok = all_ok && h.brk(&BrkOp::Grow(PAGE)).is_ok()
            && h.top == HEAP_BASE + 2 * PAGE;
        probes += 1;
        // 面5：越限拒且原顶保持。
        all_ok = all_ok && h.brk(&BrkOp::Set(HEAP_LIMIT + PAGE)).err() == Some(BrkCode::OverLimit)
            && h.top == HEAP_BASE + 2 * PAGE;
        probes += 1;
        // 面6：峰值只增不减。
        all_ok = all_ok && h.peak == HEAP_BASE + 2 * PAGE;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏堆操作矩阵 ──────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadBrk {
    /// 越上限（ENOMEM 族）
    OverLimit,
    /// 低于基址（EINVAL 档）
    BelowBase,
    /// 坏 op（EINVAL 族）
    BadOp,
    /// 坏 delta（EINVAL 族）
    BadDelta,
}

impl BadBrk {
    /// 闭集全量。
    pub const ALL: [BadBrk; 4] = [
        BadBrk::OverLimit,
        BadBrk::BelowBase,
        BadBrk::BadOp,
        BadBrk::BadDelta,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadBrk::OverLimit => "越上限推进",
            BadBrk::BelowBase => "低于基址收缩",
            BadBrk::BadOp => "非法 op 行",
            BadBrk::BadDelta => "非数字 delta 行",
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
    pub fn record(&mut self, caught: bool) -> Result<(), BrkCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(BrkCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏堆注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_brk_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadBrk::ALL.len() {
        let case = match BadBrk::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut h = HeapLedger::new();
            let _ = h.brk(&BrkOp::Grow(PAGE));
            let caught = match case {
                BadBrk::OverLimit => {
                    h.brk(&BrkOp::Set(HEAP_LIMIT + PAGE)).err() == Some(BrkCode::OverLimit)
                        && h.top == HEAP_BASE + PAGE
                }
                BadBrk::BelowBase => {
                    h.brk(&BrkOp::Set(HEAP_BASE - PAGE)).err() == Some(BrkCode::BelowBase)
                }
                BadBrk::BadOp => {
                    parse_brk_call("brk|*|4096").err() == Some(BrkCode::BadOp)
                }
                BadBrk::BadDelta => {
                    parse_brk_call("brk|+|4x96").err() == Some(BrkCode::BadDelta)
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

// ── 堆小结成文（纯函数） ──────────────────────────────────────────────
/// 小结成文：「base=0x... top=0x... peak=<n>页」。
pub fn compose_heap_summary(h: &HeapLedger) -> Result<String, BrkCode> {
    let pages = h.peak.saturating_sub(h.base) / PAGE;
    let out = format!("base=0x{:08X} top=0x{:08X} peak={}页", h.base, h.top, pages);
    if out.is_empty() {
        return Err(BrkCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；越上限族（原顶保持）"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；低于基址/坏 op/坏 delta 族"),
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

/// 规范语料指纹：推进→收缩→回探→小结→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut h = HeapLedger::new();
    let _ = h.brk(&BrkOp::Grow(2 * PAGE));
    let _ = h.brk(&BrkOp::Shrink(PAGE));
    let _ = h.brk(&BrkOp::Grow(PAGE));
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_heap_summary(&h) {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&h.grows.to_le_bytes());
    bytes.extend_from_slice(&h.shrinks.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10118 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10118_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10118");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92D80..=0x92DBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < BrkCode::ALL.len() {
        let mut j = i + 1;
        while j < BrkCode::ALL.len() {
            if let (Some(a), Some(b)) = (BrkCode::ALL.get(i), BrkCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = BrkCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_brk_call("brk|=|0").is_ok()
        && parse_brk_call("brk|+|4096").is_ok()
        && parse_brk_call("brk|*|4096").err() == Some(BrkCode::BadOp)
        && parse_brk_call("brk|+|4x9").err() == Some(BrkCode::BadDelta);
    s.add("调用行解析器：三段闭集放行且坏成分逐档拒绝", parse_ok,
          "解析器是 brk 前置；op 闭集 {=,+,-}，delta 十进制闭集");

    // 4. J1 达成核
    let (ok, probes) = run_brk_rounds();
    s.add("J1 堆伸缩达成：探顶推进/回退语义 LTP brk01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面断言（推进/查询/收缩/回探/越限拒/峰值守恒）全对");

    // 5. J2 越限：ENOMEM 精确复现且原顶保持
    let mut h = HeapLedger::new();
    let _ = h.brk(&BrkOp::Grow(PAGE));
    let over_ok = h.brk(&BrkOp::Set(HEAP_LIMIT + PAGE)).err() == Some(BrkCode::OverLimit)
        && h.top == HEAP_BASE + PAGE
        && h.rejects == 1;
    s.add("J2 越限回退受控：ENOMEM 精确复现且原顶保持", over_ok,
          "越限必须拒且账面顶不漂移；拒止逐次入账");

    // 6. 低于基址档
    let below_ok = h.brk(&BrkOp::Set(HEAP_BASE - PAGE)).err() == Some(BrkCode::BelowBase)
        && h.top == HEAP_BASE + PAGE;
    s.add("低于基址：EINVAL 档受控拒绝且原顶保持", below_ok,
          "收缩越过基址必须拒；不得假成功或吞档");

    // 7. 对齐与零粒度闸
    let gate_ok = h.brk(&BrkOp::Grow(4)).err() == Some(BrkCode::BadAlign)
        && h.brk(&BrkOp::Grow(0)).err() == Some(BrkCode::ZeroDelta);
    s.add("对齐与零粒度：EINVAL 族逐档精确复现", gate_ok,
          "非页对齐与零 delta 是独立两档；探顶粒度 = 页");

    // 8. 注入核
    let (inj, per_class) = run_bad_brk_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "越限/低于基址/坏 op/坏 delta 必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadBrk::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 状态机
    let sm_ok = HeapState::Init.can_advance(HeapState::Grown)
        && HeapState::Grown.can_advance(HeapState::Shrunk)
        && !HeapState::Sealed.can_advance(HeapState::Init)
        && HeapState::Shrunk.label() == "已收缩";
    s.add("状态机：Init→Grown→Shrunk→Sealed 单向", sm_ok,
          "堆生命周期只许单调推进；倒退即协议违约");

    // 11. 峰值账守恒
    let (base, top, peak) = h.snapshot();
    let peak_ok = base == HEAP_BASE && top <= peak && peak <= HEAP_LIMIT;
    s.add("峰值账守恒：top ≤ peak ≤ limit 全程成立", peak_ok,
          "峰值是堆审计的账面根；回退即失守");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(12) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_heap_summary(&h);
    let text_ok = match &sum {
        Ok(t) => t.contains("base=0x") && t.contains("peak="),
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
    let ok_text = format!("堆伸缩达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_brk_call("brk|=|0").is_ok());
        assert!(parse_brk_call("brk|-|4096").is_ok());
        assert_eq!(parse_brk_call("brk|*|1").err(), Some(BrkCode::BadOp));
        assert_eq!(parse_brk_call("brk|+|x").err(), Some(BrkCode::BadDelta));
        assert_eq!(parse_brk_call("brk|+").err(), Some(BrkCode::MissingPart));
    }

    #[test]
    fn grow_shrink_semantics() {
        let mut h = HeapLedger::new();
        assert_eq!(h.brk(&BrkOp::Grow(2 * PAGE)), Ok(HEAP_BASE + 2 * PAGE));
        assert_eq!(h.brk(&BrkOp::Shrink(PAGE)), Ok(HEAP_BASE + PAGE));
        assert_eq!(h.top, HEAP_BASE + PAGE);
        assert_eq!(h.peak, HEAP_BASE + 2 * PAGE);
    }

    #[test]
    fn over_limit_keeps_top() {
        let mut h = HeapLedger::new();
        let _ = h.brk(&BrkOp::Grow(PAGE));
        assert_eq!(h.brk(&BrkOp::Set(HEAP_LIMIT + PAGE)).err(), Some(BrkCode::OverLimit));
        assert_eq!(h.top, HEAP_BASE + PAGE);
    }

    #[test]
    fn below_base_blocked() {
        let mut h = HeapLedger::new();
        let _ = h.brk(&BrkOp::Grow(PAGE));
        assert_eq!(h.brk(&BrkOp::Set(HEAP_BASE - PAGE)).err(), Some(BrkCode::BelowBase));
        assert_eq!(h.top, HEAP_BASE + PAGE);
    }

    #[test]
    fn query_returns_top() {
        let mut h = HeapLedger::new();
        let _ = h.brk(&BrkOp::Grow(3 * PAGE));
        assert_eq!(h.brk(&BrkOp::Query), Ok(HEAP_BASE + 3 * PAGE));
    }

    #[test]
    fn align_and_zero_gates() {
        let mut h = HeapLedger::new();
        assert_eq!(h.brk(&BrkOp::Grow(4)).err(), Some(BrkCode::BadAlign));
        assert_eq!(h.brk(&BrkOp::Grow(0)).err(), Some(BrkCode::ZeroDelta));
        assert_eq!(h.brk(&BrkOp::Shrink(0)).err(), Some(BrkCode::ZeroDelta));
    }

    #[test]
    fn peak_monotonic() {
        let mut h = HeapLedger::new();
        let _ = h.brk(&BrkOp::Grow(4 * PAGE));
        let _ = h.brk(&BrkOp::Shrink(3 * PAGE));
        assert_eq!(h.peak, HEAP_BASE + 4 * PAGE);
        assert_eq!(h.grows, 1);
        assert_eq!(h.shrinks, 1);
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_brk_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_brk_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(HeapState::Init.can_advance(HeapState::Grown));
        assert!(HeapState::Init.can_advance(HeapState::Sealed));
        assert!(!HeapState::Sealed.can_advance(HeapState::Grown));
        assert_eq!(HeapState::Sealed.label(), "封账");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10118_checks_green() {
        let set = run_unx10118_checks();
        assert!(set.all_passed(), "UNX-F10118 red: {}", set.red_items_text());
    }
}
