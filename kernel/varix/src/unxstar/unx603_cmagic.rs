//! unx603_cmagic —— UNX-F0603 cpio c_magic 字段识别与 newc 整数字段编解码。
//!
//! 判据原文摘录（CoRun Varix STAR II · Unxreal 主册 · UNX-A1-B31）：
//!   UNX-F0603 · cpio newc 魔数识别（0x070701）· 判据：魔数核（首条目 0x070701
//!   6 字节 1/1 命中），newc 格式版本账注（c_magic 字段）。
//! 本模块落地判据后半句：c_magic 字段（newc 头第 0..6 字节）识别 + newc 全部
//!   整数字段（c_ino..c_check 等）ASCII-十六进制编解码（真实 cpio 规格）+ 字段
//!   级版本账注（每解析一条目以其 c_magic 定版本入账）。纯功能线 no_std + alloc，
//!   零 unwrap/expect/panic，逐条定制，诊断段 0x7520。

use alloc::{format, string::String};

use super::unx603_newcmagic::CpioMagicKind;

/// 模块版本。
pub const V603C_VERSION: &str = "v603-cmagic-v1";

/// 诊断段基址（0x7520）。
pub const V603C_SEG_BASE: u16 = 0x7520;

// --------------------------------------------------------------------------
// 诊断码（v603c 十六码互斥 + segment_guard）
// --------------------------------------------------------------------------
/// c_magic 字段识别诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V603cCode {
    /// 0x7520 字段长度非法（c_magic 非 6 字节——头截断）。
    FieldLenIllegal,
    /// 0x7521 魔数字符非数字（c_magic 含非 0-7/非 ASCII 数字）。
    MagicCharNonDigit,
    /// 0x7522 整数字段字符非十六进制（8 字段含非 [0-9a-fA-F]）。
    IntCharNonHex,
    /// 0x7523 整数字段长度非法（c_ino 等固定 8 字节被违反）。
    IntFieldLenIllegal,
    /// 0x7524 整数字段溢出（解析值超出 32 位表示）。
    IntFieldOverflow,
    /// 0x7525 字段版本未知（c_magic 判别落出已知格式族）。
    FieldVersionUnknown,
    /// 0x7526 字段版本账失配（账注版本与 c_magic 判别不一致）。
    VersionLedgerMiss,
    /// 0x7527 账目不守恒（Σ字段登记 ≠ 已解析条目数）。
    LedgerBroken,
    /// 0x7528 编号字段缺失（newc 头必填字段位图为空）。
    RequiredFieldMissing,
    /// 0x7529 字段派生命名冲突（两字段同名/同位）。
    FieldAliasClash,
    /// 0x752a 头部尾随越界（字段解析越过 110 头界）。
    HeaderOverflow,
    /// 0x752b 消歧失败（c_magic 多候选无法定版）。
    DisambiguateFail,
    /// 0x752c 回环字段（字段依赖自身）。
    FieldSelfLoop,
    /// 0x752d 次序违例（字段解析序非规格升序）。
    FieldOrderViolated,
    /// 0x752e 溢出守护（长度/计数加法饱和）。
    OverflowGuard,
    /// 0x752f 重入守卫（字段解析临界区嵌套越限）。
    ReentrantGuard,
}

impl V603cCode {
    /// 全量闭集。
    pub const ALL: [V603cCode; 16] = [
        V603cCode::FieldLenIllegal,
        V603cCode::MagicCharNonDigit,
        V603cCode::IntCharNonHex,
        V603cCode::IntFieldLenIllegal,
        V603cCode::IntFieldOverflow,
        V603cCode::FieldVersionUnknown,
        V603cCode::VersionLedgerMiss,
        V603cCode::LedgerBroken,
        V603cCode::RequiredFieldMissing,
        V603cCode::FieldAliasClash,
        V603cCode::HeaderOverflow,
        V603cCode::DisambiguateFail,
        V603cCode::FieldSelfLoop,
        V603cCode::FieldOrderViolated,
        V603cCode::OverflowGuard,
        V603cCode::ReentrantGuard,
    ];

    /// 码值（与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            V603cCode::FieldLenIllegal => 0x7520,
            V603cCode::MagicCharNonDigit => 0x7521,
            V603cCode::IntCharNonHex => 0x7522,
            V603cCode::IntFieldLenIllegal => 0x7523,
            V603cCode::IntFieldOverflow => 0x7524,
            V603cCode::FieldVersionUnknown => 0x7525,
            V603cCode::VersionLedgerMiss => 0x7526,
            V603cCode::LedgerBroken => 0x7527,
            V603cCode::RequiredFieldMissing => 0x7528,
            V603cCode::FieldAliasClash => 0x7529,
            V603cCode::HeaderOverflow => 0x752a,
            V603cCode::DisambiguateFail => 0x752b,
            V603cCode::FieldSelfLoop => 0x752c,
            V603cCode::FieldOrderViolated => 0x752d,
            V603cCode::OverflowGuard => 0x752e,
            V603cCode::ReentrantGuard => 0x752f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            V603cCode::FieldLenIllegal => "字段长度非法（c_magic 非 6 字节——头截断）",
            V603cCode::MagicCharNonDigit => "魔数字符非数字（c_magic 含非 ASCII 数字）",
            V603cCode::IntCharNonHex => "整数字段字符非十六进制（含非 [0-9a-fA-F]）",
            V603cCode::IntFieldLenIllegal => "整数字段长度非法（固定 8 字节被违反）",
            V603cCode::IntFieldOverflow => "整数字段溢出（解析值超 32 位）",
            V603cCode::FieldVersionUnknown => "字段版本未知（c_magic 判别落出已知族）",
            V603cCode::VersionLedgerMiss => "字段版本账失配（账注版本与 c_magic 判别不一致）",
            V603cCode::LedgerBroken => "账目不守恒（Σ字段登记 ≠ 已解析条目数）",
            V603cCode::RequiredFieldMissing => "编号字段缺失（newc 头必填字段位图为空）",
            V603cCode::FieldAliasClash => "字段派生命名冲突（两字段同名/同位）",
            V603cCode::HeaderOverflow => "头部尾随越界（字段解析越过 110 头界）",
            V603cCode::DisambiguateFail => "消歧失败（c_magic 多候选无法定版）",
            V603cCode::FieldSelfLoop => "回环字段（字段依赖自身）",
            V603cCode::FieldOrderViolated => "次序违例（字段解析序非规格升序）",
            V603cCode::OverflowGuard => "溢出守护（长度/计数加法饱和）",
            V603cCode::ReentrantGuard => "重入守卫（字段解析临界区嵌套越限）",
        }
    }

    /// 可操作修复建议。
    pub const fn suggestion(self) -> &'static str {
        match self {
            V603cCode::FieldLenIllegal => "按 newc 规格取 c_magic 恰好 6 字节",
            V603cCode::MagicCharNonDigit => "核对 c_magic 六字符均为 ASCII 数字",
            V603cCode::IntCharNonHex => "剔除非法字符或改判非 newc 头",
            V603cCode::IntFieldLenIllegal => "整数字段一律 8 字符，补齐或截纠",
            V603cCode::IntFieldOverflow => "8 字符恰 32 位，超出即拒并核对输入",
            V603cCode::FieldVersionUnknown => "按已知格式族表补登记或判为新格式",
            V603cCode::VersionLedgerMiss => "以 c_magic 判别族重登记版本账后重跑",
            V603cCode::LedgerBroken => "按守恒律重建字段账后重跑",
            V603cCode::RequiredFieldMissing => "补齐 newc 必填字段位图后重跑",
            V603cCode::FieldAliasClash => "重命名字段保证同位唯一",
            V603cCode::HeaderOverflow => "限制字段解析于 110 头界内",
            V603cCode::DisambiguateFail => "收敛候选至单一格式族再定版",
            V603cCode::FieldSelfLoop => "打断字段自依赖环",
            V603cCode::FieldOrderViolated => "按规格字段序重排后重跑",
            V603cCode::OverflowGuard => "改用 saturating/wrapping 或收紧输入界",
            V603cCode::ReentrantGuard => "退出临界区后再入",
        }
    }
}

/// 段位守卫。
pub const fn v603c_segment_guard() -> bool {
    let mut i = 0usize;
    while i < V603cCode::ALL.len() {
        let c = V603cCode::ALL[i];
        if (c.code() as usize) != (V603C_SEG_BASE as usize) + i {
            return false;
        }
        i += 1;
    }
    true
}

/// 错误载体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct V603cErr {
    /// 诊断码值。
    pub code: u16,
    /// 现场位置中文短注。
    pub at: &'static str,
}

impl V603cErr {
    /// 构造。
    pub const fn new(code: V603cCode, at: &'static str) -> V603cErr {
        V603cErr { code: code.code(), at }
    }
    /// 中文诊断全句。
    pub fn detail(&self) -> String {
        let idx = self.code.saturating_sub(V603C_SEG_BASE) as usize;
        let (nm, sg) = if idx < V603cCode::ALL.len() {
            (V603cCode::ALL[idx].name(), V603cCode::ALL[idx].suggestion())
        } else {
            ("未知诊断（越段）", "核对诊断码分配表")
        };
        format!("0x{:04X} {} @{}：{}", self.code, nm, self.at, sg)
    }
}

const fn v603c_err(code: V603cCode, at: &'static str) -> V603cErr {
    V603cErr::new(code, at)
}

// --------------------------------------------------------------------------
// c_magic 字段识别（newc 头第 0..6 字节，ASCII 十进制串，须为已知族）
// --------------------------------------------------------------------------
/// c_magic 字段长度（固定 6）。
pub const CMAGIC_LEN: usize = 6;

/// 单字符是否为 ASCII 十进制数字。
const fn is_ascii_digit(b: u8) -> bool {
    b >= b'0' && b <= b'9'
}

/// c_magic 字段判别（取恰好 6 字节且判别为已知格式族）。
/// 长度不足/非数字/未知族均显性返错（零 panic）。
pub fn parse_cmagic_field(field: &[u8]) -> Result<CpioMagicKind, V603cErr> {
    if field.len() < CMAGIC_LEN {
        return Err(v603c_err(V603cCode::FieldLenIllegal, "parse_cmagic_field"));
    }
    let mut i = 0usize;
    while i < CMAGIC_LEN {
        match field.get(i) {
            None => return Err(v603c_err(V603cCode::FieldLenIllegal, "parse_cmagic_field")),
            Some(&b) => {
                if !is_ascii_digit(b) {
                    return Err(v603c_err(V603cCode::MagicCharNonDigit, "parse_cmagic_field"));
                }
            }
        }
        i += 1;
    }
    let want = match field.get(..CMAGIC_LEN) {
        Some(w) => w,
        None => return Err(v603c_err(V603cCode::FieldLenIllegal, "parse_cmagic_field")),
    };
    // 精确比已知族（newc 族 ASCII "07070X"，odc "070707"，bin 非 ASCII 由上层处理）。
    if want == b"070701" {
        Ok(CpioMagicKind::Newc)
    } else if want == b"070702" {
        Ok(CpioMagicKind::NewcCrc)
    } else if want == b"070707" {
        Ok(CpioMagicKind::Odc)
    } else {
        Err(v603c_err(V603cCode::FieldVersionUnknown, "parse_cmagic_field"))
    }
}

/// c_magic 字段中文串表示（返回 "070701" 等 6 字符字面，仅 ASCII 数字合法）。
pub fn cmagic_field_display(field: &[u8]) -> Result<String, V603cErr> {
    if field.len() < CMAGIC_LEN {
        return Err(v603c_err(V603cCode::FieldLenIllegal, "cmagic_field_display"));
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < CMAGIC_LEN {
        match field.get(i) {
            None => return Err(v603c_err(V603cCode::FieldLenIllegal, "cmagic_field_display")),
            Some(&b) => {
                if !is_ascii_digit(b) {
                    return Err(v603c_err(V603cCode::MagicCharNonDigit, "cmagic_field_display"));
                }
                s.push(b as char);
            }
        }
        i += 1;
    }
    Ok(s)
}

// --------------------------------------------------------------------------
// newc 整数字段编解码（ASCII-十六进制，8 字符定长，真实 cpio 规格）
// --------------------------------------------------------------------------
/// newc 整数字段长度（固定 8 字符十六进制）。
pub const INT_FIELD_LEN: usize = 8;

/// 单字符十六进制值（0-15），非十六进制返 None（零 panic 编码）。
const fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        b'A'..=b'F' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

/// 解码 8 字符 ASCII-十六进制为 u32（大端：首字符为高位半字节）。
/// 长度/字符/溢出均显性返错。
pub fn decode_int_field(field: &[u8]) -> Result<u32, V603cErr> {
    if field.len() != INT_FIELD_LEN {
        return Err(v603c_err(V603cCode::IntFieldLenIllegal, "decode_int_field"));
    }
    let mut acc: u32 = 0;
    let mut i = 0usize;
    while i < INT_FIELD_LEN {
        let ch = match field.get(i) {
            None => return Err(v603c_err(V603cCode::IntFieldLenIllegal, "decode_int_field")),
            Some(&b) => b,
        };
        let hv = match hex_val(ch) {
            None => return Err(v603c_err(V603cCode::IntCharNonHex, "decode_int_field")),
            Some(v) => v,
        };
        // 左移 4 位前的溢出守卫（满 8 字符时最高半字节落在位 28..31，须 |hv|≤0xF）。
        let shifted = acc.checked_shl(4);
        match shifted {
            None => return Err(v603c_err(V603cCode::IntFieldOverflow, "decode_int_field")),
            Some(s) => {
                acc = s | hv;
            }
        }
        i += 1;
    }
    Ok(acc)
}

/// 编码 u32 为 8 字符 ASCII-十六进制（大写，定长，超宽会被丢弃以保持 8 字符）。
/// 返回固定 8 字节缓冲。
pub fn encode_int_field(value: u32) -> [u8; INT_FIELD_LEN] {
    const HEXD: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0u8; INT_FIELD_LEN];
    let mut i = 0usize;
    while i < INT_FIELD_LEN {
        let shift = (INT_FIELD_LEN - 1 - i) * 4;
        let nib = ((value >> shift) & 0xF) as usize;
        // nib ∈ 0..15 恒在 HEXD 界内；用 get 兜底零 panic。
        match HEXD.get(nib) {
            Some(&c) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = c;
                }
            }
            None => {}
        }
        i += 1;
    }
    out
}

/// 往返一致性（encode 后可 decode 回原值——确定性编解码）。
pub fn int_field_roundtrip(value: u32) -> bool {
    let enc = encode_int_field(value);
    match decode_int_field(&enc) {
        Ok(v) => v == value,
        Err(_) => false,
    }
}

// --------------------------------------------------------------------------
// newc 头部字段位图（13 字段：c_magic + 12 个 8 字符整数字段）
// --------------------------------------------------------------------------
/// newc 头字段总数（1 魔数 + 12 整数）。
pub const NEWC_HEAD_FIELDS: usize = 13;
/// newc 整数字段数（c_ino..c_check 共 13 个；newc 头 = 6 + 13×8 = 110 字节）。
pub const NEWC_INT_FIELDS: usize = 13;

/// 整数字段的规格名表（按规格序；索引 0 = c_ino ... 索引 11 = c_check）。
pub static INT_FIELD_NAMES: [&str; NEWC_INT_FIELDS] = [
    "c_ino", "c_mode", "c_uid", "c_gid", "c_nlink", "c_mtime",
    "c_filesize", "c_devmajor", "c_devminor", "c_rdevmajor",
    "c_rdevminor", "c_namesize", "c_check",
];
/// 注：末位 c_check 在 newc(070701) 通常为零、在 newc-crc(070702) 承载校验和；

/// 字段名唯一性守卫（逐对比对同名）。
pub fn int_field_names_unique() -> bool {
    let mut i = 0usize;
    while i < NEWC_INT_FIELDS {
        match INT_FIELD_NAMES.get(i) {
            None => return false,
            Some(&a) => {
                let mut j = i + 1;
                while j < NEWC_INT_FIELDS {
                    match INT_FIELD_NAMES.get(j) {
                        Some(&b) => {
                            if a == b {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    j += 1;
                }
            }
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 字段级 newc 头解析器（c_magic + 12 整数字段）+ 字段版本账注
// --------------------------------------------------------------------------
/// 解析后的 newc 头（c_magic 族 + 12 整数字段值 + 可选 c_check）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParsedNewcHead {
    /// 格式族（由 c_magic 判别）。
    pub kind: CpioMagicKind,
    /// 12 个整数字段值（规格序）。
    pub ints: [u32; NEWC_INT_FIELDS],
    /// c_check 值（newc-crc 特有；其余族 None）。
    pub check: Option<u32>,
    /// 头部吞掉的字节数（110；newc/odc 族变长另计）。
    pub consumed: u32,
}

/// 字段级 newc 头解析（从头缓冲按规格切分 13 字段逐一判别/解码）。
/// 缓冲区须 ≥ 110 字节；越界/非 hex/未知魔数显性返错。
pub fn parse_newc_head(buf: &[u8]) -> Result<ParsedNewcHead, V603cErr> {
    // 字段 0：c_magic。
    let magic_win = match buf.get(..CMAGIC_LEN) {
        Some(w) => w,
        None => return Err(v603c_err(V603cCode::FieldLenIllegal, "parse_newc_head")),
    };
    let kind = parse_cmagic_field(magic_win)?;
    if kind == CpioMagicKind::Odc || kind == CpioMagicKind::Bin {
        // 本解析器专司 newc 族；odc/bin 走各自头解析（此显性拒以防混用）。
        return Err(v603c_err(V603cCode::FieldVersionUnknown, "parse_newc_head"));
    }

    let int_count = NEWC_INT_FIELDS;
    let mut ints = [0u32; NEWC_INT_FIELDS];
    let mut pos = CMAGIC_LEN;
    let mut idx = 0usize;
    while idx < int_count {
        // 越界守卫：字段不得越过 110 头界。
        let end = pos.wrapping_add(INT_FIELD_LEN);
        if end > buf.len() {
            return Err(v603c_err(V603cCode::HeaderOverflow, "parse_newc_head"));
        }
        let win = match buf.get(pos..end) {
            Some(w) => w,
            None => return Err(v603c_err(V603cCode::HeaderOverflow, "parse_newc_head")),
        };
        let v = decode_int_field(win)?;
        match ints.get_mut(idx) {
            Some(slot) => *slot = v,
            None => return Err(v603c_err(V603cCode::RequiredFieldMissing, "parse_newc_head")),
        }
        pos = end;
        idx += 1;
    }
    // c_check 为第 13 个整数字段（索引 12）：newc-crc 承载校验和，newc 常零。
    let check = match ints.get(NEWC_INT_FIELDS - 1) {
        Some(&v) => Some(v),
        None => None,
    };
    // 头长度守恒：6（魔数）+ 13×8（整数）= 110 字节（pos 落于此）。
    let consumed = pos as u32;
    Ok(ParsedNewcHead { kind, ints, check, consumed })
}

// --------------------------------------------------------------------------
// 字段版本账注（每解析条目以 c_magic 定版入账；封账 + 指纹 + 守恒）
// --------------------------------------------------------------------------
/// 字段版本账（newc 版本账注的记录载体）。
pub struct CmagicVersionLedger {
    /// 每族登记次数（索引 = 族 index()）。
    counts: [u32; 4],
    /// 已解析条目总数（守恒用）。
    entries: u32,
    /// 是否封账。
    sealed: bool,
    /// 封账指纹。
    seal_digest: u32,
}

impl CmagicVersionLedger {
    /// 新建空账。
    pub fn new() -> CmagicVersionLedger {
        CmagicVersionLedger { counts: [0; 4], sealed: false, seal_digest: 0, entries: 0 }
    }

    /// 登记一个已解析头的族（封账拒改；未知族拒；序号由内部自增保证单调）。
    pub fn record_head(&mut self, head: &ParsedNewcHead) -> Result<(), V603cErr> {
        if self.sealed {
            return Err(v603c_err(V603cCode::VersionLedgerMiss, "CmagicVersionLedger::record_head"));
        }
        let idx = head.kind.index();
        if idx >= self.counts.len() {
            return Err(v603c_err(V603cCode::FieldVersionUnknown, "CmagicVersionLedger::record_head"));
        }
        if let Some(slot) = self.counts.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
        self.entries = self.entries.saturating_add(1);
        Ok(())
    }

    /// 某族登记条目数。
    pub fn count(&self, kind: CpioMagicKind) -> u32 {
        let idx = kind.index();
        match self.counts.get(idx) {
            Some(c) => *c,
            None => 0,
        }
    }

    /// 已登记条目总数。
    pub fn entries(&self) -> u32 {
        self.entries
    }

    /// 守恒自检：Σ counts == entries。
    pub fn conservation(&self) -> bool {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                s = s.wrapping_add(*c);
            }
            i += 1;
        }
        s == self.entries
    }

    /// 指纹（对 counts + entries 的确定性摘要）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                let cb = c.to_le_bytes();
                let mut t = 0usize;
                while t < cb.len() {
                    if let Some(b) = cb.get(t) {
                        h ^= *b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    t += 1;
                }
            }
            i += 1;
        }
        let eb = self.entries.to_le_bytes();
        let mut t = 0usize;
        while t < eb.len() {
            if let Some(b) = eb.get(t) {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            t += 1;
        }
        h
    }

    /// 封账（末版指纹；已封幂等）。
    pub fn seal(&mut self) -> u32 {
        if self.sealed {
            return self.seal_digest;
        }
        self.seal_digest = self.digest();
        self.sealed = true;
        self.seal_digest
    }

    /// 封账指纹校验（守恒 + 现值恒等；不符显性报）。
    pub fn verify_seal(&self) -> Result<(), V603cErr> {
        if !self.sealed {
            return Err(v603c_err(V603cCode::VersionLedgerMiss, "CmagicVersionLedger::verify_seal"));
        }
        if !self.conservation() {
            return Err(v603c_err(V603cCode::LedgerBroken, "CmagicVersionLedger::verify_seal"));
        }
        if self.digest() != self.seal_digest {
            return Err(v603c_err(V603cCode::VersionLedgerMiss, "CmagicVersionLedger::verify_seal"));
        }
        Ok(())
    }
}

/// c_magic 识别全守卫入口（段位 + 字段名唯一 + 往返一致抽样）。
pub fn cmagic_all_green() -> bool {
    v603c_segment_guard() && int_field_names_unique() && int_field_roundtrip(0xDEAD_BEEF)
}
