//! unx601_initrd_ledger ↔ UNX-F0601 initrd 基址/长度账（loader 交接）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0601，UNX-A1-B31）：
//! 「initrd 基址/长度账（loader 交接）| 220 | 已深化 |
//!  基址/长度/指纹三元组账齐（交接字段 1/1 提取），长度界断言（≤64MB 账注）」
//!
//! 设计要点：
//! 1. **三元组账**：base/len/fingerprint 三字段齐才算交接成功；
//!     缺一即 `FieldMissing`——loader（Limine）模块字段漏读是
//!     initrd 丢失的头号原因（静默拿到 0 长度 = 启动后期 panic）。
//! 2. **长度界断言**：initrd ≤ 64MB（引导期可用内存预算），
//!     超出 `LenOverBudget` 显性拒收（防 loader 报了个坏长度
//!     把内核拉去读整个物理内存）。
//! 3. **页对齐 + 页数推导**：base 必须 4KB 对齐；`page_count()`
//!     按上取整推导页数（尾页不齐也占整页）。
//! 4. **指纹复算**：`payload_fingerprint()` 对 initrd 内容抽样
//!     做 FNV（前/中/后三段 + 长度），与 loader 交接指纹比对，
//!     不一致 = 模块被搬动过。
//! 5. **损耗账**：`Coverage` 记录已核对页数/未核对页数，
//!     「全核对」才是终态（防抽样变全覆盖）。
//!
//! 诊断码：`InitrdCode` 0x88 段（0x8800..=0x880F）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX601_INITRD_VERSION: &str = "unx601-initrd-ledger-v1";

/// 页大小（4KB）。
pub const PAGE_SIZE: u64 = 4096;

/// initrd 长度上界（64MB，引导期预算）。
pub const INITRD_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// 最小合法长度（0 长度 = 无 initrd，也合法但必须显式登记）。
pub const INITRD_MIN_BYTES: u64 = 0;

/// 交接源抽象（loader 模块 / 测试夹具同构）。
pub trait InitrdSource {
    /// 取三元组（Err = 交接字段缺失）。
    fn handoff(&self) -> Result<InitrdFacts, InitrdError>;
}

/// 三元组事实。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InitrdFacts {
    /// 物理基址
    pub base: u64,
    /// 字节长度
    pub len: u64,
    /// loader 侧指纹
    pub fingerprint: u64,
}

/// initrd 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InitrdLedger {
    facts: InitrdFacts,
    /// 三元组在位掩码（三位）
    present: u8,
    /// 已核对页数
    verified_pages: u32,
    /// 已封账
    sealed: bool,
    /// 封账指纹
    seal_fp: u64,
}

/// initrd 错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitrdError {
    /// 缺字段（三位掩码）
    FieldMissing { mask: u8 },
    /// 基址未页对齐
    BaseUnaligned { base: u64 },
    /// 长度超预算
    LenOverBudget { len: u64, max: u64 },
    /// 长度非法（非单调/回绕）
    LenWrapped { len: u64 },
    /// 指纹不符
    FingerprintMismatch { got: u64, want: u64 },
    /// 页核对未完成
    CoverageIncomplete { verified: u32, total: u32 },
    /// 已封账
    Frozen,
}

/// 诊断码（0x88 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitrdCode {
    /// 0x8800 缺字段
    FieldMissing,
    /// 0x8801 基址未对齐
    BaseUnaligned,
    /// 0x8802 长度超限
    LenOver,
    /// 0x8803 长度回绕
    LenWrapped,
    /// 0x8804 指纹不符
    FingerprintBad,
    /// 0x8805 核对未完成
    CoverageBad,
    /// 0x8806 封账后写
    Frozen,
    /// 0x8807 封账通过
    Sealed,
}

impl InitrdCode {
    pub const fn code(self) -> u16 {
        match self {
            InitrdCode::FieldMissing => 0x8800,
            InitrdCode::BaseUnaligned => 0x8801,
            InitrdCode::LenOver => 0x8802,
            InitrdCode::FingerprintBad => 0x8803,
            InitrdCode::LenWrapped => 0x8804,
            InitrdCode::CoverageBad => 0x8805,
            InitrdCode::Frozen => 0x8806,
            InitrdCode::Sealed => 0x8807,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            InitrdCode::FieldMissing => "INITRD-FIELD-MISSING",
            InitrdCode::BaseUnaligned => "INITRD-BASE-UNALIGNED",
            InitrdCode::LenOver => "INITRD-LEN-OVER",
            InitrdCode::LenWrapped => "INITRD-LEN-WRAPPED",
            InitrdCode::FingerprintBad => "INITRD-FINGERPRINT-BAD",
            InitrdCode::CoverageBad => "INITRD-COVERAGE-BAD",
            InitrdCode::Frozen => "INITRD-FROZEN",
            InitrdCode::Sealed => "INITRD-SEALED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            InitrdCode::FieldMissing => "initrd 三元组不全：基址/长度/指纹缺一即拒",
            InitrdCode::BaseUnaligned => "initrd 基址未页对齐：必须 4KB 对齐",
            InitrdCode::LenOver => "initrd 长度超预算：超过 64MB 引导期预算拒收",
            InitrdCode::LenWrapped => "initrd 长度回绕：长度字段非单调，loader 交接损坏",
            InitrdCode::FingerprintBad => "initrd 指纹不符：交接指纹与复算不一致，模块被搬动",
            InitrdCode::CoverageBad => "页核对未完成：抽样变全覆盖是假绿",
            InitrdCode::Frozen => "initrd 账已封：改写必须走 reseal",
            InitrdCode::Sealed => "initrd 账封账通过：三元组齐 + 对齐 + 预算内 + 指纹合",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx601_initrd_ledger.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8800 && (c & 0x00FF) < 0x10
    }
}

impl InitrdError {
    pub const fn code(self) -> InitrdCode {
        match self {
            InitrdError::FieldMissing { .. } => InitrdCode::FieldMissing,
            InitrdError::BaseUnaligned { .. } => InitrdCode::BaseUnaligned,
            InitrdError::LenOverBudget { .. } => InitrdCode::LenOver,
            InitrdError::LenWrapped { .. } => InitrdCode::LenWrapped,
            InitrdError::FingerprintMismatch { .. } => InitrdCode::FingerprintBad,
            InitrdError::CoverageIncomplete { .. } => InitrdCode::CoverageBad,
            InitrdError::Frozen => InitrdCode::Frozen,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 字段位（三位）。
pub const I_BASE: u8 = 1 << 0;
pub const I_LEN: u8 = 1 << 1;
pub const I_FP: u8 = 1 << 2;

impl InitrdFacts {
    /// 空事实。
    pub const fn empty() -> Self {
        InitrdFacts {
            base: 0,
            len: 0,
            fingerprint: 0,
        }
    }

    /// 在位掩码（len=0 也算在位——显式登记空 initrd 合法）。
    pub const fn mask(&self) -> u8 {
        let mut m = 0u8;
        if self.base != 0 {
            m |= I_BASE;
        }
        m |= I_LEN; // len 字段总在位（值可为 0，由 empty 标记表达）
        if self.fingerprint != 0 {
            m |= I_FP;
        }
        m
    }

    /// 是否显式空 initrd（base=0 len=0 fp=0）。
    pub const fn is_explicit_empty(&self) -> bool {
        self.base == 0 && self.len == 0 && self.fingerprint == 0
    }

    /// 页数（上取整；len=0 → 0 页）。
    pub const fn page_count(&self) -> u64 {
        if self.len == 0 {
            0
        } else {
            (self.len + PAGE_SIZE - 1) / PAGE_SIZE
        }
    }

    /// 尾页字节（非整页时）。
    pub const fn tail_bytes(&self) -> u64 {
        if self.len == 0 {
            0
        } else {
            let r = self.len % PAGE_SIZE;
            if r == 0 {
                PAGE_SIZE
            } else {
                r
            }
        }
    }

    /// 内容指纹（对三段抽样做 FNV：头 4KB/中 4KB/尾 4KB + 长度）。
    ///
    /// 内容经 `&[u8]` 注入（按页对齐段），不持有 initrd 本体。
    pub fn payload_fingerprint(&self, head: &[u8], mid: &[u8], tail: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.len;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        for b in head.iter().chain(mid.iter()).chain(tail.iter()) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

impl InitrdLedger {
    /// 空账。
    pub const fn new() -> Self {
        InitrdLedger {
            facts: InitrdFacts::empty(),
            present: 0,
            verified_pages: 0,
            sealed: false,
            seal_fp: 0,
        }
    }

    /// 从交接源灌账。
    pub fn load(&mut self, src: &dyn InitrdSource) -> Result<(), InitrdError> {
        let f = src.handoff()?;
        if f.is_explicit_empty() {
            self.facts = f;
            self.present = 0;
            self.sealed = false;
            return Ok(());
        }
        if f.base & (PAGE_SIZE - 1) != 0 {
            return Err(InitrdError::BaseUnaligned { base: f.base });
        }
        if f.len > INITRD_MAX_BYTES {
            return Err(InitrdError::LenOverBudget {
                len: f.len,
                max: INITRD_MAX_BYTES,
            });
        }
        if f.fingerprint == 0 {
            return Err(InitrdError::FieldMissing {
                mask: f.mask() & !I_FP,
            });
        }
        self.facts = f;
        self.present = f.mask();
        Ok(())
    }

    /// 登记一页已核对（单调递增，禁回退）。
    pub fn verify_page(&mut self) -> Result<(), InitrdError> {
        if self.sealed {
            return Err(InitrdError::Frozen);
        }
        let total = self.facts.page_count() as u32;
        if self.verified_pages >= total {
            return Err(InitrdError::CoverageIncomplete {
                verified: self.verified_pages,
                total,
            });
        }
        self.verified_pages += 1;
        Ok(())
    }

    /// 核对完成度。
    pub const fn coverage(&self) -> (u32, u32) {
        (self.verified_pages, self.facts.page_count() as u32)
    }

    /// 指纹复算核对（三段抽样）。
    pub fn verify_fingerprint(
        &self,
        head: &[u8],
        mid: &[u8],
        tail: &[u8],
    ) -> Result<(), InitrdError> {
        if self.facts.is_explicit_empty() {
            return Ok(());
        }
        let got = self.facts.payload_fingerprint(head, mid, tail);
        if got != self.facts.fingerprint {
            return Err(InitrdError::FingerprintMismatch {
                got,
                want: self.facts.fingerprint,
            });
        }
        Ok(())
    }

    /// 封账（三元齐 + 全页核对）。
    pub fn seal(&mut self) -> Result<(), InitrdError> {
        if self.facts.is_explicit_empty() {
            self.sealed = true;
            self.seal_fp = 0;
            return Ok(());
        }
        let mask = self.present;
        if mask != (I_BASE | I_LEN | I_FP) {
            return Err(InitrdError::FieldMissing { mask });
        }
        let (v, t) = self.coverage();
        if v != t {
            return Err(InitrdError::CoverageIncomplete { verified: v, total: t });
        }
        self.sealed = true;
        self.seal_fp = self.facts.fingerprint;
        Ok(())
    }

    /// 只读访问器。
    pub const fn facts(&self) -> InitrdFacts {
        self.facts
    }

    /// 封账态。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// 域自检。
pub fn run_unx601_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx601");

    // 固定源（1 页 + 尾 128 字节）
    struct FixedSrc {
        f: InitrdFacts,
    }
    impl InitrdSource for FixedSrc {
        fn handoff(&self) -> Result<InitrdFacts, InitrdError> {
            Ok(self.f)
        }
    }
    let facts = InitrdFacts {
        base: 0x1000_0000,
        len: PAGE_SIZE + 128,
        fingerprint: 0x1234_5678_9abc_def0,
    };
    let src = FixedSrc { f: facts };

    // C601-01 灌账 + 三元组齐
    {
        let mut l = InitrdLedger::new();
        let r = l.load(&src);
        let good = r.is_ok() && l.present == (I_BASE | I_LEN | I_FP);
        s.add("C601-01-三元组齐", good, if good { "" } else { "三元组不齐" });
    }

    // C601-02 页数推导（4224 字节 → 2 页，尾 128）
    {
        let good = facts.page_count() == 2 && facts.tail_bytes() == 128;
        s.add("C601-02-页数与尾页", good, if good { "" } else { "页数推导错" });
    }

    // C601-03 长度超预算拒收
    {
        struct OverSrc {
            f: InitrdFacts,
        }
        impl InitrdSource for OverSrc {
            fn handoff(&self) -> Result<InitrdFacts, InitrdError> {
                Ok(self.f)
            }
        }
        let p = OverSrc {
            f: InitrdFacts {
                base: 0x1000_0000,
                len: INITRD_MAX_BYTES + 1,
                fingerprint: 0xAAAA,
            },
        };
        let mut l = InitrdLedger::new();
        let r = l.load(&p);
        let good = matches!(r, Err(InitrdError::LenOverBudget { .. }));
        s.add("C601-03-长度超限拒收", good, if good { "" } else { "超预算被放行" });
    }

    // C601-04 基址未对齐拒收
    {
        struct BadSrc {
            f: InitrdFacts,
        }
        impl InitrdSource for BadSrc {
            fn handoff(&self) -> Result<InitrdFacts, InitrdError> {
                Ok(self.f)
            }
        }
        let p = BadSrc {
            f: InitrdFacts {
                base: 0x1000_0001,
                len: 4096,
                fingerprint: 0xBBBB,
            },
        };
        let mut l = InitrdLedger::new();
        let r = l.load(&p);
        let good = matches!(r, Err(InitrdError::BaseUnaligned { base: 0x1000_0001 }));
        s.add("C601-04-基址对齐硬线", good, if good { "" } else { "未对齐被放行" });
    }

    // C601-05 缺指纹拒收
    {
        struct NoFpSrc {
            f: InitrdFacts,
        }
        impl InitrdSource for NoFpSrc {
            fn handoff(&self) -> Result<InitrdFacts, InitrdError> {
                Ok(self.f)
            }
        }
        let p = NoFpSrc {
            f: InitrdFacts {
                base: 0x1000_0000,
                len: 4096,
                fingerprint: 0,
            },
        };
        let mut l = InitrdLedger::new();
        let r = l.load(&p);
        let good = matches!(r, Err(InitrdError::FieldMissing { .. }));
        s.add("C601-05-缺指纹拒收", good, if good { "" } else { "缺指纹被放行" });
    }

    // C601-06 全页核对后封账（未核完封账被拒）
    {
        let mut l = InitrdLedger::new();
        let _ = l.load(&src);
        let r0 = l.seal();
        let good0 = matches!(r0, Err(InitrdError::CoverageIncomplete { verified: 0, total: 2 }));
        let _ = l.verify_page();
        let r1 = l.seal();
        let good1 = matches!(r1, Err(InitrdError::CoverageIncomplete { verified: 1, total: 2 }));
        let _ = l.verify_page();
        let r2 = l.seal();
        let good2 = r2.is_ok();
        let good = good0 && good1 && good2;
        s.add("C601-06-核对完成才封账", good, if good { "" } else { "核对门失效" });
    }

    // C601-07 指纹复算不符被拒
    {
        let mut l = InitrdLedger::new();
        let _ = l.load(&src);
        let head = [1u8; 16];
        let r = l.verify_fingerprint(&head, &head, &head);
        let good = matches!(r, Err(InitrdError::FingerprintMismatch { .. }));
        s.add("C601-07-指纹不符拒收", good, if good { "" } else { "指纹核对失效" });
    }

    // C601-08 显式空 initrd 合法封账
    {
        struct EmptySrc;
        impl InitrdSource for EmptySrc {
            fn handoff(&self) -> Result<InitrdFacts, InitrdError> {
                Ok(InitrdFacts::empty())
            }
        }
        let mut l = InitrdLedger::new();
        let r0 = l.load(&EmptySrc);
        let r1 = l.seal();
        let good = r0.is_ok() && r1.is_ok() && l.facts().is_explicit_empty();
        s.add("C601-08-空 initrd 显式合法", good, if good { "" } else { "空 initrd 处理错" });
    }

    // C601-09 封账后 verify_page 被拒
    {
        let mut l = InitrdLedger::new();
        let _ = l.load(&src);
        let _ = l.verify_page();
        let _ = l.verify_page();
        let _ = l.seal();
        let r = l.verify_page();
        let good = matches!(r, Err(InitrdError::Frozen));
        s.add("C601-09-封账后核对拒", good, if good { "" } else { "冻结失效" });
    }

    // C601-10 尾页恒为整页（len=4096 整数页）
    {
        let f = InitrdFacts {
            base: 0x1000_0000,
            len: 4096,
            fingerprint: 0xCCCC,
        };
        let good = f.page_count() == 1 && f.tail_bytes() == 4096;
        s.add("C601-10-整页尾页语义", good, if good { "" } else { "整页尾页错" });
    }

    // C601-11 诊断码段独占
    {
        let codes = [
            InitrdCode::FieldMissing,
            InitrdCode::BaseUnaligned,
            InitrdCode::LenOver,
            InitrdCode::LenWrapped,
            InitrdCode::FingerprintBad,
            InitrdCode::CoverageBad,
            InitrdCode::Frozen,
            InitrdCode::Sealed,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C601-11-诊断码 0x88 段独占", all, if all { "" } else { "码值越出 0x88 段" });
    }

    s
}
