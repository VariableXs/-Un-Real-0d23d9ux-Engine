//! UNX-F56116 IO 采集失败痕迹件（0x60E60 段）
//!
//! 锚点：UNX-F56116-J1 失败痕迹全量（采集失败逐次落痕：阶段 × 症状 ×
//! 波次三元组）；J2 痕迹可复核（阶段/症状闭集、指纹逐条复算一致）；J3
//! 失败审计链（痕迹链复合、封账只读、矩阵统计闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·失败痕迹面）。
//!
//! 结构：失败阶段闭集（脚本分发/执行/采样/提交四阶段）+ 症状码闭集
//! （脚本漂移/盘忙/超时/无响应四症状）+ 痕迹登记册 + 痕迹链复合核 +
//! 阶段×症状矩阵统计 + 注入核（未知阶段/未知症状/空描述/波次零/封账
//! 改写/复核算越界/空账矩阵必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60E60 独占 0x20）。
pub const SEG_BASE: u32 = 0x60E60;
pub const IF_UNK_STAGE: u32 = SEG_BASE + 0x01; // 未知失败阶段
pub const IF_UNK_SYMPTOM: u32 = SEG_BASE + 0x02; // 未知症状码
pub const IF_DESC_EMPTY: u32 = SEG_BASE + 0x03; // 痕迹描述为空
pub const IF_SEALED: u32 = SEG_BASE + 0x04; // 痕迹账已封账拒写
pub const IF_NOT_FOUND: u32 = SEG_BASE + 0x05; // 复核算越界
pub const IF_WAVE_ZERO: u32 = SEG_BASE + 0x06; // 波次零
pub const IF_MATRIX_OVERFLOW: u32 = SEG_BASE + 0x07; // 矩阵越界

/// 失败阶段闭集。
pub const S_DISPATCH: usize = 0;
pub const S_EXECUTE: usize = 1;
pub const S_SAMPLE: usize = 2;
pub const S_COMMIT: usize = 3;
pub const S_COUNT: usize = 4;
pub const S_NAMES: [&str; S_COUNT] = ["dispatch", "execute", "sample", "commit"];

/// 症状码闭集。
pub const Y_SCRIPT_DRIFT: usize = 0;
pub const Y_DISK_BUSY: usize = 1;
pub const Y_TIMEOUT: usize = 2;
pub const Y_NORESP: usize = 3;
pub const Y_COUNT: usize = 4;
pub const Y_NAMES: [&str; Y_COUNT] =
    ["script_drift", "disk_busy", "timeout", "no_response"];

/// 痕迹指纹：FNV-1a 32 位。
pub fn trace_fingerprint(stage: usize, symptom: usize, wave: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let s = if stage < S_COUNT { S_NAMES[stage] } else { "?" };
    let y = if symptom < Y_COUNT { Y_NAMES[symptom] } else { "?" };
    let line = format!("{}|{}|{}", s, y, wave);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 失败痕迹：阶段 × 症状 × 波次 × 描述 × 指纹 × 链。
#[derive(Clone)]
pub struct IoTrace {
    pub stage: usize,
    pub symptom: usize,
    pub wave: u32,
    pub desc: String,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 痕迹总账。
pub struct IoTraceBook {
    pub traces: Vec<IoTrace>,
    pub sealed: bool,
}

impl IoTraceBook {
    pub fn new() -> Self {
        IoTraceBook { traces: Vec::new(), sealed: false }
    }

    /// 逐次落痕：阶段/症状闭集 + 非空描述 + 波次 ≥1 四闸。
    pub fn record(&mut self, stage: usize, symptom: usize, wave: u32, desc: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(IF_SEALED);
        }
        if stage >= S_COUNT {
            return Err(IF_UNK_STAGE);
        }
        if symptom >= Y_COUNT {
            return Err(IF_UNK_SYMPTOM);
        }
        if desc.is_empty() {
            return Err(IF_DESC_EMPTY);
        }
        if wave == 0 {
            return Err(IF_WAVE_ZERO);
        }
        let fp = trace_fingerprint(stage, symptom, wave);
        let chain = match self.traces.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.traces.push(IoTrace {
            stage,
            symptom,
            wave,
            desc: desc.to_string(),
            fp,
            chain,
        });
        Ok(chain)
    }

    /// 复核第 i 条痕迹。
    pub fn trace_at(&self, i: usize) -> Result<(usize, usize, u32), u32> {
        if i >= self.traces.len() {
            return Err(IF_NOT_FOUND);
        }
        let t = &self.traces[i];
        Ok((t.stage, t.symptom, t.wave))
    }

    /// J1：痕迹全量（逐条合法）。
    pub fn traces_ok(&self) -> bool {
        !self.traces.is_empty()
            && self
                .traces
                .iter()
                .all(|t| t.stage < S_COUNT && t.symptom < Y_COUNT && !t.desc.is_empty())
    }

    /// J2：指纹逐条复算一致。
    pub fn fingerprints_ok(&self) -> bool {
        self.traces
            .iter()
            .all(|t| t.fp == trace_fingerprint(t.stage, t.symptom, t.wave))
    }

    /// J3：痕迹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.traces.is_empty() {
            return false;
        }
        for i in 0..self.traces.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.traces[0].fp
            } else {
                self.traces[i - 1].fp ^ self.traces[i].fp
            };
            if self.traces[i].chain != expect {
                return false;
            }
        }
        true
    }

    /// J3：阶段×症状矩阵统计闭式。
    pub fn stat_matrix(&self) -> Result<[[u32; Y_COUNT]; S_COUNT], u32> {
        if self.traces.is_empty() {
            return Err(IF_NOT_FOUND);
        }
        let mut m = [[0u32; Y_COUNT]; S_COUNT];
        let mut total = 0u32;
        for t in self.traces.iter() {
            if t.stage >= S_COUNT || t.symptom >= Y_COUNT {
                return Err(IF_MATRIX_OVERFLOW);
            }
            m[t.stage][t.symptom] += 1;
            total += 1;
        }
        let mut sum = 0u32;
        for row in m.iter() {
            for v in row.iter() {
                sum += *v;
            }
        }
        if sum != total {
            return Err(IF_MATRIX_OVERFLOW);
        }
        Ok(m)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.traces.last() {
            None => CHAIN_SEED,
            Some(t) => t.chain,
        }
    }
}

/// 标准夹具：4 痕（四阶段各一症状）。
pub fn standard_book() -> IoTraceBook {
    let mut b = IoTraceBook::new();
    let plan: [(usize, usize, u32, &str); 4] = [
        (S_DISPATCH, Y_SCRIPT_DRIFT, 5, "脚本签名漂移拒分发"),
        (S_EXECUTE, Y_DISK_BUSY, 5, "盘忙执行中断"),
        (S_SAMPLE, Y_TIMEOUT, 5, "采样窗超时"),
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
    let mut b = IoTraceBook::new();
    if b.record(9, Y_TIMEOUT, 1, "x") == Err(IF_UNK_STAGE) {
        caught += 1;
    }
    // 2) 未知症状
    if b.record(S_SAMPLE, 9, 1, "x") == Err(IF_UNK_SYMPTOM) {
        caught += 1;
    }
    // 3) 空描述
    if b.record(S_SAMPLE, Y_TIMEOUT, 1, "") == Err(IF_DESC_EMPTY) {
        caught += 1;
    }
    // 4) 波次零
    if b.record(S_SAMPLE, Y_TIMEOUT, 0, "x") == Err(IF_WAVE_ZERO) {
        caught += 1;
    }
    // 5) 复核算越界
    if standard_book().trace_at(9).err() == Some(IF_NOT_FOUND) {
        caught += 1;
    }
    // 6) 空账矩阵
    if IoTraceBook::new().stat_matrix().is_err() {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.record(S_SAMPLE, Y_TIMEOUT, 6, "late") == Err(IF_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 痕迹账渲染导出。
pub fn render_book(b: &IoTraceBook) -> String {
    let mut out = String::new();
    for t in b.traces.iter() {
        out.push_str(&format!(
            "{}|{}|w{}|{}|{:08x}\n",
            S_NAMES[t.stage], Y_NAMES[t.symptom], t.wave, t.desc, t.fp
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行五段 + 阶段闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
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
        hit
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56116_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56116");
    // J1-1 痕迹全量（四痕逐条合法）
    let b = standard_book();
    let ok = b.traces_ok() && b.traces.len() == 4;
    s.add("56116-J1-痕迹全量", ok, if ok { "" } else { "痕迹残缺" });
    // J1-2 阶段与症状闭集齐
    let ok = S_NAMES.len() == S_COUNT && Y_NAMES.len() == Y_COUNT;
    s.add("56116-J1-闭集齐", ok, if ok { "" } else { "闭集漂移" });
    // J1-3 复核算逐条命中
    let ok = b.trace_at(0) == Ok((S_DISPATCH, Y_SCRIPT_DRIFT, 5))
        && b.trace_at(3) == Ok((S_COMMIT, Y_NORESP, 5));
    s.add("56116-J1-复核算命中", ok, if ok { "" } else { "复核漂移" });
    // J2-1 指纹逐条一致
    let ok = b.fingerprints_ok();
    s.add("56116-J2-指纹逐条一致", ok, if ok { "" } else { "指纹漂移" });
    // J2-2 指纹确定性
    let ok = trace_fingerprint(S_EXECUTE, Y_DISK_BUSY, 5)
        == trace_fingerprint(S_EXECUTE, Y_DISK_BUSY, 5)
        && trace_fingerprint(S_EXECUTE, Y_DISK_BUSY, 5)
            != trace_fingerprint(S_EXECUTE, Y_DISK_BUSY, 6);
    s.add("56116-J2-指纹确定性", ok, if ok { "" } else { "指纹碰撞" });
    // J2-3 矩阵闭式（四痕各占一格）
    let m = b.stat_matrix();
    let ok = match m {
        Ok(m) => m[S_DISPATCH][Y_SCRIPT_DRIFT] == 1
            && m[S_EXECUTE][Y_DISK_BUSY] == 1
            && m[S_SAMPLE][Y_TIMEOUT] == 1
            && m[S_COMMIT][Y_NORESP] == 1,
        Err(_) => false,
    };
    s.add("56116-J2-矩阵闭式", ok, if ok { "" } else { "矩阵漂移" });
    // J2-4 复核算越界显性拒绝
    let ok = b.trace_at(4).err() == Some(IF_NOT_FOUND);
    s.add("56116-J2-越界拒绝", ok, if ok { "" } else { "越界语义漂移" });
    // J3-1 痕迹链复核
    let ok = b.chain_ok();
    s.add("56116-J3-痕迹链复核", ok, if ok { "" } else { "痕迹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56116-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（追加一痕链尾变）
    let mut b3 = standard_book();
    let _ = b3.record(S_SAMPLE, Y_DISK_BUSY, 6, "追加一痕");
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56116-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.record(S_SAMPLE, Y_TIMEOUT, 7, "late") == Err(IF_SEALED);
    s.add("56116-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出四行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 4) && text.starts_with("dispatch|script_drift|w5|");
    s.add("56116-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56116-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56116_all_checks_green() {
        let set = run_unx56116_checks();
        assert!(set.all_passed(), "UNX-F56116 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56116_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
