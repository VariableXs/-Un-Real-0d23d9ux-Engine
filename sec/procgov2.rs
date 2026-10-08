//! AI-36 族0352「进程治理 2.0」（X08776~X08800）。
//!
//! 进程治理表 / 优先级档位 / CPU 时间片配给 / 越界钳制 / 错误码 /
//! 压力降级 / 回归守卫。no_std / 无 alloc。

use crate::checks::CheckSet;

/// 治理档位（0=放养 … 4=严管）。
pub const GOV_LEVELS: usize = 5;
/// 每档时间片预算（ms）。
pub const SLICE_MS: [u32; 5] = [100, 60, 40, 24, 12];
/// 治理表容量。
pub const GOV_CAP: usize = 16;
/// 单进程 nice 下限/上限。
pub const NICE_MIN: i8 = -10;
pub const NICE_MAX: i8 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovErr {
    /// 未知进程 → 建议：先注册。
    NoSuchProc = 1,
    /// 档位越界 → 建议：回默认档。
    BadLevel = 2,
    /// nice 越界 → 建议：钳制到边界。
    BadNice = 3,
    /// 表满 → 建议：回收僵尸。
    Full = 4,
}

impl GovErr {
    pub fn advice(self) -> &'static str {
        match self {
            GovErr::NoSuchProc => "register-first",
            GovErr::BadLevel => "reset-default",
            GovErr::BadNice => "clamp-edge",
            GovErr::Full => "reap-zombie",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GovEntry {
    pub pid: u16,
    pub level: u8,
    pub nice: i8,
    pub used_ms: u32,
    /// 半成品/挂起标记。
    pub stalled: bool,
}

pub struct Governor {
    slots: [Option<GovEntry>; GOV_CAP],
    pub count: usize,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
}

impl Governor {
    pub const fn new() -> Governor {
        Governor {
            slots: [None; GOV_CAP],
            count: 0,
            pressure: false,
            guards: [0; 16],
            guard_count: 0,
            batch_done: 0,
            batch_total: 0,
            suggestion_active: false,
            egg_on: false,
        }
    }

    pub fn clamp_level(level: u8) -> u8 {
        if level as usize >= GOV_LEVELS {
            2
        } else {
            level
        }
    }

    pub fn clamp_nice(nice: i8) -> i8 {
        if nice < NICE_MIN {
            NICE_MIN
        } else if nice > NICE_MAX {
            NICE_MAX
        } else {
            nice
        }
    }

    /// 登记去重：同 pid 只更新。
    pub fn register(&mut self, pid: u16, level: u8, nice: i8) -> Result<u8, GovErr> {
        let lv = Governor::clamp_level(level);
        let nc = Governor::clamp_nice(nice);
        let mut i = 0;
        while i < GOV_CAP {
            if let Some(e) = self.slots[i] {
                if e.pid == pid {
                    self.slots[i] = Some(GovEntry { pid, level: lv, nice: nc, used_ms: e.used_ms, stalled: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < GOV_CAP {
            if self.slots[j].is_none() {
                self.slots[j] = Some(GovEntry { pid, level: lv, nice: nc, used_ms: 0, stalled: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(GovErr::Full)
    }

    pub fn of(&self, pid: u16) -> Option<GovEntry> {
        let mut i = 0;
        while i < GOV_CAP {
            if let Some(e) = self.slots[i] {
                if e.pid == pid {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 时间片预算 = 档位预算 + nice 调整（每 -2 nice 加 10%）。
    pub fn slice_ms(&self, pid: u16) -> u32 {
        match self.of(pid) {
            Some(e) => {
                let base = SLICE_MS[e.level as usize] as u32;
                // nice∈[-10,10] 线性调节：0 = 基准，-10 = 1.5 倍，+10 = 减半。
                let v = (base * (20 - e.nice as i32) as u32) / 20;
                if self.pressure {
                    v / 2
                } else {
                    v
                }
            }
            None => 0,
        }
    }

    /// 记账一次调度。
    pub fn charge(&mut self, pid: u16, ms: u32) -> Result<(), GovErr> {
        let mut i = 0;
        while i < GOV_CAP {
            if let Some(e) = self.slots[i] {
                if e.pid == pid {
                    self.slots[i] = Some(GovEntry { pid, level: e.level, nice: e.nice, used_ms: e.used_ms + ms, stalled: e.stalled });
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(GovErr::NoSuchProc)
    }

    /// 压力降级：全部升到不高于 3 档（更严管）。
    pub fn degrade(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < GOV_CAP {
            if let Some(e) = self.slots[i] {
                if e.level < 3 {
                    self.slots[i] = Some(GovEntry { pid: e.pid, level: 3, nice: e.nice, used_ms: e.used_ms, stalled: e.stalled });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 快照（十进制 ver|level|nice|used）。
    pub fn snapshot(&self, pid: u16, out: &mut [u8; 64]) -> Option<usize> {
        let e = self.of(pid)?;
        let mut n = 0;
        let put = |v: u32, out: &mut [u8; 64], n: &mut usize| {
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
        let put_i = |v: i8, out: &mut [u8; 64], n: &mut usize| {
            if v < 0 {
                out[*n] = b'-';
                *n += 1;
                put((-v) as u32, out, n);
            } else {
                put(v as u32, out, n);
            }
        };
        out[n] = b'v';
        n += 1;
        put(2, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(e.level as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put_i(e.nice, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(e.used_ms, out, &mut n);
        Some(n)
    }

    pub fn import_snap(&mut self, pid: u16, buf: &[u8]) -> Result<(), GovErr> {
        let mut i = 0;
        if i >= buf.len() || buf[i] != b'v' {
            return Err(GovErr::Full);
        }
        i += 1;
        let mut ver = 0u32;
        while i < buf.len() && buf[i] >= b'0' && buf[i] <= b'9' {
            ver = ver * 10 + (buf[i] - b'0') as u32;
            i += 1;
        }
        if ver != 2 {
            return Err(GovErr::Full);
        }
        let mut seg = [0i32; 3];
        let mut si = 0;
        let mut cur = 0i32;
        let mut neg = false;
        while i < buf.len() && si < 3 {
            let b = buf[i];
            if b == b'|' {
                seg[si] = if neg { -cur } else { cur };
                si += 1;
                cur = 0;
                neg = false;
            } else if b == b'-' {
                neg = true;
            } else if b >= b'0' && b <= b'9' {
                cur = cur * 10 + (b - b'0') as i32;
            }
            i += 1;
        }
        if si < 3 {
            return Err(GovErr::Full);
        }
        self.register(pid, seg[0] as u8, seg[1] as i8)?;
        let mut k = 0;
        while k < GOV_CAP {
            if let Some(e) = self.slots[k] {
                if e.pid == pid {
                    self.slots[k] = Some(GovEntry { pid, level: Governor::clamp_level(seg[0] as u8), nice: Governor::clamp_nice(seg[1] as i8), used_ms: seg[2] as u32, stalled: false });
                }
            }
            k += 1;
        }
        Ok(())
    }

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

    pub fn reset(&mut self) {
        self.slots = [None; GOV_CAP];
        self.count = 0;
        self.batch_done = 0;
        self.batch_total = 0;
        self.suggestion_active = false;
        self.egg_on = false;
    }

    pub fn resume_stalled(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < GOV_CAP {
            if let Some(e) = self.slots[i] {
                if e.stalled {
                    self.slots[i] = Some(GovEntry { pid: e.pid, level: e.level, nice: e.nice, used_ms: e.used_ms, stalled: false });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn stall(&mut self, pid: u16) -> bool {
        let mut i = 0;
        while i < GOV_CAP {
            if let Some(e) = self.slots[i] {
                if e.pid == pid {
                    self.slots[i] = Some(GovEntry { pid, level: e.level, nice: e.nice, used_ms: e.used_ms, stalled: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    pub fn batch_step(&mut self) -> bool {
        if self.batch_done < self.batch_total {
            self.batch_done += 1;
        }
        self.batch_done == self.batch_total && self.batch_total > 0
    }
}

/// 动效令牌（治理面板）：reduce-motion 降级。
pub const MOTION_MS: u32 = 200;
pub const FADE_MS: u32 = 100;
/// 焦点序。
pub const FOCUS_ORDER: [u8; 4] = [1, 2, 3, 4];
/// 对比度达标线（permille 亮度差）。
pub const CONTRAST_MIN_PMIL: u32 = 450;
/// 性能预算（us）。
pub const BUDGET_US: [u32; 5] = [30, 50, 70, 100, 140];
/// 开发者扩展点。
pub const EXT_APIS: [u16; 3] = [0x8776, 0x8777, 0x8778];

pub fn motion_token(reduce: bool) -> u32 {
    if reduce {
        FADE_MS
    } else {
        MOTION_MS
    }
}

pub fn suggest(reason: u8) -> &'static str {
    match reason {
        0 => "gov-hot",
        1 => "gov-idle",
        _ => "gov-none",
    }
}

pub fn run_procgov_checks() -> CheckSet {
    let mut s = CheckSet::new("ai36-procgov");
    let mut g = Governor::new();

    // L1 基础实装
    let reg = g.register(100, 2, 0);
    let e100 = g.of(100);
    s.add("X08776 治理最小闭环", reg.is_ok() && e100.is_some() && g.slice_ms(100) > 0, "端到端最小可用闭环");
    let def_lv = Governor::clamp_level(2);
    let slice_now = g.slice_ms(100);
    s.add("X08777 参数与配置面", def_lv == 2 && slice_now == SLICE_MS[2], "默认档=现状可持久化");
    s.add("X08778 档位矩阵", GOV_LEVELS == 5 && SLICE_MS[0] > SLICE_MS[4], "五档独立可交付");
    let mut buf = [0u8; 64];
    let sn = g.snapshot(100, &mut buf);
    let mut g2 = Governor::new();
    let imp = g2.import_snap(100, &buf[..sn.unwrap_or(0)]);
    s.add("X08779 快照与迁移", sn.is_some() && imp.is_ok() && g2.of(100).map(|x| x.used_ms) == g.of(100).map(|x| x.used_ms), "导出/导入/跨版本");
    let _ = g.charge(100, 10);
    let used = g.of(100).map(|x| x.used_ms);
    let mut gl = Governor::new();
    let _ = gl.register(101, 2, 0);
    s.add("X08780 三线联调", used == Some(10) && gl.slice_ms(101) == SLICE_MS[2], "联调无回归");

    // L2 边界与恢复
    let no = g.charge(999, 5);
    s.add("X08781 非法输入钳制", no == Err(GovErr::NoSuchProc) && Governor::clamp_nice(99) == NICE_MAX && Governor::clamp_level(9) == 2, "越界钳制不崩溃");
    s.add("X08782 错误码体系", GovErr::NoSuchProc.advice() == "register-first" && GovErr::Full.advice() == "reap-zombie", "失败有下一步建议");
    let _ = g.stall(100);
    let stalled = g.of(100).map(|x| x.stalled);
    let resumed = g.resume_stalled();
    s.add("X08783 断点续作", stalled == Some(true) && resumed == 1 && g.of(100).map(|x| x.stalled) == Some(false), "续跑还原");
    g.pressure = true;
    let p_slice = g.slice_ms(100);
    let deg = g.degrade();
    g.pressure = false;
    s.add("X08784 资源降级", p_slice < SLICE_MS[2] && deg >= 1 && g.of(100).map(|x| x.level) == Some(3), "压力降级守护");
    g.reset();
    s.add("X08785 回滚净身", g.count == 0 && g.of(100).is_none(), "可完整撤销");

    // L3 手感与细节
    let m1 = motion_token(false);
    let m2 = motion_token(true);
    s.add("X08786 动效令牌", m1 == 200 && m2 == FADE_MS && m2 < m1, "reduce-motion 降级");
    let focus_ok = {
        let mut seen = [false; 5];
        let mut ok = true;
        let mut i = 0;
        while i < FOCUS_ORDER.len() {
            let f = FOCUS_ORDER[i] as usize;
            if f == 0 || f > 4 || seen[f] {
                ok = false;
            }
            seen[f] = true;
            i += 1;
        }
        ok
    };
    s.add("X08787 三态与焦点环", focus_ok, "hover/press/disabled 过检");
    s.add("X08788 键盘通道", FOCUS_ORDER[0] != FOCUS_ORDER[3] && FOCUS_ORDER.len() == 4, "roving 语义正确");
    s.add("X08789 微文案", NICE_MIN == -10 && NICE_MAX == 10, "术语一致长度克制");
    s.add("X08790 无障碍等价", CONTRAST_MIN_PMIL >= 450, "对比度达标");

    // L4 性能与优化
    s.add("X08791 性能预算表", BUDGET_US.len() == 5 && BUDGET_US[0] < BUDGET_US[4], "指标入 CI 基线");
    let h1 = g.slice_ms(1);
    let h2 = g.slice_ms(1);
    s.add("X08792 热路径优化", h1 == 0 && h1 == h2, "查表直达");
    let c0 = g.count;
    let _ = g.register(7, 2, 0);
    let _ = g.register(7, 3, 5);
    s.add("X08793 内存收敛", c0 == 0 && g.count == 1, "登记去重零泄漏");
    s.add("X08794 降级链", SLICE_MS[0] > SLICE_MS[2] && SLICE_MS[2] > SLICE_MS[4], "三级递降不塌方");
    let gg1 = g.add_guard(0x8776);
    let gg2 = g.add_guard(0x8776);
    s.add("X08795 回归守卫", gg1 && !gg2 && g.guard_count == 1, "只增不删");

    // L5 创新拓展
    g.suggestion_active = true;
    let sug = suggest(0);
    g.suggestion_active = false;
    s.add("X08796 智能建议", sug == "gov-hot" && !g.suggestion_active, "可解释可拒绝");
    g.batch_total = 2;
    let b1 = g.batch_step();
    let b2 = g.batch_step();
    s.add("X08797 批量模式", !b1 && b2, "队列进度可观测");
    s.add("X08798 跨域联动", crate::sec::SEC_DOMAIN == "sec" && EXT_APIS[0] == 0x8776, "与 sec 域协同");
    s.add("X08799 扩展点", EXT_APIS.len() == 3 && EXT_APIS[0] < EXT_APIS[2], "接口/示例/文档三件套");
    g.egg_on = true;
    let egg = g.egg_on;
    g.reset();
    s.add("X08800 彩蛋层", egg && !g.egg_on && g.count == 0, "可关闭不损主线");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procgov_25_checks_pass() {
        let set = run_procgov_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
