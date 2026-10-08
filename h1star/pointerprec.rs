//! F250 指针精度与双击速度基线 · 判据实装。
//!
//! **判据锚**：主册 F250「指针精度与双击速度基线」。
//!
//! **验收标准第一句（任务包原文）**：1:1 映射误差 <1px（慢速/快速两档
//! 轨迹记录）。
//!
//! **判据（主册原文摘录）**：鼠标手感两个底层参数给用户且给默认：指针
//! 加速（Windows「提高指针精确度」语义）默认关（游戏与设计人群共识，
//! 设置可开）、双击速度默认 500ms 档（四档 300/400/500/700 可调，设置
//! 页带测试靶——点气球验证当前档位）；指针移动走合成器直通路径（不经
//! 过多余处理层，F063 触控板前瞻同源），1:1 物理映射。
//!
//! **设计要点**：
//! - 加速关（默认）= 1:1 直通：输出位移逐字节等于输入位移，整数域
//!   误差恒 0（<1px 的严格满足）；误差由轨迹环逐条复核，不做抽样；
//! - 加速开 = 经典三段斜率曲线（低/中/高，×100 定点）：按瞬时速度
//!   （counts/ms ×100 定点）分档取增益——低速 1:1 保守、高速放大；
//!   段界 0.1 / 1.0 counts/ms，标注为 Windows「提高指针精确度」的
//!   定性三段近似；
//! - 双击判定边界：间隔 **≤ 档值** 判双击（500ms 档下 500 判双击、
//!   501 判单击）——边界值判定是验收主路径；
//! - 测试靶「点气球」：两击间隔实测 → 气球爆开并回显间隔与档位，
//!   让用户用身体确认当前档位（判据「设置页带测试靶」的交互面）；
//! - 直通路径是架构位：路由表显式列出处理层，判定函数证明指针移动
//!   不经过额外变换层（与 F063 触控板前瞻同源的直通道德）；
//! - 零堆热路径：移动/点击判定全定长结构；轨迹记入定容环（256 条，
//!   滚动淘汰）；时间一律注入（毫秒戳）。
//!
//! **依赖锚点**：F063（触控板前瞻——直通路径同源）、F237（设置项
//! 持久化面共用位包编码纪律）。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, RingLog};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 双击速度四档（ms）——主册「四档 300/400/500/700 可调」数值原文。
pub const DBLCLICK_TIERS_MS: [u64; 4] = [300, 400, 500, 700];

/// 默认档下标 = 2（500ms 档）——主册「双击速度默认 500ms 档」。
pub const DBLCLICK_DEFAULT_TIER: usize = 2;

/// 加速增益定点标度（×100）。
pub const GAIN_SCALE: u32 = 100;

/// 经典加速曲线低段增益（×100 = 1.0 倍）。
pub const GAIN_LOW: u32 = 100;

/// 经典加速曲线中段增益（×100 = 1.5 倍）。
pub const GAIN_MID: u32 = 150;

/// 经典加速曲线高段增益（×100 = 2.0 倍）。
pub const GAIN_HIGH: u32 = 200;

/// 低/中段界：瞬时速度 0.1 counts/ms（×100 定点 = 10）。
pub const ZONE_LOW_MAX: u32 = 10;

/// 中/高段界：瞬时速度 1.0 counts/ms（×100 定点 = 100）。
pub const ZONE_MID_MAX: u32 = 100;

/// 1:1 映射误差门（px）——判据「<1px」，整数直通实现下恒 0。
pub const MAP_ERR_LIMIT_PX: i64 = 1;

/// 轨迹环容量——慢速/快速两档轨迹记录（滚动淘汰，定容纪律）。
pub const TRAJ_CAP: usize = 256;

/// 测试靶实测间隔环容量——爆开与未爆都留痕，档位手感统计的原始凭据。
pub const TARGET_HIST_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单步移动轨迹记录（误差审计的原始凭据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveRec {
    /// 事件时刻（ms，注入式）。
    pub ts: u64,
    /// 输入位移（counts → px 直通语义下的物理像素）。
    pub dx: i32,
    pub dy: i32,
    /// 输出位移（最终作用到光标的像素）。
    pub ox: i32,
    pub oy: i32,
}

impl MoveRec {
    /// 本步映射误差（曼哈顿距离，px）。
    pub fn err_px(&self) -> i64 {
        (self.ox - self.dx).abs() as i64 + (self.oy - self.dy).abs() as i64
    }
}

/// 双击/单击判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickVerdict {
    /// 两击间隔 > 档值：单击。
    Single,
    /// 两击间隔 ≤ 档值：双击。
    Double,
}

/// 测试靶（点气球）判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetVerdict {
    /// 等待第二击。
    Waiting,
    /// 气球爆开：回显实测间隔与当前档位值（用户用身体确认档位）。
    Popped { interval_ms: u64, tier_ms: u64 },
}

// ---------------------------------------------------------------------------
// 指针调速器
// ---------------------------------------------------------------------------

/// 指针精度治理器：加速开关 + 双击档位 + 轨迹审计 + 测试靶。
pub struct PointerGov {
    /// 指针加速（Windows「提高指针精确度」语义）——**默认关**。
    pub accel: bool,
    /// 双击速度档下标（0..=3，默认 2 = 500ms 档）。
    tier: usize,
    /// 轨迹环（滚动淘汰，误差审计凭据）。
    traj: RingLog<MoveRec, TRAJ_CAP>,
    last_move_ts: Option<u64>,
    /// 测试靶状态：上一击时刻（None = 等第一击）。
    target_last_ts: Option<u64>,
    /// 气球爆开次数（测试靶交互走查的累计凭据）。
    pub balloon_pops: u32,
    /// 测试靶试验次数（第二击次数，爆开与否都算一次手感实测）。
    target_trials: u32,
    /// 测试靶实测间隔环（最新在前的滚动记录）。
    target_intervals: RingLog<u64, TARGET_HIST_CAP>,
    /// 设置版本号（持久化面/即时生效判定）。
    pub version: u32,
}

impl PointerGov {
    pub fn new() -> PointerGov {
        PointerGov {
            accel: false,
            tier: DBLCLICK_DEFAULT_TIER,
            traj: RingLog::new(),
            last_move_ts: None,
            target_last_ts: None,
            balloon_pops: 0,
            target_trials: 0,
            target_intervals: RingLog::new(),
            version: 0,
        }
    }

    pub fn tier(&self) -> usize {
        self.tier
    }

    pub fn tier_ms(&self) -> u64 {
        DBLCLICK_TIERS_MS[self.tier]
    }

    /// 双击档位设置（四档钳制；变化即推进版本——设置页即时生效）。
    pub fn set_tier(&mut self, tier: usize) -> usize {
        let t = tier.min(3);
        if t != self.tier {
            self.tier = t;
            self.version = self.version.wrapping_add(1);
        }
        t
    }

    /// 加速开关（默认关；变化即推进版本）。
    pub fn set_accel(&mut self, on: bool) {
        if on != self.accel {
            self.accel = on;
            self.version = self.version.wrapping_add(1);
        }
    }

    /// 经典加速三段斜率：瞬时速度（counts/ms ×100）→ 增益（×100）。
    /// 段界 0.1 / 1.0 counts/ms：低速保 1:1（游戏/设计人群的底线），
    /// 高速放大（少抬腕走长距的桌面直觉）。
    pub fn accel_gain(speed_c100: u32) -> u32 {
        if speed_c100 <= ZONE_LOW_MAX {
            GAIN_LOW
        } else if speed_c100 <= ZONE_MID_MAX {
            GAIN_MID
        } else {
            GAIN_HIGH
        }
    }

    /// 指针移动主路径（合成器直通位）。
    ///
    /// 加速关：输出 = 输入，逐字节 1:1；加速开：按瞬时速度取三段增益
    /// （首步无速度参考 → 直通）。每步记录入轨迹环供误差审计。
    pub fn apply_move(&mut self, dx: i32, dy: i32, ts: u64) -> (i32, i32) {
        let (ox, oy) = if !self.accel {
            (dx, dy)
        } else {
            match self.last_move_ts {
                Some(prev) => {
                    let dt = ts.saturating_sub(prev).max(1);
                    let dist = (dx.abs() as u64 + dy.abs() as u64) as u32;
                    let speed_c100 = ((dist as u64 * GAIN_SCALE as u64) / dt as u64) as u32;
                    let gain = PointerGov::accel_gain(speed_c100);
                    (
                        (dx as i64 * gain as i64 / GAIN_SCALE as i64) as i32,
                        (dy as i64 * gain as i64 / GAIN_SCALE as i64) as i32,
                    )
                }
                None => (dx, dy),
            }
        };
        self.last_move_ts = Some(ts);
        self.traj.push(MoveRec { ts, dx, dy, ox, oy });
        (ox, oy)
    }

    /// 轨迹最大映射误差（px）——1:1 判定「<1px」的实测凭据。
    /// 加速关闭时段内全部记录逐条复核，恒 0。
    pub fn max_map_err_px(&self) -> i64 {
        let mut worst: i64 = 0;
        for m in self.traj.newest_first().iter() {
            worst = worst.max(m.err_px());
        }
        worst
    }

    /// 轨迹记录条数（审计面：慢速/快速两档都须有记录才下结论）。
    pub fn traj_len(&self) -> usize {
        self.traj.len()
    }

    /// 直通路径架构位：指针移动处理层列表。
    ///
    /// 判据「走合成器直通路径（不经过多余处理层）」——表里没有输入
    /// 变换/平滑/预测层；1:1 映射在该架构下是结构事实而非参数巧合。
    pub fn route_layers() -> &'static [&'static str] {
        &["hid-report", "compositor-direct"]
    }

    /// 额外处理层存在性判定（恒 false；一旦架构加层，这里必须同步改
    /// 并重跑 1:1 误差判据——直通是承诺不是巧合）。
    pub fn extra_layer_present() -> bool {
        false
    }

    /// 双击判定：间隔 ≤ 档值判双击（边界值归双击侧）。
    pub fn classify_click(&self, interval_ms: u64) -> ClickVerdict {
        if interval_ms <= self.tier_ms() {
            ClickVerdict::Double
        } else {
            ClickVerdict::Single
        }
    }

    /// 测试靶（点气球）：两击间隔实测 → 爆开并回显间隔与档位。
    pub fn target_click(&mut self, ts: u64) -> TargetVerdict {
        match self.target_last_ts {
            None => {
                self.target_last_ts = Some(ts);
                TargetVerdict::Waiting
            }
            Some(prev) => {
                let interval = ts.saturating_sub(prev);
                self.target_last_ts = None;
                self.target_trials = self.target_trials.wrapping_add(1);
                self.target_intervals.push(interval);
                let tier_ms = self.tier_ms();
                if interval <= tier_ms {
                    self.balloon_pops += 1;
                    TargetVerdict::Popped { interval_ms: interval, tier_ms }
                } else {
                    TargetVerdict::Waiting // 超档：气球不爆，重新等第一击
                }
            }
        }
    }

    /// 测试靶累计试验次数（第二击次数——爆开与否都算一次手感实测）。
    pub fn target_trials(&self) -> u32 {
        self.target_trials
    }

    /// 测试靶实测间隔列表（最新在前——档位手感统计的逐条凭据）。
    pub fn target_intervals(&self) -> Vec<u64> {
        self.target_intervals.newest_first()
    }

    /// 双击命中率（百分比 0~100）：爆开次数 / 试验次数。无试验时为 0。
    /// 判据「设置页带测试靶」的量化面：档位合不合适由这个数字说话——
    /// 命中率过低说明用户手速快于当前档位，设置页可据此给出调档建议。
    pub fn double_rate_pct(&self) -> u32 {
        if self.target_trials == 0 {
            0
        } else {
            (self.balloon_pops as u64 * 100 / self.target_trials as u64) as u32
        }
    }

    /// 轨迹映射误差 P95（px）——「<1px」的分位审计：主干交互满足即
    /// 交互质量满足；最大值另由 max_map_err_px 把关（两者互补）。
    pub fn p95_map_err_px(&self) -> u64 {
        let mut errs: Vec<u64> = Vec::new();
        for m in self.traj.newest_first().iter() {
            errs.push(m.err_px() as u64);
        }
        pct_near(&errs, 95)
    }

    /// 持久化编码：byte0 = 加速位，byte1 = 档位下标（设置页 round-trip）。
    pub fn encode(&self) -> [u8; 2] {
        [self.accel as u8, self.tier as u8]
    }

    /// 持久化解码：越界显性拒绝（保持现状），合法则推进版本。
    pub fn decode(&mut self, data: &[u8; 2]) -> bool {
        if data[1] > 3 {
            return false;
        }
        self.set_accel(data[0] & 0x01 == 1);
        self.set_tier(data[1] as usize);
        true
    }
}

impl Default for PointerGov {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F250 自检（判据：1:1 误差 <1px 等；含 xors32 fuzz）。
pub fn run_pointerprec_checks() -> CheckSet {
    let mut set = CheckSet::new("F250-pointerprec");

    // 1. 默认审计：加速关 + 500ms 档（判据两个「默认」逐字核对）。
    let gov = PointerGov::new();
    set.add(
        "defaults: accel off, 500ms tier",
        !gov.accel && gov.tier_ms() == 500 && gov.tier() == DBLCLICK_DEFAULT_TIER,
        "",
    );

    // 2. 直通路径架构位：路由表只有 HID 报文与合成器直通两层，无变换层。
    let layers = PointerGov::route_layers();
    set.add(
        "direct route: no extra layer",
        !PointerGov::extra_layer_present()
            && layers.len() == 2
            && layers[0] == "hid-report"
            && layers[1] == "compositor-direct",
        "",
    );

    // 3. 慢速轨迹 1:1：每步 1px、10ms 间隔，误差恒 0。
    let mut slow = PointerGov::new();
    for k in 0..100u64 {
        let _ = slow.apply_move(1, 0, k * 10);
    }
    set.add(
        "slow trajectory 1:1 err 0",
        slow.traj_len() == 100 && slow.max_map_err_px() < MAP_ERR_LIMIT_PX,
        "",
    );

    // 4. 快速轨迹 1:1：每步 30px、4ms 间隔，误差仍恒 0。
    let mut fast = PointerGov::new();
    for k in 0..100u64 {
        let _ = fast.apply_move(30, -12, k * 4);
    }
    set.add(
        "fast trajectory 1:1 err 0",
        fast.traj_len() == 100 && fast.max_map_err_px() < MAP_ERR_LIMIT_PX,
        "",
    );

    // 5. 加速开：三段增益边界值（0.1/1.0 counts/ms 段界两侧）。
    set.add(
        "accel zone boundaries",
        PointerGov::accel_gain(10) == GAIN_LOW
            && PointerGov::accel_gain(11) == GAIN_MID
            && PointerGov::accel_gain(100) == GAIN_MID
            && PointerGov::accel_gain(101) == GAIN_HIGH,
        "",
    );

    // 6. 加速开：快速移动放大比例大于慢速（曲线形状判定）。
    let mut curve = PointerGov::new();
    curve.set_accel(true);
    let _ = curve.apply_move(1, 0, 0); // 首步直通
    let (sx, _) = curve.apply_move(1, 0, 100); // 速度 0.01 counts/ms → 低段
    let (fx, _) = curve.apply_move(40, 0, 104); // 速度 10 counts/ms → 高段
    set.add(
        "accel scales fast more than slow",
        sx == 1 && fx > 40,
        "",
    );

    // 7. 四档数值表逐字核对（300/400/500/700）。
    set.add(
        "tier table 300/400/500/700",
        DBLCLICK_TIERS_MS == [300, 400, 500, 700],
        "",
    );

    // 8. 边界值判定：间隔 ≤ 档值判双击（500/501 与 300/301 两侧）。
    let mut g5 = PointerGov::new();
    let b1 = g5.classify_click(500) == ClickVerdict::Double
        && g5.classify_click(501) == ClickVerdict::Single;
    g5.set_tier(0); // 300ms 档
    let b2 = g5.classify_click(300) == ClickVerdict::Double
        && g5.classify_click(301) == ClickVerdict::Single;
    set.add("dblclick boundary at tier edges", b1 && b2, "");

    // 9. 测试靶（点气球）：档内两击爆开并回显间隔与档位；超档不爆。
    let mut tgt = PointerGov::new();
    let v1 = tgt.target_click(0);
    let v2 = tgt.target_click(420);
    let v3 = tgt.target_click(1000);
    let v4 = tgt.target_click(1700);
    set.add(
        "balloon target pops in tier",
        v1 == TargetVerdict::Waiting
            && v2 == TargetVerdict::Popped { interval_ms: 420, tier_ms: 500 }
            && v3 == TargetVerdict::Waiting
            && v4 == TargetVerdict::Waiting
            && tgt.balloon_pops == 1,
        "",
    );

    // 10. 设置持久化 round-trip + 越界显性拒绝。
    let mut g6 = PointerGov::new();
    g6.set_accel(true);
    g6.set_tier(3);
    let blob = g6.encode();
    let mut g7 = PointerGov::new();
    let ok = g7.decode(&blob);
    set.add(
        "settings round-trip + reject out of range",
        ok && g7.accel && g7.tier() == 3 && !g7.decode(&[0, 4]),
        "",
    );

    // 11. xors32 fuzz：随机位移流（加速关）——输出恒等于输入、
    //     误差恒 0、轨迹环容量有界、无 panic。
    let mut x: u32 = 0x8532_5A2E;
    let mut fz = PointerGov::new();
    let mut survived = true;
    for k in 0..3000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let dx = ((x >> 8) % 61) as i32 - 30;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let dy = ((x >> 4) % 61) as i32 - 30;
        let (ox, oy) = fz.apply_move(dx, dy, k as u64 * 7);
        if ox != dx || oy != dy || fz.max_map_err_px() != 0 {
            survived = false;
        }
    }
    set.add(
        "fuzz 3000 passthrough moves exact",
        survived && fz.traj_len() <= TRAJ_CAP,
        "",
    );

    // 12. xors32 fuzz：随机开关/档位序列——版本号单调、状态一致、无 panic。
    let mut gz = PointerGov::new();
    let mut prev_ver = gz.version;
    let mut mono = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        gz.set_accel(x & 1 == 1);
        gz.set_tier((x >> 3) as usize % 7); // 故意含越界值 → 钳到 3
        if gz.version < prev_ver {
            mono = false;
        }
        prev_ver = gz.version;
        let t = gz.tier();
        if t > 3 || gz.tier_ms() != DBLCLICK_TIERS_MS[t] {
            mono = false;
        }
    }
    set.add("fuzz settings clamp & version monotonic", mono && gz.tier() <= 3, "");

    // 13. 手感统计与 P95 分位：试验/爆开/未爆三态留痕，命中率与间隔
    //     逐条对账；P95 与最大误差在 1:1 直通下同为 0。
    let mut st = PointerGov::new();
    let _ = st.target_click(0);
    let _ = st.target_click(200); // 爆：200ms
    let _ = st.target_click(1000);
    let _ = st.target_click(1500); // 爆：500ms（边界归双击侧）
    let _ = st.target_click(3000);
    let v6 = st.target_click(3600); // 未爆：600ms > 500ms 档
    let ivals = st.target_intervals();
    let mut pz = PointerGov::new();
    for k in 0..40u64 {
        let _ = pz.apply_move(3, 4, k * 5);
    }
    set.add(
        "target stats rate & interval log & p95",
        v6 == TargetVerdict::Waiting
            && st.target_trials() == 3
            && st.balloon_pops == 2
            && st.double_rate_pct() == 66
            && ivals.len() == 3
            && ivals[0] == 600
            && ivals[1] == 500
            && ivals[2] == 200
            && pz.p95_map_err_px() == 0
            && pz.max_map_err_px() == 0,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_is_byte_exact() {
        let mut g = PointerGov::new();
        for k in 0..50u64 {
            let (ox, oy) = g.apply_move(k as i32 % 7 - 3, -(k as i32 % 5), k * 3);
            assert_eq!(ox, k as i32 % 7 - 3);
            assert_eq!(oy, -(k as i32 % 5));
        }
        assert_eq!(g.max_map_err_px(), 0, "1:1 判据：整数直通误差必须恒 0");
    }

    #[test]
    fn accel_curve_monotone_zones() {
        // 增益沿速度单调不减：低 ≤ 中 ≤ 高。
        let mut prev = 0u32;
        for s in 0..300u32 {
            let g = PointerGov::accel_gain(s);
            assert!(g >= prev);
            prev = g;
        }
        assert_eq!(prev, GAIN_HIGH);
    }

    #[test]
    fn all_four_tiers_boundary() {
        for (i, &ms) in DBLCLICK_TIERS_MS.iter().enumerate() {
            let mut g = PointerGov::new();
            g.set_tier(i);
            assert_eq!(g.tier_ms(), ms);
            assert_eq!(g.classify_click(ms), ClickVerdict::Double, "档值本身判双击");
            assert_eq!(g.classify_click(ms + 1), ClickVerdict::Single, "档值+1 判单击");
        }
    }

    #[test]
    fn balloon_target_alternates() {
        let mut t = PointerGov::new();
        assert_eq!(t.target_click(0), TargetVerdict::Waiting);
        assert_eq!(t.target_click(200), TargetVerdict::Popped { interval_ms: 200, tier_ms: 500 });
        // 爆开后回到等第一击状态。
        assert_eq!(t.target_click(9999), TargetVerdict::Waiting);
        assert_eq!(t.balloon_pops, 1);
    }

    #[test]
    fn traj_ring_wraps_at_cap() {
        let mut g = PointerGov::new();
        for k in 0..(TRAJ_CAP + 50) as u64 {
            let _ = g.apply_move(1, 1, k);
        }
        assert_eq!(g.traj_len(), TRAJ_CAP);
        assert_eq!(g.max_map_err_px(), 0);
    }

    #[test]
    fn target_stats_and_p95() {
        let mut g = PointerGov::new();
        for k in 0..20u64 {
            let _ = g.apply_move(3, 4, k * 5);
        }
        assert_eq!(g.p95_map_err_px(), 0);
        assert_eq!(g.double_rate_pct(), 0, "无试验时命中率为 0 而非 panic");
        // 300ms 档：300ms 边界判双击（爆开），301ms 判单击（不爆）。
        g.set_tier(0);
        let _ = g.target_click(0);
        let _ = g.target_click(300);
        assert_eq!(g.target_trials(), 1);
        assert_eq!(g.double_rate_pct(), 100);
        let ivals = g.target_intervals();
        assert_eq!(ivals.len(), 1);
        assert_eq!(ivals[0], 300);
        let _ = g.target_click(1000);
        let _ = g.target_click(1301);
        assert_eq!(g.target_trials(), 2);
        assert_eq!(g.balloon_pops, 1);
        assert_eq!(g.double_rate_pct(), 50);
    }

    #[test]
    fn pointerprec_selfcheck_all_green() {
        let set = run_pointerprec_checks();
        assert!(set.all_passed(), "F250 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F250（指针精度与双击速度基线）。v2 三件事：
// 1) 持久化 I/O：指针偏好册（加速位 + 双击档位）v2 定长容器序列化——
//    magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验和，四类损坏
//    显性拒绝（与既有 2 字节位包并存于追加段）；
// 2) UI 壳接线：四档选择行清单（档值直取 DBLCLICK_TIERS_MS）+ 行命中
//    测试 + 测试靶气球几何与命中——「设置页带测试靶（点气球）」的
//    几何承载；
// 3) 判定面扩展：run_pointerprec_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：byte0 = bit0 加速位 + bit1-2 档位，byte1..4 保留。
pub const VX2_PP_PAYLOAD: usize = 4;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_PP_BLOB: usize = 9 + VX2_PP_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或编码域越界（档位 > 3）。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 指针偏好册（设置页指针/双击页的持久化数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerPrefsBook {
    pub accel: bool,
    /// 双击速度档下标（0..=3）。
    pub tier: u8,
}

impl PointerPrefsBook {
    pub const fn new() -> PointerPrefsBook {
        PointerPrefsBook { accel: false, tier: DBLCLICK_DEFAULT_TIER as u8 }
    }

    /// 从治理器读出（加速位/档位唯一事实源）。
    pub fn snapshot(g: &PointerGov) -> PointerPrefsBook {
        PointerPrefsBook { accel: g.accel, tier: g.tier() as u8 }
    }

    /// 推到治理器：走既有 set_accel/set_tier 正规路径（钳制 + 版本推进）。
    pub fn apply_to(&self, g: &mut PointerGov) {
        g.set_accel(self.accel);
        g.set_tier(self.tier as usize);
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_PP_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        out[5] = (self.accel as u8) | ((self.tier.min(3)) << 1);
        out[6] = 0;
        out[7] = 0;
        out[8] = 0;
        let crc = vx2_fnv(&out[..9 + VX2_PP_PAYLOAD - 4]);
        out[9 + VX2_PP_PAYLOAD - 4..9 + VX2_PP_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        VX2_PP_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<PointerPrefsBook, Vx2Error> {
        if blob.len() != VX2_PP_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_PP_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        if blob[5] & 0x80 != 0 || blob[5] >> 1 > 3 {
            return Err(Vx2Error::BadLength);
        }
        Ok(PointerPrefsBook { accel: blob[5] & 1 == 1, tier: blob[5] >> 1 })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 档位行高（px）——v2 布局常量：F250 设置页档位行 32px。
pub const VX2_ROW_H_PX: i32 = 32;
/// 测试靶气球半径（px）——点气球的命中面。
pub const VX2_BALLOON_R: i32 = 24;
/// 键盘换档键码（VK_UP/VK_DOWN 同码）。
pub const VX2_KEY_UP: u8 = 0x26;
pub const VX2_KEY_DOWN: u8 = 0x27;

/// 档位行绘制条目：行矩形 + 档值 + 选中徽标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierRow {
    pub tier: usize,
    pub y: i32,
    pub h: i32,
    /// 档值（ms，直取 DBLCLICK_TIERS_MS——档位表唯一事实源）。
    pub tier_ms: u64,
    pub selected: bool,
}

/// 生成四档行清单（行序 = 档位下标序）。
pub fn tier_rows(g: &PointerGov, out: &mut [TierRow]) -> usize {
    let m = 4.min(out.len());
    for t in 0..m {
        out[t] = TierRow {
            tier: t,
            y: t as i32 * VX2_ROW_H_PX,
            h: VX2_ROW_H_PX,
            tier_ms: DBLCLICK_TIERS_MS[t],
            selected: g.tier() == t,
        };
    }
    m
}

/// 行命中测试（页面坐标；x ∈ [0, w) 且落在行内）。
pub fn tier_row_hit(rows: &[TierRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

/// 测试靶气球外接矩形（面板中央；命中域 = 外接方框）。
pub fn balloon_rect(panel_w: i32, panel_h: i32) -> crate::h1star::h1base::Rect {
    crate::h1star::h1base::Rect::new(
        panel_w / 2 - VX2_BALLOON_R,
        panel_h / 2 - VX2_BALLOON_R,
        VX2_BALLOON_R * 2,
        VX2_BALLOON_R * 2,
    )
}

/// 测试靶命中测试。
pub fn balloon_hit(panel_w: i32, panel_h: i32, px: i32, py: i32) -> bool {
    let r = balloon_rect(panel_w, panel_h);
    px >= r.x && px < r.right() && py >= r.y && py < r.bottom()
}

/// 键盘换档：Up 加档 / Down 减档（钳制 0..=3；返回新档位）。
pub fn tier_nav(tier: usize, key: u8) -> usize {
    match key {
        VX2_KEY_UP => (tier + 1).min(3),
        VX2_KEY_DOWN => tier.saturating_sub(1),
        _ => tier,
    }
}

// -- 判定面扩展 ------------------------------------------------------------

/// F250 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_pointerprec_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F250-pointerprec-v2");

    // 1. 持久化 round-trip：偏好册编→解→推新治理器→加速位/档位一致。
    let book = PointerPrefsBook { accel: true, tier: 3 };
    let mut buf = [0u8; VX2_PP_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut gov = PointerGov::new();
    match PointerPrefsBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            b2.apply_to(&mut gov);
            set.add(
                "v2 persistence round-trip",
                b2 == book && gov.accel && gov.tier() == 3 && gov.tier_ms() == 700
                    && gov.version == 2,
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / 翻位与编码域越界）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 2;
    let mut c = buf;
    c[5] ^= 0xFF; // bit7 置位 → 编码域越界
    set.add(
        "v2 corruption explicitly rejected",
        PointerPrefsBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && PointerPrefsBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && PointerPrefsBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && (PointerPrefsBook::from_bytes(&c) == Err(Vx2Error::BadChecksum)
                || PointerPrefsBook::from_bytes(&c) == Err(Vx2Error::BadLength)),
        "",
    );

    // 3. 档位行清单：四行档值逐字对主册 300/400/500/700、选中徽标跟随
    //    当前档、命中测试、键盘换档钳位。
    let mut g3 = PointerGov::new();
    g3.set_tier(2);
    let mut rows = [TierRow { tier: 0, y: 0, h: 0, tier_ms: 0, selected: false }; 4];
    let rn = tier_rows(&g3, &mut rows);
    let mut t = 0usize;
    for _ in 0..6 {
        t = tier_nav(t, VX2_KEY_UP);
    }
    for _ in 0..6 {
        t = tier_nav(t, VX2_KEY_DOWN);
    }
    set.add(
        "v2 tier rows & keyboard nav",
        rn == 4
            && rows.iter().enumerate().all(|(k, r)| r.tier_ms == DBLCLICK_TIERS_MS[k] && r.y == k as i32 * VX2_ROW_H_PX)
            && rows[2].selected && !rows[0].selected
            && tier_row_hit(&rows, rn, 40, 3 * VX2_ROW_H_PX + 4, 300) == Some(3)
            && tier_row_hit(&rows, rn, 40, -1, 300).is_none()
            && t == 0 && tier_nav(3, VX2_KEY_UP) == 3,
        "",
    );

    // 4. 测试靶几何与联动：气球居中、命中界内/界外、档内两击气球爆开
    //    （「点气球验证当前档位」的几何承载）。
    let mut g4 = PointerGov::new();
    let bw = balloon_rect(400, 300);
    let cx = 400 / 2;
    let cy = 300 / 2;
    let _ = g4.target_click(0);
    let popped = g4.target_click(200);
    set.add(
        "v2 balloon geometry & pop",
        bw.x == 200 - VX2_BALLOON_R && bw.w == VX2_BALLOON_R * 2
            && balloon_hit(400, 300, cx, cy)
            && !balloon_hit(400, 300, 0, 0)
            && popped == TargetVerdict::Popped { interval_ms: 200, tier_ms: 500 }
            && g4.balloon_pops == 1,
        "",
    );

    // 5. xors32 fuzz 500 轮：随机偏好册 round-trip 逐字段相等、payload
    //    任一字节翻位必被校验和或编码域捕获。
    let mut x: u32 = 0x250A_F6BC;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let b = PointerPrefsBook { accel: x & 1 == 1, tier: ((x >> 2) % 4) as u8 };
        let mut tbuf = [0u8; VX2_PP_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_PP_BLOB && PointerPrefsBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_PP_PAYLOAD] ^= 0x44;
        ok &= PointerPrefsBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum)
            || PointerPrefsBook::from_bytes(&tbuf) == Err(Vx2Error::BadLength);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_prefs_roundtrip_and_reject() {
        let b = PointerPrefsBook { accel: false, tier: 1 };
        let mut buf = [0u8; VX2_PP_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_PP_BLOB);
        assert_eq!(PointerPrefsBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[5] = 0x86; // bit7 置位（篡改后重算校验和，专测编码域分支）
        let crc = vx2_fnv(&bad[..9 + VX2_PP_PAYLOAD - 4]);
        bad[9 + VX2_PP_PAYLOAD - 4..9 + VX2_PP_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(PointerPrefsBook::from_bytes(&bad), Err(Vx2Error::BadLength));
        let mut bad2 = buf;
        bad2[7] ^= 0x01;
        assert_eq!(PointerPrefsBook::from_bytes(&bad2), Err(Vx2Error::BadChecksum));
    }

    #[test]
    fn v2_tier_rows_selected_tracks_gov() {
        let mut g = PointerGov::new();
        g.set_tier(0);
        let mut rows = [TierRow { tier: 0, y: 0, h: 0, tier_ms: 0, selected: false }; 4];
        let _ = tier_rows(&g, &mut rows);
        assert!(rows[0].selected && !rows[3].selected);
        g.set_tier(3);
        let _ = tier_rows(&g, &mut rows);
        assert!(rows[3].selected && !rows[0].selected);
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_pointerprec_v2_checks();
        assert!(set.all_passed(), "F250 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
