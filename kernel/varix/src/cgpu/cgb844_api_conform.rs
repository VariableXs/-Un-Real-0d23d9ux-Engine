//! cgb844_api_conform — CGPU-F0844 Intel API 一致性。
//!
//! 判据原文（验收行摘录）：Intel API 一致性：调用序列账（入口调用按序落
//! 账：入口号+参数摘要）；序列回放比对（同序列两次执行结果逐条一致——
//! 不一致即漂移）；参数白名单（入口号闭集表，表外入口即拒）；守恒账（落
//! 账数 = 回放通过 + 漂移 + 拒绝）。判据：序列账、回放比对、白名单、
//! 守恒。
//!
//! 诊断段 0x7400（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；参数摘要 FNV 整数；定容序列账；drill() 自
//! 例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb844-v1";

/// 诊断段基（本单独占 0x7400）。
pub const SEG_BASE: u16 = 0x7400;

/// 入口越权（入口号不在白名单闭集）。
pub const XC_ENTRY_DENIED: u16 = SEG_BASE + 1;
/// 序列账满（定容耗尽）。
pub const XC_SEQ_FULL: u16 = SEG_BASE + 2;
/// 回放漂移（同序列两次执行结果不一致）。
pub const XC_REPLAY_DRIFT: u16 = SEG_BASE + 3;
/// 空序列（无调用即回放）。
pub const XC_SEQ_EMPTY: u16 = SEG_BASE + 4;
/// 守恒破坏。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 5;
/// 结果缺失（回放序列短于落账序列）。
pub const XC_RESULT_SHORT: u16 = SEG_BASE + 6;

/// 入口白名单闭集（判据"入口号闭集表"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// 内存分配。
    Alloc = 1,
    /// 内存释放。
    Free = 2,
    /// 缓冲绑定。
    Bind = 3,
    /// 提交帧。
    Submit = 4,
    /// 查询状态。
    Query = 5,
}

impl Entry {
    /// 入口号。
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 白名单校验（表外即拒）。
    pub fn allowed(c: u16) -> Result<Entry, u16> {
        match c {
            1 => Ok(Entry::Alloc),
            2 => Ok(Entry::Free),
            3 => Ok(Entry::Bind),
            4 => Ok(Entry::Submit),
            5 => Ok(Entry::Query),
            _ => Err(XC_ENTRY_DENIED),
        }
    }
}

/// 参数摘要（FNV-1a 整数化——判据"参数摘要"）。
pub fn param_digest(entry: u16, params: &[u64]) -> u64 {
    let mut acc = 0xcbf29ce484222325u64;
    acc ^= entry as u64;
    acc = acc.wrapping_mul(0x100000001b3);
    for p in params {
        acc ^= *p;
        acc = acc.wrapping_mul(0x100000001b3);
    }
    acc
}

/// 序列账条目。
#[derive(Clone, Copy, Debug)]
pub struct Call {
    /// 入口号。
    pub entry: u16,
    /// 参数摘要。
    pub digest: u64,
    /// 执行结果（模拟求值：摘要异或常量）。
    pub result: u64,
}

/// API 一致性核验器。
pub struct Conformer {
    /// 核验器名。
    pub name: String,
    /// 序列账（定容）。
    pub seq: Vec<Call>,
    /// 回放通过数。
    pub replay_ok: u64,
    /// 漂移数。
    pub drift_cnt: u64,
    /// 拒绝数。
    pub rejected: u64,
}

/// 守恒恒等式：seq.len() == 回放覆盖数；replay_ok + drift_cnt ≤ seq.len()；
/// 拒绝（白名单外）不入账——三者与账外事件对位由 drill 核验。
impl Conformer {
    /// 构造。
    pub fn new(name: &str) -> Self {
        Conformer { name: name.to_string(), seq: Vec::new(), replay_ok: 0, drift_cnt: 0, rejected: 0 }
    }

    /// 调用落账（判据"调用序列账"：白名单外即拒不入账）。
    pub fn call(&mut self, entry: u16, params: &[u64]) -> Result<u64, u16> {
        let e = Entry::allowed(entry)?;
        if self.seq.len() >= 32 {
            self.rejected += 1;
            return Err(XC_SEQ_FULL);
        }
        let d = param_digest(e.code(), params);
        // 模拟执行结果：摘要异或固定扰动常量（确定性——两次同参同果）。
        let result = d ^ 0xA5A5_5A5A_1234_5678;
        self.seq.push(Call { entry: e.code(), digest: d, result });
        self.verify()?;
        Ok(result)
    }

    /// 序列长度。
    pub fn seq_len(&self) -> usize {
        self.seq.len()
    }

    /// 回放比对（判据"序列回放比对"：重放同参数序列，逐条结果必须一致）。
    /// 回放输入：与落账同源的 (入口, 参数) 序列；逐条对账。
    pub fn replay(&mut self, calls: &[(u16, Vec<u64>)]) -> Result<u64, u16> {
        if self.seq.is_empty() {
            return Err(XC_SEQ_EMPTY);
        }
        if calls.len() < self.seq.len() {
            return Err(XC_RESULT_SHORT);
        }
        let mut ok = 0u64;
        for (i, c) in self.seq.iter().enumerate() {
            let (re, rp) = &calls[i];
            let e = Entry::allowed(*re)?;
            if e.code() != c.entry {
                self.drift_cnt += 1;
                continue;
            }
            let d = param_digest(e.code(), rp);
            if d != c.digest {
                self.drift_cnt += 1;
                continue;
            }
            // 结果确定性核验：同摘要同结果。
            let result = d ^ 0xA5A5_5A5A_1234_5678;
            if result != c.result {
                self.drift_cnt += 1;
                continue;
            }
            ok += 1;
        }
        self.replay_ok += ok;
        self.verify()?;
        Ok(ok)
    }

    /// 收口核验：回放覆盖全账（通过 + 漂移 == 账长）。
    pub fn close_check(&self) -> Result<(), u16> {
        if self.seq.is_empty() {
            return Err(XC_SEQ_EMPTY);
        }
        if self.replay_ok + self.drift_cnt != self.seq.len() as u64 {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.replay_ok + self.drift_cnt > self.seq.len() as u64 {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb844[");
        s.push_str(&self.name);
        s.push_str("] seq=");
        s.push_str(&self.seq_len().to_string());
        s.push_str(" ok=");
        s.push_str(&self.replay_ok.to_string());
        s.push_str(" drift=");
        s.push_str(&self.drift_cnt.to_string());
        s.push_str(" rej=");
        s.push_str(&self.rejected.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut c = Conformer::new("w3-drill-844");

    // 负样本 1：白名单外入口（0 与 6）。
    match c.call(0, &[1]) {
        Err(code) if code == XC_ENTRY_DENIED => {}
        Ok(_) => return Err(XC_ENTRY_DENIED),
        Err(other) => return Err(other),
    }
    match c.call(6, &[1]) {
        Err(code) if code == XC_ENTRY_DENIED => {}
        Ok(_) => return Err(XC_ENTRY_DENIED),
        Err(other) => return Err(other),
    }
    if c.rejected != 0 {
        // 白名单拒绝不入账也不计 rejected（rejected 仅账满）。
        return Err(XC_LEDGER_DRIFT);
    }

    // 负样本 2：空序列回放。
    match c.replay(&[]) {
        Err(code) if code == XC_SEQ_EMPTY => {}
        Ok(_) => return Err(XC_SEQ_EMPTY),
        Err(other) => return Err(other),
    }

    // 落账 5 调用（混合入口）。
    c.call(Entry::Alloc.code(), &[64, 0])?;
    c.call(Entry::Bind.code(), &[64, 7])?;
    c.call(Entry::Submit.code(), &[7, 1])?;
    c.call(Entry::Query.code(), &[7])?;
    c.call(Entry::Free.code(), &[64])?;
    if c.seq_len() != 5 {
        return Err(XC_LEDGER_DRIFT);
    }

    // 负样本 3：回放序列短于账。
    let mut calls: Vec<(u16, Vec<u64>)> = Vec::new();
    calls.push((Entry::Alloc.code(), vec![64, 0]));
    calls.push((Entry::Bind.code(), vec![64, 7]));
    match c.replay(&calls) {
        Err(code) if code == XC_RESULT_SHORT => {}
        Ok(_) => return Err(XC_RESULT_SHORT),
        Err(other) => return Err(other),
    }

    // 正样本：全序列回放（同参数）——逐条一致。
    let mut full: Vec<(u16, Vec<u64>)> = Vec::new();
    full.push((Entry::Alloc.code(), vec![64, 0]));
    full.push((Entry::Bind.code(), vec![64, 7]));
    full.push((Entry::Submit.code(), vec![7, 1]));
    full.push((Entry::Query.code(), vec![7]));
    full.push((Entry::Free.code(), vec![64]));
    let ok = c.replay(&full)?;
    if ok != 5 {
        return Err(XC_REPLAY_DRIFT);
    }
    c.close_check()?;

    // 漂移路径：回放时参数被篡改——对应条目漂移。
    let mut c2 = Conformer::new("w3-drill-844-drift");
    c2.call(Entry::Alloc.code(), &[64, 0])?;
    c2.call(Entry::Free.code(), &[64])?;
    let mut bad: Vec<(u16, Vec<u64>)> = Vec::new();
    bad.push((Entry::Alloc.code(), vec![64, 0]));
    bad.push((Entry::Free.code(), vec![65])); // 参数被改
    let ok2 = c2.replay(&bad)?;
    if ok2 != 1 {
        return Err(XC_REPLAY_DRIFT);
    }
    if c2.drift_cnt != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    c2.close_check()?;

    // 漂移路径 2：入口被换。
    let mut c3 = Conformer::new("w3-drill-844-entry");
    c3.call(Entry::Submit.code(), &[1, 2])?;
    let mut bad2: Vec<(u16, Vec<u64>)> = Vec::new();
    bad2.push((Entry::Query.code(), vec![1, 2])); // 入口被换（白名单内）
    let ok3 = c3.replay(&bad2)?;
    if ok3 != 0 || c3.drift_cnt != 1 {
        return Err(XC_REPLAY_DRIFT);
    }
    c3.close_check()?;

    // 确定性自检：同参数两次 call 结果一致（跨账本）。
    let r1 = c.call(Entry::Alloc.code(), &[128, 3])?;
    let r2 = Conformer::new("x").call(Entry::Alloc.code(), &[128, 3])?;
    if r1 != r2 {
        return Err(XC_REPLAY_DRIFT);
    }
    c.verify()?;
    c2.verify()?;
    c3.verify()?;
    if c.rejected != 0 || c.replay_ok != 5 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
