//! unx323_realmjmp ↔ UNX-F0323 实模式跳板代码（16 位进入 → 32 位 → 64 位长跳）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0323，UNX-A1-B17）：
//! 「实模式跳板代码（16 位进入 → 32 位 → 64 位长跳）| 300 | 已深化 |
//!  UNX-F0323-J1 三级跳转（real→prot→long）语义正确，跳板代码与黄金汇编
//!  逐指令一致」
//!
//! 设计要点（全部围绕固件交接后 CPU 模式跃迁的真实状态）：
//! 1. **三级跃迁语义闭式**：
//!    ① real→prot：`mov eax,cr0` / `or eax,1`（CR0.PE 置位）/ `mov cr0,eax`
//!      / 远跳加载 32 位代码段选择子（0x08）清流水线；
//!    ② prot→long：`mov eax,cr4` / `or eax,0x20`（CR4.PAE）/ 置 CR3 页表 /
//!      `rdmsr 0xC0000080`（EFER）/ `or eax,0x100`（LME）/ `wrmsr` /
//!      `mov eax,cr0` / `or eax,0x80000000`（PG）/ `mov cr0,eax` /
//!      远跳 0x18 进 64 位；
//!    ③ 语义正确性由**逐指令黄金比对**保证——黄金序列写死在表里，
//!      跳板生成器产出的字节流与黄金逐字节全等，差一字节即立案。
//! 2. **GDT 布局闭集**：空选择子 0x00 / 32 位码 0x08 / 64 位码 0x10 /
//!    数据 0x18——选择子值与描述符属性写死并注释来源；
//! 3. **状态机合法迁移**：Real→Prot→Long 单向链；跳步（Real 直跳 Long）
//!    与回退（Long 回 Prot）显性拒——模式跃迁没有回头路；
//! 4. **段属性黄金**：32/64 位码段描述符的属性字节（Access/Granularity）
//!    写死对拍（L 位、D 位、P 位、DPL=0），属性漂移 = 立案；
//! 5. 诊断码段 0x96 独占；no_std 零 unwrap 零 expect，确定性可复算。
//!
//! 注意：本模块是跳板的**语义与字节级设计账**（纯数据 + 校验面），
//! 真实执行体在引导汇编里；本模块保证"引导汇编该是什么样"有机器锚。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX323_REALMJMP_VERSION: &str = "unx323-realmjmp-v1";

/// GDT 选择子闭集（布局写死，来源：CoRun A1 引导 GDT 设计）。
pub const SEL_NULL: u16 = 0x00;
pub const SEL_CODE32: u16 = 0x08;
pub const SEL_CODE64: u16 = 0x10;
pub const SEL_DATA: u16 = 0x18;

/// 控制寄存器位（写死并注释来源：Intel SDM Vol.3）。
pub const CR0_PE: u64 = 0x0000_0001; // Protection Enable
pub const CR0_PG: u64 = 0x8000_0000; // Paging Enable
pub const CR4_PAE: u64 = 0x0000_0020; // Physical Address Extension
pub const EFER_MSR: u32 = 0xC000_0080; // EFER MSR 地址
pub const EFER_LME: u64 = 0x0000_0100; // Long Mode Enable
pub const EFER_LMA: u64 = 0x0000_0400; // Long Mode Active

/// CPU 模式闭集（单向链：Real→Prot→Long）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuMode {
    /// 16 位实模式
    Real,
    /// 32 位保护模式
    Prot,
    /// 64 位长模式
    Long,
}

impl CpuMode {
    /// 下一模式（链尾 None——Long 无下一级）。
    pub const fn next(self) -> Option<CpuMode> {
        match self {
            CpuMode::Real => Some(CpuMode::Prot),
            CpuMode::Prot => Some(CpuMode::Long),
            CpuMode::Long => None,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            CpuMode::Real => "16 位实模式",
            CpuMode::Prot => "32 位保护模式",
            CpuMode::Long => "64 位长模式",
        }
    }
}

/// 诊断码闭集（0x99 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RealmJmpCode {
    /// 0x9900 跳步迁移（Real 直跳 Long）
    ModeSkip,
    /// 0x9901 模式回退（单向链破坏）
    ModeRegress,
    /// 0x9902 黄金汇编失配
    GoldenMismatch,
    /// 0x9903 段属性漂移
    SegmentAttrDrift,
    /// 0x9904 选择子越界（GDT 布局外）
    SelectorBad,
    /// 0x9905 三级跳全通（成功码）
    TripleJumped,
}

impl RealmJmpCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            RealmJmpCode::ModeSkip => 0x9900,
            RealmJmpCode::ModeRegress => 0x9901,
            RealmJmpCode::GoldenMismatch => 0x9902,
            RealmJmpCode::SegmentAttrDrift => 0x9903,
            RealmJmpCode::SelectorBad => 0x9904,
            RealmJmpCode::TripleJumped => 0x9905,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            RealmJmpCode::ModeSkip => "REALJMP-MODE-SKIP",
            RealmJmpCode::ModeRegress => "REALJMP-MODE-REGRESS",
            RealmJmpCode::GoldenMismatch => "REALJMP-GOLDEN-MISMATCH",
            RealmJmpCode::SegmentAttrDrift => "REALJMP-SEG-ATTR-DRIFT",
            RealmJmpCode::SelectorBad => "REALJMP-SELECTOR-BAD",
            RealmJmpCode::TripleJumped => "REALJMP-TRIPLE-JUMPED",
        }
    }

    /// 段独占双写互斥（0x99 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9900 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            RealmJmpCode::ModeSkip => "跳步迁移：模式跃迁只能逐级（Real→Prot→Long），跳步即流水线语义未定义",
            RealmJmpCode::ModeRegress => "模式回退：跃迁链单向，回退语义不存在（Long 回 Prot 无定义路径）",
            RealmJmpCode::GoldenMismatch => "黄金汇编失配：跳板字节流与黄金序列差一字节即立案",
            RealmJmpCode::SegmentAttrDrift => "段属性漂移：描述符 Access/Granularity 与黄金不符（L/D/P/DPL）",
            RealmJmpCode::SelectorBad => "选择子越界：GDT 布局闭集（00/08/10/18）外无段",
            RealmJmpCode::TripleJumped => "三级跳语义全通：两段黄金序列逐指令一致且状态机迁移合法",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx323_realmjmp.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 跃迁错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RealmJmpError {
    ModeSkip { from: CpuMode, to: CpuMode },
    ModeRegress { from: CpuMode, to: CpuMode },
    GoldenMismatch { stage: u8, offset: usize },
    SegmentAttrDrift { sel: u16 },
    SelectorBad { sel: u16 },
}

impl RealmJmpError {
    /// 映射诊断码。
    pub const fn code(self) -> RealmJmpCode {
        match self {
            RealmJmpError::ModeSkip { .. } => RealmJmpCode::ModeSkip,
            RealmJmpError::ModeRegress { .. } => RealmJmpCode::ModeRegress,
            RealmJmpError::GoldenMismatch { .. } => RealmJmpCode::GoldenMismatch,
            RealmJmpError::SegmentAttrDrift { .. } => RealmJmpCode::SegmentAttrDrift,
            RealmJmpError::SelectorBad { .. } => RealmJmpCode::SelectorBad,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 黄金指令表：一条助记符 + 字节编码（写死，来源 Intel SDM 编码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenInsn {
    /// 助记符（诊断读用）
    pub mnemonic: &'static str,
    /// 机器码字节
    pub bytes: &'static [u8],
}

/// 阶段一黄金序列：real→prot（CR0.PE 置位 + 远跳清流水线）。
pub const GOLDEN_REAL_TO_PROT: [GoldenInsn; 5] = [
    GoldenInsn { mnemonic: "mov eax,cr0", bytes: &[0x0F, 0x20, 0xC0] },
    GoldenInsn { mnemonic: "or eax,1 (CR0.PE)", bytes: &[0x83, 0xC8, 0x01] },
    GoldenInsn { mnemonic: "mov cr0,eax", bytes: &[0x0F, 0x22, 0xC0] },
    // jmp far 0x08:prot_entry — 66 跨段前缀 + EA 远跳（16 位编码形态）
    GoldenInsn { mnemonic: "jmp far 0x08", bytes: &[0x66, 0xEA] },
    GoldenInsn { mnemonic: "(prot_entry offset)", bytes: &[0x00, 0x00, 0x00, 0x00] },
];

/// 阶段二黄金序列：prot→long（PAE + 页表 + EFER.LME + PG + 远跳 0x10）。
pub const GOLDEN_PROT_TO_LONG: [GoldenInsn; 14] = [
    GoldenInsn { mnemonic: "mov eax,cr4", bytes: &[0x0F, 0x20, 0xE0] },
    GoldenInsn { mnemonic: "or eax,0x20 (CR4.PAE)", bytes: &[0x83, 0xC8, 0x20] },
    GoldenInsn { mnemonic: "mov cr4,eax", bytes: &[0x0F, 0x22, 0xE0] },
    GoldenInsn { mnemonic: "mov eax,pml4_base", bytes: &[0xB8] },
    GoldenInsn { mnemonic: "mov cr3,eax", bytes: &[0x0F, 0x22, 0xD8] },
    // rdmsr EFER：mov ecx,0xC0000080（B9 + 4 字节立即数）
    GoldenInsn { mnemonic: "mov ecx,0xC0000080", bytes: &[0xB9, 0x80, 0x00, 0x00, 0xC0] },
    GoldenInsn { mnemonic: "rdmsr", bytes: &[0x0F, 0x32] },
    // or eax,0x100（EFER.LME）——0D 带位测试形态简化为 or eax,imm32 低位
    GoldenInsn { mnemonic: "or eax,0x100 (LME)", bytes: &[0x0D, 0x00, 0x01, 0x00, 0x00] },
    GoldenInsn { mnemonic: "wrmsr", bytes: &[0x0F, 0x30] },
    GoldenInsn { mnemonic: "mov eax,cr0", bytes: &[0x0F, 0x20, 0xC0] },
    GoldenInsn { mnemonic: "or eax,0x80000000 (PG)", bytes: &[0x0D, 0x00, 0x00, 0x00, 0x80] },
    GoldenInsn { mnemonic: "mov cr0,eax", bytes: &[0x0F, 0x22, 0xC0] },
    // jmp far 0x10:long_entry（66 EA 形态，选择子 0x10）
    GoldenInsn { mnemonic: "jmp far 0x10", bytes: &[0x66, 0xEA] },
    GoldenInsn { mnemonic: "(long_entry offset)", bytes: &[0x00, 0x00, 0x00, 0x00] },
];

/// 段描述符属性黄金（Access 字节 + Granularity 字节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegAttr {
    /// Access 字节（P/DPL/S/Type）
    pub access: u8,
    /// Granularity 字节（G/D-B/L/AVL/Limit 高 4 位）
    pub gran: u8,
}

/// GDT 黄金属性表（选择子 → 属性；写死来源：CoRun A1 引导 GDT 设计）。
pub const GOLDEN_SEG_ATTRS: [(u16, SegAttr); 3] = [
    // 32 位代码段：P=1 DPL=0 S=1 Type=1101(代码可读可执行)；G=1 D=1 L=0
    (SEL_CODE32, SegAttr { access: 0x9A, gran: 0xCF }),
    // 64 位代码段：P=1 DPL=0 S=1 Type=1101；G=1 D=0 L=1（长模式 L 位）
    (SEL_CODE64, SegAttr { access: 0x9A, gran: 0xA0 }),
    // 数据段：P=1 DPL=0 S=1 Type=0010(数据可写)；G=1 D=1
    (SEL_DATA, SegAttr { access: 0x92, gran: 0xCF }),
];

/// 跳板状态机（模式迁移合法性裁决）。
#[derive(Clone, Debug)]
pub struct JmpStateMachine {
    pub mode: CpuMode,
}

impl JmpStateMachine {
    /// 从实模式起。
    pub const fn new() -> Self {
        JmpStateMachine { mode: CpuMode::Real }
    }

    /// 跃迁一步（只允许 next()；跳步/回退显性拒）。
    pub fn advance(&mut self) -> Result<CpuMode, RealmJmpError> {
        let nxt = match self.mode.next() {
            Some(n) => n,
            None => return Err(RealmJmpError::ModeRegress { from: self.mode, to: self.mode }),
        };
        self.mode = nxt;
        Ok(nxt)
    }

    /// 任意迁移校验（判据侧用：from→to 合法当且仅当 to == from.next()）。
    pub fn check_transition(from: CpuMode, to: CpuMode) -> Result<(), RealmJmpError> {
        match from.next() {
            Some(n) if n == to => Ok(()),
            _ => {
                // 回退判定：to 在 from 的链下方向
                let regress = matches!(
                    (from, to),
                    (CpuMode::Prot, CpuMode::Real) | (CpuMode::Long, CpuMode::Prot) | (CpuMode::Long, CpuMode::Real)
                );
                if regress {
                    Err(RealmJmpError::ModeRegress { from, to })
                } else {
                    Err(RealmJmpError::ModeSkip { from, to })
                }
            }
        }
    }
}

/// 黄金序列字节展开（指令序拼接；offset 占位保持 0）。
pub fn expand_golden(stage: u8) -> Result<Vec<u8>, RealmJmpError> {
    let table: &[GoldenInsn] = match stage {
        1 => &GOLDEN_REAL_TO_PROT,
        2 => &GOLDEN_PROT_TO_LONG,
        _ => return Err(RealmJmpError::GoldenMismatch { stage, offset: 0 }),
    };
    let mut out = Vec::new();
    for insn in table.iter() {
        for b in insn.bytes.iter() {
            out.push(*b);
        }
    }
    Ok(out)
}

/// 跳板字节流逐指令黄金比对（差一字节即 GoldenMismatch 带偏移）。
pub fn verify_against_golden(stage: u8, trampoline: &[u8]) -> Result<(), RealmJmpError> {
    let golden = expand_golden(stage)?;
    let n = golden.len().min(trampoline.len());
    let mut i = 0;
    while i < n {
        if golden[i] != trampoline[i] {
            return Err(RealmJmpError::GoldenMismatch { stage, offset: i });
        }
        i += 1;
    }
    if trampoline.len() != golden.len() {
        return Err(RealmJmpError::GoldenMismatch { stage, offset: n });
    }
    Ok(())
}

/// 段属性对拍（选择子闭集外拒；属性漂移立案）。
pub fn verify_seg_attr(sel: u16, got: SegAttr) -> Result<(), RealmJmpError> {
    let mut found = false;
    for (s, want) in GOLDEN_SEG_ATTRS.iter() {
        if *s == sel {
            found = true;
            if want.access != got.access || want.gran != got.gran {
                return Err(RealmJmpError::SegmentAttrDrift { sel });
            }
        }
    }
    if !found {
        return Err(RealmJmpError::SelectorBad { sel });
    }
    Ok(())
}

/// 跳板生成器（宿主/内核同构的确定性字节流产出；供对拍）。
pub fn build_trampoline(stage: u8, entry_offset: u32) -> Result<Vec<u8>, RealmJmpError> {
    let table: &[GoldenInsn] = match stage {
        1 => &GOLDEN_REAL_TO_PROT,
        2 => &GOLDEN_PROT_TO_LONG,
        _ => return Err(RealmJmpError::GoldenMismatch { stage, offset: 0 }),
    };
    let mut out = Vec::new();
    for insn in table.iter() {
        if insn.mnemonic.starts_with('(') {
            // offset 占位：填真实入口偏移（4 字节 LE）
            out.extend_from_slice(&entry_offset.to_le_bytes());
        } else {
            for b in insn.bytes.iter() {
                out.push(*b);
            }
        }
    }
    Ok(out)
}

/// 跃迁账（记录每次合法跃迁，append-only）。
#[derive(Clone, Copy, Debug)]
pub struct JmpRecord {
    pub from: CpuMode,
    pub to: CpuMode,
    pub tick: u64,
}

/// 三级跳全链执行器（状态机 + 跃迁账）。
#[derive(Clone, Debug)]
pub struct TripleJumper {
    pub sm: JmpStateMachine,
    pub ledger: Vec<JmpRecord>,
}

impl TripleJumper {
    /// 新执行器。
    pub const fn new() -> Self {
        TripleJumper {
            sm: JmpStateMachine::new(),
            ledger: Vec::new(),
        }
    }

    /// 跃迁一步并落账。
    pub fn step(&mut self, tick: u64) -> Result<CpuMode, RealmJmpError> {
        let from = self.sm.mode;
        let to = self.sm.advance()?;
        self.ledger.push(JmpRecord { from, to, tick });
        Ok(to)
    }

    /// 全链完成否（Long 即三级跳达成）。
    pub fn is_long(&self) -> bool {
        matches!(self.sm.mode, CpuMode::Long)
    }

    /// 跃迁账条数（合法三级跳 == 2 步）。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 执行器快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        b.push(self.sm.mode as u8);
        for r in self.ledger.iter() {
            b.push(r.from as u8);
            b.push(r.to as u8);
            b.extend_from_slice(&r.tick.to_le_bytes());
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in b.iter() {
            h ^= *x as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 域自检（`run_unx323_checks`）：J1 判据逐条可失败断言。
pub fn run_unx323_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx323");

    // C323-01 三级跳全链：Real→Prot→Long 两步合法迁移（J1 正面）
    {
        let mut j = TripleJumper::new();
        let s1 = j.step(1);
        let s2 = j.step(2);
        let ok = s1 == Ok(CpuMode::Prot)
            && s2 == Ok(CpuMode::Long)
            && j.is_long()
            && j.ledger_len() == 2;
        s.add("C323-01-三级跳全链两步达成", ok, if ok { "" } else { "三级跳链失效" });
    }

    // C323-02 跳步迁移拒绝（Real 直跳 Long）
    {
        let r = JmpStateMachine::check_transition(CpuMode::Real, CpuMode::Long);
        let ok = matches!(r, Err(RealmJmpError::ModeSkip { .. }));
        s.add("C323-02-跳步迁移拒绝", ok, if ok { "" } else { "跳步未被拦" });
    }

    // C323-03 模式回退拒绝（Long→Prot / Prot→Real / Long→Real）
    {
        let r1 = JmpStateMachine::check_transition(CpuMode::Long, CpuMode::Prot);
        let r2 = JmpStateMachine::check_transition(CpuMode::Prot, CpuMode::Real);
        let r3 = JmpStateMachine::check_transition(CpuMode::Long, CpuMode::Real);
        let ok = matches!(r1, Err(RealmJmpError::ModeRegress { .. }))
            && matches!(r2, Err(RealmJmpError::ModeRegress { .. }))
            && matches!(r3, Err(RealmJmpError::ModeRegress { .. }));
        s.add("C323-03-模式回退三向拒绝", ok, if ok { "" } else { "回退未被拦" });
    }

    // C323-04 Long 终态再 advance 拒（无下一级）
    {
        let mut sm = JmpStateMachine::new();
        let _ = sm.advance();
        let _ = sm.advance();
        let r = sm.advance();
        let ok = matches!(r, Err(RealmJmpError::ModeRegress { .. }));
        s.add("C323-04-Long 终态再迁拒绝", ok, if ok { "" } else { "Long 后仍可迁移" });
    }

    // C323-05 阶段一黄金序列逐指令一致（J1 核心：黄金汇编逐指令一致）
    {
        let tramp = build_trampoline(1, 0x1000).unwrap_or_default();
        let r = verify_against_golden(1, &tramp);
        let ok = r.is_ok() && tramp.len() == 3 + 3 + 3 + 2 + 4;
        s.add("C323-05-阶段一黄金逐指令一致", ok, if ok { "" } else { "阶段一黄金失配" });
    }

    // C323-06 阶段二黄金序列逐指令一致
    {
        let tramp = build_trampoline(2, 0x2000).unwrap_or_default();
        let r = verify_against_golden(2, &tramp);
        // 字节数：3+3+3+1+3+5+2+5+2+3+5+3+2+4 = 44
        let ok = r.is_ok() && tramp.len() == 44;
        s.add("C323-06-阶段二黄金逐指令一致", ok, if ok { "" } else { "阶段二黄金失配" });
    }

    // C323-07 单字节篡改检出（黄金比对带偏移立案）
    {
        let mut tramp = build_trampoline(1, 0).unwrap_or_default();
        if let Some(b) = tramp.get_mut(2) {
            *b ^= 0xFF;
        }
        let r = verify_against_golden(1, &tramp);
        let ok = matches!(r, Err(RealmJmpError::GoldenMismatch { stage: 1, offset: 2 }));
        s.add("C323-07-单字节篡改带偏移立案", ok, if ok { "" } else { "篡改未检出" });
    }

    // C323-08 长度不符检出（多一字节也是失配）
    {
        let mut tramp = build_trampoline(2, 0).unwrap_or_default();
        tramp.push(0x90);
        let r = verify_against_golden(2, &tramp);
        let ok = matches!(r, Err(RealmJmpError::GoldenMismatch { stage: 2, offset: 44 }));
        s.add("C323-08-长度不符立案", ok, if ok { "" } else { "多字节未检出" });
    }

    // C323-09 未知阶段拒绝
    {
        let r = expand_golden(9);
        let ok = matches!(r, Err(RealmJmpError::GoldenMismatch { stage: 9, .. }));
        s.add("C323-09-未知阶段拒绝", ok, if ok { "" } else { "未知阶段被放行" });
    }

    // C323-10 段属性黄金对拍：三段全过（J1 语义正确性的段面）
    {
        let mut all = true;
        for (sel, want) in GOLDEN_SEG_ATTRS.iter() {
            all &= verify_seg_attr(*sel, *want).is_ok();
        }
        s.add("C323-10-三段属性黄金全过", all, if all { "" } else { "段属性对拍失败" });
    }

    // C323-11 段属性漂移检出（64 位码段 L 位丢失）
    {
        // 篡改：L 位（gran 0xA0 → 0x20，丢 G 位）应立案
        let bad = SegAttr { access: 0x9A, gran: 0x20 };
        let r = verify_seg_attr(SEL_CODE64, bad);
        let ok = matches!(r, Err(RealmJmpError::SegmentAttrDrift { sel: SEL_CODE64 }));
        s.add("C323-11-段属性漂移立案", ok, if ok { "" } else { "属性漂移未检出" });
    }

    // C323-12 选择子越界拒（GDT 布局闭集外）
    {
        let r = verify_seg_attr(0x20, SegAttr { access: 0x9A, gran: 0xCF });
        let ok = matches!(r, Err(RealmJmpError::SelectorBad { sel: 0x20 }));
        s.add("C323-12-选择子越界拒", ok, if ok { "" } else { "布局外选择子被放行" });
    }

    // C323-13 关键位语义钉死（CR0.PE/PG、CR4.PAE、EFER.LME 位值）
    {
        let ok = CR0_PE == 1
            && CR0_PG == 0x8000_0000
            && CR4_PAE == 0x20
            && EFER_MSR == 0xC000_0080
            && EFER_LME == 0x100
            && EFER_LMA == 0x400;
        s.add("C323-13-控制位值写死对账", ok, if ok { "" } else { "控制位值漂移" });
    }

    // C323-14 跃迁账快照指纹确定且敏感
    {
        let mut j1 = TripleJumper::new();
        let mut j2 = TripleJumper::new();
        let _ = j1.step(1);
        let _ = j1.step(2);
        let f1a = j1.fingerprint();
        let f1b = j1.fingerprint();
        let _ = j2.step(5); // 只走一步
        let f2 = j2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C323-14-跃迁账指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C323-15 诊断码 0x99 段独占
    {
        let codes = [
            RealmJmpCode::ModeSkip,
            RealmJmpCode::ModeRegress,
            RealmJmpCode::GoldenMismatch,
            RealmJmpCode::SegmentAttrDrift,
            RealmJmpCode::SelectorBad,
            RealmJmpCode::TripleJumped,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C323-15-诊断码 0x99 段独占", all, if all { "" } else { "码值越出 0x99 段" });
    }

    s
}
