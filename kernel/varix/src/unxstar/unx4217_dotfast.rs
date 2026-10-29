//! unx4217_dotfast — UNX-F4217 空组件/点组件的性能特例（诊断段 0xA060）。
//!
//! 判据原文：
//! - J1 连续 `.` 与 `//` 混合路径快速通道（不穿透 fs 层）延迟账低于常规路径（可测量）；
//! - J2 快速通道与常规路径结果逐字节一致（1,000 例）；
//! - J3 延迟账口径：以 fs 层触碰次数为代理度量，快速通道触碰数 <= 常规路径；
//! - J4 边界显性：空输入/超深路径/非法分量拒收并记错误账；
//! - J5 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//! - D1 常规路径逐分量模拟 fs 层触碰（每真实分量 +1）；
//! - D2 快速通道把空分量与 `.` 在词法层吞掉，仅真实分量触碰 fs 层；
//! - D3 两通道共享同一词法核，保证逐字节一致；
//! - D4 负载确定性生成（序号驱动），1,000 例固定;
//! - D5 drill 分步：等价对照→延迟对账→注入拦截→1,000 例批量。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xA060;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY: u16 = SEG_BASE + 1;     // 空路径拒收
const E_NOT_ABS: u16 = SEG_BASE + 2;   // 非绝对路径拒收
const E_DEPTH: u16 = SEG_BASE + 3;     // 路径深度超 4096 上限
const E_MISMATCH: u16 = SEG_BASE + 4;  // 两通道结果逐字节不一致
const E_SLOWER: u16 = SEG_BASE + 5;    // 快速通道触碰数大于常规路径
const E_BAD_CASE: u16 = SEG_BASE + 6;  // 对照例号越界
const E_BATCH: u16 = SEG_BASE + 7;     // 批量对照例数不足 1000
const E_NULL_BYTE: u16 = SEG_BASE + 8; // 分量含 NUL 非法字节

/// 批量对照例数（判据 J2）。
pub const BATCH_CASES: u32 = 1000;
/// 路径深度上限（分量数），超限拒收。
pub const DEPTH_CAP: usize = 4096;

/// 延迟账：一次解析的 fs 层触碰记账快照。
#[derive(Clone, PartialEq)]
pub struct LatencySnap {
    /// fs 层触碰次数（延迟代理度量）。
    pub fs_touches: u32,
    /// 吞掉的空分量个数。
    pub skipped_empty: u32,
    /// 吞掉的点分量个数。
    pub skipped_dot: u32,
    /// 有效分量个数。
    pub real_comps: u32,
}

/// 解析通道状态机。
pub enum Chan {
    /// 常规通道：逐分量穿透 fs 层。
    Regular,
    /// 快速通道：空分量/点分量词法吞并。
    Fast,
}

/// 解析器：按通道执行并记延迟账。
pub struct DotFast {
    /// 本轮通道。
    pub chan: Chan,
    /// 最近一次延迟账快照。
    pub last: Option<LatencySnap>,
    /// 错误账次数。
    pub err_count: u32,
}

impl DotFast {
    /// 建常规通道解析器。
    pub fn new_regular() -> Self {
        DotFast { chan: Chan::Regular, last: None, err_count: 0 }
    }

    /// 建快速通道解析器。
    pub fn new_fast() -> Self {
        DotFast { chan: Chan::Fast, last: None, err_count: 0 }
    }

    /// 解析（返回规范化路径 owned + 记延迟账）。空输入/越深/NUL 全拒收。
    pub fn resolve(&mut self, path: &str) -> Result<String, u16> {
        match self.resolve_inner(path) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.err_count += 1;
                Err(e)
            }
        }
    }

    fn resolve_inner(&mut self, path: &str) -> Result<String, u16> {
        if path.is_empty() {
            return Err(E_EMPTY);
        }
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let mut stack: Vec<&str> = Vec::new();
        let mut snap = LatencySnap { fs_touches: 0, skipped_empty: 0, skipped_dot: 0, real_comps: 0 };
        for seg in path.split('/') {
            if seg.contains('\0') {
                return Err(E_NULL_BYTE);
            }
            match seg {
                "" => {
                    snap.skipped_empty += 1;
                    match self.chan {
                        // 常规通道：空分量也要经 fs 层查找目录项（计一次触碰）。
                        Chan::Regular => snap.fs_touches += 1,
                        Chan::Fast => {}
                    }
                }
                "." => {
                    snap.skipped_dot += 1;
                    match self.chan {
                        // 常规通道：`.` 触发 readdir 确认自指（计一次触碰）。
                        Chan::Regular => snap.fs_touches += 1,
                        Chan::Fast => {}
                    }
                }
                ".." => {
                    snap.real_comps += 1;
                    stack.pop();
                    snap.fs_touches += 1;
                }
                _ => {
                    if stack.len() >= DEPTH_CAP {
                        return Err(E_DEPTH);
                    }
                    snap.real_comps += 1;
                    stack.push(seg);
                    snap.fs_touches += 1;
                }
            }
        }
        self.last = Some(LencyClone::clone_snap(&snap));
        let mut out = String::new();
        if stack.is_empty() {
            out.push('/');
            return Ok(out);
        }
        for seg in &stack {
            out.push('/');
            out.push_str(seg);
        }
        Ok(out)
    }

    /// 最近延迟账快照（owned）。
    pub fn latency(&self) -> Result<LatencySnap, u16> {
        match &self.last {
            Some(s) => Ok(LencyClone::clone_snap(s)),
            None => Err(E_BAD_CASE),
        }
    }

    /// 错误账读取。
    pub fn errs(&self) -> u32 {
        self.err_count
    }
}

/// 快照克隆小工具（避免为 LatencySnap 引入外部 trait 依赖歧义）。
struct LencyClone;

impl LencyClone {
    fn clone_snap(s: &LatencySnap) -> LatencySnap {
        LatencySnap {
            fs_touches: s.fs_touches,
            skipped_empty: s.skipped_empty,
            skipped_dot: s.skipped_dot,
            real_comps: s.real_comps,
        }
    }
}

/// 确定性混合负载：第 i 例生成一条含空分量与点分量的路径。
pub fn gen_case(i: u32) -> String {
    let roots = ["/a", "/proc", "/mnt", "/tmp"];
    let parts = [".", ".", "b", "", "c", ".", "", "d"];
    let mut s = String::new();
    s.push_str(roots[(i % 4) as usize]);
    for k in 0..6 {
        s.push('/');
        s.push_str(parts[((i + k * 3) % 8) as usize]);
    }
    if i % 7 == 0 {
        s.push_str("/../z");
    }
    s
}

/// 单例等价对照：两通道结果逐字节一致且快速通道不劣于常规（J1/J2 单例）。
pub fn compare_one(i: u32) -> Result<(String, LatencySnap, LatencySnap), u16> {
    if i >= BATCH_CASES {
        return Err(E_BAD_CASE);
    }
    let path = gen_case(i);
    let mut reg = DotFast::new_regular();
    let mut fst = DotFast::new_fast();
    let a = reg.resolve(&path)?;
    let b = fst.resolve(&path)?;
    if a != b {
        return Err(E_MISMATCH);
    }
    let lr = reg.latency()?;
    let lf = fst.latency()?;
    if lf.fs_touches > lr.fs_touches {
        return Err(E_SLOWER);
    }
    Ok((a, lr, lf))
}

/// 批量对照 1,000 例（判据 J2），并累计延迟差总量（判据 J1 可测量口径）。
pub fn compare_batch() -> Result<(u32, u64), u16> {
    let mut done: u32 = 0;
    let mut saved: u64 = 0;
    for i in 0..BATCH_CASES {
        let (_, lr, lf) = compare_one(i)?;
        saved += (lr.fs_touches - lf.fs_touches) as u64;
        done += 1;
    }
    if done != BATCH_CASES {
        return Err(E_BATCH);
    }
    Ok((done, saved))
}

/// 结果摘要（判据 J5）。
pub fn summary(done: u32, saved: u64) -> String {
    let mut s = String::new();
    s.push_str("dotfast: cases=");
    push_u32(&mut s, done);
    s.push_str(" saved_touches=");
    push_u64(&mut s, saved);
    s
}

/// u32 十进制追加。
fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// u64 十进制追加。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for k in i..buf.len() {
        s.push(buf[k] as char);
    }
}

/// drill 自例程：R1 等价对照 → R2 延迟对账 → R3 注入拦截 → R4 1,000 例批量。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 单例等价：结果一致、快速通道触碰更少。
    let (p, lr, lf) = compare_one(3)?;
    if p.is_empty() {
        return Err(E_MISMATCH);
    }
    if lf.fs_touches > lr.fs_touches {
        return Err(E_SLOWER);
    }
    hits += 1;

    // R2 延迟对账：快速通道在含空/点分量负载上必然省触碰（J1 可测量）。
    let mut reg = DotFast::new_regular();
    let mut fst = DotFast::new_fast();
    let path = gen_case(0);
    reg.resolve(&path)?;
    fst.resolve(&path)?;
    let a = reg.latency()?;
    let b = fst.latency()?;
    if b.fs_touches >= a.fs_touches {
        return Err(E_SLOWER);
    }
    hits += 1;

    // R3 注入拦截：空输入/相对路径/越界例号全拒收并计错。
    let mut bad = DotFast::new_fast();
    if !matches!(bad.resolve(""), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    if !matches!(bad.resolve("a/b"), Err(E_NOT_ABS)) {
        return Err(E_NOT_ABS);
    }
    if !matches!(compare_one(BATCH_CASES), Err(E_BAD_CASE)) {
        return Err(E_BAD_CASE);
    }
    if bad.errs() != 2 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R4 批量 1,000 例：逐字节一致 + 省触碰总量有账（J1/J2）。
    let (done, saved) = compare_batch()?;
    if done != BATCH_CASES || saved == 0 {
        return Err(E_BATCH);
    }
    if summary(done, saved).is_empty() {
        return Err(E_BATCH);
    }
    hits += 1;

    Ok(hits)
}
