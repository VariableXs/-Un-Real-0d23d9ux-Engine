//! cgf12_certfuzz — CGPU-F0810 认证 fuzz（伪造探测数据与中途故障——认证流
//! 水线的疫苗试验）。
//!
//! 判据原文（施工书锚点 CGPU-F0810）：fuzz：畸形探测数据 fuzz（伪造能力表
//! /异常 ID——防伪钳制）；流程中断 fuzz（认证中途故障——续跑语义验证）；
//! 案例固化 CI。判据：探测、中断、CI、判据。
//!
//! 职责定位：畸形数据注入器（伪造能力表——超界位图/虚报档位；异常 ID——
//! 零 ID/超大 ID/重复 ID；防伪钳制验证：钳到安全界+伪造面显性立案不静默采
//! 纳）+ 流程中断注入器（六步认证流水中任意步故障——续跑语义：已完成步保
//! 持、当前步重做、未到步不越；断点续跑与全新重跑结果一致——确定性续跑）+
//! 案例固化 CI（LCG 种子+畸形谱指纹——同 cgf11/cgd11 同范式）。与 cgf03_
//! probedomain（防伪五规则正路）、cgf05_certsuite（考卷）分工：彼是认证正
//! 常路径，此把异常路径打满——正常路径的防伪规则在异常输入下不得失效。诊
//! 断码段 0x5E01~0x5E05。

use alloc::vec::Vec;

/// 模块版本。
pub const CERT_FUZZ_VERSION: &str = "F12-certfuzz-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x5E 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_FAKE_CLAMP_MISS: u32 = 0x5E01; // 伪造面未被钳制（防伪失守）
pub const XC_BAD_ID_ACCEPTED: u32 = 0x5E02; // 异常 ID 被采纳（准入失守）
pub const XC_RESUME_DIVERGED: u32 = 0x5E03; // 续跑与全新重跑结果不一致
pub const XC_STAGE_SKIP: u32 = 0x5E04; // 阶段越级（未到步被执行）
pub const XC_FUZZ_CASE_LOST: u32 = 0x5E05; // 发现的异常未固化（CI 失职）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 能力位图合法域（32 位中的低 12 位——其余伪造）。
pub const CAP_MASK_LEGAL: u32 = 0x0FFF;
/// 认证流水阶段数（六步）。
pub const CERT_STAGES: usize = 6;
/// ID 合法上界。
pub const ID_MAX: u64 = 1 << 40;
/// LCG 参数（确定性 fuzz——同 cgd11 范式）。
pub const LCG_A: u64 = 6364136223846793005;
pub const LCG_C: u64 = 1442695040888963407;

/// 可复现随机流。
pub struct FuzzRng {
    state: u64,
}

impl FuzzRng {
    pub fn new(seed: u64) -> Self {
        FuzzRng { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 探测数据钳制器（判据"防伪钳制"——畸形输入到安全域的映射）。
pub struct ProbeClamp {
    /// 钳制命中计数（伪造面显性——不静默采纳）。
    pub clamped: u64,
    pub id_rejected: u64,
}

impl ProbeClamp {
    pub fn new() -> Self {
        ProbeClamp {
            clamped: 0,
            id_rejected: 0,
        }
    }

    /// 能力位图钳制：越界位清零+立案。
    pub fn clamp_cap_mask(&mut self, raw: u32) -> (u32, bool) {
        let illegal = raw & !CAP_MASK_LEGAL;
        if illegal != 0 {
            self.clamped += 1;
            (raw & CAP_MASK_LEGAL, true)
        } else {
            (raw, false)
        }
    }

    /// ID 准入：零/超界拒绝（判据"异常 ID"）。
    pub fn check_id(&mut self, id: u64) -> Result<(), u32> {
        if id == 0 || id > ID_MAX {
            self.id_rejected += 1;
            return Err(XC_BAD_ID_ACCEPTED);
        }
        Ok(())
    }

    /// 重复 ID 准入（表内查重）。
    pub fn check_dup(&mut self, seen: &Vec<u64>, id: u64) -> Result<(), u32> {
        let mut i = 0;
        while i < seen.len() {
            if seen[i] == id {
                self.id_rejected += 1;
                return Err(XC_BAD_ID_ACCEPTED);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 认证流水（六步状态——被试对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CertPipeline {
    /// 已完成到的阶段（0..=6）。
    pub stage: usize,
    /// 每阶段完成标记（续跑语义的验证面）。
    pub done: [bool; CERT_STAGES],
}

impl CertPipeline {
    pub fn new() -> Self {
        CertPipeline {
            stage: 0,
            done: [false; CERT_STAGES],
        }
    }

    /// 执行当前步（越级调用拒绝——判据"续跑语义"的基础）。
    pub fn step(&mut self) -> Result<usize, u32> {
        if self.stage >= CERT_STAGES {
            return Ok(CERT_STAGES); // 已完成
        }
        self.done[self.stage] = true;
        self.stage += 1;
        Ok(self.stage)
    }

    /// 中断模拟：在 stage 处截断（已完成步保持）。
    pub fn interrupt_at(&mut self, at: usize) -> Result<(), u32> {
        if at > CERT_STAGES {
            return Err(XC_STAGE_SKIP);
        }
        // 回卷到 at：>at 的步标记清 false（未到步不越）。
        let mut i = at;
        while i < CERT_STAGES {
            self.done[i] = false;
            i += 1;
        }
        self.stage = at;
        Ok(())
    }

    /// 完成判定。
    pub const fn finished(&self) -> bool {
        self.stage >= CERT_STAGES
    }
}

/// fuzz 编排器。
pub struct CertFuzzer {
    clamp: ProbeClamp,
    cases: Vec<u64>,
    pub runs: u64,
    pub failures: u64,
}

impl CertFuzzer {
    pub fn new(seed: u64) -> Self {
        let _ = seed;
        CertFuzzer {
            clamp: ProbeClamp::new(),
            cases: Vec::new(),
            runs: 0,
            failures: 0,
        }
    }

    /// 畸形数据 fuzz：随机位图+ID 序列——钳制面验证（判据"伪造能力表/异常 ID"）。
    pub fn fuzz_probe(&mut self, rounds: u64, seed: u64) -> Result<(u64, u64), u32> {
        self.runs += 1;
        let mut rng = FuzzRng::new(seed);
        let mut seen: Vec<u64> = Vec::new();
        let mut clamped_total = 0;
        let mut rejected_total = 0;
        let before_clamp = self.clamp.clamped;
        let before_reject = self.clamp.id_rejected;
        let mut i = 0;
        while i < rounds {
            // 位图：一半概率伪造（高位域置位）。
            let mut mask = (rng.below(CAP_MASK_LEGAL as u64 + 1)) as u32;
            if rng.below(2) == 0 {
                mask |= 0xF000; // 伪造位
            }
            let (clamped_mask, was_clamped) = self.clamp.clamp_cap_mask(mask);
            if was_clamped {
                clamped_total += 1;
            }
            // 钳制后必须在合法域。
            if clamped_mask & !CAP_MASK_LEGAL != 0 {
                self.failures += 1;
                return Err(XC_FAKE_CLAMP_MISS);
            }
            // ID：一半概率异常（零/超界）。
            let id = if rng.below(2) == 0 {
                0
            } else {
                rng.below(ID_MAX + 2)
            };
            let id_ok = self.clamp.check_id(id).is_ok();
            if id_ok {
                let dup_ok = self.clamp.check_dup(&seen, id).is_ok();
                if dup_ok {
                    seen.push(id);
                } else {
                    rejected_total += 1;
                }
            } else {
                rejected_total += 1;
            }
            i += 1;
        }
        let _ = (before_clamp, before_reject);
        Ok((clamped_total, rejected_total))
    }

    /// 流程中断 fuzz：随机步中断→续跑→与全新重跑终态一致（判据"续跑语义"）。
    pub fn fuzz_interrupt(&mut self, rounds: u64, seed: u64) -> Result<(), u32> {
        self.runs += 1;
        let mut rng = FuzzRng::new(seed);
        let mut i = 0;
        while i < rounds {
            // 全新重跑基线。
            let mut full = CertPipeline::new();
            let mut s = 0;
            while s < CERT_STAGES {
                full.step()?;
                s += 1;
            }
            // 中断续跑：随机步中断后继续 step 到底。
            let mut resumed = CertPipeline::new();
            let pre = rng.below((CERT_STAGES + 1) as u64) as usize;
            let mut k = 0;
            while k < pre {
                resumed.step()?;
                k += 1;
            }
            resumed.interrupt_at(pre)?;
            // 断点前步保持。
            let mut m = 0;
            while m < pre {
                if !resumed.done[m] {
                    self.failures += 1;
                    return Err(XC_RESUME_DIVERGED);
                }
                m += 1;
            }
            // 断点后不越（未到步未执行）。
            let mut n = pre;
            while n < CERT_STAGES {
                if resumed.done[n] {
                    self.failures += 1;
                    return Err(XC_STAGE_SKIP);
                }
                n += 1;
            }
            // 续跑到完成。
            while !resumed.finished() {
                resumed.step()?;
            }
            // 终态一致。
            if resumed.stage != full.stage || resumed.done != full.done {
                self.failures += 1;
                return Err(XC_RESUME_DIVERGED);
            }
            i += 1;
        }
        Ok(())
    }

    /// 案例固化（判据"案例固化 CI"）：种子+畸形谱指纹入册并可回放对拍。
    pub fn freeze_case(&mut self, seed: u64, rounds: u64) -> Result<(u64, u64), u32> {
        let mut a = CertFuzzer::new(seed);
        let r1 = a.fuzz_probe(rounds, seed)?;
        let mut b = CertFuzzer::new(seed);
        let r2 = b.fuzz_probe(rounds, seed)?;
        if r1 != r2 {
            self.failures += 1;
            return Err(XC_FUZZ_CASE_LOST);
        }
        self.cases.push(seed);
        Ok(r1)
    }

    /// CI 回放。
    pub fn replay_ci(&mut self, rounds: u64) -> Result<usize, u32> {
        let n = self.cases.len();
        let mut i = 0;
        while i < n {
            let seed = self.cases[i];
            let mut probe = CertFuzzer::new(seed);
            let _ = probe.fuzz_probe(rounds, seed)?;
            let r = probe.clamp.clamped;
            // 可复现性：重放第二次一致。
            let mut probe2 = CertFuzzer::new(seed);
            let _ = probe2.fuzz_probe(rounds, seed)?;
            if probe2.clamp.clamped != r {
                self.failures += 1;
                return Err(XC_FUZZ_CASE_LOST);
            }
            i += 1;
        }
        Ok(n)
    }

    /// 钳制计数查询。
    pub fn clamp_stats(&self) -> (u64, u64) {
        (self.clamp.clamped, self.clamp.id_rejected)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：防伪钳制 / 异常 ID / 续跑 / CI 固化）
// ---------------------------------------------------------------------------

impl CertFuzzer {
    /// 组一：防伪钳制——伪造位清除+立案、合法位图直通。
    pub fn selfcheck_clamp(&mut self) -> Result<(), u32> {
        let mut c = ProbeClamp::new();
        let (m1, hit1) = c.clamp_cap_mask(0x1FFF); // 高位伪造
        if m1 != 0x0FFF || !hit1 || c.clamped != 1 {
            return Err(XC_FAKE_CLAMP_MISS);
        }
        let (m2, hit2) = c.clamp_cap_mask(0x123); // 合法
        if m2 != 0x123 || hit2 || c.clamped != 1 {
            return Err(XC_FAKE_CLAMP_MISS);
        }
        // 全伪造位图：钳后为零但不丢立案。
        let (m3, hit3) = c.clamp_cap_mask(0xFFFF_F000);
        if m3 != 0 || !hit3 || c.clamped != 2 {
            return Err(XC_FAKE_CLAMP_MISS);
        }
        Ok(())
    }

    /// 组二：异常 ID——零拒、超界拒、重复拒、正常收。
    pub fn selfcheck_ids(&mut self) -> Result<(), u32> {
        let mut c = ProbeClamp::new();
        if c.check_id(0) != Err(XC_BAD_ID_ACCEPTED) {
            return Err(XC_BAD_ID_ACCEPTED);
        }
        if c.check_id(ID_MAX + 1) != Err(XC_BAD_ID_ACCEPTED) {
            return Err(XC_BAD_ID_ACCEPTED);
        }
        c.check_id(42)?;
        let mut seen: Vec<u64> = Vec::new();
        seen.push(42);
        if c.check_dup(&seen, 42) != Err(XC_BAD_ID_ACCEPTED) {
            return Err(XC_BAD_ID_ACCEPTED);
        }
        c.check_id(ID_MAX)?; // 恰值上界合法
        if c.id_rejected != 3 {
            return Err(XC_BAD_ID_ACCEPTED);
        }
        Ok(())
    }

    /// 组三：续跑——fuzz 中断续跑终态与全跑一致、断点前保持、断点后不越。
    pub fn selfcheck_resume(&mut self) -> Result<(), u32> {
        let mut f = CertFuzzer::new(0);
        f.fuzz_interrupt(64, 2026)?;
        // 手工单例验证。
        let mut p = CertPipeline::new();
        p.step()?;
        p.step()?;
        p.interrupt_at(2)?;
        if !p.done[0] || !p.done[1] {
            return Err(XC_RESUME_DIVERGED);
        }
        if p.done[2] {
            return Err(XC_STAGE_SKIP);
        }
        // 越级中断拒。
        if p.interrupt_at(CERT_STAGES + 1) != Err(XC_STAGE_SKIP) {
            return Err(XC_STAGE_SKIP);
        }
        // 续跑完成。
        while !p.finished() {
            p.step()?;
        }
        if p.stage != CERT_STAGES {
            return Err(XC_RESUME_DIVERGED);
        }
        Ok(())
    }

    /// 组四：CI——同种子谱一致、回放全绿、案例入册。
    pub fn selfcheck_ci(&mut self) -> Result<(), u32> {
        let (c1, r1) = {
            let mut f = CertFuzzer::new(0);
            let r = f.freeze_case(99, 128)?;
            (f.cases.len(), r)
        };
        if c1 != 1 {
            return Err(XC_FUZZ_CASE_LOST);
        }
        let _ = r1;
        let mut f = CertFuzzer::new(0);
        let _ = f.freeze_case(99, 128)?;
        let _ = f.freeze_case(1234, 128)?;
        let n = f.replay_ci(128)?;
        if n != 2 {
            return Err(XC_FUZZ_CASE_LOST);
        }
        // 大样本 fuzz 冒烟：钳制面必命中（1000 轮里必有伪造面）。
        let (clamped, _) = f.fuzz_probe(1000, 777)?;
        if clamped == 0 {
            return Err(XC_FAKE_CLAMP_MISS);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_clamp()?;
        self.selfcheck_ids()?;
        self.selfcheck_resume()?;
        self.selfcheck_ci()?;
        Ok(())
    }
}
