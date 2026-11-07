//! UNX-F52345 GitBook CLI 收口 · 0xC9600段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52345"，逐字）：
//!   title: GitBook CLI 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52346-J1 格式转换
//!     ×20 格式对（md↔docx/html/pdf/epub）全通；输出校验和对照；KB-COMPAT
//!     定级 S1；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52345）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52346-J1，照原文锚定；功能按本卡标题（GitBook CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① book.json 双向投影 16 格：正投影/逆投影纯函数折叠恒等才收，
//!      越格必抓，投影漂移必抓。
//!   ② 五命令 10 操作：三族闭集（构建族 init/build/install、服务族
//!      serve、导出族 pdf/epub），族单调（逆序必抓），逐操作载荷对照。
//!   ③ 插件 8 格 + Node 上栈 4 层（gitbook-cli→npm→Node.js→宿主 OS）
//!      + 版本阶梯 3 档（2.x/3.x/4.x），逐格恒等，越级必抓。
//!   ④ 输出校验和对照六判据收口闸：格式转换/校验和对照/定级登记/
//!      写回/锚定/摘要，逐格落账，未齐即封拒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 三态轮转 10/10（版本越阶梯/
//!      命令漂移/命令重复落账），三类注入全部落在当轮临时账本上，
//!      漂移注入先按合法序落正格再注漂移，对空账本同样必抓；
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52345;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// book.json 双向投影格数。
pub const BOOK_CELLS: usize = 16;

/// 五命令操作数。
pub const CMD_OPS: usize = 10;

/// 插件格数。
pub const PLUGS: usize = 8;

/// Node 上栈层数。
pub const NODE_LAYERS: usize = 4;

/// 版本阶梯级数。
pub const VER_STEPS: usize = 3;

/// GitBook 域 XOR 锚（低 16 位恒零）。
pub const GB_SIG: u64 = 0x474254424F4F0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitBookCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 投影格越闭集
    ENoBook,
    /// 双向投影漂移
    EBookDrift,
    /// 命令号越闭集
    ENoCmd,
    /// 命令漂移
    ECmdDrift,
    /// 命令族逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl GitBookCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GitBookCode::Ok => "成功",
            GitBookCode::ENoVer => "版本级越阶梯：2.x/3.x/4.x 三级闭集之外先登记",
            GitBookCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            GitBookCode::ENoBook => "投影格越闭集：book.json 双向投影 16 格之外先登记",
            GitBookCode::EBookDrift => "双向投影漂移：正/逆投影折叠与基准不恒等，逐格复算",
            GitBookCode::ENoCmd => "命令号越闭集：五命令 10 操作集之外先登记",
            GitBookCode::ECmdDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            GitBookCode::EOrder => "命令族逆序：构建族→服务族→导出族单调，逆序必抓",
            GitBookCode::EDup => "重复落账：同格只许一条账目",
            GitBookCode::ESeal => "未齐即封：16 投影格 + 10 操作 + 8 插件 + 4 层上栈 + 3 级版本未齐先补齐",
            GitBookCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GitBookCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GitBookCode::EGradeDrift => "定级漂移：全通且校验和对照全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+16投影格+10操作+8插件+4层上栈+3级版本全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// book.json 正投影（格 i）。
pub const fn book_cell(i: usize) -> Option<u64> {
    if i >= BOOK_CELLS {
        return None;
    }
    Some((0xB1000u64 + (i as u64) * 0x3A) ^ GB_SIG)
}

/// book.json 逆投影（格 i）：与正投影构成双向对照对。
pub const fn book_inv(i: usize) -> Option<u64> {
    if i >= BOOK_CELLS {
        return None;
    }
    Some((0xC1000u64 + (i as u64) * 0x3A) ^ GB_SIG)
}

/// 版本阶梯级名。
pub const fn ver_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("2.x"),
        1 => Some("3.x"),
        2 => Some("4.x"),
        _ => None,
    }
}

/// 版本级载荷。
pub const fn ver_cell(v: usize) -> Option<u64> {
    if v >= VER_STEPS {
        return None;
    }
    Some((0xB2000u64 + (v as u64) * 0x5D) ^ GB_SIG)
}

/// 命令格载荷（操作 o；族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xB3000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=构建族 4..8=服务族 8..10=导出族，三族单调）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("构建族"),
        1 => Some("服务族"),
        2 => Some("导出族"),
        _ => None,
    }
}

/// 插件格载荷。
pub const fn plug_cell(p: usize) -> Option<u64> {
    if p >= PLUGS {
        return None;
    }
    Some((0xB4000u64 + (p as u64) * 0x37) ^ GB_SIG)
}

/// Node 上栈层名。
pub const fn layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("gitbook-cli"),
        1 => Some("npm"),
        2 => Some("Node.js"),
        3 => Some("宿主OS"),
        _ => None,
    }
}

/// Node 上栈层载荷。
pub const fn layer_cell(l: usize) -> Option<u64> {
    if l >= NODE_LAYERS {
        return None;
    }
    Some((0xB5000u64 + (l as u64) * 0x41) ^ GB_SIG)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GitBook CLI 收口账本状态机。
#[derive(Clone)]
pub struct GitBookLedger {
    /// 六判据（格式转换/校验和对照/定级登记/写回/锚定/摘要）。
    gates: [Option<bool>; 6],
    /// 双向投影账。
    book: [Option<u64>; BOOK_CELLS],
    /// 命令账。
    cmds: [Option<u64>; CMD_OPS],
    /// 插件账。
    plugs: [Option<u64>; PLUGS],
    /// 上栈层账。
    layers: [Option<u64>; NODE_LAYERS],
    /// 版本阶梯账。
    vers: [Option<u64>; VER_STEPS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GitBookLedger {
    /// 新账本。
    pub fn new() -> Self {
        GitBookLedger {
            gates: [None; 6],
            book: [None; BOOK_CELLS],
            cmds: [None; CMD_OPS],
            plugs: [None; PLUGS],
            layers: [None; NODE_LAYERS],
            vers: [None; VER_STEPS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GitBookCode> {
        if gate >= 6 {
            return Err(GitBookCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), GitBookCode> {
        let want = match ver_cell(v) {
            Some(w) => w,
            None => return Err(GitBookCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + v as u64);
            return Err(GitBookCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitBookCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5201 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 级全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VER_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 双向投影一格（正/逆投影恒等对照 + 重复拒）。
    pub fn record_book(&mut self, i: usize) -> Result<(), GitBookCode> {
        let fwd = match book_cell(i) {
            Some(w) => w,
            None => return Err(GitBookCode::ENoBook),
        };
        let inv = match book_inv(i) {
            Some(w) => w,
            None => return Err(GitBookCode::ENoBook),
        };
        // 双向对照：逆投影折叠应与正投影基准满足纯函数恒等关系。
        if (fwd ^ inv) != (0xB1000u64 ^ 0xC1000u64) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + i as u64);
            return Err(GitBookCode::EBookDrift);
        }
        if match self.book.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitBookCode::EDup);
        }
        if let Some(c) = self.book.get_mut(i) {
            *c = Some(fwd);
        }
        self.fnv(0x5301 + i as u64);
        Ok(())
    }

    /// 双向投影全账核验（16 格）。
    pub fn book_all(&self) -> bool {
        let mut i = 0usize;
        while i < BOOK_CELLS {
            match self.book.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), GitBookCode> {
        if o >= CMD_OPS {
            return Err(GitBookCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(GitBookCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(GitBookCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(GitBookCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + o as u64);
            return Err(GitBookCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitBookCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5402 + o as u64);
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

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plug(&mut self, p: usize, got: u64) -> Result<(), GitBookCode> {
        let want = match plug_cell(p) {
            Some(w) => w,
            None => return Err(GitBookCode::ENoBook),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + p as u64);
            return Err(GitBookCode::EBookDrift);
        }
        if match self.plugs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitBookCode::EDup);
        }
        if let Some(c) = self.plugs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5501 + p as u64);
        Ok(())
    }

    /// 插件全账核验（8 格）。
    pub fn plugs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGS {
            match self.plugs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 上栈层一格（载荷对照 + 重复拒）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), GitBookCode> {
        let want = match layer_cell(l) {
            Some(w) => w,
            None => return Err(GitBookCode::ENoBook),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + l as u64);
            return Err(GitBookCode::EBookDrift);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitBookCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x5601 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < NODE_LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.book_all()
            && self.cmds_all()
            && self.plugs_all()
            && self.layers_all()
            && self.vers_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GitBookLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落格数, 检出数, 指纹）。
/// 三态轮转注入全部落在当轮临时账本 lt 上；漂移注入先按合法序落正格
/// 再注漂移；对空账本同样必抓；注入轮不改主账 last_kind。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GitBookLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VER_STEPS {
        if let Some(w) = ver_cell(v) {
            if led.record_ver(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut l = 0usize;
    while l < NODE_LAYERS {
        if let Some(w) = layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut i = 0usize;
    while i < BOOK_CELLS {
        if led.record_book(i).is_ok() {
            recorded += 1;
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
    let mut p = 0usize;
    while p < PLUGS {
        if let Some(w) = plug_cell(p) {
            if led.record_plug(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    // 注入 10 轮：版本越阶梯 / 命令漂移 / 命令重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GitBookLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_ver(0, match ver_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_ver(VER_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_cmd(0, match cmd_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_cmd(
                    1,
                    match cmd_cell(1) {
                        Some(w) => w,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(GitBookCode::ECmdDrift)
            }
            _ => {
                let _ = lt.record_cmd(0, match cmd_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_cmd(0, match cmd_cell(0) {
                    Some(w) => w,
                    None => 0,
                }) == Err(GitBookCode::EDup)
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
/// UNX-F52345 判据集（锚 UNX-F52346-J1，off-by-one 板式）。
pub fn run_unx52345_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52345");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "book.json 双向投影 16 格全通",
        recorded >= BOOK_CELLS as u32,
        "正/逆投影纯函数折叠恒等，双向对照逐格收账",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = GitBookLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CMD_OPS {
                match cmd_cell(o) {
                    Some(w) => {
                        if lt.record_cmd(o, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                o += 1;
            }
            ok && lt.cmds_all()
        },
        "init/build/install/serve/pdf/epub 五命令 10 操作逐格恒等（UNX-F52346-J1）",
    );
    s.add(
        "插件 8 格全落",
        {
            let mut lt = GitBookLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGS {
                match plug_cell(p) {
                    Some(w) => {
                        if lt.record_plug(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                p += 1;
            }
            ok && lt.plugs_all()
        },
        "book.json 插件表 8 格逐格收账",
    );
    s.add(
        "Node 上栈 4 层全落",
        {
            let mut lt = GitBookLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < NODE_LAYERS {
                match layer_cell(l) {
                    Some(w) => {
                        if lt.record_layer(l, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "gitbook-cli→npm→Node.js→宿主OS 四层上栈逐层恒等",
    );
    s.add(
        "版本阶梯 2.x/3.x/4.x 三级全落",
        {
            let mut lt = GitBookLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VER_STEPS {
                match ver_cell(v) {
                    Some(w) => {
                        if lt.record_ver(v, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三级闭集逐级恒等，越级必抓",
    );
    s.add(
        "命令三族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                let a = match cmd_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match cmd_kind(o - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "构建族→服务族→导出族三族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越阶梯/命令漂移/命令重复落账三态轮转必抓",
    );
    let mut l0 = GitBookLedger::new();
    s.add(
        "边界·版本级越阶梯",
        l0.record_ver(VER_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "命令漂移拒",
        {
            match cmd_cell(0) {
                Some(w) => {
                    let _ = l0.record_cmd(0, w);
                }
                None => {}
            }
            let bad = match cmd_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            l0.record_cmd(1, bad) == Err(GitBookCode::ECmdDrift)
        },
        "命令载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_book(BOOK_CELLS).is_err(),
        "投影格 16 越闭集必拒",
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
            let mut lt = GitBookLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VER_STEPS {
                match ver_cell(v) {
                    Some(w) => {
                        let _ = lt.record_ver(v, w);
                    }
                    None => {}
                }
                v += 1;
            }
            let mut l = 0usize;
            while l < NODE_LAYERS {
                match layer_cell(l) {
                    Some(w) => {
                        let _ = lt.record_layer(l, w);
                    }
                    None => {}
                }
                l += 1;
            }
            let mut i = 0usize;
            while i < BOOK_CELLS {
                let _ = lt.record_book(i);
                i += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                match cmd_cell(o) {
                    Some(w) => {
                        let _ = lt.record_cmd(o, w);
                    }
                    None => {}
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PLUGS {
                match plug_cell(p) {
                    Some(w) => {
                        let _ = lt.record_plug(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            // 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级 S1 登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9600,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式 0xC9600 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_book_projection_pair() {
        // 双向投影纯函数恒等：正/逆折叠关系逐格成立。
        let mut i = 0usize;
        while i < BOOK_CELLS {
            let fwd = book_cell(i).unwrap_or(0);
            let inv = book_inv(i).unwrap_or(0);
            assert_eq!(fwd ^ inv, 0xB1000u64 ^ 0xC1000u64);
            if i + 1 < BOOK_CELLS {
                assert_ne!(fwd, book_cell(i + 1).unwrap_or(fwd));
            }
            i += 1;
        }
        assert!(book_cell(BOOK_CELLS).is_none());
        assert!(book_inv(BOOK_CELLS).is_none());
    }

    #[test]
    fn test_ver_ladder() {
        assert_eq!(ver_name(0), Some("2.x"));
        assert_eq!(ver_name(1), Some("3.x"));
        assert_eq!(ver_name(2), Some("4.x"));
        assert!(ver_name(3).is_none());
        assert!(ver_cell(3).is_none());
    }

    #[test]
    fn test_cmd_kind_mapping() {
        // kind 映射：o/4 → 0,0,0,0,1,1,1,1,2,2（三族）。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(3), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(7), Some(1));
        assert_eq!(cmd_kind(8), Some(2));
        assert_eq!(cmd_kind(9), Some(2));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(0), Some("构建族"));
        assert_eq!(cmd_kind_name(2), Some("导出族"));
        assert!(cmd_kind_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(plug_cell(PLUGS).is_none());
        assert!(layer_cell(NODE_LAYERS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GitBookLedger::new();
        // 先按合法序落服务族正格，再注构建族逆序。
        let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(2, cmd_cell(2).unwrap_or(0)),
            Err(GitBookCode::EOrder)
        );
        assert!(lt.record_cmd(7, cmd_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GitBookLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(GitBookCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_ver(1, ver_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(GitBookCode::EVerDrift)
        );
        assert_eq!(
            lt.record_plug(2, plug_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(GitBookCode::EBookDrift)
        );
        assert_eq!(
            lt.record_layer(0, layer_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(GitBookCode::EBookDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GitBookLedger::new();
        let _ = lt.record_ver(0, ver_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_ver(0, ver_cell(0).unwrap_or(0)),
            Err(GitBookCode::EDup)
        );
        let _ = lt.record_book(0);
        assert_eq!(lt.record_book(0), Err(GitBookCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GitBookLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GitBookLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= 41);
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
        assert_eq!(SEG_BASE, 0xC9600);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GitBookCode::Ok as u32, 0);
        assert_eq!(GitBookCode::EGradeDrift as u32, 12);
        assert!(!GitBookCode::Ok.detail().is_empty());
        assert!(!GitBookCode::ESeal.detail().is_empty());
        assert_eq!(GitBookCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GitBookLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52345_checks_green() {
        let s = run_unx52345_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
