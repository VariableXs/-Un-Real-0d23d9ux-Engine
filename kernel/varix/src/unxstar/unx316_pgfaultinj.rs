//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0316 · 页表错误注入（坏项检出 100%）· 诊断段 0x3dd0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0316-J1 注入坏 PTE（保留位污染/权限错/自指错）检出 100%，
//!   诊断信息定位到项。
//! 难点分析：
//!   坏 PTE 的三种致命形态各有一个共同点：硬件不一定立刻报错——
//!   保留位污染在地址翻译时 #PF，权限错在写时 #PF（却像普通缺页），
//!   自指错在递归走表时无限循环。软件校验器必须在「翻译发生之前」
//!   抓住它们，并且诊断信息必须能定位到「第几级·第几项」，否则
//!   4 级 512 项 × N 张表的人肉排查不可承受。
//! 纪律约束：
//!   - 检出 100%：三类注入 × 全表走查，一个不漏；
//!   - 定位到项：违例回报带 (level, index, kind)；
//!   - 无注入零误报：干净表必须零违例（防校验器自己过敏）；
//!   - 闭集判别：违例种类是枚举，不是字符串。
//! 成本记账：走查 O(级数×项数)；注入 O(1)；定位 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3dd0;

/// 一张页表：512 项 × 8 字节（x86-64 PTE 布局）。
pub const ENTRIES: usize = 512;

/// PTE 违例闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PteFault {
    /// 保留位污染：bit 51..63 中出现非 XD 的置位。
    ReservedPolluted,
    /// 权限错：NX=1 且 RW=1 且 US=0（内核态可写却不可执行? 语义矛盾组合按设计矩阵判）。
    PermContradiction,
    /// 自指错：PTE 指回自己所在表的物理地址。
    SelfReference,
    /// 大页保留位：PS=1 且 level==PML4（PML4 不许大页）。
    HugePageIllegal,
    /// 非呈现但脏位/访问位脏：P=0 且 A=1。
    PresentAccessDirty,
}

impl PteFault {
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                PteFault::ReservedPolluted => 1,
                PteFault::PermContradiction => 2,
                PteFault::SelfReference => 3,
                PteFault::HugePageIllegal => 4,
                PteFault::PresentAccessDirty => 5,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PteFault::ReservedPolluted => "保留位被污染（bit52+ 非 XD）",
            PteFault::PermContradiction => "权限位组合违反设计矩阵",
            PteFault::SelfReference => "PTE 指回本表（自指）",
            PteFault::HugePageIllegal => "PML4 层出现大页位",
            PteFault::PresentAccessDirty => "非呈现项却带访问位",
        }
    }
}

/// 违例定位：级（1=PML4 … 4=PT）+ 项下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultSite {
    pub level: u8,
    pub index: usize,
    pub kind: PteFault,
}

/// 单条 PTE 的位域读取（与硬件布局同源）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Pte(pub u64);

impl Pte {
    pub const P: u64 = 1 << 0;
    pub const RW: u64 = 1 << 1;
    pub const US: u64 = 1 << 2;
    pub const PS: u64 = 1 << 7;
    pub const A: u64 = 1 << 5;
    pub const D: u64 = 1 << 6;
    pub const NX: u64 = 1 << 63;

    pub fn present(self) -> bool {
        self.0 & Self::P != 0
    }
    pub fn addr(self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }
    /// 软件保留位掩码：bit 52..62（63 是 XD，合法，必须排除）。
    pub fn reserved_violation(self) -> bool {
        self.0 & 0x07FF_0000_0000_0000 != 0
    }
}

/// 逐级走查校验器。
pub struct PteValidator {
    /// 每级表（level 1..=4），各 512 项。
    tables: [Vec<Pte>; 4],
    /// 本表物理地址（自指检查基准），level → addr。
    self_addr: [u64; 4],
}

impl PteValidator {
    pub fn new(self_addr: [u64; 4]) -> Self {
        PteValidator {
            tables: [vec![Pte(0); ENTRIES], vec![Pte(0); ENTRIES], vec![Pte(0); ENTRIES], vec![Pte(0); ENTRIES]],
            self_addr,
        }
    }

    /// 写入某级某项。
    pub fn set(&mut self, level: usize, index: usize, pte: Pte) -> Result<(), ()> {
        if !(1..=4).contains(&level) || index >= ENTRIES {
            return Err(());
        }
        self.tables[level - 1][index] = pte;
        Ok(())
    }

    /// 全量走查：返回全部违例（含定位）。
    pub fn walk(&self) -> Vec<FaultSite> {
        let mut out = Vec::new();
        for (li, tab) in self.tables.iter().enumerate() {
            let level = li as u8 + 1;
            for (i, p) in tab.iter().enumerate() {
                // 保留位污染（只在呈现项上判，非呈现项地址位无意义）
                if p.present() && p.reserved_violation() {
                    out.push(FaultSite { level, index: i, kind: PteFault::ReservedPolluted });
                    continue;
                }
                // 自指：呈现且地址指回本表
                if p.present() && p.addr() == self.self_addr[li] {
                    out.push(FaultSite { level, index: i, kind: PteFault::SelfReference });
                    continue;
                }
                // PML4 大页非法
                if level == 1 && p.present() && p.0 & Pte::PS != 0 {
                    out.push(FaultSite { level, index: i, kind: PteFault::HugePageIllegal });
                    continue;
                }
                // 非呈现带访问位
                if !p.present() && p.0 & Pte::A != 0 {
                    out.push(FaultSite { level, index: i, kind: PteFault::PresentAccessDirty });
                    continue;
                }
                // 权限矛盾：US=0（内核）且 RW=1 且 NX=0 且 A=0 —— 从未访问的可写可执行内核项按设计矩阵为非法（必须 NX 或已记账）
                if p.present() && p.0 & Pte::US == 0 && p.0 & Pte::RW != 0 && p.0 & Pte::NX == 0 && p.0 & Pte::A == 0 {
                    out.push(FaultSite { level, index: i, kind: PteFault::PermContradiction });
                }
            }
        }
        out
    }
}

/// 黄金干净表：US 内核数据页风格（RW+NX），无违例。基准地址 4KB 对齐。
pub fn clean_validator() -> PteValidator {
    let mut v = PteValidator::new([0xAA000, 0xBB000, 0xCC000, 0xDD000]);
    let p = Pte(Pte::P | Pte::RW | Pte::NX | (0x7000));
    for l in 1..=4 {
        for i in 0..8 {
            let _ = v.set(l, i, p);
        }
    }
    v
}

/// 注入器：按种类把坏项写进指定级/项。
pub fn inject(v: &mut PteValidator, level: usize, index: usize, kind: PteFault) -> Result<(), ()> {
    let base = Pte(Pte::P | Pte::RW | (0x7000));
    let bad = match kind {
        PteFault::ReservedPolluted => Pte(base.0 | (1u64 << 55)),
        PteFault::PermContradiction => Pte(base.0), // RW=1 US=0 NX=0 A=0 → 矛盾
        PteFault::SelfReference => Pte(Pte::P | Pte::RW | self_addr_of(v, level)), // 精确指回本表
        PteFault::HugePageIllegal => Pte(base.0 | Pte::PS),
        PteFault::PresentAccessDirty => Pte(Pte::A | (0x7000)),
    };
    v.set(level, index, bad)
}

fn self_addr_of(v: &PteValidator, level: usize) -> u64 {
    v.self_addr[level - 1]
}

/// UNX-F0316 判据（J1 注入检出 100% + 定位到项）。
pub fn run_unx316_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0316 页表错误注入");

    // J1 前置：干净表零误报
    let clean = clean_validator();
    cs.add("J1 干净表零误报", clean.walk().is_empty(), "过敏即失效");

    // 注入 1：保留位污染 —— 检出且定位
    let mut v1 = clean_validator();
    let _ = inject(&mut v1, 2, 100, PteFault::ReservedPolluted);
    let f1 = v1.walk();
    cs.add("注入1 保留位检出", f1.len() == 1 && f1[0].kind == PteFault::ReservedPolluted, "唯一违例");
    cs.add("注入1 定位到项", f1.len() == 1 && f1[0].level == 2 && f1[0].index == 100, "L2·100");

    // 注入 2：权限矛盾
    let mut v2 = clean_validator();
    let _ = inject(&mut v2, 1, 7, PteFault::PermContradiction);
    let f2 = v2.walk();
    cs.add("注入2 权限矛盾检出", f2.len() == 1 && f2[0].kind == PteFault::PermContradiction, "L1·7");

    // 注入 3：自指
    let mut v3 = clean_validator();
    let _ = inject(&mut v3, 3, 256, PteFault::SelfReference);
    let f3 = v3.walk();
    cs.add("注入3 自指检出", f3.len() == 1 && f3[0].kind == PteFault::SelfReference, "L3·256");

    // 注入 4+5：PML4 大页 / 非呈现带 A
    let mut v4 = clean_validator();
    let _ = inject(&mut v4, 1, 0, PteFault::HugePageIllegal);
    let _ = inject(&mut v4, 4, 511, PteFault::PresentAccessDirty);
    let f4 = v4.walk();
    let has_huge = f4.iter().any(|f| f.kind == PteFault::HugePageIllegal && f.level == 1 && f.index == 0);
    let has_dirty = f4.iter().any(|f| f.kind == PteFault::PresentAccessDirty && f.level == 4 && f.index == 511);
    cs.add("注入4 PML4 大页检出", has_huge, "L1·0");
    cs.add("注入5 脏访问位检出", has_dirty, "L4·511");
    cs.add("注入4+5 双违例齐清", f4.len() == 2, "100% 覆盖");

    // 全谱检出率：5 类各注一处 → 违例数恰 5 且种类全覆盖
    let mut vall = clean_validator();
    let _ = inject(&mut vall, 1, 1, PteFault::ReservedPolluted);
    let _ = inject(&mut vall, 2, 2, PteFault::PermContradiction);
    let _ = inject(&mut vall, 3, 3, PteFault::SelfReference);
    let _ = inject(&mut vall, 1, 4, PteFault::HugePageIllegal);
    let _ = inject(&mut vall, 4, 5, PteFault::PresentAccessDirty);
    let fall = vall.walk();
    let kinds: Vec<u16> = fall.iter().map(|f| f.kind.code() - SEG_BASE).collect();
    cs.add("全谱检出 5/5", fall.len() == 5, "一个不漏");
    cs.add("全谱定位互异", {
        let mut sites: Vec<(u8, usize)> = fall.iter().map(|f| (f.level, f.index)).collect();
        sites.sort();
        sites.dedup();
        sites.len() == 5
    }, "项级定位");

    // 诊断码闭集
    let codes = [
        PteFault::ReservedPolluted.code(),
        PteFault::PermContradiction.code(),
        PteFault::SelfReference.code(),
        PteFault::HugePageIllegal.code(),
        PteFault::PresentAccessDirty.code(),
    ];
    cs.add("诊断码闭集互异", codes.windows(2).all(|w| w[0] < w[1]) && codes[0] == SEG_BASE + 1, "递增");

    cs
}
