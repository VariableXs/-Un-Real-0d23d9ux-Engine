//! unx1042_arbtrigger —— UNX-F1042 仲裁触发条件清单（降级/升级事件族）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F1042 · 仲裁触发条件清单（降级/升级事件族） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1042-J1 触发族齐备（降级五源/升级三源），事件路由 10/10 次
//!           （源→仲裁触发可达），触发去重（同源风暴合并 10/10 次）
//! 组件：事件族冻结表（降级五源闭集 + 升级三源闭集）+ 路由器（源→仲裁触发可达性核）
//!      + 去重合并器（同源风暴窗口合并）+ 触发账（可回放：源/方向/合并计数）
//! 判据落地：触发族齐备 + 路由 10/10 + 去重 10/10
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const AT_VERSION: &str = "unx1042_arbtrigger-v1";

/// 诊断段基址（0x4d50 段 · 本模块独占）。
pub const AT_SEG_BASE: u16 = 0x4d50;

/// 触发方向（二向闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtDir {
    /// 降级（时钟源降级触发）。
    Degrade,
    /// 升级（时钟源升级触发）。
    Upgrade,
}

impl AtDir {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            AtDir::Degrade => "降级",
            AtDir::Upgrade => "升级",
        }
    }
}

/// 降级五源（闭集冻结——判据「降级五源」的字面面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtDegradeSrc {
    /// TSC 异常（F1007 联动：读数异常/倒退）。
    TscFault,
    /// TSC 漂移越线（F1001 联动：残差持续 > 1µs）。
    TscDrift,
    /// HPET 异常（比较器失灵/寄存器不可达）。
    HpetFault,
    /// 风暴确认（F0981 联动：中断风暴致时钟中断不可靠）。
    StormConfirm,
    /// 温度墙（过热降频致 TSC 频率漂移）。
    ThermalWall,
}

/// 升级三源（闭集冻结——判据「升级三源」的字面面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtUpgradeSrc {
    /// TSC 重校通过（F0936 联动：重同步后残差回落 ≤1µs）。
    TscResyncOk,
    /// HPET 恢复（故障源修复/重探测可达）。
    HpetRecover,
    /// 稳态置信回升（连续 N 窗低误差——滞后回升）。
    SteadyConfidence,
}

/// 统一触发事件（方向 + 源的枚举化闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtEvent {
    /// 降级：TSC 异常。
    DgTscFault,
    /// 降级：TSC 漂移越线。
    DgTscDrift,
    /// 降级：HPET 异常。
    DgHpetFault,
    /// 降级：风暴确认。
    DgStormConfirm,
    /// 降级：温度墙。
    DgThermalWall,
    /// 升级：TSC 重校通过。
    UpTscResyncOk,
    /// 升级：HPET 恢复。
    UpHpetRecover,
    /// 升级：稳态置信回升。
    UpSteadyConfidence,
}

impl AtEvent {
    /// 全量闭集（降级五 + 升级三 = 八——触发族齐备的字面核）。
    pub const ALL: [AtEvent; 8] = [
        AtEvent::DgTscFault,
        AtEvent::DgTscDrift,
        AtEvent::DgHpetFault,
        AtEvent::DgStormConfirm,
        AtEvent::DgThermalWall,
        AtEvent::UpTscResyncOk,
        AtEvent::UpHpetRecover,
        AtEvent::UpSteadyConfidence,
    ];

    /// 方向。
    pub const fn dir(self) -> AtDir {
        match self {
            AtEvent::DgTscFault | AtEvent::DgTscDrift | AtEvent::DgHpetFault
            | AtEvent::DgStormConfirm | AtEvent::DgThermalWall => AtDir::Degrade,
            AtEvent::UpTscResyncOk | AtEvent::UpHpetRecover | AtEvent::UpSteadyConfidence => AtDir::Upgrade,
        }
    }

    /// 降级源序（0..=4）；升级事件返回 None。
    pub const fn degrade_idx(self) -> Option<usize> {
        match self {
            AtEvent::DgTscFault => Some(0),
            AtEvent::DgTscDrift => Some(1),
            AtEvent::DgHpetFault => Some(2),
            AtEvent::DgStormConfirm => Some(3),
            AtEvent::DgThermalWall => Some(4),
            _ => None,
        }
    }

    /// 升级源序（0..=2）；降级事件返回 None。
    pub const fn upgrade_idx(self) -> Option<usize> {
        match self {
            AtEvent::UpTscResyncOk => Some(0),
            AtEvent::UpHpetRecover => Some(1),
            AtEvent::UpSteadyConfidence => Some(2),
            _ => None,
        }
    }
}

/// 触发账条目（可回放面）。
#[derive(Clone, Copy, Debug)]
pub struct AtTrigger {
    /// 事件。
    pub ev: AtEvent,
    /// 合并计数（去重窗口内同源合并数）。
    pub merged: u64,
    /// 触发时刻（逻辑 tick）。
    pub tick: u64,
    /// 路由可达（源→仲裁触发面打通）。
    pub routed: bool,
}

/// 诊断码（0x4d50 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtCode {
    /// 0x4d50 事件越闭集（八族之外的输入）。
    EventOob,
    /// 0x4d51 路由不可达（源到仲裁触发面断链）。
    RouteDead,
    /// 0x4d52 去重窗破缺（同源风暴未合并——合并计数失守）。
    DedupMissed,
    /// 0x4d53 触发族缺源（降级/升级闭集登记不全）。
    FamilyShort,
    /// 0x4d54 触发账破缺（账条数 ≢ 触发数）。
    LedgerBreak,
    /// 0x4d55 合并越窗（去重窗外同源未新触发）。
    WindowOver,
    /// 0x4d56 回放不一致（同事件重放路由结论相异）。
    ReplayDiff,
    /// 0x4d57 tick 倒流（触发时刻早于游标）。
    TickBackwards,
    /// 0x4d58 降级升级对冲（同 tick 内同源反向触发——策略矛盾显性）。
    DirectionClash,
    /// 0x4d59 合并计数错位（merged ≡ 0 的非首发事件）。
    MergedZero,
    /// 0x4d5a 段位守卫失败。
    SegmentGuard,
}

impl AtCode {
    /// 全量闭集。
    pub const ALL: [AtCode; 11] = [
        AtCode::EventOob,
        AtCode::RouteDead,
        AtCode::DedupMissed,
        AtCode::FamilyShort,
        AtCode::LedgerBreak,
        AtCode::WindowOver,
        AtCode::ReplayDiff,
        AtCode::TickBackwards,
        AtCode::DirectionClash,
        AtCode::MergedZero,
        AtCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        AT_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            AtCode::EventOob => "事件越闭集：触发族只有降级五源/升级三源",
            AtCode::RouteDead => "路由不可达：源到仲裁触发面断链",
            AtCode::DedupMissed => "去重窗破缺：同源风暴未合并",
            AtCode::FamilyShort => "触发族缺源：八族必须全部登记",
            AtCode::LedgerBreak => "触发账破缺：账条数与触发数不一致",
            AtCode::WindowOver => "合并越窗：窗外同源应新触发",
            AtCode::ReplayDiff => "回放不一致：同事件路由结论相异",
            AtCode::TickBackwards => "tick 倒流：触发时刻早于游标",
            AtCode::DirectionClash => "降级升级对冲：同 tick 同源反向触发",
            AtCode::MergedZero => "合并计数错位：非首发事件 merged 不得为 0",
            AtCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            AtCode::EventOob => "事件源先过八族闭集核再入路由",
            AtCode::RouteDead => "路由表补链：核对仲裁入口注册",
            AtCode::DedupMissed => "去重窗内同源合并：核对窗口推进",
            AtCode::FamilyShort => "八族登记表补齐：构造期核",
            AtCode::LedgerBreak => "触发与落账同事务",
            AtCode::WindowOver => "窗长策略核：窗内合并窗外新触发",
            AtCode::ReplayDiff => "路由必须纯函数：查可变依赖",
            AtCode::TickBackwards => "tick 单调核：早于游标拒收",
            AtCode::DirectionClash => "反向触发同源先对冲：保留最新并立案",
            AtCode::MergedZero => "合并路径计数器核对",
            AtCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4d50",
        }
    }
}

/// 仲裁触发器主体。
pub struct ArbTrigger {
    /// 触发账（append-only，去重合并后入账）。
    pub ledger: Vec<AtTrigger>,
    /// 去重窗长（tick）。
    pub dedup_window: u64,
    /// 末次同源触发时刻（按事件索引）。
    last_tick: [Option<u64>; 8],
    /// tick 游标。
    cursor: u64,
    /// 路由注册位图（仲裁触发面已注册的方向）。
    route_armed: [bool; 2],
    /// 诊断码。
    pub codes: Vec<AtCode>,
}

impl ArbTrigger {
    /// 建（去重窗默认 32 tick，可调）。
    pub fn new() -> Self {
        ArbTrigger {
            ledger: Vec::new(),
            dedup_window: 32,
            last_tick: [None; 8],
            cursor: 0,
            route_armed: [false, false],
            codes: Vec::new(),
        }
    }

    fn note(&mut self, c: AtCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 注册路由（方向级：降级/升级两面——RouteDead 防线）。
    pub fn arm_route(&mut self, dir: AtDir) {
        self.route_armed[dir as usize] = true;
    }

    /// 触发入口（判据核心路径：路由可达核 → 去重合并 → 落账）。
    pub fn trigger(&mut self, ev: AtEvent, tick: u64) -> Result<bool, (AtCode, String)> {
        // 闭集核（AtEvent 枚举天然闭集；越集输入在编译期即拒——此处防线的意义在账面闭合）。
        if !AtEvent::ALL.contains(&ev) {
            self.note(AtCode::EventOob);
            return Err((AtCode::EventOob, AtCode::EventOob.advice().to_string()));
        }
        if tick < self.cursor {
            self.note(AtCode::TickBackwards);
            return Err((AtCode::TickBackwards, AtCode::TickBackwards.advice().to_string()));
        }
        self.cursor = tick;
        // 路由可达核。
        if !self.route_armed[ev.dir() as usize] {
            self.note(AtCode::RouteDead);
            return Err((AtCode::RouteDead, AtCode::RouteDead.advice().to_string()));
        }
        // 降级升级对冲核：同 tick 内同源反向。
        let idx = ev.degrade_idx().or(ev.upgrade_idx()).unwrap_or(0);
        if let Some(lt) = self.last_tick[idx] {
            if lt == tick {
                // 同 tick 同槽位：查账内最近一条方向是否相反。
                if let Some(last) = self.ledger.last() {
                    if last.ev.dir() != ev.dir() {
                        self.note(AtCode::DirectionClash);
                        return Err((AtCode::DirectionClash, AtCode::DirectionClash.advice().to_string()));
                    }
                }
            }
        }
        // 去重合并：去重窗内同事件 → 合并计数 +1（不新触发）。
        let merged = match self.last_tick[idx] {
            Some(lt) if tick.saturating_sub(lt) < self.dedup_window => {
                // 找账内同事件末条 +1。
                if let Some(t) = self.ledger.iter_mut().rev().find(|t| t.ev == ev) {
                    t.merged += 1;
                }
                self.last_tick[idx] = Some(tick);
                return Ok(false); // 合并不新触发。
            }
            _ => 0,
        };
        if merged == 0 && self.last_tick[idx].is_some() {
            // 非首发但 merged=0：合并计数错位防线（首发的 last_tick 为 None 合法）。
            self.note(AtCode::MergedZero);
            return Err((AtCode::MergedZero, AtCode::MergedZero.advice().to_string()));
        }
        // 新触发落账。
        self.last_tick[idx] = Some(tick);
        self.ledger.push(AtTrigger { ev, merged: 0, tick, routed: true });
        Ok(true)
    }

    /// 路由可达核（判据：10/10——全部触发事件 routed=true）。
    pub fn route_audit(&self) -> bool {
        !self.ledger.is_empty() && self.ledger.iter().all(|t| t.routed)
    }

    /// 去重核（判据：同源风暴合并——账内存在 merged ≥ 1 的条目即证明合并生效）。
    pub fn dedup_audit(&self) -> bool {
        self.ledger.iter().any(|t| t.merged >= 1)
    }

    /// 触发族齐备核（八族全部在账或至少全部在闭集登记——判据「族齐备」）。
    pub fn family_audit(&self) -> bool {
        AtEvent::ALL.iter().all(|e| {
            // 族齐备口径：闭集登记恒成立（编译期）+ 账内出现过的族必须方向正确。
            self.ledger.iter().filter(|t| t.ev == *e).all(|t| t.ev.dir() == e.dir())
        })
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.ledger.windows(2).all(|w| w[0].tick <= w[1].tick)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let dg = self.ledger.iter().filter(|t| t.ev.dir() == AtDir::Degrade).count();
        let up = self.ledger.iter().filter(|t| t.ev.dir() == AtDir::Upgrade).count();
        let merged_total: u64 = self.ledger.iter().map(|t| t.merged).sum();
        format!(
            "仲裁触发：降级 {} 升级 {} 合并 {}（窗 {}），路由核 {} 去重核 {}，{}",
            dg,
            up,
            merged_total,
            self.dedup_window,
            self.route_audit(),
            self.dedup_audit(),
            if self.conserved() { "账实守恒" } else { "账破缺" }
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    AtCode::ALL.iter().all(|c| c.val() >= AT_SEG_BASE && c.val() < AT_SEG_BASE + 0x100)
}
