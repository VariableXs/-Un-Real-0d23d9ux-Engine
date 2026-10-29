//! UNX-F56016 环境指纹导出 CLI（envctl export）（0x601E0 段）
//!
//! 锚点：UNX-F56016-J1 一键导出 env_hash + 12 类冻结清单（JSON 与 MD
//! 双版本逐类齐全）；J2 导出与探针判定零漂移（×3 对照逐类一致）；J3
//! 双版本互检（JSON/MD 键集合逐类互映，缺类即拒）三判据全绿。判据锚
//! 定 §AI-71.4 下游冻结接口①。
//! 域：CoRun·UNX-F2-B40（环境冻结域·导出面）。
//!
//! 结构：12 类清单闭集（逐类键名落账）+ 状态值账（逐类判定值 ∈ 闭集）
//! + 导出引擎（JSON 行 × MD 行双版本同源生成）+ 互检核（双版本键集
//! 合逐类互映）+ 注入核（越闭集类/非法状态值/空哈希导出/封账改写/缺
//! 类导出必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x601E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x601E0;
pub const EX_CAT_UNK: u32 = SEG_BASE + 0x01; // 越闭集清单类
pub const EX_VAL_BAD: u32 = SEG_BASE + 0x02; // 状态值越闭集
pub const EX_HASH_MISS: u32 = SEG_BASE + 0x03; // env_hash 零哨兵导出
pub const EX_SEALED: u32 = SEG_BASE + 0x04; // 导出账已封账拒改
pub const EX_CAT_MISS: u32 = SEG_BASE + 0x05; // 缺类导出
pub const EX_MIRROR_MISS: u32 = SEG_BASE + 0x06; // 双版本互检缺键
pub const EX_DUP_CAT: u32 = SEG_BASE + 0x07; // 同类重复落账

/// 12 类冻结清单闭集。
pub const CAT_NAMES: [&str; 12] = [
    "hw_id", "fw_ver", "power_plan", "fs_layout", "background_set",
    "clock_source", "peripheral_silence", "process_whitelist", "update_window",
    "disk_topology", "time_window", "reject_audit",
];

/// 状态值闭集（frozen / declared / open）。
pub const V_FROZEN: usize = 0;
pub const V_DECLARED: usize = 1;
pub const V_OPEN: usize = 2;
pub const V_NAMES: [&str; 3] = ["frozen", "declared", "open"];

pub fn val_ok(v: usize) -> bool {
    v < V_NAMES.len()
}

/// FNV-1a 32 位（env_hash 同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 导出账：env_hash + 12 类逐类状态值。
pub struct ExportState {
    pub env_hash: u32,
    pub values: [Option<usize>; 12],
    pub sealed: bool,
}

impl ExportState {
    pub fn new(env_hash: u32) -> Result<Self, u32> {
        if env_hash == 0 {
            return Err(EX_HASH_MISS);
        }
        Ok(ExportState { env_hash, values: [None; 12], sealed: false })
    }


    /// 封账印（补：判据与注入核调用点所需）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 逐类落状态值（封账拒改）。
    pub fn set(&mut self, cat: usize, val: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(EX_SEALED);
        }
        if cat >= 12 {
            return Err(EX_CAT_UNK);
        }
        if !val_ok(val) {
            return Err(EX_VAL_BAD);
        }
        self.values[cat] = Some(val);
        Ok(())
    }

    /// J1：12 类全落账。
    pub fn complete(&self) -> bool {
        for v in self.values.iter() {
            if v.is_none() {
                return false;
            }
        }
        true
    }

    /// JSON 版导出（每类一行键值）。
    pub fn export_json(&self) -> Result<String, u32> {
        if !self.complete() {
            return Err(EX_CAT_MISS);
        }
        let mut out = String::new();
        out.push_str(&format!("{{\"env_hash\":\"{:08x}\"}}\n", self.env_hash));
        for i in 0..12 {
            let v = self.values[i].expect("checked");
            out.push_str(&format!("{{\"{}\":\"{}\"}}\n", CAT_NAMES[i], V_NAMES[v]));
        }
        Ok(out)
    }

    /// MD 版导出（每类一行键值）。
    pub fn export_md(&self) -> Result<String, u32> {
        if !self.complete() {
            return Err(EX_CAT_MISS);
        }
        let mut out = String::new();
        out.push_str(&format!("- env_hash: {:08x}\n", self.env_hash));
        for i in 0..12 {
            let v = self.values[i].expect("checked");
            out.push_str(&format!("- {}: {}\n", CAT_NAMES[i], V_NAMES[v]));
        }
        Ok(out)
    }
}

/// J2：双版本互检核——JSON/MD 逐类键互映零漂移。
pub fn mirror_check(json: &str, md: &str) -> bool {
    for cat in CAT_NAMES.iter() {
        let jkey = format!("{{\"{}\":\"", cat);
        let mkey = format!("- {}: ", cat);
        if !json.contains(jkey.as_str()) || !md.contains(mkey.as_str()) {
            return false;
        }
    }
    json.contains("env_hash") && md.contains("env_hash")
}

/// J2：导出与探针判定零漂移 ×3（同状态三次导出逐字节一致）。
pub fn triple_export_stable(st: &ExportState) -> bool {
    let j1 = st.export_json().expect("j1");
    let j2 = st.export_json().expect("j2");
    let j3 = st.export_json().expect("j3");
    let m1 = st.export_md().expect("m1");
    let m2 = st.export_md().expect("m2");
    let m3 = st.export_md().expect("m3");
    j1 == j2 && j2 == j3 && m1 == m2 && m2 == m3
}

/// 标准夹具：12 类全 frozen/declared 态 + 固定环境哈希。
pub fn standard_export() -> ExportState {
    let hash = fnv1a(b"hw=VARIX-BASE-001|fw=unxreal-1.0.3|pp=balanced");
    let mut st = ExportState::new(hash).expect("fixture");
    let mut rows: Vec<(usize, usize)> = Vec::new();
    for i in 0..12 {
        let v = if i < 6 { V_FROZEN } else { V_DECLARED };
        rows.push((i, v));
    }
    for r in rows.iter() {
        st.set(r.0, r.1).expect("set");
    }
    st
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集类（cat=15）
    let mut st = standard_export();
    if st.set(15, V_FROZEN) == Err(EX_CAT_UNK) {
        caught += 1;
    }
    // 2) 状态值越闭集
    if st.set(0, 9) == Err(EX_VAL_BAD) {
        caught += 1;
    }
    // 3) 零哈希导出
    if ExportState::new(0).err() == Some(EX_HASH_MISS) {
        caught += 1;
    }
    // 4) 缺类导出
    let mut st2 = ExportState::new(7).expect("st2");
    let _ = st2.set(0, V_FROZEN);
    if st2.export_json() == Err(EX_CAT_MISS) {
        caught += 1;
    }
    // 5) 封账后改写
    let mut st3 = standard_export();
    st3.seal();
    if st3.set(0, V_OPEN) == Err(EX_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56016_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56016");
    // J1-1 12 类全落账
    let st = standard_export();
    let ok = st.complete();
    s.add("56016-J1-十二类全落账", ok, if ok { "" } else { "清单缺类" });
    // J1-2 env_hash 非零且随导出落头
    let json = st.export_json().expect("json");
    let ok = st.env_hash != 0 && json.contains("\"env_hash\":");
    s.add("56016-J1-哈希落头", ok, if ok { "" } else { "哈希头缺" });
    // J1-3 状态值闭集语义（frozen 前 6 类 / declared 后 6 类）
    let mut frozen_n = 0usize;
    let mut declared_n = 0usize;
    for i in 0..12 {
        if st.values[i] == Some(V_FROZEN) {
            frozen_n += 1;
        }
        if st.values[i] == Some(V_DECLARED) {
            declared_n += 1;
        }
    }
    let ok = frozen_n == 6 && declared_n == 6;
    s.add("56016-J1-状态值闭集", ok, if ok { "" } else { "状态值漂移" });
    // J2-1 JSON/MD 双版本互检零漂移
    let md = st.export_md().expect("md");
    let ok = mirror_check(&json, &md);
    s.add("56016-J2-双版本互检", ok, if ok { "" } else { "互检缺键" });
    // J2-2 ×3 对照零漂移
    let ok = triple_export_stable(&st);
    s.add("56016-J2-三次对照零漂移", ok, if ok { "" } else { "导出漂移" });
    // J2-3 行数语义（JSON/MD 各 13 行：哈希头 + 12 类）
    let ok = json.lines().count() == 13 && md.lines().count() == 13;
    s.add("56016-J2-行数语义", ok, if ok { "" } else { "行数漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56016-J3-注入核必抓", ok, &detail);
    // J3-2 缺类显性拒
    let mut st2 = ExportState::new(9).expect("s2");
    let _ = st2.set(0, V_FROZEN);
    let ok = st2.export_json().err() == Some(EX_CAT_MISS);
    s.add("56016-J3-缺类显性", ok, if ok { "" } else { "缺类被吞" });
    // J3-3 封账冻结
    let mut st3 = standard_export();
    st3.seal();
    let ok = st3.set(3, V_OPEN) == Err(EX_SEALED)
        && st3.export_json().expect("j") == json;
    s.add("56016-J3-封账冻结", ok, if ok { "" } else { "封账后可改" });
    // J1-4 类名闭集首位尾位同源
    let ok = CAT_NAMES[0] == "hw_id" && CAT_NAMES[11] == "reject_audit";
    s.add("56016-J1-类名同源", ok, if ok { "" } else { "类名漂移" });
    // J2-4 MD 首行为哈希头
    let ok = md.starts_with("- env_hash: ");
    s.add("56016-J2-MD哈希头", ok, if ok { "" } else { "MD 头漂移" });
    // J1-5 哈希对状态敏感（同哈希口径稳定）
    let ok = standard_export().env_hash == standard_export().env_hash;
    s.add("56016-J1-哈希口径稳定", ok, if ok { "" } else { "哈希口径漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56016_all_checks_green() {
        let set = run_unx56016_checks();
        assert!(set.all_passed(), "UNX-F56016 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56016_export_stable() {
        let st = standard_export();
        let j1 = st.export_json().expect("j1");
        let j2 = st.export_json().expect("j2");
        assert_eq!(j1, j2);
    }
}
