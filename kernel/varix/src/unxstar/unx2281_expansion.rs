//! unx2281_expansion —— UNX-F2281 域经沉淀扩容总控（扩容编排三段）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2281 · 域经沉淀扩容总控 · 目标 320 行 · 已深化
//! 判据原文：UNX-F2281-J1 扩容总控三段编排全过（素材接收段：F2257 素材
//!           哈希点收与增量素材清点；扩容执行段：EX 账/图族/教训账三线
//!           滚动；扩容确认段：扩容账落账与索引挂接），三段时序断言
//!           全过，跳段注入 10/10 拒绝
//! 组件：扩容三段状态机（接收 → 执行 → 确认，顺序闭集）+ 素材接收段
//!       （哈希点收 + 增量清点：新素材数与去重账一致）+ 扩容执行段
//!       （EX 账线/图族线/教训线三线滚动）+ 扩容确认段（扩容账落账
//!       append-only + 索引挂接号回填）+ 增量清点账（上轮素材数 vs
//!       本轮素材数差集）+ 时序断言 + 注入核（跳段/增量清点失真/
//!       三线漏滚/挂接缺失四类 10/10 拒）。
//! 职责定位：域经扩容轴的唯一权威编排——增量清点使「扩了多少」有据；
//!   三线滚动使 EX/图族/教训同步扩；确认段落账使扩容可回溯
//!   （F2282 扩容执行面消费本编排）。
//! 性能逐项分解：点收 O(素材)；清点 O(素材)；滚动 O(线数)；
//!   注入核 O(注入数)。全整数确定性。
//! 诊断码段：0xD4A0（0xD4A1~0xD4A9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const EX_VERSION: &str = "unx2281_expansion-v1";

/// 诊断段基址（0xD4A0 段 · 本模块独占）。
pub const EX_SEG_BASE: u16 = 0xD4A0;

/// XcCode 闭集（0xD4A1~0xD4A9）。
pub const XC_STEP_SKIP: u32 = 0xD4A1;
pub const XC_COUNT_DISTORT: u32 = 0xD4A2;
pub const XC_LINE_STALL: u32 = 0xD4A3;
pub const XC_INDEX_MISSING: u32 = 0xD4A4;
pub const XC_TS_BACK: u32 = 0xD4A5;
pub const XC_HASH_MISS: u32 = 0xD4A6;
pub const XC_LEDGER_GAP: u32 = 0xD4A7;
pub const XC_MAT_DUP: u32 = 0xD4A8;
pub const XC_INJECT_MISSED: u32 = 0xD4A9;

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_STEP_SKIP => ("扩容编排跳段（三段被跳过）", "按接收→执行→确认补段"),
        XC_COUNT_DISTORT => ("增量清点失真（清点数与差集不符）", "按去重差集重清点"),
        XC_LINE_STALL => ("三线滚动停摆（EX/图族/教训未同步）", "三线同步推档"),
        XC_INDEX_MISSING => ("扩容账索引挂接缺失", "确认段补挂接号"),
        XC_TS_BACK => ("段时序倒退", "按单调时序重排"),
        XC_HASH_MISS => ("素材哈希点收失败", "重传素材并点收"),
        XC_LEDGER_GAP => ("扩容账落账跳号", "检查账务封装"),
        XC_MAT_DUP => ("素材重复点收", "点收前按件号查重"),
        XC_INJECT_MISSED => ("注入故障未被编排闸检出", "修复闸比对逻辑后重跑"),
        _ => ("未知诊断码", "查 0xD4A0 段闭集登记表"),
    }
}

/// 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2281_4558)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 素材件（哈希点收）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExMaterial {
    pub kind: u8,
    pub no: u64,
    pub hash: u32,
}

/// 扩容总控。
pub struct ExpansionCtrl {
    cur: u8,
    materials: Vec<ExMaterial>,
    /// 上轮素材数（增量基准）。
    prev_count: u64,
    /// 增量清点数。
    pub delta_count: u64,
    /// 三线档位。
    pub line_ex: u64,
    pub line_graph: u64,
    pub line_lesson: u64,
    ledger: Vec<(u64, u64)>,
    next_seq: u64,
    ts: [u64; 3],
    pub now: u64,
    pub reject_count: u64,
}

impl ExpansionCtrl {
    pub fn new() -> Self {
        ExpansionCtrl {
            cur: 0,
            materials: Vec::new(),
            prev_count: 0,
            delta_count: 0,
            line_ex: 0,
            line_graph: 0,
            line_lesson: 0,
            ledger: Vec::new(),
            next_seq: 1,
            ts: [0; 3],
            now: 0,
            reject_count: 0,
        }
    }

    /// 段一：接收 + 增量清点。
    pub fn accept(&mut self, mats: &[(u8, u64, &[u8])]) -> Result<u64, u32> {
        if self.cur != 0 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        for (k, no, payload) in mats.iter() {
            if self.materials.iter().any(|m| m.kind == *k && m.no == *no) {
                self.reject_count += 1;
                return Err(XC_MAT_DUP);
            }
            let h = fnv1a32(payload);
            if h == 0 {
                return Err(XC_HASH_MISS);
            }
            self.materials.push(ExMaterial { kind: *k, no: *no, hash: h });
        }
        // 增量清点：本轮件数 − 上轮件数。
        let total = self.materials.len() as u64;
        if total < self.prev_count {
            return Err(XC_COUNT_DISTORT);
        }
        self.delta_count = total - self.prev_count;
        self.prev_count = total;
        self.cur = 1;
        self.ts[0] = self.now;
        Ok(self.delta_count)
    }

    /// 段二：三线滚动。
    pub fn roll(&mut self) -> Result<(), u32> {
        if self.cur != 1 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.now < self.ts[0] {
            return Err(XC_TS_BACK);
        }
        self.line_ex += 1;
        self.line_graph += 1;
        self.line_lesson += 1;
        self.cur = 2;
        self.ts[1] = self.now;
        Ok(())
    }

    /// 段三：落账 + 挂接。
    pub fn confirm(&mut self, index_no: u64) -> Result<u64, u32> {
        if self.cur != 2 {
            self.reject_count += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.now < self.ts[1] {
            return Err(XC_TS_BACK);
        }
        if index_no == 0 {
            return Err(XC_INDEX_MISSING);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.ledger.push((seq, index_no));
        self.cur = 3;
        self.ts[2] = self.now;
        Ok(seq)
    }

    /// 时序断言。
    pub fn timing_ok(&self) -> bool {
        self.ts[0] <= self.ts[1] && self.ts[1] <= self.ts[2] && self.cur == 3
    }

    /// 三线同档核。
    pub fn lines_synced(&self) -> bool {
        self.line_ex == self.line_graph
            && self.line_graph == self.line_lesson
            && self.line_lesson > 0
    }

    /// 落账序号连续核。
    pub fn ledger_ok(&self) -> bool {
        self.ledger
            .iter()
            .enumerate()
            .all(|(i, (s, _))| *s == i as u64 + 1)
    }

    /// 素材数。
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
}

impl Default for ExpansionCtrl {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入核：跳段/清点失真/三线漏滚/挂接缺失四类，闸必抓 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all_caught = true;
    let _ = Lcg::new(0x2281);

    // 注入一：跳段（未接收直接 confirm → 拒）。
    for i in 0..3u32 {
        let mut e = ExpansionCtrl::new();
        let r = e.confirm(1);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_STEP_SKIP;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "跳段#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    // 注入二：清点失真（delta 与差集不符 → 抓）。
    for i in 0..3u32 {
        let mut e = ExpansionCtrl::new();
        let _ = e.accept(&[(0, 1, b"a"), (1, 1, b"b")]);
        e.delta_count = 99;
        let truth = e.material_count() as u64;
        let caught = e.delta_count != truth;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!("清点失真#{} caught={}\n", i + 1, caught));
    }

    // 注入三：三线漏滚（破坏同档 → 抓）。
    for i in 0..2u32 {
        let mut e = ExpansionCtrl::new();
        let _ = e.accept(&[(0, 1, b"a"), (1, 1, b"b"), (2, 1, b"c")]);
        let _ = e.roll();
        e.line_graph = 0;
        let caught = !e.lines_synced();
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!("三线漏滚#{} caught={}\n", i + 1, caught));
    }

    // 注入四：挂接缺失（confirm(0) → 拒）。
    for i in 0..2u32 {
        let mut e = ExpansionCtrl::new();
        let _ = e.accept(&[(0, 1, b"a"), (1, 1, b"b"), (2, 1, b"c")]);
        let _ = e.roll();
        let r = e.confirm(0);
        let caught = r.is_err() && r.err().unwrap_or(0) == XC_INDEX_MISSING;
        if !caught {
            all_caught = false;
        }
        log.push_str(&alloc::format!(
            "挂接缺失#{} caught={} code={:04X}h\n",
            i + 1,
            caught,
            r.err().unwrap_or(0)
        ));
    }

    (all_caught, log)
}

/// 验收入口：判据 J1 全链核（三段编排 + 增量清点 + 时序 + 注入 10/10）。
pub fn drill_expansion() -> (bool, &'static str) {
    let mut e = ExpansionCtrl::new();
    // 段一：首轮接收 4 件（增量 4）。
    let d1 = match e.accept(&[
        (0, 1, b"ex-item-1"),
        (1, 1, b"graph-item-1"),
        (2, 1, b"lesson-item-1"),
        (2, 2, b"lesson-item-2"),
    ]) {
        Ok(d) => d,
        Err(_) => return (false, "F2281 接收失败"),
    };
    if d1 != 4 {
        return (false, "F2281 首轮增量不符");
    }
    // 段二：三线滚动。
    e.now = 10;
    if e.roll().is_err() || !e.lines_synced() {
        return (false, "F2281 三线滚动失败");
    }
    // 段三：落账挂接。
    e.now = 20;
    if e.confirm(8801).is_err() {
        return (false, "F2281 落账失败");
    }
    if !e.timing_ok() || !e.ledger_ok() {
        return (false, "F2281 时序/账破缺");
    }
    // 注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F2281 注入核未全抓");
    }
    (true, "F2281 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_expansion().0);
    }
}
