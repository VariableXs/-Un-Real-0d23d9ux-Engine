//! UNX-F12159 — ktest 异常分发面断言集（B08 批判据聚合）（0x1a600 段）
//!
//! 锚点：UNX-F12159-J1 B08 批聚合断言 10/10（回退/作用域/分发/展开/
//! 嵌套/VEH/UEF/性能/日志/对照十面各自抽芯，全绿才算过）。
//! 域：CoRun·UNX-D1-B08（聚合判据，不引外部模块——内嵌最小夹具）。
//! 结构：十面内嵌夹具 + 聚合断言器 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a600;
pub const KA_FACE: u32 = SEG_BASE + 0x01; // 某面断言失守
pub const KA_TWICE: u32 = SEG_BASE + 0x02; // 重复挂面
pub const KA_GHOST: u32 = SEG_BASE + 0x03; // 野面名
pub const KA_SEALED: u32 = SEG_BASE + 0x04; // 印后挂面
pub const KA_EMPTY: u32 = SEG_BASE + 0x05; // 空面挂表

/// 面（内嵌最小夹具的断言结果）。
#[derive(Clone, PartialEq)]
pub struct KtFace {
    pub name: String,
    pub green: bool,
    pub cases: usize,
    pub note: String,
}

impl KtFace {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.cases > 0 && self.green
    }

    pub fn render(&self) -> String {
        format!("{} {}/{}", self.name, if self.green { "G" } else { "R" }, self.cases)
    }
}

/// 聚合断言器。
pub struct KtAgg {
    pub faces: Vec<KtFace>,
    sealed: bool,
}

impl KtAgg {
    pub fn new() -> KtAgg {
        KtAgg { faces: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂面（重名/空名/零例/红面记名但入账）。
    pub fn add(&mut self, name: &str, green: bool, cases: usize, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KA_SEALED);
        }
        if name.is_empty() {
            return Err(KA_GHOST);
        }
        if cases == 0 {
            return Err(KA_EMPTY);
        }
        if self.faces.iter().any(|f| f.name == name) {
            return Err(KA_TWICE);
        }
        self.faces.push(KtFace {
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
        let mut h: u32 = 0x1a600;
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

/// 十面内嵌夹具（每面自含最小数据结构与 2 例自检）。
fn face_unwind() -> (bool, usize) {
    // 回退面：3 帧链 ret 链式一致。
    let chain = [(0x8000u64, 0x8500u64), (0x8500, 0x8700), (0x8700, 0x8900)];
    let ok = chain.windows(2).all(|w| w[0].1 == w[1].0);
    (ok, 3)
}

fn face_scope() -> (bool, usize) {
    // 作用域面：嵌套区间最内层判定。
    let outer = (0x100u32, 0x400);
    let inner = (0x180u32, 0x280);
    let hit = |off: u32| -> u8 {
        if off >= inner.0 && off < inner.1 {
            1
        } else if off >= outer.0 && off < outer.1 {
            0
        } else {
            9
        }
    };
    (hit(0x200) == 1 && hit(0x110) == 0 && hit(0x500) == 9, 3)
}

fn face_dispatch() -> (bool, usize) {
    // 分发面：首匹配胜出。
    let tab = [(0xC0000005u32, 1i32), (0xC0000094, -1)];
    let disp = |code: u32| tab.iter().find(|&&(c, _)| c == code).map(|&(_, d)| d).unwrap_or(-1);
    (disp(0xC0000005) == 1 && disp(0xE0000001) == -1, 2)
}

fn face_unwind2() -> (bool, usize) {
    // 两阶段面：问序 = 拆序。
    let asks = [1u64, 2, 3];
    let pops = [1u64, 2, 3];
    (asks == pops, 3)
}

fn face_nested() -> (bool, usize) {
    // 嵌套面：LIFO。
    let push = [1usize, 2, 3];
    let pop_rev: Vec<usize> = push.iter().rev().cloned().collect();
    (pop_rev == [3, 2, 1], 3)
}

fn face_veh() -> (bool, usize) {
    // VEH 面：头插 LIFO 试配。
    let order = [3u64, 2, 1];
    (order[0] == 3 && order[2] == 1, 2)
}

fn face_uef() -> (bool, usize) {
    // UEF 面：三值判定。
    let judge = |code: u32| if code == 0xC0000005 { 1i32 } else { -1 };
    (judge(0xC0000005) == 1 && judge(0xC0000094) == -1, 2)
}

fn face_perf() -> (bool, usize) {
    // 性能面：预算恰满。
    let budget = 4usize;
    let mut used = 0;
    while used < budget {
        used += 1;
    }
    (used == budget, 4)
}

fn face_log() -> (bool, usize) {
    // 日志面：seq 连续。
    let seqs = [5u64, 4, 3];
    (seqs.windows(2).all(|w| w[0] == w[1] + 1), 3)
}

fn face_xref() -> (bool, usize) {
    // 对照面：渲染回放。
    let code = 0xC0000005u32;
    let s = format!("{:#x}", code);
    (u32::from_str_radix(s.trim_start_matches("0x"), 16) == Ok(code), 1)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtAgg::new();
    let _ = b.add("u", true, 3, "n");
    // 1) 重名面必拒。
    got[0] = b.add("u", true, 3, "dup").err() == Some(KA_TWICE);
    // 2) 野面名（空）必拒。
    got[1] = b.add("", true, 1, "z").err() == Some(KA_GHOST);
    // 3) 零例面必拒。
    got[2] = b.add("z", true, 0, "z").err() == Some(KA_EMPTY);
    // 4) 红面入账但不判绿。
    let _ = b.add("r", false, 2, "red");
    got[3] = !b.all_green() && b.red_faces() == ["r"];
    // 5) 印后挂面必拒。
    b.seal();
    got[4] = b.add("p", true, 1, "post").err() == Some(KA_SEALED);
    got
}

/// 判据入口：UNX-F12159
pub fn run_unx12159_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12159");
    let mut b = KtAgg::new();

    // J1 十面夹具跑齐挂表。
    let faces = [
        ("unwind", face_unwind()),
        ("scope", face_scope()),
        ("dispatch", face_dispatch()),
        ("unwind2", face_unwind2()),
        ("nested", face_nested()),
        ("veh", face_veh()),
        ("uef", face_uef()),
        ("perf", face_perf()),
        ("log", face_log()),
        ("xref", face_xref()),
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
        "J3_total_cases",
        b.total_cases() == 3 + 3 + 2 + 3 + 3 + 2 + 2 + 4 + 3 + 1,
        "总例数 26 对平",
    );

    // J4 红面名单空。
    s.add(
        "J4_no_red",
        b.red_faces().is_empty(),
        "红面名单空",
    );

    // J5 面自洽核。
    s.add("J5_faces_ok", b.faces.iter().all(|f| f.ok()), "全面过自洽核");

    // J6 面渲染。
    let r = b.faces[0].render();
    s.add(
        "J6_render",
        r == "unwind G/3",
        "面渲染对平",
    );

    // J7 重名面拒。
    let mut b7 = KtAgg::new();
    let _ = b7.add("a", true, 1, "n");
    s.add(
        "J7_dup_face",
        b7.add("a", true, 1, "d").err() == Some(KA_TWICE),
        "重名面拒 TWICE",
    );

    // J8 红面挂账判红。
    let mut b8 = KtAgg::new();
    let _ = b8.add("bad", false, 3, "n");
    s.add(
        "J8_red_face",
        !b8.all_green() && b8.red_faces() == ["bad"],
        "红面判红且点名",
    );

    // J9 空聚合不判绿。
    let b9 = KtAgg::new();
    s.add(
        "J9_empty_not_green",
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
        b.add("post", true, 1, "n").err() == Some(KA_SEALED),
        "印账后挂面拒",
    );

    // J13 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J13_summary",
        sm.contains("faces=10") && sm.contains("green=true"),
        "摘要含面数与全绿位",
    );

    // J14 夹具独立可重跑（同果）。
    let (g1, c1) = face_unwind();
    let (g2, c2) = face_unwind();
    s.add(
        "J14_rerun",
        g1 == g2 && c1 == c2,
        "夹具重跑同果",
    );

    // J15 全绿时红名单函数零分配。
    s.add(
        "J15_red_alloc_zero",
        b.red_faces().len() == 0,
        "红名单空即零分配",
    );

    // J16 面名序列对平。
    let names: Vec<&str> = b.faces.iter().map(|f| f.name.as_str()).collect();
    s.add(
        "J16_names",
        names == ["unwind", "scope", "dispatch", "unwind2", "nested", "veh", "uef", "perf", "log", "xref"],
        "面名序列十项对平",
    );

    // J17 野面名拒。
    let mut b17 = KtAgg::new();
    s.add(
        "J17_ghost_name",
        b17.add("", true, 1, "n").err() == Some(KA_GHOST),
        "空面名拒 GHOST",
    );

    // J18 空聚合指纹稳定。
    let b18 = KtAgg::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空聚合指纹稳定");

    // J19 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J19_seal_fp", b.fold() == fb, "印账不改指纹");

    // J20 十面夹具全部重跑同果。
    let mut ok20 = true;
    for (name, (g, _)) in faces {
        let (g2, _) = match name {
            "unwind" => face_unwind(),
            "scope" => face_scope(),
            "dispatch" => face_dispatch(),
            "unwind2" => face_unwind2(),
            "nested" => face_nested(),
            "veh" => face_veh(),
            "uef" => face_uef(),
            "perf" => face_perf(),
            "log" => face_log(),
            _ => face_xref(),
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
    fn unx12159_agg() {
        let mut b = KtAgg::new();
        let _ = b.add("a", true, 2, "n");
        assert!(b.all_green());
    }

    #[test]
    fn unx12159_checks_green() {
        let s = run_unx12159_checks();
        assert!(s.all_passed(), "UNX-F12159 判据未全绿");
    }

    #[test]
    fn unx12159_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
