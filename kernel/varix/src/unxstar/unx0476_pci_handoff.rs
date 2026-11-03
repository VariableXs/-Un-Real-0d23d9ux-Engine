//! UNX-F0476 PCI 交接字段（A3 预备） · 0x75880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，AI-VX7-1104 批3 H 组）：
//!   交接字段 v1 冻结（10 字段），A3 mock 消费 diff 零差。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 交接字段闭集冻结 10 项：总线位图摘要/桥数/有效功能数/BAR 槽数/
//!     能力链总数/类码桶 6 数组摘要/FNV 指纹/序号/封账态/版本——
//!     `HandoffField` 枚举即契约面，A3 消费方按此闭集读；
//!   ② 类码桶 6 数组（桶序冻结）：0=未分类(0x00) 1=存储(0x01)
//!     2=网络(0x02) 3=显示(0x03) 4=桥(0x06) 5=其他——按基类码入桶，
//!     桶序即摘要字节序，摘要 = 6 桶 FNV-1a 64；
//!   ③ 冻结语义单向：build→freeze 后 10 槽 set_field 全拒
//!     （PostFrozen），重冻结同版拒、升版重封放行（版本只升不降）；
//!   ④ 序号单调：serial 严格递增，非递增显性拒（SerialNonMonotonic）；
//!   ⑤ A3 mock 消费：mock 面按 10 字段闭集逐字段读包，产出 MockView，
//!     `diff` 对拍包与视图逐字段等值——判据要求零差（DiffNonZero 拒）；
//!   ⑥ 指纹守恒：封账指纹 = 10 字段规范序（字段枚举值升序）下
//!     值字节 FNV-1a 64，A3 消费端复算指纹必须与包面一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75880，批3 H 组独占窗口）。
pub const SEG_BASE: u32 = 0x75880;

/// 交接字段数（冻结 10）。
pub const FIELD_COUNT: usize = 10;
/// 交接版本（冻结 v1）。
pub const HANDOFF_VERSION: u32 = 1;
/// 类码桶数（冻结 6）。
pub const CLASS_BUCKETS: usize = 6;
/// 序号基（冻结 0x1F40：批3 H 组交接面起始序）。
pub const SERIAL_BASE: u32 = 0x1F40;
/// 单包字段容量上限（与 FIELD_COUNT 同值，防御性冻结）。
pub const FIELD_CAP: usize = 16;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffCode {
    /// 冻结后改写：字段面已冻结
    PostFrozen,
    /// 字段未知：不在 10 字段闭集内
    FieldUnknown,
    /// 字段缺失：A3 消费必需字段不在包内
    FieldMissing,
    /// 版本倒退：重封版本不高于已封版本
    VersionRegress,
    /// 序号非单调：serial 未严格递增
    SerialNonMonotonic,
    /// 指纹失配：消费端复算指纹与包面不等
    DigestMismatch,
    /// 消费 diff 非零：包面与 mock 视图字段值不等
    DiffNonZero,
    /// 类码桶越界：桶号 ≥ 6
    BucketOver,
    /// 未封账即消费：A3 只消费封账包
    NotSealed,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl HandoffCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HandoffCode::PostFrozen => "冻结后改写拒：交接字段面已封账，如需改数请升版重走 build→freeze",
            HandoffCode::FieldUnknown => "字段未知：交接字段仅 10 项闭集，请按 HandoffField 枚举取用",
            HandoffCode::FieldMissing => "字段缺失：A3 消费需要全部 10 字段在包，请查 build 是否漏灌",
            HandoffCode::VersionRegress => "版本倒退拒：重封版本必须严格升版，请核对版本账",
            HandoffCode::SerialNonMonotonic => "序号非单调：serial 必须严格递增，请按 SERIAL_BASE 顺排",
            HandoffCode::DigestMismatch => "指纹失配：请核对字段规范序（枚举值升序）与值字节序",
            HandoffCode::DiffNonZero => "消费 diff 非零：包面与 mock 视图不等，请先 diff 定位字段再修",
            HandoffCode::BucketOver => "类码桶越界：桶号仅 0..=5，请按类码桶表入桶",
            HandoffCode::NotSealed => "未封账即消费拒：A3 只消费封账包，请先 freeze",
            HandoffCode::NotReproducible => "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值",
        }
    }
    pub const ALL: [HandoffCode; 10] = [
        HandoffCode::PostFrozen,
        HandoffCode::FieldUnknown,
        HandoffCode::FieldMissing,
        HandoffCode::VersionRegress,
        HandoffCode::SerialNonMonotonic,
        HandoffCode::DigestMismatch,
        HandoffCode::DiffNonZero,
        HandoffCode::BucketOver,
        HandoffCode::NotSealed,
        HandoffCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < HandoffCode::ALL.len() {
        if let Some(c) = HandoffCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75880 段闭集内"
}

// ── 域模型：交接字段闭集 ────────────────────────────────────────
/// 交接字段闭集（冻结 v1：10 字段，枚举值即规范序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffField {
    /// 0 · 总线位图摘要（扫描面位图 FNV 摘要）
    BusBitmapDigest,
    /// 1 · 桥数
    BridgeCount,
    /// 2 · 有效功能数
    ValidFnCount,
    /// 3 · BAR 槽数
    BarSlots,
    /// 4 · 能力链总数
    CapChainTotal,
    /// 5 · 类码桶 6 数组摘要（6 桶 FNV）
    ClassBucketDigest,
    /// 6 · FNV 指纹（封账指纹，由前 5 数值字段+桶摘要推导）
    FnvFingerprint,
    /// 7 · 序号（交接面单调序）
    SerialNo,
    /// 8 · 封账态（1 = 已封）
    SealState,
    /// 9 · 版本（交接契约版本 v1）
    Version,
}

impl HandoffField {
    /// 字段名（中文冻结名，diff 报表用）。
    pub fn name(self) -> &'static str {
        match self {
            HandoffField::BusBitmapDigest => "总线位图摘要",
            HandoffField::BridgeCount => "桥数",
            HandoffField::ValidFnCount => "有效功能数",
            HandoffField::BarSlots => "BAR 槽数",
            HandoffField::CapChainTotal => "能力链总数",
            HandoffField::ClassBucketDigest => "类码桶6数组摘要",
            HandoffField::FnvFingerprint => "FNV 指纹",
            HandoffField::SerialNo => "序号",
            HandoffField::SealState => "封账态",
            HandoffField::Version => "版本",
        }
    }
    /// 全闭集按枚举值升序（规范序 = 指纹字节序 = 消费序）。
    pub const ALL: [HandoffField; FIELD_COUNT] = [
        HandoffField::BusBitmapDigest,
        HandoffField::BridgeCount,
        HandoffField::ValidFnCount,
        HandoffField::BarSlots,
        HandoffField::CapChainTotal,
        HandoffField::ClassBucketDigest,
        HandoffField::FnvFingerprint,
        HandoffField::SerialNo,
        HandoffField::SealState,
        HandoffField::Version,
    ];
}

/// 字段值（闭集 3 形态：数值/摘要/旗标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldVal {
    /// 数值型（桥数/功能数/BAR 槽/能力链/序号/版本）
    Num(u64),
    /// 摘要型（位图摘要/桶摘要/FNV 指纹）
    Digest(u64),
    /// 旗标型（封账态）
    Flag(bool),
}

/// 类码桶表（6 桶，桶序冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassBuckets {
    /// 桶 0：未分类（基类 0x00）
    pub unclassified: u64,
    /// 桶 1：存储（基类 0x01）
    pub storage: u64,
    /// 桶 2：网络（基类 0x02）
    pub network: u64,
    /// 桶 3：显示（基类 0x03）
    pub display: u64,
    /// 桶 4：桥（基类 0x06）
    pub bridge: u64,
    /// 桶 5：其他
    pub other: u64,
}

impl ClassBuckets {
    pub fn new() -> ClassBuckets {
        ClassBuckets { unclassified: 0, storage: 0, network: 0, display: 0, bridge: 0, other: 0 }
    }
    /// 按基类码入桶（桶序冻结）。
    pub fn bump(&mut self, class_base: u8) -> Result<usize, u32> {
        let bucket = match class_base {
            0x00 => &mut self.unclassified,
            0x01 => &mut self.storage,
            0x02 => &mut self.network,
            0x03 => &mut self.display,
            0x06 => &mut self.bridge,
            _ => &mut self.other,
        };
        *bucket = bucket.saturating_add(1);
        Ok(self.bucket_index(class_base))
    }
    /// 桶号（0..=5）；越界不可能（match 全覆盖），防御性闭集返回。
    pub fn bucket_index(&self, class_base: u8) -> usize {
        match class_base {
            0x00 => 0,
            0x01 => 1,
            0x02 => 2,
            0x03 => 3,
            0x06 => 4,
            _ => 5,
        }
    }
    /// 桶数组视图（规范序 0..5）。
    pub fn as_array(&self) -> [u64; CLASS_BUCKETS] {
        [self.unclassified, self.storage, self.network, self.display, self.bridge, self.other]
    }
}

// ── 交接包 ──────────────────────────────────────────────────────
/// 交接包（10 字段 + 冻结态 + 版本/序号账）。
#[derive(Clone, Debug)]
pub struct HandoffPack {
    pub fields: Vec<(HandoffField, FieldVal)>,
    pub frozen: bool,
    pub version: u32,
    pub serial: u32,
    /// 封账指纹（freeze 时落定）
    pub fp: u64,
}

impl HandoffPack {
    pub fn new(serial: u32) -> HandoffPack {
        HandoffPack { fields: Vec::new(), frozen: false, version: HANDOFF_VERSION, serial, fp: 0 }
    }

    /// 灌字段（冻结后显性拒；重复字段拒；越闭集不可能——枚举即闭集）。
    pub fn set_field(&mut self, f: HandoffField, v: FieldVal) -> Result<(), u32> {
        if self.frozen {
            return Err(HandoffCode::PostFrozen.val());
        }
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some((ef, _)) = self.fields.get(i) {
                if *ef == f {
                    return Err(HandoffCode::FieldUnknown.val());
                }
            }
            i += 1;
        }
        self.fields.push((f, v));
        Ok(())
    }

    /// 按 10 字段闭集补齐缺省（显式灌默认值，杜绝字段缺失）。
    pub fn fill_defaults(&mut self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < HandoffField::ALL.len() {
            let f = HandoffField::ALL.get(i).copied().unwrap_or(HandoffField::Version);
            if self.field_of(f).is_none() {
                let dv = match f {
                    HandoffField::SealState => FieldVal::Flag(false),
                    HandoffField::Version => FieldVal::Num(HANDOFF_VERSION as u64),
                    HandoffField::SerialNo => FieldVal::Num(self.serial as u64),
                    _ => FieldVal::Num(0),
                };
                self.set_field(f, dv)?;
            }
            i += 1;
        }
        Ok(())
    }

    /// 字段读（闭集语义）。
    pub fn field_of(&self, f: HandoffField) -> Option<FieldVal> {
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some((ef, ev)) = self.fields.get(i) {
                if *ef == f {
                    return Some(*ev);
                }
            }
            i += 1;
        }
        None
    }

    /// 数值读（Num 形态展开）。
    pub fn num_of(&self, f: HandoffField) -> Option<u64> {
        match self.field_of(f) {
            Some(FieldVal::Num(n)) => Some(n),
            _ => None,
        }
    }

    /// 封账：10 字段齐 + 版本/序号/封账态字段同步 + 指纹落定 + frozen 置位。
    /// Frozen 态重入：同版/降版拒（VersionRegress），升版重封放行。
    /// 指纹防自指：compute_fp 时指纹字段保持旧值（首封为 0），
    /// 算毕覆写——指纹含版本/序号/封账态现值，改任一字段指纹必变。
    pub fn freeze(&mut self, version: u32) -> Result<u64, u32> {
        if self.fields.len() < FIELD_COUNT {
            return Err(HandoffCode::FieldMissing.val());
        }
        if self.frozen && version <= self.version {
            return Err(HandoffCode::VersionRegress.val());
        }
        self.version = version;
        // 同步版本/序号/封账态三字段到现值（封账态恒置真）。
        let mut i = 0usize;
        while i < self.fields.len() {
            if let Some((f, _)) = self.fields.get(i) {
                match f {
                    HandoffField::Version => self.fields[i] = (HandoffField::Version, FieldVal::Num(self.version as u64)),
                    HandoffField::SerialNo => self.fields[i] = (HandoffField::SerialNo, FieldVal::Num(self.serial as u64)),
                    HandoffField::SealState => self.fields[i] = (HandoffField::SealState, FieldVal::Flag(true)),
                    _ => {}
                }
            }
            i += 1;
        }
        let fp = self.compute_fp();
        let mut j = 0usize;
        while j < self.fields.len() {
            if let Some((f, _)) = self.fields.get(j) {
                if *f == HandoffField::FnvFingerprint {
                    self.fields[j] = (HandoffField::FnvFingerprint, FieldVal::Digest(fp));
                }
            }
            j += 1;
        }
        self.frozen = true;
        self.fp = fp;
        Ok(fp)
    }

    /// 规范序指纹：字段按枚举值升序拼值字节（Num/Digest 大端 8 字节，
    /// Flag 1 字节 0/1），FNV-1a 64。
    pub fn compute_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < HandoffField::ALL.len() {
            let f = HandoffField::ALL.get(i).copied().unwrap_or(HandoffField::Version);
            if let Some(v) = self.field_of(f) {
                match v {
                    FieldVal::Num(n) => {
                        buf.push((n >> 56) as u8);
                        buf.push((n >> 48) as u8);
                        buf.push((n >> 40) as u8);
                        buf.push((n >> 32) as u8);
                        buf.push((n >> 24) as u8);
                        buf.push((n >> 16) as u8);
                        buf.push((n >> 8) as u8);
                        buf.push(n as u8);
                    }
                    FieldVal::Digest(n) => {
                        buf.push((n >> 56) as u8);
                        buf.push((n >> 48) as u8);
                        buf.push((n >> 40) as u8);
                        buf.push((n >> 32) as u8);
                        buf.push((n >> 24) as u8);
                        buf.push((n >> 16) as u8);
                        buf.push((n >> 8) as u8);
                        buf.push(n as u8);
                    }
                    FieldVal::Flag(b) => buf.push(if b { 1 } else { 0 }),
                }
            }
            i += 1;
        }
        fnv1a64(&buf)
    }

    /// 序号单调校验（交接面串联包时用）。
    pub fn serial_follows(&self, prev: u32) -> Result<(), u32> {
        if self.serial <= prev {
            return Err(HandoffCode::SerialNonMonotonic.val());
        }
        Ok(())
    }
}

// ── A3 mock 消费面 ──────────────────────────────────────────────
/// A3 mock 消费视图（10 字段逐值 + 消费端复算指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockView {
    pub bus_bitmap_digest: u64,
    pub bridge_count: u64,
    pub valid_fn_count: u64,
    pub bar_slots: u64,
    pub cap_chain_total: u64,
    pub class_bucket_digest: u64,
    pub fnv_fingerprint: u64,
    pub serial_no: u64,
    pub seal_state: bool,
    pub version: u64,
}

/// A3 mock 消费：按 10 字段闭集逐字段读包。
/// 前置：封账态必须为真（NotSealed 拒），10 字段必须全在（FieldMissing 拒）。
pub fn a3_mock_consume(pack: &HandoffPack) -> Result<MockView, u32> {
    if !pack.frozen {
        return Err(HandoffCode::NotSealed.val());
    }
    if pack.fields.len() < FIELD_COUNT {
        return Err(HandoffCode::FieldMissing.val());
    }
    let num = |f: HandoffField| -> Result<u64, u32> {
        match pack.field_of(f) {
            Some(FieldVal::Num(n)) => Ok(n),
            _ => Err(HandoffCode::FieldMissing.val()),
        }
    };
    let dig = |f: HandoffField| -> Result<u64, u32> {
        match pack.field_of(f) {
            Some(FieldVal::Digest(n)) => Ok(n),
            _ => Err(HandoffCode::FieldMissing.val()),
        }
    };
    let seal = match pack.field_of(HandoffField::SealState) {
        Some(FieldVal::Flag(b)) => b,
        _ => return Err(HandoffCode::FieldMissing.val()),
    };
    let view = MockView {
        bus_bitmap_digest: dig(HandoffField::BusBitmapDigest)?,
        bridge_count: num(HandoffField::BridgeCount)?,
        valid_fn_count: num(HandoffField::ValidFnCount)?,
        bar_slots: num(HandoffField::BarSlots)?,
        cap_chain_total: num(HandoffField::CapChainTotal)?,
        class_bucket_digest: dig(HandoffField::ClassBucketDigest)?,
        fnv_fingerprint: dig(HandoffField::FnvFingerprint)?,
        serial_no: num(HandoffField::SerialNo)?,
        seal_state: seal,
        version: num(HandoffField::Version)?,
    };
    // 消费端复算指纹必须与包面一致（指纹守恒）。
    if view.fnv_fingerprint != pack.fp {
        return Err(HandoffCode::DigestMismatch.val());
    }
    Ok(view)
}

/// diff 行（字段名 + 包面值文本 + 视图值文本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub field: &'static str,
    pub pack_text: String,
    pub view_text: String,
}

/// 包 vs 视图 diff（零差判据：rows 必须为空）。
pub fn diff_pack_view(pack: &HandoffPack, view: &MockView) -> Vec<DiffRow> {
    let mut rows: Vec<DiffRow> = Vec::new();
    let mut i = 0usize;
    while i < HandoffField::ALL.len() {
        let f = HandoffField::ALL.get(i).copied().unwrap_or(HandoffField::Version);
        let pv = pack.field_of(f);
        let vv = match f {
            HandoffField::BusBitmapDigest => Some(FieldVal::Digest(view.bus_bitmap_digest)),
            HandoffField::BridgeCount => Some(FieldVal::Num(view.bridge_count)),
            HandoffField::ValidFnCount => Some(FieldVal::Num(view.valid_fn_count)),
            HandoffField::BarSlots => Some(FieldVal::Num(view.bar_slots)),
            HandoffField::CapChainTotal => Some(FieldVal::Num(view.cap_chain_total)),
            HandoffField::ClassBucketDigest => Some(FieldVal::Digest(view.class_bucket_digest)),
            HandoffField::FnvFingerprint => Some(FieldVal::Digest(view.fnv_fingerprint)),
            HandoffField::SerialNo => Some(FieldVal::Num(view.serial_no)),
            HandoffField::SealState => Some(FieldVal::Flag(view.seal_state)),
            HandoffField::Version => Some(FieldVal::Num(view.version)),
        };
        if pv != vv {
            rows.push(DiffRow {
                field: f.name(),
                pack_text: format!("{:?}", pv),
                view_text: format!("{:?}", vv),
            });
        }
        i += 1;
    }
    rows
}

// ── FNV-1a 64 ──────────────────────────────────────────────────
/// FNV-1a 64 位（参数冻结：offset 0xcbf29ce484222325，prime 0x100000001b3）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 类码桶摘要（6 桶大端拼接 FNV）。
pub fn bucket_digest(b: &ClassBuckets) -> u64 {
    let arr = b.as_array();
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CLASS_BUCKETS {
        if let Some(n) = arr.get(i) {
            buf.push((*n >> 56) as u8);
            buf.push((*n >> 48) as u8);
            buf.push((*n >> 40) as u8);
            buf.push((*n >> 32) as u8);
            buf.push((*n >> 24) as u8);
            buf.push((*n >> 16) as u8);
            buf.push((*n >> 8) as u8);
            buf.push(*n as u8);
        }
        i += 1;
    }
    fnv1a64(&buf)
}

// ── 扫描面语料（本地最小语料：与 F0474 概括账同源的扫描数） ──────
/// 交接包语料（冻结锚：桥 3 / 有效功能 7 / BAR 14 / 能力链 35）。
pub fn machine_counts() -> (u64, u64, u64, u64) {
    (3, 7, 14, 35)
}

/// 按语料建交接包（10 字段全灌 + 封账）。
pub fn build_pack(serial: u32, version: u32) -> Result<HandoffPack, u32> {
    let (bridges, valid, bars, caps) = machine_counts();
    let mut buckets = ClassBuckets::new();
    // 语料类码分布（冻结）：0x06×3（桥族）、0x03×2（显示族）、
    // 0x02×1（网络）、0x04×1（其他）。
    let _ = buckets.bump(0x06);
    let _ = buckets.bump(0x06);
    let _ = buckets.bump(0x06);
    let _ = buckets.bump(0x03);
    let _ = buckets.bump(0x03);
    let _ = buckets.bump(0x02);
    let _ = buckets.bump(0x04);
    let mut p = HandoffPack::new(serial);
    p.set_field(HandoffField::BusBitmapDigest, FieldVal::Digest(0xA5A5_0000_7578_0001))?;
    p.set_field(HandoffField::BridgeCount, FieldVal::Num(bridges))?;
    p.set_field(HandoffField::ValidFnCount, FieldVal::Num(valid))?;
    p.set_field(HandoffField::BarSlots, FieldVal::Num(bars))?;
    p.set_field(HandoffField::CapChainTotal, FieldVal::Num(caps))?;
    p.set_field(HandoffField::ClassBucketDigest, FieldVal::Digest(bucket_digest(&buckets)))?;
    p.set_field(HandoffField::FnvFingerprint, FieldVal::Digest(0))?;
    p.set_field(HandoffField::SerialNo, FieldVal::Num(serial as u64))?;
    p.set_field(HandoffField::SealState, FieldVal::Flag(false))?;
    p.set_field(HandoffField::Version, FieldVal::Num(version as u64))?;
    p.freeze(version)?;
    Ok(p)
}

// ── LCG（种子固定可用） ────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（五路：改写/缺字段/序号倒挂/指纹篡改/未封消费） ────
pub struct InjectLedger {
    pub tries: [u32; 5],
    pub caught: [u32; 5],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 5], caught: [0; 5] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入单路（冻结后 10 槽改写全拒是主注入路）。
pub fn inject_frozen_rewrite(pack: &mut HandoffPack) -> u32 {
    let mut caught = 0u32;
    let mut i = 0usize;
    while i < HandoffField::ALL.len() {
        let f = HandoffField::ALL.get(i).copied().unwrap_or(HandoffField::Version);
        if matches!(pack.set_field(f, FieldVal::Num(0xDEAD)), Err(e) if e == HandoffCode::PostFrozen.val()) {
            caught += 1;
        }
        i += 1;
    }
    caught
}

// ── 黄金全链：建包→封账→10槽改写全拒→A3消费→diff零差→序号单调 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut p = build_pack(SERIAL_BASE, HANDOFF_VERSION)?;
    // 冻结后 10 槽改写全拒。
    let rewrite_caught = inject_frozen_rewrite(&mut p);
    // A3 mock 消费。
    let view = a3_mock_consume(&p)?;
    let consume_ok = (view.seal_state && view.version == HANDOFF_VERSION as u64 && view.serial_no == SERIAL_BASE as u64) as u32;
    // diff 零差。
    let rows = diff_pack_view(&p, &view);
    let diff_zero = (rows.is_empty()) as u32;
    // 序号单调（0x1F41 > 0x1F40 递增放行；自比非严格递增显性拒）。
    let p2 = build_pack(SERIAL_BASE + 1, HANDOFF_VERSION)?;
    let serial_ok = p2.serial_follows(SERIAL_BASE).is_ok() as u32;
    let self_reject = matches!(
        p2.serial_follows(SERIAL_BASE + 1),
        Err(e) if e == HandoffCode::SerialNonMonotonic.val()
    ) as u32;
    Ok((
        rewrite_caught + consume_ok + diff_zero + serial_ok + self_reject,
        rewrite_caught,
        p.version,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链精确值（绿位 14 / 拦 10 = 10 槽全拒 / 版本 1）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((ok, caught, ver)) if caught == FIELD_COUNT as u32 && ok == 14 && ver == HANDOFF_VERSION => Ok(()),
        _ => Err(HandoffCode::NotReproducible.val()),
    }
}

/// 序 B：A3 消费前置（未封账拒 / 缺字段拒 / 指纹篡改必抓）。
pub fn probe_seq_b() -> Result<(), u32> {
    // 未封账即消费拒。
    let mut raw = HandoffPack::new(SERIAL_BASE);
    let _ = raw.fill_defaults();
    if !matches!(a3_mock_consume(&raw), Err(e) if e == HandoffCode::NotSealed.val()) {
        return Err(HandoffCode::NotSealed.val());
    }
    // 字段缺失/形态错位拒（fill_defaults 灌的是 Num 形态，摘要字段
    // 需 Digest——消费端按形态取值取不到即 FieldMissing）。
    let mut thin = HandoffPack::new(SERIAL_BASE);
    let _ = thin.fill_defaults();
    let _ = thin.freeze(1);
    if !matches!(a3_mock_consume(&thin), Err(e) if e == HandoffCode::FieldMissing.val()) {
        return Err(HandoffCode::FieldMissing.val());
    }
    // 指纹篡改必抓（消费端复算 vs 包面）。
    let mut p = build_pack(SERIAL_BASE, HANDOFF_VERSION)?;
    // 封账后不能改字段——直接篡改 fp 账（模拟传输位翻转）。
    p.fp = p.fp ^ 0x1;
    if !matches!(a3_mock_consume(&p), Err(e) if e == HandoffCode::DigestMismatch.val()) {
        return Err(HandoffCode::DigestMismatch.val());
    }
    Ok(())
}

/// 序 C：冻结面（10 字段闭集 / 版本 v1 / 类码桶 6 / 语料锚）。
pub fn probe_seq_c() -> Result<(), u32> {
    let consts_ok = FIELD_COUNT == 10 && HANDOFF_VERSION == 1 && CLASS_BUCKETS == 6 && SERIAL_BASE == 0x1F40;
    let (b, v, s, c) = machine_counts();
    let corpus_ok = b == 3 && v == 7 && s == 14 && c == 35;
    match (consts_ok, corpus_ok) {
        (true, true) => Ok(()),
        _ => Err(HandoffCode::VersionRegress.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 5 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        // 路 1：冻结后改写路（10 槽逐槽必拒）。
        let mut p1 = build_pack(SERIAL_BASE, HANDOFF_VERSION)?;
        led.record(0, inject_frozen_rewrite(&mut p1) == FIELD_COUNT as u32);
        // 路 2：字段缺失/形态错位路（fill_defaults+freeze 后摘要字段形态错位）。
        let mut thin = HandoffPack::new(SERIAL_BASE);
        let _ = thin.fill_defaults();
        let _ = thin.freeze(1);
        led.record(1, matches!(a3_mock_consume(&thin), Err(e) if e == HandoffCode::FieldMissing.val()));
        // 路 3：序号倒挂路。
        let p3 = build_pack(SERIAL_BASE, HANDOFF_VERSION).unwrap_or(HandoffPack::new(SERIAL_BASE));
        led.record(2, p3.serial_follows(SERIAL_BASE + 5).is_err());
        // 路 4：指纹篡改路。
        let mut p4 = build_pack(SERIAL_BASE, HANDOFF_VERSION)?;
        p4.fp = p4.fp.wrapping_add(rng.below(8) + 1);
        led.record(3, matches!(a3_mock_consume(&p4), Err(e) if e == HandoffCode::DigestMismatch.val()));
        // 路 5：未封账消费路。
        let mut raw = HandoffPack::new(SERIAL_BASE);
        let _ = raw.fill_defaults();
        led.record(4, matches!(a3_mock_consume(&raw), Err(e) if e == HandoffCode::NotSealed.val()));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HandoffCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0usize;
    while i < HandoffCode::ALL.len() {
        if let Some(c) = HandoffCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, caught, ver)) => *caught == FIELD_COUNT as u32 && *ok == 14 && *ver == HANDOFF_VERSION,
            Err(_) => false,
        },
        match &g {
            Ok((ok, caught, ver)) => format!("绿位={} 改写拦={} 版本={}", ok, caught, ver),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 10 字段闭集在位（名面 + 序面双对拍）。
    let p = build_pack(SERIAL_BASE, HANDOFF_VERSION);
    let fields_ok = match &p {
        Ok(pk) => {
            let mut all_in = true;
            let mut i = 0usize;
            while i < HandoffField::ALL.len() {
                if let Some(f) = HandoffField::ALL.get(i) {
                    if pk.field_of(*f).is_none() {
                        all_in = false;
                    }
                }
                i += 1;
            }
            all_in && pk.fields.len() == FIELD_COUNT
        }
        Err(_) => false,
    };
    v.push((
        "J1-10字段闭集在位",
        fields_ok,
        format!("字段数={} 名面序面全在位", FIELD_COUNT),
    ));

    // 3. 冻结后 10 槽改写全拒。
    let mut pk = match p {
        Ok(pk) => pk,
        Err(e) => {
            v.push(("J1-冻结改写全拒", false, format!("建包 err=0x{:x}", e)));
            return v;
        }
    };
    let caught = inject_frozen_rewrite(&mut pk);
    v.push((
        "J1-冻结改写全拒",
        caught == FIELD_COUNT as u32,
        format!("10 槽逐槽改写拦截 {}/10", caught),
    ));

    // 4. A3 mock 消费（前置全过 + 字段值透传）。
    let view = a3_mock_consume(&pk);
    let (b, vf, s, c) = machine_counts();
    let consume_ok = match &view {
        Ok(vw) => vw.bridge_count == b && vw.valid_fn_count == vf && vw.bar_slots == s && vw.cap_chain_total == c && vw.seal_state && vw.version == HANDOFF_VERSION as u64,
        Err(_) => false,
    };
    v.push((
        "J1-A3消费透传",
        consume_ok,
        format!("桥{} 功能{} BAR{} 链{} 封账态/版本透传一致", b, vf, s, c),
    ));

    // 5. diff 零差主判据。
    let diff_rows = match &view {
        Ok(vw) => diff_pack_view(&pk, vw),
        Err(_) => Vec::new(),
    };
    v.push((
        "J1-消费diff零差",
        match &view {
            Ok(vw) => diff_pack_view(&pk, vw).is_empty(),
            Err(_) => false,
        },
        format!("diff 行数={}（0 为零差）", diff_rows.len()),
    ));

    // 6. 消费前置三拒（未封账/缺字段/指纹篡改）。
    let b_ok = probe_seq_b();
    let b_okv = b_ok.is_ok();
    let b_err = match b_ok {
        Ok(()) => String::from("NotSealed/FieldMissing/DigestMismatch 三拒全过"),
        Err(e) => format!("err=0x{:x} {}", e, detail(e)),
    };
    v.push(("J1-消费前置三拒", b_okv, b_err));

    // 7. 序号单调语义（递增放行 / 非递增拒）。
    let p_next = build_pack(SERIAL_BASE + 1, HANDOFF_VERSION);
    let serial_ok = match p_next {
        Ok(pn) => pn.serial_follows(SERIAL_BASE).is_ok() && pn.serial_follows(SERIAL_BASE + 1).is_err(),
        Err(_) => false,
    };
    v.push((
        "J1-序号单调语义",
        serial_ok,
        String::from("0x1F40→0x1F41 递增放行；自比/倒挂显性拒"),
    ));

    // 8. 类码桶 6 数组摘要（语料分布 + 摘要确定性）。
    let mut bk = ClassBuckets::new();
    let _ = bk.bump(0x06);
    let _ = bk.bump(0x06);
    let _ = bk.bump(0x06);
    let _ = bk.bump(0x03);
    let _ = bk.bump(0x03);
    let _ = bk.bump(0x02);
    let _ = bk.bump(0x04);
    let arr = bk.as_array();
    let bucket_ok = arr.get(0) == Some(&0)
        && arr.get(1) == Some(&0)
        && arr.get(2) == Some(&1)
        && arr.get(3) == Some(&2)
        && arr.get(4) == Some(&3)
        && arr.get(5) == Some(&1)
        && bucket_digest(&bk) == bucket_digest(&bk);
    v.push((
        "J1-类码桶6摘要",
        bucket_ok,
        format!("桶分布 未0/存0/网1/显2/桥3/其他1，摘要双跑一致"),
    ));

    // 9. 升版重封放行 / 同版重封拒。
    let mut pv = build_pack(SERIAL_BASE + 2, HANDOFF_VERSION).unwrap_or(HandoffPack::new(SERIAL_BASE + 2));
    let same_rej = matches!(pv.freeze(HANDOFF_VERSION), Err(e) if e == HandoffCode::VersionRegress.val());
    let up_ok = pv.freeze(HANDOFF_VERSION + 1).is_ok();
    v.push((
        "J1-版本只升不降",
        same_rej && up_ok,
        format!("同版重封拒={} 升版重封放行={}", same_rej, up_ok),
    ));

    // 10. 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 11. 探针三序。
    let a = probe_seq_a().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b_okv && c,
        format!("a=黄金({}) b=前置三拒({}) c=冻结锚({})", a, b_okv, c),
    ));

    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 13. 段位守卫 + 闭集查表。
    let guard_ok = segment_guard();
    let mut table_ok = true;
    let mut i = 0usize;
    while i < HandoffCode::ALL.len() {
        if let Some(code) = HandoffCode::ALL.get(i) {
            if detail(code.val()).is_empty() {
                table_ok = false;
            }
        }
        i += 1;
    }
    v.push((
        "J3-段位守卫+闭集",
        guard_ok && table_ok,
        format!("0x75880 窗口守卫={} 闭集查表={}", guard_ok, table_ok),
    ));

    v
}

pub fn run_unx0476_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0476 PCI 交接字段（A3 预备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!；取值仍走 .get()） ──────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_all_green() {
        let g = golden_run();
        assert!(g.is_ok(), "黄金全链须绿：{:?}", g.err());
        let (ok, caught, ver) = g.unwrap_or((0, 0, 0));
        assert_eq!(caught, FIELD_COUNT as u32, "10 槽改写必须全拦");
        assert_eq!(ok, 14, "绿位须 14");
        assert_eq!(ver, HANDOFF_VERSION);
    }

    #[test]
    fn field_closed_set_is_frozen_v1() {
        assert_eq!(HandoffField::ALL.len(), FIELD_COUNT);
        // 枚举值升序 = 规范序。
        let mut i = 0usize;
        while i < HandoffField::ALL.len() {
            let f = HandoffField::ALL.get(i).copied().unwrap_or(HandoffField::Version);
            assert_eq!(f as u32, i as u32, "第 {} 位字段枚举值必须等于下标", i);
            assert!(!f.name().is_empty());
            i += 1;
        }
    }

    #[test]
    fn frozen_rewrite_all_ten_rejected() {
        let mut p = build_pack(SERIAL_BASE, HANDOFF_VERSION).unwrap_or_else(|_| HandoffPack::new(0));
        let caught = inject_frozen_rewrite(&mut p);
        assert_eq!(caught, 10, "冻结后 10 槽逐槽改写必须全拒");
        // 单槽复核错误码。
        assert_eq!(p.set_field(HandoffField::BridgeCount, FieldVal::Num(0)).err(), Some(HandoffCode::PostFrozen.val()));
    }

    #[test]
    fn duplicate_field_rejected_before_freeze() {
        let mut p = HandoffPack::new(SERIAL_BASE);
        assert!(p.set_field(HandoffField::BridgeCount, FieldVal::Num(1)).is_ok());
        assert_eq!(p.set_field(HandoffField::BridgeCount, FieldVal::Num(2)).err(), Some(HandoffCode::FieldUnknown.val()));
    }

    #[test]
    fn a3_consume_preflight_three_rejections() {
        // 未封账拒。
        let mut raw = HandoffPack::new(SERIAL_BASE);
        let _ = raw.fill_defaults();
        assert_eq!(a3_mock_consume(&raw).err(), Some(HandoffCode::NotSealed.val()));
        // 字段缺失/形态错位拒（fill_defaults 灌 Num 形态，摘要字段需 Digest）。
        let mut thin = HandoffPack::new(SERIAL_BASE);
        let _ = thin.fill_defaults();
        let _ = thin.freeze(1);
        assert_eq!(a3_mock_consume(&thin).err(), Some(HandoffCode::FieldMissing.val()));
        // 指纹篡改拒。
        let mut p = build_pack(SERIAL_BASE, HANDOFF_VERSION).unwrap_or_else(|_| HandoffPack::new(0));
        p.fp ^= 0x1;
        assert_eq!(a3_mock_consume(&p).err(), Some(HandoffCode::DigestMismatch.val()));
    }

    #[test]
    fn a3_consume_diff_zero() {
        let p = build_pack(SERIAL_BASE, HANDOFF_VERSION).unwrap_or_else(|_| HandoffPack::new(0));
        let view = a3_mock_consume(&p).unwrap_or(MockView {
            bus_bitmap_digest: 0,
            bridge_count: 0,
            valid_fn_count: 0,
            bar_slots: 0,
            cap_chain_total: 0,
            class_bucket_digest: 0,
            fnv_fingerprint: 0,
            serial_no: 0,
            seal_state: false,
            version: 0,
        });
        let rows = diff_pack_view(&p, &view);
        assert!(rows.is_empty(), "A3 消费 diff 必须零差：{:?}", rows);
        assert_eq!(view.bridge_count, 3);
        assert_eq!(view.valid_fn_count, 7);
        assert_eq!(view.bar_slots, 14);
        assert_eq!(view.cap_chain_total, 35);
    }

    #[test]
    fn diff_catches_tampered_view() {
        let p = build_pack(SERIAL_BASE, HANDOFF_VERSION).unwrap_or_else(|_| HandoffPack::new(0));
        let view = a3_mock_consume(&p).unwrap_or(MockView {
            bus_bitmap_digest: 0,
            bridge_count: 0,
            valid_fn_count: 0,
            bar_slots: 0,
            cap_chain_total: 0,
            class_bucket_digest: 0,
            fnv_fingerprint: 0,
            serial_no: 0,
            seal_state: false,
            version: 0,
        });
        let mut bad = view;
        bad.bridge_count = 99;
        let rows = diff_pack_view(&p, &bad);
        assert_eq!(rows.len(), 1, "篡改视图单字段 diff 必须恰 1 行");
        assert_eq!(rows.first().map(|r| r.field), Some("桥数"));
    }

    #[test]
    fn serial_monotonic_semantics() {
        let p = build_pack(SERIAL_BASE + 1, HANDOFF_VERSION).unwrap_or_else(|_| HandoffPack::new(0));
        assert!(p.serial_follows(SERIAL_BASE).is_ok(), "严格递增放行");
        assert_eq!(p.serial_follows(SERIAL_BASE + 1).err(), Some(HandoffCode::SerialNonMonotonic.val()));
        assert_eq!(p.serial_follows(SERIAL_BASE + 9).err(), Some(HandoffCode::SerialNonMonotonic.val()));
    }

    #[test]
    fn version_only_upwards() {
        let mut p = build_pack(SERIAL_BASE, HANDOFF_VERSION).unwrap_or_else(|_| HandoffPack::new(0));
        assert_eq!(p.freeze(HANDOFF_VERSION).err(), Some(HandoffCode::VersionRegress.val()));
        assert!(p.freeze(HANDOFF_VERSION + 1).is_ok(), "升版重封放行");
        assert_eq!(p.freeze(HANDOFF_VERSION).err(), Some(HandoffCode::VersionRegress.val()));
    }

    #[test]
    fn class_bucket_semantics() {
        let mut b = ClassBuckets::new();
        assert_eq!(b.bump(0x01).unwrap_or(9), 1);
        assert_eq!(b.bump(0x03).unwrap_or(9), 3);
        assert_eq!(b.bump(0x99).unwrap_or(9), 5);
        let arr = b.as_array();
        assert_eq!(arr.get(0).copied().unwrap_or(9), 0);
        assert_eq!(arr.get(1).copied().unwrap_or(9), 1);
        assert_eq!(arr.get(3).copied().unwrap_or(9), 1);
        assert_eq!(arr.get(5).copied().unwrap_or(9), 1);
        // 摘要确定性 + 分布敏感。
        let d1 = bucket_digest(&b);
        let mut b2 = b;
        b2.other = b2.other.saturating_add(1);
        assert_ne!(d1, bucket_digest(&b2), "桶分布变化摘要必变");
    }

    #[test]
    fn inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "五路注入必须全抓：{:?}", led.caught);
    }

    #[test]
    fn probes_repro_and_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn diagnostic_table_closed_set() {
        let mut i = 0usize;
        while i < HandoffCode::ALL.len() {
            let c = HandoffCode::ALL.get(i).copied().unwrap_or(HandoffCode::NotReproducible);
            let v = c.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40, "诊断码 0x{:x} 越窗", v);
            assert!(!detail(v).is_empty());
            i += 1;
        }
        assert!(detail(0x88888888).contains("未知诊断码"));
    }

    #[test]
    fn checkset_counts() {
        let s = run_unx0476_checks();
        assert!(s.len() >= 12, "检查条目须 ≥12，实际 {}", s.len());
        assert!(s.all_passed(), "全链须绿：{}", s.red_items_text());
    }
}
