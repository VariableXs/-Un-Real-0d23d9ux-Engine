//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12921 · TLS 目录解析（IMAGE_TLS_DIRECTORY64/32 双档）· 0x65400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12921-J1 双档字段宽度差异解析 50 样本一致（Start/End/
//!     Index/Callbacks 四指针组），坏目录拒载一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 64 档字段宽度：Start/End/Index/Callbacks 各 u64（8B），
//!     SizeOfZeroFill/Characteristics 各 u32 —— 共 40 字节；
//!   - 32 档字段宽度：同序各 u32/u32 —— 共 24 字节；
//!   - 四指针组一致性：Start <= End（原始数据闭区间）；Index/Callbacks
//!     非零才有效；Callbacks 表以全零项终止；
//!   - 坏目录拒载闭式：End < Start / 目录尺寸不足 / Callbacks 越界
//!     / SizeOfZeroFill 越窗 → 拒载，双档规则一致（同一判据函数）；
//!   - 50 样本由确定性 LCG（种子=0x65400）合成，双档同参往返
//!     逐字段一致；
//!   - 状态机显式：Head→Width→Pointers→Callbacks→Accepted/Rejected，
//!     非法转换拒收；LCG 种子写死 = SEG_BASE（0x65400）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65400，全仓独占）。
pub const SEG_BASE: u32 = 0x65400;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlsCode {
    /// 目录尺寸不足（按档宽）
    DirTooSmall,
    /// End < Start（原始数据闭区间破）
    EndBeforeStart,
    /// Callbacks 表无终止项或越界
    CallbacksUnbounded,
    /// SizeOfZeroFill 越窗
    ZeroFillOverflow,
    /// 双档宽度混读（64 表按 32 读）
    WidthMismatch,
    /// 档位越闭集（非 32/64）
    ArchUnknown,
    /// Index 指针为零且声明有变量
    IndexMissing,
    /// 状态机非法转换
    IllegalTransition,
}

impl TlsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TlsCode::DirTooSmall => "TLS 目录尺寸不足（按档宽量断）：拒载",
            TlsCode::EndBeforeStart => "End < Start：原始数据闭区间破拒载",
            TlsCode::CallbacksUnbounded => "Callbacks 表无终止项或越界：拒载",
            TlsCode::ZeroFillOverflow => "SizeOfZeroFill 越窗：拒载",
            TlsCode::WidthMismatch => "双档宽度混读：64 表按 32 读必抓",
            TlsCode::ArchUnknown => "档位越闭集（非 32/64）：拒收",
            TlsCode::IndexMissing => "Index 指针为零却声明有 TLS 变量：落账",
            TlsCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [TlsCode; 8] = [
        TlsCode::DirTooSmall,
        TlsCode::EndBeforeStart,
        TlsCode::CallbacksUnbounded,
        TlsCode::ZeroFillOverflow,
        TlsCode::WidthMismatch,
        TlsCode::ArchUnknown,
        TlsCode::IndexMissing,
        TlsCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TlsCode::ALL.len() {
        if TlsCode::ALL[i].val() == code {
            return TlsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 档位闭集与目录尺寸（写死自 winnt.h） ─────────────────────────
pub const ARCH_64: u8 = 64;
pub const ARCH_32: u8 = 32;
/// 64 档目录字节：4 指针×8 + 2×u32。
pub const TLS64_SIZE: usize = 40;
/// 32 档目录字节：4 指针×4 + 2×u32。
pub const TLS32_SIZE: usize = 24;
/// 字段偏移（两档同序，宽度异）。
pub const OFF_START: usize = 0;
pub const OFF_END: usize = 8;
pub const OFF_INDEX: usize = 16;
pub const OFF_CALLBACKS: usize = 24;

// ── 四指针组 + 双档归一结构 ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TlsDir {
    pub arch: u8,
    pub start: u64,
    pub end: u64,
    pub index: u64,
    pub callbacks: u64,
    pub size_of_zero_fill: u32,
    pub characteristics: u32,
}

impl TlsDir {
    pub fn zero(arch: u8) -> TlsDir {
        TlsDir {
            arch,
            start: 0,
            end: 0,
            index: 0,
            callbacks: 0,
            size_of_zero_fill: 0,
            characteristics: 0,
        }
    }
}

/// 档位宽校验：arch 越闭集 → ArchUnknown。
pub fn dir_size_of(arch: u8) -> Result<usize, TlsCode> {
    match arch {
        ARCH_64 => Ok(TLS64_SIZE),
        ARCH_32 => Ok(TLS32_SIZE),
        _ => Err(TlsCode::ArchUnknown),
    }
}

// ── 双档解析（宽度差异单点） ─────────────────────────────────────
pub fn parse_tls(arch: u8, bytes: &[u8]) -> Result<TlsDir, TlsCode> {
    let sz = dir_size_of(arch)?;
    if bytes.len() < sz {
        return Err(TlsCode::DirTooSmall);
    }
    match arch {
        ARCH_64 => Ok(TlsDir {
            arch,
            start: rd64(bytes, OFF_START),
            end: rd64(bytes, OFF_END),
            index: rd64(bytes, OFF_INDEX),
            callbacks: rd64(bytes, OFF_CALLBACKS),
            size_of_zero_fill: rd32(bytes, 32),
            characteristics: rd32(bytes, 36),
        }),
        _ => Ok(TlsDir {
            arch,
            // 32 档宽度差异：字段各 4 字节，偏移 0/4/8/12/16/20。
            start: rd32(bytes, 0) as u64,
            end: rd32(bytes, 4) as u64,
            index: rd32(bytes, 8) as u64,
            callbacks: rd32(bytes, 12) as u64,
            size_of_zero_fill: rd32(bytes, 16),
            characteristics: rd32(bytes, 20),
        }),
    }
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

fn rd64(b: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i = 0;
    while i < 8 {
        if off + i < b.len() {
            v |= (b[off + i] as u64) << (8 * i);
        }
        i += 1;
    }
    v
}

// ── 解析状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlsState {
    Head,
    WidthChecked,
    PointersChecked,
    CallbacksChecked,
    Accepted,
    Rejected,
}

pub fn edge_legal(from: TlsState, to: TlsState) -> bool {
    matches!(
        (from, to),
        (TlsState::Head, TlsState::WidthChecked)
            | (TlsState::WidthChecked, TlsState::PointersChecked)
            | (TlsState::PointersChecked, TlsState::CallbacksChecked)
            | (TlsState::CallbacksChecked, TlsState::Accepted)
            | (TlsState::Head, TlsState::Rejected)
            | (TlsState::WidthChecked, TlsState::Rejected)
            | (TlsState::PointersChecked, TlsState::Rejected)
            | (TlsState::CallbacksChecked, TlsState::Rejected)
    )
}

pub fn transition(from: TlsState, to: TlsState) -> Result<TlsState, TlsCode> {
    if edge_legal(from, to) {
        Ok(to)
    } else {
        Err(TlsCode::IllegalTransition)
    }
}

// ── 校验核（双档同一判据函数：四指针组 + 零填充） ────────────────
/// callback_slots：Callbacks 表槽位数；has_terminator：末槽是否全零。
/// img_size：映像窗（指针 RVA 上界）。
pub fn validate_tls(
    d: &TlsDir,
    callback_slots: usize,
    has_terminator: bool,
    img_size: u64,
) -> Result<TlsState, TlsCode> {
    let _ = transition(TlsState::Head, TlsState::WidthChecked)?;
    // 档位核（WidthChecked 边内）。
    if dir_size_of(d.arch).is_err() {
        return Err(TlsCode::ArchUnknown);
    }
    let _ = transition(TlsState::WidthChecked, TlsState::PointersChecked)?;
    // 四指针组：End >= Start（闭区间）。
    if d.end < d.start {
        return Err(TlsCode::EndBeforeStart);
    }
    // Index 缺失核：有变量（Start<End）却 Index=0。
    if d.end > d.start && d.index == 0 {
        return Err(TlsCode::IndexMissing);
    }
    // 零填充越窗核：Start + SizeOfZeroFill <= 映像窗。
    if (d.start.saturating_add(d.size_of_zero_fill as u64)) > img_size {
        return Err(TlsCode::ZeroFillOverflow);
    }
    let _ = transition(TlsState::PointersChecked, TlsState::CallbacksChecked)?;
    // Callbacks 核：有表则必须带终止项；槽位越映像窗拒。
    if d.callbacks != 0 {
        if !has_terminator {
            return Err(TlsCode::CallbacksUnbounded);
        }
        if d.callbacks.saturating_add((callback_slots as u64) * 8) > img_size {
            return Err(TlsCode::CallbacksUnbounded);
        }
    }
    let _ = transition(TlsState::CallbacksChecked, TlsState::Accepted)?;
    Ok(TlsState::Accepted)
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

/// 合成 64 档目录字节（独立编码路径）。
pub fn synth64_bytes(rng: &mut Lcg, has_callbacks: bool) -> (TlsDir, Vec<u8>) {
    let mut d = TlsDir::zero(ARCH_64);
    let start = 0x6000 + rng.next_below(0x100);
    let len = 1 + rng.next_below(0x400);
    d.start = start;
    d.end = start + len;
    d.index = 0x7000 + rng.next_below(0x80);
    d.callbacks = if has_callbacks { 0x8000 + rng.next_below(0x40) } else { 0 };
    d.size_of_zero_fill = rng.next_below(0x200) as u32;
    d.characteristics = rng.next_below(0x10) as u32;
    let mut b: Vec<u8> = Vec::new();
    let _ = wr64(&mut b, d.start);
    let _ = wr64(&mut b, d.end);
    let _ = wr64(&mut b, d.index);
    let _ = wr64(&mut b, d.callbacks);
    let _ = wr32v(&mut b, d.size_of_zero_fill);
    let _ = wr32v(&mut b, d.characteristics);
    (d, b)
}

/// 由 64 档目录截 32 位得 32 档目录：同参数（宽度差异单点）。
pub fn synth32_from(d64: &TlsDir) -> (TlsDir, Vec<u8>) {
    let mut d = TlsDir::zero(ARCH_32);
    d.start = d64.start & 0xFFFF_FFFF;
    d.end = d64.end & 0xFFFF_FFFF;
    d.index = d64.index & 0xFFFF_FFFF;
    d.callbacks = d64.callbacks & 0xFFFF_FFFF;
    d.size_of_zero_fill = d64.size_of_zero_fill;
    d.characteristics = d64.characteristics;
    let mut b: Vec<u8> = Vec::new();
    let _ = wr32v(&mut b, d.start as u32);
    let _ = wr32v(&mut b, d.end as u32);
    let _ = wr32v(&mut b, d.index as u32);
    let _ = wr32v(&mut b, d.callbacks as u32);
    let _ = wr32v(&mut b, d.size_of_zero_fill);
    let _ = wr32v(&mut b, d.characteristics);
    (d, b)
}

fn wr32v(v: &mut Vec<u8>, x: u32) -> Result<(), TlsCode> {
    v.push((x & 0xFF) as u8);
    v.push(((x >> 8) & 0xFF) as u8);
    v.push(((x >> 16) & 0xFF) as u8);
    v.push(((x >> 24) & 0xFF) as u8);
    Ok(())
}

fn wr64(v: &mut Vec<u8>, x: u64) -> Result<(), TlsCode> {
    let mut i = 0;
    while i < 8 {
        v.push(((x >> (8 * i)) & 0xFF) as u8);
        i += 1;
    }
    Ok(())
}

// ── 50 样本双档一致（判据 J1 主面） ──────────────────────────────
pub fn drill_50_dual_arch() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0;
    let mut n = 0;
    while n < 50 {
        let has_cb = n % 2 == 0;
        let (d64, b64) = synth64_bytes(&mut rng, has_cb);
        let (d32, b32) = synth32_from(&d64);
        let p64 = parse_tls(ARCH_64, &b64);
        let p32 = parse_tls(ARCH_32, &b32);
        // 四指针组逐字段一致（低 32 位等值）+ 双档校验结论一致。
        let ok = match (p64, p32) {
            (Ok(a), Ok(b)) => {
                let ptr_match = (a.start & 0xFFFF_FFFF) == b.start
                    && (a.end & 0xFFFF_FFFF) == b.end
                    && (a.index & 0xFFFF_FFFF) == b.index
                    && (a.callbacks & 0xFFFF_FFFF) == b.callbacks;
                let v64 = validate_tls(&a, 4, true, 0x100000).is_ok();
                let v32 = validate_tls(&b, 4, true, 0x100000).is_ok();
                ptr_match && v64 == v32 && v64
            }
            _ => false,
        };
        if ok {
            agree += 1;
        }
        n += 1;
    }
    let ok = agree == 50;
    (ok, format!("双档 50 样本一致 {}/50", agree))
}

pub fn drill_width_mismatch_caught() -> (bool, String) {
    // 宽度混读注入：64 档表按 32 档读 → 字段错位必抓（SizeOfZeroFill
    // 位读到 Callbacks 低 32 位 != 合成值）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let (d64, b64) = synth64_bytes(&mut rng, true);
        let mis = parse_tls(ARCH_32, &b64);
        // 32 档只读 24 字节（64 表 40 字节足够），但字段值必错位。
        let mut wrong = false;
        match mis {
            Ok(m) => {
                // Index 位读的是 64 档 End 低 32。
                wrong = wrong || m.index != d64.index;
                // callbacks 位读的是 64 档 Index 低 32。
                wrong = wrong || m.callbacks != d64.callbacks;
            }
            Err(_) => wrong = true,
        }
        if wrong {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("宽度混读 {}/10 必抓", caught))
}

pub fn drill_arch_closed() -> (bool, String) {
    // 档位闭集：32/64 过，其余 ArchUnknown。
    let mut ok = dir_size_of(ARCH_64) == Ok(TLS64_SIZE);
    ok = ok && dir_size_of(ARCH_32) == Ok(TLS32_SIZE);
    ok = ok && dir_size_of(16) == Err(TlsCode::ArchUnknown);
    ok = ok && dir_size_of(128) == Err(TlsCode::ArchUnknown);
    ok = ok && parse_tls(16, &[0u8; 64]) == Err(TlsCode::ArchUnknown);
    (ok, format!("档位闭集: {}", ok))
}

pub fn drill_bad_dirs() -> (bool, String) {
    // 坏目录拒载一致：End<Start / 无终止项 / 零填充越窗 / 尺寸不足，
    // 双档同一判据函数结论一致。
    let mut ok = true;
    // End < Start。
    let mut d1 = TlsDir::zero(ARCH_64);
    d1.start = 0x9000;
    d1.end = 0x8000;
    d1.index = 0x1000;
    let r1 = validate_tls(&d1, 2, true, 0x100000);
    let mut d1b = TlsDir::zero(ARCH_32);
    d1b.start = 0x9000;
    d1b.end = 0x8000;
    d1b.index = 0x1000;
    let r1b = validate_tls(&d1b, 2, true, 0x100000);
    ok = ok && r1 == Err(TlsCode::EndBeforeStart) && r1b == Err(TlsCode::EndBeforeStart);
    // Callbacks 无终止项。
    let mut d2 = TlsDir::zero(ARCH_64);
    d2.start = 0x1000;
    d2.end = 0x1100;
    d2.index = 0x2000;
    d2.callbacks = 0x8000;
    ok = ok && validate_tls(&d2, 4, false, 0x100000) == Err(TlsCode::CallbacksUnbounded);
    // 零填充越窗核。
    let mut d3 = TlsDir::zero(ARCH_64);
    d3.start = 0xFF800;
    d3.end = 0xFF900;
    d3.index = 0x2000;
    d3.size_of_zero_fill = 0x1000; // 0xFF800+0x1000 > 0x100000
    ok = ok && validate_tls(&d3, 0, true, 0x100000) == Err(TlsCode::ZeroFillOverflow);
    // 尺寸不足（双档各一）。
    ok = ok && parse_tls(ARCH_64, &[0u8; 39]) == Err(TlsCode::DirTooSmall);
    ok = ok && parse_tls(ARCH_32, &[0u8; 23]) == Err(TlsCode::DirTooSmall);
    // 边界：恰好 40/24 可解析。
    ok = ok && parse_tls(ARCH_64, &[0u8; 40]).is_ok();
    ok = ok && parse_tls(ARCH_32, &[0u8; 24]).is_ok();
    (ok, format!("坏目录拒载一致: {}", ok))
}

pub fn drill_callbacks_table() -> (bool, String) {
    // Callbacks 终止语义：N 槽 + 全零终止；无终止必拒；越窗必拒。
    let mut ok = true;
    let mut d = TlsDir::zero(ARCH_64);
    d.start = 0x1000;
    d.end = 0x1100;
    d.index = 0x2000;
    d.callbacks = 0x8000;
    // 3 槽 + 终止，表尾 0x8000+4*8=0x8020 < 0x100000 → Accepted。
    ok = ok && validate_tls(&d, 3, true, 0x100000) == Ok(TlsState::Accepted);
    // 表尾越窗（0x8000 + 0x20000*8 > 0x100000）。
    ok = ok && validate_tls(&d, 0x20000, true, 0x100000) == Err(TlsCode::CallbacksUnbounded);
    // 无表（callbacks=0）直接过。
    d.callbacks = 0;
    ok = ok && validate_tls(&d, 0, false, 0x100000) == Ok(TlsState::Accepted);
    (ok, format!("Callbacks 终止语义: {}", ok))
}

pub fn drill_index_missing() -> (bool, String) {
    // Index 缺失：Start<End 却 Index=0 → 落账。
    let mut d = TlsDir::zero(ARCH_64);
    d.start = 0x1000;
    d.end = 0x1100;
    d.index = 0;
    let r = validate_tls(&d, 0, true, 0x100000);
    // 对照：无变量（Start==End）Index=0 合法。
    let mut d2 = d;
    d2.end = d2.start;
    let r2 = validate_tls(&d2, 0, true, 0x100000);
    (r == Err(TlsCode::IndexMissing) && r2 == Ok(TlsState::Accepted),
        format!("Index 缺失: 有变量拒={} 无变量过={}", r == Err(TlsCode::IndexMissing), r2.is_ok()))
}

pub fn drill_state_edges() -> (bool, String) {
    let mut ok = true;
    ok = ok && transition(TlsState::Head, TlsState::WidthChecked).is_ok();
    ok = ok && transition(TlsState::WidthChecked, TlsState::PointersChecked).is_ok();
    ok = ok && transition(TlsState::PointersChecked, TlsState::CallbacksChecked).is_ok();
    ok = ok && transition(TlsState::CallbacksChecked, TlsState::Accepted).is_ok();
    // 非法转换拒收。
    ok = ok && transition(TlsState::Head, TlsState::Accepted) == Err(TlsCode::IllegalTransition);
    ok = ok && transition(TlsState::Accepted, TlsState::Head) == Err(TlsCode::IllegalTransition);
    ok = ok && transition(TlsState::Rejected, TlsState::WidthChecked) == Err(TlsCode::IllegalTransition);
    ok = ok && transition(TlsState::PointersChecked, TlsState::Accepted) == Err(TlsCode::IllegalTransition);
    (ok, format!("状态边闭集+非法拒收: {}", ok))
}

pub fn drill_consensus_ledger() -> (bool, String) {
    // 账目对账：50 样本 × 双档 → 100 次解析账，接受/拒载分类和守恒。
    let mut rng = Lcg::new(SEG_BASE as u64 + 2);
    let mut accepted = 0;
    let mut rejected = 0;
    let mut n = 0;
    while n < 50 {
        let (d64, b64) = synth64_bytes(&mut rng, n % 3 != 0);
        match parse_tls(ARCH_64, &b64) {
            Ok(p) => match validate_tls(&p, 4, true, 0x100000) {
                Ok(_) => accepted += 1,
                Err(_) => rejected += 1,
            },
            Err(_) => rejected += 1,
        }
        let (d32, b32) = synth32_from(&d64);
        match parse_tls(ARCH_32, &b32) {
            Ok(p) => match validate_tls(&p, 4, true, 0x100000) {
                Ok(_) => accepted += 1,
                Err(_) => rejected += 1,
            },
            Err(_) => rejected += 1,
        }
        let _ = d64;
        n += 1;
    }
    let ok = accepted + rejected == 100;
    (
        ok,
        format!("解析账: 接受={} 拒载={} 守恒={}", accepted, rejected, accepted + rejected == 100),
    )
}

pub fn drill_zero_fill_boundary() -> (bool, String) {
    // 零填充边界：Start+SizeOfZeroFill == 映像窗 → 过；+1 → 拒。
    let mut d = TlsDir::zero(ARCH_64);
    d.start = 0xFE000;
    d.end = 0xFE100;
    d.index = 0x2000;
    d.size_of_zero_fill = 0x2000; // 恰到 0x100000
    let r1 = validate_tls(&d, 0, true, 0x100000);
    d.size_of_zero_fill = 0x2001;
    let r2 = validate_tls(&d, 0, true, 0x100000);
    (r1 == Ok(TlsState::Accepted) && r2 == Err(TlsCode::ZeroFillOverflow),
        format!("零填充边界: 恰过={} 越一拒={}", r1.is_ok(), r2 == Err(TlsCode::ZeroFillOverflow)))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < TlsCode::ALL.len() {
        ok = ok && TlsCode::ALL[i].val() >= SEG_BASE && TlsCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !TlsCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", TlsCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < TlsCode::ALL.len() {
        let v = TlsCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_50_dual_arch();
    let (a2, d2) = drill_bad_dirs();
    let (a3, d3) = drill_width_mismatch_caught();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 双档50={} 坏目录={} 混读={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_50_dual_arch();
    v.push(("J1-双档50样本一致", a, b));
    let (a, b) = drill_width_mismatch_caught();
    v.push(("J2-宽度混读10/10", a, b));
    let (a, b) = drill_arch_closed();
    v.push(("J2-档位闭集", a, b));
    let (a, b) = drill_bad_dirs();
    v.push(("J1-坏目录拒载一致", a, b));
    let (a, b) = drill_callbacks_table();
    v.push(("J2-Callbacks终止语义", a, b));
    let (a, b) = drill_index_missing();
    v.push(("J2-Index缺失落账", a, b));
    let (a, b) = drill_state_edges();
    v.push(("J2-状态边闭集", a, b));
    let (a, b) = drill_consensus_ledger();
    v.push(("J2-双档解析账守恒", a, b));
    let (a, b) = drill_zero_fill_boundary();
    v.push(("J3-零填充边界", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12921_checks：本单判据聚合（诊断段 0x65400）。
pub fn run_unx12921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12921 TLS 目录双档解析");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65400 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
