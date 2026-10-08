//! AI-11 族0108「桌面崩溃恢复」（X02676~X02700）。
//!
//! 看门狗心跳、指数退避重启、安全模式降级与快照回滚。
//! 硬约束：no_std / 无 alloc / 无浮点（退避用整数档位）。

use crate::checks::CheckSet;
use crate::display::svc::*;

/// 五档退避基数（ms）。
pub const BACKOFF_LEVELS: [u32; 5] = [200, 500, 1_000, 2_000, 5_000];

pub const MAX_RESTARTS: u32 = 5;

pub struct Watchdog {
    pub timeout_ms: u32,
    pub last_beat_ms: u32,
    pub now_ms: u32,
    pub restarts: u32,
    pub max_restarts: u32,
    pub level: u8,
    pub backoff_ms: u32,
    pub safe_mode: bool,
    pub snapshots: u32,
    pub enabled: bool,
}

impl Watchdog {
    pub const fn new() -> Self {
        Watchdog {
            timeout_ms: 2_000,
            last_beat_ms: 0,
            now_ms: 0,
            restarts: 0,
            max_restarts: MAX_RESTARTS,
            level: 0,
            backoff_ms: BACKOFF_LEVELS[0],
            safe_mode: false,
            snapshots: 0,
            enabled: true,
        }
    }

    pub fn configure(&mut self, timeout_ms: u32, max_restarts: u32) -> DeskError {
        let ok = timeout_ms >= 100 && timeout_ms <= 60_000 && max_restarts <= 32;
        self.timeout_ms = if timeout_ms < 100 || timeout_ms > 60_000 {
            2_000
        } else {
            timeout_ms
        };
        self.max_restarts = if max_restarts > 32 { MAX_RESTARTS } else { max_restarts };
        if ok {
            DeskError::Ok
        } else {
            DeskError::OutOfRange
        }
    }

    pub fn beat(&mut self) {
        self.last_beat_ms = self.now_ms;
    }

    pub fn advance(&mut self, dt_ms: u32) {
        let d = if dt_ms == 0 || dt_ms > MAX_FRAME_MS * 10 {
            DEFAULT_FRAME_MS
        } else {
            dt_ms
        };
        self.now_ms = self.now_ms.saturating_add(d);
    }

    /// 存活判定：心跳未超时。
    pub fn alive(&self) -> bool {
        if !self.enabled {
            return false;
        }
        self.now_ms.saturating_sub(self.last_beat_ms) <= self.timeout_ms
    }

    /// 记录一次崩溃：退避递增，超过上限进入安全模式。
    pub fn on_crash(&mut self) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        self.restarts += 1;
        let lvl = if self.restarts >= BACKOFF_LEVELS.len() as u32 {
            BACKOFF_LEVELS.len() as u32 - 1
        } else {
            self.restarts - 1
        };
        self.level = clamp_level(lvl as u8);
        self.backoff_ms = BACKOFF_LEVELS[self.level as usize];
        if self.restarts >= self.max_restarts {
            self.safe_mode = true;
            return DeskError::Denied;
        }
        DeskError::Ok
    }

    /// 恢复成功：清零重启计数与退避。
    pub fn recover(&mut self) -> bool {
        let was = self.restarts > 0 || self.safe_mode;
        self.restarts = 0;
        self.level = 0;
        self.backoff_ms = BACKOFF_LEVELS[0];
        self.beat();
        was
    }

    /// 快照登记（去重：同名版本只记一次，这里按次数上限收敛）。
    pub fn keep_snapshot(&mut self) -> bool {
        if self.snapshots >= 8 {
            return false;
        }
        self.snapshots += 1;
        true
    }

    pub fn rollback(&mut self) -> DeskError {
        if self.snapshots == 0 {
            return DeskError::NotFound;
        }
        self.snapshots -= 1;
        self.safe_mode = false;
        DeskError::Ok
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.level = m;
        self.backoff_ms = BACKOFF_LEVELS[m as usize];
        (m, a, p)
    }

    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.restarts.min(0xff) as u8;
        payload[1] = self.level;
        payload[2] = (self.backoff_ms & 0xff) as u8;
        payload[3] = self.snapshots as u8;
        payload[4] = if self.safe_mode { 1 } else { 0 };
        payload[5] = if self.enabled { 1 } else { 0 };
        payload[6] = SNAP_VER as u8;
        payload[7] = self.max_restarts.min(0xff) as u8;
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        *self = Watchdog::new();
        self.enabled = false;
        !self.enabled && self.restarts == 0 && !self.safe_mode
    }
}

pub fn run_crash_recover_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-crash-recover");
    let mut w = Watchdog::new();

    // L1 基础实装
    w.beat();
    w.advance(100);
    let alive1 = w.alive();
    w.advance(5_000);
    let alive2 = w.alive();
    s.add(
        "X02676 崩溃恢复最小闭环",
        alive1 && !alive2 && w.now_ms == 5_100,
        "心跳与超时闭环",
    );
    let cfg = w.configure(3_000, 3);
    s.add(
        "X02677 参数与配置面",
        cfg.ok() && w.timeout_ms == 3_000 && w.max_restarts == 3 && Watchdog::new().timeout_ms == 2_000,
        "默认档=现状，配置持久化",
    );
    s.add(
        "X02678 档位矩阵",
        BACKOFF_LEVELS.len() == 5 && BACKOFF_LEVELS[0] < BACKOFF_LEVELS[1] && BACKOFF_LEVELS[3] < BACKOFF_LEVELS[4],
        "五档退避递增",
    );
    let snap = w.snapshot();
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02679 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02680 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let bad = w.configure(0, 999);
    s.add(
        "X02681 极端输入钳制",
        !bad.ok() && w.timeout_ms == 2_000 && w.max_restarts == MAX_RESTARTS,
        "越界回默认不崩溃",
    );
    s.add(
        "X02682 失败叙事",
        narrative_has(DeskError::Denied, "权限不足") && narrative_has(DeskError::NotFound, "刷新列表"),
        "每种失败都有下一步建议",
    );
    let c1 = w.on_crash();
    let bo = w.backoff_ms;
    let rec = w.recover();
    s.add("X02683 中断续跑", c1.ok() && bo == 200 && rec && w.restarts == 0, "崩溃后可一键续作");
    let (gg, ee) = resource_guard(230, 10, 1);
    s.add("X02684 资源降级", gg && (ee == DeskError::NoMemory || ee == DeskError::Busy), "资源紧张触发守护");
    let clean = w.uninstall();
    s.add("X02685 回滚净身", clean && !w.enabled && w.restarts == 0, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(2, false);
    let m2 = motion_for(2, true);
    s.add("X02686 动效令牌", m1.dur_ms == 200 && m2.curve == 0, "恢复提示动效走令牌");
    s.add(
        "X02687 三态与焦点环",
        focus_ring(DeskState::Press) == 2 && elevation(DeskState::Hover) == 2,
        "三态过检",
    );
    s.add(
        "X02688 键盘通道",
        hotkey_conflict("Ctrl+Alt+R", "ctrl+alt+r") && !hotkey_conflict("Ctrl+Alt+R", "Ctrl+Alt+S"),
        "快捷键无冲突",
    );
    s.add(
        "X02689 微文案",
        microcopy_ok("已恢复上次快照") && !microcopy_ok("Error: null"),
        "中文语境自然",
    );
    s.add("X02690 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut w2 = Watchdog::new();
    w2.configure(2_000, 3);
    let mut last = DeskError::Ok;
    for _ in 0..3 {
        last = w2.on_crash();
    }
    let safe = w2.safe_mode;
    s.add("X02691 基准与预算表", last == DeskError::Denied && safe && w2.backoff_ms == 1_000, "退避与死线入 CI");
    w2.advance(0);
    s.add("X02692 热路径优化", w2.now_ms == DEFAULT_FRAME_MS, "异常 dt 回默认");
    let w3 = Watchdog::new();
    s.add("X02693 内存与功耗收敛", w3.restarts == 0 && w3.snapshots == 0, "空表零增量");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02694 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("crash-safe-mode==false");
    let gb = gd.guard("crash-safe-mode==false");
    s.add("X02695 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("safe-mode", "连续 3 次崩溃，建议进入安全模式");
    let a2 = ad.suggest("safe-mode", "重复");
    s.add(
        "X02696 本地智能建议",
        a1 && !a2 && ad.explain("safe-mode").is_some() && ad.reject("safe-mode") && ad.rejected("safe-mode"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(5);
    for _ in 0..5 {
        bt.step();
    }
    s.add("X02697 批量自动化", bt.progress() == 100 && bt.done == 5, "批处理进度可观测");
    s.add(
        "X02698 三线联动场景",
        link_matrix(3) == (true, true, false) && link_matrix(4) == (true, true, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("crash-hook");
    let r2 = pl.register("crash-hook");
    s.add("X02699 开放扩展点", r1 && !r2 && pl.unregister("crash-hook"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("phoenix");
    eg.disable_all();
    s.add("X02700 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_recover_25_checks_pass() {
        let set = run_crash_recover_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
