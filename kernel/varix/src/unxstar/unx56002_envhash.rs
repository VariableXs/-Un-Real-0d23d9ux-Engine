//! UNX-F56002 env_profile 数据结构与环境哈希计算器（0x60020 段）
//!
//! 锚点：UNX-F56002-J1 env_profile{hw_id, fw_ver, power_plan, fs_layout,
//! background_set[], env_hash} schema 字段全落且哈希计算幂等（同环境两次
//! 计算零差异 ×3 轮）；J2 清单变更任一字段哈希必变（×12 注入全检出）；
//! J3 规范化与封账语义（background_set 排序去重后入哈希、封账只读拒改）
//! 三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·profile 面）。
//!
//! 结构：env_profile 登记册（五 schema 字段逐项落账 + background_set
//! 规范化排序去重）+ 环境哈希核（FNV-1a 逐字段折叠，幂等重算）+ 字段
//! 变更注入核（×12 变更逐项哈希必变全检出）+ 封账只读闸 + 渲染导出面。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60020 独占 0x20）。
pub const SEG_BASE: u32 = 0x60020;
pub const ENV_UNK_FIELD: u32 = SEG_BASE + 0x01; // 未知 schema 字段（非五字段闭集）
pub const ENV_EMPTY: u32 = SEG_BASE + 0x02; // 标量字段空串拒收
pub const ENV_BG_EMPTY: u32 = SEG_BASE + 0x03; // background_set 空集拒收
pub const ENV_BG_BLANK: u32 = SEG_BASE + 0x04; // background_set 含空白项
pub const ENV_SEALED: u32 = SEG_BASE + 0x05; // env_profile 已封账拒改
pub const ENV_HASH_ZERO: u32 = SEG_BASE + 0x06; // 环境哈希退化为零哨兵
pub const ENV_SCHEMA_MISS: u32 = SEG_BASE + 0x07; // schema 字段未落全即投入使用

/// env_profile schema 字段闭集（判据与登记共用同源命名）。
pub const F_HW: usize = 0;
pub const F_FW: usize = 1;
pub const F_POWER: usize = 2;
pub const F_FS: usize = 3;
pub const F_BG: usize = 4;
pub const F_COUNT: usize = 5;

/// schema 字段名（渲染导出与核对共用）。
pub const FIELD_NAMES: [&str; F_COUNT] =
    ["hw_id", "fw_ver", "power_plan", "fs_layout", "background_set"];

/// FNV-1a 32 位通用指纹原语（环境哈希与账目指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// background_set 规范化：空集拒收、空白项拒收、排序去重折叠。
pub fn canon_bg(raw: &[&str]) -> Result<Vec<String>, u32> {
    if raw.is_empty() {
        return Err(ENV_BG_EMPTY);
    }
    let mut v: Vec<String> = Vec::new();
    for item in raw.iter() {
        if item.trim().is_empty() {
            return Err(ENV_BG_BLANK);
        }
        v.push((*item).to_string());
    }
    // 排序后相邻去重（同项重复折叠为一）。
    v.sort();
    let mut out: Vec<String> = Vec::new();
    for s in v.into_iter() {
        match out.last() {
            Some(l) if *l == s => {}
            _ => out.push(s),
        }
    }
    Ok(out)
}

/// env_profile 登记册：五字段逐项落账 + 环境哈希随写随算。
#[derive(Clone)]
pub struct EnvProfile {
    pub hw_id: String,
    pub fw_ver: String,
    pub power_plan: String,
    pub fs_layout: String,
    pub background_set: Vec<String>,
    pub env_hash: u32,
    pub sealed: bool,
}

impl EnvProfile {
    pub fn new() -> Self {
        EnvProfile {
            hw_id: String::new(),
            fw_ver: String::new(),
            power_plan: String::new(),
            fs_layout: String::new(),
            background_set: Vec::new(),
            env_hash: 0,
            sealed: false,
        }
    }

    /// 规范序列化（哈希输入口径，字段顺序固定）。
    pub fn hash_text(&self) -> String {
        format!(
            "hw={}|fw={}|pp={}|fs={}|bg={}",
            self.hw_id,
            self.fw_ver,
            self.power_plan,
            self.fs_layout,
            self.background_set.join(",")
        )
    }

    /// 环境哈希重算（幂等：同状态两算零差异）。
    pub fn recompute_hash(&mut self) -> u32 {
        self.env_hash = fnv1a(self.hash_text().as_bytes());
        self.env_hash
    }

    /// 标量字段落账：未知字段拒、空串拒、封账拒，落后即重算哈希。
    pub fn set_scalar(&mut self, field: usize, val: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(ENV_SEALED);
        }
        if field >= F_COUNT || field == F_BG {
            return Err(ENV_UNK_FIELD);
        }
        if val.is_empty() {
            return Err(ENV_EMPTY);
        }
        match field {
            F_HW => self.hw_id = val.to_string(),
            F_FW => self.fw_ver = val.to_string(),
            F_POWER => self.power_plan = val.to_string(),
            F_FS => self.fs_layout = val.to_string(),
            _ => return Err(ENV_UNK_FIELD),
        }
        self.recompute_hash();
        Ok(())
    }

    /// background_set 落账：规范化后重算哈希。
    pub fn set_bg(&mut self, raw: &[&str]) -> Result<(), u32> {
        if self.sealed {
            return Err(ENV_SEALED);
        }
        self.background_set = canon_bg(raw)?;
        self.recompute_hash();
        Ok(())
    }

    /// J1：schema 字段全落（标量四项非空 + bg 非空 + 哈希非零哨兵）。
    pub fn schema_complete(&self) -> bool {
        !self.hw_id.is_empty()
            && !self.fw_ver.is_empty()
            && !self.power_plan.is_empty()
            && !self.fs_layout.is_empty()
            && !self.background_set.is_empty()
            && self.env_hash != 0
    }

    /// 封账（封后所有 setter 拒写，闭账材料不可变）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：基准机 env_profile（固定字段，确定性哈希）。
pub fn standard_profile() -> EnvProfile {
    let mut p = EnvProfile::new();
    let _ = p.set_scalar(F_HW, "VARIX-BASE-001");
    let _ = p.set_scalar(F_FW, "unxreal-1.0.3");
    let _ = p.set_scalar(F_POWER, "balanced");
    let _ = p.set_scalar(F_FS, "varixfs|/|gpt:align4096");
    let _ = p.set_bg(&["varixd", "logd", "probe"]);
    p
}

/// ×12 字段变更注入核：任一字段变更后哈希必变，逐项检出计数。
pub fn mutation_core() -> (usize, usize) {
    // 变更清单：标量四项直改 + bg 三态（增/删/换）+ 四项细节漂移。
    let mut caught = 0usize;
    let base = standard_profile();
    let base_hash = base.env_hash;
    // 1) hw_id 变更
    let mut p = base.clone();
    if p.set_scalar(F_HW, "VARIX-BASE-002").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 2) fw_ver 变更
    let mut p = base.clone();
    if p.set_scalar(F_FW, "unxreal-1.0.4").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 3) power_plan 变更
    let mut p = base.clone();
    if p.set_scalar(F_POWER, "performance").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 4) fs_layout 变更
    let mut p = base.clone();
    if p.set_scalar(F_FS, "varixfs|/|gpt:align2048").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 5) bg 增项
    let mut p = base.clone();
    if p.set_bg(&["varixd", "logd", "probe", "netd"]).is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 6) bg 删项
    let mut p = base.clone();
    if p.set_bg(&["varixd", "logd"]).is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 7) bg 换项
    let mut p = base.clone();
    if p.set_bg(&["varixd", "logd", "storaged"]).is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 8) bg 大小写变更（规范化保序但内容敏感）
    let mut p = base.clone();
    if p.set_bg(&["varixd", "logd", "Probe"]).is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 9) hw_id 大小写漂移
    let mut p = base.clone();
    if p.set_scalar(F_HW, "varix-base-001").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 10) fs_layout 尾缀漂移
    let mut p = base.clone();
    if p.set_scalar(F_FS, "varixfs|/|gpt:align4096|ro").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 11) fw_ver 段位漂移
    let mut p = base.clone();
    if p.set_scalar(F_FW, "unxreal-1.1.3").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    // 12) power_plan 附加档位后缀
    let mut p = base.clone();
    if p.set_scalar(F_POWER, "balanced-eco").is_ok() && p.env_hash != base_hash {
        caught += 1;
    }
    (caught, 12)
}

/// 拒收注入核：构造期非法输入逐类必抓（5 类）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知字段（越闭集）
    let mut p = EnvProfile::new();
    if p.set_scalar(9, "x") == Err(ENV_UNK_FIELD) {
        caught += 1;
    }
    // 2) 标量空串
    if p.set_scalar(F_HW, "") == Err(ENV_EMPTY) {
        caught += 1;
    }
    // 3) background_set 空集
    if p.set_bg(&[]).err() == Some(ENV_BG_EMPTY) {
        caught += 1;
    }
    // 4) background_set 空白项
    if p.set_bg(&["varixd", "  "]).err() == Some(ENV_BG_BLANK) {
        caught += 1;
    }
    // 5) 封账后改写
    p = standard_profile();
    p.seal();
    if p.set_scalar(F_FW, "unxreal-9.9.9") == Err(ENV_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 渲染导出（envctl 导出面第 1 波组成部分的可读面，逐字段一行）。
pub fn render_profile(p: &EnvProfile) -> String {
    let mut out = String::new();
    out.push_str(&format!("hw_id={}\n", p.hw_id));
    out.push_str(&format!("fw_ver={}\n", p.fw_ver));
    out.push_str(&format!("power_plan={}\n", p.power_plan));
    out.push_str(&format!("fs_layout={}\n", p.fs_layout));
    out.push_str(&format!("background_set={}\n", p.background_set.join(",")));
    out.push_str(&format!("env_hash={:08x}\n", p.env_hash));
    out
}

/// 渲染完整性核：6 行（五字段 + 哈希行）且键齐全。
pub fn render_complete(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != F_COUNT + 1 {
        return false;
    }
    let mut keys_ok = true;
    for (i, ln) in lines.iter().enumerate() {
        if i < F_COUNT {
            if !ln.starts_with(FIELD_NAMES[i]) {
                keys_ok = false;
            }
        } else if !ln.starts_with("env_hash=") {
            keys_ok = false;
        }
    }
    keys_ok && text.contains("env_hash=")
}

/// 判据入口（CheckSet）。
pub fn run_unx56002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56002");
    // J1-1 schema 字段全落（标量四项 + bg + 哈希非零）
    let p = standard_profile();
    let ok = p.schema_complete() && p.background_set.len() == 3;
    s.add("56002-J1-schema字段全落", ok, if ok { "" } else { "schema 字段缺失" });
    // J1-2 哈希幂等 ×3 轮（同环境三次重算零差异）
    let mut p2 = standard_profile();
    let h1 = p2.recompute_hash();
    let h2 = p2.recompute_hash();
    let h3 = p2.recompute_hash();
    let ok = h1 == h2 && h2 == h3 && h1 == p2.env_hash;
    s.add("56002-J1-哈希幂等三轮", ok, if ok { "" } else { "哈希幂等破" });
    // J1-3 同构环境哈希一致（两份同字段 profile 零差异）
    let ok = standard_profile().env_hash == standard_profile().env_hash;
    s.add("56002-J1-同构哈希一致", ok, if ok { "" } else { "同构哈希漂移" });
    // J2-1 ×12 字段变更注入全检出
    let (c, n) = mutation_core();
    let ok = c == n && n == 12;
    let detail = if ok { String::new() } else { format!("变更检出 {c}/{n}") };
    s.add_dyn("56002-J2-字段变更哈希必变", ok, &detail);
    // J2-2 bg 乱序输入哈希一致（规范化保幂等）
    let mut pa = standard_profile();
    let mut pb = standard_profile();
    let _ = pa.set_bg(&["probe", "logd", "varixd"]);
    let _ = pb.set_bg(&["varixd", "probe", "logd"]);
    let ok = pa.env_hash == pb.env_hash;
    s.add("56002-J2-乱序规范化等价", ok, if ok { "" } else { "乱序哈希漂移" });
    // J2-3 bg 重复项折叠等哈希
    let mut pc = standard_profile();
    let _ = pc.set_bg(&["varixd", "logd", "probe", "logd", "varixd"]);
    let ok = pc.env_hash == standard_profile().env_hash && pc.background_set.len() == 3;
    s.add("56002-J2-重复项折叠", ok, if ok { "" } else { "去重语义漂移" });
    // J3-1 拒收注入 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("拒收检出 {ic}/{in_}") };
    s.add_dyn("56002-J3-拒收注入必抓", ok, &detail);
    // J3-2 封账只读（封后哈希冻结）
    let mut pd = standard_profile();
    let frozen = pd.env_hash;
    pd.seal();
    let ok = pd.set_scalar(F_FW, "unxreal-2.0.0").is_err() && pd.env_hash == frozen;
    s.add("56002-J3-封账只读", ok, if ok { "" } else { "封账后哈希被改" });
    // J3-3 渲染导出面齐（6 行键全）
    let text = render_profile(&standard_profile());
    let ok = render_complete(&text) && text.starts_with("hw_id=VARIX-BASE-001");
    s.add("56002-J3-渲染导出面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-4 bg 空集拒收与空白项拒收（schema 纪律）
    let mut pe = EnvProfile::new();
    let e1 = pe.set_bg(&[]).err();
    let e2 = pe.set_bg(&["varixd", " "]).err();
    let ok = e1 == Some(ENV_BG_EMPTY) && e2 == Some(ENV_BG_BLANK);
    s.add("56002-J1-bg输入纪律", ok, if ok { "" } else { "bg 输入纪律松" });
    // J1-5 未知字段拒收（闭集守闸）
    let ok = EnvProfile::new().set_scalar(7, "x") == Err(ENV_UNK_FIELD);
    s.add("56002-J1-未知字段拒收", ok, if ok { "" } else { "闭集守闸松" });
    // J1-6 哈希非零哨兵（零值视为未计算）
    let ok = standard_profile().env_hash != 0;
    s.add("56002-J1-哈希非零哨兵", ok, if ok { "" } else { "哈希退化零" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56002_all_checks_green() {
        let set = run_unx56002_checks();
        assert!(set.all_passed(), "UNX-F56002 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56002_hash_idempotent() {
        let mut p = standard_profile();
        let a = p.recompute_hash();
        let b = p.recompute_hash();
        assert_eq!(a, b);
    }
}
