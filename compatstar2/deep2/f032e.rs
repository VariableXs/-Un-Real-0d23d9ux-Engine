//! F032 深化批次三 · 包版本解析面（compatstar2/deep2 · G-A-32）。
//!
//! 批次一/批次二已覆盖 F032 的协议语义面与执行治理面；本批补主册
//! 【功能定义】「全语义对齐」的执行/边界/注入面：版本比较器（点分数字
//! 主序 + 预发布序 alpha<beta<rc<正式，len 比较逐段定长 8 段）、pep440
//! 兼容子集（`~=1.4.2` 的展开模型）、依赖解析回退链（多镜像优先级表
//! 定长 8，逐级尝试，失败降级记账，全失败显性 Err）、锁定文件条目对拍
//! 账（name@version:hash 三元组定长 32 条，缺失/不匹配/多余三分类计数）、
//! PATH 注入去重模型（注入前查重、重复计数、定长 16 槽）。
//!
//! 判据对账：深化以主册【设计细节】「版本并存激活机制：会话内显式切换
//! （显式无魔法）」、【状态与异常】「包管理器下载失败 → 三要素错误 +
//! 重试」未落地面为源，一处一事实（PEP 440 版本序与兼容释放语义、
//! npm/pip 锁定文件完整性语义对拍）。
//!
//! 零堆纪律：定长版本段/镜像表/锁定账/注入槽，无 alloc。

use crate::checks::CheckSet;

// 常量（一处一事实）

/// 版本段数上限（len 比较口径：定长 8 段，不足补零对齐）。
pub const VERSION_SEGS: usize = 8;
/// 预发布序表（PEP 440：alpha < beta < rc < 正式释放）。
pub const PRERELEASE_ORDER: [&str; 3] = ["alpha", "beta", "rc"];
/// 多镜像优先级表容量（域内口径：定长 8）。
pub const MAX_MIRRORS: usize = 8;
/// 锁定文件条目容量（域内口径：定长 32）。
pub const MAX_LOCK_ENTRIES: usize = 32;
/// PATH 注入槽容量（F011 会话级注入的域内口径：定长 16）。
pub const MAX_PATH_SLOTS: usize = 16;

// 版本比较器

/// 预发布序 rank：正式(空 tag)=3 最高，rc=2，beta=1，alpha/未知=0。
pub fn prerelease_rank(tag: &str) -> u32 {
    match tag { "" => 3, "rc" => 2, "beta" => 1, _ => 0 }
}

/// 点分版本解析：`1.4.2-rc1` → （定长 8 段数字 + 预发布 tag）。
/// tag 按 PEP 440 预发布词前缀归一（rc1/rc2 都归 rc 档）。
pub fn parse_version(v: &str) -> ([u32; VERSION_SEGS], &'static str) {
    let (num_part, raw_tag) = match v.find('-') {
        Some(i) => (&v[..i], &v[i + 1..]),
        None => (v, ""),
    };
    let mut segs = [0u32; VERSION_SEGS];
    let mut idx = 0usize;
    for part in num_part.split('.') {
        if idx >= VERSION_SEGS { break; } // 超长截断——len 口径如实降级
        segs[idx] = part.parse::<u32>().unwrap_or(0);
        idx += 1;
    }
    let tag: &'static str = if raw_tag.starts_with("alpha") { "alpha" }
        else if raw_tag.starts_with("beta") { "beta" }
        else if raw_tag.starts_with("rc") { "rc" } else { "" };
    (segs, tag)
}

/// 版本比较：点分数字主序（逐段定长 8），数字全等再比预发布序。
pub fn compare_version(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    let (sa, ta) = parse_version(a);
    let (sb, tb) = parse_version(b);
    for i in 0..VERSION_SEGS {
        let o = sa[i].cmp(&sb[i]);
        if o != Ordering::Equal { return o; }
    }
    prerelease_rank(ta).cmp(&prerelease_rank(tb))
}

// pep440 兼容子集（`~=X.Y.Z` 展开模型）

/// `~=X.Y.Z` → （下界含端 X.Y.Z，上界去尾进位排他）。
/// PEP 440：`~=1.4.2` ≡ `>=1.4.2, ==1.4.*`；`~=1.4` ≡ `>=1.4, ==1.*`。
pub fn compatible_bounds(spec: &str) -> ([u32; VERSION_SEGS], [u32; VERSION_SEGS]) {
    let num = match spec.find("~=") { Some(i) => &spec[i + 2..], None => spec };
    let lower = parse_version(num).0;
    let mut upper = lower;
    // 规格实际组件数（'.' 分段；上界按规格组件数对倒数第二段进位）：
    // ~=1.4.2 → 上界 1.5.0（==1.4.*）；~=1.4 → 上界 2.0.0（==1.*）。
    let mut comps = num.split('.').count();
    if comps == 0 || comps > VERSION_SEGS { comps = VERSION_SEGS; }
    let bump = if comps >= 2 { comps - 2 } else { 0 };
    upper[bump] += 1;
    let mut j = bump + 1;
    while j < VERSION_SEGS {
        upper[j] = 0;
        j += 1;
    }
    (lower, upper)
}

/// 区间判定：lower <= v < upper（点分数组字典序）。
pub fn compatible_contains(bounds: (&[u32; VERSION_SEGS], &[u32; VERSION_SEGS]), v: &str) -> bool {
    let segs = parse_version(v).0;
    segs >= *bounds.0 && segs < *bounds.1
}

// 依赖解析回退链（多镜像优先级表）

/// 镜像回退链：优先级表定长 8，逐级尝试；不可达镜像失败降级记账；
/// 全失败显性 Err——零静默吞错。
pub struct MirrorChain {
    mirrors: [&'static str; MAX_MIRRORS],
    pub mirror_count: usize,
    /// 逐镜像失败计数（降级账）；失败总数见 total_failures。
    pub per_mirror_failures: [u32; MAX_MIRRORS],
    pub total_failures: u32,
}

impl MirrorChain {
    pub const fn new() -> Self {
        MirrorChain { mirrors: [""; MAX_MIRRORS], mirror_count: 0, per_mirror_failures: [0; MAX_MIRRORS], total_failures: 0 }
    }

    /// 登记镜像（下标即优先级；表满如实拒绝）。
    pub fn add_mirror(&mut self, url: &'static str) -> bool {
        if self.mirror_count >= MAX_MIRRORS { return false; }
        self.mirrors[self.mirror_count] = url;
        self.mirror_count += 1;
        true
    }

    /// 解析：不可达镜像（`bad:` 前缀——确定性判据）逐级降级；
    /// 全失败 → Err（三要素错误语义的回退面出口）。
    pub fn resolve(&mut self) -> Result<&'static str, &'static str> {
        for i in 0..self.mirror_count {
            let m = self.mirrors[i];
            if m.starts_with("bad:") {
                self.per_mirror_failures[i] += 1;
                self.total_failures += 1;
                continue;
            }
            return Ok(m);
        }
        Err("all-mirrors-failed")
    }
}

// 锁定文件条目对拍账

/// 锁定条目：name@version:hash 三元组。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LockEntry { pub name: &'static str, pub version: &'static str, pub hash: u32 }

/// 对拍账：缺失/不匹配/多余三分类计数（完整性审计面）。
pub struct LockAudit {
    expected: [Option<LockEntry>; MAX_LOCK_ENTRIES],
    expected_n: usize,
    pub missing: u32,
    pub mismatched: u32,
    pub extra: u32,
}

fn find_entry(table: &[Option<LockEntry>; MAX_LOCK_ENTRIES], name: &str) -> Option<LockEntry> {
    for slot in table.iter().flatten() {
        if slot.name == name {
            return Some(*slot);
        }
    }
    None
}

impl LockAudit {
    pub const fn new() -> Self {
        LockAudit { expected: [None; MAX_LOCK_ENTRIES], expected_n: 0, missing: 0, mismatched: 0, extra: 0 }
    }

    /// 登记期望条目（表满如实拒绝）。
    pub fn expect(&mut self, e: LockEntry) -> bool {
        if self.expected_n >= MAX_LOCK_ENTRIES { return false; }
        self.expected[self.expected_n] = Some(e);
        self.expected_n += 1;
        true
    }

    /// 对拍：期望侧逐条找实际（缺→missing，版本或哈希不符→mismatched）；
    /// 实际侧反查期望（无主→extra）。三分类独立计数。
    pub fn audit(&mut self, actual: &[Option<LockEntry>; MAX_LOCK_ENTRIES]) {
        for i in 0..self.expected_n {
            if let Some(e) = self.expected[i] {
                match find_entry(actual, e.name) {
                    None => self.missing += 1,
                    Some(a) => if a.version != e.version || a.hash != e.hash { self.mismatched += 1 },
                }
            }
        }
        for slot in actual.iter().flatten() {
            if find_entry(&self.expected, slot.name).is_none() {
                self.extra += 1;
            }
        }
    }
}

// PATH 注入去重模型（F011 会话级注入的边界面）

/// 注入槽：注入前查重、重复计数、容量外溢出计数——零重复注入。
pub struct PathInjector {
    slots: [&'static str; MAX_PATH_SLOTS],
    /// 已注入槽位。
    pub injected: usize,
    /// 重复注入检出计数。
    pub duplicates: u32,
    /// 容量溢出检出计数。
    pub overflow: u32,
}

impl PathInjector {
    pub const fn new() -> Self {
        PathInjector { slots: [""; MAX_PATH_SLOTS], injected: 0, duplicates: 0, overflow: 0 }
    }

    /// 注入一个目录：命中已注入 → 记重复并拒绝；槽满 → 记溢出并拒绝。
    pub fn inject(&mut self, dir: &'static str) -> bool {
        for i in 0..self.injected {
            if self.slots[i] == dir { self.duplicates += 1; return false; }
        }
        if self.injected >= MAX_PATH_SLOTS { self.overflow += 1; return false; }
        self.slots[self.injected] = dir;
        self.injected += 1;
        true
    }
}

/// 判例注入清单（互异目录定长 15：填满剩余槽位用，一处一事实）。
const JUDGE_DIRS: [&str; MAX_PATH_SLOTS - 1] = [
    "toolchains\\node\\20", "toolchains\\java\\21", "toolchains\\go\\1.22",
    "toolchains\\rust\\stable", "toolchains\\python\\3.11", "tools\\git\\bin",
    "tools\\cmake\\bin", "tools\\ninja", "tools\\make\\bin",
    "cache\\pip\\scripts", "cache\\npm\\scripts", "cache\\cargo\\bin",
    "session\\venv\\scripts", "session\\path-extra-a", "session\\path-extra-b",
];

// 域自检（深化批次三）

pub fn run_f032e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F032-version-d3");
    // 1) 主序按数字不按字典序：1.10.0 > 1.9.9。
    cs.add("version_numeric_order",
        compare_version("1.10.0", "1.9.9") == core::cmp::Ordering::Greater
            && compare_version("1.4.2", "1.10.0") == core::cmp::Ordering::Less, "");
    // 2) len 对齐：补零段全等；同串相等。
    cs.add("version_pad_equal",
        compare_version("1.4", "1.4.0") == core::cmp::Ordering::Equal
            && compare_version("1.4.2", "1.4.2") == core::cmp::Ordering::Equal, "");
    // 3) 预发布序：alpha < beta < rc < 正式；rc1 归 rc 档。
    cs.add("prerelease_order",
        compare_version("1.0.0-alpha", "1.0.0-beta") == core::cmp::Ordering::Less
            && compare_version("1.0.0-beta", "1.0.0-rc") == core::cmp::Ordering::Less
            && compare_version("1.0.0-rc1", "1.0.0") == core::cmp::Ordering::Less
            && compare_version("1.0.0", "1.0.0-alpha") == core::cmp::Ordering::Greater, "");
    // 4) pep440：~=1.4.2 → >=1.4.2 且 <1.5.0（==1.4.*）。
    let (lo, hi) = compatible_bounds("~=1.4.2");
    cs.add("pep440_three_seg",
        lo[0] == 1 && lo[1] == 4 && lo[2] == 2 && hi[0] == 1 && hi[1] == 5 && hi[2] == 0, "");
    // 5) pep440 区间判定：界内收、界外拒（含下界含端、上界排他）。
    cs.add("pep440_contains",
        compatible_contains((&lo, &hi), "1.4.2") && compatible_contains((&lo, &hi), "1.4.9")
            && !compatible_contains((&lo, &hi), "1.5.0") && !compatible_contains((&lo, &hi), "1.4.1"), "");
    // 6) pep440 两段式：~=1.4 → 上界 2.0.0（==1.*）。
    let (lo2, hi2) = compatible_bounds("~=1.4");
    cs.add("pep440_two_seg",
        hi2[0] == 2 && hi2[1] == 0 && compatible_contains((&lo2, &hi2), "1.9.9")
            && !compatible_contains((&lo2, &hi2), "2.0.0"), "");
    // 7) 镜像回退：首镜像不可达 → 降级次镜像，失败逐级记账。
    let mut mc = MirrorChain::new();
    let _ = mc.add_mirror("bad:mirror-a");
    let _ = mc.add_mirror("mirror-b");
    let got = mc.resolve();
    cs.add("mirror_fallback",
        got == Ok("mirror-b") && mc.per_mirror_failures[0] == 1 && mc.total_failures == 1, "");
    // 8) 镜像全失败：显性 Err（三要素错误语义），账面如实。
    let mut mc2 = MirrorChain::new();
    let _ = mc2.add_mirror("bad:a");
    let _ = mc2.add_mirror("bad:b");
    let _ = mc2.add_mirror("bad:c");
    cs.add("mirror_all_fail_err", mc2.resolve() == Err("all-mirrors-failed") && mc2.total_failures == 3, "");
    // 9) 锁定对拍：缺失/哈希不匹配/多余三分类各计一笔；全对账面归零。
    let mut la = LockAudit::new();
    let _ = la.expect(LockEntry { name: "numpy", version: "1.4.2", hash: 0xAAAA });
    let _ = la.expect(LockEntry { name: "requests", version: "2.0.0", hash: 0xBBBB });
    let _ = la.expect(LockEntry { name: "flask", version: "3.0.0", hash: 0xCCCC });
    let mut actual: [Option<LockEntry>; MAX_LOCK_ENTRIES] = [None; MAX_LOCK_ENTRIES];
    actual[0] = Some(LockEntry { name: "numpy", version: "1.4.2", hash: 0xAAAA });
    actual[1] = Some(LockEntry { name: "requests", version: "2.0.0", hash: 0xDEAD });
    actual[2] = Some(LockEntry { name: "rogue", version: "9.9.9", hash: 0x1 });
    la.audit(&actual);
    cs.add("lock_audit_classes", la.missing == 1 && la.mismatched == 1 && la.extra == 1, "");
    // 10) PATH 注入：重复拒绝记账；容量外溢出记账；成功注入计数如实。
    let mut pin = PathInjector::new();
    let first = pin.inject("toolchains\\python\\3.12");
    let dup = pin.inject("toolchains\\python\\3.12");
    let mut filled = true;
    for d in JUDGE_DIRS.iter() {
        filled = filled && pin.inject(d);
    }
    let over = pin.inject("one-too-many");
    cs.add("path_inject_dedup",
        first && !dup && pin.duplicates == 1 && filled && !over
            && pin.overflow == 1 && pin.injected == MAX_PATH_SLOTS, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_normalizes_and_truncates() {
        // rc1/rc2 归 rc 档；无 tag 为正式；超 8 段按 len 口径截断。
        assert_eq!(parse_version("2.0.0-rc2").1, "rc");
        assert_eq!(parse_version("3.1.0-beta7").1, "beta");
        assert_eq!(parse_version("1.0.0").1, "");
        let (segs, _) = parse_version("1.2.3.4.5.6.7.8.9");
        assert_eq!(segs[7], 8);
    }

    #[test]
    fn lock_audit_clean_is_silent() {
        // 全对账面三分类归零——审计只对差异出声。
        let mut la = LockAudit::new();
        let _ = la.expect(LockEntry { name: "numpy", version: "1.4.2", hash: 7 });
        let _ = la.expect(LockEntry { name: "requests", version: "2.0.0", hash: 9 });
        let mut actual: [Option<LockEntry>; MAX_LOCK_ENTRIES] = [None; MAX_LOCK_ENTRIES];
        actual[0] = Some(LockEntry { name: "requests", version: "2.0.0", hash: 9 });
        actual[1] = Some(LockEntry { name: "numpy", version: "1.4.2", hash: 7 });
        la.audit(&actual);
        assert_eq!((la.missing, la.mismatched, la.extra), (0, 0, 0));
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f032e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
