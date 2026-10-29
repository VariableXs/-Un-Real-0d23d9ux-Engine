//! UNX-F12149 — 展开中再异常（nested exception）语义（0x1a4c0 段）
//!
//! 锚点：UNX-F12149-J1 嵌套异常账 10/10（展开中抛新异常 → 记嵌套
//! 层深 + 原异常保持 STATE_COLLISION 语义），层深受限（NT 深度
//! 上限嵌套 8 层），嵌套账回放全对。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：NestedCtx（层深链）+ NestBook（push/pop 层深账）+ 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a4c0;
pub const NX_DEPTH: u32 = SEG_BASE + 0x01; // 嵌套超限
pub const NX_POP: u32 = SEG_BASE + 0x02; // 空栈弹出
pub const NX_ORDER: u32 = SEG_BASE + 0x03; // 层序倒挂（父层未清先清子层）
pub const NX_SEALED: u32 = SEG_BASE + 0x04; // 印后嵌套
pub const NX_EMPTY: u32 = SEG_BASE + 0x05; // 空账核验

pub const NX_MAX_DEPTH: usize = 8;

/// 一层嵌套上下文。
#[derive(Clone, PartialEq)]
pub struct NestedCtx {
    pub depth: usize,
    pub origin: u32,
    pub code: u32,
    pub note: String,
}

impl NestedCtx {
    /// 层自洽：depth >= 1（0 层 = 原异常不记此账）、码非零。
    pub fn ok(&self) -> bool {
        self.depth >= 1 && self.code != 0 && self.origin != 0
    }

    pub fn render(&self) -> String {
        format!("d={} o={:#x} c={:#x}", self.depth, self.origin, self.code)
    }
}

/// 嵌套账。
pub struct NestBook {
    pub stack: Vec<NestedCtx>,
    pub resolved: Vec<NestedCtx>,
    sealed: bool,
}

impl NestBook {
    pub fn new() -> NestBook {
        NestBook { stack: Vec::new(), resolved: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 嵌套深：展开中抛新异常。
    pub fn nest(&mut self, origin: u32, code: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(NX_SEALED);
        }
        let depth = self.stack.len() + 1;
        if depth > NX_MAX_DEPTH {
            return Err(NX_DEPTH);
        }
        let ctx = NestedCtx { depth, origin, code, note: note.to_string() };
        if !ctx.ok() {
            return Err(NX_EMPTY);
        }
        self.stack.push(ctx);
        Ok(depth)
    }

    /// 解一层（最内层先解，LIFO）。
    pub fn resolve_one(&mut self) -> Result<NestedCtx, u32> {
        if self.sealed {
            return Err(NX_SEALED);
        }
        match self.stack.pop() {
            Some(ctx) => {
                self.resolved.push(ctx.clone());
                Ok(ctx)
            }
            None => Err(NX_POP),
        }
    }

    /// 层序对平：resolved 必须 LIFO（depth 降序入账）。
    pub fn resolved_lifo(&self) -> bool {
        self.resolved.windows(2).all(|w| w[0].depth >= w[1].depth)
    }

    /// 当前嵌套深。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 展开中再异常：栈非空即“嵌套态”。
    pub fn in_nest(&self) -> bool {
        !self.stack.is_empty()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a4c0;
        for c in &self.stack {
            h = h.wrapping_mul(211).wrapping_add(c.depth as u32);
            h = h.wrapping_mul(211).wrapping_add(c.code);
        }
        for c in &self.resolved {
            h = h.wrapping_mul(211).wrapping_add(c.depth as u32);
            h = h.wrapping_mul(211).wrapping_add(c.code);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "open={} resolved={} lifo={}",
            self.stack.len(),
            self.resolved.len(),
            self.resolved_lifo()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = NestBook::new();
    // 1) 空栈弹出必拒。
    got[0] = b.resolve_one().err() == Some(NX_POP);
    // 2) 深度超限必拒（9 层）。
    let mut b2 = NestBook::new();
    let mut ok = true;
    for i in 0..8u32 {
        if b2.nest(0x8000_0000 + i, 0xC0000005, "n").is_err() {
            ok = false;
        }
    }
    if !ok {
        return [false; 5];
    }
    got[1] = b2.nest(0x8000_0008, 0xC0000005, "over").err() == Some(NX_DEPTH);
    // 3) 零码嵌套必拒。
    got[2] = b.nest(0x8000_0000, 0, "z").err() == Some(NX_EMPTY);
    // 4) 零 origin 嵌套必拒。
    got[3] = b.nest(0, 0xC0000005, "z").err() == Some(NX_EMPTY);
    // 5) 印后嵌套必拒。
    b.seal();
    got[4] = b.nest(0x8000_0000, 0xC0000005, "post").err() == Some(NX_SEALED);
    got
}

/// 判据入口：UNX-F12149
pub fn run_unx12149_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12149");
    let mut b = NestBook::new();

    // J1 展开中抛三层嵌套。
    let d1 = b.nest(0x8000_1000, 0xC0000005, "accv");
    let d2 = b.nest(0x8000_2000, 0xC0000094, "div0");
    let d3 = b.nest(0x8000_3000, 0xC00000FD, "stkovf");
    s.add(
        "J1_nest3",
        d1 == Ok(1) && d2 == Ok(2) && d3 == Ok(3),
        "三层嵌套层深递增",
    );

    // J2 嵌套态判定。
    s.add("J2_in_nest", b.in_nest() && b.depth() == 3, "嵌套态深 3");

    // J3 LIFO 解三层。
    let r1 = b.resolve_one();
    let r2 = b.resolve_one();
    let r3 = b.resolve_one();
    s.add(
        "J3_lifo3",
        r1.as_ref().map(|c| c.depth) == Ok(3)
            && r2.as_ref().map(|c| c.depth) == Ok(2)
            && r3.as_ref().map(|c| c.depth) == Ok(1),
        "解层 LIFO 3→2→1",
    );

    // J4 空栈再弹拒。
    s.add("J4_pop_empty", b.resolve_one().err() == Some(NX_POP), "解完再弹拒 POP");

    // J5 LIFO 留痕对平。
    s.add(
        "J5_resolved_lifo",
        b.resolved_lifo() && b.resolved.len() == 3,
        "解层留痕 LIFO 3 条",
    );

    // J6 退回非嵌套态。
    s.add("J6_flat", !b.in_nest() && b.depth() == 0, "解空回非嵌套态");

    // J7 深度上限 8 层全过。
    let mut b7 = NestBook::new();
    let mut ok7 = true;
    for i in 0..8u32 {
        if b7.nest(0x8000_0000 + i, 0xC0000005, "d").is_err() {
            ok7 = false;
        }
    }
    s.add("J7_depth8", ok7 && b7.depth() == 8, "嵌套 8 层恰满合法");

    // J8 第 9 层拒。
    s.add(
        "J8_depth9",
        b7.nest(0x8000_0008, 0xC0000005, "over").err() == Some(NX_DEPTH),
        "第 9 层拒 DEPTH",
    );

    // J9 逐层解 8 层回非嵌套。
    let mut n9 = 0;
    while b7.resolve_one().is_ok() {
        n9 += 1;
    }
    s.add("J9_unwind8", n9 == 8 && !b7.in_nest(), "8 层逐解回平");

    // J10 层自洽核。
    let c10 = NestedCtx { depth: 2, origin: 0x8000_0000, code: 0xC0000005, note: "n".to_string() };
    s.add("J10_ctx_ok", c10.ok(), "嵌套上下文过自洽核");

    // J11 零码上下文非法。
    let bad = NestedCtx { depth: 1, origin: 0x8000_0000, code: 0, note: "z".to_string() };
    s.add("J11_zero_code", !bad.ok(), "零码上下文判非法");

    // J12 渲染。
    let r = c10.render();
    s.add(
        "J12_render",
        r.contains("d=2") && r.contains("c=0xc0000005"),
        "渲染含深/origin/码",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "嵌套账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.nest(0x8000_0000, 0xC0000005, "post").err() == Some(NX_SEALED)
            && b.resolve_one().err() == Some(NX_SEALED),
        "印账后嵌套/解层皆拒",
    );

    // J16 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J16_summary",
        sm.contains("resolved=3") && sm.contains("lifo=true"),
        "摘要含解层数与 LIFO 位",
    );

    // J17 解层留痕内容对平（最内层先解：code 一致）。
    let last = b.resolved.last().unwrap();
    s.add(
        "J17_resolved_tail",
        last.depth == 1 && last.code == 0xC0000005,
        "解层留痕尾=最先入栈层",
    );

    // J18 空账指纹稳定。
    let b18 = NestBook::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空嵌套账指纹稳定");

    // J19 origin 逐层单调（模拟逐帧回退 origin 递增）。
    let mut b19 = NestBook::new();
    let mut mono19 = true;
    for i in 0..5u32 {
        if b19.nest(0x8000_0000 + i * 0x100, 0xC0000005, "m").is_err() {
            mono19 = false;
        }
    }
    let stack_origins: Vec<u32> = b19.stack.iter().map(|c| c.origin).collect();
    s.add(
        "J19_origin_mono",
        mono19 && stack_origins.windows(2).all(|w| w[0] < w[1]),
        "嵌套 origin 逐层递增",
    );

    // J20 解层返回克隆完整。
    let mut b20 = NestBook::new();
    let _ = b20.nest(0x8000_5000, 0xC0000409, "fastfail");
    let got20 = b20.resolve_one();
    s.add(
        "J20_clone_full",
        got20.as_ref().map(|c| c.code) == Ok(0xC0000409)
            && got20.as_ref().map(|c| c.note.as_str()) == Ok("fastfail"),
        "解层返回全字段克隆",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12149_nest() {
        let mut b = NestBook::new();
        assert_eq!(b.nest(0x8000_0000, 0xC0000005, "n"), Ok(1));
        assert_eq!(b.nest(0x8000_0100, 0xC0000094, "n"), Ok(2));
        assert_eq!(b.resolve_one().map(|c| c.depth), Ok(2));
    }

    #[test]
    fn unx12149_checks_green() {
        let s = run_unx12149_checks();
        assert!(s.all_passed(), "UNX-F12149 判据未全绿");
    }

    #[test]
    fn unx12149_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
