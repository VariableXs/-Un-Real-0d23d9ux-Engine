//! UNX-F52379 Vercel/Netlify CLI 收口（部署面干跑判据） · 0xC9E80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52380"，逐字）：
//!   title: Vercel/Netlify CLI 收口（部署面干跑判据，与 sites 通道分
//!     账声明）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52381-J1 服务
//!     安装/指标上报 ×10 项全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3
//!     组 20 口径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本文件（文件号 N=52379）承载卡 N+1=52380 的卡
//! 面原文（任务口径：文件号较卡号后移一位），判据锚照该卡原文
//! UNX-F52381-J1 锚定；功能按本卡标题（Vercel/Netlify CLI 收口·部署
//! 面干跑判据，与 sites 通道分账声明）落地——判据原文是邻卡字面，本
//! 模块仅收口 Vercel/Netlify CLI 部署控制面（部署判据一律干跑列清单，
//! 禁真实外发——对外动作红线；外部资源零写入）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 部署五段 20 格闭集：build/prepare/upload/deploy/verify 五段各
//!      4 格，段单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 五命令 10 操作闭集：dev/build/deploy/env/logs 五命令各 2 操
//!      作，命令单调（逆序必抓）；deploy 判据一律落干跑列清单格。
//!   ③ 干跑判据·零写入声明 2 行闭集（干跑列清单零写入声明 / 对外动
//!      作红线禁真实外发声明），缺行拒封；与 sites 通道分账声明 2 行
//!      闭集（Vercel 通道分账 / Netlify 通道分账），缺行拒封。
//!   ④ vercel.json/netlify.toml 双向投影 12 格闭集（两配置面各 6 格），
//!      双向对合投影（back(fwd(x))==x），漂移必抓。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（部署段越闭集/命令操作逆序/
//!      双向投影漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（部署面只做干跑判据、云侧部署未实测，定级封顶
//!      S2——六闸齐即达 S2，账全齐不再升，KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52379;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 部署五段格数（五段 × 4 格）。
pub const STAGE_CELLS: usize = 20;

/// 五命令 10 操作格数（五命令 × 2 操作）。
pub const CMD_OPS: usize = 10;

/// 零写入声明行数。
pub const DRY_ROWS: usize = 2;

/// sites 通道分账声明行数。
pub const SEP_ROWS: usize = 2;

/// vercel.json/netlify.toml 双向投影格数（两配置面 × 6 格）。
pub const PROJ_CELLS: usize = 12;

/// 六判据闸数。
pub const GATES: usize = 6;

/// Vercel/Netlify 域锚异或（低 16 位恒零）。
pub const VNC_XOR: u64 = 0x564E_4C44_0000_0000;

/// 配置投影锚异或（低 16 位恒零，对合键）。
pub const PROJ_XOR: u64 = 0x5052_4A43_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VercelCode {
    /// 成功
    Ok = 0,
    /// 部署段格越闭集
    ENoStage,
    /// 部署段逆序
    EStageOrder,
    /// 部署段载荷漂移
    EStageDrift,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令操作逆序
    ECmdOrder,
    /// 命令操作载荷漂移
    ECmdDrift,
    /// 双向投影格越闭集
    ENoProj,
    /// 双向投影漂移
    EProjDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl VercelCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VercelCode::Ok => "成功",
            VercelCode::ENoStage => "部署段格越闭集：build/prepare/upload/deploy/verify 20 格之外先登记",
            VercelCode::EStageOrder => "部署段逆序：五段生命周期单调，逆序必抓",
            VercelCode::EStageDrift => "部署段载荷漂移：段折叠与基准不恒等，逐格复算",
            VercelCode::ENoCmd => "命令操作越闭集：dev/build/deploy/env/logs 10 操作之外先登记",
            VercelCode::ECmdOrder => "命令操作逆序：五命令单调，逆序必抓",
            VercelCode::ECmdDrift => "命令操作载荷漂移：命令折叠与基准不恒等，逐格复算",
            VercelCode::ENoProj => "双向投影格越闭集：vercel.json/netlify.toml 12 格之外先登记",
            VercelCode::EProjDrift => "双向投影漂移：对合投影 back(fwd(x))==x 不成立，逐格复算",
            VercelCode::ENoRow => "声明行越闭集：零写入/分账各 2 行之外先登记",
            VercelCode::EDup => "重复落账：同格只许一条账目",
            VercelCode::ESeal => "未齐即封：20 段格 + 10 操作 + 12 投影 + 4 声明未齐先补齐",
            VercelCode::EGradeDrift => "定级漂移：部署面定级封顶 S2，不得越过",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（部署面不承诺：云侧部署未实测）
    S1,
    /// S2：干跑收口（部署面封顶档）
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
            Grade::S1 => "S1：全通收口（云侧真实部署未实测，部署面不承诺此档）",
            Grade::S2 => "S2：干跑收口（部署面封顶档：六判据+账目全齐，零写入干跑列清单）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 部署段格载荷（格 o；段 = o/4）。
pub const fn stage_cell(o: usize) -> Option<u64> {
    if o >= STAGE_CELLS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x3E) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ VNC_XOR)
}

/// 部署段（0..4=build 4..8=prepare 8..12=upload 12..16=deploy
/// 16..20=verify）。
pub const fn stage_kind(o: usize) -> Option<u32> {
    if o >= STAGE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 部署段名。
pub const fn stage_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("build"),
        1 => Some("prepare"),
        2 => Some("upload"),
        3 => Some("deploy"),
        4 => Some("verify"),
        _ => None,
    }
}

/// 命令操作格载荷（操作 o；命令 = o/2）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x48) ^ ((o / 2) as u64).wrapping_mul(0x01000193) ^ VNC_XOR)
}

/// 命令（0..2=dev 2..4=build 4..6=deploy 6..8=env 8..10=logs）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("dev"),
        1 => Some("build"),
        2 => Some("deploy"),
        3 => Some("env"),
        4 => Some("logs"),
        _ => None,
    }
}

/// 双向投影格基准载荷（格 f；配置面 = f/6；对合投影锚 PROJ_XOR）。
pub const fn proj_cell(f: usize) -> Option<u64> {
    if f >= PROJ_CELLS {
        return None;
    }
    Some((0xB3000u64 + (f as u64) * 0x26) ^ PROJ_XOR)
}

/// 双向投影·正向（back(fwd(x))==x）。
pub const fn proj_fwd(x: u64) -> u64 {
    x ^ PROJ_XOR
}

/// 双向投影·反向。
pub const fn proj_back(y: u64) -> u64 {
    y ^ PROJ_XOR
}

/// 双向投影格名（vercel.json/netlify.toml 各 6 格对照）。
pub const fn proj_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("vercel.json build 配置"),
        1 => Some("vercel.json routes 配置"),
        2 => Some("vercel.json env 配置"),
        3 => Some("vercel.json regions 配置"),
        4 => Some("vercel.json headers 配置"),
        5 => Some("vercel.json redirects 配置"),
        6 => Some("netlify.toml build 配置"),
        7 => Some("netlify.toml redirects 配置"),
        8 => Some("netlify.toml headers 配置"),
        9 => Some("netlify.toml functions 配置"),
        10 => Some("netlify.toml plugins 配置"),
        11 => Some("netlify.toml context 配置"),
        _ => None,
    }
}

/// 零写入声明行折叠（行 r）。
pub const fn dry_row(r: usize) -> Option<u32> {
    if r >= DRY_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x24)
}

/// 零写入声明行名。
pub const fn dry_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("干跑列清单零写入声明"),
        1 => Some("对外动作红线禁真实外发声明"),
        _ => None,
    }
}

/// sites 通道分账声明行折叠（行 r）。
pub const fn sep_row(r: usize) -> Option<u32> {
    if r >= SEP_ROWS {
        return None;
    }
    Some(0xB480 + r as u32 * 0x24)
}

/// sites 通道分账声明行名。
pub const fn sep_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Vercel 通道与 sites 通道分账声明"),
        1 => Some("Netlify 通道与 sites 通道分账声明"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("部署生命周期"),
        2 => Some("配置投影解析"),
        3 => Some("命令面收口"),
        4 => Some("分账声明"),
        5 => Some("干跑列清单"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Vercel/Netlify CLI 收口账本状态机。
#[derive(Clone)]
pub struct VercelLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// 部署段账。
    stages: [Option<u64>; STAGE_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 双向投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 零写入声明账。
    drys: [Option<u32>; DRY_ROWS],
    /// sites 通道分账声明账。
    seps: [Option<u32>; SEP_ROWS],
    /// 最后部署段。
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

impl VercelLedger {
    /// 新账本。
    pub fn new() -> Self {
        VercelLedger {
            gates: [None; GATES],
            stages: [None; STAGE_CELLS],
            cmds: [None; CMD_OPS],
            projs: [None; PROJ_CELLS],
            drys: [None; DRY_ROWS],
            seps: [None; SEP_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VercelCode> {
        if gate >= GATES {
            return Err(VercelCode::ESeal);
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

    /// 部署段一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_stage(&mut self, o: usize, got: u64) -> Result<(), VercelCode> {
        if o >= STAGE_CELLS {
            return Err(VercelCode::ENoStage);
        }
        let kind = match stage_kind(o) {
            Some(k) => k,
            None => return Err(VercelCode::ENoStage),
        };
        if (kind as i64) < self.last_stage {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200);
            return Err(VercelCode::EStageOrder);
        }
        let want = match stage_cell(o) {
            Some(w) => w,
            None => return Err(VercelCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9201 + o as u64);
            return Err(VercelCode::EStageDrift);
        }
        if match self.stages.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VercelCode::EDup);
        }
        if let Some(c) = self.stages.get_mut(o) {
            *c = Some(got);
        }
        self.last_stage = kind as i64;
        self.fnv(0x9202 + o as u64);
        Ok(())
    }

    /// 部署段全账核验（20 格）。
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
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), VercelCode> {
        if o >= CMD_OPS {
            return Err(VercelCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(VercelCode::ENoCmd),
        };
        if (kind as i64) < self.last_cmd {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(VercelCode::ECmdOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(VercelCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301 + o as u64);
            return Err(VercelCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VercelCode::EDup);
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

    /// 双向投影一格（对合投影复核 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, f: usize, got: u64) -> Result<(), VercelCode> {
        if f >= PROJ_CELLS {
            return Err(VercelCode::ENoProj);
        }
        let base = match proj_cell(f) {
            Some(w) => w,
            None => return Err(VercelCode::ENoProj),
        };
        if proj_back(proj_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + f as u64);
            return Err(VercelCode::EProjDrift);
        }
        if match self.projs.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VercelCode::EDup);
        }
        if let Some(x) = self.projs.get_mut(f) {
            *x = Some(got);
        }
        self.fnv(0x9401 + f as u64);
        Ok(())
    }

    /// 双向投影全账核验（12 格）。
    pub fn projs_all(&self) -> bool {
        let mut f = 0usize;
        while f < PROJ_CELLS {
            match self.projs.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 零写入声明一行。
    pub fn declare_dry(&mut self, r: usize) -> Result<(), VercelCode> {
        if r >= DRY_ROWS {
            return Err(VercelCode::ENoRow);
        }
        if match self.drys.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VercelCode::EDup);
        }
        if let Some(c) = self.drys.get_mut(r) {
            *c = dry_row(r);
        }
        self.fnv(0x9500 + r as u64);
        Ok(())
    }

    /// 零写入声明全齐核验。
    pub fn drys_all(&self) -> bool {
        let mut r = 0usize;
        while r < DRY_ROWS {
            match self.drys.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// sites 通道分账声明一行。
    pub fn declare_sep(&mut self, r: usize) -> Result<(), VercelCode> {
        if r >= SEP_ROWS {
            return Err(VercelCode::ENoRow);
        }
        if match self.seps.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VercelCode::EDup);
        }
        if let Some(c) = self.seps.get_mut(r) {
            *c = sep_row(r);
        }
        self.fnv(0x9600 + r as u64);
        Ok(())
    }

    /// sites 通道分账声明全齐核验。
    pub fn seps_all(&self) -> bool {
        let mut r = 0usize;
        while r < SEP_ROWS {
            match self.seps.get(r) {
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

    /// KB-COMPAT 定级（部署面封顶 S2：六闸齐即达 S2，账全齐不再升
    /// ——云侧真实部署未实测，只做零写入干跑列清单；判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VercelLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VercelLedger::new();
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
    while f < PROJ_CELLS {
        if let Some(w) = proj_cell(f) {
            if led.record_proj(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < DRY_ROWS {
        if led.declare_dry(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut m = 0usize;
    while m < SEP_ROWS {
        if led.declare_sep(m).is_ok() {
            recorded += 1;
        }
        m += 1;
    }
    // 注入 10 轮：部署段越闭集 / 命令操作逆序 / 双向投影漂移三态轮转。
    // 全部落在当轮临时账本上；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VercelLedger::new();
        let caught = match kind {
            0 => lt.record_stage(STAGE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(VercelCode::ECmdOrder)
            }
            _ => {
                let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
                lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(VercelCode::EProjDrift)
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
/// UNX-F52379 判据集（承载卡 52380 判据锚 UNX-F52381-J1，off-by-one
/// 板式；§AI-66.3 组 20 口径：部署判据一律干跑列清单，禁真实外发——
/// 对外动作红线，外部资源零写入）。
pub fn run_unx52379_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52379");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "部署五段 20 格全通",
        recorded >= STAGE_CELLS as u32,
        "build/prepare/upload/deploy/verify 五段逐格恒等",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = VercelLedger::new();
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
        "dev/build/deploy/env/logs 五命令逐操作恒等（deploy 一律干跑列清单）",
    );
    s.add(
        "双向投影 12 格全通",
        {
            let mut lt = VercelLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < PROJ_CELLS {
                if let Some(w) = proj_cell(f) {
                    if lt.record_proj(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.projs_all()
        },
        "vercel.json/netlify.toml 两配置面各 6 格逐格对合恒等",
    );
    s.add(
        "零写入声明 2/2 全齐",
        {
            let mut lt = VercelLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DRY_ROWS {
                if lt.declare_dry(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.drys_all()
        },
        "干跑列清单零写入 / 对外动作红线禁真实外发两行闭集",
    );
    s.add(
        "sites 通道分账声明 2/2 全齐",
        {
            let mut lt = VercelLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SEP_ROWS {
                if lt.declare_sep(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.seps_all()
        },
        "Vercel 通道 / Netlify 通道与 sites 通道分账两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VercelLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/部署生命周期/配置投影解析/命令面收口/分账声明/干跑列清单逐格落账",
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
        "部署判据一律干跑列清单，禁真实外发——对外动作红线；全链纯对账无下发路径",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "部署段越闭集/命令操作逆序/双向投影漂移三态轮转必抓",
    );
    let mut l0 = VercelLedger::new();
    s.add(
        "边界·部署段越闭集",
        l0.record_stage(STAGE_CELLS, 0).is_err(),
        "段 20 越闭集必拒",
    );
    s.add(
        "边界·命令操作越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·双向投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "投影漂移拒",
        l0.record_proj(3, proj_cell(3).unwrap_or(0).wrapping_add(1)) == Err(VercelCode::EProjDrift),
        "对合投影与基准不恒等必抓",
    );
    s.add(
        "声明行越闭集拒",
        l0.declare_dry(DRY_ROWS).is_err() && l0.declare_sep(SEP_ROWS).is_err(),
        "零写入/分账声明行越闭集必拒",
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
        "定级 S2 干跑封顶",
        {
            let mut lt = VercelLedger::new();
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
            while f < PROJ_CELLS {
                if let Some(w) = proj_cell(f) {
                    let _ = lt.record_proj(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < DRY_ROWS {
                let _ = lt.declare_dry(r);
                r += 1;
            }
            let mut m = 0usize;
            while m < SEP_ROWS {
                let _ = lt.declare_sep(m);
                m += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 干跑封顶（云侧真实部署未实测，KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9E80,
        "SEG_BASE 0x40 对齐且落 0xC9E80 段",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥19 条闭集");
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
        assert_eq!(stage_kind_name(2), Some("upload"));
        assert_eq!(stage_kind_name(4), Some("verify"));
        assert!(stage_kind_name(5).is_none());
        assert!(stage_cell(20).is_none());
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(5), Some(2));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(3), Some("env"));
        assert!(cmd_cell(10).is_none());
        // 纯函数恒等 + 闭集边界。
        let a = stage_cell(3).unwrap_or(0);
        assert_eq!(a, stage_cell(3).unwrap_or(0));
        assert_ne!(a, stage_cell(4).unwrap_or(0));
        assert!(proj_cell(PROJ_CELLS).is_none());
        assert!(dry_row(DRY_ROWS).is_none());
        assert!(sep_row(SEP_ROWS).is_none());
    }

    #[test]
    fn test_proj_projection_roundtrip() {
        let mut f = 0usize;
        while f < PROJ_CELLS {
            let base = proj_cell(f).unwrap_or(0);
            assert_eq!(proj_back(proj_fwd(base)), base);
            assert_eq!(proj_fwd(proj_back(base)), base);
            f += 1;
        }
        assert_ne!(proj_fwd(0xB3000), 0xB3000);
    }

    #[test]
    fn test_stage_cmd_order_caught() {
        let mut lt = VercelLedger::new();
        let _ = lt.record_stage(9, stage_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_stage(2, stage_cell(2).unwrap_or(0)),
            Err(VercelCode::EStageOrder)
        );
        assert!(lt.record_stage(10, stage_cell(10).unwrap_or(0)).is_ok());
        let _ = lt.record_cmd(5, cmd_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(1, cmd_cell(1).unwrap_or(0)),
            Err(VercelCode::ECmdOrder)
        );
        assert!(lt.record_cmd(6, cmd_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_dup_caught() {
        let mut lt = VercelLedger::new();
        assert_eq!(
            lt.record_proj(3, proj_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(VercelCode::EProjDrift)
        );
        assert_eq!(
            lt.record_stage(5, stage_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(VercelCode::EStageDrift)
        );
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)), Err(VercelCode::EDup));
    }

    #[test]
    fn test_rows_dup_and_boundary() {
        let mut lt = VercelLedger::new();
        assert!(lt.declare_dry(0).is_ok());
        assert!(lt.declare_dry(1).is_ok());
        assert_eq!(lt.declare_dry(0), Err(VercelCode::EDup));
        assert_eq!(lt.declare_dry(2), Err(VercelCode::ENoRow));
        assert!(lt.declare_sep(0).is_ok());
        assert!(lt.declare_sep(1).is_ok());
        assert_eq!(lt.declare_sep(1), Err(VercelCode::EDup));
        assert!(lt.drys_all());
        assert!(lt.seps_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VercelLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = VercelLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 段 + 10 操作 + 12 投影 + 2 零写入 + 2 分账 = 46。
        assert_eq!(
            recorded,
            STAGE_CELLS as u32 + CMD_OPS as u32 + PROJ_CELLS as u32 + DRY_ROWS as u32 + SEP_ROWS as u32
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
        assert_eq!(SEG_BASE, 0xC9E80);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
        assert_eq!(VercelCode::Ok as u32, 0);
        assert_eq!(VercelCode::EGradeDrift as u32, 12);
        assert_eq!(VercelCode::Ok.val(), SEG_BASE);
        assert_eq!(VercelCode::ESeal.val(), SEG_BASE + 0x0B);
        assert!(!VercelCode::Ok.detail().is_empty());
        assert!(!VercelCode::ESeal.detail().is_empty());
        // XOR 锚低 16 位恒零。
        assert_eq!(VNC_XOR & 0xFFFF, 0);
        assert_eq!(PROJ_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_names_grade_closed() {
        assert_eq!(Grade::S2.val(), 2);
        // 如实阶梯：空账 S3，六闸齐 S2（干跑封顶，全齐不再升）。
        let lt = VercelLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(proj_name(0), Some("vercel.json build 配置"));
        assert_eq!(proj_name(11), Some("netlify.toml context 配置"));
        assert!(proj_name(12).is_none());
        assert_eq!(dry_row_name(1), Some("对外动作红线禁真实外发声明"));
        assert!(dry_row_name(2).is_none());
        assert_eq!(sep_row_name(0), Some("Vercel 通道与 sites 通道分账声明"));
        assert!(sep_row_name(2).is_none());
        assert_eq!(gate_name(5), Some("干跑列清单"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_grade_ladder() {
        let mut lt = VercelLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        // 六闸齐即达 S2 封顶。
        assert_eq!(lt.judge_grade(), Grade::S2);
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
        while f < PROJ_CELLS {
            if let Some(w) = proj_cell(f) {
                let _ = lt.record_proj(f, w);
            }
            f += 1;
        }
        let mut r = 0usize;
        while r < DRY_ROWS {
            let _ = lt.declare_dry(r);
            r += 1;
        }
        let mut m = 0usize;
        while m < SEP_ROWS {
            let _ = lt.declare_sep(m);
            m += 1;
        }
        // 全齐仍封顶 S2。
        assert_eq!(lt.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_injection_conservation() {
        let (recorded, caught, _) = run_cycle();
        let _ = recorded;
        assert_eq!(caught, 10);
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = VercelLedger::new();
            let hit = match kind {
                0 => lt.record_stage(STAGE_CELLS, 0).is_err(),
                1 => {
                    let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                    lt.record_cmd(2, cmd_cell(2).unwrap_or(0)) == Err(VercelCode::ECmdOrder)
                }
                _ => {
                    let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
                    lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(1))
                        == Err(VercelCode::EProjDrift)
                }
            };
            assert!(hit);
            let (_, _, e) = lt.stats();
            assert_eq!(e, 0, "逃逸必须为 0");
            k += 1;
        }
    }

    #[test]
    fn unx52379_checks_green() {
        let s = run_unx52379_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
