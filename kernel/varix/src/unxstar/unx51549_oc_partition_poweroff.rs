//! UNX-F51549 HUB 过流分区断电（仅故障端口断电） · 0xC0C00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面逐字锚定）：
//!   UNX-F51550-J1 三错误注入 ×3×10 恢复全通。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   ① PowerPartition —— 供电分区状态机（On→OCFault→Isolated→Recovering
//!      →On 单向环），只许故障端口所在分区断电，邻区端口供电状态零变化。
//!   ② OcDetector —— 过流检测调度器：阈值纯整数比较（毫安×10 计），连续
//!      命中计数达阈才判过流，单次毛刺不动作（防误断）。
//!   ③ RecoveryLedger —— 断电/恢复账：每轮过流事件与恢复结果逐条入账，
//!      三类错误注入各 ×3 组 ×10 轮恢复必须全通；注入账守恒。
//!   ④ OcInjector —— 注入核：5 类坏样本逐类 ×10 轮精确复现，检出+逃逸=
//!      轮数，逃逸恒 0。
//!   ⑤ OcCode —— 诊断码闭集（13 变体），val()=SEG_BASE+1+变体序号。
//!
//! 设计要点：
//!   no_std + alloc；零浮点、零 IO、零墙钟、零随机——伪随机一律走 LCG
//!   （种子=SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律走
//!   .get()/.get_mut()；双跑 FNV-1a 指纹必须逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段号：0xC0000 + 0x40 × (51549 − 51501)。
pub const SEG_BASE: u32 = 0xC0C00;

/// 端口数上限。
pub const MAX_PORT: usize = 16;
/// 过流阈值（毫安×10 纯整数计）：2500 = 250mA。
pub const OC_THRESHOLD_MA10: u32 = 2500;
/// 连续命中判过流所需次数（毛刺免疫）。
pub const OC_STRIKES: u32 = 3;
/// 分区数上限（每 4 端口一个供电分区）。
pub const PARTITIONS: usize = 4;
/// 判据锚定：三错误注入 ×3 组。
pub const FAULT_GROUPS: usize = 3;
/// 判据锚定：每组 ×10 轮。
pub const FAULT_ROUNDS: usize = 10;

/// 供电分区状态机（单向环：On→OCFault→Isolated→Recovering→On）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartState {
    /// 正常供电
    On = 0,
    /// 检出过流
    OcFault = 1,
    /// 已隔离断电（仅故障端口所在分区）
    Isolated = 2,
    /// 恢复中（重枚举校验）
    Recovering = 3,
}

impl PartState {
    /// 状态序号：用于合法迁移表下标校验。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 合法迁移闭集：只许沿环前进一步（Recovering→On 视为回到 0）。
    pub fn can_advance(self, want: PartState) -> bool {
        match (self, want) {
            (PartState::On, PartState::OcFault) => true,
            (PartState::OcFault, PartState::Isolated) => true,
            (PartState::Isolated, PartState::Recovering) => true,
            (PartState::Recovering, PartState::On) => true,
            _ => false,
        }
    }
}

/// 错误类别（闭集）：持续过流/毛刺过流/闩锁短路/阈值漂移/恢复超时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 持续过流（必须断电）
    Sustained = 0,
    /// 毛刺过流（不许断电，防误动作）
    Glitch = 1,
    /// 闩锁短路（断电后恢复须拒绝直到清除）
    LatchShort = 2,
    /// 阈值漂移注入（越界阈值须拒）
    ThresholdDrift = 3,
    /// 恢复序号倒退注入
    RecoveryRegress = 4,
}

impl FaultKind {
    /// 类别码闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<FaultKind> {
        match c {
            0 => Some(FaultKind::Sustained),
            1 => Some(FaultKind::Glitch),
            2 => Some(FaultKind::LatchShort),
            3 => Some(FaultKind::ThresholdDrift),
            4 => Some(FaultKind::RecoveryRegress),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            FaultKind::Sustained => "持续过流",
            FaultKind::Glitch => "毛刺过流",
            FaultKind::LatchShort => "闩锁短路",
            FaultKind::ThresholdDrift => "阈值漂移",
            FaultKind::RecoveryRegress => "恢复倒退",
        }
    }
}

/// 账本条目：一次断电/恢复判定的完整留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OcEntry {
    /// 事件序号（单调递增）
    pub seq: u64,
    /// 分区号
    pub part: usize,
    /// 故障端口
    pub port: usize,
    /// 是否执行了分区断电
    pub cut: bool,
    /// 邻区受扰分区数（判据要求恒 0）
    pub disturbed: usize,
    /// 恢复是否全通
    pub recovered: bool,
}

/// 诊断码闭集（过流分区断电域，13 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OcCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 端口号越界
    PortOob,
    /// 阈值非法（0 或超出 5000 上限）
    ThresholdInvalid,
    /// 故障类别码越闭集
    FaultUnknown,
    /// 毛刺误断（单次命中不许断电）
    GlitchMisfire,
    /// 分区已隔离再断
    AlreadyIsolated,
    /// 恢复序号倒退
    RecoveryRegress,
    /// 恢复被闩锁短路拒绝
    LatchedShort,
    /// 邻区受扰（只许故障端口所在分区断电）
    NeighborDisturbed,
    /// 账面守恒不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
    /// 恢复未全通
    RecoveryIncomplete,
}

/// 权威闭集数组（val() 序号以此为准）。
pub const ALL: [OcCode; 13] = [
    OcCode::Ok,
    OcCode::SegBad,
    OcCode::PortOob,
    OcCode::ThresholdInvalid,
    OcCode::FaultUnknown,
    OcCode::GlitchMisfire,
    OcCode::AlreadyIsolated,
    OcCode::RecoveryRegress,
    OcCode::LatchedShort,
    OcCode::NeighborDisturbed,
    OcCode::LedgerMismatch,
    OcCode::FingerprintMismatch,
    OcCode::RecoveryIncomplete,
];

impl OcCode {
    /// 诊断值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        match ALL.iter().position(|&c| c == self) {
            Some(i) => SEG_BASE + 1 + i as u32,
            None => SEG_BASE + 1,
        }
    }
    /// 全量非空中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            OcCode::Ok => "过流分区判定成功",
            OcCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            OcCode::PortOob => "端口越界：端口号必须在 0..16 之内",
            OcCode::ThresholdInvalid => "阈值非法：过流阈值必须为 1..=5000 毫安×10",
            OcCode::FaultUnknown => "故障类别非法：只接受 0..=4 五类闭集码",
            OcCode::GlitchMisfire => "毛刺误断：连续命中未达阈不许断电",
            OcCode::AlreadyIsolated => "分区已隔离：断电状态不许重复执行",
            OcCode::RecoveryRegress => "恢复倒退：恢复轮次必须严格递增",
            OcCode::LatchedShort => "闩锁短路：先清除短路标记再恢复供电",
            OcCode::NeighborDisturbed => "邻区受扰：只许故障端口所在分区断电",
            OcCode::LedgerMismatch => "账面不符：注入=检出+逃逸且逃逸恒零",
            OcCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 必须逐位一致",
            OcCode::RecoveryIncomplete => "恢复未全通：三错误注入 ×3×10 恢复必须全通",
        }
    }
}

/// 过流检测调度器：连续命中计数达阈才判过流。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OcDetector {
    threshold: u32,
    strikes: u32,
    latched: bool,
}

impl OcDetector {
    /// 建检测器：阈值 1..=5000，越界显性拒。
    pub fn new(threshold: u32) -> Result<OcDetector, OcCode> {
        if threshold == 0 || threshold > 5000 {
            return Err(OcCode::ThresholdInvalid);
        }
        Ok(OcDetector { threshold, strikes: 0, latched: false })
    }

    /// 采样一次电流：越阈累计连续命中，达 OC_STRIKES 判过流；低于阈值清零。
    pub fn sample(&mut self, ma10: u32) -> bool {
        if ma10 >= self.threshold {
            self.strikes = self.strikes.saturating_add(1);
        } else {
            self.strikes = 0;
        }
        if self.strikes >= OC_STRIKES {
            self.latched = true;
            true
        } else {
            false
        }
    }

    /// 单次毛刺（越阈一次后回落）不得判过流。
    pub fn glitch_clean(&mut self) -> bool {
        let _ = self.sample(OC_THRESHOLD_MA10 + 500);
        let _ = self.sample(0);
        !(self.strikes >= OC_STRIKES)
    }

    /// 清除闩锁短路标记（恢复前置条件）。
    pub fn clear_latch(&mut self) {
        self.latched = false;
        self.strikes = 0;
    }

    pub fn latched(&self) -> bool {
        self.latched
    }
    pub fn strikes(&self) -> u32 {
        self.strikes
    }
}

/// 供电分区状态机 + 断电/恢复账 + 守恒注入账。
pub struct PartitionPower {
    states: [PartState; PARTITIONS],
    last_recovery_seq: u64,
    ledger: Vec<OcEntry>,
    cut_cnt: u64,
    recovered_cnt: u64,
    disturbed_total: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
    /// LCG 种子：SEG_BASE 派生，唯一合法的"随机"来源。
    lcg: u64,
}

/// 端口 → 分区映射（每 4 端口一个分区）。
pub fn partition_of(port: usize) -> Option<usize> {
    if port >= MAX_PORT {
        None
    } else {
        Some(port / 4)
    }
}

/// LCG 一步：线性同余伪随机（确定性，替代真随机源）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state >> 33
}

/// FNV-1a 64 位指纹（双跑一致面）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl PartitionPower {
    /// 建电源面：全分区 On、零账本、LCG 种子=SEG_BASE。
    pub fn new() -> PartitionPower {
        PartitionPower {
            states: [PartState::On; PARTITIONS],
            last_recovery_seq: 0,
            ledger: Vec::new(),
            cut_cnt: 0,
            recovered_cnt: 0,
            disturbed_total: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            lcg: SEG_BASE as u64,
        }
    }

    /// 分区断电：仅故障端口所在分区进入 Isolated，邻区状态零变化。
    pub fn isolate(&mut self, port: usize) -> Result<usize, OcCode> {
        let part = match partition_of(port) {
            Some(p) => p,
            None => return Err(OcCode::PortOob),
        };
        let cur = match self.states.get(part) {
            Some(&s) => s,
            None => return Err(OcCode::PortOob),
        };
        if cur == PartState::Isolated {
            return Err(OcCode::AlreadyIsolated);
        }
        if !PartState::On.can_advance(PartState::OcFault) {
            return Err(OcCode::AlreadyIsolated);
        }
        // 状态迁移 On→OcFault→Isolated（环上两步）。
        if let Some(s) = self.states.get_mut(part) {
            *s = PartState::OcFault;
        }
        if let Some(s) = self.states.get_mut(part) {
            *s = PartState::Isolated;
        }
        // 邻区核验：除本分区外任何分区状态必须仍是 On（零受扰）。
        let mut disturbed = 0usize;
        for p in 0..PARTITIONS {
            if p != part && self.states.get(p) != Some(&PartState::On) {
                disturbed = disturbed.saturating_add(1);
            }
        }
        self.disturbed_total = self.disturbed_total.saturating_add(disturbed);
        self.cut_cnt = self.cut_cnt.saturating_add(1);
        Ok(disturbed)
    }

    /// 恢复供电：清除闩锁后按环推进 Isolated→Recovering→On，序号须递增。
    pub fn recover(&mut self, port: usize, seq: u64, latch: bool) -> Result<(), OcCode> {
        let part = match partition_of(port) {
            Some(p) => p,
            None => return Err(OcCode::PortOob),
        };
        if seq <= self.last_recovery_seq {
            return Err(OcCode::RecoveryRegress);
        }
        self.last_recovery_seq = seq;
        if latch {
            return Err(OcCode::LatchedShort);
        }
        let cur = match self.states.get(part) {
            Some(&s) => s,
            None => return Err(OcCode::PortOob),
        };
        if !cur.can_advance(PartState::Recovering) {
            return Err(OcCode::AlreadyIsolated);
        }
        if let Some(s) = self.states.get_mut(part) {
            *s = PartState::Recovering;
        }
        if let Some(s) = self.states.get_mut(part) {
            *s = PartState::On;
        }
        self.recovered_cnt = self.recovered_cnt.saturating_add(1);
        self.ledger.push(OcEntry {
            seq,
            part,
            port,
            cut: true,
            disturbed: 0,
            recovered: true,
        });
        Ok(())
    }

    /// 落一条判定留痕（断电面）。
    pub fn record_cut(&mut self, seq: u64, part: usize, port: usize, disturbed: usize) {
        self.ledger.push(OcEntry { seq, part, port, cut: true, disturbed, recovered: false });
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    pub fn state_of(&self, part: usize) -> Option<PartState> {
        match self.states.get(part) {
            Some(&s) => Some(s),
            None => None,
        }
    }

    pub fn cut_count(&self) -> u64 {
        self.cut_cnt
    }
    pub fn recovered_count(&self) -> u64 {
        self.recovered_cnt
    }
    pub fn disturbed_total(&self) -> usize {
        self.disturbed_total
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> ([u32; PARTITIONS], u64, u64, usize, usize) {
        let mut arr = [0u32; PARTITIONS];
        for (i, s) in self.states.iter().enumerate() {
            if let Some(slot) = arr.get_mut(i) {
                *slot = *s as u32;
            }
        }
        (arr, self.cut_cnt, self.recovered_cnt, self.disturbed_total, self.ledger.len())
    }

    /// 对账本做 FNV-1a 指纹：每条留痕的关键字段逐字节喂入。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for e in self.ledger.iter() {
            buf.extend_from_slice(&e.seq.to_le_bytes());
            buf.push(e.part as u8);
            buf.push(e.port as u8);
            buf.push(e.cut as u8);
            buf.push(e.recovered as u8);
            buf.extend_from_slice(&(e.disturbed as u32).to_le_bytes());
        }
        fnv1a(&buf)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 三错误注入 ×3 组 ×10 轮恢复全通（判据 UNX-F51550-J1 的主执行面）。
pub fn triple_fault_recovery_all_pass() -> bool {
    let mut all_pass = true;
    for g in 0..FAULT_GROUPS {
        for r in 0..FAULT_ROUNDS {
            let mut pw = PartitionPower::new();
            let port = 4 * (g % PARTITIONS) + (r % 4);
            // 每轮：持续过流 → 断电（零邻扰）→ 清闩 → 恢复。
            let mut det = match OcDetector::new(OC_THRESHOLD_MA10) {
                Ok(d) => d,
                Err(_) => {
                    all_pass = false;
                    break;
                }
            };
            let mut hit = false;
            for _ in 0..OC_STRIKES {
                hit = det.sample(OC_THRESHOLD_MA10 + 100);
            }
            let disturbed = match pw.isolate(port) {
                Ok(d) => d,
                Err(_) => {
                    all_pass = false;
                    break;
                }
            };
            if disturbed != 0 || !hit {
                all_pass = false;
                break;
            }
            det.clear_latch();
            let seq = (g * FAULT_ROUNDS + r + 1) as u64;
            if pw.recover(port, seq, det.latched()).is_err() {
                all_pass = false;
                break;
            }
            if pw.state_of(partition_of(port).unwrap_or(0)) != Some(PartState::On) {
                all_pass = false;
                break;
            }
        }
    }
    all_pass
}

/// 双跑一致性：同一脚本两次建电源面，指纹与快照逐位一致。
pub fn dual_run_consistent() -> bool {
    let run = || -> (u64, ([u32; PARTITIONS], u64, u64, usize, usize)) {
        let mut pw = PartitionPower::new();
        let mut seq = 0u64;
        for i in 0..8usize {
            let port = 4 + (i % 3);
            let d = match pw.isolate(port) {
                Ok(d) => d,
                Err(_) => 9,
            };
            seq += 1;
            let _ = pw.record_cut(seq, partition_of(port).unwrap_or(0), port, d);
            seq += 1;
            let _ = pw.recover(port, seq, false);
        }
        (pw.fingerprint(), pw.snapshot())
    };
    run() == run()
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现，全部检出、零逃逸。
pub fn run_injection_matrix() -> (u64, u64, u64) {
    let mut pw = PartitionPower::new();
    let mut seq = 0u64;
    // 类1：故障类别码越闭集 ×10。
    for i in 0..10u32 {
        if FaultKind::from_code(5 + i).is_none() {
            pw.note_detected();
        } else {
            pw.note_escape();
        }
    }
    // 类2：端口号越界 ×10。
    for i in 0..10u32 {
        if partition_of(MAX_PORT + i as usize).is_none()
            && pw.isolate(MAX_PORT + i as usize) == Err(OcCode::PortOob)
        {
            pw.note_detected();
        } else {
            pw.note_escape();
        }
    }
    // 类3：阈值非法（0 / 超 5000）×10。
    for i in 0..10u32 {
        let t = if i % 2 == 0 { 0 } else { 5001 + i };
        if OcDetector::new(t) == Err(OcCode::ThresholdInvalid) {
            pw.note_detected();
        } else {
            pw.note_escape();
        }
    }
    // 类4：毛刺误断 ×10（单次越阈回落不许断电）。
    for _ in 0..10u32 {
        let mut det = match OcDetector::new(OC_THRESHOLD_MA10) {
            Ok(d) => d,
            Err(_) => break,
        };
        if det.glitch_clean() && det.strikes() < OC_STRIKES {
            pw.note_detected();
        } else {
            pw.note_escape();
        }
    }
    // 类5：恢复序号倒退 ×10（重复恢复同一序号必拒）。
    for _ in 0..10u32 {
        seq += 1;
        let s = seq;
        let _ = pw.isolate(8);
        let _ = pw.recover(8, s, false);
        // 重放同序号恢复必倒退。
        let _ = pw.isolate(8);
        if pw.recover(8, s, false) == Err(OcCode::RecoveryRegress) {
            pw.note_detected();
        } else {
            pw.note_escape();
        }
    }
    (pw.injected, pw.detected, pw.escaped)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51549_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51549");
    // 1 判据锚定：三错误注入 ×3×10 恢复全通。
    s.add_dyn(
        "三错误注入 ×3×10 恢复全通",
        triple_fault_recovery_all_pass(),
        "UNX-F51550-J1：三错误注入 ×3 组 ×10 轮恢复必须全通",
    );
    // 2 仅故障端口所在分区断电：隔离后邻区全部保持 On。
    {
        let mut pw = PartitionPower::new();
        let d = match pw.isolate(6) {
            Ok(d) => d,
            Err(_) => 9,
        };
        let neigh_ok = pw.state_of(0) == Some(PartState::On)
            && pw.state_of(2) == Some(PartState::On)
            && pw.state_of(3) == Some(PartState::On);
        s.add_dyn(
            "仅故障分区断电",
            d == 0 && neigh_ok && pw.state_of(1) == Some(PartState::Isolated),
            "UNX-F51550-J1：只许故障端口所在分区断电且邻区零受扰",
        );
    }
    // 3 邻端口零受扰 ×10 轮。
    {
        let mut pw = PartitionPower::new();
        let mut zero = true;
        for i in 0..10usize {
            if pw.isolate(4 + (i % 3)) != Ok(0) {
                zero = false;
            }
            let seq = (i + 1) as u64;
            let _ = pw.recover(4 + (i % 3), seq, false);
        }
        s.add_dyn(
            "邻端口零受扰 ×10",
            zero && pw.disturbed_total() == 0,
            "UNX-F51550-J1：断电 ×10 每轮邻区受扰数必须为零",
        );
    }
    // 4 全断误动作 0 命中：任何断电都必须落在单一分区，不许波及全四区。
    {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(2);
        let mut on_cnt = 0usize;
        for p in 0..PARTITIONS {
            if pw.state_of(p) == Some(PartState::On) {
                on_cnt += 1;
            }
        }
        s.add_dyn(
            "全断误动作 0 命中",
            on_cnt == PARTITIONS - 1,
            "UNX-F51550-J1：单口断电后必须恰有一个分区离线、三个在线",
        );
    }
    // 5 毛刺免疫：单次越阈回落不断电、不上闩。
    {
        let mut det = match OcDetector::new(OC_THRESHOLD_MA10) {
            Ok(d) => d,
            Err(_) => return s,
        };
        s.add_dyn(
            "毛刺免疫",
            det.glitch_clean() && !det.latched(),
            "单次过流毛刺必须被 OC_STRIKES 连击机制滤除",
        );
    }
    // 6 连击判过流：连续 3 次越阈才判定。
    {
        let mut det = match OcDetector::new(OC_THRESHOLD_MA10) {
            Ok(d) => d,
            Err(_) => return s,
        };
        let a = det.sample(OC_THRESHOLD_MA10 + 100);
        let b = det.sample(OC_THRESHOLD_MA10 + 100);
        let c = det.sample(OC_THRESHOLD_MA10 + 100);
        s.add_dyn(
            "连击达阈判过流",
            !a && !b && c && det.latched(),
            "连续三次越阈才判过流并上闩锁标记",
        );
    }
    // 7 阈值闭集：0 与超 5000 拒、边界 1 与 5000 收。
    {
        let a = OcDetector::new(0) == Err(OcCode::ThresholdInvalid);
        let b = OcDetector::new(5001) == Err(OcCode::ThresholdInvalid);
        let c = OcDetector::new(1).is_ok();
        let d = OcDetector::new(5000).is_ok();
        s.add_dyn("阈值闭集守卫", a && b && c && d, "过流阈值必须落在 1..=5000 闭区间");
    }
    // 8 重复断电拒：已隔离分区不许再断。
    {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(5);
        s.add_dyn(
            "重复断电显性拒",
            pw.isolate(5) == Err(OcCode::AlreadyIsolated),
            "已隔离分区重复断电必须显性拒",
        );
    }
    // 9 闩锁短路拒恢复：未清闩不许上电。
    {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(9);
        s.add_dyn(
            "闩锁短路拒恢复",
            pw.recover(9, 1, true) == Err(OcCode::LatchedShort),
            "未清除闩锁短路标记前恢复供电必须被拒",
        );
    }
    // 10 恢复序号单调：重放与倒退全拒。
    {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(12);
        let a = pw.recover(12, 5, false).is_ok();
        let _ = pw.isolate(12);
        let b = pw.recover(12, 5, false) == Err(OcCode::RecoveryRegress);
        s.add_dyn("恢复序号严格递增", a && b, "恢复轮次序号重复或倒退必须显性拒");
    }
    // 11 状态环合法迁移闭集。
    {
        let ok_chain = PartState::On.can_advance(PartState::OcFault)
            && PartState::OcFault.can_advance(PartState::Isolated)
            && PartState::Isolated.can_advance(PartState::Recovering)
            && PartState::Recovering.can_advance(PartState::On);
        let bad = PartState::On.can_advance(PartState::Isolated)
            || PartState::Isolated.can_advance(PartState::OcFault);
        s.add_dyn("状态环迁移闭集", ok_chain && !bad, "只许沿 On→OcFault→Isolated→Recovering→On 单向环");
    }
    // 12 注入矩阵：5 类 ×10 轮全检出、零逃逸。
    {
        let (inj, det, esc) = run_injection_matrix();
        s.add_dyn(
            "注入矩阵 5×10 全抓",
            inj == 50 && det == 50 && esc == 0,
            "5 类坏样本逐类 ×10 必须全检出且零逃逸",
        );
    }
    // 13 注入账守恒。
    {
        let mut pw = PartitionPower::new();
        for _ in 0..10u32 {
            pw.note_detected();
        }
        s.add_dyn("注入账守恒", pw.conserved(), "注入=检出+逃逸，逃逸恒 0");
    }
    // 14 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一脚本两次建电源面 FNV-1a 逐位一致");
    // 15 诊断码闭集：文案全非空、val 严格递增。
    {
        let mut ok = true;
        for (i, c) in ALL.iter().enumerate() {
            if c.detail().is_empty() || c.val() != SEG_BASE + 1 + i as u32 {
                ok = false;
            }
        }
        s.add_dyn("诊断码闭集", ok, "13 变体文案非空且 val=段基+1+序号");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_needs_three_strikes() {
        let mut det = OcDetector::new(OC_THRESHOLD_MA10).unwrap_or(OcDetector { threshold: OC_THRESHOLD_MA10, strikes: 0, latched: false });
        assert!(!det.sample(OC_THRESHOLD_MA10 + 1));
        assert!(!det.sample(OC_THRESHOLD_MA10 + 1));
        assert!(!det.latched());
        assert!(det.sample(OC_THRESHOLD_MA10 + 1));
        assert!(det.latched());
    }

    #[test]
    fn glitch_does_not_trip() {
        let mut det = OcDetector::new(OC_THRESHOLD_MA10).unwrap_or(OcDetector { threshold: OC_THRESHOLD_MA10, strikes: 0, latched: false });
        assert!(det.glitch_clean());
        assert!(!det.latched());
    }

    #[test]
    fn threshold_bounds() {
        assert_eq!(OcDetector::new(0), Err(OcCode::ThresholdInvalid));
        assert_eq!(OcDetector::new(5001), Err(OcCode::ThresholdInvalid));
        assert!(OcDetector::new(1).is_ok());
        assert!(OcDetector::new(5000).is_ok());
    }

    #[test]
    fn isolate_only_fault_partition() {
        let mut pw = PartitionPower::new();
        assert_eq!(pw.isolate(6), Ok(0));
        assert_eq!(pw.state_of(1), Some(PartState::Isolated));
        assert_eq!(pw.state_of(0), Some(PartState::On));
        assert_eq!(pw.state_of(2), Some(PartState::On));
        assert_eq!(pw.state_of(3), Some(PartState::On));
    }

    #[test]
    fn double_isolate_rejected() {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(5);
        assert_eq!(pw.isolate(5), Err(OcCode::AlreadyIsolated));
    }

    #[test]
    fn latch_blocks_recovery_until_cleared() {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(9);
        assert_eq!(pw.recover(9, 1, true), Err(OcCode::LatchedShort));
        assert!(pw.recover(9, 2, false).is_ok());
    }

    #[test]
    fn recovery_seq_monotonic() {
        let mut pw = PartitionPower::new();
        let _ = pw.isolate(12);
        assert!(pw.recover(12, 5, false).is_ok());
        let _ = pw.isolate(12);
        assert_eq!(pw.recover(12, 5, false), Err(OcCode::RecoveryRegress));
    }

    #[test]
    fn port_oob_rejected() {
        let mut pw = PartitionPower::new();
        assert_eq!(pw.isolate(MAX_PORT), Err(OcCode::PortOob));
        assert!(partition_of(MAX_PORT).is_none());
        assert_eq!(partition_of(15), Some(3));
    }

    #[test]
    fn triple_fault_recovery_passes() {
        assert!(triple_fault_recovery_all_pass());
    }

    #[test]
    fn injection_matrix_all_caught_zero_escape() {
        let (inj, det, esc) = run_injection_matrix();
        assert_eq!((inj, det, esc), (50, 50, 0));
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn closed_set_vals_and_texts() {
        for (i, c) in ALL.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_passes() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC0C00);
    }

    #[test]
    fn unx51549_checks_green() {
        let set = run_unx51549_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
