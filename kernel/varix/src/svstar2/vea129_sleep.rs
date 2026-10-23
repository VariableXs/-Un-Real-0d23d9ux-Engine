//! VE-F0129 · 休眠/唤醒渲染状态保持（VE-A 域 · A06 环境与系统集成组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0129`
//!
//! **职责定位（锚点原文）**：系统休眠唤醒后的渲染状态保持（设备丢失处理+场景恢复），
//! 唤醒首帧预算（可感知延迟承诺），唤醒失败降级。
//!
//! **铁律**：
//! 1. **休眠保持**——休眠前完整落盘，唤醒后可恢复。
//! 2. **首帧预算**——唤醒首帧延迟有可感知承诺（分档）。
//! 3. **失败降级**——恢复失败走重初始化路径，不硬等。
//! 4. **状态对拍**——恢复前后状态指纹一致（漂移即报）。
//!
//! **设备丢失语义**：休眠期间 GPU 设备可能丢失（驱动重置/固件重刷），唤醒后
//! 的第一件事是问「设备还在不在」——不在就丢弃旧句柄走重建，绝不拿旧句柄
//! 硬渲染（那会静默出错或炸驱动）。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8C7（休眠唤醒状态保持）。
pub const SEG_BASE: u16 = 0xE8C7;
/// 状态指纹不一致（恢复对拍失败）。
pub const XC_STATE_DRIFT: u16 = SEG_BASE | 0x01;
/// 设备丢失未处置。
pub const XC_DEVICE_LOST: u16 = SEG_BASE | 0x02;
/// 首帧超时（超可感知预算）。
pub const XC_FIRST_FRAME: u16 = SEG_BASE | 0x03;
/// 恢复失败（走重初始化）。
pub const XC_RESTORE_FAIL: u16 = SEG_BASE | 0x04;
/// 非法状态迁移（未休眠即唤醒等）。
pub const XC_BAD_PHASE: u16 = SEG_BASE | 0x05;
/// 快照容量超限。
pub const XC_SNAPSHOT_FULL: u16 = SEG_BASE | 0x06;

/// 休眠/唤醒相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepPhase {
    /// 正常运行。
    Awake,
    /// 休眠中（快照已落盘）。
    Sleeping,
    /// 唤醒恢复中（设备重建+状态回灌）。
    Waking,
    /// 降级运行（恢复失败后的重初始化路径）。
    Degraded,
}

/// 首帧延迟分档（可感知承诺——数字即承诺）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FirstFrameTier {
    /// 即时档：≤33ms（一帧内）。
    Instant,
    /// 快速档：≤100ms（可感知但不烦）。
    Fast,
    /// 宽松档：≤500ms（复杂场景重建）。
    Relaxed,
}

impl FirstFrameTier {
    /// 档位延迟上限（毫秒）。
    pub fn budget_ms(&self) -> u32 {
        match self {
            FirstFrameTier::Instant => 33,
            FirstFrameTier::Fast => 100,
            FirstFrameTier::Relaxed => 500,
        }
    }
}

/// 单个渲染资源的休眠快照条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResSnapshot {
    /// 资源标识。
    pub id: u32,
    /// 资源类别（0=纹理 1=缓冲 2=管道 3=表面）。
    pub kind: u8,
    /// 状态指纹（恢复对拍基准）。
    pub fingerprint: u32,
    /// 字节大小（恢复预算用）。
    pub bytes: u64,
    /// 是否关键资源（关键资源恢复失败=整体降级）。
    pub critical: bool,
}

/// 设备丢失证据（唤醒后问设备的第一手答案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceProbe {
    /// 设备是否存活。
    pub alive: bool,
    /// 驱动代数（变了=固件重刷过，旧句柄全废）。
    pub driver_gen: u32,
    /// 探测耗时（微秒）。
    pub probe_us: u32,
}

/// 唤醒结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WakeOutcome {
    /// 首帧实际耗时（毫秒）。
    pub first_frame_ms: u32,
    /// 恢复资源数。
    pub restored: u32,
    /// 重建资源数（设备丢失后重建的）。
    pub rebuilt: u32,
    /// 是否走了降级路径。
    pub degraded: bool,
}

/// 休眠/唤醒状态保持器。
///
/// **状态机**：Awake →(sleep)→ Sleeping →(wake)→ Waking →(ok)→ Awake
///                                    ↘(fail)→ Degraded →(reinit)→ Awake
pub struct SleepKeeper {
    /// 当前相位。
    phase: SleepPhase,
    /// 休眠快照（容量受控）。
    snaps: Vec<ResSnapshot>,
    /// 快照容量上限。
    capacity: usize,
    /// 首帧延迟档位。
    tier: FirstFrameTier,
    /// 最近一次唤醒结果。
    last_wake: Option<WakeOutcome>,
    /// 状态指纹累积（对拍基准：快照序的 XOR 折叠）。
    snap_fingerprint: u32,
    /// 最近诊断码。
    last_code: u16,
    /// 唤醒失败次数（降级决策输入）。
    wake_fails: u32,
    /// 休眠次数。
    sleeps: u32,
    /// 设备丢失遭遇次数（统计诚实——不隐藏）。
    device_losses: u32,
}

impl SleepKeeper {
    /// 建保持器（容量>0，档位预算必须为正）。
    pub fn new(capacity: usize, tier: FirstFrameTier) -> Result<Self, u16> {
        if capacity == 0 {
            return Err(XC_SNAPSHOT_FULL);
        }
        if tier.budget_ms() == 0 {
            return Err(XC_FIRST_FRAME);
        }
        Ok(SleepKeeper {
            phase: SleepPhase::Awake,
            snaps: Vec::new(),
            capacity,
            tier,
            last_wake: None,
            snap_fingerprint: 0,
            last_code: 0,
            wake_fails: 0,
            sleeps: 0,
            device_losses: 0,
        })
    }

    /// 当前相位。
    pub fn phase(&self) -> SleepPhase {
        self.phase
    }

    /// 最近诊断码。
    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 登记资源快照（休眠前逐条落盘）。
    pub fn capture(&mut self, snap: ResSnapshot) -> Result<(), u16> {
        if self.phase != SleepPhase::Awake && self.phase != SleepPhase::Sleeping {
            self.last_code = XC_BAD_PHASE;
            return Err(XC_BAD_PHASE);
        }
        if self.snaps.len() >= self.capacity {
            self.last_code = XC_SNAPSHOT_FULL;
            return Err(XC_SNAPSHOT_FULL);
        }
        // 重复 id = 覆盖语义（后登记的为准，但指纹折叠要先撤销旧贡献）。
        if let Some(i) = self.snaps.iter().position(|s| s.id == snap.id) {
            self.snap_fingerprint ^= self.snaps[i].fingerprint ^ snap.fingerprint;
            self.snaps[i] = snap;
            self.last_code = 0;
            return Ok(());
        }
        self.snap_fingerprint ^= snap.fingerprint;
        self.snaps.push(snap);
        self.last_code = 0;
        Ok(())
    }

    /// 快照指纹（对拍基准）。
    pub fn snapshot_fingerprint(&self) -> u32 {
        self.snap_fingerprint
    }

    /// 快照条目数。
    pub fn snapshot_len(&self) -> usize {
        self.snaps.len()
    }

    /// 进入休眠：快照收尾+相位迁移。
    ///
    /// 空快照允许休眠（无资源场景合法），唤醒时零资源对零指纹自然通过。
    pub fn sleep(&mut self) -> Result<(), u16> {
        match self.phase {
            SleepPhase::Awake => {
                self.phase = SleepPhase::Sleeping;
                self.sleeps += 1;
                self.last_code = 0;
                Ok(())
            }
            _ => {
                self.last_code = XC_BAD_PHASE;
                Err(XC_BAD_PHASE)
            }
        }
    }

    /// 唤醒：设备探测 → 快照恢复 → 首帧渲染 → 对拍。
    ///
    /// `probe` 是唤醒后对设备的第一手探测结果；`restore_fn(i, snap)`
    /// 逐条恢复资源并返回恢复后指纹（Ok）或失败码（Err）。
    /// `first_frame_ms` 是首帧实际耗时——超预算即报 XC_FIRST_FRAME
    /// （诚实记账：超了就超了，不四舍五入）。
    pub fn wake<F>(
        &mut self,
        probe: DeviceProbe,
        mut restore_fn: F,
        first_frame_ms: u32,
    ) -> Result<WakeOutcome, u16>
    where
        F: FnMut(usize, &ResSnapshot) -> Result<u32, u16>,
    {
        if self.phase != SleepPhase::Sleeping {
            self.last_code = XC_BAD_PHASE;
            return Err(XC_BAD_PHASE);
        }
        self.phase = SleepPhase::Waking;

        // 设备丢失处置：句柄全废 → 重建计数（critical 资源失败会走降级）。
        let lost = !probe.alive;
        if lost {
            self.device_losses += 1;
        }

        let mut restored: u32 = 0;
        let mut rebuilt: u32 = 0;
        let mut critical_fail = false;
        let mut fp: u32 = 0;
        for i in 0..self.snaps.len() {
            let snap = self.snaps[i];
            match restore_fn(i, &snap) {
                Ok(f) => {
                    fp ^= f;
                    if lost {
                        rebuilt += 1;
                    } else {
                        restored += 1;
                    }
                    // 恢复后指纹漂移：关键资源漂移=降级信号。
                    if f != snap.fingerprint && snap.critical {
                        critical_fail = true;
                    }
                }
                Err(_) => {
                    if snap.critical {
                        critical_fail = true;
                    }
                }
            }
        }

        // 恢复失败 / 关键资源漂移 → 降级路径（重初始化），不硬撑。
        if critical_fail {
            self.wake_fails += 1;
            self.phase = SleepPhase::Degraded;
            self.last_code = XC_RESTORE_FAIL;
            let outcome = WakeOutcome {
                first_frame_ms,
                restored,
                rebuilt,
                degraded: true,
            };
            self.last_wake = Some(outcome);
            return Err(XC_RESTORE_FAIL);
        }

        // 状态对拍：恢复指纹折叠必须等于快照指纹折叠。
        if fp != self.snap_fingerprint {
            self.wake_fails += 1;
            self.phase = SleepPhase::Degraded;
            self.last_code = XC_STATE_DRIFT;
            let outcome = WakeOutcome {
                first_frame_ms,
                restored,
                rebuilt,
                degraded: true,
            };
            self.last_wake = Some(outcome);
            return Err(XC_STATE_DRIFT);
        }

        // 首帧预算核验（诚实记账，超预算显性报但状态仍为 Awake——
        // 超时是性能问题不是正确性问题，两者要分开）。
        let degraded = false;
        if first_frame_ms > self.tier.budget_ms() {
            self.phase = SleepPhase::Awake;
            self.last_code = XC_FIRST_FRAME;
            let outcome = WakeOutcome {
                first_frame_ms,
                restored,
                rebuilt,
                degraded,
            };
            self.last_wake = Some(outcome);
            return Err(XC_FIRST_FRAME);
        }

        self.phase = SleepPhase::Awake;
        self.last_code = 0;
        let outcome = WakeOutcome {
            first_frame_ms,
            restored,
            rebuilt,
            degraded,
        };
        self.last_wake = Some(outcome);
        Ok(outcome)
    }

    /// 降级态再初始化（Degraded → Awake 的唯一出口）。
    pub fn reinit(&mut self) -> Result<(), u16> {
        if self.phase != SleepPhase::Degraded {
            self.last_code = XC_BAD_PHASE;
            return Err(XC_BAD_PHASE);
        }
        // 再初始化 = 清快照重新积累（旧句柄已不可信）。
        self.snaps.clear();
        self.snap_fingerprint = 0;
        self.phase = SleepPhase::Awake;
        self.last_code = 0;
        Ok(())
    }

    /// 最近唤醒结果。
    pub fn last_wake(&self) -> Option<WakeOutcome> {
        self.last_wake
    }

    /// 唤醒失败次数。
    pub fn wake_fails(&self) -> u32 {
        self.wake_fails
    }

    /// 休眠次数。
    pub fn sleeps(&self) -> u32 {
        self.sleeps
    }

    /// 设备丢失遭遇次数。
    pub fn device_losses(&self) -> u32 {
        self.device_losses
    }

    /// 首帧档位。
    pub fn tier(&self) -> FirstFrameTier {
        self.tier
    }

    /// 读屏行（相位显式，数值经状态查询接口）。
    pub fn status_line(&self) -> &'static str {
        match self.phase {
            SleepPhase::Awake => "sleep:awake",
            SleepPhase::Sleeping => "sleep:sleeping",
            SleepPhase::Waking => "sleep:waking",
            SleepPhase::Degraded => "sleep:degraded",
        }
    }
}

/// 自检：休眠保持、首帧预算、失败降级、状态对拍、设备丢失。
pub fn run_vea129_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn snap(id: u32, fp: u32, critical: bool) -> ResSnapshot {
        ResSnapshot {
            id,
            kind: 0,
            fingerprint: fp,
            bytes: 4096,
            critical,
        }
    }

    fn alive() -> DeviceProbe {
        DeviceProbe {
            alive: true,
            driver_gen: 7,
            probe_us: 50,
        }
    }

    let mut set = CheckSet::new("vea129_sleepkeeper");

    // C129-01 合法建 + 默认 Awake
    {
        let k = SleepKeeper::new(8, FirstFrameTier::Fast);
        let ok = match k {
            Ok(k) => k.phase() == SleepPhase::Awake,
            Err(_) => false,
        };
        chk(&mut set, "C129-01 合法建", ok, "默认相位非 Awake");
    }

    // C129-02 容量零拒绝
    {
        let r = SleepKeeper::new(0, FirstFrameTier::Fast);
        chk(
            &mut set,
            "C129-02 容量零拒",
            r.err() == Some(XC_SNAPSHOT_FULL),
            "零容量被放行",
        );
    }

    // C129-03 休眠保持：快照落盘+相位迁移
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.capture(snap(2, 0xBB, false));
        let r = k.sleep();
        let ok = r.is_ok() && k.phase() == SleepPhase::Sleeping && k.snapshot_len() == 2;
        chk(&mut set, "C129-03 休眠保持", ok, "快照未落盘或相位错");
    }

    // C129-04 重复 id 覆盖且指纹撤销旧贡献
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.capture(snap(1, 0xCC, true));
        let ok = k.snapshot_len() == 1 && k.snapshot_fingerprint() == 0xCC;
        chk(&mut set, "C129-04 覆盖对拍", ok, "覆盖后指纹错");
    }

    // C129-05 正常唤醒：对拍通过+首帧达标
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.capture(snap(2, 0xBB, false));
        let _ = k.sleep();
        let r = k.wake(alive(), |_, s| Ok(s.fingerprint), 40);
        let ok = r.is_ok()
            && k.phase() == SleepPhase::Awake
            && k.last_wake().map(|w| w.restored == 2 && !w.degraded).unwrap_or(false);
        chk(&mut set, "C129-05 正常唤醒", ok, "唤醒后对拍或计数错");
    }

    // C129-06 首帧超预算：显性报但不降级
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Instant).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.sleep();
        let r = k.wake(alive(), |_, s| Ok(s.fingerprint), 200);
        let ok = r.err() == Some(XC_FIRST_FRAME) && k.phase() == SleepPhase::Awake;
        chk(&mut set, "C129-06 首帧预算", ok, "超时未显性或误降级");
    }

    // C129-07 状态漂移：对拍失败 → 降级
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.sleep();
        let r = k.wake(alive(), |_, _| Ok(0x99), 40);
        let ok = r.err() == Some(XC_STATE_DRIFT) && k.phase() == SleepPhase::Degraded;
        chk(&mut set, "C129-07 状态对拍", ok, "漂移未检出或未降级");
    }

    // C129-08 关键资源恢复失败 → 降级
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.sleep();
        let r = k.wake(alive(), |_, _| Err(XC_RESTORE_FAIL), 40);
        let ok = r.err() == Some(XC_RESTORE_FAIL) && k.phase() == SleepPhase::Degraded;
        chk(&mut set, "C129-08 失败降级", ok, "关键失败未降级");
    }

    // C129-09 设备丢失：重建计数+丢失统计
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.sleep();
        let probe = DeviceProbe {
            alive: false,
            driver_gen: 8,
            probe_us: 90,
        };
        let r = k.wake(probe, |_, s| Ok(s.fingerprint), 40);
        let ok = r.is_ok()
            && k.device_losses() == 1
            && k.last_wake().map(|w| w.rebuilt == 1).unwrap_or(false);
        chk(&mut set, "C129-09 设备丢失", ok, "丢失未统计或未重建");
    }

    // C129-10 降级再初始化：清快照回 Awake
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let _ = k.sleep();
        let _ = k.wake(alive(), |_, _| Err(XC_RESTORE_FAIL), 40);
        let r = k.reinit();
        let ok = r.is_ok()
            && k.phase() == SleepPhase::Awake
            && k.snapshot_len() == 0
            && k.snapshot_fingerprint() == 0;
        chk(&mut set, "C129-10 再初始化", ok, "再初始化未清态");
    }

    // C129-11 非法迁移：未休眠即唤醒被拒
    {
        let mut k = SleepKeeper::new(8, FirstFrameTier::Fast).unwrap();
        let r = k.wake(alive(), |_, s| Ok(s.fingerprint), 10);
        let ok = r.err() == Some(XC_BAD_PHASE);
        chk(&mut set, "C129-11 相位守卫", ok, "非法唤醒被放行");
    }

    // C129-12 快照容量超限显性
    {
        let mut k = SleepKeeper::new(1, FirstFrameTier::Fast).unwrap();
        let _ = k.capture(snap(1, 0xAA, true));
        let r = k.capture(snap(2, 0xBB, false));
        let ok = r.err() == Some(XC_SNAPSHOT_FULL);
        chk(&mut set, "C129-12 容量上限", ok, "超限未报");
    }

    set
}
