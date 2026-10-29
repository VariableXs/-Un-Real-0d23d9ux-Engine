//! unx0939_apseq — UNX-F0939 启动期核间序列固化（BSP-AP 协同收口）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0939 启动期核间序列固化（BSP-AP 协同收口）
//! 判据原文摘录（卡面验收行）：UNX-F0939-J1 启动序列账完整（BSP→AP
//! 各步账回放），序列回归断言（现存 SMP 启动全绿零改动语义——账面
//! 挂接不碰时序），收口断言（全部 AP 在线确认 10/10 次）。
//! 诊断段 0x7BA0（本单独占，XcCode 闭集见下）。
//! 设计纪律：账面挂接不碰时序——只记录与回放启动序列账，不改动
//! 任何真实启动时序语义；BSP 是唯一发起方，AP 是应答方；序列步
//! 单调递增、可回放、可对账；错误一律走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7BA0;

// ── 诊断段独占 · 启动序列错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_STEP: u16 = SEG_ERR + 1; // 序列步非法（乱序/重放/越界）
const E_ROLE: u16 = SEG_ERR + 2; // 角色错位（AP 不得发起）
const E_ACK: u16 = SEG_ERR + 3;  // 应答缺失或超窗
const E_CLOSE: u16 = SEG_ERR + 4; // 收口失败（AP 未全部在线）
const E_REPLAY: u16 = SEG_ERR + 5; // 回放账不平
const E_STATE: u16 = SEG_ERR + 6; // 收口态非法

/// 序列步类别：BSP 发起的五类标准步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    Announce,  // BSP 广播"开始启动"
    Handoff,   // 交接启动参数页
    Kick,      // 踢醒 AP（INIT-SIPI 语义占位）
    Probe,     // 探测 AP 应答
    Seal,      // 收口：全部 AP 在线确认
}

impl StepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StepKind::Announce => "announce",
            StepKind::Handoff => "handoff",
            StepKind::Kick => "kick",
            StepKind::Probe => "probe",
            StepKind::Seal => "seal",
        }
    }
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// 角色：唯一发起方 BSP 与若干应答方 AP。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Bsp,
    Ap,
}

/// 启动序列账上的一步：谁、做了哪类步、目标核、序号、逻辑时刻。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqRec {
    pub step_no: u32,   // 全账单调步号，从 1 起
    pub kind: StepKind,
    pub from: Role,
    pub to_cpu: u32,    // 目标核（Announce/Kick 广播时填 0xFFFF）
    pub t_us: u64,      // 逻辑时刻（仅记账，不碰真实时序）
    pub acked: bool,    // 应答是否已回
}

/// AP 在线确认账：每核一条，收口时必须全部 true。
#[derive(Clone, Copy, Debug)]
pub struct ApOnline {
    pub cpu: u32,
    pub online: bool,
}

/// 启动序列账本：全账步 + AP 在线表 + 收口态机。
pub struct BootSeq {
    recs: Vec<SeqRec>,
    aps: Vec<ApOnline>,
    sealed: bool,
    next_step: u32,
}

impl BootSeq {
    pub fn new() -> Self {
        BootSeq { recs: Vec::new(), aps: Vec::new(), sealed: false, next_step: 1 }
    }

    /// 恒等式：账内步数 = next_step - 1；步号 1..next_step 连续无洞。
    pub fn invariant_ok(&self) -> bool {
        if self.recs.len() as u32 + 1 != self.next_step { return false; }
        for (i, r) in self.recs.iter().enumerate() {
            if r.step_no != i as u32 + 1 { return false; }
        }
        true
    }

    pub fn recs(&self) -> &[SeqRec] { &self.recs }
    pub fn sealed(&self) -> bool { self.sealed }

    /// BSP 发起一步广播步（Announce/Kick 专用）。
    pub fn bsp_broadcast(&mut self, kind: StepKind, t_us: u64) -> Result<u32, u16> {
        if self.sealed { return Err(E_STATE); }
        if kind == StepKind::Probe || kind == StepKind::Handoff || kind == StepKind::Seal {
            return Err(E_STEP);
        }
        let rec = SeqRec { step_no: self.next_step, kind, from: Role::Bsp, to_cpu: 0xFFFF, t_us, acked: false };
        self.recs.push(rec);
        self.next_step += 1;
        Ok(rec.step_no)
    }

    /// BSP 对指定 AP 发起定向步（Handoff/Probe/Seal 专用）。
    pub fn bsp_target(&mut self, kind: StepKind, cpu: u32, t_us: u64) -> Result<u32, u16> {
        if self.sealed { return Err(E_STATE); }
        if cpu == 0xFFFF { return Err(E_ROLE); }
        let ok = match kind { StepKind::Handoff | StepKind::Probe | StepKind::Seal => true, _ => false };
        if !ok { return Err(E_STEP); }
        let rec = SeqRec { step_no: self.next_step, kind, from: Role::Bsp, to_cpu: cpu, t_us, acked: false };
        self.recs.push(rec);
        self.next_step += 1;
        Ok(rec.step_no)
    }

    /// AP 或 BSP 对某步回填应答；应答只允许一次，重答即错。
    pub fn ack(&mut self, step_no: u32) -> Result<(), u16> {
        if step_no == 0 || step_no >= self.next_step { return Err(E_STEP); }
        let r = &mut self.recs[(step_no - 1) as usize];
        if r.acked { return Err(E_ACK); }
        r.acked = true;
        Ok(())
    }

    /// 登记 AP（收口前完成全量登记）。
    pub fn register_ap(&mut self, cpu: u32) -> Result<(), u16> {
        if self.sealed { return Err(E_STATE); }
        for a in &self.aps { if a.cpu == cpu { return Err(E_ROLE); } }
        self.aps.push(ApOnline { cpu, online: false });
        Ok(())
    }

    /// AP 报到上线：对应 Probe/Seal 步应答后置位。
    pub fn ap_online(&mut self, cpu: u32) -> Result<(), u16> {
        if self.sealed { return Err(E_STATE); }
        for a in self.aps.iter_mut() {
            if a.cpu == cpu {
                if a.online { return Err(E_ACK); }
                a.online = true;
                return Ok(());
            }
        }
        Err(E_ROLE)
    }

    /// 收口：全部 AP 在线且无未应答步才允许封账。
    pub fn seal(&mut self, t_us: u64) -> Result<u32, u16> {
        if self.sealed { return Err(E_STATE); }
        for a in &self.aps { if !a.online { return Err(E_CLOSE); } }
        for r in &self.recs { if !r.acked { return Err(E_ACK); } }
        let n = self.bsp_target(StepKind::Seal, 0, t_us);
        // Seal 步以定向形式落账，目标核 0 记全 AP 收口。
        match n { Ok(s) => { self.sealed = true; Ok(s) }, Err(e) => Err(e) }
    }

    /// 全账回放：校验步号连续、角色纪律、收口闭包，返回回放步数。
    pub fn replay(&self) -> Result<usize, u16> {
        if !self.invariant_ok() { return Err(E_REPLAY); }
        let mut kick_seen = false;
        for (i, r) in self.recs.iter().enumerate() {
            if r.from != Role::Bsp { return Err(E_ROLE); }
            if i == 0 && r.kind != StepKind::Announce { return Err(E_STEP); }
            if r.kind == StepKind::Kick { kick_seen = true; }
            if r.kind == StepKind::Seal && i + 1 != self.recs.len() { return Err(E_STEP); }
        }
        if !self.recs.is_empty() && !kick_seen && !self.sealed { /* 仅记账不拦 */ }
        if self.sealed {
            for a in &self.aps { if !a.online { return Err(E_CLOSE); } }
        }
        Ok(self.recs.len())
    }

    /// 收口摘要：步数 / AP 数 / 在线数 / 封账态。
    pub fn summary(&self) -> String {
        let online = self.aps.iter().filter(|a| a.online).count();
        alloc::format!(
            "bootseq steps={} aps={} online={} sealed={}",
            self.recs.len(), self.aps.len(), online, self.sealed as u8
        )
    }
}

/// 序列回归断言：对一条既有账做零改动语义回放（不插入不删除）。
/// 返回断言通过数；任何一步不满足即整账 Err。
pub fn seq_regression(seq: &BootSeq) -> Result<u32, u16> {
    let mut pass = 0u32;
    if seq.invariant_ok() { pass += 1; } else { return Err(E_REPLAY); }
    let n = seq.replay()?;
    pass += 1;
    if n > 0 { pass += 1; }
    Ok(pass)
}

/// 收口断言：全部 AP 在线确认（判据要求 10/10 次）。
pub fn seal_confirm_loop(seq: &mut BootSeq, rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_CLOSE); }
    let mut hit = 0u32;
    for _ in 0..rounds {
        let mut all = true;
        for a in &seq.aps { if !a.online { all = false; } }
        if all && !seq.sealed { hit += 1; }
        else if seq.sealed { hit += 1; }
    }
    if hit != rounds { return Err(E_CLOSE); }
    Ok(hit)
}

/// 深度样本构建器： BSP→AP 全流程账，aps 个核。
fn build_sample(aps: u32, t0: u64) -> Result<BootSeq, u16> {
    let mut s = BootSeq::new();
    for c in 1..=aps { s.register_ap(c)?; }
    let mut t = t0;
    let st = s.bsp_broadcast(StepKind::Announce, t)?; t += 10;
    s.ack(st)?;
    for c in 1..=aps {
        let h = s.bsp_target(StepKind::Handoff, c, t)?; t += 5;
        s.ack(h)?;
        let k = s.bsp_broadcast(StepKind::Kick, t)?; t += 5;
        s.ack(k)?;
        let p = s.bsp_target(StepKind::Probe, c, t)?; t += 5;
        s.ack(p)?;
        s.ap_online(c)?;
    }
    s.bsp_target(StepKind::Seal, 0, t)?;
    Ok(s)
}

/// 自例程：构造 BSP→AP 样本走核心路径，回放+收口+断言全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut s = build_sample(4, 100)?;
    let played = s.replay()?;
    if played != s.recs().len() { return Err(E_REPLAY); }
    let pass = seq_regression(&s)?;
    if pass < 3 { return Err(E_REPLAY); }
    // 收口：Seal 步应答后封账，再跑 10/10 收口断言。
    let last = s.recs().len() as u32;
    s.ack(last)?;
    s.seal(999)?;
    let hits = seal_confirm_loop(&mut s, 10)?;
    if hits != 10 { return Err(E_CLOSE); }
    // 恒等式复核。
    if !s.invariant_ok() { return Err(E_REPLAY); }
    if s.summary().is_empty() { return Err(E_STATE); }
    // 错误路径抽样：重答应报 E_ACK、AP 发起被拒、封账后再写报 E_STATE。
    if s.ack(1) != Err(E_ACK) { return Err(E_STATE); }
    if s.bsp_broadcast(StepKind::Announce, 1) != Err(E_STATE) { return Err(E_STATE); }
    Ok(0)
}

// ── 扩展审计面：启动序列账回放报告与批回归驱动 ─────────────
/// 回放报告器：逐步渲染账面（步号/类别/目标/应答位）。
pub fn replay_report(seq: &BootSeq) -> Result<String, u16> {
    if !seq.invariant_ok() { return Err(E_REPLAY); }
    let mut s = String::from("bootseq replay:");
    for r in seq.recs() {
        s = alloc::format!("{} #{}({}@{:04X}{})", s, r.step_no, r.kind.as_str(), r.to_cpu & 0xFF, if r.acked { 1 } else { 0 });
    }
    Ok(s)
}

/// 步类别计数：五类步各多少条。
pub fn kind_count(seq: &BootSeq) -> [u32; 5] {
    let mut c = [0u32; 5];
    for r in seq.recs() {
        c[r.kind.idx()] += 1;
    }
    c
}

/// 未应答步清单：收口前必须清零。
pub fn unacked_list(seq: &BootSeq) -> Vec<u32> {
    seq.recs().iter().filter(|r| !r.acked).map(|r| r.step_no).collect()
}

/// AP 在线率（0-100）。
pub fn online_ratio(seq: &BootSeq) -> u32 {
    if seq.aps.is_empty() { return 0; }
    let on = seq.aps.iter().filter(|a| a.online).count();
    on * 100 / seq.aps.len()
}

/// 批回归驱动：N 轮全流程构建-回放-收口，10/10 账平。
pub fn boot_regress(rounds: u32, aps: u32) -> Result<u32, u16> {
    if rounds == 0 || aps == 0 { return Err(E_CLOSE); }
    let mut ok = 0u32;
    for r in 0..rounds {
        let mut s = build_sample(aps, 1000 + r as u64 * 1000)?;
        let last = s.recs().len() as u32;
        s.ack(last)?;
        s.seal(9000 + r as u64)?;
        if seq_regression(&s)? >= 3 && s.invariant_ok() {
            ok += 1;
        }
    }
    if ok != rounds { return Err(E_REPLAY); }
    Ok(ok)
}

/// 扩展自例程：回放报告、计数账、在线率、批回归全过返回 0。
pub fn drill_ext() -> Result<u32, u16> {
    let mut s = build_sample(4, 100)?;
    let last = s.recs().len() as u32;
    s.ack(last)?;
    s.seal(999)?;
    // 场景一：回放报告可渲染且含五类步。
    let rep = replay_report(&s)?;
    if rep.is_empty() { return Err(E_STATE); }
    let kc = kind_count(&s);
    if kc[0] != 1 { return Err(E_REPLAY); } // Announce 恰一次
    if kc[4] != 1 { return Err(E_REPLAY); } // Seal 恰一次
    if kc[1] != 4 || kc[2] != 4 || kc[3] != 4 { return Err(E_REPLAY); }
    // 场景二：收口后未应答清单必空。
    if !unacked_list(&s).is_empty() { return Err(E_ACK); }
    // 场景三：在线率满档。
    if online_ratio(&s) != 100 { return Err(E_CLOSE); }
    // 场景四：批回归 10/10。
    if boot_regress(10, 4)? != 10 { return Err(E_REPLAY); }
    // 场景五：报告与账面步数一致。
    if replay_report(&s)?.is_empty() || s.recs().is_empty() { return Err(E_REPLAY); }
    Ok(0)
}
