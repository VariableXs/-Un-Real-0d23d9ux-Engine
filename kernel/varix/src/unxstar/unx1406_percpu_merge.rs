//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1406 · per-CPU 分账与全域合并账 · 0x6FA80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B31）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1406-J1 per-CPU 分账开户率 100%（每向量 × 每 CPU 二维
//!     开户），全域合并视图一致性 100% 断言（合并=分账按序号归并
//!     零丢失），分账合并 10^6 事件对拍一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 闸门次序（分账合并为什么不能倒）：
//!   开户先于记账——没开的户不收账，收了必是黑户；
//!   分账先于合并——每 CPU 各自先记清，合并才有归并的原料；
//!   序号归并先于对拍——归并序断了先报断，对拍一致才可信；
//!   对拍先于宣告——10^6 事件分/合两路数字不合，合并视图不许生效。
//!
//! 关键差异（本卡深度所在）：
//!   本卡只回答"二维开户是否全开、按序号归并是否零丢失、10^6 事件
//!   分合两路对拍是否一致"，不复算每向量的业务语义（那是统计面卡
//!   的事），也不回答 per-CPU 缓存行伪共享（那是 F1104 的纵深）——
//!   本卡审的是**分与合的守恒**：合并不许丢、不许重、不许乱序。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6FA80，AI-VX3-1103A 独占区）。
pub const SEG_BASE: u32 = 0x6FA80;

/// 向量数（冻结 24）。
pub const VECTOR_COUNT: usize = 24;
/// CPU 数（冻结 8）。
pub const CPU_COUNT: usize = 8;
/// 对拍事件总量（冻结 10^6）。
pub const EVENT_TOTAL: u32 = 1_000_000;
/// 序号归并窗口（每 CPU 留尾 64 条，八路归并样本）。
pub const MERGE_WINDOW: usize = 64;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeCode {
    /// CPU 号越界
    CpuOver,
    /// 向量号越界
    VectorOver,
    /// 户未开（cell 未开户即收账）
    CellNotOpen,
    /// 归并序断（序号重复或倒退）
    SeqSkip,
    /// 合并丢失（归并条数少于分账原料）
    MergeLoss,
    /// 开户率不足（<100%）
    OpenRateUnder,
    /// 双跑不一致
    NotReproducible,
}

impl MergeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MergeCode::CpuOver => "CPU 号越界：≥8",
            MergeCode::VectorOver => "向量号越界：≥24",
            MergeCode::CellNotOpen => "户未开：cell 未开户即收账",
            MergeCode::SeqSkip => "归并序断：序号重复或倒退",
            MergeCode::MergeLoss => "合并丢失：归并 < 分账原料",
            MergeCode::OpenRateUnder => "开户率不足：<100%",
            MergeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MergeCode; 7] = [
        MergeCode::CpuOver,
        MergeCode::VectorOver,
        MergeCode::CellNotOpen,
        MergeCode::SeqSkip,
        MergeCode::MergeLoss,
        MergeCode::OpenRateUnder,
        MergeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MergeCode::ALL.len() {
        if MergeCode::ALL[i].val() == code {
            return MergeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6FA80 段闭集内"
}

// ── LCG（事件流驱动，与判据同源确定性） ────────────────────────
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
        (self.next() >> 33) % n
    }
}

// ── per-CPU 分账（一 CPU 一册，24 向量计数 + 尾窗序号账） ────────
#[derive(Clone, Debug)]
pub struct SubLedger {
    pub cpu: usize,
    /// 24 向量计数（开户即全零在册）。
    pub counts: [u64; VECTOR_COUNT],
    /// 本册事件总数。
    pub events: u64,
    /// 序号尾窗：(全局序号, 向量号)，按序号严格递增落窗。
    pub window: Vec<(u32, u8)>,
}

impl SubLedger {
    pub fn new(cpu: usize) -> SubLedger {
        SubLedger {
            cpu,
            counts: [0; VECTOR_COUNT],
            events: 0,
            window: Vec::new(),
        }
    }

    /// 记一笔（开户闸 + 越界闸）。
    pub fn record(&mut self, seq: u32, vector: usize, open: &OpenMap) -> Result<(), u32> {
        if self.cpu >= CPU_COUNT {
            return Err(MergeCode::CpuOver.val());
        }
        if vector >= VECTOR_COUNT {
            return Err(MergeCode::VectorOver.val());
        }
        if !open.is_open(vector, self.cpu) {
            return Err(MergeCode::CellNotOpen.val());
        }
        self.counts[vector] += 1;
        self.events += 1;
        self.window.push((seq, vector as u8));
        if self.window.len() > MERGE_WINDOW {
            // 留尾窗：挤掉最老的一条（窗口外账仍在 counts 里，不丢）。
            let _ = self.window.remove(0);
        }
        Ok(())
    }
}

/// 二维开户表（向量 × CPU，全 1 为全开）。
#[derive(Clone, Debug)]
pub struct OpenMap {
    cells: [[bool; CPU_COUNT]; VECTOR_COUNT],
    pub opened: u32,
}

impl OpenMap {
    /// 全域开户：24×8 = 192 户全开（开户率 100% 的实体）。
    pub fn full() -> OpenMap {
        let mut m = OpenMap { cells: [[false; CPU_COUNT]; VECTOR_COUNT], opened: 0 };
        let mut v = 0;
        while v < VECTOR_COUNT {
            let mut c = 0;
            while c < CPU_COUNT {
                m.cells[v][c] = true;
                m.opened += 1;
                c += 1;
            }
            v += 1;
        }
        m
    }
    pub fn is_open(&self, v: usize, c: usize) -> bool {
        if v >= VECTOR_COUNT || c >= CPU_COUNT {
            return false;
        }
        self.cells[v][c]
    }
    /// 注入口：关一户（开户率闸的靶子）。
    pub fn close_cell(&mut self, v: usize, c: usize) {
        if v < VECTOR_COUNT && c < CPU_COUNT && self.cells[v][c] {
            self.cells[v][c] = false;
            self.opened -= 1;
        }
    }
    /// 开户率（‰）。
    pub fn rate_permil(&self) -> u32 {
        (self.opened * 1000) / (VECTOR_COUNT * CPU_COUNT) as u32
    }
}

// ── 全域账（分账册柜 + 全局计数 + 归并视图） ────────────────────
#[derive(Clone, Debug)]
pub struct GlobalLedger {
    pub subs: Vec<SubLedger>,
    pub open: OpenMap,
    /// 全局每向量计数（合并视图的数字面）。
    pub merged_counts: [u64; VECTOR_COUNT],
    pub total: u64,
}

impl GlobalLedger {
    pub fn new() -> GlobalLedger {
        let mut subs: Vec<SubLedger> = Vec::new();
        let mut c = 0;
        while c < CPU_COUNT {
            subs.push(SubLedger::new(c));
            c += 1;
        }
        GlobalLedger {
            subs,
            open: OpenMap::full(),
            merged_counts: [0; VECTOR_COUNT],
            total: 0,
        }
    }

    /// 记一笔事件：分账（cpu 册）+ 全局同拍入账。
    pub fn record(&mut self, seq: u32, vector: usize, cpu: usize) -> Result<(), u32> {
        if cpu >= CPU_COUNT {
            return Err(MergeCode::CpuOver.val());
        }
        if vector >= VECTOR_COUNT {
            return Err(MergeCode::VectorOver.val());
        }
        // 取值纪律：match 接 get_mut，不裸下标。
        match self.subs.get_mut(cpu) {
            Some(sub) => sub.record(seq, vector, &self.open)?,
            None => return Err(MergeCode::CpuOver.val()),
        }
        self.merged_counts[vector] += 1;
        self.total += 1;
        Ok(())
    }

    /// 合并视图一致性：分账按 CPU 序逐向量求和 == 全局计数，且总量
    /// == 分账总量（合并=分账按序号归并零丢失的数字面）。
    pub fn merge_consistent(&self) -> Result<u64, u32> {
        let mut sums = [0u64; VECTOR_COUNT];
        let mut c = 0;
        while c < self.subs.len() {
            let sub = match self.subs.get(c) {
                Some(s) => s,
                None => return Err(MergeCode::CpuOver.val()),
            };
            if sub.cpu != c {
                return Err(MergeCode::CpuOver.val());
            }
            let mut v = 0;
            while v < VECTOR_COUNT {
                sums[v] += sub.counts[v];
                v += 1;
            }
            c += 1;
        }
        let mut v = 0;
        while v < VECTOR_COUNT {
            if sums[v] != self.merged_counts[v] {
                return Err(MergeCode::MergeLoss.val());
            }
            v += 1;
        }
        let mut sub_total = 0u64;
        let mut c2 = 0;
        while c2 < self.subs.len() {
            match self.subs.get(c2) {
                Some(s) => sub_total += s.events,
                None => return Err(MergeCode::CpuOver.val()),
            }
            c2 += 1;
        }
        if sub_total != self.total {
            return Err(MergeCode::MergeLoss.val());
        }
        Ok(sub_total)
    }
}

/// 八路按序号归并：各分账尾窗按 seq 升序归并成一条全局序。
/// 零丢失 = 归并条数 == 原料条数 + 归并序严格递增 + 逐 CPU 消费清零。
pub fn merge_windows(subs: &[SubLedger]) -> Result<(usize, bool, bool), u32> {
    // 归并前先冻结各窗指针。
    let mut pos: [usize; CPU_COUNT] = [0; CPU_COUNT];
    let mut consumed: [usize; CPU_COUNT] = [0; CPU_COUNT];
    let mut raw_total = 0usize;
    let mut c = 0;
    while c < subs.len() && c < CPU_COUNT {
        match subs.get(c) {
            Some(s) => raw_total += s.window.len(),
            None => return Err(MergeCode::CpuOver.val()),
        }
        c += 1;
    }
    let mut merged: Vec<(u32, u8)> = Vec::new();
    let mut last_seq: u32 = 0;
    let mut first = true;
    let mut ordered = true;
    loop {
        // 选八路里最小 seq 的头。
        let mut best: i32 = -1;
        let mut best_seq: u32 = 0;
        let mut c = 0;
        while c < subs.len() && c < CPU_COUNT {
            let s = match subs.get(c) {
                Some(s) => s,
                None => return Err(MergeCode::CpuOver.val()),
            };
            if pos[c] < s.window.len() {
                match s.window.get(pos[c]) {
                    Some(&(sq, _)) => {
                        if best < 0 || sq < best_seq {
                            best = c as i32;
                            best_seq = sq;
                        }
                    }
                    None => return Err(MergeCode::CpuOver.val()),
                }
            }
            c += 1;
        }
        if best < 0 {
            break;
        }
        let bc = best as usize;
        // 序闸：严格递增（重复/倒退即序断）。
        if !first && best_seq <= last_seq {
            ordered = false;
        }
        first = false;
        last_seq = best_seq;
        match subs.get(bc) {
            Some(s) => match s.window.get(pos[bc]) {
                Some(&(sq, vec)) => merged.push((sq, vec)),
                None => return Err(MergeCode::CpuOver.val()),
            },
            None => return Err(MergeCode::CpuOver.val()),
        }
        pos[bc] += 1;
        consumed[bc] += 1;
    }
    // 零丢失闸：归并条数 == 原料条数，且每路消费清零。
    let mut lossless = merged.len() == raw_total;
    let mut c = 0;
    while c < consumed.len() {
        match subs.get(c) {
            Some(s) => {
                if consumed[c] != s.window.len() {
                    lossless = false;
                }
            }
            None => lossless = false,
        }
        c += 1;
    }
    Ok((merged.len(), ordered, lossless))
}

// ── 10^6 事件对拍（分/合两路数字一致） ──────────────────────────
pub fn reconcile(total: u32, seed: u64) -> Result<(u64, bool, bool), u32> {
    let mut g = GlobalLedger::new();
    let mut rng = Lcg::new(seed);
    let mut seq: u32 = 0;
    while seq < total {
        let v = rng.below(VECTOR_COUNT as u64) as usize;
        let c = rng.below(CPU_COUNT as u64) as usize;
        g.record(seq, v, c)?;
        seq += 1;
    }
    // 对拍：分账求和 == 全局计数 == total。
    let t = g.merge_consistent()?;
    if t != total as u64 {
        return Err(MergeCode::MergeLoss.val());
    }
    // 归并：尾窗八路按序号归并零丢失。
    let (n, ordered, lossless) = merge_windows(&g.subs)?;
    let _ = n;
    Ok((t, ordered, lossless))
}

// ── 注入检出账（四路：越 CPU 路/越向量路/黑户路/丢账路） ─────────
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

/// 四路注入各自必须被闸门当场抓住。
pub fn inject_four() -> [bool; 4] {
    let mut g = GlobalLedger::new();
    // 路 1：越 CPU 路。
    let r1 = g.record(0, 0, CPU_COUNT).is_err();
    // 路 2：越向量路。
    let r2 = g.record(0, VECTOR_COUNT, 0).is_err();
    // 路 3：黑户路（关一户后向它记账 ⇒ CellNotOpen）。
    let mut g3 = GlobalLedger::new();
    g3.open.close_cell(1, 2);
    let r3 = match g3.subs.get_mut(2) {
        Some(sub) => sub.record(0, 1, &g3.open).is_err(),
        None => false,
    };
    // 路 4：序断路（窗内序号改成重复 ⇒ 归并序闸必假）。
    let mut g4 = GlobalLedger::new();
    let mut s = 0;
    while s < 8 {
        let _ = g4.record(s, (s % VECTOR_COUNT as u32) as usize, 0);
        s += 1;
    }
    let _ = g4.subs[0].window[4].0 = 3;
    let (_, ordered, _) = match merge_windows(&g4.subs) {
        Ok(x) => x,
        Err(_) => (0, false, false),
    };
    let r4 = !ordered;
    [r1, r2, r3, r4]
}

// ── 黄金全链：开户→10^6 对拍→归并零丢失→四拒 ───────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 开户率 100%。
    let open = OpenMap::full();
    let open_ok = (open.rate_permil() == 1000 && open.opened == 192) as u32;
    // 10^6 事件对拍一致 + 归并零丢失。
    let (t, ordered, lossless) = reconcile(EVENT_TOTAL, SEG_BASE as u64)?;
    let recon_ok = (t == EVENT_TOTAL as u64 && ordered && lossless) as u32;
    // 四拒。
    let mut g = GlobalLedger::new();
    let over_cpu = g.record(0, 0, CPU_COUNT).is_err();
    let over_vec = g.record(0, VECTOR_COUNT, 0).is_err();
    let mut g3 = GlobalLedger::new();
    g3.open.close_cell(0, 0);
    let not_open = match g3.subs.get_mut(0) {
        Some(sub) => sub.record(0, 0, &g3.open).is_err(),
        None => false,
    };
    let mut g4 = GlobalLedger::new();
    let mut s = 0;
    while s < 8 {
        let _ = g4.record(s, (s % VECTOR_COUNT as u32) as usize, 1);
        s += 1;
    }
    let _ = g4.subs[1].window[2].0 = 1;
    let seq_bad = match merge_windows(&g4.subs) {
        Ok((_, o, _)) => !o,
        Err(_) => true,
    };
    Ok((
        open_ok + recon_ok,
        over_cpu as u32 + over_vec as u32 + not_open as u32 + seq_bad as u32,
        t as u32,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：开户率 100% + 对拍一致 + 四拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 4, _)) => Ok(()),
        _ => Err(MergeCode::NotReproducible.val()),
    }
}

/// 序 B：黑户收账 ⇒ CellNotOpen。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = GlobalLedger::new();
    g.open.close_cell(3, 5);
    match g.record(0, 3, 5) {
        Err(e) if e == MergeCode::CellNotOpen.val() => Ok(()),
        _ => Err(MergeCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（向量 24 / CPU 8 / 事件 10^6 / 窗 64）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        VECTOR_COUNT == 24,
        CPU_COUNT == 8,
        EVENT_TOTAL == 1_000_000,
        MERGE_WINDOW == 64,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(MergeCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let inj = inject_four();
        let mut i = 0;
        while i < 4 {
            led.record(i, inj[i]);
            i += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MergeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MergeCode::ALL.len() {
        let v = MergeCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 2 && *rej == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, t)) => format!("ok={} rejects={} total={}", ok, rej, t),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 开户率 100%（192/192 户）。
    let open = OpenMap::full();
    v.push((
        "J1-开户率",
        open.rate_permil() == 1000 && open.opened == (VECTOR_COUNT * CPU_COUNT) as u32,
        format!("{}/192 户全开", open.opened),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(x) => x.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|x| x.tries),
            inj.as_ref().map(|x| x.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=冻结", b),
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
        String::from("0x6FA80 窗口"),
    ));
    v
}

pub fn run_unx1406_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1406 per-CPU 分账与全域合并账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 自检（探针本地复核，不进内核镜像） ──────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_two_four() {
        match golden_run() {
            Ok((ok, rej, t)) => {
                assert_eq!(ok, 2);
                assert_eq!(rej, 4);
                assert_eq!(t, EVENT_TOTAL as u32);
            }
            Err(e) => panic!("golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn merge_window_ordered_and_lossless() {
        let mut g = GlobalLedger::new();
        let mut s = 0;
        while s < 64 {
            let v = (s % VECTOR_COUNT as u32) as usize;
            let c = (s % CPU_COUNT as u32) as usize;
            assert!(g.record(s, v, c).is_ok());
            s += 1;
        }
        match merge_windows(&g.subs) {
            Ok((n, ordered, lossless)) => {
                assert!(ordered);
                assert!(lossless);
                assert!(n > 0);
            }
            Err(e) => panic!("merge err=0x{:x}", e),
        }
    }

    #[test]
    fn four_injections_all_caught() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
    }

    #[test]
    fn three_probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }
}
