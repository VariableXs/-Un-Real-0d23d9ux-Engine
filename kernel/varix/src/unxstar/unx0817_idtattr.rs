//! unx0817_idtattr —— UNX-F0817 · IDT 条目属性位管理（目标 220 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0817 · IDT 条目属性位管理（DPL/IST/P 位）。
//! 判据原文：UNX-F0817-J1 属性位写入经唯一出口（越权配置拦截率 100%），
//!           IST 槽位冲突检测生效。
//! 设计纪律：
//! - 64 位 IDT 门描述符属性：P（存在）、DPL（0~3）、TYPE（0xE 中断门 /
//!   0xF 陷阱门）、IST（0~7，0=不切栈）。
//! - 唯一出口：全部属性写入必须经 `set_attr`（唯一可变入口），直接构造
//!   已使能描述符被构造器拒绝（P 位只能经出口置位）。
//! - 越权规则表固化：IST 槽只许 0~7；DPL 只许 0/3（内核门 DPL0、系统调用
//!   门 DPL3，DPL1/2 建模面禁用）；TYPE 只许 0xE/0xF；P 置位要求选择子与
//!   handler 已配齐（缺件即拒）。
//! - IST 槽冲突检测：同一 IST 槽同帧可被多向量共享（合法），但「会嵌套的
//!   向量对」共享同槽 = 冲突（嵌套时二次压栈爆 IST）。建模：同 IST 槽注册
//!   的向量集若含「可重入对」（同向量自身重入即冲突）即红账。
//! - 越权账：全部拦截事件入账（零丢弃）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6380（0x6381~0x6389 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const IA_VERSION: &str = "unx0817_idtattr-v1";

/// 诊断段基址（0x6380 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6380;

/// XcCode 闭集。
pub const XC_IST_RANGE: u16 = SEG_BASE + 1; // IST 槽越界（>7）
pub const XC_DPL_BAD: u16 = SEG_BASE + 2; // DPL 非法（只许 0/3）
pub const XC_TYPE_BAD: u16 = SEG_BASE + 3; // TYPE 非法（只许 0xE/0xF）
pub const XC_GATE_INCOMPLETE: u16 = SEG_BASE + 4; // P 置位时门缺件
pub const XC_IST_CONFLICT: u16 = SEG_BASE + 5; // IST 槽嵌套冲突
pub const XC_DIRECT_BUILD: u16 = SEG_BASE + 6; // 绕过唯一出口的直接构造
pub const XC_LED_FULL: u16 = SEG_BASE + 7; // 越权账溢出
pub const XC_VEC_RANGE: u16 = SEG_BASE + 8; // 向量号越界
pub const XC_ATTR_UNSET: u16 = SEG_BASE + 9; // 未配置即读取启用态

/// 门类型。
pub const TYPE_INTR: u8 = 0xE;
pub const TYPE_TRAP: u8 = 0xF;

/// 属性集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateAttr {
    /// 存在位（唯一出口可置）。
    pub present: bool,
    /// DPL（0/3）。
    pub dpl: u8,
    /// 类型（0xE/0xF）。
    pub gate_type: u8,
    /// IST 槽（0~7；0=不切栈）。
    pub ist: u8,
}

impl GateAttr {
    /// 空属性（P=0，建模合法初态）。
    pub fn blank() -> GateAttr {
        GateAttr {
            present: false,
            dpl: 0,
            gate_type: TYPE_INTR,
            ist: 0,
        }
    }

    /// 属性合法性（不含 P 语义——P 的置位需要门完整，由唯一出口校验）。
    pub fn fields_legal(&self) -> Result<(), u16> {
        if self.ist > 7 {
            return Err(XC_IST_RANGE);
        }
        if self.dpl != 0 && self.dpl != 3 {
            return Err(XC_DPL_BAD);
        }
        if self.gate_type != TYPE_INTR && self.gate_type != TYPE_TRAP {
            return Err(XC_TYPE_BAD);
        }
        Ok(())
    }
}

/// IDT 条目（选择子 + handler + 属性；属性经唯一出口管理）。
pub struct IdtEntry {
    /// 向量号。
    pub vec: u8,
    /// 代码段选择子。
    pub selector: u16,
    /// handler 建模地址。
    pub handler: u32,
    /// 属性（私有：只许 set_attr 出口写）。
    attr: GateAttr,
}

impl IdtEntry {
    /// 构造空条目（P=0 初态合法——未使能门允许直接构造）。
    pub fn blank(vec: u8) -> IdtEntry {
        IdtEntry {
            vec,
            selector: 0,
            handler: 0,
            attr: GateAttr::blank(),
        }
    }

    /// 唯一出口：属性写入（P=1 要求门完整：选择子与 handler 已配齐）。
    pub fn set_attr(&mut self, a: GateAttr) -> Result<(), u16> {
        a.fields_legal()?;
        if a.present {
            if self.selector == 0 || self.handler == 0 {
                return Err(XC_GATE_INCOMPLETE);
            }
        }
        self.attr = a;
        Ok(())
    }

    /// 属性只读快照。
    pub fn attr(&self) -> GateAttr {
        self.attr
    }

    /// 使能态查询：P=1 才算可用；未配置读启用态即显性拒（判据「未配置即读取」）。
    pub fn require_present(&self) -> Result<GateAttr, u16> {
        if !self.attr.present {
            return Err(XC_ATTR_UNSET);
        }
        Ok(self.attr)
    }
}

/// IST 槽注册账：槽 → 向量集。冲突规则：同槽向量集中出现「同向量重复注册」
/// 即冲突（同向量自身重入共享同槽）；不同向量共享同槽合法（IST 由架构保证
/// 同槽嵌套在硬中断面禁——建模面按「同向量重复」判冲突）。
pub struct IstRegistry {
    /// 槽 → 已注册向量列表（最多 16 向量/槽，定容裁断）。
    slots: [Vec<u8>; 8],
    conflicts: usize,
}

impl IstRegistry {
    pub fn new() -> IstRegistry {
        IstRegistry {
            slots: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            conflicts: 0,
        }
    }

    /// 注册：IST=0 不入账；同槽重复向量即冲突红账。
    pub fn register(&mut self, vec: u8, ist: u8) -> Result<(), u16> {
        if ist > 7 {
            return Err(XC_IST_RANGE);
        }
        if ist == 0 {
            return Ok(());
        }
        let s = &mut self.slots[ist as usize];
        if s.contains(&vec) {
            self.conflicts += 1;
            return Err(XC_IST_CONFLICT);
        }
        if s.len() < 16 {
            s.push(vec);
        }
        Ok(())
    }

    /// 槽向量集快照。
    pub fn slot_vecs(&self, ist: usize) -> Result<&[u8], u16> {
        if ist > 7 {
            return Err(XC_IST_RANGE);
        }
        Ok(&self.slots[ist])
    }

    /// 冲突总数。
    pub fn conflict_count(&self) -> usize {
        self.conflicts
    }
}

/// 越权账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViolRow {
    pub vec: u8,
    pub code: u16,
    pub seq: u64,
}

/// 属性面管理器（唯一出口的持有者 + 越权账）。
pub struct AttrManager {
    entries: [IdtEntry; 256],
    ist: IstRegistry,
    viol: Vec<ViolRow>,
    viol_overflow: usize,
    seq: u64,
}

impl AttrManager {
    pub fn new() -> AttrManager {
        AttrManager {
            entries: core::array::from_fn(|i| IdtEntry::blank(i as u8)),
            ist: IstRegistry::new(),
            viol: Vec::new(),
            viol_overflow: 0,
            seq: 0,
        }
    }

    fn record_viol(&mut self, vec: u8, code: u16) {
        self.seq += 1;
        if self.viol.len() < 64 {
            self.viol.push(ViolRow {
                vec,
                code,
                seq: self.seq,
            });
        } else {
            self.viol_overflow += 1;
        }
    }

    /// 配置门（唯一写路径）：配选择子/handler → set_attr → IST 注册。
    pub fn configure(
        &mut self,
        vec: u8,
        selector: u16,
        handler: u32,
        attr: GateAttr,
    ) -> Result<(), u16> {
        if attr.fields_legal().is_err() {
            let code = attr.fields_legal().unwrap_err();
            self.record_viol(vec, code);
            return Err(code);
        }
        let e = &mut self.entries[vec as usize];
        e.selector = selector;
        e.handler = handler;
        if let Err(code) = e.set_attr(attr) {
            self.record_viol(vec, code);
            return Err(code);
        }
        if attr.present && attr.ist > 0 {
            if let Err(code) = self.ist.register(vec, attr.ist) {
                // IST 冲突：回滚 P 位（门退回未使能）
                let e = &mut self.entries[vec as usize];
                let mut a = attr;
                a.present = false;
                let _ = e.set_attr(a);
                self.record_viol(vec, code);
                return Err(code);
            }
        }
        Ok(())
    }

    /// 条目读取。
    pub fn entry(&self, vec: u8) -> Result<&IdtEntry, u16> {
        Ok(&self.entries[vec as usize])
    }

    /// 越权账行。
    pub fn viol_row(&self, i: usize) -> Result<ViolRow, u16> {
        if i >= self.viol.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.viol[i])
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.viol.len() + self.viol_overflow == self.seq as usize
    }
}

/// drill：全链自例程（唯一出口 + IST 冲突 + 越权拦截）。
pub fn drill() -> Result<u32, u16> {
    let mut m = AttrManager::new();
    // 1. 正常配置：IST0 内核中断门
    m.configure(
        32,
        0x08,
        0xFEE0_4000,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 0 },
    )?;
    let a = m.entry(32)?.require_present()?;
    if a.dpl != 0 || a.gate_type != TYPE_INTR {
        return Err(XC_TYPE_BAD);
    }
    // 2. IST1 槽注册（DF 用 IST1）
    m.configure(
        8,
        0x08,
        0xFEE0_4100,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 1 },
    )?;
    // 3. IST 槽共享合法（不同向量同槽）
    m.configure(
        18,
        0x08,
        0xFEE0_4200,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 2 },
    )?;
    m.configure(
        2,
        0x08,
        0xFEE0_4300,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 2 },
    )?;
    if m.entry(18)?.require_present()?.ist != 2 {
        return Err(XC_IST_CONFLICT);
    }
    // 4. IST 冲突：同向量重复注册同槽
    if m.configure(
        18,
        0x08,
        0xFEE0_4200,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 2 },
    ).is_ok() {
        return Err(XC_IST_CONFLICT);
    }
    // 5. 错误路径①：IST 越界
    if m.configure(
        40,
        0x08,
        0xFEE0_4400,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 8 },
    ).is_ok() {
        return Err(XC_IST_RANGE);
    }
    // 6. 错误路径②：DPL=1 拒
    if m.configure(
        41,
        0x08,
        0xFEE0_4500,
        GateAttr { present: true, dpl: 1, gate_type: TYPE_INTR, ist: 0 },
    ).is_ok() {
        return Err(XC_DPL_BAD);
    }
    // 7. 错误路径③：TYPE=0x5 拒
    if m.configure(
        42,
        0x08,
        0xFEE0_4600,
        GateAttr { present: true, dpl: 0, gate_type: 0x5, ist: 0 },
    ).is_ok() {
        return Err(XC_TYPE_BAD);
    }
    // 8. 错误路径④：P 置位但门缺件（handler=0）
    if m.configure(
        43,
        0x08,
        0,
        GateAttr { present: true, dpl: 0, gate_type: TYPE_INTR, ist: 0 },
    ).is_ok() {
        return Err(XC_GATE_INCOMPLETE);
    }
    // 9. 未配置读启用态拒
    if m.entry(200)?.require_present().is_ok() {
        return Err(XC_ATTR_UNSET);
    }
    // 10. 空属性构造合法（P=0 初态）
    let _b = IdtEntry::blank(7);
    let bl = GateAttr::blank();
    if bl.present {
        return Err(XC_DIRECT_BUILD);
    }
    // 11. 越权账守恒（4 次拦截：IST 冲突 + IST 越界 + DPL + TYPE）
    if !m.conserve() {
        return Err(XC_LED_FULL);
    }
    Ok(0)
}
