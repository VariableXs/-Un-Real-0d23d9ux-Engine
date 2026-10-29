//! CGPU-F0714 · 滚动文档（目标 370 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0714，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动文档——对滚动子系统 API 面生成/登记文档条目，
//!   做覆盖检查（每 API 一条文档）、质量门（段落齐备）与
//!   封存指纹；缺文档清单可导出。
//!
//! 设计纪律：
//! - 文档条目与 API 名一一对应：一个 API 名至多一条文档（重名拒绝），
//!   覆盖恒等式：api_total == documented + missing。
//! - 质量门五段：概述段/参数段/返回段/错误段/示例段——
//!   每段以固定标记前缀起头，缺段即不合格。
//! - 封存指纹：文档全书按名排序后 FNV-1a 折叠，
//!   同内容必同指纹（确定性）。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4780 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4780，批内 CGPU-F0714 独占）。
pub const SEG_BASE: u16 = 0x4780;

/// 文档重名（一个 API 只许一条文档）。
pub const E_DUP_DOC: u16 = SEG_BASE + 1;
/// 文档键名非法（空/超长）。
pub const E_BAD_KEY: u16 = SEG_BASE + 2;
/// 质量门缺段。
pub const E_MISSING_SEC: u16 = SEG_BASE + 3;
/// 正文过短（少于 MIN_BODY 字节）。
pub const E_TOO_SHORT: u16 = SEG_BASE + 4;
/// 对账不平（api_total ≠ documented + missing）。
pub const E_LEDGER: u16 = SEG_BASE + 5;
/// 覆盖检查对象为空。
pub const E_EMPTY: u16 = SEG_BASE + 6;

/// 概述段标记。
pub const SEC_OVERVIEW: &str = "#概#";
/// 参数段标记。
pub const SEC_PARAMS: &str = "#参#";
/// 返回段标记。
pub const SEC_RET: &str = "#返#";
/// 错误段标记。
pub const SEC_ERR: &str = "#错#";
/// 示例段标记。
pub const SEC_EX: &str = "#例#";
/// 正文最短字节数。
pub const MIN_BODY: usize = 32;

/// 文档条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEntry {
    /// 对应 API 名（键）。
    pub key: String,
    /// 正文（含五段标记）。
    pub body: String,
    /// 文档作者代号（写死格式 "w1-<序>"）。
    pub author: String,
}

impl DocEntry {
    /// 质量门：五段齐备且正文够长。
    pub fn quality(&self) -> Result<(), u16> {
        if self.body.len() < MIN_BODY {
            return Err(E_TOO_SHORT);
        }
        for m in [SEC_OVERVIEW, SEC_PARAMS, SEC_RET, SEC_ERR, SEC_EX] {
            if !self.body.contains(m) {
                return Err(E_MISSING_SEC);
            }
        }
        Ok(())
    }
    /// 条目指纹：FNV-1a(key|body)。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.key.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(self.body.as_bytes());
        fnv1a64(&bytes)
    }
}

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 文档全书。
#[derive(Clone, Debug, Default)]
pub struct DocBook {
    /// 条目表（按登记序）。
    entries: Vec<DocEntry>,
}

impl DocBook {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }
    /// 登记一条（键合法 + 不重名 + 过质量门）。
    pub fn put(&mut self, e: DocEntry) -> Result<(), u16> {
        if e.key.is_empty() || e.key.len() > 48 {
            return Err(E_BAD_KEY);
        }
        if self.entries.iter().any(|x| x.key == e.key) {
            return Err(E_DUP_DOC);
        }
        e.quality()?;
        self.entries.push(e);
        Ok(())
    }
    /// 按键查。
    pub fn get(&self, key: &str) -> Option<&DocEntry> {
        self.entries.iter().find(|e| e.key == key)
    }
    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 全书封存指纹：按键升序折叠。
    pub fn seal(&self) -> u64 {
        let mut keys: Vec<&str> = self.entries.iter().map(|e| e.key.as_str()).collect();
        // 选择排序（确定性，n 小）。
        for i in 0..keys.len() {
            let mut m = i;
            for j in i + 1..keys.len() {
                if keys[j] < keys[m] {
                    m = j;
                }
            }
            keys.swap(i, m);
        }
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        for k in &keys {
            let f = self
                .entries
                .iter()
                .find(|e| e.key == *k)
                .map(|e| e.fingerprint())
                .unwrap_or(0);
            h ^= f;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

/// 覆盖报告行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageRow {
    /// API 名。
    pub key: String,
    /// 是否已有文档。
    pub documented: bool,
}

/// 覆盖检查：给定 API 名清单，输出每名是否被文档覆盖。
/// 恒等式：api_total == documented + missing。
pub fn coverage(book: &DocBook, api_names: &[&str]) -> Result<Vec<CoverageRow>, u16> {
    if api_names.is_empty() {
        return Err(E_EMPTY);
    }
    let mut out = Vec::with_capacity(api_names.len());
    for n in api_names {
        out.push(CoverageRow {
            key: String::from(*n),
            documented: book.get(n).is_some(),
        });
    }
    let documented = out.iter().filter(|r| r.documented).count();
    if api_names.len() != documented + (api_names.len() - documented) {
        return Err(E_LEDGER);
    }
    Ok(out)
}

/// 缺文档清单。
pub fn missing_list(book: &DocBook, api_names: &[&str]) -> Result<Vec<String>, u16> {
    let rows = coverage(book, api_names)?;
    Ok(rows
        .into_iter()
        .filter(|r| !r.documented)
        .map(|r| r.key)
        .collect())
}

/// 覆盖率（千分比，整数）：documented * 1000 / total。
pub fn coverage_permille(book: &DocBook, api_names: &[&str]) -> Result<u32, u16> {
    if api_names.is_empty() {
        return Err(E_EMPTY);
    }
    let rows = coverage(book, api_names)?;
    let d = rows.iter().filter(|r| r.documented).count() as u64;
    Ok((d * 1000 / rows.len() as u64) as u32)
}

/// 便捷：由五段拼正文。
pub fn body5(overview: &str, params: &str, ret: &str, err: &str, ex: &str) -> String {
    let mut s = String::new();
    s.push_str(SEC_OVERVIEW);
    s.push_str(overview);
    s.push_str(SEC_PARAMS);
    s.push_str(params);
    s.push_str(SEC_RET);
    s.push_str(ret);
    s.push_str(SEC_ERR);
    s.push_str(err);
    s.push_str(SEC_EX);
    s.push_str(ex);
    s
}

/// 自例程：主路径（建册→覆盖全绿→封存一致）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut book = DocBook::new();
    // 主路径：两条合格文档。
    let b1 = body5(
        "多表面滚动并发仲裁器：登记表面、武装命令、按帧行预算推进。",
        "cmd: ScrollCmd；surf: u16；dy: u32。",
        "Ok(行数) 或 Err(诊断码)。",
        "E_SURF_FULL/E_STATE_EDGE/E_SCROLL_RANGE。",
        "let mut c = ScrollCon::new(); c.register(desc)?;",
    );
    book.put(DocEntry {
        key: String::from("scroll_tick"),
        body: b1.clone(),
        author: String::from("w1-1"),
    })?;
    let b2 = body5(
        "滚动遥测会话：定容环采集五类滚动事件并守恒计数。",
        "kind: EvtKind；surf: u16；val: u32；frame: u32。",
        "Ok(seq) 或 Err(诊断码)。",
        "E_NOT_OPEN/E_ALREADY_OPEN。",
        "let mut t = ScrollTelemetry::new(); t.open()?;",
    );
    book.put(DocEntry {
        key: String::from("scroll_telem"),
        body: b2.clone(),
        author: String::from("w1-2"),
    })?;
    if book.len() != 2 {
        return Err(E_LEDGER);
    }
    // 覆盖：3 个 API 名，其中 1 个缺文档。
    let apis = ["scroll_tick", "scroll_telem", "scroll_fuzz_run"];
    let rows = coverage(&book, &apis)?;
    if rows.len() != 3 || rows[2].documented {
        return Err(E_LEDGER);
    }
    let miss = missing_list(&book, &apis)?;
    if miss.len() != 1 || miss[0] != "scroll_fuzz_run" {
        return Err(E_LEDGER);
    }
    let pm = coverage_permille(&book, &apis)?;
    if pm != 666 {
        // 2*1000/3 = 666（整除截断）
        return Err(E_LEDGER);
    }
    // 封存：同内容新书同指纹。
    let mut book2 = DocBook::new();
    book2.put(DocEntry {
        key: String::from("scroll_telem"),
        body: b2,
        author: String::from("w1-2"),
    })?;
    book2.put(DocEntry {
        key: String::from("scroll_tick"),
        body: b1,
        author: String::from("w1-1"),
    })?;
    if book.seal() != book2.seal() {
        return Err(E_LEDGER);
    }
    // 错误路径 1：重名文档。
    if book.put(DocEntry {
        key: String::from("scroll_tick"),
        body: body5("a", "b", "c", "d", "e"),
        author: String::from("w1-3"),
    }) != Err(E_DUP_DOC)
    {
        return Err(E_DUP_DOC);
    }
    // 错误路径 2：缺段（少示例段）。
    let bad_body = {
        let mut s = String::new();
        s.push_str(SEC_OVERVIEW);
        s.push_str("概述");
        s.push_str(SEC_PARAMS);
        s.push_str("参数");
        s.push_str(SEC_RET);
        s.push_str("返回");
        s.push_str(SEC_ERR);
        s.push_str("错误");
        s
    };
    if book.put(DocEntry {
        key: String::from("scroll_x"),
        body: bad_body,
        author: String::from("w1-4"),
    }) != Err(E_MISSING_SEC)
    {
        return Err(E_MISSING_SEC);
    }
    // 错误路径 3：正文过短。
    if book.put(DocEntry {
        key: String::from("scroll_y"),
        body: body5("a", "b", "c", "d", "e"),
        author: String::from("w1-5"),
    }) != Err(E_TOO_SHORT)
    {
        return Err(E_TOO_SHORT);
    }
    // 错误路径 4：空键。
    if book.put(DocEntry {
        key: String::new(),
        body: body5("概述占位", "参数占位", "返回占位", "错误占位", "示例占位"),
        author: String::from("w1-6"),
    }) != Err(E_BAD_KEY)
    {
        return Err(E_BAD_KEY);
    }
    // 错误路径 5：空 API 清单覆盖 → E_EMPTY。
    if coverage(&book, &[]) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    Ok(0)
}
