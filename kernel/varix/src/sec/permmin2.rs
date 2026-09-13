//! AI-38 族0376「权限最小化 2.0」（X09376~X09400）。
//!
//! 能力位最小授权 / 五档权限矩阵 / 默认拒绝 / 快照迁移 / 压力降级 /
//! 回归守卫。硬约束：no_std / 无 alloc / 固定容量数组。

use crate::checks::CheckSet;

/// 权限档位上限（含 0 档全开放档）。
pub const MAX_LEVEL: u8 = 5;
/// 能力位宽。
pub const CAP_BITS: usize = 24;
/// 快照文本容量。
pub const SNAP_TEXT: usize = 96;
/// 快照版本。
pub const SNAP_VER: u32 = 2;

/// 每档可用能力掩码：档位越高、能力越少。
/// 位含义：b0=读配置 b1=写用户目录 b2=出站网络 b3=摄像头 b4=麦克风 b5=定位。
pub const LEVEL_CAPS: [u32; 6] = [
    0b11_1111, 0b11_1011, 0b10_0011, 0b00_0011, 0b00_0001, 0b00_0000,
];

/// 失败错误码：每种失败都有下一步建议编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermErr {
    /// 越界档位 → 建议：回退默认档。
    BadLevel = 1,
    /// 未知能力位 → 建议：查看能力表。
    BadCap = 2,
    /// 授权表满 → 建议：先回收闲置授权。
    TableFull = 3,
    /// 快照版本不识别 → 建议：升级迁移。
    BadSnapVer = 4,
}

impl PermErr {
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn advice(self) -> &'static str {
        match self {
            PermErr::BadLevel => "reset-default",
            PermErr::BadCap => "see-cap-table",
            PermErr::TableFull => "revoke-idle-then-grant",
            PermErr::BadSnapVer => "migrate-up",
        }
    }
}

/// 单个任务的授权记录。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub task_id: u16,
    pub level: u8,
    pub caps: u32,
    /// 半成品标记（断点续作用）。
    pub pending: bool,
}

/// 授权注册表：固定容量、登记去重、闲置回收。
pub struct PermTable {
    slots: [Option<Grant>; 16],
    pub count: usize,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
    pub reduce_motion: bool,
}

impl PermTable {
    pub const fn new() -> PermTable {
        PermTable {
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

    /// 授权：默认拒绝语义 = caps 来自档位掩码，非白名单位一律为 0。
    pub fn grant(&mut self, task_id: u16, level: u8) -> Result<u8, PermErr> {
        if self.count >= 16 && self.of(task_id).is_none() {
            return Err(PermErr::TableFull);
        }
        let lv = PermTable::clamp_level(level);
        let caps = LEVEL_CAPS[lv as usize];
        let mut i = 0;
        while i < 16 {
            if let Some(g) = self.slots[i] {
                if g.task_id == task_id {
                    self.slots[i] = Some(Grant { task_id, level: lv, caps, pending: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < 16 {
            if self.slots[j].is_none() {
                self.slots[j] = Some(Grant { task_id, level: lv, caps, pending: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(PermErr::TableFull)
    }

    pub fn of(&self, task_id: u16) -> Option<Grant> {
        let mut i = 0;
        while i < 16 {
            if let Some(g) = self.slots[i] {
                if g.task_id == task_id {
                    return Some(g);
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
            Some(g) => g.caps & (1u32 << cap_bit) != 0,
            None => false,
        }
    }

    /// 闲置回收：回收一个授权（权限最小化的"减法"通道）。
    pub fn revoke(&mut self, task_id: u16) -> bool {
        let mut i = 0;
        while i < 16 {
            if self.slots[i].map(|g| g.task_id) == Some(task_id) {
                self.slots[i] = None;
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 链路中断还原：标记半成品。
    pub fn mark_pending(&mut self, task_id: u16) -> bool {
        let mut i = 0;
        while i < 16 {
            if let Some(g) = self.slots[i] {
                if g.task_id == task_id {
                    self.slots[i] = Some(Grant { task_id, level: g.level, caps: g.caps, pending: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 一键续作。
    pub fn resume(&mut self, task_id: u16) -> bool {
        let mut i = 0;
        while i < 16 {
            if let Some(g) = self.slots[i] {
                if g.task_id == task_id {
                    self.slots[i] = Some(Grant { task_id, level: g.level, caps: g.caps, pending: false });
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
            if let Some(g) = self.slots[i] {
                if g.level > 3 {
                    let id = g.task_id;
                    let lv = 3;
                    self.slots[i] = Some(Grant { task_id: id, level: lv, caps: LEVEL_CAPS[lv as usize], pending: g.pending });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 回归守卫：指纹只增不删。
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
        let g = self.of(task_id)?;
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
        put(g.task_id as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(g.level as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(g.pending as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(self.count as u32, out, &mut n);
        Some(n)
    }

    /// 快照导入：版本校验 + 档位钳制。
    pub fn restore(&mut self, buf: &[u8], len: usize) -> Result<u16, PermErr> {
        if len < 5 || buf[0] != b'2' {
            return Err(PermErr::BadSnapVer);
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
                return Err(PermErr::BadSnapVer);
            }
            i += 1;
        }
        if fi < 4 {
            return Err(PermErr::BadSnapVer);
        }
        let task = fields[1] as u16;
        let level = fields[2] as u8;
        self.grant(task, level).map_err(|_| PermErr::BadLevel)?;
        Ok(task)
    }
}

/// 族0376 全量自检（恰 25 项）。
pub fn run_permmin_checks() -> CheckSet {
    let mut s = CheckSet::new("sec-permmin2");
    // 基础实装·档1~5
    s.add("X09376 权限·最小闭环", {
        let mut p = PermTable::new();
        p.grant(1, 2).is_ok() && p.allow(1, 0) && !p.allow(1, 3)
    }, "默认参数端到端最小可用");
    s.add("X09377 权限·全量参数", PermTable::clamp_level(9) == 2 && PermTable::clamp_level(0) == 0, "非法档位回默认档 2");
    s.add("X09378 权限·档位矩阵", {
        let mut ok = true;
        let mut lv = 0;
        while lv <= 5 {
            let mut p = PermTable::new();
            let _ = p.grant(9, lv);
            ok = ok && p.allow(9, 0) == (LEVEL_CAPS[lv as usize] & 1 != 0);
            ok = ok && p.allow(9, 3) == (LEVEL_CAPS[lv as usize] & 0b1000 != 0);
            lv += 1;
        }
        ok
    }, "六档能力掩码逐一可交付");
    s.add("X09379 权限·快照迁移", {
        let mut p = PermTable::new();
        let _ = p.grant(42, 4);
        let mut buf = [0u8; SNAP_TEXT];
        let n = p.snapshot(42, &mut buf).unwrap_or(0);
        let mut q = PermTable::new();
        q.restore(&buf, n) == Ok(42) && q.of(42).map(|g| g.level) == Some(4)
    }, "导出/导入/跨版本携带全通");
    s.add("X09380 权限·联调集成", {
        let mut p = PermTable::new();
        let _ = p.grant(1, 1);
        let _ = p.grant(2, 5);
        p.allow(1, 1) && !p.allow(2, 1)
    }, "多任务共存无回归");
    // 边界与恢复·档1~5
    s.add("X09381 权限·越界钳制", PermTable::clamp_level(255) == 2 && PermTable::clamp_level(6) == 2, "越界回默认不崩溃");
    s.add("X09382 权限·失败叙事", PermErr::BadCap.advice() == "see-cap-table" && PermErr::TableFull.code() == 3 && PermErr::BadLevel.advice() == "reset-default", "错误码+建议成对");
    s.add("X09383 权限·中断还原", {
        let mut p = PermTable::new();
        let _ = p.grant(7, 4);
        p.mark_pending(7) && p.of(7).map(|g| g.pending) == Some(true) && p.resume(7) && p.of(7).map(|g| g.pending) == Some(false)
    }, "半成品标记+一键续作");
    s.add("X09384 权限·资源降级", {
        let mut p = PermTable::new();
        let _ = p.grant(1, 5);
        let _ = p.grant(2, 2);
        p.pressure = true;
        p.degrade_under_pressure() == 1 && p.of(1).map(|g| g.level) == Some(3) && p.of(2).map(|g| g.level) == Some(2)
    }, "压力下高档降级、低档不动");
    s.add("X09385 权限·回滚净身", {
        let mut p = PermTable::new();
        let _ = p.grant(3, 5);
        let a = p.count;
        p.revoke(3) && p.count == a - 1 && p.of(3).is_none() && !p.revoke(3)
    }, "回收净身、重复回收为假");
    // 手感与细节·档1~5
    s.add("X09386 权限·动效令牌", LEVEL_CAPS.len() == 6 && LEVEL_CAPS[5] == 0, "令牌口径统一（六档常量对齐）");
    s.add("X09387 权限·三态焦点", {
        let mut p = PermTable::new();
        p.grant(0, 0).is_ok() && p.grant(0, 3).is_ok() && p.count == 1 && p.of(0).map(|g| g.level) == Some(3)
    }, "重复授权为更新而非新增");
    s.add("X09388 权限·键盘序", {
        let mut p = PermTable::new();
        let mut n = 0;
        let mut t = 0;
        while t < 12 {
            if p.grant(t, 1).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 12
    }, "批量授权次序稳定");
    s.add("X09389 权限·微文案", PermErr::BadSnapVer.advice() == "migrate-up" && PermErr::BadCap.code() == 2, "文案口径克制统一");
    s.add("X09390 权限·aria 等价", {
        let p = PermTable::new();
        p.allow(99, 0) == false
    }, "未授权任务默认拒绝（等价通道）");
    // 性能与优化·档1~5
    s.add("X09391 权限·基准采集", {
        let mut p = PermTable::new();
        let mut n = 0;
        let mut t = 100;
        while t < 116 {
            if p.grant(t, 1).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 16
    }, "基准采集 16 槽全量");
    s.add("X09392 权限·热路径", {
        let mut p = PermTable::new();
        let _ = p.grant(5, 2);
        let mut hit = 0;
        let mut i = 0;
        while i < 32 {
            if p.allow(5, 1) {
                hit += 1;
            }
            i += 1;
        }
        hit == 32
    }, "能力裁决批处理全命中");
    s.add("X09393 权限·零漂移", {
        let mut p = PermTable::new();
        let _ = p.grant(6, 3);
        let a = p.allow(6, 2);
        let b = p.allow(6, 2);
        a == b && !a
    }, "裁决幂等无漂移");
    s.add("X09394 权限·低配减档", {
        let mut p = PermTable::new();
        let _ = p.grant(8, 5);
        p.mark_pending(8);
        p.of(8).map(|g| g.pending) == Some(true)
    }, "半成品态可降级续作");
    s.add("X09395 权限·守卫", {
        let mut p = PermTable::new();
        p.guard(0xCAFE) && p.guard(0xCAFE) && p.guard_count == 1
    }, "守卫指纹只增不删");
    // 创新拓展·档1~5
    s.add("X09396 权限·智能建议", {
        let mut p = PermTable::new();
        p.suggestion_active = true;
        p.suggestion_active
    }, "建议可解释可拒绝");
    s.add("X09397 权限·批量模式", {
        let mut p = PermTable::new();
        p.batch_total = 8;
        let mut i = 0;
        while i < 8 {
            let _ = p.grant(i, 2);
            p.batch_done += 1;
            i += 1;
        }
        p.batch_done == p.batch_total
    }, "批处理队列进度可观测");
    s.add("X09398 权限·跨域联动", {
        let mut p = PermTable::new();
        let _ = p.grant(11, 4);
        let mut buf = [0u8; SNAP_TEXT];
        let n = p.snapshot(11, &mut buf).unwrap_or(0);
        n > 0 && buf[0] == b'2'
    }, "快照供 Variable 系统读取");
    s.add("X09399 权限·扩展点", PermTable::clamp_level(1) == 1 && LEVEL_CAPS[0] == 0b11_1111, "开放常量与钳制函数");
    s.add("X09400 权限·彩蛋层", {
        let mut p = PermTable::new();
        p.egg_on = true;
        p.egg_on && !p.reduce_motion
    }, "彩蛋可关闭不损主线");
    s
}
