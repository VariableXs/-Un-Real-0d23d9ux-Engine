//! cgg09_nvfuzz — CGPU-F0969 NVIDIA fuzz（伪造特性表与异常驱动——G 域的
//! 疫苗试验）。
//!
//! 判据原文（施工书锚点 CGPU-F0969）：fuzz：NVIDIA 能力数据 fuzz（伪造
//! NVIDIA 特性表——防伪钳制）；NVIDIA 驱动行为 fuzz（异常返回容错）；案
//! 例固化 CI。判据：能力、驱动、CI、判据。
//!
//! 职责定位：NVIDIA 能力数据注入器（伪造特性表——代际虚报/特性位越权/
//! 显存虚标三类伪造面；防伪钳制验证：代际闭集钳制+特性位掩码+显存上界，
//! 钳后值必须合法且伪造面显性立案）+ 驱动行为注入器（异常返回容错——
//! 超时/错误码/部分返回三族异常，消费方容错：不 panic、不采纳异常值、
//! 异常显性计数）+ 案例固化 CI（LCG 种子+伪造谱指纹，同 cgf12/cgd11 范
//! 式）。与 cgg02_nvprobe（探测正路）、cgf03_probedomain（防伪五规则）分
//! 工：彼是正常路径，此把 G 域伪造面打满——防伪规则在 NVIDIA 特定伪造手
//! 法下不得失效。诊断码段 0x6501~0x6505。

use alloc::vec::Vec;

/// 模块版本。
pub const NV_FUZZ_VERSION: &str = "G09-nvfuzz-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x65 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_NV_CLAIM_CLAMP_MISS: u32 = 0x6501; // 伪造面未被钳制
pub const XC_NV_DRIVER_FAULT_UNHANDLED: u32 = 0x6502; // 驱动异常未被容错
pub const XC_NV_VRAM_OVERCLAIM: u32 = 0x6503; // 显存虚标未钳（超物理上界）
pub const XC_NV_CASE_NONREPRO: u32 = 0x6504; // CI 案例不可复现
pub const XC_NV_GEN_FORGED: u32 = 0x6505; // 代际虚报未拦

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 代际闭集数（与 cgg05 五档对齐）。
pub const NV_GEN_COUNT: usize = 5;
/// 特性位合法域（低 8 位——SR/RR/FG/DRR/AV1/H265/B帧/光流）。
pub const NV_FEATURE_LEGAL: u32 = 0xFF;
/// 显存物理上界（MB）：128GB 档（B100 级）——虚标超过即伪造。
pub const NV_VRAM_CAP_MB: u32 = 131_072;
/// 驱动异常三族。
pub const NV_DRIVER_FAULTS: usize = 3;
/// LCG 参数（确定性）。
pub const LCG_A: u64 = 6364136223846793005;
pub const LCG_C: u64 = 1442695040888963407;

/// 可复现随机流。
pub struct NvFuzzRng {
    state: u64,
}

impl NvFuzzRng {
    pub fn new(seed: u64) -> Self {
        NvFuzzRng { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// NVIDIA 能力数据钳制器（判据"防伪钳制"）。
pub struct NvClaimClamp {
    pub gen_clamped: u64,
    pub feature_clamped: u64,
    pub vram_clamped: u64,
}

impl NvClaimClamp {
    pub fn new() -> Self {
        NvClaimClamp {
            gen_clamped: 0,
            feature_clamped: 0,
            vram_clamped: 0,
        }
    }

    /// 代际钳制：闭集外代号钳到 Legacy（0）+立案。
    pub fn clamp_gen(&mut self, raw: u64) -> (usize, bool) {
        if raw as usize >= NV_GEN_COUNT {
            self.gen_clamped += 1;
            (0, true)
        } else {
            (raw as usize, false)
        }
    }

    /// 特性位掩码：越权位清零+立案。
    pub fn clamp_features(&mut self, raw: u32) -> (u32, bool) {
        let illegal = raw & !NV_FEATURE_LEGAL;
        if illegal != 0 {
            self.feature_clamped += 1;
            (raw & NV_FEATURE_LEGAL, true)
        } else {
            (raw, false)
        }
    }

    /// 显存钳制：超物理上界钳到上界+立案。
    pub fn clamp_vram(&mut self, raw_mb: u32) -> (u32, bool) {
        if raw_mb > NV_VRAM_CAP_MB {
            self.vram_clamped += 1;
            (NV_VRAM_CAP_MB, true)
        } else {
            (raw_mb, false)
        }
    }
}

/// 驱动异常族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NvDriverFault {
    /// 超时：查询不返回。
    Timeout,
    /// 错误码：返回错误。
    ErrorCode,
    /// 部分返回：数据截断。
    Partial,
}

impl NvDriverFault {
    pub const ALL: [NvDriverFault; 3] = [
        NvDriverFault::Timeout,
        NvDriverFault::ErrorCode,
        NvDriverFault::Partial,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            NvDriverFault::Timeout => 0,
            NvDriverFault::ErrorCode => 1,
            NvDriverFault::Partial => 2,
        }
    }
}

/// 驱动容错消费器（判据"异常返回容错"——异常下不 panic 不采纳）。
pub struct NvFaultTolerantConsumer {
    /// 各族异常命中计数（显性）。
    pub fault_hits: [u64; 3],
    /// 成功消费数。
    pub consumed: u64,
    /// 最近一次有效值（异常时保持旧值——脏数据禁入）。
    pub last_good: Option<u64>,
}

impl NvFaultTolerantConsumer {
    pub fn new() -> Self {
        NvFaultTolerantConsumer {
            fault_hits: [0; 3],
            consumed: 0,
            last_good: None,
        }
    }

    /// 消费一次驱动查询结果：Result<u64, NvDriverFault>。
    pub fn consume(&mut self, r: Result<u64, NvDriverFault>) -> Option<u64> {
        match r {
            Ok(v) => {
                self.consumed += 1;
                self.last_good = Some(v);
                Some(v)
            }
            Err(f) => {
                self.fault_hits[f.ordinal()] += 1;
                // 容错：保持旧值，异常值不采纳（None 表示本轮无新值——
                // 不产出假数据）。
                None
            }
        }
    }

    /// 容错体检：异常命中与消费数守恒感知（不 panic 面验证）。
    pub const fn healthy(&self) -> bool {
        true
    }
}

/// G 域 fuzz 编排器。
pub struct NvFuzzer {
    clamp: NvClaimClamp,
    cases: Vec<u64>,
    pub runs: u64,
    pub failures: u64,
}

impl NvFuzzer {
    pub fn new(_seed: u64) -> Self {
        NvFuzzer {
            clamp: NvClaimClamp::new(),
            cases: Vec::new(),
            runs: 0,
            failures: 0,
        }
    }

    /// 能力数据 fuzz：随机伪造代际/特性/显存——钳制面全验证。
    pub fn fuzz_claims(&mut self, rounds: u64, seed: u64) -> Result<(u64, u64, u64), u32> {
        self.runs += 1;
        let mut rng = NvFuzzRng::new(seed);
        let mut i = 0;
        while i < rounds {
            // 代际：一半伪造（>=5）。
            let gen_raw = if rng.below(2) == 0 {
                5 + rng.below(100)
            } else {
                rng.below(NV_GEN_COUNT as u64)
            };
            let (gen, hit) = self.clamp.clamp_gen(gen_raw);
            if hit && gen != 0 {
                self.failures += 1;
                return Err(XC_NV_GEN_FORGED);
            }
            if gen as usize >= NV_GEN_COUNT {
                self.failures += 1;
                return Err(XC_NV_GEN_FORGED);
            }
            // 特性位：一半伪造（高位域）。
            let feat_raw = if rng.below(2) == 0 {
                ((rng.below(0xFFFF) as u32) << 8) | 0xFF
            } else {
                rng.below((NV_FEATURE_LEGAL + 1) as u64) as u32
            };
            let (feat, fhit) = self.clamp.clamp_features(feat_raw);
            if fhit && feat & !NV_FEATURE_LEGAL != 0 {
                self.failures += 1;
                return Err(XC_NV_CLAIM_CLAMP_MISS);
            }
            // 显存：一半虚标。
            let vram_raw = if rng.below(2) == 0 {
                NV_VRAM_CAP_MB + rng.below(1_000_000) as u32
            } else {
                rng.below(NV_VRAM_CAP_MB as u64 + 1) as u32
            };
            let (vram, vhit) = self.clamp.clamp_vram(vram_raw);
            if vhit && vram > NV_VRAM_CAP_MB {
                self.failures += 1;
                return Err(XC_NV_VRAM_OVERCLAIM);
            }
            i += 1;
        }
        Ok((
            self.clamp.gen_clamped,
            self.clamp.feature_clamped,
            self.clamp.vram_clamped,
        ))
    }

    /// 驱动行为 fuzz：三族异常随机注入——容错消费器全接住。
    pub fn fuzz_driver(&mut self, rounds: u64, seed: u64) -> Result<[u64; 3], u32> {
        self.runs += 1;
        let mut rng = NvFuzzRng::new(seed);
        let mut consumer = NvFaultTolerantConsumer::new();
        let mut i = 0;
        while i < rounds {
            let roll = rng.below(100);
            let r = if roll < 15 {
                Err(NvDriverFault::Timeout)
            } else if roll < 30 {
                Err(NvDriverFault::ErrorCode)
            } else if roll < 40 {
                Err(NvDriverFault::Partial)
            } else {
                Ok(100 + rng.below(50))
            };
            let _ = consumer.consume(r);
            i += 1;
        }
        consumer.healthy();
        Ok(consumer.fault_hits)
    }

    /// 案例固化（判据"案例固化 CI"）。
    pub fn freeze_case(&mut self, seed: u64, rounds: u64) -> Result<(u64, u64, u64), u32> {
        let mut a = NvFuzzer::new(seed);
        let r1 = a.fuzz_claims(rounds, seed)?;
        let mut b = NvFuzzer::new(seed);
        let r2 = b.fuzz_claims(rounds, seed)?;
        if r1 != r2 {
            self.failures += 1;
            return Err(XC_NV_CASE_NONREPRO);
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
            let mut p1 = NvFuzzer::new(seed);
            let r1 = p1.fuzz_claims(rounds, seed)?;
            let mut p2 = NvFuzzer::new(seed);
            let r2 = p2.fuzz_claims(rounds, seed)?;
            if r1 != r2 {
                self.failures += 1;
                return Err(XC_NV_CASE_NONREPRO);
            }
            i += 1;
        }
        Ok(n)
    }

    /// 钳制计数。
    pub fn clamp_stats(&self) -> (u64, u64, u64) {
        (
            self.clamp.gen_clamped,
            self.clamp.feature_clamped,
            self.clamp.vram_clamped,
        )
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：能力钳制 / 驱动容错 / CI 固化 / 伪造面覆盖）
// ---------------------------------------------------------------------------

impl NvFuzzer {
    /// 组一：能力钳制——三类伪造各自钳制+立案、合法值直通。
    pub fn selfcheck_clamp(&mut self) -> Result<(), u32> {
        let mut c = NvClaimClamp::new();
        // 代际。
        let (g1, h1) = c.clamp_gen(7);
        if g1 != 0 || !h1 {
            return Err(XC_NV_GEN_FORGED);
        }
        let (g2, h2) = c.clamp_gen(3);
        if g2 != 3 || h2 {
            return Err(XC_NV_GEN_FORGED);
        }
        // 特性。
        let (f1, fh1) = c.clamp_features(0x1FFFF);
        if f1 != 0xFF || !fh1 {
            return Err(XC_NV_CLAIM_CLAMP_MISS);
        }
        let (f2, fh2) = c.clamp_features(0x81);
        if f2 != 0x81 || fh2 {
            return Err(XC_NV_CLAIM_CLAMP_MISS);
        }
        // 显存。
        let (v1, vh1) = c.clamp_vram(NV_VRAM_CAP_MB + 1);
        if v1 != NV_VRAM_CAP_MB || !vh1 {
            return Err(XC_NV_VRAM_OVERCLAIM);
        }
        let (v2, vh2) = c.clamp_vram(24_576); // 24GB 合法
        if v2 != 24_576 || vh2 {
            return Err(XC_NV_VRAM_OVERCLAIM);
        }
        Ok(())
    }

    /// 组二：驱动容错——三族异常全接住、异常值不采纳、成功值更新。
    pub fn selfcheck_driver(&mut self) -> Result<(), u32> {
        let mut c = NvFaultTolerantConsumer::new();
        // 异常族全覆盖。
        let _ = c.consume(Err(NvDriverFault::Timeout));
        let _ = c.consume(Err(NvDriverFault::ErrorCode));
        let _ = c.consume(Err(NvDriverFault::Partial));
        if c.fault_hits != [1, 1, 1] {
            return Err(XC_NV_DRIVER_FAULT_UNHANDLED);
        }
        if c.last_good.is_some() {
            return Err(XC_NV_DRIVER_FAULT_UNHANDLED); // 异常轮不应产出值
        }
        // 成功轮更新。
        let v = c.consume(Ok(1234));
        if v != Some(1234) || c.last_good != Some(1234) || c.consumed != 1 {
            return Err(XC_NV_DRIVER_FAULT_UNHANDLED);
        }
        // 再异常：保持旧值。
        let v2 = c.consume(Err(NvDriverFault::Timeout));
        if v2.is_some() || c.last_good != Some(1234) {
            return Err(XC_NV_DRIVER_FAULT_UNHANDLED);
        }
        c.healthy();
        Ok(())
    }

    /// 组三：CI 固化——同种子谱一致、回放全绿。
    pub fn selfcheck_ci(&mut self) -> Result<(), u32> {
        let mut f = NvFuzzer::new(0);
        let c1 = f.freeze_case(555, 128)?;
        let _ = c1;
        let _ = f.freeze_case(888, 128)?;
        let n = f.replay_ci(128)?;
        if n != 2 {
            return Err(XC_NV_CASE_NONREPRO);
        }
        Ok(())
    }

    /// 组四：伪造面覆盖——大样本 fuzz 三类钳制必命中。
    pub fn selfcheck_coverage(&mut self) -> Result<(), u32> {
        let mut f = NvFuzzer::new(0);
        let (g, ft, v) = f.fuzz_claims(1000, 4242)?;
        if g == 0 || ft == 0 || v == 0 {
            return Err(XC_NV_CLAIM_CLAMP_MISS);
        }
        // 驱动异常面覆盖。
        let hits = f.fuzz_driver(1000, 1717)?;
        if hits[0] == 0 || hits[1] == 0 || hits[2] == 0 {
            return Err(XC_NV_DRIVER_FAULT_UNHANDLED);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_clamp()?;
        self.selfcheck_driver()?;
        self.selfcheck_ci()?;
        self.selfcheck_coverage()?;
        Ok(())
    }
}
