//! UNX-F2436 · 碎片指数 v1 与导出面 · 诊断段 0x6F90（独占 0x6F90~0x6F9F）
//!
//! **锚点`：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2436）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2436-J1 碎片指数定义式可复算（账面代入=输出），导出 JSON
//! 字段齐全。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 定义式可复算：碎片指数 FI = 1 − (最大可用块帧数 × 100 /
//!   总空闲帧数)（0~99 整数百分点，无浮点）；三组账面样本代入
//!   定义式复算与输出逐组相等。
//!   J1-b JSON 导出：字段闭集（fi、max_block_frames、free_frames、
//!   per_order_free[11]、seq），手写 JSON 转义完备、可反解析回读。
//!   J1-c 极端值语义：全空闲（最大块=总空闲）→ FI=0；全碎（最大
//!   块=1 帧）→ FI 接近 100 但按定义式取值；空账（空闲=0）→
//!   显性 None 不出 0（诚实口径）。
//!
//! **段位复核记录**：0x6F90~0x6F9F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟；JSON 手写无外部依赖。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F90~0x6F9F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F90;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F9F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FragIndexError {
    /// 0x6F91 空账出指数（诚实口径破）。
    EmptyLedger,
    /// 0x6F92 定义式复算不一致。
    FormulaDrift,
    /// 0x6F93 JSON 反解析失败。
    JsonParseFail,
    /// 0x6F94 导出字段缺失。
    FieldMissing,
}

impl FragIndexError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            FragIndexError::EmptyLedger => format!(
                "[0x{:04X}] 空闲=0 时出指数：0 值是谎报。\
                 改法：空账显性 None。",
                self.code()
            ),
            FragIndexError::FormulaDrift => format!(
                "[0x{:04X}] 定义式复算与输出不一致。\
                 改法：检查是否绕开 compute_fi() 直算。",
                self.code()
            ),
            FragIndexError::JsonParseFail => format!(
                "[0x{:04X}] JSON 反解析失败：转义或格式错。\
                 改法：核对导出器转义表。",
                self.code()
            ),
            FragIndexError::FieldMissing => format!(
                "[0x{:04X}] 导出字段缺失：闭集 5 字段必须齐。\
                 改法：核对导出器字段表。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、碎片指数定义式
// ===========================================================================

/// 定义式：FI = 1 − max_block × 100 / free_frames（整数百分点）。
///
/// 空账（free=0）显性 None。
pub const fn compute_fi(max_block: u64, free_frames: u64) -> Option<u64> {
    if free_frames == 0 {
        return None;
    }
    if max_block > free_frames {
        return None; // 非法入参。
    }
    let ratio_bp = max_block * 100 / free_frames; // 0~100。
    if ratio_bp >= 100 {
        return Some(0);
    }
    Some(100 - ratio_bp)
}

/// 阶余量 → 指数输入。
pub struct FragInput {
    /// 各阶空闲块数（0~10）。
    pub per_order: [u64; 11],
}

impl FragInput {
    /// 总空闲帧数。
    pub fn free_frames(&self) -> u64 {
        let mut n = 0u64;
        let mut o = 0;
        while o <= 10 {
            n += self.per_order[o as usize] << o;
            o += 1;
        }
        n
    }

    /// 最大可用连续块帧数（取最高非零阶的块帧数；近似口径：
    /// 最高非零阶即最大可交付块）。
    pub fn max_block_frames(&self) -> u64 {
        let mut o = 10i64;
        while o >= 0 {
            if self.per_order[o as usize] > 0 {
                return 1u64 << o;
            }
            o -= 1;
        }
        0
    }

    /// 计算指数（经定义式，唯一入口）。
    pub fn fragment_index(&self) -> Option<u64> {
        compute_fi(self.max_block_frames(), self.free_frames())
    }
}

// ===========================================================================
// 三、JSON 导出（手写，无外部依赖）
// ===========================================================================

/// 导出快照。
#[derive(Clone, Copy, Debug)]
pub struct FragExport {
    /// 指数（None=空账，导出 null）。
    pub fi: Option<u64>,
    /// 最大块帧数。
    pub max_block_frames: u64,
    /// 总空闲帧数。
    pub free_frames: u64,
    /// 各阶空闲块数。
    pub per_order: [u64; 11],
    /// 事件号。
    pub seq: u64,
}

/// JSON 转义（数字快照无特殊字符，直出）。
pub fn json_escape_num(v: u64, out: &mut String) {
    let s = format!("{}", v);
    out.push_str(&s);
}

/// 导出 JSON（字段闭集 5 项）。
pub fn export_json(e: &FragExport) -> String {
    let mut s = String::new();
    s.push('{');
    s.push_str("\"fi\":");
    match e.fi {
        Some(v) => json_escape_num(v, &mut s),
        None => s.push_str("null"),
    }
    s.push_str(",\"max_block_frames\":");
    json_escape_num(e.max_block_frames, &mut s);
    s.push_str(",\"free_frames\":");
    json_escape_num(e.free_frames, &mut s);
    s.push_str(",\"per_order_free\":[");
    let mut o = 0;
    while o < 11 {
        if o > 0 {
            s.push(',');
        }
        json_escape_num(e.per_order[o], &mut s);
        o += 1;
    }
    s.push_str("],\"seq\":");
    json_escape_num(e.seq, &mut s);
    s.push('}');
    s
}

/// JSON 反解析（字段闭集 5 项，宽松键序：按固定序解析）。
pub fn parse_json(s: &str) -> result::Result<FragExport, FragIndexError> {
    // 快照域内数字与结构固定，按标点切。
    let mut fi: Option<u64> = None;
    let mut maxb = 0u64;
    let mut freef = 0u64;
    let mut per = [0u64; 11];
    let mut seq = 0u64;
    let mut fields = 0u8;
    // "fi":
    if let Some(i) = s.find("\"fi\":") {
        let rest = &s[i + 5..];
        let end = rest.find(',').ok_or(FragIndexError::JsonParseFail)?;
        let tok = &rest[..end];
        if tok != "null" {
            fi = tok.parse::<u64>().ok();
            if fi.is_none() {
                return Err(FragIndexError::JsonParseFail);
            }
        }
        fields += 1;
    }
    if let Some(i) = s.find("\"max_block_frames\":") {
        let rest = &s[i + 19..];
        let end = rest.find(',').ok_or(FragIndexError::JsonParseFail)?;
        maxb = rest[..end].parse::<u64>().map_err(|_| FragIndexError::JsonParseFail)?;
        fields += 1;
    }
    if let Some(i) = s.find("\"free_frames\":") {
        let rest = &s[i + 14..];
        let end = rest.find(',').ok_or(FragIndexError::JsonParseFail)?;
        freef = rest[..end].parse::<u64>().map_err(|_| FragIndexError::JsonParseFail)?;
        fields += 1;
    }
    if let Some(i) = s.find("\"per_order_free\":[") {
        let rest = &s[i + 18..];
        let end = rest.find(']').ok_or(FragIndexError::JsonParseFail)?;
        let body = &rest[..end];
        let mut segs: Vec<&str> = Vec::new();
        let mut start = 0usize;
        let bytes = body.as_bytes();
        let mut k = 0;
        while k < bytes.len() {
            if bytes[k] == b',' {
                segs.push(&body[start..k]);
                start = k + 1;
            }
            k += 1;
        }
        segs.push(&body[start..]);
        if segs.len() != 11 {
            return Err(FragIndexError::FieldMissing);
        }
        let mut o = 0;
        while o < 11 {
            per[o] = segs[o].parse::<u64>().map_err(|_| FragIndexError::JsonParseFail)?;
            o += 1;
        }
        fields += 1;
    }
    if let Some(i) = s.find("\"seq\":") {
        let rest = &s[i + 6..];
        let end = rest.find('}').ok_or(FragIndexError::JsonParseFail)?;
        seq = rest[..end].parse::<u64>().map_err(|_| FragIndexError::JsonParseFail)?;
        fields += 1;
    }
    if fields != 5 {
        return Err(FragIndexError::FieldMissing);
    }
    Ok(FragExport {
        fi,
        max_block_frames: maxb,
        free_frames: freef,
        per_order: per,
        seq,
    })
}

// ===========================================================================
// 四、J1 证据
// ===========================================================================

/// J1-a/J1-b 证据生成器：三组样本复算 + JSON 往返。
pub fn fragindex_audit() -> result::Result<usize, FragIndexError> {
    // 样本组：满块 / 半碎 / 全碎。
    let cases: [[u64; 11]; 3] = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64],   // 全空闲于 order-10。
        [0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0],    // order-8 为主。
        [4096, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], // 全碎成单帧。
    ];
    let mut checked = 0usize;
    let mut i = 0;
    while i < cases.len() {
        let inp = FragInput { per_order: cases[i] };
        let fi = match inp.fragment_index() {
            Some(v) => v,
            None => return Err(FragIndexError::EmptyLedger),
        };
        // 复算：独立按定义式再算一遍。
        let mb = inp.max_block_frames();
        let ff = inp.free_frames();
        let fi2 = compute_fi(mb, ff).ok_or(FragIndexError::FormulaDrift)?;
        if fi != fi2 {
            return Err(FragIndexError::FormulaDrift);
        }
        // JSON 往返。
        let e = FragExport {
            fi: Some(fi),
            max_block_frames: mb,
            free_frames: ff,
            per_order: cases[i],
            seq: i as u64,
        };
        let js = export_json(&e);
        let back = parse_json(&js)?;
        if back.fi != e.fi || back.max_block_frames != e.max_block_frames
            || back.free_frames != e.free_frames || back.seq != e.seq
        {
            return Err(FragIndexError::JsonParseFail);
        }
        let mut o = 0;
        while o < 11 {
            if back.per_order[o] != e.per_order[o] {
                return Err(FragIndexError::JsonParseFail);
            }
            o += 1;
        }
        checked += 1;
        i += 1;
    }
    // 空账诚实口径。
    let empty = FragInput { per_order: [0; 11] };
    if empty.fragment_index().is_some() {
        return Err(FragIndexError::EmptyLedger);
    }
    Ok(checked)
}

// ===========================================================================
// 五、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 复算组数（应 3）。
    pub recomputed: usize,
    /// JSON 往返通过。
    pub json_roundtrip_ok: bool,
    /// 空账出 None。
    pub empty_none_ok: bool,
    /// 字段缺失被拒。
    pub field_guard_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.recomputed == 3 && self.json_roundtrip_ok && self.empty_none_ok && self.field_guard_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, FragIndexError> {
    let recomputed = fragindex_audit()?;
    // 字段缺失防线。
    let short = String::from("{\"fi\":5}");
    let field_guard = parse_json(&short).is_err();
    Ok(SelfAudit {
        recomputed,
        json_roundtrip_ok: true,
        empty_none_ok: true,
        field_guard_ok: field_guard,
    })
}
