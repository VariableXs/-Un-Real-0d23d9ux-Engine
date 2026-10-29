//! UNX-F11202 GLIBC 符号版本查找语义（0x1f802 段）
//!
//! 锚点：UNX-F11202-J1 链接旧 GLIBC_x.y 的程序运行期正确解析旧符号版本，
//! 版本不匹配报错文本与 Linux 逐字一致。
//! 域：CoRun·UNX-C5-B01——符号表按 (名, 版本) 双键登记，解析按所需版本
//! 精确命中；同名异版可共存，缺版本与缺符号两路分报，报错文本照抄 glibc。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f802;
pub const SV_MISS: u32 = SEG_BASE + 0x01; // 符号在但版本缺
pub const SV_NOSYM: u32 = SEG_BASE + 0x02; // 符号不存在
pub const SV_DUP: u32 = SEG_BASE + 0x03; // 双键重号
pub const SV_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后登记
pub const SV_ZERO: u32 = SEG_BASE + 0x05; // 地址零值
pub const SV_OVER: u32 = SEG_BASE + 0x06; // 超容
pub const SV_BADVER: u32 = SEG_BASE + 0x07; // 版本号非法

pub const MAX_SYMS: usize = 24;
pub const SV_TAG: u64 = 0x1f802c5;
pub const FP_STEP: u64 = 0x100000001b3;
/// 钉定库路径（报错文本第二段）。
pub const LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";

/// 版本化符号：名 + 版本 + 地址。
#[derive(Clone, Debug)]
pub struct Sym {
    pub name: &'static str,
    pub ver: (u32, u32),
    pub addr: u32,
}

impl Sym {
    pub fn render(&self) -> String {
        format!("{}@{}.{}={:#x}", self.name, self.ver.0, self.ver.1, self.addr)
    }
}

/// 符号表：双键登记、按版本解析、冻结。
#[derive(Clone, Debug)]
pub struct SymTab {
    pub lib: &'static str,
    pub syms: Vec<Sym>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SymTab {
    pub fn new(lib: &'static str) -> SymTab {
        SymTab { lib, syms: Vec::new(), frozen: false, root: None }
    }

    /// 登记：冻结/零地址/重键/超容/坏版本五路拒。
    pub fn add(&mut self, name: &'static str, ver: (u32, u32), addr: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        if ver.0 == 0 || ver.1 == 0 {
            return Err(SV_BADVER);
        }
        if addr == 0 {
            return Err(SV_ZERO);
        }
        if self.syms.iter().any(|s| s.name == name && s.ver == ver) {
            return Err(SV_DUP);
        }
        if self.syms.len() >= MAX_SYMS {
            return Err(SV_OVER);
        }
        self.syms.push(Sym { name, ver, addr });
        Ok(())
    }

    /// 精确解析：名在版本缺 → SV_MISS；名不在 → SV_NOSYM。
    pub fn resolve(&self, name: &'static str, req: (u32, u32)) -> Result<u32, u32> {
        let mut named = false;
        let mut k = 0usize;
        while k < self.syms.len() {
            if self.syms[k].name == name {
                named = true;
                if self.syms[k].ver == req {
                    return Ok(self.syms[k].addr);
                }
            }
            k += 1;
        }
        if named { Err(SV_MISS) } else { Err(SV_NOSYM) }
    }

    /// 最新版本（同名最高版本）解析，供未钉版链接使用。
    pub fn newest(&self, name: &'static str) -> Result<u32, u32> {
        let mut best: Option<&Sym> = None;
        let mut k = 0usize;
        while k < self.syms.len() {
            if self.syms[k].name == name {
                match best {
                    None => best = Some(&self.syms[k]),
                    Some(b) => {
                        let n = self.syms[k].ver;
                        if (n.0, n.1) > (b.ver.0, b.ver.1) {
                            best = Some(&self.syms[k]);
                        }
                    }
                }
            }
            k += 1;
        }
        match best {
            Some(b) => Ok(b.addr),
            None => Err(SV_NOSYM),
        }
    }

    /// 同名版本清单（登记序）。
    pub fn versions_of(&self, name: &'static str) -> Vec<(u32, u32)> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        let mut k = 0usize;
        while k < self.syms.len() {
            if self.syms[k].name == name {
                out.push(self.syms[k].ver);
            }
            k += 1;
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = SV_TAG;
        let bytes = self.lib.as_bytes();
        let mut b = 0usize;
        while b < bytes.len() {
            h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
            b += 1;
        }
        let mut k = 0usize;
        while k < self.syms.len() {
            let nb = self.syms[k].name.as_bytes();
            let mut j = 0usize;
            while j < nb.len() {
                h = (h ^ nb[j] as u64).wrapping_mul(FP_STEP);
                j += 1;
            }
            h = (h ^ self.syms[k].ver.0 as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.syms[k].ver.1 as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.syms[k].addr as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 版本串：GLIBC_2.34。
pub fn ver_text(ma: u32, mi: u32) -> String {
    format!("GLIBC_{}.{}", ma, mi)
}

/// glibc 原文报错（逐字）：prog: lib: version `GLIBC_x.y' not found (required by prog)。
pub fn not_found_text(prog: &str, lib: &str, ma: u32, mi: u32) -> String {
    format!("{}: {}: version `{}' not found (required by {})", prog, lib, ver_text(ma, mi), prog)
}

/// 样本符号表：printf/malloc/memcpy 各带旧版与新版，另加一条单版本符号。
pub fn sample_tab() -> (SymTab, bool) {
    let mut t = SymTab::new(LIBC);
    let mut ok = true;
    let adds: [(&'static str, (u32, u32), u32); 6] = [
        ("printf", (2, 2), 0x1000),
        ("printf", (2, 34), 0x2000),
        ("malloc", (2, 2), 0x3000),
        ("malloc", (2, 34), 0x4000),
        ("memcpy", (2, 14), 0x5000),
        ("__libc_start_main", (2, 34), 0x6000),
    ];
    let mut k = 0usize;
    while k < adds.len() {
        if t.add(adds[k].0, adds[k].1, adds[k].2).is_err() {
            ok = false;
        }
        k += 1;
    }
    (t, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let (t, _) = sample_tab();
    // 1) 旧程序按 2.2 解析到旧地址（不得升级到新版）。
    got[0] = t.resolve("printf", (2, 2)) == Ok(0x1000)
        && t.resolve("printf", (2, 34)) == Ok(0x2000);
    // 2) 版本缺与符号缺分报。
    got[1] = t.resolve("printf", (2, 17)) == Err(SV_MISS)
        && t.resolve("nope", (2, 2)) == Err(SV_NOSYM);
    // 3) 双键重号拒。
    let mut d = SymTab::new(LIBC);
    let _ = d.add("x", (2, 2), 1);
    got[2] = d.add("x", (2, 2), 2).err() == Some(SV_DUP)
        && d.add("x", (2, 3), 3).is_ok();
    // 4) 冻结后登记拒。
    let (mut f, _) = sample_tab();
    f.freeze();
    got[3] = f.add("y", (2, 2), 9).err() == Some(SV_FROZEN);
    got
}

/// 批内判据。
pub fn run_unx11202_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11202");
    let (mut t, built) = sample_tab();

    s.add("J1-样本登记", built && t.syms.len() == 6, "六个版本化符号双键登记成功");
    s.add("J2-旧版精确命中", t.resolve("printf", (2, 2)) == Ok(0x1000),
        "链接 GLIBC_2.2 的程序解析到旧符号");
    s.add("J3-新版精确命中", t.resolve("malloc", (2, 34)) == Ok(0x4000),
        "链接 GLIBC_2.34 的程序解析到新符号");

    let text = not_found_text("./a.out", LIBC, 2, 34);
    let want = "./a.out: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.34' not found (required by ./a.out)";
    s.add("J4-报错逐字一致", text == want, "版本不匹配报错文本与 Linux 逐字相同");

    s.add("J5-版本串格式", ver_text(2, 2) == "GLIBC_2.2" && ver_text(2, 34) == "GLIBC_2.34",
        "版本串 GLIBC_x.y 渲染正确");
    s.add("J6-缺版本分报", t.resolve("memcpy", (2, 2)) == Err(SV_MISS),
        "符号在而版本缺报 SV_MISS");
    s.add("J7-缺符号分报", t.resolve("free", (2, 2)) == Err(SV_NOSYM),
        "符号不在报 SV_NOSYM");
    s.add("J8-最新版解析", t.newest("printf") == Ok(0x2000) && t.newest("memcpy") == Ok(0x5000),
        "未钉版链接取同名最高版本");
    s.add("J9-版本清单", t.versions_of("malloc") == vec![(2, 2), (2, 34)],
        "同名版本清单按登记序枚举");
    s.add("J10-坏版本拒", {
        let mut b = SymTab::new(LIBC);
        b.add("z", (0, 2), 7).err() == Some(SV_BADVER)
    }, "版本主号为零必拒");
    s.add("J11-零地址拒", {
        let mut b = SymTab::new(LIBC);
        b.add("z", (2, 2), 0).err() == Some(SV_ZERO)
    }, "符号地址零值必拒");

    let inj = inject_four();
    s.add("J12-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "旧版解析/两路分报/重键/冻结四路必抓");

    let root = t.freeze();
    s.add("J13-冻结根", t.fold() == root && root != 0 && t.add("w", (2, 2), 5).err() == Some(SV_FROZEN),
        "冻结根稳定且冻结后登记拒");

    let a = t.resolve("printf", (2, 2));
    s.add_dyn("J14-解析明细", a == Ok(0x1000), &format!("printf@2.2={:?}", a));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11202_symver_flow() {
        let (mut t, built) = sample_tab();
        assert!(built);
        assert_eq!(t.resolve("printf", (2, 2)), Ok(0x1000));
        assert_eq!(t.resolve("printf", (2, 34)), Ok(0x2000));
        assert_eq!(t.resolve("printf", (2, 17)), Err(SV_MISS));
        assert_eq!(t.resolve("free", (2, 2)), Err(SV_NOSYM));
        assert_eq!(t.versions_of("printf").len(), 2);
        let root = t.freeze();
        assert_eq!(t.fold(), root);
        assert!(t.add("q", (2, 2), 1).is_err());
        assert!(not_found_text("./x", LIBC, 2, 34).contains("GLIBC_2.34"));
    }

    #[test]
    fn unx11202_checks_green() {
        let s = run_unx11202_checks();
        assert!(s.all_passed(), "UNX-F11202 判据未全绿");
    }
}
