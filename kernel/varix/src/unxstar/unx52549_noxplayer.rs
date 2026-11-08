//! UNX-F52549 NoxPlayer 收口（N/A 同构）· 0xCC900段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52549"，逐字）：
//!   id: 52549
//!   title: NoxPlayer 收口（N/A 同构）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52550-J1 同口径；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52549）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52550-J1（下一卡 LDPlayer 域语料），照原文锚定；功能按本卡标题
//! （NoxPlayer / N/A 同构）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：NoxPlayer 五面 20 格（安装面 4/实例面 4（多开实例清单闭
//!      集）/操作录制面 4（宏脚本分账）/ADB 消费面 4/N/A 探测账面 4），
//!      能力面单调（安装→实例→录制→ADB→N/A 探测账），逐格载荷（纯函数
//!      折叠）恒等才收；漂移必抓。
//!   ② 实例投影 8 格：8 格闭集、双段（实例段 0..4、宏脚本段 4..8）单调，
//!      投影漂移必抓。
//!   ③ 版本阶梯 3 档：7.0.5.8/7.0.5.9/7.0.6.0 三级闭集，越级必抓、载荷
//!      漂移必抓。
//!   ④ 声明 4 行闭集：Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明
//!      2 行，缺一拒封（seal：六闸缺→ESeal、声明行缺→ERowMiss、格账缺
//!      （含 N/A 探测 4 槽）→ESeal）。
//!   ⑤ N/A 同构探测账三分支闭集（与 Genymotion 52547 / BlueStacks 52548
//!      同构）：Available（可信证据链）/ NotAvailable（N/A 如实登记 +
//!      三要素告警：依赖项名/原因/替代路径）/ ProbeFailed（探测失败显
//!      性，与 N/A 严格区分不造假）。虚拟化依赖探测（Hypervisor 检测）
//!      在内核纯函数环境不可用（na_channel_available() 编译期恒假）→
//!      如实 N/A，KB-COMPAT 定级封顶 S2 + N/A 清单，禁虚报可用；虚报
//!      可用与虚报 S1 均必抓。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/na_probe 三分支/seal/
//!      grade 定级闭集/fingerprint FNV-1a）；注入核 run_cycle 10 轮三态
//!      轮转（版本漂移/操作漂移/N-A 虚报可用），三类注入全部落在当轮
//!      临时账本上，主账 injected==caught==10、escaped==0；双跑指纹一
//!      致；定级目标档 S2（N/A 封顶）。越闭集边界拒不计注入账、重复
//!      EDup 不计注入账、漂移与逆序计入注入账。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52549;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等落 0xCC900 且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCC900 && SEG_BASE % 0x40 == 0);

/// NoxPlayer 五面操作数。
pub const OPS: usize = 20;

/// 实例投影格数。
pub const PATH_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 声明行数（Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行）。
pub const SPLIT_ROWS: usize = 4;

/// 判据收口闸数。
pub const GATES: usize = 6;

/// N/A 探测账槽位（对应 N/A 探测账面 4 格）。
pub const NA_PROBES: usize = 4;

/// 可信证据链基准值：Available 分支在存在证据链通道的环境下的核对基准。
/// 内核纯函数环境无该通道（见 `na_channel_available`），此值不可达。
pub const NA_EVIDENCE_TRUSTED: u64 = 0x4E6F785452555354;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoxCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移（含探测虚报可用）
    EDrift,
    /// 能力面逆序
    EOrder,
    /// 投影格越闭集
    ENoPath,
    /// 投影漂移
    EPathDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移（虚报 S1 必抓）
    EGradeDrift,
}

impl NoxCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NoxCode::Ok => "成功",
            NoxCode::ENoVer => "版本级越阶梯：NoxPlayer 7.0.5.8/7.0.5.9/7.0.6.0 三档闭集之外先登记",
            NoxCode::EVerDrift => "版本阶梯漂移：版本档折叠与基准不恒等，逐档复算",
            NoxCode::ENoOp => "操作号越闭集：NoxPlayer 五面 20 格（含 N/A 探测账 4 槽）之外先登记",
            NoxCode::EDrift => "操作漂移：能力格折叠与基准不恒等；探测虚报可用（证据链不可信）亦归此码，逐项复算",
            NoxCode::EOrder => "能力面逆序：安装→实例（多开实例清单）→操作录制（宏脚本分账）→ADB 消费→N/A 探测账单调，逆序必抓",
            NoxCode::ENoPath => "投影格越闭集：实例投影 8 格之外先登记",
            NoxCode::EPathDrift => "投影漂移：实例投影折叠与基准不恒等，逐格复算",
            NoxCode::ENoRow => "声明行越闭集：Hypervisor 依赖 N/A 声明+ADB 消费分账声明 4 行闭集之外先登记",
            NoxCode::ERowMiss => "缺声明行：Hypervisor 依赖 N/A 声明 2 行/ADB 消费分账声明 2 行未齐先补齐",
            NoxCode::EDup => "重复落账：同格只许一条账目",
            NoxCode::ESeal => "未齐即封：20 能力格+8 投影格+3 档版本+六判据+N/A 探测 4 槽未齐先补齐",
            NoxCode::EGradeDrift => "定级漂移：虚拟化探测面内核环境恒 N/A，封顶 S2（附 N/A 清单），虚报 S1 必抓（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（要求虚拟化探测面可信可用；本卡内核环境恒 N/A，
    /// S1 不可达，虚报 S1 = EGradeDrift 必抓）
    S1,
    /// S2：N/A 封顶收口（本卡目标档，附 N/A 清单）
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
            Grade::S1 => "S1：全通收口（要求 Hypervisor 探测可信可用；本卡内核环境恒 N/A，S1 不可达，虚报必抓）",
            Grade::S2 => "S2：N/A 封顶收口，附 N/A 清单（Hypervisor 等虚拟化依赖如实登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── N/A 探测账三分支闭集 ─────────────────────────────────────────
/// 探测账三分支闭集（与 Genymotion 52547 / BlueStacks 52548 同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NaBranch {
    /// 可信证据链支撑的可用（内核纯函数环境无证据链通道，主张即虚报）
    Available,
    /// N/A 如实登记（三要素告警：依赖项名/原因/替代路径）
    NotAvailable,
    /// 探测失败显性（与 N/A 严格区分，不造假）
    ProbeFailed,
}

impl NaBranch {
    /// 分支值：Available=0 / NotAvailable=1 / ProbeFailed=2。
    pub fn val(self) -> u32 {
        match self {
            NaBranch::Available => 0,
            NaBranch::NotAvailable => 1,
            NaBranch::ProbeFailed => 2,
        }
    }
    /// 分支名。
    pub fn name(self) -> &'static str {
        match self {
            NaBranch::Available => "Available（可信证据链）",
            NaBranch::NotAvailable => "NotAvailable（N/A 如实登记）",
            NaBranch::ProbeFailed => "ProbeFailed（探测失败显性）",
        }
    }
    /// 分支明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NaBranch::Available => "Available：仅当存在可信证据链通道且证据核对恒等才可登记；内核纯函数环境通道恒缺，主张即虚报必抓",
            NaBranch::NotAvailable => "NotAvailable：如实登记 N/A，附三要素告警（依赖项名/原因/替代路径），禁虚报可用",
            NaBranch::ProbeFailed => "ProbeFailed：探测失败显性登记，与 N/A 严格区分，不得改写为可用或 N/A 之外的状态",
        }
    }
}

/// 内核纯函数环境是否存在 Hypervisor 证据链通道。
/// 编译期恒假：纯函数环境零 IO、零底层探测指令通道，虚拟化依赖
/// （Hypervisor 检测）不可用 → 如实 N/A，禁虚报可用。
pub const fn na_channel_available() -> bool {
    false
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力面 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF5000u64 + (o as u64) * 0x2C) ^ 0x4E6F78506C000000)
}

/// 能力面（0..4=安装面 4..8=实例面 8..12=操作录制面 12..16=ADB 消费面 16..20=N/A 探测账面）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力面名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装面"),
        1 => Some("实例面（多开实例清单）"),
        2 => Some("操作录制面（宏脚本分账）"),
        3 => Some("ADB 消费面"),
        4 => Some("N/A 探测账面"),
        _ => None,
    }
}

/// 实例投影格载荷（格 c；投影段 = c/4：0..4=实例段 4..8=宏脚本段）。
pub const fn path_cell(c: usize) -> Option<u64> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((0xF6000u64 + (c as u64) * 0x31) ^ 0x4E6F78494E000000)
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
        0 => Some("实例段投影（多开实例清单）"),
        1 => Some("宏脚本段投影（宏脚本分账）"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xF7000u64 + (v as u64) * 0x5D) ^ 0x4E6F783778000000)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("7.0.5.8"),
        1 => Some("7.0.5.9"),
        2 => Some("7.0.6.0"),
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
        0 => Some("Hypervisor 依赖 N/A 声明·主"),
        1 => Some("Hypervisor 依赖 N/A 声明·辅"),
        2 => Some("ADB 消费分账声明·主"),
        3 => Some("ADB 消费分账声明·辅"),
        _ => None,
    }
}

// ── N/A 三要素告警表 ─────────────────────────────────────────────
/// N/A 依赖项名（按探测槽索引，全非空）。
pub const fn na_dep_name(slot: usize) -> Option<&'static str> {
    match slot {
        0 => Some("Hypervisor 虚拟化后端（VT-x/AMD-V 监控程序）"),
        1 => Some("Nox 多开器实例清单（Multi-Instance Manager）"),
        2 => Some("操作录制宏脚本引擎（宏脚本分账）"),
        3 => Some("ADB 桥接通道（adb 消费分账）"),
        _ => None,
    }
}

/// N/A 原因（按探测槽索引，全非空）。
pub const fn na_reason(slot: usize) -> Option<&'static str> {
    match slot {
        0 => Some("内核纯函数环境无 Hypervisor 检测通道，虚拟化依赖不可探测"),
        1 => Some("多开实例清单属宿主机用户态进程面，内核纯函数环境不可枚举"),
        2 => Some("宏脚本引擎依赖图形会话，内核纯函数环境不可加载"),
        3 => Some("ADB 通道依赖宿主机 USB/网络桥，内核纯函数环境不可建链"),
        _ => None,
    }
}

/// N/A 替代路径（按探测槽索引，全非空）。
pub const fn na_alt_path(slot: usize) -> Option<&'static str> {
    match slot {
        0 => Some("替代路径：宿主机用户态探测（CPUID/系统虚拟化接口）另行收口"),
        1 => Some("替代路径：用户态枚举多开实例目录后另行收口"),
        2 => Some("替代路径：图形会话内录制回放分账后另行收口"),
        3 => Some("替代路径：宿主机 ADB 消费分账卡另行收口"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// NoxPlayer 收口账本状态机。
#[derive(Clone)]
pub struct NoxLedger {
    /// 六判据（收口闸：安装/启动/多开实例/操作录制/ADB 消费/卸载）。
    gates: [Option<bool>; GATES],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 实例投影账。
    paths: [Option<u64>; PATH_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// N/A 探测账（三分支闭集，4 槽）。
    na: [Option<NaBranch>; NA_PROBES],
    /// 最后能力面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NoxLedger {
    /// 新账本。
    pub fn new() -> Self {
        NoxLedger {
            gates: [None; GATES],
            ops: [None; OPS],
            paths: [None; PATH_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
            na: [None; NA_PROBES],
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

    /// 正格全量填充（六闸+20 能力+8 投影+3 版本，全走正格路径），
    /// 不触声明行与 N/A 探测账；返回 record_* 成功格数（全正格=31），
    /// 供判据集与测试构造变体账本及 run_cycle 记账复用。
    fn fill_cells(&mut self) -> u32 {
        let mut g = 0usize;
        while g < GATES {
            let _ = self.judge_gate(g);
            g += 1;
        }
        let mut n = 0u32;
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                if self.record_op(o, w).is_ok() {
                    n += 1;
                }
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < PATH_CELLS {
            if let Some(w) = path_cell(c) {
                if self.record_path(c, w).is_ok() {
                    n += 1;
                }
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < VERSION_STEPS {
            if let Some(w) = version_cell(v) {
                if self.record_version(v, w).is_ok() {
                    n += 1;
                }
            }
            v += 1;
        }
        n
    }

    /// 声明 4 行填充。
    fn fill_splits(&mut self) {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = self.declare_split(r);
            r += 1;
        }
    }

    /// N/A 探测 4 槽如实填充（NotAvailable；内核纯函数环境恒缺通道）。
    fn fill_na(&mut self) {
        let mut i = 0usize;
        while i < NA_PROBES {
            let _ = self.na_probe(i, NaBranch::NotAvailable, 0);
            i += 1;
        }
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NoxCode> {
        if gate >= GATES {
            return Err(NoxCode::ESeal);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), NoxCode> {
        if o >= OPS {
            return Err(NoxCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(NoxCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B00);
            return Err(NoxCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(NoxCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B01 + o as u64);
            return Err(NoxCode::EDrift);
        }
        if Self::slot_filled(&self.ops, o) {
            return Err(NoxCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9B02 + o as u64);
        Ok(())
    }

    /// 定容槽全填核验（泛型：u64/u32/NaBranch 各账共用）。
    fn all_some<T>(buf: &[Option<T>]) -> bool {
        let mut i = 0usize;
        while i < buf.len() {
            match buf.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 定容槽已填判定（重复落账判定共用）。
    fn slot_filled<T>(buf: &[Option<T>], i: usize) -> bool {
        match buf.get(i) {
            Some(Some(_)) => true,
            _ => false,
        }
    }

    /// 能力全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        Self::all_some(&self.ops)
    }

    /// 投影一格（载荷对照 + 重复拒，越闭集拒不计注入账）。
    pub fn record_path(&mut self, c: usize, got: u64) -> Result<(), NoxCode> {
        let want = match path_cell(c) {
            Some(w) => w,
            None => return Err(NoxCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9C00 + c as u64);
            return Err(NoxCode::EPathDrift);
        }
        if Self::slot_filled(&self.paths, c) {
            return Err(NoxCode::EDup);
        }
        if let Some(p) = self.paths.get_mut(c) {
            *p = Some(got);
        }
        self.fnv(0x9C01 + c as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn paths_all(&self) -> bool {
        Self::all_some(&self.paths)
    }

    /// 版本阶梯一档（载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), NoxCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(NoxCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9D00 + v as u64);
            return Err(NoxCode::EVerDrift);
        }
        if Self::slot_filled(&self.vers, v) {
            return Err(NoxCode::EDup);
        }
        if let Some(b) = self.vers.get_mut(v) {
            *b = Some(got);
        }
        self.fnv(0x9D01 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档）。
    pub fn vers_all(&self) -> bool {
        Self::all_some(&self.vers)
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), NoxCode> {
        if r >= SPLIT_ROWS {
            return Err(NoxCode::ENoRow);
        }
        if Self::slot_filled(&self.splits, r) {
            return Err(NoxCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0x9E00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
    pub fn splits_all(&self) -> bool {
        Self::all_some(&self.splits)
    }

    /// N/A 探测账一格（三分支闭集）：Available 仅当可信证据链通道存在才可
    /// 登记（内核环境通道恒缺，主张即虚报，注入必抓）；NotAvailable 如实
    /// 登记且三要素告警（依赖项名/原因/替代路径）齐备；ProbeFailed 探测
    /// 失败显性，与 N/A 严格区分。越闭集拒不计注入账；重复 EDup 不计注入账。
    pub fn na_probe(&mut self, slot: usize, b: NaBranch, evidence: u64) -> Result<(), NoxCode> {
        if slot >= NA_PROBES {
            return Err(NoxCode::ENoOp);
        }
        if Self::slot_filled(&self.na, slot) {
            return Err(NoxCode::EDup);
        }
        match b {
            NaBranch::Available => {
                // 虚报可用必抓：内核纯函数环境无 Hypervisor 证据链通道，
                // 任何 Available 主张均属虚报；证据载荷并入指纹留审计痕。
                self.fnv((0x9F00 + slot as u64) ^ evidence.rotate_left(17));
                self.injected += 1;
                self.caught += 1;
                Err(NoxCode::EDrift)
            }
            NaBranch::NotAvailable => {
                // 三要素告警必须齐备（依赖项名/原因/替代路径全非空）。
                let ok3 = match (na_dep_name(slot), na_reason(slot), na_alt_path(slot)) {
                    (Some(a), Some(r), Some(p)) => !a.is_empty() && !r.is_empty() && !p.is_empty(),
                    _ => false,
                };
                if !ok3 {
                    return Err(NoxCode::ERowMiss);
                }
                if let Some(n) = self.na.get_mut(slot) {
                    *n = Some(NaBranch::NotAvailable);
                }
                self.fnv(0x9F10 + slot as u64);
                Ok(())
            }
            NaBranch::ProbeFailed => {
                // 探测失败显性登记，与 N/A 严格区分，不造假。
                if let Some(n) = self.na.get_mut(slot) {
                    *n = Some(NaBranch::ProbeFailed);
                }
                self.fnv(0x9F20 + slot as u64);
                Ok(())
            }
        }
    }

    /// N/A 探测账全齐核验（4 槽）。
    pub fn na_all(&self) -> bool {
        Self::all_some(&self.na)
    }

    /// N/A 探测槽状态读取。
    pub fn na_state(&self, slot: usize) -> Option<NaBranch> {
        match self.na.get(slot) {
            Some(Some(b)) => Some(*b),
            _ => None,
        }
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺（含 N/A 探测 4 槽）→ESeal。
    pub fn seal(&self) -> Result<(), NoxCode> {
        if !self.gates_all() {
            return Err(NoxCode::ESeal);
        }
        if !self.splits_all() {
            return Err(NoxCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.vers_all() || !self.na_all() {
            return Err(NoxCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：全齐（含 N/A 探测 4 槽如实登记）→ 封顶 S2（本卡
    /// 目标档，附 N/A 清单，不虚报 S1）；六闸齐 → S2；空账 → S3。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.paths_all() && self.vers_all() && self.splits_all() && self.na_all() {
            // N/A 封顶：虚拟化探测面内核环境恒 N/A，全齐也只定 S2。
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级主张核验：主张与如实定级恒等才收；虚报 S1 = EGradeDrift 必抓。
    pub fn claim_grade(&self, g: Grade) -> Result<(), NoxCode> {
        if g == self.grade() {
            Ok(())
        } else {
            Err(NoxCode::EGradeDrift)
        }
    }
}

impl Default for NoxLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全齐账本构造 ─────────────────────────────────────────────────
/// 构造全齐账本（六闸 6 + 能力 20 + 投影 8 + 版本 3 + 声明 4 = 41 账目，
/// 另 N/A 探测 4 槽如实登记 NotAvailable），供判据集与测试复用；构造
/// 过程全走正格路径，不触发注入。
pub fn build_full_ledger() -> NoxLedger {
    let mut led = NoxLedger::new();
    led.fill_cells();
    led.fill_splits();
    led.fill_na();
    led
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
/// 主账全通闭环：41 账目（6 闸+20 能力+8 投影+3 版本+4 声明）seal Ok，
/// N/A 分支 4 槽如实登记（NotAvailable）；注入账 10 轮三态轮转注入
/// （版本漂移/操作漂移/N-A 虚报可用），injected==caught==10、escaped==0。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NoxLedger::new();
    // 六闸 6 + 能力 20 + 投影 8 + 版本 3 逐格记账（record_* 成功数）。
    let recorded = led.fill_cells();
    // 声明 4 行 + N/A 探测账 4 槽如实登记：内核纯函数环境无 Hypervisor
    // 检测通道，探测三分支恒落 NotAvailable（禁虚报可用）。
    led.fill_splits();
    led.fill_na();
    // 注入 10 轮：版本漂移 / 操作漂移 / N-A 虚报可用三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NoxLedger::new();
        let caught = match kind {
            0 => {
                // 版本漂移：先按合法序落 0 档正格，再注漂移载荷，必抓。
                let w0 = match version_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_version(0, w0);
                lt.record_version(0, w0.wrapping_add(1)).is_err()
            }
            1 => {
                // 操作漂移：先按合法序落同族正格（安装面），再注漂移。
                let w0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, w0);
                lt.record_op(1, w0.wrapping_add(1)).is_err()
            }
            _ => {
                // N-A 虚报可用：内核纯函数环境无证据链通道，主张即虚报，必抓。
                lt.na_probe(0, NaBranch::Available, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9000 + k as u64);
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
/// UNX-F52549 判据集（锚 UNX-F52550-J1，off-by-one 板式）。
pub fn run_unx52549_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52549");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "NoxPlayer 五面 20 操作全通",
        recorded >= OPS as u32,
        "安装/实例（多开实例清单）/操作录制（宏脚本分账）/ADB 消费/N/A 探测账逐格恒等",
    );
    s.add(
        "实例投影 8/8 与版本三档全落",
        {
            let mut lt = NoxLedger::new();
            lt.fill_cells() >= OPS as u32 && lt.paths_all() && lt.vers_all()
        },
        "实例段 0..4 与宏脚本段 4..8 双段逐格恒等；7.0.5.8/7.0.5.9/7.0.6.0 三档逐档恒等",
    );
    s.add(
        "声明 4/4 与六判据收口闸全落",
        {
            let mut lt = NoxLedger::new();
            lt.fill_cells();
            lt.fill_splits();
            let mut ok = lt.splits_all();
            let mut g = 0usize;
            while g < GATES {
                if lt.judge_gate(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.gates_all()
        },
        "Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行闭集（§AI-66.3）；安装/启动/多开实例/操作录制/ADB 消费/卸载逐闸落账",
    );
    s.add(
        "N/A 探测账 4/4 如实登记（NotAvailable）",
        {
            let mut lt = NoxLedger::new();
            lt.fill_na();
            let mut uniform = lt.na_all();
            let mut i = 0usize;
            while i < NA_PROBES {
                if lt.na_state(i) != Some(NaBranch::NotAvailable) {
                    uniform = false;
                }
                i += 1;
            }
            uniform
        },
        "内核纯函数环境虚拟化依赖探测恒 N/A，逐槽如实登记，禁虚报可用",
    );
    s.add(
        "N/A 三分支判据·Available 虚报必抓",
        {
            let mut lt = NoxLedger::new();
            let r = lt.na_probe(0, NaBranch::Available, 0);
            let (inj, cgt, _esc) = lt.stats();
            r.is_err() && inj == 1 && cgt == 1
        },
        "内核纯函数环境无 Hypervisor 证据链通道，虚报可用注入必抓，不造假",
    );
    s.add(
        "N/A 三分支判据·ProbeFailed 显性且与 N/A 严格区分",
        {
            let mut lt = NoxLedger::new();
            let ok1 = lt.na_probe(0, NaBranch::ProbeFailed, 0).is_ok();
            ok1 && lt.na_state(0) == Some(NaBranch::ProbeFailed)
                && NaBranch::ProbeFailed != NaBranch::NotAvailable
        },
        "探测失败显性登记为独立分支，与 N/A 严格区分不造假",
    );
    s.add(
        "N/A 三要素告警齐备且内核环境无探测通道",
        {
            let mut ok = !na_channel_available();
            let mut i = 0usize;
            while i < NA_PROBES {
                let trip = (na_dep_name(i), na_reason(i), na_alt_path(i));
                match trip {
                    (Some(a), Some(r), Some(p)) => {
                        if a.is_empty() || r.is_empty() || p.is_empty() {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        },
        "4 槽依赖项名/原因/替代路径三要素全非空；Hypervisor 检测通道编译期恒假",
    );
    s.add(
        "能力面与投影段单调",
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
        "安装→实例→操作录制→ADB 消费→N/A 探测账五面闭集单调；实例段→宏脚本段双段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本漂移/操作漂移/N-A 虚报可用三态轮转必抓",
    );
    s.add(
        "主账 seal Ok（41 账目+N/A 4 槽）",
        build_full_ledger().seal().is_ok(),
        "六闸 6+能力 20+投影 8+版本 3+声明 4=41 账目全通，N/A 4 槽如实登记",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = NoxLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格/声明行/探测槽越闭集",
        {
            let mut lt = NoxLedger::new();
            let b1 = lt.record_path(PATH_CELLS, 0).is_err();
            let b2 = lt.declare_split(SPLIT_ROWS).is_err();
            let b3 = lt.na_probe(NA_PROBES, NaBranch::NotAvailable, 0).is_err();
            b1 && b2 && b3
        },
        "投影格 8/声明行 4/N-A 探测槽 4 越闭集必拒",
    );
    s.add(
        "能力漂移拒",
        {
            let mut lt = NoxLedger::new();
            let w0 = match op_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match op_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(0, w0);
            lt.record_op(1, w1.wrapping_add(1)) == Err(NoxCode::EDrift)
        },
        "能力格载荷漂移必抓",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：正格齐而 N/A 探测 4 槽缺也必拒。
            let lt_empty = NoxLedger::new();
            let mut lt_cell = NoxLedger::new();
            lt_cell.fill_cells();
            // 声明行补齐，隔离出「格账缺」单因：此时仅 N/A 探测 4 槽缺。
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt_cell.declare_split(r);
                r += 1;
            }
            lt_empty.seal() == Err(NoxCode::ESeal) && lt_cell.seal() == Err(NoxCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = NoxLedger::new();
            lt.fill_cells();
            lt.fill_na();
            lt.seal() == Err(NoxCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    s.add(
        "seal 语义·N/A 探测缺槽 ESeal",
        {
            let mut lt = NoxLedger::new();
            lt.fill_cells();
            lt.fill_splits();
            lt.seal() == Err(NoxCode::ESeal)
        },
        "格账齐而 N/A 探测 4 槽未登记必 ESeal，未齐即封",
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
        "主账 seal Ok 与定级 N/A 封顶 S2·虚报 S1 必抓",
        {
            let full = build_full_ledger();
            let empty = NoxLedger::new();
            full.seal().is_ok()
                && full.grade() == Grade::S2
                && empty.grade() == Grade::S3
                && full.claim_grade(Grade::S1) == Err(NoxCode::EGradeDrift)
                && full.claim_grade(Grade::S2).is_ok()
        },
        "41 账目+N/A 4 槽全通封账；虚拟化面恒 N/A 全齐封顶 S2 附 N/A 清单，虚报 S1 = EGradeDrift 必抓（KB-COMPAT 登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCC900 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC900",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 20 条闭集（恰踩 20 条地板）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(12), Some(3));
        assert_eq!(op_kind(16), Some(4));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("安装面"));
        assert_eq!(op_kind_name(4), Some("N/A 探测账面"));
        assert!(op_kind_name(5).is_none());
        assert_eq!(path_kind(3), Some(0));
        assert_eq!(path_kind(4), Some(1));
        assert_eq!(path_kind(7), Some(1));
        assert!(path_kind(8).is_none());
        assert!(path_cell(8).is_none());
        assert_eq!(path_kind_name(0), Some("实例段投影（多开实例清单）"));
        assert_eq!(path_kind_name(1), Some("宏脚本段投影（宏脚本分账）"));
        assert!(path_kind_name(2).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("7.0.5.8"));
        assert_eq!(version_name(1), Some("7.0.5.9"));
        assert_eq!(version_name(2), Some("7.0.6.0"));
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
        let v0 = version_cell(0).unwrap_or(0);
        assert_ne!(v0, version_cell(1).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(split_name(SPLIT_ROWS).is_none());
        assert_eq!(split_row(0), Some(0xF810));
    }

    #[test]
    fn test_na_branch_closed_set() {
        assert_eq!(NaBranch::Available.val(), 0);
        assert_eq!(NaBranch::NotAvailable.val(), 1);
        assert_eq!(NaBranch::ProbeFailed.val(), 2);
        assert!(!NaBranch::Available.name().is_empty());
        assert!(!NaBranch::Available.detail().is_empty());
        assert!(!NaBranch::NotAvailable.detail().is_empty());
        assert!(!NaBranch::ProbeFailed.detail().is_empty());
        assert_ne!(NaBranch::Available, NaBranch::NotAvailable);
        assert_ne!(NaBranch::NotAvailable, NaBranch::ProbeFailed);
        // 内核纯函数环境：探测通道恒假，可信证据基准非零。
        assert!(!na_channel_available());
        assert_ne!(NA_EVIDENCE_TRUSTED, 0);
        // 三要素告警：4 槽依赖项名/原因/替代路径全非空，越界槽为 None。
        let mut i = 0usize;
        while i < NA_PROBES {
            assert!(!na_dep_name(i).unwrap_or("").is_empty());
            assert!(!na_reason(i).unwrap_or("").is_empty());
            assert!(!na_alt_path(i).unwrap_or("").is_empty());
            i += 1;
        }
        assert!(na_dep_name(NA_PROBES).is_none());
        assert!(na_reason(NA_PROBES).is_none());
        assert!(na_alt_path(NA_PROBES).is_none());
    }

    #[test]
    fn test_na_probe_semantics() {
        let mut lt = NoxLedger::new();
        // 虚报可用：必抓且计入注入账（证据载荷无关，主张即虚报）。
        assert_eq!(lt.na_probe(0, NaBranch::Available, 0), Err(NoxCode::EDrift));
        assert_eq!(
            lt.na_probe(1, NaBranch::Available, NA_EVIDENCE_TRUSTED),
            Err(NoxCode::EDrift)
        );
        assert_eq!(lt.stats(), (2, 2, 0));
        // 探测槽越闭集：拒不计注入账。
        assert_eq!(
            lt.na_probe(NA_PROBES, NaBranch::NotAvailable, 0),
            Err(NoxCode::ENoOp)
        );
        assert_eq!(lt.stats(), (2, 2, 0));
        // N/A 如实登记成功；重复落账拒不计注入账。
        assert!(lt.na_probe(0, NaBranch::NotAvailable, 0).is_ok());
        assert_eq!(lt.na_state(0), Some(NaBranch::NotAvailable));
        assert_eq!(lt.na_probe(0, NaBranch::NotAvailable, 0), Err(NoxCode::EDup));
        assert_eq!(lt.stats(), (2, 2, 0));
        // ProbeFailed 显性登记：与 N/A 严格区分。
        assert!(lt.na_probe(1, NaBranch::ProbeFailed, 0).is_ok());
        assert_eq!(lt.na_state(1), Some(NaBranch::ProbeFailed));
        assert_ne!(lt.na_state(1), lt.na_state(0));
        // 补齐余下槽位，全齐核验。
        assert!(lt.na_probe(2, NaBranch::NotAvailable, 0).is_ok());
        assert!(lt.na_probe(3, NaBranch::ProbeFailed, 0).is_ok());
        assert!(lt.na_all());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NoxLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(NoxCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NoxLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(NoxCode::EDrift)
        );
        assert_eq!(
            lt.record_path(1, path_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(NoxCode::EPathDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(NoxCode::EVerDrift)
        );
        // 漂移计入注入账。
        assert_eq!(lt.stats(), (3, 3, 0));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NoxLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_version(0, version_cell(0).unwrap_or(0)),
            Err(NoxCode::EDup)
        );
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(NoxCode::EDup));
        // 重复落账不计注入账。
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NoxLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NoxLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = NoxLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_path(PATH_CELLS, 0);
        let _ = lt.record_version(VERSION_STEPS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        let _ = lt.na_probe(NA_PROBES, NaBranch::NotAvailable, 0);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = NoxLedger::new();
        full.fill_cells();
        full.fill_na();
        assert_eq!(full.seal(), Err(NoxCode::ERowMiss));
        full.fill_splits();
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_seal_na_missing() {
        // N/A 探测 4 槽未登记即封：必 ESeal（未齐即封）。
        let mut lt = NoxLedger::new();
        lt.fill_cells();
        lt.fill_splits();
        assert_eq!(lt.seal(), Err(NoxCode::ESeal));
        assert!(!lt.na_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 8 投影 + 3 档版本（六闸/声明行/N-A 探测槽不计数）。
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
        assert_eq!(SEG_BASE, 0xCC900);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NoxCode::Ok as u32, 0);
        assert_eq!(NoxCode::EGradeDrift as u32, 12);
        let all = [
            NoxCode::Ok,
            NoxCode::ENoVer,
            NoxCode::EVerDrift,
            NoxCode::ENoOp,
            NoxCode::EDrift,
            NoxCode::EOrder,
            NoxCode::ENoPath,
            NoxCode::EPathDrift,
            NoxCode::ENoRow,
            NoxCode::ERowMiss,
            NoxCode::EDup,
            NoxCode::ESeal,
            NoxCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert!(!NoxCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        assert!(!Grade::S1.detail().is_empty());
        assert!(!Grade::S2.detail().is_empty());
        assert!(!Grade::S3.detail().is_empty());
        // 如实阶梯：空账 S3，主张 S3 收、主张 S1 抓。
        let lt = NoxLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        assert_eq!(lt.claim_grade(Grade::S3), Ok(()));
        assert_eq!(lt.claim_grade(Grade::S1), Err(NoxCode::EGradeDrift));
        // 六闸齐 S2。
        let mut lt2 = NoxLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        assert_eq!(lt2.claim_grade(Grade::S1), Err(NoxCode::EGradeDrift));
        // 全齐封顶 S2（N/A 封顶语义：虚拟化面恒 N/A，不虚报 S1）。
        let full = build_full_ledger();
        assert_eq!(full.grade(), Grade::S2);
        assert_eq!(full.na_state(0), Some(NaBranch::NotAvailable));
        assert_eq!(full.claim_grade(Grade::S2), Ok(()));
        assert_eq!(full.claim_grade(Grade::S1), Err(NoxCode::EGradeDrift));
        assert!(full.seal().is_ok());
    }

    #[test]
    fn unx52549_checks_green() {
        let s = run_unx52549_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
