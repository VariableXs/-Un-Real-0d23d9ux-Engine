//! UNX-F11205 dlopen/dlsym 运行期装载语义顶层锚（0x1f805 段）
//!
//! 锚点：UNX-F11205-J1 运行期装载/卸载语义总判据立账（100 轮零泄漏零错位
//! 在 B09 实例化细化），dlopen 错误文本与 Linux 一致。
//! 域：CoRun·UNX-C5-B01——句柄表按路径去重并计数引用，dlsym 只在已装载
//! 映像内解析；dlclose 递减到零才真正卸载，100 轮开-解-关后必须零残留。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f805;
pub const DL_NOFILE: u32 = SEG_BASE + 0x01; // 映像不存在
pub const DL_FLAGS: u32 = SEG_BASE + 0x02; // 绑定模式非法
pub const DL_SYM: u32 = SEG_BASE + 0x03; // 符号不存在
pub const DL_BADH: u32 = SEG_BASE + 0x04; // 句柄非法
pub const DL_LEAK: u32 = SEG_BASE + 0x05; // 卸载后残留
pub const DL_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后开卸
pub const DL_OVER: u32 = SEG_BASE + 0x07; // 句柄超容

pub const RTLD_LAZY: u32 = 0x00001;
pub const RTLD_NOW: u32 = 0x00002;
pub const RTLD_GLOBAL: u32 = 0x00100;
pub const MAX_HANDLES: usize = 8;
pub const ROUNDS: u32 = 100;
pub const DL_TAG: u64 = 0x1f805c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 已装载映像：路径、符号清单、构造计数、打开引用数。
#[derive(Clone, Debug)]
pub struct Image {
    pub path: &'static str,
    pub syms: Vec<(&'static str, u32)>,
    pub refs: u32,
    pub ctors: u32,
}

impl Image {
    pub fn sym(&self, name: &str) -> Option<u32> {
        let mut k = 0usize;
        while k < self.syms.len() {
            if self.syms[k].0 == name {
                return Some(self.syms[k].1);
            }
            k += 1;
        }
        None
    }
}

/// 装载器：映像表 + 句柄账 + 错误文本。
#[derive(Clone, Debug)]
pub struct Loader {
    pub images: Vec<Image>,
    pub opens: u32,
    pub closes: u32,
    pub resolves: u32,
    pub last_error: String,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Loader {
    pub fn new() -> Loader {
        Loader {
            images: Vec::new(),
            opens: 0,
            closes: 0,
            resolves: 0,
            last_error: String::new(),
            frozen: false,
            root: None,
        }
    }

    /// 注册映像（装载器视野内的可装载集）。
    pub fn register(&mut self, path: &'static str, syms: &[(&'static str, u32)]) -> Result<(), u32> {
        if self.frozen {
            return Err(DL_FROZEN);
        }
        if self.images.len() >= MAX_HANDLES {
            return Err(DL_OVER);
        }
        let mut v: Vec<(&'static str, u32)> = Vec::new();
        let mut k = 0usize;
        while k < syms.len() {
            v.push(syms[k]);
            k += 1;
        }
        self.images.push(Image { path, syms: v, refs: 0, ctors: 0 });
        Ok(())
    }

    /// dlopen：同路径复用句柄并增引用；缺文件落 Linux 原文并拒。
    pub fn dlopen(&mut self, path: &str, flags: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(DL_FROZEN);
        }
        if flags & (RTLD_LAZY | RTLD_NOW) == 0 {
            self.last_error = format!("invalid mode for dlopen(): Invalid argument");
            return Err(DL_FLAGS);
        }
        let mut k = 0usize;
        while k < self.images.len() {
            if self.images[k].path == path {
                self.images[k].refs = self.images[k].refs.wrapping_add(1);
                self.opens = self.opens.wrapping_add(1);
                return Ok(k as u32);
            }
            k += 1;
        }
        self.last_error = format!("{}: cannot open shared object file: No such file or directory", path);
        Err(DL_NOFILE)
    }

    /// dlsym：句柄越界、符号缺失两路拒。
    pub fn dlsym(&mut self, handle: u32, name: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(DL_FROZEN);
        }
        let idx = handle as usize;
        if idx >= self.images.len() {
            self.last_error = "undefined symbol: (null)".to_string();
            return Err(DL_BADH);
        }
        match self.images[idx].sym(name) {
            Some(a) => {
                self.resolves = self.resolves.wrapping_add(1);
                Ok(a)
            }
            None => {
                self.last_error = format!("{}: undefined symbol: {}", self.images[idx].path, name);
                Err(DL_SYM)
            }
        }
    }

    /// dlclose：递减引用，归零才销毁。
    pub fn dlclose(&mut self, handle: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(DL_FROZEN);
        }
        let idx = handle as usize;
        if idx >= self.images.len() {
            return Err(DL_BADH);
        }
        if self.images[idx].refs == 0 {
            return Err(DL_BADH);
        }
        self.images[idx].refs = self.images[idx].refs.wrapping_sub(1);
        self.closes = self.closes.wrapping_add(1);
        Ok(self.images[idx].refs)
    }

    /// 残留引用总数（零泄漏判据）。
    pub fn live_refs(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.images.len() {
            t = t.saturating_add(self.images[k].refs);
            k += 1;
        }
        t
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = DL_TAG;
        let mut k = 0usize;
        while k < self.images.len() {
            let bytes = self.images[k].path.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.images[k].refs as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.images[k].ctors as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.opens as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.closes as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.resolves as u64).wrapping_mul(FP_STEP);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 样本装载器：两个映像，含同名符号与缺失符号样本。
pub fn sample_loader() -> (Loader, bool) {
    let mut l = Loader::new();
    let a: [(&'static str, u32); 3] = [("alpha", 0x10), ("beta", 0x20), ("gamma", 0x30)];
    let b: [(&'static str, u32); 2] = [("alpha", 0x99), ("delta", 0x40)];
    let ok = l.register("/lib/liba.so", &a).is_ok() && l.register("/lib/libb.so", &b).is_ok();
    (l, ok)
}

/// 100 轮开-解-关：每轮 dlopen(LAZY) → dlsym(alpha) → dlclose。
pub fn run_rounds(l: &mut Loader, rounds: u32) -> Result<u32, u32> {
    if rounds == 0 {
        return Err(DL_FLAGS);
    }
    let mut i = 0u32;
    let mut last = 0u32;
    while i < rounds {
        let h = l.dlopen("/lib/liba.so", RTLD_LAZY)?;
        last = l.dlsym(h, "alpha")?;
        let r = l.dlclose(h)?;
        if r != 0 {
            return Err(DL_LEAK);
        }
        i += 1;
    }
    Ok(last)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let (mut l, _) = sample_loader();
    // 1) 缺文件报 Linux 原文且拒。
    got[0] = l.dlopen("/lib/none.so", RTLD_NOW).err() == Some(DL_NOFILE)
        && l.last_error == "/lib/none.so: cannot open shared object file: No such file or directory";
    // 2) 绑定模式零值拒。
    got[1] = l.dlopen("/lib/liba.so", 0).err() == Some(DL_FLAGS);
    // 3) 未装载符号报 undefined symbol 且拒。
    let h = l.dlopen("/lib/liba.so", RTLD_LAZY).unwrap_or(0);
    got[2] = l.dlsym(h, "zeta").err() == Some(DL_SYM)
        && l.last_error.ends_with("undefined symbol: zeta");
    // 4) 引用归零后再关必拒。
    let _ = l.dlclose(h);
    got[3] = l.dlclose(h).err() == Some(DL_BADH) && l.live_refs() == 0;
    got
}

/// 批内判据。
pub fn run_unx11205_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11205");
    let (mut l, built) = sample_loader();

    let run = run_rounds(&mut l, ROUNDS);
    s.add("J1-百轮零泄漏", built && run == Ok(0x10) && l.live_refs() == 0,
        "100 轮开-解-关后引用残留为零");
    s.add("J2-开关守恒", l.opens == ROUNDS && l.closes == ROUNDS,
        "打开与关闭计数各为 100");
    s.add("J3-解析计数", l.resolves == ROUNDS, "dlsym 成功计数与轮数一致");

    let j4 = {
        let mut c = Loader::new();
        let a: [(&'static str, u32); 1] = [("f", 1)];
        let _ = c.register("/lib/x.so", &a);
        let h1 = c.dlopen("/lib/x.so", RTLD_NOW);
        let h2 = c.dlopen("/lib/x.so", RTLD_LAZY);
        h1 == h2 && h1 == Ok(0) && c.images[0].refs == 2
    };
    s.add("J4-同路径复用", j4, "同路径二次打开复用句柄并增引用");

    let j5 = {
        let (mut c, _) = sample_loader();
        let h = c.dlopen("/lib/libb.so", RTLD_NOW).unwrap_or(9);
        c.dlsym(h, "alpha") == Ok(0x99) && c.dlsym(h, "delta") == Ok(0x40)
    };
    s.add("J5-按句柄解析", j5, "dlsym 只在指句柄映像内解析（同名不串库）");

    let j6 = {
        let (mut c, _) = sample_loader();
        c.dlsym(9, "alpha").err() == Some(DL_BADH)
    };
    s.add("J6-句柄越界拒", j6, "越界句柄 dlsym 必拒");

    let j7 = {
        let (mut c, _) = sample_loader();
        c.dlclose(9).err() == Some(DL_BADH)
    };
    s.add("J7-关闭越界拒", j7, "越界句柄 dlclose 必拒");

    s.add("J8-缺符号文本", {
        let (mut c, _) = sample_loader();
        let h = c.dlopen("/lib/liba.so", RTLD_LAZY).unwrap_or(0);
        c.dlsym(h, "nope").err() == Some(DL_SYM)
            && c.last_error == "/lib/liba.so: undefined symbol: nope"
    }, "缺符号错误文本含库路径与符号名");

    s.add("J9-引用递减", {
        let (mut c, _) = sample_loader();
        let h = c.dlopen("/lib/liba.so", RTLD_LAZY).unwrap_or(0);
        let _ = c.dlopen("/lib/liba.so", RTLD_LAZY);
        c.dlclose(h) == Ok(1) && c.dlclose(h) == Ok(0)
    }, "两次打开两次关闭引用依次递减至零");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "缺文件/坏模式/缺符号/二次关闭四路必抓");

    let root = l.freeze();
    s.add("J11-冻结根", l.fold() == root && root != 0, "冻结后指纹可重算");
    s.add("J12-冻结拒开", l.dlopen("/lib/liba.so", RTLD_NOW).err() == Some(DL_FROZEN)
        && l.dlsym(0, "alpha").err() == Some(DL_FROZEN), "冻结后开/解均拒");

    let fps = l.fold();
    s.add_dyn("J13-装载明细", l.live_refs() == 0 && fps == root,
        &format!("opens={} refs={}", l.opens, l.live_refs()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11205_dlopen_flow() {
        let (mut l, built) = sample_loader();
        assert!(built);
        assert_eq!(run_rounds(&mut l, 10), Ok(0x10));
        assert_eq!(l.live_refs(), 0);
        assert_eq!(l.opens, 10);
        assert_eq!(l.dlopen("/lib/none.so", RTLD_NOW), Err(DL_NOFILE));
        assert!(l.last_error.contains("cannot open shared object file"));
        let h = l.dlopen("/lib/libb.so", RTLD_NOW).unwrap_or(9);
        assert_eq!(l.dlsym(h, "delta"), Ok(0x40));
        assert_eq!(l.dlsym(h, "zeta"), Err(DL_SYM));
        let root = l.freeze();
        assert_eq!(l.freeze(), root);
        assert!(l.dlopen("/lib/liba.so", RTLD_LAZY).is_err());
    }

    #[test]
    fn unx11205_checks_green() {
        let s = run_unx11205_checks();
        assert!(s.all_passed(), "UNX-F11205 判据未全绿");
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn fresh_image_symbol_and_unload() {
        let mut l = Loader::new();
        let syms: [(&'static str, u32); 2] = [("omega", 0x50), ("psi", 0x60)];
        assert!(l.register("/lib/libx.so", &syms).is_ok());
        assert_eq!(l.dlopen("/lib/libx.so", 0), Err(DL_FLAGS));
        let h = match l.dlopen("/lib/libx.so", RTLD_NOW | RTLD_GLOBAL) {
            Ok(h) => h,
            Err(_) => 9,
        };
        assert_eq!(h, 0);
        assert_eq!(l.dlsym(h, "omega"), Ok(0x50));
        assert_eq!(l.dlsym(h, "psi"), Ok(0x60));
        assert_eq!(l.dlsym(h, "chi"), Err(DL_SYM));
        assert_eq!(l.dlclose(h), Ok(0));
        assert_eq!(l.live_refs(), 0);
        assert_eq!(l.dlclose(h), Err(DL_BADH));
        assert_eq!(l.dlopen("/lib/none.so", RTLD_LAZY), Err(DL_NOFILE));
    }
}
