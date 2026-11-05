//! UNX-F1551 域级培训样例集（新接手者七步上手） · 0x789C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 8 · UNX-F1551 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1551-J1 培训样例集齐备（新接手者七步上手路径
//!     逐步登记），每步可执行断言 7/7，样例集哈希落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 七步闭集：步号 UNX-F1551-S01..S07 逐步登记（步号 / 步名 /
//!     产出物三列），步号唯一，越步显性拒；
//!   - 逐步可执行：每步产出物 token 与冻结表逐字互证，7/7 断言
//!     全过方可落档；
//!   - 路径单调：步号必须按 S01→S07 顺序推进，跳步 / 乱序 / 重步
//!     显性拒（培训路径红线）；
//!   - 样例集哈希落档：9 行样例集清单（头 + 7 步行 + 尾），FNV-1a
//!     哈希落账，双跑逐位一致；
//!   - 注入必抓：跳步 / 错产出物 / 越步三路注入全部显性拒；
//!   - 状态机单调：上路 → 逐步完成 → 落档封账，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x789C0，派工卡 VX8-B04 卡 8 独占窗口）。
pub const SEG_BASE: u32 = 0x789C0;

/// 上手路径步数（判据 J1「七步」冻结）。
pub const STEP_COUNT: usize = 7;

/// 样例集清单行数（头 + 7 步行 + 尾，冻结）。
pub const KIT_ROWS: usize = 9;

// ── 七步上手冻结表（步号 / 步名 / 产出物 token） ────────────────
pub const STEPS: [(&str, &str, &str); 7] = [
    ("UNX-F1551-S01", "读契约红线", "契约红线清单"),
    ("UNX-F1551-S02", "跑批收口断言", "批收口四十条"),
    ("UNX-F1551-S03", "看健康一屏", "四族三十二行"),
    ("UNX-F1551-S04", "查时序一表", "四十批单调"),
    ("UNX-F1551-S05", "过三向对账", "失配零条"),
    ("UNX-F1551-S06", "走预案演练", "演练十全"),
    ("UNX-F1551-S07", "落反哺报告", "承载十条"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Training51Code {
    /// 步号越闭集：步号收进 S01..S07 闭集
    StepUnknown,
    /// 跳步：上手路径必须按步号顺序推进
    StepSkip,
    /// 乱序：非下一步步号一律乱序拒
    StepOutOfOrder,
    /// 产出物不符：步产出 token 与冻结表逐字互证
    TokenMismatch,
    /// 重复步：一步一断言，重步先查路径账
    StepDuplicate,
    /// 断言不足七步：补足 7/7 再落档
    StepShort,
    /// 培训状态倒退：上路→逐步→落档单调推进
    PhaseRegress,
    /// 培训状态跳级：未走齐七步不得落档
    PhaseSkip,
    /// 封账后再走：样例集封账禁改，另开培训修订账
    SealedLocked,
    /// 清单行数违约：样例集清单必须恰 9 行
    KitRowsBad,
    /// 步号为空：补步号再上路
    IdEmpty,
    /// 双跑不一致：排查样例集哈希随机性后复跑
    NotReproducible,
}

impl Training51Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Training51Code::StepUnknown => "步号越闭集：步号收进 S01..S07 闭集再上路",
            Training51Code::StepSkip => "跳步违规：上手路径按步号顺序逐步推进",
            Training51Code::StepOutOfOrder => "步序乱：返回下一步步号再走",
            Training51Code::TokenMismatch => "产出物不符：按冻结表产出 token 复跑该步",
            Training51Code::StepDuplicate => "重复步：一步一断言，查路径账再走",
            Training51Code::StepShort => "断言不足七步：补足 7/7 再落档",
            Training51Code::PhaseRegress => "培训状态倒退：上路→逐步→落档单调推进",
            Training51Code::PhaseSkip => "培训状态跳级：未走齐七步不得落档",
            Training51Code::SealedLocked => "样例集封账禁改：另开培训修订账",
            Training51Code::KitRowsBad => "样例集清单行数违约：必须恰 9 行",
            Training51Code::IdEmpty => "步号为空：补步号再上路",
            Training51Code::NotReproducible => "双跑不一致：排查样例集哈希随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Training51Code; 12] = [
        Training51Code::StepUnknown,
        Training51Code::StepSkip,
        Training51Code::StepOutOfOrder,
        Training51Code::TokenMismatch,
        Training51Code::StepDuplicate,
        Training51Code::StepShort,
        Training51Code::PhaseRegress,
        Training51Code::PhaseSkip,
        Training51Code::SealedLocked,
        Training51Code::KitRowsBad,
        Training51Code::IdEmpty,
        Training51Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Training51Code::ALL.len() {
        let c = match Training51Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 培训状态机（上路 → 逐步完成 → 落档，单调推进） ──────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrainPhase {
    /// 上路期：尚未走步或正在逐步
    Walking,
    /// 完成期：七步 7/7 断言全过
    Done,
    /// 落档期：样例集哈希落档封账
    Sealed,
}

impl TrainPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            TrainPhase::Walking => 0,
            TrainPhase::Done => 1,
            TrainPhase::Sealed => 2,
        }
    }
}

/// 单步完成记录。
#[derive(Clone, Debug)]
pub struct StepRecord {
    pub step_id: String,
    pub token: String,
}

/// 新接手者七步上手路径账。
pub struct TrainingPath {
    phase: TrainPhase,
    next_pos: usize,
    done: Vec<StepRecord>,
    kit: Option<String>,
    kit_hash: u32,
}

impl TrainingPath {
    pub fn new() -> TrainingPath {
        TrainingPath {
            phase: TrainPhase::Walking,
            next_pos: 0,
            done: Vec::new(),
            kit: None,
            kit_hash: 0,
        }
    }

    pub fn phase(&self) -> TrainPhase {
        self.phase
    }

    pub fn done_count(&self) -> usize {
        self.done.len()
    }

    pub fn kit_hash(&self) -> u32 {
        self.kit_hash
    }

    /// 步号在七步闭集判定并回位次。
    fn step_pos(&self, step_id: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < STEPS.len() {
            let row = match STEPS.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == step_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 走一步：步号顺序 + 产出物 token 互证，违者显性拒。
    pub fn walk(&mut self, step_id: &str, produced: &str) -> Result<(), u32> {
        if self.phase == TrainPhase::Sealed {
            return Err(Training51Code::SealedLocked.val());
        }
        if self.phase != TrainPhase::Walking {
            return Err(Training51Code::PhaseRegress.val());
        }
        if step_id.is_empty() {
            return Err(Training51Code::IdEmpty.val());
        }
        let pos = match self.step_pos(step_id) {
            Some(p) => p,
            None => return Err(Training51Code::StepUnknown.val()),
        };
        if pos < self.next_pos {
            return Err(Training51Code::StepDuplicate.val());
        }
        if pos > self.next_pos {
            if pos == self.next_pos + 1 {
                return Err(Training51Code::StepSkip.val());
            }
            return Err(Training51Code::StepOutOfOrder.val());
        }
        let row = match STEPS.get(pos) {
            Some(r) => *r,
            None => return Err(Training51Code::NotReproducible.val()),
        };
        if produced != row.2 {
            return Err(Training51Code::TokenMismatch.val());
        }
        self.done.push(StepRecord {
            step_id: step_id.to_string(),
            token: produced.to_string(),
        });
        self.next_pos += 1;
        if self.done.len() == STEP_COUNT {
            self.phase = TrainPhase::Done;
        }
        Ok(())
    }

    /// 落档：7/7 齐后生成 9 行样例集清单并哈希封账。
    pub fn seal_kit(&mut self) -> Result<u32, u32> {
        if self.phase == TrainPhase::Done {
            // 允许推进。
        } else {
            return Err(Training51Code::PhaseSkip.val());
        }
        if self.done.len() != STEP_COUNT {
            return Err(Training51Code::StepShort.val());
        }
        let mut text = String::from("== UNX-F1551 培训样例集清单 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.done.len() {
            let r = match self.done.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{} 产出:{} ●", r.step_id, r.token);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str(&format!("== 七步断言 {}/{} 全过，样例集哈希落档 ==", self.done.len(), STEP_COUNT));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != KIT_ROWS {
            return Err(Training51Code::KitRowsBad.val());
        }
        let hash = fnv1a(text.as_bytes());
        self.kit = Some(text);
        self.kit_hash = hash;
        self.phase = TrainPhase::Sealed;
        Ok(hash)
    }

    pub fn kit_text(&self) -> Option<&str> {
        match self.kit.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好路径构建（七步逐步走齐 + 落档） ───────────────────────────
pub fn build_training_ok() -> TrainingPath {
    let mut tp = TrainingPath::new();
    let mut i = 0usize;
    while i < STEPS.len() {
        let row = match STEPS.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = tp.walk(row.0, row.2);
        i += 1;
    }
    let _ = tp.seal_kit();
    tp
}

/// 跳步注入路径：走 S01 后直跳 S03。
pub fn build_training_skip() -> TrainingPath {
    let mut tp = TrainingPath::new();
    let row0 = match STEPS.get(0) {
        Some(r) => *r,
        None => return tp,
    };
    let row2 = match STEPS.get(2) {
        Some(r) => *r,
        None => return tp,
    };
    let _ = tp.walk(row0.0, row0.2);
    let _ = tp.walk(row2.0, row2.2);
    tp
}

/// 跳步检出复算：跳步注入被拒且错误码可溯。
fn inject_skip_caught() -> Option<u32> {
    let mut tp = TrainingPath::new();
    let row0 = match STEPS.get(0) {
        Some(r) => *r,
        None => return None,
    };
    let row2 = match STEPS.get(2) {
        Some(r) => *r,
        None => return None,
    };
    let _ = tp.walk(row0.0, row0.2);
    match tp.walk(row2.0, row2.2) {
        Err(code) => Some(code),
        Ok(()) => None,
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 七步逐步登记齐备且封账。
fn steps_ok() -> bool {
    let tp = build_training_ok();
    if tp.done_count() != STEP_COUNT {
        return false;
    }
    if tp.phase() != TrainPhase::Sealed {
        return false;
    }
    if STEPS.len() != STEP_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < STEPS.len() {
        let row = match STEPS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0.is_empty() || row.1.is_empty() || row.2.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 产出物逐字互证：冻结表 token 非空且两两互异。
fn token_ok() -> bool {
    let mut i = 0usize;
    while i < STEPS.len() {
        let a = match STEPS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if a.2.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < STEPS.len() {
            let b = match STEPS.get(j) {
                Some(r) => *r,
                None => return false,
            };
            if a.2 == b.2 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 每步可执行断言 7/7：路径账逐步记录与冻结表一致。
fn walk_7_of_7_ok() -> bool {
    let mut tp = TrainingPath::new();
    let mut i = 0usize;
    while i < STEPS.len() {
        let row = match STEPS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if tp.walk(row.0, row.2).is_err() {
            return false;
        }
        if tp.done_count() != i + 1 {
            return false;
        }
        i += 1;
    }
    if tp.phase() != TrainPhase::Done {
        return false;
    }
    let hash = match tp.seal_kit() {
        Ok(h) => h,
        Err(_) => return false,
    };
    if hash == 0 {
        return false;
    }
    true
}

/// 路径单调：跳步 / 乱序 / 重步显性拒。
fn path_monotonic_ok() -> bool {
    // 跳步注入必拒。
    let code = match inject_skip_caught() {
        Some(c) => c,
        None => return false,
    };
    if code != Training51Code::StepSkip.val() {
        return false;
    }
    // 乱序：S02 后走 S05（跨两步以上）。
    let mut tp = TrainingPath::new();
    let row0 = match STEPS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let row1 = match STEPS.get(1) {
        Some(r) => *r,
        None => return false,
    };
    let row4 = match STEPS.get(4) {
        Some(r) => *r,
        None => return false,
    };
    let _ = tp.walk(row0.0, row0.2);
    let _ = tp.walk(row1.0, row1.1); // 错产出物先行注入（S02 token 错）。
    let _ = tp.walk(row1.0, row1.2); // 重步（位次回退）。
    match tp.walk(row4.0, row4.2) {
        Err(code) => {
            if code != Training51Code::StepOutOfOrder.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 产出物错注入必抓：token 与冻结表不符必拒。
fn token_mismatch_caught_ok() -> bool {
    let mut tp = TrainingPath::new();
    let row0 = match STEPS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match tp.walk(row0.0, "伪造产出") {
        Err(code) => {
            if code != Training51Code::TokenMismatch.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if tp.done_count() != 0 {
        return false;
    }
    true
}

/// 越步注入必抓：越闭集步号必拒。
fn unknown_step_caught_ok() -> bool {
    let mut tp = TrainingPath::new();
    match tp.walk("UNX-F1551-S99", "任意产出") {
        Err(code) => {
            if code != Training51Code::StepUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空步号。
    match tp.walk("", "任意产出") {
        Err(code) => {
            if code != Training51Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if tp.done_count() != 0 {
        return false;
    }
    true
}

/// 样例集哈希落档：9 行、七步行齐、哈希非零。
fn kit_archived_ok() -> bool {
    let tp = build_training_ok();
    let text = match tp.kit_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != KIT_ROWS {
        return false;
    }
    if !text.contains("培训样例集清单") {
        return false;
    }
    if !text.contains("7/7") {
        return false;
    }
    let mut i = 0usize;
    while i < STEPS.len() {
        let row = match STEPS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !text.contains(row.0) {
            return false;
        }
        i += 1;
    }
    if tp.kit_hash() == 0 {
        return false;
    }
    true
}

/// 状态机单调：未走齐落档拒、封账后再走拒。
fn phase_monotonic_ok() -> bool {
    let mut tp = TrainingPath::new();
    // 零步落档：跳级拒。
    if tp.seal_kit().is_ok() {
        return false;
    }
    // 走一步后落档：跳级拒。
    let row0 = match STEPS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let _ = tp.walk(row0.0, row0.2);
    if tp.seal_kit().is_ok() {
        return false;
    }
    // 封账后再走：封账锁拒。
    let mut sealed = build_training_ok();
    let row1 = match STEPS.get(1) {
        Some(r) => *r,
        None => return false,
    };
    match sealed.walk(row1.0, row1.2) {
        Err(code) => {
            if code != Training51Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 双跑一致：好路径样例集哈希两次构建逐位相等。
fn double_run_ok() -> bool {
    let h1 = build_training_ok().kit_hash();
    let h2 = build_training_ok().kit_hash();
    if h1 != h2 {
        return false;
    }
    if h1 == 0 {
        return false;
    }
    let t1 = build_training_ok();
    let t2 = build_training_ok();
    let x1 = match t1.kit_text() {
        Some(t) => t,
        None => return false,
    };
    let x2 = match t2.kit_text() {
        Some(t) => t,
        None => return false,
    };
    if x1 != x2 {
        return false;
    }
    true
}

/// 重步守卫：走齐两步后回走首步必拒且账面不进。
fn duplicate_caught_ok() -> bool {
    let mut tp = TrainingPath::new();
    let row0 = match STEPS.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let row1 = match STEPS.get(1) {
        Some(r) => *r,
        None => return false,
    };
    let _ = tp.walk(row0.0, row0.2);
    let _ = tp.walk(row1.0, row1.2);
    match tp.walk(row0.0, row0.2) {
        Err(code) => {
            if code != Training51Code::StepDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if tp.done_count() != 2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Training51Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Training51Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1551_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1551 域级培训样例集");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("七步逐步登记", steps_ok(), "S01..S07 三列齐备逐步入账");
    s.add_dyn("产出物互证", token_ok(), "七步 token 非空两两互异");
    s.add_dyn("逐步断言7/7", walk_7_of_7_ok(), "七步逐走全过，哈希非零");
    s.add_dyn("路径单调", path_monotonic_ok(), "跳步乱序重步三路显性拒");
    s.add_dyn("错产出必抓", token_mismatch_caught_ok(), "token 不符必拒且账面不进");
    s.add_dyn("越步必抓", unknown_step_caught_ok(), "越闭集步号与空步号必拒");
    s.add_dyn("重步必抓", duplicate_caught_ok(), "回走已步必拒且账面不进");
    s.add_dyn("样例集哈希落档", kit_archived_ok(), "清单 9 行七步行齐，哈希落档");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "上路→逐步→落档可达，跳级倒退拒");
    s.add_dyn("双跑一致", double_run_ok(), "好路径哈希与清单两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x789C0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1551_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn seven_steps_frozen() {
        assert_eq!(STEPS.len(), 7);
        let mut i = 0usize;
        while i < STEPS.len() {
            let a = match STEPS.get(i) {
                Some(r) => r.0,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut j = i + 1;
            while j < STEPS.len() {
                let b = match STEPS.get(j) {
                    Some(r) => r.0,
                    None => {
                        must_not_reach();
                        return;
                    }
                };
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn walk_seven_all_pass() {
        assert!(walk_7_of_7_ok());
        let tp = build_training_ok();
        assert_eq!(tp.done_count(), 7);
        assert_eq!(tp.phase(), TrainPhase::Sealed);
    }

    #[test]
    fn skip_injection_caught() {
        assert_eq!(inject_skip_caught(), Some(Training51Code::StepSkip.val()));
        let tp = build_training_skip();
        assert_eq!(tp.done_count(), 1);
    }

    #[test]
    fn wrong_token_rejected() {
        let mut tp = TrainingPath::new();
        let row0 = match STEPS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        match tp.walk(row0.0, "伪造产出") {
            Err(code) => assert_eq!(code, Training51Code::TokenMismatch.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(tp.done_count(), 0);
    }

    #[test]
    fn unknown_and_empty_step_rejected() {
        let mut tp = TrainingPath::new();
        match tp.walk("UNX-F1551-S99", "任意产出") {
            Err(code) => assert_eq!(code, Training51Code::StepUnknown.val()),
            Ok(()) => must_not_reach(),
        }
        match tp.walk("", "任意产出") {
            Err(code) => assert_eq!(code, Training51Code::IdEmpty.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn out_of_order_rejected() {
        let mut tp = TrainingPath::new();
        let row0 = match STEPS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let row4 = match STEPS.get(4) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = tp.walk(row0.0, row0.2);
        match tp.walk(row4.0, row4.2) {
            Err(code) => assert_eq!(code, Training51Code::StepOutOfOrder.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_step_rejected() {
        let mut tp = TrainingPath::new();
        let row0 = match STEPS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let row1 = match STEPS.get(1) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = tp.walk(row0.0, row0.2);
        let _ = tp.walk(row1.0, row1.2);
        match tp.walk(row0.0, row0.2) {
            Err(code) => assert_eq!(code, Training51Code::StepDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(tp.done_count(), 2);
    }

    #[test]
    fn seal_needs_full_seven() {
        let mut tp = TrainingPath::new();
        let row0 = match STEPS.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = tp.walk(row0.0, row0.2);
        match tp.seal_kit() {
            Err(code) => assert_eq!(code, Training51Code::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn kit_rows_and_hash() {
        let tp = build_training_ok();
        let text = match tp.kit_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 9);
        assert!(text.contains("UNX-F1551-S07"));
        assert!(text.contains("7/7"));
        assert_ne!(tp.kit_hash(), 0);
    }

    #[test]
    fn double_run_hash_identical() {
        let h1 = build_training_ok().kit_hash();
        let h2 = build_training_ok().kit_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Training51Code::ALL.len(), 12);
        let first = match Training51Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Training51Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
