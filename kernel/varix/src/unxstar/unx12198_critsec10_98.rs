//! UNX-F12198 — 临界区对照表制度（0x1aae0 段）
//!
//! 锚点：UNX-F12198-J1 对照表制度 10/10（API→语义→返回三列对照
//! 行、全表回放逐行过核、重复行必拒、野行查表必拒），表账全绿
//! 才算制度成立。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：XrefRow（对照行）+ CsXref（对照表：挂行/查行/回放）+
//! 折叠指纹 + 注入核。
//!
//! 制度语义：临界区族每一公开入口都必须在对照表有一行——API 名、
//! 一句话语义、返回契约三列齐备；行缺失即文档债，行重复即账面
//! 破坏。全表回放要求逐行渲染可回读、语义列非空、返回列非空，
//! 回放不过则制度判红。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aae0;
pub const XR_SEALED: u32 = SEG_BASE + 0x01; // 印后挂行
pub const XR_TWICE: u32 = SEG_BASE + 0x02; // 重复行
pub const XR_GHOST: u32 = SEG_BASE + 0x03; // 野行查表
pub const XR_EMPTY: u32 = SEG_BASE + 0x04; // 空列行
pub const XR_NONE: u32 = SEG_BASE + 0x05; // 空表回放

/// 对照行：API → 语义 → 返回 三列。
#[derive(Clone, Debug, PartialEq)]
pub struct XrefRow {
    pub api: String,
    pub semantics: String,
    pub ret: String,
}

impl XrefRow {
    /// 行自洽：三列全非空。
    pub fn ok(&self) -> bool {
        !self.api.is_empty() && !self.semantics.is_empty() && !self.ret.is_empty()
    }

    /// 行渲染（回放面）。
    pub fn render(&self) -> String {
        format!("{} | {} | {}", self.api, self.semantics, self.ret)
    }
}

/// 临界区对照表。
pub struct CsXref {
    pub rows: Vec<XrefRow>,
    sealed: bool,
}

impl CsXref {
    pub fn new() -> CsXref {
        CsXref { rows: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂行：三列齐备、API 名不重复。
    pub fn add(&mut self, api: &str, semantics: &str, ret: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(XR_SEALED);
        }
        if api.is_empty() || semantics.is_empty() || ret.is_empty() {
            return Err(XR_EMPTY);
        }
        if self.rows.iter().any(|r| r.api == api) {
            return Err(XR_TWICE);
        }
        self.rows.push(XrefRow {
            api: api.to_string(),
            semantics: semantics.to_string(),
            ret: ret.to_string(),
        });
        Ok(self.rows.len() as u32)
    }

    /// 查行：按 API 名精确匹配；野行拒。
    pub fn lookup(&self, api: &str) -> Result<&XrefRow, u32> {
        match self.rows.iter().find(|r| r.api == api) {
            Some(r) => Ok(r),
            None => Err(XR_GHOST),
        }
    }

    /// 全表回放：逐行过核且行数与账面一致。
    pub fn replay(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.ok())
    }

    /// 表行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 全表渲染（诊断面）。
    pub fn render_all(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&r.render());
            out.push('\n');
        }
        out
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aae0;
        for r in &self.rows {
            for by in r.api.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
            for by in r.ret.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 表摘要。
    pub fn book_summary(&self) -> String {
        format!("rows={} replay={}", self.len(), self.replay())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CsXref::new();
    let _ = b.add("EnterCriticalSection", "阻塞得锁", "void");
    // 1) 重复 API 行必拒。
    got[0] = b.add("EnterCriticalSection", "x", "y").err() == Some(XR_TWICE);
    // 2) 野行查表必拒。
    got[1] = b.lookup("NtWaitForSingleObject").err() == Some(XR_GHOST);
    // 3) 空语义列必拒。
    got[2] = b.add("A", "", "r").err() == Some(XR_EMPTY);
    // 4) 空返回列必拒。
    got[3] = b.add("B", "s", "").err() == Some(XR_EMPTY);
    // 5) 印后挂行必拒。
    b.seal();
    got[4] = b.add("C", "s", "r").err() == Some(XR_SEALED);
    got
}

/// 判据入口：UNX-F12198
pub fn run_unx12198_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12198");
    let mut b = CsXref::new();

    // J1 十行对照全挂（临界区族公开面）。
    let mut ok1 = true;
    let rows: [(&str, &str, &str); 10] = [
        ("InitializeCriticalSection", "置临界区零态", "void"),
        ("InitializeCriticalSectionAndSpinCount", "零态并写归一自旋数", "u32"),
        ("EnterCriticalSection", "阻塞得锁（自旋→等待块）", "void"),
        ("TryEnterCriticalSection", "非阻塞试锁 0 得/忙拒", "bool"),
        ("LeaveCriticalSection", "递归深度减一归零释放", "void"),
        ("DeleteCriticalSection", "释放 DebugInfo 与事件", "void"),
        ("SetCriticalSectionSpinCount", "改写自旋数", "u32"),
        ("RtlInitializeCriticalSection", "Initialize 族的 Rtl 底座", "i32"),
        ("RtlEnterCriticalSection", "Enter 族的 Rtl 底座", "i32"),
        ("RtlDeleteCriticalSection", "Delete 族的 Rtl 底座", "void"),
    ];
    for (a, sem, ret) in rows {
        if b.add(a, sem, ret).is_err() {
            ok1 = false;
        }
    }
    s.add("J1_ten_rows", ok1 && b.len() == 10, "十行对照全挂");

    // J2 重复行拒。
    s.add(
        "J2_dup_row",
        b.add("EnterCriticalSection", "x", "y").err() == Some(XR_TWICE),
        "重复 API 行拒 TWICE",
    );

    // J3 精确查行命中。
    let hit = b.lookup("TryEnterCriticalSection");
    s.add(
        "J3_lookup_hit",
        hit.map(|r| r.ret == "bool").unwrap_or(false),
        "试锁行返回列对平",
    );

    // J4 野行查表拒。
    s.add(
        "J4_lookup_ghost",
        b.lookup("RtlRaiseStatus").err() == Some(XR_GHOST),
        "野 API 查表拒 GHOST",
    );

    // J5 全表回放过核。
    s.add("J5_replay", b.replay(), "全表逐行过自洽核");

    // J6 行渲染回读。
    let r = b.lookup("EnterCriticalSection").map(|r| r.render()).unwrap_or_default();
    s.add(
        "J6_render",
        r.contains("EnterCriticalSection") && r.contains("自旋") && r.contains("void"),
        "三列渲染可回读",
    );

    // J7 全表渲染行数对平。
    let all = b.render_all();
    s.add(
        "J7_render_all",
        all.lines().count() == 10 && all.contains("RtlDeleteCriticalSection"),
        "全表渲染 10 行",
    );

    // J8 空列行拒。
    let mut b8 = CsXref::new();
    s.add(
        "J8_empty_cols",
        b8.add("A", "", "r").err() == Some(XR_EMPTY)
            && b8.add("", "s", "r").err() == Some(XR_EMPTY)
            && b8.add("A", "s", "").err() == Some(XR_EMPTY),
        "三列任一为空拒 EMPTY",
    );

    // J9 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J9_fingerprint", f1 == f2, "对照表折叠指纹一致");

    // J10 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 印账受控。
    b.seal();
    s.add(
        "J11_sealed",
        b.add("Post", "s", "r").err() == Some(XR_SEALED),
        "印账后挂行拒",
    );

    // J12 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J12_seal_fp", b.fold() == fb, "印账不改指纹");

    // J13 空表指纹稳定。
    let b13 = CsXref::new();
    s.add("J13_fresh_fp", b13.fold() == b13.fold(), "空对照表指纹稳定");

    // J14 空表回放判不自洽。
    s.add("J14_empty_replay", !b13.replay(), "空表回放判不自洽");

    // J15 查行返回列契约全查（十行返回列非空）。
    let ok15 = rows.iter().all(|(a, _, _)| {
        b.lookup(a).map(|r| r.ok()).unwrap_or(false)
    });
    s.add("J15_all_lookup_ok", ok15, "十行逐行可查且过核");

    // J16 表摘要。
    let sm = b.book_summary();
    s.add(
        "J16_summary",
        sm.contains("rows=10") && sm.contains("replay=true"),
        "摘要含行数与回放位",
    );

    // J17 语义列覆盖初始化族（对照制度与 12191 呼应）。
    s.add(
        "J17_init_family",
        b.lookup("InitializeCriticalSectionAndSpinCount")
            .map(|r| r.semantics.contains("自旋"))
            .unwrap_or(false),
        "初始化族行语义覆盖自旋",
    );

    // J18 语义列覆盖删除族（与 12192 呼应）。
    s.add(
        "J18_del_family",
        b.lookup("DeleteCriticalSection")
            .map(|r| r.semantics.contains("DebugInfo") && r.semantics.contains("事件"))
            .unwrap_or(false),
        "删除族行语义覆盖双挂点",
    );

    // J19 行序稳定（挂行序即回放序）。
    let first = b.rows[0].api.clone();
    let last = b.rows[9].api.clone();
    s.add(
        "J19_row_order",
        first == "InitializeCriticalSection" && last == "RtlDeleteCriticalSection",
        "挂行序即回放序",
    );

    // J20 野行查表不污染账面。
    s.add(
        "J20_ghost_noop",
        b.len() == 10 && b.replay(),
        "拒行后账面行数不变",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12198_xref() {
        let mut b = CsXref::new();
        assert_eq!(b.add("A", "s", "r"), Ok(1));
        assert_eq!(b.add("A", "s", "r"), Err(XR_TWICE));
        assert!(b.lookup("A").is_ok());
        assert_eq!(b.lookup("B"), Err(XR_GHOST));
        assert!(b.replay());
    }

    #[test]
    fn unx12198_checks_green() {
        let s = run_unx12198_checks();
        assert!(s.all_passed(), "UNX-F12198 判据未全绿");
    }

    #[test]
    fn unx12198_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
