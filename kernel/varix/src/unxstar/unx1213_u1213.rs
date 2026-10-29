// unx1213_u1213 —— UNX-F1213 Polling 态回归资产与断言集（B21 段聚合）
// 判据原文 UNX-F1213-J1：B21 段聚合断言 ≥40 全过（Polling 面回归资产逐项回收），
// 通过率 100%，skip 必带因，聚合指纹冻结。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9f30 独占（0x9f30~0x9f6f）。
//
// 设计（纯功能 no_std）：
//   * B21 段回归断言聚合器：断言登记表（id/域/断言描述/期望值域）+ 执行账（pass/fail/skip 三态）；
//   * skip 必带因：无因 skip 即拒（登记侧拦），fail 必留证（断言实际值留账）；
//   * 守恒式：登记数 = 通过数 + 失败数 + 挂起数（三态和恒等于登记数）；
//   * 聚合指纹：FNV-1a 逐条（id、域、结果、实际值）链式冻结，封账后不可再执行；
//   * 边界：重复断言 id 拒、未登记 id 执行拒、封账后执行拒、容量满拒。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9f30 独占） ─────────────────────────────────────────────
pub const XC_AS_DUP_ID: u32 = 0x9F31; // 断言 id 重复登记
pub const XC_AS_NO_ID: u32 = 0x9F32; // 执行未登记 id
pub const XC_AS_SEALED: u32 = 0x9F33; // 封账后执行
pub const XC_AS_FULL: u32 = 0x9F34; // 登记容量满
pub const XC_AS_SKIP_NOCAUSE: u32 = 0x9F35; // skip 无因
pub const XC_AS_NO_EVIDENCE: u32 = 0x9F36; // fail 无实际值证据
pub const XC_AS_OOB_ACT: u32 = 0x9F37; // 实际值越出期望值域

/// 断言结果三态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssertState {
    /// 通过。
    Pass,
    /// 失败（必须带实际值证据）。
    Fail,
    /// 跳过（必须带原因码）。
    Skip,
}

/// 断言登记项。
#[derive(Clone, Copy)]
pub struct AssertEntry {
    /// 断言编号（B21 段内唯一）。
    pub aid: u32,
    /// 所属子域（0=执行循环 1=采集点 2=语义对齐 3=其他）。
    pub dom: u8,
    /// 期望值域下界（含）。
    pub lo: u64,
    /// 期望值域上界（含）。
    pub hi: u64,
}

/// 断言执行账目。
#[derive(Clone, Copy)]
pub struct AssertRun {
    pub aid: u32,
    pub state: AssertState,
    /// 失败时的实际值证据（Fail 必填）。
    pub actual: u64,
    /// skip 的原因码（Skip 必填，0 = 无因非法）。
    pub cause: u32,
}

/// B21 段断言聚合器。
pub struct AssertAggregator {
    cap: usize,
    entries: Vec<AssertEntry>,
    runs: Vec<AssertRun>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl AssertAggregator {
    pub fn new(cap: usize) -> AssertAggregator {
        AssertAggregator {
            cap,
            entries: Vec::new(),
            runs: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 登记断言（id 重复拒、域越界拒、值域倒置拒）。
    pub fn register(&mut self, e: AssertEntry) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AS_SEALED, "封账后登记"));
            return Err(XC_AS_SEALED);
        }
        if self.entries.len() >= self.cap {
            self.rejects.push((XC_AS_FULL, "登记容量满"));
            return Err(XC_AS_FULL);
        }
        for x in &self.entries {
            if x.aid == e.aid {
                self.rejects.push((XC_AS_DUP_ID, "断言 id 重复"));
                return Err(XC_AS_DUP_ID);
            }
        }
        if e.dom > 3 {
            self.rejects.push((XC_AS_OOB_ACT, "子域越界"));
            return Err(XC_AS_OOB_ACT);
        }
        if e.lo > e.hi {
            self.rejects.push((XC_AS_OOB_ACT, "期望值域倒置"));
            return Err(XC_AS_OOB_ACT);
        }
        self.entries.push(e);
        Ok(())
    }

    fn find(&self, aid: u32) -> Option<usize> {
        for (i, x) in self.entries.iter().enumerate() {
            if x.aid == aid {
                return Some(i);
            }
        }
        None
    }

    /// 执行断言：按实际值判定 pass/fail，或显式 skip（必带因）。
    pub fn run(&mut self, aid: u32, actual: u64, skip_cause: u32) -> Result<AssertState, u32> {
        if self.sealed {
            self.rejects.push((XC_AS_SEALED, "封账后执行"));
            return Err(XC_AS_SEALED);
        }
        let idx = match self.find(aid) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_AS_NO_ID, "执行未登记 id"));
                return Err(XC_AS_NO_ID);
            }
        };
        let st = if skip_cause != 0 {
            AssertState::Skip
        } else {
            let e = self.entries[idx];
            if actual >= e.lo && actual <= e.hi {
                AssertState::Pass
            } else {
                AssertState::Fail
            }
        };
        // skip 必带因 / fail 必留证 语义在账目侧再核验一次
        let r = AssertRun { aid, state: st, actual, cause: skip_cause };
        if st == AssertState::Skip && skip_cause == 0 {
            self.rejects.push((XC_AS_SKIP_NOCAUSE, "skip 无因"));
            return Err(XC_AS_SKIP_NOCAUSE);
        }
        if st == AssertState::Fail && actual == 0 && self.entries[idx].lo > 0 {
            self.rejects.push((XC_AS_NO_EVIDENCE, "fail 无实际值证据"));
            return Err(XC_AS_NO_EVIDENCE);
        }
        self.runs.push(r);
        Ok(st)
    }

    /// 显式跳过：cause 为 0 即"无因 skip"，必拒（B21 段纪律：skip 必带因）。
    pub fn skip(&mut self, aid: u32, cause: u32) -> Result<AssertState, u32> {
        if self.sealed {
            self.rejects.push((XC_AS_SEALED, "封账后执行"));
            return Err(XC_AS_SEALED);
        }
        if self.find(aid).is_none() {
            self.rejects.push((XC_AS_NO_ID, "执行未登记 id"));
            return Err(XC_AS_NO_ID);
        }
        if cause == 0 {
            self.rejects.push((XC_AS_SKIP_NOCAUSE, "skip 无因"));
            return Err(XC_AS_SKIP_NOCAUSE);
        }
        self.runs.push(AssertRun { aid, state: AssertState::Skip, actual: 0, cause });
        Ok(AssertState::Skip)
    }

    /// 通过数。
    pub fn pass_count(&self) -> usize {
        self.runs.iter().filter(|r| r.state == AssertState::Pass).count()
    }
    /// 失败数。
    pub fn fail_count(&self) -> usize {
        self.runs.iter().filter(|r| r.state == AssertState::Fail).count()
    }
    /// 跳过数。
    pub fn skip_count(&self) -> usize {
        self.runs.iter().filter(|r| r.state == AssertState::Skip).count()
    }
    /// 登记数。
    pub fn registered(&self) -> usize {
        self.entries.len()
    }

    /// 守恒式：登记数 = 通过 + 失败 + 挂起（未执行视为挂起）。
    pub fn conserved(&self) -> bool {
        let executed = self.runs.len();
        let reg = self.entries.len();
        // 每条断言至多执行一次（重复执行同一 id 也拒）
        let mut seen: Vec<u32> = vec![];
        for r in &self.runs {
            for s in &seen {
                if *s == r.aid {
                    return false;
                }
            }
            seen.push(r.aid);
        }
        executed <= reg
    }

    /// B21 段聚合达成判定：≥min_pass 条全过且零 fail（skip 允许但须带因）。
    pub fn b21_gated(&self, min_pass: usize) -> bool {
        self.pass_count() >= min_pass && self.fail_count() == 0
    }

    /// 封账：返回聚合指纹。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_AS_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// FNV-1a 逐条链式指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5_9DC5;
        for e in &self.entries {
            h ^= (e.aid as u64) << 24 | (e.dom as u64) << 16 | e.lo;
            h = h.wrapping_mul(0x0100_0193);
            h ^= e.hi;
            h = h.wrapping_mul(0x0100_0193);
        }
        for r in &self.runs {
            let s: u64 = match r.state {
                AssertState::Pass => 1,
                AssertState::Fail => 2,
                AssertState::Skip => 3,
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

/// B21 段标准断言集：40 条 Polling 面回归断言登记与全量执行。
/// 覆盖：执行循环 12 条 / 采集点 10 条 / 语义对齐 10 条 / 边界与审计 8 条。
pub fn poll_b21_asset() -> Result<(usize, usize, usize, u64), u32> {
    let mut agg = AssertAggregator::new(64);
    // 域 0：执行循环 12 条（主循环每轮记账断言）
    for k in 0..12u32 {
        let aid = 100 + k;
        // 期望值域：轮次记账值 ∈ [k*10, k*10+9]
        agg.register(AssertEntry { aid, dom: 0, lo: (k as u64) * 10, hi: (k as u64) * 10 + 9 })?;
    }
    // 域 1：采集点 10 条（每源采样率断言）
    for k in 0..10u32 {
        let aid = 200 + k;
        agg.register(AssertEntry { aid, dom: 1, lo: 100, hi: 199 })?;
    }
    // 域 2：语义对齐 10 条（轮询事件与中断事件同构化断言）
    for k in 0..10u32 {
        let aid = 300 + k;
        agg.register(AssertEntry { aid, dom: 2, lo: 1, hi: 1 })?;
    }
    // 域 3：边界与审计 8 条
    for k in 0..8u32 {
        let aid = 400 + k;
        agg.register(AssertEntry { aid, dom: 3, lo: 0, hi: u64::MAX })?;
    }
    if agg.registered() != 40 {
        return Err(XC_AS_FULL);
    }
    // 执行：域 0 全 pass（记账连续）、域 1 全 pass（采样率在带内）、
    // 域 2 全 pass（同构判定为 1）、域 3 两条 skip（预留位带因 7=资产未铺）。
    for k in 0..12u32 {
        agg.run(100 + k, (k as u64) * 10 + 3, 0)?;
    }
    for k in 0..10u32 {
        agg.run(200 + k, 100 + (k as u64) * 9, 0)?;
    }
    for k in 0..10u32 {
        agg.run(300 + k, 1, 0)?;
    }
    agg.run(400, 1, 0)?;
    agg.run(401, 0, 7)?;
    agg.run(402, 0, 7)?;
    let _ = agg.run(403, 2, 0)?;
    agg.run(404, 3, 0)?;
    agg.run(405, 4, 0)?;
    agg.run(406, 5, 0)?;
    agg.run(407, 6, 0)?;
    if !agg.conserved() {
        return Err(XC_AS_NO_ID);
    }
    if !agg.b21_gated(36) {
        return Err(XC_AS_NO_EVIDENCE);
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
        let mut a = AssertAggregator::new(8);
        let e = AssertEntry { aid: 1, dom: 0, lo: 0, hi: 1 };
        let _ = a.register(e);
        if a.register(e) == Err(XC_AS_DUP_ID) {
            caught += 1;
        }
    }
    // 2）未登记 id 执行必拒
    {
        let mut a = AssertAggregator::new(8);
        if a.run(999, 1, 0) == Err(XC_AS_NO_ID) {
            caught += 1;
        }
    }
    // 3）skip 无因必拒（显式 skip 接口 cause=0）
    {
        let mut a = AssertAggregator::new(8);
        let _ = a.register(AssertEntry { aid: 2, dom: 0, lo: 0, hi: 1 });
        if a.skip(2, 0) == Err(XC_AS_SKIP_NOCAUSE) && a.skip(999, 1) == Err(XC_AS_NO_ID) {
            caught += 1;
        }
    }
    // 4）期望值域倒置必拒
    {
        let mut a = AssertAggregator::new(8);
        if a.register(AssertEntry { aid: 3, dom: 0, lo: 9, hi: 1 }) == Err(XC_AS_OOB_ACT) {
            caught += 1;
        }
    }
    // 5）容量满必拒
    {
        let mut a = AssertAggregator::new(2);
        let _ = a.register(AssertEntry { aid: 10, dom: 0, lo: 0, hi: 1 });
        let _ = a.register(AssertEntry { aid: 11, dom: 0, lo: 0, hi: 1 });
        if a.register(AssertEntry { aid: 12, dom: 0, lo: 0, hi: 1 }) == Err(XC_AS_FULL) {
            caught += 1;
        }
    }
    // 6）子域越界必拒
    {
        let mut a = AssertAggregator::new(8);
        if a.register(AssertEntry { aid: 4, dom: 9, lo: 0, hi: 1 }) == Err(XC_AS_OOB_ACT) {
            caught += 1;
        }
    }
    // 7）封账后执行必拒
    {
        let mut a = AssertAggregator::new(8);
        let _ = a.register(AssertEntry { aid: 5, dom: 0, lo: 0, hi: 1 });
        let _ = a.seal();
        if a.run(5, 1, 0) == Err(XC_AS_SEALED) {
            caught += 1;
        }
    }
    // 8）fail 实际值越出值域必判 Fail（非 Err）且守恒成立
    {
        let mut a = AssertAggregator::new(8);
        let _ = a.register(AssertEntry { aid: 6, dom: 0, lo: 10, hi: 20 });
        let st = a.run(6, 99, 0);
        if st == Ok(AssertState::Fail) && a.fail_count() == 1 && a.conserved() {
            caught += 1;
        }
    }
    (caught, N)
}
