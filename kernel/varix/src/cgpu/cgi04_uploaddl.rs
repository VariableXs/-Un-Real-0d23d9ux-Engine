//! cgi04_uploaddl — CGPU-F0010 上传下载带宽调度器（CPU→GPU 拷贝的帧预算化）。
//!
//! 判据原文（施工书锚点 CGPU-F0010）：CPU→GPU 的纹理/几何上传是最容易被忽视
//! 的帧杀手。本功能实现带宽调度：上传队列与渲染并行但限速——按帧预算动态分
//! 配上传带宽（默认每帧 ≤1.5ms 的拷贝时间片），大资产（4K 纹理 32MB 级）强制
//! 分片跨帧渐进上传，配合 mipmap 渐进（先传低分辨率级保证可见，高级别后台补
//! 齐）。下载（截图/读回）走低优先级通道，与上传对称限速。拥塞归因：上传队
//! 列深度与 PCIe 利用率计数联动，带宽瓶颈自动标定并通知任务放置决策器
//! （CGPU-F0004）——带宽紧时把可 CPU 计算的任务留在 CPU。判据：上传限速帧超
//! 时零贡献、分片渐进上传可见性优先、读回不抢帧带宽、瓶颈标定联动放置决策、
//! 带宽利用率遥测全量。
//!
//! 职责定位：每帧拷贝时间片分配+大资产分片跨帧渐进+mipmap 可见性优先+下载对
//! 称限速低优先级+拥塞归因标定联动放置决策。链路口径声明式复用 cgi02_pciebw
//! （F1282）与 cgi03_bwaccount（F1283 账户层）——本模块是拷贝时间片调度层，
//! 不重定义带宽物理量。诊断码段 0x44xx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// 链路口径：声明式复用 cgi02_pciebw（F1282）。
use crate::cgpu::cgi02_pciebw::{
    effective_mbps, gen_theory_mbps, EFFICIENCY_DEFAULT_PPM, PcieGen,
};

/// 模块版本。
pub const UPLOAD_DL_VERSION: &str = "I04-uploaddl-v1";

/// 每帧上传拷贝时间片 1.5ms（判据原文数字）。
pub const UPLOAD_SLICE_US: u64 = 1_500;
/// 每帧下载时间片 0.5ms（对称限速且低优先级——读回不抢帧带宽）。
pub const DOWNLOAD_SLICE_US: u64 = 500;
/// 大资产分片阈值 8MB（≥ 此值强制分片跨帧——判据"32MB 级强制分片"的
/// 保守门槛：8MB 在 1.5ms 片内已不可整传）。
pub const BIG_ASSET_MB: u64 = 8;
/// 分片粒度 2MB（每片 ≤ 时间片容量的稳健粒度）。
pub const SLICE_MB: u64 = 2;
/// 队列深度预警线（片数）——拥塞归因输入之一。
pub const QUEUE_DEEP_WARN: usize = 64;
/// PCIe 利用率预警线（ppm）。
pub const PCIE_UTIL_WARN_PPM: u64 = 900_000;

/// mipmap 级序（0=最低分辨率最优先——判据"先传低分辨率级保证可见"）。
pub type MipLevel = u8;

/// 资产分片（渐进上传的最小发放单位）。
#[derive(Clone, Copy, Debug)]
pub struct AssetSlice {
    /// 资产 id。
    pub asset_id: u32,
    /// 片序（同资产内递增）。
    pub slice_idx: u16,
    /// 片大小（MB，≤ SLICE_MB）。
    pub mb: u64,
    /// mipmap 级（0 最先——可见性优先）。
    pub mip: MipLevel,
}

impl AssetSlice {
    /// 发放优先键：mip 升序优先（低级先传保证可见），同 mip 按片序。
    pub const fn order_key(&self) -> u64 {
        (self.mip as u64) << 48 | (self.slice_idx as u64) << 32 | self.asset_id as u64
    }
}

/// 诊断码（0x44xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4401 资产无有效分片（0MB 资产）。
    UdAssetEmpty,
    /// 0x4402 片非法（大小超 SLICE_MB 上界）。
    UdSliceInvalid,
    /// 0x4403 时间片超发（发放总量超帧预算——内部不变量，出现即立案）。
    UdBudgetOver,
    /// 0x4404 带宽瓶颈标定（联动放置决策的信号）。
    UdCongested,
    /// 0x4405 队列深度超限（拥塞归因之二）。
    UdQueueOverflow,
    /// 0x4406 链路口径解析失败（复用层透传）。
    UdLinkInvalid,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::UdAssetEmpty => "资产无有效分片：0MB 资产不是一次真实上传",
            XcCode::UdSliceInvalid => "分片非法：单片必须 ≤ 分片粒度上界",
            XcCode::UdBudgetOver => "拷贝时间片超发：发放总量超过帧预算，调度器不变量被破坏",
            XcCode::UdCongested => "带宽瓶颈标定：PCIe 利用率/队列深度超预警，通知放置决策器",
            XcCode::UdQueueOverflow => "上传队列深度超限：拥塞归因之二",
            XcCode::UdLinkInvalid => "链路口径解析失败：代际×lane 组合非法",
        }
    }
}

/// 拥塞归因结论（判据"瓶颈自动标定并通知任务放置决策器"的出口单）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CongestionKind {
    /// 无拥塞。
    None,
    /// 队列深（资产堆积——生产快于链路消化）。
    QueueDeep,
    /// 链路饱和（利用率超预警）。
    LinkSaturated,
    /// 双重瓶颈。
    Both,
}

impl CongestionKind {
    /// 放置决策建议（判据"带宽紧时把可 CPU 计算的任务留在 CPU"——F0004 消费）。
    pub const fn placement_advice(self) -> &'static str {
        match self {
            CongestionKind::None => "链路空闲：GPU 放置不受限",
            CongestionKind::QueueDeep => "队列堆积：削减上传生产速率，GPU 放置可维持",
            CongestionKind::LinkSaturated => "链路饱和：可 CPU 计算的任务留在 CPU（放置决策器 F0004 消费）",
            CongestionKind::Both => "双重瓶颈：立即降上传优先级并全部可 CPU 任务留 CPU",
        }
    }
}

/// 每帧遥测行（判据"带宽利用率遥测全量"）。
#[derive(Clone, Copy, Debug)]
pub struct TelemetryRow {
    /// 帧号。
    pub frame_no: u64,
    /// 上传发放（MB）。
    pub upload_mb: u64,
    /// 上传时间片用量（µs）。
    pub upload_us: u64,
    /// 下载发放（MB）。
    pub download_mb: u64,
    /// 下载时间片用量（µs）。
    pub download_us: u64,
    /// 拥塞结论。
    pub congestion: CongestionKind,
}

/// 上传下载调度器（判据本体）。
pub struct UploadDl {
    /// 每帧可传 MB（链路有效带宽 × 时间片占比——构造冻结）。
    upload_mb_per_frame: u64,
    /// 下载每帧可传 MB（独立时间片——不侵占上传）。
    download_mb_per_frame: u64,
    /// 上传等待队列（按 order_key 升序发放）。
    upload_q: Vec<AssetSlice>,
    /// 下载等待队列。
    download_q: Vec<AssetSlice>,
    /// 遥测全量账。
    pub telemetry: Vec<TelemetryRow>,
    /// 帧号发生器。
    frame_no: u64,
    /// 立案计数。
    pub cases: u32,
}

impl UploadDl {
    /// 从链路口径构造（复用 cgi02：有效带宽 × 时间片占比 = 每帧可传 MB）。
    pub fn from_link(gen: PcieGen, lanes: u32) -> Result<Self, XcCode> {
        let theory = gen_theory_mbps(gen, lanes).map_err(|_| XcCode::UdLinkInvalid)?;
        let eff = effective_mbps(theory, EFFICIENCY_DEFAULT_PPM);
        // MB/ms = Mbps/8000 → 每帧 MB = eff × slice_us / 1_000_000
        let up = eff.saturating_mul(UPLOAD_SLICE_US) / 1_000_000;
        let dl = eff.saturating_mul(DOWNLOAD_SLICE_US) / 1_000_000;
        if up == 0 || dl == 0 {
            return Err(XcCode::UdLinkInvalid); // 时间片太小连 1MB 都放不下的口径不合法
        }
        Ok(UploadDl {
            upload_mb_per_frame: up,
            download_mb_per_frame: dl,
            upload_q: Vec::new(),
            download_q: Vec::new(),
            telemetry: Vec::new(),
            frame_no: 0,
            cases: 0,
        })
    }

    /// 每帧上传预算（MB）。
    pub const fn upload_budget_mb(&self) -> u64 {
        self.upload_mb_per_frame
    }

    /// 入上传队列（大资产强制分片：total_mb > BIG_ASSET_MB 时按 SLICE_MB 切——
    /// 判据"32MB 级强制分片跨帧渐进上传"）。
    pub fn enqueue_upload(&mut self, asset_id: u32, total_mb: u64, top_mip: MipLevel) -> Result<usize, XcCode> {
        if total_mb == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::UdAssetEmpty);
        }
        // mipmap 渐进序：mip 0（最低分辨率）到 top_mip（高级别后台补齐）。
        // 每级容量按 4 倍递增（mip 几何：level n 容量 = base×4^n），
        // base 级取 max(1, total_mb >> (2×top_mip))——保证 Σ级容量 ≈ total_mb。
        let base_mb = (total_mb >> (2 * top_mip as u32)).max(1);
        let mut n = 0usize;
        let mut slice_seq = 0u16;
        for mip in 0..=top_mip {
            // 级容量：base × 4^mip，封顶 total
            let mut cap = base_mb;
            for _ in 0..mip {
                cap = cap.saturating_mul(4);
            }
            let cap = cap.min(total_mb);
            if cap == 0 {
                continue;
            }
            // 级内分片
            let mut left = cap;
            while left > 0 {
                let mb = left.min(SLICE_MB);
                self.upload_q.push(AssetSlice {
                    asset_id,
                    slice_idx: slice_seq,
                    mb,
                    mip,
                });
                slice_seq = slice_seq.wrapping_add(1);
                left -= mb;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 入下载队列（读回请求——低优先级对称限速）。
    pub fn enqueue_download(&mut self, asset_id: u32, total_mb: u64) -> Result<usize, XcCode> {
        if total_mb == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::UdAssetEmpty);
        }
        let mut n = 0usize;
        let mut left = total_mb;
        let mut seq = 0u16;
        while left > 0 {
            let mb = left.min(SLICE_MB);
            self.download_q.push(AssetSlice { asset_id, slice_idx: seq, mb, mip: 0 });
            seq = seq.wrapping_add(1);
            left -= mb;
            n += 1;
        }
        Ok(n)
    }

    /// 队列深度（拥塞归因输入）。
    pub fn queue_depth(&self) -> usize {
        self.upload_q.len()
    }

    /// 帧推进（判据主路径：按 order_key 升序发放上传片直至时间片尽；
    /// 下载独立片发放——读回不抢帧带宽；随后拥塞归因标定）。
    pub fn tick(&mut self) -> Result<TelemetryRow, XcCode> {
        self.frame_no = self.frame_no.saturating_add(1);
        // 上传发放
        self.upload_q.sort_by_key(|s| s.order_key());
        let mut up_mb = 0u64;
        let mut up_us = 0u64;
        while !self.upload_q.is_empty() {
            let s = self.upload_q[0];
            let cost_us = s.mb.saturating_mul(1_000_000)
                / (self.upload_mb_per_frame.saturating_mul(1_000_000) / UPLOAD_SLICE_US).max(1);
            if up_us + cost_us > UPLOAD_SLICE_US {
                break; // 时间片尽——剩余片留下一帧（跨帧渐进）
            }
            self.upload_q.remove(0);
            up_mb += s.mb;
            up_us += cost_us;
        }
        if up_us > UPLOAD_SLICE_US {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::UdBudgetOver); // 不变量：永不超发
        }
        // 下载发放（独立时间片——与上传对称限速且不侵占）
        let mut dl_mb = 0u64;
        let mut dl_us = 0u64;
        while !self.download_q.is_empty() {
            let s = self.download_q[0];
            let cost_us = s.mb.saturating_mul(1_000_000)
                / (self.download_mb_per_frame.saturating_mul(1_000_000) / DOWNLOAD_SLICE_US).max(1);
            if dl_us + cost_us > DOWNLOAD_SLICE_US {
                break;
            }
            self.download_q.remove(0);
            dl_mb += s.mb;
            dl_us += cost_us;
        }
        if dl_us > DOWNLOAD_SLICE_US {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::UdBudgetOver);
        }
        // 拥塞归因（判据"队列深度与 PCIe 利用率计数联动"——利用率由
        // 调用方注入：此处以队列深度 + 时间片占满率代理，利用率入参见
        // note_congestion；tick 内先出队列深度维度结论）。
        let cong = if self.upload_q.len() >= QUEUE_DEEP_WARN {
            CongestionKind::QueueDeep
        } else {
            CongestionKind::None
        };
        let row = TelemetryRow {
            frame_no: self.frame_no,
            upload_mb: up_mb,
            upload_us: up_us,
            download_mb: dl_mb,
            download_us: dl_us,
            congestion: cong,
        };
        self.telemetry.push(row);
        Ok(row)
    }

    /// 利用率注入式拥塞归因（判据"上传队列深度与 PCIe 利用率计数联动"：
    /// 调用方（链路计数器）按帧注入利用率 ppm——两源合一出结论）。
    pub fn note_congestion(&mut self, pcie_util_ppm: u64) -> CongestionKind {
        let deep = self.upload_q.len() >= QUEUE_DEEP_WARN;
        let sat = pcie_util_ppm >= PCIE_UTIL_WARN_PPM;
        let kind = match (deep, sat) {
            (true, true) => CongestionKind::Both,
            (true, false) => CongestionKind::QueueDeep,
            (false, true) => CongestionKind::LinkSaturated,
            (false, false) => CongestionKind::None,
        };
        if kind != CongestionKind::None {
            self.cases = self.cases.saturating_add(1);
        }
        kind
    }

    /// 遥测全量账只读。
    pub fn telemetry(&self) -> &[TelemetryRow] {
        &self.telemetry
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "上传下载{} 帧预算上传{}MB/片{}µs 下载{}MB/片{}µs 上传队列{} 下载队列{} 立案{}",
            UPLOAD_DL_VERSION,
            self.upload_mb_per_frame,
            UPLOAD_SLICE_US,
            self.download_mb_per_frame,
            DOWNLOAD_SLICE_US,
            self.upload_q.len(),
            self.download_q.len(),
            self.cases,
        )
    }
}

/// 焦点资产优先重排（深化：上传队列发放序 = mipmap 渐进序 × 焦点加权——
/// 焦点资产（当前视口主资产）在同 mip 级内优先发放）。
pub struct FocusBoost;

impl FocusBoost {
    /// 加权键：非焦点键 + 1<<56 抬升（同 mip 下焦点优先；跨 mip 仍按
    /// mip 序——可见性优先纪律不被焦点打破）。
    pub const fn weighted_key(base_key: u64, focus: bool) -> u64 {
        if focus {
            base_key | (1u64 << 56)
        } else {
            base_key
        }
    }
}

/// 帧超时联动的预算收缩（深化：帧超时史 → 上传时间片收缩——帧预算
/// 红线优先于上传吞吐：超时帧率超 5% 时片缩 1/2，回落 1% 以下恢复）。
#[derive(Clone, Copy, Debug)]
pub struct SliceGovernor {
    /// 滚动超时帧计数。
    overtime: u32,
    /// 滚动总帧计数。
    total: u32,
    /// 当前收缩态（true=已收缩至半片）。
    pub shrunk: bool,
}

impl SliceGovernor {
    pub const fn new() -> Self {
        SliceGovernor { overtime: 0, total: 0, shrunk: false }
    }

    /// 帧结果回填（迟滞：超 5% 即缩；回落 ≤1% 且样本足才恢复）。
    pub fn on_frame(&mut self, frame_overtime: bool) {
        self.total = self.total.saturating_add(1);
        if frame_overtime {
            self.overtime = self.overtime.saturating_add(1);
        }
        let ppm = self.overtime_ppm();
        if ppm >= 50_000 {
            self.shrunk = true;
        } else if ppm <= 10_000 && self.total >= 100 {
            self.shrunk = false;
        }
    }

    /// 生效时间片（µs）。
    pub const fn effective_slice_us(&self) -> u64 {
        if self.shrunk {
            UPLOAD_SLICE_US / 2
        } else {
            UPLOAD_SLICE_US
        }
    }

    /// 滚动超时率（ppm）。
    pub const fn overtime_ppm(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            (self.overtime as u64) * 1_000_000 / self.total as u64
        }
    }
}

/// 资产取消回执（深化：未发放片的撤销——已发放的片不可撤（硬件在途），
/// 只撤队列中剩余片；撤销计数留痕）。
#[derive(Clone, Copy, Debug)]
pub struct CancelReport {
    /// 撤销片数。
    pub cancelled: u32,
    /// 在途不可撤片数（由执行层回填，本层标记零）。
    pub inflight: u32,
}

impl UploadDl {
    /// 撤销某资产的未发放片（判据纪律：在途片不可撤——只清队列）。
    pub fn cancel_asset(&mut self, asset_id: u32) -> CancelReport {
        let before = self.upload_q.len();
        self.upload_q.retain(|s| s.asset_id != asset_id);
        self.download_q.retain(|s| s.asset_id != asset_id);
        CancelReport {
            cancelled: (before - self.upload_q.len()) as u32,
            inflight: 0,
        }
    }
}
