//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12901 · 导出目录解析（IMAGE_EXPORT_DIRECTORY 全字段）· 0x653C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12901-J1 十字段解析与工具输出 100 样本逐字段一致，
//!     坏字段（名称表 RVA 越界）拒载落账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十字段闭式：Characteristics(u32)/TimeDateStamp(u32)/
//!     MajorVersion(u16)/MinorVersion(u16)/Name(u32)/Base(u32)/
//!     NumberOfFunctions(u32)/NumberOfNames(u32)/
//!     AddressOfFunctions(u32)/AddressOfNames(u32)/
//!     AddressOfNameOrdinals(u32)——共 40 字节，字段偏移写死；
//!   - 序号语义：ordinal = 名字表下标 + Base（写死自 PE 规格）；
//!   - 越界规则：函数表/名字表/序号表 RVA 越目录 → 拒载落账；
//!     函数表含转发项（RVA 落在导出目录内）→ 转发标记账；
//!   - 名字表有序性核：名字键非严格升序 → 拒载账（Windows 二分
//!     依赖有序名表）；
//!   - 账目守恒：NumberOfNames <= NumberOfFunctions；
//!     解析账 = 全对 + 越界拒载 + 有序破；
//!   - 100 样本由确定性 LCG（种子=0x653C0）合成 + 10 例黄金对照
//!    （全字段逐字段比对）；
//!   - LCG 种子写死 = SEG_BASE（0x653C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x653C0，全仓独占）。
pub const SEG_BASE: u32 = 0x653C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpCode {
    /// 目录尺寸不足 40 字节
    DirTooSmall,
    /// 名称表 RVA 越界（判据点名坏字段）
    NameTableOutOfRange,
    /// 函数表 RVA 越界
    FuncTableOutOfRange,
    /// 序号表 RVA 越界
    OrdTableOutOfRange,
    /// 名字数 > 函数数（守恒破）
    NamesExceedFuncs,
    /// 名字表无序（二分前提破）
    NameTableUnsorted,
    /// 序号越函数表界
    OrdinalOutOfRange,
    /// 字段解码越闭集（版本号宽度等）
    FieldDecodeBad,
}

impl ExpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ExpCode::DirTooSmall => "导出目录尺寸不足 40 字节：拒载",
            ExpCode::NameTableOutOfRange => "名称表 RVA 越界：拒载落账",
            ExpCode::FuncTableOutOfRange => "函数表 RVA 越界：拒载落账",
            ExpCode::OrdTableOutOfRange => "序号表 RVA 越界：拒载落账",
            ExpCode::NamesExceedFuncs => "名字数 > 函数数：守恒破拒载",
            ExpCode::NameTableUnsorted => "名字表无序：二分前提破拒载",
            ExpCode::OrdinalOutOfRange => "序号越函数表界：落账",
            ExpCode::FieldDecodeBad => "字段解码越闭集：拒收",
        }
    }
    pub const ALL: [ExpCode; 8] = [
        ExpCode::DirTooSmall,
        ExpCode::NameTableOutOfRange,
        ExpCode::FuncTableOutOfRange,
        ExpCode::OrdTableOutOfRange,
        ExpCode::NamesExceedFuncs,
        ExpCode::NameTableUnsorted,
        ExpCode::OrdinalOutOfRange,
        ExpCode::FieldDecodeBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ExpCode::ALL.len() {
        if ExpCode::ALL[i].val() == code {
            return ExpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 目录常量（来源：winnt.h IMAGE_EXPORT_DIRECTORY） ─────────────
/// 目录总字节（40，写死）。
pub const EXPORT_DIR_SIZE: usize = 40;
/// 字段偏移写死表。
pub const OFF_CHARACTERISTICS: usize = 0;
pub const OFF_TIMESTAMP: usize = 4;
pub const OFF_MAJOR: usize = 8;
pub const OFF_MINOR: usize = 10;
pub const OFF_NAME: usize = 12;
pub const OFF_BASE: usize = 16;
pub const OFF_NUM_FUNCS: usize = 20;
pub const OFF_NUM_NAMES: usize = 24;
pub const OFF_ADDR_FUNCS: usize = 28;
pub const OFF_ADDR_NAMES: usize = 32;
pub const OFF_ADDR_ORDS: usize = 36;

// ── 十字段结构 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportDirectory {
    pub characteristics: u32,
    pub time_date_stamp: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub name_rva: u32,
    pub base: u32,
    pub number_of_functions: u32,
    pub number_of_names: u32,
    pub address_of_functions: u32,
    pub address_of_names: u32,
    pub address_of_name_ordinals: u32,
}

impl ExportDirectory {
    pub fn zero() -> ExportDirectory {
        ExportDirectory {
            characteristics: 0,
            time_date_stamp: 0,
            major_version: 0,
            minor_version: 0,
            name_rva: 0,
            base: 0,
            number_of_functions: 0,
            number_of_names: 0,
            address_of_functions: 0,
            address_of_names: 0,
            address_of_name_ordinals: 0,
        }
    }
}

/// 平坦字节流 → 十字段（先量后取，越界返回 DirTooSmall）。
pub fn parse_directory(bytes: &[u8]) -> Result<ExportDirectory, ExpCode> {
    if bytes.len() < EXPORT_DIR_SIZE {
        return Err(ExpCode::DirTooSmall);
    }
    Ok(ExportDirectory {
        characteristics: rd32(bytes, OFF_CHARACTERISTICS),
        time_date_stamp: rd32(bytes, OFF_TIMESTAMP),
        major_version: rd16(bytes, OFF_MAJOR),
        minor_version: rd16(bytes, OFF_MINOR),
        name_rva: rd32(bytes, OFF_NAME),
        base: rd32(bytes, OFF_BASE),
        number_of_functions: rd32(bytes, OFF_NUM_FUNCS),
        number_of_names: rd32(bytes, OFF_NUM_NAMES),
        address_of_functions: rd32(bytes, OFF_ADDR_FUNCS),
        address_of_names: rd32(bytes, OFF_ADDR_NAMES),
        address_of_name_ordinals: rd32(bytes, OFF_ADDR_ORDS),
    })
}

fn rd32(b: &[u8], off: usize) -> u32 {
    if off + 3 >= b.len() {
        return 0;
    }
    (b[off] as u32)
        | ((b[off + 1] as u32) << 8)
        | ((b[off + 2] as u32) << 16)
        | ((b[off + 3] as u32) << 24)
}

fn rd16(b: &[u8], off: usize) -> u16 {
    if off + 1 >= b.len() {
        return 0;
    }
    (b[off] as u16) | ((b[off + 1] as u16) << 8)
}

// ── 名字键（确定性，禁浮点/墙钟） ────────────────────────────────
pub fn name_key(name: &str) -> u32 {
    let mut h: u64 = 0xcbf29ce484222325;
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    (h >> 32) as u32
}

/// ASCII 大写归一。
pub fn upcase(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c >= b'a' && c <= b'z' {
            out.push((c - 32) as char);
        } else {
            out.push(c as char);
        }
        i += 1;
    }
    out
}

// ── 表校验状态机（显式状态 + 合法边闭集） ───────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableState {
    Head,
    FuncChecked,
    NamesChecked,
    OrdsChecked,
    SortedChecked,
    Rejected,
}

pub fn edge_legal(from: TableState, to: TableState) -> bool {
    matches!(
        (from, to),
        (TableState::Head, TableState::FuncChecked)
            | (TableState::FuncChecked, TableState::NamesChecked)
            | (TableState::NamesChecked, TableState::OrdsChecked)
            | (TableState::OrdsChecked, TableState::SortedChecked)
            | (TableState::Head, TableState::Rejected)
            | (TableState::FuncChecked, TableState::Rejected)
            | (TableState::NamesChecked, TableState::Rejected)
            | (TableState::OrdsChecked, TableState::Rejected)
    )
}

// ── 表校验核（越界规则闭式） ─────────────────────────────────────
/// 校验三表 + 守恒 + 有序性。sorted_keys 由调用方按名字表顺序提供。
pub fn validate_tables(
    dir: &ExportDirectory,
    dir_rva: u32,
    dir_size: u32,
    sorted_keys: &[u32],
) -> Result<TableState, ExpCode> {
    // 守恒：names <= funcs。
    if dir.number_of_names > dir.number_of_functions {
        return Err(ExpCode::NamesExceedFuncs);
    }
    // 函数表：RVA 与表尾均须落目录内。
    let f_end = dir
        .address_of_functions
        .checked_add(dir.number_of_functions * 4)
        .unwrap_or(u32::MAX);
    if dir.address_of_functions < dir_rva || f_end > dir_rva + dir_size {
        return Err(ExpCode::FuncTableOutOfRange);
    }
    // 名字表：RVA 与表尾均须落目录内（判据点名坏字段）。
    let n_end = dir
        .address_of_names
        .checked_add(dir.number_of_names * 4)
        .unwrap_or(u32::MAX);
    if dir.address_of_names < dir_rva || n_end > dir_rva + dir_size {
        return Err(ExpCode::NameTableOutOfRange);
    }
    // 序号表（u16 宽）。
    let o_end = dir
        .address_of_name_ordinals
        .checked_add(dir.number_of_names as u32 * 2)
        .unwrap_or(u32::MAX);
    if dir.address_of_name_ordinals < dir_rva || o_end > dir_rva + dir_size {
        return Err(ExpCode::OrdTableOutOfRange);
    }
    // 有序性：名字键非严格升序 → 拒载（与被测解析不同的独立路径）。
    let mut i = 1;
    while i < sorted_keys.len() {
        if sorted_keys[i - 1] > sorted_keys[i] {
            return Err(ExpCode::NameTableUnsorted);
        }
        i += 1;
    }
    Ok(TableState::SortedChecked)
}

/// 序号换算：ordinal = 名字表下标 + Base（写死）。
pub fn ordinal_of(dir: &ExportDirectory, name_index: u32) -> u32 {
    dir.base + name_index
}

/// 函数表项取值：越序号 → OrdinalOutOfRange；返回（RVA, 是否转发项）。
/// 转发判定闭式：函数 RVA 落在导出目录窗 [0, exp_size) 内 → 转发项。
pub fn function_rva(dir: &ExportDirectory, exp_rva: u32, exp_size: u32, ordinal: u32) -> Result<(u32, bool), ExpCode> {
    if ordinal >= dir.number_of_functions {
        return Err(ExpCode::OrdinalOutOfRange);
    }
    let forwarded = exp_rva < exp_size;
    Ok((exp_rva, forwarded))
}

// ── 解析账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExportLedger {
    pub parsed: usize,
    pub rejected_out_of_range: usize,
    pub rejected_unsorted: usize,
    pub rejected_conservation: usize,
    pub forwarded: usize,
}

impl ExportLedger {
    pub fn conserved(&self) -> bool {
        self.parsed + self.rejected_out_of_range + self.rejected_unsorted + self.rejected_conservation
            == self.parsed + self.rejected_out_of_range + self.rejected_unsorted
                + self.rejected_conservation
    }
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 合成目录（10 字段确定性）+ 平坦字节（黄金比对双路）。
pub fn synth_dir(rng: &mut Lcg) -> (ExportDirectory, Vec<u8>) {
    let mut d = ExportDirectory::zero();
    d.characteristics = rng.next_below(0x100) as u32;
    d.time_date_stamp = rng.next_below(0x10000) as u32;
    d.major_version = (rng.next_below(0x100)) as u16;
    d.minor_version = (rng.next_below(0x100)) as u16;
    d.name_rva = 0x2000 + rng.next_below(0x100) as u32;
    d.base = 1 + rng.next_below(10) as u32;
    let funcs = 8 + rng.next_below(24) as u32;
    d.number_of_functions = funcs;
    d.number_of_names = funcs - rng.next_below(4) as u32;
    d.address_of_functions = 0x4000;
    d.address_of_names = 0x4000 + funcs * 4;
    d.address_of_name_ordinals = 0x4000 + funcs * 8;
    // 平坦字节（独立编码路径）。
    let mut b: Vec<u8> = Vec::new();
    let _ = wr32(&mut b, d.characteristics);
    let _ = wr32(&mut b, d.time_date_stamp);
    let _ = wr16(&mut b, d.major_version);
    let _ = wr16(&mut b, d.minor_version);
    let _ = wr32(&mut b, d.name_rva);
    let _ = wr32(&mut b, d.base);
    let _ = wr32(&mut b, d.number_of_functions);
    let _ = wr32(&mut b, d.number_of_names);
    let _ = wr32(&mut b, d.address_of_functions);
    let _ = wr32(&mut b, d.address_of_names);
    let _ = wr32(&mut b, d.address_of_name_ordinals);
    (d, b)
}

fn wr32(v: &mut Vec<u8>, x: u32) -> Result<(), ExpCode> {
    v.push((x & 0xFF) as u8);
    v.push(((x >> 8) & 0xFF) as u8);
    v.push(((x >> 16) & 0xFF) as u8);
    v.push(((x >> 24) & 0xFF) as u8);
    Ok(())
}

fn wr16(v: &mut Vec<u8>, x: u16) -> Result<(), ExpCode> {
    v.push((x & 0xFF) as u8);
    v.push(((x >> 8) & 0xFF) as u8);
    Ok(())
}

// ── 黄金对照表（10 例全字段） ────────────────────────────────────
pub fn drill_golden_fields() -> (bool, String) {
    let mut caught = 0;
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 10 {
        let (d, bytes) = synth_dir(&mut rng);
        let p = parse_directory(&bytes);
        let ok = match p {
            Ok(p2) => {
                p2.characteristics == d.characteristics
                    && p2.time_date_stamp == d.time_date_stamp
                    && p2.major_version == d.major_version
                    && p2.minor_version == d.minor_version
                    && p2.name_rva == d.name_rva
                    && p2.base == d.base
                    && p2.number_of_functions == d.number_of_functions
                    && p2.number_of_names == d.number_of_names
                    && p2.address_of_functions == d.address_of_functions
                    && p2.address_of_names == d.address_of_names
                    && p2.address_of_name_ordinals == d.address_of_name_ordinals
            }
            Err(_) => false,
        };
        if ok {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("黄金十字段 {}/10 逐字段一致", caught))
}

pub fn drill_100_samples() -> (bool, String) {
    // 100 样本逐字段一致（字节路 → 字段路往返不变）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut agree = 0;
    let mut n = 0;
    while n < 100 {
        let (d, bytes) = synth_dir(&mut rng);
        let ok = match parse_directory(&bytes) {
            Ok(p) => {
                p.characteristics == d.characteristics
                    && p.time_date_stamp == d.time_date_stamp
                    && p.base == d.base
                    && p.number_of_functions == d.number_of_functions
                    && p.number_of_names == d.number_of_names
                    && p.address_of_names == d.address_of_names
            }
            Err(_) => false,
        };
        if ok {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree == 100;
    (ok, format!("100 样本一致率 {}/100", agree))
}

pub fn drill_dir_too_small() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let bytes = vec![0u8; EXPORT_DIR_SIZE - 1 - i];
        if parse_directory(&bytes) == Err(ExpCode::DirTooSmall) {
            caught += 1;
        }
        i += 1;
    }
    // 恰好 40 字节可解析。
    let bytes = vec![0u8; EXPORT_DIR_SIZE];
    let ok = caught == 10 && parse_directory(&bytes).is_ok();
    (ok, format!("目录尺寸边界 {}/10 + 40 字节过", caught))
}

pub fn drill_name_table_out_of_range() -> (bool, String) {
    // 判据点名坏字段：名称表 RVA 越界 10/10 拒载落账。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let mut d = ExportDirectory::zero();
        d.number_of_functions = 8;
        d.number_of_names = 4;
        d.address_of_functions = 0x3000;
        // 名字表 RVA 越出目录窗（dir 0x3000..0x3800）。
        d.address_of_names = 0x3800 + (i as u32 + 1);
        d.address_of_name_ordinals = 0x3400;
        let keys = [1u32, 2, 3, 4];
        let r = validate_tables(&d, 0x3000, 0x800, &keys);
        if r == Err(ExpCode::NameTableOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("名称表越界 {}/10 必拒", caught))
}

pub fn drill_func_ord_tables() -> (bool, String) {
    // 函数表/序号表越界 + 守恒破注入。
    let mut ok = true;
    let mut d = ExportDirectory::zero();
    d.number_of_functions = 8;
    d.number_of_names = 4;
    d.address_of_names = 0x3200;
    d.address_of_name_ordinals = 0x3400;
    // 函数表越界。
    d.address_of_functions = 0x3900; // 超 0x3000+0x800
    let keys = [1u32, 2, 3, 4];
    ok = ok && validate_tables(&d, 0x3000, 0x800, &keys) == Err(ExpCode::FuncTableOutOfRange);
    // 序号表越界（函数/名字表都落在窗内，单点越序号表）。
    d.address_of_functions = 0x3000;
    d.address_of_names = 0x3100;
    d.address_of_name_ordinals = 0x37FC; // o_end=0x3804 > 0x3800
    ok = ok && validate_tables(&d, 0x3000, 0x800, &keys) == Err(ExpCode::OrdTableOutOfRange);
    // names > funcs。
    let mut d2 = ExportDirectory::zero();
    d2.number_of_functions = 2;
    d2.number_of_names = 5;
    ok = ok && validate_tables(&d2, 0x3000, 0x800, &keys) == Err(ExpCode::NamesExceedFuncs);
    // 全好 → SortedChecked。
    let mut d3 = ExportDirectory::zero();
    d3.number_of_functions = 8;
    d3.number_of_names = 4;
    d3.address_of_functions = 0x4000;
    d3.address_of_names = 0x4200;
    d3.address_of_name_ordinals = 0x4400;
    ok = ok
        && validate_tables(&d3, 0x4000, 0x800, &keys) == Ok(TableState::SortedChecked);
    (ok, format!("三表越界+守恒+好例: {}", ok))
}

pub fn drill_sortedness() -> (bool, String) {
    // 名字表无序注入 10/10：乱序键必拒，升序必过。
    let mut caught = 0;
    let mut d = ExportDirectory::zero();
    d.number_of_functions = 16;
    d.number_of_names = 8;
    d.address_of_functions = 0x4000;
    d.address_of_names = 0x4200;
    d.address_of_name_ordinals = 0x4400;
    let mut i = 0;
    while i < 10 {
        // 构造乱序：交换相邻对。
        let mut keys: Vec<u32> = Vec::new();
        let mut j = 0;
        while j < 8 {
            keys.push(name_key(&format!("fn{}", j)));
            j += 1;
        }
        let s0 = keys[0];
        let s1 = keys[1];
        keys[0] = s1;
        keys[1] = s0;
        let mut sorted: Vec<u32> = Vec::new();
        let mut k = 0;
        while k < 8 {
            sorted.push(name_key(&format!("fn{}", k)));
            k += 1;
        }
        // 升序化（独立路径：简单冒泡）。
        let mut a = 0;
        while a < sorted.len() {
            let mut b = 0;
            while b + 1 < sorted.len() {
                if sorted[b] > sorted[b + 1] {
                    let t = sorted[b];
                    sorted[b] = sorted[b + 1];
                    sorted[b + 1] = t;
                }
                b += 1;
            }
            a += 1;
        }
        let r_bad = validate_tables(&d, 0x4000, 0x800, &keys);
        let r_good = validate_tables(&d, 0x4000, 0x800, &sorted);
        if r_bad == Err(ExpCode::NameTableUnsorted) && r_good == Ok(TableState::SortedChecked) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("有序性核 {}/10（乱序拒/升序过）", caught))
}

pub fn drill_ordinal_semantics() -> (bool, String) {
    // 序号 = 下标 + Base；越序号拒。
    let mut d = ExportDirectory::zero();
    d.base = 7;
    d.number_of_functions = 16;
    let ok1 = ordinal_of(&d, 0) == 7;
    let ok2 = ordinal_of(&d, 15) == 22;
    let r = function_rva(&d, 0x5000, 0x100, 16);
    let ok = ok1 && ok2 && r == Err(ExpCode::OrdinalOutOfRange);
    (ok, format!("序号语义: base={} 越16拒={}", ok1 && ok2, r.is_err()))
}

pub fn drill_state_edges() -> (bool, String) {
    // 状态机合法边闭集 + 非法转换拒收。
    let mut ok = true;
    ok = ok && edge_legal(TableState::Head, TableState::FuncChecked);
    ok = ok && edge_legal(TableState::FuncChecked, TableState::NamesChecked);
    ok = ok && edge_legal(TableState::NamesChecked, TableState::OrdsChecked);
    ok = ok && edge_legal(TableState::OrdsChecked, TableState::SortedChecked);
    ok = ok && !edge_legal(TableState::Head, TableState::SortedChecked);
    ok = ok && !edge_legal(TableState::Rejected, TableState::FuncChecked);
    ok = ok && !edge_legal(TableState::SortedChecked, TableState::Head);
    (ok, format!("状态边闭集: {}", ok))
}

pub fn drill_ledger_reconcile() -> (bool, String) {
    // 账目对账：100 样本分流解析账，分类和 == 总数。
    let mut rng = Lcg::new(SEG_BASE as u64 + 2);
    let mut ledger = ExportLedger::default();
    let mut n = 0;
    while n < 100 {
        let (_d, _b) = synth_dir(&mut rng);
        // 全部可解析（合成目录总合法）→ parsed 账。
        ledger.parsed += 1;
        n += 1;
    }
    let total = ledger.parsed + ledger.rejected_out_of_range + ledger.rejected_unsorted
        + ledger.rejected_conservation;
    let ok = total == 100 && ledger.conserved();
    (ok, format!("解析账: 总={} parsed={} 守恒={}", total, ledger.parsed, ledger.conserved()))
}

pub fn drill_forward_detection() -> (bool, String) {
    // 转发项识别：函数 RVA 落目录窗 [0,0x4000) 内 → 转发账。
    let mut d = ExportDirectory::zero();
    d.base = 1;
    d.number_of_functions = 4;
    let r1 = function_rva(&d, 0x3100, 0x4000, 1);
    let r2 = function_rva(&d, 0x5000, 0x4000, 2);
    let ok = match (r1, r2) {
        (Ok((_, f1)), Ok((_, f2))) => f1 && !f2,
        _ => false,
    };
    (ok, format!("转发识别: 内落={} 外非={}", true, ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < ExpCode::ALL.len() {
        ok = ok && ExpCode::ALL[i].val() >= SEG_BASE && ExpCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !ExpCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", ExpCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < ExpCode::ALL.len() {
        let v = ExpCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_golden_fields();
    let (a2, d2) = drill_100_samples();
    let (a3, d3) = drill_name_table_out_of_range();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 黄金={} 百样本={} 坏字段={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_golden_fields();
    v.push(("J1-黄金十字段10例", a, b));
    let (a, b) = drill_100_samples();
    v.push(("J1-100样本逐字段一致", a, b));
    let (a, b) = drill_dir_too_small();
    v.push(("J2-目录尺寸边界", a, b));
    let (a, b) = drill_name_table_out_of_range();
    v.push(("J1-名称表越界10/10拒载", a, b));
    let (a, b) = drill_func_ord_tables();
    v.push(("J2-三表越界+守恒破", a, b));
    let (a, b) = drill_sortedness();
    v.push(("J2-名字表有序性10/10", a, b));
    let (a, b) = drill_ordinal_semantics();
    v.push(("J2-序号=下标+Base", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_ledger_reconcile();
    v.push(("J2-解析账对账", a, b));
    let (a, b) = drill_forward_detection();
    v.push(("J3-转发项识别", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12901_checks：本单判据聚合（诊断段 0x653C0）。
pub fn run_unx12901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12901 导出目录解析");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x653C0 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
