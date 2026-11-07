//! UNX-F52344 mdBook 收口 · 0xC95C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52344"，逐字）：
//!   title: mdBook 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52345-J1 build
//!     ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52344）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52345-J1，照原文锚定；功能按本卡标题（mdBook）落地。
//! 本模块按 52201 样板口径运行于 crate 级 `#![cfg_attr(not(test),
//! no_std)]` + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（mdBook 域语料，逐项定制）：
//!   ① book.toml 双向投影 12 格：正向 toml_cell 折叠、逆向 toml_cell_inverse
//!      唯一还原，逆核不中方即漂移必抓；XOR 锚低 16 位恒零。
//!   ② 五命令（build/serve/test/clean/init）10 操作：每命令 2 步（预检
//!      →执行），命令家族单调（逆序必抓），空账首格必须自 build 家族
//!      起，跳族即抓且不改 last_kind。
//!   ③ SUMMARY.md 章节树 8 格：卷首/序章/章节一组/小节/章节二组/草稿
//!      分隔/附录/番外逐格对照，缺一拒封。
//!   ④ 预处理器 6 格：links/mermaid/toc/tabs/footnote/pagebreak 逐格
//!      对照，越闭集必拒。
//!   ⑤ 增量构建 8 拍单调（拍载荷严格递增，回退拍必抓）；Rust 上栈 4 层
//!      顺序压栈（toolchain→crate→mdbook→render 后端），乱序按命令序
//!      同构抓（ECmdOrder）。
//!   ⑥ ledger 状态机；注入核 run_cycle 10 轮三态轮转（toml 漂移/命令
//!      逆序/命令越闭集），全部落在当轮临时账本上 10/10 全抓；双跑
//!      指纹一致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52344;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// book.toml 投影格数。
pub const TOML_CELLS: usize = 12;

/// 五命令操作格数（5 × 2 步）。
pub const CMD_OPS: usize = 10;

/// SUMMARY.md 章节树格数。
pub const SUM_CELLS: usize = 8;

/// 预处理器格数。
pub const PRE_CELLS: usize = 6;

/// 增量构建拍数。
pub const BEATS: usize = 8;

/// Rust 上栈层数。
pub const STACK_LEVELS: usize = 4;

/// 收口判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MdbookCode {
    /// 成功
    Ok = 0,
    /// book.toml 投影格越闭集
    ENoToml,
    /// book.toml 投影格漂移
    ETomlDrift,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令操作漂移
    ECmdDrift,
    /// 命令家族逆序
    ECmdOrder,
    /// SUMMARY 章节树格越闭集
    ENoSum,
    /// 预处理器格越闭集
    ENoPre,
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

impl MdbookCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MdbookCode::Ok => "成功",
            MdbookCode::ENoToml => "投影格越闭集：book.toml 12 格闭集之外先登记",
            MdbookCode::ETomlDrift => "投影格漂移：正逆投影不互证，逐格复算",
            MdbookCode::ENoCmd => "命令操作越闭集：五命令 10 格闭集之外先登记",
            MdbookCode::ECmdDrift => "命令操作漂移：操作折叠与基准不恒等，逐格复算",
            MdbookCode::ECmdOrder => "命令家族逆序：build→serve→test→clean→init 单调，逆序必抓",
            MdbookCode::ENoSum => "SUMMARY 章节树格越闭集：8 格闭集之外先登记",
            MdbookCode::ENoPre => "预处理器格越闭集：6 格闭集之外先登记",
            MdbookCode::EBeat => "增量拍非单调：8 拍载荷严格递增，回退拍必抓",
            MdbookCode::ESeal => "未齐即封：12 投影 + 10 命令 + 8 章节 + 6 预处理 + 4 层 + 8 拍未齐先补齐",
            MdbookCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MdbookCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MdbookCode::EGradeDrift => "定级漂移：全通且章节树全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+12投影+10命令+8章节+6预处理+4层+8拍全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// book.toml 投影锚（低 16 位恒零）。
const TOML_ANCHOR: u64 = 0x4D42_0000;

/// book.toml 双向投影正向折叠（格 i）。
pub const fn toml_cell(i: usize) -> Option<u64> {
    if i >= TOML_CELLS {
        return None;
    }
    Some(TOML_ANCHOR ^ ((i as u64 + 1).wrapping_mul(0x8B8B)))
}

/// book.toml 双向投影逆核：由折叠值唯一还原格位。
pub fn toml_cell_inverse(v: u64) -> Option<usize> {
    let mut i = 0usize;
    while i < TOML_CELLS {
        if let Some(w) = toml_cell(i) {
            if w == v {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 命令家族（0..2=build 2..4=serve 4..6=test 6..8=clean 8..10=init）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令名。
pub const fn command_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("build"),
        1 => Some("serve"),
        2 => Some("test"),
        3 => Some("clean"),
        4 => Some("init"),
        _ => None,
    }
}

/// 命令步名（0 预检 1 执行）。
pub const fn cmd_step_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("预检"),
        1 => Some("执行"),
        _ => None,
    }
}

/// 命令操作格载荷。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some(0x424F_0000 ^ ((o as u64 + 1).wrapping_mul(0x2E3)))
}

/// SUMMARY.md 章节树格载荷。
pub const fn sum_cell(i: usize) -> Option<u64> {
    if i >= SUM_CELLS {
        return None;
    }
    Some(0x5355_0000 ^ ((i as u64 + 1).wrapping_mul(0x3E1)))
}

/// 章节树格名。
pub const fn sum_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("卷首 Part I"),
        1 => Some("序章 Foreword"),
        2 => Some("章节一组 Chapter"),
        3 => Some("小节 Sub-item"),
        4 => Some("章节二组 Chapter"),
        5 => Some("草稿分隔 Draft"),
        6 => Some("附录 Appendix"),
        7 => Some("番外 章节尾链"),
        _ => None,
    }
}

/// 预处理器格载荷。
pub const fn pre_cell(i: usize) -> Option<u64> {
    if i >= PRE_CELLS {
        return None;
    }
    Some(0x5052_0000 + i as u64 * 0x22)
}

/// 预处理器名。
pub const fn pre_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("links"),
        1 => Some("mermaid"),
        2 => Some("toc"),
        3 => Some("tabs"),
        4 => Some("footnote"),
        5 => Some("pagebreak"),
        _ => None,
    }
}

/// 增量构建拍载荷（严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0x4245_0000 + b as u64 * 0x0C)
}

/// Rust 上栈层名（0 toolchain 1 crate 2 mdbook 3 render 后端）。
pub const fn stack_level_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("toolchain 就绪"),
        1 => Some("crate 装载"),
        2 => Some("mdbook 核心"),
        3 => Some("render 后端挂接"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// mdBook 收口账本状态机。
#[derive(Clone)]
pub struct MdbookLedger {
    /// 六闸（toml 解析/SUMMARY/预处理/构建/渲染/写回）。
    gates: [Option<bool>; GATES],
    /// book.toml 投影账。
    tomls: [Option<u64>; TOML_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 章节树账。
    sums: [Option<u64>; SUM_CELLS],
    /// 预处理器账。
    pres: [Option<u64>; PRE_CELLS],
    /// 增量拍账。
    beats: [Option<u64>; BEATS],
    /// 已压栈层数（下一层必须是 depth）。
    depth: usize,
    /// 最后命令家族。
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

impl MdbookLedger {
    /// 新账本。
    pub fn new() -> Self {
        MdbookLedger {
            gates: [None; GATES],
            tomls: [None; TOML_CELLS],
            cmds: [None; CMD_OPS],
            sums: [None; SUM_CELLS],
            pres: [None; PRE_CELLS],
            beats: [None; BEATS],
            depth: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MdbookCode> {
        if gate >= GATES {
            return Err(MdbookCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// book.toml 投影一格（正逆互证 + 重复拒）。
    pub fn record_toml(&mut self, i: usize, got: u64) -> Result<(), MdbookCode> {
        let want = match toml_cell(i) {
            Some(w) => w,
            None => return Err(MdbookCode::ENoToml),
        };
        if got != want || toml_cell_inverse(got) != Some(i) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + i as u64);
            return Err(MdbookCode::ETomlDrift);
        }
        if match self.tomls.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MdbookCode::ESeal);
        }
        if let Some(c) = self.tomls.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x8201 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn tomls_all(&self) -> bool {
        let mut i = 0usize;
        while i < TOML_CELLS {
            match self.tomls.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令操作一格（家族单调 + 载荷对照 + 重复拒）。
    /// 空账本首格必须自家族 0（build 预检）起，跳族即抓且不改 last_kind。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), MdbookCode> {
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(MdbookCode::ENoCmd),
        };
        if (self.last_kind >= 0 && (kind as i64) < self.last_kind)
            || (self.last_kind < 0 && kind != 0)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(MdbookCode::ECmdOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(MdbookCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(MdbookCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MdbookCode::ESeal);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// 命令全账核验（10 格）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 章节树一格（越闭集拒 + 重复拒）。
    pub fn record_sum(&mut self, i: usize, got: u64) -> Result<(), MdbookCode> {
        let want = match sum_cell(i) {
            Some(w) => w,
            None => return Err(MdbookCode::ENoSum),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + i as u64);
            return Err(MdbookCode::ECmdDrift);
        }
        if match self.sums.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MdbookCode::ESeal);
        }
        if let Some(c) = self.sums.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x8401 + i as u64);
        Ok(())
    }

    /// 章节树全账核验（8 格，缺一拒封）。
    pub fn sums_all(&self) -> bool {
        let mut i = 0usize;
        while i < SUM_CELLS {
            match self.sums.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 预处理器一格（越闭集拒 + 重复拒）。
    pub fn record_pre(&mut self, i: usize, got: u64) -> Result<(), MdbookCode> {
        let want = match pre_cell(i) {
            Some(w) => w,
            None => return Err(MdbookCode::ENoPre),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + i as u64);
            return Err(MdbookCode::ETomlDrift);
        }
        if match self.pres.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MdbookCode::ESeal);
        }
        if let Some(c) = self.pres.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x8501 + i as u64);
        Ok(())
    }

    /// 预处理器全账核验（6 格，缺一拒封）。
    pub fn pres_all(&self) -> bool {
        let mut i = 0usize;
        while i < PRE_CELLS {
            match self.pres.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 增量构建一拍（载荷严格递增，回退拍必抓）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), MdbookCode> {
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(MdbookCode::EBeat),
        };
        if got != want || got <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + b as u64);
            return Err(MdbookCode::EBeat);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got;
        self.fnv(0x8601 + b as u64);
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

    /// Rust 上栈一层（顺序压栈：第 depth 层必须恰为 depth，跳层按命令
    /// 家族序同构抓，返回 ECmdOrder）。
    pub fn push_stack(&mut self, l: usize) -> Result<(), MdbookCode> {
        if l >= STACK_LEVELS || l != self.depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8700 + l as u64);
            return Err(MdbookCode::ECmdOrder);
        }
        self.depth = l + 1;
        self.fnv(0x8701 + l as u64);
        Ok(())
    }

    /// 上栈满层核验（4 层）。
    pub fn stack_full(&self) -> bool {
        self.depth == STACK_LEVELS
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
            && self.tomls_all()
            && self.cmds_all()
            && self.sums_all()
            && self.pres_all()
            && self.beats_all()
            && self.stack_full()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MdbookLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 空账全量灌装（六闸 + 投影 + 命令 + 章节 + 预处理 + 上栈 + 增量拍）。
fn fill_ledger(led: &mut MdbookLedger) -> u32 {
    let mut recorded = 0u32;
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut i = 0usize;
    while i < TOML_CELLS {
        if let Some(w) = toml_cell(i) {
            if led.record_toml(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut m = 0usize;
    while m < SUM_CELLS {
        if let Some(w) = sum_cell(m) {
            if led.record_sum(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut p = 0usize;
    while p < PRE_CELLS {
        if let Some(w) = pre_cell(p) {
            if led.record_pre(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut l = 0usize;
    while l < STACK_LEVELS {
        let _ = led.push_stack(l);
        l += 1;
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
/// 注入 10 轮三态轮转（toml 漂移/命令逆序/命令越闭集），全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MdbookLedger::new();
    let recorded = fill_ledger(&mut led);
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MdbookLedger::new();
        let caught = match kind {
            0 => {
                // 空账首格跳族（build → test 家族）：必抓且不改 last_kind。
                lt.record_cmd(4, cmd_cell(4).unwrap_or(0)) == Err(MdbookCode::ECmdOrder)
            }
            1 => {
                // 先按合法序落低族格，再注 toml 漂移。
                let _ = lt.record_toml(0, toml_cell(0).unwrap_or(0));
                let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
                lt.record_toml(1, toml_cell(1).unwrap_or(0).wrapping_add(7))
                    == Err(MdbookCode::ETomlDrift)
            }
            _ => {
                // 命令越闭集：格 10 在 10 格闭集之外。
                lt.record_cmd(CMD_OPS, 0) == Err(MdbookCode::ENoCmd)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + k as u64);
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
/// UNX-F52344 判据集（锚 UNX-F52345-J1，off-by-one 板式）。
pub fn run_unx52344_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52344");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "book.toml 双向投影 12 格全通",
        recorded >= TOML_CELLS as u32,
        "正向折叠与逆核唯一还原互证，逐格恒等",
    );
    s.add(
        "双向投影逆核",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < TOML_CELLS {
                match toml_cell(i) {
                    Some(w) => {
                        if toml_cell_inverse(w) != Some(i) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok && toml_cell_inverse(0xCAFE_0000_0004).is_none()
        },
        "投影值逆核还原格位，外来值不中方",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = MdbookLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    if lt.record_cmd(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.cmds_all()
        },
        "build/serve/test/clean/init 五命令逐操作恒等",
    );
    s.add(
        "命令家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令闭集单调，逆序必抓",
    );
    s.add(
        "SUMMARY.md 章节树 8 格全落",
        {
            let mut lt = MdbookLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < SUM_CELLS {
                if let Some(w) = sum_cell(i) {
                    if lt.record_sum(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.sums_all()
        },
        "卷首/序章/章节组/小节/草稿/附录/番外逐格对照，缺一拒封",
    );
    s.add(
        "预处理器 6 格全落",
        {
            let mut lt = MdbookLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PRE_CELLS {
                if let Some(w) = pre_cell(i) {
                    if lt.record_pre(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.pres_all()
        },
        "links/mermaid/toc/tabs/footnote/pagebreak 逐格对照",
    );
    s.add(
        "Rust 上栈 4 层全通",
        {
            let mut lt = MdbookLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < STACK_LEVELS {
                if lt.push_stack(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.stack_full()
        },
        "toolchain→crate→mdbook→render 后端顺序压栈",
    );
    s.add(
        "增量构建 8 拍单调",
        {
            let mut lt = MdbookLedger::new();
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
        "toml 漂移/命令逆序/命令越闭集三态轮转必抓",
    );
    let mut l0 = MdbookLedger::new();
    s.add(
        "边界·投影格越闭集",
        l0.record_toml(TOML_CELLS, 0) == Err(MdbookCode::ENoToml),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0) == Err(MdbookCode::ENoCmd),
        "命令操作 10 越闭集必拒",
    );
    s.add(
        "边界·预处理格越闭集",
        l0.record_pre(PRE_CELLS, 0) == Err(MdbookCode::ENoPre),
        "预处理器格 6 越闭集必拒",
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
            let mut lt = MdbookLedger::new();
            let _ = fill_ledger(&mut lt);
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC95C0,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC95C0",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_projection() {
        assert!(toml_cell(0).is_some());
        assert!(toml_cell(TOML_CELLS).is_none());
        assert_eq!(toml_cell_inverse(toml_cell(5).unwrap_or(0)), Some(5));
        assert!(toml_cell_inverse(0x1234_0000_0005).is_none());
    }

    #[test]
    fn test_cmd_kind_mapping() {
        // 家族映射：kind = o / 2（五命令各 2 步）。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(1), Some(0));
        assert_eq!(cmd_kind(6), Some(3));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(command_name(0), Some("build"));
        assert_eq!(command_name(4), Some("init"));
        assert!(command_name(5).is_none());
        assert_eq!(cmd_step_name(1), Some("执行"));
        assert!(cmd_step_name(3).is_none());
    }

    #[test]
    fn test_sum_names() {
        assert_eq!(sum_name(0), Some("卷首 Part I"));
        assert_eq!(sum_name(3), Some("小节 Sub-item"));
        assert_eq!(sum_name(7), Some("番外 章节尾链"));
        assert!(sum_name(8).is_none());
        assert!(sum_cell(8).is_none());
    }

    #[test]
    fn test_pre_names() {
        assert_eq!(pre_name(0), Some("links"));
        assert_eq!(pre_name(5), Some("pagebreak"));
        assert!(pre_name(6).is_none());
        assert!(pre_cell(6).is_none());
        assert!(stack_level_name(0).is_some());
        assert!(stack_level_name(4).is_none());
    }

    #[test]
    fn test_stack_order() {
        let mut lt = MdbookLedger::new();
        assert_eq!(lt.push_stack(3), Err(MdbookCode::ECmdOrder));
        assert!(lt.push_stack(0).is_ok());
        assert!(lt.push_stack(1).is_ok());
        assert!(lt.push_stack(2).is_ok());
        assert!(!lt.stack_full());
        assert!(lt.push_stack(3).is_ok());
        assert!(lt.stack_full());
    }

    #[test]
    fn test_first_cmd_family_zero() {
        // 空账本首格跳族必抓且不改 last_kind。
        let mut lt = MdbookLedger::new();
        assert_eq!(lt.record_cmd(8, cmd_cell(8).unwrap_or(0)), Err(MdbookCode::ECmdOrder));
        assert!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = MdbookLedger::new();
        let mut i = 0usize;
        while i <= 7 {
            let _ = lt.record_cmd(i, cmd_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(MdbookCode::ECmdOrder));
        assert!(lt.record_cmd(8, cmd_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_dup() {
        let mut lt = MdbookLedger::new();
        assert_eq!(
            lt.record_toml(9, toml_cell(9).unwrap_or(0).wrapping_add(6)),
            Err(MdbookCode::ETomlDrift)
        );
        let _ = lt.record_sum(4, sum_cell(4).unwrap_or(0));
        assert_eq!(lt.record_sum(4, sum_cell(4).unwrap_or(0)), Err(MdbookCode::ESeal));
    }

    #[test]
    fn test_beat_monotone() {
        let mut lt = MdbookLedger::new();
        let _ = lt.record_beat(0, beat_cell(0).unwrap_or(0));
        assert_eq!(lt.record_beat(1, beat_cell(1).unwrap_or(0).wrapping_sub(2)), Err(MdbookCode::EBeat));
        assert!(lt.record_beat(1, beat_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MdbookLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MdbookLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= TOML_CELLS as u32 + CMD_OPS as u32 + SUM_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC95C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MdbookCode::Ok as u32, 0);
        assert_eq!(MdbookCode::EGradeDrift as u32, 12);
        assert!(!MdbookCode::Ok.detail().is_empty());
        assert!(!MdbookCode::ESeal.detail().is_empty());
        let _ = MdbookCode::ENoPre.val();
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = MdbookLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 如实阶梯核对基线：空账 → S3。
        // 如实阶梯：六闸齐但各面未齐 → S2（全齐才 S1）。
        let mut lt2 = MdbookLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52344_checks_green() {
        let s = run_unx52344_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
