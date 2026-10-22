// unx2623_invpcid —— UNX-F2623 INVPCID 三类型逐型消费点
// （individual/context-global/all-global）
// 判据原文 UNX-F2623-J1：三类型触发场景逐型登记（消费点清单零遗漏），
// 逐型可观测断言与 F2405 判据一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF800 独占（0xF800~0xF87F）。
//
// 设计（纯功能 no_std）：
//   * 三类型闭集：Type0 individual-address（单地址失效）/ Type1 single-context
//     （单 PCID 全失效）/ Type2 all-context（保留全局项）/ Type3 all-context
//     （全失效）——判据口径“三类型”按 Task 书分组为 0/1/2+3 两合并场景，此处
//     按四编码登记、三消费面归组；
//   * 消费点清单：unmap / mprotect / munmap / fork(COW) / swap-out 五场景逐型映射，
//     清单零遗漏（五场景全覆盖才过）；
//   * 可观测断言：每型失效后 TLB 观测面（残留计数）与期望一致（与 F2405 同口径）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF800 独占） ─────────────────────────────────────────────
pub const XC_INV_TYPE: u32 = 0xF801; // 类型编码越界
pub const XC_INV_SCENE: u32 = 0xF802; // 场景漏登记（清单缺口）
pub const XC_INV_OBSERVE: u32 = 0xF803; // 观测面与期望不符
pub const XC_INV_SEALED: u32 = 0xF804; // 封账后失效
pub const XC_INV_MAPPING: u32 = 0xF805; // 场景→类型映射漂移

/// INVPCID 类型四编码。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InvPcidType {
    Individual = 0,
    SingleCtx = 1,
    AllCtxKeepGlobal = 2,
    AllCtxAll = 3,
}

impl InvPcidType {
    pub fn num(self) -> u8 {
        self as u8
    }
    pub fn name(self) -> &'static str {
        match self {
            InvPcidType::Individual => "INDIVIDUAL",
            InvPcidType::SingleCtx => "SINGLE-CTX",
            InvPcidType::AllCtxKeepGlobal => "ALL-CTX-KEEP-G",
            InvPcidType::AllCtxAll => "ALL-CTX-ALL",
        }
    }
    pub fn from_num(n: u8) -> Option<InvPcidType> {
        match n {
            0 => Some(InvPcidType::Individual),
            1 => Some(InvPcidType::SingleCtx),
            2 => Some(InvPcidType::AllCtxKeepGlobal),
            3 => Some(InvPcidType::AllCtxAll),
            _ => None,
        }
    }
}

/// 五触发场景闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Unmap = 0,
    Mprotect = 1,
    Munmap = 2,
    ForkCow = 3,
    SwapOut = 4,
}

impl Scene {
    pub fn num(self) -> u8 {
        self as u8
    }
    pub fn name(self) -> &'static str {
        match self {
            Scene::Unmap => "UNMAP",
            Scene::Mprotect => "MPROTECT",
            Scene::Munmap => "MUNMAP",
            Scene::ForkCow => "FORK-COW",
            Scene::SwapOut => "SWAP-OUT",
        }
    }
}

/// TLB 观测模型：项 = (pcid, vpn, global)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TlbItem {
    pub pcid: u16,
    pub vpn: u64,
    pub global: bool,
}

/// 消费点登记簿 + 观测器。
pub struct InvPcidBook {
    scene_type: Vec<(Scene, InvPcidType)>,
    tlb: Vec<TlbItem>,
    invalidations: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl InvPcidBook {
    pub fn new() -> InvPcidBook {
        InvPcidBook { scene_type: Vec::new(), tlb: Vec::new(), invalidations: 0, sealed: false, rejects: Vec::new() }
    }

    /// 场景→类型登记（每场景恰一型，重复/越界拒）。
    pub fn register(&mut self, s: Scene, t: InvPcidType) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_INV_SEALED, "封账后登记"));
            return Err(XC_INV_SEALED);
        }
        for &(sc, _) in &self.scene_type {
            if sc == s {
                self.rejects.push((XC_INV_SCENE, "场景重复"));
                return Err(XC_INV_SCENE);
            }
        }
        self.scene_type.push((s, t));
        Ok(())
    }

    /// 消费点清单零遗漏：五场景全登记。
    pub fn coverage(&self) -> bool {
        for s in [Scene::Unmap, Scene::Mprotect, Scene::Munmap, Scene::ForkCow, Scene::SwapOut] {
            let mut hit = false;
            for &(sc, _) in &self.scene_type {
                if sc == s {
                    hit = true;
                }
            }
            if !hit {
                return false;
            }
        }
        true
    }

    /// 类型映射冻结面（标准映射，判据对齐）：
    /// UNMAP→Individual, MPROTECT→SingleCtx, MUNMAP→SingleCtx,
    /// FORK-COW→AllCtxKeepGlobal, SWAP-OUT→AllCtxAll。
    pub fn standard_mapping() -> [(Scene, InvPcidType); 5] {
        [
            (Scene::Unmap, InvPcidType::Individual),
            (Scene::Mprotect, InvPcidType::SingleCtx),
            (Scene::Munmap, InvPcidType::SingleCtx),
            (Scene::ForkCow, InvPcidType::AllCtxKeepGlobal),
            (Scene::SwapOut, InvPcidType::AllCtxAll),
        ]
    }

    pub fn mapping_aligned(&self) -> bool {
        if self.scene_type.len() != 5 {
            return false;
        }
        for &(s, t) in Self::standard_mapping().iter() {
            let mut hit = false;
            for &(sc, tc) in &self.scene_type {
                if sc == s && tc == t {
                    hit = true;
                }
            }
            if !hit {
                return false;
            }
        }
        true
    }

    /// TLB 装载。
    pub fn fill(&mut self, pcid: u16, vpn: u64, global: bool) {
        if self.sealed {
            return;
        }
        self.tlb.push(TlbItem { pcid, vpn, global });
    }

    /// 执行失效（三消费面语义）。
    pub fn invalidate(&mut self, t: InvPcidType, pcid: u16, vpn: u64) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_INV_SEALED, "封账后失效"));
            return Err(XC_INV_SEALED);
        }
        if InvPcidType::from_num(t.num()).is_none() {
            self.rejects.push((XC_INV_TYPE, "类型越界"));
            return Err(XC_INV_TYPE);
        }
        let before = self.tlb.len();
        match t {
            InvPcidType::Individual => {
                self.tlb.retain(|e| !(e.pcid == pcid && e.vpn == vpn));
            }
            InvPcidType::SingleCtx => {
                self.tlb.retain(|e| e.pcid != pcid);
            }
            InvPcidType::AllCtxKeepGlobal => {
                self.tlb.retain(|e| e.global);
            }
            InvPcidType::AllCtxAll => {
                self.tlb.clear();
            }
        }
        self.invalidations += 1;
        Ok(before - self.tlb.len())
    }

    /// 观测断言：失效后残留计数 == 期望。
    pub fn observe_expect(&self, expect: usize) -> Result<(), u32> {
        if self.tlb.len() != expect {
            return Err(XC_INV_OBSERVE);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_INV_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for &(s, t) in &self.scene_type {
            h ^= (s.num() as u32) ^ (t.num() as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        for e in &self.tlb {
            h ^= e.pcid as u32 ^ (e.vpn as u32).wrapping_mul(0x85EB) ^ (e.global as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.invalidations;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | self.invalidations as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn tlb_len(&self) -> usize {
        self.tlb.len()
    }
}

/// 标准夹具：TLB 载 6 项（1 global + 5 非全局），标准映射全登记。
pub fn standard_book() -> InvPcidBook {
    let mut b = InvPcidBook::new();
    for &(s, t) in InvPcidBook::standard_mapping().iter() {
        let _ = b.register(s, t);
    }
    b.fill(1, 0x1000, false);
    b.fill(1, 0x2000, false);
    b.fill(2, 0x1000, false);
    b.fill(2, 0x3000, false);
    b.fill(3, 0x4000, false);
    b.fill(9, 0x5000, true); // 全局项
    b
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）类型编码越界 10/10
    {
        let mut b = InvPcidBook::new();
        let mut hits = 0;
        for n in 4..14u8 {
            if InvPcidType::from_num(n).is_none() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）场景重复必拒
    {
        let mut b = InvPcidBook::new();
        let _ = b.register(Scene::Unmap, InvPcidType::Individual);
        if b.register(Scene::Unmap, InvPcidType::SingleCtx) == Err(XC_INV_SCENE) {
            caught += 1;
        }
    }
    // 3）清单缺口必抓
    {
        let mut b = InvPcidBook::new();
        let _ = b.register(Scene::Unmap, InvPcidType::Individual); // 只 1/5
        if !b.coverage() {
            caught += 1;
        }
    }
    // 4）观测不符必抓
    {
        let mut b = standard_book();
        let _ = b.invalidate(InvPcidType::Individual, 1, 0x1000); // 失效 1 项
        if b.observe_expect(4) == Err(XC_INV_OBSERVE) {
            caught += 1;
        }
    }
    // 5）封账后失效必拒
    {
        let mut b = standard_book();
        let _ = b.seal();
        if b.invalidate(InvPcidType::AllCtxAll, 0, 0) == Err(XC_INV_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2623-J1。
pub fn run_unx2623_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2623");

    // C23-01 类型四编码闭集
    {
        let ok = InvPcidType::from_num(0).is_some()
            && InvPcidType::from_num(3).is_some()
            && InvPcidType::from_num(4).is_none()
            && InvPcidType::Individual.name() == "INDIVIDUAL";
        s.add("C23-01-类型闭集", ok, if ok { "" } else { "类型表漂移" });
    }
    // C23-02 五场景清单零遗漏
    {
        let b = standard_book();
        s.add("C23-02-清单零遗漏", b.coverage(), if b.coverage() { "" } else { "清单缺口" });
    }
    // C23-03 标准映射对齐
    {
        let b = standard_book();
        s.add(
            "C23-03-映射对齐",
            b.mapping_aligned(),
            if b.mapping_aligned() { "" } else { "映射漂移" },
        );
    }
    // C23-04 Type0 individual：单地址恰失效 1 项
    {
        let mut b = standard_book();
        let n = b.invalidate(InvPcidType::Individual, 1, 0x1000);
        let ok = n == Ok(1) && b.observe_expect(5).is_ok();
        s.add("C23-04-individual单项", ok, if ok { "" } else { "单项破" });
    }
    // C23-05 Type1 single-ctx：同 PCID 全失效（global 保留）
    {
        let mut b = standard_book();
        let n = b.invalidate(InvPcidType::SingleCtx, 1, 0);
        let ok = n == Ok(2) && b.observe_expect(4).is_ok();
        s.add("C23-05-singlectx", ok, if ok { "" } else { "单上下文破" });
    }
    // C23-06 Type2 all-ctx keep global：只留全局项
    {
        let mut b = standard_book();
        let n = b.invalidate(InvPcidType::AllCtxKeepGlobal, 0, 0);
        let ok = n == Ok(5) && b.observe_expect(1).is_ok();
        s.add("C23-06-keepglobal", ok, if ok { "" } else { "保留语义破" });
    }
    // C23-07 Type3 all-ctx all：全空
    {
        let mut b = standard_book();
        let n = b.invalidate(InvPcidType::AllCtxAll, 0, 0);
        let ok = n == Ok(6) && b.observe_expect(0).is_ok();
        s.add("C23-07-all全失效", ok, if ok { "" } else { "全失效破" });
    }
    // C23-08 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C23-08-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C23-09 场景名冻结
    {
        let ok = Scene::Unmap.name() == "UNMAP"
            && Scene::SwapOut.name() == "SWAP-OUT"
            && Scene::ForkCow.num() == 3;
        s.add("C23-09-场景名冻结", ok, if ok { "" } else { "场景表漂移" });
    }
    // C23-10 指纹冻结 + 封账稳定
    {
        let f = || standard_book().fingerprint();
        let mut g = standard_book();
        let pre = g.fingerprint();
        let _ = g.seal();
        let ok = f() == f() && g.fingerprint() == pre && g.seal().is_err();
        s.add("C23-10-指纹与封账", ok, if ok { "" } else { "漂移" });
    }
    s
}
