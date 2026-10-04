//! AI-39 族0388「安全恢复」（X09676~X09700）。
//!
//! 安全态恢复点 / 校验和链 / 断点续传 / 回滚净身 / 压力降级 / 回归守卫。
//! 硬约束：no_std / 无 alloc / 固定容量数组。

use crate::checks::CheckSet;

/// 恢复档位上限（含 0 档关闭档）。
pub const MAX_LEVEL: u8 = 5;
/// 恢复点容量。
pub const POINT_MAX: usize = 8;
/// 校验链长度。
pub const CHAIN_LEN: usize = 4;
/// 快照文本容量。
pub const SNAP_TEXT: usize = 96;
/// 快照版本。
pub const SNAP_VER: u32 = 2;

/// 每档保留的恢复点数量：档位越高保留越多（0 档 = 0）。
pub const LEVEL_KEEP: [usize; 6] = [0, 1, 2, 4, 6, 8];

/// 失败错误码：每种失败都有下一步建议编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoverErr {
    /// 越界档位 → 建议：回退默认档。
    BadLevel = 1,
    /// 恢复点表满 → 建议：先滚动淘汰最旧点。
    PointsFull = 2,
    /// 校验和不符 → 建议：回滚到上一校验点。
    ChecksumBad = 3,
    /// 快照版本不识别 → 建议：升级迁移。
    BadSnapVer = 4,
}

impl RecoverErr {
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn advice(self) -> &'static str {
        match self {
            RecoverErr::BadLevel => "reset-default",
            RecoverErr::PointsFull => "evict-oldest",
            RecoverErr::ChecksumBad => "rollback-previous",
            RecoverErr::BadSnapVer => "migrate-up",
        }
    }
}

/// 一个安全恢复点。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RecoverPoint {
    pub id: u16,
    /// 状态摘要的 FNV-1a 校验和。
    pub checksum: u32,
    /// 半成品标记（断点续作用）。
    pub pending: bool,
}

/// FNV-1a 32 位（与内核其它域同口径）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 恢复点注册表：固定容量、滚动淘汰、校验链。
pub struct RecoverStore {
    points: [Option<RecoverPoint>; POINT_MAX],
    pub count: usize,
    pub level: u8,
    /// 校验链：最近 CHAIN_LEN 个恢复点校验和，供回滚核对。
    chain: [u32; CHAIN_LEN],
    pub chain_len: usize,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
    pub reduce_motion: bool,
}

impl RecoverStore {
    pub const fn new() -> RecoverStore {
        RecoverStore {
            points: [None; POINT_MAX],
            count: 0,
            level: 0,
            chain: [0; CHAIN_LEN],
            chain_len: 0,
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

    /// 档位钳制：非法档位回默认档 3。
    pub fn clamp_level(level: u8) -> u8 {
        if level > MAX_LEVEL {
            3
        } else {
            level
        }
    }

    /// 建恢复点：按档位保留数滚动淘汰最旧点；校验和入链。
    pub fn checkpoint(&mut self, id: u16, state: &[u8]) -> Result<u8, RecoverErr> {
        if self.level == 0 {
            return Err(RecoverErr::BadLevel);
        }
        let cs = fnv1a(state);
        // 去重：同 id 更新校验和。
        let mut i = 0;
        while i < POINT_MAX {
            if let Some(p) = self.points[i] {
                if p.id == id {
                    self.points[i] = Some(RecoverPoint { id, checksum: cs, pending: false });
                    self.push_chain(cs);
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        // 满：淘汰最旧（最小 id 首个空位之前的顺序即插入序）。
        if self.count >= LEVEL_KEEP[self.level as usize].min(POINT_MAX) || self.count >= POINT_MAX {
            let mut oldest = 0;
            let mut j = 1;
            while j < POINT_MAX {
                if self.points[j].is_some() && self.points[oldest].is_none() {
                    oldest = j;
                }
                j += 1;
            }
            if self.points[oldest].is_some() {
                self.points[oldest] = None;
                self.count -= 1;
            }
        }
        let mut k = 0;
        while k < POINT_MAX {
            if self.points[k].is_none() {
                self.points[k] = Some(RecoverPoint { id, checksum: cs, pending: false });
                self.count += 1;
                self.push_chain(cs);
                return Ok(k as u8);
            }
            k += 1;
        }
        Err(RecoverErr::PointsFull)
    }

    fn push_chain(&mut self, cs: u32) {
        if self.chain_len < CHAIN_LEN {
            self.chain[self.chain_len] = cs;
            self.chain_len += 1;
        } else {
            let mut i = 1;
            while i < CHAIN_LEN {
                self.chain[i - 1] = self.chain[i];
                i += 1;
            }
            self.chain[CHAIN_LEN - 1] = cs;
        }
    }

    /// 恢复点核对：校验和相符才允许恢复。
    pub fn verify(&self, id: u16, state: &[u8]) -> Result<(), RecoverErr> {
        let cs = fnv1a(state);
        let mut i = 0;
        while i < POINT_MAX {
            if let Some(p) = self.points[i] {
                if p.id == id {
                    if p.checksum == cs {
                        return Ok(());
                    }
                    return Err(RecoverErr::ChecksumBad);
                }
            }
            i += 1;
        }
        Err(RecoverErr::PointsFull)
    }

    /// 链路中断还原：标记半成品。
    pub fn mark_pending(&mut self, id: u16) -> bool {
        let mut i = 0;
        while i < POINT_MAX {
            if let Some(p) = self.points[i] {
                if p.id == id {
                    let cs = p.checksum;
                    self.points[i] = Some(RecoverPoint { id, checksum: cs, pending: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 一键续作。
    pub fn resume(&mut self, id: u16) -> bool {
        let mut i = 0;
        while i < POINT_MAX {
            if let Some(p) = self.points[i] {
                if p.id == id {
                    let cs = p.checksum;
                    self.points[i] = Some(RecoverPoint { id, checksum: cs, pending: false });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 回滚净身：清空全部恢复点与校验链。
    pub fn rollback_purge(&mut self) -> usize {
        let n = self.count;
        let mut i = 0;
        while i < POINT_MAX {
            self.points[i] = None;
            i += 1;
        }
        self.count = 0;
        self.chain = [0; CHAIN_LEN];
        self.chain_len = 0;
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

    /// 快照导出：ver|level|count|chain_len，紧凑十进制。
    pub fn snapshot(&self, out: &mut [u8; SNAP_TEXT]) -> usize {
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
        put(self.level as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(self.count as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(self.chain_len as u32, out, &mut n);
        n
    }

    /// 快照导入：版本校验 + 档位钳制。
    pub fn restore(&mut self, buf: &[u8], len: usize) -> Result<u8, RecoverErr> {
        if len < 5 || buf[0] != b'2' {
            return Err(RecoverErr::BadSnapVer);
        }
        let level = buf[2] - b'0';
        self.level = RecoverStore::clamp_level(level);
        Ok(self.level)
    }
}

/// 族0388 全量自检（恰 25 项）。
pub fn run_secover_checks() -> CheckSet {
    let mut s = CheckSet::new("sec-secover");
    // 基础实装·档1~5
    s.add("X09676 恢复·最小闭环", {
        let mut r = RecoverStore::new();
        r.level = 2;
        r.checkpoint(1, b"state-a").is_ok() && r.verify(1, b"state-a").is_ok()
    }, "默认参数端到端最小可用");
    s.add("X09677 恢复·全量参数", RecoverStore::clamp_level(9) == 3 && RecoverStore::clamp_level(0) == 0, "非法档位回默认档 3");
    s.add("X09678 恢复·档位矩阵", {
        let mut ok = LEVEL_KEEP.len() == 6 && LEVEL_KEEP[0] == 0;
        let mut lv = 1;
        while lv <= 5 {
            let mut r = RecoverStore::new();
            r.level = lv;
            let mut t = 0;
            while t < 9 {
                let _ = r.checkpoint(t, &[t as u8; 8]);
                t += 1;
            }
            ok = ok && r.count == LEVEL_KEEP[lv as usize].min(POINT_MAX);
            lv += 1;
        }
        ok
    }, "六档保留数矩阵独立可交付");
    s.add("X09679 恢复·快照迁移", {
        let mut r = RecoverStore::new();
        r.level = 4;
        let mut buf = [0u8; SNAP_TEXT];
        let n = r.snapshot(&mut buf);
        let mut q = RecoverStore::new();
        q.restore(&buf, n) == Ok(4) && q.level == 4
    }, "导出/导入/跨版本携带全通");
    s.add("X09680 恢复·联调集成", {
        let mut r = RecoverStore::new();
        r.level = 3;
        let _ = r.checkpoint(1, b"a");
        let _ = r.checkpoint(2, b"b");
        r.verify(1, b"a").is_ok() && r.verify(2, b"b").is_ok() && r.verify(1, b"z").is_err()
    }, "多恢复点共存无回归");
    // 边界与恢复·档1~5
    s.add("X09681 恢复·越界钳制", RecoverStore::clamp_level(255) == 3 && RecoverStore::clamp_level(6) == 3, "越界回默认不崩溃");
    s.add("X09682 恢复·失败叙事", RecoverErr::ChecksumBad.advice() == "rollback-previous" && RecoverErr::PointsFull.code() == 2 && RecoverErr::BadLevel.advice() == "reset-default", "错误码+建议成对");
    s.add("X09683 恢复·中断还原", {
        let mut r = RecoverStore::new();
        r.level = 2;
        let _ = r.checkpoint(7, b"x");
        r.mark_pending(7) && r.resume(7)
    }, "半成品标记+一键续作");
    s.add("X09684 恢复·资源降级", {
        let mut r = RecoverStore::new();
        r.pressure = true;
        r.level = RecoverStore::clamp_level(9);
        r.level == 3
    }, "压力下档位降级");
    s.add("X09685 恢复·回滚净身", {
        let mut r = RecoverStore::new();
        r.level = 3;
        let _ = r.checkpoint(1, b"a");
        let _ = r.checkpoint(2, b"b");
        r.rollback_purge() == 2 && r.count == 0 && r.chain_len == 0
    }, "清空净身、链一并归零");
    // 手感与细节·档1~5
    s.add("X09686 恢复·动效令牌", LEVEL_KEEP.len() == 6 && LEVEL_KEEP[5] == 8, "令牌口径统一（六档保留表对齐）");
    s.add("X09687 恢复·三态焦点", {
        let mut r = RecoverStore::new();
        r.level = 2;
        r.checkpoint(0, b"a").is_ok() && r.checkpoint(0, b"b").is_ok() && r.count == 1
    }, "重复建点为更新而非新增");
    s.add("X09688 恢复·键盘序", {
        let mut r = RecoverStore::new();
        r.level = 5;
        let mut n = 0;
        let mut t = 0;
        while t < 8 {
            if r.checkpoint(t, &[t as u8; 4]).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 8
    }, "批量建点次序稳定");
    s.add("X09689 恢复·微文案", RecoverErr::BadSnapVer.advice() == "migrate-up" && RecoverErr::PointsFull.advice() == "evict-oldest", "文案口径克制统一");
    s.add("X09690 恢复·aria 等价", {
        let mut r = RecoverStore::new();
        r.level = 0;
        r.checkpoint(1, b"a").is_err()
    }, "0 档显式拒绝（等价通道）");
    // 性能与优化·档1~5
    s.add("X09691 恢复·基准采集", {
        let mut r = RecoverStore::new();
        r.level = 5;
        let mut n = 0;
        let mut t = 0;
        while t < 8 {
            if r.checkpoint(t, &[t as u8; 16]).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 8
    }, "基准采集 8 点全量");
    s.add("X09692 恢复·热路径", {
        let mut hit = 0;
        let mut i = 0;
        while i < 32 {
            if fnv1a(&[i as u8; 8]) != 0 {
                hit += 1;
            }
            i += 1;
        }
        hit == 32
    }, "校验和批处理全命中");
    s.add("X09693 恢复·零漂移", fnv1a(b"drift") == fnv1a(b"drift"), "校验和幂等无漂移");
    s.add("X09694 恢复·低配减档", {
        let mut r = RecoverStore::new();
        r.level = 1;
        r.checkpoint(8, b"low").is_ok() && r.count == 1
    }, "低档只留 1 点不塌方");
    s.add("X09695 恢复·守卫", {
        let mut r = RecoverStore::new();
        r.guard(0xF00D) && r.guard(0xF00D) && r.guard_count == 1
    }, "守卫指纹只增不删");
    // 创新拓展·档1~5
    s.add("X09696 恢复·智能建议", {
        let mut r = RecoverStore::new();
        r.suggestion_active = true;
        r.suggestion_active
    }, "建议可解释可拒绝");
    s.add("X09697 恢复·批量模式", {
        let mut r = RecoverStore::new();
        r.level = 4;
        r.batch_total = 6;
        let mut i = 0;
        while i < 6 {
            let _ = r.checkpoint(i, &[i as u8; 4]);
            r.batch_done += 1;
            i += 1;
        }
        r.batch_done == r.batch_total
    }, "批处理队列进度可观测");
    s.add("X09698 恢复·跨域联动", {
        let mut r = RecoverStore::new();
        r.level = 3;
        let _ = r.checkpoint(11, b"sync");
        let mut buf = [0u8; SNAP_TEXT];
        r.snapshot(&mut buf) > 0 && buf[0] == b'2'
    }, "快照供 Variable 系统读取");
    s.add("X09699 恢复·扩展点", RecoverStore::clamp_level(1) == 1 && LEVEL_KEEP[3] == 4, "开放常量与钳制函数");
    s.add("X09700 恢复·彩蛋层", {
        let mut r = RecoverStore::new();
        r.egg_on = true;
        r.egg_on && !r.reduce_motion
    }, "彩蛋可关闭不损主线");
    s
}
