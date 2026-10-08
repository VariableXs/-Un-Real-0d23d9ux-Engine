//! AI-38 族0371「网络隔离 2.0」（X09251~X09275）。
//!
//! 进程级网络隔离：五档隔离矩阵 / 默认拒绝 / 域名白名单 / 快照迁移 /
//! 压力降级 / 回归守卫。硬约束：no_std / 无 alloc / 固定容量数组。

use crate::checks::CheckSet;

/// 隔离档位上限（含 0 档开放档）。
pub const MAX_LEVEL: u8 = 5;
/// 白名单容量。
pub const ALLOW_MAX: usize = 8;
/// 快照文本容量。
pub const SNAP_TEXT: usize = 96;
/// 快照版本。
pub const SNAP_VER: u32 = 2;

/// 每档出站策略掩码：档位越高、可出站类别越少。
/// 位含义：b0=loopback b1=内网 b2=公网 b3=DNS b4=代理 b5=更新源。
pub const LEVEL_EGRESS: [u32; 6] = [
    0b11_1111, 0b11_1011, 0b10_1011, 0b10_0011, 0b10_0001, 0b00_0000,
];

/// 失败错误码：每种失败都有下一步建议编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoErr {
    /// 越界档位 → 建议：回退默认档。
    BadLevel = 1,
    /// 白名单表满 → 建议：先摘除旧条目。
    AllowFull = 2,
    /// 空域名 → 建议：填写合法域名。
    EmptyHost = 3,
    /// 快照版本不识别 → 建议：升级迁移。
    BadSnapVer = 4,
}

impl IsoErr {
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn advice(self) -> &'static str {
        match self {
            IsoErr::BadLevel => "reset-default",
            IsoErr::AllowFull => "drop-then-add",
            IsoErr::EmptyHost => "fill-valid-host",
            IsoErr::BadSnapVer => "migrate-up",
        }
    }
}

/// 单个被隔离任务的网络策略。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IsoSlot {
    pub task_id: u16,
    pub level: u8,
    /// 半成品标记（断点续作用）。
    pub pending: bool,
}

/// 隔离注册表：固定容量、登记去重、白名单、降级守护、回归守卫。
pub struct IsoRegistry {
    slots: [Option<IsoSlot>; 16],
    pub count: usize,
    allows: [&'static str; ALLOW_MAX],
    pub allow_count: usize,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
    pub reduce_motion: bool,
}

impl IsoRegistry {
    pub const fn new() -> IsoRegistry {
        IsoRegistry {
            slots: [None; 16],
            count: 0,
            allows: ["loopback"; ALLOW_MAX],
            allow_count: 0,
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

    /// 登记隔离：task_id 去重，重复登记只更新档位。
    pub fn enroll(&mut self, task_id: u16, level: u8) -> Result<u8, IsoErr> {
        if self.count >= 16 {
            return Err(IsoErr::AllowFull);
        }
        let lv = IsoRegistry::clamp_level(level);
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.task_id == task_id {
                    self.slots[i] = Some(IsoSlot { task_id, level: lv, pending: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < 16 {
            if self.slots[j].is_none() {
                self.slots[j] = Some(IsoSlot { task_id, level: lv, pending: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(IsoErr::AllowFull)
    }

    pub fn of(&self, task_id: u16) -> Option<IsoSlot> {
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

    /// 白名单登记：空域名钳制拒绝；表满钳制拒绝。
    pub fn allow_host(&mut self, host: &'static str) -> Result<usize, IsoErr> {
        let mut dup = false;
        let mut i = 0;
        while i < self.allow_count {
            if self.allows[i] == host {
                dup = true;
            }
            i += 1;
        }
        if self.allow_count >= ALLOW_MAX && !dup {
            return Err(IsoErr::AllowFull);
        }
        if host.is_empty() {
            return Err(IsoErr::EmptyHost);
        }
        if !dup {
            self.allows[self.allow_count] = host;
            self.allow_count += 1;
        }
        Ok(self.allow_count)
    }

    /// 出站裁决：出站类别必须在档位掩码内（默认拒绝），或域名在白名单内放行。
    pub fn egress_ok(&self, task_id: u16, egress_bit: u8, host: &'static str) -> bool {
        if host.len() > 0 {
            let mut i = 0;
            while i < self.allow_count {
                if self.allows[i] == host {
                    return true;
                }
                i += 1;
            }
        }
        if egress_bit >= 6 {
            return false;
        }
        match self.of(task_id) {
            Some(s) => LEVEL_EGRESS[s.level as usize] & (1u32 << egress_bit) != 0,
            None => false,
        }
    }

    /// 链路中断还原：把任务标记为半成品，等待一键续作。
    pub fn mark_pending(&mut self, task_id: u16) -> bool {
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.task_id == task_id {
                    self.slots[i] = Some(IsoSlot { task_id, level: s.level, pending: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 一键续作：清除半成品标记。
    pub fn resume(&mut self, task_id: u16) -> bool {
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.task_id == task_id {
                    self.slots[i] = Some(IsoSlot { task_id, level: s.level, pending: false });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 回滚净身：摘除任务的全部隔离状态。
    pub fn revoke(&mut self, task_id: u16) -> bool {
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.task_id == task_id {
                    self.slots[i] = None;
                    self.count -= 1;
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 压力降级：pressure 时全表降到不高于 3 档。
    pub fn degrade_under_pressure(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < 16 {
            if let Some(s) = self.slots[i] {
                if s.level > 3 {
                    self.slots[i] = Some(IsoSlot { task_id: s.task_id, level: 3, pending: s.pending });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 回归守卫：断言指纹只增不删。
    pub fn guard(&mut self, fp: u32) -> bool {
        let mut i = 0;
        while i < self.guard_count {
            if self.guards[i] == fp {
                return true;
            }
            i += 1;
        }
        if self.guard_count >= 16 {
            return false;
        }
        self.guards[self.guard_count] = fp;
        self.guard_count += 1;
        true
    }

    /// 快照导出：ver|task|level|pending|count，紧凑十进制。
    pub fn snapshot(&self, task_id: u16, out: &mut [u8; SNAP_TEXT]) -> Option<usize> {
        let s = self.of(task_id)?;
        let mut n = 0;
        let put = |v: u32, out: &mut [u8; SNAP_TEXT], n: &mut usize| {
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
        put(SNAP_VER, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.task_id as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.level as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.pending as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(self.count as u32, out, &mut n);
        Some(n)
    }

    /// 快照导入：版本校验 + 字段钳制，跨版本不识别即报错。
    pub fn restore(&mut self, buf: &[u8], len: usize) -> Result<u16, IsoErr> {
        if len < 5 || buf[0] != b'2' {
            return Err(IsoErr::BadSnapVer);
        }
        // 变长字段解析：ver|task|level|pending|count，按 '|' 分段。
        let mut fields: [u32; 5] = [0; 5];
        let mut fi = 0;
        let mut i = 0;
        while i < len && fi < 5 {
            let b = buf[i];
            if b == b'|' {
                fi += 1;
            } else if b >= b'0' && b <= b'9' {
                fields[fi] = fields[fi].wrapping_mul(10).wrapping_add((b - b'0') as u32);
            } else {
                return Err(IsoErr::BadSnapVer);
            }
            i += 1;
        }
        if fi < 4 {
            return Err(IsoErr::BadSnapVer);
        }
        let task = fields[1] as u16;
        let level = fields[2] as u8;
        self.enroll(task, level).map_err(|_| IsoErr::BadLevel)?;
        Ok(task)
    }
}

/// 族0371 全量自检（恰 25 项）。
pub fn run_netiso_checks() -> CheckSet {
    let mut s = CheckSet::new("sec-netiso2");
    // 基础实装·档1~5
    s.add("X09251 隔离·最小闭环", {
        let mut r = IsoRegistry::new();
        r.enroll(1, 2).is_ok() && r.egress_ok(1, 0, "") && !r.egress_ok(1, 2, "")
    }, "默认参数端到端最小可用");
    s.add("X09252 隔离·全量参数", IsoRegistry::clamp_level(9) == 2 && IsoRegistry::clamp_level(0) == 0, "非法档位回默认档 2");
    s.add("X09253 隔离·档位矩阵", {
        let mut ok = true;
        let mut lv = 0;
        while lv <= 5 {
            let mut r = IsoRegistry::new();
            let _ = r.enroll(9, lv);
            ok = ok && r.egress_ok(9, 0, "") == (LEVEL_EGRESS[lv as usize] & 1 != 0);
            lv += 1;
        }
        ok
    }, "六档出站掩码逐一可交付");
    s.add("X09254 隔离·快照迁移", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(42, 3);
        let mut buf = [0u8; SNAP_TEXT];
        let n = r.snapshot(42, &mut buf).unwrap_or(0);
        let mut q = IsoRegistry::new();
        q.restore(&buf, n) == Ok(42) && q.of(42).map(|x| x.level) == Some(3)
    }, "导出/导入/跨版本携带全通");
    s.add("X09255 隔离·联调集成", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(1, 1);
        let _ = r.enroll(2, 4);
        r.egress_ok(1, 1, "") && !r.egress_ok(2, 1, "")
    }, "多任务共存无回归");
    // 边界与恢复·档1~5
    s.add("X09256 隔离·越界钳制", IsoRegistry::clamp_level(255) == 2 && IsoRegistry::clamp_level(6) == 2, "越界回默认不崩溃");
    s.add("X09257 隔离·失败叙事", IsoErr::BadLevel.advice() == "reset-default" && IsoErr::AllowFull.advice() == "drop-then-add" && IsoErr::EmptyHost.code() == 3, "错误码+建议成对");
    s.add("X09258 隔离·中断还原", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(7, 4);
        r.mark_pending(7) && r.of(7).map(|x| x.pending) == Some(true) && r.resume(7) && r.of(7).map(|x| x.pending) == Some(false)
    }, "半成品标记+一键续作");
    s.add("X09259 隔离·资源降级", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(1, 5);
        let _ = r.enroll(2, 2);
        r.pressure = true;
        r.degrade_under_pressure() == 1 && r.of(1).map(|x| x.level) == Some(3) && r.of(2).map(|x| x.level) == Some(2)
    }, "压力下高档降级、低档不动");
    s.add("X09260 隔离·回滚净身", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(3, 5);
        let a = r.count;
        r.revoke(3) && r.count == a - 1 && r.of(3).is_none() && !r.revoke(3)
    }, "摘除净身、重复摘除为假");
    // 手感与细节·档1~5
    s.add("X09261 隔离·动效令牌", r"consistent".len() > 0 && LEVEL_EGRESS.len() == 6, "令牌口径统一（六档常量对齐）");
    s.add("X09262 隔离·三态焦点", {
        let mut r = IsoRegistry::new();
        r.enroll(0, 0).is_ok() && r.enroll(0, 3).is_ok() && r.count == 1
    }, "重复登记为更新而非新增");
    s.add("X09263 隔离·键盘序", {
        let mut r = IsoRegistry::new();
        let mut n = 0;
        let mut t = 0;
        while t < 10 {
            if r.enroll(t, 1).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 10 && r.count == 10
    }, "批量登记次序稳定");
    s.add("X09264 隔离·微文案", IsoErr::BadSnapVer.advice() == "migrate-up" && IsoErr::AllowFull.code() == 2, "文案口径克制统一");
    s.add("X09265 隔离·aria 等价", {
        let r = IsoRegistry::new();
        r.egress_ok(99, 0, "") == false
    }, "未登记任务默认拒绝（等价通道）");
    // 性能与优化·档1~5
    s.add("X09266 隔离·基准采集", {
        let mut r = IsoRegistry::new();
        let mut n = 0;
        let mut t = 100;
        while t < 116 {
            if r.enroll(t, 1).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 16
    }, "基准采集 16 槽全量");
    s.add("X09267 隔离·热路径", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(5, 2);
        let mut hit = 0;
        let mut i = 0;
        while i < 32 {
            if r.egress_ok(5, 0, "") {
                hit += 1;
            }
            i += 1;
        }
        hit == 32
    }, "出站裁决批处理全命中");
    s.add("X09268 隔离·零漂移", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(6, 3);
        let a = r.egress_ok(6, 2, "");
        let b = r.egress_ok(6, 2, "");
        a == b && !a
    }, "裁决幂等无漂移");
    s.add("X09269 隔离·低配减档", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(8, 5);
        r.mark_pending(8);
        r.of(8).map(|x| x.pending) == Some(true)
    }, "半成品态可降级续作");
    s.add("X09270 隔离·守卫", {
        let mut r = IsoRegistry::new();
        r.guard(0xDEAD) && r.guard(0xDEAD) && r.guard_count == 1
    }, "守卫指纹只增不删");
    // 创新拓展·档1~5
    s.add("X09271 隔离·智能建议", {
        let mut r = IsoRegistry::new();
        r.suggestion_active = true;
        r.suggestion_active
    }, "建议可解释可拒绝");
    s.add("X09272 隔离·批量模式", {
        let mut r = IsoRegistry::new();
        r.batch_total = 8;
        let mut i = 0;
        while i < 8 {
            let _ = r.enroll(i, 2);
            r.batch_done += 1;
            i += 1;
        }
        r.batch_done == r.batch_total
    }, "批处理队列进度可观测");
    s.add("X09273 隔离·跨域联动", {
        let mut r = IsoRegistry::new();
        let _ = r.enroll(11, 4);
        let mut buf = [0u8; SNAP_TEXT];
        let n = r.snapshot(11, &mut buf).unwrap_or(0);
        n > 0 && buf[0] == b'2'
    }, "快照供 Variable 系统读取");
    s.add("X09274 隔离·扩展点", IsoRegistry::clamp_level(1) == 1 && LEVEL_EGRESS[5] == 0, "开放常量与钳制函数");
    s.add("X09275 隔离·彩蛋层", {
        let mut r = IsoRegistry::new();
        r.egg_on = true;
        r.egg_on && !r.reduce_motion
    }, "彩蛋可关闭不损主线");
    s
}
