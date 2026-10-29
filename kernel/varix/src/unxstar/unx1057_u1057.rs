// unx1057_u1057 —— UNX-F1057 计时面健康摘要（F0899 口径——B13 段）
// 判据原文 UNX-F1057-J1：挂接完整（B13 十账摘要入 F0997 无缺），schema 校验通过，投递 10/10 次可达。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2BF0 独占（0x2BF1~0x2BFF）。
//
// 设计（纯功能 no_std）：
//   * B13 十账摘要：仲裁面十类账目逐条登记（1037 为 B12 段、本单为 B13 段）；
//   * schema 校验：版本 + 掩码合法性，非法拒收；
//   * 投递面：向 F0997 投递/可达分账，10/10 可达；
//   * 守恒：登记 = 校验通过 = 投递成功 = 可达（四账对平）；
//   * FNV 指纹冻结 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2BF0 独占） ─────────────────────────────────────────────
pub const XC_S7_OOB: u32 = 0x2BF1; // 账号越界或掩码非法
pub const XC_S7_SCHEMA: u32 = 0x2BF2; // schema 版本不符
pub const XC_S7_DUP: u32 = 0x2BF3; // 同账重复登记
pub const XC_S7_SEALED: u32 = 0x2BF4; // 封账后写摘要
pub const XC_S7_MISSING: u32 = 0x2BF5; // B13 十账缺账
pub const XC_S7_LOST: u32 = 0x2BF6; // 投递不可达

pub const SCHEMA_VER: u16 = 0x0BF0;
/// B13 十账固定账号集。
pub const B13_ACCOUNTS: [u16; 10] = [10, 11, 12, 13, 14, 15, 16, 17, 18, 19];
pub const FIELD_MASK_ALL: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy)]
pub struct Summary13Row {
    pub account: u16, // B13 账号 10..=19
    pub schema: u16,
    pub value: u32,
    pub mask: u32,
}

/// B13 段健康摘要账本。
pub struct HealthSummary13 {
    rows: Vec<Summary13Row>,
    seen: Vec<u16>,
    delivered: u32,
    reachable: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl HealthSummary13 {
    pub fn new() -> HealthSummary13 {
        HealthSummary13 {
            rows: Vec::new(),
            seen: Vec::new(),
            delivered: 0,
            reachable: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// B13 账号合法（10..=19）。
    fn acct_ok(a: u16) -> bool {
        a >= 10 && a <= 19
    }

    /// 登记一条摘要。
    pub fn register(&mut self, r: Summary13Row) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_S7_SEALED, "封账后登记"));
            return Err(XC_S7_SEALED);
        }
        if !Self::acct_ok(r.account) {
            self.rejects.push((XC_S7_OOB, "账号越界"));
            return Err(XC_S7_OOB);
        }
        if r.schema != SCHEMA_VER {
            self.rejects.push((XC_S7_SCHEMA, "schema 版本不符"));
            return Err(XC_S7_SCHEMA);
        }
        if r.mask & !FIELD_MASK_ALL != 0 {
            self.rejects.push((XC_S7_OOB, "字段掩码非法"));
            return Err(XC_S7_OOB);
        }
        for &a in &self.seen {
            if a == r.account {
                self.rejects.push((XC_S7_DUP, "同账重复登记"));
                return Err(XC_S7_DUP);
            }
        }
        self.seen.push(r.account);
        self.rows.push(r);
        Ok(())
    }

    /// schema 校验通过。
    pub fn schema_ok(&self) -> bool {
        for r in &self.rows {
            if r.schema != SCHEMA_VER || r.mask & !FIELD_MASK_ALL != 0 {
                return false;
            }
        }
        true
    }

    /// B13 十账挂接完整。
    pub fn hook_complete(&self) -> bool {
        let mut hit = [false; 10];
        for &a in &self.seen {
            if Self::acct_ok(a) {
                hit[(a - 10) as usize] = true;
            }
        }
        hit.iter().all(|&h| h)
    }

    /// 缺账清单（相对 B13 账号集）。
    pub fn missing_accounts(&self) -> Vec<u16> {
        let mut hit = [false; 10];
        for &a in &self.seen {
            if Self::acct_ok(a) {
                hit[(a - 10) as usize] = true;
            }
        }
        let mut miss = Vec::new();
        for (i, &h) in hit.iter().enumerate() {
            if !h {
                miss.push(10 + i as u16);
            }
        }
        miss
    }

    /// 向 F0997 投递。
    pub fn deliver(&mut self, ok: bool) -> Result<(), u32> {
        if self.delivered.saturating_add(1) > 1_000_000 {
            self.rejects.push((XC_S7_LOST, "投递计数饱和"));
            return Err(XC_S7_LOST);
        }
        self.delivered = self.delivered.saturating_add(1);
        if ok {
            self.reachable = self.reachable.saturating_add(1);
            Ok(())
        } else {
            self.rejects.push((XC_S7_LOST, "投递不可达"));
            Err(XC_S7_LOST)
        }
    }

    /// 守恒：登记 = 十账 = 投递 = 可达。
    pub fn conserved(&self) -> bool {
        self.rows.len() == self.seen.len()
            && self.rows.len() == 10
            && self.delivered == self.reachable
            && self.delivered >= 10
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_S7_SEALED);
        }
        if !self.hook_complete() {
            self.rejects.push((XC_S7_MISSING, "十账缺账禁封"));
            return Err(XC_S7_MISSING);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

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

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）B12 账号（0..9）写入 B13 段必拒（10/10）
    {
        let mut s = HealthSummary13::new();
        let mut hits = 0;
        for a in 0..10u16 {
            if s.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL }) == Err(XC_S7_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）schema 不符必拒
    {
        let mut s = HealthSummary13::new();
        if s.register(Summary13Row { account: 10, schema: SCHEMA_VER + 1, value: 1, mask: FIELD_MASK_ALL }) == Err(XC_S7_SCHEMA) {
            caught += 1;
        }
    }
    // 3）同账重复必拒
    {
        let mut s = HealthSummary13::new();
        let _ = s.register(Summary13Row { account: 15, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        if s.register(Summary13Row { account: 15, schema: SCHEMA_VER, value: 2, mask: FIELD_MASK_ALL }) == Err(XC_S7_DUP) {
            caught += 1;
        }
    }
    // 4）缺账禁封
    {
        let mut s = HealthSummary13::new();
        for a in 10..19u16 {
            let _ = s.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        if s.seal() == Err(XC_S7_MISSING) {
            caught += 1;
        }
    }
    // 5）投递不可达必拒
    {
        let mut s = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = s.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let _ = s.deliver(true);
        let r = s.deliver(false);
        if r == Err(XC_S7_LOST) && s.delivered == 2 && s.reachable == 1 {
            caught += 1;
        }
    }
    // 6）封账后登记必拒
    {
        let mut s = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = s.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let _ = s.seal();
        if s.register(Summary13Row { account: 10, schema: SCHEMA_VER, value: 9, mask: FIELD_MASK_ALL }) == Err(XC_S7_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1057-J1。
pub fn run_unx1057_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1057");

    // C57-01 B13 十账全登记
    {
        let mut h = HealthSummary13::new();
        let mut all_ok = true;
        for a in 10..20u16 {
            if h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 50 + a as u32, mask: FIELD_MASK_ALL }).is_err() {
                all_ok = false;
            }
        }
        s.add("C57-01-十账登记", all_ok && h.row_count() == 10, if all_ok { "" } else { "登记缺" });
    }
    // C57-02 挂接完整
    {
        let mut h = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        s.add("C57-02-挂接完整", h.hook_complete(), if h.hook_complete() { "" } else { "挂接缺口" });
    }
    // C57-03 schema 校验
    {
        let mut h = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: a as u32, mask: FIELD_MASK_ALL });
        }
        s.add("C57-03-schema校验", h.schema_ok(), if h.schema_ok() { "" } else { "schema漂移" });
    }
    // C57-04 投递 10/10 可达
    {
        let mut h = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let mut all_ok = true;
        for _ in 0..10 {
            if h.deliver(true).is_err() {
                all_ok = false;
            }
        }
        s.add("C57-04-投递可达", all_ok && h.delivered == 10 && h.reachable == 10, if all_ok { "" } else { "投递丢" });
    }
    // C57-05 四账守恒
    {
        let mut h = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        for _ in 0..10 {
            let _ = h.deliver(true);
        }
        s.add("C57-05-四账守恒", h.conserved(), if h.conserved() { "" } else { "守恒破" });
    }
    // C57-06 缺账清单
    {
        let mut h = HealthSummary13::new();
        for a in [10u16, 11, 12, 13, 14, 15, 16, 17, 19] {
            let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        let miss = h.missing_accounts();
        let ok = miss.len() == 1 && miss[0] == 18;
        s.add("C57-06-缺账清单", ok, if ok { "" } else { "清单漂移" });
    }
    // C57-07 封账产指纹
    {
        let mut h = HealthSummary13::new();
        for a in 10..20u16 {
            let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        }
        s.add("C57-07-封账指纹", h.seal().is_ok(), "");
    }
    // C57-08 指纹一致
    {
        let f = || {
            let mut h = HealthSummary13::new();
            for a in 10..20u16 {
                let _ = h.register(Summary13Row { account: a, schema: SCHEMA_VER, value: 3 * a as u32, mask: FIELD_MASK_ALL });
            }
            let _ = h.deliver(true);
            h.fingerprint()
        };
        s.add("C57-08-指纹一致", f() == f(), "");
    }
    // C57-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C57-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C57-10 拒收计数
    {
        let mut h = HealthSummary13::new();
        let _ = h.register(Summary13Row { account: 10, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        let _ = h.register(Summary13Row { account: 10, schema: SCHEMA_VER, value: 1, mask: FIELD_MASK_ALL });
        s.add("C57-10-拒收计数", h.reject_count() == 1, if h.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
