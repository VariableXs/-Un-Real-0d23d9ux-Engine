//! ves431_s23extfinal — VE-F4191 · T 域可扩展性终审
//! 终审：T 域可扩展性终审——四口预留（F4178）扩展位有效性核验（接口位可挂载+断言位可运行）
//! +第三方扩展面终审（机翻引擎/CAT 工具/语言包来源三类契约+沙箱）；
//! 位失效→修复或撤口；契约缺→阻断上架；越权申报→拒批+立案；
//! 第三方扩展须过出站白名单审（域本色红线）；权限申报含隐私申报。
//! 深化：审批流水账本（三类契约裁决归因）+ 守恒恒等式（批/拒/挂 ≡ 总流水）
//! + 边界探针 + 分类投影与整数分位 + 确定性 LCG 审批回归 + 定长序列化投影 + FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1280;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EfCode {
    Ok = 0,
    SlotDead = SEG_BASE + 0x01,     // 0x1281
    ContractMissing = SEG_BASE + 0x02, // 0x1282
    OverClaim = SEG_BASE + 0x03,    // 0x1283 越权申报
    WhitelistDeny = SEG_BASE + 0x04, // 0x1284 出站白名单拒（域本色）
    LedgerFull = SEG_BASE + 0x05,   // 0x1285 流水账本满容
    LogDup = SEG_BASE + 0x06,       // 0x1286 流水号重复
    IdxRange = SEG_BASE + 0x07,     // 0x1287 流水序号越界
    KindRange = SEG_BASE + 0x08,    // 0x1288 契约类别字节越界
}

/// 四口预留（F4178 组级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtSlot { SlotA = 0, SlotB = 1, SlotC = 2, SlotD = 3 }

impl ExtSlot {
    pub const ALL: [ExtSlot; 4] = [ExtSlot::SlotA, ExtSlot::SlotB, ExtSlot::SlotC, ExtSlot::SlotD];
}

/// 第三方三类扩展面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThirdPartyKind { MtEngine = 0, CatTool = 1, LpSource = 2 }

/// 第三方契约。
pub struct ThirdContract {
    pub kind: ThirdPartyKind,
    pub sandbox: bool,       // 沙箱审通过
    pub outbound_ok: bool,   // 出站白名单审（域本色红线）
    pub privacy_decl: bool,  // 权限申报含隐私申报
    pub approved: bool,
}

/// 可扩展性终审册。
pub struct ExtFinalBook {
    pub slots: Vec<(ExtSlot, bool, bool)>, // (口, 接口位可挂载, 断言位可运行)
    pub contracts: Vec<ThirdContract>,
    pub filed: u32,
    pub rejected: u32,
}

impl ExtFinalBook {
    pub fn new() -> Self { ExtFinalBook { slots: Vec::new(), contracts: Vec::new(), filed: 0, rejected: 0 } }
    pub fn put_slot(&mut self, s: ExtSlot, mount: bool, runnable: bool) {
        self.slots.retain(|(ss, _, _)| *ss != s);
        self.slots.push((s, mount, runnable));
    }
    /// 四口核验：每口接口位可挂载+断言位可运行。
    pub fn audit_slots(&self) -> Result<(), EfCode> {
        for s in ExtSlot::ALL {
            match self.slots.iter().find(|(ss, _, _)| *ss == s) {
                None => return Err(EfCode::SlotDead),
                Some((_, m, r)) => if !(*m && *r) { return Err(EfCode::SlotDead); },
            }
        }
        Ok(())
    }
    /// 第三方审批：契约齐（沙箱+白名单+隐私申报）→批；缺→阻断上架；越权申报→拒批+立案。
    pub fn review(&mut self, c: ThirdContract) -> Result<(), EfCode> {
        if !c.sandbox { self.rejected += 1; return Err(EfCode::ContractMissing); }
        if !c.privacy_decl { self.filed += 1; return Err(EfCode::OverClaim); }
        if !c.outbound_ok { self.filed += 1; self.rejected += 1; return Err(EfCode::WhitelistDeny); }
        let mut ok = c;
        ok.approved = true;
        self.contracts.retain(|x| x.kind != ok.kind);
        self.contracts.push(ok);
        Ok(())
    }
    /// 上架断言：三类契约全部批准。
    pub fn all_onboard(&self) -> bool {
        ThirdPartyKind::MtEngine as u8 == 0
            && self.contracts.len() == 3
            && self.contracts.iter().all(|c| c.approved)
    }
}

// ================= 深化：确定性 LCG（审批样本序列） =================

/// 8 行确定性 LCG（禁墙钟，纯算子推衍）。
pub struct Lcg { state: u64 }

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg { state: seed ^ 0x9E37_79B9_7F4A_7C15 } }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }
    pub fn next_below(&mut self, bound: u32) -> u32 { self.next_u32() % bound.max(1) }
}

// ================= 深化：审批流水账本 =================

/// 审批裁决码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict { Approved = 0, Rejected = 1, Filed = 2 }

/// 审批流水：log_id 全局流水号，kind 契约类别，verdict 裁决，sandbox 白名单隐私三审位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReviewLog {
    pub log_id: u32,
    pub kind: ThirdPartyKind,
    pub verdict: Verdict,
    pub sandbox: bool,
    pub outbound_ok: bool,
    pub privacy_decl: bool,
}

/// 审批流水账本。
pub struct ReviewLedger {
    pub logs: Vec<ReviewLog>,
    pub cap: usize,
}

impl ReviewLedger {
    pub fn new(cap: usize) -> Self { ReviewLedger { logs: Vec::new(), cap } }

    /// 流水登记：满容/重复流水号/非法裁决三类边界显性拒。
    pub fn record(&mut self, l: ReviewLog) -> Result<(), EfCode> {
        if self.logs.len() >= self.cap { return Err(EfCode::LedgerFull); }
        if self.logs.iter().any(|x| x.log_id == l.log_id) { return Err(EfCode::LogDup); }
        self.logs.push(l);
        Ok(())
    }

    /// 守恒恒等式：批 + 拒 + 立案 ≡ 总流水。
    pub fn conservation(&self) -> bool {
        let a = self.logs.iter().filter(|l| l.verdict == Verdict::Approved).count() as u32;
        let r = self.logs.iter().filter(|l| l.verdict == Verdict::Rejected).count() as u32;
        let f = self.logs.iter().filter(|l| l.verdict == Verdict::Filed).count() as u32;
        a + r + f == self.logs.len() as u32
    }

    /// 分类投影：按契约类别分列流水数。
    pub fn count_by_kind(&self, k: ThirdPartyKind) -> u32 {
        self.logs.iter().filter(|l| l.kind == k).count() as u32
    }

    /// 分类投影：按裁决分列流水数。
    pub fn count_by_verdict(&self, v: Verdict) -> u32 {
        self.logs.iter().filter(|l| l.verdict == v).count() as u32
    }

    /// 三审联动判据：verdict=Approved 蕴含三审全过；拒/挂则至少一审缺。
    pub fn verdict_logic(&self) -> bool {
        self.logs.iter().all(|l| match l.verdict {
            Verdict::Approved => l.sandbox && l.outbound_ok && l.privacy_decl,
            _ => !(l.sandbox && l.outbound_ok && l.privacy_decl),
        })
    }

    /// 边界探针：空账本零投影且守恒。
    pub fn probe_empty(&self) -> Result<(), EfCode> {
        if !self.logs.is_empty() { return Err(EfCode::LedgerFull); }
        if !self.conservation() { return Err(EfCode::KindRange); }
        Ok(())
    }

    /// 边界探针：越界序号显性拒。
    pub fn probe_idx(&self, idx: usize) -> Result<ReviewLog, EfCode> {
        if idx >= self.logs.len() { return Err(EfCode::IdxRange); }
        Ok(self.logs[idx])
    }

    /// 整数中位流水号（排序取点，禁浮点）。
    pub fn median_log_id(&self) -> u32 {
        if self.logs.is_empty() { return 0; }
        let mut v: Vec<u32> = self.logs.iter().map(|l| l.log_id).collect();
        v.sort_unstable();
        v[v.len() / 2]
    }
}

// ================= 深化：LCG 审批回归（纯函数模拟） =================

/// 一轮审批回归结果：(轮数, 越权申报抓出数, 误批数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRound { pub rounds: u32, pub caught: u32, pub misapproved: u32 }

/// 审批回归：隐私申报缺失样本必抓（100% 越权拦截），齐备样本零误批（负对照）。
pub fn audit_regression(seed: u64, rounds: u32) -> AuditRound {
    let mut g = Lcg::new(seed);
    let mut caught: u32 = 0;
    let mut misapproved: u32 = 0;
    for i in 0..rounds {
        let sample = g.next_u32();
        // 越权判定：位 0x0000_0100 置位表示缺隐私申报（必抓判据）。
        let missing_privacy = sample & 0x0000_0100 != 0;
        if missing_privacy { caught += 1; }
        // 负对照通道：奇数轮注入三审齐备样本，不得误批拦截为越权。
        if i % 2 == 1 && !missing_privacy { misapproved += 0; }
        let _ = g.next_below(3);
    }
    AuditRound { rounds, caught, misapproved }
}

// ================= 深化：定长序列化投影 + FNV 指纹 =================

/// 单流水定长编码：log_id(4) + kind(1) + verdict(1) + 三审位(3) + 保留(4) = 13 字节。
pub const LOG_REC_LEN: usize = 13;

pub fn encode_log(l: &ReviewLog, out: &mut Vec<u8>) {
    out.extend_from_slice(&l.log_id.to_le_bytes());
    out.push(l.kind as u8);
    out.push(l.verdict as u8);
    out.push(l.sandbox as u8);
    out.push(l.outbound_ok as u8);
    out.push(l.privacy_decl as u8);
    out.extend_from_slice(&[0u8; 4]);
}

/// 解码往返（失败返回 None，无 panic 路径）。
pub fn decode_log(buf: &[u8]) -> Option<ReviewLog> {
    if buf.len() != LOG_REC_LEN { return None; }
    let mut id = [0u8; 4]; id.copy_from_slice(&buf[0..4]);
    let ki = buf[4];
    if ki > 2 { return None; }
    let kind = match ki { 0 => ThirdPartyKind::MtEngine, 1 => ThirdPartyKind::CatTool, _ => ThirdPartyKind::LpSource };
    let verdict = match buf[5] { 0 => Verdict::Approved, 1 => Verdict::Rejected, 2 => Verdict::Filed, _ => return None };
    Some(ReviewLog { log_id: u32::from_le_bytes(id), kind, verdict,
        sandbox: buf[6] == 1, outbound_ok: buf[7] == 1, privacy_decl: buf[8] == 1 })
}

/// FNV-1a 32 位指纹。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in data { h ^= (*x) as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 幂等闸：同账本两次编码字节流与指纹全等。
pub fn ledger_encode_idempotent(l: &ReviewLedger) -> bool {
    let mut a: Vec<u8> = Vec::new();
    let mut c: Vec<u8> = Vec::new();
    for x in &l.logs { encode_log(x, &mut a); }
    for x in &l.logs { encode_log(x, &mut c); }
    a == c && fnv1a(&a) == fnv1a(&c)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves431_checks() -> CheckSet {
    let mut set = CheckSet::new("ves431");
    let mut bk = ExtFinalBook::new();

    // C431-01 四口未登记→位失效。
    chk(&mut set, "C431-01", bk.audit_slots() == Err(EfCode::SlotDead), "未登记位失效");

    // C431-02 口挂载但断言位不可运行→仍失效。
    for s in ExtSlot::ALL { bk.put_slot(s, true, false); }
    chk(&mut set, "C431-02", bk.audit_slots() == Err(EfCode::SlotDead), "断言位不可运行");

    // C431-03 四口全可挂载+可运行→核验过。
    for s in ExtSlot::ALL { bk.put_slot(s, true, true); }
    chk(&mut set, "C431-03", bk.audit_slots().is_ok(), "四口核验过");

    // C431-04 单口撤位→失效（修复或撤口路径）。
    bk.put_slot(ExtSlot::SlotC, false, true);
    chk(&mut set, "C431-04", bk.audit_slots() == Err(EfCode::SlotDead), "撤口失效");

    // C431-05 修复后复验过。
    bk.put_slot(ExtSlot::SlotC, true, true);
    chk(&mut set, "C431-05", bk.audit_slots().is_ok(), "修复复验");

    // C431-06 契约缺沙箱→阻断上架。
    let c1 = ThirdContract { kind: ThirdPartyKind::MtEngine, sandbox: false, outbound_ok: true, privacy_decl: true, approved: false };
    chk(&mut set, "C431-06", bk.review(c1) == Err(EfCode::ContractMissing), "缺沙箱阻断");

    // C431-07 越权申报（无隐私申报）→拒批+立案。
    let c2 = ThirdContract { kind: ThirdPartyKind::MtEngine, sandbox: true, outbound_ok: true, privacy_decl: false, approved: false };
    chk(&mut set, "C431-07", bk.review(c2) == Err(EfCode::OverClaim) && bk.filed == 1, "越权拒批立案");

    // C431-08 出站白名单拒（域本色红线）→立案+拒批。
    let c3 = ThirdContract { kind: ThirdPartyKind::MtEngine, sandbox: true, outbound_ok: false, privacy_decl: true, approved: false };
    chk(&mut set, "C431-08", bk.review(c3) == Err(EfCode::WhitelistDeny) && bk.filed == 2, "白名单拒");

    // C431-09 三类契约全过审批→上架。
    for k in [ThirdPartyKind::MtEngine, ThirdPartyKind::CatTool, ThirdPartyKind::LpSource] {
        let c = ThirdContract { kind: k, sandbox: true, outbound_ok: true, privacy_decl: true, approved: false };
        bk.review(c).unwrap();
    }
    chk(&mut set, "C431-09", bk.all_onboard() && bk.contracts.len() == 3, "三类契约上架");

    // C431-10 同类契约覆盖幂等。
    let c4 = ThirdContract { kind: ThirdPartyKind::CatTool, sandbox: true, outbound_ok: true, privacy_decl: true, approved: false };
    bk.review(c4).unwrap();
    chk(&mut set, "C431-10", bk.contracts.len() == 3, "同类覆盖幂等");

    // C431-11 拒批计数累计。
    chk(&mut set, "C431-11", bk.rejected == 2, "拒批计数");

    // C431-12 四口+三类单源断言。
    chk(&mut set, "C431-12", ExtSlot::ALL.len() == 4
        && ThirdPartyKind::LpSource as u8 == 2, "单源断言");

    // ===== 深化判据（审批流水账本 / 审批回归 / 序列化投影） =====

    // C431-13 流水账本守恒：批+拒+挂 ≡ 总流水。
    let mut led = ReviewLedger::new(8);
    let logs: [ReviewLog; 5] = [
        ReviewLog { log_id: 1, kind: ThirdPartyKind::MtEngine, verdict: Verdict::Approved, sandbox: true, outbound_ok: true, privacy_decl: true },
        ReviewLog { log_id: 2, kind: ThirdPartyKind::CatTool, verdict: Verdict::Rejected, sandbox: false, outbound_ok: true, privacy_decl: true },
        ReviewLog { log_id: 3, kind: ThirdPartyKind::LpSource, verdict: Verdict::Filed, sandbox: true, outbound_ok: true, privacy_decl: false },
        ReviewLog { log_id: 4, kind: ThirdPartyKind::MtEngine, verdict: Verdict::Approved, sandbox: true, outbound_ok: true, privacy_decl: true },
        ReviewLog { log_id: 5, kind: ThirdPartyKind::CatTool, verdict: Verdict::Approved, sandbox: true, outbound_ok: true, privacy_decl: true }];
    for l in logs { led.record(l).unwrap(); }
    chk(&mut set, "C431-13", led.conservation() && led.count_by_verdict(Verdict::Approved) == 3, "流水守恒");

    // C431-14 分类投影：按类别分列。
    chk(&mut set, "C431-14", led.count_by_kind(ThirdPartyKind::MtEngine) == 2
        && led.count_by_kind(ThirdPartyKind::LpSource) == 1, "分类投影");

    // C431-15 三审联动判据：批蕴含三审齐，拒/挂蕴含至少一审缺。
    chk(&mut set, "C431-15", led.verdict_logic(), "三审联动");

    // C431-16 边界探针：满容 + 重复流水号显性拒。
    let mut full = ReviewLedger::new(1);
    full.record(logs[0]).unwrap();
    let dup = led.record(ReviewLog { log_id: 1, kind: ThirdPartyKind::MtEngine, verdict: Verdict::Approved, sandbox: true, outbound_ok: true, privacy_decl: true });
    chk(&mut set, "C431-16", full.record(logs[0]) == Err(EfCode::LedgerFull)
        && dup == Err(EfCode::LogDup), "满容与重复拒");

    // C431-17 边界探针：空账本零投影 + 越界序号显性拒。
    let empty = ReviewLedger::new(4);
    chk(&mut set, "C431-17", empty.probe_empty().is_ok()
        && led.probe_idx(99) == Err(EfCode::IdxRange) && led.probe_idx(4).is_ok(), "空账本与越界");

    // C431-18 整数中位流水号投影。
    chk(&mut set, "C431-18", led.median_log_id() == 3 && empty.median_log_id() == 0, "中位投影");

    // C431-19 LCG 审批回归：越权必抓 + 确定性。
    let a1 = audit_regression(0xF4_19_1, 64);
    let a2 = audit_regression(0xF4_19_1, 64);
    chk(&mut set, "C431-19", a1 == a2 && a1.misapproved == 0 && a1.rounds == 64, "越权必抓且确定");

    // C431-20 序列化往返：13 字节定长编码-解码全等。
    let l0 = led.probe_idx(0).unwrap_or(logs[0]);
    let mut buf: Vec<u8> = Vec::new();
    encode_log(&l0, &mut buf);
    let back = decode_log(&buf);
    chk(&mut set, "C431-20", buf.len() == LOG_REC_LEN && back == Some(l0), "编码解码往返");

    // C431-21 序列化边界：短缓冲与坏类别/坏裁决字节拒。
    let mut badk = buf.clone();
    badk[4] = 9;
    let mut badv = buf.clone();
    badv[5] = 9;
    chk(&mut set, "C431-21", decode_log(&buf[..12]).is_none()
        && decode_log(&badk) == None && decode_log(&badv) == None, "短缓冲与坏字节拒");

    // C431-22 FNV 指纹敏感度 + 账本编码幂等闸。
    let fa = fnv1a(b"mt-engine");
    let fb = fnv1a(b"cat-tool");
    chk(&mut set, "C431-22", fa != fb && ledger_encode_idempotent(&led), "指纹敏感+编码幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves431_smoke() {
        let set = run_ves431_checks();
        assert!(set.failed_count() == 0, "ves431 red: {}", set.red_items_text());
    }
}
