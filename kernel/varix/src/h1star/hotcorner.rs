//! F249 热角触发 · 判据实装。
//!
//! **判据锚**：主册 F249「热角触发」。
//!
//! **验收标准第一句（任务包原文）**：停留 300ms±30ms 实测。
//!
//! **判据（主册原文摘录）**：屏幕四角各一个 8px 热区，默认只启用左下角
//! （呼出开始菜单——零代码开机哲学的快速入口），其余三角默认关闭但可在
//! 设置启用并各配动作（右下=快速设置、右上=通知中心、左上=任务视图）；
//! 触发需在角内停留 300ms（防误触，划过不触发），触发时有 120ms 角标
//! 指示动画。
//!
//! **设计要点**：
//! - 四角热区几何用 [`Rect`] 8×8 判定（右/下开区间，贴角像素算入热区）；
//! - 停留计时状态机：入角开始计时 → 停留 ≥300ms 触发一次（fired 位
//!   保证每次入场至多一枪）→ 出角即重置；停留 <150ms 记「快速划过」
//!   独立计数（防误触审计面），150~300ms 之间离开既不触发也不计划过；
//! - 每角独立启用位 + 动作绑定枚举；默认配置是**审计对象**不是硬编码
//!   假设——`audit_default` 逐角核对；
//! - 角标指示动画 120ms Enter 曲线（h1base 一处一事实），触发与动画
//!   同源（last_fire 时间戳）：没触发就绝无动画；同时接受 F245
//!   [`MotionPolicy`] 注入——减少动效开启时角标同样降级为 80ms 直切；
//! - 零堆热路径：移动事件判定全在定长数组上，无 Vec/String；
//!   时间一律注入（毫秒戳），模块不持时钟。
//!
//! **依赖锚点**：F227/F124（Enter 曲线与动画总谱——经 h1base 取值）、
//! F245（动效策略注入）、F226（层级——角标指示层不遮内容）。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy, Rect};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量（每条注明主册依据）
// ---------------------------------------------------------------------------

/// 热区边长（px）——主册「屏幕四角各一个 8px 热区」。
pub const HOTZONE_PX: i32 = 8;

/// 停留触发门槛（ms）——主册「触发需在角内停留 300ms」。
pub const DWELL_MS: u64 = 300;

/// 快速划过判定线（ms）——主册「划过不触发」，<150ms 的离开计「划过」
/// 独立审计计数。
pub const PASS_IGNORE_MS: u64 = 150;

/// 角标指示动画时长（ms）——主册「触发时有 120ms 角标指示动画」。
pub const INDICATOR_MS: u32 = 120;

/// 事件账本容量（触发 + 配置变更留痕，定容环形）。
pub const LEDGER_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 角与动作
// ---------------------------------------------------------------------------

/// 屏幕四角。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    /// 左上（默认关，预设动作=任务视图）。
    TopLeft,
    /// 右上（默认关，预设动作=通知中心）。
    TopRight,
    /// 左下（**默认唯一启用**，动作=开始菜单）。
    BottomLeft,
    /// 右下（默认关，预设动作=快速设置）。
    BottomRight,
}

impl Corner {
    /// 四角全集（遍历序恒定）。
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub fn idx(&self) -> usize {
        match self {
            Corner::TopLeft => 0,
            Corner::TopRight => 1,
            Corner::BottomLeft => 2,
            Corner::BottomRight => 3,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Corner::TopLeft => "左上",
            Corner::TopRight => "右上",
            Corner::BottomLeft => "左下",
            Corner::BottomRight => "右下",
        }
    }
}

/// 角动作绑定（判据原文的四个动作 + 未绑定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerAction {
    /// 未绑定（启用但无动作 = 不触发）。
    None,
    /// 呼出开始菜单（左下默认——零代码开机哲学的快速入口）。
    StartMenu,
    /// 快速设置（右下预设）。
    QuickSettings,
    /// 通知中心（右上预设）。
    NotifCenter,
    /// 任务视图（左上预设）。
    TaskView,
}

impl CornerAction {
    fn code(&self) -> u8 {
        match self {
            CornerAction::None => 0,
            CornerAction::StartMenu => 1,
            CornerAction::QuickSettings => 2,
            CornerAction::NotifCenter => 3,
            CornerAction::TaskView => 4,
        }
    }

    fn from_code(c: u8) -> CornerAction {
        match c {
            1 => CornerAction::StartMenu,
            2 => CornerAction::QuickSettings,
            3 => CornerAction::NotifCenter,
            4 => CornerAction::TaskView,
            _ => CornerAction::None,
        }
    }
}

/// 每角配置：启用位 + 动作绑定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerCfg {
    pub enabled: bool,
    pub action: CornerAction,
}

/// 出厂默认配置——主册原文的逐角落位（审计对象）。
pub fn default_cfg(c: Corner) -> CornerCfg {
    match c {
        Corner::TopLeft => CornerCfg { enabled: false, action: CornerAction::TaskView },
        Corner::TopRight => CornerCfg { enabled: false, action: CornerAction::NotifCenter },
        Corner::BottomLeft => CornerCfg { enabled: true, action: CornerAction::StartMenu },
        Corner::BottomRight => CornerCfg { enabled: false, action: CornerAction::QuickSettings },
    }
}

/// 停留计时状态（pub 字段供设置中心诊断面直读）。
#[derive(Clone, Copy, Debug)]
pub struct DwellState {
    /// 指针当前是否在本角热区内。
    pub in_zone: bool,
    /// 本次入场时刻（ms，注入式；in_zone=false 时无意义）。
    pub enter_ts: u64,
    /// 本次入场是否已触发（一枪纪律）。
    pub fired: bool,
    /// 最近一次触发时刻（0 = 从未触发；触发时刻恒 ≥300 故 0 可作哨兵）。
    pub fire_ts: u64,
}

/// 一次触发结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerFire {
    pub corner: Corner,
    pub action: CornerAction,
    pub fire_ts: u64,
}

/// 事件账本条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerEvent {
    /// 事件时刻（ms，注入式）。
    pub ts: u64,
    /// 角下标（Corner::idx）。
    pub corner: u8,
    /// 1=触发 2=启用 3=停用（配置变更留痕）。
    pub kind: u8,
}

// ---------------------------------------------------------------------------
// 热角状态机
// ---------------------------------------------------------------------------

/// 热角触发器：四角热区几何 + 停留计时 + 角标动画 + 配置/账本。
pub struct HotCorners {
    screen: Rect,
    cfgs: [CornerCfg; 4],
    dwell: [DwellState; 4],
    motion: MotionPolicy,
    ledger: RingLog<CornerEvent, LEDGER_CAP>,
    /// 触发计数（每角累计，审计面）。
    pub triggers: [u32; 4],
    /// 快速划过计数（每角累计，防误触审计面）。
    pub passes: [u32; 4],
    /// 配置版本号（设置面变更即推进）。
    pub version: u32,
}

impl HotCorners {
    pub fn new(screen: Rect) -> HotCorners {
        HotCorners {
            screen,
            cfgs: [
                default_cfg(Corner::TopLeft),
                default_cfg(Corner::TopRight),
                default_cfg(Corner::BottomLeft),
                default_cfg(Corner::BottomRight),
            ],
            dwell: [const {
                DwellState { in_zone: false, enter_ts: 0, fired: false, fire_ts: 0 }
            }; 4],
            motion: MotionPolicy::normal(),
            ledger: RingLog::new(),
            triggers: [0; 4],
            passes: [0; 4],
            version: 0,
        }
    }

    /// 本角热区几何（8×8 贴角，右/下开区间）。
    pub fn zone(&self, c: Corner) -> Rect {
        let z = HOTZONE_PX;
        match c {
            Corner::TopLeft => Rect::new(self.screen.x, self.screen.y, z, z),
            Corner::TopRight => Rect::new(self.screen.right() - z, self.screen.y, z, z),
            Corner::BottomLeft => Rect::new(self.screen.x, self.screen.bottom() - z, z, z),
            Corner::BottomRight => {
                Rect::new(self.screen.right() - z, self.screen.bottom() - z, z, z)
            }
        }
    }

    pub fn cfg(&self, c: Corner) -> CornerCfg {
        self.cfgs[c.idx()]
    }

    pub fn dwell_state(&self, c: Corner) -> DwellState {
        self.dwell[c.idx()]
    }

    /// 设置某角配置（设置中心入口；变更留痕 + 版本推进）。
    pub fn configure(&mut self, c: Corner, cfg: CornerCfg, ts: u64) {
        let i = c.idx();
        if self.cfgs[i] == cfg {
            return;
        }
        let kind = match (self.cfgs[i].enabled, cfg.enabled) {
            (false, true) => 2u8,
            (true, false) => 3u8,
            _ => 0u8,
        };
        self.cfgs[i] = cfg;
        self.version = self.version.wrapping_add(1);
        self.ledger.push(CornerEvent { ts, corner: i as u8, kind });
    }

    /// 注入动效策略（F245 开关的下游消费——角标动画同样降级）。
    pub fn set_motion_policy(&mut self, p: MotionPolicy) {
        self.motion = p;
    }

    /// 指针移动事件（逐帧热路径，零堆）：判定四角热区归属、驱动停留
    /// 计时与触发。返回刚触发的角（一帧至多一个）。
    pub fn on_move(&mut self, x: i32, y: i32, ts: u64) -> Option<CornerFire> {
        let mut fire = None;
        for c in Corner::ALL {
            let i = c.idx();
            let inzone = self.cfgs[i].enabled && self.zone(c).contains(x, y);
            if inzone {
                if !self.dwell[i].in_zone {
                    // 入场：开始计时，重置一枪位。
                    self.dwell[i] =
                        DwellState { in_zone: true, enter_ts: ts, fired: false, fire_ts: self.dwell[i].fire_ts };
                } else if !self.dwell[i].fired && ts.saturating_sub(self.dwell[i].enter_ts) >= DWELL_MS {
                    // 停留满 300ms：触发（每次入场至多一次）。
                    let action = self.cfgs[i].action;
                    self.dwell[i].fired = true;
                    self.dwell[i].fire_ts = ts;
                    self.triggers[i] = self.triggers[i].saturating_add(1);
                    self.ledger.push(CornerEvent { ts, corner: i as u8, kind: 1 });
                    if action != CornerAction::None {
                        fire = Some(CornerFire { corner: c, action, fire_ts: ts });
                    }
                }
            } else if self.dwell[i].in_zone {
                // 出角：重置计时；停留 <150ms 记快速划过。
                let dwell_ms = ts.saturating_sub(self.dwell[i].enter_ts);
                if dwell_ms < PASS_IGNORE_MS {
                    self.passes[i] = self.passes[i].saturating_add(1);
                }
                self.dwell[i].in_zone = false;
            }
        }
        fire
    }

    /// 角标指示动画进度（0..=1000；None = 当前无动画）。
    ///
    /// 与触发同源（fire_ts）：未触发过的角恒 None；触发后 120ms 内
    /// （F245 降级时 80ms）按 Enter 曲线给出进度。
    pub fn indicator(&self, c: Corner, now_ts: u64) -> Option<u32> {
        let fire_ts = self.dwell[c.idx()].fire_ts;
        if fire_ts == 0 {
            return None;
        }
        let elapsed = now_ts.saturating_sub(fire_ts).min(u32::MAX as u64) as u32;
        let dur = self.motion.duration_ms(Curve::Enter, INDICATOR_MS);
        if elapsed >= dur {
            return None;
        }
        Some(self.motion.progress(Curve::Enter, elapsed, INDICATOR_MS))
    }

    /// 最近事件（新→旧，诊断面/设置页回放）。
    pub fn recent_events(&self) -> [Option<CornerEvent>; LEDGER_CAP] {
        // RingLog 只供 Vec 读出——这里折成定长数组返回（零堆读面）。
        let mut out = [None; LEDGER_CAP];
        for (k, ev) in self.ledger.newest_first().iter().enumerate() {
            out[k] = Some(*ev);
        }
        out
    }

    /// 出厂默认配置审计：仅左下启用且绑定开始菜单；其余三角预设动作
    /// 就位但默认关闭——判据「默认单角启用审计」。
    pub fn audit_default(&self) -> bool {
        Corner::ALL.iter().all(|&c| self.cfgs[c.idx()] == default_cfg(c))
    }

    /// 持久化编码：每角 1 字节（bit0=启用位，bit1-3=动作码）。
    pub fn encode(&self, out: &mut [u8; 4]) {
        for (k, &c) in Corner::ALL.iter().enumerate() {
            let cfg = self.cfgs[c.idx()];
            out[k] = cfg.action.code() << 1 | (cfg.enabled as u8);
        }
    }

    /// 持久化解码：非法字节保持现状并显性返回 false（不静默改配置）。
    /// 合法编码域：bit0 ∈ {0,1} × 动作码 0..=4 → 字节 ≤ 9。
    pub fn decode(&mut self, data: &[u8; 4], ts: u64) -> bool {
        for &b in data.iter() {
            if b > 9 {
                return false;
            }
        }
        let mut cfgs = [CornerCfg { enabled: false, action: CornerAction::None }; 4];
        for (k, &c) in Corner::ALL.iter().enumerate() {
            let b = data[k];
            cfgs[c.idx()] = CornerCfg { enabled: b & 0x01 == 1, action: CornerAction::from_code(b >> 1) };
        }
        for &c in Corner::ALL.iter() {
            self.configure(c, cfgs[c.idx()], ts);
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F249 自检（判据：停留 300ms±30ms 实测等；含 xors32 fuzz）。
pub fn run_hotcorner_checks() -> CheckSet {
    let mut set = CheckSet::new("F249-hotcorner");
    let screen = Rect::new(0, 0, 1920, 1080);
    let bl = (screen.x + 2, screen.bottom() - 3); // 左下热区内采样点

    // 1. 默认配置审计：仅左下启用=开始菜单，其余三角预设动作但默认关。
    let mut hc = HotCorners::new(screen);
    set.add(
        "default audit: only bottom-left enabled",
        hc.audit_default()
            && hc.cfg(Corner::BottomLeft).action == CornerAction::StartMenu
            && hc.cfg(Corner::TopRight).action == CornerAction::NotifCenter
            && hc.cfg(Corner::BottomRight).action == CornerAction::QuickSettings
            && hc.cfg(Corner::TopLeft).action == CornerAction::TaskView,
        "",
    );

    // 2. 四角热区几何：8×8 且贴角。
    let z_ok = Corner::ALL.iter().all(|&c| {
        let z = hc.zone(c);
        z.w == HOTZONE_PX && z.h == HOTZONE_PX && z.right() <= screen.right() && z.bottom() <= screen.bottom()
    }) && hc.zone(Corner::TopLeft).x == 0
        && hc.zone(Corner::TopLeft).y == 0
        && hc.zone(Corner::BottomRight).right() == screen.right()
        && hc.zone(Corner::BottomRight).bottom() == screen.bottom();
    set.add("zones are 8px at four corners", z_ok, "");

    // 3. 停留 300ms 触发：0/100/200/300ms 采样，300ms 处开枪。
    let f300 = hc.on_move(bl.0, bl.1, 0);
    let _ = hc.on_move(bl.0, bl.1, 100);
    let _ = hc.on_move(bl.0, bl.1, 200);
    let f300b = hc.on_move(bl.0, bl.1, 300);
    set.add(
        "dwell 300ms triggers bottom-left",
        f300.is_none()
            && f300b == Some(CornerFire { corner: Corner::BottomLeft, action: CornerAction::StartMenu, fire_ts: 300 }),
        "",
    );

    // 4. 299ms 不触发（±30ms 实测门的下沿之外——门槛是硬 300）。
    let mut hc2 = HotCorners::new(screen);
    let _ = hc2.on_move(bl.0, bl.1, 0);
    let _ = hc2.on_move(bl.0, bl.1, 299);
    set.add("299ms dwell does not fire", hc2.dwell_state(Corner::BottomLeft).fired == false, "");

    // 5. 快速划过 20 次（<150ms）：0 触发、划过计数 == 20。
    let mut hc3 = HotCorners::new(screen);
    for k in 0..20u64 {
        let t0 = k * 400;
        let _ = hc3.on_move(bl.0, bl.1, t0);
        let _ = hc3.on_move(500, 500, t0 + 149); // 149ms 出角
    }
    set.add(
        "20 quick passes never fire, pass count 20",
        hc3.triggers[Corner::BottomLeft.idx()] == 0
            && hc3.passes[Corner::BottomLeft.idx()] == 20,
        "",
    );

    // 6. 出角重置：200ms 出角后必须重新满 300ms。
    let mut hc4 = HotCorners::new(screen);
    let _ = hc4.on_move(bl.0, bl.1, 0);
    let _ = hc4.on_move(bl.0, bl.1, 200); // 停 200ms
    let _ = hc4.on_move(500, 500, 250); // 出角（200ms ≥150 不计划过）
    // 修障登记（check 6 红）：注释「重入 299ms / 301ms」对应的时间线是
    // 300ms 重入、599ms 驻留 299ms、601ms 驻留 301ms——旧场景漏写了
    // 300ms 的重入采样，把 599ms 当重入事件，601ms 处驻留仅 2ms 永不
    // 触发。补上重入事件后本检查才真测「出角重置」（若时钟未重置，
    // 599ms 处就会开枪，f_again.is_none() 转红）。
    let _ = hc4.on_move(bl.0, bl.1, 300); // 重入（时钟重置）
    let f_again = hc4.on_move(bl.0, bl.1, 599); // 重入 299ms
    let f_late = hc4.on_move(bl.0, bl.1, 601); // 301ms
    set.add(
        "leaving corner resets dwell clock",
        f_again.is_none()
            && f_late.map(|f| f.fire_ts == 601).unwrap_or(false),
        "",
    );

    // 7. 禁用角不触发（右下默认关，停留再久也不动）。
    let mut hc5 = HotCorners::new(screen);
    let br = (screen.right() - 3, screen.bottom() - 3);
    let _ = hc5.on_move(br.0, br.1, 0);
    let f = hc5.on_move(br.0, br.1, 500);
    set.add(
        "disabled corner never fires",
        f.is_none() && !hc5.dwell_state(Corner::BottomRight).in_zone,
        "",
    );

    // 8. 角标动画一致性：只在触发后 120ms 内存在；未触发角恒 None。
    let mid = hc.indicator(Corner::BottomLeft, 360);
    let late = hc.indicator(Corner::BottomLeft, 431);
    let untouched = hc.indicator(Corner::TopRight, 360);
    set.add(
        "indicator anim 120ms tied to fire",
        mid.map(|p| p > 0 && p < 1000).unwrap_or(false)
            && late.is_none()
            && untouched.is_none(),
        "",
    );

    // 9. F245 联动：注入 reduced 策略，角标 80ms 直切（进度立即 1000）。
    hc.set_motion_policy(MotionPolicy::reduced());
    let r_early = hc.indicator(Corner::BottomLeft, 0 + 360); // 相对触发+60ms
    let _ = hc.set_motion_policy(MotionPolicy::normal());
    set.add(
        "reduced motion cuts indicator to 80ms",
        r_early == Some(1000),
        "",
    );

    // 10. 设置面：改绑右下=任务视图并启用，立即生效并留痕。
    hc.configure(Corner::BottomRight, CornerCfg { enabled: true, action: CornerAction::TaskView }, 500);
    let brf = {
        let br = (screen.right() - 3, screen.bottom() - 3);
        let _ = hc.on_move(br.0, br.1, 600);
        hc.on_move(br.0, br.1, 950)
    };
    let has_cfg_event = hc.recent_events().iter().flatten().any(|e| e.kind == 2);
    set.add(
        "configure takes effect immediately",
        brf.map(|f| f.action == CornerAction::TaskView).unwrap_or(false) && has_cfg_event,
        "",
    );

    // 11. 持久化 round-trip：encode → 新实例 decode → 配置逐角一致。
    let mut blob = [0u8; 4];
    hc.encode(&mut blob);
    let mut hc6 = HotCorners::new(screen);
    let ok = hc6.decode(&blob, 1000);
    set.add(
        "settings round-trip",
        ok && Corner::ALL.iter().all(|&c| hc6.cfg(c) == hc.cfg(c)),
        "",
    );

    // 12. xors32 fuzz：随机点流——仅启用角触发、单次入场至多一枪、
    //     触发时刻-入场时刻 ≥300ms、外部计得触发数与状态计数一致、无 panic。
    let mut x: u32 = 0x9E37_79B9;
    let mut hc7 = HotCorners::new(screen);
    hc7.configure(Corner::TopRight, CornerCfg { enabled: true, action: CornerAction::NotifCenter }, 1);
    let mut ts: u64 = 0;
    let mut survived = true;
    let mut fired_total: u32 = 0;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        ts += (x % 80 + 1) as u64;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let px = (x % 1920) as i32;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let py = (x % 1080) as i32;
        let entry = [
            hc7.dwell_state(Corner::TopLeft),
            hc7.dwell_state(Corner::TopRight),
            hc7.dwell_state(Corner::BottomLeft),
            hc7.dwell_state(Corner::BottomRight),
        ];
        if let Some(f) = hc7.on_move(px, py, ts) {
            let i = f.corner.idx();
            fired_total += 1;
            if !hc7.cfg(f.corner).enabled
                || ts.saturating_sub(entry[i].enter_ts) < DWELL_MS
                || entry[i].fired
            {
                survived = false;
            }
        }
    }
    let ledger_len = hc7.recent_events().iter().flatten().count();
    let total_triggers: u32 = hc7.triggers.iter().sum();
    set.add(
        "fuzz 2000 moves invariants hold",
        survived
            && fired_total == total_triggers
            && ledger_len <= LEDGER_CAP,
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
    fn dwell_boundary_exact_300() {
        let screen = Rect::new(0, 0, 800, 600);
        let mut hc = HotCorners::new(screen);
        let p = (screen.x + 1, screen.bottom() - 1);
        let _ = hc.on_move(p.0, p.1, 0);
        let _ = hc.on_move(p.0, p.1, 299);
        assert!(hc.dwell_state(Corner::BottomLeft).fired == false);
        let _ = hc.on_move(p.0, p.1, 300);
        assert!(hc.dwell_state(Corner::BottomLeft).fired);
    }

    #[test]
    fn one_shot_per_entry() {
        let screen = Rect::new(0, 0, 800, 600);
        let mut hc = HotCorners::new(screen);
        let p = (screen.x + 1, screen.bottom() - 1);
        let _ = hc.on_move(p.0, p.1, 0);
        let f1 = hc.on_move(p.0, p.1, 400);
        let f2 = hc.on_move(p.0, p.1, 900); // 同一次入场不再开枪
        assert!(f1.is_some() && f2.is_none());
        assert_eq!(hc.triggers[Corner::BottomLeft.idx()], 1);
    }

    #[test]
    fn mid_dwell_leave_neither_fires_nor_counts_pass() {
        let screen = Rect::new(0, 0, 800, 600);
        let mut hc = HotCorners::new(screen);
        let p = (screen.x + 1, screen.bottom() - 1);
        let _ = hc.on_move(p.0, p.1, 0);
        let _ = hc.on_move(400, 300, 200); // 200ms 离开：≥150 不计划过
        assert_eq!(hc.passes[Corner::BottomLeft.idx()], 0);
        assert_eq!(hc.triggers[Corner::BottomLeft.idx()], 0);
    }

    #[test]
    fn indicator_enter_curve_shape() {
        let screen = Rect::new(0, 0, 800, 600);
        let mut hc = HotCorners::new(screen);
        let p = (screen.x + 1, screen.bottom() - 1);
        let _ = hc.on_move(p.0, p.1, 0);
        let _ = hc.on_move(p.0, p.1, 300);
        // Enter 曲线 ease-out：60ms 处过半。
        let mid = hc.indicator(Corner::BottomLeft, 360).unwrap();
        assert!(mid > 500, "Enter 曲线中点应过半，实测 {mid}");
        assert!(hc.indicator(Corner::BottomLeft, 420).is_none());
    }

    #[test]
    fn decode_rejects_malformed_and_keeps_cfg() {
        let screen = Rect::new(0, 0, 800, 600);
        let mut hc = HotCorners::new(screen);
        // 动作码 5（0x0A >> 1）越出 0..=4 合法域 → 拒绝且配置原封不动。
        assert!(!hc.decode(&[0x0A, 0x00, 0x00, 0x00], 1));
        assert!(hc.audit_default());
        // 合法编码 round-trip：关左下、开右上。
        assert!(hc.decode(&[0x00, 0x07, 0x00, 0x00], 2)); // 右上=0b111: 启用+通知中心
        assert!(!hc.cfg(Corner::BottomLeft).enabled);
        assert!(hc.cfg(Corner::TopRight).enabled);
        assert_eq!(hc.cfg(Corner::TopRight).action, CornerAction::NotifCenter);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F249（热角触发）。v2 三件事：
// 1) 持久化 I/O：四角配置册 v2 定长容器序列化——magic b"VXH1" + 版本 1
//    + 定长 payload（每角 1 字节，编码域与既有 encode/decode 一致）+
//    FNV-1a 校验和，四类损坏显性拒绝（与既有 4 字节裸位包并存）；
// 2) UI 壳接线：设置页四角行清单（角名 + 启用位 + 动作码）+ 行命中
//    测试 + 角标指示器几何（触发动画的绘制面矩形）；
// 3) 判定面扩展：run_hotcorner_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：每角 1 字节（bit0=启用位，bit1-3=动作码）×4。
pub const VX2_HC_PAYLOAD: usize = 4;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_HC_BLOB: usize = 9 + VX2_HC_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或角字节越出 0..=9 合法编码域。
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

/// 四角配置册（设置中心热角页的持久化数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerCfgBook {
    pub cfgs: [CornerCfg; 4],
}

impl CornerCfgBook {
    /// 从状态机读出（遍历序 = Corner::ALL）。
    pub fn snapshot(hc: &HotCorners) -> CornerCfgBook {
        CornerCfgBook { cfgs: [hc.cfg(Corner::TopLeft), hc.cfg(Corner::TopRight), hc.cfg(Corner::BottomLeft), hc.cfg(Corner::BottomRight)] }
    }

    /// 推到状态机：走既有 decode 正规路径（非法域显性拒绝、configure
    /// 留痕 + 版本推进）。
    pub fn apply_to(&self, hc: &mut HotCorners, ts: u64) -> bool {
        let mut raw = [0u8; 4];
        for (k, c) in Corner::ALL.iter().enumerate() {
            raw[k] = self.cfgs[c.idx()].action.code() << 1 | (self.cfgs[c.idx()].enabled as u8);
        }
        hc.decode(&raw, ts)
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_HC_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        for (k, c) in Corner::ALL.iter().enumerate() {
            out[5 + k] = self.cfgs[c.idx()].action.code() << 1 | (self.cfgs[c.idx()].enabled as u8);
        }
        let crc = vx2_fnv(&out[..9 + VX2_HC_PAYLOAD - 4]);
        out[9 + VX2_HC_PAYLOAD - 4..9 + VX2_HC_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        VX2_HC_BLOB
    }

    /// 反序列化：四类损坏显性拒绝（角字节 > 9 按域越界处理）。
    pub fn from_bytes(blob: &[u8]) -> Result<CornerCfgBook, Vx2Error> {
        if blob.len() != VX2_HC_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_HC_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        if blob[5..9].iter().any(|&b| b > 9) {
            return Err(Vx2Error::BadLength);
        }
        let mut book = CornerCfgBook { cfgs: [CornerCfg { enabled: false, action: CornerAction::None }; 4] };
        for (k, c) in Corner::ALL.iter().enumerate() {
            let b = blob[5 + k];
            book.cfgs[c.idx()] = CornerCfg { enabled: b & 0x01 == 1, action: CornerAction::from_code(b >> 1) };
        }
        Ok(book)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 设置页行高（px）——v2 布局常量：F249 热角页行 32px。
pub const VX2_ROW_H_PX: i32 = 32;
/// 角标指示器边长（px）——触发动画的绘制面尺寸。
pub const VX2_IND_SIZE: i32 = 16;

/// 热角页行绘制条目：行矩形 + 启用位 + 动作码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerRow {
    pub corner_idx: usize,
    pub y: i32,
    pub h: i32,
    pub enabled: bool,
    pub action_code: u8,
}

/// 生成四角行清单（行序 = Corner::ALL 遍历序；动作码直取枚举编码——
/// 与持久化面同一编码源，页面不另造映射）。
pub fn corner_rows(hc: &HotCorners, out: &mut [CornerRow]) -> usize {
    let m = 4.min(out.len());
    for (k, c) in Corner::ALL.iter().enumerate() {
        if k >= m {
            break;
        }
        let cfg = hc.cfg(*c);
        out[k] = CornerRow {
            corner_idx: c.idx(),
            y: k as i32 * VX2_ROW_H_PX,
            h: VX2_ROW_H_PX,
            enabled: cfg.enabled,
            action_code: cfg.action.code(),
        };
    }
    m
}

/// 行命中测试（页面坐标；x ∈ [0, w) 且落在行内）。
pub fn corner_row_hit(rows: &[CornerRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

/// 角标指示器矩形：贴角 16×16（与热区同锚点，覆盖热区外沿）。
pub fn indicator_rect(hc: &HotCorners, c: Corner) -> Rect {
    let z = hc.zone(c);
    Rect::new(z.x, z.y, VX2_IND_SIZE.max(z.w), VX2_IND_SIZE.max(z.h))
}

// -- 判定面扩展 ------------------------------------------------------------

/// F249 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_hotcorner_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F249-hotcorner-v2");
    let screen = Rect::new(0, 0, 1920, 1080);

    // 1. 持久化 round-trip：配置册编→解→推新状态机→逐角一致。
    let mut src = HotCorners::new(screen);
    src.configure(Corner::TopRight, CornerCfg { enabled: true, action: CornerAction::NotifCenter }, 10);
    src.configure(Corner::BottomRight, CornerCfg { enabled: true, action: CornerAction::QuickSettings }, 20);
    let book = CornerCfgBook::snapshot(&src);
    let mut buf = [0u8; VX2_HC_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut dst = HotCorners::new(screen);
    match CornerCfgBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let ok = b2 == book && b2.apply_to(&mut dst, 30);
            set.add(
                "v2 persistence round-trip",
                ok && Corner::ALL.iter().all(|&c| dst.cfg(c) == src.cfg(c)) && dst.audit_default() == false,
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / 翻位与编码域越界）。
    let mut m = buf;
    m[0] = b'X';
    let mut v = buf;
    v[4] = 8;
    let mut c = buf;
    c[6] ^= 0xFF; // 角字节翻位越出 0..=9 合法域
    set.add(
        "v2 corruption explicitly rejected",
        CornerCfgBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && CornerCfgBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && CornerCfgBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && (CornerCfgBook::from_bytes(&c) == Err(Vx2Error::BadChecksum)
                || CornerCfgBook::from_bytes(&c) == Err(Vx2Error::BadLength)),
        "",
    );

    // 3. 行清单与命中：四行、启用位与状态一致、界内命中/界外不命中。
    let mut rows = [CornerRow { corner_idx: 0, y: 0, h: 0, enabled: false, action_code: 0 }; 4];
    let rn = corner_rows(&src, &mut rows);
    set.add(
        "v2 corner rows & hit",
        rn == 4
            && rows[0].corner_idx == 0 && !rows[0].enabled
            && rows[1].enabled && rows[1].action_code == CornerAction::NotifCenter.code()
            && rows[2].enabled && rows[2].action_code == CornerAction::StartMenu.code()
            && rows[3].y == 3 * VX2_ROW_H_PX
            && corner_row_hit(&rows, rn, 50, VX2_ROW_H_PX + 4, 400) == Some(1)
            && corner_row_hit(&rows, rn, 50, -1, 400).is_none()
            && corner_row_hit(&rows, rn, 400, 4, 400).is_none(),
        "",
    );

    // 4. 角标指示器几何与触发联动：矩形贴角覆盖热区、停留 300ms 触发后
    //    指示动画在窗口内给出进度（触发与动画同源判据的几何复核）。
    let mut hc4 = HotCorners::new(screen);
    let p = (screen.x + 2, screen.bottom() - 3);
    let _ = hc4.on_move(p.0, p.1, 0);
    let f = hc4.on_move(p.0, p.1, 300);
    let ind = indicator_rect(&hc4, Corner::BottomLeft);
    let zone = hc4.zone(Corner::BottomLeft);
    set.add(
        "v2 indicator rect tied to fire",
        f.is_some()
            && ind.x == zone.x && ind.y == zone.y
            && ind.w >= zone.w && ind.h >= zone.h
            && hc4.indicator(Corner::BottomLeft, 320).is_some()
            && hc4.indicator(Corner::TopRight, 320).is_none(),
        "",
    );

    // 5. xors32 fuzz 500 轮：随机配置册 round-trip 逐角相等、payload 任一
    //    字节翻位必被校验和或编码域捕获。
    let mut x: u32 = 0x2499_E5AB;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut b = CornerCfgBook { cfgs: [CornerCfg { enabled: false, action: CornerAction::None }; 4] };
        for (k, cc) in b.cfgs.iter_mut().enumerate() {
            cc.enabled = (x >> k) & 1 == 1;
            cc.action = CornerAction::from_code(((x >> (k + 2)) % 5) as u8);
        }
        let mut tbuf = [0u8; VX2_HC_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_HC_BLOB && CornerCfgBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_HC_PAYLOAD] ^= 0x33;
        ok &= CornerCfgBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum)
            || CornerCfgBook::from_bytes(&tbuf) == Err(Vx2Error::BadLength);
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
    fn v2_cfg_book_roundtrip_and_reject() {
        let screen = Rect::new(0, 0, 800, 600);
        let mut hc = HotCorners::new(screen);
        hc.configure(Corner::TopLeft, CornerCfg { enabled: true, action: CornerAction::TaskView }, 1);
        let b = CornerCfgBook::snapshot(&hc);
        let mut buf = [0u8; VX2_HC_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_HC_BLOB);
        assert_eq!(CornerCfgBook::from_bytes(&buf), Ok(b));
        let mut bad = buf;
        bad[7] = 10; // 编码域越界（篡改后重算校验和，专测值域分支）
        let crc = vx2_fnv(&bad[..9 + VX2_HC_PAYLOAD - 4]);
        bad[9 + VX2_HC_PAYLOAD - 4..9 + VX2_HC_PAYLOAD].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(CornerCfgBook::from_bytes(&bad), Err(Vx2Error::BadLength));
        let mut bad2 = buf;
        bad2[8] ^= 0x01;
        assert_eq!(CornerCfgBook::from_bytes(&bad2), Err(Vx2Error::BadChecksum));
    }

    #[test]
    fn v2_indicator_rect_covers_zone() {
        let screen = Rect::new(0, 0, 800, 600);
        let hc = HotCorners::new(screen);
        for c in Corner::ALL {
            let ind = indicator_rect(&hc, c);
            let z = hc.zone(c);
            assert_eq!(ind.x, z.x);
            assert_eq!(ind.y, z.y);
            assert!(ind.w >= z.w && ind.h >= z.h);
        }
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_hotcorner_v2_checks();
        assert!(set.all_passed(), "F249 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
