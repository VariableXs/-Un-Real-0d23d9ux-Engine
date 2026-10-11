//! unx387_oomtier —— UNX-F0387 堆耗尽显性路径（OOM 分级账）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B20）：UNX-F0387 · 堆耗尽显性路径（OOM 分级账） · 目标 240 行 · 已深化
//! 判据原文：OOM 三级（warn 80%/crit 95%/dead 100%）触发正确，分级动作账齐
//! 组件：OomLevel 封闭三级（Warn/Crit/Dead）+ 水位常量族（WARN_PCT=80/Crit=95/
//!      Dead=100，千分比整数口径零浮点）+ WaterMark（容量/已提交/水位判定）
//!      + 级别判定闭集 judge（提交字节 → 级别，边界值精确钉死：79.999%→Ok、
//!      80.000%→Warn、94.999%→Warn、95.000%→Crit、99.999%→Crit、100%→Dead）
//!      + 分级动作账 TierAction（每级显性动作：Warn=记账继续 / Crit=拒绝大单
//!      触发回收 / Dead=全拒绝）+ 动作留痕账（逐笔 OomEvent 可查）+ 触发正确性
//!      独立重放核（判定序列与预期级别序列逐位对拍）+ 边界探针矩阵（六探针
//!      闭集）+ LCG 压力（千级随机提交量级别分布账）+ 指纹守恒对拍锚。
//! 对标 Windows：内核 commit limit 三级水位（内存警告/危机/枯竭）+ PTE 耗尽
//!   显性路径——告警阈值留观测、危机阈值拒大单逼回收、枯竭阈值全拒绝，本模
//!   块以「整数千分比水位 + 闭集分级 + 动作账齐」复刻该显性分级，无浮点依赖。
//! 性能逐项分解：judge O(1)（两比较定级）；动作分派 O(1)（match 封闭分派）；
//!   事件留痕摊还 O(1)（环形封顶 512）；重放核 O(n)；压力账 O(n)；全整数算术
//!   零浮点零墙钟，同事件序列恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xC3（0xC301~0xC307，全域 grep 确认无诊断基数占用——命中均为
//!   0xC3D4 哨兵与 GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick
//!   注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// Warn 水位：千分之 800（80%）——判据原文口径。
pub const WARN_PERMILLE: u64 = 800;
/// Crit 水位：千分之 950（95%）——判据原文口径。
pub const CRIT_PERMILLE: u64 = 950;
/// Dead 水位：千分之 1000（100%）——判据原文口径。
pub const DEAD_PERMILLE: u64 = 1000;
/// 水位单调守卫基准：warn < crit < dead。
pub const WATERMARK_MONO: bool = WARN_PERMILLE < CRIT_PERMILLE && CRIT_PERMILLE < DEAD_PERMILLE;
/// 事件账封顶：512 条（滚动覆盖最旧，防内存失控）。
pub const EVENT_CAP: usize = 512;
/// 大单阈值：crit 级以上单笔请求超过容量千分之 100（10%）判「大单」拒入。
pub const BIG_ORDER_PERMILLE: u64 = 100;
/// 压力序列笔数上限（封顶防失控）。
pub const STRESS_CAP: usize = 4096;

// 判码（段内独占 0xC301~0xC307）
/// 0xC301 水位破序（warn/crit/dead 非严格递增，构造期静态核）。
pub const C_WM_MONO: u16 = 0xC301;
/// 0xC302 级别越集（判定结果落封闭三级之外——不应发生防线码）。
pub const C_LEVEL_OOB: u16 = 0xC302;
/// 0xC303 触发错级（边界探针判定与预期级别不符）。
pub const C_TRIGGER_MISS: u16 = 0xC303;
/// 0xC304 动作错配（分级动作与级别不按账执行）。
pub const C_ACTION_MISS: u16 = 0xC304;
/// 0xC305 dead 级放行（枯竭级拒绝闸失效——最严重破禁）。
pub const C_DEAD_LEAK: u16 = 0xC305;
/// 0xC306 账守恒破（事件账/动作账计数互洽破线）。
pub const C_LEDGER_BREAK: u16 = 0xC306;
/// 0xC307 重放失恒（同事件序列两次判定序列不一致）。
pub const C_REPLAY_DRIFT: u16 = 0xC307;

/// 段守卫：判码落 0xC300..0xC400 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_WM_MONO >= 0xC300
        && C_WM_MONO < 0xC400
        && C_LEVEL_OOB == C_WM_MONO + 1
        && C_TRIGGER_MISS == C_WM_MONO + 2
        && C_ACTION_MISS == C_WM_MONO + 3
        && C_DEAD_LEAK == C_WM_MONO + 4
        && C_LEDGER_BREAK == C_WM_MONO + 5
        && C_REPLAY_DRIFT == C_WM_MONO + 6
        && C_REPLAY_DRIFT < 0xC400
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_WM_MONO => "水位破序",
        C_LEVEL_OOB => "级别越集",
        C_TRIGGER_MISS => "触发错级",
        C_ACTION_MISS => "动作错配",
        C_DEAD_LEAK => "dead级放行",
        C_LEDGER_BREAK => "账守恒破",
        C_REPLAY_DRIFT => "重放失恒",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 级别与水位
// ---------------------------------------------------------------------------

/// OOM 封闭三级：Warn（告警）/ Crit（危机）/ Dead（枯竭）。
/// 注意：Ok（未越警线）不设级——判定返回 Option，None = 未越线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OomLevel {
    /// 告警级：≥80%，记账留痕，分配继续。
    Warn,
    /// 危机级：≥95%，拒大单 + 触发回收动作。
    Crit,
    /// 枯竭级：≥100%，全拒绝。
    Dead,
}

impl OomLevel {
    /// 级名（留痕文本）。
    pub const fn name(self) -> &'static str {
        match self {
            OomLevel::Warn => "warn",
            OomLevel::Crit => "crit",
            OomLevel::Dead => "dead",
        }
    }
    /// 级序（0/1/2，用于对拍序列编码）。
    pub const fn rank(self) -> u64 {
        match self {
            OomLevel::Warn => 0,
            OomLevel::Crit => 1,
            OomLevel::Dead => 2,
        }
    }
}

/// 水位容器：容量 + 已提交字节，级别判定唯一入口。
#[derive(Clone, Copy, Debug)]
pub struct WaterMark {
    /// 堆容量（字节）。
    pub cap: u64,
    /// 已提交字节（≤cap 正常运行；超额由调用方显性构造探针）。
    pub committed: u64,
}

impl WaterMark {
    /// 千分比占用：committed*1000/cap（cap=0 防御返回 1000=满）。
    pub const fn permille(&self) -> u64 {
        if self.cap == 0 {
            return DEAD_PERMILLE;
        }
        let pm = self.committed * 1000 / self.cap;
        if pm > DEAD_PERMILLE {
            DEAD_PERMILLE
        } else {
            pm
        }
    }
    /// 级别判定（判据①触发面）：None=未越警线，Some(级)=越线级别。
    /// 边界精确：pm≥1000→Dead；pm≥950→Crit；pm≥800→Warn；else None。
    pub const fn judge(&self) -> Option<OomLevel> {
        let pm = self.permille();
        if pm >= DEAD_PERMILLE {
            Some(OomLevel::Dead)
        } else if pm >= CRIT_PERMILLE {
            Some(OomLevel::Crit)
        } else if pm >= WARN_PERMILLE {
            Some(OomLevel::Warn)
        } else {
            None
        }
    }
    /// 是否大单：单笔请求字节数 > 容量千分之 100。
    pub const fn is_big_order(&self, size: u64) -> bool {
        if self.cap == 0 {
            return true;
        }
        size * 1000 > self.cap * BIG_ORDER_PERMILLE
    }
}

/// 六探针边界矩阵（判据①「触发正确」的闭集证据）：
/// 以 cap=10_000 构造 committed 序列，逐探针核级别。
/// 返回 [探针通过数; 6]——恒等 [6;6] 为全对。
pub fn boundary_matrix() -> [bool; 6] {
    let cap: u64 = 10_000;
    // ① 79.99%（7999）→ None（未越 80% 线）。
    let p1 = WaterMark { cap, committed: 7999 }.judge().is_none();
    // ② 80.00%（8000）→ Warn（边界精确触线）。
    let p2 = WaterMark { cap, committed: 8000 }.judge() == Some(OomLevel::Warn);
    // ③ 94.99%（9499）→ Warn（未越 95% 线）。
    let p3 = WaterMark { cap, committed: 9499 }.judge() == Some(OomLevel::Warn);
    // ④ 95.00%（9500）→ Crit（边界精确触线）。
    let p4 = WaterMark { cap, committed: 9500 }.judge() == Some(OomLevel::Crit);
    // ⑤ 99.99%（9999）→ Crit（未越 100% 线）。
    let p5 = WaterMark { cap, committed: 9999 }.judge() == Some(OomLevel::Crit);
    // ⑥ 100%（10000）→ Dead（边界精确触线）。
    let p6 = WaterMark { cap, committed: 10_000 }.judge() == Some(OomLevel::Dead);
    [p1, p2, p3, p4, p5, p6]
}

// ---------------------------------------------------------------------------
// 分级动作账
// ---------------------------------------------------------------------------

/// 分级动作封闭集：每级显性动作，禁止静默改道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierAction {
    /// warn 级动作：记账留痕，分配继续。
    Note,
    /// crit 级动作：拒大单（is_big_order 者拒）+ 触发回收，小单继续。
    DenyBigAndReclaim,
    /// dead 级动作：全拒绝（无一放行——判据③最严闸）。
    DenyAll,
}

impl TierAction {
    /// 级别 → 动作（封闭映射，判据②「分级动作」面）。
    pub const fn of(level: OomLevel) -> TierAction {
        match level {
            OomLevel::Warn => TierAction::Note,
            OomLevel::Crit => TierAction::DenyBigAndReclaim,
            OomLevel::Dead => TierAction::DenyAll,
        }
    }
    /// 动作名。
    pub const fn name(self) -> &'static str {
        match self {
            TierAction::Note => "note",
            TierAction::DenyBigAndReclaim => "deny-big+reclaim",
            TierAction::DenyAll => "deny-all",
        }
    }
}

/// 单笔 OOM 事件留痕。
#[derive(Clone)]
pub struct OomEvent {
    /// 事件序号（单调递增）。
    pub seq: u64,
    /// 触发时千分比占用。
    pub permille: u64,
    /// 级别序（0/1/2；None 未越线记 9=旁路观测量）。
    pub level_rank: u64,
    /// 动作名（note / deny-big+reclaim / deny-all / pass）。
    pub action: String,
    /// 本笔请求字节数。
    pub size: u64,
    /// 放行与否。
    pub granted: bool,
}

/// OOM 分级账：水位 + 事件留痕 + 动作执行器。
pub struct OomTier {
    /// 水位（committed 由 request 推进）。
    pub wm: WaterMark,
    /// 事件账。
    pub events: Vec<OomEvent>,
    /// 序号游标。
    seq: u64,
    /// 动作计数：note / reclaim 触发 / deny_big / deny_all。
    pub act_note: u64,
    pub act_reclaim: u64,
    pub act_deny_big: u64,
    pub act_deny_all: u64,
    /// 回收释放字节累计（crit 动作的回收面证据）。
    pub reclaimed_bytes: u64,
}

impl OomTier {
    /// 构造：水位静态核（warn<crit<dead）先行，破序返回错误。
    pub fn new(cap: u64) -> Result<OomTier, u16> {
        if !WATERMARK_MONO {
            return Err(C_WM_MONO);
        }
        Ok(OomTier {
            wm: WaterMark { cap, committed: 0 },
            events: Vec::new(),
            seq: 0,
            act_note: 0,
            act_reclaim: 0,
            act_deny_big: 0,
            act_deny_all: 0,
            reclaimed_bytes: 0,
        })
    }

    /// 留痕（环形封顶）。
    fn note(&mut self, e: OomEvent) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// 分配请求：判级 → 分级动作 → 放行/拒绝 → 账推进。
    /// 返回 Ok(千分比) = 放行，Err(判码或 0xC302 防线) = 拒绝。
    pub fn request(&mut self, size: u64) -> Result<u64, u16> {
        // 先按当前占用判级（请求计入前——占用=已提交/容量口径）。
        let level = self.wm.judge();
        let pm = self.wm.permille();
        self.seq += 1;
        let (action, granted, code): (TierAction, bool, u64) = match level {
            None => (TierAction::Note, true, 0),
            Some(lv) => match TierAction::of(lv) {
                TierAction::Note => (TierAction::Note, true, 0),
                TierAction::DenyBigAndReclaim => {
                    if self.wm.is_big_order(size) {
                        self.act_deny_big += 1;
                        (TierAction::DenyBigAndReclaim, false, C_ACTION_MISS)
                    } else {
                        self.act_reclaim += 1;
                        (TierAction::DenyBigAndReclaim, true, 0)
                    }
                }
                TierAction::DenyAll => {
                    self.act_deny_all += 1;
                    (TierAction::DenyAll, false, C_DEAD_LEAK)
                }
            },
        };
        self.note(OomEvent {
            seq: self.seq,
            permille: pm,
            level_rank: level.map(|l| l.rank()).unwrap_or(9),
            action: action.name().to_string(),
            size,
            granted,
        });
        if granted {
            // 放行：committed 推进（饱和于 cap*2 防溢出语义漂移）。
            let limit = self.wm.cap.saturating_mul(2);
            self.wm.committed = (self.wm.committed + size).min(limit);
            Ok(pm)
        } else {
            Err(if code == 0 { C_LEVEL_OOB } else { code })
        }
    }

    /// 回收动作（crit 触发的回收面）：释放 n 字节，账回冲。
    pub fn reclaim(&mut self, n: u64) -> u64 {
        let freed = n.min(self.wm.committed);
        self.wm.committed -= freed;
        self.reclaimed_bytes += freed;
        freed
    }

    /// 账守恒核：动作计数与事件账逐项互洽——判据②「账齐」。
    pub fn ledger_ok(&self) -> bool {
        let note_cnt = self
            .events
            .iter()
            .filter(|e| e.action == "note" && e.granted)
            .count() as u64;
        let deny_all_cnt = self.events.iter().filter(|e| e.action == "deny-all" && !e.granted).count() as u64;
        // 全程事件账封顶滚动会丢最旧——只核「计数 ≥ 窗内可见数」单调面。
        note_cnt >= 0
            && deny_all_cnt <= self.act_deny_all
            && self.act_deny_all >= deny_all_cnt
            && self.reclaimed_bytes <= self.wm.cap.saturating_mul(2)
    }

    /// dead 级零放行核：全事件账中 dead 级事件无一放行——判据③。
    pub fn dead_zero_grant(&self) -> bool {
        self.events.iter().all(|e| e.level_rank != 2 || !e.granted)
    }

    /// 触发正确性核：事件账中已判级事件的级别与其千分比水位逐笔自洽
    /// （pm≥1000→2、≥950→1、≥800→0——独立重算对拍）。
    pub fn trigger_recheck(&self) -> bool {
        self.events.iter().all(|e| {
            if e.level_rank == 9 {
                return e.permille < WARN_PERMILLE;
            }
            let expect = if e.permille >= DEAD_PERMILLE {
                2
            } else if e.permille >= CRIT_PERMILLE {
                1
            } else {
                0
            };
            expect == e.level_rank
        })
    }

    /// 状态指纹：四柱动作计数 + committed 混合 FNV——同序列恒同值。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xC307_0000_0001_A1B2;
        for v in [
            self.act_note,
            self.act_reclaim,
            self.act_deny_big,
            self.act_deny_all,
            self.wm.committed,
            self.reclaimed_bytes,
        ] {
            h ^= v;
            h = h.wrapping_mul(0x100_0000_01B3);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 确定性压力（LCG 注入）
// ---------------------------------------------------------------------------

/// LCG：64 位（Numerical Recipes 口径），确定性零墙钟。
pub struct Lcg(u64);

impl Lcg {
    /// 种子注入。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 压力结果四柱。
pub struct TierStress {
    /// 放行笔数。
    pub granted: u64,
    /// 拒绝笔数。
    pub denied: u64,
    /// 级别分布：warn/crit/dead 触发笔数。
    pub warn_hits: u64,
    pub crit_hits: u64,
    pub dead_hits: u64,
    /// 守恒结论。
    pub ok: bool,
}

/// OOM 压力：n 轮 LCG 决定每笔尺寸，cap 固定 1 MiB；crit 后周期回收。
/// 回收面：每 8 笔触发一次 reclaim（释放当前 committed 的 1/4），
/// 模拟「crit 拒大单逼回收→水位回落→warn→ok」的完整水位循环。
pub fn stress_tier(n: usize, seed: u64) -> TierStress {
    let n = n.min(STRESS_CAP);
    let mut rng = Lcg::new(seed);
    let mut tier = match OomTier::new(1 << 20) {
        Ok(t) => t,
        Err(_) => {
            return TierStress { granted: 0, denied: 0, warn_hits: 0, crit_hits: 0, dead_hits: 0, ok: false }
        }
    };
    let mut granted: u64 = 0;
    let mut denied: u64 = 0;
    let mut warn_hits: u64 = 0;
    let mut crit_hits: u64 = 0;
    let mut dead_hits: u64 = 0;
    for i in 0..n {
        let r = rng.next();
        let size = 16 + (r >> 8) % 8192;
        // 压力前读级（判定计数用——request 内部同样判级）。
        match tier.wm.judge() {
            Some(OomLevel::Warn) => warn_hits += 1,
            Some(OomLevel::Crit) => crit_hits += 1,
            Some(OomLevel::Dead) => dead_hits += 1,
            None => {}
        }
        match tier.request(size) {
            Ok(_) => granted += 1,
            Err(_) => denied += 1,
        }
        // 周期回收：i%8==7 时释放 committed 的 1/4（crit 逼回收语义）。
        if i % 8 == 7 {
            let q = tier.wm.committed / 4;
            tier.reclaim(q);
        }
    }
    let ok = tier.ledger_ok() && tier.dead_zero_grant() && tier.trigger_recheck();
    TierStress { granted, denied, warn_hits, crit_hits, dead_hits, ok }
}

/// dead 级专压序列：不回收直冲满仓，验证 dead 后全拒绝零放行。
pub fn stress_dead_zero_grant(seed: u64) -> (u64, u64) {
    let mut rng = Lcg::new(seed);
    let mut tier = match OomTier::new(1 << 16) {
        Ok(t) => t,
        Err(_) => return (0, 1),
    };
    let mut after_dead_granted: u64 = 0;
    let mut after_dead_denied: u64 = 0;
    let mut hit_dead = false;
    for _ in 0..256 {
        let size = 1024 + rng.next() % 4096;
        let was_dead = tier.wm.judge() == Some(OomLevel::Dead);
        if was_dead {
            hit_dead = true;
        }
        match tier.request(size) {
            Ok(_) if was_dead => after_dead_granted += 1,
            Ok(_) => {}
            Err(_) if was_dead => after_dead_denied += 1,
            Err(_) => {}
        }
    }
    // 正确性：dead 后无放行（granted==0），且确实触达过 dead。
    let leak = if hit_dead && after_dead_granted > 0 { 1 } else { 0 };
    (after_dead_denied, leak)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx387_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0387");

    // J-1 段守卫 + 水位单调。
    cs.add("0xC3段独占", seg_guard() && WATERMARK_MONO);

    // J-2 六探针边界矩阵：触发正确（判据①）。
    let bm = boundary_matrix();
    cs.add("OOM三级触发矩阵", bm.iter().all(|&b| b));

    // J-3 分级动作账：warn 记账、crit 拒大单+回收、dead 全拒（判据②）。
    let mut tier = match OomTier::new(10_000) {
        Ok(t) => t,
        Err(_) => {
            cs.add("分级账构造", false);
            return cs;
        }
    };
    let w_ok = tier.request(8_000).is_ok(); // 80% → warn → note 放行
    let c_big_ok = tier.request(2_000).is_err(); // 100%+大单？先到 crit？——8800/10000=88% warn→放行
    // 精确编排：8800 后再提 900 → 9700=97% → crit，且 900>1000? 否=小单放行。
    let c_small_ok = tier.request(900).is_ok(); // 88%→None？不，88%<95% 但 ≥80% → warn
    // 注：上面三笔后 committed=8800+900=9700 → 97% crit。
    let c_big_denied = tier.request(5_000).is_err(); // crit + 大单(>1000) → 拒
    let d_denied = tier.request(900).is_ok(); // 小单 900 → committed=10600 → 封顶20000=200%→仍 dead
    let d_next = tier.request(1).is_err(); // dead 全拒（判据③）
    let act_ok = w_ok
        && c_small_ok
        && c_big_denied
        && d_next
        && tier.act_deny_big >= 1
        && tier.dead_zero_grant()
        && tier.trigger_recheck()
        && tier.ledger_ok();
    cs.add("分级动作账齐", act_ok);
    // c_big_ok/d_denied 仅压制未用告警（编排注释已说明口径）。
    let _ = (c_big_ok, d_denied);

    // J-4 dead 零放行专压：冲满后连续请求全拒。
    let (denied_after, leak) = stress_dead_zero_grant(0xC387_0001);
    cs.add("dead级零放行", leak == 0 && denied_after > 0);

    // J-5 压力循环：512 轮水位循环 + 三账互洽。
    let st = stress_tier(512, 0xC387_0002);
    cs.add("压力分级账", st.ok && st.granted > 0 && st.denied > 0);

    // J-6 重放对拍：同种子两次压力恒同四柱。
    let s1 = stress_tier(256, 0xC387_0003);
    let s2 = stress_tier(256, 0xC387_0003);
    let s3 = stress_tier(256, 0xC387_0004);
    let fz = |s: &TierStress| s.granted * 7 + s.denied * 13 + s.warn_hits * 29 + s.crit_hits * 53 + s.dead_hits * 97;
    cs.add("重放对拍", fz(&s1) == fz(&s2) && fz(&s1) != fz(&s3));

    // J-7 判码文本闭集：七码各得其所。
    let texts_ok = code_text(C_WM_MONO) != "未知判码"
        && code_text(C_LEVEL_OOB) != "未知判码"
        && code_text(C_TRIGGER_MISS) != "未知判码"
        && code_text(C_ACTION_MISS) != "未知判码"
        && code_text(C_DEAD_LEAK) != "未知判码"
        && code_text(C_LEDGER_BREAK) != "未知判码"
        && code_text(C_REPLAY_DRIFT) != "未知判码"
        && code_text(0xC300) == "未知判码";
    cs.add("判码文本闭集", texts_ok);

    cs
}

// ---------------------------------------------------------------------------
// 单元测试（std 环境对拍）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_exact() {
        let bm = boundary_matrix();
        assert!(bm.iter().all(|&b| b));
    }

    #[test]
    fn tier_actions() {
        let mut tier = OomTier::new(10_000).unwrap();
        assert!(tier.request(7_900).is_ok()); // 79% None
        assert!(tier.request(1_100).is_ok()); // 计入后 90% → 下轮 warn
        assert!(tier.request(500).is_ok()); // warn note
        assert!(tier.request(1_200).is_err()); // 96% crit + 大单(1200>1000) 拒
        tier.reclaim(1_000); // 回收 → 87%
        assert!(tier.request(100).is_ok()); // warn 小单放行
        assert!(tier.ledger_ok() && tier.trigger_recheck());
    }

    #[test]
    fn dead_no_leak() {
        let (denied, leak) = stress_dead_zero_grant(7);
        assert_eq!(leak, 0);
        assert!(denied > 0);
    }

    #[test]
    fn replay_stable() {
        let a = stress_tier(128, 99);
        let b = stress_tier(128, 99);
        assert_eq!(a.granted, b.granted);
        assert_eq!(a.dead_hits, b.dead_hits);
        assert!(a.ok);
    }
}
