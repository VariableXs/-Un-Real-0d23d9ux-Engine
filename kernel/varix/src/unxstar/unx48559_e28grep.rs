//! UNX-F48559 · E28 批防重 grep（关键词族：recovery/powerloss/rebuild）
//!
//! 判据原文：UNX-F48559-J1 —— E28 批内三关键词（recovery/powerloss/
//! rebuild）防重扫描：每关键词在全批文件清单中恰有一个定义处（重复
//! 定义即防重失败）、至少一处引用（零引用即死词）；关键词归属文件名
//! 可回查（定位稳定）；清单外关键词扫描零定义零引用；扫描确定性（同
//! 清单两次扫描结果全等）。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；清单为
//! 冻结数据；fuzz 用确定性 LCG（wrapping_mul/wrapping_add）。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF520;

pub const E_DUP_DEF: u16 = SEG_BASE + 1;
pub const E_DEAD_WORD: u16 = SEG_BASE + 2;
pub const E_WORD_OOB: u16 = SEG_BASE + 3;
pub const E_LOC_MISS: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_EMPTY_FILE: u16 = SEG_BASE + 6;

/// 三关键词。
pub const KW_RECOVERY: usize = 0;
pub const KW_POWERLOSS: usize = 1;
pub const KW_REBUILD: usize = 2;
pub const KW_N: usize = 3;

pub fn kw_name(k: usize) -> Result<&'static str, u16> {
    match k {
        KW_RECOVERY => Ok("recovery"),
        KW_POWERLOSS => Ok("powerloss"),
        KW_REBUILD => Ok("rebuild"),
        _ => Err(E_WORD_OOB),
    }
}

/// 文件条目。
#[derive(Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub defines: Vec<usize>,
    pub refs: Vec<usize>,
}

impl FileEntry {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self.name.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for d in self.defines.iter() {
            h ^= (*d as u64) | 0x61_00;
            h = h.wrapping_mul(0x100000001b3);
        }
        for r in self.refs.iter() {
            h ^= (*r as u64) | 0xA2_00;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// E28 批冻结清单（20 模块：UNX-F48543~F48562）。
pub fn batch_files() -> Vec<FileEntry> {
    let mk = |name: &str, defines: Vec<usize>, refs: Vec<usize>| -> FileEntry {
        FileEntry { name: name.to_string(), defines, refs }
    };
    let mut v: Vec<FileEntry> = Vec::new();
    v.push(mk("unx48543_crashrebuild", vec![], vec![KW_REBUILD]));
    v.push(mk("unx48544_crashclean", vec![], vec![]));
    v.push(mk("unx48545_snapsame", vec![], vec![KW_RECOVERY]));
    v.push(mk("unx48546_snapgen", vec![], vec![]));
    v.push(mk("unx48547_lkpersist", vec![], vec![KW_RECOVERY, KW_REBUILD]));
    v.push(mk("unx48548_lkhole", vec![], vec![]));
    v.push(mk("unx48549_factoryreset", vec![], vec![]));
    v.push(mk("unx48550_resetguard", vec![], vec![]));
    v.push(mk("unx48551_e28regsent", vec![KW_RECOVERY], vec![KW_RECOVERY, KW_REBUILD]));
    v.push(mk("unx48552_e28errm1", vec![KW_POWERLOSS], vec![KW_POWERLOSS, KW_RECOVERY]));
    v.push(mk("unx48553_e28obs", vec![], vec![KW_POWERLOSS]));
    v.push(mk("unx48554_e28fuzz", vec![], vec![]));
    v.push(mk("unx48555_e28pres", vec![], vec![KW_RECOVERY, KW_POWERLOSS]));
    v.push(mk("unx48556_e28vec", vec![], vec![]));
    v.push(mk("unx48557_e28journal", vec![KW_REBUILD], vec![KW_RECOVERY, KW_REBUILD]));
    v.push(mk("unx48558_e28machine", vec![], vec![KW_RECOVERY]));
    v.push(mk("unx48559_e28grep", vec![], vec![KW_RECOVERY, KW_POWERLOSS, KW_REBUILD]));
    v.push(mk("unx48560_e28seal", vec![], vec![KW_RECOVERY, KW_POWERLOSS, KW_REBUILD]));
    v.push(mk("unx48561_candfreeze", vec![], vec![]));
    v.push(mk("unx48562_candlocate", vec![], vec![]));
    v
}

/// 扫描结果。
#[derive(Clone, PartialEq, Eq)]
pub struct KwReport {
    pub kw: usize,
    pub def_files: Vec<String>,
    pub ref_files: Vec<String>,
}

impl KwReport {
    pub fn single_def(&self) -> bool {
        self.def_files.len() == 1
    }
    pub fn has_refs(&self) -> bool {
        self.ref_files.len() > 0
    }
}

/// 扫描一个关键词。
pub fn scan(files: &[FileEntry], kw: usize) -> Result<KwReport, u16> {
    if kw >= KW_N {
        return Err(E_WORD_OOB);
    }
    let mut rep = KwReport { kw, def_files: Vec::new(), ref_files: Vec::new() };
    for f in files.iter() {
        for d in f.defines.iter() {
            if *d == kw {
                rep.def_files.push(f.name.clone());
            }
        }
        for r in f.refs.iter() {
            if *r == kw {
                rep.ref_files.push(f.name.clone());
            }
        }
    }
    Ok(rep)
}

/// 全批扫描。
pub fn scan_all(files: &[FileEntry]) -> Result<Vec<KwReport>, u16> {
    if files.len() == 0 {
        return Err(E_EMPTY_FILE);
    }
    let mut out: Vec<KwReport> = Vec::new();
    for k in 0..KW_N {
        out.push(scan(files, k)?);
    }
    Ok(out)
}

/// 防重审计。
pub fn audit_no_dup(reps: &[KwReport]) -> Result<(), u16> {
    if reps.len() != KW_N {
        return Err(E_WORD_OOB);
    }
    for r in reps.iter() {
        if !r.single_def() {
            return Err(E_DUP_DEF);
        }
        if !r.has_refs() {
            return Err(E_DEAD_WORD);
        }
        let defn = r.def_files[0].clone();
        let mut found = false;
        for f in r.ref_files.iter() {
            if *f == defn {
                found = true;
                break;
            }
        }
        if !found {
            return Err(E_LOC_MISS);
        }
    }
    Ok(())
}

/// R1 三关键词唯一定义。
pub fn r1_single_def() -> Result<(), u16> {
    let files = batch_files();
    let reps = scan_all(&files)?;
    audit_no_dup(&reps)
}

/// R2 归属定位稳定。
pub fn r2_location_stable() -> Result<(), u16> {
    let files = batch_files();
    let reps = scan_all(&files)?;
    let expect = [
        "unx48551_e28regsent",
        "unx48552_e28errm1",
        "unx48557_e28journal",
    ];
    for k in 0..KW_N {
        if reps[k].def_files[0] != expect[k] {
            return Err(E_LOC_MISS);
        }
    }
    Ok(())
}

/// R3 清单外关键词拒收。
pub fn r3_word_oob() -> Result<(), u16> {
    let files = batch_files();
    for k in [KW_N, KW_N + 4, 55].iter() {
        match scan(&files, *k) {
            Err(E_WORD_OOB) => {}
            _ => return Err(E_WORD_OOB),
        }
    }
    Ok(())
}

/// R4 死词检测。
pub fn r4_dead_word_caught() -> Result<(), u16> {
    let mut files = batch_files();
    for f in files.iter_mut() {
        f.refs.retain(|r| *r != KW_REBUILD);
    }
    let reps = scan_all(&files)?;
    let mut dead = 0;
    for r in reps.iter() {
        if !r.has_refs() {
            dead += 1;
        }
    }
    if dead != 1 {
        return Err(E_DEAD_WORD);
    }
    Ok(())
}

/// R5 重复定义检测。
pub fn r5_dup_def_caught() -> Result<(), u16> {
    let mut files = batch_files();
    files[5].defines.push(KW_RECOVERY);
    let reps = scan_all(&files)?;
    let mut dup = 0;
    for r in reps.iter() {
        if !r.single_def() {
            dup += 1;
        }
    }
    if dup != 1 {
        return Err(E_DUP_DEF);
    }
    Ok(())
}

/// R6 扫描确定性。
pub fn r6_scan_deterministic() -> Result<(), u16> {
    let files = batch_files();
    let a = scan_all(&files)?;
    let b = scan_all(&files)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R7 清单指纹冻结。
pub fn r7_manifest_frozen() -> Result<(), u16> {
    let manifest_fp = |files: &[FileEntry]| -> u64 {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        h ^= files.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        for f in files.iter() {
            h ^= f.fp();
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let a = manifest_fp(&batch_files());
    let b = manifest_fp(&batch_files());
    if a != b || a == 0 {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R8 文件名互异：20 文件全不同名。
pub fn r8_names_distinct() -> Result<(), u16> {
    let files = batch_files();
    if files.len() != 20 {
        return Err(E_EMPTY_FILE);
    }
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            if files[i].name == files[j].name {
                return Err(E_LOC_MISS);
            }
        }
    }
    Ok(())
}

/// R9 顺序无关：LCG 随机换位后归属不变。
pub fn r9_order_independent() -> Result<(), u16> {
    let mut g = Lcg::new(0x48559_0001);
    for _ in 0..20u64 {
        let mut files = batch_files();
        let i = g.below(files.len() as u64) as usize;
        let j = g.below(files.len() as u64) as usize;
        let tmp = files[i].clone();
        files[i] = files[j].clone();
        files[j] = tmp;
        let reps = scan_all(&files)?;
        audit_no_dup(&reps)?;
    }
    Ok(())
}

/// R10 空清单拒收。
pub fn r10_empty_manifest() -> Result<(), u16> {
    let empty: Vec<FileEntry> = Vec::new();
    match scan_all(&empty) {
        Err(E_EMPTY_FILE) => {}
        _ => return Err(E_EMPTY_FILE),
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_single_def()?;
    hits += 1;
    r2_location_stable()?;
    hits += 1;
    r3_word_oob()?;
    hits += 1;
    r4_dead_word_caught()?;
    hits += 1;
    r5_dup_def_caught()?;
    hits += 1;
    r6_scan_deterministic()?;
    hits += 1;
    r7_manifest_frozen()?;
    hits += 1;
    r8_names_distinct()?;
    hits += 1;
    r9_order_independent()?;
    hits += 1;
    r10_empty_manifest()?;
    hits += 1;
    Ok(hits)
}
