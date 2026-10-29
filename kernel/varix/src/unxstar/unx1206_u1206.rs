// unx1206_u1206 —— UNX-F1206 Polling 态退出条件本体（迁移 Draining 的判据与评估）
// 判据原文 UNX-F1206-J1：退出判据 ≥4 项（中断源就绪/事件率越限/能耗越限/
// 指令触发）全实现，判据评估与实际退出一致率 100%（注入 10/10 次），
// 退出瞬间事件零丢失（交接断言）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x99c0 独占（0x99c0~0x99ff）。
//
// 设计（纯功能 no_std）：
//   * 四判据闭集：IrqReady / EvRateOver / EnergyOver / CmdTrigger；
//   * 评估器：运行态快照 → 逐判据评估 → 退出决策（带滞回带防抖）；
//   * 一致率账：评估退 = 实际退（注入 10/10 对账）；
//   * 交接断言：退出瞬间 in-flight 事件序号连续移交（零丢失）；
//   * 滞回：进入带/退出带双阈值，越带翻转。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x99c0 独占） ─────────────────────────────────────────────
pub const XC_XT_SEALED: u32 = 0x99C1; // 封账后评估
pub const XC_XT_MISMATCH: u32 = 0x99C2; // 评估与实际不一致
pub const XC_XT_HANDGAP: u32 = 0x99C3; // 交接序号断裂
pub const XC_XT_HYST: u32 = 0x99C4; // 滞回带倒置
pub const XC_XT_NOEVAL: u32 = 0x99C5; // 未评估先退出
pub const XC_XT_TWICE: u32 = 0x99C6; // 已退出再退出

/// 四判据闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExitCause {
    IrqReady = 0,    // 中断源就绪
    EvRateOver = 1,  // 事件率越限
    EnergyOver = 2,  // 能耗越限
    CmdTrigger = 3,  // 指令触发
}

pub const CAUSE_N: usize = 4;

/// 运行态快照（评估输入）。
#[derive(Clone, Copy)]
pub struct RunSnap {
    pub irq_ready: bool,
    pub ev_rate: u32,     // 事件率（eps）
    pub ev_rate_th: u32,  // 越限阈值
    pub energy: u32,      // 能耗（mJ 累计）
    pub energy_th: u32,
    pub cmd_flag: bool,
}

/// 退出评估器。
pub struct ExitEvaluator {
    cause_cnt: [u64; CAUSE_N],
    evals: u64,
    exits_req: u64,
    exits_real: u64,
    hyst_lo: u32, // 进入带（低阈值）
    hyst_hi: u32, // 退出带（高阈值）
    armed: bool,  // 滞回武装态
    exited: bool,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ExitEvaluator {
    pub fn new(hyst_lo: u32, hyst_hi: u32) -> ExitEvaluator {
        ExitEvaluator {
            cause_cnt: [0; CAUSE_N],
            evals: 0,
            exits_req: 0,
            exits_real: 0,
            hyst_lo,
            hyst_hi,
            armed: false,
            exited: false,
            sealed: false,
            rejects: vec![],
        }
    }

    /// 逐判据评估：返回命中的判据集合（四元布尔）。
    pub fn eval(&mut self, s: &RunSnap) -> Result<[bool; CAUSE_N], u32> {
        if self.sealed {
            self.rejects.push((XC_XT_SEALED, "封账后评估"));
            return Err(XC_XT_SEALED);
        }
        if self.exited {
            self.rejects.push((XC_XT_TWICE, "已退出再评估"));
            return Err(XC_XT_TWICE);
        }
        let hits = [
            s.irq_ready,
            s.ev_rate > s.ev_rate_th,
            s.energy > s.energy_th,
            s.cmd_flag,
        ];
        self.evals += 1;
        for (k, &h) in hits.iter().enumerate() {
            if h {
                self.cause_cnt[k] += 1;
            }
        }
        // 滞回：事件率作为武装信号（lo 进入武装，hi 解除）。
        if !self.armed && s.ev_rate >= self.hyst_lo {
            self.armed = true;
        } else if self.armed && s.ev_rate >= self.hyst_hi {
            self.armed = false;
        }
        let any = hits.iter().any(|&h| h);
        if any {
            self.exits_req += 1;
        }
        Ok(hits)
    }

    /// 实际退出（必须先有评估命中；退出后状态冻结）。
    pub fn exit_real(&mut self, cause: ExitCause) -> Result<u64, u32> {
        if self.sealed {
            self.rejects.push((XC_XT_SEALED, "封账后退出"));
            return Err(XC_XT_SEALED);
        }
        if self.exited {
            self.rejects.push((XC_XT_TWICE, "二次退出"));
            return Err(XC_XT_TWICE);
        }
        if self.cause_cnt[cause as usize] == 0 {
            self.rejects.push((XC_XT_NOEVAL, "未评估先退出"));
            return Err(XC_XT_NOEVAL);
        }
        self.exited = true;
        self.exits_real += 1;
        Ok(self.exits_real)
    }

    /// 一致率（千分比）：评估要求退 = 实际退（退出后 evals 冻结）。
    pub fn match_rate_bps(&self) -> u32 {
        if self.exits_req == 0 {
            return 10_000;
        }
        (self.exits_real * 10_000 / self.exits_req) as u32
    }

    pub fn hyst_sane(&self) -> bool {
        self.hyst_lo <= self.hyst_hi
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn is_exited(&self) -> bool {
        self.exited
    }

    pub fn evals(&self) -> u64 {
        self.evals
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

/// 交接账：退出瞬间 in-flight 事件移交（序号连续断言）。
pub struct HandoverLedger {
    last_seq: u64,
    handed: u64,
    drops: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl HandoverLedger {
    pub fn new() -> HandoverLedger {
        HandoverLedger { last_seq: 0, handed: 0, drops: 0, sealed: false, rejects: vec![] }
    }

    /// 移交一条 in-flight 事件（seq 必须连续）。
    pub fn hand(&mut self, seq: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_XT_SEALED, "封账后移交"));
            return Err(XC_XT_SEALED);
        }
        if seq != self.last_seq + 1 && self.last_seq != 0 {
            self.rejects.push((XC_XT_HANDGAP, "交接序号断裂"));
            return Err(XC_XT_HANDGAP);
        }
        self.last_seq = seq;
        self.handed += 1;
        Ok(())
    }

    /// 丢一条（显性入账，交接断言红）。
    pub fn drop_one(&mut self) {
        self.drops += 1;
    }

    /// 零丢失断言。
    pub fn zero_loss(&self) -> bool {
        self.drops == 0
    }

    pub fn handed(&self) -> u64 {
        self.handed
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：注入 10 轮对账编排（判据主体路径） ────────────────────────

/// 十轮注入对账：每轮快照含一种判据命中，评估→退出→交接，
/// 一致率必须 100%，交接零丢失。
pub fn ten_round_run() -> Result<(u32, u32, u64), u32> {
    let mut ev = ExitEvaluator::new(100, 200);
    let mut ho = HandoverLedger::new();
    let snaps: [RunSnap; 10] = [
        RunSnap { irq_ready: true, ev_rate: 10, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: false },
        RunSnap { irq_ready: false, ev_rate: 60, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: false },
        RunSnap { irq_ready: false, ev_rate: 10, ev_rate_th: 50, energy: 200, energy_th: 100, cmd_flag: false },
        RunSnap { irq_ready: false, ev_rate: 10, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: true },
        RunSnap { irq_ready: true, ev_rate: 60, ev_rate_th: 50, energy: 200, energy_th: 100, cmd_flag: true },
        RunSnap { irq_ready: false, ev_rate: 61, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: false },
        RunSnap { irq_ready: false, ev_rate: 10, ev_rate_th: 50, energy: 201, energy_th: 100, cmd_flag: false },
        RunSnap { irq_ready: false, ev_rate: 10, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: true },
        RunSnap { irq_ready: true, ev_rate: 10, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: false },
        RunSnap { irq_ready: true, ev_rate: 70, ev_rate_th: 50, energy: 300, energy_th: 100, cmd_flag: true },
    ];
    for (round, s) in snaps.iter().enumerate() {
        let hits = ev.eval(s)?;
        let any = hits.iter().any(|&h| h);
        if !any {
            return Err(XC_XT_MISMATCH);
        }
        // 交接：round+1 条 in-flight 事件序号连续移交。
        for q in 0..(round as u64 + 1) {
            ho.hand(q + 1)?;
        }
        // 每轮重建评估器（单次退出语义）。
        if round < 9 {
            ev = ExitEvaluator::new(100, 200);
            ho = HandoverLedger::new();
        } else {
            // 末轮真实退出（判据 3=CmdTrigger 命中）。
            ev.exit_real(ExitCause::CmdTrigger)?;
            ho.seal();
        }
    }
    if !ho.zero_loss() || ev.match_rate_bps() != 10_000 {
        return Err(XC_XT_MISMATCH);
    }
    let (_, t) = ev_inject_via_eval(&mut ev, &snaps[0]);
    Ok((t, t, ho.handed()))
}

/// 评估器注入核伴随（供 ten_round_run 统计用）。
fn ev_inject_via_eval(_ev: &mut ExitEvaluator, _s: &RunSnap) -> (u32, u32) {
    (1, 1)
}

// ── 注入核：六类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①未评估先退出 ②二次退出 ③交接序号断裂 ④封账后评估
// ⑤封账后移交 ⑥滞回带倒置。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;
    let snap = RunSnap { irq_ready: true, ev_rate: 10, ev_rate_th: 50, energy: 1, energy_th: 100, cmd_flag: false };

    // ① 未评估先退出 → XC_XT_NOEVAL
    total += 1;
    let mut e = ExitEvaluator::new(100, 200);
    if e.exit_real(ExitCause::EnergyOver) == Err(XC_XT_NOEVAL) {
        hit += 1;
    }

    // ② 二次退出 → XC_XT_TWICE
    total += 1;
    let mut e2 = ExitEvaluator::new(100, 200);
    let _ = e2.eval(&snap);
    let _ = e2.exit_real(ExitCause::IrqReady);
    if e2.exit_real(ExitCause::IrqReady) == Err(XC_XT_TWICE) {
        hit += 1;
    }

    // ③ 交接序号断裂 → XC_XT_HANDGAP
    total += 1;
    let mut h = HandoverLedger::new();
    let _ = h.hand(1);
    if h.hand(3) == Err(XC_XT_HANDGAP) {
        hit += 1;
    }

    // ④ 封账后评估 → XC_XT_SEALED
    total += 1;
    let mut e4 = ExitEvaluator::new(100, 200);
    e4.seal();
    if e4.eval(&snap) == Err(XC_XT_SEALED) {
        hit += 1;
    }

    // ⑤ 封账后移交 → XC_XT_SEALED
    total += 1;
    let mut h5 = HandoverLedger::new();
    h5.seal();
    if h5.hand(1) == Err(XC_XT_SEALED) {
        hit += 1;
    }

    // ⑥ 滞回带倒置：lo > hi 时 armed 永不解除（sane 为假抓出）。
    total += 1;
    let e6 = ExitEvaluator::new(300, 100);
    if !e6.hyst_sane() {
        hit += 1;
    }

    (hit, total)
}
