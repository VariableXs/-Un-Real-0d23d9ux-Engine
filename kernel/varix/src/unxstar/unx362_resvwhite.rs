//! unx362_resvwhite ↔ UNX-F0362 保留区登记与白名单（引导自用段冻结）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0362，UNX-A1-B19）：
//! 「保留区登记与白名单（引导自用段冻结）| 220 | 已深化 |
//!  J1 保留区白名单逐段冻结，白名单外分配尝试拦截 100%」
//!
//! 设计要点（全部围绕引导自用内存段保护的真实状态）：
//! 1. **白名单逐段登记**：每段（基址/长度/用途标签）独立登记，
//!    重叠段登记即拒（白名单自身不得自相矛盾）；
//! 2. **冻结制**：登记完成后显性冻结（fingerprint 钉死全表）；
//!    冻结后加段/删段/改段一律 `SealedImmutable`——修订必须出新一版
//!    白名单（版本单调 + 旧版留档）；
//! 3. **拦截 100%**：任何分配尝试逐段比对（含边界碰触：申请段与白名单
//!    段哪怕重叠一字节都算白名单内冲突）；白名单外的"分配成功"在
//!    类型上不可能——allocator 门只放行落在自由区的请求；
//! 4. **拦截账**：被拦尝试 append-only 留痕（谁/何时/申请哪段），
//!    账满显性拒（不静默覆盖——拦截历史是安全审计面）；
//! 5. 诊断码段 0x98 独占；no_std 零 unwrap 零 expect，确定性可复算。
//!
//! 代码落点：kernel/varix/src/unxstar/（CoRun 册 A1-B19）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX362_RESVWHITE_VERSION: &str = "unx362-resvwhite-v1";

/// 白名单段容量。
pub const REGION_CAP: usize = 32;

/// 拦截账容量（append-only）。
pub const BLOCK_CAP: usize = 64;

/// 白名单版本容量（旧版留档）。
pub const VERSION_CAP: usize = 4;

/// 保留段（基址/长度/用途）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResvRegion {
    /// 基址（含）
    pub base: u64,
    /// 长度（字节；0 长段拒）
    pub len: u64,
    /// 用途标签（如 "kernel-text" / "report-ring" / "audit-region"）
    pub purpose: &'static str,
}

impl ResvRegion {
    /// 区间末尾（不含）。
    pub const fn end(&self) -> u64 {
        self.base.wrapping_add(self.len)
    }

    /// 与另一段是否重叠（含边界碰触）。
    pub fn overlaps(&self, o: &ResvRegion) -> bool {
        self.base < o.end() && o.base < self.end()
    }

    /// 段合法性：len>0 且 base+len 不回绕。
    pub const fn is_valid(&self) -> bool {
        self.len > 0 && self.base.checked_add(self.len).is_some()
    }
}

/// 诊断码闭集（0xA2 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResvWhiteCode {
    /// 0xA200 段非法（零长/回绕）
    RegionBad,
    /// 0xA201 白名单段自重叠
    RegionOverlap,
    /// 0xA202 冻结后改表被拒
    SealedImmutable,
    /// 0xA203 拦截账满
    LedgerFull,
    /// 0xA204 白名单段容量满
    RegionCapFull,
    /// 0xA205 版本档满
    VersionCapFull,
    /// 0xA206 白名单外分配被拦（成功码语义：拦截生效）
    Blocked,
    /// 0xA207 冻结通过
    Sealed,
}

impl ResvWhiteCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            ResvWhiteCode::RegionBad => 0xA200,
            ResvWhiteCode::RegionOverlap => 0xA201,
            ResvWhiteCode::SealedImmutable => 0xA202,
            ResvWhiteCode::LedgerFull => 0xA203,
            ResvWhiteCode::RegionCapFull => 0xA204,
            ResvWhiteCode::VersionCapFull => 0xA205,
            ResvWhiteCode::Blocked => 0xA206,
            ResvWhiteCode::Sealed => 0xA207,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            ResvWhiteCode::RegionBad => "RESVWHITE-REGION-BAD",
            ResvWhiteCode::RegionOverlap => "RESVWHITE-REGION-OVERLAP",
            ResvWhiteCode::SealedImmutable => "RESVWHITE-SEALED-IMMUTABLE",
            ResvWhiteCode::LedgerFull => "RESVWHITE-LEDGER-FULL",
            ResvWhiteCode::RegionCapFull => "RESVWHITE-REGION-CAP-FULL",
            ResvWhiteCode::VersionCapFull => "RESVWHITE-VERSION-CAP-FULL",
            ResvWhiteCode::Blocked => "RESVWHITE-BLOCKED",
            ResvWhiteCode::Sealed => "RESVWHITE-SEALED",
        }
    }

    /// 段独占双写互斥（0xA2 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0xA200 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            ResvWhiteCode::RegionBad => "段非法：零长段或 base+len 回绕段不可登记",
            ResvWhiteCode::RegionOverlap => "白名单段自重叠：白名单自身矛盾，登记拒绝",
            ResvWhiteCode::SealedImmutable => "冻结后改表被拒：修订必须出新版白名单（版本单调+旧版留档）",
            ResvWhiteCode::LedgerFull => "拦截账满：无法记录本次拦截，按显性失败处理（拦截历史不可丢）",
            ResvWhiteCode::RegionCapFull => "白名单段容量满：按显性失败处理",
            ResvWhiteCode::VersionCapFull => "版本档满：旧版留档滚动最旧（显性策略）",
            ResvWhiteCode::Blocked => "白名单外分配被拦：拦截 100% 生效",
            ResvWhiteCode::Sealed => "白名单冻结通过：全表指纹钉死",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx362_resvwhite.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 白名单错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResvWhiteError {
    RegionBad,
    RegionOverlap { with: usize },
    SealedImmutable,
    LedgerFull { cap: usize },
    RegionCapFull,
    VersionCapFull,
    Blocked { base: u64, len: u64 },
}

impl ResvWhiteError {
    /// 映射诊断码。
    pub const fn code(self) -> ResvWhiteCode {
        match self {
            ResvWhiteError::RegionBad => ResvWhiteCode::RegionBad,
            ResvWhiteError::RegionOverlap { .. } => ResvWhiteCode::RegionOverlap,
            ResvWhiteError::SealedImmutable => ResvWhiteCode::SealedImmutable,
            ResvWhiteError::LedgerFull { .. } => ResvWhiteCode::LedgerFull,
            ResvWhiteError::RegionCapFull => ResvWhiteCode::RegionCapFull,
            ResvWhiteError::VersionCapFull => ResvWhiteCode::VersionCapFull,
            ResvWhiteError::Blocked { .. } => ResvWhiteCode::Blocked,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 拦截账单条。
#[derive(Clone, Copy, Debug)]
pub struct BlockEntry {
    /// 申请段基址
    pub base: u64,
    /// 申请段长度
    pub len: u64,
    /// 申请 tick
    pub tick: u64,
}

/// 白名单（一段版）。
#[derive(Clone, Debug)]
pub struct ResvWhitelist {
    pub version: u32,
    regions: Vec<ResvRegion>,
    sealed: bool,
    sealed_tick: u64,
    blocks: Vec<BlockEntry>,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl ResvWhitelist {
    /// 新空白名单。
    pub fn new(version: u32) -> Self {
        ResvWhitelist {
            version,
            regions: Vec::new(),
            sealed: false,
            sealed_tick: 0,
            blocks: Vec::new(),
        }
    }

    /// 登记段（非法/重叠/冻结后/容量满 各显性拒）。
    pub fn add_region(&mut self, r: ResvRegion) -> Result<(), ResvWhiteError> {
        if self.sealed {
            return Err(ResvWhiteError::SealedImmutable);
        }
        if !r.is_valid() {
            return Err(ResvWhiteError::RegionBad);
        }
        if self.regions.len() >= REGION_CAP {
            return Err(ResvWhiteError::RegionCapFull);
        }
        for (i, e) in self.regions.iter().enumerate() {
            if e.overlaps(&r) {
                return Err(ResvWhiteError::RegionOverlap { with: i });
            }
        }
        self.regions.push(r);
        Ok(())
    }

    /// 冻结（全表指纹钉死；冻结后不可变）。
    pub fn seal(&mut self, tick: u64) -> Result<u64, ResvWhiteError> {
        if self.sealed {
            return Err(ResvWhiteError::SealedImmutable);
        }
        self.sealed = true;
        self.sealed_tick = tick;
        Ok(self.fingerprint())
    }

    /// 冻结否。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 分配尝试门（J1 核心：白名单外拦截 100%）。
    ///
    /// 语义：申请段与任一白名单段重叠 → 放行被拒（白名单是"引导自用"段，
    /// 分配器不得占用）；完全落在白名单外的自由区 → 放行。
    /// 注意：拦截尝试本身也记账（无论成败——审计面要看到所有尝试）。
    pub fn try_allocate(&mut self, base: u64, len: u64, tick: u64) -> Result<(), ResvWhiteError> {
        let req = ResvRegion { base, len, purpose: "alloc-req" };
        let mut hit: Option<usize> = None;
        for (i, e) in self.regions.iter().enumerate() {
            if len == 0 || !req.is_valid() {
                break;
            }
            if e.overlaps(&req) {
                hit = Some(i);
                break;
            }
        }
        // 拦截账（含非法请求：零长/回绕也拦）
        if self.blocks.len() >= BLOCK_CAP {
            return Err(ResvWhiteError::LedgerFull { cap: BLOCK_CAP });
        }
        self.blocks.push(BlockEntry { base, len, tick });
        if hit.is_some() || len == 0 || !req.is_valid() {
            Err(ResvWhiteError::Blocked { base, len })
        } else {
            Ok(())
        }
    }

    /// 拦截账条数。
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 白名单段数。
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// 段读（越界安全）。
    pub fn region_at(&self, idx: usize) -> Option<ResvRegion> {
        self.regions.get(idx).copied()
    }

    /// 全表指纹（段集 + 冻结态全进）。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(&self.version.to_le_bytes());
        for r in self.regions.iter() {
            b.extend_from_slice(&r.base.to_le_bytes());
            b.extend_from_slice(&r.len.to_le_bytes());
            b.extend_from_slice(r.purpose.as_bytes());
        }
        b.push(if self.sealed { 1 } else { 0 });
        b.extend_from_slice(&self.sealed_tick.to_le_bytes());
        fnv1a64(&b)
    }
}

/// 白名单版本管理（修订出新版 + 旧版留档）。
#[derive(Clone, Debug)]
pub struct ResvWhiteBook {
    /// 当前版
    pub current: ResvWhitelist,
    /// 旧版档（滚动显性）
    archive: Vec<ResvWhitelist>,
}

impl ResvWhiteBook {
    /// 新书（v1 空白名单）。
    pub fn new() -> Self {
        ResvWhiteBook {
            current: ResvWhitelist::new(1),
            archive: Vec::new(),
        }
    }

    /// 修订出新版（旧版必须已封；版本严格 +1；档满滚最旧显性）。
    pub fn revise(&mut self) -> Result<(), ResvWhiteError> {
        if !self.current.is_sealed() {
            return Err(ResvWhiteError::SealedImmutable);
        }
        let new_ver = self
            .current
            .version
            .checked_add(1)
            .ok_or(ResvWhiteError::VersionCapFull)?;
        // 旧版留档（浅拷贝段集，清空拦截账——拦截账不跨版本带）
        let archived = ResvWhitelist {
            version: self.current.version,
            regions: self.current.regions.clone(),
            sealed: true,
            sealed_tick: self.current.sealed_tick,
            blocks: Vec::new(),
        };
        if self.archive.len() >= VERSION_CAP {
            self.archive.remove(0); // 滚最旧（显性策略）
        }
        self.archive.push(archived);
        self.current = ResvWhitelist::new(new_ver);
        Ok(())
    }

    /// 档内版本清单（升序）。
    pub fn archived_versions(&self) -> Vec<u32> {
        self.archive.iter().map(|w| w.version).collect()
    }

    /// 档卷数。
    pub fn archive_len(&self) -> usize {
        self.archive.len()
    }
}

/// 域自检（`run_unx362_checks`）：J1 判据逐条可失败断言。
pub fn run_unx362_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx362");

    // C362-01 白名单逐段登记 + 冻结通过（J1 正面）
    {
        let mut w = ResvWhitelist::new(1);
        let r1 = w.add_region(ResvRegion { base: 0x1000, len: 0x4000, purpose: "kernel-text" });
        let r2 = w.add_region(ResvRegion { base: 0x8000, len: 0x1000, purpose: "report-ring" });
        let seal = w.seal(10);
        let ok = r1.is_ok() && r2.is_ok() && seal.is_ok() && w.is_sealed() && w.region_count() == 2;
        s.add("C362-01-逐段登记冻结通过", ok, if ok { "" } else { "登记冻结路径失效" });
    }

    // C362-02 白名单外分配拦截 100%（J1 核心）
    {
        let mut w = ResvWhitelist::new(1);
        let _ = w.add_region(ResvRegion { base: 0x1000, len: 0x4000, purpose: "kernel-text" });
        let _ = w.seal(1);
        // 命中白名单（完全落入）
        let hit_full = w.try_allocate(0x2000, 0x100, 2);
        // 边界碰触（重叠一字节：0xFFF 起 2 字节 → 0xFFF..0x1001 撞 base 0x1000）
        let hit_edge = w.try_allocate(0x0FFF, 2, 3);
        // 白名单外自由区放行
        let free = w.try_allocate(0x9000, 0x100, 4);
        let ok = matches!(hit_full, Err(ResvWhiteError::Blocked { .. }))
            && matches!(hit_edge, Err(ResvWhiteError::Blocked { .. }))
            && free.is_ok()
            && w.block_count() == 3;
        s.add("C362-02-白名单外拦截百分百", ok, if ok { "" } else { "拦截有漏" });
    }

    // C362-03 白名单段自重叠拒绝
    {
        let mut w = ResvWhitelist::new(1);
        let r1 = w.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let r2 = w.add_region(ResvRegion { base: 0x1800, len: 0x800, purpose: "b" });
        let ok = r1.is_ok() && matches!(r2, Err(ResvWhiteError::RegionOverlap { with: 0 }));
        s.add("C362-03-自重叠段拒绝", ok, if ok { "" } else { "矛盾白名单被接受" });
    }

    // C362-04 冻结后加段拒绝（改表必须出新版）
    {
        let mut w = ResvWhitelist::new(1);
        let _ = w.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let _ = w.seal(1);
        let r = w.add_region(ResvRegion { base: 0x9000, len: 0x1000, purpose: "b" });
        let seal2 = w.seal(2);
        let ok = matches!(r, Err(ResvWhiteError::SealedImmutable))
            && matches!(seal2, Err(ResvWhiteError::SealedImmutable));
        s.add("C362-04-冻结后改表拒绝", ok, if ok { "" } else { "冻结表被改动" });
    }

    // C362-05 零长段非法拒绝 + 零长分配请求拦截
    {
        let mut w = ResvWhitelist::new(1);
        let r = w.add_region(ResvRegion { base: 0x1000, len: 0, purpose: "zero" });
        let _ = w.seal(1);
        let alloc0 = w.try_allocate(0x9000, 0, 2);
        let ok = matches!(r, Err(ResvWhiteError::RegionBad))
            && matches!(alloc0, Err(ResvWhiteError::Blocked { base: 0x9000, len: 0 }));
        s.add("C362-05-零长段与零长申请拦", ok, if ok { "" } else { "零长语义漏洞" });
    }

    // C362-06 回绕段拒绝
    {
        let mut w = ResvWhitelist::new(1);
        let r = w.add_region(ResvRegion { base: u64::MAX - 2, len: 0x10, purpose: "wrap" });
        let ok = matches!(r, Err(ResvWhiteError::RegionBad));
        s.add("C362-06-回绕段拒绝", ok, if ok { "" } else { "回绕段被登记" });
    }

    // C362-07 拦截账满显性拒
    {
        let mut w = ResvWhitelist::new(1);
        let _ = w.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let _ = w.seal(1);
        let mut overflow = false;
        for i in 0..(BLOCK_CAP + 4) {
            if matches!(w.try_allocate(0x2000 + i as u64 * 8, 8, i as u64), Err(ResvWhiteError::LedgerFull { .. })) {
                overflow = true;
            }
        }
        let ok = overflow && w.block_count() == BLOCK_CAP;
        s.add("C362-07-拦截账满显性拒", ok, if ok { "" } else { "拦截账被静默覆盖" });
    }

    // C362-08 修订出新版：版本单调 + 旧版留档（冻结制修订通道）
    {
        let mut book = ResvWhiteBook::new();
        let _ = book.current.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let fp1 = book.current.seal(1).unwrap_or(0);
        let rev = book.revise();
        let _ = book.current.add_region(ResvRegion { base: 0x9000, len: 0x1000, purpose: "b" });
        let _ = book.current.seal(2);
        let arch = book.archived_versions();
        let ok = rev.is_ok()
            && book.current.version == 2
            && arch == vec![1]
            && book.archive_len() == 1
            && fp1 != 0
            && book.current.region_count() == 1;
        s.add("C362-08-修订出新版旧版留档", ok, if ok { "" } else { "修订通道失效" });
    }

    // C362-09 未封卷修订拒绝
    {
        let mut book = ResvWhiteBook::new();
        let _ = book.current.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let r = book.revise();
        let ok = matches!(r, Err(ResvWhiteError::SealedImmutable));
        s.add("C362-09-未封修订拒绝", ok, if ok { "" } else { "未封表被换版" });
    }

    // C362-10 版本档满滚动最旧
    {
        let mut book = ResvWhiteBook::new();
        let mut all_ok = true;
        for v in 1..=(VERSION_CAP as u32 + 2) {
            let _ = book.current.seal(v as u64);
            all_ok &= book.revise().is_ok();
        }
        let arch = book.archived_versions();
        // revise 6 次后：current.version = 7；档内应留 3..=6（滚掉 1..2）
        let ok = all_ok
            && book.current.version == VERSION_CAP as u32 + 3
            && arch == vec![3, 4, 5, 6];
        s.add("C362-10-版本档滚动最旧", ok, if ok { "" } else { "版本档滚动失效" });
    }

    // C362-11 指纹确定且段集敏感
    {
        let mut w1 = ResvWhitelist::new(1);
        let mut w2 = ResvWhitelist::new(1);
        let _ = w1.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let _ = w2.add_region(ResvRegion { base: 0x1000, len: 0x1000, purpose: "a" });
        let f1a = w1.fingerprint();
        let f1b = w1.fingerprint();
        let _ = w2.add_region(ResvRegion { base: 0x9000, len: 0x100, purpose: "b" });
        let f2 = w2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C362-11-指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C362-12 诊断码 0xA2 段独占
    {
        let codes = [
            ResvWhiteCode::RegionBad,
            ResvWhiteCode::RegionOverlap,
            ResvWhiteCode::SealedImmutable,
            ResvWhiteCode::LedgerFull,
            ResvWhiteCode::RegionCapFull,
            ResvWhiteCode::VersionCapFull,
            ResvWhiteCode::Blocked,
            ResvWhiteCode::Sealed,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C362-12-诊断码 0xA2 段独占", all, if all { "" } else { "码值越出 0xA2 段" });
    }

    // C362-13 白名单容量满显性拒
    {
        let mut w = ResvWhitelist::new(1);
        let mut overflow = false;
        // 间隔足够大的段避免自重叠
        for i in 0..(REGION_CAP + 2) {
            let base = (i as u64) * 0x1000_0000;
            if w.add_region(ResvRegion { base, len: 0x1000, purpose: "seg" }).is_err() {
                overflow = true;
            }
        }
        let ok = overflow && w.region_count() == REGION_CAP;
        s.add("C362-13-段容量满显性拒", ok, if ok { "" } else { "容量被静默截断" });
    }

    s
}
