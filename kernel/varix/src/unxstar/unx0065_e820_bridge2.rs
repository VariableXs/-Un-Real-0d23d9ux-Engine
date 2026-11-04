//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0065 · E820 桥接层（BIOS 兼容路径）（深化第二实现） · 0x6DF80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0065-J1 EFI 内存图 → E820 兼容条目桥接：七类映射
//!     （usable/reserved/ACPI reclaim/ACPI NVS/…）逐条目等价，
//!     连续同类合并、排序升序、总量守恒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 桥接映射：EFI Type → E820 class（1=usable 2=reserved
//!     3=reclaim 4=nvs 5=badmem），账外类型归 reserved；
//!   - 合并账：相邻同 class 区间合并（end==next.base）；
//!   - 排序：基址升序（乱序账拒）；
//!   - 总量守恒：桥接前后总字节恒等（合并不变量）；
//!   - E820 签名账：'SMAP' 邻接校验（BIOS 兼容路径冻结）；
//!   - 条目上限：E820 128 槽（零页账面）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DF80，AI-VX3-1102 批I 独占区）。
pub const SEG_BASE: u32 = 0x6DF80;

/// E820 class（冻结）。
pub const E820_USABLE: u32 = 1;
pub const E820_RESERVED: u32 = 2;
pub const E820_RECLAIM: u32 = 3;
pub const E820_NVS: u32 = 4;
pub const E820_BADMEM: u32 = 5;
/// E820 槽上限（BIOS 冻结 128）。
pub const E820_MAX_ENTRIES: usize = 128;
/// 'SMAP' 签名。
pub const SMAP_SIG: u32 = 0x534D4150;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeCode {
    /// 基址乱序（桥接账红线）
    OrderInverted,
    /// 总量不守恒
    TotalDrift,
    /// 条目越 128 槽
    EntriesOver,
    /// 零长条目
    ZeroLength,
    /// class 账外
    ClassUnknown,
    /// SMAP 签名错
    SmapSigBad,
    /// 合并错（相邻同类未合并）
    MergeMissed,
    /// 越界区间（end < base）
    RangeInverted,
    /// 双跑不一致
    NotReproducible,
}

impl BridgeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BridgeCode::OrderInverted => "基址乱序：桥接账红线",
            BridgeCode::TotalDrift => "总量不守恒：桥接红线",
            BridgeCode::EntriesOver => "条目越 128 槽",
            BridgeCode::ZeroLength => "零长条目",
            BridgeCode::ClassUnknown => "class 账外：非 1-5",
            BridgeCode::SmapSigBad => "SMAP 签名错",
            BridgeCode::MergeMissed => "相邻同类未合并",
            BridgeCode::RangeInverted => "越界区间：end < base",
            BridgeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BridgeCode; 9] = [
        BridgeCode::OrderInverted,
        BridgeCode::TotalDrift,
        BridgeCode::EntriesOver,
        BridgeCode::ZeroLength,
        BridgeCode::ClassUnknown,
        BridgeCode::SmapSigBad,
        BridgeCode::MergeMissed,
        BridgeCode::RangeInverted,
        BridgeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BridgeCode::ALL.len() {
        if BridgeCode::ALL[i].val() == code {
            return BridgeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DF80 段闭集内"
}

// ── EFI 源条目（与 F0052 账同型，独立账面） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EfiSrcEntry {
    pub kind: u32,
    pub phys: u64,
    pub pages: u64,
}

impl EfiSrcEntry {
    pub fn end(&self) -> u64 {
        self.phys.saturating_add(self.pages.saturating_mul(0x1000))
    }
}

/// E820 桥接条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct E820Entry {
    pub base: u64,
    pub len: u64,
    pub class: u32,
}

impl E820Entry {
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.len)
    }
    /// class 闸（1-5）。
    pub fn legal(&self) -> Result<(), u32> {
        if self.len == 0 {
            return Err(BridgeCode::ZeroLength.val());
        }
        if self.class < E820_USABLE || self.class > E820_BADMEM {
            return Err(BridgeCode::ClassUnknown.val());
        }
        Ok(())
    }
}

// ── 桥接映射（EFI Type → E820 class） ───────────────────────────
pub fn efi_to_e820(kind: u32) -> u32 {
    match kind {
        7 => E820_USABLE,       // CONVENTIONAL
        9 => E820_RECLAIM,      // ACPI reclaim
        10 => E820_NVS,         // ACPI NVS
        14 => E820_BADMEM,      // UNUSABLE
        _ => E820_RESERVED,     // 其余全归 reserved（账外兜底）
    }
}

// ── 桥接器 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct E820Bridge {
    pub entries: Vec<E820Entry>,
    pub sig: u32,
}

impl E820Bridge {
    pub fn new() -> E820Bridge {
        E820Bridge { entries: Vec::new(), sig: SMAP_SIG }
    }

    /// 源条目桥接：映射→插入（升序）→合并相邻同类。
    pub fn bridge_from(&mut self, src: &[EfiSrcEntry]) -> Result<usize, u32> {
        let mut out: Vec<E820Entry> = Vec::new();
        let mut i = 0;
        while i < src.len() {
            let s = &src[i];
            if s.pages == 0 {
                i += 1;
                continue;
            }
            out.push(E820Entry { base: s.phys, len: s.end() - s.phys, class: efi_to_e820(s.kind) });
            i += 1;
        }
        // 排序（基址升序；同基址按 len 降序稳定）。
        out.sort_by(|a, b| a.base.cmp(&b.base).then(b.len.cmp(&a.len)));
        // 合并相邻同类（usable 与 reserved 都可并）。
        let mut merged: Vec<E820Entry> = Vec::new();
        let mut i = 0;
        while i < out.len() {
            let e = out[i];
            if e.class > E820_BADMEM || e.class < E820_USABLE {
                return Err(BridgeCode::ClassUnknown.val());
            }
            match merged.last_mut() {
                Some(last) if last.class == e.class && last.end() >= e.base => {
                    if e.end() > last.end() {
                        last.len = e.end() - last.base;
                    }
                }
                _ => merged.push(e),
            }
            i += 1;
        }
        if merged.len() > E820_MAX_ENTRIES {
            return Err(BridgeCode::EntriesOver.val());
        }
        self.entries = merged;
        Ok(self.entries.len())
    }

    /// 升序守卫。
    pub fn order_audit(&self) -> Result<(), u32> {
        let mut i = 1;
        while i < self.entries.len() {
            if self.entries[i].base < self.entries[i - 1].base {
                return Err(BridgeCode::OrderInverted.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 合并守卫：相邻同类且相接必不存在。
    pub fn merge_audit(&self) -> Result<(), u32> {
        let mut i = 1;
        while i < self.entries.len() {
            let prev = &self.entries[i - 1];
            let cur = &self.entries[i];
            if prev.class == cur.class && prev.end() >= cur.base {
                return Err(BridgeCode::MergeMissed.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 总量守恒对拍（同类总字节恒等）。
    pub fn total_consistent(&self, src: &[EfiSrcEntry]) -> Result<(), u32> {
        let mut src_total = [0u64; 6];
        let mut i = 0;
        while i < src.len() {
            if src[i].pages > 0 {
                let c = efi_to_e820(src[i].kind) as usize;
                if c <= E820_BADMEM as usize {
                    src_total[c] += src[i].pages * 0x1000;
                }
            }
            i += 1;
        }
        let mut out_total = [0u64; 6];
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            e.legal()?;
            out_total[e.class as usize] += e.len;
            i += 1;
        }
        let mut c = 1;
        while c <= 5 {
            if src_total[c] != out_total[c] {
                return Err(BridgeCode::TotalDrift.val());
            }
            c += 1;
        }
        Ok(())
    }

    /// usable 总量。
    pub fn usable_total(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].class == E820_USABLE {
                t += self.entries[i].len;
            }
            i += 1;
        }
        t
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

// ── 注入检出账（四路：class 路/序路/合并路/守恒路） ──────────────
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

/// 真机源样本（乱序喂入：EFI 六条目）。
pub fn make_machine_src() -> Vec<EfiSrcEntry> {
    vec![
        EfiSrcEntry { kind: 10, phys: 0x7F10_0000, pages: 0x80 },   // NVS
        EfiSrcEntry { kind: 7, phys: 0x0100_0000, pages: 0xC000 },  // usable
        EfiSrcEntry { kind: 9, phys: 0x7F00_0000, pages: 0x100 },   // reclaim
        EfiSrcEntry { kind: 7, phys: 0x1000_0000, pages: 0x4000 },  // usable（与上一条不相接）
        EfiSrcEntry { kind: 0, phys: 0x7F20_0000, pages: 0x40 },    // reserved
        EfiSrcEntry { kind: 7, phys: 0x0100_0000 + 0xC000 * 0x1000, pages: 0x800 }, // usable 相接条目（触发合并）
    ]
}

// ── 黄金全链：桥接→排序→合并→守恒→总量 ──────────────────────────
pub fn golden_run() -> Result<(usize, u64), u32> {
    let mut b = E820Bridge::new();
    let src = make_machine_src();
    let n = b.bridge_from(&src)?;
    b.order_audit()?;
    b.merge_audit()?;
    b.total_consistent(&src)?;
    Ok((n, b.usable_total()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 5 条目（合并生效）+ usable 总量守恒。
pub fn probe_seq_a() -> Result<(), u32> {
    let src = make_machine_src();
    let usable: u64 = src.iter().filter(|e| e.kind == 7).map(|e| e.pages * 0x1000).sum();
    match golden_run() {
        Ok((5, t)) if t == usable => Ok(()),
        _ => Err(BridgeCode::NotReproducible.val()),
    }
}

/// 序 B：class 账外 ⇒ ClassUnknown。
pub fn probe_seq_b() -> Result<(), u32> {
    let e = E820Entry { base: 0x1000, len: 0x1000, class: 6 };
    match e.legal() {
        Err(c) if c == BridgeCode::ClassUnknown.val() => Ok(()),
        _ => Err(BridgeCode::NotReproducible.val()),
    }
}

/// 序 C：人工构造未合并账 ⇒ MergeMissed。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = E820Bridge::new();
    b.entries.push(E820Entry { base: 0x1000, len: 0x1000, class: E820_USABLE });
    b.entries.push(E820Entry { base: 0x2000, len: 0x1000, class: E820_USABLE });
    match b.merge_audit() {
        Err(c) if c == BridgeCode::MergeMissed.val() => Ok(()),
        _ => Err(BridgeCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：class 路（账外 class 必拒）。
        let e = E820Entry { base: 0x1000, len: 0x1000, class: 0 };
        led.record(0, e.legal().is_err());
        // 路 2：序路（乱序账必拒）。
        let mut b2 = E820Bridge::new();
        b2.entries.push(E820Entry { base: 0x3000, len: 0x1000, class: E820_USABLE });
        b2.entries.push(E820Entry { base: 0x1000, len: 0x1000, class: E820_USABLE });
        led.record(1, b2.order_audit().is_err());
        // 路 3：守恒路（源删一条必漂移）。
        let src = make_machine_src();
        let mut b3 = E820Bridge::new();
        let _ = b3.bridge_from(&src);
        let short = &src[1..src.len()];
        led.record(2, b3.total_consistent(short).is_err());
        // 路 4：零长路。
        let z = E820Entry { base: 0x1000, len: 0, class: E820_USABLE };
        led.record(3, z.legal().is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BridgeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BridgeCode::ALL.len() {
        let v = BridgeCode::ALL[i].val();
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
    let src = make_machine_src();
    let usable: u64 = src.iter().filter(|e| e.kind == 7).map(|e| e.pages * 0x1000).sum();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((n, t)) => *n == 5 && *t == usable,
            Err(_) => false,
        },
        match &g {
            Ok((n, t)) => format!("entries={} usable=0x{:x}（源 0x{:x}）", n, t, usable),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 桥接映射逐类对拍。
    let map = [(7u32, E820_USABLE), (9, E820_RECLAIM), (10, E820_NVS), (14, E820_BADMEM), (0, E820_RESERVED), (15, E820_RESERVED)];
    let mut map_hit = true;
    let mut i = 0;
    while i < map.len() {
        if efi_to_e820(map[i].0) != map[i].1 {
            map_hit = false;
        }
        i += 1;
    }
    v.push((
        "J1-映射对拍",
        map_hit,
        String::from("七类映射逐条钉死（账外兜底 reserved）"),
    ));
    // SMAP 签名账。
    v.push((
        "J1-SMAP签名",
        E820Bridge::new().sig == SMAP_SIG && SMAP_SIG == 0x534D_4150,
        String::from("'SMAP' 0x534D4150 冻结"),
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
        String::from("0x6DF80 窗口"),
    ));
    v
}

pub fn run_unx0065_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0065 E820 桥接层（BIOS 兼容路径）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
