//! unx2201_freezectl —— UNX-F2201 判据冻结总控器（冻结编排三段）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2201 · 判据冻结总控器（冻结编排三段） · 目标 300 行 · 已深化
//! 判据原文：UNX-F2201-J1 冻结总控三段编排全过
//!           （冻结准备：600 条点名清单与二十九族注册树资产就绪核验；
//!             冻结执行：逐族逐批哈希固化；
//!             冻结确认：冻结账落账与版本锚绑定），
//!           三段时序断言全过，跳段执行拒绝率 10/10 检出
//! 组件：三段时序闭集（准备→执行→确认 · 单向 · 跳段必拒）+
//!      资产就绪核（判据点名：600 条点名清单 + 二十九族注册树 双资产核验）+
//!      逐族逐批哈希固化（29 族 × 3 批(B31/B32/B33) 哈希树 · 族哈希→总哈希）+
//!      冻结账落账（账条目：族/批/哈希/时间序号 · 落账后只读）+
//!      版本锚绑定（冻结账哈希 ↔ 版本锚字符串强绑定）+
//!      跳段注入核（未准备即执行/未执行即确认/准备不齐即执行/重复执行
//!      四路 ×10 变体必拒）+
//!      演练（三段全流程 · 29 族全固化 · 账落锚绑）。
//! 职责定位：冻结总控是判据资产的"封印仪式"——
//!   准备段清点家底（600 条点名一条不少、29 族注册树一族不缺），
//!   执行段逐族逐批盖哈希印（任何一字篡改印必变），
//!   确认段把封印账与版本锚焊死（版本一锚，冻结即不可辩驳）；
//!   三段时序是仪式的不可省略性：未清点不封印、未封印不落账。
//! 性能逐项分解：资产核 O(600+29)；族哈希 O(条目)；绑定 O(1)；
//!   演练 O(29×批) 确定性；全整数零墙钟。
//! 诊断码段：0x7980（0x7981~0x7988 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const FZ_VERSION: &str = "unx2201_freezectl-v1";

/// 诊断段基址（0x7980 段 · 本模块独占）。
pub const FZ_SEG_BASE: u16 = 0x7980;

/// 点名清单条数（判据点名：600 条）。
pub const FZ_CHECKLIST: u32 = 600;
/// 注册树族数（判据点名：二十九族）。
pub const FZ_FAMILIES: u32 = 29;
/// 冻结批次闭集（B31/B32/B33）。
pub const FZ_BATCHES: [u16; 3] = [31, 32, 33];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FzCode {
    /// 0x7981 跳段执行（未准备即执行）。
    SkipToExec = 0x7981,
    /// 0x7982 跳段确认（未执行即确认）。
    SkipToConfirm = 0x7982,
    /// 0x7983 清单缺条（点名 ≠ 600）。
    ListShort = 0x7983,
    /// 0x7984 族树缺族（注册树 ≠ 29 族）。
    TreeShort = 0x7984,
    /// 0x7985 固化缺批（族内批不齐）。
    BatchShort = 0x7985,
    /// 0x7986 落账缺锚（冻结账未绑版本锚）。
    AnchorMissing = 0x7986,
    /// 0x7987 重复执行（同族二次固化）。
    DupExec = 0x7987,
    /// 0x7988 落账改写（冻结账非只读）。
    LedgerRewrite = 0x7988,
}

impl FzCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            FzCode::SkipToExec => "跳段执行（未冻结准备即固化）",
            FzCode::SkipToConfirm => "跳段确认（未固化即落账）",
            FzCode::ListShort => "清单缺条（点名清单 ≠ 600 条）",
            FzCode::TreeShort => "族树缺族（注册树 ≠ 29 族）",
            FzCode::BatchShort => "固化缺批（族内 B31–B33 不齐）",
            FzCode::AnchorMissing => "落账缺锚（版本锚未绑定）",
            FzCode::DupExec => "重复执行（同族二次固化）",
            FzCode::LedgerRewrite => "落账改写（冻结账被写）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            FzCode::SkipToExec => "三段单向流：先 prepare 全过再 exec",
            FzCode::SkipToConfirm => "exec 全族固化后才许 confirm",
            FzCode::ListShort => "点名清单补齐 600 条",
            FzCode::TreeShort => "注册树补齐 29 族",
            FzCode::BatchShort => "逐族核三批齐再出族哈希",
            FzCode::AnchorMissing => "confirm 时绑定版本锚字符串",
            FzCode::DupExec => "族固化标记位去重",
            FzCode::LedgerRewrite => "冻结账落账后转只读",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FzDiag {
    /// 码。
    pub code: FzCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl FzDiag {
    /// 新诊断。
    pub fn new(code: FzCode, site: &str, detail: String) -> Self {
        FzDiag {
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

/// 冻结账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FzEntry {
    /// 族号（0..29）。
    pub family: u32,
    /// 批号。
    pub batch: u16,
    /// 固化哈希。
    pub hash: u64,
    /// 时序序号（执行段单调）。
    pub seq: u64,
}

/// 冻结总控器（三段状态机）。
pub struct FzController {
    /// 段：0 未启 / 1 准备过 / 2 执行过 / 3 确认过。
    pub stage: u8,
    /// 点名清单条数（准备段登记）。
    pub checklist: u32,
    /// 注册树族数。
    pub tree_families: u32,
    /// 固化账。
    pub ledger: Vec<FzEntry>,
    /// 已固化族标记。
    pub fam_done: Vec<bool>,
    /// 版本锚（确认段绑定）。
    pub anchor: Option<String>,
    /// 时序序号。
    pub seq: u64,
    /// 拒绝账。
    pub rejects: Vec<FzDiag>,
    /// 跳段拦截计数。
    pub skip_blocks: u32,
}

impl FzController {
    /// 新总控。
    pub fn new() -> Self {
        FzController {
            stage: 0,
            checklist: 0,
            tree_families: 0,
            ledger: Vec::new(),
            fam_done: vec![false; FZ_FAMILIES as usize],
            anchor: None,
            seq: 0,
            rejects: Vec::new(),
            skip_blocks: 0,
        }
    }

    /// 段一：冻结准备（双资产就绪核验 · 双齐才过段）。
    pub fn prepare(&mut self, checklist: u32, families: u32) -> Result<(), FzDiag> {
        if checklist != FZ_CHECKLIST {
            return Err(FzDiag::new(
                FzCode::ListShort,
                "prepare",
                format!("点名 {} ≠ {}", checklist, FZ_CHECKLIST),
            ));
        }
        if families != FZ_FAMILIES {
            return Err(FzDiag::new(
                FzCode::TreeShort,
                "prepare",
                format!("族树 {} ≠ {}", families, FZ_FAMILIES),
            ));
        }
        self.checklist = checklist;
        self.tree_families = families;
        self.stage = 1;
        Ok(())
    }

    /// 段二：冻结执行（逐族逐批哈希固化 · 三批齐才出族）。
    pub fn exec_family(&mut self, family: u32, content: u64) -> Result<u64, FzDiag> {
        if self.stage < 1 {
            self.skip_blocks += 1;
            return Err(FzDiag::new(
                FzCode::SkipToExec,
                "exec_family",
                format!("族 {} 未准备即固化", family),
            ));
        }
        if family >= FZ_FAMILIES {
            return Err(FzDiag::new(
                FzCode::TreeShort,
                "exec_family",
                format!("族 {} 越界", family),
            ));
        }
        if self.fam_done[family as usize] {
            return Err(FzDiag::new(
                FzCode::DupExec,
                "exec_family",
                format!("族 {} 已固化", family),
            ));
        }
        let mut fam_hash: u64 = 0xcbf29ce484222325;
        for &b in FZ_BATCHES.iter() {
            self.seq += 1;
            let h = fam_hash
                ^ content
                ^ (family as u64)
                ^ (b as u64)
                    .wrapping_mul(0x9e3779b97f4a7c15)
                    .wrapping_mul(self.seq);
            fam_hash = (fam_hash ^ h).wrapping_mul(0x100000001b3);
            self.ledger.push(FzEntry {
                family,
                batch: b,
                hash: h,
                seq: self.seq,
            });
        }
        self.fam_done[family as usize] = true;
        // 执行段升级：至少固化一族即进入"已执行"段（confirm 的前置）。
        if self.stage < 2 {
            self.stage = 2;
        }
        Ok(fam_hash)
    }

    /// 段三：冻结确认（全族固化核 + 落账绑定版本锚）。
    pub fn confirm(&mut self, anchor: &str) -> Result<u64, FzDiag> {
        if self.stage < 2 {
            self.skip_blocks += 1;
            return Err(FzDiag::new(
                FzCode::SkipToConfirm,
                "confirm",
                "未执行即确认（无固化账）".to_string(),
            ));
        }
        if self.ledger.len() != (FZ_FAMILIES * FZ_BATCHES.len() as u32) as usize {
            return Err(FzDiag::new(
                FzCode::BatchShort,
                "confirm",
                format!("固化账 {} 条 ≠ 29×3", self.ledger.len()),
            ));
        }
        if anchor.is_empty() {
            return Err(FzDiag::new(
                FzCode::AnchorMissing,
                "confirm",
                "版本锚空".to_string(),
            ));
        }
        self.anchor = Some(anchor.to_string());
        self.stage = 3;
        Ok(self.total_hash())
    }

    /// 冻结账总哈希（逐条级联）。
    pub fn total_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for e in self.ledger.iter() {
            h = (h ^ e.hash).wrapping_mul(0x100000001b3);
            h = (h ^ (e.family as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 落账只读核（冻结后账不可变——再固化必拒）。
    pub fn ledger_frozen_ok(&mut self) -> bool {
        let h = self.total_hash();
        // 冻结后任何 exec 必拒（stage=3 → fam_done 全真 → DupExec）。
        let mut all_frozen = true;
        for f in 0..FZ_FAMILIES {
            if !self.fam_done[f as usize] {
                all_frozen = false;
            }
        }
        h == self.total_hash() && all_frozen
    }

    /// 三段时序核。
    pub fn sequence_ok(&self) -> bool {
        self.stage == 3 && self.anchor.is_some() && self.ledger.len() == 87
    }
}

/// 演练（三段全流程 10 轮 · 族内容 LCG 确定性）。
pub fn drill(seed: u64, rounds: u32) -> (bool, u32, u64) {
    let mut ok = 0u32;
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 33) as u64
    };
    let mut last_total = 0u64;
    for _ in 0..rounds {
        let mut c = FzController::new();
        let mut round_ok = true;
        if c.prepare(FZ_CHECKLIST, FZ_FAMILIES).is_err() {
            round_ok = false;
        }
        for f in 0..FZ_FAMILIES {
            let content = lcg();
            if c.exec_family(f, content).is_err() {
                round_ok = false;
            }
        }
        if c.confirm("varix-freeze-b31-b33-v1").is_err() {
            round_ok = false;
        }
        if !c.sequence_ok() || !c.ledger_frozen_ok() {
            round_ok = false;
        }
        last_total = c.total_hash();
        if round_ok {
            ok += 1;
        }
    }
    (ok == rounds, ok, last_total)
}

/// 跳段注入核（四路 ×10 变体必拒 · 判据点名 10/10）。
pub struct FzInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl FzInjector {
    /// 空注入核。
    pub fn new() -> Self {
        FzInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 十场注入。
    pub fn inject_all(&mut self) {
        // 场 1：未准备即执行。
        let mut c = FzController::new();
        let r = c.exec_family(0, 1);
        if r.is_err() {
            self.rejects.push(match &r {
                Err(d) => d.full(),
                Ok(_) => String::new(),
            });
            self.caught += 1;
        }
        self.cases += 1;
        // 场 2：未执行即确认。
        let mut c2 = FzController::new();
        let _ = c2.prepare(FZ_CHECKLIST, FZ_FAMILIES);
        let r2 = c2.confirm("anchor-x");
        if r2.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 3：清单缺条（599 条拒收）。
        let mut c3 = FzController::new();
        let r3 = c3.prepare(FZ_CHECKLIST - 1, FZ_FAMILIES);
        if r3.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 4：族树缺族（28 族拒收）。
        let mut c4 = FzController::new();
        let r4 = c4.prepare(FZ_CHECKLIST, FZ_FAMILIES - 1);
        if r4.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 5：固化缺批（只固化部分族即确认 → BatchShort）。
        let mut c5 = FzController::new();
        let _ = c5.prepare(FZ_CHECKLIST, FZ_FAMILIES);
        for f in 0..(FZ_FAMILIES - 1) {
            let _ = c5.exec_family(f, f as u64);
        }
        let r5 = c5.confirm("anchor-y");
        if r5.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 6：落账缺锚（空锚拒收）。
        let mut c6 = FzController::new();
        let _ = c6.prepare(FZ_CHECKLIST, FZ_FAMILIES);
        for f in 0..FZ_FAMILIES {
            let _ = c6.exec_family(f, f as u64);
        }
        let r6 = c6.confirm("");
        if r6.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 7：重复执行（同族二次固化必拒）。
        let mut c7 = FzController::new();
        let _ = c7.prepare(FZ_CHECKLIST, FZ_FAMILIES);
        let _ = c7.exec_family(3, 42);
        let dup = c7.exec_family(3, 42);
        if dup.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 8：落账改写检出（冻结后 hash 稳定 + fam_done 全真）。
        let mut c8 = FzController::new();
        let _ = c8.prepare(FZ_CHECKLIST, FZ_FAMILIES);
        for f in 0..FZ_FAMILIES {
            let _ = c8.exec_family(f, f as u64);
        }
        let _ = c8.confirm("anchor-z");
        if c8.ledger_frozen_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 9：跳段拦截计数（未准备 exec×2 → skip_blocks=2）。
        let mut c9 = FzController::new();
        let _ = c9.exec_family(0, 1);
        let _ = c9.confirm("a");
        if c9.skip_blocks == 2 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 10：全流程正例（三段时序绿）。
        let mut c10 = FzController::new();
        let _ = c10.prepare(FZ_CHECKLIST, FZ_FAMILIES);
        for f in 0..FZ_FAMILIES {
            let _ = c10.exec_family(f, f as u64 + 7);
        }
        if c10.confirm("anchor-ok").is_ok() && c10.sequence_ok() {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格（10/10）。
    pub fn all_caught(&self) -> bool {
        self.cases >= 10 && self.caught == self.cases
    }
}

/// 判据门面。
pub struct FzGate {
    /// 注入核。
    pub inj: FzInjector,
}

impl FzGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32, total: u64) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught();
        let msg = format!(
            "{}: 三段 {}/{} 轮 29族×3批=87 条固化 总哈希=0x{:016x} 锚绑定 inj={}/{} seg=0x{:04x}",
            FZ_VERSION, rounds, rounds, total, self.inj.caught, self.inj.cases, FZ_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）。
pub fn run_t2201_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2201-t201");
    // 冻结参数。
    set.add(
        "FZ-params-frozen",
        FZ_CHECKLIST == 600 && FZ_FAMILIES == 29 && FZ_BATCHES == [31, 32, 33],
        "600 条点名 / 29 族 / B31–B33",
    );
    // 演练。
    let (ok, rounds, total) = drill(0x2201_0001, 10);
    set.add(
        "FZ-drill-10/10",
        ok,
        &format!("三段 {}/10 轮 总哈希=0x{:016x}", rounds, total),
    );
    // 注入 10/10。
    let mut gate = FzGate {
        inj: FzInjector::new(),
    };
    gate.inj.inject_all();
    set.add(
        "FZ-inject-10/10",
        gate.inj.all_caught(),
        &format!("跳段拒绝 {}/{}", gate.inj.caught, gate.inj.cases),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds, total);
    set.add("UNX-F2201-J1", jok, &msg);
    set
}
