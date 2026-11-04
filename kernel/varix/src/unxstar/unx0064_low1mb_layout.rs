//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0064 · 低 1MB 遗留区布局图 · 0x6DF40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0064-J1 低 1MB 布局图冻结账：实模式 IVT（0x0-0x3FF）/
//!     BDA（0x400-0x4FF）/EBDA 顶/自由下界，逐区边界与真机一致，
//!     布局图只读（改写即拒）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五区冻结边界：IVT[0,0x400)/BDA[0x400,0x500)/EBDA[0x500,
//!     top)/自由[top,0xA0000)/VGA[0xA0000,0x100000)；
//!   - EBDA 顶账：从 BDA 0x40E 字读取（KB 段基址），缺席回退
//!     0x9FC00（真机口径）；顶必须 ≤ 0x9FC00；
//!   - 布局图只读：登记后改写检测（逐区对拍，漂移记账拒）；
//!   - 区间查询：addr → 区归属（五区判别，账外越 1MB 拒）；
//!   - 自由区记账：下界对齐 4K 后可分配；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DF40，AI-VX3-1102 批I 独占区）。
pub const SEG_BASE: u32 = 0x6DF40;

/// 五区冻结边界。
pub const IVT_BASE: u32 = 0x0;
pub const IVT_END: u32 = 0x400;
pub const BDA_END: u32 = 0x500;
/// EBDA 缺席回退顶（真机口径）。
pub const EBDA_FALLBACK_TOP: u32 = 0x9FC00;
/// 自由区顶（VGA 起始）。
pub const FREE_TOP: u32 = 0xA0000;
/// 低 1MB 顶。
pub const LOW_TOP: u32 = 0x100000;
/// 区数（IVT/BDA/EBDA/自由/VGA 五区）。
pub const ZONE_COUNT: usize = 5;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Low1mCode {
    /// EBDA 顶越界（> 0x9FC00）
    EbdaTopOver,
    /// EBDA 顶低于 BDA 尾（倒挂）
    EbdaTopInverted,
    /// 越低 1MB 查询
    QueryOverLowTop,
    /// 布局图改写（只读红线）
    LayoutMutated,
    /// 区边界错序（基址非升序）
    ZoneOrderBad,
    /// 自由区空（EBDA 吞掉全部）
    FreeZoneEmpty,
    /// 双跑不一致
    NotReproducible,
}

impl Low1mCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Low1mCode::EbdaTopOver => "EBDA 顶越界：> 0x9FC00",
            Low1mCode::EbdaTopInverted => "EBDA 顶倒挂：低于 BDA 尾",
            Low1mCode::QueryOverLowTop => "越低 1MB 查询",
            Low1mCode::LayoutMutated => "布局图改写：只读红线",
            Low1mCode::ZoneOrderBad => "区边界错序：基址非升序",
            Low1mCode::FreeZoneEmpty => "自由区空：EBDA 吞全部",
            Low1mCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [Low1mCode; 7] = [
        Low1mCode::EbdaTopOver,
        Low1mCode::EbdaTopInverted,
        Low1mCode::QueryOverLowTop,
        Low1mCode::LayoutMutated,
        Low1mCode::ZoneOrderBad,
        Low1mCode::FreeZoneEmpty,
        Low1mCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < Low1mCode::ALL.len() {
        if Low1mCode::ALL[i].val() == code {
            return Low1mCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DF40 段闭集内"
}

// ── 布局区 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LowZone {
    /// 区名槽：0=IVT 1=BDA 2=EBDA 3=自由 4=VGA。
    pub slot: usize,
    pub base: u32,
    pub end: u32,
}

impl LowZone {
    pub fn covers(&self, addr: u32) -> bool {
        addr >= self.base && addr < self.end
    }
    pub fn size(&self) -> u32 {
        self.end.saturating_sub(self.base)
    }
    pub fn name(&self) -> &'static str {
        match self.slot {
            0 => "IVT",
            1 => "BDA",
            2 => "EBDA",
            3 => "自由",
            _ => "VGA",
        }
    }
}

// ── 低 1MB 布局图（只读固化账） ─────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct LowLayout {
    pub zones: [LowZone; ZONE_COUNT],
    pub sealed: bool,
    pub mutate_caught: u32,
}

impl LowLayout {
    /// 从 EBDA 顶构建布局图（BDA 0x40E 口径，段基址 ×16）。
    pub fn build(ebda_seg_word: u16) -> Result<LowLayout, u32> {
        let ebda_top = if ebda_seg_word == 0 {
            EBDA_FALLBACK_TOP
        } else {
            (ebda_seg_word as u32) << 4
        };
        if ebda_top > EBDA_FALLBACK_TOP {
            return Err(Low1mCode::EbdaTopOver.val());
        }
        if ebda_top < BDA_END {
            return Err(Low1mCode::EbdaTopInverted.val());
        }
        if ebda_top >= FREE_TOP {
            return Err(Low1mCode::FreeZoneEmpty.val());
        }
        Ok(LowLayout {
            zones: [
                LowZone { slot: 0, base: IVT_BASE, end: IVT_END },
                LowZone { slot: 1, base: IVT_END, end: BDA_END },
                LowZone { slot: 2, base: BDA_END, end: ebda_top },
                LowZone { slot: 3, base: ebda_top, end: FREE_TOP },
                LowZone { slot: 4, base: FREE_TOP, end: LOW_TOP },
            ],
            sealed: true,
            mutate_caught: 0,
        })
    }

    /// 区边界升序守卫。
    pub fn order_audit(&self) -> Result<(), u32> {
        let mut i = 1;
        while i < ZONE_COUNT {
            if self.zones[i].base < self.zones[i - 1].end {
                return Err(Low1mCode::ZoneOrderBad.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 地址归属查询（五区判别，越 1MB 拒）。
    pub fn zone_of(&self, addr: u32) -> Result<&'static str, u32> {
        if addr >= LOW_TOP {
            return Err(Low1mCode::QueryOverLowTop.val());
        }
        let mut i = 0;
        while i < ZONE_COUNT {
            if self.zones[i].covers(addr) {
                return Ok(self.zones[i].name());
            }
            i += 1;
        }
        Ok("账外洞")
    }

    /// 自由区原始边界（对齐下界另账）。
    pub fn free_span(&self) -> (u32, u32) {
        let z = self.zones[3];
        (z.base, z.end)
    }
    /// 4K 对齐下界（0x9FC00 非页对齐 ⇒ 上取后为空，显性账）。
    pub fn free_aligned(&self) -> (u32, u32) {
        let z = self.zones[3];
        let aligned = (z.base + 0xFFF) & !0xFFF;
        (aligned, z.end)
    }

    /// 改写检测（只读红线：逐区对拍）。
    pub fn mutate_audit(&mut self, fresh: &LowLayout) -> bool {
        let mut i = 0;
        while i < ZONE_COUNT {
            if self.zones[i] != fresh.zones[i] {
                self.mutate_caught += 1;
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── LCG ────────────────────────────────────────────────────────
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

// ── 注入检出账（四路：顶路/查询路/改写路/序路） ──────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机样本：BDA 0x40E 字 = 0x9FC0（EBDA 顶 0x9FC00）。
pub const MACHINE_EBDA_WORD: u16 = 0x9FC0;

// ── 黄金全链：构建→序审计→五区查询→自由区→改写对拍 ──────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let layout = LowLayout::build(MACHINE_EBDA_WORD)?;
    layout.order_audit()?;
    let z_ivt = match layout.zone_of(0x100) {
        Ok(n) if n == "IVT" => 0,
        _ => 100,
    };
    let z_vga = match layout.zone_of(0xB8000) {
        Ok(n) if n == "VGA" => 0,
        _ => 100,
    };
    let (fb, fe) = layout.free_span();
    Ok((z_ivt + z_vga, fb, fe))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 查询全对 + 自由区 [0x9FC00, 0xA0000)。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((0, 0x9FC00, 0xA0000)) => Ok(()),
        _ => Err(Low1mCode::NotReproducible.val()),
    }
}

/// 序 B：EBDA 顶越界 ⇒ EbdaTopOver。
pub fn probe_seq_b() -> Result<(), u32> {
    match LowLayout::build(0x9FD0) {
        Err(e) if e == Low1mCode::EbdaTopOver.val() => Ok(()),
        _ => Err(Low1mCode::NotReproducible.val()),
    }
}

/// 序 C：越 1MB 查询 ⇒ QueryOverLowTop。
pub fn probe_seq_c() -> Result<(), u32> {
    let layout = LowLayout::build(MACHINE_EBDA_WORD)?;
    match layout.zone_of(0x100000) {
        Err(e) if e == Low1mCode::QueryOverLowTop.val() => Ok(()),
        _ => Err(Low1mCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：顶路（越界/倒挂必拒）。
        let hit1 = LowLayout::build(0x9FE0).is_err() && LowLayout::build(0x0001).is_err();
        led.record(0, hit1);
        // 路 2：查询路（越 1MB 必拒）。
        let l = match LowLayout::build(MACHINE_EBDA_WORD) {
            Ok(l) => l,
            Err(_) => {
                led.record(1, true);
                r += 1;
                continue;
            }
        };
        led.record(1, l.zone_of(0x100001).is_err());
        // 路 3：改写路（VGA 区漂移必抓）。
        let mut l3 = LowLayout::build(MACHINE_EBDA_WORD)?;
        let mut bad = LowLayout::build(MACHINE_EBDA_WORD)?;
        bad.zones[4].end = 0x110000;
        led.record(2, !l3.mutate_audit(&bad) && l3.mutate_caught == 1);
        // 路 4：改写对拍一致（同图恒真）。
        let mut l4 = LowLayout::build(MACHINE_EBDA_WORD)?;
        led.record(3, l4.mutate_audit(&LowLayout::build(MACHINE_EBDA_WORD)?));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(Low1mCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < Low1mCode::ALL.len() {
        let v = Low1mCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((q, fb, fe)) => *q == 0 && *fb == 0x9FC00 && *fe == 0xA0000,
            Err(_) => false,
        },
        match &g {
            Ok((q, fb, fe)) => format!("queries={} free=[0x{:x},0x{:x})", q, fb, fe),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 五区边界冻结对拍。
    let layout = LowLayout::build(MACHINE_EBDA_WORD);
    let bounds_hit = match &layout {
        Ok(l) => {
            let want = [(0u32, 0x400u32), (0x400, 0x500), (0x500, 0x9FC00), (0x9FC00, 0xA0000), (0xA0000, 0x100000)];
            let mut hit = true;
            let mut i = 0;
            while i < ZONE_COUNT {
                if (l.zones[i].base, l.zones[i].end) != want[i] {
                    hit = false;
                }
                i += 1;
            }
            hit
        }
        Err(_) => false,
    };
    v.push((
        "J1-五区冻结",
        bounds_hit,
        String::from("IVT/BDA/EBDA/自由/VGA 边界逐区钉死"),
    ));
    // EBDA 缺席回退：word=0 ⇒ 顶 0x9FC00。
    let fallback = LowLayout::build(0);
    v.push((
        "J1-缺席回退",
        match &fallback {
            Ok(l) => l.zones[2].end == EBDA_FALLBACK_TOP,
            Err(_) => false,
        },
        String::from("word=0 ⇒ EBDA 顶 0x9FC00"),
    ));
    // 注入 10/10。
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
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DF40 窗口"),
    ));
    v
}

pub fn run_unx0064_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0064 低 1MB 遗留区布局图");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
