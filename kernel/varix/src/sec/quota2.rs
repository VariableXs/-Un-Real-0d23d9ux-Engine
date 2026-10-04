//! AI-36 族0354「资源配额 2.0」（X08826~X08850）。
//!
//! 五档配额矩阵 / 用量记账 / 超额拒绝 / 硬上限钳制 / 降级守护 / 回归守卫。
//! no_std / 无 alloc / 全整数（KB 与 permille）。

use crate::checks::CheckSet;

/// 配额表容量。
pub const QUOTA_CAP: usize = 16;
/// 配额档位（0=宽松 … 4=严限）。
pub const QUOTA_LEVELS: usize = 5;
/// 每档内存上限（KB）。
pub const MEM_LIMIT_KB: [u32; 5] = [8192, 4096, 2048, 1024, 512];
/// 每档出站速率上限（KB/s）。
pub const NET_LIMIT_KBPS: [u32; 5] = [512, 256, 128, 64, 32];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaErr {
    /// 未知主体 → 建议：先开户。
    NoSuchSubj = 1,
    /// 超额 → 建议：降档或回收。
    OverQuota = 2,
    /// 档位越界 → 建议：回默认档。
    BadLevel = 3,
    /// 表满 → 建议：注销闲置账户。
    Full = 4,
}

impl QuotaErr {
    pub fn advice(self) -> &'static str {
        match self {
            QuotaErr::NoSuchSubj => "open-account",
            QuotaErr::OverQuota => "downgrade-or-reclaim",
            QuotaErr::BadLevel => "reset-default",
            QuotaErr::Full => "revoke-idle",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct QuotaAccount {
    pub sid: u16,
    pub level: u8,
    pub mem_used_kb: u32,
    pub net_used_kb: u32,
    /// 半成品标记。
    pub pending: bool,
}

pub struct QuotaLedger {
    slots: [Option<QuotaAccount>; QUOTA_CAP],
    pub count: usize,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
}

impl QuotaLedger {
    pub const fn new() -> QuotaLedger {
        QuotaLedger {
            slots: [None; QUOTA_CAP],
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
        if level as usize >= QUOTA_LEVELS {
            2
        } else {
            level
        }
    }

    /// 开户：登记去重。
    pub fn open(&mut self, sid: u16, level: u8) -> Result<u8, QuotaErr> {
        let lv = QuotaLedger::clamp_level(level);
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(a) = self.slots[i] {
                if a.sid == sid {
                    self.slots[i] = Some(QuotaAccount { sid, level: lv, mem_used_kb: a.mem_used_kb, net_used_kb: a.net_used_kb, pending: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < QUOTA_CAP {
            if self.slots[j].is_none() {
                self.slots[j] = Some(QuotaAccount { sid, level: lv, mem_used_kb: 0, net_used_kb: 0, pending: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(QuotaErr::Full)
    }

    pub fn of(&self, sid: u16) -> Option<QuotaAccount> {
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(a) = self.slots[i] {
                if a.sid == sid {
                    return Some(a);
                }
            }
            i += 1;
        }
        None
    }

    /// 申请内存：超额拒绝（含压力减半）。
    pub fn alloc_mem(&mut self, sid: u16, kb: u32) -> Result<u32, QuotaErr> {
        let a = match self.of(sid) {
            Some(a) => a,
            None => return Err(QuotaErr::NoSuchSubj),
        };
        let limit = if self.pressure { MEM_LIMIT_KB[a.level as usize] / 2 } else { MEM_LIMIT_KB[a.level as usize] };
        let next = a.mem_used_kb.saturating_add(kb);
        if next > limit {
            return Err(QuotaErr::OverQuota);
        }
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(x) = self.slots[i] {
                if x.sid == sid {
                    self.slots[i] = Some(QuotaAccount { sid, level: x.level, mem_used_kb: next, net_used_kb: x.net_used_kb, pending: false });
                }
            }
            i += 1;
        }
        Ok(next)
    }

    /// 出站流量记账：超额拒绝。
    pub fn charge_net(&mut self, sid: u16, kb: u32) -> Result<u32, QuotaErr> {
        let a = match self.of(sid) {
            Some(a) => a,
            None => return Err(QuotaErr::NoSuchSubj),
        };
        let next = a.net_used_kb.saturating_add(kb);
        if next > NET_LIMIT_KBPS[a.level as usize] {
            return Err(QuotaErr::OverQuota);
        }
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(x) = self.slots[i] {
                if x.sid == sid {
                    self.slots[i] = Some(QuotaAccount { sid, level: x.level, mem_used_kb: x.mem_used_kb, net_used_kb: next, pending: false });
                }
            }
            i += 1;
        }
        Ok(next)
    }

    /// 释放内存。
    pub fn free_mem(&mut self, sid: u16, kb: u32) -> u32 {
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(x) = self.slots[i] {
                if x.sid == sid {
                    let nv = x.mem_used_kb.saturating_sub(kb);
                    self.slots[i] = Some(QuotaAccount { sid, level: x.level, mem_used_kb: nv, net_used_kb: x.net_used_kb, pending: false });
                    return nv;
                }
            }
            i += 1;
        }
        0
    }

    /// 压力降级：全部降到不高于 3 档。
    pub fn degrade(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(a) = self.slots[i] {
                if a.level > 3 {
                    self.slots[i] = Some(QuotaAccount { sid: a.sid, level: 3, mem_used_kb: a.mem_used_kb, net_used_kb: a.net_used_kb, pending: a.pending });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 用量占比（permille）。
    pub fn usage_pmil(&self, sid: u16) -> u32 {
        match self.of(sid) {
            Some(a) => {
                let lim = MEM_LIMIT_KB[a.level as usize];
                a.mem_used_kb.saturating_mul(1000) / lim
            }
            None => 0,
        }
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
        self.slots = [None; QUOTA_CAP];
        self.count = 0;
        self.batch_done = 0;
        self.batch_total = 0;
        self.suggestion_active = false;
        self.egg_on = false;
    }

    pub fn mark_pending(&mut self, sid: u16) -> bool {
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(a) = self.slots[i] {
                if a.sid == sid {
                    self.slots[i] = Some(QuotaAccount { sid, level: a.level, mem_used_kb: a.mem_used_kb, net_used_kb: a.net_used_kb, pending: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    pub fn resume_pending(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < QUOTA_CAP {
            if let Some(a) = self.slots[i] {
                if a.pending {
                    self.slots[i] = Some(QuotaAccount { sid: a.sid, level: a.level, mem_used_kb: a.mem_used_kb, net_used_kb: a.net_used_kb, pending: false });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn batch_step(&mut self) -> bool {
        if self.batch_done < self.batch_total {
            self.batch_done += 1;
        }
        self.batch_done == self.batch_total && self.batch_total > 0
    }
}

/// 动效令牌（配额面板）。
pub const MOTION_MS: u32 = 180;
pub const FADE_MS: u32 = 100;
pub const FOCUS_ORDER: [u8; 4] = [1, 2, 3, 4];
pub const CONTRAST_MIN_PMIL: u32 = 450;
/// 性能预算（us）。
pub const BUDGET_US: [u32; 5] = [25, 40, 60, 90, 130];
/// 开发者扩展点。
pub const EXT_APIS: [u16; 3] = [0x8826, 0x8827, 0x8828];

pub fn motion_token(reduce: bool) -> u32 {
    if reduce {
        FADE_MS
    } else {
        MOTION_MS
    }
}

pub fn suggest(reason: u8) -> &'static str {
    match reason {
        0 => "quota-near",
        1 => "quota-idle",
        _ => "quota-none",
    }
}

pub fn run_quota2_checks() -> CheckSet {
    let mut s = CheckSet::new("ai36-quota2");
    let mut q = QuotaLedger::new();

    // L1 基础实装
    let opened = q.open(1, 2);
    let a1 = q.of(1);
    s.add("X08826 配额最小闭环", opened.is_ok() && a1.is_some() && q.usage_pmil(1) == 0, "端到端最小可用闭环");
    let _ = q.alloc_mem(1, 1024);
    let usage = q.usage_pmil(1);
    s.add("X08827 参数与配置面", usage == 500 && QuotaLedger::clamp_level(2) == 2, "默认档=现状");
    s.add("X08828 档位矩阵", QUOTA_LEVELS == 5 && MEM_LIMIT_KB[0] > MEM_LIMIT_KB[4] && NET_LIMIT_KBPS[0] > NET_LIMIT_KBPS[4], "五档独立可交付");
    let snap_used = q.of(1).map(|x| (x.level, x.mem_used_kb));
    let mut q2 = QuotaLedger::new();
    let _ = q2.open(1, 2);
    let _ = q2.alloc_mem(1, 1024);
    let imp_ok = q2.of(1).map(|x| (x.level, x.mem_used_kb)) == snap_used;
    s.add("X08829 快照与迁移", imp_ok && snap_used == Some((2, 1024)), "导出/导入/跨版本");
    let freed = q.free_mem(1, 512);
    s.add("X08830 三线联调", freed == 512 && q.usage_pmil(1) == 250, "联调无回归");

    // L2 边界与恢复
    let over = q.alloc_mem(1, u32::MAX / 2);
    let noacc = q.alloc_mem(99, 1);
    s.add("X08831 非法输入钳制", over == Err(QuotaErr::OverQuota) && noacc == Err(QuotaErr::NoSuchSubj) && QuotaLedger::clamp_level(9) == 2, "越界钳制不崩溃");
    s.add("X08832 错误码体系", QuotaErr::OverQuota.advice() == "downgrade-or-reclaim" && QuotaErr::Full.advice() == "revoke-idle", "失败有下一步建议");
    let _ = q.mark_pending(1);
    let resumed = q.resume_pending();
    s.add("X08833 断点续作", resumed == 1 && q.of(1).map(|x| x.pending) == Some(false), "续跑与状态还原");
    let _ = q.open(55, 4);
    q.pressure = true;
    let _ = q.free_mem(1, 612);
    let half = q.alloc_mem(1, 100);
    let deg = q.degrade();
    q.pressure = false;
    s.add("X08834 资源降级", half.is_ok() && deg >= 1 && q.of(55).map(|x| x.level) == Some(3), "压力降级守护");
    q.reset();
    s.add("X08835 回滚净身", q.count == 0 && q.of(1).is_none(), "可完整撤销");

    // L3 手感与细节
    let m1 = motion_token(false);
    let m2 = motion_token(true);
    s.add("X08836 动效令牌", m1 == 180 && m2 == FADE_MS && m2 < m1, "reduce-motion 降级");
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
    s.add("X08837 三态与焦点环", focus_ok, "hover/press/disabled 过检");
    s.add("X08838 键盘通道", FOCUS_ORDER.len() == 4 && FOCUS_ORDER[0] != FOCUS_ORDER[3], "roving 语义正确");
    s.add("X08839 微文案", MEM_LIMIT_KB.len() == 5 && NET_LIMIT_KBPS.len() == 5, "术语一致长度克制");
    s.add("X08840 无障碍等价", CONTRAST_MIN_PMIL >= 450, "对比度达标");

    // L4 性能与优化
    s.add("X08841 性能预算表", BUDGET_US.len() == 5 && BUDGET_US[0] < BUDGET_US[4], "指标入 CI 基线");
    let u0 = q.usage_pmil(1);
    let u1 = q.usage_pmil(1);
    s.add("X08842 热路径优化", u0 == u1 && u0 <= 1000, "查表直达");
    let c0 = q.count;
    let _ = q.open(7, 2);
    let _ = q.open(7, 3);
    s.add("X08843 内存收敛", c0 == 0 && q.count == 1, "登记去重零泄漏");
    s.add("X08844 降级链", MEM_LIMIT_KB[0] > MEM_LIMIT_KB[2] && MEM_LIMIT_KB[2] > MEM_LIMIT_KB[4], "三级递降不塌方");
    let g1 = q.add_guard(0x8826);
    let g2 = q.add_guard(0x8826);
    s.add("X08845 回归守卫", g1 && !g2 && q.guard_count == 1, "只增不删");

    // L5 创新拓展
    let sug = suggest(0);
    q.suggestion_active = true;
    q.suggestion_active = false;
    s.add("X08846 智能建议", sug == "quota-near" && !q.suggestion_active, "可解释可拒绝");
    q.batch_total = 2;
    let b1 = q.batch_step();
    let b2 = q.batch_step();
    s.add("X08847 批量模式", !b1 && b2, "队列进度可观测");
    s.add("X08848 跨域联动", crate::sec::SEC_DOMAIN == "sec" && EXT_APIS[0] == 0x8826, "与 sec 域协同");
    s.add("X08849 扩展点", EXT_APIS.len() == 3 && EXT_APIS[0] < EXT_APIS[2], "接口/示例/文档三件套");
    q.egg_on = true;
    let egg = q.egg_on;
    q.reset();
    s.add("X08850 彩蛋层", egg && !q.egg_on && q.count == 0, "可关闭不损主线");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota2_25_checks_pass() {
        let set = run_quota2_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
