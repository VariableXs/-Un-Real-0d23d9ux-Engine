//! F031 深化批次三 · 残留扫描启发式面（compatstar2/deep2 · G-A-31）。
//!
//! 批次一/批次二已覆盖 F031 的协议语义面与执行治理面；本批补主册
//! 【功能定义】「全语义对齐」的执行/边界/注入面：路径模式匹配器（`*`
//! 通配 + 前缀/后缀匹配，定长模式表 16，区分大小写策略开关）、注册表
//! 键路径五类归属分类器（软件自身键/共享运行时键/用户文档键/系统键
//! 禁扫/未知——按键路径段数与已知前缀表归类）、三证据置信度评分模型
//! （路径证据/时间证据/引用计数加权求和 0-100，确删/待审/放过三分档）、
//! 清扫白名单闸（命中白名单一律放过且记账，零越权删除）。
//!
//! 判据对账：深化以主册【设计细节】「清单缺失时目录树清扫排除用户明确
//! 放入文件（时间戳早于安装时刻的文件列外并说明）」、【状态与异常】
//! 「清扫记录审计条目」未落地面为源，一处一事实（MS 卸载清单与
//! SharedDLLs 引用计数残留判定语义对拍）。
//!
//! 零堆纪律：定长模式表/前缀表/白名单表，无 alloc。

use crate::checks::CheckSet;

// 常量（一处一事实）
/// 残留判定模式表容量（域内口径：定长 16）。
pub const MAX_PATTERNS: usize = 16;
/// 键路径段数下限（自身键判定：HKCU\Software\厂商\产品 ≥ 4 段）。
pub const OWN_MIN_SEGS: usize = 4;
/// 置信度满分（三证据加权求和口径）。
pub const SCORE_MAX: u32 = 100;
/// 路径证据权重。
pub const W_PATH: u32 = 45;
/// 时间证据权重（主册【设计细节】：时间戳早于安装时刻的文件列外）。
pub const W_TIME: u32 = 35;
/// 引用计数权重（MS SharedDLLs 语义：被引用即不可删）。
pub const W_REFCOUNT: u32 = 20;
/// 三分档阈值：≥80 确删 / ≥40 待审（其余放过）。
pub const THRESH_DELETE: u32 = 80;
pub const THRESH_REVIEW: u32 = 40;

/// 残留判定模式表（定长 16；`*` 通配；区分大小写策略由调用侧开关决定）。
pub const RESIDUE_PATTERNS: [&str; MAX_PATTERNS] = [
    "*\\*.tmp", "*\\*.log", "*\\cache\\*", "*\\Cache\\*",
    "*\\temp\\*", "*\\Temp\\*", "*\\uninstall.exe", "*\\uninstall.exe.manifest",
    "*\\*.dll.bak", "*\\settings.ini.bak", "*\\crash\\*", "*\\CrashDumps\\*",
    "*\\thumbcache_*", "*\\*.old", "*\\Update\\*", "*\\logs\\*",
];

/// 清扫白名单（命中一律放过——零越权红线；MS 已知共享组件键语义）。
const WHITELIST: [&str; 4] = [
    "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SharedDLLs*",
    "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall*",
    "HKLM\\SYSTEM\\CurrentControlSet*",
    "*\\Shell Folders*",
];

/// 注册表键路径已知前缀表（顺序即优先级；MS 根键语义一处一事实）。
const KEY_PREFIX_TABLE: [(&str, KeyClass); 6] = [
    ("HKLM\\SYSTEM", KeyClass::System),
    ("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders", KeyClass::UserDoc),
    ("HKLM\\SOFTWARE\\Microsoft", KeyClass::SharedRuntime),
    ("HKCU\\Software\\Microsoft", KeyClass::SharedRuntime),
    ("HKLM\\SOFTWARE", KeyClass::Own),
    ("HKCU\\Software", KeyClass::Own),
];

// 路径模式匹配器
fn eq_byte(a: u8, b: u8, case_sensitive: bool) -> bool {
    if case_sensitive { a == b } else { a.to_ascii_lowercase() == b.to_ascii_lowercase() }
}

/// `*` 通配匹配器：`*` 匹配任意长（含空），其余逐字节比对。
/// 前缀模式 `varix*`、后缀模式 `*.tmp`、中段模式 `*\cache\*` 同一面承载。
pub fn wildcard_match(pattern: &str, path: &str, case_sensitive: bool) -> bool {
    let p = pattern.as_bytes();
    let s = path.as_bytes();
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_s = 0usize;
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'*' || eq_byte(p[pi], s[si], case_sensitive)) {
            if p[pi] == b'*' { star = Some(pi); star_s = si; pi += 1; } else { pi += 1; si += 1; }
        } else if let Some(sp) = star {
            // 回溯：`*` 多吞一个字符重试（经典单星回溯算法）。
            pi = sp + 1;
            star_s += 1;
            si = star_s;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// 残留模式表命中计数（启发式记账面：命中数随路径返回，不落判）。
pub fn residue_hits(path: &str, case_sensitive: bool) -> u32 {
    let mut n = 0u32;
    for p in RESIDUE_PATTERNS.iter() {
        if wildcard_match(p, path, case_sensitive) {
            n += 1;
        }
    }
    n
}

// 注册表键路径五类归属分类器
/// 键归属五类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyClass {
    /// 软件自身键——清单驱动的正常清扫对象。
    Own,
    /// 共享运行时键——须引用计数裁决，不直接删。
    SharedRuntime,
    /// 用户文档键——用户明确放入，勾选外不清。
    UserDoc,
    /// 系统键——禁扫红线（命中即放过并记账）。
    System,
    /// 未知——降级待审。
    Unknown,
}

/// 键路径段数（`\` 分段）。
pub fn key_segment_count(path: &str) -> usize {
    path.split('\\').count()
}

/// 五类归属判定：前缀表优先命中 + 自身键段数下限（不足降级未知）。
pub fn classify_key(path: &str) -> KeyClass {
    for (prefix, class) in KEY_PREFIX_TABLE.iter() {
        if path.starts_with(prefix) {
            let short_own = *class == KeyClass::Own && key_segment_count(path) < OWN_MIN_SEGS;
            return if short_own { KeyClass::Unknown } else { *class };
        }
    }
    KeyClass::Unknown
}

// 置信度评分模型（三证据加权 0-100 三分档）
/// 裁决三分档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 确删。
    Delete,
    /// 待审。
    Review,
    /// 放过。
    Spare,
}

/// 三证据加权求和：路径 45 / 时间 35 / 引用计数 20（引用 1 次折半、
/// ≥2 次归零——MS SharedDLLs 引用计数语义）；阈值三分档。
pub fn score(path_evidence: bool, time_evidence: bool, refcount: u32) -> (u32, Verdict) {
    let mut s = 0u32;
    if path_evidence { s += W_PATH; }
    if time_evidence { s += W_TIME; }
    s += match refcount { 0 => W_REFCOUNT, 1 => 10, _ => 0 };
    let v = if s >= THRESH_DELETE { Verdict::Delete }
        else if s >= THRESH_REVIEW { Verdict::Review }
        else { Verdict::Spare };
    (s, v)
}

// 清扫白名单闸（记账面：零越权）
/// 清扫闸：候选路径先过白名单，再分类，再评分裁决；每步如实记账。
pub struct SweepGate {
    /// 白名单命中放过计数。
    pub spared_by_whitelist: u32,
    /// 系统键（禁扫）检出计数——越权未遂账。
    pub forbidden_hits: u32,
    /// 确删计数。
    pub deleted: u32,
    /// 待审计数。
    pub review: u32,
    /// 放过计数（分类面）。
    pub spared: u32,
}

impl SweepGate {
    pub const fn new() -> Self {
        SweepGate { spared_by_whitelist: 0, forbidden_hits: 0, deleted: 0, review: 0, spared: 0 }
    }

    /// 单候选裁决：白名单 → 分类 → 评分。全链显性，不静默吞。
    pub fn judge(&mut self, path: &str, path_ev: bool, time_ev: bool, refcount: u32) -> Verdict {
        for w in WHITELIST.iter() {
            if wildcard_match(w, path, false) {
                self.spared_by_whitelist += 1;
                return Verdict::Spare;
            }
        }
        match classify_key(path) {
            KeyClass::System => { self.forbidden_hits += 1; Verdict::Spare }
            KeyClass::UserDoc => { self.spared += 1; Verdict::Spare }
            _ => {
                let (_, v) = score(path_ev, time_ev, refcount);
                match v {
                    Verdict::Delete => self.deleted += 1,
                    Verdict::Review => self.review += 1,
                    Verdict::Spare => self.spared += 1,
                }
                v
            }
        }
    }
}

// 域自检（深化批次三）
pub fn run_f031e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F031-residual-d3");
    // 1) `*` 通配：前缀/后缀/中段三形态 + 空匹配 + 大小写开关。
    cs.add(
        "wildcard_forms",
        wildcard_match("varix*", "varix-engine", true)
            && wildcard_match("*.tmp", "a.TMP", false)
            && !wildcard_match("*.tmp", "a.TMP", true)
            && wildcard_match("*\\cache\\*", "C:\\p\\cache\\f.bin", true)
            && wildcard_match("*", "", true),
        "",
    );
    // 2) 回溯：单星吞多字符后仍命中；无星模式全等判定。
    cs.add(
        "wildcard_backtrack",
        wildcard_match("*ab", "aab", true)
            && wildcard_match("a*b*c", "aXbYc", true)
            && !wildcard_match("a*", "ba", true)
            && !wildcard_match("a", "ab", true),
        "",
    );
    // 3) 模式表命中计数：缓存目录 + 临时后缀双模式合流；开关生效。
    cs.add(
        "residue_pattern_hits",
        residue_hits("C:\\prog\\cache\\f.tmp", false) >= 2
            && residue_hits("C:\\prog\\Cache\\f.TMP", false) >= 2
            && residue_hits("C:\\prog\\Cache\\f.TMP", true) == 1
            && residue_hits("C:\\prog\\bin\\f.bin", false) == 0,
        "",
    );
    // 4) 分类器：系统键禁扫红线。
    cs.add(
        "classify_system",
        classify_key("HKLM\\SYSTEM\\CurrentControlSet\\Services\\x") == KeyClass::System,
        "",
    );
    // 5) 分类器：自身键段数下限（4 段成键，3 段降级未知）；共享运行时键。
    cs.add(
        "classify_own_shared",
        classify_key("HKCU\\Software\\VarixStar\\App") == KeyClass::Own
            && classify_key("HKCU\\Software\\VarixStar") == KeyClass::Unknown
            && classify_key("HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\x")
                == KeyClass::SharedRuntime,
        "",
    );
    // 6) 分类器：用户文档键优先于共享运行时键（前缀表序即优先级）。
    cs.add(
        "classify_userdoc_priority",
        classify_key("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders")
            == KeyClass::UserDoc,
        "",
    );
    // 7) 评分：三证据满分确删；弱证据待审；零证据放过。
    cs.add(
        "score_tiers",
        score(true, true, 0) == (100, Verdict::Delete)
            && score(true, false, 1) == (55, Verdict::Review)
            && score(false, false, 2) == (0, Verdict::Spare),
        "",
    );
    // 8) 评分边界：恰在阈值线上（90 确删 / 10 放过 / 45 待审 / 35 放过）。
    cs.add(
        "score_threshold_edges",
        score(true, true, 1) == (90, Verdict::Delete)
            && score(false, false, 1) == (10, Verdict::Spare)
            && score(true, false, 2) == (45, Verdict::Review)
            && score(false, true, 2) == (35, Verdict::Spare),
        "",
    );
    // 9) 白名单闸：SharedDLLs 命中放过并记账（哪怕证据满分）。
    let mut g = SweepGate::new();
    let v1 = g.judge("HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SharedDLLs\\a.dll", true, true, 0);
    cs.add("whitelist_gate_spare", v1 == Verdict::Spare && g.spared_by_whitelist == 1 && g.deleted == 0, "");
    // 10) 禁扫红线：白名单未覆盖的系统键不进评分链，越权未遂记账。
    let v2 = g.judge("HKLM\\SYSTEM\\Setup\\x", true, true, 0);
    cs.add("gate_system_forbidden", v2 == Verdict::Spare && g.forbidden_hits == 1 && g.deleted == 0, "");
    // 11) 闸账自洽：确删/待审/放过分类分流，总数对账。
    let _ = g.judge("HKCU\\Software\\VarixStar\\App\\cache", true, true, 0);
    let _ = g.judge("HKCU\\Software\\VarixStar\\App\\weak", true, false, 1);
    let _ = g.judge("HKCU\\Software\\UnknownV", false, false, 2);
    cs.add("gate_ledger_consistent", g.deleted == 1 && g.review == 1 && g.spared == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_star_semantics() {
        // `*` 匹配空串与跨段任意串；无星模式必须全等。
        assert!(wildcard_match("", "", true));
        assert!(wildcard_match("*\\*", "x\\y", true));
        assert!(!wildcard_match("*\\a", "x\\b", true));
        assert!(wildcard_match("A*Z", "aMMMMz", false));
        assert!(!wildcard_match("A*Z", "aMMMMz", true));
    }

    #[test]
    fn refcount_evidence_drops() {
        // 引用计数证据：0→20、1→10、≥2→0（MS SharedDLLs 语义）。
        assert_eq!(score(true, true, 0).0, 100);
        assert_eq!(score(true, true, 1).0, 90);
        assert_eq!(score(true, true, 255).0, 80);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f031e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
