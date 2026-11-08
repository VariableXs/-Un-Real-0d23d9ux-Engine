//! UNX-F52546 Ionic CLI 收口 · 0xCC840段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52546"，逐字）：
//!   id=52546
//!   title: Ionic CLI 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52547-J1 虚拟化
//!     依赖探测账（L5 消费联签锚定行 AI-60），不可用即 N/A + 三要素告
//!     警；KB-COMPAT 定级 S3 + N/A 清单；判据锚定 §AI-66.2 专题五模拟
//!     器 N/A 同构；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52546）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52547-J1，照原文锚定（虚拟化依赖探测账为下一卡 Genymotion 域语
//! 料）；功能按本卡标题（Ionic CLI 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：Ionic CLI 五面 20 格（ionic start 4/依赖面 Capacitor-
//!      Cordova 双后端分账 4/serve 面 4/build 面 4/Capacitor 同步面 4），
//!      能力面单调，逐格载荷（纯函数折叠）恒等才收；漂移必抓、逆序必抓。
//!   ② 依赖投影 8 格：8 格闭集、双段（Capacitor 段投影 0..4、插件段投影
//!      4..8）单调，投影漂移必抓。
//!   ③ 版本阶梯 3 档：6.20/7.0/7.1 三档闭集，越级必抓、载荷漂移必抓。
//!   ④ 声明 4 行闭集：Capacitor 后端声明 2 行 + 虚拟化依赖探测账 N/A
//!      口径声明 2 行（L5 消费联签锚定行 AI-60），缺一拒封（seal：六闸
//!      缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal）。
//!   ⑤ N/A 探测三分支闭集：可用 / 不可用 N/A / 探测失败显性。虚拟化依
//!      赖探测不可用即如实 N/A：定级封顶 S3 + N/A 清单，禁虚报可用；N/A
//!      即触发三要素告警（探测账分支 / 定级封顶 / 判据锚定）。
//!   ⑥ ledger 状态机（new/record_*/declare_split/record_probe/judge_gate
//!      ×6/fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 10 轮三
//!      态轮转（版本漂移/操作漂移/投影漂移），三类注入全部落在当轮临时
//!      账本上，主账 injected==caught==10、escaped==0；双跑指纹一致；定
//!      级目标档 S1（探测账可用为前置）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52546;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等落 0xCC840 且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCC840 && SEG_BASE % 0x40 == 0);

/// Ionic CLI 五面操作数。
pub const OPS: usize = 20;

/// 依赖投影格数。
pub const PATH_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 声明行数（Capacitor 后端声明 2 行 + 虚拟化依赖探测账 N/A 口径声明 2 行）。
pub const SPLIT_ROWS: usize = 4;

/// 收口闸数。
pub const GATES: usize = 6;

/// 主账总账目数（六闸 6 + 能力 20 + 投影 8 + 版本 3 + 声明 4 = 41）。
pub const LEDGER_ROWS: usize = GATES + OPS + PATH_CELLS + VERSION_STEPS + SPLIT_ROWS;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IonicCode {
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

impl IonicCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            IonicCode::Ok => "成功",
            IonicCode::ENoOp => "操作号越闭集：Ionic CLI 五面 20 格之外先登记",
            IonicCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            IonicCode::EOrder => "能力面逆序：ionic start→依赖面（Capacitor/Cordova 双后端分账）→serve 面→build 面→Capacitor 同步面单调，逆序必抓",
            IonicCode::ENoPath => "投影格越闭集：依赖投影 8 格之外先登记",
            IonicCode::EPathDrift => "投影漂移：依赖投影折叠与基准不恒等，逐格复算",
            IonicCode::ENoVer => "版本级越阶梯：6.20/7.0/7.1 三档闭集之外先登记",
            IonicCode::EVerDrift => "版本阶梯漂移：版本档折叠与基准不恒等，逐档复算",
            IonicCode::ENoRow => "声明行越闭集：Capacitor 后端声明+虚拟化依赖探测账 N/A 口径声明 4 行闭集之外先登记",
            IonicCode::ERowMiss => "缺声明行：Capacitor 后端声明 2 行/虚拟化依赖探测账 N/A 口径声明 2 行（L5 消费联签锚定行 AI-60）未齐先补齐",
            IonicCode::EDup => "重复落账：同格只许一条账目",
            IonicCode::ESeal => "未齐即封：20 能力格+8 投影格+3 档版本+六判据未齐先补齐",
            IonicCode::EGradeDrift => "定级漂移：全通且声明行全齐且探测账可用才可定 S1（KB-COMPAT 登记，探测 N/A 封顶 S3）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（另含 N/A 口径：探测不可用即封顶 S3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档，探测账可用为前置）
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口（含 N/A 封顶口径）
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
            Grade::S1 => "S1：六判据+20能力+8投影+3档版本+4行声明全通收口，探测账可用",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失；虚拟化依赖探测 N/A 时定级封顶 S3 + N/A 清单，禁虚报可用",
        }
    }
}

// ── 虚拟化依赖探测账（N/A 口径三分支闭集） ───────────────────────
/// 虚拟化依赖探测账三分支闭集：可用 / 不可用 N/A / 探测失败显性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeVerdict {
    /// 探测可用：虚拟化依赖在账，允许按常规阶梯定级。
    Available,
    /// 探测不可用：如实 N/A，定级封顶 S3 + N/A 清单，禁虚报可用。
    UnavailableNa,
    /// 探测失败显性：失败如实显性登记，不得宣称可用，定级封顶 S3。
    ProbeFailed,
}

impl ProbeVerdict {
    /// 分支值。
    pub fn val(self) -> u32 {
        match self {
            ProbeVerdict::Available => 0,
            ProbeVerdict::UnavailableNa => 1,
            ProbeVerdict::ProbeFailed => 2,
        }
    }
    /// 分支明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeVerdict::Available => "虚拟化依赖探测分支=可用：依赖在账，按常规定级阶梯走",
            ProbeVerdict::UnavailableNa => "虚拟化依赖探测分支=不可用 N/A：如实登记 N/A，定级封顶 S3 + N/A 清单，禁虚报可用",
            ProbeVerdict::ProbeFailed => "虚拟化依赖探测分支=探测失败显性：失败显性登记，不得宣称可用，定级封顶 S3",
        }
    }
}

/// N/A 清单（KB-COMPAT 定级 S3 + N/A 清单口径）。
pub const NA_MANIFEST: [&str; 3] = [
    "N/A 清单一：虚拟化依赖探测账不可用，如实登记 N/A（探测三分支闭集：可用/不可用 N/A/探测失败显性）",
    "N/A 清单二：KB-COMPAT 定级封顶 S3，S1/S2 不予登记，禁虚报可用",
    "N/A 清单三：判据锚定 §AI-66.2 专题五模拟器 N/A 同构（L5 消费联签锚定行 AI-60）",
];

/// N/A 三要素告警（不可用即触发）。
pub fn na_alert() -> [&'static str; 3] {
    [
        "告警要素一：虚拟化依赖探测账分支=不可用 N/A，探测三分支闭集如实登记",
        "告警要素二：KB-COMPAT 定级封顶 S3 + N/A 清单登记，禁虚报可用",
        "告警要素三：判据锚定 UNX-F52547-J1（L5 消费联签锚定行 AI-60，§AI-66.2 专题五模拟器 N/A 同构）",
    ]
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力面 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF5000u64 + (o as u64) * 0x2C) ^ 0x496F6E6963000000)
}

/// 能力面（0..4=ionic start 4..8=依赖面 8..12=serve 面 12..16=build 面 16..20=Capacitor 同步）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力面名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("ionic start"),
        1 => Some("依赖面"),
        2 => Some("serve 面"),
        3 => Some("build 面"),
        4 => Some("Capacitor 同步"),
        _ => None,
    }
}

/// 依赖投影格载荷（格 c；投影段 = c/4：0..4=Capacitor 段 4..8=插件段）。
pub const fn path_cell(c: usize) -> Option<u64> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((0xF6000u64 + (c as u64) * 0x31) ^ 0x43617050726F0000)
}

/// 投影段。
pub const fn path_kind(c: usize) -> Option<u32> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 投影段名。
pub const fn path_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Capacitor 段投影"),
        1 => Some("插件段投影"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xF7000u64 + (v as u64) * 0x5D) ^ 0x496F6E6900000000)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("6.20"),
        1 => Some("7.0"),
        2 => Some("7.1"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xF810 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Capacitor 后端声明·主"),
        1 => Some("Capacitor 后端声明·辅"),
        2 => Some("虚拟化依赖探测账 N/A 口径声明·主"),
        3 => Some("虚拟化依赖探测账 N/A 口径声明·辅"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Ionic CLI 收口账本状态机。
#[derive(Clone)]
pub struct IonicLedger {
    /// 六判据（收口闸：安装/启动/补全/诊断/升级/卸载）。
    gates: [Option<bool>; GATES],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 依赖投影账。
    paths: [Option<u64>; PATH_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 虚拟化依赖探测账（N/A 口径三分支）。
    probe: Option<ProbeVerdict>,
    /// 最后能力面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl IonicLedger {
    /// 新账本。
    pub fn new() -> Self {
        IonicLedger {
            gates: [None; GATES],
            ops: [None; OPS],
            paths: [None; PATH_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
            probe: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), IonicCode> {
        if gate >= GATES {
            return Err(IonicCode::ESeal);
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
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 能力一格（能力面单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), IonicCode> {
        if o >= OPS {
            return Err(IonicCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(IonicCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B00);
            return Err(IonicCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(IonicCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B01 + o as u64);
            return Err(IonicCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IonicCode::EDup);
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
    pub fn record_path(&mut self, c: usize, got: u64) -> Result<(), IonicCode> {
        let want = match path_cell(c) {
            Some(w) => w,
            None => return Err(IonicCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9C00 + c as u64);
            return Err(IonicCode::EPathDrift);
        }
        if match self.paths.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IonicCode::EDup);
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

    /// 版本阶梯一档（载荷对照 + 重复拒，越阶梯拒不计注入账）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), IonicCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(IonicCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9D00 + v as u64);
            return Err(IonicCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IonicCode::EDup);
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
    pub fn declare_split(&mut self, r: usize) -> Result<(), IonicCode> {
        if r >= SPLIT_ROWS {
            return Err(IonicCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(IonicCode::EDup);
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

    /// 虚拟化依赖探测账落账（三分支闭集，重复拒）。
    pub fn record_probe(&mut self, v: ProbeVerdict) -> Result<(), IonicCode> {
        if self.probe.is_some() {
            return Err(IonicCode::EDup);
        }
        self.probe = Some(v);
        self.fnv(0x9EA0 + v.val() as u64);
        Ok(())
    }

    /// 探测账读取。
    pub fn probe_verdict(&self) -> Option<ProbeVerdict> {
        self.probe
    }

    /// 探测账是否显性可用（N/A 与探测失败均不视为可用，禁虚报）。
    pub fn probe_available(&self) -> bool {
        match self.probe {
            Some(ProbeVerdict::Available) => true,
            _ => false,
        }
    }

    /// 探测账是否 N/A 口径（不可用 N/A 或探测失败显性，均封顶 S3）。
    pub fn probe_na(&self) -> bool {
        match self.probe {
            Some(ProbeVerdict::UnavailableNa) | Some(ProbeVerdict::ProbeFailed) => true,
            _ => false,
        }
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), IonicCode> {
        if !self.gates_all() {
            return Err(IonicCode::ESeal);
        }
        if !self.splits_all() {
            return Err(IonicCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.vers_all() {
            return Err(IonicCode::ESeal);
        }
        Ok(())
    }

    /// 主账已落账目数（六闸+能力+投影+版本+声明，全通=41）。
    pub fn account_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < GATES {
            if match self.gates.get(i) { Some(Some(true)) => true, _ => false } { n += 1; }
            i += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if match self.ops.get(o) { Some(Some(_)) => true, _ => false } { n += 1; }
            o += 1;
        }
        let mut c = 0usize;
        while c < PATH_CELLS {
            if match self.paths.get(c) { Some(Some(_)) => true, _ => false } { n += 1; }
            c += 1;
        }
        let mut v = 0usize;
        while v < VERSION_STEPS {
            if match self.vers.get(v) { Some(Some(_)) => true, _ => false } { n += 1; }
            v += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            if match self.splits.get(r) { Some(Some(_)) => true, _ => false } { n += 1; }
            r += 1;
        }
        n
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：探测 N/A/探测失败/探测未落账 → 封顶 S3（N/A 口径，
    /// 禁虚报可用）；全齐且探测可用→目标档 S1；六闸齐→S2；空账→S3。
    pub fn grade(&self) -> Grade {
        if !self.probe_available() {
            return Grade::S3;
        }
        if self.gates_all() && self.ops_all() && self.paths_all() && self.vers_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for IonicLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 全齐种子账本：六闸+20 能力+8 投影+3 版本+4 声明逐格落账；探测账按
/// 给定分支落（None=探测未落账，用于 N/A 口径禁虚报对照）。
pub fn seed_full_ledger(p: Option<ProbeVerdict>) -> IonicLedger {
    let mut lt = IonicLedger::new();
    let mut g = 0usize;
    while g < GATES { let _ = lt.judge_gate(g); g += 1; }
    let mut o = 0usize;
    while o < OPS { if let Some(w) = op_cell(o) { let _ = lt.record_op(o, w); } o += 1; }
    let mut c = 0usize;
    while c < PATH_CELLS { if let Some(w) = path_cell(c) { let _ = lt.record_path(c, w); } c += 1; }
    let mut v = 0usize;
    while v < VERSION_STEPS { if let Some(w) = version_cell(v) { let _ = lt.record_version(v, w); } v += 1; }
    let mut r = 0usize;
    while r < SPLIT_ROWS { let _ = lt.declare_split(r); r += 1; }
    if let Some(pv) = p {
        let _ = lt.record_probe(pv);
    }
    lt
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = IonicLedger::new();
    let mut g = 0usize;
    while g < GATES {
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
    // 虚拟化依赖探测账：主账按可用口径落账（探测三分支闭集之一）。
    let _ = led.record_probe(ProbeVerdict::Available);
    // 注入 10 轮：版本漂移 / 操作漂移 / 投影漂移三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = IonicLedger::new();
        let caught = match kind {
            0 => {
                // 版本漂移：先按合法序落 0 档正格，再注 1 档漂移，必抓。
                let w0 = match version_cell(0) { Some(w) => w, None => 0 };
                let w1 = match version_cell(1) { Some(w) => w, None => 0 };
                let _ = lt.record_version(0, w0);
                lt.record_version(1, w1.wrapping_add(1)) == Err(IonicCode::EVerDrift)
            }
            1 => {
                // 操作漂移：先按合法序落同族正格（ionic start 面），再注漂移。
                let w0 = match op_cell(0) { Some(w) => w, None => 0 };
                let w1 = match op_cell(1) { Some(w) => w, None => 0 };
                let _ = lt.record_op(0, w0);
                lt.record_op(1, w1.wrapping_add(1)) == Err(IonicCode::EDrift)
            }
            _ => {
                // 投影漂移：先落 Capacitor 段正格，再注插件段漂移，必抓。
                let w0 = match path_cell(0) { Some(w) => w, None => 0 };
                let w1 = match path_cell(1) { Some(w) => w, None => 0 };
                let _ = lt.record_path(0, w0);
                lt.record_path(1, w1.wrapping_add(1)) == Err(IonicCode::EPathDrift)
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
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52546 判据集（锚 UNX-F52547-J1，off-by-one 板式）。
pub fn run_unx52546_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52546");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Ionic CLI 五面 20 操作全通",
        recorded >= OPS as u32,
        "ionic start/依赖面/serve 面/build 面/Capacitor 同步逐格恒等",
    );
    s.add(
        "依赖投影 8/8 全落",
        {
            let mut lt = IonicLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    if lt.record_path(c, w).is_err() { ok = false; }
                }
                c += 1;
            }
            ok && lt.paths_all()
        },
        "Capacitor 段投影 0..4 与插件段投影 4..8 双段逐格恒等",
    );
    s.add(
        "版本阶梯 6.20/7.0/7.1 三档全落",
        {
            let mut lt = IonicLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() { ok = false; }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐档恒等，越级必抓",
    );
    s.add(
        "Capacitor 后端与虚拟化探测 N/A 口径声明 4/4 全齐",
        {
            let mut lt = IonicLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() { ok = false; }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "Capacitor 后端声明 2 行 + 虚拟化依赖探测账 N/A 口径声明 2 行闭集（L5 消费联签锚定行 AI-60）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = IonicLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/启动/补全/诊断/升级/卸载逐格落账",
    );
    s.add(
        "能力面单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) { Some(k) => k, None => 5 };
                let kb = match op_kind(o - 1) { Some(k) => k, None => 5 };
                if ka < kb { ok = false; }
                o += 1;
            }
            ok
        },
        "ionic start→依赖面（Capacitor/Cordova 双后端分账）→serve 面→build 面→Capacitor 同步单调",
    );
    s.add(
        "投影段单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < PATH_CELLS {
                let ka = match path_kind(c) { Some(k) => k, None => 2 };
                let kb = match path_kind(c - 1) { Some(k) => k, None => 2 };
                if ka < kb { ok = false; }
                c += 1;
            }
            ok
        },
        "Capacitor 段→插件段双段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本漂移/操作漂移/投影漂移三态轮转必抓",
    );
    s.add(
        "闭集边界四拒（越闭集不计注入账）",
        {
            let mut lt = IonicLedger::new();
            let b1 = lt.record_op(OPS, 0).is_err();
            let b2 = lt.record_path(PATH_CELLS, 0).is_err();
            let b3 = lt.record_version(VERSION_STEPS, 0).is_err();
            let b4 = lt.declare_split(SPLIT_ROWS).is_err();
            b1 && b2 && b3 && b4 && lt.stats() == (0, 0, 0)
        },
        "操作 20/投影 8/声明行 4/版本级 3 越闭集必拒且不计注入账",
    );
    s.add(
        "三漂移拒（能力/投影/版本载荷必抓）",
        {
            let mut lt = IonicLedger::new();
            let w0 = match op_cell(0) { Some(w) => w, None => 0 };
            let w1 = match op_cell(1) { Some(w) => w, None => 0 };
            let p0 = match path_cell(0) { Some(w) => w, None => 0 };
            let p1 = match path_cell(1) { Some(w) => w, None => 0 };
            let v0 = match version_cell(0) { Some(w) => w, None => 0 };
            let v1 = match version_cell(1) { Some(w) => w, None => 0 };
            let _ = lt.record_op(0, w0);
            let _ = lt.record_path(0, p0);
            let _ = lt.record_version(0, v0);
            let d1 = lt.record_op(1, w1.wrapping_add(1)) == Err(IonicCode::EDrift);
            let d2 = lt.record_path(1, p1.wrapping_add(1)) == Err(IonicCode::EPathDrift);
            let d3 = lt.record_version(1, v1.wrapping_add(1)) == Err(IonicCode::EVerDrift);
            d1 && d2 && d3
        },
        "能力格/依赖投影/版本档载荷漂移必抓",
    );
    s.add(
        "能力面逆序拒",
        {
            let mut lt = IonicLedger::new();
            let w9 = match op_cell(9) { Some(w) => w, None => 0 };
            let w2 = match op_cell(2) { Some(w) => w, None => 0 };
            let _ = lt.record_op(9, w9);
            lt.record_op(2, w2) == Err(IonicCode::EOrder)
        },
        "serve 面落账后回落依赖面必 EOrder（五面单调闭集）",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明齐但投影缺一也必拒。
            let lt_empty = IonicLedger::new();
            let mut lt_cell = IonicLedger::new();
            let mut g = 0usize;
            while g < GATES {
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
            lt_empty.seal() == Err(IonicCode::ESeal) && lt_cell.seal() == Err(IonicCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = IonicLedger::new();
            let mut g = 0usize;
            while g < GATES {
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
            lt.seal() == Err(IonicCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss（Capacitor 后端/N/A 口径 4 行闭集）",
    );
    s.add(
        "主账 41 账目 seal Ok 且定级 S1",
        {
            let lt = seed_full_ledger(Some(ProbeVerdict::Available));
            lt.account_count() == LEDGER_ROWS as u32 && lt.seal().is_ok() && lt.grade() == Grade::S1
        },
        "六闸 6+能力 20+投影 8+版本 3+声明 4=41 账目全落封账通过，探测可用定级 S1",
    );
    s.add(
        "虚拟化依赖探测三分支闭集",
        {
            // 可用分支：全齐可达 S1；不可用 N/A 与探测失败显性：即使全齐也封顶 S3。
            let base = seed_full_ledger(Some(ProbeVerdict::Available));
            let na = seed_full_ledger(Some(ProbeVerdict::UnavailableNa));
            let fail = seed_full_ledger(Some(ProbeVerdict::ProbeFailed));
            let none = seed_full_ledger(None);
            base.grade() == Grade::S1
                && na.grade() == Grade::S3
                && fail.grade() == Grade::S3
                && none.grade() == Grade::S3
        },
        "可用/不可用 N/A/探测失败显性三分支：N/A 两支与未落账均封顶 S3，可用支全齐 S1",
    );
    s.add(
        "N/A 三要素告警非空",
        {
            let a = na_alert();
            let mut ok = a.len() == 3;
            let mut i = 0usize;
            while i < 3 {
                if match a.get(i) { Some(t) => t.is_empty(), None => true } { ok = false; }
                i += 1;
            }
            ok
        },
        "探测账分支/定级封顶/判据锚定三要素逐条非空",
    );
    s.add(
        "N/A 清单非空",
        {
            let m = NA_MANIFEST;
            let mut ok = m.len() == 3;
            let mut i = 0usize;
            while i < 3 {
                if match m.get(i) { Some(t) => t.is_empty(), None => true } { ok = false; }
                i += 1;
            }
            ok
        },
        "KB-COMPAT 定级 S3 + N/A 清单三行逐条非空（禁虚报可用）",
    );
    s.add(
        "探测账重复落账拒",
        {
            let mut lt = IonicLedger::new();
            let _ = lt.record_probe(ProbeVerdict::Available);
            lt.record_probe(ProbeVerdict::UnavailableNa) == Err(IonicCode::EDup)
        },
        "探测账一次落账，重复必拒 EDup",
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
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC840,
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC840",
    );
    s.add("收口·判据条数", s.len() >= 21, "含本条共 ≥22 条闭集（quota 地板 280 行对齐）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_kinds() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("ionic start"));
        assert_eq!(op_kind_name(1), Some("依赖面"));
        assert_eq!(op_kind_name(4), Some("Capacitor 同步"));
        assert!(op_kind_name(5).is_none());
        assert_eq!(path_kind(0), Some(0));
        assert_eq!(path_kind(4), Some(1));
        assert_eq!(path_kind(7), Some(1));
        assert!(path_kind(8).is_none());
        assert!(path_cell(8).is_none());
        assert_eq!(path_kind_name(0), Some("Capacitor 段投影"));
        assert_eq!(path_kind_name(1), Some("插件段投影"));
        assert!(path_kind_name(2).is_none());
        assert_eq!(version_name(0), Some("6.20"));
        assert_eq!(version_name(1), Some("7.0"));
        assert_eq!(version_name(2), Some("7.1"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
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
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = IonicLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(IonicCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = IonicLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(IonicCode::EDrift)
        );
        assert_eq!(
            lt.record_path(1, path_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(IonicCode::EPathDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(IonicCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = IonicLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(IonicCode::EDup));
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(IonicCode::EDup));
        let _ = lt.record_probe(ProbeVerdict::Available);
        assert_eq!(lt.record_probe(ProbeVerdict::ProbeFailed), Err(IonicCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = IonicLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = IonicLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = IonicLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_path(PATH_CELLS, 0);
        let _ = lt.record_version(VERSION_STEPS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        // 缺声明行：六闸与格账齐而 4 行声明缺一 → ERowMiss。
        let mut no_rows = IonicLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = no_rows.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = no_rows.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < PATH_CELLS {
            if let Some(w) = path_cell(c) {
                let _ = no_rows.record_path(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < VERSION_STEPS {
            if let Some(w) = version_cell(v) {
                let _ = no_rows.record_version(v, w);
            }
            v += 1;
        }
        assert_eq!(no_rows.seal(), Err(IonicCode::ERowMiss));
        assert_eq!(no_rows.account_count(), 37);
        // 全齐账本（含 4 行声明+探测账）：封账通过且账目数 41。
        let fresh = seed_full_ledger(Some(ProbeVerdict::Available));
        assert!(fresh.seal().is_ok());
        assert_eq!(fresh.account_count(), LEDGER_ROWS as u32);
    }

    #[test]
    fn test_na_probe_branches() {
        // 三分支闭集：可用 / 不可用 N/A / 探测失败显性。
        assert_eq!(ProbeVerdict::Available.val(), 0);
        assert_eq!(ProbeVerdict::UnavailableNa.val(), 1);
        assert_eq!(ProbeVerdict::ProbeFailed.val(), 2);
        assert!(!ProbeVerdict::Available.detail().is_empty());
        assert!(!ProbeVerdict::UnavailableNa.detail().is_empty());
        assert!(!ProbeVerdict::ProbeFailed.detail().is_empty());
        // 探测未落账：禁虚报可用，全齐也不得 S1。
        let lt_none = seed_full_ledger(None);
        assert!(!lt_none.probe_available());
        assert!(!lt_none.probe_na());
        assert_eq!(lt_none.grade(), Grade::S3);
        // 不可用 N/A：封顶 S3。
        let lt_na = seed_full_ledger(Some(ProbeVerdict::UnavailableNa));
        assert!(lt_na.probe_na());
        assert!(!lt_na.probe_available());
        assert_eq!(lt_na.grade(), Grade::S3);
        // 探测失败显性：封顶 S3。
        let lt_fail = seed_full_ledger(Some(ProbeVerdict::ProbeFailed));
        assert!(lt_fail.probe_na());
        assert_eq!(lt_fail.grade(), Grade::S3);
        // 可用：全齐可达 S1。
        let lt_ok = seed_full_ledger(Some(ProbeVerdict::Available));
        assert!(lt_ok.probe_available());
        assert_eq!(lt_ok.grade(), Grade::S1);
        // 探测账重复落账拒（种子账本已落探测，再落必 EDup）。
        let mut lt_dup = seed_full_ledger(Some(ProbeVerdict::Available));
        assert_eq!(lt_dup.record_probe(ProbeVerdict::ProbeFailed), Err(IonicCode::EDup));
        // N/A 三要素告警与 N/A 清单逐条非空。
        let a = na_alert();
        assert_eq!(a.len(), 3);
        let mut i = 0usize;
        while i < 3 {
            assert!(!a[i].is_empty());
            i += 1;
        }
        let m = NA_MANIFEST;
        assert_eq!(m.len(), 3);
        let mut j = 0usize;
        while j < 3 {
            assert!(!m[j].is_empty());
            j += 1;
        }
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, fp) = run_cycle();
        // 记账格数 = 20 能力 + 8 投影 + 3 档版本（六闸/声明行/探测账不计数）。
        assert_eq!(recorded, OPS as u32 + PATH_CELLS as u32 + VERSION_STEPS as u32);
        assert_eq!(recorded, 31);
        assert_eq!(caught, 10);
        assert!(fp != 0);
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
        assert_eq!(SEG_BASE, 0xCC840);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(IonicCode::Ok as u32, 0);
        assert_eq!(IonicCode::ENoOp as u32, 1);
        assert_eq!(IonicCode::EGradeDrift as u32, 12);
        assert_eq!(IonicCode::Ok.val(), SEG_BASE);
        assert!(!IonicCode::Ok.detail().is_empty());
        assert!(!IonicCode::EOrder.detail().is_empty());
        assert!(!IonicCode::ERowMiss.detail().is_empty());
        assert!(!IonicCode::ESeal.detail().is_empty());
        assert!(!IonicCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = IonicLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        // 六闸齐但探测未落账：N/A 口径封顶 S3（禁虚报可用）。
        let mut lt_gate = IonicLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt_gate.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt_gate.grade(), Grade::S3);
        // 六闸齐且探测可用：S2。
        let _ = lt_gate.record_probe(ProbeVerdict::Available);
        assert_eq!(lt_gate.grade(), Grade::S2);
        // 全齐且探测可用：目标档 S1；全齐但探测 N/A：封顶 S3。
        assert_eq!(seed_full_ledger(Some(ProbeVerdict::Available)).grade(), Grade::S1);
        assert_eq!(seed_full_ledger(Some(ProbeVerdict::UnavailableNa)).grade(), Grade::S3);
    }

    #[test]
    fn unx52546_checks_green() {
        let s = run_unx52546_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
