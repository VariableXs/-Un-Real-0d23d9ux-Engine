//! CGPU-F0008 · 掉帧降质链（CGPU-C 域 · 帧预算域 · C09 组 · 深化版）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0008`
//!
//! **判据（锚点原文）**：五级降质逐级实测画质参数生效、触发/恢复滞回
//! 行为确定、用户锁定档位生效、降质期间帧率承诺保持、降质动作全量入
//! 帧日志。
//!
//! **职责定位（锚点原文）**：保帧率舍画质的全局纪律。降质链五级：
//! L0 全质→L1 关 Bloom/折射→L2 模糊半径减半/阴影降档→L3 动态画面帧率
//! 减半（30fps）/粒子减量→L4 全局半分辨率合成→L5 最小保真（静态化最远
//! 表面）。触发条件三级：帧级超时（单帧超预算 20% 下一帧降一级）、
//! 滑动窗口（10 秒内 P95 低于承诺降一级）、合同违约（连续违约直接进
//! 对应档）。恢复条件：连续 5 秒 P95 达标且有余量，逐级回升（防抖：
//! 回升需连续 10 秒达标）。全程用户可见（设置中心显示当前画质档，可
//! 锁定档位禁自动）。降质只降参数不换算法，像素结果仍走双路对拍纪律。
//!
//! 本深化版扩出的真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **五级参数表**（判据一）：[`LEVEL_PARAMS`] L0-L5 逐级参数面
//!    （bloom/折射/模糊半径/阴影档/动态帧率减半/粒子比例/合成分辨率
//!    /远景静态化）——逐级**严格劣化**（校验面：每级至少一项比上级
//!    差，且永不回升参数），"只降参数不换算法"落在参数表闭集。
//! 2. **触发三级**（判据二）：帧级超时（单帧 > 预算×120%，[`FRAME_
//!    OVER_BPS`]）下一帧降一级；滑动窗（10s P95 < 承诺）降一级；
//!    合同违约（连续违约计数达线）**直接跳档**（[`BREACH_LEVEL_MAP`]
//!    违约档映射）。
//! 3. **恢复滞回**（判据二补）：恢复资格 5s（[`RECOVER_ELIGIBLE_MS`]）
//!    P95 达标且有余量；实际回升需 10s（[`RECOVER_CONFIRM_MS`]）连续
//!    达标——降快升慢，滞回确定可测。
//! 4. **用户锁定**（判据三）：[`DegradationChain::lock_level`] 锁档后
//!    自动触发只出建议不出档（建议留痕），解锁显性——锁定生效是
//!    机制面。
//! 5. **帧率承诺保持**（判据四）：L3 起动态帧率减半后承诺线同步改写
//!    （[`LevelParams::promise_fps`]），承诺-实测对照逐窗核验。
//! 6. **全量入帧日志**（判据五）：每次档位迁移/触发原因/参数表指纹
//!    （双路对拍锚——参数表指纹随帧日志下发给渲染面）全量留痕，FNV
//!    链式防篡改。
//! 7. **P95 计算**：10s 滑动窗（800 帧 ring），插入排序取 P95——
//!    零浮点、确定性。
//! 8. **双路对拍纪律**：档位参数表指纹（[`LevelParams::fingerprint`]）
//!    入帧日志，渲染面按同指纹取参数——参数即对拍锚。
//!
//! **对接**：F0005（仲裁器——帧级超时证据源）；F0006（帧合同——承诺
//! 口径与违约计数）；F0007（归因引擎——降质反噬归因消费本链动作）；
//! F0004（成本模型——预算基线）。零 panic 面、零 IO、零墙钟
//! （tick 注入）、零浮点、no_std。
use alloc::format;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（阈值/窗口唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DEGRADE_CHAIN_VERSION: &str = "C09-degradechain-v1";

/// 帧级超时线（bps；单帧超预算 120% 触发——锚点"超预算 20%"）。
pub const FRAME_OVER_BPS: u64 = 12_000;

/// 滑动窗容量（帧；10s @ 80fps = 800）。
pub const WINDOW_FRAMES: usize = 800;

/// 滑动窗时长（ms；P95 承诺对照窗）。
pub const WINDOW_MS: u64 = 10_000;

/// 恢复资格时长（ms；连续 5 秒 P95 达标且有余量——锚点数字）。
pub const RECOVER_ELIGIBLE_MS: u64 = 5_000;

/// 回升确认时长（ms；防抖回升需连续 10 秒达标——锚点数字）。
pub const RECOVER_CONFIRM_MS: u64 = 10_000;

/// 合同违约跳档计数（连续违约达线直接进对应档）。
pub const BREACH_JUMP_COUNT: u32 = 3;

/// 有余量线（bps；P95 达标且帧时间 ≤ 预算×80% 视为有余量）。
pub const HEADROOM_BPS: u64 = 8_000;

/// 承诺帧率（fps；L0 全质承诺线）。
pub const PROMISE_FPS_FULL: u64 = 60;

/// 降半后承诺帧率（fps；L3 起锚点"30fps"）。
pub const PROMISE_FPS_HALF: u64 = 30;

/// 帧日志容量（滚动）。
pub const FRAME_LOG_CAPACITY: usize = 512;

/// 审计账容量。
pub const EVENT_LEDGER_CAPACITY: usize = 96;

// ---------------------------------------------------------------------------
// 二、诊断码（0x24 段独占：C09 降质链面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x24 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 档位越界（L0-L5 之外）。
    LevelOutOfRange,
    /// 参数表非严格劣化（校验面——逐级必须至少一项更差）。
    ParamsNotDegrading,
    /// 预算为 0（触发面除零防线）。
    ZeroBudget,
    /// 锁档下自动降级被正确拦截（合法拒绝——调用方误用提示）。
    LockedAutoBlocked,
    /// 回升确认不足（防抖窗口未满——拒绝回升）。
    RecoverNotConfirmed,
    /// 帧日志指纹链断裂。
    LedgerCorrupt,
    /// 承诺违背（当前档承诺帧率与实测承诺线不符——承诺保持判据）。
    PromiseBroken,
    /// 违约档映射越界。
    BreachMapBad,
}

impl XcCode {
    /// 人读诊断（全带可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::LevelOutOfRange => "档位越界：降质链闭集 L0-L5，建议核对档位序号来源",
            XcCode::ParamsNotDegrading => "参数表非严格劣化：每级必须至少一项比上级差，建议核查 LEVEL_PARAMS 表",
            XcCode::ZeroBudget => "预算为 0：超时触发面除零防线，建议核对帧合同预算注入",
            XcCode::LockedAutoBlocked => "锁档下自动降级被拦截：属预期行为，自动建议已留痕，解锁请走显性指令",
            XcCode::RecoverNotConfirmed => "回升确认不足：防抖窗口（10s）未满不回升，建议继续观测",
            XcCode::LedgerCorrupt => "帧日志指纹链断裂：账面被篡改或乱序，建议以快照为准重建",
            XcCode::PromiseBroken => "承诺违背：当前档承诺帧率与实测线不符，建议核查参数表与承诺改写联动",
            XcCode::BreachMapBad => "违约档映射越界：映射表必须落在 L1-L5，建议核查 BREACH_LEVEL_MAP",
        }
    }

    /// 0x24 段十六进制码。
    pub const fn code_hex(self) -> u16 {
        match self {
            XcCode::LevelOutOfRange => 0x2401,
            XcCode::ParamsNotDegrading => 0x2402,
            XcCode::ZeroBudget => 0x2403,
            XcCode::LockedAutoBlocked => 0x2404,
            XcCode::RecoverNotConfirmed => 0x2405,
            XcCode::LedgerCorrupt => 0x2406,
            XcCode::PromiseBroken => 0x2407,
            XcCode::BreachMapBad => 0x2408,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节混入。
pub const fn fnv1a_byte(state: u64, b: u8) -> u64 {
    (state ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a u64 定长混入。
pub const fn fnv1a_u64(state: u64, v: u64) -> u64 {
    let s1 = fnv1a_byte(state, (v & 0xff) as u8);
    let s2 = fnv1a_byte(s1, ((v >> 8) & 0xff) as u8);
    let s3 = fnv1a_byte(s2, ((v >> 16) & 0xff) as u8);
    let s4 = fnv1a_byte(s3, ((v >> 24) & 0xff) as u8);
    let s5 = fnv1a_byte(s4, ((v >> 32) & 0xff) as u8);
    let s6 = fnv1a_byte(s5, ((v >> 40) & 0xff) as u8);
    let s7 = fnv1a_byte(s6, ((v >> 48) & 0xff) as u8);
    fnv1a_byte(s7, ((v >> 56) & 0xff) as u8)
}

/// FNV-1a 字符串混入。
pub const fn fnv1a_str(state: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut st = state;
    while i < b.len() {
        st = fnv1a_byte(st, b[i]);
        i += 1;
    }
    st
}

// ---------------------------------------------------------------------------
// 四、五级参数表（判据一：只降参数不换算法——参数即档位本体）
// ---------------------------------------------------------------------------

/// 单档画质参数面（L0-L5 六档闭集；渲染面按本表取参——不换算法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelParams {
    /// 档位名。
    pub name: &'static str,
    /// Bloom 后处理。
    pub bloom: bool,
    /// 折射。
    pub refraction: bool,
    /// 模糊半径减半。
    pub blur_radius_half: bool,
    /// 阴影档（0 最高 → 3 最低/关）。
    pub shadow_tier: u8,
    /// 动态帧率减半（L3 锚点 30fps）。
    pub dynamic_fps_half: bool,
    /// 粒子比例（bps；10000=全量）。
    pub particle_scale_bps: u64,
    /// 合成分辨率（bps；10000=全分辨率，5000=半分辨率 L4）。
    pub render_scale_bps: u64,
    /// 最远表面静态化（L5 最小保真）。
    pub far_static: bool,
    /// 本档帧率承诺（fps；L3 起减半改写）。
    pub promise_fps: u64,
}

impl LevelParams {
    /// 参数表指纹（双路对拍锚——渲染面同指纹取参）。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_str(FNV_OFFSET, self.name);
        let s = fnv1a_u64(s, self.bloom as u64);
        let s = fnv1a_u64(s, self.refraction as u64);
        let s = fnv1a_u64(s, self.blur_radius_half as u64);
        let s = fnv1a_u64(s, self.shadow_tier as u64);
        let s = fnv1a_u64(s, self.dynamic_fps_half as u64);
        let s = fnv1a_u64(s, self.particle_scale_bps);
        let s = fnv1a_u64(s, self.render_scale_bps);
        let s = fnv1a_u64(s, self.far_static as u64);
        fnv1a_u64(s, self.promise_fps)
    }

    /// 劣化度量（越小越差——逐级校验用：本档相对上级至少一项变差）。
    pub const fn quality_score(&self) -> u64 {
        let st = if self.shadow_tier > 3 { 3 } else { self.shadow_tier };
        self.particle_scale_bps
            .saturating_add(self.render_scale_bps)
            .saturating_add((3 - st) as u64 * 1000)
            .saturating_add(self.bloom as u64 * 500)
            .saturating_add(self.refraction as u64 * 500)
            .saturating_add(self.blur_radius_half as u64 * 200)
            .saturating_add(self.dynamic_fps_half as u64 * 800)
            .saturating_add(self.far_static as u64 * 600)
    }
}

/// 五级参数表（L0-L5；锚点逐级原文映射）。
pub const LEVEL_PARAMS: [LevelParams; 6] = [
    LevelParams {
        name: "L0全质",
        bloom: true,
        refraction: true,
        blur_radius_half: false,
        shadow_tier: 0,
        dynamic_fps_half: false,
        particle_scale_bps: 10_000,
        render_scale_bps: 10_000,
        far_static: false,
        promise_fps: PROMISE_FPS_FULL,
    },
    LevelParams {
        name: "L1关Bloom折射",
        bloom: false,
        refraction: false,
        blur_radius_half: false,
        shadow_tier: 0,
        dynamic_fps_half: false,
        particle_scale_bps: 10_000,
        render_scale_bps: 10_000,
        far_static: false,
        promise_fps: PROMISE_FPS_FULL,
    },
    LevelParams {
        name: "L2模糊减半阴影降档",
        bloom: false,
        refraction: false,
        blur_radius_half: true,
        shadow_tier: 2,
        dynamic_fps_half: false,
        particle_scale_bps: 10_000,
        render_scale_bps: 10_000,
        far_static: false,
        promise_fps: PROMISE_FPS_FULL,
    },
    LevelParams {
        name: "L3帧率减半粒子减量",
        bloom: false,
        refraction: false,
        blur_radius_half: true,
        shadow_tier: 2,
        dynamic_fps_half: true,
        particle_scale_bps: 5_000,
        render_scale_bps: 10_000,
        far_static: false,
        promise_fps: PROMISE_FPS_HALF,
    },
    LevelParams {
        name: "L4半分辨率合成",
        bloom: false,
        refraction: false,
        blur_radius_half: true,
        shadow_tier: 3,
        dynamic_fps_half: true,
        particle_scale_bps: 4_000,
        render_scale_bps: 5_000,
        far_static: false,
        promise_fps: PROMISE_FPS_HALF,
    },
    LevelParams {
        name: "L5最小保真",
        bloom: false,
        refraction: false,
        blur_radius_half: true,
        shadow_tier: 3,
        dynamic_fps_half: true,
        particle_scale_bps: 2_000,
        render_scale_bps: 5_000,
        far_static: true,
        promise_fps: PROMISE_FPS_HALF,
    },
];

/// 校验参数表逐级严格劣化（判据一机制面——非劣化即断言失败）。
pub fn verify_params_monotonic() -> Result<(), XcCode> {
    let mut i = 1usize;
    while i < LEVEL_PARAMS.len() {
        let prev = LEVEL_PARAMS[i - 1];
        let cur = LEVEL_PARAMS[i];
        if cur.quality_score() >= prev.quality_score() {
            return Err(XcCode::ParamsNotDegrading);
        }
        i += 1;
    }
    Ok(())
}

/// 违约跳档映射（连续违约计数 → 直接进入档；锚点"直接进对应档"）。
/// 违约 3 次 → L2；5 次 → L3；8 次 → L4；12 次 → L5。
pub const BREACH_LEVEL_MAP: [(u32, usize); 4] = [(3, 2), (5, 3), (8, 4), (12, 5)];

/// 由违约计数解析跳档目标（映射表闭集内取最大命中档）。
pub const fn breach_target_level(breach_count: u32) -> Result<usize, XcCode> {
    let mut target: usize = 0;
    let mut i = 0usize;
    while i < BREACH_LEVEL_MAP.len() {
        let row = BREACH_LEVEL_MAP[i];
        if breach_count >= row.0 {
            target = row.1;
        }
        i += 1;
    }
    if target > 5 {
        return Err(XcCode::BreachMapBad);
    }
    Ok(target)
}

// ---------------------------------------------------------------------------
// 五、滑动窗与 P95（10s 窗；确定性排序）
// ---------------------------------------------------------------------------

/// 10s 滑动窗（800 帧 ring；P95 承诺对照）。
#[derive(Clone, Debug)]
pub struct SlideWindow {
    samples: Vec<u64>,
    /// 累计样本数（含逐出）。
    pub produced: u64,
    /// 窗内最新帧号。
    pub last_frame: u64,
}

impl SlideWindow {
    /// 空窗。
    pub fn new() -> SlideWindow {
        SlideWindow {
            samples: Vec::new(),
            produced: 0,
            last_frame: 0,
        }
    }

    /// 打点（帧耗时 ms；滚动逐出最旧）。
    pub fn record(&mut self, frame_ms: u64, frame_id: u64) {
        if self.samples.len() >= WINDOW_FRAMES {
            let _ = self.samples.remove(0);
        }
        self.samples.push(frame_ms);
        self.produced = self.produced.saturating_add(1);
        self.last_frame = frame_id;
    }

    /// 窗内样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// P95（ms；插入排序副本取 95 分位——零浮点确定性）。
    pub fn p95_ms(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < self.samples.len() {
            sorted.push(*self.samples.get(i).unwrap_or(&0));
            i += 1;
        }
        let n = sorted.len();
        let mut a = 1usize;
        while a < n {
            let key = *sorted.get(a).unwrap_or(&0);
            let mut b = a;
            while b > 0 {
                let pv = *sorted.get(b.saturating_sub(1)).unwrap_or(&0);
                if pv > key {
                    let v = sorted.remove(b.saturating_sub(1));
                    sorted.insert(b, v);
                    b -= 1;
                } else {
                    break;
                }
            }
            a += 1;
        }
        let idx = (n.saturating_mul(95) / 100).min(n.saturating_sub(1));
        *sorted.get(idx).unwrap_or(&0)
    }

    /// 承诺帧时间上沿（ms；fps → ms 定点取整）。
    pub const fn promise_frame_ms(promise_fps: u64) -> u64 {
        if promise_fps == 0 {
            return 0;
        }
        1000 / promise_fps
    }
}

// ---------------------------------------------------------------------------
// 六、事件账（五类闭集 + 滚动 + FNV 链）
// ---------------------------------------------------------------------------

/// 账面事件闭集（五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    /// 档位下调（含原因）。
    LevelDown,
    /// 档位回升。
    LevelUp,
    /// 锁档拦截（自动建议留痕）。
    LockBlocked,
    /// 违约跳档。
    BreachJump,
    /// 参数表指纹下发（双路对拍锚）。
    ParamsSealed,
}

impl LedgerKind {
    /// 序号。
    pub const fn kind_id(self) -> u8 {
        match self {
            LedgerKind::LevelDown => 0,
            LedgerKind::LevelUp => 1,
            LedgerKind::LockBlocked => 2,
            LedgerKind::BreachJump => 3,
            LedgerKind::ParamsSealed => 4,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            LedgerKind::LevelDown => "降档",
            LedgerKind::LevelUp => "回升",
            LedgerKind::LockBlocked => "锁档拦截",
            LedgerKind::BreachJump => "违约跳档",
            LedgerKind::ParamsSealed => "参数指纹下发",
        }
    }
}

/// 账面行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRecord {
    /// 序号。
    pub seq: u64,
    /// tick。
    pub tick: u64,
    /// 类别。
    pub kind: LedgerKind,
    /// 档位。
    pub level: u8,
    /// 量。
    pub amount: u64,
    /// 摘要。
    pub summary: String,
    /// 指纹。
    pub fp: u64,
}

/// 滚动审计账。
#[derive(Clone, Debug)]
pub struct EventLedger {
    records: Vec<LedgerRecord>,
    next_seq: u64,
    /// 逐出累计。
    pub evicted: u64,
    /// 尾指纹。
    pub tail_fp: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            records: Vec::new(),
            next_seq: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 追加。
    pub fn append(&mut self, tick: u64, kind: LedgerKind, level: u8, amount: u64, summary: &str) -> u64 {
        let seq = self.next_seq;
        let mut fp = fnv1a_u64(self.tail_fp, seq);
        fp = fnv1a_u64(fp, tick);
        fp = fnv1a_u64(fp, kind.kind_id() as u64);
        fp = fnv1a_u64(fp, level as u64);
        fp = fnv1a_u64(fp, amount);
        fp = fnv1a_str(fp, summary);
        self.tail_fp = fp;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() >= EVENT_LEDGER_CAPACITY {
            let _ = self.records.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push(LedgerRecord {
            seq,
            tick,
            kind,
            level,
            amount,
            summary: summary.to_string(),
            fp,
        });
        seq
    }

    /// 链复验。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let mut fp = fnv1a_u64(prev, r.seq);
            fp = fnv1a_u64(fp, r.tick);
            fp = fnv1a_u64(fp, r.kind.kind_id() as u64);
            fp = fnv1a_u64(fp, r.level as u64);
            fp = fnv1a_u64(fp, r.amount);
            fp = fnv1a_str(fp, &r.summary);
            if fp != r.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = r.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }

    /// 按类别计数。
    pub fn count_by_kind(&self, kind: LedgerKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.kind == kind {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 七、降质链总编排（触发三级 + 恢复滞回 + 锁档 + 承诺保持）
// ---------------------------------------------------------------------------

/// 降质链引擎。
#[derive(Clone, Debug)]
pub struct DegradationChain {
    /// 当前档位（0-5）。
    pub level: usize,
    /// 用户锁档（Some(档位) = 锁定禁自动）。
    pub locked: Option<usize>,
    /// 滑动窗（10s P95）。
    pub window: SlideWindow,
    /// 连续违约计数（帧级超时连续累计）。
    pub breach_streak: u32,
    /// 恢复资格累计（ms；≥5000 有资格）。
    pub recover_eligible_ms: u64,
    /// 回升确认累计（ms；≥10000 才回升——防抖）。
    pub recover_confirm_ms: u64,
    /// 事件账。
    pub ledger: EventLedger,
    /// 当前档参数表指纹（帧日志对拍锚）。
    pub params_fp: u64,
    /// 降档次数（含跳档）。
    pub down_total: u64,
    /// 回升次数。
    pub up_total: u64,
    /// 锁档拦截次数（自动建议留痕）。
    pub lock_blocked_total: u64,
    /// 帧号推进。
    pub last_frame: u64,
    /// 上次窗对照 tick。
    pub last_window_check_tick: u64,
}

impl DegradationChain {
    /// 新链（L0 起步；参数表校验先行——非严格劣化构造即失败语义由
    /// 校验面承担）。
    pub fn new() -> Result<DegradationChain, XcCode> {
        verify_params_monotonic()?;
        let mut chain = DegradationChain {
            level: 0,
            locked: None,
            window: SlideWindow::new(),
            breach_streak: 0,
            recover_eligible_ms: 0,
            recover_confirm_ms: 0,
            ledger: EventLedger::new(),
            params_fp: LEVEL_PARAMS[0].fingerprint(),
            down_total: 0,
            up_total: 0,
            lock_blocked_total: 0,
            last_frame: 0,
            last_window_check_tick: 0,
        };
        chain.ledger.append(0, LedgerKind::ParamsSealed, 0, 0, "L0 参数表指纹下发（对拍锚）");
        Ok(chain)
    }

    /// 当前档参数（渲染面取参入口）。
    pub fn params(&self) -> &'static LevelParams {
        &LEVEL_PARAMS[self.level_clamped()]
    }

    /// 档位钳制（const 合规写法——不用 Ord::min）。
    pub const fn level_clamped(&self) -> usize {
        if self.level > 5 {
            5
        } else {
            self.level
        }
    }

    /// 当前承诺帧率（承诺保持判据的读数面）。
    pub const fn promise_fps(&self) -> u64 {
        LEVEL_PARAMS[self.level_clamped()].promise_fps
    }

    /// 锁档（显性；锁在当前档）。
    pub fn lock_level(&mut self) {
        self.locked = Some(self.level.min(5));
    }

    /// 解锁（显性）。
    pub fn unlock(&mut self) {
        self.locked = None;
    }

    /// 逐帧推进（证据注入：帧耗时/预算；返回迁移后的档位）。
    /// 触发三级在帧面统一裁决——帧级超时与违约计数在帧粒度，
    /// 滑动窗在窗粒度（每 1s 核对一次防过频）。
    pub fn on_frame(
        &mut self,
        frame_id: u64,
        frame_ms: u64,
        budget_ms: u64,
        tick: u64,
    ) -> Result<usize, XcCode> {
        if budget_ms == 0 {
            return Err(XcCode::ZeroBudget);
        }
        self.window.record(frame_ms, frame_id);
        self.last_frame = frame_id;
        let mut changed = false;
        // —— 触发一：帧级超时（单帧超预算 20%）——
        let over_line = budget_ms.saturating_mul(FRAME_OVER_BPS) / 10_000;
        if frame_ms > over_line {
            self.breach_streak = self.breach_streak.saturating_add(1);
            // 违约跳档面（连续违约达线直接进对应档）。
            let target = breach_target_level(self.breach_streak)?;
            if self.breach_streak >= BREACH_JUMP_COUNT && target > self.level {
                changed = self.apply_level(target, LedgerKind::BreachJump, tick, "连续违约直接跳档")?;
            } else if target <= self.level {
                // 未达跳档线：普通降一级（下一帧生效语义——本帧即记）。
                if self.level > 0 {
                    changed = self.apply_level(self.level - 1, LedgerKind::LevelDown, tick, "帧级超时降一级")?;
                }
            }
        } else {
            // 达标：违约连击清零 + 恢复累计推进。
            self.breach_streak = 0;
            let promise_ms = SlideWindow::promise_frame_ms(self.promise_fps());
            if frame_ms <= promise_ms.saturating_mul(HEADROOM_BPS) / 10_000 {
                // 达标且有余量：推进恢复双计时（Δ帧时间近似——用帧间隔
                // 无法注入，取承诺帧时间作为时钟粒度——tick 注入面）。
                self.recover_eligible_ms = self.recover_eligible_ms.saturating_add(promise_ms.max(1));
                self.recover_confirm_ms = self.recover_confirm_ms.saturating_add(promise_ms.max(1));
            } else {
                // 达标但无余量：只推进资格，不推进防抖确认。
                self.recover_eligible_ms = self.recover_eligible_ms.saturating_add(promise_ms.max(1));
                self.recover_confirm_ms = 0;
            }
            // —— 恢复面（降快升慢：资格 5s + 确认 10s 双闸）——
            if self.level > 0
                && self.recover_eligible_ms >= RECOVER_ELIGIBLE_MS
                && self.recover_confirm_ms >= RECOVER_CONFIRM_MS
            {
                changed = self.apply_level(self.level - 1, LedgerKind::LevelUp, tick, "连续达标回升一级")?;
                self.recover_eligible_ms = 0;
                self.recover_confirm_ms = 0;
            }
        }
        // —— 触发二：滑动窗（每 1s 核对；P95 低于承诺降一级）——
        if tick.saturating_sub(self.last_window_check_tick) >= 1000 && self.window.len() >= 8 {
            self.last_window_check_tick = tick;
            let p95 = self.window.p95_ms();
            let promise_ms = SlideWindow::promise_frame_ms(self.promise_fps());
            if p95 > promise_ms && self.level > 0 {
                let _ = self.apply_level(self.level - 1, LedgerKind::LevelDown, tick, "滑动窗 P95 低于承诺降一级");
                changed = true;
            }
        }
        let _ = changed;
        Ok(self.level)
    }

    /// 应用档位迁移（锁档拦截 + 全量入帧日志 + 参数指纹重下发）。
    fn apply_level(&mut self, target: usize, kind: LedgerKind, tick: u64, reason: &str) -> Result<bool, XcCode> {
        if target > 5 {
            return Err(XcCode::LevelOutOfRange);
        }
        if target == self.level {
            return Ok(false);
        }
        // 锁档拦截：自动触发只出建议不出档（判据三机制面）。
        if self.locked.is_some() {
            self.lock_blocked_total = self.lock_blocked_total.saturating_add(1);
            self.ledger.append(
                tick,
                LedgerKind::LockBlocked,
                self.level as u8,
                target as u64,
                &alloc::format!("锁档拦截：{} 建议→L{}（未执行）", reason, target),
            );
            return Ok(false);
        }
        let from = self.level;
        self.level = target;
        self.params_fp = LEVEL_PARAMS[target].fingerprint();
        self.ledger.append(
            tick,
            kind,
            target as u8,
            from as u64,
            &alloc::format!("{}：L{}→L{}", reason, from, target),
        );
        self.ledger.append(
            tick,
            LedgerKind::ParamsSealed,
            target as u8,
            0,
            "参数表指纹重下发（对拍锚）",
        );
        if target < from {
            self.up_total = self.up_total.saturating_add(1);
        } else {
            self.down_total = self.down_total.saturating_add(1);
        }
        Ok(true)
    }

    /// 帧日志链复验。
    pub fn ledger_intact(&self) -> Result<(), XcCode> {
        self.ledger.verify_chain()
    }

    /// 承诺保持核验（判据四：当前档承诺与参数表一致——联动改写没断）。
    pub fn promise_intact(&self) -> Result<(), XcCode> {
        let cur = &LEVEL_PARAMS[self.level_clamped()];
        let expect = if cur.dynamic_fps_half { PROMISE_FPS_HALF } else { PROMISE_FPS_FULL };
        if cur.promise_fps != expect {
            return Err(XcCode::PromiseBroken);
        }
        Ok(())
    }

    /// 全量校验（参数表单调 + 映射表合法 + 承诺联动 + 账链）。
    pub fn validate(&self) -> Result<(), XcCode> {
        verify_params_monotonic()?;
        // 映射表全行合法。
        let mut i = 0usize;
        while i < BREACH_LEVEL_MAP.len() {
            let row = BREACH_LEVEL_MAP[i];
            if row.1 > 5 {
                return Err(XcCode::BreachMapBad);
            }
            i += 1;
        }
        self.promise_intact()?;
        self.ledger_intact()?;
        if self.level > 5 {
            return Err(XcCode::LevelOutOfRange);
        }
        Ok(())
    }

    /// 人读汇总（设置中心显示当前画质档——判据三可见面）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        let cur = self.params();
        s.push_str("== CGPU-F0008 掉帧降质链 汇报 ==\n");
        s.push_str(&alloc::format!(
            "当前档：{}（{}）承诺 {}fps 锁定={}\n",
            cur.name,
            self.level,
            cur.promise_fps,
            self.locked.is_some()
        ));
        s.push_str(&alloc::format!(
            "降档 {} 回升 {} 锁档拦截 {} 违约连击 {} 账 {} 行\n",
            self.down_total,
            self.up_total,
            self.lock_blocked_total,
            self.breach_streak,
            self.ledger.len()
        ));
        s.push_str(&alloc::format!(
            "参数：bloom={} 折射={} 模糊减半={} 阴影档{} 帧率减半={} 粒子{} 合成{} 远景静态={} 指纹 {:016x}\n",
            cur.bloom,
            cur.refraction,
            cur.blur_radius_half,
            cur.shadow_tier,
            cur.dynamic_fps_half,
            cur.particle_scale_bps,
            cur.render_scale_bps,
            cur.far_static,
            self.params_fp
        ));
        s
    }

    /// 无障碍读屏（三段固定预算：当前档/承诺/锁定态——全程用户可见）。
    pub fn read_screen(&self) -> Vec<String> {
        let mut out = Vec::new();
        let cur = self.params();
        out.push(alloc::format!("当前画质档：{}（第 {} 档）", cur.name, self.level));
        out.push(alloc::format!("帧率承诺：{}fps", cur.promise_fps));
        if let Some(lk) = self.locked {
            out.push(alloc::format!("用户已锁定档位 L{}：自动降级暂停", lk));
        } else if self.level > 0 {
            out.push("自动降级运行中：连续达标将逐级回升".to_string());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 八、参考探针（判据自证——五级生效/滞回确定/锁档/承诺/日志）
// ---------------------------------------------------------------------------

/// 参考探针：核心判据自证（纯函数口径，零 IO）。
pub fn run_cgc09_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    // 判据一：五级逐级参数生效（严格劣化 + 每级指纹互异）。
    out.push(("参数表严格劣化", verify_params_monotonic().is_ok()));
    out.push(("五级指纹互异", {
        let mut i = 0usize;
        let mut distinct = true;
        while i < LEVEL_PARAMS.len() {
            let mut j = i.saturating_add(1);
            while j < LEVEL_PARAMS.len() {
                let a = LEVEL_PARAMS[i];
                let b = LEVEL_PARAMS[j];
                if a.fingerprint() == b.fingerprint() {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        distinct
    }));
    out.push(("L1 关Bloom折射", !LEVEL_PARAMS[1].bloom && !LEVEL_PARAMS[1].refraction && LEVEL_PARAMS[1].blur_radius_half == LEVEL_PARAMS[0].blur_radius_half));
    out.push(("L2 模糊减半阴影降档", LEVEL_PARAMS[2].blur_radius_half && LEVEL_PARAMS[2].shadow_tier > LEVEL_PARAMS[1].shadow_tier));
    out.push(("L3 帧率减半30fps粒子减量", LEVEL_PARAMS[3].dynamic_fps_half && LEVEL_PARAMS[3].promise_fps == 30 && LEVEL_PARAMS[3].particle_scale_bps < 10_000));
    out.push(("L4 半分辨率", LEVEL_PARAMS[4].render_scale_bps == 5_000));
    out.push(("L5 远景静态化", LEVEL_PARAMS[5].far_static));
    // 违约映射。
    out.push(("违约3次跳L2", breach_target_level(3).unwrap_or(0) == 2));
    out.push(("违约12次跳L5", breach_target_level(12).unwrap_or(0) == 5));
    out.push(("违约2次不跳", breach_target_level(2).unwrap_or(0) == 0));
    // 判据二：触发滞回行为确定（降快升慢）。
    let mut ch = DegradationChain::new().expect_probe();
    let lv1 = ch.on_frame(1, 30, 25, 10).unwrap_or(0); // 30 > 25*1.2=30? 相等不触发。
    out.push(("等值不触发", lv1 == 0));
    let _ = ch.on_frame(2, 31, 25, 20); // 31 > 30 → 降 L1。
    out.push(("帧级超时降一级", ch.level == 1));
    let _ = ch.on_frame(3, 40, 25, 30); // 再超 → L2。
    out.push(("连续超时再降", ch.level == 2));
    // 达标回升：10s 防抖确认。
    let promise_ms = SlideWindow::promise_frame_ms(ch.promise_fps());
    let mut f = 4u64;
    let mut t = 40u64;
    while f < 400 && ch.level > 0 {
        let _ = ch.on_frame(f, promise_ms.saturating_mul(HEADROOM_BPS) / 10_000, 25, t);
        f += 1;
        t += promise_ms;
    }
    out.push(("防抖回升需连续达标", ch.level == 0 && ch.up_total >= 1));
    // 判据三：锁档生效。
    let mut ch2 = DegradationChain::new().expect_probe();
    let _ = ch2.on_frame(1, 50, 25, 10); // 降 L1。
    ch2.lock_level();
    let _ = ch2.on_frame(2, 50, 25, 20);
    out.push(("锁档禁自动", ch2.level == 1 && ch2.lock_blocked_total >= 1));
    ch2.unlock();
    let _ = ch2.on_frame(3, 50, 25, 30);
    out.push(("解锁后自动恢复", ch2.level == 2));
    // 判据四：承诺保持（L3 起减半改写）。
    let mut ch3 = DegradationChain::new().expect_probe();
    let _ = ch3.on_frame(1, 100, 25, 10);
    let _ = ch3.on_frame(2, 100, 25, 20);
    let _ = ch3.on_frame(3, 100, 25, 30);
    let _ = ch3.on_frame(4, 100, 25, 40);
    out.push(("L3 承诺改写30fps", ch3.level >= 3 && ch3.promise_fps() == 30));
    out.push(("承诺联动核验", ch3.promise_intact().is_ok()));
    // 判据五：降质动作全量入帧日志。
    out.push(("降档全量入账", ch3.ledger.count_by_kind(LedgerKind::LevelDown) >= ch3.down_total && ch3.down_total >= 3));
    out.push(("参数指纹随档下发", ch3.ledger.count_by_kind(LedgerKind::ParamsSealed) >= ch3.level as u64 + 1));
    // 守恒与读屏。
    out.push(("账链完整", ch3.ledger_intact().is_ok() && ch3.validate().is_ok()));
    out.push(("读屏三段", ch3.read_screen().len() == 3));
    out.push(("诊断码带建议", !XcCode::RecoverNotConfirmed.detail().is_empty() && XcCode::RecoverNotConfirmed.code_hex() == 0x2405));
    out
}

/// 探针辅助（期望语义：构造失败即探针断言面——避免 unwrap 的合规写法）。
trait ProbeExt {
    fn expect_probe(self) -> DegradationChain;
}

impl ProbeExt for Result<DegradationChain, XcCode> {
    fn expect_probe(self) -> DegradationChain {
        match self {
            Ok(c) => c,
            Err(_) => {
                // 构造失败在探针面属断言失败——回退默认链（探针仍会因
                // 参数断言不过而失败，不会静默）。
                DegradationChain {
                    level: 0,
                    locked: None,
                    window: SlideWindow::new(),
                    breach_streak: 0,
                    recover_eligible_ms: 0,
                    recover_confirm_ms: 0,
                    ledger: EventLedger::new(),
                    params_fp: LEVEL_PARAMS[0].fingerprint(),
                    down_total: 0,
                    up_total: 0,
                    lock_blocked_total: 0,
                    last_frame: 0,
                    last_window_check_tick: 0,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 旧式判据桥接（2026-10-22 接线轮补）：run_cgc09_checks 返回 CheckSet 之前的旧式
// Vec<(&'static str, bool)>，不能直接进 CheckSet 函数指针表；此桥逐项转入
// CheckSetBuilder（语义等价：判据名/通过位一一对应，零信息丢失）。
// ---------------------------------------------------------------------------
pub fn run_cgc09_checks_cs() -> crate::checks::CheckSet {
    let mut b = crate::checks::CheckSetBuilder::new("CGPU-F0008", "旧式判据桥接");
    for (name, ok) in run_cgc09_checks() {
        b.ok(name, ok, "");
    }
    b.done()
}

