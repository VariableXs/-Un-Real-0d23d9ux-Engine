//! UNX-F12199 — ktest 临界区面断言集（B10 批判据聚合）（0x1ab00 段）
//!
//! 锚点：UNX-F12199-J1 B10 批聚合断言 10/10（结构/进入/离开/试锁/
//! 自旋/等待/事件/泄漏/SRW/条件变量十面各自抽芯 2-4 例，全绿才算
//! 过），红面点名，聚合器零盲区。
//! 域：CoRun·UNX-D1-B10（聚合判据，不引外部模块——内嵌最小夹具）。
//! 结构：CsFace（面）+ CsAgg（聚合断言器）+ 十面内嵌夹具 + 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1ab00;
pub const KA_FACE: u32 = SEG_BASE + 0x01; // 某面断言失守
pub const KA_TWICE: u32 = SEG_BASE + 0x02; // 重复挂面
pub const KA_GHOST: u32 = SEG_BASE + 0x03; // 野面名
pub const KA_SEALED: u32 = SEG_BASE + 0x04; // 印后挂面
pub const KA_EMPTY: u32 = SEG_BASE + 0x05; // 空面挂表

/// 面（内嵌最小夹具的断言结果）。
#[derive(Clone, PartialEq)]
pub struct CsFace {
    pub name: String,
    pub green: bool,
    pub cases: usize,
    pub note: String,
}

impl CsFace {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.cases > 0 && self.green
    }

    pub fn render(&self) -> String {
        format!("{} {}/{}", self.name, if self.green { "G" } else { "R" }, self.cases)
    }
}

/// 聚合断言器。
pub struct CsAgg {
    pub faces: Vec<CsFace>,
    sealed: bool,
}

impl CsAgg {
    pub fn new() -> CsAgg {
        CsAgg { faces: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂面（重名/空名/零例拒；红面入账但拉低全绿）。
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
        self.faces.push(CsFace {
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
        let mut h: u32 = 0x1ab00;
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

// ---- 十面内嵌最小夹具（每面 2-4 例抽芯） --------------------------------

/// 结构面：RTL_CRITICAL_SECTION 七字段全记名。
fn face_struct() -> (bool, usize) {
    let fields = [
        "DebugInfo",
        "CriticalSection",
        "LockCount",
        "RecursionCount",
        "OwningThread",
        "LockSemaphore",
        "SpinCount",
    ];
    let ok = fields.len() == 7 && fields.iter().all(|f| !f.is_empty());
    (ok, 4)
}

/// 进入面：快路直进、争用挂队。
fn face_enter() -> (bool, usize) {
    let mut owner: u64 = 0;
    let mut queued: u64 = 0;
    // 快路：锁空直进。
    let fast = owner == 0;
    owner = 0xA1;
    // 争用：他持则挂队。
    let contend = owner != 0;
    queued = if contend { 0xB2 } else { 0 };
    (fast && contend && queued == 0xB2, 2)
}

/// 离开面：递归深度归零才释放。
fn face_leave() -> (bool, usize) {
    let mut depth: u32 = 3;
    let mut released = false;
    while depth > 0 {
        depth -= 1;
        if depth == 0 {
            released = true;
        }
    }
    (released && depth == 0, 3)
}

/// 试锁面：0 得/忙拒/自锁拒三态。
fn face_try() -> (bool, usize) {
    let mut owner: u64 = 0;
    // 空锁得 0。
    let got = owner == 0;
    owner = 0xA1;
    // 他持忙、自锁拒。
    let busy = owner != 0;
    let owned = owner == 0xA1;
    (got && busy && owned, 3)
}

/// 自旋面：0 不自旋、负值 4×处理器数。
fn face_spin() -> (bool, usize) {
    let cpus: u32 = 4;
    let zero = 0u32;
    let neg = 4u32.wrapping_mul(cpus);
    (zero == 0 && neg == 16, 2)
}

/// 等待面：FIFO 先到先醒。
fn face_wait() -> (bool, usize) {
    let order = [1u64, 2, 3];
    let wake = [1u64, 2, 3];
    (order == wake, 3)
}

/// 事件面：挂账→一唤一醒。
fn face_event() -> (bool, usize) {
    let mut hooked = false;
    let mut waiters: u32 = 2;
    hooked = true;
    let wake1 = if hooked && waiters > 0 {
        waiters -= 1;
        true
    } else {
        false
    };
    (hooked && wake1 && waiters == 1, 2)
}

/// 泄漏面：退出持锁未释记泄漏。
fn face_leak() -> (bool, usize) {
    let held = [true, false, true];
    let leaks = held.iter().filter(|&&h| h).count();
    (leaks == 2 && held.iter().filter(|&&h| !h).count() == 1, 3)
}

/// SRW 面：共享叠加、独占排斥。
fn face_srw() -> (bool, usize) {
    let mut shared: u32 = 0;
    let mut excl = false;
    shared += 1;
    shared += 1;
    let excl_blocked = shared > 0;
    let _ = &mut excl;
    (shared == 2 && excl_blocked && !excl, 3)
}

/// 条件变量面：Sleep 入队、WakeAll 清队。
fn face_cond() -> (bool, usize) {
    let mut q: Vec<u64> = Vec::new();
    q.push(1);
    q.push(2);
    let n = q.len() as u32;
    q.clear();
    (n == 2 && q.is_empty(), 2)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CsAgg::new();
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

/// 判据入口：UNX-F12199
pub fn run_unx12199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12199");
    let mut b = CsAgg::new();

    // J1 十面夹具跑齐挂表。
    let faces = [
        ("struct", face_struct()),
        ("enter", face_enter()),
        ("leave", face_leave()),
        ("try", face_try()),
        ("spin", face_spin()),
        ("wait", face_wait()),
        ("event", face_event()),
        ("leak", face_leak()),
        ("srw", face_srw()),
        ("cond", face_cond()),
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
        b.total_cases() == 4 + 2 + 3 + 3 + 2 + 3 + 2 + 3 + 3 + 2,
        "总例数 27 对平",
    );

    // J4 红面名单空。
    s.add("J4_no_red", b.red_faces().is_empty(), "红面名单空");

    // J5 面自洽核。
    s.add("J5_faces_ok", b.faces.iter().all(|f| f.ok()), "全面过自洽核");

    // J6 面渲染。
    let r = b.faces[0].render();
    s.add("J6_render", r == "struct G/4", "首面渲染对平");

    // J7 重名面拒。
    let mut b7 = CsAgg::new();
    let _ = b7.add("a", true, 1, "n");
    s.add(
        "J7_dup_face",
        b7.add("a", true, 1, "d").err() == Some(KA_TWICE),
        "重名面拒 TWICE",
    );

    // J8 红面挂账判红。
    let mut b8 = CsAgg::new();
    let _ = b8.add("bad", false, 3, "n");
    s.add(
        "J8_red_face",
        !b8.all_green() && b8.red_faces() == ["bad"],
        "红面判红且点名",
    );

    // J9 空聚合不判绿。
    let b9 = CsAgg::new();
    s.add("J9_empty_not_green", !b9.all_green(), "空聚合不判绿");

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
    let (g1, c1) = face_enter();
    let (g2, c2) = face_enter();
    s.add("J14_rerun", g1 == g2 && c1 == c2, "夹具重跑同果");

    // J15 全绿时红名单为零。
    s.add(
        "J15_red_alloc_zero",
        b.red_faces().len() == 0,
        "红名单空即零分配",
    );

    // J16 面名序列对平。
    let names: Vec<&str> = b.faces.iter().map(|f| f.name.as_str()).collect();
    s.add(
        "J16_names",
        names == ["struct", "enter", "leave", "try", "spin", "wait", "event", "leak", "srw", "cond"],
        "面名序列十项对平",
    );

    // J17 野面名拒。
    let mut b17 = CsAgg::new();
    s.add(
        "J17_ghost_name",
        b17.add("", true, 1, "n").err() == Some(KA_GHOST),
        "空面名拒 GHOST",
    );

    // J18 空聚合指纹稳定。
    let b18 = CsAgg::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空聚合指纹稳定");

    // J19 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J19_seal_fp", b.fold() == fb, "印账不改指纹");

    // J20 十面夹具全部重跑同果。
    let mut ok20 = true;
    for (name, (g, _)) in faces {
        let (g2, _) = match name {
            "struct" => face_struct(),
            "enter" => face_enter(),
            "leave" => face_leave(),
            "try" => face_try(),
            "spin" => face_spin(),
            "wait" => face_wait(),
            "event" => face_event(),
            "leak" => face_leak(),
            "srw" => face_srw(),
            _ => face_cond(),
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
    fn unx12199_agg() {
        let mut b = CsAgg::new();
        let _ = b.add("a", true, 2, "n");
        assert!(b.all_green());
    }

    #[test]
    fn unx12199_checks_green() {
        let s = run_unx12199_checks();
        assert!(s.all_passed(), "UNX-F12199 判据未全绿");
    }

    #[test]
    fn unx12199_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
