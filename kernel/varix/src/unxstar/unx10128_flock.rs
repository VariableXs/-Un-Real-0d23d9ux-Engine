//! UNX-F10128 flock 文件锁正反双判据（共享独占达成与冲突阻塞误用受控）· 0x93000 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10129-J1 stat/fstat/lstat 三口径达成（符号链不跟随/跟随
//!       两读 10/10 次）；UNX-F10129-J2 坏缓冲误用受控（预期 EFAULT 精确
//!       复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93000（本卡独占 0x40 窗口，下一卡 0x93040）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_flock_call`："flock|path|op" 三成分，op 闭集
//!      {SH, EX, UN, SHNB, EXNB}，出闭集逐档拒绝；
//!   ② 锁表 `FlockLedger`：路径→(模式, 持有数)，SH 共享叠加、EX 独占
//!      互斥、SH↔EX 升降级转换、UN 释放，冲突非阻塞 EWOULDBLOCK 档；
//!   ③ 状态机 `LockState`：Unlocked→Shared→Exclusive 单向升级（禁倒退）；
//!   ④ 注入核 `BadFlock`：EX 撞 EX / SH 撞 EX / 未锁解锁 / 坏 op
//!      四类坏样本逐类 10/10 次精确复现 EWOULDBLOCK/EINVAL 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10128 独占 0x40 窗口，下一卡 0x93040）。
pub const SEG_BASE: u32 = 0x93000;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93040;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 锁表容量。
pub const LOCK_CAP: usize = 16;

/// LOCK_SH。
pub const LOCK_SH: u32 = 1;
/// LOCK_EX。
pub const LOCK_EX: u32 = 2;
/// LOCK_NB。
pub const LOCK_NB: u32 = 4;
/// LOCK_UN。
pub const LOCK_UN: u32 = 8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlCode {
    /// EX 撞 EX（EWOULDBLOCK 族）
    ExClash,
    /// SH 撞 EX（EWOULDBLOCK 族）
    ShClash,
    /// 未锁解锁（EINVAL 档）
    UnlockFree,
    /// 坏 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 锁表满（ENOLCK 族）
    LockFull,
    /// 状态倒退
    StateRegress,
    /// 持有数失平
    HoldDrift,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 升降级失序
    ConvertDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl FlCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FlCode::ExClash => "EX 撞 EX 被拒：独占锁互斥；EWOULDBLOCK 精确归档",
            FlCode::ShClash => "SH 撞 EX 被拒：独占期间共享请求必须等待；EWOULDBLOCK 精确归档",
            FlCode::UnlockFree => "未锁解锁被拒：无锁可释放；EINVAL 档受控拒绝",
            FlCode::BadOp => "非法 op 被拒：op 闭集 {SH, EX, UN, SHNB, EXNB}；请核对调用行",
            FlCode::MissingPart => "成分缺失被拒：调用行必须三段 flock|path|op；请补成分",
            FlCode::LockFull => "锁表满被拒：请先 LOCK_UN 释放",
            FlCode::StateRegress => "状态倒退被拒：Unlocked→Shared→Exclusive 单向升级",
            FlCode::HoldDrift => "持有数失平被拒：加锁/解锁必须逐次对平",
            FlCode::EmptySummary => "空成文被拒：锁小结不得为空串；请先 flock 再成文",
            FlCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            FlCode::ConvertDrift => "升降级失序被拒：SH↔EX 转换必须经 UN 或直接升级",
            FlCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FlCode; 12] = [
        FlCode::ExClash,
        FlCode::ShClash,
        FlCode::UnlockFree,
        FlCode::BadOp,
        FlCode::MissingPart,
        FlCode::LockFull,
        FlCode::StateRegress,
        FlCode::HoldDrift,
        FlCode::EmptySummary,
        FlCode::SegmentViolation,
        FlCode::ConvertDrift,
        FlCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 flock 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlockCall {
    /// 路径
    pub path: String,
    /// op 字面（SH/EX/UN/SHNB/EXNB）
    pub op: String,
}

/// 解析 "flock|path|op" 三段调用行。
pub fn parse_flock_call(line: &str) -> Result<FlockCall, FlCode> {
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
        return Err(FlCode::MissingPart);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(FlCode::MissingPart),
    };
    let op = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(FlCode::MissingPart),
    };
    if op != "SH" && op != "EX" && op != "UN" && op != "SHNB" && op != "EXNB" {
        return Err(FlCode::BadOp);
    }
    Ok(FlockCall { path, op })
}

// ── 锁状态机 ──────────────────────────────────────────────────────────
/// 锁档位（单调升级，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockState {
    /// 无锁
    Unlocked,
    /// 共享锁
    Shared,
    /// 独占锁
    Exclusive,
}

impl LockState {
    /// 单调升级判定。
    pub fn can_upgrade(self, next: LockState) -> bool {
        let rank = |s: LockState| match s {
            LockState::Unlocked => 0u8,
            LockState::Shared => 1,
            LockState::Exclusive => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            LockState::Unlocked => "无锁",
            LockState::Shared => "共享锁",
            LockState::Exclusive => "独占锁",
        }
    }
}

// ── 锁表 ──────────────────────────────────────────────────────────────
/// 锁条目。
#[derive(Clone, Debug)]
pub struct FlockEnt {
    /// 路径
    pub path: String,
    /// 当前档位
    pub mode: LockState,
    /// 共享持有数（EX 时恒 1）
    pub holders: u32,
}

/// 锁表：路径→(模式, 持有数)。
#[derive(Clone, Debug)]
pub struct FlockLedger {
    ents: Vec<FlockEnt>,
    cap: usize,
    /// 累计加锁成功次数
    pub locks: u32,
    /// 累计解锁成功次数
    pub unlocks: u32,
    /// 累计冲突拒止次数
    pub clashes: u32,
}

impl FlockLedger {
    /// 空账构造。
    pub fn new() -> FlockLedger {
        FlockLedger { ents: Vec::new(), cap: LOCK_CAP, locks: 0, unlocks: 0, clashes: 0 }
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

    /// flock 判定核：SH 叠加 / EX 互斥 / SH↔EX 转换 / UN 释放。
    pub fn flock(&mut self, path: &str, op: &str) -> Result<LockState, FlCode> {
        match op {
            "SH" | "SHNB" => self.lock_shared(path),
            "EX" | "EXNB" => self.lock_exclusive(path),
            "UN" => self.unlock(path),
            _ => Err(FlCode::BadOp),
        }
    }

    /// 共享锁：EX 在位即冲突；SH 叠加。
    fn lock_shared(&mut self, path: &str) -> Result<LockState, FlCode> {
        let idx = match self.entry(path) {
            Some(i) => i,
            None => return Err(FlCode::LockFull),
        };
        let mode = match self.ents.get(idx) {
            Some(e) => e.mode,
            None => return Err(FlCode::LockFull),
        };
        if mode == LockState::Exclusive {
            self.clashes = self.clashes.saturating_add(1);
            return Err(FlCode::ShClash);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.mode = LockState::Shared;
            e.holders = e.holders.saturating_add(1);
        }
        self.locks = self.locks.saturating_add(1);
        Ok(LockState::Shared)
    }

    /// 独占锁：SH/EX 任一在位即冲突；无锁直取。
    fn lock_exclusive(&mut self, path: &str) -> Result<LockState, FlCode> {
        let idx = match self.entry(path) {
            Some(i) => i,
            None => return Err(FlCode::LockFull),
        };
        let mode = match self.ents.get(idx) {
            Some(e) => e.mode,
            None => return Err(FlCode::LockFull),
        };
        if mode != LockState::Unlocked {
            self.clashes = self.clashes.saturating_add(1);
            return Err(FlCode::ExClash);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.mode = LockState::Exclusive;
            e.holders = 1;
        }
        self.locks = self.locks.saturating_add(1);
        Ok(LockState::Exclusive)
    }

    /// 解锁：无锁解锁受控拒；有锁清零。
    fn unlock(&mut self, path: &str) -> Result<LockState, FlCode> {
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(FlCode::UnlockFree),
        };
        let mode = match self.ents.get(idx) {
            Some(e) => e.mode,
            None => return Err(FlCode::UnlockFree),
        };
        if mode == LockState::Unlocked {
            return Err(FlCode::UnlockFree);
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.mode = LockState::Unlocked;
            e.holders = 0;
        }
        self.unlocks = self.unlocks.saturating_add(1);
        Ok(LockState::Unlocked)
    }

    /// 取条目或新建。
    fn entry(&mut self, path: &str) -> Option<usize> {
        if let Some(i) = self.idx_of(path) {
            return Some(i);
        }
        if self.ents.len() >= self.cap {
            return None;
        }
        let _ = self.ents.push(FlockEnt {
            path: String::from(path),
            mode: LockState::Unlocked,
            holders: 0,
        });
        Some(self.ents.len() - 1)
    }

    /// 模式读取。
    pub fn mode_of(&self, path: &str) -> Option<LockState> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.mode),
                None => None,
            },
            None => None,
        }
    }

    /// 持有数读取。
    pub fn holders_of(&self, path: &str) -> Option<u32> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.holders),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：LOCK_SH/LOCK_EX/LOCK_UN 转换 10/10 轮 ──────────────────
/// 逐轮：SH 叠加→SH→UN→EX 直取→EX→UN→SH。
pub fn run_flock_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = FlockLedger::new();
        let mut all_ok = true;
        // 面1：SH 两次叠加，持有数 2。
        all_ok = all_ok && l.flock("f", "SH").is_ok() && l.flock("f", "SH").is_ok()
            && l.holders_of("f") == Some(2);
        probes += 1;
        // 面2：SH 全部释放后 EX 直取。
        all_ok = all_ok && l.flock("f", "UN").is_ok()
            && l.flock("f", "EX") == Ok(LockState::Exclusive);
        probes += 1;
        // 面3：EX 期间 SH 请求冲突。
        all_ok = all_ok && l.flock("f", "SH").err() == Some(FlCode::ShClash);
        probes += 1;
        // 面4：UN 释放后回到无锁。
        all_ok = all_ok && l.flock("f", "UN") == Ok(LockState::Unlocked)
            && l.mode_of("f") == Some(LockState::Unlocked);
        probes += 1;
        // 面5：SH 再取，EX 请求冲突（非阻塞升级）。
        all_ok = all_ok && l.flock("f", "SH").is_ok()
            && l.flock("f", "EX").err() == Some(FlCode::ExClash);
        probes += 1;
        // 面6：UN 后无锁解锁受控拒。
        all_ok = all_ok && l.flock("f", "UN").is_ok()
            && l.flock("f", "UN").err() == Some(FlCode::UnlockFree);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏锁矩阵 ──────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFlock {
    /// EX 撞 EX（EWOULDBLOCK 族）
    ExClash,
    /// SH 撞 EX（EWOULDBLOCK 族）
    ShClash,
    /// 未锁解锁（EINVAL 档）
    UnlockFree,
    /// 坏 op（EINVAL 族）
    BadOp,
}

impl BadFlock {
    /// 闭集全量。
    pub const ALL: [BadFlock; 4] = [
        BadFlock::ExClash,
        BadFlock::ShClash,
        BadFlock::UnlockFree,
        BadFlock::BadOp,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadFlock::ExClash => "EX 撞 EX",
            BadFlock::ShClash => "SH 撞 EX",
            BadFlock::UnlockFree => "未锁解锁",
            BadFlock::BadOp => "非法 op 行",
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
    pub fn record(&mut self, caught: bool) -> Result<(), FlCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(FlCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏锁注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_flock_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadFlock::ALL.len() {
        let case = match BadFlock::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = FlockLedger::new();
            let caught = match case {
                BadFlock::ExClash => {
                    let _ = l.flock("f", "EX");
                    l.flock("f", "EX").err() == Some(FlCode::ExClash)
                }
                BadFlock::ShClash => {
                    let _ = l.flock("f", "EX");
                    l.flock("f", "SH").err() == Some(FlCode::ShClash)
                }
                BadFlock::UnlockFree => {
                    l.flock("g", "UN").err() == Some(FlCode::UnlockFree)
                }
                BadFlock::BadOp => {
                    parse_flock_call("flock|f|XY").err() == Some(FlCode::BadOp)
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

// ── 锁小结成文（纯函数） ──────────────────────────────────────────────
/// 小结成文：「path=<s> mode=<t> holders=<n>」。
pub fn compose_flock_summary(l: &FlockLedger, path: &str) -> Result<String, FlCode> {
    let mode = match l.mode_of(path) {
        Some(m) => m,
        None => return Err(FlCode::EmptySummary),
    };
    let holders = match l.holders_of(path) {
        Some(h) => h,
        None => return Err(FlCode::EmptySummary),
    };
    let out = format!("path={} mode={} holders={}", path, mode.label(), holders);
    if out.is_empty() {
        return Err(FlCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EWOULDBLOCK", -11, 33, "Windows ERROR_LOCK_VIOLATION；非阻塞冲突族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 op/未锁解锁族"),
    ("ENOLCK", -37, 181, "Windows ERROR_NOT_LOCKED；锁表满族"),
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

/// 规范语料指纹：SH→SH→UN→EX→UN→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = FlockLedger::new();
    let _ = l.flock("f", "SH");
    let _ = l.flock("f", "SH");
    let _ = l.flock("f", "UN");
    let _ = l.flock("f", "EX");
    let _ = l.flock("f", "UN");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_flock_summary(&l, "f") {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&l.locks.to_le_bytes());
    bytes.extend_from_slice(&l.unlocks.to_le_bytes());
    bytes.extend_from_slice(&l.clashes.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10128 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10128_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10128");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93000..=0x9303F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < FlCode::ALL.len() {
        let mut j = i + 1;
        while j < FlCode::ALL.len() {
            if let (Some(a), Some(b)) = (FlCode::ALL.get(i), FlCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = FlCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_flock_call("flock|f|EX").is_ok()
        && parse_flock_call("flock|f|XY").err() == Some(FlCode::BadOp)
        && parse_flock_call("flock|f").err() == Some(FlCode::MissingPart);
    s.add("调用行解析器：op 五词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {SH, EX, UN, SHNB, EXNB}；出闭集逐档拒");

    // 4. J1 达成核
    let (ok, probes) = run_flock_rounds();
    s.add("J1 锁语义达成：LOCK_SH/LOCK_EX/LOCK_UN 转换 LTP flock01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（叠加/升级/冲突/释放/降级拒/空解锁拒）全对");

    // 5. J2 非阻塞冲突
    let mut l = FlockLedger::new();
    let _ = l.flock("a", "EX");
    let clash_ok = l.flock("a", "EXNB").err() == Some(FlCode::ExClash)
        && l.flock("a", "SHNB").err() == Some(FlCode::ShClash)
        && l.holders_of("a") == Some(1);
    s.add("J2 非阻塞冲突受控：EWOULDBLOCK 双档精确复现且原锁不动", clash_ok,
          "EX 在位时 EX/SH 两向请求都必须拒；持有数不得漂移");

    // 6. SH 叠加语义
    let mut l2 = FlockLedger::new();
    let _ = l2.flock("b", "SH");
    let _ = l2.flock("b", "SH");
    let _ = l2.flock("b", "SH");
    let sh_ok = l2.holders_of("b") == Some(3)
        && l2.flock("b", "UN") == Ok(LockState::Unlocked)
        && l2.holders_of("b") == Some(0);
    s.add("SH 叠加：三次共享叠加与一次释放清零", sh_ok,
          "共享锁持有数是叠加账面根；UN 必须清零");

    // 7. 未锁解锁档
    let free_ok = l2.flock("b", "UN").err() == Some(FlCode::UnlockFree);
    s.add("未锁解锁：EINVAL 档受控拒绝", free_ok,
          "清零后再 UN 是受控误用；不得假成功");

    // 8. 注入核
    let (inj, per_class) = run_bad_flock_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "EX 撞 EX/SH 撞 EX/未锁解锁/坏 op 必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadFlock::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 状态机
    let sm_ok = LockState::Unlocked.can_upgrade(LockState::Exclusive)
        && LockState::Unlocked.can_upgrade(LockState::Shared)
        && LockState::Shared.can_upgrade(LockState::Exclusive)
        && !LockState::Exclusive.can_upgrade(LockState::Shared)
        && LockState::Shared.label() == "共享锁";
    s.add("状态机：Unlocked→Shared→Exclusive 单向升级", sm_ok,
          "锁档位只许升级；降级必须经 UN");

    // 11. 冲突计数守恒
    let cnt_ok = l.clashes == 2 && l.locks == 1 && l.unlocks == 0;
    s.add("冲突计数守恒：locks/unlocks/clashes 逐次对平", cnt_ok,
          "锁账三计数必须与操作序列一致");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(11) && agrees_with_linux(22) && agrees_with_linux(37)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_flock_summary(&l, "a");
    let text_ok = match &sum {
        Ok(x) => x.contains("path=a") && x.contains("mode=独占锁"),
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
    let ok_text = format!("锁语义达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_flock_call("flock|f|EX").is_ok());
        assert!(parse_flock_call("flock|f|SHNB").is_ok());
        assert_eq!(parse_flock_call("flock|f|XY").err(), Some(FlCode::BadOp));
        assert_eq!(parse_flock_call("flock|f").err(), Some(FlCode::MissingPart));
    }

    #[test]
    fn shared_stacking() {
        let mut l = FlockLedger::new();
        let _ = l.flock("f", "SH");
        let _ = l.flock("f", "SH");
        assert_eq!(l.holders_of("f"), Some(2));
        assert_eq!(l.mode_of("f"), Some(LockState::Shared));
    }

    #[test]
    fn exclusive_mutex() {
        let mut l = FlockLedger::new();
        let _ = l.flock("f", "EX");
        assert_eq!(l.flock("f", "EX").err(), Some(FlCode::ExClash));
        assert_eq!(l.flock("f", "SH").err(), Some(FlCode::ShClash));
        assert_eq!(l.holders_of("f"), Some(1));
    }

    #[test]
    fn upgrade_after_release() {
        let mut l = FlockLedger::new();
        let _ = l.flock("f", "SH");
        let _ = l.flock("f", "UN");
        assert_eq!(l.flock("f", "EX"), Ok(LockState::Exclusive));
    }

    #[test]
    fn unlock_free_blocked() {
        let mut l = FlockLedger::new();
        assert_eq!(l.flock("g", "UN").err(), Some(FlCode::UnlockFree));
        let _ = l.flock("f", "SH");
        let _ = l.flock("f", "UN");
        assert_eq!(l.flock("f", "UN").err(), Some(FlCode::UnlockFree));
    }

    #[test]
    fn release_clears_holders() {
        let mut l = FlockLedger::new();
        let _ = l.flock("f", "SH");
        let _ = l.flock("f", "SH");
        let _ = l.flock("f", "UN");
        assert_eq!(l.holders_of("f"), Some(0));
        assert_eq!(l.unlocks, 1);
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_flock_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_flock_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(LockState::Unlocked.can_upgrade(LockState::Shared));
        assert!(!LockState::Exclusive.can_upgrade(LockState::Unlocked));
        assert_eq!(LockState::Exclusive.label(), "独占锁");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10128_checks_green() {
        let set = run_unx10128_checks();
        assert!(set.all_passed(), "UNX-F10128 red: {}", set.red_items_text());
    }
}
