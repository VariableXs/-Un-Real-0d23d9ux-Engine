//! VE-F4302 · 一致性×新功能开发（U06 场景应用组）
//! 职责：新功能四点嵌入（设计期令牌预检/开发期 IDE 实时查/联调期跨域对拍
//! /收口期闸门）+ 清单随需求模板下发 + 首犯辅导制（首犯转辅导不记过，
//! 重复首犯转正式记录）+ 嵌入点缺失探针补位 + 清单无障碍条目强制。
//! 段位：0x2FE0（续表 idx536）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2FE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NfCode {
    /// 嵌入点缺失。
    EmbedMissing = SEG_BASE,
    /// 清单漏项。
    ChecklistHole = SEG_BASE + 0x01,
    /// 辅导滥用（重复首犯转正式）。
    TutorAbuse = SEG_BASE + 0x02,
    /// 清单账漂移（未下发先确认/重复下发）。
    AuditDrift = SEG_BASE + 0x03,
    /// 清单指纹失配（序列化损坏）。
    FpMismatch = SEG_BASE + 0x04,
}

/// 四点嵌入。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Embed {
    Design = 0,
    Coding = 1,
    Integration = 2,
    Close = 3,
}
pub const EMBED_N: usize = 4;

/// 首犯账。
#[derive(PartialEq, Clone, Copy, Debug)]
pub struct FirstOff {
    pub who: u8,
    pub count: u8,
    pub tutoring: bool,
    pub formal: bool,
}

/// 新功能场景器。
pub struct NewFeature {
    pub embeds: [bool; EMBED_N],
    /// 清单模板：(条目, 强制)。
    pub checklist: Vec<(u16, bool)>,
    pub firsts: Vec<FirstOff>,
    pub probes: Vec<u8>,
}

impl NewFeature {
    pub fn new() -> Self {
        NewFeature { embeds: [false; EMBED_N], checklist: Vec::new(), firsts: Vec::new(), probes: Vec::new() }
    }

    /// 嵌入点登记。
    pub fn embed(&mut self, e: Embed) {
        self.embeds[e as usize] = true;
    }

    /// 探针补位：缺哪点补哪点。
    pub fn probe_fill(&mut self) -> usize {
        let mut filled = 0;
        for i in 0..EMBED_N {
            if !self.embeds[i] {
                self.embeds[i] = true;
                self.probes.push(i as u8);
                filled += 1;
            }
        }
        filled
    }

    /// 清单下发（无障碍条目强制——域本色）。
    pub fn issue_checklist(&mut self, items: Vec<(u16, bool)>) -> Result<(), NfCode> {
        // 强制条目须含 a11y 前缀（编号 < 100 视为无障碍条）
        let has_a11y = items.iter().any(|(id, forced)| *forced && *id < 100);
        if !has_a11y {
            return Err(NfCode::ChecklistHole);
        }
        self.checklist = items;
        Ok(())
    }

    /// 违例处置：首犯辅导不记过；重复首犯转正式。
    pub fn violate(&mut self, who: u8) -> Result<(), NfCode> {
        if let Some(f) = self.firsts.iter_mut().find(|f| f.who == who) {
            f.count += 1;
            if f.count >= 2 {
                f.tutoring = false;
                f.formal = true; // 重复首犯转正式记录
                return Err(NfCode::TutorAbuse);
            }
            return Ok(());
        }
        self.firsts.push(FirstOff { who, count: 1, tutoring: true, formal: false });
        Ok(())
    }

    pub fn checklist_forced(&self) -> usize {
        self.checklist.iter().filter(|(_, f)| *f).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C536-01..12。
pub fn run_ves536_checks() -> CheckSet {
    let mut set = CheckSet::new("ves536");

    let mut nf = NewFeature::new();
    nf.embed(Embed::Design);
    nf.embed(Embed::Coding);
    chk(&mut set, "C536-01", nf.probe_fill() == 2, "探针补位两点（联调/收口）");
    chk(&mut set, "C536-02", nf.embeds.iter().all(|e| *e) && nf.probes == alloc::vec![2, 3], "四点嵌入齐+探针账");
    chk(&mut set, "C536-03", nf.probe_fill() == 0, "补齐后探针零补");

    // 清单
    chk(&mut set, "C536-04", nf.issue_checklist(alloc::vec![(200, true), (201, false)]) == Err(NfCode::ChecklistHole), "清单缺无障碍强制条拒绝（域色）");
    let _ = nf.issue_checklist(alloc::vec![(50, true), (200, true), (201, false)]);
    chk(&mut set, "C536-05", nf.checklist.len() == 3 && nf.checklist_forced() == 2, "清单含无障碍条下发");
    chk(&mut set, "C536-06", nf.checklist[0].0 == 50 && nf.checklist[0].1, "无障碍条目强制在册");

    // 首犯辅导
    chk(&mut set, "C536-07", nf.violate(1).is_ok(), "首犯转辅导不记过");
    chk(&mut set, "C536-08", nf.firsts[0].tutoring && !nf.firsts[0].formal, "辅导标记在册");
    chk(&mut set, "C536-09", nf.violate(1) == Err(NfCode::TutorAbuse), "重复首犯转正式记录");
    chk(&mut set, "C536-10", nf.firsts[0].formal && !nf.firsts[0].tutoring, "正式记录替换辅导");
    chk(&mut set, "C536-11", nf.violate(2).is_ok() && nf.firsts.len() == 2, "新人独立首犯账");

    chk(&mut set, "C536-12", EMBED_N == 4 && nf.firsts.iter().filter(|f| f.formal).count() == 1, "四点与首犯账自洽");

    ves536_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：清单确认账本（下发/确认两态归因）+ 守恒恒等式 + 边界探针
// + 强制条整数占比投影 + LCG 违例注入回归 + 幂等闸 + 首犯定长序列化
// 往返 + FNV-1a 清单指纹。

use alloc::string::{String, ToString};

/// 清单条目两态账：下发 → 确认。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChecklistItem {
    pub item: u16,
    pub forced: bool,
    pub acked: bool,
}

/// 清单深化账本。
pub struct ChecklistAudit {
    pub items: Vec<ChecklistItem>,
    pub issued_total: u32,
}

impl ChecklistAudit {
    pub fn new() -> Self {
        ChecklistAudit { items: Vec::new(), issued_total: 0 }
    }

    /// 下发条目（同号重复下发判漂移）。
    pub fn issue(&mut self, item: u16, forced: bool) -> Result<(), NfCode> {
        if self.items.iter().any(|i| i.item == item) {
            return Err(NfCode::AuditDrift);
        }
        self.items.push(ChecklistItem { item, forced, acked: false });
        self.issued_total += 1;
        Ok(())
    }

    /// 确认条目：未下发先确认 → Err；重复确认幂等返回 Ok。
    pub fn ack(&mut self, item: u16) -> Result<(), NfCode> {
        match self.items.iter_mut().find(|i| i.item == item) {
            Some(i) => {
                i.acked = true; // 重复置位零副作用
                Ok(())
            }
            None => Err(NfCode::AuditDrift),
        }
    }

    /// 守恒恒等式：已确认数 ≤ 下发总数，且下发账条目数 ≡ issued_total。
    pub fn conservation(&self) -> bool {
        self.items.iter().filter(|i| i.acked).count() as u32 <= self.issued_total
            && self.items.len() as u32 == self.issued_total
    }

    /// 边界探针：确认账零条目 → 下发账空。
    pub fn empty_probe(&self) -> bool {
        self.items.is_empty() && self.issued_total == 0
    }

    /// 投影：强制条整数占比（百分数，禁浮点）。
    pub fn forced_pct(&self) -> u32 {
        if self.items.is_empty() {
            return 0;
        }
        let f = self.items.iter().filter(|i| i.forced).count();
        f as u32 * 100 / self.items.len() as u32
    }

    /// 投影：未确认条目号分列（升序）。
    pub fn unacked(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.items.iter().filter(|i| !i.acked).map(|i| i.item).collect();
        v.sort_unstable();
        v
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

/// LCG 违例注入回归：随机人员序列随机违例 1..=4 次——
/// 计数 ≥2 必转正式记录（异常必抓）；恰 1 次者必须停留在辅导态
/// （负对照零误转）。任何偏离中文报错。
fn violate_inject(seed: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..64u32 {
        let mut nf = NewFeature::new();
        let who = (rng.next() % 32) as u8;
        let times = (rng.next() % 4) + 1; // 1..=4
        for _ in 0..times {
            let _ = nf.violate(who);
        }
        let f = match nf.firsts.iter().find(|f| f.who == who) {
            Some(f) => *f,
            None => return Err("注入回归：违例未落账".to_string()),
        };
        if times >= 2 && !f.formal {
            return Err("注入回归：重复违例未转正式记录".to_string());
        }
        if times == 1 && (f.formal || !f.tutoring) {
            return Err("注入回归负对照：单次违例被误转正式".to_string());
        }
    }
    Ok(())
}

/// 幂等闸：同嵌入点重复登记零副作用（probes 不增长）。
fn idempotence_probe() -> bool {
    let mut nf = NewFeature::new();
    nf.embed(Embed::Design);
    let n1 = nf.probes.len();
    nf.embed(Embed::Design); // 重复登记
    nf.embed(Embed::Design);
    nf.probes.len() == n1 && nf.embeds[Embed::Design as usize]
}

/// 首犯账定长 4 字节编码：[who, count, 态位(1=辅导 2=正式), 0xA5 哨兵]。
pub fn encode_first(f: &FirstOff) -> [u8; 4] {
    let state = if f.formal { 2u8 } else if f.tutoring { 1 } else { 0 };
    [f.who, f.count, state, 0xA5]
}

/// 解码：哨兵/态位损坏 → FpMismatch。
pub fn decode_first(b: &[u8; 4]) -> Result<FirstOff, NfCode> {
    if b[3] != 0xA5 || b[2] > 2 {
        return Err(NfCode::FpMismatch);
    }
    Ok(FirstOff { who: b[0], count: b[1], tutoring: b[2] == 1, formal: b[2] == 2 })
}

/// FNV-1a 32 位首犯账指纹。
pub fn fnv_firsts(firsts: &[FirstOff]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for f in firsts {
        for byte in encode_first(f) {
            h ^= byte as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// 深化判据并入主判据流（C536-13..22）。
pub fn ves536_deep_checks(set: &mut CheckSet) {
    let mut ca = ChecklistAudit::new();
    let i1 = ca.issue(50, true);
    let i2 = ca.issue(200, true);
    let i3 = ca.issue(201, false);
    set.add("C536-13", i1.is_ok() && i2.is_ok() && i3.is_ok() && ca.conservation(), "清单账守恒（账目数≡下发总数）");
    set.add("C536-14", ca.issue(50, false) == Err(NfCode::AuditDrift), "同号重复下发判账本漂移");
    set.add("C536-15", ca.ack(999) == Err(NfCode::AuditDrift), "未下发先确认探针显性报错");
    let _ = ca.ack(50);
    let _ = ca.ack(50); // 重复确认幂等
    set.add("C536-16", ca.conservation() && ca.unacked() == alloc::vec![200, 201], "确认幂等+未确认分列投影");
    set.add("C536-17", ca.forced_pct() == 66, "强制条整数占比 2/3→66（禁浮点）");
    set.add("C536-18", ChecklistAudit::new().empty_probe(), "空账边界探针");

    match violate_inject(0xBEEF_3601) {
        Ok(()) => set.add("C536-19", true, "LCG 违例注入 64 轮重复必转正式+单次零误转"),
        Err(msg) => set.add_dyn("C536-19", false, &msg),
    }

    set.add("C536-20", idempotence_probe(), "幂等闸：同嵌入点重复登记零副作用");

    let f = FirstOff { who: 9, count: 3, tutoring: false, formal: true };
    match decode_first(&encode_first(&f)) {
        Ok(back) => set.add("C536-21", back.who == f.who && back.count == f.count && back.formal && !back.tutoring, "首犯账 4 字节编码解码往返"),
        Err(_) => set.add("C536-21", false, "首犯账解码异常失败"),
    }
    set.add("C536-22", decode_first(&[9, 3, 9, 0xA5]) == Err(NfCode::FpMismatch) && decode_first(&[9, 3, 2, 0x00]) == Err(NfCode::FpMismatch), "态位/哨兵损坏拒绝解码");

    let mut nf2 = NewFeature::new();
    let _ = nf2.violate(1);
    let fp1 = fnv_firsts(&nf2.firsts);
    let _ = nf2.violate(2);
    set.add("C536-23", fnv_firsts(&nf2.firsts) != fp1, "FNV 首犯账指纹随账目漂移");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_feature() {
        let mut nf = NewFeature::new();
        assert_eq!(nf.probe_fill(), 4);
        assert!(nf.issue_checklist(alloc::vec![(1, true)]).is_ok());
        assert!(nf.violate(1).is_ok());
        assert!(nf.violate(1).is_err());
    }
}
