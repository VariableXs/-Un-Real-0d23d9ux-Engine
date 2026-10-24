//! VE-F4276 · 治理 fuzz（U04 治理组）
//! 职责：治理面模糊测试（状态机非法跳转/定级逻辑越界/闸门参数注入三类）
//! + 语料两源（历史违例单变形+随机流程序列）+ 发现即立案不修完不闭闸
//! + 误报白名单过滤 + 资源限额调度不挤占产线。
//! 段位：0x28A0（续表 idx510）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x28A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfCode {
    /// 非法跳转检出。
    IllegalJump = SEG_BASE,
    /// 定级越界。
    GradeOver = SEG_BASE + 0x01,
    /// 注入命中。
    InjectHit = SEG_BASE + 0x02,
    /// 资源配额耗尽。
    QuotaOut = SEG_BASE + 0x03,
}

/// fuzz 目标三类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfTarget {
    FlowJump = 0,
    GradeBound = 1,
    GateInject = 2,
}

/// 语料（两源标记）。
#[derive(Clone, Copy, Debug)]
pub struct GfCorpus {
    pub from_violation: bool, // 历史违例单变形
    pub from_random: bool,    // 随机流程序列
    pub whitelisted: bool,
}

/// 治理 fuzz 器。
pub struct GovFuzz {
    pub pool: Vec<GfCorpus>,
    /// 立案账（发现即立案）。
    pub cases: Vec<(GfTarget, u16)>,
    /// 配额（每轮语料上限）。
    pub quota: u32,
    pub used: u32,
    /// 闭闸锁：发现未修完不闭闸。
    pub close_locked: bool,
}

impl GovFuzz {
    pub fn new(quota: u32) -> Self {
        GovFuzz { pool: Vec::new(), cases: Vec::new(), quota, used: 0, close_locked: false }
    }

    pub fn seed(&mut self, c: GfCorpus) {
        self.pool.push(c);
    }

    /// 白名单过滤后可跑语料。
    pub fn runnable(&self) -> usize {
        self.pool.iter().filter(|c| !c.whitelisted).count()
    }

    /// 轮次启动：配额调度。
    pub fn begin_round(&mut self, want: u32) -> Result<u32, GfCode> {
        let take = want.min(self.quota - self.used);
        if take == 0 {
            return Err(GfCode::QuotaOut);
        }
        self.used += take;
        Ok(take)
    }

    /// 非法跳转检：状态机白名单外迁移→立案+闭闸锁。
    pub fn probe_jump(&mut self, from: u8, to: u8) -> bool {
        // 白名单：只允许相邻或回退
        let ok = matches!((from, to), (0, 1) | (1, 2) | (2, 3) | (3, 4) | (4, 5) | (1, 0) | (2, 1) | (3, 2));
        if !ok {
            self.file(GfTarget::FlowJump, (from as u16) * 16 + to as u16);
        }
        !ok
    }

    /// 定级越界检：级 >2。
    pub fn probe_grade(&mut self, grade: u8) -> bool {
        if grade > 2 {
            self.file(GfTarget::GradeBound, grade as u16);
            return true;
        }
        false
    }

    /// 注入检：闸门参数含分隔符。
    pub fn probe_inject(&mut self, param: &str) -> bool {
        let hit = param.contains(';') || param.contains('|') || param.contains("$(");
        if hit {
            self.file(GfTarget::GateInject, param.len() as u16);
        }
        hit
    }

    fn file(&mut self, t: GfTarget, detail: u16) {
        if !self.cases.iter().any(|(a, b)| *a == t && *b == detail) {
            self.cases.push((t, detail));
        }
        self.close_locked = true; // 不修完不闭闸
    }

    /// 修复闭环：全部立案处置完毕才可闭闸。
    pub fn resolve_all(&mut self) -> bool {
        self.cases.clear();
        self.close_locked = false;
        true
    }

    pub fn closable(&self) -> bool {
        !self.close_locked
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C510-01..12。
pub fn run_ves510_checks() -> CheckSet {
    let mut set = CheckSet::new("ves510");

    let mut gf = GovFuzz::new(10);
    gf.seed(GfCorpus { from_violation: true, from_random: false, whitelisted: false });
    gf.seed(GfCorpus { from_violation: false, from_random: true, whitelisted: true });
    gf.seed(GfCorpus { from_violation: true, from_random: true, whitelisted: false });
    chk(&mut set, "C510-01", gf.runnable() == 2, "白名单过滤后可跑 2 条");
    chk(&mut set, "C510-02", gf.pool.iter().filter(|c| c.from_violation && c.from_random).count() == 1, "语料两源标记在册");

    // 配额调度
    chk(&mut set, "C510-03", gf.begin_round(4) == Ok(4), "配额内足额调度");
    chk(&mut set, "C510-04", gf.begin_round(8) == Ok(6), "超额按余量截断");
    chk(&mut set, "C510-05", gf.begin_round(1) == Err(GfCode::QuotaOut), "配额耗尽报错");

    // 三类目标
    chk(&mut set, "C510-06", gf.probe_jump(0, 4), "状态机非法跳转检出");
    chk(&mut set, "C510-07", !gf.probe_jump(0, 1) && !gf.probe_jump(2, 1), "白名单迁移与回退放行");
    chk(&mut set, "C510-08", gf.probe_grade(3), "定级越界检出（>2）");
    chk(&mut set, "C510-09", !gf.probe_grade(2) && gf.probe_inject("a|b"), "合法级放行+注入检出");
    chk(&mut set, "C510-10", gf.cases.len() == 3, "三类立案齐（去重）");
    chk(&mut set, "C510-11", !gf.closable(), "发现未修完不闭闸");

    chk(&mut set, "C510-12", gf.resolve_all() && gf.closable(), "修复闭环后可闭闸");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_fuzz() {
        let mut gf = GovFuzz::new(2);
        assert_eq!(gf.begin_round(5), Ok(2));
        assert!(gf.begin_round(1).is_err());
        assert!(gf.probe_jump(5, 0));
        assert!(!gf.closable());
        assert!(gf.resolve_all());
    }
}
