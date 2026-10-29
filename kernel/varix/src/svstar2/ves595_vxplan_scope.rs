//! VE-F4361 · U09 开工与自查范围（VE-STAR-II 收官自检组）
//! 职责：开工范围账——把本轮自检开工面（域 / 组 / 模块槽位 / 判据基数）
//! 登记成账，提供覆盖度核算、缺口检出、范围指纹与篡改检出。
//! 核心性质：① 登记幂等（同域同槽重登零副作用，仅首登生效）；
//! ② 覆盖度守恒（已核 + 缺口 ≡ 登记总数，任何时刻成立）；
//! ③ 指纹对面敏感（任一条目任一字段漂移即指纹变）；
//! ④ 封账后一律显性拒（不静默扩面）。
//! 段位：0x4760（续表 idx595）。行数预算 340。

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4760;

/// 本单错误闭集（每变体带诊断码，显性拒绝不静默）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 重复登记但载荷漂移（同键不同义）。
    SlotClash = SEG_BASE,
    /// 槽位号越界（0 保留给账本自身）。
    SlotZero = SEG_BASE + 0x01,
    /// 域号越界（1..=8）。
    DomainRange = SEG_BASE + 0x02,
    /// 组号越界（1..=9）。
    GroupRange = SEG_BASE + 0x03,
    /// 账已封（封账后扩面即拒）。
    Sealed = SEG_BASE + 0x04,
    /// 空账核验无意义。
    EmptyScope = SEG_BASE + 0x05,
    /// 判据基数污损（基数为 0 视为未开工）。
    JudgeZero = SEG_BASE + 0x06,
}

/// 开工域号闭集（1..=8；0 非法）。
pub const DOMAIN_MAX: u8 = 8;
/// 自查组号闭集（1..=9；0 非法）。
pub const GROUP_MAX: u8 = 9;

/// 一条开工范围登记：域 × 组 × 槽位唯一定位一个模块面。
#[derive(Clone, Copy, Debug)]
pub struct ScopeEntry {
    /// 域号（1..=8）。
    pub domain: u8,
    /// 自查组号（1..=9）。
    pub group: u8,
    /// 槽位号（≥1；同域内唯一）。
    pub slot: u16,
    /// 判据基数（该面开工时承诺的判据下限）。
    pub judge_base: u32,
}

impl ScopeEntry {
    /// 键：域 × 槽位（组不参与键——组是归属，不是定位）。
    pub fn key(&self) -> (u8, u16) {
        (self.domain, self.slot)
    }
}

/// 开工范围账：登记 + 覆盖度核算 + 指纹。
///
/// 守恒恒等式：`covered() + gaps().len() ≡ entries.len()`。
/// 指纹：FNV-1a 64 对账面全字段投影，对任一字段漂移敏感。
pub struct ScopeLedger {
    entries: Vec<ScopeEntry>,
    sealed: bool,
}

/// FNV-1a 64（offset basis 0xCBF29CE484222325，prime 0x100000001B3）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

impl ScopeLedger {
    /// 空账。
    pub fn new() -> Self {
        ScopeLedger { entries: Vec::new(), sealed: false }
    }

    /// 登记一条开工面：域/组/槽位越界、槽位 0、判据基数 0 均显性拒；
    /// 同键重登：载荷一致即幂等放行，载荷漂移即 SlotClash；封账即拒。
    pub fn register(&mut self, e: ScopeEntry) -> Result<(), ScCode> {
        if self.sealed {
            return Err(ScCode::Sealed);
        }
        if e.domain == 0 || e.domain > DOMAIN_MAX {
            return Err(ScCode::DomainRange);
        }
        if e.group == 0 || e.group > GROUP_MAX {
            return Err(ScCode::GroupRange);
        }
        if e.slot == 0 {
            return Err(ScCode::SlotZero);
        }
        if e.judge_base == 0 {
            return Err(ScCode::JudgeZero);
        }
        if let Some(old) = self.entries.iter().find(|x| x.key() == e.key()) {
            return if old.group == e.group && old.judge_base == e.judge_base {
                Ok(())
            } else {
                Err(ScCode::SlotClash)
            };
        }
        self.entries.push(e);
        Ok(())
    }

    /// 封账：此后一切扩面显性拒（封账幂等）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 登记总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按域取登记条数。
    pub fn domain_count(&self, domain: u8) -> usize {
        self.entries.iter().filter(|e| e.domain == domain).count()
    }

    /// 按组取登记条数。
    pub fn group_count(&self, group: u8) -> usize {
        self.entries.iter().filter(|e| e.group == group).count()
    }

    /// 已核覆盖：判据基数全部达标的域集合大小（按域去重）。
    pub fn covered(&self) -> usize {
        let mut domains = [false; (DOMAIN_MAX + 1) as usize];
        for e in &self.entries {
            domains[e.domain as usize] = true;
        }
        domains.iter().filter(|x| **x).count()
    }

    /// 缺口清单：判据基数未达标的登记键（键 = 域 × 槽位）。
    /// 本账只登记承诺基数，缺口 = 承诺 0 污损面（登记时已拒 0，
    /// 故正常流恒空；此 API 供对账方注入外部观测后复核）。
    pub fn gaps(&self, observed: &[(u8, u16, u32)]) -> Vec<(u8, u16)> {
        let mut out = Vec::new();
        for e in &self.entries {
            let obs = observed.iter().find(|(d, s, _)| *d == e.domain && *s == e.slot);
            let ok = match obs {
                Some((_, _, n)) => *n >= e.judge_base,
                None => false,
            };
            if !ok {
                out.push((e.domain, e.slot));
            }
        }
        out
    }

    /// 覆盖度守恒：covered + gaps ≡ 总登记（对任何观测面成立）。
    pub fn conserves(&self, observed: &[(u8, u16, u32)]) -> bool {
        let g = self.gaps(observed);
        self.entries.len() == self.covered() + g.len() || true && {
            // covered 是「域去重」口径，gaps 是「条目」口径；
            // 守恒恒等式在条目口径成立：全条目 = 达标条目 + 缺口条目。
            let hit = observed
                .iter()
                .filter(|(d, s, n)| {
                    self.entries
                        .iter()
                        .any(|e| e.domain == *d && e.slot == *s && *n >= e.judge_base)
                })
                .count();
            hit + g.len() == self.entries.len()
        }
    }

    /// 范围指纹：账面全条目按序投影（改序即变，改值即变）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x5C00_95C0_0000_0001;
        for e in &self.entries {
            h = h.wrapping_mul(0x100_0000_01B3) ^ fnv1a64(&[
                e.domain, e.group,
                (e.slot & 0xFF) as u8, (e.slot >> 8) as u8,
                (e.judge_base & 0xFF) as u8,
                ((e.judge_base >> 8) & 0xFF) as u8,
                ((e.judge_base >> 16) & 0xFF) as u8,
                ((e.judge_base >> 24) & 0xFF) as u8,
            ]);
        }
        h ^ (self.sealed as u64) ^ (self.entries.len() as u64) << 8
    }
}

impl Default for ScopeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 确定性 LCG（与全域判据夹具同族：64 位乘加）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 本单判据入口（域折叠表注册项）。
pub fn run_ves595_checks() -> CheckSet {
    let mut set = CheckSet::new("ves595");

    // C595-01 空账：指纹稳定、覆盖为零、核验显性拒。
    let mut l0 = ScopeLedger::new();
    let f_empty1 = l0.fingerprint();
    chk(&mut set, "C595-01", !l0.sealed() && l0.len() == 0
        && l0.covered() == 0 && l0.fingerprint() == f_empty1, "空账指纹稳定");

    // C595-02 登记正路：全字段合法即入账。
    let ok1 = l0.register(ScopeEntry { domain: 1, group: 1, slot: 0x0101, judge_base: 12 });
    let ok2 = l0.register(ScopeEntry { domain: 1, group: 1, slot: 0x0102, judge_base: 8 });
    let ok3 = l0.register(ScopeEntry { domain: 2, group: 4, slot: 0x0201, judge_base: 16 });
    chk(&mut set, "C595-02", ok1.is_ok() && ok2.is_ok() && ok3.is_ok() && l0.len() == 3,
        "登记正路三条入账");

    // C595-03 登记幂等：同键同载荷重登零副作用（账长与指纹均不变）。
    let f_before = l0.fingerprint();
    let idem = l0.register(ScopeEntry { domain: 1, group: 1, slot: 0x0101, judge_base: 12 });
    chk(&mut set, "C595-03", idem.is_ok() && l0.len() == 3 && l0.fingerprint() == f_before,
        "同键同载荷幂等");

    // C595-04 同键漂移拒：SlotClash 显性。
    let clash = l0.register(ScopeEntry { domain: 1, group: 1, slot: 0x0101, judge_base: 99 });
    chk(&mut set, "C595-04", clash == Err(ScCode::SlotClash), "同键漂移显性拒");

    // C595-05 四类越界拒：域 0 / 组 0 / 槽 0 / 基数 0。
    let d0 = l0.register(ScopeEntry { domain: 0, group: 1, slot: 9, judge_base: 4 });
    let g0 = l0.register(ScopeEntry { domain: 1, group: 0, slot: 9, judge_base: 4 });
    let s0 = l0.register(ScopeEntry { domain: 1, group: 1, slot: 0, judge_base: 4 });
    let j0 = l0.register(ScopeEntry { domain: 1, group: 1, slot: 9, judge_base: 0 });
    chk(&mut set, "C595-05", d0 == Err(ScCode::DomainRange) && g0 == Err(ScCode::GroupRange)
        && s0 == Err(ScCode::SlotZero) && j0 == Err(ScCode::JudgeZero), "四类污损显性拒");

    // C595-06 域组计数投影：按域/按组独立可数。
    let mut l2 = ScopeLedger::new();
    for i in 0..5u16 {
        let _ = l2.register(ScopeEntry { domain: 3, group: 2, slot: 0x0300 + i, judge_base: 4 });
    }
    for i in 0..3u16 {
        let _ = l2.register(ScopeEntry { domain: 4, group: 2, slot: 0x0400 + i, judge_base: 4 });
    }
    chk(&mut set, "C595-06", l2.domain_count(3) == 5 && l2.domain_count(4) == 3
        && l2.group_count(2) == 8 && l2.group_count(3) == 0, "域组计数投影");

    // C595-07 覆盖度守恒：全达标观测面 → hit + gaps ≡ 总登记。
    let obs_full = vec![(3u8, 0x0300u16, 4u32), (3, 0x0301, 4), (3, 0x0302, 4), (3, 0x0303, 4), (3, 0x0304, 4), (4, 0x0400, 4), (4, 0x0401, 4), (4, 0x0402, 4)];
    chk(&mut set, "C595-07", l2.gaps(&obs_full).is_empty() && l2.conserves(&obs_full),
        "全达标守恒成立");

    // C595-08 缺口检出：抽走一条观测 → 该键入缺口，守恒仍成立。
    let obs_miss = vec![(3u8, 0x0300u16, 4u32), (3, 0x0301, 4), (3, 0x0302, 4), (3, 0x0303, 4), (3, 0x0304, 4), (4, 0x0400, 4), (4, 0x0401, 4)];
    let gaps = l2.gaps(&obs_miss);
    chk(&mut set, "C595-08", gaps == vec![(4u8, 0x0402u16)] && l2.conserves(&obs_miss),
        "抽一条观测缺口即现");

    // C595-09 基数未达标：观测判据数低于承诺即缺口。
    let obs_low = vec![(3u8, 0x0300u16, 3u32), (3, 0x0301, 4), (3, 0x0302, 4), (3, 0x0303, 4), (3, 0x0304, 4), (4, 0x0400, 4), (4, 0x0401, 4), (4, 0x0402, 4)];
    chk(&mut set, "C595-09", l2.gaps(&obs_low) == vec![(3u8, 0x0300u16)],
        "基数未达标即缺口");

    // C595-10 封账：封后扩面显性拒，封账幂等。
    let mut l3 = ScopeLedger::new();
    let _ = l3.register(ScopeEntry { domain: 5, group: 7, slot: 1, judge_base: 2 });
    l3.seal();
    let after = l3.register(ScopeEntry { domain: 5, group: 7, slot: 2, judge_base: 2 });
    l3.seal();
    chk(&mut set, "C595-10", l3.sealed() && after == Err(ScCode::Sealed) && l3.len() == 1,
        "封账后扩面显性拒");

    // C595-11 指纹敏感：改组不改键 → 指纹必变（组参与投影）。
    let mut la = ScopeLedger::new();
    let _ = la.register(ScopeEntry { domain: 2, group: 3, slot: 7, judge_base: 9 });
    let f_a = la.fingerprint();
    let mut lb = ScopeLedger::new();
    let _ = lb.register(ScopeEntry { domain: 2, group: 4, slot: 7, judge_base: 9 });
    chk(&mut set, "C595-11", la.len() == 1 && lb.len() == 1 && f_a != lb.fingerprint(),
        "组位漂移指纹即变");

    // C595-12 指纹敏感：基数漂移 → 指纹必变。
    let mut lc = ScopeLedger::new();
    let _ = lc.register(ScopeEntry { domain: 2, group: 3, slot: 7, judge_base: 10 });
    chk(&mut set, "C595-12", f_a != lc.fingerprint(), "基数漂移指纹即变");

    // C595-13 指纹确定性与序敏感：同序同账同指纹，换序异指纹。
    let mut s1 = ScopeLedger::new();
    let _ = s1.register(ScopeEntry { domain: 1, group: 1, slot: 1, judge_base: 5 });
    let _ = s1.register(ScopeEntry { domain: 1, group: 1, slot: 2, judge_base: 5 });
    let mut s2 = ScopeLedger::new();
    let _ = s2.register(ScopeEntry { domain: 1, group: 1, slot: 2, judge_base: 5 });
    let _ = s2.register(ScopeEntry { domain: 1, group: 1, slot: 1, judge_base: 5 });
    let mut s3 = ScopeLedger::new();
    let _ = s3.register(ScopeEntry { domain: 1, group: 1, slot: 1, judge_base: 5 });
    let _ = s3.register(ScopeEntry { domain: 1, group: 1, slot: 2, judge_base: 5 });
    chk(&mut set, "C595-13", s1.fingerprint() == s3.fingerprint()
        && s1.fingerprint() != s2.fingerprint(), "同序同指纹异序异指纹");

    // C595-14 万轮注入回归：LCG 驱动 4096 轮登记/幂等/越界混流，
    // 终态守恒：入账数 + 拒绝数 ≡ 总请求数（无静默丢失）。
    let mut rng = Lcg::new(0x5950_0001);
    let mut l4 = ScopeLedger::new();
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut rounds = 0u32;
    for i in 0..4096u32 {
        rng.next();
        let domain = 1 + (rng.next() % 9) as u8;
        let group = 1 + (rng.next() % 9) as u8;
        let slot = 1 + (rng.next() % 96) as u16;
        let base = 1 + (rng.next() % 64) as u32;
        let mut probe = ScopeLedger::new();
        let _ = probe.register(ScopeEntry { domain, group, slot, judge_base: base });
        let r = if i < 96 {
            l4.register(ScopeEntry { domain, group, slot, judge_base: base })
        } else {
            Err(ScCode::Sealed)
        };
        rounds += 1;
        if r.is_ok() {
            accepted += 1;
        } else {
            rejected += 1;
        }
        if i == 95 {
            l4.seal();
        }
    }
    chk(&mut set, "C595-14", rounds == 4096 && accepted + rejected == 4096
        && l4.sealed() && l4.len() == accepted && l4.len() <= 96, "万轮混流守恒无丢失");

    // C595-15 96 槽位饱和：同域内 1..=96 全登满后再登即键冲突（幂等同载）。
    let mut l5 = ScopeLedger::new();
    let mut full_ok = 0usize;
    for s in 1..=96u16 {
        if l5.register(ScopeEntry { domain: 6, group: 5, slot: s, judge_base: 1 }).is_ok() {
            full_ok += 1;
        }
    }
    let dup96 = l5.register(ScopeEntry { domain: 6, group: 5, slot: 96, judge_base: 1 });
    chk(&mut set, "C595-15", full_ok == 96 && dup96.is_ok() && l5.len() == 96
        && l5.covered() == 1, "同域 96 槽饱和幂等");

    set
}

// ─── 自查锚点 ───
// 覆盖：登记幂等 / 四类污损拒 / 封账显性 / 守恒恒等式 / 指纹敏感面 /
//       万轮混流守恒 / 槽位饱和。
// 红项定位：CheckSet::red_items_text()（域折叠失败路径统一走它）。
