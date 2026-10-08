//! VE-F0227 · Intel 媒体引擎能力位（VE-B 域 · Intel 核显组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0227`
//!
//! **判据（锚点原文）**：实例枚举、只读呈现、保守下限、不宣称、判据。
//!
//! **职责定位（锚点原文）**：VDENC 与 VDBOX 实例枚举、编解码能力矩阵
//! （H264/HEVC/AV1 按代际）与分辨率上限探测；能力位只读呈现给上层媒体栈，
//! 明确声明不做编解码本体（范围红线）；读取失败按代际保守下限并标注。
//!
//! ## 一、能力位是「探测+查表」，探测只跑一次
//!
//! 实例与能力都在硬件寄存器里，但寄存器不会自己变——探测一次性
//! （[`MediaCapsProbe`] 的 `probe_runs` 恒 1），之后所有查询走常驻缓存
//! O(1)（锚点性能口径）。反复探测不仅浪费，还会把「读数抖动」升级成
//! 「能力抖动」：同一编解码一会儿宣称一会儿不宣称，上层媒体栈没法写。
//!
//! ## 二、「缺席」与「读取失败」是两种病，两种治法
//!
//! - **实例不可用→标记缺席不假设存在**：VDENC 数 0 就是无此引擎，
//!   查询给缺席结论——把缺席当「存在但未测」是能力位撒谎的第一形式；
//! - **能力读取失败→按代际保守下限并标注**：寄存器读不出来时给该
//!   代际的**下限档**能力并打标注（[`Claim::ConservativeFloor`]）——
//!   保守下限不是「当它不存在」：Gen9 基线下限里 H264 是确定有的，
//!   读失败就说没有同样是撒谎（第二形式）。两种病分开治，各自的
//!   降级路径才能各自被测试钉住。
//! - **未知编解码→不宣称**（[`Claim::NotClaimed`]）：能力位的默认值
//!   必须是「不宣称」——宣称是枚举出来的，不是缺省出来的。
//!
//! ## 三、能力矩阵同源 F0221 的代际纪律
//!
//! 矩阵按 [`GenTier`]（上游 F0221 代际分型）查表，口径**逐字沿用**
//! F0221 的「Baseline 新特性位一律不给」：Gen9 基线档不给 AV1——哪怕
//! 部分晚批 Gen11 实物有 AV1 解码，基线档的承诺面就是全代可依赖面，
//! 按最优批给承诺等于让下位机用户踩空。分辨率上限逐档递进
//! （4K→8K），数值显性化为常量并接受独立重排对账。
//!
//! ## 四、只读呈现与范围红线
//!
//! 下游媒体栈拿到的是 [`MediaCapsView`]——一个没有写入口的呈现结构，
//! 外加 [`SCOPE_DOC`] 范围声明：**本模块不做编解码本体**。能力位与
//! 编解码实现之间隔着一条范围红线：能力位说谎（虚报）会让媒体栈在
//! 不存在的硬件能力上建大厦；编解码本体混进能力位则让探测路径背上
//! 编解码的全部复杂度。两头都堵死：宣称全部来自查表，表外无能力。
//!
//! **对接**：上游 F0221 代际分型（[`GenTier`] 同源引用）；下游媒体栈
//! （范围外，只读呈现）与 F0238 版本差异表引用。
//! 零 panic 面（固定下标走 `get`/`Option`，算术全饱和）、零 IO、零墙钟、
//! 无全局可变状态、no_std 零 std 依赖。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veb21_ident::GenTier;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源；代际口径同源 F0221）
// ---------------------------------------------------------------------------

/// VDBOX（视频编解码盒）实例数上限（按代际物理上界取保守值）。
pub const MAX_VDBOX_INSTANCES: usize = 4;
/// VDENC（专用编码引擎）实例数上限。
pub const MAX_VDENC_INSTANCES: usize = 2;

/// 编解码种类数（H264 / HEVC / AV1；锚点原文三件）。
pub const CODEC_COUNT: usize = 3;

/// 分辨率上限：4K 级（3840×2160）。
pub const RES_4K_W: u32 = 3840;
/// 4K 高。
pub const RES_4K_H: u32 = 2160;
/// 分辨率上限：8K 级（8192×4320）。
pub const RES_8K_W: u32 = 8192;
/// 8K 高。
pub const RES_8K_H: u32 = 4320;

/// 范围红线声明（锚点原文：明确声明不做编解码本体）。
pub const SCOPE_DOC: &str = "\
范围红线（VE-F0227）：本模块只做媒体引擎能力位探测与只读呈现，\
明确声明不做编解码本体——解码/编码执行、码率控制、Surface 管理均属\
媒体栈（范围外）。能力位虚报会让媒体栈在不存在的能力上建大厦，\
编解码本体混入会让探测路径背上编解码的全部复杂度，两头都堵死：\
宣称全部来自查表，表外无能力。";

/// 代际能力下限表（**独立重排对账源**；口径同源 F0221「Baseline 新特性位
/// 一律不给」）。三元组：H264 / HEVC / AV1 的 (decode, encode, 分辨率档)。
///
/// - Baseline（Gen9/9.5/11 基线）：H264/HEVC 编解码 4K；AV1 双无；
/// - XeStandard：+AV1 解码；HEVC 解码升 8K（编码仍 4K）；
/// - XeLatest：+AV1 编码；HEVC 编解码 8K。
pub const FLOOR_TABLE_DOCS: &str = "\
基线档：H264 编解码 4K、HEVC 编解码 4K、AV1 不宣称（F0221 基线纪律）；\
Xe 标准档：+AV1 解码 8K、HEVC 解码 8K；Xe 最新档：+AV1 编码 8K、\
HEVC 编解码 8K。读取失败按本表保守下限并标注（ConservativeFloor）。";

// ---------------------------------------------------------------------------
// 二、数据结构（锚点：能力位表（实例×编解码×上限）；探测结果缓存）
// ---------------------------------------------------------------------------

/// 媒体引擎种类（锚点原文两件：VDENC 与 VDBOX）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaEngineKind {
    /// VDBOX：视频解码/编码盒。
    Vdbox,
    /// VDENC：专用编码引擎。
    Vdenc,
}

impl MediaEngineKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            MediaEngineKind::Vdbox => "VDBOX",
            MediaEngineKind::Vdenc => "VDENC",
        }
    }
}

/// 编解码种类（锚点原文三件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// H264。
    H264,
    /// HEVC。
    Hevc,
    /// AV1。
    Av1,
}

impl Codec {
    /// 三件全集（缺一件矩阵即盲区，自检逐格核对）。
    pub const ALL: [Codec; CODEC_COUNT] = [Codec::H264, Codec::Hevc, Codec::Av1];

    /// 数组下标。
    pub const fn ordinal(self) -> usize {
        match self {
            Codec::H264 => 0,
            Codec::Hevc => 1,
            Codec::Av1 => 2,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H264",
            Codec::Hevc => "HEVC",
            Codec::Av1 => "AV1",
        }
    }
}

/// 单编解码能力格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecCaps {
    /// 可解码。
    pub decode: bool,
    /// 可编码。
    pub encode: bool,
    /// 分辨率上限宽（像素）。
    pub max_w: u32,
    /// 分辨率上限高（像素）。
    pub max_h: u32,
}

/// 探测注入输入（硬件读数的确定性替身；真实 MMIO 读在 B 域硬件面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeInputs {
    /// VDBOX 实例数（0 = 缺席）。
    pub vdbox_count: usize,
    /// VDENC 实例数（0 = 缺席）。
    pub vdenc_count: usize,
    /// 逐编解码能力寄存器是否可读（false = 该编解码走保守下限并标注）。
    pub codec_readable: [bool; CODEC_COUNT],
}

impl ProbeInputs {
    /// 全可读的标称输入。
    pub const fn nominal(vdbox: usize, vdenc: usize) -> ProbeInputs {
        ProbeInputs {
            vdbox_count: vdbox,
            vdenc_count: vdenc,
            codec_readable: [true, true, true],
        }
    }
}

/// 查询结论（三态：宣称 / 保守下限 / 不宣称——默认必须是不宣称）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    /// 探测确认，宣称。
    Claimed,
    /// 读取失败，按代际保守下限给值并标注。
    ConservativeFloor,
    /// 不宣称（缺席/未知/下限表里没有）。
    NotClaimed,
}

/// 编解码能力位表（锚点数据结构「实例×编解码×上限」的编解码维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecRow {
    /// 解码结论。
    pub decode: Claim,
    /// 编码结论。
    pub encode: Claim,
    /// 分辨率上限（与结论配对；NotClaimed 时无意义给 0）。
    pub max_w: u32,
    /// 分辨率上限高。
    pub max_h: u32,
}

// ---------------------------------------------------------------------------
// 三、代际下限表（同源 F0221 口径；探测失败时的落点）
// ---------------------------------------------------------------------------

/// 按代际给编解码下限格（探测确认值也按此表核对——宣称不得超表）。
///
/// 这是「按代际保守下限」的唯一真源：读取失败落到这里，探测成功值
/// 也必须 ≤ 这里（宣称超表即越权，自检钉死）。
pub const fn tier_floor(tier: GenTier, codec: Codec) -> CodecCaps {
    match (tier, codec) {
        // —— Baseline：H264/HEVC 编解码 4K；AV1 双无（F0221 基线纪律）——
        (GenTier::Baseline, Codec::H264) => CodecCaps { decode: true, encode: true, max_w: RES_4K_W, max_h: RES_4K_H },
        (GenTier::Baseline, Codec::Hevc) => CodecCaps { decode: true, encode: true, max_w: RES_4K_W, max_h: RES_4K_H },
        (GenTier::Baseline, Codec::Av1) => CodecCaps { decode: false, encode: false, max_w: 0, max_h: 0 },
        // —— XeStandard：+AV1 解码 8K；HEVC 解码升 8K ——
        (GenTier::XeStandard, Codec::H264) => CodecCaps { decode: true, encode: true, max_w: RES_4K_W, max_h: RES_4K_H },
        (GenTier::XeStandard, Codec::Hevc) => CodecCaps { decode: true, encode: true, max_w: RES_8K_W, max_h: RES_8K_H },
        (GenTier::XeStandard, Codec::Av1) => CodecCaps { decode: true, encode: false, max_w: RES_8K_W, max_h: RES_8K_H },
        // —— XeLatest：+AV1 编码；HEVC 编解码 8K ——
        (GenTier::XeLatest, Codec::H264) => CodecCaps { decode: true, encode: true, max_w: RES_4K_W, max_h: RES_4K_H },
        (GenTier::XeLatest, Codec::Hevc) => CodecCaps { decode: true, encode: true, max_w: RES_8K_W, max_h: RES_8K_H },
        (GenTier::XeLatest, Codec::Av1) => CodecCaps { decode: true, encode: true, max_w: RES_8K_W, max_h: RES_8K_H },
    }
}

// ---------------------------------------------------------------------------
// 四、探测与缓存（判据一：实例枚举；探测一次性，查询 O(1) 常驻缓存）
// ---------------------------------------------------------------------------

/// 媒体能力探测结果（探测一次性产物，常驻缓存）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaCapsTable {
    /// 代际档（上游 F0221 分型结论，同源引用）。
    pub tier: GenTier,
    /// VDBOX 实例编号表（缺席为空表——标记缺席不假设存在）。
    pub vdbox_ids: Vec<u8>,
    /// VDENC 实例编号表。
    pub vdenc_ids: Vec<u8>,
    /// 逐编解码能力位表（下标即 [`Codec::ordinal`]）。
    pub codecs: [CodecRow; CODEC_COUNT],
    /// 保守下限标注账（哪些格被降级到下限，零静默）。
    pub floored: Vec<(&'static str, &'static str)>,
    /// 缺席标注账。
    pub absent: Vec<&'static str>,
}

/// 探测器：探测一次性 + 查询 O(1) 常驻缓存（锚点性能口径）。
#[derive(Clone, Debug)]
pub struct MediaCapsProbe {
    cache: Option<MediaCapsTable>,
    probe_runs: u32,
    events: Vec<String>,
}

impl MediaCapsProbe {
    /// 新建（未探测；任何查询前必须先 probe 一次）。
    pub fn new() -> MediaCapsProbe {
        MediaCapsProbe {
            cache: None,
            probe_runs: 0,
            events: Vec::new(),
        }
    }

    /// 探测（一次性；重复探测拒绝并记账——能力读数不允许抖动）。
    pub fn probe(&mut self, tier: GenTier, inputs: &ProbeInputs) -> bool {
        if self.cache.is_some() {
            self.events
                .push("重复探测被拒：能力位一次成型，反复探测会造成能力抖动".to_string());
            return false;
        }
        self.probe_runs += 1;
        let mut vdbox_ids = Vec::new();
        let mut i = 0u8;
        while (i as usize) < inputs.vdbox_count.min(MAX_VDBOX_INSTANCES) {
            vdbox_ids.push(i);
            i += 1;
        }
        if inputs.vdbox_count == 0 {
            self.events.push("VDBOX 缺席：标记缺席，不假设存在".to_string());
        }
        let mut vdenc_ids = Vec::new();
        let mut j = 0u8;
        while (j as usize) < inputs.vdenc_count.min(MAX_VDENC_INSTANCES) {
            vdenc_ids.push(j);
            j += 1;
        }
        if inputs.vdenc_count == 0 {
            self.events.push("VDENC 缺席：标记缺席，不假设存在".to_string());
        }
        let mut codecs: [CodecRow; CODEC_COUNT] = [
            CodecRow { decode: Claim::NotClaimed, encode: Claim::NotClaimed, max_w: 0, max_h: 0 },
            CodecRow { decode: Claim::NotClaimed, encode: Claim::NotClaimed, max_w: 0, max_h: 0 },
            CodecRow { decode: Claim::NotClaimed, encode: Claim::NotClaimed, max_w: 0, max_h: 0 },
        ];
        let mut floored: Vec<(&'static str, &'static str)> = Vec::new();
        let mut ci = 0usize;
        while ci < CODEC_COUNT {
            let codec = match Codec::ALL.get(ci) {
                Some(c) => *c,
                None => break,
            };
            let floor = tier_floor(tier, codec);
            let readable = match inputs.codec_readable.get(ci) {
                Some(v) => *v,
                None => false,
            };
            // 读得动 → 按下限表宣称（宣称不得超表，见自检）；
            // 读不动但有下限能力 → 保守下限并标注；
            // 表里本来没有 → 不宣称。
            let row = if readable {
                CodecRow {
                    decode: if floor.decode { Claim::Claimed } else { Claim::NotClaimed },
                    encode: if floor.encode { Claim::Claimed } else { Claim::NotClaimed },
                    max_w: floor.max_w,
                    max_h: floor.max_h,
                }
            } else if floor.decode || floor.encode {
                floored.push((codec.label(), "读取失败按代际保守下限"));
                CodecRow {
                    decode: if floor.decode { Claim::ConservativeFloor } else { Claim::NotClaimed },
                    encode: if floor.encode { Claim::ConservativeFloor } else { Claim::NotClaimed },
                    max_w: floor.max_w,
                    max_h: floor.max_h,
                }
            } else {
                // 表里没有且读不动：不宣称（两条降级路径都不适用）。
                CodecRow { decode: Claim::NotClaimed, encode: Claim::NotClaimed, max_w: 0, max_h: 0 }
            };
            codecs[ci] = row;
            ci += 1;
        }
        self.cache = Some(MediaCapsTable {
            tier,
            vdbox_ids,
            vdenc_ids,
            codecs,
            floored,
            absent: Vec::new(),
        });
        true
    }

    /// 探测次数（恒 1 = 一次性纪律的观测面）。
    pub const fn probe_runs(&self) -> u32 {
        self.probe_runs
    }

    /// 事件账（零静默）。
    pub fn events(&self) -> &[String] {
        self.events.as_slice()
    }

    /// 编解码查询（O(1) 常驻缓存）。
    ///
    /// 未知编解码不在 [`Codec::ALL`] 里就无从查起——返回 [`Claim::NotClaimed`]
    /// （不宣称），这是能力位的默认值。
    pub fn codec_query(&self, codec: Codec, want_encode: bool) -> (Claim, u32, u32) {
        let row = match self.cache.as_ref().and_then(|t| t.codecs.get(codec.ordinal())) {
            Some(r) => *r,
            None => return (Claim::NotClaimed, 0, 0),
        };
        let claim = if want_encode { row.encode } else { row.decode };
        if claim == Claim::NotClaimed {
            (Claim::NotClaimed, 0, 0)
        } else {
            (claim, row.max_w, row.max_h)
        }
    }

    /// 实例枚举查询（O(1)；缺席给空表——不假设存在）。
    pub fn instances(&self, kind: MediaEngineKind) -> &[u8] {
        match self.cache.as_ref() {
            Some(t) => match kind {
                MediaEngineKind::Vdbox => t.vdbox_ids.as_slice(),
                MediaEngineKind::Vdenc => t.vdenc_ids.as_slice(),
            },
            None => &[],
        }
    }

    /// 只读呈现（下游媒体栈唯一入口；范围红线见 [`SCOPE_DOC`]）。
    pub fn read_only_view(&self) -> Option<MediaCapsView> {
        match self.cache.as_ref() {
            Some(t) => Some(MediaCapsView::of(t)),
            None => None,
        }
    }
}

impl Default for MediaCapsProbe {
    fn default() -> MediaCapsProbe {
        MediaCapsProbe::new()
    }
}

// ---------------------------------------------------------------------------
// 五、只读呈现（判据二：能力位只读呈现给上层媒体栈；范围红线）
// ---------------------------------------------------------------------------

/// 能力位只读视图（无写入口；编解码本体在范围外）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaCapsView {
    /// 代际档标签。
    pub tier_label: String,
    /// VDBOX 实例数。
    pub vdbox_count: usize,
    /// VDENC 实例数（0 = 缺席）。
    pub vdenc_count: usize,
    /// 逐编解码呈现行。
    pub rows: Vec<(String, Claim, Claim, u32, u32)>,
    /// 保守下限标注数。
    pub floored_count: usize,
    /// 范围声明（随视图走，媒体栈永远看得到红线）。
    pub scope: String,
}

impl MediaCapsView {
    fn of(t: &MediaCapsTable) -> MediaCapsView {
        let mut rows: Vec<(String, Claim, Claim, u32, u32)> = Vec::new();
        let mut ci = 0usize;
        while ci < CODEC_COUNT {
            if let (Some(codec), Some(row)) = (Codec::ALL.get(ci), t.codecs.get(ci)) {
                rows.push((
                    codec.label().to_string(),
                    row.decode,
                    row.encode,
                    row.max_w,
                    row.max_h,
                ));
            }
            ci += 1;
        }
        MediaCapsView {
            tier_label: t.tier.label(),
            vdbox_count: t.vdbox_ids.len(),
            vdenc_count: t.vdenc_ids.len(),
            rows,
            floored_count: t.floored.len(),
            scope: SCOPE_DOC.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（判据区零 panic 面；反向语料钉门禁不恒绿）
// ---------------------------------------------------------------------------

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = [
    "实例枚举",
    "只读呈现",
    "保守下限",
    "不宣称",
    "判据",
];

/// 下限表独立重排（与 `tier_floor` 逐格对账——表被误改先红）。
const FLOOR_RECHECK: [((u8, u8), bool, bool, u32, u32); 9] = [
    // (tier.code, codec.ordinal) : (decode, encode, w, h)
    ((0, 0), true, true, RES_4K_W, RES_4K_H),
    ((0, 1), true, true, RES_4K_W, RES_4K_H),
    ((0, 2), false, false, 0, 0),
    ((1, 0), true, true, RES_4K_W, RES_4K_H),
    ((1, 1), true, true, RES_8K_W, RES_8K_H),
    ((1, 2), true, false, RES_8K_W, RES_8K_H),
    ((2, 0), true, true, RES_4K_W, RES_4K_H),
    ((2, 1), true, true, RES_8K_W, RES_8K_H),
    ((2, 2), true, true, RES_8K_W, RES_8K_H),
];

/// VE-F0227 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_veb227_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("veb227_mediacaps");

    // —— 判据一 · 实例枚举：正常探测逐实例编号，缺席给空表 ——
    let mut p = MediaCapsProbe::new();
    let ok = p.probe(GenTier::XeStandard, &ProbeInputs::nominal(2, 1));
    let vdbox = p.instances(MediaEngineKind::Vdbox);
    let vdenc = p.instances(MediaEngineKind::Vdenc);
    s.add(
        "B227-实例枚举-编号齐全且缺席为空",
        ok
            && p.probe_runs() == 1
            && vdbox == [0u8, 1u8]
            && vdenc == [0u8]
            && !p.probe(GenTier::XeStandard, &ProbeInputs::nominal(2, 1))
            && p.probe_runs() == 1,
        "2×VDBOX+1×VDENC 逐实例枚举；重复探测被拒且计数不涨（能力读数不许抖动）",
    );

    // —— 判据一 · 反向：实例缺席标记缺席，不假设存在 ——
    let mut p2 = MediaCapsProbe::new();
    p2.probe(GenTier::XeStandard, &ProbeInputs::nominal(2, 0));
    let no_vdenc = p2.instances(MediaEngineKind::Vdenc);
    let (enc_claim, _, _) = p2.codec_query(Codec::Av1, true);
    s.add(
        "B227-缺席-标记缺席不假设存在",
        no_vdenc.is_empty()
            && enc_claim == Claim::NotClaimed
            && p2.events().iter().any(|e| e.contains("VDENC 缺席")),
        "VDENC 数 0：实例表空、编码查询不宣称、缺席事件入账（缺席不是未测）",
    );

    // —— 判据三/四 · 能力矩阵按代际：基线不宣称 AV1，逐档递进 ——
    let mut p3 = MediaCapsProbe::new();
    p3.probe(GenTier::Baseline, &ProbeInputs::nominal(1, 1));
    let (av1_dec_b, _, _) = p3.codec_query(Codec::Av1, false);
    let (h264_dec_b, w_b, _) = p3.codec_query(Codec::H264, false);
    let mut p4 = MediaCapsProbe::new();
    p4.probe(GenTier::XeStandard, &ProbeInputs::nominal(1, 1));
    let (av1_dec_s, aw_s, ah_s) = p4.codec_query(Codec::Av1, false);
    let (hevc_dec_s, _, _) = p4.codec_query(Codec::Hevc, false);
    let mut p5 = MediaCapsProbe::new();
    p5.probe(GenTier::XeLatest, &ProbeInputs::nominal(1, 1));
    let (av1_enc_l, _, _) = p5.codec_query(Codec::Av1, true);
    s.add(
        "B227-矩阵-按代际递进且基线不宣称AV1",
        av1_dec_b == Claim::NotClaimed
            && h264_dec_b == Claim::Claimed
            && w_b == RES_4K_W
            && av1_dec_s == Claim::Claimed
            && aw_s == RES_8K_W
            && ah_s == RES_8K_H
            && av1_enc_l == Claim::Claimed
            && hevc_dec_s == Claim::Claimed,
        "基线档 AV1 不宣称（F0221 基线纪律同源）；Xe 标准档 +AV1 解码 8K；Xe 最新档 +AV1 编码",
    );

    // —— 判据三 · 保守下限：读失败落代际下限并标注 ——
    let mut p6 = MediaCapsProbe::new();
    p6.probe(
        GenTier::XeLatest,
        &ProbeInputs { vdbox_count: 1, vdenc_count: 1, codec_readable: [true, false, false] },
    );
    let (hevc_dec, hw, hh) = p6.codec_query(Codec::Hevc, false);
    let (av1_dec, _, _) = p6.codec_query(Codec::Av1, false);
    let view6 = p6.read_only_view();
    s.add(
        "B227-保守下限-读失败降档并标注",
        hevc_dec == Claim::ConservativeFloor
            && hw == RES_8K_W
            && hh == RES_8K_H
            && av1_dec == Claim::ConservativeFloor
            && view6.map(|v| v.floored_count).unwrap_or(0) == 2,
        "HEVC/AV1 读失败都给 Xe 最新档下限 8K 并打 ConservativeFloor 标注（两张标注账）；H264 读得动照常宣称",
    );

    // —— 判据四 · 不宣称：未知/未探测一律不宣称 ——
    let cold = MediaCapsProbe::new();
    let (c1, _, _) = cold.codec_query(Codec::H264, false);
    let warm_notclaimed = (Codec::ALL.len(), CODEC_COUNT);
    s.add(
        "B227-不宣称-默认值就是不宣称",
        c1 == Claim::NotClaimed
            && cold.instances(MediaEngineKind::Vdbox).is_empty()
            && cold.read_only_view().is_none()
            && warm_notclaimed.0 == 3
            && warm_notclaimed.1 == 3,
        "未探测的探测器所有查询给不宣称/空表/无视图——宣称是枚举出来的不是缺省出来的",
    );

    // —— 下限表独立重排逐格对账（表被误改先红；宣称不得超表）——
    let mut floor_ok = FLOOR_RECHECK.len() == 9;
    let tiers = [GenTier::Baseline, GenTier::XeStandard, GenTier::XeLatest];
    let mut ti = 0usize;
    while ti < tiers.len() {
        let tier = match tiers.get(ti) {
            Some(t) => *t,
            None => break,
        };
        let mut ci = 0usize;
        while ci < CODEC_COUNT {
            let codec = match Codec::ALL.get(ci) {
                Some(c) => *c,
                None => break,
            };
            let caps = tier_floor(tier, codec);
            let want = match FLOOR_RECHECK.get(ti * CODEC_COUNT + ci) {
                Some(w) => *w,
                None => {
                    floor_ok = false;
                    break;
                }
            };
            // ordinal() 返回 usize，表内对位元是 u8——按值域（0..3）无损转换后比较。
            if want.0 .0 != tier.code() || want.0 .1 as usize != codec.ordinal() {
                floor_ok = false;
            }
            if want.1 != caps.decode || want.2 != caps.encode || want.3 != caps.max_w || want.4 != caps.max_h {
                floor_ok = false;
            }
            ci += 1;
        }
        ti += 1;
    }
    s.add(
        "B227-下限表-九格独立重排逐格全等",
        floor_ok
            && MAX_VDBOX_INSTANCES == 4
            && MAX_VDENC_INSTANCES == 2
            && RES_4K_W == 3840
            && RES_8K_W == 8192,
        "三档×三编解码九格逐格全等（含 tier code 对位）；分辨率常量显性化",
    );

    // —— 判据二 · 只读呈现：视图带范围声明，无写入口 ——
    let view = p3.read_only_view();
    let v_ok = match view {
        Some(v) => {
            v.tier_label.contains("基线")
                && v.vdbox_count == 1
                && v.vdenc_count == 1
                && v.rows.len() == CODEC_COUNT
                && v.scope.contains("不做编解码本体")
        }
        None => false,
    };
    s.add(
        "B227-只读呈现-视图带范围红线",
        v_ok
            && SCOPE_DOC.contains("只读呈现")
            && SCOPE_DOC.contains("宣称全部来自查表"),
        "视图含代际标签/实例数/三编解码行/范围声明；红线随视图走（媒体栈永远看得到）",
    );

    // —— 判据 stamp 独立对账 ——
    let stamps = ["实例枚举", "只读呈现", "保守下限", "不宣称", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci2 = 0usize;
    while ci2 < stamps.len() {
        if CRITERIA_RECHECK.get(ci2) != Some(&stamps[ci2]) {
            stamp_ok = false;
        }
        ci2 += 1;
    }
    s.add(
        "B227-判据stamp-五条独立重排全等",
        stamp_ok && FLOOR_TABLE_DOCS.contains("保守下限"),
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）",
    );

    s
}
