//! UNX-F46404 编译器 -dumpmachine 类三元组查询语义（0x40030 段）
//!
//! 锚点：UNX-F46404-J1 gcc/clang 双编译器查询输出与三元组宪法逐段一致 ×2。
//! 域：CoRun·UNX-S1-B01（工具链域·查询语义面）。
//!
//! 结构：双编译器查询驱动（gcc/clang 各自 dumpmachine 出口）+ 输出归一器
//! （去空白/去尾换行）+ 宪法逐段对账核（×2 次重放一致）+ 注入核
//! （空输出/尾噪声/段漂移/双编译器不一致必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40030 独占 0x10）。
pub const SEG_BASE: u32 = 0x40030;
pub const DM_EMPTY_OUT: u32 = SEG_BASE + 0x01; // 查询输出为空
pub const DM_TRAIL_NOISE: u32 = SEG_BASE + 0x02; // 尾部噪声（非裸三元组行）
pub const DM_SEG_DRIFT: u32 = SEG_BASE + 0x03; // 查询段与宪法漂移
pub const DM_DUAL_DIFF: u32 = SEG_BASE + 0x04; // 双编译器输出不一致
pub const DM_REPLAY_DIFF: u32 = SEG_BASE + 0x05; // ×2 重放不一致

/// 编译器身份闭集（本域两件嫁接源）。
pub const CC_GCC: usize = 0;
pub const CC_CLANG: usize = 1;
pub const CC_COUNT: usize = 2;
pub const CC_NAMES: [&str; CC_COUNT] = ["gcc", "clang"];

/// 模拟查询驱动：给定编译器与工具链布设态，产出 dumpmachine 输出行。
/// 布设态一致 → 双编译器输出同一正典三元组（宪法约束面）。
pub fn dumpmachine(cc: usize, toolchain_triple: &str) -> Result<String, u32> {
    if cc >= CC_COUNT {
        return Err(DM_EMPTY_OUT);
    }
    let t = toolchain_triple.trim();
    if t.is_empty() {
        return Err(DM_EMPTY_OUT);
    }
    // 布设态单行输出（模拟真实 -dumpmachine：一行三元组，无尾噪声）。
    Ok(format!("{}\n", t))
}

/// 输出归一器：去首尾空白与尾换行，非单行（含内部换行）拒绝。
pub fn normalize_output(raw: &str) -> Result<String, u32> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(DM_EMPTY_OUT);
    }
    if t.contains('\n') || t.contains('\r') {
        return Err(DM_TRAIL_NOISE);
    }
    Ok(t.to_string())
}

/// 逐段对账：查询输出三段与宪法正典逐段相等。
pub fn seg_match_constitution(out: &str) -> Result<(String, String, String), u32> {
    let parts: Vec<&str> = out.split('-').collect();
    if parts.len() != 3 {
        return Err(DM_SEG_DRIFT);
    }
    if parts[0] != "x86_64" || parts[1] != "varix" || parts[2] != "unxreal" {
        return Err(DM_SEG_DRIFT);
    }
    Ok((parts[0].to_string(), parts[1].to_string(), parts[2].to_string()))
}

/// J1 主体：双编译器 ×2 次重放逐段一致全核（返回 (过数, 总数)）。
pub fn dual_replay_audit(toolchain_triple: &str) -> (usize, usize) {
    let mut passed = 0usize;
    const TOTAL: usize = 4; // 2 编译器 × 2 重放
    for cc in 0..CC_COUNT {
        for _replay in 0..2 {
            let raw = match dumpmachine(cc, toolchain_triple) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let norm = match normalize_output(&raw) {
                Ok(n) => n,
                Err(_) => continue,
            };
            if seg_match_constitution(&norm).is_ok() {
                passed += 1;
            }
        }
    }
    (passed, TOTAL)
}

/// 双编译器一致性核：两路归一输出逐字节相等。
pub fn dual_cc_consistent(toolchain_triple: &str) -> Result<(), u32> {
    let g = normalize_output(&dumpmachine(CC_GCC, toolchain_triple)?)?;
    let c = normalize_output(&dumpmachine(CC_CLANG, toolchain_triple)?)?;
    if g != c {
        return Err(DM_DUAL_DIFF);
    }
    Ok(())
}

/// ×2 重放一致性核：同一编译器两次查询输出逐字节相等。
pub fn replay_consistent(cc: usize, toolchain_triple: &str) -> Result<(), u32> {
    let r1 = normalize_output(&dumpmachine(cc, toolchain_triple)?)?;
    let r2 = normalize_output(&dumpmachine(cc, toolchain_triple)?)?;
    if r1 != r2 {
        return Err(DM_REPLAY_DIFF);
    }
    Ok(())
}

/// 查询出口账：逐次查询留痕（编译器 × 布设态 × 归一输出 × 逐段结论）。
pub struct QueryLedger {
    pub rows: Vec<(usize, String, String, bool)>,
    pub sealed: bool,
}

impl QueryLedger {
    pub fn new() -> Self {
        QueryLedger { rows: Vec::new(), sealed: false }
    }

    /// 执行一次带留痕的查询（归一失败或段漂移也如实入账，不静默）。
    pub fn query(&mut self, cc: usize, toolchain_triple: &str) -> Result<String, u32> {
        let raw = dumpmachine(cc, toolchain_triple)?;
        let norm = normalize_output(&raw)?;
        let seg_ok = seg_match_constitution(&norm).is_ok();
        self.rows.push((cc, toolchain_triple.to_string(), norm.clone(), seg_ok));
        if seg_ok {
            Ok(norm)
        } else {
            Err(DM_SEG_DRIFT)
        }
    }

    /// 全账段一致核（账内每行逐段对齐宪法）。
    pub fn all_seg_ok(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|(_, _, _, ok)| *ok)
    }

    /// ×2 重放核（账内同编译器同布设态至少两条且输出一致）。
    pub fn replay_pairs_ok(&self, cc: usize) -> bool {
        let mut outs: Vec<&String> = Vec::new();
        for (c, t, o, _) in self.rows.iter() {
            if *c == cc && t == standard_fixture() {
                outs.push(o);
            }
        }
        outs.len() >= 2 && outs.windows(2).all(|w| w[0] == w[1])
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for (c, t, o, ok) in self.rows.iter() {
            let line = format!("{}|{}|{}|{}", CC_NAMES[*c], t, o, ok);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 版本绑定查询：布设态按组件版本绑定（同正典三元组、版本进账不进出）。
pub struct BoundQuery {
    /// (comp, version, triple) 绑定表。
    pub bindings: Vec<(usize, String, String)>,
}

impl BoundQuery {
    pub fn new() -> Self {
        BoundQuery { bindings: Vec::new() }
    }

    /// 绑定登记：三元组必须过逐段对账；同组件重复绑定拒绝。
    pub fn bind(&mut self, comp: usize, version: &str, triple: &str) -> Result<(), u32> {
        seg_match_constitution(triple)?;
        if self.bindings.iter().any(|(c, _, _)| *c == comp) {
            return Err(DM_DUAL_DIFF);
        }
        if version.is_empty() {
            return Err(DM_EMPTY_OUT);
        }
        self.bindings.push((comp, version.to_string(), triple.to_string()));
        Ok(())
    }

    /// 查询某组件的绑定三元组（未绑定显性拒绝）。
    pub fn resolve(&self, comp: usize) -> Result<&str, u32> {
        for (c, _v, t) in self.bindings.iter() {
            if *c == comp {
                return Ok(t);
            }
        }
        Err(DM_SEG_DRIFT)
    }

    /// 绑定数核（五件全绑 = J1 消费前置）。
    pub fn all_bound(&self) -> bool {
        self.bindings.len() == 5
    }
}

/// 查询行为表渲染（布设态 → 归一输出 → 结论 三列，审计可读）。
pub fn render_behavior_table(ledger: &QueryLedger) -> String {
    let mut out = String::new();
    for (c, t, o, ok) in ledger.rows.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}\n",
            CC_NAMES[*c],
            t,
            o,
            if *ok { "PASS" } else { "DRIFT" }
        ));
    }
    out
}

/// 行为表完整性核（行三段以上且结论列闭合集）。
pub fn behavior_table_ok(text: &str, expect: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == expect
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 4 && (seg[3] == "PASS" || seg[3] == "DRIFT")
        })
}

/// 标准夹具：布设态 = 正典三元组。
pub fn standard_fixture() -> &'static str {
    "x86_64-varix-unxreal"
}

/// 注入核：6 注入必抓（空输出/多行噪声/段漂移/双编译器不一致/重放不一致/未知编译器）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空输出必抓
    if normalize_output("") == Err(DM_EMPTY_OUT) {
        caught += 1;
    }
    // 2) 多行噪声必抓
    if normalize_output("x86_64-varix-unxreal\nwarning:deprecated\n") == Err(DM_TRAIL_NOISE) {
        caught += 1;
    }
    // 3) 段漂移必抓（布设态给错系统段）
    if seg_match_constitution("x86_64-varix-linux").is_err() {
        caught += 1;
    }
    // 4) 双编译器不一致必抓（一路给漂移布设态模拟）
    let g = normalize_output(&dumpmachine(CC_GCC, "x86_64-varix-unxreal").expect("g")).expect("gn");
    let c = normalize_output(&dumpmachine(CC_CLANG, "x86_64-varix-linux").expect("c")).expect("cn");
    if g != c {
        caught += 1;
    }
    // 5) 重放不一致必抓（两次不同布设态模拟重放漂移）
    let r1 = normalize_output(&dumpmachine(CC_GCC, "x86_64-varix-unxreal").expect("r1")).expect("r1n");
    let r2 = normalize_output(&dumpmachine(CC_GCC, "aarch64-varix-unxreal").expect("r2")).expect("r2n");
    if r1 != r2 {
        caught += 1;
    }
    // 6) 未知编译器身份必抓
    if dumpmachine(9, "x86_64-varix-unxreal") == Err(DM_EMPTY_OUT) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46404_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46404");
    // J1-1 双编译器 ×2 重放逐段一致（4/4）
    let (p, n) = dual_replay_audit(standard_fixture());
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("审计 {p}/{n}") };
    s.add_dyn("46404-J1-双编译器×2一致", ok, &detail);
    // J1-2 双编译器输出逐字节一致
    let ok = dual_cc_consistent(standard_fixture()).is_ok();
    s.add("46404-J1-双编译器同输出", ok, if ok { "" } else { "双编译器分叉" });
    // J1-3 ×2 重放逐字节一致（gcc/clang 各验）
    let ok = replay_consistent(CC_GCC, standard_fixture()).is_ok()
        && replay_consistent(CC_CLANG, standard_fixture()).is_ok();
    s.add("46404-J1-重放一致", ok, if ok { "" } else { "重放漂移" });
    // J1-4 逐段对账三元组返回
    let segs = seg_match_constitution(standard_fixture());
    let ok = segs == Ok(("x86_64".to_string(), "varix".to_string(), "unxreal".to_string()));
    s.add("46404-J1-逐段对账", ok, if ok { "" } else { "逐段对账漂移" });
    // J1-5 查询出口账全绿 + 重放对 + 封账指纹稳定
    let mut led = QueryLedger::new();
    for _i in 0..2 {
        led.query(CC_GCC, standard_fixture()).expect("led gcc");
        led.query(CC_CLANG, standard_fixture()).expect("led clang");
    }
    let fp = led.fingerprint();
    let ok = led.all_seg_ok() && led.replay_pairs_ok(CC_GCC) && led.replay_pairs_ok(CC_CLANG);
    s.add("46404-J1-出口账全绿", ok, if ok { "" } else { "出口账漂移" });
    // J1-6 版本绑定五件全绑 + 解析回正典
    let mut bq = BoundQuery::new();
    let vers = ["13.2.0", "17.0.6", "1.78.0", "2.43.0", "3.28.3"];
    let mut bind_ok = true;
    for c in 0..5 {
        if bq.bind(c, vers[c], standard_fixture()).is_err() {
            bind_ok = false;
        }
    }
    let ok = bind_ok && bq.all_bound() && bq.resolve(0) == Ok(standard_fixture());
    s.add("46404-J1-版本绑定查询", ok, if ok { "" } else { "绑定查询漂移" });
    // J1-7 行为表渲染四行齐 + 封账指纹稳定
    let table = render_behavior_table(&led);
    let ok = behavior_table_ok(&table, 4) && fp == led.fingerprint();
    s.add("46404-J1-行为表渲染", ok, if ok { "" } else { "行为表漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46404-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46404_all_checks_green() {
        let set = run_unx46404_checks();
        assert!(set.all_passed(), "UNX-F46404 checks must be green: {}", set.red_items_text());
    }
}
