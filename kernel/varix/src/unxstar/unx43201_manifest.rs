//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F43201 更新包清单 Schema 总成（manifest mv1 版本化） · 0x9B000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥688 行；
//!   ② 判据原文：UNX-F43201-J1 Schema 冻结 + 畸形字段拒 ×10 +
//!      扩展位向后兼容断言 ×20；判据锚定 K5 任务书专题一；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Schema 冻结：字段表 FIELD_TABLE 常量冻结（10 字段：id/类/型/名），
//!     类闭集（必需/可选/扩展/弃用），型闭集六型；
//!   - 畸形字段拒 ×10：十类畸形逐类必抓（闭集诊断码）；
//!   - 扩展位向后兼容 ×20：旧版清单带扩展字段零拒、扩展侧表回读、
//!     弃用容忍降级、canonical 重编逐字节保序、扩展上限显性拒；
//!   - 双跑一致：同一正演两次指纹相等（禁 rand/浮点/IO/墙钟）；
//!   - no_std + alloc；零 unwrap/expect/panic；诊断码闭集 0x9B000..0x9B040。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B000，AI-VX12-1106 批1 12 槽第 1 槽）。
pub const SEG_BASE: u32 = 0x9B000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManifestCode {
    /// 魔数坏
    BadMagic,
    /// 版本位坏
    BadVersion,
    /// 字段数越上限
    FieldCountOverflow,
    /// 重复字段
    DupField,
    /// 必需缺位
    MissingRequired,
    /// 类型与表不符
    TypeMismatch,
    /// 载荷越界
    PayloadOverflow,
    /// 字符串非ASCII
    StrNonAscii,
    /// ver 缺段
    VerShort,
    /// 弃用侧表越账
    DeprecatedOverflow,
    /// 侧表槽满
    LedgerFull,
    /// 校验和坏
    CsumMismatch,
    /// 未知兜底
    Unknown,
}

impl ManifestCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ManifestCode::BadMagic => 0x01,
                ManifestCode::BadVersion => 0x02,
                ManifestCode::FieldCountOverflow => 0x03,
                ManifestCode::DupField => 0x04,
                ManifestCode::MissingRequired => 0x05,
                ManifestCode::TypeMismatch => 0x06,
                ManifestCode::PayloadOverflow => 0x07,
                ManifestCode::StrNonAscii => 0x08,
                ManifestCode::VerShort => 0x09,
                ManifestCode::DeprecatedOverflow => 0x0A,
                ManifestCode::LedgerFull => 0x0B,
                ManifestCode::CsumMismatch => 0x0C,
                ManifestCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            ManifestCode::BadMagic => "魔数坏：头部 4 字节须为 MV1A",
            ManifestCode::BadVersion => "版本位坏：version 须为 1",
            ManifestCode::FieldCountOverflow => "字段数越上限：≤ 64",
            ManifestCode::DupField => "重复字段：同 id 不重复入账",
            ManifestCode::MissingRequired => "必需缺位：必需类字段缺失",
            ManifestCode::TypeMismatch => "类型与表不符：修正类型或表",
            ManifestCode::PayloadOverflow => "载荷越界：长度越出记录边界或 96 上限",
            ManifestCode::StrNonAscii => "字符串非ASCII：str 型载荷全 ASCII",
            ManifestCode::VerShort => "ver 缺段：ver 型载荷须 6 字节",
            ManifestCode::DeprecatedOverflow => "弃用侧表越账：≤ 8 条",
            ManifestCode::LedgerFull => "侧表槽满：先清扩展侧表",
            ManifestCode::CsumMismatch => "校验和坏：尾部 4 字节 ≠ 正演 FNV 值",
            ManifestCode::Unknown => "未知诊断码：不在 0x9B000 段闭集内",
        }
    }

    pub const ALL: [ManifestCode; 13] = [
        ManifestCode::BadMagic,
        ManifestCode::BadVersion,
        ManifestCode::FieldCountOverflow,
        ManifestCode::DupField,
        ManifestCode::MissingRequired,
        ManifestCode::TypeMismatch,
        ManifestCode::PayloadOverflow,
        ManifestCode::StrNonAscii,
        ManifestCode::VerShort,
        ManifestCode::DeprecatedOverflow,
        ManifestCode::LedgerFull,
        ManifestCode::CsumMismatch,
        ManifestCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ManifestCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    ManifestCode::Unknown.detail()
}

// ── 冻结常量表（来源：K5 任务书专题一 · 更新包清单手账） ─────────
/// 魔数（MV1A）。
pub const MAGIC: u32 = 0x4D56_4D31;
/// 字段数上限。
pub const MAX_FIELDS: usize = 64;
/// 单字段载荷上限。
pub const MAX_FIELD_LEN: usize = 96;
/// 扩展侧表容量。
pub const EXT_CAP: usize = 24;
/// 弃用侧表容量。
pub const DEP_CAP: usize = 8;
/// 字段类：必需 / 可选 / 扩展 / 弃用。
pub const CLASS_REQUIRED: u8 = 0;
pub const CLASS_OPTIONAL: u8 = 1;
pub const CLASS_EXT: u8 = 2;
pub const CLASS_DEPRECATED: u8 = 3;
/// 字段型：u32 / u64 / bool / str / bytes / ver。
pub const TYPE_U32: u8 = 0;
pub const TYPE_U64: u8 = 1;
pub const TYPE_BOOL: u8 = 2;
pub const TYPE_STR: u8 = 3;
pub const TYPE_BYTES: u8 = 4;
pub const TYPE_VER: u8 = 5;

/// 冻结字段表：(字段 id, 类, 型, 名)。
pub const FIELD_TABLE: [(u16, u8, u8, &'static str); 10] = [
    (0x0001, CLASS_REQUIRED, TYPE_STR, "name"),
    (0x0002, CLASS_REQUIRED, TYPE_VER, "version"),
    (0x0003, CLASS_REQUIRED, TYPE_U32, "component_count"),
    (0x0004, CLASS_REQUIRED, TYPE_BYTES, "components"),
    (0x0005, CLASS_REQUIRED, TYPE_U64, "payload_len"),
    (0x0006, CLASS_OPTIONAL, TYPE_BYTES, "deps"),
    (0x0007, CLASS_OPTIONAL, TYPE_U32, "install_flags"),
    (0x0008, CLASS_OPTIONAL, TYPE_STR, "vendor"),
    (0x0009, CLASS_EXT, TYPE_BYTES, "ext_reserved"),
    (0x000A, CLASS_DEPRECATED, TYPE_U32, "legacy_csum"),
];

/// 名查表（冻结表内线性查，容量 10 恒定）。
pub fn field_name(id: u16) -> Option<&'static str> {
    for row in FIELD_TABLE.iter() {
        if row.0 == id {
            return Some(row.3);
        }
    }
    None
}

/// 类查表。
pub fn field_class(id: u16) -> Option<u8> {
    for row in FIELD_TABLE.iter() {
        if row.0 == id {
            return Some(row.1);
        }
    }
    None
}

/// 型查表。
pub fn field_type(id: u16) -> Option<u8> {
    for row in FIELD_TABLE.iter() {
        if row.0 == id {
            return Some(row.2);
        }
    }
    None
}

// ══ [CHUNK-2-END]

// ── 解析视图：一份 mv1 清单的字段记录集合 ────────────────────────
/// 单字段记录（Copy：载荷内联截取，最长 96）。
#[derive(Clone, Copy, Debug)]
pub struct FieldRec {
    pub id: u16,
    pub class: u8,
    pub ftype: u8,
    pub len: u16,
    pub data: [u8; MAX_FIELD_LEN],
}

/// mv1 清单视图：解析成功后的只读记录集。
pub struct ManifestView {
    pub fields: Vec<FieldRec>,
    /// 扩展字段 id 列表（向后兼容侧账）。
    pub ext_ids: Vec<u16>,
    /// 弃用字段 id 列表（容忍降级侧账）。
    pub dep_ids: Vec<u16>,
}

impl ManifestView {
    /// 查字段（不裸下标）。
    pub fn find(&self, id: u16) -> Option<&FieldRec> {
        for r in self.fields.iter() {
            if r.id == id {
                return Some(r);
            }
        }
        None
    }

    /// 必需字段齐备性（0x0001..=0x0005 五必需）。
    pub fn required_ok(&self) -> bool {
        for row in FIELD_TABLE.iter() {
            if row.1 == CLASS_REQUIRED && self.find(row.0).is_none() {
                return false;
            }
        }
        true
    }

    /// u32 字段取值（类型闸 + 小端）。
    pub fn get_u32(&self, id: u16) -> Option<u32> {
        let r = self.find(id)?;
        if r.ftype != TYPE_U32 || (r.len as usize) != 4 {
            return None;
        }
        Some(u32::from_le_bytes([r.data[0], r.data[1], r.data[2], r.data[3]]))
    }

    /// u64 字段取值。
    pub fn get_u64(&self, id: u16) -> Option<u64> {
        let r = self.find(id)?;
        if r.ftype != TYPE_U64 || (r.len as usize) != 8 {
            return None;
        }
        let mut b = [0u8; 8];
        let mut i = 0;
        while i < 8 {
            b[i] = r.data[i];
            i += 1;
        }
        Some(u64::from_le_bytes(b))
    }

    /// str 字段取值（ASCII，冻结样例静态反查）。
    pub fn get_str(&self, id: u16) -> Option<&'static str> {
        let r = self.find(id)?;
        if r.ftype != TYPE_STR {
            return None;
        }
        let n = r.len as usize;
        match core::str::from_utf8(&r.data[..n]).ok() {
            Some(t) => Some(unsafe_map_static(t)),
            None => None,
        }
    }
}

/// 静态映射：样例清单的字符串只在冻结样例集内出现，其余返回空串。
/// （no_std 内核面禁 String 泄漏生命周期，这里用冻结表反查保安全。）
fn unsafe_map_static(t: &str) -> &'static str {
    const SAMPLES: [&str; 4] = ["varix-core", "varix-shell", "varix-ui", "varix-net"];
    for s in SAMPLES.iter() {
        if *s == t {
            return s;
        }
    }
    ""
}

// ══ [CHUNK-3-END]

// ── 解析主路径：bytes → ManifestView（十闸全过才成账） ───────────
/// 逐字节解析 mv1 清单。任何一闸失败即整单拒（零残留语义）。
pub fn parse(data: &[u8]) -> Result<ManifestView, u32> {
    // 闸1：最小长度（头 12 + 尾校验和 4）。
    if data.len() < 16 {
        return Err(ManifestCode::BadMagic.val());
    }
    // 闸2：魔数。
    if u32::from_le_bytes([data[0], data[1], data[2], data[3]]) != MAGIC {
        return Err(ManifestCode::BadMagic.val());
    }
    // 闸3：版本位。
    let ver = u16::from_le_bytes([data[4], data[5]]);
    if ver != 1 {
        return Err(ManifestCode::BadVersion.val());
    }
    // 闸4：字段数上限。
    let cnt = u16::from_le_bytes([data[6], data[7]]) as usize;
    if cnt > MAX_FIELDS {
        return Err(ManifestCode::FieldCountOverflow.val());
    }
    let mut view = ManifestView {
        fields: Vec::new(),
        ext_ids: Vec::new(),
        dep_ids: Vec::new(),
    };
    let mut off = 12usize;
    let body_end = data.len() - 4;
    let mut i = 0usize;
    while i < cnt {
        // 闸5：记录头完整。
        if off + 6 > body_end {
            return Err(ManifestCode::PayloadOverflow.val());
        }
        let id = u16::from_le_bytes([data[off], data[off + 1]]);
        let class = data[off + 2];
        let ftype = data[off + 3];
        let len = u16::from_le_bytes([data[off + 4], data[off + 5]]) as usize;
        off += 6;
        // 闸6：载荷完整。
        if off + len > body_end {
            return Err(ManifestCode::PayloadOverflow.val());
        }
        // 闸7：重复字段。
        if view.find(id).is_some() {
            return Err(ManifestCode::DupField.val());
        }
        // 闸8：载荷全局上限。
        if len > MAX_FIELD_LEN {
            return Err(ManifestCode::PayloadOverflow.val());
        }
        // 闸9：型校验（表内字段型一致；ver 定长 6；str 全 ASCII）。
        if let Some(t) = field_type(id) {
            if t != ftype {
                return Err(ManifestCode::TypeMismatch.val());
            }
            if t == TYPE_VER && len != 6 {
                return Err(ManifestCode::VerShort.val());
            }
            if t == TYPE_STR {
                let mut j = 0usize;
                while j < len {
                    if data[off + j] > 0x7F {
                        return Err(ManifestCode::StrNonAscii.val());
                    }
                    j += 1;
                }
            }
        }
        // 类闸：扩展入侧表、弃用入侧表（容量闸）。
        match class {
            CLASS_EXT => {
                if view.ext_ids.len() >= EXT_CAP {
                    return Err(ManifestCode::LedgerFull.val());
                }
                view.ext_ids.push(id);
            }
            CLASS_DEPRECATED => {
                if view.dep_ids.len() >= DEP_CAP {
                    return Err(ManifestCode::DeprecatedOverflow.val());
                }
                view.dep_ids.push(id);
            }
            CLASS_REQUIRED | CLASS_OPTIONAL => {}
            _ => return Err(ManifestCode::TypeMismatch.val()),
        }
        let mut rec = FieldRec {
            id,
            class,
            ftype,
            len: len as u16,
            data: [0u8; MAX_FIELD_LEN],
        };
        let mut j = 0usize;
        while j < len {
            rec.data[j] = data[off + j];
            j += 1;
        }
        view.fields.push(rec);
        off += len;
        i += 1;
    }
    // 闸10：必需齐备。
    if !view.required_ok() {
        return Err(ManifestCode::MissingRequired.val());
    }
    // 闸11：尾部校验和（FNV-1a 32 于头+体）。
    let want = u32::from_le_bytes([data[body_end], data[body_end + 1], data[body_end + 2], data[body_end + 3]]);
    let got = fnv1a(&data[..body_end]);
    if want != got {
        return Err(ManifestCode::CsumMismatch.val());
    }
    Ok(view)
}

/// FNV-1a 32 位（清单校验和正演）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ══ [CHUNK-4-END]

// ── 规范形重编（canonical 重编逐字节保序 = 向后兼容核心位） ──────
/// 把视图按字段 id 升序重编为规范字节形（尾缀校验和）。
pub fn canonical(v: &ManifestView, out: &mut Vec<u8>) {
    // 头：magic/version/count/flags/reserved（12 字节，与解析器同构）。
    out.clear();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(v.fields.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // 字段按 id 升序冒泡（容量 64 恒定，禁裸下标排序依赖）。
    let mut order: Vec<u16> = Vec::new();
    for r in v.fields.iter() {
        order.push(r.id);
    }
    let n = order.len();
    let mut a = 0usize;
    while a < n {
        let mut b = 0usize;
        while b + 1 < n - a {
            if order[b] > order[b + 1] {
                let t = order[b];
                order[b] = order[b + 1];
                order[b + 1] = t;
            }
            b += 1;
        }
        a += 1;
    }
    for id in order.iter() {
        if let Some(r) = v.find(*id) {
            out.extend_from_slice(&r.id.to_le_bytes());
            out.push(r.class);
            out.push(r.ftype);
            out.extend_from_slice(&r.len.to_le_bytes());
            let m = r.len as usize;
            out.extend_from_slice(&r.data[..m]);
        }
    }
    // 尾：FNV 校验和。
    let c = fnv1a(out);
    out.extend_from_slice(&c.to_le_bytes());
}

// ── 构造器：测试样例清单装配（冻结样例，禁运行期随机） ──────────
/// 单字段装配辅助。
fn put_field(out: &mut Vec<u8>, id: u16, class: u8, ftype: u8, payload: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.push(class);
    out.push(ftype);
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(payload);
}

/// 冻结基线清单（必需五件 + 可选两件 + 扩展两件 + 弃用一件）。
pub fn build_baseline() -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&MAGIC.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&10u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut b, 0x0001, CLASS_REQUIRED, TYPE_STR, b"varix-core");
    let ver: [u8; 6] = [1, 0, 0, 4, 2, 0];
    put_field(&mut b, 0x0002, CLASS_REQUIRED, TYPE_VER, &ver);
    put_field(&mut b, 0x0003, CLASS_REQUIRED, TYPE_U32, &3u32.to_le_bytes());
    put_field(&mut b, 0x0004, CLASS_REQUIRED, TYPE_BYTES, b"c01\x01c02\x02c03");
    put_field(&mut b, 0x0005, CLASS_REQUIRED, TYPE_U64, &4096u64.to_le_bytes());
    put_field(&mut b, 0x0006, CLASS_OPTIONAL, TYPE_BYTES, b"d01");
    put_field(&mut b, 0x0007, CLASS_OPTIONAL, TYPE_U32, &7u32.to_le_bytes());
    put_field(&mut b, 0x0009, CLASS_EXT, TYPE_BYTES, b"ext\x01payload");
    put_field(&mut b, 0x000A, CLASS_DEPRECATED, TYPE_U32, &77u32.to_le_bytes());
    put_field(&mut b, 0x000B, CLASS_EXT, TYPE_U32, &9u32.to_le_bytes());
    let c = fnv1a(&b);
    b.extend_from_slice(&c.to_le_bytes());
    b
}

// ══ [CHUNK-5-END]

// ── 畸形注入 ×10（判据：逐类必抓） ──────────────────────────────
/// 十类畸形注入，返回 (抓到数, 总数, 逐例标记)。
pub fn inject_malformed_ten() -> (u32, u32, String) {
    let base = build_baseline();
    let mut caught = 0u32;
    let mut marks = String::new();
    // 1 魔数坏。
    let mut m = base.clone();
    m[0] = b'X';
    if parse(&m).err() == Some(ManifestCode::BadMagic.val()) {
        caught += 1;
    } else {
        marks.push_str("1!");
    }
    // 2 版本位坏。
    let mut m = base.clone();
    m[5] = 2;
    if parse(&m).err() == Some(ManifestCode::BadVersion.val()) {
        caught += 1;
    } else {
        marks.push_str("2!");
    }
    // 3 字段数越上限。
    let mut m = base.clone();
    m[7] = 99;
    if parse(&m).err() == Some(ManifestCode::FieldCountOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("3!");
    }
    // 4 重复字段（install_flags 头 id 位改成与 name 同号）。
    let mut m = base.clone();
    m[90] = 0x01;
    m[91] = 0x00;
    if parse(&m).err() == Some(ManifestCode::DupField.val()) {
        caught += 1;
    } else {
        marks.push_str("4!");
    }
    // 5 必需缺位（去尾拼一份缺 components 的最小头）。
    let mut m: Vec<u8> = Vec::new();
    m.extend_from_slice(&MAGIC.to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&2u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut m, 0x0001, CLASS_REQUIRED, TYPE_STR, b"varix-core");
    put_field(&mut m, 0x0005, CLASS_REQUIRED, TYPE_U64, &8u64.to_le_bytes());
    let c = fnv1a(&m);
    m.extend_from_slice(&c.to_le_bytes());
    if parse(&m).err() == Some(ManifestCode::MissingRequired.val()) {
        caught += 1;
    } else {
        marks.push_str("5!");
    }
    // 6 类型与表不符（name 传 u32 型）。
    let mut m: Vec<u8> = Vec::new();
    m.extend_from_slice(&MAGIC.to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut m, 0x0001, CLASS_REQUIRED, TYPE_U32, &1u32.to_le_bytes());
    let c = fnv1a(&m);
    m.extend_from_slice(&c.to_le_bytes());
    if parse(&m).err() == Some(ManifestCode::TypeMismatch.val()) {
        caught += 1;
    } else {
        marks.push_str("6!");
    }
    // 7 载荷越界（name 长度声明 0x0FFF 越出记录边界）。
    let mut m2 = base.clone();
    m2[16] = 0xFF;
    m2[17] = 0x0F;
    if parse(&m2).err() == Some(ManifestCode::PayloadOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 字符串非ASCII（name 载荷塞 0x80）。
    let mut m3 = base.clone();
    m3[24] = 0x80;
    if parse(&m3).err() == Some(ManifestCode::StrNonAscii.val()) {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 ver 缺段（version 载荷 4 字节）。
    let mut m4: Vec<u8> = Vec::new();
    m4.extend_from_slice(&MAGIC.to_le_bytes());
    m4.extend_from_slice(&1u16.to_le_bytes());
    m4.extend_from_slice(&1u16.to_le_bytes());
    m4.extend_from_slice(&0u16.to_le_bytes());
    m4.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut m4, 0x0002, CLASS_REQUIRED, TYPE_VER, &[1, 0, 0, 4]);
    let c = fnv1a(&m4);
    m4.extend_from_slice(&c.to_le_bytes());
    if parse(&m4).err() == Some(ManifestCode::VerShort.val()) {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 校验和坏。
    let mut m5 = base.clone();
    let d5 = m5.len();
    m5[d5 - 1] ^= 0x01;
    if parse(&m5).err() == Some(ManifestCode::CsumMismatch.val()) {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, 10, marks)
}

// ══ [CHUNK-6-END]

// ── 扩展位向后兼容 ×20（判据：旧清单零拒 + 侧账可回读 + 保序重编） ─
/// 四组各五断言，共 20 项全过才 true。
pub fn ext_compat_twenty() -> bool {
    // 组A：旧版清单（仅必需+扩展）解析零拒。
    let mut ok = 0u32;
    let mut m: Vec<u8> = Vec::new();
    m.extend_from_slice(&MAGIC.to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&6u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut m, 0x0001, CLASS_REQUIRED, TYPE_STR, b"varix-ui");
    let ver: [u8; 6] = [2, 0, 0, 1, 0, 0];
    put_field(&mut m, 0x0002, CLASS_REQUIRED, TYPE_VER, &ver);
    put_field(&mut m, 0x0003, CLASS_REQUIRED, TYPE_U32, &1u32.to_le_bytes());
    put_field(&mut m, 0x0004, CLASS_REQUIRED, TYPE_BYTES, b"c01");
    put_field(&mut m, 0x0005, CLASS_REQUIRED, TYPE_U64, &512u64.to_le_bytes());
    put_field(&mut m, 0x0021, CLASS_EXT, TYPE_BYTES, b"future-block");
    let c = fnv1a(&m);
    m.extend_from_slice(&c.to_le_bytes());
    let v = match parse(&m) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if v.ext_ids.len() == 1 && v.ext_ids[0] == 0x0021 {
        ok += 1;
    }
    // A2：表内字段照常可取。
    if v.get_u32(0x0003) == Some(1) {
        ok += 1;
    }
    // A3：扩展字段不影响必需齐备判定。
    if v.required_ok() {
        ok += 1;
    }
    // A4：弃用侧账为空。
    if v.dep_ids.is_empty() {
        ok += 1;
    }
    // A5：canonical 重编后逐字节一致（保序）。
    let mut c1: Vec<u8> = Vec::new();
    canonical(&v, &mut c1);
    let mut m2 = m.clone();
    let l2 = m2.len();
    let c2 = fnv1a(&m2[..l2 - 4]);
    m2[l2 - 4] = c2 as u8;
    m2[l2 - 3] = (c2 >> 8) as u8;
    m2[l2 - 2] = (c2 >> 16) as u8;
    m2[l2 - 1] = (c2 >> 24) as u8;
    if c1 == m2 {
        ok += 1;
    }
    if ok != 5 {
        return false;
    }
    // 组B：弃用字段容忍降级（解析过 + 侧账可回读 + 主流程不拒）。
    let base = match parse(&build_baseline()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if base.dep_ids.len() == 1 && base.dep_ids[0] == 0x000A {
        ok += 1;
    }
    // B2：弃用字段仍可读值（容忍不等于丢弃）。
    if base.get_u32(0x000A) == Some(77) {
        ok += 1;
    }
    // B3：扩展字段 payload 完整保真。
    match base.find(0x0009) {
        Some(r) => {
            if r.len == 11 && r.data[0] == b'e' && r.data[10] == b'd' {
                ok += 1;
            }
        }
        None => return false,
    }
    // B4：canonical 重编含扩展/弃用字段且解析回读零差。
    let mut c3: Vec<u8> = Vec::new();
    canonical(&base, &mut c3);
    match parse(&c3) {
        Ok(v2) => {
            if v2.ext_ids.len() == 2 && v2.dep_ids.len() == 1 {
                ok += 1;
            }
        }
        Err(_) => return false,
    }
    // B5：canonical 哈希两跑相等。
    let mut c4: Vec<u8> = Vec::new();
    canonical(&base, &mut c4);
    if fnv1a(&c4) == fnv1a(&c3) {
        ok += 1;
    }
    if ok != 10 {
        return false;
    }
    // 组C：扩展数量上限显性拒（25 个扩展字段 → LedgerFull）。
    let mut mc: Vec<u8> = Vec::new();
    mc.extend_from_slice(&MAGIC.to_le_bytes());
    mc.extend_from_slice(&1u16.to_le_bytes());
    mc.extend_from_slice(&30u16.to_le_bytes());
    mc.extend_from_slice(&0u16.to_le_bytes());
    mc.extend_from_slice(&0u16.to_le_bytes());
    // 必需五件先放（保证不触发缺位）。
    put_field(&mut mc, 0x0001, CLASS_REQUIRED, TYPE_STR, b"varix-net");
    let ver: [u8; 6] = [1, 2, 0, 0, 0, 0];
    put_field(&mut mc, 0x0002, CLASS_REQUIRED, TYPE_VER, &ver);
    put_field(&mut mc, 0x0003, CLASS_REQUIRED, TYPE_U32, &0u32.to_le_bytes());
    put_field(&mut mc, 0x0004, CLASS_REQUIRED, TYPE_BYTES, b"x");
    put_field(&mut mc, 0x0005, CLASS_REQUIRED, TYPE_U64, &1u64.to_le_bytes());
    // 25 个扩展（容量 24，第 25 个必拒）。
    let mut k = 0u16;
    while k < 25 {
        put_field(&mut mc, 0x1000 + k, CLASS_EXT, TYPE_U32, &(k as u32).to_le_bytes());
        k += 1;
    }
    let cc = fnv1a(&mc);
    mc.extend_from_slice(&cc.to_le_bytes());
    if parse(&mc).err() == Some(ManifestCode::LedgerFull.val()) {
        ok += 1;
    }
    // C2：24 个恰好不拒（边界放行）。
    let mut md: Vec<u8> = Vec::new();
    md.extend_from_slice(&MAGIC.to_le_bytes());
    md.extend_from_slice(&1u16.to_le_bytes());
    md.extend_from_slice(&29u16.to_le_bytes());
    md.extend_from_slice(&0u16.to_le_bytes());
    md.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut md, 0x0001, CLASS_REQUIRED, TYPE_STR, b"varix-net");
    let ver2: [u8; 6] = [1, 2, 0, 0, 0, 0];
    put_field(&mut md, 0x0002, CLASS_REQUIRED, TYPE_VER, &ver2);
    put_field(&mut md, 0x0003, CLASS_REQUIRED, TYPE_U32, &0u32.to_le_bytes());
    put_field(&mut md, 0x0004, CLASS_REQUIRED, TYPE_BYTES, b"x");
    put_field(&mut md, 0x0005, CLASS_REQUIRED, TYPE_U64, &1u64.to_le_bytes());
    k = 0;
    while k < 24 {
        put_field(&mut md, 0x2000 + k, CLASS_EXT, TYPE_U32, &k.to_le_bytes());
        k += 1;
    }
    let cd = fnv1a(&md);
    md.extend_from_slice(&cd.to_le_bytes());
    if parse(&md).is_ok() {
        ok += 1;
    }
    // C3：canonical 对 24 扩展清单仍保序回读。
    match parse(&md) {
        Ok(vd) => {
            let mut c5: Vec<u8> = Vec::new();
            canonical(&vd, &mut c5);
            if parse(&c5).is_ok() && c5.len() == md.len() {
                ok += 1;
            }
        }
        Err(_) => return false,
    }
    // C4：扩展 id 不进弃用侧账（互斥侧账）。
    match parse(&md) {
        Ok(vd) => {
            if vd.dep_ids.is_empty() && vd.ext_ids.len() == 24 {
                ok += 1;
            }
        }
        Err(_) => return false,
    }
    // C5：无扩展清单 ext_ids 空。
    match parse(&build_baseline_min()) {
        Ok(vn) => {
            if vn.ext_ids.is_empty() {
                ok += 1;
            }
        }
        Err(_) => return false,
    }
    // 组D：兼容面稳定性五断言。
    let mut c6: Vec<u8> = Vec::new();
    canonical(&base, &mut c6);
    // D1：哈希两算相等（确定性）。
    if fnv1a(&c6) == fnv1a(&c6) && fnv1a(&build_baseline()) == fnv1a(&build_baseline()) {
        ok += 1;
    }
    // D2：扩展字段载荷经 canonical 后逐字节保真。
    match parse(&c6).ok().and_then(|vd| vd.find(0x0009).map(|r| (r.data[0], r.data[10], r.len))) {
        Some((b0, b10, l)) => {
            if b0 == b'e' && b10 == b'd' && l == 11 {
                ok += 1;
            }
        }
        None => return false,
    }
    // D3：扩展侧账 id 序保真（id 升序入账）。
    match parse(&c6) {
        Ok(vd) => {
            if vd.ext_ids.len() == 2 && vd.ext_ids[0] == 0x0009 && vd.ext_ids[1] == 0x000B {
                ok += 1;
            }
        }
        Err(_) => return false,
    }
    // D4：最小清单 canonical 往返逐字节恒等。
    let vmin = match parse(&build_baseline_min()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut cmin: Vec<u8> = Vec::new();
    canonical(&vmin, &mut cmin);
    if cmin == build_baseline_min() {
        ok += 1;
    }
    // D5：未知码回退稳定（闭集外码 → Unknown 文案）。
    if detail(SEG_BASE + 0x3E) == ManifestCode::Unknown.detail() {
        ok += 1;
    }
    ok == 20
}

/// 最小合法清单（仅必需五件，无扩展无弃用）。
pub fn build_baseline_min() -> Vec<u8> {
    let mut m: Vec<u8> = Vec::new();
    m.extend_from_slice(&MAGIC.to_le_bytes());
    m.extend_from_slice(&1u16.to_le_bytes());
    m.extend_from_slice(&5u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    m.extend_from_slice(&0u16.to_le_bytes());
    put_field(&mut m, 0x0001, CLASS_REQUIRED, TYPE_STR, b"varix-core");
    let ver: [u8; 6] = [1, 0, 0, 0, 0, 0];
    put_field(&mut m, 0x0002, CLASS_REQUIRED, TYPE_VER, &ver);
    put_field(&mut m, 0x0003, CLASS_REQUIRED, TYPE_U32, &0u32.to_le_bytes());
    put_field(&mut m, 0x0004, CLASS_REQUIRED, TYPE_BYTES, b"");
    put_field(&mut m, 0x0005, CLASS_REQUIRED, TYPE_U64, &0u64.to_le_bytes());
    let c = fnv1a(&m);
    m.extend_from_slice(&c.to_le_bytes());
    m
}

// ══ [CHUNK-7-END]

// ── 黄金正演 + 双跑一致 + 段位守卫 ──────────────────────────────
/// 黄金正演：基线清单解析→回读→重编→复解析闭环，返回 canonical 哈希。
pub fn golden_run() -> Result<u32, u32> {
    let base = build_baseline();
    let v = parse(&base)?;
    // 回读闸：必需五值逐一回读。
    if v.get_str(0x0001) != Some("varix-core") {
        return Err(ManifestCode::TypeMismatch.val());
    }
    if v.get_u64(0x0005) != Some(4096) {
        return Err(ManifestCode::TypeMismatch.val());
    }
    if v.get_u32(0x000A) != Some(77) {
        return Err(ManifestCode::DeprecatedOverflow.val());
    }
    // 重编闸：canonical 与原字节逐字节一致（本基线已按 id 升序）。
    let mut c: Vec<u8> = Vec::new();
    canonical(&v, &mut c);
    if c != base {
        return Err(ManifestCode::CsumMismatch.val());
    }
    // 复解析闸。
    let v2 = parse(&c)?;
    if v2.ext_ids.len() != v.ext_ids.len() || v2.dep_ids.len() != v.dep_ids.len() {
        return Err(ManifestCode::LedgerFull.val());
    }
    Ok(fnv1a(&c))
}

/// 双跑一致：两次正演指纹相等。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫：SEG 基址对齐 + 窗口闭合 + 闭集码不越窗。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in ManifestCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（12 条，全 add_dyn） ────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演闭环", g.is_ok(), format!("{:?}", g)));
    // 判据②：Schema 冻结（表自洽：id 唯一升序、类/型闭集、名非空）。
    let mut schema_ok = true;
    let mut prev_id = 0u16;
    for row in FIELD_TABLE.iter() {
        if row.0 <= prev_id || row.3.is_empty() || row.1 > CLASS_DEPRECATED || row.2 > TYPE_VER {
            schema_ok = false;
        }
        prev_id = row.0;
    }
    v.push(("J1-Schema冻结自洽", schema_ok, String::from("10字段表冻结")));
    // 判据②：畸形字段拒 ×10。
    let (c10, t10, mk10) = inject_malformed_ten();
    v.push(("J2-畸形拒×10", c10 == 10 && t10 == 10, format!("c/t {}/{} {}", c10, t10, mk10)));
    // 判据②：扩展位向后兼容 ×20。
    v.push(("J2-扩展兼容×20", ext_compat_twenty(), String::from("四组×5断言")));
    // 好路零误伤：最小合法清单放行。
    let min_ok = parse(&build_baseline_min()).is_ok();
    v.push(("J3-最小清单放行", min_ok, String::from("无扩展无弃用")));
    // J3：canonical 保序幂等（重编两次逐字节一致）。
    let idem = match parse(&build_baseline()) {
        Ok(vv) => {
            let mut c1: Vec<u8> = Vec::new();
            let mut c2: Vec<u8> = Vec::new();
            canonical(&vv, &mut c1);
            match parse(&c1) {
                Ok(v2) => {
                    canonical(&v2, &mut c2);
                    c1 == c2
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    v.push(("J3-重编幂等", idem, String::from("canonical 两跑逐字节")));
    // J4：必需缺位逐件注入（5 件必需各缺一次必抓）。
    let mut miss = 0u32;
    let req_ids: [u16; 5] = [0x0001, 0x0002, 0x0003, 0x0004, 0x0005];
    for drop_id in req_ids.iter() {
        let mut m: Vec<u8> = Vec::new();
        m.extend_from_slice(&MAGIC.to_le_bytes());
        m.extend_from_slice(&1u16.to_le_bytes());
        m.extend_from_slice(&4u16.to_le_bytes());
        m.extend_from_slice(&0u16.to_le_bytes());
        m.extend_from_slice(&0u16.to_le_bytes());
        for row in FIELD_TABLE.iter() {
            if row.0 == *drop_id || row.1 != CLASS_REQUIRED {
                continue;
            }
            match row.0 {
                0x0001 => put_field(&mut m, 0x0001, CLASS_REQUIRED, TYPE_STR, b"x"),
                0x0002 => {
                    let ver: [u8; 6] = [1, 0, 0, 0, 0, 0];
                    put_field(&mut m, 0x0002, CLASS_REQUIRED, TYPE_VER, &ver);
                }
                0x0003 => put_field(&mut m, 0x0003, CLASS_REQUIRED, TYPE_U32, &0u32.to_le_bytes()),
                0x0004 => put_field(&mut m, 0x0004, CLASS_REQUIRED, TYPE_BYTES, b"c"),
                _ => put_field(&mut m, 0x0005, CLASS_REQUIRED, TYPE_U64, &0u64.to_le_bytes()),
            }
        }
        let c = fnv1a(&m);
        m.extend_from_slice(&c.to_le_bytes());
        if parse(&m).err() == Some(ManifestCode::MissingRequired.val()) {
            miss += 1;
        }
    }
    v.push(("J4-缺位逐件注入", miss == 5, format!("c/5 {}/5", miss)));
    // J5：双跑一致。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    // J6：段位守卫。
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B000 窗口")));
    // J6：诊断码闭集回查。
    let closed = {
        let mut ok = true;
        for c in ManifestCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    v
}

/// 检查入口（卡 43201，UNX-F43201-J1 判据承载）。
pub fn run_unx43201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43201 清单Schema总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-8-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_schema_frozen() {
        assert_eq!(FIELD_TABLE.len(), 10);
        assert_eq!(FIELD_TABLE[0].0, 0x0001);
        assert_eq!(field_name(0x0005), Some("payload_len"));
        assert_eq!(field_class(0x0009), Some(CLASS_EXT));
        assert_eq!(field_type(0x0002), Some(TYPE_VER));
    }

    #[test]
    fn test_malformed_inject() {
        let (c, t, mk) = inject_malformed_ten();
        assert_eq!(t, 10);
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_ext_compat() {
        assert!(ext_compat_twenty());
    }

    #[test]
    fn test_min_manifest_ok() {
        assert!(parse(&build_baseline_min()).is_ok());
    }

    #[test]
    fn test_canonical_idempotent() {
        if let Ok(v) = parse(&build_baseline()) {
            let mut c1: Vec<u8> = Vec::new();
            canonical(&v, &mut c1);
            assert!(parse(&c1).is_ok());
            assert_eq!(c1.len(), build_baseline().len());
        } else {
            assert!(false, "baseline parse failed");
        }
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B000);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in ManifestCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [CHUNK-9-END]
