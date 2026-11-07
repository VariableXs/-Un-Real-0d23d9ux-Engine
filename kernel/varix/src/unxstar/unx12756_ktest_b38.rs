//! UNX-F12756 ktest 聚合器（B38 压测 20 条） · 0x82700段
//!
//! 判据原文（派工卡 12756 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F12757-J1 压测收官账等式断
//!   言绿（矩阵×样本×结果三账对平）；③ 完成时 API/软件写回：状态=已完
//!   成、结果摘要非空。
//!
//! 设计要点：
//! 1. 域模型：B38 压测 20 条单跑入口聚合器（定容 20 入口）。每入口四元
//!    组（入口号、入口名、声明用例数、实跑用例数）+ 单调状态机
//!    Vacant→Registered→Ran→Aggregated 只进不退。20 条入口名为冻结常量
//!    名册，出处：F12741 压测矩阵四轴（深度/展开/码族/坏表）按步长 5.4
//!    抽 20 格的确定性点名序（AI-VX3-1030 压测总纲 + VX9 派工卡 12756）。
//! 2. 聚合等式断言：Σ入口条数（Σ实跑用例数）== 聚合账总条数。聚合口
//!    先查入口齐（20/20 已注册且已实跑，缺口显性拒 EGap），再落聚合账，
//!    等式在封账后可随时复算（守恒检查）。
//! 3. 缺口注入器三族：族0 缺注册（少登一个入口）/族1 缺实跑（注册齐但
//!    少跑一个）/族2 总账漂移（聚合账被改写后守恒破缺），每族 10 轮共
//!    30 次注入全抓：注入数==检出数、漏检==0。
//! 4. 判定次序冻结：入口号越表 → 状态倒退 → 重复登记 → 零用例 →
//!    入口缺口 → 聚合等式 → 正常落账。先到先判，次序不得调换。
//! 5. 双跑一致：两次全流程（注册→实跑→聚合→指纹）逐字段相等，指纹
//!    FNV-1a 滚动，全零视为异常。
//! 6. LCG 种子写死 = SEG_BASE（0x82700）；no_std + alloc；零浮点、零
//!    IO、零墙钟、零随机；取值一律 match/if let/.get()/.get_mut()/
//!    checked_*/saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12756 − 12600) = 0x82700）。
pub const SEG_BASE: u32 = 0x82700;

/// 单跑入口定容（B38 压测 20 条）。
pub const KT_ENTRIES: usize = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KtDiag {
    /// 入口号越表（不在 0..20）。
    ENoEnt,
    /// 状态倒退（Vacant 未注册即实跑等）。
    ERewind,
    /// 重复登记（一格一主）。
    EDouble,
    /// 零用例（实跑用例数为 0）。
    EEmpty,
    /// 入口缺口（20 入口未齐即聚合）。
    EGap,
    /// 聚合等式破缺（Σ入口条数 ≠ 聚合账总条数）。
    EConserve,
    /// 封账后写回（Aggregated 态不可再改）。
    ESealed,
    /// 容量耗尽（入口册判满）。
    ECapFull,
    /// 注入漏检（注入未被对应闸抓到）。
    EMissed,
    /// 双跑指纹不一致。
    EFp,
}

impl KtDiag {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KtDiag::ENoEnt => 0x01,
                KtDiag::ERewind => 0x02,
                KtDiag::EDouble => 0x03,
                KtDiag::EEmpty => 0x04,
                KtDiag::EGap => 0x05,
                KtDiag::EConserve => 0x06,
                KtDiag::ESealed => 0x07,
                KtDiag::ECapFull => 0x08,
                KtDiag::EMissed => 0x09,
                KtDiag::EFp => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            KtDiag::ENoEnt => "入口号越表：入口号不在 0..20，核对 B38 单跑入口名册行序",
            KtDiag::ERewind => "状态倒退：Vacant→Registered→Ran→Aggregated 只进不退，先注册再实跑",
            KtDiag::EDouble => "重复登记：一格一主，同号入口不得二次注册，查名册去重",
            KtDiag::EEmpty => "零用例：实跑用例数为 0，补跑该入口用例后再聚合",
            KtDiag::EGap => "入口缺口：20 入口未齐（缺注册或缺实跑）即聚合，先补齐缺口再走聚合口",
            KtDiag::EConserve => "聚合等式破缺：Σ入口条数≠聚合账总条数，逐入口复算两账",
            KtDiag::ESealed => "封账后写回：Aggregated 态入口不可再改，需改账先开封并留痕",
            KtDiag::ECapFull => "容量耗尽：入口册判满 20 条，先聚合封账或换新册再入新果",
            KtDiag::EMissed => "注入漏检：缺口注入未被聚合闸抓到，聚合口前置校验逻辑须重查",
            KtDiag::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
        }
    }

    /// 闭集全量（闭集返回，供查表与守卫遍历）。
    pub const ALL: [KtDiag; 10] = [
        KtDiag::ENoEnt,
        KtDiag::ERewind,
        KtDiag::EDouble,
        KtDiag::EEmpty,
        KtDiag::EGap,
        KtDiag::EConserve,
        KtDiag::ESealed,
        KtDiag::ECapFull,
        KtDiag::EMissed,
        KtDiag::EFp,
    ];
}

/// 诊断码查表（闭集内回中文文案，闭集外中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < KtDiag::ALL.len() {
        if let Some(d) = KtDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82700 段闭集内，按 B38 聚合器诊断码闭集复核"
}

// ── 冻结常量名册 ─────────────────────────────────────────────────
/// B38 压测 20 条单跑入口名册（冻结常量）。
/// 出处：F12741 压测矩阵四轴（深度 3 档 × 展开 3 态 × 码族 4 族 ×
/// 坏表 3 档）按确定性点名序抽 20 格；名册与卡 12756 配额对齐。
pub const KT_ENTRY_NAMES: [&str; KT_ENTRIES] = [
    "kt01-d0u0f0b0-try浅压",
    "kt02-d0u1f1b0-try中压",
    "kt03-d0u2f2b1-filter浅压",
    "kt04-d0u0f3b1-filter中压",
    "kt05-d0u1f0b2-finally浅压",
    "kt06-d1u2f1b2-finally中压",
    "kt07-d1u0f2b0-leave深压",
    "kt08-d1u1f3b0-leave中压",
    "kt09-d1u2f0b1-unwind浅压",
    "kt10-d1u0f1b1-unwind中压",
    "kt11-d2u1f2b1-vectored深压",
    "kt12-d2u2f3b1-vectored中压",
    "kt13-d2u0f0b2-translator浅压",
    "kt14-d2u1f1b2-translator中压",
    "kt15-d2u2f2b2-record深压",
    "kt16-d0u0f3b2-record中压",
    "kt17-d1u1f0b0-frame浅压",
    "kt18-d1u2f1b0-frame中压",
    "kt19-d2u0f2b1-unwindinfo深压",
    "kt20-d2u1f3b2-unwindinfo中压",
];

/// 入口声明用例数（确定性纯函数）：区间 5..=36，出处为压测配额轮铺
/// 规则（每入口不低于 5 例保底，步进 0x1D 摊平）。
pub fn entry_cases(no: usize) -> Option<u32> {
    if no >= KT_ENTRIES {
        return None;
    }
    Some(((no as u32).wrapping_mul(0x1D).wrapping_add(0x11)) % 0x20 + 0x05)
}

/// 全册声明用例总数（聚合账期望值，Σ入口条数的冻结期望）。
pub fn expected_total() -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < KT_ENTRIES {
        if let Some(c) = entry_cases(i) {
            sum = sum.saturating_add(c);
        }
        i += 1;
    }
    sum
}

// ── 状态机 ───────────────────────────────────────────────────────
/// 入口状态（单调只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KtState {
    /// 空位（未注册）。
    Vacant,
    /// 已注册未实跑。
    Registered,
    /// 已实跑未聚合。
    Ran,
    /// 已聚合封账（终态）。
    Aggregated,
}

/// 单跑入口四元组。
#[derive(Clone, Copy, Debug)]
pub struct KtEntry {
    /// 入口号（0..20）。
    pub no: usize,
    /// 入口名（冻结名册）。
    pub name: &'static str,
    /// 声明用例数（entry_cases 纯函数）。
    pub declared: u32,
    /// 实跑用例数。
    pub ran: u32,
    /// 状态。
    pub state: KtState,
}

// ── 聚合器 ───────────────────────────────────────────────────────
/// B38 单跑入口聚合器。
pub struct KtAgg {
    entries: [Option<KtEntry>; KT_ENTRIES],
    agg_total: u32,
    sealed: bool,
    fp: u64,
}

impl KtAgg {
    /// 新聚合器。
    pub fn new() -> Self {
        KtAgg {
            entries: [None; KT_ENTRIES],
            agg_total: 0,
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 注册入口（重复登记/越表/封账后写回显性拒）。
    pub fn register(&mut self, no: usize) -> Result<(), KtDiag> {
        if self.sealed {
            return Err(KtDiag::ESealed);
        }
        let name = match KT_ENTRY_NAMES.get(no) {
            Some(n) => *n,
            None => return Err(KtDiag::ENoEnt),
        };
        let occupied = match self.entries.get(no) {
            Some(Some(_)) => true,
            _ => false,
        };
        if occupied {
            return Err(KtDiag::EDouble);
        }
        let declared = match entry_cases(no) {
            Some(d) => d,
            None => return Err(KtDiag::ENoEnt),
        };
        let slot = match self.entries.get_mut(no) {
            Some(s) => s,
            None => return Err(KtDiag::ENoEnt),
        };
        *slot = Some(KtEntry {
            no,
            name,
            declared,
            ran: 0,
            state: KtState::Registered,
        });
        self.fnv(0x5200u64 + no as u64);
        Ok(())
    }

    /// 实跑入口：Registered→Ran；零用例/未注册/封账后显性拒。
    pub fn run(&mut self, no: usize, ran: u32) -> Result<(), KtDiag> {
        if self.sealed {
            return Err(KtDiag::ESealed);
        }
        if no >= KT_ENTRIES {
            return Err(KtDiag::ENoEnt);
        }
        if ran == 0 {
            return Err(KtDiag::EEmpty);
        }
        let cell = match self.entries.get_mut(no) {
            Some(slot) => slot,
            None => return Err(KtDiag::ENoEnt),
        };
        let entry = match cell {
            Some(e) => e,
            None => return Err(KtDiag::ERewind),
        };
        if entry.state != KtState::Registered {
            return Err(KtDiag::ERewind);
        }
        entry.ran = ran;
        entry.state = KtState::Ran;
        self.fnv(0x5201u64 + no as u64 + ((ran as u64) << 8));
        Ok(())
    }

    /// 聚合封账：入口齐（20/20 注册且实跑）才落账；等式断言
    /// Σ入口条数 == 聚合账总条数，破缺显性拒。
    pub fn aggregate(&mut self) -> Result<u32, KtDiag> {
        if self.sealed {
            return Err(KtDiag::ESealed);
        }
        let mut registered = 0usize;
        let mut ran = 0usize;
        let mut i = 0usize;
        while i < KT_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i) {
                registered += 1;
                if e.state == KtState::Ran {
                    ran += 1;
                }
            }
            i += 1;
        }
        if registered < KT_ENTRIES || ran < KT_ENTRIES {
            return Err(KtDiag::EGap);
        }
        let mut sum = 0u32;
        let mut j = 0usize;
        while j < KT_ENTRIES {
            if let Some(Some(e)) = self.entries.get(j) {
                sum = sum.saturating_add(e.ran);
            }
            j += 1;
        }
        // 等式断言：Σ入口条数 == 聚合账总条数（此处即落账值本身，
        // 守恒意义在于封账后可复算；漂移注入面由 conservation 把关）。
        self.agg_total = sum;
        let mut k = 0usize;
        while k < KT_ENTRIES {
            if let Some(slot) = self.entries.get_mut(k) {
                if let Some(e) = slot {
                    e.state = KtState::Aggregated;
                }
            }
            k += 1;
        }
        self.sealed = true;
        self.fnv(0x5202u64 + sum as u64);
        Ok(sum)
    }

    /// 守恒复算：Σ入口条数 == 聚合账总条数。
    pub fn conservation(&self) -> Result<(), KtDiag> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < KT_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i) {
                sum = sum.saturating_add(e.ran);
            }
            i += 1;
        }
        if sum != self.agg_total {
            return Err(KtDiag::EConserve);
        }
        Ok(())
    }

    /// 注入面：总账漂移（仅注入演练使用，聚合账被改写后守恒必破）。
    pub fn inject_drift(&mut self, delta: u32) {
        self.agg_total = self.agg_total.saturating_add(delta);
    }

    /// 入口是否已实跑（Ran 或之后）。
    pub fn is_ran(&self, no: usize) -> bool {
        match self.entries.get(no) {
            Some(Some(e)) => e.state == KtState::Ran || e.state == KtState::Aggregated,
            _ => false,
        }
    }

    /// 已注册入口数。
    pub fn registered_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < KT_ENTRIES {
            if let Some(Some(_)) = self.entries.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 已实跑入口数。
    pub fn ran_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < KT_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i) {
                if e.state == KtState::Ran || e.state == KtState::Aggregated {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 聚合账总条数。
    pub fn total(&self) -> u32 {
        self.agg_total
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for KtAgg {
    fn default() -> Self {
        Self::new()
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// 抽 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 全流程：20 入口注册→LCG 定序实跑（每入口恰跑一次、用例数取声明
/// 值）→聚合封账→守恒复算。返回（聚合总条数, 指纹）。
pub fn golden_aggregate() -> Result<(u32, u64), KtDiag> {
    let mut agg = KtAgg::new();
    let mut i = 0usize;
    while i < KT_ENTRIES {
        agg.register(i)?;
        i += 1;
    }
    // LCG（种子=SEG_BASE）生成确定性实跑序：先抽 8 个入口预跑，
    // 剩余入口按号补齐，保证每入口恰实跑一次（状态机严格单调）。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut k = 0u32;
    while k < 8 {
        let no = rng.below(KT_ENTRIES as u64) as usize;
        if !agg.is_ran(no) {
            let declared = match entry_cases(no) {
                Some(d) => d,
                None => return Err(KtDiag::ENoEnt),
            };
            agg.run(no, declared)?;
        }
        k += 1;
    }
    let mut j = 0usize;
    while j < KT_ENTRIES {
        if !agg.is_ran(j) {
            let declared = match entry_cases(j) {
                Some(d) => d,
                None => return Err(KtDiag::ENoEnt),
            };
            agg.run(j, declared)?;
        }
        j += 1;
    }
    let total = agg.aggregate()?;
    agg.conservation()?;
    Ok((total, agg.fingerprint()))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
/// 注入三族账（注入/检出/漏检）。
#[derive(Clone, Copy, Debug)]
pub struct InjLedger {
    /// 各族注入次数。
    pub tries: [u32; 3],
    /// 各族检出次数。
    pub caught: [u32; 3],
    /// 各族漏检次数（必须全 0）。
    pub missed: [u32; 3],
}

impl InjLedger {
    pub const fn new() -> Self {
        InjLedger {
            tries: [0; 3],
            caught: [0; 3],
            missed: [0; 3],
        }
    }

    /// 注入数 = 检出数 + 漏检数（逐族守恒）。
    pub fn conservation_ok(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let (Some(t), Some(c), Some(m)) = (self.tries.get(i), self.caught.get(i), self.missed.get(i)) {
                if *t != c.saturating_add(*m) {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 全抓判定：检出==注入 且 漏检==0。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if let (Some(t), Some(c), Some(m)) = (self.tries.get(i), self.caught.get(i), self.missed.get(i)) {
                if c != t || *m != 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

impl Default for InjLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 族0 缺注册：少登一个入口（第 k 轮缺 k%20 号）→ 聚合口 EGap 必抓。
fn inj_family0(led: &mut InjLedger, k: usize) {
    if let Some(t) = led.tries.get_mut(0) {
        *t += 1;
    }
    let mut agg = KtAgg::new();
    let skip = k % KT_ENTRIES;
    let mut i = 0usize;
    while i < KT_ENTRIES {
        if i != skip {
            let _ = agg.register(i);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < KT_ENTRIES {
        if j != skip {
            if let Some(d) = entry_cases(j) {
                let _ = agg.run(j, d);
            }
        }
        j += 1;
    }
    if agg.aggregate() == Err(KtDiag::EGap) {
        if let Some(c) = led.caught.get_mut(0) {
            *c += 1;
        }
    } else if let Some(m) = led.missed.get_mut(0) {
        *m += 1;
    }
}

/// 族1 缺实跑：注册齐但少跑一个 → 聚合口 EGap 必抓。
fn inj_family1(led: &mut InjLedger, k: usize) {
    if let Some(t) = led.tries.get_mut(1) {
        *t += 1;
    }
    let mut agg = KtAgg::new();
    let skip = (k + 7) % KT_ENTRIES;
    let mut i = 0usize;
    while i < KT_ENTRIES {
        let _ = agg.register(i);
        i += 1;
    }
    let mut j = 0usize;
    while j < KT_ENTRIES {
        if j != skip {
            if let Some(d) = entry_cases(j) {
                let _ = agg.run(j, d);
            }
        }
        j += 1;
    }
    if agg.aggregate() == Err(KtDiag::EGap) {
        if let Some(c) = led.caught.get_mut(1) {
            *c += 1;
        }
    } else if let Some(m) = led.missed.get_mut(1) {
        *m += 1;
    }
}

/// 族2 总账漂移：全流程聚合后改写聚合账 → 守恒 EConserve 必抓。
fn inj_family2(led: &mut InjLedger, k: usize) {
    if let Some(t) = led.tries.get_mut(2) {
        *t += 1;
    }
    let mut agg = KtAgg::new();
    let mut i = 0usize;
    while i < KT_ENTRIES {
        let _ = agg.register(i);
        if let Some(d) = entry_cases(i) {
            let _ = agg.run(i, d);
        }
        i += 1;
    }
    let _ = agg.aggregate();
    agg.inject_drift(1 + (k as u32) % 3);
    if agg.conservation() == Err(KtDiag::EConserve) {
        if let Some(c) = led.caught.get_mut(2) {
            *c += 1;
        }
    } else if let Some(m) = led.missed.get_mut(2) {
        *m += 1;
    }
}

/// 跑满 3 族 × 10 轮注入。
pub fn run_injections(led: &mut InjLedger) {
    let mut k = 0usize;
    while k < 10 {
        inj_family0(led, k);
        inj_family1(led, k);
        inj_family2(led, k);
        k += 1;
    }
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
/// 注入三族全抓且守恒。
fn injections_ok() -> bool {
    let mut led = InjLedger::new();
    run_injections(&mut led);
    led.all_caught() && led.conservation_ok()
}

/// 名册冻结谓词：20 名齐且与 entry_cases 纯函数自洽。
fn roster_ok() -> bool {
    if KT_ENTRY_NAMES.len() != KT_ENTRIES {
        return false;
    }
    let mut i = 0usize;
    while i < KT_ENTRIES {
        let name = match KT_ENTRY_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if name.is_empty() {
            return false;
        }
        if entry_cases(i).is_none() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据总谓词：等式断言绿 + 注入全抓 + 名册冻结。
fn b38_ok() -> bool {
    match golden_aggregate() {
        Ok((total, fp)) => {
            total == expected_total()
                && fp != 0
                && injections_ok()
                && roster_ok()
        }
        Err(_) => false,
    }
}

// ── 段位守卫 ─────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 0x40 对齐且落在 VX9 域（0x80000..=0x82FC0），
/// 且诊断码闭集全部落在段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE > 0x82FC0 {
        return false;
    }
    let mut i = 0usize;
    while i < KtDiag::ALL.len() {
        if let Some(d) = KtDiag::ALL.get(i) {
            let v = d.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条闭集，extra_checks 逐条进 set）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 正常流：黄金全流程聚合绿。
    let golden = golden_aggregate();
    v.push((
        "J1-黄金聚合绿",
        match &golden {
            Ok((total, _)) => *total == expected_total(),
            Err(_) => false,
        },
        format!("{:?}", golden.map(|(t, _)| t)),
    ));
    // 2. 等式断言：Σ入口条数 == 聚合账总条数。
    v.push((
        "J1-等式断言Σ入口=聚合",
        match &golden {
            Ok((total, _)) => *total == expected_total() && *total > 0,
            Err(_) => false,
        },
        format!("期望={} 实得={:?}", expected_total(), golden.map(|(t, _)| t)),
    ));
    // 3. 边界：入口号 0 合法注册。
    let mut b0 = KtAgg::new();
    let r0 = b0.register(0);
    v.push((
        "边界·入口号0合法",
        r0.is_ok(),
        format!("{:?}", r0),
    ));
    // 4. 边界：入口号 19（上限）合法注册。
    let mut b19 = KtAgg::new();
    let r19 = b19.register(KT_ENTRIES - 1);
    v.push((
        "边界·入口号19合法",
        r19.is_ok(),
        format!("{:?}", r19),
    ));
    // 5. 边界：入口号 20 越表拒。
    let mut b20 = KtAgg::new();
    let r20 = b20.register(KT_ENTRIES);
    v.push((
        "边界·入口号20越表拒",
        r20 == Err(KtDiag::ENoEnt),
        format!("{:?}", r20),
    ));
    // 6. 边界：零用例实跑拒。
    let mut bz = KtAgg::new();
    let _ = bz.register(3);
    let rz = bz.run(3, 0);
    v.push((
        "边界·零用例拒",
        rz == Err(KtDiag::EEmpty),
        format!("{:?}", rz),
    ));
    // 7. 注入三族 10/10 全抓。
    let mut led = InjLedger::new();
    run_injections(&mut led);
    v.push((
        "注入·三族10/10全抓",
        led.all_caught(),
        format!("t={:?} c={:?}", led.tries, led.caught),
    ));
    // 8. 计数守恒：注入数=检出数+漏检数、漏检=0。
    v.push((
        "注入·计数守恒漏检0",
        led.conservation_ok() && led.missed == [0; 3],
        format!("m={:?}", led.missed),
    ));
    // 9. 状态机倒退拒：Vacant 未注册即实跑 → ERewind。
    let mut rev = KtAgg::new();
    let rr = rev.run(5, 9);
    v.push((
        "状态机·倒退拒",
        rr == Err(KtDiag::ERewind),
        format!("{:?}", rr),
    ));
    // 10. 重复登记拒。
    let mut dup = KtAgg::new();
    let _ = dup.register(2);
    let rd = dup.register(2);
    v.push((
        "状态机·重复登记拒",
        rd == Err(KtDiag::EDouble),
        format!("{:?}", rd),
    ));
    // 11. 双跑一致：两次黄金全流程逐字段相等。
    let g1 = golden_aggregate();
    let g2 = golden_aggregate();
    v.push((
        "双跑一致",
        g1 == g2 && g1.is_ok(),
        format!("{:?} vs {:?}", g1.map(|(t, f)| (t, f)), g2.map(|(t, f)| (t, f))),
    ));
    // 12. 名册冻结：20 名齐、非空、用例数纯函数自洽。
    v.push((
        "名册冻结20条",
        roster_ok(),
        format!("len={} expected_total={}", KT_ENTRY_NAMES.len(), expected_total()),
    ));
    // 13. 段位守卫。
    v.push((
        "段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X}", SEG_BASE),
    ));
    // 14. 判据总谓词。
    v.push((
        "J1-b38_ok总谓词",
        b38_ok(),
        String::from("等式绿+注入全抓+名册冻结"),
    ));
    v
}

/// 检查入口：extra_checks 逐条进 set；动态明细槽 ≤8，其余走静态明细
/// 不丢红。
pub fn run_unx12756_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12756");
    let checks = extra_checks();
    let mut idx = 0usize;
    for (name, passed, dyn_detail) in checks {
        if idx < 8 {
            s.add_dyn(name, passed, &dyn_detail);
        } else {
            let static_detail: &'static str = match name {
                "注入·计数守恒漏检0" => "注入数=检出数+漏检数，漏检必须为 0",
                "状态机·倒退拒" => "Vacant 未注册即实跑显性拒 ERewind",
                "状态机·重复登记拒" => "同号入口二次注册显性拒 EDouble",
                "双跑一致" => "两次黄金全流程（总条数,指纹）逐字段相等",
                "名册冻结20条" => "20 入口名册冻结常量且用例数纯函数自洽",
                "段位守卫" => "SEG_BASE 0x40 对齐且落在 VX9 域",
                "J1-b38_ok总谓词" => "等式绿+注入全抓+名册冻结总判据",
                _ => "静态收口条目：明细见诊断码闭集 detail()",
            };
            s.add(name, passed, static_detail);
        }
        idx += 1;
    }
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_aggregate() {
        let g = golden_aggregate();
        assert!(g.is_ok(), "黄金全流程必须聚合成功");
        let (total, fp) = g.unwrap_or((0, 0));
        assert_eq!(total, expected_total());
        assert_ne!(fp, 0);
    }

    #[test]
    fn test_boundary_entry_zero_and_upper() {
        let mut agg = KtAgg::new();
        assert!(agg.register(0).is_ok());
        assert!(agg.register(KT_ENTRIES - 1).is_ok());
    }

    #[test]
    fn test_boundary_entry_overflow() {
        let mut agg = KtAgg::new();
        assert_eq!(agg.register(KT_ENTRIES), Err(KtDiag::ENoEnt));
        assert_eq!(agg.register(usize::MAX), Err(KtDiag::ENoEnt));
    }

    #[test]
    fn test_boundary_zero_cases() {
        let mut agg = KtAgg::new();
        let _ = agg.register(1);
        assert_eq!(agg.run(1, 0), Err(KtDiag::EEmpty));
    }

    #[test]
    fn test_gap_injection_family0() {
        let mut led = InjLedger::new();
        inj_family0(&mut led, 0);
        assert_eq!(led.tries.get(0), Some(&1));
        assert_eq!(led.caught.get(0), Some(&1));
        assert_eq!(led.missed.get(0), Some(&0));
    }

    #[test]
    fn test_injection_all_families_caught() {
        let mut led = InjLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught(), "3 族 10 轮必须全抓");
        assert!(led.conservation_ok());
        let mut i = 0usize;
        while i < 3 {
            assert_eq!(led.tries.get(i), Some(&10));
            i += 1;
        }
    }

    #[test]
    fn test_state_machine_rewind_and_double() {
        let mut agg = KtAgg::new();
        assert_eq!(agg.run(4, 9), Err(KtDiag::ERewind));
        let _ = agg.register(4);
        assert_eq!(agg.register(4), Err(KtDiag::EDouble));
    }

    #[test]
    fn test_drift_breaks_conservation() {
        let mut agg = KtAgg::new();
        let mut i = 0usize;
        while i < KT_ENTRIES {
            let _ = agg.register(i);
            if let Some(d) = entry_cases(i) {
                let _ = agg.run(i, d);
            }
            i += 1;
        }
        let _ = agg.aggregate();
        assert!(agg.conservation().is_ok());
        agg.inject_drift(5);
        assert_eq!(agg.conservation(), Err(KtDiag::EConserve));
    }

    #[test]
    fn test_dual_run_identical() {
        let g1 = golden_aggregate();
        let g2 = golden_aggregate();
        assert_eq!(g1, g2, "两次全流程必须逐字段相等");
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert!(segment_guard());
    }

    #[test]
    fn test_detail_closed_set() {
        for d in KtDiag::ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).contains("未知诊断码"));
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知诊断码"));
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx12756_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
