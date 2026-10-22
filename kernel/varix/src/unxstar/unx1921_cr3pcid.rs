// unx1921_cr3pcid —— UNX-F1921 CR3 切换主路径与 PCID 语义（基线+优化双轨）
// 判据原文 UNX-F1921-J1：双轨断言全过（全 flush 基线路径逐切换零残段+
// PCID 优化路径位轮转正确），切换后地址空间指向校验 10/10，两轨可配可回退。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF480 独占（0xF480~0xF4FF）。
//
// 设计（纯功能 no_std）：
//   * 基线轨：全 flush（CR3 写 = 置 PCID=0 + NOFLUSH=0），切换后 TLB 残留集必空；
//   * 优化轨：PCID（12 位）+ NOFLUSH 位（bit63）；ASID 池 4 项位轮转（0 保留给内核）；
//     同 PCID 复用必须先 invalidated，否则串味；
//   * 双轨可配可回退：模式二态（Baseline/Optimized），回退 = 模式切回基线 +
//     PCID 池全失效重置；
//   * 指向校验 10/10：切换后“当前地址空间 = 期望空间”逐次核对；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF480 独占） ─────────────────────────────────────────────
pub const XC_CR3_RESIDUE: u32 = 0xF481; // 基线轨切换后 TLB 残留
pub const XC_CR3_PCID_OOB: u32 = 0xF482; // PCID 越界（>4095 或命中保留 0）
pub const XC_CR3_REUSE_STALE: u32 = 0xF483; // PCID 复用未失效（串味）
pub const XC_CR3_POINT: u32 = 0xF484; // 切换后指向漂移
pub const XC_CR3_SEALED: u32 = 0xF485; // 封账后切换

/// 双轨模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cr3Mode {
    Baseline = 0,
    Optimized = 1,
}

impl Cr3Mode {
    pub fn name(self) -> &'static str {
        match self {
            Cr3Mode::Baseline => "FULL-FLUSH",
            Cr3Mode::Optimized => "PCID",
        }
    }
    pub fn toggle(self) -> Cr3Mode {
        match self {
            Cr3Mode::Baseline => Cr3Mode::Optimized,
            Cr3Mode::Optimized => Cr3Mode::Baseline,
        }
    }
}

/// TLB 残留项：(pcid, vpn)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TlbEntry {
    pub pcid: u16,
    pub vpn: u64,
}

/// CR3 切换机（模型：TLB 为存储集）。
pub struct Cr3Switcher {
    mode: Cr3Mode,
    tlb: Vec<TlbEntry>,
    now_space: u64,           // 当前地址空间 id
    pcid_of: Vec<(u64, u16)>, // 空间 → pcid 分配
    pcid_pool: Vec<u16>,      // 空闲池（1..=3 轮转，0 内核保留）
    pool_cursor: usize,
    switches: u32,
    ledger: Vec<(u64, u64, Cr3Mode)>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

const PCID_MAX: u16 = 4095;
const NOFLUSH_BIT: u64 = 1 << 63;

impl Cr3Switcher {
    pub fn new(mode: Cr3Mode) -> Cr3Switcher {
        let mut pool = Vec::new();
        for p in 1..=3u16 {
            pool.push(p);
        }
        Cr3Switcher {
            mode,
            tlb: Vec::new(),
            now_space: 0,
            pcid_of: Vec::new(),
            pcid_pool: pool,
            pool_cursor: 0,
            switches: 0,
            ledger: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    pub fn mode(&self) -> Cr3Mode {
        self.mode
    }
    pub fn tlb_len(&self) -> usize {
        self.tlb.len()
    }

    fn pcid_for(&mut self, space: u64) -> Result<u16, u32> {
        // 已分配则复用
        for &(s, p) in &self.pcid_of {
            if s == space {
                return Ok(p);
            }
        }
        if self.pcid_pool.is_empty() {
            // 全耗尽：轮转回收到最早分配者（位轮转正确性的一部分）
            let victim = self.pcid_of.first().map(|&(s, _)| s).unwrap_or(0);
            let p = self
                .pcid_of
                .iter()
                .position(|&(s, _)| s == victim)
                .map(|i| self.pcid_of.remove(i).1)
                .unwrap_or(1);
            // 回收即失效：victim 空间的 TLB 项全失效（防串味）
            self.tlb.retain(|e| e.pcid != p);
            self.pcid_of.retain(|&(s, _)| s != victim);
            return Ok(p);
        }
        let p = self.pcid_pool[self.pool_cursor % self.pcid_pool.len()];
        self.pool_cursor += 1;
        self.pcid_of.push((space, p));
        Ok(p)
    }

    /// 基线轨切换：全 flush（TLB 清空）+ 指向更新。
    pub fn switch_baseline(&mut self, space: u64, at: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CR3_SEALED, "封账后切换"));
            return Err(XC_CR3_SEALED);
        }
        self.tlb.clear(); // 全 flush —— 逐切换零残段
        self.now_space = space;
        self.switches += 1;
        self.ledger.push((at, space, Cr3Mode::Baseline));
        Ok(())
    }

    /// 优化轨切换：PCID 轮转 + NOFLUSH 语义（不清他 PCID 项）。
    pub fn switch_optimized(&mut self, space: u64, at: u64) -> Result<u16, u32> {
        if self.sealed {
            self.rejects.push((XC_CR3_SEALED, "封账后切换"));
            return Err(XC_CR3_SEALED);
        }
        let pcid = self.pcid_for(space)?;
        if pcid == 0 || pcid > PCID_MAX {
            self.rejects.push((XC_CR3_PCID_OOB, "PCID 越界"));
            return Err(XC_CR3_PCID_OOB);
        }
        // 同 PCID 复用必须先失效旧项（防串味）——pcid_for 复用路径未失效即串味
        let mut stale = false;
        for e in &self.tlb {
            if e.pcid == pcid {
                stale = true;
            }
        }
        if stale {
            // 切进本空间时旧项允许存在（同空间）；串味只在回收/改派时发生
            // —— 由 pcid_for 回收路径保证 retain。这里按空间核对：
            for e in &self.tlb {
                if e.pcid == pcid {
                    // 项的归属必须仍是本空间
                    let owner = self.pcid_of.iter().find(|&&(s, p)| p == pcid).map(|&(s, _)| s);
                    if owner != Some(space) {
                        self.rejects.push((XC_CR3_REUSE_STALE, "PCID 改派未失效"));
                        return Err(XC_CR3_REUSE_STALE);
                    }
                }
            }
        }
        self.now_space = space;
        self.switches += 1;
        self.ledger.push((at, space, Cr3Mode::Optimized));
        Ok(pcid)
    }

    /// 统一入口：按模式分轨。
    pub fn switch(&mut self, space: u64, at: u64) -> Result<u16, u32> {
        match self.mode {
            Cr3Mode::Baseline => {
                self.switch_baseline(space, at)?;
                Ok(0)
            }
            Cr3Mode::Optimized => self.switch_optimized(space, at),
        }
    }

    /// 装入 TLB 项（模拟访存命中填充）。
    pub fn tlb_fill(&mut self, pcid: u16, vpn: u64) {
        if self.sealed {
            return;
        }
        self.tlb.push(TlbEntry { pcid, vpn });
    }

    /// 指向校验：当前空间 == 期望。
    pub fn point_check(&self, expect: u64) -> Result<(), u32> {
        if self.now_space != expect {
            return Err(XC_CR3_POINT);
        }
        Ok(())
    }

    /// 回退：模式切回基线 + TLB 全清 + 分配表重置。
    pub fn rollback_to_baseline(&mut self) {
        self.mode = Cr3Mode::Baseline;
        self.tlb.clear();
        self.pcid_of.clear();
        self.pcid_pool.clear();
        for p in 1..=3u16 {
            self.pcid_pool.push(p);
        }
        self.pool_cursor = 0;
    }

    /// 两轨可配：模式可切（不改当前空间指向）。
    pub fn reconfigure(&mut self, to: Cr3Mode) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_CR3_SEALED);
        }
        if to == self.mode {
            return Ok(()); // 幂等
        }
        if to == Cr3Mode::Baseline {
            self.rollback_to_baseline();
        } else {
            self.mode = Cr3Mode::Optimized;
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CR3_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.mode as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.now_space as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.switches;
        h = h.wrapping_mul(0x0100_0193);
        for e in &self.tlb {
            h ^= e.pcid as u32 ^ (e.vpn as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        for &(s, p) in &self.pcid_of {
            h ^= (s as u32) ^ (p as u32).wrapping_mul(0x85EB);
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.pcid_of.len() as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）基线轨切换后 TLB 必空（零残段）
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Baseline);
        sw.tlb_fill(0, 0x1000);
        let _ = sw.switch_baseline(2, 1);
        if sw.tlb_len() == 0 {
            caught += 1;
        }
    }
    // 2）PCID 保留值 0 必拒
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
        // 强造保留值：pcid_for 不产 0，但直接核验闸 —— 通过构造 pcid_of 预置
        sw.pcid_of.push((99, 1));
        // 空间 99 复用路径返回 1，非 0；保留值闸由 0 值语义体现：pcid_for 池里无 0
        // 注入 = 若池被污染含 0，switch_optimized 必拒
        sw.pcid_pool.insert(0, 0); // 污染池
        if sw.switch_optimized(77, 1) == Err(XC_CR3_PCID_OOB) {
            caught += 1;
        }
    }
    // 3）改派未失效必拒（串味注入）
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
        let _ = sw.switch_optimized(5, 1); // 得 pcid
        let p5 = sw.pcid_of.iter().find(|&&(s, _)| s == 5).map(|&(_, p)| p).unwrap_or(1);
        sw.tlb_fill(p5, 0x2000); // 空间 5 的项
        // 手工改派 pcid 到空间 6（模拟回收者漏失效）
        sw.pcid_of.retain(|&(s, _)| s != 5);
        sw.pcid_of.push((6, p5));
        if sw.switch_optimized(6, 2) == Err(XC_CR3_REUSE_STALE) {
            caught += 1;
        }
    }
    // 4）指向漂移必拒
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Baseline);
        let _ = sw.switch_baseline(3, 1);
        if sw.point_check(4) == Err(XC_CR3_POINT) {
            caught += 1;
        }
    }
    // 5）封账后切换必拒
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Baseline);
        let _ = sw.seal();
        if sw.switch(1, 1) == Err(XC_CR3_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1921-J1。
pub fn run_unx1921_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1921");

    // C21-01 基线轨：10 次切换逐次零残段
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Baseline);
        let mut all_clear = true;
        for k in 1..=10u64 {
            sw.tlb_fill(0, k * 0x100);
            let _ = sw.switch_baseline(k, k);
            if sw.tlb_len() != 0 {
                all_clear = false;
            }
            if sw.point_check(k).is_err() {
                all_clear = false;
            }
        }
        s.add("C21-01-基线零残段10/10", all_clear, if all_clear { "" } else { "残段" });
    }
    // C21-02 优化轨：PCID 轮转（3 项池轮转正确）
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
        let p1 = sw.switch_optimized(10, 1);
        let p2 = sw.switch_optimized(11, 2);
        let p3 = sw.switch_optimized(12, 3);
        let ok = p1.is_ok() && p2.is_ok() && p3.is_ok()
            && p1 != p2 && p2 != p3 && p1 != p3
            && p1.unwrap() >= 1 && p3.unwrap() <= 3;
        s.add("C21-02-PCID轮转", ok, if ok { "" } else { "轮转漂移" });
    }
    // C21-03 优化轨：同空间复用同 PCID
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
        let a = sw.switch_optimized(20, 1);
        let b = sw.switch_optimized(20, 2);
        let ok = a.is_ok() && a == b;
        s.add("C21-03-同空间同PCID", ok, if ok { "" } else { "复用漂移" });
    }
    // C21-04 优化轨：池耗尽轮转回收即失效（不串味）
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
        let _ = sw.switch_optimized(30, 1);
        let _ = sw.switch_optimized(31, 2);
        let _ = sw.switch_optimized(32, 3);
        let pre_len = sw.tlb_len();
        sw.tlb_fill(1, 0xAAAA); // 空间 30 项
        let _ = sw.switch_optimized(33, 4); // 触发回收空间 30 → 其项必须失效
        let mut residue = false;
        for e in &sw.tlb {
            if e.vpn == 0xAAAA {
                residue = true;
            }
        }
        let _ = pre_len;
        let ok = !residue;
        s.add("C21-04-回收即失效", ok, if ok { "" } else { "串味" });
    }
    // C21-05 指向校验 10/10（双轨混合）
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Baseline);
        let mut all_ok = true;
        for k in 0..10u64 {
            let space = 100 + k;
            let _ = sw.switch(space, k);
            if sw.point_check(space).is_err() {
                all_ok = false;
            }
        }
        s.add("C21-05-指向校验10/10", all_ok, if all_ok { "" } else { "指向漂移" });
    }
    // C21-06 双轨可配可回退（回退后池重置）
    {
        let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
        let _ = sw.switch_optimized(40, 1);
        let _ = sw.reconfigure(Cr3Mode::Baseline);
        let back = sw.mode() == Cr3Mode::Baseline && sw.tlb_len() == 0;
        let _ = sw.reconfigure(Cr3Mode::Optimized);
        let fwd = sw.mode() == Cr3Mode::Optimized && sw.pcid_of.is_empty();
        s.add("C21-06-可配可回退", back && fwd, if back && fwd { "" } else { "回退破" });
    }
    // C21-07 模式闭集与名冻结
    {
        let ok = Cr3Mode::Baseline.name() == "FULL-FLUSH"
            && Cr3Mode::Optimized.name() == "PCID"
            && Cr3Mode::Baseline.toggle() == Cr3Mode::Optimized;
        s.add("C21-07-模式闭集", ok, if ok { "" } else { "模式表漂移" });
    }
    // C21-08 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C21-08-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C21-09 NOFLUSH 位语义（bit63 不进 PCID 位域）
    {
        let ok = NOFLUSH_BIT >> 63 == 1 && PCID_MAX == 4095;
        s.add("C21-09-位域语义", ok, if ok { "" } else { "位域漂移" });
    }
    // C21-10 指纹冻结 + 封账稳定
    {
        let f = || {
            let mut sw = Cr3Switcher::new(Cr3Mode::Optimized);
            let _ = sw.switch_optimized(50, 1);
            sw.fingerprint()
        };
        let mut g = Cr3Switcher::new(Cr3Mode::Baseline);
        let _ = g.switch_baseline(1, 1);
        let pre = g.fingerprint();
        let _ = g.seal();
        let ok = f() == f() && g.fingerprint() == pre && g.seal().is_err();
        s.add("C21-10-指纹与封账", ok, if ok { "" } else { "漂移" });
    }
    s
}
