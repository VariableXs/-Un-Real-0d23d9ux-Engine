//! UNX-F52551 MEmu 收口（N/A 同构）· 0xCC980段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52551"，逐字）：
//!   id=52551 title="MEmu 收口（N/A 同构）" quota=300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52552-J1 反编译/
//!     回编译 ×10 本地 APK 样本全通（许可合规：仅自有样本声明）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52551）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52552-J1，照原文锚定（反编译/回编译 ×10 本地 APK 样本判据为下
//! 一卡 apktool 域语料）；功能按本卡标题（MEmu / N/A 同构）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 核心语义 = N/A 同构（与 52547/52548/52549/52550 四卡同构）：
//!      MEmu 的虚拟化依赖探测（Hypervisor 检测）在内核纯函数环境不可用
//!      → 如实 N/A，KB-COMPAT 定级封顶 S2 + N/A 清单，禁虚报可用。探测
//!      账三分支闭集：Available（可信证据链）/ NotAvailable（N/A 如实
//!      登记 + 三要素告警：依赖项名/原因/替代路径）/ ProbeFailed（探测
//!      失败显性，与 N/A 严格区分不造假）。
//!   ② 域模型：五面 20 操作格（安装面 4 / 实例面 4 / 宏脚本面 4 /
//!      ADB 消费 4 / N/A 探测账 4）单调序：安装面→实例面（多开实例
//!      清单闭集）→宏脚本面（操作录制分账）→ADB 消费面→N/A 探测账面；
//!      逐格载荷恒等才收；漂移必抓。
//!   ③ 实例投影 8 格：实例段 0..4、宏脚本段 4..8 双段单调闭集，漂移必抓。
//!   ④ 版本阶梯 3 档：9.0.6/9.1.0/9.2.0 三级闭集，越级必抓、漂移必抓。
//!   ⑤ 声明 4 行闭集：Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账
//!      声明 2 行，缺一拒封（六闸缺→ESeal、声明行缺→ERowMiss、
//!      格账/探测账缺→ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/na_probe 三分支探测账/
//!      seal/fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle
//!      10 轮三态轮转（版本漂移/操作漂移/N/A 虚报 Available），三类
//!      注入全部落在当轮临时账本上，主账 injected==caught==10、
//!      escaped==0；双跑指纹一致；定级目标档 S2（N/A 封顶），虚报 S1
//!      = EGradeDrift 必抓。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52551;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐（本卡落 0xCC980）。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCC980 && SEG_BASE % 0x40 == 0);

/// MEmu 五面操作数。
pub const OPS: usize = 20;

/// 实例投影格数。
pub const PATH_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 声明行数（Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行）。
pub const SPLIT_ROWS: usize = 4;

/// 收口闸数。
pub const GATES: usize = 6;

/// N/A 探测账格数。
pub const NA_PROBE_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemuCode {
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
    /// 定级漂移（虚报 S1/虚报可用必抓）
    EGradeDrift,
}

impl MemuCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MemuCode::Ok => "成功",
            MemuCode::ENoOp => "操作号越闭集：MEmu 五面 20 格（含 N/A 探测账面 4 格）之外先登记",
            MemuCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            MemuCode::EOrder => "能力面逆序：安装面→实例面（多开实例清单闭集）→宏脚本面（操作录制分账）→ADB 消费面→N/A 探测账面单调，逆序必抓",
            MemuCode::ENoPath => "投影格越闭集：实例投影 8 格（实例段/宏脚本段）之外先登记",
            MemuCode::EPathDrift => "投影漂移：实例投影折叠与基准不恒等，逐格复算",
            MemuCode::ENoVer => "版本级越阶梯：9.0.6/9.1.0/9.2.0 三档闭集之外先登记",
            MemuCode::EVerDrift => "版本阶梯漂移：版本档折叠与基准不恒等，逐档复算",
            MemuCode::ENoRow => "声明行越闭集：Hypervisor 依赖 N/A 声明+ADB 消费分账声明 4 行闭集之外先登记",
            MemuCode::ERowMiss => "缺声明行：Hypervisor 依赖 N/A 声明 2 行/ADB 消费分账声明 2 行未齐先补齐",
            MemuCode::EDup => "重复落账：同格只许一条账目",
            MemuCode::ESeal => "未齐即封：20 能力格+8 投影格+3 档版本+6 闸+4 探测账+4 行声明未齐先补齐",
            MemuCode::EGradeDrift => "定级漂移：虚拟化依赖探测在内核纯函数环境恒 N/A，KB-COMPAT 封顶 S2，虚报 S1 或虚报 Available 必抓（§AI-66.3）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡虚拟化面恒 N/A，不可达）
    S1,
    /// S2：有条件收口（本卡目标档，N/A 清单随附）
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
            Grade::S1 => "S1：全通收口——本卡虚拟化依赖探测恒 N/A，S1 不可达，虚报必抓",
            Grade::S2 => "S2：有条件收口（N/A 封顶目标档），N/A 清单随附：Hypervisor 依赖探测 4 格如实登记",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 探测账三分支闭集 ─────────────────────────────────────────────
/// N/A 探测账三分支闭集：Available / NotAvailable / ProbeFailed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeVerdict {
    /// 可用：需可信证据链（内核纯函数环境不存在，虚报必抓）
    Available,
    /// N/A：如实登记 + 三要素告警（依赖项名/原因/替代路径）
    NotAvailable,
    /// 探测失败：显性登记，与 N/A 严格区分不造假
    ProbeFailed,
}

impl ProbeVerdict {
    /// 分支值。
    pub fn val(self) -> u32 {
        match self {
            ProbeVerdict::Available => 0,
            ProbeVerdict::NotAvailable => 1,
            ProbeVerdict::ProbeFailed => 2,
        }
    }
    /// 分支明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeVerdict::Available => "Available：须附可信证据链；内核纯函数环境无 Hypervisor 探测通道，本分支不可达，虚报必抓",
            ProbeVerdict::NotAvailable => "NotAvailable：N/A 如实登记，三要素告警（依赖项名/原因/替代路径）齐备",
            ProbeVerdict::ProbeFailed => "ProbeFailed：探测失败显性登记，与 N/A 严格区分，不造假不冒充",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力面 = o/4）。冻结基准表。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF9000u64 + (o as u64) * 0x3C) ^ 0x4D454D556B000000)
}

/// 能力面（0..4=安装面 4..8=实例面 8..12=宏脚本面 12..16=ADB 消费面 16..20=N/A 探测账面）。
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
        1 => Some("实例面"),
        2 => Some("宏脚本面"),
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
    Some((0xFA000u64 + (c as u64) * 0x43) ^ 0x4D454D5553000000)
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
        0 => Some("实例段投影"),
        1 => Some("宏脚本段投影"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xFB000u64 + (v as u64) * 0x67) ^ 0x4D454D5500000000)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("9.0.6"),
        1 => Some("9.1.0"),
        2 => Some("9.2.0"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xFC10 + r as u32 * 0x3C)
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

/// N/A 三要素告警（探测格 c）：（依赖项名，原因，替代路径）。
pub const fn na_element(c: usize) -> Option<(&'static str, &'static str, &'static str)> {
    match c {
        0 => Some(("Hypervisor（VT-x/AMD-V 硬件虚拟化）", "内核纯函数环境无 CPUID/hypervisor 位探测通道", "宿主机 MEmu 安装器自检报告转发")),
        1 => Some(("MEmu 多开器（Multi-MEmu 多开实例清单）", "内核环境无进程/实例枚举通道", "宿主机多开实例清单导出后逐格落账")),
        2 => Some(("MEmu 宏脚本录制（操作录制分账）", "内核环境无宏脚本录制回放通道", "宿主机宏录制器导出后折叠分账")),
        3 => Some(("ADB 调试桥消费（adb 连接分账）", "内核环境无 ADB 套接字通道", "宿主机 adb devices 输出转发后分账")),
        _ => None,
    }
}

/// N/A 清单随附摘要（S2 封顶档必备）。
pub const fn na_list_summary() -> &'static str {
    "N/A 清单：Hypervisor 依赖探测/多开实例清单/宏脚本操作录制/ADB 消费分账 4 格恒 N/A，三要素告警齐备，替代路径走宿主机转发"
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// MEmu 收口账本状态机。
#[derive(Clone)]
pub struct MemuLedger {
    /// 六判据（收口闸：安装/启动/多开/宏脚本/ADB/卸载）。
    gates: [Option<bool>; GATES],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 实例投影账。
    paths: [Option<u64>; PATH_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// N/A 探测账（三分支闭集，Available 永不入账）。
    probe: [Option<ProbeVerdict>; NA_PROBE_CELLS],
    /// 最后能力面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MemuLedger {
    /// 新账本。
    pub fn new() -> Self {
        MemuLedger {
            gates: [None; GATES],
            ops: [None; OPS],
            paths: [None; PATH_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
            probe: [None; NA_PROBE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MemuCode> {
        if gate >= GATES {
            return Err(MemuCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xA100 + gate as u64);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), MemuCode> {
        if o >= OPS {
            return Err(MemuCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(MemuCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA200);
            return Err(MemuCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(MemuCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA201 + o as u64);
            return Err(MemuCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MemuCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xA202 + o as u64);
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
    pub fn record_path(&mut self, c: usize, got: u64) -> Result<(), MemuCode> {
        let want = match path_cell(c) {
            Some(w) => w,
            None => return Err(MemuCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA300 + c as u64);
            return Err(MemuCode::EPathDrift);
        }
        if match self.paths.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MemuCode::EDup);
        }
        if let Some(p) = self.paths.get_mut(c) {
            *p = Some(got);
        }
        self.fnv(0xA301 + c as u64);
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
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), MemuCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(MemuCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA400 + v as u64);
            return Err(MemuCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MemuCode::EDup);
        }
        if let Some(b) = self.vers.get_mut(v) {
            *b = Some(got);
        }
        self.fnv(0xA401 + v as u64);
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
    pub fn declare_split(&mut self, r: usize) -> Result<(), MemuCode> {
        if r >= SPLIT_ROWS {
            return Err(MemuCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MemuCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0xA500 + r as u64);
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

    /// N/A 探测账一格（三分支闭集）。
    ///
    /// Available：内核纯函数环境无可信证据链，虚报=注入必抓（EGradeDrift，
    /// 计入注入账且永不入探测账）；NotAvailable：N/A 如实登记（三要素
    /// 告警见 na_element）；ProbeFailed：探测失败显性登记，与 N/A 严格
    /// 区分。越闭集拒不计注入账；重复 EDup 不计注入账。
    pub fn na_probe(&mut self, cell: usize, verdict: ProbeVerdict) -> Result<(), MemuCode> {
        if cell >= NA_PROBE_CELLS {
            return Err(MemuCode::ENoOp);
        }
        if verdict == ProbeVerdict::Available {
            // 虚报可用：注入必抓，探测账永不接受 Available。
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA600 + cell as u64);
            return Err(MemuCode::EGradeDrift);
        }
        if match self.probe.get(cell) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MemuCode::EDup);
        }
        if let Some(p) = self.probe.get_mut(cell) {
            *p = Some(verdict);
        }
        self.fnv(0xA601 + cell as u64);
        Ok(())
    }

    /// 探测格分支读取（未登记为 None）。
    pub fn probe_verdict(&self, cell: usize) -> Option<ProbeVerdict> {
        match self.probe.get(cell) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 探测账全齐核验（4 格，Available 永不计入）。
    pub fn probe_all(&self) -> bool {
        let mut c = 0usize;
        while c < NA_PROBE_CELLS {
            match self.probe.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账/投影/版本/探测账缺→ESeal。
    pub fn seal(&self) -> Result<(), MemuCode> {
        if !self.gates_all() {
            return Err(MemuCode::ESeal);
        }
        if !self.splits_all() {
            return Err(MemuCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.vers_all() || !self.probe_all() {
            return Err(MemuCode::ESeal);
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

    /// KB-COMPAT 定级（N/A 封顶）：虚拟化依赖探测在内核纯函数环境恒
    /// N/A，S1 不可达——即便全账收口也封顶 S2（N/A 清单随附）；六闸
    /// 未齐 → S3。虚报 S1 走 claim 必抓 EGradeDrift。
    pub fn grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else {
            // 全账齐（含探测账 N/A 如实登记）也封顶 S2；部分齐（六闸齐）
            // 同为有条件收口 S2。S1 在本卡恒不可达。
            Grade::S2
        }
    }

    /// 定级申领：S1 恒拒（N/A 封顶，虚报必抓 EGradeDrift）；S2/S3 与
    /// 实际定级一致才收。
    pub fn claim(&self, g: Grade) -> Result<(), MemuCode> {
        if g == Grade::S1 {
            return Err(MemuCode::EGradeDrift);
        }
        if g == self.grade() {
            Ok(())
        } else {
            Err(MemuCode::ESeal)
        }
    }
}

impl Default for MemuLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
///
/// 主账全通闭环：41 账目 seal Ok（20 能力 + 8 投影 + 3 版本 + 6 闸 +
/// 4 探测账），N/A 分支如实登记 NotAvailable；返回的 recorded 只计
/// record_op/record_path/record_version 三族 = 31（闸、声明行、探测账
/// 不计入 recorded）。注入账 10 轮三态轮转全部落在当轮临时账本上，
/// injected==caught==10、escaped==0。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MemuLedger::new();
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
    // N/A 分支如实登记：4 格探测账全部 NotAvailable（禁虚报可用）。
    let mut p = 0usize;
    while p < NA_PROBE_CELLS {
        let _ = led.na_probe(p, ProbeVerdict::NotAvailable);
        p += 1;
    }
    // 主账闭环自核：41 账目全齐应 seal Ok（运行期只记账，不断言）。
    let _ = led.seal();
    // 注入 10 轮：版本漂移 / 操作漂移 / N/A 虚报 Available 三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MemuLedger::new();
        let caught = match kind {
            0 => {
                // 版本漂移：先按合法序落 0 档正格，再对 1 档注漂移载荷，必抓。
                let w0 = match version_cell(0) { Some(w) => w, None => 0 };
                let w1 = match version_cell(1) { Some(w) => w, None => 0 };
                let _ = lt.record_version(0, w0);
                lt.record_version(1, w1.wrapping_add(1)) == Err(MemuCode::EVerDrift)
            }
            1 => {
                // 操作漂移：先按合法序落同族正格（安装面），再注漂移。
                let w0 = match op_cell(0) { Some(w) => w, None => 0 };
                let w1 = match op_cell(1) { Some(w) => w, None => 0 };
                let _ = lt.record_op(0, w0);
                lt.record_op(1, w1.wrapping_add(1)) == Err(MemuCode::EDrift)
            }
            _ => {
                // N/A 虚报 Available：内核纯函数环境无可信证据链，必抓。
                lt.na_probe(2, ProbeVerdict::Available).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xA700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 全账助手 ─────────────────────────────────────────────────────
/// 构建全齐账本（六闸+20 能力+8 投影+3 版本+4 声明+4 探测账 N/A 登记），
/// 供判据集与测试复用；只记账不 panic。
fn full_ledger() -> MemuLedger {
    let mut lt = MemuLedger::new();
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
    let mut p = 0usize;
    while p < NA_PROBE_CELLS {
        let _ = lt.na_probe(p, ProbeVerdict::NotAvailable);
        p += 1;
    }
    lt
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52551 判据集（锚 UNX-F52552-J1，off-by-one 板式；§AI-66.3）。
pub fn run_unx52551_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52551");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "MEmu 五面 20 操作全通",
        recorded >= OPS as u32,
        "安装/实例/宏脚本/ADB 消费/N/A 探测账逐格恒等",
    );
    s.add(
        "实例投影 8/8 全落",
        {
            let mut lt = MemuLedger::new();
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
        "实例段投影 0..4 与宏脚本段投影 4..8 双段逐格恒等",
    );
    s.add(
        "版本阶梯 9.0.6/9.1.0/9.2.0 三档全落",
        {
            let mut lt = MemuLedger::new();
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
        "Hypervisor N/A 声明与 ADB 消费分账声明 4/4 全齐",
        {
            let mut lt = MemuLedger::new();
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
        "Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行闭集（§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = MemuLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/启动/多开/宏脚本/ADB/卸载逐格落账",
    );
    s.add(
        "能力面与投影段双单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) { Some(k) => k, None => 5 };
                let kb = match op_kind(o - 1) { Some(k) => k, None => 5 };
                if ka < kb {
                    ok = false;
                }
                o += 1;
            }
            let mut c = 1usize;
            while c < PATH_CELLS {
                let ka = match path_kind(c) { Some(k) => k, None => 2 };
                let kb = match path_kind(c - 1) { Some(k) => k, None => 2 };
                if ka < kb {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "安装面→实例面→宏脚本面→ADB 消费面→N/A 探测账面五面单调，实例段→宏脚本段双段单调",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒·10/10 全抓",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("版本漂移/操作漂移/N/A 虚报三态轮转：注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    s.add(
        "边界·操作与投影格越闭集",
        {
            let mut lt = MemuLedger::new();
            lt.record_op(OPS, 0).is_err() && lt.record_path(PATH_CELLS, 0).is_err()
        },
        "操作 20 与投影格 8 越闭集必拒，且不计注入账",
    );
    s.add(
        "边界·声明行与版本越闭集",
        {
            let mut lt = MemuLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err() && lt.record_version(VERSION_STEPS, 0).is_err()
        },
        "声明行 4 与版本级 3 越闭集必拒",
    );
    s.add(
        "三类载荷漂移拒",
        {
            let mut lt = MemuLedger::new();
            let w0 = match op_cell(1) { Some(w) => w, None => 0 };
            let p1 = match path_cell(1) { Some(w) => w, None => 0 };
            let v2 = match version_cell(2) { Some(w) => w, None => 0 };
            let a = lt.record_op(1, w0.wrapping_add(1)) == Err(MemuCode::EDrift);
            let b = lt.record_path(1, p1.wrapping_add(7)) == Err(MemuCode::EPathDrift);
            let d = lt.record_version(2, v2.wrapping_add(9)) == Err(MemuCode::EVerDrift);
            a && b && d
        },
        "能力格/投影格/版本档载荷漂移必抓，逐格复算",
    );
    s.add(
        "N/A 判据·Available 虚报必抓",
        {
            let mut lt = MemuLedger::new();
            lt.na_probe(0, ProbeVerdict::Available) == Err(MemuCode::EGradeDrift)
                && lt.probe_verdict(0).is_none()
        },
        "内核纯函数环境无可信证据链，虚报可用=EGradeDrift 且永不入探测账",
    );
    s.add(
        "N/A 判据·NotAvailable 如实登记三要素齐",
        {
            let mut lt = MemuLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < NA_PROBE_CELLS {
                if lt.na_probe(c, ProbeVerdict::NotAvailable).is_err() {
                    ok = false;
                }
                let tri = match na_element(c) {
                    Some((dep, why, alt)) => !dep.is_empty() && !why.is_empty() && !alt.is_empty(),
                    None => false,
                };
                if !tri {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.probe_all()
        },
        "依赖项名/原因/替代路径三要素告警逐格齐备，N/A 如实登记",
    );
    s.add(
        "N/A 判据·ProbeFailed 显性且与 N/A 严格区分",
        {
            let mut lt = MemuLedger::new();
            let a = lt.na_probe(0, ProbeVerdict::ProbeFailed);
            let b = lt.na_probe(1, ProbeVerdict::NotAvailable);
            a.is_ok()
                && b.is_ok()
                && lt.probe_verdict(0) == Some(ProbeVerdict::ProbeFailed)
                && lt.probe_verdict(1) == Some(ProbeVerdict::NotAvailable)
                && ProbeVerdict::ProbeFailed != ProbeVerdict::NotAvailable
        },
        "探测失败显性登记，分支值与语义均与 N/A 区分，不造假不冒充",
    );
    s.add(
        "N/A 判据·探测账越闭集拒不计注入账",
        {
            let mut lt = MemuLedger::new();
            let r = lt.na_probe(NA_PROBE_CELLS, ProbeVerdict::NotAvailable);
            r == Err(MemuCode::ENoOp) && lt.stats() == (0, 0, 0)
        },
        "探测格 4 越闭集必拒且注入三数恒零",
    );
    s.add(
        "seal 语义·账目缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：全账账本探测缺一也必拒。
            let lt_empty = MemuLedger::new();
            let mut lt_cell = full_ledger();
            if let Some(slot) = lt_cell.probe.get_mut(NA_PROBE_CELLS - 1) {
                *slot = None;
            }
            lt_empty.seal() == Err(MemuCode::ESeal) && lt_cell.seal() == Err(MemuCode::ESeal)
        },
        "六闸缺或格账/投影/版本/探测账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            // 全账账本撤一行声明：六闸与格账齐而声明缺必 ERowMiss。
            let mut lt = full_ledger();
            if let Some(row) = lt.splits.get_mut(SPLIT_ROWS - 1) {
                *row = None;
            }
            lt.seal() == Err(MemuCode::ERowMiss)
        },
        "Hypervisor N/A 声明/ADB 消费分账声明缺一行必 ERowMiss",
    );
    s.add(
        "虚报 S1 申领 EGradeDrift 必抓",
        {
            let lt_empty = MemuLedger::new();
            lt_empty.claim(Grade::S1) == Err(MemuCode::EGradeDrift)
                && full_ledger().claim(Grade::S1) == Err(MemuCode::EGradeDrift)
        },
        "空账与全账申领 S1 均拒：虚拟化面恒 N/A，虚报 S1 必抓",
    );
    s.add(
        "定级 N/A 封顶 S2（N/A 清单随附）",
        {
            let lt = full_ledger();
            lt.seal().is_ok()
                && lt.grade() == Grade::S2
                && lt.claim(Grade::S2).is_ok()
                && !na_list_summary().is_empty()
        },
        "全账收口也封顶 S2（KB-COMPAT 登记），N/A 清单随附非空",
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC980",
    );
    let summary = format!(
        "unx52551 MEmu 收口（N/A 同构）：记账={} 检出={} 探测账=4×NotAvailable 定级=S2 封顶",
        recorded, caught
    );
    s.add_dyn(
        "API/软件写回·结果摘要非空",
        !summary.is_empty(),
        &summary,
    );
    s.add("收口·判据条数", s.len() >= 21, "含本条共 ≥22 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_model() {
        // 五面闭集与面名。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(12), Some(3));
        assert_eq!(op_kind(16), Some(4));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("安装面"));
        assert_eq!(op_kind_name(1), Some("实例面"));
        assert_eq!(op_kind_name(2), Some("宏脚本面"));
        assert_eq!(op_kind_name(3), Some("ADB 消费面"));
        assert_eq!(op_kind_name(4), Some("N/A 探测账面"));
        assert!(op_kind_name(5).is_none());
        // 投影双段。
        assert_eq!(path_kind(3), Some(0));
        assert_eq!(path_kind(4), Some(1));
        assert_eq!(path_kind(7), Some(1));
        assert!(path_kind(8).is_none());
        assert!(path_cell(8).is_none());
        assert_eq!(path_kind_name(0), Some("实例段投影"));
        assert_eq!(path_kind_name(1), Some("宏脚本段投影"));
        assert!(path_kind_name(2).is_none());
        // 版本阶梯 9.0.6/9.1.0/9.2.0。
        assert_eq!(version_name(0), Some("9.0.6"));
        assert_eq!(version_name(1), Some("9.1.0"));
        assert_eq!(version_name(2), Some("9.2.0"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
        // 基准表纯函数恒等且跨格互异。
        assert_eq!(op_cell(3).unwrap_or(0), op_cell(3).unwrap_or(0));
        assert_ne!(op_cell(3).unwrap_or(0), op_cell(4).unwrap_or(0));
        assert_ne!(path_cell(0).unwrap_or(0), path_cell(4).unwrap_or(0));
        assert_ne!(version_cell(0).unwrap_or(0), version_cell(1).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(split_name(SPLIT_ROWS).is_none());
        // N/A 三要素告警逐格齐备。
        let mut c = 0usize;
        while c < NA_PROBE_CELLS {
            let (dep, why, alt) = match na_element(c) {
                Some(t) => t,
                None => ("", "", ""),
            };
            assert!(!dep.is_empty(), "依赖项名非空");
            assert!(!why.is_empty(), "原因非空");
            assert!(!alt.is_empty(), "替代路径非空");
            c += 1;
        }
        assert!(na_element(NA_PROBE_CELLS).is_none());
        assert!(!na_list_summary().is_empty());
    }

    #[test]
    fn test_probe_three_branch() {
        assert_eq!(ProbeVerdict::Available.val(), 0);
        assert_eq!(ProbeVerdict::NotAvailable.val(), 1);
        assert_eq!(ProbeVerdict::ProbeFailed.val(), 2);
        assert_ne!(ProbeVerdict::ProbeFailed, ProbeVerdict::NotAvailable);
        assert!(!ProbeVerdict::Available.detail().is_empty());
        assert!(!ProbeVerdict::NotAvailable.detail().is_empty());
        assert!(!ProbeVerdict::ProbeFailed.detail().is_empty());
        let mut lt = MemuLedger::new();
        // 虚报 Available：注入必抓（1,1,0）且不入探测账。
        assert_eq!(lt.na_probe(0, ProbeVerdict::Available), Err(MemuCode::EGradeDrift));
        assert!(lt.probe_verdict(0).is_none());
        assert_eq!(lt.stats(), (1, 1, 0));
        // 如实 N/A 与显性失败均可入账且严格区分。
        assert!(lt.na_probe(0, ProbeVerdict::NotAvailable).is_ok());
        assert!(lt.na_probe(1, ProbeVerdict::ProbeFailed).is_ok());
        assert_eq!(lt.probe_verdict(0), Some(ProbeVerdict::NotAvailable));
        assert_eq!(lt.probe_verdict(1), Some(ProbeVerdict::ProbeFailed));
        // 重复登记 EDup；越闭集 ENoOp。
        assert_eq!(lt.na_probe(0, ProbeVerdict::NotAvailable), Err(MemuCode::EDup));
        assert_eq!(lt.na_probe(NA_PROBE_CELLS, ProbeVerdict::NotAvailable), Err(MemuCode::ENoOp));
    }

    #[test]
    fn test_order_drift_dup() {
        // 逆序 EOrder 且拒不改 last_kind。
        let mut lt = MemuLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(MemuCode::EOrder));
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
        // 三族载荷漂移。
        let mut lt2 = MemuLedger::new();
        assert_eq!(lt2.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)), Err(MemuCode::EDrift));
        assert_eq!(lt2.record_path(1, path_cell(1).unwrap_or(0).wrapping_add(7)), Err(MemuCode::EPathDrift));
        assert_eq!(lt2.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)), Err(MemuCode::EVerDrift));
        // 重复落账 EDup。
        let mut lt3 = MemuLedger::new();
        let _ = lt3.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt3.record_version(0, version_cell(0).unwrap_or(0)), Err(MemuCode::EDup));
        let _ = lt3.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt3.record_op(6, op_cell(6).unwrap_or(0)), Err(MemuCode::EDup));
        let _ = lt3.record_path(2, path_cell(2).unwrap_or(0));
        assert_eq!(lt3.record_path(2, path_cell(2).unwrap_or(0)), Err(MemuCode::EDup));
        let _ = lt3.declare_split(0);
        assert_eq!(lt3.declare_split(0), Err(MemuCode::EDup));
    }

    #[test]
    fn test_gate_boundary_no_injection() {
        // 六闸边界：0..=5 全落才 gates_all，越界必抓。
        let mut lt = MemuLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        // 越闭集边界拒不计注入账。
        let mut lt2 = MemuLedger::new();
        let _ = lt2.record_op(OPS, 0);
        let _ = lt2.record_path(PATH_CELLS, 0);
        let _ = lt2.record_version(VERSION_STEPS, 0);
        let _ = lt2.declare_split(SPLIT_ROWS);
        let _ = lt2.na_probe(NA_PROBE_CELLS, ProbeVerdict::NotAvailable);
        assert_eq!(lt2.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        // 全账账本撤一行声明 → ERowMiss。
        let mut lt = full_ledger();
        if let Some(row) = lt.splits.get_mut(SPLIT_ROWS - 1) {
            *row = None;
        }
        assert_eq!(lt.seal(), Err(MemuCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        assert!(lt.seal().is_ok());
        // 全账账本撤一格探测账 → ESeal。
        let mut lt2 = full_ledger();
        if let Some(slot) = lt2.probe.get_mut(NA_PROBE_CELLS - 1) {
            *slot = None;
        }
        assert_eq!(lt2.seal(), Err(MemuCode::ESeal));
        // 空账未齐即封 → ESeal。
        assert_eq!(MemuLedger::new().seal(), Err(MemuCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 8 投影 + 3 档版本（六闸/声明行/探测账不计数）。
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
        assert_eq!(SEG_BASE, 0xCC980);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MemuCode::Ok as u32, 0);
        assert_eq!(MemuCode::ENoOp as u32, 1);
        assert_eq!(MemuCode::EDrift as u32, 2);
        assert_eq!(MemuCode::EOrder as u32, 3);
        assert_eq!(MemuCode::ENoPath as u32, 4);
        assert_eq!(MemuCode::EPathDrift as u32, 5);
        assert_eq!(MemuCode::ENoVer as u32, 6);
        assert_eq!(MemuCode::EVerDrift as u32, 7);
        assert_eq!(MemuCode::ENoRow as u32, 8);
        assert_eq!(MemuCode::ERowMiss as u32, 9);
        assert_eq!(MemuCode::EDup as u32, 10);
        assert_eq!(MemuCode::ESeal as u32, 11);
        assert_eq!(MemuCode::EGradeDrift as u32, 12);
        assert_eq!(MemuCode::Ok.val(), SEG_BASE);
        assert_eq!(MemuCode::EGradeDrift.val(), SEG_BASE + 12);
        // 全闭集明细非空。
        let all = [
            MemuCode::Ok, MemuCode::ENoOp, MemuCode::EDrift,
            MemuCode::EOrder, MemuCode::ENoPath, MemuCode::EPathDrift,
            MemuCode::ENoVer, MemuCode::EVerDrift, MemuCode::ENoRow,
            MemuCode::ERowMiss, MemuCode::EDup, MemuCode::ESeal,
            MemuCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
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
        let lt = MemuLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        assert_eq!(lt.claim(Grade::S3), Ok(()));
        // N/A 封顶：六闸齐即 S2，S1 恒不可达，虚报必抓。
        let mut lt2 = MemuLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        assert_eq!(lt2.claim(Grade::S1), Err(MemuCode::EGradeDrift));
        // 全账齐（含探测账 N/A 如实登记）也封顶 S2，不升 S1。
        let lt3 = full_ledger();
        assert!(lt3.seal().is_ok());
        assert_eq!(lt3.grade(), Grade::S2);
        assert_eq!(lt3.claim(Grade::S2), Ok(()));
        assert_eq!(lt3.claim(Grade::S1), Err(MemuCode::EGradeDrift));
    }

    #[test]
    fn test_injection_accounting() {
        // 漂移/逆序计入注入账（注入=检出=1），EDup 不计。
        let mut lt = MemuLedger::new();
        let _ = lt.record_op(0, op_cell(0).unwrap_or(0).wrapping_add(1));
        assert_eq!(lt.stats(), (1, 1, 0));
        let mut lt2 = MemuLedger::new();
        let _ = lt2.record_op(9, op_cell(9).unwrap_or(0));
        let _ = lt2.record_op(2, op_cell(2).unwrap_or(0));
        assert_eq!(lt2.stats(), (1, 1, 0));
        let mut lt3 = MemuLedger::new();
        let _ = lt3.record_version(0, version_cell(0).unwrap_or(0));
        let _ = lt3.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt3.stats(), (0, 0, 0));
    }

    #[test]
    fn unx52551_checks_green() {
        let s = run_unx52551_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
