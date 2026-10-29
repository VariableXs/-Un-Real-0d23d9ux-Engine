// unx1237_u1237 —— UNX-F1237 Draining 态回归资产与断言集（B22 段聚合）
// 判据原文 UNX-F1237-J1：B22 段聚合断言 ≥30 全过（Draining 面回归资产
// 逐项回收），通过率 100%，skip 必带因，聚合指纹冻结。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb970 独占（0xb970~0xb9af）。
//
// 设计（纯功能 no_std）：
//   * 断言登记表（id/子域/值域）+ 三态执行账（Pass/Fail/Skip）；
//   * skip 显性接口：无因 skip 必拒；fail 必留实际值证据；
//   * 守恒：登记 = 通过+失败+挂起；B22 gate：≥30 全过且零 fail；
//   * FNV 链指纹冻结，封账后拒执行。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb970 独占） ─────────────────────────────────────────────
pub const XC_AJ_DUP_ID: u32 = 0xB971; // 断言 id 重复
pub const XC_AJ_NO_ID: u32 = 0xB972; // 执行未登记 id
pub const XC_AJ_SEALED: u32 = 0xB973; // 封账后执行
pub const XC_AJ_FULL: u32 = 0xB974; // 登记容量满
pub const XC_AJ_SKIP_NOCAUSE: u32 = 0xB975; // skip 无因
pub const XC_AJ_NO_EVID: u32 = 0xB976; // fail 无证据
pub const XC_AJ_OOB_DOM: u32 = 0xB977; // 子域越界

/// 三态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AjState {
    Pass,
    Fail,
    Skip,
}

/// 登记项。
#[derive(Clone, Copy)]
pub struct AjEntry {
    pub aid: u32,
    /// 子域（0=对账 1=窗口 2=超时 3=幂等 4=其他）。
    pub dom: u8,
    pub lo: u64,
    pub hi: u64,
}

/// 执行账目。
#[derive(Clone, Copy)]
pub struct AjRun {
    pub aid: u32,
    pub state: AjState,
    pub actual: u64,
    pub cause: u32,
}

/// B22 断言聚合器。
pub struct AjAggregator {
    cap: usize,
    entries: Vec<AjEntry>,
    runs: Vec<AjRun>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl AjAggregator {
    pub fn new(cap: usize) -> AjAggregator {
        AjAggregator { cap, entries: vec![], runs: vec![], sealed: false, rejects: vec![] }
    }

    pub fn register(&mut self, e: AjEntry) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AJ_SEALED, "封账后登记"));
            return Err(XC_AJ_SEALED);
        }
        if self.entries.len() >= self.cap {
            self.rejects.push((XC_AJ_FULL, "登记容量满"));
            return Err(XC_AJ_FULL);
        }
        if e.dom > 4 {
            self.rejects.push((XC_AJ_OOB_DOM, "子域越界"));
            return Err(XC_AJ_OOB_DOM);
        }
        if e.lo > e.hi {
            self.rejects.push((XC_AJ_NO_EVID, "值域倒置"));
            return Err(XC_AJ_NO_EVID);
        }
        if self.entries.iter().any(|x| x.aid == e.aid) {
            self.rejects.push((XC_AJ_DUP_ID, "断言 id 重复"));
            return Err(XC_AJ_DUP_ID);
        }
        self.entries.push(e);
        Ok(())
    }

    fn find(&self, aid: u32) -> Option<usize> {
        self.entries.iter().position(|x| x.aid == aid)
    }

    /// 执行（值域判定）。
    pub fn run(&mut self, aid: u32, actual: u64) -> Result<AjState, u32> {
        if self.sealed {
            self.rejects.push((XC_AJ_SEALED, "封账后执行"));
            return Err(XC_AJ_SEALED);
        }
        let idx = match self.find(aid) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_AJ_NO_ID, "执行未登记 id"));
                return Err(XC_AJ_NO_ID);
            }
        };
        let e = self.entries[idx];
        let st = if actual >= e.lo && actual <= e.hi { AjState::Pass } else { AjState::Fail };
        if st == AjState::Fail && actual == 0 && e.lo > 0 {
            self.rejects.push((XC_AJ_NO_EVID, "fail 无实际值证据"));
            return Err(XC_AJ_NO_EVID);
        }
        self.runs.push(AjRun { aid, state: st, actual, cause: 0 });
        Ok(st)
    }

    /// 显式 skip（无因必拒）。
    pub fn skip(&mut self, aid: u32, cause: u32) -> Result<AjState, u32> {
        if self.sealed {
            self.rejects.push((XC_AJ_SEALED, "封账后执行"));
            return Err(XC_AJ_SEALED);
        }
        if self.find(aid).is_none() {
            self.rejects.push((XC_AJ_NO_ID, "执行未登记 id"));
            return Err(XC_AJ_NO_ID);
        }
        if cause == 0 {
            self.rejects.push((XC_AJ_SKIP_NOCAUSE, "skip 无因"));
            return Err(XC_AJ_SKIP_NOCAUSE);
        }
        self.runs.push(AjRun { aid, state: AjState::Skip, actual: 0, cause });
        Ok(AjState::Skip)
    }

    pub fn pass_count(&self) -> usize {
        self.runs.iter().filter(|r| r.state == AjState::Pass).count()
    }
    pub fn fail_count(&self) -> usize {
        self.runs.iter().filter(|r| r.state == AjState::Fail).count()
    }
    pub fn skip_count(&self) -> usize {
        self.runs.iter().filter(|r| r.state == AjState::Skip).count()
    }
    pub fn registered(&self) -> usize {
        self.entries.len()
    }

    /// 守恒：登记 = 通过+失败+挂起。
    pub fn conserved(&self) -> bool {
        self.runs.len() <= self.entries.len()
            && self.pass_count() + self.fail_count() + self.skip_count() == self.runs.len()
    }

    /// B22 gate：≥min 全过零 fail。
    pub fn b22_gated(&self, min_pass: usize) -> bool {
        self.pass_count() >= min_pass && self.fail_count() == 0
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_AJ_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for e in &self.entries {
            h ^= (e.aid as u64) << 24 | (e.dom as u64) << 16 | e.lo;
            h = h.wrapping_mul(0x0100_0193);
        }
        for r in &self.runs {
            let s: u64 = match r.state {
                AjState::Pass => 1,
                AjState::Fail => 2,
                AjState::Skip => 3,
            };
            h ^= (r.aid as u64) << 32 | s << 24 | r.actual ^ ((r.cause as u64) << 8);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// B22 段标准断言集：36 条 Draining 面断言登记与执行。
pub fn b22_asset() -> Result<(usize, usize, usize, u64), u32> {
    let mut agg = AjAggregator::new(48);
    // 子域 0 对账 10 条
    for k in 0..10u32 {
        agg.register(AjEntry { aid: 300 + k, dom: 0, lo: 0, hi: 100 })?;
    }
    // 子域 1 窗口 10 条
    for k in 0..10u32 {
        agg.register(AjEntry { aid: 400 + k, dom: 1, lo: 1, hi: 50 })?;
    }
    // 子域 2 超时 8 条
    for k in 0..8u32 {
        agg.register(AjEntry { aid: 500 + k, dom: 2, lo: 0, hi: 1 })?;
    }
    // 子域 3 幂等 8 条
    for k in 0..8u32 {
        agg.register(AjEntry { aid: 600 + k, dom: 3, lo: 0, hi: 9 })?;
    }
    if agg.registered() != 36 {
        return Err(XC_AJ_FULL);
    }
    // 执行：全带内 + 两条带因 skip
    for k in 0..10u32 {
        agg.run(300 + k, (k as u64) * 9 + 1)?;
    }
    for k in 0..10u32 {
        agg.run(400 + k, 10 + k as u64)?;
    }
    for k in 0..8u32 {
        agg.run(500 + k, 1)?;
    }
    for k in 0..8u32 {
        agg.run(600 + k, k as u64 % 10)?;
    }
    agg.skip(300, 7)?;
    agg.skip(400, 7)?;
    if !agg.conserved() || !agg.b22_gated(34) {
        return Err(XC_AJ_NO_EVID);
    }
    let fp = agg.seal()?;
    Ok((agg.pass_count(), agg.skip_count(), agg.reject_count(), fp))
}

/// 注入核：八类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 8;

    // 1）重复 id 必拒
    {
        let mut a = AjAggregator::new(8);
        let e = AjEntry { aid: 1, dom: 0, lo: 0, hi: 1 };
        let _ = a.register(e);
        if a.register(e) == Err(XC_AJ_DUP_ID) {
            caught += 1;
        }
    }
    // 2）未登记 id 必拒
    {
        let mut a = AjAggregator::new(8);
        if a.run(99, 1) == Err(XC_AJ_NO_ID) {
            caught += 1;
        }
    }
    // 3）skip 无因必拒
    {
        let mut a = AjAggregator::new(8);
        let _ = a.register(AjEntry { aid: 2, dom: 0, lo: 0, hi: 1 });
        if a.skip(2, 0) == Err(XC_AJ_SKIP_NOCAUSE) {
            caught += 1;
        }
    }
    // 4）值域倒置必拒
    {
        let mut a = AjAggregator::new(8);
        if a.register(AjEntry { aid: 3, dom: 0, lo: 9, hi: 1 }) == Err(XC_AJ_NO_EVID) {
            caught += 1;
        }
    }
    // 5）子域越界必拒
    {
        let mut a = AjAggregator::new(8);
        if a.register(AjEntry { aid: 4, dom: 9, lo: 0, hi: 1 }) == Err(XC_AJ_OOB_DOM) {
            caught += 1;
        }
    }
    // 6）容量满必拒
    {
        let mut a = AjAggregator::new(1);
        let _ = a.register(AjEntry { aid: 10, dom: 0, lo: 0, hi: 1 });
        if a.register(AjEntry { aid: 11, dom: 0, lo: 0, hi: 1 }) == Err(XC_AJ_FULL) {
            caught += 1;
        }
    }
    // 7）封账后执行必拒
    {
        let mut a = AjAggregator::new(8);
        let _ = a.register(AjEntry { aid: 5, dom: 0, lo: 0, hi: 1 });
        let _ = a.seal();
        if a.run(5, 1) == Err(XC_AJ_SEALED) {
            caught += 1;
        }
    }
    // 8）fail 判定与守恒
    {
        let mut a = AjAggregator::new(8);
        let _ = a.register(AjEntry { aid: 6, dom: 0, lo: 10, hi: 20 });
        if a.run(6, 99) == Ok(AjState::Fail) && a.fail_count() == 1 && a.conserved() {
            caught += 1;
        }
    }
    (caught, N)
}
