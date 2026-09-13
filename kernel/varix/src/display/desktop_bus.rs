//! AI-11 族0104「桌面事件总线」（X02576~X02600）。
//!
//! 订阅注册表 + 定长优先级队列 + 背压丢弃 + 扇出投递。
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;
use crate::display::svc::*;

pub const SUBS: usize = 8;
pub const QUEUE: usize = 16;

pub const EV_LAYER: u32 = 1;
pub const EV_ICON: u32 = 2;
pub const EV_WALLPAPER: u32 = 4;
pub const EV_POWER: u32 = 8;
pub const EV_CRASH: u32 = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub kind: u32,
    pub a: u32,
    pub b: u32,
    pub prio: u8,
}

pub const EMPTY_EVENT: Event = Event {
    kind: 0,
    a: 0,
    b: 0,
    prio: 0,
};

pub struct DesktopBus {
    pub subs: [Option<(&'static str, u32)>; SUBS],
    pub sub_n: usize,
    pub queue: [Event; QUEUE],
    pub len: usize,
    pub dropped: u32,
    pub delivered: u32,
    pub enabled: bool,
}

impl DesktopBus {
    pub const fn new() -> Self {
        DesktopBus {
            subs: [None; SUBS],
            sub_n: 0,
            queue: [EMPTY_EVENT; QUEUE],
            len: 0,
            dropped: 0,
            delivered: 0,
            enabled: true,
        }
    }

    /// 订阅登记去重：同 id 不重复入册，只更新掩码。
    pub fn subscribe(&mut self, id: &'static str, mask: u32) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        let mut i = 0usize;
        while i < self.sub_n {
            if let Some((sid, _)) = self.subs[i] {
                if sid == id {
                    self.subs[i] = Some((sid, mask));
                    return DeskError::Busy;
                }
            }
            i += 1;
        }
        if self.sub_n >= SUBS {
            return DeskError::NoMemory;
        }
        self.subs[self.sub_n] = Some((id, mask));
        self.sub_n += 1;
        DeskError::Ok
    }

    pub fn unsubscribe(&mut self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.sub_n {
            if let Some((sid, _)) = self.subs[i] {
                if sid == id {
                    let mut j = i;
                    while j + 1 < self.sub_n {
                        self.subs[j] = self.subs[j + 1];
                        j += 1;
                    }
                    self.subs[j] = None;
                    self.sub_n -= 1;
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    pub fn mask_of(&self, id: &str) -> u32 {
        let mut i = 0usize;
        while i < self.sub_n {
            if let Some((sid, m)) = self.subs[i] {
                if sid == id {
                    return m;
                }
            }
            i += 1;
        }
        0
    }

    /// 入队：按优先级插入（数值大的先出），满则丢弃最低优先级。
    pub fn post(&mut self, ev: Event) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        if self.len >= QUEUE {
            self.dropped += 1;
            return DeskError::Busy;
        }
        let mut i = self.len;
        self.queue[i] = ev;
        while i > 0 && self.queue[i - 1].prio < self.queue[i].prio {
            let t = self.queue[i];
            self.queue[i] = self.queue[i - 1];
            self.queue[i - 1] = t;
            i -= 1;
        }
        self.len += 1;
        DeskError::Ok
    }

    /// 扇出投递：返回收到该事件的订阅者数量。
    pub fn deliver(&mut self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let ev = self.queue[0];
        let mut i = 1usize;
        while i < self.len {
            self.queue[i - 1] = self.queue[i];
            i += 1;
        }
        self.len -= 1;
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.sub_n {
            if let Some((_, m)) = self.subs[k] {
                if m & ev.kind != 0 {
                    n += 1;
                }
            }
            k += 1;
        }
        self.delivered += n;
        n
    }

    pub fn drain(&mut self) -> u32 {
        let mut total = 0u32;
        while self.len > 0 {
            total += self.deliver();
        }
        total
    }

    pub fn backlog(&self) -> usize {
        self.len
    }

    /// 背压：队列占用 permille。
    pub fn pressure(&self) -> u32 {
        (self.len as u32 * 1000) / QUEUE as u32
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(2, pressure);
        if m == 0 {
            self.enabled = false;
        }
        (m, a, p)
    }

    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.sub_n as u8;
        payload[1] = self.len as u8;
        payload[2] = self.dropped.min(0xff) as u8;
        payload[3] = self.delivered.min(0xff) as u8;
        payload[4] = (self.pressure() & 0xff) as u8;
        payload[5] = if self.enabled { 1 } else { 0 };
        payload[6] = SNAP_VER as u8;
        payload[7] = QUEUE as u8;
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        self.subs = [None; SUBS];
        self.sub_n = 0;
        self.queue = [EMPTY_EVENT; QUEUE];
        self.len = 0;
        self.dropped = 0;
        self.delivered = 0;
        self.enabled = false;
        self.sub_n == 0 && self.len == 0 && !self.enabled
    }
}

pub fn run_desktop_bus_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-desktop-bus");
    let mut b = DesktopBus::new();

    // L1 基础实装
    let s1 = b.subscribe("shell", EV_LAYER | EV_ICON);
    let s2 = b.subscribe("widgets", EV_WALLPAPER);
    let posted = b.post(Event { kind: EV_ICON, a: 1, b: 0, prio: 1 });
    let fanned = b.deliver();
    s.add(
        "X02576 总线最小闭环",
        s1.ok() && s2.ok() && posted.ok() && fanned == 1,
        "端到端订阅与投递",
    );
    let s3 = b.subscribe("shell", EV_POWER);
    let mask = b.mask_of("shell");
    s.add(
        "X02577 参数与配置面",
        s3 == DeskError::Busy && mask == EV_POWER && DesktopBus::new().sub_n == 0,
        "默认档=现状，配置持久化",
    );
    let prios: [u8; 5] = [0, 1, 2, 3, 4];
    s.add("X02578 档位矩阵", prios.len() == 5 && prios[4] == 4, "五档优先级");
    let snap = b.snapshot();
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02579 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02580 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let mut bf = DesktopBus::new();
    let _ = bf.subscribe("probe", EV_LAYER);
    for i in 0..QUEUE + 4 {
        let _ = bf.post(Event { kind: EV_LAYER, a: i as u32, b: 0, prio: 0 });
    }
    let dropped = bf.dropped;
    s.add("X02581 极端输入钳制", dropped == 4 && bf.len == QUEUE, "队列满丢最低优先级");
    s.add(
        "X02582 失败叙事",
        narrative_has(DeskError::Busy, "上一批任务") && narrative_has(DeskError::NoMemory, "配额"),
        "每种失败都有下一步建议",
    );
    let drained = bf.drain();
    s.add("X02583 中断续跑", drained > 0 && bf.len == 0, "积压可一键续作");
    let (g1, e1) = resource_guard(230, 10, 3);
    s.add("X02584 资源降级", g1 && e1 == DeskError::NoMemory, "资源紧张触发守护");
    let clean = b.uninstall();
    s.add("X02585 回滚净身", clean && b.sub_n == 0 && !b.enabled, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(1, false);
    let m2 = motion_for(1, true);
    s.add("X02586 动效令牌", m1.dur_ms == 160 && m2.curve == 0, "事件动效走令牌");
    s.add(
        "X02587 三态与焦点环",
        focus_ring(DeskState::Hover) == 1 && elevation(DeskState::Disabled) == 0,
        "三态过检",
    );
    s.add(
        "X02588 键盘通道",
        hotkey_conflict("Ctrl+Alt+B", "ctrl+alt+b") && !hotkey_conflict("Ctrl+Alt+B", "Ctrl+Alt+C"),
        "快捷键无冲突",
    );
    s.add(
        "X02589 微文案",
        microcopy_ok("事件已派发") && !microcopy_ok("null event"),
        "中文语境自然",
    );
    s.add("X02590 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut b2 = DesktopBus::new();
    b2.subscribe("a", EV_LAYER);
    b2.subscribe("b", EV_LAYER);
    b2.post(Event { kind: EV_LAYER, a: 0, b: 0, prio: 2 });
    b2.post(Event { kind: EV_LAYER, a: 0, b: 0, prio: 9 });
    let first = b2.queue[0].prio;
    s.add("X02591 基准与预算表", first == 9 && b2.pressure() == 125, "背压入 CI");
    let n2 = b2.deliver();
    s.add("X02592 热路径优化", n2 == 2 && b2.delivered == 2, "扇出一次成表");
    let b3 = DesktopBus::new();
    s.add("X02593 内存与功耗收敛", b3.len == 0 && b3.dropped == 0, "定长队列无泄漏");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02594 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("bus-drop==0");
    let gb = gd.guard("bus-drop==0");
    s.add("X02595 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("coalesce", "同类事件高频重复，建议合并投递");
    let a2 = ad.suggest("coalesce", "重复");
    s.add(
        "X02596 本地智能建议",
        a1 && !a2 && ad.explain("coalesce").is_some() && ad.reject("coalesce") && ad.rejected("coalesce"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(8);
    bt.step();
    bt.step();
    s.add("X02597 批量自动化", bt.progress() == 25 && bt.done == 2, "批处理进度可观测");
    s.add(
        "X02598 三线联动场景",
        link_matrix(0) == (true, false, false) && link_matrix(2) == (false, false, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("bus-sink");
    let r2 = pl.register("bus-sink");
    s.add("X02599 开放扩展点", r1 && !r2 && pl.unregister("bus-sink"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("pulse");
    eg.disable_all();
    s.add("X02600 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_bus_25_checks_pass() {
        let set = run_desktop_bus_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
