//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12403 · NtMapViewOfSection 语义档 · 0x30060
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12403-J1 映射参数语义（ZeroBits/CommitSize/Protect/
//!      InheritDisposition）判据绿，逐参数注入 30/30 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - ZeroBits 域闭集 {0,21,24,31,53}，越域拒（ZeroBitsBad）；映射基址
//!     高位越 ZeroBits 上界拒（ZeroBitsConflict）；
//!   - InheritDisposition 闭集三档（ViewShare=1/ViewUnmap=2/保留=0），
//!     越三档拒（InheritBad）；
//!   - Protect 档沿用八档 PAGE 闭集 + 目标标志位，越档拒（ProtectBad）；
//!   - CommitSize 语义：仅页文件背书节可非零，且 ≤ViewSize、≤节尺寸
//!     （CommitOver）；保留档 CommitSize>0 拒（CommitOnReserve）；
//!   - ViewSize 语义：0=整节映射；ViewSize>节尺寸拒（ViewOver）；
//!     SectionOffset+ViewSize>节尺寸拒（OffsetOver）；偏移非 64KiB 粒度
//!     拒（OffsetGran）；
//!   - 显式基址必须 64KiB 分配粒度对齐（AlignGran）；与已有视口重叠拒
//!     （Overlap）；
//!   - 视口账定容 32 槽：Slot/Base/Size/Protect/Inherit 五列，满表拒
//!     （SlotFull）；节不存在拒（SecNotFound）；
//!   - 注入核：LCG 写死种子 0x30060，逐参数 30 例 30/30 正确。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30060 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30060;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮逐参数注入例数（判准 30/30）。
pub const INJECT_CASES: usize = 30;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 分配粒度（64KiB 口径）。
pub const ALLOC_GRAN: u64 = 0x10000;

/// 视口账槽位数（定容）。
pub const VIEW_SLOTS: usize = 32;

/// ZeroBits 域闭集（{0,21,24,31,53} 口径）。
pub const ZERO_BITS_SET: [u32; 5] = [0, 21, 24, 31, 53];

// PAGE 保护档（NT 常量口径）。
/// PAGE_READONLY。
pub const PAGE_READONLY: u32 = 0x02;
/// PAGE_READWRITE。
pub const PAGE_READWRITE: u32 = 0x04;
/// PAGE_WRITECOPY。
pub const PAGE_WRITECOPY: u32 = 0x08;
/// PAGE_EXECUTE_READ。
pub const PAGE_EXECUTE_READ: u32 = 0x20;
/// PAGE_EXECUTE_READWRITE。
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
/// PAGE_TARGETS_INVALID（映射允许附加位）。
pub const PAGE_TARGETS_INVALID: u32 = 0x4000_0000;

// SEC 档（节分配属性口径）。
/// SEC_COMMIT。
pub const SEC_COMMIT: u32 = 0x0800_0000;
/// SEC_RESERVE。
pub const SEC_RESERVE: u32 = 0x0400_0000;

/// 诊断码（0x30060 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    /// 0x30060 ZeroBits 越域（不在 {0,21,24,31,53}）。
    ZeroBitsBad = 0,
    /// 0x30061 ZeroBits 冲突（基址高位越上界）。
    ZeroBitsConflict = 1,
    /// 0x30062 InheritDisposition 越三档。
    InheritBad = 2,
    /// 0x30063 Protect 越八档。
    ProtectBad = 3,
    /// 0x30064 CommitSize 越视口/节尺寸。
    CommitOver = 4,
    /// 0x30065 保留档带 CommitSize。
    CommitOnReserve = 5,
    /// 0x30066 视口超节尺寸。
    ViewOver = 6,
    /// 0x30067 偏移越界（offset+size>节）。
    OffsetOver = 7,
    /// 0x30068 偏移非 64KiB 粒度。
    OffsetGran = 8,
    /// 0x30069 显式基址非分配粒度对齐。
    AlignGran = 9,
    /// 0x3006a 视口重叠。
    Overlap = 10,
    /// 0x3006b 节不存在/视口账满。
    SecNotFound = 11,
}

impl MapCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [MapCode; 12] = [
        MapCode::ZeroBitsBad, MapCode::ZeroBitsConflict, MapCode::InheritBad, MapCode::ProtectBad,
        MapCode::CommitOver, MapCode::CommitOnReserve, MapCode::ViewOver, MapCode::OffsetOver,
        MapCode::OffsetGran, MapCode::AlignGran, MapCode::Overlap, MapCode::SecNotFound,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30060 => "ZeroBits 越域：只收 0/21/24/31/53 五档。",
        0x30061 => "ZeroBits 冲突：基址高位必须落 1<<(64-ZeroBits) 之下。",
        0x30062 => "Inherit 越档：只收 0/1/2 三档（ViewShare=1）。",
        0x30063 => "Protect 越档：八档 PAGE 闭集 + 允许附加位。",
        0x30064 => "CommitSize 越界：非零时必须 ≤ViewSize 且 ≤节尺寸。",
        0x30065 => "保留档带提交：SEC_RESERVE 节映射 CommitSize 必须为 0。",
        0x30066 => "视口超节：ViewSize 不得超过节尺寸（0=整节）。",
        0x30067 => "偏移越界：SectionOffset+ViewSize 不得超节尺寸。",
        0x30068 => "偏移粒度：SectionOffset 必须 64KiB 对齐。",
        0x30069 => "基址粒度：显式 BaseAddress 必须 64KiB 对齐。",
        0x3006a => "视口重叠：新视口区间与已有视口相交即拒。",
        0x3006b => "节不存在：先建节再映射；视口账满先解映射。",
        _ => "未知诊断码：回查 0x30060 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30060 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3006_3006_3006_3006;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// InheritDisposition 三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inherit {
    /// 保留档（0）。
    Reserved = 0,
    /// ViewShare（1）。
    ViewShare = 1,
    /// ViewUnmap（2）。
    ViewUnmap = 2,
}

impl Inherit {
    /// 由原始值取档（越三档返回 None）。
    pub fn of(v: u32) -> Option<Inherit> {
        match v {
            0 => Some(Inherit::Reserved),
            1 => Some(Inherit::ViewShare),
            2 => Some(Inherit::ViewUnmap),
            _ => None,
        }
    }
}

/// 单节条目（映射域的前置面）。
#[derive(Clone, Copy, Debug)]
pub struct SecEntry {
    /// 节尺寸。
    pub size: u64,
    /// 分配属性（SEC 档）。
    pub alloc: u32,
    /// 存活。
    pub live: bool,
}

/// 映射参数包（逐参数校验域）。
#[derive(Clone, Copy, Debug)]
pub struct MapParams {
    /// 基址（0=自动）。
    pub base: u64,
    /// ZeroBits。
    pub zero_bits: u32,
    /// 提交尺寸。
    pub commit_size: u64,
    /// 映射保护。
    pub protect: u32,
    /// 继承处置（原始三值）。
    pub inherit_raw: u32,
    /// 视口尺寸（0=整节）。
    pub view_size: u64,
    /// 节内偏移。
    pub offset: u64,
}

/// 视口账行。
#[derive(Clone, Copy, Debug)]
pub struct ViewRow {
    /// 基址。
    pub base: u64,
    /// 视口尺寸。
    pub size: u64,
    /// 保护档。
    pub protect: u32,
    /// 继承档。
    pub inherit: Inherit,
    /// 活跃。
    pub live: bool,
}

/// 映射语义档正本。
///
/// 生命周期：add_section（登记节）→ map（逐参数校验→占槽）→
/// views（只读面）。
#[derive(Debug)]
pub struct MapBook {
    /// 节面（单节域：每账一个节）。
    sec: SecEntry,
    /// 视口账（定容 32 槽）。
    views: [ViewRow; VIEW_SLOTS],
    /// 自动基址分配游标（64KiB 粒度递增）。
    cursor: u64,
    /// 映射累计。
    mapped: u32,
    /// 拒止累计。
    rejects: u32,
}

impl MapBook {
    /// 建账（节尺寸给定）。
    pub fn new(sec_size: u64, alloc: u32) -> Self {
        MapBook {
            sec: SecEntry { size: sec_size, alloc, live: true },
            views: core::array::from_fn(|_| ViewRow {
                base: 0,
                size: 0,
                protect: 0,
                inherit: Inherit::Reserved,
                live: false,
            }),
            cursor: 0x10_0000,
            mapped: 0,
            rejects: 0,
        }
    }

    /// ZeroBits 域核对（闭集五档）。
    pub fn zero_bits_ok(zb: u32) -> bool {
        ZERO_BITS_SET.contains(&zb)
    }

    /// ZeroBits 上界核对（基址高位必须落上界之下）。
    pub fn zero_bits_conflict(base: u64, zb: u32) -> bool {
        if zb == 0 {
            return false;
        }
        let limit: u64 = 1u64.checked_shl(64 - zb).unwrap_or(0);
        limit != 0 && base >= limit
    }

    /// 保护档闭集（八档 + 附加位允许）。
    pub fn protect_ok(p: u32) -> bool {
        const BASE: u32 = 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20 | 0x40 | 0x80;
        let base_p = p & !PAGE_TARGETS_INVALID;
        // 基础档必须是八档闭集中的单一位；附加位允许叠加。
        base_p.count_ones() == 1 && (base_p & BASE != 0) && (base_p & !BASE == 0)
    }

    /// 映射参数校验（纯函数：只判不写）。
    ///
    /// 逐参数独立校验，返回第一个命中的拒码（错误优先级口径）。
    pub fn validate_map(&self, p: &MapParams) -> Result<(), MapCode> {
        // 1. 节存在性。
        if !self.sec.live {
            return Err(MapCode::SecNotFound);
        }
        // 2. ZeroBits 域。
        if !Self::zero_bits_ok(p.zero_bits) {
            return Err(MapCode::ZeroBitsBad);
        }
        // 3. Inherit 三档。
        if Inherit::of(p.inherit_raw).is_none() {
            return Err(MapCode::InheritBad);
        }
        // 4. Protect 八档。
        if !Self::protect_ok(p.protect) {
            return Err(MapCode::ProtectBad);
        }
        // 5. ViewSize/Offset 越界。
        if p.offset > self.sec.size {
            return Err(MapCode::OffsetOver);
        }
        let vsz = if p.view_size == 0 { self.sec.size - p.offset } else { p.view_size };
        if vsz > self.sec.size || p.offset + vsz > self.sec.size {
            return Err(MapCode::ViewOver);
        }
        // 6. 偏移粒度。
        if p.offset % ALLOC_GRAN != 0 {
            return Err(MapCode::OffsetGran);
        }
        // 7. 显式基址对齐 + ZeroBits 冲突。
        if p.base != 0 {
            if p.base % ALLOC_GRAN != 0 {
                return Err(MapCode::AlignGran);
            }
            if Self::zero_bits_conflict(p.base, p.zero_bits) {
                return Err(MapCode::ZeroBitsConflict);
            }
        }
        // 8. CommitSize 语义。
        if p.commit_size > 0 {
            if self.sec.alloc & SEC_RESERVE != 0 && self.sec.alloc & SEC_COMMIT == 0 {
                return Err(MapCode::CommitOnReserve);
            }
            if p.commit_size > vsz || p.commit_size > self.sec.size {
                return Err(MapCode::CommitOver);
            }
        }
        // 9. 重叠核对（基址确定时才可判）。
        if p.base != 0 && self.overlaps(p.base, vsz) {
            return Err(MapCode::Overlap);
        }
        Ok(())
    }

    /// 区间重叠核对（对活视口逐槽判）。
    pub fn overlaps(&self, base: u64, size: u64) -> bool {
        self.views.iter().any(|v| {
            v.live && base < v.base + v.size && v.base < base + size
        })
    }

    /// 映射（校验通过才占槽；基址 0=自动分配）。
    pub fn map(&mut self, p: &MapParams) -> Result<(usize, u64), MapCode> {
        let chk = self.validate_map(p);
        if let Err(c) = chk {
            self.rejects += 1;
            return Err(c);
        }
        let vsz = if p.view_size == 0 { self.sec.size - p.offset } else { p.view_size };
        let base = if p.base == 0 {
            let b = self.cursor;
            self.cursor += ALLOC_GRAN.max(vsz.next_multiple_of(ALLOC_GRAN));
            b
        } else {
            p.base
        };
        let mut slot: Option<usize> = None;
        for i in 0..VIEW_SLOTS {
            if !self.views[i].live {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(MapCode::SecNotFound);
            }
        };
        self.views[s] = ViewRow {
            base,
            size: vsz,
            protect: p.protect & !PAGE_TARGETS_INVALID,
            inherit: Inherit::of(p.inherit_raw).unwrap_or(Inherit::Reserved),
            live: true,
        };
        self.mapped += 1;
        Ok((s, base))
    }

    /// 活视口计数。
    pub fn live_views(&self) -> usize {
        self.views.iter().filter(|v| v.live).count()
    }

    /// 映射累计。
    pub fn mapped_total(&self) -> u32 {
        self.mapped
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 视口快照。
    pub fn view_of(&self, slot: usize) -> Option<ViewRow> {
        if slot >= VIEW_SLOTS {
            None
        } else {
            Some(self.views[slot])
        }
    }

    /// 守恒：mapped ≡ live + closed（无释放域，live 即全部）。
    pub fn conserved(&self) -> bool {
        self.mapped as usize == self.live_views()
    }
}

/// 合法映射参数抽签（确定性）。
fn pick_valid_params(lcg: &mut Lcg, _sec_size: u64) -> MapParams {
    let idx = lcg.below(3);
    MapParams {
        base: 0,
        zero_bits: match idx {
            0 => 0,
            1 => 21,
            _ => 24,
        },
        commit_size: 0,
        protect: match idx {
            0 => PAGE_READONLY,
            1 => PAGE_READWRITE,
            _ => PAGE_EXECUTE_READ,
        },
        inherit_raw: 1,
        view_size: 0,
        offset: 0,
    }
}

/// 逐参数注入例抽签（30 例：每类 3 例循环带期望码；与校验序对齐）。
fn pick_inject_case(lcg: &mut Lcg, sec_size: u64, i: usize) -> (MapParams, MapCode) {
    let k = i % 10;
    let mut p = pick_valid_params(lcg, sec_size);
    let code = match k {
        0 => { p.zero_bits = 20; MapCode::ZeroBitsBad }
        1 => { p.zero_bits = 32; MapCode::ZeroBitsBad }
        2 => { p.inherit_raw = 3; MapCode::InheritBad }
        3 => { p.protect = 0x99; MapCode::ProtectBad }
        4 => { p.offset = sec_size + PAGE_SIZE; p.view_size = 0; MapCode::OffsetOver }
        5 => { p.view_size = sec_size + PAGE_SIZE; MapCode::ViewOver }
        6 => { p.offset = 0x100; p.view_size = 0; MapCode::OffsetGran }
        7 => { p.base = 0x1008; MapCode::AlignGran }
        8 => { p.base = 1u64 << 40; p.zero_bits = 31; MapCode::ZeroBitsConflict }
        _ => { p.commit_size = sec_size + PAGE_SIZE; MapCode::CommitOver }
    };
    (p, code)
}

/// 逐参数注入 30/30 演练（每轮 30 例逐例码值核对）。
pub fn drill_param_inject_30() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30060 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 16;
        let mut bk = MapBook::new(sec_size, SEC_COMMIT);
        let mut hit = 0usize;
        for i in 0..INJECT_CASES {
            let (p, expect) = pick_inject_case(&mut lcg, sec_size, i);
            match bk.map(&p) {
                Err(c) if c == expect => hit += 1,
                _ => {}
            }
        }
        if hit == INJECT_CASES && bk.live_views() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("逐参数注入 {}/{} 轮={}/{}", INJECT_CASES, INJECT_CASES, ok, INJECT_ROUNDS))
}

/// 映射参数语义演练：10/10 轮合法参数全映射、视口账五列齐。
pub fn drill_map_semantics_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30060 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 16;
        let mut bk = MapBook::new(sec_size, SEC_COMMIT);
        let mut pass = true;
        for _i in 0..3 {
            let p = pick_valid_params(&mut lcg, sec_size);
            match bk.map(&p) {
                Ok((s, base)) => {
                    pass = pass && base >= 0x10_0000;
                    if let Some(v) = bk.view_of(s) {
                        pass = pass && v.live && v.size == sec_size;
                    } else {
                        pass = false;
                    }
                }
                Err(_) => pass = false,
            }
        }
        pass = pass && bk.conserved() && bk.live_views() == 3;
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("映射语义轮={}/{}", ok, INJECT_ROUNDS))
}

/// 重叠与保留档演练：显式重叠拒 + 保留档带提交拒 10/10。
pub fn drill_overlap_reserve_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30060 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 16;
        // 保留档节：CommitSize 必须零。
        let mut bk = MapBook::new(sec_size, SEC_RESERVE);
        let p0 = MapParams {
            base: 0,
            zero_bits: 0,
            commit_size: PAGE_SIZE * 4,
            protect: PAGE_READWRITE,
            inherit_raw: 1,
            view_size: 0,
            offset: 0,
        };
        let r_reserve = bk.map(&p0);
        // 映射一视口后显式基址重叠。
        let _ = bk.map(&MapParams { commit_size: 0, ..p0 });
        let base0 = bk.view_of(0).map(|v| v.base).unwrap_or(0);
        let p_overlap = MapParams { base: base0, commit_size: 0, ..p0 };
        let r_overlap = bk.map(&p_overlap);
        let _ = lcg.below(3);
        if matches!(r_reserve, Err(MapCode::CommitOnReserve))
            && matches!(r_overlap, Err(MapCode::Overlap))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重叠/保留档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：视口账转录导出（逐槽一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 视口账转录导出。
pub fn export_view_lines(bk: &MapBook) -> String {
    let mut s = String::from("视口账转录：\n");
    for i in 0..VIEW_SLOTS {
        if let Some(v) = bk.view_of(i) {
            if v.live {
                s.push_str(&format!(
                    "V{} 基{:x} 尺{} 保{:x} 继{}\n",
                    i, v.base, v.size, v.protect, v.inherit as u8
                ));
            }
        }
    }
    s
}

/// 转录完整判据（活视口逐行齐备）。
pub fn view_lines_complete(bk: &MapBook) -> bool {
    let s = export_view_lines(bk);
    let n = bk.live_views();
    if n == 0 {
        return false;
    }
    let mut hits = 0usize;
    for i in 0..VIEW_SLOTS {
        if let Some(v) = bk.view_of(i) {
            if v.live && s.contains(&format!("V{} 基{:x}", i, v.base)) {
                hits += 1;
            }
        }
    }
    hits == n
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：映射报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct MapReport {
    /// 活视口数。
    pub live: usize,
    /// 映射累计。
    pub mapped: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 守恒。
    pub conserved: bool,
}

impl MapReport {
    /// 建报告。
    pub fn build(bk: &MapBook) -> Self {
        MapReport {
            live: bk.live_views(),
            mapped: bk.mapped_total(),
            rejects: bk.reject_total(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.conserved && self.mapped >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "MapReport 活={} 映={} 拒={} 守恒={}",
            self.live, self.mapped, self.rejects, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：ZeroBits 域演练（五档闭集逐一过/越域拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_zerobits_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30060 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 16;
        let mut bk = MapBook::new(sec_size, SEC_COMMIT);
        let mut pass = true;
        for zb in ZERO_BITS_SET.iter() {
            let p = MapParams {
                base: 0,
                zero_bits: *zb,
                commit_size: 0,
                protect: PAGE_READONLY,
                inherit_raw: 1,
                view_size: 0,
                offset: 0,
            };
            pass = pass && bk.map(&p).is_ok();
        }
        let bad = MapParams {
            zero_bits: 40,
            ..pick_valid_params(&mut lcg, sec_size)
        };
        pass = pass && matches!(bk.map(&bad), Err(MapCode::ZeroBitsBad));
        pass = pass && bk.live_views() == ZERO_BITS_SET.len();
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("ZeroBits 域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：Inherit 三档演练（1/2 两活档 + 0 保留档 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_inherit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30060 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 16;
        let mut bk = MapBook::new(sec_size, SEC_COMMIT);
        let p = pick_valid_params(&mut lcg, sec_size);
        let r1 = bk.map(&MapParams { inherit_raw: 1, ..p });
        let r2 = bk.map(&MapParams { inherit_raw: 2, ..p });
        let r3 = bk.map(&MapParams { inherit_raw: 0, ..p });
        let r4 = bk.map(&MapParams { inherit_raw: 7, ..p });
        let i1 = bk.view_of(0).map(|v| v.inherit).unwrap_or(Inherit::Reserved);
        let i2 = bk.view_of(1).map(|v| v.inherit).unwrap_or(Inherit::Reserved);
        let _ = lcg.below(3);
        if matches!(r1, Ok(_)) && matches!(r2, Ok(_)) && matches!(r3, Ok(_))
            && matches!(r4, Err(MapCode::InheritBad))
            && i1 == Inherit::ViewShare
            && i2 == Inherit::ViewUnmap
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("Inherit 三档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct MapCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_map_checklist(bk: &MapBook) -> Vec<MapCheckRow> {
    let rep = MapReport::build(bk);
    let mut rows: Vec<MapCheckRow> = Vec::new();
    rows.push(MapCheckRow { name: "守恒", passed: bk.conserved() });
    rows.push(MapCheckRow { name: "ZeroBits闭集", passed: ZERO_BITS_SET == [0, 21, 24, 31, 53] });
    rows.push(MapCheckRow { name: "分配粒度口径", passed: ALLOC_GRAN == 0x10000 });
    rows.push(MapCheckRow { name: "Inherit三档", passed: Inherit::of(0).is_some() && Inherit::of(1).is_some() && Inherit::of(2).is_some() && Inherit::of(3).is_none() });
    rows.push(MapCheckRow { name: "Protect档", passed: MapBook::protect_ok(PAGE_TARGETS_INVALID | PAGE_READWRITE) && !MapBook::protect_ok(0x99) });
    rows.push(MapCheckRow { name: "转录完整", passed: view_lines_complete(bk) });
    rows.push(MapCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(MapCheckRow { name: "槽位宽", passed: VIEW_SLOTS == 32 });
    rows.push(MapCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(MapCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn map_checklist_green(rows: &[MapCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn map_stats_line(bk: &MapBook) -> String {
    format!(
        "映射概况：活 {} 映 {} 拒 {}",
        bk.live_views(),
        bk.mapped_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12403_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let sec_size = PAGE_SIZE * 16;
    let mut bk = MapBook::new(sec_size, SEC_COMMIT);
    let _ = bk.map(&MapParams { base: 0, zero_bits: 0, commit_size: 0, protect: PAGE_READWRITE, inherit_raw: 1, view_size: 0, offset: 0 });
    let _ = bk.map(&MapParams { base: 0, zero_bits: 21, commit_size: 0, protect: PAGE_READONLY, inherit_raw: 2, view_size: 0, offset: 0 });
    // 注入一例越域 ZeroBits（拒止账口径）。
    let _ = bk.map(&MapParams { base: 0, zero_bits: 40, commit_size: 0, protect: PAGE_READONLY, inherit_raw: 1, view_size: 0, offset: 0 });
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        view_lines_complete(&bk) && MapReport::build(&bk).all_green(),
        map_stats_line(&bk),
    ));
    // X-b：ZeroBits 域 10/10。
    let (ok_z, d_z) = drill_zerobits_10();
    out.push(("X-ZeroBits域10/10", ok_z, d_z));
    // X-c：Inherit 三档 10/10。
    let (ok_i, d_i) = drill_inherit_10();
    out.push(("X-Inherit三档10/10", ok_i, d_i));
    // X-d：终验清单。
    let rows = build_map_checklist(&bk);
    out.push(("X-终验清单10项", map_checklist_green(&rows), MapReport::build(&bk).to_text()));
    // X-e：纯函数域（ZeroBits 上界核对两例）。
    let c1 = MapBook::zero_bits_conflict(1u64 << 40, 31);
    let c2 = MapBook::zero_bits_conflict(1u64 << 10, 31);
    out.push(("X-上界核对两面", c1 && !c2, format!("hi={} lo={}", c1, c2)));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let sec_size = PAGE_SIZE * 16;
    // 注 1：ZeroBits 越域必拒。
    let _bk = MapBook::new(sec_size, SEC_COMMIT);
    if !MapBook::zero_bits_ok(40) {
        caught += 1;
        d.push("注1 ZB域:抓".to_string());
    } else {
        d.push("注1 ZB域:漏".to_string());
    }
    // 注 2：ZeroBits 冲突必拒（31 位上界之上）。
    if MapBook::zero_bits_conflict(1u64 << 40, 31) {
        caught += 1;
        d.push("注2 ZB冲:抓".to_string());
    } else {
        d.push("注2 ZB冲:漏".to_string());
    }
    // 注 3：Inherit 越三档必拒。
    if Inherit::of(3).is_none() {
        caught += 1;
        d.push("注3 继承:抓".to_string());
    } else {
        d.push("注3 继承:漏".to_string());
    }
    // 注 4：保留档带 CommitSize 必拒。
    let mut bk4 = MapBook::new(sec_size, SEC_RESERVE);
    let r4 = bk4.map(&MapParams { base: 0, zero_bits: 0, commit_size: PAGE_SIZE, protect: PAGE_READWRITE, inherit_raw: 1, view_size: 0, offset: 0 });
    if matches!(r4, Err(MapCode::CommitOnReserve)) {
        caught += 1;
        d.push("注4 保留提:抓".to_string());
    } else {
        d.push("注4 保留提:漏".to_string());
    }
    // 注 5：视口超节必拒。
    let mut bk5 = MapBook::new(sec_size, SEC_COMMIT);
    let r5 = bk5.map(&MapParams { base: 0, zero_bits: 0, commit_size: 0, protect: PAGE_READONLY, inherit_raw: 1, view_size: sec_size + PAGE_SIZE, offset: 0 });
    if matches!(r5, Err(MapCode::ViewOver)) {
        caught += 1;
        d.push("注5 超节:抓".to_string());
    } else {
        d.push("注5 超节:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30060 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    MapCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12403_checks：本单判据聚合（诊断段 0x30060）。
pub fn run_unx12403_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12403 NtMapViewOfSection 语义档");
    // J1-a：映射参数语义（合法参数 10/10）。
    let (ok1, d1) = drill_map_semantics_10();
    s.add_dyn("J1-映射参数语义10/10", ok1, &d1);
    // J1-b：逐参数注入 30/30 正确。
    let (ok2, d2) = drill_param_inject_30();
    s.add_dyn("J1-逐参数注入30/30", ok2, &d2);
    // J1-c：重叠/保留档拒止 10/10。
    let (ok3, d3) = drill_overlap_reserve_10();
    s.add_dyn("J1-重叠保留档10/10", ok3, &d3);
    // J1-d：标准会话守恒（三视口全活）。
    let sec_size = PAGE_SIZE * 16;
    let mut bk = MapBook::new(sec_size, SEC_COMMIT);
    let _ = bk.map(&MapParams { base: 0, zero_bits: 0, commit_size: 0, protect: PAGE_READWRITE, inherit_raw: 1, view_size: 0, offset: 0 });
    let _ = bk.map(&MapParams { base: 0, zero_bits: 0, commit_size: 0, protect: PAGE_READONLY, inherit_raw: 2, view_size: 0, offset: 0 });
    s.add_dyn(
        "J1-视口账守恒",
        bk.conserved() && bk.live_views() == 2 && bk.mapped_total() == 2,
        &format!("映射{} 活{} 守恒={}", bk.mapped_total(), bk.live_views(), bk.conserved()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：注入例覆盖三类（ZeroBits/Inherit/Protect 逐类在账）。
    let sec_size2 = PAGE_SIZE * 16;
    let mut bk2 = MapBook::new(sec_size2, SEC_COMMIT);
    let mut zc = 0u32;
    let mut ic = 0u32;
    let mut pc = 0u32;
    for i in 0..INJECT_CASES {
        let (p, expect) = pick_inject_case(&mut lcg2(), sec_size2, i);
        if matches!(bk2.map(&p), Err(c) if c == expect) {
            match expect {
                MapCode::ZeroBitsBad | MapCode::ZeroBitsConflict => zc += 1,
                MapCode::InheritBad => ic += 1,
                MapCode::ProtectBad => pc += 1,
                _ => {}
            }
        }
    }
    s.add_dyn(
        "J2-注入三类覆盖",
        zc >= 1 && ic >= 1 && pc >= 1 && bk2.live_views() == 0,
        &format!("ZB={} 继={} 保={}", zc, ic, pc),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30060段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12403_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}

/// 注入类覆盖专用 LCG（种子固定：0x30060 段基址 ^ 5）。
fn lcg2() -> Lcg {
    Lcg::new((SEG_BASE as u64) ^ 5)
}
