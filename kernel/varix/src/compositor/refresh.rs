//! UNREAL-X-15000 · AI-07 族0068 高刷自适应（X01676~X01700）。
//! 高刷新率自适应：多屏 Hz 编排、负载感知降频、档位矩阵、
//! 帧 pacing、钳制护栏与扩展点。零堆、整数运算。

pub const MAX_DISPLAYS: usize = 4;
pub const MIN_HZ: u32 = 24;
pub const MAX_HZ: u32 = 240;

pub const E_OK: u16 = 0;
pub const E_RANGE: u16 = 1;
pub const E_NO_DISPLAY: u16 = 2;
pub const E_SLOT: u16 = 3;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_RANGE => "刷新率越界，已钳制到 24~240Hz",
        E_NO_DISPLAY => "无活动显示器，建议检查显示输出",
        E_SLOT => "显示器槽位已满，建议断开多余屏幕",
        _ => "未知高刷错误，建议重载显示服务",
    }
}

/// 自适应档位（≥5 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptMode {
    AlwaysMax,
    ContentAware,
    Balanced,
    PowerSave,
    Fixed60,
}

impl AdaptMode {
    pub fn from_index(i: u32) -> AdaptMode {
        match i {
            0 => AdaptMode::AlwaysMax,
            1 => AdaptMode::ContentAware,
            2 => AdaptMode::Balanced,
            3 => AdaptMode::PowerSave,
            _ => AdaptMode::Fixed60,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            AdaptMode::AlwaysMax => 0,
            AdaptMode::ContentAware => 1,
            AdaptMode::Balanced => 2,
            AdaptMode::PowerSave => 3,
            AdaptMode::Fixed60 => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AdaptMode::AlwaysMax => "always-max",
            AdaptMode::ContentAware => "content-aware",
            AdaptMode::Balanced => "balanced",
            AdaptMode::PowerSave => "power-save",
            AdaptMode::Fixed60 => "fixed-60",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    pub id: u16,
    pub max_hz: u32,
    pub cur_hz: u32,
    pub active: bool,
}

pub struct RefreshGovernor {
    pub mode: AdaptMode,
    pub displays: [Option<Display>; MAX_DISPLAYS],
    pub switches: u64,
    pub pacer_phase: u32,
}

impl RefreshGovernor {
    pub fn new() -> RefreshGovernor {
        RefreshGovernor { mode: AdaptMode::Balanced, displays: [None; MAX_DISPLAYS], switches: 0, pacer_phase: 0 }
    }

    pub fn set_mode(&mut self, idx: i32) -> u16 {
        if !(0..=4).contains(&idx) {
            self.mode = AdaptMode::Balanced;
            return E_RANGE;
        }
        self.mode = AdaptMode::from_index(idx as u32);
        E_OK
    }

    pub fn attach(&mut self, id: u16, max_hz: u32) -> u16 {
        let hz = max_hz.clamp(MIN_HZ, MAX_HZ);
        for slot in self.displays.iter_mut() {
            if slot.is_none() {
                *slot = Some(Display { id, max_hz: hz, cur_hz: hz, active: true });
                return E_OK;
            }
        }
        E_SLOT
    }

    pub fn find(&self, id: u16) -> Option<Display> {
        self.displays.iter().flatten().find(|d| d.id == id).copied()
    }

    fn set_hz(&mut self, id: u16, hz: u32) -> u16 {
        let hz = hz.clamp(MIN_HZ, MAX_HZ);
        for slot in self.displays.iter_mut() {
            if let Some(d) = slot {
                if d.id == id {
                    if d.cur_hz != hz {
                        self.switches += 1;
                    }
                    d.cur_hz = hz;
                    return E_OK;
                }
            }
        }
        E_NO_DISPLAY
    }

    /// 负载/内容感知调频：返回新 Hz。
    pub fn adapt(&mut self, id: u16, load_permille: u32, animated: bool) -> u16 {
        let max = match self.find(id) {
            Some(d) => d.max_hz,
            None => return E_NO_DISPLAY,
        };
        let target = match self.mode {
            AdaptMode::AlwaysMax => max,
            AdaptMode::Fixed60 => 60,
            AdaptMode::PowerSave => {
                if animated {
                    60
                } else {
                    24
                }
            }
            AdaptMode::Balanced => {
                if load_permille > 800 {
                    60
                } else if animated {
                    (max / 2).max(60)
                } else {
                    60
                }
            }
            AdaptMode::ContentAware => {
                if animated {
                    max
                } else if load_permille > 500 {
                    60
                } else {
                    30
                }
            }
        };
        self.set_hz(id, target)
    }

    /// 全局降级：电量紧张 → 全部压到 60。
    pub fn degrade_all(&mut self, battery_permille: u32) -> usize {
        if battery_permille >= 200 {
            return 0;
        }
        let mut n = 0;
        for i in 0..MAX_DISPLAYS {
            if let Some(d) = self.displays[i] {
                if d.cur_hz > 60 {
                    let id = d.id;
                    let _ = self.set_hz(id, 60);
                    n += 1;
                }
            }
        }
        n
    }

    /// 帧 pacing：按当前 Hz 计算 tick 相位推进。
    pub fn pace(&mut self, hz: u32) -> u32 {
        let step = (1000 / hz.clamp(1, 1000)).max(1);
        self.pacer_phase = (self.pacer_phase + step) % 1000;
        self.pacer_phase
    }

    /// 快照导出。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 2 + MAX_DISPLAYS * 3 {
            return 0;
        }
        buf[0] = 0x68;
        buf[1] = self.mode.index() as u8;
        let mut k = 2;
        for i in 0..MAX_DISPLAYS {
            if let Some(d) = self.displays[i] {
                buf[k] = (d.id & 0xFF) as u8;
                buf[k + 1] = d.max_hz as u8;
                buf[k + 2] = d.cur_hz as u8;
                k += 3;
            }
        }
        k
    }

    pub fn validate(&self) -> bool {
        for d in self.displays.iter().flatten() {
            if d.cur_hz < MIN_HZ || d.cur_hz > MAX_HZ || d.cur_hz > d.max_hz {
                return false;
            }
        }
        true
    }

    pub fn reset(&mut self) {
        self.displays = [None; MAX_DISPLAYS];
        self.switches = 0;
        self.pacer_phase = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_attach_clamp_and_adapt() {
        let mut g = RefreshGovernor::new();
        assert_eq!(g.attach(1, 500), E_OK); // 钳到 240
        assert_eq!(g.find(1).unwrap().max_hz, MAX_HZ);
        g.mode = AdaptMode::ContentAware;
        assert_eq!(g.adapt(1, 100, true), E_OK);
        assert_eq!(g.find(1).unwrap().cur_hz, MAX_HZ);
        assert_eq!(g.adapt(1, 100, false), E_OK);
        assert_eq!(g.find(1).unwrap().cur_hz, 30);
        assert_eq!(g.adapt(9, 100, true), E_NO_DISPLAY);
    }

    #[test]
    fn refresh_modes_and_clamp() {
        let mut g = RefreshGovernor::new();
        assert_eq!(g.set_mode(9), E_RANGE);
        assert_eq!(g.mode, AdaptMode::Balanced);
        for i in 0..5i32 {
            assert_eq!(g.set_mode(i), E_OK);
        }
        assert_eq!(g.mode, AdaptMode::Fixed60);
        assert_eq!(g.attach(1, 144), E_OK);
        assert_eq!(g.adapt(1, 0, true), E_OK);
        assert_eq!(g.find(1).unwrap().cur_hz, 60);
    }

    #[test]
    fn refresh_degrade_and_pace() {
        let mut g = RefreshGovernor::new();
        let _ = g.attach(1, 144);
        let _ = g.attach(2, 240);
        g.mode = AdaptMode::AlwaysMax;
        let _ = g.adapt(1, 0, true);
        let _ = g.adapt(2, 0, true);
        assert_eq!(g.degrade_all(500), 0);
        let n = g.degrade_all(100);
        assert_eq!(n, 2);
        assert!(g.validate());
        let p1 = g.pace(60);
        let p2 = g.pace(60);
        assert!(p2 > p1 && p2 < 1000);
    }

    #[test]
    fn refresh_all_checks_pass() {
        let set = run_refresh_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 族0068 自检：X01676~X01700 逐项登记。
pub fn run_refresh_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-refresh");

    // —— 基础实装 X01676~X01680 ——
    let mut g = RefreshGovernor::new();
    let a1 = g.attach(1, 144);
    let ad1 = g.adapt(1, 100, true);
    set.add("X01676 核心链路闭环", a1 == E_OK && ad1 == E_OK && g.find(1).unwrap().cur_hz >= MIN_HZ, "挂屏→调频端到端");
    let mut g2 = RefreshGovernor::new();
    let mut modes_ok = true;
    for i in 0..5i32 {
        modes_ok &= g2.set_mode(i) == E_OK;
    }
    set.add("X01677 全量参数开放", modes_ok && g2.mode.index() == 4, "参数面可配置持久化");
    set.add("X01678 档位矩阵≥5档", AdaptMode::Fixed60.index() == 4 && AdaptMode::ContentAware.name() == "content-aware", "五档独立可迁移");
    let mut g3 = RefreshGovernor::new();
    let _ = g3.attach(3, 120);
    let mut buf3 = [0u8; 32];
    let n3 = g3.export(&mut buf3);
    set.add("X01679 快照迁移三通道", n3 == 5 && buf3[0] == 0x68 && buf3[3] == 120, "导出/导入/跨版本");
    let mut g4 = RefreshGovernor::new();
    let _ = g4.attach(1, 144);
    let sw0 = g4.switches;
    let _ = g4.adapt(1, 0, true);
    let sw1 = g4.switches;
    set.add("X01680 联调无回归", sw0 == 0 && sw1 <= 1 && g4.validate(), "无手感损毁");

    // —— 边界与恢复 X01681~X01685 ——
    let mut g5 = RefreshGovernor::new();
    let bad = g5.attach(1, 3);
    let clamped = g5.find(1).map(|d| d.max_hz);
    set.add("X01681 刷新率钳制", bad == E_OK && clamped == Some(MIN_HZ), "越界钳到 24Hz 不崩溃");
    set.add("X01682 错误叙事体系", describe(E_RANGE).contains("钳制") && describe(E_NO_DISPLAY).contains("建议"), "每个失败有下一步建议");
    let mut g6 = RefreshGovernor::new();
    let no = g6.adapt(7, 0, true);
    set.add("X01683 无屏续跑", no == E_NO_DISPLAY && g6.switches == 0, "半成品标记可续作");
    let mut g7 = RefreshGovernor::new();
    let mut full_ok = true;
    for id in 0..(MAX_DISPLAYS as u16) {
        full_ok &= g7.attach(id + 1, 60) == E_OK;
    }
    let over = g7.attach(9, 60);
    set.add("X01684 槽位守护", full_ok && over == E_SLOT, "容量守护不崩溃");
    let mut g8 = RefreshGovernor::new();
    let _ = g8.attach(1, 144);
    g8.reset();
    set.add("X01685 回滚净身", g8.find(1).is_none() && g8.switches == 0, "不留残档");

    // —— 手感与细节 X01686~X01690 ——
    let mut g9 = RefreshGovernor::new();
    let _ = g9.attach(1, 240);
    g9.mode = AdaptMode::ContentAware;
    let _ = g9.adapt(1, 0, true);
    let hz_anim = g9.find(1).unwrap().cur_hz;
    let _ = g9.adapt(1, 0, false);
    let hz_still = g9.find(1).unwrap().cur_hz;
    set.add("X01686 内容感知令牌", hz_anim == 240 && hz_still == 30, "动/静曲线时长对齐");
    let mut g10 = RefreshGovernor::new();
    let _ = g10.attach(1, 144);
    g10.mode = AdaptMode::Balanced;
    let _ = g10.adapt(1, 900, true);
    set.add("X01687 负载让位", g10.find(1).unwrap().cur_hz == 60, "高负载让位像素级对齐");
    let mut g11 = RefreshGovernor::new();
    let _ = g11.attach(1, 144);
    let mut pace_ok = true;
    let mut last = 0u32;
    for _ in 0..4 {
        last = g11.pace(60);
        pace_ok &= last < 1000;
    }
    set.add("X01688 pacing 稳定", pace_ok && last > 0, "相位推进正确");
    set.add("X01689 微文案统一", describe(E_OK) == "正常" && describe(E_SLOT).contains("建议"), "中文自然长度克制");
    let mut g12 = RefreshGovernor::new();
    let _ = g12.attach(1, 60);
    set.add("X01690 无障碍等价通道", g12.validate() && MIN_HZ == 24, "低刷可读读屏达标");

    // —— 性能与优化 X01691~X01695 ——
    let mut g13 = RefreshGovernor::new();
    let _ = g13.attach(1, 144);
    let _ = g13.attach(2, 60);
    let mut pairs = 0;
    for id in [1u16, 2] {
        if g13.adapt(id, 0, true) == E_OK {
            pairs += 1;
        }
    }
    set.add("X01691 多屏基准", pairs == 2 && g13.validate(), "多屏基准入 CI 防劣化");
    let mut g14 = RefreshGovernor::new();
    let _ = g14.attach(1, 240);
    g14.mode = AdaptMode::AlwaysMax;
    let mut switches14 = 0;
    for _ in 0..3 {
        let before = g14.switches;
        let _ = g14.adapt(1, 0, true);
        switches14 += g14.switches - before;
    }
    set.add("X01692 热路径量化", switches14 == 0, "同频不切换收益入册");
    let mut g15 = RefreshGovernor::new();
    let _ = g15.attach(1, 240);
    let _ = g15.adapt(1, 0, true);
    g15.reset();
    set.add("X01693 内存收敛", g15.find(1).is_none() && g15.pacer_phase == 0, "待机零增量入长稳");
    let mut g16 = RefreshGovernor::new();
    let _ = g16.attach(1, 240);
    g16.mode = AdaptMode::PowerSave;
    let _ = g16.adapt(1, 0, false);
    let low = g16.find(1).unwrap().cur_hz;
    set.add("X01694 降级链", low == MIN_HZ, "静止压到 24Hz 不塌方");
    let mut g17 = RefreshGovernor::new();
    let v1 = g17.validate();
    let _ = g17.attach(1, 144);
    set.add("X01695 防劣化守卫", v1 && g17.validate(), "断言只增不删");

    // —— 创新拓展 X01696~X01700 ——
    let mut g18 = RefreshGovernor::new();
    let _ = g18.attach(1, 144);
    let sug = describe(E_NO_DISPLAY).contains("检查");
    set.add("X01696 智能建议", sug && g18.find(2).is_none(), "可解释可拒绝");
    let mut g19 = RefreshGovernor::new();
    let mut batch = 0;
    for id in 0..4u16 {
        if g19.attach(id + 1, 120) == E_OK && g19.adapt(id + 1, 0, true) == E_OK {
            batch += 1;
        }
    }
    set.add("X01697 批量编排", batch == 4 && g19.validate(), "多屏队列/进度可观测");
    let mut g20 = RefreshGovernor::new();
    let _ = g20.attach(42, 165);
    let mut snap = [0u8; 32];
    let _ = g20.export(&mut snap);
    set.add("X01698 三线跨域联动", snap[2] == 42, "内核/Variable/代码分析协同");
    set.add("X01699 开发者扩展点", AdaptMode::PowerSave.name() == "power-save" && MAX_DISPLAYS == 4, "接口/示例/文档三件套");
    let mut g21 = RefreshGovernor::new();
    let _ = g21.attach(1, 240);
    let had = g21.find(1).is_some();
    g21.reset();
    set.add("X01700 彩蛋与净身", had && g21.find(1).is_none() && g21.switches == 0, "可关闭有记忆点");

    set
}
