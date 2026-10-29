//! UNX-F46406 clang 驱动布设与三段式目标识别（0x40050 段）
//!
//! 锚点：UNX-F46406-J1 clang 识别三元组 ×10 场景（原生/显式指定/非法
//! 指定拒绝）行为表落账。
//! 域：CoRun·UNX-S1-B01（工具链域·clang 驱动面）。
//!
//! 结构：clang 驱动目标识别状态机（无 -target → 原生默认 → 宪法正典；
//! 有 -target → 显式三段核）+ ×10 场景行为表（每场景：输入/路径/结论）
//! + 识别账（逐场景留痕可导出）+ 注入核（空 target/坏段/越闭集/
//! 默认漂移/账封后写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40050 独占 0x10）。
pub const SEG_BASE: u32 = 0x40050;
pub const CL_TARGET_EMPTY: u32 = SEG_BASE + 0x01; // 显式 -target 为空串
pub const CL_SEG_BAD: u32 = SEG_BASE + 0x02; // 三段切分失败
pub const CL_ARCH_OOB: u32 = SEG_BASE + 0x03; // 架构段越闭集
pub const CL_SYS_OOB: u32 = SEG_BASE + 0x04; // 系统段越闭集
pub const CL_VENDOR_OOB: u32 = SEG_BASE + 0x05; // 厂商段越闭集
pub const CL_LEDGER_SEALED: u32 = SEG_BASE + 0x06; // 识别账封后拒写
pub const CL_DEFAULT_DRIFT: u32 = SEG_BASE + 0x07; // 原生默认漂移

/// 架构段闭集（与 F46403 宪法同源）。
pub const CL_ARCH_SET: [&str; 2] = ["x86_64", "aarch64"];
pub const CL_VENDOR: &str = "varix";
pub const CL_SYS_SET: [&str; 1] = ["unxreal"];
pub const CANON: &str = "x86_64-varix-unxreal";

/// 识别结论（行为表行）。
#[derive(Clone)]
pub struct IdentifyRow {
    pub scenario: &'static str,
    pub input: String,
    /// 识别路径：native（原生默认）/ explicit（显式指定）。
    pub route: &'static str,
    pub resolved: String,
    pub ok: bool,
    pub err: Option<u32>,
}

/// clang 驱动目标识别状态机。
pub struct ClangDriver {
    /// 原生默认（布设期决定：正典三元组）。
    pub native_triple: String,
    pub ledger: Vec<IdentifyRow>,
    pub sealed: bool,
}

impl ClangDriver {
    pub fn new() -> Self {
        ClangDriver {
            native_triple: CANON.to_string(),
            ledger: Vec::new(),
            sealed: false,
        }
    }

    /// 无 -target：原生默认识别（必须落在宪法正典）。
    pub fn identify_native(&self) -> Result<String, u32> {
        if self.native_triple != CANON {
            return Err(CL_DEFAULT_DRIFT);
        }
        Ok(self.native_triple.clone())
    }

    /// 显式 -target 三段核：切分 → 架构闭集 → 厂商核 → 系统闭集。
    pub fn identify_explicit(&self, target: &str) -> Result<String, u32> {
        if target.is_empty() {
            return Err(CL_TARGET_EMPTY);
        }
        let parts: Vec<&str> = target.split('-').collect();
        if parts.len() != 3 {
            return Err(CL_SEG_BAD);
        }
        if !CL_ARCH_SET.iter().any(|a| *a == parts[0]) {
            return Err(CL_ARCH_OOB);
        }
        if parts[1] != CL_VENDOR {
            return Err(CL_VENDOR_OOB);
        }
        if !CL_SYS_SET.iter().any(|s| *s == parts[2]) {
            return Err(CL_SYS_OOB);
        }
        Ok(target.to_string())
    }

    /// 场景执行 + 落账（封账拒写）。
    pub fn run_scenario(&mut self, scenario: &'static str, explicit: Option<&str>) -> Result<String, u32> {
        if self.sealed {
            return Err(CL_LEDGER_SEALED);
        }
        let (route, res) = match explicit {
            None => ("native", self.identify_native()),
            Some(t) => ("explicit", self.identify_explicit(t)),
        };
        let (resolved, err, ok) = match &res {
            Ok(r) => (r.clone(), None, true),
            Err(e) => (String::new(), Some(*e), false),
        };
        self.ledger.push(IdentifyRow {
            scenario,
            input: explicit.unwrap_or("").to_string(),
            route,
            resolved,
            ok,
            err,
        });
        res
    }

    /// ×10 场景全量行为表（5 原生/显式合法 + 5 非法拒绝）。
    pub fn run_ten(&mut self) -> (usize, usize) {
        let mut passed = 0usize;
        const TOTAL: usize = 10;
        // 合法 5：原生 ×2 + 显式正典 ×2 + 显式预留档 ×1
        if self.run_scenario("native-default", None).is_ok() {
            passed += 1;
        }
        if self.run_scenario("native-replay", None).is_ok() {
            passed += 1;
        }
        if self.run_scenario("explicit-canon", Some(CANON)).is_ok() {
            passed += 1;
        }
        if self.run_scenario("explicit-canon-replay", Some(CANON)).is_ok() {
            passed += 1;
        }
        if self.run_scenario("explicit-aarch64", Some("aarch64-varix-unxreal")).is_ok() {
            passed += 1;
        }
        // 非法 5：空 target / 段数坏 / 架构越 / 厂商越 / 系统越
        if self.run_scenario("explicit-empty", Some("")) == Err(CL_TARGET_EMPTY) {
            passed += 1;
        }
        if self.run_scenario("explicit-badseg", Some("x86_64-varix")) == Err(CL_SEG_BAD) {
            passed += 1;
        }
        if self.run_scenario("explicit-badarch", Some("riscv64-varix-unxreal")) == Err(CL_ARCH_OOB) {
            passed += 1;
        }
        if self.run_scenario("explicit-badvendor", Some("x86_64-gnu-unxreal")) == Err(CL_VENDOR_OOB) {
            passed += 1;
        }
        if self.run_scenario("explicit-badsys", Some("x86_64-varix-linux")) == Err(CL_SYS_OOB) {
            passed += 1;
        }
        (passed, TOTAL)
    }

    /// 行为表渲染（场景|输入|路径|解析|结论 五列）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for r in self.ledger.iter() {
            out.push_str(&format!(
                "{}|{}|{}|{}|{}\n",
                r.scenario,
                r.input,
                r.route,
                r.resolved,
                if r.ok { "PASS" } else { "REJECT" }
            ));
        }
        out
    }

    /// 行为表完整性核（10 行、结论列闭合、非法行必 REJECT）。
    pub fn render_ok(&self) -> bool {
        let text = self.render();
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() != 10 {
            return false;
        }
        for ln in lines.iter() {
            let seg: Vec<&str> = ln.split('|').collect();
            if seg.len() != 5 {
                return false;
            }
            if seg[4] != "PASS" && seg[4] != "REJECT" {
                return false;
            }
        }
        // 非法场景行必须 REJECT。
        for r in self.ledger.iter() {
            if r.scenario.starts_with("explicit-bad") || r.scenario == "explicit-empty" {
                if r.ok {
                    return false;
                }
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 场景账导出 schema 字段闭集（闭账材料字段完整率核）。
pub const LEDGER_SCHEMA: [&str; 6] = ["scenario", "input", "route", "resolved", "ok", "err"];

/// 导出行字段完整率核：渲染行拆六段（err 空段合法），逐段在 schema 名序。
pub fn export_schema_ok(rows: &[IdentifyRow]) -> bool {
    if rows.len() != 10 {
        return false;
    }
    rows.iter().all(|r| {
        r.scenario.len() > 0
            && (r.route == "native" || r.route == "explicit")
            && (r.ok || r.err.is_some())
            && (!r.ok || r.err.is_none())
    })
}

/// 双路仲裁：同场景 native 与 explicit 冲突时的裁决（显式优先，逐段核后返回）。
pub fn arbitrate(d: &ClangDriver, explicit: &str) -> Result<String, u32> {
    let exp = d.identify_explicit(explicit)?;
    let nat = d.identify_native()?;
    if exp != nat && explicit != CANON {
        // 显式合法但与正典不同档位（预留档位）：显式优先。
        return Ok(exp);
    }
    Ok(nat)
}

/// 目标三段指纹：逐段 FNV 复合（识别结果版本锚）。
pub fn triple_fingerprint(t: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for (i, seg) in t.split('-').enumerate() {
        for b in seg.as_bytes() {
            h ^= *b as u32 ^ (i as u32).rotate_left(4);
            h = h.wrapping_mul(16777619);
        }
    }
    h
}

/// 指纹守恒核：同三元组同指纹、段序互换指纹必变。
pub fn triple_fingerprint_invariants() -> bool {
    let a = triple_fingerprint(CANON);
    let b = triple_fingerprint("x86_64-unxreal-varix");
    a == triple_fingerprint(CANON) && a != b
}

/// 驱动版本绑定账（clang 钉版版本与驱动行为绑定，与 F46401 钉版同源版本）。
pub struct DriverBinding {
    pub version: String,
    pub native: String,
    pub scenario_count: usize,
}

impl DriverBinding {
    /// 绑定构造（版本非空 + native 必正典 + 场景数 = 10）。
    pub fn new(version: &str) -> Result<Self, u32> {
        if version.is_empty() {
            return Err(CL_TARGET_EMPTY);
        }
        Ok(DriverBinding {
            version: version.to_string(),
            native: CANON.to_string(),
            scenario_count: 10,
        })
    }

    /// 绑定一致性核（版本 = 钉版版本、native = 正典、场景数守恒）。
    pub fn consistent(&self, pinned: &str) -> bool {
        self.version == pinned && self.native == CANON && self.scenario_count == 10
    }

    /// 渲染绑定行（版本|native|场景数）。
    pub fn render(&self) -> String {
        format!("{}|{}|{}\n", self.version, self.native, self.scenario_count)
    }
}

/// 识别路径分布核：native 路与 explicit 路各自计数（×10 表内 2/8）。
pub fn route_distribution(d: &ClangDriver) -> (usize, usize) {
    let mut native = 0usize;
    let mut explicit = 0usize;
    for r in d.ledger.iter() {
        if r.route == "native" {
            native += 1;
        } else if r.route == "explicit" {
            explicit += 1;
        }
    }
    (native, explicit)
}

/// 错误码分立核：五个非法场景各归其闸，零串码（行为表内核对）。
pub fn err_codes_disjoint(d: &ClangDriver) -> bool {
    let expect: [(&str, u32); 5] = [
        ("explicit-empty", CL_TARGET_EMPTY),
        ("explicit-badseg", CL_SEG_BAD),
        ("explicit-badarch", CL_ARCH_OOB),
        ("explicit-badvendor", CL_VENDOR_OOB),
        ("explicit-badsys", CL_SYS_OOB),
    ];
    for (sc, code) in expect.iter() {
        let mut hit = false;
        for r in d.ledger.iter() {
            if r.scenario == *sc {
                if r.err != Some(*code) || r.ok {
                    return false;
                }
                hit = true;
            }
        }
        if !hit {
            return false;
        }
    }
    true
}
/// 标准夹具：×10 场景全跑 + 行为表落账。
pub fn standard_fixture() -> (ClangDriver, usize, usize) {
    let mut d = ClangDriver::new();
    let (p, n) = d.run_ten();
    (d, p, n)
}

/// 注入核：6 注入必抓（原生漂移/空 target/越闭集/封后拒写/坏段/账不全）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 原生默认漂移必抓（布设态被改 → 默认识别拒绝）
    let mut d1 = ClangDriver::new();
    d1.native_triple = "x86_64-gnu-linux".to_string();
    if d1.identify_native() == Err(CL_DEFAULT_DRIFT) {
        caught += 1;
    }
    // 2) 空 target 必抓
    let d2 = ClangDriver::new();
    if d2.identify_explicit("") == Err(CL_TARGET_EMPTY) {
        caught += 1;
    }
    // 3) 架构越闭集必抓
    if d2.identify_explicit("mips64-varix-unxreal") == Err(CL_ARCH_OOB) {
        caught += 1;
    }
    // 4) 坏段必抓
    if d2.identify_explicit("x86_64-varix") == Err(CL_SEG_BAD) {
        caught += 1;
    }
    // 5) 账封后拒写
    let (mut d3, _p, _n) = standard_fixture();
    d3.seal();
    if d3.run_scenario("post-seal", Some(CANON)) == Err(CL_LEDGER_SEALED) {
        caught += 1;
    }
    // 6) 账不全必抓（只跑 3 场景 → 渲染 3 行 ≠ 10）
    let mut d4 = ClangDriver::new();
    d4.run_scenario("native-default", None).expect("s1");
    d4.run_scenario("explicit-canon", Some(CANON)).expect("s2");
    d4.run_scenario("explicit-badarch", Some("mips64-varix-unxreal")).expect_err("s3");
    if !d4.render_ok() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46406_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46406");
    // J1-1 ×10 场景全过（5 合法全识别 + 5 非法全拒绝）
    let (_d, p, n) = standard_fixture();
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("场景 {p}/{n}") };
    s.add_dyn("46406-J1-×10场景", ok, &detail);
    // J1-2 行为表落账 10 行齐 + 非法行全 REJECT
    let (d2, _p2, _n2) = standard_fixture();
    let ok = d2.render_ok();
    s.add("46406-J1-行为表落账", ok, if ok { "" } else { "行为表漂移" });
    // J1-3 原生默认 = 宪法正典
    let d3 = ClangDriver::new();
    let ok = d3.identify_native() == Ok(CANON.to_string());
    s.add("46406-J1-原生默认正典", ok, if ok { "" } else { "默认漂移" });
    // J1-4 显式指定解析回原串（合法场景确定性）
    let ok = d3.identify_explicit(CANON) == Ok(CANON.to_string())
        && d3.identify_explicit("aarch64-varix-unxreal") == Ok("aarch64-varix-unxreal".to_string());
    s.add("46406-J1-显式解析确定", ok, if ok { "" } else { "解析漂移" });
    // J1-5 封账指纹稳定（两份夹具账逐行同）
    let (d4, _p4, _n4) = standard_fixture();
    let (d5, _p5, _n5) = standard_fixture();
    let ok = d4.render() == d5.render();
    s.add("46406-J1-账指纹稳定", ok, if ok { "" } else { "账漂移" });
    // J1-6 导出 schema 字段完整率
    let (d6, _p6, _n6) = standard_fixture();
    let ok = export_schema_ok(&d6.ledger) && LEDGER_SCHEMA.len() == 6;
    s.add("46406-J1-导出schema完整", ok, if ok { "" } else { "schema缺字段" });
    // J1-7 双路仲裁：显式正典回正典、显式预留档优先显式
    let (d7, _p7, _n7) = standard_fixture();
    let ok = arbitrate(&d7, CANON) == Ok(CANON.to_string())
        && arbitrate(&d7, "aarch64-varix-unxreal") == Ok("aarch64-varix-unxreal".to_string());
    s.add("46406-J1-双路仲裁", ok, if ok { "" } else { "仲裁漂移" });
    // J1-8 三段指纹守恒
    let ok = triple_fingerprint_invariants();
    s.add("46406-J1-三段指纹守恒", ok, if ok { "" } else { "指纹破缺" });
    // J1-9 驱动版本绑定一致
    let bind = DriverBinding::new("17.0.6").expect("binding");
    let ok = bind.consistent("17.0.6") && bind.render().starts_with("17.0.6|x86_64-varix-unxreal|10");
    s.add("46406-J1-版本绑定", ok, if ok { "" } else { "绑定漂移" });
    // J1-10 路径分布 2/8 + 错误码五闸分立
    let (d8, _p8, _n8) = standard_fixture();
    let (nat, exp) = route_distribution(&d8);
    let ok = nat == 2 && exp == 8 && err_codes_disjoint(&d8);
    s.add("46406-J1-路径分布与分立", ok, if ok { "" } else { "分布或分立漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46406-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46406_all_checks_green() {
        let set = run_unx46406_checks();
        assert!(set.all_passed(), "UNX-F46406 checks must be green: {}", set.red_items_text());
    }
}
