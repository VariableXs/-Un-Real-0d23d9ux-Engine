//! CGPU-F0010 · 上传下载带宽调度器（cga10_bandwidth_sched）
//!
//! CPU→GPU 的纹理/几何上传是**最容易被忽视的帧杀手**——本单把上
//! 传变成**有预算的 politely 并行**：上传队列与渲染并行但限速，按帧
//! 预算动态分配上传带宽（**默认每帧 ≤1.5ms 拷贝时间片**——时间片
//! 耗尽上传立即让路，帧超时零贡献绝不挤占渲染）；大资产（4K 纹理
//! 32MB 级）**强制分片跨帧渐进上传**，配合 **mipmap 渐进**（先传低
//! 分辨率 preview 级保证可见，高级别后台补齐——可见时刻远早于完成
//! 时刻）；下载（截图/读回）走**低优先级通道**与上传对称限速（独立
//! 切片上传之后消费——读回不抢帧带宽）；**拥塞归因**：上传队列深度
//! 与 PCIe 利用率计数联动，带宽瓶颈自动标定（迟滞防抖）并产出**放
//! 置建议**（带宽紧时可 CPU 计算的任务留在 CPU——对接 CGPU-F0004
//! 放置决策器的信号面）。
//!
//! 数据结构：上传/下载双队列；拥塞探针；遥测账本。
//!
//! 错误路径与降级矩阵：**时间片耗尽→零贡献让路**（正常机制——渲染
//! 优先是宪法）；**大资产未分片→强制分片**（单帧容量外必须跨帧）；
//! **队列满→拒绝+计数**；**拥塞→归因标定+放置建议**。性能：O(任务)
//! 单遍消费。跨批对接点：CGPU-F0004 放置决策器（PlacementHint 信
//! 号面）/cgi01_bandwidth I 域账户化带宽总架构（本单是 A 域帧级调
//! 度实现位）/cgc08 卡顿归因（时间片让路可解释）。
//! 无障碍：遥测 spoken 读屏可达。隐私面：零内容只记字节与耗时。
//!
//! 诊断码 0x3E 段五码互异：`E_BW_QUEUE_FULL` / `E_BW_BIG_UNCHUNKED`
//! / `E_BW_CONGESTION` / `E_BW_BAD_JOB` / `E_BW_PCIE_STALL`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 带宽调度器版本。
pub const BW_SCHED_VERSION: &str = "A10-bwsched-v1";
/// 每帧上传拷贝时间片（µs——锚点原文 1.5ms 恰值）。
pub const COPY_SLICE_US: u64 = 1500;
/// 每帧下载切片（µs——与上传对称的低优先级半片）。
pub const DOWN_SLICE_US: u64 = 750;
/// 拷贝速率（bytes/µs=16KB——PCIe 4.0 x16 保守标定 16GB/s）。
pub const COPY_RATE_BYTES_PER_US: u64 = 16 * 1024;
/// 单帧上传容量（手算锚：1500×16384=24,576,000 bytes=24MB）。
pub const FRAME_UPLOAD_BYTES: u64 = COPY_SLICE_US * COPY_RATE_BYTES_PER_US;
/// 大资产阈值（32MB——4K 纹理级，超此强制分片）。
pub const BIG_ASSET_BYTES: u64 = 32 * 1024 * 1024;
/// mipmap preview 级尺寸占比（1/8 线性——先传保证可见）。
pub const MIP_PREVIEW_NUM: u64 = 1;
pub const MIP_PREVIEW_DEN: u64 = 64; // (1/8)^2 面积比
/// 上传队列深度告警阈值。
pub const QUEUE_DEPTH_ALERT: usize = 8;
/// 队列硬容量。
pub const QUEUE_CAP: usize = 32;
/// PCIe 利用率告警阈值（‰）。
pub const PCIE_UTIL_ALERT_PERMILLE: u64 = 900;
/// 拥塞标定迟滞：连续紧帧数。
pub const CONGEST_ON_FRAMES: u32 = 2;
/// 拥塞解除迟滞：连续松帧数。
pub const CONGEST_OFF_FRAMES: u32 = 3;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0x3E 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 队列满：提交被拒（容量硬顶）。
pub const E_BW_QUEUE_FULL: u16 = 0x3E01;
/// 大资产未声明分片：提交即强制分片并留痕。
pub const E_BW_BIG_UNCHUNKED: u16 = 0x3E02;
/// 拥塞标定：带宽瓶颈成立（迟滞通过）。
pub const E_BW_CONGESTION: u16 = 0x3E03;
/// 查无任务/未知类型：job 引用无效。
pub const E_BW_BAD_JOB: u16 = 0x3E04;
/// PCIe 停顿：利用率超阈且队列深——传输层异常告警。
pub const E_BW_PCIE_STALL: u16 = 0x3E05;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_BW_QUEUE_FULL => "队列满：容量硬顶拒绝提交并计数",
        E_BW_BIG_UNCHUNKED => "大资产未分片：单帧容量外强制跨帧",
        E_BW_CONGESTION => "拥塞标定：队列深与 PCIe 利用率联动成立",
        E_BW_BAD_JOB => "查无任务：job 引用无效或类型未知",
        E_BW_PCIE_STALL => "PCIe 停顿：利用率超阈且队列深",
        _ => "未知诊断码",
    }
}

/// 立案簿（0x3E 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 任务模型（上传 job 带 mipmap 渐进计划；下载 job 低优先级）
// ---------------------------------------------------------------------------

/// 资产类型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 4K 纹理（32MB 级——强制分片+渐进可见）。
    Texture4K,
    /// 几何（顶点/索引缓冲）。
    Geometry,
    /// 截图（下载侧）。
    Screenshot,
    /// 读回（下载侧）。
    Readback,
}

impl AssetKind {
    pub fn is_download(self) -> bool {
        matches!(self, AssetKind::Screenshot | AssetKind::Readback)
    }

    pub fn zh(self) -> &'static str {
        match self {
            AssetKind::Texture4K => "4K纹理",
            AssetKind::Geometry => "几何",
            AssetKind::Screenshot => "截图",
            AssetKind::Readback => "读回",
        }
    }
}

/// 上传任务（分片渐进状态机——sent 推进+mipmap 可见判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadJob {
    pub id: u32,
    pub kind: AssetKind,
    pub total: u64,
    /// 已传字节。
    pub sent: u64,
    /// preview 级字节数（低分辨率级——传完即可见）。
    pub preview_bytes: u64,
    /// preview 级已传。
    pub preview_sent: u64,
    /// 是否强制分片（大资产）。
    pub chunked: bool,
}

impl UploadJob {
    /// 构造（大资产自动强制分片并留痕标记——enforce 语义在构造面）。
    pub fn new(id: u32, kind: AssetKind, total: u64) -> (UploadJob, bool) {
        let big = total > FRAME_UPLOAD_BYTES;
        let preview = if kind == AssetKind::Texture4K {
            total * MIP_PREVIEW_NUM / MIP_PREVIEW_DEN
        } else {
            0
        };
        (
            UploadJob {
                id,
                kind,
                total,
                sent: 0,
                preview_bytes: preview,
                preview_sent: 0,
                chunked: big,
            },
            big,
        )
    }

    /// 是否可见（mipmap 渐进：preview 级传完即低清可见——远早于完成）。
    pub fn visible(&self) -> bool {
        self.preview_bytes == 0 || self.preview_sent >= self.preview_bytes
    }

    /// 是否完成。
    pub fn done(&self) -> bool {
        self.sent >= self.total
    }

    /// 进度（千分比）。
    pub fn permille(&self) -> u64 {
        if self.total == 0 {
            return 1000;
        }
        self.sent * 1000 / self.total
    }
}

/// 下载任务（低优先级——读回不抢帧带宽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadJob {
    pub id: u32,
    pub kind: AssetKind,
    pub total: u64,
    pub got: u64,
}

impl DownloadJob {
    pub fn new(id: u32, kind: AssetKind, total: u64) -> DownloadJob {
        DownloadJob { id, kind, total, got: 0 }
    }

    pub fn done(&self) -> bool {
        self.got >= self.total
    }
}

// ---------------------------------------------------------------------------
// 拥塞探针（队列深度 × PCIe 利用率联动 + 迟滞防抖 + 放置建议）
// ---------------------------------------------------------------------------

/// 瓶颈标定闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bottleneck {
    /// 无瓶颈（正常）。
    None,
    /// PCIe 饱和（带宽紧——可 CPU 计算任务留 CPU）。
    PcieSat,
}

/// 放置建议（对接 CGPU-F0004 放置决策器的信号面——纯数据不依赖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementHint {
    /// 带宽紧时 true：可 CPU 计算的任务留在 CPU。
    pub keep_on_cpu: bool,
    /// 建议依据（瓶颈标定）。
    pub cause: Bottleneck,
    /// 上传队列深度快照。
    pub depth: usize,
    /// PCIe 利用率快照（‰）。
    pub util_permille: u64,
}

/// 拥塞探针（迟滞防抖——连续紧 2 帧标定/连续松 3 帧解除）。
#[derive(Debug)]
pub struct CongestionProbe {
    pub state: Bottleneck,
    tight_run: u32,
    loose_run: u32,
    pub on_events: u32,
    pub off_events: u32,
}

impl CongestionProbe {
    pub fn new() -> CongestionProbe {
        CongestionProbe {
            state: Bottleneck::None,
            tight_run: 0,
            loose_run: 0,
            on_events: 0,
            off_events: 0,
        }
    }

    /// 帧探针（每帧喂数：队列深度+PCIe 利用率‰——联动判定）。
    /// 返回 (当前标定, 是否刚发生状态翻转, 本帧建议)。
    pub fn step(&mut self, depth: usize, util_permille: u64, bag: &mut DiagBag) -> (Bottleneck, bool, PlacementHint) {
        let tight = depth >= QUEUE_DEPTH_ALERT || util_permille >= PCIE_UTIL_ALERT_PERMILLE;
        let mut flipped = false;
        match self.state {
            Bottleneck::None => {
                if tight {
                    self.tight_run += 1;
                    self.loose_run = 0;
                    if self.tight_run >= CONGEST_ON_FRAMES {
                        self.state = Bottleneck::PcieSat;
                        self.on_events += 1;
                        self.tight_run = 0;
                        flipped = true;
                        bag.push(
                            E_BW_CONGESTION,
                            "带宽瓶颈标定",
                            &format!("深{} 利用{}‰", depth, util_permille),
                        );
                    }
                } else {
                    self.tight_run = 0;
                }
            }
            Bottleneck::PcieSat => {
                if tight {
                    self.loose_run = 0;
                } else {
                    self.loose_run += 1;
                    if self.loose_run >= CONGEST_OFF_FRAMES {
                        self.state = Bottleneck::None;
                        self.off_events += 1;
                        self.loose_run = 0;
                        flipped = true;
                    }
                }
            }
        }
        // PCIe 停顿独立告警：利用率超阈**且**队列深（双条件叠加）。
        if depth >= QUEUE_DEPTH_ALERT && util_permille >= PCIE_UTIL_ALERT_PERMILLE {
            bag.push(E_BW_PCIE_STALL, "PCIe 停顿", &format!("深{} 利用{}‰", depth, util_permille));
        }
        let hint = PlacementHint {
            keep_on_cpu: self.state == Bottleneck::PcieSat,
            cause: self.state,
            depth,
            util_permille,
        };
        (self.state, flipped, hint)
    }

    pub fn spoken(&self) -> String {
        format!(
            "拥塞[{}]标定{}解除{}",
            match self.state {
                Bottleneck::None => "无",
                Bottleneck::PcieSat => "PCIe饱和",
            },
            self.on_events,
            self.off_events
        )
    }
}

// ---------------------------------------------------------------------------
// 四元画像（遥测全量——vez10/cga 系同口径）
// ---------------------------------------------------------------------------

/// 四元画像聚合器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuadAgg {
    pub count: u64,
    pub total: u64,
    pub max: u64,
    pub min: u64,
}

impl QuadAgg {
    pub fn new() -> QuadAgg {
        QuadAgg { count: 0, total: 0, max: 0, min: u64::MAX }
    }

    pub fn add(&mut self, v: u64) {
        self.count += 1;
        self.total += v;
        if v > self.max {
            self.max = v;
        }
        if v < self.min {
            self.min = v;
        }
    }

    pub fn mean(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.total / self.count
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ---------------------------------------------------------------------------
// 调度器总装（双队列+帧步进+时间片零贡献+遥测全量）
// ---------------------------------------------------------------------------

/// 带宽利用率遥测账本（全量——字节账/切片画像/分片账/告警账）。
#[derive(Debug)]
pub struct BwTelemetry {
    /// 上传字节账。
    pub up_bytes: u64,
    /// 下载字节账。
    pub down_bytes: u64,
    /// 上传切片耗时画像（count/total/max/min）。
    pub up_slice: QuadAgg,
    /// 下载切片耗时画像。
    pub down_slice: QuadAgg,
    /// 分片发生次数。
    pub chunk_events: u32,
    /// 时间片让路次数（片内任务没传完被停——零贡献机制执行次数）。
    pub yield_events: u32,
    /// 队列拒绝次数。
    pub reject_count: u32,
    /// 帧序号账。
    pub frames: u64,
}

impl BwTelemetry {
    pub fn new() -> BwTelemetry {
        BwTelemetry {
            up_bytes: 0,
            down_bytes: 0,
            up_slice: QuadAgg::new(),
            down_slice: QuadAgg::new(),
            chunk_events: 0,
            yield_events: 0,
            reject_count: 0,
            frames: 0,
        }
    }

    /// PCIe 带宽利用率（‰）= 累计上传+下载字节 / 单帧理论总容量
    /// （上传+下载）×1000——累计口径，随帧单调上升。
    pub fn util_permille(&self) -> u64 {
        (self.up_bytes + self.down_bytes) * 1000
            / (FRAME_UPLOAD_BYTES + DOWN_SLICE_US * COPY_RATE_BYTES_PER_US)
    }

    pub fn spoken(&self) -> String {
        format!(
            "遥测 上{}B 下{}B 帧片上{}让{}拒{}",
            self.up_bytes, self.down_bytes, self.up_slice.count, self.yield_events, self.reject_count
        )
    }
}

/// 上传下载带宽调度器。
#[derive(Debug)]
pub struct BandwidthSched {
    /// 上传队列（FIFO——先来先传）。
    pub up_q: Vec<UploadJob>,
    /// 下载队列（低优先级——上传后消费）。
    pub down_q: Vec<DownloadJob>,
    /// 拥塞探针。
    pub probe: CongestionProbe,
    /// 遥测账本。
    pub tel: BwTelemetry,
    /// 立案簿。
    pub bag: DiagBag,
    /// 最近一次放置建议。
    pub last_hint: Option<PlacementHint>,
    /// job id 发号。
    next_id: u32,
}

impl BandwidthSched {
    pub fn new() -> BandwidthSched {
        BandwidthSched {
            up_q: Vec::new(),
            down_q: Vec::new(),
            probe: CongestionProbe::new(),
            tel: BwTelemetry::new(),
            bag: DiagBag::new(),
            last_hint: None,
            next_id: 1,
        }
    }

    /// 提交上传（大资产强制分片留痕；队列满拒绝计数）。
    /// 返回 Ok(job_id) 或 Err(码)。
    pub fn submit_upload(&mut self, kind: AssetKind, total: u64) -> Result<u32, u16> {
        if self.up_q.len() >= QUEUE_CAP {
            self.tel.reject_count += 1;
            self.bag.push(E_BW_QUEUE_FULL, "上传提交", &format!("类型{} 字节{}", kind.zh(), total));
            return Err(E_BW_QUEUE_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        let (job, big) = UploadJob::new(id, kind, total);
        if big {
            // 大资产强制分片——提交面即 enforce 并留痕（不拒绝，改约分片）。
            self.tel.chunk_events += 1;
            self.bag.push(
                E_BW_BIG_UNCHUNKED,
                &format!("job{}", id),
                &format!("{}字节>单帧容量{}——跨帧分片", total, FRAME_UPLOAD_BYTES),
            );
        }
        self.up_q.push(job);
        Ok(id)
    }

    /// 提交下载（低优先级通道——同样受限速对称约束）。
    pub fn submit_download(&mut self, kind: AssetKind, total: u64) -> Result<u32, u16> {
        if self.down_q.len() >= QUEUE_CAP {
            self.tel.reject_count += 1;
            self.bag.push(E_BW_QUEUE_FULL, "下载提交", &format!("类型{} 字节{}", kind.zh(), total));
            return Err(E_BW_QUEUE_FULL);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.down_q.push(DownloadJob::new(id, kind, total));
        Ok(id)
    }

    /// 上传队列当前深度。
    pub fn up_depth(&self) -> usize {
        self.up_q.len()
    }

    /// 帧步进：上传切片（≤COPY_SLICE_US）→ 下载切片（≤DOWN_SLICE_US，
    /// 上传之后消费——读回不抢帧带宽）→ 拥塞探针。
    /// **时间片耗尽零贡献**：片内没传完立即让路，绝不超时。
    pub fn step_frame(&mut self, pcie_util_permille: u64) -> PlacementHint {
        self.tel.frames += 1;
        // 1. 上传切片：预算 = 1500µs × 速率 = FRAME_UPLOAD_BYTES。
        let mut up_budget = FRAME_UPLOAD_BYTES;
        let mut i = 0;
        while i < self.up_q.len() && up_budget > 0 {
            let need = self.up_q[i].total - self.up_q[i].sent;
            let grant = if need <= up_budget { need } else { up_budget };
            // mipmap 渐进可见优先：先补 preview 级。
            let pv_left = self.up_q[i].preview_bytes - self.up_q[i].preview_sent;
            let pv_grant = if pv_left <= grant { pv_left } else { grant };
            self.up_q[i].preview_sent += pv_grant;
            self.up_q[i].sent += grant;
            self.tel.up_bytes += grant;
            up_budget -= grant;
            if self.up_q[i].done() {
                self.up_q.remove(i);
            } else {
                self.tel.yield_events += 1; // 片内未完——让路（零贡献执行）。
                i += 1;
            }
        }
        self.tel.up_slice.add(COPY_SLICE_US - up_budget / COPY_RATE_BYTES_PER_US);
        // 2. 下载切片：上传之后（低优先级——不抢帧带宽）。
        let mut dl_budget = DOWN_SLICE_US * COPY_RATE_BYTES_PER_US;
        let mut j = 0;
        while j < self.down_q.len() && dl_budget > 0 {
            let need = self.down_q[j].total - self.down_q[j].got;
            let grant = if need <= dl_budget { need } else { dl_budget };
            self.down_q[j].got += grant;
            self.tel.down_bytes += grant;
            dl_budget -= grant;
            if self.down_q[j].done() {
                self.down_q.remove(j);
            } else {
                j += 1;
            }
        }
        self.tel.down_slice.add(DOWN_SLICE_US - dl_budget / COPY_RATE_BYTES_PER_US);
        // 3. 拥塞探针（队列深度+PCIe 利用率联动——迟滞防抖）。
        let (st, _flip, hint) = self.probe.step(self.up_depth(), pcie_util_permille, &mut self.bag);
        let _ = st;
        self.last_hint = Some(hint);
        hint
    }

    /// 全量读屏摘要。
    pub fn spoken(&self) -> String {
        format!(
            "带宽调度 上传深度{}下载深度{}；{}；{}；{}",
            self.up_depth(),
            self.down_q.len(),
            self.tel.spoken(),
            self.probe.spoken(),
            self.bag.spoken()
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（十项——限速零贡献/单帧恰值/分片渐进/可见性优先/读回不抢帧/
// 拥塞标定/迟滞防抖/遥测全量/队列满拒绝/全链编排）
// ---------------------------------------------------------------------------

/// F0010 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_cga10_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0010");

    // 限-01+限-02 上传限速：64MB 资产逐帧传——每帧恰 24,576,000 字节
    // （1500µs×16KB/µs 手算锚），片内没传完即让路（yield 计数），渲染
    // 帧时间不受影响（帧时间账独立于上传——零贡献）。
    {
        let mut bw = BandwidthSched::new();
        let total: u64 = 64 * 1024 * 1024;
        let _id = bw.submit_upload(AssetKind::Geometry, total);
        let mut frames = 0;
        loop {
            let _h = bw.step_frame(500);
            frames += 1;
            if bw.up_q.is_empty() {
                break;
            }
            if frames > 10 {
                break; // 防死循环（判据护栏）。
            }
        }
        // 64MB = 67,108,864B；67,108,864 / 24,576,000 = 2.73 → 3 帧。
        let expect_frames = (total + FRAME_UPLOAD_BYTES - 1) / FRAME_UPLOAD_BYTES;
        let limit_ok = frames as u64 == expect_frames
            && expect_frames == 3
            && bw.tel.up_bytes == total
            && bw.tel.yield_events == 2 // 前 2 帧片内没传完——让路 2 次
            && bw.tel.up_slice.count == 3
            && bw.tel.up_slice.max == COPY_SLICE_US; // 每片顶满 1500µs
        if limit_ok {
            s.ok("F0010-限-01-上传限速帧超时零贡献");
        } else {
            s.fail("F0010-限-01-上传限速帧超时零贡献", "帧数或字节账与手算不符");
        }
    }

    // 片-01 大资产强制分片：32MB 4K 纹理提交即留痕 chunked+分片账。
    {
        let mut bw = BandwidthSched::new();
        let id = bw.submit_upload(AssetKind::Texture4K, BIG_ASSET_BYTES).unwrap_or(0);
        let chunk_ok = id != 0
            && bw.tel.chunk_events == 1
            && bw.bag.count_of(E_BW_BIG_UNCHUNKED) == 1
            && bw.up_q[0].chunked
            && BIG_ASSET_BYTES > FRAME_UPLOAD_BYTES; // 32MB>24MB 分片前提成立
        if chunk_ok {
            s.ok("F0010-片-01-大资产强制分片留痕");
        } else {
            s.fail("F0010-片-01-大资产强制分片留痕", "分片标记或留痕不符");
        }
    }

    // 片-02 mipmap 渐进可见性优先：preview 级（1/64 面积）传完即可见，
    // 可见发生远早于完成（可见帧 < 完成帧）。
    {
        let mut bw = BandwidthSched::new();
        let total: u64 = BIG_ASSET_BYTES; // 32MB
        let _id = bw.submit_upload(AssetKind::Texture4K, total).unwrap_or(0);
        let mut visible_at = 0;
        let mut done_at = 0;
        let mut f = 0;
        while f < 10 {
            f += 1;
            let _h = bw.step_frame(500);
            if done_at == 0 && !bw.up_q.is_empty() && bw.up_q[0].visible() && visible_at == 0 {
                visible_at = f;
            }
            if bw.up_q.is_empty() {
                if done_at == 0 {
                    done_at = f;
                }
                break;
            }
        }
        // preview = 32MB/64 = 512KB，第一帧上传容量 24MB ≫ 512KB——
        // 可见发生在帧 1；完成需 2 帧（24MB+8MB）。
        let mip_ok = visible_at == 1
            && done_at == 2
            && visible_at < done_at
            && MIP_PREVIEW_DEN == 64;
        if mip_ok {
            s.ok("F0010-片-02-mipmap渐进可见性优先");
        } else {
            s.fail("F0010-片-02-mipmap渐进可见性优先", "可见时刻或完成时刻不符");
        }
    }

    // 读-01 读回不抢帧带宽：同帧上传+读回——下载切片独立 750µs 且在
    // 上传之后消费；上传容量不被下载挤占（手算锚）。
    {
        let mut bw = BandwidthSched::new();
        let _u = bw.submit_upload(AssetKind::Geometry, FRAME_UPLOAD_BYTES).unwrap_or(0);
        let _d = bw.submit_download(AssetKind::Readback, 4 * 1024 * 1024).unwrap_or(0);
        let _h = bw.step_frame(500);
        // 上传恰好单帧容量——帧 1 内完成；下载 4MB 在同帧下载切片
        // （750µs×16KB=12MB 容量）内也完成——两账独立不互抢。
        let dl_ok = bw.up_q.is_empty()
            && bw.down_q.is_empty()
            && bw.tel.up_bytes == FRAME_UPLOAD_BYTES
            && bw.tel.down_bytes == 4 * 1024 * 1024
            && DOWN_SLICE_US * COPY_RATE_BYTES_PER_US == 12_288_000; // 750×16384 手算
        if dl_ok {
            s.ok("F0010-读-01-读回低优先不抢帧带宽");
        } else {
            s.fail("F0010-读-01-读回低优先不抢帧带宽", "下载切片或字节账不符");
        }
    }

    // 拥-01 拥塞标定联动放置：9 个 32MB 大 job 持续积压（小 job 一帧
    // 就被消费完不构成积压——拥塞的本质是积压），深 9+利用率 950‰
    // 连续 2 帧→PcieSat+keep_on_cpu=true（F0004 信号面）。
    {
        let mut bw = BandwidthSched::new();
        for i in 0..QUEUE_DEPTH_ALERT + 1 {
            let _ = bw.submit_upload(AssetKind::Texture4K, BIG_ASSET_BYTES);
            let _ = i;
        }
        let _h1 = bw.step_frame(950); // 紧 1（depth 9 未达迟滞）
        let st1 = bw.probe.state;
        let h2 = bw.step_frame(950); // 紧 2——标定
        let cong_ok = st1 == Bottleneck::None
            && h2.cause == Bottleneck::PcieSat
            && h2.keep_on_cpu
            && h2.depth == 8 // 帧 2 消费完 job1（33,554,432=24,576,000+8,978,432）剩 8
            && bw.bag.count_of(E_BW_CONGESTION) == 1
            && bw.bag.count_of(E_BW_PCIE_STALL) == 2; // 双条件叠加两帧各告警
        if cong_ok {
            s.ok("F0010-拥-01-拥塞标定联动放置建议");
        } else {
            s.fail("F0010-拥-01-拥塞标定联动放置建议", "标定时机或建议不符");
        }
    }

    // 拥-02 迟滞防抖：单帧尖峰（清队列即消）不标定；标定后连续 3 帧
    // 松才解除（32MB 大 job 制造真实积压——小 job 一帧消费完不构成）。
    {
        let mut bw = BandwidthSched::new();
        // 单帧尖峰：8 个 32MB 一次涌入，下一帧立即清空队列。
        for i in 0..QUEUE_DEPTH_ALERT {
            let _ = bw.submit_upload(AssetKind::Texture4K, BIG_ASSET_BYTES);
            let _ = i;
        }
        let _h = bw.step_frame(500); // tight_run=1（未达 2）
        bw.up_q.clear(); // 立即松
        let _h2 = bw.step_frame(500);
        let spike_ok = bw.probe.state == Bottleneck::None && bw.probe.on_events == 0;
        // 进入标定：9 个 32MB 持续积压+高利用率连续 2 帧。
        for i in 0..QUEUE_DEPTH_ALERT + 1 {
            let _ = bw.submit_upload(AssetKind::Texture4K, BIG_ASSET_BYTES);
            let _ = i;
        }
        let _a = bw.step_frame(950);
        let h_b = bw.step_frame(950); // 标定
        let cong_now = h_b.cause == Bottleneck::PcieSat;
        // 连续 3 帧松（清队列+低利用率）→解除。
        bw.up_q.clear();
        let _c1 = bw.step_frame(100);
        let _c2 = bw.step_frame(100);
        let h_c3 = bw.step_frame(100);
        let off_ok = cong_now
            && bw.probe.state == Bottleneck::None
            && h_c3.cause == Bottleneck::None
            && bw.probe.on_events == 1
            && bw.probe.off_events == 1;
        let hyst_ok = spike_ok && off_ok;
        if hyst_ok {
            s.ok("F0010-拥-02-迟滞防抖尖峰不标定");
        } else {
            s.fail("F0010-拥-02-迟滞防抖尖峰不标定", "迟滞时机或翻转账不符");
        }
    }

    // 遥-01 带宽利用率遥测全量：util 千分比恰值（手算锚）+画像账。
    {
        let mut bw = BandwidthSched::new();
        // 传一半容量：12,288,000B 上传 → util = 12,288,000/(24,576,000+12,288,000) ≈ 333‰。
        let _u = bw.submit_upload(AssetKind::Geometry, FRAME_UPLOAD_BYTES / 2).unwrap_or(0);
        let _h = bw.step_frame(500);
        let expect_up_half = FRAME_UPLOAD_BYTES / 2;
        let expect_util = expect_up_half * 1000 / (FRAME_UPLOAD_BYTES + DOWN_SLICE_US * COPY_RATE_BYTES_PER_US);
        let tel_ok = bw.tel.up_bytes == expect_up_half
            && bw.tel.util_permille() == expect_util
            && expect_util == 333 // 12,288,000×1000/36,864,000=333.33→333
            && bw.tel.up_slice.count == 1
            && bw.tel.frames == 1;
        if tel_ok {
            s.ok("F0010-遥-01-带宽利用率遥测全量恰值");
        } else {
            s.fail("F0010-遥-01-带宽利用率遥测全量恰值", "利用率或画像账不符");
        }
    }

    // 队-01 队列满拒绝：32 容量硬顶→拒绝+计数留痕。
    {
        let mut bw = BandwidthSched::new();
        let mut ok_ids = 0;
        let mut rejected = 0;
        for i in 0..(QUEUE_CAP + 4) {
            match bw.submit_upload(AssetKind::Geometry, 1024) {
                Ok(_) => ok_ids += 1,
                Err(code) => {
                    rejected += 1;
                    let _ = code;
                }
            }
            let _ = i;
        }
        let q_ok = ok_ids == QUEUE_CAP
            && rejected == 4
            && bw.tel.reject_count == 4
            && bw.bag.count_of(E_BW_QUEUE_FULL) == 4
            && bw.up_depth() == QUEUE_CAP;
        if q_ok {
            s.ok("F0010-队-01-队列满拒绝计数留痕");
        } else {
            s.fail("F0010-队-01-队列满拒绝计数留痕", "容量或拒绝账不符");
        }
    }

    // 坏-01 查无任务面：job 构造参数越界语义（total=0 的空任务进度恒满）。
    {
        let (j, big) = UploadJob::new(7, AssetKind::Geometry, 0);
        let bad_ok = j.done() && j.visible() && j.permille() == 1000 && !big;
        if bad_ok {
            s.ok("F0010-坏-01-空任务边界语义确定");
        } else {
            s.fail("F0010-坏-01-空任务边界语义确定", "空任务进度或可见不符");
        }
    }

    // 编-01 全链编排：32MB 纹理（分片 2 帧+可见优先帧 1）+ 读回并行
    // + 拥塞探针喂数 + 放置建议快照 + 遥测对账 + 读屏摘要非空。
    {
        let mut bw = BandwidthSched::new();
        let _u = bw.submit_upload(AssetKind::Texture4K, BIG_ASSET_BYTES).unwrap_or(0);
        let _d = bw.submit_download(AssetKind::Screenshot, 8 * 1024 * 1024).unwrap_or(0);
        let h1 = bw.step_frame(500);
        let vis_frame1 = h1.depth > 0 && bw.last_hint.is_some();
        // 帧探针：上传队列仍有 1 个 job（未完成）——深度 1。
        let h2 = bw.step_frame(990); // 高利用率但不深——不触发双条件停顿
        let f3 = bw.step_frame(500);
        // 手算：单 job 33,554,432 分 2 帧传完（24,576,000+8,978,432）+
        // 下载 8,388,608 → 账面利用率 41,943,040×1000/36,864,000=1137‰；
        // h2.util_permille 是观测回显（调用方传入 990）。
        let chain_ok = vis_frame1
            && h2.util_permille == 990
            && bw.tel.util_permille() == 1137
            && f3.depth == 0 // 帧 2 已完成 job（帧 3 空转）
            && bw.tel.up_bytes == BIG_ASSET_BYTES
            && bw.tel.chunk_events == 1
            && bw.tel.down_bytes == 8 * 1024 * 1024
            && bw.tel.up_slice.count == 3 // 三帧各计一次（第 3 帧空转 0µs）
            && bw.bag.count_of(E_BW_BIG_UNCHUNKED) == 1
            && bw.spoken().contains("带宽调度");
        if chain_ok {
            s.ok("F0010-编-01-分片可见读回拥塞全链对账");
        } else {
            s.fail("F0010-编-01-分片可见读回拥塞全链对账", "编排对账不符");
        }
    }

    s
}
