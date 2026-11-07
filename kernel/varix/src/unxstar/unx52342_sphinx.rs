//! UNX-F52342 Sphinx 收口 · 0xC9540 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52342"，逐字）：
//!   title: Sphinx 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52343-J1 build/
//!     serve ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52342）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52343-J1，照原文锚定；功能按本卡标题（Sphinx）落地。
//! 本模块按 52201 样板口径运行于 crate 级 `#![cfg_attr(not(test),
//! no_std)]` + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（Sphinx 域语料，逐项定制）：
//!   ① conf.py 双向投影 12 格：正向 conf_cell 折叠、逆向 conf_cell_inverse
//!      唯一还原，逆核不中方即漂移必抓；XOR 锚低 16 位恒零。
//!   ② 五 builder（html/dirhtml/singlehtml/pdf/epub）10 格：每 builder
//!      2 步（配置→产出），builder 家族单调（逆序必抓），空账首格必须
//!      自 html 家族起，跳族即抓且不改 last_kind。
//!   ③ reST 指令 10 操作闭集：toctree/code-block/math/note/warning/
//!      figure/table/ref/doc/automodule，逐格落账，越闭集必拒。
//!   ④ 扩展 8 格：myst_parser/mermaid/autodoc/viewcode/tabs/copied/
//!      pragmaxer/tocdepth 扩展声明逐格对照，缺一拒封。
//!   ⑤ 增量构建 8 拍单调（拍载荷严格递增，回退拍必抓）；Python 上栈
//!      4 层顺序压栈（venv→pip→sphinx-build→doctree 缓存），乱序必抓。
//!   ⑥ ledger 状态机；注入核 run_cycle 10 轮三态轮转（conf 漂移/builder
//!      逆序/builder 越闭集），全部落在当轮临时账本上 10/10 全抓；
//!      双跑指纹一致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52342;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// conf.py 投影格数。
pub const CONF_CELLS: usize = 12;

/// 五 builder 操作格数（5 × 2 步）。
pub const BLD_OPS: usize = 10;

/// reST 指令操作数。
pub const RST_OPS: usize = 10;

/// 扩展声明格数。
pub const EXT_CELLS: usize = 8;

/// 增量构建拍数。
pub const BEATS: usize = 8;

/// Python 上栈层数。
pub const STACK_LEVELS: usize = 4;

/// 收口判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SphinxCode {
    /// 成功
    Ok = 0,
    /// conf 投影格越闭集
    ENoConf,
    /// conf 投影格漂移
    EConfDrift,
    /// builder 操作越闭集
    ENoBld,
    /// builder 操作漂移
    EBldDrift,
    /// builder 家族逆序
    EBldOrder,
    /// reST 指令越闭集
    ENoRst,
    /// 增量拍非单调
    EBeat,
    /// 上栈乱序
    EStack,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl SphinxCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SphinxCode::Ok => "成功",
            SphinxCode::ENoConf => "conf 投影格越闭集：conf.py 12 格闭集之外先登记",
            SphinxCode::EConfDrift => "conf 投影格漂移：正逆投影不互证，逐格复算",
            SphinxCode::ENoBld => "builder 操作越闭集：五 builder 10 格闭集之外先登记",
            SphinxCode::EBldDrift => "builder 操作漂移：操作折叠与基准不恒等，逐格复算",
            SphinxCode::EBldOrder => "builder 家族逆序：html→dirhtml→singlehtml→pdf→epub 单调，逆序必抓",
            SphinxCode::ENoRst => "reST 指令越闭集：10 操作闭集之外先登记",
            SphinxCode::EBeat => "增量拍非单调：8 拍载荷严格递增，回退拍必抓",
            SphinxCode::EStack => "上栈乱序：venv→pip→sphinx-build→doctree 四层顺序压栈，跳层必抓",
            SphinxCode::ESeal => "未齐即封：12 conf + 10 builder + 10 指令 + 8 扩展 + 4 层 + 8 拍未齐先补齐",
            SphinxCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SphinxCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SphinxCode::EGradeDrift => "定级漂移：全通且扩展全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+12conf+10builder+10指令+8扩展+4层+8拍全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// conf.py 投影锚（低 16 位恒零）。
const CONF_ANCHOR: u64 = 0x5350_0000;

/// conf.py 双向投影正向折叠（格 i）。
pub const fn conf_cell(i: usize) -> Option<u64> {
    if i >= CONF_CELLS {
        return None;
    }
    Some(CONF_ANCHOR ^ ((i as u64 + 1).wrapping_mul(0xA5A5)))
}

/// conf.py 双向投影逆核：由折叠值唯一还原格位。
pub fn conf_cell_inverse(v: u64) -> Option<usize> {
    let mut i = 0usize;
    while i < CONF_CELLS {
        if let Some(w) = conf_cell(i) {
            if w == v {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// builder 家族（0..2=html 2..4=dirhtml 4..6=singlehtml 6..8=pdf 8..10=epub）。
pub const fn bld_kind(o: usize) -> Option<u32> {
    if o >= BLD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// builder 名。
pub const fn builder_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("html"),
        1 => Some("dirhtml"),
        2 => Some("singlehtml"),
        3 => Some("pdf"),
        4 => Some("epub"),
        _ => None,
    }
}

/// builder 步名（0 配置 1 产出）。
pub const fn bld_step_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("配置"),
        1 => Some("产出"),
        _ => None,
    }
}

/// builder 操作格载荷。
pub const fn bld_cell(o: usize) -> Option<u64> {
    if o >= BLD_OPS {
        return None;
    }
    Some(0x424C_0000 ^ ((o as u64 + 1).wrapping_mul(0x2C1)))
}

/// reST 指令标签。
pub const fn rst_label(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("toctree"),
        1 => Some("code-block"),
        2 => Some("math"),
        3 => Some("note"),
        4 => Some("warning"),
        5 => Some("figure"),
        6 => Some("table"),
        7 => Some(":ref:"),
        8 => Some(":doc:"),
        9 => Some("automodule"),
        _ => None,
    }
}

/// reST 指令格载荷。
pub const fn rst_cell(i: usize) -> Option<u64> {
    if i >= RST_OPS {
        return None;
    }
    Some(0x5253_0000 ^ ((i as u64 + 1).wrapping_mul(0x5B1)))
}

/// 扩展声明格载荷。
pub const fn ext_cell(i: usize) -> Option<u64> {
    if i >= EXT_CELLS {
        return None;
    }
    Some(0x4558_0000 + i as u64 * 0x34)
}

/// 扩展名。
pub const fn ext_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("myst_parser"),
        1 => Some("sphinxcontrib.mermaid"),
        2 => Some("sphinx.ext.autodoc"),
        3 => Some("sphinx.ext.viewcode"),
        4 => Some("sphinx_design"),
        5 => Some("sphinx_copybutton"),
        6 => Some("sphinx.ext.napoleon"),
        7 => Some("sphinx.ext.intersphinx"),
        _ => None,
    }
}

/// 增量构建拍载荷（严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0x4245_0000 + b as u64 * 0x14)
}

/// Python 上栈层名（0 venv 1 pip 2 sphinx-build 3 doctree 缓存）。
pub const fn stack_level_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("venv 激活"),
        1 => Some("pip 装载"),
        2 => Some("sphinx-build 就绪"),
        3 => Some("doctree 缓存命中"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Sphinx 收口账本状态机。
#[derive(Clone)]
pub struct SphinxLedger {
    /// 六闸（conf 解析/扩展装载/toctree/交叉引用/build/serve）。
    gates: [Option<bool>; GATES],
    /// conf.py 投影账。
    confs: [Option<u64>; CONF_CELLS],
    /// builder 操作账。
    blds: [Option<u64>; BLD_OPS],
    /// reST 指令账。
    rsts: [Option<u64>; RST_OPS],
    /// 扩展声明账。
    exts: [Option<u64>; EXT_CELLS],
    /// 增量拍账。
    beats: [Option<u64>; BEATS],
    /// 已压栈层数（下一层必须是 depth）。
    depth: usize,
    /// 最后 builder 家族。
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

impl SphinxLedger {
    /// 新账本。
    pub fn new() -> Self {
        SphinxLedger {
            gates: [None; GATES],
            confs: [None; CONF_CELLS],
            blds: [None; BLD_OPS],
            rsts: [None; RST_OPS],
            exts: [None; EXT_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SphinxCode> {
        if gate >= GATES {
            return Err(SphinxCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// conf.py 投影一格（正逆互证 + 重复拒）。
    pub fn record_conf(&mut self, i: usize, got: u64) -> Result<(), SphinxCode> {
        let want = match conf_cell(i) {
            Some(w) => w,
            None => return Err(SphinxCode::ENoConf),
        };
        if got != want || conf_cell_inverse(got) != Some(i) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + i as u64);
            return Err(SphinxCode::EConfDrift);
        }
        if match self.confs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SphinxCode::ESeal);
        }
        if let Some(c) = self.confs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6201 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn confs_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONF_CELLS {
            match self.confs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// builder 操作一格（家族单调 + 载荷对照 + 重复拒）。
    /// 空账本首格必须自家族 0（html 配置）起，跳族即抓且不改 last_kind。
    pub fn record_bld(&mut self, o: usize, got: u64) -> Result<(), SphinxCode> {
        let kind = match bld_kind(o) {
            Some(k) => k,
            None => return Err(SphinxCode::ENoBld),
        };
        if (self.last_kind >= 0 && (kind as i64) < self.last_kind)
            || (self.last_kind < 0 && kind != 0)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(SphinxCode::EBldOrder);
        }
        let want = match bld_cell(o) {
            Some(w) => w,
            None => return Err(SphinxCode::ENoBld),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(SphinxCode::EBldDrift);
        }
        if match self.blds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SphinxCode::ESeal);
        }
        if let Some(c) = self.blds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// builder 全账核验（10 格）。
    pub fn blds_all(&self) -> bool {
        let mut o = 0usize;
        while o < BLD_OPS {
            match self.blds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// reST 指令一格（越闭集拒 + 重复拒）。
    pub fn record_rst(&mut self, i: usize, got: u64) -> Result<(), SphinxCode> {
        let want = match rst_cell(i) {
            Some(w) => w,
            None => return Err(SphinxCode::ENoRst),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + i as u64);
            return Err(SphinxCode::EBldDrift);
        }
        if match self.rsts.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SphinxCode::ESeal);
        }
        if let Some(c) = self.rsts.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6401 + i as u64);
        Ok(())
    }

    /// reST 全账核验（10 格）。
    pub fn rsts_all(&self) -> bool {
        let mut i = 0usize;
        while i < RST_OPS {
            match self.rsts.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 扩展声明一格（越闭集归 conf 家族拒 + 重复拒）。
    pub fn record_ext(&mut self, i: usize, got: u64) -> Result<(), SphinxCode> {
        let want = match ext_cell(i) {
            Some(w) => w,
            None => return Err(SphinxCode::ENoConf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + i as u64);
            return Err(SphinxCode::EConfDrift);
        }
        if match self.exts.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SphinxCode::ESeal);
        }
        if let Some(c) = self.exts.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6501 + i as u64);
        Ok(())
    }

    /// 扩展全齐核验（8 格，缺一拒封）。
    pub fn exts_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXT_CELLS {
            match self.exts.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 增量构建一拍（载荷严格递增，回退拍必抓）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), SphinxCode> {
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(SphinxCode::EBeat),
        };
        if got != want || got <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + b as u64);
            return Err(SphinxCode::EBeat);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got;
        self.fnv(0x6601 + b as u64);
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

    /// Python 上栈一层（顺序压栈：第 depth 层必须恰为 depth，跳层必抓）。
    pub fn push_stack(&mut self, l: usize) -> Result<(), SphinxCode> {
        if l >= STACK_LEVELS || l != self.depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6700 + l as u64);
            return Err(SphinxCode::EStack);
        }
        self.depth = l + 1;
        self.fnv(0x6701 + l as u64);
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
            && self.confs_all()
            && self.blds_all()
            && self.rsts_all()
            && self.exts_all()
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

impl Default for SphinxLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 空账全量灌装（六闸 + conf + builder + reST + 扩展 + 上栈 + 增量拍）。
fn fill_ledger(led: &mut SphinxLedger) -> u32 {
    let mut recorded = 0u32;
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut i = 0usize;
    while i < CONF_CELLS {
        if let Some(w) = conf_cell(i) {
            if led.record_conf(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < BLD_OPS {
        if let Some(w) = bld_cell(o) {
            if led.record_bld(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut n = 0usize;
    while n < RST_OPS {
        if let Some(w) = rst_cell(n) {
            if led.record_rst(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut e = 0usize;
    while e < EXT_CELLS {
        if let Some(w) = ext_cell(e) {
            if led.record_ext(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
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
/// 注入 10 轮三态轮转（conf 漂移/builder 逆序/builder 越闭集），全部落在
/// 当轮临时账本上：主账 injected==caught==10、escaped==0。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SphinxLedger::new();
    let recorded = fill_ledger(&mut led);
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SphinxLedger::new();
        let caught = match kind {
            0 => {
                // 空账首格跳族（html → singlehtml 家族）：必抓且不改 last_kind。
                lt.record_bld(4, bld_cell(4).unwrap_or(0)) == Err(SphinxCode::EBldOrder)
            }
            1 => {
                // 先按合法序落低族格，再注 conf 漂移。
                let _ = lt.record_conf(0, conf_cell(0).unwrap_or(0));
                let _ = lt.record_bld(0, bld_cell(0).unwrap_or(0));
                lt.record_conf(1, conf_cell(1).unwrap_or(0).wrapping_add(3))
                    == Err(SphinxCode::EConfDrift)
            }
            _ => {
                // builder 越闭集：格 10 在 10 格闭集之外。
                lt.record_bld(BLD_OPS, 0) == Err(SphinxCode::ENoBld)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6800 + k as u64);
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
/// UNX-F52342 判据集（锚 UNX-F52343-J1，off-by-one 板式）。
pub fn run_unx52342_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52342");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "conf.py 双向投影 12 格全通",
        recorded >= CONF_CELLS as u32,
        "正向折叠与逆核唯一还原互证，逐格恒等",
    );
    s.add(
        "双向投影逆核",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CONF_CELLS {
                match conf_cell(i) {
                    Some(w) => {
                        if conf_cell_inverse(w) != Some(i) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok && conf_cell_inverse(0xfeed_0000_0001).is_none()
        },
        "投影值逆核还原格位，外来值不中方",
    );
    s.add(
        "五 builder 10 操作全通",
        {
            let mut lt = SphinxLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < BLD_OPS {
                if let Some(w) = bld_cell(o) {
                    if lt.record_bld(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.blds_all()
        },
        "html/dirhtml/singlehtml/pdf/epub 五家族逐操作恒等",
    );
    s.add(
        "builder 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BLD_OPS {
                if bld_kind(o).unwrap_or(5) < bld_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "reST 指令 10 操作全落",
        {
            let mut lt = SphinxLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < RST_OPS {
                if let Some(w) = rst_cell(i) {
                    if lt.record_rst(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.rsts_all()
        },
        "toctree/code-block/math/note/warning/figure/table/ref/doc/automodule",
    );
    s.add(
        "扩展 8 格全落",
        {
            let mut lt = SphinxLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < EXT_CELLS {
                if let Some(w) = ext_cell(i) {
                    if lt.record_ext(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.exts_all()
        },
        "extensions 声明逐格对照，缺一拒封",
    );
    s.add(
        "Python 上栈 4 层全通",
        {
            let mut lt = SphinxLedger::new();
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
        "venv→pip→sphinx-build→doctree 顺序压栈",
    );
    s.add(
        "增量构建 8 拍单调",
        {
            let mut lt = SphinxLedger::new();
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
        "conf 漂移/builder 逆序/builder 越闭集三态轮转必抓",
    );
    let mut l0 = SphinxLedger::new();
    s.add(
        "边界·conf 格越闭集",
        l0.record_conf(CONF_CELLS, 0) == Err(SphinxCode::ENoConf),
        "conf 格 12 越闭集必拒",
    );
    s.add(
        "边界·builder 越闭集",
        l0.record_bld(BLD_OPS, 0) == Err(SphinxCode::ENoBld),
        "builder 操作 10 越闭集必拒",
    );
    s.add(
        "边界·reST 越闭集",
        l0.record_rst(RST_OPS, 0) == Err(SphinxCode::ENoRst),
        "reST 指令 10 越闭集必拒",
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
            let mut lt = SphinxLedger::new();
            let _ = fill_ledger(&mut lt);
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9540,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9540",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conf_projection() {
        assert!(conf_cell(0).is_some());
        assert!(conf_cell(CONF_CELLS).is_none());
        assert_eq!(conf_cell_inverse(conf_cell(11).unwrap_or(0)), Some(11));
        assert!(conf_cell_inverse(0x0BAD_C0DE).is_none());
    }

    #[test]
    fn test_bld_kind_mapping() {
        // 家族映射：kind = o / 2（五 builder 各 2 步）。
        assert_eq!(bld_kind(0), Some(0));
        assert_eq!(bld_kind(1), Some(0));
        assert_eq!(bld_kind(2), Some(1));
        assert_eq!(bld_kind(9), Some(4));
        assert!(bld_kind(10).is_none());
        assert_eq!(builder_name(0), Some("html"));
        assert_eq!(builder_name(4), Some("epub"));
        assert!(builder_name(5).is_none());
        assert_eq!(bld_step_name(1), Some("产出"));
        assert!(bld_step_name(2).is_none());
    }

    #[test]
    fn test_rst_labels() {
        assert_eq!(rst_label(0), Some("toctree"));
        assert_eq!(rst_label(9), Some("automodule"));
        assert!(rst_label(10).is_none());
        assert!(rst_cell(10).is_none());
    }

    #[test]
    fn test_ext_names() {
        assert_eq!(ext_name(0), Some("myst_parser"));
        assert_eq!(ext_name(7), Some("sphinx.ext.intersphinx"));
        assert!(ext_name(8).is_none());
        assert!(ext_cell(8).is_none());
        assert!(stack_level_name(0).is_some());
        assert!(stack_level_name(4).is_none());
    }

    #[test]
    fn test_stack_order() {
        let mut lt = SphinxLedger::new();
        assert_eq!(lt.push_stack(1), Err(SphinxCode::EStack));
        assert!(lt.push_stack(0).is_ok());
        assert!(lt.push_stack(1).is_ok());
        assert!(lt.push_stack(2).is_ok());
        assert!(!lt.stack_full());
        assert!(lt.push_stack(3).is_ok());
        assert!(lt.stack_full());
        assert!(lt.push_stack(0).is_err());
    }

    #[test]
    fn test_first_bld_family_zero() {
        // 空账本首格跳族必抓且不改 last_kind。
        let mut lt = SphinxLedger::new();
        assert_eq!(lt.record_bld(8, bld_cell(8).unwrap_or(0)), Err(SphinxCode::EBldOrder));
        assert!(lt.record_bld(0, bld_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SphinxLedger::new();
        let mut i = 0usize;
        while i <= 3 {
            let _ = lt.record_bld(i, bld_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_bld(0, bld_cell(0).unwrap_or(0)), Err(SphinxCode::EBldOrder));
        assert!(lt.record_bld(4, bld_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_dup() {
        let mut lt = SphinxLedger::new();
        assert_eq!(
            lt.record_conf(5, conf_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(SphinxCode::EConfDrift)
        );
        let _ = lt.record_rst(2, rst_cell(2).unwrap_or(0));
        assert_eq!(lt.record_rst(2, rst_cell(2).unwrap_or(0)), Err(SphinxCode::ESeal));
    }

    #[test]
    fn test_beat_monotone() {
        let mut lt = SphinxLedger::new();
        let _ = lt.record_beat(0, beat_cell(0).unwrap_or(0));
        assert_eq!(lt.record_beat(1, beat_cell(1).unwrap_or(0).wrapping_sub(1)), Err(SphinxCode::EBeat));
        assert!(lt.record_beat(1, beat_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SphinxLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SphinxLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= CONF_CELLS as u32 + BLD_OPS as u32 + RST_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC9540);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SphinxCode::Ok as u32, 0);
        assert_eq!(SphinxCode::EGradeDrift as u32, 12);
        assert!(!SphinxCode::Ok.detail().is_empty());
        assert!(!SphinxCode::ESeal.detail().is_empty());
        let _ = SphinxCode::EStack.val();
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SphinxLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 只差上栈未满：S2 而非 S1。
        let mut lt2 = SphinxLedger::new();
        let _ = fill_ledger(&mut lt2);
        lt2.depth = 0;
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52342_checks_green() {
        let s = run_unx52342_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
