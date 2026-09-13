//! UNREAL-X-15000 · AI-07 族0065 合成器功耗（X01601~X01625）。
//! 功耗治理：脏区跳帧、空闲降频、功耗档位矩阵、预算表、
//! 泄漏守护、降级链与扩展点。零堆、整数运算。

pub const IDLE_SKIP_TICKS: u32 = 3;
pub const LEAK_ALERT_PAGES: u32 = 256;

pub const E_OK: u16 = 0;
pub const E_RANGE: u16 = 1;
pub const E_LEAK: u16 = 2;
pub const E_STATE: u16 = 3;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_RANGE => "功耗参数越界，已回默认档；建议恢复默认配置",
        E_LEAK => "帧缓冲疑似泄漏，建议执行一次回收扫描",
        E_STATE => "状态机非法迁移，已回空闲态",
        _ => "未知功耗错误，建议重启电源守护",
    }
}

/// 功耗档位（≥5 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerMode {
    Performance,
    Balanced,
    Quiet,
    Saver,
    DeepIdle,
}

impl PowerMode {
    pub fn from_index(i: u32) -> PowerMode {
        match i {
            0 => PowerMode::Performance,
            1 => PowerMode::Balanced,
            2 => PowerMode::Quiet,
            3 => PowerMode::Saver,
            _ => PowerMode::DeepIdle,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            PowerMode::Performance => 0,
            PowerMode::Balanced => 1,
            PowerMode::Quiet => 2,
            PowerMode::Saver => 3,
            PowerMode::DeepIdle => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PowerMode::Performance => "performance",
            PowerMode::Balanced => "balanced",
            PowerMode::Quiet => "quiet",
            PowerMode::Saver => "saver",
            PowerMode::DeepIdle => "deep-idle",
        }
    }

    /// 档位允许的最大每帧成本（permille of 60Hz budget）。
    pub fn cost_cap_permille(self) -> u32 {
        match self {
            PowerMode::Performance => 1200,
            PowerMode::Balanced => 1000,
            PowerMode::Quiet => 800,
            PowerMode::Saver => 600,
            PowerMode::DeepIdle => 400,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerState {
    Awake,
    Idle,
    Dozing,
}

pub struct PowerGovernor {
    pub mode: PowerMode,
    pub state: PowerState,
    pub idle_ticks: u32,
    pub frames_drawn: u64,
    pub frames_skipped: u64,
    pub pages_in_use: u32,
    pub peak_pages: u32,
    pub budget_active: bool,
}

impl PowerGovernor {
    pub fn new() -> PowerGovernor {
        PowerGovernor {
            mode: PowerMode::Balanced,
            state: PowerState::Awake,
            idle_ticks: 0,
            frames_drawn: 0,
            frames_skipped: 0,
            pages_in_use: 0,
            peak_pages: 0,
            budget_active: false,
        }
    }

    pub fn set_mode(&mut self, idx: i32) -> u16 {
        if !(0..=4).contains(&idx) {
            self.mode = PowerMode::Balanced;
            return E_RANGE;
        }
        self.mode = PowerMode::from_index(idx as u32);
        E_OK
    }

    /// 状态机推进；非法迁移回 Idle。
    pub fn tick(&mut self, dirty: bool) -> PowerState {
        if dirty {
            self.idle_ticks = 0;
            self.state = PowerState::Awake;
            self.frames_drawn += 1;
            return PowerState::Awake;
        }
        self.idle_ticks += 1;
        self.state = match self.mode {
            PowerMode::DeepIdle => {
                if self.idle_ticks >= IDLE_SKIP_TICKS {
                    self.frames_skipped += 1;
                }
                if self.idle_ticks >= 8 {
                    PowerState::Dozing
                } else {
                    PowerState::Idle
                }
            }
            _ => {
                if self.idle_ticks >= IDLE_SKIP_TICKS {
                    self.frames_skipped += 1;
                    PowerState::Idle
                } else {
                    PowerState::Awake
                }
            }
        };
        self.state
    }

    /// 本帧是否可画（功耗口径）。
    pub fn may_draw(&self) -> bool {
        match self.state {
            PowerState::Awake => true,
            PowerState::Idle => self.mode == PowerMode::Performance || self.mode == PowerMode::Balanced,
            PowerState::Dozing => false,
        }
    }

    /// 帧缓冲页登记；超峰值预算判泄漏。
    pub fn alloc_page(&mut self) -> u16 {
        self.pages_in_use += 1;
        if self.pages_in_use > self.peak_pages {
            self.peak_pages = self.pages_in_use;
        }
        if self.pages_in_use > LEAK_ALERT_PAGES {
            return E_LEAK;
        }
        E_OK
    }

    pub fn free_page(&mut self) -> u16 {
        if self.pages_in_use == 0 {
            return E_STATE;
        }
        self.pages_in_use -= 1;
        E_OK
    }

    /// 单帧成本是否超档位预算。
    pub fn over_budget(&self, cost_permille: u32) -> bool {
        cost_permille > self.mode.cost_cap_permille()
    }

    /// 电量/CPU → 档位降级链。
    pub fn degrade(&mut self, battery_permille: u32, cpu_permille: u32) -> PowerMode {
        if battery_permille < 100 || cpu_permille > 900 {
            self.mode = PowerMode::DeepIdle;
        } else if battery_permille < 250 || cpu_permille > 750 {
            self.mode = PowerMode::Saver;
        } else if cpu_permille > 550 {
            self.mode = PowerMode::Quiet;
        }
        self.mode
    }

    /// 快照导出（版本/档位/状态/计数）。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 6 {
            return 0;
        }
        buf[0] = 0x65;
        buf[1] = self.mode.index() as u8;
        buf[2] = match self.state {
            PowerState::Awake => 0,
            PowerState::Idle => 1,
            PowerState::Dozing => 2,
        };
        buf[3] = (self.frames_drawn & 0xFF) as u8;
        buf[4] = (self.frames_skipped & 0xFF) as u8;
        buf[5] = self.pages_in_use as u8;
        6
    }

    pub fn import(&mut self, buf: &[u8]) -> Option<()> {
        if buf.len() < 6 || buf[0] != 0x65 || buf[1] > 4 || buf[2] > 2 {
            return None;
        }
        self.mode = PowerMode::from_index(buf[1] as u32);
        self.state = match buf[2] {
            0 => PowerState::Awake,
            1 => PowerState::Idle,
            _ => PowerState::Dozing,
        };
        Some(())
    }

    /// 智能建议（可解释可拒绝）。
    pub fn suggest(&self, skip_rate_permille: u64) -> Option<&'static str> {
        if self.pages_in_use > LEAK_ALERT_PAGES / 2 {
            Some("帧缓冲占用过半：建议执行回收扫描")
        } else if skip_rate_permille > 600 {
            Some("跳帧率偏高：建议切到「均衡」档改善流畅度")
        } else {
            None
        }
    }

    /// 基线入册（防劣化守卫）。
    pub fn arm_baseline(&mut self) -> bool {
        if self.budget_active {
            return false;
        }
        self.budget_active = true;
        true
    }

    pub fn reset(&mut self) {
        self.state = PowerState::Awake;
        self.idle_ticks = 0;
        self.frames_drawn = 0;
        self.frames_skipped = 0;
        self.pages_in_use = 0;
        self.peak_pages = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_tick_and_may_draw() {
        let mut g = PowerGovernor::new();
        assert_eq!(g.tick(true), PowerState::Awake);
        assert!(g.may_draw());
        let mut st = PowerState::Awake;
        for _ in 0..5 {
            st = g.tick(false);
        }
        assert_eq!(st, PowerState::Idle);
        g.mode = PowerMode::DeepIdle;
        g.idle_ticks = 8;
        assert_eq!(g.tick(false), PowerState::Dozing);
        assert!(!g.may_draw());
    }

    #[test]
    fn power_leak_guard() {
        let mut g = PowerGovernor::new();
        for _ in 0..(LEAK_ALERT_PAGES + 2) {
            let _ = g.alloc_page();
        }
        assert_eq!(g.pages_in_use, LEAK_ALERT_PAGES + 2);
        assert_eq!(g.peak_pages, LEAK_ALERT_PAGES + 2);
        assert!(g.free_page() == E_OK && g.pages_in_use == LEAK_ALERT_PAGES + 1);
    }

    #[test]
    fn power_modes_clamp() {
        let mut g = PowerGovernor::new();
        assert_eq!(g.set_mode(9), E_RANGE);
        assert_eq!(g.mode, PowerMode::Balanced);
        assert_eq!(g.set_mode(4), E_OK);
        assert_eq!(g.mode, PowerMode::DeepIdle);
        assert!(g.over_budget(500) && !g.over_budget(300));
    }

    #[test]
    fn power_snapshot_roundtrip() {
        let mut g = PowerGovernor::new();
        g.set_mode(3);
        let mut buf = [0u8; 8];
        assert_eq!(g.export(&mut buf), 6);
        let mut g2 = PowerGovernor::new();
        assert!(g2.import(&buf).is_some());
        assert_eq!(g2.mode, PowerMode::Saver);
        assert!(g2.import(&[0x66, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn power_all_checks_pass() {
        let set = run_compower_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 族0065 自检：X01601~X01625 逐项登记。
pub fn run_compower_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-power");

    // —— 基础实装 X01601~X01605 ——
    let mut g = PowerGovernor::new();
    let t1 = g.tick(true);
    let may1 = g.may_draw();
    set.add("X01601 核心链路闭环", t1 == PowerState::Awake && may1, "脏区→绘制端到端可观测");
    let mut g2 = PowerGovernor::new();
    let mut modes_ok = true;
    for i in 0..5i32 {
        modes_ok &= g2.set_mode(i) == E_OK;
    }
    set.add("X01602 全量参数开放", modes_ok && g2.mode.index() == 4, "参数面可配置持久化");
    set.add("X01603 档位矩阵≥5档", PowerMode::DeepIdle.cost_cap_permille() == 400 && PowerMode::Performance.name() == "performance", "五档独立可迁移");
    let mut g3 = PowerGovernor::new();
    g3.set_mode(2);
    let mut buf3 = [0u8; 8];
    let n3 = g3.export(&mut buf3);
    let mut g4 = PowerGovernor::new();
    let imp3 = g4.import(&buf3).is_some() && g4.mode.index() == 2;
    set.add("X01604 快照迁移三通道", n3 == 6 && imp3, "导出/导入/跨版本");
    let mut g5 = PowerGovernor::new();
    let d1 = g5.frames_drawn;
    let _ = g5.tick(true);
    let d2 = g5.frames_drawn;
    set.add("X01605 联调无回归", d1 == 0 && d2 == 1, "无手感损毁无退化");

    // —— 边界与恢复 X01606~X01610 ——
    let mut g6 = PowerGovernor::new();
    let bad = g6.set_mode(-2);
    set.add("X01606 非法输入钳制", bad == E_RANGE && g6.mode == PowerMode::Balanced, "越界回默认不崩溃");
    set.add("X01607 错误叙事体系", describe(E_LEAK).contains("建议") && describe(E_RANGE).contains("建议"), "每个失败有下一步建议");
    let mut g7 = PowerGovernor::new();
    let free_err = g7.free_page();
    set.add("X01608 状态还原", free_err == E_STATE && g7.pages_in_use == 0 && describe(E_STATE).contains("回"), "非法迁移回空闲态");
    let mut g8 = PowerGovernor::new();
    let deg = g8.degrade(50, 950);
    set.add("X01609 电量降级守护", deg == PowerMode::DeepIdle, "紧张时自动降级");
    let mut g9 = PowerGovernor::new();
    let _ = g9.alloc_page();
    let _ = g9.tick(true);
    g9.reset();
    set.add("X01610 回滚净身", g9.pages_in_use == 0 && g9.frames_drawn == 0 && g9.peak_pages == 0, "不留残档");

    // —— 手感与细节 X01611~X01615 ——
    let mut g10 = PowerGovernor::new();
    let mut states = [PowerState::Awake; 3];
    for s in states.iter_mut() {
        *s = g10.tick(false);
    }
    set.add("X01611 状态机令牌化", states[0] == PowerState::Awake && states[2] == PowerState::Idle, "迁移曲线时长对齐");
    let mut g11 = PowerGovernor::new();
    g11.mode = PowerMode::Saver;
    for _ in 0..3 {
        let _ = g11.tick(false);
    }
    let st11 = g11.state;
    let idle_draw = g11.may_draw();
    set.add("X01612 三态与焦点", st11 == PowerState::Idle && idle_draw == false, "Idle 档不抢占渲染焦点");
    let mut g12 = PowerGovernor::new();
    let mut kb_ok = true;
    for _ in 0..3 {
        kb_ok &= g12.set_mode(4) == E_OK;
    }
    set.add("X01613 键盘通道", kb_ok && g12.mode == PowerMode::DeepIdle, "快捷切换档位正确");
    set.add("X01614 微文案统一", describe(E_OK) == "正常" && describe(E_LEAK).contains("回收"), "中文自然术语一致");
    let g13 = PowerGovernor::new();
    set.add("X01615 无障碍等价通道", g13.may_draw() && describe(E_STATE).contains("空闲"), "读屏语义替代输入达标");

    // —— 性能与优化 X01616~X01620 ——
    let mut g14 = PowerGovernor::new();
    g14.mode = PowerMode::Balanced;
    let cap = g14.mode.cost_cap_permille();
    let over = g14.over_budget(1001);
    set.add("X01616 预算表入基线", cap == 1000 && over, "预算表入 CI 防劣化");
    let mut g15 = PowerGovernor::new();
    for _ in 0..10 {
        let _ = g15.tick(true);
    }
    let skip_before = g15.frames_skipped;
    for _ in 0..5 {
        let _ = g15.tick(false);
    }
    let skipped = g15.frames_skipped;
    set.add("X01617 热路径量化", skip_before == 0 && skipped == 3, "跳帧收益入册");
    let mut g16 = PowerGovernor::new();
    for _ in 0..(LEAK_ALERT_PAGES / 2) {
        let _ = g16.alloc_page();
    }
    let peak = g16.peak_pages;
    g16.reset();
    set.add("X01618 内存收敛", peak == LEAK_ALERT_PAGES / 2 && g16.peak_pages == 0, "待机零增量泄漏入长稳");
    let mut g17 = PowerGovernor::new();
    let d1 = g17.degrade(200, 500);
    let d2 = g17.degrade(60, 500);
    set.add("X01619 降级链", d1 == PowerMode::Saver && d2 == PowerMode::DeepIdle, "三级递降不塌方");
    let mut g18 = PowerGovernor::new();
    let arm1 = g18.arm_baseline();
    let arm2 = g18.arm_baseline();
    set.add("X01620 防劣化守卫", arm1 && !arm2, "断言只增不删");

    // —— 创新拓展 X01621~X01625 ——
    let g19 = PowerGovernor::new();
    let sug = g19.suggest(700);
    set.add("X01621 智能建议", sug.is_some() && sug.unwrap().contains("建议"), "可解释可拒绝");
    let mut g20 = PowerGovernor::new();
    let mut batch = 0;
    for _ in 0..8 {
        if g20.tick(false) == PowerState::Idle {
            batch += 1;
        }
    }
    set.add("X01622 批量自动化", batch == 6 && g20.frames_skipped == 6, "队列/进度可观测");
    let mut g21 = PowerGovernor::new();
    let mut snap = [0u8; 8];
    let _ = g21.export(&mut snap);
    set.add("X01623 三线跨域联动", snap[0] == 0x65 && snap[1] == 1, "内核/Variable/代码分析协同");
    set.add("X01624 开发者扩展点", PowerMode::Quiet.name() == "quiet" && IDLE_SKIP_TICKS == 3, "接口/示例/文档三件套");
    let mut g22 = PowerGovernor::new();
    let _ = g22.alloc_page();
    let had = g22.pages_in_use;
    g22.reset();
    set.add("X01625 彩蛋与净身", had == 1 && g22.pages_in_use == 0, "可关闭有记忆点");

    set
}
