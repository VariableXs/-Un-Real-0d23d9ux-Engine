//! VE-F5210 · 特效遥测（VE-Z 域 · 遥测面 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5210`
//!
//! **判据（锚点原文）**：三流、默认关、采样、补传、判据。
//!
//! **职责定位（锚点原文）**：特效遥测——遥测三流：特效使用率流（哪些
//! 特效被用哪些闲置）/性能开销流（逐特效耗时画像）/组合密度流（场景特
//! 效密度统计）；隐私红线：只记事件与计数不记内容——脱敏开关默认关
//! 显式开。
//!
//! **数据结构（锚点原文）**：三流遥测器；脱敏开关；消费接口。
//!
//! **错误路径与降级矩阵（锚点原文）**：事件洪水→采样降频；开关误开→
//! 隐私告警；流断→缓冲补传。
//!
//! **性能逐项分解（锚点原文）**：采集 O(1)；采样 O(1)；补传 O(积压)。
//!
//! **跨批对接点（锚点原文）**：F5203-F5209 事件源；F5212 文档消费。
//!
//! **无障碍与隐私（锚点原文）**：遥测即隐私红线执行件（默认关）；不
//! 含内容。
//!
//! # 深化说明：遥测的三条命脉——红线比采集先存在，降频比丢弃先告知，
//! ## 积压比丢失先兜底
//!
//! 「只记事件与计数不记内容」如果只是文档承诺，第一个加需求的迭代就
//! 会把玩家文本塞进事件——红线必须落成**类型面闸**：事件体只有特效名
//! （索引键）+计数+耗时，`has_content` 标志的事件在 [`TelemetryHub::emit`]
//! 入口即拦 [`E_TM_PRIVACY`]；「默认关」落成构造即 false 的
//! [`PrivacyGate`]——未显式开的遥测**一条不采**（not_collected 计数
//! 显性化），开闸路径必须携带显式同意凭证（`explicit-user-opt-in`），
//! 其他路径想开→隐私告警+保持关——开关误开比采集泄露便宜一百倍；
//! 「事件洪水→采样降频」落成 O(1) 步长采样（[`Sampler`]）：超阈值瞬
//! 间步长翻倍并显性告知（[`E_TM_FLOOD`]——降频不静默），低流量迟滞回
//! 落（settle 达标步长减半——降 2 升 1 保守方向）；「流断→缓冲补传」
//! 落成有界积压环（[`ReplayBuffer`]）：sink 失败事件进缓冲（满载丢最
//! 旧**计数不静默**），恢复后 flush O(积压) 全量补传。
//!
//! ## 深化一：三流分账互不串——使用率/性能/密度各是独立账本
//!
//! 使用率流（[`UsageStream`]）：特效名→使用次数（BTreeMap O(log n)），
//! 闲置名册（[`UsageStream::idle_list`]）按 F5209 资产全集点名零使用
//! 特效——闲置是优化的第一信号；性能开销流（[`PerfStream`]）：count/
//! total/max/min 四元画像逐特效累计（采集 O(1)——四次算术），平均耗时
//! 整数除法（判据手算锚）；组合密度流（[`DensityStream`]）：场景闭集
//! （F5208 三类同口径）外拒收（[`E_TM_BAD_SCENE`]），samples/sum/peak
//! 三元统计——峰值密度是节制律的反光镜。三流账本独立守恒（判据-03
//! 对拍：混流灌入后各流计数恰等于各流事件数，无串账）。
//!
//! ## 深化二：采集与外发分离——采样降的是外发频不是本地账
//!
//! 本地三流账永远全记（O(1) 内存账本——本地账是真相）；sink 外发走
//! [`Sampler::admit`] O(1) 判定（窗口计数+步长取模）——洪水时外发降
//! 频，本地账不丢（「哪些特效被用」的真相不因网络抖动而残缺）。补传
//! （[`TelemetryHub::flush`]）O(积压) 一次清空。
//!
//! ## 深化三：消费接口是注册制——重复注册与空名都进不了花名册
//!
//! sink 按名注册（[`TelemetryHub::register_sink`]），重复注册与空名
//! [`E_TM_BAD_SINK`]——消费接口的花名册唯一，遥测事件不会同名双投。

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_TM_* 前缀）
// ---------------------------------------------------------------------------

/// 隐私红线：内容入遥测 / 开关误开路径。
pub const E_TM_PRIVACY: &str = "E_TM_PRIVACY";
/// 事件洪水：采样降频已触发（显性告知，本地账不丢）。
pub const E_TM_FLOOD: &str = "E_TM_FLOOD";
/// 流断：sink 投递失败，事件已入缓冲待补传。
pub const E_TM_GAP: &str = "E_TM_GAP";
/// 消费接口非法：sink 名重复注册或空名。
pub const E_TM_BAD_SINK: &str = "E_TM_BAD_SINK";
/// 密度流场景非法：场景名不在三类闭集内。
pub const E_TM_BAD_SCENE: &str = "E_TM_BAD_SCENE";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_TM_PRIVACY => "隐私红线：内容入遥测或开关误开",
        E_TM_FLOOD => "事件洪水：采样降频已触发（本地账不丢）",
        E_TM_GAP => "流断：sink 投递失败已入缓冲",
        E_TM_BAD_SINK => "消费接口非法：重复注册或空名",
        E_TM_BAD_SCENE => "密度流场景非法：不在三类闭集",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_TM_PRIVACY => "剥离内容或走显式同意路径开遥测",
        E_TM_FLOOD => "知悉外发已降频，本地账完整",
        E_TM_GAP => "等待链路恢复后 flush 补传",
        E_TM_BAD_SINK => "换唯一 sink 名再注册",
        E_TM_BAD_SCENE => "改用 battle/ambience/cinematic 闭集内场景",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、隐私红线：脱敏开关（默认关显式开）+ 内容拦截
// ---------------------------------------------------------------------------

/// 显式同意凭证（唯一合法开闸路径标记）。
pub const OPT_IN_TOKEN: &str = "explicit-user-opt-in";

/// 脱敏开关+内容红线闸（遥测即隐私红线执行件——默认关）。
#[derive(Clone, Debug)]
pub struct PrivacyGate {
    /// 是否显式开启（默认 false——遥测默认关）。
    pub opt_in: bool,
    /// 开关误开告警计数。
    pub privacy_alerts: u32,
    /// 内容拦截计数。
    pub content_blocks: u32,
}

impl PrivacyGate {
    /// 默认关（域本色——未显式同意不采集）。
    pub fn new() -> Self {
        PrivacyGate { opt_in: false, privacy_alerts: 0, content_blocks: 0 }
    }

    /// **开闸**：仅显式同意凭证合法；其他路径→隐私告警+保持原状态
    /// （开关误开→隐私告警——错误路径锚点）。
    pub fn turn_on(&mut self, token: &str) -> Result<(), &'static str> {
        if token != OPT_IN_TOKEN {
            self.privacy_alerts += 1;
            return Err(E_TM_PRIVACY);
        }
        self.opt_in = true;
        Ok(())
    }

    /// 显式关（用户随时可撤回同意）。
    pub fn turn_off(&mut self) {
        self.opt_in = false;
    }

    /// **内容红线**：携带内容的事件入口即拦（只记事件与计数不记内容）。
    pub fn admit_event(&mut self, has_content: bool) -> Result<(), &'static str> {
        if has_content {
            self.content_blocks += 1;
            return Err(E_TM_PRIVACY);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、遥测事件与三流账本
// ---------------------------------------------------------------------------

/// 遥测流三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    /// 使用率流：哪些特效被用哪些闲置。
    Usage,
    /// 性能开销流：逐特效耗时画像。
    Perf,
    /// 组合密度流：场景特效密度统计。
    Density,
}

/// 场景闭集（F5208 三类同口径）。
pub const SCENES: [&str; 3] = ["battle", "ambience", "cinematic"];

/// 单条遥测事件（只含索引键+计数+耗时——**无内容位**）。
#[derive(Clone, Debug)]
pub struct TmEvent {
    /// 所属流。
    pub stream: StreamKind,
    /// 特效名（索引键非内容）。
    pub fx: String,
    /// 场景名（密度流用；其他流空串）。
    pub scene: String,
    /// 计数（使用次数/密度样本值）。
    pub count: u64,
    /// 耗时（微秒；perf 流用）。
    pub us: u32,
    /// 内容标志（生产面恒 false；true 的事件是红线拦截对象）。
    pub has_content: bool,
}

/// 事件构造辅助（判据与事件源共用）。
pub fn ev(stream: StreamKind, fx: &str, count: u64, us: u32) -> TmEvent {
    TmEvent {
        stream,
        fx: String::from(fx),
        scene: String::new(),
        count,
        us,
        has_content: false,
    }
}

/// 密度事件构造辅助。
pub fn ev_density(fx: &str, scene: &str, live: u64) -> TmEvent {
    TmEvent {
        stream: StreamKind::Density,
        fx: String::from(fx),
        scene: String::from(scene),
        count: live,
        us: 0,
        has_content: false,
    }
}

/// 使用率流：特效名→使用次数。
#[derive(Clone, Debug, Default)]
pub struct UsageStream {
    uses: BTreeMap<String, u64>,
}

impl UsageStream {
    /// 记一次使用（采集 O(1) 均摊）。
    pub fn record_use(&mut self, fx: &str) {
        *self.uses.entry(String::from(fx)).or_insert(0) += 1;
    }

    /// 某特效使用次数（未用=0）。
    pub fn use_count(&self, fx: &str) -> u64 {
        self.uses.get(fx).copied().unwrap_or(0)
    }

    /// 使用总次数（守恒对拍面）。
    pub fn total_uses(&self) -> u64 {
        self.uses.values().sum()
    }

    /// **闲置名册**：按 F5209 资产全集点名零使用特效（组织 O(资产数)）
    /// ——闲置是优化的第一信号。
    pub fn idle_list(&self, registry: &[String]) -> Vec<String> {
        registry
            .iter()
            .filter(|n| self.use_count(n) == 0)
            .cloned()
            .collect()
    }
}

/// 单特效性能画像四元组。
#[derive(Clone, Copy, Debug, Default)]
pub struct PerfProfile {
    /// 采样次数。
    pub count: u64,
    /// 总耗时（微秒）。
    pub total_us: u64,
    /// 峰值耗时。
    pub max_us: u32,
    /// 谷值耗时。
    pub min_us: u32,
}

/// 性能开销流：逐特效耗时画像。
#[derive(Clone, Debug, Default)]
pub struct PerfStream {
    profiles: BTreeMap<String, PerfProfile>,
}

impl PerfStream {
    /// 记一次耗时（采集 O(1)——四次算术）。
    pub fn record_cost(&mut self, fx: &str, us: u32) {
        let p = self.profiles.entry(String::from(fx)).or_insert(PerfProfile {
            count: 0,
            total_us: 0,
            max_us: 0,
            min_us: u32::MAX,
        });
        p.count += 1;
        p.total_us += us as u64;
        if us > p.max_us {
            p.max_us = us;
        }
        if us < p.min_us {
            p.min_us = us;
        }
    }

    /// 画像查询。
    pub fn profile(&self, fx: &str) -> Option<&PerfProfile> {
        self.profiles.get(fx)
    }

    /// 采样总次数（守恒对拍面）。
    pub fn total_samples(&self) -> u64 {
        self.profiles.values().map(|p| p.count).sum()
    }

    /// 平均耗时（整数除法——count 0 返 0）。
    pub fn avg_us(&self, fx: &str) -> u64 {
        match self.profiles.get(fx) {
            Some(p) if p.count > 0 => p.total_us / p.count,
            _ => 0,
        }
    }
}

/// 单场景密度统计三元组。
#[derive(Clone, Copy, Debug, Default)]
pub struct DensityStat {
    /// 样本数。
    pub samples: u64,
    /// 在屏数总和。
    pub sum: u64,
    /// 峰值在屏数（节制律的反光镜）。
    pub peak: u64,
}

/// 组合密度流：场景→密度统计。
#[derive(Clone, Debug, Default)]
pub struct DensityStream {
    stats: BTreeMap<String, DensityStat>,
}

impl DensityStream {
    /// 记一次密度样本（场景闭集外拒——调用前用 [`scene_legal`] 校验）。
    pub fn record_density(&mut self, scene: &str, live: u64) -> Result<(), &'static str> {
        if !scene_legal(scene) {
            return Err(E_TM_BAD_SCENE);
        }
        let st = self.stats.entry(String::from(scene)).or_insert(DensityStat {
            samples: 0,
            sum: 0,
            peak: 0,
        });
        st.samples += 1;
        st.sum += live;
        if live > st.peak {
            st.peak = live;
        }
        Ok(())
    }

    /// 统计查询。
    pub fn stat(&self, scene: &str) -> Option<&DensityStat> {
        self.stats.get(scene)
    }

    /// 样本总数（守恒对拍面）。
    pub fn total_samples(&self) -> u64 {
        self.stats.values().map(|s| s.samples).sum()
    }
}

/// 场景合法性（O(1)——三闭集线性）。
pub fn scene_legal(scene: &str) -> bool {
    SCENES.contains(&scene)
}

// ---------------------------------------------------------------------------
// 三、采样器：事件洪水→采样降频（O(1) 步长，迟滞回落）
// ---------------------------------------------------------------------------

/// 迟滞回落所需平静采样数（降 2 升 1 保守方向）。
pub const SETTLE_LIMIT: u32 = 4;

/// 采样器：窗口计数+步长取模（采集 O(1)/采样 O(1)——锚点双 O(1)）。
#[derive(Clone, Debug)]
pub struct Sampler {
    /// 洪水阈值（单窗口事件数）。
    pub flood_threshold: u64,
    /// 当前窗口事件计数（帧界重置）。
    window_seen: u64,
    /// 本窗口是否已触发降频（每窗口至多翻倍一次——防指数爆炸）。
    window_flooded: bool,
    /// 本窗口首次越阈标志（emit 读以显性告知降频——读后即清）。
    pub just_flooded: bool,
    /// 采样步长（1=全过；洪水翻倍；平静迟滞减半）。
    pub stride: u64,
    /// 外发通过计数。
    pub sampled: u64,
    /// 平静采样计数（回落累积）。
    settle: u32,
}

impl Sampler {
    /// 以洪水阈值建采样器。
    pub fn new(flood_threshold: u64) -> Self {
        Sampler {
            flood_threshold,
            window_seen: 0,
            window_flooded: false,
            just_flooded: false,
            stride: 1,
            sampled: 0,
            settle: 0,
        }
    }

    /// **采样判定**：返回 true=本事件放行外发。窗口首次越阈→步长翻倍
    /// （降频——`just_flooded` 置位供调用方以 [`E_TM_FLOOD`] 显性告知）；
    /// 平静采样累积达标→步长减半回落（迟滞——不抖动）。全程 O(1)。
    pub fn admit(&mut self) -> bool {
        self.window_seen += 1;
        let flooded = self.window_seen > self.flood_threshold;
        if flooded && !self.window_flooded {
            self.stride = self.stride * 2;
            self.window_flooded = true;
            self.just_flooded = true;
            self.settle = 0;
        } else {
            self.just_flooded = false;
        }
        let pass = (self.window_seen - 1) % self.stride == 0;
        if pass {
            self.sampled += 1;
            if !flooded && self.stride > 1 {
                self.settle += 1;
                if self.settle >= SETTLE_LIMIT {
                    self.stride /= 2;
                    self.settle = 0;
                }
            }
        }
        pass
    }

    /// 窗口重置（帧界调用——洪水标志随窗口清零）。
    pub fn new_window(&mut self) {
        self.window_seen = 0;
        self.window_flooded = false;
        self.just_flooded = false;
    }
}

// ---------------------------------------------------------------------------
// 四、补传缓冲：流断积压→恢复 flush（O(积压)）
// ---------------------------------------------------------------------------

/// 补传缓冲默认容量。
pub const BUFFER_CAP: usize = 1024;

/// 有界积压缓冲：sink 失败事件暂存（满载丢最旧**计数不静默**）。
#[derive(Clone, Debug)]
pub struct ReplayBuffer {
    pending: Vec<TmEvent>,
    cap: usize,
    /// 满载丢弃计数。
    pub dropped: u64,
}

impl ReplayBuffer {
    /// 以容量建缓冲。
    pub fn new(cap: usize) -> Self {
        ReplayBuffer { pending: Vec::new(), cap, dropped: 0 }
    }

    /// 入缓冲（满载丢最旧+计数）。
    pub fn push(&mut self, e: TmEvent) {
        if self.pending.len() >= self.cap {
            self.pending.remove(0);
            self.dropped += 1;
        }
        self.pending.push(e);
    }

    /// 积压数。
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// **flush**（O(积压)）：逐条送 sink；sink 中途再断→剩余保留续等
    /// （返回已补传数）。
    pub fn flush(&mut self, sink: &mut dyn TmSink) -> usize {
        let mut sent = 0usize;
        while !self.pending.is_empty() {
            let r = sink.deliver(&self.pending[0]);
            match r {
                Ok(()) => {
                    self.pending.remove(0);
                    sent += 1;
                }
                Err(_) => break,
            }
        }
        sent
    }
}

// ---------------------------------------------------------------------------
// 五、消费接口：注册制 sink
// ---------------------------------------------------------------------------

/// 遥测消费接口（F5212 文档消费等实现此口）。
pub trait TmSink {
    /// 投递一条事件（失败=流断，由调用方积压补传）。
    fn deliver(&mut self, e: &TmEvent) -> Result<(), &'static str>;
    /// sink 名（花名册键）。
    fn name(&self) -> &'static str;
}

/// 判据/测试用记忆 sink（可模拟断流 N 次）。
#[derive(Clone, Debug, Default)]
pub struct MemSink {
    pub got: Vec<TmEvent>,
    /// 剩余失败次数（>0 时投递失败并递减——流断模拟）。
    pub fail_next: u32,
}

impl TmSink for MemSink {
    fn deliver(&mut self, e: &TmEvent) -> Result<(), &'static str> {
        if self.fail_next > 0 {
            self.fail_next -= 1;
            return Err(E_TM_GAP);
        }
        self.got.push(e.clone());
        Ok(())
    }

    fn name(&self) -> &'static str {
        "mem"
    }
}

// ---------------------------------------------------------------------------
// 六、三流遥测器总编排：红线→开关→三流账→采样外发→积压补传
// ---------------------------------------------------------------------------

/// 三流遥测器（总编排——F5203-F5209 事件源的汇入口）。
#[derive(Clone, Debug)]
pub struct TelemetryHub {
    /// 隐私红线闸。
    pub gate: PrivacyGate,
    /// 流 1：使用率。
    pub usage: UsageStream,
    /// 流 2：性能开销。
    pub perf: PerfStream,
    /// 流 3：组合密度。
    pub density: DensityStream,
    /// 采样器（外发降频）。
    pub sampler: Sampler,
    /// 补传缓冲。
    pub buffer: ReplayBuffer,
    sink_names: Vec<String>,
    /// 默认关期间未采集事件计数（显性化——不是静默丢弃）。
    pub not_collected: u64,
    /// 红线拦截计数（内容/误开合计）。
    pub privacy_blocks: u32,
}

impl TelemetryHub {
    /// 以洪水阈值建遥测器（默认关）。
    pub fn new(flood_threshold: u64) -> Self {
        TelemetryHub {
            gate: PrivacyGate::new(),
            usage: UsageStream::default(),
            perf: PerfStream::default(),
            density: DensityStream::default(),
            sampler: Sampler::new(flood_threshold),
            buffer: ReplayBuffer::new(BUFFER_CAP),
            sink_names: Vec::new(),
            not_collected: 0,
            privacy_blocks: 0,
        }
    }

    /// **注册消费接口**：重复注册与空名 [`E_TM_BAD_SINK`]（花名册唯一）。
    pub fn register_sink(&mut self, name: &str) -> Result<(), &'static str> {
        if name.is_empty() || self.sink_names.iter().any(|n| n == name) {
            return Err(E_TM_BAD_SINK);
        }
        self.sink_names.push(String::from(name));
        Ok(())
    }

    /// 已注册 sink 数。
    pub fn sink_count(&self) -> usize {
        self.sink_names.len()
    }

    /// **采集**（锚点 O(1)）：内容红线→默认关闸→本地三流账→采样外发
    /// （失败积压）。返回 Err 的语义：
    /// - [`E_TM_PRIVACY`]：含内容（拦）或未显式开（不采集）——红线执行；
    /// - [`E_TM_FLOOD`]：窗口首次越阈降频（**事件已投递/已入账**，返回
    ///   值仅显性告知降频——降频不静默）；
    /// - [`E_TM_GAP`]：投递失败已积压（等 flush）。
    pub fn emit(&mut self, e: TmEvent, sink: Option<&mut dyn TmSink>) -> Result<(), &'static str> {
        // 闸 1：内容红线（只记事件与计数不记内容）——计数先落账再冒泡。
        let gate_r = self.gate.admit_event(e.has_content);
        self.privacy_blocks = self.gate.content_blocks + self.gate.privacy_alerts;
        gate_r?;
        // 闸 2：默认关——未显式开不采集（not_collected 显性化）。
        if !self.gate.opt_in {
            self.not_collected += 1;
            return Err(E_TM_PRIVACY);
        }
        // 闸 3：本地三流账（全记——采样只降外发频，真相不残缺）。
        match e.stream {
            StreamKind::Usage => self.usage.record_use(&e.fx),
            StreamKind::Perf => self.perf.record_cost(&e.fx, e.us),
            StreamKind::Density => {
                self.density.record_density(&e.scene, e.count)?;
            }
        }
        // 闸 4：采样外发（O(1) 判定）。
        let pass = self.sampler.admit();
        let just_flooded = self.sampler.just_flooded;
        if !pass {
            return Ok(());
        }
        let deliver_r = match sink {
            Some(sk) => match sk.deliver(&e) {
                Ok(()) => Ok(()),
                Err(_) => {
                    self.buffer.push(e);
                    Err(E_TM_GAP)
                }
            },
            None => {
                self.buffer.push(e);
                Err(E_TM_GAP)
            }
        };
        // 降频显性告知优先于投递结果（事件本身已落账/已投递）。
        if just_flooded {
            return Err(E_TM_FLOOD);
        }
        deliver_r
    }

    /// **补传**（O(积压)）：链路恢复后一次清空积压。
    pub fn flush(&mut self, sink: &mut dyn TmSink) -> usize {
        self.buffer.flush(sink)
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5210 域自检（判据 14 条七组：三流/默认关/采样/补传/接口/守恒/判据元）。
pub fn run_vez10_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5210");

    // --- 三流（判据一）---
    // 三-01：使用率流——记账+闲置名册点名（5 资产用 2，闲置 3 可点名）。
    let mut us = UsageStream::default();
    us.record_use("fx_p_hit_spark");
    us.record_use("fx_p_hit_spark");
    us.record_use("fx_e_fog_glow");
    let reg: Vec<String> = ["fx_p_hit_spark", "fx_e_fog_glow", "fx_t_fade_black", "fx_i_panel_pulse", "fx_s_seed"]
        .iter()
        .map(|n| String::from(*n))
        .collect();
    let idle = us.idle_list(&reg);
    let usage_ok = us.use_count("fx_p_hit_spark") == 2
        && us.use_count("fx_t_fade_black") == 0
        && us.total_uses() == 3
        && idle.len() == 3
        && idle.iter().any(|n| n == "fx_t_fade_black")
        && !idle.iter().any(|n| n == "fx_p_hit_spark");
    if usage_ok {
        s.ok("EZ10-三-01-使用率流与闲置点名");
    } else {
        s.fail("EZ10-三-01-使用率流与闲置点名", "使用率账失效");
    }
    // 三-02：性能开销流——画像手算锚（10/20/30 → count3 total60 avg20 max30 min10）。
    let mut ps = PerfStream::default();
    ps.record_cost("fx_e_rain", 10);
    ps.record_cost("fx_e_rain", 30);
    ps.record_cost("fx_e_rain", 20);
    let prof_ok = match ps.profile("fx_e_rain") {
        Some(p) => p.count == 3 && p.total_us == 60 && p.max_us == 30 && p.min_us == 10,
        None => false,
    } && ps.avg_us("fx_e_rain") == 20
        && ps.avg_us("fx_p_ghost") == 0
        && ps.total_samples() == 3;
    if prof_ok {
        s.ok("EZ10-三-02-性能画像手算锚");
    } else {
        s.fail("EZ10-三-02-性能画像手算锚", "画像账失效");
    }
    // 三-03：密度流——场景闭集+峰值对拍（5/8/3 → peak8 sum16 samples3）+非法场景拒。
    let mut ds = DensityStream::default();
    let d1 = ds.record_density("battle", 5);
    let d2 = ds.record_density("battle", 8);
    let d3 = ds.record_density("battle", 3);
    let dbad = ds.record_density("nightmare", 1) == Err(E_TM_BAD_SCENE);
    let dens_ok = d1.is_ok() && d2.is_ok() && d3.is_ok() && dbad
        && match ds.stat("battle") {
            Some(st) => st.samples == 3 && st.sum == 16 && st.peak == 8,
            None => false,
        }
        && ds.total_samples() == 3
        && ds.stat("nightmare").is_none();
    if dens_ok {
        s.ok("EZ10-三-03-密度闭集与峰值");
    } else {
        s.fail("EZ10-三-03-密度闭集与峰值", "密度账或场景闸失效");
    }

    // --- 默认关（判据二）---
    // 默-01：默认关——未显式开 emit 不采集（not_collected 计数显性化）。
    let mut h1 = TelemetryHub::new(8);
    let r = h1.emit(ev(StreamKind::Usage, "fx_p_hit_spark", 1, 0), None);
    let off_ok = h1.gate.opt_in == false
        && r == Err(E_TM_PRIVACY)
        && h1.not_collected == 1
        && h1.usage.total_uses() == 0
        && h1.buffer.pending() == 0;
    if off_ok {
        s.ok("EZ10-默-01-默认关一条不采");
    } else {
        s.fail("EZ10-默-01-默认关一条不采", "默认关失效");
    }
    // 默-02：显式开合法凭证；误开路径→隐私告警+保持关。
    let mut h2 = TelemetryHub::new(8);
    let mis = h2.gate.turn_on("internal-metrics") == Err(E_TM_PRIVACY)
        && h2.gate.opt_in == false
        && h2.gate.privacy_alerts == 1;
    let ok_on = h2.gate.turn_on(OPT_IN_TOKEN).is_ok() && h2.gate.opt_in == true;
    let mut mem2 = MemSink::default();
    let e2r = h2.emit(ev(StreamKind::Usage, "fx_p_hit_spark", 1, 0), Some(&mut mem2));
    let collect_ok = e2r.is_ok() && h2.usage.total_uses() == 1 && mem2.got.len() == 1;
    if mis && ok_on && collect_ok {
        s.ok("EZ10-默-02-显式开与误开告警");
    } else {
        s.fail("EZ10-默-02-显式开与误开告警", "开关闸失效");
    }
    // 默-03：内容红线——携带内容的事件入口即拦（不入账不外发）。
    let mut h3 = TelemetryHub::new(8);
    let _ = h3.gate.turn_on(OPT_IN_TOKEN);
    let mut bad = ev(StreamKind::Usage, "fx_p_hit_spark", 1, 0);
    bad.has_content = true;
    let mut mem3 = MemSink::default();
    let rb = h3.emit(bad, Some(&mut mem3));
    let content_ok = rb == Err(E_TM_PRIVACY)
        && h3.gate.content_blocks == 1
        && h3.privacy_blocks == 1
        && h3.usage.total_uses() == 0
        && mem3.got.is_empty();
    if content_ok {
        s.ok("EZ10-默-03-内容红线入口拦截");
    } else {
        s.fail("EZ10-默-03-内容红线入口拦截", "红线漏");
    }

    // --- 采样（判据三）---
    // 采-01：洪水降频——阈值 8 内全过；第 9 个触发翻倍并显性告知；本地账全记。
    let mut h4 = TelemetryHub::new(8);
    let _ = h4.gate.turn_on(OPT_IN_TOKEN);
    let _ = h4.register_sink("mem");
    let mut mem4 = MemSink::default();
    let mut flood_flag = false;
    let mut i = 0u64;
    while i < 8 {
        if h4.emit(ev(StreamKind::Perf, "fx_e_rain", 1, 10), Some(&mut mem4)).is_err() {
            flood_flag = true;
        }
        i += 1;
    }
    let r9 = h4.emit(ev(StreamKind::Perf, "fx_e_rain", 1, 10), Some(&mut mem4));
    let r10 = h4.emit(ev(StreamKind::Perf, "fx_e_rain", 1, 10), Some(&mut mem4));
    let flood_ok = !flood_flag
        && r9 == Err(E_TM_FLOOD)
        && r10.is_ok()
        && h4.sampler.stride == 2
        && h4.sampler.sampled == 9
        && mem4.got.len() == 9
        && h4.perf.total_samples() == 10
        && h4.usage.total_uses() == 0
        && h4.density.total_samples() == 0;
    if flood_ok {
        s.ok("EZ10-采-01-洪水降频显性告知");
    } else {
        s.fail("EZ10-采-01-洪水降频显性告知", "采样或分账失效");
    }
    // 采-02：迟滞回落——窗口重置后平静采样累积达标步长减半回 1。
    let mut sm = Sampler::new(8);
    let mut j = 0u64;
    while j < 9 {
        let _ = sm.admit();
        j += 1;
    }
    let stride_hi = sm.stride >= 2;
    sm.new_window();
    let mut k = 0u64;
    while k < 8 {
        let _ = sm.admit();
        k += 1;
    }
    let settle_ok = stride_hi && sm.stride == 1 && sm.settle < SETTLE_LIMIT;
    if settle_ok {
        s.ok("EZ10-采-02-迟滞回落步长归一");
    } else {
        s.fail("EZ10-采-02-迟滞回落步长归一", "回落失效");
    }

    // --- 补传（判据四）---
    // 补-01：流断积压→恢复 flush O(积压) 全量补传+清空。
    let mut h5 = TelemetryHub::new(64);
    let _ = h5.gate.turn_on(OPT_IN_TOKEN);
    let mut mem5 = MemSink::default();
    mem5.fail_next = 3;
    let g1 = h5.emit(ev(StreamKind::Usage, "fx_p_a", 1, 0), Some(&mut mem5));
    let g2 = h5.emit(ev(StreamKind::Usage, "fx_p_b", 1, 0), Some(&mut mem5));
    let g3 = h5.emit(ev(StreamKind::Usage, "fx_p_c", 1, 0), Some(&mut mem5));
    let buffered = g1 == Err(E_TM_GAP) && g2 == Err(E_TM_GAP) && g3 == Err(E_TM_GAP)
        && h5.buffer.pending() == 3
        && h5.usage.total_uses() == 3;
    mem5.fail_next = 0;
    let sent = h5.flush(&mut mem5);
    let flush_ok = buffered && sent == 3 && h5.buffer.pending() == 0 && mem5.got.len() == 3;
    if flush_ok {
        s.ok("EZ10-补-01-流断积压补传闭合");
    } else {
        s.fail("EZ10-补-01-流断积压补传闭合", "补传失效");
    }
    // 补-02：满载丢最旧不静默（容量 4 灌 6 → pending4 dropped2）。
    let mut buf = ReplayBuffer::new(4);
    let mut n = 0u64;
    while n < 6 {
        buf.push(ev(StreamKind::Usage, "fx_p_x", n, 0));
        n += 1;
    }
    let cap_ok = buf.pending() == 4 && buf.dropped == 2;
    if cap_ok {
        s.ok("EZ10-补-02-满载丢最旧计数不静默");
    } else {
        s.fail("EZ10-补-02-满载丢最旧计数不静默", "满载策略失效");
    }

    // --- 消费接口（判据五）---
    // 接-01：重复注册与空名拒（花名册唯一）。
    let mut h6 = TelemetryHub::new(8);
    let s1 = h6.register_sink("mem");
    let s2 = h6.register_sink("mem");
    let s3 = h6.register_sink("");
    let sink_ok = s1.is_ok()
        && s2 == Err(E_TM_BAD_SINK)
        && s3 == Err(E_TM_BAD_SINK)
        && h6.sink_count() == 1;
    if sink_ok {
        s.ok("EZ10-接-01-sink注册唯一性");
    } else {
        s.fail("EZ10-接-01-sink注册唯一性", "注册闸漏");
    }

    // --- 三流守恒（判据六）---
    // 守-01：混流灌入各流账独立守恒（无串账）。
    let mut h7 = TelemetryHub::new(64);
    let _ = h7.gate.turn_on(OPT_IN_TOKEN);
    let mut mem7 = MemSink::default();
    let _ = h7.emit(ev(StreamKind::Usage, "fx_p_a", 1, 0), Some(&mut mem7));
    let _ = h7.emit(ev(StreamKind::Usage, "fx_p_a", 1, 0), Some(&mut mem7));
    let _ = h7.emit(ev(StreamKind::Perf, "fx_p_a", 1, 15), Some(&mut mem7));
    let _ = h7.emit(ev_density("fx_p_a", "battle", 3), Some(&mut mem7));
    let _ = h7.emit(ev_density("fx_p_a", "ambience", 2), Some(&mut mem7));
    let mix_ok = h7.usage.total_uses() == 2
        && h7.perf.total_samples() == 1
        && h7.density.total_samples() == 2
        && h7.density.stat("battle").is_some()
        && h7.usage.use_count("fx_e_none") == 0
        && mem7.got.len() == 5;
    if mix_ok {
        s.ok("EZ10-守-01-三流分账独立守恒");
    } else {
        s.fail("EZ10-守-01-三流分账独立守恒", "串账");
    }

    // --- 判据元（判据七）---
    // 元-01：五码互异 + 三要素齐备。
    let codes = [E_TM_PRIVACY, E_TM_FLOOD, E_TM_GAP, E_TM_BAD_SINK, E_TM_BAD_SCENE];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ10-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ10-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic 零 unsafe（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez10_telemetry.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ10-判据-02-零panic零unsafe面");
    } else {
        s.fail("EZ10-判据-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元-03：隐私口径自洽——事件构造默认无内容+场景闭集与 F5208 三类同名。
    let probe = ev(StreamKind::Usage, "fx_p_probe", 1, 0);
    let mut self_ok = !probe.has_content && SCENES.len() == 3;
    for sc in SCENES.iter() {
        if !scene_legal(sc) {
            self_ok = false;
        }
    }
    if scene_legal("ghost_scene") {
        self_ok = false;
    }
    if self_ok {
        s.ok("EZ10-判据-03-隐私与场景口径自洽");
    } else {
        s.fail("EZ10-判据-03-隐私与场景口径自洽", "口径漂移");
    }

    s
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
