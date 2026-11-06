//! UNX-F50123 IEEE 1284.4 设备 ID 解析（MFG/MDL/CMD 逐键）（0x43b80 段）
//!
//! 锚点：UNX-F50123-J1 ID 解析 ×20 逐键一致，坏 ID 显式记账；判据锚定 §AI-63 §2 专题八。
//!
//! 1284 设备 ID 是打印链路里唯一一段「设备自称」的文本。它的危险不在于难解析，
//! 而在于**宽容**：现实设备会缺分号、混小写、塞进控制字符、把同一个键写两遍。
//! 只要解析器悄悄挑一个值，上层就会拿着一个来源不明的型号去做驱动匹配。
//! 因此本模块的纪律是「能解析的逐键落账，不能解析的带码入账」：
//!
//! * 长度前缀（2 字节大端）必须与实际长度**严格相等**，不做「取小者」的裁剪；
//! * 键限定 1..=8 个 ASCII 字母数字，大小写不敏感但**归一化为大写**并计数；
//! * 值限定 0x20..=0x7E、非空、≤128 字节；空值不是「未知」，是坏样本；
//! * 键重复、空对、缺分号、缺必需键（MFG/MDL/CMD）一律显式失败；
//! * 未知键不拒绝（前向兼容），但受 extras 容量约束，超限报 `Capacity`。
//!
//! 数据结构：
//! * `CmdSet` —— CMD 逗号表（保序、去重、超额显式计数、可复算摘要）；
//! * `DeviceId` —— 解析结果（MFG/MDL/CMD + 可选 DES/CLS/SN + extras + 记账位）；
//! * `IdLedger` —— 解析账本（好 ID 与坏 ID 都入账，坏 ID 带诊断码，重复 ID 拒绝）；
//! * `SAMPLES` —— 20 条真实形态的 ID 样本（J1 的「×20 逐键一致」分母）。
//!
//! 错误闭集：`SEG_BASE + n`（0x43b80 段），所有码值都被真实构造。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43b80 独占 0x20）。
pub const SEG_BASE: u32 = 0x43b80;

pub const E_EMPTY: u16 = 1;
pub const E_LEN_PREFIX: u16 = 2;
pub const E_NO_COLON: u16 = 3;
pub const E_KEY: u16 = 4;
pub const E_VALUE: u16 = 5;
pub const E_UNTERMINATED: u16 = 6;
pub const E_EMPTY_PAIR: u16 = 7;
pub const E_DUP_KEY: u16 = 8;
pub const E_CAPACITY: u16 = 9;
pub const E_MISSING_KEY: u16 = 10;
pub const E_CMD: u16 = 11;
pub const E_DUP_ID: u16 = 12;

/// 单条 ID 的最大键对数。IEEE 1284 只规定常见键，未限制对数；16 对足以覆盖
/// 真实设备（MFG/MDL/CMD/CLS/DES/SN + 少量厂商键），超出说明这不是设备 ID。
pub const MAX_PAIRS: usize = 16;

/// 厂商私有键（extras）容量。
pub const EXTRAS_CAP: usize = 8;

/// 单个值的最大字节数。128 是 1284 设备 ID 的常规上限（USB 打印类字符串
/// 描述符 255B 减去键/分隔开销后的保守值）。
pub const MAX_VALUE: usize = 128;

/// 键名最大长度。标准键均为 3 字符，留 8 个字符给厂商扩展键。
pub const MAX_KEY: usize = 8;

/// CMD 表容量。真实设备最多列 8 条命令语言，超出部分显式计数而不是静默截断。
pub const CMD_CAP: usize = 8;

/// 设备 ID 错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    Empty,
    LenPrefix,
    NoColon,
    Key,
    Value,
    Unterminated,
    EmptyPair,
    DupKey,
    Capacity,
    MissingKey,
    Cmd,
    DupId,
}

impl DeviceError {
    pub fn code(self) -> u32 {
        let n = match self {
            DeviceError::Empty => E_EMPTY,
            DeviceError::LenPrefix => E_LEN_PREFIX,
            DeviceError::NoColon => E_NO_COLON,
            DeviceError::Key => E_KEY,
            DeviceError::Value => E_VALUE,
            DeviceError::Unterminated => E_UNTERMINATED,
            DeviceError::EmptyPair => E_EMPTY_PAIR,
            DeviceError::DupKey => E_DUP_KEY,
            DeviceError::Capacity => E_CAPACITY,
            DeviceError::MissingKey => E_MISSING_KEY,
            DeviceError::Cmd => E_CMD,
            DeviceError::DupId => E_DUP_ID,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            DeviceError::Empty => "empty device id buffer",
            DeviceError::LenPrefix => "declared length prefix disagrees with buffer",
            DeviceError::NoColon => "key/value pair without ':'",
            DeviceError::Key => "key empty, too long or not alphanumeric",
            DeviceError::Value => "value empty, non-printable or too long",
            DeviceError::Unterminated => "last pair not terminated by ';'",
            DeviceError::EmptyPair => "empty pair between ';;'",
            DeviceError::DupKey => "same key appears twice",
            DeviceError::Capacity => "pair/extras/ledger capacity exceeded",
            DeviceError::MissingKey => "required key MFG/MDL/CMD absent",
            DeviceError::Cmd => "CMD list has an empty item",
            DeviceError::DupId => "same device id recorded twice",
        }
    }
}

/// 键值对（未知键原样收下，键已归一化为大写）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyVal {
    pub key: String,
    pub value: String,
}

/// CMD 逗号表。保序（设备声明顺序即优先级顺序）、去重、超额显式计数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CmdSet {
    items: Vec<String>,
    dups: u16,
    dropped: u16,
    raw_len: u16,
}

impl CmdSet {
    /// 解析逗号分隔的命令语言表。空项（`a,,b` 或末尾逗号）显式失败：
    /// 空项无法区分「设备漏写」与「解析器多切了一刀」。
    pub fn parse(v: &str) -> Result<CmdSet, DeviceError> {
        if v.is_empty() {
            return Err(DeviceError::Cmd);
        }
        let bytes = v.as_bytes();
        let mut items: Vec<String> = Vec::with_capacity(4);
        let mut dups = 0u16;
        let mut dropped = 0u16;
        let mut start = 0usize;
        let mut i = 0usize;
        while i <= bytes.len() {
            let at_end = i == bytes.len();
            let is_sep = at_end || bytes.get(i) == Some(&b',');
            if is_sep {
                let part = match v.get(start..i) {
                    Some(p) => trim_str(p),
                    None => return Err(DeviceError::Cmd),
                };
                if part.is_empty() {
                    return Err(DeviceError::Cmd);
                }
                if contains_str(&items, part) {
                    dups += 1;
                } else if items.len() < CMD_CAP {
                    items.push(part.to_string());
                } else {
                    dropped += 1;
                }
                start = i + 1;
            }
            i += 1;
        }
        Ok(CmdSet { items, dups, dropped, raw_len: v.len() as u16 })
    }

    /// 仅含一条命令的表（样本构造用）。
    pub fn single(cmd: &str) -> Result<CmdSet, DeviceError> {
        CmdSet::parse(cmd)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 保留的重复项条数（`PCL,PCL` → 1）。
    pub fn dups(&self) -> u16 {
        self.dups
    }

    /// 因容量被丢弃的条数（显式记账，不静默）。
    pub fn dropped(&self) -> u16 {
        self.dropped
    }

    pub fn raw_len(&self) -> u16 {
        self.raw_len
    }

    /// 按声明顺序取第 i 条。
    pub fn get(&self, i: usize) -> Option<&str> {
        match self.items.get(i) {
            Some(v) => Some(v.as_str()),
            None => None,
        }
    }

    /// 是否声明了某条命令语言。
    pub fn contains(&self, cmd: &str) -> bool {
        contains_str(&self.items, cmd)
    }

    /// 规范化文本（逗号连接，保持声明顺序）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(v) = self.items.get(i) {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(v);
            }
            i += 1;
        }
        out
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.items.len() as u64;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(v) = self.items.get(i) {
                h = fnv_str(h, v);
            }
            i += 1;
        }
        h
    }
}

fn contains_str(items: &[String], needle: &str) -> bool {
    let mut i = 0usize;
    while i < items.len() {
        if let Some(v) = items.get(i) {
            if v.as_str() == needle {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 解析结果。所有字段都是「设备说过的话」，不做任何补全或猜测。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceId {
    pub mfg: String,
    pub mdl: String,
    pub cmd: CmdSet,
    pub desc: Option<String>,
    pub class: Option<String>,
    pub serial: Option<String>,
    pub extras: Vec<KeyVal>,
    /// 实际出现的键对数（含 extras）。
    pub pair_count: u16,
    /// 以小写形式到达的键个数（已归一化，仅用于运维统计）。
    pub lower_keys: u16,
    /// 输入是否带 2 字节长度前缀。
    pub had_prefix: bool,
    /// 前缀声明的长度（无前缀为 0）。
    pub declared_len: u16,
}

impl DeviceId {
    /// 按键取值（MFG/MDL/DES/CLS/SN 与 extras）。
    pub fn key(&self, k: &str) -> Option<&str> {
        if k == "MFG" {
            return Some(self.mfg.as_str());
        }
        if k == "MDL" {
            return Some(self.mdl.as_str());
        }
        if k == "CMD" {
            // CMD 是表不是单值，这里返回规范化文本；逐条请走 `cmd`。
            return None;
        }
        if k == "DES" {
            return match &self.desc {
                Some(v) => Some(v.as_str()),
                None => None,
            };
        }
        if k == "CLS" {
            return match &self.class {
                Some(v) => Some(v.as_str()),
                None => None,
            };
        }
        if k == "SN" {
            return match &self.serial {
                Some(v) => Some(v.as_str()),
                None => None,
            };
        }
        let mut i = 0usize;
        while i < self.extras.len() {
            if let Some(kv) = self.extras.get(i) {
                if kv.key == k {
                    return Some(kv.value.as_str());
                }
            }
            i += 1;
        }
        None
    }

    /// 规范化文本。`render()` 的输出必须能被 `parse_device_id` 原样解析回
    /// 同一内容摘要（往返判据），因此这里只输出合法键值对、统一大写键、
    /// 以 `;` 收尾。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("MFG:");
        out.push_str(&self.mfg);
        out.push_str(";MDL:");
        out.push_str(&self.mdl);
        out.push_str(";CMD:");
        out.push_str(&self.cmd.render());
        out.push(';');
        if let Some(d) = &self.desc {
            out.push_str("DES:");
            out.push_str(d);
            out.push(';');
        }
        if let Some(c) = &self.class {
            out.push_str("CLS:");
            out.push_str(c);
            out.push(';');
        }
        if let Some(s) = &self.serial {
            out.push_str("SN:");
            out.push_str(s);
            out.push(';');
        }
        let mut i = 0usize;
        while i < self.extras.len() {
            if let Some(kv) = self.extras.get(i) {
                out.push_str(&kv.key);
                out.push(':');
                out.push_str(&kv.value);
                out.push(';');
            }
            i += 1;
        }
        out
    }

    /// 内容摘要（不含 had_prefix/lower_keys/declared_len 这类传输层记账位，
    /// 否则同一设备走两种封装会得到不同摘要，往返判据就失去意义）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_str(h, &self.mfg);
        h = fnv_str(h, &self.mdl);
        h ^= self.cmd.digest();
        h = h.wrapping_mul(0x100_0000_01b3);
        h = fnv_opt(h, &self.desc);
        h = fnv_opt(h, &self.class);
        h = fnv_opt(h, &self.serial);
        h = h.wrapping_mul(0x100_0000_01b3);
        h ^= self.extras.len() as u64;
        let mut i = 0usize;
        while i < self.extras.len() {
            if let Some(kv) = self.extras.get(i) {
                h = fnv_str(h, &kv.key);
                h = fnv_str(h, &kv.value);
            }
            i += 1;
        }
        h
    }
}

fn fnv_opt(mut h: u64, v: &Option<String>) -> u64 {
    match v {
        Some(s) => {
            h = fnv_str(h, s);
            fnv_u8(h, 1)
        }
        None => fnv_u8(h, 0),
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_str(mut h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            h = fnv_u8(h, *c);
        }
        i += 1;
    }
    fnv_u8(h, 0xff)
}

fn trim_u8(d: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = d.len();
    while a < b {
        match d.get(a) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' {
                    a += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    while b > a {
        match d.get(b - 1) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' {
                    b -= 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    match d.get(a..b) {
        Some(v) => v,
        None => &[],
    }
}

fn trim_str(s: &str) -> &str {
    let b = s.as_bytes();
    let mut a = 0usize;
    let mut z = b.len();
    while a < z {
        match b.get(a) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' {
                    a += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    while z > a {
        match b.get(z - 1) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' {
                    z -= 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    match s.get(a..z) {
        Some(v) => v,
        None => "",
    }
}

fn normalize_key(k: &[u8], lower: &mut u16) -> Result<String, DeviceError> {
    if k.is_empty() || k.len() > MAX_KEY {
        return Err(DeviceError::Key);
    }
    let mut s = String::with_capacity(k.len());
    let mut had_lower = false;
    let mut i = 0usize;
    while i < k.len() {
        let b = match k.get(i) {
            Some(v) => *v,
            None => return Err(DeviceError::Key),
        };
        let up = if b >= b'a' && b <= b'z' {
            had_lower = true;
            b - 32
        } else {
            b
        };
        let ok = (up >= b'A' && up <= b'Z') || (up >= b'0' && up <= b'9');
        if !ok {
            return Err(DeviceError::Key);
        }
        s.push(up as char);
        i += 1;
    }
    // 计的是「以小写到达的键个数」，不是小写字符个数：运维只关心有多少个键
    // 走了非标准大小写，逐字符计数会把 MFG 的三次出现当成三条告警。
    if had_lower {
        *lower = lower.saturating_add(1);
    }
    Ok(s)
}

fn normalize_value(v: &[u8]) -> Result<String, DeviceError> {
    if v.is_empty() || v.len() > MAX_VALUE {
        return Err(DeviceError::Value);
    }
    let mut s = String::with_capacity(v.len());
    let mut i = 0usize;
    while i < v.len() {
        let b = match v.get(i) {
            Some(x) => *x,
            None => return Err(DeviceError::Value),
        };
        if b < 0x20 || b > 0x7e {
            return Err(DeviceError::Value);
        }
        s.push(b as char);
        i += 1;
    }
    Ok(s)
}

/// 剥离 2 字节大端长度前缀。只有「首字节为 0」才按前缀解释，且声明长度必须
/// 与剩余字节数**严格相等**——真实 1284 响应就是这个形状，短一个字节都说明
/// 缓冲被截断。
fn strip_prefix(raw: &[u8]) -> Result<(&[u8], bool, u16), DeviceError> {
    if raw.is_empty() {
        return Err(DeviceError::Empty);
    }
    let first_zero = raw.first() == Some(&0u8);
    if !first_zero {
        return Ok((raw, false, 0));
    }
    if raw.len() < 2 {
        return Err(DeviceError::LenPrefix);
    }
    let hi = match raw.first() {
        Some(v) => *v as u16,
        None => 0,
    };
    let lo = match raw.get(1) {
        Some(v) => *v as u16,
        None => 0,
    };
    let declared = (hi << 8) | lo;
    let rest = raw.len().saturating_sub(2);
    if declared == 0 || declared as usize != rest {
        return Err(DeviceError::LenPrefix);
    }
    let body = match raw.get(2..) {
        Some(v) => v,
        None => return Err(DeviceError::LenPrefix),
    };
    Ok((body, true, declared))
}

/// 解析一条设备 ID。
///
/// 顺序：空缓冲 → 前缀 → 逐对切分（每对必须有 `:` 与结尾 `;`）→ 键/值规范化
/// → 重复键 → 必需键。失败即返回具体码，调用方（账本）原样入账。
pub fn parse_device_id(raw: &[u8]) -> Result<DeviceId, DeviceError> {
    let (body_raw, had_prefix, declared) = strip_prefix(raw)?;
    let body = trim_u8(body_raw);
    if body.is_empty() {
        return Err(DeviceError::Empty);
    }
    let mut mfg: Option<String> = None;
    let mut mdl: Option<String> = None;
    let mut cmd: Option<CmdSet> = None;
    let mut desc: Option<String> = None;
    let mut class: Option<String> = None;
    let mut serial: Option<String> = None;
    let mut extras: Vec<KeyVal> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut pair_count = 0u16;
    let mut lower_keys = 0u16;
    let mut pos = 0usize;

    while pos < body.len() {
        // 找分号；找不到说明最后一对没有终止符。
        let mut end = pos;
        while end < body.len() {
            if body.get(end) == Some(&b';') {
                break;
            }
            end += 1;
        }
        if end >= body.len() {
            return Err(DeviceError::Unterminated);
        }
        let chunk = match body.get(pos..end) {
            Some(v) => v,
            None => return Err(DeviceError::Unterminated),
        };
        let pair = trim_u8(chunk);
        if pair.is_empty() {
            return Err(DeviceError::EmptyPair);
        }
        pair_count = pair_count.saturating_add(1);
        if pair_count as usize > MAX_PAIRS {
            return Err(DeviceError::Capacity);
        }
        // 切 ':'
        let mut colon: Option<usize> = None;
        let mut i = 0usize;
        while i < pair.len() {
            if pair.get(i) == Some(&b':') {
                colon = Some(i);
                break;
            }
            i += 1;
        }
        let ci = match colon {
            Some(v) => v,
            None => return Err(DeviceError::NoColon),
        };
        let key_raw = trim_u8(match pair.get(0..ci) {
            Some(v) => v,
            None => return Err(DeviceError::Key),
        });
        let val_raw = trim_u8(match pair.get(ci + 1..) {
            Some(v) => v,
            None => return Err(DeviceError::Value),
        });
        let key = normalize_key(key_raw, &mut lower_keys)?;
        let value = normalize_value(val_raw)?;

        // 重复键：即使是未知键也必须拦。`MFG:a;MFG:b` 里谁是厂商没有答案。
        let mut dup = false;
        let mut k = 0usize;
        while k < keys.len() {
            if let Some(existing) = keys.get(k) {
                if existing == &key {
                    dup = true;
                    break;
                }
            }
            k += 1;
        }
        if dup {
            return Err(DeviceError::DupKey);
        }
        keys.push(key.clone());

        if key == "MFG" {
            mfg = Some(value);
        } else if key == "MDL" {
            mdl = Some(value);
        } else if key == "CMD" {
            cmd = Some(CmdSet::parse(&value)?);
        } else if key == "DES" {
            desc = Some(value);
        } else if key == "CLS" {
            class = Some(value);
        } else if key == "SN" {
            serial = Some(value);
        } else if extras.len() < EXTRAS_CAP {
            extras.push(KeyVal { key, value });
        } else {
            // 未知键超容：显式失败。静默丢弃会让人以为「设备没报这个键」。
            return Err(DeviceError::Capacity);
        }
        pos = end + 1;
    }

    let mfg = match mfg {
        Some(v) => v,
        None => return Err(DeviceError::MissingKey),
    };
    let mdl = match mdl {
        Some(v) => v,
        None => return Err(DeviceError::MissingKey),
    };
    let cmd = match cmd {
        Some(v) => v,
        None => return Err(DeviceError::MissingKey),
    };
    Ok(DeviceId {
        mfg,
        mdl,
        cmd,
        desc,
        class,
        serial,
        extras,
        pair_count,
        lower_keys,
        had_prefix,
        declared_len: declared,
    })
}

/// 文本便捷入口。
pub fn parse_str(s: &str) -> Result<DeviceId, DeviceError> {
    parse_device_id(s.as_bytes())
}

/// 给正文加 2 字节大端长度前缀（构造 1284 原始响应用）。正文超 65535 返回 None。
pub fn with_prefix(body: &[u8]) -> Option<Vec<u8>> {
    if body.len() > 0xffff {
        return None;
    }
    let mut v = Vec::with_capacity(body.len() + 2);
    v.push(((body.len() >> 8) & 0xff) as u8);
    v.push((body.len() & 0xff) as u8);
    v.extend_from_slice(body);
    Some(v)
}

// ---------------------------------------------------------------------------
// 解析账本：好 ID 与坏 ID 都入账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdEntry {
    pub raw_digest: u64,
    pub ok: bool,
    /// 成功为 0，失败为 `DeviceError::code()` 的低位偏移量（1..=12）。
    pub code_n: u16,
    pub pairs: u16,
}

/// 解析账本。重复 ID 显式拒绝：同一台设备在枚举里出现两次，说明上层
/// 去重逻辑坏了，记两条会让「20 台设备」失去计量意义。
pub struct IdLedger {
    entries: Vec<IdEntry>,
    cap: usize,
    dups: u16,
    rejected: u16,
}

impl IdLedger {
    pub fn new(cap: usize) -> Result<IdLedger, DeviceError> {
        if cap == 0 || cap > 64 {
            return Err(DeviceError::Capacity);
        }
        Ok(IdLedger { entries: Vec::with_capacity(cap), cap, dups: 0, rejected: 0 })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn rejected(&self) -> u16 {
        self.rejected
    }

    pub fn dups(&self) -> u16 {
        self.dups
    }

    pub fn get(&self, i: usize) -> Option<&IdEntry> {
        self.entries.get(i)
    }

    /// 入账并返回内容摘要（失败时返回错误，但错误**已经**记在账上）。
    pub fn record(&mut self, raw: &[u8]) -> Result<u64, DeviceError> {
        let raw_digest = fnv_bytes(0xcbf2_9ce4_8422_2325, raw);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.raw_digest == raw_digest {
                    self.dups += 1;
                    return Err(DeviceError::DupId);
                }
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(DeviceError::Capacity);
        }
        let parsed = parse_device_id(raw);
        match parsed {
            Ok(id) => {
                self.entries.push(IdEntry {
                    raw_digest,
                    ok: true,
                    code_n: 0,
                    pairs: id.pair_count,
                });
                Ok(id.digest())
            }
            Err(e) => {
                self.rejected += 1;
                self.entries.push(IdEntry {
                    raw_digest,
                    ok: false,
                    code_n: code_offset(e),
                    pairs: 0,
                });
                Err(e)
            }
        }
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账本摘要（序号 + 原始摘要 + 成败 + 码偏移 + 键对数）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.entries.len() as u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h ^= e.raw_digest;
                h = h.wrapping_mul(0x100_0000_01b3);
                h = fnv_u8(h, if e.ok { 1 } else { 0 });
                h = fnv_u8(h, (e.code_n & 0xff) as u8);
                h = fnv_u8(h, (e.pairs & 0xff) as u8);
            }
            i += 1;
        }
        h
    }
}

fn fnv_bytes(mut h: u64, d: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < d.len() {
        if let Some(b) = d.get(i) {
            h = fnv_u8(h, *b);
        }
        i += 1;
    }
    fnv_u8(h, d.len() as u8)
}

fn code_offset(e: DeviceError) -> u16 {
    (e.code() - SEG_BASE) as u16
}

// ---------------------------------------------------------------------------
// 20 条样本（真实形态：MFG/MDL/CMD 必有，DES/CLS/SN/extras 可选，
// 部分样本带 2 字节长度前缀）
// ---------------------------------------------------------------------------

pub struct IdSample {
    pub body: &'static [u8],
    pub prefixed: bool,
    pub mfg: &'static str,
    pub mdl: &'static str,
    pub cmd_count: u8,
}

pub const SAMPLES: [IdSample; 20] = [
    IdSample { body: b"MFG:Acme;MDL:LaserJet 400;CMD:PJL,PCL,MLC;CLS:PRINTER;DES:Acme LaserJet 400;", prefixed: false, mfg: "Acme", mdl: "LaserJet 400", cmd_count: 3 },
    IdSample { body: b"MFG:Brother;MDL:HL-2270DW;CMD:PJL,PCL;CLS:PRINTER;", prefixed: false, mfg: "Brother", mdl: "HL-2270DW", cmd_count: 2 },
    IdSample { body: b"MFG:Canon;MDL:iR-ADV C5535;CMD:UFRII,PS3;CLS:PRINTER;DES:imageRUNNER;", prefixed: false, mfg: "Canon", mdl: "iR-ADV C5535", cmd_count: 2 },
    IdSample { body: b"MFG:epson;MDL:WF-3620;CMD:ESCPL2,BDC;CLS:PRINTER;", prefixed: false, mfg: "epson", mdl: "WF-3620", cmd_count: 2 },
    IdSample { body: b"MFG:HP;MDL:OfficeJet Pro 8600;CMD:PCL3GUI,PJL;SN:CN12345678;", prefixed: false, mfg: "HP", mdl: "OfficeJet Pro 8600", cmd_count: 2 },
    IdSample { body: b"MFG:Konica;MDL:bizhub C368;CMD:PCL,PS3,PDF;CLS:PRINTER;", prefixed: false, mfg: "Konica", mdl: "bizhub C368", cmd_count: 3 },
    IdSample { body: b"MFG:Kyocera;MDL:ECOSYS P2040dn;CMD:PCL6,PJL;", prefixed: false, mfg: "Kyocera", mdl: "ECOSYS P2040dn", cmd_count: 2 },
    IdSample { body: b"MFG:Lexmark;MDL:MS417dn;CMD:PCL5,PCL6,PJL;DES:Lexmark MS417;", prefixed: false, mfg: "Lexmark", mdl: "MS417dn", cmd_count: 3 },
    IdSample { body: b"MFG:OKI;MDL:C332dn;CMD:PCL,PS;CLS:PRINTER;", prefixed: false, mfg: "OKI", mdl: "C332dn", cmd_count: 2 },
    IdSample { body: b"MFG:Ricoh;MDL:SP 4510DN;CMD:PCL5C,PCLXL,PJL;CLS:PRINTER;", prefixed: false, mfg: "Ricoh", mdl: "SP 4510DN", cmd_count: 3 },
    IdSample { body: b"MFG:Samsung;MDL:ProXpress M4020;CMD:SPL,PCL6;CLS:PRINTER;", prefixed: false, mfg: "Samsung", mdl: "ProXpress M4020", cmd_count: 2 },
    IdSample { body: b"MFG:Sharp;MDL:MX-5140;CMD:PCL6,PS3;CLS:PRINTER;", prefixed: false, mfg: "Sharp", mdl: "MX-5140", cmd_count: 2 },
    IdSample { body: b"MFG:Toshiba;MDL:e-STUDIO 3505AC;CMD:PCL,PS3,PDF;CLS:PRINTER;", prefixed: false, mfg: "Toshiba", mdl: "e-STUDIO 3505AC", cmd_count: 3 },
    IdSample { body: b"MFG:Xerox;MDL:VersaLink C405;CMD:PCL5E,PCL6,PJL;DES:VersaLink;", prefixed: false, mfg: "Xerox", mdl: "VersaLink C405", cmd_count: 3 },
    IdSample { body: b"MFG:Dell;MDL:C1760dn;CMD:PCL,PS;CLS:PRINTER;", prefixed: true, mfg: "Dell", mdl: "C1760dn", cmd_count: 2 },
    IdSample { body: b"MFG:Fuji;MDL:ApeosPort-V 5070;CMD:PCL6,PS3,PDF;CLS:PRINTER;", prefixed: true, mfg: "Fuji", mdl: "ApeosPort-V 5070", cmd_count: 3 },
    IdSample { body: b"MFG:Minolta;MDL:magicolor 4750DN;CMD:PCL,PS3;DES:magicolor;", prefixed: true, mfg: "Minolta", mdl: "magicolor 4750DN", cmd_count: 2 },
    IdSample { body: b"MFG:Panasonic;MDL:KX-MB2025;CMD:PCL5,PJL;SN:PA0001;", prefixed: false, mfg: "Panasonic", mdl: "KX-MB2025", cmd_count: 2 },
    IdSample { body: b"MFG:Zebra;MDL:ZT410;CMD:ZPL,ZPLII;CLS:PRINTER;", prefixed: false, mfg: "Zebra", mdl: "ZT410", cmd_count: 2 },
    IdSample { body: b"MFG:Citizen;MDL:CT-S310;CMD:ESC/POS;", prefixed: false, mfg: "Citizen", mdl: "CT-S310", cmd_count: 1 },
];

/// 第 i 条样本的字节流（无前缀样本原样返回；越界返回 None）。
pub fn sample_bytes(i: usize) -> Option<Vec<u8>> {
    let s = SAMPLES.get(i)?;
    if s.prefixed {
        with_prefix(s.body)
    } else {
        let mut v = Vec::with_capacity(s.body.len());
        v.extend_from_slice(s.body);
        Some(v)
    }
}

// ---------------------------------------------------------------------------
// 注入核：十二类坏 ID 必须全部被拒并给出具体码
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    if parse_device_id(b"").err() == Some(DeviceError::Empty) {
        caught += 1;
    }
    // 前缀声明 5 字节，实际只有 1 字节
    if parse_device_id(b"\x00\x05M").err() == Some(DeviceError::LenPrefix) {
        caught += 1;
    }
    if parse_device_id(b"MFGx;").err() == Some(DeviceError::NoColon) {
        caught += 1;
    }
    if parse_device_id(b":x;").err() == Some(DeviceError::Key) {
        caught += 1;
    }
    if parse_device_id(b"MFG:ab\x01cd;").err() == Some(DeviceError::Value) {
        caught += 1;
    }
    if parse_device_id(b"MFG:x").err() == Some(DeviceError::Unterminated) {
        caught += 1;
    }
    if parse_device_id(b"MFG:x;;MDL:y;").err() == Some(DeviceError::EmptyPair) {
        caught += 1;
    }
    if parse_device_id(b"MFG:a;MFG:b;MDL:m;CMD:c;").err() == Some(DeviceError::DupKey) {
        caught += 1;
    }
    if parse_device_id(b"MDL:m;CMD:c;").err() == Some(DeviceError::MissingKey) {
        caught += 1;
    }
    if parse_device_id(b"MFG:a;MDL:m;CMD:x,,y;").err() == Some(DeviceError::Cmd) {
        caught += 1;
    }
    // 17 对键，超过 MAX_PAIRS
    let long = b"MFG:a;MDL:m;CMD:c;K01:1;K02:2;K03:3;K04:4;K05:5;K06:6;K07:7;K08:8;K09:9;K10:1;K11:2;K12:3;K13:4;K14:5;";
    if parse_device_id(long).err() == Some(DeviceError::Capacity) {
        caught += 1;
    }
    // 未知键 9 个，超过 EXTRAS_CAP
    let extras = b"MFG:a;MDL:m;CMD:c;K01:1;K02:2;K03:3;K04:4;K05:5;K06:6;K07:7;K08:8;K09:9;";
    if parse_device_id(extras).err() == Some(DeviceError::Capacity) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50123_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50123");

    // J1-1 20 条样本全部解析成功，且 MFG/MDL/CMD 三条逐键一致（60/60）
    let mut parsed: Vec<DeviceId> = Vec::with_capacity(SAMPLES.len());
    let mut matched = 0u16;
    let mut compared = 0u16;
    let mut i = 0usize;
    while i < SAMPLES.len() {
        let ok = match (sample_bytes(i), SAMPLES.get(i)) {
            (Some(b), Some(exp)) => match parse_device_id(&b) {
                Ok(id) => {
                    let mut m = 0u16;
                    if id.mfg == exp.mfg {
                        m += 1;
                    }
                    if id.mdl == exp.mdl {
                        m += 1;
                    }
                    if id.cmd.len() == exp.cmd_count as usize {
                        m += 1;
                    }
                    matched += m;
                    compared += 3;
                    parsed.push(id);
                    m == 3
                }
                Err(_) => false,
            },
            _ => false,
        };
        if !ok {
            i += 1;
            continue;
        }
        i += 1;
    }
    let ok = parsed.len() == SAMPLES.len() && matched == 60 && compared == 60;
    let det = format!("{}/{} (ids={})", matched, compared, parsed.len());
    s.add_dyn("50123-J1-逐键一致", ok, &det);

    // J1-2 前缀封装与裸文本解析出同一内容摘要（同一设备的两种封装）
    let mut rt = 0usize;
    let mut k = 0usize;
    while k < SAMPLES.len() {
        if let Some(exp) = SAMPLES.get(k) {
            let raw = parse_device_id(exp.body);
            let pfx = match with_prefix(exp.body) {
                Some(v) => parse_device_id(&v),
                None => return s,
            };
            if let (Ok(a), Ok(b)) = (raw, pfx) {
                if a.digest() == b.digest() && b.had_prefix != a.had_prefix {
                    rt += 1;
                }
            }
        }
        k += 1;
    }
    let ok = rt == SAMPLES.len();
    let det = format!("rt={}/{}", rt, SAMPLES.len());
    s.add_dyn("50123-J1-前缀往返", ok, &det);

    let first = match parsed.first() {
        Some(v) => v.clone(),
        None => return s,
    };

    // J1-3 解析确定性：同字节两次解析内容摘要一致
    let again = parse_device_id(match SAMPLES.first() {
        Some(v) => v.body,
        None => &[],
    });
    let ok = match again {
        Ok(v) => v.digest() == first.digest() && v == first,
        Err(_) => false,
    };
    s.add("50123-J1-解析确定", ok, if ok { "" } else { "同字节摘要漂移" });

    // J1-4 规范化文本往返：render → parse 摘要一致（20 条全覆盖）
    let mut ok = true;
    let mut j = 0usize;
    while j < parsed.len() {
        if let Some(id) = parsed.get(j) {
            match parse_str(&id.render()) {
                Ok(back) => {
                    if back.digest() != id.digest() || back.pair_count != id.pair_count {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        j += 1;
    }
    s.add("50123-J1-往返幂等", ok, if ok { "" } else { "render 不可回读" });

    // J1-5 CMD 表：保序、去重、按声明顺序取
    let ordered = first.cmd.get(0) == Some("PJL")
        && first.cmd.get(1) == Some("PCL")
        && first.cmd.get(2) == Some("MLC")
        && first.cmd.len() == 3
        && first.cmd.contains("PCL")
        && !first.cmd.contains("PS3")
        && first.cmd.render() == "PJL,PCL,MLC";
    s.add("50123-J1-CMD保序", ordered, if ordered { "" } else { "CMD 顺序/去重错" });

    // J1-6 CMD 去重显式计数
    let dupcmd = parse_str("MFG:a;MDL:m;CMD:PCL,PCL,PJL,PCL;");
    let ok = match dupcmd {
        Ok(id) => id.cmd.len() == 2 && id.cmd.dups() == 2 && id.cmd.get(0) == Some("PCL"),
        Err(_) => false,
    };
    s.add("50123-J1-CMD去重", ok, if ok { "" } else { "重复命令未记账" });

    // J1-7 未知键进 extras，按到达顺序保存，键对数含 extras
    let ext = parse_str("MFG:a;MDL:m;CMD:c;SN12345:v1;VNDR:Acme;");
    let ok = match ext {
        Ok(id) => {
            id.pair_count == 5
                && id.extras.len() == 2
                && id.extras.get(0).map(|kv| kv.key.as_str()) == Some("SN12345")
                && id.key("VNDR") == Some("Acme")
                && id.key("MFG") == Some("a")
                && id.key("CMD").is_none()
        }
        Err(_) => false,
    };
    s.add("50123-J1-私有键", ok, if ok { "" } else { "extras 顺序/查找错" });

    // J1-8 小写键归一化并计数（真实设备常写小写）
    let lower = parse_str("mfg:a;mdl:m;cmd:PCL;");
    let ok = match lower {
        Ok(id) => id.mfg == "a" && id.mdl == "m" && id.lower_keys == 3 && id.pair_count == 3,
        Err(_) => false,
    };
    s.add("50123-J1-小写归一", ok, if ok { "" } else { "小写键未归一" });

    // J1-9 空白容错：键与值两侧空格/制表符被裁掉，内容不变
    let spaced = parse_str("  MFG : a ;\tMDL: m ; CMD : PCL , PS ;  ");
    let ok = match spaced {
        Ok(id) => id.mfg == "a" && id.mdl == "m" && id.cmd.len() == 2 && id.cmd.get(1) == Some("PS"),
        Err(_) => false,
    };
    s.add("50123-J1-空白容错", ok, if ok { "" } else { "空白裁剪不全" });

    // J1-10 值长度边界：128 接受、129 拒绝；非打印字符拒绝
    let v128 = repeat_byte(b'v', 128);
    let v129 = repeat_byte(b'v', 129);
    let ok128 = build_id(&v128).as_deref().map(parse_str).map(|r| r.is_ok()) == Some(true);
    let ok129 = build_id(&v129)
        .as_deref()
        .map(parse_str)
        .map(|r| r.err() == Some(DeviceError::Value))
        == Some(true);
    let ok = ok128 && ok129 && parse_str("MFG:a;MDL:m;CMD:c;DES:bad\x7f;").err() == Some(DeviceError::Value);
    s.add("50123-J1-值边界", ok, if ok { "" } else { "值长度/字符集失守" });

    // J1-11 坏 ID 全部显式入账：账本里 ok=0 的条目带具体码偏移
    let mut ledger = match IdLedger::new(40) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let bad_inputs: [&[u8]; 8] = [
        b"",
        b"\x00\x09M",
        b"MFGx;",
        b":x;",
        b"MFG:ab\x01cd;",
        b"MFG:x",
        b"MFG:x;;MDL:y;",
        b"MFG:a;MFG:b;MDL:m;CMD:c;",
    ];
    let mut bi = 0usize;
    let mut bad_recorded = 0u16;
    while bi < bad_inputs.len() {
        if let Some(raw) = bad_inputs.get(bi) {
            let _ = ledger.record(raw);
        }
        bi += 1;
    }
    let mut ei = 0usize;
    let mut codes_ok = true;
    while ei < ledger.len() {
        if let Some(e) = ledger.get(ei) {
            if !e.ok && e.code_n > 0 && e.code_n <= E_DUP_ID {
                bad_recorded += 1;
            } else if e.ok {
                codes_ok = false;
            }
        }
        ei += 1;
    }
    let ok = bad_recorded == 8 && ledger.rejected() == 8 && ledger.ok_count() == 0 && codes_ok;
    let det = format!("bad={} rej={}", bad_recorded, ledger.rejected());
    s.add_dyn("50123-J1-坏ID入账", ok, &det);

    // J1-12 好 ID 入账 ×20，账本拒绝重复 ID
    let mut good = match IdLedger::new(40) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut rec = 0usize;
    let mut m = 0usize;
    while m < SAMPLES.len() {
        if let Some(bytes) = sample_bytes(m) {
            if good.record(&bytes).is_ok() {
                rec += 1;
            }
        }
        m += 1;
    }
    let dup = match SAMPLES.first() {
        Some(v) => good.record(v.body),
        None => Ok(0),
    };
    let ok = rec == 20 && good.len() == 20 && good.ok_count() == 20 && dup.err() == Some(DeviceError::DupId);
    let det = format!("rec={} len={} dup={:?}", rec, good.len(), dup.err());
    s.add_dyn("50123-J1-账本去重", ok, &det);

    // J1-13 账本容量/参数边界，且重复 ID 被拒时账本摘要不变（拒绝无副作用）
    let d0 = good.digest();
    let dup2 = match SAMPLES.first() {
        Some(v) => good.record(v.body),
        None => Ok(0),
    };
    let ok = IdLedger::new(0).err() == Some(DeviceError::Capacity)
        && IdLedger::new(65).err() == Some(DeviceError::Capacity)
        && dup2.err() == Some(DeviceError::DupId)
        && good.digest() == d0;
    s.add("50123-J1-账本边界", ok, if ok { "" } else { "账本容量失守" });

    // J1-14 含长度前缀的 ID 声明长度回写正确
    let ok = match SAMPLES.get(14) {
        Some(v) => match sample_bytes(14) {
            Some(bytes) => match parse_device_id(&bytes) {
                Ok(id) => id.had_prefix && id.declared_len as usize == v.body.len(),
                Err(_) => false,
            },
            None => false,
        },
        None => false,
    };
    s.add("50123-J1-前缀长度", ok, if ok { "" } else { "前缀长度记账错" });

    // J1-15 缺必需键分别报 MissingKey（MFG/MDL/CMD 三缺一）
    let ok = parse_str("MDL:m;CMD:c;").err() == Some(DeviceError::MissingKey)
        && parse_str("MFG:a;CMD:c;").err() == Some(DeviceError::MissingKey)
        && parse_str("MFG:a;MDL:m;").err() == Some(DeviceError::MissingKey);
    s.add("50123-J1-必需键", ok, if ok { "" } else { "缺键被放行" });

    // J1-16 注入核：十二类坏 ID 全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50123-J1-注入必抓", ok, &det);

    s
}

fn repeat_byte(b: u8, n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(b);
        i += 1;
    }
    v
}

fn build_id(value: &[u8]) -> Option<String> {
    let mut s = String::new();
    s.push_str("MFG:a;MDL:m;CMD:c;DES:");
    let mut i = 0usize;
    while i < value.len() {
        if let Some(b) = value.get(i) {
            if *b < 0x20 || *b > 0x7e {
                return None;
            }
            s.push(*b as char);
        }
        i += 1;
    }
    s.push(';');
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50123_checks_green() {
        let set = run_unx50123_checks();
        assert!(set.all_passed(), "UNX-F50123 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50123_twenty_samples_match() {
        let mut i = 0usize;
        while i < SAMPLES.len() {
            let bytes = sample_bytes(i);
            assert!(bytes.is_some());
            if let (Some(b), Some(exp)) = (bytes, SAMPLES.get(i)) {
                let id = parse_device_id(&b);
                assert!(id.is_ok());
                if let Ok(id) = id {
                    assert_eq!(id.mfg, exp.mfg);
                    assert_eq!(id.mdl, exp.mdl);
                    assert_eq!(id.cmd.len(), exp.cmd_count as usize);
                }
            }
            i += 1;
        }
    }

    #[test]
    fn unx50123_bad_ids_are_recorded() {
        let mut l = IdLedger::new(8);
        assert!(l.is_ok());
        if let Ok(mut l) = l {
            assert_eq!(l.record(b"MFG:a;MDL:m;CMD:c;PCL;").err(), Some(DeviceError::NoColon));
            assert_eq!(l.rejected(), 1);
            assert_eq!(l.len(), 1);
        }
    }

    #[test]
    fn unx50123_error_codes_in_segment() {
        assert_eq!(DeviceError::Empty.code(), SEG_BASE + 1);
        assert_eq!(DeviceError::DupId.code(), SEG_BASE + 12);
    }
}
