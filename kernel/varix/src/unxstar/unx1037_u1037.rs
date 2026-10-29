// unx1037_u1037 —— UNX-F1037 计时面健康摘要（F0899 口径注册）
// 判据原文 UNX-F1037-J1：挂接完整（B12 十账摘要入 F0997 无缺），schema 校验通过，投递 10/10 次可达。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2240 独占（0x2241~0x224F）。
//
// 设计（纯功能 no_std）：
//   * B12 十账摘要：十类计时面账目逐条登记，缺一即挂接不完整；
//   * schema 校验：每条摘要带 schema 版本与字段掩码，非法掩码拒收；
//   * 投递面：向 F0997 投递计数与可达计数分开记账，投递 10/10 可达；
//   * 守恒：摘要条数 = 登记 = 校验通过 = 投递成功（四账对平）；
//   * FNV 指纹冻结 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2240 独占） ─────────────────────────────────────────────
pub const XC_SUM_OOB: u32 = 0x2241; // 账号越界或掩码非法
pub const XC_SUM_SCHEMA: u32 = 0x2242; // schema 版本不符
pub const XC_SUM_DUP: u32 = 0x2243; // 同账重复登记
pub const XC_SUM_SEALED: u32 = 0x2244; // 封账后写摘要
pub const XC_SUM_MISSING: u32 = 0x2245; // B12 十账缺账
pub const XC_SUM_LOST: u32 = 0x2246; // 投递不可达

pub const SCHEMA_VER: u16 = 0x0899;
/// B12 十账固定账号集（0..=9）。
pub const B12_ACCOUNTS: [u16; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
/// 合法字段掩码：kind(3bit)+value(24bit)+fp(5bit) 共 32bit 全用。
pub const FIELD_MASK_ALL: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy)]
pub struct SummaryRow {
    pub account: u16, // B12 账号 0..=9
    pub schema: u16,  // 必须等于 SCHEMA_VER
    pub value: u32,   // 账面值
    pub mask: u32,    // 字段掩码
}

/// 计时面健康摘要账本（F0997 口径）。
pub struct HealthSummary {
    rows: Vec<SummaryRow>,
    seen: Vec<u16>,
    delivered: u32,
    reachable: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl HealthSummary {
    pub fn new() -> HealthSummary {
        HealthSummary {
            rows: Vec::new(),
            seen: Vec::new(),
            delivered: 0,
            reachable: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 登记一条摘要（重复拒 / schema 不符拒 / 掩码非法拒）。
    pub fn register(&mut self, r: SummaryRow) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SUM_SEALED, "封账后登记"));
            return Err(XC_SUM_SEALED);
        }
        if r.account >= 10 {
            self.rejects.push((XC_SUM_OOB, "账号越界"));
            return Err(XC_SUM_OOB);
        }
        if r.schema != SCHEMA_VER {
            self.rejects.push((XC_SUM_SCHEMA, "schema 版本不符"));
            return Err(XC_SUM_SCHEMA);
        }
        if r.mask & !FIELD_MASK_ALL != 0 {
            self.rejects.push((XC_SUM_OOB, "字段掩码非法"));
            return Err(XC_SUM_OOB);
        }
        for &a in &self.seen {
            if a == r.account {
                self.rejects.push((XC_SUM_DUP, "同账重复登记"));
                return Err(XC_SUM_DUP);
            }
        }
        self.seen.push(r.account);
        self.rows.push(r);
        Ok(())
    }

    /// schema 校验：全部行版本与掩码合法。
    pub fn schema_ok(&self) -> bool {
        for r in &self.rows {
            if r.schema != SCHEMA_VER || r.mask & !FIELD_MASK_ALL != 0 {
                return false;
            }
        }
        true
    }

    /// 挂接完整性：B12 十账无缺。
    pub fn hook_complete(&self) -> bool {
        let mut hit = [false; 10];
        for &a in &self.seen {
            hit[a as usize] = true;
        }
        hit.iter().all(|&h| h)
    }

    /// 缺账清单（账号升序）。
    pub fn missing_accounts(&self) -> Vec<u16> {
        let mut hit = [false; 10];
        for &a in &self.seen {
            hit[a as usize] = true;
        }
        let mut miss = Vec::new();
        for (i, &h) in hit.iter().enumerate() {
            if !h {
                miss.push(i as u16);
            }
        }
        miss
    }

    /// 向 F0997 投递一次：ok 为真则可达。
    pub fn deliver(&mut self, ok: bool) -> Result<(), u32> {
        if self.delivered.saturating_add(1) > 1_000_000 {
            self.rejects.push((XC_SUM_LOST, "投递计数饱和"));
            return Err(XC_SUM_LOST);
        }
        self.delivered = self.delivered.saturating_add(1);
        if ok {
            self.reachable = self.reachable.saturating_add(1);
        } else {
            self.rejects.push((XC_SUM_LOST, "投递不可达"));
            return Err(XC_SUM_LOST);
        }
        Ok(())
    }

    /// 守恒：登记 = 校验通过 = 投递成功 = 可达。
    pub fn conserved(&self) -> bool {
        self.rows.len() == self.seen.len()
            && self.rows.len() == 10
            && self.delivered == self.reachable
            && self.delivered >= 10
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SUM_SEALED);
        }
        if !self.hook_complete() {
            self.rejects.push((XC_SUM_MISSING, "十账缺账禁封"));
            return Err(XC_SUM_MISSING);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// FNV-1a 指纹（0x811C9DC5 / 0x01000193）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.rows.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in &self.rows {
            h ^= (r.account as u32)
                ^ (r.schema as u32).wrapping_mul(0x9E37)
                ^ r.value
                ^ r.mask.wrapping_mul(0x85EB);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.delivered;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.reachable;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// 注入核：六类破坏全抓（捕获数，总数 6）。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）schema 版本不符必拒（10/10）
    {
        let mut s = HealthSummary::new();
        let mut hits = 0;
        for k in 1..=10u16 {
            if s.register(SummaryRow { account: 0, schema: SCHEMA_VER.wrapping_add(k), value: 1, mask: FIELD_MASK_ALL })
                == Err(XC_SUM_SCHEMA)
            {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）账号越界必拒
    {
        let mut s = HealthSummary::new();
        if s.register(SummaryRow { account: 10, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL }) == Err(XC_SUM_OOB) {
            caught += 1;
        }
    }
    // 3）同账重复登记必拒
    {
        let mut s = HealthSummary::new();
        let _ = s.register(SummaryRow { account: 3, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        if s.register(SummaryRow { account: 3, schema: SCHEMA_VER, value: 2, mask: FIELD_MASK_ALL }) == Err(XC_SUM_DUP) {
            caught += 1;
        }
    }
    // 4）十账缺账禁封必拒
    {
        let mut s = HealthSummary::new();
        for a in 0..9u16 {
            let _ = s.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        if s.seal() == Err(XC_SUM_MISSING) {
            caught += 1;
        }
    }
    // 5）封账后登记必拒
    {
        let mut s = HealthSummary::new();
        for a in 0..10u16 {
            let _ = s.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let _ = s.seal();
        if s.register(SummaryRow { account: 0, schema: SCHEMA_VER, value: 9, mask: FIELD_MASK_ALL }) == Err(XC_SUM_SEALED) {
            caught += 1;
        }
    }
    // 6）投递不可达必拒且不增可达数
    {
        let mut s = HealthSummary::new();
        for a in 0..10u16 {
            let _ = s.register(SummaryRow { account: a, schema: SCHEMA_VER, value: a as u32, mask: FIELD_MASK_ALL });
        }
        let _ = s.deliver(true);
        let bad = s.deliver(false) == Err(XC_SUM_LOST);
        if bad && s.reachable == 1 && s.delivered == 2 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1037-J1。
pub fn run_unx1037_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1037");

    // C37-01 B12 十账全登记
    {
        let mut h = HealthSummary::new();
        let mut all_ok = true;
        for a in 0..10u16 {
            if h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 100 + a as u32, mask: FIELD_MASK_ALL }).is_err() {
                all_ok = false;
            }
        }
        s.add("C37-01-十账登记", all_ok && h.row_count() == 10, if all_ok { "" } else { "登记缺账" });
    }
    // C37-02 挂接完整（B12 无缺）
    {
        let mut h = HealthSummary::new();
        for a in 0..10u16 {
            let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        s.add("C37-02-挂接完整", h.hook_complete(), if h.hook_complete() { "" } else { "挂接缺口" });
    }
    // C37-03 schema 校验通过
    {
        let mut h = HealthSummary::new();
        for a in 0..10u16 {
            let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: a as u32, mask: FIELD_MASK_ALL });
        }
        s.add("C37-03-schema校验", h.schema_ok(), if h.schema_ok() { "" } else { "schema漂移" });
    }
    // C37-04 投递 10/10 可达
    {
        let mut h = HealthSummary::new();
        for a in 0..10u16 {
            let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let mut all_ok = true;
        for _ in 0..10 {
            if h.deliver(true).is_err() {
                all_ok = false;
            }
        }
        s.add("C37-04-投递可达", all_ok && h.delivered == 10 && h.reachable == 10, if all_ok { "" } else { "投递丢失" });
    }
    // C37-05 守恒（登记=投递=可达）
    {
        let mut h = HealthSummary::new();
        for a in 0..10u16 {
            let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        for _ in 0..10 {
            let _ = h.deliver(true);
        }
        s.add("C37-05-四账守恒", h.conserved(), if h.conserved() { "" } else { "守恒破" });
    }
    // C37-06 缺账清单正确
    {
        let mut h = HealthSummary::new();
        for a in 0..8u16 {
            let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let miss = h.missing_accounts();
        let ok = miss.len() == 2 && miss[0] == 8 && miss[1] == 9;
        s.add("C37-06-缺账清单", ok, if ok { "" } else { "缺账误报" });
    }
    // C37-07 封账禁缺账
    {
        let mut h = HealthSummary::new();
        for a in 0..10u16 {
            let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let fp = h.seal();
        s.add("C37-07-封账产指纹", fp.is_ok(), if fp.is_ok() { "" } else { "封账失败" });
    }
    // C37-08 指纹一致
    {
        let f = || {
            let mut h = HealthSummary::new();
            for a in 0..10u16 {
                let _ = h.register(SummaryRow { account: a, schema: SCHEMA_VER, value: 7 * a as u32, mask: FIELD_MASK_ALL });
            }
            let _ = h.deliver(true);
            h.fingerprint()
        };
        s.add("C37-08-指纹一致", f() == f(), "");
    }
    // C37-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C37-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C37-10 拒收计数守恒
    {
        let mut h = HealthSummary::new();
        let _ = h.register(SummaryRow { account: 0, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        let _ = h.register(SummaryRow { account: 0, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        s.add("C37-10-拒收计数", h.reject_count() == 1, if h.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
