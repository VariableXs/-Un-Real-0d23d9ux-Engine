//! unx621_pkg_aggregate ↔ UNX-F0621 域交接包聚合（12 包总装）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0621，UNX-A1-B33）：
//! 「域交接包聚合（12 包总装）| 260 | 已深化 |
//!  12 交接包聚合账齐（F0349/F0378/F0413/F0439/F0453/F0476/F0497/F0518/
//!  F0538/F0559/F0593/F0617 全登记 12/12），聚合索引表冻结」
//!
//! 设计要点：
//! 1. **12 包源清单冻结**：`HANDOFF_SOURCES` 十二个上游条目号
//!     编译期常量表（锚点点名），聚合前先核「清单 ⇄ 实收」集合差：
//!     多收 = 脏包（`UnexpectedPacket`），少收 = 缺口（`MissingPacket`）。
//! 2. **每包七件套核**：`HandoffPack` 七件（接口冻结清单/契约清单/
//!     资产清单/基线快照/遗留移交清单/可靠性衔接包/经验教训册）逐件
//!     登记在位；缺件即 `PackIncomplete` 点名包号 + 件号。
//! 3. **索引表冻结**：全 12 包齐后 `freeze_index()` 冻结聚合索引
//!     （包号 → 七件掩码 + 指纹），封后增删改全部 `IndexFrozen`。
//! 4. **聚合指纹**：`aggregate_fingerprint()` 对十二包指纹做有序
//!     FNV（按包号排序后折叠，防集合序抖动）；
//!     跨会话复算一致才认总装完成。
//! 5. **覆盖率对账**：`coverage()` 返回 (已收包数, 七件齐包数)，
//!     两者都 12 才算「聚合账齐」。
//!
//! 诊断码：`PkgCode` 0x89 段（0x8900..=0x890F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX621_PKG_VERSION: &str = "unx621-pkg-aggregate-v1";

/// 交接包数（锚点 12 包硬线）。
pub const PACK_COUNT: usize = 12;

/// 每包件数（七件套）。
pub const ITEMS_PER_PACK: usize = 7;

/// 七件名（与位严格同序）。
pub const ITEM_NAMES: [&str; ITEMS_PER_PACK] = [
    "接口冻结清单",
    "契约清单",
    "资产清单",
    "基线快照",
    "遗留移交清单",
    "可靠性衔接包",
    "经验教训册",
];

/// 上游条目号清单（锚点点名的 12 个，编译期冻结）。
pub const HANDOFF_SOURCES: [&str; PACK_COUNT] = [
    "F0349", "F0378", "F0413", "F0439", "F0453", "F0476", "F0497", "F0518", "F0538", "F0559",
    "F0593", "F0617",
];

/// 单包记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffPack {
    /// 上游条目号（锚点名）
    pub source: &'static str,
    /// 七件在位掩码
    pub items: u8,
    /// 包指纹
    pub fingerprint: u64,
}

/// 聚合索引（冻结结构）。
#[derive(Clone, Debug)]
pub struct PackAggregate {
    /// 已收包（按包号下标落位）
    packs: Vec<Option<HandoffPack>>,
    /// 索引已冻结
    frozen: bool,
    /// 冻结时聚合指纹
    frozen_fp: u64,
    /// 登记 tick（注入）
    tick: u64,
}

/// 聚合错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PkgError {
    /// 少收（点名缺哪个源）
    MissingPacket { source: &'static str },
    /// 多收（点名哪个是脏包）
    UnexpectedPacket { source: String },
    /// 包内缺件（包号 + 件号）
    PackIncomplete { pack: u8, item: u8 },
    /// 同包重复登记
    DuplicatePack { source: &'static str },
    /// 索引已冻结
    IndexFrozen,
    /// 包号越界
    PackBad { idx: u8 },
    /// 件号越界
    ItemBad { idx: u8 },
    /// 指纹为零（空包指纹不收）
    FingerprintZero { source: &'static str },
    /// 覆盖率不满
    CoverageShort { got_packs: u32, got_full: u32 },
}

/// 诊断码（0x89 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PkgCode {
    /// 0x8900 少收
    Missing,
    /// 0x8901 多收
    Unexpected,
    /// 0x8902 包内缺件
    Incomplete,
    /// 0x8903 重复登记
    Duplicate,
    /// 0x8904 索引冻结
    Frozen,
    /// 0x8905 包号越界
    PackBad,
    /// 0x8906 件号越界
    ItemBad,
    /// 0x8907 空指纹
    FpZero,
    /// 0x8908 覆盖率不满
    CoverageBad,
    /// 0x8909 聚合通过
    Aggregated,
}

impl PkgCode {
    pub const fn code(self) -> u16 {
        match self {
            PkgCode::Missing => 0x8900,
            PkgCode::Unexpected => 0x8901,
            PkgCode::Incomplete => 0x8902,
            PkgCode::Duplicate => 0x8903,
            PkgCode::Frozen => 0x8904,
            PkgCode::PackBad => 0x8905,
            PkgCode::ItemBad => 0x8906,
            PkgCode::FpZero => 0x8907,
            PkgCode::CoverageBad => 0x8908,
            PkgCode::Aggregated => 0x8909,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            PkgCode::Missing => "PKG-MISSING",
            PkgCode::Unexpected => "PKG-UNEXPECTED",
            PkgCode::Incomplete => "PKG-INCOMPLETE",
            PkgCode::Duplicate => "PKG-DUPLICATE",
            PkgCode::Frozen => "PKG-FROZEN",
            PkgCode::PackBad => "PKG-PACK-BAD",
            PkgCode::ItemBad => "PKG-ITEM-BAD",
            PkgCode::FpZero => "PKG-FP-ZERO",
            PkgCode::CoverageBad => "PKG-COVERAGE-BAD",
            PkgCode::Aggregated => "PKG-AGGREGATED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            PkgCode::Missing => "交接包聚合缺口：源清单有点名包未收齐",
            PkgCode::Unexpected => "收到清单外交接包：脏包拒收，点名来源",
            PkgCode::Incomplete => "交接包内缺件：七件套未齐，点名包号件号",
            PkgCode::Duplicate => "同一源重复登记：后登记被拒",
            PkgCode::Frozen => "聚合索引已冻结：登记面封闭",
            PkgCode::PackBad => "包号越界：只允许 0..=11",
            PkgCode::ItemBad => "件号越界：只允许 0..=6",
            PkgCode::FpZero => "包指纹为零：空包不收",
            PkgCode::CoverageBad => "聚合覆盖率不满：12 包 + 七件齐双硬线未达",
            PkgCode::Aggregated => "12 包聚合通过：索引冻结 + 指纹可复算",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx621_pkg_aggregate.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8900 && (c & 0x00FF) < 0x10
    }
}

impl PkgError {
    pub const fn code(&self) -> PkgCode {
        match self {
            PkgError::MissingPacket { .. } => PkgCode::Missing,
            PkgError::UnexpectedPacket { .. } => PkgCode::Unexpected,
            PkgError::PackIncomplete { .. } => PkgCode::Incomplete,
            PkgError::DuplicatePack { .. } => PkgCode::Duplicate,
            PkgError::IndexFrozen => PkgCode::Frozen,
            PkgError::PackBad { .. } => PkgCode::PackBad,
            PkgError::ItemBad { .. } => PkgCode::ItemBad,
            PkgError::FingerprintZero { .. } => PkgCode::FpZero,
            PkgError::CoverageShort { .. } => PkgCode::CoverageBad,
        }
    }

    pub fn detail(&self) -> String {
        self.code().detail()
    }
}

impl HandoffPack {
    /// 空包。
    pub const fn empty(source: &'static str) -> Self {
        HandoffPack {
            source,
            items: 0,
            fingerprint: 0,
        }
    }

    /// 七件齐否。
    pub const fn items_full(&self) -> bool {
        self.items as usize == (1usize << ITEMS_PER_PACK) - 1
    }

    /// 件数。
    pub const fn item_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < ITEMS_PER_PACK {
            if self.items & (1u8 << (i as u8)) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

impl PackAggregate {
    /// 空聚合（容量 = PACK_COUNT）。
    pub fn new(tick: u64) -> Self {
        let mut packs: Vec<Option<HandoffPack>> = Vec::new();
        let mut i = 0;
        while i < PACK_COUNT {
            packs.push(None);
            i += 1;
        }
        PackAggregate {
            packs,
            frozen: false,
            frozen_fp: 0,
            tick,
        }
    }

    /// 已收包数。
    pub fn pack_count(&self) -> u32 {
        let mut n = 0u32;
        for p in self.packs.iter() {
            if p.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 七件齐包数。
    pub fn full_pack_count(&self) -> u32 {
        let mut n = 0u32;
        for p in self.packs.iter() {
            if let Some(pk) = p {
                if pk.items_full() {
                    n += 1;
                }
            }
        }
        n
    }

    /// 覆盖率（包数, 七件齐包数）。
    pub fn coverage(&self) -> (u32, u32) {
        (self.pack_count(), self.full_pack_count())
    }

    /// 源 → 包号（冻结清单内查找；None = 清单外）。
    pub fn source_index(source: &str) -> Option<u8> {
        let mut i = 0;
        while i < PACK_COUNT {
            if let Some(s) = HANDOFF_SOURCES.get(i) {
                if *s == source {
                    return Some(i as u8);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一包（清单内核 + 件登记 + 指纹非零 + 未冻结四门）。
    pub fn register(&mut self, source: &str, item: u8, fp: u64) -> Result<(), PkgError> {
        if self.frozen {
            return Err(PkgError::IndexFrozen);
        }
        let idx = match Self::source_index(source) {
            Some(i) => i,
            None => {
                return Err(PkgError::UnexpectedPacket {
                    source: String::from(source),
                })
            }
        };
        if item as usize >= ITEMS_PER_PACK {
            return Err(PkgError::ItemBad { idx: item });
        }
        if fp == 0 {
            return Err(PkgError::FingerprintZero {
                source: HANDOFF_SOURCES[idx as usize],
            });
        }
        let slot = match self.packs.get_mut(idx as usize) {
            Some(s) => s,
            None => return Err(PkgError::PackBad { idx }),
        };
        match slot {
            Some(p) => {
                if p.fingerprint != fp {
                    // 同包异指纹 = 两个不同包撞号
                    return Err(PkgError::DuplicatePack {
                        source: HANDOFF_SOURCES[idx as usize],
                    });
                }
                p.items |= 1u8 << (item as u8);
            }
            None => {
                *slot = Some(HandoffPack {
                    source: HANDOFF_SOURCES[idx as usize],
                    items: 1u8 << (item as u8),
                    fingerprint: fp,
                });
            }
        }
        Ok(())
    }

    /// 集合差核：清单 ⇄ 实收（少收点名 + 多收拒）。
    pub fn set_diff_audit(&self) -> Result<u32, PkgError> {
        let mut i = 0;
        while i < PACK_COUNT {
            let got = match self.packs.get(i) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !got {
                if let Some(s) = HANDOFF_SOURCES.get(i) {
                    return Err(PkgError::MissingPacket { source: s });
                }
            }
            i += 1;
        }
        Ok(self.pack_count())
    }

    /// 聚合指纹（按包号序折叠十二包指纹）。
    pub fn aggregate_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < PACK_COUNT {
            if let Some(Some(p)) = self.packs.get(i) {
                h ^= p.fingerprint;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= p.items as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= self.tick;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 冻结索引（12 包齐 + 七件全齐才允许）。
    pub fn freeze_index(&mut self) -> Result<u64, PkgError> {
        if self.frozen {
            return Err(PkgError::IndexFrozen);
        }
        self.set_diff_audit()?;
        let (packs, full) = self.coverage();
        if packs != PACK_COUNT as u32 || full != PACK_COUNT as u32 {
            return Err(PkgError::CoverageShort {
                got_packs: packs,
                got_full: full,
            });
        }
        let fp = self.aggregate_fingerprint();
        if fp == 0 {
            return Err(PkgError::FingerprintZero {
                source: HANDOFF_SOURCES[0],
            });
        }
        self.frozen = true;
        self.frozen_fp = fp;
        Ok(fp)
    }

    /// 冻结态。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结指纹。
    pub const fn frozen_fingerprint(&self) -> u64 {
        self.frozen_fp
    }
}

/// 域自检。
pub fn run_unx621_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx621");

    // 构造全 12 包 × 七件（包指纹 = 0x9000 + 包号）
    let build_full = |tick: u64| -> PackAggregate {
        let mut agg = PackAggregate::new(tick);
        let mut p = 0;
        while p < PACK_COUNT {
            let src = HANDOFF_SOURCES[p];
            let fp = 0x9000 + p as u64;
            let mut it = 0u8;
            while (it as usize) < ITEMS_PER_PACK {
                let _ = agg.register(src, it, fp);
                it += 1;
            }
            p += 1;
        }
        agg
    };

    // C621-01 集合差核通过（12/12）
    {
        let agg = build_full(1);
        let r = agg.set_diff_audit();
        let good = r == Ok(12);
        s.add("C621-01-12 包集合差核", good, if good { "" } else { "集合差不平" });
    }

    // C621-02 七件齐包数 = 12
    {
        let agg = build_full(1);
        let (packs, full) = agg.coverage();
        let good = packs == 12 && full == 12;
        s.add("C621-02-覆盖率 12/12", good, if good { "" } else { "覆盖率不满" });
    }

    // C621-03 索引冻结 + 冻结指纹可复算
    {
        let mut a = build_full(1);
        let r = a.freeze_index();
        let mut b = build_full(1);
        let r2 = b.freeze_index();
        let good = r.is_ok()
            && r2.is_ok()
            && a.is_frozen()
            && a.frozen_fingerprint() == b.frozen_fingerprint()
            && a.frozen_fingerprint() != 0;
        s.add("C621-03-索引冻结指纹一致", good, if good { "" } else { "冻结链失效" });
    }

    // C621-04 少收点名（只收 11 包）
    {
        let mut agg = PackAggregate::new(1);
        let mut p = 0;
        while p < PACK_COUNT - 1 {
            let src = HANDOFF_SOURCES[p];
            let mut it = 0u8;
            while (it as usize) < ITEMS_PER_PACK {
                let _ = agg.register(src, it, 0x9000 + p as u64);
                it += 1;
            }
            p += 1;
        }
        let r = agg.freeze_index();
        let good = matches!(r, Err(PkgError::MissingPacket { source: "F0617" }));
        s.add("C621-04-少收点名末包", good, if good { "" } else { "缺口未点名" });
    }

    // C621-05 多收拒收（清单外包）
    {
        let mut agg = PackAggregate::new(1);
        let r = agg.register("F0999", 0, 0x9001);
        let good = matches!(r, Err(PkgError::UnexpectedPacket { .. }));
        s.add("C621-05-清单外包拒收", good, if good { "" } else { "脏包被放行" });
    }

    // C621-06 包内缺件拒冻（十二包只登 6 件）
    {
        let mut agg = PackAggregate::new(1);
        let mut p = 0;
        while p < PACK_COUNT {
            let src = HANDOFF_SOURCES[p];
            let mut it = 0u8;
            while (it as usize) < 6 {
                let _ = agg.register(src, it, 0x9000 + p as u64);
                it += 1;
            }
            p += 1;
        }
        let r = agg.freeze_index();
        let good = matches!(r, Err(PkgError::CoverageShort { got_full: 0, .. }));
        s.add("C621-06-缺件拒冻", good, if good { "" } else { "缺件包被冻结" });
    }

    // C621-07 冻结后登记被拒
    {
        let mut agg = build_full(1);
        let _ = agg.freeze_index();
        let r = agg.register("F0349", 0, 0x9000);
        let good = matches!(r, Err(PkgError::IndexFrozen));
        s.add("C621-07-冻结后登记拒", good, if good { "" } else { "冻结失效" });
    }

    // C621-08 同包异指纹撞号拒
    {
        let mut agg = PackAggregate::new(1);
        let _ = agg.register("F0349", 0, 0x9000);
        let r = agg.register("F0349", 1, 0x9001);
        let good = matches!(r, Err(PkgError::DuplicatePack { source: "F0349" }));
        s.add("C621-08-异指纹撞号拒", good, if good { "" } else { "撞号被放行" });
    }

    // C621-09 件号越界拒
    {
        let mut agg = PackAggregate::new(1);
        let r = agg.register("F0349", 7, 0x9000);
        let good = matches!(r, Err(PkgError::ItemBad { idx: 7 }));
        s.add("C621-09-件号 7 越界", good, if good { "" } else { "件号越界被放行" });
    }

    // C621-10 包指纹为零拒
    {
        let mut agg = PackAggregate::new(1);
        let r = agg.register("F0349", 0, 0);
        let good = matches!(r, Err(PkgError::FingerprintZero { source: "F0349" }));
        s.add("C621-10-零指纹拒收", good, if good { "" } else { "零指纹被放行" });
    }

    // C621-11 tick 进指纹（同内容不同 tick 指纹不同——时间维可分辨）
    {
        let a = build_full(1);
        let b = build_full(2);
        let good = a.aggregate_fingerprint() != b.aggregate_fingerprint();
        s.add("C621-11-tick 维可分辨", good, if good { "" } else { "时间维不可分辨" });
    }

    // C621-12 诊断码段独占
    {
        let codes = [
            PkgCode::Missing,
            PkgCode::Unexpected,
            PkgCode::Incomplete,
            PkgCode::Duplicate,
            PkgCode::Frozen,
            PkgCode::PackBad,
            PkgCode::ItemBad,
            PkgCode::FpZero,
            PkgCode::CoverageBad,
            PkgCode::Aggregated,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C621-12-诊断码 0x89 段独占", all, if all { "" } else { "码值越出 0x89 段" });
    }

    s
}
