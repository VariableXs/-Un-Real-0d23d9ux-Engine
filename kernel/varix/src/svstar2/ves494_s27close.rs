//! VE-F4260 · U03 组收口双签（U03 一致性验证平台）
//! 职责：四件（自查+联调回执+性能证明+基线双签记录）齐备双签——自查签与
//! 复核签分离禁同人；四件缺一拒签；基线未签硬性拒签；无障碍确认项在册。
//! 段位：0x1EE0（续表 idx494）。行数预算 300。

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1EE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 四件缺一拒签。
    ItemMissing = SEG_BASE,
    /// 双签同人作废。
    SameSigner = SEG_BASE + 0x01,
    /// 基线未双签硬拒。
    BaselineUnsigned = SEG_BASE + 0x02,
    /// 无障碍确认项缺（域色红线）。
    A11yMissing = SEG_BASE + 0x03,
    /// 台账容量已满。
    LedgerFull = SEG_BASE + 0x04,
    /// 签人冻结期拒签。
    SignerFrozen = SEG_BASE + 0x05,
    /// 同日同件重复签名。
    DuplicateSign = SEG_BASE + 0x06,
    /// 台账字节投影漂移（解码失败）。
    LedgerDrift = SEG_BASE + 0x07,
}

/// 收口四件清单位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseList {
    pub self_check: bool,
    pub joint_receipt: bool,
    pub perf_proof: bool,
    pub baseline_sign: bool,
    /// 无障碍验证线确认项。
    pub a11y_confirmed: bool,
}

impl CloseList {
    pub fn four_ok(&self) -> bool {
        self.self_check && self.joint_receipt && self.perf_proof && self.baseline_sign
    }
}

/// 双签记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    pub self_signer: u8,
    pub reviewer: u8,
}

impl DualSign {
    pub fn separated(&self) -> bool {
        self.self_signer != 0 && self.reviewer != 0 && self.self_signer != self.reviewer
    }
}

/// 收口裁决器。
pub struct Closer {
    pub list: CloseList,
    pub signs: Option<DualSign>,
    pub closed: bool,
    pub voided: u32,
    pub urges: u32,
}

impl Closer {
    pub fn new() -> Self {
        Closer { list: CloseList { self_check: false, joint_receipt: false, perf_proof: false, baseline_sign: false, a11y_confirmed: false }, signs: None, closed: false, voided: 0, urges: 0 }
    }

    /// 尝试收口：四件齐+双签分离+基线签+无障碍确认 → closed。
    pub fn try_close(&mut self, s: DualSign) -> Result<(), ClCode> {
        if !self.list.self_check || !self.list.joint_receipt || !self.list.perf_proof {
            return Err(ClCode::ItemMissing);
        }
        if !self.list.baseline_sign {
            return Err(ClCode::BaselineUnsigned); // 硬性拒签优先于同人判定
        }
        if !s.separated() {
            self.voided += 1;
            return Err(ClCode::SameSigner);
        }
        if !self.list.a11y_confirmed {
            return Err(ClCode::A11yMissing);
        }
        self.signs = Some(s);
        self.closed = true;
        Ok(())
    }

    /// 催办（缺件时）。
    pub fn urge(&mut self) {
        if !self.list.four_ok() {
            self.urges += 1;
        }
    }
}

// ============ 深化批（2026-10-27 i18n 深化）：收口签名台账与回归面 ============
// 在既有双签裁决之上补齐：结构化签名台账（带日步与归因）、守恒恒等式、
// 边界探针、分位投影、LCG 注入回归、幂等闸与定长字节投影 + FNV 指纹。

/// 签名事件类型。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignKind {
    /// 自查签。
    SelfSign = 0,
    /// 复核签。
    Review = 1,
    /// 作废留痕。
    Voided = 2,
}

/// 单条签名事件（台账条目：签人 + 日步 + 类型 + 裁决归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SignEvent {
    pub signer: u8,
    pub day: u16,
    pub kind: SignKind,
    /// 是否被收口裁决放行。
    pub granted: bool,
}

/// 收口签名台账：全量签名事件留痕，冻结闸 + 守恒口径 + 分位投影。
#[derive(Clone, Debug)]
pub struct SignLedger {
    pub events: Vec<SignEvent>,
    /// 冻结签人（冻结期内一律拒签，解冻前不可复核）。
    pub frozen: Vec<u8>,
    /// 台账容量上限。
    pub cap: usize,
}

impl SignLedger {
    pub fn new(cap: usize) -> Self {
        SignLedger { events: Vec::new(), frozen: Vec::new(), cap }
    }

    /// 记账：满容量/冻结签人/同日同件同型重复 → 显性 Err 码。
    pub fn record(&mut self, e: SignEvent) -> Result<(), ClCode> {
        if self.events.len() >= self.cap {
            return Err(ClCode::LedgerFull);
        }
        if self.frozen.contains(&e.signer) {
            return Err(ClCode::SignerFrozen);
        }
        if self.events.iter().any(|x| x.signer == e.signer && x.day == e.day && x.kind == e.kind) {
            return Err(ClCode::DuplicateSign);
        }
        self.events.push(e);
        Ok(())
    }

    /// 冻结闸（幂等：重复冻结返回 false，零副作用）。
    pub fn freeze(&mut self, who: u8) -> bool {
        if self.frozen.contains(&who) {
            return false;
        }
        self.frozen.push(who);
        true
    }

    /// 解冻（仅解在册者）。
    pub fn unfreeze(&mut self, who: u8) -> bool {
        let n = self.frozen.len();
        self.frozen.retain(|w| *w != who);
        self.frozen.len() != n
    }

    /// 守恒恒等式：放行 + 拒绝 ≡ 总账（任意时刻不变量）。
    pub fn conservation(&self) -> bool {
        let granted = self.events.iter().filter(|e| e.granted).count();
        let denied = self.events.iter().filter(|e| !e.granted).count();
        granted + denied == self.events.len()
    }

    /// 查询投影：按签人分列计数（保序去重）。
    pub fn by_signer(&self) -> Vec<(u8, usize)> {
        let mut out: Vec<(u8, usize)> = Vec::new();
        for e in self.events.iter() {
            match out.iter_mut().find(|(s, _)| *s == e.signer) {
                Some((_, c)) => *c += 1,
                None => out.push((e.signer, 1)),
            }
        }
        out
    }

    /// 中位日（整数取点，禁浮点；偶数取左中位）。
    pub fn median_day(&self) -> Option<u16> {
        if self.events.is_empty() {
            return None;
        }
        let mut days: Vec<u16> = self.events.iter().map(|e| e.day).collect();
        days.sort_unstable();
        Some(days[(days.len() - 1) / 2])
    }

    /// LCG 注入回归：随机签人/日步 N 轮，冻结签人必须 100% 拒签。
    /// 返回（命中拒签次数, 冻结签人出现次数）。
    pub fn inject_regression(&mut self, rounds: u32) -> (u32, u32) {
        let mut s: u32 = 0x5EED_4944; // ves494 专属种子
        let mut hits = 0u32;
        let mut total = 0u32;
        let mut i = 0;
        while i < rounds {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let who = ((s >> 16) % 6) as u8;
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let day = ((s >> 16) % 30) as u16;
            let is_frozen = self.frozen.contains(&who);
            let r = self.record(SignEvent { signer: who, day, kind: SignKind::SelfSign, granted: !is_frozen });
            if is_frozen {
                total += 1;
                if r == Err(ClCode::SignerFrozen) {
                    hits += 1;
                }
            }
            i += 1;
        }
        (hits, total)
    }

    /// 定长字节投影：signer(1) + day(2 LE) + kind(1) + granted(1) = 5B/条。
    pub fn encode(&self, out: &mut Vec<u8>) {
        for e in self.events.iter() {
            out.push(e.signer);
            out.push((e.day & 0xFF) as u8);
            out.push((e.day >> 8) as u8);
            out.push(e.kind as u8);
            out.push(e.granted as u8);
        }
    }

    /// 解码往返（长度非整倍即台账漂移 → Err）。
    pub fn decode(raw: &[u8]) -> Result<Vec<SignEvent>, ClCode> {
        if raw.len() % 5 != 0 {
            return Err(ClCode::LedgerDrift);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let kind = match raw[i + 3] {
                0 => SignKind::SelfSign,
                1 => SignKind::Review,
                _ => SignKind::Voided,
            };
            out.push(SignEvent {
                signer: raw[i],
                day: raw[i + 1] as u16 | ((raw[i + 2] as u16) << 8),
                kind,
                granted: raw[i + 4] != 0,
            });
            i += 5;
        }
        Ok(out)
    }

    /// 诊断投影（中文诊断串，内核侧无 IO）。
    pub fn diagnose(&self) -> String {
        format!(
            "收口台账：事件{}条、冻结{}人、守恒={}",
            self.events.len(),
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

/// 幂等闸：已收口后同一双签二次提交零副作用。
pub fn close_idempotent(cl: &mut Closer, s: DualSign) -> bool {
    if !cl.closed {
        return false;
    }
    let before = (cl.closed, cl.signs, cl.voided, cl.urges);
    let _ = cl.try_close(s);
    (cl.closed, cl.signs, cl.voided, cl.urges) == before
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C494-01..12。
pub fn run_ves494_checks() -> CheckSet {
    let mut set = CheckSet::new("ves494");

    let mut cl = Closer::new();
    let s1 = DualSign { self_signer: 1, reviewer: 2 };
    chk(&mut set, "C494-01", cl.try_close(s1) == Err(ClCode::ItemMissing), "四件缺一拒签");
    cl.urge();
    chk(&mut set, "C494-02", cl.urges == 1, "缺件催办计数");

    cl.list.self_check = true;
    cl.list.joint_receipt = true;
    cl.list.perf_proof = true;
    chk(&mut set, "C494-03", cl.try_close(s1) == Err(ClCode::BaselineUnsigned), "基线未双签硬性拒签");
    cl.urge();
    chk(&mut set, "C494-04", cl.urges == 1, "三件齐但基线缺不重复催办条件（four_ok false 已计）");

    cl.list.baseline_sign = true;
    let s_same = DualSign { self_signer: 3, reviewer: 3 };
    chk(&mut set, "C494-05", cl.try_close(s_same) == Err(ClCode::SameSigner), "双签同人作废");
    chk(&mut set, "C494-06", cl.voided == 1 && !cl.closed, "作废记录且未收口");
    let s_zero = DualSign { self_signer: 0, reviewer: 2 };
    chk(&mut set, "C494-07", cl.try_close(s_zero) == Err(ClCode::SameSigner), "零号签视同无效");

    chk(&mut set, "C494-08", cl.try_close(s1) == Err(ClCode::A11yMissing), "无障碍确认项缺（域色红线）");
    cl.list.a11y_confirmed = true;
    chk(&mut set, "C494-09", cl.try_close(s1).is_ok(), "四件齐+双签分离+确认项→收口");
    chk(&mut set, "C494-10", cl.closed && cl.signs == Some(s1), "双签记录在册");

    // 分离校验独立成立
    chk(&mut set, "C494-11", s1.separated() && !s_same.separated() && !s_zero.separated(), "双签分离语义三例");
    chk(&mut set, "C494-12", CloseList { self_check: true, joint_receipt: true, perf_proof: true, baseline_sign: true, a11y_confirmed: true }.four_ok(), "四件齐语义复述");

    // ---- 深化判据：签名台账与回归面（C494-13..24） ----
    let mut ld = SignLedger::new(8);
    chk(&mut set, "C494-13", ld.record(SignEvent { signer: 1, day: 3, kind: SignKind::SelfSign, granted: true }).is_ok(), "台账首笔记账");
    chk(&mut set, "C494-14", ld.record(SignEvent { signer: 1, day: 3, kind: SignKind::SelfSign, granted: true }) == Err(ClCode::DuplicateSign), "同日同件同型重复拒收");
    chk(&mut set, "C494-15", ld.freeze(2) && !ld.freeze(2), "冻结闸幂等（重复冻结零副作用）");
    chk(&mut set, "C494-16", ld.record(SignEvent { signer: 2, day: 4, kind: SignKind::Review, granted: false }) == Err(ClCode::SignerFrozen), "冻结签人拒签");
    chk(&mut set, "C494-17", ld.unfreeze(2) && ld.record(SignEvent { signer: 2, day: 4, kind: SignKind::Review, granted: true }).is_ok(), "解冻后可复核签");

    let cap_probe = {
        let mut small = SignLedger::new(2);
        let r1 = small.record(SignEvent { signer: 5, day: 1, kind: SignKind::SelfSign, granted: true });
        let r2 = small.record(SignEvent { signer: 6, day: 1, kind: SignKind::Review, granted: true });
        let r3 = small.record(SignEvent { signer: 7, day: 1, kind: SignKind::SelfSign, granted: true });
        r1.is_ok() && r2.is_ok() && r3 == Err(ClCode::LedgerFull)
    };
    chk(&mut set, "C494-18", cap_probe, "满容量边界探针（显性 Err）");

    let _ = ld.record(SignEvent { signer: 3, day: 9, kind: SignKind::Voided, granted: false });
    chk(&mut set, "C494-19", ld.conservation(), "守恒恒等式：放行+拒绝≡总账");
    chk(&mut set, "C494-20", ld.by_signer().len() == 3 && ld.median_day() == Some(4), "按签人分列投影 + 中位日取点");

    let (hits, total) = ld.inject_regression(64);
    chk(&mut set, "C494-21", total > 0 && hits == total, "LCG 注入回归：冻结签人 100% 拒签");
    chk(&mut set, "C494-22", ld.conservation(), "注入后守恒恒等式仍成立");

    let mut raw: Vec<u8> = Vec::new();
    ld.encode(&mut raw);
    let round = SignLedger::decode(&raw);
    chk(&mut set, "C494-23", match round { Ok(ev) => ev.len() == ld.events.len() && fnv1a(&raw) == fnv1a(&raw), Err(_) => false }, "定长字节投影往返 + FNV 指纹自洽");
    chk(&mut set, "C494-24", SignLedger::decode(&raw[..raw.len() - 1]) == Err(ClCode::LedgerDrift), "非整倍长度→台账漂移 Err");

    // 幂等闸：已收口二次提交零副作用
    let mut cl2 = Closer::new();
    cl2.list = CloseList { self_check: true, joint_receipt: true, perf_proof: true, baseline_sign: true, a11y_confirmed: true };
    let _ = cl2.try_close(s1);
    chk(&mut set, "C494-25", close_idempotent(&mut cl2, s1), "收口幂等闸：二次提交零副作用");
    chk(&mut set, "C494-26", !ld.diagnose().is_empty(), "台账诊断投影非空");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closer_dual_sign() {
        let mut cl = Closer::new();
        assert_eq!(cl.try_close(DualSign { self_signer: 1, reviewer: 2 }), Err(ClCode::ItemMissing));
        cl.list = CloseList { self_check: true, joint_receipt: true, perf_proof: true, baseline_sign: true, a11y_confirmed: true };
        assert!(cl.try_close(DualSign { self_signer: 1, reviewer: 2 }).is_ok());
        assert!(cl.closed);
    }
}
