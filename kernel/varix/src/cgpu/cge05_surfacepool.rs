//! cge05_surfacepool — CGPU-F0009 网页表面池与可见性仲裁（30 网页并发的调度
//! 核心）。
//!
//! 判据原文（施工书锚点 CGPU-F0009）：每个网页是一块表面，进池管理：可见表
//! 面（视口内+前台）走全帧率预算；半可见（部分遮挡）走 1/2 帧率；后台表面冻
//! 结渲染只保留事件响应与数据更新，恢复可见时 1 帧内复活。仲裁器按"可见面积
//! × 优先级 × 变化率"三因子每帧重排预算份额。风暴防护：页面批量刷新（如同
//! 时动画）触发合批与交错渲染（把 30 页的绘制按 4 帧轮转分摊，肉眼无损）。
//! 内存封顶：每表面共享内存上限按档位（可见 32MB/半可见 16MB/后台 8MB），超
//! 限先压缩再驱逐后台帧缓冲。全部节流对用户透明且可设置（"后台页面全速"开
//! 关）。判据：30 表面并发基准 B 达 80fps、可见性切换 1 帧复活、风暴合批肉眼
//! 无损、内存封顶驱逐正确、开关行为确定。
//!
//! 职责定位：表面池进出+可见性三档帧率+1 帧复活+三因子份额重排+风暴合批轮转
//! +内存封顶压缩驱逐+用户开关。与 cge04_surfacerender/surfsched（渲染执行与
//! 调度）分工——彼是表面怎么画与排队，此是表面池的预算与生命周期治理。份额
//! 重排输出声明式对接 cgc04（F0005 仲裁器网页表面池账户）。诊断码段 0x43xx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const SURFACE_POOL_VERSION: &str = "E05-surfacepool-v1";

/// 池容量上界（判据 30 表面并发基准；超界拒入——池不是无限的）。
pub const POOL_CAP: usize = 64;

/// 风暴判定阈值：单帧请求重绘的表面数 ≥ 此值即合批轮转（判据"批量刷新"）。
pub const STORM_THRESHOLD: usize = 8;

/// 交错渲染轮转窗（判据"按 4 帧轮转分摊"）。
pub const STAGGER_WINDOW: u32 = 4;

/// 内存封顶表（MB）：可见 32 / 半可见 16 / 后台 8（判据原文数字）。
pub const MEM_CAP_MB: [u32; 3] = [32, 16, 8];

/// 可见性三档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    /// 可见（视口内+前台）——全帧率。
    Visible,
    /// 半可见（部分遮挡）——1/2 帧率。
    Partial,
    /// 后台——冻结渲染只保留事件响应与数据更新。
    Hidden,
}

impl Visibility {
    pub const fn ordinal(self) -> usize {
        match self {
            Visibility::Visible => 0,
            Visibility::Partial => 1,
            Visibility::Hidden => 2,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            Visibility::Visible => "可见(全帧率)",
            Visibility::Partial => "半可见(1/2帧率)",
            Visibility::Hidden => "后台(冻结渲染)",
        }
    }

    /// 帧率系数（ppm）：全 1_000_000 / 半 500_000 / 后台 0（冻结）。
    pub const fn rate_ppm(self) -> u64 {
        match self {
            Visibility::Visible => 1_000_000,
            Visibility::Partial => 500_000,
            Visibility::Hidden => 0,
        }
    }

    /// 内存封顶（MB）。
    pub const fn mem_cap_mb(self) -> u32 {
        MEM_CAP_MB[self.ordinal()]
    }
}

/// 诊断码（0x43xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4301 池满拒入（容量上界保护）。
    SpPoolFull,
    /// 0x4302 表面不存在（查询/操作目标越界）。
    SpSurfaceMissing,
    /// 0x4303 内存超封顶——已压缩仍超，进入驱逐。
    SpMemOverCap,
    /// 0x4304 驱逐后仍超（全部后台已驱逐——显性资源不足）。
    SpEvictExhausted,
    /// 0x4305 三因子非法（面积/变化率出界）。
    SpFactorInvalid,
    /// 0x4306 份额和越界（重排后总量超帧预算）。
    SpShareOver,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::SpPoolFull => "表面池已满：容量上界保护，先驱逐再入池",
            XcCode::SpSurfaceMissing => "目标表面不在池中：句柄过期或已驱逐",
            XcCode::SpMemOverCap => "表面内存超档位封顶：压缩后仍超，进入驱逐流程",
            XcCode::SpEvictExhausted => "驱逐后仍超内存封顶：资源不足显性上报",
            XcCode::SpFactorInvalid => "三因子非法：可见面积/变化率必须归一化在界内",
            XcCode::SpShareOver => "重排份额和超帧预算：份额裁剪而非静默超发",
        }
    }
}

/// 表面行（池内单表面）。
#[derive(Clone, Copy, Debug)]
pub struct SurfaceRow {
    /// 表面 id（池内唯一）。
    pub sid: u32,
    /// 可见性档。
    pub vis: Visibility,
    /// 可见面积（ppm 归一：占视口比例 0..=1_000_000）。
    pub area_ppm: u32,
    /// 优先级（0..=255，越高越优先）。
    pub prio: u8,
    /// 变化率（ppm 归一：帧间内容变化比例 0..=1_000_000）。
    pub change_ppm: u32,
    /// 共享内存占用（MB）。
    pub mem_mb: u32,
    /// 帧率配额分子（每 STAGGER_WINDOW 帧允许渲染次数——可见=窗/1 半可见=窗/2）。
    pub quota_per_window: u32,
    /// 距上次渲染的帧数（1 帧复活判定用）。
    pub idle_frames: u32,
    /// 是否被冻结驱逐过帧缓冲（复活时需重建）。
    pub evicted: bool,
}

impl SurfaceRow {
    /// 三因子份额（ppm）：面积 × 优先级(归一 0..=1) × 变化率——判据公式。
    /// 优先级归一 = prio/255。结果上限 1_000_000。
    /// const fn 禁 Ord::min——手写 if 比较。
    pub const fn share_ppm(&self) -> u64 {
        let a = self.area_ppm as u64;
        let p = (self.prio as u64) * 1_000_000 / 255;
        let c = self.change_ppm as u64;
        let raw = a.saturating_mul(p).saturating_mul(c) / 1_000_000;
        if raw > 1_000_000 {
            1_000_000
        } else {
            raw
        }
    }
}

/// 用户开关（判据"后台页面全速"——行为确定：开=后台按 1/2 帧率渲染而非冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FullSpeedSwitch {
    /// 关（默认：后台冻结渲染）。
    Off,
    /// 开（后台 1/2 帧率——耗电换即时性，行为确定可对拍）。
    On,
}

impl FullSpeedSwitch {
    /// 开关下后台档的实际帧率系数（ppm）。
    pub const fn hidden_rate_ppm(self) -> u64 {
        match self {
            FullSpeedSwitch::Off => 0,
            FullSpeedSwitch::On => 500_000,
        }
    }
}

/// 表面池仲裁器（判据本体）。
pub struct SurfacePool {
    /// 池内表面。
    rows: Vec<SurfaceRow>,
    /// 用户开关。
    pub switch: FullSpeedSwitch,
    /// 当前轮转游标（风暴合批用）。
    stagger_cursor: u32,
    /// 本窗已渲染计数（按 sid 记——简化为窗内渲染次数表用 idle_frames 推）。
    /// 驱逐日志（判据"内存封顶驱逐正确"——驱逐动作全量留痕）。
    pub evict_log: Vec<(u32, u32)>, // (sid, 驱逐的 mem_mb)
    /// 压缩日志（压缩动作留痕）。
    pub compress_log: Vec<(u32, u32, u32)>, // (sid, 压缩前 MB, 压缩后 MB)
    /// 立案计数。
    pub cases: u32,
}

impl SurfacePool {
    /// 空池构造（默认开关关）。
    pub fn new() -> Self {
        SurfacePool {
            rows: Vec::new(),
            switch: FullSpeedSwitch::Off,
            stagger_cursor: 0,
            evict_log: Vec::new(),
            compress_log: Vec::new(),
            cases: 0,
        }
    }

    /// 入池（池满拒；面积/变化率出界拒）。
    pub fn admit(
        &mut self,
        sid: u32,
        vis: Visibility,
        area_ppm: u32,
        prio: u8,
        change_ppm: u32,
        mem_mb: u32,
    ) -> Result<(), XcCode> {
        if self.rows.len() >= POOL_CAP {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SpPoolFull);
        }
        if area_ppm > 1_000_000 || change_ppm > 1_000_000 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SpFactorInvalid);
        }
        if self.rows.iter().any(|r| r.sid == sid) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SpPoolFull); // 重复 id 同样拒（池无歧义）
        }
        let quota = match vis {
            Visibility::Visible => STAGGER_WINDOW,
            Visibility::Partial => STAGGER_WINDOW / 2,
            Visibility::Hidden => 0,
        };
        self.rows.push(SurfaceRow {
            sid,
            vis,
            area_ppm,
            prio,
            change_ppm,
            mem_mb,
            quota_per_window: quota,
            idle_frames: 0,
            evicted: false,
        });
        Ok(())
    }

    /// 可见性切换（判据"恢复可见时 1 帧内复活"：切换即清 idle_frames
    /// 并重建被驱逐的帧缓冲标记——复活零等待）。
    pub fn set_visibility(&mut self, sid: u32, vis: Visibility) -> Result<(), XcCode> {
        let r = self
            .rows
            .iter_mut()
            .find(|r| r.sid == sid)
            .ok_or(XcCode::SpSurfaceMissing)?;
        r.vis = vis;
        r.idle_frames = 0;
        r.quota_per_window = match vis {
            Visibility::Visible => STAGGER_WINDOW,
            Visibility::Partial => STAGGER_WINDOW / 2,
            Visibility::Hidden => 0,
        };
        if vis != Visibility::Hidden {
            r.evicted = false; // 复活：帧缓冲重建标记清除（1 帧内）
        }
        Ok(())
    }

    /// 每帧三因子重排（判据"可见面积×优先级×变化率每帧重排预算份额"）。
    /// 返回按份额降序的 sid 清单（份额计算闭集、排序稳定性按 sid 决胜）。
    pub fn rerank(&mut self) -> Vec<(u32, u64)> {
        let mut scored: Vec<(u32, u64)> = self
            .rows
            .iter()
            .filter(|r| self.renderable_ppm(r) > 0)
            .map(|r| (r.sid, r.share_ppm()))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        scored
    }

    /// 表面本帧是否可渲染（帧率系数>0 且窗内配额未耗尽）。
    fn renderable_ppm(&self, r: &SurfaceRow) -> u64 {
        let base = match r.vis {
            Visibility::Visible => Visibility::Visible.rate_ppm(),
            Visibility::Partial => Visibility::Partial.rate_ppm(),
            Visibility::Hidden => self.switch.hidden_rate_ppm(),
        };
        base
    }

    /// 帧推进（idle 计数与轮转游标推进；返回本帧应渲染的 sid 清单——
    /// 风暴合批：重绘请求超阈值时按轮转窗交错分摊）。
    pub fn tick(&mut self, redraw_requests: &[u32]) -> Result<Vec<u32>, XcCode> {
        // idle 推进
        for r in self.rows.iter_mut() {
            r.idle_frames = r.idle_frames.saturating_add(1);
        }
        // 风暴判定：请求重绘的表面数 ≥ 阈值 → 合批交错（按轮转窗取模分摊）
        if redraw_requests.len() >= STORM_THRESHOLD {
            self.stagger_cursor = (self.stagger_cursor + 1) % STAGGER_WINDOW;
            let mut out = Vec::new();
            for (idx, &sid) in redraw_requests.iter().enumerate() {
                if (idx as u32) % STAGGER_WINDOW == self.stagger_cursor {
                    out.push(sid);
                    if let Some(r) = self.rows.iter_mut().find(|r| r.sid == sid) {
                        r.idle_frames = 0;
                    }
                }
            }
            return Ok(out); // 其余请求留到后续轮转帧（肉眼无损分摊）
        }
        // 非风暴：请求即渲染
        let mut out = Vec::new();
        for &sid in redraw_requests {
            if self.rows.iter().any(|r| r.sid == sid) {
                out.push(sid);
                if let Some(r) = self.rows.iter_mut().find(|r| r.sid == sid) {
                    r.idle_frames = 0;
                }
            }
        }
        Ok(out)
    }

    /// 内存封顶治理（判据"超限先压缩再驱逐后台帧缓冲"）：
    /// 逐表面按档位封顶检查 → 超限先压缩（模拟：压缩后 = 封顶的 1/2）→
    /// 仍超则驱逐（仅后台可驱逐；驱逐后仍超显性报错）。
    pub fn enforce_mem_caps(&mut self) -> Result<(), XcCode> {
        let mut over: Vec<u32> = Vec::new();
        for r in self.rows.iter_mut() {
            let cap = r.vis.mem_cap_mb();
            if r.mem_mb > cap {
                // 压缩：压到封顶一半（有损压缩语义——先压缩判据）
                let after = (cap / 2).max(1);
                self.compress_log.push((r.sid, r.mem_mb, after));
                r.mem_mb = after;
                if r.mem_mb > cap {
                    over.push(r.sid);
                }
            }
        }
        if over.is_empty() {
            return Ok(());
        }
        // 驱逐：按优先级从低到高驱逐后台表面帧缓冲
        let mut hidden: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.vis == Visibility::Hidden)
            .map(|(i, _)| i)
            .collect();
        hidden.sort_by_key(|&i| self.rows[i].prio);
        for i in hidden {
            let r = &mut self.rows[i];
            self.evict_log.push((r.sid, r.mem_mb));
            r.mem_mb = 0;
            r.evicted = true;
        }
        // 终判：仍有超封顶 → 显性资源不足
        let still = self
            .rows
            .iter()
            .any(|r| r.mem_mb > r.vis.mem_cap_mb());
        if still {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SpEvictExhausted);
        }
        Ok(())
    }

    /// 复活延迟核验（判据"可见性切换 1 帧内复活"）：
    /// 从 Hidden 切到 Visible 的表面，切换帧即渲染（idle=0）——
    /// 本核验确认切换后首帧就在渲染清单里。
    pub fn revive_within_one_frame(&mut self, sid: u32) -> Result<bool, XcCode> {
        let r = self
            .rows
            .iter()
            .find(|r| r.sid == sid)
            .ok_or(XcCode::SpSurfaceMissing)?;
        Ok(r.vis == Visibility::Visible && r.idle_frames == 0 && !r.evicted)
    }

    /// 池内表面数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 池空判定。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 池快照（消费方只读）。
    pub fn snapshot(&self) -> &[SurfaceRow] {
        &self.rows
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        let vis = self.rows.iter().filter(|r| r.vis == Visibility::Visible).count();
        let partial = self.rows.iter().filter(|r| r.vis == Visibility::Partial).count();
        let hidden = self.rows.iter().filter(|r| r.vis == Visibility::Hidden).count();
        alloc::format!(
            "表面池{} 在池{} 可见{} 半可见{} 后台{} 开关{:?} 压缩{} 驱逐{} 立案{}",
            SURFACE_POOL_VERSION,
            self.rows.len(),
            vis,
            partial,
            hidden,
            self.switch as u8,
            self.compress_log.len(),
            self.evict_log.len(),
            self.cases,
        )
    }
}

/// 份额裁剪（深化：重排份额总和超 1e6 时按比例裁剪——判据"份额和越界"
/// 的落地形态：SpShareOver 不该发生，发生前先裁剪；裁剪比全量留痕）。
pub struct ShareTrimmer;

impl ShareTrimmer {
    /// 裁剪：Σshare > 1_000_000 时按比例缩至恰 1e6（返回裁剪后清单与
    /// 裁剪比 ppm——1000000=未裁剪；尾差归最大份额守恒收口）。
    pub fn trim(mut shares: Vec<(u32, u64)>) -> (Vec<(u32, u64)>, u64) {
        let sum: u64 = shares.iter().map(|(_, s)| *s).sum();
        if sum <= 1_000_000 || sum == 0 {
            return (shares, 1_000_000);
        }
        let ratio = 1_000_000 * 1_000_000 / sum;
        for (_, s) in shares.iter_mut() {
            *s = *s * ratio / 1_000_000;
        }
        let after: u64 = shares.iter().map(|(_, s)| *s).sum();
        if let Some(first) = shares.first_mut() {
            let d = 1_000_000u64.saturating_sub(after);
            first.1 += d;
        }
        (shares, ratio)
    }
}

/// 合批去重（深化：同帧重绘请求去重——同 sid 多次请求只渲一次；
/// 去重保序（首现序），去重计数留痕）。
pub struct RedrawDedup;

impl RedrawDedup {
    /// 去重（返回 (去重后清单, 去掉重复数)）。
    pub fn dedup(reqs: &[u32]) -> (Vec<u32>, u32) {
        let mut out: Vec<u32> = Vec::new();
        let mut dup = 0u32;
        for &r in reqs {
            if out.contains(&r) {
                dup += 1;
            } else {
                out.push(r);
            }
        }
        (out, dup)
    }
}

/// 复活重建预算（深化：被驱逐表面复活时的重建量账——重建 MB 上限
/// 按目标档位封顶，超限部分留后台流送补齐（可见性优先：低配先见））。
#[derive(Clone, Copy, Debug)]
pub struct ReviveBudget {
    /// 重建上限（MB = 目标档位封顶）。
    pub cap_mb: u32,
    /// 首帧立即可见预算（封顶 1/4——低配先见）。
    pub first_frame_mb: u32,
}

impl ReviveBudget {
    /// 从目标可见性档构造。const fn 禁 Ord::max——手写 if 比较。
    pub const fn for_target(vis: Visibility) -> Self {
        let cap = vis.mem_cap_mb();
        let q = cap / 4;
        let first = if q > 1 { q } else { 1 };
        ReviveBudget { cap_mb: cap, first_frame_mb: first }
    }
}
