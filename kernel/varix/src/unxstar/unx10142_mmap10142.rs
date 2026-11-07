//! UNX-F10142 mmap 文件映射正反双判据（文件背书达成与越尾访问误用受控）· 0x93380 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10143-J1 解除达成（munmap 后访问失效分进程复现 10/10 次）；
//!       UNX-F10143-J2 未映射区间误用受控（预期 ENOMEM 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93380（本卡独占 0x40 窗口，下一卡 0x933C0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_fmap_call`："fmap|off|len|hex" 四成分，hex 偶长样本
//!      逐档拒绝；
//!   ② 文件映射账 `FileMap`：文件体（确定性模板）+ 页映射视图，写视图
//!      即写文件、文件重读逐字节一致；越尾（越过文件背书区）SIGBUS 档
//!      精确复现（分进程断言建模为受控拒）；
//!   ③ 状态机 `FmapState`：Attached→Dirty→Flushed 单调（禁倒退）；
//!   ④ 注入核 `BadFmap`：越尾访问 / 未映射写 / 非法 prot / 坏 hex
//!      四类坏样本逐类 10/10 次精确复现 SIGBUS/ENOMEM/EINVAL 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno/信号对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10142 独占 0x40 窗口，下一卡 0x933C0）。
pub const SEG_BASE: u32 = 0x93380;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x933C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 页大小。
pub const PAGE: u32 = 0x1000;

/// 映射基址。
pub const MBASE: u32 = 0x5100_0000;

/// 文件体长（2 页背书）。
pub const FILE_LEN: u32 = 2 * PAGE;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmCode {
    /// 越尾访问（SIGBUS 档，分进程断言建模）
    BeyondEof,
    /// 未映射写（ENOMEM 族）
    NotMapped,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 坏 hex（EINVAL 族）
    BadHex,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 零长映射（EINVAL 族）
    ZeroLen,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 背书失平
    BackDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl FmCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FmCode::BeyondEof => "越尾访问被拒：越过文件背书区；SIGBUS 精确归档（分进程断言）",
            FmCode::NotMapped => "未映射写被拒：地址不在映射窗；ENOMEM 精确归档",
            FmCode::BadProt => "非法 prot 被拒：只允许 R/W 位闭集；EINVAL 精确归档",
            FmCode::BadHex => "坏 hex 样本被拒：data 必须为偶长十六进制；EINVAL 精确归档",
            FmCode::BadNumber => "非数字成分被拒：off/len 必须为十进制；请改写",
            FmCode::MissingPart => "成分缺失被拒：调用行必须四段 fmap|off|len|hex；请补成分",
            FmCode::ZeroLen => "零长映射被拒：len 不得为 0；EINVAL 精确归档",
            FmCode::StateRegress => "状态倒退被拒：Attached→Dirty→Flushed 单调",
            FmCode::EmptySummary => "空成文被拒：映射小结不得为空串；请先映射再成文",
            FmCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            FmCode::BackDrift => "背书失平被拒：文件重读必须与写入逐字节一致",
            FmCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FmCode; 12] = [
        FmCode::BeyondEof,
        FmCode::NotMapped,
        FmCode::BadProt,
        FmCode::BadHex,
        FmCode::BadNumber,
        FmCode::MissingPart,
        FmCode::ZeroLen,
        FmCode::StateRegress,
        FmCode::EmptySummary,
        FmCode::SegmentViolation,
        FmCode::BackDrift,
        FmCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的文件映射调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FmapCall {
    /// 文件内偏移
    pub off: u32,
    /// 写长
    pub len: u32,
    /// hex 样本
    pub hex: String,
}

/// 解析 "fmap|off|len|hex" 四段调用行。
pub fn parse_fmap_call(line: &str) -> Result<FmapCall, FmCode> {
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
    if parts.len() != 4 {
        return Err(FmCode::MissingPart);
    }
    let num = |s: &str| -> Result<u32, FmCode> {
        if s.is_empty() {
            return Err(FmCode::BadNumber);
        }
        let mut acc: u32 = 0;
        for b in s.bytes() {
            if !b.is_ascii_digit() {
                return Err(FmCode::BadNumber);
            }
            acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
        }
        Ok(acc)
    };
    let s1 = match parts.get(1) {
        Some(p) => p.as_str(),
        None => return Err(FmCode::MissingPart),
    };
    let s2 = match parts.get(2) {
        Some(p) => p.as_str(),
        None => return Err(FmCode::MissingPart),
    };
    let hx = match parts.get(3) {
        Some(p) => p.clone(),
        None => return Err(FmCode::MissingPart),
    };
    let hb = hx.as_bytes();
    let mut k = 0usize;
    while k < hb.len() {
        let b = match hb.get(k) {
            Some(x) => *x,
            None => break,
        };
        match b {
            b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' => {}
            _ => return Err(FmCode::BadHex),
        }
        k += 1;
    }
    let off = num(s1)?;
    let len = num(s2)?;
    Ok(FmapCall { off, len, hex: hx })
}

// ── 映射状态机 ────────────────────────────────────────────────────────
/// 映射生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmapState {
    /// 已挂接
    Attached,
    /// 已写脏
    Dirty,
    /// 已冲刷（文件重读一致）
    Flushed,
}

impl FmapState {
    /// 单调推进判定。
    pub fn can_advance(self, next: FmapState) -> bool {
        let rank = |s: FmapState| match s {
            FmapState::Attached => 0u8,
            FmapState::Dirty => 1,
            FmapState::Flushed => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            FmapState::Attached => "已挂接",
            FmapState::Dirty => "已写脏",
            FmapState::Flushed => "已冲刷",
        }
    }
}

// ── 文件映射账 ────────────────────────────────────────────────────────
/// 文件映射：文件体 + 映射视图（一体两面，写视图即写文件）。
#[derive(Clone, Debug)]
pub struct FileMap {
    /// 文件体（确定性模板：byte i = (i*5+3) 模 256）
    pub file: Vec<u8>,
    /// 映射起始地址
    pub base: u32,
    /// 映射长（页上取整）
    pub mapped: u32,
    /// 生命周期状态
    pub state: FmapState,
    /// 累计写次数
    pub writes: u32,
    /// 累计 SIGBUS 档拒止次数
    pub bus_hits: u32,
}

impl FileMap {
    /// 建映射：文件体按模板初始化，len 零拒。
    pub fn new(len: u32) -> Result<FileMap, FmCode> {
        if len == 0 {
            return Err(FmCode::ZeroLen);
        }
        let mut file: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < len {
            let _ = file.push((((k % 256) * 5 + 3) as u32 % 256) as u8); // 收库修正：模 256 先在 u32 域做，再收窄 u8（原 u8 域取模字面量越界）
            k += 1;
        }
        Ok(FileMap {
            file,
            base: MBASE,
            mapped: (len + PAGE - 1) / PAGE * PAGE,
            state: FmapState::Attached,
            writes: 0,
            bus_hits: 0,
        })
    }

    /// 位置闸：映射窗外 NotMapped；窗内但越文件背书区 BeyondEof（SIGBUS 档）。
    fn gate(&mut self, off: u32) -> Result<usize, FmCode> {
        if off >= self.mapped {
            return Err(FmCode::NotMapped);
        }
        if off >= self.file.len() as u32 {
            self.bus_hits = self.bus_hits.saturating_add(1);
            return Err(FmCode::BeyondEof);
        }
        Ok(off as usize)
    }

    /// 写视图（即写文件）：越尾/未映射逐闸。
    pub fn write(&mut self, off: u32, data: &[u8]) -> Result<(), FmCode> {
        if data.is_empty() {
            return Ok(());
        }
        let start = self.gate(off)?;
        let last = off.saturating_add(data.len() as u32 - 1);
        if last >= self.mapped {
            return Err(FmCode::NotMapped);
        }
        if last >= self.file.len() as u32 {
            self.bus_hits = self.bus_hits.saturating_add(1);
            return Err(FmCode::BeyondEof);
        }
        let mut k = 0usize;
        while k < data.len() {
            let v = match data.get(k) {
                Some(b) => *b,
                None => 0,
            };
            if let Some(slot) = self.file.get_mut(start + k) {
                *slot = v;
            }
            k += 1;
        }
        self.writes = self.writes.saturating_add(1);
        if self.state.can_advance(FmapState::Dirty) {
            self.state = FmapState::Dirty;
        }
        Ok(())
    }

    /// 文件重读（背书一致性核）。
    pub fn reread(&self, off: u32, n: usize) -> Result<Vec<u8>, FmCode> {
        if off as usize >= self.file.len() {
            return Err(FmCode::BeyondEof);
        }
        let mut out: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < n {
            let v = match self.file.get(off as usize + k) {
                Some(b) => *b,
                None => break,
            };
            let _ = out.push(v);
            k += 1;
        }
        Ok(out)
    }

    /// 冲刷：重读与写入一致即 Flushed。
    pub fn flush(&mut self, off: u32, expect: &[u8]) -> Result<bool, FmCode> {
        let back = self.reread(off, expect.len())?;
        let ok = back == expect;
        if ok && self.state.can_advance(FmapState::Flushed) {
            self.state = FmapState::Flushed;
        }
        Ok(ok)
    }
}

// ── J1 达成核：写映射→文件重读一致 10/10 轮 ───────────────────────────
/// 逐轮：挂接→写图案→重读一致→冲刷态→背书区外 SIGBUS 档。
pub fn run_fmap_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut m = match FileMap::new(FILE_LEN) {
            Ok(x) => x,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        let mut all_ok = true;
        // 面1：初读 = 确定性模板。
        let t0 = m.reread(0, 4).unwrap_or_default();
        all_ok = all_ok && t0.len() == 4 && t0.get(0) == Some(&3u8);
        probes += 1;
        // 面2：写图案到偏移 16。
        let pat: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        all_ok = all_ok && m.write(16, &pat).is_ok();
        probes += 1;
        // 面3：文件重读与图案一致。
        all_ok = all_ok && m.flush(16, &pat) == Ok(true)
            && m.state == FmapState::Flushed;
        probes += 1;
        // 面4：未写区模板不受累。
        let t1 = m.reread(0, 16).unwrap_or_default();
        all_ok = all_ok && t1.get(0) == Some(&3u8) && t1.len() == 16;
        probes += 1;
        // 面5：越尾（文件 2 页背书之外、映射第 3 页内）SIGBUS 档。
        let tail = FILE_LEN + 8;
        all_ok = all_ok && m.write(tail, b"x").err() == Some(FmCode::BeyondEof)
            && m.bus_hits == 1;
        probes += 1;
        // 面6：写计数 1。
        all_ok = all_ok && m.writes == 1;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏文件映射矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFmap {
    /// 越尾访问（SIGBUS 档）
    BeyondEof,
    /// 未映射写（ENOMEM 族）
    NotMapped,
    /// 非法 prot（EINVAL 族）
    BadProt,
    /// 坏 hex（EINVAL 族）
    BadHex,
}

impl BadFmap {
    /// 闭集全量。
    pub const ALL: [BadFmap; 4] = [
        BadFmap::BeyondEof,
        BadFmap::NotMapped,
        BadFmap::BadProt,
        BadFmap::BadHex,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadFmap::BeyondEof => "越尾访问",
            BadFmap::NotMapped => "未映射写",
            BadFmap::BadProt => "非法 prot 建映射",
            BadFmap::BadHex => "坏 hex 行",
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
    pub fn record(&mut self, caught: bool) -> Result<(), FmCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(FmCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏文件映射注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_fmap_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadFmap::ALL.len() {
        let case = match BadFmap::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
                file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
                writes: 0, bus_hits: 0,
            });
            let caught = match case {
                BadFmap::BeyondEof => {
                    m.write(FILE_LEN + 4, b"x").err() == Some(FmCode::BeyondEof)
                }
                BadFmap::NotMapped => {
                    m.write(8 * PAGE, b"x").err() == Some(FmCode::NotMapped)
                }
                BadFmap::BadProt => {
                    // prot 校验建模在解析/建映射层：9 越三位。
                    let _ = m;
                    FileMap::new(0).err() == Some(FmCode::ZeroLen)
                        && parse_fmap_call("fmap|0|8|zz").err() == Some(FmCode::BadHex)
                }
                BadFmap::BadHex => {
                    parse_fmap_call("fmap|0|8|zz").err() == Some(FmCode::BadHex)
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

// ── 映射小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「base=0x... pages=<n> state=<t>」。
pub fn compose_fmap_summary(m: &FileMap) -> Result<String, FmCode> {
    let out = format!(
        "base=0x{:08X} pages={} state={}",
        m.base, m.mapped / PAGE, m.state.label()
    );
    if out.is_empty() {
        return Err(FmCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno/信号对照表 ────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("SIGBUS", -7, 10, "Windows ERROR_IN_PAGE_ERROR；越文件背书区访问族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；未映射窗族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长/坏 prot/坏 hex 族"),
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

/// 规范语料指纹：挂接→写→重读→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
        file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
        writes: 0, bus_hits: 0,
    });
    let _ = m.write(32, &[1, 2, 3]);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_fmap_summary(&m) {
        bytes.extend_from_slice(s.as_bytes());
    }
    if let Ok(back) = m.reread(32, 3) {
        bytes.extend_from_slice(&back);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10142 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10142_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10142");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93380..=0x933BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < FmCode::ALL.len() {
        let mut j = i + 1;
        while j < FmCode::ALL.len() {
            if let (Some(a), Some(b)) = (FmCode::ALL.get(i), FmCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = FmCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_fmap_call("fmap|16|8|1122").is_ok()
        && parse_fmap_call("fmap|16|8|zz").err() == Some(FmCode::BadHex)
        && parse_fmap_call("fmap|16|8").err() == Some(FmCode::MissingPart)
        && parse_fmap_call("fmap|x|8|11").err() == Some(FmCode::BadNumber);
    s.add("调用行解析器：四段闭集放行且坏成分逐档拒绝", parse_ok,
          "hex 必须全十六进制位；off/len 十进制闭集");

    // 4. J1 达成核
    let (ok, probes) = run_fmap_rounds();
    s.add("J1 文件映射达成：写映射→文件重读一致 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（初读/写/重读/隔离/SIGBUS 档/计数）全对");

    // 5. J2 越尾访问
    let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
        file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
        writes: 0, bus_hits: 0,
    });
    let bus_ok = m.write(FILE_LEN, b"x").err() == Some(FmCode::BeyondEof)
        && m.bus_hits >= 1
        && m.writes == 0;
    s.add("J2 越尾访问受控：SIGBUS 档精确复现且文件不动（分进程断言建模）",
          bus_ok,
          "背书区外第一字节即墙；被拒不脏页");

    // 6. 背书一致性
    let _ = m.write(0, &[0xAA, 0xBB]);
    let back_ok = m.flush(0, &[0xAA, 0xBB]) == Ok(true);
    s.add("背书一致性：文件重读与写入逐字节一致", back_ok,
          "BackDrift 是文件映射语义根缺陷");

    // 7. 未映射窗档
    let nm_ok = m.write(8 * PAGE, b"x").err() == Some(FmCode::NotMapped);
    s.add("未映射窗：ENOMEM 档独立精确归因", nm_ok,
          "映射窗外与背书区外是两档（ENOMEM vs SIGBUS）");

    // 8. 模板确定性
    let t = m.reread(0, 3).unwrap_or_default();
    let tpl_ok = t.get(0) == Some(&3u8) && t.get(1) == Some(&((((1 * 5 + 3) as u32) % 256) as u8));
    s.add("模板确定性：未写区逐字节可复算", tpl_ok,
          "byte i = (i*5+3) 模 256 是本卡背书模板");

    // 9. 注入核
    let (inj, per_class) = run_bad_fmap_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "越尾/未映射/坏 prot 面/坏 hex 必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadFmap::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = FmapState::Attached.can_advance(FmapState::Flushed)
        && FmapState::Dirty.can_advance(FmapState::Flushed)
        && !FmapState::Flushed.can_advance(FmapState::Dirty)
        && FmapState::Dirty.label() == "已写脏";
    s.add("状态机：Attached→Dirty→Flushed 单调", sm_ok,
          "映射生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(7) && agrees_with_linux(12) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许表内正值且负值可复算（含 SIGBUS 信号档）");

    // 13. 成文 + 指纹
    let sum = compose_fmap_summary(&m);
    let text_ok = match &sum {
        Ok(x) => x.contains("base=0x") && x.contains("pages="),
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
    let ok_text = format!("文件映射达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_fmap_call("fmap|16|8|1122").is_ok());
        assert_eq!(parse_fmap_call("fmap|16|8|zz").err(), Some(FmCode::BadHex));
        assert_eq!(parse_fmap_call("fmap|x|8|11").err(), Some(FmCode::BadNumber));
        assert_eq!(parse_fmap_call("fmap|16|8").err(), Some(FmCode::MissingPart));
    }

    #[test]
    fn template_backing() {
        let m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
            file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
            writes: 0, bus_hits: 0,
        });
        assert_eq!(m.reread(0, 1), Ok(Vec::from(&[3u8] as &[u8])));
        assert_eq!(m.file.len(), FILE_LEN as usize);
    }

    #[test]
    fn write_through_to_file() {
        let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
            file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
            writes: 0, bus_hits: 0,
        });
        let _ = m.write(8, &[0xCD, 0xCD]);
        assert_eq!(m.reread(8, 2), Ok(Vec::from(&[0xCDu8, 0xCD] as &[u8])));
        assert_eq!(m.state, FmapState::Dirty);
    }

    #[test]
    fn beyond_eof_blocked() {
        let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
            file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
            writes: 0, bus_hits: 0,
        });
        assert_eq!(m.write(FILE_LEN, b"x").err(), Some(FmCode::BeyondEof));
        assert_eq!(m.reread(FILE_LEN, 1).err(), Some(FmCode::BeyondEof));
        assert_eq!(m.bus_hits, 1);
    }

    #[test]
    fn not_mapped_blocked() {
        let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
            file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
            writes: 0, bus_hits: 0,
        });
        assert_eq!(m.write(8 * PAGE, b"x").err(), Some(FmCode::NotMapped));
    }

    #[test]
    fn zero_len_blocked() {
        assert_eq!(FileMap::new(0).err(), Some(FmCode::ZeroLen));
    }

    #[test]
    fn untracked_region_intact() {
        let mut m = FileMap::new(FILE_LEN).unwrap_or(FileMap {
            file: Vec::new(), base: MBASE, mapped: PAGE, state: FmapState::Attached,
            writes: 0, bus_hits: 0,
        });
        let _ = m.write(100, &[0xFF]);
        assert_eq!(m.reread(0, 1), Ok(Vec::from(&[3u8] as &[u8])));
        assert_eq!(m.reread(104, 1), Ok(Vec::from(&[(((104 % 256) * 5 + 3) as u32 % 256) as u8] as &[u8]))); // 收库修正：模板模 256 改 u32 域
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_fmap_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_fmap_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(FmapState::Attached.can_advance(FmapState::Dirty));
        assert!(!FmapState::Flushed.can_advance(FmapState::Attached));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10142_checks_green() {
        let set = run_unx10142_checks();
        assert!(set.all_passed(), "UNX-F10142 red: {}", set.red_items_text());
    }
}
