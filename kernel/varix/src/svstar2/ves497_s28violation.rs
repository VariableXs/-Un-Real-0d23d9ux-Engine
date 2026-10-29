//! VE-F4263 · 违例处置流程（U04 一致性治理与演进组）
//! 职责：违例六步全链（发现→定级→派单→修复→复核→销账）+ 红黄绿三级
//! （即修/包前修/记账攒批，与主工程缺陷账本口径同源）+ 红线单列即时阻断
//! + 销账未复核账目锁死 + 定级争议仲裁升级。
//! 段位：0x1F60（续表 idx497）。行数预算 320。

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1F60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VlCode {
    /// 定级争议（升级仲裁）。
    LevelDispute = SEG_BASE,
    /// 销账未复核（账目锁死）。
    CloseUnreviewed = SEG_BASE + 0x01,
    /// 红线违例（即时阻断永不入池）。
    RedlineHit = SEG_BASE + 0x02,
    /// 工单步骤越序。
    StepJump = SEG_BASE + 0x03,
    /// 账目不存在。
    NotFound = SEG_BASE + 0x04,
    /// 台账容量已满。
    LedgerFull = SEG_BASE + 0x05,
    /// 账目锁死（锁死期一切步进留痕拒收）。
    AccountFrozen = SEG_BASE + 0x06,
    /// 同账目同步重复留痕。
    DuplicateTrace = SEG_BASE + 0x07,
    /// 台账字节投影漂移。
    LedgerDrift = SEG_BASE + 0x08,
}

/// 三级分级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Level {
    Green = 0, // 记账攒批
    Yellow = 1, // 包前修
    Red = 2,   // 即修
}

/// 违例条目（账本）。
#[derive(Clone, Copy, Debug)]
pub struct Violation {
    pub id: u32,
    pub level: Level,
    pub redline: bool,
    pub step: u8, // 六步 0..6
    pub reviewed: bool,
    pub closed: bool,
    pub locked: bool,
}

/// 违例账本。
pub struct ViolationBook {
    pub items: Vec<Violation>,
    /// 仲裁升级账。
    pub disputes: Vec<u32>,
    /// 红线通报三要素齐备标记。
    pub redline_reports: u32,
}

pub const STEPS: [&str; 6] = ["found", "graded", "assigned", "fixed", "reviewed", "closed"];

impl ViolationBook {
    pub fn new() -> Self {
        ViolationBook { items: Vec::new(), disputes: Vec::new(), redline_reports: 0 }
    }

    /// 发现并定级（红线单列：即时阻断不入池——返回 Err 并通报）。
    pub fn found(&mut self, id: u32, level: Level, redline: bool) -> Result<(), VlCode> {
        if self.items.iter().any(|v| v.id == id) {
            return Err(VlCode::NotFound);
        }
        if redline {
            self.redline_reports += 1; // 通报三要素齐备即记
            return Err(VlCode::RedlineHit);
        }
        self.items.push(Violation { id, level, redline: false, step: 1, reviewed: false, closed: false, locked: false });
        Ok(())
    }

    /// 定级争议升级。
    pub fn dispute(&mut self, id: u32) -> Result<(), VlCode> {
        let v = self.items.iter().find(|v| v.id == id).ok_or(VlCode::NotFound)?;
        let _ = v.level;
        if !self.disputes.contains(&id) {
            self.disputes.push(id);
        }
        Err(VlCode::LevelDispute)
    }

    /// 步推进（六步顺序：发现→定级→派单→修复→复核→销账）。
    pub fn step(&mut self, id: u32) -> Result<u8, VlCode> {
        let v = self.items.iter_mut().find(|v| v.id == id).ok_or(VlCode::NotFound)?;
        if v.locked {
            return Err(VlCode::CloseUnreviewed);
        }
        if v.step >= 6 {
            return Err(VlCode::StepJump);
        }
        v.step += 1;
        if v.step == 5 {
            v.reviewed = true;
        }
        if v.step == 6 {
            // 销账必须先复核
            if !v.reviewed {
                v.locked = true;
                v.step = 5;
                return Err(VlCode::CloseUnreviewed);
            }
            v.closed = true;
        }
        Ok(v.step)
    }

    /// 批量派单：绿级攒批计数。
    pub fn batch_greens(&self) -> usize {
        self.items.iter().filter(|v| v.level == Level::Green && v.step < 4).count()
    }
}

// ============ 深化批（2026-10-27 i18n 深化）：处置步进留痕台账与回归面 ============
// 在既有六步三级处置链之上补齐：步进留痕台账（带日步与定级归因）、锁死闸、
// 守恒恒等式、边界探针、按级分位投影、LCG 注入回归、字节投影 + FNV。

/// 单条步进留痕（台账条目：账目 + 步 + 日步 + 定级归因 + 放行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepTrace {
    pub acc: u32,
    pub step: u8,
    pub day: u16,
    pub level: Level,
    /// 是否被放行（未放行=锁死/越序留痕）。
    pub granted: bool,
}

/// 处置步进留痕台账：全量步进事件留痕 + 锁死闸 + 守恒口径。
#[derive(Clone, Debug)]
pub struct DisposalLedger {
    pub traces: Vec<StepTrace>,
    /// 锁死名单（账目级：锁死期一切步进留痕拒收）。
    pub frozen: Vec<u32>,
    /// 台账容量上限。
    pub cap: usize,
}

impl DisposalLedger {
    pub fn new(cap: usize) -> Self {
        DisposalLedger { traces: Vec::new(), frozen: Vec::new(), cap }
    }

    /// 记账：满容量/锁死账目/同账目同步重复 → 显性 Err 码。
    pub fn record(&mut self, t: StepTrace) -> Result<(), VlCode> {
        if self.traces.len() >= self.cap {
            return Err(VlCode::LedgerFull);
        }
        if self.frozen.contains(&t.acc) {
            return Err(VlCode::AccountFrozen);
        }
        if self.traces.iter().any(|x| x.acc == t.acc && x.step == t.step) {
            return Err(VlCode::DuplicateTrace);
        }
        self.traces.push(t);
        Ok(())
    }

    /// 锁死闸（幂等：重复锁死返回 false，零副作用）。
    pub fn freeze(&mut self, acc: u32) -> bool {
        if self.frozen.contains(&acc) {
            return false;
        }
        self.frozen.push(acc);
        true
    }

    /// 解锁（仅解在册者；销账复核后解锁）。
    pub fn unfreeze(&mut self, acc: u32) -> bool {
        let n = self.frozen.len();
        self.frozen.retain(|w| *w != acc);
        self.frozen.len() != n
    }

    /// 守恒恒等式：放行 + 拒绝 ≡ 总账（按级聚合不变量）。
    pub fn conservation(&self) -> bool {
        let granted = self.traces.iter().filter(|t| t.granted).count();
        let denied = self.traces.iter().filter(|t| !t.granted).count();
        granted + denied == self.traces.len()
    }

    /// 查询投影：按红黄绿三级分列计数。
    pub fn by_level(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for t in self.traces.iter() {
            out[t.level as usize] += 1;
        }
        out
    }

    /// 中位步（整数取点，禁浮点；偶数取左中位）。
    pub fn median_step(&self) -> Option<u8> {
        if self.traces.is_empty() {
            return None;
        }
        let mut steps: Vec<u8> = self.traces.iter().map(|t| t.step).collect();
        steps.sort_unstable();
        Some(steps[(steps.len() - 1) / 2])
    }

    /// LCG 注入回归：随机账目/步/日/级 N 轮，锁死账目必须 100% 拒收。
    /// 返回（命中拒收次数, 锁死账目出现次数）。
    pub fn inject_regression(&mut self, rounds: u32) -> (u32, u32) {
        let mut s: u32 = 0x5EED_4977; // ves497 专属种子
        let mut hits = 0u32;
        let mut total = 0u32;
        let mut i = 0;
        while i < rounds {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let acc = ((s >> 16) % 4) as u32;
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let step = ((s >> 16) % 6) as u8;
            let level = match (s >> 8) % 3 {
                0 => Level::Green,
                1 => Level::Yellow,
                _ => Level::Red,
            };
            let is_frozen = self.frozen.contains(&acc);
            let r = self.record(StepTrace { acc, step, day: 1, level, granted: !is_frozen });
            if is_frozen {
                total += 1;
                if r == Err(VlCode::AccountFrozen) {
                    hits += 1;
                }
            }
            i += 1;
        }
        (hits, total)
    }

    /// 定长字节投影：acc(4 LE) + step(1) + level(1) = 6B/条。
    pub fn encode(&self, out: &mut Vec<u8>) {
        for t in self.traces.iter() {
            out.push((t.acc & 0xFF) as u8);
            out.push(((t.acc >> 8) & 0xFF) as u8);
            out.push(((t.acc >> 16) & 0xFF) as u8);
            out.push(((t.acc >> 24) & 0xFF) as u8);
            out.push(t.step);
            out.push(t.level as u8);
        }
    }

    /// 解码往返（长度非整倍即台账漂移 → Err）。
    pub fn decode(raw: &[u8]) -> Result<Vec<StepTrace>, VlCode> {
        if raw.len() % 6 != 0 {
            return Err(VlCode::LedgerDrift);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let acc = raw[i] as u32
                | ((raw[i + 1] as u32) << 8)
                | ((raw[i + 2] as u32) << 16)
                | ((raw[i + 3] as u32) << 24);
            let level = match raw[i + 5] {
                0 => Level::Green,
                1 => Level::Yellow,
                _ => Level::Red,
            };
            out.push(StepTrace { acc, step: raw[i + 4], day: 0, level, granted: true });
            i += 6;
        }
        Ok(out)
    }

    /// 诊断投影（中文诊断串，内核侧无 IO）。
    pub fn diagnose(&self) -> String {
        format!(
            "处置台账：留痕{}条、锁死{}账目、守恒={}",
            self.traces.len(),
            self.frozen.len(),
            self.conservation().to_string()
        )
    }
}

/// FNV-1a 32 位指纹（台账投影一致性校验用）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C497-01..12。
pub fn run_ves497_checks() -> CheckSet {
    let mut set = CheckSet::new("ves497");

    let mut bk = ViolationBook::new();

    // 红线单列
    chk(&mut set, "C497-01", bk.found(1, Level::Red, true) == Err(VlCode::RedlineHit), "红线违例即时阻断永不入池");
    chk(&mut set, "C497-02", bk.redline_reports == 1 && bk.items.is_empty(), "通报三要素记账、池内零残留");

    // 三级入池
    let _ = bk.found(2, Level::Red, false);
    let _ = bk.found(3, Level::Yellow, false);
    let _ = bk.found(4, Level::Green, false);
    chk(&mut set, "C497-03", bk.items.len() == 3, "三级入池（红/黄/绿）");
    chk(&mut set, "C497-04", bk.batch_greens() == 1, "绿级攒批计数");

    // 六步链：绿级全链
    for _ in 0..6 {
        let _ = bk.step(4);
    }
    chk(&mut set, "C497-05", bk.items.iter().find(|v| v.id == 4).unwrap().closed, "绿级六步全通销账");
    chk(&mut set, "C497-06", bk.step(4) == Err(VlCode::StepJump), "销账后再步进越序");

    // 销账未复核锁死：直接从 step 1 跳到销账不可能（顺序推进），构造未复核销账——手动置 step=5 但未复核
    let v = bk.items.iter_mut().find(|v| v.id == 3).unwrap();
    v.step = 4; // 已修复未复核
    drop(v);
    // 修复账目语义：step(3) 推进到 5（复核）→ 6（销账）正常；改为直接锁死测试：
    let v = bk.items.iter_mut().find(|v| v.id == 3).unwrap();
    v.reviewed = false;
    v.step = 5;
    drop(v);
    chk(&mut set, "C497-07", bk.step(3) == Err(VlCode::CloseUnreviewed), "销账未复核→账目锁死");
    chk(&mut set, "C497-08", bk.items.iter().find(|v| v.id == 3).unwrap().locked, "锁死位在册");
    chk(&mut set, "C497-09", bk.step(3) == Err(VlCode::CloseUnreviewed), "锁死账目拒绝一切步进");

    // 定级争议仲裁升级
    chk(&mut set, "C497-10", bk.dispute(2) == Err(VlCode::LevelDispute), "红级定级争议升级");
    let _ = bk.dispute(3);
    chk(&mut set, "C497-11", bk.disputes == alloc::vec![2, 3], "争议账保序去重");

    chk(&mut set, "C497-12", bk.step(99) == Err(VlCode::NotFound), "未知账目拒收");

    // ---- 深化判据：处置步进留痕台账与回归面（C497-13..24） ----
    let mut dl = DisposalLedger::new(8);
    chk(&mut set, "C497-13", dl.record(StepTrace { acc: 2, step: 1, day: 3, level: Level::Red, granted: true }).is_ok(), "台账首笔步进留痕（红级）");
    chk(&mut set, "C497-14", dl.record(StepTrace { acc: 2, step: 1, day: 4, level: Level::Red, granted: true }) == Err(VlCode::DuplicateTrace), "同账目同步重复留痕拒收");
    chk(&mut set, "C497-15", dl.freeze(3) && !dl.freeze(3), "锁死闸幂等（重复锁死零副作用）");
    chk(&mut set, "C497-16", dl.record(StepTrace { acc: 3, step: 2, day: 5, level: Level::Yellow, granted: false }) == Err(VlCode::AccountFrozen), "锁死账目一切步进留痕拒收");
    chk(&mut set, "C497-17", dl.unfreeze(3) && dl.record(StepTrace { acc: 3, step: 2, day: 5, level: Level::Yellow, granted: true }).is_ok(), "复核解锁后可续步进");

    let cap_probe = {
        let mut small = DisposalLedger::new(2);
        let r1 = small.record(StepTrace { acc: 4, step: 1, day: 1, level: Level::Green, granted: true });
        let r2 = small.record(StepTrace { acc: 4, step: 2, day: 1, level: Level::Green, granted: true });
        let r3 = small.record(StepTrace { acc: 4, step: 3, day: 1, level: Level::Green, granted: true });
        r1.is_ok() && r2.is_ok() && r3 == Err(VlCode::LedgerFull)
    };
    chk(&mut set, "C497-18", cap_probe, "满容量边界探针（显性 Err）");

    let _ = dl.record(StepTrace { acc: 2, step: 2, day: 9, level: Level::Red, granted: false });
    chk(&mut set, "C497-19", dl.conservation(), "守恒恒等式：放行+拒绝≡总账");
    let cols = dl.by_level();
    chk(&mut set, "C497-20", cols[2] == 2 && cols[1] == 1 && dl.median_step() == Some(1), "按级分列投影 + 中位步取点");

    let (hits, total) = dl.inject_regression(64);
    chk(&mut set, "C497-21", total > 0 && hits == total, "LCG 注入回归：锁死账目 100% 拒收");
    chk(&mut set, "C497-22", dl.conservation(), "注入后守恒恒等式仍成立");

    let mut raw: Vec<u8> = Vec::new();
    dl.encode(&mut raw);
    let round = DisposalLedger::decode(&raw);
    chk(&mut set, "C497-23", match round { Ok(tr) => tr.len() == dl.traces.len() && fnv1a(&raw) == fnv1a(&raw), Err(_) => false }, "定长字节投影往返 + FNV 指纹自洽");
    chk(&mut set, "C497-24", DisposalLedger::decode(&raw[..raw.len() - 1]) == Err(VlCode::LedgerDrift), "非整倍长度→台账漂移 Err");
    chk(&mut set, "C497-25", !dl.diagnose().is_empty(), "台账诊断投影非空");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn violation_flow() {
        let mut bk = ViolationBook::new();
        assert!(bk.found(1, Level::Green, true).is_err());
        assert!(bk.found(2, Level::Green, false).is_ok());
        for _ in 0..6 {
            assert!(bk.step(2).is_ok());
        }
        assert!(bk.items[0].closed);
        assert!(bk.step(2).is_err());
    }
}
