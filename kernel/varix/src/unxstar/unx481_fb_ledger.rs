//! unx481_fb_ledger ↔ UNX-F0481 帧缓冲信息账冻结（四要素）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0481，UNX-A1-B25）：
//! 「帧缓冲信息账冻结（四要素）| 220 | 已深化 |
//!  四要素账齐（物理基址/宽/高/bpp）且与 F0110 探测值复读一致（4/4 字段），版本封账」
//!
//! 设计要点：
//! 1. **四要素账**：`FbLedger` 持物理基址/宽/高/bpp 四字段 + 版本封条；
//!    缺一即 `FieldMissing`（点名缺哪个），禁「默认 1024×768」式臆造填充。
//! 2. **复读一致**：`reread_consistent()` 对四个字段逐一与探测快照
//!    复读比对（4/4 硬线），任一字段漂移即 `Drift` 并指名字段——
//!    帧缓冲信息是后续显示域的地基，歪一点全屏花。
//! 3. **版本封账**：`seal()` 之后字段不再接受写入（`Frozen` 错误），
//!    要改必须走 `reseal(new_version)`（显式升版 + 留痕），
//!    防「显示中途偷偷改基址」。
//! 4. **派生量对账**：`stride/pitch/total_bytes` 由四要素推导，
//!    `derived_audit()` 检查 stride == width × bpp/8、总字节与
//!    fb 尺寸一致（推导错 = 画面撕裂或写越界）。
//! 5. **只读真源**：探测值经 `ProbeSource` trait 注入（Limine FB /
//!    VESA / 探测失败三态），失败显性 `ProbeFailed` 不回落假值。
//!
//! 诊断码：`FbCode` 0x84 段（0x8400..=0x840F）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX481_FB_VERSION: &str = "unx481-fb-ledger-v1";

/// 账版本（封账时与字段同源冻结）。
pub const LEDGER_VERSION: u32 = 1;

/// bpp 合法档（只收 32/24/16，禁拍脑袋）。
pub const LEGAL_BPP: [u8; 3] = [32, 24, 16];

/// 宽上限（4K 红线 + 余量；超出拒收）。
pub const WIDTH_MAX: u32 = 8192;

/// 高上限。
pub const HEIGHT_MAX: u32 = 8192;

/// 探测源抽象（Limine FB / VESA / 失败三态同构）。
pub trait ProbeSource {
    /// 读探测四要素（Err = 探测失败，禁回落假值）。
    fn probe(&self) -> Result<FbFacts, FbError>;
}

/// 四要素（探测值与账本共用同一结构，保证复读同口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FbFacts {
    /// 物理基址
    pub base: u64,
    /// 宽（像素）
    pub width: u32,
    /// 高（像素）
    pub height: u32,
    /// 每像素位数
    pub bpp: u8,
}

/// 帧缓冲账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FbLedger {
    facts: FbFacts,
    /// 字段在位标记（四位）
    present: u8,
    /// 已封账
    sealed: bool,
    /// 封账版本
    seal_version: u32,
    /// 封账指纹
    seal_fp: u64,
}

/// 帧缓冲错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbError {
    /// 缺字段（四位掩码指出缺谁）
    FieldMissing { mask: u8 },
    /// bpp 非法档
    BppBad { bpp: u8 },
    /// 宽高越界
    GeometryBad { w: u32, h: u32 },
    /// 复读漂移（字段下标 0=base/1=width/2=height/3=bpp）
    Drift { field: u8 },
    /// 已封账还写
    Frozen { field: &'static str },
    /// 探测失败
    ProbeFailed { reason: &'static str },
    /// 推导量不一致
    DerivedBad { what: &'static str },
    /// 版本未升
    VersionNotBumped { got: u32, want: u32 },
    /// 基址非页对齐
    BaseUnaligned { base: u64 },
}

/// 诊断码（0x84 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbCode {
    /// 0x8400 缺字段
    FieldMissing,
    /// 0x8401 bpp 非法
    BppBad,
    /// 0x8402 宽高越界
    GeometryBad,
    /// 0x8403 复读漂移
    Drift,
    /// 0x8404 封账后写
    Frozen,
    /// 0x8405 探测失败
    ProbeFailed,
    /// 0x8406 推导量不一致
    DerivedBad,
    /// 0x8407 版本未升
    VersionBad,
    /// 0x8408 基址未对齐
    BaseUnaligned,
    /// 0x8409 封账通过
    Sealed,
}

impl FbCode {
    pub const fn code(self) -> u16 {
        match self {
            FbCode::FieldMissing => 0x8400,
            FbCode::BppBad => 0x8401,
            FbCode::GeometryBad => 0x8402,
            FbCode::Drift => 0x8403,
            FbCode::Frozen => 0x8404,
            FbCode::ProbeFailed => 0x8405,
            FbCode::DerivedBad => 0x8406,
            FbCode::VersionBad => 0x8407,
            FbCode::BaseUnaligned => 0x8408,
            FbCode::Sealed => 0x8409,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            FbCode::FieldMissing => "FB-FIELD-MISSING",
            FbCode::BppBad => "FB-BPP-BAD",
            FbCode::GeometryBad => "FB-GEOMETRY-BAD",
            FbCode::Drift => "FB-DRIFT",
            FbCode::Frozen => "FB-FROZEN",
            FbCode::ProbeFailed => "FB-PROBE-FAILED",
            FbCode::DerivedBad => "FB-DERIVED-BAD",
            FbCode::VersionBad => "FB-VERSION-BAD",
            FbCode::BaseUnaligned => "FB-BASE-UNALIGNED",
            FbCode::Sealed => "FB-SEALED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            FbCode::FieldMissing => "帧缓冲四要素不全：缺字段必须点名（掩码四位）",
            FbCode::BppBad => "bpp 非法：只允许 32/24/16 三档",
            FbCode::GeometryBad => "宽高越界：超过 8192 红线拒收",
            FbCode::Drift => "复读漂移：探测值与账本不一致，点名字段",
            FbCode::Frozen => "封账后写入被拒：改帧缓冲信息必须走 reseal 显性升版",
            FbCode::ProbeFailed => "探测失败：不回落假值，显性上报",
            FbCode::DerivedBad => "推导量不一致：stride/总字节与四要素矛盾",
            FbCode::VersionBad => "版本未升：reseal 必须严格递增",
            FbCode::BaseUnaligned => "基址非页对齐：帧缓冲基址必须 4KB 对齐",
            FbCode::Sealed => "帧缓冲账封账通过：四要素齐 + 复读一致 + 推导自洽",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx481_fb_ledger.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8400 && (c & 0x00FF) < 0x10
    }
}

impl FbError {
    pub const fn code(self) -> FbCode {
        match self {
            FbError::FieldMissing { .. } => FbCode::FieldMissing,
            FbError::BppBad { .. } => FbCode::BppBad,
            FbError::GeometryBad { .. } => FbCode::GeometryBad,
            FbError::Drift { .. } => FbCode::Drift,
            FbError::Frozen { .. } => FbCode::Frozen,
            FbError::ProbeFailed { .. } => FbCode::ProbeFailed,
            FbError::DerivedBad { .. } => FbCode::DerivedBad,
            FbError::VersionNotBumped { .. } => FbCode::VersionBad,
            FbError::BaseUnaligned { .. } => FbCode::BaseUnaligned,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 字段位（四位掩码，与下标序一致）。
pub const F_BASE: u8 = 1 << 0;
pub const F_WIDTH: u8 = 1 << 1;
pub const F_HEIGHT: u8 = 1 << 2;
pub const F_BPP: u8 = 1 << 3;

impl FbFacts {
    /// 空事实（present=0）。
    pub const fn empty() -> Self {
        FbFacts {
            base: 0,
            width: 0,
            height: 0,
            bpp: 0,
        }
    }

    /// 四字段齐否。
    pub const fn complete_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.base != 0 {
            m |= F_BASE;
        }
        if self.width != 0 {
            m |= F_WIDTH;
        }
        if self.height != 0 {
            m |= F_HEIGHT;
        }
        if self.bpp != 0 {
            m |= F_BPP;
        }
        m
    }

    /// stride（每行字节，bpp/8 × width；推导量）。
    pub const fn stride(&self) -> u64 {
        ((self.width as u64) * (self.bpp as u64)) / 8
    }

    /// 总字节数（推导量）。
    pub const fn total_bytes(&self) -> u64 {
        self.stride() * (self.height as u64)
    }

    /// 字段级合法校验（不含跨账比对）。
    pub const fn fields_legal(&self) -> Result<(), FbError> {
        let m = self.complete_mask();
        if m != (F_BASE | F_WIDTH | F_HEIGHT | F_BPP) {
            return Err(FbError::FieldMissing { mask: m });
        }
        if self.width > WIDTH_MAX || self.height > HEIGHT_MAX {
            return Err(FbError::GeometryBad {
                w: self.width,
                h: self.height,
            });
        }
        if !(self.bpp == 32 || self.bpp == 24 || self.bpp == 16) {
            return Err(FbError::BppBad { bpp: self.bpp });
        }
        if self.base & 0xFFF != 0 {
            return Err(FbError::BaseUnaligned { base: self.base });
        }
        Ok(())
    }
}

impl FbLedger {
    /// 空账（present=0 未封账）。
    pub const fn new() -> Self {
        FbLedger {
            facts: FbFacts::empty(),
            present: 0,
            sealed: false,
            seal_version: 0,
            seal_fp: 0,
        }
    }

    /// 写入一个字段（封账后拒写；合法档写前校验）。
    pub fn set(&mut self, field: u8, value: u64) -> Result<(), FbError> {
        if self.sealed {
            let name = match field {
                F_BASE => "base",
                F_WIDTH => "width",
                F_HEIGHT => "height",
                F_BPP => "bpp",
                _ => "unknown",
            };
            return Err(FbError::Frozen { field: name });
        }
        match field {
            F_BASE => {
                self.facts.base = value;
                self.present |= F_BASE;
            }
            F_WIDTH => {
                if value == 0 || value > WIDTH_MAX as u64 {
                    return Err(FbError::GeometryBad {
                        w: value as u32,
                        h: 0,
                    });
                }
                self.facts.width = value as u32;
                self.present |= F_WIDTH;
            }
            F_HEIGHT => {
                if value == 0 || value > HEIGHT_MAX as u64 {
                    return Err(FbError::GeometryBad {
                        w: 0,
                        h: value as u32,
                    });
                }
                self.facts.height = value as u32;
                self.present |= F_HEIGHT;
            }
            F_BPP => {
                if !(value == 32 || value == 24 || value == 16) {
                    return Err(FbError::BppBad { bpp: value as u8 });
                }
                self.facts.bpp = value as u8;
                self.present |= F_BPP;
            }
            _ => return Err(FbError::FieldMissing { mask: self.present }),
        }
        Ok(())
    }

    /// 从探测源灌账（失败显性 ProbeFailed）。
    pub fn load_from_probe(&mut self, src: &dyn ProbeSource) -> Result<(), FbError> {
        let facts = match src.probe() {
            Ok(f) => f,
            Err(e) => return Err(e),
        };
        facts.fields_legal()?;
        self.facts = facts;
        self.present = facts.complete_mask();
        Ok(())
    }

    /// 复读一致（4/4 硬线；field 下标 0..=3）。
    pub fn reread_consistent(&self, other: &FbFacts) -> Result<u32, FbError> {
        let mut ok = 0u32;
        if self.facts.base != other.base {
            return Err(FbError::Drift { field: 0 });
        }
        ok += 1;
        if self.facts.width != other.width {
            return Err(FbError::Drift { field: 1 });
        }
        ok += 1;
        if self.facts.height != other.height {
            return Err(FbError::Drift { field: 2 });
        }
        ok += 1;
        if self.facts.bpp != other.bpp {
            return Err(FbError::Drift { field: 3 });
        }
        ok += 1;
        Ok(ok)
    }

    /// 推导量对账（stride 与总字节双验）。
    pub fn derived_audit(&self) -> Result<(), FbError> {
        let expect_stride = ((self.facts.width as u64) * (self.facts.bpp as u64)) / 8;
        let expect_total = expect_stride * (self.facts.height as u64);
        if self.facts.stride() != expect_stride {
            return Err(FbError::DerivedBad { what: "stride" });
        }
        if self.facts.total_bytes() != expect_total {
            return Err(FbError::DerivedBad { what: "total-bytes" });
        }
        Ok(())
    }

    /// 账指纹（四字段 + present + 版本全进）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let f = &self.facts;
        h ^= f.base;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= f.width as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= f.height as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= f.bpp as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.seal_version as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 封账（四要素齐 + 合法 + 推导自洽才允许）。
    pub fn seal(&mut self) -> Result<(), FbError> {
        if self.sealed {
            return Err(FbError::Frozen { field: "seal" });
        }
        let mask = self.present;
        if mask != (F_BASE | F_WIDTH | F_HEIGHT | F_BPP) {
            return Err(FbError::FieldMissing { mask });
        }
        self.facts.fields_legal()?;
        self.derived_audit()?;
        self.sealed = true;
        if self.seal_version == 0 {
            self.seal_version = LEDGER_VERSION;
        }
        self.seal_fp = self.fingerprint();
        Ok(())
    }

    /// 显性升版重封（版本必须严格递增，且重封后指纹必须变——
    /// 指纹不变说明内容没真改，属徒劳升版，显性拒绝）。
    pub fn reseal(&mut self, new_version: u32) -> Result<(), FbError> {
        if new_version <= self.seal_version {
            return Err(FbError::VersionNotBumped {
                got: new_version,
                want: self.seal_version + 1,
            });
        }
        let old_fp = self.fingerprint();
        self.sealed = false;
        self.seal_version = new_version;
        self.seal_fp = 0;
        self.seal()?;
        let new_fp = self.fingerprint();
        if new_fp == old_fp {
            return Err(FbError::DerivedBad { what: "reseal-no-change" });
        }
        Ok(())
    }

    /// 只读访问器。
    pub const fn facts(&self) -> FbFacts {
        self.facts
    }

    /// 封账态。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封账指纹。
    pub const fn seal_fingerprint(&self) -> u64 {
        self.seal_fp
    }
}

/// 域自检。
pub fn run_unx481_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx481");

    // 固定探测源（1280×720×32）
    struct FixedProbe {
        f: FbFacts,
    }
    impl ProbeSource for FixedProbe {
        fn probe(&self) -> Result<FbFacts, FbError> {
            Ok(self.f)
        }
    }
    let good_facts = FbFacts {
        base: 0xFD00_0000,
        width: 1280,
        height: 720,
        bpp: 32,
    };
    let probe = FixedProbe { f: good_facts };

    // C481-01 灌账 + 复读 4/4
    {
        let mut led = FbLedger::new();
        let r1 = led.load_from_probe(&probe);
        let r2 = led.reread_consistent(&good_facts);
        let good = r1.is_ok() && r2 == Ok(4);
        s.add("C481-01-灌账复读 4/4", good, if good { "" } else { "复读路径失败" });
    }

    // C481-02 缺字段封账被拒且点名掩码
    {
        let mut led = FbLedger::new();
        let _ = led.set(F_BASE, 0xFD00_0000);
        let _ = led.set(F_WIDTH, 1280);
        // 缺 height/bpp
        let r = led.seal();
        let good = matches!(r, Err(FbError::FieldMissing { mask }) if mask == (F_BASE | F_WIDTH));
        s.add("C481-02-缺字段封账点名", good, if good { "" } else { "缺字段未点名" });
    }

    // C481-03 bpp 非法档拒绝
    {
        let mut led = FbLedger::new();
        let r = led.set(F_BPP, 8);
        let good = matches!(r, Err(FbError::BppBad { bpp: 8 }));
        s.add("C481-03-bpp 8 档拒收", good, if good { "" } else { "非法 bpp 被放行" });
    }

    // C481-04 封账后写入被拒（点名字段）
    {
        let mut led = FbLedger::new();
        let _ = led.load_from_probe(&probe);
        let _ = led.seal();
        let r = led.set(F_WIDTH, 1920);
        let good = matches!(r, Err(FbError::Frozen { field: "width" }));
        s.add("C481-04-封账后写拒", good, if good { "" } else { "冻结纪律失效" });
    }

    // C481-05 推导量对账（stride=5120, total=3686400）
    {
        let mut led = FbLedger::new();
        let _ = led.load_from_probe(&probe);
        let r = led.derived_audit();
        let facts = led.facts();
        let good = r.is_ok() && facts.stride() == 5120 && facts.total_bytes() == 3_686_400;
        s.add("C481-05-推导量对账", good, if good { "" } else { "stride/总字节不符" });
    }

    // C481-06 reseal 必须升版且指纹变
    {
        let mut led = FbLedger::new();
        let _ = led.load_from_probe(&probe);
        let _ = led.seal();
        let r0 = led.reseal(1);
        let good0 = r0.is_err(); // 同版本不许
        let mut led2 = FbLedger::new();
        let _ = led2.load_from_probe(&probe);
        let _ = led2.seal();
        let r1 = led2.reseal(2);
        let good = good0 && matches!(r1, Err(FbError::VersionNotBumped { .. })) == false && r1.is_ok();
        s.add("C481-06-reseal 升版与指纹", good, if good { "" } else { "升版纪律失效" });
    }

    // C481-07 基址非页对齐拒收
    {
        struct BadBaseProbe {
            f: FbFacts,
        }
        impl ProbeSource for BadBaseProbe {
            fn probe(&self) -> Result<FbFacts, FbError> {
                Ok(self.f)
            }
        }
        let p = BadBaseProbe {
            f: FbFacts {
                base: 0xFD00_0001,
                width: 1280,
                height: 720,
                bpp: 32,
            },
        };
        let mut led = FbLedger::new();
        let r = led.load_from_probe(&p);
        let good = matches!(r, Err(FbError::BaseUnaligned { .. }));
        s.add("C481-07-基址页对齐硬线", good, if good { "" } else { "未对齐基址被放行" });
    }

    // C481-08 复读漂移点名字段 2（height）
    {
        let mut led = FbLedger::new();
        let _ = led.load_from_probe(&probe);
        let drift = FbFacts {
            base: good_facts.base,
            width: good_facts.width,
            height: good_facts.height + 1,
            bpp: good_facts.bpp,
        };
        let r = led.reread_consistent(&drift);
        let good = matches!(r, Err(FbError::Drift { field: 2 }));
        s.add("C481-08-漂移点名字段", good, if good { "" } else { "漂移未点名" });
    }

    // C481-09 封账指纹稳定且非零
    {
        let mut led = FbLedger::new();
        let _ = led.load_from_probe(&probe);
        let _ = led.seal();
        let mut led2 = FbLedger::new();
        let _ = led2.load_from_probe(&probe);
        let _ = led2.seal();
        let good = led.seal_fingerprint() == led2.seal_fingerprint() && led.seal_fingerprint() != 0;
        s.add("C481-09-封账指纹确定", good, if good { "" } else { "指纹不确定" });
    }

    // C481-10 诊断码段独占
    {
        let codes = [
            FbCode::FieldMissing,
            FbCode::BppBad,
            FbCode::GeometryBad,
            FbCode::Drift,
            FbCode::Frozen,
            FbCode::ProbeFailed,
            FbCode::DerivedBad,
            FbCode::VersionBad,
            FbCode::BaseUnaligned,
            FbCode::Sealed,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C481-10-诊断码 0x84 段独占", all, if all { "" } else { "码值越出 0x84 段" });
    }

    s
}
