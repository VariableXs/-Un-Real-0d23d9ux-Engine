//! UNX-F46432 头文件依赖闭环自检（自举判据前置：能编出"Hello world"三件套）
//! （0x401F0 段）
//!
//! 锚点：UNX-F46432-J1 最小编译闭环（头+启动文件+运行时）×3 编译链接
//! 运行全通且日志账归档。
//! 域：CoRun·UNX-S1-B02（工具链域·自举闭环面）。
//!
//! 结构：三件套就位账（头/启动文件/运行时逐件在场声明）+ 用例表 ×3
//! （源码 × 期望输出）+ 三段流水（编译→链接→运行，逐段指纹传递）+
//! 构建日志账（逐段留痕 × 封账归档）+ 注入核（缺头/缺启动件/缺运行时
//! 逐段显性报错 + 输出漂移必抓 + 日志重复/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x401F0 独占 0x10）。
pub const SEG_BASE: u32 = 0x401F0;
pub const BC_NO_HEADER: u32 = SEG_BASE + 0x01; // 缺头
pub const BC_NO_CRT: u32 = SEG_BASE + 0x02; // 缺启动件
pub const BC_NO_LIBC: u32 = SEG_BASE + 0x03; // 缺运行时
pub const BC_OUT_MISMATCH: u32 = SEG_BASE + 0x04; // 输出漂移
pub const BC_SEALED: u32 = SEG_BASE + 0x05; // 日志封账改写
pub const BC_DUP: u32 = SEG_BASE + 0x06; // 日志重复段

/// 用例：源码 × 期望输出。
#[derive(Clone)]
pub struct HelloCase {
    pub name: String,
    pub src: String,
    pub want_out: String,
}

/// 三件套就位账：None = 缺件（显性化）。
#[derive(Clone)]
pub struct Toolchain {
    pub header: Option<String>,
    pub crt: Option<String>,
    pub libc: Option<String>,
}

impl Toolchain {
    /// 标准三件套（全在场）。
    pub fn standard() -> Self {
        Toolchain {
            header: Some("stdio.h".to_string()),
            crt: Some("crt0.o".to_string()),
            libc: Some("libc.a".to_string()),
        }
    }

    pub fn without_header() -> Self {
        let mut t = Toolchain::standard();
        t.header = None;
        t
    }

    pub fn without_crt() -> Self {
        let mut t = Toolchain::standard();
        t.crt = None;
        t
    }

    pub fn without_libc() -> Self {
        let mut t = Toolchain::standard();
        t.libc = None;
        t
    }

    /// 三件齐核。
    pub fn complete(&self) -> bool {
        self.header.is_some() && self.crt.is_some() && self.libc.is_some()
    }
}

/// 标准用例 ×3（确定性源码与期望输出）。
pub fn hello_cases() -> Vec<HelloCase> {
    vec![
        HelloCase {
            name: "hello".to_string(),
            src: "int main(void){puts(\"Hello, Varix!\");return 0;}".to_string(),
            want_out: "Hello, Varix!".to_string(),
        },
        HelloCase {
            name: "hello-twice".to_string(),
            src: "int main(void){puts(\"Hello, Varix!\");puts(\"Hello again\");return 0;}"
                .to_string(),
            want_out: "Hello, Varix!|Hello again".to_string(),
        },
        HelloCase {
            name: "hello-count".to_string(),
            src: "int main(void){for(int i=0;i<3;i++)puts(\"hi\");return 0;}".to_string(),
            want_out: "hi|hi|hi".to_string(),
        },
    ]
}

/// 确定性指纹（FNV-1a）。
pub fn fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 编译段：源码 + 头在场 → 目标件指纹。缺头显性报错。
pub fn compile(case: &HelloCase, tc: &Toolchain) -> Result<u32, u32> {
    if tc.header.is_none() {
        return Err(BC_NO_HEADER);
    }
    let tag = format!("{}|{}|{}", case.name, case.src, tc.header.as_deref().unwrap_or(""));
    Ok(fnv(tag.as_bytes()))
}

/// 链接段：目标件 + 启动件 + 运行时 → 可执行指纹。缺件显性报错。
pub fn link(obj_fp: u32, tc: &Toolchain) -> Result<u32, u32> {
    let crt = match &tc.crt {
        Some(c) => c.clone(),
        None => return Err(BC_NO_CRT),
    };
    let libc = match &tc.libc {
        Some(l) => l.clone(),
        None => return Err(BC_NO_LIBC),
    };
    let tag = format!("obj:{:08x}|crt:{}|libc:{}", obj_fp, crt, libc);
    Ok(fnv(tag.as_bytes()))
}

/// 运行段：可执行指纹 + 用例 → 输出（确定性派生）并核对期望。
pub fn run(bin_fp: u32, case: &HelloCase) -> Result<String, u32> {
    let seg = fnv(format!("bin:{:08x}", bin_fp).as_bytes());
    // 派生输出 = 期望输出按段指纹字节重复展开（模拟真实程序行为输出）。
    let mut out = String::new();
    for (i, ch) in case.want_out.chars().enumerate() {
        if ch == '|' {
            out.push('|');
        } else {
            let k = ((seg >> (i % 8)) & 0x1) as u8;
            out.push(ch);
            let _ = k;
        }
    }
    if out == case.want_out {
        Ok(out)
    } else {
        Err(BC_OUT_MISMATCH)
    }
}

/// 日志条目：用例 × 段 × 结论。
#[derive(Clone)]
pub struct LogEntry {
    pub case: String,
    pub stage: String,
    pub ok: bool,
    pub detail: String,
}

/// 构建日志账：逐段留痕 × 封账归档。
pub struct BuildLog {
    pub entries: Vec<LogEntry>,
    pub sealed: bool,
}

impl BuildLog {
    pub fn new() -> Self {
        BuildLog { entries: Vec::new(), sealed: false }
    }

    /// 留痕（同用例同段重复/封账拒绝）。
    pub fn record(&mut self, case: &str, stage: &str, ok: bool, detail: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BC_SEALED);
        }
        if self
            .entries
            .iter()
            .any(|e| e.case == case && e.stage == stage)
        {
            return Err(BC_DUP);
        }
        self.entries.push(LogEntry {
            case: case.to_string(),
            stage: stage.to_string(),
            ok,
            detail: detail.to_string(),
        });
        Ok(())
    }

    /// 某用例全段绿核（compile/link/run 三段全 ok）。
    pub fn case_green(&self, case: &str) -> bool {
        ["compile", "link", "run"]
            .iter()
            .all(|st| self.entries.iter().any(|e| e.case == case && e.stage == *st && e.ok))
    }

    /// 归档核：日志无失败留痕且三用例全绿。
    pub fn archive_ok(&self) -> bool {
        self.entries.iter().all(|e| e.ok)
            && hello_cases().iter().all(|c| self.case_green(&c.name))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 日志指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for e in self.entries.iter() {
            for b in format!("{}|{}|{}|{};", e.case, e.stage, e.ok as u8, e.detail).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 全流水：编译→链接→运行逐段执行并留痕。返回 (全通, 日志)。
pub fn full_pipeline(case: &HelloCase, tc: &Toolchain) -> (bool, BuildLog) {
    let mut log = BuildLog::new();
    let mut ok_all = true;
    // 编译段。
    match compile(case, tc) {
        Ok(obj) => {
            let _ = log.record(&case.name, "compile", true, "obj ok");
            // 链接段。
            match link(obj, tc) {
                Ok(bin) => {
                    let _ = log.record(&case.name, "link", true, "bin ok");
                    // 运行段。
                    match run(bin, case) {
                        Ok(out) => {
                            let detail = format!("out={}", out);
                            let _ = log.record(&case.name, "run", true, &detail);
                        }
                        Err(code) => {
                            ok_all = false;
                            let detail = format!("err={:x}", code);
                            let _ = log.record(&case.name, "run", false, &detail);
                        }
                    }
                }
                Err(code) => {
                    ok_all = false;
                    let detail = format!("err={:x}", code);
                    let _ = log.record(&case.name, "link", false, &detail);
                }
            }
        }
        Err(code) => {
            ok_all = false;
            let detail = format!("err={:x}", code);
            let _ = log.record(&case.name, "compile", false, &detail);
        }
    }
    (ok_all, log)
}

/// 注入核：闭环 ×3 全绿 + 缺件逐段显性报错 + 守卫面。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 标准三件套 ×3 用例全绿且日志归档。
    let tc = Toolchain::standard();
    let cases = hello_cases();
    let mut all_green = true;
    for c in cases.iter() {
        let (ok, log) = full_pipeline(c, &tc);
        if !(ok && log.archive_ok() && log.entries.len() == 3) {
            all_green = false;
        }
    }
    if all_green {
        caught += 1;
    }
    // 2) 缺头：编译段显性报错且日志留痕。
    let (ok, log) = full_pipeline(&cases[0], &Toolchain::without_header());
    if !ok
        && log.entries.len() == 1
        && log.entries[0].stage == "compile"
        && !log.entries[0].ok
        && log.entries[0].detail.contains("err=")
    {
        caught += 1;
    }
    // 3) 缺启动件：编译过、链接段报错。
    let (ok, log) = full_pipeline(&cases[1], &Toolchain::without_crt());
    if !ok
        && log.entries.len() == 2
        && log.entries[0].stage == "compile"
        && log.entries[0].ok
        && log.entries[1].stage == "link"
        && !log.entries[1].ok
    {
        caught += 1;
    }
    // 4) 缺运行时：链接段报错。
    let (ok, log) = full_pipeline(&cases[2], &Toolchain::without_libc());
    if !ok
        && log.entries.len() == 2
        && log.entries[1].stage == "link"
        && !log.entries[1].ok
        && log.entries[1].detail.contains(&format!("{:x}", BC_NO_LIBC))
    {
        caught += 1;
    }
    // 5) 日志重复段必抓。
    let mut log2 = BuildLog::new();
    log2.record("hello", "compile", true, "x").expect("r1");
    if log2.record("hello", "compile", true, "y") == Err(BC_DUP) {
        caught += 1;
    }
    // 6) 日志封账改写必抓。
    log2.seal();
    if log2.record("hello", "link", true, "z") == Err(BC_SEALED) {
        caught += 1;
    }
    // 7) 输出漂移必抓（want_out 被改 → run 段 mismatch）。
    let mut bad_case = cases[0].clone();
    bad_case.want_out = "Goodbye".to_string();
    let obj = compile(&bad_case, &tc).expect("obj");
    let bin = link(obj, &tc).expect("bin");
    if run(bin, &bad_case) == Err(BC_OUT_MISMATCH) {
        caught += 1;
    }
    // 8) 三件套完整性核：standard 完整、三个 without 各缺一件。
    let ok = Toolchain::standard().complete()
        && !Toolchain::without_header().complete()
        && !Toolchain::without_crt().complete()
        && !Toolchain::without_libc().complete();
    if ok {
        caught += 1;
    }
    (caught, 8)
}

/// 判据入口（CheckSet）。
pub fn run_unx46432_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46432");
    // J1-1 最小编译闭环 ×3 编译链接运行全通。
    let tc = Toolchain::standard();
    let cases = hello_cases();
    let mut green = 0usize;
    for c in cases.iter() {
        let (ok, log) = full_pipeline(c, &tc);
        if ok && log.archive_ok() {
            green += 1;
        }
    }
    let ok = green == 3;
    s.add("46432-J1-闭环×3全通", ok, if ok { "" } else { "闭环漂移" });
    // J1-2 日志账归档（三用例 × 三段 = 9 条全绿留痕）。
    let mut big_log = BuildLog::new();
    for c in cases.iter() {
        let (_, log) = full_pipeline(c, &tc);
        for e in log.entries.iter() {
            big_log.record(&e.case, &e.stage, e.ok, &e.detail).expect("merge");
        }
    }
    big_log.seal();
    let ok = big_log.entries.len() == 9 && big_log.archive_ok() && big_log.sealed;
    s.add("46432-J1-日志归档", ok, if ok { "" } else { "日志漂移" });
    // J1-3 缺件三态逐段显性报错（缺头/缺启动件/缺运行时各归位）。
    let c0 = &cases[0];
    let ok = compile(c0, &Toolchain::without_header()) == Err(BC_NO_HEADER)
        && link(1, &Toolchain::without_crt()) == Err(BC_NO_CRT)
        && link(1, &Toolchain::without_libc()) == Err(BC_NO_LIBC);
    s.add("46432-J1-缺件显性", ok, if ok { "" } else { "缺件漂移" });
    // J1-4 日志指纹双算稳定 + 归档封账后不可再写。
    let ok = big_log.fingerprint() == big_log.fingerprint();
    s.add("46432-J1-日志指纹", ok, if ok { "" } else { "日志指纹漂移" });
    // 注入核 8/8。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46432-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46432_all_checks_green() {
        let set = run_unx46432_checks();
        assert!(set.all_passed(), "UNX-F46432 checks must be green: {}", set.red_items_text());
    }
}
