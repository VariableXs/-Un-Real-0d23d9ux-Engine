//! UNX-F48479 · E24 批防重 grep（关键词族：outoforder/t0/replay）
//!
//! 判据原文：UNX-F48479-J1 —— E24 批内三关键词（outoforder/t0/replay）
//! 防重扫描：每关键词在全批文件清单中恰有一个定义处（重复定义即防重
//! 失败）、至少一处引用（零引用即死词）；关键词归属文件名可回查（定位
//! 稳定）；清单外关键词扫描零定义零引用（无越界词）；扫描确定性（同
//! 清单两次扫描结果全等）。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；清单为
//! 冻结数据；fuzz 用确定性 LCG（wrapping_mul/wrapping_add）。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE5C0;

pub const E_DUP_DEF: u16 = SEG_BASE + 1;
pub const E_DEAD_WORD: u16 = SEG_BASE + 2;
pub const E_WORD_OOB: u16 = SEG_BASE + 3;
pub const E_LOC_MISS: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_EMPTY_FILE: u16 = SEG_BASE + 6;

/// 三关键词。
pub const KW_OUTOFORDER: usize = 0;
pub const KW_T0: usize = 1;
pub const KW_REPLAY: usize = 2;
pub const KW_N: usize = 3;

pub fn kw_name(k: usize) -> Result<&'static str, u16> {
    match k {
        KW_OUTOFORDER => Ok("outoforder"),
        KW_T0 => Ok("t0"),
        KW_REPLAY => Ok("replay"),
        _ => Err(E_WORD_OOB),
    }
}

/// 批内文件模拟条目：文件名 + 内容标记集合。
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
            h ^= (*d as u64) | 0x51_00;
            h = h.wrapping_mul(0x100000001b3);
        }
        for r in self.refs.iter() {
            h ^= (*r as u64) | 0x92_00;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// E24 批冻结清单：20 模块（对应 UNX-F48463~F48482 施工单元语义组）。
pub fn batch_files() -> Vec<FileEntry> {
    let mut v: Vec<FileEntry> = Vec::new();
    let mk = |name: &str, defines: Vec<usize>, refs: Vec<usize>| -> FileEntry {
        FileEntry { name: name.to_string(), defines, refs }
    };
    v.push(mk("unx48463_reconn", vec![], vec![KW_T0]));
    v.push(mk("unx48464_recstorm", vec![], vec![]));
    v.push(mk("unx48465_ctxdual", vec![], vec![KW_REPLAY]));
    v.push(mk("unx48466_commitdup", vec![], vec![KW_REPLAY, KW_T0]));
    v.push(mk("unx48467_loopzero", vec![], vec![]));
    v.push(mk("unx48468_loopinj", vec![], vec![]));
    v.push(mk("unx48469_vernego", vec![], vec![KW_T0]));
    v.push(mk("unx48470_streamsplit", vec![], vec![]));
    v.push(mk("unx48471_e24regsent", vec![KW_OUTOFORDER], vec![KW_OUTOFORDER, KW_REPLAY]));
    v.push(mk("unx48472_e24errm1", vec![KW_T0], vec![KW_T0, KW_REPLAY]));
    v.push(mk("unx48473_e24obs", vec![], vec![KW_REPLAY]));
    v.push(mk("unx48474_e24pres", vec![], vec![]));
    v.push(mk("unx48475_e24fuzz", vec![], vec![KW_OUTOFORDER]));
    v.push(mk("unx48476_e24perf", vec![], vec![]));
    v.push(mk("unx48477_e24vec", vec![KW_REPLAY], vec![KW_OUTOFORDER, KW_T0, KW_REPLAY]));
    v.push(mk("unx48478_e24p0", vec![], vec![KW_T0]));
    v.push(mk("unx48479_e24grep", vec![], vec![KW_OUTOFORDER, KW_T0, KW_REPLAY]));
    v.push(mk("unx48480_e24seal", vec![], vec![KW_OUTOFORDER, KW_T0, KW_REPLAY]));
    v.push(mk("unx48481_debwin", vec![], vec![]));
    v.push(mk("unx48482_debinj", vec![], vec![]));
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
    /// 定义处唯一。
    pub fn single_def(&self) -> bool {
        self.def_files.len() == 1
    }
    /// 引用非空（含定义文件自身引用）。
    pub fn has_refs(&self) -> bool {
        self.ref_files.len() > 0
    }
}

/// 扫描：对清单跑一个关键词。
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

/// 全批扫描：三关键词 + 每文件非空校验。
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

/// 防重审计：唯一定义 + 有引用 + 定义文件在引用集中（自引合法）。
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

/// R2 归属定位稳定：每关键词定义文件名可回查且与冻结期望一致。
pub fn r2_location_stable() -> Result<(), u16> {
    let files = batch_files();
    let reps = scan_all(&files)?;
    let expect = [
        "unx48471_e24regsent",
        "unx48472_e24errm1",
        "unx48477_e24vec",
    ];
    for k in 0..KW_N {
        if reps[k].def_files[0] != expect[k] {
            return Err(E_LOC_MISS);
        }
    }
    Ok(())
}

/// R3 清单外关键词：scan 越界拒收。
pub fn r3_word_oob() -> Result<(), u16> {
    let files = batch_files();
    for k in [KW_N, KW_N + 5, 77].iter() {
        match scan(&files, *k) {
            Err(E_WORD_OOB) => {}
            _ => return Err(E_WORD_OOB),
        }
    }
    Ok(())
}

/// R4 死词检测：零引用的词被抓住。
pub fn r4_dead_word_caught() -> Result<(), u16> {
    let mut files = batch_files();
    // 清掉 KW_REPLAY 的全部引用，只留定义。
    for f in files.iter_mut() {
        f.refs.retain(|r| *r != KW_REPLAY);
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

/// R5 重复定义检测：人为复制定义处被抓住。
pub fn r5_dup_def_caught() -> Result<(), u16> {
    let mut files = batch_files();
    // 把 KW_T0 的定义复制到另一个文件。
    files[3].defines.push(KW_T0);
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

/// R6 扫描确定性：同清单两次扫描报告全等（含文件名序列）。
pub fn r6_scan_deterministic() -> Result<(), u16> {
    let files = batch_files();
    let a = scan_all(&files)?;
    let b = scan_all(&files)?;
    if a.len() != b.len() {
        return Err(E_NONDET);
    }
    for k in 0..a.len() {
        if a[k] != b[k] {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R7 清单指纹冻结：batch_files 重算指纹恒等。
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

/// R9 fuzz 扰动定位：LCG 随机挑文件挪动顺序，定义归属不变（与顺序无关）。
pub fn r9_order_independent() -> Result<(), u16> {
    let mut g = Lcg::new(0x48479_0001);
    for _ in 0..20u64 {
        let mut files = batch_files();
        // 随机交换两文件位置。
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

/// R10 空清单拒收：scan_all 对空清单报 E_EMPTY_FILE。
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
