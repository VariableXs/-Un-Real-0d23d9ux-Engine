//! CGPU-F0386 · 账户探针（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 探针：单账户深度观测（选中账户的逐帧决策全记录——调试模式）；探针
//!   开销（深度记录的开销量化与上限）；观测安全（只读不改预算——观测
//!   不影响被观测）。判据：深度记录、开销上限、只读、判据。
//!
//! 设计纪律：
//! - 探针是旁路镜像：记录面拿到的永远是决策副本，无写路径（只读铁律）。
//! - 深度记录环形缓冲定容，开销以记录条数计量，越上限自动降采样。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0AE0 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0AE0，全仓独占）。
pub const SEG_BASE: u16 = 0x0AE0;

pub const E_TARGET_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_CAP_EXCEEDED: u16 = SEG_BASE + 2;
pub const E_WRITE_DENIED: u16 = SEG_BASE + 3;
pub const E_SAMPLING_DEGRADED: u16 = SEG_BASE + 4;

/// 深度记录容量上限（条）。
pub const CAP_MAX: usize = 512;
/// 开销上限：每帧最多记录条数（越线即降采样）。
pub const PER_FRAME_CAP: usize = 4;

/// 单帧深度决策记录（只读副本形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameDecision {
    pub frame: u64,
    /// 申请额度（千分位）。
    pub requested_pm: u32,
    /// 实际授予（千分位）。
    pub granted_pm: u32,
    /// 结果（0 准 1 拒 2 降档）。
    pub verdict: u8,
    /// 仲裁理由码（与决策流 reason 同源）。
    pub reason_code: u16,
}

/// 探针（单账户调试模式）。
pub struct AccountProbe {
    target: u32,
    ring: Vec<FrameDecision>,
    /// 采样计数（开销账）。
    pub samples: u64,
    /// 降采样触发次数。
    pub degradations: u64,
    /// 每帧当前帧已记条数。
    frame_count: usize,
}

impl AccountProbe {
    pub fn new(target: u32) -> Self {
        AccountProbe { target, ring: Vec::new(), samples: 0, degradations: 0, frame_count: 0 }
    }

    pub fn target(&self) -> u32 {
        self.target
    }

    /// 帧边界：换帧清每帧计数。
    pub fn next_frame(&mut self) {
        self.frame_count = 0;
    }

    /// 记录一条（判据「深度记录」+「开销上限」）：
    /// 环形定容 + 每帧限额，越帧限即降采样并计数。
    pub fn record(&mut self, d: FrameDecision) -> Result<(), u16> {
        if d.granted_pm > d.requested_pm && d.verdict == 0 {
            return Err(E_WRITE_DENIED); // 授予>申请且判准——数据面违例
        }
        if self.frame_count >= PER_FRAME_CAP {
            self.degradations += 1;
            return Err(E_SAMPLING_DEGRADED);
        }
        if self.ring.len() >= CAP_MAX {
            self.ring.remove(0);
        }
        self.frame_count += 1;
        self.samples += 1;
        self.ring.push(d);
        Ok(())
    }

    /// 只读铁律（判据「观测安全」）：探针对外只暴露只读切片，
    /// 无任何修改接口（编译层面以 &self 保证）。
    pub fn records(&self) -> &[FrameDecision] {
        &self.ring
    }

    /// 最近 N 条（回看）。
    pub fn tail(&self, n: usize) -> &[FrameDecision] {
        let start = self.ring.len().saturating_sub(n);
        &self.ring[start..]
    }

    /// 探针开销量化：记录条数/容量（千分位）。
    pub fn overhead_permille(&self) -> u32 {
        ((self.ring.len() as u64) * 1000 / CAP_MAX as u64) as u32
    }

    /// 载量告警：>800‰ 提示接近上限。
    pub fn near_cap(&self) -> bool {
        self.overhead_permille() > 800
    }
}

/// 探针注册表：管理多探针 + 目标账户存活核。
pub struct ProbeHub {
    probes: Vec<AccountProbe>,
    known_accounts: Vec<u32>,
}

impl ProbeHub {
    pub fn new() -> Self {
        ProbeHub { probes: Vec::new(), known_accounts: Vec::new() }
    }

    pub fn register_account(&mut self, account: u32) {
        if !self.known_accounts.contains(&account) {
            self.known_accounts.push(account);
        }
    }

    /// 挂探针（目标必须已知——不观测幽灵账户）。
    pub fn attach(&mut self, target: u32) -> Result<(), u16> {
        if !self.known_accounts.contains(&target) {
            return Err(E_TARGET_UNKNOWN);
        }
        if self.probes.iter().any(|p| p.target() == target) {
            return Ok(()); // 幂等
        }
        self.probes.push(AccountProbe::new(target));
        Ok(())
    }

    pub fn probe(&self, target: u32) -> Option<&AccountProbe> {
        self.probes.iter().find(|p| p.target() == target)
    }

    /// 广播一条决策到对应探针（旁路镜像入口——决策面不感知探针存在）。
    pub fn mirror(&mut self, target: u32, d: FrameDecision) -> Result<(), u16> {
        let known = self.probes.iter().any(|p| p.target() == target);
        if !known {
            return Err(E_TARGET_UNKNOWN);
        }
        if let Some(p) = self.probes.iter_mut().find(|p| p.target() == target) {
            p.record(d)
        } else {
            Err(E_TARGET_UNKNOWN)
        }
    }

    /// 帧推进广播（所有探针同步换帧）。
    pub fn next_frame_all(&mut self) {
        for p in self.probes.iter_mut() {
            p.next_frame();
        }
    }

    /// 开销总账：所有探针样本和（判据「开销量化」）。
    pub fn total_samples(&self) -> u64 {
        self.probes.iter().map(|p| p.samples).sum()
    }

    pub fn probe_count(&self) -> usize {
        self.probes.len()
    }
}

impl Default for ProbeHub {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn dec(frame: u64, req: u32, grant: u32, verdict: u8) -> FrameDecision {
        FrameDecision { frame, requested_pm: req, granted_pm: grant, verdict, reason_code: SEG_BASE }
    }

    #[test]
    fn a86_深度记录_环形与回看() {
        let mut p = AccountProbe::new(7);
        for f in 0..600u64 {
            p.next_frame();
            p.record(dec(f, 500, 400, 0)).unwrap();
        }
        assert_eq!(p.records().len(), CAP_MAX);
        assert_eq!(p.records()[0].frame, 88, "oldest evicted");
        assert_eq!(p.tail(3).len(), 3);
        assert_eq!(p.tail(3)[2].frame, 599);
        assert_eq!(p.samples, 600);
        // 开销满载告警。
        assert!(p.near_cap());
        assert_eq!(p.overhead_permille(), 1000);
    }

    #[test]
    fn a86_开销上限_每帧限额降采样() {
        let mut p = AccountProbe::new(1);
        p.next_frame();
        for _ in 0..PER_FRAME_CAP {
            p.record(dec(1, 300, 200, 0)).unwrap();
        }
        // 第 5 条同帧：降采样拒并计数。
        assert_eq!(p.record(dec(1, 300, 200, 0)).unwrap_err(), E_SAMPLING_DEGRADED);
        assert_eq!(p.degradations, 1);
        assert_eq!(p.samples, PER_FRAME_CAP as u64);
        // 换帧后恢复。
        p.next_frame();
        assert!(p.record(dec(2, 300, 200, 0)).is_ok());
        assert_eq!(p.degradations, 1);
        // 常量自检。
        assert_eq!(PER_FRAME_CAP, 4);
    }

    #[test]
    fn a86_只读_观测不改被观测() {
        let mut hub = ProbeHub::new();
        hub.register_account(3);
        hub.attach(3).unwrap();
        // 幂等挂载。
        hub.attach(3).unwrap();
        assert_eq!(hub.probe_count(), 1);
        // 镜像 5 帧（每帧先推进）。
        for f in 0..5u64 {
            hub.next_frame_all();
            hub.mirror(3, dec(f, 800, 800, 0)).unwrap();
        }
        // 探针只读切片无写路径：records() 返回 &[FrameDecision]——
        // 只读断言先取值，不与后续可变调用冲突。
        let (len, last_req) = {
            let probe = hub.probe(3).unwrap();
            (probe.records().len(), probe.records()[4].requested_pm)
        };
        assert_eq!(len, 5);
        assert_eq!(last_req, 800);
        // 数据面违例：授予>申请且判准。
        assert_eq!(hub.mirror(3, dec(9, 100, 200, 0)).unwrap_err(), E_WRITE_DENIED);
        // 幽灵账户拒。
        assert_eq!(hub.mirror(99, dec(9, 100, 100, 0)).unwrap_err(), E_TARGET_UNKNOWN);
        assert_eq!(hub.attach(99).unwrap_err(), E_TARGET_UNKNOWN);
        assert_eq!(hub.total_samples(), 5);
    }
}
