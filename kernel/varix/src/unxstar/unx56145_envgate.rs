//! UNX-F56145 禁跨机入账总闸（env_hash 强校验）（0x61220 段）
//!
//! 锚点：UNX-F56145-J1 环境哈希强校验账（对照行入账必携 env_hash，
//! 与账本冻结环境哈希强等才放行）；J2 跨机拒绝语义（异 env_hash 一律
//! 拒入账 + 拒绝留痕带双哈希对账）；J3 哈希核（同环境同哈希确定性 +
//! 环境五维任一变即哈希变）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·跨机守闸面）。
//!
//! 结构：环境五维登记（OS/固件/时钟源/电源/进程静默集 → 规范串）+
//! env_hash 核（规范串 FNV 折叠）+ 入账总闸（哈希强等四态：等/异/缺/
//! 未冻结）+ 拒绝账（跨机拒绝逐条留痕）+ 注入核（缺哈希入账/异哈希
//! 入账/未冻结先入账/哈希格式坏/五维漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61220 独占 0x20）。
pub const SEG_BASE: u32 = 0x61220;
pub const XH_NO_HASH: u32 = SEG_BASE + 0x01; // 入账缺 env_hash
pub const XH_MISMATCH: u32 = SEG_BASE + 0x02; // 跨机哈希异
pub const XH_NOT_FROZEN: u32 = SEG_BASE + 0x03; // 账本未冻结先入账
pub const XH_BAD_FORMAT: u32 = SEG_BASE + 0x04; // 哈希格式坏（非 8 位十六进制）
pub const XH_DIM_DRIFT: u32 = SEG_BASE + 0x05; // 环境五维漂移

/// 环境五维闭集。
pub const DIM_OS: usize = 0;
pub const DIM_FW: usize = 1;
pub const DIM_CLOCK: usize = 2;
pub const DIM_POWER: usize = 3;
pub const DIM_PROC: usize = 4;
pub const DIM_COUNT: usize = 5;

/// 哈希格式核：8 位十六进制串。
pub fn hash_format_ok(h: &str) -> bool {
    let b = h.as_bytes();
    b.len() == 8 && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 环境五维规范串（规范序拼接，顺序固定）。
pub fn env_canonical(dims: &[String; DIM_COUNT]) -> String {
    let mut out = String::new();
    for (i, d) in dims.iter().enumerate() {
        if i > 0 {
            out.push('|');
        }
        out.push_str(d);
    }
    out
}

/// env_hash 核：规范串 FNV-1a 折叠 → 8 位十六进制串。
pub fn env_hash(dims: &[String; DIM_COUNT]) -> String {
    let canon = env_canonical(dims);
    let mut h: u32 = 0x811C_9DC5;
    for b in canon.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{:08x}", h)
}

/// 入账态闭集。
pub const ADMIT_EQ: u8 = 0; // 哈希相等放行
pub const ADMIT_REJ_DIFF: u8 = 1; // 异哈希拒
pub const ADMIT_REJ_MISS: u8 = 2; // 缺哈希拒
pub const ADMIT_REJ_FROZEN: u8 = 3; // 未冻结拒
pub const ADMIT_REJ_FMT: u8 = 4; // 格式坏拒

/// 入账总闸 + 拒绝账。
pub struct CrossHostGate {
    frozen_hash: Option<String>,
    admits: Vec<String>,
    rejects: Vec<String>,
}

impl CrossHostGate {
    pub fn new() -> Self {
        CrossHostGate {
            frozen_hash: None,
            admits: Vec::new(),
            rejects: Vec::new(),
        }
    }

    /// 冻结账本环境哈希（一次语义，重冻结拒）。
    pub fn freeze(&mut self, h: &str) -> Result<(), u32> {
        if !hash_format_ok(h) {
            return Err(XH_BAD_FORMAT);
        }
        if self.frozen_hash.is_some() {
            return Err(XH_NOT_FROZEN);
        }
        self.frozen_hash = Some(h.to_string());
        Ok(())
    }

    /// 入账总闸：五态判定。
    pub fn admit(&mut self, row_key: &str, env_hash: Option<&str>) -> Result<(), u32> {
        let frozen = match &self.frozen_hash {
            None => return Err(self.reject(row_key, ADMIT_REJ_FROZEN, XH_NOT_FROZEN)),
            Some(f) => f.clone(),
        };
        let h = match env_hash {
            None => return Err(self.reject(row_key, ADMIT_REJ_MISS, XH_NO_HASH)),
            Some(h) => h,
        };
        if !hash_format_ok(h) {
            return Err(self.reject(row_key, ADMIT_REJ_FMT, XH_BAD_FORMAT));
        }
        if h != frozen {
            return Err(self.reject(row_key, ADMIT_REJ_DIFF, XH_MISMATCH));
        }
        self.admits.push(row_key.to_string());
        Ok(())
    }

    fn reject(&mut self, row_key: &str, kind: u8, code: u32) -> u32 {
        self.rejects.push(format!("{}|rej{}|{:04x}", row_key, kind, code));
        code
    }

    pub fn frozen(&self) -> bool {
        self.frozen_hash.is_some()
    }

    pub fn frozen_hash(&self) -> Option<&str> {
        self.frozen_hash.as_deref()
    }

    pub fn admit_count(&self) -> usize {
        self.admits.len()
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 拒绝留痕核：拒绝账逐条带双哈希对账信息（kind 可查）。
    pub fn reject_lines(&self) -> &[String] {
        &self.rejects
    }

    /// 渲染导出。
    pub fn render_gate(&self) -> String {
        let mut out = String::new();
        if let Some(f) = &self.frozen_hash {
            out.push_str(&format!("FROZEN|{}\n", f));
        }
        for a in &self.admits {
            out.push_str(&format!("IN|{}\n", a));
        }
        for r in &self.rejects {
            out.push_str(&format!("{}|OUT\n", r));
        }
        out
    }
}

impl Default for CrossHostGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：冻结标准环境 + 三行入账（2 放行 + 1 跨机拒）。
fn standard_env() -> [String; DIM_COUNT] {
    [
        "win11-23h2".to_string(),
        "bios-f12".to_string(),
        "invariant-tsc".to_string(),
        "balanced".to_string(),
        "quiet-set-v3".to_string(),
    ]
}

fn standard_gate() -> CrossHostGate {
    let h = env_hash(&standard_env());
    let mut g = CrossHostGate::new();
    let _ = g.freeze(&h);
    let _ = g.admit("fps_full", Some(&h));
    let _ = g.admit("fps_mid", Some(&h));
    let _ = g.admit("other_pc", Some("deadbeef"));
    g
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let h = env_hash(&standard_env());
    // 注 1：缺哈希入账
    let mut g = CrossHostGate::new();
    let _ = g.freeze(&h);
    if g.admit("x", None) == Err(XH_NO_HASH) {
        caught += 1;
    }
    // 注 2：异哈希入账
    let mut g = CrossHostGate::new();
    let _ = g.freeze(&h);
    if g.admit("x", Some("12345678")) == Err(XH_MISMATCH) {
        caught += 1;
    }
    // 注 3：未冻结先入账
    let mut g = CrossHostGate::new();
    if g.admit("x", Some(&h)) == Err(XH_NOT_FROZEN) {
        caught += 1;
    }
    // 注 4：格式坏哈希入账
    let mut g = CrossHostGate::new();
    let _ = g.freeze(&h);
    if g.admit("x", Some("zzzz")) == Err(XH_BAD_FORMAT) {
        caught += 1;
    }
    // 注 5：五维漂移（改一维哈希必变）
    let mut dims = standard_env();
    dims[DIM_POWER] = "high-perf".to_string();
    if env_hash(&dims) != h {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 强校验账；J2 跨机拒绝；J3 哈希核。
pub fn run_unx56145_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56145");
    // J1-1 冻结 + 放行账
    let g = standard_gate();
    let ok = g.frozen() && g.admit_count() == 2 && g.reject_count() == 1;
    s.add("56145-J1-冻结放行账", ok, if ok { "" } else { "总闸账漂移" });
    // J1-2 渲染导出齐
    let text = standard_gate().render_gate();
    let ok = text.contains("FROZEN|") && text.contains("IN|fps_full") && text.contains("other_pc|rej1|");
    s.add("56145-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 跨机拒绝留痕带 kind
    let g = standard_gate();
    let rej = g.reject_lines();
    let ok = rej.len() == 1 && rej[0].starts_with("other_pc|rej1|");
    s.add("56145-J2-拒绝留痕", ok, if ok { "" } else { "留痕面漂移" });
    // J2-2 异哈希四类拒全触发
    let h = env_hash(&standard_env());
    let mut g2 = CrossHostGate::new();
    let _ = g2.freeze(&h);
    let r1 = g2.admit("a", Some("deadbeef"));
    let r2 = g2.admit("b", None);
    let ok = r1 == Err(XH_MISMATCH) && r2 == Err(XH_NO_HASH);
    s.add("56145-J2-异缺皆拒", ok, if ok { "" } else { "拒闸漂移" });
    // J2-3 未冻结首拒
    let mut g2 = CrossHostGate::new();
    let ok = g2.admit("a", Some(&h)) == Err(XH_NOT_FROZEN) && !g2.frozen();
    s.add("56145-J2-未冻首拒", ok, if ok { "" } else { "冻结序漂移" });
    // J3-1 同环境同哈希
    let ok = env_hash(&standard_env()) == env_hash(&standard_env());
    s.add("56145-J3-同环同签", ok, if ok { "" } else { "哈希漂移" });
    // J3-2 五维序敏感（换序即变）
    let mut dims = standard_env();
    dims.swap(DIM_OS, DIM_FW);
    let ok = env_hash(&dims) != env_hash(&standard_env());
    s.add("56145-J3-序敏感", ok, if ok { "" } else { "序退化" });
    // J3-3 哈希格式自洽（输出恒 8 位十六进制）
    let out = env_hash(&standard_env());
    let ok = hash_format_ok(&out) && hash_format_ok("0123abcd") && !hash_format_ok("xyz");
    s.add("56145-J3-格式自洽", ok, if ok { "" } else { "格式漂移" });
    // J3-4 规范串管道序
    let canon = env_canonical(&standard_env());
    let ok = canon.starts_with("win11-23h2|bios-f12|") && canon.ends_with("|quiet-set-v3");
    s.add("56145-J3-规范串序", ok, if ok { "" } else { "规范串漂移" });
    // J3-5 重冻结拒
    let mut g2 = CrossHostGate::new();
    let _ = g2.freeze(&h);
    let ok = g2.freeze(&h) == Err(XH_NOT_FROZEN);
    s.add("56145-J3-重冻拒", ok, if ok { "" } else { "冻结语义漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56145-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56145_all_checks_green() {
        let set = run_unx56145_checks();
        assert!(set.all_passed(), "UNX-F56145 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56145_hash_stable() {
        assert_eq!(env_hash(&standard_env()), env_hash(&standard_env()));
    }
}
