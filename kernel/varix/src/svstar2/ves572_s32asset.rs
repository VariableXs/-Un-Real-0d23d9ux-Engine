//! VE-F4338 · 一致性性能资产汇总（U07 性能与预算组）
//! 职责：性能组资产汇总——资产清单五类登记（基准样本 / 预算表 / 调试器 /
//! 病态语料库 / 长稳剧本）+ 资产可用性验证（每类逐项可取用验证）+ 资产
//! 移交 U08 测试组复用（移交单对拍）+ 资产失效登记标记并补制 + 验证缺项
//! 补验后移交 + 资产含无障碍性能测试样本（域本色）。
//! 段位：0x39E0（续表 idx572）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x39E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    /// 资产失效（登记标记并补制）。
    AssetDead = SEG_BASE,
    /// 验证缺项（补验后移交）。
    VerifyGap = SEG_BASE + 0x01,
    /// 移交失配（对拍拒绝）。
    HandoverMismatch = SEG_BASE + 0x02,
    /// 资产类越界。
    ClassRange = SEG_BASE + 0x03,
    /// 资产账漂移（分类/履历计数不一致）。
    LedgerDrift = SEG_BASE + 0x04,
}

/// 五类资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    BenchSamples,
    BudgetTable,
    Debugger,
    PathoCorpus,
    LongRunScript,
}

pub const CLASSES: usize = 5;

/// 资产项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Asset {
    pub kind: AssetKind,
    pub id: u32,
    /// 内容指纹（移交对拍 + 失效检测）。
    pub fp: u32,
    pub verified: bool,
    pub dead: bool,
    /// 无障碍样本标记（域本色——至少一项）。
    pub a11y_sample: bool,
}

/// 资产台。
#[derive(Default)]
pub struct AssetDesk {
    pub items: Vec<Asset>,
    pub handover: Option<(u32, u32)>, // (资产 id, 指纹)
    pub remade: u32,
    pub handed: u32,
    /// 深化：失效/补制事件账 (id, 事件类型: 0=失效 1=补制)，时间序。
    pub events: Vec<(u32, u8)>,
    /// 深化：移交回执对账（成功/失配计数）。
    pub ack_ok: u32,
    pub ack_bad: u32,
}

fn fnv(x: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut v = x;
    for _ in 0..4 {
        h ^= v & 0xFF;
        h = h.wrapping_mul(0x01000193);
        v >>= 8;
    }
    h
}

impl AssetDesk {
    pub fn new() -> Self {
        AssetDesk { ..Default::default() }
    }

    /// 资产登记（指纹自动核算——防登记即错）。
    pub fn register(&mut self, kind: AssetKind, id: u32, a11y: bool) {
        if !self.items.iter().any(|x| x.id == id) {
            self.items.push(Asset {
                kind,
                id,
                fp: fnv(id),
                verified: false,
                dead: false,
                a11y_sample: a11y,
            });
        }
    }

    /// 可取用验证：指纹自洽即通过（失效 = 指纹漂移）。
    pub fn verify(&mut self, id: u32, observed_fp: u32) -> Result<(), AsCode> {
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(AsCode::AssetDead)?;
        if x.fp != observed_fp {
            x.dead = true;
            return Err(AsCode::AssetDead);
        }
        x.verified = true;
        Ok(())
    }

    /// 失效补制：dead 资产重新登记指纹并复位（计数留痕）。
    pub fn remake(&mut self, id: u32) -> Result<(), AsCode> {
        let x = self.items.iter_mut().find(|x| x.id == id).ok_or(AsCode::AssetDead)?;
        if !x.dead {
            return Ok(());
        }
        x.fp = fnv(id ^ 0xFFFF);
        x.dead = false;
        x.verified = false;
        self.remade += 1;
        Ok(())
    }

    /// 五类齐备断言（去失效口径）。
    pub fn all_classes(&self) -> bool {
        (0..CLASSES).all(|i| {
            self.items
                .iter()
                .any(|x| !x.dead && x.kind as usize == i)
        })
    }

    /// 移交单出库：全部活资产逐项已验证才可移交（缺项拒绝）。
    pub fn handover(&mut self) -> Result<usize, AsCode> {
        let live: Vec<&Asset> = self.items.iter().filter(|x| !x.dead).collect();
        if live.is_empty() {
            return Err(AsCode::VerifyGap);
        }
        if live.iter().any(|x| !x.verified) {
            return Err(AsCode::VerifyGap);
        }
        self.handover = Some((live[0].id, live[0].fp));
        self.handed += 1;
        Ok(live.len())
    }

    /// 移交对拍：U08 侧指纹回执与移交单比对。
    pub fn ack(&self, id: u32, fp: u32) -> Result<(), AsCode> {
        match self.handover {
            Some((hid, hfp)) if hid == id && hfp == fp => Ok(()),
            _ => Err(AsCode::HandoverMismatch),
        }
    }

    /// 无障碍样本断言（域本色）：至少一项 a11y 样本在册。
    pub fn a11y_present(&self) -> bool {
        self.items.iter().any(|x| !x.dead && x.a11y_sample)
    }

    // ─── 深化块：分类守恒账 ───

    /// 各类资产计数（含死件，去重按类）。
    pub fn class_census(&self) -> [u32; CLASSES] {
        let mut out = [0u32; CLASSES];
        for x in self.items.iter() {
            let k = x.kind as usize;
            if k < CLASSES {
                out[k] += 1;
            }
        }
        out
    }

    /// 分类守恒：五类合计 ≡ 资产总数。
    pub fn census_conserves(&self) -> bool {
        self.class_census().iter().sum::<u32>() == self.items.len() as u32
    }

    // ─── 深化块：事件账（失效/补制）───

    /// 验证入账版：失效留事件（verify 之上的包装）。
    pub fn verify_ledger(&mut self, id: u32, observed_fp: u32) -> Result<(), AsCode> {
        let r = self.verify(id, observed_fp);
        if let Err(AsCode::AssetDead) = r {
            self.events.push((id, 0));
        }
        r
    }

    /// 补制入账版：补制留事件。
    pub fn remake_ledger(&mut self, id: u32) -> Result<(), AsCode> {
        let r = self.remake(id);
        if r.is_ok() {
            self.events.push((id, 1));
        }
        r
    }

    /// 事件账守恒：失效事件数 ≡ 死件当前数 + 补制数（失效总流恒等）。
    pub fn events_conserves(&self) -> bool {
        let dead_n = self.items.iter().filter(|x| x.dead).count() as u32;
        let made = self.events.iter().filter(|(_, t)| *t == 1).count() as u32;
        let died = self.events.iter().filter(|(_, t)| *t == 0).count() as u32;
        died == dead_n + made
    }

    // ─── 深化块：移交履历对账 ───

    /// 回执入账版：成功/失配都计数。
    pub fn ack_ledger(&mut self, id: u32, fp: u32) -> Result<(), AsCode> {
        match self.ack(id, fp) {
            Ok(()) => {
                self.ack_ok += 1;
                Ok(())
            }
            Err(e) => {
                self.ack_bad += 1;
                Err(e)
            }
        }
    }

    /// 履历守恒：成功+失配 ≡ 回执总数。
    pub fn ack_conserves(&self, receipts: u32) -> bool {
        self.ack_ok + self.ack_bad == receipts
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves572_checks() -> CheckSet {
    let mut set = CheckSet::new("ves572");
    let mut ad = AssetDesk::new();

    // C572-01 五类资产登记（含无障碍样本）。
    ad.register(AssetKind::BenchSamples, 1, false);
    ad.register(AssetKind::BudgetTable, 2, false);
    ad.register(AssetKind::Debugger, 3, false);
    ad.register(AssetKind::PathoCorpus, 4, false);
    ad.register(AssetKind::LongRunScript, 5, true);
    chk(&mut set, "C572-01", ad.items.len() == 5 && ad.a11y_present(), "五类登记含无障碍样本");

    // C572-02 可取用验证通过（指纹自洽）。
    let mut all_ok = true;
    for id in 1..=5u32 {
        if ad.verify(id, fnv(id)).is_err() {
            all_ok = false;
        }
    }
    chk(&mut set, "C572-02", all_ok, "五项验证通过");

    // C572-03 指纹漂移 = 资产失效。
    chk(&mut set, "C572-03", ad.verify(2, 0xDEAD) == Err(AsCode::AssetDead) && ad.items[1].dead, "指纹漂移判失效");

    // C572-04 失效阻断移交（五类缺一）。
    chk(&mut set, "C572-04", ad.handover() == Err(AsCode::VerifyGap), "失效缺类阻断移交");

    // C572-05 失效补制复位（计数）。
    chk(&mut set, "C572-05", ad.remake(2).is_ok() && !ad.items[1].dead && ad.remade == 1, "补制复位留痕");

    // C572-06 补制后须重验。
    chk(&mut set, "C572-06", ad.handover() == Err(AsCode::VerifyGap) && ad.verify(2, fnv(2 ^ 0xFFFF)).is_ok(), "补制重验");

    // C572-07 全验后移交成功（单对拍）。
    let n = ad.handover();
    chk(&mut set, "C572-07", n == Ok(5) && ad.handed == 1, "全验移交");

    // C572-08 移交回执对拍一致。
    chk(&mut set, "C572-08", ad.ack(1, fnv(1)).is_ok(), "回执对拍一致");

    // C572-09 回执失配拒绝。
    chk(&mut set, "C572-09", ad.ack(1, 0xBEEF) == Err(AsCode::HandoverMismatch), "回执失配拒");

    // C572-10 未移交先回执拒。
    let ad2 = AssetDesk::new();
    chk(&mut set, "C572-10", ad2.ack(1, fnv(1)) == Err(AsCode::HandoverMismatch), "未移交回执拒");

    // C572-11 未验证资产阻断移交（缺项语义）。
    let mut ad3 = AssetDesk::new();
    ad3.register(AssetKind::Debugger, 9, false);
    chk(&mut set, "C572-11", ad3.handover() == Err(AsCode::VerifyGap), "未验证阻断");

    // C572-12 空台移交拒绝（不臆测）+ 幂等登记。
    let mut ad4 = AssetDesk::new();
    ad4.register(AssetKind::Debugger, 1, false);
    ad4.register(AssetKind::Debugger, 1, false);
    chk(&mut set, "C572-12", ad4.handover().is_err() && ad4.items.len() == 1, "空台拒+幂等");

    // ── 深化判据：分类守恒账 ──
    // ad：五类各 1 件。
    let cen = ad.class_census();
    chk(&mut set, "C572-13", cen == [1, 1, 1, 1, 1] && ad.census_conserves(),
        "五类一一件分类守恒");
    // 同类多件 + 篡改检出。
    let mut ac = AssetDesk::new();
    ac.register(AssetKind::Debugger, 1, false);
    ac.register(AssetKind::Debugger, 2, false);
    ac.register(AssetKind::BudgetTable, 3, false);
    chk(&mut set, "C572-14", ac.class_census() == [0, 1, 2, 0, 0]
        && ac.census_conserves(), "同类多件分类守恒");

    // ── 深化判据：事件账恒等（失效流 ≡ 当前死件 + 补制）──
    let mut ae = AssetDesk::new();
    ae.register(AssetKind::Debugger, 1, false);
    let _ = ae.verify_ledger(1, 0xDEAD); // 失效
    chk(&mut set, "C572-15", ae.events.len() == 1 && ae.events_conserves(),
        "失效事件留痕恒等");
    let _ = ae.remake_ledger(1); // 补制
    chk(&mut set, "C572-16", ae.events.len() == 2 && ae.events_conserves(),
        "补制后恒等仍守");

    // ── 深化判据：移交履历对账 ──
    let mut aa = AssetDesk::new();
    aa.register(AssetKind::Debugger, 7, false);
    let _ = aa.verify(7, fnv(7));
    let _ = aa.handover();
    let _ = aa.ack_ledger(7, fnv(7));      // 成功
    let _ = aa.ack_ledger(7, 0xBEEF);      // 失配
    let _ = aa.ack_ledger(9, fnv(9));      // 无件失配
    chk(&mut set, "C572-17", aa.ack_ok == 1 && aa.ack_bad == 2
        && aa.ack_conserves(3), "回执 1 成 2 失配守恒");
    chk(&mut set, "C572-18", !aa.ack_conserves(4), "回执总数不符守恒即破");

    // ── 深化判据：LCG 指纹漂移注入（必抓率 100%）──
    let mut rng: u64 = 0x5720_DEAD;
    let mut ai = AssetDesk::new();
    let mut caught = 0u32;
    for i in 0..64u32 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ai.register(AssetKind::Debugger, i, false);
        let drift = rng % 2 == 0;
        let observed = if drift { rng as u32 } else { fnv(i) };
        if ai.verify_ledger(i, observed).is_err() {
            caught += 1;
        }
    }
    chk(&mut set, "C572-19", caught == 32 && ai.events_conserves(),
        "64 轮漂移注入 32/32 全抓");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_dead_remake() {
        let mut ad = AssetDesk::new();
        ad.register(AssetKind::Debugger, 1, false);
        assert_eq!(ad.verify(1, 9), Err(AsCode::AssetDead));
        assert!(ad.items[0].dead);
        assert!(ad.remake(1).is_ok());
        assert!(!ad.items[0].dead);
        assert!(!ad.items[0].verified);
        assert!(ad.verify(1, fnv(1 ^ 0xFFFF)).is_ok());
    }

    #[test]
    fn handover_needs_all_verified() {
        let mut ad = AssetDesk::new();
        for i in 0..5u32 {
            ad.register(AssetKind::BenchSamples, i, i == 4);
        }
        assert_eq!(ad.handover(), Err(AsCode::VerifyGap));
        for i in 0..5u32 {
            let _ = ad.verify(i, fnv(i));
        }
        assert_eq!(ad.handover(), Ok(5));
    }

    #[test]
    fn event_ledger_conserves() {
        let mut ad = AssetDesk::new();
        ad.register(AssetKind::Debugger, 1, false);
        let _ = ad.verify_ledger(1, 9);
        assert!(ad.events_conserves());
        let _ = ad.remake_ledger(1);
        assert!(ad.events_conserves());
    }

    #[test]
    fn ack_ledger_counts() {
        let mut ad = AssetDesk::new();
        let _ = ad.ack_ledger(1, 2); // 无移交失配
        assert_eq!(ad.ack_bad, 1);
        assert!(ad.ack_conserves(1));
    }
}
