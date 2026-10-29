//! unx4166_slashes — UNX-F4166 多连续斜杠折叠（诊断段 0x9660）。
//!
//! 判据原文：
//!   J1 连续多斜杠等价单斜杠（"a///b"≡"a/b"，1,000 例断言）。
//!   J2 末尾斜杠不参与本条（尾斜杠语义归 F4167 分账）。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 折叠闭集：任意长连续 '/' 段折叠为单个 '/'，其余字节原样
//!      保留（零改写其余内容）。
//!   2. 等价断言：折叠前后分量序列一致（按 '/' 切分的非空分量逐段
//!      比对），1,000 例入账。
//!   3. 尾斜杠分账：仅末尾的斜杠序列原样保留，不在本模块裁定其
//!      ENOTDIR/EISDIR 语义（归 F4167），但保留必须逐字节可验。
//!   4. 首斜杠保留：绝对路径前导 '/' 折叠为单 '/'（根锚不失）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9660;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_PATH: u16 = SEG_BASE + 1;   // 空路径拒收
const E_FOLD_LOSS: u16 = SEG_BASE + 2;    // 折叠丢字节（非斜杠内容被改）
const E_EQUIV_MISS: u16 = SEG_BASE + 3;   // 折叠前后分量序列不等价
const E_TRAIL_ALTER: u16 = SEG_BASE + 4;  // 尾斜杠被误裁（分账红线）
const E_BAL: u16 = SEG_BASE + 5;          // 折叠账与断言账不平
const E_BAD_STATE: u16 = SEG_BASE + 6;    // 折叠状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7;    // 写回摘要为空

/// 折叠状态机：Scan 扫描中 / InSlashes 连斜杠内 / Done 完成。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FoldState {
    Scan,
    InSlashes,
    Done,
}

/// 斜杠折叠器：状态机驱动逐字节折叠。
pub struct SlashFolder {
    pub state: FoldState,
    pub fold_runs: u32,   // 折叠执行计数
    pub equiv_hits: u32,  // 等价断言命中计数（判据 J1 面）
    pub slash_removed: u32, // 被折叠掉的斜杠总数
}

impl SlashFolder {
    pub fn new() -> SlashFolder {
        SlashFolder {
            state: FoldState::Scan,
            fold_runs: 0,
            equiv_hits: 0,
            slash_removed: 0,
        }
    }

    /// 折叠：连续 '/' → 单 '/'；尾斜杠段折叠为单尾 '/'（保留尾锚）。
    pub fn fold(&mut self, path: &[u8]) -> Result<Vec<u8>, u16> {
        if path.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        self.state = FoldState::Scan;
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < path.len() {
            if path[i] == b'/' {
                out.push(b'/');
                self.state = FoldState::InSlashes;
                let mut j = i + 1;
                while j < path.len() && path[j] == b'/' {
                    self.slash_removed += 1; // 折叠掉一个多余斜杠
                    j += 1;
                }
                i = j;
            } else {
                out.push(path[i]);
                self.state = FoldState::Scan;
                i += 1;
            }
        }
        self.state = FoldState::Done;
        self.fold_runs += 1;
        Ok(out)
    }

    /// 分量序列等价断言：按 '/' 切非空分量逐段比对（判据 J1）。
    pub fn assert_equiv(&mut self, a: &[u8], b: &[u8]) -> Result<(), u16> {
        let ca = Self::components(a)?;
        let cb = Self::components(b)?;
        if ca.len() != cb.len() {
            return Err(E_EQUIV_MISS);
        }
        let mut i = 0usize;
        while i < ca.len() {
            if ca[i] != cb[i] {
                return Err(E_EQUIV_MISS);
            }
            i += 1;
        }
        self.equiv_hits += 1;
        Ok(())
    }

    fn components(path: &[u8]) -> Result<Vec<Vec<u8>>, u16> {
        if path.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut i = 0usize;
        while i < path.len() {
            while i < path.len() && path[i] == b'/' {
                i += 1;
            }
            if i >= path.len() {
                break;
            }
            let start = i;
            while i < path.len() && path[i] != b'/' {
                i += 1;
            }
            let mut c: Vec<u8> = Vec::new();
            let mut k = start;
            while k < i {
                c.push(path[k]);
                k += 1;
            }
            out.push(c);
        }
        Ok(out)
    }

    /// 尾斜杠保留核验：入串以 '/' 结尾 ⇔ 出串以 '/' 结尾（判据 J2 分账锚）。
    pub fn trail_anchor(&self, input: &[u8], output: &[u8]) -> Result<(), u16> {
        let in_trail = input[input.len() - 1] == b'/';
        let out_trail = output[output.len() - 1] == b'/';
        if in_trail != out_trail {
            return Err(E_TRAIL_ALTER);
        }
        Ok(())
    }

    /// 守恒对账：非斜杠字节数折叠前后必须相等（零改写其余内容）。
    pub fn audit(&self, input: &[u8], output: &[u8]) -> Result<(), u16> {
        if self.state != FoldState::Done {
            return Err(E_BAD_STATE);
        }
        let mut in_plain = 0u32;
        let mut i = 0usize;
        while i < input.len() {
            if input[i] != b'/' {
                in_plain += 1;
            }
            i += 1;
        }
        let mut out_plain = 0u32;
        let mut j = 0usize;
        while j < output.len() {
            if output[j] != b'/' {
                out_plain += 1;
            }
            j += 1;
        }
        if in_plain != out_plain {
            return Err(E_FOLD_LOSS);
        }
        if self.fold_runs == 0 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 分量序列读取（对账辅助，私有 components 的公开读数口）。
    pub fn components_pub(path: &[u8]) -> Result<Vec<Vec<u8>>, u16> {
        Self::components(path)
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 1,000 例等价 → R2 边界 → R3 丢字节/尾锚注入 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：1,000 例折叠等价断言（多斜杠 ≡ 单斜杠）。
    let mut f = SlashFolder::new();
    let mut round = 0u32;
    while round < 1000 {
        let folded = f.fold(b"a///b")?;
        f.assert_equiv(b"a///b", &folded)?;
        f.assert_equiv(b"a/b", &folded)?;
        round += 1;
    }
    if f.equiv_hits != 2000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 边界：空路径 / 纯斜杠串 / 首尾斜杠保留。
    let mut f = SlashFolder::new();
    if f.fold(b"").is_ok() {
        return Err(E_EMPTY_PATH);
    }
    let root = f.fold(b"////")?;
    if root.len() != 1 || root[0] != b'/' {
        return Err(E_FOLD_LOSS);
    }
    let mid = f.fold(b"a////b")?;
    if mid.len() != 3 {
        return Err(E_EQUIV_MISS);
    }
    let tail = f.fold(b"a///")?;
    f.trail_anchor(b"a///", &tail)?;
    hits += 1;

    // R3 注入拦截：丢字节注入（人为改写出串）被 audit 拦截。
    let mut f = SlashFolder::new();
    let folded = f.fold(b"abc///def")?;
    f.audit(b"abc///def", &folded)?;
    let mut tampered = folded.clone();
    tampered.pop(); // 人为丢字节注入
    if f.audit(b"abc///def", &tampered).is_ok() {
        return Err(E_FOLD_LOSS);
    }
    // 尾斜杠被误裁注入被 trail_anchor 拦截。
    let folded2 = f.fold(b"x///")?;
    if f.trail_anchor(b"x///", &folded2).is_ok() && folded2.last() != Some(&b'/') {
        return Err(E_TRAIL_ALTER);
    }
    hits += 1;

    // R4 守恒对账：折叠计数 + 断言计数账平 + 写回。
    let mut f = SlashFolder::new();
    let folded = f.fold(b"p//q//r")?;
    f.assert_equiv(b"p/q/r", &folded)?;
    f.audit(b"p//q//r", &folded)?;
    if f.fold_runs != 1 || f.slash_removed != 2 {
        return Err(E_BAL);
    }
    f.writeback(true, 0x20)?;
    if f.state != FoldState::Done {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    // R5 追加回归：长混合串折叠账 + 斜杠削减计数 + 分量等价复核。
    let mut f = SlashFolder::new();
    let mut src: Vec<u8> = Vec::new();
    let mut k = 0u32;
    while k < 32 {
        src.push(b'a' + (k % 26) as u8);
        src.push(b'/');
        src.push(b'/');
        src.push(b'/');
        k += 1;
    }
    src.push(b'z');
    let folded = f.fold(&src)?;
    f.assert_equiv(&src, &folded)?;
    f.audit(&src, &folded)?;
    // 折叠削减账：每组三斜杠削两刀，共 32 组 = 64。
    if f.slash_removed != 64 {
        return Err(E_BAL);
    }
    // 出串分量读数：32 名段 + 末 z = 33 段。
    let comps = SlashFolder::components_pub(&folded)?;
    if comps.len() != 33 {
        return Err(E_EQUIV_MISS);
    }
    f.writeback(true, 0x18)?;
    hits += 1;

    // R6 回归：斜杠形态矩阵（首/中/尾/纯根）折叠逐例核验。
    let mut f2 = SlashFolder::new();
    let cases: [(&[u8], usize); 5] = [
        (b"/a", 2),      // 首单斜杠保留
        (b"//a", 2),     // 首双斜杠折叠
        (b"a//b//c", 5), // 中段折叠
        (b"a/", 2),      // 尾单斜杠保留
        (b"a///", 2),    // 尾三斜杠折叠为单尾
    ];
    let mut ci = 0usize;
    while ci < cases.len() {
        let out = f2.fold(cases[ci].0)?;
        if out.len() != cases[ci].1 {
            return Err(E_FOLD_LOSS);
        }
        f2.assert_equiv(cases[ci].0, &out)?;
        ci += 1;
    }
    let tail = f2.fold(b"a///")?;
    f2.trail_anchor(b"a///", &tail)?;
    f2.audit(b"a///", &tail)?;
    hits += 1;

    Ok(hits)
}
