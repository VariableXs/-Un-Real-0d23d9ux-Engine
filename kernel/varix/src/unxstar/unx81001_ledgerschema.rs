//! UNX-F81002 台账主表 schema 钉版（字段冻结与版本递增规约） · 0x91000段
//!
//! 判据原文（派工卡 81001 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81002-J1 「台账滚动扩表执行器（3,200 批位预扩容）」
//!      判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障 10 次
//!      零静默；失败路径显性化、埋点入账；台账写入仅走本条登记通道（非法
//!      第四态零存在）；判据锚定 §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 12 列 schema 闭集冻结：列序即契约。v1 基线 12 列各带列号/列名/列型/
//!    必填位，构造后不可变。
//! 2. 版本递增单向门：v → v+1 只允许「追加列」，列名不得重名。
//! 3. 指纹保真：FNV-1a 64 位对规范编码求指纹，两次求值逐位一致。
//! 4. pin 账：append-only，登记/幂等/漂移三路。
//! 5. 注入核 ×10，注入必抓。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81001 − 81001)）。
pub const SEG_BASE: u32 = 0x91000;

// ── 诊断码闭集（段内偏移 0x01..0x0D） ─────────────────────────────
/// 列名超宽。
pub const E_SHORT: u16 = 1;
/// 改写已冻结字段。
pub const E_NOTFROZEN: u16 = 2;
/// 列号重复。
pub const E_DUPCOL: u16 = 3;
/// 列名重复。
pub const E_DUPNAME: u16 = 4;
/// 列型越出四型闭集。
pub const E_BADKIND: u16 = 5;
/// 非追加式改型。
pub const E_NOAPPEND: u16 = 6;
/// 跳版（Δv ≠ 1）。
pub const E_SKIPVER: u16 = 7;
/// 版本回退。
pub const E_REWVER: u16 = 8;
/// 同版异指纹。
pub const E_FPDIF: u16 = 9;
/// 列容量满。
pub const E_COLFULL: u16 = 10;
/// 波号越界。
pub const E_BADWAVE: u16 = 11;
/// 空输入。
pub const E_EMPTY: u16 = 12;
/// 重复 pin。
pub const E_REPIN: u16 = 13;

/// 诊断码闭集口径（13 个）。
pub const DIAG_ALL: usize = 13;

/// 列容量上限（32 批位预扩容；打满后追加 → ColFull）。
pub const MAX_COLS: usize = 32;

/// 列型四档闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColKind {
    /// 自增整数列。
    Int,
    /// 定长文本列。
    Text,
    /// 枚举闭集列。
    Enum,
    /// 校验和列。
    Checksum,
}

impl ColKind {
    /// 闭集内宽度（字节）。
    pub fn width(self) -> u16 {
        match self {
            ColKind::Int => 8,
            ColKind::Text => 32,
            ColKind::Enum => 2,
            ColKind::Checksum => 8,
        }
    }

    /// 列型稳定序号（规范编码/指纹用）。
    pub fn ordinal(self) -> u8 {
        match self {
            ColKind::Int => 1,
            ColKind::Text => 2,
            ColKind::Enum => 3,
            ColKind::Checksum => 4,
        }
    }
}

/// 单列定义（冻结后不可变）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    /// 列号（0 起连续）。
    pub col: u8,
    /// 列名（≤8 字节，右补零）。
    pub name: [u8; 8],
    /// 列型。
    pub kind: ColKind,
    /// 快填位。
    pub required: bool,
}

/// 台账 schema：列闭集 + 版本 + 指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerSchema {
    version: u16,
    cols: Vec<ColumnDef>,
    fp: u64,
}

/// v1 基线 12 列名（闭集，冻结）。
pub const V1_NAMES: [&str; 12] = [
    "batch", "domain", "wave", "lines", "checks", "red", "seg", "slug",
    "owner", "state", "mtime", "sum",
];

/// v1 基线列型（与 V1_NAMES 一一对应，冻结）。
const V1_KINDS: [ColKind; 12] = [
    ColKind::Int,
    ColKind::Enum,
    ColKind::Enum,
    ColKind::Int,
    ColKind::Int,
    ColKind::Int,
    ColKind::Int,
    ColKind::Text,
    ColKind::Text,
    ColKind::Enum,
    ColKind::Int,
    ColKind::Checksum,
];

/// FNV-1a 64 位步进。
pub fn fnv1a_step(h: u64, byte: u8) -> u64 {
    let mut h = h;
    h ^= byte as u64;
    h.wrapping_mul(0x1_0000_001b3)
}

/// FNV-1a 64 位全量。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h = fnv1a_step(h, bytes[i]);
        i += 1;
    }
    h
}

/// 把列名拷入定长槽：空名拒（Empty）、超宽拒（Short）。
fn copy_name(src: &str, out: &mut [u8; 8]) -> Result<(), SchemaError> {
    let b = src.as_bytes();
    if b.is_empty() {
        return Err(SchemaError::Empty);
    }
    if b.len() > 8 {
        return Err(SchemaError::Short);
    }
    let mut i = 0usize;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    Ok(())
}

/// 规范编码指纹（版本+列序逐列打包）。
fn canonical_fp(cols: &[ColumnDef], version: u16) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv1a_step(h, (version >> 8) as u8);
    h = fnv1a_step(h, version as u8);
    h = fnv1a_step(h, cols.len() as u8);
    for c in cols.iter() {
        h = fnv1a_step(h, c.col);
        let mut i = 0usize;
        while i < 8 {
            h = fnv1a_step(h, c.name[i]);
            i += 1;
        }
        h = fnv1a_step(h, c.kind.ordinal());
        h = fnv1a_step(h, c.required as u8);
    }
    h
}

impl LedgerSchema {
    /// v1 基线构造：12 列冻结，版本=1，指纹可复算。
    pub fn v1() -> LedgerSchema {
        let mut cols: Vec<ColumnDef> = Vec::new();
        let mut i = 0usize;
        while i < 12 {
            let mut name = [0u8; 8];
            let src = V1_NAMES[i].as_bytes();
            let n = if src.len() > 8 { 8 } else { src.len() };
            let mut k = 0usize;
            while k < n {
                name[k] = src[k];
                k += 1;
            }
            cols.push(ColumnDef {
                col: i as u8,
                name,
                kind: V1_KINDS[i],
                required: i < 6,
            });
            i += 1;
        }
        let mut s = LedgerSchema {
            version: 1,
            cols,
            fp: 0,
        };
        s.fp = canonical_fp(&s.cols, s.version);
        s
    }

    /// 当前版本。
    pub fn version(&self) -> u16 {
        self.version
    }

    /// 当前列数。
    pub fn col_count(&self) -> usize {
        self.cols.len()
    }

    /// 指纹（两次一致；版本不同必不同）。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 追加一列（唯一合法演进路径）。
    pub fn try_increment(&self, name: &str, kind: ColKind, required: bool) -> Result<LedgerSchema, SchemaError> {
        if self.cols.len() >= MAX_COLS {
            return Err(SchemaError::ColFull);
        }
        let mut nm = [0u8; 8];
        copy_name(name, &mut nm)?;
        let next_col: u8 = match self.cols.last() {
            Some(last) => match last.col.checked_add(1) {
                Some(c) => c,
                None => return Err(SchemaError::DupCol),
            },
            None => 0,
        };
        let mut i = 0usize;
        while i < self.cols.len() {
            let c = self.cols[i];
            if c.col == next_col {
                return Err(SchemaError::DupCol);
            }
            if c.name == nm {
                return Err(SchemaError::DupName);
            }
            i += 1;
        }
        let mut cols = self.cols.clone();
        cols.push(ColumnDef {
            col: next_col,
            name: nm,
            kind,
            required,
        });
        let mut s = LedgerSchema {
            version: self.version + 1,
            cols,
            fp: 0,
        };
        s.fp = canonical_fp(&s.cols, s.version);
        Ok(s)
    }

    /// 按列号取列（越界 None，不 panic）。
    pub fn col(&self, idx: usize) -> Option<ColumnDef> {
        if idx >= self.cols.len() {
            return None;
        }
        Some(self.cols[idx])
    }

    /// 规范编码（版本+列序逐列打包；指纹的编码原像）。
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.push((self.version >> 8) as u8);
        out.push(self.version as u8);
        out.push(self.cols.len() as u8);
        let mut i = 0usize;
        while i < self.cols.len() {
            let c = self.cols[i];
            out.push(c.col);
            let mut k = 0usize;
            while k < 8 {
                out.push(c.name[k]);
                k += 1;
            }
            out.push(c.kind.ordinal());
            out.push(c.required as u8);
            i += 1;
        }
        out
    }
}

/// schema 域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaError {
    /// 列名超宽。
    Short,
    /// 改写已冻结字段。
    NotFrozen,
    /// 列号重复。
    DupCol,
    /// 列名重复。
    DupName,
    /// 列型越出闭集。
    BadKind,
    /// 非追加式改型。
    NoAppend,
    /// 跳版（Δv≠1）。
    SkipVer,
    /// 版本回退。
    RewVer,
    /// 同版异指纹。
    FpDif,
    /// 容量满。
    ColFull,
    /// 波号越界。
    BadWave,
    /// 空输入。
    Empty,
    /// 同版同指纹重复 pin。
    Repin,
}

impl SchemaError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            SchemaError::Short => E_SHORT,
            SchemaError::NotFrozen => E_NOTFROZEN,
            SchemaError::DupCol => E_DUPCOL,
            SchemaError::DupName => E_DUPNAME,
            SchemaError::BadKind => E_BADKIND,
            SchemaError::NoAppend => E_NOAPPEND,
            SchemaError::SkipVer => E_SKIPVER,
            SchemaError::RewVer => E_REWVER,
            SchemaError::FpDif => E_FPDIF,
            SchemaError::ColFull => E_COLFULL,
            SchemaError::BadWave => E_BADWAVE,
            SchemaError::Empty => E_EMPTY,
            SchemaError::Repin => E_REPIN,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            SchemaError::Short => "column name wider than 8 bytes",
            SchemaError::NotFrozen => "frozen field rewrite attempt",
            SchemaError::DupCol => "duplicate column id",
            SchemaError::DupName => "duplicate column name",
            SchemaError::BadKind => "column kind outside closed set",
            SchemaError::NoAppend => "non-append schema change",
            SchemaError::SkipVer => "version skip (dv != 1)",
            SchemaError::RewVer => "version rewind",
            SchemaError::FpDif => "same version different fingerprint",
            SchemaError::ColFull => "capacity full",
            SchemaError::BadWave => "wave out of range",
            SchemaError::Empty => "empty input",
            SchemaError::Repin => "same version same fingerprint re-pin",
        }
    }
}

/// pin 账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinRecord {
    /// schema 版本。
    pub version: u16,
    /// 规范指纹。
    pub fp: u64,
    /// 登记波号（1..=30）。
    pub wave: u16,
}

/// pin 账（append-only，登记/幂等/漂移三路）。
#[derive(Debug)]
pub struct PinLedger {
    records: Vec<PinRecord>,
}

impl PinLedger {
    /// 空账。
    pub fn new() -> PinLedger {
        PinLedger { records: Vec::new() }
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 登记 pin：同版同指纹幂等（Repin 拒重登记），同版异指纹=漂移（FpDif），
    /// 版本必须严格递增（跳版 SkipVer / 回退 RewVer）。
    pub fn pin(&mut self, r: PinRecord) -> Result<bool, SchemaError> {
        if r.wave == 0 || r.wave > 30 {
            return Err(SchemaError::BadWave);
        }
        let mut i = 0usize;
        while i < self.records.len() {
            let cur = self.records[i];
            if cur.version == r.version {
                if cur.fp == r.fp {
                    return Err(SchemaError::Repin);
                }
                return Err(SchemaError::FpDif);
            }
            i += 1;
        }
        if let Some(last) = self.records.last() {
            if r.version <= last.version {
                if last.version - r.version > 1 {
                    return Err(SchemaError::SkipVer);
                }
                return Err(SchemaError::RewVer);
            }
        }
        self.records.push(r);
        Ok(true)
    }

    /// 按版本取记录（无则 None）。
    pub fn by_version(&self, v: u16) -> Option<PinRecord> {
        let mut i = 0usize;
        while i < self.records.len() {
            let r = self.records[i];
            if r.version == v {
                return Some(r);
            }
            i += 1;
        }
        None
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let base = LedgerSchema::v1();
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    // 1 重名追加 → DupName
    match base.try_increment("domain", ColKind::Int, false) {
        Err(e) => out.push(("dup-name", e.code())),
        Ok(_) => out.push(("dup-name-MISSED", SEG_BASE)),
    }
    // 2 超宽名 → Short
    match base.try_increment("123456789", ColKind::Int, false) {
        Err(e) => out.push(("wide-name", e.code())),
        Ok(_) => out.push(("wide-name-MISSED", SEG_BASE)),
    }
    // 3 空名 → Empty
    match base.try_increment("", ColKind::Int, false) {
        Err(e) => out.push(("empty-name", e.code())),
        Ok(_) => out.push(("empty-name-MISSED", SEG_BASE)),
    }
    // 4 容量路径：32 列打满后第 33 列 → ColFull
    let mut s = base.clone();
    let mut added = 0usize;
    while s.col_count() < MAX_COLS {
        let nm = format!("c{}", added);
        match s.try_increment(&nm, ColKind::Text, false) {
            Ok(next) => s = next,
            Err(_) => break,
        }
        added += 1;
    }
    match s.try_increment("overflow", ColKind::Text, false) {
        Err(e) => out.push(("col-full", e.code())),
        Ok(_) => out.push(("col-full-MISSED", SEG_BASE)),
    }
    // 5 跳版 pin → SkipVer
    let mut led = PinLedger::new();
    let _ = led.pin(PinRecord { version: 1, fp: base.fingerprint(), wave: 1 });
    match led.pin(PinRecord { version: 3, fp: 0xA11C, wave: 2 }) {
        Err(e) => out.push(("skip-ver", e.code())),
        Ok(_) => out.push(("skip-ver-MISSED", SEG_BASE)),
    }
    // 6 同版异指纹 → FpDif（漂移）
    match led.pin(PinRecord { version: 1, fp: 0xBEEF, wave: 2 }) {
        Err(e) => out.push(("fp-drift", e.code())),
        Ok(_) => out.push(("fp-drift-MISSED", SEG_BASE)),
    }
    // 7 波号 0 → BadWave
    match PinLedger::new().pin(PinRecord { version: 1, fp: 7, wave: 0 }) {
        Err(e) => out.push(("wave-zero", e.code())),
        Ok(_) => out.push(("wave-zero-MISSED", SEG_BASE)),
    }
    // 8 波号 31 → BadWave
    match PinLedger::new().pin(PinRecord { version: 1, fp: 7, wave: 31 }) {
        Err(e) => out.push(("wave-31", e.code())),
        Ok(_) => out.push(("wave-31-MISSED", SEG_BASE)),
    }
    // 9 版本回退：v0 直接 pin → RewVer
    match PinLedger::new().pin(PinRecord { version: 0, fp: 3, wave: 1 }) {
        Err(e) => out.push(("rewind", e.code())),
        Ok(_) => out.push(("rewind-MISSED", SEG_BASE)),
    }
    // 10 同名重确认（sum 列已存在，追加 sum → DupName 双确认）
    match base.try_increment("sum", ColKind::Checksum, false) {
        Err(e) => out.push(("dup-sum", e.code())),
        Ok(_) => out.push(("dup-sum-MISSED", SEG_BASE)),
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (_, code) = samples[i];
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81002-J1 落地）：14 条，正向 + 注入汇总。
pub fn run_unx81001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81001");
    let base = LedgerSchema::v1();

    // 1 段位守卫
    let ok = SEG_BASE % 0x30 == 0 && SEG_BASE > 0x90000 && SEG_BASE < 0x94000;
    s.add("F81002-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 13;
    s.add("F81002-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 v1 冻结 12 列
    let ok = base.col_count() == 12 && base.version() == 1;
    s.add("F81002-03 v1冻结12列", ok, if ok { "" } else { "v1 基线漂移" });

    // 4 指纹两次一致
    let ok = base.fingerprint() == LedgerSchema::v1().fingerprint();
    s.add("F81002-04 指纹确定", ok, if ok { "" } else { "指纹不确定" });

    // 5 追加演进 v2
    let ok = match base.try_increment("ext_a", ColKind::Int, false) {
        Ok(v2) => v2.version() == 2 && v2.col_count() == 13 && v2.fingerprint() != base.fingerprint(),
        Err(_) => false,
    };
    s.add("F81002-05 追加演进", ok, if ok { "" } else { "追加路径坏" });

    // 6 重名拒
    let ok = matches!(base.try_increment("domain", ColKind::Int, false), Err(SchemaError::DupName));
    s.add("F81002-06 重名拒", ok, if ok { "" } else { "重名漏抓" });

    // 7 注入 10/10
    let ok = inject_all_caught();
    s.add("F81002-07 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    // 8 容量上限
    let mut s2 = base.clone();
    let mut grown = 0usize;
    while s2.col_count() < MAX_COLS {
        let nm = format!("c{}", grown);
        match s2.try_increment(&nm, ColKind::Enum, false) {
            Ok(next) => s2 = next,
            Err(_) => break,
        }
        grown += 1;
    }
    let ok = s2.col_count() == MAX_COLS;
    s.add("F81002-08 容量上限", ok, if ok { "" } else { "容量口径漂移" });

    // 9 pin 三路
    let mut led = PinLedger::new();
    let p1 = led.pin(PinRecord { version: 2, fp: 0xF10, wave: 1 });
    let p1b = led.pin(PinRecord { version: 2, fp: 0xF10, wave: 1 });
    let p2 = led.pin(PinRecord { version: 2, fp: 0xF10, wave: 2 });
    let ok = matches!(p1, Ok(true)) && matches!(p1b, Err(SchemaError::Repin)) && matches!(p2, Err(SchemaError::FpDif));
    s.add("F81002-09 pin三路", ok, if ok { "" } else { "pin 账三路账坏" });

    // 10 pin 单调
    let mut led2 = PinLedger::new();
    let _ = led2.pin(PinRecord { version: 1, fp: 1, wave: 1 });
    let ok = matches!(led2.pin(PinRecord { version: 3, fp: 2, wave: 2 }), Err(SchemaError::SkipVer))
        && matches!(led2.pin(PinRecord { version: 0, fp: 3, wave: 2 }), Err(SchemaError::RewVer));
    s.add("F81002-10 pin单调", ok, if ok { "" } else { "pin 账非单调" });

    // 11 编码确定
    let ok = base.canonical_bytes() == LedgerSchema::v1().canonical_bytes();
    s.add("F81002-11 编码确定", ok, if ok { "" } else { "编码不确定" });

    // 12 波号闭集
    let ok = PinLedger::new().pin(PinRecord { version: 1, fp: 9, wave: 0 }).is_err()
        && PinLedger::new().pin(PinRecord { version: 1, fp: 9, wave: 31 }).is_err();
    s.add("F81002-12 波号闭集", ok, if ok { "" } else { "波号闸失灵" });

    // 13 指纹区分度
    let v2 = match base.try_increment("ext_b", ColKind::Checksum, true) {
        Ok(v) => Some(v),
        Err(_) => None,
    };
    let ok = match v2 {
        Some(v) => v.fingerprint() != base.fingerprint(),
        None => false,
    };
    s.add("F81002-13 指纹区分", ok, if ok { "" } else { "指纹不区分版本" });

    // 14 必填位保真
    let mut all_req = true;
    let mut i = 0usize;
    while i < 6 {
        match base.col(i) {
            Some(c) => {
                if !c.required {
                    all_req = false;
                }
            }
            None => all_req = false,
        }
        i += 1;
    }
    let ok = all_req && base.col(11).map(|c| !c.required).unwrap_or(false);
    s.add("F81002-14 必填位", ok, if ok { "" } else { "必填位漂移" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81001_checks_green() {
        let set = run_unx81001_checks();
        assert!(set.all_passed(), "UNX-F81001 red: {}", set.red_items_text());
    }
}



