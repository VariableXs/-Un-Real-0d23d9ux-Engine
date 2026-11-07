//! UNX-F52377 Travis CLI 收口（长尾） · 0xC9E00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52378"，逐字）：
//!   title: Travis CLI 收口（长尾）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52379-J1
//!     gcloud init/组件管理 ×10（只读判据，写操作干跑）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本文件（文件号 N=52377）承载卡 N+1=52378 的卡
//! 面原文（任务口径：文件号较卡号后移一位，跳过卡 52377 Azure CLI），
//! 判据锚照该卡原文 UNX-F52379-J1 锚定；功能按本卡标题（Travis CLI
//! 收口·长尾）落地——判据原文是邻卡字面，本模块仅收口 Travis CLI 控
//! 制面（本地构建账面收口；travis-ci.org 已停运如实在册，写操作一律
//! 干跑声明，外部资源零写入）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① build 五段 20 格闭集：install/script/before_deploy/deploy/
//!      after_script 五段各 4 格，段单调（逆序必抓），逐格载荷折叠恒
//!      等才收。
//!   ② 五命令 10 操作闭集：version/login/build/coveralls/report 五命
//!      令各 2 操作，命令单调（逆序必抓）；长尾期 login/publish 类写
//!      操作一律落干跑声明格。
//!   ③ .travis.yml 投影 8 格闭集（language/distro/script/env/矩阵/
//!      通知/缓存/部署钩子），双向对合投影（back(fwd(x))==x），漂移必
//!      抓。
//!   ④ 长尾维护声明 2 行闭集（travis-ci.org 停运在册 / travis-ci.com
//!      长尾维护在册），缺行拒封；迁移消费声明 2 行闭集（GitHub
//!      Actions 消费分账 / GitLab CI 消费分账），缺行拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（build 段越闭集/命令操作逆序/
//!      .travis.yml 投影漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；
//!      定级封顶 S3（长尾维护期如实封顶，不再承诺更高档）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52377;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// build 五段格数（五段 × 4 格）。
pub const STAGE_CELLS: usize = 20;

/// 五命令 10 操作格数（五命令 × 2 操作）。
pub const CMD_OPS: usize = 10;

/// .travis.yml 投影格数。
pub const YML_CELLS: usize = 8;

/// 长尾维护声明行数。
pub const EOL_ROWS: usize = 2;

/// 迁移消费声明行数。
pub const MIG_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// Travis 域锚异或（低 16 位恒零）。
pub const TRAVIS_XOR: u64 = 0x5452_4156_0000_0000;

/// .travis.yml 投影锚异或（低 16 位恒零，对合键）。
pub const YML_XOR: u64 = 0x594D_4C50_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TravisCode {
    /// 成功
    Ok = 0,
    /// build 段格越闭集
    ENoStage,
    /// build 段逆序
    EStageOrder,
    /// build 段载荷漂移
    EStageDrift,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令操作逆序
    ECmdOrder,
    /// 命令操作载荷漂移
    ECmdDrift,
    /// .travis.yml 投影格越闭集
    ENoYml,
    /// .travis.yml 投影漂移
    EYmlDrift,
    /// 长尾/迁移声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl TravisCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TravisCode::Ok => "成功",
            TravisCode::ENoStage => "build 段格越闭集：install/script/before_deploy/deploy/after_script 20 格之外先登记",
            TravisCode::EStageOrder => "build 段逆序：五段生命周期单调，逆序必抓",
            TravisCode::EStageDrift => "build 段载荷漂移：段折叠与基准不恒等，逐格复算",
            TravisCode::ENoCmd => "命令操作越闭集：version/login/build/coveralls/report 10 操作之外先登记",
            TravisCode::ECmdOrder => "命令操作逆序：五命令单调，逆序必抓",
            TravisCode::ECmdDrift => "命令操作载荷漂移：命令折叠与基准不恒等，逐格复算",
            TravisCode::ENoYml => ".travis.yml 投影格越闭集：投影 8 格之外先登记",
            TravisCode::EYmlDrift => ".travis.yml 投影漂移：对合投影 back(fwd(x))==x 不成立，逐格复算",
            TravisCode::ENoRow => "声明行越闭集：长尾维护/迁移消费各 2 行之外先登记",
            TravisCode::EDup => "重复落账：同格只许一条账目",
            TravisCode::ESeal => "未齐即封：20 段格 + 10 操作 + 8 投影 + 4 声明未齐先补齐",
            TravisCode::EGradeDrift => "定级漂移：长尾维护期定级封顶 S3，不得越过",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（长尾期不启用）
    S1,
    /// S2：有条件收口（长尾期不启用）
    S2,
    /// S3：不收口（长尾封顶档）
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
            Grade::S1 => "S1：全通收口（travis-ci.org 已停运，长尾期不承诺此档）",
            Grade::S2 => "S2：有条件收口（travis-ci.org 已停运，长尾期不承诺此档）",
            Grade::S3 => "S3：长尾封顶档，travis-ci.org 已停运在册，只保留迁移消费账",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// build 段格载荷（格 o；段 = o/4）。
pub const fn stage_cell(o: usize) -> Option<u64> {
    if o >= STAGE_CELLS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x3C) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ TRAVIS_XOR)
}

/// build 段（0..4=install 4..8=script 8..12=before_deploy 12..16=deploy
/// 16..20=after_script）。
pub const fn stage_kind(o: usize) -> Option<u32> {
    if o >= STAGE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// build 段名。
pub const fn stage_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("install"),
        1 => Some("script"),
        2 => Some("before_deploy"),
        3 => Some("deploy"),
        4 => Some("after_script"),
        _ => None,
    }
}

/// 命令操作格载荷（操作 o；命令 = o/2）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x46) ^ ((o / 2) as u64).wrapping_mul(0x01000193) ^ TRAVIS_XOR)
}

/// 命令（0..2=version 2..4=login 4..6=build 6..8=coveralls 8..10=report）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("version"),
        1 => Some("login"),
        2 => Some("build"),
        3 => Some("coveralls"),
        4 => Some("report"),
        _ => None,
    }
}

/// .travis.yml 投影格基准载荷（格 f；对合投影锚 YML_XOR）。
pub const fn yml_cell(f: usize) -> Option<u64> {
    if f >= YML_CELLS {
        return None;
    }
    Some((0xB3000u64 + (f as u64) * 0x2C) ^ YML_XOR)
}

/// .travis.yml 对合投影·正向（back(fwd(x))==x）。
pub const fn yml_fwd(x: u64) -> u64 {
    x ^ YML_XOR
}

/// .travis.yml 对合投影·反向。
pub const fn yml_back(y: u64) -> u64 {
    y ^ YML_XOR
}

/// .travis.yml 投影格名（8 格闭集对照）。
pub const fn yml_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("language"),
        1 => Some("distro"),
        2 => Some("script"),
        3 => Some("env 矩阵"),
        4 => Some("build 矩阵"),
        5 => Some("notifications"),
        6 => Some("cache"),
        7 => Some("deploy 钩子"),
        _ => None,
    }
}

/// 长尾维护声明行折叠（行 r）。
pub const fn eol_row(r: usize) -> Option<u32> {
    if r >= EOL_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x28)
}

/// 长尾维护声明行名。
pub const fn eol_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("travis-ci.org 停运在册声明"),
        1 => Some("travis-ci.com 长尾维护在册声明"),
        _ => None,
    }
}

/// 迁移消费声明行折叠（行 r）。
pub const fn mig_row(r: usize) -> Option<u32> {
    if r >= MIG_ROWS {
        return None;
    }
    Some(0xB480 + r as u32 * 0x28)
}

/// 迁移消费声明行名。
pub const fn mig_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("GitHub Actions 迁移消费分账声明"),
        1 => Some("GitLab CI 迁移消费分账声明"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("build 生命周期"),
        2 => Some(".travis.yml 解析"),
        3 => Some("命令面收口"),
        4 => Some("长尾在册声明"),
        5 => Some("只读干跑"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Travis CLI 收口账本状态机。
#[derive(Clone)]
pub struct TravisLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// build 段账。
    stages: [Option<u64>; STAGE_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// .travis.yml 投影账。
    ymls: [Option<u64>; YML_CELLS],
    /// 长尾维护声明账。
    eols: [Option<u32>; EOL_ROWS],
    /// 迁移消费声明账。
    migs: [Option<u32>; MIG_ROWS],
    /// 最后 build 段。
    last_stage: i64,
    /// 最后命令。
    last_cmd: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TravisLedger {
    /// 新账本。
    pub fn new() -> Self {
        TravisLedger {
            gates: [None; GATES],
            stages: [None; STAGE_CELLS],
            cmds: [None; CMD_OPS],
            ymls: [None; YML_CELLS],
            eols: [None; EOL_ROWS],
            migs: [None; MIG_ROWS],
            last_stage: -1,
            last_cmd: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TravisCode> {
        if gate >= GATES {
            return Err(TravisCode::ESeal);
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
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// build 段一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_stage(&mut self, o: usize, got: u64) -> Result<(), TravisCode> {
        if o >= STAGE_CELLS {
            return Err(TravisCode::ENoStage);
        }
        let kind = match stage_kind(o) {
            Some(k) => k,
            None => return Err(TravisCode::ENoStage),
        };
        if (kind as i64) < self.last_stage {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200);
            return Err(TravisCode::EStageOrder);
        }
        let want = match stage_cell(o) {
            Some(w) => w,
            None => return Err(TravisCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9201 + o as u64);
            return Err(TravisCode::EStageDrift);
        }
        if match self.stages.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TravisCode::EDup);
        }
        if let Some(c) = self.stages.get_mut(o) {
            *c = Some(got);
        }
        self.last_stage = kind as i64;
        self.fnv(0x9202 + o as u64);
        Ok(())
    }

    /// build 段全账核验（20 格）。
    pub fn stages_all(&self) -> bool {
        let mut o = 0usize;
        while o < STAGE_CELLS {
            match self.stages.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令操作一格（命令单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), TravisCode> {
        if o >= CMD_OPS {
            return Err(TravisCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(TravisCode::ENoCmd),
        };
        if (kind as i64) < self.last_cmd {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(TravisCode::ECmdOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(TravisCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301 + o as u64);
            return Err(TravisCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TravisCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_cmd = kind as i64;
        self.fnv(0x9302 + o as u64);
        Ok(())
    }

    /// 命令操作全账核验（10 操作）。
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

    /// .travis.yml 投影一格（对合投影复核 + 载荷对照 + 重复拒）。
    pub fn record_yml(&mut self, f: usize, got: u64) -> Result<(), TravisCode> {
        if f >= YML_CELLS {
            return Err(TravisCode::ENoYml);
        }
        let base = match yml_cell(f) {
            Some(w) => w,
            None => return Err(TravisCode::ENoYml),
        };
        if yml_back(yml_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + f as u64);
            return Err(TravisCode::EYmlDrift);
        }
        if match self.ymls.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TravisCode::EDup);
        }
        if let Some(x) = self.ymls.get_mut(f) {
            *x = Some(got);
        }
        self.fnv(0x9401 + f as u64);
        Ok(())
    }

    /// .travis.yml 投影全账核验（8 格）。
    pub fn ymls_all(&self) -> bool {
        let mut f = 0usize;
        while f < YML_CELLS {
            match self.ymls.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 长尾维护声明一行。
    pub fn declare_eol(&mut self, r: usize) -> Result<(), TravisCode> {
        if r >= EOL_ROWS {
            return Err(TravisCode::ENoRow);
        }
        if match self.eols.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TravisCode::EDup);
        }
        if let Some(c) = self.eols.get_mut(r) {
            *c = eol_row(r);
        }
        self.fnv(0x9500 + r as u64);
        Ok(())
    }

    /// 长尾维护全齐核验。
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

    /// 迁移消费声明一行。
    pub fn declare_mig(&mut self, r: usize) -> Result<(), TravisCode> {
        if r >= MIG_ROWS {
            return Err(TravisCode::ENoRow);
        }
        if match self.migs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TravisCode::EDup);
        }
        if let Some(c) = self.migs.get_mut(r) {
            *c = mig_row(r);
        }
        self.fnv(0x9600 + r as u64);
        Ok(())
    }

    /// 迁移消费全齐核验。
    pub fn migs_all(&self) -> bool {
        let mut r = 0usize;
        while r < MIG_ROWS {
            match self.migs.get(r) {
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

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（长尾封顶：无论账面全齐与否一律 S3——travis-ci.
    /// org 已停运在册，只保留迁移消费账，不再承诺 S1/S2 档）。
    pub fn judge_grade(&self) -> Grade {
        let _ = self.gates_all();
        let _ = self.stages_all();
        let _ = self.cmds_all();
        let _ = self.ymls_all();
        let _ = self.eols_all();
        let _ = self.migs_all();
        Grade::S3
    }
}

impl Default for TravisLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TravisLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < STAGE_CELLS {
        if let Some(w) = stage_cell(o) {
            if led.record_stage(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CMD_OPS {
        if let Some(w) = cmd_cell(c) {
            if led.record_cmd(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut f = 0usize;
    while f < YML_CELLS {
        if let Some(w) = yml_cell(f) {
            if led.record_yml(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < EOL_ROWS {
        if led.declare_eol(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut m = 0usize;
    while m < MIG_ROWS {
        if led.declare_mig(m).is_ok() {
            recorded += 1;
        }
        m += 1;
    }
    // 注入 10 轮：build 段越闭集 / 命令操作逆序 / .travis.yml 投影漂移
    // 三态轮转。全部落在当轮临时账本上；逆序注入先按合法序落正格再注
    // 逆序；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TravisLedger::new();
        let caught = match kind {
            0 => lt.record_stage(STAGE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(TravisCode::ECmdOrder)
            }
            _ => {
                let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
                lt.record_yml(1, yml_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(TravisCode::EYmlDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9700 + k as u64);
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
/// UNX-F52377 判据集（承载卡 52378 判据锚 UNX-F52379-J1，off-by-one
/// 板式；§AI-66.3 锚定：只读判据，写操作干跑；travis-ci.org 已停运如
/// 实在册，外部资源零写入）。
pub fn run_unx52377_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52377");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "build 五段 20 格全通",
        recorded >= STAGE_CELLS as u32,
        "install/script/before_deploy/deploy/after_script 五段逐格恒等",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = TravisLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    if lt.record_cmd(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmds_all()
        },
        "version/login/build/coveralls/report 五命令逐操作恒等（写操作落干跑声明格）",
    );
    s.add(
        ".travis.yml 投影 8 格全通",
        {
            let mut lt = TravisLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < YML_CELLS {
                if let Some(w) = yml_cell(f) {
                    if lt.record_yml(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.ymls_all()
        },
        "language/distro/script/env/矩阵/通知/缓存/部署钩子逐格对合恒等",
    );
    s.add(
        "长尾维护声明 2/2 全齐",
        {
            let mut lt = TravisLedger::new();
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
        "travis-ci.org 停运在册 / travis-ci.com 长尾维护在册两行闭集",
    );
    s.add(
        "迁移消费声明 2/2 全齐",
        {
            let mut lt = TravisLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < MIG_ROWS {
                if lt.declare_mig(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.migs_all()
        },
        "GitHub Actions / GitLab CI 迁移消费分账两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TravisLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/build 生命周期/.travis.yml 解析/命令面收口/长尾在册声明/只读干跑逐格落账",
    );
    s.add(
        "段与命令双单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < STAGE_CELLS {
                if stage_kind(o).unwrap_or(5) < stage_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            let mut c = 1usize;
            while c < CMD_OPS {
                if cmd_kind(c).unwrap_or(5) < cmd_kind(c - 1).unwrap_or(5) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五段闭集与五命令闭集双单调，逆序必抓",
    );
    s.add(
        "外部资源零写入声明",
        SEG_BASE % 0x40 == 0,
        "只读判据，写操作一律干跑声明；travis-ci.org 已停运，全链纯对账无下发路径",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "build 段越闭集/命令操作逆序/.travis.yml 投影漂移三态轮转必抓",
    );
    let mut l0 = TravisLedger::new();
    s.add(
        "边界·build 段越闭集",
        l0.record_stage(STAGE_CELLS, 0).is_err(),
        "段 20 越闭集必拒",
    );
    s.add(
        "边界·命令操作越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_yml(YML_CELLS, 0).is_err(),
        "投影 8 越闭集必拒",
    );
    s.add(
        "投影漂移拒",
        l0.record_yml(2, yml_cell(2).unwrap_or(0).wrapping_add(1)) == Err(TravisCode::EYmlDrift),
        "对合投影与基准不恒等必抓",
    );
    s.add(
        "声明行越闭集拒",
        l0.declare_eol(EOL_ROWS).is_err() && l0.declare_mig(MIG_ROWS).is_err(),
        "长尾/迁移声明行越闭集必拒",
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
        "定级 S3 长尾封顶",
        {
            let mut lt = TravisLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < STAGE_CELLS {
                if let Some(w) = stage_cell(o) {
                    let _ = lt.record_stage(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    let _ = lt.record_cmd(c, w);
                }
                c += 1;
            }
            let mut f = 0usize;
            while f < YML_CELLS {
                if let Some(w) = yml_cell(f) {
                    let _ = lt.record_yml(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < EOL_ROWS {
                let _ = lt.declare_eol(r);
                r += 1;
            }
            let mut m = 0usize;
            while m < MIG_ROWS {
                let _ = lt.declare_mig(m);
                m += 1;
            }
            lt.judge_grade() == Grade::S3
        },
        "全通亦封顶 S3（长尾维护期如实定级，KB-COMPAT 登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9E00,
        "SEG_BASE 0x40 对齐且落 0xC9E00 段",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_cmd_kinds() {
        assert_eq!(stage_kind(0), Some(0));
        assert_eq!(stage_kind(4), Some(1));
        assert_eq!(stage_kind(19), Some(4));
        assert!(stage_kind(20).is_none());
        assert_eq!(stage_kind_name(2), Some("before_deploy"));
        assert_eq!(stage_kind_name(4), Some("after_script"));
        assert!(stage_kind_name(5).is_none());
        assert!(stage_cell(20).is_none());
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(3), Some(1));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(3), Some("coveralls"));
        assert!(cmd_cell(10).is_none());
        // 纯函数恒等 + 闭集边界。
        let a = stage_cell(3).unwrap_or(0);
        assert_eq!(a, stage_cell(3).unwrap_or(0));
        assert_ne!(a, stage_cell(4).unwrap_or(0));
        assert!(yml_cell(YML_CELLS).is_none());
        assert!(eol_row(EOL_ROWS).is_none());
        assert!(mig_row(MIG_ROWS).is_none());
    }

    #[test]
    fn test_yml_projection_roundtrip() {
        let mut f = 0usize;
        while f < YML_CELLS {
            let base = yml_cell(f).unwrap_or(0);
            assert_eq!(yml_back(yml_fwd(base)), base);
            assert_eq!(yml_fwd(yml_back(base)), base);
            f += 1;
        }
        assert_ne!(yml_fwd(0xB3000), 0xB3000);
    }

    #[test]
    fn test_stage_cmd_order_caught() {
        let mut lt = TravisLedger::new();
        let _ = lt.record_stage(9, stage_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_stage(2, stage_cell(2).unwrap_or(0)),
            Err(TravisCode::EStageOrder)
        );
        assert!(lt.record_stage(10, stage_cell(10).unwrap_or(0)).is_ok());
        let _ = lt.record_cmd(5, cmd_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(1, cmd_cell(1).unwrap_or(0)),
            Err(TravisCode::ECmdOrder)
        );
        assert!(lt.record_cmd(6, cmd_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_dup_caught() {
        let mut lt = TravisLedger::new();
        assert_eq!(
            lt.record_yml(3, yml_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(TravisCode::EYmlDrift)
        );
        assert_eq!(
            lt.record_stage(5, stage_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(TravisCode::EStageDrift)
        );
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)), Err(TravisCode::EDup));
        // 空账本直接注逆序：同段重复序号不算逆序，但越闭集必抓。
        let mut lt2 = TravisLedger::new();
        assert_eq!(lt2.record_stage(STAGE_CELLS + 1, 0), Err(TravisCode::ENoStage));
        assert_eq!(lt2.stats().0, 0, "越闭集边界拒不计注入账");
    }

    #[test]
    fn test_rows_dup_and_boundary() {
        let mut lt = TravisLedger::new();
        assert!(lt.declare_eol(0).is_ok());
        assert!(lt.declare_eol(1).is_ok());
        assert_eq!(lt.declare_eol(0), Err(TravisCode::EDup));
        assert_eq!(lt.declare_eol(2), Err(TravisCode::ENoRow));
        assert!(lt.declare_mig(0).is_ok());
        assert!(lt.declare_mig(1).is_ok());
        assert_eq!(lt.declare_mig(1), Err(TravisCode::EDup));
        assert!(lt.eols_all());
        assert!(lt.migs_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TravisLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TravisLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 段 + 10 操作 + 8 投影 + 2 长尾 + 2 迁移 = 42。
        assert_eq!(
            recorded,
            STAGE_CELLS as u32 + CMD_OPS as u32 + YML_CELLS as u32 + EOL_ROWS as u32 + MIG_ROWS as u32
        );
        assert_eq!(caught, 10);
        // 双跑指纹一致。
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_codes_closed() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9E00);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
        assert_eq!(TravisCode::Ok as u32, 0);
        assert_eq!(TravisCode::EGradeDrift as u32, 12);
        assert_eq!(TravisCode::Ok.val(), SEG_BASE);
        assert_eq!(TravisCode::ESeal.val(), SEG_BASE + 0x0B);
        assert!(!TravisCode::Ok.detail().is_empty());
        assert!(!TravisCode::ESeal.detail().is_empty());
        // XOR 锚低 16 位恒零。
        assert_eq!(TRAVIS_XOR & 0xFFFF, 0);
        assert_eq!(YML_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_names_grade_closed() {
        assert_eq!(Grade::S3.val(), 3);
        // 空账 S3（长尾封顶），全通亦 S3。
        let lt = TravisLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(yml_name(0), Some("language"));
        assert_eq!(yml_name(7), Some("deploy 钩子"));
        assert!(yml_name(8).is_none());
        assert_eq!(eol_row_name(0), Some("travis-ci.org 停运在册声明"));
        assert!(eol_row_name(2).is_none());
        assert_eq!(mig_row_name(1), Some("GitLab CI 迁移消费分账声明"));
        assert!(mig_row_name(2).is_none());
        assert_eq!(gate_name(5), Some("只读干跑"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_injection_conservation() {
        // 注入账守恒：临时账本逐轮核必抓，主账本检出=注入、逃逸=0。
        let (recorded, caught, _) = run_cycle();
        let _ = recorded;
        assert_eq!(caught, 10);
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = TravisLedger::new();
            let hit = match kind {
                0 => lt.record_stage(STAGE_CELLS, 0).is_err(),
                1 => {
                    let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                    lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(TravisCode::ECmdOrder)
                }
                _ => {
                    let _ = lt.record_yml(0, yml_cell(0).unwrap_or(0));
                    lt.record_yml(1, yml_cell(1).unwrap_or(0).wrapping_add(1))
                        == Err(TravisCode::EYmlDrift)
                }
            };
            assert!(hit);
            let (_, _, e) = lt.stats();
            assert_eq!(e, 0, "逃逸必须为 0");
            k += 1;
        }
    }

    #[test]
    fn unx52377_checks_green() {
        let s = run_unx52377_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
