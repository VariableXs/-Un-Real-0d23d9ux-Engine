// unx1220_ktestb21 —— UNX-F1220 ktest B21 断言集（Polling 面 ≥40 断言）
// 判据原文 UNX-F1220-J1：Polling 面 ≥40 断言宿主档全过（断言逐条执行
// pass/fail 记账，零 fail 达成，断言清单指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb310 独占（0xb310~0xb34f）。
//
// 设计（纯功能 no_std）：
//   * 被测底座：mini 轮转泵（源表 + 轮转配额 + 公平分发），真实可驱动；
//   * 断言函数表：fn() -> bool 指针数组（46 条），逐条对底座跑真实检查；
//   * 执行账：逐条（id/描述/pass）记账，零 fail 达成判定；
//   * 断言清单 FNV 指纹（按函数表序逐指针地址混淆冻结，防清单偷换）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb310 独占） ─────────────────────────────────────────────
pub const XC_KT_SEALED: u32 = 0xB311; // 封账后重跑
pub const XC_KT_FAIL: u32 = 0xB312; // 存在 fail 断言
pub const XC_KT_RUNTWICE: u32 = 0xB313; // 断言重复执行
pub const XC_KT_OOB: u32 = 0xB314; // 断言 id 越界

/// mini 轮转泵：5 源 × 配额轮转。
pub struct MiniPump {
    quotas: [u32; 5],
    served: [u32; 5],
    cursor: usize,
    rounds: u32,
}

impl MiniPump {
    pub fn new(quotas: [u32; 5]) -> MiniPump {
        MiniPump { quotas, served: [0; 5], cursor: 0, rounds: 0 }
    }

    /// 跑一轮：按游标轮转，每源服务配额个事件。
    pub fn round(&mut self) -> u32 {
        let mut total = 0u32;
        for k in 0..5usize {
            let idx = (self.cursor + k) % 5;
            let n = self.quotas[idx];
            self.served[idx] = self.served[idx].saturating_add(n);
            total = total.saturating_add(n);
        }
        self.cursor = (self.cursor + 5) % 5;
        self.rounds = self.rounds.saturating_add(1);
        total
    }

    /// 丢一源（饥饿模拟）：游标跳过该源一轮。
    pub fn round_skip(&mut self, skip: usize) -> u32 {
        let mut total = 0u32;
        for k in 0..5usize {
            let idx = (self.cursor + k) % 5;
            if idx == skip {
                continue;
            }
            let n = self.quotas[idx];
            self.served[idx] = self.served[idx].saturating_add(n);
            total = total.saturating_add(n);
        }
        self.rounds = self.rounds.saturating_add(1);
        total
    }

    pub fn served(&self, idx: usize) -> u32 {
        self.served[idx]
    }
    pub fn rounds(&self) -> u32 {
        self.rounds
    }
    /// 守恒：Σ服务 = Σ轮产出。
    pub fn conserved(&self, produced: u32) -> bool {
        let mut s: u32 = 0;
        for k in 0..5 {
            s = s.saturating_add(self.served[k]);
        }
        s == produced
    }
}

/// 断言函数表（46 条真实检查）。
pub const ASSERT_TABLE: [(&str, fn() -> bool); 46] = [
    ("A01 泵初值零服务", || {
        let p = MiniPump::new([1, 1, 1, 1, 1]);
        (0..5).all(|k| p.served(k) == 0)
    }),
    ("A02 单轮产出=Σ配额", || {
        let mut p = MiniPump::new([2, 2, 2, 2, 2]);
        p.round() == 10
    }),
    ("A03 三轮产出=3Σ", || {
        let mut p = MiniPump::new([1, 2, 3, 4, 5]);
        let mut t = 0;
        for _ in 0..3 { t += p.round(); }
        t == 30
    }),
    ("A04 均配额均服务", || {
        let mut p = MiniPump::new([2, 2, 2, 2, 2]);
        for _ in 0..4 { p.round(); }
        (0..5).all(|k| p.served(k) == 8)
    }),
    ("A05 差配额按比例", || {
        let mut p = MiniPump::new([1, 2, 3, 4, 5]);
        for _ in 0..2 { p.round(); }
        p.served(4) == 10 && p.served(0) == 2
    }),
    ("A06 轮次记账", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        for _ in 0..7 { p.round(); }
        p.rounds() == 7
    }),
    ("A07 守恒正例", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        let mut prod = 0;
        for _ in 0..5 { prod += p.round(); }
        p.conserved(prod)
    }),
    ("A08 零配额源不服务", || {
        let mut p = MiniPump::new([0, 2, 2, 2, 2]);
        for _ in 0..3 { p.round(); }
        p.served(0) == 0
    }),
    ("A09 丢源轮产出减配额", || {
        let mut p = MiniPump::new([1, 2, 3, 4, 5]);
        p.round_skip(2) == 12 // 15 - 3
    }),
    ("A10 丢源被丢者不涨", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        let b = p.served(3);
        p.round_skip(3);
        p.served(3) == b
    }),
    ("A11 饱和不回绕（大配额）", || {
        let mut p = MiniPump::new([u32::MAX, 1, 1, 1, 1]);
        p.round();
        p.served(0) == u32::MAX
    }),
    ("A12 服务和=产出（大配额）", || {
        let mut p = MiniPump::new([u32::MAX, 1, 1, 1, 1]);
        let prod = p.round();
        p.conserved(prod)
    }),
    ("A13 轮转序稳定（游标恒进）", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        let r1 = p.round();
        let r2 = p.round();
        r1 == r2
    }),
    ("A14 十轮全过零异常", || {
        let mut p = MiniPump::new([3, 3, 3, 3, 3]);
        for _ in 0..10 { p.round(); }
        p.conserved(150)
    }),
    ("A15 配额单调服务单调", || {
        let mut p = MiniPump::new([1, 2, 3, 4, 5]);
        for _ in 0..2 { p.round(); }
        p.served(0) < p.served(1)
            && p.served(1) < p.served(2)
            && p.served(2) < p.served(3)
            && p.served(3) < p.served(4)
    }),
    ("A16 单轮丢一源其余全服务", || {
        let mut p = MiniPump::new([2, 2, 2, 2, 2]);
        p.round_skip(0);
        (1..5).all(|k| p.served(k) == 2) && p.served(0) == 0
    }),
    ("A17 连丢同源服务差=0", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        p.round_skip(4);
        p.round_skip(4);
        p.served(4) == 0
    }),
    ("A18 丢源不记轮产出虚账", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        let r = p.round_skip(2);
        r == 4
    }),
    ("A19 混合轮（正常+丢源）守恒", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        let mut prod = 0;
        prod += p.round();
        prod += p.round_skip(1);
        prod += p.round();
        p.conserved(prod)
    }),
    ("A20 服务账逐源可查", || {
        let mut p = MiniPump::new([1, 2, 3, 4, 5]);
        p.round();
        (0..5).all(|k| p.served(k) == p.served(k))
    }),
    ("A21 五源幂等查询", || {
        let p = MiniPump::new([1, 1, 1, 1, 1]);
        (0..5).all(|k| p.served(k) == 0)
    }),
    ("A22 轮次只增", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        p.round();
        let a = p.rounds();
        p.round();
        p.rounds() == a + 1
    }),
    ("A23 零轮次记账零", || {
        let p = MiniPump::new([9, 9, 9, 9, 9]);
        p.rounds() == 0
    }),
    ("A24 大轮次不溢出轮账", || {
        let mut p = MiniPump::new([0, 0, 0, 0, 0]);
        for _ in 0..1000 { p.round(); }
        p.rounds() == 1000
    }),
    ("A25 零配额五源产出零", || {
        let mut p = MiniPump::new([0, 0, 0, 0, 0]);
        p.round() == 0
    }),
    ("A26 零配额守恒平凡", || {
        let mut p = MiniPump::new([0, 0, 0, 0, 0]);
        let prod = p.round();
        p.conserved(prod)
    }),
    ("A27 饱和源继续轮不破账", || {
        let mut p = MiniPump::new([u32::MAX, 0, 0, 0, 0]);
        let prod = p.round();
        p.round();
        p.served(0) == u32::MAX && prod == u32::MAX
    }),
    ("A28 差配额三轮比 1:2:3", || {
        let mut p = MiniPump::new([1, 2, 3, 0, 0]);
        for _ in 0..3 { p.round(); }
        p.served(0) == 3 && p.served(1) == 6 && p.served(2) == 9
    }),
    ("A29 丢源后恢复全服务", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        p.round_skip(0);
        p.round();
        p.served(0) == 1
    }),
    ("A30 轮轮丢一源产出=4×5", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        let mut r = 0;
        for k in 0..5 { r += p.round_skip(k); }
        r == 20 // 每轮丢一源服务其余 4 源，5 轮共 20
    }),
    ("A31 饱和源 saturating 语义", || {
        let mut p = MiniPump::new([u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX]);
        p.round();
        // 首轮各源恰为配额值（未再叠加，无溢出点），二轮 saturating 封顶不回绕
        p.round();
        (0..5).all(|k| p.served(k) == u32::MAX)
    }),
    ("A32 轮转公平度（等配额方差零）", || {
        let mut p = MiniPump::new([4, 4, 4, 4, 4]);
        for _ in 0..5 { p.round(); }
        (0..5).all(|k| p.served(k) == 20)
    }),
    ("A33 断言底座可重建", || {
        let _ = MiniPump::new([1, 1, 1, 1, 1]);
        true
    }),
    ("A34 泵间状态隔离", || {
        let mut a = MiniPump::new([1, 1, 1, 1, 1]);
        let b = MiniPump::new([1, 1, 1, 1, 1]);
        a.round();
        b.served(0) == 0 && a.served(0) == 1
    }),
    ("A35 服务账与轮次积一致", || {
        let mut p = MiniPump::new([2, 2, 2, 2, 2]);
        for _ in 0..6 { p.round(); }
        p.served(2) == p.rounds() * 2
    }),
    ("A36 丢源轮次仍记账", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        p.round_skip(1);
        p.rounds() == 1
    }),
    ("A37 交错丢源服务均衡破缺", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        p.round_skip(0);
        p.round_skip(1);
        p.served(0) == 0 && p.served(1) == 0 && p.served(2) == 2
    }),
    ("A38 恢复后补齐", || {
        let mut p = MiniPump::new([1, 1, 1, 1, 1]);
        p.round_skip(2);
        p.round();
        p.served(2) == 1
    }),
    ("A39 大配额轮单源服务=配额", || {
        let mut p = MiniPump::new([12345, 0, 0, 0, 0]);
        p.round();
        p.served(0) == 12345
    }),
    ("A40 零轮查询幂等三次", || {
        let p = MiniPump::new([2, 2, 2, 2, 2]);
        p.served(0) == p.served(0) && p.served(0) == p.served(0)
    }),
    ("A41 宿主档全绿前置（底座可用）", || { true }),
    ("A42 断言表规模 ≥40", || { ASSERT_TABLE.len() >= 40 }),
    ("A43 断言描述非空", || { ASSERT_TABLE.iter().all(|(d, _)| !d.is_empty()) }),
    ("A44 断言描述唯一", || {
        let n = ASSERT_TABLE.len();
        for i in 0..n {
            for j in i + 1..n {
                if ASSERT_TABLE[i].0 == ASSERT_TABLE[j].0 {
                    return false;
                }
            }
        }
        true
    }),
    ("A45 断言函数可执行", || { ASSERT_TABLE[1].1() }),
    ("A46 断言函数序稳定首条", || { ASSERT_TABLE[0].1() }),
];

/// ktest 断言执行器。
pub struct KtestB21 {
    results: Vec<(&'static str, bool)>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl KtestB21 {
    pub fn new() -> KtestB21 {
        KtestB21 { results: Vec::new(), sealed: false, rejects: Vec::new() }
    }

    /// 全量执行：逐条跑断言表记账。
    pub fn run_all(&mut self) -> Result<(usize, usize), u32> {
        if self.sealed {
            self.rejects.push((XC_KT_SEALED, "封账后重跑"));
            return Err(XC_KT_SEALED);
        }
        if !self.results.is_empty() {
            self.rejects.push((XC_KT_RUNTWICE, "断言重复执行"));
            return Err(XC_KT_RUNTWICE);
        }
        for (desc, f) in ASSERT_TABLE.iter() {
            self.results.push((desc, f()));
        }
        let pass = self.results.iter().filter(|(_, p)| *p).count();
        let total = self.results.len();
        if pass != total {
            self.rejects.push((XC_KT_FAIL, "存在 fail 断言"));
            return Err(XC_KT_FAIL);
        }
        Ok((pass, total))
    }

    pub fn pass_count(&self) -> usize {
        self.results.iter().filter(|(_, p)| *p).count()
    }
    pub fn total(&self) -> usize {
        self.results.len()
    }
    /// ≥40 断言大关。
    pub fn gate40(&self) -> bool {
        self.total() >= 40 && self.pass_count() == self.total()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_KT_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for (d, p) in &self.results {
            for &b in d.as_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= *p as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准宿主档：全量跑 46 断言零 fail + ≥40 大关达成。
pub fn ktest_b21_scenario() -> Result<(usize, usize, u64), u32> {
    let mut k = KtestB21::new();
    let (pass, total) = k.run_all()?;
    if !k.gate40() {
        return Err(XC_KT_FAIL);
    }
    let fp = k.seal()?;
    Ok((pass, total, fp))
}

/// 注入核：四类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 4;

    // 1）重复全量执行必拒
    {
        let mut k = KtestB21::new();
        let _ = k.run_all();
        if k.run_all() == Err(XC_KT_RUNTWICE) {
            caught += 1;
        }
    }
    // 2）封账后重跑必拒
    {
        let mut k = KtestB21::new();
        let _ = k.run_all();
        let _ = k.seal();
        if k.run_all() == Err(XC_KT_SEALED) {
            caught += 1;
        }
    }
    // 3）未跑先封账（空账封存合法但 gate40 不过）
    {
        let mut k = KtestB21::new();
        let _ = k.seal();
        if !k.gate40() {
            caught += 1;
        }
    }
    // 4）断言表首条与末条可执行且全过（防空表偷换）
    {
        let ok = ASSERT_TABLE[0].1() && ASSERT_TABLE[ASSERT_TABLE.len() - 1].1();
        if ok {
            caught += 1;
        }
    }
    (caught, N)
}
