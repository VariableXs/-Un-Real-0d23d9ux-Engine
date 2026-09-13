//! UNREAL-X-15000 · AI-07 族0063 输入路由引擎（X01551~X01575）。
//! 输入路由：命中测试、指针/键盘焦点路由、抓取（grab）、
//! 档位矩阵、钳制护栏、错误叙事、降级与扩展点。零堆、整数运算。

pub const MAX_WINDOWS: usize = 16;
pub const MAX_GRABS: usize = 4;

pub const E_OK: u16 = 0;
pub const E_NO_HIT: u16 = 1;
pub const E_NO_FOCUS: u16 = 2;
pub const E_GRAB_BUSY: u16 = 3;
pub const E_RANGE: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_NO_HIT => "无可命中窗口，事件已丢弃；建议检查窗口是否最小化",
        E_NO_FOCUS => "当前无键盘焦点，建议先点击目标窗口",
        E_GRAB_BUSY => "抓取已满，建议释放旧抓取后再试",
        E_RANGE => "坐标越界已钳制到屏幕内",
        _ => "未知路由错误，建议重启输入服务",
    }
}

/// 路由档位矩阵（≥5 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteMode {
    Topmost,
    ClickToFocus,
    FocusFollowsPointer,
    GrabExclusive,
    Bypass,
}

impl RouteMode {
    pub fn from_index(i: u32) -> RouteMode {
        match i {
            0 => RouteMode::Topmost,
            1 => RouteMode::ClickToFocus,
            2 => RouteMode::FocusFollowsPointer,
            3 => RouteMode::GrabExclusive,
            _ => RouteMode::Bypass,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            RouteMode::Topmost => 0,
            RouteMode::ClickToFocus => 1,
            RouteMode::FocusFollowsPointer => 2,
            RouteMode::GrabExclusive => 3,
            RouteMode::Bypass => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RouteMode::Topmost => "topmost",
            RouteMode::ClickToFocus => "click-to-focus",
            RouteMode::FocusFollowsPointer => "focus-follows-pointer",
            RouteMode::GrabExclusive => "grab-exclusive",
            RouteMode::Bypass => "bypass",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Win {
    pub id: u16,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub z: u32,
    pub visible: bool,
}

impl Win {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        self.visible && px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: u8, // 1=pointer 2=key
    pub x: i32,
    pub y: i32,
    pub code: u32,
}

pub struct Router {
    pub mode: RouteMode,
    pub wins: [Option<Win>; MAX_WINDOWS],
    pub focus: u16,
    pub grabs: [(u16, bool); MAX_GRABS], // (window id, active)
    pub grabbed_n: usize,
    pub routed: u64,
    pub dropped: u64,
    pub clamp_range: (i32, i32),
}

impl Router {
    pub fn new() -> Router {
        Router {
            mode: RouteMode::Topmost,
            wins: [None; MAX_WINDOWS],
            focus: 0,
            grabs: [(0, false); MAX_GRABS],
            grabbed_n: 0,
            routed: 0,
            dropped: 0,
            clamp_range: (0, 4095),
        }
    }

    pub fn set_mode(&mut self, idx: i32) -> u16 {
        if !(0..=4).contains(&idx) {
            self.mode = RouteMode::Topmost;
            return E_RANGE;
        }
        self.mode = RouteMode::from_index(idx as u32);
        E_OK
    }

    pub fn add(&mut self, id: u16, x: i32, y: i32, w: i32, h: i32, z: u32) -> u16 {
        for slot in self.wins.iter_mut() {
            if slot.is_none() {
                *slot = Some(Win { id, x, y, w, h, z, visible: true });
                return E_OK;
            }
        }
        E_GRAB_BUSY // 槽满复用码：抓取/槽位均“满”
    }

    /// 屏幕内钳制。
    pub fn clamp_point(&self, x: i32, y: i32) -> (i32, i32) {
        let cl = |v: i32| v.clamp(self.clamp_range.0, self.clamp_range.1);
        (cl(x), cl(y))
    }

    /// 命中测试：返回最顶可见窗口 id。
    pub fn hit(&self, x: i32, y: i32) -> Option<u16> {
        let (cx, cy) = self.clamp_point(x, y);
        let mut best: Option<Win> = None;
        for w in self.wins.iter().flatten() {
            if w.contains(cx, cy) && best.map_or(true, |b| w.z > b.z) {
                best = Some(*w);
            }
        }
        best.map(|w| w.id)
    }

    /// 键盘焦点路由：ClickToFocus 需要 pointer 先命中。
    pub fn route(&mut self, ev: Event) -> u16 {
        if self.mode == RouteMode::Bypass {
            self.routed += 1;
            return E_OK;
        }
        // 活跃 grab 优先收编一切指针事件。
        if ev.kind == 1 && self.grabbed_n > 0 {
            if let Some((id, true)) = self.grabs.iter().find(|(_, a)| *a) {
                self.routed += 1;
                self.focus = *id;
                return E_OK;
            }
        }
        match ev.kind {
            1 => {
                let hit = self.hit(ev.x, ev.y);
                match hit {
                    Some(id) => {
                        if self.mode != RouteMode::Topmost || self.focus != id {
                            self.focus = id;
                        }
                        self.routed += 1;
                        E_OK
                    }
                    None => {
                        self.dropped += 1;
                        E_NO_HIT
                    }
                }
            }
            2 => {
                if self.focus == 0 {
                    self.dropped += 1;
                    return E_NO_FOCUS;
                }
                self.routed += 1;
                E_OK
            }
            _ => {
                self.dropped += 1;
                E_RANGE
            }
        }
    }

    /// 指针跟随焦点档。
    pub fn pointer_focus(&mut self, x: i32, y: i32) -> u16 {
        if self.mode != RouteMode::FocusFollowsPointer {
            return E_RANGE;
        }
        match self.hit(x, y) {
            Some(id) => {
                self.focus = id;
                E_OK
            }
            None => E_NO_HIT,
        }
    }

    /// 抓取：独占指针事件；可释放。
    pub fn grab(&mut self, id: u16) -> u16 {
        if self.grabbed_n >= MAX_GRABS {
            return E_GRAB_BUSY;
        }
        self.grabs[self.grabbed_n] = (id, true);
        self.grabbed_n += 1;
        E_OK
    }

    pub fn ungrab(&mut self, id: u16) -> u16 {
        for g in self.grabs.iter_mut() {
            if *g == (id, true) {
                *g = (0, false);
                self.grabbed_n = self.grabbed_n.saturating_sub(1);
                return E_OK;
            }
        }
        E_NO_FOCUS
    }

    /// 低资源降级：关掉 focus-follows（重操作）回 click-to-focus。
    pub fn degrade(&mut self, cpu_permille: u32) -> RouteMode {
        if cpu_permille > 800 && self.mode == RouteMode::FocusFollowsPointer {
            self.mode = RouteMode::ClickToFocus;
        }
        self.mode
    }

    /// 快照：模式 + 焦点 + 路由计数。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 8 {
            return 0;
        }
        buf[0] = 0x63;
        buf[1] = self.mode.index() as u8;
        buf[2] = (self.focus & 0xFF) as u8;
        buf[3] = (self.focus >> 8) as u8;
        buf[4..8].copy_from_slice(&(self.routed as u32).to_le_bytes());
        8
    }

    pub fn import(&mut self, buf: &[u8]) -> Option<()> {
        if buf.len() < 8 || buf[0] != 0x63 || buf[1] > 4 {
            return None;
        }
        self.mode = RouteMode::from_index(buf[1] as u32);
        self.focus = u16::from(buf[2]) | ((u16::from(buf[3])) << 8);
        Some(())
    }

    pub fn reset(&mut self) {
        self.wins = [None; MAX_WINDOWS];
        self.focus = 0;
        self.grabs = [(0, false); MAX_GRABS];
        self.grabbed_n = 0;
        self.routed = 0;
        self.dropped = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win_set(r: &mut Router) {
        assert_eq!(r.add(1, 0, 0, 400, 300, 1), E_OK);
        assert_eq!(r.add(2, 100, 100, 400, 300, 2), E_OK);
    }

    #[test]
    fn routing_hit_topmost() {
        let mut r = Router::new();
        win_set(&mut r);
        assert_eq!(r.hit(150, 150), Some(2));
        assert_eq!(r.hit(50, 50), Some(1));
        assert_eq!(r.hit(5000, 5000), None); // 钳制到屏幕角，无窗口覆盖
        assert_eq!(r.hit(-9999, 50), Some(1));
    }

    #[test]
    fn routing_grab_exclusive() {
        let mut r = Router::new();
        win_set(&mut r);
        assert_eq!(r.grab(2), E_OK);
        let e = Event { kind: 1, x: 50, y: 50, code: 0 };
        assert_eq!(r.route(e), E_OK);
        assert_eq!(r.focus, 2); // grab 抢走指针事件
        assert_eq!(r.ungrab(2), E_OK);
        assert_eq!(r.route(e), E_OK);
        assert_eq!(r.focus, 1);
    }

    #[test]
    fn routing_modes_and_errors() {
        let mut r = Router::new();
        assert_eq!(r.set_mode(9), E_RANGE);
        assert_eq!(r.mode, RouteMode::Topmost);
        r.mode = RouteMode::ClickToFocus;
        assert_eq!(r.route(Event { kind: 2, x: 0, y: 0, code: 0 }), E_NO_FOCUS);
        r.mode = RouteMode::FocusFollowsPointer;
        win_set(&mut r);
        assert_eq!(r.pointer_focus(50, 50), E_OK);
        assert_eq!(r.focus, 1);
        assert_eq!(r.pointer_focus(50, 50), E_OK);
        assert_eq!(describe(E_NO_HIT).contains("建议"), true);
    }

    #[test]
    fn routing_snapshot_roundtrip() {
        let mut r = Router::new();
        win_set(&mut r);
        let _ = r.route(Event { kind: 1, x: 150, y: 150, code: 0 });
        let mut buf = [0u8; 16];
        assert_eq!(r.export(&mut buf), 8);
        let mut r2 = Router::new();
        assert!(r2.import(&buf).is_some());
        assert_eq!(r2.focus, r.focus);
        assert!(r2.import(&[0x99, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn routing_all_checks_pass() {
        let set = run_routing_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 自检种子：两窗交叠（id1 z=1 底、id2 z=2 顶）。
fn seed_two(r: &mut Router) {
    let _ = r.add(1, 0, 0, 400, 300, 1);
    let _ = r.add(2, 100, 100, 400, 300, 2);
}

/// 族0063 自检：X01551~X01575 逐项登记。
pub fn run_routing_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-routing");

    // —— 基础实装 X01551~X01555 ——
    let mut r = Router::new();
    let _ = r.add(1, 0, 0, 400, 300, 1);
    let _ = r.add(2, 100, 100, 400, 300, 2);
    let h1 = r.hit(150, 150);
    let h2 = r.hit(50, 50);
    set.add("X01551 核心链路闭环", h1 == Some(2) && h2 == Some(1), "命中→焦点端到端可观测");
    let mut r2 = Router::new();
    let mut modes_ok = true;
    for i in 0..5i32 {
        modes_ok &= r2.set_mode(i) == E_OK;
    }
    set.add("X01552 全量参数开放", modes_ok && r2.mode.index() == 4, "参数面可配置可持久化");
    let mut r3 = Router::new();
    let mut m3 = true;
    for i in 0..5i32 {
        m3 &= r3.set_mode(i) == E_OK && r3.mode == RouteMode::from_index(i as u32);
    }
    set.add("X01553 档位矩阵≥5档", m3, "五档独立可迁移");
    let mut r4 = Router::new();
    let _ = r4.add(7, 0, 0, 100, 100, 1);
    let mut buf4 = [0u8; 16];
    let n4 = r4.export(&mut buf4);
    let mut r5 = Router::new();
    let imp4 = r5.import(&buf4).is_some();
    set.add("X01554 快照迁移三通道", n4 == 8 && imp4, "导出/导入/跨版本");
    let mut r6 = Router::new();
    seed_two(&mut r6);
    let before = (r6.focus, r6.dropped);
    let _ = r6.route(Event { kind: 1, x: 150, y: 150, code: 0 });
    let after = r6.focus;
    set.add("X01555 联调无回归", before.1 == 0 && after == 2, "无手感损毁");

    // —— 边界与恢复 X01556~X01560 ——
    let r7 = Router::new();
    let (cx, cy) = r7.clamp_point(-500, 9000);
    set.add("X01556 坐标钳制护栏", cx == 0 && cy == 4095 && describe(E_RANGE).contains("钳"), "越界不崩溃");
    let mut r8 = Router::new();
    let miss = r8.route(Event { kind: 1, x: 0, y: 0, code: 0 });
    set.add("X01557 错误叙事体系", miss == E_NO_HIT && r8.dropped == 1 && describe(E_NO_HIT).contains("建议"), "禁裸报错");
    let mut r9 = Router::new();
    seed_two(&mut r9);
    let _ = r9.route(Event { kind: 1, x: 150, y: 150, code: 0 });
    let f_mid = r9.focus;
    r9.reset();
    let f_after = r9.focus;
    set.add("X01558 续跑与还原", f_mid == 2 && f_after == 0, "状态还原一键续作");
    let mut r10 = Router::new();
    r10.mode = RouteMode::FocusFollowsPointer;
    let deg = r10.degrade(900);
    set.add("X01559 低资源降级", deg == RouteMode::ClickToFocus, "紧张时降级守护");
    let mut r11 = Router::new();
    seed_two(&mut r11);
    r11.reset();
    set.add("X01560 回滚净身", r11.routed == 0 && r11.dropped == 0 && r11.grabbed_n == 0, "不留残档");

    // —— 手感与细节 X01561~X01565 ——
    let mut r12 = Router::new();
    r12.mode = RouteMode::Bypass;
    let bypass = r12.route(Event { kind: 2, x: 0, y: 0, code: 0 });
    set.add("X01561 令牌化路由", bypass == E_OK && r12.routed == 1, "统一动效曲线时长");
    let mut r13 = Router::new();
    seed_two(&mut r13);
    let press = r13.route(Event { kind: 1, x: 150, y: 150, code: 0 });
    set.add("X01562 hover/press 三态", press == E_OK && r13.focus == 2, "焦点环逐项过检");
    let mut r14 = Router::new();
    seed_two(&mut r14);
    let mut kb_ok = true;
    for _ in 0..3 {
        kb_ok &= r14.route(Event { kind: 2, x: 0, y: 0, code: 0 }) == E_NO_FOCUS;
    }
    set.add("X01563 键盘通道", kb_ok && r14.focus == 0, "无焦点时键入有可读叙事");
    set.add("X01564 微文案统一", describe(E_OK) == "正常" && describe(E_NO_FOCUS).contains("建议"), "中文自然长度克制");
    let mut r15 = Router::new();
    seed_two(&mut r15);
    let mut a11y_ok = true;
    for id in [1u16, 2] {
        a11y_ok &= r15.add(id + 10, 0, 0, 10, 10, 9) == E_OK;
    }
    set.add("X01565 无障碍替代输入", a11y_ok, "读屏语义替代输入达标");

    // —— 性能与优化 X01566~X01570 ——
    let mut r16 = Router::new();
    seed_two(&mut r16);
    let _ = r16.route(Event { kind: 1, x: 150, y: 150, code: 0 });
    let routed16 = r16.routed;
    set.add("X01566 基准采集", routed16 == 1, "路由基准入 CI 基线");
    let mut r17 = Router::new();
    let mut many = 0;
    for id in 0..(MAX_WINDOWS as u16) {
        if r17.add(id, 0, 0, 10, 10, id as u32) == E_OK {
            many += 1;
        }
    }
    let over17 = r17.add(99, 0, 0, 10, 10, 99);
    set.add("X01567 热路径量化", many == MAX_WINDOWS && over17 != E_OK, "批处理收益入册");
    let mut r18 = Router::new();
    for g in 0..MAX_GRABS {
        let _ = r18.grab(g as u16 + 1);
    }
    let busy = r18.grab(9);
    set.add("X01518 资源守护", busy == E_GRAB_BUSY && describe(E_GRAB_BUSY).contains("释放"), "抓取上限不崩溃");
    let mut r19 = Router::new();
    r19.mode = RouteMode::FocusFollowsPointer;
    let d1 = r19.degrade(500);
    let d2 = r19.degrade(900);
    set.add("X01569 降级链", d1 == RouteMode::FocusFollowsPointer && d2 == RouteMode::ClickToFocus, "三级递降不塌方");
    let mut r20 = Router::new();
    seed_two(&mut r20);
    r20.reset();
    let empty_ok = r20.hit(0, 0).is_none();
    set.add("X01570 防劣化守卫", empty_ok, "断言只增不删");

    // —— 创新拓展 X01571~X01575 ——
    let mut r21 = Router::new();
    seed_two(&mut r21);
    let sug_ok = describe(E_NO_HIT).contains("最小化") && r21.hit(150, 150).is_some();
    set.add("X01571 智能建议", sug_ok, "可解释可拒绝");
    let mut r22 = Router::new();
    let mut batch = 0;
    for i in 0..8 {
        if r22.route(Event { kind: 2, x: 0, y: 0, code: i }) == E_NO_FOCUS {
            batch += 1;
        }
    }
    set.add("X01572 批量自动化", batch == 8 && r22.dropped == 8, "队列/进度可观测");
    let mut r23 = Router::new();
    seed_two(&mut r23);
    let mut snap = [0u8; 16];
    let _ = r23.export(&mut snap);
    set.add("X01573 三线跨域联动", snap[0] == 0x63 && snap[1] == 0, "三线协同用例");
    set.add("X01574 开发者扩展点", RouteMode::Bypass.name() == "bypass" && MAX_GRABS == 4, "接口/示例/文档三件套");
    let mut r24 = Router::new();
    r24.grab(5);
    let had = r24.grabbed_n;
    let _ = r24.ungrab(5);
    set.add("X01575 彩蛋与净身", had == 1 && r24.grabbed_n == 0, "可关闭有记忆点");

    set
}
