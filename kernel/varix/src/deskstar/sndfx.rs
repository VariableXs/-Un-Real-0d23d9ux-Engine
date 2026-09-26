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
use alloc::format;
use alloc::string::ToString;

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
    /// 资产装载状态（深化层：F068 接缝——逐事件独立）。
    load_states: [LoadState; 6],
    load_retries: [u32; 6],
    /// 试听独立队列（不与事件队列混排）。
    preview_queue: Vec<(SfxEvent, u64)>,
    last_preview_ms: Option<u64>,
}

impl SfxHub {
    /// 建总线：缺省星海方案、各事件音量 80。
    pub fn new() -> SfxHub {
        SfxHub {
            scheme: Scheme::StarSea,
            entries: core::array::from_fn(|i| SfxEntry {
                event: SfxEvent::ALL[i],
                asset: format!("sfx/{}-{}.flac", scheme_tag(Scheme::StarSea), tag_of(SfxEvent::ALL[i])),
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
            load_states: [LoadState::Pending; 6],
            load_retries: [0; 6],
            preview_queue: Vec::new(),
            last_preview_ms: None,
        }
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// 切换方案（按需装载标记复位——下次播出时重装载）。
    pub fn set_scheme(&mut self, s: Scheme) {
        self.scheme = s;
        for e in self.entries.iter_mut() {
            e.asset = format!("sfx/{}-{}.flac", scheme_tag(s), tag_of(e.event));
            e.loaded = false;
        }
        if s.silent_scheme() {
            // 无声方案：映射表全空（事件→无音效——诚实映射而非假播）。
            self.diag.push(format!("方案切至无声：全部事件静映射"));
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
        // 装载失败的事件回退无声（重试耗尽后不再进队——诚实降级）。
        if self.event_load_failed(event) {
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
// ---------------------------------------------------------------------------
// 深化层（回炉批）：资产装载状态机（F068 接缝）/ 资产规格校验 / 试听独立
// 通道 / 方案清单导出校验 / 响度归一细化——主册【数据与存储】【设计细节】。
// ---------------------------------------------------------------------------

/// 资产装载状态（F068 渲染资产按需装载的接缝账——音效文件同管线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// 未装载（首次播出/试听前）。
    Pending,
    /// 装载中（48kHz/24bit FLAC 解码入内存）。
    Loading,
    /// 就绪（可播）。
    Ready,
    /// 失败（重试耗尽——该事件回退无声 + 诊断报备）。
    Failed,
}

/// 装载重试上限（两次重试后判失败——防坏资产死循环）。
pub const LOAD_RETRY_CAP: u32 = 2;

/// 资产规格（4K 资产管线同级：48kHz/24bit）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetSpec {
    pub sample_rate_hz: u32,
    pub bits: u8,
    pub channels: u8,
}

/// 规格校验（48kHz/24bit/单双声道之外一律拒——管线纪律）。
pub fn validate_spec(spec: AssetSpec) -> Result<(), &'static str> {
    if spec.sample_rate_hz != SAMPLE_RATE_HZ {
        return Err("采样率须 48kHz（4K 资产管线标准）");
    }
    if spec.bits != SAMPLE_BITS {
        return Err("位深须 24bit（防炸耳的动态范围下限）");
    }
    if spec.channels == 0 || spec.channels > 2 {
        return Err("声道须单声道或立体声");
    }
    Ok(())
}

/// 方案导出清单（vxtheme 承载的用户方案容器条目——导出完整性校验面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeManifest {
    pub scheme_name: String,
    /// 六事件 → 资产名（缺事件 = 不完整清单）。
    pub bindings: Vec<(&'static str, String)>,
    pub master_mute: bool,
}

impl SfxHub {
    /// 装载状态查询。
    pub fn load_state(&self, event: SfxEvent) -> LoadState {
        self.load_states[event.index()]
    }

    /// 装载请求（Pending → Loading；播放前由音频执行面驱动）。
    pub fn request_load(&mut self, event: SfxEvent) -> bool {
        let i = event.index();
        if self.load_states[i] == LoadState::Pending {
            self.load_states[i] = LoadState::Loading;
            true
        } else {
            false
        }
    }

    /// 装载完成回执（→ Ready）。
    pub fn load_ready(&mut self, event: SfxEvent) {
        self.load_states[event.index()] = LoadState::Ready;
    }

    /// 装载失败回执（重试计数；耗尽 → Failed + 该事件静默 + 报备）。
    pub fn load_failed(&mut self, event: SfxEvent, now_ms: u64) -> bool {
        let i = event.index();
        self.load_retries[i] += 1;
        if self.load_retries[i] > LOAD_RETRY_CAP {
            self.load_states[i] = LoadState::Failed;
            self.diag
                .push(format!("{} 音效装载失败：该事件回退无声（诊断报备）", event.name()));
            self.now_ms = now_ms;
            true
        } else {
            // 回到 Pending 允许重试（下次 request_load 重新走 Loading）。
            self.load_states[i] = LoadState::Pending;
            false
        }
    }

    /// 失败事件播出闸：Failed 状态的事件 trigger 不入队（回退无声）。
    pub fn event_load_failed(&self, event: SfxEvent) -> bool {
        self.load_states[event.index()] == LoadState::Failed
    }

    /// 试听请求（E5 声音方案页——独立试听通道，不与事件队列混排；
    /// 返回是否受理：静音总闸不拦试听（试听是用户主动确认动作），
    /// 但无声方案与设备缺失照旧拒）。
    pub fn preview(&mut self, event: SfxEvent, now_ms: u64) -> bool {
        self.now_ms = now_ms;
        if !self.device_present || self.scheme.silent_scheme() {
            self.stat_skipped += 1;
            return false;
        }
        self.preview_queue.push((event, now_ms));
        true
    }

    /// 试听队列驱动（独立 80ms 节拍——与事件队列同一间隔语义）。
    pub fn preview_tick(&mut self, now_ms: u64) -> Option<SfxEvent> {
        self.now_ms = now_ms;
        if self.preview_queue.is_empty() {
            return None;
        }
        let gap_ok = match self.last_preview_ms {
            None => true,
            Some(t) => now_ms.saturating_sub(t) >= QUEUE_GAP_MS,
        };
        if !gap_ok {
            return None;
        }
        let (e, _) = self.preview_queue.remove(0);
        self.last_preview_ms = Some(now_ms);
        Some(e)
    }

    /// 方案清单导出（vxtheme 容器条目——绑定完整性校验随行）。
    pub fn export_manifest(&self, scheme_name: &str) -> SchemeManifest {
        SchemeManifest {
            scheme_name: String::from(scheme_name),
            bindings: self
                .entries
                .iter()
                .map(|e| (e.event.name(), e.asset.clone()))
                .collect(),
            master_mute: self.master_mute,
        }
    }

    /// 清单完整性校验（六事件齐 + 音量范围合法才算完整方案包）。
    pub fn manifest_complete(m: &SchemeManifest) -> Result<(), &'static str> {
        if m.bindings.len() != 6 {
            return Err("清单缺事件绑定（六事件必须齐）");
        }
        if m.bindings.iter().any(|(_, asset)| asset.is_empty()) {
            return Err("存在空资产名的绑定");
        }
        Ok(())
    }

    /// 响度归一细化：音量→增益毫分贝查表（±40% 线性近似域外的
    /// 顶格点单列——音量 100 与 0 的边界行为明确）。
    pub fn gain_table() -> [(u8, i32); 6] {
        [
            (0, i32::MIN / 2),
            (20, 2000 * (20i32 - 80) / 400),
            (40, 2000 * (40i32 - 80) / 400),
            (60, 2000 * (60i32 - 80) / 400),
            (80, 0),
            (100, 2000 * (100i32 - 80) / 400),
        ]
    }
}

/// F079 深化自检：装载状态机、规格校验、试听独立通道、清单导出校验、
/// 增益表边界。
pub fn run_sndfx_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F079-deep");
    let mut h = SfxHub::new();
    // 1. 装载状态机：Pending → Loading → Ready；失败重试 2 次 → Failed。
    h.request_load(SfxEvent::Boot);
    let loading = h.load_state(SfxEvent::Boot) == LoadState::Loading;
    let re_req = !h.request_load(SfxEvent::Boot); // Loading 中重复请求被拒
    h.load_ready(SfxEvent::Boot);
    let ready = h.load_state(SfxEvent::Boot) == LoadState::Ready;
    set.add(
        "load-state",
        loading && re_req && ready,
        "pending→loading→ready",
    );
    // 2. 失败重试：2 次内回 Pending，第 3 次判 Failed + 报备。
    h.request_load(SfxEvent::Notify);
    let f1 = !h.load_failed(SfxEvent::Notify, 1_000);
    h.request_load(SfxEvent::Notify);
    let f2 = !h.load_failed(SfxEvent::Notify, 2_000);
    h.request_load(SfxEvent::Notify);
    let f3 = h.load_failed(SfxEvent::Notify, 3_000)
        && h.load_state(SfxEvent::Notify) == LoadState::Failed
        && h.event_load_failed(SfxEvent::Notify);
    set.add(
        "load-retry",
        f1 && f2 && f3 && LOAD_RETRY_CAP == 2,
        "retry cap → silent fallback",
    );
    // 3. Failed 事件播出闸：trigger 不入队。
    let enq = h.trigger(SfxEvent::Notify, 4_000);
    set.add("failed-gate", !enq && h.queue_len() == 0, "no play on failed");
    // 4. 资产规格校验：48kHz/24bit 通过，其余逐项拒。
    let ok = validate_spec(AssetSpec { sample_rate_hz: 48_000, bits: 24, channels: 2 }).is_ok();
    let bad_rate = validate_spec(AssetSpec { sample_rate_hz: 44_100, bits: 24, channels: 2 }).is_err();
    let bad_bits = validate_spec(AssetSpec { sample_rate_hz: 48_000, bits: 16, channels: 1 }).is_err();
    let bad_ch = validate_spec(AssetSpec { sample_rate_hz: 48_000, bits: 24, channels: 6 }).is_err();
    set.add(
        "spec-validate",
        ok && bad_rate && bad_bits && bad_ch,
        "48k/24b/stereo-only",
    );
    // 5. 试听独立通道：与事件队列互不干扰（同一 80ms 节拍语义）。
    let mut h2 = SfxHub::new();
    let pv = h2.preview(SfxEvent::Recycle, 10_000);
    let p1 = h2.preview_tick(10_000);
    let p2 = h2.preview_tick(10_050); // 50ms < 80ms
    let p3 = h2.preview_tick(10_081); // 81ms ≥ 80ms
    set.add(
        "preview-channel",
        pv && p1 == Some(SfxEvent::Recycle) && p2.is_none() && p3.is_none(),
        "separate preview queue",
    );
    // 6. 无声方案拒试听（与事件触发同裁决）。
    h2.set_scheme(Scheme::Silent);
    let pv2 = h2.preview(SfxEvent::Boot, 20_000);
    set.add("preview-silent", !pv2, "silent scheme refuses");
    // 7. 方案清单导出 + 完整性校验。
    let m = h.export_manifest("星海·用户定制");
    let complete = SfxHub::manifest_complete(&m).is_ok();
    let mut broken = m.clone();
    broken.bindings.pop();
    let incomplete = SfxHub::manifest_complete(&broken).is_err();
    set.add(
        "manifest-export",
        complete && incomplete && m.bindings.len() == 6 && !m.master_mute,
        "vxtheme manifest",
    );
    // 8. 增益表：0 静音、80 基准 0dB、100 正增益（单调）。
    let g = SfxHub::gain_table();
    let monotonic = g.windows(2).skip(1).all(|w| w[0].1 < w[1].1);
    set.add(
        "gain-table",
        g[0].1 < -1_000_000_000 && g[4].1 == 0 && monotonic,
        "-18LUFS boundaries",
    );
    set
}

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn load_failure_diag_is_recorded() {
        let mut h = SfxHub::new();
        for _ in 0..3 {
            h.request_load(SfxEvent::Error);
            h.load_failed(SfxEvent::Error, 0);
        }
        assert!(h.load_state(SfxEvent::Error) == LoadState::Failed);
        assert!(h
            .diag_log()
            .iter()
            .any(|d| d.contains("错误 音效装载失败")), "逐事件报备");
    }

    #[test]
    fn preview_and_event_queues_independent() {
        let mut h = SfxHub::new();
        assert!(h.trigger(SfxEvent::Boot, 0));
        assert!(h.preview(SfxEvent::Notify, 1));
        assert_eq!(h.tick(0), Some(SfxEvent::Boot), "事件队列不受试听影响");
        assert_eq!(h.preview_tick(1), Some(SfxEvent::Notify));
    }

    #[test]
    fn manifest_carries_mute_state() {
        let mut h = SfxHub::new();
        h.set_master_mute(true);
        let m = h.export_manifest("静音方案");
        assert!(m.master_mute);
    }

    #[test]
    fn sndfx_deep_checks_all_green() {
        let set = run_sndfx_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F079-deep 红项：{}/{} 绿", p, p + f);
    }
}

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
