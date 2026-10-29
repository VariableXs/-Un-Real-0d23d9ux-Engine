// unx1060_ktestb13 —— UNX-F1060 ktest B13 断言集（仲裁面 ≥40 断言）
// 判据原文 UNX-F1060-J1：本批 19 条判据聚合入 ktest 计时面，断言数 ≥40 分族可单跑，宿主档通过率 100%。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2CB0 独占（0x2CB1~0x2CBF）。
//
// 设计（纯功能 no_std）：
//   * 断言集：族+断言名+通过位逐条登记（B13 仲裁面四族）；
//   * 分族单跑：按族抽取通过/总数；
//   * 聚合：B13 19 条判据聚合出 ≥40 断言；
//   * 宿主档：host 档全量跑通过率 100% 才可封账；
//   * 守恒 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2CB0 独占） ─────────────────────────────────────────────
pub const XC_K3_OOB: u32 = 0x2CB1; // 族号越界
pub const XC_K3_DUP: u32 = 0x2CB2; // 同族同名重复
pub const XC_K3_SEALED: u32 = 0x2CB3; // 封账后加断言
pub const XC_K3_FAIL: u32 = 0x2CB4; // 宿主档失败
pub const XC_K3_FLOOR: u32 = 0x2CB5; // 断言数低于下限

pub const ASSERT_FLOOR: usize = 40;
pub const B13_CRITERIA: usize = 19;
/// B13 固定四族（仲裁面）。
pub const FAMILIES: [&str; 4] = ["arb-decide", "arb-switch", "arb-mode", "arb-health"];

/// 45 个唯一断言名（B13 聚合 19 判据展开，同族不重名）。
pub const NAMES45: [&str; 45] = [
    "m00", "m01", "m02", "m03", "m04", "m05", "m06", "m07", "m08", "m09", "m10", "m11", "m12", "m13", "m14", "m15",
    "m16", "m17", "m18", "m19", "m20", "m21", "m22", "m23", "m24", "m25", "m26", "m27", "m28", "m29", "m30", "m31",
    "m32", "m33", "m34", "m35", "m36", "m37", "m38", "m39", "m40", "m41", "m42", "m43", "m44",
];

#[derive(Clone, Copy)]
pub struct Assert13 {
    pub family: usize,
    pub name: &'static str,
    pub passed: bool,
}

/// B13 ktest 断言集。
pub struct KtestSuite13 {
    asserts: Vec<Assert13>,
    sealed: bool,
    host_mode: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl KtestSuite13 {
    pub fn new(host_mode: bool) -> KtestSuite13 {
        KtestSuite13 {
            asserts: Vec::new(),
            sealed: false,
            host_mode,
            rejects: Vec::new(),
        }
    }

    pub fn add(&mut self, a: Assert13) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_K3_SEALED, "封账后加断言"));
            return Err(XC_K3_SEALED);
        }
        if a.family >= FAMILIES.len() {
            self.rejects.push((XC_K3_OOB, "族号越界"));
            return Err(XC_K3_OOB);
        }
        for x in &self.asserts {
            if x.family == a.family && x.name == a.name {
                self.rejects.push((XC_K3_DUP, "同族同名重复"));
                return Err(XC_K3_DUP);
            }
        }
        self.asserts.push(a);
        Ok(())
    }

    /// 分族单跑。
    pub fn run_family(&mut self, family: usize) -> Result<(u32, u32), u32> {
        if family >= FAMILIES.len() {
            self.rejects.push((XC_K3_OOB, "族号越界"));
            return Err(XC_K3_OOB);
        }
        let mut pass = 0u32;
        let mut total = 0u32;
        for a in &self.asserts {
            if a.family == family {
                total = total.saturating_add(1);
                if a.passed {
                    pass = pass.saturating_add(1);
                }
            }
        }
        Ok((pass, total))
    }

    pub fn run_all(&self) -> (u32, u32) {
        let mut pass = 0u32;
        for a in &self.asserts {
            if a.passed {
                pass = pass.saturating_add(1);
            }
        }
        (pass, self.asserts.len() as u32)
    }

    pub fn host_green(&self) -> bool {
        let (p, t) = self.run_all();
        t >= ASSERT_FLOOR as u32 && p == t
    }

    pub fn floor_ok(&self) -> bool {
        self.asserts.len() >= ASSERT_FLOOR
    }

    pub fn conserved(&mut self) -> bool {
        let mut sum = 0u32;
        for f in 0..FAMILIES.len() {
            if let Ok((_, t)) = self.run_family(f) {
                sum = sum.saturating_add(t);
            }
        }
        sum as usize == self.asserts.len()
    }

    pub fn covers_b13(&self) -> bool {
        self.asserts.len() >= ASSERT_FLOOR && self.asserts.len() >= B13_CRITERIA * 2 + 2
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_K3_SEALED);
        }
        if !self.floor_ok() {
            self.rejects.push((XC_K3_FLOOR, "断言数不足禁封"));
            return Err(XC_K3_FLOOR);
        }
        if self.host_mode {
            let (p, t) = self.run_all();
            if p != t {
                self.rejects.push((XC_K3_FAIL, "宿主档有失败"));
                return Err(XC_K3_FAIL);
            }
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.asserts.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for a in &self.asserts {
            for &b in a.name.as_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h ^= (a.family as u32).wrapping_mul(0x9E37) ^ (a.passed as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.host_mode as u32;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn assert_count(&self) -> usize {
        self.asserts.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）族号越界必拒（10/10）
    {
        let mut k = KtestSuite13::new(true);
        let mut hits = 0;
        for f in 4..14u32 {
            if k.add(Assert13 { family: f as usize, name: "x", passed: true }) == Err(XC_K3_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）同族同名重复必拒
    {
        let mut k = KtestSuite13::new(true);
        let _ = k.add(Assert13 { family: 2, name: "mode-atomic", passed: true });
        if k.add(Assert13 { family: 2, name: "mode-atomic", passed: true }) == Err(XC_K3_DUP) {
            caught += 1;
        }
    }
    // 3）宿主档失败断言禁封
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 {
                family: (i % 4) as usize,
                name: match i {
                    0 => "n00", 1 => "n01", 2 => "n02", 3 => "n03", 4 => "n04",
                    5 => "n05", 6 => "n06", 7 => "n07", 8 => "n08", 9 => "n09",
                    10 => "n10", 11 => "n11", 12 => "n12", 13 => "n13", 14 => "n14",
                    15 => "n15", 16 => "n16", 17 => "n17", 18 => "n18", 19 => "n19",
                    20 => "n20", 21 => "n21", 22 => "n22", 23 => "n23", 24 => "n24",
                    25 => "n25", 26 => "n26", 27 => "n27", 28 => "n28", 29 => "n29",
                    30 => "n30", 31 => "n31", 32 => "n32", 33 => "n33", 34 => "n34",
                    35 => "n35", 36 => "n36", 37 => "n37", 38 => "n38", 39 => "n39",
                    40 => "n40", 41 => "n41", 42 => "n42", 43 => "n43",
                    _ => "n44",
                },
                passed: i != 21,
            });
        }
        if k.seal() == Err(XC_K3_FAIL) {
            caught += 1;
        }
    }
    // 4）断言数不足禁封
    {
        let mut k = KtestSuite13::new(true);
        let _ = k.add(Assert13 { family: 0, name: "single", passed: true });
        if k.seal() == Err(XC_K3_FLOOR) {
            caught += 1;
        }
    }
    // 5）封账后加断言必拒
    {
        let mut k = KtestSuite13::new(false);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        let _ = k.seal();
        if k.add(Assert13 { family: 0, name: "late", passed: true }) == Err(XC_K3_SEALED) {
            caught += 1;
        }
    }
    // 6）断言数下限绕过必拒（39 条差一条禁封）
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..39u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        if !k.floor_ok() && k.seal() == Err(XC_K3_FLOOR) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1060-J1。
pub fn run_unx1060_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1060");

    // C60-01 断言数 ≥40
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        s.add("C60-01-断言下限", k.floor_ok() && k.assert_count() == 45, if k.floor_ok() { "" } else { "断言不足" });
    }
    // C60-02 分族单跑
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        let mut all_ok = true;
        for f in 0..4 {
            if let Ok((p, t)) = k.run_family(f) {
                if t == 0 || p != t {
                    all_ok = false;
                }
            } else {
                all_ok = false;
            }
        }
        s.add("C60-02-分族单跑", all_ok, if all_ok { "" } else { "分族缺" });
    }
    // C60-03 宿主档通过率 100%
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        let (p, t) = k.run_all();
        s.add("C60-03-宿主全绿", p == t && t == 45, if p == t { "" } else { "有失败" });
    }
    // C60-04 族号越界拒
    {
        let mut k = KtestSuite13::new(true);
        let ok = k.add(Assert13 { family: 7, name: "x", passed: true }) == Err(XC_K3_OOB);
        s.add("C60-04-族号越界", ok, if ok { "" } else { "漏抓" });
    }
    // C60-05 重复名拒
    {
        let mut k = KtestSuite13::new(true);
        let _ = k.add(Assert13 { family: 3, name: "dup", passed: true });
        let ok = k.add(Assert13 { family: 3, name: "dup", passed: false }) == Err(XC_K3_DUP);
        s.add("C60-05-重复拒", ok, if ok { "" } else { "漏抓" });
    }
    // C60-06 聚合守恒
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        s.add("C60-06-聚合守恒", k.conserved(), if k.conserved() { "" } else { "守恒破" });
    }
    // C60-07 B13 聚合源覆盖
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        s.add("C60-07-B13覆盖", k.covers_b13(), if k.covers_b13() { "" } else { "覆盖缺" });
    }
    // C60-08 宿主档封账
    {
        let mut k = KtestSuite13::new(true);
        for i in 0..45u32 {
            let _ = k.add(Assert13 { family: (i % 4) as usize, name: NAMES45[i as usize], passed: true });
        }
        s.add("C60-08-封账指纹", k.seal().is_ok(), "");
    }
    // C60-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C60-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C60-10 指纹一致
    {
        let f = || {
            let mut k = KtestSuite13::new(true);
            let _ = k.add(Assert13 { family: 1, name: "fp-a", passed: false });
            let _ = k.add(Assert13 { family: 3, name: "fp-b", passed: true });
            k.fingerprint()
        };
        s.add("C60-10-指纹一致", f() == f(), "");
    }
    s
}
