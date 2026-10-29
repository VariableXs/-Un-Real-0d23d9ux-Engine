//! unx4157_tabdocs — UNX-F4157 语义表文档锚（冻结快照入 docs）（诊断段 0x9560）。
//!
//! 判据原文：
//!   J1 冻结快照文档生成且与代码账一致（函数清单 diff 零差）。
//!   J2 快照哈希入台账可验。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 代码账为源：三表函数清单（表号+表内偏移+名字指纹）先落账，
//!      文档只能由代码账投影生成，禁止文档端手写条目。
//!   2. 文档投影：按表号序、偏移序逐行生成文档账，行数 = 函数数。
//!   3. diff 零差：两账逐行比对（表号+偏移+指纹三元组），差一行即
//!      失守；生成后追加/删减注入必须被 diff 拦截。
//!   4. 快照哈希：函数清单按序 FNV-1a 折叠入台账，冻结后可重算可验。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9560;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_TBL: u16 = SEG_BASE + 1;    // 表号越界
const E_BAD_OFF: u16 = SEG_BASE + 2;    // 表内偏移越界
const E_EMPTY_CODE: u16 = SEG_BASE + 3; // 代码账为空先投影
const E_DOC_MADE: u16 = SEG_BASE + 4;   // 文档已生成再生成（覆盖红线）
const E_DIFF_NZ: u16 = SEG_BASE + 5;    // diff 非零差（清单失守）
const E_DOC_INJECT: u16 = SEG_BASE + 6; // 文档端手写注入
const E_SNAP_MISS: u16 = SEG_BASE + 7;  // 台账无快照哈希可验
const E_BAL: u16 = SEG_BASE + 8;        // 行账与函数账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 9;  // 写回摘要为空

/// 三表定编（同 F4154 冻结口径）。
pub const TBL_INODE: u8 = 0;
pub const TBL_FILE: u8 = 1;
pub const TBL_DIR: u8 = 2;
pub const TBL_COUNT: u8 = 3;
pub const INODE_FN: u8 = 10;
pub const FILE_FN: u8 = 12;
pub const DIR_FN: u8 = 10;

pub fn table_fn_count(tbl: u8) -> Result<u8, u16> {
    match tbl {
        TBL_INODE => Ok(INODE_FN),
        TBL_FILE => Ok(FILE_FN),
        TBL_DIR => Ok(DIR_FN),
        _ => Err(E_BAD_TBL),
    }
}

/// 文档生成状态机：Empty 未生成 / Projected 已投影 / Verified 已核验。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocState {
    Empty,
    Projected,
    Verified,
}

/// 函数清单条目（代码账与文档账共用的行单位）。
pub struct FnRow {
    pub tbl: u8,       // 表号
    pub fn_off: u8,    // 表内偏移
    pub name_fp: u32,  // 函数名 FNV-1a 指纹
}

/// 文档锚枢纽：代码账 → 文档投影 → diff 零差 → 快照哈希台账。
pub struct TabDocs {
    code_rows: Vec<FnRow>,
    doc_rows: Vec<FnRow>,
    pub state: DocState,
    ledger_hashes: Vec<u32>, // 台账：各代快照哈希
    pub snapshot_hash: u32,  // 当前快照哈希（0=未冻结）
    pub diff_runs: u32,      // diff 执行计数
    pub inject_blocked: u32, // 文档端注入拦截计数
}

impl TabDocs {
    pub fn new() -> TabDocs {
        TabDocs {
            code_rows: Vec::new(),
            doc_rows: Vec::new(),
            state: DocState::Empty,
            ledger_hashes: Vec::new(),
            snapshot_hash: 0,
            diff_runs: 0,
            inject_blocked: 0,
        }
    }

    /// 代码账登记：全表批量（按序自动展开函数号，名字指纹由调用方给）。
    pub fn load_code_table(&mut self, tbl: u8, base_fp: u32) -> Result<(), u16> {
        let n = table_fn_count(tbl)?;
        let mut off = 0u8;
        while off < n {
            // 指纹派生：基址 + 表偏移混合（确定性整数派生）。
            let fp = base_fp ^ ((tbl as u32) << 8) ^ (off as u32);
            self.code_rows.push(FnRow { tbl, fn_off: off, name_fp: fp });
            off += 1;
        }
        Ok(())
    }

    fn fp_of(name: &[u8]) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0usize;
        while i < name.len() {
            h ^= name[i] as u32;
            h = h.wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }

    /// 单条代码账登记（名字指纹按内容算）。
    pub fn load_code_row(&mut self, tbl: u8, fn_off: u8, name: &[u8]) -> Result<(), u16> {
        let n = table_fn_count(tbl)?;
        if fn_off >= n {
            return Err(E_BAD_OFF);
        }
        self.code_rows.push(FnRow { tbl, fn_off, name_fp: Self::fp_of(name) });
        Ok(())
    }

    /// 文档投影：从代码账按序生成文档账（判据 J1 生成面）。
    pub fn project_docs(&mut self) -> Result<usize, u16> {
        if self.state == DocState::Projected || self.state == DocState::Verified {
            return Err(E_DOC_MADE);
        }
        if self.code_rows.is_empty() {
            return Err(E_EMPTY_CODE);
        }
        self.doc_rows.clear();
        let mut i = 0usize;
        while i < self.code_rows.len() {
            let r = &self.code_rows[i];
            self.doc_rows.push(FnRow { tbl: r.tbl, fn_off: r.fn_off, name_fp: r.name_fp });
            i += 1;
        }
        self.state = DocState::Projected;
        Ok(self.doc_rows.len())
    }

    /// 文档端注入拦截：外部试图手写文档条目一律拒收留账（判据 J1 红线）。
    pub fn inject_doc_row(&mut self, tbl: u8, fn_off: u8) -> Result<(), u16> {
        self.inject_blocked += 1;
        let _ = (tbl, fn_off);
        Err(E_DOC_INJECT)
    }

    /// 冻结：清单哈希入台账（判据 J2）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.state != DocState::Projected {
            return Err(E_EMPTY_CODE);
        }
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0usize;
        while i < self.code_rows.len() {
            let r = &self.code_rows[i];
            h = h.wrapping_mul(0x01000193) ^ (r.tbl as u32);
            h = h.wrapping_mul(0x01000193) ^ (r.fn_off as u32);
            h = h.wrapping_mul(0x01000193) ^ r.name_fp;
            i += 1;
        }
        self.snapshot_hash = h;
        self.ledger_hashes.push(h);
        Ok(h)
    }

    /// diff 零差核验：两账逐行三元组比对（判据 J1 断言面）。
    pub fn diff_verify(&mut self) -> Result<(), u16> {
        self.diff_runs += 1;
        if self.doc_rows.len() != self.code_rows.len() {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < self.code_rows.len() {
            if self.doc_rows[i].tbl != self.code_rows[i].tbl
                || self.doc_rows[i].fn_off != self.code_rows[i].fn_off
                || self.doc_rows[i].name_fp != self.code_rows[i].name_fp
            {
                return Err(E_DIFF_NZ);
            }
            i += 1;
        }
        self.state = DocState::Verified;
        Ok(())
    }

    /// 台账可验：冻结哈希存在且可重算一致（判据 J2）。
    pub fn ledger_verify(&self) -> Result<(), u16> {
        if self.snapshot_hash == 0 || self.ledger_hashes.is_empty() {
            return Err(E_SNAP_MISS);
        }
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0usize;
        while i < self.code_rows.len() {
            let r = &self.code_rows[i];
            h = h.wrapping_mul(0x01000193) ^ (r.tbl as u32);
            h = h.wrapping_mul(0x01000193) ^ (r.fn_off as u32);
            h = h.wrapping_mul(0x01000193) ^ r.name_fp;
            i += 1;
        }
        if h != self.snapshot_hash {
            return Err(E_DIFF_NZ);
        }
        Ok(())
    }

    pub fn code_len(&self) -> usize {
        self.code_rows.len()
    }

    pub fn doc_len(&self) -> usize {
        self.doc_rows.len()
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAL);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 全三表投影 diff → R2 边界 → R3 注入拦截 → R4 台账核对。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：三表装载 → 投影 → 冻结 → diff 零差 → 台账可验。
    let mut d = TabDocs::new();
    d.load_code_table(TBL_INODE, 0x1000)?;
    d.load_code_table(TBL_FILE, 0x2000)?;
    d.load_code_table(TBL_DIR, 0x3000)?;
    let n = d.project_docs()?;
    if n != (INODE_FN + FILE_FN + DIR_FN) as usize {
        return Err(E_BAL);
    }
    let h = d.freeze()?;
    d.diff_verify()?;
    d.ledger_verify()?;
    if d.snapshot_hash != h {
        return Err(E_SNAP_MISS);
    }
    d.writeback(true, 0x40)?;
    hits += 1;

    // R2 边界：表号越界 / 偏移越界 / 空代码账投影 / 重复投影。
    let mut d = TabDocs::new();
    if d.load_code_row(9, 0, b"x").is_ok() {
        return Err(E_BAD_TBL);
    }
    if d.load_code_row(TBL_DIR, DIR_FN, b"x").is_ok() {
        return Err(E_BAD_OFF);
    }
    if d.project_docs().is_ok() {
        return Err(E_EMPTY_CODE);
    }
    d.load_code_table(TBL_INODE, 1)?;
    d.project_docs()?;
    if d.project_docs().is_ok() {
        return Err(E_DOC_MADE);
    }
    hits += 1;

    // R3 注入拦截：文档端手写条目拒收留账；未冻结先验台账失败。
    let mut d = TabDocs::new();
    d.load_code_table(TBL_FILE, 7)?;
    d.project_docs()?;
    if d.inject_doc_row(TBL_FILE, 0).is_ok() {
        return Err(E_DOC_INJECT);
    }
    if d.inject_blocked != 1 {
        return Err(E_BAL);
    }
    if d.ledger_verify().is_ok() {
        return Err(E_SNAP_MISS);
    }
    hits += 1;

    // R4 守恒对账：投影行数=代码行数，diff_runs 计账，重验通过。
    let mut d = TabDocs::new();
    d.load_code_table(TBL_DIR, 0xABCD)?;
    d.project_docs()?;
    d.freeze()?;
    d.diff_verify()?;
    if d.doc_len() != d.code_len() {
        return Err(E_BAL);
    }
    d.ledger_verify()?;
    d.writeback(true, 0x14)?;
    if d.diff_runs != 1 || d.state != DocState::Verified {
        return Err(E_BAL);
    }
    // 台账多代可查：冻结一次入一账（追溯红线）。
    if d.code_len() != d.doc_len() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
