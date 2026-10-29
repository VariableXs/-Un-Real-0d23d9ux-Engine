// unx1034_u1034 —— UNX-F1034 HPET 面人话化（explain 对齐体例）
// 判据原文 UNX-F1034-J1：覆盖断言（HPET 面全部状态可解释），一致性 1000 次，
// 快照通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2180 独占（0x2180~0x21BF）。
//
// 设计（纯功能 no_std）：
//   * 状态闭集人话表：HPET 面七态（在位正常/降频运行/主计数器停走/通道缺/
//     中断丢失/校准未定/封账）逐态 explain 文案冻结；
//   * 覆盖闸：七态全可解释（无未知出口）；
//   * 一致性：同态 1000 次 explain 字节级一致；
//   * 快照：FNV 逐字节指纹（三段式体例：态名+建议+码）；
//   * 码段：0x2180 + 态 + 1（闭集内单调）；
//   * 注入核六类：未知态/空文案/重态/越界码/封账改表/快照漂移全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x2180 独占） ─────────────────────────────────────────────
pub const XC_HEXP_UNKNOWN: u32 = 0x2181; // 未知状态
pub const XC_HEXP_EMPTY: u32 = 0x2182; // 空文案
pub const XC_HEXP_DUP: u32 = 0x2183; // 重态登记
pub const XC_HEXP_CODE: u32 = 0x2184; // 态码越界
pub const XC_HEXP_SEALED: u32 = 0x2185; // 封账后改表
pub const XC_HEXP_SNAP: u32 = 0x2186; // 快照漂移

/// HPET 面状态闭集（七态）。
pub const HS_OK: u32 = 0; // 在位正常
pub const HS_LOWF: u32 = 1; // 降频运行
pub const HS_MAINHALT: u32 = 2; // 主计数器停走
pub const HS_CHMISS: u32 = 3; // 通道缺
pub const HS_IRQLOST: u32 = 4; // 中断丢失
pub const HS_NOCAL: u32 = 5; // 校准未定
pub const HS_SEALED: u32 = 6; // 面已封账
pub const N_STATES: u32 = 7;

/// 三段式快照。
#[derive(Clone, Copy, PartialEq)]
pub struct HExpSnap {
    pub state: u32,
    pub name: &'static str,
    pub advice: &'static str,
    pub code: u32,
}

/// HPET 人话化表。
pub struct HExpTable {
    rows: Vec<HExpSnap>,
    sealed: bool,
    explains: u64,
}

impl HExpTable {
    pub fn new() -> HExpTable {
        HExpTable {
            rows: Vec::new(),
            sealed: false,
            explains: 0,
        }
    }

    /// 登记一态（态闭集 + 非空双闸 + 重态拒）。
    pub fn register(&mut self, state: u32, name: &'static str, advice: &'static str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_HEXP_SEALED);
        }
        if state >= N_STATES {
            return Err(XC_HEXP_CODE);
        }
        if self.rows.iter().any(|r| r.state == state) {
            return Err(XC_HEXP_DUP);
        }
        if name.is_empty() || advice.is_empty() {
            return Err(XC_HEXP_EMPTY);
        }
        self.rows.push(HExpSnap {
            state,
            name,
            advice,
            code: 0x2180 + state + 1,
        });
        Ok(())
    }

    /// explain（未登记态显性拒）。
    pub fn explain(&mut self, state: u32) -> Result<HExpSnap, u32> {
        if state >= N_STATES {
            return Err(XC_HEXP_UNKNOWN);
        }
        for r in self.rows.iter() {
            if r.state == state {
                self.explains = self.explains.wrapping_add(1);
                return Ok(*r);
            }
        }
        Err(XC_HEXP_UNKNOWN)
    }

    /// 覆盖闸：七态全可解释。
    pub fn coverage_full(&mut self) -> bool {
        for st in 0..N_STATES {
            if self.explain(st).is_err() {
                return false;
            }
        }
        true
    }

    /// 一致性：同态 1000 次全等。
    pub fn consistent_1000(&mut self, state: u32) -> bool {
        let first = match self.explain(state) {
            Ok(s) => s,
            Err(_) => return false,
        };
        for _ in 0..1000 {
            match self.explain(state) {
                Ok(s) => {
                    if s.name != first.name || s.advice != first.advice || s.code != first.code {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }

    /// 快照指纹（逐字节 FNV）。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.rows.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in self.rows.iter() {
            for b in r.name.as_bytes().iter() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            for b in r.advice.as_bytes().iter() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= r.state ^ r.code;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 20 | (self.rows.len() as u64 & 0xFFFFF)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_HEXP_SEALED);
        }
        if self.rows.len() != N_STATES as usize {
            return Err(XC_HEXP_EMPTY);
        }
        self.sealed = true;
        Ok(self.snapshot_fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn explain_count(&self) -> u64 {
        self.explains
    }
}

/// 默认七态文案。
pub fn register_hpet_defaults(t: &mut HExpTable) -> Result<(), u32> {
    t.register(HS_OK, "HPET 在位正常", "维持 HPET 主源；无需动作")?;
    t.register(HS_LOWF, "降频运行", "频点低于标称；F1003 重校准")?;
    t.register(HS_MAINHALT, "主计数器停走", "F1007 死表链联动；切备源")?;
    t.register(HS_CHMISS, "定时器通道缺", "通道账对齐；按在位通道降容")?;
    t.register(HS_IRQLOST, "中断丢失", "投递账缺口；查中断路由")?;
    t.register(HS_NOCAL, "校准未定", "先跑 F1003 三源校准")?;
    t.register(HS_SEALED, "面已封账", "账目冻结；重建面才可重配")?;
    Ok(())
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）未知态 explain 必拒（7..=16）
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let mut hits = 0;
        for st in 7..=16u32 {
            if t.explain(st) == Err(XC_HEXP_UNKNOWN) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）空文案必拒
    {
        let mut t = HExpTable::new();
        let a = t.register(HS_OK, "", "x") == Err(XC_HEXP_EMPTY);
        let b = t.register(HS_OK, "n", "") == Err(XC_HEXP_EMPTY);
        if a && b {
            caught += 1;
        }
    }
    // 3）重态登记必拒（10/10）
    {
        let mut t = HExpTable::new();
        let _ = t.register(HS_OK, "a", "b");
        let mut hits = 0;
        for _ in 0..10 {
            if t.register(HS_OK, "c", "d") == Err(XC_HEXP_DUP) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）态码越界登记必拒（7..=16）
    {
        let mut t = HExpTable::new();
        let mut hits = 0;
        for st in 7..=16u32 {
            if t.register(st, "x", "y") == Err(XC_HEXP_CODE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）封账后改表必拒
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let _ = t.seal();
        if t.register(HS_OK, "n", "a") == Err(XC_HEXP_SEALED) {
            caught += 1;
        }
    }
    // 6）快照漂移必抓（改一行指纹必变）
    {
        let fp_a = {
            let mut t = HExpTable::new();
            let _ = register_hpet_defaults(&mut t);
            t.snapshot_fingerprint()
        };
        let fp_b = {
            let mut t2 = HExpTable::new();
            let _ = t2.register(HS_OK, "HPET 在位正常", "维持 HPET 主源；无需动作");
            let _ = t2.register(HS_LOWF, "降频运行", "频点低于标称；需重校准");
            let _ = t2.register(HS_MAINHALT, "主计数器停走", "切备源");
            let _ = t2.register(HS_CHMISS, "定时器通道缺", "按在位通道降容");
            let _ = t2.register(HS_IRQLOST, "中断丢失", "查中断路由");
            let _ = t2.register(HS_NOCAL, "校准未定", "先跑三源校准");
            let _ = t2.register(HS_SEALED, "面已封账", "账目冻结");
            t2.snapshot_fingerprint()
        };
        if fp_a != fp_b {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1034-J1。
pub fn run_unx1034_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1034");

    // C34-01 覆盖断言（七态全可解释）
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let ok = t.coverage_full() && t.row_count() == 7;
        s.add("C34-01-七态覆盖", ok, if ok { "" } else { "覆盖缺" });
    }
    // C34-02 一致性 1000 次（逐态）
    {
        let mut ok = true;
        for st in 0..N_STATES {
            let mut t = HExpTable::new();
            let _ = register_hpet_defaults(&mut t);
            if !t.consistent_1000(st) {
                ok = false;
                break;
            }
        }
        s.add("C34-02-千次一致", ok, if ok { "" } else { "一致性破" });
    }
    // C34-03 快照通过
    {
        let build = || {
            let mut t = HExpTable::new();
            let _ = register_hpet_defaults(&mut t);
            t.snapshot_fingerprint()
        };
        let fp = build();
        let ok = fp != 0 && build() == fp;
        s.add("C34-03-快照通过", ok, if ok { "" } else { "快照漂" });
    }
    // C34-04 三段式码段（code = 0x2180 + state + 1）
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let mut ok = true;
        for st in 0..N_STATES {
            if let Ok(r) = t.explain(st) {
                if r.code != 0x2180 + st + 1 {
                    ok = false;
                }
            } else {
                ok = false;
            }
        }
        s.add("C34-04-码段体例", ok, if ok { "" } else { "体例破" });
    }
    // C34-05 未知态显性拒
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let ok = t.explain(7) == Err(XC_HEXP_UNKNOWN) && t.explain(1000) == Err(XC_HEXP_UNKNOWN);
        s.add("C34-05-未知态拒", ok, if ok { "" } else { "未知出口" });
    }
    // C34-06 空文案闸
    {
        let mut t = HExpTable::new();
        let ok = t.register(HS_OK, "", "x") == Err(XC_HEXP_EMPTY);
        s.add("C34-06-空文案闸", ok, if ok { "" } else { "空文案漏" });
    }
    // C34-07 重态闸
    {
        let mut t = HExpTable::new();
        let _ = t.register(HS_MAINHALT, "a", "b");
        let ok = t.register(HS_MAINHALT, "c", "d") == Err(XC_HEXP_DUP);
        s.add("C34-07-重态闸", ok, if ok { "" } else { "重态漏" });
    }
    // C34-08 封账冻结
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let fp = t.seal();
        let ok = fp.is_ok()
            && t.is_sealed()
            && t.register(HS_OK, "x", "y") == Err(XC_HEXP_SEALED)
            && t.seal() == Err(XC_HEXP_SEALED);
        s.add("C34-08-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C34-09 explain 计数
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        for st in 0..N_STATES {
            let _ = t.explain(st);
        }
        let ok = t.explain_count() == N_STATES as u64;
        s.add("C34-09-计数守恒", ok, if ok { "" } else { "计数漂" });
    }
    // C34-10 文案抽查（停走/中断丢失）
    {
        let mut t = HExpTable::new();
        let _ = register_hpet_defaults(&mut t);
        let a = t.explain(HS_MAINHALT);
        let b = t.explain(HS_IRQLOST);
        let ok = matches!(a, Ok(r) if r.name == "主计数器停走") && matches!(b, Ok(r) if r.name == "中断丢失");
        s.add("C34-10-文案冻结", ok, if ok { "" } else { "文案漂" });
    }
    // C34-11 半表显性
    {
        let mut t = HExpTable::new();
        let _ = t.register(HS_OK, "a", "b");
        let ok = t.explain(HS_CHMISS) == Err(XC_HEXP_UNKNOWN) && t.explain(HS_OK).is_ok();
        s.add("C34-11-半表显性", ok, if ok { "" } else { "半表假绿" });
    }
    // C34-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C34-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
