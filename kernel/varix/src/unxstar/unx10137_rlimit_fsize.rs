//! UNX-F10137 RLIMIT_FSIZE 正反双判据（限值达成与超限写误用受控）· 0x93240 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10138-J1 掩码达成（umask 置位→创建文件 mode 按掩码清零
//!       10/10 次）；UNX-F10138-J2 掩码不可设置位误用受控（预期 EINVAL
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93240（本卡独占 0x40 窗口，下一卡 0x93280）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_rlimit_call`："op|arg" 两成分（op ∈ {setrlimit,
//!      write}），arg 允许带负号（负限值是 J2 注入对象），逐档拒绝；
//!   ② 限值账 `RlimitLog`：软限/硬限双锚，写至限位成功、超限 EFBIG 档
//!      （SIGXFSZ 按 L1 注记），软限 ≤ 硬限单向闸；
//!   ③ 状态机 `RlimState`：Infinite→Capped→AtLimit 单调（禁倒退）；
//!   ④ 注入核 `BadRlimit`：负限值 / 软限超硬限 / 未绑定写 / 坏调用行
//!      四类坏样本逐类 10/10 次精确复现 EINVAL/EPERM/EBADF 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10137 独占 0x40 窗口，下一卡 0x93280）。
pub const SEG_BASE: u32 = 0x93240;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93280;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 硬限上限。
pub const HARD_MAX: u32 = 1024;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlCode {
    /// 负限值（EINVAL 族）
    NegativeLimit,
    /// 软限超硬限（EPERM 族）
    SoftOverHard,
    /// 未绑定写（EBADF 族）
    NotOpen,
    /// 超软限写（EFBIG 族，SIGXFSZ 按 L1 注记）
    OverSoft,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 限值账失平
    LimitDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl RlCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RlCode::NegativeLimit => "负限值被拒：rlimit 不得为负；EINVAL 精确归档",
            RlCode::SoftOverHard => "软限超硬限被拒：非特权进程只许降不许升；EPERM 精确归档",
            RlCode::NotOpen => "未绑定写被拒：fd 未绑定限值面；EBADF 精确归档",
            RlCode::OverSoft => "超软限写被拒：越限位即拒；EFBIG 精确归档（SIGXFSZ 按 L1 注记）",
            RlCode::BadOp => "非法 op 被拒：op 闭集 {setrlimit, write}；请核对调用行",
            RlCode::BadNumber => "非数字成分被拒：arg 必须为十进制；请改写",
            RlCode::MissingPart => "成分缺失被拒：调用行必须两段 op|arg；请补成分",
            RlCode::StateRegress => "状态倒退被拒：Infinite→Capped→AtLimit 单调",
            RlCode::EmptySummary => "空成文被拒：限值小结不得为空串；请先置限再成文",
            RlCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            RlCode::LimitDrift => "限值账失平被拒：软限 ≤ 硬限必须全程成立",
            RlCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RlCode; 12] = [
        RlCode::NegativeLimit,
        RlCode::SoftOverHard,
        RlCode::NotOpen,
        RlCode::OverSoft,
        RlCode::BadOp,
        RlCode::BadNumber,
        RlCode::MissingPart,
        RlCode::StateRegress,
        RlCode::EmptySummary,
        RlCode::SegmentViolation,
        RlCode::LimitDrift,
        RlCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的限值调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RlimitCall {
    /// 动词
    pub op: String,
    /// 参数（可带负号）
    pub arg: i64,
}

/// 解析 "op|arg" 两段调用行。
pub fn parse_rlimit_call(line: &str) -> Result<RlimitCall, RlCode> {
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
    if parts.len() != 2 {
        return Err(RlCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(RlCode::MissingPart),
    };
    if op != "setrlimit" && op != "write" {
        return Err(RlCode::BadOp);
    }
    let as_ = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(RlCode::MissingPart),
    };
    if as_.is_empty() {
        return Err(RlCode::BadNumber);
    }
    let (neg, digits) = match as_.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, as_.as_str()),
    };
    if digits.is_empty() {
        return Err(RlCode::BadNumber);
    }
    let mut acc: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return Err(RlCode::BadNumber);
        }
        acc = acc.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    Ok(RlimitCall { op, arg: if neg { -acc } else { acc } })
}

// ── 限值状态机 ────────────────────────────────────────────────────────
/// 限值生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlimState {
    /// 无限
    Infinite,
    /// 已置限
    Capped,
    /// 已达限位
    AtLimit,
}

impl RlimState {
    /// 单调推进判定。
    pub fn can_advance(self, next: RlimState) -> bool {
        let rank = |s: RlimState| match s {
            RlimState::Infinite => 0u8,
            RlimState::Capped => 1,
            RlimState::AtLimit => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            RlimState::Infinite => "无限",
            RlimState::Capped => "已置限",
            RlimState::AtLimit => "已达限位",
        }
    }
}

// ── 限值账 ────────────────────────────────────────────────────────────
/// 限值账：软/硬双锚 + 文件长账。
#[derive(Clone, Debug)]
pub struct RlimitLog {
    /// 软限（EFBIG 触发线）
    pub soft: u32,
    /// 硬限（上限的天花板）
    pub hard: u32,
    /// 文件长
    pub size: u32,
    /// 生命周期状态
    pub state: RlimState,
    /// fd 绑定位
    pub bound: bool,
    /// 累计置限次数
    pub sets: u32,
    /// 累计超限拒止次数
    pub denials: u32,
}

impl RlimitLog {
    /// 初始账：无限（u32::MAX 软限）、硬限 HARD_MAX。
    pub fn new() -> RlimitLog {
        RlimitLog {
            soft: u32::MAX,
            hard: HARD_MAX,
            size: 0,
            state: RlimState::Infinite,
            bound: false,
            sets: 0,
            denials: 0,
        }
    }

    /// 绑定 fd 面。
    pub fn bind(&mut self) {
        self.bound = true;
    }

    /// setrlimit：负值/越硬限/软超硬逐闸；软限可降可回无限。
    pub fn setrlimit(&mut self, soft: i64) -> Result<u32, RlCode> {
        if soft < 0 {
            return Err(RlCode::NegativeLimit);
        }
        let s = soft as u64;
        if s > HARD_MAX as u64 {
            return Err(RlCode::SoftOverHard);
        }
        self.soft = s as u32;
        self.sets = self.sets.saturating_add(1);
        if self.state.can_advance(RlimState::Capped) {
            self.state = RlimState::Capped;
        }
        Ok(self.soft)
    }

    /// write：写至限位成功、超软限 EFBIG；未绑定拒。
    pub fn write(&mut self, n: u32) -> Result<u32, RlCode> {
        if !self.bound {
            return Err(RlCode::NotOpen);
        }
        if n == 0 {
            return Ok(self.size);
        }
        let target = self.size.saturating_add(n);
        if target > self.soft {
            self.denials = self.denials.saturating_add(1);
            return Err(RlCode::OverSoft);
        }
        self.size = target;
        if self.size == self.soft && self.state.can_advance(RlimState::AtLimit) {
            self.state = RlimState::AtLimit;
        }
        Ok(self.size)
    }

    /// 限值账审计：软 ≤ 硬全程成立。
    pub fn limits_consistent(&self) -> bool {
        self.soft <= self.hard || self.soft == u32::MAX
    }

    /// 读限值三元组。
    pub fn snapshot(&self) -> (u32, u32, u32) {
        (self.soft, self.hard, self.size)
    }
}

// ── J1 达成核：置限后写至限位成功 10/10 轮 ────────────────────────────
/// 逐轮：置限 16→写 16 成功（限位）→再写 1 拒→态 AtLimit。
pub fn run_rlimit_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = RlimitLog::new();
        g.bind();
        let mut all_ok = true;
        // 面1：置软限 16。
        all_ok = all_ok && g.setrlimit(16) == Ok(16);
        probes += 1;
        // 面2：写至限位 16 成功。
        all_ok = all_ok && g.write(16) == Ok(16);
        probes += 1;
        // 面3：态 AtLimit。
        all_ok = all_ok && g.state == RlimState::AtLimit;
        probes += 1;
        // 面4：再写 1 超限拒。
        all_ok = all_ok && g.write(1).err() == Some(RlCode::OverSoft);
        probes += 1;
        // 面5：限值账平。
        all_ok = all_ok && g.limits_consistent() && g.denials == 1;
        probes += 1;
        // 面6：降软限后写仍按新限。
        all_ok = all_ok && g.setrlimit(8).is_ok() && g.write(1).err() == Some(RlCode::OverSoft);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏限值矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRlimit {
    /// 负限值（EINVAL 族）
    NegativeLimit,
    /// 软限超硬限（EPERM 族）
    SoftOverHard,
    /// 未绑定写（EBADF 族）
    NotOpen,
    /// 坏调用行（EINVAL 族）
    BadCall,
}

impl BadRlimit {
    /// 闭集全量。
    pub const ALL: [BadRlimit; 4] = [
        BadRlimit::NegativeLimit,
        BadRlimit::SoftOverHard,
        BadRlimit::NotOpen,
        BadRlimit::BadCall,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadRlimit::NegativeLimit => "负限值置限",
            BadRlimit::SoftOverHard => "软限超硬限",
            BadRlimit::NotOpen => "未绑定写",
            BadRlimit::BadCall => "坏调用行",
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
    pub fn record(&mut self, caught: bool) -> Result<(), RlCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RlCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏限值注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_rlimit_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRlimit::ALL.len() {
        let case = match BadRlimit::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut g = RlimitLog::new();
            let caught = match case {
                BadRlimit::NegativeLimit => {
                    g.setrlimit(-1).err() == Some(RlCode::NegativeLimit)
                }
                BadRlimit::SoftOverHard => {
                    g.setrlimit((HARD_MAX as i64) + 1).err() == Some(RlCode::SoftOverHard)
                }
                BadRlimit::NotOpen => {
                    g.write(1).err() == Some(RlCode::NotOpen)
                }
                BadRlimit::BadCall => {
                    parse_rlimit_call("getrlimit|8").err() == Some(RlCode::BadOp)
                        && parse_rlimit_call("setrlimit|x").err() == Some(RlCode::BadNumber)
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

// ── 限值小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「soft=<n> hard=<h> size=<s>」。
pub fn compose_rlimit_summary(g: &RlimitLog) -> Result<String, RlCode> {
    let (soft, hard, size) = g.snapshot();
    let out = format!("soft={} hard={} size={}", soft, hard, size);
    if out.is_empty() {
        return Err(RlCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFBIG", -27, 223, "Windows ERROR_FILE_TOO_LARGE；超软限写族（SIGXFSZ 按 L1）"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；负限值/坏行族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；软限超硬限族"),
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；未绑定写族"),
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

/// 规范语料指纹：置限→写至限位→超限拒→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = RlimitLog::new();
    g.bind();
    let _ = g.setrlimit(32);
    let _ = g.write(32);
    let _ = g.write(1);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_rlimit_summary(&g) {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&g.denials.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10137 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10137_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10137");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93240..=0x9327F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < RlCode::ALL.len() {
        let mut j = i + 1;
        while j < RlCode::ALL.len() {
            if let (Some(a), Some(b)) = (RlCode::ALL.get(i), RlCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = RlCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_rlimit_call("setrlimit|64").is_ok()
        && parse_rlimit_call("setrlimit|-1").is_ok()
        && parse_rlimit_call("getrlimit|8").err() == Some(RlCode::BadOp)
        && parse_rlimit_call("setrlimit|8x").err() == Some(RlCode::BadNumber);
    s.add("调用行解析器：op 双词闭集放行且坏成分逐档拒绝", parse_ok,
          "负限值解析层不吞（判定层拒）");

    // 4. J1 达成核
    let (ok, probes) = run_rlimit_rounds();
    s.add("J1 限值达成：prlimit 置限后写至限位成功 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（置限/限位写/态推进/超限拒/账平/降限生效）全对");

    // 5. J2 超限写
    let mut g = RlimitLog::new();
    g.bind();
    let _ = g.setrlimit(8);
    let _ = g.write(8);
    let over_ok = g.write(1).err() == Some(RlCode::OverSoft)
        && g.size == 8 && g.denials == 1;
    s.add("J2 超限写受控：EFBIG 精确复现且体不越限（SIGXFSZ 按 L1 注记）",
          over_ok,
          "限位即墙；被拒写不得推进体长");

    // 6. 限位边界：恰好限位成功
    let mut g2 = RlimitLog::new();
    g2.bind();
    let _ = g2.setrlimit(4);
    let edge_ok = g2.write(4).is_ok() && g2.size == 4;
    s.add("限位边界：写至恰好限位成功", edge_ok,
          "== soft 是合法终位；> soft 才拒");

    // 7. 软限超硬限档
    let eperm_ok = g.setrlimit((HARD_MAX as i64) + 1).err() == Some(RlCode::SoftOverHard)
        && g.setrlimit(-1).err() == Some(RlCode::NegativeLimit);
    s.add("置限双闸：负值 EINVAL 与越硬限 EPERM 独立精确复现", eperm_ok,
          "两档互不吞档");

    // 8. 限值账平
    let cons_ok = g.limits_consistent() && g.snapshot().0 == 8;
    s.add("限值账平：软 ≤ 硬全程成立", cons_ok,
          "LimitDrift 是限值语义根缺陷");

    // 9. 注入核
    let (inj, per_class) = run_bad_rlimit_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "负限值/越硬限/未绑定/坏行必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRlimit::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = RlimState::Infinite.can_advance(RlimState::AtLimit)
        && RlimState::Capped.can_advance(RlimState::AtLimit)
        && !RlimState::AtLimit.can_advance(RlimState::Infinite)
        && RlimState::AtLimit.label() == "已达限位";
    s.add("状态机：Infinite→Capped→AtLimit 单调", sm_ok,
          "限值生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(27) && agrees_with_linux(22)
        && agrees_with_linux(1) && agrees_with_linux(9)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_rlimit_summary(&g);
    let text_ok = match &sum {
        Ok(x) => x.contains("soft=8") && x.contains("size=8"),
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
    let ok_text = format!("限值达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_rlimit_call("setrlimit|64").is_ok());
        assert_eq!(parse_rlimit_call("getrlimit|8").err(), Some(RlCode::BadOp));
        assert_eq!(parse_rlimit_call("setrlimit|8x").err(), Some(RlCode::BadNumber));
        let c = parse_rlimit_call("setrlimit|-2").unwrap_or(RlimitCall { op: String::new(), arg: 0 });
        assert_eq!(c.arg, -2);
    }

    #[test]
    fn write_to_limit_ok() {
        let mut g = RlimitLog::new();
        g.bind();
        let _ = g.setrlimit(16);
        assert_eq!(g.write(16), Ok(16));
        assert_eq!(g.state, RlimState::AtLimit);
    }

    #[test]
    fn over_soft_blocked() {
        let mut g = RlimitLog::new();
        g.bind();
        let _ = g.setrlimit(8);
        let _ = g.write(8);
        assert_eq!(g.write(1).err(), Some(RlCode::OverSoft));
        assert_eq!(g.size, 8);
    }

    #[test]
    fn negative_limit_blocked() {
        let mut g = RlimitLog::new();
        assert_eq!(g.setrlimit(-1).err(), Some(RlCode::NegativeLimit));
    }

    #[test]
    fn soft_over_hard_blocked() {
        let mut g = RlimitLog::new();
        assert_eq!(g.setrlimit((HARD_MAX as i64) + 1).err(), Some(RlCode::SoftOverHard));
        assert!(g.setrlimit(HARD_MAX as i64).is_ok());
    }

    #[test]
    fn unbound_write_blocked() {
        let mut g = RlimitLog::new();
        assert_eq!(g.write(1).err(), Some(RlCode::NotOpen));
    }

    #[test]
    fn lower_limit_takes_effect() {
        let mut g = RlimitLog::new();
        g.bind();
        let _ = g.setrlimit(16);
        let _ = g.write(16);
        let _ = g.setrlimit(8);
        assert_eq!(g.write(1).err(), Some(RlCode::OverSoft));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_rlimit_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_rlimit_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(RlimState::Infinite.can_advance(RlimState::Capped));
        assert!(!RlimState::AtLimit.can_advance(RlimState::Capped));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10137_checks_green() {
        let set = run_unx10137_checks();
        assert!(set.all_passed(), "UNX-F10137 red: {}", set.red_items_text());
    }
}
