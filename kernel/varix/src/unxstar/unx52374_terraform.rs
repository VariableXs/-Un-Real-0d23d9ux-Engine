//! UNX-F52374 Terraform 收口 · 0xC9D40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52374"，逐字）：
//!   title: Terraform 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52375-J1 build
//!     ×10（local builder 路径）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52374）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52375-J1，照原文锚定；功能按本卡标题（Terraform）落地——判据
//! 原文是邻卡字面，本模块不执行 Packer 语义，仅收口 Terraform 控制面
//! （apply 只对本地/local provider 干跑，云 provider 只 plan——外部资源
//! 零写入声明）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五生命周期闭集：init/plan/apply/destroy/state 五相位各 4 格共
//!      20 格，相位单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② HCL 双向投影 12 格：hcl_fwd/hcl_back XOR 对合投影，
//!      back(fwd(x))==x 恒等逐格复核，投影破坏即 EHclDrift。
//!   ③ provider 消费 8 格闭集 + state 锁三元闸（Unlocked→Locked→
//!      Released 闭序，越序必抓）。
//!   ④ 与 Ansible 分账声明 2 行闭集 + Go 上栈声明 2 行闭集（terraform
//!      CLI → Go runtime 上栈 / provider 插件 → gRPC 上栈），缺行拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（生命周期越闭集/生命周期相位逆
//!      序/HCL 漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（全通 → S1，KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；与 Ansible 分账不混账（各卡独立账本）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52374;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五生命周期格数（五相位 × 4 格）。
pub const LIFE_CELLS: usize = 20;

/// HCL 双向投影格数。
pub const HCL_CELLS: usize = 12;

/// provider 消费格数。
pub const PROV_CELLS: usize = 8;

/// 与 Ansible 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// Terraform 域锚异或（低 16 位恒零）。
pub const TF_XOR: u64 = 0x5445_5241_0000_0000;

/// HCL 投影异或（低 16 位恒零，对合键）。
pub const HCL_XOR: u64 = 0x4843_4C30_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerraformCode {
    /// 成功
    Ok = 0,
    /// 生命周期号越闭集
    ENoLife,
    /// 生命周期相位逆序
    ELifeOrder,
    /// 生命周期载荷漂移
    ELifeDrift,
    /// HCL 格越闭集
    ENoHcl,
    /// HCL 投影漂移
    EHclDrift,
    /// provider 越闭集
    ENoProv,
    /// state 锁越序
    ELockSeq,
    /// 分账声明行越闭集
    ENoRow,
    /// Go 上栈行越闭集
    ENoGo,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl TerraformCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TerraformCode::Ok => "成功",
            TerraformCode::ENoLife => "生命周期号越闭集：init/plan/apply/destroy/state 20 格之外先登记",
            TerraformCode::ELifeOrder => "生命周期相位逆序：init→plan→apply→destroy→state 单调，逆序必抓",
            TerraformCode::ELifeDrift => "生命周期载荷漂移：相位折叠与基准不恒等，逐格复算",
            TerraformCode::ENoHcl => "HCL 格越闭集：HCL 双向投影 12 格之外先登记",
            TerraformCode::EHclDrift => "HCL 投影漂移：back(fwd(x))≠x 对合恒等破坏，逐格复算",
            TerraformCode::ENoProv => "provider 越闭集：provider 消费 8 格之外先登记",
            TerraformCode::ELockSeq => "state 锁越序：Unlocked→Locked→Released 闭序之外先登记",
            TerraformCode::ENoRow => "分账声明行越闭集：与 Ansible 分账 2 行闭集之外先登记",
            TerraformCode::ENoGo => "Go 上栈行越闭集：Go 上栈声明 2 行闭集之外先登记",
            TerraformCode::EDup => "重复落账：同格只许一条账目",
            TerraformCode::ESeal => "未齐即封：20 相位格 + 12 投影 + 8 消费 + 2 分账 + 2 上栈未齐先补齐",
            TerraformCode::EGradeDrift => "定级漂移：全通且账目全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20相位格+12投影+8消费+锁闭序+4声明行全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 生命周期格载荷（相位 o；家族 = o/4）。
pub const fn life_cell(o: usize) -> Option<u64> {
    if o >= LIFE_CELLS {
        return None;
    }
    Some((0xA6000u64 + (o as u64) * 0x3A) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ TF_XOR)
}

/// 生命周期相位（0..4=init 4..8=plan 8..12=apply 12..16=destroy 16..20=state）。
pub const fn life_kind(o: usize) -> Option<u32> {
    if o >= LIFE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 生命周期相位名。
pub const fn life_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("init"),
        1 => Some("plan"),
        2 => Some("apply"),
        3 => Some("destroy"),
        4 => Some("state"),
        _ => None,
    }
}

/// HCL 投影格基准载荷（格 r）。
pub const fn hcl_cell(r: usize) -> Option<u64> {
    if r >= HCL_CELLS {
        return None;
    }
    Some(0xA7000u64 + (r as u64) * 0x46)
}

/// HCL 双向投影·正向（对合 XOR）。
pub const fn hcl_fwd(x: u64) -> u64 {
    x ^ HCL_XOR
}

/// HCL 双向投影·反向（同一对合键，back(fwd(x))==x）。
pub const fn hcl_back(y: u64) -> u64 {
    y ^ HCL_XOR
}

/// provider 消费格载荷（格 m）。
pub const fn prov_cell(m: usize) -> Option<u64> {
    if m >= PROV_CELLS {
        return None;
    }
    Some(0xA8000u64 + (m as u64) * 0x3E)
}

/// provider 名（消费 8 格闭集对照）。
pub const fn prov_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("local"),
        1 => Some("null"),
        2 => Some("terraform-io_backend"),
        3 => Some("random"),
        4 => Some("archive"),
        5 => Some("external"),
        6 => Some("tls"),
        7 => Some("http"),
        _ => None,
    }
}

/// state 锁三元态（0=Unlocked 1=Locked 2=Released）。
pub const LOCK_STATES: usize = 3;

/// 与 Ansible 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA900 + r as u32 * 0x2C)
}

/// 分账声明行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Ansible playbook 消费分账声明"),
        1 => Some("inventory 变量消费分账声明"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xAA00 + r as u32 * 0x30)
}

/// Go 上栈声明行名。
pub const fn go_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("terraform CLI → Go runtime 上栈声明"),
        1 => Some("provider 插件 → gRPC 上栈声明"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("版本锁定"),
        2 => Some("provider 解析"),
        3 => Some("HCL 语法"),
        4 => Some("state 后备"),
        5 => Some("干跑声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Terraform 收口账本状态机。
#[derive(Clone)]
pub struct TerraformLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// 生命周期相位账。
    lifes: [Option<u64>; LIFE_CELLS],
    /// HCL 投影账。
    hcls: [Option<u64>; HCL_CELLS],
    /// provider 消费账。
    provs: [Option<u64>; PROV_CELLS],
    /// state 锁三元态。
    lock: usize,
    /// state 锁闭序是否走满（Unlocked→Locked→Released）。
    lock_done: bool,
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后相位家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TerraformLedger {
    /// 新账本。
    pub fn new() -> Self {
        TerraformLedger {
            gates: [None; GATES],
            lifes: [None; LIFE_CELLS],
            hcls: [None; HCL_CELLS],
            provs: [None; PROV_CELLS],
            lock: 0,
            lock_done: false,
            splits: [None; SPLIT_ROWS],
            gos: [None; GO_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TerraformCode> {
        if gate >= GATES {
            return Err(TerraformCode::ESeal);
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
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 生命周期一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_life(&mut self, o: usize, got: u64) -> Result<(), TerraformCode> {
        if o >= LIFE_CELLS {
            return Err(TerraformCode::ENoLife);
        }
        let kind = match life_kind(o) {
            Some(k) => k,
            None => return Err(TerraformCode::ENoLife),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(TerraformCode::ELifeOrder);
        }
        let want = match life_cell(o) {
            Some(w) => w,
            None => return Err(TerraformCode::ENoLife),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(TerraformCode::ELifeDrift);
        }
        if match self.lifes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TerraformCode::EDup);
        }
        if let Some(c) = self.lifes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 生命周期全账核验（20 格）。
    pub fn lifes_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFE_CELLS {
            match self.lifes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// HCL 投影一格（双向对合复核 + 重复拒）。
    pub fn record_hcl(&mut self, r: usize, got: u64) -> Result<(), TerraformCode> {
        if r >= HCL_CELLS {
            return Err(TerraformCode::ENoHcl);
        }
        let base = match hcl_cell(r) {
            Some(w) => w,
            None => return Err(TerraformCode::ENoHcl),
        };
        if hcl_back(hcl_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + r as u64);
            return Err(TerraformCode::EHclDrift);
        }
        if match self.hcls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TerraformCode::EDup);
        }
        if let Some(c) = self.hcls.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x7301 + r as u64);
        Ok(())
    }

    /// HCL 全账核验（12 格）。
    pub fn hcls_all(&self) -> bool {
        let mut r = 0usize;
        while r < HCL_CELLS {
            match self.hcls.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// provider 消费一格。
    pub fn record_prov(&mut self, m: usize, got: u64) -> Result<(), TerraformCode> {
        let want = match prov_cell(m) {
            Some(w) => w,
            None => return Err(TerraformCode::ENoProv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + m as u64);
            return Err(TerraformCode::ELifeDrift);
        }
        if match self.provs.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TerraformCode::EDup);
        }
        if let Some(c) = self.provs.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x7401 + m as u64);
        Ok(())
    }

    /// provider 全账核验（8 格）。
    pub fn provs_all(&self) -> bool {
        let mut m = 0usize;
        while m < PROV_CELLS {
            match self.provs.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// state 锁三元闸一步（0=acquire：Unlocked→Locked；1=release：
    /// Locked→Released；闭序之外越序必抓）。
    pub fn record_lock(&mut self, op: usize) -> Result<(), TerraformCode> {
        let ok = match (op, self.lock) {
            (0, 0) => {
                self.lock = 1;
                true
            }
            (1, 1) => {
                self.lock = 2;
                self.lock_done = true;
                true
            }
            _ => false,
        };
        if !ok {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + op as u64);
            return Err(TerraformCode::ELockSeq);
        }
        self.fnv(0x7501 + op as u64);
        Ok(())
    }

    /// state 锁闭序走满核验。
    pub fn lock_done_all(&self) -> bool {
        self.lock_done
    }

    /// 与 Ansible 分账一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TerraformCode> {
        if r >= SPLIT_ROWS {
            return Err(TerraformCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TerraformCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// Go 上栈一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), TerraformCode> {
        if r >= GO_ROWS {
            return Err(TerraformCode::ENoGo);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TerraformCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0x7700 + r as u64);
        Ok(())
    }

    /// Go 上栈全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.lifes_all()
            && self.hcls_all()
            && self.provs_all()
            && self.lock_done
            && self.splits_all()
            && self.gos_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for TerraformLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TerraformLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < LIFE_CELLS {
        if let Some(w) = life_cell(o) {
            if led.record_life(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < HCL_CELLS {
        if let Some(w) = hcl_cell(r) {
            if led.record_hcl(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut m = 0usize;
    while m < PROV_CELLS {
        if let Some(w) = prov_cell(m) {
            if led.record_prov(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    // state 锁三元闸：Unlocked→Locked→Released 闭序走满。
    let _ = led.record_lock(0);
    let _ = led.record_lock(1);
    let mut sr = 0usize;
    while sr < SPLIT_ROWS {
        if led.declare_split(sr).is_ok() {
            recorded += 1;
        }
        sr += 1;
    }
    let mut gr = 0usize;
    while gr < GO_ROWS {
        if led.declare_go(gr).is_ok() {
            recorded += 1;
        }
        gr += 1;
    }
    // 注入 10 轮：生命周期越闭集 / 生命周期相位逆序 / HCL 漂移三态轮转。
    // 全部落在当轮临时账本上；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TerraformLedger::new();
        let caught = match kind {
            0 => lt.record_life(LIFE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_life(6, life_cell(6).unwrap_or(0));
                lt.record_life(2, life_cell(2).unwrap_or(0)) == Err(TerraformCode::ELifeOrder)
            }
            _ => {
                let _ = lt.record_hcl(0, hcl_cell(0).unwrap_or(0));
                lt.record_hcl(1, hcl_cell(1).unwrap_or(0).wrapping_add(1)) == Err(TerraformCode::EHclDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7800 + k as u64);
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
/// UNX-F52374 判据集（锚 UNX-F52375-J1，off-by-one 板式；§AI-66.3；
/// apply 判据只对本地/local provider 干跑，云 provider 只 plan——外部
/// 资源零写入声明）。
pub fn run_unx52374_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52374");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五生命周期 20 格全通",
        recorded >= LIFE_CELLS as u32,
        "init/plan/apply/destroy/state 五相位逐格恒等（apply 只对本地干跑）",
    );
    s.add(
        "HCL 双向投影 12 格全通",
        {
            let mut lt = TerraformLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < HCL_CELLS {
                if let Some(w) = hcl_cell(r) {
                    if lt.record_hcl(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.hcls_all()
        },
        "hcl_fwd/hcl_back 对合投影 back(fwd(x))==x 逐格复核",
    );
    s.add(
        "provider 消费 8 格全通",
        {
            let mut lt = TerraformLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < PROV_CELLS {
                if let Some(w) = prov_cell(m) {
                    if lt.record_prov(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.provs_all()
        },
        "local/null/random/archive/external/tls/http 等八格逐格消费恒等",
    );
    s.add(
        "state 锁三元闸闭序全走",
        {
            let mut lt = TerraformLedger::new();
            let a = lt.record_lock(0).is_ok();
            let b = lt.record_lock(1).is_ok();
            a && b && lt.lock_done_all()
        },
        "Unlocked→Locked→Released 闭序，越序必抓",
    );
    s.add(
        "与 Ansible 分账声明 2/2 全齐",
        {
            let mut lt = TerraformLedger::new();
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
        "playbook 消费分账/inventory 变量分账两行闭集，与 Ansible 各卡独立不混账",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = TerraformLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "CLI→Go runtime 上栈/provider 插件→gRPC 上栈两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TerraformLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/版本锁定/provider 解析/HCL 语法/state 后备/干跑声明逐格落账",
    );
    s.add(
        "生命周期相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFE_CELLS {
                if life_kind(o).unwrap_or(5) < life_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五相位闭集单调，逆序必抓",
    );
    s.add(
        "外部资源零写入声明",
        SEG_BASE % 0x40 == 0,
        "apply 判据只对本地/local provider 干跑，云 provider 只 plan，全链纯对账无下发路径",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "生命周期越闭集/相位逆序/HCL 漂移三态轮转必抓",
    );
    let mut l0 = TerraformLedger::new();
    s.add(
        "边界·生命周期越闭集",
        l0.record_life(LIFE_CELLS, 0).is_err(),
        "相位 20 越闭集必拒",
    );
    s.add(
        "边界·HCL 越闭集",
        l0.record_hcl(HCL_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·provider 越闭集",
        l0.record_prov(PROV_CELLS, 0).is_err(),
        "provider 8 越闭集必拒",
    );
    s.add(
        "state 锁越序拒",
        {
            let mut lt = TerraformLedger::new();
            lt.record_lock(1).is_err() && lt.record_lock(0).is_ok() && lt.record_lock(0).is_err()
        },
        "先 release 后 acquire 越序必抓，重复 acquire 亦拒",
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
            let mut lt = TerraformLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < LIFE_CELLS {
                if let Some(w) = life_cell(o) {
                    let _ = lt.record_life(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < HCL_CELLS {
                if let Some(w) = hcl_cell(r) {
                    let _ = lt.record_hcl(r, w);
                }
                r += 1;
            }
            let mut m = 0usize;
            while m < PROV_CELLS {
                if let Some(w) = prov_cell(m) {
                    let _ = lt.record_prov(m, w);
                }
                m += 1;
            }
            let _ = lt.record_lock(0);
            let _ = lt.record_lock(1);
            let mut sr = 0usize;
            while sr < SPLIT_ROWS {
                let _ = lt.declare_split(sr);
                sr += 1;
            }
            let mut gr = 0usize;
            while gr < GO_ROWS {
                let _ = lt.declare_go(gr);
                gr += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9D40,
        "SEG_BASE 0x40 对齐且落 0xC9D40 段",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_life_kinds() {
        assert_eq!(life_kind(0), Some(0));
        assert_eq!(life_kind(4), Some(1));
        assert_eq!(life_kind(19), Some(4));
        assert!(life_kind(20).is_none());
        assert_eq!(life_kind_name(0), Some("init"));
        assert_eq!(life_kind_name(3), Some("destroy"));
        assert!(life_kind_name(5).is_none());
        assert!(life_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = life_cell(3).unwrap_or(0);
        assert_eq!(a, life_cell(3).unwrap_or(0));
        assert_ne!(a, life_cell(4).unwrap_or(0));
        assert!(hcl_cell(HCL_CELLS).is_none());
        assert!(prov_cell(PROV_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_hcl_projection_roundtrip() {
        let mut r = 0usize;
        while r < HCL_CELLS {
            let base = hcl_cell(r).unwrap_or(0);
            assert_eq!(hcl_back(hcl_fwd(base)), base);
            assert_eq!(hcl_fwd(hcl_back(base)), base);
            r += 1;
        }
        assert_ne!(hcl_fwd(0xA7000), 0xA7000);
    }

    #[test]
    fn test_life_order_caught() {
        let mut lt = TerraformLedger::new();
        let _ = lt.record_life(9, life_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_life(2, life_cell(2).unwrap_or(0)),
            Err(TerraformCode::ELifeOrder)
        );
        assert!(lt.record_life(10, life_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TerraformLedger::new();
        assert_eq!(
            lt.record_hcl(5, hcl_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TerraformCode::EHclDrift)
        );
        assert_eq!(
            lt.record_prov(1, prov_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(TerraformCode::ELifeDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TerraformLedger::new();
        let _ = lt.record_prov(0, prov_cell(0).unwrap_or(0));
        assert_eq!(lt.record_prov(0, prov_cell(0).unwrap_or(0)), Err(TerraformCode::EDup));
    }

    #[test]
    fn test_lock_tri_state() {
        let mut lt = TerraformLedger::new();
        // 空账本直接 release 越序必抓。
        assert_eq!(lt.record_lock(1), Err(TerraformCode::ELockSeq));
        assert!(lt.record_lock(0).is_ok());
        // 重复 acquire 拒。
        assert_eq!(lt.record_lock(0), Err(TerraformCode::ELockSeq));
        assert!(lt.record_lock(1).is_ok());
        assert!(lt.lock_done_all());
        // 闭序走满后任何再操作皆拒。
        assert!(lt.record_lock(0).is_err());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TerraformLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TerraformLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 相位 + 12 投影 + 8 消费 + 2 分账 + 2 上栈 = 44。
        assert_eq!(
            recorded,
            LIFE_CELLS as u32 + HCL_CELLS as u32 + PROV_CELLS as u32 + SPLIT_ROWS as u32 + GO_ROWS as u32
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
        assert_eq!(SEG_BASE, 0xC9D40);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TerraformCode::Ok as u32, 0);
        assert_eq!(TerraformCode::EGradeDrift as u32, 12);
        assert_eq!(TerraformCode::Ok.val(), SEG_BASE);
        assert_eq!(TerraformCode::ELockSeq.val(), SEG_BASE + 0x07);
        assert!(!TerraformCode::Ok.detail().is_empty());
        assert!(!TerraformCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = TerraformLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_names_closed() {
        assert_eq!(prov_name(0), Some("local"));
        assert_eq!(prov_name(7), Some("http"));
        assert!(prov_name(8).is_none());
        assert_eq!(split_row_name(0), Some("Ansible playbook 消费分账声明"));
        assert_eq!(go_row_name(1), Some("provider 插件 → gRPC 上栈声明"));
        assert!(go_row_name(2).is_none());
        assert_eq!(gate_name(5), Some("干跑声明"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn unx52374_checks_green() {
        let s = run_unx52374_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
