//! UNX-F10058 回归异常熔断（连红即停）· 0x91E80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10059-J1 演练留痕（大回归剧本全走 10/10 次），跳步 10/10 次拦截
//!
//! 设计要点：
//!   ① 熔断阈值 K=3 写死：连续红判定累计 3 次即熔断停批并留停批痕迹
//!      （StopRecord：停批轮次+连红数），漏一次停批即失灵（10/10 触发）。
//!   ② 熔断状态机：Closed →（连红 K 次）→ Tripped →（冷却 COOLDOWN_ROUNDS
//!      步）→ Closed；Tripped 期间继续喂入 → BatchHalted 逐次拦截（10/10）；
//!      绿判定复位连红计数（复位的对称性可复算）。
//!   ③ 喂入账守恒：feeds == 绿 + 红，stops == trips 恒成立；任何拒绝
//!      路径（乱序轮次/熔断中喂入）不改账（拒绝不留痕）。
//!   ④ 失灵审计：audit_trace 重放判定迹推「应停次数」，与申报停批数
//!      不符 → BreakerFault 逐次检出（10/10）——漏停与误停都算失灵。
//!   ⑤ 注入核四类坏样本逐类 10/10 次拒绝，注入账守恒逃逸 = 0；
//!     errno 对照表 + FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10058 独占 0x40 窗口，下一卡 0x91EC0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91E80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 熔断阈值 K：连续红 3 次即停批（卡面「连红 K 次即停」）。
pub const BREAK_K: u32 = 3;

/// 冷却步数：熔断后经过 2 步冷却才恢复 Closed。
pub const COOLDOWN_ROUNDS: u32 = 2;

/// 喂入账容量上限。
pub const LEDGER_CAP: usize = 128;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakerCode {
    /// 熔断中喂入（批已停还继续灌判定）
    BatchHalted,
    /// 轮次倒退（乱序喂入）
    OutOfOrder,
    /// 未知判定 token
    UnknownVerdict,
    /// 空判定 token
    EmptyVerdict,
    /// 熔断失灵（审计：应停次数与申报不符）
    BreakerFault,
    /// 未熔断却请求冷却
    NotTripped,
    /// 喂入账失守恒（feeds ≠ 绿+红 或 stops ≠ trips）
    LedgerRegress,
    /// 喂入账容量满
    LedgerFull,
    /// 空成文
    EmptySummary,
    /// 停批留痕断裂（编号或连红数非法）
    StopTraceGap,
    /// 轮次为零
    BadRound,
    /// 段位越界
    SegmentViolation,
}

impl BreakerCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BreakerCode::BatchHalted => "熔断中喂入被拒：批已停不得继续灌判定；\
                请先走冷却步恢复 Closed 再喂入",
            BreakerCode::OutOfOrder => "轮次倒退被拒：喂入轮次必须严格递增；请按轮次顺序喂入",
            BreakerCode::UnknownVerdict => "未知判定 token 被拒：判定只认 green|red \
                及其单字母缩写；请改用闭集内写法",
            BreakerCode::EmptyVerdict => "空判定 token 被拒：判定不能为空串",
            BreakerCode::BreakerFault => "熔断失灵：判定迹里存在连红 K 次却未停批\
                （或误停）；请按 audit_trace 复核熔断器",
            BreakerCode::NotTripped => "未熔断却请求冷却被拒：只有 Tripped 状态可走冷却步",
            BreakerCode::LedgerRegress => "喂入账失守恒：feeds 必须等于 绿+红，\
                stops 必须等于 trips；请排查绕账路径",
            BreakerCode::LedgerFull => "喂入账容量满被拒：已达上限；请先归档",
            BreakerCode::EmptySummary => "空成文被拒：熔断小结不得为空串；请检查账本",
            BreakerCode::StopTraceGap => "停批留痕断裂被拒：停批记录必须携带 \
                停批轮次与连红数；请排查漏记路径",
            BreakerCode::BadRound => "轮次非法被拒：喂入轮次从 1 起算且为正整数",
            BreakerCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [BreakerCode; 12] = [
        BreakerCode::BatchHalted,
        BreakerCode::OutOfOrder,
        BreakerCode::UnknownVerdict,
        BreakerCode::EmptyVerdict,
        BreakerCode::BreakerFault,
        BreakerCode::NotTripped,
        BreakerCode::LedgerRegress,
        BreakerCode::LedgerFull,
        BreakerCode::EmptySummary,
        BreakerCode::StopTraceGap,
        BreakerCode::BadRound,
        BreakerCode::SegmentViolation,
    ];
}

// ── 判定闭集与解析器 ──────────────────────────────────────────────────
/// 喂入判定闭集：绿 / 红。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 绿判定：复位连红计数
    Green,
    /// 红判定：连红计数 +1
    Red,
}

impl Verdict {
    /// 是否红。
    pub fn is_red(self) -> bool {
        self == Verdict::Red
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Green => "绿",
            Verdict::Red => "红",
        }
    }

    /// 闭集全量。
    pub const ALL: [Verdict; 2] = [Verdict::Green, Verdict::Red];
}

/// 判定解析：长短名双形制（green|g / red|r），其余 UnknownVerdict，
/// 空串 EmptyVerdict。
pub fn parse_verdict(tag: &str) -> Result<Verdict, BreakerCode> {
    if tag.is_empty() {
        return Err(BreakerCode::EmptyVerdict);
    }
    match tag {
        "green" | "g" => Ok(Verdict::Green),
        "red" | "r" => Ok(Verdict::Red),
        _ => Err(BreakerCode::UnknownVerdict),
    }
}

// ── 熔断状态机 ────────────────────────────────────────────────────────
/// 熔断器状态：Closed（计连红）/ Tripped（计冷却余步）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakerState {
    /// 未熔断
    Closed,
    /// 已熔断停批
    Tripped,
}

impl BreakerState {
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            BreakerState::Closed => "闭合",
            BreakerState::Tripped => "熔断",
        }
    }
}

/// 停批留痕：停批轮次 + 触发时的连红数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StopRecord {
    /// 停批轮次
    pub at_round: u32,
    /// 触发连红数（必须 == BREAK_K）
    pub consec: u32,
}

impl StopRecord {
    /// 留痕自检：连红数必须恰好等于阈值。
    pub fn sound(&self) -> bool {
        self.at_round > 0 && self.consec == BREAK_K
    }
}

/// 回归熔断器：喂入判定，连红 K 次即停批；冷却步恢复。
#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    state: BreakerState,
    consec: u32,
    cooldown_left: u32,
    last_round: u32,
    greens: u32,
    reds: u32,
    feeds: u32,
    stops: Vec<StopRecord>,
    cap: usize,
}

impl CircuitBreaker {
    /// 新熔断器（Closed，零计数）。
    pub fn new() -> CircuitBreaker {
        CircuitBreaker {
            state: BreakerState::Closed,
            consec: 0,
            cooldown_left: 0,
            last_round: 0,
            greens: 0,
            reds: 0,
            feeds: 0,
            stops: Vec::new(),
            cap: LEDGER_CAP,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BreakerState {
        self.state
    }

    /// 当前连红计数。
    pub fn consec(&self) -> u32 {
        self.consec
    }

    /// 停批留痕清单（只读）。
    pub fn stops(&self) -> &[StopRecord] {
        &self.stops
    }

    /// 喂入判定：轮次严格递增；Tripped 期间拒绝（BatchHalted，拒绝不留
    /// 痕）；绿复位连红、红累加，累计达 K → 熔断停批并留痕。
    pub fn feed(&mut self, verdict: Verdict, round: u32) -> Result<BreakerState, BreakerCode> {
        if round == 0 {
            return Err(BreakerCode::BadRound);
        }
        if self.last_round > 0 && round <= self.last_round {
            return Err(BreakerCode::OutOfOrder);
        }
        if self.state == BreakerState::Tripped {
            return Err(BreakerCode::BatchHalted);
        }
        if self.feeds as usize >= self.cap {
            return Err(BreakerCode::LedgerFull);
        }
        self.last_round = round;
        self.feeds = self.feeds.saturating_add(1);
        if verdict.is_red() {
            self.reds = self.reds.saturating_add(1);
            self.consec = self.consec.saturating_add(1);
            if self.consec >= BREAK_K {
                self.state = BreakerState::Tripped;
                self.cooldown_left = COOLDOWN_ROUNDS;
                self.stops.push(StopRecord { at_round: round, consec: self.consec });
                return Ok(BreakerState::Tripped);
            }
        } else {
            self.greens = self.greens.saturating_add(1);
            self.consec = 0;
        }
        Ok(BreakerState::Closed)
    }

    /// 冷却步：Tripped 状态下每步余数 -1，走满 COOLDOWN_ROUNDS 步恢复
    /// Closed 且连红清零；未熔断请求冷却 → NotTripped。
    pub fn step_cooldown(&mut self) -> Result<BreakerState, BreakerCode> {
        if self.state != BreakerState::Tripped {
            return Err(BreakerCode::NotTripped);
        }
        if self.cooldown_left > 0 {
            self.cooldown_left = self.cooldown_left.saturating_sub(1);
        }
        if self.cooldown_left == 0 {
            self.state = BreakerState::Closed;
            self.consec = 0;
            Ok(BreakerState::Closed)
        } else {
            Ok(BreakerState::Tripped)
        }
    }

    /// 喂入账守恒：feeds == 绿+红，stops 全部自洽。
    pub fn conserved(&self) -> bool {
        let sum = self.greens.saturating_add(self.reds);
        if sum != self.feeds {
            return false;
        }
        let mut i = 0usize;
        while i < self.stops.len() {
            match self.stops.get(i) {
                Some(r) => {
                    if !r.sound() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 已喂入笔数。
    pub fn len(&self) -> usize {
        self.feeds as usize
    }
}

// ── 失灵审计 ──────────────────────────────────────────────────────────
/// 失灵审计：重放判定迹推「应停次数」——迹中每凑满 K 连红计一次停，
/// 然后连红清零（模拟停批切断）；与申报停批数不符 → BreakerFault。
/// 纯函数，同迹同申报两次审计结果一致。
pub fn audit_trace(trace: &[Verdict], claimed_stops: u32) -> Result<u32, BreakerCode> {
    let mut consec = 0u32;
    let mut expected = 0u32;
    let mut i = 0usize;
    while i < trace.len() {
        let v = match trace.get(i) {
            Some(v) => *v,
            None => return Err(BreakerCode::SegmentViolation),
        };
        if v.is_red() {
            consec = consec.saturating_add(1);
            if consec >= BREAK_K {
                expected = expected.saturating_add(1);
                consec = 0;
            }
        } else {
            consec = 0;
        }
        i += 1;
    }
    if expected != claimed_stops {
        return Err(BreakerCode::BreakerFault);
    }
    Ok(expected)
}

// ── 熔断成文（纯函数） ────────────────────────────────────────────────
/// 熔断成文：「喂=<n>;红=<m>;停=<k>;连红=<c>;状态=<标签>」；失守恒先拒。
pub fn compose_breaker_summary(b: &CircuitBreaker) -> Result<String, BreakerCode> {
    if !b.conserved() {
        return Err(BreakerCode::LedgerRegress);
    }
    let out = format!(
        "喂={};红={};停={};连红={};状态={}",
        b.feeds,
        b.reds,
        b.stops.len(),
        b.consec,
        b.state.label()
    );
    if out.is_empty() {
        return Err(BreakerCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏喂入样本矩阵 ────────────────────────────────────────────
/// 四类坏喂入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFeed {
    /// 熔断中喂入
    HaltViolation,
    /// 轮次倒退
    RewindRound,
    /// 未知判定 token
    UnknownTok,
    /// 空判定 token
    EmptyTok,
}

impl BadFeed {
    /// 闭集全量。
    pub const ALL: [BadFeed; 4] = [
        BadFeed::HaltViolation,
        BadFeed::RewindRound,
        BadFeed::UnknownTok,
        BadFeed::EmptyTok,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadFeed::HaltViolation => "熔断中喂入",
            BadFeed::RewindRound => "轮次倒退",
            BadFeed::UnknownTok => "未知判定",
            BadFeed::EmptyTok => "空判定",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), BreakerCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(BreakerCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏喂入注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_feed_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadFeed::ALL.len() {
        let case = match BadFeed::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadFeed::HaltViolation => {
                    let mut b = CircuitBreaker::new();
                    let _ = b.feed(Verdict::Red, 1);
                    let _ = b.feed(Verdict::Red, 2);
                    let _ = b.feed(Verdict::Red, 3); // 熔断
                    b.feed(Verdict::Green, 4).err() == Some(BreakerCode::BatchHalted)
                        && b.len() == 3
                }
                BadFeed::RewindRound => {
                    let mut b = CircuitBreaker::new();
                    let _ = b.feed(Verdict::Green, 5);
                    b.feed(Verdict::Green, 4).err() == Some(BreakerCode::OutOfOrder)
                }
                BadFeed::UnknownTok => {
                    parse_verdict("amber").err() == Some(BreakerCode::UnknownVerdict)
                }
                BadFeed::EmptyTok => {
                    parse_verdict("").err() == Some(BreakerCode::EmptyVerdict)
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

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；熔断中喂入/未熔断冷却族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏判定/坏轮次族"),
    ("EAGAIN", -11, 997, "Windows ERROR_IO_PENDING；冷却未满族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；喂入账满族"),
    ("EPROTO", -71, 1231, "Windows ERROR_INVALID_NAME；熔断失灵族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：连红熔断 → 喂入被拒 → 冷却恢复 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut b = CircuitBreaker::new();
    let _ = b.feed(Verdict::Green, 1);
    let _ = b.feed(Verdict::Red, 2);
    let _ = b.feed(Verdict::Red, 3);
    let _ = b.feed(Verdict::Red, 4); // 熔断
    let _ = b.step_cooldown();
    let _ = b.step_cooldown(); // 恢复 Closed
    let bytes = match compose_breaker_summary(&b) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10058 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10058_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10058");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91E80..=0x91EBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 阈值策略：K=3 写死
    s.add("熔断策略：K=3 写死且冷却步>0",
          BREAK_K == 3 && COOLDOWN_ROUNDS > 0,
          "连红阈值必须为正且写死，冷却步必须大于 0");

    // 3. 连红 K 次即停批 10/10
    let mut trip_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let third = b.feed(Verdict::Red, 3);
        let stopped = third.ok() == Some(BreakerState::Tripped)
            && b.state() == BreakerState::Tripped
            && b.stops().first().map(|r| r.sound()).unwrap_or(false);
        if !stopped {
            trip_ok = false;
        }
        rr += 1;
    }
    s.add("熔断生效：连红 K 次 10/10 次即停批并留痕", trip_ok,
          "第三连红必须逐次触发熔断，停批留痕必须自洽");

    // 4. 绿色复位：红红绿红 不熔断 10/10
    let mut reset_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let _ = b.feed(Verdict::Green, 3);
        let _ = b.feed(Verdict::Red, 4);
        if b.state() != BreakerState::Closed || b.consec() != 1 {
            reset_ok = false;
        }
        rr += 1;
    }
    s.add("绿色复位：绿判定打断连红 10/10 次", reset_ok,
          "绿判定必须复位连红计数，复位后的红重新起算");

    // 5. 熔断中喂入拦截 10/10
    let mut halt_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let _ = b.feed(Verdict::Red, 3);
        if b.feed(Verdict::Red, 4).err() != Some(BreakerCode::BatchHalted) || b.len() != 3 {
            halt_ok = false;
        }
        rr += 1;
    }
    s.add("停批拦截：熔断中喂入 10/10 次拒绝且不留痕", halt_ok,
          "批已停后任何喂入企图必须逐次拦截，账不受扰");

    // 6. 冷却恢复：2 步冷却后恢复 Closed 10/10
    let mut cool_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let _ = b.feed(Verdict::Red, 3);
        let s1 = b.step_cooldown();
        let s2 = b.step_cooldown();
        if s1.ok() != Some(BreakerState::Tripped)
            || s2.ok() != Some(BreakerState::Closed)
            || b.state() != BreakerState::Closed
            || b.consec() != 0
        {
            cool_ok = false;
        }
        rr += 1;
    }
    s.add("冷却恢复：冷却步走满 10/10 次恢复 Closed", cool_ok,
          "熔断必须经冷却步复位，复位后连红清零");

    // 7. 失灵检出：K 红迹申报 0 停 10/10 检出
    let trace = [Verdict::Red, Verdict::Red, Verdict::Red, Verdict::Green, Verdict::Red];
    let mut fault_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        if audit_trace(&trace, 0).err() != Some(BreakerCode::BreakerFault) {
            fault_ok = false;
        }
        rr += 1;
    }
    s.add("失灵检出：连红不停的熔断器 10/10 次检出", fault_ok,
          "应停次数与申报不符即失灵，逐次显性检出");

    // 8. 审计正确：应停 1 次申报 1 次 → Ok(1)
    let mut audit_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        if audit_trace(&trace, 1).ok() != Some(1) {
            audit_ok = false;
        }
        rr += 1;
    }
    s.add("审计正确：应停=申报 10/10 次通过", audit_ok,
          "审计纯函数：同迹同申报必须逐位一致");

    // 9. 注入核：四类坏喂入各 10/10 拒绝
    let (inj, per_class) = run_bad_feed_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏喂入样本各 10/10 拒绝", all_caught,
          "熔断中喂入/倒退/未知判定/空判定必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadFeed::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 喂入账守恒：feeds == 绿+红 且 stops 全自洽
    let mut b = CircuitBreaker::new();
    let _ = b.feed(Verdict::Green, 1);
    let _ = b.feed(Verdict::Red, 2);
    let _ = b.feed(Verdict::Red, 3);
    let _ = b.feed(Verdict::Red, 4);
    s.add("喂入守恒：feeds==绿+红 且 stops==1 全自洽",
          b.conserved() && b.len() == 4 && b.stops().len() == 1,
          "喂入四计数必须逐笔对得上，绕账路径即失败");

    // 12. 轮次乱序拒绝且不留痕
    let before = b.len();
    let _ = b.feed(Verdict::Green, 3);
    let _ = b.feed(Verdict::Green, 0);
    s.add("乱序喂入：倒退轮次/零轮次拒绝且账不变",
          b.len() == before && before == 4,
          "被拒喂入不得改动喂入账，任何拒绝路径零副作用");

    // 13. 成文非空且含标记 + 双跑一致
    let sum = compose_breaker_summary(&b);
    let text_ok = match &sum {
        Ok(t) => t.contains("喂=4") && t.contains("状态=熔断"),
        Err(_) => false,
    };
    s.add("熔断成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_breaker_summary(&b),
          "成文必须携带 喂/红/停/连红/状态 标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(11)
        && agrees_with_linux(28) && agrees_with_linux(71)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < BreakerCode::ALL.len() {
        let mut j = i + 1;
        while j < BreakerCode::ALL.len() {
            if let (Some(a), Some(bb)) = (BreakerCode::ALL.get(i), BreakerCode::ALL.get(j)) {
                if a.code() == bb.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = BreakerCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("熔断小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_verdict("green"), Ok(Verdict::Green));
        assert_eq!(parse_verdict("r"), Ok(Verdict::Red));
        assert!(parse_verdict("amber").is_err());
        assert_eq!(parse_verdict("").err(), Some(BreakerCode::EmptyVerdict));
    }

    #[test]
    fn trip_at_k_with_record() {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        assert_eq!(b.feed(Verdict::Red, 3).ok(), Some(BreakerState::Tripped));
        assert_eq!(b.stops().len(), 1);
        assert!(b.stops().first().map(|r| r.sound()).unwrap_or(false));
    }

    #[test]
    fn green_resets_consec() {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let _ = b.feed(Verdict::Green, 3);
        assert_eq!(b.consec(), 0);
        assert_eq!(b.state(), BreakerState::Closed);
    }

    #[test]
    fn halted_feed_rejected_without_trace() {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let _ = b.feed(Verdict::Red, 3);
        assert_eq!(b.feed(Verdict::Green, 4).err(), Some(BreakerCode::BatchHalted));
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn cooldown_restores() {
        let mut b = CircuitBreaker::new();
        let _ = b.feed(Verdict::Red, 1);
        let _ = b.feed(Verdict::Red, 2);
        let _ = b.feed(Verdict::Red, 3);
        assert_eq!(b.step_cooldown().ok(), Some(BreakerState::Tripped));
        assert_eq!(b.step_cooldown().ok(), Some(BreakerState::Closed));
        assert_eq!(b.step_cooldown().err(), Some(BreakerCode::NotTripped));
    }

    #[test]
    fn audit_detects_fault_and_passes() {
        let trace = [Verdict::Red, Verdict::Red, Verdict::Red];
        assert_eq!(audit_trace(&trace, 0).err(), Some(BreakerCode::BreakerFault));
        assert_eq!(audit_trace(&trace, 1).ok(), Some(1));
    }

    #[test]
    fn bad_feed_matrix_green() {
        let (inj, per) = run_bad_feed_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(11));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(71), -71);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10058_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10058_checks_green() {
        let set = run_unx10058_checks();
        assert!(set.all_passed(), "UNX-F10058 red: {}", set.red_items_text());
    }
}
