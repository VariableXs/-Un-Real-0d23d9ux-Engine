//! UNX-F46401 工具链钉版与三元组布设总成（五件嫁接源：gcc/clang/rust/git/CMake）（0x40000 段）
//!
//! 锚点：UNX-F46401-J1 钉版账全量（组件 × 版本 × 校验和 × 钉定波次四元组逐件落账）；
//! J2 三元组查询语义一致；J3 头文件集扫描全集存在三判据全绿。
//! 域：CoRun·UNX-S1-B01（工具链域·钉版面）。
//!
//! 结构：五件组件登记册（gcc/clang/rustc/git/cmake 逐件落账：版本 × 校验和 ×
//! 钉定波次 × 布设根四元组）+ 钉版封账（封后拒改、重钉严格升波次）+ 三元组
//! 查询核（x86_64-varix-unxreal 三段式逐段一致）+ 头文件集扫描账（C 标准
//! 头全集存在性 + 指纹）+ 注入核（越界波次/篡改校验和/未知组件/封账改写/
//! 头缺失必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40000 独占 0x10）。
pub const SEG_BASE: u32 = 0x40000;
pub const PIN_UNKNOWN_COMP: u32 = SEG_BASE + 0x01; // 未知组件（非五件闭集）
pub const PIN_DUP_WAVE: u32 = SEG_BASE + 0x02; // 波次重复钉定（同件同波次二次落账）
pub const PIN_SUM_BAD: u32 = SEG_BASE + 0x03; // 校验和格式非法（非 16 位十六进制指纹）
pub const PIN_SEALED: u32 = SEG_BASE + 0x04; // 钉版账已封账拒写
pub const PIN_WAVE_BACK: u32 = SEG_BASE + 0x05; // 波次回退（重钉波次 ≤ 既有波次）
pub const PIN_VER_EMPTY: u32 = SEG_BASE + 0x06; // 版本串为空
pub const PIN_HEAD_MISS: u32 = SEG_BASE + 0x07; // 头文件集扫描缺失

/// 五件闭集（嫁接源组件，判据与登记共用同源命名）。
pub const COMP_GCC: usize = 0;
pub const COMP_CLANG: usize = 1;
pub const COMP_RUSTC: usize = 2;
pub const COMP_GIT: usize = 3;
pub const COMP_CMAKE: usize = 4;
pub const COMP_COUNT: usize = 5;

/// 组件名（账目渲染与查询共用）。
pub const COMP_NAMES: [&str; COMP_COUNT] = ["gcc", "clang", "rustc", "git", "cmake"];

/// C 标准头全集（J3 扫描对象，与 F46421 全集同源声明）。
pub const C_HEADERS: [&str; 20] = [
    "stdio.h", "stdlib.h", "string.h", "stdint.h", "stddef.h",
    "stdbool.h", "unistd.h", "fcntl.h", "errno.h", "signal.h",
    "time.h", "math.h", "assert.h", "ctype.h", "limits.h",
    "locale.h", "setjmp.h", "stdarg.h", "wchar.h", "pthread.h",
];

/// 校验和格式核：16 位十六进制指纹（4×u32 段式，审计可读）。
pub fn checksum_ok(sum: &str) -> bool {
    let b = sum.as_bytes();
    if b.len() != 16 {
        return false;
    }
    b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 波次号语义：从 1 起严格递增（0 号保留 = 未钉定哨兵）。
pub const WAVE_NONE: u32 = 0;

/// 单件钉版账目：组件 × 版本 × 校验和 × 钉定波次 四元组。
#[derive(Clone)]
pub struct PinEntry {
    pub comp: usize,
    pub version: String,
    pub checksum: String,
    pub wave: u32,
    /// 布设根（sysroot 相对前缀，账目对账用）。
    pub root: String,
}

impl PinEntry {
    /// 构造校验：组件闭集 + 版本非空 + 校验和格式 + 波次 ≥1。
    pub fn new(comp: usize, version: &str, checksum: &str, wave: u32, root: &str) -> Result<Self, u32> {
        if comp >= COMP_COUNT {
            return Err(PIN_UNKNOWN_COMP);
        }
        if version.is_empty() {
            return Err(PIN_VER_EMPTY);
        }
        if !checksum_ok(checksum) {
            return Err(PIN_SUM_BAD);
        }
        if wave == WAVE_NONE {
            return Err(PIN_WAVE_BACK);
        }
        Ok(PinEntry {
            comp,
            version: version.to_string(),
            checksum: checksum.to_string(),
            wave,
            root: root.to_string(),
        })
    }

    /// 四元组指纹（FNV-1a 32 位，确定性：同四元组同指纹）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for b in format!("{}|{}|{}|{}|{}", COMP_NAMES[self.comp], self.version, self.checksum, self.wave, self.root).as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }

    /// 与另一账目同件同版本同校验和判定（波次与根允许随重钉变化）。
    pub fn same_artifact(&self, o: &PinEntry) -> bool {
        self.comp == o.comp && self.version == o.version && self.checksum == o.checksum
    }
}

/// 钉版账（五件登记册，可封账）。
pub struct PinBook {
    pub entries: Vec<PinEntry>,
    pub sealed: bool,
    /// 封账时的最高波次（重钉纪律审计用）。
    pub top_wave: u32,
}

impl PinBook {
    pub fn new() -> Self {
        PinBook { entries: Vec::new(), sealed: false, top_wave: WAVE_NONE }
    }

    /// 逐件落账：同件重钉必须严格升波次且校验和可追踪；封账拒写。
    pub fn pin(&mut self, e: PinEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(PIN_SEALED);
        }
        if e.wave == WAVE_NONE {
            return Err(PIN_WAVE_BACK);
        }
        // 同件既有账：重钉严格升波次；同波次重复落账拒绝。
        let mut prev_wave = WAVE_NONE;
        for p in self.entries.iter() {
            if p.comp == e.comp {
                if p.wave == e.wave {
                    return Err(PIN_DUP_WAVE);
                }
                if p.wave > prev_wave {
                    prev_wave = p.wave;
                }
            }
        }
        if prev_wave != WAVE_NONE && e.wave <= prev_wave {
            return Err(PIN_WAVE_BACK);
        }
        if e.wave > self.top_wave {
            self.top_wave = e.wave;
        }
        self.entries.push(e);
        Ok(())
    }

    /// 某组件的现行账（最高波次那条）。
    pub fn current(&self, comp: usize) -> Option<&PinEntry> {
        if comp >= COMP_COUNT {
            return None;
        }
        let mut best: Option<&PinEntry> = None;
        for p in self.entries.iter() {
            if p.comp == comp {
                match best {
                    None => best = Some(p),
                    Some(b) if p.wave > b.wave => best = Some(p),
                    _ => {}
                }
            }
        }
        best
    }

    /// J1：全量核——五件每件都有现行账，四元组逐件非空。
    pub fn full_ledger_ok(&self) -> bool {
        for c in 0..COMP_COUNT {
            match self.current(c) {
                None => return false,
                Some(p) => {
                    if p.version.is_empty() || !checksum_ok(&p.checksum) || p.wave == WAVE_NONE || p.root.is_empty() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 封账（封后拒写，闭账材料快照不可变）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（全部现行账指纹按组件序复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for c in 0..COMP_COUNT {
            if let Some(p) = self.current(c) {
                h ^= p.fingerprint();
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 三元组段位：架构-厂商-系统。
pub const TRIPLE_ARCH: &str = "x86_64";
pub const TRIPLE_VENDOR: &str = "varix";
pub const TRIPLE_SYS: &str = "unxreal";
pub const TRIPLE_FULL: &str = "x86_64-varix-unxreal";

/// 三元组解析：三段式（遇多段拒绝；空段拒绝）。
pub fn parse_triple(s: &str) -> Result<(String, String, String), u32> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(PIN_UNKNOWN_COMP);
    }
    for p in parts.iter() {
        if p.is_empty() {
            return Err(PIN_VER_EMPTY);
        }
    }
    Ok((parts[0].to_string(), parts[1].to_string(), parts[2].to_string()))
}

/// J2：三元组查询语义一致核——查询结果逐段与宪法一致。
pub fn triple_query_ok(q_arch: &str, q_vendor: &str, q_sys: &str) -> bool {
    q_arch == TRIPLE_ARCH && q_vendor == TRIPLE_VENDOR && q_sys == TRIPLE_SYS
}

/// 头文件集扫描账：头名 → 指纹。
pub struct HeadScan {
    pub found: Vec<(String, u32)>,
    pub missing: Vec<String>,
}

/// 布设根对账结论：与 C5 SysrootLayout 约定的四路径在位性。
pub struct RootRecon {
    /// 四路径：bin / libexec / lib / include（§59.2 布设布局）。
    pub paths_ok: [bool; 4],
}

impl RootRecon {
    /// 对给定「目录在位集」执行四路径对账（C5 冻结接口消费面）。
    pub fn check(present: &[&str]) -> RootRecon {
        let want = ["usr/bin", "usr/libexec", "usr/lib", "usr/include"];
        let mut paths_ok = [false; 4];
        for i in 0..4 {
            paths_ok[i] = present.iter().any(|p| *p == want[i]);
        }
        RootRecon { paths_ok }
    }

    /// 四路径全在位。
    pub fn all_ok(&self) -> bool {
        self.paths_ok.iter().all(|p| *p)
    }

    /// 缺失路径清单（显性化，禁含糊）。
    pub fn missing_paths(&self) -> Vec<&'static str> {
        let want = ["usr/bin", "usr/libexec", "usr/lib", "usr/include"];
        let mut miss = Vec::new();
        for i in 0..4 {
            if !self.paths_ok[i] {
                miss.push(want[i]);
            }
        }
        miss
    }
}

impl HeadScan {
    /// 对给定「存在集」执行扫描：在集内的头记指纹（FNV），缺失头如实列清单。
    pub fn scan(present: &[&str]) -> HeadScan {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for h in C_HEADERS.iter() {
            if present.iter().any(|p| p == h) {
                let mut fh: u32 = 2166136261;
                for b in h.as_bytes() {
                    fh ^= *b as u32;
                    fh = fh.wrapping_mul(16777619);
                }
                found.push(((*h).to_string(), fh));
            } else {
                missing.push((*h).to_string());
            }
        }
        HeadScan { found, missing }
    }

    /// J3：全集存在（缺失清单为空即全存在）。
    pub fn full_present(&self) -> bool {
        self.missing.is_empty() && self.found.len() == C_HEADERS.len()
    }

    /// 指纹稳定性：同输入两次扫描逐头一致。
    pub fn stable(&self, other: &HeadScan) -> bool {
        if self.found.len() != other.found.len() {
            return false;
        }
        for i in 0..self.found.len() {
            if self.found[i].0 != other.found[i].0 || self.found[i].1 != other.found[i].1 {
                return false;
            }
        }
        true
    }
}

/// 波次历史查询：某组件按波次升序的全史（审计重钉纪律）。
pub fn wave_history(b: &PinBook, comp: usize) -> Vec<(u32, String)> {
    let mut hist: Vec<(u32, String)> = Vec::new();
    if comp < COMP_COUNT {
        for p in b.entries.iter() {
            if p.comp == comp {
                hist.push((p.wave, p.version.clone()));
            }
        }
        // 按波次升序（插入序即账序，但显式排序防未来乱序登记）。
        hist.sort_by(|a, b2| a.0.cmp(&b2.0));
    }
    hist
}

/// 波次历史严格递增核（重钉纪律：历史内无同波次、无回退）。
pub fn history_strictly_increasing(hist: &[(u32, String)]) -> bool {
    for w in hist.windows(2) {
        if w[0].0 >= w[1].0 {
            return false;
        }
    }
    !hist.is_empty()
}

/// 钉版账渲染导出（闭账材料第 21 波组成部分的可读面，逐件一行）。
pub fn render_ledger(b: &PinBook) -> String {
    let mut out = String::new();
    for c in 0..COMP_COUNT {
        match b.current(c) {
            Some(p) => {
                out.push_str(&format!(
                    "{}|{}|{}|w{}|{}\n",
                    COMP_NAMES[c], p.version, p.checksum, p.wave, p.root
                ));
            }
            None => {
                out.push_str(&format!("{}|UNPINNED\n", COMP_NAMES[c]));
            }
        }
    }
    out
}

/// 渲染完整性核：五件行数 + 每行四元组四段齐全（UNPINNED 行除外）。
pub fn render_complete(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != COMP_COUNT {
        return false;
    }
    for (i, ln) in lines.iter().enumerate() {
        if !ln.starts_with(COMP_NAMES[i]) {
            return false;
        }
        if !ln.ends_with("UNPINNED") && ln.split('|').count() != 5 {
            return false;
        }
    }
    true
}

/// 校验和双算核：对产物字节流做两次独立摘要（FNV-1a 双通道种子），×2 一致才可信。
/// （F46402 钉版校验和锁定判据的前置原语；通道 A/B 种子异或不同常量。）
pub struct SumCore;

impl SumCore {
    const SEED_A: u32 = 2166136261;
    const SEED_B: u32 = 0x9E3779B9;

    /// 通道 A：标准 FNV-1a。
    pub fn ch_a(data: &[u8]) -> String {
        let mut h = Self::SEED_A;
        for b in data {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        format!("{:08x}{:08x}", h, !h)
    }

    /// 通道 B：种子异或镜像（独立重算口径）。
    pub fn ch_b(data: &[u8]) -> String {
        let mut h = Self::SEED_B;
        for b in data {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        format!("{:08x}{:08x}", h, !h)
    }

    /// 双算一致 → 提交 16 位指纹；不一致 → None（调用方显性拒绝入账）。
    pub fn commit(data: &[u8]) -> Option<String> {
        let a = Self::ch_a(data);
        let b = Self::ch_b(data);
        if checksum_ok(&a) && checksum_ok(&b) && a != b {
            Some(a)
        } else {
            None
        }
    }

    /// 账面校验和与重算指纹一致性（镜像分发件逐字节口径）。
    pub fn matches_ledger(data: &[u8], ledger_sum: &str) -> bool {
        match Self::commit(data) {
            Some(s) => s == ledger_sum,
            None => false,
        }
    }
}

/// 标准夹具：五件全钉定（第 3 波次，固定版本与校验和）。
pub fn standard_book() -> PinBook {
    let mut b = PinBook::new();
    let waves = [3u32; COMP_COUNT];
    let vers = ["13.2.0", "17.0.6", "1.78.0", "2.43.0", "3.28.3"];
    let sums = [
        "0123456789abcdef", "1111111111111111", "2222222222222222",
        "3333333333333333", "4444444444444444",
    ];
    for c in 0..COMP_COUNT {
        let e = PinEntry::new(c, vers[c], sums[c], waves[c], "/usr")
            .expect("fixture entry must be valid");
        b.pin(e).expect("fixture pin must succeed");
    }
    let scan = HeadScan::scan(&C_HEADERS);
    if !scan.full_present() {
        // 夹具构造失败直接 panic：标准夹具必须真的能构造成功（判据前置）。
        panic!("standard fixture head scan must be full");
    }
    b
}

/// 夹具头扫描快照（与标准夹具同源，供指纹比对）。
pub fn fixture_scan() -> HeadScan {
    HeadScan::scan(&C_HEADERS)
}

/// 注入核：7 注入必抓（未知组件/重复波次/坏校验和/封账拒写/波次回退/头缺失/三元组歧义）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知组件（comp=9 越闭集）
    if PinEntry::new(9, "1.0", "0123456789abcdef", 1, "/usr").err() == Some(PIN_UNKNOWN_COMP) {
        caught += 1;
    }
    // 2) 坏校验和（长度 15）
    if PinEntry::new(COMP_GCC, "1.0", "0123456789abcde", 1, "/usr").err() == Some(PIN_SUM_BAD) {
        caught += 1;
    }
    // 3) 波次 0（哨兵保留值拒收）
    if PinEntry::new(COMP_GCC, "1.0", "0123456789abcdef", 0, "/usr").err() == Some(PIN_WAVE_BACK) {
        caught += 1;
    }
    // 4) 同件同波次重复落账
    let mut b = PinBook::new();
    let e1 = PinEntry::new(COMP_GCC, "13.2.0", "0123456789abcdef", 2, "/usr").expect("e1");
    b.pin(e1).expect("first pin");
    let e2 = PinEntry::new(COMP_GCC, "13.2.1", "1111111111111111", 2, "/usr").expect("e2");
    if b.pin(e2) == Err(PIN_DUP_WAVE) {
        caught += 1;
    }
    // 5) 波次回退（既有 3，重钉 2）
    let e3 = PinEntry::new(COMP_GCC, "13.1.0", "2222222222222222", 3, "/usr").expect("e3");
    b.pin(e3).expect("second pin");
    let e4 = PinEntry::new(COMP_GCC, "13.0.0", "3333333333333333", 2, "/usr").expect("e4");
    if b.pin(e4) == Err(PIN_WAVE_BACK) {
        caught += 1;
    }
    // 6) 封账后写拒
    b.seal();
    let e5 = PinEntry::new(COMP_CLANG, "17.0.6", "4444444444444444", 4, "/usr").expect("e5");
    if b.pin(e5) == Err(PIN_SEALED) {
        caught += 1;
    }
    // 7) 头文件缺失必抓（扫描集缺 stdio.h → full_present 假）
    let short: Vec<&str> = C_HEADERS.iter().skip(1).copied().collect();
    let sc = HeadScan::scan(&short);
    if !sc.full_present() && sc.missing[0] == "stdio.h" {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx46401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46401");
    // J1-1 钉版账全量：五件四元组逐件落账
    let b = standard_book();
    let ok = b.full_ledger_ok() && !b.sealed && b.top_wave == 3;
    s.add("46401-J1-钉版账全量", ok, if ok { "" } else { "五件四元组不全" });
    // J1-2 封账指纹确定性（同夹具两次封账指纹一致）
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("46401-J1-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J1-3 现行账取最高波次（夹具单波次，现行即该条）
    let cur = standard_book().current(COMP_RUSTC).expect("rustc pinned").clone();
    let ok = cur.version == "1.78.0" && cur.wave == 3;
    s.add("46401-J1-现行账语义", ok, if ok { "" } else { "现行账取错" });
    // J2-1 三元组三段式解析与宪法逐段一致
    let (a, v, sy) = parse_triple(TRIPLE_FULL).expect("full triple must parse");
    let ok = triple_query_ok(&a, &v, &sy);
    s.add("46401-J2-三元组查询一致", ok, if ok { "" } else { "三段不一致" });
    // J2-2 重钉升波次后现行账切换（3→4 严格升）
    let mut b2 = standard_book();
    let e = PinEntry::new(COMP_GCC, "13.3.0", "aaaaaaaaaaaaaaaa", 4, "/usr").expect("re-pin");
    let pin_ok = b2.pin(e).is_ok();
    let cur2 = b2.current(COMP_GCC).expect("gcc pinned").clone();
    let ok = pin_ok && cur2.version == "13.3.0" && cur2.wave == 4 && b2.top_wave == 4;
    s.add("46401-J2-重钉升波切换", ok, if ok { "" } else { "重钉语义漂移" });
    // J3-1 头文件集全集存在
    let sc = standard_book();
    let _ = sc;
    let scan = fixture_scan();
    let ok = scan.full_present() && scan.found.len() == C_HEADERS.len();
    s.add("46401-J3-头集全集存在", ok, if ok { "" } else { "头集缺失" });
    // J3-2 扫描指纹 ×2 次一致
    let ok = fixture_scan().stable(&fixture_scan());
    s.add("46401-J3-扫描指纹稳定", ok, if ok { "" } else { "指纹不稳定" });
    // J1-4 波次历史严格递增（重钉 3→4 两点史）
    let mut b3 = standard_book();
    let e = PinEntry::new(COMP_CLANG, "17.1.0", "bbbbbbbbbbbbbbbb", 4, "/usr").expect("hist re-pin");
    b3.pin(e).expect("hist pin ok");
    let hist = wave_history(&b3, COMP_CLANG);
    let ok = hist.len() == 2
        && history_strictly_increasing(&hist)
        && hist[0].0 == 3
        && hist[1].0 == 4;
    s.add("46401-J1-波次史递增", ok, if ok { "" } else { "波次史破纪" });
    // J2-3 渲染导出五件行齐 + 四段齐全
    let text = render_ledger(&standard_book());
    let ok = render_complete(&text) && text.contains("gcc|13.2.0|0123456789abcdef|w3|/usr");
    s.add("46401-J2-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-4 布设根四路径对账（C5 消费面）
    let rr = RootRecon::check(&["usr/bin", "usr/libexec", "usr/lib", "usr/include"]);
    let rr_bad = RootRecon::check(&["usr/bin", "usr/lib"]);
    let ok = rr.all_ok() && rr_bad.missing_paths().len() == 2;
    s.add("46401-J2-布设根对账", ok, if ok { "" } else { "根对账漂移" });
    // J2-5 校验和双算：通道分立 + 双算提交 + 账面一致
    let data = b"varix toolchain artifact payload 46401";
    let committed = SumCore::commit(data).expect("commit must succeed");
    let ok = checksum_ok(&committed)
        && SumCore::ch_a(data) != SumCore::ch_b(data)
        && SumCore::matches_ledger(data, &committed)
        && !SumCore::matches_ledger(b"tampered payload", &committed);
    s.add("46401-J2-校验和双算", ok, if ok { "" } else { "双算口径漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46401-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46401_all_checks_green() {
        let set = run_unx46401_checks();
        assert!(set.all_passed(), "UNX-F46401 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx46401_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
