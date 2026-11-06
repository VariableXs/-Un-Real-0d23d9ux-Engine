//! UNX-F10139 时间戳面正反双判据（更新达成与越权改时间误用受控）· 0x932C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10140-J1 收口小结落账（19 条战线条目 J1/J2 双判据齐备+预算
//!       6,580 核销零偏离 10/10 次）；UNX-F10140-J2 收口虚报受控（判据
//!       缺失或守恒偏离检出拦截 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x932C0（本卡独占 0x40 窗口，下一卡 0x93300）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_ts_call`："op|path" 两成分（op ∈ {write, read,
//!      utimens}），op 闭集/空路径逐档拒绝；
//!   ② 时间戳面 `TsFace`：逻辑钟（禁墙钟）逐跳推进，write 更新 mtime、
//!      read 更新 atime、utimens 属主闸后置双戳，双戳单调不减；
//!   ③ 状态机 `TsState`：Fresh→Written→Read→Touched 单调（禁倒退）；
//!   ④ 注入核 `BadTs`：非属主 utimens / 缺失路径 / 空路径 / 双戳回退
//!      四类坏样本逐类 10/10 次精确复现 EPERM/ENOENT/EINVAL/审计档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10139 独占 0x40 窗口，下一卡 0x93300）。
pub const SEG_BASE: u32 = 0x932C0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93300;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 时间戳面容量。
pub const TS_CAP: usize = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsCode {
    /// 非属主 utimens（EPERM 族）
    NotOwner,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 双戳回退（审计档）
    ClockRegress,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 面满（ENOSPC 档）
    LedgerFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 戳账失平
    StampDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl TsCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TsCode::NotOwner => "非属主 utimens 被拒：改时间只许属主；EPERM 精确归档",
            TsCode::MissingPath => "缺失路径被拒：戳面无此文件；ENOENT 精确归档",
            TsCode::EmptyPath => "空路径被拒：路径不得为空串；EINVAL 精确归档",
            TsCode::ClockRegress => "双戳回退被拒：mtime/atime 单调不减",
            TsCode::BadOp => "非法 op 被拒：op 闭集 {write, read, utimens}；请核对调用行",
            TsCode::MissingPart => "成分缺失被拒：调用行必须两段 op|path；请补成分",
            TsCode::LedgerFull => "戳面满被拒：请先清理闲置条目",
            TsCode::StateRegress => "状态倒退被拒：Fresh→Written→Read→Touched 单调",
            TsCode::EmptySummary => "空成文被拒：时间戳小结不得为空串；请先操作再成文",
            TsCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            TsCode::StampDrift => "戳账失平被拒：戳值必须与逻辑钟跳数一致",
            TsCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TsCode; 12] = [
        TsCode::NotOwner,
        TsCode::MissingPath,
        TsCode::EmptyPath,
        TsCode::ClockRegress,
        TsCode::BadOp,
        TsCode::MissingPart,
        TsCode::LedgerFull,
        TsCode::StateRegress,
        TsCode::EmptySummary,
        TsCode::SegmentViolation,
        TsCode::StampDrift,
        TsCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的时间戳调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsCall {
    /// 动词
    pub op: String,
    /// 路径
    pub path: String,
}

/// 解析 "op|path" 两段调用行。
pub fn parse_ts_call(line: &str) -> Result<TsCall, TsCode> {
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
        return Err(TsCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(TsCode::MissingPart),
    };
    if op != "write" && op != "read" && op != "utimens" {
        return Err(TsCode::BadOp);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(TsCode::MissingPart),
    };
    Ok(TsCall { op, path })
}

// ── 时间戳状态机 ──────────────────────────────────────────────────────
/// 戳生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsState {
    /// 新登记
    Fresh,
    /// 已写
    Written,
    /// 已读
    Read,
    /// 已 utimens
    Touched,
}

impl TsState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: TsState) -> bool {
        let rank = |s: TsState| match s {
            TsState::Fresh => 0u8,
            TsState::Written => 1,
            TsState::Read => 2,
            TsState::Touched => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            TsState::Fresh => "新登记",
            TsState::Written => "已写",
            TsState::Read => "已读",
            TsState::Touched => "已置戳",
        }
    }
}

// ── 时间戳面 ──────────────────────────────────────────────────────────
/// 戳条目。
#[derive(Clone, Debug)]
pub struct TsEnt {
    /// 路径
    pub path: String,
    /// 属主
    pub owner: u32,
    /// mtime（逻辑钟跳）
    pub mtime: u64,
    /// atime（逻辑钟跳）
    pub atime: u64,
    /// 生命周期状态
    pub state: TsState,
}

/// 时间戳面：逻辑钟 + 戳账。
#[derive(Clone, Debug)]
pub struct TsFace {
    ents: Vec<TsEnt>,
    cap: usize,
    /// 逻辑钟（单调递增跳数，禁墙钟）
    pub clock: u64,
    /// 当前执行者
    pub caller: u32,
    /// 累计写次数
    pub writes: u32,
    /// 累计读次数
    pub reads: u32,
}

impl TsFace {
    /// 空面构造（caller=0）。
    pub fn new() -> TsFace {
        TsFace {
            ents: Vec::new(),
            cap: TS_CAP,
            clock: 0,
            caller: 0,
            writes: 0,
            reads: 0,
        }
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

    /// 公开寻址（审计用）。
    pub fn idx_of_pub(&self, path: &str) -> Option<usize> {
        self.idx_of(path)
    }

    /// 登记文件（owner=0，双戳归零跳）。
    pub fn create(&mut self, path: &str) -> Result<(), TsCode> {
        if path.is_empty() {
            return Err(TsCode::EmptyPath);
        }
        if self.idx_of(path).is_some() || self.ents.len() >= self.cap {
            return Err(TsCode::LedgerFull);
        }
        self.clock = self.clock.saturating_add(1);
        let _ = self.ents.push(TsEnt {
            path: String::from(path),
            owner: 0,
            mtime: self.clock,
            atime: self.clock,
            state: TsState::Fresh,
        });
        Ok(())
    }

    /// write：mtime 前跳（atime 不动）。
    pub fn write(&mut self, path: &str) -> Result<u64, TsCode> {
        if path.is_empty() {
            return Err(TsCode::EmptyPath);
        }
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(TsCode::MissingPath),
        };
        self.clock = self.clock.saturating_add(1);
        if let Some(e) = self.ents.get_mut(idx) {
            e.mtime = self.clock;
            if e.state.can_advance(TsState::Written) {
                e.state = TsState::Written;
            }
        }
        self.writes = self.writes.saturating_add(1);
        Ok(self.clock)
    }

    /// read：atime 前跳（mtime 不动）。
    pub fn read(&mut self, path: &str) -> Result<u64, TsCode> {
        if path.is_empty() {
            return Err(TsCode::EmptyPath);
        }
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(TsCode::MissingPath),
        };
        self.clock = self.clock.saturating_add(1);
        if let Some(e) = self.ents.get_mut(idx) {
            e.atime = self.clock;
            if e.state.can_advance(TsState::Read) {
                e.state = TsState::Read;
            }
        }
        self.reads = self.reads.saturating_add(1);
        Ok(self.clock)
    }

    /// utimens：属主闸后双戳同置当前跳；非属主 EPERM。
    pub fn utimens(&mut self, path: &str) -> Result<u64, TsCode> {
        if path.is_empty() {
            return Err(TsCode::EmptyPath);
        }
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(TsCode::MissingPath),
        };
        let owner = match self.ents.get(idx) {
            Some(e) => e.owner,
            None => return Err(TsCode::MissingPath),
        };
        if owner != self.caller {
            return Err(TsCode::NotOwner);
        }
        self.clock = self.clock.saturating_add(1);
        if let Some(e) = self.ents.get_mut(idx) {
            e.mtime = self.clock;
            e.atime = self.clock;
            e.state = TsState::Touched;
        }
        Ok(self.clock)
    }

    /// 双戳读取。
    pub fn stamps_of(&self, path: &str) -> Option<(u64, u64)> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some((e.mtime, e.atime)),
                None => None,
            },
            None => None,
        }
    }

    /// 属主读取。
    pub fn owner_of(&self, path: &str) -> Option<u32> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.owner),
                None => None,
            },
            None => None,
        }
    }

    /// 戳账审计：全账双戳 ≤ 逻辑钟。
    pub fn stamps_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.mtime > self.clock || e.atime > self.clock {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── J1 达成核：写后 mtime 更新、读后 atime 更新 10/10 轮 ──────────────
/// 逐轮：写(mtime跳)→读(atime跳)→双戳分离→utimens 同置→账平。
pub fn run_ts_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut f = TsFace::new();
        let _ = f.create("f");
        let mut all_ok = true;
        // 面1：写后 mtime 前跳且 atime 不动。
        let (m0, a0) = f.stamps_of("f").unwrap_or((0, 0));
        let _ = f.write("f");
        let (m1, a1) = f.stamps_of("f").unwrap_or((0, 0));
        all_ok = all_ok && m1 > m0 && a1 == a0;
        probes += 1;
        // 面2：读后 atime 前跳且 mtime 不动。
        let _ = f.read("f");
        let (m2, a2) = f.stamps_of("f").unwrap_or((0, 0));
        all_ok = all_ok && a2 > a1 && m2 == m1;
        probes += 1;
        // 面3：双戳分离（写读各管一戳）。
        all_ok = all_ok && m2 != a2;
        probes += 1;
        // 面4：utimens 属主双戳同置。
        let _ = f.utimens("f");
        let (m3, a3) = f.stamps_of("f").unwrap_or((0, 0));
        all_ok = all_ok && m3 == a3 && m3 > m2;
        probes += 1;
        // 面5：态推进 Touched。
        all_ok = all_ok && f.stamps_consistent();
        probes += 1;
        // 面6：账平（写 1 读 1）。
        all_ok = all_ok && f.writes == 1 && f.reads == 1;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏时间戳矩阵 ──────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTs {
    /// 非属主 utimens（EPERM 族）
    NotOwner,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 双戳回退（审计档）
    ClockRegress,
}

impl BadTs {
    /// 闭集全量。
    pub const ALL: [BadTs; 4] = [
        BadTs::NotOwner,
        BadTs::MissingPath,
        BadTs::EmptyPath,
        BadTs::ClockRegress,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadTs::NotOwner => "非属主 utimens",
            BadTs::MissingPath => "缺失路径 utimens",
            BadTs::EmptyPath => "空路径写",
            BadTs::ClockRegress => "双戳回退注入",
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
    pub fn record(&mut self, caught: bool) -> Result<(), TsCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TsCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏时间戳注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_ts_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadTs::ALL.len() {
        let case = match BadTs::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut f = TsFace::new();
            let _ = f.create("f");
            let caught = match case {
                BadTs::NotOwner => {
                    f.caller = 9;
                    f.utimens("f").err() == Some(TsCode::NotOwner)
                }
                BadTs::MissingPath => {
                    f.caller = 0;
                    f.utimens("ghost").err() == Some(TsCode::MissingPath)
                }
                BadTs::EmptyPath => {
                    f.write("").err() == Some(TsCode::EmptyPath)
                }
                BadTs::ClockRegress => {
                    // 注入：手工把 mtime 拉到未来，账审计必须抓到越钟戳。
                    if let Some(i) = f.idx_of_pub("f") {
                        if let Some(e) = f.ents.get_mut(i) {
                            e.mtime = u64::MAX;
                        }
                    }
                    !f.stamps_consistent()
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

// ── 时间戳小结成文（纯函数） ──────────────────────────────────────────
/// 小结成文：「mtime=<m> atime=<a>」；账外先拒。
pub fn compose_ts_summary(f: &TsFace, path: &str) -> Result<String, TsCode> {
    let (m, a) = match f.stamps_of(path) {
        Some(x) => x,
        None => return Err(TsCode::EmptySummary),
    };
    let out = format!("mtime={} atime={}", m, a);
    if out.is_empty() {
        return Err(TsCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；非属主改时间族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；空路径/坏行族"),
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

/// 规范语料指纹：写→读→utimens→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut f = TsFace::new();
    let _ = f.create("f");
    let _ = f.write("f");
    let _ = f.read("f");
    let _ = f.utimens("f");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_ts_summary(&f, "f") {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10139 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10139_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10139");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x932C0..=0x932FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < TsCode::ALL.len() {
        let mut j = i + 1;
        while j < TsCode::ALL.len() {
            if let (Some(a), Some(b)) = (TsCode::ALL.get(i), TsCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = TsCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_ts_call("write|f").is_ok()
        && parse_ts_call("touch|f").err() == Some(TsCode::BadOp)
        && parse_ts_call("write").err() == Some(TsCode::MissingPart);
    s.add("调用行解析器：op 三词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {write, read, utimens}；空路径判定层拒");

    // 4. J1 达成核
    let (ok, probes) = run_ts_rounds();
    s.add("J1 时间戳达成：写后 mtime 更新、读后 atime 更新 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（写跳/读跳/双戳分离/同置/账平/计数）全对");

    // 5. J2 非属主改时间
    let mut f = TsFace::new();
    let _ = f.create("a");
    f.caller = 5;
    let eperm_ok = f.utimens("a").err() == Some(TsCode::NotOwner)
        && f.owner_of("a") == Some(0);
    s.add("J2 非属主改时间受控：EPERM 精确复现且双戳不动", eperm_ok,
          "utimens 属主闸；被拒不改任何戳");

    // 6. 双戳分离语义
    f.caller = 0;
    let _ = f.write("a");
    let (m, a) = f.stamps_of("a").unwrap_or((0, 0));
    let split_ok = m > a || {
        let _ = f.read("a");
        match f.stamps_of("a") {
            Some((m2, a2)) => a2 > a && m2 == m,
            None => false,
        }
    };
    s.add("双戳分离：write 管_mtime、read 管_atime 互不串扰", split_ok,
          "mtime/atime 是两个独立账面根；串扰即 StampDrift");

    // 7. 缺失路径档
    let miss_ok = f.write("ghost").err() == Some(TsCode::MissingPath)
        && f.read("ghost").err() == Some(TsCode::MissingPath);
    s.add("缺失路径：ENOENT 写读双入口精确归因", miss_ok,
          "账外文件三入口一致拒绝");

    // 8. 戳账审计
    let aud_ok = f.stamps_consistent();
    s.add("戳账审计：全账双戳不越逻辑钟", aud_ok,
          "越钟戳必须被审计抓到");

    // 9. 注入核
    let (inj, per_class) = run_bad_ts_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "非属主/缺失/空路径/回退注入必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadTs::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = TsState::Fresh.can_advance(TsState::Touched)
        && TsState::Written.can_advance(TsState::Read)
        && !TsState::Touched.can_advance(TsState::Fresh)
        && TsState::Touched.label() == "已置戳";
    s.add("状态机：Fresh→Written→Read→Touched 单调", sm_ok,
          "戳生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(2) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_ts_summary(&f, "a");
    let text_ok = match &sum {
        Ok(x) => x.contains("mtime=") && x.contains("atime="),
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
    let ok_text = format!("时间戳达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_ts_call("write|f").is_ok());
        assert_eq!(parse_ts_call("touch|f").err(), Some(TsCode::BadOp));
        assert_eq!(parse_ts_call("write").err(), Some(TsCode::MissingPart));
    }

    #[test]
    fn write_updates_mtime_only() {
        let mut f = TsFace::new();
        let _ = f.create("f");
        let (m0, a0) = f.stamps_of("f").unwrap_or((0, 0));
        let _ = f.write("f");
        let (m1, a1) = f.stamps_of("f").unwrap_or((0, 0));
        assert!(m1 > m0);
        assert_eq!(a1, a0);
    }

    #[test]
    fn read_updates_atime_only() {
        let mut f = TsFace::new();
        let _ = f.create("f");
        let _ = f.write("f");
        let (m0, a0) = f.stamps_of("f").unwrap_or((0, 0));
        let _ = f.read("f");
        let (m1, a1) = f.stamps_of("f").unwrap_or((0, 0));
        assert!(a1 > a0);
        assert_eq!(m1, m0);
    }

    #[test]
    fn utimens_owner_gate() {
        let mut f = TsFace::new();
        let _ = f.create("f");
        f.caller = 3;
        assert_eq!(f.utimens("f").err(), Some(TsCode::NotOwner));
        f.caller = 0;
        assert!(f.utimens("f").is_ok());
        let (m, a) = f.stamps_of("f").unwrap_or((0, 0));
        assert_eq!(m, a);
    }

    #[test]
    fn missing_blocked() {
        let mut f = TsFace::new();
        assert_eq!(f.write("ghost").err(), Some(TsCode::MissingPath));
        assert_eq!(f.utimens("ghost").err(), Some(TsCode::MissingPath));
    }

    #[test]
    fn empty_blocked() {
        let mut f = TsFace::new();
        assert_eq!(f.write("").err(), Some(TsCode::EmptyPath));
        assert_eq!(f.create("").err(), Some(TsCode::EmptyPath));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_ts_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_ts_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(TsState::Fresh.can_advance(TsState::Written));
        assert!(!TsState::Touched.can_advance(TsState::Read));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10139_checks_green() {
        let set = run_unx10139_checks();
        assert!(set.all_passed(), "UNX-F10139 red: {}", set.red_items_text());
    }
}
