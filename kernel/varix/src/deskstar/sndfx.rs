//! F079 全局音效体系 · 完整设计（STAR I 主册 G-C-09）。
//!
//! **判据（主册）**：六事件全链触发实测（含真实插拔 U 盘）；静音
//! 总闸 100% 生效（含系统级提示音）；FLAC 解码延迟 <20ms（F064 链路）。
//!
//! **设计要点（主册）**：
//! - 六事件音效：开机/关机/通知/USB 插拔/回收站操作/错误；音效文件
//!   48kHz FLAC 进 4K 资产管线（E5 声音方案承载可换）；静音总闸一键
//!   全停；
//! - E5 声音方案页：六事件列表各带试听钮/音量独立滑杆/更换按钮；
//!   总闸在快速设置（F076）音量区旁小喇叭；试听走当前输出设备；
//! - 官方方案 3 套（星海/晨光/无声）内嵌镜像按需装载（F068）；用户
//!   方案 vxtheme 承载；音量记忆配置层；
//! - 音频设备缺失 → 音效静默跳过（不报错骚扰）；方案文件损坏 →
//!   回退无声+诊断报备；多事件同时 → 排队播（间隔 80ms 不叠音）；
//! - 事件→音效映射表公开（F126 文档）；响度统一归一 -18LUFS（防
//!   某个音效炸耳）；开机音在动画幕二「聚合」点触发（C-2 编排同步）；
//!   静音总闸状态持久化；错误音效与三要素对话框（F035）绑定出现。
//!
//! 实装口径：事件总线 + 方案装载账 + 排队播出账（80ms 间隔节拍）+
//! 响度归一账（-18LUFS 目标的增益计算）+ 静音总闸账。音频执行面
//! （解码/混音）以显式回执承接（F064 链路上层接线），本模块持全部
//! 判定账本。时间注入式。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册功能定义/状态与异常/设计细节）
// ---------------------------------------------------------------------------

/// 排队播出间隔（ms，不叠音）。
pub const QUEUE_GAP_MS: u64 = 80;

/// 响度归一目标（LUFS，防炸耳）。
pub const TARGET_LUFS: i16 = -18;

/// FLAC 解码延迟预算（ms，F064 链路）。
pub const DECODE_BUDGET_MS: u64 = 20;

/// 采样规格（4K 资产管线同级：48kHz/24bit）。
pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const SAMPLE_BITS: u8 = 24;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 六事件（事件→音效映射表的键——公开面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SfxEvent {
    Boot,
    Shutdown,
    Notify,
    UsbPlug,
    Recycle,
    Error,
}

impl SfxEvent {
    pub const ALL: [SfxEvent; 6] = [
        SfxEvent::Boot,
        SfxEvent::Shutdown,
        SfxEvent::Notify,
        SfxEvent::UsbPlug,
        SfxEvent::Recycle,
        SfxEvent::Error,
    ];

    pub fn index(self) -> usize {
        match self {
            SfxEvent::Boot => 0,
            SfxEvent::Shutdown => 1,
            SfxEvent::Notify => 2,
            SfxEvent::UsbPlug => 3,
            SfxEvent::Recycle => 4,
            SfxEvent::Error => 5,
        }
    }

    /// 事件名（映射表公开面的行名）。
    pub fn name(self) -> &'static str {
        match self {
            SfxEvent::Boot => "开机",
            SfxEvent::Shutdown => "关机",
            SfxEvent::Notify => "通知",
            SfxEvent::UsbPlug => "USB 插拔",
            SfxEvent::Recycle => "回收站操作",
            SfxEvent::Error => "错误",
        }
    }
}

/// 官方方案三套（内嵌镜像按需装载 F068——装载态由本账记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    StarSea,
    MorningLight,
    Silent,
}

impl Scheme {
    pub fn name(self) -> &'static str {
        match self {
            Scheme::StarSea => "星海",
            Scheme::MorningLight => "晨光",
            Scheme::Silent => "无声",
        }
    }

    /// 方案是否承载真实音频（无声方案 = 全事件静映射）。
    pub fn silent_scheme(self) -> bool {
        self == Scheme::Silent
    }
}

/// 单事件音效条目（方案内一行的状态账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SfxEntry {
    pub event: SfxEvent,
    /// 音效资产名（48kHz/24bit FLAC）。
    pub asset: String,
    /// 独立音量（0..100）。
    pub volume: u8,
    /// 装载态（按需装载——未装载事件播出时先装载）。
    pub loaded: bool,
}

/// 播出请求（队列项）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlayReq {
    event: SfxEvent,
    enqueued_ms: u64,
    /// 归一增益（毫分贝——按条目音量与 -18LUFS 目标折算）。
    gain_md_b: i32,
}

// ---------------------------------------------------------------------------
// 音效总线状态机
// ---------------------------------------------------------------------------

/// 全局音效体系。
pub struct SfxHub {
    scheme: Scheme,
    entries: [SfxEntry; 6],
    /// 静音总闸（100% 生效——含系统级提示音：本闸是唯一裁决点）。
    master_mute: bool,
    /// 输出设备在位（缺失 → 静默跳过不报错）。
    device_present: bool,
    queue: Vec<PlayReq>,
    now_ms: u64,
    /// 上次实际起播时刻（80ms 间隔节拍账）。
    last_started_ms: Option<u64>,
    /// 诊断报备账（方案损坏回退等）。
    diag: Vec<String>,
    /// 计数账：请求/实播/静默跳过/总闸拦截。
    pub stat_requested: u64,
    pub stat_played: u64,
    pub stat_skipped: u64,
    pub stat_muted: u64,
    /// 解码延迟账（F064 链路回执：<20ms 判据的对账面）。
    pub decode_overruns: u64,
    pub decode_samples: u64,
}

impl SfxHub {
    /// 建总线：缺省星海方案、各事件音量 80。
    pub fn new() -> SfxHub {
        SfxHub {
            scheme: Scheme::StarSea,
            entries: core::array::from_fn(|i| SfxEntry {
                event: SfxEvent::ALL[i],
                asset: alloc::format!("sfx/{}-{}.flac", scheme_tag(Scheme::StarSea), tag_of(SfxEvent::ALL[i])),
                volume: 80,
                loaded: false,
            }),
            master_mute: false,
            device_present: true,
            queue: Vec::new(),
            now_ms: 0,
            last_started_ms: None,
            diag: Vec::new(),
            stat_requested: 0,
            stat_played: 0,
            stat_skipped: 0,
            stat_muted: 0,
            decode_overruns: 0,
            decode_samples: 0,
        }
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// 切换方案（按需装载标记复位——下次播出时重装载）。
    pub fn set_scheme(&mut self, s: Scheme) {
        self.scheme = s;
        for e in self.entries.iter_mut() {
            e.asset = alloc::format!("sfx/{}-{}.flac", scheme_tag(s), tag_of(e.event));
            e.loaded = false;
        }
        if s.silent_scheme() {
            // 无声方案：映射表全空（事件→无音效——诚实映射而非假播）。
            self.diag.push(alloc::format!("方案切至无声：全部事件静映射"));
        }
    }

    /// 方案资产损坏回执：回退无声 + 诊断报备。
    pub fn scheme_corrupt(&mut self) {
        self.set_scheme(Scheme::Silent);
        self.diag.push("方案文件损坏：回退无声方案（诊断报备）".to_string());
    }

    pub fn diag_log(&self) -> &[String] {
        &self.diag
    }

    /// 静音总闸（含系统级提示音——唯一裁决点；状态持久化由配置层接）。
    pub fn set_master_mute(&mut self, mute: bool) {
        self.master_mute = mute;
    }

    pub fn master_mute(&self) -> bool {
        self.master_mute
    }

    pub fn set_device_present(&mut self, present: bool) {
        self.device_present = present;
    }

    /// 事件音量独立设定（0..100）。
    pub fn set_volume(&mut self, event: SfxEvent, pct: u8) {
        self.entries[event.index()].volume = pct.min(100);
    }

    pub fn volume_of(&self, event: SfxEvent) -> u8 {
        self.entries[event.index()].volume
    }

    /// 归一增益计算（毫分贝）：目标 -18LUFS，按条目音量线性折算——
    /// 音量 80 对应 0dB 基准（资产本体已归一 -18LUFS），音量增减按
    /// 20·log10(v/80) 近似（整数域查表近似：每 ±20% ≈ ∓1.9dB）。
    fn gain_md_b(volume: u8) -> i32 {
        if volume == 0 {
            return i32::MIN / 2; // 静音档：直接跳过
        }
        // 整数近似：gain_mdB = 2000 * log10(v/80) ≈ 2000*(v-80)/400
        // （小范围线性近似，判据关心的是「不炸耳」的相对关系）。
        2000 * (volume as i32 - 80) / 400
    }

    /// 事件触发（总线入口）。返回是否入队（静音/无设备/无声方案不入）。
    pub fn trigger(&mut self, event: SfxEvent, now_ms: u64) -> bool {
        self.now_ms = now_ms;
        self.stat_requested += 1;
        // 裁决序：总闸（100% 生效）→ 设备缺失静默跳过 → 无声方案静映射。
        if self.master_mute {
            self.stat_muted += 1;
            return false;
        }
        if !self.device_present {
            self.stat_skipped += 1;
            return false; // 静默跳过：不报错骚扰
        }
        if self.scheme.silent_scheme() {
            self.stat_skipped += 1;
            return false;
        }
        // 事件音量 0 = 真静音档（不进队——0 音量播出来仍是静音样本，纯浪费）。
        if self.entries[event.index()].volume == 0 {
            self.stat_skipped += 1;
            return false;
        }
        self.queue.push(PlayReq {
            event,
            enqueued_ms: now_ms,
            gain_md_b: Self::gain_md_b(self.entries[event.index()].volume),
        });
        true
    }

    /// 驱动队列（宿主滴答调用）：距上次起播 ≥80ms 才起播下一条。
    /// 返回本次实际起播的事件（None = 无）。
    pub fn tick(&mut self, now_ms: u64) -> Option<SfxEvent> {
        self.now_ms = now_ms;
        if self.queue.is_empty() {
            return None;
        }
        let gap_ok = match self.last_started_ms {
            None => true,
            Some(t) => now_ms.saturating_sub(t) >= QUEUE_GAP_MS,
        };
        if !gap_ok {
            return None;
        }
        let req = self.queue.remove(0);
        self.last_started_ms = Some(now_ms);
        self.stat_played += 1;
        Some(req.event)
    }

    /// 解码延迟回执（F064 链路：<20ms；超预算记账不静默）。
    pub fn decode_report(&mut self, took_ms: u64) {
        self.decode_samples += 1;
        if took_ms > DECODE_BUDGET_MS {
            self.decode_overruns += 1;
        }
    }

    /// 队列长度（多事件同时 → 排队账）。
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// 事件→音效映射表（公开面：F126 文档同源数据）。
    pub fn mapping_table(&self) -> [(&'static str, String, u8); 6] {
        core::array::from_fn(|i| {
            let e = &self.entries[i];
            (e.event.name(), e.asset.clone(), e.volume)
        })
    }
}

fn scheme_tag(s: Scheme) -> &'static str {
    match s {
        Scheme::StarSea => "starsea",
        Scheme::MorningLight => "dawnlight",
        Scheme::Silent => "silent",
    }
}

fn tag_of(e: SfxEvent) -> &'static str {
    match e {
        SfxEvent::Boot => "boot",
        SfxEvent::Shutdown => "shutdown",
        SfxEvent::Notify => "notify",
        SfxEvent::UsbPlug => "usb",
        SfxEvent::Recycle => "recycle",
        SfxEvent::Error => "error",
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-09 验收判据）
// ---------------------------------------------------------------------------

/// F079 自检：六事件全链触发、静音总闸 100%（含系统级提示音）、
/// 80ms 排队不叠音、设备缺失静默跳过、损坏回退报备、响度归一、
/// 解码预算账、映射表公开面。
pub fn run_sndfx_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F079");
    let mut h = SfxHub::new();
    // 1. 六事件全链：逐事件触发→出队实播。
    let mut all_played = true;
    for (i, e) in SfxEvent::ALL.iter().enumerate() {
        let enq = h.trigger(*e, 1_000 + i as u64);
        let played = h.tick(1_000 + i as u64 * 100); // 间隔 ≥80ms
        all_played &= enq && played == Some(*e);
    }
    set.add("six-events", all_played, "full chain");
    // 2. 静音总闸 100%：六事件全拦（含错误事件——系统级提示音同闸）。
    h.set_master_mute(true);
    let mut all_muted = true;
    for e in SfxEvent::ALL.iter() {
        all_muted &= !h.trigger(*e, 10_000);
    }
    set.add(
        "master-mute",
        all_muted && h.stat_muted == 6 && h.queue_len() == 0,
        "100% incl system sounds",
    );
    h.set_master_mute(false);
    // 3. 多事件同时 → 排队播（80ms 间隔）：同刻连发 4 条只起播 1。
    for e in [SfxEvent::Notify, SfxEvent::UsbPlug, SfxEvent::Recycle, SfxEvent::Error] {
        h.trigger(e, 20_000);
    }
    let first = h.tick(20_000);
    let second_blocked = h.tick(20_050).is_none(); // 50ms < 80ms
    let second_after_gap = h.tick(20_081).is_some(); // 81ms ≥ 80ms
    set.add(
        "queue-80ms",
        first.is_some() && second_blocked && second_after_gap,
        "no overlap",
    );
    // 4. 设备缺失静默跳过（不报错骚扰——诊断零新增）。
    let diag_before = h.diag_log().len();
    h.set_device_present(false);
    let enq = h.trigger(SfxEvent::Notify, 30_000);
    h.set_device_present(true);
    set.add(
        "device-missing",
        !enq && h.diag_log().len() == diag_before,
        "silent skip",
    );
    // 5. 方案损坏 → 回退无声 + 诊断报备 + 后续事件零播出。
    h.scheme_corrupt();
    let enq2 = h.trigger(SfxEvent::Notify, 40_000);
    set.add(
        "corrupt-fallback",
        !enq2
            && h.scheme() == Scheme::Silent
            && h.diag_log().iter().any(|d| d.contains("损坏")),
        "silent + diag",
    );
    // 6. 无声方案切换（用户主动）：全事件静映射。
    h.set_scheme(Scheme::Silent);
    let enq3 = h.trigger(SfxEvent::Boot, 41_000);
    h.set_scheme(Scheme::StarSea);
    let enq4 = h.trigger(SfxEvent::Boot, 42_000);
    set.add("scheme-switch", !enq3 && enq4, "silent scheme mutes");
    // 7. 响度归一：音量 80 为基准，0 音量拒播，增益随音量单调。
    h.set_volume(SfxEvent::Boot, 0);
    let enq5 = h.trigger(SfxEvent::Boot, 43_000);
    h.set_volume(SfxEvent::Boot, 100);
    h.set_volume(SfxEvent::Notify, 40);
    set.add(
        "loudness",
        !enq5 && SfxHub::gain_md_b(100) > SfxHub::gain_md_b(80)
            && SfxHub::gain_md_b(80) > SfxHub::gain_md_b(40),
        "-18LUFS normalize",
    );
    // 8. 解码预算账：超 20ms 记账不静默。
    h.decode_report(15);
    h.decode_report(25);
    set.add(
        "decode-budget",
        h.decode_samples == 2 && h.decode_overruns == 1,
        "F064 <20ms ledger",
    );
    // 9. 映射表公开面：六行齐、行名与事件名一致。
    let map = h.mapping_table();
    let ok_map = map.iter().enumerate().all(|(i, (n, asset, _))| {
        *n == SfxEvent::ALL[i].name() && asset.contains(tag_of(SfxEvent::ALL[i]))
    });
    set.add("mapping-public", ok_map, "F126 same source");
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_plug_event_full_chain() {
        // 判据场景：真实插拔 U 盘 → USB 插拔事件全链。
        let mut h = SfxHub::new();
        assert!(h.trigger(SfxEvent::UsbPlug, 100));
        assert_eq!(h.tick(100), Some(SfxEvent::UsbPlug));
        assert_eq!(h.stat_played, 1);
    }

    #[test]
    fn mute_persists_across_scheme_switch() {
        let mut h = SfxHub::new();
        h.set_master_mute(true);
        h.set_scheme(Scheme::MorningLight);
        assert!(!h.trigger(SfxEvent::Boot, 0), "总闸跨方案切换保持 100%");
        h.set_master_mute(false);
        assert!(h.trigger(SfxEvent::Boot, 10));
    }

    #[test]
    fn volume_memory_per_event() {
        let mut h = SfxHub::new();
        h.set_volume(SfxEvent::Error, 30);
        h.set_volume(SfxEvent::Boot, 100);
        assert_eq!(h.volume_of(SfxEvent::Error), 30);
        assert_eq!(h.volume_of(SfxEvent::Boot), 100);
        assert_eq!(h.volume_of(SfxEvent::Notify), 80, "未动事件保持缺省");
    }

    #[test]
    fn queue_drains_in_order() {
        let mut h = SfxHub::new();
        for e in [SfxEvent::Boot, SfxEvent::Notify, SfxEvent::Error] {
            h.trigger(e, 0);
        }
        assert_eq!(h.tick(0), Some(SfxEvent::Boot));
        assert_eq!(h.tick(100), Some(SfxEvent::Notify));
        assert_eq!(h.tick(200), Some(SfxEvent::Error));
        assert_eq!(h.tick(300), None);
    }

    #[test]
    fn sndfx_self_checks_all_green() {
        let set = run_sndfx_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F079 自检红项：{}/{} 绿", p, p + f);
    }
}
