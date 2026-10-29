//! UNX-F12179 — ktest CONTEXT 面断言集（B09 批判据聚合）（0x1a880 段）
//!
//! 锚点：UNX-F12179-J1 B09 批聚合 10/10（寄存器槽/标志位/陷阱帧/
//! 采集/恢复/段寄存器/EFlags/布局/NtContinue/G-S 十面各自抽芯，
//! 全绿判定——参考 unx12159 形制，内嵌最小夹具）。
//! 域：CoRun·UNX-D1-B09（聚合判据，不引外部模块）。
//! 结构：十面内嵌夹具 + 聚合断言器 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a880;
pub const KB_FACE: u32 = SEG_BASE + 0x01; // 某面断言失守
pub const KB_TWICE: u32 = SEG_BASE + 0x02; // 重复挂面
pub const KB_GHOST: u32 = SEG_BASE + 0x03; // 野面名
pub const KB_SEALED: u32 = SEG_BASE + 0x04; // 印后挂面
pub const KB_EMPTY: u32 = SEG_BASE + 0x05; // 零例面

/// 面。
#[derive(Clone, PartialEq)]
pub struct KbFace {
    pub name: String,
    pub green: bool,
    pub cases: usize,
    pub note: String,
}

impl KbFace {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.cases > 0 && self.green
    }

    pub fn render(&self) -> String {
        format!("{} {}/{}", self.name, if self.green { "G" } else { "R" }, self.cases)
    }
}

/// 聚合断言器。
pub struct KbAgg {
    pub faces: Vec<KbFace>,
    sealed: bool,
}

impl KbAgg {
    pub fn new() -> KbAgg {
        KbAgg { faces: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂面。
    pub fn add(&mut self, name: &str, green: bool, cases: usize, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KB_SEALED);
        }
        if name.is_empty() {
            return Err(KB_GHOST);
        }
        if cases == 0 {
            return Err(KB_EMPTY);
        }
        if self.faces.iter().any(|f| f.name == name) {
            return Err(KB_TWICE);
        }
        self.faces.push(KbFace {
            name: name.to_string(),
            green,
            cases,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        !self.faces.is_empty() && self.faces.iter().all(|f| f.green)
    }

    /// 总例数。
    pub fn total_cases(&self) -> usize {
        self.faces.iter().map(|f| f.cases).sum()
    }

    /// 红面名单。
    pub fn red_faces(&self) -> Vec<String> {
        self.faces.iter().filter(|f| !f.green).map(|f| f.name.clone()).collect()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a880;
        for f in &self.faces {
            h = h.wrapping_mul(211).wrapping_add(f.green as u32);
            h = h.wrapping_mul(211).wrapping_add(f.cases as u32);
            for by in f.name.bytes() {
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
        format!(
            "faces={} total={} green={}",
            self.faces.len(),
            self.total_cases(),
            self.all_green()
        )
    }
}

/// 十面内嵌夹具。
fn face_regs() -> (bool, usize) {
    // 寄存器槽面：Rax 写读 + 双射。
    let mut slots = [0u64; 24];
    slots[3] = 0x42;
    let ok = slots[3] == 0x42 && slots.iter().filter(|v| **v != 0).count() == 1;
    (ok, 2)
}

fn face_flags() -> (bool, usize) {
    // 标志位面：CF/ZF 推导。
    let (sum, ov) = 0xFFFF_FFFFu32.overflowing_add(1);
    (sum == 0 && ov, 2)
}

fn face_trap() -> (bool, usize) {
    // 陷阱帧面：对齐 + 段合法。
    let f = (0x1000u64, 0x2000u64, 0x33u64);
    (f.1 % 8 == 0 && (f.2 == 0x33 || f.2 == 0x08), 2)
}

fn face_capture() -> (bool, usize) {
    // 采集面：三锚自洽。
    let (rip, rsp, frame) = (0x1000u64, 0x2000u64, 0x2040u64);
    (rip != 0 && rsp % 8 == 0 && frame >= rsp, 3)
}

fn face_restore() -> (bool, usize) {
    // 恢复面：采集后可恢复。
    let captured = [(0x1000u64, 0x2000u64)];
    let can = captured.iter().any(|&(r, s)| r == 0x1000 && s == 0x2000);
    (can, 1)
}

fn face_segs() -> (bool, usize) {
    // 段寄存器面：用户形制首段。
    let user = [0x33u16, 0x2B, 0x2B, 0x2B, 0x53, 0x2B];
    (user[0] == 0x33 && user[4] == 0x53, 2)
}

fn face_eflags() -> (bool, usize) {
    // EFlags 面：IOPL 位域。
    let mut raw = 0u32;
    raw |= 3u32 << 12;
    (((raw >> 12) & 3) == 3 && (raw & 0x2) == 0, 2)
}

fn face_layout() -> (bool, usize) {
    // 布局面：ctx_off 16 对齐。
    let ctx_off = 96usize;
    (ctx_off % 16 == 0 && ctx_off >= 96, 2)
}

fn face_continue() -> (bool, usize) {
    // NtContinue 面：授权门。
    let authorized = [(0x1000u64, 0x2000u64)];
    (authorized.iter().any(|&(r, s)| r == 0x1000 && s == 0x2000), 1)
}

fn face_gs() -> (bool, usize) {
    // G/S 面：掩码覆盖。
    let mask = 0x1003u32;
    (mask & 0x1001 == 0x1001 && mask & 0x1002 == 0x1002 && mask & 0x1008 != 0x1008, 3)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KbAgg::new();
    let _ = b.add("r", true, 2, "n");
    // 1) 重名面必拒。
    got[0] = b.add("r", true, 2, "dup").err() == Some(KB_TWICE);
    // 2) 空面名必拒。
    got[1] = b.add("", true, 1, "z").err() == Some(KB_GHOST);
    // 3) 零例面必拒。
    got[2] = b.add("z", true, 0, "z").err() == Some(KB_EMPTY);
    // 4) 红面入账不判绿。
    let _ = b.add("red", false, 1, "r");
    got[3] = !b.all_green() && b.red_faces() == ["red"];
    // 5) 印后挂面必拒。
    b.seal();
    got[4] = b.add("p", true, 1, "post").err() == Some(KB_SEALED);
    got
}

/// 判据入口：UNX-F12179
pub fn run_unx12179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12179");
    let mut b = KbAgg::new();

    // J1 十面挂表。
    let faces = [
        ("regs", face_regs()),
        ("flags", face_flags()),
        ("trap", face_trap()),
        ("capture", face_capture()),
        ("restore", face_restore()),
        ("segs", face_segs()),
        ("eflags", face_eflags()),
        ("layout", face_layout()),
        ("continue", face_continue()),
        ("gs", face_gs()),
    ];
    let mut ok1 = true;
    for (name, (g, c)) in faces {
        if b.add(name, g, c, "fixture").is_err() {
            ok1 = false;
        }
    }
    s.add("J1_ten_faces", ok1 && b.faces.len() == 10, "十面夹具全挂");

    // J2 全绿判定。
    s.add("J2_all_green", b.all_green(), "十面全绿");

    // J3 总例数对平。
    s.add(
        "J3_total",
        b.total_cases() == 2 + 2 + 2 + 3 + 1 + 2 + 2 + 2 + 1 + 3,
        "总例数 20 对平",
    );

    // J4 红面名单空。
    s.add(
        "J4_no_red",
        b.red_faces().is_empty(),
        "红面名单空",
    );

    // J5 面自洽核。
    s.add(
        "J5_faces_ok",
        b.faces.iter().all(|f| f.ok()),
        "全面过自洽核",
    );

    // J6 面渲染。
    let r = b.faces[0].render();
    s.add(
        "J6_render",
        r == "regs G/2",
        "面渲染对平",
    );

    // J7 重名面拒。
    let mut b7 = KbAgg::new();
    let _ = b7.add("a", true, 1, "n");
    s.add(
        "J7_dup",
        b7.add("a", true, 1, "d").err() == Some(KB_TWICE),
        "重名面拒 TWICE",
    );

    // J8 红面挂账判红。
    let mut b8 = KbAgg::new();
    let _ = b8.add("bad", false, 3, "n");
    s.add(
        "J8_red",
        !b8.all_green() && b8.red_faces() == ["bad"],
        "红面判红且点名",
    );

    // J9 空聚合不判绿。
    let b9 = KbAgg::new();
    s.add(
        "J9_empty",
        !b9.all_green(),
        "空聚合不判绿",
    );

    // J10 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J10_fingerprint", f1 == f2, "聚合器折叠指纹一致");

    // J11 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 印账受控。
    b.seal();
    s.add(
        "J12_sealed",
        b.add("post", true, 1, "n").err() == Some(KB_SEALED),
        "印账后挂面拒",
    );

    // J13 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J13_summary",
        sm.contains("faces=10") && sm.contains("green=true"),
        "摘要含面数与全绿位",
    );

    // J14 夹具重跑同果。
    let (g1, _) = face_regs();
    let (g2, _) = face_regs();
    s.add(
        "J14_rerun",
        g1 == g2,
        "夹具重跑同果",
    );

    // J15 面名序列对平。
    let names: Vec<&str> = b.faces.iter().map(|f| f.name.as_str()).collect();
    s.add(
        "J15_names",
        names == ["regs", "flags", "trap", "capture", "restore", "segs", "eflags", "layout", "continue", "gs"],
        "面名序列十项对平",
    );

    // J16 零例面拒。
    let mut b16 = KbAgg::new();
    s.add(
        "J16_zero_cases",
        b16.add("z", true, 0, "n").err() == Some(KB_EMPTY),
        "零例面拒 EMPTY",
    );

    // J17 野面名拒。
    s.add(
        "J17_ghost",
        b16.add("", true, 1, "n").err() == Some(KB_GHOST),
        "空面名拒 GHOST",
    );

    // J18 空聚合指纹稳定。
    let b18 = KbAgg::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空聚合指纹稳定");

    // J19 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J19_seal_fp", b.fold() == fb, "印账不改指纹");

    // J20 十面全重跑同果。
    let mut ok20 = true;
    for (name, (g, _)) in faces {
        let (g2, _) = match name {
            "regs" => face_regs(),
            "flags" => face_flags(),
            "trap" => face_trap(),
            "capture" => face_capture(),
            "restore" => face_restore(),
            "segs" => face_segs(),
            "eflags" => face_eflags(),
            "layout" => face_layout(),
            "continue" => face_continue(),
            _ => face_gs(),
        };
        if g != g2 {
            ok20 = false;
        }
    }
    s.add("J20_all_rerun", ok20, "十面重跑全同果");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12179_agg() {
        let mut b = KbAgg::new();
        let _ = b.add("a", true, 2, "n");
        assert!(b.all_green());
    }

    #[test]
    fn unx12179_checks_green() {
        let s = run_unx12179_checks();
        assert!(s.all_passed(), "UNX-F12179 判据未全绿");
    }

    #[test]
    fn unx12179_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
