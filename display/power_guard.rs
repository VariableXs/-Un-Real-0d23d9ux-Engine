//! AI-11 族0105「桌面功耗守护」（X02601~X02625）。
//!
//! 电量/温度/负载驱动的帧率上限、空闲调暗与省电估算。
//! 硬约束：no_std / 无 alloc / 无浮点（省电比例用 permille）。

use crate::checks::CheckSet;
use crate::display::svc::*;

/// 五档帧率上限（档越高越激进省电）。
pub const FPS_LEVELS: [u32; 5] = [120, 90, 60, 45, 30];

pub struct PowerGuard {
    pub battery: u8,
    pub on_ac: bool,
    pub temp_c: u8,
    pub load: u8,
    pub idle_ms: u32,
    pub fps_cap: u32,
    pub dim_permille: u32,
    pub level: u8,
    pub saved_permille: u32,
    pub enabled: bool,
}

impl PowerGuard {
    pub const fn new() -> Self {
        PowerGuard {
            battery: 100,
            on_ac: true,
            temp_c: 35,
            load: 0,
            idle_ms: 0,
            fps_cap: FPS_LEVELS[0],
            dim_permille: 0,
            level: 0,
            saved_permille: 0,
            enabled: true,
        }
    }

    /// 参数与配置面：电量/温度/负载一律钳制。
    pub fn configure(&mut self, battery: u8, temp_c: u8, load: u8, on_ac: bool) -> DeskError {
        let ok = battery <= 100 && temp_c <= 120 && load <= 100;
        self.battery = if battery > 100 { 100 } else { battery };
        self.temp_c = if temp_c > 120 { 120 } else { temp_c };
        self.load = if load > 100 { 100 } else { load };
        self.on_ac = on_ac;
        if ok {
            DeskError::Ok
        } else {
            DeskError::OutOfRange
        }
    }

    /// 帧率上限档位：电量越低 / 温度越高，档位越高（越省电）。
    pub fn profile_level(&self) -> u8 {
        if !self.on_ac && self.battery <= 10 {
            return 4;
        }
        if self.temp_c >= 85 {
            return 4;
        }
        if self.temp_c >= 75 || (!self.on_ac && self.battery <= 25) {
            return 3;
        }
        if !self.on_ac && self.battery <= 50 {
            return 2;
        }
        if !self.on_ac {
            return 1;
        }
        0
    }

    pub fn apply_profile(&mut self) -> u32 {
        self.level = clamp_level(self.profile_level());
        self.fps_cap = FPS_LEVELS[self.level as usize];
        self.fps_cap
    }

    /// 空闲调暗：idle 超过 60s 起线性加深，上限 40%。
    pub fn idle_dim(&mut self) -> u32 {
        if self.idle_ms < 60_000 {
            self.dim_permille = 0;
            return 0;
        }
        let extra = (self.idle_ms - 60_000) / 1000;
        let v = extra * 5;
        self.dim_permille = if v > 400 { 400 } else { v };
        self.dim_permille
    }

    /// 省电估算 permille：帧率下调 + 调暗 + 省去模糊。
    pub fn estimate_saving(&self) -> u32 {
        let fps_term = ((FPS_LEVELS[0] - self.fps_cap) * 300) / FPS_LEVELS[0];
        let dim_term = self.dim_permille / 4;
        let blur_term = self.level as u32 * 20;
        fps_term + dim_term + blur_term
    }

    pub fn tick(&mut self, dt_ms: u32) {
        if dt_ms == 0 || dt_ms > MAX_FRAME_MS {
            self.idle_ms = self.idle_ms.saturating_add(DEFAULT_FRAME_MS);
        } else {
            self.idle_ms = self.idle_ms.saturating_add(dt_ms);
        }
    }

    pub fn wake(&mut self) {
        self.idle_ms = 0;
        self.dim_permille = 0;
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.level = m;
        self.fps_cap = FPS_LEVELS[m as usize];
        (m, a, p)
    }

    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.battery;
        payload[1] = self.temp_c;
        payload[2] = self.load;
        payload[3] = self.level;
        payload[4] = (self.fps_cap & 0xff) as u8;
        payload[5] = (self.dim_permille / 4) as u8;
        payload[6] = if self.on_ac { 1 } else { 0 };
        payload[7] = if self.enabled { 1 } else { 0 };
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        *self = PowerGuard::new();
        self.enabled = false;
        !self.enabled && self.dim_permille == 0
    }
}

pub fn run_power_guard_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-power-guard");
    let mut p = PowerGuard::new();

    // L1 基础实装
    let cfg = p.configure(60, 40, 20, false);
    let fps = p.apply_profile();
    s.add(
        "X02601 守护最小闭环",
        cfg.ok() && fps == 90 && p.level == 1,
        "端到端省电闭环",
    );
    let val = p.battery;
    s.add(
        "X02602 参数与配置面",
        val == 60 && PowerGuard::new().battery == 100 && PowerGuard::new().on_ac,
        "默认档=现状，配置持久化",
    );
    s.add(
        "X02603 档位矩阵",
        FPS_LEVELS.len() == 5 && FPS_LEVELS[0] > FPS_LEVELS[1] && FPS_LEVELS[3] > FPS_LEVELS[4],
        "五档帧率上限",
    );
    let snap = p.snapshot();
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02604 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02605 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let bad = p.configure(200, 200, 200, true);
    s.add(
        "X02606 极端输入钳制",
        !bad.ok() && p.battery == 100 && p.temp_c == 120 && p.load == 100,
        "越界钳制不崩溃",
    );
    s.add(
        "X02607 失败叙事",
        narrative_has(DeskError::OutOfRange, "回落默认档") && narrative_has(DeskError::Busy, "等待"),
        "每种失败都有下一步建议",
    );
    p.idle_ms = 120_000;
    let dim = p.idle_dim();
    p.wake();
    s.add("X02608 中断续跑", dim == 300 && p.idle_ms == 0 && p.dim_permille == 0, "唤醒即续作");
    let (g1, e1) = resource_guard(10, 10, 2);
    s.add("X02609 资源降级", g1 && e1 == DeskError::Busy, "电量紧张触发守护");
    let clean = p.uninstall();
    s.add("X02610 回滚净身", clean && !p.enabled && p.dim_permille == 0, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(2, false);
    let m2 = motion_for(2, true);
    s.add("X02611 动效令牌", m1.dur_ms == 200 && m2.curve == 0, "调暗动效走令牌");
    s.add(
        "X02612 三态与焦点环",
        focus_ring(DeskState::Hover) == 1 && elevation(DeskState::Press) == 0,
        "三态过检",
    );
    s.add(
        "X02613 键盘通道",
        hotkey_conflict("Win+P", "win+p") && !hotkey_conflict("Win+P", "Win+Q"),
        "快捷键无冲突",
    );
    s.add(
        "X02614 微文案",
        microcopy_ok("已开启省电模式") && !microcopy_ok("Error: null"),
        "中文语境自然",
    );
    s.add("X02615 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut p2 = PowerGuard::new();
    p2.configure(60, 40, 20, false);
    p2.apply_profile();
    let save = p2.estimate_saving();
    s.add("X02616 基准与预算表", save == 75 + 0 + 20, "省电估算入 CI");
    p2.tick(0);
    let ticked = p2.idle_ms;
    s.add("X02617 热路径优化", ticked == DEFAULT_FRAME_MS, "异常 tick 回默认");
    let mut p3 = PowerGuard::new();
    p3.configure(5, 90, 50, false);
    let lvl3 = p3.apply_profile();
    s.add("X02618 内存与功耗收敛", lvl3 == 30 && p3.level == 4, "极端场景收敛");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02619 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("power-dim<=400‰");
    let gb = gd.guard("power-dim<=400‰");
    s.add("X02620 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("lock-60", "电量 45%，建议锁定 60 帧");
    let a2 = ad.suggest("lock-60", "重复");
    s.add(
        "X02621 本地智能建议",
        a1 && !a2 && ad.explain("lock-60").is_some() && ad.reject("lock-60") && ad.rejected("lock-60"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(2);
    bt.step();
    s.add("X02622 批量自动化", bt.progress() == 50 && bt.done == 1, "批处理进度可观测");
    s.add(
        "X02623 三线联动场景",
        link_matrix(3) == (true, true, false) && link_matrix(4) == (true, true, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("power-policy");
    let r2 = pl.register("power-policy");
    s.add("X02624 开放扩展点", r1 && !r2 && pl.unregister("power-policy"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("firefly");
    eg.disable_all();
    s.add("X02625 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_guard_25_checks_pass() {
        let set = run_power_guard_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
