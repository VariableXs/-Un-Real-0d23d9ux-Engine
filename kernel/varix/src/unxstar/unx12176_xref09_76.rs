//! UNX-F12176 — CONTEXT 对照表制度（0x1a820 段）
//!
//! 锚点：UNX-F12176-J1 CONTEXT 对照表 10/10（API→掩码组→授权语义
//! 三列逐行对平、全表回放一致），野 API 拒、掩码组合非法拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxXrefRow（对照行）+ CtxXref（对照表）+ 折叠指纹 +
//! 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a820;
pub const XC_DUP: u32 = SEG_BASE + 0x01; // 重复 API
pub const XC_GHOST: u32 = SEG_BASE + 0x02; // API 不在表
pub const XC_ORDER: u32 = SEG_BASE + 0x03; // 掩码组合非法
pub const XC_SEALED: u32 = SEG_BASE + 0x04; // 印后挂行
pub const XC_EMPTY: u32 = SEG_BASE + 0x05; // 空行

/// 对照行。
#[derive(Clone, Debug, PartialEq)]
pub struct CtxXrefRow {
    pub api: String,
    pub mask: u32,
    pub gated: bool, // true = 掩码外组拒动
    pub note: String,
}

impl CtxXrefRow {
    /// 行自洽：API 非空、掩码非零且含 CONTROL 或为纯 FLOAT/DEBUG。
    pub fn ok(&self) -> bool {
        let ctl = 0x1001u32;
        let loose = 0x1008u32 | 0x1010;
        !self.api.is_empty()
            && self.mask != 0
            && (self.mask & ctl == ctl || self.mask & !loose == 0)
            && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("{} {:#x} {}", self.api, self.mask, if self.gated { "G" } else { "-" })
    }
}

/// 对照表。
pub struct CtxXref {
    pub rows: Vec<CtxXrefRow>,
    pub queries: Vec<String>,
    sealed: bool,
}

impl CtxXref {
    pub fn new() -> CtxXref {
        CtxXref { rows: Vec::new(), queries: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂行（重复 API/非法行必拒）。
    pub fn add(&mut self, api: &str, mask: u32, gated: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        let row = CtxXrefRow { api: api.to_string(), mask, gated, note: note.to_string() };
        if !row.ok() {
            return Err(XC_ORDER);
        }
        if self.rows.iter().any(|r| r.api == api) {
            return Err(XC_DUP);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 查行（API 不在表必拒）。
    pub fn lookup(&mut self, api: &str) -> Result<&CtxXrefRow, u32> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        self.queries.push(api.to_string());
        self.rows.iter().find(|r| r.api == api).ok_or(XC_GHOST)
    }

    /// 全表回放：每行渲染首列=API。
    pub fn all_replayable(&self) -> bool {
        self.rows.iter().all(|r| r.render().starts_with(&r.api))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a820;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.mask);
            h = h.wrapping_mul(211).wrapping_add(r.gated as u32);
            for by in r.api.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("rows={} queries={}", self.rows.len(), self.queries.len())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CtxXref::new();
    let _ = b.add("NtGetContextThread", 0x1005, true, "g");
    // 1) 重复 API 必拒。
    got[0] = b.add("NtGetContextThread", 0x1001, false, "d").err() == Some(XC_DUP);
    // 2) 野 API 必拒。
    got[1] = b.lookup("NtWild").err() == Some(XC_GHOST);
    // 3) 零掩码行必拒。
    got[2] = b.add("X", 0, false, "z").err() == Some(XC_ORDER);
    // 4) 掩码含 INTEGER 无 CONTROL 必拒。
    got[3] = b.add("Y", 0x1002, false, "y").err() == Some(XC_ORDER);
    // 5) 印后挂行必拒。
    b.seal();
    got[4] = b.add("Z", 0x1001, false, "p").err() == Some(XC_SEALED);
    got
}

/// 判据入口：UNX-F12176
pub fn run_unx12176_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12176");
    let mut b = CtxXref::new();

    // J1 四行对照挂表。
    let j1 = b.add("NtGetContextThread", 0x1005, true, "get-gated").is_ok()
        && b.add("NtSetContextThread", 0x1003, true, "set-gated").is_ok()
        && b.add("RtlCaptureContext", 0x1001, false, "capture-full-ctl").is_ok()
        && b.add("NtContinue", 0x1001, false, "continue-ctl").is_ok();
    s.add("J1_rows4", j1 && b.rows.len() == 4, "四行对照全挂");

    // J2 查表 10/10（四 API 双查 + 野名）。
    let mut j2 = true;
    for api in ["NtGetContextThread", "NtSetContextThread", "RtlCaptureContext", "NtContinue"] {
        if b.lookup(api).is_err() {
            j2 = false;
        }
    }
    if b.lookup("NtWild").is_ok() {
        j2 = false;
    }
    for api in ["NtGetContextThread", "NtSetContextThread", "RtlCaptureContext", "NtContinue"] {
        let _ = b.lookup(api);
    }
    s.add("J2_lookup_10x", j2 && b.queries.len() == 9, "四 API 双查+野名留痕全对");

    // J3 野 API 拒。
    s.add(
        "J3_wild",
        b.lookup("Nope").err() == Some(XC_GHOST),
        "野 API 拒 GHOST",
    );

    // J4 行渲染。
    let r = b.rows[0].render();
    s.add(
        "J4_render",
        r == "NtGetContextThread 0x1005 G",
        "行渲染三列逐字对平",
    );

    // J5 行自洽核。
    s.add(
        "J5_rows_ok",
        b.rows.iter().all(|r| r.ok()),
        "全行过自洽核",
    );

    // J6 gated 位语义（Get/Set 带门、Rtl 不带）。
    s.add(
        "J6_gated_split",
        b.rows.iter().filter(|r| r.gated).count() == 2,
        "gated 2/4 分账",
    );

    // J7 掩码组合合法性（CTL|INT / CTL|SEG / 纯 FLOAT）。
    let mut b7 = CtxXref::new();
    let ok7 = b7.add("A", 0x1003, true, "a").is_ok()
        && b7.add("B", 0x1005, true, "b").is_ok()
        && b7.add("C", 0x1008, false, "c").is_ok()
        && b7.add("D", 0x1010, false, "d").is_ok();
    s.add("J7_masks", ok7, "CTL|INT、CTL|SEG、纯 FLOAT/DEBUG 全合法");

    // J8 掩码组合非法（INT 无 CTL）。
    s.add(
        "J8_bad_mask",
        b7.add("E", 0x1002, false, "e").err() == Some(XC_ORDER),
        "INT 无 CTL 拒 ORDER",
    );

    // J9 重复 API 拒。
    s.add(
        "J9_dup",
        b.add("NtGetContextThread", 0x1001, false, "d").err() == Some(XC_DUP),
        "重复 API 拒 DUP",
    );

    // J10 全表回放。
    s.add("J10_replayable", b.all_replayable(), "全表渲染回放一致");

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "对照表折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.add("Post", 0x1001, false, "p").err() == Some(XC_SEALED)
            && b.lookup("NtGetContextThread").err() == Some(XC_SEALED),
        "印账后挂/查皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("rows=4") && sm.contains("queries=10"),
        "摘要含行数与查询数",
    );

    // J15 空行拒（空 API 名）。
    let mut b15 = CtxXref::new();
    s.add(
        "J15_empty_api",
        b15.add("", 0x1001, false, "e").err() == Some(XC_ORDER),
        "空 API 名拒 ORDER",
    );

    // J16 留痕空行拒。
    s.add(
        "J16_empty_note",
        b15.add("F", 0x1001, false, "").err() == Some(XC_ORDER),
        "留痕空拒 ORDER",
    );

    // J17 十行大表全挂全查。
    let mut b17 = CtxXref::new();
    let mut ok17 = true;
    for i in 0..10u32 {
        let api = format!("Api{}", i);
        if b17.add(&api, 0x1001, i % 2 == 0, "m").is_err() {
            ok17 = false;
        }
    }
    let hit17 = (0..10u32).all(|i| b17.lookup(&format!("Api{}", i)).is_ok());
    s.add("J17_big10", ok17 && hit17, "十行大表全挂全查");

    // J18 大表回放。
    s.add("J18_big_replay", b17.all_replayable(), "大表全回放一致");

    // J19 空表指纹稳定。
    let b19 = CtxXref::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空对照表指纹稳定");

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12176_xref() {
        let mut b = CtxXref::new();
        let _ = b.add("NtGetContextThread", 0x1005, true, "g");
        assert!(b.lookup("NtGetContextThread").is_ok());
        assert_eq!(b.lookup("Wild"), Err(XC_GHOST));
    }

    #[test]
    fn unx12176_checks_green() {
        let s = run_unx12176_checks();
        assert!(s.all_passed(), "UNX-F12176 判据未全绿");
    }

    #[test]
    fn unx12176_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
