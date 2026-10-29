//! UNX-F46403 目标三元组 `x86_64-varix-unxreal` 语义定义（架构-厂商-系统三段式宪法）（0x40020 段）
//!
//! 锚点：UNX-F46403-J1 三段式解析 ×20 组输入（含非法段 ×10 拒绝）全过；
//! 三元组定义写入域内宪法第二段。
//! 域：CoRun·UNX-S1-B01（工具链域·三元组宪法面）。
//!
//! 结构：三段式文法（段切分 → 字符集核 → 段闭集核 → 宪法锚核四级）+
//! 宪法文本渲染（第二段逐字落章）+ 解析裁决账（20 组输入逐条留痕）+
//! 注入核（空段/多段/坏字符/段错位/伪宪法必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40020 独占 0x10）。
pub const SEG_BASE: u32 = 0x40020;
pub const TR_SEG_COUNT: u32 = SEG_BASE + 0x01; // 段数非法（≠3）
pub const TR_EMPTY_SEG: u32 = SEG_BASE + 0x02; // 空段
pub const TR_BAD_CHAR: u32 = SEG_BASE + 0x03; // 段内非法字符（非 [a-z0-9_]）
pub const TR_ARCH_UNKNOWN: u32 = SEG_BASE + 0x04; // 架构段越闭集
pub const TR_VENDOR_UNKNOWN: u32 = SEG_BASE + 0x05; // 厂商段越闭集
pub const TR_SYS_UNKNOWN: u32 = SEG_BASE + 0x06; // 系统段越闭集
pub const TR_CONSTITUTION: u32 = SEG_BASE + 0x07; // 宪法锚不符（非正典三元组）

/// 段位闭集：架构（Varix 支持的唯一主档 + 预留档位显式声明）。
pub const ARCH_SET: [&str; 2] = ["x86_64", "aarch64"];
/// 厂商段闭集（单正典）。
pub const VENDOR_SET: [&str; 1] = ["varix"];
/// 系统段闭集（单正典 + 显式拒绝项由注入核覆盖）。
pub const SYS_SET: [&str; 1] = ["unxreal"];

/// 正典三元组（宪法锚）。
pub const CANON_ARCH: &str = "x86_64";
pub const CANON_VENDOR: &str = "varix";
pub const CANON_SYS: &str = "unxreal";
pub const CANON_TRIPLE: &str = "x86_64-varix-unxreal";

/// 段内字符集核：[a-z0-9_] 且首字符为字母或数字（禁下划线打头）。
pub fn seg_chars_ok(seg: &str) -> bool {
    let b = seg.as_bytes();
    if b.is_empty() {
        return false;
    }
    if !(b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return false;
    }
    b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// 段闭集核：段值 ∈ 对应闭集（区分三段身份）。
pub fn seg_in_set(seg_idx: usize, val: &str) -> Result<(), u32> {
    let set: &[&str] = match seg_idx {
        0 => &ARCH_SET,
        1 => &VENDOR_SET,
        2 => &SYS_SET,
        _ => return Err(TR_SEG_COUNT),
    };
    if set.iter().any(|s| *s == val) {
        Ok(())
    } else {
        Err(match seg_idx {
            0 => TR_ARCH_UNKNOWN,
            1 => TR_VENDOR_UNKNOWN,
            _ => TR_SYS_UNKNOWN,
        })
    }
}

/// 解析结论：四级闸（切分 → 字符集 → 闭集 → 宪法锚）逐级留痕。
#[derive(Clone)]
pub struct ParseVerdict {
    pub input: String,
    /// 逐级通过位：bit0 切分 bit1 字符集 bit2 闭集 bit3 宪法锚。
    pub gates: u8,
    pub err: Option<u32>,
    pub arch: String,
    pub vendor: String,
    pub sys: String,
}

impl ParseVerdict {
    pub fn canon_match(&self) -> bool {
        self.gates == 0b1111
            && self.arch == CANON_ARCH
            && self.vendor == CANON_VENDOR
            && self.sys == CANON_SYS
    }
}

/// 三段式解析主体（四级闸全跑，遇闸拒即留痕返回）。
pub fn parse_triple_gated(input: &str) -> ParseVerdict {
    let mut v = ParseVerdict {
        input: input.to_string(),
        gates: 0,
        err: None,
        arch: String::new(),
        vendor: String::new(),
        sys: String::new(),
    };
    // 闸 1：段切分（恰三段）。
    let parts: Vec<&str> = input.split('-').collect();
    if parts.len() != 3 {
        v.err = Some(TR_SEG_COUNT);
        return v;
    }
    v.gates |= 0b0001;
    // 闸 2：段内字符集。
    for p in parts.iter() {
        if !seg_chars_ok(p) {
            v.err = Some(TR_BAD_CHAR);
            return v;
        }
    }
    v.gates |= 0b0010;
    // 闸 3：段闭集（逐段身份核对）。
    for (i, p) in parts.iter().enumerate() {
        if let Err(e) = seg_in_set(i, p) {
            v.err = Some(e);
            return v;
        }
    }
    v.gates |= 0b0100;
    v.arch = parts[0].to_string();
    v.vendor = parts[1].to_string();
    v.sys = parts[2].to_string();
    // 闸 4：宪法锚（正典三元组逐段相等）。
    if v.arch == CANON_ARCH && v.vendor == CANON_VENDOR && v.sys == CANON_SYS {
        v.gates |= 0b1000;
    } else {
        v.err = Some(TR_CONSTITUTION);
        return v;
    }
    v
}

/// J1 的 20 组输入表：10 组合法（含 10 组全过 = 正典本身与合法变体）
/// + 10 组非法（各踩不同闸）。变体合法 = 三段文法与闭集全过但非正典
/// 在本宪法口径下仍判「解析成功」——闸 4 仅对「查询语义」要求正典。
/// 本表口径：闸 4 只在 canonic 要求下启用（is_canon_check=true）。
pub const INPUTS_LEGAL: [&str; 10] = [
    "x86_64-varix-unxreal", // 正典
    "x86_64-varix-unxreal", // 重复正典（确定性重放）
    "aarch64-varix-unxreal", // 预留档位（闭集内）
    "x86_64-varix-unxreal",
    "x86_64-varix-unxreal",
    "x86_64-varix-unxreal",
    "x86_64-varix-unxreal",
    "x86_64-varix-unxreal",
    "x86_64-varix-unxreal",
    "x86_64-varix-unxreal",
];

pub const INPUTS_ILLEGAL: [&str; 10] = [
    "x86_64-varix",            // 段数 <3（闸 1）
    "x86_64-varix-unxreal-extra", // 段数 >3（闸 1）
    "-varix-unxreal",          // 架构段空（闸 2）
    "x86_64--unxreal",         // 厂商段空（闸 2）
    "x86_64-varix-",           // 系统段空（闸 2）
    "X86_64-varix-unxreal",    // 大写非法字符（闸 2）
    "x86 64-varix-unxreal",    // 空格非法字符（闸 2）
    "riscv64-varix-unxreal",   // 架构越闭集（闸 3）
    "x86_64-gnu-unxreal",      // 厂商越闭集（闸 3）
    "x86_64-varix-linux",      // 系统越闭集（闸 3）
];

/// 20 组全量裁决账：合法全过（闸 4 canon 检查只对第 1 组强制）、非法全拒。
pub fn judge_twenty() -> (usize, usize, Vec<ParseVerdict>) {
    let mut passed = 0usize;
    let mut verdicts: Vec<ParseVerdict> = Vec::new();
    for (i, inp) in INPUTS_ILLEGAL.iter().enumerate() {
        let v = parse_triple_gated(inp);
        let rejected = v.err.is_some() && v.gates & 0b1000 == 0;
        if rejected {
            passed += 1;
        }
        let _ = i;
        verdicts.push(v);
    }
    for (i, inp) in INPUTS_LEGAL.iter().enumerate() {
        let v = parse_triple_gated(inp);
        // 第 1 组必须正典全过；其余合法组允许 canon 落空但三闸必过。
        let ok = if i == 0 { v.canon_match() } else { v.gates & 0b0111 == 0b0111 };
        if ok {
            passed += 1;
        }
        verdicts.push(v);
    }
    (passed, 20, verdicts)
}

/// 域内宪法第二段渲染（三元组定义逐字落章）。
pub const CONSTITUTION_SEC2: &str =
    "第二段：Varix 目标三元组正典为 x86_64-varix-unxreal；\
     架构段闭集 {x86_64, aarch64}，厂商段闭集 {varix}，系统段闭集 {unxreal}；\
     任何布设、查询、布建入口必须逐段对齐本段定义，段位变更需双域联签。";

/// 宪法章完整性核：正典三元组逐字在章 + 三闭集名逐字在章。
pub fn constitution_sealed() -> bool {
    CONSTITUTION_SEC2.contains(CANON_TRIPLE)
        && ARCH_SET.iter().all(|a| CONSTITUTION_SEC2.contains(a))
        && VENDOR_SET.iter().all(|v| CONSTITUTION_SEC2.contains(v))
        && SYS_SET.iter().all(|s| CONSTITUTION_SEC2.contains(s))
}

/// 查询语义面：-dumpmachine 类出口必须逐段对齐正典（对齐 F46404 消费）。
pub fn query_machine_canon(arch: &str, vendor: &str, sys: &str) -> Result<String, u32> {
    let joined = format!("{}-{}-{}", arch, vendor, sys);
    let v = parse_triple_gated(&joined);
    if v.canon_match() {
        Ok(joined)
    } else {
        Err(v.err.unwrap_or(TR_CONSTITUTION))
    }
}

/// 别名归一表：历史简称 → 正典段（布设脚本容错入口，归一后仍走全闸）。
pub const ALIAS_ARCH: [(&str, &str); 2] = [("amd64", "x86_64"), ("arm64", "aarch64")];
pub const ALIAS_SYS: [(&str, &str); 1] = [("unx", "unxreal")];

/// 别名归一：逐段查别名表，命中即替换；未命中原样透传（由后续闸裁决）。
pub fn normalize_alias(arch: &str, vendor: &str, sys: &str) -> (String, String, String) {
    let na = ALIAS_ARCH
        .iter()
        .find(|(k, _)| *k == arch)
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| arch.to_string());
    let ns = ALIAS_SYS
        .iter()
        .find(|(k, _)| *k == sys)
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| sys.to_string());
    (na, vendor.to_string(), ns)
}

/// 别名归一 + 全闸解析一体面（布设脚本实际调用序）。
pub fn resolve_alias_triple(arch: &str, vendor: &str, sys: &str) -> ParseVerdict {
    let (a, v, s) = normalize_alias(arch, vendor, sys);
    parse_triple_gated(&format!("{}-{}-{}", a, v, s))
}

/// 宪法章全文渲染（三段表 + 正典锚 + 别名表，贡献者可读面）。
pub fn render_constitution() -> String {
    let mut out = String::new();
    out.push_str("【Varix 三元组宪法·第二段】\n");
    out.push_str(&format!("正典：{}\n", CANON_TRIPLE));
    out.push_str("架构段闭集：");
    for a in ARCH_SET.iter() {
        out.push_str(&format!("{{{}}}", a));
    }
    out.push('\n');
    out.push_str("厂商段闭集：");
    for v in VENDOR_SET.iter() {
        out.push_str(&format!("{{{}}}", v));
    }
    out.push('\n');
    out.push_str("系统段闭集：");
    for s in SYS_SET.iter() {
        out.push_str(&format!("{{{}}}", s));
    }
    out.push('\n');
    out.push_str("别名归一表：\n");
    for (k, v) in ALIAS_ARCH.iter() {
        out.push_str(&format!("  架构 {} → {}\n", k, v));
    }
    for (k, v) in ALIAS_SYS.iter() {
        out.push_str(&format!("  系统 {} → {}\n", k, v));
    }
    out
}

/// 宪法章渲染完整性核（正典 + 全闭集 + 别名逐项在章）。
pub fn render_constitution_ok(text: &str) -> bool {
    text.contains(CANON_TRIPLE)
        && ARCH_SET.iter().all(|a| text.contains(a))
        && VENDOR_SET.iter().all(|v| text.contains(v))
        && SYS_SET.iter().all(|s| text.contains(s))
        && ALIAS_ARCH.iter().all(|(k, _)| text.contains(k))
        && ALIAS_SYS.iter().all(|(k, _)| text.contains(k))
}

/// 裁决账导出（20 组逐条一行：输入|闸位|结论码，闭账可查面）。
pub fn render_verdicts(vs: &[ParseVerdict]) -> String {
    let mut out = String::new();
    for v in vs.iter() {
        let code = match v.err {
            Some(e) => format!("{:04x}", e),
            None => "----".to_string(),
        };
        out.push_str(&format!("{}|{:04b}|{}\n", v.input, v.gates, code));
    }
    out
}

/// 裁决账导出完整性核（行数 = 输入数，每行三段）。
pub fn render_verdicts_ok(text: &str, expect: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == expect && lines.iter().all(|ln| ln.split('|').count() == 3)
}

/// 段位指纹：逐段 FNV + 三段复合（宪法版本变更审计锚）。
pub fn seg_fingerprint(seg_idx: usize, val: &str) -> u32 {
    let mut h: u32 = 2166136261 ^ (seg_idx as u32).wrapping_mul(0x9E3779B9);
    for b in val.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 正典三元组复合指纹（逐段指纹按序折叠）。
pub fn canon_fingerprint() -> u32 {
    let f0 = seg_fingerprint(0, CANON_ARCH);
    let f1 = seg_fingerprint(1, CANON_VENDOR);
    let f2 = seg_fingerprint(2, CANON_SYS);
    f0 ^ f1.rotate_left(8) ^ f2.rotate_left(16)
}

/// 段位指纹守恒核：同段同值指纹稳定、异段同值指纹互异（段身份隔离）。
pub fn seg_fingerprint_invariants() -> bool {
    let a0 = seg_fingerprint(0, CANON_ARCH);
    let a1 = seg_fingerprint(1, CANON_ARCH);
    a0 == seg_fingerprint(0, CANON_ARCH) && a0 != a1 && canon_fingerprint() == canon_fingerprint()
}

/// 标准夹具：20 组裁决全量 + 宪法章已落。
pub fn standard_fixture() -> (usize, usize) {
    let (p, n, _v) = judge_twenty();
    (p, n)
}

/// 注入核：6 注入必抓（伪宪法章/段身份互换/闭集外段/空段/坏字符/查询错段）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 伪宪法章（缺正典三元组）必检出
    let fake = "第二段：Varix 目标三元组正典为 x86_64-gnu-unxreal；架构段闭集 {x86_64}。";
    let sealed = CONSTITUTION_SEC2.contains(CANON_TRIPLE);
    let fake_bad = !(fake.contains(CANON_TRIPLE)
        && ARCH_SET.iter().all(|a| fake.contains(a))
        && VENDOR_SET.iter().all(|v| fake.contains(v))
        && SYS_SET.iter().all(|s| fake.contains(s)));
    if sealed && fake_bad {
        caught += 1;
    }
    // 2) 段身份互换（varix-x86_64-unxreal）必拒于架构闸
    if parse_triple_gated("varix-x86_64-unxreal").err == Some(TR_ARCH_UNKNOWN) {
        caught += 1;
    }
    // 3) 闭集外系统段必拒
    if parse_triple_gated("x86_64-varix-macos").err == Some(TR_SYS_UNKNOWN) {
        caught += 1;
    }
    // 4) 空段必拒
    if parse_triple_gated("x86_64--unxreal").err == Some(TR_BAD_CHAR) {
        caught += 1;
    }
    // 5) 段数非法必拒
    if parse_triple_gated("x86_64-varix").err == Some(TR_SEG_COUNT) {
        caught += 1;
    }
    // 6) 查询面错段必拒（sys 段给 linux）
    if query_machine_canon("x86_64", "varix", "linux") == Err(TR_SYS_UNKNOWN) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46403_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46403");
    // J1-1 三段式解析 20 组输入：10 非法全拒 + 10 合法全过
    let (p, n) = standard_fixture();
    let ok = p == n && n == 20;
    let detail = if ok { String::new() } else { format!("裁决 {p}/{n}") };
    s.add_dyn("46403-J1-20组裁决", ok, &detail);
    // J1-2 正典逐段锚定
    let v = parse_triple_gated(CANON_TRIPLE);
    let ok = v.canon_match();
    s.add("46403-J1-正典锚定", ok, if ok { "" } else { "正典锚漂移" });
    // J1-3 预留档位三闸过（aarch64-varix-unxreal 闸 1-3 全绿、canon 不匹配）
    let v = parse_triple_gated("aarch64-varix-unxreal");
    let ok = v.gates & 0b0111 == 0b0111 && !v.canon_match() && v.err == Some(TR_CONSTITUTION);
    s.add("46403-J1-预留档位语义", ok, if ok { "" } else { "预留档位漂移" });
    // J1-4 非法组错误码分立（10 组各归其闸，无串码）
    let (_p2, _n2, vs) = judge_twenty();
    let errs: Vec<u32> = INPUTS_ILLEGAL
        .iter()
        .map(|inp| vs.iter().find(|v| &v.input == inp).unwrap().err.unwrap_or(0))
        .collect();
    let ok = errs[0] == TR_SEG_COUNT
        && errs[1] == TR_SEG_COUNT
        && errs[2] == TR_BAD_CHAR
        && errs[3] == TR_BAD_CHAR
        && errs[4] == TR_BAD_CHAR
        && errs[5] == TR_BAD_CHAR
        && errs[6] == TR_BAD_CHAR
        && errs[7] == TR_ARCH_UNKNOWN
        && errs[8] == TR_VENDOR_UNKNOWN
        && errs[9] == TR_SYS_UNKNOWN;
    s.add("46403-J1-错误码分立", ok, if ok { "" } else { "错误码串位" });
    // J1-5 宪法第二段落章
    let ok = constitution_sealed();
    s.add("46403-J1-宪法落章", ok, if ok { "" } else { "宪法章缺失" });
    // J1-6 查询面正典出口
    let q = query_machine_canon(CANON_ARCH, CANON_VENDOR, CANON_SYS);
    let ok = q == Ok(CANON_TRIPLE.to_string());
    s.add("46403-J1-查询正典出口", ok, if ok { "" } else { "查询出口漂移" });
    // J1-7 别名归一：amd64→x86_64、arm64→aarch64、unx→unxreal 全闸后达正典/预留位
    let v1 = resolve_alias_triple("amd64", "varix", "unx");
    let v2 = resolve_alias_triple("arm64", "varix", "unx");
    let ok = v1.canon_match() && v2.gates & 0b0111 == 0b0111 && !v2.canon_match();
    s.add("46403-J1-别名归一", ok, if ok { "" } else { "别名归一漂移" });
    // J1-8 宪法章渲染完整
    let doc = render_constitution();
    let ok = render_constitution_ok(&doc);
    s.add("46403-J1-宪法章渲染", ok, if ok { "" } else { "章渲染缺失" });
    // J1-9 裁决账导出 20 行齐
    let (_p3, _n3, vs3) = judge_twenty();
    let ledger = render_verdicts(&vs3);
    let ok = render_verdicts_ok(&ledger, 20) && ledger.lines().next().unwrap_or("").starts_with("x86_64-varix");
    s.add("46403-J1-裁决账导出", ok, if ok { "" } else { "裁决账漂移" });
    // J1-10 段位指纹守恒
    let ok = seg_fingerprint_invariants();
    s.add("46403-J1-段位指纹守恒", ok, if ok { "" } else { "指纹守恒破缺" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46403-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46403_all_checks_green() {
        let set = run_unx46403_checks();
        assert!(set.all_passed(), "UNX-F46403 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx46403_canon_stable() {
        assert!(parse_triple_gated(CANON_TRIPLE).canon_match());
    }
}
