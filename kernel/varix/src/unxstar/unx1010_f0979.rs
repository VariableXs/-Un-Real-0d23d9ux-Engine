// unx1010_f0979 —— UNX-F1010 计时面启动序列（时钟源初始化序与 F0979 衔接消费）
// 判据原文 UNX-F1010-J1：初始化序正确（探测→校准→单调面→死表检测四段依赖序
// 10/10 次），衔接断言（F0979 挂点消费正确），启动耗时 ≤30ms 断言。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1AA0 独占（0x1AA0~0x1ADF）。
//
// 设计（纯功能 no_std）：
//   * 四段依赖序闭集：Probe→Calibrate→Ktime→DeadDetect，越序即立案；
//   * 状态机：S0 未启→S1 探测→S2 校准→S3 单调→S4 死表→S5 就绪（单向）；
//   * F0979 挂点消费：每段完成时向 F0979 启动账投递一次（对冲计数）；
//   * 耗时账：各段 ms 记账，总耗 ≤30ms 预算闸（saturating 累加）；
//   * 挂点缺失拒：F0979 未挂即启动拒（衔接断言硬闸）；
//   * 守恒：段完成数=投递数；FNV 指纹冻结；注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1AA0 独占） ─────────────────────────────────────────────
pub const XC_SEQ_ORDER: u32 = 0x1AA1; // 段序越位（依赖序破）
pub const XC_SEQ_NOHOOK: u32 = 0x1AA2; // F0979 挂点未挂即启
pub const XC_SEQ_BUDGET: u32 = 0x1AA3; // 启动耗时超 30ms
pub const XC_SEQ_TWICE: u32 = 0x1AA4; // 段重复执行
pub const XC_SEQ_SEALED: u32 = 0x1AA5; // 收口后再动账
pub const XC_SEQ_LOST: u32 = 0x1AA6; // 段完成与投递不守恒

/// 四段闭集 id。
pub const ST_PROBE: u32 = 0;
pub const ST_CALIB: u32 = 1;
pub const ST_KTIME: u32 = 2;
pub const ST_DEAD: u32 = 3;
pub const N_STAGES: usize = 4;

/// 启动耗时预算（ms）。
pub const BUDGET_MS: u32 = 30;

/// 一段耗时账。
#[derive(Clone, Copy)]
pub struct StageCost {
    pub stage: u32,
    pub ms: u32,
}

/// 启动序列账本。
pub struct BootSeq {
    hooked: bool,        // F0979 挂点在位
    stages_done: Vec<u32>,
    costs: Vec<StageCost>,
    hook_sends: u32,
    total_ms: u32,
    ready: bool,
    sealed: bool,
    violations: Vec<(u32, &'static str)>,
}

impl BootSeq {
    pub fn new() -> BootSeq {
        BootSeq {
            hooked: false,
            stages_done: Vec::new(),
            costs: Vec::new(),
            hook_sends: 0,
            total_ms: 0,
            ready: false,
            sealed: false,
            violations: Vec::new(),
        }
    }

    /// 挂 F0979 挂点（启动前置硬闸）。
    pub fn hook_f0979(&mut self) {
        self.hooked = true;
    }

    pub fn hooked(&self) -> bool {
        self.hooked
    }

    /// 段完成上报（依赖序硬闸：必须严格按 0→1→2→3 一次一推进）。
    pub fn complete_stage(&mut self, stage: u32, ms: u32) -> Result<(), u32> {
        if self.sealed {
            self.violations.push((XC_SEQ_SEALED, "收口后动账"));
            return Err(XC_SEQ_SEALED);
        }
        if !self.hooked {
            self.violations.push((XC_SEQ_NOHOOK, "挂点未挂"));
            return Err(XC_SEQ_NOHOOK);
        }
        if stage >= N_STAGES as u32 {
            self.violations.push((XC_SEQ_ORDER, "段号越界"));
            return Err(XC_SEQ_ORDER);
        }
        if self.ready {
            self.violations.push((XC_SEQ_TWICE, "已就绪再上报"));
            return Err(XC_SEQ_TWICE);
        }
        // 依赖序：stage 必须恰好等于已完成数（严格顺序）
        if stage != self.stages_done.len() as u32 {
            self.violations.push((XC_SEQ_ORDER, "依赖序越位"));
            return Err(XC_SEQ_ORDER);
        }
        self.stages_done.push(stage);
        self.costs.push(StageCost { stage, ms });
        self.total_ms = self.total_ms.saturating_add(ms);
        // F0979 挂点消费：每段投递一次
        self.hook_sends = self.hook_sends.wrapping_add(1);
        // 四段全齐 → 就绪判定（含预算闸）
        if self.stages_done.len() == N_STAGES {
            if self.total_ms > BUDGET_MS {
                self.violations.push((XC_SEQ_BUDGET, "耗时超 30ms"));
                // 超预算不就绪（显性失败，不假绿）
                return Err(XC_SEQ_BUDGET);
            }
            self.ready = true;
        }
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// 守恒：段完成数 = 投递数（一次一投）。
    pub fn conserved(&self) -> bool {
        self.stages_done.len() as u32 == self.hook_sends
    }

    /// 依赖序复核（账面重放：0,1,2,3 精确序列）。
    pub fn order_ok(&self) -> bool {
        if self.stages_done.len() != N_STAGES {
            return false;
        }
        self.stages_done
            .iter()
            .enumerate()
            .all(|(i, &s)| s == i as u32)
    }

    /// 耗时分解（逐段可解引）。
    pub fn stage_ms(&self, stage: u32) -> u32 {
        for c in self.costs.iter() {
            if c.stage == stage {
                return c.ms;
            }
        }
        0
    }

    pub fn total_ms(&self) -> u32 {
        self.total_ms
    }
    pub fn budget_ok(&self) -> bool {
        self.total_ms <= BUDGET_MS
    }
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SEQ_SEALED);
        }
        if !self.conserved() {
            return Err(XC_SEQ_LOST);
        }
        if !self.ready {
            self.violations.push((XC_SEQ_ORDER, "未就绪收口"));
            return Err(XC_SEQ_ORDER);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹：段序+耗时+投递全入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.hook_sends;
        h = h.wrapping_mul(0x0100_0193);
        for c in self.costs.iter() {
            h ^= c.stage ^ c.ms.wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.total_ms;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.stages_done.len() as u64 & 0xFFFF)
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）越序必抓（跳段/倒段十例）
    {
        let mut hits = 0;
        for bad in [2u32, 3, 1, 5, 9, 2, 3, 1, 7, 4].iter() {
            let mut b = BootSeq::new();
            b.hook_f0979();
            if b.complete_stage(*bad, 1) == Err(XC_SEQ_ORDER) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）挂点未挂必拒（10/10）
    {
        let mut hits = 0;
        for _ in 0..10 {
            let mut b = BootSeq::new();
            if b.complete_stage(ST_PROBE, 1) == Err(XC_SEQ_NOHOOK) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）超预算必拒（总耗 >30ms 十例）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut b = BootSeq::new();
            b.hook_f0979();
            let _ = b.complete_stage(ST_PROBE, 1);
            let _ = b.complete_stage(ST_CALIB, 2);
            let _ = b.complete_stage(ST_KTIME, 3);
            let heavy = BUDGET_MS + 1 + k; // 第四段直接顶爆预算
            if b.complete_stage(ST_DEAD, heavy) == Err(XC_SEQ_BUDGET) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）就绪后重复上报必拒
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let _ = b.complete_stage(ST_PROBE, 2);
        let _ = b.complete_stage(ST_CALIB, 2);
        let _ = b.complete_stage(ST_KTIME, 2);
        let _ = b.complete_stage(ST_DEAD, 2);
        if b.complete_stage(ST_DEAD, 2) == Err(XC_SEQ_TWICE) {
            caught += 1;
        }
    }
    // 5）收口后再动账必拒
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        for s in 0..4u32 {
            let _ = b.complete_stage(s, 2);
        }
        let _ = b.seal();
        if b.complete_stage(ST_DEAD, 1) == Err(XC_SEQ_SEALED) {
            caught += 1;
        }
    }
    // 6）未就绪收口必拒
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let _ = b.complete_stage(ST_PROBE, 2);
        if b.seal() == Err(XC_SEQ_ORDER) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1010-J1。
pub fn run_unx1010_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1010");

    // C10-01 四段依赖序 10/10（十轮全序重放）
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut b = BootSeq::new();
            b.hook_f0979();
            for s in 0..4u32 {
                if b.complete_stage(s, 2).is_err() {
                    ok = false;
                    break;
                }
            }
            if !b.is_ready() || !b.order_ok() {
                ok = false;
                break;
            }
        }
        s.add("C10-01-四段序十连", ok, if ok { "" } else { "依赖序破" });
    }
    // C10-02 越序拒绝（跳段显性）
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let a = b.complete_stage(ST_KTIME, 1) == Err(XC_SEQ_ORDER);
        let _ = b.complete_stage(ST_PROBE, 1);
        let c = b.complete_stage(ST_DEAD, 1) == Err(XC_SEQ_ORDER);
        s.add("C10-02-越序拒", a && c, if a && c { "" } else { "越序漏抓" });
    }
    // C10-03 F0979 挂点消费正确（四段四投递）
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        for s in 0..4u32 {
            let _ = b.complete_stage(s, 1);
        }
        let ok = b.hooked() && b.conserved() && b.hook_sends == 4;
        s.add("C10-03-挂点消费", ok, if ok { "" } else { "挂点缺" });
    }
    // C10-04 挂点未挂拒启
    {
        let mut b = BootSeq::new();
        let ok = b.complete_stage(ST_PROBE, 1) == Err(XC_SEQ_NOHOOK) && !b.is_ready();
        s.add("C10-04-挂点硬闸", ok, if ok { "" } else { "硬闸漏" });
    }
    // C10-05 启动耗时 ≤30ms（典型分解 8+7+9+5=29）
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let _ = b.complete_stage(ST_PROBE, 8);
        let _ = b.complete_stage(ST_CALIB, 7);
        let _ = b.complete_stage(ST_KTIME, 9);
        let _ = b.complete_stage(ST_DEAD, 5);
        let ok = b.is_ready() && b.budget_ok() && b.total_ms() == 29;
        s.add("C10-05-耗时预算", ok, if ok { "" } else { "预算破" });
    }
    // C10-06 逐段可解引
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let _ = b.complete_stage(ST_PROBE, 8);
        let _ = b.complete_stage(ST_CALIB, 7);
        let _ = b.complete_stage(ST_KTIME, 9);
        let _ = b.complete_stage(ST_DEAD, 5);
        let ok = b.stage_ms(0) == 8 && b.stage_ms(1) == 7 && b.stage_ms(2) == 9 && b.stage_ms(3) == 5 && b.stage_ms(9) == 0;
        s.add("C10-06-逐段解引", ok, if ok { "" } else { "解引漂" });
    }
    // C10-07 超预算不就绪（假绿拦截）
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let _ = b.complete_stage(ST_PROBE, 20);
        let _ = b.complete_stage(ST_CALIB, 20);
        let _ = b.complete_stage(ST_KTIME, 20);
        let r = b.complete_stage(ST_DEAD, 20);
        let ok = r == Err(XC_SEQ_BUDGET) && !b.is_ready() && b.violation_count() >= 1;
        s.add("C10-07-假绿拦截", ok, if ok { "" } else { "假绿" });
    }
    // C10-08 收口冻结
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        for s in 0..4u32 {
            let _ = b.complete_stage(s, 3);
        }
        let fp = b.seal();
        let ok = fp.is_ok() && b.seal() == Err(XC_SEQ_SEALED) && b.is_sealed();
        s.add("C10-08-收口冻结", ok, if ok { "" } else { "收口失守" });
    }
    // C10-09 守恒（段=投递）
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        for s in 0..4u32 {
            let _ = b.complete_stage(s, 1);
        }
        s.add("C10-09-守恒", b.conserved(), if b.conserved() { "" } else { "守恒破" });
    }
    // C10-10 指纹往返
    {
        let build = || {
            let mut b = BootSeq::new();
            b.hook_f0979();
            let _ = b.complete_stage(ST_PROBE, 5);
            let _ = b.complete_stage(ST_CALIB, 6);
            let _ = b.complete_stage(ST_KTIME, 7);
            let _ = b.complete_stage(ST_DEAD, 8);
            b.fingerprint()
        };
        let ok = build() == build();
        s.add("C10-10-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C10-11 段号越界拒
    {
        let mut b = BootSeq::new();
        b.hook_f0979();
        let ok = b.complete_stage(4, 1) == Err(XC_SEQ_ORDER) && b.complete_stage(99, 1) == Err(XC_SEQ_ORDER);
        s.add("C10-11-段号界", ok, if ok { "" } else { "界判漏" });
    }
    // C10-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C10-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
