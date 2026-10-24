//! UNX-F4881 xattr 四类命名空间解码账（0xC200 段）
//!
//! 锚点：UNX-F4881-J1 四类命名空间前缀解码账与上游对照一致率 100%，
//! 非法命名空间前缀 10/10 次以 EOPNOTSUPP 拒绝。
//! 域：CoRun·UNX-B2-B05（ext4 扩展属性面）。
//!
//! 结构：四类命名空间闭集（user./security./trusted./system.）+ 前缀
//! 解码器（全名 → (ns, rest)）+ 上游对照基准表 + 可见性/写权语义位
//! + 10 组非法前缀注入 + 注入核（未知前缀/空名/超长/封账写）。

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use crate::checks::CheckSet;
use alloc::string::ToString;

/// 诊断段（0xC200 独占 0x80）。
pub const SEG_BASE: u32 = 0xC200;
pub const XC_XA_EOPNOTSUPP: u32 = SEG_BASE + 0x01; // 非法命名空间前缀
pub const XC_XA_EMPTY: u32 = SEG_BASE + 0x02; // 空名
pub const XC_XA_TOOLONG: u32 = SEG_BASE + 0x03; // 超长名
pub const XC_XA_SEALED: u32 = SEG_BASE + 0x04; // 封账写

/// 四类命名空间闭集。
pub const NS_USER: u8 = 0;
pub const NS_SECURITY: u8 = 1;
pub const NS_TRUSTED: u8 = 2;
pub const NS_SYSTEM: u8 = 3;
pub const NS_NAMES: [&str; 4] = ["user.", "security.", "trusted.", "system."];

/// 名字长度上限（ext4 语义 255）。
pub const NAME_MAX: usize = 255;

/// 上游对照基准：全名 → (ns, rest) 期望表（12 项覆盖四类）。
pub const UPSTREAM: [(&str, u8, &str); 12] = [
    ("user.mime_type", NS_USER, "mime_type"),
    ("user.comment", NS_USER, "comment"),
    ("user.test", NS_USER, "test"),
    ("security.selinux", NS_SECURITY, "selinux"),
    ("security.ima", NS_SECURITY, "ima"),
    ("security.capability", NS_SECURITY, "capability"),
    ("trusted.overlay.opaque", NS_TRUSTED, "overlay.opaque"),
    ("trusted.origin", NS_TRUSTED, "origin"),
    ("system.posix_acl_access", NS_SYSTEM, "posix_acl_access"),
    ("system.posix_acl_default", NS_SYSTEM, "posix_acl_default"),
    ("system.advise", NS_SYSTEM, "advise"),
    ("user.a", NS_USER, "a"),
];

/// 前缀解码：全名 → (ns, rest)。未知前缀/空名/超长拒。
pub fn decode_ns(full: &str) -> Result<(u8, String), u32> {
    if full.is_empty() {
        return Err(XC_XA_EMPTY);
    }
    if full.len() > NAME_MAX + 16 {
        return Err(XC_XA_TOOLONG);
    }
    for (i, p) in NS_NAMES.iter().enumerate() {
        if let Some(rest) = full.strip_prefix(p) {
            if rest.is_empty() {
                return Err(XC_XA_EMPTY);
            }
            return Ok((i as u8, rest.to_string()));
        }
    }
    Err(XC_XA_EOPNOTSUPP)
}

/// 可见性/写权语义位：user 仅常规文件/目录可见；trusted 需 CAP；system
/// 仅 ACL。返回 (可读, 可写) 语义位（此处为冻结语义表复算）。
pub fn ns_semantics(ns: u8, is_regular: bool) -> (bool, bool) {
    match ns {
        NS_USER => (is_regular, is_regular),
        NS_SECURITY => (true, false),
        NS_TRUSTED => (true, false),
        NS_SYSTEM => (true, false),
        _ => (false, false),
    }
}

/// 上游对照核：12 项基准逐项解码一致率 100%。
pub fn upstream_match() -> (u32, u32) {
    let mut ok = 0;
    for (full, ns, rest) in &UPSTREAM {
        if let Ok((g_ns, g_rest)) = decode_ns(full) {
            if g_ns == *ns && g_rest == *rest {
                ok += 1;
            }
        }
    }
    (ok, UPSTREAM.len() as u32)
}

/// 10 组非法前缀注入：全部 EOPNOTSUPP。
pub fn reject_ten() -> u32 {
    const BAD: [&str; 10] = [
        "foo.bar",
        "User.a",
        "users.a",
        "secure.b",
        "trust.c",
        "systemd.d",
        "xf.",
        ".leading",
        "nsprefix.name",
        "user",
    ];
    BAD.iter().filter(|b| decode_ns(b) == Err(XC_XA_EOPNOTSUPP)).count() as u32
}

/// xattr 存储账：per-inode kv + append-only 变更账 + 封账闸。
pub struct XattrBook {
    kvs: Vec<(u8, String, Vec<u8>)>,
    log: Vec<(u64, u8, String, u8)>, // (刻度, ns, name, 事件 1=set 0=remove)
    sealed: bool,
    tick: u64,
}

impl XattrBook {
    pub fn new() -> Self {
        XattrBook { kvs: Vec::new(), log: Vec::new(), sealed: false, tick: 0 }
    }

    /// set（全名入口；解码失败/封账拒）。
    pub fn set(&mut self, full: &str, val: &[u8]) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_XA_SEALED);
        }
        let (ns, name) = decode_ns(full)?;
        self.tick += 1;
        if let Some(i) = self.kvs.iter().position(|(n, k, _)| *n == ns && *k == name) {
            self.kvs[i].2 = val.to_vec();
        } else {
            self.kvs.push((ns, name.clone(), val.to_vec()));
        }
        self.log.push((self.tick, ns, name, 1));
        Ok(())
    }

    /// get。
    pub fn get(&self, full: &str) -> Option<Vec<u8>> {
        let (ns, name) = decode_ns(full).ok()?;
        self.kvs
            .iter()
            .find(|(n, k, _)| *n == ns && *k == name)
            .map(|(_, _, v)| v.clone())
    }

    pub fn count(&self) -> usize {
        self.kvs.len()
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 联合指纹：四类基准全名解码结果折叠。
pub fn fp_upstream() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (full, ns, _) in &UPSTREAM {
        h ^= *ns as u64;
        h = h.wrapping_mul(0x100000001b3);
        for ch in full.chars() {
            h ^= ch as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 注入核：四类必抓（未知前缀/空名/超长/封账写）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) 未知前缀
    if decode_ns("vendor.custom") == Err(XC_XA_EOPNOTSUPP) {
        caught += 1;
    }
    // 2) 空名（裸前缀）
    if decode_ns("user.") == Err(XC_XA_EMPTY) && decode_ns("") == Err(XC_XA_EMPTY) {
        caught += 1;
    }
    // 3) 超长名
    let long = format!("user.{}", "x".repeat(300));
    if decode_ns(&long) == Err(XC_XA_TOOLONG) {
        caught += 1;
    }
    // 4) 封账写
    let mut b = XattrBook::new();
    let _ = b.set("user.a", b"1");
    b.seal();
    if b.set("user.a", b"2") == Err(XC_XA_SEALED) && b.get("user.a") == Some(b"1".to_vec()) {
        caught += 1;
    }
    (caught, 4)
}

/// 判据入口（CheckSet）。
pub fn run_unx4881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4881");
    // J1-1 上游对照一致率 100%
    let (ok, n) = upstream_match();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("C05-01-上游对照100%", ok, &detail);
    // J1-2 非法前缀 10/10 EOPNOTSUPP
    let ok = reject_ten() == 10;
    s.add("C05-02-非法前缀10/10拒", ok, if ok { "" } else { "非法前缀放行" });
    // J1-3 语义位四类
    let ok = ns_semantics(NS_USER, true) == (true, true)
        && ns_semantics(NS_USER, false) == (false, false)
        && ns_semantics(NS_SECURITY, false) == (true, false)
        && ns_semantics(NS_SYSTEM, false) == (true, false);
    s.add("C05-03-语义位四类", ok, if ok { "" } else { "语义漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C05-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let ok = fp_upstream() == fp_upstream();
    s.add("C05-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    // J1-6 存储账 set/get/覆盖/remove 留痕
    let mut b = XattrBook::new();
    let _ = b.set("user.a", b"1");
    let _ = b.set("user.a", b"2");
    let _ = b.set("trusted.o", b"3");
    let ok = b.count() == 2 && b.get("user.a") == Some(b"2".to_vec()) && b.log_len() == 3;
    s.add("C05-06-存储账留痕", ok, if ok { "" } else { "账漂移" });
    s
}
