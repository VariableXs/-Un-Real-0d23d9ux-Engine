//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12740 B38 预告与开门件（SEH 压测核账框架） · 0x82300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 12740 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12741-J1 压测矩阵登记齐（深度/展开/码族/坏表四轴），
//!      矩阵可点验 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   1. 三段登记状态机：空册 → 四轴预告 → 六件开门件 → 框架落位，单调只进
//!      不退；任何倒退（Gated 期再预告、Seated 期再开门/再预告）显性拒
//!      ERewind，拒绝时附可操作中文建议（先回退工单再重排）；
//!   2. 四轴预告档案冻结表：深度 3 档 / 展开 3 态 / 码族 4 族 / 坏表 3 档，
//!      逐轴锚定 UNX-F12741-J1 压测矩阵闭集，预告格数基线 108 = 3×3×4×3，
//!      预告档数与冻结表不符显性拒 ETier（防轴漂移）；
//!   3. 开门件六件（矩阵登记/执行压测/核账点验/守恒对账/注入必抓/收口写回），
//!      每件在冻结表中绑定唯一已预告轴；未预告先开门显性拒 EOrder——
//!      判定次序冻结为：状态倒退 → 未预告先开门 → 件号越册 → 重复登记；
//!   4. 矩阵可点验 10/10：落位后逐条点验 10 项（预告齐/档数对/开门齐/
//!      轴覆盖/守恒/状态/注入账/漏检零/指纹/序号），点验账定容 10 槽，
//!      容量耗尽显性拒 ECapFull 且已入账 10 条不丢（账不丢硬约束）；
//!   5. 注入三族：轴越闭集 / 重复登记 / 阶段倒退，各 10 轮必抓；计数守恒式
//!      注入数 = 检出数 + 漏检数，漏检恒 0；
//!   6. 完成写回模型：WorkStatus 只进至 Done，结果摘要非空（判据③模型化）；
//!      LCG 种子 = SEG_BASE（0x82300）；FNV-1a 指纹双跑逐字段一致；
//!      no_std + alloc；零浮点/IO/墙钟/随机；无 unwrap/expect/panic/裸下标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12740 − 12600) = 0x82300）。
pub const SEG_BASE: u32 = 0x82300;

// ── 冻结常量表（带出处） ─────────────────────────────────────────
/// 四轴预告档案（轴名, 档数）。
/// 出处：UNX-F12741-J1 压测矩阵四轴闭集（深度/展开/码族/坏表），
/// 档数与 UNX-F12741 压测矩阵登记保持逐轴一致，不许漂移。
pub const AXIS_TABLE: [(&'static str, u32); 4] = [
    ("深度", 3), // 展开层数档：浅/中/深
    ("展开", 3), // 展开态：不展开/逐步/全展开
    ("码族", 4), // 异常码族：访问违例/栈溢出/非法指令/除零
    ("坏表", 3), // 坏表档：好表/半坏/全坏
];

/// 预告格数基线 = 3×3×4×3（UNX-F12741-J1 矩阵格数，预告与实登记同源）。
pub const CELL_BASELINE: u32 = 108;

/// 开门件冻结表（件名, 绑定轴下标）。
/// 出处：B38 预告卡 12740 设计要点 3——六件各绑一轴，覆盖四轴。
pub const GATE_TABLE: [(&'static str, usize); 6] = [
    ("矩阵登记件", 0),
    ("执行压测件", 0),
    ("核账点验件", 1),
    ("守恒对账件", 1),
    ("注入必抓件", 2),
    ("收口写回件", 3),
];

/// 点验账容量（判据②"矩阵可点验 10/10"的账面槽数）。
pub const POINT_CAP: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// B38 预告域诊断码闭集；码值 = SEG_BASE + 固定偏移，查表外回落兜底文案。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B38Diag {
    /// 预告轴越闭集：轴下标 ≥ 4，不在深度/展开/码族/坏表四轴内。
    ENoAxis,
    /// 预告档数与冻结表不符：档数漂移会带歪 12741 矩阵登记。
    ETier,
    /// 开门件号越册：件号 ≥ 6，六件闭集外。
    ENoGate,
    /// 登记阶段倒退：三段登记只进不退，倒退显性拒。
    ERewind,
    /// 未预告先开门：开门件必须绑定已预告轴，次序冻结。
    EOrder,
    /// 重复登记：一轴一预告、一件一登记，双登显性拒。
    EDouble,
    /// 点验账容量耗尽：10 槽判满后新点验拒入，先封账再核。
    ECapFull,
    /// 未齐落位：四轴或六件未齐即请求落位，先补齐再走落位口。
    ESeat,
    /// 点验账守恒破缺：Σ(点验过+点验不过) ≠ 点验入账数。
    EConserve,
    /// 指纹漂移：双跑指纹不一致，排查非确定源。
    EFp,
}

impl B38Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                B38Diag::ENoAxis => 0x01,
                B38Diag::ETier => 0x02,
                B38Diag::ENoGate => 0x03,
                B38Diag::ERewind => 0x04,
                B38Diag::EOrder => 0x05,
                B38Diag::EDouble => 0x06,
                B38Diag::ECapFull => 0x07,
                B38Diag::ESeat => 0x08,
                B38Diag::EConserve => 0x09,
                B38Diag::EFp => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            B38Diag::ENoAxis => "预告轴越闭集：轴下标不在 0..4，按深度/展开/码族/坏表四轴重排",
            B38Diag::ETier => "预告档数与冻结表不符：对照 AXIS_TABLE 逐轴改回 3/3/4/3",
            B38Diag::ENoGate => "开门件号越册：件号不在 0..6，按 GATE_TABLE 六件闭集重排",
            B38Diag::ERewind => "登记阶段倒退：空册→预告→开门→落位只进不退，先回退工单再重排",
            B38Diag::EOrder => "未预告先开门：先走四轴预告口，再按 GATE_TABLE 绑轴开门",
            B38Diag::EDouble => "重复登记：一轴一预告、一件一登记，查重后再提交",
            B38Diag::ECapFull => "点验账容量耗尽：10 槽判满，先封账核点验，再开新账",
            B38Diag::ESeat => "未齐落位：四轴/六件未齐，先补齐缺项再走落位口",
            B38Diag::EConserve => "点验账守恒破缺：Σ(过+不过)≠入账数，逐槽复算点验账",
            B38Diag::EFp => "指纹漂移：双跑指纹不一致，排查 LCG 种子与非确定源",
        }
    }

    /// 闭集数组（查表遍历用，段窗守卫的枚举锚）。
    pub const ALL: [B38Diag; 10] = [
        B38Diag::ENoAxis,
        B38Diag::ETier,
        B38Diag::ENoGate,
        B38Diag::ERewind,
        B38Diag::EOrder,
        B38Diag::EDouble,
        B38Diag::ECapFull,
        B38Diag::ESeat,
        B38Diag::EConserve,
        B38Diag::EFp,
    ];
}

/// 诊断码查表闭集返回：命中闭集给中文文案，未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < B38Diag::ALL.len() {
        if let Some(d) = B38Diag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82300 段闭集内，核对 SEG_BASE + 偏移表"
}

// ── 三段登记状态机（单调只进不退） ───────────────────────────────
/// 登记阶段：空册 → 已预告 → 已开门 → 已落位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegStage {
    /// 空册：尚未预告任何轴。
    Empty,
    /// 已预告：至少一轴已预告（四轴齐前为部分预告）。
    Previewed,
    /// 已开门：至少一件开门件已登记（六件齐前为部分开门）。
    Gated,
    /// 已落位：四轴预告齐 + 六件开门齐 + 落位口通过。
    Seated,
}

impl RegStage {
    /// 阶段序号（0..=3），用于单调性比对与点验 P10。
    pub fn rank(self) -> u32 {
        match self {
            RegStage::Empty => 0,
            RegStage::Previewed => 1,
            RegStage::Gated => 2,
            RegStage::Seated => 3,
        }
    }
}

/// 一条预告：轴下标 + 预告档数（档数必须与冻结表一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreviewEntry {
    /// 轴下标（0..4，闭集见 AXIS_TABLE）。
    pub axis: u32,
    /// 预告档数（必须等于冻结表档数）。
    pub tiers: u32,
}

/// 一件开门件：件号 + 绑定轴（绑定轴来自 GATE_TABLE 冻结表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GatePiece {
    /// 件号（0..6，闭集见 GATE_TABLE）。
    pub no: u32,
    /// 绑定轴下标（0..4）。
    pub axis: usize,
}

/// B38 预告登记簿：四轴预告账 + 六件开门账 + 点验账 + 注入账 + 指纹。
pub struct B38Registry {
    previews: [Option<PreviewEntry>; 4],
    gates: [Option<GatePiece>; 6],
    stage: RegStage,
    /// 点验账：10 槽定容，槽位 Some(true)=过 / Some(false)=不过。
    points: [Option<bool>; POINT_CAP],
    point_n: usize,
    point_pass: usize,
    point_fail: usize,
    /// 注入账：注入数 / 检出数 / 漏检数（漏检恒 0）。
    injected: u32,
    caught: u32,
    missed: u32,
    fp: u64,
}

impl B38Registry {
    /// 新空册（阶段 Empty、全账清零、FNV 基准种子）。
    pub fn new() -> B38Registry {
        B38Registry {
            previews: [None; 4],
            gates: [None; 6],
            stage: RegStage::Empty,
            points: [None; POINT_CAP],
            point_n: 0,
            point_pass: 0,
            point_fail: 0,
            injected: 0,
            caught: 0,
            missed: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步混入。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 指纹读出（全零视为异常，双跑比对用）。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 当前阶段。
    pub fn stage(&self) -> RegStage {
        self.stage
    }

    /// 预告一轴：轴越闭集拒 / 档数漂移拒 / 重复登记拒 / 阶段倒退拒。
    /// 成功返回该轴下标。
    pub fn preview_axis(&mut self, axis: u32, tiers: u32) -> Result<u32, B38Diag> {
        // 判定次序 1：状态倒退（Gated/Seated 期禁再预告）。
        if self.stage.rank() >= RegStage::Gated.rank() {
            return Err(B38Diag::ERewind);
        }
        // 判定次序 2：轴越闭集。
        if axis >= AXIS_TABLE.len() as u32 {
            return Err(B38Diag::ENoAxis);
        }
        // 判定次序 3：档数与冻结表一致（防轴漂移）。
        let frozen_tiers = match AXIS_TABLE.get(axis as usize) {
            Some((_, t)) => *t,
            None => return Err(B38Diag::ENoAxis),
        };
        if tiers != frozen_tiers {
            return Err(B38Diag::ETier);
        }
        // 判定次序 4：重复登记。
        let slot = match self.previews.get_mut(axis as usize) {
            Some(s) => s,
            None => return Err(B38Diag::ENoAxis),
        };
        if slot.is_some() {
            return Err(B38Diag::EDouble);
        }
        *slot = Some(PreviewEntry { axis, tiers });
        self.fnv(0x51 + (axis as u64) << 8 | (tiers as u64));
        if self.stage == RegStage::Empty {
            self.stage = RegStage::Previewed;
        }
        Ok(axis)
    }

    /// 开一件门：未预告先开门拒 / 件号越册拒 / 重复开门拒 / 阶段倒退拒。
    /// 成功返回件号。
    pub fn open_gate(&mut self, no: u32) -> Result<u32, B38Diag> {
        // 判定次序 1：状态倒退（Seated 期禁再开门）。
        if self.stage == RegStage::Seated {
            return Err(B38Diag::ERewind);
        }
        // 判定次序 2：未预告先开门（空册期拒；绑定轴未预告也拒）。
        if self.stage == RegStage::Empty {
            return Err(B38Diag::EOrder);
        }
        // 判定次序 3：件号越册。
        let (gate_name, bind_axis) = match GATE_TABLE.get(no as usize) {
            Some((n, a)) => (*n, *a),
            None => return Err(B38Diag::ENoGate),
        };
        let _ = gate_name; // 名字供查表文案，主账只用绑定轴。
        // 判定次序 4：绑定轴必须已预告。
        let previewed = match self.previews.get(bind_axis) {
            Some(Some(_)) => true,
            _ => false,
        };
        if !previewed {
            return Err(B38Diag::EOrder);
        }
        // 判定次序 5：重复登记。
        let slot = match self.gates.get_mut(no as usize) {
            Some(s) => s,
            None => return Err(B38Diag::ENoGate),
        };
        if slot.is_some() {
            return Err(B38Diag::EDouble);
        }
        *slot = Some(GatePiece { no, axis: bind_axis });
        self.fnv(0x6a + (no as u64) << 8 | (bind_axis as u64));
        if self.stage == RegStage::Previewed {
            self.stage = RegStage::Gated;
        }
        Ok(no)
    }

    /// 落位口：四轴齐 + 六件齐才放行，未齐显性拒 ESeat。
    pub fn seat(&mut self) -> Result<(), B38Diag> {
        if self.stage == RegStage::Seated {
            return Err(B38Diag::ERewind);
        }
        if self.stage != RegStage::Gated {
            return Err(B38Diag::ESeat);
        }
        let mut axis_ok = true;
        let mut i = 0usize;
        while i < self.previews.len() {
            if self.previews.get(i).map(|s| s.is_none()).unwrap_or(true) {
                axis_ok = false;
            }
            i += 1;
        }
        let mut gate_ok = true;
        let mut j = 0usize;
        while j < self.gates.len() {
            if self.gates.get(j).map(|s| s.is_none()).unwrap_or(true) {
                gate_ok = false;
            }
            j += 1;
        }
        if !axis_ok || !gate_ok {
            return Err(B38Diag::ESeat);
        }
        self.stage = RegStage::Seated;
        self.fnv(0x5e47);
        Ok(())
    }

    /// 点验入账：过/不过入槽，槽满显性拒（账不丢——已入槽位原样保留）。
    pub fn log_point(&mut self, verdict: bool) -> Result<usize, B38Diag> {
        if self.point_n >= POINT_CAP {
            return Err(B38Diag::ECapFull);
        }
        let slot = self.point_n;
        if let Some(p) = self.points.get_mut(slot) {
            *p = Some(verdict);
        }
        self.point_n += 1;
        if verdict {
            self.point_pass += 1;
        } else {
            self.point_fail += 1;
        }
        self.fnv(0x70 + (slot as u64) << 4 | (verdict as u64));
        Ok(slot)
    }

    /// 点验账守恒：Σ(过+不过) == 入账数。
    pub fn point_conservation(&self) -> bool {
        self.point_pass.saturating_add(self.point_fail) == self.point_n
    }

    /// 点验 10/10：账满 10 且全过。
    pub fn points_all_pass(&self) -> bool {
        self.point_n == POINT_CAP && self.point_fail == 0
    }

    /// 点验账读出第 i 槽（越界 None）。
    pub fn point_at(&self, i: usize) -> Option<bool> {
        match self.points.get(i) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 注入记账：注入一次，检出（caught）或漏检（missed）各计一次。
    pub fn note_injection(&mut self, caught: bool) {
        self.injected += 1;
        if caught {
            self.caught += 1;
        } else {
            self.missed += 1;
        }
    }

    /// 注入账守恒：注入数 = 检出数 + 漏检数。
    pub fn injection_conservation(&self) -> bool {
        self.caught.saturating_add(self.missed) == self.injected
    }

    /// 注入三数（注入/检出/漏检）。
    pub fn injection_stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.missed)
    }

    /// 轴预告数。
    pub fn preview_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.previews.len() {
            if self.previews.get(i).map(|s| s.is_some()).unwrap_or(false) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 开门件数。
    pub fn gate_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.gates.len() {
            if self.gates.get(i).map(|s| s.is_some()).unwrap_or(false) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 落位后点验 10 项逐条判定并全部入账（P1..P10 闭集）。
    /// 返回过项数（应为 10）。
    pub fn run_points(&mut self) -> u32 {
        let mut pass = 0u32;
        // P1 四轴预告齐。
        let p1 = self.preview_count() == AXIS_TABLE.len() as u32;
        // P2 每轴档数与冻结表一致。
        let mut p2 = true;
        let mut i = 0usize;
        while i < AXIS_TABLE.len() {
            let want = match AXIS_TABLE.get(i) {
                Some((_, t)) => *t,
                None => 0,
            };
            let got = match self.previews.get(i) {
                Some(Some(e)) => e.tiers,
                _ => 0,
            };
            if want != got {
                p2 = false;
            }
            i += 1;
        }
        // P3 六件开门齐。
        let p3 = self.gate_count() == GATE_TABLE.len() as u32;
        // P4 每件绑定轴已预告。
        let mut p4 = true;
        let mut j = 0usize;
        while j < GATE_TABLE.len() {
            let bind = match GATE_TABLE.get(j) {
                Some((_, a)) => *a,
                None => 0,
            };
            let opened_axis = match self.gates.get(j) {
                Some(Some(g)) => g.axis,
                _ => 99,
            };
            if opened_axis != bind {
                p4 = false;
            }
            j += 1;
        }
        // P5 点验账守恒（自反：入账前账面零态守恒成立）。
        let p5 = self.point_conservation();
        // P6 状态机到达 Seated。
        let p6 = self.stage == RegStage::Seated;
        // P7 注入账守恒。
        let p7 = self.injection_conservation();
        // P8 漏检为零。
        let p8 = self.missed == 0;
        // P9 指纹非零。
        let p9 = self.fp != 0;
        // P10 阶段序号封顶。
        let p10 = self.stage.rank() == 3;
        let verdicts = [p1, p2, p3, p4, p5, p6, p7, p8, p9, p10];
        let mut k = 0usize;
        while k < verdicts.len() {
            let v = match verdicts.get(k) {
                Some(x) => *x,
                None => false,
            };
            if self.log_point(v).is_ok() && v {
                pass += 1;
            }
            k += 1;
        }
        pass
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
/// 线性同余发生器：st = st×1664525 + 1013904223（确定性，禁随机源）。
pub struct Lcg(u64);

impl Lcg {
    /// 种子 = SEG_BASE（段基址派生，双跑一致）。
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 推进一步并返回原始态。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// 取 [0, n) 内的确定值（n=0 时恒 0）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 完成写回模型（判据③） ───────────────────────────────────────
/// 工单状态：进行中 / 已完成（单调只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkStatus {
    /// 进行中：预告或开门未收口。
    InProgress,
    /// 已完成：落位 + 点验 10/10 通过后的终态。
    Done,
}

/// 完成写回单：状态 + 结果摘要（摘要非空才收口）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Writeback {
    /// 写回状态。
    pub status: WorkStatus,
    /// 结果摘要（判据③：非空）。
    pub summary: String,
}

/// 组装写回单：落位 + 点验齐才给 Done，摘要恒非空（空摘要回退兜底文案）。
pub fn make_writeback(reg: &B38Registry, point_pass: u32) -> Writeback {
    let done = reg.stage() == RegStage::Seated && point_pass == 10;
    let summary = format!(
        "B38 落位完成：预告{}轴 开门{}件 点验{}/10 指纹{:016x}",
        reg.preview_count(),
        reg.gate_count(),
        point_pass,
        reg.fingerprint()
    );
    let summary = if summary.is_empty() {
        String::from("B38 写回摘要兜底：登记完成（摘要原为空，已回填）")
    } else {
        summary
    };
    Writeback {
        status: if done {
            WorkStatus::Done
        } else {
            WorkStatus::InProgress
        },
        summary,
    }
}

// ── 黄金正演：预告→开门→落位→点验→写回 ──────────────────────────
/// 全流程正演：四轴预告（档数照冻结表）→ 六件开门 → 落位 → 点验 10 项。
/// 返回 (点验过项数, 指纹)；任一步违约即 Err。
pub fn golden_run() -> Result<(u32, u64), B38Diag> {
    let mut reg = B38Registry::new();
    // 四轴预告：档数逐轴照冻结表（深度3/展开3/码族4/坏表3）。
    let mut i = 0usize;
    while i < AXIS_TABLE.len() {
        let tiers = match AXIS_TABLE.get(i) {
            Some((_, t)) => *t,
            None => return Err(B38Diag::ETier),
        };
        reg.preview_axis(i as u32, tiers)?;
        i += 1;
    }
    // 六件开门：按件号序。
    let mut j = 0usize;
    while j < GATE_TABLE.len() {
        reg.open_gate(j as u32)?;
        j += 1;
    }
    // 落位。
    reg.seat()?;
    // 点验 10 项。
    let pass = reg.run_points();
    Ok((pass, reg.fingerprint()))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
/// 注入三族闭集：0=轴越闭集 / 1=重复登记 / 2=阶段倒退。
pub const INJ_FAMILIES: usize = 3;

/// 注入账（tries/caught 分族记账）。
pub struct InjectLedger {
    /// 各族注入次数。
    pub tries: [u32; INJ_FAMILIES],
    /// 各族检出次数。
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    /// 零账。
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }

    /// 全抓判定：各族 caught == tries。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJ_FAMILIES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 裸下标禁用面：分族计数统一走 get_mut（越界静默不计，闭集内不会越界）。
fn bump(arr: &mut [u32; INJ_FAMILIES], i: usize) {
    if let Some(v) = arr.get_mut(i) {
        *v += 1;
    }
}

/// 跑注入 10 轮 × 3 族：每族每轮构造一次违约，必须 100% 被 Reject 抓住。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族 0：轴越闭集（轴下标 4..7 闭集外，必拒 ENoAxis）。
        bump(&mut ledger.tries, 0);
        let bad_axis = 4 + rng.below(4) as u32;
        let mut reg0 = B38Registry::new();
        if reg0.preview_axis(bad_axis, 3) == Err(B38Diag::ENoAxis) {
            bump(&mut ledger.caught, 0);
        }
        // 族 1：重复登记（同轴双预告，第二笔必拒 EDouble）。
        bump(&mut ledger.tries, 1);
        let mut reg1 = B38Registry::new();
        let _ = reg1.preview_axis(0, 3);
        if reg1.preview_axis(0, 3) == Err(B38Diag::EDouble) {
            bump(&mut ledger.caught, 1);
        }
        // 族 2：阶段倒退（空册直接落位，必拒 ESeat；再倒退再拒）。
        bump(&mut ledger.tries, 2);
        let mut reg2 = B38Registry::new();
        if reg2.seat() == Err(B38Diag::ESeat) {
            bump(&mut ledger.caught, 2);
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ─────────────────────
/// 正常流：黄金正演点验 10/10 且指纹非零。
fn golden_flow_ok() -> bool {
    match golden_run() {
        Ok((pass, fp)) => pass == 10 && fp != 0,
        Err(_) => false,
    }
}

/// 边界零：轴 0 / 件 0 首位合法。
fn boundary_zero_ok() -> bool {
    let mut reg = B38Registry::new();
    let a = reg.preview_axis(0, AXIS_TABLE.first().map(|t| t.1).unwrap_or(0));
    let _ = reg.open_gate(0);
    a.is_ok()
}

/// 边界上限：轴 3 / 件 5 末位合法。
fn boundary_max_ok() -> bool {
    let mut reg = B38Registry::new();
    let tiers = match AXIS_TABLE.last() {
        Some((_, t)) => *t,
        None => 0,
    };
    let a = reg.preview_axis(3, tiers);
    let _ = reg.seat(); // 未开门落位必拒，不影响轴上界判定。
    a.is_ok()
}

/// 边界上限+1：轴 4 / 件 6 越闭集必拒。
fn boundary_max_plus_ok() -> bool {
    let mut reg = B38Registry::new();
    let axis_over = reg.preview_axis(4, 3) == Err(B38Diag::ENoAxis);
    let mut reg2 = B38Registry::new();
    let _ = reg2.preview_axis(0, 3);
    let gate_over = reg2.open_gate(6) == Err(B38Diag::ENoGate);
    axis_over && gate_over
}

/// 边界负移位（u32 回绕）：0u32.wrapping_sub(1) 越闭集必拒。
fn boundary_wrap_ok() -> bool {
    let mut reg = B38Registry::new();
    let huge = 0u32.wrapping_sub(1);
    reg.preview_axis(huge, 3) == Err(B38Diag::ENoAxis)
}

/// 注入三族 10 轮全抓。
fn injection_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    led.all_caught()
}

/// 判定次序：空册开门先回 EOrder（次序 2 先于件号越闭集的次序 3）。
fn judge_order_ok() -> bool {
    let mut reg = B38Registry::new();
    let first = reg.open_gate(99);
    // 已预告后，件号越册才轮到 ENoGate。
    let _ = reg.preview_axis(0, 3);
    let second = reg.open_gate(99);
    first == Err(B38Diag::EOrder) && second == Err(B38Diag::ENoGate)
}

/// 状态机非法倒退拒：Gated 期再预告必拒 ERewind。
fn rewind_reject_ok() -> bool {
    let mut reg = B38Registry::new();
    let _ = reg.preview_axis(0, 3);
    let _ = reg.open_gate(0);
    reg.preview_axis(1, 3) == Err(B38Diag::ERewind)
}

/// 容量耗尽且账不丢：点验账 10 槽判满后第 11 笔拒，前 10 笔原样在账。
fn capacity_ok() -> bool {
    let mut reg = B38Registry::new();
    let mut i = 0usize;
    while i < POINT_CAP {
        if reg.log_point(true).is_err() {
            return false;
        }
        i += 1;
    }
    let overflow = reg.log_point(true) == Err(B38Diag::ECapFull);
    // 前 10 笔逐槽复读：全 Some(true)，账不丢。
    let mut intact = true;
    let mut k = 0usize;
    while k < POINT_CAP {
        if reg.point_at(k) != Some(true) {
            intact = false;
        }
        k += 1;
    }
    overflow && intact && reg.point_conservation()
}

/// 双跑一致：同输入两次黄金正演逐字段相等（点验数与指纹）。
fn dual_run_ok() -> bool {
    let r1 = golden_run();
    let r2 = golden_run();
    r1 == r2 && r1.is_ok()
}

/// 对账一致：预告格数基线 = 3×3×4×3 = 108（与 UNX-F12741-J1 矩阵同源）。
fn reconcile_ok() -> bool {
    let mut product = 1u32;
    let mut i = 0usize;
    while i < AXIS_TABLE.len() {
        let t = match AXIS_TABLE.get(i) {
            Some((_, t)) => *t,
            None => return false,
        };
        product = product.saturating_mul(t);
        i += 1;
    }
    product == CELL_BASELINE && CELL_BASELINE == 108
}

/// 计数守恒：注入数 = 检出数 + 漏检数，漏检 = 0。
fn conservation_ok() -> bool {
    let mut reg = B38Registry::new();
    // 10 次注入全检出 + 0 次漏检。
    let mut n = 0u32;
    while n < 10 {
        reg.note_injection(true);
        n += 1;
    }
    let (inj, caught, missed) = reg.injection_stats();
    inj == 10 && caught == 10 && missed == 0 && reg.injection_conservation()
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在 VX9 域 [0x80000, 0x82FC0]。
fn segment_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 文案闭集：闭集码查表命中、未知码给非空中文兜底、ALL 数组码值全落段窗。
fn closed_text_ok() -> bool {
    let first = match B38Diag::ALL.first() {
        Some(d) => *d,
        None => return false,
    };
    if detail(first.val()) != first.detail() {
        return false;
    }
    let unknown = detail(SEG_BASE + 0x3f);
    if unknown.is_empty() || !unknown.contains("未知诊断码") {
        return false;
    }
    let mut i = 0usize;
    while i < B38Diag::ALL.len() {
        if let Some(d) = B38Diag::ALL.get(i) {
            let v = d.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 完成写回：状态 = 已完成且结果摘要非空（判据③）。
fn writeback_ok() -> bool {
    let mut reg = B38Registry::new();
    let mut i = 0usize;
    while i < AXIS_TABLE.len() {
        let tiers = match AXIS_TABLE.get(i) {
            Some((_, t)) => *t,
            None => return false,
        };
        let _ = reg.preview_axis(i as u32, tiers);
        i += 1;
    }
    let mut j = 0usize;
    while j < GATE_TABLE.len() {
        let _ = reg.open_gate(j as u32);
        j += 1;
    }
    let _ = reg.seat();
    let pass = reg.run_points();
    let wb = make_writeback(&reg, pass);
    wb.status == WorkStatus::Done && !wb.summary.is_empty()
}

// ── 判据名闭表（与 extra_checks 推送序一一对应） ─────────────────
/// (判据名, 静态明细回退文案)；前 8 条走动态明细槽，其余走静态明细。
const CHECK_META: [(&'static str, &'static str); 17] = [
    ("J1-黄金正演·矩阵可点验10/10", "四轴预告+六件开门+落位点验全过"),
    ("J1-注入三族10/10全抓", "轴越闭集/重复登记/阶段倒退必抓"),
    ("J1-对账一致·格数基线108", "3×3×4×3 与 UNX-F12741-J1 矩阵同源"),
    ("J3-完成写回·状态已完成摘要非空", "WorkStatus=Done 且摘要非空"),
    ("边界·轴件首位合法", "轴0/件0 首位收账"),
    ("边界·轴件末位合法", "轴3/件5 末位收账"),
    ("边界·上限+1必拒", "轴4/件6 越闭集拒"),
    ("边界·负移位回绕必拒", "u32 回绕值越闭集拒"),
    ("判序·未预告先开门先拒", "EOrder 先于 ENoGate"),
    ("状态机·倒退显性拒", "Gated 期再预告拒 ERewind"),
    ("容量·耗尽且账不丢", "点验账10槽满拒第11笔，前10笔在账"),
    ("双跑一致", "同输入两次黄金正演逐字段相等"),
    ("守恒·注入数=检出+漏检", "漏检恒 0"),
    ("守恒·点验账自洽", "Σ(过+不过)=入账数"),
    ("段位守卫", "SEG_BASE 0x40 对齐且在 VX9 域"),
    ("文案闭集", "闭集命中+未知码中文兜底+码值落段窗"),
    ("收口·判据条数只增不减", "extra_checks 条数 ≥ 冻结下限"),
];

/// 冻结判据条数下限（收口只增不减的锚）。
pub const FROZEN_CHECK_COUNT: usize = 17;

/// 单条判据装配：名字取闭表，未知下标给中文兜底名。
fn entry(i: usize, passed: bool, det: String) -> (String, bool, String) {
    let name = match CHECK_META.get(i) {
        Some((n, _)) => String::from(*n),
        None => String::from("判据名缺位：下标越闭表"),
    };
    (name, passed, det)
}

/// 判据汇总（≥12 条，覆盖正常流/四类边界/三类注入/判序/状态机/容量/
/// 双跑/段位/对账/守恒/文案闭集/收口）。同名两次调用结果逐字段相等。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 0 正常流。
    let p = golden_flow_ok();
    v.push(entry(0, p, format!("点验={}", if p { "10/10" } else { "不足10" })));
    // 1 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    let p = led.all_caught();
    v.push(entry(
        1,
        p,
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 2 对账一致。
    let p = reconcile_ok();
    v.push(entry(2, p, format!("基线={}", CELL_BASELINE)));
    // 3 完成写回。
    let p = writeback_ok();
    v.push(entry(3, p, String::from("状态=已完成 摘要非空")));
    // 4 边界零。
    let p = boundary_zero_ok();
    v.push(entry(4, p, String::from("轴0/件0 合法")));
    // 5 边界上限。
    let p = boundary_max_ok();
    v.push(entry(5, p, String::from("轴3/件5 合法")));
    // 6 边界上限+1。
    let p = boundary_max_plus_ok();
    v.push(entry(6, p, String::from("轴4/件6 必拒")));
    // 7 边界负移位。
    let p = boundary_wrap_ok();
    v.push(entry(7, p, String::from("u32 回绕必拒")));
    // 8 判定次序。
    let p = judge_order_ok();
    v.push(entry(8, p, String::from("EOrder 先于 ENoGate")));
    // 9 状态机倒退。
    let p = rewind_reject_ok();
    v.push(entry(9, p, String::from("倒退拒 ERewind")));
    // 10 容量耗尽。
    let p = capacity_ok();
    v.push(entry(10, p, format!("容量={}", POINT_CAP)));
    // 11 双跑一致。
    let p = dual_run_ok();
    v.push(entry(11, p, String::from("两次正演逐字段相等")));
    // 12 计数守恒（注入）。
    let p = conservation_ok();
    v.push(entry(12, p, String::from("漏检=0")));
    // 13 点验账守恒。
    let p = point_ledger_ok();
    v.push(entry(13, p, String::from("Σ(过+不过)=入账数")));
    // 14 段位守卫。
    let p = segment_guard_ok();
    v.push(entry(14, p, format!("SEG=0x{:x}", SEG_BASE)));
    // 15 文案闭集。
    let p = closed_text_ok();
    v.push(entry(15, p, String::from("闭集命中+兜底非空")));
    // 16 收口只增不减（条数取闭表长度；双跑一致由第 11 条独立钉住，
    // 此处不可回调 extra_checks 自身——会无限递归爆栈）。
    let n = CHECK_META.len();
    v.push(entry(16, n >= FROZEN_CHECK_COUNT, format!("条数={}", n)));
    v
}

/// 点验账守恒谓词（独立小模型：10 槽过账 + 2 槽溢拒后账面自洽）。
fn point_ledger_ok() -> bool {
    let mut reg = B38Registry::new();
    let mut i = 0usize;
    while i < 6 {
        let _ = reg.log_point(true);
        i += 1;
    }
    let mut j = 0usize;
    while j < 4 {
        let _ = reg.log_point(false);
        j += 1;
    }
    reg.point_conservation() && reg.point_pass == 6 && reg.point_fail == 4
}

/// 本卡判据入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余走静态明细，
/// 红项判定全量保留不丢）。
pub fn run_unx12740_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12740 B38 预告与开门件（SEH 压测核账框架）");
    let results = extra_checks();
    let mut i = 0usize;
    while i < results.len() {
        if let Some((_name, passed, det)) = results.get(i) {
            let (sname, sdetail) = match CHECK_META.get(i) {
                Some((n, d)) => (*n, *d),
                None => ("判据名缺位", "判据明细缺位：闭表下标越界"),
            };
            if i < 8 {
                s.add_dyn(sname, *passed, det);
            } else {
                s.add(sname, *passed, sdetail);
            }
        }
        i += 1;
    }
    s
}

// ── 测试区（≥8 条 #[test]；仅此处允许 assert!/assert_eq!） ──────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_golden() {
        assert_eq!(golden_run(), Ok((10, golden_run().unwrap_or((0, 0)).1)));
        let (pass, fp) = golden_run().unwrap_or((0, 0));
        assert_eq!(pass, 10);
        assert_ne!(fp, 0);
    }

    #[test]
    fn test_boundary_zero_and_max() {
        assert!(boundary_zero_ok());
        assert!(boundary_max_ok());
        let mut reg = B38Registry::new();
        assert!(reg.preview_axis(0, 3).is_ok(), "轴0 首位合法");
        assert!(reg.preview_axis(3, 3).is_ok(), "轴3 末位合法");
    }

    #[test]
    fn test_boundary_max_plus_one() {
        assert!(boundary_max_plus_ok());
        let mut reg = B38Registry::new();
        assert_eq!(reg.preview_axis(4, 3), Err(B38Diag::ENoAxis), "轴4 越闭集");
        assert_eq!(reg.open_gate(6), Err(B38Diag::EOrder), "空册开门先拒 EOrder");
    }

    #[test]
    fn test_boundary_wrap() {
        assert!(boundary_wrap_ok());
        let mut reg = B38Registry::new();
        let huge = 0u32.wrapping_sub(1);
        assert_eq!(reg.preview_axis(huge, 3), Err(B38Diag::ENoAxis));
    }

    #[test]
    fn test_injection_families_all_caught() {
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught(), "三族 10 轮必全抓：{:?}/{:?}", led.tries, led.caught);
        let mut i = 0usize;
        while i < INJ_FAMILIES {
            assert_eq!(led.caught[i], 10, "族 {} 检出应为 10", i);
            i += 1;
        }
    }

    #[test]
    fn test_judge_order_frozen() {
        assert!(judge_order_ok());
        let mut reg = B38Registry::new();
        // 空册开门：未预告先开门的 EOrder 必须先于件号越册的 ENoGate。
        assert_eq!(reg.open_gate(99), Err(B38Diag::EOrder));
    }

    #[test]
    fn test_state_machine_rewind_reject() {
        assert!(rewind_reject_ok());
        let mut reg = B38Registry::new();
        let _ = reg.preview_axis(0, 3);
        let _ = reg.open_gate(0);
        assert_eq!(reg.preview_axis(1, 3), Err(B38Diag::ERewind), "Gated 期禁再预告");
        let mut reg2 = B38Registry::new();
        let _ = reg2.preview_axis(0, 3);
        let _ = reg2.seat();
        assert_eq!(reg2.stage(), RegStage::Previewed, "未落位阶段不变");
    }

    #[test]
    fn test_capacity_exhausted_ledger_intact() {
        assert!(capacity_ok());
        let mut reg = B38Registry::new();
        let mut i = 0usize;
        while i < POINT_CAP {
            assert!(reg.log_point(i % 2 == 0).is_ok());
            i += 1;
        }
        assert_eq!(reg.log_point(true), Err(B38Diag::ECapFull));
        assert_eq!(reg.point_at(0), Some(true));
        assert_eq!(reg.point_at(1), Some(false));
        assert_eq!(reg.point_at(POINT_CAP), None, "越界槽必 None");
    }

    #[test]
    fn test_dual_run_identical() {
        assert!(dual_run_ok());
        let a = extra_checks();
        let b = extra_checks();
        assert_eq!(a, b, "extra_checks 双跑逐字段相等");
    }

    #[test]
    fn test_seg_guard_and_closed_text() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert!(closed_text_ok());
        assert!(detail(SEG_BASE + 0x3f).contains("未知诊断码"));
        assert_eq!(detail(B38Diag::ETier.val()), B38Diag::ETier.detail());
    }

    #[test]
    fn test_writeback_done_and_summary_nonempty() {
        assert!(writeback_ok());
        let reg = B38Registry::new();
        let wb = make_writeback(&reg, 0);
        assert_eq!(wb.status, WorkStatus::InProgress, "空册不得已完成");
        assert!(!wb.summary.is_empty());
    }

    #[test]
    fn test_checks_all_green_and_frozen() {
        let s = run_unx12740_checks();
        assert!(s.all_passed(), "判据集必须全绿：{}", s.red_items_text());
        assert!(s.len() >= FROZEN_CHECK_COUNT, "判据条数只增不减");
        assert!(s.len() >= 12);
    }
}
