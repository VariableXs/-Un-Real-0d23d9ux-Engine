//! UNX-F0086 · 符号表保留与 panic 符号化 · 诊断段 0x2220
//! 验收锚点：UNX-F0086-J1 静态符号表按址索引，PC→符号+行内偏移
//! 解析与 panic 路径判据通过；纯功能代码 ≥300 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2220;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;    // PC 越出符号覆盖域
const E_DUP: u16 = SEG_BASE + 2;      // 同址符号重复
const E_EMPTY: u16 = SEG_BASE + 3;    // 空符号表
const E_RDONLY: u16 = SEG_BASE + 4;   // 表已冻结
const E_NAME: u16 = SEG_BASE + 5;     // 符号名非法（空/超长）
const E_ORDER: u16 = SEG_BASE + 6;    // 地址非升序登记
const E_BAL: u16 = SEG_BASE + 7;      // 解析账守恒不平
const E_DEPTH: u16 = SEG_BASE + 8;    // 回溯深度超限

/// 符号名上限。
pub const NAME_MAX: usize = 96;
/// 回溯深度上限。
pub const BACKTRACE_MAX: usize = 32;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "PC 越出符号覆盖域：符号化失败按裸地址输出",
        E_DUP => "同址符号重复：后到拒绝",
        E_EMPTY => "空符号表：panic 符号化退化为裸地址",
        E_RDONLY => "符号表已冻结：运行期禁增",
        E_NAME => "符号名非法：空或超长",
        E_ORDER => "地址非升序登记：二分查找依赖有序表",
        E_BAL => "解析账守恒：命中+未命中 = 解析请求数",
        E_DEPTH => "回溯深度超限：截断并留痕",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub addr: u64,
    pub name: String,
    pub size: u64, // 符号体大小（字节）
}

impl Symbol {
    pub fn contains(&self, pc: u64) -> bool {
        pc >= self.addr && pc < self.addr + self.size.max(1)
    }
}

/// 符号化结果。
#[derive(Clone, Debug, PartialEq)]
pub enum SymResolution {
    Hit { name: String, offset: u64 },
    Bare(u64),
}

impl SymResolution {
    pub fn render(&self) -> String {
        match self {
            SymResolution::Hit { name, offset } => {
                if *offset == 0 {
                    format!("{}", name)
                } else {
                    format!("{}+0x{:X}", name, offset)
                }
            }
            SymResolution::Bare(a) => format!("0x{:X}", a),
        }
    }
}

// ── 符号表 ───────────────────────────────────────────────────
pub struct SymbolTable {
    syms: Vec<Symbol>,
    pub frozen: bool,
    pub lookups: usize,
    pub hits: usize,
}

impl SymbolTable {
    pub fn new() -> SymbolTable {
        SymbolTable {
            syms: Vec::new(),
            frozen: false,
            lookups: 0,
            hits: 0,
        }
    }

    /// 登记符号：升序纪律 + 名字校验 + 同址拒绝。
    pub fn add(&mut self, addr: u64, name: &str, size: u64) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME);
        }
        if let Some(last) = self.syms.last() {
            if addr < last.addr {
                return Err(E_ORDER);
            }
        }
        if self.syms.iter().any(|s| s.addr == addr) {
            return Err(E_DUP);
        }
        self.syms.push(Symbol {
            addr,
            name: name.to_string(),
            size,
        });
        Ok(())
    }

    pub fn freeze(&mut self) -> Result<usize, u16> {
        if self.syms.is_empty() {
            return Err(E_EMPTY);
        }
        self.frozen = true;
        Ok(self.syms.len())
    }

    /// PC→符号：二分查找最近前驱符号，体大小内命中。
    pub fn resolve(&mut self, pc: u64) -> SymResolution {
        self.lookups += 1;
        if self.syms.is_empty() {
            return SymResolution::Bare(pc);
        }
        // 二分：找最后一个 addr <= pc 的符号
        let mut lo = 0usize;
        let mut hi = self.syms.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.syms[mid].addr <= pc {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return SymResolution::Bare(pc);
        }
        let s = &self.syms[lo - 1];
        if s.contains(pc) {
            self.hits += 1;
            SymResolution::Hit {
                name: s.name.clone(),
                offset: pc - s.addr,
            }
        } else {
            SymResolution::Bare(pc)
        }
    }

    pub fn len(&self) -> usize {
        self.syms.len()
    }

    pub fn reconcile(&self) -> Result<(), u16> {
        if self.hits > self.lookups {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for s in &self.syms {
            buf.extend_from_slice(&s.addr.to_le_bytes());
            buf.extend_from_slice(&s.size.to_le_bytes());
            buf.extend_from_slice(s.name.as_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── panic 符号化输出（行内偏移 + 回溯账）────────────────────────
pub struct PanicReport {
    pub lines: Vec<String>,
    pub frames: usize,
    pub truncated: bool,
}

/// panic 路径：现场 PC 链逐帧符号化。
/// `pcs`：回溯链（由新到旧）。
pub fn symbolize_panic(
    table: &mut SymbolTable,
    reason: &str,
    pcs: &[u64],
) -> Result<PanicReport, u16> {
    let mut lines = Vec::new();
    lines.push(format!("panic: {}", reason));
    let mut truncated = false;
    let n = pcs.len();
    for (i, pc) in pcs.iter().enumerate() {
        if i >= BACKTRACE_MAX {
            truncated = true;
            break;
        }
        let r = table.resolve(*pc);
        lines.push(format!("#{:02} {}", i, r.render()));
    }
    if n > BACKTRACE_MAX {
        truncated = true;
    }
    table.reconcile()?;
    Ok(PanicReport {
        lines,
        frames: n.min(BACKTRACE_MAX),
        truncated,
    })
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建表 → 冻结 → 命中与行内偏移。
    let mut t = SymbolTable::new();
    t.add(0x1000_0000, "kernel_start", 0x100)?;
    t.add(0x1000_0100, "sched_tick", 0x80)?;
    t.add(0x1000_0200, "page_fault", 0x40)?;
    t.freeze()?;
    let r = t.resolve(0x1000_0130);
    match &r {
        SymResolution::Hit { name, offset } => {
            if name != "sched_tick" || *offset != 0x30 {
                return Err(E_BAL);
            }
        }
        _ => return Err(E_BAL),
    }
    if r.render() != "sched_tick+0x30" {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：符号间空档 → 裸地址。
    let r2 = t.resolve(0x1000_01FF);
    if r2 != SymResolution::Bare(0x1000_01FF) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：panic 符号化全链 + 回溯截断。
    let pcs = [0x1000_0208u64, 0x1000_0104, 0x1000_0000, 0xDEAD_0000];
    let rep = symbolize_panic(&mut t, "页表校验失败", &pcs)?;
    if rep.frames != 4 || rep.truncated {
        return Err(E_BAL);
    }
    if !rep.lines[1].contains("page_fault+0x8") {
        return Err(E_BAL);
    }
    if !rep.lines[3].contains("kernel_start") {
        return Err(E_BAL);
    }
    if !rep.lines[4].contains("0xDEAD0000") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：深度截断——35 帧 → 32 帧留痕。
    let long_pcs: Vec<u64> = (0..35u64).map(|i| 0x1000_0000 + i).collect();
    let rep2 = symbolize_panic(&mut t, "深回溯", &long_pcs)?;
    if rep2.frames != BACKTRACE_MAX || !rep2.truncated {
        return Err(E_DEPTH);
    }
    hits += 1;

    // I1：同址重复 → E_DUP。
    let mut i1 = SymbolTable::new();
    i1.add(0x2000, "甲", 16)?;
    if i1.add(0x2000, "乙", 16) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I2：降序登记 → E_ORDER。
    let mut i2 = SymbolTable::new();
    i2.add(0x3000, "先", 16)?;
    if i2.add(0x2000, "后", 16) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I3：空名 → E_NAME。
    if i2.add(0x4000, "", 16) != Err(E_NAME) {
        return Err(E_NAME);
    }
    hits += 1;

    // I4：空表冻结 → E_EMPTY。
    let mut i4 = SymbolTable::new();
    if i4.freeze() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I5：冻结后登记 → E_RDONLY。
    let mut i5 = SymbolTable::new();
    i5.add(0x5000, "唯", 16)?;
    i5.freeze()?;
    if i5.add(0x6000, "后补", 16) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I6：账守恒。
    t.reconcile()?;
    hits += 1;

    Ok(hits)
}
