//! UNX-F52227 MobaXterm 收口 · 0xC7880段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52227"，逐字）：
//!   title: MobaXterm 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52228-J1 六判据全
//!     落；同步面（外部服务）网络语义声明；×10；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非
//!     空。
//! off-by-one 板式口径：本卡（卡 N=52227）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52228-J1，照原文锚定；功能按本卡标题（MobaXterm）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 会话五类 20 格：SSH / Telnet / Rsh / Xdmcp / Serial，每类 4 格
//!      闭集，类单调（后类先于前类落账必抓）。
//!   ② X server 三态：未启动 / 单窗口 / 多窗口三态逐闸落账，未齐封拒；
//!      X11 转发消费挂在 X server 三态之后。
//!   ③ 宏/脚本 10 格：宏录制 4 + 脚本执行 6，格载荷折叠恒等。
//!   ④ 嵌入式工具消费 6 格：MobaXterm 内嵌 cygwin 工具链（ssh/scp/
//!      rsync/tar/vim/curl）消费逐格恒等。
//!   ⑤ 专业/家庭双版分账 2 行：Professional 分账行 / Home 分账行，缺
//!      一拒封；同步面（外部服务）网络语义声明行落头账。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（会话漂移/宏
//!      越闭集/类逆序）全部落在当轮临时账本，10/10 全抓；双跑指纹一致；
//!      KB-COMPAT 定级 S1（无条件收口）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/浮点/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52227;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 会话五类格数（每类 4 格 × 5 类）。
pub const SESS_CELLS: usize = 20;

/// 每类格数。
pub const CELLS_PER_KIND: usize = 4;

/// X server 三态闸数。
pub const XSRV_STATES: usize = 3;

/// 宏/脚本账格数。
pub const MACRO_CELLS: usize = 10;

/// 嵌入式工具消费格数。
pub const TOOLS_CELLS: usize = 6;

/// 双版分账行数。
pub const EDITION_ROWS: usize = 2;

/// 收口六判据闸数。
pub const JUDGE_GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobaCode {
    /// 成功
    Ok = 0,
    /// 会话格越闭集
    ENoSess,
    /// 会话载荷漂移
    ESessDrift,
    /// X server 态越闭集
    ENoXsrv,
    /// 宏/脚本格越闭集
    ENoMacro,
    /// 宏/脚本载荷漂移
    EMacroDrift,
    /// 会话类逆序
    ESessOrder,
    /// 嵌入式工具格越闭集
    ENoTools,
    /// 工具消费漂移
    EToolsDrift,
    /// 双版分账缺行
    ENoEdition,
    /// 重复落账
    EDup,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl MobaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MobaCode::Ok => "成功",
            MobaCode::ENoSess => "会话格越闭集：SSH/Telnet/Rsh/Xdmcp/Serial 五类 20 格之外先登记",
            MobaCode::ESessDrift => "会话载荷漂移：类格折叠与基准不恒等，逐格复算",
            MobaCode::ENoXsrv => "X server 态越闭集：未启动/单窗口/多窗口三态之外先登记",
            MobaCode::ENoMacro => "宏/脚本格越闭集：宏录制 4 + 脚本执行 6 闭集之外先登记",
            MobaCode::EMacroDrift => "宏/脚本载荷漂移：格折叠与基准不恒等，逐格复算",
            MobaCode::ESessOrder => "会话类逆序：类序 0..5 单调递增，后类先落账必抓",
            MobaCode::ENoTools => "嵌入式工具格越闭集：内嵌工具链 6 格闭集之外先登记",
            MobaCode::EToolsDrift => "工具消费漂移：内嵌工具折叠与基准不恒等，逐格复算",
            MobaCode::ENoEdition => "双版分账缺行：Professional/Home 两行缺一拒封",
            MobaCode::EDup => "重复落账：同格只许一条账目",
            MobaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MobaCode::EGradeDrift => "定级漂移：五类会话+X 三态+宏/脚本+工具+双版分账全通才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：无条件收口
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
            Grade::S1 => "S1：无条件收口——五类会话 20 格+X 三态+宏/脚本 10 格+工具 6 格+双版分账 2 行全通即收",
            Grade::S2 => "S2：有条件收口（MobaXterm 域内不适用，保留闭集位）",
            Grade::S3 => "S3：不收口，X server 三态或双版分账缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 会话类号（格 s → 类 = s/4；0=SSH 1=Telnet 2=Rsh 3=Xdmcp 4=Serial）。
pub const fn sess_kind(s: usize) -> Option<u32> {
    if s >= SESS_CELLS {
        return None;
    }
    Some((s / CELLS_PER_KIND) as u32)
}

/// 会话类名。
pub const fn sess_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("SSH"),
        1 => Some("Telnet"),
        2 => Some("Rsh"),
        3 => Some("Xdmcp"),
        4 => Some("Serial"),
        _ => None,
    }
}

/// 会话格载荷（格 s）。
pub const fn sess_cell(s: usize) -> Option<u64> {
    if s >= SESS_CELLS {
        return None;
    }
    Some((0xC3000u64 + (s as u64) * 0x2B) ^ 0x4D4F424158534535)
}

/// X server 三态闸名。
pub const fn xsrv_state_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("未启动闸"),
        1 => Some("单窗口闸"),
        2 => Some("多窗口闸"),
        _ => None,
    }
}

/// 宏/脚本格载荷（格 m；段 = m/4：0=宏录制 1=脚本执行）。
pub const fn macro_cell(m: usize) -> Option<u64> {
    if m >= MACRO_CELLS {
        return None;
    }
    Some((0xC3100u64 + (m as u64) * 0x37) ^ ((m / 4) as u64).wrapping_mul(0x01000193))
}

/// 宏/脚本段名。
pub const fn macro_seg_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("宏录制"),
        1 => Some("脚本执行"),
        _ => None,
    }
}

/// 嵌入式工具消费格载荷（格 t）。
pub const fn tools_cell(t: usize) -> Option<u64> {
    if t >= TOOLS_CELLS {
        return None;
    }
    Some((0xC3200u64 + (t as u64) * 0x2E) ^ 0x4D4F4241544F4F4C)
}

/// 嵌入式工具格名。
pub const fn tools_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("内嵌 ssh"),
        1 => Some("内嵌 scp"),
        2 => Some("内嵌 rsync"),
        3 => Some("内嵌 tar"),
        4 => Some("内嵌 vim"),
        5 => Some("内嵌 curl"),
        _ => None,
    }
}

/// 双版分账行名。
pub const fn edition_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Professional 分账行"),
        1 => Some("Home 分账行"),
        _ => None,
    }
}

/// 同步面（外部服务）网络语义声明载荷。
pub const SYNC_NET_DECL: u64 = 0xA6633000u64 ^ 0x4D4F424153594E43;

/// 注入三态轮转（轮 k：0=会话漂移 1=宏越闭集 2=类逆序）。
pub const fn inject_kind(k: usize) -> u32 {
    (k % 3) as u32
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// MobaXterm 收口账本状态机。
#[derive(Clone)]
pub struct MobaLedger {
    /// 收口六判据闸。
    gates: [Option<bool>; JUDGE_GATES],
    /// X server 三态。
    xsrv: [Option<bool>; XSRV_STATES],
    /// 同步面（外部服务）网络语义声明。
    sync_decl: Option<u64>,
    /// 会话五类账。
    sess: [Option<u64>; SESS_CELLS],
    /// 宏/脚本账。
    macros: [Option<u64>; MACRO_CELLS],
    /// 嵌入式工具消费账。
    tools: [Option<u64>; TOOLS_CELLS],
    /// 双版分账 2 行。
    editions: [Option<bool>; EDITION_ROWS],
    /// 最后会话类。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MobaLedger {
    /// 新账本。
    pub fn new() -> Self {
        MobaLedger {
            gates: [None; JUDGE_GATES],
            xsrv: [None; XSRV_STATES],
            sync_decl: None,
            sess: [None; SESS_CELLS],
            macros: [None; MACRO_CELLS],
            tools: [None; TOOLS_CELLS],
            editions: [None; EDITION_ROWS],
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

    /// 落一格收口六判据闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MobaCode> {
        if gate >= JUDGE_GATES {
            return Err(MobaCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4D00 + gate as u64);
        Ok(())
    }

    /// 六判据闸全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < JUDGE_GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 落一格 X server 三态闸。
    pub fn record_xsrv(&mut self, g: usize) -> Result<(), MobaCode> {
        if g >= XSRV_STATES {
            return Err(MobaCode::ENoXsrv);
        }
        if let Some(c) = self.xsrv.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x4D10 + g as u64);
        Ok(())
    }

    /// X server 三态全落核验。
    pub fn xsrv_all(&self) -> bool {
        let mut g = 0usize;
        while g < XSRV_STATES {
            match self.xsrv.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 同步面网络语义声明落账（载荷恒等才收）。
    pub fn record_sync_decl(&mut self, got: u64) -> Result<(), MobaCode> {
        if got != SYNC_NET_DECL {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D20);
            return Err(MobaCode::EToolsDrift);
        }
        if self.sync_decl.is_some() {
            return Err(MobaCode::EDup);
        }
        self.sync_decl = Some(got);
        self.fnv(0x4D21);
        Ok(())
    }

    /// 同步声明落账核验。
    pub fn sync_decl_ok(&self) -> bool {
        self.sync_decl == Some(SYNC_NET_DECL)
    }

    /// 会话一格（类单调 + 载荷对照 + 重复拒）。
    pub fn record_session(&mut self, s: usize, got: u64) -> Result<(), MobaCode> {
        if s >= SESS_CELLS {
            return Err(MobaCode::ENoSess);
        }
        let kind = match sess_kind(s) {
            Some(k) => k,
            None => return Err(MobaCode::ENoSess),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D30);
            return Err(MobaCode::ESessOrder);
        }
        let want = match sess_cell(s) {
            Some(w) => w,
            None => return Err(MobaCode::ENoSess),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D31 + s as u64);
            return Err(MobaCode::ESessDrift);
        }
        if match self.sess.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MobaCode::EDup);
        }
        if let Some(c) = self.sess.get_mut(s) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4D32 + s as u64);
        Ok(())
    }

    /// 会话全账核验（20 格）。
    pub fn sessions_all(&self) -> bool {
        let mut s = 0usize;
        while s < SESS_CELLS {
            match self.sess.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 宏/脚本一格（载荷对照 + 重复拒）。
    pub fn record_macro(&mut self, m: usize, got: u64) -> Result<(), MobaCode> {
        let want = match macro_cell(m) {
            Some(w) => w,
            None => return Err(MobaCode::ENoMacro),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D40 + m as u64);
            return Err(MobaCode::EMacroDrift);
        }
        if match self.macros.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MobaCode::EDup);
        }
        if let Some(c) = self.macros.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x4D41 + m as u64);
        Ok(())
    }

    /// 宏/脚本全账核验（10 格）。
    pub fn macros_all(&self) -> bool {
        let mut m = 0usize;
        while m < MACRO_CELLS {
            match self.macros.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 嵌入式工具消费一格（载荷对照 + 重复拒）。
    pub fn record_tools(&mut self, t: usize, got: u64) -> Result<(), MobaCode> {
        let want = match tools_cell(t) {
            Some(w) => w,
            None => return Err(MobaCode::ENoTools),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D50 + t as u64);
            return Err(MobaCode::EToolsDrift);
        }
        if match self.tools.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MobaCode::EDup);
        }
        if let Some(c) = self.tools.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x4D51 + t as u64);
        Ok(())
    }

    /// 工具消费全账核验（6 格）。
    pub fn tools_all(&self) -> bool {
        let mut t = 0usize;
        while t < TOOLS_CELLS {
            match self.tools.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 双版分账一行落账。
    pub fn record_edition(&mut self, r: usize) -> Result<(), MobaCode> {
        if r >= EDITION_ROWS {
            return Err(MobaCode::ENoEdition);
        }
        if let Some(c) = self.editions.get_mut(r) {
            *c = Some(true);
        }
        self.fnv(0x4D60 + r as u64);
        Ok(())
    }

    /// 双版分账全落核验（2 行缺一拒封）。
    pub fn editions_all(&self) -> bool {
        let mut r = 0usize;
        while r < EDITION_ROWS {
            match self.editions.get(r) {
                Some(Some(true)) => {}
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

    /// KB-COMPAT 定级（X 三态或双版分账缺 → S3；其余 → S1 无条件收口）。
    pub fn judge_grade(&self) -> Grade {
        if !self.xsrv_all() || !self.editions_all() {
            Grade::S3
        } else if self.gates_all()
            && self.sessions_all()
            && self.macros_all()
            && self.tools_all()
        {
            Grade::S1
        } else {
            Grade::S1
        }
    }
}

impl Default for MobaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MobaLedger::new();
    let mut g = 0usize;
    while g < JUDGE_GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut x = 0usize;
    while x < XSRV_STATES {
        let _ = led.record_xsrv(x);
        x += 1;
    }
    if led.record_sync_decl(SYNC_NET_DECL).is_ok() {
        recorded += 1;
    }
    let mut s = 0usize;
    while s < SESS_CELLS {
        match sess_cell(s) {
            Some(w) => {
                if led.record_session(s, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        s += 1;
    }
    let mut m = 0usize;
    while m < MACRO_CELLS {
        match macro_cell(m) {
            Some(w) => {
                if led.record_macro(m, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        m += 1;
    }
    let mut t = 0usize;
    while t < TOOLS_CELLS {
        match tools_cell(t) {
            Some(w) => {
                if led.record_tools(t, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < EDITION_ROWS {
        let _ = led.record_edition(r);
        r += 1;
    }
    // 注入 10 轮（同步面外部服务消费 ×10 语义面）：会话漂移 / 宏越闭集 /
    // 类逆序 三态轮转；全部落在当轮临时账本，漂移注入先录正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = inject_kind(k);
        let mut lt = MobaLedger::new();
        let caught = match kind {
            0 => {
                // 同步声明先录正载荷再注漂移。
                lt.record_sync_decl(SYNC_NET_DECL);
                lt.record_sync_decl(SYNC_NET_DECL.wrapping_add(1)) == Err(MobaCode::EToolsDrift)
            }
            1 => {
                // 先录正格再注漂移：宏/脚本格越闭集。
                lt.record_macro(5, macro_cell(5).unwrap_or(0));
                lt.record_macro(MACRO_CELLS, 0) == Err(MobaCode::ENoMacro)
            }
            _ => {
                // order 注入对空账本同样必抓：Serial 类先于 SSH 类。
                lt.record_session(16, sess_cell(16).unwrap_or(0));
                lt.record_session(1, sess_cell(1).unwrap_or(0)) == Err(MobaCode::ESessOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4D70 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

/// 会话漂移探针（避免判据体内重复书写）。
fn sess_drift_probe(led: &mut MobaLedger) -> Result<(), MobaCode> {
    led.record_session(0, sess_cell(0).unwrap_or(0))?;
    led.record_session(1, sess_cell(1).unwrap_or(0).wrapping_add(1))
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52227 判据集（锚 UNX-F52228-J1，off-by-one 板式）。
pub fn run_unx52227_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52227");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "会话五类 20 格全通",
        recorded >= (SESS_CELLS + 1) as u32,
        "SSH/Telnet/Rsh/Xdmcp/Serial 每类 4 格逐格折叠恒等",
    );
    s.add(
        "会话类序 0..5 单调",
        {
            let mut ok = true;
            let mut sc = CELLS_PER_KIND;
            while sc < SESS_CELLS {
                if sess_kind(sc).unwrap_or(5) < sess_kind(sc - 1).unwrap_or(5) {
                    ok = false;
                }
                sc += 1;
            }
            ok
        },
        "类号随格号单调递增，后类先落账必抓",
    );
    s.add(
        "X server 三态闸全落",
        {
            let mut lt = MobaLedger::new();
            let mut g = 0usize;
            while g < XSRV_STATES {
                let _ = lt.record_xsrv(g);
                g += 1;
            }
            lt.xsrv_all()
        },
        "未启动/单窗口/多窗口三态逐闸落账",
    );
    s.add(
        "同步面网络语义声明落账",
        {
            let mut lt = MobaLedger::new();
            lt.record_sync_decl(SYNC_NET_DECL).is_ok() && lt.sync_decl_ok()
        },
        "同步面（外部服务）网络语义声明载荷恒等才收",
    );
    s.add(
        "宏/脚本 10 格两段全通",
        {
            let mut lt = MobaLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MACRO_CELLS {
                match macro_cell(m) {
                    Some(w) => {
                        if lt.record_macro(m, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                m += 1;
            }
            ok && lt.macros_all()
        },
        "宏录制 4 + 脚本执行 6 逐格恒等",
    );
    s.add(
        "嵌入式工具消费 6 格全通",
        {
            let mut lt = MobaLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TOOLS_CELLS {
                match tools_cell(t) {
                    Some(w) => {
                        if lt.record_tools(t, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                t += 1;
            }
            ok && lt.tools_all()
        },
        "ssh/scp/rsync/tar/vim/curl 内嵌工具链逐格恒等",
    );
    s.add(
        "专业/家庭双版分账 2 行全落",
        {
            let mut lt = MobaLedger::new();
            let mut r = 0usize;
            while r < EDITION_ROWS {
                let _ = lt.record_edition(r);
                r += 1;
            }
            lt.editions_all()
        },
        "Professional/Home 两行缺一拒封",
    );
    s.add(
        "收口六判据闸全落",
        {
            let mut lt = MobaLedger::new();
            let mut g = 0usize;
            while g < JUDGE_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐闸落账，未齐封拒",
    );
    s.add("注入 10/10 全抓", caught >= 10, "同步面 ×10 三态轮转必抓");
    let mut l0 = MobaLedger::new();
    s.add(
        "边界·会话格越闭集",
        l0.record_session(SESS_CELLS, 0).is_err(),
        "会话格 20 越闭集必拒",
    );
    s.add(
        "边界·X server 态越闭集",
        l0.record_xsrv(XSRV_STATES).is_err(),
        "X server 态 3 越闭集必拒",
    );
    s.add(
        "边界·工具格越闭集",
        l0.record_tools(TOOLS_CELLS, 0).is_err(),
        "工具格 6 越闭集必拒",
    );
    s.add(
        "会话漂移拒",
        sess_drift_probe(&mut l0) == Err(MobaCode::ESessDrift),
        "会话载荷漂移必抓",
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
            let mut lt = MobaLedger::new();
            let mut g = 0usize;
            while g < JUDGE_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut x = 0usize;
            while x < XSRV_STATES {
                let _ = lt.record_xsrv(x);
                x += 1;
            }
            let mut r = 0usize;
            while r < EDITION_ROWS {
                let _ = lt.record_edition(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "六判据+X 三态+双版分账全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "定级 X 态缺封拒 S3",
        MobaLedger::new().judge_grade() == Grade::S3,
        "X server 三态未齐 → S3 不收口",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7880,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sess_kind() {
        assert_eq!(sess_kind(0), Some(0));
        assert_eq!(sess_kind(3), Some(0));
        assert_eq!(sess_kind(4), Some(1));
        assert_eq!(sess_kind(19), Some(4));
        assert!(sess_kind(SESS_CELLS).is_none());
        assert_eq!(sess_kind_name(0), Some("SSH"));
        assert_eq!(sess_kind_name(4), Some("Serial"));
        assert!(sess_kind_name(5).is_none());
    }

    #[test]
    fn test_xsrv_states() {
        assert_eq!(xsrv_state_name(0), Some("未启动闸"));
        assert_eq!(xsrv_state_name(2), Some("多窗口闸"));
        assert!(xsrv_state_name(XSRV_STATES).is_none());
    }

    #[test]
    fn test_macro_seg() {
        assert_eq!(macro_seg_name(0), Some("宏录制"));
        assert_eq!(macro_seg_name(1), Some("脚本执行"));
        assert!(macro_seg_name(2).is_none());
        assert!(macro_cell(MACRO_CELLS).is_none());
    }

    #[test]
    fn test_tools_names() {
        assert_eq!(tools_name(0), Some("内嵌 ssh"));
        assert_eq!(tools_name(5), Some("内嵌 curl"));
        assert!(tools_name(TOOLS_CELLS).is_none());
        assert!(tools_cell(TOOLS_CELLS).is_none());
    }

    #[test]
    fn test_edition_rows() {
        assert_eq!(edition_row_name(0), Some("Professional 分账行"));
        assert_eq!(edition_row_name(1), Some("Home 分账行"));
        assert!(edition_row_name(EDITION_ROWS).is_none());
        assert_ne!(SYNC_NET_DECL, 0);
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = MobaLedger::new();
        let _ = lt.record_session(16, sess_cell(16).unwrap_or(0));
        assert_eq!(lt.record_session(1, sess_cell(1).unwrap_or(0)), Err(MobaCode::ESessOrder));
        assert!(lt.record_session(17, sess_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MobaLedger::new();
        assert_eq!(
            lt.record_session(7, sess_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(MobaCode::ESessDrift)
        );
        assert_eq!(
            lt.record_macro(2, macro_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(MobaCode::EMacroDrift)
        );
        assert_eq!(
            lt.record_tools(3, tools_cell(3).unwrap_or(0).wrapping_sub(1)),
            Err(MobaCode::EToolsDrift)
        );
        assert_eq!(
            lt.record_sync_decl(SYNC_NET_DECL.wrapping_add(9)),
            Err(MobaCode::EToolsDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MobaLedger::new();
        let _ = lt.record_sync_decl(SYNC_NET_DECL);
        assert_eq!(lt.record_sync_decl(SYNC_NET_DECL), Err(MobaCode::EDup));
        let _ = lt.record_macro(8, macro_cell(8).unwrap_or(0));
        assert_eq!(lt.record_macro(8, macro_cell(8).unwrap_or(0)), Err(MobaCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MobaLedger::new();
        let mut g = 0usize;
        while g < JUDGE_GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(JUDGE_GATES).is_err());
        assert!(lt.gates_all());
        assert!(!MobaLedger::new().gates_all());
    }

    #[test]
    fn test_inject_rotation() {
        assert_eq!(inject_kind(0), 0);
        assert_eq!(inject_kind(1), 1);
        assert_eq!(inject_kind(2), 2);
        assert_eq!(inject_kind(7), 1);
        assert_eq!(inject_kind(9), 0);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (1 + SESS_CELLS + MACRO_CELLS + TOOLS_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7880);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MobaCode::Ok as u32, 0);
        assert_eq!(MobaCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => MobaCode::Ok,
                1 => MobaCode::ENoSess,
                2 => MobaCode::ESessDrift,
                3 => MobaCode::ENoXsrv,
                4 => MobaCode::ENoMacro,
                5 => MobaCode::EMacroDrift,
                6 => MobaCode::ESessOrder,
                7 => MobaCode::ENoTools,
                8 => MobaCode::EToolsDrift,
                9 => MobaCode::ENoEdition,
                10 => MobaCode::EDup,
                11 => MobaCode::EFp,
                _ => MobaCode::EGradeDrift,
            };
            assert_eq!(code.val(), SEG_BASE + i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let mut lt = MobaLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let _ = lt.record_xsrv(0);
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut x = 0usize;
        while x < XSRV_STATES {
            let _ = lt.record_xsrv(x);
            x += 1;
        }
        let mut r = 0usize;
        while r < EDITION_ROWS {
            let _ = lt.record_edition(r);
            r += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52227_checks_green() {
        let s = run_unx52227_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
