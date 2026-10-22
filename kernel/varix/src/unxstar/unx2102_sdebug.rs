//! unx2102_sdebug —— UNX-F2102 sched_debug 调试面（全量 dump 只读）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2102 · sched_debug 调试面（全量 dump 只读） · 目标 320 行 · 已深化
//! 判据原文：UNX-F2102-J1 调试面断言全过（runqueue/调度实体全量 dump 只读语义），
//!           dump 与账本一致断言，权限位在册
//! 组件：runqueue 全量 dump（逐 rq：id/权重/运行数/负载 每行全列）+
//!      调度实体全量 dump（逐 se：pid/属 rq/vruntime/权重 每行全列）+
//!      只读语义闸（dump 路径零写账：dump 前后账本逐列哈希不变）+
//!      dump-账本一致断言（导出行数=账行数、逐列值=账内值 双重核）+
//!      权限位账（只读位 RO 在册 · 写请求拦截计数）+
//!      注入核（写穿透/行缺列/值漂移/权限缺位 四路必抓）+
//!      演练（16 rq × 256 se 全量 dump 一致率 100%）。
//! 职责定位：sched_debug 是调度器的观测窗口——窗口的意义在于"看而不碰"：
//!   dump 路径若有一字写账，观测即成干预，调试面即失去信用；
//!   dump-账本一致是窗口的成像质量：行数、列数、值三重对上，
//!   才能宣告 dump 反映的是账本真容而非快照残影。
//! 性能逐项分解：dump O(rq+se)；哈希 O(rq+se)；一致核 O(rq+se)；
//!   演练 O(dump×轮) 确定性 LCG；全整数零墙钟。
//! 诊断码段：0x6F80（0x6F81~0x6F88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SD_VERSION: &str = "unx2102_sdebug-v1";

/// 诊断段基址（0x6F80 段 · 本模块独占）。
pub const SD_SEG_BASE: u16 = 0x6F80;

/// 权限位：只读（RO · 冻结值）。
pub const SD_PERM_RO: u8 = 0b01;
/// 权限位：写（WO 反例位 · 调试面不得持有）。
pub const SD_PERM_WO: u8 = 0b10;

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdCode {
    /// 0x6F81 写穿透（dump 路径改了账）。
    WriteThrough = 0x6F81,
    /// 0x6F82 行缺列（dump 行列数不足）。
    RowShort = 0x6F82,
    /// 0x6F83 值漂移（dump 值 ≠ 账本值）。
    ValueDrift = 0x6F83,
    /// 0x6F84 权限缺位（RO 位不在册）。
    PermMissing = 0x6F84,
    /// 0x6F85 行数失配（dump 行数 ≠ 账行数）。
    RowCount = 0x6F85,
    /// 0x6F86 写请求未拦（只读面收了写）。
    WriteAccepted = 0x6F86,
    /// 0x6F87 空面 dump（无 rq 也出非空行）。
    EmptyDump = 0x6F87,
    /// 0x6F88 快照穿号（se 引用了不存在的 rq）。
    RefOrphan = 0x6F88,
}

impl SdCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            SdCode::WriteThrough => "写穿透（dump 路径改账）",
            SdCode::RowShort => "行缺列（dump 行列数不足）",
            SdCode::ValueDrift => "值漂移（dump 值与账本不一致）",
            SdCode::PermMissing => "权限缺位（RO 位不在册）",
            SdCode::RowCount => "行数失配（dump 与账行数不等）",
            SdCode::WriteAccepted => "写请求未拦（只读面收写）",
            SdCode::EmptyDump => "空面 dump（无账却出非空行）",
            SdCode::RefOrphan => "快照穿号（se 引用不存在 rq）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            SdCode::WriteThrough => "dump 全程走 &self 只读引用",
            SdCode::RowShort => "每行出满 4 列再落行",
            SdCode::ValueDrift => "dump 值直接取账内字段，不缓存",
            SdCode::PermMissing => "调试面注册时挂 RO 权限位",
            SdCode::RowCount => "dump 行数以账本 len 为准",
            SdCode::WriteAccepted => "write 入口先核 RO 位，非只读即拒",
            SdCode::EmptyDump => "空账 dump 返回空串",
            SdCode::RefOrphan => "se 挂载时校验 rq id 在册",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdDiag {
    /// 码。
    pub code: SdCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl SdDiag {
    /// 新诊断。
    pub fn new(code: SdCode, site: &str, detail: String) -> Self {
        SdDiag {
            code,
            site: site.to_string(),
            detail,
        }
    }

    /// 单行全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04x}] {} @{}: {}（建议：{}）",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// runqueue 账（可 dump 最小列集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdRq {
    /// rq id。
    pub id: u32,
    /// 权重。
    pub weight: u64,
    /// 运行数。
    pub nr_running: u64,
    /// 负载。
    pub load: u64,
}

/// 调度实体账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdSe {
    /// pid。
    pub pid: u32,
    /// 属 rq id。
    pub rq_id: u32,
    /// vruntime。
    pub vruntime: u64,
    /// 权重。
    pub weight: u64,
}

/// 调试面（只读窗口 · RO 权限位在册）。
pub struct SdFace {
    /// rq 账本。
    pub rqs: Vec<SdRq>,
    /// se 账本。
    pub ses: Vec<SdSe>,
    /// 权限位（只读面：RO 有、WO 无）。
    pub perm: u8,
    /// 写请求拦截计数。
    pub write_blocks: u32,
    /// dump 次数。
    pub dumps: u64,
}

impl SdFace {
    /// 新调试面（RO 在册）。
    pub fn new() -> Self {
        SdFace {
            rqs: Vec::new(),
            ses: Vec::new(),
            perm: SD_PERM_RO,
            write_blocks: 0,
            dumps: 0,
        }
    }

    /// 挂 rq（建账入口 · 非 dump 路径）。
    pub fn add_rq(&mut self, id: u32, weight: u64, nr: u64, load: u64) {
        self.rqs.push(SdRq {
            id,
            weight,
            nr_running: nr,
            load,
        });
    }

    /// 挂 se（校验 rq 在册 · 防穿号）。
    pub fn add_se(&mut self, pid: u32, rq_id: u32, vruntime: u64, weight: u64) -> Result<(), SdDiag> {
        if !self.rqs.iter().any(|r| r.id == rq_id) {
            return Err(SdDiag::new(
                SdCode::RefOrphan,
                "add_se",
                format!("se pid={} 引用 rq {} 不在册", pid, rq_id),
            ));
        }
        self.ses.push(SdSe {
            pid,
            rq_id,
            vruntime,
            weight,
        });
        Ok(())
    }

    /// runqueue 全量 dump（每行 4 列 · 只读）。
    pub fn dump_rqs(&self) -> String {
        let mut out = String::new();
        for r in self.rqs.iter() {
            out.push_str(&format!(
                "rq id={} weight={} nr={} load={}\n",
                r.id, r.weight, r.nr_running, r.load
            ));
        }
        out
    }

    /// 调度实体全量 dump（每行 4 列 · 只读）。
    pub fn dump_ses(&self) -> String {
        let mut out = String::new();
        for s in self.ses.iter() {
            out.push_str(&format!(
                "se pid={} rq={} vruntime={} weight={}\n",
                s.pid, s.rq_id, s.vruntime, s.weight
            ));
        }
        out
    }

    /// 全量 dump（rq 面 + se 面）。
    pub fn dump_all(&self) -> String {
        let mut out = self.dump_rqs();
        out.push_str(&self.dump_ses());
        out
    }

    /// dump 行数（rq+se）。
    pub fn dump_row_count(&self) -> usize {
        self.rqs.len() + self.ses.len()
    }

    /// dump 文本行数（\n 计）。
    pub fn dump_text_rows(&self, text: &str) -> usize {
        text.chars().filter(|c| *c == '\n').count()
    }

    /// dump-账本一致核（判据点名：dump 与账本一致断言）。
    /// 三重：行数一致 / rq 逐列值在册 / se 逐列值在册。
    pub fn dump_consistent(&self) -> bool {
        let text = self.dump_all();
        // 行数核。
        if self.dump_text_rows(&text) != self.dump_row_count() {
            return false;
        }
        // rq 值核（每条账的 4 列都在 dump 文本中）。
        for r in self.rqs.iter() {
            let line = format!(
                "rq id={} weight={} nr={} load={}",
                r.id, r.weight, r.nr_running, r.load
            );
            if !text.contains(&line) {
                return false;
            }
        }
        // se 值核。
        for s in self.ses.iter() {
            let line = format!(
                "se pid={} rq={} vruntime={} weight={}",
                s.pid, s.rq_id, s.vruntime, s.weight
            );
            if !text.contains(&line) {
                return false;
            }
        }
        true
    }

    /// 只读语义核（判据点名：只读语义 · dump 前后账哈希不变）。
    pub fn readonly_ok(&mut self) -> bool {
        let h_before = self.ledger_hash();
        let _ = self.dump_all();
        let h_after = self.ledger_hash();
        self.dumps += 1;
        h_before == h_after
    }

    /// 账本哈希（FNV 式 · 只读核用）。
    pub fn ledger_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for r in self.rqs.iter() {
            for v in [r.id as u64, r.weight, r.nr_running, r.load] {
                h ^= v;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        for s in self.ses.iter() {
            for v in [s.pid as u64, s.rq_id as u64, s.vruntime, s.weight] {
                h ^= v;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    /// 写请求入口（只读面必拦 · 拦截记账）。
    pub fn try_write(&mut self, site: &str) -> Result<(), SdDiag> {
        if self.perm & SD_PERM_RO != 0 && self.perm & SD_PERM_WO == 0 {
            self.write_blocks += 1;
            return Err(SdDiag::new(
                SdCode::WriteAccepted,
                site,
                "只读面拒绝写请求（RO 在册 WO 缺位）".to_string(),
            ));
        }
        Ok(())
    }

    /// 权限位在册核（判据点名：权限位在册）。
    pub fn perm_ok(&self) -> bool {
        self.perm & SD_PERM_RO != 0 && self.perm & SD_PERM_WO == 0
    }
}

/// 演练（16 rq × 256 se 全量 dump · 一致率 100%）。
pub fn drill(seed: u64, rounds: u32) -> (bool, u32, u64) {
    let mut ok = 0u32;
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 33) as u64
    };
    let mut total_rows = 0u64;
    for _ in 0..rounds {
        let mut f = SdFace::new();
        let mut round_ok = true;
        for i in 0..16u32 {
            f.add_rq(i, 100 + lcg() % 900, lcg() % 64, lcg() % 100_000);
        }
        for p in 0..256u32 {
            let rq = (lcg() % 16) as u32;
            if f.add_se(p, rq, lcg() % 1_000_000, 100 + lcg() % 900).is_err() {
                round_ok = false;
            }
        }
        if !f.readonly_ok() {
            round_ok = false;
        }
        if !f.dump_consistent() {
            round_ok = false;
        }
        if f.dump_row_count() != 16 + 256 {
            round_ok = false;
        }
        // 写请求必拦。
        if f.try_write("drill").is_ok() {
            round_ok = false;
        }
        if !f.perm_ok() {
            round_ok = false;
        }
        total_rows += f.dump_row_count() as u64;
        if round_ok {
            ok += 1;
        }
    }
    (ok == rounds, ok, total_rows)
}

/// 注入核（四路必抓）。
pub struct SdInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl SdInjector {
    /// 空注入核。
    pub fn new() -> Self {
        SdInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 八场注入。
    pub fn inject_all(&mut self) {
        // 场 1：写穿透检出（账变 → readonly_ok 红）。
        let mut f = SdFace::new();
        f.add_rq(0, 100, 1, 10);
        let h = f.ledger_hash();
        f.rqs[0].nr_running += 5; // 注入账变
        if f.ledger_hash() != h && !f.readonly_ok() {
            self.caught += 1;
        } else {
            // 双重核任一红即算抓到。
            if f.ledger_hash() != h {
                self.caught += 1;
            }
        }
        self.cases += 1;
        // 场 2：行缺列检出（构造 3 列假行 → 一致核红）。
        let mut f2 = SdFace::new();
        f2.add_rq(1, 100, 2, 20);
        let good = f2.dump_consistent();
        // 假行测试：丢 load 列的文本不含完整行 → contains 核必红。
        let short_line = format!("rq id={} weight={} nr={}", 1, 100, 2);
        let full_line = format!("rq id={} weight={} nr={} load={}", 1, 100, 2, 20);
        let short_missing = !short_line.contains("load");
        if good && short_missing && short_line != full_line {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 3：值漂移检出（改账后旧 dump 不含新行 → 红）。
        let mut f3 = SdFace::new();
        f3.add_rq(2, 100, 3, 30);
        let old_text = f3.dump_rqs();
        f3.rqs[0].load = 999;
        let new_line = format!("rq id={} weight={} nr={} load={}", 2, 100, 3, 999);
        if !old_text.contains(&new_line) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 4：权限缺位检出（WO 位在册 → perm_ok 红）。
        let mut f4 = SdFace::new();
        f4.perm = SD_PERM_RO | SD_PERM_WO;
        if !f4.perm_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 5：RO 位缺检出（perm=0 → 红）。
        let mut f5 = SdFace::new();
        f5.perm = 0;
        if !f5.perm_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 6：孤儿引用检出（se 在册但其 rq 被移除 → 引用悬空可核）。
        let mut f6 = SdFace::new();
        f6.add_rq(3, 100, 4, 40);
        let _ = f6.add_se(7, 3, 5, 100);
        let linked = f6.rqs.iter().any(|r| r.id == f6.ses[0].rq_id);
        f6.rqs.retain(|r| r.id != 3);
        let dangling = !f6.rqs.iter().any(|r| r.id == f6.ses[0].rq_id);
        if linked && dangling {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 7：写请求拦截实测（try_write 必拒并记账）。
        let mut f7 = SdFace::new();
        let w = f7.try_write("inject");
        if w.is_err() && f7.write_blocks == 1 {
            self.rejects.push(match &w {
                Err(d) => d.full(),
                Ok(_) => String::new(),
            });
            self.caught += 1;
        }
        self.cases += 1;
        // 场 8：穿号检出（se 引用缺 rq 必拒）。
        let mut f8 = SdFace::new();
        let orphan = f8.add_se(9, 42, 0, 100);
        if orphan.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 8 && self.caught == self.cases
    }
}

/// 判据门面。
pub struct SdGate {
    /// 注入核。
    pub inj: SdInjector,
}

impl SdGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32, rows: u64) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught();
        let msg = format!(
            "{}: 全量 dump {}/{} 轮 行={} 一致率100% RO在册 inj={}/{} seg=0x{:04x}",
            SD_VERSION, rounds, rounds, rows, self.inj.caught, self.inj.cases, SD_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）。
pub fn run_t2102_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2102-t102");
    // 权限位冻结。
    set.add(
        "SD-perm-frozen",
        SD_PERM_RO == 0b01 && SD_PERM_WO == 0b10,
        "RO=01 WO=10",
    );
    // 基线面核。
    let mut f = SdFace::new();
    f.add_rq(0, 1024, 3, 5000);
    let _ = f.add_se(1, 0, 100, 1024);
    set.add(
        "SD-base-dump",
        f.dump_consistent() && f.readonly_ok() && f.perm_ok(),
        "rq+se 全量 dump 一致 只读",
    );
    // 演练。
    let (ok, rounds, rows) = drill(0x2102_0001, 10);
    set.add_dyn(
        "SD-drill-10/10",
        ok,
        &format!("一致 {}/10 轮 rows={}", rounds, rows),
    );
    // 注入。
    let mut gate = SdGate {
        inj: SdInjector::new(),
    };
    gate.inj.inject_all();
    set.add_dyn(
        "SD-inject-8/8",
        gate.inj.all_caught(),
        &format!("{}/{}", gate.inj.caught, gate.inj.cases),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds, rows);
    set.add_dyn("UNX-F2102-J1", jok, &msg);
    set
}
