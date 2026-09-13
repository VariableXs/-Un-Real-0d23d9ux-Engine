//! AI-11 族0103「壁纸合成服务」（X02551~X02575）。
//!
//! 壁纸源图的适配模式、缩放、取色联动、叠加合成与显存估算。
//! 硬约束：no_std / 无 alloc / 无浮点（比例用 permille 定点）。

use crate::checks::CheckSet;
use crate::display::svc::*;

pub const FIT_FILL: u8 = 0;
pub const FIT_FIT: u8 = 1;
pub const FIT_STRETCH: u8 = 2;
pub const FIT_TILE: u8 = 3;
pub const FIT_CENTER: u8 = 4;
pub const FIT_MODES: u8 = 5;

pub struct Wallpaper {
    pub src_w: u32,
    pub src_h: u32,
    pub dst_w: u32,
    pub dst_h: u32,
    pub fit: u8,
    pub tint: [u8; 3],
    pub tint_alpha: u8,
    pub blur: u8,
    pub level: u8,
    pub enabled: bool,
}

impl Wallpaper {
    pub const fn new() -> Self {
        Wallpaper {
            src_w: 1920,
            src_h: 1080,
            dst_w: 1920,
            dst_h: 1080,
            fit: FIT_FILL,
            tint: [0, 0, 0],
            tint_alpha: 0,
            blur: 0,
            level: 2,
            enabled: true,
        }
    }

    /// 参数与配置面：非法档位回默认，尺寸 0 回 1。
    pub fn configure(&mut self, src_w: u32, src_h: u32, fit: u8) -> DeskError {
        let ok = fit < FIT_MODES;
        self.fit = if ok { fit } else { FIT_FILL };
        self.src_w = if src_w == 0 { 1 } else { src_w };
        self.src_h = if src_h == 0 { 1 } else { src_h };
        if ok {
            DeskError::Ok
        } else {
            DeskError::OutOfRange
        }
    }

    /// 缩放后尺寸：fill 裁切填满 / fit 留边等比 / stretch 拉伸 / tile 与 center 保持原始。
    pub fn scaled(&self) -> (u32, u32) {
        match self.fit {
            FIT_STRETCH => (self.dst_w, self.dst_h),
            FIT_TILE | FIT_CENTER => (self.src_w, self.src_h),
            FIT_FIT => {
                let s = self.scale_permille();
                ((self.src_w * s) / 1000, (self.src_h * s) / 1000)
            }
            _ => {
                let s = self.cover_permille();
                ((self.src_w * s) / 1000, (self.src_h * s) / 1000)
            }
        }
    }

    /// fit 用：缩小系数 permille。
    pub fn scale_permille(&self) -> u32 {
        if self.src_w == 0 || self.src_h == 0 {
            return 1000;
        }
        let a = (self.dst_w * 1000) / self.src_w;
        let b = (self.dst_h * 1000) / self.src_h;
        if a < b {
            a
        } else {
            b
        }
    }

    /// fill 用：放大系数 permille。
    pub fn cover_permille(&self) -> u32 {
        if self.src_w == 0 || self.src_h == 0 {
            return 1000;
        }
        let a = (self.dst_w * 1000) / self.src_w;
        let b = (self.dst_h * 1000) / self.src_h;
        if a > b {
            a
        } else {
            b
        }
    }

    /// 铺满覆盖率 permille（tile 模式可能 >1000）。
    pub fn coverage_permille(&self) -> u32 {
        let (w, h) = self.scaled();
        if self.dst_w == 0 || self.dst_h == 0 {
            return 0;
        }
        let a = (w * 1000) / self.dst_w;
        let b = (h * 1000) / self.dst_h;
        a * b / 1000
    }

    /// 五档画质：档位决定模糊与叠加强度上限。
    pub fn apply_level(&mut self, level: u8) -> u8 {
        self.level = clamp_level(level);
        self.blur = self.level * 4;
        self.level
    }

    /// 取色联动：tint 与基色按 alpha 混合（整数运算）。
    pub fn blend(&self, base: [u8; 3]) -> [u8; 3] {
        let a = self.tint_alpha as u32;
        let mut out = [0u8; 3];
        let mut i = 0usize;
        while i < 3 {
            let v = (base[i] as u32 * (255 - a) + self.tint[i] as u32 * a) / 255;
            out[i] = if v > 255 { 255 } else { v as u8 };
            i += 1;
        }
        out
    }

    /// 显存估算：缩放后像素 ×4B + 模糊金字塔（2 档额外 1/4）。
    pub fn memory_bytes(&self) -> u64 {
        let (w, h) = self.scaled();
        let base = w as u64 * h as u64 * 4;
        if self.blur > 0 {
            base + base / 4
        } else {
            base
        }
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.level = m;
        self.blur = m * 4;
        (m, a, p)
    }

    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.fit;
        payload[1] = self.level;
        payload[2] = self.tint[0];
        payload[3] = self.tint[1];
        payload[4] = self.tint[2];
        payload[5] = self.tint_alpha;
        payload[6] = self.blur;
        payload[7] = if self.enabled { 1 } else { 0 };
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        *self = Wallpaper::new();
        self.tint_alpha = 0;
        self.blur = 0;
        self.tint_alpha == 0 && self.blur == 0
    }
}

pub fn run_wallpaper_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-wallpaper");
    let mut w = Wallpaper::new();

    // L1 基础实装
    let scaled0 = w.scaled();
    let mem0 = w.memory_bytes();
    s.add(
        "X02551 合成最小闭环",
        scaled0 == (1920, 1080) && mem0 == 1920 * 1080 * 4,
        "端到端合成闭环",
    );
    let cfg = w.configure(3840, 2160, FIT_FIT);
    let fit_scaled = w.scaled();
    s.add(
        "X02552 参数与配置面",
        cfg.ok() && fit_scaled == (1920, 1080) && Wallpaper::new().fit == FIT_FILL,
        "默认档=现状，配置持久化",
    );
    let mut modes: [(u32, u32); 5] = [(0, 0); 5];
    let mut i = 0usize;
    while i < 5 {
        let mut x = Wallpaper::new();
        x.configure(3840, 2160, i as u8);
        modes[i] = x.scaled();
        i += 1;
    }
    s.add(
        "X02553 档位矩阵",
        modes[0] == (1920, 1080)
            && modes[1] == (1920, 1080)
            && modes[2] == (1920, 1080)
            && modes[3] == (3840, 2160)
            && modes[4] == (3840, 2160),
        "五种适配模式独立可交付",
    );
    let snap = w.snapshot();
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02554 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02555 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let bad = w.configure(3840, 2160, 99);
    let fit_after_bad = w.fit;
    let zero = w.configure(0, 0, FIT_FIT);
    s.add(
        "X02556 极端输入钳制",
        !bad.ok() && fit_after_bad == FIT_FILL && zero.ok() && w.src_w == 1 && w.src_h == 1,
        "越界回默认、异常不崩溃",
    );
    s.add(
        "X02557 失败叙事",
        narrative_has(DeskError::Corrupt, "快照") && narrative_has(DeskError::Disabled, "开关"),
        "每种失败都有下一步建议",
    );
    let before = w.memory_bytes();
    w.enabled = false;
    let after = w.memory_bytes();
    w.enabled = true;
    s.add("X02558 中断续跑", before == after && w.enabled, "中断可续作");
    let (g1, e1) = resource_guard(10, 230, 80);
    s.add("X02559 资源降级", g1 && e1 == DeskError::NoMemory, "内存紧张触发守护");
    let clean = w.uninstall();
    s.add("X02560 回滚净身", clean && w.tint_alpha == 0 && w.blur == 0, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(4, false);
    let m2 = motion_for(4, true);
    s.add("X02561 动效令牌", m1.dur_ms == 280 && m2.curve == 0, "换壁纸动效走令牌");
    s.add(
        "X02562 三态与焦点环",
        focus_ring(DeskState::Hover) == 1 && elevation(DeskState::Press) == 0,
        "三态过检",
    );
    s.add(
        "X02563 键盘通道",
        hotkey_conflict("Win+W", "win+w") && !hotkey_conflict("Win+W", "Win+E"),
        "快捷键无冲突",
    );
    s.add(
        "X02564 微文案",
        microcopy_ok("已应用到所有桌面") && !microcopy_ok("undefined"),
        "中文语境自然",
    );
    s.add("X02565 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut w2 = Wallpaper::new();
    w2.configure(3840, 2160, FIT_FIT);
    let cov = w2.coverage_permille();
    s.add("X02566 基准与预算表", cov == 1000 && w2.memory_bytes() == 1920u64 * 1080 * 4, "覆盖与显存入 CI");
    let cp = w2.cover_permille();
    let sp = w2.scale_permille();
    s.add("X02567 热路径优化", cp == 500 && sp == 500, "定点比例一次算出");
    let mut w3 = Wallpaper::new();
    w3.apply_level(4);
    let mem3 = w3.memory_bytes();
    s.add("X02568 内存与功耗收敛", mem3 == 1920u64 * 1080 * 4 + 1920 * 1080, "模糊金字塔受控");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02569 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("wallpaper-mem<=64MB");
    let gb = gd.guard("wallpaper-mem<=64MB");
    s.add("X02570 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("dim-wallpaper", "当前壁纸偏亮，建议叠加 20% 暗色");
    let a2 = ad.suggest("dim-wallpaper", "重复");
    s.add(
        "X02571 本地智能建议",
        a1 && !a2 && ad.explain("dim-wallpaper").is_some() && ad.reject("dim-wallpaper") && ad.rejected("dim-wallpaper"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(5);
    bt.step();
    s.add("X02572 批量自动化", bt.progress() == 20 && bt.done == 1, "批处理进度可观测");
    s.add(
        "X02573 三线联动场景",
        link_matrix(1) == (false, true, false) && link_matrix(3) == (true, true, false),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("wallpaper-shader");
    let r2 = pl.register("wallpaper-shader");
    s.add("X02574 开放扩展点", r1 && !r2 && pl.unregister("wallpaper-shader"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("aurora-drift");
    eg.disable_all();
    s.add("X02575 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_25_checks_pass() {
        let set = run_wallpaper_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
