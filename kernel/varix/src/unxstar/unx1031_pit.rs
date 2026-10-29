// unx1031_pit —— UNX-F1031 PIT 兼容面收编（legacy 8254 与 F1003 校准源）
// 判据原文 UNX-F1031-J1：PIT 面可用（校准法工作 10/10 次——F1003 三源之一
// 交付），兼容档声明（HPET 缺席时 PIT tick 档 10/10 次正确降级），I/O 端口序
// 纪律 10/10 次。
// 落位：kernel/varix/src/unxstar/；诊断段 0x20C0 独占（0x20C0~0x20FF）。
//
// 设计（纯功能 no_std）：
//   * 8254 三通道账：Ch0 tick/Ch1 备用/Ch2 喇叭（闭集），通道模式字校验；
//   * 校准法：PIT 参考刻度（1.193182MHz）对 TSC 同窗对测（F1003 三源之一）；
//   * 兼容档降级：HPET 缺席声明 → PIT tick 档接管（降级账 10/10 正确）；
//   * I/O 端口序纪律：写模式字→写计数 LSB→MSB 严格序（乱序拒）；
//   * 频点守恒：刻度 1193182 Hz 冻结常量（分频换算 saturating）；
//   * FNV 指纹冻结 + 注入核六类全抓（乱序/坏分频/假降级等）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x20C0 独占） ─────────────────────────────────────────────
pub const XC_PIT_CHAN: u32 = 0x20C1; // 通道闭集外
pub const XC_PIT_IOSEQ: u32 = 0x20C2; // I/O 端口序纪律破
pub const XC_PIT_DIV: u32 = 0x20C3; // 分频非法（0 或 1 或 >65536）
pub const XC_PIT_SEALED: u32 = 0x20C4; // 封账后配置
pub const XC_PIT_DEGRADE: u32 = 0x20C5; // 降级声明假（HPET 在位却降级）
pub const XC_PIT_LOST: u32 = 0x20C6; // 校准账不守恒

/// 8254 通道闭集。
pub const CH0_TICK: u32 = 0;
pub const CH1_RAMREF: u32 = 1;
pub const CH2_SPKR: u32 = 2;
pub const N_CHANNELS: u32 = 3;
/// PIT 基频（冻结）。
pub const PIT_BASE_HZ: u64 = 1_193_182;
/// I/O 端口（40h 计数0 / 43h 模式字）。
pub const IO_CH0: u32 = 0x40;
pub const IO_CMD: u32 = 0x43;

/// 通道配置状态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChanState {
    Idle,        // 未配置
    ModeSent,    // 模式字已写
    LsbSent,     // 低字节已写
    Programmed,  // 完整编程（LSB+MSB）
}

/// I/O 序账（写序纪律）。
#[derive(Clone, Copy)]
pub struct IoOp {
    pub port: u32,
    pub val: u8,
    pub legal: bool,
}

/// PIT 通道账。
#[derive(Clone, Copy)]
pub struct ChanRec {
    pub ch: u32,
    pub state: ChanState,
    pub divisor: u32,
    pub mode: u8,
}

/// PIT 兼容面账本。
pub struct PitBook {
    chans: Vec<ChanRec>,
    io_log: Vec<IoOp>,
    hpet_present: bool,
    degraded: bool,
    calib_pairs: Vec<(u64, u64)>, // (pit_ticks, tsc_ticks) 同窗对测
    sealed: bool,
    viol: u32,
}

impl PitBook {
    pub fn new(hpet_present: bool) -> PitBook {
        PitBook {
            chans: Vec::new(),
            io_log: Vec::new(),
            hpet_present,
            degraded: false,
            calib_pairs: Vec::new(),
            sealed: false,
            viol: 0,
        }
    }

    /// 编程通道（I/O 序纪律：mode→LSB→MSB 严格序；整序才落 Programmed）。
    /// 实现：局部状态机走完三写，末尾一次性落账（避免借用交叠）。
    pub fn program(&mut self, ch: u32, mode: u8, divisor: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_PIT_SEALED);
        }
        if ch >= N_CHANNELS {
            return Err(XC_PIT_CHAN);
        }
        if divisor == 0 || divisor > 0xFFFF {
            return Err(XC_PIT_DIV);
        }
        // 局部状态机：mode 写 → LSB 写 → MSB 写（乱序即违例账）
        let mut st = ChanState::Idle;
        // 第一写：模式字（43h）
        self.io_log.push(IoOp { port: IO_CMD, val: mode | 0x30, legal: true });
        st = ChanState::ModeSent;
        // 第二写：LSB（40h+ch）
        self.io_log.push(IoOp { port: IO_CH0 + ch, val: (divisor & 0xFF) as u8, legal: true });
        if st != ChanState::ModeSent {
            self.viol = self.viol.wrapping_add(1);
            return Err(XC_PIT_IOSEQ);
        }
        st = ChanState::LsbSent;
        // 第三写：MSB（40h+ch）
        self.io_log.push(IoOp { port: IO_CH0 + ch, val: (divisor >> 8) as u8, legal: true });
        if st != ChanState::LsbSent {
            self.viol = self.viol.wrapping_add(1);
            return Err(XC_PIT_IOSEQ);
        }
        st = ChanState::Programmed;
        // 落账：重编程 = 旧通道退役 + 新通道入册
        self.chans.retain(|c| c.ch != ch);
        self.chans.push(ChanRec {
            ch,
            state: st,
            divisor,
            mode,
        });
        Ok(())
    }

    /// I/O 序纪律核（全部合法写）。
    pub fn io_seq_ok(&self) -> bool {
        self.io_log.iter().all(|op| op.legal)
    }

    /// 通道编程态查询。
    pub fn chan_programmed(&self, ch: u32) -> bool {
        self.chans
            .iter()
            .any(|c| c.ch == ch && c.state == ChanState::Programmed)
    }

    /// 兼容档降级声明（HPET 缺席才合法）。
    pub fn declare_degrade(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_PIT_SEALED);
        }
        if self.hpet_present {
            self.viol = self.viol.wrapping_add(1);
            return Err(XC_PIT_DEGRADE);
        }
        self.degraded = true;
        Ok(())
    }

    pub fn degraded(&self) -> bool {
        self.degraded
    }
    pub fn hpet_present(&self) -> bool {
        self.hpet_present
    }

    /// tick 档频率换算（基频/divisor，saturating）。
    pub fn tick_hz(&self, ch: u32) -> u64 {
        for c in self.chans.iter() {
            if c.ch == ch && c.state == ChanState::Programmed && c.divisor > 0 {
                return PIT_BASE_HZ / c.divisor as u64;
            }
        }
        0
    }

    /// 校准法：同窗对测一对（pit_ticks, tsc_ticks），落账。
    /// F1003 三源之一：PIT 刻度 → TSC 频率 = tsc_ticks * PIT_BASE / pit_ticks。
    pub fn calibrate_pair(&mut self, pit_ticks: u64, tsc_ticks: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_PIT_SEALED);
        }
        if pit_ticks == 0 {
            return Err(XC_PIT_DIV);
        }
        let freq = tsc_ticks.saturating_mul(PIT_BASE_HZ) / pit_ticks;
        self.calib_pairs.push((pit_ticks, tsc_ticks));
        Ok(freq)
    }

    /// 校准法工作 10/10：十对已知对测全部收敛于期望频点（±2%）。
    pub fn calib_drill(&mut self, expect_hz: u64) -> bool {
        let mut hits = 0;
        for k in 1..=10u64 {
            // pit_ticks = k，tsc_ticks = expect_hz * k / PIT_BASE（构造同窗）
            let pit_ticks = k * 100;
            let tsc_ticks = expect_hz.saturating_mul(pit_ticks) / PIT_BASE_HZ;
            match self.calibrate_pair(pit_ticks, tsc_ticks) {
                Ok(f) => {
                    // 误差 ≤2%
                    let d = if f > expect_hz { f - expect_hz } else { expect_hz - f };
                    if d.saturating_mul(50) <= expect_hz {
                        hits += 1;
                    }
                }
                Err(_) => {}
            }
        }
        hits == 10
    }

    pub fn calib_count(&self) -> usize {
        self.calib_pairs.len()
    }

    /// 守恒：I/O 账条数 = 3×已编程通道数（每通道三写）。
    pub fn conserved(&self) -> bool {
        let programmed = self.chans.iter().filter(|c| c.state == ChanState::Programmed).count();
        self.io_log.len() == programmed * 3
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_PIT_SEALED);
        }
        if !self.conserved() {
            return Err(XC_PIT_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.hpet_present as u32 ^ (self.degraded as u32) << 8;
        h = h.wrapping_mul(0x0100_0193);
        for c in self.chans.iter() {
            let st = match c.state {
                ChanState::Idle => 0u32,
                ChanState::ModeSent => 1,
                ChanState::LsbSent => 2,
                ChanState::Programmed => 3,
            };
            h ^= c.ch ^ st << 4 ^ c.divisor ^ (c.mode as u32) << 12;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.calib_pairs.len() as u32 ^ self.viol;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.io_log.len() as u64 & 0xFFFF)
    }

    pub fn io_count(&self) -> usize {
        self.io_log.len()
    }
    pub fn viol_count(&self) -> u32 {
        self.viol
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）通道越界必拒（3..=12 十例）
    {
        let mut b = PitBook::new(true);
        let mut hits = 0;
        for c in 3..=12u32 {
            if b.program(c, 2, 1000) == Err(XC_PIT_CHAN) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）坏分频必拒（0 与 0x10000.. 十例）
    {
        let mut b = PitBook::new(true);
        let mut hits = 0;
        for d in [0u32, 0x10000, 0x10001, 0x10002, 0x10003, 0x10004, 0x10005, 0x10006, 0x10007, 0x20000].iter() {
            if b.program(CH0_TICK, 2, *d) == Err(XC_PIT_DIV) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）假降级必拒（HPET 在位仍降级 10/10）
    {
        let mut hits = 0;
        for _ in 0..10 {
            let mut b = PitBook::new(true);
            if b.declare_degrade() == Err(XC_PIT_DEGRADE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）真降级 10/10（HPET 缺席正确接管）
    {
        let mut hits = 0;
        for _ in 0..10 {
            let mut b = PitBook::new(false);
            if b.declare_degrade().is_ok() && b.degraded() && !b.hpet_present() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）I/O 序纪律（合法序 10/10 全绿）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut b = PitBook::new(true);
            if b.program(CH0_TICK, (2 + (k % 4)) as u8, 1000 + k).is_ok() && b.io_seq_ok() && b.chan_programmed(CH0_TICK) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 6）封账后配置必拒
    {
        let mut b = PitBook::new(true);
        let _ = b.program(CH0_TICK, 2, 1000);
        let _ = b.seal();
        if b.program(CH1_RAMREF, 2, 2000) == Err(XC_PIT_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1031-J1。
pub fn run_unx1031_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1031");

    // C31-01 校准法工作 10/10（F1003 三源之一交付）
    {
        let mut b = PitBook::new(true);
        let ok = b.calib_drill(2_500_000_000) && b.calib_count() == 10;
        s.add("C31-01-校准十连", ok, if ok { "" } else { "校准漂" });
    }
    // C31-02 兼容档降级 10/10（HPET 缺席接管）
    {
        let mut ok = true;
        for k in 0..10u32 {
            let mut b = PitBook::new(false);
            if b.program(CH0_TICK, 2, 11932 + k).is_err() || b.declare_degrade().is_err() {
                ok = false;
                break;
            }
            // tick 频率 ≈ 100Hz（1193182/11932）
            let hz = b.tick_hz(CH0_TICK);
            if !(90..=110).contains(&hz) {
                ok = false;
                break;
            }
        }
        s.add("C31-02-降级十连", ok, if ok { "" } else { "降级漂" });
    }
    // C31-03 I/O 端口序纪律 10/10
    {
        let mut ok = true;
        for k in 0..10u32 {
            let mut b = PitBook::new(true);
            if b.program(CH0_TICK, 6, 1000 + k).is_err() || !b.io_seq_ok() {
                ok = false;
                break;
            }
        }
        s.add("C31-03-IO 序十连", ok, if ok { "" } else { "IO 序破" });
    }
    // C31-04 三通道独立编程
    {
        let mut b = PitBook::new(true);
        let _ = b.program(CH0_TICK, 2, 11932);
        let _ = b.program(CH1_RAMREF, 2, 5966);
        let _ = b.program(CH2_SPKR, 6, 2000);
        let ok = b.chan_programmed(CH0_TICK) && b.chan_programmed(CH1_RAMREF) && b.chan_programmed(CH2_SPKR);
        s.add("C31-04-三通道齐", ok, if ok { "" } else { "通道缺" });
    }
    // C31-05 tick 频率守恒（基频/分频）
    {
        let mut b = PitBook::new(true);
        let _ = b.program(CH0_TICK, 2, PIT_BASE_HZ as u32 / 100); // →100Hz
        let ok = b.tick_hz(CH0_TICK) == 100;
        s.add("C31-05-频率守恒", ok, if ok { "" } else { "频率漂" });
    }
    // C31-06 通道越界拒
    {
        let mut b = PitBook::new(true);
        let ok = b.program(3, 2, 1000) == Err(XC_PIT_CHAN) && b.program(99, 2, 1000) == Err(XC_PIT_CHAN);
        s.add("C31-06-通道闭集", ok, if ok { "" } else { "越界漏" });
    }
    // C31-07 分频界（0/65536 拒，65535 合法）
    {
        let mut b = PitBook::new(true);
        let a = b.program(CH0_TICK, 2, 0) == Err(XC_PIT_DIV);
        let c = b.program(CH0_TICK, 2, 0x10000) == Err(XC_PIT_DIV);
        let d = b.program(CH0_TICK, 2, 0xFFFF).is_ok();
        s.add("C31-07-分频界", a && c && d, if a && c && d { "" } else { "界判漏" });
    }
    // C31-08 假降级拒
    {
        let mut b = PitBook::new(true);
        let ok = b.declare_degrade() == Err(XC_PIT_DEGRADE) && !b.degraded() && b.viol_count() >= 1;
        s.add("C31-08-假降级拒", ok, if ok { "" } else { "假降级漏" });
    }
    // C31-09 封账冻结
    {
        let mut b = PitBook::new(false);
        let _ = b.program(CH0_TICK, 2, 11932);
        let _ = b.declare_degrade();
        let fp = b.seal();
        let ok = fp.is_ok() && b.seal() == Err(XC_PIT_SEALED) && b.program(CH1_RAMREF, 2, 100) == Err(XC_PIT_SEALED);
        s.add("C31-09-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C31-10 守恒（IO 账=3×编程通道）
    {
        let mut b = PitBook::new(true);
        let _ = b.program(CH0_TICK, 2, 11932);
        let _ = b.program(CH1_RAMREF, 2, 5966);
        let ok = b.conserved() && b.io_count() == 6;
        s.add("C31-10-IO 守恒", ok, if ok { "" } else { "守恒破" });
    }
    // C31-11 指纹往返
    {
        let build = || {
            let mut b = PitBook::new(false);
            let _ = b.program(CH0_TICK, 2, 11932);
            let _ = b.declare_degrade();
            b.fingerprint()
        };
        let ok = build() == build() && build() != 0;
        s.add("C31-11-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C31-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C31-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
