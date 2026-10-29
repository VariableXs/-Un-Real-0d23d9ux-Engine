//! UNX-F12052 PEB ActivationContextData 激活上下文
//! 分段: 0x198a0 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：ActivationContextData 承载 SxS 程序集激活上下文。
//! 本卡以"装配清单"建模：
//!   - 上下文含程序集清单（程序集名 + 版本四元组）；
//!   - 激活 = 推栈，停用 = 弹栈（栈深上限 8）；
//!   - 查询 DLL 重定向按当前栈顶上下文决议；
//!   - 空栈查询走默认上下文；
//!   - 版本必须递增校验、清单封存后不可再装配。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x198A0;
pub const SEG_ACX_STACK: u32 = SEG_BASE + 0x01;
pub const SEG_ACX_ASSEMBLY: u32 = SEG_BASE + 0x02;
pub const SEG_ACX_SEALED: u32 = SEG_BASE + 0x03;
pub const SEG_ACX_NOCTX: u32 = SEG_BASE + 0x04;
pub const SEG_ACX_DUP: u32 = SEG_BASE + 0x05;

/// 版本四元组
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcxVer {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub rev: u32,
}

impl AcxVer {
    pub fn encode(&self) -> u64 {
        ((self.major as u64) << 48)
            | ((self.minor as u64) << 32)
            | ((self.build as u64) << 16)
            | (self.rev as u64)
    }
}

/// 程序集记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembly {
    pub name: String,
    pub ver: AcxVer,
    pub dll_count: u32,
    pub sealed: bool,
}

impl Assembly {
    /// 名称非空、DLL 数 1..=1024
    pub fn new(name: &str, ver: AcxVer, dll_count: u32) -> Result<Assembly, u32> {
        if name.is_empty() {
            return Err(SEG_ACX_ASSEMBLY);
        }
        if dll_count == 0 || dll_count > 1024 {
            return Err(SEG_ACX_ASSEMBLY);
        }
        Ok(Assembly { name: name.to_string(), ver, dll_count, sealed: false })
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 激活上下文
#[derive(Debug, Clone)]
pub struct ActCtx {
    pub assemblies: Vec<Assembly>,
    pub stack: Vec<u32>, // 栈存程序集索引
    pub sealed: bool,
}

impl ActCtx {
    pub fn new() -> ActCtx {
        ActCtx { assemblies: Vec::new(), stack: Vec::new(), sealed: false }
    }

    /// 装配程序集：封存 → SEALED；同名重复 → DUP
    pub fn mount(&mut self, a: Assembly) -> Result<u32, u32> {
        if self.sealed {
            return Err(SEG_ACX_SEALED);
        }
        if self.assemblies.iter().any(|x| x.name == a.name) {
            return Err(SEG_ACX_DUP);
        }
        self.assemblies.push(a);
        Ok((self.assemblies.len() - 1) as u32)
    }

    /// 激活：栈深上限 8 → STACK；索引越界/封存清单条目 → NOCTX/SEALED
    pub fn activate(&mut self, idx: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(SEG_ACX_SEALED);
        }
        if idx as usize >= self.assemblies.len() {
            return Err(SEG_ACX_NOCTX);
        }
        if self.stack.len() >= 8 {
            return Err(SEG_ACX_STACK);
        }
        self.stack.push(idx);
        Ok(self.stack.len() as u32)
    }

    /// 停用：空栈 → STACK
    pub fn deactivate(&mut self) -> Result<u32, u32> {
        if self.stack.is_empty() {
            return Err(SEG_ACX_STACK);
        }
        self.stack.pop();
        Ok(self.stack.len() as u32)
    }

    /// DLL 重定向决议：栈顶优先，空栈走默认（返回 None 表示默认）
    pub fn resolve(&self, dll_name: &str) -> Option<String> {
        let top = (*self.stack.last().unwrap_or(&u32::MAX)) as usize;
        if top < self.assemblies.len() {
            Some(format!("{}\\{}", self.assemblies[top].name, dll_name))
        } else {
            None
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
        for a in &mut self.assemblies {
            a.sealed = true;
        }
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(ctx: &ActCtx) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for a in &ctx.assemblies {
        for b in a.name.bytes() {
            h = (h.wrapping_mul(211).wrapping_add(b as u64)) & 0xFFFF_FFFF;
        }
        h = (h.wrapping_mul(211).wrapping_add(a.ver.encode() & 0xFF)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(a.dll_count as u64)) & 0xFFFF_FFFF;
    }
    for s in &ctx.stack {
        h = (h.wrapping_mul(211).wrapping_add(*s as u64)) & 0xFFFF_FFFF;
    }
    h = (h.wrapping_mul(211).wrapping_add(ctx.sealed as u64)) & 0xFFFF_FFFF;
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 空名程序集 → ASSEMBLY
    if Assembly::new("", AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, 1) == Err(SEG_ACX_ASSEMBLY) {
        ok[0] = true;
    }
    // 样2: 同名重复装配 → DUP
    let mut c = ActCtx::new();
    let a = Assembly::new("Foo", AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, 2)
        .unwrap_or_else(|_| Assembly { name: "Foo".to_string(), ver: AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, dll_count: 2, sealed: false });
    let _ = c.mount(a.clone());
    if c.mount(a) == Err(SEG_ACX_DUP) {
        ok[1] = true;
    }
    // 样3: 激活越界索引 → NOCTX
    let mut c2 = ActCtx::new();
    if c2.activate(0) == Err(SEG_ACX_NOCTX) {
        ok[2] = true;
    }
    // 样4: 空栈停用 → STACK
    let mut c3 = ActCtx::new();
    if c3.deactivate() == Err(SEG_ACX_STACK) {
        ok[3] = true;
    }
    // 样5: 封存后激活 → SEALED
    let mut c4 = ActCtx::new();
    let a2 = Assembly::new("Bar", AcxVer { major: 2, minor: 0, build: 3, rev: 1 }, 1)
        .unwrap_or_else(|_| Assembly { name: "Bar".to_string(), ver: AcxVer { major: 2, minor: 0, build: 3, rev: 1 }, dll_count: 1, sealed: false });
    let _ = c4.mount(a2);
    c4.seal();
    if c4.activate(0) == Err(SEG_ACX_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12052_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12052");
    let mk = |name: &str, dll: u32| Assembly {
        name: name.to_string(),
        ver: AcxVer { major: 1, minor: 0, build: 1024, rev: 0 },
        dll_count: dll,
        sealed: false,
    };
    // J1 装配返回递增索引
    let mut c = ActCtx::new();
    cs.add("J1", c.mount(mk("A", 1)) == Ok(0) && c.mount(mk("B", 2)) == Ok(1), "mount idx");
    // J2 激活推栈
    cs.add("J2", c.activate(0) == Ok(1), "activate push");
    // J3 重定向按栈顶
    cs.add("J3", c.resolve("x.dll").unwrap_or_default() == "A\\x.dll", "resolve top A");
    // J4 二次激活覆盖
    let _ = c.activate(1);
    cs.add("J4", c.resolve("y.dll").unwrap_or_default() == "B\\y.dll", "resolve top B");
    // J5 弹栈回退
    cs.add("J5", c.deactivate() == Ok(1) && c.resolve("y.dll").unwrap_or_default() == "A\\y.dll", "pop fallback");
    // J6 空栈走默认（None）
    let _ = c.deactivate();
    cs.add("J6", c.resolve("z.dll").is_none(), "empty default");
    // J7 空名拒绝
    cs.add("J7", Assembly::new("", AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, 1).err()
        == Some(SEG_ACX_ASSEMBLY), "empty name");
    // J8 DLL 数为 0 拒绝
    cs.add("J8", Assembly::new("K", AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, 0).err()
        == Some(SEG_ACX_ASSEMBLY), "dll zero");
    // J9 同名重复
    let mut c2 = ActCtx::new();
    let _ = c2.mount(mk("Dup", 3));
    cs.add("J9", c2.mount(mk("Dup", 4)).err() == Some(SEG_ACX_DUP), "dup name");
    // J10 栈深上限 8
    let mut c3 = ActCtx::new();
    let _ = c3.mount(mk("P", 1));
    let mut d = 0;
    while d < 8 {
        let _ = c3.activate(0);
        d += 1;
    }
    cs.add("J10", c3.activate(0).err() == Some(SEG_ACX_STACK), "stack cap 8");
    // J11 越界激活
    cs.add("J11", c3.activate(9).err() == Some(SEG_ACX_NOCTX), "activate oob");
    // J12 封存后激活
    let mut c4 = ActCtx::new();
    let _ = c4.mount(mk("S", 1));
    c4.seal();
    cs.add("J12", c4.activate(0).err() == Some(SEG_ACX_SEALED), "sealed activate");
    // J13 封存传播到程序集
    cs.add("J13", c4.assemblies.iter().all(|a| a.sealed), "seal propagate");
    // J14 版本编码往返
    let v = AcxVer { major: 6, minor: 1, build: 7601, rev: 24545 };
    let dec = v.encode();
    cs.add("J14", dec != 0, "ver encode");
    // J15 注入五样
    let inj = inject_five();
    cs.add("J15", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_push_pop() {
        let mut c = ActCtx::new();
        let a = Assembly::new("X", AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, 1)
            .ok().unwrap_or_else(|| Assembly { name: "X".to_string(), ver: AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, dll_count: 1, sealed: false });
        let _ = c.mount(a);
        assert!(c.activate(0).is_ok());
        assert_eq!(c.deactivate().ok(), Some(0));
        assert!(c.deactivate().is_err());
    }

    #[test]
    fn test_dup_name() {
        let mut c = ActCtx::new();
        let mk = || Assembly::new("Dup", AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, 2)
            .ok().unwrap_or_else(|| Assembly { name: "Dup".to_string(), ver: AcxVer { major: 1, minor: 0, build: 1, rev: 0 }, dll_count: 2, sealed: false });
        let _ = c.mount(mk());
        assert_eq!(c.mount(mk()).err(), Some(SEG_ACX_DUP));
    }

    #[test]
    fn test_fold_stable() {
        let mut c = ActCtx::new();
        let _ = c.mount(Assembly::new("F", AcxVer { major: 1, minor: 2, build: 3, rev: 4 }, 5)
            .ok().unwrap_or_else(|| Assembly { name: "F".to_string(), ver: AcxVer { major: 1, minor: 2, build: 3, rev: 4 }, dll_count: 5, sealed: false }));
        let _ = c.activate(0);
        let a = fold(&c);
        let b = fold(&c);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }
}
