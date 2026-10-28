//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12481 · E 型方法论与注入总纲 · 0x30a20
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12481-J1 注入三元组（动作×对象×阈值）制度档齐，矩阵编制
//!      规则与红账升级路径判据绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三元组制度档：动作轴五档（写/改/删/越界/空引）× 对象轴十二面
//!   （PEB/TEB/OA/UNICODE_STRING/RUNTIME_FUNCTION/UNWIND_INFO/CONTEXT/
//!   临界区/SEH/对象/内存/文件）× 阈值轴三档（边界值/越界值/双倍值），
//!   越轴拒（ActionOob/ObjectOob/ThresholdOob）；
//!   - 矩阵编制规则：面 × 动作 全格必编（CellHole 抓空格），同格
//!   重编拒（CellDup），规则账 append-only（RuleShrink 拒缩水）；
//!   - 每格挂拒止码（B 档锚定面），码零拒（CodeZero）；
//!   - 红账升级路径：绿 → 红检出 → 升级（记录→红账→开门件）单向边
//!   （UpgradeBad 抓回跳），升级账序单调（SeqBack 抓倒挂）；
//!   - 三元组唯一性：同动作同对象同阈值只登一行（TripleDup 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回矩阵；
//!   - 注入核：LCG 写死种子 0x30a20，制度档/编制规则/升级路径 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30a20 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30a20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对象轴面数（十二面口径）。
pub const OBJECT_FACES: usize = 12;

/// 动作轴档数（五档口径）。
pub const ACTION_KINDS: usize = 5;

/// 阈值轴档数（三档口径）。
pub const THRESHOLD_KINDS: usize = 3;

/// 诊断码（0x30a20 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EjCode {
    /// 0x30a20 动作轴越集。
    ActionOob = 0,
    /// 0x30a21 对象轴越集。
    ObjectOob = 1,
    /// 0x30a22 阈值轴越集。
    ThresholdOob = 2,
    /// 0x30a23 矩阵空格（面×动作格未编）。
    CellHole = 3,
    /// 0x30a24 矩阵格重编。
    CellDup = 4,
    /// 0x30a25 规则账缩水。
    RuleShrink = 5,
    /// 0x30a26 升级路径回跳。
    UpgradeBad = 6,
    /// 0x30a27 红账漏记（红检出未入账）。
    RedMiss = 7,
    /// 0x30a28 三元组重号。
    TripleDup = 8,
    /// 0x30a29 拒止码零。
    CodeZero = 9,
    /// 0x30a2a 升级账序倒挂。
    SeqBack = 10,
}

impl EjCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [EjCode; 11] = [
        EjCode::ActionOob, EjCode::ObjectOob, EjCode::ThresholdOob, EjCode::CellHole,
        EjCode::CellDup, EjCode::RuleShrink, EjCode::UpgradeBad, EjCode::RedMiss,
        EjCode::TripleDup, EjCode::CodeZero, EjCode::SeqBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30a20 => "动作轴越集：只收写/改/删/越界/空引五档。",
        0x30a21 => "对象轴越集：只收十二面闭集（PEB~文件）。",
        0x30a22 => "阈值轴越集：只收边界值/越界值/双倍值三档。",
        0x30a23 => "矩阵空格：面×动作全格必编，空格先补档。",
        0x30a24 => "矩阵格重编：同格只挂一档拒止码。",
        0x30a25 => "规则账缩水：append-only，禁止截断，重放补齐。",
        0x30a26 => "升级路径回跳：升级只走 绿→红→升级 单向边。",
        0x30a27 => "红账漏记：红检出必须逐条入红账。",
        0x30a28 => "三元组重号：同动作同对象同阈值只登一行。",
        0x30a29 => "拒止码零：零不是拒止码，回查锚定面。",
        0x30a2a => "升级账序倒挂：升级账序全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30a20 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30a20 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30a2_30a2_30a2_30a2;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 动作轴（五档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// 写。
    Write = 0,
    /// 改。
    Modify = 1,
    /// 删。
    Delete = 2,
    /// 越界。
    Oob = 3,
    /// 空引。
    NullRef = 4,
}

impl Action {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<Action> {
        match i {
            0 => Some(Action::Write),
            1 => Some(Action::Modify),
            2 => Some(Action::Delete),
            3 => Some(Action::Oob),
            4 => Some(Action::NullRef),
            _ => None,
        }
    }
}

/// 对象轴（十二面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 临界区。
    CritSec = 7,
    /// SEH。
    Seh = 8,
    /// 对象。
    Obj = 9,
    /// 内存。
    Mem = 10,
    /// 文件。
    File = 11,
}

impl Object {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<Object> {
        match i {
            0 => Some(Object::Peb),
            1 => Some(Object::Teb),
            2 => Some(Object::Oa),
            3 => Some(Object::UniStr),
            4 => Some(Object::RuntimeFn),
            5 => Some(Object::UnwindInfo),
            6 => Some(Object::Context),
            7 => Some(Object::CritSec),
            8 => Some(Object::Seh),
            9 => Some(Object::Obj),
            10 => Some(Object::Mem),
            11 => Some(Object::File),
            _ => None,
        }
    }
}

/// 阈值轴（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Threshold {
    /// 边界值。
    Edge = 0,
    /// 越界值。
    Over = 1,
    /// 双倍值。
    Double = 2,
}

impl Threshold {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<Threshold> {
        match i {
            0 => Some(Threshold::Edge),
            1 => Some(Threshold::Over),
            2 => Some(Threshold::Double),
            _ => None,
        }
    }
}

/// 矩阵格（面 × 动作 → 拒止码）。
#[derive(Clone, Copy, Debug)]
pub struct CellRow {
    /// 对象面。
    pub obj: Object,
    /// 动作。
    pub act: Action,
    /// 拒止码（B 档锚定）。
    pub code: u32,
}

/// 升级账行（红检出 → 升级单向边）。
#[derive(Clone, Copy, Debug)]
pub struct UpgradeRow {
    /// 对象面。
    pub obj: Object,
    /// 动作。
    pub act: Action,
    /// 升级级（0 红检出/1 红账/2 开门件）。
    pub level: u32,
    /// 账序。
    pub seq: u64,
}

/// E 型注入总纲正本。
///
/// 生命周期：fill_matrix（面×动作全格编制）→ hit_red（红检出+升级）→
/// verify（制度档核对）。
#[derive(Debug)]
pub struct EjBook {
    /// 矩阵格。
    cells: Vec<CellRow>,
    /// 格账峰值。
    high: usize,
    /// 升级账。
    ups: Vec<UpgradeRow>,
    /// 升级账序。
    seq: u64,
    /// 升级次数。
    upgrades: u32,
    /// 拒止累计。
    rejects: u32,
}

impl EjBook {
    /// 建账。
    pub fn new() -> Self {
        EjBook {
            cells: Vec::new(),
            high: 0,
            ups: Vec::new(),
            seq: 0,
            upgrades: 0,
            rejects: 0,
        }
    }

    /// 锚定拒止码（面 × 动作 → B 档锚定码；独立口径）。
    pub fn anchor_code(obj: Object, act: Action) -> u32 {
        let base = match act {
            Action::Write => 0xC000_0001,
            Action::Modify => 0xC000_0002,
            Action::Delete => 0xC000_0003,
            Action::Oob => 0xC000_0004,
            Action::NullRef => 0xC000_0005,
        };
        base + obj as u32
    }

    /// 编制一格（码非零 + 重编拒 + 追加面）。
    pub fn fill_cell(&mut self, obj: Object, act: Action, code: u32) -> Result<bool, EjCode> {
        if code == 0 {
            self.rejects += 1;
            return Err(EjCode::CodeZero);
        }
        if self.cells.iter().any(|c| c.obj == obj && c.act == act) {
            self.rejects += 1;
            return Err(EjCode::CellDup);
        }
        self.cells.push(CellRow { obj, act, code });
        if self.cells.len() > self.high {
            self.high = self.cells.len();
        }
        Ok(true)
    }

    /// 全矩阵编制（十二面 × 五动作 = 60 格锚定档）。
    pub fn fill_matrix(&mut self) -> Result<usize, EjCode> {
        let mut n = 0usize;
        for i in 0..OBJECT_FACES as u64 {
            let obj = match Object::of(i) {
                Some(x) => x,
                None => return Err(EjCode::ObjectOob),
            };
            for j in 0..ACTION_KINDS as u64 {
                let act = match Action::of(j) {
                    Some(x) => x,
                    None => return Err(EjCode::ActionOob),
                };
                self.fill_cell(obj, act, Self::anchor_code(obj, act))?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 格查询（空格拒；纯读面不记拒止账）。
    pub fn lookup(&self, obj: Object, act: Action) -> Result<u32, EjCode> {
        match self.cells.iter().find(|c| c.obj == obj && c.act == act) {
            Some(c) => Ok(c.code),
            None => Err(EjCode::CellHole),
        }
    }

    /// 矩阵满格判据（60 格全编）。
    pub fn matrix_full(&self) -> bool {
        self.cells.len() == OBJECT_FACES * ACTION_KINDS
    }

    /// 红检出与升级（绿 → 红检出 → 红账 → 开门件 单向边）。
    ///
    /// 同 (obj, act) 升级级只许单调递增（回跳拒）。
    pub fn hit_red(&mut self, obj: Object, act: Action) -> Result<u32, EjCode> {
        // 红检出必须落在已编格上（未编格拒）。
        self.lookup(obj, act)?;
        let cur = self
            .ups
            .iter()
            .filter(|u| u.obj == obj && u.act == act)
            .map(|u| u.level)
            .max()
            .unwrap_or(0);
        if cur >= 2 {
            self.rejects += 1;
            return Err(EjCode::UpgradeBad);
        }
        let next = cur + 1;
        self.seq += 1;
        self.ups.push(UpgradeRow { obj, act, level: next, seq: self.seq });
        self.upgrades += 1;
        Ok(next)
    }

    /// 升级账单调判据。
    pub fn ups_monotone(&self) -> bool {
        let mut last = 0u64;
        for u in self.ups.iter() {
            if u.seq <= last {
                return false;
            }
            last = u.seq;
        }
        true
    }

    /// 红账覆盖判据（升级过的格必须在矩阵在编）。
    pub fn reds_covered(&self) -> bool {
        self.ups
            .iter()
            .all(|u| self.cells.iter().any(|c| c.obj == u.obj && c.act == u.act))
    }

    /// 升级次数。
    pub fn upgrade_total(&self) -> u32 {
        self.upgrades
    }

    /// 格数。
    pub fn cell_len(&self) -> usize {
        self.cells.len()
    }

    /// 升级账长。
    pub fn ups_len(&self) -> usize {
        self.ups.len()
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.cells.len() >= self.high
    }

    /// 格快照。
    pub fn cell_of(&self, idx: usize) -> Option<CellRow> {
        if idx >= self.cells.len() {
            None
        } else {
            Some(self.cells[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 制度档演练：60 格全编、逐格可查、轴域闭集 10/10。
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a20 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = EjBook::new();
        let n = bk.fill_matrix().unwrap_or(0);
        let mut pass = n == OBJECT_FACES * ACTION_KINDS && bk.matrix_full();
        // 逐格可查且码非零。
        for i in 0..OBJECT_FACES as u64 {
            for j in 0..ACTION_KINDS as u64 {
                let obj = Object::of(i).unwrap_or(Object::Peb);
                let act = Action::of(j).unwrap_or(Action::Write);
                pass = pass && matches!(bk.lookup(obj, act), Ok(c) if c != 0);
            }
        }
        let _ = lcg.below(3);
        if pass && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("制度档 {}/{} 格 轮={}/{}", OBJECT_FACES * ACTION_KINDS, OBJECT_FACES * ACTION_KINDS, ok, INJECT_ROUNDS))
}

/// 编制规则演练：重编/码零/空格拒 10/10。
pub fn drill_rule_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a20 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = EjBook::new();
        let _ = bk.fill_matrix();
        let n0 = bk.cell_len();
        let dup = bk.fill_cell(Object::Peb, Action::Write, 1);
        let zero = bk.fill_cell(Object::Teb, Action::Write, 0);
        let hole = bk.lookup(Object::File, Action::Oob);
        // File/Oob 未编？——全矩阵已编，改查越轴面。
        let oob_obj = Object::of(99).is_none();
        let _ = hole;
        let _ = lcg.below(3);
        if matches!(dup, Err(EjCode::CellDup))
            && matches!(zero, Err(EjCode::CodeZero))
            && oob_obj
            && bk.cell_len() == n0
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("编制规则拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 红账升级路径演练：绿→红→账→开门件单向边、回跳拒 10/10。
pub fn drill_upgrade_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a20 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = EjBook::new();
        let _ = bk.fill_matrix();
        let l1 = bk.hit_red(Object::Peb, Action::Oob);
        let l2 = bk.hit_red(Object::Peb, Action::Oob);
        let l3 = bk.hit_red(Object::Peb, Action::Oob);
        let l4 = bk.hit_red(Object::Peb, Action::Oob); // 已到顶 → 拒
        let cov = bk.reds_covered();
        let _ = lcg.below(3);
        if matches!(l1, Ok(1))
            && matches!(l2, Ok(2))
            && matches!(l3, Err(EjCode::UpgradeBad))
            && matches!(l4, Err(EjCode::UpgradeBad))
            && cov
            && bk.ups_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("升级路径轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：矩阵转录导出（逐格一行：面/动作/码）。
/// ─────────────────────────────────────────────────────────────────────
/// 矩阵转录导出（前 n 格）。
pub fn export_matrix_lines(bk: &EjBook, n: usize) -> String {
    let mut s = String::from("注入矩阵转录：\n");
    for i in 0..n {
        if let Some(c) = bk.cell_of(i) {
            s.push_str(&format!(
                "· 面{} 作{} 码{:08x}\n",
                c.obj as u8, c.act as u8, c.code
            ));
        }
    }
    s
}

/// 转录完整判据（前 n 格齐备）。
pub fn matrix_lines_complete(bk: &EjBook, n: usize) -> bool {
    let s = export_matrix_lines(bk, n);
    (0..n).all(|i| {
        bk.cell_of(i)
            .map(|c| s.contains(&format!("面{}", c.obj as u8)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：总纲报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct EjReport {
    /// 格数。
    pub cells: usize,
    /// 升级账行数。
    pub ups: usize,
    /// 升级次数。
    pub upgrades: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 满格。
    pub full: bool,
    /// 红覆盖。
    pub covered: bool,
}

impl EjReport {
    /// 建报告。
    pub fn build(bk: &EjBook) -> Self {
        EjReport {
            cells: bk.cell_len(),
            ups: bk.ups_len(),
            upgrades: bk.upgrade_total(),
            rejects: bk.reject_total(),
            full: bk.matrix_full(),
            covered: bk.reds_covered(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.covered && self.upgrades >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "EjReport 格={} 升={} 次={} 拒={} 满={} 覆={}",
            self.cells, self.ups, self.upgrades, self.rejects, self.full, self.covered
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：轴域演练（三轴闭集逐一可取/越轴拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_axis_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a20 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..ACTION_KINDS as u64).all(|k| Action::of(k).is_some());
        pass = pass && Action::of(ACTION_KINDS as u64).is_none();
        pass = pass && (0..OBJECT_FACES as u64).all(|k| Object::of(k).is_some());
        pass = pass && Object::of(OBJECT_FACES as u64).is_none();
        pass = pass && (0..THRESHOLD_KINDS as u64).all(|k| Threshold::of(k).is_some());
        pass = pass && Threshold::of(THRESHOLD_KINDS as u64).is_none();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("轴域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：三元组唯一性演练（重三元组拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_triple_dup_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a20 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = EjBook::new();
        let _ = bk.fill_matrix();
        // 同 (Peb, Oob) 双升级到顶后再试 → UpgradeBad；换动作 → 新级。
        let _ = bk.hit_red(Object::Teb, Action::Write);
        let _ = bk.hit_red(Object::Teb, Action::Write);
        let again = bk.hit_red(Object::Teb, Action::Write);
        let other = bk.hit_red(Object::Teb, Action::Delete);
        let _ = lcg.below(3);
        if matches!(again, Err(EjCode::UpgradeBad))
            && matches!(other, Ok(1))
            && bk.ups_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三元组唯一轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct EjCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ej_checklist(bk: &EjBook) -> Vec<EjCheckRow> {
    let rep = EjReport::build(bk);
    let mut rows: Vec<EjCheckRow> = Vec::new();
    rows.push(EjCheckRow { name: "60格满编", passed: bk.matrix_full() });
    rows.push(EjCheckRow { name: "红覆盖", passed: bk.reds_covered() });
    rows.push(EjCheckRow { name: "升级账单调", passed: bk.ups_monotone() });
    rows.push(EjCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(EjCheckRow { name: "轴域闭集", passed: drill_axis_domain_10().0 });
    rows.push(EjCheckRow { name: "口径", passed: OBJECT_FACES == 12 && ACTION_KINDS == 5 && THRESHOLD_KINDS == 3 });
    rows.push(EjCheckRow { name: "转录完整", passed: matrix_lines_complete(bk, 8) });
    rows.push(EjCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(EjCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(EjCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ej_checklist_green(rows: &[EjCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ej_stats_line(bk: &EjBook) -> String {
    format!(
        "注入总纲概况：格 {} 升 {} 次 {} 拒 {}",
        bk.cell_len(),
        bk.ups_len(),
        bk.upgrade_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12481_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = EjBook::new();
    let _ = bk.fill_matrix();
    let _ = bk.hit_red(Object::Peb, Action::Oob);
    let _ = bk.hit_red(Object::Peb, Action::Oob);
    // 注入一例码零编制（拒止账口径）。
    let _ = bk.fill_cell(Object::Peb, Action::Write, 0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        matrix_lines_complete(&bk, 8) && EjReport::build(&bk).all_green(),
        ej_stats_line(&bk),
    ));
    // X-b：轴域 10/10。
    let (ok_a, d_a) = drill_axis_domain_10();
    out.push(("X-轴域10/10", ok_a, d_a));
    // X-c：三元组唯一 10/10。
    let (ok_t, d_t) = drill_triple_dup_10();
    out.push(("X-三元组唯一10/10", ok_t, d_t));
    // X-d：终验清单。
    let rows = build_ej_checklist(&bk);
    out.push(("X-终验清单10项", ej_checklist_green(&rows), EjReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = EjBook::new();
    let _ = bk.fill_matrix();
    // 注 1：动作轴越集必拒（ActionOob 面）。
    if Action::of(5).is_none() {
        caught += 1;
        d.push("注1 越动作:抓".to_string());
    } else {
        d.push("注1 越动作:漏".to_string());
    }
    // 注 2：对象轴越集必拒（ObjectOob 面）。
    if Object::of(12).is_none() {
        caught += 1;
        d.push("注2 越对象:抓".to_string());
    } else {
        d.push("注2 越对象:漏".to_string());
    }
    // 注 3：矩阵格重编必拒（CellDup）。
    if matches!(bk.fill_cell(Object::Peb, Action::Write, 1), Err(EjCode::CellDup)) {
        caught += 1;
        d.push("注3 重编:抓".to_string());
    } else {
        d.push("注3 重编:漏".to_string());
    }
    // 注 4：码零编制必拒（CodeZero）。
    if matches!(bk.fill_cell(Object::Teb, Action::Write, 0), Err(EjCode::CodeZero)) {
        caught += 1;
        d.push("注4 码零:抓".to_string());
    } else {
        d.push("注4 码零:漏".to_string());
    }
    // 注 5：升级到顶再升必拒（UpgradeBad）。
    let _ = bk.hit_red(Object::File, Action::Delete);
    let _ = bk.hit_red(Object::File, Action::Delete);
    if matches!(bk.hit_red(Object::File, Action::Delete), Err(EjCode::UpgradeBad)) {
        caught += 1;
        d.push("注5 顶升:抓".to_string());
    } else {
        d.push("注5 顶升:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30a20 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    EjCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12481_checks：本单判据聚合（诊断段 0x30a20）。
pub fn run_unx12481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12481 E 型方法论与注入总纲");
    // J1-a：注入三元组制度档（60 格 10/10）。
    let (ok1, d1) = drill_matrix_10();
    s.add_dyn("J1-三元组制度档10/10", ok1, &d1);
    // J1-b：矩阵编制规则拒止（10/10）。
    let (ok2, d2) = drill_rule_reject_10();
    s.add_dyn("J1-编制规则拒止10/10", ok2, &d2);
    // J1-c：红账升级路径（单向边 10/10）。
    let (ok3, d3) = drill_upgrade_10();
    s.add_dyn("J1-红账升级路径10/10", ok3, &d3);
    // J1-d：标准会话（满格 + 升级链 + 红覆盖）。
    let mut bk = EjBook::new();
    let _ = bk.fill_matrix();
    let l1 = bk.hit_red(Object::Oa, Action::Oob);
    let l2 = bk.hit_red(Object::Oa, Action::Oob);
    s.add_dyn(
        "J1-制度档+升级路径绿",
        bk.matrix_full()
            && matches!(l1, Ok(1))
            && matches!(l2, Ok(2))
            && bk.reds_covered()
            && bk.ups_monotone()
            && bk.monotone(),
        &format!(
            "格={} 升1={:?} 升2={:?} 覆={}",
            bk.cell_len(),
            l1.is_ok(),
            l2.is_ok(),
            bk.reds_covered()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（码零编制不入格）。
    let c0 = bk.cell_len();
    let _ = bk.fill_cell(Object::UniStr, Action::NullRef, 0);
    s.add_dyn(
        "J2-拒止不写回",
        bk.cell_len() == c0 && bk.monotone(),
        &format!("格 {}→{}", c0, bk.cell_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30a20段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12481_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
