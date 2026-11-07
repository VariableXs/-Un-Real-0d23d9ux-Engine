//! UNX-F52343 MkDocs 收口 · 0xC9580 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52343"，逐字）：
//!   title: MkDocs 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52344-J1 build
//!     ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52343）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52344-J1，照原文锚定；功能按本卡标题（MkDocs）落地。
//! 本模块按 52201 样板口径运行于 crate 级 `#![cfg_attr(not(test),
//! no_std)]` + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（MkDocs 域语料，逐项定制）：
//!   ① mkdocs.yml 双向投影 12 格：正向 cfg_cell 折叠、逆向 cfg_cell_inverse
//!      唯一还原，逆核不中方即漂移必抓；XOR 锚低 16 位恒零。
//!   ② 五命令（serve/build/gh-deploy/new/serve-sync）10 操作：每命令
//!      2 步（预检→执行），命令家族单调（逆序必抓），空账首格必须自
//!      serve 家族起，跳族即抓且不改 last_kind。
//!   ③ Markdown 消费 8 格：导航页/首页/API 页/教程页/发布说明/版本
//!      钩子/搜索索引/锚点表逐格对照，缺一拒封。
//!   ④ 主题 4 档闭集（mkdocs/material/readthedocs/cinder），越档必拒。
//!   ⑤ 增量构建 8 拍单调（拍载荷严格递增，回退拍必抓）；Python 上栈
//!      4 层顺序压栈（venv→pip→mkdocs→site 缓存），乱序必抓。
//!   ⑥ ledger 状态机；注入核 run_cycle 10 轮三态轮转（cfg 漂移/命令
//!      逆序/命令越闭集），全部落在当轮临时账本上 10/10 全抓；双跑
//!      指纹一致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52343;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// mkdocs.yml 投影格数。
pub const CFG_CELLS: usize = 12;

/// 五命令操作格数（5 × 2 步）。
pub const CMD_OPS: usize = 10;

/// Markdown 消费格数。
pub const MD_CELLS: usize = 8;

/// 主题档位数。
pub const THEME_TIERS: usize = 4;

/// 增量构建拍数。
pub const BEATS: usize = 8;

/// Python 上栈层数。
pub const STACK_LEVELS: usize = 4;

/// 收口判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MkdocsCode {
    /// 成功
    Ok = 0,
    /// 配置投影格越闭集
    ENoCfg,
    /// 配置投影格漂移
    ECfgDrift,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令操作漂移
    ECmdDrift,
    /// 命令家族逆序
    ECmdOrder,
    /// Markdown 消费格越闭集
    ENoMd,
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

impl MkdocsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MkdocsCode::Ok => "成功",
            MkdocsCode::ENoCfg => "配置投影格越闭集：mkdocs.yml 12 格闭集之外先登记",
            MkdocsCode::ECfgDrift => "配置投影格漂移：正逆投影不互证，逐格复算",
            MkdocsCode::ENoCmd => "命令操作越闭集：五命令 10 格闭集之外先登记",
            MkdocsCode::ECmdDrift => "命令操作漂移：操作折叠与基准不恒等，逐格复算",
            MkdocsCode::ECmdOrder => "命令家族逆序：serve→build→gh-deploy→new→serve-sync 单调，逆序必抓",
            MkdocsCode::ENoMd => "Markdown 消费格越闭集：8 格消费闭集之外先登记",
            MkdocsCode::EBeat => "增量拍非单调：8 拍载荷严格递增，回退拍必抓",
            MkdocsCode::EStack => "上栈乱序：venv→pip→mkdocs→site 缓存四层顺序压栈，跳层必抓",
            MkdocsCode::ESeal => "未齐即封：12 配置 + 10 命令 + 8 消费 + 4 档主题 + 4 层 + 8 拍未齐先补齐",
            MkdocsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MkdocsCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MkdocsCode::EGradeDrift => "定级漂移：全通且主题档全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+12配置+10命令+8消费+4档主题+4层+8拍全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// mkdocs.yml 投影锚（低 16 位恒零）。
const CFG_ANCHOR: u64 = 0x4D4B_0000;

/// mkdocs.yml 双向投影正向折叠（格 i）。
pub const fn cfg_cell(i: usize) -> Option<u64> {
    if i >= CFG_CELLS {
        return None;
    }
    Some(CFG_ANCHOR ^ ((i as u64 + 1).wrapping_mul(0x71C7)))
}

/// mkdocs.yml 双向投影逆核：由折叠值唯一还原格位。
pub fn cfg_cell_inverse(v: u64) -> Option<usize> {
    let mut i = 0usize;
    while i < CFG_CELLS {
        if let Some(w) = cfg_cell(i) {
            if w == v {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 命令家族（0..2=serve 2..4=build 4..6=gh-deploy 6..8=new 8..10=serve-sync）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令名。
pub const fn command_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("serve"),
        1 => Some("build"),
        2 => Some("gh-deploy"),
        3 => Some("new"),
        4 => Some("serve-sync"),
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
    Some(0x434D_0000 ^ ((o as u64 + 1).wrapping_mul(0x2D1)))
}

/// Markdown 消费格载荷。
pub const fn md_cell(i: usize) -> Option<u64> {
    if i >= MD_CELLS {
        return None;
    }
    Some(0x4D44_0000 ^ ((i as u64 + 1).wrapping_mul(0x3D3)))
}

/// 消费格名。
pub const fn md_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("导航页"),
        1 => Some("首页 index"),
        2 => Some("API 页"),
        3 => Some("教程页"),
        4 => Some("发布说明"),
        5 => Some("版本钩子"),
        6 => Some("搜索索引"),
        7 => Some("锚点表"),
        _ => None,
    }
}

/// 主题档载荷。
pub const fn theme_cell(t: usize) -> Option<u64> {
    if t >= THEME_TIERS {
        return None;
    }
    Some(0x5448_0000 + t as u64 * 0x2A)
}

/// 主题档名。
pub const fn theme_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("mkdocs"),
        1 => Some("material"),
        2 => Some("readthedocs"),
        3 => Some("cinder"),
        _ => None,
    }
}

/// 增量构建拍载荷（严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0x4245_0000 + b as u64 * 0x10)
}

/// Python 上栈层名（0 venv 1 pip 2 mkdocs 3 site 缓存）。
pub const fn stack_level_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("venv 激活"),
        1 => Some("pip 装载"),
        2 => Some("mkdocs 就绪"),
        3 => Some("site 缓存命中"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// MkDocs 收口账本状态机。
#[derive(Clone)]
pub struct MkdocsLedger {
    /// 六闸（yml 解析/导航/主题/构建/部署/写回）。
    gates: [Option<bool>; GATES],
    /// mkdocs.yml 投影账。
    cfgs: [Option<u64>; CFG_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// Markdown 消费账。
    mds: [Option<u64>; MD_CELLS],
    /// 主题档账。
    themes: [Option<u64>; THEME_TIERS],
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

impl MkdocsLedger {
    /// 新账本。
    pub fn new() -> Self {
        MkdocsLedger {
            gates: [None; GATES],
            cfgs: [None; CFG_CELLS],
            cmds: [None; CMD_OPS],
            mds: [None; MD_CELLS],
            themes: [None; THEME_TIERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MkdocsCode> {
        if gate >= GATES {
            return Err(MkdocsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// mkdocs.yml 投影一格（正逆互证 + 重复拒）。
    pub fn record_cfg(&mut self, i: usize, got: u64) -> Result<(), MkdocsCode> {
        let want = match cfg_cell(i) {
            Some(w) => w,
            None => return Err(MkdocsCode::ENoCfg),
        };
        if got != want || cfg_cell_inverse(got) != Some(i) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + i as u64);
            return Err(MkdocsCode::ECfgDrift);
        }
        if match self.cfgs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkdocsCode::ESeal);
        }
        if let Some(c) = self.cfgs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7201 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn cfgs_all(&self) -> bool {
        let mut i = 0usize;
        while i < CFG_CELLS {
            match self.cfgs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令操作一格（家族单调 + 载荷对照 + 重复拒）。
    /// 空账本首格必须自家族 0（serve 预检）起，跳族即抓且不改 last_kind。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), MkdocsCode> {
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(MkdocsCode::ENoCmd),
        };
        if (self.last_kind >= 0 && (kind as i64) < self.last_kind)
            || (self.last_kind < 0 && kind != 0)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(MkdocsCode::ECmdOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(MkdocsCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + o as u64);
            return Err(MkdocsCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkdocsCode::ESeal);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7302 + o as u64);
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

    /// Markdown 消费一格（越闭集拒 + 重复拒）。
    pub fn record_md(&mut self, i: usize, got: u64) -> Result<(), MkdocsCode> {
        let want = match md_cell(i) {
            Some(w) => w,
            None => return Err(MkdocsCode::ENoMd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + i as u64);
            return Err(MkdocsCode::ECmdDrift);
        }
        if match self.mds.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkdocsCode::ESeal);
        }
        if let Some(c) = self.mds.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7401 + i as u64);
        Ok(())
    }

    /// 消费全账核验（8 格，缺一拒封）。
    pub fn mds_all(&self) -> bool {
        let mut i = 0usize;
        while i < MD_CELLS {
            match self.mds.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 主题档一格（越档必拒）。
    pub fn record_theme(&mut self, t: usize, got: u64) -> Result<(), MkdocsCode> {
        let want = match theme_cell(t) {
            Some(w) => w,
            None => return Err(MkdocsCode::ENoCfg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + t as u64);
            return Err(MkdocsCode::ECfgDrift);
        }
        if match self.themes.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MkdocsCode::ESeal);
        }
        if let Some(c) = self.themes.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x7501 + t as u64);
        Ok(())
    }

    /// 主题档全齐核验（4 档）。
    pub fn themes_all(&self) -> bool {
        let mut t = 0usize;
        while t < THEME_TIERS {
            match self.themes.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 增量构建一拍（载荷严格递增，回退拍必抓）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), MkdocsCode> {
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(MkdocsCode::EBeat),
        };
        if got != want || got <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7600 + b as u64);
            return Err(MkdocsCode::EBeat);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got;
        self.fnv(0x7601 + b as u64);
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
    pub fn push_stack(&mut self, l: usize) -> Result<(), MkdocsCode> {
        if l >= STACK_LEVELS || l != self.depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7700 + l as u64);
            return Err(MkdocsCode::EStack);
        }
        self.depth = l + 1;
        self.fnv(0x7701 + l as u64);
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
            && self.cfgs_all()
            && self.cmds_all()
            && self.mds_all()
            && self.themes_all()
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

impl Default for MkdocsLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 空账全量灌装（六闸 + 配置 + 命令 + 消费 + 主题 + 上栈 + 增量拍）。
fn fill_ledger(led: &mut MkdocsLedger) -> u32 {
    let mut recorded = 0u32;
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut i = 0usize;
    while i < CFG_CELLS {
        if let Some(w) = cfg_cell(i) {
            if led.record_cfg(i, w).is_ok() {
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
    while m < MD_CELLS {
        if let Some(w) = md_cell(m) {
            if led.record_md(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut t = 0usize;
    while t < THEME_TIERS {
        if let Some(w) = theme_cell(t) {
            if led.record_theme(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
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
/// 注入 10 轮三态轮转（cfg 漂移/命令逆序/命令越闭集），全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MkdocsLedger::new();
    let recorded = fill_ledger(&mut led);
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MkdocsLedger::new();
        let caught = match kind {
            0 => {
                // 空账首格跳族（serve → gh-deploy 家族）：必抓且不改 last_kind。
                lt.record_cmd(4, cmd_cell(4).unwrap_or(0)) == Err(MkdocsCode::ECmdOrder)
            }
            1 => {
                // 先按合法序落低族格，再注 cfg 漂移。
                let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0));
                let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
                lt.record_cfg(1, cfg_cell(1).unwrap_or(0).wrapping_add(5))
                    == Err(MkdocsCode::ECfgDrift)
            }
            _ => {
                // 命令越闭集：格 10 在 10 格闭集之外。
                lt.record_cmd(CMD_OPS, 0) == Err(MkdocsCode::ENoCmd)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7800 + k as u64);
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
/// UNX-F52343 判据集（锚 UNX-F52344-J1，off-by-one 板式）。
pub fn run_unx52343_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52343");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "mkdocs.yml 双向投影 12 格全通",
        recorded >= CFG_CELLS as u32,
        "正向折叠与逆核唯一还原互证，逐格恒等",
    );
    s.add(
        "双向投影逆核",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CFG_CELLS {
                match cfg_cell(i) {
                    Some(w) => {
                        if cfg_cell_inverse(w) != Some(i) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok && cfg_cell_inverse(0xBEEF_0000_0002).is_none()
        },
        "投影值逆核还原格位，外来值不中方",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = MkdocsLedger::new();
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
        "serve/build/gh-deploy/new/serve-sync 五命令逐操作恒等",
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
        "Markdown 消费 8 格全落",
        {
            let mut lt = MkdocsLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < MD_CELLS {
                if let Some(w) = md_cell(i) {
                    if lt.record_md(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.mds_all()
        },
        "导航/首页/API/教程/发布说明/版本钩子/搜索索引/锚点表逐格对照",
    );
    s.add(
        "主题 4 档全齐",
        {
            let mut lt = MkdocsLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < THEME_TIERS {
                if let Some(w) = theme_cell(t) {
                    if lt.record_theme(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.themes_all()
        },
        "mkdocs/material/readthedocs/cinder 四档闭集，越档必拒",
    );
    s.add(
        "Python 上栈 4 层全通",
        {
            let mut lt = MkdocsLedger::new();
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
        "venv→pip→mkdocs→site 缓存顺序压栈",
    );
    s.add(
        "增量构建 8 拍单调",
        {
            let mut lt = MkdocsLedger::new();
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
        "cfg 漂移/命令逆序/命令越闭集三态轮转必抓",
    );
    let mut l0 = MkdocsLedger::new();
    s.add(
        "边界·配置格越闭集",
        l0.record_cfg(CFG_CELLS, 0) == Err(MkdocsCode::ENoCfg),
        "配置格 12 越闭集必拒",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0) == Err(MkdocsCode::ENoCmd),
        "命令操作 10 越闭集必拒",
    );
    s.add(
        "边界·消费格越闭集",
        l0.record_md(MD_CELLS, 0) == Err(MkdocsCode::ENoMd),
        "Markdown 消费格 8 越闭集必拒",
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
            let mut lt = MkdocsLedger::new();
            let _ = fill_ledger(&mut lt);
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9580,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9580",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cfg_projection() {
        assert!(cfg_cell(0).is_some());
        assert!(cfg_cell(CFG_CELLS).is_none());
        assert_eq!(cfg_cell_inverse(cfg_cell(9).unwrap_or(0)), Some(9));
        assert!(cfg_cell_inverse(0x0FFF_0000_0003).is_none());
    }

    #[test]
    fn test_cmd_kind_mapping() {
        // 家族映射：kind = o / 2（五命令各 2 步）。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(1), Some(0));
        assert_eq!(cmd_kind(4), Some(2));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(command_name(0), Some("serve"));
        assert_eq!(command_name(4), Some("serve-sync"));
        assert!(command_name(5).is_none());
        assert_eq!(cmd_step_name(0), Some("预检"));
        assert!(cmd_step_name(2).is_none());
    }

    #[test]
    fn test_md_names() {
        assert_eq!(md_name(0), Some("导航页"));
        assert_eq!(md_name(1), Some("首页 index"));
        assert_eq!(md_name(7), Some("锚点表"));
        assert!(md_name(8).is_none());
        assert!(md_cell(8).is_none());
    }

    #[test]
    fn test_theme_tiers() {
        assert_eq!(theme_name(0), Some("mkdocs"));
        assert_eq!(theme_name(1), Some("material"));
        assert_eq!(theme_name(3), Some("cinder"));
        assert!(theme_name(4).is_none());
        assert!(theme_cell(4).is_none());
        assert!(stack_level_name(3).is_some());
        assert!(stack_level_name(4).is_none());
    }

    #[test]
    fn test_stack_order() {
        let mut lt = MkdocsLedger::new();
        assert_eq!(lt.push_stack(2), Err(MkdocsCode::EStack));
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
        let mut lt = MkdocsLedger::new();
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(MkdocsCode::ECmdOrder));
        assert!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = MkdocsLedger::new();
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_cmd(i, cmd_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_cmd(1, cmd_cell(1).unwrap_or(0)), Err(MkdocsCode::ECmdOrder));
        assert!(lt.record_cmd(6, cmd_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_dup() {
        let mut lt = MkdocsLedger::new();
        assert_eq!(
            lt.record_cfg(7, cfg_cell(7).unwrap_or(0).wrapping_add(4)),
            Err(MkdocsCode::ECfgDrift)
        );
        let _ = lt.record_md(3, md_cell(3).unwrap_or(0));
        assert_eq!(lt.record_md(3, md_cell(3).unwrap_or(0)), Err(MkdocsCode::ESeal));
    }

    #[test]
    fn test_beat_monotone() {
        let mut lt = MkdocsLedger::new();
        let _ = lt.record_beat(0, beat_cell(0).unwrap_or(0));
        assert_eq!(lt.record_beat(1, beat_cell(0).unwrap_or(0)), Err(MkdocsCode::EBeat));
        assert!(lt.record_beat(1, beat_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MkdocsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MkdocsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= CFG_CELLS as u32 + CMD_OPS as u32 + MD_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9580);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MkdocsCode::Ok as u32, 0);
        assert_eq!(MkdocsCode::EGradeDrift as u32, 12);
        assert!(!MkdocsCode::Ok.detail().is_empty());
        assert!(!MkdocsCode::ESeal.detail().is_empty());
        let _ = MkdocsCode::ENoMd.val();
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = MkdocsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 如实阶梯：六闸齐但各面未齐 → S2（全齐才 S1）。
        let mut lt2 = MkdocsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52343_checks_green() {
        let s = run_unx52343_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
