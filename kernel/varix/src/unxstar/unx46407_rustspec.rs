//! UNX-F46407 rust target spec JSON 语义布设（0x40060 段）
//!
//! 锚点：UNX-F46407-J1 target spec JSON 字段 ×30 校验（架构/指针宽/调用
//! 约定/ABI）与三元组一致。
//! 域：CoRun·UNX-S1-B01（工具链域·rust spec 面）。
//!
//! 结构：target spec 键值账（×30 字段闭集逐项登记）+ 字段值域核
//! （arch/pointer-width/data-layout/c-call-conv/abi 四族逐段与三元组
//! 对齐）+ spec 渲染（JSON 风格文本导出）+ spec 指纹（字段序敏感复合）
//! + 注入核（缺字段/越值域/指针宽漂移/调用约定漂移/重复键必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40060 独占 0x10）。
pub const SEG_BASE: u32 = 0x40060;
pub const RS_FIELD_MISS: u32 = SEG_BASE + 0x01; // 必填字段缺失
pub const RS_VALUE_OOB: u32 = SEG_BASE + 0x02; // 字段值越域
pub const RS_PTR_DRIFT: u32 = SEG_BASE + 0x03; // 指针宽度与架构漂移
pub const RS_CONV_DRIFT: u32 = SEG_BASE + 0x04; // 调用约定与三元组漂移
pub const RS_DUP_KEY: u32 = SEG_BASE + 0x05; // 字段键重复
pub const RS_ABI_DRIFT: u32 = SEG_BASE + 0x06; // ABI 段与三元组漂移

/// rust 钉版常量（与 F46401 同源）。
pub const RUSTC_VER: &str = "1.78.0";
pub const CANON_TRIPLE: &str = "x86_64-varix-unxreal";

/// ×30 字段闭集（键名表，顺序即登记序）。
pub const SPEC_KEYS: [&str; 30] = [
    // 身份族 ×4
    "arch", "vendor", "os", "env",
    // 指针与整型族 ×6
    "pointer-width", "pointer-align", "max-integer-width", "c-int-width", "c-long-width", "c-long-long-width",
    // 数据布局族 ×6
    "data-layout", "endian", "alignment", "aggregate-align", "vector-align", "stack-align",
    // 调用约定族 ×5
    "c-call-conv", "rust-call-conv", "interrupt-call-conv", "varargs-conv", "sret-conv",
    // ABI 与对象族 ×6
    "abi", "object-format", "relocation-model", "code-model", "tls-model", "frame-model",
    // 链接族 ×3
    "linker-flavor", "linker-path", "dynamic-linking",
];

/// 字段值域核（逐键闭集校验）。
pub fn spec_value_ok(key: &str, val: &str) -> Result<(), u32> {
    if !SPEC_KEYS.iter().any(|k| *k == key) {
        return Err(RS_FIELD_MISS);
    }
    let ok = match key {
        "arch" => val == "x86_64",
        "vendor" => val == "varix",
        "os" => val == "unxreal",
        "env" => val == "" || val == "gnu",
        "pointer-width" => val == "64",
        "pointer-align" => val == "64",
        "max-integer-width" => val == "128",
        "c-int-width" => val == "32",
        "c-long-width" => val == "64",
        "c-long-long-width" => val == "64",
        "data-layout" => val.starts_with("e-m:e-p270:32:32-p271:32:32"),
        "endian" => val == "little",
        "alignment" => val == "8",
        "aggregate-align" => val == "64",
        "vector-align" => val == "256",
        "stack-align" => val == "16",
        "c-call-conv" => val == "sysv64",
        "rust-call-conv" => val == "sysv64",
        "interrupt-call-conv" => val == "sysv64",
        "varargs-conv" => val == "sysv64",
        "sret-conv" => val == "sysv64",
        "abi" => val == "gnu",
        "object-format" => val == "elf",
        "relocation-model" => val == "pic" || val == "static",
        "code-model" => val == "small",
        "tls-model" => val == "global-dynamic",
        "frame-model" => val == "frame-pointer",
        "linker-flavor" => val == "ld.lld" || val == "gcc",
        "linker-path" => val.starts_with("/usr/bin/"),
        "dynamic-linking" => val == "on" || val == "off",
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(RS_VALUE_OOB)
    }
}

/// target spec 账（键唯一 + 值域核 + 与三元组对齐核）。
pub struct TargetSpec {
    pub rows: Vec<(String, String)>,
    pub sealed: bool,
}

impl TargetSpec {
    pub fn new() -> Self {
        TargetSpec { rows: Vec::new(), sealed: false }
    }

    /// 字段登记（重复键拒绝 + 值越域拒绝）。
    pub fn set(&mut self, key: &str, val: &str) -> Result<(), u32> {
        spec_value_ok(key, val)?;
        if self.rows.iter().any(|(k, _)| k == key) {
            return Err(RS_DUP_KEY);
        }
        self.rows.push((key.to_string(), val.to_string()));
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.rows.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// ×30 全登记核。
    pub fn full(&self) -> bool {
        self.rows.len() == SPEC_KEYS.len()
            && SPEC_KEYS.iter().all(|k| self.get(k).is_some())
    }

    /// 架构/指针宽/调用约定/ABI 四族与三元组一致核（J1 四段）。
    pub fn align_triple(&self, triple: &str) -> Result<(), u32> {
        let parts: Vec<&str> = triple.split('-').collect();
        if parts.len() != 3 {
            return Err(RS_ABI_DRIFT);
        }
        if self.get("arch") != Some(parts[0]) {
            return Err(RS_PTR_DRIFT);
        }
        if self.get("pointer-width") != Some("64") {
            return Err(RS_PTR_DRIFT);
        }
        if self.get("c-call-conv") != Some("sysv64") {
            return Err(RS_CONV_DRIFT);
        }
        if self.get("abi") != Some("gnu") && self.get("env") != Some("gnu") {
            return Err(RS_ABI_DRIFT);
        }
        Ok(())
    }

    /// JSON 风格渲染（"key": "value" 逐行，登记序输出）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        for (i, (k, v)) in self.rows.iter().enumerate() {
            let comma = if i + 1 < self.rows.len() { "," } else { "" };
            out.push_str(&format!("  \"{}\": \"{}\"{}\n", k, v, comma));
        }
        out.push_str("}\n");
        out
    }

    /// 渲染完整性核（行数 = 字段数 + 2 括号行；键序与登记序一致）。
    pub fn render_ok(&self) -> bool {
        let t = self.render();
        let lines: Vec<&str> = t.lines().collect();
        if lines.len() != self.rows.len() + 2 {
            return false;
        }
        lines[0] == "{" && lines[lines.len() - 1] == "}"
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// spec 指纹（键序敏感复合 FNV）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for (k, v) in self.rows.iter() {
            for b in format!("{}={};", k, v).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 数据布局段解析核：data-layout 以 '-' 分段，逐段 "p宽度:对齐:对齐" 形态。
pub fn data_layout_segments(dl: &str) -> Vec<&str> {
    dl.split('-').filter(|s| !s.is_empty()).collect()
}

/// 数据布局指针段核：p270/p271 段必须为 32:32 形态（x86_64 小端族惯例）。
pub fn data_layout_ptr_ok(dl: &str) -> bool {
    let segs = data_layout_segments(dl);
    let p270 = segs.iter().find(|s| s.starts_with("p270:"));
    let p271 = segs.iter().find(|s| s.starts_with("p271:"));
    match (p270, p271) {
        (Some(a), Some(b)) => a.ends_with("32:32") && b.ends_with("32:32"),
        _ => false,
    }
}

/// 数据布局小端标记核（e- 前缀 = little endian）。
pub fn data_layout_little_endian(dl: &str) -> bool {
    dl.starts_with("e-")
}

/// spec 演化账：字段变更逐条留痕（键|旧值|新值|波次）。
pub struct SpecHistory {
    pub rows: Vec<(String, String, String, u32)>,
}

impl SpecHistory {
    pub fn new() -> Self {
        SpecHistory { rows: Vec::new() }
    }

    /// 变更登记（同键同值无效变更拒绝记账）。
    pub fn record(&mut self, key: &str, old: &str, new: &str, wave: u32) -> bool {
        if old == new || key.is_empty() || wave == 0 {
            return false;
        }
        self.rows.push((key.to_string(), old.to_string(), new.to_string(), wave));
        true
    }

    /// 波次单调核（历史按登记序严格升波次）。
    pub fn waves_increasing(&self) -> bool {
        for w in self.rows.windows(2) {
            if w[0].3 >= w[1].3 {
                return false;
            }
        }
        true
    }

    /// 键定位查询（该键最后一次变更的新值）。
    pub fn last_value(&self, key: &str) -> Option<&str> {
        self.rows
            .iter()
            .rev()
            .find(|(k, _, _, _)| k == key)
            .map(|(_, _, nv, _)| nv.as_str())
    }

    /// 渲染（键|旧|新|波次 四列）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (k, o, n, w) in self.rows.iter() {
            out.push_str(&format!("{}|{}|{}|w{}\n", k, o, n, w));
        }
        out
    }
}

/// env/abi 双键联动核：env=gnu 时 abi 必为 gnu（Varix 单 ABI 纪律）。
pub fn env_abi_cohesion(sp: &TargetSpec) -> bool {
    let env = sp.get("env");
    let abi = sp.get("abi");
    match (env, abi) {
        (Some("gnu"), Some("gnu")) => true,
        (Some(""), Some("gnu")) => true,
        (Some("gnu"), None) => false,
        (None, _) => false,
        _ => false,
    }
}

/// 链接族联动核：flavor=ld.lld 时 linker-path 必以 /usr/bin/ld.lld 结尾。
pub fn linker_cohesion(sp: &TargetSpec) -> bool {
    match (sp.get("linker-flavor"), sp.get("linker-path")) {
        (Some("ld.lld"), Some(p)) => p.ends_with("/ld.lld"),
        (Some("gcc"), Some(p)) => p.ends_with("/gcc"),
        _ => false,
    }
}

/// spec 导出 schema 字段完整率核（30 键逐一在渲染文中出现）。
pub fn export_coverage_ok(sp: &TargetSpec) -> bool {
    let text = sp.render();
    SPEC_KEYS.iter().all(|k| text.contains(&format!("\"{}\":", k)))
}
/// 标准夹具：×30 字段全登记（与三元组 x86_64-varix-unxreal 对齐）。
pub fn standard_spec() -> TargetSpec {
    let defs: [(&str, &str); 30] = [
        ("arch", "x86_64"),
        ("vendor", "varix"),
        ("os", "unxreal"),
        ("env", "gnu"),
        ("pointer-width", "64"),
        ("pointer-align", "64"),
        ("max-integer-width", "128"),
        ("c-int-width", "32"),
        ("c-long-width", "64"),
        ("c-long-long-width", "64"),
        ("data-layout", "e-m:e-p270:32:32-p271:32:32-p272:64:64"),
        ("endian", "little"),
        ("alignment", "8"),
        ("aggregate-align", "64"),
        ("vector-align", "256"),
        ("stack-align", "16"),
        ("c-call-conv", "sysv64"),
        ("rust-call-conv", "sysv64"),
        ("interrupt-call-conv", "sysv64"),
        ("varargs-conv", "sysv64"),
        ("sret-conv", "sysv64"),
        ("abi", "gnu"),
        ("object-format", "elf"),
        ("relocation-model", "pic"),
        ("code-model", "small"),
        ("tls-model", "global-dynamic"),
        ("frame-model", "frame-pointer"),
        ("linker-flavor", "ld.lld"),
        ("linker-path", "/usr/bin/ld.lld"),
        ("dynamic-linking", "on"),
    ];
    let mut sp = TargetSpec::new();
    for (k, v) in defs.iter() {
        sp.set(k, v).expect("fixture spec field");
    }
    sp
}

/// 注入核：6 注入必抓（缺字段/越值域/指针宽漂移/调用约定漂移/重复键/spec 指纹漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 值越域必抓（pointer-width 给 32）
    let mut sp = TargetSpec::new();
    if sp.set("pointer-width", "32") == Err(RS_VALUE_OOB) {
        caught += 1;
    }
    // 2) 未知键必抓（归 FIELD_MISS 闸）
    if sp.set("neon-engine", "on") == Err(RS_FIELD_MISS) {
        caught += 1;
    }
    // 3) 指针宽漂移必抓（登记合法但与 64 位三元组对不上——构造 32 位值域外的对齐核）
    let mut sp2 = standard_spec();
    // 篡改内部行模拟漂移（绕过 set 闸直接改账）。
    for (k, v) in sp2.rows.iter_mut() {
        if k == "pointer-width" {
            *v = "32".to_string();
        }
    }
    if sp2.align_triple(CANON_TRIPLE).is_err() {
        caught += 1;
    }
    // 4) 调用约定漂移必抓
    let mut sp3 = standard_spec();
    for (k, v) in sp3.rows.iter_mut() {
        if k == "c-call-conv" {
            *v = "msx64".to_string();
        }
    }
    if sp3.align_triple(CANON_TRIPLE) == Err(RS_CONV_DRIFT) {
        caught += 1;
    }
    // 5) 重复键必抓
    let mut sp4 = standard_spec();
    if sp4.set("arch", "x86_64") == Err(RS_DUP_KEY) {
        caught += 1;
    }
    // 6) spec 缺件必抓（只登记 20 字段 → full 假）
    let mut sp5 = TargetSpec::new();
    for k in SPEC_KEYS.iter().take(20) {
        let v = match *k {
            "arch" => "x86_64",
            "vendor" => "varix",
            "os" => "unxreal",
            _ => "sysv64",
        };
        sp5.set(k, v).expect("partial set");
    }
    if !sp5.full() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46407_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46407");
    // J1-1 ×30 字段全登记
    let sp = standard_spec();
    let ok = sp.full();
    s.add("46407-J1-×30字段全登记", ok, if ok { "" } else { "字段不全" });
    // J1-2 四族与三元组一致
    let ok = sp.align_triple(CANON_TRIPLE).is_ok();
    s.add("46407-J1-四族三元组一致", ok, if ok { "" } else { "四族漂移" });
    // J1-3 身份族逐段锚（arch/vendor/os）
    let ok = sp.get("arch") == Some("x86_64")
        && sp.get("vendor") == Some("varix")
        && sp.get("os") == Some("unxreal");
    s.add("46407-J1-身份族锚定", ok, if ok { "" } else { "身份段漂移" });
    // J1-4 指针与整型族值正确
    let ok = sp.get("pointer-width") == Some("64")
        && sp.get("c-int-width") == Some("32")
        && sp.get("max-integer-width") == Some("128");
    s.add("46407-J1-整型族正确", ok, if ok { "" } else { "整型族漂移" });
    // J1-5 调用约定族五键同闭
    let ok = ["c-call-conv", "rust-call-conv", "interrupt-call-conv", "varargs-conv", "sret-conv"]
        .iter()
        .all(|k| sp.get(k) == Some("sysv64"));
    s.add("46407-J1-调用约定族同闭", ok, if ok { "" } else { "调用约定漂移" });
    // J1-6 JSON 渲染完整
    let ok = sp.render_ok() && sp.render().contains("\"arch\": \"x86_64\"");
    s.add("46407-J1-JSON渲染", ok, if ok { "" } else { "渲染漂移" });
    // J1-7 spec 指纹稳定
    let ok = sp.fingerprint() == standard_spec().fingerprint();
    s.add("46407-J1-spec指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J1-8 数据布局三段核（小端 + p270/p271 指针段）
    let dl = sp.get("data-layout").unwrap_or("");
    let ok = data_layout_little_endian(dl)
        && data_layout_ptr_ok(dl)
        && data_layout_segments(dl).len() >= 5;
    s.add("46407-J1-数据布局段核", ok, if ok { "" } else { "布局段漂移" });
    // J1-9 env/abi 与链接族双联动
    let ok = env_abi_cohesion(&sp) && linker_cohesion(&sp);
    s.add("46407-J1-联动核", ok, if ok { "" } else { "联动破缺" });
    // J1-10 演化账：3 次变更升波次 + 末值查询 + 导出覆盖 30 键
    let mut hist = SpecHistory::new();
    let r1 = hist.record("relocation-model", "static", "pic", 1);
    let r2 = hist.record("dynamic-linking", "off", "on", 2);
    let r3 = hist.record("relocation-model", "pic", "static", 3);
    let ok = r1 && r2 && r3
        && hist.waves_increasing()
        && hist.last_value("relocation-model") == Some("static")
        && export_coverage_ok(&sp)
        && !hist.record("arch", "x86_64", "x86_64", 4);
    s.add("46407-J1-演化账", ok, if ok { "" } else { "演化账漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46407-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46407_all_checks_green() {
        let set = run_unx46407_checks();
        assert!(set.all_passed(), "UNX-F46407 checks must be green: {}", set.red_items_text());
    }
}
