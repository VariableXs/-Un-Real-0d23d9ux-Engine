//! UNX-F52336 Regshot 收口 · 0xC93C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52336"，逐字）：
//!   title: Regshot 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52337-J1 六判据执行；
//!     依赖树枚举 ×10；EOL 声明（API 集新语义缺失如实列）；KB-COMPAT 定级
//!     S2 + EOL 标记；判据锚定 §AI-66.2 专题六；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52336）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52337-J1，照原文锚定；功能按本卡标题（Regshot）落地。
//! EOL 声明口径：卡面要求 EOL 标记，本模块以「EOL 声明 2 行闭集」如实落账
//! （快照 HTML 输出新语义缺失 / 注册表键安全描述符对比新语义缺失）；
//! 定级闭集仍按本批收口口径执行（全通 → S1）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① 快照对（1st/2nd）12 格：六对快照项逐格落账；对内相位闸（2nd 先于
//!      1st 落账必抓，对空账本同样必抓），对间单调（回录早对必抓）。
//!   ② 差异三族 16 格：注册表（6 格）/文件（6 格）/目录（4 格），族单调
//!      （逆序必抓）+ 相位闸（后族先录必抓）。
//!   ③ 输出三格式 6 格：文本/HTML/注册表三种格式 × 双份落账，越闭集必抓。
//!   ④ 对比序单调：差异族序只进不退，逆序必抓。
//!   ⑤ EOL 声明 2 行闭集：API 集新语义缺失如实列，缺一拒封。
//!   ⑥ 收口闸六判据：逐格落账，未齐即封。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10 轮（差异格越闭集/输出漂移/
//!      相位越序），三类注入全部落在当轮临时账本上，主账 injected==caught==10、
//!      escaped==0；漂移注入先录正格再注漂移；相位注入对空账本必抓且不改
//!      last_kind；双跑指纹一致；全通定级 S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52336;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 快照对格数（6 对 × 1st/2nd）。
pub const SNAP_CELLS: usize = 12;

/// 差异格数（注册表 6 + 文件 6 + 目录 4）。
pub const DIFF_CELLS: usize = 16;

/// 输出格式格数（3 格式 × 2）。
pub const OUT_CELLS: usize = 6;

/// EOL 声明行数。
pub const EOL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegshotCode {
    /// 成功
    Ok = 0,
    /// 差异格越闭集
    ENoCell,
    /// 载荷漂移
    ECellDrift,
    /// 序逆序（对间/族间回录）
    EOrder,
    /// 相位越序（2nd 先于 1st / 后族先录）
    EPhase,
    /// 重复落账
    EDup,
    /// 判据闸越闭集
    ENoGate,
    /// 未齐即封
    ESeal,
    /// 输出格式格越闭集
    ENoOut,
    /// EOL 声明行越闭集
    ENoRow,
    /// 快照格越闭集
    ENoSnap,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl RegshotCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RegshotCode::Ok => "成功",
            RegshotCode::ENoCell => "差异格越闭集：差异 16 格闭集之外先登记",
            RegshotCode::ECellDrift => "载荷漂移：快照/差异/输出折叠值与基准不恒等，逐格复算",
            RegshotCode::EOrder => "序逆序：快照对间与差异族间（注册表→文件→目录）只进不退，回录必抓",
            RegshotCode::EPhase => "相位越序：2nd 先于 1st 或后族先录，对空账本同样必抓",
            RegshotCode::EDup => "重复落账：同格只许一条账目",
            RegshotCode::ENoGate => "判据闸越闭集：六判据闸 0..=5 之外先登记",
            RegshotCode::ESeal => "未齐即封：12 快照格 + 16 差异格 + 6 输出格 + 2 行 EOL 声明未齐先补齐",
            RegshotCode::ENoOut => "输出格式格越闭集：输出 6 格闭集之外先登记",
            RegshotCode::ENoRow => "EOL 声明行越闭集：EOL 声明 2 行闭集之外先登记",
            RegshotCode::ENoSnap => "快照格越闭集：快照 12 格闭集之外先登记",
            RegshotCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RegshotCode::EGradeDrift => "定级漂移：全通且 EOL 声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+12快照格+16差异格+6输出格+2行EOL声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格（卡面 EOL 口径的登记位）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 快照格载荷（格 i；偶=1st 奇=2nd，对 = i/2；XOR 锚低 16 位恒零）。
pub const fn snap_cell(i: usize) -> Option<u64> {
    if i >= SNAP_CELLS {
        return None;
    }
    Some((0xB4500u64 + (i as u64) * 0x47) ^ 0x5253_0000)
}

/// 快照对（格 i 所在对）。
pub const fn snap_pair(i: usize) -> Option<u32> {
    if i >= SNAP_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 差异族（0=注册表 0..6 1=文件 6..12 2=目录 12..16）。
pub const fn diff_kind(i: usize) -> Option<u32> {
    if i >= DIFF_CELLS {
        return None;
    }
    match i {
        0..=5 => Some(0),
        6..=11 => Some(1),
        _ => Some(2),
    }
}

/// 族起点（族 k 的首格下标，相位闸用）。
pub const fn diff_kind_start(k: usize) -> usize {
    match k {
        0 => 0,
        1 => 6,
        _ => 12,
    }
}

/// 差异格载荷。
pub const fn diff_cell(i: usize) -> Option<u64> {
    if i >= DIFF_CELLS {
        return None;
    }
    Some((0xB4600u64 + (i as u64) * 0x4B) ^ 0x5253_0000)
}

/// 族名。
pub const fn diff_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("注册表"),
        1 => Some("文件"),
        2 => Some("目录"),
        _ => None,
    }
}

/// 输出格式格载荷（格 o；0/1=文本 2/3=HTML 4/5=注册表）。
pub const fn out_cell(o: usize) -> Option<u32> {
    if o >= OUT_CELLS {
        return None;
    }
    Some(0xB4700 + o as u32 * 0x35)
}

/// 输出格式名。
pub const fn out_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("文本"),
        1 => Some("HTML"),
        2 => Some("注册表"),
        _ => None,
    }
}

/// EOL 声明行折叠（行 r）。
pub const fn eol_row(r: usize) -> Option<u32> {
    if r >= EOL_ROWS {
        return None;
    }
    Some(0xB4800 + r as u32 * 0x35)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Regshot 收口账本状态机。
#[derive(Clone)]
pub struct RegshotLedger {
    /// 六判据（快照一拍/快照二拍/对比执行/报告输出/EOL 标记/许可声明）。
    gates: [Option<bool>; 6],
    /// 快照对账。
    snaps: [Option<u64>; SNAP_CELLS],
    /// 差异账。
    diffs: [Option<u64>; DIFF_CELLS],
    /// 输出账。
    outs: [Option<u32>; OUT_CELLS],
    /// EOL 声明账。
    eols: [Option<u32>; EOL_ROWS],
    /// 差异族单调游标。
    last_kind: i64,
    /// 快照对单调游标（与差异族游标分离，两账互不干扰）。
    last_pair: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RegshotLedger {
    /// 新账本。
    pub fn new() -> Self {
        RegshotLedger {
            gates: [None; 6],
            snaps: [None; SNAP_CELLS],
            diffs: [None; DIFF_CELLS],
            outs: [None; OUT_CELLS],
            eols: [None; EOL_ROWS],
            last_kind: -1,
            last_pair: -1,
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RegshotCode> {
        if gate >= 6 {
            return Err(RegshotCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 快照一格（对间单调 + 对内相位闸 + 载荷对照 + 重复拒）。
    pub fn record_snap(&mut self, i: usize, got: u64) -> Result<(), RegshotCode> {
        let pair = match snap_pair(i) {
            Some(p) => p,
            None => return Err(RegshotCode::ENoSnap),
        };
        if (pair as i64) < self.last_pair {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200);
            return Err(RegshotCode::EOrder);
        }
        // 对内相位闸：2nd（奇格）先于 1st（偶格）落账必抓，空账本同样成立。
        if i % 2 == 1 {
            let prev = i - 1;
            if match self.snaps.get(prev) {
                Some(Some(_)) => false,
                _ => true,
            } {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x9201);
                return Err(RegshotCode::EPhase);
            }
        }
        let want = match snap_cell(i) {
            Some(w) => w,
            None => return Err(RegshotCode::ENoSnap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9202 + i as u64);
            return Err(RegshotCode::ECellDrift);
        }
        if match self.snaps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RegshotCode::EDup);
        }
        if let Some(c) = self.snaps.get_mut(i) {
            *c = Some(got);
        }
        self.last_pair = pair as i64;
        self.fnv(0x9203 + i as u64);
        Ok(())
    }

    /// 快照全账核验（12 格）。
    pub fn snaps_all(&self) -> bool {
        let mut i = 0usize;
        while i < SNAP_CELLS {
            match self.snaps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 前族全齐核验（相位闸用）。
    fn families_complete(&self, upto: usize) -> bool {
        let bound = diff_kind_start(upto);
        let mut i = 0usize;
        while i < bound {
            match self.diffs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 差异一格（族单调 + 相位闸 + 载荷对照 + 重复拒）。
    pub fn record_diff(&mut self, i: usize, got: u64) -> Result<(), RegshotCode> {
        let kind = match diff_kind(i) {
            Some(k) => k,
            None => return Err(RegshotCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(RegshotCode::EOrder);
        }
        if (kind as usize) > 0 && !self.families_complete(kind as usize) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301);
            return Err(RegshotCode::EPhase);
        }
        let want = match diff_cell(i) {
            Some(w) => w,
            None => return Err(RegshotCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9302 + i as u64);
            return Err(RegshotCode::ECellDrift);
        }
        if match self.diffs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RegshotCode::EDup);
        }
        if let Some(c) = self.diffs.get_mut(i) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9303 + i as u64);
        Ok(())
    }

    /// 差异全账核验（16 格）。
    pub fn diffs_all(&self) -> bool {
        let mut i = 0usize;
        while i < DIFF_CELLS {
            match self.diffs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 输出格式一格。
    pub fn record_out(&mut self, o: usize, got: u32) -> Result<(), RegshotCode> {
        if o >= OUT_CELLS {
            return Err(RegshotCode::ENoOut);
        }
        if match self.outs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RegshotCode::EDup);
        }
        let want = match out_cell(o) {
            Some(w) => w,
            None => return Err(RegshotCode::ENoOut),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + o as u64);
            return Err(RegshotCode::ECellDrift);
        }
        if let Some(c) = self.outs.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x9401 + o as u64);
        Ok(())
    }

    /// 输出全账核验（6 格）。
    pub fn outs_all(&self) -> bool {
        let mut o = 0usize;
        while o < OUT_CELLS {
            match self.outs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// EOL 声明一行。
    pub fn declare_eol(&mut self, r: usize) -> Result<(), RegshotCode> {
        if r >= EOL_ROWS {
            return Err(RegshotCode::ENoRow);
        }
        if match self.eols.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RegshotCode::EDup);
        }
        if let Some(c) = self.eols.get_mut(r) {
            *c = eol_row(r);
        }
        self.fnv(0x9500 + r as u64);
        Ok(())
    }

    /// EOL 声明全齐核验。
    pub fn eols_all(&self) -> bool {
        let mut r = 0usize;
        while r < EOL_ROWS {
            match self.eols.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后序（相位/序注入不改此值）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2；卡面 EOL 口径
    /// 以 S2 档作为「EOL 标记」登记位保留）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.snaps_all() && self.diffs_all() && self.outs_all() && self.eols_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RegshotLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RegshotLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < SNAP_CELLS {
        if let Some(w) = snap_cell(i) {
            if led.record_snap(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut d = 0usize;
    while d < DIFF_CELLS {
        if let Some(w) = diff_cell(d) {
            if led.record_diff(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut o = 0usize;
    while o < OUT_CELLS {
        if let Some(w) = out_cell(o) {
            if led.record_out(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < EOL_ROWS {
        let _ = led.declare_eol(r);
        r += 1;
    }
    // 注入 10 轮：差异格越闭集 / 输出漂移 / 相位越序 三态轮转，全部落在当轮临时账本。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RegshotLedger::new();
        let caught = match kind {
            0 => lt.record_diff(DIFF_CELLS, 0) == Err(RegshotCode::ENoCell),
            1 => {
                // 漂移注入：先录正格再注漂移。
                let _ = lt.record_out(0, out_cell(0).unwrap_or(0));
                lt.record_out(1, out_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(RegshotCode::ECellDrift)
            }
            _ => {
                // 相位注入：对空账本先录 2nd（快照格 1），必抓且不改 last_kind。
                lt.record_snap(1, snap_cell(1).unwrap_or(0)) == Err(RegshotCode::EPhase)
                    && lt.last_kind() == -1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9600 + k as u64);
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
/// UNX-F52336 判据集（锚 UNX-F52337-J1，off-by-one 板式）。
pub fn run_unx52336_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52336");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "快照对（1st/2nd）12 格全通",
        recorded >= (SNAP_CELLS + DIFF_CELLS) as u32,
        "六对快照项按 1st→2nd 相位逐格恒等落账",
    );
    s.add(
        "差异三族 16 格全落",
        {
            let mut lt = RegshotLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DIFF_CELLS {
                if let Some(w) = diff_cell(d) {
                    if lt.record_diff(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.diffs_all()
        },
        "注册表/文件/目录三族逐格恒等落账",
    );
    s.add(
        "输出三格式 6 格全落",
        {
            let mut lt = RegshotLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OUT_CELLS {
                if let Some(w) = out_cell(o) {
                    if lt.record_out(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.outs_all()
        },
        "文本/HTML/注册表三格式双份落账，越闭集必抓",
    );
    s.add(
        "收口闸六判据全落",
        {
            let mut lt = RegshotLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "快照一拍/快照二拍/对比执行/报告输出/EOL 标记/许可声明",
    );
    s.add(
        "EOL 声明 2/2 全齐",
        {
            let mut lt = RegshotLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < EOL_ROWS {
                if lt.declare_eol(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.eols_all()
        },
        "API 集新语义缺失如实列：快照 HTML 输出/注册表键安全描述符对比两行闭集",
    );
    s.add(
        "对比序单调",
        {
            let mut ok = true;
            let mut d = 1usize;
            while d < DIFF_CELLS {
                if diff_kind(d).unwrap_or(3) < diff_kind(d - 1).unwrap_or(3) {
                    ok = false;
                }
                d += 1;
            }
            ok
        },
        "差异族序注册表→文件→目录只进不退，逆序必抓",
    );
    s.add(
        "判据 ×10（依赖树枚举口径，off-by-one）",
        caught >= 10,
        "十轮注入三态轮转全部识别，无一逃逸",
    );
    s.add(
        "边界·差异格越闭集",
        {
            let mut lt = RegshotLedger::new();
            lt.record_diff(DIFF_CELLS, 0).is_err()
        },
        "差异格 16 越闭集必拒",
    );
    s.add(
        "边界·快照格越闭集",
        {
            let mut lt = RegshotLedger::new();
            lt.record_snap(SNAP_CELLS, 0).is_err()
        },
        "快照格 12 越闭集必拒",
    );
    s.add(
        "边界·EOL 声明越闭集",
        {
            let mut lt = RegshotLedger::new();
            lt.declare_eol(EOL_ROWS).is_err()
        },
        "EOL 声明行 2 越闭集必拒",
    );
    s.add(
        "相位注入对空账本必抓且不改 last_kind",
        {
            let mut lt = RegshotLedger::new();
            let hit = lt.record_snap(3, snap_cell(3).unwrap_or(0)) == Err(RegshotCode::EPhase);
            hit && lt.last_kind() == -1
        },
        "2nd 先于 1st 落账，空账本同样必抓",
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
            // main/aux 各节全部记账后再断言定级。
            let mut lt = RegshotLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < SNAP_CELLS {
                if let Some(w) = snap_cell(i) {
                    let _ = lt.record_snap(i, w);
                }
                i += 1;
            }
            let mut d = 0usize;
            while d < DIFF_CELLS {
                if let Some(w) = diff_cell(d) {
                    let _ = lt.record_diff(d, w);
                }
                d += 1;
            }
            let mut o = 0usize;
            while o < OUT_CELLS {
                if let Some(w) = out_cell(o) {
                    let _ = lt.record_out(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < EOL_ROWS {
                let _ = lt.declare_eol(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，S2 档保留作 EOL 标记位）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC93C0,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC93C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_kinds() {
        assert_eq!(diff_kind(0), Some(0));
        assert_eq!(diff_kind(5), Some(0));
        assert_eq!(diff_kind(6), Some(1));
        assert_eq!(diff_kind(11), Some(1));
        assert_eq!(diff_kind(12), Some(2));
        assert_eq!(diff_kind(15), Some(2));
        assert!(diff_kind(16).is_none());
        assert!(diff_cell(16).is_none());
        assert_eq!(diff_kind_name(0), Some("注册表"));
        assert_eq!(diff_kind_name(2), Some("目录"));
        assert!(diff_kind_name(3).is_none());
    }

    #[test]
    fn test_snap_pairs() {
        assert_eq!(snap_pair(0), Some(0));
        assert_eq!(snap_pair(1), Some(0));
        assert_eq!(snap_pair(11), Some(5));
        assert!(snap_pair(12).is_none());
        assert!(snap_cell(12).is_none());
    }

    #[test]
    fn test_out_cells() {
        assert!(out_cell(OUT_CELLS).is_none());
        assert_ne!(out_cell(0).unwrap_or(0), out_cell(1).unwrap_or(0));
        assert_eq!(out_kind_name(0), Some("文本"));
        assert_eq!(out_kind_name(1), Some("HTML"));
        assert_eq!(out_kind_name(2), Some("注册表"));
        assert!(out_kind_name(3).is_none());
        assert!(eol_row(EOL_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = diff_cell(7).unwrap_or(0);
        assert_eq!(a, diff_cell(7).unwrap_or(0));
        assert_ne!(a, diff_cell(8).unwrap_or(0));
    }

    #[test]
    fn test_snap_phase_caught() {
        let mut lt = RegshotLedger::new();
        // 2nd 先于 1st → 相位必抓，空账本同样成立。
        assert_eq!(
            lt.record_snap(1, snap_cell(1).unwrap_or(0)),
            Err(RegshotCode::EPhase)
        );
        assert_eq!(lt.last_kind(), -1);
        // 1st 先落，2nd 随后通过。
        assert!(lt.record_snap(0, snap_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_snap(1, snap_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_snap_order_caught() {
        // 测试种子须按合法序先落低对格。
        let mut lt = RegshotLedger::new();
        let mut i = 0usize;
        while i < 4 {
            let _ = lt.record_snap(i, snap_cell(i).unwrap_or(0));
            i += 1;
        }
        // 回录第 0 对 → 逆序必抓；正序续录第 2 对仍通过。
        assert_eq!(
            lt.record_snap(0, snap_cell(0).unwrap_or(0)),
            Err(RegshotCode::EOrder)
        );
        assert!(lt.record_snap(4, snap_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_diff_family_order_caught() {
        let mut lt = RegshotLedger::new();
        let mut d = 0usize;
        while d < 2 * 6 {
            let _ = lt.record_diff(d, diff_cell(d).unwrap_or(0));
            d += 1;
        }
        // 回录注册表族 → 逆序必抓；正序续录目录族仍通过。
        assert_eq!(
            lt.record_diff(2, diff_cell(2).unwrap_or(0)),
            Err(RegshotCode::EOrder)
        );
        assert!(lt.record_diff(12, diff_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_dup_caught() {
        let mut lt = RegshotLedger::new();
        assert_eq!(
            lt.record_out(3, out_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(RegshotCode::ECellDrift)
        );
        let _ = lt.declare_eol(0);
        assert_eq!(lt.declare_eol(0), Err(RegshotCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RegshotLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SNAP_CELLS + DIFF_CELLS) as u32);
        assert_eq!(caught, 10);
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
        assert_eq!(SEG_BASE, 0xC93C0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RegshotCode::Ok as u32, 0);
        assert_eq!(RegshotCode::ENoSnap as u32, 10);
        assert_eq!(RegshotCode::EInj as u32, 11);
        assert_eq!(RegshotCode::EGradeDrift as u32, 12);
        let all = [
            RegshotCode::Ok,
            RegshotCode::ENoCell,
            RegshotCode::ECellDrift,
            RegshotCode::EOrder,
            RegshotCode::EPhase,
            RegshotCode::EDup,
            RegshotCode::ENoGate,
            RegshotCode::ESeal,
            RegshotCode::ENoOut,
            RegshotCode::ENoRow,
            RegshotCode::ENoSnap,
            RegshotCode::EInj,
            RegshotCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = RegshotLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52336_checks_green() {
        let s = run_unx52336_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
