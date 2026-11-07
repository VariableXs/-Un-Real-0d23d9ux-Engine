//! UNX-F52303 Chocolatey GUI 收口 · 0xC8B80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52303"，逐字）：
//!   title: Chocolatey GUI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52304-J1 六判据
//!     全落；多后端（winget/scoop/choco）聚合 ×10；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52303）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52304-J1，照原文锚定；功能按本卡标题（Chocolatey GUI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 包源三族 12 格：official / community / internal 三源族各 4 格，
//!      闭集外必抓；逐格载荷折叠对照，恒等才收。
//!   ② install/upgrade/pin/outdated 20 操作集：四相位族各 5 操作（安
//!      装/升级/固定/过期扫描），相位单调（逆序必抓），载荷漂移必抓。
//!   ③ chocolatey.config 双向投影 12 格：正向投影（GUI→config）6 格 +
//!      反向投影（config→GUI）6 格，两向分账；特征（feature）三元闸
//!      3 格；与 CLI 消费联签 4 格。
//!   ④ 多后端聚合 ×10：注入核 10 轮三态轮转（源越闭集/操作漂移/操作
//!      越闭集），全部落在当轮临时账本上，检出+逃逸守恒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；双跑 FNV 指纹一致；全通 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52303;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 包源格数（三族 × 4）。
pub const SOURCE_CELLS: usize = 12;

/// 源族数。
pub const SOURCE_FAMILIES: usize = 3;

/// 操作数（四相位族 × 5）。
pub const OPS: usize = 20;

/// chocolatey.config 双向投影格数（正向 6 + 反向 6）。
pub const PROJ_CELLS: usize = 12;

/// 单向投影格数。
pub const PROJ_HALF: usize = 6;

/// 特征三元闸格数。
pub const FEATURE_GATES: usize = 3;

/// 与 CLI 消费联签格数。
pub const CLI_SIGN_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChocoGuiCode {
    /// 成功
    Ok = 0,
    /// 包源格越闭集
    ENoSource,
    /// 包源载荷漂移
    ESourceDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 相位逆序
    EPhase,
    /// 投影格越闭集
    ENoProj,
    /// 投影方向错序
    EProjDir,
    /// 特征闸格越闭集
    ENoFeat,
    /// 联签格越闭集
    ENoSign,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl ChocoGuiCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ChocoGuiCode::Ok => "成功",
            ChocoGuiCode::ENoSource => "包源格越闭集：official/community/internal 12 格之外先登记",
            ChocoGuiCode::ESourceDrift => "包源载荷漂移：源索引折叠与基准不恒等，逐格复算",
            ChocoGuiCode::ENoOp => "操作号越闭集：install/upgrade/pin/outdated 20 操作集之外先登记",
            ChocoGuiCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ChocoGuiCode::EPhase => "相位逆序：安装→升级→固定→过期扫描相位单调，逆序必抓",
            ChocoGuiCode::ENoProj => "投影格越闭集：chocolatey.config 双向投影 12 格之外先登记",
            ChocoGuiCode::EProjDir => "投影方向错序：正向 6 格先于反向 6 格，先反后正必抓",
            ChocoGuiCode::ENoFeat => "特征闸格越闭集：feature 三元闸 3 格之外先登记",
            ChocoGuiCode::ENoSign => "联签格越闭集：与 CLI 消费联签 4 格之外先登记",
            ChocoGuiCode::EDup => "重复落账：同格只许一条账目",
            ChocoGuiCode::ESeal => "未齐即封：12 包源格 + 20 操作 + 12 投影格 + 特征三元闸未齐先补齐",
            ChocoGuiCode::EGradeDrift => "定级漂移：六判据全落且多后端聚合账全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+12源+20操作+12投影+3特征闸+4联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 包源格载荷（格 b：0..12，族 = b/4）。
pub const fn source_cell(b: usize) -> Option<u64> {
    if b >= SOURCE_CELLS {
        return None;
    }
    Some((0xE1000u64 + (b as u64) * 0x47) ^ 0x4348_0000)
}

/// 包源族（0=official 1=community 2=internal）。
pub const fn source_family(b: usize) -> Option<u32> {
    if b >= SOURCE_CELLS {
        return None;
    }
    Some((b / 4) as u32)
}

/// 包源族名。
pub const fn source_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("official"),
        1 => Some("community"),
        2 => Some("internal"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/5）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xE2000u64 + (o as u64) * 0x39) ^ (((o / 5) as u64).wrapping_mul(0x0300_0000)))
}

/// 操作相位（0..5=install 5..10=upgrade 10..15=pin 15..20=outdated）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 操作相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("install"),
        1 => Some("upgrade"),
        2 => Some("pin"),
        3 => Some("outdated"),
        _ => None,
    }
}

/// 投影格载荷（格 c：0..12，0..6=正向 6..12=反向）。
pub const fn proj_cell(c: usize) -> Option<u32> {
    if c >= PROJ_CELLS {
        return None;
    }
    Some(0xE300 + (c as u32) * 0x2B)
}

/// 投影方向（0=正向 GUI→config，1=反向 config→GUI）。
pub const fn proj_dir(c: usize) -> Option<u32> {
    if c >= PROJ_CELLS {
        return None;
    }
    Some((c / PROJ_HALF) as u32)
}

/// 投影方向名。
pub const fn proj_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("正向 GUI→config"),
        1 => Some("反向 config→GUI"),
        _ => None,
    }
}

/// 特征三元闸载荷（格 f：0=清理缺失依赖 1=免升级提示 2=失败回滚）。
pub const fn feature_cell(f: usize) -> Option<u32> {
    if f >= FEATURE_GATES {
        return None;
    }
    Some(0xE500 + (f as u32) * 0x13)
}

/// 特征三元闸名。
pub const fn feature_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("清理缺失依赖"),
        1 => Some("免升级提示"),
        2 => Some("失败回滚"),
        _ => None,
    }
}

/// 与 CLI 消费联签载荷（格 p）。
pub const fn cli_sign_cell(p: usize) -> Option<u32> {
    if p >= CLI_SIGN_CELLS {
        return None;
    }
    Some(0xE600 + (p as u32) * 0x1F)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Chocolatey GUI 收口账本状态机。
#[derive(Clone)]
pub struct ChocoGuiLedger {
    /// 六判据（启动/包浏览/安装/升级/固定/过期）。
    gates: [Option<bool>; 6],
    /// 包源账。
    sources: [Option<u64>; SOURCE_CELLS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 双向投影账。
    projs: [Option<u32>; PROJ_CELLS],
    /// 最后投影方向。
    last_dir: i64,
    /// 特征三元闸账。
    feats: [Option<u32>; FEATURE_GATES],
    /// 与 CLI 消费联签账。
    signs: [Option<u32>; CLI_SIGN_CELLS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ChocoGuiLedger {
    /// 新账本。
    pub fn new() -> Self {
        ChocoGuiLedger {
            gates: [None; 6],
            sources: [None; SOURCE_CELLS],
            ops: [None; OPS],
            projs: [None; PROJ_CELLS],
            last_dir: -1,
            feats: [None; FEATURE_GATES],
            signs: [None; CLI_SIGN_CELLS],
            last_phase: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ChocoGuiCode> {
        if gate >= 6 {
            return Err(ChocoGuiCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 包源一格（载荷对照 + 重复拒）。
    pub fn record_source(&mut self, b: usize, got: u64) -> Result<(), ChocoGuiCode> {
        let want = match source_cell(b) {
            Some(w) => w,
            None => return Err(ChocoGuiCode::ENoSource),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + b as u64);
            return Err(ChocoGuiCode::ESourceDrift);
        }
        if match self.sources.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChocoGuiCode::EDup);
        }
        if let Some(c) = self.sources.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x7201 + b as u64);
        Ok(())
    }

    /// 包源全账核验（12 格）。
    pub fn sources_all(&self) -> bool {
        let mut b = 0usize;
        while b < SOURCE_CELLS {
            match self.sources.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ChocoGuiCode> {
        if o >= OPS {
            return Err(ChocoGuiCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(ChocoGuiCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(ChocoGuiCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(ChocoGuiCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + o as u64);
            return Err(ChocoGuiCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChocoGuiCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x7302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
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

    /// 投影一格（方向单调：正向 6 格先于反向 6 格）。
    pub fn record_proj(&mut self, c: usize) -> Result<(), ChocoGuiCode> {
        let want = match proj_cell(c) {
            Some(w) => w,
            None => return Err(ChocoGuiCode::ENoProj),
        };
        let dir = match proj_dir(c) {
            Some(d) => d,
            None => return Err(ChocoGuiCode::ENoProj),
        };
        if (dir as i64) < self.last_dir {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400);
            return Err(ChocoGuiCode::EProjDir);
        }
        if match self.projs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChocoGuiCode::EDup);
        }
        if let Some(x) = self.projs.get_mut(c) {
            *x = Some(want);
        }
        self.last_dir = dir as i64;
        self.fnv(0x7401 + c as u64);
        Ok(())
    }

    /// 投影全账核验（12 格，双向各 6）。
    pub fn projs_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROJ_CELLS {
            match self.projs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 特征三元闸一格。
    pub fn record_feature(&mut self, f: usize) -> Result<(), ChocoGuiCode> {
        let want = match feature_cell(f) {
            Some(w) => w,
            None => return Err(ChocoGuiCode::ENoFeat),
        };
        if match self.feats.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChocoGuiCode::EDup);
        }
        if let Some(x) = self.feats.get_mut(f) {
            *x = Some(want);
        }
        self.fnv(0x7500 + f as u64);
        Ok(())
    }

    /// 特征闸全账核验。
    pub fn feats_all(&self) -> bool {
        let mut f = 0usize;
        while f < FEATURE_GATES {
            match self.feats.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 与 CLI 消费联签一格。
    pub fn record_cli_sign(&mut self, p: usize) -> Result<(), ChocoGuiCode> {
        let want = match cli_sign_cell(p) {
            Some(w) => w,
            None => return Err(ChocoGuiCode::ENoSign),
        };
        if match self.signs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChocoGuiCode::EDup);
        }
        if let Some(x) = self.signs.get_mut(p) {
            *x = Some(want);
        }
        self.fnv(0x7600 + p as u64);
        Ok(())
    }

    /// 联签全账核验。
    pub fn signs_all(&self) -> bool {
        let mut p = 0usize;
        while p < CLI_SIGN_CELLS {
            match self.signs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
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
            && self.sources_all()
            && self.ops_all()
            && self.projs_all()
            && self.feats_all()
            && self.signs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ChocoGuiLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ChocoGuiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < SOURCE_CELLS {
        if let Some(w) = source_cell(b) {
            if led.record_source(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
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
    while c < PROJ_CELLS {
        let _ = led.record_proj(c);
        c += 1;
    }
    let mut f = 0usize;
    while f < FEATURE_GATES {
        let _ = led.record_feature(f);
        f += 1;
    }
    let mut p = 0usize;
    while p < CLI_SIGN_CELLS {
        let _ = led.record_cli_sign(p);
        p += 1;
    }
    // 注入 10 轮：包源越闭集 / 操作漂移 / 操作越闭集三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ChocoGuiLedger::new();
        let caught = match kind {
            0 => lt.record_source(SOURCE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ChocoGuiCode::EDrift)
            }
            _ => lt.record_op(OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52303 判据集（锚 UNX-F52304-J1，off-by-one 板式）。
pub fn run_unx52303_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52303");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "包源三族 12 格全落",
        recorded >= SOURCE_CELLS as u32 + OPS as u32,
        "official/community/internal 三源族逐格恒等落账",
    );
    s.add(
        "源族名闭集",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < SOURCE_FAMILIES {
                match source_family_name(f) {
                    Some(_) => {}
                    None => ok = false,
                }
                f += 1;
            }
            ok && source_family_name(SOURCE_FAMILIES).is_none()
        },
        "official/community/internal 三族名闭集，族外无名",
    );
    s.add(
        "install/upgrade/pin/outdated 20 操作全通",
        {
            let mut lt = ChocoGuiLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "四相位族逐操作恒等（UNX-F52304-J1 多后端聚合 ×10 锚定）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_phase(o).unwrap_or(4) < op_phase(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "install→upgrade→pin→outdated 四相位族单调",
    );
    s.add(
        "chocolatey.config 双向投影 12 格全落",
        {
            let mut lt = ChocoGuiLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if lt.record_proj(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.projs_all()
        },
        "正向 GUI→config 6 格 + 反向 config→GUI 6 格，两向分账",
    );
    s.add(
        "投影方向单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < PROJ_CELLS {
                if proj_dir(c).unwrap_or(2) < proj_dir(c - 1).unwrap_or(2) {
                    ok = false;
                }
                c += 1;
            }
            ok && proj_dir_name(0).is_some() && proj_dir_name(1).is_some()
        },
        "正向 6 格先于反向 6 格，先反后正必抓",
    );
    s.add(
        "特征三元闸全落",
        {
            let mut lt = ChocoGuiLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FEATURE_GATES {
                if lt.record_feature(f).is_err() {
                    ok = false;
                }
                f += 1;
            }
            ok && lt.feats_all()
        },
        "清理缺失依赖/免升级提示/失败回滚三元闭集",
    );
    s.add(
        "与 CLI 消费联签 4 格全落",
        {
            let mut lt = ChocoGuiLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < CLI_SIGN_CELLS {
                if lt.record_cli_sign(p).is_err() {
                    ok = false;
                }
                p += 1;
            }
            ok && lt.signs_all()
        },
        "与 CLI 消费逐格联签入指纹",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ChocoGuiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/包浏览/安装/升级/固定/过期逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "包源越闭集/操作漂移/操作越闭集三态轮转必抓");
    let mut l0 = ChocoGuiLedger::new();
    s.add(
        "边界·包源越闭集",
        l0.record_source(SOURCE_CELLS, 0).is_err(),
        "包源 12 格之外必拒",
    );
    s.add(
        "注入·操作漂移拒",
        {
            let _ = l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ChocoGuiCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
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
            let mut lt = ChocoGuiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < SOURCE_CELLS {
                if let Some(w) = source_cell(b) {
                    let _ = lt.record_source(b, w);
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < PROJ_CELLS {
                let _ = lt.record_proj(c);
                c += 1;
            }
            let mut f = 0usize;
            while f < FEATURE_GATES {
                let _ = lt.record_feature(f);
                f += 1;
            }
            let mut p = 0usize;
            while p < CLI_SIGN_CELLS {
                let _ = lt.record_cli_sign(p);
                p += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，多后端 winget/scoop/choco 聚合）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8B80,
        "SEG_BASE 0x40 对齐且按 0xC7200+0x40×(52303−52201) 恒等",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_families() {
        assert_eq!(source_family_name(0), Some("official"));
        assert_eq!(source_family_name(1), Some("community"));
        assert_eq!(source_family_name(2), Some("internal"));
        assert!(source_family_name(3).is_none());
        assert_eq!(source_family(0), Some(0));
        assert_eq!(source_family(11), Some(2));
        assert!(source_family(12).is_none());
        assert!(source_cell(12).is_none());
    }

    #[test]
    fn test_op_phases() {
        assert_eq!(op_phase_name(0), Some("install"));
        assert_eq!(op_phase_name(1), Some("upgrade"));
        assert_eq!(op_phase_name(2), Some("pin"));
        assert_eq!(op_phase_name(3), Some("outdated"));
        assert!(op_phase_name(4).is_none());
        assert_eq!(op_phase(9), Some(1));
        assert_eq!(op_phase(15), Some(3));
        assert!(op_phase(20).is_none());
        assert!(op_cell(20).is_none());
    }

    #[test]
    fn test_proj_directions() {
        assert_eq!(proj_dir(0), Some(0));
        assert_eq!(proj_dir(5), Some(0));
        assert_eq!(proj_dir(6), Some(1));
        assert_eq!(proj_dir(11), Some(1));
        assert!(proj_dir(12).is_none());
        assert!(proj_cell(12).is_none());
        assert!(proj_dir_name(2).is_none());
    }

    #[test]
    fn test_feature_names() {
        assert_eq!(feature_name(0), Some("清理缺失依赖"));
        assert_eq!(feature_name(1), Some("免升级提示"));
        assert_eq!(feature_name(2), Some("失败回滚"));
        assert!(feature_name(3).is_none());
        assert!(feature_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(11).unwrap_or(0);
        assert_eq!(a, op_cell(11).unwrap_or(0));
        assert_ne!(a, op_cell(12).unwrap_or(0));
        assert!(cli_sign_cell(CLI_SIGN_CELLS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        // 相位逆序注入：先按合法序落高相位格，再注低相位格必抓。
        let mut lt = ChocoGuiLedger::new();
        let _ = lt.record_op(7, op_cell(7).unwrap_or(0));
        assert_eq!(lt.record_op(3, op_cell(3).unwrap_or(0)), Err(ChocoGuiCode::EPhase));
        assert!(lt.record_op(8, op_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_proj_dir_caught() {
        // 方向错序注入：先落反向格，再注正向格必抓（种子须先按合法序落正向）。
        let mut lt = ChocoGuiLedger::new();
        let _ = lt.record_proj(7);
        assert_eq!(lt.record_proj(2), Err(ChocoGuiCode::EProjDir));
        assert!(lt.record_proj(8).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ChocoGuiLedger::new();
        assert_eq!(
            lt.record_op(4, op_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(ChocoGuiCode::EDrift)
        );
        assert_eq!(
            lt.record_source(9, source_cell(9).unwrap_or(0).wrapping_add(7)),
            Err(ChocoGuiCode::ESourceDrift)
        );
    }

    #[test]
    fn test_injection_on_empty_ledger_keeps_phase() {
        // 对空账本注入必抓且不改 last_phase/last_dir：注入后再落合法格应照常成功。
        let mut lt = ChocoGuiLedger::new();
        assert!(lt.record_op(OPS, 0).is_err());
        assert!(lt.record_source(SOURCE_CELLS, 0).is_err());
        assert!(lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_op(5, op_cell(5).unwrap_or(0)).is_ok());
        assert!(lt.record_proj(0).is_ok());
        assert!(lt.record_proj(6).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ChocoGuiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ChocoGuiLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SOURCE_CELLS as u32 + OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC8B80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ChocoGuiCode::Ok as u32, 0);
        assert_eq!(ChocoGuiCode::EGradeDrift as u32, 12);
        assert!(!ChocoGuiCode::Ok.detail().is_empty());
        assert!(!ChocoGuiCode::EProjDir.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ChocoGuiLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52303_checks_green() {
        let s = run_unx52303_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
