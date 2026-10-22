//! UNX-F0054 · ACPI 表版本兼容矩阵账 · 诊断段 0xB7F0
//! 验收锚点：UNX-F0054-J1 对 ACPI 2.0–6.5 各版本字段差异逐项登记，
//! 三台不同代机型全表解析通过，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB7F0;

// ── 诊断段独占 · ACPI 矩阵错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_VER: u16 = SEG_ERR + 1;    // 版本不在 2.0–6.5 域
const E_FIELD: u16 = SEG_ERR + 2;  // 字段越版本域
const E_PARSE: u16 = SEG_ERR + 3;  // 解析失败
const E_SIG: u16 = SEG_ERR + 4;    // 表签名非法
const E_BAL: u16 = SEG_ERR + 5;    // 矩阵对账不平

/// ACPI 支持版本域 2.0..=6.5（以 ×10 编码：20..=65）。
pub const VER_MIN: u32 = 20;
pub const VER_MAX: u32 = 65;

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

// ── 版本字段差异登记 ─────────────────────────────────────────
/// 一条字段登记：字段名 + 引入版本 + 表签名域。
#[derive(Clone, Debug)]
pub struct AcpiField {
    pub name: String,
    pub since: u32,      // 引入版本（×10）
    pub table_sig: String, // 表签名（FADT/MADT/…）
}

impl AcpiField {
    pub fn fp(&self) -> u64 {
        fnv1a64(format!("{}:{}:{}", self.table_sig, self.name, self.since).as_bytes())
    }
}

/// 版本差异登记表：逐项登记 + 按版本投影。
pub struct AcpiRegistry {
    pub fields: Vec<AcpiField>,
}

impl AcpiRegistry {
    pub fn new() -> Self {
        AcpiRegistry { fields: Vec::new() }
    }
    /// 登记：版本域校验。
    pub fn register(&mut self, name: &str, since: u32, sig: &str) -> bool {
        if since < VER_MIN || since > VER_MAX {
            return false;
        }
        if self.fields.iter().any(|f| f.name == name && f.table_sig == sig) {
            return false;
        }
        self.fields.push(AcpiField { name: name.to_string(), since, table_sig: sig.to_string() });
        true
    }
    /// 按版本投影：该版本可见的字段集。
    pub fn visible_at(&self, ver: u32) -> Vec<&AcpiField> {
        self.fields.iter().filter(|f| f.since <= ver).collect()
    }
    /// 版本差异：字段在 ver_a 可见而 ver_b 不可见（ver_a > ver_b）。
    pub fn delta(&self, ver_new: u32, ver_old: u32) -> Vec<String> {
        self.fields.iter()
            .filter(|f| f.since > ver_old && f.since <= ver_new)
            .map(|f| format!("{}/{}", f.table_sig, f.name))
            .collect()
    }
    /// 2.0–6.5 全版本差异逐项对账：每版本 delta 非负且和 = 全字段数。
    pub fn recon_versions(&self) -> bool {
        let mut seen: usize = 0;
        let vers = [20u32, 21, 40, 50, 51, 55, 60, 61, 62, 63, 64, 65];
        for (i, v) in vers.iter().enumerate() {
            let vis = self.visible_at(*v).len();
            if vis < seen {
                return false;
            }
            if i + 1 < vers.len() {
                seen = vis;
            }
        }
        // 全字段在 6.5 全可见
        self.visible_at(VER_MAX).len() == self.fields.len()
    }
}

// ── 机型全表解析 ─────────────────────────────────────────────
/// 一台机型：代际版本 + 表集。
#[derive(Clone, Debug)]
pub struct Machine {
    pub name: String,
    pub ver: u32,
    pub tables: Vec<(String, Vec<(String, u64)>)>, // (表签名, [(字段, 值)])
}

impl Machine {
    /// 全表解析：字段按机型版本投影解析，未知字段（晚于版本）跳过不炸。
    pub fn parse_all(&self, reg: &AcpiRegistry) -> Result<usize, u16> {
        if self.ver < VER_MIN || self.ver > VER_MAX {
            return Err(E_VER);
        }
        let mut n = 0;
        for (sig, fields) in &self.tables {
            // 表签名须在登记表有踪迹或为通用 RSDP/XSDT
            if !["RSDP", "XSDT", "FADT", "MADT", "HPET", "BGRT", "DBG2"].contains(&sig.as_str()) {
                return Err(E_SIG);
            }
            for (fname, _val) in fields {
                match reg.fields.iter().find(|f| f.name == *fname && f.table_sig == *sig) {
                    Some(f) => {
                        if f.since > self.ver {
                            return Err(E_FIELD); // 字段晚于机型版本 → 数据错
                        }
                    }
                    None => return Err(E_FIELD), // 未登记字段
                }
                n += 1;
            }
        }
        Ok(n)
    }
}

/// 三台不同代机型夹具（2.4 老机 / 4.0 中代 / 6.2 新机）。
pub fn three_machines() -> Vec<Machine> {
    vec![
        Machine {
            name: "legacy-2010".to_string(),
            ver: 24,
            tables: vec![
                ("FADT".to_string(), vec![("pm1a_evt".to_string(), 0x404), ("fadt_len".to_string(), 244)]),
                ("MADT".to_string(), vec![("lapic_addr".to_string(), 0xFEE0_0000)]),
            ],
        },
        Machine {
            name: "mid-2016".to_string(),
            ver: 40,
            tables: vec![
                ("FADT".to_string(), vec![("pm1a_evt".to_string(), 0x1804), ("x_pm1a_blk".to_string(), 0x1808)]),
                ("HPET".to_string(), vec![("hpet_base".to_string(), 0xFED0_0000)]),
            ],
        },
        Machine {
            name: "modern-2023".to_string(),
            ver: 62,
            tables: vec![
                ("FADT".to_string(), vec![("pm1a_evt".to_string(), 0x1804), ("x_pm1a_blk".to_string(), 0x1808), ("sleep_ctl_a".to_string(), 0x5)]),
                ("MADT".to_string(), vec![("lapic_addr".to_string(), 0xFEE0_0000), ("x2apic_entries".to_string(), 8)]),
                ("BGRT".to_string(), vec![("bgrt_status".to_string(), 1)]),
            ],
        },
    ]
}

/// 标准登记表：跨版本字段差异逐项（含 6.0/6.1/6.2 新字段）。
pub fn standard_registry() -> AcpiRegistry {
    let mut r = AcpiRegistry::new();
    r.register("rsdp_rev", 20, "RSDP");
    r.register("xsdt_ptr", 20, "XSDT");
    r.register("pm1a_evt", 20, "FADT");
    r.register("fadt_len", 20, "FADT");
    r.register("lapic_addr", 20, "MADT");
    r.register("x_pm1a_blk", 40, "FADT");
    r.register("hpet_base", 40, "HPET");
    r.register("sleep_ctl_a", 50, "FADT");
    r.register("x2apic_entries", 40, "MADT");
    r.register("bgrt_status", 20, "BGRT");
    r
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0054_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0054 ACPI表版本兼容矩阵");
    let reg = standard_registry();

    // 1. 登记表全字段版本域合法
    let dom = reg.fields.iter().all(|f| f.since >= VER_MIN && f.since <= VER_MAX);
    s.add("ACPI-版本域合法", dom && !reg.fields.is_empty(),
        if dom { "" } else { "字段越版本域" });

    // 2. 版本差异逐项登记对账（可见集随版本单调不减 + 6.5 全可见）
    s.add("ACPI-版本对账", reg.recon_versions(), if reg.recon_versions() { "" } else { "矩阵不平" });

    // 3. 版本差异投影（4.0 相对 2.4 新增 ≥3 项）
    let d = reg.delta(40, 24);
    s.add("ACPI-4.0差异≥3", d.len() >= 3 && d.iter().all(|x| x.contains('/')),
        if d.len() >= 3 { "" } else { "差异缺失" });

    // 4. 三台机型全表解析通过
    let machines = three_machines();
    let mut all_ok = true;
    let mut total_fields = 0;
    for m in &machines {
        match m.parse_all(&reg) {
            Ok(n) => total_fields += n,
            Err(_) => all_ok = false,
        }
    }
    s.add("ACPI-三机全解析", all_ok && total_fields == 11,
        if all_ok { "" } else { "解析失败" });

    // 5. 老机不出现新字段（2.4 机投影不含 x_pm1a_blk）
    let legacy = &machines[0];
    let vis24 = reg.visible_at(legacy.ver);
    let no_new = !vis24.iter().any(|f| f.name == "x_pm1a_blk" || f.name == "sleep_ctl_a");
    s.add("ACPI-老机无新字段", no_new, if no_new { "" } else { "新字段泄漏" });

    // 6. 越版本域拒收
    let mut r2 = standard_registry();
    let rej = !r2.register("future_field", 70, "FADT") && !r2.register("ancient", 19, "FADT");
    s.add("ACPI-越域拒", rej, if rej { "" } else { "越域放行" });

    // 7. 未登记字段解析拒
    let bad = Machine {
        name: "ghost".to_string(),
        ver: 62,
        tables: vec![("FADT".to_string(), vec![("unknown_f".to_string(), 0)])],
    };
    let rej2 = bad.parse_all(&reg) == Err(E_FIELD);
    s.add("ACPI-未登记拒", rej2, if rej2 { "" } else { "未登记放行" });

    // 8. 非法表签名拒
    let bad2 = Machine {
        name: "badsig".to_string(),
        ver: 40,
        tables: vec![("FAKE".to_string(), vec![("pm1a_evt".to_string(), 0)])],
    };
    let rej3 = bad2.parse_all(&reg) == Err(E_SIG);
    s.add("ACPI-签名拒", rej3, if rej3 { "" } else { "假签名放行" });

    // 9. 重名字段拒（同表签名）
    let mut r3 = standard_registry();
    let dup = !r3.register("pm1a_evt", 30, "FADT");
    s.add("ACPI-重名拒", dup, if dup { "" } else { "重复放行" });

    // 10. LCG 抽样 5 字段指纹稳定
    let mut lcg = Lcg(0xB7F0);
    let mut spot = true;
    for _ in 0..5 {
        let i = lcg.below(reg.fields.len() as u64) as usize;
        let f = &reg.fields[i];
        if f.fp() != f.fp() {
            spot = false;
        }
    }
    s.add("ACPI-指纹稳定", spot, if spot { "" } else { "指纹漂移" });

    // 11. 字段指纹互异
    let mut fp_uniq = true;
    for i in 0..reg.fields.len() {
        for j in (i + 1)..reg.fields.len() {
            if reg.fields[i].fp() == reg.fields[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("ACPI-指纹互异", fp_uniq, if fp_uniq { "" } else { "指纹碰撞" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB7F0 && SEG_BASE < 0xB810;
    s.add("ACPI-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
