//! UNX-F10081 E 型开卷总纲（双判据哲学与 M→E 换轨） · 0x92400 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10081（E 型开卷第一卡：把双判据哲学写成可机读协议）。
//! 锚点：UNX-F10082-J1 结构协议冻结（J1/J2 字段形态三成分各齐），
//!       缺成分 10/10 次拒收。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92400（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `CharterCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `parse_field`：E 型判据字段协议 "J1|主体|路径" 三成分逐段
//!     解析（tag/subject/path），任一成分缺失即拒收，逐次留痕；
//!   - 注册表 `CharterRegistry`：解析过的字段逐条登记（J1 正/J2 反
//!     双列），重复 tag 对 / 越界登记显性拒绝；
//!   - 状态机 `CharterState`：Open → Structured → Frozen 单向推进，
//!     冻结后字段只读；
//!   - 换轨账 `TrackLedger`：M→E 换轨条目逐条登记，M/E 双面
//!     数量守恒（每条 E 型条目必须挂一条 M 型来源）；
//!   - 注入核 `BadFieldCase`：缺 tag / 缺主体 / 缺路径 / 非法 tag /
//!     重复登记 五类坏样本逐类 10/10 次拒收；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10081 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92400;

/// 段窗上界（下一卡 0x92440 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92440;

/// 协议字段三成分数（tag / subject / path）。
pub const FIELD_COMPS: usize = 3;

/// 协议槽位数（J1 正 + J2 反 双列各 4 条示例，共 8 槽）。
pub const FIELD_SLOTS: usize = 8;

/// 换轨账 M/E 双面配对基数（每面 4 条）。
pub const TRACK_PAIRS: usize = 4;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EACCES: i32 = 13;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCode {
    /// 缺 tag 成分（字段首段非 J1/J2）
    MissingTag,
    /// 缺主体成分（字段中段为空）
    MissingSubject,
    /// 缺路径成分（字段尾段为空）
    MissingPath,
    /// 非法 tag（三成分齐全但 tag 出 J1/J2 闭集）
    IllegalTag,
    /// 分段不足（段数 < 3）
    TooFewSegs,
    /// 重复登记同 tag 对
    DuplicatePair,
    /// 槽位越界（登记下标出 0..8 窗）
    SlotRange,
    /// 换轨账缺来源（E 型条目无 M 型来源锚）
    TrackOrphan,
    /// 冻结后改字段
    FrozenMutate,
    /// 状态倒退
    StateRegress,
    /// 协议未冻结即用
    NotFrozen,
    /// 段位越界
    SegmentViolation,
}

impl CharterCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CharterCode::MissingTag => "缺 tag 成分被拒：字段首段必须是 J1 或 J2；请补判据位",
            CharterCode::MissingSubject => "缺主体成分被拒：字段中段为空；请补主体子句",
            CharterCode::MissingPath => "缺路径成分被拒：字段尾段为空；请补达成/复现路径",
            CharterCode::IllegalTag => "非法 tag 被拒：判据位只允许 J1/J2；请改写档位",
            CharterCode::TooFewSegs => "分段不足被拒：协议行必须恰为 tag|主体|路径 三段；请补齐",
            CharterCode::DuplicatePair => "重复登记被拒：同 tag 对已存在；请先核查再改",
            CharterCode::SlotRange => "槽位越界被拒：登记下标只允许 0..8；请核对槽号",
            CharterCode::TrackOrphan => "换轨缺来源被拒：E 型条目必须挂 M 型来源锚；请补来源",
            CharterCode::FrozenMutate => "冻结后改字段被拒：协议已冻结只读；请走修订通道",
            CharterCode::StateRegress => "状态倒退被拒：Open→Structured→Frozen 单向；请排查回滚",
            CharterCode::NotFrozen => "未冻结即用被拒：结构协议必须先冻结再消费；请先冻结",
            CharterCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92400..=0x92440",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CharterCode; 12] = [
        CharterCode::MissingTag,
        CharterCode::MissingSubject,
        CharterCode::MissingPath,
        CharterCode::IllegalTag,
        CharterCode::TooFewSegs,
        CharterCode::DuplicatePair,
        CharterCode::SlotRange,
        CharterCode::TrackOrphan,
        CharterCode::FrozenMutate,
        CharterCode::StateRegress,
        CharterCode::NotFrozen,
        CharterCode::SegmentViolation,
    ];
}

// ── 字段协议解析器 ────────────────────────────────────────────────────
/// 判据位闭集：J1 正判据 / J2 反判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldTag {
    /// J1 正判据（达成路径可复测）
    J1,
    /// J2 反判据（误用路径可复现）
    J2,
}

/// 一个协议字段：tag + 主体 + 路径（三成分各齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtocolField {
    /// 判据位
    pub tag: FieldTag,
    /// 主体子句（谁被验收，UTF-8 定容 24 字节）
    pub subject: [u8; 24],
    /// 主体字节数
    pub subject_len: usize,
    /// 路径字面量（达成/复现路径，谁复测）
    pub path: [u8; 32],
    /// 路径字节数
    pub path_len: usize,
}

/// 分段器：按 '|' 切分协议行（无堆复制，返回段字节区间）。
fn split_segs(line: &str) -> Option<[usize; 4]> {
    // 返回 [seg0 起, seg1 起, seg2 起, 总长]，找不到恰 3 段返回 None
    let b = line.as_bytes();
    let mut cuts = [0usize; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 2 {
                return None; // 超过 2 个分隔符 → 段数 > 3，协议不允许
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 2 {
        return None;
    }
    cuts[3] = b.len();
    Some(cuts)
}

/// 拷贝字面量到定容缓冲（超容截断拒绝，显性返回 None）。
fn copy_lit(dst: &mut [u8], src: &str) -> Option<usize> {
    let s = src.as_bytes();
    if s.is_empty() || s.len() > dst.len() {
        return None;
    }
    let mut i = 0usize;
    while i < s.len() {
        if let Some(slot) = dst.get_mut(i) {
            if let Some(v) = s.get(i) {
                *slot = *v;
            }
        }
        i += 1;
    }
    Some(s.len())
}

/// 协议行解析器："J1|主体|路径" → ProtocolField。
///
/// 三成分逐段核：段数≠3 / 缺 tag / 缺主体 / 缺路径 / tag 出闭集
/// 五类错误逐一显性返回，缺成分 10/10 次拒收。
pub fn parse_field(line: &str) -> Result<ProtocolField, CharterCode> {
    let cuts = match split_segs(line) {
        Some(c) => c,
        None => return Err(CharterCode::TooFewSegs),
    };
    let tag_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let subj_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let path_seg = line.get(cuts[2]..cuts[3]).unwrap_or("");
    let tag = match tag_seg {
        "J1" => FieldTag::J1,
        "J2" => FieldTag::J2,
        "" => return Err(CharterCode::MissingTag),
        _ => return Err(CharterCode::IllegalTag),
    };
    if subj_seg.is_empty() {
        return Err(CharterCode::MissingSubject);
    }
    if path_seg.is_empty() {
        return Err(CharterCode::MissingPath);
    }
    let mut f = ProtocolField {
        tag,
        subject: [0u8; 24],
        subject_len: 0,
        path: [0u8; 32],
        path_len: 0,
    };
    f.subject_len = match copy_lit(&mut f.subject, subj_seg) {
        Some(n) => n,
        None => return Err(CharterCode::MissingSubject),
    };
    f.path_len = match copy_lit(&mut f.path, path_seg) {
        Some(n) => n,
        None => return Err(CharterCode::MissingPath),
    };
    Ok(f)
}

// ── 协议注册表与状态机 ────────────────────────────────────────────────
/// 状态机：Open → Structured → Frozen 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterState {
    /// 开卷（解析进行时）
    Open,
    /// 已结构化（双列槽位齐）
    Structured,
    /// 已冻结（结构协议封版）
    Frozen,
}

/// 协议注册表：8 槽字段（J1×4 + J2×4）+ 状态机。
#[derive(Clone, Debug)]
pub struct CharterRegistry {
    /// 已登记字段（None=空槽）
    pub fields: [Option<ProtocolField>; FIELD_SLOTS],
    /// 当前状态
    pub state: CharterState,
}

impl CharterRegistry {
    /// 构造空注册表。
    pub const fn new() -> CharterRegistry {
        CharterRegistry { fields: [None; FIELD_SLOTS], state: CharterState::Open }
    }

    /// 登记一个字段：槽号必检、重复必拒、冻结后必拒。
    /// 重复判定：同 tag 对（J1 与 J2 各允许 4 条，同 tag 超第 4 条拒）。
    pub fn register(&mut self, slot: usize, line: &str) -> Result<usize, CharterCode> {
        if self.state != CharterState::Open {
            return Err(CharterCode::FrozenMutate);
        }
        if slot >= FIELD_SLOTS {
            return Err(CharterCode::SlotRange);
        }
        let f = parse_field(line)?;
        // 重复检查：已有同 tag 的字段即视为重复对（每个 tag 槽位独立占位）
        let mut i = 0usize;
        while i < FIELD_SLOTS {
            if i != slot {
                if let Some(e) = self.fields.get(i) {
                    if let Some(ex) = e {
                        if ex.tag == f.tag && ex.subject == f.subject && ex.path == f.path {
                            return Err(CharterCode::DuplicatePair);
                        }
                    }
                }
            }
            i += 1;
        }
        if let Some(s) = self.fields.get_mut(slot) {
            *s = Some(f);
        }
        Ok(slot)
    }

    /// 结构化：8 槽齐且 J1/J2 各 4 才可推进。
    pub fn structure(&mut self) -> Result<(), CharterCode> {
        if self.state != CharterState::Open {
            return Err(CharterCode::StateRegress);
        }
        let mut j1 = 0usize;
        let mut j2 = 0usize;
        let mut i = 0usize;
        while i < FIELD_SLOTS {
            match self.fields.get(i).copied().unwrap_or(None) {
                Some(f) => match f.tag {
                    FieldTag::J1 => j1 += 1,
                    FieldTag::J2 => j2 += 1,
                },
                None => return Err(CharterCode::TooFewSegs),
            }
            i += 1;
        }
        if j1 != TRACK_PAIRS || j2 != TRACK_PAIRS {
            return Err(CharterCode::TooFewSegs);
        }
        self.state = CharterState::Structured;
        Ok(())
    }

    /// 冻结：结构化后才可冻结；冻结后字段只读。
    pub fn freeze(&mut self) -> Result<(), CharterCode> {
        if self.state != CharterState::Structured {
            return Err(CharterCode::NotFrozen);
        }
        self.state = CharterState::Frozen;
        Ok(())
    }
}

/// 标准开卷：J1×4（读/写/关/映射达成）+ J2×4（负 fd/缺路径/重关闭/零长）。
pub fn register_standard(reg: &mut CharterRegistry) -> Result<(), CharterCode> {
    let lines = [
        ("J1|fd 读路径|LTP read01 复测", 0usize),
        ("J1|fd 写路径|LTP write01 复测", 1usize),
        ("J1|fd 关闭路径|LTP close01 复测", 2usize),
        ("J1|匿名映射路径|LTP mmap01 复测", 3usize),
        ("J2|负 fd 误用|预期 EBADF 复现", 4usize),
        ("J2|缺失路径误用|预期 ENOENT 复现", 5usize),
        ("J2|重关闭误用|预期 EBADF 复现", 6usize),
        ("J2|零长映射误用|预期 EINVAL 复现", 7usize),
    ];
    let mut i = 0usize;
    while i < lines.len() {
        if let Some((line, slot)) = lines.get(i) {
            reg.register(*slot, line)?;
        }
        i += 1;
    }
    Ok(())
}

// ── 换轨账 ────────────────────────────────────────────────────────────
/// 一条 M→E 换轨条目：E 型判据 + M 型来源锚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrackEntry {
    /// E 型判据槽号
    pub e_slot: usize,
    /// M 型来源锚（非零即有源）
    pub m_source: u32,
}

/// 换轨账：每条 E 型条目必须挂 M 型来源，孤儿条目逐条检出。
#[derive(Clone, Debug)]
pub struct TrackLedger {
    /// 已登记换轨条目
    pub entries: Vec<TrackEntry>,
    /// 孤儿条目数（无 M 来源，必须为 0）
    pub orphans: u32,
}

impl TrackLedger {
    /// 构造零账。
    pub fn new() -> TrackLedger {
        TrackLedger { entries: Vec::new(), orphans: 0 }
    }

    /// 登记一条换轨条目：E 槽号必检、M 来源必检、重复必拒。
    pub fn record(&mut self, e_slot: usize, m_source: u32) -> Result<(), CharterCode> {
        if e_slot >= FIELD_SLOTS {
            return Err(CharterCode::SlotRange);
        }
        if m_source == 0 {
            self.orphans = self.orphans.saturating_add(1);
            return Err(CharterCode::TrackOrphan);
        }
        if self.entries.iter().any(|e| e.e_slot == e_slot) {
            return Err(CharterCode::DuplicatePair);
        }
        self.entries.push(TrackEntry { e_slot, m_source });
        Ok(())
    }

    /// 守恒式：E 型条目数 = 换轨条目数 = 配对基数 才为零漂移。
    pub fn drift(&self) -> Option<usize> {
        let need = TRACK_PAIRS;
        if self.entries.len() == need {
            None
        } else {
            Some(need.saturating_sub(self.entries.len()))
        }
    }
}

/// 标准换轨方案：J2 列 4 条逐条挂 M 型来源锚 1..=4。
pub fn track_standard(ledger: &mut TrackLedger) -> Result<(), CharterCode> {
    let mut n = 0usize;
    while n < TRACK_PAIRS {
        ledger.record(4 + n, (n as u32) + 1)?;
        n += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏字段样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFieldCase {
    /// 缺 tag（首段空）
    MissingTag,
    /// 缺主体（中段空）
    MissingSubject,
    /// 缺路径（尾段空）
    MissingPath,
    /// 非法 tag（J3）
    IllegalTag,
    /// 分段不足（两段）
    TooFewSegs,
}

impl BadFieldCase {
    /// 闭集全量。
    pub const ALL: [BadFieldCase; 5] = [
        BadFieldCase::MissingTag,
        BadFieldCase::MissingSubject,
        BadFieldCase::MissingPath,
        BadFieldCase::IllegalTag,
        BadFieldCase::TooFewSegs,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒收。
    pub fn inject(self) -> bool {
        let lines = [
            "|主体|路径",
            "J1||路径",
            "J1|主体|",
            "J3|主体|路径",
            "J1|主体",
        ];
        let mut i = 0usize;
        while i < BadFieldCase::ALL.len() {
            if let (Some(line), Some(c)) = (lines.get(i), BadFieldCase::ALL.get(i)) {
                if *c == self {
                    let r = parse_field(line);
                    return match c {
                        BadFieldCase::MissingTag => r.err() == Some(CharterCode::MissingTag),
                        BadFieldCase::MissingSubject => r.err() == Some(CharterCode::MissingSubject),
                        BadFieldCase::MissingPath => r.err() == Some(CharterCode::MissingPath),
                        BadFieldCase::IllegalTag => r.err() == Some(CharterCode::IllegalTag),
                        BadFieldCase::TooFewSegs => r.err() == Some(CharterCode::TooFewSegs),
                    };
                }
            }
            i += 1;
        }
        false
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_charter_injection() -> (TrackLedger, [u32; 5]) {
    let mut ledger = TrackLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadFieldCase::ALL.len() {
        if let Some(case) = BadFieldCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.orphans = ledger.orphans.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺成分/缺段族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；冻结后改字段族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；未冻结即用族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法 tag/槽位越界族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复登记族"),
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

/// 规范语料双跑指纹：解析→登记→结构化→冻结→换轨全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = CharterRegistry::new();
    let _ = register_standard(&mut reg);
    bytes.push(match reg.structure() {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.push(match reg.freeze() {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(reg.state as u8);
    let mut tl = TrackLedger::new();
    let _ = track_standard(&mut tl);
    bytes.push(match tl.drift() {
        None => 5,
        Some(_) => 6,
    });
    bytes.extend_from_slice(&(tl.entries.len() as u32).to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10081 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10081_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10081");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92400..=0x92440", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准开卷：8 槽字段逐条解析登记成功
    let mut reg = CharterRegistry::new();
    let std_ok = register_standard(&mut reg).is_ok();
    s.add("开卷登记：J1×4+J2×4 逐条解析入槽", std_ok,
          "结构协议冻结前先把 8 条示例字段逐条落槽");

    // 3. 结构化推进：双列各 4 才过闸
    let mut st = CharterRegistry::new();
    let _ = register_standard(&mut st);
    let structured = st.structure().is_ok() && st.state == CharterState::Structured;
    s.add("结构化闸：J1/J2 各 4 齐才推进", structured,
          "字段形态双列齐备是协议冻结的硬前置");

    // 4. 冻结单向：未结构化冻结拒、冻结成功、冻结后改字段拒
    let mut fz = CharterRegistry::new();
    let early_rej = fz.freeze() == Err(CharterCode::NotFrozen);
    let _ = register_standard(&mut fz);
    let _ = fz.structure();
    let frozen = fz.freeze().is_ok() && fz.state == CharterState::Frozen;
    let after_rej = fz.register(0, "J1|主体|路径") == Err(CharterCode::FrozenMutate);
    s.add("冻结闸：未结构化拒、冻结成、冻结后改拒", early_rej && frozen && after_rej,
          "Open→Structured→Frozen 单向，冻结后字段只读");

    // 5. 缺成分 10/10 拒收（锚点判据）
    let mut miss_hits = 0u32;
    let miss_lines = ["|主体|路径", "J1||路径", "J1|主体|"];
    let mut mi = 0usize;
    while mi < miss_lines.len() {
        if let Some(line) = miss_lines.get(mi) {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                if parse_field(line).is_err() {
                    miss_hits += 1;
                }
                round += 1;
            }
        }
        mi += 1;
    }
    let miss_expect = INJECT_ROUNDS * miss_lines.len() as u32;
    s.add("缺成分：10/10 次拒收", miss_hits == miss_expect,
          "tag/主体/路径任一缺失逐次显性拒收，30 轮全检出");

    // 6. 非法 tag 与分段不足拒绝
    let illegal_rej = parse_field("J3|主体|路径").err() == Some(CharterCode::IllegalTag);
    let segs_rej = parse_field("J1|主体").err() == Some(CharterCode::TooFewSegs)
        && parse_field("J1|主体|路径|多余").err() == Some(CharterCode::TooFewSegs);
    s.add("非法形态：J3 拒、两段/四段拒", illegal_rej && segs_rej,
          "判据位闭集 J1/J2；协议行恰三段，多段少段都拒");

    // 7. 重复登记拦截
    let mut dup = CharterRegistry::new();
    let _ = dup.register(0, "J1|fd 读|复测");
    let dup_ok = dup.register(1, "J1|fd 读|复测") == Err(CharterCode::DuplicatePair);
    s.add("重复登记：同 tag 同主体同路径拒收", dup_ok,
          "一槽一字段，完全同构字段不得重复占位");

    // 8. 槽位越界拦截
    let mut rr = CharterRegistry::new();
    let rr_ok = rr.register(FIELD_SLOTS, "J1|主体|路径") == Err(CharterCode::SlotRange);
    s.add("槽位越界：下标出 0..8 拒收", rr_ok,
          "登记必须落在合法槽窗内，越界显性拒绝");

    // 9. 标准换轨：4 条 E 型来源锚逐条挂账零漂移
    let mut tl = TrackLedger::new();
    let track_ok = track_standard(&mut tl).is_ok();
    s.add("换轨账：J2×4 逐条挂 M 来源零漂移",
          track_ok && tl.drift().is_none() && tl.entries.len() == TRACK_PAIRS,
          "每条 E 型条目必须可回溯到 M 型来源，换轨零孤儿");

    // 10. 孤儿条目拦截 + 逃逸显性入账
    let mut orp = TrackLedger::new();
    let orp_rej = orp.record(4, 0).is_err() && orp.orphans == 1;
    s.add("换轨孤儿：无 M 来源锚拒记且入账", orp_rej,
          "无源换轨不得入账；孤儿逃逸显性可见不静默");

    // 11. 重复换轨条目拦截
    let mut dt = TrackLedger::new();
    let _ = dt.record(4, 1);
    let dt_ok = dt.record(4, 2) == Err(CharterCode::DuplicatePair);
    s.add("重复换轨：同 E 槽二次挂账被拒", dt_ok,
          "一条 E 型判据只允许一条 M 来源锚");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_charter_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺 tag/缺主体/缺路径/非法 tag/分段不足全被显式拒收");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.orphans == 0 && ledger.entries.is_empty(),
          "注入核全程不得产生孤儿换轨条目逃逸");

    // 14. 文案闭集：非空
    let text_ok = CharterCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < CharterCode::ALL.len() {
        let mut j = i2 + 1;
        while j < CharterCode::ALL.len() {
            if let (Some(x), Some(y)) = (CharterCode::ALL.get(i2), CharterCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = CharterCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EPERM)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：槽位快照 + 换轨快照 + 指纹快照
    let slot_text = format!("已登记 {}/8 槽（J1×4+J2×4）", reg.fields.iter().filter(|f| f.is_some()).count());
    s.add_dyn("槽位快照", std_ok, &slot_text);
    let track_text = format!("换轨 {}/{} 对", tl.entries.len(), TRACK_PAIRS);
    s.add_dyn("换轨快照", tl.drift().is_none(), &track_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok_three_comps() {
        let f = parse_field("J1|fd 读|复测");
        assert!(f.is_ok());
        let f = f.ok().unwrap();
        assert_eq!(f.tag, FieldTag::J1);
        // "fd 读" = 'f','d',' ' 共 3 字节 + "读" 3 字节 = 6 字节
        assert_eq!(f.subject_len, 6);
    }

    #[test]
    fn missing_comps_rejected() {
        assert_eq!(parse_field("|主体|路径"), Err(CharterCode::MissingTag));
        assert_eq!(parse_field("J1||路径"), Err(CharterCode::MissingSubject));
        assert_eq!(parse_field("J1|主体|"), Err(CharterCode::MissingPath));
    }

    #[test]
    fn illegal_shapes_rejected() {
        assert_eq!(parse_field("J3|主体|路径"), Err(CharterCode::IllegalTag));
        assert_eq!(parse_field("J1|主体"), Err(CharterCode::TooFewSegs));
        assert_eq!(parse_field("J1|主体|路径|尾"), Err(CharterCode::TooFewSegs));
    }

    #[test]
    fn standard_registration_ok() {
        let mut reg = CharterRegistry::new();
        assert!(register_standard(&mut reg).is_ok());
        assert_eq!(reg.fields.iter().filter(|f| f.is_some()).count(), FIELD_SLOTS);
    }

    #[test]
    fn structure_then_freeze() {
        let mut reg = CharterRegistry::new();
        assert_eq!(reg.freeze(), Err(CharterCode::NotFrozen));
        let _ = register_standard(&mut reg);
        assert!(reg.structure().is_ok());
        assert!(reg.freeze().is_ok());
        assert_eq!(reg.register(0, "J1|x|y"), Err(CharterCode::FrozenMutate));
    }

    #[test]
    fn duplicate_pair_rejected() {
        let mut reg = CharterRegistry::new();
        let _ = reg.register(0, "J1|fd 读|复测");
        assert_eq!(reg.register(1, "J1|fd 读|复测"), Err(CharterCode::DuplicatePair));
    }

    #[test]
    fn slot_range_rejected() {
        let mut reg = CharterRegistry::new();
        assert_eq!(reg.register(8, "J1|x|y"), Err(CharterCode::SlotRange));
    }

    #[test]
    fn track_conservation() {
        let mut tl = TrackLedger::new();
        assert!(track_standard(&mut tl).is_ok());
        assert!(tl.drift().is_none());
        assert_eq!(tl.record(4, 0), Err(CharterCode::TrackOrphan));
        assert_eq!(tl.orphans, 1);
    }

    #[test]
    fn duplicate_track_rejected() {
        let mut tl = TrackLedger::new();
        let _ = tl.record(5, 1);
        assert_eq!(tl.record(5, 2), Err(CharterCode::DuplicatePair));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_charter_injection();
        assert_eq!(l.orphans, 0);
        assert!(l.entries.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(ENOENT));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10081_checks_green() {
        let s = run_unx10081_checks();
        assert!(s.all_passed(), "UNX-F10081 red: {}", s.red_items_text());
    }
}
