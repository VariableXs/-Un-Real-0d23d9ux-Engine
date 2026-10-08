//! VE-F5602 · 音频设备与输出管理（AB 域 · 音频域 · 批次 AB01 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5602`
//!
//! **判据（锚点原文）**：热切换、淡出保护、跟随系统、判据。
//!
//! **职责定位（锚点原文）**：设备枚举、热切换无缝续播、默认设备跟随系统
//! （拔耳机的一瞬间声音去哪，是体验题不是技术题）；设备切换含切换时长
//! 预算（**超预算即缺陷**）；默认设备跟随含用户手动锁定的例外规则——
//! **锁定优先于跟随**；设备能力含**延迟档位披露**——各设备输出延迟实测
//! 建档，空间音频按档位调补偿。数据结构：设备表；切换状态机。错误路径
//! 与降级矩阵：设备消失→无缝切换；切换爆音→淡出保护；无设备→静音态
//! 提示。
//!
//! # 一、热切换为什么必须「无缝续播」而不是「重播」
//!
//! 拔掉耳机的一瞬间，如果输出从耳机切到扬声器时播放位回跳或断流，用户
//! 感知到的是「声音卡了一下」——设备切换的代价被转嫁给了内容。故切换
//! 状态机（[`SwitchRecord`]）把「续播位」作为一等产出：切到新设备后从
//! **同一播放位**继续（[`SwitchRecord::resume_at_ms`]），切换期间源端
//! 不停流。「无缝」不是形容词，是可断言的播放位连续性。
//!
//! # 二、切换时长为什么钉预算且「超预算即缺陷」
//!
//! 切换慢一拍，用户听到的是静默窗口；静默窗口超过感知阈值，体验退化
//! 为「声音丢了再找回来」。预算（[`SWITCH_BUDGET_MS`]）钉死后，每次
//! 切换完成时**独立对账**：实际耗时超过预算即立案（[`SwitchRecord::
//! over_budget`]）——不静默吞掉，缺陷必须显性（锚点红线原文）。
//!
//! # 三、爆音为什么用「淡出保护」而不是「直接切」
//!
//! 设备在出声途中被切断，DAC/混音器状态突变会产生「啪」的爆音。故切换
//! 第一步恒为**淡出**（[`FADE_OUT_MS`] 短淡出压平波形），再重定向，再
//! 续播——淡出保护是切换状态机的**强制相位**，任何「跳过淡出直接切」
//! 的捷径都在状态机层面被拒绝（爆音红线）。
//!
//! # 四、「锁定优先于跟随」为什么需要失效路径
//!
//! 默认设备跟随系统是常态；用户手动锁定是显性意志，意志优先于跟随。
//! 但锁定的设备**拔掉之后**锁定必须失效——跟随一个不存在的设备比跟随
//! 系统默认更糟。失效要**显性**（[`ELECT_LOCK_GONE`] 计数）：用户知道
//! 自己的锁定被解除了，而不是某天发现「声音从没听过的设备里出来」。
//!
//! # 五、与相邻条的分工
//!
//! AB01 管总架构与音频三律（域宪法，本条遵守不重复声明）；AB03 总线是
//! 下游（消费本条的输出路由结果）；播放管线/混音不属本条——本条只管
//! 「**有哪些设备、默认跟谁、切换怎么无缝、切换多久算缺陷**」。
//!
//! **性能（锚点原文）**：切换 O(1)（状态迁移与账面更新为常数动作；设备
//! 表按 id 定位，枚举规模由 AB01 架构约束在常数界内）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码（同域 AB01 风格：'static str 码）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const AUDIOOUT_VERSION: &str = "AB02-audioout-v1";

/// 设备未知（查无/已摘除）。
pub const E_DEVICE_UNKNOWN: &str = "E_DEVICE_UNKNOWN";

/// 切换超预算（超预算即缺陷——显性立案口径）。
pub const E_SWITCH_BUDGET: &str = "E_SWITCH_BUDGET";

/// 切换请求非法（目标不可用/自切/状态机错序）。
pub const E_SWITCH_BAD: &str = "E_SWITCH_BAD";

/// 切换时长预算（毫秒，钉死）：超过即缺陷立案。
pub const SWITCH_BUDGET_MS: u64 = 150;

/// 淡出保护时长（毫秒，钉死）：切换必经的压平波形窗口。
pub const FADE_OUT_MS: u32 = 20;

/// 延迟档位数（三档闭集）。
pub const TIER_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 二、延迟档位与设备模型
// ---------------------------------------------------------------------------

/// 输出延迟档位（三档闭集——实测建档后的离散披露，不裸报毫秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyTier {
    /// 低延迟（近场监听/游戏向）。
    Low,
    /// 中延迟（普通消费输出）。
    Mid,
    /// 高延迟（蓝牙/网络音频类）。
    High,
}

impl LatencyTier {
    /// 全集（顺序即档位序）。
    pub fn all() -> [LatencyTier; TIER_COUNT] {
        [LatencyTier::Low, LatencyTier::Mid, LatencyTier::High]
    }

    /// 档位短码。
    pub fn tag(self) -> &'static str {
        match self {
            LatencyTier::Low => "low",
            LatencyTier::Mid => "mid",
            LatencyTier::High => "high",
        }
    }

    /// 实测毫秒 → 档位（建档分档线钉死：≤40 低、≤120 中、其余高）。
    pub const fn from_latency_ms(ms: u32) -> LatencyTier {
        if ms <= 40 {
            LatencyTier::Low
        } else if ms <= 120 {
            LatencyTier::Mid
        } else {
            LatencyTier::High
        }
    }
}

/// 设备条目（设备表的最小单位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceEntry {
    /// 设备 id（稳定标识）。
    pub id: u32,
    /// 设备短名（读屏可达，不含路径）。
    pub tag: &'static str,
    /// 延迟档位（实测建档）。
    pub tier: LatencyTier,
    /// 是否支持空间音频。
    pub spatial: bool,
    /// 用户是否手动锁定为默认（锁定优先于跟随）。
    pub user_locked: bool,
    /// 是否在线（拔出即摘除）。
    pub alive: bool,
}

impl DeviceEntry {
    /// 空间音频补偿基数（毫秒）：按档位调补偿——高延迟档补得多。
    pub const fn spatial_compensation_ms(&self) -> u32 {
        match self.tier {
            LatencyTier::Low => 0,
            LatencyTier::Mid => 40,
            LatencyTier::High => 100,
        }
    }
}

/// 设备表：枚举/定位/摘除（锚点数据结构一）。
#[derive(Clone, Debug, Default)]
pub struct DeviceTable {
    entries: Vec<DeviceEntry>,
}

impl DeviceTable {
    /// 空表。
    pub fn new() -> DeviceTable {
        DeviceTable { entries: Vec::new() }
    }

    /// 枚举登记（重复 id 拒——设备身份唯一）。
    pub fn enroll(&mut self, e: DeviceEntry) -> Result<(), &'static str> {
        if self.entries.iter().any(|x| x.id == e.id) {
            return Err(E_DEVICE_UNKNOWN);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 按 id 定位（在线设备）。
    pub fn get(&self, id: u32) -> Option<&DeviceEntry> {
        self.entries.iter().find(|x| x.id == id && x.alive)
    }

    /// 实测延迟建档（覆盖档位——披露值以最新实测为准）。
    pub fn record_latency(&mut self, id: u32, measured_ms: u32) -> Result<LatencyTier, &'static str> {
        match self.entries.iter_mut().find(|x| x.id == id && x.alive) {
            Some(e) => {
                e.tier = LatencyTier::from_latency_ms(measured_ms);
                Ok(e.tier)
            }
            None => Err(E_DEVICE_UNKNOWN),
        }
    }

    /// 设备消失：摘除（alive=false）——降级矩阵第一路的触发源。
    pub fn unplug(&mut self, id: u32) -> Result<(), &'static str> {
        match self.entries.iter_mut().find(|x| x.id == id && x.alive) {
            Some(e) => {
                e.alive = false;
                Ok(())
            }
            None => Err(E_DEVICE_UNKNOWN),
        }
    }

    /// 在线设备数。
    pub fn alive_len(&self) -> usize {
        self.entries.iter().filter(|x| x.alive).count()
    }

    /// 在线设备快照（枚举——设备状态可见的数据面）。
    pub fn alive_ids(&self) -> Vec<u32> {
        self.entries.iter().filter(|x| x.alive).map(|x| x.id).collect()
    }
}

// ---------------------------------------------------------------------------
// 三、默认设备选举（跟随系统 + 锁定优先 + 锁定失效显性）
// ---------------------------------------------------------------------------

/// 选举结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElectVerdict {
    /// 跟随系统默认。
    FollowedSystem,
    /// 用户锁定生效（锁定优先于跟随）。
    LockedHold,
    /// 锁定失效回落跟随（锁定的设备已拔出——显性事件）。
    LockGone,
}

/// 默认设备选举（锚点「默认设备跟随系统+锁定例外」）。
///
/// 无可用设备返回 `Err(E_DEVICE_UNKNOWN)`（调用方落**静音态提示**路径
/// ——降级矩阵第三路，不虚构设备）。
pub fn elect_default(
    table: &DeviceTable,
    system_default: u32,
) -> Result<(u32, ElectVerdict), &'static str> {
    // 锁定优先：在线的锁定设备恒当选中（用户意志 > 系统默认）。
    let locked_alive = table
        .alive_ids()
        .iter()
        .filter_map(|id| table.get(*id))
        .find(|e| e.user_locked)
        .map(|e| e.id);
    if let Some(id) = locked_alive {
        return Ok((id, ElectVerdict::LockedHold));
    }
    // 锁定存在但已拔出：失效显性（返回值带 LockGone，调用方立案）。
    let lock_gone = table.entries.iter().any(|e| e.user_locked && !e.alive);
    // 跟随系统默认；系统默认也不在线则无设备可选举。
    if table.get(system_default).is_some() {
        let v = if lock_gone { ElectVerdict::LockGone } else { ElectVerdict::FollowedSystem };
        return Ok((system_default, v));
    }
    Err(E_DEVICE_UNKNOWN)
}

/// 锁定失效立案计数（显性——不让锁定静默消失）。
pub const ELECT_LOCK_GONE: &str = "E_LOCK_GONE";

// ---------------------------------------------------------------------------
// 四、热切换状态机（无缝续播 + 淡出保护 + 超预算立案）
// ---------------------------------------------------------------------------

/// 切换阶段（状态机：淡出→重定向→续播→完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchPhase {
    /// 淡出保护中（压平波形，爆音红线强制相位）。
    Fading,
    /// 已重定向到新设备（输出已改道）。
    Rerouted,
    /// 续播确认完成（播放位连续）。
    Done,
}

/// 一次热切换记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchRecord {
    /// 原设备。
    pub from: u32,
    /// 新设备。
    pub to: u32,
    /// 当前阶段。
    pub phase: SwitchPhase,
    /// 发起时刻。
    pub started_ms: u64,
    /// 完成时刻（Done 后回填）。
    pub done_ms: Option<u64>,
    /// 续播位（源端播放位置，毫秒）——无缝=新设备从此位继续。
    pub resume_at_ms: u64,
}

impl SwitchRecord {
    /// 实际切换耗时（Done 前 None）。
    pub fn elapsed_ms(&self) -> Option<u64> {
        self.done_ms.map(|d| d.saturating_sub(self.started_ms))
    }

    /// 超预算判定（**超预算即缺陷**——独立对账，不问状态机自己对不对）。
    pub fn over_budget(&self) -> bool {
        matches!(self.elapsed_ms(), Some(e) if e > SWITCH_BUDGET_MS)
    }
}

/// 发起热切换：校验→建记录（首相位恒 Fading——淡出保护不可跳过）。
pub fn begin_switch(
    table: &DeviceTable,
    from: u32,
    to: u32,
    resume_at_ms: u64,
    now_ms: u64,
) -> Result<SwitchRecord, &'static str> {
    if from == to {
        return Err(E_SWITCH_BAD);
    }
    if table.get(to).is_none() {
        return Err(E_DEVICE_UNKNOWN);
    }
    if table.get(from).is_none() {
        // 原设备已不在（拔出场景走 emergency 切换，不经本入口）。
        return Err(E_SWITCH_BAD);
    }
    Ok(SwitchRecord {
        from,
        to,
        phase: SwitchPhase::Fading,
        started_ms: now_ms,
        done_ms: None,
        resume_at_ms,
    })
}

/// 推进状态机：Fading → Rerouted → Done（错序拒绝——淡出不可跳）。
pub fn advance(r: &mut SwitchRecord, now_ms: u64) -> Result<SwitchPhase, &'static str> {
    r.phase = match r.phase {
        SwitchPhase::Fading => SwitchPhase::Rerouted,
        SwitchPhase::Rerouted => {
            r.done_ms = Some(now_ms);
            SwitchPhase::Done
        }
        SwitchPhase::Done => return Err(E_SWITCH_BAD),
    };
    Ok(r.phase)
}

/// 紧急切换（降级矩阵第一路：设备消失→无缝切换，跳过自切校验）。
///
/// 原设备已拔出时由**拔出事件**驱动：目标必须在线，记录显性标注来源
/// 是紧急路径（`from` 设备已 `alive=false`）——不静默当成正常切换。
pub fn emergency_switch(
    table: &DeviceTable,
    from_gone: u32,
    to: u32,
    resume_at_ms: u64,
    now_ms: u64,
) -> Result<SwitchRecord, &'static str> {
    if from_gone == to {
        return Err(E_SWITCH_BAD);
    }
    if table.get(to).is_none() {
        return Err(E_DEVICE_UNKNOWN);
    }
    if table.get(from_gone).is_some() {
        // 原设备还在：这不是紧急场景，走 begin_switch 正常入口。
        return Err(E_SWITCH_BAD);
    }
    Ok(SwitchRecord {
        from: from_gone,
        to,
        phase: SwitchPhase::Fading,
        started_ms: now_ms,
        done_ms: None,
        resume_at_ms,
    })
}

/// 静音态提示（降级矩阵第三路：无设备→静音态，状态可见不黑箱）。
pub const MUTE_NOTICE: &str = "AUDIO-MUTE: 无可用输出设备，已进入静音态";

// ---------------------------------------------------------------------------
// 五、读屏替述（设备状态可见——域本色）
// ---------------------------------------------------------------------------

/// 设备读屏单行（id/短名/档位/锁定态；无路径无内容）。
pub fn screen_line_device(e: &DeviceEntry) -> String {
    let lock = if e.user_locked { "，已锁定为默认" } else { "" };
    format!(
        "输出设备 {}（id {}）：延迟档 {}，{}空间音频{}",
        e.tag,
        e.id,
        e.tier.tag(),
        if e.spatial { "支持" } else { "不支持" },
        lock
    )
}

/// 切换读屏单行（阶段+续播位；无内容）。
pub fn screen_line_switch(r: &SwitchRecord) -> String {
    let phase = match r.phase {
        SwitchPhase::Fading => "淡出保护中",
        SwitchPhase::Rerouted => "已切换到新设备",
        SwitchPhase::Done => "切换完成",
    };
    format!(
        "输出切换 {}→{}：{}，续播位 {} 毫秒",
        r.from, r.to, phase, r.resume_at_ms
    )
}
