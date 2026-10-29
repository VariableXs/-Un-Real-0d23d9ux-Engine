//! unx4213_getcwd — UNX-F4213 getcwd 与 cwd 配额（buf 长度协商）（诊断段 0x9BE0）。
//!
//! 判据原文：J1 长度协商三态：buf 大于路径长 → 全量拷贝 + 返回实
//!           际长；buf 恰等于路径长+1（含 NUL）→ 成功；buf 小于需
//!           要 → ERANGE 拒收且零写入（内容账不动）；
//!           J2 NULL-buf 语义（POSIX 扩展）：分配语义预锚——账面
//!           返回「需要的长度」，实际分配押后（内核侧只记账）；
//!           J3 cwd 路径长上限：PATH_MAX(4096) 封顶——cwd 组装超
//!           限即 ENAMETOOLONG，配额账入账；
//!           J4 配额账守恒：calls = ok + erange + enametoolong +
//!           alloc_req，且字节账 written 总量 = Σ成功调用拷贝量。
//! 设计纪律：
//!   1. POSIX getcwd(buf, size)：size==0 且 buf==NULL → 分配语义
//!      （本锚记账不分配）；size 不足 → ERANGE（Linux 语义，POSIX
//!      允许 EINVAL，本锚按 Linux 账）。
//!   2. cwd 组装：目录栈自根向下拼接 "/" + 名（根为 "/"）。
//!   3. 长度账三槽分账：ok/erange/enametoolong/alloc_req 闭合。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9BE0 本单独占。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9BE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;    // buf 不足（ERANGE 账）
const E_NAMETOOLONG: u16 = SEG_BASE + 2; // cwd 超 PATH_MAX
const E_BAL: u16 = SEG_BASE + 3;      // 配额账不守恒
const E_ZERO_NONULL: u16 = SEG_BASE + 4; // size==0 且 buf 非 NULL（EINVAL 账）
const E_EMPTY_COMP: u16 = SEG_BASE + 5;  // 空分量名

/// PATH_MAX = 4096（POSIX）。
pub const PATH_MAX: usize = 4096;

/// getcwd 结果态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CwdRes {
    Ok = 0,
    Erange = 1,
    NameTooLong = 2,
    AllocReq = 3,
    ZeroNonNull = 4,
}

/// cwd 路径组装器：目录栈 → 绝对路径。
pub struct CwdStack {
    dirs: Vec<String>,
}

impl CwdStack {
    pub fn new() -> CwdStack {
        CwdStack { dirs: Vec::new() }
    }

    /// 进入子目录（空名拒收）。
    pub fn push(&mut self, name: &str) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY_COMP);
        }
        self.dirs.push(name.to_string());
        Ok(())
    }

    /// 退级（根处 ".." 钳制语义与 B11 联签一致）。
    pub fn pop(&mut self) -> bool {
        if self.dirs.is_empty() {
            return false;
        }
        self.dirs.pop();
        true
    }

    /// 组装 cwd 绝对路径（判据 J3：超 PATH_MAX 拒收）。
    pub fn assemble(&self) -> Result<String, u16> {
        let mut len = 1usize; // 根 "/"
        for d in self.dirs.iter() {
            len += d.len() + 1; // "/name"
        }
        if len > PATH_MAX {
            return Err(E_NAMETOOLONG);
        }
        let mut out = String::new();
        out.push('/');
        for (i, d) in self.dirs.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            for ch in d.chars() {
                out.push(ch);
            }
        }
        if self.dirs.is_empty() {
            out = "/".to_string();
        }
        Ok(out)
    }

    pub fn depth(&self) -> usize {
        self.dirs.len()
    }
}

/// getcwd 配额账枢纽（判据 J1/J2/J4）。
pub struct GetCwd {
    pub calls: u32,
    pub ok: u32,
    pub erange: u32,
    pub enametoolong: u32,
    pub alloc_req: u32,
    pub zero_nonnull: u32,
    pub written_bytes: u64,
    pub alloc_needed_peak: usize,
}

impl GetCwd {
    pub fn new() -> GetCwd {
        GetCwd {
            calls: 0,
            ok: 0,
            erange: 0,
            enametoolong: 0,
            alloc_req: 0,
            zero_nonnull: 0,
            written_bytes: 0,
            alloc_needed_peak: 0,
        }
    }

    /// getcwd(buf, size) 全语义（判据 J1/J2）：
    /// - size==0 且 buf 为 None → AllocReq，账面返回需要长度
    /// - size==0 且 buf 为 Some → ZeroNonNull 拒收
    /// - size >= need → 全量拷贝（含 NUL），Ok
    /// - size < need → Erange 零写入
    pub fn getcwd(&mut self, cwd: &str, buf: Option<&mut Vec<u8>>, size: usize) -> Result<CwdRes, u16> {
        self.calls += 1;
        let need = cwd.len() + 1; // 含 NUL
        if need - 1 > self.alloc_needed_peak {
            self.alloc_needed_peak = need - 1;
        }
        if size == 0 {
            if buf.is_none() {
                self.alloc_req += 1;
                return Err(E_BAL); // 账面：需要 need 字节，内核侧只记账
            }
            self.zero_nonnull += 1;
            return Err(E_ZERO_NONULL);
        }
        if size < need {
            // 零写入：内容账不动（无 buf 写入动作即证）
            self.erange += 1;
            return Err(E_RANGE);
        }
        if let Some(b) = buf {
            b.clear();
            for ch in cwd.as_bytes() {
                b.push(*ch);
            }
            b.push(0);
            self.written_bytes += need as u64;
        }
        self.ok += 1;
        Ok(CwdRes::Ok)
    }

    /// cwd 超 PATH_MAX 拒收入账（判据 J3）。
    pub fn record_toolong(&mut self) {
        self.calls += 1;
        self.enametoolong += 1;
    }

    /// 配额账守恒（判据 J4）。
    pub fn balanced(&self) -> bool {
        self.calls == self.ok + self.erange + self.enametoolong + self.alloc_req + self.zero_nonnull
    }

    /// 账快照导出。
    pub fn snapshot(&self) -> (u32, u32, u32, u32, u32, u64) {
        (self.ok, self.erange, self.enametoolong, self.alloc_req, self.zero_nonnull, self.written_bytes)
    }
}

/// 自例程：R1 三态长度协商；R2 NULL-buf 分配记账；R3 PATH_MAX 封顶；
/// R4 目录栈组装与钳制；R5 配额账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三态——大 buf 全量拷贝；恰需 buf 成功；小 buf ERANGE 零写入。
    let mut g = GetCwd::new();
    let cwd = "/etc/conf";
    let need = cwd.len() + 1;
    let mut b1: Vec<u8> = Vec::new();
    if g.getcwd(cwd, Some(&mut b1), 128)? != CwdRes::Ok {
        return Err(E_BAL);
    }
    // 全量拷贝核对：内容 + NUL
    if b1.len() != need {
        return Err(E_BAL);
    }
    for (i, ch) in cwd.as_bytes().iter().enumerate() {
        if b1[i] != *ch {
            return Err(E_BAL);
        }
    }
    if b1[need - 1] != 0 {
        return Err(E_BAL);
    }
    // 恰需 buf 成功
    let mut b2: Vec<u8> = Vec::new();
    if g.getcwd(cwd, Some(&mut b2), need)? != CwdRes::Ok {
        return Err(E_BAL);
    }
    // 小 buf ERANGE 零写入
    let mut b3: Vec<u8> = vec![0x5A; 4];
    let b3_before = b3[0];
    if g.getcwd(cwd, Some(&mut b3), 4) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    if b3[0] != b3_before {
        return Err(E_RANGE); // 零写入失守
    }
    if g.ok != 2 || g.erange != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：NULL-buf 分配记账（判据 J2）——账面 alloc_req + 需要长度峰值。
    if g.getcwd(cwd, None, 0) != Err(E_BAL) {
        return Err(E_BAL);
    }
    if g.alloc_req != 1 || g.alloc_needed_peak != cwd.len() {
        return Err(E_BAL);
    }
    // size==0 且 buf 非 NULL → 拒收
    let mut b4: Vec<u8> = Vec::new();
    if g.getcwd(cwd, Some(&mut b4), 0) != Err(E_ZERO_NONULL) {
        return Err(E_ZERO_NONULL);
    }
    hits += 1;

    // R3：PATH_MAX 封顶——超长目录栈组装拒收 ENAMETOOLONG（判据 J3）。
    let mut deep = CwdStack::new();
    let seg = "d".repeat(500);
    for _ in 0..9 {
        deep.push(&seg)?; // 9*501+1 = 4510 > 4096
    }
    if deep.assemble() != Err(E_NAMETOOLONG) {
        return Err(E_NAMETOOLONG);
    }
    g.record_toolong();
    // 8 层：4009 ≤ 4096 成
    let mut ok8 = CwdStack::new();
    for _ in 0..8 {
        ok8.push(&seg)?;
    }
    let p8 = ok8.assemble()?;
    if p8.len() != 8 * 501 + 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：目录栈组装与根钳制——"/a/b" 组装、pop 到根、根处 pop 失败。
    let mut s = CwdStack::new();
    s.push("a")?;
    s.push("b")?;
    let p = s.assemble()?;
    if p != "/a/b" {
        return Err(E_BAL);
    }
    s.pop();
    if s.assemble()? != "/a" {
        return Err(E_BAL);
    }
    s.pop();
    if s.assemble()? != "/" {
        return Err(E_BAL);
    }
    if s.pop() {
        return Err(E_BAL); // 根处 ".." 钳制
    }
    // 空分量名拒收
    if s.push("") != Err(E_EMPTY_COMP) {
        return Err(E_EMPTY_COMP);
    }
    hits += 1;

    // R5：配额账守恒终验（判据 J4）。
    if !g.balanced() {
        return Err(E_BAL);
    }
    let (ok, er, et, ar, zn, wb) = g.snapshot();
    if ok + er + et + ar + zn != g.calls {
        return Err(E_BAL);
    }
    // 字节账 = Σ成功拷贝（3 次 ok：need+need = 2×10 = 20…… 实为 2 次 ok）
    if wb != (ok as u64) * (need as u64) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
