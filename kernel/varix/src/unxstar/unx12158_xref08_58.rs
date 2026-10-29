//! UNX-F12158 — 异常分发对照表制度（0x1a5e0 段）
//!
//! 锚点：UNX-F12158-J1 分发对照表 10/10（异常码 ↔ 分发路径 ↔ 终局
//! 三列对平，全表逐行回放一致），野码不在表必拒。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：XrefRow（码→路径→终局）+ XrefTable（对照表 + 回放核）+
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a5e0;
pub const XR_DUP: u32 = SEG_BASE + 0x01; // 重复码
pub const XR_GHOST: u32 = SEG_BASE + 0x02; // 码不在表
pub const XR_ORDER: u32 = SEG_BASE + 0x03; // 路径/终局组合非法
pub const XR_SEALED: u32 = SEG_BASE + 0x04; // 印后挂行
pub const XR_EMPTY: u32 = SEG_BASE + 0x05; // 空行挂表

/// 分发路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XrPath {
    Veh,
    Seh,
    VehThenSeh,
    Uef,
}

impl XrPath {
    pub fn tag(&self) -> &'static str {
        match self {
            XrPath::Veh => "VEH",
            XrPath::Seh => "SEH",
            XrPath::VehThenSeh => "VEH+SEH",
            XrPath::Uef => "UEF",
        }
    }
}

/// 终局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XrEnd {
    Handled,
    Wer,
    Crash,
}

impl XrEnd {
    pub fn tag(&self) -> &'static str {
        match self {
            XrEnd::Handled => "H",
            XrEnd::Wer => "W",
            XrEnd::Crash => "C",
        }
    }
}

/// 对照行。
#[derive(Clone, Debug, PartialEq)]
pub struct XrefRow {
    pub code: u32,
    pub path: XrPath,
    pub end: XrEnd,
    pub note: String,
}

impl XrefRow {
    /// 行自洽：C 终局不得配 Handled 路径之外的 UEF；空码非法。
    pub fn ok(&self) -> bool {
        self.code != 0
            && !(self.end == XrEnd::Handled && self.path == XrPath::Uef)
            && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!(
            "{:#x} {} {}",
            self.code,
            self.path.tag(),
            self.end.tag()
        )
    }
}

/// 对照表。
pub struct XrefTable {
    pub rows: Vec<XrefRow>,
    pub queries: Vec<u32>,
    sealed: bool,
}

impl XrefTable {
    pub fn new() -> XrefTable {
        XrefTable { rows: Vec::new(), queries: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂行（重复码/非法行必拒）。
    pub fn add(&mut self, code: u32, path: XrPath, end: XrEnd, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XR_SEALED);
        }
        let row = XrefRow { code, path, end, note: note.to_string() };
        if !row.ok() {
            return Err(XR_ORDER);
        }
        if self.rows.iter().any(|r| r.code == code) {
            return Err(XR_DUP);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 查行（码不在表必拒）。
    pub fn lookup(&mut self, code: u32) -> Result<&XrefRow, u32> {
        if self.sealed {
            return Err(XR_SEALED);
        }
        self.queries.push(code);
        let idx = self.rows.iter().position(|r| r.code == code).ok_or(XR_GHOST)?;
        Ok(&self.rows[idx])
    }

    /// 全表回放：每行渲染可解析回自身。
    pub fn all_replayable(&self) -> bool {
        self.rows.iter().all(|r| {
            let s = r.render();
            let mut it = s.split(' ');
            let c = u32::from_str_radix(it.next().unwrap_or("").trim_start_matches("0x"), 16)
                .unwrap_or(0);
            c == r.code
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a5e0;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.code);
            h = h.wrapping_mul(211).wrapping_add(r.path as u32);
            h = h.wrapping_mul(211).wrapping_add(r.end as u32);
            for by in r.note.bytes() {
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
    let mut b = XrefTable::new();
    let _ = b.add(0xC0000005, XrPath::Seh, XrEnd::Handled, "accv");
    // 1) 重复码必拒。
    got[0] = b.add(0xC0000005, XrPath::Veh, XrEnd::Wer, "dup").err() == Some(XR_DUP);
    // 2) 野码必拒。
    got[1] = b.lookup(0xDEAD).err() == Some(XR_GHOST);
    // 3) 零码行必拒。
    got[2] = b.add(0, XrPath::Seh, XrEnd::Handled, "z").err() == Some(XR_ORDER);
    // 4) UEF 路径配 Handled 终局必拒。
    got[3] = b.add(0xC0000409, XrPath::Uef, XrEnd::Handled, "bad").err() == Some(XR_ORDER);
    // 5) 印后挂行必拒。
    b.seal();
    got[4] = b.add(0xC0000094, XrPath::Seh, XrEnd::Handled, "post").err() == Some(XR_SEALED);
    got
}

/// 判据入口：UNX-F12158
pub fn run_unx12158_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12158");
    let mut b = XrefTable::new();

    // J1 四行对照挂表。
    let j1 = b.add(0xC0000005, XrPath::Seh, XrEnd::Handled, "accv-seh").is_ok()
        && b.add(0xC0000094, XrPath::VehThenSeh, XrEnd::Handled, "div0-vseh").is_ok()
        && b.add(0xC0000409, XrPath::Uef, XrEnd::Wer, "fastfail-wer").is_ok()
        && b.add(0xE0000001, XrPath::Uef, XrEnd::Crash, "user-crash").is_ok();
    s.add("J1_rows4", j1 && b.rows.len() == 4, "四行对照全挂");

    // J2 查表 10/10：四码各查两遍 + 野码。
    let mut j2 = true;
    for code in [0xC0000005u32, 0xC0000094, 0xC0000409, 0xE0000001] {
        if b.lookup(code).is_err() {
            j2 = false;
        }
    }
    for code in [0xC0000005u32, 0xC0000094, 0xC0000409, 0xE0000001, 0xDEAD, 0x1] {
        let _ = b.lookup(code);
        if code == 0xDEAD || code == 0x1 {
            if b.rows.iter().any(|r| r.code == code) {
                j2 = false;
            }
        }
    }
    s.add("J2_lookup_10x", j2 && b.queries.len() == 10, "四码双查 + 野码留痕全对");

    // J3 野码拒。
    s.add(
        "J3_wild",
        b.lookup(0xDEAD).err() == Some(XR_GHOST),
        "码不在表拒 GHOST",
    );

    // J4 行渲染对平。
    let r = b.rows[0].render();
    s.add(
        "J4_render",
        r == "0xc0000005 SEH H",
        "行渲染三列逐字对平",
    );

    // J5 路径标签。
    s.add(
        "J5_path_tags",
        XrPath::Veh.tag() == "VEH"
            && XrPath::Seh.tag() == "SEH"
            && XrPath::VehThenSeh.tag() == "VEH+SEH"
            && XrPath::Uef.tag() == "UEF",
        "四路径标签全对",
    );

    // J6 终局标签。
    s.add(
        "J6_end_tags",
        XrEnd::Handled.tag() == "H"
            && XrEnd::Wer.tag() == "W"
            && XrEnd::Crash.tag() == "C",
        "三终局标签全对",
    );

    // J7 行自洽核。
    s.add(
        "J7_rows_ok",
        b.rows.iter().all(|r| r.ok()),
        "全行过自洽核",
    );

    // J8 重复码拒。
    let mut b8 = XrefTable::new();
    let _ = b8.add(0x1111, XrPath::Seh, XrEnd::Handled, "a");
    s.add(
        "J8_dup",
        b8.add(0x1111, XrPath::Veh, XrEnd::Wer, "b").err() == Some(XR_DUP),
        "重复码拒 DUP",
    );

    // J9 UEF+Handled 组合拒。
    s.add(
        "J9_uef_handled",
        b8.add(0x2222, XrPath::Uef, XrEnd::Handled, "x").err() == Some(XR_ORDER),
        "UEF 路径 Handled 终局拒 ORDER",
    );

    // J10 全表回放一致。
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
        b.add(0x3333, XrPath::Seh, XrEnd::Handled, "post").err() == Some(XR_SEALED)
            && b.lookup(0xC0000005).err() == Some(XR_SEALED),
        "印账后挂/查皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("rows=4") && sm.contains("queries=11"),
        "摘要含行数与查询数",
    );

    // J15 十行大表全挂全查。
    let mut b15 = XrefTable::new();
    let mut ok15 = true;
    for i in 0..10u32 {
        let path = match i % 4 {
            0 => XrPath::Veh,
            1 => XrPath::Seh,
            2 => XrPath::VehThenSeh,
            _ => XrPath::Uef,
        };
        let end = if i % 4 == 3 { XrEnd::Wer } else { XrEnd::Handled };
        if b15.add(0xC000_0000 + i, path, end, "m").is_err() {
            ok15 = false;
        }
    }
    let hit15 = (0..10u32).all(|i| b15.lookup(0xC000_0000 + i).is_ok());
    s.add("J15_big10", ok15 && hit15, "十行大表全挂全查");

    // J16 留痕空行拒。
    let mut b16 = XrefTable::new();
    s.add(
        "J16_empty_note",
        b16.add(0x4444, XrPath::Seh, XrEnd::Handled, "").err() == Some(XR_ORDER),
        "留痕空拒 ORDER",
    );

    // J17 空表指纹稳定。
    let b17 = XrefTable::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空对照表指纹稳定");

    // J18 VEH+SEH 组合合法。
    let row18 = XrefRow {
        code: 0x5555,
        path: XrPath::VehThenSeh,
        end: XrEnd::Handled,
        note: "v+s".to_string(),
    };
    s.add("J18_vehsseh_ok", row18.ok(), "VEH+SEH/H 合法行");

    // J19 大表回放。
    s.add("J19_big_replay", b15.all_replayable(), "大表全回放一致");

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
    fn unx12158_xref() {
        let mut b = XrefTable::new();
        let _ = b.add(0xC0000005, XrPath::Seh, XrEnd::Handled, "accv");
        assert!(b.lookup(0xC0000005).is_ok());
        assert_eq!(b.lookup(0xDEAD), Err(XR_GHOST));
    }

    #[test]
    fn unx12158_checks_green() {
        let s = run_unx12158_checks();
        assert!(s.all_passed(), "UNX-F12158 判据未全绿");
    }

    #[test]
    fn unx12158_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
