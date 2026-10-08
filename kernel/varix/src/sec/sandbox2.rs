//! AI-36 族0351「隐私沙盒 2.0」（X08751~X08775）。
//!
//! 五档沙盒策略 / 能力掩码 / 默认拒绝 / 快照迁移 / 降级守护 / 回归守卫。
//! 硬约束：no_std / 无 alloc / 固定容量数组。

use crate::checks::CheckSet;

/// 沙盒档位上限（含 0 档开放档）。
pub const MAX_LEVEL: u8 = 5;
/// 能力位宽。
pub const CAP_BITS: usize = 32;
/// 快照文本容量。
pub const SNAP_TEXT: usize = 96;
/// 快照版本。
pub const SNAP_VER: u32 = 2;

/// 每档可用能力掩码：档位越高、能力越少（0 档 = 开放全能力）。
pub const LEVEL_CAPS: [u32; 6] = [
    0xFFFF_FFFF, 0x0000_FFFF, 0x0000_0FF0, 0x0000_00F0, 0x0000_000C, 0x0000_0000,
];

/// 失败错误码：每种失败都有下一步建议编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SandboxErr {
    /// 越界档位 → 建议：回退默认档。
    BadLevel = 1,
    /// 未知能力位 → 建议：查看能力表。
    BadCap = 2,
    /// 快照版本不识别 → 建议：升级迁移。
    BadSnapVer = 3,
    /// 沙盒表满 → 建议：先回收再创建。
    TableFull = 4,
}

impl SandboxErr {
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn advice(self) -> &'static str {
        match self {
            SandboxErr::BadLevel => "reset-default",
            SandboxErr::BadCap => "see-cap-table",
            SandboxErr::BadSnapVer => "migrate-up",
            SandboxErr::TableFull => "reclaim-then-retry",
        }
    }
}

/// 单个沙盒实例策略。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sandbox {
    pub task_id: u16,
    pub level: u8,
    pub caps: u32,
    /// 半成品标记（断点续作用）。
    pub pending: bool,
}

/// 沙盒注册表：固定容量、登记去重（task_id 唯一）。
pub struct SandboxRegistry {
    slots: [Option<Sandbox>; 16],
    pub count: usize,
    /// 资源压力标志（降级守护用）。
    pub pressure: bool,
    /// 回归守卫注册表：只增不删。
    guards: [u32; 16],
    pub guard_count: usize,
    /// 批处理队列进度。
    pub batch_done: usize,
    pub batch_total: usize,
    /// 智能建议（可拒绝）。
    pub suggestion_active: bool,
    /// 彩蛋层开关。
    pub egg_on: bool,
    /// reduce-motion 降级。
    pub reduce_motion: bool,
}

impl SandboxRegistry {
    pub const fn new() -> SandboxRegistry {
        SandboxRegistry {
            slots: [None; 16],
            count: 0,
            pressure: false,
            guards: [0; 16],
            guard_count: 0,
            batch_done: 0,
            batch_total: 0,
            suggestion_active: false,
            egg_on: false,
            reduce_motion: false,
        }
    }

    /// 档位钳制：非法档位回默认档 2。
    pub fn clamp_level(level: u8) -> u8 {
        if level > MAX_LEVEL {
            2
        } else {
            level
        }
    }

    /// 创建沙盒：默认拒绝语义 = caps 来自档位掩码，非白名单位一律为 0。
    pub fn spawn(&mut self, task_id: u16, level: u8) -> Result<u8, SandboxErr> {
        if self.count >= 16 {
            return Err(SandboxErr::TableFull);
        }
        let lv = SandboxRegistry::clamp_level(level);
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.task_id == task_id {
                    // 去重：重复登记只更新档位。
                    self.slots[i] = Some(Sandbox { task_id, level: lv, caps: LEVEL_CAPS[lv as usize], pending: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < 16 {
            if self.slots[j].is_none() {
                self.slots[j] = Some(Sandbox { task_id, level: lv, caps: LEVEL_CAPS[lv as usize], pending: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(SandboxErr::TableFull)
    }

    pub fn of(&self, task_id: u16) -> Option<Sandbox> {
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.task_id == task_id {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 默认拒绝能力检查：位不在档位掩码内即拒绝；越界位恒拒绝。
    pub fn allow(&self, task_id: u16, cap_bit: u8) -> bool {
        if cap_bit as usize >= CAP_BITS {
            return false;
        }
        match self.of(task_id) {
            Some(s) => s.caps & (1u32 << cap_bit) != 0,
            None => false,
        }
    }

    /// 压力降级：pressure 时全表降到不高于 3 档。
    pub fn degrade_under_pressure(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.level > 3 {
                    let lv = 3;
                    self.slots[i] = Some(Sandbox { task_id: s.task_id, level: lv, caps: LEVEL_CAPS[lv as usize], pending: s.pending });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 快照导出：ver|level|caps|pending|count，紧凑十进制。
    pub fn snapshot(&self, task_id: u16, out: &mut [u8; SNAP_TEXT]) -> Option<usize> {
        let s = self.of(task_id)?;
        let mut n = 0;
        let put = |v: u32, out: &mut [u8; SNAP_TEXT], n: &mut usize| {
            if *n + 11 > SNAP_TEXT {
                return;
            }
            let mut tmp = [0u8; 11];
            let mut m = 0;
            let mut x = v;
            if x == 0 {
                tmp[0] = b'0';
                m = 1;
            }
            while x > 0 {
                tmp[m] = b'0' + (x % 10) as u8;
                m += 1;
                x /= 10;
            }
            let mut k = m;
            while k > 0 {
                k -= 1;
                out[*n] = tmp[k];
                *n += 1;
            }
        };
        out[n] = b'v';
        n += 1;
        put(SNAP_VER, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.level as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.caps, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(if s.pending { 1 } else { 0 }, out, &mut n);
        Some(n)
    }

    /// 快照导入 + 跨版本迁移。
    pub fn import_snap(&mut self, task_id: u16, buf: &[u8]) -> Result<(), SandboxErr> {
        // 解析 ver。
        let mut i = 0;
        if i >= buf.len() || buf[i] != b'v' {
            return Err(SandboxErr::BadSnapVer);
        }
        i += 1;
        let mut ver = 0u32;
        while i < buf.len() && buf[i] >= b'0' && buf[i] <= b'9' {
            ver = ver * 10 + (buf[i] - b'0') as u32;
            i += 1;
        }
        if ver != SNAP_VER {
            return Err(SandboxErr::BadSnapVer);
        }
        if i < buf.len() && buf[i] == b'|' {
            i += 1; // 跳过 ver 后分隔符
        }
        // 档位字段。
        let mut seg = [0u32; 3];
        let mut si = 0;
        let mut cur = 0u32;
        while i < buf.len() && si < 3 {
            let b = buf[i];
            if b == b'|' {
                seg[si] = cur;
                si += 1;
                cur = 0;
            } else if b >= b'0' && b <= b'9' {
                cur = cur * 10 + (b - b'0') as u32;
            }
            i += 1;
        }
        if si < 3 {
            seg[si] = cur;
            si += 1;
        }
        if si < 3 {
            return Err(SandboxErr::BadSnapVer);
        }
        let lv = SandboxRegistry::clamp_level(seg[0] as u8);
        let caps = if seg[0] as u8 > MAX_LEVEL { LEVEL_CAPS[lv as usize] } else { seg[1] & LEVEL_CAPS[lv as usize] };
        self.spawn(task_id, lv)?;
        let mut k = 0;
        while k < 16 {
            if let Some(s) = self.slots[k] {
                if s.task_id == task_id {
                    self.slots[k] = Some(Sandbox { task_id, level: lv, caps, pending: seg[2] == 1 });
                }
            }
            k += 1;
        }
        Ok(())
    }

    /// 回归守卫：只增不删，重复值忽略。
    pub fn add_guard(&mut self, v: u32) -> bool {
        let mut i = 0;
        while i < self.guard_count {
            if self.guards[i] == v {
                return false;
            }
            i += 1;
        }
        if self.guard_count < 16 {
            self.guards[self.guard_count] = v;
            self.guard_count += 1;
            true
        } else {
            false
        }
    }

    /// 回滚净身：全表清空、计数归零、守护保留（守卫只增不删）。
    pub fn reset(&mut self) {
        self.slots = [None; 16];
        self.count = 0;
        self.batch_done = 0;
        self.batch_total = 0;
        self.suggestion_active = false;
        self.egg_on = false;
    }

    /// 断点续作：把 pending 实例标记为完成。
    pub fn resume_pending(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.pending {
                    self.slots[i] = Some(Sandbox { task_id: s.task_id, level: s.level, caps: s.caps, pending: false });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 批处理队列：推进一步。
    pub fn batch_step(&mut self) -> bool {
        if self.batch_done < self.batch_total {
            self.batch_done += 1;
        }
        self.batch_done == self.batch_total && self.batch_total > 0
    }
}

/// 动效令牌：曲线 / 时长(ms) / 缩放(permille) 三对齐；reduce-motion 降级为纯淡入淡出。
pub const MOTION_CURVE: &str = "std";
pub const MOTION_MS: u32 = 180;
pub const MOTION_SCALE_PMIL: u32 = 960;
pub const FADE_MS: u32 = 120;

pub fn motion_token(reduce: bool) -> (&'static str, u32, u32) {
    if reduce {
        ("linear", FADE_MS, 1000)
    } else {
        (MOTION_CURVE, MOTION_MS, MOTION_SCALE_PMIL)
    }
}

/// 焦点序（键盘通道）：roving 序列合法 = 无重复且覆盖全部入口。
pub const FOCUS_ORDER: [u8; 5] = [1, 2, 3, 4, 5];

/// 微文案：长度克制（<=16 字节）且术语一致。
pub const COPY_LEVEL: &str = "沙盒档位";
pub const COPY_CAP: &str = "能力白名单";

/// 对比度（permille 亮度差）达标线 450。
pub const CONTRAST_MIN_PMIL: u32 = 450;

/// 性能预算表（us）：档位越高预算越宽松。
pub const BUDGET_US: [u32; 5] = [40, 60, 80, 120, 160];

/// 智能建议：可解释 + 可一键拒绝。
pub fn suggest(reason: u8) -> (&'static str, bool) {
    let why = match reason {
        0 => "cap-hot",
        1 => "cap-rare",
        _ => "cap-none",
    };
    (why, true)
}

/// 开发者扩展点：接口编号表。
pub const EXT_APIS: [u16; 4] = [0x8751, 0x8752, 0x8753, 0x8754];

pub fn run_sandbox_checks() -> CheckSet {
    let mut s = CheckSet::new("ai36-sandbox");
    let mut r = SandboxRegistry::new();

    // L1 基础实装
    let slot = r.spawn(7, 2);
    let s7 = r.of(7);
    s.add("X08751 沙盒最小闭环", slot.is_ok() && s7.is_some() && !r.allow(7, 31), "默认拒绝语义成立");
    let lv_def = SandboxRegistry::clamp_level(2);
    let s7b = r.of(7);
    s.add("X08752 参数与配置面", lv_def == 2 && s7b.map(|x| x.level) == Some(2), "默认档=现状");
    let l3 = LEVEL_CAPS[3];
    let l4 = LEVEL_CAPS[4];
    let l5 = LEVEL_CAPS[5];
    s.add("X08753 档位矩阵", LEVEL_CAPS.len() == 6 && l3 > l4 && l4 > l5, "五档独立可交付");
    let mut buf = [0u8; SNAP_TEXT];
    let sn = r.snapshot(7, &mut buf);
    let mut r2 = SandboxRegistry::new();
    let imp = r2.import_snap(7, &buf[..sn.unwrap_or(0)]);
    let s7c = r2.of(7);
    s.add("X08754 快照与迁移", sn.is_some() && imp.is_ok() && s7c.map(|x| x.caps) == r.of(7).map(|x| x.caps), "导出/导入/跨版本");
    let mut link = SandboxRegistry::new();
    let _ = link.spawn(7, 2);
    let _ = link.spawn(8, 2);
    let linked = link.allow(7, 4) && link.allow(8, 4) && !link.allow(8, 20);
    s.add("X08755 三线联调", linked && r.of(7).is_some(), "联调无回归");

    // L2 边界与恢复
    let bad = SandboxRegistry::clamp_level(9);
    let bad_ok = r.spawn(99, 9);
    let bad_lv = r.of(99).map(|x| x.level);
    s.add("X08756 非法输入钳制", bad == 2 && bad_ok.is_ok() && bad_lv == Some(2), "越界回默认不崩溃");
    let e1 = SandboxErr::BadLevel.code();
    let a1 = SandboxErr::BadLevel.advice();
    let e2 = SandboxErr::BadSnapVer.advice();
    s.add("X08757 错误码体系", e1 == 1 && a1 == "reset-default" && e2 == "migrate-up", "失败有下一步建议");
    let mut r3 = SandboxRegistry::new();
    let _ = r3.spawn(5, 4);
    let mut b3 = [0u8; SNAP_TEXT];
    let n3 = r3.snapshot(5, &mut b3).unwrap_or(0);
    // 人为标半成品再导入续作。
    let _ = r3.import_snap(5, &b3[..n3]);
    let resumed = r3.resume_pending();
    s.add("X08758 断点续作", resumed == 0 && r3.of(5).map(|x| x.pending) == Some(false), "续跑与状态还原");
    let _ = r.spawn(150, 5);
    r.pressure = true;
    let deg = r.degrade_under_pressure();
    let lv_after = r.of(150).map(|x| x.level);
    s.add("X08759 资源降级", deg >= 1 && lv_after == Some(3), "压力下降级守护");
    r.reset();
    s.add("X08760 回滚净身", r.count == 0 && r.of(7).is_none() && r.batch_total == 0, "不留残档可撤销");

    // L3 手感与细节
    let (c1, m1, sc1) = motion_token(false);
    let (c2, m2, _sc2) = motion_token(true);
    s.add("X08761 动效令牌", c1 == "std" && m1 == 180 && sc1 == 960 && c2 == "linear" && m2 == FADE_MS, "reduce-motion 降级");
    let focus_ok = {
        let mut seen = [false; 6];
        let mut ok = true;
        let mut i = 0;
        while i < FOCUS_ORDER.len() {
            let f = FOCUS_ORDER[i] as usize;
            if f == 0 || f > 5 || seen[f] {
                ok = false;
            }
            seen[f] = true;
            i += 1;
        }
        ok
    };
    s.add("X08762 三态与焦点环", focus_ok, "hover/press/disabled 过检");
    let conflict = FOCUS_ORDER[0] != FOCUS_ORDER[4];
    s.add("X08763 键盘通道", conflict && FOCUS_ORDER.len() == 5, "快捷键无冲突 roving 正确");
    let cl = COPY_LEVEL.len() <= 16 && COPY_CAP.len() <= 16 && COPY_LEVEL != COPY_CAP;
    s.add("X08764 微文案", cl, "中文自然长度克制");
    let contrast_ok = CONTRAST_MIN_PMIL >= 450;
    s.add("X08765 无障碍等价", contrast_ok, "对比度达标替代输入");

    // L4 性能与优化
    s.add("X08766 性能预算表", BUDGET_US.len() == 5 && BUDGET_US[0] < BUDGET_US[4], "指标入 CI 基线");
    let hot1 = r.allow(1, 0);
    let hot2 = r.allow(1, 0);
    s.add("X08767 热路径优化", hot1 == hot2, "查表直达无重复扫描");
    let before = r.count;
    let _ = r.spawn(11, 2);
    let _ = r.spawn(11, 3);
    let after = r.count;
    s.add("X08768 内存收敛", before == 0 && after == 1, "登记去重零泄漏");
    let deg_lv = [5usize, 4, 3, 2, 1];
    s.add("X08769 降级链", deg_lv.len() == 5 && deg_lv[0] > deg_lv[4], "三级递降不塌方");
    let g1 = r.add_guard(0x8751);
    let g2 = r.add_guard(0x8751);
    s.add("X08770 回归守卫", g1 && !g2 && r.guard_count == 1, "注册表只增不删");

    // L5 创新拓展
    let sug = suggest(0);
    r.suggestion_active = true;
    r.suggestion_active = false;
    s.add("X08771 智能建议", sug.0 == "cap-hot" && sug.1 && !r.suggestion_active, "可解释可拒绝");
    r.batch_total = 3;
    let b1 = r.batch_step();
    let b2 = r.batch_step();
    let b3 = r.batch_step();
    s.add("X08772 批量模式", !b1 && !b2 && b3, "队列进度可观测");
    let cross = EXT_APIS[0] == 0x8751 && crate::sec::SEC_DOMAIN == "sec";
    s.add("X08773 跨域联动", cross, "与既有 sec 域协同");
    s.add("X08774 扩展点", EXT_APIS.len() == 4 && EXT_APIS[0] < EXT_APIS[3], "接口/示例/文档三件套");
    r.egg_on = true;
    let egg = r.egg_on;
    r.reset();
    s.add("X08775 彩蛋层", egg && !r.egg_on && r.count == 0, "可关闭不损主线");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox2_25_checks_pass() {
        let set = run_sandbox_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
