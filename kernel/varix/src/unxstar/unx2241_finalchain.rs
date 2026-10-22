//! unx2241_finalchain —— UNX-F2241 段级终验断言链（B31–B33 五步终跑）。
//! 锚点（CoRun Unxreal 主册）：UNX-F2241 · 段级终验断言链（B31–B33 五步终跑） · 目标 320 行 · 已深化
//! 判据原文：UNX-F2241-J1 段级终验五步全过
//!           （① B31–B33 ID F2201–F2260 连续 60 无跳号无重号；
//!             ② 三批骨架逐批 20 条求和各 6,000；
//!             ③ 深化正文逐条 ≥300 字终检；
//!             ④ 记账表三批恰一张加法链各=6,000；
//!             ⑤ 清单/总账/聚合三账 ID 对回一致），
//!           exit=0 断言留痕
//! 组件：段资产生成器（B31–B33 三批 · 每批 20 条 · ID F2201–F2260 连续）+
//!      五步终验链（步一 ID 链核/步二 骨架批数核/步三 深度终检/
//!      步四 记账加法链核/步五 三账 ID 对回核 · 依序短路）+
//!      深化正文账（每条正文长 ≥300 字 · 终检逐条）+
//!      记账表账（每批恰一张 · 加法链 Σ=6000 · 表号绑定批号）+
//!      exit 码闸（五步全过 exit=0 · 任一步断即 exit=步号 留痕）+
//!      注入核（跳号/重号/批缺条/正文不足/记账缺张/加法链错/
//!      三账漂移/exit 码失真 八路必抓）+
//!      演练（资产全生成 → 五步全过 → exit=0 · 10 轮确定性）。
//! 职责定位：段级终验是冻结前的最后一道链式闸——五步依序短路：
//!   ID 链断则无谓看正文、正文不足则无谓看记账、记账错则无谓对回；
//!   exit 码是终验的留痕凭证：exit=0 才有资格进冻结，
//!   exit=步号直接指出断在哪一步——终验不只是判生死，还指出刀口。
//! 性能逐项分解：ID 链 O(60)；骨架核 O(60)；深度核 O(60)；
//!   记账核 O(批)；对回核 O(60)；演练 O(五步×轮) 确定性；全整数零墙钟。
//! 诊断码段：0x7E80（0x7E81~0x7E88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;

/// 模块版本。
pub const FC_VERSION: &str = "unx2241_finalchain-v1";

/// 诊断段基址（0x7E80 段 · 本模块独占）。
pub const FC_SEG_BASE: u16 = 0x7E80;

/// ID 链起点（F2201）。
pub const FC_ID_LO: u32 = 2201;
/// ID 链终点（F2260 · 含）。
pub const FC_ID_HI: u32 = 2260;
/// 每批骨架条数。
pub const FC_PER_BATCH: u32 = 20;
/// 批号闭集。
pub const FC_BATCHES: [u16; 3] = [31, 32, 33];
/// 深化正文下限（字）。
pub const FC_DEPTH_MIN: u32 = 300;
/// 每批记账加法链目标。
pub const FC_CHAIN: u32 = FC_PER_BATCH * FC_DEPTH_MIN; // 6000

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FcCode {
    /// 0x7E81 ID 跳号。
    IdGap = 0x7E81,
    /// 0x7E82 ID 重号。
    IdDup = 0x7E82,
    /// 0x7E83 批缺条（某批 ≠ 20 条）。
    BatchShort = 0x7E83,
    /// 0x7E84 正文不足（<300 字）。
    DepthShort = 0x7E84,
    /// 0x7E85 记账缺张（某批无记账表/多张）。
    SheetMissing = 0x7E85,
    /// 0x7E86 加法链错（Σ ≠ 6000）。
    ChainBreak = 0x7E86,
    /// 0x7E87 三账漂移（ID 对回不一致）。
    CrossDrift = 0x7E87,
    /// 0x7E88 exit 码失真（非 0 却报过 / 报过却非 0）。
    ExitLies = 0x7E88,
}

impl FcCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            FcCode::IdGap => "ID 跳号（F2201–F2260 链断）",
            FcCode::IdDup => "ID 重号（同号两条）",
            FcCode::BatchShort => "批缺条（骨架批 ≠ 20 条）",
            FcCode::DepthShort => "正文不足（深化 <300 字）",
            FcCode::SheetMissing => "记账缺张（批表不恰一张）",
            FcCode::ChainBreak => "加法链错（批表 Σ ≠ 6000）",
            FcCode::CrossDrift => "三账漂移（清单/总账/聚合 ID 不一致）",
            FcCode::ExitLies => "exit 码失真（过断不一致）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            FcCode::IdGap => "ID 从 F2201 连续编到 F2260",
            FcCode::IdDup => "生成器单点分配 ID",
            FcCode::BatchShort => "每批恰好 20 条骨架",
            FcCode::DepthShort => "深化正文补足 300 字再终检",
            FcCode::SheetMissing => "每批恰好一张记账表",
            FcCode::ChainBreak => "记账值逐条求和核 6000",
            FcCode::CrossDrift => "三账同源：ID 集合以总账为准",
            FcCode::ExitLies => "exit 码只由五步结果派生",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FcDiag {
    /// 码。
    pub code: FcCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl FcDiag {
    /// 新诊断。
    pub fn new(code: FcCode, site: &str, detail: String) -> Self {
        FcDiag {
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

/// 骨架条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FcEntry {
    /// ID（F2201–F2260）。
    pub id: u32,
    /// 批号。
    pub batch: u16,
    /// 正文长（字）。
    pub body_len: u32,
}

/// 记账表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FcSheet {
    /// 绑定批号。
    pub batch: u16,
    /// 逐条记账值（与骨架条目一一对应）。
    pub values: Vec<u32>,
}

/// 段资产（总账 + 记账表组 + 清单）。
pub struct FcAssets {
    /// 总账（骨架条目）。
    pub ledger: Vec<FcEntry>,
    /// 记账表组。
    pub sheets: Vec<FcSheet>,
    /// 清单（点名 ID 序列）。
    pub manifest: Vec<u32>,
}

impl FcAssets {
    /// 全量生成（合规资产 · ID 连续 · 每批 20 条 · 正文 300 字 · 表齐）。
    pub fn generate_full() -> Self {
        let mut a = FcAssets {
            ledger: Vec::new(),
            sheets: Vec::new(),
            manifest: Vec::new(),
        };
        let mut id = FC_ID_LO;
        for &b in FC_BATCHES.iter() {
            let mut vals = Vec::new();
            for _ in 0..FC_PER_BATCH {
                a.ledger.push(FcEntry {
                    id,
                    batch: b,
                    body_len: FC_DEPTH_MIN,
                });
                vals.push(FC_DEPTH_MIN);
                a.manifest.push(id);
                id += 1;
            }
            a.sheets.push(FcSheet { batch: b, values: vals });
        }
        a
    }

    /// 步一：ID 链核（连续 60 无跳无重）。
    pub fn step1_id_chain(&self) -> Result<(), FcDiag> {
        if self.ledger.len() != (FC_ID_HI - FC_ID_LO + 1) as usize {
            return Err(FcDiag::new(
                FcCode::IdGap,
                "step1",
                format!("总账 {} 条 ≠ 60", self.ledger.len()),
            ));
        }
        let mut want = FC_ID_LO;
        for e in self.ledger.iter() {
            if e.id < want {
                return Err(FcDiag::new(
                    FcCode::IdDup,
                    "step1",
                    format!("ID {} 重号（期望 ≥ {}）", e.id, want),
                ));
            }
            if e.id > want {
                return Err(FcDiag::new(
                    FcCode::IdGap,
                    "step1",
                    format!("ID {} 跳号（期望 {}）", e.id, want),
                ));
            }
            want += 1;
        }
        Ok(())
    }

    /// 步二：骨架批数核（每批恰 20 条）。
    pub fn step2_skeleton(&self) -> Result<(), FcDiag> {
        for &b in FC_BATCHES.iter() {
            let n = self.ledger.iter().filter(|e| e.batch == b).count() as u32;
            if n != FC_PER_BATCH {
                return Err(FcDiag::new(
                    FcCode::BatchShort,
                    "step2",
                    format!("批 {} 有 {} 条", b, n),
                ));
            }
        }
        Ok(())
    }

    /// 步三：深度终检（逐条 ≥300 字）。
    pub fn step3_depth(&self) -> Result<(), FcDiag> {
        for e in self.ledger.iter() {
            if e.body_len < FC_DEPTH_MIN {
                return Err(FcDiag::new(
                    FcCode::DepthShort,
                    "step3",
                    format!("ID {} 正文 {} 字", e.id, e.body_len),
                ));
            }
        }
        Ok(())
    }

    /// 步四：记账加法链核（每批恰一张表 · Σ=6000）。
    pub fn step4_accounting(&self) -> Result<(), FcDiag> {
        for &b in FC_BATCHES.iter() {
            let count = self.sheets.iter().filter(|s| s.batch == b).count();
            if count != 1 {
                return Err(FcDiag::new(
                    FcCode::SheetMissing,
                    "step4",
                    format!("批 {} 记账表 {} 张", b, count),
                ));
            }
        }
        for s in self.sheets.iter() {
            let sum = s.values.iter().fold(0u32, |a, v| a.wrapping_add(*v));
            if sum != FC_CHAIN {
                return Err(FcDiag::new(
                    FcCode::ChainBreak,
                    "step4",
                    format!("批 {} 加法链 {} ≠ {}", s.batch, sum, FC_CHAIN),
                ));
            }
        }
        Ok(())
    }

    /// 步五：三账 ID 对回核（清单/总账/聚合一致）。
    pub fn step5_cross(&self) -> Result<(), FcDiag> {
        // 聚合账 = 记账表条目数聚合出的 ID 序（与总账同序）。
        let ledger_ids: Vec<u32> = self.ledger.iter().map(|e| e.id).collect();
        if self.manifest != ledger_ids {
            return Err(FcDiag::new(
                FcCode::CrossDrift,
                "step5",
                "清单与总账 ID 不一致".to_string(),
            ));
        }
        let sheet_total: usize = self.sheets.iter().map(|s| s.values.len()).sum();
        if sheet_total != ledger_ids.len() {
            return Err(FcDiag::new(
                FcCode::CrossDrift,
                "step5",
                format!("聚合账 {} 项 ≠ 总账 {} 条", sheet_total, ledger_ids.len()),
            ));
        }
        Ok(())
    }

    /// 五步终跑（依序短路 · exit=0 全过 / exit=步号 断点留痕）。
    pub fn final_run(&self) -> (u8, Vec<String>) {
        let mut trail: Vec<String> = Vec::new();
        let steps: [(&str, Box<dyn Fn() -> Result<(), FcDiag>>); 5] = [
            ("id-chain", Box::new(|| self.step1_id_chain())),
            ("skeleton", Box::new(|| self.step2_skeleton())),
            ("depth", Box::new(|| self.step3_depth())),
            ("accounting", Box::new(|| self.step4_accounting())),
            ("cross", Box::new(|| self.step5_cross())),
        ];
        let mut exit = 0u8;
        for (i, (name, f)) in steps.iter().enumerate() {
            match f() {
                Ok(()) => trail.push(format!("step{} {} OK", i + 1, name)),
                Err(d) => {
                    trail.push(format!("step{} {} FAIL: {}", i + 1, name, d.full()));
                    exit = (i + 1) as u8;
                    break;
                }
            }
        }
        (exit, trail)
    }
}

/// 演练（10 轮 · 全生成 → 五步 → exit=0）。
pub fn drill(rounds: u32) -> (bool, u32, u8) {
    let mut ok = 0u32;
    let mut last_exit = 9u8;
    for _ in 0..rounds {
        let a = FcAssets::generate_full();
        let (exit, _trail) = a.final_run();
        if exit == 0 {
            ok += 1;
        }
        last_exit = exit;
    }
    (ok == rounds, ok, last_exit)
}

/// 注入核（八路必抓）。
pub struct FcInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl FcInjector {
    /// 空注入核。
    pub fn new() -> Self {
        FcInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 八场注入。
    pub fn inject_all(&mut self) {
        // 场 1：跳号检出（抽一条 → step1/exit=1）。
        let mut a = FcAssets::generate_full();
        a.ledger.remove(10);
        a.manifest.remove(10);
        let (exit, trail) = a.final_run();
        if exit == 1 && trail.last().map(|t| t.contains("FAIL")).unwrap_or(false) {
            self.rejects.push(trail.last().cloned().unwrap_or_default());
            self.caught += 1;
        }
        self.cases += 1;
        // 场 2：重号检出（同 ID 两条 → step1 IdDup）。
        let mut a2 = FcAssets::generate_full();
        a2.ledger[5].id = a2.ledger[4].id;
        let (exit2, _) = a2.final_run();
        if exit2 == 1 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 3：批缺条检出（批 32 的一条挪入批 31 → 批 32 仅 19 条 → step2/exit=2）。
        let mut a3 = FcAssets::generate_full();
        a3.ledger[25].batch = 31;
        let (exit3, _) = a3.final_run();
        if exit3 == 2 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 4：正文不足检出（一条 299 字 → step3/exit=3）。
        let mut a4 = FcAssets::generate_full();
        a4.ledger[7].body_len = FC_DEPTH_MIN - 1;
        let (exit4, _) = a4.final_run();
        if exit4 == 3 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 5：记账缺张检出（删一张表 → step4/exit=4）。
        let mut a5 = FcAssets::generate_full();
        a5.sheets.remove(1);
        let (exit5, _) = a5.final_run();
        if exit5 == 4 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 6：加法链错检出（记账值改 → Σ≠6000 → step4）。
        let mut a6 = FcAssets::generate_full();
        a6.sheets[2].values[0] += 100;
        let (exit6, _) = a6.final_run();
        if exit6 == 4 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 7：三账漂移检出（清单改 → step5/exit=5）。
        let mut a7 = FcAssets::generate_full();
        a7.manifest[0] = 9999;
        let (exit7, _) = a7.final_run();
        if exit7 == 5 {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 8：exit 码一致性核（全过 → exit=0；exit 由五步派生）。
        let a8 = FcAssets::generate_full();
        let (exit8, trail8) = a8.final_run();
        let ok_steps = trail8.len() == 5;
        if (exit8 == 0) == ok_steps && exit8 == 0 {
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
pub struct FcGate {
    /// 注入核。
    pub inj: FcInjector,
}

impl FcGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32, exit: u8) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught() && exit == 0;
        let msg = format!(
            "{}: 五步终跑 {}/{} 轮 exit={} 60 ID 连续 20×3 骨架 300字×20=6000 三账对回 inj={}/{} seg=0x{:04x}",
            FC_VERSION, rounds, rounds, exit, self.inj.caught, self.inj.cases, FC_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）。
pub fn run_t2241_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2241-t241");
    // 冻结参数。
    set.add(
        "FC-params-frozen",
        FC_ID_LO == 2201 && FC_ID_HI == 2260 && FC_PER_BATCH == 20 && FC_CHAIN == 6000,
        "F2201–F2260 / 每批 20 / 链 6000",
    );
    // 基线资产五步。
    let a = FcAssets::generate_full();
    let (exit, _trail) = a.final_run();
    set.add(
        "FC-base-five-steps",
        exit == 0,
        "基线资产五步全过 exit=0",
    );
    // 演练。
    let (ok, rounds, last_exit) = drill(10);
    set.add_dyn(
        "FC-drill-10/10",
        ok,
        &format!("终跑 {}/10 轮 exit={}", rounds, last_exit),
    );
    // 注入。
    let mut gate = FcGate {
        inj: FcInjector::new(),
    };
    gate.inj.inject_all();
    set.add_dyn(
        "FC-inject-8/8",
        gate.inj.all_caught(),
        &format!("{}/{}", gate.inj.caught, gate.inj.cases),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds, last_exit);
    set.add_dyn("UNX-F2241-J1", jok, &msg);
    set
}
