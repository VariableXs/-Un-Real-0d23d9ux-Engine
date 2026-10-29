//! VE-F4337 · 一致性遗留清账（U07 性能与预算组）
//! 职责：性能面遗留清账——清账来源（开发期账本优化项 + 历史性能类遗留）
//! + 清账五步（逐条定级→排程→修复→复核→销账）+ 清账纪律（新账跨组隐藏
//! 禁止——发现即归位移交）+ 销账未复核锁死 + 账实不符重扫对账。
//! 段位：0x38E0（续表 idx571）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x38E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LgCode {
    /// 销账未复核（账目锁死）。
    LockedNoReview = SEG_BASE,
    /// 跨组新账隐藏（发现即归位移交）。
    CrossGroupHide = SEG_BASE + 0x01,
    /// 账实不符（重扫对账）。
    LedgerMismatch = SEG_BASE + 0x02,
    /// 步骤越序（五步顺序违规）。
    StepSkip = SEG_BASE + 0x03,
    /// 重扫风暴（同账连续重扫超限升级）。
    ResweepStorm = SEG_BASE + 0x04,
}

/// 清账五步。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Step {
    Grade,
    Schedule,
    Fix,
    Review,
    Close,
}

/// 账目级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    P0,
    P1,
    P2,
}

/// 遗留账目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Legacy {
    pub id: u32,
    pub grade: Option<Grade>,
    pub step: Step,
    /// 归属组号（跨组隐藏检测）。
    pub group: u8,
    pub reviewed: bool,
    pub closed: bool,
}

/// 清账台。
#[derive(Default)]
pub struct LedgerDesk {
    pub items: Vec<Legacy>,
    /// 实测残留集合（账实对账用）。
    pub measured: Vec<u32>,
    pub transfers: u32,
    pub resweeps: u32,
    /// 深化：重扫事件账 (id, 轮次)，时间序。
    pub resweep_events: Vec<(u32, u32)>,
    /// 深化：风暴隔离账（连续重扫 ≥3 的 id）。
    pub stormed: Vec<u32>,
}

impl LedgerDesk {
    pub fn new() -> Self {
        LedgerDesk { ..Default::default() }
    }

    /// 登记遗留账目（初始待定级）。
    pub fn register(&mut self, id: u32, group: u8) {
        if !self.items.iter().any(|x| x.id == id) {
            self.items.push(Legacy { id, grade: None, step: Step::Grade, group, reviewed: false, closed: false });
        }
    }

    /// 五步推进：越序拒绝；修复步前须定级。
    pub fn advance(&mut self, id: u32, next: Step) -> Result<(), LgCode> {
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(LgCode::StepSkip)?;
        let expect = match x.step {
            Step::Grade => Step::Schedule,
            Step::Schedule => Step::Fix,
            Step::Fix => Step::Review,
            Step::Review => Step::Close,
            Step::Close => return Err(LgCode::LockedNoReview),
        };
        if next != expect {
            return Err(LgCode::StepSkip);
        }
        if next == Step::Fix && x.grade.is_none() {
            return Err(LgCode::StepSkip); // 未定级不得修复
        }
        x.step = next;
        if next == Step::Review {
            x.reviewed = true;
        }
        if next == Step::Close {
            if !x.reviewed {
                x.step = Step::Review; // 锁死回退
                return Err(LgCode::LockedNoReview);
            }
            x.closed = true;
        }
        Ok(())
    }

    /// 定级。
    pub fn grade(&mut self, id: u32, g: Grade) -> Result<(), LgCode> {
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(LgCode::StepSkip)?;
        if x.step != Step::Grade {
            return Err(LgCode::StepSkip);
        }
        x.grade = Some(g);
        Ok(())
    }

    /// 跨组隐藏检测：登记的组号不在归位映射内 = 隐藏 → 归位移交（计数）。
    pub fn transfer_cross(&mut self, id: u32, allowed: &[u8]) -> Result<(), LgCode> {
        let x = self.items.iter().find(|x| x.id == id).ok_or(LgCode::CrossGroupHide)?;
        if !allowed.contains(&x.group) {
            let _ = x;
            self.transfers += 1;
            return Err(LgCode::CrossGroupHide);
        }
        Ok(())
    }

    /// 账实对账：已销账 id 不得再出现在实测残留（残留 = 账实不符 → 重扫）。
    pub fn reconcile(&mut self, measured: &[u32]) -> Result<(), LgCode> {
        self.measured = measured.to_vec();
        let closed: Vec<u32> = self.items.iter().filter(|x| x.closed).map(|x| x.id).collect();
        for id in closed {
            if measured.contains(&id) {
                self.resweeps += 1;
                // 销账作废：重新入账。
                if let Some(x) = self.items.iter_mut().find(|x| x.id == id) {
                    x.closed = false;
                    x.step = Step::Fix;
                    x.reviewed = false;
                }
                return Err(LgCode::LedgerMismatch);
            }
        }
        Ok(())
    }

    /// 未复核销账锁死断言：Close 前必须 Review。
    pub fn lock_audit(&self) -> bool {
        self.items.iter().all(|x| !(x.closed && !x.reviewed))
    }

    // ─── 深化块：分级账统计 ───

    /// 各级计数：(P0, P1, P2, 未定级)。
    pub fn grade_census(&self) -> (u32, u32, u32, u32) {
        let mut p0 = 0u32;
        let mut p1 = 0u32;
        let mut p2 = 0u32;
        let mut none = 0u32;
        for x in self.items.iter() {
            match x.grade {
                Some(Grade::P0) => p0 += 1,
                Some(Grade::P1) => p1 += 1,
                Some(Grade::P2) => p2 += 1,
                None => none += 1,
            }
        }
        (p0, p1, p2, none)
    }

    /// 分级守恒：四级合计 ≡ 账目总数。
    pub fn census_conserves(&self) -> bool {
        let (a, b, c, d) = self.grade_census();
        a + b + c + d == self.items.len() as u32
    }

    // ─── 深化块：P0 优先出队 ───

    /// 下一待修账：P0 → P1 → P2 → 未定级（同级按登记序），返回 id。
    /// 全空返回 None。已销账/复核中的跳过（只出 Fix 及之前步的）。
    pub fn next_to_fix(&self) -> Option<u32> {
        for g in [Some(Grade::P0), Some(Grade::P1), Some(Grade::P2), None] {
            for x in self.items.iter() {
                if x.grade == g && !x.closed && x.step != Step::Review {
                    return Some(x.id);
                }
            }
        }
        None
    }

    // ─── 深化块：重扫风暴升级 ───

    /// 对账带风暴检测：同 id 累计重扫 ≥3 即入风暴隔离账并拒绝对账。
    pub fn reconcile_capped(&mut self, measured: &[u32]) -> Result<(), LgCode> {
        let r = self.reconcile(measured);
        if let Err(LgCode::LedgerMismatch) = r {
            // 找出触发重扫的 id（closed 且残留的）
            let trigger: Vec<u32> = self.items.iter()
                .filter(|x| x.step == Step::Fix && measured.contains(&x.id))
                .map(|x| x.id)
                .collect();
            for id in trigger {
                let cnt = self.resweep_events.iter()
                    .filter(|(k, _)| *k == id)
                    .map(|(_, v)| *v)
                    .next()
                    .unwrap_or(0) + 1;
                // 更新事件账
                if let Some(p) = self.resweep_events.iter_mut().find(|(k, _)| *k == id) {
                    p.1 = cnt;
                } else {
                    self.resweep_events.push((id, cnt));
                }
                if cnt >= 3 && !self.stormed.contains(&id) {
                    self.stormed.push(id);
                }
            }
            return Err(LgCode::ResweepStorm);
        }
        r
    }

    /// 重扫事件账守恒：总轮次 ≡ resweeps 计数。
    pub fn resweep_conserves(&self) -> bool {
        let total: u32 = self.resweep_events.iter().map(|(_, v)| v).sum();
        total == self.resweeps
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves571_checks() -> CheckSet {
    let mut set = CheckSet::new("ves571");
    let mut ld = LedgerDesk::new();

    // C571-01 登记遗留账目。
    ld.register(1, 7);
    ld.register(2, 7);
    ld.register(3, 9); // 非法组
    chk(&mut set, "C571-01", ld.items.len() == 3, "遗留账登记");

    // C571-02 跨组隐藏检测 → 归位移交。
    chk(&mut set, "C571-02", ld.transfer_cross(3, &[7, 8]) == Err(LgCode::CrossGroupHide) && ld.transfers == 1, "跨组归位移交");
    chk(&mut set, "C571-02b", ld.transfer_cross(1, &[7, 8]).is_ok(), "合法组不转");

    // C571-03 未定级不得修复。
    chk(&mut set, "C571-03", ld.advance(1, Step::Fix) == Err(LgCode::StepSkip), "未定级不修复");

    // C571-04 定级 + 五步顺序贯通。
    let _ = ld.grade(1, Grade::P0);
    let mut ok = true;
    for s in [Step::Schedule, Step::Fix, Step::Review, Step::Close] {
        if ld.advance(1, s).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C571-04", ok && ld.items[0].closed && ld.items[0].reviewed, "五步贯通销账");

    // C571-05 越序拦截（Grade 后直跳 Fix）。
    let _ = ld.grade(2, Grade::P1);
    chk(&mut set, "C571-05", ld.advance(2, Step::Fix) == Err(LgCode::StepSkip), "越序拦截");

    // C571-06 未复核销账锁死（Review 前直接 Close 拒）。
    let mut ld2 = LedgerDesk::new();
    ld2.register(5, 7);
    let _ = ld2.grade(5, Grade::P2);
    let _ = ld2.advance(5, Step::Schedule);
    let _ = ld2.advance(5, Step::Fix);
    chk(&mut set, "C571-06", ld2.advance(5, Step::Close) == Err(LgCode::LockedNoReview), "未复核锁死");
    chk(&mut set, "C571-06b", ld2.lock_audit(), "锁死审计过");

    // C571-07 复核后销账成功。
    let _ = ld2.advance(5, Step::Review);
    chk(&mut set, "C571-07", ld2.advance(5, Step::Close).is_ok() && ld2.items[0].closed, "复核后销账");

    // C571-08 账实对账：销账款残留 = 不符重扫。
    chk(&mut set, "C571-08", ld.reconcile(&[1]) == Err(LgCode::LedgerMismatch) && ld.resweeps == 1, "账实不符重扫");
    // 销账作废回 Fix 态
    chk(&mut set, "C571-08b", !ld.items[0].closed && ld.items[0].step == Step::Fix, "销账作废回修");

    // C571-09 对账干净放行。
    chk(&mut set, "C571-09", ld.reconcile(&[2, 3]).is_ok(), "对账干净放行");

    // C571-10 销账后再销 = 拒（终态）。
    chk(&mut set, "C571-10", ld2.advance(5, Step::Close) == Err(LgCode::LockedNoReview), "终态再销拒");

    // C571-11 不存在账目推进拒绝。
    chk(&mut set, "C571-11", ld.advance(999, Step::Grade).is_err(), "无件推进拒");

    // C571-12 全账锁死审计（closed 蕴含 reviewed）。
    chk(&mut set, "C571-12", ld.lock_audit() && ld2.lock_audit(), "全账审计过");

    // ── 深化判据：分级账统计 + 守恒 ──
    // ld 当前：1 回修无级？——1 曾定级 P0（grade 保级），2 定级 P1 未修，3 未定级。
    let (p0, p1, p2, none) = ld.grade_census();
    chk(&mut set, "C571-13", p0 == 1 && p1 == 1 && p2 == 0 && none == 1
        && ld.census_conserves(), "分级四桶计数守恒");

    // ── 深化判据：P0 优先出队 ──
    let mut lq = LedgerDesk::new();
    lq.register(10, 7);
    lq.register(11, 7);
    lq.register(12, 7);
    let _ = lq.grade(11, Grade::P1);
    let _ = lq.grade(12, Grade::P0);
    let _ = lq.grade(10, Grade::P2);
    chk(&mut set, "C571-14", lq.next_to_fix() == Some(12), "P0 优先出队");
    // P0 走完五步后 P1 顶上。
    for s in [Step::Schedule, Step::Fix, Step::Review, Step::Close] {
        let _ = lq.advance(12, s);
    }
    chk(&mut set, "C571-15", lq.next_to_fix() == Some(11), "P0 销后 P1 顶上");

    // ── 深化判据：重扫风暴升级（同 id 3 次重扫入隔离账）──
    let mut ls2 = LedgerDesk::new();
    ls2.register(20, 7);
    let _ = ls2.grade(20, Grade::P0);
    for s in [Step::Schedule, Step::Fix, Step::Review, Step::Close] {
        let _ = ls2.advance(20, s);
    }
    let _ = ls2.reconcile_capped(&[20]); // 重扫 1
    // 回修重销再重扫 2、3。
    for _ in 0..2 {
        for s in [Step::Review, Step::Close] {
            let _ = ls2.advance(20, s);
        }
        let _ = ls2.reconcile_capped(&[20]);
    }
    chk(&mut set, "C571-16", ls2.stormed == [20u32] && ls2.resweep_events.len() == 1,
        "3 次重扫风暴隔离");
    chk(&mut set, "C571-17", ls2.resweep_conserves(), "重扫账守恒");

    // ── 深化判据：对账干净路径不触发风暴 ──
    let mut lc = LedgerDesk::new();
    lc.register(30, 7);
    chk(&mut set, "C571-18", lc.reconcile_capped(&[]).is_ok()
        && lc.stormed.is_empty() && lc.resweep_conserves(), "干净对账无风暴");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_step_flow() {
        let mut ld = LedgerDesk::new();
        ld.register(1, 7);
        let _ = ld.grade(1, Grade::P1);
        for s in [Step::Schedule, Step::Fix, Step::Review, Step::Close] {
            assert!(ld.advance(1, s).is_ok());
        }
        assert!(ld.items[0].closed);
        assert!(ld.lock_audit());
    }

    #[test]
    fn grade_required_before_fix() {
        let mut ld = LedgerDesk::new();
        ld.register(1, 7);
        let _ = ld.advance(1, Step::Schedule);
        assert_eq!(ld.advance(1, Step::Fix), Err(LgCode::StepSkip));
        let _ = ld.grade(1, Grade::P0);
        assert!(ld.advance(1, Step::Fix).is_ok());
    }

    #[test]
    fn census_and_priority() {
        let mut ld = LedgerDesk::new();
        ld.register(1, 7);
        ld.register(2, 7);
        let _ = ld.grade(2, Grade::P0);
        assert!(ld.census_conserves());
        assert_eq!(ld.next_to_fix(), Some(2));
    }
}
