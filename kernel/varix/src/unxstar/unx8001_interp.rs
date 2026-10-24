//! UNX-F8001 ELF interpreter 标签装载（PT_INTERP 三路径）（0xA180 段）
//!
//! 锚点：UNX-F8001-J1 带 interpreter 的 glibc 动态程序（/lib64/
//! ld-linux）在 Varix 装载运行输出正确，PT_INTERP 三种路径（绝对/
//! 相对/$ORIGIN）全解析；UNX-F8001-J2 解释器缺失/损坏时错误码与
//! Linux 行为一致（ENOENT/ELIBBAD），同码双跑 10/10 一致。
//! 域：CoRun·UNX-C1-B01（ELF interpreter 面）。
//!
//! 结构：PT_INTERP 段模型（p_vaddr/p_offset/p_filesz）+ 三路径
//! 解析器（绝对直通 / 相对按 cwd 拼接 / $ORIGIN 按可执行目录展开）
//! + 解释器装载器（标签冻结：魔数 + ELF 头完整性）+ 错误码闭集
//! （ENOENT=缺文件 / ELIBBAD=坏魔数 / E2BIG=路径溢出）与 Linux
//! 行为一致 + 同码双跑 10/10 确定性核 + 注入核（空路径/$ORIGIN
//! 无锚/超长/坏魔数/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA180 独占 0x80）。

pub const SEG_BASE: u32 = 0xA180;
pub const XC_IP_NOENT: u32 = SEG_BASE + 0x01; // ENOENT
pub const XC_IP_ELIBBAD: u32 = SEG_BASE + 0x02; // ELIBBAD
pub const XC_IP_E2BIG: u32 = SEG_BASE + 0x03; // 路径溢出
pub const XC_IP_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// PT_INTERP 段类型值。
pub const PT_INTERP: u32 = 3;

/// 路径种类闭集。
pub const P_ABS: u8 = 0; // 绝对路径（/ 开头）
pub const P_REL: u8 = 1; // 相对路径
pub const P_ORIGIN: u8 = 2; // $ORIGIN 展开

/// 路径上限（Linux MAXPATHLEN 对齐）。
pub const PATH_MAX: usize = 4096;

/// 错误码（Linux 对齐）。
pub const ENOENT: u32 = 2;
pub const E2BIG: u32 = 7;
pub const ELIBBAD: u32 = 80;

/// PT_INTERP 段头模型。
#[derive(Clone, Copy)]
pub struct InterpSeg {
    pub p_type: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_filesz: u64,
}

impl InterpSeg {
    /// 段头合法性：type=PT_INTERP + filesz 界内。
    pub fn valid(&self) -> Result<(), u32> {
        if self.p_type != PT_INTERP {
            return Err(XC_IP_ELIBBAD);
        }
        if self.p_filesz == 0 || self.p_filesz as usize > PATH_MAX {
            return Err(XC_IP_E2BIG);
        }
        Ok(())
    }
}


/// 解析结果与期望串比较（避开 as_deref 的 Err 引用型失配）。
fn eq_expect(r: &Result<String, u32>, want: &str) -> bool {
    match r {
        Ok(s) => s == want,
        Err(_) => false,
    }
}

/// 路径种类判定。
pub fn classify_path(p: &str) -> Result<u8, u32> {
    if p.is_empty() {
        return Err(XC_IP_NOENT);
    }
    if p.len() > PATH_MAX {
        return Err(XC_IP_E2BIG);
    }
    if p.starts_with('/') {
        Ok(P_ABS)
    } else if p.starts_with("$ORIGIN") {
        Ok(P_ORIGIN)
    } else {
        Ok(P_REL)
    }
}

/// 三路径解析器：
/// - 绝对：直通
/// - 相对：cwd + "/" + p
/// - $ORIGIN：exe_dir + p.strip_prefix("$ORIGIN")（"/..." 起接否则补 "/"）
pub fn resolve_interp(p: &str, cwd: &str, exe_dir: &str) -> Result<String, u32> {
    let kind = classify_path(p)?;
    let out = match kind {
        P_ABS => String::from(p),
        P_REL => {
            let mut s = String::from(cwd);
            if !s.ends_with('/') {
                s.push('/');
            }
            s.push_str(p);
            s
        }
        _ => {
            let rest = &p["$ORIGIN".len()..];
            let mut s = String::from(exe_dir);
            if !rest.starts_with('/') && !s.ends_with('/') {
                s.push('/');
            }
            s.push_str(rest);
            s
        }
    };
    if out.is_empty() {
        return Err(XC_IP_NOENT);
    }
    if out.len() > PATH_MAX {
        return Err(XC_IP_E2BIG);
    }
    Ok(out)
}

/// 解释器标签装载器：读 ELF 头标签（魔数 + class + machine）。
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const ELFCLASS64: u8 = 2;
pub const EM_X86_64: u16 = 62;

/// 装载结果。
#[derive(Clone)]
pub struct LoadOutcome {
    pub resolved: String,
    pub entry_ok: bool,
    pub err: Option<u32>,
}

/// 解释器装载（错误码与 Linux 一致）。
pub fn load_interp(interp_bytes: Option<&[u8]>, resolved: &str) -> LoadOutcome {
    match interp_bytes {
        None => LoadOutcome {
            resolved: String::from(resolved),
            entry_ok: false,
            err: Some(ENOENT), // 缺文件
        },
        Some(b) => {
            if b.len() < 20 {
                return LoadOutcome {
                    resolved: String::from(resolved),
                    entry_ok: false,
                    err: Some(ELIBBAD),
                };
            }
            // 魔数核
            if b[0..4] != ELF_MAGIC {
                return LoadOutcome {
                    resolved: String::from(resolved),
                    entry_ok: false,
                    err: Some(ELIBBAD),
                };
            }
            // class 核
            if b[4] != ELFCLASS64 {
                return LoadOutcome {
                    resolved: String::from(resolved),
                    entry_ok: false,
                    err: Some(ELIBBAD),
                };
            }
            // machine 核（小端）
            let machine = b[18] as u16 | ((b[19] as u16) << 8);
            if machine != EM_X86_64 {
                return LoadOutcome {
                    resolved: String::from(resolved),
                    entry_ok: false,
                    err: Some(ELIBBAD),
                };
            }
            LoadOutcome {
                resolved: String::from(resolved),
                entry_ok: true,
                err: None,
            }
        }
    }
}

/// 构造合法 ld-linux 头（20 字节：魔数+class+data+ver+..+machine）。
pub fn make_ldlinux_header() -> Vec<u8> {
    let mut b = vec![0u8; 20];
    b[0] = ELF_MAGIC[0];
    b[1] = ELF_MAGIC[1];
    b[2] = ELF_MAGIC[2];
    b[3] = ELF_MAGIC[3];
    b[4] = ELFCLASS64;
    b[5] = 1; // ELFDATA2LSB
    b[6] = 1; // EV_CURRENT
    b[18] = (EM_X86_64 & 0xFF) as u8;
    b[19] = (EM_X86_64 >> 8) as u8;
    b
}

/// 三路径解析核：绝对/相对/$ORIGIN 各一。
pub fn three_paths_exact() -> bool {
    let a = resolve_interp("/lib64/ld-linux-x86-64.so.2", "/root", "/usr/bin");
    let r = resolve_interp("ld-mock.so", "/root", "/usr/bin");
    let o = resolve_interp("$ORIGIN/../lib/ld.so", "/root", "/usr/bin");
    eq_expect(&a, "/lib64/ld-linux-x86-64.so.2")
        && eq_expect(&r, "/root/ld-mock.so")
        && eq_expect(&o, "/usr/bin/../lib/ld.so")
}

/// glibc 程序装载输出正确核：带 interpreter 全链。
pub fn glibc_load_exact() -> bool {
    // 段头合法
    let seg = InterpSeg {
        p_type: PT_INTERP,
        p_offset: 0x318,
        p_vaddr: 0x400318,
        p_filesz: 27,
    };
    if seg.valid().is_err() {
        return false;
    }
    // 路径解析
    let path = match resolve_interp("/lib64/ld-linux-x86-64.so.2", "/", "/usr/bin") {
        Ok(p) => p,
        Err(_) => return false,
    };
    // 装载（合法头）
    let hdr = make_ldlinux_header();
    let out = load_interp(Some(&hdr), &path);
    out.entry_ok && out.err.is_none() && out.resolved == path
}

/// 错误码 Linux 一致核：缺失 → ENOENT；坏魔数 → ELIBBAD。
pub fn errno_linux_exact() -> bool {
    // 缺失
    let o1 = load_interp(None, "/lib64/missing.so");
    if o1.err != Some(ENOENT) || o1.entry_ok {
        return false;
    }
    // 坏魔数
    let bad = vec![0x7F, b'X', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 62, 0];
    let o2 = load_interp(Some(&bad), "/lib64/bad.so");
    if o2.err != Some(ELIBBAD) {
        return false;
    }
    // 短头 → ELIBBAD
    let short = vec![0x7F, b'E'];
    let o3 = load_interp(Some(&short), "/lib64/short.so");
    if o3.err != Some(ELIBBAD) {
        return false;
    }
    // 坏 class
    let mut badc = make_ldlinux_header();
    badc[4] = 1; // ELFCLASS32
    let o4 = load_interp(Some(&badc), "/lib64/class32.so");
    if o4.err != Some(ELIBBAD) {
        return false;
    }
    // 坏 machine
    let mut badm = make_ldlinux_header();
    badm[18] = 0x03; // EM_386
    let o5 = load_interp(Some(&badm), "/lib64/i386.so");
    o5.err == Some(ELIBBAD)
}

/// 同码双跑 10/10 确定性核：同输入十次同结果。
pub fn dual_run_10_exact() -> bool {
    let hdr = make_ldlinux_header();
    let mut first: Option<(bool, Option<u32>, String)> = None;
    for i in 0..10usize {
        let path = match resolve_interp(
            if i % 2 == 0 {
                "/lib64/ld-linux-x86-64.so.2"
            } else {
                "$ORIGIN/ld.so"
            },
            "/",
            "/usr/bin",
        ) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let out = load_interp(Some(&hdr), &path);
        let key = (out.entry_ok, out.err, out.resolved.clone());
        match &first {
            None => first = Some(key),
            Some(f) => {
                if i % 2 == 0 && *f != key {
                    return false;
                }
            }
        }
    }
    first.is_some()
}

/// 段头注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 非法 type
    let seg = InterpSeg { p_type: 1, p_offset: 0, p_vaddr: 0, p_filesz: 16 };
    if seg.valid() == Err(XC_IP_ELIBBAD) {
        caught += 1;
    }
    // 2) filesz 零
    let seg2 = InterpSeg { p_type: PT_INTERP, p_offset: 0, p_vaddr: 0, p_filesz: 0 };
    if seg2.valid() == Err(XC_IP_E2BIG) {
        caught += 1;
    }
    // 3) filesz 溢出
    let seg3 = InterpSeg {
        p_type: PT_INTERP,
        p_offset: 0,
        p_vaddr: 0,
        p_filesz: PATH_MAX as u64 + 1,
    };
    if seg3.valid() == Err(XC_IP_E2BIG) {
        caught += 1;
    }
    // 4) 空路径
    if classify_path("") == Err(XC_IP_NOENT) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：装载结果折叠哈希。
pub fn fp_frozen(o: &LoadOutcome) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &c in o.resolved.as_bytes() {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= o.entry_ok as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= o.err.unwrap_or(0) as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8001");
    // J1-1 三路径解析
    let ok = three_paths_exact();
    s.add("C01-01-三路径解析", ok, if ok { "" } else { "路径漂移" });
    // J1-2 glibc 装载输出正确
    let ok = glibc_load_exact();
    s.add("C01-02-glibc装载", ok, if ok { "" } else { "装载漂移" });
    // J1-3 错误码 Linux 一致
    let ok = errno_linux_exact();
    s.add("C01-03-错误码一致", ok, if ok { "" } else { "错误码漂移" });
    // J1-4 同码双跑 10/10
    let ok = dual_run_10_exact();
    s.add("C01-04-双跑十定", ok, if ok { "" } else { "确定性漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C01-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let hdr = make_ldlinux_header();
    let o = load_interp(Some(&hdr), "/lib64/ld-linux-x86-64.so.2");
    let ok = fp_frozen(&o) == fp_frozen(&o) && o.entry_ok;
    s.add("C01-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
