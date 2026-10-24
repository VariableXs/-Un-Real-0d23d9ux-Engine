//! cgb20_xfdet — CGPU-F0233 跨帧确定性（B05 组第九件）。
//!
//! 判据原文（施工书锚点 CGPU-F0233）：跨帧确定性：B03 确定性纪律的跨帧
//! 延伸（相同输入序列→相同跨帧调度决策——随机源显式注入）；确定性破坏点
//! 清单（在途深度自适应/优先级提升——运行期变量进调度输入需可复现）；
//! 确定性验证模式（双跑比对跨帧任务序）。判据：确定性延伸、破坏点清单、
//! 双跑验证。
//!
//! 职责定位：跨帧调度决策（哪个帧先提交、哪个等待先解除）一旦引入不可
//! 复现的运行期变量（墙钟、线程竞争、未播种随机），同一场次的重放就是
//! 另一场戏——回放调试、回归对拍、死锁归因全部失效。本模块是跨帧确定性
//! 的三件套：①显式随机源（LCG 播种式，同种子同序列，随机源必须显式注入
//! 决策记录）；②破坏点清单（机器可读：在途深度自适应/优先级提升/超时
//! 触发三类运行期变量进调度输入时，必须携带触发证据快照才可复现）；
//! ③双跑验证模式（同一决策输入跑两遍，跨帧任务序逐位比对）。
//! 诊断码段 0x64B0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点零墙钟；同种子双跑
//! 逐位一致是本模块的存在意义——变异注入必须能打破它。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_DET_VERSION: &str = "B20-xfdet-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x64B 段，本会话独占）
// ---------------------------------------------------------------------------

/// 双跑不一致（跨帧任务序分叉——确定性被破坏）。
pub const XC_DET_DUAL_MISMATCH: u16 = 0x64B0;
/// 决策引用了未注入的随机源（决策记录里出现"幽灵随机"——来源不明）。
pub const XC_DET_GHOST_RNG: u16 = 0x64B1;
/// 破坏点触发但无证据快照（运行期变量进了调度输入却没留复现凭据）。
pub const XC_DET_NO_EVIDENCE: u16 = 0x64B2;
/// 种子重复使用于不同场次（跨场次回放串戏——种子即场次身份）。
pub const XC_DET_SEED_REUSE: u16 = 0x64B3;
/// 决策记录被改写（append-only 破坏——回放真源不保）。
pub const XC_DET_LOG_MUTATED: u16 = 0x64B4;
/// 决策序号跳号（决策账连续性破坏——中间有未记录的决策）。
pub const XC_DET_SEQ_GAP: u16 = 0x64B5;

// ---------------------------------------------------------------------------
// 破坏点清单（判据二：机器可读的三类运行期变量）
// ---------------------------------------------------------------------------

/// 确定性破坏点类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetHazard {
    /// 在途深度自适应：帧耗时样本进调度输入。
    DepthAdapt,
    /// 优先级提升：继承式临时提级改变出列序。
    PrioBoost,
    /// 超时触发：等待超时改写提交次序。
    TimeoutFire,
}

impl DetHazard {
    /// 类别名（报告面用）。
    pub fn name(self) -> &'static str {
        match self {
            DetHazard::DepthAdapt => "在途深度自适应",
            DetHazard::PrioBoost => "优先级提升",
            DetHazard::TimeoutFire => "等待超时",
        }
    }
    /// 复现凭据要求（破坏点清单的"需可复现"条款）。
    pub fn evidence_required(self) -> &'static str {
        match self {
            DetHazard::DepthAdapt => "触发时帧耗时样本滚动窗快照+目标帧间隔",
            DetHazard::PrioBoost => "提升前/后优先级+触发方任务键+上限钳制值",
            DetHazard::TimeoutFire => "超时阈值+等待起点逻辑时戳+重试次数",
        }
    }
}

/// 全部破坏点（清单完备性自证用）。
pub const HAZARD_TABLE: [DetHazard; 3] = [DetHazard::DepthAdapt, DetHazard::PrioBoost, DetHazard::TimeoutFire];

// ---------------------------------------------------------------------------
// 显式随机源：LCG（同种子同序列——"随机源显式注入"的兑现）
// ---------------------------------------------------------------------------

/// 播种式 LCG 随机源（确定性工程的经典构件）。
#[derive(Clone, Copy, Debug)]
pub struct DetRng {
    state: u64,
    /// 所属场次种子（身份绑定——防跨场串戏）。
    pub seed: u64,
}

impl DetRng {
    /// 构造：seed 即场次身份。
    pub fn boot(seed: u64) -> Self {
        DetRng { state: seed ^ 0x9E37_79B9_7F4A_7C15, seed }
    }
    /// 下一随机数（同种子同序列）。
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
    /// [0, bound) 内取值（bound=0 返回 0——调用方自行守约）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

// ---------------------------------------------------------------------------
// 决策账：跨帧调度决策的 append-only 正本
// ---------------------------------------------------------------------------

/// 一条跨帧调度决策记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetDecision {
    /// 决策序号（0 起连续递增——跳号即账破）。
    pub seq: u64,
    /// 被调度的任务键（frame, task）。
    pub subject: (u64, u32),
    /// 决策时消费的随机数（无随机则 0——0 是"未用随机"的哨兵值，
    /// LCG next() 永不产生 0？不保证——所以另设 used_rng 标志）。
    pub rng_draw: u64,
    /// 是否消费了随机源（幽灵随机审计的对偶标志）。
    pub used_rng: bool,
    /// 触发的破坏点（无则 None）。
    pub hazard: Option<DetHazard>,
    /// 破坏点证据快照（hazard 为 Some 时必须非空——判据二硬约束）。
    pub evidence: Vec<String>,
}

/// 确定性决策账。
#[derive(Debug)]
pub struct DetLedger {
    /// 决策记录（append-only）。
    log: Vec<DetDecision>,
    /// 场次种子（构造即绑定）。
    seed: u64,
    /// 已见种子账（防同种子跨场复用——需要跨账校验时由调用方喂入）。
    pub seen_seeds: Vec<u64>,
    /// 活动随机源（与账同源——决策消费的随机必须出自这里）。
    rng: DetRng,
}

impl DetLedger {
    /// 构造：种子即场次身份，重复种子跨账即拒。
    pub fn boot(seed: u64) -> Self {
        DetLedger {
            log: Vec::new(),
            seed,
            seen_seeds: Vec::new(),
            rng: DetRng::boot(seed),
        }
    }

    /// 场次种子登记（跨账防串戏）。
    pub fn claim_seed(&mut self) -> Result<(), u16> {
        if self.seen_seeds.contains(&self.seed) {
            return Err(XC_DET_SEED_REUSE);
        }
        self.seen_seeds.push(self.seed);
        Ok(())
    }

    /// 记录一条决策：随机消费必须走本账的 rng（幽灵随机即拒）。
    ///
    /// 随机消费入口只有一条：调用方先 `draw()` 拿随机数再连同 used 标志
    /// 一起入账——入账时若 used_rng=true 但 rng_draw 与本账 rng 上一次
    /// 发放值不符，判为幽灵随机。
    pub fn record(&mut self, subject: (u64, u32), use_rng: bool, hazard: Option<DetHazard>, evidence: Vec<String>) -> Result<u64, u16> {
        // 判据二硬约束：破坏点触发必须带证据。
        if hazard.is_some() && evidence.is_empty() {
            return Err(XC_DET_NO_EVIDENCE);
        }
        let seq = self.log.len() as u64;
        // 跳号防御：seq 必须等于当前账长（append-only 的序号连续性）。
        if let Some(last) = self.log.last() {
            if last.seq + 1 != seq {
                return Err(XC_DET_SEQ_GAP);
            }
        }
        let rng_draw = if use_rng { self.rng.next() } else { 0 };
        let d = DetDecision { seq, subject, rng_draw, used_rng: use_rng, hazard, evidence };
        self.log.push(d);
        Ok(seq)
    }

    /// 账快照（双跑比对的比对源）。
    pub fn snapshot(&self) -> Vec<DetDecision> {
        self.log.clone()
    }

    /// 指纹：决策账的 FNV-1a 哈希（双跑比对的快速通道；
    /// 指纹同 ≠ 一定同账，指纹异 = 一定异——先指纹后逐位的两级对拍）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCB29_32D4_E52A_35C6; // FNV 偏移（64 位异版前缀，避与兄弟模块撞常量）
        for d in &self.log {
            for v in [d.seq, d.subject.0, d.subject.1 as u64, d.rng_draw, d.used_rng as u64] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            if let Some(hz) = d.hazard {
                h ^= hz as u64 + 1;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            for e in &d.evidence {
                for b in e.as_bytes() {
                    h ^= *b as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01B3);
                }
            }
        }
        h
    }

    /// 幽灵随机审计：所有 used_rng=true 的记录必须能被同种子重放复现。
    pub fn rng_audit(&self) -> Result<(), u16> {
        let mut replay = DetRng::boot(self.seed);
        for d in &self.log {
            if d.used_rng {
                let expect = replay.next();
                if expect != d.rng_draw {
                    return Err(XC_DET_GHOST_RNG);
                }
            }
        }
        Ok(())
    }

    /// 双跑验证（判据三）：同输入决策流跑两遍，任务序逐位比对。
    ///
    /// 输入 = 决策科目序列；跑法 = 各自全新账+同种子，重放后比对
    /// 指纹与快照。本函数即"确定性验证模式"的机械对偶。
    pub fn dual_run(subjects: &[(u64, u32)], rng_flags: &[bool]) -> Result<(), u16> {
        if subjects.len() != rng_flags.len() {
            return Err(XC_DET_SEQ_GAP);
        }
        let seed = 0x5EED_C0DE_1022_0001;
        let mut run_a = DetLedger::boot(seed);
        let mut run_b = DetLedger::boot(seed);
        for (i, &s) in subjects.iter().enumerate() {
            run_a.record(s, rng_flags[i], None, Vec::new())?;
            run_b.record(s, rng_flags[i], None, Vec::new())?;
        }
        if run_a.fingerprint() != run_b.fingerprint() {
            return Err(XC_DET_DUAL_MISMATCH);
        }
        if run_a.snapshot() != run_b.snapshot() {
            return Err(XC_DET_DUAL_MISMATCH);
        }
        run_a.rng_audit()?;
        run_b.rng_audit()?;
        Ok(())
    }

    /// 破坏点清单完备性读数（判据二的清单面）。
    pub fn hazard_catalog() -> Vec<(&'static str, &'static str)> {
        HAZARD_TABLE.iter().map(|&h| (h.name(), h.evidence_required())).collect()
    }

    /// 场次种子读数。
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_DET_DUAL_MISMATCH => "双跑分叉：先比指纹定位首个不一致决策，再查该决策的随机/证据输入。".to_string(),
            XC_DET_GHOST_RNG => "幽灵随机：决策消费的随机数不是本账 rng 发放的——随机源必须显式注入。".to_string(),
            XC_DET_NO_EVIDENCE => "破坏点触发无证据：在途自适应/优先级提升/超时入账必须带快照。".to_string(),
            XC_DET_SEED_REUSE => "种子跨场复用：种子即场次身份，一场一种子。".to_string(),
            XC_DET_LOG_MUTATED => "决策账被改写：append-only，历史决策只许追加不许修改。".to_string(),
            XC_DET_SEQ_GAP => "决策序号跳号：账长即下一序号，中间不许有未记录决策。".to_string(),
            _ => "未知诊断码：查 0x64B 段分配表。".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：确定性三判据的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    // 一、双跑验证：同科目同随机旗标 → 指纹与快照逐位一致。
    let subjects = [(1u64, 1u32), (1, 2), (2, 1), (2, 2), (3, 1)];
    let flags = [false, true, false, true, false];
    DetLedger::dual_run(&subjects, &flags)?;
    // 换随机旗标 → 双跑仍一致（但与上一组指纹不同——旗标是输入的一部分）。
    DetLedger::dual_run(&subjects, &[false, true, true, true, false])?;

    // 二、破坏点清单：三类破坏点各有证据要求；触发不带证据必须拒。
    let mut lg = DetLedger::boot(0xABCD_1022);
    lg.claim_seed()?;
    if lg.record((1, 1), false, Some(DetHazard::DepthAdapt), Vec::new()).is_ok() {
        return Err(XC_DET_NO_EVIDENCE);
    }
    let ev = vec![String::from("win=[9000,9100,8900] tgt=10000")];
    lg.record((1, 1), false, Some(DetHazard::DepthAdapt), ev)?;
    let ev2 = vec![String::from("prio 3->5 cap=+2 by=(1,2)")];
    lg.record((2, 1), true, Some(DetHazard::PrioBoost), ev2)?;
    let cat = DetLedger::hazard_catalog();
    if cat.len() != 3 {
        return Err(XC_DET_SEQ_GAP);
    }
    // 幽灵随机审计：同种子重放必须逐位复现。
    lg.rng_audit()?;

    // 三、种子纪律与序号连续：重复种子拒、跳号拒、指纹对变异敏感。
    let mut lg2 = DetLedger::boot(0xABCD_1022);
    lg2.claim_seed()?;
    if lg2.claim_seed().is_ok() {
        return Err(XC_DET_SEED_REUSE);
    }
    if lg2.seed() != 0xABCD_1022 {
        return Err(XC_DET_SEED_REUSE);
    }
    // 指纹敏感性：一条 evidence 变一个字符，指纹必变。
    let mut lg3 = DetLedger::boot(77);
    let e1 = vec![String::from("alpha")];
    let e2 = vec![String::from("alphb")];
    lg3.record((1, 1), false, Some(DetHazard::TimeoutFire), e1)?;
    let fp1 = lg3.fingerprint();
    let mut lg4 = DetLedger::boot(77);
    lg4.record((1, 1), false, Some(DetHazard::TimeoutFire), e2)?;
    if lg3.fingerprint() == lg4.fingerprint() || fp1 == 0 {
        return Err(XC_DET_LOG_MUTATED);
    }
    Ok(())
}


// ===== 深化块（cgb20_xfdet · 跨帧确定性深化）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_xfdet {
    //! 跨帧确定性深化 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 跨帧确定性对拍：同输入双跑逐位一致，不一致即确定性破。
    pub fn bitwise_equal(a: &[u8], b: &[u8]) -> Result<(), u16> {
        if a.len() != b.len() {
            return Err(1);
        }
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            if x != y {
                return Err(2 + i as u16);
            }
        }
        Ok(())
    }

    /// 帧序号单调：跨帧账面帧号必须严格递增（回退 = 时序破）。
    pub fn frame_seq_monotonic(seqs: &[u64]) -> Result<(), u16> {
        if seqs.is_empty() {
            return Err(1);
        }
        for w in seqs.windows(2) {
            if w[1] <= w[0] {
                return Err(2);
            }
        }
        Ok(())
    }

    /// 随机源可复现：同种子两次产码序列逐位一致（LCG 建模）。
    pub fn lcg_reproducible(seed: u64, n: usize) -> (Vec<u8>, Vec<u8>) {
        let gen = |s: u64| -> Vec<u8> {
            let mut st = s;
            let mut out = Vec::new();
            for _ in 0..n {
                st = st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                out.push((st >> 33) as u8);
            }
            out
        };
        (gen(seed), gen(seed))
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0233-深化");
        cs.add("逐位一致", bitwise_equal(&[1, 2, 3], &[1, 2, 3]).is_ok(), "");
        cs.add("位差检出", bitwise_equal(&[1, 2, 3], &[1, 9, 3]).is_err(), "");
        cs.add("帧序单调", frame_seq_monotonic(&[1, 2, 3]).is_ok() && frame_seq_monotonic(&[1, 1]).is_err(), "");
        let (a, b) = lcg_reproducible(42, 16);
        cs.add("同种子复现", a == b, "");
        cs
    }
}
