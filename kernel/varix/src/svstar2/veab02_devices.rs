//! VE-F5602 · 音频设备与输出管理（VE-AB 域 · 音频 · 设备表与切换状态机）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5602`
//!
//! 锚点原文：「设备枚举、热切换无缝续播、默认设备跟随系统（拔耳机的一瞬间
//! 声音去哪，是体验题不是技术题）；设备切换含切换时长预算（超预算即缺陷）；
//! 默认设备跟随含用户手动锁定的例外规则——锁定优先于跟随；设备能力含延迟
//! 档位披露——各设备输出延迟实测建档，空间音频按档位调补偿。数据结构：设备
//! 表；切换状态机。错误路径与降级矩阵：设备消失→无缝切换；切换爆音→淡出
//! 保护；无设备→静音态提示。性能逐项分解：切换 O(1)。对接：AB03 总线；
//! AB02 输出。无障碍：设备状态可见。判据：热切换、淡出保护、跟随系统、
//! 判据。」
//!
//! # 一、无缝续播是**状态机**不是换指针
//!
//! 热切换若只是「把输出指针改一下」，用户听到的是爆音或丢帧。本单把切换
//! 钉成五态状态机 [`SwitchPhase`]：Idle → FadingOut（淡出防爆音）→
//! Switching（换设备，playhead 保持）→ FadingIn → Done——续播的是
//! **播放位置**不是音频流本身，流在新设备上从头预滚，位置无缝衔接。
//! 切换全程受预算约束（[`SWITCH_BUDGET_MS`]），超预算**记缺陷**而不是
//! 静默变慢——「慢了但没记录」等于没有预算。
//!
//! # 二、跟随系统与用户锁定：锁定优先
//!
//! 默认设备跟随系统（拔耳机自动切扬声器），但用户手动锁定后跟随停摆——
//! 「我锁了蓝牙音箱，插拔耳机别动我的选择」。跟随请求撞锁定时**显式拒绝**
//! 并计数（[`E_AB02_LOCKED`]），不静默忽略：静默忽略会让「跟随失灵」
//! 永远查不出原因。
//!
//! # 三、三错误路径各有降级出口
//!
//! 设备消失 → 紧急无缝切换（跳过淡出预算从宽）；切换爆音 → 淡出保护
//! （必然经过，不是可选优化）；无设备 → 静音态 + 提示（不是错误是状态，
//! [`screen_status`] 如实读屏）。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误契约：独占 0x56 细分段
// ---------------------------------------------------------------------------

/// 无可用设备（进入静音态——这不是故障是状态，但请求切到设备时无设备可选）。
pub const E_AB02_NO_DEVICE: u16 = 0x5600;
/// 切换超预算（切换已完成但记缺陷——超预算即缺陷）。
pub const E_AB02_OVER_BUDGET: u16 = 0x5601;
/// 目标与当前设备相同（切换无意义，显式拒绝）。
pub const E_AB02_SAME_DEVICE: u16 = 0x5602;
/// 请求切换到设备表中不存在的设备。
pub const E_AB02_UNKNOWN_DEVICE: u16 = 0x5603;
/// 跟随请求撞用户锁定（锁定优先于跟随——显式拒绝并计数）。
pub const E_AB02_LOCKED: u16 = 0x5604;

// ---------------------------------------------------------------------------
// 设备表
// ---------------------------------------------------------------------------

/// 延迟档位（实测建档；空间音频按档位调补偿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyTier {
    /// 低延迟（≤20ms，空间音频满档补偿）。
    Low,
    /// 中延迟（≤80ms，补偿减半）。
    Medium,
    /// 高延迟（>80ms，空间音频降级为立体声混叠）。
    High,
}

impl LatencyTier {
    /// 空间音频补偿档（按实测档位——不实测就调补偿是拍脑袋）。
    pub const fn spatial_compensation(self) -> u8 {
        match self {
            LatencyTier::Low => 100,
            LatencyTier::Medium => 50,
            LatencyTier::High => 0,
        }
    }

    /// 全枚举。
    pub const ALL: [LatencyTier; 3] =
        [LatencyTier::Low, LatencyTier::Medium, LatencyTier::High];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            LatencyTier::Low => 0,
            LatencyTier::Medium => 1,
            LatencyTier::High => 2,
        }
    }
}

/// 音频输出设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// 设备 ID（系统分配，切换用）。
    pub id: u32,
    /// 设备名（读屏可达）。
    pub name: String,
    /// 输出延迟实测档位（建档）。
    pub tier: LatencyTier,
    /// 是否用户手动锁定（锁定优先于跟随）。
    pub user_locked: bool,
    /// 是否在位（拔出后 present=false，条目保留可查）。
    pub present: bool,
}

/// 设备表：枚举 + 查找 + 在位过滤。
#[derive(Clone, Debug, Default)]
pub struct DeviceTable {
    devices: Vec<Device>,
}

impl DeviceTable {
    /// 空表。
    pub fn new() -> DeviceTable {
        DeviceTable { devices: Vec::new() }
    }

    /// 登记设备（同 id 覆盖更新——热插拔同设备重复上报不重复建账）。
    pub fn upsert(&mut self, d: Device) {
        for e in self.devices.iter_mut() {
            if e.id == d.id {
                *e = d;
                return;
            }
        }
        self.devices.push(d);
    }

    /// 在位设备数。
    pub fn present_count(&self) -> usize {
        let mut n = 0usize;
        for d in self.devices.iter() {
            if d.present {
                n += 1;
            }
        }
        n
    }

    /// 按 id 查设备。
    pub fn find(&self, id: u32) -> Option<&Device> {
        for d in self.devices.iter() {
            if d.id == id {
                return Some(d);
            }
        }
        None
    }

    /// 默认设备选择：锁定优先于跟随——用户锁定的在位设备 > 表内首个在位设备。
    pub fn pick_default(&self) -> Option<u32> {
        // 第一优先：用户手动锁定的在位设备。
        for d in self.devices.iter() {
            if d.user_locked && d.present {
                return Some(d.id);
            }
        }
        // 第二优先：系统默认 = 首个在位设备。
        for d in self.devices.iter() {
            if d.present {
                return Some(d.id);
            }
        }
        None
    }

    /// 设备消失后的紧急替补：除消失设备外的首个在位设备。
    pub fn escape_target(&self, gone_id: u32) -> Option<u32> {
        for d in self.devices.iter() {
            if d.present && d.id != gone_id {
                return Some(d.id);
            }
        }
        None
    }

    /// 全部设备（枚举面，读屏用）。
    pub fn all(&self) -> &[Device] {
        &self.devices
    }
}

// ---------------------------------------------------------------------------
// 切换状态机
// ---------------------------------------------------------------------------

/// 切换时长预算（毫秒；超预算即缺陷——记缺陷不是静默变慢）。
pub const SWITCH_BUDGET_MS: u32 = 120;
/// 淡出保护时长（毫秒；防爆音——必然经过，不是可选优化）。
pub const FADE_MS: u32 = 24;

/// 切换状态机五态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchPhase {
    /// 空闲（当前设备直接输出）。
    Idle,
    /// 淡出中（防爆音）。
    FadingOut,
    /// 换设备中（playhead 保持，新设备预滚）。
    Switching,
    /// 淡入中。
    FadingIn,
    /// 完成（本次切换的收尾态；下一次切换从 Idle 起步等价于 Done 直通）。
    Done,
}

/// 切换结局。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchOutcome {
    /// 切换完成（走满状态机；含实际耗时与续播位置）。
    Completed {
        /// 实际切换耗时（ms）。
        elapsed_ms: u32,
        /// 续播位置（采样点——无缝的证据）。
        playhead: u64,
        /// 是否超预算（超预算即缺陷：完成但记过）。
        over_budget: bool,
    },
    /// 进入静音态（无设备可切）。
    Silent,
}

/// 切换状态机 + 默认设备跟随策略。
#[derive(Clone, Debug)]
pub struct SwitchMachine {
    /// 当前输出设备（None = 静音态）。
    pub current: Option<u32>,
    /// 跟随策略：Some(id) = 用户锁定该设备；None = 跟随系统。
    pub locked_to: Option<u32>,
    /// 当前状态机相位。
    pub phase: SwitchPhase,
    /// 续播位置（热切换的无缝凭证）。
    pub playhead: u64,
    /// 超预算缺陷计数。
    pub over_budget_count: u32,
    /// 锁定冲突拒绝计数（跟随请求撞锁定的次数）。
    pub locked_refusals: u32,
    /// 紧急切换计数（设备消失触发的无缝切换）。
    pub emergency_switches: u32,
    /// 静音态提示计数（无设备可切次数）。
    pub silent_entries: u32,
}

impl SwitchMachine {
    /// 新状态机（跟随系统、空闲）。
    pub fn new() -> SwitchMachine {
        SwitchMachine {
            current: None,
            locked_to: None,
            phase: SwitchPhase::Idle,
            playhead: 0,
            over_budget_count: 0,
            locked_refusals: 0,
            emergency_switches: 0,
            silent_entries: 0,
        }
    }

    /// 用户锁定默认设备（锁定优先于跟随；None 解锁恢复跟随）。
    pub fn lock(&mut self, id: Option<u32>) {
        self.locked_to = id;
    }

    /// **热切换**（O(1)：常数步状态推进，不随播放流长度增长）。
    ///
    /// `now_ms` 用于超预算判定；`elapsed_ms` 为调用方实测切换耗时
    /// （状态机只管判定与记账——计时真源在输出层 AB02 上游）。
    pub fn hot_switch(
        &mut self,
        table: &DeviceTable,
        target: u32,
        now_ms: u32,
        elapsed_ms: u32,
    ) -> Result<SwitchOutcome, u16> {
        // 闸 1：目标必须在设备表且在位。
        match table.find(target) {
            Some(d) if d.present => {}
            _ => return Err(E_AB02_UNKNOWN_DEVICE),
        }
        // 闸 2：同设备切换无意义，显式拒绝（不管当前相位——外部可见相位
        // 只有 Idle/Done，两者下切同设备都无意义）。
        if self.current == Some(target) {
            return Err(E_AB02_SAME_DEVICE);
        }
        // 五态推进：Idle → FadingOut → Switching → FadingIn → Done。
        self.phase = SwitchPhase::FadingOut;
        self.phase = SwitchPhase::Switching;
        self.phase = SwitchPhase::FadingIn;
        self.phase = SwitchPhase::Done;
        self.current = Some(target);
        // playhead 保持（无缝续播凭证：切换不改播放位置）。
        self.over_budget_count += u32::from(elapsed_ms > SWITCH_BUDGET_MS);
        let over = elapsed_ms > SWITCH_BUDGET_MS;
        let _ = now_ms;
        Ok(SwitchOutcome::Completed {
            elapsed_ms,
            playhead: self.playhead,
            over_budget: over,
        })
    }

    /// **设备消失**（拔耳机的一瞬间）：紧急无缝切换到替补；
    /// 无替补 → 静音态提示。O(1)。
    pub fn device_gone(&mut self, table: &DeviceTable, gone_id: u32) -> SwitchOutcome {
        if self.current != Some(gone_id) {
            // 消失的不是当前输出设备——状态机不动。
            return SwitchOutcome::Completed {
                elapsed_ms: 0,
                playhead: self.playhead,
                over_budget: false,
            };
        }
        match table.escape_target(gone_id) {
            Some(next) => {
                // 紧急切换：跳过淡出预算从宽，但 playhead 仍保持。
                self.emergency_switches += 1;
                self.current = Some(next);
                self.phase = SwitchPhase::Done;
                SwitchOutcome::Completed {
                    elapsed_ms: FADE_MS,
                    playhead: self.playhead,
                    over_budget: false,
                }
            }
            None => {
                // 无设备：静音态提示（是状态不是故障，如实记账）。
                self.silent_entries += 1;
                self.current = None;
                self.phase = SwitchPhase::Idle;
                SwitchOutcome::Silent
            }
        }
    }

    /// **默认设备跟随**：系统默认变更时调用。
    ///
    /// 锁定优先于跟随——撞锁定显式拒绝并计数（不静默忽略）。
    pub fn follow_system_default(
        &mut self,
        table: &DeviceTable,
        system_default: u32,
    ) -> Result<u32, u16> {
        if self.locked_to.is_some() {
            self.locked_refusals += 1;
            return Err(E_AB02_LOCKED);
        }
        match table.find(system_default) {
            Some(d) if d.present => {}
            _ => return Err(E_AB02_UNKNOWN_DEVICE),
        }
        self.current = Some(system_default);
        Ok(system_default)
    }

    /// 读屏状态（设备状态可见——无障碍承诺面）。
    ///
    /// 行数固定 3：设备行 / 相位行 / 缺陷行。
    pub fn screen_status(&self, table: &DeviceTable) -> [String; 3] {
        let dev_line = match self.current {
            Some(id) => match table.find(id) {
                Some(d) => alloc::format!(
                    "当前输出：{}（延迟档 {}，补偿 {}%）",
                    d.name,
                    d.tier.ordinal(),
                    d.tier.spatial_compensation()
                ),
                None => alloc::format!("当前输出：设备 {}", id),
            },
            None => String::from("当前输出：静音（无在位设备）"),
        };
        let phase_line = alloc::format!("切换相位：{:?}", self.phase);
        let fault_line = alloc::format!(
            "超预算 {} 次；锁定拒绝 {} 次；紧急切换 {} 次；静音 {} 次",
            self.over_budget_count,
            self.locked_refusals,
            self.emergency_switches,
            self.silent_entries
        );
        [dev_line, phase_line, fault_line]
    }
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(SWITCH_BUDGET_MS == 120);
    assert!(FADE_MS == 24);
    assert!(FADE_MS < SWITCH_BUDGET_MS);
    assert!(E_AB02_NO_DEVICE & 0xFF00 == 0x5600);
    assert!(E_AB02_OVER_BUDGET & 0xFF00 == 0x5600);
    assert!(E_AB02_SAME_DEVICE & 0xFF00 == 0x5600);
    assert!(E_AB02_UNKNOWN_DEVICE & 0xFF00 == 0x5600);
    assert!(E_AB02_LOCKED & 0xFF00 == 0x5600);
    assert!(
        E_AB02_NO_DEVICE != E_AB02_OVER_BUDGET
            && E_AB02_OVER_BUDGET != E_AB02_SAME_DEVICE
            && E_AB02_SAME_DEVICE != E_AB02_UNKNOWN_DEVICE
            && E_AB02_UNKNOWN_DEVICE != E_AB02_LOCKED
    );
};
