//! unx0974_regsum — UNX-F0974 中断面回归汇总档（B01–B08 断言总矩阵）。
//!
//! 任务锚点：UNX-F0974（第二批工位 W7，SEG 段位 0x7FC0，本单独占
//! 0x7FC0..=0x7FDF；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**B01–B08 八批回归断言总矩阵入档（断言
//! 零遗漏、通过/失败/跳过三态守恒），失败断言逐条带码可追，总矩阵
//! 通过率按万分数落账且双导出指纹稳定**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x7FC 段闭集）；tick 显性
//! 注入零墙钟；矩阵守恒（pass+fail+skip==total 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x7FC0 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面回归汇总段基址（0x7FC0，本单 0x7FC0..=0x7FDF 独占）。
pub const SEG_BASE: u16 = 0x7FC0;
/// 0x7FC1 断言 id 非法（零号/重复——矩阵身份不唯一）。
pub const XC_ASSERT_BAD: u16 = SEG_BASE + 1;
/// 0x7FC2 断言结果越界（结果码不在三态闭集内）。
pub const XC_RESULT_BAD: u16 = SEG_BASE + 2;
/// 0x7FC3 批次越界（批号不在 B01–B08 闭集内）。
pub const XC_BATCH_BAD: u16 = SEG_BASE + 3;
/// 0x7FC4 矩阵守恒破坏（三态和 != 总数——账面被外力改写）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 4;
/// 0x7FC5 失败断言无码（失败态必须带诊断码——可追性破坏）。
pub const XC_FAIL_NOCODE: u16 = SEG_BASE + 5;
/// 0x7FC6 重复出档（终稿唯一性破坏）。
pub const XC_FINAL_DUP: u16 = SEG_BASE + 6;
/// 0x7FC7 矩阵指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x7FC8 定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 8;
/// 0x7FC9 已定案断言改判（三态迁移只进不改——终态纪律破坏）。
pub const XC_VERDICT_FROZEN: u16 = SEG_BASE + 9;
/// 0x7FCA 批次集不齐（八批未收全即出终稿）。
pub const XC_BATCH_INCOMPLETE: u16 = SEG_BASE + 10;

const _: () = assert!(XC_ASSERT_BAD != XC_RESULT_BAD);
const _: () = assert!(XC_CONSERVATION != XC_FINAL_DUP);
const _: () = assert!(XC_FAIL_NOCODE != XC_VERDICT_FROZEN);

// ---------------------------------------------------------------------------
// §1 回归批次闭集（B01–B08）
// ---------------------------------------------------------------------------

/// 回归批次闭集（对齐卡面 B01–B08）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegBatch {
    /// B01 中断开合基本回归。
    B01Open,
    /// B02 中断向量分配回归。
    B02Vector,
    /// B03 EOI 路径回归。
    B03Eoi,
    /// B04 中断吞吐回归。
    B04Throughput,
    /// B05 中断延迟回归。
    B05Latency,
    /// B06 风暴检测回归。
    B06Storm,
    /// B07 节流降级回归。
    B07Throttle,
    /// B08 恢复序列回归。
    B08Recover,
}

impl RegBatch {
    /// 批号（1..=8）。
    pub const fn no(self) -> u8 {
        match self {
            RegBatch::B01Open => 1,
            RegBatch::B02Vector => 2,
            RegBatch::B03Eoi => 3,
            RegBatch::B04Throughput => 4,
            RegBatch::B05Latency => 5,
            RegBatch::B06Storm => 6,
            RegBatch::B07Throttle => 7,
            RegBatch::B08Recover => 8,
        }
    }
    /// 批名。
    pub const fn name(self) -> &'static str {
        match self {
            RegBatch::B01Open => "开合",
            RegBatch::B02Vector => "向量",
            RegBatch::B03Eoi => "EOI",
            RegBatch::B04Throughput => "吞吐",
            RegBatch::B05Latency => "延迟",
            RegBatch::B06Storm => "风暴",
            RegBatch::B07Throttle => "节流",
            RegBatch::B08Recover => "恢复",
        }
    }
    /// 由批号还原。
    pub const fn from_no(n: u8) -> Option<RegBatch> {
        match n {
            1 => Some(RegBatch::B01Open),
            2 => Some(RegBatch::B02Vector),
            3 => Some(RegBatch::B03Eoi),
            4 => Some(RegBatch::B04Throughput),
            5 => Some(RegBatch::B05Latency),
            6 => Some(RegBatch::B06Storm),
            7 => Some(RegBatch::B07Throttle),
            8 => Some(RegBatch::B08Recover),
            _ => None,
        }
    }
}

/// 批次总数。
pub const BATCH_COUNT: usize = 8;

// ---------------------------------------------------------------------------
// §2 断言三态闭集与断言行
// ---------------------------------------------------------------------------

/// 断言结果三态闭集（Pass/Fail/Skip——终态，只进不改）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 失败（必须带诊断码）。
    Fail,
    /// 跳过（前置未满足——不算失败但留痕）。
    Skip,
}

impl Verdict {
    /// 态码（1/2/3）。
    pub const fn code(self) -> u8 {
        match self {
            Verdict::Pass => 1,
            Verdict::Fail => 2,
            Verdict::Skip => 3,
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<Verdict> {
        match c {
            1 => Some(Verdict::Pass),
            2 => Some(Verdict::Fail),
            3 => Some(Verdict::Skip),
            _ => None,
        }
    }
}

/// 矩阵一行：断言 id / 批号 / 判定 / 失败码 / 入档 tick。
#[derive(Clone, Copy, Debug)]
struct AssertRow {
    id: u32,
    batch: RegBatch,
    verdict: Verdict,
    fail_code: u16,
    at_tick: u64,
}

// ---------------------------------------------------------------------------
// §3 断言总矩阵（RegMatrix）
// ---------------------------------------------------------------------------

/// 矩阵容量定值：64 行（八批各至多 8 条主断言——与卡面总矩阵规模对齐）。
pub const MATRIX_CAP: usize = 64;

/// 断言总矩阵——B01–B08 回归资产总编本体。
///
/// 守恒恒等式：`pass + fail + skip == total`（每行恰处一态，无第四
/// 态、无重复计数）；`audit_conservation()` 重扫机检此式。
#[derive(Debug)]
pub struct RegMatrix {
    rows: Vec<AssertRow>,
    cap: usize,
    total: u64,
    per_verdict: [u64; 3],
    per_batch: [u64; BATCH_COUNT],
    finalized: bool,
    final_fp: u64,
    rejected: u32,
}

impl RegMatrix {
    /// 建矩阵。
    pub fn new(cap: usize) -> Result<RegMatrix, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(RegMatrix {
            rows: Vec::with_capacity(cap),
            cap,
            total: 0,
            per_verdict: [0; 3],
            per_batch: [0; BATCH_COUNT],
            finalized: false,
            final_fp: 0,
            rejected: 0,
        })
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// 三态计数快照 [pass, fail, skip]。
    pub fn verdict_counts(&self) -> [u64; 3] {
        self.per_verdict
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 是否已定案。
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    /// 入档一条断言判定（终态一次定案——改判拒收；失败必须带码）。
    pub fn record(
        &mut self,
        id: u32,
        batch: RegBatch,
        verdict: Verdict,
        fail_code: u16,
        tick: u64,
    ) -> Result<(), u16> {
        if self.finalized {
            self.rejected += 1;
            return Err(XC_FINAL_DUP);
        }
        if id == 0 {
            self.rejected += 1;
            return Err(XC_ASSERT_BAD);
        }
        if RegBatch::from_no(batch.no()).is_none() {
            self.rejected += 1;
            return Err(XC_BATCH_BAD);
        }
        if Verdict::from_code(verdict.code()).is_none() {
            self.rejected += 1;
            return Err(XC_RESULT_BAD);
        }
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                if self.rows[i].verdict == verdict && self.rows[i].batch == batch {
                    self.rejected += 1;
                    return Err(XC_VERDICT_FROZEN);
                }
                self.rejected += 1;
                return Err(XC_ASSERT_BAD);
            }
            i += 1;
        }
        if verdict == Verdict::Fail && fail_code == 0 {
            self.rejected += 1;
            return Err(XC_FAIL_NOCODE);
        }
        if self.rows.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.rows.push(AssertRow {
            id,
            batch,
            verdict,
            fail_code,
            at_tick: tick,
        });
        self.total += 1;
        self.per_verdict[(verdict.code() - 1) as usize] += 1;
        self.per_batch[(batch.no() - 1) as usize] += 1;
        Ok(())
    }

    /// 守恒机检：三态和 == total 且逐批和 == total。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let s = self.per_verdict[0] + self.per_verdict[1] + self.per_verdict[2];
        if s != self.total {
            return Err(XC_CONSERVATION);
        }
        let mut bs: u64 = 0;
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            bs += self.per_batch[i];
            i += 1;
        }
        if bs != self.total {
            return Err(XC_CONSERVATION);
        }
        if self.total != self.rows.len() as u64 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 失败断言清单（id/批号/码——可追性出口）。
    pub fn failures(&self) -> Vec<(u32, u8, u16)> {
        let mut out: Vec<(u32, u8, u16)> = Vec::new();
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].verdict == Verdict::Fail {
                out.push((self.rows[i].id, self.rows[i].batch.no(), self.rows[i].fail_code));
            }
            i += 1;
        }
        out
    }

    /// 通过率（万分数基点——10000 即 100%；分母不含 skip）。
    pub fn pass_rate_bp(&self) -> u32 {
        let denom = self.per_verdict[0] + self.per_verdict[1];
        if denom == 0 {
            return 0;
        }
        ((self.per_verdict[0] * 10_000) / denom) as u32
    }

    /// 批次集齐备机检（八批均有行才可定案）。
    pub fn batches_complete(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            if self.per_batch[i] == 0 {
                return Err(XC_BATCH_INCOMPLETE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 定案（守恒 + 批齐备 + 指纹封存；重复定案拒收）。
    pub fn finalize(&mut self) -> Result<(u64, u32), u16> {
        if self.finalized {
            self.rejected += 1;
            return Err(XC_FINAL_DUP);
        }
        self.audit_conservation()?;
        self.batches_complete()?;
        self.finalized = true;
        let fp = self.fingerprint();
        self.final_fp = fp;
        Ok((fp, self.total as u32))
    }

    /// 矩阵指纹（id/批/态/码逐行 FNV——双导出对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ r.id as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.batch.no() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.verdict.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ r.fail_code as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 终稿文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        if !self.finalized {
            self.rejected += 1;
            return Err(XC_BATCH_INCOMPLETE);
        }
        let mut s = String::from("UNX-F0974-MATRIX|v1|total=");
        s.push_str(&self.total.to_string());
        s.push_str("|pass=");
        s.push_str(&self.per_verdict[0].to_string());
        s.push_str("|fail=");
        s.push_str(&self.per_verdict[1].to_string());
        s.push_str("|skip=");
        s.push_str(&self.per_verdict[2].to_string());
        s.push_str("|rate_bp=");
        s.push_str(&self.pass_rate_bp().to_string());
        s.push('\n');
        let fs = self.failures();
        let mut i: usize = 0;
        while i < fs.len() {
            s.push_str("FAIL|id=");
            s.push_str(&fs[i].0.to_string());
            s.push_str("|batch=");
            s.push_str(&fs[i].1.to_string());
            s.push_str("|code=0x");
            s.push_str(&hex4(fs[i].2));
            s.push('\n');
            i += 1;
        }
        let fp = self.fingerprint();
        if fp != self.final_fp {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

/// u16 四位十六进制（失败码文本格式）。
fn hex4(v: u16) -> String {
    const HEXD: [&str; 16] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
    ];
    let mut out = String::new();
    let mut i: u32 = 0;
    while i < 4 {
        let nib = ((v >> (12 - i * 4)) & 0xF) as usize;
        out.push_str(HEXD[nib]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：八批断言入矩阵→失败带码→守恒→定案→双导出→注入面。
pub fn drill() -> Result<u16, u16> {
    let mut m = RegMatrix::new(MATRIX_CAP)?;
    // 注入面：零号 id、越界失败码缺失、改判全拦。
    if m.record(0, RegBatch::B01Open, Verdict::Pass, 0, 1) != Err(XC_ASSERT_BAD) {
        return Err(XC_ASSERT_BAD);
    }
    if m.record(9, RegBatch::B01Open, Verdict::Fail, 0, 2) != Err(XC_FAIL_NOCODE) {
        return Err(XC_FAIL_NOCODE);
    }
    // 八批各 2 条（16 行），其中 1 条失败带码、1 条跳过。
    let batches = [
        RegBatch::B01Open,
        RegBatch::B02Vector,
        RegBatch::B03Eoi,
        RegBatch::B04Throughput,
        RegBatch::B05Latency,
        RegBatch::B06Storm,
        RegBatch::B07Throttle,
        RegBatch::B08Recover,
    ];
    let mut id: u32 = 1;
    let mut tick: u64 = 10;
    let mut bi: usize = 0;
    while bi < BATCH_COUNT {
        let b = batches[bi];
        if bi == 5 {
            m.record(id, b, Verdict::Fail, XC_RESULT_BAD, tick)?;
        } else if bi == 6 {
            m.record(id, b, Verdict::Skip, 0, tick)?;
        } else {
            m.record(id, b, Verdict::Pass, 0, tick)?;
        }
        id += 1;
        tick += 10;
        m.record(id, b, Verdict::Pass, 0, tick)?;
        id += 1;
        tick += 10;
        bi += 1;
    }
    // 守恒机检：16 = 14 pass + 1 fail + 1 skip（三态和恰等总行）。
    m.audit_conservation()?;
    let vc = m.verdict_counts();
    if vc[0] + vc[1] + vc[2] != 16 || vc[1] != 1 {
        return Err(XC_CONSERVATION);
    }
    // 失败可追：失败清单恰一行且码正确。
    let fs = m.failures();
    if fs.len() != 1 || fs[0].2 != XC_RESULT_BAD {
        return Err(XC_FAIL_NOCODE);
    }
    // 定案 + 双导出指纹稳定。
    let (fp1, total) = m.finalize()?;
    if total != 16 {
        return Err(XC_CONSERVATION);
    }
    let _ = m.export()?;
    let _ = m.export()?;
    if m.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    // 定案后再入档拒收。
    if m.record(id, RegBatch::B01Open, Verdict::Pass, 0, tick) != Err(XC_FINAL_DUP) {
        return Err(XC_FINAL_DUP);
    }
    Ok(0)
}
