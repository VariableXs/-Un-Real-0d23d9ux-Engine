//! unx0871_msiicap —— UNX-F0871 · MSI/MSI-X 能力结构解析底座（PCI 配置空间消费）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0871 · MSI/MSI-X 能力结构解析底座。
//! 判据原文摘录（卡面验收行）：MSI/MSI-X 能力结构解析底座（PCI 配置空间消费）
//!           ——能力链逐项消费、结构字段逐位解码、伪/截断结构全拦截。
//! 诊断段 0x7860（本单独占，0x7861~0x786F 闭集）。
//!
//! 设计纪律：
//! - PCI 配置空间以 256 字节（64 个 u32 槽）建模；能力链自 CAP_PTR（0x34）
//!   起步，按 `ptr & 0xFC` 四字节对齐步进，至 0x00 终止。
//! - MSI 能力（CapID 0x05）：MSG_CTRL[15:8]，MSG_ADDR 0x04/0x08（64 位面），
//!   MSG_DATA（0x0C 或 0x0C+0x04），槽位扩展掩码 MAX_MSI（CTRL[10:8]×2^1）。
//! - MSI-X 能力（CapID 0x11）：MSG_CTRL[15:9]（表容量 = 掩码+1，上限 2048），
//!   TABLE_OFF/BIR（0x04）低 30 位偏移 + 低 2 位 BIR，PBA_OFF/BIR（0x08）。
//! - 拦截面：指针环（同指针二次访问）、越界（ptr+结构长 > 256）、ID 非法
//!   （非 0x05/0x11 不入 MSI 账）、MSI-X 表容量越 2048、BIR > 5。
//! - 消费账：每能力一行（零丢弃），解析序列号守恒：消费数 = 成功数 + 拦截数。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const MC_VERSION: &str = "unx0871_msiicap-v1";

/// 诊断段基址（0x7860 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7860;

/// XcCode 闭集（0x7861~0x786F）。
pub const XC_PTR_LOOP: u16 = SEG_BASE + 1; // 能力链指针环
pub const XC_PTR_RANGE: u16 = SEG_BASE + 2; // 指针越界（≥256 或未对齐）
pub const XC_STRUCT_SHORT: u16 = SEG_BASE + 3; // 结构跨出配置空间（截断）
pub const XC_ID_BAD: u16 = SEG_BASE + 4; // 能力 ID 不属 MSI 族
pub const XC_MSIX_CAP: u16 = SEG_BASE + 5; // MSI-X 表容量越 2048
pub const XC_BIR_BAD: u16 = SEG_BASE + 6; // BIR 越界（>5）
pub const XC_CTRL_RSVD: u16 = SEG_BASE + 7; // 控制字保留位非零
pub const XC_LED_FULL: u16 = SEG_BASE + 8; // 消费账溢出
pub const XC_CONSERVE: u16 = SEG_BASE + 9; // 守恒破

/// 配置空间槽数（256 字节 / 4）。
pub const CFG_SLOTS: usize = 64;
/// 能力链起点寄存器偏移（字节）。
pub const REG_CAP_PTR: usize = 0x34;
/// MSI-X 表容量上限（SDM：2048 项）。
pub const MSIX_CAP_MAX: usize = 2048;

/// 能力类型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapKind {
    /// MSI（CapID 0x05）。
    Msi,
    /// MSI-X（CapID 0x11）。
    Msix,
}

/// MSI 能力解码行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsiCap {
    /// 能力链偏移（字节）。
    pub offset: u8,
    /// 消息控制字 MSG_CTRL。
    pub ctrl: u16,
    /// 消息地址（低半）。
    pub addr_lo: u32,
    /// 消息地址（高半；32 位面为 0）。
    pub addr_hi: u32,
    /// 消息数据。
    pub data: u32,
    /// 是否 64 位地址面（CTRL[7]）。
    pub addr64: bool,
    /// 是否带每向量掩码位（CTRL[8]）。
    pub per_vec_mask: bool,
}

impl MsiCap {
    /// 多消息请求数 = 2^MME（CTRL[6:4]）。
    pub fn requested(&self) -> usize {
        1usize << ((self.ctrl >> 4) & 0x7)
    }
    /// 槽位扩展上限 = 2^MAX_MSI（CTRL[10:8]）。
    pub fn slots(&self) -> usize {
        1usize << ((self.ctrl >> 8) & 0x7)
    }
    /// 控制字合法：保留位 [15:11] 为零。
    pub fn ctrl_legal(&self) -> bool {
        self.ctrl & 0xF800 == 0
    }
}

/// MSI-X 能力解码行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsixCap {
    /// 能力链偏移（字节）。
    pub offset: u8,
    /// 消息控制字 MSG_CTRL。
    pub ctrl: u16,
    /// 表偏移（低 30 位，四字节对齐）。
    pub table_off: u32,
    /// 表 BIR（BAR 索引 0~5）。
    pub table_bir: u8,
    /// PBA 偏移。
    pub pba_off: u32,
    /// PBA BIR。
    pub pba_bir: u8,
}

impl MsixCap {
    /// 表容量 = 表大小掩码（CTRL[14:9]）+ 1。
    pub fn table_size(&self) -> usize {
        ((self.ctrl >> 9) & 0x7FF) as usize + 1
    }
    /// 控制字合法：保留位 [15] 与 [8:2] 为零。
    pub fn ctrl_legal(&self) -> bool {
        self.ctrl & 0x80FC == 0
    }
}

/// 消费账行（逐能力，零丢弃）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapRow {
    /// MSI 行。
    Msi(MsiCap),
    /// MSI-X 行。
    Msix(MsixCap),
}

/// 能力链解析器：PCI 配置空间消费底座。
pub struct MsiCapWalker {
    rows: Vec<CapRow>,
    /// 拦截账：(偏移, 码, 序号)。
    intercepts: Vec<(u8, u16, u64)>,
    seq: u64,
    intercept_overflow: usize,
    /// 已访问指针账（防环）。
    seen: Vec<u8>,
}

impl MsiCapWalker {
    pub fn new() -> MsiCapWalker {
        MsiCapWalker {
            rows: Vec::new(),
            intercepts: Vec::new(),
            seq: 0,
            intercept_overflow: 0,
            seen: Vec::new(),
        }
    }

    fn record(&mut self, off: u8, code: u16) {
        self.seq += 1;
        if self.intercepts.len() < 64 {
            self.intercepts.push((off, code, self.seq));
        } else {
            self.intercept_overflow += 1;
        }
    }

    /// 从配置空间（u32 槽序）走能力链并逐项解码。
    /// 返回成功入账行数。链上任一环/越界即整链止步（该点起拦截）。
    pub fn walk(&mut self, cfg: &[u32; CFG_SLOTS]) -> Result<usize, u16> {
        let byte = |off: usize| -> u8 {
            let w = cfg[off / 4];
            ((w >> ((off % 4) * 8)) & 0xFF) as u8
        };
        let word = |off: usize| -> u32 { cfg[off / 4] };
        let mut ptr = byte(REG_CAP_PTR) as usize & 0xFC;
        let mut ok = 0usize;
        loop {
            // 1. 终止
            if ptr == 0 {
                break;
            }
            // 2. 对齐与越界首查
            if ptr >= 256 || ptr % 4 != 0 {
                self.record(ptr as u8, XC_PTR_RANGE);
                break;
            }
            // 3. 防环：同指针二次访问即环
            if self.seen.iter().any(|s| *s as usize == ptr) {
                self.record(ptr as u8, XC_PTR_LOOP);
                break;
            }
            self.seen.push(ptr as u8);
            // 4. 解码头字：ID[7:0] NEXT[15:8]
            let id = byte(ptr);
            let next = byte(ptr + 1) as usize & 0xFC;
            match id {
                0x05 => {
                    // MSI：结构长 = 0x0A / 0x0C / 0x0D / 0x14 按 64 位与掩码位
                    if ptr + 0x10 > 256 {
                        self.record(ptr as u8, XC_STRUCT_SHORT);
                        break;
                    }
                    let ctrl = ((word(ptr + 2) >> (((ptr + 2) % 4) * 8)) & 0xFFFF) as u16;
                    // 简化：ctrl 取 ptr+2 起双字节（槽内偏移模型）
                    let cap = MsiCap {
                        offset: ptr as u8,
                        ctrl,
                        addr_lo: word(ptr + 4),
                        addr_hi: word(ptr + 8),
                        data: word(ptr + 12),
                        addr64: ctrl & 0x80 != 0,
                        per_vec_mask: ctrl & 0x100 != 0,
                    };
                    if !cap.ctrl_legal() {
                        self.record(ptr as u8, XC_CTRL_RSVD);
                        break;
                    }
                    self.rows.push(CapRow::Msi(cap));
                    ok += 1;
                }
                0x11 => {
                    // MSI-X：结构定长 0x10
                    if ptr + 0x10 > 256 {
                        self.record(ptr as u8, XC_STRUCT_SHORT);
                        break;
                    }
                    let ctrl = ((word(ptr + 2) >> (((ptr + 2) % 4) * 8)) & 0xFFFF) as u16;
                    let t = word(ptr + 4);
                    let p = word(ptr + 8);
                    let cap = MsixCap {
                        offset: ptr as u8,
                        ctrl,
                        table_off: t & 0xFFFF_FFF8,
                        table_bir: (t & 0x7) as u8,
                        pba_off: p & 0xFFFF_FFF8,
                        pba_bir: (p & 0x7) as u8,
                    };
                    if cap.table_size() > MSIX_CAP_MAX {
                        self.record(ptr as u8, XC_MSIX_CAP);
                        break;
                    }
                    if !cap.ctrl_legal() {
                        self.record(ptr as u8, XC_CTRL_RSVD);
                        break;
                    }
                    if cap.table_bir > 5 || cap.pba_bir > 5 {
                        self.record(ptr as u8, XC_BIR_BAD);
                        break;
                    }
                    self.rows.push(CapRow::Msix(cap));
                    ok += 1;
                }
                _ => {
                    // 非 MSI 族：不入账，仅拦截记名（底座只消费 MSI 族）
                    self.record(ptr as u8, XC_ID_BAD);
                }
            }
            ptr = next;
        }
        Ok(ok)
    }

    /// 消费账行读取。
    pub fn row(&self, i: usize) -> Result<&CapRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(&self.rows[i])
    }

    /// 拦截账行读取。
    pub fn intercept(&self, i: usize) -> Result<(u8, u16, u64), u16> {
        if i >= self.intercepts.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.intercepts[i])
    }

    /// MSI 行数。
    pub fn msi_count(&self) -> usize {
        self.rows.iter().filter(|r| match r {
            CapRow::Msi(_) => true,
            _ => false,
        }).count()
    }

    /// MSI-X 行数。
    pub fn msix_count(&self) -> usize {
        self.rows.iter().filter(|r| match r {
            CapRow::Msix(_) => true,
            _ => false,
        }).count()
    }

    /// 守恒：已访问节点数 = 成功入账 + 节点级拦截（ID 非法/保留位）。
    /// 指针级拦截（环/越界）不计入节点（该指针未入访问账）。
    pub fn consume_conserve(&self) -> bool {
        let node_rej = self
            .intercepts
            .iter()
            .filter(|(_, code, _)| *code == XC_ID_BAD || *code == XC_CTRL_RSVD)
            .count();
        self.seen.len() == self.rows.len() + node_rej
    }

    /// 拦截账守恒（拦截总发 = 在账 + 溢出）。
    pub fn intercept_conserve(&self) -> bool {
        self.intercepts.len() + self.intercept_overflow == self.seq as usize
    }
}

/// 构造一张含 MSI + MSI-X 能力链的样例配置空间（测试面自足）。
pub fn sample_cfg(msi_ptr: u8, msix_ptr: u8, next_of_msi: u8) -> [u32; CFG_SLOTS] {
    let mut cfg = [0u32; CFG_SLOTS];
    let put = |cfg: &mut [u32; CFG_SLOTS], off: usize, v: u32| {
        cfg[off / 4] |= v << ((off % 4) * 8);
    };
    put(&mut cfg, REG_CAP_PTR, msi_ptr as u32);
    // MSI @msi_ptr：ID=0x05 NEXT=next_of_msi CTRL=0x0081（1 槽 64 位）
    put(&mut cfg, msi_ptr as usize, 0x05);
    put(&mut cfg, msi_ptr as usize + 1, next_of_msi as u32);
    put(&mut cfg, msi_ptr as usize + 2, 0x81);
    put(&mut cfg, msi_ptr as usize + 4, 0xFEE0_0000);
    put(&mut cfg, msi_ptr as usize + 12, 0x41);
    // MSI-X @msix_ptr：ID=0x11 NEXT=0 CTRL=0x0402（表掩码 2 → 3 项）
    put(&mut cfg, msix_ptr as usize, 0x11);
    put(&mut cfg, msix_ptr as usize + 2, 0x0402);
    put(&mut cfg, msix_ptr as usize + 4, 0x0000_0002);
    put(&mut cfg, msix_ptr as usize + 8, 0x0000_1002);
    cfg
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 正常链：MSI(0x50)→MSI-X(0x70)→终
    let mut w = MsiCapWalker::new();
    let cfg = sample_cfg(0x50, 0x70, 0x70);
    let ok = w.walk(&cfg)?;
    if ok != 2 || w.msi_count() != 1 || w.msix_count() != 1 {
        return Err(XC_CONSERVE);
    }
    // 2. MSI-X 表容量对拍：CTRL 表掩码=2 → 3 项
    match w.row(1)? {
        CapRow::Msix(c) => {
            if c.table_size() != 3 || c.table_bir != 2 || c.pba_bir != 2 {
                return Err(XC_MSIX_CAP);
            }
        }
        _ => return Err(XC_ID_BAD),
    }
    // 3. MSI 槽位对拍：CTRL=0x81 → 64 位 + 单槽
    match w.row(0)? {
        CapRow::Msi(c) => {
            if !c.addr64 || c.slots() != 1 || c.requested() != 1 {
                return Err(XC_CTRL_RSVD);
            }
        }
        _ => return Err(XC_ID_BAD),
    }
    // 4. 错误路径①：指针环（MSI NEXT 指回自身）
    let mut w2 = MsiCapWalker::new();
    let cfg2 = sample_cfg(0x50, 0x50, 0x50);
    w2.walk(&cfg2)?;
    if w2.intercept(0).is_err() {
        return Err(XC_PTR_LOOP);
    }
    // 5. 错误路径②：非 MSI 族 ID 拦截（0x10 PCI-E）：不入账、链继续
    let mut w3 = MsiCapWalker::new();
    let mut cfg3 = sample_cfg(0x50, 0x70, 0x70);
    cfg3[0x50 / 4] = 0x10;
    let ok3 = w3.walk(&cfg3)?;
    if ok3 != 1 || w3.msi_count() != 0 || w3.msix_count() != 1 {
        return Err(XC_ID_BAD);
    }
    if w3.intercept(0)? != (0x50, XC_ID_BAD, 1) {
        return Err(XC_ID_BAD);
    }
    // 6. 错误路径③：MSI-X TBL BIR=6（>5）拦截
    let mut w4 = MsiCapWalker::new();
    let mut cfg4 = sample_cfg(0x50, 0x70, 0x70);
    cfg4[0x74 / 4] = 0x0000_0006; // TBL BIR=6
    w4.walk(&cfg4)?;
    if w4.msix_count() != 0 {
        return Err(XC_BIR_BAD);
    }
    if w4.intercept(0)? != (0x70, XC_BIR_BAD, 1) {
        return Err(XC_BIR_BAD);
    }
    // 7. 守恒核
    if !w.intercept_conserve() || !w.consume_conserve() {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
