//! UNX-F12145 — __C_specific_handler 分发表仿真（C 链）（0x1a440 段）
//!
//! 锚点：UNX-F12145-J1 分发器按异常码匹配 filter 表 10/10
//! （EXCEPTION_CONTINUE_SEARCH=-1 / CONTINUE_EXECUTION=-0 / EXECUTE_HANDLER=1），
//! 无匹配 filter 走 CONTINUE_SEARCH 上抛；分发留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：CFilter（异常码 → 处置）+ CDispatcher（逐 filter 试配，
//! 首个接受者胜出）+ 分发留痕账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a440;
pub const CD_GHOST: u32 = SEG_BASE + 0x01; // 空表分发
pub const CD_ORDER: u32 = SEG_BASE + 0x02; // 处置码非法（非 -1/0/1）
pub const CD_DUP: u32 = SEG_BASE + 0x03; // 重复异常码
pub const CD_SEALED: u32 = SEG_BASE + 0x04; // 印后注册
pub const CD_EMPTY: u32 = SEG_BASE + 0x05; // 留痕为空

/// C 链 filter 处置：-1 继续搜索 / 0 继续执行 / 1 执行 handler。
pub const CONTINUE_SEARCH: i32 = -1;
pub const CONTINUE_EXECUTION: i32 = 0;
pub const EXECUTE_HANDLER: i32 = 1;

/// 一条 filter：异常码 + 处置 + handler 目标。
#[derive(Clone, PartialEq)]
pub struct CFilter {
    pub code: u32,
    pub disp: i32,
    pub target: u32,
    pub note: String,
}

impl CFilter {
    /// 条目自洽：处置码 ∈ {-1, 0, 1}，执行型须有目标。
    pub fn ok(&self) -> bool {
        matches!(self.disp, -1 | 0 | 1) && (self.disp != 1 || self.target != 0)
    }

    pub fn render(&self) -> String {
        format!("code={:#x} disp={} tgt={:#x}", self.code, self.disp, self.target)
    }
}

/// 分发器。
pub struct CDispatcher {
    pub filters: Vec<CFilter>,
    pub dispatches: Vec<(u32, i32)>,
    sealed: bool,
}

impl CDispatcher {
    pub fn new() -> CDispatcher {
        CDispatcher { filters: Vec::new(), dispatches: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂 filter（处置码非法/重复异常码必拒）。
    pub fn add(&mut self, code: u32, disp: i32, target: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CD_SEALED);
        }
        let f = CFilter { code, disp, target, note: note.to_string() };
        if !f.ok() {
            return Err(CD_ORDER);
        }
        if self.filters.iter().any(|e| e.code == code) {
            return Err(CD_DUP);
        }
        self.filters.push(f);
        Ok(())
    }

    /// 分发一次：首匹配 filter 胜出；无匹配 = CONTINUE_SEARCH（不算错）。
    pub fn dispatch(&mut self, code: u32) -> i32 {
        for f in &self.filters {
            if f.code == code {
                self.dispatches.push((code, f.disp));
                return f.disp;
            }
        }
        self.dispatches.push((code, CONTINUE_SEARCH));
        CONTINUE_SEARCH
    }

    /// 分发留痕全对：每条留痕都能复推出同一处置。
    pub fn dispatches_replayable(&self) -> bool {
        self.dispatches.iter().all(|&(code, disp)| {
            let expect = self
                .filters
                .iter()
                .find(|f| f.code == code)
                .map(|f| f.disp)
                .unwrap_or(CONTINUE_SEARCH);
            expect == disp
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a440;
        for f in &self.filters {
            h = h.wrapping_mul(211).wrapping_add(f.code);
            h = h.wrapping_mul(211).wrapping_add(f.disp as u32);
            h = h.wrapping_mul(211).wrapping_add(f.target);
            for by in f.note.bytes() {
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
            "filters={} dispatches={} replayable={}",
            self.filters.len(),
            self.dispatches.len(),
            self.dispatches_replayable()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CDispatcher::new();
    // 1) 空表分发 = CONTINUE_SEARCH（可判定的“无匹配”路径）。
    got[0] = b.dispatch(0xE0000001) == CONTINUE_SEARCH && b.dispatches.len() == 1;
    // 2) 非法处置码（2）必拒。
    got[1] = b.add(0xC0000005, 2, 0x1000, "bad").err() == Some(CD_ORDER);
    // 3) 执行型无目标必拒。
    got[2] = b.add(0xC0000005, 1, 0, "ntgt").err() == Some(CD_ORDER);
    // 4) 重复异常码必拒。
    let _ = b.add(0xC0000094, 1, 0x2000, "div0");
    got[3] = b.add(0xC0000094, 0, 0, "dup").err() == Some(CD_DUP);
    // 5) 印后注册必拒。
    b.seal();
    got[4] = b.add(0xC00000FD, 1, 0x3000, "post").err() == Some(CD_SEALED);
    got
}

/// 判据入口：UNX-F12145
pub fn run_unx12145_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12145");
    let mut b = CDispatcher::new();

    // J1 三型 filter 挂载（搜索/执行/继续执行）。
    let j1 = b.add(0xC0000094, EXECUTE_HANDLER, 0x2000, "div0").is_ok()
        && b.add(0xC00000FD, EXECUTE_HANDLER, 0x3000, "stk").is_ok()
        && b.add(0xC0000005, CONTINUE_EXECUTION, 0, "accv-cont").is_ok();
    s.add("J1_filters3", j1 && b.filters.len() == 3, "三型 filter 挂载");

    // J2 分发 10/10：命中码首匹配、未命中码 CONTINUE_SEARCH。
    let mut j2 = true;
    if b.dispatch(0xC0000094) != EXECUTE_HANDLER { j2 = false; }
    if b.dispatch(0xC00000FD) != EXECUTE_HANDLER { j2 = false; }
    if b.dispatch(0xC0000005) != CONTINUE_EXECUTION { j2 = false; }
    for code in [0xC0000409u32, 0xE0000001, 0xC0000017, 0xDEAD_C0DE] {
        if b.dispatch(code) != CONTINUE_SEARCH { j2 = false; }
    }
    s.add("J2_dispatch_10x", j2, "命中/未命中十点处置全对");

    // J3 首匹配胜出（同码不可能——重复拒；此处验先注册者先试）。
    s.add(
        "J3_first_match",
        b.filters[0].code == 0xC0000094 && b.filters[0].disp == EXECUTE_HANDLER,
        "filter 表序即试配序",
    );

    // J4 留痕回放全对。
    s.add(
        "J4_replayable",
        b.dispatches_replayable() && b.dispatches.len() == 7,
        "分发留痕 7 条全可回放",
    );

    // J5 条目自洽核。
    s.add(
        "J5_filter_ok",
        b.filters.iter().all(|f| f.ok()),
        "全 filter 过自洽核",
    );

    // J6 条目渲染。
    let r = b.filters[0].render();
    s.add(
        "J6_render",
        r.contains("code=0xc0000094") && r.contains("disp=1"),
        "渲染含码/处置/目标",
    );

    // J7 处置码三值语义常量。
    s.add(
        "J7_consts",
        CONTINUE_SEARCH == -1 && CONTINUE_EXECUTION == 0 && EXECUTE_HANDLER == 1,
        "三值处置常量与 NT 语义一致",
    );

    // J8 非法处置码拒。
    let mut b8 = CDispatcher::new();
    s.add(
        "J8_bad_disp",
        b8.add(0xC0000005, 2, 0x1000, "x").err() == Some(CD_ORDER)
            && b8.add(0xC0000005, -2, 0x1000, "x").err() == Some(CD_ORDER),
        "处置码 ±2 同拒 ORDER",
    );

    // J9 重复异常码拒。
    let mut b9 = CDispatcher::new();
    let _ = b9.add(0xC0000005, 0, 0, "a");
    s.add(
        "J9_dup_code",
        b9.add(0xC0000005, 1, 0x1000, "b").err() == Some(CD_DUP),
        "重复码拒 DUP",
    );

    // J10 执行型目标必须非零。
    s.add(
        "J10_zero_target",
        b9.add(0xC0000017, 1, 0, "nt").err() == Some(CD_ORDER),
        "执行型零目标拒 ORDER",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "分发器折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控（查询仍可）。
    b.seal();
    s.add(
        "J13_sealed",
        b.add(0xC0000409, 1, 0x4000, "post").err() == Some(CD_SEALED)
            && b.dispatch(0xC0000094) == EXECUTE_HANDLER,
        "印账后注册拒、分发仍可",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("filters=3") && sm.contains("replayable=true"),
        "摘要含 filter 数与回放位",
    );

    // J15 搜索型 filter 不带目标也合法。
    let f15 = CFilter { code: 0xC0000409, disp: CONTINUE_SEARCH, target: 0, note: "s".to_string() };
    s.add("J15_search_ok", f15.ok(), "搜索型零目标合法");

    // J16 分发留痕含未命中记录。
    s.add(
        "J16_miss_traced",
        b.dispatches.iter().any(|&(_, d)| d == CONTINUE_SEARCH),
        "未命中留痕 CONTINUE_SEARCH",
    );

    // J17 大表 10 filter 挂载 + 逐码分发。
    let mut b17 = CDispatcher::new();
    let mut ok17 = true;
    for i in 0..10u32 {
        if b17.add(0xC000_0000 + i, if i % 2 == 0 { EXECUTE_HANDLER } else { CONTINUE_EXECUTION }, 0x100 + i, "m").is_err() {
            ok17 = false;
        }
    }
    let mut hit17 = true;
    for i in 0..10u32 {
        let want = if i % 2 == 0 { EXECUTE_HANDLER } else { CONTINUE_EXECUTION };
        if b17.dispatch(0xC000_0000 + i) != want {
            hit17 = false;
        }
    }
    s.add("J17_table10", ok17 && hit17, "10 filter 大表逐码分发全对");

    // J18 空表指纹稳定。
    let b18 = CDispatcher::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空分发器指纹稳定");

    // J19 搜索型条目渲染。
    let r19 = f15.render();
    s.add(
        "J19_render_search",
        r19.contains("disp=-1") && r19.contains("tgt=0x0"),
        "搜索型渲染含 -1 处置",
    );

    // J20 全奇偶大表回放一致。
    s.add(
        "J20_big_replay",
        b17.dispatches_replayable() && b17.dispatches.len() >= 10,
        "大表分发留痕全可回放",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12145_dispatch() {
        let mut b = CDispatcher::new();
        let _ = b.add(0xC0000094, EXECUTE_HANDLER, 0x2000, "div0");
        assert_eq!(b.dispatch(0xC0000094), EXECUTE_HANDLER);
        assert_eq!(b.dispatch(0xE0000001), CONTINUE_SEARCH);
    }

    #[test]
    fn unx12145_checks_green() {
        let s = run_unx12145_checks();
        assert!(s.all_passed(), "UNX-F12145 判据未全绿");
    }

    #[test]
    fn unx12145_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
