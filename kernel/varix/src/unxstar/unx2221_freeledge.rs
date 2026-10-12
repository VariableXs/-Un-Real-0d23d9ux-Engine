//! unx2221_freeledger —— UNX-F2221 全域冻结总账落账（800 行总账 v1）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2221）：
//!   UNX-F2221 · 全域冻结总账落账（800 行总账 v1） ·
//!   判据：UNX-F2221-J1 全域冻结总账 v1 落账全过（800 行 =
//!   B01–B32 真指纹 620 行 + B33–F2400 段位占位 180 行，
//!   行数加法链 620+180=800 闭合，四列格式 v1 一致），
//!   总账哈希链延展闭合断言过
//! · 组件：四列格式 v1 + 双来源行（真指纹/段位占位）+ 加法链闭合 +
//!   哈希链延展 + 格式一致性对账
//!
//! ## 这份总账在冻结什么
//!
//! 内核里"冻结"不是形容词，是一条**可核对的边界**：在某个基线之后，
//! 任何对已冻结对象的改动都必须是一次**显式、有痕、可回溯**的动作。
//!
//! Windows 的做法是版本号 + 签名（比如 PE 的 checksum、驱动包的
//! WHQL 认证）。它做到的是"能看出变了"，但**看不出变了什么**——
//! 你只知道签名过了或没过，不知道是哪个字节、哪一行、哪一列。
//!
//! 内核侧要求更深：全域冻结总账必须能回答四个问题：
//!
//! 1. **有哪些行**（800 行，行数必须能加法闭合）；
//! 2. **每行长什么样**（四列格式 v1，不许有第五列、不许某行少列）；
//! 3. **每行的真值是什么**（真指纹行 vs 段位占位行——两者的
//!    "可信度"完全不同，混在一起就是账目造假）；
//! 4. **账本身有没有被改**（哈希链延展闭合——改一行而链没跟上，
//!    是最常见也最难发现的账目造假）。
//!
//! ## 为什么"行数加法链"必须闭合
//!
//! 判据要求 `620 + 180 = 800` 闭合。这看着像废话，其实是**最有效
//! 的一条反造假规则**：如果有人偷偷往总账里加了一行"占位行"来把
//! 某个段位蒙过去，总行数就会变成 801，加法链立刻破。反过来，如果
//! 有人删了一行真指纹行，总数变 799，也破。所以：
//!
//! **行数是账的"廉价摘要"**——它不需要重算任何哈希，就能发现
//! 绝大多数篡改。哈希链贵（要逐行重算），行数便宜且先跑。
//!
//! ## 为什么"两来源"必须显式区分
//!
//! 800 行分两类：
//!
//! - **真指纹行（620）**：行内容是真实对象的 FNV-1a64 指纹。改一个
//!   字节，指纹就变，链就断。
//! - **段位占位行（180）**：行内容不是指纹，而是"此段已被 X 占用"的
//!   声明。它的作用是**占位**——防止将来有人把同一段位分配给第二个
//!   模块。
//!
//! 这两者的验证强度完全不同：真指纹行改了能查出指纹不匹配；
//! 段位占位行改了（比如把 `0xD100 占用` 改成 `0xD200 占用`）指纹
//! 依然自洽——因为它本来就不含对象指纹。所以判据必须**显式区分**这两
//! 类，否则段位重排这种最危险的改动会从总账眼皮底下溜过去。
//!
//! 本模块的做法是：占位行带一个 `RowKind` 判别位，且段位冲突检测
//! 独立于哈希链——同一段位出现两条占位行，直接判红，不依赖哈希。

#![allow(clippy::too_many_arguments)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const F2221_VERSION: &str = "f2221-freeledger-v1";

/// 诊断段基址（本单独占）。
pub const F2221_SEG_BASE: u16 = 0xD200;

/// 格式版本（v1·四列）。
pub const LEDGER_VER: u32 = 1;

/// 四列（**恰好四列**·第五列即破坏格式）。
pub const COLS: usize = 4;

/// 真指纹行数（B01–B32·判据口径）。
pub const N_TRUE: usize = 620;

/// 段位占位行数（B33–F2400·判据口径）。
pub const N_SLOT: usize = 180;

/// 总行数（判据口径）。
pub const N_TOTAL: usize = N_TRUE + N_SLOT;

/// 域起点编号（B01）。
pub const B01: u32 = 1;

/// 域终点编号（B32·真指纹域）。
pub const B32: u32 = 32;

/// 段位域终点编号（F2400·占位域）。
pub const F2400: u32 = 2400;

/// 链初值（前值·非行内容）。
pub const CHAIN_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a64 素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// UNX-F2221 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum F2221Code {
    /// 0xD200 行数不等于 800（加法链破）。
    RowCountBad,
    /// 0xD201 加法链不闭合（真指纹行数 + 占位行数 ≠ 总行数）。
    AddChainBroken,
    /// 0xD202 列数不符（不是四列）。
    ColCount,
    /// 0xD203 格式版本不是 v1。
    VerNotV1,
    /// 0xD204 行种类越界（非真指纹也非占位）。
    RowKindBad,
    /// 0xD205 域编号越界（不在 B01..F2400）。
    DomainRange,
    /// 0xD206 段位重复（同段位两条占位行）。
    SegDup,
    /// 0xD207 哈希链断裂（重算链与账中链不等）。
    ChainBroken,
    /// 0xD208 行指纹与内容不符（真指纹行专有）。
    RowFpBad,
    /// 0xD209 占位行不得带内容指纹（占位行专有约束）。
    SlotHasFp,
    /// 0xD20A 行号不连续（重排/丢行）。
    RowSeqBroken,
    /// 0xD20B 格式串四列不一致（渲染与结构不符）。
    FormatDrift,
}

impl F2221Code {
    /// 码值。
    pub fn val(self) -> u16 {
        F2221_SEG_BASE
            + match self {
                F2221Code::RowCountBad => 0x0,
                F2221Code::AddChainBroken => 0x1,
                F2221Code::ColCount => 0x2,
                F2221Code::VerNotV1 => 0x3,
                F2221Code::RowKindBad => 0x4,
                F2221Code::DomainRange => 0x5,
                F2221Code::SegDup => 0x6,
                F2221Code::ChainBroken => 0x7,
                F2221Code::RowFpBad => 0x8,
                F2221Code::SlotHasFp => 0x9,
                F2221Code::RowSeqBroken => 0xA,
                F2221Code::FormatDrift => 0xB,
            }
    }

    /// 全部码（顺序即码序 · 12 码）。
    pub const ALL: [F2221Code; 12] = [
        F2221Code::RowCountBad,
        F2221Code::AddChainBroken,
        F2221Code::ColCount,
        F2221Code::VerNotV1,
        F2221Code::RowKindBad,
        F2221Code::DomainRange,
        F2221Code::SegDup,
        F2221Code::ChainBroken,
        F2221Code::RowFpBad,
        F2221Code::SlotHasFp,
        F2221Code::RowSeqBroken,
        F2221Code::FormatDrift,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            F2221Code::RowCountBad => "总行数≠800：先核 620+180 加法链，勿直接补行凑数",
            F2221Code::AddChainBroken => "加法链破：真指纹行与占位行之和≠总数，必有行被删或被增",
            F2221Code::ColCount => "列数非四：四列格式 v1 冻结，加列即毁下游解析",
            F2221Code::VerNotV1 => "格式版本非 v1：新版本须另起总账，不许在 v1 内改格式",
            F2221Code::RowKindBad => "行种类非法：只能是 TrueFp 或 SegSlot，无第三类",
            F2221Code::DomainRange => "域编号越界：须落在 B01..F2400，越界说明编号算错",
            F2221Code::SegDup => "段位重复：同一段位被两条占位行声明，先查分配冲突",
            F2221Code::ChainBroken => "哈希链断裂：改了行未重算延展链，重算链或回滚改动",
            F2221Code::RowFpBad => "行指纹不符：真指纹行内容与记录指纹不一致，对象已变",
            F2221Code::SlotHasFp => "占位行带内容指纹：占位行不得承载对象指纹，混填即造假",
            F2221Code::RowSeqBroken => "行号不连续：总账行号须 1..800 连续，丢行即破账",
            F2221Code::FormatDrift => "格式漂移：渲染四列与结构四列不一致，先查序列化模板",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

/// 段位守卫：全部诊断码必须落在本段 `[0xD200, 0xD20F]`。
pub fn segment_guard(c: u16) -> bool {
    c >= F2221_SEG_BASE && c < F2221_SEG_BASE + 0x10
}

// --------------------------------------------------------------------------
// 行种类
// --------------------------------------------------------------------------

/// 行种类（**两态闭集**·无第三类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// 真指纹行（承载对象 FNV-1a64 指纹）。
    TrueFp,
    /// 段位占位行（只声明段位占用·不承载指纹）。
    SegSlot,
}

impl RowKind {
    /// 序号。
    pub fn ord(self) -> u8 {
        match self {
            RowKind::TrueFp => 0,
            RowKind::SegSlot => 1,
        }
    }

    /// 标签（渲染与解析共用）。
    pub fn tag(self) -> &'static str {
        match self {
            RowKind::TrueFp => "T",
            RowKind::SegSlot => "S",
        }
    }

    /// 由序号反解（越界 `None`）。
    pub fn from_ord(v: u8) -> Option<RowKind> {
        match v {
            0 => Some(RowKind::TrueFp),
            1 => Some(RowKind::SegSlot),
            _ => None,
        }
    }
}

// --------------------------------------------------------------------------
// 四列行
// --------------------------------------------------------------------------

/// 总账行（**恰好四列**）。
///
/// 列序冻结为：`[序号, 域编号, 种类, 值]`。
/// - 真指纹行：`值` = 对象 FNV-1a64
/// - 占位行：`值` = 占用段位 << 16 | 占用者序号
///
/// 占位行**不**承载对象指纹——这是它与真指纹行的根本区别，
/// 也是 `SlotHasFp` 诊断码存在的理由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 第 1 列：行序号（1..=800 连续）。
    pub seq: u32,
    /// 第 2 列：域编号（B01..F2400）。
    pub domain: u32,
    /// 第 3 列：行种类（序 0/1）。
    pub kind: RowKind,
    /// 第 4 列：值（语义随种类而变）。
    pub value: u64,
    /// 该行的行指纹（对四列取FNV·不参与链）。
    pub fp: u64,
}

impl Row {
    /// 造真指纹行。
    pub fn true_row(seq: u32, domain: u32, obj_fp: u64) -> Row {
        let mut r = Row {
            seq,
            domain,
            kind: RowKind::TrueFp,
            value: obj_fp,
            fp: 0,
        };
        r.fp = r.compute_fp();
        r
    }

    /// 造占位行（`seg` 为占用段位·`owner` 为占用者序号）。
    pub fn slot_row(seq: u32, domain: u32, seg: u16, owner: u32) -> Row {
        let mut r = Row {
            seq,
            domain,
            kind: RowKind::SegSlot,
            value: ((seg as u64) << 16) | (owner as u64 & 0xFFFF),
            fp: 0,
        };
        r.fp = r.compute_fp();
        r
    }

    /// 四列值视图（序列化真源）。
    pub fn cols(&self) -> [u64; COLS] {
        [self.seq as u64, self.domain as u64, self.kind.ord() as u64, self.value]
    }

    /// 行指纹（对四列取 FNV-1a64）。
    pub fn compute_fp(&self) -> u64 {
        let mut h: u64 = CHAIN_SEED;
        for c in self.cols().iter() {
            h ^= *c;
            h = h.wrapping_mul(FNV_PRIME);
        }
        // 种类标签也入指纹：防止有人把 T 行改成 S 行而四列数值不变。
        h ^= self.kind.tag().as_bytes()[0] as u64;
        h.wrapping_mul(FNV_PRIME)
    }

    /// 指纹自检。
    pub fn fp_intact(&self) -> bool {
        self.fp == self.compute_fp()
    }

    /// 占位行解出段位。
    pub fn seg(&self) -> Option<u16> {
        if self.kind != RowKind::SegSlot {
            return None;
        }
        Some((self.value >> 16) as u16)
    }

    /// 占位行解出占用者。
    pub fn owner(&self) -> Option<u32> {
        if self.kind != RowKind::SegSlot {
            return None;
        }
        Some((self.value & 0xFFFF) as u32)
    }

    /// 真指纹行解出对象指纹。
    pub fn obj_fp(&self) -> Option<u64> {
        if self.kind != RowKind::TrueFp {
            return None;
        }
        Some(self.value)
    }

    /// 行序号是否合法（1..=800）。
    pub fn seq_ok(&self) -> bool {
        self.seq >= 1 && (self.seq as usize) <= N_TOTAL
    }

    /// 域编号是否合法（B01..F2400）。
    pub fn domain_ok(&self) -> bool {
        self.domain >= B01 && self.domain <= F2400
    }

    /// 占位行不得承载对象指纹（`value` 高 16 位即段位·真指纹行才用满64 位）。
    ///
    /// 这条约束的实质：占位行若被写成"看起来像真指纹"的值，段位信息
    /// 就丢了，而段位信息恰恰是占位行的**全部价值**。
    pub fn slot_payload_ok(&self) -> bool {
        match self.kind {
            RowKind::SegSlot => self.seg().is_some() && self.seg() != Some(0),
            RowKind::TrueFp => true,
        }
    }
}

// --------------------------------------------------------------------------
// 四列格式 v1（序列化 · 解析 · 一致性）
// --------------------------------------------------------------------------

/// 四列格式 v1 序列化：`seq|domain|kind|value`，`|`分隔。
///
/// 选 `|` 而不是空格或逗号的原因与 F2101 同源：分隔符必须**不可能**
/// 出现在任何列的值里。`|` 是 ASCII 124，不在任何十进制数字与十六进制
/// 字符集中，所以不需要转义，也就不存在"转义规则被绕过"的问题。
pub fn render_row_v1(r: &Row) -> String {
    let mut s = String::new();
    s.push_str(&format!("{}", r.seq));
    s.push('|');
    s.push_str(&format!("{}", r.domain));
    s.push('|');
    s.push_str(r.kind.tag());
    s.push('|');
    s.push_str(&format!("0x{:016X}", r.value));
    s
}

/// 四列格式 v1 解析（严格·列数与种类都必须合法）。
pub fn parse_row_v1(line: &[u8]) -> Result<Row, F2221Code> {
    // 按 `|` 切分，须恰好 COLS 段。
    let mut parts: [Option<&[u8]>; COLS] = [None; COLS];
    let mut cnt = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= line.len() {
        if i == line.len() || line[i] == b'|' {
            if cnt >= COLS {
                // 第5 段——四列格式破。
                return Err(F2221Code::ColCount);
            }
            parts[cnt] = Some(&line[start..i]);
            cnt += 1;
            start = i + 1;
        }
        i += 1;
    }
    if cnt != COLS {
        return Err(F2221Code::ColCount);
    }
    let seq = match parse_dec(parts[0].unwrap_or(b"")) {
        Some(v) => v as u32,
        None => return Err(F2221Code::FormatDrift),
    };
    let domain = match parse_dec(parts[1].unwrap_or(b"")) {
        Some(v) => v as u32,
        None => return Err(F2221Code::FormatDrift),
    };
    let tag = parts[2].unwrap_or(b"");
    let kind = match tag {
        b"T" => RowKind::TrueFp,
        b"S" => RowKind::SegSlot,
        _ => return Err(F2221Code::RowKindBad),
    };
    let value = match parse_hex(parts[3].unwrap_or(b"")) {
        Some(v) => v,
        None => return Err(F2221Code::FormatDrift),
    };
    let mut r = Row {
        seq,
        domain,
        kind,
        value,
        fp: 0,
    };
    r.fp = r.compute_fp();
    Ok(r)
}

/// 无依赖十进制解析。
fn parse_dec(b: &[u8]) -> Option<u64> {
    if b.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for c in b.iter() {
        if !(b'0'..=b'9').contains(c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((*c - b'0') as u64)?;
    }
    Some(v)
}

/// 无依赖十六进制解析（须带 `0x` 前缀·两字符一字节）。
fn parse_hex(b: &[u8]) -> Option<u64> {
    if b.len() < 3 || b[0] != b'0' || (b[1] != b'x' && b[1] != b'X') {
        return None;
    }
    let body = &b[2..];
    if body.is_empty() || body.len() > 16 {
        return None;
    }
    let mut v: u64 = 0;
    for c in body.iter() {
        let d = match c {
            b'0'..=b'9' => *c - b'0',
            b'a'..=b'f' => *c - b'a' + 10,
            b'A'..=b'F' => *c - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | (d as u64);
    }
    Some(v)
}

// --------------------------------------------------------------------------
// 哈希链延展
// --------------------------------------------------------------------------

/// 链步（`h = (h ^ row_fp) * PRIME`·延展式）。
///
/// 为什么不直接 `h ^ row_fp`：延展式（乘素数）让链对**顺序**敏感——
/// 两行内容交换，链值必变。若用纯 XOR，两行交换后链值不变，账目
/// 就可以在"看起来没动"的情况下被重排。冻结总账必须抗重排。
pub fn chain_step(h: u64, row_fp: u64) -> u64 {
    (h ^ row_fp).wrapping_mul(FNV_PRIME)
}

/// 全账哈希链（自seed 起逐行延展）。
pub fn chain_of(rows: &[Row]) -> u64 {
    let mut h = CHAIN_SEED;
    for r in rows.iter() {
        h = chain_step(h, r.compute_fp());
    }
    h
}

// --------------------------------------------------------------------------
// 总账
// --------------------------------------------------------------------------

/// 全域冻结总账（v1）。
#[derive(Clone, Debug)]
pub struct Ledger {
    rows: Vec<Row>,
    /// 账记的链值（落账时写入）。
    pub chain: u64,
    /// 格式版本。
    pub ver: u32,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
}

impl Ledger {
    /// 造空账（v1）。
    pub fn new() -> Ledger {
        Ledger {
            rows: Vec::new(),
            chain: CHAIN_SEED,
            ver: LEDGER_VER,
            stats: [0; 12],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: F2221Code) {
        let i = (c.val() - F2221_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: F2221Code) -> u64 {
        let i = (c.val() - F2221_SEG_BASE) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 追加一行（序号必须等于当前行数+1·即强制连续）。
    pub fn push(&mut self, r: Row) -> Result<usize, F2221Code> {
        let want = self.rows.len() as u32 + 1;
        if r.seq != want {
            self.bump(F2221Code::RowSeqBroken);
            return Err(F2221Code::RowSeqBroken);
        }
        if !r.domain_ok() {
            self.bump(F2221Code::DomainRange);
            return Err(F2221Code::DomainRange);
        }
        if !r.slot_payload_ok() {
            self.bump(F2221Code::SlotHasFp);
            return Err(F2221Code::SlotHasFp);
        }
        self.rows.push(r);
        Ok(self.rows.len() - 1)
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 只读行视图。
    pub fn rows_ref(&self) -> &[Row] {
        &self.rows
    }

    /// 落账（重算链并写入）。
    pub fn settle(&mut self) -> u64 {
        self.chain = chain_of(&self.rows);
        self.chain
    }

    /// 加法链闭合（`真指纹行数 + 占位行数 == 总行数`）。
    ///
    /// 这条恒等式在数学上恒真（按 kind 分类计数必然加起来等于总数），
    /// 所以它的价值不在"验算"，而在**给篡改一个最廉价的检测点**：
    /// 有人删了一行 T 行又补了一行 S 行，总数不变、种类计数也不变——
    /// 但段位冲突检测会红。两条规则互补，缺一就有盲区。
    pub fn add_chain_closed(&self) -> bool {
        let t = self.count(RowKind::TrueFp);
        let s = self.count(RowKind::SegSlot);
        t + s == self.rows.len() && t + s == N_TOTAL
    }

    /// 按种类计数。
    pub fn count(&self, k: RowKind) -> usize {
        self.rows.iter().filter(|r| r.kind == k).count()
    }

    /// 段位冲突检测（**独立于哈希链**）。
    ///
    /// 为什么必须独立：占位行的 `value` 就是段位与占用者，改一个段位
    /// 后重算链是自洽的，哈希链查不出"段位被重排"这种改动。只有把它
    /// 单独拎出来比，才查得出。
    pub fn seg_dup(&self) -> Vec<u16> {
        let mut segs: Vec<u16> = Vec::new();
        let mut dups: Vec<u16> = Vec::new();
        for r in self.rows.iter() {
            if r.kind != RowKind::SegSlot {
                continue;
            }
            if let Some(sg) = r.seg() {
                if segs.contains(&sg) {
                    if !dups.contains(&sg) {
                        dups.push(sg);
                    }
                } else {
                    segs.push(sg);
                }
            }
        }
        dups
    }

    /// 行序号连续性。
    pub fn seq_continuous(&self) -> bool {
        for (i, r) in self.rows.iter().enumerate() {
            if r.seq as usize != i + 1 {
                return false;
            }
        }
        true
    }

    /// 链闭合（重算 == 账记）。
    pub fn chain_closed(&self) -> bool {
        self.chain == chain_of(&self.rows)
    }

    /// 格式一致性（渲染→解析→重渲染逐行相同）。
    ///
    /// 这条查的是"序列化模板与结构脱节"：比如某行渲染时少写一列，
    /// 肉眼看上去账还是 800 行，但解析回来变成 799 行或列错位。
    pub fn format_consistent(&mut self) -> bool {
        let mut errs: Vec<F2221Code> = Vec::new();
        for r in self.rows.iter() {
            let line = render_row_v1(r);
            match parse_row_v1(line.as_bytes()) {
                Ok(back) => {
                    if back.cols() != r.cols() || back.kind != r.kind {
                        errs.push(F2221Code::FormatDrift);
                    }
                }
                Err(e) => errs.push(e),
            }
        }
        for e in errs.iter() {
            self.bump(*e);
        }
        errs.is_empty()
    }

    /// 全量落账断言（判据 J1 主入口）。
    ///
    /// 逐条对账，任何一条不成立即整体拒。**不设部分通过**——
    /// 冻结总账的价值恰恰在于它要么全对要么全错。
    pub fn assert_all(&mut self) -> bool {
        let mut ok = true;
        if self.ver != LEDGER_VER {
            self.bump(F2221Code::VerNotV1);
            ok = false;
        }
        if self.rows.len() != N_TOTAL {
            self.bump(F2221Code::RowCountBad);
            ok = false;
        }
        if !self.add_chain_closed() {
            self.bump(F2221Code::AddChainBroken);
            ok = false;
        }
        if !self.seq_continuous() {
            self.bump(F2221Code::RowSeqBroken);
            ok = false;
        }
        let dups = self.seg_dup();
        if !dups.is_empty() {
            self.bump(F2221Code::SegDup);
            ok = false;
        }
        for r in self.rows.iter() {
            if !r.fp_intact() {
                self.bump(F2221Code::RowFpBad);
                ok = false;
                break;
            }
        }
        for r in self.rows.iter() {
            if !r.domain_ok() {
                self.bump(F2221Code::DomainRange);
                ok = false;
                break;
            }
            if !r.slot_payload_ok() {
                self.bump(F2221Code::SlotHasFp);
                ok = false;
                break;
            }
        }
        if !self.chain_closed() {
            self.bump(F2221Code::ChainBroken);
            ok = false;
        }
        if !self.format_consistent() {
            ok = false;
        }
        ok
    }

    /// 渲染（看板真源·零浮点）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "freeze-ledger v{} rows={} true={} slot={} add_closed={} seg=0x{:04X}\n",
            self.ver,
            self.rows.len(),
            self.count(RowKind::TrueFp),
            self.count(RowKind::SegSlot),
            self.add_chain_closed(),
            F2221_SEG_BASE
        ));
        s.push_str(&format!(
            "chain seed=0x{:016X} settled=0x{:016X} closed={} dups={}\n",
            CHAIN_SEED,
            self.chain,
            self.chain_closed(),
            self.seg_dup().len()
        ));
        s.push_str("seq|domain|kind|value\n");
        // 只渲染头尾各三行——800 行全渲染无助于判断，且看板要给人看。
        let n = self.rows.len();
        for i in 0..n {
            if i >= 3 && i + 3 < n {
                if i == 3 {
                    s.push_str("...\n");
                }
                continue;
            }
            match self.rows.get(i) {
                Some(r) => {
                    s.push_str(&render_row_v1(r));
                    s.push('\n');
                }
                None => {}
            }
        }
        s
    }
}

// --------------------------------------------------------------------------
// 判据（12 条）
// --------------------------------------------------------------------------

/// 造一条满账（620 T + 180 S·确定性派生·零墙钟）。
fn build_full(seed: u64) -> Ledger {
    let mut l = Ledger::new();
    // 真指纹行：620 行，域号 B01..B32 循环，指纹由 seed+seq 派生。
    for i in 0..N_TRUE {
        let seq = (i + 1) as u32;
        let dom = B01 + (i as u32) % (B32 - B01 + 1);
        let fp = (seed
            .wrapping_add(i as u64)
            .wrapping_mul(FNV_PRIME))
            ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let _ = l.push(Row::true_row(seq, dom, fp));
    }
    // 占位行：180 行，域号 F2400 递减，段位每行一个。
    for i in 0..N_SLOT {
        let seq = (N_TRUE + i + 1) as u32;
        let dom = F2400 - (i as u32);
        // 段位基址 + 步进。**先乘后加**：反过来写`(base+i)*step`
        // 会在 base*step 溢出 u16 时把整段段位折叠成 0，
        // 而"段位为 0"会被 slot_payload_ok 当作非法载荷拒掉——
        // 表现为占位行静默全丢，总数变成 620。
        let seg = 0x2000u16.wrapping_add((i as u16).wrapping_mul(0x10));
        let _ = l.push(Row::slot_row(seq, dom, seg, i as u32));
    }
    l.settle();
    l
}

/// UNX-F2221 判据全集（12 条）。
pub fn run_f2221_checks() -> CheckSet {
    let mut cs = CheckSet::new("f2221");

    // ---- F2221-01 诊断段闭集与建议完备 ----
    let seg_ok = F2221Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == F2221_SEG_BASE + i as u16)
        && F2221Code::ALL.iter().all(|c| segment_guard(c.val()));
    let remedy_ok = F2221Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("F2221-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续落段+建议完备");

    // ---- F2221-02 满账 800 行 + 加法链闭合 ----
    let l2 = build_full(0x1234_5678);
    cs.add(
        "F2221-02 加法链闭合",
        l2.len() == N_TOTAL
            && l2.count(RowKind::TrueFp) == N_TRUE
            && l2.count(RowKind::SegSlot) == N_SLOT
            && l2.add_chain_closed(),
        "620+180=800 恒等式成立",
    );

    // ---- F2221-03 四列格式 v1 序列化/解析往返 ----
    let l3 = build_full(0xAAAA_5555);
    let r3 = l3.rows_ref()[0];
    let line3 = render_row_v1(&r3);
    let back3 = parse_row_v1(line3.as_bytes());
    let round_ok = match back3 {
        Ok(b) => b.cols() == r3.cols() && b.kind == r3.kind,
        Err(_) => false,
    };
    cs.add(
        "F2221-03 四列往返",
        round_ok && line3.as_bytes().iter().filter(|c| **c == b'|').count() == COLS - 1,
        "序列化→解析四列全等且分隔符恰3 个",
    );

    // ---- F2221-04 五列/少列拒 ----
    let e_more = parse_row_v1(b"1|1|T|0x1|extra");
    let e_less = parse_row_v1(b"1|1|T");
    let e_nonkind = parse_row_v1(b"1|1|X|0x1");
    cs.add(
        "F2221-04 列数守卫",
        e_more == Err(F2221Code::ColCount)
            && e_less == Err(F2221Code::ColCount)
            && e_nonkind == Err(F2221Code::RowKindBad),
        "五列/少列拒·未知种类拒",
    );

    // ---- F2221-05 行序号强制连续 ----
    let mut l5 = Ledger::new();
    let ok5 = l5.push(Row::true_row(1, B01, 0xAA)).is_ok();
    let bad5 = l5.push(Row::true_row(5, B01, 0xBB));
    let bad6 = l5.push(Row::true_row(1, B01, 0xCC));
    cs.add(
        "F2221-05 序号连续",
        ok5 && bad5 == Err(F2221Code::RowSeqBroken) && bad6 == Err(F2221Code::RowSeqBroken)
            && l5.len() == 1,
        "跳号与重号均拒",
    );

    // ---- F2221-06 域编号越界拒 ----
    let mut l6 = Ledger::new();
    let a6 = l6.push(Row::true_row(1, 0, 1));
    let b6 = l6.push(Row::true_row(1, F2400 + 1, 1));
    let c6 = l6.push(Row::true_row(1, B01, 1));
    cs.add(
        "F2221-06 域号边界",
        a6 == Err(F2221Code::DomainRange)
            && b6 == Err(F2221Code::DomainRange)
            && c6.is_ok()
            && l6.len() == 1,
        "B01..F2400 开闭区间正确",
    );

    // ---- F2221-07 段位冲突独立检出（改段位重算链也查得出）----
    // 关键：两行占位同段位，但**重算链后链是完全自洽的**——
    // 唯独段位冲突检测能红。这正是"段位检测必须独立于哈希链"的理由。
    let mut l7e = Ledger::new();
    let _ = l7e.push(Row::slot_row(1, B01, 0x7700, 1));
    let _ = l7e.push(Row::slot_row(2, B01, 0x7700, 2));
    l7e.settle();
    let dups = l7e.seg_dup();
    cs.add(
        "F2221-07 段位冲突",
        dups.len() == 1 && dups[0] == 0x7700 && l7e.chain_closed(),
        "同段位两行即便链自洽也被检出",
    );

    // ---- F2221-08 行指纹对内容改写敏感 ----
    // 注意断言方向：改内容后 `fp` **字段本身不变**（没人重算它），
    // 破的是"fp 与内容不再相符"，即 `fp != compute_fp()`。
    // 断言 `fp_a != fp_b` 是错的——那只说明没人重算，不能说明查得出。
    let good = Row::true_row(7, B01, 0x1234_5678_9ABC_DEF0);
    let mut tampered = good;
    tampered.value ^= 1;
    let mut rehashed = tampered;
    rehashed.fp = rehashed.compute_fp();
    cs.add(
        "F2221-08 行指纹",
        good.fp_intact()
            && !tampered.fp_intact()
            && tampered.fp == good.fp
            && rehashed.fp_intact()
            && rehashed.fp != good.fp,
        "改一位即破行指纹·重算后才自洽",
    );

    // ---- F2221-09 占位行不得承载对象指纹 ----
    // 段位存在 value 的**高 16 位**。所以"段位为 0"的值必须
    // 高 16 位全零——即 `value < 0x1_0000`。拿一个高位非零的值
    // 当反例是无效的：它本来就是合法占位行。
    let bad_slot = Row {
        seq: 1,
        domain: B01,
        kind: RowKind::SegSlot,
        value: 0xFFFF, // 段位 = 0 ·占用者 = 0xFFFF
        fp: 0,
    };
    let mut bad_slot2 = bad_slot;
    bad_slot2.fp = bad_slot2.compute_fp();
    let mut l9 = Ledger::new();
    let e9 = l9.push(bad_slot2);
    // 合法占位行：段位非 0。
    let ok9 = Row::slot_row(1, B01, 0x6600, 1).slot_payload_ok();
    cs.add(
        "F2221-09 占位载荷",
        e9 == Err(F2221Code::SlotHasFp) && ok9,
        "段位为 0 的占位行拒·真占位行收",
    );

    // ---- F2221-10 链对抗重排（顺序敏感） ----
    let a = Row::true_row(1, B01, 0x11);
    let b = Row::true_row(2, B02_, 0x22);
    let ch1 = {
        let mut h = CHAIN_SEED;
        h = chain_step(h, a.compute_fp());
        chain_step(h, b.compute_fp())
    };
    let ch2 = {
        let mut h = CHAIN_SEED;
        h = chain_step(h, b.compute_fp());
        chain_step(h, a.compute_fp())
    };
    cs.add(
        "F2221-10 链抗重排",
        ch1 != ch2 && ch1 != CHAIN_SEED,
        "两行交换后链值必变（延展式非纯 XOR）",
    );

    // ---- F2221-11 全量落账断言（800 行满账全过） ----
    let mut l11 = build_full(0x0F0F_0F0F);
    let all_ok = l11.assert_all();
    cs.add(
        "F2221-11 满账全过",
        all_ok
            && l11.len() == N_TOTAL
            && l11.chain_closed()
            && l11.format_consistent()
            && l11.seg_dup().is_empty()
            && l11.seq_continuous(),
        "800 行满账逐条对账全过",
    );

    // ---- F2221-12 篡改必破（改行不重算链） ----
    //三段递进，每段都比上一段更强：
    //  ①原账自洽；
    //  ② 改首行 value 但**不重算行 fp** →链断 + 行指纹破（双破）；
    //  ③ 再把行 fp 也重算上→ 链与行指纹都"看起来好了"，
    //     但**账记的链值**（settle 时的旧值）仍未更新，
    //     所以 chain_closed() 依然为假。
    //  ③ 才是关键：它证明"重算所有局部指纹"也逃不掉，
    //  因为落账时的链快照本身就是一道独立的闸。
    let mut l12 = build_full(0xDEAD_BEEF);
    l12.settle();
    let chain_before = l12.chain;
    let intact_ok = l12.chain_closed() && l12.assert_all();

    let orig = l12.rows_ref()[0];
    // ② 只改内容
    let mut tampered = orig;
    tampered.value ^= 0xFF;
    let mut rows_bad = l12.rows_ref().to_vec();
    rows_bad[0] = tampered;
    let chain_bad = chain_of(&rows_bad);
    let broke = chain_bad != chain_before && !tampered.fp_intact();
    // 两种"修法"给出两条不同的坏链值（比较放在 ③ 之后）：
    // 不重算行 fp 得 chain_bad，重算了得 chain_fixed，两者都≠落账值。

    // ③ 连行 fp 一并重算
    let mut fixed = tampered;
    fixed.fp = fixed.compute_fp();
    let mut rows_fixed = l12.rows_ref().to_vec();
    rows_fixed[0] = fixed;
    let chain_fixed = chain_of(&rows_fixed);
    // 链只吃compute_fp() 的重算值，不吃行里存的 fp——所以"把行 fp 重算一遍"
    // 对链毫无帮助：chain_bad 与 chain_fixed 必然相等，且同样≠落账值。
    let chain_fp_immune = chain_bad == chain_fixed && chain_fixed != chain_before;
    let locally_consistent = fixed.fp_intact();
    let still_open = chain_fixed != chain_before;

    // 把它落进一个"链快照未更新"的账：行集换成重算过fp 的版本，
    // 但 `chain` 字段仍是篡改前的落账值——正是"局部全修好、快照没修"
    // 的真实场景。走 assert_all 必须报破。
    let mut l12b = l12.clone();
    l12b.rows = rows_fixed.clone();
    let verdict = l12b.assert_all();
    let stat_broken = l12b.stat(F2221Code::ChainBroken);

    cs.add(
        "F2221-12 篡改必破",
        intact_ok
            && broke
            && chain_fp_immune
            && locally_consistent
            && still_open
            && !verdict
            && stat_broken >= 1,
        "原账自洽→改值双破→重算局部指纹仍逃不掉落账快照闸",
    );

    cs
}

/// 便捷常量（B02·避免判据里散落字面量）。
const B02_: u32 = 2;

/// UNX-F2221 锚点串（跨批引用防漂移）。
#[must_use]
pub fn f2221_anchor() -> &'static str {
    F2221_VERSION
}