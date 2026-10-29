//! UNX-F56005 BIOS/固件版本锚登记与漂移探针（0x60080 段）
//!
//! 锚点：UNX-F56005-J1 固件版本/微码/设置导出逐项落账（代际锚三元组
//! 逐代登记，账本只追加不改写）；J2 版本锚变更事件→登记→重录基线→
//! 旧账保留（×2 演练全链零覆盖）；J3 漂移探针（新旧代逐字段差分显性
//! 化，无变化不构成新代）三判据全绿。判据锚定 §AI-71.2 专题一错误路径。
//! 域：CoRun·UNX-F2-B40（环境冻结域·固件锚面）。
//!
//! 结构：代际锚登记册（fw_ver × microcode × settings 逐代落账）+ 只
//! 追加纪律（代号严格递增、既有代不可改）+ 漂移探针（逐字段差分返回
//! 变更清单）+ 重录链（变更事件登记后返回新代号供 Windows 侧重录）+
//! 注入核（代号回退/空字段/零变化新代/封账改写/指纹漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60080 独占 0x20）。
pub const SEG_BASE: u32 = 0x60080;
pub const AN_GEN_BACK: u32 = SEG_BASE + 0x01; // 代号回退（新代 ≤ 既有代）
pub const AN_VER_EMPTY: u32 = SEG_BASE + 0x02; // 固件版本空串
pub const AN_SET_EMPTY: u32 = SEG_BASE + 0x03; // 设置导出空串
pub const AN_SEALED: u32 = SEG_BASE + 0x04; // 锚账已封账拒追加
pub const AN_DUP_GEN: u32 = SEG_BASE + 0x05; // 同代号重复登记
pub const AN_ZERO_ANCHOR: u32 = SEG_BASE + 0x06; // 锚指纹退化零哨兵
pub const AN_NO_CHANGE: u32 = SEG_BASE + 0x07; // 零变化新代（与上代逐字段一致）

/// 漂移字段闭集（探针差分对象）。
pub const DF_FW: usize = 0;
pub const DF_UC: usize = 1;
pub const DF_SET: usize = 2;
pub const DF_COUNT: usize = 3;
pub const DF_NAMES: [&str; DF_COUNT] = ["fw_ver", "microcode", "settings"];

/// FNV-1a 32 位指纹（锚指纹与快照核同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 漂移探针：新旧代逐字段差分，返回变更字段名清单（空 = 零变化）。
pub fn drift_probe(old: &AnchorEntry, new: &AnchorEntry) -> Vec<String> {
    let mut diff: Vec<String> = Vec::new();
    if old.fw_ver != new.fw_ver {
        diff.push(DF_NAMES[DF_FW].to_string());
    }
    if old.microcode != new.microcode {
        diff.push(DF_NAMES[DF_UC].to_string());
    }
    if old.settings_text != new.settings_text {
        diff.push(DF_NAMES[DF_SET].to_string());
    }
    diff
}

/// 代际锚账目：代号 × fw_ver × microcode × settings × 锚指纹。
#[derive(Clone)]
pub struct AnchorEntry {
    pub gen: u32,
    pub fw_ver: String,
    pub microcode: String,
    pub settings_text: String,
    pub fingerprint: u32,
}

impl AnchorEntry {
    pub fn new(gen: u32, fw_ver: &str, microcode: &str, settings_text: &str) -> Result<Self, u32> {
        if gen == 0 {
            return Err(AN_GEN_BACK);
        }
        if fw_ver.is_empty() {
            return Err(AN_VER_EMPTY);
        }
        if settings_text.is_empty() {
            return Err(AN_SET_EMPTY);
        }
        let fp = fnv1a(format!("{}|{}|{}", fw_ver, microcode, settings_text).as_bytes());
        if fp == 0 {
            return Err(AN_ZERO_ANCHOR);
        }
        Ok(AnchorEntry {
            gen,
            fw_ver: fw_ver.to_string(),
            microcode: microcode.to_string(),
            settings_text: settings_text.to_string(),
            fingerprint: fp,
        })
    }
}

/// 代际锚登记册：只追加不改写（append-only 纪律核心）。
pub struct AnchorBook {
    pub entries: Vec<AnchorEntry>,
    pub sealed: bool,
    /// 登记事件链（gen|动作 逐条留痕）。
    pub events: Vec<String>,
}

impl AnchorBook {
    pub fn new() -> Self {
        AnchorBook { entries: Vec::new(), sealed: false, events: Vec::new() }
    }

    /// 登记新代：代号严格递增 + 零变化拒收 + 只追加。
    pub fn register(&mut self, e: AnchorEntry) -> Result<u32, u32> {
        if self.sealed {
            return Err(AN_SEALED);
        }
        if let Some(prev) = self.entries.last() {
            if e.gen == prev.gen {
                return Err(AN_DUP_GEN);
            }
            if e.gen < prev.gen {
                return Err(AN_GEN_BACK);
            }
            if drift_probe(prev, &e).is_empty() {
                return Err(AN_NO_CHANGE);
            }
        }
        let gen = e.gen;
        self.entries.push(e);
        self.events.push(format!("ANCHOR|REG|gen={}", gen));
        Ok(gen)
    }

    /// 版本锚变更事件全链：事件→登记→返回新代号（供 Windows 侧重录）。
    pub fn on_change_event(
        &mut self,
        fw_ver: &str,
        microcode: &str,
        settings_text: &str,
    ) -> Result<u32, u32> {
        let gen = match self.entries.last() {
            Some(p) => p.gen + 1,
            None => 1,
        };
        let e = AnchorEntry::new(gen, fw_ver, microcode, settings_text)?;
        self.register(e)
    }

    /// 某代账目（只读查询）。
    pub fn get(&self, gen: u32) -> Option<&AnchorEntry> {
        for e in self.entries.iter() {
            if e.gen == gen {
                return Some(e);
            }
        }
        None
    }

    /// J1：只追加核——登记后早期代账目指纹不变（旧账零覆盖）。
    pub fn append_only_ok(&self, gen: u32, snapshot_fp: u32) -> bool {
        match self.get(gen) {
            Some(e) => e.fingerprint == snapshot_fp,
            None => false,
        }
    }

    /// 代数与代号连续性（1..=n 无断号）。
    pub fn gens_continuous(&self) -> bool {
        for (i, e) in self.entries.iter().enumerate() {
            if e.gen != i as u32 + 1 {
                return false;
            }
        }
        !self.entries.is_empty()
    }

    /// 封账（封后拒追加）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：两代锚（1 代基准 / 2 代微码变更）。
pub fn standard_anchors() -> AnchorBook {
    let mut b = AnchorBook::new();
    let _ = b.on_change_event("AMI-2.14", "0xd6-0xa0", "vt-on|tsx-off|cstates-default");
    let _ = b.on_change_event("AMI-2.14", "0xd6-0xa1", "vt-on|tsx-off|cstates-default");
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 代号回退（已到 2，登 1）
    let mut b = standard_anchors();
    let e = AnchorEntry::new(1, "AMI-2.15", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("back entry");
    if b.register(e) == Err(AN_GEN_BACK) {
        caught += 1;
    }
    // 2) 空固件版本
    if AnchorEntry::new(3, "", "0xd6", "s").err() == Some(AN_VER_EMPTY) {
        caught += 1;
    }
    // 3) 零变化新代（逐字段与上代一致）
    let mut b2 = standard_anchors();
    let e2 = AnchorEntry::new(3, "AMI-2.14", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("same entry");
    if b2.register(e2) == Err(AN_NO_CHANGE) {
        caught += 1;
    }
    // 4) 封账后追加
    let mut b3 = standard_anchors();
    b3.seal();
    let e3 = AnchorEntry::new(3, "AMI-2.16", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("sealed entry");
    if b3.register(e3) == Err(AN_SEALED) {
        caught += 1;
    }
    // 5) 漂移探针灵敏度（同代对比零差分，异代对比必出 diff）
    let a = AnchorEntry::new(9, "AMI-2.14", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("probe a");
    let c = AnchorEntry::new(9, "AMI-2.14", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("probe c");
    let d = AnchorEntry::new(9, "AMI-2.15", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("probe d");
    if drift_probe(&a, &c).is_empty() && drift_probe(&a, &d).len() == 1 {
        caught += 1;
    }
    (caught, 5)
}

/// 锚账渲染（审计面：逐代一行）。
pub fn render_anchors(b: &AnchorBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!(
            "gen={}|fw={}|uc={}|set={}|fp={:08x}\n",
            e.gen, e.fw_ver, e.microcode, e.settings_text, e.fingerprint
        ));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56005");
    // J1-1 逐项落账全量（两代三元组齐 + 代号连续）
    let b = standard_anchors();
    let ok = b.gens_continuous() && b.get(1).is_some() && b.get(2).is_some();
    s.add("56005-J1-逐代逐项落账", ok, if ok { "" } else { "代际账不全" });
    // J1-2 锚指纹确定性（同三元组同指纹）
    let ok = standard_anchors().get(1).expect("g1").fingerprint
        == standard_anchors().get(1).expect("g1b").fingerprint;
    s.add("56005-J1-锚指纹稳定", ok, if ok { "" } else { "锚指纹漂移" });
    // J2-1 ×2 演练全链（事件→登记→旧账保留）
    let mut drill = AnchorBook::new();
    let g1 = drill.on_change_event("AMI-2.14", "0xd6-0xa0", "vt-on|tsx-off").expect("drill1");
    let fp1 = drill.get(g1).expect("g1").fingerprint;
    let g2 = drill.on_change_event("AMI-2.15", "0xd6-0xa0", "vt-on|tsx-off").expect("drill2");
    let fp1b = drill.get(g1).expect("g1b").fingerprint;
    let g3 = drill.on_change_event("AMI-2.15", "0xd6-0xa2", "vt-on|tsx-off").expect("drill3");
    let fp1c = drill.get(g1).expect("g1c").fingerprint;
    let ok = g1 == 1 && g2 == 2 && g3 == 3 && fp1 == fp1b && fp1 == fp1c;
    s.add("56005-J2-两次演练全链", ok, if ok { "" } else { "演练链断" });
    // J2-2 旧账保留（append-only 快照核）
    let mut b2 = standard_anchors();
    let snap = b2.get(1).expect("snap").fingerprint;
    let _ = b2.on_change_event("AMI-2.16", "0xd6-0xa1", "vt-on|tsx-off|cstates-default");
    let ok = b2.append_only_ok(1, snap) && b2.entries.len() == 3;
    s.add("56005-J2-旧账零覆盖", ok, if ok { "" } else { "旧账被改写" });
    // J2-3 漂移探针差分显性（微码变更恰出 1 项）
    let d = drift_probe(
        b2.get(1).expect("o"),
        b2.get(2).expect("n"),
    );
    let ok = d.len() == 1 && d[0] == "microcode";
    s.add("56005-J2-差分逐字段显性", ok, if ok { "" } else { "差分漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56005-J3-注入核必抓", ok, &detail);
    // J3-2 事件留痕齐（REG 行数 = 代数）
    let mut ev_cnt = 0usize;
    for ev in b2.events.iter() {
        if ev.contains("ANCHOR|REG|") {
            ev_cnt += 1;
        }
    }
    let ok = ev_cnt == b2.entries.len();
    s.add("56005-J3-事件留痕齐", ok, if ok { "" } else { "事件留痕缺" });
    // J3-3 零变化新代拒收（守闸）
    let e = AnchorEntry::new(4, "AMI-2.16", "0xd6-0xa1", "vt-on|tsx-off|cstates-default")
        .expect("nochange entry");
    let ok = b2.register(e) == Err(AN_NO_CHANGE);
    s.add("56005-J3-零变化拒收", ok, if ok { "" } else { "零变化穿透" });
    // J3-4 封账只追加冻结
    let mut b3 = standard_anchors();
    b3.seal();
    let ok = b3.on_change_event("AMI-2.99", "0xd6-0xa9", "x").is_err();
    s.add("56005-J3-封账冻结", ok, if ok { "" } else { "封账后可追加" });
    // J1-3 空设置导出拒收
    let ok = AnchorEntry::new(9, "AMI-2.14", "0xd6", "").err() == Some(AN_SET_EMPTY);
    s.add("56005-J1-空设置拒收", ok, if ok { "" } else { "空设置穿透" });
    // J1-4 代号零哨兵拒收
    let ok = AnchorEntry::new(0, "AMI-2.14", "0xd6", "s").err() == Some(AN_GEN_BACK);
    s.add("56005-J1-代号零哨兵", ok, if ok { "" } else { "零代号穿透" });
    // J1-5 渲染审计面行数与代数一致
    let text = render_anchors(&b2);
    let ok = text.lines().count() == b2.entries.len() && text.contains("gen=1|");
    s.add("56005-J1-渲染审计面齐", ok, if ok { "" } else { "渲染面漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56005_all_checks_green() {
        let set = run_unx56005_checks();
        assert!(set.all_passed(), "UNX-F56005 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56005_append_only() {
        let mut b = standard_anchors();
        let snap = b.get(1).expect("g1").fingerprint;
        let _ = b.on_change_event("AMI-3.0", "0xd6-0xb0", "vt-on");
        assert!(b.append_only_ok(1, snap));
    }
}
