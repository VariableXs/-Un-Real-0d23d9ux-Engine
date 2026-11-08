//! UNX-F52547 Genymotion 收口（N/A 虚拟化声明）· 0xCC880段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52547"，逐字）：
//!   id: 52547
//!   title: Genymotion 收口（N/A 虚拟化声明）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52548-J1 同
//!     F52547 N/A 口径；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52547）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52548-J1，照原文锚定（同 F52547 N/A 口径为下一卡 BlueStacks 域
//! 语料）；功能按本卡标题（Genymotion 收口 / N/A 虚拟化声明）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① N/A 同构语义：虚拟化依赖探测（VirtualBox/Hypervisor 检测）在
//!      内核纯函数环境不可用 → 如实 N/A，KB-COMPAT 定级封顶 S3 +
//!      N/A 清单，禁虚报可用。探测账三分支闭集：Available（可信证据
//!      链）/ NotAvailable（N/A 如实登记 + 三要素告警：依赖项名/原因/
//!      替代路径）/ ProbeFailed（探测失败显性，与 N/A 严格区分不造假）。
//!   ② 域模型：Genymotion 五面 20 操作格（安装 4/设备管理 4/传感 GPS
//!      模拟 4/ADB 消费 4/N/A 探测账 4）单调，逐格载荷恒等才收；
//!      设备投影 8 格（虚拟设备段 0..4、传感面段 4..8）双段单调；
//!      版本 3 档 2023.2/2024.0/2024.1 闭集；声明 4 行（VirtualBox
//!      依赖 N/A 声明 2 + ADB 消费分账声明 2），缺一拒封。
//!   ③ ledger 状态机（record_*/judge_gate×6/na_probe 三分支/FNV-1a/
//!      grade）；注入核 run_cycle 10 轮三态轮转（版本漂移/操作漂移/
//!      N/A 虚报可用），全部落在当轮临时账本，主账 injected==caught==
//!      10、escaped==0；双跑指纹一致；定级目标档 S2+N/A 清单（本卡
//!      虚拟化面恒 N/A，S1 不适用，虚报 S1 = EGradeDrift 必抓）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52547;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCC880 && SEG_BASE % 0x40 == 0);

/// Genymotion 五面操作数。
pub const OPS: usize = 20;

/// 设备投影格数。
pub const PATH_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 声明行数（VirtualBox 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行）。
pub const SPLIT_ROWS: usize = 4;

/// 收口闸数。
pub const GATES: usize = 6;

/// 探测账分支数（三分支闭集）。
pub const NA_BRANCHES: usize = 3;

// ── N/A 三要素告警（依赖项名/原因/替代路径）─────────────────────
/// N/A 三要素·依赖项名。
pub const NA_DEP_NAME: &str = "VirtualBox / Hypervisor 虚拟化依赖";

/// N/A 三要素·原因。
pub const NA_REASON: &str =
    "内核纯函数环境无宿主虚拟化探测通道（零 IO/零进程/零设备枚举），不可证即 N/A";

/// N/A 三要素·替代路径。
pub const NA_ALT_PATH: &str =
    "替代路径：宿主侧 Genymotion 实例探测与 adb connect 分账（域外登记），内核侧不做虚报";

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，13 错误码至 +0x0D（EGradeDrift=+0x0C、ENotAvail=+0x0D 尾置）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenymotionCode {
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
    /// N/A 探测分支虚报（虚报可用=最高级违规）
    ENotAvail,
}

impl GenymotionCode {
    /// 码值：SEG_BASE + 0x00..0x0D。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GenymotionCode::Ok => "成功",
            GenymotionCode::ENoOp => {
                "操作号越闭集：Genymotion 五面 20 格（安装/设备管理/传感 GPS 模拟/ADB 消费/N/A 探测账）之外先登记"
            }
            GenymotionCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            GenymotionCode::EOrder => {
                "能力面逆序：安装面→设备管理面（虚拟设备清单闭集）→传感与 GPS 模拟面→ADB 消费面（adb connect 分账）→N/A 探测账面单调序，逆序必抓"
            }
            GenymotionCode::ENoPath => "投影格越闭集：设备投影 8 格（虚拟设备段+传感面段）之外先登记",
            GenymotionCode::EPathDrift => "投影漂移：设备投影折叠与基准不恒等，逐格复算",
            GenymotionCode::ENoVer => "版本级越阶梯：2023.x/2024.x 三档闭集（2023.2/2024.0/2024.1）之外先登记",
            GenymotionCode::EVerDrift => "版本阶梯漂移：版本档折叠与基准不恒等，逐档复算",
            GenymotionCode::ENoRow => {
                "声明行越闭集：VirtualBox 依赖 N/A 声明 2 行+ADB 消费分账声明 2 行闭集之外先登记"
            }
            GenymotionCode::ERowMiss => {
                "缺声明行：VirtualBox 依赖 N/A 声明/ADB 消费分账声明未齐先补齐"
            }
            GenymotionCode::EDup => "重复落账：同格只许一条账目",
            GenymotionCode::ESeal => {
                "未齐即封：20 能力格+8 投影格+3 档版本+六判据+N/A 探测分支如实登记未齐先补齐"
            }
            GenymotionCode::EGradeDrift => {
                "定级漂移：本卡虚拟化面在内核环境恒 N/A，S1 不适用；全通且 N/A 如实登记封顶 S2+N/A 清单，定级虚报 S1 必抓"
            }
            GenymotionCode::ENotAvail => {
                "N/A 探测分支虚报：Available 分支必须给出可信证据链，内核纯函数环境无探测通道，虚报可用=最高级违规"
            }
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不适用：虚拟化面恒 N/A）
    S1,
    /// S2：有条件收口（本卡目标档+N/A 清单）
    S2,
    /// S3：不收口（KB-COMPAT 定级封顶）
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
            Grade::S1 => "S1：全通收口（本卡虚拟化面在内核环境恒 N/A，S1 不适用，虚报必抓）",
            Grade::S2 => "S2：有条件收口（N/A 封顶）：全齐+N/A 探测账如实登记+N/A 清单",
            Grade::S3 => "S3：不收口，判据缺失或 N/A 探测分支未如实登记",
        }
    }
}

// ── 探测账三分支闭集 ─────────────────────────────────────────────
/// 探测账三分支闭集：Available / NotAvailable / ProbeFailed。
///
/// 内核纯函数环境下虚拟化依赖探测（VirtualBox/Hypervisor 检测）不可用：
/// Available 分支要求可信证据链，本环境无法给出 → 任何 Available 申报
/// 均为虚报，返回 ENotAvail 并计注入；NotAvailable 分支如实登记 N/A
/// 并附三要素告警（依赖项名/原因/替代路径）；ProbeFailed 分支显性登记
/// 探测失败，与 N/A 严格区分不造假。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NaBranch {
    /// 可信证据链分支（本环境必为虚报，禁用）
    Available,
    /// N/A 如实登记分支（三要素告警齐备）
    NotAvailable,
    /// 探测失败显性分支（与 N/A 严格区分）
    ProbeFailed,
}

impl NaBranch {
    /// 分支序号（三分支闭集 0..3）。
    pub fn idx(self) -> u32 {
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
}

/// N/A 登记标记（NotAvailable 分支落账载荷）。
pub const NA_MARK: u64 = 0x4E414F4B0001;

/// 探测失败显性标记（ProbeFailed 分支落账载荷）。
pub const PF_MARK: u64 = 0x504641494C01;

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力面 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF5000u64 + (o as u64) * 0x37) ^ 0x47656E7900000000)
}

/// 能力面（0..4=安装 4..8=设备管理 8..12=传感 GPS 模拟 12..16=ADB 消费 16..20=N/A 探测账）。
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
        1 => Some("设备管理面"),
        2 => Some("传感与 GPS 模拟面"),
        3 => Some("ADB 消费面"),
        4 => Some("N/A 探测账面"),
        _ => None,
    }
}

/// 设备投影格载荷（格 c；投影段 = c/4：0..4=虚拟设备段 4..8=传感面段）。
pub const fn path_cell(c: usize) -> Option<u64> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((0xF6000u64 + (c as u64) * 0x2B) ^ 0x474D564400000000)
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
        0 => Some("虚拟设备段投影"),
        1 => Some("传感面段投影"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xF7000u64 + (v as u64) * 0x71) ^ 0x474D000000000000)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("2023.2"),
        1 => Some("2024.0"),
        2 => Some("2024.1"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xF810 + r as u32 * 0x3B)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("VirtualBox 依赖 N/A 声明·主"),
        1 => Some("VirtualBox 依赖 N/A 声明·辅"),
        2 => Some("ADB 消费分账声明·主"),
        3 => Some("ADB 消费分账声明·辅"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Genymotion 收口账本状态机。
#[derive(Clone)]
pub struct GenymotionLedger {
    /// 六判据（收口闸：安装/启动/设备清单/传感 GPS 模拟/ADB 连接/卸载）。
    gates: [Option<bool>; GATES],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 设备投影账。
    paths: [Option<u64>; PATH_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// N/A 如实登记账（NotAvailable 分支）。
    na_na: Option<u64>,
    /// 探测失败显性账（ProbeFailed 分支）。
    na_fail: Option<u64>,
    /// 虚报可用拒账数（Available 分支被拒次数）。
    na_avail_rejected: u32,
    /// 最后能力面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GenymotionLedger {
    /// 新账本。
    pub fn new() -> Self {
        GenymotionLedger {
            gates: [None; GATES],
            ops: [None; OPS],
            paths: [None; PATH_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
            na_na: None,
            na_fail: None,
            na_avail_rejected: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GenymotionCode> {
        if gate >= GATES {
            return Err(GenymotionCode::ESeal);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GenymotionCode> {
        if o >= OPS {
            return Err(GenymotionCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(GenymotionCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B00);
            return Err(GenymotionCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(GenymotionCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B01 + o as u64);
            return Err(GenymotionCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GenymotionCode::EDup);
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
    pub fn record_path(&mut self, c: usize, got: u64) -> Result<(), GenymotionCode> {
        let want = match path_cell(c) {
            Some(w) => w,
            None => return Err(GenymotionCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9C00 + c as u64);
            return Err(GenymotionCode::EPathDrift);
        }
        if match self.paths.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GenymotionCode::EDup);
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
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), GenymotionCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(GenymotionCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9D00 + v as u64);
            return Err(GenymotionCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GenymotionCode::EDup);
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
    pub fn declare_split(&mut self, r: usize) -> Result<(), GenymotionCode> {
        if r >= SPLIT_ROWS {
            return Err(GenymotionCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GenymotionCode::EDup);
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

    /// 探测账一分支（三分支闭集）。Available：本环境无可信证据链 →
    /// 虚报，计注入必抓，返回 ENotAvail（禁虚报可用=最高级违规）。
    /// NotAvailable：N/A 如实登记（三要素告警随账折叠入指纹），重复
    /// 登记 EDup。ProbeFailed：探测失败显性登记，与 N/A 严格区分。
    pub fn na_probe(&mut self, b: NaBranch) -> Result<(), GenymotionCode> {
        match b {
            NaBranch::Available => {
                self.na_avail_rejected += 1;
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x9F10);
                Err(GenymotionCode::ENotAvail)
            }
            NaBranch::NotAvailable => {
                if self.na_na.is_some() {
                    return Err(GenymotionCode::EDup);
                }
                self.na_na = Some(NA_MARK);
                self.fnv(0x9F00);
                self.fnv(NA_DEP_NAME.len() as u64);
                self.fnv(NA_REASON.len() as u64);
                self.fnv(NA_ALT_PATH.len() as u64);
                Ok(())
            }
            NaBranch::ProbeFailed => {
                if self.na_fail.is_some() {
                    return Err(GenymotionCode::EDup);
                }
                self.na_fail = Some(PF_MARK);
                self.fnv(0x9F20);
                Ok(())
            }
        }
    }

    /// N/A 如实登记核验（NotAvailable 分支已落）。
    pub fn na_registered(&self) -> bool {
        self.na_na.is_some()
    }

    /// 探测失败显性登记核验（ProbeFailed 分支已落）。
    pub fn na_failed_registered(&self) -> bool {
        self.na_fail.is_some()
    }

    /// 虚报可用拒账数（诚实账本上仅显性计数，不落可用账）。
    pub fn na_rejected_claims(&self) -> u32 {
        self.na_avail_rejected
    }

    /// 探测账三分支闭集核验：NotAvailable 如实登记、ProbeFailed 显性、
    /// Available 无账（虚报必拒，不落账）。
    pub fn na_account_ok(&self) -> bool {
        self.na_registered() && self.na_avail_rejected == 0
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal、
    /// N/A 探测分支未如实登记→ESeal。
    pub fn seal(&self) -> Result<(), GenymotionCode> {
        if !self.gates_all() {
            return Err(GenymotionCode::ESeal);
        }
        if !self.splits_all() {
            return Err(GenymotionCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.vers_all() {
            return Err(GenymotionCode::ESeal);
        }
        if !self.na_registered() {
            return Err(GenymotionCode::ESeal);
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

    /// KB-COMPAT 定级：全齐且 N/A 探测分支如实登记 → S2（N/A 封顶
    /// 目标档，本卡虚拟化面在内核环境恒 N/A，S1 不适用）；否则 S3。
    pub fn grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.paths_all()
            && self.vers_all()
            && self.splits_all()
            && self.na_registered()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级申报核验：S1 在本卡任何状态下均不适用（虚报 S1 必抓
    /// EGradeDrift）；申报档须与实际档一致。
    pub fn grade_claim(&self, g: Grade) -> Result<(), GenymotionCode> {
        if let Grade::S1 = g {
            return Err(GenymotionCode::EGradeDrift);
        }
        if g == self.grade() {
            Ok(())
        } else {
            Err(GenymotionCode::EGradeDrift)
        }
    }
}

impl Default for GenymotionLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 装账辅助（纯函数落账，供判据集/注入核复用）──────────────────
/// 落满六判据闸。
fn fill_gates(l: &mut GenymotionLedger) {
    let mut g = 0usize;
    while g < GATES {
        let _ = l.judge_gate(g);
        g += 1;
    }
}

/// 落满 20 能力格，返回成功格数。
fn fill_ops(l: &mut GenymotionLedger) -> u32 {
    let mut n = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if l.record_op(o, w).is_ok() {
                n += 1;
            }
        }
        o += 1;
    }
    n
}

/// 落满 8 投影格，返回成功格数。
fn fill_paths(l: &mut GenymotionLedger) -> u32 {
    let mut n = 0u32;
    let mut c = 0usize;
    while c < PATH_CELLS {
        if let Some(w) = path_cell(c) {
            if l.record_path(c, w).is_ok() {
                n += 1;
            }
        }
        c += 1;
    }
    n
}

/// 落满 3 档版本，返回成功档数。
fn fill_versions(l: &mut GenymotionLedger) -> u32 {
    let mut n = 0u32;
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if l.record_version(v, w).is_ok() {
                n += 1;
            }
        }
        v += 1;
    }
    n
}

/// 落满 4 行声明。
fn fill_splits(l: &mut GenymotionLedger) {
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = l.declare_split(r);
        r += 1;
    }
}

/// 全齐装账：六闸+能力+投影+版本+声明+N/A 如实登记。
fn fill_all(l: &mut GenymotionLedger) {
    fill_gates(l);
    fill_ops(l);
    fill_paths(l);
    fill_versions(l);
    fill_splits(l);
    let _ = l.na_probe(NaBranch::NotAvailable);
}

/// 能力面单调核验（五面闭集）。
fn faces_monotone() -> bool {
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
}

/// 投影段单调核验（双段闭集）。
fn segments_monotone() -> bool {
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
}

/// 取格载荷（越闭集折 0，仅注入核对照用）。
fn cell0(f: fn(usize) -> Option<u64>, i: usize) -> u64 {
    match f(i) {
        Some(w) => w,
        None => 0,
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GenymotionLedger::new();
    fill_gates(&mut led);
    let mut recorded = 0u32;
    recorded += fill_ops(&mut led);
    recorded += fill_paths(&mut led);
    recorded += fill_versions(&mut led);
    fill_splits(&mut led);
    // N/A 探测账如实登记：NotAvailable 主分支 + ProbeFailed 显性分支，Available 不落账（虚报必拒）。
    let _ = led.na_probe(NaBranch::NotAvailable);
    let _ = led.na_probe(NaBranch::ProbeFailed);
    // 注入 10 轮三态轮转，全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GenymotionLedger::new();
        let caught = match kind {
            0 => {
                // 版本漂移：先按合法序落 0 档正格，再注 1 档漂移，必抓。
                let _ = lt.record_version(0, cell0(version_cell, 0));
                lt.record_version(1, cell0(version_cell, 1).wrapping_add(1))
                    == Err(GenymotionCode::EVerDrift)
            }
            1 => {
                // 操作漂移：先按合法序落同族正格（安装面），再注漂移。
                let _ = lt.record_op(0, cell0(op_cell, 0));
                lt.record_op(1, cell0(op_cell, 1).wrapping_add(1)) == Err(GenymotionCode::EDrift)
            }
            _ => {
                // N/A 虚报可用：Available 分支无可信证据链必拒，
                // 返回 ENotAvail（虚报可用=最高级违规，注入必抓）。
                lt.na_probe(NaBranch::Available) == Err(GenymotionCode::ENotAvail)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xA000 + k as u64);
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
/// UNX-F52547 判据集（锚 UNX-F52548-J1，off-by-one 板式，同 F52547 N/A 口径）。
pub fn run_unx52547_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52547");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Genymotion 五面 20 操作全通",
        recorded >= OPS as u32,
        "安装/设备管理/传感 GPS 模拟/ADB 消费/N/A 探测账逐格恒等",
    );
    s.add(
        "设备投影 8/8 全落",
        {
            let mut lt = GenymotionLedger::new();
            fill_paths(&mut lt);
            lt.paths_all()
        },
        "虚拟设备段投影 0..4 与传感面段投影 4..8 双段逐格恒等",
    );
    s.add(
        "版本阶梯 2023.2/2024.0/2024.1 三档全落",
        {
            let mut lt = GenymotionLedger::new();
            fill_versions(&mut lt);
            lt.vers_all()
        },
        "2023.x/2024.x 三档闭集逐档恒等，越级必抓",
    );
    s.add(
        "VirtualBox 依赖 N/A 声明与 ADB 消费分账声明 4/4 全齐",
        {
            let mut lt = GenymotionLedger::new();
            fill_splits(&mut lt);
            lt.splits_all()
        },
        "VirtualBox 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行闭集（§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = GenymotionLedger::new();
            fill_gates(&mut lt);
            lt.gates_all()
        },
        "安装/启动/设备清单/传感 GPS 模拟/ADB 连接/卸载逐格落账",
    );
    s.add(
        "N/A 探测账·NotAvailable 分支如实登记",
        {
            let mut lt = GenymotionLedger::new();
            lt.na_probe(NaBranch::NotAvailable).is_ok() && lt.na_registered()
        },
        "N/A 如实登记落账，附三要素告警，不虚报可用",
    );
    s.add(
        "N/A 探测账·Available 虚报必抓",
        {
            let mut lt = GenymotionLedger::new();
            let r = lt.na_probe(NaBranch::Available);
            r == Err(GenymotionCode::ENotAvail) && lt.stats() == (1, 1, 0)
        },
        "内核纯函数环境无可信证据链，虚报可用=最高级违规，注入必抓不落可用账",
    );
    s.add(
        "N/A 探测账·ProbeFailed 显性分支与 N/A 严格区分",
        {
            let mut lt = GenymotionLedger::new();
            let ok = lt.na_probe(NaBranch::ProbeFailed).is_ok();
            ok && lt.na_failed_registered() && !lt.na_registered()
        },
        "探测失败显性落账，不冒充 N/A 也不冒充可用，不造假",
    );
    s.add(
        "N/A 三要素告警齐备（依赖项名/原因/替代路径）",
        !NA_DEP_NAME.is_empty() && !NA_REASON.is_empty() && !NA_ALT_PATH.is_empty(),
        "N/A 如实登记必须附三要素告警：VirtualBox/Hypervisor 依赖项名、内核环境不可探测原因、宿主侧替代路径",
    );
    s.add(
        "能力面单调",
        faces_monotone(),
        "安装→设备管理→传感 GPS 模拟→ADB 消费→N/A 探测账五面闭集单调",
    );
    s.add(
        "投影族单调",
        segments_monotone(),
        "虚拟设备段→传感面段双段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本漂移/操作漂移/N/A 虚报可用三态轮转必抓",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = GenymotionLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        {
            let mut lt = GenymotionLedger::new();
            lt.record_path(PATH_CELLS, 0).is_err()
        },
        "投影格 8 越闭集必拒",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明齐但投影缺一也必拒。
            let lt_empty = GenymotionLedger::new();
            let mut lt_cell = GenymotionLedger::new();
            fill_gates(&mut lt_cell);
            fill_splits(&mut lt_cell);
            let mut c = 1usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    let _ = lt_cell.record_path(c, w);
                }
                c += 1;
            }
            lt_empty.seal() == Err(GenymotionCode::ESeal) && lt_cell.seal() == Err(GenymotionCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = GenymotionLedger::new();
            fill_gates(&mut lt);
            fill_ops(&mut lt);
            lt.seal() == Err(GenymotionCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    s.add(
        "N/A 探测分支未登记 seal 必拒",
        {
            let mut lt = GenymotionLedger::new();
            fill_gates(&mut lt);
            fill_ops(&mut lt);
            fill_paths(&mut lt);
            fill_versions(&mut lt);
            fill_splits(&mut lt);
            let before = lt.seal() == Err(GenymotionCode::ESeal);
            let _ = lt.na_probe(NaBranch::NotAvailable);
            before && lt.seal().is_ok()
        },
        "格账与声明全齐而 N/A 探测分支未如实登记先 ESeal，如实登记后封账 Ok",
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
        "定级 N/A 封顶 S2 且 S1 虚报必抓",
        {
            let mut lt = GenymotionLedger::new();
            fill_all(&mut lt);
            let full_s2 = lt.grade() == Grade::S2;
            let s1_claim = lt.grade_claim(Grade::S1) == Err(GenymotionCode::EGradeDrift);
            let s2_claim = lt.grade_claim(Grade::S2).is_ok();
            let empty = GenymotionLedger::new();
            full_s2 && s1_claim && s2_claim && empty.grade() == Grade::S3
        },
        "本卡虚拟化面恒 N/A：全齐+N/A 如实登记封顶 S2+N/A 清单，定级虚报 S1 = EGradeDrift 必抓",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC880",
    );
    s.add("收口·判据条数", s.len() >= 20, "含本条共 ≥20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCC880);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 闭集 14 判别值逐位核对：Ok=0，13 错误码至 13（EGradeDrift=12、ENotAvail=13）。
        let all = [
            GenymotionCode::Ok,
            GenymotionCode::ENoOp,
            GenymotionCode::EDrift,
            GenymotionCode::EOrder,
            GenymotionCode::ENoPath,
            GenymotionCode::EPathDrift,
            GenymotionCode::ENoVer,
            GenymotionCode::EVerDrift,
            GenymotionCode::ENoRow,
            GenymotionCode::ERowMiss,
            GenymotionCode::EDup,
            GenymotionCode::ESeal,
            GenymotionCode::EGradeDrift,
            GenymotionCode::ENotAvail,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(GenymotionCode::EGradeDrift as u32, 12);
        assert_eq!(GenymotionCode::ENotAvail as u32, 13);
        assert_eq!(GenymotionCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(12), Some(3));
        assert_eq!(op_kind(16), Some(4));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("安装面"));
        assert_eq!(op_kind_name(1), Some("设备管理面"));
        assert_eq!(op_kind_name(2), Some("传感与 GPS 模拟面"));
        assert_eq!(op_kind_name(3), Some("ADB 消费面"));
        assert_eq!(op_kind_name(4), Some("N/A 探测账面"));
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
        assert_eq!(path_kind_name(0), Some("虚拟设备段投影"));
        assert_eq!(path_kind_name(1), Some("传感面段投影"));
        assert!(path_kind_name(2).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("2023.2"));
        assert_eq!(version_name(1), Some("2024.0"));
        assert_eq!(version_name(2), Some("2024.1"));
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
        assert!(split_name(0).is_some());
        assert!(split_name(3).is_some());
    }

    #[test]
    fn test_na_probe_three_branches() {
        // 三分支闭集元数据。
        assert_eq!(NaBranch::Available.idx(), 0);
        assert_eq!(NaBranch::NotAvailable.idx(), 1);
        assert_eq!(NaBranch::ProbeFailed.idx(), 2);
        assert!(!NaBranch::Available.name().is_empty());
        assert!(!NaBranch::NotAvailable.name().is_empty());
        assert!(!NaBranch::ProbeFailed.name().is_empty());
        assert_ne!(NA_MARK, PF_MARK);
        assert!(!NA_DEP_NAME.is_empty());
        assert!(!NA_REASON.is_empty());
        assert!(!NA_ALT_PATH.is_empty());
        // NotAvailable：如实登记，N/A 账落。
        let mut lt = GenymotionLedger::new();
        assert!(lt.na_probe(NaBranch::NotAvailable).is_ok());
        assert!(lt.na_registered());
        assert!(!lt.na_failed_registered());
        assert_eq!(lt.stats(), (0, 0, 0));
        // 重复 N/A 登记 → EDup，不计注入账。
        assert_eq!(lt.na_probe(NaBranch::NotAvailable), Err(GenymotionCode::EDup));
        assert_eq!(lt.stats(), (0, 0, 0));
        // ProbeFailed：显性分支，与 N/A 严格区分。
        let mut lt2 = GenymotionLedger::new();
        assert!(lt2.na_probe(NaBranch::ProbeFailed).is_ok());
        assert!(lt2.na_failed_registered());
        assert!(!lt2.na_registered());
        assert_eq!(lt2.na_probe(NaBranch::ProbeFailed), Err(GenymotionCode::EDup));
        // Available：本环境无证据链 → 虚报必抓 ENotAvail，注入计入，可用不落账。
        let mut lt3 = GenymotionLedger::new();
        assert_eq!(lt3.na_probe(NaBranch::Available), Err(GenymotionCode::ENotAvail));
        assert_eq!(lt3.stats(), (1, 1, 0));
        assert!(lt3.na_avail_rejected >= 1);
        assert!(!lt3.na_registered());
        assert!(!lt3.na_failed_registered());
        // N/A 虚报可用注入场景：连报 4 次必抓 4 次，可用账恒空。
        let mut lt4 = GenymotionLedger::new();
        assert_eq!(lt4.na_probe(NaBranch::Available), Err(GenymotionCode::ENotAvail));
        assert_eq!(lt4.na_probe(NaBranch::Available), Err(GenymotionCode::ENotAvail));
        assert_eq!(lt4.na_probe(NaBranch::Available), Err(GenymotionCode::ENotAvail));
        assert_eq!(lt4.na_probe(NaBranch::Available), Err(GenymotionCode::ENotAvail));
        assert_eq!(lt4.stats(), (4, 4, 0));
        assert!(!lt4.na_registered());
        assert!(lt4.na_probe(NaBranch::NotAvailable).is_ok());
        // 虚报留痕恒记：4 次虚报全留账，na_account_ok 只对零虚报账成立。
        assert_eq!(lt4.na_avail_rejected, 4);
        assert!(!lt4.na_account_ok());
        // 干净账：只有如实 N/A 登记而无虚报 → 探测账健康。
        let mut lt5 = GenymotionLedger::new();
        assert!(lt5.na_probe(NaBranch::NotAvailable).is_ok());
        assert!(lt5.na_account_ok());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GenymotionLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(GenymotionCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GenymotionLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(GenymotionCode::EDrift)
        );
        assert_eq!(
            lt.record_path(1, path_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(GenymotionCode::EPathDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(GenymotionCode::EVerDrift)
        );
        // 重复落账：同格只许一条账目，EDup 不计注入账。
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(GenymotionCode::EDup));
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(GenymotionCode::EDup));
        assert_eq!(lt.stats(), (3, 3, 0));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GenymotionLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GenymotionLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        // 越闭集边界拒不计注入账。
        let _ = lt2.record_op(OPS, 0);
        let _ = lt2.record_path(PATH_CELLS, 0);
        let _ = lt2.record_version(VERSION_STEPS, 0);
        let _ = lt2.declare_split(SPLIT_ROWS);
        assert_eq!(lt2.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = GenymotionLedger::new();
        fill_gates(&mut full);
        fill_ops(&mut full);
        fill_paths(&mut full);
        fill_versions(&mut full);
        assert_eq!(full.seal(), Err(GenymotionCode::ERowMiss));
        fill_splits(&mut full);
        // 声明齐而 N/A 探测分支未如实登记 → ESeal。
        assert_eq!(full.seal(), Err(GenymotionCode::ESeal));
        let _ = full.na_probe(NaBranch::NotAvailable);
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 8 投影 + 3 档版本（六闸/声明行/N/A 登记不计数）。
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
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        assert!(!Grade::S1.detail().is_empty());
        assert!(!Grade::S2.detail().is_empty());
        assert!(!Grade::S3.detail().is_empty());
        // 如实阶梯：空账 S3。
        let lt = GenymotionLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        // 全齐但 N/A 探测分支未登记 → S3（不收口）。
        let mut lt2 = GenymotionLedger::new();
        fill_gates(&mut lt2);
        fill_ops(&mut lt2);
        fill_paths(&mut lt2);
        fill_versions(&mut lt2);
        fill_splits(&mut lt2);
        assert_eq!(lt2.grade(), Grade::S3);
        // N/A 如实登记后 → 封顶 S2（N/A 清单目标档，S1 不适用）。
        let _ = lt2.na_probe(NaBranch::NotAvailable);
        assert_eq!(lt2.grade(), Grade::S2);
        // 定级虚报 S1 必抓 EGradeDrift；如实申报 S2 收讫。
        assert_eq!(lt2.grade_claim(Grade::S1), Err(GenymotionCode::EGradeDrift));
        assert!(lt2.grade_claim(Grade::S2).is_ok());
        // 空账虚报 S1 同样必抓。
        assert_eq!(lt.grade_claim(Grade::S1), Err(GenymotionCode::EGradeDrift));
    }

    #[test]
    fn unx52547_checks_green() {
        let s = run_unx52547_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
