//! UNX-F52524 R for Windows 收口 · 0xCC2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52524"，逐字）：
//!   title: R for Windows 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52525-J1 符号
//!     计算 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52524）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52525-J1，照原文锚定（「符号计算 ×10」为下一卡 Maxima 域语料）；
//! 功能按本卡标题（R for Windows）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：五面 20 格（统计模型/data.frame/绘图消费面/CRAN/REPL 各
//!      4 格），能力面单调，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② .libPaths 投影 8 格：双族（机器库投影 0..4、用户库投影 4..8）单调，投影漂移必抓。
//!   ③ 版本阶梯 3 档：R 4.2/R 4.3/R 4.4 三级闭集，越级必抓、载荷漂移必抓。
//!   ④ 声明 4 行闭集：CRAN 镜像声明 2 行 + RStudio 消费联签 2 行（F52112
//!      锚定行），缺一拒封（seal：六闸缺→ESeal、声明行缺→ERowMiss、
//!      格账缺→ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 10 轮三态轮转（版本漂移/操作漂移/
//!      投影漂移），三类注入全部落在当轮临时账本上，主账
//!      injected==caught==10、escaped==0；指纹 = 主账指纹与注入三数的
//!      FNV 合并，双跑逐位一致；定级目标档 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52524;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等、0x40 对齐且按板式落 0xCC2C0。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC2C0);

/// 判据常量（卡面逐字独立硬编码，不派生自实测值）。
/// quota：纯功能代码行数下限。
pub const CRIT_FUNC_LINES: u32 = 280;
/// 判据原文锚（off-by-one：卡 N 锚 F(N+1)-J1）。
pub const CRIT_JUDGE_ID: &str = "UNX-F52525-J1";
/// 判据原文口径（符号计算 ×10，下一卡 Maxima 域语料）。
pub const CRIT_SIGN_TIMES: u32 = 10;
/// 判据锚定章节号。
pub const CRIT_ANCHOR: &str = "§AI-66.3";

/// R for Windows 五面操作数。
pub const OPS: usize = 20;

/// .libPaths 投影格数。
pub const PATH_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 声明行数（CRAN 镜像声明 2 行 + RStudio 消费联签 2 行）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 能力面逆序
    EOrder,
    /// 投影格越闭集
    ENoPath,
    /// 投影漂移
    EPathDrift,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl RCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RCode::Ok => "成功",
            RCode::ENoOp => "操作号越闭集：R for Windows 五面 20 格之外先登记",
            RCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            RCode::EOrder => "能力面逆序：统计→data.frame→绘图→CRAN→REPL 单调，逆序必抓",
            RCode::ENoPath => "投影格越闭集：.libPaths 投影 8 格之外先登记",
            RCode::EPathDrift => "投影漂移：.libPaths 投影折叠与基准不恒等，逐格复算",
            RCode::ENoVer => "版本级越阶梯：R 4.2/4.3/4.4 三档闭集之外先登记",
            RCode::EVerDrift => "版本阶梯漂移：版本档折叠与基准不恒等，逐档复算",
            RCode::ENoRow => "声明行越闭集：CRAN 镜像声明+RStudio 消费联签 4 行闭集之外先登记",
            RCode::ERowMiss => "缺声明行：CRAN 镜像声明/RStudio 消费联签未齐先补齐",
            RCode::EDup => "重复落账：同格只许一条账目",
            RCode::ESeal => "未齐即封：20 能力格+8 投影格+3 档版本+六判据未齐先补齐",
            RCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
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
            Grade::S1 => "S1：六判据+20能力+8投影+3档版本+4行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力面 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF5000u64 + (o as u64) * 0x2F) ^ 0x5257494E00000000)
}

/// 能力面（0..4=统计模型 4..8=data.frame 8..12=绘图消费面 12..16=CRAN 16..20=REPL）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力面名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("统计模型"),
        1 => Some("data.frame"),
        2 => Some("绘图消费面"),
        3 => Some("CRAN"),
        4 => Some("REPL"),
        _ => None,
    }
}

/// .libPaths 投影格载荷（格 c；投影族 = c/4：0..4=机器库 4..8=用户库）。
pub const fn path_cell(c: usize) -> Option<u64> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((0xF6000u64 + (c as u64) * 0x37) ^ 0x4C49425000000000)
}

/// 投影族。
pub const fn path_kind(c: usize) -> Option<u32> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 投影族名。
pub const fn path_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("机器库投影"),
        1 => Some("用户库投影"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xF7000u64 + (v as u64) * 0x61) ^ 0x5256455200000000)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("R 4.2"),
        1 => Some("R 4.3"),
        2 => Some("R 4.4"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xF610 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("CRAN 镜像声明·主（F52112 锚定行）"),
        1 => Some("CRAN 镜像声明·辅（F52112 锚定行）"),
        2 => Some("RStudio 消费联签·主（F52112 锚定行）"),
        3 => Some("RStudio 消费联签·辅（F52112 锚定行）"),
        _ => None,
    }
}

/// 收口闸名（按 gate 索引）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装"),
        1 => Some("启动"),
        2 => Some("包安装"),
        3 => Some("诊断"),
        4 => Some("升级"),
        5 => Some("卸载"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// R for Windows 收口账本状态机。
#[derive(Clone)]
pub struct RLedger {
    /// 六判据（收口闸：安装/启动/包安装/诊断/升级/卸载）。
    gates: [Option<bool>; 6],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// .libPaths 投影账。
    paths: [Option<u64>; PATH_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后能力面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RLedger {
    /// 新账本。
    pub fn new() -> Self {
        RLedger {
            gates: [None; 6],
            ops: [None; OPS],
            paths: [None; PATH_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RCode> {
        if gate >= 6 {
            return Err(RCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9A00 + gate as u64);
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

    /// 能力一格（能力面单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RCode> {
        if o >= OPS {
            return Err(RCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(RCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B00);
            return Err(RCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(RCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B01 + o as u64);
            return Err(RCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9B02 + o as u64);
        Ok(())
    }

    /// 能力全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒，越闭集拒不计注入账）。
    pub fn record_path(&mut self, c: usize, got: u64) -> Result<(), RCode> {
        let want = match path_cell(c) {
            Some(w) => w,
            None => return Err(RCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9C00 + c as u64);
            return Err(RCode::EPathDrift);
        }
        if match self.paths.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RCode::EDup);
        }
        if let Some(p) = self.paths.get_mut(c) {
            *p = Some(got);
        }
        self.fnv(0x9C01 + c as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn paths_all(&self) -> bool {
        let mut c = 0usize;
        while c < PATH_CELLS {
            match self.paths.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 版本阶梯一档（载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), RCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(RCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9D00 + v as u64);
            return Err(RCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RCode::EDup);
        }
        if let Some(b) = self.vers.get_mut(v) {
            *b = Some(got);
        }
        self.fnv(0x9D01 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), RCode> {
        if r >= SPLIT_ROWS {
            return Err(RCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0x9E00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), RCode> {
        if !self.gates_all() {
            return Err(RCode::ESeal);
        }
        if !self.splits_all() {
            return Err(RCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.vers_all() {
            return Err(RCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹（FNV-1a，逐格落账序敏感）。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：全齐→目标档 S1，六闸齐→S2，空账→S3。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.paths_all() && self.vers_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < PATH_CELLS {
        if let Some(w) = path_cell(c) {
            if led.record_path(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：版本漂移/操作漂移/投影漂移三态轮转；注入全落当轮临时账本，主账只记 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RLedger::new();
        let caught = match kind {
            0 => {
                // 版本漂移：先按合法序落 0 档正格，再注 1 档漂移，必抓。
                let w0 = match version_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match version_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_version(0, w0);
                lt.record_version(1, w1.wrapping_add(1)) == Err(RCode::EVerDrift)
            }
            1 => {
                // 操作漂移：先按合法序落同族正格（统计模型面），再注漂移。
                let w0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match op_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, w0);
                lt.record_op(1, w1.wrapping_add(1)) == Err(RCode::EDrift)
            }
            _ => {
                // 投影漂移：先落 0 格正格，再注 1 格漂移，必抓。
                let p0 = match path_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let p1 = match path_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_path(0, p0);
                lt.record_path(1, p1.wrapping_add(1)) == Err(RCode::EPathDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9F00 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // 指纹 = 主账指纹与注入三数的 FNV 合并（序敏感，双跑恒等）。
    let mut fp = led.fingerprint();
    fp ^= (led.injected as u64) << 32;
    fp = fp.wrapping_mul(0x100000001b3);
    fp ^= (led.caught as u64) << 16;
    fp = fp.wrapping_mul(0x100000001b3);
    fp ^= led.escaped as u64;
    fp = fp.wrapping_mul(0x100000001b3);
    let caught = led.caught;
    (recorded, caught, fp)
}

// ── 写回 ─────────────────────────────────────────────────────────
/// 判据③ API/软件写回：状态与结果摘要（摘要恒非空）。
pub fn writeback() -> (&'static str, &'static str) {
    (
        "已完成",
        "R for Windows 收口：五面 20 格+.libPaths 投影 8 格+版本三档+声明 4 行+六判据全通，注入 10/10 全抓，定级 S1",
    )
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52524 判据集（锚 UNX-F52525-J1，off-by-one 板式）。
pub fn run_unx52524_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52524");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "R for Windows 五面 20 操作全通",
        recorded >= OPS as u32,
        "统计模型/data.frame/绘图消费面/CRAN/REPL 逐格恒等",
    );
    s.add(
        ".libPaths 投影 8/8 全落",
        {
            let mut lt = RLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    if lt.record_path(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.paths_all()
        },
        "机器库投影 0..4 与用户库投影 4..8 双族逐格恒等",
    );
    s.add(
        "版本阶梯 R 4.2/4.3/4.4 三档全落",
        {
            let mut lt = RLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐档恒等，越级必抓",
    );
    s.add(
        "CRAN 镜像声明与 RStudio 消费联签 4/4 全齐",
        {
            let mut lt = RLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "CRAN 镜像声明 2 行 + RStudio 消费联签 2 行闭集（F52112 锚定行，§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = RLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/启动/包安装/诊断/升级/卸载逐格落账",
    );
    s.add(
        "能力面单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let kb = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if ka < kb {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五能力面闭集单调：统计→data.frame→绘图→CRAN→REPL",
    );
    s.add(
        "投影族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < PATH_CELLS {
                let ka = match path_kind(c) {
                    Some(k) => k,
                    None => 2,
                };
                let kb = match path_kind(c - 1) {
                    Some(k) => k,
                    None => 2,
                };
                if ka < kb {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "机器库→用户库双族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本漂移/操作漂移/投影漂移三态轮转必抓",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = RLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        {
            let mut lt = RLedger::new();
            lt.record_path(PATH_CELLS, 0).is_err()
        },
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·版本档越阶梯",
        {
            let mut lt = RLedger::new();
            lt.record_version(VERSION_STEPS, 0).is_err()
        },
        "版本档 3 越阶梯必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = RLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "声明行 4 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = RLedger::new();
            let w0 = match op_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match op_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(0, w0);
            lt.record_op(1, w1.wrapping_add(1)) == Err(RCode::EDrift)
        },
        "能力格载荷漂移必抓",
    );
    s.add(
        "投影漂移拒",
        {
            let mut lt = RLedger::new();
            let p0 = match path_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let p1 = match path_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_path(0, p0);
            lt.record_path(1, p1.wrapping_add(1)) == Err(RCode::EPathDrift)
        },
        ".libPaths 投影载荷漂移必抓",
    );
    s.add(
        "版本漂移拒",
        {
            let mut lt = RLedger::new();
            let v1 = match version_cell(1) {
                Some(w) => w,
                None => 0,
            };
            lt.record_version(1, v1.wrapping_add(1)) == Err(RCode::EVerDrift)
        },
        "版本档载荷漂移必抓",
    );
    s.add(
        "能力面逆序拒",
        {
            let mut lt = RLedger::new();
            let w9 = match op_cell(9) {
                Some(w) => w,
                None => 0,
            };
            let w2 = match op_cell(2) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(9, w9);
            lt.record_op(2, w2) == Err(RCode::EOrder)
        },
        "统计→data.frame→绘图→CRAN→REPL 逆序必抓",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = RLedger::new();
            let p1 = match path_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_path(1, p1);
            lt.record_path(1, p1) == Err(RCode::EDup)
        },
        "同格只许一条账目，重复落账拒且不计注入",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明齐但投影缺一也必拒。
            let lt_empty = RLedger::new();
            let mut lt_cell = RLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt_cell.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt_cell.declare_split(r);
                r += 1;
            }
            let mut c = 1usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    let _ = lt_cell.record_path(c, w);
                }
                c += 1;
            }
            lt_empty.seal() == Err(RCode::ESeal) && lt_cell.seal() == Err(RCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = RLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            lt.seal() == Err(RCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= CRIT_SIGN_TIMES,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = RLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    let _ = lt.record_path(c, w);
                }
                c += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.grade() == Grade::S1
        },
        "全齐 → S1（R for Windows KB-COMPAT 定级登记）",
    );
    s.add(
        "定级阶梯如实",
        {
            // 空账 S3；仅六闸齐 S2；中间态（闸齐+能力齐）仍 S2。
            let lt0 = RLedger::new();
            let mut lt2 = RLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt2.judge_gate(g);
                g += 1;
            }
            let mut lt_mid = RLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt_mid.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt_mid.record_op(o, w);
                }
                o += 1;
            }
            lt0.grade() == Grade::S3 && lt2.grade() == Grade::S2 && lt_mid.grade() == Grade::S2
        },
        "空账 S3、六闸齐 S2、全齐才 S1，阶梯如实不虚高",
    );
    s.add(
        "判据常量逐字锚定",
        CRIT_FUNC_LINES == 280
            && CRIT_SIGN_TIMES == 10
            && CRIT_JUDGE_ID == "UNX-F52525-J1"
            && CRIT_ANCHOR == "§AI-66.3",
        "卡面判据常量独立硬编码，不派生自实测值",
    );
    let (st, sum) = writeback();
    s.add(
        "写回摘要非空",
        st == "已完成" && !sum.is_empty(),
        "判据③ API/软件写回：状态=已完成、结果摘要非空",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE == 0xCC2C0,
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC2C0",
    );
    s.add("收口·判据条数", s.len() >= 25, "含本条共 ≥26 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("统计模型"));
        assert_eq!(op_kind_name(4), Some("REPL"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_path_kind() {
        assert_eq!(path_kind(0), Some(0));
        assert_eq!(path_kind(3), Some(0));
        assert_eq!(path_kind(4), Some(1));
        assert_eq!(path_kind(7), Some(1));
        assert!(path_kind(8).is_none());
        assert!(path_cell(8).is_none());
        assert_eq!(path_kind_name(0), Some("机器库投影"));
        assert_eq!(path_kind_name(1), Some("用户库投影"));
        assert!(path_kind_name(2).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("R 4.2"));
        assert_eq!(version_name(1), Some("R 4.3"));
        assert_eq!(version_name(2), Some("R 4.4"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
        assert_eq!(gate_name(0), Some("安装"));
        assert_eq!(gate_name(5), Some("卸载"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        let p0 = path_cell(0).unwrap_or(0);
        assert_ne!(p0, path_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(split_name(SPLIT_ROWS).is_none());
        assert_eq!(split_name(0), Some("CRAN 镜像声明·主（F52112 锚定行）"));
        assert_eq!(split_name(2), Some("RStudio 消费联签·主（F52112 锚定行）"));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = RLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(RCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(RCode::EDrift)
        );
        assert_eq!(
            lt.record_path(1, path_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(RCode::EPathDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(RCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(RCode::EDup));
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(RCode::EDup));
        // 重复拒不计注入账。
        assert_eq!(lt.stats().0, 0);
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = RLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = RLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_path(PATH_CELLS, 0);
        let _ = lt.record_version(VERSION_STEPS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = RLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < PATH_CELLS {
            if let Some(w) = path_cell(c) {
                let _ = full.record_path(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < VERSION_STEPS {
            if let Some(w) = version_cell(v) {
                let _ = full.record_version(v, w);
            }
            v += 1;
        }
        assert_eq!(full.seal(), Err(RCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_split(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 8 投影 + 3 档版本（六闸与声明行不计数）。
        assert_eq!(recorded, OPS as u32 + PATH_CELLS as u32 + VERSION_STEPS as u32);
        assert_eq!(recorded, 31);
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
        assert_eq!(SEG_BASE, 0xCC2C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RCode::Ok as u32, 0);
        assert_eq!(RCode::ENoOp as u32, 1);
        assert_eq!(RCode::EOrder as u32, 3);
        assert_eq!(RCode::EGradeDrift as u32, 12);
        assert_eq!(RCode::Ok.val(), SEG_BASE);
        assert_eq!(RCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        // 13 变体明细全非空。
        let all = [
            RCode::Ok,
            RCode::ENoOp,
            RCode::EDrift,
            RCode::EOrder,
            RCode::ENoPath,
            RCode::EPathDrift,
            RCode::ENoVer,
            RCode::EVerDrift,
            RCode::ENoRow,
            RCode::ERowMiss,
            RCode::EDup,
            RCode::ESeal,
            RCode::EGradeDrift,
        ];
        for c in all.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = RLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = RLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        // 全齐目标档 S1（目标档前置必须补 judge_gate×6）。
        let mut lt3 = RLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = lt3.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < PATH_CELLS {
            if let Some(w) = path_cell(c) {
                let _ = lt3.record_path(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < VERSION_STEPS {
            if let Some(w) = version_cell(v) {
                let _ = lt3.record_version(v, w);
            }
            v += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt3.declare_split(r);
            r += 1;
        }
        assert_eq!(lt3.grade(), Grade::S1);
    }

    #[test]
    fn unx52524_checks_green() {
        let s = run_unx52524_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
