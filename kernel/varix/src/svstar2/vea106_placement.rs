//! VE-F0106 · 多适配器资源放置策略（VE-A 域 · A05 多适配器组 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0106`
//!
//! **职责定位（锚点原文）**：多适配器资源放置策略——多 GPU 系统的资源放置
//! 策略总纲（集显/独显/外接卡的资源放置决策），**放置三因子**
//! （带宽需求 / 延迟敏感 / 功耗预算），**放置决策可解释**
//! （为什么放这张卡说得清）。多卡不是简单的谁强用谁——是资源的户口分配；
//! 含放置策略的**热图**（资源分布可视化）。
//!
//! **数据结构（锚点原文）**：放置策略。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 放置失配 → 重放置（给出迁移建议与迁移代价，不擅自搬）；
//! - 因子缺失 → 默认（缺功耗预算 = 不约束功耗；缺带宽需求 = 只按容量放）；
//! - 决策无据 → 登记（任何决策都必须带结构化理由位，空理由不可构造）。
//!
//! **性能逐项分解（锚点原文）**：O(资源 × 适配器)。适配器数 ≤ 8，
//! 资源放置是每次创建一次，常数级。
//!
//! **跨批对接点（锚点原文）**：A05 显存预算联动——放置前先问预算册的余量
//! （`Adapter::vram_free_kb` 由 A05 维护），本模块不自行扣账。
//!
//! **无障碍与隐私（锚点原文）**：放置记录读屏可达——`explain()` 输出
//! 纯文本「资源→卡，理由三条」。
//!
//! **判据（锚点原文）**：三因子决策、可解释、重放置、默认兜底。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数评分；确定性。
//!
//! 诊断段：0xE700 段 **0xE75x 子块**（0xE751~0xE75A 十码）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PM_VERSION: &str = "vea106_placement-v1";

/// 诊断段（子块）基址。
pub const PM_SEG_BASE: u16 = 0xE750;

/// 诊断码闭集（0xE751~0xE75A）。
pub const XC_ADAPTER_BAD: u32 = 0xE751;
pub const XC_SIZE_ZERO: u32 = 0xE752;
pub const XC_ADAPTER_FULL: u32 = 0xE753;
pub const XC_NO_FIT: u32 = 0xE754;
pub const XC_NO_REASON: u32 = 0xE755;
pub const XC_PEER_MISS: u32 = 0xE756;
pub const XC_DUPLICATE_ID: u32 = 0xE757;
pub const XC_MOVE_NOT_BETTER: u32 = 0xE758;
pub const XC_LEDGER_BROKEN: u32 = 0xE759;
pub const XC_HEATMAP_FULL: u32 = 0xE75A;

/// 适配器上限（工程红线：单机 GPU 数 ≤ 8；超过走机间调度，不在本模块）。
pub const MAX_ADAPTERS: usize = 8;

/// 放置余量（千分比）：必须剩 5% 空闲，避免把卡塞满导致后续碎片化。
pub const HEADROOM_PERMILLE: u32 = 50;

/// 重放置阈值（千分比）：新方案评分须高出 10% 才建议搬（搬有代价，别抖）。
pub const MOVE_MARGIN_PERMILLE: u32 = 100;

/// 适配器描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub id: u8,
    pub vram_total_kb: u32,
    pub vram_free_kb: u32,
    /// 显存带宽（GB/s）。
    pub bandwidth_gbps: u16,
    /// 到 CPU 的 NUMA 距离（0 = 本地）。
    pub numa_distance: u8,
    /// 独显（true）/ 集显（false）。
    pub discrete: bool,
    /// 功耗余量（mW；0 = 未知）。
    pub power_headroom_mw: u32,
    /// P2P 可达位图（bit i = 可与适配器 i 直接 P2P）。
    pub p2p_mask: u8,
}

impl Adapter {
    /// 可用容量：扣掉 5% 余量后的可分配上限。
    pub fn usable_free_kb(&self) -> u32 {
        let keep = self.vram_total_kb / 1000 * HEADROOM_PERMILLE;
        let usable_cap = self.vram_total_kb.saturating_sub(keep);
        if self.vram_free_kb > usable_cap {
            usable_cap
        } else {
            self.vram_free_kb
        }
    }

    pub fn has_p2p(&self, other: u8) -> bool {
        other < 8 && (self.p2p_mask & (1u8 << other)) != 0
    }
}

/// 资源放置请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceReq {
    pub res_id: u32,
    pub size_kb: u32,
    /// 带宽需求（GB/s；0 = 未声明，不约束）。
    pub min_bandwidth_gbps: u16,
    /// 延迟敏感（输入采样、VR 透视等）。
    pub latency_sensitive: bool,
    /// 功耗预算（mW；0 = 未声明，不约束）。
    pub power_budget_mw: u32,
    /// 需要与 `peer_mask` 里的适配器 P2P。
    pub peer_mask: u8,
    /// 是否允许放到系统内存（本模块只做决策记录，实际回退由 A05 执行）。
    pub sysmem_fallback_ok: bool,
}

/// 结构化理由位（可解释性的最小形式：每一位对应一条因子结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reasons(pub u8);

impl Reasons {
    pub const CAPACITY: u8 = 1 << 0;
    pub const BANDWIDTH: u8 = 1 << 1;
    pub const LATENCY: u8 = 1 << 2;
    pub const POWER: u8 = 1 << 3;
    pub const P2P: u8 = 1 << 4;
    pub const DEFAULTED: u8 = 1 << 5;

    pub const fn empty() -> Reasons {
        Reasons(0)
    }

    pub const fn raw(self) -> u8 {
        self.0
    }

    pub fn has(self, bit: u8) -> bool {
        (self.0 & bit) != 0
    }

    pub fn count(self) -> u32 {
        let mut n = 0;
        let mut b = self.0;
        while b != 0 {
            if b & 1 != 0 {
                n += 1;
            }
            b >>= 1;
        }
        n
    }
}

/// 一个放置决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub res_id: u32,
    pub adapter: u8,
    pub score: i64,
    pub reasons: Reasons,
    /// 是否走了默认兜底（因子缺失）。
    pub defaulted: bool,
    /// 是否延迟不满足但容量满足（显性降级，不是静默）。
    pub latency_degraded: bool,
}

/// 放置结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Placed(Placement),
    /// 无卡可放：给出缺口与最接近的一张卡（可执行建议的来源）。
    Denied { shortfall_kb: u32, best_id: u8, code: u32 },
    Rejected { code: u32 },
}

/// 迁移建议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveAdvice {
    pub res_id: u32,
    pub from: u8,
    pub to: u8,
    /// 评分提升（新 - 旧）。
    pub gain: i64,
    /// 迁移代价（µs，按 带宽 估算；0 = 未知带宽不算代价）。
    pub cost_us: u32,
}

/// 放置器：适配器表 + 决策账 + 热图。
pub struct Placer {
    adapters: Vec<Adapter>,
    /// 决策账（append-only，定容）。
    log: Vec<Placement>,
    log_dropped: u32,
    /// 每卡已放资源数与字节数（热图数据）。
    heat_count: [u32; MAX_ADAPTERS],
    heat_kb: [u32; MAX_ADAPTERS],
    /// 否决次数。
    denied: u32,
    /// 降级（延迟不达标）次数。
    degraded: u32,
    /// 最近诊断码。
    last_code: u32,
}

impl Placer {
    pub fn new() -> Placer {
        Placer {
            adapters: Vec::new(),
            log: Vec::new(),
            log_dropped: 0,
            heat_count: [0; MAX_ADAPTERS],
            heat_kb: [0; MAX_ADAPTERS],
            denied: 0,
            degraded: 0,
            last_code: 0,
        }
    }

    pub fn add_adapter(&mut self, a: Adapter) -> bool {
        if self.adapters.len() >= MAX_ADAPTERS {
            self.last_code = XC_ADAPTER_FULL;
            return false;
        }
        if a.id as usize >= MAX_ADAPTERS || a.vram_total_kb == 0 {
            self.last_code = XC_ADAPTER_BAD;
            return false;
        }
        if self.adapters.iter().any(|x| x.id == a.id) {
            self.last_code = XC_DUPLICATE_ID;
            return false;
        }
        self.adapters.push(a);
        true
    }

    pub fn adapters(&self) -> &[Adapter] {
        &self.adapters
    }

    pub fn counters(&self) -> (u32, u32, usize, u32) {
        (self.denied, self.degraded, self.log.len(), self.log_dropped)
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 三因子评分（整数，越大越好）。
    ///
    /// 评分构造（可解释性的量化基础）：
    ///   score = 带宽×100 − NUMA距离×10 + 独显加成 200 + 容量富余率(‰)
    /// 三项系数的含义：带宽是主因子（1GB/s ≈ 100 分，跨代差距可分辨）；
    /// NUMA 距离是延迟惩罚（每跳 −10，跨路 CPU 明显吃亏）；独显加成反映
    /// 「同带宽下独显更稳」（不受系统内存争用）。容量富余率作末位打破平局，
    /// 让「同样强的两张卡」优先放更空的那张。
    fn score(a: &Adapter, req: &PlaceReq) -> i64 {
        let mut s: i64 = a.bandwidth_gbps as i64 * 100;
        s -= a.numa_distance as i64 * 10;
        if a.discrete {
            s += 200;
        }
        let free = a.vram_free_kb.saturating_sub(req.size_kb);
        // 富余率（‰），上限 1000 防止超大空闲卡碾压带宽因子。
        let ratio = if a.vram_total_kb == 0 {
            0
        } else {
            ((free as u64 * 1000) / a.vram_total_kb as u64).min(1000) as i64
        };
        s + ratio
    }

    /// 因子判定：返回 (容量, 带宽, 延迟, 功耗, P2P, 是否默认兜底, 是否延迟降级)。
    fn factors(a: &Adapter, req: &PlaceReq) -> (bool, bool, bool, bool, bool, bool, bool) {
        let cap_ok = req.size_kb > 0 && a.usable_free_kb() >= req.size_kb;

        // 带宽：未声明 = 不约束，但记 DEFAULTED。
        let (bw_ok, bw_default) = if req.min_bandwidth_gbps == 0 {
            (true, true)
        } else {
            (a.bandwidth_gbps >= req.min_bandwidth_gbps, false)
        };

        // 延迟：敏感资源要求 NUMA 距离 ≤ 1（本地或同路）。
        // 修正记录：初版写作 `discrete || numa==0`——外接卡是独显因而被判「延迟达标」，
        // 但外接卡的实际延迟惩罚来自 PCIe 跳数而不是「是不是独显」。判据
        // C106-02 因此恒绿却测不到东西。改为按距离判，独显身份只参与评分。
        let lat_ok = !req.latency_sensitive || a.numa_distance <= 1;

        // 功耗：未声明 = 不约束（DEFAULTED）；已声明但卡无数据 = 不拦（缺数据不猜）。
        let (pwr_ok, pwr_default) = if req.power_budget_mw == 0 {
            (true, true)
        } else if a.power_headroom_mw == 0 {
            (true, true)
        } else {
            (a.power_headroom_mw >= req.power_budget_mw, false)
        };

        // P2P：要求与每个 peer 都可达。
        let mut p2p_ok = true;
        let mut i = 0u8;
        while i < 8 {
            if (req.peer_mask & (1u8 << i)) != 0 && i != a.id && !a.has_p2p(i) {
                p2p_ok = false;
            }
            i += 1;
        }

        (cap_ok, bw_ok, lat_ok, pwr_ok, p2p_ok, bw_default || pwr_default, false)
    }

    /// 放置：唯一决策入口。
    pub fn place(&mut self, req: PlaceReq) -> Outcome {
        if req.size_kb == 0 {
            self.last_code = XC_SIZE_ZERO;
            return Outcome::Rejected { code: XC_SIZE_ZERO };
        }
        if self.adapters.is_empty() {
            self.last_code = XC_ADAPTER_BAD;
            return Outcome::Rejected { code: XC_ADAPTER_BAD };
        }

        let mut best: Option<(i64, Adapter, Reasons, bool, bool)> = None;
        let mut best_free: u32 = 0;
        let mut best_free_id: u8 = 0;
        let mut i = 0usize;
        while i < self.adapters.len() {
            let a = self.adapters[i];
            let (cap_ok, bw_ok, lat_ok, pwr_ok, p2p_ok, defaulted, _) = Self::factors(&a, &req);

            // 记录最接近的一张卡（供否决建议用）。
            if a.usable_free_kb() > best_free {
                best_free = a.usable_free_kb();
                best_free_id = a.id;
            }

            // 硬条件：容量 + 带宽 + 功耗 + P2P 必须全过；延迟是软条件（可降级）。
            if cap_ok && bw_ok && pwr_ok && p2p_ok {
                let mut bits = Reasons::CAPACITY;
                if bw_ok && req.min_bandwidth_gbps > 0 {
                    bits |= Reasons::BANDWIDTH;
                }
                if lat_ok {
                    bits |= Reasons::LATENCY;
                }
                if pwr_ok && req.power_budget_mw > 0 {
                    bits |= Reasons::POWER;
                }
                if p2p_ok && req.peer_mask != 0 {
                    bits |= Reasons::P2P;
                }
                if defaulted {
                    bits |= Reasons::DEFAULTED;
                }
                let sc = Self::score(&a, &req);
                let cand = (sc, a, Reasons(bits), defaulted, !lat_ok);
                best = match best {
                    None => Some(cand),
                    Some(cur) => {
                        // 评分高者胜；同分时：延迟达标者胜；再同分取 id 小者（确定性）。
                        let take = sc > cur.0
                            || (sc == cur.0 && cur.4 && !cand.4)
                            || (sc == cur.0 && cur.4 == cand.4 && a.id < cur.1.id);
                        if take {
                            Some(cand)
                        } else {
                            Some(cur)
                        }
                    }
                };
            }
            i += 1;
        }

        match best {
            Some((sc, a, reasons, defaulted, lat_deg)) => {
                if reasons.count() == 0 {
                    // 理论上不可达（至少 CAPACITY 位已置），但保留显性防线。
                    self.last_code = XC_NO_REASON;
                    return Outcome::Rejected { code: XC_NO_REASON };
                }
                let p = Placement {
                    res_id: req.res_id,
                    adapter: a.id,
                    score: sc,
                    reasons,
                    defaulted,
                    latency_degraded: lat_deg,
                };
                if lat_deg {
                    self.degraded += 1;
                }
                if self.log.len() < 64 {
                    self.log.push(p);
                } else {
                    self.log_dropped += 1;
                }
                let idx = a.id as usize;
                if idx < MAX_ADAPTERS {
                    self.heat_count[idx] = self.heat_count[idx].saturating_add(1);
                    self.heat_kb[idx] = self.heat_kb[idx].saturating_add(req.size_kb);
                }
                self.last_code = 0;
                Outcome::Placed(p)
            }
            None => {
                self.denied += 1;
                // 归因：容量不足 vs P2P 不满足（两者处置完全不同）。
                let code = if req.peer_mask != 0 && best_free >= req.size_kb {
                    XC_PEER_MISS
                } else {
                    XC_NO_FIT
                };
                self.last_code = code;
                let shortfall = req.size_kb.saturating_sub(best_free);
                Outcome::Denied { shortfall_kb: shortfall, best_id: best_free_id, code }
            }
        }
    }

    /// 重放置建议：资源已在 `from`，是否有明显更好的卡。
    ///
    /// 只在**增益超过阈值**时给建议——搬迁移有真实代价（拷贝 + 帧抖动），
    /// 增益不足就搬是「优化幻觉」。
    pub fn suggest_move(&mut self, req: PlaceReq, from: u8) -> Option<MoveAdvice> {
        let cur = self.adapters.iter().find(|a| a.id == from).copied()?;
        let cur_score = Self::score(&cur, &req);
        let mut best: Option<Adapter> = None;
        let mut best_score = cur_score;
        for a in &self.adapters {
            if a.id == from {
                continue;
            }
            let (cap_ok, bw_ok, lat_ok, pwr_ok, p2p_ok, _, _) = Self::factors(a, &req);
            if cap_ok && bw_ok && pwr_ok && p2p_ok && lat_ok {
                let s = Self::score(a, &req);
                if s > best_score {
                    best_score = s;
                    best = Some(*a);
                }
            }
        }
        let target = match best {
            Some(t) => t,
            None => {
                // 没有更优目标也是「不值得搬」的一种，必须留下归因码，
                // 否则调用方分不清「没算」和「算过但否决」。
                self.last_code = XC_MOVE_NOT_BETTER;
                return None;
            }
        };
        let gain = best_score - cur_score;
        // 阈值：gain / cur_score ≥ 10%（cur_score 可能为负，取绝对值下界）。
        let base = if cur_score > 0 { cur_score } else { 1 };
        if (gain as u64) * 1000 < (base as u64) * MOVE_MARGIN_PERMILLE as u64 {
            self.last_code = XC_MOVE_NOT_BETTER;
            return None;
        }
        let cost_us = if target.bandwidth_gbps == 0 {
            0
        } else {
            // size_kb / (GB/s) 换算 µs：KB / (GB/s) = KB·s/GB = size_kb*1e6/(bw*1e6) µs
            let us = (req.size_kb as u64) * 1000u64 / (target.bandwidth_gbps as u64 * 1000u64);
            if us > u32::MAX as u64 {
                u32::MAX
            } else {
                us as u32
            }
        };
        Some(MoveAdvice { res_id: req.res_id, from, to: target.id, gain, cost_us })
    }

    /// 热图：每卡 (资源数, KB)。
    pub fn heatmap(&self) -> ([u32; MAX_ADAPTERS], [u32; MAX_ADAPTERS]) {
        (self.heat_count, self.heat_kb)
    }

    /// 热图守恒：热图计数之和 ≡ 已放置资源总数（账内 + 溢出）。
    ///
    /// 只对「计数」守恒：决策账是定容（64 条）且**不保存尺寸**，
    /// 溢出后无法逐条对字节，因此字节侧只作展示（`heat_kb`），
    /// 不参与守恒断言——写一条永远为真的字节「守恒式」比不写更糟。
    pub fn heatmap_balanced(&self) -> bool {
        let mut cnt = 0u32;
        for c in self.heat_count.iter() {
            cnt = cnt.saturating_add(*c);
        }
        let placed = self.log.len() as u32 + self.log_dropped;
        cnt == placed
    }

    /// 可解释文本（读屏可达）。
    pub fn explain(&self, i: usize) -> String {
        let mut s = String::new();
        match self.log.get(i) {
            None => s.push_str("放置记录越界"),
            Some(p) => {
                s.push_str("资源 #");
                push_u32(&mut s, p.res_id);
                s.push_str(" → 适配器 ");
                push_u32(&mut s, p.adapter as u32);
                s.push_str(" 评分=");
                push_i64(&mut s, p.score);
                s.push_str(" 理由:");
                if p.reasons.has(Reasons::CAPACITY) {
                    s.push_str("容量 ");
                }
                if p.reasons.has(Reasons::BANDWIDTH) {
                    s.push_str("带宽 ");
                }
                if p.reasons.has(Reasons::LATENCY) {
                    s.push_str("延迟 ");
                }
                if p.reasons.has(Reasons::POWER) {
                    s.push_str("功耗 ");
                }
                if p.reasons.has(Reasons::P2P) {
                    s.push_str("P2P ");
                }
                if p.reasons.has(Reasons::DEFAULTED) {
                    s.push_str("(默认因子) ");
                }
                if p.latency_degraded {
                    s.push_str("「延迟降级」");
                }
            }
        }
        s
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
        push_u32(s, (-v) as u32);
    } else {
        push_u32(s, v as u32);
    }
}

/// 标准三卡拓扑：0=集显 32GB/s 本地；1=独显 512GB/s 本地；2=外接 256GB/s 远端。
pub fn standard_topology() -> Placer {
    let mut p = Placer::new();
    let _ = p.add_adapter(Adapter {
        id: 0,
        vram_total_kb: 8 * 1024 * 1024,
        vram_free_kb: 6 * 1024 * 1024,
        bandwidth_gbps: 32,
        numa_distance: 0,
        discrete: false,
        power_headroom_mw: 15_000,
        p2p_mask: 0b010,
    });
    let _ = p.add_adapter(Adapter {
        id: 1,
        vram_total_kb: 16 * 1024 * 1024,
        vram_free_kb: 12 * 1024 * 1024,
        bandwidth_gbps: 512,
        numa_distance: 0,
        discrete: true,
        power_headroom_mw: 180_000,
        p2p_mask: 0b011,
    });
    let _ = p.add_adapter(Adapter {
        id: 2,
        vram_total_kb: 8 * 1024 * 1024,
        vram_free_kb: 7 * 1024 * 1024,
        bandwidth_gbps: 256,
        numa_distance: 2,
        discrete: true,
        power_headroom_mw: 90_000,
        p2p_mask: 0b010,
    });
    p
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

fn req(id: u32, size_kb: u32) -> PlaceReq {
    PlaceReq {
        res_id: id,
        size_kb,
        min_bandwidth_gbps: 0,
        latency_sensitive: false,
        power_budget_mw: 0,
        peer_mask: 0,
        sysmem_fallback_ok: true,
    }
}

/// VE-F0106 判据入口。
pub fn run_vea106_checks() -> CheckSet {
    let mut set = CheckSet::new("vea106_placement");

    // C106-01 带宽因子：高带宽需求必须落到独显（512GB/s），不放集显
    {
        let mut p = standard_topology();
        let mut r = req(1, 1024);
        r.min_bandwidth_gbps = 400;
        let o = p.place(r);
        let ok = matches!(o, Outcome::Placed(x) if x.adapter == 1 && x.reasons.has(Reasons::BANDWIDTH));
        chk(&mut set, "C106-01 带宽因子", ok, "高带宽需求落错卡");
    }

    // C106-02 延迟因子：延迟敏感资源不给远端外接卡
    {
        let mut p = standard_topology();
        let mut r = req(2, 1024);
        r.min_bandwidth_gbps = 240;
        r.latency_sensitive = true;
        let o = p.place(r);
        let ok = matches!(o, Outcome::Placed(x) if x.adapter == 1 && x.reasons.has(Reasons::LATENCY));
        chk(&mut set, "C106-02 延迟因子", ok, "延迟敏感资源放到远端卡");
    }

    // C106-03 功耗因子：预算不足的卡被排除
    {
        let mut p = standard_topology();
        let mut r = req(3, 512);
        r.power_budget_mw = 120_000; // 只有独显 1（180W 余量）够
        let o = p.place(r);
        let ok = matches!(o, Outcome::Placed(x) if x.adapter == 1 && x.reasons.has(Reasons::POWER));
        chk(&mut set, "C106-03 功耗因子", ok, "功耗约束被忽略");
    }

    // C106-04 因子缺失走默认（未声明带宽/功耗 → DEFAULTED 位）
    {
        let mut p = standard_topology();
        let o = p.place(req(4, 1024));
        let ok = matches!(o, Outcome::Placed(x) if x.defaulted && x.reasons.has(Reasons::DEFAULTED));
        chk(&mut set, "C106-04 因子缺失默认", ok, "缺因子未标默认");
    }

    // C106-05 可解释：理由位非空且含容量位
    {
        let mut p = standard_topology();
        let o = p.place(req(5, 2048));
        let ok = match o {
            Outcome::Placed(x) => x.reasons.count() >= 1 && x.reasons.has(Reasons::CAPACITY),
            _ => false,
        };
        chk(&mut set, "C106-05 决策可解释", ok, "理由位为空");
    }

    // C106-06 容量不足 → 否决并给缺口与最接近卡
    {
        let mut p = standard_topology();
        let o = p.place(req(6, 100u32 * 1024 * 1024)); // 100GB，谁也放不下
        let ok = matches!(o, Outcome::Denied { shortfall_kb, best_id, code }
            if shortfall_kb > 0 && best_id == 1 && code == XC_NO_FIT);
        chk(&mut set, "C106-06 容量否决", ok, "否决无缺口/无建议");
    }

    // C106-07 余量纪律：卡不能放到 100%（95% 为上限）
    {
        let mut p = Placer::new();
        let _ = p.add_adapter(Adapter {
            id: 0,
            vram_total_kb: 1000,
            vram_free_kb: 1000,
            bandwidth_gbps: 100,
            numa_distance: 0,
            discrete: true,
            power_headroom_mw: 0,
            p2p_mask: 0,
        });
        // usable = 1000 - 1000/1000*50 = 950；申请 960 必拒
        let o = p.place(req(7, 960));
        let ok = matches!(o, Outcome::Denied { .. });
        chk(&mut set, "C106-07 余量纪律", ok, "余量被吃穿");
    }

    // C106-08 P2P 因子：要求与 2 号卡 P2P，集显（仅与 1 通）不可选
    {
        let mut p = standard_topology();
        let mut r = req(8, 1024);
        r.peer_mask = 0b100; // 需要与 2 号 P2P
        let o = p.place(r);
        let ok = matches!(o, Outcome::Placed(x) if x.adapter == 2 || x.adapter == 1);
        chk(&mut set, "C106-08 P2P 因子", ok, "P2P 不可达仍被选中");
    }

    // C106-09 P2P 不可达的独立归因（容量够但 P2P 不够）
    {
        let mut p = Placer::new();
        let _ = p.add_adapter(Adapter {
            id: 0,
            vram_total_kb: 1024,
            vram_free_kb: 1024,
            bandwidth_gbps: 100,
            numa_distance: 0,
            discrete: true,
            power_headroom_mw: 0,
            p2p_mask: 0b001,
        });
        let mut r = req(9, 256);
        r.peer_mask = 0b010; // 要求与 1 号通，但 1 号不存在/不通
        let o = p.place(r);
        let ok = matches!(o, Outcome::Denied { code, .. } if code == XC_PEER_MISS);
        chk(&mut set, "C106-09 P2P 归因", ok, "P2P 失败归因错");
    }

    // C106-10 延迟降级显性（容量/带宽满足但延迟不满足 → 放行且标降级）
    {
        let mut p = Placer::new();
        let _ = p.add_adapter(Adapter {
            id: 0,
            vram_total_kb: 4096,
            vram_free_kb: 4096,
            bandwidth_gbps: 400,
            numa_distance: 3, // 远端
            discrete: false,  // 集显
            power_headroom_mw: 0,
            p2p_mask: 0,
        });
        let mut r = req(10, 1024);
        r.latency_sensitive = true;
        let o = p.place(r);
        let ok = matches!(o, Outcome::Placed(x) if x.latency_degraded && !x.reasons.has(Reasons::LATENCY))
            && p.counters().1 == 1;
        chk(&mut set, "C106-10 延迟降级显性", ok, "降级未标记");
    }

    // C106-11 重放置：明显更优才建议（大资源 → 独显）
    {
        let mut p = standard_topology();
        let mut r = req(11, 4096);
        r.min_bandwidth_gbps = 200;
        let adv = p.suggest_move(r, 0); // 假设当前在集显
        let ok = matches!(adv, Some(a) if a.to == 1 && a.gain > 0 && a.cost_us > 0);
        chk(&mut set, "C106-11 重放置建议", ok, "未给迁移建议");
    }

    // C106-12 重放置阈值：已经最优时不搬（不制造优化幻觉）
    {
        let mut p = standard_topology();
        let r = req(12, 1024);
        let adv = p.suggest_move(r, 1); // 已经在独显
        let ok = adv.is_none() && p.last_code() == XC_MOVE_NOT_BETTER;
        chk(&mut set, "C106-12 阈值防抖", ok, "最优卡上仍建议搬");
    }

    // C106-13 热图计数与决策账一致
    {
        let mut p = standard_topology();
        for i in 0..10u32 {
            let _ = p.place(req(100 + i, 512));
        }
        let (cnt, _kb) = p.heatmap();
        let mut sum = 0u32;
        for c in cnt.iter() {
            sum += *c;
        }
        chk(&mut set, "C106-13 热图守恒", sum == 10 && p.heatmap_balanced(), "热图与账不符");
    }

    // C106-14 决策账满账显性（不静默覆盖）
    {
        let mut p = standard_topology();
        for i in 0..70u32 {
            let _ = p.place(req(200 + i, 64));
        }
        let c = p.counters();
        let ok = c.2 == 64 && c.3 == 6;
        chk(&mut set, "C106-14 决策账满账", ok, "满账记账错");
    }

    // C106-15 零尺寸拒绝
    {
        let mut p = standard_topology();
        let o = p.place(req(15, 0));
        let ok = matches!(o, Outcome::Rejected { code } if code == XC_SIZE_ZERO);
        chk(&mut set, "C106-15 零尺寸拒绝", ok, "零尺寸被放置");
    }

    // C106-16 重复 id / 非法 id 拒绝
    {
        let mut p = standard_topology();
        let dup = p.add_adapter(Adapter {
            id: 1,
            vram_total_kb: 1024,
            vram_free_kb: 1024,
            bandwidth_gbps: 1,
            numa_distance: 0,
            discrete: false,
            power_headroom_mw: 0,
            p2p_mask: 0,
        });
        let bad = p.add_adapter(Adapter {
            id: 9,
            vram_total_kb: 1024,
            vram_free_kb: 1024,
            bandwidth_gbps: 1,
            numa_distance: 0,
            discrete: false,
            power_headroom_mw: 0,
            p2p_mask: 0,
        });
        chk(&mut set, "C106-16 适配器入参闸", !dup && !bad, "非法适配器被接受");
    }

    // C106-17 确定性：同拓扑同请求两次落到同一张卡
    {
        let mut a = standard_topology();
        let mut b = standard_topology();
        let r = req(17, 2048);
        let oa = a.place(r);
        let ob = b.place(r);
        let ok = matches!((oa, ob), (Outcome::Placed(x), Outcome::Placed(y)) if x.adapter == y.adapter && x.score == y.score);
        chk(&mut set, "C106-17 确定性", ok, "两次放置不同");
    }

    // C106-18 评分可比较（独显 > 集显，同条件）
    {
        let a0 = Adapter {
            id: 0,
            vram_total_kb: 8192,
            vram_free_kb: 8192,
            bandwidth_gbps: 32,
            numa_distance: 0,
            discrete: false,
            power_headroom_mw: 0,
            p2p_mask: 0,
        };
        let a1 = Adapter { id: 1, discrete: true, bandwidth_gbps: 64, ..a0 };
        let r = req(18, 1024);
        let ok = Placer::score(&a1, &r) > Placer::score(&a0, &r);
        chk(&mut set, "C106-18 评分序", ok, "独显未胜过集显");
    }

    // C106-19 读屏可解释（含资源号、卡号、理由）
    {
        let mut p = standard_topology();
        let mut r = req(19, 1024);
        r.min_bandwidth_gbps = 400;
        let _ = p.place(r);
        let line = p.explain(0);
        let ok = line.contains("资源 #") && line.contains("适配器") && line.contains("理由");
        chk(&mut set, "C106-19 解释行读屏", ok, "解释行不可读");
    }

    // C106-20 空表放置显性拒绝（不 panic）
    {
        let mut p = Placer::new();
        let o = p.place(req(20, 1024));
        let ok = matches!(o, Outcome::Rejected { code } if code == XC_ADAPTER_BAD);
        chk(&mut set, "C106-20 空表拒绝", ok, "空表未拒");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea106_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
