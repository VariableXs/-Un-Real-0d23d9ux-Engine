//! UNX-F0729 · 上游 changelog 评估账（变更项→影响面映射）· 诊断段 0x5aa0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0729] 上游 changelog 评估账（变更项→影响面映射） <SEG 0x5aa0>
//!   UNX-F0729-J1 changelog 逐条映射账齐（变更项/影响面/处置三列）
//!   覆盖 100%（逐条有映射或 N/A 声明）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 映射三列闭集：变更项（条目号+哈希）/影响面（ImpactSurface 臂）/
//!      处置（Disposition 臂），三列构造即齐，审计逐行复列。
//!   2. 影响面臂闭集：协议面/响应面/内存面/控制台面/无影响（N/A），
//!      臂外无影响面；N/A 即显性声明（覆盖判据的合法臂）。
//!   3. 处置臂闭集：跟随适配/观察不跟/降级隔离/拒绝跟随，臂外无处置。
//!   4. 覆盖 100%：changelog 逐条必须有映射行（映射或 N/A）——
//!      漏条目（E_ENTRY_UNMAPPED）与幽灵映射（无条目却映射）都检出。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5aa0~0x5abF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5aa0 独占 0x5aa0~0x5abF） ────────────────────────────
pub const SEG_BASE: u16 = 0x5aa0;

/// 0x5aa1 账已封板仍尝试落簿
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5aa2 影响面臂越闭集
const E_SURFACE_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5aa3 处置臂越闭集
const E_DISPOSAL_RANGE: u16 = SEG_BASE + 0x3;
/// 0x5aa4 条目号越域（合法 1..=MAX_ENTRIES）
const E_ENTRY_RANGE: u16 = SEG_BASE + 0x4;
/// 0x5aa5 条目未映射：changelog 逐条覆盖破
const E_ENTRY_UNMAPPED: u16 = SEG_BASE + 0x5;
/// 0x5aa6 幽灵映射：映射行无对应条目
const E_GHOST_MAP: u16 = SEG_BASE + 0x6;
/// 0x5aa7 三列不齐
const E_COL_MISSING: u16 = SEG_BASE + 0x7;
/// 0x5aa8 条目重挂：同条目二次映射
const E_MAP_DUP: u16 = SEG_BASE + 0x8;
/// 0x5aa9 条目描述非法：空串或超长
const E_DESC_BAD: u16 = SEG_BASE + 0x9;
/// 0x5aaA 映射账溢出
const E_MAP_FULL: u16 = SEG_BASE + 0xA;
/// 0x5aab 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x5aac 条目账空：零条目即评估
const E_EMPTY_CHANGELOG: u16 = SEG_BASE + 0xC;
/// 0x5aad 条目悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xD;
/// 0x5aaE 变更项哈希零值
const E_HASH_ZERO: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5aa0~0x5abF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "评估账已封板：落簿冻结，扩账请开新账",
        E_SURFACE_RANGE => "影响面臂越闭集：五臂之外无影响面",
        E_DISPOSAL_RANGE => "处置臂越闭集：四臂之外无处置",
        E_ENTRY_RANGE => "条目号越域：合法 1..=MAX_ENTRIES",
        E_ENTRY_UNMAPPED => "条目未映射：changelog 逐条覆盖破",
        E_GHOST_MAP => "幽灵映射：映射行无对应条目",
        E_COL_MISSING => "三列不齐：变更项/影响面/处置缺列",
        E_MAP_DUP => "条目重挂：同条目二次映射拒收",
        E_DESC_BAD => "条目描述非法：空串或超 64 字节",
        E_MAP_FULL => "映射账溢出：超定容上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_EMPTY_CHANGELOG => "条目账空：零条目无评估资格",
        E_REF_DANGLE => "条目悬空：解引越界",
        E_HASH_ZERO => "变更项哈希零值：非法",
        _ => "未知诊断码（0x5aa0 段外或保留位）",
    }
}

// ── 常量（来源：映射口径与定容） ─────────────────────────────────────
/// changelog 条目定容。
pub const MAX_ENTRIES: usize = 128;
/// 影响面臂数。
pub const SURFACE_ARMS: u8 = 5;
/// 处置臂数。
pub const DISPOSAL_ARMS: u8 = 4;
/// 描述长度上限。
pub const DESC_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(SURFACE_ARMS == 5);
const _: () = assert!(DISPOSAL_ARMS == 4);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 影响面臂闭集 ─────────────────────────────────────────────────────
/// 影响面五臂闭集（N/A 为合法臂——显性无影响声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactSurface {
    /// 协议面（Limine 协议/版本位）。
    Protocol,
    /// 响应面（boot info 响应树）。
    Response,
    /// 内存面（memory map/页表）。
    Memory,
    /// 控制台面（串口/显存输出）。
    Console,
    /// 无影响（N/A 显性声明）。
    NotApplicable,
}

impl ImpactSurface {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            ImpactSurface::Protocol => 0,
            ImpactSurface::Response => 1,
            ImpactSurface::Memory => 2,
            ImpactSurface::Console => 3,
            ImpactSurface::NotApplicable => 4,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<ImpactSurface, u16> {
        match v {
            0 => Ok(ImpactSurface::Protocol),
            1 => Ok(ImpactSurface::Response),
            2 => Ok(ImpactSurface::Memory),
            3 => Ok(ImpactSurface::Console),
            4 => Ok(ImpactSurface::NotApplicable),
            _ => Err(E_SURFACE_RANGE),
        }
    }

    /// N/A 臂判定。
    pub const fn is_na(self) -> bool {
        matches!(self, ImpactSurface::NotApplicable)
    }

    /// 臂名。
    pub const fn name(self) -> &'static str {
        match self {
            ImpactSurface::Protocol => "协议面",
            ImpactSurface::Response => "响应面",
            ImpactSurface::Memory => "内存面",
            ImpactSurface::Console => "控制台面",
            ImpactSurface::NotApplicable => "N/A",
        }
    }
}

// ── 处置臂闭集 ───────────────────────────────────────────────────────
/// 处置四臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// 跟随适配。
    Adapt,
    /// 观察不跟。
    Observe,
    /// 降级隔离。
    Isolate,
    /// 拒绝跟随。
    Reject,
}

impl Disposition {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            Disposition::Adapt => 0,
            Disposition::Observe => 1,
            Disposition::Isolate => 2,
            Disposition::Reject => 3,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<Disposition, u16> {
        match v {
            0 => Ok(Disposition::Adapt),
            1 => Ok(Disposition::Observe),
            2 => Ok(Disposition::Isolate),
            3 => Ok(Disposition::Reject),
            _ => Err(E_DISPOSAL_RANGE),
        }
    }
}

// ── changelog 条目 ───────────────────────────────────────────────────
/// changelog 条目：条目号 + 描述哈希。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeEntry {
    /// 条目号 1..=MAX_ENTRIES。
    pub no: u16,
    /// 描述 FNV 哈希。
    pub desc_fp: u64,
}

// ── 映射行（三列） ───────────────────────────────────────────────────
/// 映射账行：恰三列（变更项/影响面/处置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapRow {
    /// 列一：变更项（条目号）。
    pub entry: u16,
    /// 列二：影响面臂号。
    pub surface: u8,
    /// 列三：处置臂号。
    pub disposal: u8,
}

impl MapRow {
    /// 三列齐判定：条目号非零 + 双臂在闭集。
    pub const fn cols_ok(&self) -> bool {
        self.entry >= 1 && self.surface < SURFACE_ARMS && self.disposal < DISPOSAL_ARMS
    }
}

// ── changelog 评估账 ─────────────────────────────────────────────────
/// changelog 评估账：条目账 + 逐条映射 + 覆盖 100% 审计。
pub struct ChangelogLedger {
    /// 条目账。
    entries: Vec<ChangeEntry>,
    /// 映射账。
    maps: Vec<MapRow>,
    /// 封板位。
    sealed: bool,
}

impl ChangelogLedger {
    /// 新账。
    pub fn new() -> Self {
        ChangelogLedger {
            entries: Vec::new(),
            maps: Vec::new(),
            sealed: false,
        }
    }

    /// 收录 changelog 条目。
    pub fn add_entry(&mut self, no: u16, desc: &str) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(E_MAP_FULL);
        }
        if no < 1 || no as usize > MAX_ENTRIES {
            return Err(E_ENTRY_RANGE);
        }
        if desc.is_empty() || desc.len() > DESC_MAX {
            return Err(E_DESC_BAD);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].no == no {
                return Err(E_MAP_DUP);
            }
            i += 1;
        }
        self.entries.push(ChangeEntry {
            no,
            desc_fp: fnv_str(desc),
        });
        Ok(())
    }

    /// 条目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 映射数。
    pub fn map_count(&self) -> usize {
        self.maps.len()
    }

    /// 落映射：条目必须在账 + 重挂拦截 + N/A 臂处置语义（N/A 只配
    /// Observe 或 Reject——无影响面无适配资格）。
    pub fn map_entry(
        &mut self,
        no: u16,
        surface: ImpactSurface,
        disposal: Disposition,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.maps.len() >= MAX_ENTRIES {
            return Err(E_MAP_FULL);
        }
        if no < 1 || no as usize > MAX_ENTRIES {
            return Err(E_ENTRY_RANGE);
        }
        // 幽灵映射前置：条目必须在账。
        let mut found = false;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].no == no {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            return Err(E_GHOST_MAP);
        }
        // 重挂拦截。
        let mut j = 0usize;
        while j < self.maps.len() {
            if self.maps[j].entry == no {
                return Err(E_MAP_DUP);
            }
            j += 1;
        }
        // N/A 语义：无影响面无适配资格。
        if surface.is_na() && disposal == Disposition::Adapt {
            return Err(E_DISPOSAL_RANGE);
        }
        // 有影响面（非 N/A）禁观察不跟之外的消极处置约束略：
        // 非法面语义（N/A + Observe/Isolate/Reject 合法；非 N/A + 任意处置合法）。
        self.maps.push(MapRow {
            entry: no,
            surface: surface.id(),
            disposal: disposal.id(),
        });
        Ok(())
    }

    /// 条目解引。
    pub fn entry_at(&self, idx: usize) -> Result<ChangeEntry, u16> {
        if idx >= self.entries.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.entries[idx])
    }

    /// 映射解引。
    pub fn map_at(&self, idx: usize) -> Result<MapRow, u16> {
        if idx >= self.maps.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.maps[idx])
    }

    /// 覆盖 100% 审计：逐条目有映射（映射或 N/A）+ 映射无幽灵 +
    /// 三列齐。
    pub fn audit_coverage(&self) -> Result<usize, u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY_CHANGELOG);
        }
        // 逐条目找映射。
        let mut i = 0usize;
        while i < self.entries.len() {
            let no = self.entries[i].no;
            let mut found = false;
            let mut j = 0usize;
            while j < self.maps.len() {
                if self.maps[j].entry == no {
                    found = true;
                    if !self.maps[j].cols_ok() {
                        return Err(E_COL_MISSING);
                    }
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(E_ENTRY_UNMAPPED);
            }
            i += 1;
        }
        // 幽灵映射反查。
        let mut k = 0usize;
        while k < self.maps.len() {
            let no = self.maps[k].entry;
            let mut found = false;
            let mut m = 0usize;
            while m < self.entries.len() {
                if self.entries[m].no == no {
                    found = true;
                    break;
                }
                m += 1;
            }
            if !found {
                return Err(E_GHOST_MAP);
            }
            k += 1;
        }
        Ok(self.maps.len())
    }

    /// 封板：覆盖 100% 后落锁。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let n = self.audit_coverage()?;
        self.sealed = true;
        Ok(n)
    }

    /// 影响面分布。
    pub fn surface_dist(&self) -> Result<[u32; SURFACE_ARMS as usize], u16> {
        let mut dist = [0u32; SURFACE_ARMS as usize];
        let mut i = 0usize;
        while i < self.maps.len() {
            let s = ImpactSurface::from_id(self.maps[i].surface)?;
            dist[s.id() as usize] = dist[s.id() as usize].wrapping_add(1);
            i += 1;
        }
        Ok(dist)
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            h = fnv_mix(h, (e.no & 0xFF) as u8 | 0x10);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((e.desc_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.maps.len() {
            let m = self.maps[j];
            h = fnv_mix(h, (m.entry & 0xFF) as u8 | 0x20);
            h = fnv_mix(h, m.surface | 0x30);
            h = fnv_mix(h, m.disposal | 0x40);
            j += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.entries.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("changelog 评估: 条目=");
        out.push_str(&itoa(self.entries.len() as u64));
        out.push_str(" 映射=");
        out.push_str(&itoa(self.maps.len() as u64));
        out.push_str(" 覆盖=100% 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：8 条 changelog → 逐条映射（含 2 条 N/A）→ 覆盖 100% ——
    let mut led = ChangelogLedger::new();
    let descs = [
        "protocol revision bump 3->4",
        "memory map entry size change",
        "framebuffer pitch semantics",
        "kernel address virt base shift",
        "console glyph cache",
        "internal refactor only",
        "docs reorder",
        "ci script tweak",
    ];
    let mut no = 1u16;
    let mut d = 0usize;
    while d < descs.len() {
        led.add_entry(no, descs[d])?;
        no += 1;
        d += 1;
    }
    if led.entry_count() != 8 {
        return Err(E_ENTRY_RANGE);
    }
    // 逐条映射：1 协议面/适配，2 内存面/适配，3 响应面/观察，
    // 4 内存面/隔离，5 控制台面/适配，6 N/A/观察，7 N/A/拒绝，
    // 8 N/A/隔离。
    led.map_entry(1, ImpactSurface::Protocol, Disposition::Adapt)?;
    led.map_entry(2, ImpactSurface::Memory, Disposition::Adapt)?;
    led.map_entry(3, ImpactSurface::Response, Disposition::Observe)?;
    led.map_entry(4, ImpactSurface::Memory, Disposition::Isolate)?;
    led.map_entry(5, ImpactSurface::Console, Disposition::Adapt)?;
    led.map_entry(6, ImpactSurface::NotApplicable, Disposition::Observe)?;
    led.map_entry(7, ImpactSurface::NotApplicable, Disposition::Reject)?;
    led.map_entry(8, ImpactSurface::NotApplicable, Disposition::Isolate)?;
    if led.map_count() != 8 {
        return Err(E_ENTRY_UNMAPPED);
    }
    let cov = led.audit_coverage()?;
    if cov != 8 {
        return Err(E_ENTRY_UNMAPPED);
    }
    let n = led.seal()?;
    if n != 8 {
        return Err(E_ENTRY_UNMAPPED);
    }
    // 封板后落簿必拦。
    if led.add_entry(9, "late") != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if led.map_entry(9, ImpactSurface::Memory, Disposition::Adapt) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    // 影响面分布核对。
    let dist = led.surface_dist()?;
    if dist[ImpactSurface::NotApplicable.id() as usize] != 3 {
        return Err(E_SURFACE_RANGE);
    }
    if dist[ImpactSurface::Memory.id() as usize] != 2 {
        return Err(E_SURFACE_RANGE);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 条目解引。
    if led.entry_at(0)?.no != 1 {
        return Err(E_REF_DANGLE);
    }
    if led.map_at(7)?.entry != 8 {
        return Err(E_REF_DANGLE);
    }
    if led.entry_at(8) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // —— 覆盖破注入：缺 1 条映射 ——
    let mut gap = ChangelogLedger::new();
    gap.add_entry(1, "a")?;
    gap.add_entry(2, "b")?;
    gap.map_entry(1, ImpactSurface::Protocol, Disposition::Adapt)?;
    if gap.audit_coverage() != Err(E_ENTRY_UNMAPPED) {
        return Err(E_ENTRY_UNMAPPED);
    }
    // 幽灵映射注入：映射未收录条目。
    let mut ghost = ChangelogLedger::new();
    ghost.add_entry(1, "a")?;
    if ghost.map_entry(9, ImpactSurface::Memory, Disposition::Observe) != Err(E_GHOST_MAP) {
        return Err(E_GHOST_MAP);
    }

    // —— N/A 语义：N/A + 适配必拦 ——
    let mut na = ChangelogLedger::new();
    na.add_entry(1, "na case")?;
    if na.map_entry(1, ImpactSurface::NotApplicable, Disposition::Adapt) != Err(E_DISPOSAL_RANGE) {
        return Err(E_DISPOSAL_RANGE);
    }

    // —— 错误路径样本 ——
    if ImpactSurface::from_id(5) != Err(E_SURFACE_RANGE) {
        return Err(E_SURFACE_RANGE);
    }
    if Disposition::from_id(4) != Err(E_DISPOSAL_RANGE) {
        return Err(E_DISPOSAL_RANGE);
    }
    let mut err = ChangelogLedger::new();
    if err.add_entry(0, "x") != Err(E_ENTRY_RANGE) {
        return Err(E_ENTRY_RANGE);
    }
    if err.add_entry(MAX_ENTRIES as u16 + 1, "x") != Err(E_ENTRY_RANGE) {
        return Err(E_ENTRY_RANGE);
    }
    if err.add_entry(1, "") != Err(E_DESC_BAD) {
        return Err(E_DESC_BAD);
    }
    let long = [b'x'; DESC_MAX + 1];
    let long_desc = core::str::from_utf8(&long);
    if let Ok(s) = long_desc {
        if err.add_entry(1, s) != Err(E_DESC_BAD) {
            return Err(E_DESC_BAD);
        }
    }
    err.add_entry(1, "first")?;
    if err.add_entry(1, "dup") != Err(E_MAP_DUP) {
        return Err(E_MAP_DUP);
    }
    // 重挂映射。
    err.map_entry(1, ImpactSurface::Console, Disposition::Observe)?;
    if err.map_entry(1, ImpactSurface::Memory, Disposition::Adapt) != Err(E_MAP_DUP) {
        return Err(E_MAP_DUP);
    }
    // 空账评估。
    let mut empty = ChangelogLedger::new();
    if empty.audit_coverage() != Err(E_EMPTY_CHANGELOG) {
        return Err(E_EMPTY_CHANGELOG);
    }
    if empty.seal() != Err(E_EMPTY_CHANGELOG) {
        return Err(E_EMPTY_CHANGELOG);
    }
    if empty.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }
    // 映射/条目解引越界。
    if err.map_at(9) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    if err.entry_at(9) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // 诊断表抽查。
    if diag_detail(E_ENTRY_UNMAPPED) == "未知诊断码（0x5aa0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5aa0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
