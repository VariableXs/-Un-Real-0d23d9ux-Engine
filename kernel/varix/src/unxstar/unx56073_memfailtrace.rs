//! UNX-F56073 内存采集失败痕迹件（0x60900 段）
//!
//! 锚点：UNX-F56073-J1 失败痕迹登记全量（采集失败逐次落痕：阶段 × 症状
//! 码 × 波次）；J2 痕迹可复核（痕迹码闭集 + 症状指纹与登记逐条一致）；
//! J3 失败审计链（痕迹链复合、封账只读、失败统计闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·失败痕迹面）。
//!
//! 结构：失败阶段闭集（打开/采样/换算/提交四阶段）+ 症状码闭集（权限/
//! 超时/溢出/无响应四症状）+ 痕迹登记册（阶段 × 症状 × 波次逐次落痕）+
//! 痕迹链复合核（前痕指纹混入后痕防篡改）+ 失败统计闭式（阶段×症状
//! 矩阵计数）+ 注入核（未知阶段/未知症状/空痕/封账改写/未登记复核算
//! 必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60900 独占 0x20）。
pub const SEG_BASE: u32 = 0x60900;
pub const FT_UNK_STAGE: u32 = SEG_BASE + 0x01; // 未知失败阶段（闭集外）
pub const FT_UNK_SYMPTOM: u32 = SEG_BASE + 0x02; // 未知症状码（闭集外）
pub const FT_DESC_EMPTY: u32 = SEG_BASE + 0x03; // 痕迹描述为空
pub const FT_SEALED: u32 = SEG_BASE + 0x04; // 痕迹账已封账拒写
pub const FT_NOT_FOUND: u32 = SEG_BASE + 0x05; // 复核算未在痕迹册
pub const FT_WAVE_ZERO: u32 = SEG_BASE + 0x06; // 波次 0（哨兵拒收）
pub const FT_MATRIX_OVERFLOW: u32 = SEG_BASE + 0x07; // 统计矩阵越界

/// 失败阶段闭集。
pub const S_OPEN: usize = 0;
pub const S_SAMPLE: usize = 1;
pub const S_CONVERT: usize = 2;
pub const S_COMMIT: usize = 3;
pub const S_COUNT: usize = 4;
pub const S_NAMES: [&str; S_COUNT] = ["open", "sample", "convert", "commit"];

/// 症状码闭集。
pub const Y_PERM: usize = 0;
pub const Y_TIMEOUT: usize = 1;
pub const Y_OVERFLOW: usize = 2;
pub const Y_NORESP: usize = 3;
pub const Y_COUNT: usize = 4;
pub const Y_NAMES: [&str; Y_COUNT] = ["perm_denied", "timeout", "counter_overflow", "no_response"];

/// 痕迹指纹：FNV-1a 32 位。
pub fn trace_fingerprint(stage: usize, symptom: usize, wave: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", S_NAMES[stage], Y_NAMES[symptom], wave);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单次失败痕迹：阶段 × 症状 × 波次 × 描述 × 指纹链。
#[derive(Clone)]
pub struct TraceEntry {
    pub stage: usize,
    pub symptom: usize,
    pub wave: u32,
    pub desc: String,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 失败痕迹总账。
pub struct TraceBook {
    pub entries: Vec<TraceEntry>,
    pub sealed: bool,
}

impl TraceBook {
    pub fn new() -> Self {
        TraceBook { entries: Vec::new(), sealed: false }
    }

    /// 逐次落痕：阶段/症状闭集 + 非空描述 + 波次 ≥1 四闸，指纹链复合。
    pub fn record(&mut self, stage: usize, symptom: usize, wave: u32, desc: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        if stage >= S_COUNT {
            return Err(FT_UNK_STAGE);
        }
        if symptom >= Y_COUNT {
            return Err(FT_UNK_SYMPTOM);
        }
        if desc.is_empty() {
            return Err(FT_DESC_EMPTY);
        }
        if wave == 0 {
            return Err(FT_WAVE_ZERO);
        }
        let fp = trace_fingerprint(stage, symptom, wave);
        let chain = match self.entries.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.entries.push(TraceEntry {
            stage,
            symptom,
            wave,
            desc: desc.to_string(),
            fp,
            chain,
        });
        Ok(chain)
    }

    /// 复核第 i 条痕迹（越界显性拒绝）。
    pub fn trace_at(&self, i: usize) -> Result<(usize, usize, u32), u32> {
        if i >= self.entries.len() {
            return Err(FT_NOT_FOUND);
        }
        let e = &self.entries[i];
        Ok((e.stage, e.symptom, e.wave))
    }

    /// J1：痕迹全量（逐条阶段/症状合法 + 描述非空）。
    pub fn traces_ok(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| e.stage < S_COUNT && e.symptom < Y_COUNT && !e.desc.is_empty())
    }

    /// J2：痕迹指纹逐条一致（复算指纹与登记指纹相等）。
    pub fn fingerprints_ok(&self) -> bool {
        self.entries.iter().all(|e| e.fp == trace_fingerprint(e.stage, e.symptom, e.wave))
    }

    /// J3：痕迹链复核（逐条 chain = 前痕 fp ^ 本痕 fp，首痕对种子）。
    pub fn chain_ok(&self) -> bool {
        for i in 0..self.entries.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.entries[0].fp
            } else {
                self.entries[i - 1].fp ^ self.entries[i].fp
            };
            if self.entries[i].chain != expect {
                return false;
            }
        }
        !self.entries.is_empty()
    }

    /// J3：阶段×症状统计矩阵（计数闭式：矩阵和 = 痕迹总数）。
    pub fn stat_matrix(&self) -> Result<[[u32; Y_COUNT]; S_COUNT], u32> {
        if self.entries.is_empty() {
            return Err(FT_NOT_FOUND);
        }
        let mut m = [[0u32; Y_COUNT]; S_COUNT];
        let mut total = 0u32;
        for e in self.entries.iter() {
            if e.stage >= S_COUNT || e.symptom >= Y_COUNT {
                return Err(FT_MATRIX_OVERFLOW);
            }
            m[e.stage][e.symptom] += 1;
            total += 1;
        }
        let mut sum = 0u32;
        for row in m.iter() {
            for v in row.iter() {
                sum += *v;
            }
        }
        if sum != total {
            return Err(FT_MATRIX_OVERFLOW);
        }
        Ok(m)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 链尾指纹（封账材料）。
    pub fn chain_tail(&self) -> u32 {
        match self.entries.last() {
            None => CHAIN_SEED,
            Some(e) => e.chain,
        }
    }
}

/// 标准夹具：4 次失败痕迹（两阶段两症状，同波次）。
pub fn standard_book() -> TraceBook {
    let mut b = TraceBook::new();
    let plan: [(usize, usize, u32, &str); 4] = [
        (S_OPEN, Y_PERM, 5, "smaps 打开被拒"),
        (S_SAMPLE, Y_TIMEOUT, 5, "采样窗等待超时"),
        (S_CONVERT, Y_OVERFLOW, 5, "页计数溢出"),
        (S_COMMIT, Y_NORESP, 5, "提交无响应"),
    ];
    for p in plan.iter() {
        if b.record(p.0, p.1, p.2, p.3).is_err() {
            panic!("standard fixture record must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知阶段/未知症状/空描述/波次零/封账改写/复核算越界/空账矩阵）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知阶段
    let mut b = TraceBook::new();
    if b.record(9, Y_PERM, 1, "x") == Err(FT_UNK_STAGE) {
        caught += 1;
    }
    // 2) 未知症状
    if b.record(S_OPEN, 9, 1, "x") == Err(FT_UNK_SYMPTOM) {
        caught += 1;
    }
    // 3) 空描述
    if b.record(S_OPEN, Y_PERM, 1, "") == Err(FT_DESC_EMPTY) {
        caught += 1;
    }
    // 4) 波次零
    if b.record(S_OPEN, Y_PERM, 0, "x") == Err(FT_WAVE_ZERO) {
        caught += 1;
    }
    // 5) 复核算越界
    let sb = standard_book();
    if sb.trace_at(9).err() == Some(FT_NOT_FOUND) {
        caught += 1;
    }
    // 6) 空账统计矩阵拒绝
    if TraceBook::new().stat_matrix().is_err() {
        caught += 1;
    }
    // 7) 封账后改写拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.record(S_OPEN, Y_PERM, 6, "late") == Err(FT_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 痕迹账渲染导出（逐痕一行）。
pub fn render_book(b: &TraceBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!(
            "{}|{}|w{}|{}|{:08x}\n",
            S_NAMES[e.stage], Y_NAMES[e.symptom], e.wave, e.desc, e.fp
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行五段 + 阶段名闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    for ln in lines.iter() {
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 5 {
            return false;
        }
        let mut hit = false;
        for s in S_NAMES.iter() {
            if parts[0] == *s {
                hit = true;
            }
        }
        if !hit {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56073");
    // J1-1 痕迹登记全量（四痕逐条合法）
    let b = standard_book();
    let ok = b.traces_ok() && b.entries.len() == 4;
    s.add("56073-J1-痕迹登记全量", ok, if ok { "" } else { "痕迹残缺" });
    // J1-2 阶段与症状闭集齐
    let ok = S_NAMES.len() == S_COUNT && Y_NAMES.len() == Y_COUNT;
    s.add("56073-J1-闭集齐", ok, if ok { "" } else { "闭集漂移" });
    // J1-3 复核算逐条命中
    let ok = b.trace_at(0) == Ok((S_OPEN, Y_PERM, 5)) && b.trace_at(3) == Ok((S_COMMIT, Y_NORESP, 5));
    s.add("56073-J1-复核算命中", ok, if ok { "" } else { "复核漂移" });
    // J2-1 痕迹指纹逐条一致
    let ok = b.fingerprints_ok();
    s.add("56073-J2-指纹逐条一致", ok, if ok { "" } else { "指纹漂移" });
    // J2-2 指纹确定性（同输入同指纹）
    let ok = trace_fingerprint(S_SAMPLE, Y_TIMEOUT, 5) == trace_fingerprint(S_SAMPLE, Y_TIMEOUT, 5)
        && trace_fingerprint(S_SAMPLE, Y_TIMEOUT, 5) != trace_fingerprint(S_SAMPLE, Y_TIMEOUT, 6);
    s.add("56073-J2-指纹确定性", ok, if ok { "" } else { "指纹碰撞" });
    // J2-3 统计矩阵闭式（4 痕各占一格）
    let m = b.stat_matrix();
    let ok = match m {
        Ok(m) => m[S_OPEN][Y_PERM] == 1 && m[S_SAMPLE][Y_TIMEOUT] == 1
            && m[S_CONVERT][Y_OVERFLOW] == 1 && m[S_COMMIT][Y_NORESP] == 1,
        Err(_) => false,
    };
    s.add("56073-J2-统计矩阵闭式", ok, if ok { "" } else { "矩阵计数漂移" });
    // J2-4 复核算越界显性拒绝
    let ok = b.trace_at(4).err() == Some(FT_NOT_FOUND);
    s.add("56073-J2-复核越界拒绝", ok, if ok { "" } else { "越界语义漂移" });
    // J3-1 痕迹链复核全绿
    let ok = b.chain_ok();
    s.add("56073-J3-痕迹链复核", ok, if ok { "" } else { "痕迹链断裂" });
    // J3-2 链尾敏感（追加一痕链尾变）
    let tail0 = b.chain_tail();
    let mut b2 = standard_book();
    let _ = b2.record(S_SAMPLE, Y_PERM, 6, "追加一痕");
    let ok = b2.chain_tail() != tail0;
    s.add("56073-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-3 链尾确定性
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56073-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-4 封账拒写
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.record(S_OPEN, Y_PERM, 7, "late") == Err(FT_SEALED);
    s.add("56073-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出四行五段齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 4) && text.contains("open|perm_denied|w5|");
    s.add("56073-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56073-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56073_all_checks_green() {
        let set = run_unx56073_checks();
        assert!(set.all_passed(), "UNX-F56073 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56073_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
