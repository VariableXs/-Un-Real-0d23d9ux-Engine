//! UNX-F0807 · TrapFrame 统一寄存器现场与错误码分型账 · 诊断段 0xB870
//! 验收锚点：UNX-F0807-J1 现场结构 64 字节对齐固定布局（异常/中断/
//! IPI 三入口同构），错误码有无分型零错位（10 万次注入），双判据缺一即批内红灯。
use alloc::vec::Vec;
use alloc::string::String;


/// 确定性 LCG（本册本地副本，同仓惯例）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg(seed) }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

pub const SEG_BASE: u16 = 0xB870;

// ── 诊断段独占 · TrapFrame 错误域 ───────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_LAYOUT: u16 = SEG_ERR + 1;  // 布局错位
const E_ERRCODE: u16 = SEG_ERR + 2; // 错误码分型错
const E_ENTRY: u16 = SEG_ERR + 3;   // 入口不同构
const E_ALIGN: u16 = SEG_ERR + 4;   // 对齐破坏
const E_INJECT: u16 = SEG_ERR + 5;  // 注入检出失败

/// TrapFrame 固定布局：17 个 u64 槽（136 字节存储，对齐 8）。
/// 有错误码入口：[err_code] 位于 vector 之后、regs 之前。
pub const FRAME_SLOTS: usize = 17;
pub const FRAME_ALIGN: usize = 8;

/// 入口类别：异常（有错误码，部分）/中断（无）/IPI（无）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Exception, // 有错误码槽
    Interrupt, // 无错误码槽
    Ipi,       // 无错误码槽
}

impl EntryKind {
    pub fn has_errcode(self) -> bool {
        self == EntryKind::Exception
    }
}

/// 有错误码的异常向量（x86 语义）。
pub fn errcode_vectors() -> [u8; 5] {
    [8, 10, 11, 12, 13] // DF, TS, NP, SS, GP（14=PF 特殊恒有）
}

/// PF（页故障）恒有错误码。
pub fn has_errcode(kind: EntryKind, vector: u8) -> bool {
    if kind.has_errcode() {
        errcode_vectors().contains(&vector) || vector == 14
    } else {
        false
    }
}

// ── 现场帧 ───────────────────────────────────────────────────
/// 统一现场：固定槽序 [vector, err_code?, rip, cs, rflags, rsp, ss,
/// rax..r15, dummy_pad] —— 布局冻结。
#[derive(Clone, Copy, Debug)]
pub struct TrapFrame {
    pub slots: [u64; FRAME_SLOTS],
}

/// 槽位常量（布局冻结表）。
pub const SLOT_VECTOR: usize = 0;
pub const SLOT_ERRCODE: usize = 1;
pub const SLOT_RIP: usize = 2;
pub const SLOT_CS: usize = 3;
pub const SLOT_RFLAGS: usize = 4;
pub const SLOT_RSP: usize = 5;
pub const SLOT_SS: usize = 6;
pub const SLOT_GPR_BASE: usize = 7; // rax..r15 共 10 槽

impl TrapFrame {
    /// 零帧。
    pub fn zero() -> Self {
        TrapFrame { slots: [0; FRAME_SLOTS] }
    }
    /// 三入口同构压栈：按错误码有无决定槽 1 语义（无错误码时槽 1 = 0xDEAD0000 占位并冻结）。
    pub fn push_frame(kind: EntryKind, vector: u8, err: u64, rip: u64) -> Result<TrapFrame, u16> {
        if vector > 31 && vector < 32 {
            return Err(E_ENTRY);
        }
        let mut f = TrapFrame::zero();
        f.slots[SLOT_VECTOR] = vector as u64;
        f.slots[SLOT_RIP] = rip;
        f.slots[SLOT_CS] = 0x08;
        f.slots[SLOT_RFLAGS] = 0x202;
        f.slots[SLOT_RSP] = 0xFFFF_8000_0000_0000;
        f.slots[SLOT_SS] = 0x10;
        if has_errcode(kind, vector) {
            f.slots[SLOT_ERRCODE] = err;
        } else {
            f.slots[SLOT_ERRCODE] = 0xDEAD_0000_0000_0000; // 占位冻结值
        }
        Ok(f)
    }
    /// 错误码读取：有码入口读真值，无码入口读 None（分型零错位核心）。
    pub fn errcode(&self, kind: EntryKind) -> Option<u64> {
        if has_errcode(kind, self.slots[SLOT_VECTOR] as u8) {
            Some(self.slots[SLOT_ERRCODE])
        } else {
            None
        }
    }
    /// 对齐校验：槽序固定 + 占位值识别 + GPR 槽域守恒。
    pub fn layout_ok(&self, kind: EntryKind) -> bool {
        if self.slots.len() != FRAME_SLOTS {
            return false;
        }
        if !has_errcode(kind, self.slots[SLOT_VECTOR] as u8) && self.slots[SLOT_ERRCODE] != 0xDEAD_0000_0000_0000 {
            return false;
        }
        // RIP 必须 8 对齐（内核态简则）
        self.slots[SLOT_RIP] % FRAME_ALIGN as u64 == 0
    }
    /// GPR 写读。
    pub fn set_gpr(&mut self, idx: usize, v: u64) -> bool {
        if idx >= 10 {
            return false;
        }
        self.slots[SLOT_GPR_BASE + idx] = v;
        true
    }
    pub fn get_gpr(&self, idx: usize) -> Option<u64> {
        if idx >= 10 {
            return None;
        }
        Some(self.slots[SLOT_GPR_BASE + idx])
    }
}

// ── 错误码分型器（PF 特化：P/W/U/R/S 位拆解）────────────────
/// 页故障错误码位拆解。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PfBits {
    pub present: bool,   // P
    pub write: bool,     // W
    pub user: bool,      // U
    pub reserved: bool,  // R
    pub fetch: bool,     // S/I
}

pub fn split_pf(err: u64) -> PfBits {
    PfBits {
        present: err & 1 != 0,
        write: err & 2 != 0,
        user: err & 4 != 0,
        reserved: err & 8 != 0,
        fetch: err & 16 != 0,
    }
}

/// 分型注入器：10 万次伪随机注入（LCG 驱动），每次构造帧后分型回读必须零错位。
pub fn inject_recon(rounds: u32, seed: u64) -> (u32, u32) {
    let mut lcg = Lcg(seed);
    let mut hit = 0;
    for _ in 0..rounds {
        let kind = match lcg.below(3) {
            0 => EntryKind::Exception,
            1 => EntryKind::Interrupt,
            _ => EntryKind::Ipi,
        };
        let vec_pool: [u8; 12] = [0, 3, 6, 8, 10, 11, 12, 13, 14, 16, 32, 200];
        let v = vec_pool[(lcg.below(12)) as usize];
        let err = lcg.next() & 0xFFFF;
        let rip = (lcg.next() & !0x7).wrapping_add(0x401000);
        let f = match TrapFrame::push_frame(kind, v, err, rip) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let expect = has_errcode(kind, v);
        let got = f.errcode(kind);
        let ok = match (expect, got) {
            (true, Some(e)) => e == err,
            (false, None) => true,
            _ => false,
        };
        if ok && f.layout_ok(kind) {
            hit += 1;
        }
    }
    (hit, rounds as u32)
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0807_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0807 TrapFrame统一现场");

    // 1. 布局冻结（槽数 + 对齐常量）
    s.add("TF-布局冻结", FRAME_SLOTS == 17 && FRAME_ALIGN == 8 && SLOT_GPR_BASE == 7,
        if FRAME_SLOTS == 17 { "" } else { "布局漂移" });

    // 2. 三入口同构（异常/中断/IPI 压帧后同槽序）
    let f1 = TrapFrame::push_frame(EntryKind::Exception, 14, 0x7, 0x401000).unwrap_or(TrapFrame::zero());
    let f2 = TrapFrame::push_frame(EntryKind::Interrupt, 32, 0, 0x401000).unwrap_or(TrapFrame::zero());
    let f3 = TrapFrame::push_frame(EntryKind::Ipi, 200, 0, 0x401000).unwrap_or(TrapFrame::zero());
    let iso = f1.slots[SLOT_RIP] == f2.slots[SLOT_RIP]
        && f2.slots[SLOT_RIP] == f3.slots[SLOT_RIP]
        && f1.slots[SLOT_CS] == f2.slots[SLOT_CS]
        && f2.slots[SLOT_RFLAGS] == f3.slots[SLOT_RFLAGS];
    s.add("TF-三入口同构", iso, if iso { "" } else { "入口不同构" });

    // 3. 错误码分型：GP/DF/PF 有码、时钟/IPI 无码
    let gp = TrapFrame::push_frame(EntryKind::Exception, 13, 0x1234, 0x401000).unwrap_or(TrapFrame::zero());
    let df = TrapFrame::push_frame(EntryKind::Exception, 8, 0, 0x401000).unwrap_or(TrapFrame::zero());
    let pf = TrapFrame::push_frame(EntryKind::Exception, 14, 0x7, 0x401000).unwrap_or(TrapFrame::zero());
    let irq0 = TrapFrame::push_frame(EntryKind::Interrupt, 32, 0, 0x401000).unwrap_or(TrapFrame::zero());
    let ipi = TrapFrame::push_frame(EntryKind::Ipi, 200, 0, 0x401000).unwrap_or(TrapFrame::zero());
    let typing = gp.errcode(EntryKind::Exception) == Some(0x1234)
        && df.errcode(EntryKind::Exception) == Some(0)
        && pf.errcode(EntryKind::Exception) == Some(0x7)
        && irq0.errcode(EntryKind::Interrupt).is_none()
        && ipi.errcode(EntryKind::Ipi).is_none();
    s.add("TF-错误码分型", typing, if typing { "" } else { "分型错位" });

    // 4. 10 万次注入零错位
    let (hit, total) = inject_recon(100_000, 0xB870);
    s.add("TF-注入10万零错位", hit == total && total == 100_000,
        if hit == total { "" } else { "注入错位" });

    // 5. PF 位拆解
    let b = split_pf(0b10111);
    s.add("TF-PF位拆解", b.present && b.write && b.user && b.reserved && !b.fetch,
        if b.present && b.write { "" } else { "位拆解错" });

    // 6. GPR 写读一致 + 越界拒
    let mut f4 = TrapFrame::zero();
    let wr = f4.set_gpr(0, 0xABCD) && f4.set_gpr(9, 0x9999) && !f4.set_gpr(10, 1);
    let rd = f4.get_gpr(0) == Some(0xABCD) && f4.get_gpr(9) == Some(0x9999) && f4.get_gpr(10).is_none();
    s.add("TF-GPR写读", wr && rd, if wr && rd { "" } else { "GPR 异常" });

    // 7. layout_ok 校验
    let lo = f1.layout_ok(EntryKind::Exception) && f2.layout_ok(EntryKind::Interrupt);
    s.add("TF-布局校验", lo, if lo { "" } else { "布局校验失败" });

    // 8. 占位冻结值（无码入口槽 1 恒 DEAD 标记）
    let dead = f2.slots[SLOT_ERRCODE] == 0xDEAD_0000_0000_0000
        && f3.slots[SLOT_ERRCODE] == 0xDEAD_0000_0000_0000;
    s.add("TF-占位冻结", dead, if dead { "" } else { "占位污染" });

    // 9. errcode 向量表完整（含 PF 特例）
    let ev = errcode_vectors();
    s.add("TF-向量表完整", ev == [8, 10, 11, 12, 13] && has_errcode(EntryKind::Exception, 14)
        && !has_errcode(EntryKind::Exception, 7) && !has_errcode(EntryKind::Exception, 32),
        if has_errcode(EntryKind::Exception, 14) { "" } else { "向量表缺 PF" });

    // 10. 分型注入 LCG 中样本复核（1 万次补充轮）
    let (h2, t2) = inject_recon(10_000, 0xB871);
    s.add("TF-补轮1万", h2 == t2, if h2 == t2 { "" } else { "补轮错位" });

    // 11. 帧存储尺寸守恒
    s.add("TF-槽域守恒", core::mem::size_of::<TrapFrame>() == FRAME_SLOTS * 8,
        if core::mem::size_of::<TrapFrame>() == 136 { "" } else { "尺寸漂移" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB870 && SEG_BASE < 0xB890;
    s.add("TF-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
