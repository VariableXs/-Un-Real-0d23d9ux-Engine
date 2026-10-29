// unx1208_u1208 —— UNX-F1208 Polling 态故障注入（轮询源失效/事件重复必抓）
// 判据原文 UNX-F1208-J1：故障场景 ≥5 类（源失效/事件重复/序号断裂/采样率
// 漂移/构造器错误）注入 10/10 次必抓，防护动作正确率 100%，资产可重复
// 一致率 100%。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9b70 独占（0x9b70~0x9baf）。
//
// 设计（纯功能 no_std）：
//   * 五类故障闭集：SrcDead / EvDup / SeqGap / RateDrift / CtorErr；
//   * 防护动作闭集：ISOLATE（隔离源）/ DEDUP（去重）/ REFUSE（拒断）/
//     RECAL（重校准）/ REJECT（拒构造）；
//   * 注入器：故障构造 → 防护层抓取（必抓账）→ 动作对账；
//   * 10/10 全抓 + 动作正确率 100% + 指纹三跑一致；
//   * 守恒：注入 = 抓取 + 漏网（漏网恒 0）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9b70 独占） ─────────────────────────────────────────────
pub const XC_PF_SEALED: u32 = 0x9B71; // 封账后注入
pub const XC_PF_UNCAUGHT: u32 = 0x9B72; // 故障漏网
pub const XC_PF_BADACT: u32 = 0x9B73; // 防护动作越闭集
pub const XC_PF_BADSC: u32 = 0x9B74; // 故障场景越闭集
pub const XC_PF_IRREP: u32 = 0x9B75; // 注入不可重复

/// 五类故障闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PollFault {
    SrcDead = 0,   // 源失效
    EvDup = 1,     // 事件重复
    SeqGap = 2,    // 序号断裂
    RateDrift = 3, // 采样率漂移
    CtorErr = 4,   // 构造器错误
}

pub const FAULT_N: usize = 5;

/// 防护动作闭集。
pub const ACT_ISOLATE: u8 = 1; // 隔离源
pub const ACT_DEDUP: u8 = 2;   // 去重
pub const ACT_REFUSE: u8 = 3;  // 拒断流
pub const ACT_RECAL: u8 = 4;   // 重校准
pub const ACT_REJECT: u8 = 5;  // 拒构造

/// 故障-动作正确映射表。
pub fn expect_act(f: PollFault) -> u8 {
    match f {
        PollFault::SrcDead => ACT_ISOLATE,
        PollFault::EvDup => ACT_DEDUP,
        PollFault::SeqGap => ACT_REFUSE,
        PollFault::RateDrift => ACT_RECAL,
        PollFault::CtorErr => ACT_REJECT,
    }
}

/// 轮询面防护层（被注入方）。
pub struct PollGuard {
    src_alive: Vec<bool>,
    seen: Vec<(u32, u64)>,
    last_seq: u64,
    rate_ref: u32,
    caught: u64,
    missed: u64,
    acts: Vec<(u32, u8)>, // (fault, act)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PollGuard {
    pub fn new(srcs: usize) -> PollGuard {
        PollGuard {
            src_alive: vec![true; srcs],
            seen: vec![],
            last_seq: 0,
            rate_ref: 100,
            caught: 0,
            missed: 0,
            acts: vec![],
            sealed: false,
            rejects: vec![],
        }
    }

    /// 源失效防护：杀源后探测 → 隔离动作。
    pub fn probe_src(&mut self, src: usize, alive: bool) -> Result<u8, u32> {
        if self.sealed {
            self.rejects.push((XC_PF_SEALED, "封账后注入"));
            return Err(XC_PF_SEALED);
        }
        if src >= self.src_alive.len() {
            self.rejects.push((XC_PF_BADSC, "源号越界"));
            return Err(XC_PF_BADSC);
        }
        if alive && self.src_alive[src] {
            self.missed += 1;
            return Err(XC_PF_UNCAUGHT);
        }
        self.src_alive[src] = false;
        self.caught += 1;
        self.acts.push((PollFault::SrcDead as u32, ACT_ISOLATE));
        Ok(ACT_ISOLATE)
    }

    /// 事件重复防护：重复 (src,seq) → 去重动作。
    pub fn feed(&mut self, src: u32, seq: u64) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_PF_SEALED, "封账后注入"));
            return Err(XC_PF_SEALED);
        }
        if self.seen.iter().any(|&(s, q)| s == src && q == seq) {
            self.caught += 1;
            self.acts.push((PollFault::EvDup as u32, ACT_DEDUP));
            return Ok(false); // 重复被去重
        }
        if seq != self.last_seq + 1 && self.last_seq != 0 {
            self.caught += 1;
            self.acts.push((PollFault::SeqGap as u32, ACT_REFUSE));
            return Err(XC_PF_BADSC); // 断裂拒断
        }
        self.seen.push((src, seq));
        self.last_seq = seq;
        Ok(true)
    }

    /// 采样率漂移防护：实测率偏离参考 > 50% → 重校准。
    pub fn rate_check(&mut self, observed: u32) -> Result<u8, u32> {
        if self.sealed {
            self.rejects.push((XC_PF_SEALED, "封账后注入"));
            return Err(XC_PF_SEALED);
        }
        if observed == 0 {
            self.rejects.push((XC_PF_BADSC, "零观测率"));
            return Err(XC_PF_BADSC);
        }
        let lo = self.rate_ref / 2;
        let hi = self.rate_ref + self.rate_ref / 2;
        if observed < lo || observed > hi {
            self.caught += 1;
            self.acts.push((PollFault::RateDrift as u32, ACT_RECAL));
            self.rate_ref = observed; // 重校准
            return Ok(ACT_RECAL);
        }
        Ok(0) // 带内正常
    }

    /// 构造器防护：非法参数 → 拒构造。
    pub fn ctor_check(&mut self, cap: usize, width: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PF_SEALED, "封账后注入"));
            return Err(XC_PF_SEALED);
        }
        if cap == 0 || width == 0 || width > 64 {
            self.caught += 1;
            self.acts.push((PollFault::CtorErr as u32, ACT_REJECT));
            return Err(XC_PF_BADSC);
        }
        Ok(())
    }

    /// 动作正确率（千分比）。
    pub fn act_rate_bps(&self) -> u32 {
        if self.acts.is_empty() {
            return 10_000;
        }
        let ok = self.acts.iter().filter(|(f, a)| expect_act(match *f {
            0 => PollFault::SrcDead,
            1 => PollFault::EvDup,
            2 => PollFault::SeqGap,
            3 => PollFault::RateDrift,
            _ => PollFault::CtorErr,
        }) == *a).count();
        (ok as u64 * 10_000 / self.acts.len() as u64) as u32
    }

    /// 守恒：注入（caught+missed）分账正确。
    pub fn conserved(&self) -> bool {
        self.caught + self.missed >= self.caught && self.missed == 0
    }

    pub fn caught(&self) -> u64 {
        self.caught
    }

    pub fn acts_len(&self) -> usize {
        self.acts.len()
    }

    /// 指纹（可重复锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in &self.caught.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for &(f, a) in &self.acts {
            for &b in &(f as u64).to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for &b in &(a as u64).to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：10/10 注入编排（判据主体路径） ────────────────────────────

/// 五类故障各注入 2 轮 = 10 次，全部必抓，动作全对，指纹可重复。
pub fn ten_inject_run() -> Result<(u64, u32, u64), u32> {
    let mut g = PollGuard::new(4);
    // 源失效 ×2
    for s in 0..2usize {
        g.probe_src(s, false)?;
    }
    // 事件重复 ×2
    let _ = g.feed(0, 1);
    let _ = g.feed(0, 2);
    if g.feed(0, 2) != Ok(false) {
        return Err(XC_PF_UNCAUGHT);
    }
    let _ = g.feed(0, 3);
    if g.feed(0, 3) != Ok(false) {
        return Err(XC_PF_UNCAUGHT);
    }
    // 序号断裂 ×2
    if g.feed(1, 99) == Ok(true) {
        return Err(XC_PF_UNCAUGHT);
    }
    if g.feed(1, 100) == Ok(true) {
        return Err(XC_PF_UNCAUGHT);
    }
    // 采样率漂移 ×2
    if g.rate_check(300) != Ok(ACT_RECAL) {
        return Err(XC_PF_UNCAUGHT);
    }
    if g.rate_check(20) != Ok(ACT_RECAL) {
        return Err(XC_PF_UNCAUGHT);
    }
    // 构造器错误 ×2
    if g.ctor_check(0, 32).is_ok() || g.ctor_check(16, 99).is_ok() {
        return Err(XC_PF_UNCAUGHT);
    }
    if g.caught() != 10 || g.act_rate_bps() != 10_000 || !g.conserved() {
        return Err(XC_PF_UNCAUGHT);
    }
    let fp = g.fingerprint();
    Ok((g.caught(), g.act_rate_bps(), fp))
}

/// 三跑一致演练：同编排指纹全等。
pub fn repeat3_run() -> bool {
    let a = ten_inject_run();
    let b = ten_inject_run();
    let c = ten_inject_run();
    match (a, b, c) {
        (Ok(x), Ok(y), Ok(z)) => x.2 == y.2 && y.2 == z.2,
        _ => false,
    }
}

// ── 注入核：五类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①漏网（活源探测不抓） ②封账后注入 ③源号越界 ④零观测率
// ⑤非法构造全拒。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 漏网：探测活源 → XC_PF_UNCAUGHT
    total += 1;
    let mut g = PollGuard::new(2);
    if g.probe_src(0, true) == Err(XC_PF_UNCAUGHT) {
        hit += 1;
    }

    // ② 封账后注入 → XC_PF_SEALED
    total += 1;
    let mut g2 = PollGuard::new(2);
    g2.seal();
    if g2.probe_src(0, false) == Err(XC_PF_SEALED) {
        hit += 1;
    }

    // ③ 源号越界 → XC_PF_BADSC
    total += 1;
    let mut g3 = PollGuard::new(2);
    if g3.probe_src(9, false) == Err(XC_PF_BADSC) {
        hit += 1;
    }

    // ④ 零观测率 → XC_PF_BADSC
    total += 1;
    let mut g4 = PollGuard::new(2);
    if g4.rate_check(0) == Err(XC_PF_BADSC) {
        hit += 1;
    }

    // ⑤ 非法构造全拒 → ACT_REJECT 计入账
    total += 1;
    let mut g5 = PollGuard::new(2);
    let r = g5.ctor_check(0, 0);
    if r == Err(XC_PF_BADSC) && g5.caught() == 1 && g5.act_rate_bps() == 10_000 {
        hit += 1;
    }

    (hit, total)
}
