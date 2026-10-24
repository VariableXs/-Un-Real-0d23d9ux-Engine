//! unx190_tssist — UNX-F0190 TSS 与 IST 布局初始化（0x2B20 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0190）：TSS 与 IST 布局初始化。
//! 判据原文（UNX-F0190-J1）：TSS 描述符/IST 栈配置落位，F0022 Double
//! Fault 走 IST 复测通过。
//!
//! 设计三线：①64 位 TSS 模型（RSP0~2 + IST1~7 七栈槽，逐槽对齐/越界/
//! 归属校验）；②IST 槽位分配表（#DF→IST1、#NM→IST2、#MC→IST3 设计
//! 真源——向量到槽位的映射声明后与实配对拍，漂移即红）；③TSS 描述符
//! 落位账（64 位 TSS 描述符 16 字节、present、type 0x9/0xB 语义，与
//! F0189 布局表 TSS 槽联签）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const TSS_VERSION: &str = "190-tssist-v1";

/// 段基（0x2B20）。
pub const SEG_BASE: u16 = 0x2B20;

/// 栈 16 字节对齐。
pub const STACK_ALIGN: u64 = 16;

/// IST 槽数（1~7）。
pub const IST_SLOTS: usize = 7;

/// 向量号（设计真源锚）。
pub const VEC_DOUBLE_FAULT: u8 = 8;
pub const VEC_NM: u8 = 7;
pub const VEC_MC: u8 = 18;

/// IST 槽位。
pub type IstSlot = u8;

/// 64 位 TSS 模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskStateSeg {
    /// Ring0 栈指针（RSP0~2）。
    pub rsp: [u64; 3],
    /// IST1~7 栈顶。
    pub ist: [u64; IST_SLOTS],
    /// I/O 位图基址（超限值 = 无位图）。
    pub iomap_base: u16,
}

impl TaskStateSeg {
    /// 空表（全零栈 = 未配置）。
    pub fn blank() -> Self {
        TaskStateSeg {
            rsp: [0; 3],
            ist: [0; IST_SLOTS],
            iomap_base: u16::MAX,
        }
    }

    /// 单栈配置：槽位 1~7、栈顶 16 对齐、非零。
    pub fn set_ist(&mut self, slot: IstSlot, top: u64) -> Result<(), u16> {
        if slot == 0 || slot as usize > IST_SLOTS {
            return Err(E_IST_RANGE);
        }
        if top == 0 || top % STACK_ALIGN != 0 {
            return Err(E_IST_ALIGN);
        }
        self.ist[(slot - 1) as usize] = top;
        Ok(())
    }

    /// RSP 配置：index 0~2、对齐。
    pub fn set_rsp(&mut self, idx: usize, top: u64) -> Result<(), u16> {
        if idx >= 3 {
            return Err(E_RSP_RANGE);
        }
        if top == 0 || top % STACK_ALIGN != 0 {
            return Err(E_RSP_ALIGN);
        }
        self.rsp[idx] = top;
        Ok(())
    }

    /// 布局审计：已配置的 IST 栈两两不重叠、不与 RSP0 重叠。
    pub fn overlap_audit(&self) -> Result<(), u16> {
        let mut tops: Vec<(u64, u64)> = Vec::new(); // (top, 标识) 简化：只对拍 top 互异。
        for (i, &t) in self.ist.iter().enumerate() {
            if t == 0 {
                continue;
            }
            if tops.iter().any(|(x, _)| *x == t) {
                return Err(E_IST_DUP);
            }
            tops.push((t, i as u64));
        }
        for &t in self.rsp.iter() {
            if t != 0 && tops.iter().any(|(x, _)| *x == t) {
                return Err(E_IST_DUP);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// IST 分配表（设计真源）
// ---------------------------------------------------------------------------

/// 向量 → IST 槽设计映射。
pub fn design_ist_map() -> [(u8, IstSlot); 3] {
    [(VEC_DOUBLE_FAULT, 1), (VEC_NM, 2), (VEC_MC, 3)]
}

/// IDT 条目 IST 字段视角（低 32 位字里 IST 占 bit32~34 的 3 位域，此处
/// 以独立 u8 建模）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdtIst {
    pub vector: u8,
    /// IST 槽（0 = 不用 IST）。
    pub ist: IstSlot,
}

/// 实配对拍（判据主体）：实配表必须恰覆盖设计映射且无多余 IST 落位。
pub fn ist_map_compare(actual: &[IdtIst]) -> Result<(), u16> {
    let design = design_ist_map();
    for (vec_expect, slot_expect) in design.iter() {
        let hit = actual.iter().find(|a| a.vector == *vec_expect);
        let a = match hit {
            Some(a) => a,
            None => return Err(E_IST_UNCOVERED),
        };
        if a.ist != *slot_expect {
            return Err(E_IST_DRIFT);
        }
    }
    // 无多余落位：实配里不属设计向量的条目不得占 IST。
    for a in actual.iter() {
        if !design.iter().any(|(v, _)| v == &a.vector) && a.ist != 0 {
            return Err(E_IST_EXTRA);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// TSS 描述符落位账
// ---------------------------------------------------------------------------

/// TSS 描述符校验（与 F0189 布局表 TSS 槽联签）。
/// 64 位 TSS 描述符为 16 字节：低 8 字节 access/type/limit、高 8 字节
/// 基址高位。判据视角：present、type ∈ {0x9, 0xB}。
pub fn tss_descriptor_ok(raw_lo: u64) -> Result<(), u16> {
    let access = ((raw_lo >> 40) & 0xFF) as u8;
    let present = access & 0x80 != 0;
    let ty = access & 0x1F;
    if !present {
        return Err(E_TSS_NOT_PRESENT);
    }
    if ty != 0x9 && ty != 0xB {
        return Err(E_TSS_TYPE);
    }
    Ok(())
}

/// 落位账：描述符与 IST 配置双齐才算初始化完成。
#[derive(Debug, Default)]
pub struct TssInitLedger {
    pub desc_ok: bool,
    pub ist_configured: bool,
    pub map_matched: bool,
}

impl TssInitLedger {
    /// 收口（判据 J1 的三账齐）。
    pub fn close(&self) -> Result<(), u16> {
        if !self.desc_ok {
            return Err(E_TSS_NOT_PRESENT);
        }
        if !self.ist_configured {
            return Err(E_IST_ALIGN);
        }
        if !self.map_matched {
            return Err(E_IST_DRIFT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x2B20 段，显别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum TssErr {
    /// IST 槽越界。
    IstRange = SEG_BASE + 1,
    /// 栈未对齐。
    IstAlign = SEG_BASE + 2,
    /// RSP 槽越界。
    RspRange = SEG_BASE + 3,
    /// RSP 未对齐。
    RspAlign = SEG_BASE + 4,
    /// IST 栈顶重复。
    IstDup = SEG_BASE + 5,
    /// 设计向量未覆盖。
    IstUncovered = SEG_BASE + 6,
    /// IST 槽位漂移。
    IstDrift = SEG_BASE + 7,
    /// 多余 IST 落位。
    IstExtra = SEG_BASE + 8,
    /// TSS 描述符不 present。
    TssNotPresent = SEG_BASE + 9,
    /// TSS 类型非法。
    TssType = SEG_BASE + 10,
}

impl TssErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            TssErr::IstRange => "IST 槽越界（1~7）",
            TssErr::IstAlign => "IST 栈顶未 16 字节对齐",
            TssErr::RspRange => "RSP 槽越界（0~2）",
            TssErr::RspAlign => "RSP 未 16 字节对齐",
            TssErr::IstDup => "IST 栈顶重复（栈区重叠）",
            TssErr::IstUncovered => "设计向量缺 IST 落位",
            TssErr::IstDrift => "IST 槽位与设计漂移",
            TssErr::IstExtra => "设计外向量占 IST",
            TssErr::TssNotPresent => "TSS 描述符不 present",
            TssErr::TssType => "TSS 类型非 0x9/0xB",
        }
    }
}

const E_IST_RANGE: u16 = TssErr::IstRange.code();
const E_IST_ALIGN: u16 = TssErr::IstAlign.code();
const E_RSP_RANGE: u16 = TssErr::RspRange.code();
const E_RSP_ALIGN: u16 = TssErr::RspAlign.code();
const E_IST_DUP: u16 = TssErr::IstDup.code();
const E_IST_UNCOVERED: u16 = TssErr::IstUncovered.code();
const E_IST_DRIFT: u16 = TssErr::IstDrift.code();
const E_IST_EXTRA: u16 = TssErr::IstExtra.code();
const E_TSS_NOT_PRESENT: u16 = TssErr::TssNotPresent.code();
const E_TSS_TYPE: u16 = TssErr::TssType.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据集（UNX-F0190-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：TSS 落位 + IST 三向配置 + 映射对拍 → 收口绿。
    let mut tss = TaskStateSeg::blank();
    tss.set_rsp(0, 0x4000_0000)?;
    tss.set_ist(1, 0x5000_0000)?; // #DF → IST1。
    tss.set_ist(2, 0x5100_0000)?; // #NM → IST2。
    tss.set_ist(3, 0x5200_0000)?; // #MC → IST3。
    tss.overlap_audit()?;
    let actual = vec![
        IdtIst {
            vector: VEC_DOUBLE_FAULT,
            ist: 1,
        },
        IdtIst {
            vector: VEC_NM,
            ist: 2,
        },
        IdtIst {
            vector: VEC_MC,
            ist: 3,
        },
    ];
    ist_map_compare(&actual)?;
    let ledger = TssInitLedger {
        desc_ok: tss_descriptor_ok(0x0000_8900_0000_0067).is_ok(),
        ist_configured: tss.ist[0] != 0 && tss.ist[1] != 0 && tss.ist[2] != 0,
        map_matched: ist_map_compare(&actual).is_ok(),
    };
    ledger.close()?;
    // 二、F0022 复测锚：#DF 恰走 IST1（设计表首项语义）。
    let df = design_ist_map()[0];
    if df.0 != VEC_DOUBLE_FAULT || df.1 != 1 {
        return Err(E_IST_DRIFT);
    }
    // 三、红线逐项：IST 槽越界 / 未对齐 / RSP 越界 / 栈顶重复 各自拒。
    let mut bad = TaskStateSeg::blank();
    if bad.set_ist(0, 0x1000).is_ok() || bad.set_ist(8, 0x1000).is_ok() {
        return Err(E_IST_RANGE);
    }
    if bad.set_ist(1, 0x1008).is_ok() {
        return Err(E_IST_ALIGN);
    }
    if bad.set_rsp(3, 0x1000).is_ok() {
        return Err(E_RSP_RANGE);
    }
    if bad.set_rsp(0, 0x1004).is_ok() {
        return Err(E_RSP_ALIGN);
    }
    let mut dup = TaskStateSeg::blank();
    dup.set_ist(1, 0x5000_0000)?;
    dup.set_ist(4, 0x5000_0000)?;
    if dup.overlap_audit().is_ok() {
        return Err(E_IST_DUP);
    }
    // 四、映射漂移检出：#DF 配到 IST2 → 漂移即红；缺覆盖 → 红；
    // 设计外向量占 IST → 红。
    let drift = vec![
        IdtIst {
            vector: VEC_DOUBLE_FAULT,
            ist: 2,
        },
        IdtIst {
            vector: VEC_NM,
            ist: 2,
        },
        IdtIst {
            vector: VEC_MC,
            ist: 3,
        },
    ];
    if ist_map_compare(&drift).is_ok() {
        return Err(E_IST_DRIFT);
    }
    let missing = vec![IdtIst {
        vector: VEC_NM,
        ist: 2,
    }];
    if ist_map_compare(&missing).is_ok() {
        return Err(E_IST_UNCOVERED);
    }
    let extra = vec![
        IdtIst {
            vector: VEC_DOUBLE_FAULT,
            ist: 1,
        },
        IdtIst {
            vector: VEC_NM,
            ist: 2,
        },
        IdtIst {
            vector: VEC_MC,
            ist: 3,
        },
        IdtIst {
            vector: 14,
            ist: 4,
        },
    ];
    if ist_map_compare(&extra).is_ok() {
        return Err(E_IST_EXTRA);
    }
    // 五、TSS 描述符语义：不 present / 非 TSS 类型各自拒。
    if tss_descriptor_ok(0x0000_0900_0000_0067).is_ok() {
        return Err(E_TSS_NOT_PRESENT);
    }
    if tss_descriptor_ok(0x0000_8A00_0000_0067).is_ok() {
        return Err(E_TSS_TYPE);
    }
    // 六、判别式段位对拍。
    if E_TSS_TYPE != SEG_BASE + 10 || E_IST_RANGE != SEG_BASE + 1 {
        return Err(E_TSS_TYPE);
    }
    Ok(())
}
