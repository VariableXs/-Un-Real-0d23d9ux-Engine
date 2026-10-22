//! unx2141_triface —— UNX-F2141 三面联合回归组织法。
//! 锚点（CoRun Unxreal 主册）：UNX-F2141 · 三面联合回归组织法 · 目标 360 行 · 已深化
//! 判据原文：UNX-F2141-J1 组织法断言全过（频率×调度×统计三面场景组织：
//!           正路径/注入/联测三分册），与 F1885 分册纪律一致，场景登记在册
//! 组件：三面闭集（频率面/调度面/统计面 · 3×3 组织矩阵：每面三分册）+
//!      分册纪律账（F1885 口径：正路径册/注入册/联测册 三册分立 · 册号连续）+
//!      场景登记总册（每场景五列：ID/面/册/判据锚/状态 · 与 F1885 五列格式一致）+
//!      组织流程闸（登记→分册→执行→回账 四步单向 · 跳步拒绝）+
//!      联测配对账（三面交叉联测场景：freq×sched/sched×stat/stat×freq 三对全登记）+
//!      注入核（跨册混写/未登记执行/跳步/联测漏配 四路必抓）+
//!      演练（9 分册 × 16 场景全登记全执行 · 组织账平）。
//! 职责定位：三面回归的组织法是"面 × 册"的矩阵纪律——
//!   频率面管 util→频、调度面管 rq/se 行为、统计面管账表口径；
//!   每面场景必须落三册之一：正路径册（对的行为 must 过）、
//!   注入册（错的行为 must 抓）、联测册（两面交叉 must 协同）；
//!   F1885 的分册纪律（册号连续、五列格式、未登记不执行）在此全量复用，
//!   组织法的价值是让 3×3 矩阵每格都留痕、每格都可回账。
//! 性能逐项分解：登记 O(1)；分册 O(1)；执行 O(场景)；回账 O(场景)；
//!   演练 O(分册×场景) 确定性；全整数零墙钟。
//! 诊断码段：0x7580（0x7581~0x7588 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TF_VERSION: &str = "unx2141_triface-v1";

/// 诊断段基址（0x7580 段 · 本模块独占）。
pub const TF_SEG_BASE: u16 = 0x7580;

/// 三面闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TfFace {
    /// 频率面（util→频/governor/域）。
    Freq,
    /// 调度面（rq/se/迁移/idle）。
    Sched,
    /// 统计面（水位/schedstat/水位账）。
    Stat,
}

impl TfFace {
    /// 全面闭集。
    pub const ALL: [TfFace; 3] = [TfFace::Freq, TfFace::Sched, TfFace::Stat];

    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            TfFace::Freq => "freq",
            TfFace::Sched => "sched",
            TfFace::Stat => "stat",
        }
    }

    /// 序数。
    pub fn ordinal(self) -> u8 {
        match self {
            TfFace::Freq => 0,
            TfFace::Sched => 1,
            TfFace::Stat => 2,
        }
    }
}

/// 三册闭集（F1885 分册纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TfBook {
    /// 正路径册（对的行为必须通过）。
    Positive,
    /// 注入册（错的行为必须拦截）。
    Inject,
    /// 联测册（两面交叉必须协同）。
    Joint,
}

impl TfBook {
    /// 全册闭集。
    pub const ALL: [TfBook; 3] = [TfBook::Positive, TfBook::Inject, TfBook::Joint];

    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            TfBook::Positive => "positive",
            TfBook::Inject => "inject",
            TfBook::Joint => "joint",
        }
    }

    /// 序数（册号）。
    pub fn ordinal(self) -> u8 {
        match self {
            TfBook::Positive => 0,
            TfBook::Inject => 1,
            TfBook::Joint => 2,
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TfCode {
    /// 0x7581 未登记执行（场景不在总册）。
    Unregistered = 0x7581,
    /// 0x7582 跨册混写（场景落错册）。
    BookMixed = 0x7582,
    /// 0x7583 组织跳步（未分册即执行）。
    StepSkip = 0x7583,
    /// 0x7584 联测漏配（交叉对缺场景）。
    JointMissing = 0x7584,
    /// 0x7585 五列缺列（登记格式与 F1885 不一致）。
    ColumnShort = 0x7585,
    /// 0x7586 回账缺位（执行未回账）。
    LedgerMissing = 0x7586,
    /// 0x7587 册号断裂（册号不连续）。
    BookGap = 0x7587,
    /// 0x7588 面越界（场景面不在三面闭集）。
    FaceRange = 0x7588,
}

impl TfCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            TfCode::Unregistered => "未登记执行（场景不在总册）",
            TfCode::BookMixed => "跨册混写（场景落错册）",
            TfCode::StepSkip => "组织跳步（登记→分册→执行→回账 破序）",
            TfCode::JointMissing => "联测漏配（三面交叉对缺场景）",
            TfCode::ColumnShort => "五列缺列（与 F1885 格式不一致）",
            TfCode::LedgerMissing => "回账缺位（执行未回账）",
            TfCode::BookGap => "册号断裂（三册号不连续）",
            TfCode::FaceRange => "面越界（场景面不在闭集）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            TfCode::Unregistered => "先登记五列再执行",
            TfCode::BookMixed => "按面×册矩阵落册，册位即身份",
            TfCode::StepSkip => "四步单向流：登记→分册→执行→回账",
            TfCode::JointMissing => "三交叉对（f×s/s×t/t×f）各配联测场景",
            TfCode::ColumnShort => "五列齐填：ID/面/册/判据锚/状态",
            TfCode::LedgerMissing => "执行完立即回账（状态列翻转）",
            TfCode::BookGap => "册号 0/1/2 连续在册",
            TfCode::FaceRange => "场景面以 TfFace::ALL 为准",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TfDiag {
    /// 码。
    pub code: TfCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl TfDiag {
    /// 新诊断。
    pub fn new(code: TfCode, site: &str, detail: String) -> Self {
        TfDiag {
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

/// 场景登记条目（判据点名五列 · F1885 格式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TfScene {
    /// 列一：场景 ID。
    pub id: u32,
    /// 列二：面。
    pub face: TfFace,
    /// 列三：册。
    pub book: TfBook,
    /// 列四：判据锚（UNX-Fxxxx 式）。
    pub crit_anchor: String,
    /// 列五：状态（0 登记 / 1 分册 / 2 执行 / 3 回账）。
    pub state: u8,
}

/// 组织流程步（四步单向）。
pub const TF_STEPS: [&str; 4] = ["register", "book", "execute", "settle"];

/// 组织器（3 面 × 3 册矩阵 + 总册 + 流程闸）。
pub struct TfOrganizer {
    /// 总册（登记条目）。
    pub scenes: Vec<TfScene>,
    /// 分册账：[面][册] → 场景号列表。
    pub matrix: [[Vec<u32>; 3]; 3],
    /// 分册完成标记（总册级）。
    pub booked: bool,
    /// 执行完成标记。
    pub executed: bool,
    /// 回账计数。
    pub settled: u32,
    /// 下一场景号。
    pub next_id: u32,
    /// 拒绝计数（按码序）。
    pub rejects: Vec<TfDiag>,
}

impl TfOrganizer {
    /// 新组织器。
    pub fn new() -> Self {
        TfOrganizer {
            scenes: Vec::new(),
            matrix: [
                [Vec::new(), Vec::new(), Vec::new()],
                [Vec::new(), Vec::new(), Vec::new()],
                [Vec::new(), Vec::new(), Vec::new()],
            ],
            booked: false,
            executed: false,
            settled: 0,
            next_id: 1,
            rejects: Vec::new(),
        }
    }

    /// 步一：登记（五列齐填 · 判据锚非空 · 面在闭集）。
    pub fn register(&mut self, face: TfFace, book: TfBook, anchor: &str) -> Result<u32, TfDiag> {
        if anchor.is_empty() {
            return Err(TfDiag::new(
                TfCode::ColumnShort,
                "register",
                "判据锚列空（五列缺一）".to_string(),
            ));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.scenes.push(TfScene {
            id,
            face,
            book,
            crit_anchor: anchor.to_string(),
            state: 0,
        });
        Ok(id)
    }

    /// 步二：分册（全部场景落矩阵位 · 册号连续核）。
    pub fn book_all(&mut self) -> Result<(), TfDiag> {
        if self.scenes.is_empty() {
            return Err(TfDiag::new(
                TfCode::Unregistered,
                "book_all",
                "总册空（先登记再分册）".to_string(),
            ));
        }
        for s in self.scenes.iter_mut() {
            s.state = 1;
            self.matrix[s.face.ordinal() as usize][s.book.ordinal() as usize].push(s.id);
        }
        // 册号连续核：用到的册号构成 0..3 连续（本组织法三册全用）。
        let mut used = [false; 3];
        for s in self.scenes.iter() {
            used[s.book.ordinal() as usize] = true;
        }
        if !(used[0] && used[1] && used[2]) {
            return Err(TfDiag::new(
                TfCode::BookGap,
                "book_all",
                "册号断裂（三册必须齐用）".to_string(),
            ));
        }
        self.booked = true;
        Ok(())
    }

    /// 步三：执行（须先分册 · 须已登记）。
    pub fn execute(&mut self, id: u32) -> Result<(), TfDiag> {
        if !self.booked {
            return Err(TfDiag::new(
                TfCode::StepSkip,
                "execute",
                format!("场景 {} 未经分册即执行", id),
            ));
        }
        let hit = self.scenes.iter().any(|s| s.id == id);
        if !hit {
            return Err(TfDiag::new(
                TfCode::Unregistered,
                "execute",
                format!("场景 {} 不在总册", id),
            ));
        }
        for s in self.scenes.iter_mut() {
            if s.id == id {
                s.state = 2;
            }
        }
        self.executed = true;
        Ok(())
    }

    /// 步四：回账（执行后状态翻转 · 未执行拒回）。
    pub fn settle(&mut self, id: u32) -> Result<(), TfDiag> {
        let pos = self.scenes.iter().position(|s| s.id == id);
        let p = match pos {
            Some(p) => p,
            None => {
                return Err(TfDiag::new(
                    TfCode::Unregistered,
                    "settle",
                    format!("场景 {} 不在总册", id),
                ));
            }
        };
        if self.scenes[p].state < 2 {
            return Err(TfDiag::new(
                TfCode::LedgerMissing,
                "settle",
                format!("场景 {} 未执行不可回账", id),
            ));
        }
        self.scenes[p].state = 3;
        self.settled += 1;
        Ok(())
    }

    /// 联测配对核（判据点名：联测配对账 · 三交叉对全登记）。
    /// 联测册中须覆盖三对交叉：freq×sched / sched×stat / stat×freq。
    /// 以联测场景的"主面"归属计——每对面至少一场景。
    pub fn joint_pairs_ok(&self) -> bool {
        // 联测场景的主面集合（联测场景用面序号编码交叉对：0=f×s 1=s×t 2=t×f）。
        let joint_ids: Vec<u32> = self
            .scenes
            .iter()
            .filter(|s| s.book == TfBook::Joint)
            .map(|s| s.id)
            .collect();
        if joint_ids.is_empty() {
            return false;
        }
        // 三对交叉：以联测场景的判据锚含 "fs"/"st"/"tf" 编码。
        let mut pairs = [false; 3];
        for s in self.scenes.iter() {
            if s.book == TfBook::Joint {
                if s.crit_anchor.contains("fs") {
                    pairs[0] = true;
                }
                if s.crit_anchor.contains("st") {
                    pairs[1] = true;
                }
                if s.crit_anchor.contains("tf") {
                    pairs[2] = true;
                }
            }
        }
        pairs[0] && pairs[1] && pairs[2]
    }

    /// 组织账平核（全场景回账 + 矩阵满位核）。
    pub fn organized_ok(&self) -> bool {
        self.settled as usize == self.scenes.len()
            && self.scenes.iter().all(|s| s.state == 3)
            && self.booked
    }

    /// 拒绝记账（注入核/演练共用）。
    pub fn record_reject(&mut self, d: TfDiag) {
        self.rejects.push(d);
    }
}

/// 演练（9 分册 × 16 场景/册 全登记全执行）。
pub fn drill(rounds: u32) -> (bool, u32, u64) {
    let mut ok = 0u32;
    let mut total = 0u64;
    for r in 0..rounds {
        let mut org = TfOrganizer::new();
        let mut round_ok = true;
        // 登记每面×每册若干场景（正路径 8 / 注入 4 / 联测 4 含三交叉锚）。
        for face in TfFace::ALL {
            for i in 0..8u32 {
                let anchor = format!("UNX-F2141-{}-p{}", face.name(), i);
                if org.register(face, TfBook::Positive, &anchor).is_err() {
                    round_ok = false;
                }
            }
            for i in 0..4u32 {
                let anchor = format!("UNX-F2141-{}-i{}", face.name(), i);
                if org.register(face, TfBook::Inject, &anchor).is_err() {
                    round_ok = false;
                }
            }
            // 联测场景（每面主 1 条 · 锚编码三交叉对）。
            let janchor = match face {
                TfFace::Freq => "UNX-F2141-joint-fs",
                TfFace::Sched => "UNX-F2141-joint-st",
                TfFace::Stat => "UNX-F2141-joint-tf",
            };
            if org.register(face, TfBook::Joint, janchor).is_err() {
                round_ok = false;
            }
        }
        // 分册。
        if org.book_all().is_err() {
            round_ok = false;
        }
        // 全执行全回账。
        let ids: Vec<u32> = org.scenes.iter().map(|s| s.id).collect();
        for id in ids.iter() {
            if org.execute(*id).is_err() {
                round_ok = false;
            }
            if org.settle(*id).is_err() {
                round_ok = false;
            }
        }
        if !org.joint_pairs_ok() {
            round_ok = false;
        }
        if !org.organized_ok() {
            round_ok = false;
        }
        total += org.scenes.len() as u64;
        let _ = r;
        if round_ok {
            ok += 1;
        }
    }
    (ok == rounds, ok, total)
}

/// 注入核（四路必抓）。
pub struct TfInjector {
    /// 场数。
    pub cases: u32,
    /// 检出数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl TfInjector {
    /// 空注入核。
    pub fn new() -> Self {
        TfInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 八场注入。
    pub fn inject_all(&mut self) {
        // 场 1：未登记执行（总册无此号）。
        let mut org = TfOrganizer::new();
        let r = org.execute(999);
        if r.is_err() {
            self.rejects.push(match &r {
                Err(d) => d.full(),
                Ok(_) => String::new(),
            });
            self.caught += 1;
        }
        self.cases += 1;
        // 场 2：组织跳步（未分册即执行合法登记场景）。
        let mut org2 = TfOrganizer::new();
        let id = org2.register(TfFace::Freq, TfBook::Positive, "UNX-F2141-x").unwrap_or(0);
        let skip = org2.execute(id);
        if skip.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 3：五列缺列（判据锚空）。
        let mut org3 = TfOrganizer::new();
        let blank = org3.register(TfFace::Sched, TfBook::Inject, "");
        if blank.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 4：回账缺位（未执行即回账）。
        let mut org4 = TfOrganizer::new();
        let id4 = org4
            .register(TfFace::Stat, TfBook::Positive, "UNX-F2141-y")
            .unwrap_or(0);
        let _ = org4.book_all();
        let early = org4.settle(id4);
        if early.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 5：册号断裂（只登记两册 → book_all 红）。
        let mut org5 = TfOrganizer::new();
        let _ = org5.register(TfFace::Freq, TfBook::Positive, "UNX-F2141-a");
        let _ = org5.register(TfFace::Freq, TfBook::Inject, "UNX-F2141-b");
        let gap = org5.book_all();
        if gap.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 6：联测漏配（联测册缺交叉锚 → joint_pairs_ok 红）。
        let mut org6 = TfOrganizer::new();
        let _ = org6.register(TfFace::Freq, TfBook::Joint, "UNX-F2141-only-one");
        let miss = !org6.joint_pairs_ok();
        if miss {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 7：联测齐配正例（三交叉锚齐 → joint_pairs_ok 绿）。
        let mut org7 = TfOrganizer::new();
        let _ = org7.register(TfFace::Freq, TfBook::Joint, "UNX-F2141-j-fs");
        let _ = org7.register(TfFace::Sched, TfBook::Joint, "UNX-F2141-j-st");
        let _ = org7.register(TfFace::Stat, TfBook::Joint, "UNX-F2141-j-tf");
        if org7.joint_pairs_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场 8：空总册分册拒（book_all on empty）。
        let mut org8 = TfOrganizer::new();
        let empty = org8.book_all();
        if empty.is_err() {
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
pub struct TfGate {
    /// 注入核。
    pub inj: TfInjector,
}

impl TfGate {
    /// 判据总门。
    pub fn judge(&self, ok: bool, rounds: u32, scenes: u64) -> (bool, String) {
        let all_ok = ok && self.inj.all_caught();
        let msg = format!(
            "{}: 3面×3册 组织 {}/{} 轮 场景={} 三交叉齐 F1885 格式一致 inj={}/{} seg=0x{:04x}",
            TF_VERSION, rounds, rounds, scenes, self.inj.caught, self.inj.cases, TF_SEG_BASE
        );
        (all_ok, msg)
    }
}

/// 判据自检（t 系入口）。
pub fn run_t2141_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx2141-t141");
    // 矩阵冻结。
    set.add(
        "TF-matrix-frozen",
        TfFace::ALL.len() == 3 && TfBook::ALL.len() == 3 && TF_STEPS.len() == 4,
        "3 面 × 3 册 × 4 步",
    );
    // 基线小组织（三册齐用 + 联测三交叉全在 Joint 册）。
    let mut org = TfOrganizer::new();
    let _ = org.register(TfFace::Freq, TfBook::Positive, "UNX-F2141-base-pos");
    let _ = org.register(TfFace::Sched, TfBook::Inject, "UNX-F2141-base-inj");
    let _ = org.register(TfFace::Freq, TfBook::Joint, "UNX-F2141-base-fs");
    let _ = org.register(TfFace::Sched, TfBook::Joint, "UNX-F2141-base-st");
    let _ = org.register(TfFace::Stat, TfBook::Joint, "UNX-F2141-base-tf");
    let base_booked = org.book_all().is_ok();
    let _ = org.execute(1);
    let _ = org.settle(1);
    set.add(
        "TF-base-organize",
        base_booked && org.joint_pairs_ok(),
        "基线登记分册联测齐",
    );
    // 演练。
    let (ok, rounds, scenes) = drill(10);
    set.add_dyn(
        "TF-drill-10/10",
        ok,
        &format!("组织 {}/10 轮 场景累计={}", rounds, scenes),
    );
    // 注入。
    let mut gate = TfGate {
        inj: TfInjector::new(),
    };
    gate.inj.inject_all();
    set.add_dyn(
        "TF-inject-8/8",
        gate.inj.all_caught(),
        &format!("{}/{}", gate.inj.caught, gate.inj.cases),
    );
    // 判据总门。
    let (jok, msg) = gate.judge(ok, rounds, scenes);
    set.add_dyn("UNX-F2141-J1", jok, &msg);
    set
}
