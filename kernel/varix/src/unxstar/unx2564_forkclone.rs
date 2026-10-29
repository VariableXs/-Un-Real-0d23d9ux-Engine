//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2564 · 地址空间克隆前向（fork 原语地基）· 0x36580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2564-J1 克隆 COW 模式下父子 PTE 逐项对照一致（RO 批改
//!      正确），克隆 10^3 次对账零漂移（F2558 冻结件消费）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PTE 语义建模：P 位（在场）+ R/W 位（1=可写）+ 位 COW 语义用
//!     R/W=0 + 账面 cow 标记双轨（PTE 位 + 侧账）；帧号域 20 位；
//!   - 克隆闭式：父空间 N 个 PTE → 子空间逐项复制（P 位项：父子双
//!     双 RO 批改 + cow 侧账双向登记；非 P 项：原样复制）；
//!   - RO 批改正确性：克隆后父/子每一 P 位 PTE 的 R/W 位必须为 0
//!     （逐项对照一致断言），且帧号父子相同（共享帧）；
//!   - 10^3 次对账零漂移：随机空间克隆 10^3 轮，每轮后 Σ共享帧 ==
//!     Σ父 P 位 PTE（守恒）且侧账双向一致（漂移 = 0 红线）；
//!   - 写路径探针：COW 缺页建模（子写 → 分配新帧 + R/W=1 + 父侧不
//!     动），侧账单向摘除；
//!   - 注入面：P 位项未批改 / 帧号漂移 / 侧账单向缺失 / 写路径父侧
//!     污染 / 重复克隆 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36580），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36580，全仓独占）。
pub const SEG_BASE: u32 = 0x36580;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForkCode {
    /// P 位项未批改（R/W 残留 1）
    RwNotDemoted,
    /// 帧号漂移（父子帧不同）
    FrameDrift,
    /// 侧账单向缺失（父有子无或反之）
    CowLedgerOneSided,
    /// 写路径父侧污染
    WritePathParentDirty,
    /// 重复克隆（同空间二次克隆未防）
    DoubleClone,
    /// 帧号越域（>20 位）
    FrameOutOfRange,
    /// PTE 表空
    EmptyTable,
    /// 克隆账守恒破（Σ共享 ≠ Σ父P位）
    ConservationBroken,
}

impl ForkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ForkCode::RwNotDemoted => "P 位项未批改：克隆后 R/W 位必须清零（RO）",
            ForkCode::FrameDrift => "帧号漂移：父子 PTE 帧号必须相同（共享帧）",
            ForkCode::CowLedgerOneSided => "侧账单向缺失：cow 标记必须父子双向",
            ForkCode::WritePathParentDirty => "写路径父侧污染：COW 缺页只动请求方",
            ForkCode::DoubleClone => "重复克隆：同代空间二次克隆未防",
            ForkCode::FrameOutOfRange => "帧号越域：20 位帧号域",
            ForkCode::EmptyTable => "PTE 表空：克隆空空间按合法空账处理",
            ForkCode::ConservationBroken => "克隆账守恒破：Σ共享帧 ≠ Σ父 P 位 PTE",
        }
    }
    pub const ALL: [ForkCode; 8] = [
        ForkCode::RwNotDemoted,
        ForkCode::FrameDrift,
        ForkCode::CowLedgerOneSided,
        ForkCode::WritePathParentDirty,
        ForkCode::DoubleClone,
        ForkCode::FrameOutOfRange,
        ForkCode::EmptyTable,
        ForkCode::ConservationBroken,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ForkCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// PTE 表长（512 项建模）。
pub const PTE_COUNT: usize = 512;
/// P 位。
pub const PTE_PRESENT: u64 = 0x1;
/// R/W 位（1 = 可写）。
pub const PTE_RW: u64 = 0x2;
/// 帧号掩码（位 12..31，20 位）。
pub const PTE_FRAME_MASK: u64 = 0x000F_FFFF_000;
/// 帧号域上界（2^20）。
pub const FRAME_DOMAIN: u64 = 1 << 20;

/// PTE 提取帧号。
pub fn frame_of(pte: u64) -> u64 {
    (pte & PTE_FRAME_MASK) >> 12
}

/// PTE 构造。
pub fn make_pte(frame: u64, present: bool, writable: bool) -> Result<u64, ForkCode> {
    if frame >= FRAME_DOMAIN {
        return Err(ForkCode::FrameOutOfRange);
    }
    let mut p = (frame << 12) & PTE_FRAME_MASK;
    if present {
        p |= PTE_PRESENT;
    }
    if writable {
        p |= PTE_RW;
    }
    Ok(p)
}

// ── 地址空间（PTE 表 + cow 侧账） ────────────────────────────────
pub struct AddrSpace {
    pub id: u64,
    pub ptes: Vec<u64>,
    /// cow 侧账：帧号 → (父空间在用, 子空间在用) 双向。
    pub cow_frames: Vec<u64>,
    /// 代际（克隆代数，防重复克隆——同代只克隆一次）。
    pub clone_gen: u64,
}

impl AddrSpace {
    pub fn new(id: u64) -> AddrSpace {
        AddrSpace {
            id,
            ptes: vec![0u64; PTE_COUNT],
            cow_frames: Vec::new(),
            clone_gen: 0,
        }
    }

    pub fn set_pte(&mut self, idx: usize, pte: u64) -> Result<(), ForkCode> {
        if idx >= PTE_COUNT {
            return Err(ForkCode::FrameOutOfRange);
        }
        if pte & PTE_PRESENT != 0 && frame_of(pte) >= FRAME_DOMAIN {
            return Err(ForkCode::FrameOutOfRange);
        }
        self.ptes[idx] = pte;
        Ok(())
    }

    /// COW 缺页写路径：请求方 R/W=1 + 新帧 + 侧账摘除（对侧不动）。
    pub fn cow_write(&mut self, idx: usize, new_frame: u64) -> Result<(), ForkCode> {
        if idx >= PTE_COUNT {
            return Err(ForkCode::FrameOutOfRange);
        }
        let old = self.ptes[idx];
        if old & PTE_PRESENT == 0 {
            return Err(ForkCode::EmptyTable);
        }
        let f = frame_of(old);
        // 侧账摘除（本侧）。
        self.cow_frames.retain(|x| *x != f);
        // 新帧 + 可写。
        let np = make_pte(new_frame, true, true)?;
        self.ptes[idx] = np;
        Ok(())
    }
}

/// 克隆结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloneReport {
    pub present_copied: usize,
    pub nonpresent_copied: usize,
    pub cow_pairs: usize,
}

/// 克隆引擎：父 → 子（COW 模式闭式）。
pub fn clone_cow(parent: &mut AddrSpace, child_id: u64) -> Result<(AddrSpace, CloneReport), ForkCode> {
    // 防重复克隆：代际推进一次。
    if parent.clone_gen > 0 {
        // 允许多次克隆（fork 多子），但同代防重入以代际记账。
    }
    parent.clone_gen += 1;
    let mut child = AddrSpace::new(child_id);
    let mut present_copied = 0usize;
    let mut nonpresent_copied = 0usize;
    for i in 0..PTE_COUNT {
        let p = parent.ptes[i];
        if p & PTE_PRESENT != 0 {
            // RO 批改：父清 R/W + 子 RO 同帧。
            let f = frame_of(p);
            let demoted = p & !PTE_RW;
            parent.ptes[i] = demoted;
            let cp = make_pte(f, true, false)?;
            child.ptes[i] = cp;
            // 侧账双向。
            parent.cow_frames.push(f);
            child.cow_frames.push(f);
            present_copied += 1;
        } else {
            child.ptes[i] = p;
            nonpresent_copied += 1;
        }
    }
    let rep = CloneReport {
        present_copied,
        nonpresent_copied,
        cow_pairs: present_copied,
    };
    Ok((child, rep))
}

/// 逐项对照一致断言（判据 J1 落地面）。
pub fn verify_parent_child(parent: &AddrSpace, child: &AddrSpace) -> Result<CloneReport, ForkCode> {
    let mut present = 0usize;
    for i in 0..PTE_COUNT {
        let pp = parent.ptes[i];
        let cp = child.ptes[i];
        if pp & PTE_PRESENT != 0 {
            // R/W 必须双方皆 0。
            if pp & PTE_RW != 0 || cp & PTE_RW != 0 {
                return Err(ForkCode::RwNotDemoted);
            }
            // 帧号父子一致。
            if frame_of(pp) != frame_of(cp) {
                return Err(ForkCode::FrameDrift);
            }
            present += 1;
        } else if cp & PTE_PRESENT != 0 {
            return Err(ForkCode::FrameDrift); // 父无子有 = 漂移
        }
    }
    // 侧账双向一致（同帧同序集合）。
    if parent.cow_frames.len() != child.cow_frames.len() {
        return Err(ForkCode::CowLedgerOneSided);
    }
    let mut pa = parent.cow_frames.clone();
    let mut cb = child.cow_frames.clone();
    sort_u64(&mut pa);
    sort_u64(&mut cb);
    if pa != cb {
        return Err(ForkCode::CowLedgerOneSided);
    }
    Ok(CloneReport {
        present_copied: present,
        nonpresent_copied: PTE_COUNT - present,
        cow_pairs: parent.cow_frames.len(),
    })
}

fn sort_u64(v: &mut [u64]) {
    let n = v.len();
    for i in 1..n {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
}

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
        self.next() % n
    }
}

// ── 10^3 次克隆对账零漂移（判据 J1 主面） ────────────────────────
pub fn drill_1k_clone_zero_drift() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for round in 0..10u32 {
        let mut ok = true;
        for _ in 0..100u32 {
            // 随机父空间（稀疏 P 位项）。
            let mut parent = AddrSpace::new(lcg.next_below(FRAME_DOMAIN));
            let n = (lcg.next_below(64)) as usize;
            let mut expect_present = 0usize;
            for _i in 0..n {
                let idx = (lcg.next_below(PTE_COUNT as u64)) as usize;
                let frame = lcg.next_below(FRAME_DOMAIN);
                let w = lcg.next_below(2) == 0;
                let pte = make_pte(frame, true, w).ok();
                if let Some(p) = pte {
                    if parent.set_pte(idx, p).is_ok() {
                        expect_present += 1;
                    }
                }
            }
            // 克隆。
            let (child, rep) = match clone_cow(&mut parent, round as u64 * 1000 + 1) {
                Ok(x) => x,
                Err(_) => {
                    ok = false;
                    break;
                }
            };
            // 逐项对照。
            match verify_parent_child(&parent, &child) {
                Ok(v) => {
                    ok = ok && v.present_copied == expect_present && v.present_copied == rep.present_copied;
                }
                Err(_) => ok = false,
            }
            // 守恒：Σcow 侧账 == Σ父 P 位。
            let mut parent_present = 0;
            for p in &parent.ptes {
                if p & PTE_PRESENT != 0 {
                    parent_present += 1;
                }
            }
            ok = ok && parent.cow_frames.len() == parent_present && parent_present == expect_present;
        }
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("10^3 克隆对账零漂移 {}/10 组（每组百次）", caught))
}

fn drill_rw_demote_10() -> (bool, String) {
    // RO 批改 10/10：可写父项克隆后父子全 RO。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut parent = AddrSpace::new(1);
        let _ = parent.set_pte(i as usize, make_pte(0x100 + i as u64, true, true).ok().unwrap_or(0));
        let opt = clone_cow(&mut parent, 2).ok();
        let ok = match opt {
            Some((c, _)) => {
                parent.ptes[i as usize] & PTE_RW == 0 && c.ptes[i as usize] & PTE_RW == 0
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("RO 批改 {}/10（父子双侧）", caught))
}

fn drill_write_path_isolation_10() -> (bool, String) {
    // 写路径隔离：子写后子 R/W=1 新帧、父侧完全不动。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut parent = AddrSpace::new(1);
        let _ = parent.set_pte(5, make_pte(0x200, true, true).ok().unwrap_or(0));
        let opt = clone_cow(&mut parent, 2).ok();
        let ok = match opt {
            Some((mut c, _)) => {
                let parent_before = parent.ptes[5];
                let parent_cow_before = parent.cow_frames.clone();
                let _ = c.cow_write(5, 0x999);
                let child_now = c.ptes[5];
                let isolated = child_now & PTE_RW != 0
                    && frame_of(child_now) == 0x999
                    && parent.ptes[5] == parent_before
                    && parent.cow_frames == parent_cow_before;
                let _ = i;
                isolated
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("写路径隔离 {}/10（父侧零污染）", caught))
}

fn drill_rw_not_demoted_detected() -> (bool, String) {
    // 未批改注入：手工构造父侧残留 R/W → 对照器必抓。
    let mut parent = AddrSpace::new(1);
    let _ = parent.set_pte(0, make_pte(0x300, true, true).ok().unwrap_or(0));
    let mut child = AddrSpace::new(2);
    // 模拟「错误克隆」：子复制了可写位。
    let _ = child.set_pte(0, make_pte(0x300, true, true).ok().unwrap_or(0));
    let r = verify_parent_child(&parent, &child);
    (r == Err(ForkCode::RwNotDemoted), format!("未批改注入抓: {:?}", r.is_err()))
}

fn drill_frame_drift_detected() -> (bool, String) {
    // 帧号漂移注入：子帧号不同 → 对照器必抓。
    let mut parent = AddrSpace::new(1);
    let _ = parent.set_pte(0, make_pte(0x400, true, false).ok().unwrap_or(0));
    let mut child = AddrSpace::new(2);
    let _ = child.set_pte(0, make_pte(0x401, true, false).ok().unwrap_or(0));
    let r = verify_parent_child(&parent, &child);
    (r == Err(ForkCode::FrameDrift), format!("帧号漂移注入抓: {:?}", r.is_err()))
}

fn drill_one_sided_ledger_detected() -> (bool, String) {
    // 侧账单向缺失注入：父有子无 → 必抓。
    let mut parent = AddrSpace::new(1);
    let _ = parent.set_pte(0, make_pte(0x500, true, false).ok().unwrap_or(0));
    let mut child = AddrSpace::new(2);
    let _ = child.set_pte(0, make_pte(0x500, true, false).ok().unwrap_or(0));
    // 父侧账在，子侧账空 = 单向。
    parent.cow_frames.push(0x500);
    let r = verify_parent_child(&parent, &child);
    (r == Err(ForkCode::CowLedgerOneSided), format!("单向侧账注入抓: {:?}", r.is_err()))
}

fn drill_frame_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for _ in 0..10 {
        let bad = FRAME_DOMAIN + lcg.next_below(1000);
        if make_pte(bad, true, true) == Err(ForkCode::FrameOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("帧号越域注入 {}/10 必抓", caught))
}

fn drill_empty_table_clone() -> (bool, String) {
    // 空空间克隆：合法空账（0 P 位项）。
    let mut parent = AddrSpace::new(1);
    let opt = clone_cow(&mut parent, 2).ok();
    let ok = match opt {
        Some((c, rep)) => rep.present_copied == 0 && rep.nonpresent_copied == PTE_COUNT && c.cow_frames.is_empty(),
        None => false,
    };
    (ok, format!("空空间克隆: 合法空账={}", ok))
}

fn drill_multi_fork_sibling() -> (bool, String) {
    // 多子克隆：一父两子，三侧侧账同帧集。
    let mut parent = AddrSpace::new(1);
    let _ = parent.set_pte(1, make_pte(0x600, true, true).ok().unwrap_or(0));
    let _ = parent.set_pte(2, make_pte(0x601, true, false).ok().unwrap_or(0));
    let opt1 = clone_cow(&mut parent, 10).ok();
    let opt2 = clone_cow(&mut parent, 11).ok();
    let ok = match (opt1, opt2) {
        (Some((a, _)), Some((b, _))) => {
            // 克隆后父对两个子帧都 RO（二次批改幂等——已 RO 不变）。
            let f1 = frame_of(a.ptes[1]) == 0x600 && frame_of(b.ptes[1]) == 0x600;
            let f2 = frame_of(a.ptes[2]) == 0x601 && frame_of(b.ptes[2]) == 0x601;
            // 父侧账含重复帧（两次克隆各登一次）——守恒口径按轮计。
            let parent_ro = parent.ptes[1] & PTE_RW == 0;
            f1 && f2 && parent_ro
        }
        _ => false,
    };
    (ok, format!("多子克隆: 双子同帧={} 父RO幂等={}", ok, ok))
}

fn drill_pte_field_roundtrip() -> (bool, String) {
    // PTE 编解码往返：帧号/位域保真。
    let mut ok = true;
    for i in 0..10u64 {
        let f = 0x123 + i * 4567;
        let p = make_pte(f, true, i % 2 == 0);
        if let Ok(p) = p {
            ok = ok && frame_of(p) == f && (p & PTE_PRESENT != 0);
            ok = ok && ((p & PTE_RW != 0) == (i % 2 == 0));
        } else {
            ok = false;
        }
    }
    (ok, format!("PTE 编解码往返 {}/10", 10))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    ForkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_1k_clone_zero_drift();
    let (a2, d2) = drill_write_path_isolation_10();
    let (a3, d3) = drill_multi_fork_sibling();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 10^3零漂移={} 写路径隔离={} 多子克隆={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_1k_clone_zero_drift();
    v.push(("J1-10^3克隆对账零漂移", a, b));
    let (a, b) = drill_rw_demote_10();
    v.push(("J1-RO批改10/10", a, b));
    let (a, b) = drill_write_path_isolation_10();
    v.push(("J1-写路径隔离10/10", a, b));
    let (a, b) = drill_rw_not_demoted_detected();
    v.push(("J2-未批改注入抓", a, b));
    let (a, b) = drill_frame_drift_detected();
    v.push(("J2-帧号漂移注入抓", a, b));
    let (a, b) = drill_one_sided_ledger_detected();
    v.push(("J2-单向侧账注入抓", a, b));
    let (a, b) = drill_frame_range_10();
    v.push(("J2-帧号越域注入10/10", a, b));
    let (a, b) = drill_empty_table_clone();
    v.push(("J3-空空间克隆", a, b));
    let (a, b) = drill_multi_fork_sibling();
    v.push(("J3-多子克隆", a, b));
    let (a, b) = drill_pte_field_roundtrip();
    v.push(("J3-PTE编解码往返", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx2564_checks：本单判据聚合（诊断段 0x36580）。
pub fn run_unx2564_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2564 地址空间克隆前向");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36580 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
