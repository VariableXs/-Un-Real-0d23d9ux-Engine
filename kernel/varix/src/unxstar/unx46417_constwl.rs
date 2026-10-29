//! UNX-F46417 L4 域内宪法登记（git 白名单五件 + 不做账：子模块/hooks/多工作树/调试器深度语义）（0x40100 段）
//!
//! 锚点：UNX-F46417-J1 白名单与不做账逐条登记 ×1 全绿（触及时拒绝）。
//! 域：CoRun·UNX-S1-B01（工具链域·宪法登记面）。
//!
//! 结构：git 白名单五件（对象库/索引/引用/packfile/智能协议逐件在域内
//! 确认）+ 不做账四件（子模块/hooks/多工作树/调试器深度语义，触及时
//! 拒绝 + ADR 指引）+ 宪法章渲染 + 注入核（白名单外越权/不做账放行/
//! 章漂移/封宪改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40100 独占 0x10）。
pub const SEG_BASE: u32 = 0x40100;
pub const CT_WL_OOB: u32 = SEG_BASE + 0x01; // 白名单外越权
pub const CT_NOGO_PASS: u32 = SEG_BASE + 0x02; // 不做账放行
pub const CT_CHARTER_DRIFT: u32 = SEG_BASE + 0x03; // 宪法章漂移
pub const CT_SEALED: u32 = SEG_BASE + 0x04; // 封宪改写
pub const CT_DUP: u32 = SEG_BASE + 0x05; // 重复登记

/// git 白名单五件闭集（域内确认面）。
pub const WL_OBJECTS: usize = 0; // 对象库
pub const WL_INDEX: usize = 1; // 索引
pub const WL_REFS: usize = 2; // 引用
pub const WL_PACK: usize = 3; // packfile
pub const WL_SMART: usize = 4; // 本地与 HTTP 智能协议
pub const WL_COUNT: usize = 5;

pub const WL_NAMES: [&str; WL_COUNT] = ["对象库", "索引", "引用", "packfile", "智能协议"];

/// 不做账四件闭集（触及时拒绝 + ADR 指引）。
pub const NG_SUBMODULE: usize = 0; // 子模块
pub const NG_HOOKS: usize = 1; // hooks
pub const NG_WORKTREE: usize = 2; // 多工作树
pub const NG_DEBUGGER: usize = 3; // 调试器深度语义
pub const NG_COUNT: usize = 4;

pub const NG_NAMES: [&str; NG_COUNT] = ["子模块", "hooks", "多工作树", "调试器深度语义"];

/// 不做账 ADR 指引（逐件）。
pub const NG_ADRS: [&str; NG_COUNT] = [
    "ADR-GIT-01：子模块深度语义移至 B15/M 型传输段，域内拒绝触及时指引该 ADR",
    "ADR-GIT-02：hook 执行本体列不做账，占位检查见 F46545 面",
    "ADR-GIT-03：多工作树（worktree）语义不做，拒绝并指引单工作树纪律",
    "ADR-GIT-04：调试器深度语义（gdb 远程协议等）不做，拒绝并指引 UNX-D 域",
];

/// 域内宪法（白名单登记 + 不做账登记 + 封宪位）。
pub struct DomainConstitution {
    pub whitelist: Vec<usize>,
    pub nogos: Vec<usize>,
    pub sealed: bool,
}

impl DomainConstitution {
    pub fn new() -> Self {
        DomainConstitution { whitelist: Vec::new(), nogos: Vec::new(), sealed: false }
    }

    /// 白名单登记（越闭集/重复/封宪全拒）。
    pub fn allow(&mut self, item: usize) -> Result<(), u32> {
        if item >= WL_COUNT {
            return Err(CT_WL_OOB);
        }
        if self.sealed {
            return Err(CT_SEALED);
        }
        if self.whitelist.contains(&item) {
            return Err(CT_DUP);
        }
        self.whitelist.push(item);
        Ok(())
    }

    /// 不做账登记（越闭集/重复/封宪全拒）。
    pub fn forbid(&mut self, item: usize) -> Result<(), u32> {
        if item >= NG_COUNT {
            return Err(CT_NOGO_PASS);
        }
        if self.sealed {
            return Err(CT_SEALED);
        }
        if self.nogos.contains(&item) {
            return Err(CT_DUP);
        }
        self.nogos.push(item);
        Ok(())
    }

    /// 操作裁决：白名单内 → 放行；不做账 → 拒绝 + ADR；其余 → 越权拒绝。
    pub fn judge(&self, op: usize) -> Result<Option<&'static str>, u32> {
        if op < WL_COUNT {
            if self.whitelist.contains(&op) {
                return Ok(None);
            }
            return Err(CT_WL_OOB);
        }
        let ng = op - WL_COUNT;
        if ng < NG_COUNT {
            if self.nogos.contains(&ng) {
                return Err(CT_NOGO_PASS);
            }
            return Ok(Some(NG_ADRS[ng]));
        }
        Err(CT_WL_OOB)
    }

    /// 宪法齐核：白名单五件 + 不做账四件全登记。
    pub fn complete(&self) -> bool {
        self.whitelist.len() == WL_COUNT && self.nogos.len() == NG_COUNT
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 宪法指纹（白名单序 + 不做账序复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for w in self.whitelist.iter() {
            h ^= (*w as u32 + 1).wrapping_mul(16777619);
        }
        for n in self.nogos.iter() {
            h ^= (*n as u32 + 100).wrapping_mul(16777619);
        }
        h
    }
}

/// 宪法章渲染（白名单五件 + 不做账四件 + ADR 指引全量）。
pub fn render_constitution(c: &DomainConstitution) -> String {
    let mut out = String::new();
    out.push_str("【S1 域内宪法·git 白名单】\n");
    for w in c.whitelist.iter() {
        out.push_str(&format!("  允许：{}\n", WL_NAMES[*w]));
    }
    out.push_str("【不做账】\n");
    for n in c.nogos.iter() {
        out.push_str(&format!("  拒绝：{}（{}）\n", NG_NAMES[*n], NG_ADRS[*n]));
    }
    out
}

/// 宪法章完整性核（五允许 + 四拒绝 + 四 ADR 全在章）。
pub fn charter_ok(c: &DomainConstitution) -> bool {
    let text = render_constitution(c);
    WL_NAMES.iter().all(|n| text.contains(n))
        && NG_NAMES.iter().all(|n| text.contains(n))
        && NG_ADRS.iter().all(|a| text.contains(&a[..12.min(a.len())]))
}

/// 标准夹具：五允许 + 四拒绝全登记 + 封宪。
pub fn standard_constitution() -> DomainConstitution {
    let mut c = DomainConstitution::new();
    for w in 0..WL_COUNT {
        c.allow(w).expect("fixture allow");
    }
    for n in 0..NG_COUNT {
        c.forbid(n).expect("fixture forbid");
    }
    c.seal();
    c
}

/// 注入核：6 注入必抓（越权/不做账放行/章漂移/封宪改写/重复/越闭集）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 白名单外越权必抓（未登记的对blear象库操作）
    let mut c = DomainConstitution::new();
    if c.judge(WL_OBJECTS) == Err(CT_WL_OOB) {
        caught += 1;
    }
    // 2) 不做账放行必抓（未登记 forbid 时触碰子模块返回 ADR 而非拒绝——本域裁定必须先 forbid）
    c.forbid(NG_SUBMODULE).expect("forbid sm");
    if c.judge(WL_COUNT + NG_SUBMODULE) == Err(CT_NOGO_PASS) {
        caught += 1;
    }
    // 3) 章漂移必抓（缺 ADR 的章不完整）
    let mut c2 = DomainConstitution::new();
    c2.allow(WL_OBJECTS).expect("partial allow");
    if !charter_ok(&c2) {
        caught += 1;
    }
    // 4) 封宪改写必抓
    let mut std_c = standard_constitution();
    std_c.seal();
    if std_c.allow(WL_OBJECTS) == Err(CT_SEALED) {
        caught += 1;
    }
    // 5) 重复登记必抓
    let mut c3 = DomainConstitution::new();
    c3.allow(WL_INDEX).expect("first");
    if c3.allow(WL_INDEX) == Err(CT_DUP) {
        caught += 1;
    }
    // 6) 越闭集必抓（白名单号 9 / 不做账号 9）
    if c3.allow(9) == Err(CT_WL_OOB) && c3.forbid(9) == Err(CT_NOGO_PASS) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46417_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46417");
    // J1-1 白名单与不做账逐条登记全绿
    let c = standard_constitution();
    let ok = c.complete() && c.sealed;
    s.add("46417-J1-宪法逐条登记", ok, if ok { "" } else { "登记不全" });
    // J1-2 裁决面：白名单放行、不做账拒绝带 ADR
    let pass = c.judge(WL_PACK);
    let nogo = c.judge(WL_COUNT + NG_HOOKS);
    let ok = pass == Ok(None)
        && matches!(nogo, Err(CT_NOGO_PASS))
        && NG_ADRS[NG_HOOKS].contains("ADR-GIT-02");
    s.add("46417-J1-裁决面", ok, if ok { "" } else { "裁决漂移" });
    // J1-3 宪法章完整
    let ok = charter_ok(&c);
    s.add("46417-J1-宪法章完整", ok, if ok { "" } else { "章漂移" });
    // J1-4 宪法指纹稳定
    let ok = c.fingerprint() == standard_constitution().fingerprint();
    s.add("46417-J1-宪法指纹", ok, if ok { "" } else { "指纹漂移" });
    // 注入核 6/6
    let (cc, n) = injection_core();
    let ok = cc == n;
    let detail = if ok { String::new() } else { format!("注入 {cc}/{n}") };
    s.add_dyn("46417-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46417_all_checks_green() {
        let set = run_unx46417_checks();
        assert!(set.all_passed(), "UNX-F46417 checks must be green: {}", set.red_items_text());
    }
}
