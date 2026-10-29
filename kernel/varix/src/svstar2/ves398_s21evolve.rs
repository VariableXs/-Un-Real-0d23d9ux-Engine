//! ves398_s21evolve — VE-F4158 · i18n 演进预留
//! T 域预留面声明四口（新语言族接入位/新地区规则源位/新机翻引擎位/新方向书写系统位）；
//! 预留纪律（预留不预写——只留接口与断言位）；域级增口登记；
//! 预写越界→回退；口被占→迁移评审；登记缺→补登；四口均含无障碍适配位说明（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0060;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    Ok = 0,
    PreWriteBreach = SEG_BASE + 0x01,  // 0x0061 预写越界
    SlotOccupied = SEG_BASE + 0x02,    // 0x0062 口被占
    RegisterMissing = SEG_BASE + 0x03, // 0x0063 登记缺
    LedgerBroken = SEG_BASE + 0x04,    // 0x0064 预留账本守恒破（二轮深化）
    FingerprintDrift = SEG_BASE + 0x05, // 0x0065 指纹漂移（二轮深化）
    ProbeReject = SEG_BASE + 0x06,     // 0x0066 边界探针拒绝（二轮深化）
}

/// 四口预留位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReserveSlot {
    NewLangFamily = 0,     // 新语言族接入位
    NewRegionRuleSrc = 1,  // 新地区规则源位
    NewMtEngine = 2,       // 新机翻引擎位
    NewScriptSystem = 3,   // 新方向书写系统位
}

impl ReserveSlot {
    pub const ALL: [ReserveSlot; 4] = [
        ReserveSlot::NewLangFamily, ReserveSlot::NewRegionRuleSrc,
        ReserveSlot::NewMtEngine, ReserveSlot::NewScriptSystem];
    pub fn name(self) -> &'static str {
        match self {
            ReserveSlot::NewLangFamily => "new-lang-family",
            ReserveSlot::NewRegionRuleSrc => "new-region-rule-src",
            ReserveSlot::NewMtEngine => "new-mt-engine",
            ReserveSlot::NewScriptSystem => "new-script-system",
        }
    }
}

/// 接口位：只留签名与断言位，不得预写实现（红线）。
pub struct IfaceStub {
    pub slot: ReserveSlot,
    pub signature: &'static str,     // 接口签名（空 = 未留）
    pub assertion_site: bool,        // 断言位就位
    pub a11y_adapter_note: bool,     // 无障碍适配位说明（域本色——必须 true）
    pub occupied_by: Option<&'static str>, // 口被占时的占用者
}

impl IfaceStub {
    /// 预留合规：留接口 + 留断言位 + 无障碍适配说明，且无实现预写。
    pub fn compliant(&self) -> bool {
        !self.signature.is_empty() && self.assertion_site && self.a11y_adapter_note
            && self.occupied_by.is_none()
    }
}

/// 预留登记册。
pub struct ReserveBook {
    pub stubs: Vec<IfaceStub>,
    pub registrations: Vec<&'static str>, // 域级增口登记
    pub rollbacks: Vec<&'static str>,     // 预写越界回退留痕
    pub reviews: Vec<&'static str>,       // 迁移评审留痕
}

impl ReserveBook {
    pub fn new() -> Self {
        ReserveBook { stubs: Vec::new(), registrations: Vec::new(), rollbacks: Vec::new(), reviews: Vec::new() }
    }

    /// 建四口（空口登记——登记缺→补登）。
    pub fn init_slots(&mut self) {
        for s in ReserveSlot::ALL.iter() {
            if !self.stubs.iter().any(|x| x.slot == *s) {
                self.stubs.push(IfaceStub {
                    slot: *s, signature: "", assertion_site: false,
                    a11y_adapter_note: true, occupied_by: None,
                });
            }
        }
    }

    pub fn entry(&mut self, s: ReserveSlot) -> Option<&mut IfaceStub> {
        self.stubs.iter_mut().find(|x| x.slot == s)
    }
    pub fn get(&self, s: ReserveSlot) -> Option<&IfaceStub> {
        self.stubs.iter().find(|x| x.slot == s)
    }

    /// 留接口位（只签名与断言位）。
    pub fn stub_iface(&mut self, s: ReserveSlot, sig: &'static str, assertion: bool) -> bool {
        match self.entry(s) {
            Some(e) if e.occupied_by.is_none() => {
                e.signature = sig;
                e.assertion_site = assertion;
                true
            }
            _ => false,
        }
    }

    /// 预写越界检测：口上出现实现体（signature 含 "impl-body"）→ 强制回退。
    pub fn audit_prewrite(&mut self) -> usize {
        let mut n = 0;
        for e in self.stubs.iter_mut() {
            if e.signature.contains("impl-body") {
                e.signature = "";
                e.assertion_site = false;
                self.rollbacks.push(e.slot.name());
                n += 1;
            }
        }
        n
    }

    /// 口被占 → 迁移评审。
    pub fn occupy(&mut self, s: ReserveSlot, who: &'static str) -> Result<(), EvCode> {
        match self.entry(s) {
            Some(e) => {
                if e.occupied_by.is_some() {
                    self.reviews.push("slot-migration-review");
                    Err(EvCode::SlotOccupied)
                } else {
                    e.occupied_by = Some(who);
                    Ok(())
                }
            }
            None => Err(EvCode::RegisterMissing),
        }
    }

    /// 域级增口登记（幂等）。
    pub fn register_extra(&mut self, name: &'static str) -> bool {
        if self.registrations.contains(&name) { return false; }
        self.registrations.push(name);
        true
    }

    /// 登记完备：四口全登记 + 均合规（空口允许——预留不预写）。
    pub fn audit(&self) -> Vec<&'static str> {
        ReserveSlot::ALL.iter()
            .filter(|s| match self.get(**s) {
                None => true,
                Some(e) => !e.compliant() && e.occupied_by.is_none(),
            })
            .map(|s| s.name()).collect()
    }
}

// ============ 二轮深化块：回退结构化账本 / 预留守恒 / FNV 幂等闸 / 边界探针 ============

/// [深化] 回退结构化账本：按口记回退笔数 + 守恒（Σ口笔数 ≡ 总回退数）。
#[derive(Clone, Copy)]
pub struct RollbackLedger {
    pub per_slot: [u32; 4],
    pub total: u32,
}

impl RollbackLedger {
    pub fn new() -> Self { RollbackLedger { per_slot: [0; 4], total: 0 } }

    pub fn record(&mut self, s: ReserveSlot) {
        self.per_slot[s as usize] += 1;
        self.total += 1;
    }

    /// 守恒恒等式：四口之和 ≡ 总数。
    pub fn conserved(&self) -> bool {
        self.per_slot.iter().sum::<u32>() == self.total
    }

    /// 回退最重口查询（并列取先，空账 None）。
    pub fn heaviest_slot(&self) -> Option<ReserveSlot> {
        let mut best: Option<(usize, u32)> = None;
        for (i, c) in self.per_slot.iter().enumerate() {
            if *c > 0 {
                match best {
                    None => best = Some((i, *c)),
                    Some((_, m)) if *c > m => best = Some((i, *c)),
                    _ => {}
                }
            }
        }
        best.map(|(i, _)| ReserveSlot::ALL[i])
    }
}

/// [深化] 预留口守恒恒等式：每口恰一桩，无重复无缺。
pub fn slot_conservation(bk: &ReserveBook) -> bool {
    ReserveSlot::ALL.iter()
        .all(|s| bk.stubs.iter().filter(|e| e.slot == *s).count() == 1)
}

/// [深化] 预留册定长字节投影（6B/口，禁 JSON）：
/// slot u8 + 签名标记 u8 + 签名长 u16le + a11y u8 + 占用 u8。
pub fn reserve_bytes(bk: &ReserveBook, out: &mut Vec<u8>) {
    out.clear();
    for s in ReserveSlot::ALL.iter() {
        match bk.get(*s) {
            Some(e) => {
                out.push(*s as u8);
                out.push(if e.signature.is_empty() { 0 } else { 1 });
                out.extend_from_slice(&(e.signature.len() as u16).to_le_bytes());
                out.push(if e.a11y_adapter_note { 1 } else { 0 });
                out.push(if e.occupied_by.is_some() { 1 } else { 0 });
            }
            None => {
                out.push(*s as u8);
                out.push(0xFF);
                out.extend_from_slice(&0xFFFFu16.to_le_bytes());
                out.push(0);
                out.push(0);
            }
        }
    }
}

/// [深化] FNV-1a 64 位指纹（纯整数运算）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// [深化] 幂等指纹闸：同态二次投影指纹必须一致。
pub fn reserve_fingerprint_stable(bk: &ReserveBook) -> Result<u64, EvCode> {
    let mut b1: Vec<u8> = Vec::new();
    reserve_bytes(bk, &mut b1);
    let fp1 = fnv1a64(&b1);
    let mut b2: Vec<u8> = Vec::new();
    reserve_bytes(bk, &mut b2);
    let fp2 = fnv1a64(&b2);
    if b1.len() != b2.len() || fp1 != fp2 { Err(EvCode::FingerprintDrift) } else { Ok(fp1) }
}

/// [深化] 预留册边界探针。
pub struct ReserveProbe;

impl ReserveProbe {
    /// 空册占用探针：透传 RegisterMissing（显性回执）。
    pub fn probe_empty_occupy(bk: &mut ReserveBook, s: ReserveSlot) -> Result<(), EvCode> {
        bk.occupy(s, "probe")
    }

    /// 越界序号取口 → ProbeReject（不 panic）。
    pub fn probe_index(bk: &ReserveBook, idx: usize) -> Result<ReserveSlot, EvCode> {
        match bk.stubs.get(idx) {
            Some(e) => Ok(e.slot),
            None => Err(EvCode::ProbeReject),
        }
    }

    /// 预写审计幂等闸：二次审计零新增 + 留痕笔数一致。
    pub fn audit_idempotent(bk: &mut ReserveBook) -> bool {
        let before = bk.rollbacks.len();
        let n1 = bk.audit_prewrite();
        let n2 = bk.audit_prewrite();
        n2 == 0 && bk.rollbacks.len() == before + n1
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves398_checks() -> CheckSet {
    let mut set = CheckSet::new("ves398");
    let mut bk = ReserveBook::new();

    // C398-01 四口建册（空口登记）。
    bk.init_slots();
    chk(&mut set, "C398-01", bk.stubs.len() == 4, "四口登记");

    // C398-02 留接口位（签名 + 断言位 + a11y 适配说明）。
    chk(&mut set, "C398-02", bk.stub_iface(ReserveSlot::NewLangFamily, "fn attach_lang_family(sig)", true)
        && bk.get(ReserveSlot::NewLangFamily).unwrap().compliant(), "接口位合规");

    // C398-03 预留不预写：空口合法（不留实现）。
    chk(&mut set, "C398-03", bk.get(ReserveSlot::NewMtEngine).unwrap().signature.is_empty()
        && bk.audit().len() == 3, "空口合法余三口待留");

    // C398-04 预写越界 → 回退留痕。
    bk.stub_iface(ReserveSlot::NewScriptSystem, "impl-body: heavy logic", false);
    let n = bk.audit_prewrite();
    chk(&mut set, "C398-04", n == 1 && bk.rollbacks == vec!["new-script-system"]
        && bk.get(ReserveSlot::NewScriptSystem).unwrap().signature.is_empty(), "预写回退留痕");

    // C398-05 域级增口登记（幂等）。
    chk(&mut set, "C398-05", bk.register_extra("new-ime-family") && !bk.register_extra("new-ime-family")
        && bk.registrations.len() == 1, "增口登记幂等");

    // C398-06 口被占 → 迁移评审。
    bk.occupy(ReserveSlot::NewLangFamily, "lang-fam-v2").unwrap();
    chk(&mut set, "C398-06", bk.occupy(ReserveSlot::NewLangFamily, "lang-fam-v3")
        == Err(EvCode::SlotOccupied) && bk.reviews.len() == 1, "重占触发迁移评审");

    // C398-07 未登记口 → 登记缺。
    let mut bk2 = ReserveBook::new();
    chk(&mut set, "C398-07", bk2.occupy(ReserveSlot::NewMtEngine, "mt-x") == Err(EvCode::RegisterMissing), "未建册占用拒");

    // C398-08 四口全留接口后审计归零。
    bk.stub_iface(ReserveSlot::NewRegionRuleSrc, "fn region_rule_src(id)", true);
    bk.stub_iface(ReserveSlot::NewMtEngine, "fn mt_engine_if()", true);
    bk.stub_iface(ReserveSlot::NewScriptSystem, "fn script_dir_if()", true);
    chk(&mut set, "C398-08", bk.audit().is_empty(), "四口全合规");

    // C398-09 a11y 适配位说明（域本色）四口必备。
    chk(&mut set, "C398-09", bk.stubs.iter().all(|e| e.a11y_adapter_note), "四口 a11y 适配位");

    // C398-10 四口名单单源。
    chk(&mut set, "C398-10", ReserveSlot::ALL.len() == 4
        && ReserveSlot::ALL[0].name() == "new-lang-family"
        && ReserveSlot::ALL[3].name() == "new-script-system", "四口单源");

    // C398-11 断言位缺 → 审计红。
    bk.entry(ReserveSlot::NewMtEngine).unwrap().assertion_site = false;
    chk(&mut set, "C398-11", bk.audit() == vec!["new-mt-engine"], "缺断言位检出");

    // C398-12 建册幂等（重复 init 不增口）。
    bk.init_slots();
    chk(&mut set, "C398-12", bk.stubs.len() == 4, "建册幂等");

    // ===== 二轮深化判据（C398-13 起）=====
    // C398-13 预留口守恒：每口恰一桩。
    chk(&mut set, "C398-13", slot_conservation(&bk) && bk.stubs.len() == 4, "四口守恒一桩一口");

    // C398-14 回退账本：按口记账 + 守恒 + 最重口定位。
    let mut rl = RollbackLedger::new();
    rl.record(ReserveSlot::NewScriptSystem);
    chk(&mut set, "C398-14", rl.conserved() && rl.total == 1
        && rl.heaviest_slot() == Some(ReserveSlot::NewScriptSystem)
        && RollbackLedger::new().heaviest_slot().is_none(), "回退账本守恒最重口");

    // C398-15 预写审计幂等闸：二次审计零新增。
    chk(&mut set, "C398-15", ReserveProbe::audit_idempotent(&mut bk), "审计幂等零新增");

    // C398-16 预留册投影：24B 定长 + 指纹幂等稳定。
    let mut rb: Vec<u8> = Vec::new();
    reserve_bytes(&bk, &mut rb);
    match reserve_fingerprint_stable(&bk) {
        Ok(v) => chk(&mut set, "C398-16", rb.len() == 24 && reserve_fingerprint_stable(&bk) == Ok(v),
            "投影 24B 指纹幂等"),
        Err(_) => chk(&mut set, "C398-16", false, "指纹不应漂移"),
    }

    // C398-17 边界探针：越界序号拒 + 界内取口。
    chk(&mut set, "C398-17", ReserveProbe::probe_index(&bk, 0) == Ok(ReserveSlot::NewLangFamily)
        && ReserveProbe::probe_index(&bk, 9) == Err(EvCode::ProbeReject), "越界探针拒界内取口");

    // C398-18 空册占用探针：显性 RegisterMissing。
    let mut bk3 = ReserveBook::new();
    chk(&mut set, "C398-18",
        ReserveProbe::probe_empty_occupy(&mut bk3, ReserveSlot::NewMtEngine) == Err(EvCode::RegisterMissing)
        && !slot_conservation(&bk3), "空册占用拒守恒破");

    // C398-19 深化变体段位不撞段。
    chk(&mut set, "C398-19", EvCode::LedgerBroken as u16 == 0x0064
        && EvCode::FingerprintDrift as u16 == 0x0065 && EvCode::ProbeReject as u16 == 0x0066,
        "深化段位正确");

    // C398-20 占用口审计豁免：被占口不进待留清单（audit 语义边界）。
    let audit_n = bk.audit().len();
    chk(&mut set, "C398-20", audit_n == 1 && bk.audit() == vec!["new-mt-engine"],
        "占用口豁免仅缺断言位红");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves398_smoke() {
        let set = run_ves398_checks();
        assert!(set.failed_count() == 0, "ves398 red: {}", set.red_items_text());
    }

    #[test]
    fn ves398_no_a11y_note_fails() {
        let s = IfaceStub { slot: ReserveSlot::NewMtEngine, signature: "f", assertion_site: true, a11y_adapter_note: false, occupied_by: None };
        assert!(!s.compliant());
    }
}
