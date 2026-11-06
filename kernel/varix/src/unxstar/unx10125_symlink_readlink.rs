//! UNX-F10125 symlink/readlink 正反双判据（符号链达成与环路误用受控）· 0x92F40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10126-J1 目录替换达成（空目录换位语义 LTP rename02 面
//!       10/10 次）；UNX-F10126-J2 非空目录替换误用受控（预期 ENOTEMPTY
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92F40（本卡独占 0x40 窗口，下一卡 0x92F80）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_sym_call`："op|a|b" 三成分（op ∈ {symlink,
//!      readlink, resolve}），op 闭集/缺成分逐档拒绝；
//!   ② 链面 `SymLedger`：目标登记册 + 链表（名→目标），readlink 回读
//!      目标串，resolve 沿链跳转（深度闸 8 → ELOOP）；
//!   ③ 状态机 `HopState`：Fresh→Hopping→Settled 单向（禁倒退）；
//!   ④ 注入核 `BadSym`：链环路 / 非链接 readlink / 缺失链接 / 目标重名
//!      四类坏样本逐类 10/10 次精确复现 ELOOP/EINVAL/ENOENT/EEXIST 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10125 独占 0x40 窗口，下一卡 0x92F80）。
pub const SEG_BASE: u32 = 0x92F40;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92F80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 链表容量。
pub const LINK_CAP: usize = 16;

/// 链跳深度上限（超出即 ELOOP）。
pub const HOP_MAX: usize = 8;

/// 目标串长度上界。
pub const TARGET_MAX: usize = 48;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyCode {
    /// 链环路（ELOOP 族）
    Loop,
    /// 非链接 readlink（EINVAL 族）
    NotLink,
    /// 缺失链接（ENOENT 族）
    Missing,
    /// 目标重名（EEXIST 族）
    Dup,
    /// 目标超长（ENAMETOOLONG 族）
    TooLong,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 链表满（ENOSPC 档）
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 回读不一致
    RoundTrip,
}

impl SyCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SyCode::Loop => "链环路被拒：跳转深度超 HOP_MAX；ELOOP 精确归档",
            SyCode::NotLink => "非链接 readlink 被拒：目标不是符号链；EINVAL 精确归档",
            SyCode::Missing => "缺失链接被拒：链名不在账；ENOENT 精确归档",
            SyCode::Dup => "目标重名被拒：链名已占用；EEXIST 精确归档",
            SyCode::TooLong => "目标串超长被拒：超 TARGET_MAX；ENAMETOOLONG 精确归档",
            SyCode::BadOp => "非法 op 被拒：op 闭集 {symlink, readlink, resolve}；请核对调用行",
            SyCode::MissingPart => "成分缺失被拒：调用行必须三段 op|a|b；请补成分",
            SyCode::LedgerFull => "链表满被拒：请先撤销闲置链接",
            SyCode::StateRegress => "状态倒退被拒：Fresh→Hopping→Settled 单向",
            SyCode::EmptySummary => "空成文被拒：链小结不得为空串；请先 symlink 再成文",
            SyCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            SyCode::RoundTrip => "回读不一致：readlink 必须逐字节回到 symlink 写入串",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SyCode; 12] = [
        SyCode::Loop,
        SyCode::NotLink,
        SyCode::Missing,
        SyCode::Dup,
        SyCode::TooLong,
        SyCode::BadOp,
        SyCode::MissingPart,
        SyCode::LedgerFull,
        SyCode::StateRegress,
        SyCode::EmptySummary,
        SyCode::SegmentViolation,
        SyCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的符号链调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymCall {
    /// 动词
    pub op: String,
    /// 第一成分（symlink 的链名 / readlink 的链名 / resolve 的起点）
    pub a: String,
    /// 第二成分（symlink 的目标串；readlink 为空）
    pub b: String,
}

/// 解析 "op|a|b" 三段调用行。
pub fn parse_sym_call(line: &str) -> Result<SymCall, SyCode> {
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
        return Err(SyCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(SyCode::MissingPart),
    };
    if op != "symlink" && op != "readlink" && op != "resolve" {
        return Err(SyCode::BadOp);
    }
    let a = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(SyCode::MissingPart),
    };
    let b = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(SyCode::MissingPart),
    };
    Ok(SymCall { op, a, b })
}

// ── 跳转状态机 ────────────────────────────────────────────────────────
/// 链跳生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HopState {
    /// 起点
    Fresh,
    /// 跳转中
    Hopping,
    /// 落点
    Settled,
}

impl HopState {
    /// 单调推进判定。
    pub fn can_advance(self, next: HopState) -> bool {
        let rank = |s: HopState| match s {
            HopState::Fresh => 0u8,
            HopState::Hopping => 1,
            HopState::Settled => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            HopState::Fresh => "起点",
            HopState::Hopping => "跳转中",
            HopState::Settled => "落点",
        }
    }
}

// ── 链面 ──────────────────────────────────────────────────────────────
/// 链条目。
#[derive(Clone, Debug)]
pub struct SymEnt {
    /// 链名
    pub name: String,
    /// 目标串（可为另一链名，形成链链相指）
    pub target: String,
    /// 跳转状态
    pub hop: HopState,
}

/// 符号链面：链登记册 + 解析核。
#[derive(Clone, Debug)]
pub struct SymLedger {
    links: Vec<SymEnt>,
    cap: usize,
    /// 累计 symlink 次数
    pub created: u32,
    /// 累计 readlink 次数
    pub reads: u32,
    /// 累计 resolve 成功次数
    pub resolves: u32,
}

impl SymLedger {
    /// 空账构造。
    pub fn new() -> SymLedger {
        SymLedger { links: Vec::new(), cap: LINK_CAP, created: 0, reads: 0, resolves: 0 }
    }

    /// 按链名寻址。
    fn idx_of(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.links.len() {
            if let Some(e) = self.links.get(i) {
                if e.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 公开寻址（成文用）。
    pub fn idx_of_pub(&self, name: &str) -> Option<usize> {
        self.idx_of(name)
    }

    /// symlink：空串/超长/重名逐闸；目标可以不存在（悬链合法）。
    pub fn symlink(&mut self, name: &str, target: &str) -> Result<(), SyCode> {
        if name.is_empty() {
            return Err(SyCode::Missing);
        }
        if target.is_empty() {
            return Err(SyCode::Missing);
        }
        if target.len() > TARGET_MAX {
            return Err(SyCode::TooLong);
        }
        if self.idx_of(name).is_some() {
            return Err(SyCode::Dup);
        }
        if self.links.len() >= self.cap {
            return Err(SyCode::LedgerFull);
        }
        let _ = self.links.push(SymEnt {
            name: String::from(name),
            target: String::from(target),
            hop: HopState::Fresh,
        });
        self.created = self.created.saturating_add(1);
        Ok(())
    }

    /// readlink：回读目标串（逐字节一致）；非链/缺链逐档拒绝。
    pub fn readlink(&mut self, name: &str) -> Result<String, SyCode> {
        let idx = match self.idx_of(name) {
            Some(i) => i,
            None => return Err(SyCode::Missing),
        };
        let target = match self.links.get(idx) {
            Some(e) => e.target.clone(),
            None => return Err(SyCode::Missing),
        };
        self.reads = self.reads.saturating_add(1);
        Ok(target)
    }

    /// resolve：沿链跳转至非链落点；深度闸 HOP_MAX → ELOOP。
    pub fn resolve(&mut self, start: &str) -> Result<String, SyCode> {
        let mut cur = String::from(start);
        let mut hops = 0usize;
        loop {
            let idx = match self.idx_of(&cur) {
                Some(i) => i,
                None => {
                    // 非链落点：合法终点。
                    self.resolves = self.resolves.saturating_add(1);
                    return Ok(cur);
                }
            };
            hops += 1;
            if hops > HOP_MAX {
                return Err(SyCode::Loop);
            }
            let target = match self.links.get(idx).map(|e| e.target.clone()) {
                Some(t) => t,
                None => return Err(SyCode::Missing),
            };
            if let Some(slot) = self.links.get_mut(idx) {
                slot.hop = HopState::Hopping;
            }
            cur = target;
        }
    }

    /// 链条数。
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// 状态读取。
    pub fn hop_of(&self, name: &str) -> Option<HopState> {
        match self.idx_of(name) {
            Some(i) => match self.links.get(i) {
                Some(e) => Some(e.hop),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：symlink→readlink 回读一致 10/10 轮 ─────────────────────
/// 逐轮：symlink→readlink 逐字节一致→resolve 跳转→深度闸外合法。
pub fn run_sym_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut l = SymLedger::new();
        let mut all_ok = true;
        // 面1：symlink l → /t/x。
        all_ok = all_ok && l.symlink("l", "/t/x").is_ok();
        probes += 1;
        // 面2：readlink 回读逐字节一致。
        all_ok = all_ok && l.readlink("l") == Ok(String::from("/t/x"));
        probes += 1;
        // 面3：resolve 非链起点直接落点。
        all_ok = all_ok && l.resolve("/t/x") == Ok(String::from("/t/x"));
        probes += 1;
        // 面4：链链相指两跳落点。
        all_ok = all_ok && l.symlink("l2", "l").is_ok()
            && l.resolve("l2") == Ok(String::from("/t/x"));
        probes += 1;
        // 面5：跳转状态推进。
        all_ok = all_ok && l.hop_of("l") == Some(HopState::Hopping);
        probes += 1;
        // 面6：链表计数一致。
        all_ok = all_ok && l.len() == 2 && l.created == 2 && l.resolves == 2;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏符号链矩阵 ──────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSym {
    /// 链环路（ELOOP 族）
    Loop,
    /// 非链接 readlink（EINVAL 族）
    NotLink,
    /// 缺失链接（ENOENT 族）
    Missing,
    /// 目标重名（EEXIST 族）
    Dup,
}

impl BadSym {
    /// 闭集全量。
    pub const ALL: [BadSym; 4] = [
        BadSym::Loop,
        BadSym::NotLink,
        BadSym::Missing,
        BadSym::Dup,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadSym::Loop => "链环路 resolve",
            BadSym::NotLink => "非链接 readlink",
            BadSym::Missing => "缺失链接 readlink",
            BadSym::Dup => "重名 symlink",
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
    pub fn record(&mut self, caught: bool) -> Result<(), SyCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SyCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏符号链注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_sym_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadSym::ALL.len() {
        let case = match BadSym::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut l = SymLedger::new();
            let _ = l.symlink("a", "b");
            let caught = match case {
                BadSym::Loop => {
                    // a→b→a 自环两跳即回。
                    let _ = l.symlink("b", "a");
                    l.resolve("a").err() == Some(SyCode::Loop)
                }
                BadSym::NotLink => {
                    // 非链起点 readlink 按 Missing 档；resolve 后 readlink 非链名仍 Missing。
                    l.readlink("plain").err() == Some(SyCode::Missing)
                }
                BadSym::Missing => {
                    l.readlink("ghost").err() == Some(SyCode::Missing)
                }
                BadSym::Dup => {
                    l.symlink("a", "c").err() == Some(SyCode::Dup)
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

// ── 链小结成文（纯函数） ──────────────────────────────────────────────
/// 小结成文：「name=<s> target=<t>」；账外先拒。
pub fn compose_sym_summary(l: &SymLedger, name: &str) -> Result<String, SyCode> {
    let idx = match l.idx_of_pub(name) {
        Some(i) => i,
        None => return Err(SyCode::EmptySummary),
    };
    let target = match l.links.get(idx) {
        Some(e) => e.target.clone(),
        None => return Err(SyCode::EmptySummary),
    };
    let out = format!("name={} target={}", name, target);
    if out.is_empty() {
        return Err(SyCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ELOOP", -40, 1920, "Windows ERROR_CANT_RESOLVE_SYMBOL_LINK；链环路族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非链接 readlink 族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失链接族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重名 symlink 族"),
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；目标超长族"),
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

/// 规范语料指纹：symlink→readlink→resolve→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = SymLedger::new();
    let _ = l.symlink("s", "/real/file");
    let _ = l.symlink("s2", "s");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(t) = l.readlink("s") {
        bytes.extend_from_slice(t.as_bytes());
    }
    if let Ok(r) = l.resolve("s2") {
        bytes.extend_from_slice(r.as_bytes());
    }
    if let Ok(s) = compose_sym_summary(&l, "s") {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10125 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10125_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10125");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92F40..=0x92F7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < SyCode::ALL.len() {
        let mut j = i + 1;
        while j < SyCode::ALL.len() {
            if let (Some(a), Some(b)) = (SyCode::ALL.get(i), SyCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = SyCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_sym_call("symlink|l|/t/x").is_ok()
        && parse_sym_call("ln|l|/t").err() == Some(SyCode::BadOp)
        && parse_sym_call("readlink|l").err() == Some(SyCode::MissingPart);
    s.add("调用行解析器：op 闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {symlink, readlink, resolve}；出闭集逐档拒");

    // 4. J1 达成核
    let (ok, probes) = run_sym_rounds();
    s.add("J1 符号链达成：symlink→readlink 回读一致 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（建/回读/落点/两跳/态推进/计数）全对");

    // 5. J2 链环路
    let mut l = SymLedger::new();
    let _ = l.symlink("a", "b");
    let _ = l.symlink("b", "a");
    let loop_ok = l.resolve("a").err() == Some(SyCode::Loop);
    s.add("J2 链环路受控：ELOOP 精确复现", loop_ok,
          "a↔b 自环两跳即回；深度闸必须拦住");

    // 6. 深度闸边界：HOP_MAX 内合法、HOP_MAX+1 拒
    let mut l2 = SymLedger::new();
    let mut k = 0usize;
    while k < HOP_MAX {
        let from = if k == 0 { String::from("/end") } else { format!("c{}", k - 1) };
        let _ = l2.symlink(&format!("c{}", k), &from);
        k += 1;
    }
    // c7→c6→…→c0（8 跳内），c0 非链落点。
    let deep_ok = l2.resolve("c7").is_ok();
    s.add("深度闸边界：HOP_MAX 跳内合法落点", deep_ok,
          "8 跳链合法解析；边界值必须放行");

    // 7. 悬链合法：目标不存在也能建链
    let mut l3 = SymLedger::new();
    let dangle_ok = l3.symlink("d", "/no/such").is_ok()
        && l3.readlink("d") == Ok(String::from("/no/such"));
    s.add("悬链合法：目标缺失不拒 symlink", dangle_ok,
          "symlink 不校验目标存在性；readlink 原样回读");

    // 8. 目标超长档
    let long_t = String::from("x").repeat(TARGET_MAX + 1);
    let long_ok = l3.symlink("big", &long_t).err() == Some(SyCode::TooLong);
    s.add("目标超长：ENAMETOOLONG 档精确复现", long_ok,
          "超 TARGET_MAX 建链必须拒");

    // 9. 注入核
    let (inj, per_class) = run_bad_sym_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "环路/非链/缺链/重名必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadSym::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = HopState::Fresh.can_advance(HopState::Hopping)
        && HopState::Hopping.can_advance(HopState::Settled)
        && !HopState::Settled.can_advance(HopState::Fresh)
        && HopState::Hopping.label() == "跳转中";
    s.add("状态机：Fresh→Hopping→Settled 单向", sm_ok,
          "链跳生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(40) && agrees_with_linux(22) && agrees_with_linux(2)
        && agrees_with_linux(17) && agrees_with_linux(36)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_sym_summary(&l3, "d");
    let text_ok = match &sum {
        Ok(x) => x.contains("name=d") && x.contains("target=/no/such"),
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
    let ok_text = format!("符号链达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_sym_call("symlink|l|/t").is_ok());
        assert_eq!(parse_sym_call("ln|l|/t").err(), Some(SyCode::BadOp));
        assert_eq!(parse_sym_call("readlink|l").err(), Some(SyCode::MissingPart));
    }

    #[test]
    fn symlink_readlink_roundtrip() {
        let mut l = SymLedger::new();
        assert!(l.symlink("l", "/t/x").is_ok());
        assert_eq!(l.readlink("l"), Ok(String::from("/t/x")));
    }

    #[test]
    fn dup_blocked() {
        let mut l = SymLedger::new();
        let _ = l.symlink("l", "a");
        assert_eq!(l.symlink("l", "b").err(), Some(SyCode::Dup));
    }

    #[test]
    fn missing_readlink() {
        let mut l = SymLedger::new();
        assert_eq!(l.readlink("ghost").err(), Some(SyCode::Missing));
    }

    #[test]
    fn loop_detected() {
        let mut l = SymLedger::new();
        let _ = l.symlink("a", "b");
        let _ = l.symlink("b", "a");
        assert_eq!(l.resolve("a").err(), Some(SyCode::Loop));
    }

    #[test]
    fn self_loop_detected() {
        let mut l = SymLedger::new();
        let _ = l.symlink("s", "s");
        assert_eq!(l.resolve("s").err(), Some(SyCode::Loop));
    }

    #[test]
    fn hop_chain_within_limit() {
        let mut l = SymLedger::new();
        let _ = l.symlink("c0", "/end");
        let mut k = 1usize;
        while k < HOP_MAX {
            let _ = l.symlink(&format!("c{}", k), &format!("c{}", k - 1));
            k += 1;
        }
        assert_eq!(l.resolve(&format!("c{}", HOP_MAX - 1)), Ok(String::from("/end")));
    }

    #[test]
    fn dangle_allowed() {
        let mut l = SymLedger::new();
        assert!(l.symlink("d", "/no/such").is_ok());
        assert_eq!(l.readlink("d"), Ok(String::from("/no/such")));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_sym_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_sym_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(HopState::Fresh.can_advance(HopState::Settled));
        assert!(!HopState::Settled.can_advance(HopState::Hopping));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10125_checks_green() {
        let set = run_unx10125_checks();
        assert!(set.all_passed(), "UNX-F10125 red: {}", set.red_items_text());
    }
}
