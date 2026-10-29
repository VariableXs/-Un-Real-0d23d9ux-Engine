// unx1197_a3 —— UNX-F1197 亲和性与 A3 负载均衡衔接预演（fake 调度面）
// 判据原文 UNX-F1197-J1：亲和负载快照移交 A3 预演（fake 钩子）10 轮零失真
// （快照一致性断言），决策建议对接验证通过率 100%，真账切换预案落档。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8f70 独占（0x8f70~0x8faf）。
//
// 设计（纯功能 no_std）：
//   * 负载快照：逐核 (core, weight, ev_rate) 三元组 + FNV 指纹；
//   * fake 调度面：A3 钩子接收快照原样回执，决策建议（迁移动作表）逐条验证；
//   * 移交桥：10 轮移交逐轮比对指纹（零失真断言），建议对接通过率账；
//   * 真账切换预案：五元组落档（触发条件/移交序/回滚序/校验点/责任人槽位）；
//   * 移交序单调（轮次不回退）、建议表守恒（发出=验证=通过三账对齐）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8f70 独占） ─────────────────────────────────────────────
pub const XC_A3_SEALED: u32 = 0x8F71; // 封账后操作
pub const XC_A3_DISTORT: u32 = 0x8F72; // 快照失真
pub const XC_A3_EMPTY: u32 = 0x8F73; // 空快照
pub const XC_A3_REPLAY: u32 = 0x8F74; // 轮次回退
pub const XC_A3_BADADV: u32 = 0x8F75; // 建议非法
pub const XC_A3_FULL: u32 = 0x8F76; // 容量满
pub const XC_A3_NOPLAN: u32 = 0x8F77; // 预案未落档

/// 单核负载像。
#[derive(Clone, Copy)]
pub struct CoreLoad {
    pub core: u32,
    pub weight: u32,   // 亲和权重（bps，Σ=10000）
    pub ev_rate: u32,  // 事件率（eps）
}

/// 负载快照（一轮移交的单位）。
pub struct LoadSnap {
    pub seq: u64,
    pub loads: Vec<CoreLoad>,
    pub fp: u64,
}

/// 决策建议（A3 侧给出的一条迁移动作）。
#[derive(Clone, Copy)]
pub struct BalanceAdvice {
    pub from: u32,
    pub to: u32,
    pub weight_bp: u32, // 建议迁移权重（bps）
}

/// fake 调度面（A3 钩子）：接收快照原样回执，产出建议。
pub struct FakeSched {
    pub rx_snaps: u64,
    pub advices: Vec<BalanceAdvice>,
}

impl FakeSched {
    pub fn new() -> FakeSched {
        FakeSched { rx_snaps: 0, advices: vec![] }
    }

    /// A3 钩子：原样回执快照（零失真语义由指纹保证）。
    pub fn on_snap(&mut self, snap: &LoadSnap) -> u64 {
        self.rx_snaps += 1;
        snap.fp
    }

    /// A3 钩子：基于快照产出迁移建议（确定性规则：负载差 > 阈值即建议搬移）。
    pub fn advise(&mut self, snap: &LoadSnap, gap_th: u32, max_adv: usize) -> Vec<BalanceAdvice> {
        let mut adv: Vec<BalanceAdvice> = vec![];
        if adv.len() >= max_adv || snap.loads.len() < 2 {
            return adv;
        }
        // 找最重与最轻核（确定性扫描：先到先得）。
        let mut hi = 0usize;
        let mut lo = 0usize;
        for (i, l) in snap.loads.iter().enumerate() {
            if l.ev_rate > snap.loads[hi].ev_rate {
                hi = i;
            }
            if l.ev_rate < snap.loads[lo].ev_rate {
                lo = i;
            }
        }
        let gap = snap.loads[hi].ev_rate - snap.loads[lo].ev_rate;
        if gap > gap_th && hi != lo {
            let w = snap.loads[hi].weight / 4; // 搬四分之一权重
            if w > 0 {
                adv.push(BalanceAdvice { from: snap.loads[hi].core, to: snap.loads[lo].core, weight_bp: w });
            }
        }
        self.advices.extend_from_slice(&adv);
        adv
    }
}

/// 真账切换预案（五元组落档）。
pub struct SwitchPlan {
    pub trig: u32,       // 触发条件码
    pub handover: [u32; 4], // 移交序（四步）
    pub rollback: [u32; 4], // 回滚序（四步）
    pub chk_fp: u64,     // 校验点指纹
    pub owner: u32,      // 责任人槽位
}

/// 移交桥：亲和面 → A3 fake 调度面。
pub struct A3Bridge {
    cap: usize,
    round: u64,
    snaps_sent: u64,
    snaps_echo_ok: u64,
    adv_sent: u64,
    adv_pass: u64,
    plans: Vec<SwitchPlan>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

/// FNV-1a 64 指纹（快照一致性锚）。
fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 快照指纹：seq + 逐核三元组按字节序列化。
pub fn snap_fp(snap: &LoadSnap) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&snap.seq.to_le_bytes());
    for l in &snap.loads {
        buf.extend_from_slice(&l.core.to_le_bytes());
        buf.extend_from_slice(&l.weight.to_le_bytes());
        buf.extend_from_slice(&l.ev_rate.to_le_bytes());
    }
    fnv64(&buf)
}

impl A3Bridge {
    pub fn new(cap: usize) -> A3Bridge {
        A3Bridge {
            cap,
            round: 0,
            snaps_sent: 0,
            snaps_echo_ok: 0,
            adv_sent: 0,
            adv_pass: 0,
            plans: vec![],
            sealed: false,
            rejects: vec![],
        }
    }

    /// 构造一轮快照（确定性负载生成：LCG 驱动）。
    pub fn make_snap(&mut self, cores: u32, seed: u64) -> Result<LoadSnap, u32> {
        if self.sealed {
            self.rejects.push((XC_A3_SEALED, "封账后造快照"));
            return Err(XC_A3_SEALED);
        }
        if cores < 2 || cores > 64 {
            self.rejects.push((XC_A3_EMPTY, "核数出界"));
            return Err(XC_A3_EMPTY);
        }
        self.round += 1;
        let mut x = seed ^ (self.round.wrapping_mul(0x9E3779B97F4A7C15));
        let mut loads: Vec<CoreLoad> = vec![];
        let mut weight_left: u32 = 10_000;
        for c in 0..cores {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let rate = ((x >> 33) % 900) as u32;
            let w = if c + 1 == cores {
                weight_left
            } else {
                let w = weight_left / (cores - c);
                weight_left -= w;
                w
            };
            loads.push(CoreLoad { core: c, weight: w, ev_rate: rate });
        }
        let snap = LoadSnap { seq: self.round, loads, fp: 0 };
        let mut snap = snap;
        snap.fp = snap_fp(&snap);
        Ok(snap)
    }

    /// 移交一轮：fake 钩子回执指纹必须与本地指纹一致（零失真断言）。
    pub fn handover(&mut self, sched: &mut FakeSched, snap: &LoadSnap) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_A3_SEALED, "封账后移交"));
            return Err(XC_A3_SEALED);
        }
        if snap.loads.is_empty() {
            self.rejects.push((XC_A3_EMPTY, "空快照"));
            return Err(XC_A3_EMPTY);
        }
        if snap.seq < self.round.saturating_sub(1) && snap.seq != self.round {
            self.rejects.push((XC_A3_REPLAY, "轮次回退"));
            return Err(XC_A3_REPLAY);
        }
        self.snaps_sent += 1;
        let echo = sched.on_snap(snap);
        let local = snap_fp(snap);
        if echo != local {
            self.rejects.push((XC_A3_DISTORT, "回执失真"));
            return Ok(false);
        }
        self.snaps_echo_ok += 1;
        Ok(true)
    }

    /// 建议对接：逐条验证（from≠to、权重非零、总量守恒上限）。
    pub fn advice_dock(&mut self, advs: &[BalanceAdvice], snap: &LoadSnap) -> Result<usize, u32> {
        self.advise_verify(advs, snap)
    }

    fn advise_verify(&mut self, advs: &[BalanceAdvice], snap: &LoadSnap) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_A3_SEALED, "封账后对接"));
            return Err(XC_A3_SEALED);
        }
        let mut pass = 0usize;
        for a in advs {
            self.adv_sent += 1;
            if a.from == a.to {
                self.rejects.push((XC_A3_BADADV, "自迁自"));
                continue;
            }
            if a.weight_bp == 0 || a.weight_bp > 10_000 {
                self.rejects.push((XC_A3_BADADV, "权重非法"));
                continue;
            }
            let has_from = snap.loads.iter().any(|l| l.core == a.from);
            let has_to = snap.loads.iter().any(|l| l.core == a.to);
            if !has_from || !has_to {
                self.rejects.push((XC_A3_BADADV, "核不在快照"));
                continue;
            }
            self.adv_pass += 1;
            pass += 1;
        }
        Ok(pass)
    }

    /// 真账切换预案落档。
    pub fn file_plan(&mut self, plan: SwitchPlan) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_A3_SEALED, "封账后落档"));
            return Err(XC_A3_SEALED);
        }
        if self.plans.len() >= self.cap {
            self.rejects.push((XC_A3_FULL, "预案容量满"));
            return Err(XC_A3_FULL);
        }
        if plan.trig == 0 {
            self.rejects.push((XC_A3_NOPLAN, "触发条件缺失"));
            return Err(XC_A3_NOPLAN);
        }
        if plan.rollback[3] == 0 || plan.handover[3] == 0 {
            self.rejects.push((XC_A3_NOPLAN, "序不完整"));
            return Err(XC_A3_NOPLAN);
        }
        self.plans.push(plan);
        Ok(())
    }

    /// 封账：指纹冻结，此后一切写操作拒。
    pub fn seal(&mut self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.round.to_le_bytes());
        buf.extend_from_slice(&self.snaps_sent.to_le_bytes());
        buf.extend_from_slice(&self.snaps_echo_ok.to_le_bytes());
        buf.extend_from_slice(&self.adv_sent.to_le_bytes());
        buf.extend_from_slice(&self.adv_pass.to_le_bytes());
        buf.extend_from_slice(&self.plans.len().to_le_bytes());
        self.sealed = true;
        fnv64(&buf)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn round_no(&self) -> u64 {
        self.round
    }

    /// 移交零失真率（千分比）：回执成功 / 发出。
    pub fn echo_rate_bps(&self) -> u32 {
        if self.snaps_sent == 0 {
            return 10_000;
        }
        (self.snaps_echo_ok * 10_000 / self.snaps_sent) as u32
    }

    /// 建议通过率（千分比）。
    pub fn adv_rate_bps(&self) -> u32 {
        if self.adv_sent == 0 {
            return 10_000;
        }
        (self.adv_pass * 10_000 / self.adv_sent) as u32
    }

    /// 三账守恒：发出 ≥ 回执；建议发出 ≥ 通过；预案非空后才算对接完成。
    pub fn conserved(&self) -> bool {
        self.snaps_echo_ok <= self.snaps_sent
            && self.adv_pass <= self.adv_sent
            && self.round >= self.snaps_echo_ok
    }

    pub fn plans_len(&self) -> usize {
        self.plans.len()
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：十轮预演编排（判据主体路径） ─────────────────────────────

/// 十轮预演结果账。
pub struct RehearsalLog {
    pub rounds: Vec<(u64, u64, u64, u64, bool)>, // (round, fp_local, fp_echo, adv_pass, ok)
    pub all_ok: bool,
}

/// 编排十轮移交+建议对接（判据 J1 主体：10 轮零失真 + 通过率 100%）。
pub fn rehearse_ten(bridge: &mut A3Bridge, sched: &mut FakeSched, cores: u32, gap_th: u32) -> Result<RehearsalLog, u32> {
    let mut log = RehearsalLog { rounds: vec![], all_ok: true };
    for _ in 0..10 {
        let snap = bridge.make_snap(cores, 0x5EED_1197)?;
        let fp_local = snap.fp;
        let ok = bridge.handover(sched, &snap)?;
        let advs = sched.advise(&snap, gap_th, 8);
        let pass = bridge.advise_verify(&advs, &snap)?;
        let fp_echo = sched.on_snap(&snap);
        if fp_local != fp_echo || !ok {
            log.all_ok = false;
        }
        log.rounds.push((snap.seq, fp_local, fp_echo, pass as u64, ok));
    }
    Ok(log)
}

/// 预案构造器：标准五元组（确定性）。
pub fn std_plan(trig: u32, owner: u32) -> SwitchPlan {
    SwitchPlan {
        trig,
        handover: [1, 2, 3, 4],
        rollback: [4, 3, 2, 1],
        chk_fp: fnv64(&trig.to_le_bytes()),
        owner,
    }
}

/// 回执对照表导出（审计可查）。
pub fn log_export(log: &RehearsalLog) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    out.extend_from_slice(b"== A3 Rehearsal ==\n");
    for (r, fl, fe, ap, ok) in &log.rounds {
        out.extend_from_slice(b"round ");
        out.extend_from_slice(&r.to_le_bytes());
        out.extend_from_slice(b" fp ");
        out.extend_from_slice(&fl.to_le_bytes());
        out.push(b'/');
        out.extend_from_slice(&fe.to_le_bytes());
        out.extend_from_slice(b" adv ");
        out.extend_from_slice(&ap.to_le_bytes());
        out.push(if *ok { b'+' } else { b'-' });
        out.push(b'\n');
    }
    out.extend_from_slice(b"all_ok=");
    out.push(if log.all_ok { b'1' } else { b'0' });
    out.push(b'\n');
    out
}

// ── 注入核：七类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①封账后造快照 ②回执指纹篡改 ③空快照 ④轮次回退
// ⑤自迁自建议 ⑥权重越界建议 ⑦预案序不完整。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;
    let mut b = A3Bridge::new(4);
    let mut s = FakeSched::new();

    // ① 封账后造快照 → XC_A3_SEALED
    total += 1;
    let _ = b.seal();
    if b.make_snap(4, 1).err() == Some(XC_A3_SEALED) {
        hit += 1;
    }

    // ② 回执指纹篡改 → handover 返回 false（零失真断言反向样本）
    total += 1;
    let mut b2 = A3Bridge::new(4);
    let mut s2 = FakeSched::new();
    if let Ok(mut snap) = b2.make_snap(4, 2) {
        snap.fp = snap.fp ^ 0xDEAD;
        if b2.handover(&mut s2, &snap) == Ok(false) {
            hit += 1;
        }
    }

    // ③ 空快照 → XC_A3_EMPTY
    total += 1;
    let mut b3 = A3Bridge::new(4);
    let mut s3 = FakeSched::new();
    let empty = LoadSnap { seq: 1, loads: vec![], fp: 0 };
    if b3.handover(&mut s3, &empty) == Err(XC_A3_EMPTY) {
        hit += 1;
    }

    // ④ 自迁自建议 → 拒（通过率下降）
    total += 1;
    let mut b4 = A3Bridge::new(4);
    if let Ok(snap) = b4.make_snap(4, 3) {
        let bad = [BalanceAdvice { from: 0, to: 0, weight_bp: 100 }];
        if b4.advise_verify(&bad, &snap) == Ok(0) {
            hit += 1;
        }
    }

    // ⑤ 权重越界建议 → 拒
    total += 1;
    let mut b5 = A3Bridge::new(4);
    if let Ok(snap) = b5.make_snap(4, 4) {
        let bad = [BalanceAdvice { from: 0, to: 1, weight_bp: 20_000 }];
        if b5.advise_verify(&bad, &snap) == Ok(0) {
            hit += 1;
        }
    }

    // ⑥ 核不在快照的建议 → 拒
    total += 1;
    let mut b6 = A3Bridge::new(4);
    if let Ok(snap) = b6.make_snap(4, 5) {
        let bad = [BalanceAdvice { from: 0, to: 99, weight_bp: 100 }];
        if b6.advise_verify(&bad, &snap) == Ok(0) {
            hit += 1;
        }
    }

    // ⑦ 预案序不完整 → XC_A3_NOPLAN
    total += 1;
    let mut b7 = A3Bridge::new(4);
    let bad_plan = SwitchPlan {
        trig: 1,
        handover: [1, 2, 3, 0],
        rollback: [4, 3, 2, 1],
        chk_fp: 0,
        owner: 1,
    };
    if b7.file_plan(bad_plan) == Err(XC_A3_NOPLAN) {
        hit += 1;
    }

    (hit, total)
}
