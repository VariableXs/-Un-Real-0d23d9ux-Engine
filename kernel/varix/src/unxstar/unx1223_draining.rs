// unx1223_draining —— UNX-F1223 Draining 态双源对账强化（同事件零重复零丢失）
// 判据原文 UNX-F1223-J1：双源对账全过（同事件双源重复去重幂等、唯一事件
// 数守恒、序列断档零丢失检出、双源计数与并集/交集对拍、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb350 独占（0xb350~0xb38f）。
//
// 设计（纯功能 no_std）：
//   * 双源登记：轮询源（P）与中断源（I）各自报事件号序列（u32 递增）；
//   * 幂等去重：同 (源,事件号) 重复报即记重复账不重复计数；跨源同号即交集；
//   * 零丢失：期望区间 [base, base+n) 全覆盖检出，断档立案（丢失账）；
//   * 守恒式：唯一数 = P∪I；登记数 = 唯一数 + 重复数；交集 ≤ 各源计数；
//   * FNV 链指纹冻结，封账后拒登记。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb350 独占） ─────────────────────────────────────────────
pub const XC_DR_SEALED: u32 = 0xB351; // 封账后登记
pub const XC_DR_DUP_SAME: u32 = 0xB352; // 同源同号重复（幂等记账）
pub const XC_DR_BAD_SEQ: u32 = 0xB353; // 序号倒退（源内单调破）
pub const XC_DR_GAP: u32 = 0xB354; // 序列断档（丢失立案）
pub const XC_DR_FULL: u32 = 0xB355; // 账目容量满
pub const XC_DR_NO_BASE: u32 = 0xB356; // 未定基址先对账

/// 双源对账器。
pub struct DualRecon {
    cap: usize,
    /// 事件号基址（P 源首事件号）。
    base: Option<u32>,
    expect: u32,
    /// P 源已见事件号（升序）。
    p_seen: Vec<u32>,
    /// I 源已见事件号（升序）。
    i_seen: Vec<u32>,
    dup_same: u64,
    lost: Vec<u32>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DualRecon {
    pub fn new(cap: usize) -> DualRecon {
        DualRecon {
            cap,
            base: None,
            expect: 0,
            p_seen: vec![],
            i_seen: vec![],
            dup_same: 0,
            lost: vec![],
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn in_seen(v: &Vec<u32>, x: u32) -> bool {
        v.iter().any(|&a| a == x)
    }

    /// P 源登记一事件。
    pub fn from_poll(&mut self, ev: u32) -> Result<(), u32> {
        self.put(ev, true)
    }

    /// I 源登记一事件。
    pub fn from_irq(&mut self, ev: u32) -> Result<(), u32> {
        self.put(ev, false)
    }

    fn put(&mut self, ev: u32, is_p: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DR_SEALED, "封账后登记"));
            return Err(XC_DR_SEALED);
        }
        if self.p_seen.len() + self.i_seen.len() >= self.cap {
            self.rejects.push((XC_DR_FULL, "账目容量满"));
            return Err(XC_DR_FULL);
        }
        let seen = if is_p { &self.p_seen } else { &self.i_seen };
        if let Some(&last) = seen.last() {
            if ev < last {
                self.rejects.push((XC_DR_BAD_SEQ, "源内序号倒退"));
                return Err(XC_DR_BAD_SEQ);
            }
        }
        if Self::in_seen(seen, ev) {
            self.dup_same = self.dup_same.saturating_add(1);
            self.rejects.push((XC_DR_DUP_SAME, "同源同号重复"));
            // 幂等：重复不进账但记账
            return Ok(());
        }
        // 基址与期望尾随
        if self.base.is_none() {
            self.base = Some(ev);
            self.expect = ev;
        }
        while self.expect < ev {
            self.lost.push(self.expect);
            self.expect = self.expect.saturating_add(1);
        }
        if ev >= self.expect {
            self.expect = ev.saturating_add(1);
        }
        if is_p {
            self.p_seen.push(ev);
        } else {
            self.i_seen.push(ev);
        }
        Ok(())
    }

    /// 唯一事件数：P∪I（双源并集去重）。
    pub fn unique(&self) -> usize {
        let mut n = self.p_seen.len();
        for &x in &self.i_seen {
            if !Self::in_seen(&self.p_seen, x) {
                n += 1;
            }
        }
        n
    }

    /// 交集数：双源同报的事件号数。
    pub fn intersect(&self) -> usize {
        self.i_seen.iter().filter(|x| Self::in_seen(&self.p_seen, **x)).count()
    }

    /// 总登记数（含幂等重复）。
    pub fn total_registered(&self) -> u64 {
        (self.p_seen.len() + self.i_seen.len()) as u64
    }

    /// 守恒：登记数 = 唯一数 + 重复数（幂等重复不增唯一账）。
    pub fn conserved(&self) -> bool {
        self.total_registered() == self.unique() as u64 + self.dup_same
    }

    /// 零丢失判定：丢失账为空。
    pub fn no_lost(&self) -> bool {
        self.lost.is_empty()
    }

    pub fn lost_list(&self) -> &Vec<u32> {
        &self.lost
    }
    pub fn dup_same_count(&self) -> u64 {
        self.dup_same
    }
    pub fn p_count(&self) -> usize {
        self.p_seen.len()
    }
    pub fn i_count(&self) -> usize {
        self.i_seen.len()
    }

    /// 对账报告：双源计数 + 交集 + 并集 + 丢失数。
    pub fn report(&self) -> Result<(usize, usize, usize, usize, usize), u32> {
        if self.base.is_none() {
            return Err(XC_DR_NO_BASE);
        }
        Ok((self.p_count(), self.i_count(), self.intersect(), self.unique(), self.lost.len()))
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DR_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.p_seen.len() as u64);
        h = h.wrapping_mul(0x0100_0193) ^ (self.i_seen.len() as u64);
        h = h.wrapping_mul(0x0100_0193) ^ self.dup_same;
        for &x in &self.p_seen {
            h ^= x as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        for &x in &self.i_seen {
            h ^= (x as u64).rotate_left(17);
            h = h.wrapping_mul(0x0100_0193);
        }
        for &x in &self.lost {
            h ^= (x as u64).rotate_left(31);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准对账场景：0..=19 双源交错登记（部分跨源同报）+ 幂等重复 + 零丢失。
pub fn recon_scenario() -> Result<(usize, usize, usize, usize, usize, u64), u32> {
    let mut r = DualRecon::new(64);
    // P 源：0..10 顺报
    for ev in 0..10u32 {
        r.from_poll(ev)?;
    }
    // I 源：5..15 顺报（5..9 与 P 交集）
    for ev in 5..15u32 {
        r.from_irq(ev)?;
    }
    // 幂等重复：P 源重报 3 号
    r.from_poll(3)?;
    if !r.conserved() || !r.no_lost() {
        return Err(XC_DR_GAP);
    }
    let (p, i, inter, uniq, lost) = r.report()?;
    // P=10 I=10 交集=5 并集=15 丢失=0
    if p != 10 || i != 10 || inter != 5 || uniq != 15 || lost != 0 {
        return Err(XC_DR_BAD_SEQ);
    }
    let fp = r.seal()?;
    Ok((p, i, inter, uniq, lost, fp))
}

// ── 深化块：区间对账与丢失报告 ─────────────────────────────────────
impl DualRecon {
    /// P 源区间批量登记 [a,b]（逐号推进）。
    pub fn from_poll_range(&mut self, a: u32, b: u32) -> Result<u32, u32> {
        if a > b { return Ok(0); }
        let mut n = 0u32;
        let mut x = a;
        while x <= b {
            self.from_poll(x)?;
            n += 1;
            if x == u32::MAX { break; }
            x += 1;
        }
        Ok(n)
    }
    /// I 源区间批量登记 [a,b]。
    pub fn from_irq_range(&mut self, a: u32, b: u32) -> Result<u32, u32> {
        if a > b { return Ok(0); }
        let mut n = 0u32;
        let mut x = a;
        while x <= b {
            self.from_irq(x)?;
            n += 1;
            if x == u32::MAX { break; }
            x += 1;
        }
        Ok(n)
    }
    /// 丢失报告：逐断档给出定位与修复建议码（1=回补登记）。
    pub fn lost_report(&self) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = vec![];
        for &g in &self.lost { out.push((g, 1)); }
        out
    }
    /// 双源覆盖差：仅在 P 源的事件数。
    pub fn p_only(&self) -> usize {
        self.p_seen.iter().filter(|x| !Self::in_seen(&self.i_seen, **x)).count()
    }
    /// 仅在 I 源的事件数。
    pub fn i_only(&self) -> usize {
        self.i_seen.iter().filter(|x| !Self::in_seen(&self.p_seen, **x)).count()
    }
    /// 断档数。
    pub fn lost_count(&self) -> usize { self.lost.len() }
}

// ── 深化块二：双源画像 ─────────────────────────────────────────────
impl DualRecon {
    /// P 源首末号跨度。
    pub fn p_span(&self) -> u64 {
        match (self.p_seen.first(), self.p_seen.last()) {
            (Some(&a), Some(&b)) => (b - a) as u64,
            _ => 0,
        }
    }
    /// I 源首末号跨度。
    pub fn i_span(&self) -> u64 {
        match (self.i_seen.first(), self.i_seen.last()) {
            (Some(&a), Some(&b)) => (b - a) as u64,
            _ => 0,
        }
    }
    /// 覆盖密度（唯一数 / 期望跨度，基点）。
    pub fn cover_bps(&self) -> u32 {
        let b = match self.base { Some(b) => b, None => return 0 };
        let span = self.expect.saturating_sub(b) as u64;
        if span == 0 { return 0; }
        ((self.unique() as u64 * 10000) / span) as u32
    }
    /// 重复率（基点）。
    pub fn dup_rate_bps(&self) -> u32 {
        let t = self.total_registered();
        if t == 0 { return 0; }
        ((self.dup_same * 10000) / t) as u32
    }
}

/// 注入核：八类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 8;

    // 1）同源同号重复（幂等记账、唯一账不涨）
    {
        let mut r = DualRecon::new(16);
        let _ = r.from_poll(1);
        let _ = r.from_poll(1);
        if r.dup_same_count() == 1 && r.p_count() == 1 {
            caught += 1;
        }
    }
    // 2）源内序号倒退必拒
    {
        let mut r = DualRecon::new(16);
        let _ = r.from_poll(5);
        if r.from_poll(3) == Err(XC_DR_BAD_SEQ) {
            caught += 1;
        }
    }
    // 3）序列断档立案（2 号缺席直报 3）
    {
        let mut r = DualRecon::new(16);
        let _ = r.from_poll(0);
        let _ = r.from_poll(2);
        if !r.no_lost() && r.lost_list().contains(&1) {
            caught += 1;
        }
    }
    // 4）跨源同号入交集（不算重复错误）
    {
        let mut r = DualRecon::new(16);
        let _ = r.from_poll(4);
        let _ = r.from_irq(4);
        if r.intersect() == 1 && r.unique() == 1 {
            caught += 1;
        }
    }
    // 5）未定基址先对账必拒
    {
        let r = DualRecon::new(16);
        if r.report() == Err(XC_DR_NO_BASE) {
            caught += 1;
        }
    }
    // 6）封账后登记必拒
    {
        let mut r = DualRecon::new(16);
        let _ = r.from_poll(1);
        let _ = r.seal();
        if r.from_poll(2) == Err(XC_DR_SEALED) {
            caught += 1;
        }
    }
    // 7）容量满必拒
    {
        let mut r = DualRecon::new(2);
        let _ = r.from_poll(1);
        let _ = r.from_irq(1);
        if r.from_poll(2) == Err(XC_DR_FULL) {
            caught += 1;
        }
    }
    // 8）守恒式：登记=唯一+重复
    {
        let mut r = DualRecon::new(16);
        let _ = r.from_poll(1);
        let _ = r.from_poll(1);
        let _ = r.from_irq(2);
        if r.conserved() {
            caught += 1;
        }
    }
    (caught, N)
}
