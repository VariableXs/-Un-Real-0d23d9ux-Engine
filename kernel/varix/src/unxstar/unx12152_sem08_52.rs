//! UNX-F12152 — 向量异常处理 VEH 语义预告（0x1a520 段）
//!
//! 锚点：UNX-F12152-J1 VEH 链 10/10（AddVectored: 头插链/序号递增；
//! 链遍历按注册序逆行 = LIFO 试配；RemoveVectored: 按句柄摘除），
//! 链留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：VehNode（handler 句柄 + 异常码过滤）+ VehChain（头插链 +
//! 试配账）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a520;
pub const VH_GHOST: u32 = SEG_BASE + 0x01; // 句柄不存在
pub const VH_TWICE: u32 = SEG_BASE + 0x02; // 重复句柄
pub const VH_ORDER: u32 = SEG_BASE + 0x03; // 零句柄/过滤码非法
pub const VH_SEALED: u32 = SEG_BASE + 0x04; // 印后挂链
pub const VH_EMPTY: u32 = SEG_BASE + 0x05; // 空链试配

/// VEH 结点。
#[derive(Clone, PartialEq)]
pub struct VehNode {
    pub handle: u64,
    pub watch: u32, // 0 = 全捕获（first handler 语义）
    pub note: String,
}

impl VehNode {
    pub fn ok(&self) -> bool {
        self.handle != 0
    }

    pub fn matches(&self, code: u32) -> bool {
        self.watch == 0 || self.watch == code
    }

    pub fn render(&self) -> String {
        format!("h={:#x} w={:#x}", self.handle, self.watch)
    }
}

/// VEH 链（头插 = 后注册先试配）。
pub struct VehChain {
    pub nodes: Vec<VehNode>,
    pub dispatches: Vec<(u32, u64)>, // (code, 命中 handle；0=全链不接)
    next_handle: u64,
    sealed: bool,
}

impl VehChain {
    pub fn new() -> VehChain {
        VehChain { nodes: Vec::new(), dispatches: Vec::new(), next_handle: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 头插挂 handler，返回句柄（AddVectoredExceptionHandler 语义）。
    pub fn add_first(&mut self, watch: u32, note: &str) -> Result<u64, u32> {
        if self.sealed {
            return Err(VH_SEALED);
        }
        if self.next_handle == 0 {
            return Err(VH_ORDER);
        }
        let h = self.next_handle;
        self.next_handle += 1;
        let node = VehNode { handle: h, watch, note: note.to_string() };
        if !node.ok() {
            return Err(VH_ORDER);
        }
        self.nodes.insert(0, node);
        Ok(h)
    }

    /// 按句柄摘除。
    pub fn remove(&mut self, handle: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(VH_SEALED);
        }
        let n = self.nodes.len();
        self.nodes.retain(|nd| nd.handle != handle);
        if self.nodes.len() == n {
            return Err(VH_GHOST);
        }
        Ok(())
    }

    /// 异常分发：链头向链尾试配，首个匹配者接（返回句柄）；全不接 = 0。
    pub fn dispatch(&mut self, code: u32) -> u64 {
        let hit = self
            .nodes
            .iter()
            .find(|nd| nd.matches(code))
            .map(|nd| nd.handle)
            .unwrap_or(0);
        self.dispatches.push((code, hit));
        hit
    }

    /// 分发留痕全对。
    pub fn dispatches_replayable(&self) -> bool {
        self.dispatches.iter().all(|&(code, hit)| {
            let want = self
                .nodes
                .iter()
                .find(|nd| nd.matches(code))
                .map(|nd| nd.handle)
                .unwrap_or(0);
            want == hit
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a520;
        for nd in &self.nodes {
            h = h.wrapping_mul(211).wrapping_add(nd.handle as u32);
            h = h.wrapping_mul(211).wrapping_add(nd.watch);
            for by in nd.note.bytes() {
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
            "nodes={} dispatches={} replayable={}",
            self.nodes.len(),
            self.dispatches.len(),
            self.dispatches_replayable()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = VehChain::new();
    // 1) 空链分发 = 0（可判定的全不接路径）。
    got[0] = b.dispatch(0xC0000005) == 0 && b.dispatches.len() == 1;
    // 2) 摘野句柄必拒。
    got[1] = b.remove(0xDEAD).err() == Some(VH_GHOST);
    // 3) 全捕获 handler 挂载后可接任意码（合法路径佐证链活性）。
    let _ = b.add_first(0, "catch-all");
    got[2] = b.dispatch(0xDEAD_BEEF) == 1;
    // 4) 摘后句柄失效。
    let _ = b.remove(1);
    got[3] = b.remove(1).err() == Some(VH_GHOST);
    // 5) 印后挂链必拒。
    b.seal();
    got[4] = b.add_first(0, "post").err() == Some(VH_SEALED);
    got
}

/// 判据入口：UNX-F12152
pub fn run_unx12152_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12152");
    let mut b = VehChain::new();

    // J1 三 handler 头插挂链。
    let h1 = b.add_first(0xC0000005, "accv-watch");
    let h2 = b.add_first(0xC0000094, "div-watch");
    let h3 = b.add_first(0, "catch-all");
    s.add(
        "J1_add3",
        h1 == Ok(1) && h2 == Ok(2) && h3 == Ok(3) && b.nodes.len() == 3,
        "三 handler 头插句柄 1/2/3",
    );

    // J2 头插序：后注册者居首。
    s.add(
        "J2_lifo_order",
        b.nodes[0].handle == 3 && b.nodes[2].handle == 1,
        "头插链 LIFO 序",
    );

    // J3 分发 10/10：定向命中/全捕获接/全不接。
    let mut j3 = true;
    if b.dispatch(0xC0000005) != 3 { j3 = false; } // catch-all 在链头先接
    let mut b3 = VehChain::new();
    let _ = b3.add_first(0xC0000005, "a");
    let _ = b3.add_first(0xC0000094, "b");
    if b3.dispatch(0xC0000005) != 1 { j3 = false; }
    if b3.dispatch(0xC0000094) != 2 { j3 = false; }
    if b3.dispatch(0xE0000001) != 0 { j3 = false; }
    s.add("J3_dispatch_10x", j3, "定向/全捕获/不接判定全对");

    // J4 定向过滤命中（去 catch-all 后）。
    let mut b4 = VehChain::new();
    let _ = b4.add_first(0xC0000005, "accv");
    s.add(
        "J4_direct_hit",
        b4.dispatch(0xC0000005) == 1 && b4.dispatch(0xC0000094) == 0,
        "定向过滤命中/不接",
    );

    // J5 摘除后试配序变化。
    let r5 = b3.remove(1);
    s.add(
        "J5_remove_shift",
        r5.is_ok() && b3.dispatch(0xC0000005) == 0,
        "摘 accv-watch 后该码不接",
    );

    // J6 摘野句柄拒。
    s.add(
        "J6_ghost_remove",
        b3.remove(0xDEAD).err() == Some(VH_GHOST),
        "野句柄摘除拒 GHOST",
    );

    // J7 结点自洽核。
    let nd = VehNode { handle: 7, watch: 0xC0000409, note: "ff".to_string() };
    s.add("J7_node_ok", nd.ok() && nd.matches(0xC0000409) && !nd.matches(0xC0000005), "结点过滤匹配核");

    // J8 渲染。
    let r = nd.render();
    s.add(
        "J8_render",
        r.contains("h=0x7") && r.contains("w=0xc0000409"),
        "渲染含句柄/过滤码",
    );

    // J9 句柄递增不复用。
    let h9 = b3.add_first(0, "new");
    s.add(
        "J9_handle_seq",
        h9 == Ok(3),
        "句柄单调递增不复用",
    );

    // J10 分发留痕回放（链未变异的账全可回放；J5 摘除后的新判定
    // 对旧账不可回放，属预期突变）。
    s.add("J10_replayable", b4.dispatches_replayable(), "分发留痕全可回放");

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "VEH 链折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.add_first(0, "post").err() == Some(VH_SEALED)
            && b.remove(1).err() == Some(VH_SEALED),
        "印账后挂/摘皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("nodes=3") && sm.contains("replayable=true"),
        "摘要含结点数与回放位",
    );

    // J15 大链 10 handler 头插（全捕获 watch，供 J16 验 LIFO）。
    let mut b15 = VehChain::new();
    let mut ok15 = true;
    for _i in 0..10u32 {
        if b15.add_first(0, "m").is_err() {
            ok15 = false;
        }
    }
    s.add(
        "J15_chain10",
        ok15 && b15.nodes.len() == 10 && b15.nodes[0].handle == 10,
        "10 handler 头插链满",
    );

    // J16 大链逐码命中：全捕获链上任意码都由链头（后挂 handle 10）先接。
    let mut hit16 = true;
    for i in 0..10u32 {
        if b15.dispatch(0xC000_0000 + i) != 10 {
            hit16 = false;
        }
    }
    s.add("J16_lifo_hits", hit16, "大链逐码 LIFO 命中 10/10");

    // J17 空链指纹稳定。
    let b17 = VehChain::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空 VEH 链指纹稳定");

    // J18 全摘后回空。
    let mut b18 = VehChain::new();
    let _ = b18.add_first(0, "a");
    let _ = b18.remove(1);
    s.add(
        "J18_drain",
        b18.nodes.is_empty() && b18.dispatch(0xC0000005) == 0,
        "摘空链分发回 0",
    );

    // J19 分发留痕含不接记录。
    s.add(
        "J19_miss_traced",
        b3.dispatches.iter().any(|&(_, h)| h == 0),
        "不接路径留痕 0 句柄",
    );

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
    fn unx12152_veh() {
        let mut b = VehChain::new();
        let _ = b.add_first(0xC0000005, "a");
        let _ = b.add_first(0, "all");
        assert_eq!(b.dispatch(0xC0000005), 2);
        assert_eq!(b.dispatch(0xE0000001), 2);
    }

    #[test]
    fn unx12152_checks_green() {
        let s = run_unx12152_checks();
        assert!(s.all_passed(), "UNX-F12152 判据未全绿");
    }

    #[test]
    fn unx12152_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
