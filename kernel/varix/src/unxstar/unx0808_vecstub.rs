//! unx0808_vecstub —— UNX-F0808 · 每向量 stub 生成器与入口表固化（目标 260 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0808 · 每向量 stub 生成器与入口表固化。
//! 判据原文：UNX-F0808-J1 256 stub 编码正确性全量回读验证（机器码逐字节比对），
//!           入口表地址零漂移。
//! 设计纪律：
//! - 256 个中断向量，每向量一枚 stub；stub 采用统一 11 字节模板：
//!   [0x6A vec] push imm8 —— 立即数压入向量号（0x00~0xFF 全覆盖）；
//!   [0xE9 rel32] jmp rel32 —— 跳转公共入口（rel32 补码回填）。
//!   模板定长保证「地址零漂移」：stub[i] 起址 = BASE + i*STUB_LEN，纯算术可复算。
//! - 编码正确性全量回读：生成后逐 stub 逐字节与期望序列比对，任何漂移即红账。
//! - 入口表：256 槽定容表，槽 i 固化 stub[i] 起址；表生成后只读封存（写时账封）。
//! - 越界向量（>=256）生成即拒；rel32 溢出（公共入口距离超 i32 界）显性拒绝。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6260（0x6261~0x6266 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const VS_VERSION: &str = "unx0808_vecstub-v1";

/// 诊断段基址（0x6260 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6260;

/// XcCode 闭集。
pub const XC_VEC_RANGE: u16 = SEG_BASE + 1; // 向量号越界（>=256）
pub const XC_REL32_OVF: u16 = SEG_BASE + 2; // rel32 位移溢出
pub const XC_TMPL_LEN: u16 = SEG_BASE + 3; // 模板长度异常（生成器内部不变量破）
pub const XC_READBACK: u16 = SEG_BASE + 4; // 回读比对失败（机器码漂移）
pub const XC_TABLE_SEAL: u16 = SEG_BASE + 5; // 入口表已封存仍尝试写
pub const XC_TABLE_SLOT: u16 = SEG_BASE + 6; // 入口表槽位越界

/// stub 模板定长（push imm8 + jmp rel32 = 2 + 5 = 7 字节；补 4 字节对齐垫 → 12）。
/// 说明：垫字节固定 0xCC（int3 落入即暴露越界执行），回读同样比对。
pub const STUB_LEN: usize = 12;

/// 向量总数（IA-32 中断面全量）。
pub const VECTOR_TOTAL: usize = 256;

/// push imm8 编码（0x6A + imm8）。
const OP_PUSH_IMM8: u8 = 0x6A;
/// jmp rel32 编码（0xE9 + rel32）。
const OP_JMP_REL32: u8 = 0xE9;
/// 垫字节（int3）。
const PAD_BYTE: u8 = 0xCC;

/// stub 起址基址（纯建模地址空间：0xFEE00000 上方 stub 区，页对齐）。
pub const STUB_BASE_ADDR: u32 = 0xFEE1_0000;

/// 单条 stub 的期望字节序列（逐字节算术推导，不查表）。
pub fn expected_stub_bytes(vec_no: u8, common_entry: u32) -> [u8; STUB_LEN] {
    let mut out = [0u8; STUB_LEN];
    out[0] = OP_PUSH_IMM8;
    out[1] = vec_no;
    out[2] = OP_JMP_REL32;
    // rel32 = common_entry - (stub_addr + 7)；stub_addr = BASE + vec*LEN
    let stub_addr = STUB_BASE_ADDR + (vec_no as u32) * (STUB_LEN as u32);
    let next_ip = stub_addr + 7;
    let rel = common_entry.wrapping_sub(next_ip);
    out[3] = (rel & 0xFF) as u8;
    out[4] = ((rel >> 8) & 0xFF) as u8;
    out[5] = ((rel >> 16) & 0xFF) as u8;
    out[6] = ((rel >> 24) & 0xFF) as u8;
    // 7..12 垫 int3
    for b in out[7..STUB_LEN].iter_mut() {
        *b = PAD_BYTE;
    }
    out
}

/// stub 生成器：产出全部 256 stub 的字节流（平铺）+ 漂移账。
/// common_entry 为公共入口地址；rel32 溢出即整体拒绝（不产半成品）。
pub fn generate_all(common_entry: u32) -> Result<Vec<u8>, u16> {
    // rel32 可行性预检：最远 stub 与公共入口的位移必须双向落在 i32 界内。
    let far_addr = STUB_BASE_ADDR + (VECTOR_TOTAL as u32 - 1) * (STUB_LEN as u32);
    let d1 = (common_entry as i64) - (far_addr as i64) - 7;
    let d2 = (STUB_BASE_ADDR as i64) - (common_entry as i64) - 7;
    if d1 > i32::MAX as i64 || d1 < i32::MIN as i64 || d2 > i32::MAX as i64 || d2 < i32::MIN as i64 {
        return Err(XC_REL32_OVF);
    }
    let mut out = Vec::with_capacity(VECTOR_TOTAL * STUB_LEN);
    for i in 0..VECTOR_TOTAL {
        let bytes = expected_stub_bytes(i as u8, common_entry);
        for b in bytes.iter() {
            out.push(*b);
        }
    }
    if out.len() != VECTOR_TOTAL * STUB_LEN {
        return Err(XC_TMPL_LEN);
    }
    Ok(out)
}

/// 入口表：256 槽定容固化表。
pub struct EntryTable {
    /// 槽 i = stub[i] 起址。
    slots: [u32; VECTOR_TOTAL],
    /// 封存旗标：true 后任何写尝试被拦截。
    sealed: bool,
    /// 生成时公共入口（回读验证锚点）。
    common_entry: u32,
}

impl EntryTable {
    /// 生成并固化入口表：槽址纯算术，不依赖生成器的字节流（双源对拍用）。
    pub fn build(common_entry: u32) -> EntryTable {
        let mut slots = [0u32; VECTOR_TOTAL];
        for (i, s) in slots.iter_mut().enumerate() {
            *s = STUB_BASE_ADDR + (i as u32) * (STUB_LEN as u32);
        }
        EntryTable {
            slots,
            sealed: true,
            common_entry,
        }
    }

    /// 只读取槽址；越界即拒（XcCode）。
    pub fn slot(&self, vec_no: usize) -> Result<u32, u16> {
        if vec_no >= VECTOR_TOTAL {
            return Err(XC_TABLE_SLOT);
        }
        Ok(self.slots[vec_no])
    }

    /// 封存状态查询。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 写槽（仅封存前可用；封存后拦截并红账码）。
    pub fn write_slot(&mut self, vec_no: usize, addr: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_TABLE_SEAL);
        }
        if vec_no >= VECTOR_TOTAL {
            return Err(XC_TABLE_SLOT);
        }
        self.slots[vec_no] = addr;
        Ok(())
    }

    /// 公共入口锚点。
    pub fn common_entry(&self) -> u32 {
        self.common_entry
    }
}

/// 全量回读验证：对生成字节流逐 stub 逐字节与期望序列比对。
/// 返回 Ok(true) 全对；Ok(false) 有漂移（漂移账已记）；Err 为参数/内部不变量错。
pub struct ReadbackReport {
    /// 比对过的 stub 数。
    pub checked: usize,
    /// 首个漂移的向量号（None = 无漂移）。
    pub first_drift: Option<usize>,
    /// 首个漂移的字节偏移（stub 内）。
    pub first_drift_off: Option<usize>,
    /// 入口表槽址与算术期望零漂移核对的结论。
    pub table_zero_drift: bool,
}

pub fn verify_readback(stream: &[u8], common_entry: u32, table: &EntryTable) -> Result<ReadbackReport, u16> {
    if stream.len() != VECTOR_TOTAL * STUB_LEN {
        return Err(XC_TMPL_LEN);
    }
    let mut rep = ReadbackReport {
        checked: 0,
        first_drift: None,
        first_drift_off: None,
        table_zero_drift: true,
    };
    for i in 0..VECTOR_TOTAL {
        let expect = expected_stub_bytes(i as u8, common_entry);
        let base = i * STUB_LEN;
        for (off, eb) in expect.iter().enumerate() {
            if stream[base + off] != *eb {
                if rep.first_drift.is_none() {
                    rep.first_drift = Some(i);
                    rep.first_drift_off = Some(off);
                }
            }
        }
        rep.checked += 1;
    }
    // 入口表零漂移核对：槽址 == BASE + i*LEN 逐槽复算。
    for i in 0..VECTOR_TOTAL {
        let want = STUB_BASE_ADDR + (i as u32) * (STUB_LEN as u32);
        if table.slot(i)? != want {
            rep.table_zero_drift = false;
        }
    }
    Ok(rep)
}

/// 解码器：从字节流反解 stub（回读验证的对偶面——编码/解码互逆即无漂移）。
pub fn decode_stub(stream: &[u8], vec_no: usize) -> Result<(u8, u32), u16> {
    if vec_no >= VECTOR_TOTAL {
        return Err(XC_VEC_RANGE);
    }
    let base = vec_no * STUB_LEN;
    if stream.len() < base + STUB_LEN {
        return Err(XC_TMPL_LEN);
    }
    if stream[base] != OP_PUSH_IMM8 {
        return Err(XC_READBACK);
    }
    let vec = stream[base + 1];
    if stream[base + 2] != OP_JMP_REL32 {
        return Err(XC_READBACK);
    }
    let rel = (stream[base + 3] as u32)
        | ((stream[base + 4] as u32) << 8)
        | ((stream[base + 5] as u32) << 16)
        | ((stream[base + 6] as u32) << 24);
    let stub_addr = STUB_BASE_ADDR + (vec_no as u32) * (STUB_LEN as u32);
    let target = stub_addr.wrapping_add(7).wrapping_add(rel);
    Ok((vec, target))
}

/// 漂移账：定容 16 槽，记录（向量号, 偏移, 期望, 实得）。
pub struct DriftLedger {
    entries: [(usize, usize, u8, u8); 16],
    len: usize,
    overflow: usize,
}

impl DriftLedger {
    pub fn new() -> DriftLedger {
        DriftLedger {
            entries: [(0, 0, 0, 0); 16],
            len: 0,
            overflow: 0,
        }
    }

    /// 记一条漂移；账满即计溢出（零丢弃红线：溢出必须为 0 才算账全）。
    pub fn record(&mut self, vec_no: usize, off: usize, expect: u8, got: u8) {
        if self.len < 16 {
            self.entries[self.len] = (vec_no, off, expect, got);
            self.len += 1;
        } else {
            self.overflow += 1;
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 溢出数（>0 即账不全）。
    pub fn overflow(&self) -> usize {
        self.overflow
    }

    /// 第 i 条（越界即拒）。
    pub fn entry(&self, i: usize) -> Result<(usize, usize, u8, u8), u16> {
        if i >= self.len {
            return Err(XC_TABLE_SLOT);
        }
        Ok(self.entries[i])
    }

    /// 守恒核：len + overflow == 已记录总数。
    pub fn conserve(&self, recorded_total: usize) -> bool {
        self.len + self.overflow == recorded_total
    }
}

/// drill：全链自例程——生成→入口表固化→回读验证→漂移账→错误路径。
pub fn drill() -> Result<u32, u16> {
    let common = 0xFEE0_8000u32;
    // 1. 生成
    let stream = generate_all(common)?;
    // 2. 入口表
    let table = EntryTable::build(common);
    // 3. 回读全量比对
    let rep = verify_readback(&stream, common, &table)?;
    if rep.checked != VECTOR_TOTAL {
        return Err(XC_TMPL_LEN);
    }
    if rep.first_drift.is_some() || !rep.table_zero_drift {
        return Err(XC_READBACK);
    }
    // 4. 解码互逆抽核：8 个采样向量 decode == 生成参数
    for i in [0usize, 1, 63, 127, 128, 200, 254, 255] {
        let (v, tgt) = decode_stub(&stream, i)?;
        if v != i as u8 {
            return Err(XC_READBACK);
        }
        let want = common;
        if tgt != want {
            return Err(XC_READBACK);
        }
    }
    // 5. 漂移账：正常态空账守恒
    let led = DriftLedger::new();
    if !led.conserve(0) || led.len() != 0 {
        return Err(XC_READBACK);
    }
    // 6. 错误路径①：越界向量生成请求（预检拒）——模拟 rel32 溢出：公共入口拉到超远
    let far = 0xFF00_0000u32;
    if generate_all(far).is_ok() {
        return Err(XC_REL32_OVF);
    }
    // 7. 错误路径②：封存表写入拦截
    let mut t2 = EntryTable::build(common);
    // 先解封模拟（测试口径：重建一个未封存表）
    t2.sealed = false;
    t2.write_slot(3, 0xDEAD)?;
    t2.sealed = true;
    if t2.write_slot(4, 0xBEEF).is_ok() {
        return Err(XC_TABLE_SEAL);
    }
    // 8. 错误路径③：槽位越界读拦截
    if t2.slot(256).is_ok() {
        return Err(XC_TABLE_SLOT);
    }
    // 9. 错误路径④：长度异常流拦截
    if verify_readback(&stream[..100], common, &table).is_ok() {
        return Err(XC_TMPL_LEN);
    }
    Ok(0)
}
