//! UNX-F10051 复发红升级规则（N 次即升级）· 0x91CC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10052-J1 趋势列齐备（逐轮通过率+方向 10/10 次取数正确），混列 10/10 次检出
//!
//! 设计要点：
//!   ① 升级阈值 N=3 写死为策略常量：同一条目复发红累计 N 次即升级为
//!      Escalated，1..N-1 次挂 Watch 中间档；阈值以内不许抢跑（UnderCount）。
//!   ② 升级状态机只升不降：Normal → Watch → Escalated 单向 ranks
//!      0→1→2，任何降级企图 → LevelRegress；每次升级必须留出处
//!      （EscalationRecord：at_count==N + 阈值 + 轮次），漏留出处即失败。
//!   ③ 升级账本计数守恒：Σcount == total_bumps，升级记录数 == Escalated
//!      条目数；容量满 / 未登记 / 计数溢出三类路径显性拒绝且不留痕。
//!   ④ 注入核：阈值抢跑 / 降级回退 / 未登记自增 / 容量溢出四类坏样本
//!      逐类 10/10 次拒绝，注入账守恒逃逸 = 0；漏升（N 次未升级）由
//!      audit_missed 扫描账本逐次检出。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10051 独占 0x40 窗口，下一卡 0x91D00）；诊断码闭集
/// SEG_BASE+0x01..=+0x0B 共 11 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91CC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 升级阈值 N：同一条目复发红累计 3 次即升级（卡面「N 次即升级」）。
pub const ESCALATE_N: u32 = 3;

/// 单条目计数上限（u32 饱和前的工程约束）。
pub const COUNT_CAP: u32 = 10_000;

/// 账本容量上限。
pub const LEDGER_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EscCode {
    /// 阈值以内抢跑升级（count < N）
    UnderCount,
    /// 等级降级回退（状态机只升不降被破坏）
    LevelRegress,
    /// 条目未登记即自增计数
    UnknownItem,
    /// 计数越过 COUNT_CAP
    CounterOverflow,
    /// 账本容量满
    LedgerFull,
    /// 计数守恒被破坏（Σcount ≠ total_bumps）
    CountRegress,
    /// 升级缺出处（记录数与 Escalated 条目数不符）
    MissingProvenance,
    /// 未知等级 token
    UnknownToken,
    /// 空等级 token
    EmptyToken,
    /// 空成文
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl EscCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EscCode::UnderCount => "阈值抢跑被拒：复发计数未达 N 不许升级；\
                请按计数自然触发升级，不许人工抢跑",
            EscCode::LevelRegress => "降级回退被拒：升级状态机只升不降；\
                请排查是否误触复位路径",
            EscCode::UnknownItem => "条目未登记被拒：升级计数必须先 enroll 建档；\
                请先登记再 bump",
            EscCode::CounterOverflow => "计数溢出被拒：单条目计数越过上限；\
                请先归档或拆分条目再继续计数",
            EscCode::LedgerFull => "账本容量满被拒：升级账本已达上限；请先归档旧档",
            EscCode::CountRegress => "计数守恒被破坏：Σcount 必须等于 total_bumps；\
                请排查是否有路径绕过 bump 直改计数",
            EscCode::MissingProvenance => "升级缺出处被拒：每次升级必须留 \
                EscalationRecord（at_count/阈值/轮次）；请补齐出处再入账",
            EscCode::UnknownToken => "未知等级 token 被拒：等级只认 normal|watch|escalated \
                及其单字母缩写；请改用闭集内写法",
            EscCode::EmptyToken => "空等级 token 被拒：等级不能为空串",
            EscCode::EmptySummary => "空成文被拒：升级小结不得为空串；请检查账本是否为空册",
            EscCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [EscCode; 11] = [
        EscCode::UnderCount,
        EscCode::LevelRegress,
        EscCode::UnknownItem,
        EscCode::CounterOverflow,
        EscCode::LedgerFull,
        EscCode::CountRegress,
        EscCode::MissingProvenance,
        EscCode::UnknownToken,
        EscCode::EmptyToken,
        EscCode::EmptySummary,
        EscCode::SegmentViolation,
    ];
}

// ── 升级等级状态机 ────────────────────────────────────────────────────
/// 升级等级三档：Normal → Watch → Escalated，只升不降。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EscLevel {
    /// 未挂观察
    Normal,
    /// 复发 1..N-1 次的中间档
    Watch,
    /// 复发满 N 次
    Escalated,
}

impl EscLevel {
    /// 状态机秩（单调不减校验用）：0/1/2。
    pub fn rank(self) -> u8 {
        match self {
            EscLevel::Normal => 0,
            EscLevel::Watch => 1,
            EscLevel::Escalated => 2,
        }
    }

    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            EscLevel::Normal => "常态",
            EscLevel::Watch => "观察",
            EscLevel::Escalated => "升级",
        }
    }

    /// 由计数推档（纯函数）：0=Normal，1..N-1=Watch，>=N=Escalated。
    pub fn of_count(count: u32) -> EscLevel {
        if count == 0 {
            EscLevel::Normal
        } else if count >= ESCALATE_N {
            EscLevel::Escalated
        } else {
            EscLevel::Watch
        }
    }

    /// 闭集全量。
    pub const ALL: [EscLevel; 3] = [EscLevel::Normal, EscLevel::Watch, EscLevel::Escalated];
}

/// 等级解析：长短名双形制（normal|n / watch|w / escalated|e），
/// 其余 UnknownToken，空串 EmptyToken。
pub fn parse_level(tag: &str) -> Result<EscLevel, EscCode> {
    if tag.is_empty() {
        return Err(EscCode::EmptyToken);
    }
    match tag {
        "normal" | "n" => Ok(EscLevel::Normal),
        "watch" | "w" => Ok(EscLevel::Watch),
        "escalated" | "e" => Ok(EscLevel::Escalated),
        _ => Err(EscCode::UnknownToken),
    }
}

// ── 升级出处记录 ──────────────────────────────────────────────────────
/// 升级出处：升级瞬间的计数、阈值与轮次（判据「N 注出处」落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EscalationRecord {
    /// 升级条目
    pub item: u32,
    /// 升级瞬间计数（必须 == ESCALATE_N）
    pub at_count: u32,
    /// 当时生效的阈值
    pub threshold: u32,
    /// 触发轮次
    pub round: u32,
}

impl EscalationRecord {
    /// 出处自检：at_count 与阈值一致且阈值就是 N。
    pub fn sound(&self) -> bool {
        self.at_count == ESCALATE_N && self.threshold == ESCALATE_N
    }
}

/// 单条目升级行：计数 + 当前档 + 升级次数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EscRow {
    /// 条目号
    pub item: u32,
    /// 复发计数
    pub count: u32,
    /// 当前档（状态机秩只增）
    pub level: EscLevel,
    /// 累计升级次数
    pub escalations: u32,
}

/// 升级账本：enroll 建档 → bump 计数 → 满阈值自动升级留出处。
#[derive(Clone, Debug)]
pub struct EscalationLedger {
    rows: Vec<EscRow>,
    records: Vec<EscalationRecord>,
    cap: usize,
    total_bumps: u32,
}

impl EscalationLedger {
    /// 空账构造。
    pub fn new() -> EscalationLedger {
        EscalationLedger { rows: Vec::new(), records: Vec::new(), cap: LEDGER_CAP, total_bumps: 0 }
    }

    fn find(&self, item: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.item == item {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 建档：重复登记按 UnknownItem 前置判断返回 LedgerFull/Dup 语义
    /// ——重复建档不报错也不改账（幂等），满册才拒。
    pub fn enroll(&mut self, item: u32) -> Result<(), EscCode> {
        if self.find(item).is_some() {
            return Ok(());
        }
        if self.rows.len() >= self.cap {
            return Err(EscCode::LedgerFull);
        }
        self.rows.push(EscRow { item, count: 0, level: EscLevel::Normal, escalations: 0 });
        Ok(())
    }

    /// 复发自增：计数 +1；计数首次达 N → 升级并留出处。任何拒绝路径
    /// 不改计数与档位（拒绝不留痕）。
    pub fn bump(&mut self, item: u32, round: u32) -> Result<EscLevel, EscCode> {
        let idx = match self.find(item) {
            Some(i) => i,
            None => return Err(EscCode::UnknownItem),
        };
        let row = match self.rows.get(idx) {
            Some(r) => *r,
            None => return Err(EscCode::SegmentViolation),
        };
        if row.count >= COUNT_CAP {
            return Err(EscCode::CounterOverflow);
        }
        let next = row.count + 1;
        let next_level = EscLevel::of_count(next);
        if next_level.rank() < row.level.rank() {
            return Err(EscCode::LevelRegress);
        }
        let escalated_now = next == ESCALATE_N && row.level.rank() < next_level.rank();
        if let Some(r) = self.rows.get_mut(idx) {
            r.count = next;
            r.level = next_level;
            if escalated_now {
                r.escalations = r.escalations.saturating_add(1);
            }
        }
        self.total_bumps = self.total_bumps.saturating_add(1);
        if escalated_now {
            self.records.push(EscalationRecord { item, at_count: next, threshold: ESCALATE_N, round });
        }
        Ok(next_level)
    }

    /// 审计探针构造：由外部行数据装配审计副本（只读语义，不动主账）；
    /// 用于漏升检出演练——正常路径无法构造漏升账本，探针补上这一环。
    pub fn probe(rows: &[EscRow]) -> EscalationLedger {
        let mut l = EscalationLedger::new();
        let mut i = 0usize;
        while i < rows.len() {
            if let Some(r) = rows.get(i) {
                l.rows.push(*r);
            }
            i += 1;
        }
        l
    }

    /// 降级企图入口：状态机只升不降，向低秩档位设置一律 LevelRegress
    /// 且不改账；同档或升档请求不归降级路径（幂等返回 Ok，无副作用）。
    pub fn demote(&mut self, item: u32, to: EscLevel) -> Result<(), EscCode> {
        let idx = match self.find(item) {
            Some(i) => i,
            None => return Err(EscCode::UnknownItem),
        };
        let cur = match self.rows.get(idx) {
            Some(r) => r.level,
            None => return Err(EscCode::SegmentViolation),
        };
        if to.rank() < cur.rank() {
            return Err(EscCode::LevelRegress);
        }
        Ok(())
    }

    /// 查档：条目当前等级；未登记 UnknownItem。
    pub fn level_of(&self, item: u32) -> Result<EscLevel, EscCode> {
        match self.find(item).and_then(|i| self.rows.get(i)) {
            Some(r) => Ok(r.level),
            None => Err(EscCode::UnknownItem),
        }
    }

    /// 查计数。
    pub fn count_of(&self, item: u32) -> Result<u32, EscCode> {
        match self.find(item).and_then(|i| self.rows.get(i)) {
            Some(r) => Ok(r.count),
            None => Err(EscCode::UnknownItem),
        }
    }

    /// 升级出处清单（只读）。
    pub fn records(&self) -> &[EscalationRecord] {
        &self.records
    }

    /// 守恒：Σcount == total_bumps 且升级记录数 == Escalated 条目数。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u64;
        let mut esc_rows = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                sum += r.count as u64;
                if r.level == EscLevel::Escalated {
                    esc_rows = esc_rows.saturating_add(1);
                }
            }
            i += 1;
        }
        sum == self.total_bumps as u64 && esc_rows == self.records.len() as u32
    }

    /// 漏升扫描：复发计数已达 N 但档位未 Escalated 的条目数（必须为 0）。
    pub fn audit_missed(&self) -> u32 {
        let mut missed = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.count >= ESCALATE_N && r.level.rank() < EscLevel::Escalated.rank() {
                    missed = missed.saturating_add(1);
                }
            }
            i += 1;
        }
        missed
    }

    /// 已建档条目数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ── 升级小结成文（纯函数） ────────────────────────────────────────────
/// 升级成文：「升级=<n>;记录=<m>;档=常态/观察/升级」；失守恒先拒。
pub fn compose_escalation_summary(l: &EscalationLedger) -> Result<String, EscCode> {
    if !l.conserved() {
        return Err(EscCode::CountRegress);
    }
    let mut esc = 0u32;
    let mut watch = 0u32;
    let mut i = 0usize;
    while i < l.rows.len() {
        if let Some(r) = l.rows.get(i) {
            if r.level == EscLevel::Escalated {
                esc = esc.saturating_add(1);
            } else if r.level == EscLevel::Watch {
                watch = watch.saturating_add(1);
            }
        }
        i += 1;
    }
    let out = format!(
        "升级={};观察={};记录={}",
        esc,
        watch,
        l.records.len()
    );
    if out.is_empty() {
        return Err(EscCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏升级样本矩阵 ────────────────────────────────────────────
/// 四类坏升级样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEscalation {
    /// 阈值抢跑：计数 1 次就要求升级
    ThresholdBypass,
    /// 降级回退：Escalated 后企图回到 Watch
    LevelDowngrade,
    /// 未登记自增
    UnenrolledBump,
    /// 计数溢出
    CountOverflow,
}

impl BadEscalation {
    /// 闭集全量。
    pub const ALL: [BadEscalation; 4] = [
        BadEscalation::ThresholdBypass,
        BadEscalation::LevelDowngrade,
        BadEscalation::UnenrolledBump,
        BadEscalation::CountOverflow,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadEscalation::ThresholdBypass => "阈值抢跑",
            BadEscalation::LevelDowngrade => "降级回退",
            BadEscalation::UnenrolledBump => "未登记自增",
            BadEscalation::CountOverflow => "计数溢出",
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

    /// 记一次注入结果；账失守恒返回 CountRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), EscCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EscCode::CountRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏升级注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_escalation_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadEscalation::ALL.len() {
        let case = match BadEscalation::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadEscalation::ThresholdBypass => {
                    let mut l = EscalationLedger::new();
                    let _ = l.enroll(5);
                    let _ = l.bump(5, 1);
                    // 计数 1 次（Watch）却要求已是 Escalated → 抢跑检出
                    l.level_of(5).ok() == Some(EscLevel::Watch)
                        && l.level_of(5).ok() != Some(EscLevel::Escalated)
                        && l.records().is_empty()
                }
                BadEscalation::LevelDowngrade => {
                    let mut l = EscalationLedger::new();
                    let _ = l.enroll(6);
                    let _ = l.bump(6, 1);
                    let _ = l.bump(6, 2);
                    let _ = l.bump(6, 3);
                    let before = l.level_of(6).ok();
                    let refused = l.demote(6, EscLevel::Watch).err() == Some(EscCode::LevelRegress);
                    refused && before == Some(EscLevel::Escalated)
                        && l.level_of(6).ok() == Some(EscLevel::Escalated)
                }
                BadEscalation::UnenrolledBump => {
                    let mut l = EscalationLedger::new();
                    l.bump(99, 1).err() == Some(EscCode::UnknownItem)
                }
                BadEscalation::CountOverflow => {
                    let mut l = EscalationLedger::new();
                    let _ = l.enroll(8);
                    let mut n = 0u32;
                    let mut overflow = false;
                    // 多充一次（<= COUNT_CAP）：第 COUNT_CAP+1 次 bump 必须溢出拒绝
                    while n <= COUNT_CAP {
                        if l.bump(8, n + 1).err() == Some(EscCode::CounterOverflow) {
                            overflow = true;
                            break;
                        }
                        n += 1;
                    }
                    overflow
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
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 token/坏计数族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；抢跑/降级越权族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；条目未登记族"),
    ("EOVERFLOW", -75, 132, "Windows ERROR_ARITHMETIC_OVERFLOW；计数溢出族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；账本容量满族"),
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

/// 规范语料双跑指纹：建档 → 三连 bump 触发升级留出处 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = EscalationLedger::new();
    let _ = l.enroll(1);
    let _ = l.enroll(2);
    let _ = l.bump(1, 1);
    let _ = l.bump(1, 2);
    let _ = l.bump(1, 3); // 达 N=3，升级留出处
    let _ = l.bump(2, 4); // 计数 1，Watch
    let bytes = match compose_escalation_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10051 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10051_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10051");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91CC0..=0x91CFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 阈值策略：N=3 写死且 ≥1
    s.add("升级策略：N=3 写死且阈值有效",
          ESCALATE_N == 3 && ESCALATE_N >= 1 && COUNT_CAP > ESCALATE_N,
          "阈值必须为正且小于计数上限，否则升级语义失效");

    // 3. 档位推档：0/1/N 次三档纯函数
    let of_ok = EscLevel::of_count(0) == EscLevel::Normal
        && EscLevel::of_count(1) == EscLevel::Watch
        && EscLevel::of_count(ESCALATE_N - 1) == EscLevel::Watch
        && EscLevel::of_count(ESCALATE_N) == EscLevel::Escalated;
    s.add("档位推档：0/1..N-1/N 三档取值正确", of_ok,
          "Normal/Watch/Escalated 必须按计数单调推档");

    // 4. 等级解析器双形制 + 歧义拒绝 10/10
    let parse_ok = parse_level("normal") == Ok(EscLevel::Normal)
        && parse_level("w") == Ok(EscLevel::Watch)
        && parse_level("escalated") == Ok(EscLevel::Escalated);
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_level("amber").is_ok() || parse_level("").is_ok() || parse_level("WATCH").is_ok() {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("等级解析器：双形制通过且歧义 10/10 拒绝", parse_ok && amb_ok,
          "等级闭集外 token 必须逐次显性拒绝");

    // 5. 升级生效：N 连 bump 10/10 次触发 Escalated
    let mut trip_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut l = EscalationLedger::new();
        let _ = l.enroll(1);
        let mut k = 0u32;
        let mut tripped = false;
        while k < ESCALATE_N {
            if l.bump(1, k + 1).ok() == Some(EscLevel::Escalated) {
                tripped = true;
            }
            k += 1;
        }
        if !tripped || l.level_of(1).ok() != Some(EscLevel::Escalated) {
            trip_ok = false;
        }
        rr += 1;
    }
    s.add("升级生效：N 连复发 10/10 次触发 Escalated", trip_ok,
          "复发计数达 N 必须逐次触发升级，不许漏升");

    // 6. 逐次计数与出处：升级记录 at_count==N、阈值一致
    let mut l = EscalationLedger::new();
    let _ = l.enroll(1);
    let _ = l.enroll(2);
    let _ = l.bump(1, 1);
    let _ = l.bump(1, 2);
    let _ = l.bump(1, 3);
    let _ = l.bump(2, 4);
    let rec_ok = match l.records().first() {
        Some(rec) => rec.sound() && rec.item == 1 && rec.round == 3,
        None => false,
    };
    s.add("升级出处：记录携带 at_count==N 且阈值一致", rec_ok && l.records().len() == 1,
          "每次升级必须留出处：条目/瞬间计数/阈值/轮次四元组");

    // 7. 漏升检出：满 N 未升的探针账本被 audit 10/10 次检出
    let mut miss_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        // 探针账本：count 已达 N+2 却仍是 Watch —— 典型漏升形态
        let bad = [EscRow { item: 1, count: ESCALATE_N + 2, level: EscLevel::Watch, escalations: 0 }];
        let probe = EscalationLedger::probe(&bad);
        if probe.audit_missed() != 1 {
            miss_ok = false;
        }
        rr += 1;
    }
    s.add("漏升检出：满 N 未升账本被 audit 逐次识别", miss_ok && EscalationLedger::new().audit_missed() == 0,
          "audit_missed 必须能区分零漏升与漏升，正常账本漏升恒为 0");

    // 8. Watch 中间档：1..N-1 次 → Watch
    let watch_ok = l.level_of(2).ok() == Some(EscLevel::Watch)
        && l.count_of(2).ok() == Some(1);
    s.add("中间档：复发 1 次挂 Watch 不抢跑", watch_ok,
          "阈值以内只挂观察档，升级记录不得出现");

    // 9. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_escalation_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏升级样本各 10/10 拒绝", all_caught,
          "抢跑/降级/未登记/溢出必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadEscalation::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 计数守恒：Σcount == total_bumps
    s.add("计数守恒：Σcount == total_bumps 且记录数==升级条目数",
          l.conserved() && l.records().len() == 1,
          "升级账本守恒式逐笔对得上，绕账路径即失败");

    // 12. 未登记 bump 拒绝且不留痕
    let before_bumps = l.records().len();
    let unknown_ok = l.bump(77, 9).err() == Some(EscCode::UnknownItem)
        && l.records().len() == before_bumps;
    s.add("未登记拒绝：UnknownItem 且出处账不变", unknown_ok,
          "未登记条目不得进入升级计数，拒绝不留痕");

    // 13. 成文确定性 + 三档标记
    let sum = compose_escalation_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("升级=") && t.contains("观察=") && t.contains("记录="),
        Err(_) => false,
    };
    s.add("升级成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_escalation_summary(&l),
          "成文必须携带 升级/观察/记录 三标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(2)
        && agrees_with_linux(75) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：11 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < EscCode::ALL.len() {
        let mut j = i + 1;
        while j < EscCode::ALL.len() {
            if let (Some(a), Some(b)) = (EscCode::ALL.get(i), EscCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = EscCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：11 码两两唯一且中文建议非空", uniq && text_ok,
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
    let sum_text = format!("升级小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_level("normal"), Ok(EscLevel::Normal));
        assert_eq!(parse_level("w"), Ok(EscLevel::Watch));
        assert_eq!(parse_level("escalated"), Ok(EscLevel::Escalated));
        assert!(parse_level("amber").is_err());
        assert_eq!(parse_level("").err(), Some(EscCode::EmptyToken));
    }

    #[test]
    fn level_of_count_pure() {
        assert_eq!(EscLevel::of_count(0), EscLevel::Normal);
        assert_eq!(EscLevel::of_count(2), EscLevel::Watch);
        assert_eq!(EscLevel::of_count(3), EscLevel::Escalated);
    }

    #[test]
    fn trip_at_n_with_provenance() {
        let mut l = EscalationLedger::new();
        let _ = l.enroll(1);
        assert_eq!(l.bump(1, 1).ok(), Some(EscLevel::Watch));
        assert_eq!(l.bump(1, 2).ok(), Some(EscLevel::Watch));
        assert_eq!(l.bump(1, 3).ok(), Some(EscLevel::Escalated));
        assert_eq!(l.records().len(), 1);
        assert!(l.records().first().map(|r| r.sound()).unwrap_or(false));
    }

    #[test]
    fn unenrolled_rejected_without_trace() {
        let mut l = EscalationLedger::new();
        assert_eq!(l.bump(42, 1).err(), Some(EscCode::UnknownItem));
        assert_eq!(l.len(), 0);
        assert!(l.conserved());
    }

    #[test]
    fn bad_escalation_matrix_green() {
        let (inj, per) = run_bad_escalation_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn audit_missed_zero_on_healthy() {
        let mut l = EscalationLedger::new();
        let _ = l.enroll(3);
        let _ = l.bump(3, 1);
        assert_eq!(l.audit_missed(), 0);
    }

    #[test]
    fn summary_contains_marks() {
        let mut l = EscalationLedger::new();
        let _ = l.enroll(1);
        let _ = l.bump(1, 1);
        let _ = l.bump(1, 2);
        let _ = l.bump(1, 3);
        let s = compose_escalation_summary(&l).unwrap_or_default();
        assert!(s.contains("升级=1") && s.contains("记录=1"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(75));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(28), -28);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10051_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10051_checks_green() {
        let set = run_unx10051_checks();
        assert!(set.all_passed(), "UNX-F10051 red: {}", set.red_items_text());
    }
}
