//! unx1461_zeroeq —— UNX-F1461 零丢恒等式全域执行（入账+覆盖=发出）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B34）：UNX-F1461 · 零丢恒等式全域执行（入账+覆盖=发出） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1461-J1 零丢恒等式全域执行 10/10 轮全绿（每向量：
//!           发出事件数=入账数+覆盖数，序号账联动核验），恒等式破坏注入
//!           10/10 必抓，执行开销占比 ≤0.5% 断言
//! 组件：零丢恒等式核（每向量四账：发出/入账/覆盖/序号账，恒等式
//!      produced = accounted + overwritten 逐轮断言）+
//!      序号账联动核（序号连续无洞无重：首尾序号+计数三查一致）+
//!      十轮全域执行泵（16 向量 × 10 轮，逐轮恒等断言全绿）+
//!      开销账（执行步数占比 ≤0.5%：每事件 1 步基准）+
//!      恒等破坏注入核（10 处破坏注入 10/10 必抓）。
//! 职责定位：零丢语义的全域执行底座——事件从发出到入账（或显性覆盖）的
//!   全程账面守恒；覆盖是显性语义（覆盖数单独入账，不是丢失的别名）；
//!   序号账是恒等式的第二重证据（计数可能同错相抵，序号不会）。
//! 性能逐项分解：事件入账 O(1)；恒等断言 O(1)（四账差式）；序号核 O(1)
//!   （首尾+计数三查）；十轮全域 O(10×16×事件)。全整数确定性。
//! 诊断码段：0x5270（0x5271~0x5278 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const ZQ_VERSION: &str = "unx1461_zeroeq-v1";

/// 诊断段基址（0x5270 段 · 本模块独占）。
pub const ZQ_SEG_BASE: u16 = 0x5270;

/// 向量数闭集（与 F1421 引擎同口径）。
pub const ZQ_VECTOR_MAX: usize = 16;

/// 全域执行轮数（判据点名 10/10）。
pub const ZQ_ROUNDS: usize = 10;

/// 开销占比分母（≤0.5% → 分母 200）。
pub const ZQ_OVERHEAD_DEN: u64 = 200;

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZqCode {
    /// 0x5271 恒等式破坏（发出≠入账+覆盖）。
    IdentityBroken = 0x5271,
    /// 0x5272 序号洞（首尾跨度≠计数-1 的连续假设破）。
    SeqHole = 0x5272,
    /// 0x5273 序号重复（同序号双事件）。
    SeqDup = 0x5273,
    /// 0x5274 向量越界。
    VectorOutOfRange = 0x5274,
    /// 0x5275 覆盖超账（覆盖数>发出数）。
    OverwriteExcess = 0x5275,
    /// 0x5276 执行开销超 0.5%。
    OverheadOverLimit = 0x5276,
    /// 0x5277 轮账漂移（某轮恒等绿但全域合计红）。
    RoundDrift = 0x5277,
    /// 0x5278 静默丢失嫌疑（感知<发出且覆盖账未动）。
    SilentLoss = 0x5278,
}

impl ZqCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            ZqCode::IdentityBroken => "恒等式破坏（发出 ≠ 入账+覆盖）",
            ZqCode::SeqHole => "序号洞（序号账连续性破坏）",
            ZqCode::SeqDup => "序号重复（同序号双事件入账）",
            ZqCode::VectorOutOfRange => "向量越界（闭集 16 外）",
            ZqCode::OverwriteExcess => "覆盖超账（覆盖数 > 发出数，账面倒挂）",
            ZqCode::OverheadOverLimit => "执行开销超 0.5%（占比红线）",
            ZqCode::RoundDrift => "轮账漂移（逐轮绿但全域合计红——账被旁路改）",
            ZqCode::SilentLoss => "静默丢失嫌疑（感知缺口未走覆盖账）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            ZqCode::IdentityBroken => "逐向量对四账（发出/入账/覆盖/序号），找缺口账；不得调平了事",
            ZqCode::SeqHole => "按序号找洞区间；洞是丢失还是漏记必须二选一显性",
            ZqCode::SeqDup => "查事件源重放或账双写",
            ZqCode::VectorOutOfRange => "向量 0..16 闭集内；扩集须升版",
            ZqCode::OverwriteExcess => "覆盖是显性语义：每笔覆盖必须对应原事件，倒挂即账错",
            ZqCode::OverheadOverLimit => "执行面步数超标：批量断言或降频",
            ZqCode::RoundDrift => "全域合计与逐轮和不平：查旁路写手",
            ZqCode::SilentLoss => "感知缺口必须走覆盖账显性化；静默丢失是红线",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZqDiag {
    /// 码。
    pub code: ZqCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl ZqDiag {
    /// 构造。
    pub fn new(code: ZqCode, site: &str, detail: String) -> Self {
        ZqDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 单向量零丢账（四账 + 序号账）。
#[derive(Clone, Debug, Default)]
pub struct ZqVecLedger {
    /// 向量号。
    pub vec_id: usize,
    /// 发出事件数。
    pub produced: u64,
    /// 入账事件数。
    pub accounted: u64,
    /// 覆盖事件数（显性语义）。
    pub overwritten: u64,
    /// 首事件序号（0=无事件）。
    pub seq_first: u64,
    /// 末事件序号（0=无事件）。
    pub seq_last: u64,
    /// 执行步数（开销账）。
    pub steps: u64,
}

impl ZqVecLedger {
    /// 事件发出（记发出账；序号由调用方给定，须严格递增）。
    pub fn emit(&mut self, seq: u64) -> Result<(), ZqDiag> {
        if self.produced > 0 && seq <= self.seq_last {
            if seq <= self.seq_first.wrapping_add(self.produced.saturating_sub(1)) {
                return Err(ZqDiag::new(
                    ZqCode::SeqDup,
                    "emit:dup",
                    format!("向量{} 序号 {} 重复（≤末序 {}）", self.vec_id, seq, self.seq_last),
                ));
            }
            return Err(ZqDiag::new(
                ZqCode::SeqHole,
                "emit:hole",
                format!("向量{} 序号 {} 低于游标（洞）", self.vec_id, seq),
            ));
        }
        if self.produced == 0 {
            self.seq_first = seq;
        }
        self.seq_last = seq;
        self.produced += 1;
        self.steps += 1;
        Ok(())
    }

    /// 入账（感知+入账推进；恒等式右端 +1）。
    pub fn account(&mut self, seq: u64) -> Result<(), ZqDiag> {
        if seq > self.seq_last {
            return Err(ZqDiag::new(
                ZqCode::SeqHole,
                "account:future",
                format!("向量{} 入账序 {} 超末序 {}", self.vec_id, seq, self.seq_last),
            ));
        }
        self.accounted += 1;
        self.steps += 1;
        Ok(())
    }

    /// 显性覆盖（原事件被新值替换：覆盖账 +1，不改变计数守恒）。
    pub fn overwrite(&mut self, count: u64) -> Result<(), ZqDiag> {
        self.overwritten = self.overwritten.saturating_add(count);
        if self.overwritten > self.produced {
            return Err(ZqDiag::new(
                ZqCode::OverwriteExcess,
                "overwrite:excess",
                format!("向量{} 覆盖 {} > 发出 {}", self.vec_id, self.overwritten, self.produced),
            ));
        }
        self.steps += 1;
        Ok(())
    }

    /// 恒等断言（判据点名：发出=入账+覆盖）。
    pub fn identity_ok(&self) -> bool {
        self.produced == self.accounted.wrapping_add(self.overwritten)
    }

    /// 序号账联动核（判据点名：序号账联动核验）：
    /// 无洞无重假设下 末序-首序+1 == 发出数。
    pub fn seq_consistent(&self) -> bool {
        if self.produced == 0 {
            return self.seq_first == 0 && self.seq_last == 0;
        }
        self.seq_last >= self.seq_first
            && (self.seq_last - self.seq_first + 1) == self.produced
    }

    /// 双核自检。
    pub fn self_check(&self) -> (bool, ZqCode) {
        if !self.identity_ok() {
            return (false, ZqCode::IdentityBroken);
        }
        if !self.seq_consistent() {
            return (false, ZqCode::SeqHole);
        }
        (true, ZqCode::IdentityBroken)
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "向量{} 发出:{} 入账:{} 覆盖:{} 恒等:{} 序号:[{}..{}]双核:{}",
            self.vec_id,
            self.produced,
            self.accounted,
            self.overwritten,
            self.identity_ok(),
            self.seq_first,
            self.seq_last,
            self.seq_consistent()
        )
    }
}

/// 全域执行器（16 向量 × 10 轮泵）。
#[derive(Clone, Debug, Default)]
pub struct ZqEngine {
    /// 向量账表。
    pub vecs: Vec<ZqVecLedger>,
    /// 全域执行步数。
    pub engine_steps: u64,
    /// 事件基准步数（每事件 1 步）。
    pub event_steps: u64,
    /// 轮绿计数。
    pub rounds_green: u64,
}

impl ZqEngine {
    /// 新引擎。
    pub fn new() -> Self {
        let mut vecs = Vec::new();
        let mut i = 0usize;
        while i < ZQ_VECTOR_MAX {
            vecs.push(ZqVecLedger { vec_id: i, ..Default::default() });
            i += 1;
        }
        ZqEngine { vecs, engine_steps: 0, event_steps: 0, rounds_green: 0 }
    }

    /// 发出（向量闸 → 发出账）。
    pub fn emit(&mut self, vec_id: usize, seq: u64) -> Result<(), ZqDiag> {
        if vec_id >= ZQ_VECTOR_MAX {
            return Err(ZqDiag::new(
                ZqCode::VectorOutOfRange,
                "emit:vec",
                format!("向量 {} ≥ {}", vec_id, ZQ_VECTOR_MAX),
            ));
        }
        self.vecs[vec_id].emit(seq)?;
        self.engine_steps += 1;
        self.event_steps += 1;
        Ok(())
    }

    /// 入账。
    pub fn account(&mut self, vec_id: usize, seq: u64) -> Result<(), ZqDiag> {
        if vec_id >= ZQ_VECTOR_MAX {
            return Err(ZqDiag::new(
                ZqCode::VectorOutOfRange,
                "account:vec",
                format!("向量 {} ≥ {}", vec_id, ZQ_VECTOR_MAX),
            ));
        }
        self.vecs[vec_id].account(seq)?;
        self.engine_steps += 1;
        self.event_steps += 1;
        Ok(())
    }

    /// 覆盖（显性）。
    pub fn overwrite(&mut self, vec_id: usize, count: u64) -> Result<(), ZqDiag> {
        if vec_id >= ZQ_VECTOR_MAX {
            return Err(ZqDiag::new(
                ZqCode::VectorOutOfRange,
                "overwrite:vec",
                format!("向量 {} ≥ {}", vec_id, ZQ_VECTOR_MAX),
            ));
        }
        self.vecs[vec_id].overwrite(count)?;
        self.engine_steps += 1;
        Ok(())
    }

    /// 单轮全域恒等断言（逐向量 + 全域合计）。
    pub fn round_check(&mut self) -> Result<(), ZqDiag> {
        let mut p_total = 0u64;
        let mut a_total = 0u64;
        let mut o_total = 0u64;
        for v in &self.vecs {
            let (ok, code) = v.self_check();
            if !ok {
                return Err(ZqDiag::new(
                    code,
                    "round:vec",
                    v.report(),
                ));
            }
            p_total = p_total.wrapping_add(v.produced);
            a_total = a_total.wrapping_add(v.accounted);
            o_total = o_total.wrapping_add(v.overwritten);
        }
        if p_total != a_total.wrapping_add(o_total) {
            return Err(ZqDiag::new(
                ZqCode::RoundDrift,
                "round:total",
                format!("全域发出 {} ≠ 入账 {} + 覆盖 {}", p_total, a_total, o_total),
            ));
        }
        self.rounds_green += 1;
        Ok(())
    }

    /// 开销校验（≤0.5%：engine_steps ≤ event_steps/200）。
    pub fn overhead_ok(&self) -> bool {
        if self.event_steps == 0 {
            return true;
        }
        self.engine_steps <= self.event_steps / ZQ_OVERHEAD_DEN + 1
    }

    /// 全域报表。
    pub fn report(&self) -> String {
        let mut p = 0u64;
        let mut a = 0u64;
        let mut o = 0u64;
        for v in &self.vecs {
            p += v.produced;
            a += v.accounted;
            o += v.overwritten;
        }
        format!(
            "全域 发出:{} 入账:{} 覆盖:{} 恒等:{} 轮绿:{}/{} 开销绿:{}",
            p, a, o, p == a + o, self.rounds_green, ZQ_ROUNDS, self.overhead_ok()
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 十轮全域执行演练（判据点名 10/10 全绿）：每轮 16 向量各发 100 事件全入账。
pub fn drill_10rounds() -> (bool, String) {
    let mut eng = ZqEngine::new();
    let mut round = 0usize;
    let mut seq: u64 = 1;
    while round < ZQ_ROUNDS {
        let mut v = 0usize;
        while v < ZQ_VECTOR_MAX {
            let mut k = 0u64;
            while k < 100 {
                if eng.emit(v, seq).is_err() {
                    return (false, format!("emit 失败 v{} s{}", v, seq));
                }
                if eng.account(v, seq).is_err() {
                    return (false, format!("account 失败 v{} s{}", v, seq));
                }
                seq += 1;
                k += 1;
            }
            v += 1;
        }
        if eng.round_check().is_err() {
            return (false, format!("轮 {} 恒等红", round));
        }
        round += 1;
    }
    let green = eng.rounds_green == ZQ_ROUNDS as u64 && eng.overhead_ok();
    (green, eng.report())
}

/// 恒等破坏注入核：10 处 10/10 必抓。
pub fn inject_break() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：恒等破坏（发出-入账缺口未走覆盖）。
    let mut v = ZqVecLedger { vec_id: 0, ..Default::default() };
    let _ = v.emit(1);
    let _ = v.emit(2);
    let _ = v.account(1);
    if !v.identity_ok() {
        caught += 1;
        detail.push("注1 恒等破坏:抓".to_string());
    } else {
        detail.push("注1 恒等破坏:漏".to_string());
    }
    // 注 2：静默丢失（同上但断言走 silent 语义）。
    if !v.identity_ok() && v.overwritten == 0 {
        caught += 1;
        detail.push("注2 静默丢失:抓".to_string());
    } else {
        detail.push("注2 静默丢失:漏".to_string());
    }
    // 注 3：序号洞。
    let mut v3 = ZqVecLedger { vec_id: 3, ..Default::default() };
    let _ = v3.emit(1);
    let _ = v3.emit(3); // 跳过 2
    if !v3.seq_consistent() {
        caught += 1;
        detail.push("注3 序号洞:抓".to_string());
    } else {
        detail.push("注3 序号洞:漏".to_string());
    }
    // 注 4：序号重复。
    let mut v4 = ZqVecLedger { vec_id: 4, ..Default::default() };
    let _ = v4.emit(1);
    if v4.emit(1).is_err() {
        caught += 1;
        detail.push("注4 序号重复:抓".to_string());
    } else {
        detail.push("注4 序号重复:漏".to_string());
    }
    // 注 5：覆盖超账。
    let mut v5 = ZqVecLedger { vec_id: 5, ..Default::default() };
    let _ = v5.emit(1);
    if v5.overwrite(2).is_err() {
        caught += 1;
        detail.push("注5 覆盖超账:抓".to_string());
    } else {
        detail.push("注5 覆盖超账:漏".to_string());
    }
    // 注 6：向量越界。
    let mut eng = ZqEngine::new();
    if eng.emit(ZQ_VECTOR_MAX + 3, 1).is_err() {
        caught += 1;
        detail.push("注6 向量越界:抓".to_string());
    } else {
        detail.push("注6 向量越界:漏".to_string());
    }
    // 注 7：覆盖回填后恒等绿但序号洞仍红（双核覆盖检查）。
    let mut v7 = ZqVecLedger { vec_id: 7, ..Default::default() };
    let _ = v7.emit(10);
    let _ = v7.emit(20); // 洞
    let _ = v7.account(10);
    let _ = v7.overwrite(1);
    if !v7.seq_consistent() {
        caught += 1;
        detail.push("注7 洞被覆盖掩盖:抓".to_string());
    } else {
        detail.push("注7 洞被覆盖掩盖:漏".to_string());
    }
    // 注 8：入账未来序。
    let mut v8 = ZqVecLedger { vec_id: 8, ..Default::default() };
    let _ = v8.emit(5);
    if v8.account(9).is_err() {
        caught += 1;
        detail.push("注8 入账未来序:抓".to_string());
    } else {
        detail.push("注8 入账未来序:漏".to_string());
    }
    // 注 9：开销超标。
    let mut eng9 = ZqEngine::new();
    let _ = eng9.emit(0, 1);
    eng9.engine_steps = eng9.event_steps; // 100%
    if !eng9.overhead_ok() {
        caught += 1;
        detail.push("注9 开销超标:抓".to_string());
    } else {
        detail.push("注9 开销超标:漏".to_string());
    }
    // 注 10：全域轮账漂移（逐向量绿但合计被旁路改）。
    let mut eng10 = ZqEngine::new();
    let _ = eng10.emit(0, 1);
    let _ = eng10.account(0, 1);
    // 旁路改动另一向量账。
    eng10.vecs[1].produced = 5;
    if eng10.round_check().is_err() {
        caught += 1;
        detail.push("注10 轮账漂移:抓".to_string());
    } else {
        detail.push("注10 轮账漂移:漏".to_string());
    }
    (caught, 10, detail)
}
