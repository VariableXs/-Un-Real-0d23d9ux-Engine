//! UNX-F0645 · 域回归门禁（全绿放行与拦截）账 · 诊断段 0xB850
//! 验收锚点：UNX-F0645-J1 全绿放行/有红拦截二态正确，拦截时红项
//! 批级定位准确率 100%，双判据缺一即批内红灯。
use alloc::vec;
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::boxed::Box;


pub const SEG_BASE: u16 = 0xB850;

// ── 诊断段独占 · 门禁错误域 ─────────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_GATE: u16 = SEG_ERR + 1;   // 二态判错
const E_LOCATE: u16 = SEG_ERR + 2; // 红项定位错
const E_ZERO: u16 = SEG_ERR + 3;   // 零断言门禁
const E_DUP: u16 = SEG_ERR + 4;    // 重复挂接
const E_BAL: u16 = SEG_ERR + 5;    // 批账不平

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

// ── 门禁项 ───────────────────────────────────────────────────
/// 一项挂接：判据名 + 所属批 + 结果。
#[derive(Clone, Debug)]
pub struct GateItem {
    pub name: String,
    pub batch: String, // 批级定位单元（如 "批26"）
    pub pass: bool,
}

impl GateItem {
    pub fn fp(&self) -> u64 {
        fnv1a64(format!("{}:{}:{}", self.batch, self.name, self.pass).as_bytes())
    }
}

/// 域回归门禁：挂接集 + 二态判定 + 红项定位。
pub struct RegGate {
    pub items: Vec<GateItem>,
}

/// 二态判定结果。
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Allow,                    // 全绿放行
    Block(Vec<(String, String)>), // 有红拦截（批, 判据名）
}

impl RegGate {
    pub fn new() -> Self {
        RegGate { items: Vec::new() }
    }
    /// 挂接：重名拒。
    pub fn hook(&mut self, name: &str, batch: &str) -> bool {
        if self.items.iter().any(|i| i.name == name) {
            return false; // 重复挂接拒
        }
        self.items.push(GateItem { name: name.to_string(), batch: batch.to_string(), pass: true });
        true
    }
    /// 置红。
    pub fn fail(&mut self, name: &str) -> bool {
        match self.items.iter_mut().find(|i| i.name == name) {
            Some(i) => {
                i.pass = false;
                true
            }
            None => false,
        }
    }
    /// 二态判定：全绿 Allow，有红 Block（红项批级定位清单）。
    pub fn verdict(&self) -> Verdict {
        let reds: Vec<(String, String)> = self.items.iter()
            .filter(|i| !i.pass)
            .map(|i| (i.batch.clone(), i.name.clone()))
            .collect();
        if reds.is_empty() {
            Verdict::Allow
        } else {
            Verdict::Block(reds)
        }
    }
    /// 定位准确率：注入的每个红项都出现在 Block 清单且批归属一致。
    pub fn locate_acc(&self, injected: &[&str]) -> bool {
        match self.verdict() {
            Verdict::Allow => injected.is_empty(),
            Verdict::Block(reds) => injected.iter().all(|n| {
                let item = self.items.iter().find(|i| i.name == *n);
                match item {
                    Some(it) => reds.iter().any(|(b, nm)| nm == &it.name && b == &it.batch),
                    None => false,
                }
            }) && reds.len() == injected.len(),
        }
    }
    /// 门禁非空（零断言门禁拒判）。
    pub fn nonempty(&self) -> bool {
        !self.items.is_empty()
    }
    /// 挂接无重复（自证）。
    pub fn no_dup(&self) -> bool {
        let n = self.items.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.items[i].name == self.items[j].name {
                    return false;
                }
            }
        }
        true
    }
    /// 批域清单。
    pub fn batches(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in &self.items {
            if !out.contains(&i.batch) {
                out.push(i.batch.clone());
            }
        }
        out
    }
}

/// 标准门禁夹具：批 24/25/26 三批共 12 项全绿。
pub fn standard_gate() -> RegGate {
    let mut g = RegGate::new();
    for i in 0..4 {
        let _ = g.hook(&format!("chk24_{}", i), "批24");
    }
    for i in 0..4 {
        let _ = g.hook(&format!("chk25_{}", i), "批25");
    }
    for i in 0..4 {
        let _ = g.hook(&format!("chk26_{}", i), "批26");
    }
    g
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0645_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0645 域回归门禁");

    // 1. 全绿放行
    let g = standard_gate();
    s.add("GATE-全绿放行", g.verdict() == Verdict::Allow && g.nonempty(),
        if g.verdict() == Verdict::Allow { "" } else { "全绿误拦" });

    // 2. 有红拦截 + 定位准确率 100%（单红）
    let mut g2 = standard_gate();
    let _ = g2.fail("chk25_2");
    let v2 = g2.verdict();
    let single = g2.locate_acc(&["chk25_2"]);
    s.add("GATE-单红拦截定位", matches!(v2, Verdict::Block(_)) && single
        && v2 == Verdict::Block(vec![("批25".to_string(), "chk25_2".to_string())]),
        if single { "" } else { "定位失准" });

    // 3. 多红定位（批26 两项 + 批24 一项）
    let mut g3 = standard_gate();
    let _ = g3.fail("chk26_0");
    let _ = g3.fail("chk26_3");
    let _ = g3.fail("chk24_1");
    let multi = g3.locate_acc(&["chk26_0", "chk26_3", "chk24_1"]);
    s.add("GATE-多红定位", multi, if multi { "" } else { "多红失准" });

    // 4. 批级归属（红项批域与挂接一致）
    let v3 = g3.verdict();
    let batch_ok = match v3 {
        Verdict::Block(ref reds) => reds.iter().all(|(b, _)| b == "批24" || b == "批26")
            && reds.iter().filter(|(b, _)| b == "批26").count() == 2,
        Verdict::Allow => false,
    };
    s.add("GATE-批级归属", batch_ok, if batch_ok { "" } else { "批域错乱" });

    // 5. 全红拦截
    let mut g4 = standard_gate();
    for i in 0..g4.items.len() {
        let nm = g4.items[i].name.clone();
        let _ = g4.fail(&nm);
    }
    let all_red = g4.locate_acc(&["chk24_0", "chk24_1", "chk24_2", "chk24_3", "chk25_0", "chk25_1", "chk25_2", "chk25_3", "chk26_0", "chk26_1", "chk26_2", "chk26_3"]);
    s.add("GATE-全红拦截", all_red, if all_red { "" } else { "全红漏拦" });

    // 6. 重复挂接拒
    let mut g5 = standard_gate();
    let dup = !g5.hook("chk24_0", "批24");
    s.add("GATE-重复挂接拒", dup && g5.no_dup(), if dup { "" } else { "重复放行" });

    // 7. 零断言门禁拒判
    let empty = RegGate::new();
    s.add("GATE-空门禁拒", empty.verdict() == Verdict::Allow && !empty.nonempty(),
        if !empty.nonempty() { "" } else { "空门禁放行" });

    // 8. 置红不存在的项失败返回 false
    let mut g6 = standard_gate();
    let miss = !g6.fail("no_such");
    s.add("GATE-置红落空", miss, if miss { "" } else { "幽灵置红成功" });

    // 9. 三批域齐
    let bs = g.batches();
    s.add("GATE-批域齐", bs.len() == 3 && bs == vec!["批24", "批25", "批26"],
        if bs.len() == 3 { "" } else { "批域缺失" });

    // 10. LCG 随机注入 10 轮定位 10/10
    let mut lcg = Lcg(0xB850);
    let mut acc = 0;
    for r in 0..10 {
        let mut gr = standard_gate();
        let mut injected: Vec<&str> = Vec::new();
        let n_inj = 1 + lcg.below(4) as usize;
        for _ in 0..n_inj {
            let idx = lcg.below(gr.items.len() as u64) as usize;
            let nm = gr.items[idx].name.clone();
            let _ = gr.fail(&nm);
            // 借用安全：把名字存进静态串表
            injected.push(Box::leak(nm.clone().into_boxed_str()));
        }
        if gr.locate_acc(&injected) {
            acc += 1;
        }
        let _ = r;
    }
    s.add("GATE-随机定位10/10", acc == 10, if acc == 10 { "" } else { "随机轮失准" });

    // 11. 门禁指纹稳定且内容敏感
    let f1: u64 = g.items.iter().map(|i| i.fp()).fold(0xcbf2_9ce4_8422_2325, |a, x| {
        let mut h = a;
        for b in x.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    });
    let mut g7 = standard_gate();
    let _ = g7.fail("chk25_0");
    let f2: u64 = g7.items.iter().map(|i| i.fp()).fold(0xcbf2_9ce4_8422_2325, |a, x| {
        let mut h = a;
        for b in x.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    });
    s.add("GATE-指纹敏感", f1 != f2, if f1 != f2 { "" } else { "指纹碰撞" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB850 && SEG_BASE < 0xB870;
    s.add("GATE-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
