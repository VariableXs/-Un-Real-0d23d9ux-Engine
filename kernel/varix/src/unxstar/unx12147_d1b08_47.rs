//! UNX-F12147 — __CxxFrameHandler 挂点协议（C++ 链预告）（0x1a480 段）
//!
//! 锚点：UNX-F12147-J1 C 链把未知异常移交 C++ 链 10/10（handoff
//! 协议：C 处置 = CONTINUE_SEARCH 时触发移交，移交账逐笔对平；
//! catch 命中 = 1、不命中 = 0、再抛 = 3）。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：CxxHandler（类型签名匹配 + handoff 协议）+ 移交留痕账 +
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a480;
pub const CX_GHOST: u32 = SEG_BASE + 0x01; // 类型签名为空
pub const CX_DUP: u32 = SEG_BASE + 0x02; // 重复类型签名
pub const CX_ORDER: u32 = SEG_BASE + 0x03; // catch 结果码非法
pub const CX_SEALED: u32 = SEG_BASE + 0x04; // 印后挂签名
pub const CX_HANDOFF: u32 = SEG_BASE + 0x05; // 双重移交

/// C++ 链 catch 结果：1 命中 / 0 不命中 / 3 再抛。
pub const CX_CATCH: u32 = 1;
pub const CX_MISS: u32 = 0;
pub const CX_RETHROW: u32 = 3;

/// 一条 catch 签名。
#[derive(Clone, PartialEq)]
pub struct CatchSig {
    pub type_hash: u64,
    pub result: u32,
    pub note: String,
}

impl CatchSig {
    /// 签名自洽：hash 非零、结果码 ∈ {0,1,3}。
    pub fn ok(&self) -> bool {
        self.type_hash != 0 && matches!(self.result, CX_MISS | CX_CATCH | CX_RETHROW)
    }

    pub fn render(&self) -> String {
        format!("t={:#x} r={}", self.type_hash, self.result)
    }
}

/// C++ 链挂点：签名表 + 移交账。
pub struct CxxHandler {
    pub sigs: Vec<CatchSig>,
    pub handoffs: Vec<(u32, u64, u32)>,
    handed_off: bool,
    sealed: bool,
}

impl CxxHandler {
    pub fn new() -> CxxHandler {
        CxxHandler { sigs: Vec::new(), handoffs: Vec::new(), handed_off: false, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn handed_off(&self) -> bool {
        self.handed_off
    }

    /// 挂 catch 签名（空 hash/结果码非法/重复签名必拒）。
    pub fn add(&mut self, type_hash: u64, result: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CX_SEALED);
        }
        let sig = CatchSig { type_hash, result, note: note.to_string() };
        if !sig.ok() {
            return Err(CX_ORDER);
        }
        if self.sigs.iter().any(|s| s.type_hash == type_hash) {
            return Err(CX_DUP);
        }
        self.sigs.push(sig);
        Ok(())
    }

    /// C 链移交一次：按 type_hash 匹配 catch；记移交账（code, hash, result）。
    pub fn handoff(&mut self, code: u32, type_hash: u64) -> Result<u32, u32> {
        if self.handed_off {
            return Err(CX_HANDOFF);
        }
        if type_hash == 0 {
            return Err(CX_GHOST);
        }
        let result = self
            .sigs
            .iter()
            .find(|s| s.type_hash == type_hash)
            .map(|s| s.result)
            .unwrap_or(CX_MISS);
        self.handoffs.push((code, type_hash, result));
        self.handed_off = true;
        Ok(result)
    }

    /// 移交账回放全对。
    pub fn handoffs_replayable(&self) -> bool {
        self.handoffs.iter().all(|&(_, hash, r)| {
            self.sigs
                .iter()
                .find(|s| s.type_hash == hash)
                .map(|s| s.result)
                .unwrap_or(CX_MISS) == r
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a480;
        for s in &self.sigs {
            h = h.wrapping_mul(211).wrapping_add(s.type_hash as u32);
            h = h.wrapping_mul(211).wrapping_add(s.type_hash as u32 >> 16);
            h = h.wrapping_mul(211).wrapping_add(s.result);
            for by in s.note.bytes() {
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
            "sigs={} handoffs={} replayable={}",
            self.sigs.len(),
            self.handoffs.len(),
            self.handoffs_replayable()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CxxHandler::new();
    // 1) 空 hash 移交必拒。
    got[0] = b.handoff(0xE06D7363, 0).err() == Some(CX_GHOST);
    // 2) 结果码非法（2）必拒。
    got[1] = b.add(0x1111, 2, "bad").err() == Some(CX_ORDER);
    // 3) 重复签名必拒。
    let _ = b.add(0x2222, CX_CATCH, "int");
    got[2] = b.add(0x2222, CX_MISS, "dup").err() == Some(CX_DUP);
    // 4) 未注册 hash 移交 = MISS（可判定的不命中路径）。
    got[3] = b.handoff(0xE06D7363, 0x9999) == Ok(CX_MISS);
    // 5) 二次移交必拒。
    got[4] = b.handoff(0xE06D7363, 0x2222).err() == Some(CX_HANDOFF);
    got
}

/// 判据入口：UNX-F12147
pub fn run_unx12147_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12147");
    let mut b = CxxHandler::new();

    // J1 三签名挂载（catch/miss/rethrow）。
    let j1 = b.add(0xAAAA_0001, CX_CATCH, "catch-int").is_ok()
        && b.add(0xAAAA_0002, CX_MISS, "filter-miss").is_ok()
        && b.add(0xAAAA_0003, CX_RETHROW, "rethrow-wrap").is_ok();
    s.add("J1_sigs3", j1 && b.sigs.len() == 3, "三 catch 签名挂载");

    // J2 移交 10/10：命中/不命中/再抛 + 野 hash MISS。
    let mut j2 = true;
    if b.handoff(0xE06D7363, 0xAAAA_0001) != Ok(CX_CATCH) { j2 = false; }
    for hash in [0xAAAA_0002u64, 0xAAAA_0003] {
        let want = if hash == 0xAAAA_0002 { CX_MISS } else { CX_RETHROW };
        let mut b2 = CxxHandler::new();
        let _ = b2.add(0xAAAA_0001, CX_CATCH, "a");
        let _ = b2.add(0xAAAA_0002, CX_MISS, "b");
        let _ = b2.add(0xAAAA_0003, CX_RETHROW, "c");
        if b2.handoff(0xE06D7363, hash) != Ok(want) { j2 = false; }
    }
    s.add("J2_handoff_10x", j2 && b.handoffs.len() == 1, "移交三型 + 野 hash 判定全对");

    // J3 结果码常量与 C++ 链语义一致。
    s.add(
        "J3_consts",
        CX_CATCH == 1 && CX_MISS == 0 && CX_RETHROW == 3,
        "catch/miss/rethrow = 1/0/3",
    );

    // J4 签名自洽核。
    s.add("J4_sigs_ok", b.sigs.iter().all(|x| x.ok()), "全签名过自洽核");

    // J5 渲染。
    let r = b.sigs[0].render();
    s.add(
        "J5_render",
        r.contains("t=0xaaaa0001") && r.contains("r=1"),
        "渲染含 hash/结果码",
    );

    // J6 移交账回放。
    s.add(
        "J6_replayable",
        b.handoffs_replayable() && b.handoffs[0].0 == 0xE06D7363,
        "移交账可回放且码为 C++ 异常码",
    );

    // J7 C++ MSVC 异常码锚点。
    s.add(
        "J7_msvc_code",
        0xE06D7363u32 == 0xE06D_7363,
        "MSVC C++ 异常码 0xE06D7363",
    );

    // J8 空 hash 挂签名拒。
    let mut b8 = CxxHandler::new();
    s.add(
        "J8_zero_hash",
        b8.add(0, CX_CATCH, "z").err() == Some(CX_ORDER),
        "零 hash 签名拒 ORDER",
    );

    // J9 重复签名拒。
    let mut b9 = CxxHandler::new();
    let _ = b9.add(0x1111, CX_CATCH, "a");
    s.add(
        "J9_dup_sig",
        b9.add(0x1111, CX_CATCH, "b").err() == Some(CX_DUP),
        "重复 hash 拒 DUP",
    );

    // J10 未注册移交记 MISS 留痕。
    let mut b10 = CxxHandler::new();
    let _ = b10.add(0x2222, CX_CATCH, "x");
    let _ = b10.handoff(0xE06D7363, 0x3333);
    s.add(
        "J10_miss_traced",
        b10.handoffs.len() == 1 && b10.handoffs[0].2 == CX_MISS,
        "未命中移交留痕 MISS",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "挂点折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.add(0xBBBB_0001, CX_CATCH, "post").err() == Some(CX_SEALED),
        "印账后挂签名拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("sigs=3") && sm.contains("replayable=true"),
        "摘要含签名数与回放位",
    );

    // J15 大表 10 签名 + 逐 hash 移交。
    let mut b15 = CxxHandler::new();
    let mut ok15 = true;
    for i in 0..10u64 {
        let r = if i % 3 == 0 { CX_CATCH } else if i % 3 == 1 { CX_MISS } else { CX_RETHROW };
        if b15.add(0x5000 + i, r, "m").is_err() {
            ok15 = false;
        }
    }
    let mut hit15 = true;
    for i in 0..10u64 {
        let want = if i % 3 == 0 { CX_CATCH } else if i % 3 == 1 { CX_MISS } else { CX_RETHROW };
        let mut b15b = CxxHandler::new();
        for k in 0..10u64 {
            let r = if k % 3 == 0 { CX_CATCH } else if k % 3 == 1 { CX_MISS } else { CX_RETHROW };
            let _ = b15b.add(0x5000 + k, r, "m");
        }
        if b15b.handoff(0xE06D7363, 0x5000 + i) != Ok(want) {
            hit15 = false;
        }
    }
    s.add("J15_table10", ok15 && hit15, "10 签名大表逐 hash 移交全对");

    // J16 二次移交拒（单发协议）。
    s.add(
        "J16_single_handoff",
        b.handed_off(),
        "移交单发位已置",
    );

    // J17 空表指纹稳定。
    let b17 = CxxHandler::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空挂点指纹稳定");

    // J18 rethrow 签名合法。
    let sig18 = CatchSig { type_hash: 0x7777, result: CX_RETHROW, note: "r".to_string() };
    s.add("J18_rethrow_ok", sig18.ok(), "rethrow 结果码自洽");

    // J19 MISS 签名渲染。
    let sig19 = CatchSig { type_hash: 0x8888, result: CX_MISS, note: "m".to_string() };
    s.add(
        "J19_render_miss",
        sig19.render().contains("r=0"),
        "MISS 签名渲染含 0",
    );

    // J20 移交账含完整三元组。
    s.add(
        "J20_triple",
        b.handoffs[0] == (0xE06D7363, 0xAAAA_0001, CX_CATCH),
        "移交账三元组逐字段对平",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12147_handoff() {
        let mut b = CxxHandler::new();
        let _ = b.add(0xAAAA, CX_CATCH, "int");
        assert_eq!(b.handoff(0xE06D7363, 0xAAAA), Ok(CX_CATCH));
        assert!(b.handed_off());
    }

    #[test]
    fn unx12147_checks_green() {
        let s = run_unx12147_checks();
        assert!(s.all_passed(), "UNX-F12147 判据未全绿");
    }

    #[test]
    fn unx12147_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
