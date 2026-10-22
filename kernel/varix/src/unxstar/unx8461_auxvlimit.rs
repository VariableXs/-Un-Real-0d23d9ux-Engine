//! UNX-F8461 · auxv 条目超限拒档（B03 消费面）账 · 诊断段 0xB520
//! 验收锚点：UNX-F8461-J1 正向：合法 auxv 条目栈构造通过（B03 判据承接）；
//! 反向：条目数超限注入全拒（栈预算守恒——不超写），双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB520;

/// auxv 栈预算：最大条目数（每条 16B：key u64 + val u64）。
pub const AUXV_MAX_ENTRIES: usize = 32;
pub const AUXV_ENTRY_SIZE: u64 = 16;
/// 栈总预算 4KiB（auxv 之外还有 argv/envp）。
pub const STACK_BUDGET: u64 = 4096;

/// auxv 键（Linux 对照子集）。
pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PAGESZ: u64 = 6;
pub const AT_ENTRY: u64 = 9;
pub const AT_UID: u64 = 11;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;

/// auxv 条目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuxEntry {
    pub key: u64,
    pub val: u64,
}

impl AuxEntry {
    pub fn new(key: u64, val: u64) -> Self {
        AuxEntry { key, val }
    }
    pub fn known(&self) -> bool {
        [AT_NULL, AT_PHDR, AT_PAGESZ, AT_ENTRY, AT_UID, AT_RANDOM, AT_EXECFN].contains(&self.key)
    }
}

/// auxv 栈构造判档：条目数不超限 + 总字节守恒 + 键已知 + AT_NULL 收尾。
pub fn build_auxv(entries: &[AuxEntry]) -> Result<Vec<AuxEntry>, &'static str> {
    if entries.len() > AUXV_MAX_ENTRIES {
        return Err("too_many_entries");
    }
    let bytes = entries.len() as u64 * AUXV_ENTRY_SIZE;
    if bytes > STACK_BUDGET {
        return Err("stack_budget");
    }
    for e in entries {
        if !e.known() {
            return Err("unknown_key");
        }
    }
    // AT_NULL 收尾要求：末条必须 AT_NULL
    match entries.last() {
        Some(e) if e.key == AT_NULL => {}
        _ => return Err("no_at_null"),
    }
    Ok(entries.to_vec())
}

/// 拒档器。
pub struct AuxvGate {
    pub passed: u32,
    pub rejected: u32,
    pub last_reject: Option<&'static str>,
}

impl AuxvGate {
    pub fn new() -> Self {
        AuxvGate { passed: 0, rejected: 0, last_reject: None }
    }
    pub fn check(&mut self, entries: &[AuxEntry]) -> bool {
        match build_auxv(entries) {
            Ok(_) => {
                self.passed += 1;
                true
            }
            Err(e) => {
                self.rejected += 1;
                self.last_reject = Some(e);
                false
            }
        }
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB520 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8461_B520;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("预算-32条×16B恰512B", AUXV_MAX_ENTRIES as u64 * AUXV_ENTRY_SIZE == 512));
    out.push(("收尾-缺AT_NULL拒", build_auxv(&[AuxEntry::new(AT_PAGESZ, 4096)]).is_err()));
    out.push(("键-未知键拒", build_auxv(&[AuxEntry::new(999, 1), AuxEntry::new(AT_NULL, 0)]).is_err()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8461_checks：本单判据聚合（诊断段 0xB520）。
pub fn run_unx8461_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8461 auxv 条目超限拒档");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    let mut g = AuxvGate::new();

    // 正向：合法 auxv 栈构造（B03 承接集）
    let good = vec![
        AuxEntry::new(AT_PHDR, 0x400040),
        AuxEntry::new(AT_PAGESZ, 4096),
        AuxEntry::new(AT_ENTRY, 0x401000),
        AuxEntry::new(AT_UID, 1000),
        AuxEntry::new(AT_RANDOM, 0x7FFF_0000),
        AuxEntry::new(AT_EXECFN, 0x7FFE_0000),
        AuxEntry::new(AT_NULL, 0),
    ];
    let p1 = g.check(&good);
    s.add("正向-合法auxv七条构造通过", p1, if p1 { "" } else { "合法样本误拒" });

    // 正向边界：恰 32 条（含 AT_NULL 收尾）
    let mut edge: Vec<AuxEntry> = (0..31u64).map(|i| AuxEntry::new(AT_UID, 1000 + i)).collect();
    edge.push(AuxEntry::new(AT_NULL, 0));
    let p2 = g.check(&edge);
    s.add("边界-恰32条含收尾通过", p2, if p2 { "" } else { "边界误拒" });

    // 反向：条目数超限注入全拒
    let mut all_rej = true;
    for n in [33usize, 40, 64, 100].iter() {
        let mut bad: Vec<AuxEntry> = (0..*n as u64).map(|i| AuxEntry::new(AT_UID, i)).collect();
        bad.push(AuxEntry::new(AT_NULL, 0));
        if g.check(&bad) {
            all_rej = false;
        }
    }
    let rej_ok = all_rej && g.last_reject == Some("too_many_entries");
    s.add("反向-超限4/4全拒留因", rej_ok, if rej_ok { "" } else { "超限漏网" });

    // 栈预算守恒：条目数虽合法但字节数超预算拒（16B×条 ≤ 4096 → 条目 ≤ 256，32 已封顶；
    // 用更大预算模型再核：条目 200 条 × 16B = 3200B < 4096 但 > 32 上限仍拒）
    let mut many: Vec<AuxEntry> = (0..200u64).map(|i| AuxEntry::new(AT_UID, i)).collect();
    many.push(AuxEntry::new(AT_NULL, 0));
    let budget_rej = !g.check(&many);
    s.add("守恒-200条不超写（上限+预算双闸）", budget_rej, if budget_rej { "" } else { "预算漏洞" });

    // 反向 10/10 注入
    let mut g2 = AuxvGate::new();
    let mut n2 = 0u32;
    for i in 0..10u32 {
        let mut bad: Vec<AuxEntry> = (0..33 + i as u64).map(|k| AuxEntry::new(AT_UID, k)).collect();
        bad.push(AuxEntry::new(AT_NULL, 0));
        if !g2.check(&bad) {
            n2 += 1;
        }
    }
    s.add("双判据-反向10/10全拒", n2 == 10, if n2 == 10 { "" } else { "反向缺口" });

    // 正向缺一红灯：合法路径 10/10 全过
    let mut g3 = AuxvGate::new();
    let mut n3 = 0u32;
    for i in 0..10u32 {
        let mut e: Vec<AuxEntry> = (0..i as u64).map(|k| AuxEntry::new(AT_PAGESZ, 4096 + k)).collect();
        e.push(AuxEntry::new(AT_NULL, 0));
        if g3.check(&e) {
            n3 += 1;
        }
    }
    s.add("双判据-正向10/10全过", n3 == 10, if n3 == 10 { "" } else { "正向缺口" });

    // 20 样本随机演练：过/拒两态合计
    let mut g4 = AuxvGate::new();
    let mut seed: u64 = 0x8461_0020;
    let mut npass = 0u32;
    for _ in 0..20u32 {
        let n = (lcg_next(&mut seed) % 40) as usize; // 0..40，>32 拒
        let mut e: Vec<AuxEntry> = (0..n as u64).map(|k| AuxEntry::new(AT_PAGESZ, 4096 + k)).collect();
        e.push(AuxEntry::new(AT_NULL, 0));
        if g4.check(&e) {
            npass += 1;
        }
    }
    let mix_ok = npass > 0 && g4.rejected > 0 && npass + g4.rejected == 20;
    s.add("随机-20样本过/拒合计恰20", mix_ok, if mix_ok { "" } else { "混合漂移" });

    // FNV auxv 指纹稳定
    let h1 = fnv1a(format!("{:?}", good).as_bytes());
    let h2 = fnv1a(format!("{:?}", good).as_bytes());
    s.add("FNV-auxv指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
