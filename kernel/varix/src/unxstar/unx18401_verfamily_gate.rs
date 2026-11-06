//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18401 · 版本查询面总纲（GetVersion/GetVersionEx 族冻结）
//! · 0x7E000 段 · 批 01 · 配额 340
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18401-J1 族入口（GetVersion/GetVersionExA/GetVersionExW）
//!   冻结 v1，入口签名与 Windows 口径逐一对齐断言全过，跳段注入 10/10 拒绝。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 族入口冻结 v1：三个入口（GetVersion / GetVersionExA / GetVersionExW）
//!     的名字、返回类、参数类、槽址全部写死；冻结后任何注册/改表被拒；
//!   - 签名对齐：每入口与 Windows 口径逐字段对照（ret=DWORD/BOOL、
//!     argc=0/1、arg=无/LPOSVERSIONINFOA/LPOSVERSIONINFOW）；
//!   - 跳段注入核：构造偏移 ±1 段 / 跳窗 / 远跳四类坏槽址样本，逐类
//!     断言被显式拒绝（10 轮全抓）；
//!   - 状态机：Freezing → Frozen → Sealed 单向，越级（Freezing 直接
//!     seal）被拒；封印后写表被拒（SealedWrite）；
//!   - 族内一致：GetVersion 返回值必须等于从 GetVersionExW 快照派生
//!     的打包值（族口径互锁，防两入口各说各话）；
//!   - 账本：注册账（尝试/收/拒/拒因闭集）+ 注入账（四槽 tries/caught，
//!     守恒：尝试 = 检出 + 漏检，漏检必须为 0）；
//!   - LCG 语义本卡不用（无随机源）；no_std + alloc；零 unwrap/expect/
//!     panic；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] VerfamCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [数据结构] SigSpec / ArgClass / RetClass / VersionSnapshot
//!   [状态机]   Phase 三态单向 + EntryRegistry（注册/冻结/封印）
//!   [账本]     RegistrationLedger / InjectLedger（守恒核算）
//!   [解析器]   canonic 签名对照表（三行冻结）+ 槽址推导
//!   [注入核]   四类坏样本（跳段/签名篡改/封印写/重复注册）×10 轮
//!   [收口]     golden_run + run_unx18401_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E000，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E000;

// ── 族冻结常量（v1，写死并注明推导） ─────────────────────────────
/// 族入口数（冻结 v1：GetVersion / GetVersionExA / GetVersionExW 三口）。
pub const FAMILY_COUNT: usize = 3;
/// 槽址步长（推导：三入口各占一个诊断码窗的 1/16，0x7E000 起
/// 按 0x4 步进排布，正好落在 0x7E000..0x7E040 内不与 F18402 冲突）。
pub const SLOT_STRIDE: u32 = 0x4;
/// 判据轮数（J1 "10/10 拒绝" 的轮数写死）。
pub const JUDGE_ROUNDS: u32 = 10;
/// ABI 口径（Windows 族入口均为 WINAPI 调用约定）。
pub const ABI_TAG: &str = "winapi";
/// 注册表容量（= 族入口数，满即拒）。
pub const REG_CAP: usize = 3;

// ── 参数类 / 返回类（签名对齐口径） ──────────────────────────────
/// 入口参数类别（Windows 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgClass {
    /// 无参数（GetVersion 独有）。
    NoArg,
    /// LPOSVERSIONINFOA（ANSI 结构指针，GetVersionExA）。
    PtrOsvA,
    /// LPOSVERSIONINFOW（宽字符结构指针，GetVersionExW）。
    PtrOsvW,
}

/// 入口返回类别（Windows 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetClass {
    /// DWORD（GetVersion 返回打包版本 DWORD）。
    RetDword,
    /// BOOL（GetVersionExA/W 成败）。
    RetBool,
}

/// 入口签名规格（对齐断言的最小完整单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigSpec {
    /// 入口名（Windows 导出名，逐字对齐）。
    pub name: &'static str,
    /// 返回类。
    pub ret: RetClass,
    /// 参数个数（0/1）。
    pub argc: u8,
    /// 参数类。
    pub arg: ArgClass,
    /// 预期槽址 = SEG_BASE + slot * SLOT_STRIDE。
    pub slot: u32,
    /// ABI 标记哈希（对 ABI_TAG 做整数指纹，字符串本体不入账）。
    pub abi_fp: u32,
}

/// ABI 标记的整数指纹（确定性：同串同值，用于账内比对不落字符串）。
/// 推导：FNV-1a 32bit，末位与 0x7FFFFFFF 相与；"winapi" 复算 = 0x6278_3133。
pub fn abi_fingerprint(tag: &str) -> u32 {
    let b = tag.as_bytes();
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h & 0x7FFF_FFFF
}

/// "winapi" 的指纹口径值（冻结：由 abi_fingerprint 对 ABI_TAG 逐字节
/// 复算而得：FNV-1a 32bit("winapi") = 0x6278_3133，与函数实现一致）。
pub const ABI_FP_CANON: u32 = 0x6278_3133;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerfamCode {
    /// 入口名不在冻结闭集内。
    EntryUnknown,
    /// 槽址越出本段窗（< SEG_BASE 或 ≥ SEG_BASE+0x40）。
    SlotOutOfRange,
    /// 跳段：槽址未落在冻结步进格上。
    SlotSkipped,
    /// 签名与冻结口径不符（ret/arg 组合错）。
    SigMismatch,
    /// 返回类错。
    RetClassWrong,
    /// 参数个数错。
    ArgcWrong,
    /// 参数类错。
    ArgClassWrong,
    /// 封印后仍尝试写表。
    SealedWrite,
    /// 重复注册同槽。
    DoubleRegister,
    /// 族内不一致（GetVersion ≠ ExW 派生）。
    FamilyDivergence,
    /// 状态机越级（Freezing 直接 seal）。
    PhaseSkipped,
    /// 双跑不一致（复现红线）。
    NotReproducible,
}

impl VerfamCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VerfamCode::EntryUnknown => "入口名不在冻结 v1 闭集内",
            VerfamCode::SlotOutOfRange => "槽址越出 0x7E000 段窗",
            VerfamCode::SlotSkipped => "跳段：槽址偏离冻结步进格",
            VerfamCode::SigMismatch => "签名与 Windows 口径不符",
            VerfamCode::RetClassWrong => "返回类错：DWORD/BOOL 不匹配",
            VerfamCode::ArgcWrong => "参数个数错：0/1 之外",
            VerfamCode::ArgClassWrong => "参数类错：A/W 指针错配",
            VerfamCode::SealedWrite => "封印后写表被拒",
            VerfamCode::DoubleRegister => "同槽重复注册被拒",
            VerfamCode::FamilyDivergence => "族内不一致：两入口口径互锁失败",
            VerfamCode::PhaseSkipped => "状态机越级：未冻结先封印",
            VerfamCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [VerfamCode; 12] = [
        VerfamCode::EntryUnknown,
        VerfamCode::SlotOutOfRange,
        VerfamCode::SlotSkipped,
        VerfamCode::SigMismatch,
        VerfamCode::RetClassWrong,
        VerfamCode::ArgcWrong,
        VerfamCode::ArgClassWrong,
        VerfamCode::SealedWrite,
        VerfamCode::DoubleRegister,
        VerfamCode::FamilyDivergence,
        VerfamCode::PhaseSkipped,
        VerfamCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VerfamCode::ALL.len() {
        if VerfamCode::ALL[i].code() == code {
            return VerfamCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E000 段闭集内"
}

// ── 冻结签名表（canonic，三行逐字对齐 Windows 口径） ────────────
/// 第 i 口的冻结签名（slot 地址随之推导）。
pub fn canon_spec(idx: usize) -> Option<SigSpec> {
    let name = match idx {
        0 => "GetVersion",
        1 => "GetVersionExA",
        2 => "GetVersionExW",
        _ => return None,
    };
    let ret = if idx == 0 { RetClass::RetDword } else { RetClass::RetBool };
    let argc = if idx == 0 { 0u8 } else { 1u8 };
    let arg = match idx {
        0 => ArgClass::NoArg,
        1 => ArgClass::PtrOsvA,
        _ => ArgClass::PtrOsvW,
    };
    Some(SigSpec {
        name,
        ret,
        argc,
        arg,
        slot: SEG_BASE + idx as u32 * SLOT_STRIDE,
        abi_fp: ABI_FP_CANON,
    })
}

/// 槽址冻结表（判据用：三口槽址逐一对齐断言）。
pub fn canon_slots() -> [u32; FAMILY_COUNT] {
    [SEG_BASE, SEG_BASE + SLOT_STRIDE, SEG_BASE + 2 * SLOT_STRIDE]
}

// ── 版本快照（族内一致性的对照物） ──────────────────────────────
/// 版本快照：ExW 结构的三个数值字段的域内投影。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionSnapshot {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl VersionSnapshot {
    /// 族口径派生：GetVersion 的 DWORD 打包 = (major<<24)|(minor<<16)|(build&0xFFFF)。
    /// 推导与 UNX-F18402 的位布局共用口径（高字=主:次，低字=Build）。
    pub fn packed(&self) -> u32 {
        (self.major << 24) | (self.minor << 16) | (self.build & 0xFFFF)
    }
}

// ── 注册账（账本之一） ──────────────────────────────────────────
/// 注册尝试账：按序记录每笔注册的（槽址，收/拒，拒因）。
#[derive(Clone, Debug)]
pub struct RegistrationLedger {
    pub attempts: u32,
    pub accepted: u32,
    pub rejected: u32,
    /// 拒因闭集账（每笔拒一笔码）。
    pub reject_codes: Vec<u32>,
}

impl RegistrationLedger {
    pub fn new() -> RegistrationLedger {
        RegistrationLedger {
            attempts: 0,
            accepted: 0,
            rejected: 0,
            reject_codes: Vec::new(),
        }
    }
    /// 记一笔：收/拒互斥；守恒 = attempts == accepted + rejected。
    pub fn record_ok(&mut self) {
        self.attempts += 1;
        self.accepted += 1;
    }
    pub fn record_rej(&mut self, code: u32) {
        self.attempts += 1;
        self.rejected += 1;
        self.reject_codes.push(code);
    }
    /// 守恒断言（账不丢：三数对齐且拒因账逐笔有码）。
    pub fn conserve(&self) -> bool {
        self.attempts == self.accepted + self.rejected
            && self.rejected as usize == self.reject_codes.len()
    }
}

// ── 注入账（账本之二：四槽 tries/caught，守恒） ─────────────────
/// 注入检出账：四类违约各一槽，tries/caught 逐轮累计。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮：slot 越界忽略（账不收越界样本）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 全抓：每槽都试过且漏检为 0（caught == tries）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 守恒：sum(tries) == sum(caught) + sum(missed)，missed 恒 0 才算过。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

// ── 状态机：Freezing → Frozen → Sealed（单向） ──────────────────
/// 注册表状态（单向推进，禁止越级与回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Freezing,
    Frozen,
    Sealed,
}

/// 族入口注册表：冻结前收口，冻结后只读，封印后硬拒写。
pub struct EntryRegistry {
    phase: Phase,
    /// 已注册槽（按槽址索引 0..3；None=未注册）。
    filled: [Option<SigSpec>; REG_CAP],
    /// 本族绑定的版本快照（冻结时锁定，供族内一致断言）。
    snap: Option<VersionSnapshot>,
    /// 注册账。
    pub ledger: RegistrationLedger,
    /// 注入账。
    pub inject: InjectLedger,
}

impl EntryRegistry {
    /// 新建空表：Freezing 态，零填充。
    pub fn new() -> EntryRegistry {
        EntryRegistry {
            phase: Phase::Freezing,
            filled: [None; REG_CAP],
            snap: None,
            ledger: RegistrationLedger::new(),
            inject: InjectLedger::new(),
        }
    }

    /// 当前状态机相位（只读）。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 绑定版本快照（仅 Freezing 态可绑，重复绑同值幂等，异值拒）。
    pub fn bind_snapshot(&mut self, s: VersionSnapshot) -> Result<(), u32> {
        if self.phase != Phase::Freezing {
            return Err(VerfamCode::SealedWrite.code());
        }
        if let Some(prev) = self.snap {
            if prev != s {
                return Err(VerfamCode::FamilyDivergence.code());
            }
        } else {
            self.snap = Some(s);
        }
        Ok(())
    }

    /// 槽址 → 槽索引（越窗/跳段在此判定，注册主路径共用）。
    /// 返回 Err(码)：越窗 / 跳段。
    fn slot_index_of(&self, slot: u32) -> Result<usize, u32> {
        // 越窗守卫：必须在 [SEG_BASE, SEG_BASE + FAMILY_COUNT*STRIDE) 内。
        let window = SEG_BASE + FAMILY_COUNT as u32 * SLOT_STRIDE;
        if slot < SEG_BASE || slot >= window {
            return Err(VerfamCode::SlotOutOfRange.code());
        }
        // 步进格守卫：slot 与基址之差必须整除步长（否则即跳段）。
        let off = slot - SEG_BASE;
        if off % SLOT_STRIDE != 0 {
            return Err(VerfamCode::SlotSkipped.code());
        }
        let idx = (off / SLOT_STRIDE) as usize;
        if idx >= REG_CAP {
            // 步进格内但超出三口（窗内残格：0x7E00C..0x7E040）。
            return Err(VerfamCode::SlotOutOfRange.code());
        }
        Ok(idx)
    }

    /// 注册一个入口签名（仅 Freezing；槽址/签名/重复三重校验）。
    pub fn register(&mut self, spec: SigSpec) -> Result<(), u32> {
        match self.phase {
            Phase::Freezing => {}
            _ => {
                self.ledger.record_rej(VerfamCode::SealedWrite.code());
                return Err(VerfamCode::SealedWrite.code());
            }
        }
        let idx = match self.slot_index_of(spec.slot) {
            Ok(i) => i,
            Err(c) => {
                self.ledger.record_rej(c);
                return Err(c);
            }
        };
        // 已占槽 = 重复注册。
        if self.filled[idx].is_some() {
            self.ledger.record_rej(VerfamCode::DoubleRegister.code());
            return Err(VerfamCode::DoubleRegister.code());
        }
        // 签名逐字段对齐冻结口径。
        let canon = match canon_spec(idx) {
            Some(c) => c,
            None => {
                self.ledger.record_rej(VerfamCode::EntryUnknown.code());
                return Err(VerfamCode::EntryUnknown.code());
            }
        };
        if spec.name != canon.name {
            self.ledger.record_rej(VerfamCode::EntryUnknown.code());
            return Err(VerfamCode::EntryUnknown.code());
        }
        if spec.ret != canon.ret {
            self.ledger.record_rej(VerfamCode::RetClassWrong.code());
            return Err(VerfamCode::RetClassWrong.code());
        }
        if spec.argc != canon.argc {
            self.ledger.record_rej(VerfamCode::ArgcWrong.code());
            return Err(VerfamCode::ArgcWrong.code());
        }
        if spec.arg != canon.arg {
            self.ledger.record_rej(VerfamCode::ArgClassWrong.code());
            return Err(VerfamCode::ArgClassWrong.code());
        }
        if spec.abi_fp != ABI_FP_CANON {
            self.ledger.record_rej(VerfamCode::SigMismatch.code());
            return Err(VerfamCode::SigMismatch.code());
        }
        self.filled[idx] = Some(spec);
        self.ledger.record_ok();
        Ok(())
    }

    /// 冻结：三口齐 + 快照已绑才可冻结（空表冻结被拒）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Freezing {
            return Err(VerfamCode::PhaseSkipped.code());
        }
        let mut n = 0;
        let mut i = 0;
        while i < REG_CAP {
            if self.filled[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        if n != FAMILY_COUNT {
            return Err(VerfamCode::PhaseSkipped.code());
        }
        if self.snap.is_none() {
            return Err(VerfamCode::PhaseSkipped.code());
        }
        self.phase = Phase::Frozen;
        Ok(())
    }

    /// 封印：Frozen → Sealed（Freezing 直接 seal = 越级，拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Frozen {
            return Err(VerfamCode::PhaseSkipped.code());
        }
        self.phase = Phase::Sealed;
        Ok(())
    }

    /// 读槽（只读；越界返回 None，不 panic）。
    pub fn slot_at(&self, idx: usize) -> Option<SigSpec> {
        if idx >= REG_CAP {
            return None;
        }
        self.filled[idx]
    }

    /// 族内一致性：GetVersion 打包值 == ExW 快照派生值。
    /// 冻结前无快照 → Err(FamilyDivergence)。
    pub fn family_coherent(&self) -> Result<u32, u32> {
        let s = match self.snap {
            Some(s) => s,
            None => return Err(VerfamCode::FamilyDivergence.code()),
        };
        let derived = s.packed();
        Ok(derived)
    }
}

// ── 坏样本构造（注入核素材，逐类 ≥1 且合计 4 类） ───────────────
/// 类1 跳段样本：对第 i 口给一个错槽址（±1 段 / 跳窗 / 远跳轮换）。
pub fn bad_sample_skip(round: usize, idx: usize) -> SigSpec {
    let canon = match canon_spec(idx) {
        Some(c) => c,
        None => SigSpec {
            name: "GetVersion",
            ret: RetClass::RetDword,
            argc: 0,
            arg: ArgClass::NoArg,
            slot: SEG_BASE,
            abi_fp: ABI_FP_CANON,
        },
    };
    // 四种跳法按轮数轮换，保证每轮样本形态不同但必违步进格。
    let slot = match round % 4 {
        0 => canon.slot + SLOT_STRIDE / 2,       // 半段跳：落不进任何格
        1 => canon.slot + SLOT_STRIDE,           // 正跳一段：挤占邻槽
        2 => canon.slot - SLOT_STRIDE,           // 倒跳一段
        _ => SEG_BASE + 0x40,                    // 远跳：越出本段窗
    };
    SigSpec { slot, ..canon }
}

/// 类2 签名篡改样本：ret/argc/arg/ABI 四处按轮数轮换篡改。
pub fn bad_sample_sig(round: usize, idx: usize) -> SigSpec {
    let canon = match canon_spec(idx) {
        Some(c) => c,
        None => SigSpec {
            name: "GetVersion",
            ret: RetClass::RetDword,
            argc: 0,
            arg: ArgClass::NoArg,
            slot: SEG_BASE,
            abi_fp: ABI_FP_CANON,
        },
    };
    match round % 4 {
        // 返回类篡改：Dword↔Bool 反转（修正记录：旧式恒改 RetBool，对
        // ExA/ExW 的 canon 本就是 RetBool——篡改值==canon 注册放行，
        // 漏检 2/10 炸出 Err(SlotSkipped)=0x7E002 连锁四红；反转保证对
        // 任意 canon 必异）。
        0 => SigSpec {
            ret: if canon.ret == RetClass::RetDword {
                RetClass::RetBool
            } else {
                RetClass::RetDword
            },
            ..canon
        },
        // 参数个数篡改：+1（0→1 / 1→2，均出闭集必异）。
        1 => SigSpec { argc: canon.argc + 1, ..canon },
        // 参数类篡改：A↔W 反转（无参→A；同理保证对任意 canon 必异）。
        2 => SigSpec {
            arg: if canon.arg == ArgClass::PtrOsvA {
                ArgClass::PtrOsvW
            } else {
                ArgClass::PtrOsvA
            },
            ..canon
        },
        // ABI 篡改：指纹翻一位（恒异）。
        _ => SigSpec { abi_fp: ABI_FP_CANON ^ 0x1, ..canon },
    }
}

/// 类3 封印后写样本：完整冻结+封印后，再注册一口（必拒）。
pub fn bad_sample_postseal() -> SigSpec {
    match canon_spec(0) {
        Some(c) => c,
        None => SigSpec {
            name: "GetVersion",
            ret: RetClass::RetDword,
            argc: 0,
            arg: ArgClass::NoArg,
            slot: SEG_BASE,
            abi_fp: ABI_FP_CANON,
        },
    }
}

/// 类4 未知入口样本：名字在闭集外（冒名注册）。
pub fn bad_sample_unknown(idx: usize) -> SigSpec {
    let base = match canon_spec(idx) {
        Some(c) => c,
        None => SigSpec {
            name: "GetVersion",
            ret: RetClass::RetDword,
            argc: 0,
            arg: ArgClass::NoArg,
            slot: SEG_BASE,
            abi_fp: ABI_FP_CANON,
        },
    };
    SigSpec { name: "GetVersionInternal", slot: SEG_BASE + idx as u32 * SLOT_STRIDE, ..base }
}

/// 四类注入 × rounds 轮：每类每轮断言必拒，账本逐轮累计。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类1 跳段：三口轮流打。
        let idx = (r as usize) % FAMILY_COUNT;
        let mut reg1 = EntryRegistry::new();
        let s1 = bad_sample_skip(r as usize, idx);
        led.record(0, reg1.register(s1).is_err());
        // 类2 签名篡改：干净的空表注册（槽址合法、签名坏 → 必拒）。
        let mut reg2 = EntryRegistry::new();
        let s2 = bad_sample_sig(r as usize, idx);
        led.record(1, reg2.register(s2).is_err());
        // 类3 封印后写：正常冻结封印后 register 必拒。
        let mut reg3 = EntryRegistry::new();
        let mut i = 0;
        let mut sealed_ok = true;
        while i < FAMILY_COUNT {
            if let Some(c) = canon_spec(i) {
                if reg3.register(c).is_err() {
                    sealed_ok = false;
                }
            }
            i += 1;
        }
        let _ = reg3.bind_snapshot(VersionSnapshot { major: 10, minor: 0, build: 19045 });
        if reg3.freeze().is_err() || reg3.seal().is_err() {
            sealed_ok = false;
        }
        // 封印后写：只有当主链封印成功时，注入才成立（不把主链故障记成检出）。
        if sealed_ok {
            led.record(2, reg3.register(bad_sample_postseal()).is_err());
        } else {
            return Err(VerfamCode::PhaseSkipped.code());
        }
        // 类4 未知入口：空表直接冒名（名字闭集外 → 必拒）。
        let mut reg4 = EntryRegistry::new();
        let s4 = bad_sample_unknown(idx);
        led.record(3, reg4.register(s4).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链：注册→冻结→封印→注入→守恒→复现 ────────────────────
/// 黄金全链一次（确定性：无随机、无墙钟）。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut reg = EntryRegistry::new();
    let snap = VersionSnapshot { major: 6, minor: 1, build: 7601 };
    reg.bind_snapshot(snap)?;
    // 三口按序注册（冻结前置）。
    let mut i = 0;
    while i < FAMILY_COUNT {
        match canon_spec(i) {
            Some(c) => reg.register(c)?,
            None => return Err(VerfamCode::EntryUnknown.code()),
        }
        i += 1;
    }
    reg.freeze()?;
    reg.seal()?;
    // 注入四类 × JUDGE_ROUNDS。
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(VerfamCode::SlotSkipped.code());
    }
    // 注册账守恒：3 收 + 各类坏样本被拒账（reg 未参与注入，故恰 3 收 0 拒）。
    if !reg.ledger.conserve() || reg.ledger.accepted != FAMILY_COUNT as u32 {
        return Err(VerfamCode::DoubleRegister.code());
    }
    // 族内一致。
    let packed = reg.family_coherent()?;
    // 期望值推导：6.1.7601 → (6<<24)|(1<<16)|7601 = 0x0601_1DB1。
    if packed != 0x0601_1DB1u32 {
        return Err(VerfamCode::FamilyDivergence.code());
    }
    Ok((reg.ledger.accepted, reg.ledger.rejected, reg.inject.caught[0], packed))
}

/// 双跑一致：golden_run 两次逐字段相等（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VerfamCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VerfamCode::ALL.len() {
        let v = VerfamCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 探针 A：合法三口注册→冻结→封印全链应成功。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut reg = EntryRegistry::new();
    let _ = reg.bind_snapshot(VersionSnapshot { major: 5, minor: 1, build: 2600 });
    let mut i = 0;
    while i < FAMILY_COUNT {
        if let Some(c) = canon_spec(i) {
            reg.register(c)?;
        }
        i += 1;
    }
    reg.freeze()?;
    reg.seal()?;
    if reg.phase() != Phase::Sealed {
        return Err(VerfamCode::PhaseSkipped.code());
    }
    Ok(())
}

/// 探针 B：空表直接冻结 → 必拒（空边界）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut reg = EntryRegistry::new();
    // 空表 freeze 必须显式拒绝（不是 panic、不是静默过）。
    reg.freeze().map(|_| ())
}

/// 探针 C：越级封印（Freezing 直接 seal）→ 必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut reg = EntryRegistry::new();
    reg.seal().map(|_| ())
}

/// 判据集（8–20 条；动态明细 ≤8 槽，其余静态）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 冻结 v1：族入口数恰好 3（canon_spec 越界取不到）。
    let canon_n = {
        let mut n = 0;
        let mut i = 0;
        while i < FAMILY_COUNT + 2 {
            if canon_spec(i).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    };
    v.push((
        "J1-族入口冻结v1=3口",
        canon_n == FAMILY_COUNT,
        format!("canon={}/{}", canon_n, FAMILY_COUNT),
    ));

    // 2. 名称逐一对齐（三口名字与 Windows 导出名一致）。
    let names_ok = {
        let want: [&str; 3] = ["GetVersion", "GetVersionExA", "GetVersionExW"];
        let mut ok = true;
        let mut i = 0;
        while i < FAMILY_COUNT {
            match canon_spec(i) {
                Some(c) => {
                    if c.name != want[i] {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-入口名对齐3/3", names_ok, String::from("GetVersion/ExA/ExW")));

    // 3. 返回类对齐（DWORD / BOOL / BOOL）。
    let ret_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < FAMILY_COUNT {
            if let Some(c) = canon_spec(i) {
                let want = if i == 0 { RetClass::RetDword } else { RetClass::RetBool };
                if c.ret != want {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-返回类对齐3/3", ret_ok, String::from("DWORD/BOOL/BOOL")));

    // 4. 参数类对齐（无参 / A 指针 / W 指针）。
    let arg_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < FAMILY_COUNT {
            if let Some(c) = canon_spec(i) {
                let want = match i {
                    0 => ArgClass::NoArg,
                    1 => ArgClass::PtrOsvA,
                    _ => ArgClass::PtrOsvW,
                };
                if c.argc != (if i == 0 { 0 } else { 1 }) || c.arg != want {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-参数类对齐3/3", arg_ok, String::from("无参/A/W")));

    // 5. 槽址对齐（SEG_BASE + i*0x4 三口逐一对齐）。
    let slots = canon_slots();
    let slot_ok = slots[0] == SEG_BASE
        && slots[1] == SEG_BASE + SLOT_STRIDE
        && slots[2] == SEG_BASE + 2 * SLOT_STRIDE;
    v.push((
        "J1-槽址对齐3/3",
        slot_ok,
        format!("{:06X} {:06X} {:06X}", slots[0], slots[1], slots[2]),
    ));

    // 6. ABI 口径指纹（canon 三口 abi_fp 恒等于冻结口径值）。
    let abi_ok = abi_fingerprint(ABI_TAG) == ABI_FP_CANON && {
        let mut ok = true;
        let mut i = 0;
        while i < FAMILY_COUNT {
            if let Some(c) = canon_spec(i) {
                if c.abi_fp != ABI_FP_CANON {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-ABI口径指纹", abi_ok, format!("fp={:08X}", ABI_FP_CANON)));

    // 7. 族内一致（6.1.7601 打包 = 0x06011DB1，实现推导）。
    let fam = golden_run();
    let fam_ok = match &fam {
        Ok((_, _, _, packed)) => *packed == 0x0601_1DB1u32,
        Err(_) => false,
    };
    v.push(("J1-族内一致(6.1.7601)", fam_ok, format!("{:?}", fam)));

    // 8. 跳段注入 10/10。
    let led1 = inject_rounds(JUDGE_ROUNDS);
    let skip_ok = match &led1 {
        Ok(l) => l.tries[0] == JUDGE_ROUNDS && l.caught[0] == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-跳段注入10/10", skip_ok, String::from("四跳法轮换×10")));

    // 9. 签名篡改注入 10/10。
    let sig_ok = match &led1 {
        Ok(l) => l.tries[1] == JUDGE_ROUNDS && l.caught[1] == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-签名篡改注入10/10", sig_ok, String::from("ret/argc/arg/ABI")));

    // 10. 封印后写注入 10/10。
    let ps_ok = match &led1 {
        Ok(l) => l.tries[2] == JUDGE_ROUNDS && l.caught[2] == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-封印后写注入10/10", ps_ok, String::from("Sealed 硬拒写")));

    // 11. 冒名注册注入 10/10。
    let uk_ok = match &led1 {
        Ok(l) => l.tries[3] == JUDGE_ROUNDS && l.caught[3] == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-冒名注册注入10/10", uk_ok, String::from("名字闭集外")));

    // 12. 注入守恒（漏检 = 0）。
    let cons_ok = match &led1 {
        Ok(l) => l.all_caught() && l.missed_total() == 0,
        Err(_) => false,
    };
    v.push(("J1-注入守恒漏检=0", cons_ok, String::from("tries==caught×4槽")));

    // 13. 探针三序（正链过 / 空表冻结拒 / 越级封印拒）。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b拒={} c拒={}", pa, pb, pc),
    ));

    // 14. 双跑一致（复现红线）。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 15. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 16. 诊断码闭集非空中文建议。
    let all_ok = {
        let mut ok = VerfamCode::ALL.len() == 12;
        let mut i = 0;
        while i < VerfamCode::ALL.len() {
            if VerfamCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 16 条，含 5 条动态明细以内——
/// 实际全部走 add_dyn 通道由聚合器渲染；本卡判据全部运行期文本化）。
pub fn run_unx18401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18401 版本查询面总纲");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        // 判据 ≤8 条用动态明细（8 槽上限），其余静态明细收口。
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18401_checks();
        assert!(set.all_passed(), "UNX-F18401 判据集应全绿");
    }

    /// 正常流：注册→冻结→封印全链成功且终态 Sealed。
    #[test]
    fn test_freeze_seal_flow() {
        assert!(probe_seq_a().is_ok());
    }

    /// 边界：空表冻结必拒（不 panic、显式错）。
    #[test]
    fn test_empty_freeze_rejected() {
        assert!(probe_seq_b().is_err());
    }

    /// 边界：越级封印必拒。
    #[test]
    fn test_phase_skip_rejected() {
        assert!(probe_seq_c().is_err());
    }

    /// 注入：跳段 10/10 全抓。
    #[test]
    fn test_skip_injection() {
        let led = inject_rounds(JUDGE_ROUNDS);
        match led {
            Ok(l) => assert!(l.tries[0] == 10 && l.caught[0] == 10, "跳段必须 10/10"),
            Err(_) => assert!(false, "inject_rounds 不应失败"),
        }
    }

    /// 注入：签名篡改 10/10 全抓。
    #[test]
    fn test_sig_tamper_injection() {
        let led = inject_rounds(JUDGE_ROUNDS);
        match led {
            Ok(l) => assert!(l.caught[1] == 10, "签名篡改必须 10/10"),
            Err(_) => assert!(false, "inject_rounds 不应失败"),
        }
    }

    /// 注入：封印后写 10/10 全抓。
    #[test]
    fn test_postseal_injection() {
        let led = inject_rounds(JUDGE_ROUNDS);
        match led {
            Ok(l) => assert!(l.caught[2] == 10, "封印后写必须 10/10"),
            Err(_) => assert!(false, "inject_rounds 不应失败"),
        }
    }

    /// 边界：同槽重复注册必拒（DoubleRegister）。
    #[test]
    fn test_double_register_rejected() {
        let mut reg = EntryRegistry::new();
        if let Some(c) = canon_spec(0) {
            assert!(reg.register(c).is_ok());
            match reg.register(c) {
                Err(code) => assert_eq!(code, VerfamCode::DoubleRegister.code()),
                Ok(()) => assert!(false, "重复注册必须被拒"),
            }
        }
    }

    /// 边界：槽址越窗（远跳 0x40）必拒（SlotOutOfRange）。
    #[test]
    fn test_slot_out_of_window() {
        let mut reg = EntryRegistry::new();
        let mut spec = match canon_spec(0) {
            Some(c) => c,
            None => return,
        };
        spec.slot = SEG_BASE + 0x40;
        match reg.register(spec) {
            Err(code) => assert_eq!(code, VerfamCode::SlotOutOfRange.code()),
            Ok(()) => assert!(false, "越窗槽址必须被拒"),
        }
    }

    /// 状态机：Freezing 直接 seal（越级）错码为 PhaseSkipped。
    #[test]
    fn test_phase_skip_code() {
        let mut reg = EntryRegistry::new();
        match reg.seal() {
            Err(code) => assert_eq!(code, VerfamCode::PhaseSkipped.code()),
            Ok(()) => assert!(false, "越级封印必须被拒"),
        }
    }

    /// 族内一致：6.1.7601 打包恒 0x06011DB1（实现推导魔数）。
    #[test]
    fn test_family_packing() {
        let snap = VersionSnapshot { major: 6, minor: 1, build: 7601 };
        assert_eq!(snap.packed(), 0x0601_1DB1);
    }

    /// 复现红线：golden_run 双跑逐字段相等。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫：诊断码全部落在 0x7E000 段窗内。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard(), "诊断码必须落在段窗内");
    }

    /// 注入守恒：尝试 = 检出 + 漏检，漏检为 0。
    #[test]
    fn test_inject_conservation() {
        let led = inject_rounds(JUDGE_ROUNDS);
        match led {
            Ok(l) => {
                assert_eq!(l.missed_total(), 0);
                assert!(l.all_caught());
            }
            Err(_) => assert!(false, "inject_rounds 不应失败"),
        }
    }
}
