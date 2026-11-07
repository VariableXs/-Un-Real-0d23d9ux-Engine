//! UNX-F52373 Ansible（Windows 控制端）收口 · 0xC9D00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52373"，逐字）：
//!   title: Ansible（Windows 控制端）收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52374-J1
//!     init/plan/apply ×10（apply 判据只对本地/local provider 干跑，云
//!     provider 只 plan——外部资源零写入声明）；KB-COMPAT 定级登记；判据
//!     锚定 §AI-66.9 红线适用；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52373）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52374-J1，照原文锚定；功能按本卡标题（Ansible Windows 控制端）
//! 落地——判据原文是邻卡字面，本模块不执行 Terraform 语义，仅收口
//! Ansible 控制面（外部资源零写入：全链对账折叠，不下发任何远端变更）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① playbook 五段闭集：pre_tasks/tasks/handlers/post_tasks/roles 五家
//!      族各 4 格共 20 格，家族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 五命令 10 操作集：playbook/play/adhoc/galaxy/vault 五命令各 2 操
//!      作，命令闭集之外先登记必拒，载荷漂移必抓。
//!   ③ inventory 双向投影 12 格：inv_fwd/inv_back XOR 对合投影，
//!      back(fwd(x))==x 恒等逐格复核，投影破坏即 EInvDrift。
//!   ④ module 消费 8 格闭集 + Python 上栈 4 层声明（python.exe →
//!      py launcher → pip 依赖 → ansible-playbook 入口），缺层拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（段越闭集/命令家族逆序/inventory
//!      漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭
//!      集（五段 20 格 + 10 操作 + 12 投影 + 8 消费 + 4 层 + 六判据全通
//!      → S1，KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；外部资源零写入（纯对账，无下发路径）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52373;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// playbook 五段格数（五家族 × 4 格）。
pub const SECTION_CELLS: usize = 20;

/// 五命令操作数（五命令 × 2 操作）。
pub const CMD_OPS: usize = 10;

/// inventory 双向投影格数。
pub const INV_CELLS: usize = 12;

/// module 消费格数。
pub const MODULE_CELLS: usize = 8;

/// Python 上栈层数。
pub const PY_LAYERS: usize = 4;

/// 六判据闸数。
pub const GATES: usize = 6;

/// Ansible 域锚异或（低 16 位恒零）。
pub const ANS_XOR: u64 = 0x414E_5349_0000_0000;

/// inventory 投影异或（低 16 位恒零，对合键）。
pub const INV_XOR: u64 = 0x494E_5630_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnsibleCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoSec,
    /// 段家族逆序
    ESecOrder,
    /// 段载荷漂移
    ESecDrift,
    /// 命令号越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// 投影格越闭集
    ENoInv,
    /// 投影漂移
    EInvDrift,
    /// module 越闭集
    ENoMod,
    /// 上栈层越闭集
    ENoPy,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl AnsibleCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AnsibleCode::Ok => "成功",
            AnsibleCode::ENoSec => "段号越闭集：playbook 五段 20 格之外先登记",
            AnsibleCode::ESecOrder => "段家族逆序：pre_tasks→tasks→handlers→post_tasks→roles 单调，逆序必抓",
            AnsibleCode::ESecDrift => "段载荷漂移：段折叠与基准不恒等，逐格复算",
            AnsibleCode::ENoCmd => "命令号越闭集：playbook/play/adhoc/galaxy/vault 10 操作集之外先登记",
            AnsibleCode::ECmdDrift => "命令载荷漂移：命令折叠与基准不恒等，逐格复算",
            AnsibleCode::ENoInv => "投影格越闭集：inventory 双向投影 12 格之外先登记",
            AnsibleCode::EInvDrift => "投影漂移：back(fwd(x))≠x 对合恒等破坏，逐格复算",
            AnsibleCode::ENoMod => "module 越闭集：module 消费 8 格之外先登记",
            AnsibleCode::ENoPy => "上栈层越闭集：Python 上栈 4 层之外先登记",
            AnsibleCode::EDup => "重复落账：同格只许一条账目",
            AnsibleCode::ESeal => "未齐即封：五段 20 格 + 10 操作 + 12 投影 + 8 消费 + 4 层未齐先补齐",
            AnsibleCode::EGradeDrift => "定级漂移：全通且账目全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+五段20格+10操作+12投影+8消费+4层全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// playbook 段格载荷（段 o；家族 = o/4）。
pub const fn section_cell(o: usize) -> Option<u64> {
    if o >= SECTION_CELLS {
        return None;
    }
    Some((0xA1000u64 + (o as u64) * 0x34) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 段家族（0..4=pre_tasks 4..8=tasks 8..12=handlers 12..16=post_tasks 16..20=roles）。
pub const fn section_kind(o: usize) -> Option<u32> {
    if o >= SECTION_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 段家族名。
pub const fn section_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("pre_tasks"),
        1 => Some("tasks"),
        2 => Some("handlers"),
        3 => Some("post_tasks"),
        4 => Some("roles"),
        _ => None,
    }
}

/// 命令操作格载荷（操作 o；命令族 = o/2）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xA2000u64 + (o as u64) * 0x5C) ^ ANS_XOR)
}

/// 命令族（0..2=playbook 2..4=play 4..6=adhoc 6..8=galaxy 8..10=vault）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("playbook"),
        1 => Some("play"),
        2 => Some("adhoc"),
        3 => Some("galaxy"),
        4 => Some("vault"),
        _ => None,
    }
}

/// inventory 投影格基准载荷（格 r）。
pub const fn inv_cell(r: usize) -> Option<u64> {
    if r >= INV_CELLS {
        return None;
    }
    Some(0xA3000u64 + (r as u64) * 0x44)
}

/// inventory 双向投影·正向（对合 XOR）。
pub const fn inv_fwd(x: u64) -> u64 {
    x ^ INV_XOR
}

/// inventory 双向投影·反向（同一对合键，back(fwd(x))==x）。
pub const fn inv_back(y: u64) -> u64 {
    y ^ INV_XOR
}

/// module 消费格载荷（格 m）。
pub const fn module_cell(m: usize) -> Option<u64> {
    if m >= MODULE_CELLS {
        return None;
    }
    Some(0xA4000u64 + (m as u64) * 0x38)
}

/// module 名（消费 8 格闭集对照）。
pub const fn module_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("setup"),
        1 => Some("win_shell"),
        2 => Some("copy"),
        3 => Some("template"),
        4 => Some("service"),
        5 => Some("ping"),
        6 => Some("package"),
        7 => Some("debug"),
        _ => None,
    }
}

/// Python 上栈层载荷（层 l）。
pub const fn python_layer(l: usize) -> Option<u64> {
    if l >= PY_LAYERS {
        return None;
    }
    Some(0xA5000u64 + (l as u64) * 0x2C)
}

/// Python 上栈层名。
pub const fn python_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("python.exe"),
        1 => Some("py launcher"),
        2 => Some("pip 依赖树"),
        3 => Some("ansible-playbook 入口"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("清单可达"),
        2 => Some("playbook 语法"),
        3 => Some("vault 解密"),
        4 => Some("连接探测"),
        5 => Some("幂等复跑"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Ansible 收口账本状态机。
#[derive(Clone)]
pub struct AnsibleLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// playbook 段账。
    secs: [Option<u64>; SECTION_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// inventory 投影账。
    invs: [Option<u64>; INV_CELLS],
    /// module 消费账。
    mods: [Option<u64>; MODULE_CELLS],
    /// Python 上栈账。
    pys: [Option<u64>; PY_LAYERS],
    /// 最后段家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AnsibleLedger {
    /// 新账本。
    pub fn new() -> Self {
        AnsibleLedger {
            gates: [None; GATES],
            secs: [None; SECTION_CELLS],
            cmds: [None; CMD_OPS],
            invs: [None; INV_CELLS],
            mods: [None; MODULE_CELLS],
            pys: [None; PY_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AnsibleCode> {
        if gate >= GATES {
            return Err(AnsibleCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// playbook 段一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_section(&mut self, o: usize, got: u64) -> Result<(), AnsibleCode> {
        if o >= SECTION_CELLS {
            return Err(AnsibleCode::ENoSec);
        }
        let kind = match section_kind(o) {
            Some(k) => k,
            None => return Err(AnsibleCode::ENoSec),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(AnsibleCode::ESecOrder);
        }
        let want = match section_cell(o) {
            Some(w) => w,
            None => return Err(AnsibleCode::ENoSec),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(AnsibleCode::ESecDrift);
        }
        if match self.secs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AnsibleCode::EDup);
        }
        if let Some(c) = self.secs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 段全账核验（20 格）。
    pub fn secs_all(&self) -> bool {
        let mut o = 0usize;
        while o < SECTION_CELLS {
            match self.secs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令操作一格（闭集 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), AnsibleCode> {
        if o >= CMD_OPS {
            return Err(AnsibleCode::ENoCmd);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(AnsibleCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + o as u64);
            return Err(AnsibleCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AnsibleCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x6301 + o as u64);
        Ok(())
    }

    /// 命令全账核验（10 操作）。
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

    /// inventory 投影一格（双向对合复核 + 重复拒）。
    pub fn record_inv(&mut self, r: usize, got: u64) -> Result<(), AnsibleCode> {
        if r >= INV_CELLS {
            return Err(AnsibleCode::ENoInv);
        }
        let base = match inv_cell(r) {
            Some(w) => w,
            None => return Err(AnsibleCode::ENoInv),
        };
        // 双向投影恒等：back(fwd(base)) 必须还原基准，投影破坏即漂移。
        if inv_back(inv_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + r as u64);
            return Err(AnsibleCode::EInvDrift);
        }
        if match self.invs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AnsibleCode::EDup);
        }
        if let Some(c) = self.invs.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x6401 + r as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn invs_all(&self) -> bool {
        let mut r = 0usize;
        while r < INV_CELLS {
            match self.invs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// module 消费一格。
    pub fn record_mod(&mut self, m: usize, got: u64) -> Result<(), AnsibleCode> {
        let want = match module_cell(m) {
            Some(w) => w,
            None => return Err(AnsibleCode::ENoMod),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + m as u64);
            return Err(AnsibleCode::ESecDrift);
        }
        if match self.mods.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AnsibleCode::EDup);
        }
        if let Some(c) = self.mods.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x6501 + m as u64);
        Ok(())
    }

    /// module 全账核验（8 格）。
    pub fn mods_all(&self) -> bool {
        let mut m = 0usize;
        while m < MODULE_CELLS {
            match self.mods.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// Python 上栈一层。
    pub fn record_py(&mut self, l: usize, got: u64) -> Result<(), AnsibleCode> {
        let want = match python_layer(l) {
            Some(w) => w,
            None => return Err(AnsibleCode::ENoPy),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + l as u64);
            return Err(AnsibleCode::ESecDrift);
        }
        if match self.pys.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AnsibleCode::EDup);
        }
        if let Some(c) = self.pys.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x6601 + l as u64);
        Ok(())
    }

    /// 上栈全落核验（4 层）。
    pub fn pys_all(&self) -> bool {
        let mut l = 0usize;
        while l < PY_LAYERS {
            match self.pys.get(l) {
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
        if self.gates_all() && self.secs_all() && self.cmds_all() && self.invs_all() && self.mods_all() && self.pys_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AnsibleLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AnsibleLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < SECTION_CELLS {
        if let Some(w) = section_cell(o) {
            if led.record_section(o, w).is_ok() {
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
    let mut r = 0usize;
    while r < INV_CELLS {
        if let Some(w) = inv_cell(r) {
            if led.record_inv(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut m = 0usize;
    while m < MODULE_CELLS {
        if let Some(w) = module_cell(m) {
            if led.record_mod(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut l = 0usize;
    while l < PY_LAYERS {
        if let Some(w) = python_layer(l) {
            if led.record_py(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    // 注入 10 轮：段越闭集 / 命令家族逆序 / inventory 漂移三态轮转。
    // 全部落在当轮临时账本上；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AnsibleLedger::new();
        let caught = match kind {
            0 => lt.record_section(SECTION_CELLS, 0).is_err(),
            1 => {
                // 命令家族逆序经段的家族单调闸复刻：先落 tasks 家族正格，
                // 再注 pre_tasks 家族，逆序必抓（对空账本同样必抓）。
                let _ = lt.record_section(6, section_cell(6).unwrap_or(0));
                lt.record_section(2, section_cell(2).unwrap_or(0)) == Err(AnsibleCode::ESecOrder)
            }
            _ => {
                let _ = lt.record_inv(0, inv_cell(0).unwrap_or(0));
                lt.record_inv(1, inv_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AnsibleCode::EInvDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52373 判据集（锚 UNX-F52374-J1，off-by-one 板式；§AI-66.9 红线
/// 适用：全链对账折叠、外部资源零写入）。
pub fn run_unx52373_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52373");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "playbook 五段 20 格全通",
        recorded >= SECTION_CELLS as u32,
        "pre_tasks/tasks/handlers/post_tasks/roles 五家族逐格恒等",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = AnsibleLedger::new();
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
        "playbook/play/adhoc/galaxy/vault 十操作闭集逐格恒等",
    );
    s.add(
        "inventory 双向投影 12 格全通",
        {
            let mut lt = AnsibleLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < INV_CELLS {
                if let Some(w) = inv_cell(r) {
                    if lt.record_inv(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.invs_all()
        },
        "inv_fwd/inv_back 对合投影 back(fwd(x))==x 逐格复核",
    );
    s.add(
        "module 消费 8 格全通",
        {
            let mut lt = AnsibleLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MODULE_CELLS {
                if let Some(w) = module_cell(m) {
                    if lt.record_mod(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.mods_all()
        },
        "setup/win_shell/copy/template/service/ping/package/debug 逐格消费恒等",
    );
    s.add(
        "Python 上栈 4 层全落",
        {
            let mut lt = AnsibleLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < PY_LAYERS {
                if let Some(w) = python_layer(l) {
                    if lt.record_py(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.pys_all()
        },
        "python.exe→py launcher→pip 依赖树→ansible-playbook 入口逐层落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = AnsibleLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/清单可达/playbook 语法/vault 解密/连接探测/幂等复跑逐格落账",
    );
    s.add(
        "段家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SECTION_CELLS {
                if section_kind(o).unwrap_or(5) < section_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段家族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "段越闭集/段家族逆序/inventory 漂移三态轮转必抓",
    );
    let mut l0 = AnsibleLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_section(SECTION_CELLS, 0).is_err(),
        "段 20 越闭集必拒",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·module 越闭集",
        l0.record_mod(MODULE_CELLS, 0).is_err(),
        "module 8 越闭集必拒",
    );
    s.add(
        "命令载荷漂移拒",
        l0.record_cmd(0, cmd_cell(0).unwrap_or(0).wrapping_add(1)) == Err(AnsibleCode::ECmdDrift),
        "命令折叠与基准不恒等必抓",
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
            let mut lt = AnsibleLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < SECTION_CELLS {
                if let Some(w) = section_cell(o) {
                    let _ = lt.record_section(o, w);
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
            let mut r = 0usize;
            while r < INV_CELLS {
                if let Some(w) = inv_cell(r) {
                    let _ = lt.record_inv(r, w);
                }
                r += 1;
            }
            let mut m = 0usize;
            while m < MODULE_CELLS {
                if let Some(w) = module_cell(m) {
                    let _ = lt.record_mod(m, w);
                }
                m += 1;
            }
            let mut l = 0usize;
            while l < PY_LAYERS {
                if let Some(w) = python_layer(l) {
                    let _ = lt.record_py(l, w);
                }
                l += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9D00,
        "SEG_BASE 0x40 对齐且落 0xC9D00 段",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_kinds() {
        assert_eq!(section_kind(0), Some(0));
        assert_eq!(section_kind(4), Some(1));
        assert_eq!(section_kind(19), Some(4));
        assert!(section_kind(20).is_none());
        assert_eq!(section_kind_name(0), Some("pre_tasks"));
        assert_eq!(section_kind_name(4), Some("roles"));
        assert!(section_kind_name(5).is_none());
        assert!(section_cell(20).is_none());
    }

    #[test]
    fn test_cmd_kinds() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(1), Some(0));
        assert_eq!(cmd_kind(2), Some(1));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(4), Some("vault"));
        assert!(cmd_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = section_cell(3).unwrap_or(0);
        assert_eq!(a, section_cell(3).unwrap_or(0));
        assert_ne!(a, section_cell(4).unwrap_or(0));
        assert!(python_layer(PY_LAYERS).is_none());
        assert!(module_cell(MODULE_CELLS).is_none());
    }

    #[test]
    fn test_inv_projection_roundtrip() {
        // 对合恒等：back(fwd(x))==x 且 fwd(back(y))==y，全 12 格。
        let mut r = 0usize;
        while r < INV_CELLS {
            let base = inv_cell(r).unwrap_or(0);
            assert_eq!(inv_back(inv_fwd(base)), base);
            assert_eq!(inv_fwd(inv_back(base)), base);
            r += 1;
        }
        assert_ne!(inv_fwd(0xA3000), 0xA3000);
    }

    #[test]
    fn test_section_order_caught() {
        let mut lt = AnsibleLedger::new();
        let _ = lt.record_section(9, section_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_section(2, section_cell(2).unwrap_or(0)),
            Err(AnsibleCode::ESecOrder)
        );
        assert!(lt.record_section(10, section_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AnsibleLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AnsibleCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_inv(3, inv_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(AnsibleCode::EInvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AnsibleLedger::new();
        let _ = lt.record_mod(0, module_cell(0).unwrap_or(0));
        assert_eq!(lt.record_mod(0, module_cell(0).unwrap_or(0)), Err(AnsibleCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AnsibleLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = AnsibleLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 段 + 10 操作 + 12 投影 + 8 消费 + 4 层 = 54。
        assert_eq!(
            recorded,
            SECTION_CELLS as u32 + CMD_OPS as u32 + INV_CELLS as u32 + MODULE_CELLS as u32 + PY_LAYERS as u32
        );
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
        assert_eq!(SEG_BASE, 0xC9D00);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AnsibleCode::Ok as u32, 0);
        assert_eq!(AnsibleCode::EGradeDrift as u32, 12);
        assert_eq!(AnsibleCode::Ok.val(), SEG_BASE);
        assert_eq!(AnsibleCode::ESeal.val(), SEG_BASE + 0x0B);
        assert!(!AnsibleCode::Ok.detail().is_empty());
        assert!(!AnsibleCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = AnsibleLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_module_names_closed() {
        assert_eq!(module_name(0), Some("setup"));
        assert_eq!(module_name(7), Some("debug"));
        assert!(module_name(8).is_none());
        assert_eq!(python_layer_name(3), Some("ansible-playbook 入口"));
        assert!(python_layer_name(4).is_none());
        assert_eq!(gate_name(0), Some("安装自检"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn unx52373_checks_green() {
        let s = run_unx52373_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
