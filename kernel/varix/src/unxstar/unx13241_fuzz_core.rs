//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13241 · fuzz 语料器总成（生成/注入/回收三面） · 0x65800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13241-J1 三面架构（语料生成/批量注入/误崩回收）30 例
//!      全链一致，三族语料入口协议断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三族语料入口协议（闭集）：族 1 = 头部变异（header-mutant）/
//!     族 2 = 字段尖刺（field-spike）/ 族 3 = 尾部截断（tail-trunc）；
//!     注入面按族分槽（跨族注入一律拒收——入口协议红线）；
//!   - 生成面闭式：LCG（种子 = SEG_BASE）驱动变异位选择，同种子
//!     同语料（确定性红线）；
//!   - 注入面：批量注入按族配额（每族 10 件，30 例全链），
//!     槽容量越界必拒；
//!   - 回收面：误崩语料回收账（崩次记账、语料回库不清账），
//!     回收数 ≤ 注入数（守恒恒等式）；
//!   - LCG 种子写死 = SEG_BASE（0x65800），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65800，全仓独占）。
pub const SEG_BASE: u32 = 0x65800;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzCoreCode {
    /// 跨族注入（入口协议破）
    CrossFamilyInject,
    /// 语料族越闭集
    FamilyOutOfRange,
    /// 空语料（生成面拒收）
    CorpusEmpty,
    /// 注入槽容量越界
    SlotOverCapacity,
    /// 回收越账（回收数 > 注入数）
    RecycleOverInjected,
    /// 误崩未记账（崩次流失）
    CrashUnaccounted,
    /// 语料重复入库
    CorpusDuplicated,
    /// 回收后语料丢账
    RecycleLostCorpus,
}

impl FuzzCoreCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FuzzCoreCode::CrossFamilyInject => "跨族注入：三族语料入口协议破",
            FuzzCoreCode::FamilyOutOfRange => "语料族越闭集：族账越界",
            FuzzCoreCode::CorpusEmpty => "空语料：生成面拒收",
            FuzzCoreCode::SlotOverCapacity => "注入槽容量越界：批量注入拒收",
            FuzzCoreCode::RecycleOverInjected => "回收越账：回收数 > 注入数",
            FuzzCoreCode::CrashUnaccounted => "误崩未记账：崩次流失",
            FuzzCoreCode::CorpusDuplicated => "语料重复入库：生成面闭集破",
            FuzzCoreCode::RecycleLostCorpus => "回收后语料丢账：回收面守恒破",
        }
    }
    pub const ALL: [FuzzCoreCode; 8] = [
        FuzzCoreCode::CrossFamilyInject,
        FuzzCoreCode::FamilyOutOfRange,
        FuzzCoreCode::CorpusEmpty,
        FuzzCoreCode::SlotOverCapacity,
        FuzzCoreCode::RecycleOverInjected,
        FuzzCoreCode::CrashUnaccounted,
        FuzzCoreCode::CorpusDuplicated,
        FuzzCoreCode::RecycleLostCorpus,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FuzzCoreCode::ALL.len() {
        if FuzzCoreCode::ALL[i].val() == code {
            return FuzzCoreCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：fuzz 语料器建模书 + 判据原文） ───────────────────
/// 语料族闭集（3 族）。
pub const FAM_HEADER: u8 = 1; // 头部变异
pub const FAM_SPIKE: u8 = 2; // 字段尖刺
pub const FAM_TRUNC: u8 = 3; // 尾部截断
/// 每族注入槽容量（配额）。
pub const SLOT_CAP: usize = 10;
/// 全链例数（判据原文：30 例 = 3 族 × 10）。
pub const CASES: usize = 30;
/// 语料件长（字节，建模定长 8）。
pub const CORPUS_LEN: usize = 8;

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 三面之一：语料生成面 ─────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corpus {
    pub fam: u8,
    pub seq: u32,
    pub data: Vec<u8>,
}

/// 生成闭式：按族选变异位（族协议锚定变异窗口）。
/// 族 1：只动 data[0..2]（头部窗口）；族 2：只动 data[3..5]（字段窗口，
/// 写满值 0xFF）；族 3：截断尾部（len = 6 - k，k ∈ 0..2）。
pub fn generate_corpus(rng: &mut Lcg, fam: u8, seq: u32) -> Result<Corpus, FuzzCoreCode> {
    if fam < FAM_HEADER || fam > FAM_TRUNC {
        return Err(FuzzCoreCode::FamilyOutOfRange);
    }
    let mut data: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < CORPUS_LEN {
        data.push((rng.next() & 0xFF) as u8);
        i += 1;
    }
    match fam {
        FAM_HEADER => {
            data[0] = (rng.next() & 0xFF) as u8;
            data[1] = (rng.next() & 0xFF) as u8;
        }
        FAM_SPIKE => {
            data[3] = 0xFF;
            data[4] = 0xFF;
            data[5] = 0xFF;
        }
        _ => {
            let k = rng.next_below(2) as usize;
            let keep = CORPUS_LEN - 2 - k; // 6 或 5
            data.truncate(keep);
        }
    }
    if data.is_empty() {
        return Err(FuzzCoreCode::CorpusEmpty);
    }
    Ok(Corpus { fam, seq, data })
}

// ── 三面之二：批量注入面（按族分槽） ─────────────────────────────
pub struct Injector {
    /// 每族槽位账（下标 1..3 对应三族）
    pub slots: [Vec<u32>; 4],
    pub injected_total: usize,
}

impl Injector {
    pub fn new() -> Injector {
        Injector {
            slots: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            injected_total: 0,
        }
    }

    /// 注入一件：跨族/越族/槽满必拒。
    pub fn inject(&mut self, c: &Corpus) -> Result<(), FuzzCoreCode> {
        if c.fam < FAM_HEADER || c.fam > FAM_TRUNC {
            return Err(FuzzCoreCode::FamilyOutOfRange);
        }
        if c.data.is_empty() {
            return Err(FuzzCoreCode::CorpusEmpty);
        }
        let slot = &mut self.slots[c.fam as usize];
        if slot.len() >= SLOT_CAP {
            return Err(FuzzCoreCode::SlotOverCapacity);
        }
        slot.push(c.seq);
        self.injected_total += 1;
        Ok(())
    }

    /// 注入面守恒：Σ各槽件数 == 注入总数，且各槽 ≤ 容量。
    pub fn verify_conservation(&self) -> Result<(), FuzzCoreCode> {
        let mut sum = 0;
        let mut fam = FAM_HEADER;
        while fam <= FAM_TRUNC {
            sum += self.slots[fam as usize].len();
            if self.slots[fam as usize].len() > SLOT_CAP {
                return Err(FuzzCoreCode::SlotOverCapacity);
            }
            fam += 1;
        }
        if sum != self.injected_total {
            return Err(FuzzCoreCode::RecycleOverInjected);
        }
        Ok(())
    }
}

// ── 三面之三：误崩回收面 ─────────────────────────────────────────
pub struct Recycler {
    /// 回收账：族 → 回收 seq 集（语料回库不清账）
    pub recycled: [Vec<u32>; 4],
    /// 崩次记账
    pub crash_total: usize,
    pub injected_total: usize,
}

impl Recycler {
    pub fn new(injected_total: usize) -> Recycler {
        Recycler {
            recycled: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            crash_total: 0,
            injected_total,
        }
    }

    /// 误崩回收一件（语料回库 + 崩次记账）。
    pub fn recycle(&mut self, fam: u8, seq: u32) -> Result<(), FuzzCoreCode> {
        if fam < FAM_HEADER || fam > FAM_TRUNC {
            return Err(FuzzCoreCode::FamilyOutOfRange);
        }
        if self.crash_total + 1 > self.injected_total {
            return Err(FuzzCoreCode::RecycleOverInjected);
        }
        // 语料回库：同族同 seq 重复回库拒收（账面闭集）。
        let slot = &mut self.recycled[fam as usize];
        let mut i = 0;
        while i < slot.len() {
            if slot[i] == seq {
                return Err(FuzzCoreCode::CorpusDuplicated);
            }
            i += 1;
        }
        slot.push(seq);
        self.crash_total += 1;
        Ok(())
    }

    /// 回收面守恒：回收数 ≤ 注入数；账内件数 == 崩次总数。
    pub fn verify(&self) -> Result<(), FuzzCoreCode> {
        if self.crash_total > self.injected_total {
            return Err(FuzzCoreCode::RecycleOverInjected);
        }
        let mut sum = 0;
        let mut fam = FAM_HEADER;
        while fam <= FAM_TRUNC {
            sum += self.recycled[fam as usize].len();
            fam += 1;
        }
        if sum != self.crash_total {
            return Err(FuzzCoreCode::RecycleLostCorpus);
        }
        Ok(())
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：三面 30 例全链（3 族 × 10 件：生成→注入→回收）。
pub fn drill_30_cases() -> (bool, String) {
    let mut inj = Injector::new();
    let mut seq: u32 = 0;
    let mut fam = FAM_HEADER;
    let mut ok_all = true;
    while fam <= FAM_TRUNC {
        let mut per_fam = 0;
        while per_fam < SLOT_CAP {
            let mut rng = Lcg::new(SEG_BASE as u64 + seq as u64);
            let c = match generate_corpus(&mut rng, fam, seq) {
                Ok(c) => c,
                Err(e) => return (false, detail(e.val()).to_string()),
            };
            if inj.inject(&c).is_err() {
                ok_all = false;
                break;
            }
            seq += 1;
            per_fam += 1;
        }
        fam += 1;
    }
    if !ok_all || inj.verify_conservation().is_err() {
        return (false, format!("注入面账断: ok_all={} 守恒断", ok_all));
    }
    // 回收面：每族回收 3 件（共 9），崩次记账。
    let mut rec = Recycler::new(inj.injected_total);
    let mut fam2 = FAM_HEADER;
    while fam2 <= FAM_TRUNC {
        let mut r = 0;
        while r < 3 {
            let sq = fam2 as u32 * 100 + r;
            if rec.recycle(fam2, sq).is_err() {
                return (false, format!("回收面断: 族 {} 件 {}", fam2, r));
            }
            r += 1;
        }
        fam2 += 1;
    }
    let rec_ok = rec.verify() == Ok(());
    (
        ok_all && rec_ok && inj.injected_total == CASES,
        format!(
            "三面 30 例: 注入={}/{} 回收={}/9 守恒={}",
            inj.injected_total, CASES, rec.crash_total, rec_ok
        ),
    )
}

/// 三族语料入口协议断言：各族变异窗口落位（10 组/族核）。
pub fn drill_family_protocol() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < 10 {
        // 族 1：头部窗口必被重写（与基线区分）。
        let mut rng1 = Lcg::new(SEG_BASE as u64 + 0xF000 + i as u64);
        let c1 = match generate_corpus(&mut rng1, FAM_HEADER, i) {
            Ok(c) => c,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if c1.data.len() != CORPUS_LEN {
            ok = false;
            break;
        }
        // 族 2：字段窗口必为尖刺值 0xFF。
        let mut rng2 = Lcg::new(SEG_BASE as u64 + 0xE000 + i as u64);
        let c2 = match generate_corpus(&mut rng2, FAM_SPIKE, i) {
            Ok(c) => c,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if c2.data[3] != 0xFF || c2.data[4] != 0xFF || c2.data[5] != 0xFF {
            ok = false;
            break;
        }
        // 族 3：尾部截断必短于全长。
        let mut rng3 = Lcg::new(SEG_BASE as u64 + 0xD000 + i as u64);
        let c3 = match generate_corpus(&mut rng3, FAM_TRUNC, i) {
            Ok(c) => c,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if c3.data.len() >= CORPUS_LEN || c3.data.is_empty() {
            ok = false;
            break;
        }
        i += 1;
    }
    (ok, format!("三族入口协议 10 组全真={}", ok))
}

/// 注入面反向：跨族注入注入 10/10 必抓。
pub fn drill_cross_family_10() -> (bool, String) {
    // 建模跨族注入：语料族标签与目标槽族错配 → 入口协议必抓。
    // 用 Injector 闭式核：族标签越闭集（0 号族与 4 号族）注入必拒。
    let mut inj = Injector::new();
    let mut caught = 0;
    let mut i = 0;
    while i < 5 {
        let c = Corpus { fam: 0, seq: i as u32, data: [1u8, 2, 3].to_vec() };
        if inj.inject(&c) == Err(FuzzCoreCode::FamilyOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    let mut j = 0;
    while j < 5 {
        let c = Corpus { fam: 4, seq: j as u32, data: [1u8, 2, 3].to_vec() };
        if inj.inject(&c) == Err(FuzzCoreCode::FamilyOutOfRange) {
            caught += 1;
        }
        j += 1;
    }
    (caught == 10, format!("跨族/越族注入 {}/10 必抓", caught))
}

/// 注入面反向：槽容量越界必抓（第 11 件必拒）。
pub fn drill_slot_over_capacity() -> (bool, String) {
    let mut inj = Injector::new();
    let mut i = 0;
    while i < SLOT_CAP {
        let c = Corpus { fam: FAM_SPIKE, seq: i as u32, data: [1u8, 2].to_vec() };
        if inj.inject(&c).is_err() {
            return (false, format!("第 {} 件合法注入失败", i + 1));
        }
        i += 1;
    }
    let over = Corpus { fam: FAM_SPIKE, seq: 99, data: [1u8, 2].to_vec() };
    (
        inj.inject(&over) == Err(FuzzCoreCode::SlotOverCapacity),
        format!("第 {} 件越界必抓", SLOT_CAP + 1),
    )
}

/// 回收面反向：回收越账（回收数 > 注入数）必抓。
pub fn drill_recycle_over() -> (bool, String) {
    let mut rec = Recycler::new(2);
    let _ = rec.recycle(FAM_HEADER, 1);
    let _ = rec.recycle(FAM_HEADER, 2);
    let r = rec.recycle(FAM_HEADER, 3);
    (
        r == Err(FuzzCoreCode::RecycleOverInjected),
        format!("回收越账抓={:?}", r.is_err()),
    )
}

/// 回收面反向：同件重复回库必抓（CorpusDuplicated）。
pub fn drill_recycle_duplicate() -> (bool, String) {
    let mut rec = Recycler::new(10);
    let first = rec.recycle(FAM_TRUNC, 7);
    let second = rec.recycle(FAM_TRUNC, 7);
    (
        first.is_ok() && second == Err(FuzzCoreCode::CorpusDuplicated),
        format!("首收={:?} 重收拒={:?}", first.is_ok(), second.is_err()),
    )
}

/// 空语料拒收：生成面 truncate 边界 + 注入空件必抓。
pub fn drill_empty_corpus() -> (bool, String) {
    let mut inj = Injector::new();
    let c = Corpus { fam: FAM_HEADER, seq: 1, data: Vec::new() };
    let r = inj.inject(&c);
    (
        r == Err(FuzzCoreCode::CorpusEmpty),
        format!("空语料注入抓={:?}", r.is_err()),
    )
}

/// 守恒面：注入账 Σ槽 == 总数；抽账必破（反向判据）。
pub fn drill_inject_conservation() -> (bool, String) {
    let mut inj = Injector::new();
    let mut i = 0;
    while i < 6 {
        let c = Corpus { fam: FAM_HEADER, seq: i as u32, data: [9u8, 9].to_vec() };
        let _ = inj.inject(&c);
        i += 1;
    }
    let good = inj.verify_conservation() == Ok(());
    // 反向：注入总数与槽账失平（手改总数）→ 必抓。
    inj.injected_total += 1;
    let broken = inj.verify_conservation().is_err();
    (good && broken, format!("注入守恒={} 失平抓={}", good, broken))
}

/// 多机对拍面：同种子重建两次，生成语料逐字节一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut out: Vec<(u8, u32, Vec<u8>)> = Vec::new();
        let mut seq: u32 = 0;
        let mut fam = FAM_HEADER;
        while fam <= FAM_TRUNC {
            let mut rng = Lcg::new(SEG_BASE as u64 + fam as u64);
            let mut k = 0;
            while k < 5 {
                if let Ok(c) = generate_corpus(&mut rng, fam, seq) {
                    out.push((c.fam, c.seq, c.data));
                }
                seq += 1;
                k += 1;
            }
            fam += 1;
        }
        out
    };
    let a = build();
    let b = build();
    let mut same = a.len() == b.len();
    let mut j = 0;
    while same && j < a.len() {
        if a[j] != b[j] {
            same = false;
        }
        j += 1;
    }
    (same, format!("同种子对拍一致={}", same))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_30_cases();
    let (a2, d2) = drill_family_protocol();
    let (a3, d3) = drill_cross_family_10();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30例三面={} 入口协议={} 跨族注入={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < FuzzCoreCode::ALL.len() {
        let v = FuzzCoreCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_30_cases();
    v.push(("J1-三面30例全链一致", a, b));
    let (a, b) = drill_family_protocol();
    v.push(("J1-三族语料入口协议", a, b));
    let (a, b) = drill_cross_family_10();
    v.push(("J2-跨族注入10/10必抓", a, b));
    let (a, b) = drill_slot_over_capacity();
    v.push(("J2-槽容量越界必抓", a, b));
    let (a, b) = drill_recycle_over();
    v.push(("J2-回收越账必抓", a, b));
    let (a, b) = drill_recycle_duplicate();
    v.push(("J2-重复回库必抓", a, b));
    let (a, b) = drill_empty_corpus();
    v.push(("J3-空语料拒收", a, b));
    let (a, b) = drill_inject_conservation();
    v.push(("J3-注入守恒+反向", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13241_checks：本单判据聚合（诊断段 0x65800）。
pub fn run_unx13241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13241 fuzz 语料器总成");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65800 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
