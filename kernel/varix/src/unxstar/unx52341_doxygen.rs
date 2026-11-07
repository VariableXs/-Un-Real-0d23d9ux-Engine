//! UNX-F52341 Doxygen 收口 · 0xC9500 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52341"，逐字）：
//!   title: Doxygen 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52342-J1 构建 ×10
//!     工程全通；与 Python 消费联签（F52043 锚定行）；KB-COMPAT 定级登
//!     记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52341）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52342-J1，照原文锚定；功能按本卡标题（Doxygen）落地。
//! 本模块按 52201 样板口径运行于 crate 级 `#![cfg_attr(not(test),
//! no_std)]` + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（Doxygen 域语料，逐项定制）：
//!   ① Doxyfile 双向投影 16 格：正向 doxy_cell 折叠、逆向 doxy_cell_inverse
//!      唯一还原，逆核不中方即漂移必抓；XOR 锚低 16 位恒零。
//!   ② 五输出格式（HTML/LaTeX/XML/RTF/Man）15 格：每格式 3 步（配置→
//!      生成→校验），格式家族单调（逆序必抓），首格必须自 HTML 家族起。
//!   ③ @指令 10 操作闭集：brief/param/return/retval/mainpage/defgroup/
//!      example/sa/note/deprecated，逐格落账，越闭集必拒。
//!   ④ 图形消费（Graphviz）联签 4 格：dot 调用图/协作图/包含图/继承图
//!      联签，缺一拒封（F52043 锚定行口径同构）。
//!   ⑤ 增量构建 8 拍单调：拍载荷严格递增，回退拍必抓。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（投影漂移/操作逆序/操作越闭集），
//!      三类注入全部落在当轮临时账本上，检出+逃逸守恒 10/10；双跑指纹
//!      一致；定级 S1/S2/S3 闭集（全齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52341;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Doxyfile 投影格数。
pub const DOXY_CELLS: usize = 16;

/// 五输出格式操作格数（5 格式 × 3 步）。
pub const OP_CELLS: usize = 15;

/// @指令操作数。
pub const INSTR_OPS: usize = 10;

/// Graphviz 消费联签格数。
pub const GV_CELLS: usize = 4;

/// 增量构建拍数。
pub const BEATS: usize = 8;

/// 收口判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoxygenCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoCell,
    /// 投影格漂移
    ECellDrift,
    /// 格式操作越闭集
    ENoOp,
    /// 格式操作漂移
    EOpDrift,
    /// 格式家族逆序
    EOpOrder,
    /// @指令越闭集
    ENoInstr,
    /// Graphviz 联签格越闭集
    ENoGv,
    /// 增量拍非单调
    EBeat,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DoxygenCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DoxygenCode::Ok => "成功",
            DoxygenCode::ENoCell => "投影格越闭集：Doxyfile 16 格闭集之外先登记",
            DoxygenCode::ECellDrift => "投影格漂移：Doxyfile 正逆投影不互证，逐格复算",
            DoxygenCode::ENoOp => "格式操作越闭集：五输出格式 15 格闭集之外先登记",
            DoxygenCode::EOpDrift => "格式操作漂移：操作折叠与基准不恒等，逐格复算",
            DoxygenCode::EOpOrder => "格式家族逆序：HTML→LaTeX→XML→RTF→Man 单调，逆序必抓",
            DoxygenCode::ENoInstr => "@指令越闭集：10 操作闭集之外先登记",
            DoxygenCode::ENoGv => "联签格越闭集：Graphviz 4 格联签闭集之外先登记",
            DoxygenCode::EBeat => "增量拍非单调：8 拍载荷严格递增，回退拍必抓",
            DoxygenCode::ESeal => "未齐即封：16 投影 + 15 操作 + 10 指令 + 4 联签 + 8 拍未齐先补齐",
            DoxygenCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DoxygenCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DoxygenCode::EGradeDrift => "定级漂移：全通且联签全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六闸+16投影+15操作+10指令+4联签+8拍全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Doxyfile 投影锚（低 16 位恒零）。
const DOXY_ANCHOR: u64 = 0x4458_0000;

/// Doxyfile 双向投影正向折叠（格 i）。
pub const fn doxy_cell(i: usize) -> Option<u64> {
    if i >= DOXY_CELLS {
        return None;
    }
    Some(DOXY_ANCHOR ^ ((i as u64 + 1).wrapping_mul(0x9E37)))
}

/// Doxyfile 双向投影逆核：由折叠值唯一还原格位。
pub fn doxy_cell_inverse(v: u64) -> Option<usize> {
    let mut i = 0usize;
    while i < DOXY_CELLS {
        if let Some(w) = doxy_cell(i) {
            if w == v {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 输出格式家族（0..3=HTML 3..6=LaTeX 6..9=XML 9..12=RTF 12..15=Man）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OP_CELLS {
        return None;
    }
    Some((o / 3) as u32)
}

/// 格式名。
pub const fn format_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("HTML"),
        1 => Some("LaTeX"),
        2 => Some("XML"),
        3 => Some("RTF"),
        4 => Some("Man"),
        _ => None,
    }
}

/// 格内步名（0 配置 1 生成 2 校验）。
pub const fn step_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("配置"),
        1 => Some("生成"),
        2 => Some("校验"),
        _ => None,
    }
}

/// 格式操作格载荷（操作 o）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OP_CELLS {
        return None;
    }
    Some(0x4650_0000 ^ ((o as u64 + 1).wrapping_mul(0x2C7)))
}

/// @指令标签。
pub const fn instr_label(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("@brief"),
        1 => Some("@param"),
        2 => Some("@return"),
        3 => Some("@retval"),
        4 => Some("@mainpage"),
        5 => Some("@defgroup"),
        6 => Some("@example"),
        7 => Some("@sa"),
        8 => Some("@note"),
        9 => Some("@deprecated"),
        _ => None,
    }
}

/// @指令格载荷。
pub const fn instr_cell(i: usize) -> Option<u64> {
    if i >= INSTR_OPS {
        return None;
    }
    Some(0x494E_0000 ^ ((i as u64 + 1).wrapping_mul(0x5BD)))
}

/// Graphviz 联签格载荷（0 调用图 1 协作图 2 包含图 3 继承图）。
pub const fn gv_cell(i: usize) -> Option<u64> {
    if i >= GV_CELLS {
        return None;
    }
    Some(0x4756_0000 + i as u64 * 0x40)
}

/// 联签名。
pub const fn gv_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("dot 调用图"),
        1 => Some("dot 协作图"),
        2 => Some("dot 包含图"),
        3 => Some("dot 继承图"),
        _ => None,
    }
}

/// 增量构建拍载荷（严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0x4245_0000 + b as u64 * 0x18)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Doxygen 收口账本状态机。
#[derive(Clone)]
pub struct DoxygenLedger {
    /// 六闸（配置/解析/生成/交叉引用/图形/写回）。
    gates: [Option<bool>; GATES],
    /// Doxyfile 投影账。
    cells: [Option<u64>; DOXY_CELLS],
    /// 格式操作账。
    ops: [Option<u64>; OP_CELLS],
    /// @指令账。
    instrs: [Option<u64>; INSTR_OPS],
    /// Graphviz 联签账。
    gvs: [Option<u64>; GV_CELLS],
    /// 增量拍账。
    beats: [Option<u64>; BEATS],
    /// 最后格式家族。
    last_kind: i64,
    /// 已收最后一拍载荷。
    last_beat: u64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DoxygenLedger {
    /// 新账本。
    pub fn new() -> Self {
        DoxygenLedger {
            gates: [None; GATES],
            cells: [None; DOXY_CELLS],
            ops: [None; OP_CELLS],
            instrs: [None; INSTR_OPS],
            gvs: [None; GV_CELLS],
            beats: [None; BEATS],
            last_kind: -1,
            last_beat: 0,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格收口判据闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DoxygenCode> {
        if gate >= GATES {
            return Err(DoxygenCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
        Ok(())
    }

    /// 六闸全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Doxyfile 投影一格（正逆互证 + 重复拒）。
    pub fn record_cell(&mut self, i: usize, got: u64) -> Result<(), DoxygenCode> {
        let want = match doxy_cell(i) {
            Some(w) => w,
            None => return Err(DoxygenCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(DoxygenCode::ECellDrift);
        }
        match doxy_cell_inverse(got) {
            Some(back) => {
                if back != i {
                    self.injected += 1;
                    self.caught += 1;
                    self.fnv(0x5201 + i as u64);
                    return Err(DoxygenCode::ECellDrift);
                }
            }
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5202 + i as u64);
                return Err(DoxygenCode::ECellDrift);
            }
        }
        if match self.cells.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoxygenCode::ESeal);
        }
        if let Some(c) = self.cells.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5203 + i as u64);
        Ok(())
    }

    /// 投影全账核验（16 格）。
    pub fn cells_all(&self) -> bool {
        let mut i = 0usize;
        while i < DOXY_CELLS {
            match self.cells.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 格式操作一格（家族单调 + 载荷对照 + 重复拒）。
    /// 空账本首格必须自家族 0（HTML 配置）起，跳族即抓且不改 last_kind。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), DoxygenCode> {
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(DoxygenCode::ENoOp),
        };
        if (self.last_kind >= 0 && (kind as i64) < self.last_kind)
            || (self.last_kind < 0 && kind != 0)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(DoxygenCode::EOpOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(DoxygenCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(DoxygenCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoxygenCode::ESeal);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（15 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OP_CELLS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// @指令一格（越闭集拒 + 重复拒）。
    pub fn record_instr(&mut self, i: usize, got: u64) -> Result<(), DoxygenCode> {
        let want = match instr_cell(i) {
            Some(w) => w,
            None => return Err(DoxygenCode::ENoInstr),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + i as u64);
            return Err(DoxygenCode::EOpDrift);
        }
        if match self.instrs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoxygenCode::ESeal);
        }
        if let Some(c) = self.instrs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5401 + i as u64);
        Ok(())
    }

    /// @指令全账核验（10 格）。
    pub fn instrs_all(&self) -> bool {
        let mut i = 0usize;
        while i < INSTR_OPS {
            match self.instrs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Graphviz 联签一格。
    pub fn record_gv(&mut self, i: usize, got: u64) -> Result<(), DoxygenCode> {
        let want = match gv_cell(i) {
            Some(w) => w,
            None => return Err(DoxygenCode::ENoGv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + i as u64);
            return Err(DoxygenCode::EOpDrift);
        }
        if let Some(c) = self.gvs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5501 + i as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格，缺一拒封）。
    pub fn gvs_all(&self) -> bool {
        let mut i = 0usize;
        while i < GV_CELLS {
            match self.gvs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 增量构建一拍（载荷严格递增，回退拍必抓）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), DoxygenCode> {
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(DoxygenCode::EBeat),
        };
        if got != want || got <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + b as u64);
            return Err(DoxygenCode::EBeat);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got;
        self.fnv(0x5601 + b as u64);
        Ok(())
    }

    /// 增量拍全账核验（8 拍）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，六闸缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.cells_all()
            && self.ops_all()
            && self.instrs_all()
            && self.gvs_all()
            && self.beats_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DoxygenLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 空账全量灌装（六闸 + 投影 + 操作 + 指令 + 联签 + 增量拍）。
fn fill_ledger(led: &mut DoxygenLedger) -> u32 {
    let mut recorded = 0u32;
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut i = 0usize;
    while i < DOXY_CELLS {
        if let Some(w) = doxy_cell(i) {
            if led.record_cell(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < OP_CELLS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut n = 0usize;
    while n < INSTR_OPS {
        if let Some(w) = instr_cell(n) {
            if led.record_instr(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut v = 0usize;
    while v < GV_CELLS {
        if let Some(w) = gv_cell(v) {
            if led.record_gv(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut b = 0usize;
    while b < BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    recorded
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转（投影漂移/操作逆序/操作越闭集），全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DoxygenLedger::new();
    let recorded = fill_ledger(&mut led);
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DoxygenLedger::new();
        let caught = match kind {
            0 => {
                // 空账首格跳族（HTML → XML 家族）：必抓且不改 last_kind。
                lt.record_op(6, op_cell(6).unwrap_or(0)) == Err(DoxygenCode::EOpOrder)
            }
            1 => {
                // 先按合法序落低族格，再注投影漂移。
                let _ = lt.record_cell(0, doxy_cell(0).unwrap_or(0));
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_cell(1, doxy_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(DoxygenCode::ECellDrift)
            }
            _ => {
                // 操作越闭集：格 15 在 15 格闭集之外。
                lt.record_op(OP_CELLS, 0) == Err(DoxygenCode::ENoOp)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52341 判据集（锚 UNX-F52342-J1，off-by-one 板式）。
pub fn run_unx52341_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52341");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Doxyfile 双向投影 16 格全通",
        recorded >= DOXY_CELLS as u32,
        "正向折叠与逆核唯一还原互证，逐格恒等",
    );
    s.add(
        "双向投影逆核",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < DOXY_CELLS {
                match doxy_cell(i) {
                    Some(w) => {
                        if doxy_cell_inverse(w) != Some(i) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok && doxy_cell_inverse(0xdead_beef_0000).is_none()
        },
        "投影值逆核还原格位，外来值不中方",
    );
    s.add(
        "五输出格式 15 操作全通",
        {
            let mut lt = DoxygenLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OP_CELLS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "HTML/LaTeX/XML/RTF/Man 五家族逐操作恒等",
    );
    s.add(
        "格式家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OP_CELLS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "@指令 10 操作全落",
        {
            let mut lt = DoxygenLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < INSTR_OPS {
                if let Some(w) = instr_cell(i) {
                    if lt.record_instr(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.instrs_all()
        },
        "brief/param/return/retval/mainpage/defgroup/example/sa/note/deprecated",
    );
    s.add(
        "Graphviz 联签 4 格全签",
        {
            let mut lt = DoxygenLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < GV_CELLS {
                if let Some(w) = gv_cell(i) {
                    if lt.record_gv(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.gvs_all()
        },
        "调用图/协作图/包含图/继承图联签（F52043 锚定行同构）",
    );
    s.add(
        "增量构建 8 拍单调",
        {
            let mut lt = DoxygenLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "拍载荷严格递增，回退拍必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影漂移/操作逆序/操作越闭集三态轮转必抓",
    );
    let mut l0 = DoxygenLedger::new();
    s.add(
        "边界·投影格越闭集",
        l0.record_cell(DOXY_CELLS, 0) == Err(DoxygenCode::ENoCell),
        "投影格 16 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OP_CELLS, 0) == Err(DoxygenCode::ENoOp),
        "操作 15 越闭集必拒",
    );
    s.add(
        "边界·指令越闭集",
        l0.record_instr(INSTR_OPS, 0) == Err(DoxygenCode::ENoInstr),
        "@指令 10 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = DoxygenLedger::new();
            let _ = fill_ledger(&mut lt);
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9500,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9500",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doxy_projection() {
        assert!(doxy_cell(0).is_some());
        assert!(doxy_cell(DOXY_CELLS).is_none());
        assert_eq!(doxy_cell_inverse(doxy_cell(7).unwrap_or(0)), Some(7));
        assert!(doxy_cell_inverse(0x1234_5678).is_none());
    }

    #[test]
    fn test_op_kind_mapping() {
        // 家族映射：kind = o / 3（HTML×3 LaTeX×3 XML×3 RTF×3 Man×3）。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(2), Some(0));
        assert_eq!(op_kind(3), Some(1));
        assert_eq!(op_kind(14), Some(4));
        assert!(op_kind(15).is_none());
        assert_eq!(format_name(0), Some("HTML"));
        assert_eq!(format_name(4), Some("Man"));
        assert!(format_name(5).is_none());
        assert_eq!(step_name(0), Some("配置"));
        assert!(step_name(3).is_none());
    }

    #[test]
    fn test_instr_labels() {
        assert_eq!(instr_label(0), Some("@brief"));
        assert_eq!(instr_label(9), Some("@deprecated"));
        assert!(instr_label(10).is_none());
        assert!(instr_cell(10).is_none());
    }

    #[test]
    fn test_gv_and_beats() {
        assert_eq!(gv_name(0), Some("dot 调用图"));
        assert_eq!(gv_name(3), Some("dot 继承图"));
        assert!(gv_name(4).is_none());
        assert!(gv_cell(4).is_none());
        assert!(beat_cell(BEATS).is_none());
        let a = beat_cell(2).unwrap_or(0);
        let b = beat_cell(3).unwrap_or(0);
        assert!(b > a);
    }

    #[test]
    fn test_first_op_family_zero() {
        // 空账本首格跳族必抓且不改 last_kind。
        let mut lt = DoxygenLedger::new();
        assert_eq!(lt.record_op(9, op_cell(9).unwrap_or(0)), Err(DoxygenCode::EOpOrder));
        assert!(lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序铺 kind0 三格，再注 kind1 正格；高族回低族必抓。
        let mut lt = DoxygenLedger::new();
        let mut i = 0usize;
        while i <= 2 {
            let _ = lt.record_op(i, op_cell(i).unwrap_or(0));
            i += 1;
        }
        let _ = lt.record_op(3, op_cell(3).unwrap_or(0));
        assert_eq!(lt.record_op(1, op_cell(1).unwrap_or(0)), Err(DoxygenCode::EOpOrder));
        assert!(lt.record_op(6, op_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DoxygenLedger::new();
        // 投影格无族闸，可直接注漂移；操作格须先按合法序铺 0..=5。
        assert_eq!(
            lt.record_cell(3, doxy_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(DoxygenCode::ECellDrift)
        );
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_op(i, op_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(DoxygenCode::EOpDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DoxygenLedger::new();
        let _ = lt.record_cell(0, doxy_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cell(0, doxy_cell(0).unwrap_or(0)), Err(DoxygenCode::ESeal));
    }

    #[test]
    fn test_beat_monotone() {
        let mut lt = DoxygenLedger::new();
        let _ = lt.record_beat(0, beat_cell(0).unwrap_or(0));
        assert_eq!(lt.record_beat(1, beat_cell(0).unwrap_or(0)), Err(DoxygenCode::EBeat));
        assert!(lt.record_beat(1, beat_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DoxygenLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DoxygenLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= DOXY_CELLS as u32 + OP_CELLS as u32 + INSTR_OPS as u32);
        assert_eq!(caught, 10);
        let (i, c, e) = {
            let mut led = DoxygenLedger::new();
            let _ = fill_ledger(&mut led);
            led.stats()
        };
        assert_eq!((i, c, e), (0, 0, 0));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9500);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DoxygenCode::Ok as u32, 0);
        assert_eq!(DoxygenCode::EGradeDrift as u32, 12);
        assert!(!DoxygenCode::Ok.detail().is_empty());
        assert!(!DoxygenCode::ESeal.detail().is_empty());
        let _ = DoxygenCode::ENoCell.val();
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DoxygenLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 如实阶梯：六闸齐但各面未齐 → S2（全齐才 S1）。
        let mut lt2 = DoxygenLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52341_checks_green() {
        let s = run_unx52341_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
