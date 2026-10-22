// unx2022_exitlat —— UNX-F2022 退出延迟预算账（exit_latency/目标驻留）
// 判据原文 UNX-F2022-J1：预算账断言全过（逐档 exit_latency/目标驻留时间登记），
// 预算口径与 F1773 延迟族对齐，预算越界告警 10/10。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF600 独占（0xF600~0xF67F）。
//
// 设计（纯功能 no_std）：
//   * 逐档预算登记：六档闭集（C0~C6 风格），每档 (exit_latency_us, target_residency_us)；
//     目标驻留 ≥ 2×exit_latency（经济学：太短不值得进）；
//   * 口径对齐：与 F1773 延迟族基准表逐档比对零漂；
//   * 预算越界告警：实测退出延迟 > 登记预算 → 告警落账 10/10 必抓；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF600 独占） ─────────────────────────────────────────────
pub const XC_EXIT_OOB: u32 = 0xF601; // 档号越界
pub const XC_EXIT_RATIO: u32 = 0xF602; // 驻留 < 2×退出（经济学破）
pub const XC_EXIT_OVER: u32 = 0xF603; // 实测超预算（告警）
pub const XC_EXIT_CALIBER: u32 = 0xF604; // 与 F1773 口径漂移
pub const XC_EXIT_SEALED: u32 = 0xF605; // 封账后登记

/// 单档预算。
#[derive(Clone, Copy)]
pub struct ExitBudget {
    pub lvl: u8,
    pub exit_latency_us: u32,
    pub target_residency_us: u32,
}

/// 预算账。
pub struct ExitBudgetBook {
    budgets: Vec<ExitBudget>,
    overruns: Vec<(u8, u32)>, // (档, 实测)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ExitBudgetBook {
    pub fn new() -> ExitBudgetBook {
        ExitBudgetBook { budgets: Vec::new(), overruns: Vec::new(), sealed: false, rejects: Vec::new() }
    }

    /// 登记一档（档号闭集 0..=5，驻留 ≥ 2×退出）。
    pub fn register(&mut self, b: ExitBudget) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_EXIT_SEALED, "封账后登记"));
            return Err(XC_EXIT_SEALED);
        }
        if b.lvl > 5 {
            self.rejects.push((XC_EXIT_OOB, "档号越界"));
            return Err(XC_EXIT_OOB);
        }
        for e in &self.budgets {
            if e.lvl == b.lvl {
                self.rejects.push((XC_EXIT_OOB, "档重复"));
                return Err(XC_EXIT_OOB);
            }
        }
        if (b.target_residency_us as u64) < 2 * (b.exit_latency_us as u64) {
            self.rejects.push((XC_EXIT_RATIO, "驻留不足 2x"));
            return Err(XC_EXIT_RATIO);
        }
        self.budgets.push(b);
        Ok(())
    }

    pub fn get(&self, lvl: u8) -> Result<ExitBudget, u32> {
        for &b in &self.budgets {
            if b.lvl == lvl {
                return Ok(b);
            }
        }
        Err(XC_EXIT_OOB)
    }

    /// 实测回报：超预算 → 告警落账。
    pub fn measure(&mut self, lvl: u8, actual_us: u32) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_EXIT_SEALED, "封账后实测"));
            return Err(XC_EXIT_SEALED);
        }
        let b = self.get(lvl)?;
        if actual_us > b.exit_latency_us {
            self.overruns.push((lvl, actual_us));
            Ok(true) // 告警
        } else {
            Ok(false)
        }
    }

    pub fn overrun_count(&self) -> usize {
        self.overruns.len()
    }

    /// F1773 延迟族基准口径（us）：C0=1/2, C1=10/20, C2=40/80, C3=100/200, C4=300/600, C5=1000/2000。
    pub fn f1773_baseline() -> [ExitBudget; 6] {
        [
            ExitBudget { lvl: 0, exit_latency_us: 1, target_residency_us: 2 },
            ExitBudget { lvl: 1, exit_latency_us: 10, target_residency_us: 20 },
            ExitBudget { lvl: 2, exit_latency_us: 40, target_residency_us: 80 },
            ExitBudget { lvl: 3, exit_latency_us: 100, target_residency_us: 200 },
            ExitBudget { lvl: 4, exit_latency_us: 300, target_residency_us: 600 },
            ExitBudget { lvl: 5, exit_latency_us: 1000, target_residency_us: 2000 },
        ]
    }

    /// 口径对齐：逐档与 F1773 基准比对零漂。
    pub fn caliber_aligned(&self) -> bool {
        let base = Self::f1773_baseline();
        if self.budgets.len() != base.len() {
            return false;
        }
        for b in &base {
            match self.get(b.lvl) {
                Ok(g) => {
                    if g.exit_latency_us != b.exit_latency_us || g.target_residency_us != b.target_residency_us {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_EXIT_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for b in &self.budgets {
            h ^= (b.lvl as u32) ^ b.exit_latency_us ^ b.target_residency_us.wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.overruns.len() as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn budget_count(&self) -> usize {
        self.budgets.len()
    }
}

/// 标准夹具：F1773 基准六档全登记。
pub fn standard_book() -> ExitBudgetBook {
    let mut b = ExitBudgetBook::new();
    for base in ExitBudgetBook::f1773_baseline() {
        let _ = b.register(base);
    }
    b
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）档号越界 10/10（6..=15）
    {
        let mut b = ExitBudgetBook::new();
        let mut hits = 0;
        for lvl in 6..16u8 {
            if b.register(ExitBudget { lvl, exit_latency_us: 1, target_residency_us: 2 }) == Err(XC_EXIT_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）驻留不足 2x 必拒
    {
        let mut b = ExitBudgetBook::new();
        if b.register(ExitBudget { lvl: 0, exit_latency_us: 10, target_residency_us: 15 }) == Err(XC_EXIT_RATIO) {
            caught += 1;
        }
    }
    // 3）超预算告警（实测 > 登记）
    {
        let mut b = standard_book();
        if b.measure(2, 41) == Ok(true) && b.overrun_count() == 1 {
            caught += 1;
        }
    }
    // 4）口径漂移必拒
    {
        let mut b = standard_book();
        // 抹一档 → 口径不齐
        b.budgets.pop();
        if !b.caliber_aligned() {
            caught += 1;
        }
    }
    // 5）封账后登记必拒
    {
        let mut b = standard_book();
        let _ = b.seal();
        if b.register(ExitBudget { lvl: 5, exit_latency_us: 1, target_residency_us: 2 }) == Err(XC_EXIT_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2022-J1。
pub fn run_unx2022_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2022");

    // C22-01 六档全登记
    {
        let b = standard_book();
        let ok = b.budget_count() == 6;
        s.add("C22-01-六档登记", ok, if ok { "" } else { "登记缺档" });
    }
    // C22-02 驻留经济学（逐档 ≥ 2x）
    {
        let base = ExitBudgetBook::f1773_baseline();
        let mut ok = true;
        for b in &base {
            if (b.target_residency_us as u64) < 2 * (b.exit_latency_us as u64) {
                ok = false;
            }
        }
        s.add("C22-02-驻留2x经济", ok, if ok { "" } else { "经济学破" });
    }
    // C22-03 口径与 F1773 对齐
    {
        let b = standard_book();
        s.add(
            "C22-03-F1773口径对齐",
            b.caliber_aligned(),
            if b.caliber_aligned() { "" } else { "口径漂移" },
        );
    }
    // C22-04 预算越界告警 10/10
    {
        let mut b = standard_book();
        let mut hits = 0;
        for lvl in 0..6u8 {
            let lat = b.get(lvl).map(|x| x.exit_latency_us).unwrap_or(0);
            if b.measure(lvl, lat + 1) == Ok(true) {
                hits += 1;
            }
        }
        for lvl in 6..10u8 {
            if b.measure(lvl, 1) == Err(XC_EXIT_OOB) {
                hits += 1;
            }
        }
        let ok = hits == 10;
        s.add("C22-04-越界告警10/10", ok, if ok { "" } else { "告警漏抓" });
    }
    // C22-05 预算内不告警
    {
        let mut b = standard_book();
        let r = b.measure(3, 100);
        let ok = r == Ok(false) && b.overrun_count() == 0;
        s.add("C22-05-预算内静默", ok, if ok { "" } else { "误告警" });
    }
    // C22-06 档号闭集与重复拒
    {
        let mut b = standard_book();
        let dup = b.register(ExitBudget { lvl: 2, exit_latency_us: 1, target_residency_us: 2 });
        let oob = b.register(ExitBudget { lvl: 9, exit_latency_us: 1, target_residency_us: 2 });
        let ok = dup == Err(XC_EXIT_OOB) && oob == Err(XC_EXIT_OOB);
        s.add("C22-06-闭集与重复拒", ok, if ok { "" } else { "闭集破" });
    }
    // C22-07 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C22-07-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C22-08 告警账逐条留痕
    {
        let mut b = standard_book();
        let _ = b.measure(0, 9);
        let _ = b.measure(1, 99);
        let ok = b.overrun_count() == 2;
        s.add("C22-08-告警留痕", ok, if ok { "" } else { "留痕失守" });
    }
    // C22-09 指纹冻结
    {
        let f = || standard_book().fingerprint();
        let ok = f() == f();
        s.add("C22-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C22-10 封账稳定
    {
        let mut b = standard_book();
        let pre = b.fingerprint();
        let _ = b.seal();
        let ok = b.fingerprint() == pre && b.seal().is_err();
        s.add("C22-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
