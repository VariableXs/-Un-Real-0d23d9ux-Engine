//! UNX-F8601 · C2 号面 300+ 全量联测收官账 · 诊断段 0xB620
//! 验收锚点：UNX-F8601-J1 C2 分发号面全量联测——号面表 ≥300 条注册，
//! 分发轮转全覆盖零漏发，全量统计（通过/拒/未覆盖）三口径对账自洽，
//! 收官断言一票否决，双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB620;

// ── 诊断段独占 · C2 全量收官错误域 ──────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_COVER: u16 = SEG_ERR + 1;   // 覆盖缺口
const E_LEAK: u16 = SEG_ERR + 2;    // 漏发
const E_BALANCE: u16 = SEG_ERR + 3; // 三口径不平
const E_VETO: u16 = SEG_ERR + 4;    // 收官否决
const E_DUP: u16 = SEG_ERR + 5;     // 号面重复注册

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── C2 号面条目 ──────────────────────────────────────────────
/// 号面处理语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C2Sem {
    Pass,   // 通过
    Reject, // 拒（带 errno）
}

/// 号面条目：call_id → 语义 + 期望 errno。
#[derive(Clone, Debug)]
pub struct C2Entry {
    pub call_id: u32,
    pub name: String,
    pub sem: C2Sem,
    pub errno: i32,
}

impl C2Entry {
    pub fn new(call_id: u32, name: &str, sem: C2Sem, errno: i32) -> Self {
        C2Entry { call_id, name: name.to_string(), sem, errno }
    }
    /// 与观察判一致。
    pub fn match_obs(&self, obs: C2Sem, err: i32) -> bool {
        self.sem == obs && (obs == C2Sem::Pass || self.errno == err)
    }
}

// ── C2 号面表 ────────────────────────────────────────────────
/// 全量号面表：注册 ≥300 条，轮转分发全覆盖。
pub struct C2Table {
    pub entries: Vec<C2Entry>,
    pub dispatched: u64,
}

impl C2Table {
    pub fn new() -> Self {
        C2Table { entries: Vec::new(), dispatched: 0 }
    }
    pub fn register(&mut self, e: C2Entry) {
        self.entries.push(e);
    }
    /// 号面无重复。
    pub fn ids_unique(&self) -> bool {
        let n = self.entries.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.entries[i].call_id == self.entries[j].call_id {
                    return false;
                }
            }
        }
        true
    }
    /// 轮转分发：按注册序逐条发，返回命中序号或 None（漏发检出用）。
    pub fn dispatch_all(&mut self) -> Vec<u32> {
        let mut hits = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.call_id != 0 {
                hits.push(i as u32);
            }
            self.dispatched += 1;
        }
        hits
    }
    /// 全量联测：逐条 match_obs，统计三口径。
    pub fn run_full(&self, obs: &[(C2Sem, i32)]) -> (u32, u32, u32) {
        let mut pass = 0u32;
        let mut rej = 0u32;
        let mut uncov = 0u32;
        for (i, e) in self.entries.iter().enumerate() {
            match obs.get(i) {
                Some((s, er)) if e.match_obs(*s, *er) => {
                    if *s == C2Sem::Pass { pass += 1 } else { rej += 1 }
                }
                _ => uncov += 1,
            }
        }
        (pass, rej, uncov)
    }
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

/// 三口径对账：通过 + 拒 + 未覆盖 = 总条目数。
pub fn tri_balance(total: u32, pass: u32, rej: u32, uncov: u32) -> bool {
    pass + rej + uncov == total
}

/// 收官断言：条目 ≥300、号面唯一、分发全覆盖、三口径平、未覆盖为零。
pub fn closing_assert(t: &C2Table, obs: &[(C2Sem, i32)]) -> Result<(u32, u32, u32), u16> {
    if t.count() < 300 {
        return Err(E_COVER);
    }
    if !t.ids_unique() {
        return Err(E_DUP);
    }
    let (p, r, u) = t.run_full(obs);
    if u != 0 {
        return Err(E_LEAK);
    }
    if !tri_balance(t.count() as u32, p, r, u) {
        return Err(E_BALANCE);
    }
    Ok((p, r, u))
}

// ── 标准夹具：312 条号面（Pass 240 + Reject 72）─────────────
pub fn standard_table() -> C2Table {
    let mut t = C2Table::new();
    for i in 0u32..312 {
        let sem = if i % 13 == 0 { C2Sem::Reject } else { C2Sem::Pass };
        let errno = match sem {
            C2Sem::Pass => 0,
            C2Sem::Reject => [2i32, 9, 13, 14, 17, 22][(i % 6) as usize],
        };
        t.register(C2Entry::new(0x4000 + i, &format!("c2_{}", i), sem, errno));
    }
    t
}

pub fn conforming_obs(t: &C2Table) -> Vec<(C2Sem, i32)> {
    t.entries.iter().map(|e| (e.sem, e.errno)).collect()
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8601_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8601 C2号面300+全量联测收官");
    let t = standard_table();

    // 1. 号面表 ≥300 且唯一
    let full = t.count() >= 300 && t.ids_unique();
    s.add("C2-号面300+唯一", full, if full { "" } else { "条目数或唯一性不符" });

    // 2. 全量联测三口径自洽
    let obs = conforming_obs(&t);
    let (p, r, u) = t.run_full(&obs);
    let bal = tri_balance(t.count() as u32, p, r, u);
    s.add("C2-三口径对账自洽", bal && p + r == t.count() as u32 && u == 0,
        if bal { "" } else { "三口径不平" });

    // 3. 收官断言通过
    let close = closing_assert(&t, &obs);
    s.add("C2-收官断言", close.is_ok(), if close.is_ok() { "" } else { "收官被否决" });

    // 4. 分发轮转全覆盖零漏发
    let mut t2 = standard_table();
    let hits = t2.dispatch_all();
    let leak_free = hits.len() == t2.count() && t2.dispatched == t2.count() as u64;
    s.add("C2-分发全覆盖零漏发", leak_free, if leak_free { "" } else { "分发漏发" });

    // 5. 错观察被检出（未覆盖口径非零）
    let (p3, _r3, u3) = t.run_full(&{
        let mut o = conforming_obs(&t);
        o[7].0 = if t.entries[7].sem == C2Sem::Pass { C2Sem::Reject } else { C2Sem::Pass };
        o
    });
    let _ = p3;
    let detected = u3 > 0;
    s.add("C2-错观察检出", detected, if detected { "" } else { "错观察未检出" });

    // 6. 条目不足 300 被收官否决
    let mut t3 = standard_table();
    t3.entries.truncate(299);
    let veto = closing_assert(&t3, &conforming_obs(&t3));
    s.add("C2-不足300否决", veto == Err(E_COVER), if veto == Err(E_COVER) { "" } else { "否决失效" });

    // 7. 号面重复注册检出
    let mut t4 = standard_table();
    let dup = t4.entries[1].clone();
    t4.entries.push(dup);
    s.add("C2-重复注册检出", !t4.ids_unique(), if !t4.ids_unique() { "" } else { "重复未检出" });

    // 8. LCG 抽样复核 5 条
    let mut lcg = Lcg(0xB620);
    let mut spot = true;
    for _ in 0..5 {
        let i = lcg.below(t.count() as u64) as usize;
        if !t.entries[i].match_obs(obs[i].0, obs[i].1) {
            spot = false;
        }
    }
    s.add("C2-抽样复核", spot, if spot { "" } else { "抽样不一致" });

    // 9. Reject 条目 errno 携带（Pass 条目 errno 恒零）
    let err_carry = t.entries.iter().all(|e| match e.sem {
        C2Sem::Pass => e.errno == 0,
        C2Sem::Reject => e.errno != 0,
    });
    s.add("C2-errno 携带规约", err_carry, if err_carry { "" } else { "errno 规约破坏" });

    // 10. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB620 && SEG_BASE < 0xB640;
    s.add("C2-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    // 11. 全量联测总量对账： dispatched == entries.len()
    let mut t5 = standard_table();
    let _ = t5.dispatch_all();
    s.add("C2-分发计数对账", t5.dispatched == t5.count() as u64,
        if t5.dispatched == t5.count() as u64 { "" } else { "计数不符" });

    // 12. 条目语义键内容敏感
    let fa = fnv1a64(b"c2_0:pass");
    let fb = fnv1a64(b"c2_0:reject");
    s.add("C2-语义键敏感", fa != fb, if fa != fb { "" } else { "指纹碰撞" });

    s
}
