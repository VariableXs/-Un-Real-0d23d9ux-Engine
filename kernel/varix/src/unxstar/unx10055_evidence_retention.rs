//! UNX-F10055 回归证据链（日志留存期）· 0x91DC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10056-J1 常态挡板生效（周期未跑闸提醒 10/10 次），越闸 10/10 次拦截
//!
//! 设计要点：
//!   ① 留存窗策略 window=8 写死：日志自 born_round 起在期 window 轮；
//!      refer 在期日志 10/10 次成功，越窗引用 → ExpiredRefused 逐次拦截。
//!   ② 证据链哈希相接：每条日志携带 prev_digest，必须等于前一条的
//!      digest（断链 → ChainBreak）；born_round 必须严格递增（倒退 →
//!      OutOfOrder）；重复日志号 → DupLog。链完整性 chain_sound 可复算。
//!   ③ 过期清除必须留痕：purge_expired 精确清出全部过期日志并记
//!      PurgeRecord（轮次+条数）；空库清除返回 0 不报错；清除后被引用
//!      的日志号 → UnknownRef（证据消失必须可查）。
//!   ④ 注入核：越窗引用 / 未来出生 / 断链 / 重复日志四类坏样本逐类
//!      10/10 次拒绝，注入账守恒逃逸 = 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10055 独占 0x40 窗口，下一卡 0x91E00）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91DC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 留存窗（轮）：日志自出生轮起在期 8 轮。
pub const RETENTION_WINDOW: u32 = 8;

/// 窗宽上限（工程约束）。
pub const WINDOW_MAX: u32 = 64;

/// 证据库容量上限。
pub const VAULT_CAP: usize = 64;

/// 链首哨兵：首条日志的 prev_digest。
pub const GENESIS_DIGEST: u32 = 0;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VaultCode {
    /// 越窗引用（日志已过留存期）
    ExpiredRefused,
    /// 未来出生（born_round 晚于当前轮）
    FutureBorn,
    /// 链哈希断裂
    ChainBreak,
    /// 重复日志号
    DupLog,
    /// 出生轮倒退（乱序入链）
    OutOfOrder,
    /// 引用已清除或不存在的日志
    UnknownRef,
    /// 留存窗为零
    ZeroWindow,
    /// 留存窗超宽（越过 WINDOW_MAX）
    WindowTooWide,
    /// 空窗 token 或空库引用歧义
    EmptyWindow,
    /// 证据库容量满
    LedgerFull,
    /// 空成文
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl VaultCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            VaultCode::ExpiredRefused => "越窗引用被拒：日志已过留存期不得作为证据；\
                如需留证请重新回归产出新日志",
            VaultCode::FutureBorn => "未来出生被拒：born_round 不得晚于当前轮；\
                请核对轮次时钟再入链",
            VaultCode::ChainBreak => "链哈希断裂被拒：prev_digest 必须等于前一条 digest；\
                请按链序逐条入链，不许跳链",
            VaultCode::DupLog => "重复日志号被拒：同一日志号只允许入链一次；请换新日志号",
            VaultCode::OutOfOrder => "出生轮倒退被拒：证据链 born_round 必须严格递增；\
                请按轮次顺序入链",
            VaultCode::UnknownRef => "缺证引用被拒：引用的日志号不在库内或已被清除；\
                请先用入链返回值留存日志号",
            VaultCode::ZeroWindow => "零窗被拒：留存窗必须大于 0；请按策略重设",
            VaultCode::WindowTooWide => "超宽窗被拒：留存窗不得越过上限；请压缩窗宽",
            VaultCode::EmptyWindow => "空窗被拒：窗 token 不能为空串或非数字",
            VaultCode::LedgerFull => "证据库容量满被拒：已达上限；请先清除过期日志",
            VaultCode::EmptySummary => "空成文被拒：证据链小结不得为空串；请检查库是否为空",
            VaultCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [VaultCode; 12] = [
        VaultCode::ExpiredRefused,
        VaultCode::FutureBorn,
        VaultCode::ChainBreak,
        VaultCode::DupLog,
        VaultCode::OutOfOrder,
        VaultCode::UnknownRef,
        VaultCode::ZeroWindow,
        VaultCode::WindowTooWide,
        VaultCode::EmptyWindow,
        VaultCode::LedgerFull,
        VaultCode::EmptySummary,
        VaultCode::SegmentViolation,
    ];
}

// ── 留存窗解析器 ──────────────────────────────────────────────────────
/// 窗解析：纯数字 token → 留存窗；零窗 ZeroWindow、超宽 WindowTooWide、
/// 空串或非数字 EmptyWindow。
pub fn parse_window(token: &str) -> Result<u32, VaultCode> {
    if token.is_empty() {
        return Err(VaultCode::EmptyWindow);
    }
    let bytes = token.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => return Err(VaultCode::EmptyWindow),
        };
        if b < b'0' || b > b'9' {
            return Err(VaultCode::EmptyWindow);
        }
        v = match v.checked_mul(10).and_then(|x| x.checked_add((b - b'0') as u32)) {
            Some(x) => x,
            None => return Err(VaultCode::WindowTooWide),
        };
        i += 1;
    }
    if v == 0 {
        return Err(VaultCode::ZeroWindow);
    }
    if v > WINDOW_MAX {
        return Err(VaultCode::WindowTooWide);
    }
    Ok(v)
}

// ── 证据日志与链 ──────────────────────────────────────────────────────
/// 证据日志：日志号 + 出生轮 + 链哈希。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvidenceLog {
    /// 日志号（从 1 起算）
    pub log_id: u32,
    /// 出生轮
    pub born_round: u32,
    /// 本条链哈希
    pub digest: u32,
    /// 前一条链哈希（首条为 GENESIS_DIGEST）
    pub prev_digest: u32,
}

impl EvidenceLog {
    /// 在期判定：cur_round - born_round ≤ RETENTION_WINDOW。
    /// born_round 晚于 cur_round 视为在期（时钟由 refer 侧把关）。
    pub fn in_period(&self, cur_round: u32) -> bool {
        cur_round >= self.born_round && cur_round - self.born_round <= RETENTION_WINDOW
    }
}

/// 过期清除留痕：轮次 + 清出条数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PurgeRecord {
    /// 清除轮次
    pub at_round: u32,
    /// 清出条数
    pub purged: u32,
}

/// 证据库：按链序入链、在期引用、过期清除留痕。
#[derive(Clone, Debug)]
pub struct EvidenceVault {
    logs: Vec<EvidenceLog>,
    purges: Vec<PurgeRecord>,
    cap: usize,
}

impl EvidenceVault {
    /// 空库构造。
    pub fn new() -> EvidenceVault {
        EvidenceVault { logs: Vec::new(), purges: Vec::new(), cap: VAULT_CAP }
    }

    fn find(&self, log_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.logs.len() {
            if let Some(l) = self.logs.get(i) {
                if l.log_id == log_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 入链：链哈希相接 + 出生轮严格递增 + 日志号唯一。
    /// 每种违例单独诊断，拒绝路径不改链（拒绝不留痕）。
    pub fn append(&mut self, log: EvidenceLog) -> Result<u32, VaultCode> {
        if self.find(log.log_id).is_some() {
            return Err(VaultCode::DupLog);
        }
        if self.logs.len() >= self.cap {
            return Err(VaultCode::LedgerFull);
        }
        let expect_prev = match self.logs.last() {
            Some(last) => last.digest,
            None => GENESIS_DIGEST,
        };
        if log.prev_digest != expect_prev {
            return Err(VaultCode::ChainBreak);
        }
        if let Some(last) = self.logs.last() {
            if log.born_round <= last.born_round {
                return Err(VaultCode::OutOfOrder);
            }
        }
        self.logs.push(log);
        Ok(log.digest)
    }

    /// 在期引用：日志在期返回 digest；越窗 ExpiredRefused；缺证 UnknownRef。
    pub fn refer(&self, log_id: u32, cur_round: u32) -> Result<u32, VaultCode> {
        let idx = match self.find(log_id) {
            Some(i) => i,
            None => return Err(VaultCode::UnknownRef),
        };
        let log = match self.logs.get(idx) {
            Some(l) => *l,
            None => return Err(VaultCode::SegmentViolation),
        };
        if log.born_round > cur_round {
            return Err(VaultCode::FutureBorn);
        }
        if !log.in_period(cur_round) {
            return Err(VaultCode::ExpiredRefused);
        }
        Ok(log.digest)
    }

    /// 过期清除：精确清出全部过期日志并留痕；空库或无过期返回 Ok(0)。
    pub fn purge_expired(&mut self, cur_round: u32) -> Result<u32, VaultCode> {
        let mut kept: Vec<EvidenceLog> = Vec::new();
        let mut purged = 0u32;
        let mut i = 0usize;
        while i < self.logs.len() {
            let log = match self.logs.get(i) {
                Some(l) => *l,
                None => return Err(VaultCode::SegmentViolation),
            };
            if log.born_round <= cur_round && !log.in_period(cur_round) {
                purged = purged.saturating_add(1);
            } else {
                kept.push(log);
            }
            i += 1;
        }
        if purged > 0 {
            self.purges.push(PurgeRecord { at_round: cur_round, purged });
        }
        self.logs = kept;
        Ok(purged)
    }

    /// 清除留痕清单（只读）。
    pub fn purge_records(&self) -> &[PurgeRecord] {
        &self.purges
    }

    /// 链完整性：逐条核 prev_digest 相接与 born_round 递增。
    pub fn chain_sound(&self) -> bool {
        let mut expect_prev = GENESIS_DIGEST;
        let mut prev_round = 0u32;
        let mut i = 0usize;
        while i < self.logs.len() {
            let log = match self.logs.get(i) {
                Some(l) => *l,
                None => return false,
            };
            if log.prev_digest != expect_prev || log.born_round <= prev_round {
                return false;
            }
            expect_prev = log.digest;
            prev_round = log.born_round;
            i += 1;
        }
        true
    }

    /// 库内日志数。
    pub fn len(&self) -> usize {
        self.logs.len()
    }

    /// 在期日志数（按 cur_round 清点）。
    pub fn in_period_count(&self, cur_round: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.logs.len() {
            if let Some(l) = self.logs.get(i) {
                if l.in_period(cur_round) {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ── 证据链成文（纯函数） ──────────────────────────────────────────────
/// 证据成文：「链=<n>;在期=<m>;清除=<k>;窗=<w>」；断链先拒。
pub fn compose_vault_summary(v: &EvidenceVault, cur_round: u32) -> Result<String, VaultCode> {
    if !v.chain_sound() {
        return Err(VaultCode::ChainBreak);
    }
    let mut purged_total = 0u32;
    let mut i = 0usize;
    while i < v.purge_records().len() {
        if let Some(p) = v.purge_records().get(i) {
            purged_total = purged_total.saturating_add(p.purged);
        }
        i += 1;
    }
    let out = format!(
        "链={};在期={};清除={};窗={}",
        v.len(),
        v.in_period_count(cur_round),
        purged_total,
        RETENTION_WINDOW
    );
    if out.is_empty() {
        return Err(VaultCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏证据样本矩阵 ────────────────────────────────────────────
/// 四类坏证据样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadEvidence {
    /// 越窗引用
    ExpiredReference,
    /// 未来出生
    FutureBorn,
    /// 断链入链
    BrokenChain,
    /// 重复日志号
    DupLogId,
}

impl BadEvidence {
    /// 闭集全量。
    pub const ALL: [BadEvidence; 4] = [
        BadEvidence::ExpiredReference,
        BadEvidence::FutureBorn,
        BadEvidence::BrokenChain,
        BadEvidence::DupLogId,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadEvidence::ExpiredReference => "越窗引用",
            BadEvidence::FutureBorn => "未来出生",
            BadEvidence::BrokenChain => "断链入链",
            BadEvidence::DupLogId => "重复日志号",
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

    /// 记一次注入结果；账失守恒返回 SegmentViolation。
    pub fn record(&mut self, caught: bool) -> Result<(), VaultCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(VaultCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏证据注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_evidence_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadEvidence::ALL.len() {
        let case = match BadEvidence::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadEvidence::ExpiredReference => {
                    let mut v = EvidenceVault::new();
                    let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0xA1, prev_digest: GENESIS_DIGEST });
                    // 窗 8：出生轮 1，当前轮 10 → 越窗
                    v.refer(1, 1 + RETENTION_WINDOW + 1).err() == Some(VaultCode::ExpiredRefused)
                }
                BadEvidence::FutureBorn => {
                    let mut v = EvidenceVault::new();
                    let _ = v.append(EvidenceLog { log_id: 1, born_round: 9, digest: 0xB1, prev_digest: GENESIS_DIGEST });
                    // 当前轮 5 < 出生轮 9 → 未来出生
                    v.refer(1, 5).err() == Some(VaultCode::FutureBorn)
                }
                BadEvidence::BrokenChain => {
                    let mut v = EvidenceVault::new();
                    let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0xC1, prev_digest: GENESIS_DIGEST });
                    v.append(EvidenceLog { log_id: 2, born_round: 2, digest: 0xC2, prev_digest: 0xDEAD })
                        .err() == Some(VaultCode::ChainBreak)
                }
                BadEvidence::DupLogId => {
                    let mut v = EvidenceVault::new();
                    let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0xD1, prev_digest: GENESIS_DIGEST });
                    v.append(EvidenceLog { log_id: 1, born_round: 2, digest: 0xD2, prev_digest: 0xD1 })
                        .err() == Some(VaultCode::DupLog)
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
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏窗/坏轮次族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越窗引用族"),
    ("ESTALE", -116, 18, "Windows ERROR_MORE_DATA；过期证据族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺证引用族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复日志号族"),
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

/// 规范语料双跑指纹：三条日志入链 → 在期引用 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut v = EvidenceVault::new();
    let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0x1100_0001, prev_digest: GENESIS_DIGEST });
    let _ = v.append(EvidenceLog { log_id: 2, born_round: 2, digest: 0x1100_0002, prev_digest: 0x1100_0001 });
    let _ = v.append(EvidenceLog { log_id: 3, born_round: 3, digest: 0x1100_0003, prev_digest: 0x1100_0002 });
    let _ = v.refer(2, 4);
    let bytes = match compose_vault_summary(&v, 4) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10055 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10055_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10055");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91DC0..=0x91DFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 留存窗策略：window=8 且在上限以内
    s.add("留存策略：窗=8 写死且 1..=上限",
          RETENTION_WINDOW == 8 && RETENTION_WINDOW >= 1 && RETENTION_WINDOW <= WINDOW_MAX,
          "留存窗必须为正且不超过上限，否则在期语义失效");

    // 3. 窗解析器：合法数字通过，零窗/超宽/空串 10/10 拒绝
    let parse_ok = parse_window("8") == Ok(8) && parse_window("64") == Ok(64);
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_window("0").is_ok()
            || parse_window("65").is_ok()
            || parse_window("").is_ok()
            || parse_window("8轮").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("窗解析器：合法通过且零窗/超宽/空串 10/10 拒绝", parse_ok && amb_ok,
          "留存窗解析必须闭集校验，歧义窗逐次显性拒绝");

    // 4. 证据入链：三条日志链哈希相接且链完整
    let mut v = EvidenceVault::new();
    let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0x1100_0001, prev_digest: GENESIS_DIGEST });
    let _ = v.append(EvidenceLog { log_id: 2, born_round: 2, digest: 0x1100_0002, prev_digest: 0x1100_0001 });
    let _ = v.append(EvidenceLog { log_id: 3, born_round: 3, digest: 0x1100_0003, prev_digest: 0x1100_0002 });
    s.add("证据入链：三条链哈希相接且 chain_sound", v.len() == 3 && v.chain_sound(),
          "prev_digest 必须逐条相接，出生轮严格递增");

    // 5. 在期核：逐轮引用 10/10 次成功
    let mut in_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = EvidenceVault::new();
        let _ = g.append(EvidenceLog { log_id: 1, born_round: 2, digest: 0x55, prev_digest: GENESIS_DIGEST });
        // 出生轮 2 + 窗 8 → 第 2..10 轮在期
        let mut cur = 2u32;
        while cur <= 2 + RETENTION_WINDOW {
            if g.refer(1, cur).is_err() {
                in_ok = false;
            }
            cur += 1;
        }
        rr += 1;
    }
    s.add("在期核：窗内逐轮引用 10/10 次成功", in_ok,
          "在期日志必须逐轮可引用，窗内不得误拒");

    // 6. 过期拦截：越窗引用 10/10 次拒绝
    let mut exp_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = EvidenceVault::new();
        let _ = g.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0x66, prev_digest: GENESIS_DIGEST });
        if g.refer(1, 1 + RETENTION_WINDOW + 1).err() != Some(VaultCode::ExpiredRefused) {
            exp_ok = false;
        }
        rr += 1;
    }
    s.add("过期拦截：越窗引用 10/10 次拒绝", exp_ok,
          "越窗日志不得作为证据，逐次显性拒绝");

    // 7. 过期清除：purge 精确清出并留痕 10/10
    let mut purge_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = EvidenceVault::new();
        let _ = g.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0x71, prev_digest: GENESIS_DIGEST });
        let _ = g.append(EvidenceLog { log_id: 2, born_round: 9, digest: 0x72, prev_digest: 0x71 });
        // 当前轮 12：日志 1（出生 1，窗 8 → 过期），日志 2（出生 9 → 在期）
        let n = g.purge_expired(12);
        if n.ok() != Some(1) || g.len() != 1 || g.purge_records().len() != 1 {
            purge_ok = false;
        }
        rr += 1;
    }
    s.add("过期清除：精确清出且留痕 10/10", purge_ok,
          "过期清除必须逐条清出并记 PurgeRecord，不许静默残留");

    // 8. 清除后引用 → UnknownRef
    let mut g = EvidenceVault::new();
    let _ = g.append(EvidenceLog { log_id: 1, born_round: 1, digest: 0x81, prev_digest: GENESIS_DIGEST });
    let _ = g.purge_expired(1 + RETENTION_WINDOW + 1);
    s.add("缺证引用：清除后日志号引用拒绝",
          g.refer(1, 100).err() == Some(VaultCode::UnknownRef) && g.len() == 0,
          "证据消失必须可查，清除后的引用显性拒绝");

    // 9. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_evidence_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏证据样本各 10/10 拒绝", all_caught,
          "越窗/未来出生/断链/重复号必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadEvidence::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 拒绝不留痕：坏入链不改链
    let before = v.len();
    // 断链样本（编号新但 prev 错）与倒退样本（born_round ≤ 尾条）
    let _ = v.append(EvidenceLog { log_id: 4, born_round: 2, digest: 0x99, prev_digest: 0xDEAD });
    let _ = v.append(EvidenceLog { log_id: 5, born_round: 2, digest: 0x9A, prev_digest: 0x1100_0003 });
    s.add("拒绝不留痕：断链/倒退入链不改链长", v.len() == before && before == 3,
          "被拒入链不得改动链内容，任何拒绝路径零副作用");

    // 12. 成文非空且含标记 + 双跑一致
    let sum = compose_vault_summary(&v, 3);
    let text_ok = match &sum {
        Ok(t) => t.contains("链=3") && t.contains("窗=8"),
        Err(_) => false,
    };
    s.add("证据成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_vault_summary(&v, 3),
          "成文必须携带 链/在期/清除/窗 标记且为纯函数");

    // 13. 空库清除返回 0（幂等）
    s.add("空库幂等：空库清除返回 0 不报错",
          EvidenceVault::new().purge_expired(10).ok() == Some(0),
          "无过期可清时清除必须幂等返回 0，不许误报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(1) && agrees_with_linux(116)
        && agrees_with_linux(2) && agrees_with_linux(17)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < VaultCode::ALL.len() {
        let mut j = i + 1;
        while j < VaultCode::ALL.len() {
            if let (Some(a), Some(b)) = (VaultCode::ALL.get(i), VaultCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = VaultCode::ALL.iter().all(|c| !c.detail().is_empty());
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
    let sum_text = format!("证据小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_parser_closed_set() {
        assert_eq!(parse_window("8"), Ok(8));
        assert_eq!(parse_window("64"), Ok(64));
        assert_eq!(parse_window("0").err(), Some(VaultCode::ZeroWindow));
        assert_eq!(parse_window("65").err(), Some(VaultCode::WindowTooWide));
        assert_eq!(parse_window("").err(), Some(VaultCode::EmptyWindow));
    }

    #[test]
    fn chain_appends_in_order() {
        let mut v = EvidenceVault::new();
        assert!(v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 1, prev_digest: GENESIS_DIGEST }).is_ok());
        assert!(v.append(EvidenceLog { log_id: 2, born_round: 2, digest: 2, prev_digest: 1 }).is_ok());
        assert!(v.chain_sound());
    }

    #[test]
    fn broken_chain_rejected() {
        let mut v = EvidenceVault::new();
        let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 1, prev_digest: GENESIS_DIGEST });
        assert_eq!(v.append(EvidenceLog { log_id: 2, born_round: 2, digest: 2, prev_digest: 9 }).err(),
                   Some(VaultCode::ChainBreak));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn in_period_boundary() {
        let log = EvidenceLog { log_id: 1, born_round: 3, digest: 7, prev_digest: GENESIS_DIGEST };
        assert!(log.in_period(3 + RETENTION_WINDOW));
        assert!(!log.in_period(3 + RETENTION_WINDOW + 1));
        assert!(!log.in_period(2));
    }

    #[test]
    fn expired_reference_intercepted() {
        let mut v = EvidenceVault::new();
        let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 1, prev_digest: GENESIS_DIGEST });
        assert_eq!(v.refer(1, 100).err(), Some(VaultCode::ExpiredRefused));
    }

    #[test]
    fn purge_expired_exact() {
        let mut v = EvidenceVault::new();
        let _ = v.append(EvidenceLog { log_id: 1, born_round: 1, digest: 1, prev_digest: GENESIS_DIGEST });
        let _ = v.append(EvidenceLog { log_id: 2, born_round: 13, digest: 2, prev_digest: 1 });
        // 轮 20：日志 1（出生 1，龄 19 > 窗 8）过期；日志 2（出生 13，龄 7）在期
        assert_eq!(v.purge_expired(20).ok(), Some(1));
        assert_eq!(v.len(), 1);
        assert_eq!(v.purge_records().len(), 1);
    }

    #[test]
    fn bad_evidence_matrix_green() {
        let (inj, per) = run_bad_evidence_matrix();
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
        assert!(agrees_with_linux(116));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(2), -2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10055_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10055_checks_green() {
        let set = run_unx10055_checks();
        assert!(set.all_passed(), "UNX-F10055 red: {}", set.red_items_text());
    }
}
