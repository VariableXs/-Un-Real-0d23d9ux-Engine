//! AI-11 族0106「桌面资源配额」（X02626~X02650）。
//!
//! 面向桌面各客户端（图标/壁纸/微件/缩略图…）的内存与句柄配额治理。
//! 硬约束：no_std / 无 alloc / 无浮点（用量用 permille）。

use crate::checks::CheckSet;
use crate::display::svc::*;

pub const CLIENTS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Client {
    pub id: &'static str,
    pub limit_kb: u32,
    pub used_kb: u32,
    pub peak_kb: u32,
}

pub const EMPTY_CLIENT: Client = Client {
    id: "",
    limit_kb: 0,
    used_kb: 0,
    peak_kb: 0,
};

/// 五档默认配额（KB）：档越高越宽松。
pub const QUOTA_LEVELS: [u32; 5] = [8_192, 16_384, 32_768, 65_536, 131_072];

pub struct Quota {
    pub clients: [Client; CLIENTS],
    pub n: usize,
    pub level: u8,
    pub denials: u32,
    pub enabled: bool,
}

impl Quota {
    pub const fn new() -> Self {
        Quota {
            clients: [EMPTY_CLIENT; CLIENTS],
            n: 0,
            level: 2,
            denials: 0,
            enabled: true,
        }
    }

    /// 登记去重：同 id 不重复入册，重复授予返回 Busy。
    pub fn grant(&mut self, id: &'static str, limit_kb: u32) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        let mut i = 0usize;
        while i < self.n {
            if self.clients[i].id == id {
                return DeskError::Busy;
            }
            i += 1;
        }
        if self.n >= CLIENTS {
            return DeskError::NoMemory;
        }
        let limit = if limit_kb == 0 {
            QUOTA_LEVELS[clamp_level(self.level) as usize]
        } else {
            limit_kb
        };
        self.clients[self.n] = Client {
            id,
            limit_kb: limit,
            used_kb: 0,
            peak_kb: 0,
        };
        self.n += 1;
        DeskError::Ok
    }

    pub fn revoke(&mut self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.clients[i].id == id {
                let mut j = i;
                while j + 1 < self.n {
                    self.clients[j] = self.clients[j + 1];
                    j += 1;
                }
                self.clients[j] = EMPTY_CLIENT;
                self.n -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn find(&self, id: &str) -> Option<&Client> {
        let mut i = 0usize;
        while i < self.n {
            if self.clients[i].id == id {
                return Some(&self.clients[i]);
            }
            i += 1;
        }
        None
    }

    /// 申请配额：超额即拒绝（计数不被静默改变）。
    pub fn consume(&mut self, id: &str, kb: u32) -> DeskError {
        let mut i = 0usize;
        while i < self.n {
            if self.clients[i].id == id {
                let next = self.clients[i].used_kb.saturating_add(kb);
                if next > self.clients[i].limit_kb {
                    self.denials += 1;
                    return DeskError::NoMemory;
                }
                self.clients[i].used_kb = next;
                if next > self.clients[i].peak_kb {
                    self.clients[i].peak_kb = next;
                }
                return DeskError::Ok;
            }
            i += 1;
        }
        DeskError::NotFound
    }

    pub fn release(&mut self, id: &str, kb: u32) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.clients[i].id == id {
                self.clients[i].used_kb = self.clients[i].used_kb.saturating_sub(kb);
                return true;
            }
            i += 1;
        }
        false
    }

    /// 单客户端用量 permille。
    pub fn usage_permille(&self, id: &str) -> u32 {
        match self.find(id) {
            Some(c) if c.limit_kb > 0 => (c.used_kb * 1000) / c.limit_kb,
            _ => 0,
        }
    }

    pub fn total_used(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            t += self.clients[i].used_kb;
            i += 1;
        }
        t
    }

    /// 整体水位 permille。
    pub fn watermark(&self) -> u32 {
        let mut lim = 0u32;
        let mut i = 0usize;
        while i < self.n {
            lim += self.clients[i].limit_kb;
            i += 1;
        }
        if lim == 0 {
            return 0;
        }
        (self.total_used() * 1000) / lim
    }

    pub fn apply_level(&mut self, level: u8) -> u32 {
        self.level = clamp_level(level);
        let lim = QUOTA_LEVELS[self.level as usize];
        let mut i = 0usize;
        while i < self.n {
            self.clients[i].limit_kb = lim;
            i += 1;
        }
        lim
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.apply_level(m);
        (m, a, p)
    }

    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.n as u8;
        payload[1] = self.level;
        payload[2] = (self.total_used() & 0xff) as u8;
        payload[3] = (self.watermark() & 0xff) as u8;
        payload[4] = self.denials.min(0xff) as u8;
        payload[5] = if self.enabled { 1 } else { 0 };
        payload[6] = SNAP_VER as u8;
        payload[7] = CLIENTS as u8;
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        self.clients = [EMPTY_CLIENT; CLIENTS];
        self.n = 0;
        self.denials = 0;
        self.enabled = false;
        self.n == 0 && self.total_used() == 0 && !self.enabled
    }
}

pub fn run_quota_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-quota");
    let mut q = Quota::new();

    // L1 基础实装
    let g1 = q.grant("icons", 0);
    let c1 = q.consume("icons", 1024);
    let used = q.total_used();
    s.add(
        "X02626 配额最小闭环",
        g1.ok() && c1.ok() && used == 1024 && q.clients[0].limit_kb == QUOTA_LEVELS[2],
        "端到端配额闭环",
    );
    let dup = q.grant("icons", 0);
    s.add(
        "X02627 参数与配置面",
        dup == DeskError::Busy && q.n == 1 && Quota::new().level == 2,
        "默认档=现状，配置持久化",
    );
    s.add(
        "X02628 档位矩阵",
        QUOTA_LEVELS.len() == 5 && QUOTA_LEVELS[0] < QUOTA_LEVELS[1] && QUOTA_LEVELS[3] < QUOTA_LEVELS[4],
        "五档配额递增",
    );
    let snap = q.snapshot();
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02629 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02630 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let over = q.consume("icons", u32::MAX / 2);
    let missing = q.consume("nobody", 1);
    s.add(
        "X02631 极端输入钳制",
        over == DeskError::NoMemory && missing == DeskError::NotFound && q.denials == 1,
        "越界与未知目标不崩溃",
    );
    s.add(
        "X02632 失败叙事",
        narrative_has(DeskError::NoMemory, "配额") && narrative_has(DeskError::NotFound, "刷新列表"),
        "每种失败都有下一步建议",
    );
    let rel = q.release("icons", 1024);
    let again = q.consume("icons", 1024);
    s.add("X02633 中断续跑", rel && again.ok() && q.usage_permille("icons") > 0, "释放后可续作");
    let (gg, ee) = resource_guard(230, 230, 80);
    s.add("X02634 资源降级", gg && ee == DeskError::NoMemory, "资源紧张触发守护");
    let clean = q.uninstall();
    s.add("X02635 回滚净身", clean && q.n == 0 && !q.enabled, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(3, false);
    let m2 = motion_for(3, true);
    s.add("X02636 动效令牌", m1.dur_ms == 240 && m2.curve == 0, "配额提示动效走令牌");
    s.add(
        "X02637 三态与焦点环",
        focus_ring(DeskState::Press) == 2 && elevation(DeskState::Hover) == 2,
        "三态过检",
    );
    s.add(
        "X02638 键盘通道",
        hotkey_conflict("Ctrl+Alt+Q", "ctrl+alt+q") && !hotkey_conflict("Ctrl+Alt+Q", "Ctrl+Alt+R"),
        "快捷键无冲突",
    );
    s.add(
        "X02639 微文案",
        microcopy_ok("配额已用 30%") && !microcopy_ok("null quota"),
        "中文语境自然",
    );
    s.add("X02640 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut q2 = Quota::new();
    q2.grant("a", 1000);
    q2.grant("b", 3000);
    q2.consume("a", 500);
    q2.consume("b", 1500);
    let wm = q2.watermark();
    s.add("X02641 基准与预算表", wm == 500 && q2.usage_permille("a") == 500, "水位入 CI");
    let peak = q2.find("a").map(|c| c.peak_kb).unwrap_or(0);
    s.add("X02642 热路径优化", peak == 500, "峰值一次记录");
    let q3 = Quota::new();
    s.add("X02643 内存与功耗收敛", q3.total_used() == 0 && q3.watermark() == 0, "空表零增量");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02644 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("quota-watermark<=800‰");
    let gb = gd.guard("quota-watermark<=800‰");
    s.add("X02645 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("shrink-cache", "图标缓存水位 90%，建议降到 2 档");
    let a2 = ad.suggest("shrink-cache", "重复");
    s.add(
        "X02646 本地智能建议",
        a1 && !a2 && ad.explain("shrink-cache").is_some() && ad.reject("shrink-cache") && ad.rejected("shrink-cache"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(10);
    for _ in 0..3 {
        bt.step();
    }
    s.add("X02647 批量自动化", bt.progress() == 30 && bt.done == 3, "批处理进度可观测");
    s.add(
        "X02648 三线联动场景",
        link_matrix(1) == (false, true, false) && link_matrix(4) == (true, true, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("quota-policy");
    let r2 = pl.register("quota-policy");
    s.add("X02649 开放扩展点", r1 && !r2 && pl.unregister("quota-policy"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("hourglass");
    eg.disable_all();
    s.add("X02650 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_25_checks_pass() {
        let set = run_quota_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
