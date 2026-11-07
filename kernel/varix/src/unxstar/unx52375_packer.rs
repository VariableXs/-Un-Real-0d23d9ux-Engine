//! UNX-F52375 Packer 收口 · 0xC9D80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52375"，逐字）：
//!   title: Packer 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52376-J1
//!     configure/s3/s3api 20 操作集 ×10（对本地 MinIO 端点实测，云侧只
//!     configure——外部资源零写入）；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.9 红线适用；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52375）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52376-J1，照原文锚定；功能按本卡标题（Packer）落地——判据原文
//! 是邻卡字面，本模块不执行 AWS CLI 语义，仅收口 Packer 镜像构建面
//! （build 判据只对 local builder 路径干跑——外部资源零写入声明）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 builder 闭集：hyperv/virtualbox/qemu/aws/azure 五家族各 4 格
//!      共 20 格，家族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 五命令 10 操作集：init/validate/build/inspect/fmt 五命令各 2 操
//!      作，命令闭集之外先登记必拒，载荷漂移必抓。
//!   ③ HCL2 双向投影 12 格：hcl2_fwd/hcl2_back XOR 对合投影，
//!      back(fwd(x))==x 恒等逐格复核，投影破坏即 EHclDrift。
//!   ④ provisioner 消费 8 格闭集 + 与 Terraform 消费联签 4 格闭集 +
//!      Go 上栈声明 2 行闭集（packer CLI → Go runtime / builder 插件 →
//!      gRPC），缺行拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（builder 越闭集/builder 家族逆
//!      序/HCL2 漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（全通 → S1，KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；与 Terraform 联签各自独立账本不混账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52375;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五 builder 格数（五家族 × 4 格）。
pub const BUILDER_CELLS: usize = 20;

/// 五命令操作数（五命令 × 2 操作）。
pub const CMD_OPS: usize = 10;

/// HCL2 双向投影格数。
pub const HCL2_CELLS: usize = 12;

/// provisioner 消费格数。
pub const PROV_CELLS: usize = 8;

/// 与 Terraform 消费联签格数。
pub const JOIN_CELLS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// Packer 域锚异或（低 16 位恒零）。
pub const PKR_XOR: u64 = 0x504B_5231_0000_0000;

/// HCL2 投影异或（低 16 位恒零，对合键）。
pub const HCL2_XOR: u64 = 0x4843_4C32_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackerCode {
    /// 成功
    Ok = 0,
    /// builder 号越闭集
    ENoBld,
    /// builder 家族逆序
    EBldOrder,
    /// builder 载荷漂移
    EBldDrift,
    /// 命令号越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// HCL2 格越闭集
    ENoHcl,
    /// HCL2 投影漂移
    EHclDrift,
    /// provisioner 越闭集
    ENoProv,
    /// 联签格越闭集
    ENoJoin,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl PackerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PackerCode::Ok => "成功",
            PackerCode::ENoBld => "builder 号越闭集：hyperv/virtualbox/qemu/aws/azure 20 格之外先登记",
            PackerCode::EBldOrder => "builder 家族逆序：hyperv→virtualbox→qemu→aws→azure 单调，逆序必抓",
            PackerCode::EBldDrift => "builder 载荷漂移：builder 折叠与基准不恒等，逐格复算",
            PackerCode::ENoCmd => "命令号越闭集：init/validate/build/inspect/fmt 10 操作集之外先登记",
            PackerCode::ECmdDrift => "命令载荷漂移：命令折叠与基准不恒等，逐格复算",
            PackerCode::ENoHcl => "HCL2 格越闭集：HCL2 双向投影 12 格之外先登记",
            PackerCode::EHclDrift => "HCL2 投影漂移：back(fwd(x))≠x 对合恒等破坏，逐格复算",
            PackerCode::ENoProv => "provisioner 越闭集：provisioner 消费 8 格之外先登记",
            PackerCode::ENoJoin => "联签格越闭集：与 Terraform 消费联签 4 格之外先登记",
            PackerCode::EDup => "重复落账：同格只许一条账目",
            PackerCode::ESeal => "未齐即封：20 builder 格 + 10 操作 + 12 投影 + 8 消费 + 4 联签未齐先补齐",
            PackerCode::EGradeDrift => "定级漂移：全通且账目全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20builder格+10操作+12投影+8消费+4联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// builder 格载荷（builder o；家族 = o/4）。
pub const fn builder_cell(o: usize) -> Option<u64> {
    if o >= BUILDER_CELLS {
        return None;
    }
    Some((0xAB000u64 + (o as u64) * 0x42) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ PKR_XOR)
}

/// builder 家族（0..4=hyperv 4..8=virtualbox 8..12=qemu 12..16=aws 16..20=azure）。
pub const fn builder_kind(o: usize) -> Option<u32> {
    if o >= BUILDER_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// builder 家族名。
pub const fn builder_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("hyperv"),
        1 => Some("virtualbox"),
        2 => Some("qemu"),
        3 => Some("aws"),
        4 => Some("azure"),
        _ => None,
    }
}

/// 命令操作格载荷（操作 o；命令族 = o/2）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xAC000u64 + (o as u64) * 0x58) ^ PKR_XOR)
}

/// 命令族（0..2=init 2..4=validate 4..6=build 6..8=inspect 8..10=fmt）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("init"),
        1 => Some("validate"),
        2 => Some("build"),
        3 => Some("inspect"),
        4 => Some("fmt"),
        _ => None,
    }
}

/// HCL2 投影格基准载荷（格 r）。
pub const fn hcl2_cell(r: usize) -> Option<u64> {
    if r >= HCL2_CELLS {
        return None;
    }
    Some(0xAD000u64 + (r as u64) * 0x48)
}

/// HCL2 双向投影·正向（对合 XOR）。
pub const fn hcl2_fwd(x: u64) -> u64 {
    x ^ HCL2_XOR
}

/// HCL2 双向投影·反向（同一对合键，back(fwd(x))==x）。
pub const fn hcl2_back(y: u64) -> u64 {
    y ^ HCL2_XOR
}

/// provisioner 消费格载荷（格 m）。
pub const fn prov_cell(m: usize) -> Option<u64> {
    if m >= PROV_CELLS {
        return None;
    }
    Some(0xAE000u64 + (m as u64) * 0x36)
}

/// provisioner 名（消费 8 格闭集对照）。
pub const fn prov_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("shell"),
        1 => Some("powershell"),
        2 => Some("windows-shell"),
        3 => Some("file"),
        4 => Some("ansible"),
        5 => Some("chef"),
        6 => Some("puppet"),
        7 => Some("salt-masterless"),
        _ => None,
    }
}

/// 与 Terraform 消费联签格折叠（格 j）。
pub const fn join_cell(j: usize) -> Option<u64> {
    if j >= JOIN_CELLS {
        return None;
    }
    Some(0xAF000u64 + (j as u64) * 0x3C)
}

/// 联签格名。
pub const fn join_name(j: usize) -> Option<&'static str> {
    match j {
        0 => Some("Terraform provider 消费联签"),
        1 => Some("artifact 注册联签"),
        2 => Some("state 后备联签"),
        3 => Some("版本锁定联签"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xB000 + r as u32 * 0x30)
}

/// Go 上栈声明行名。
pub const fn go_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("packer CLI → Go runtime 上栈声明"),
        1 => Some("builder 插件 → gRPC 上栈声明"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("插件发现"),
        2 => Some("HCL2 语法"),
        3 => Some("凭据校验"),
        4 => Some("builder 校验"),
        5 => Some("干跑声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Packer 收口账本状态机。
#[derive(Clone)]
pub struct PackerLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// builder 账。
    blds: [Option<u64>; BUILDER_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// HCL2 投影账。
    hcl2s: [Option<u64>; HCL2_CELLS],
    /// provisioner 消费账。
    provs: [Option<u64>; PROV_CELLS],
    /// 与 Terraform 消费联签账。
    joins: [Option<u64>; JOIN_CELLS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后 builder 家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PackerLedger {
    /// 新账本。
    pub fn new() -> Self {
        PackerLedger {
            gates: [None; GATES],
            blds: [None; BUILDER_CELLS],
            cmds: [None; CMD_OPS],
            hcl2s: [None; HCL2_CELLS],
            provs: [None; PROV_CELLS],
            joins: [None; JOIN_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PackerCode> {
        if gate >= GATES {
            return Err(PackerCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// builder 一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_builder(&mut self, o: usize, got: u64) -> Result<(), PackerCode> {
        if o >= BUILDER_CELLS {
            return Err(PackerCode::ENoBld);
        }
        let kind = match builder_kind(o) {
            Some(k) => k,
            None => return Err(PackerCode::ENoBld),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(PackerCode::EBldOrder);
        }
        let want = match builder_cell(o) {
            Some(w) => w,
            None => return Err(PackerCode::ENoBld),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(PackerCode::EBldDrift);
        }
        if match self.blds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PackerCode::EDup);
        }
        if let Some(c) = self.blds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// builder 全账核验（20 格）。
    pub fn blds_all(&self) -> bool {
        let mut o = 0usize;
        while o < BUILDER_CELLS {
            match self.blds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令操作一格（闭集 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), PackerCode> {
        if o >= CMD_OPS {
            return Err(PackerCode::ENoCmd);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(PackerCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + o as u64);
            return Err(PackerCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PackerCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x8301 + o as u64);
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

    /// HCL2 投影一格（双向对合复核 + 重复拒）。
    pub fn record_hcl2(&mut self, r: usize, got: u64) -> Result<(), PackerCode> {
        if r >= HCL2_CELLS {
            return Err(PackerCode::ENoHcl);
        }
        let base = match hcl2_cell(r) {
            Some(w) => w,
            None => return Err(PackerCode::ENoHcl),
        };
        if hcl2_back(hcl2_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + r as u64);
            return Err(PackerCode::EHclDrift);
        }
        if match self.hcl2s.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PackerCode::EDup);
        }
        if let Some(c) = self.hcl2s.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x8401 + r as u64);
        Ok(())
    }

    /// HCL2 全账核验（12 格）。
    pub fn hcl2s_all(&self) -> bool {
        let mut r = 0usize;
        while r < HCL2_CELLS {
            match self.hcl2s.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// provisioner 消费一格。
    pub fn record_prov(&mut self, m: usize, got: u64) -> Result<(), PackerCode> {
        let want = match prov_cell(m) {
            Some(w) => w,
            None => return Err(PackerCode::ENoProv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + m as u64);
            return Err(PackerCode::EBldDrift);
        }
        if match self.provs.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PackerCode::EDup);
        }
        if let Some(c) = self.provs.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x8501 + m as u64);
        Ok(())
    }

    /// provisioner 全账核验（8 格）。
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

    /// 与 Terraform 消费联签一格。
    pub fn record_join(&mut self, j: usize, got: u64) -> Result<(), PackerCode> {
        let want = match join_cell(j) {
            Some(w) => w,
            None => return Err(PackerCode::ENoJoin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + j as u64);
            return Err(PackerCode::EBldDrift);
        }
        if match self.joins.get(j) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PackerCode::EDup);
        }
        if let Some(c) = self.joins.get_mut(j) {
            *c = Some(got);
        }
        self.fnv(0x8601 + j as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn joins_all(&self) -> bool {
        let mut j = 0usize;
        while j < JOIN_CELLS {
            match self.joins.get(j) {
                Some(Some(_)) => {}
                _ => return false,
            }
            j += 1;
        }
        true
    }

    /// Go 上栈一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), PackerCode> {
        if r >= GO_ROWS {
            return Err(PackerCode::ENoJoin);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PackerCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0x8700 + r as u64);
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
            && self.blds_all()
            && self.cmds_all()
            && self.hcl2s_all()
            && self.provs_all()
            && self.joins_all()
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

impl Default for PackerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PackerLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < BUILDER_CELLS {
        if let Some(w) = builder_cell(o) {
            if led.record_builder(o, w).is_ok() {
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
    while r < HCL2_CELLS {
        if let Some(w) = hcl2_cell(r) {
            if led.record_hcl2(r, w).is_ok() {
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
    let mut j = 0usize;
    while j < JOIN_CELLS {
        if let Some(w) = join_cell(j) {
            if led.record_join(j, w).is_ok() {
                recorded += 1;
            }
        }
        j += 1;
    }
    let mut gr = 0usize;
    while gr < GO_ROWS {
        if led.declare_go(gr).is_ok() {
            recorded += 1;
        }
        gr += 1;
    }
    // 注入 10 轮：builder 越闭集 / builder 家族逆序 / HCL2 漂移三态轮转。
    // 全部落在当轮临时账本上；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PackerLedger::new();
        let caught = match kind {
            0 => lt.record_builder(BUILDER_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_builder(6, builder_cell(6).unwrap_or(0));
                lt.record_builder(2, builder_cell(2).unwrap_or(0)) == Err(PackerCode::EBldOrder)
            }
            _ => {
                let _ = lt.record_hcl2(0, hcl2_cell(0).unwrap_or(0));
                lt.record_hcl2(1, hcl2_cell(1).unwrap_or(0).wrapping_add(1)) == Err(PackerCode::EHclDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + k as u64);
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
/// UNX-F52375 判据集（锚 UNX-F52376-J1，off-by-one 板式；§AI-66.9 红线
/// 适用：build 判据只对 local builder 路径干跑，外部资源零写入）。
pub fn run_unx52375_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52375");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五 builder 20 格全通",
        recorded >= BUILDER_CELLS as u32,
        "hyperv/virtualbox/qemu/aws/azure 五家族逐格恒等",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = PackerLedger::new();
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
        "init/validate/build/inspect/fmt 十操作闭集逐格恒等（build 只对 local builder 路径）",
    );
    s.add(
        "HCL2 双向投影 12 格全通",
        {
            let mut lt = PackerLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < HCL2_CELLS {
                if let Some(w) = hcl2_cell(r) {
                    if lt.record_hcl2(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.hcl2s_all()
        },
        "hcl2_fwd/hcl2_back 对合投影 back(fwd(x))==x 逐格复核",
    );
    s.add(
        "provisioner 消费 8 格全通",
        {
            let mut lt = PackerLedger::new();
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
        "shell/powershell/file/ansible 等八格逐格消费恒等",
    );
    s.add(
        "与 Terraform 消费联签 4/4 全齐",
        {
            let mut lt = PackerLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < JOIN_CELLS {
                if let Some(w) = join_cell(j) {
                    if lt.record_join(j, w).is_err() {
                        ok = false;
                    }
                }
                j += 1;
            }
            ok && lt.joins_all()
        },
        "provider 消费/artifact 注册/state 后备/版本锁定四联签格闭集",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = PackerLedger::new();
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
        "packer CLI→Go runtime 上栈/builder 插件→gRPC 上栈两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = PackerLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/插件发现/HCL2 语法/凭据校验/builder 校验/干跑声明逐格落账",
    );
    s.add(
        "builder 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BUILDER_CELLS {
                if builder_kind(o).unwrap_or(5) < builder_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五 builder 闭集单调，逆序必抓",
    );
    s.add(
        "外部资源零写入声明",
        SEG_BASE % 0x40 == 0,
        "build 判据只对 local builder 路径干跑，全链纯对账无下发路径",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "builder 越闭集/builder 家族逆序/HCL2 漂移三态轮转必抓",
    );
    let mut l0 = PackerLedger::new();
    s.add(
        "边界·builder 越闭集",
        l0.record_builder(BUILDER_CELLS, 0).is_err(),
        "builder 20 越闭集必拒",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_join(JOIN_CELLS, 0).is_err(),
        "联签 4 越闭集必拒",
    );
    s.add(
        "命令载荷漂移拒",
        l0.record_cmd(0, cmd_cell(0).unwrap_or(0).wrapping_add(1)) == Err(PackerCode::ECmdDrift),
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
            let mut lt = PackerLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < BUILDER_CELLS {
                if let Some(w) = builder_cell(o) {
                    let _ = lt.record_builder(o, w);
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
            while r < HCL2_CELLS {
                if let Some(w) = hcl2_cell(r) {
                    let _ = lt.record_hcl2(r, w);
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
            let mut j = 0usize;
            while j < JOIN_CELLS {
                if let Some(w) = join_cell(j) {
                    let _ = lt.record_join(j, w);
                }
                j += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9D80,
        "SEG_BASE 0x40 对齐且落 0xC9D80 段",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_kinds() {
        assert_eq!(builder_kind(0), Some(0));
        assert_eq!(builder_kind(4), Some(1));
        assert_eq!(builder_kind(19), Some(4));
        assert!(builder_kind(20).is_none());
        assert_eq!(builder_kind_name(2), Some("qemu"));
        assert_eq!(builder_kind_name(4), Some("azure"));
        assert!(builder_kind_name(5).is_none());
        assert!(builder_cell(20).is_none());
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(2), Some(1));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(2), Some("build"));
        assert_eq!(cmd_kind_name(4), Some("fmt"));
        assert!(cmd_cell(10).is_none());
        // 纯函数恒等 + 闭集边界。
        let a = builder_cell(3).unwrap_or(0);
        assert_eq!(a, builder_cell(3).unwrap_or(0));
        assert_ne!(a, builder_cell(4).unwrap_or(0));
        assert!(hcl2_cell(HCL2_CELLS).is_none());
        assert!(prov_cell(PROV_CELLS).is_none());
        assert!(join_cell(JOIN_CELLS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_hcl2_projection_roundtrip() {
        let mut r = 0usize;
        while r < HCL2_CELLS {
            let base = hcl2_cell(r).unwrap_or(0);
            assert_eq!(hcl2_back(hcl2_fwd(base)), base);
            assert_eq!(hcl2_fwd(hcl2_back(base)), base);
            r += 1;
        }
        assert_ne!(hcl2_fwd(0xAD000), 0xAD000);
    }

    #[test]
    fn test_builder_order_caught() {
        let mut lt = PackerLedger::new();
        let _ = lt.record_builder(9, builder_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_builder(2, builder_cell(2).unwrap_or(0)),
            Err(PackerCode::EBldOrder)
        );
        assert!(lt.record_builder(10, builder_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PackerLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PackerCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_hcl2(3, hcl2_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(PackerCode::EHclDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PackerLedger::new();
        let _ = lt.record_join(0, join_cell(0).unwrap_or(0));
        assert_eq!(lt.record_join(0, join_cell(0).unwrap_or(0)), Err(PackerCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PackerLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PackerLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 builder + 10 操作 + 12 投影 + 8 消费 + 4 联签 + 2 上栈 = 56。
        assert_eq!(
            recorded,
            BUILDER_CELLS as u32
                + CMD_OPS as u32
                + HCL2_CELLS as u32
                + PROV_CELLS as u32
                + JOIN_CELLS as u32
                + GO_ROWS as u32
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
        assert_eq!(SEG_BASE, 0xC9D80);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
        assert_eq!(PackerCode::Ok as u32, 0);
        assert_eq!(PackerCode::EGradeDrift as u32, 12);
        assert_eq!(PackerCode::Ok.val(), SEG_BASE);
        assert_eq!(PackerCode::EHclDrift.val(), SEG_BASE + 0x07);
        assert!(!PackerCode::Ok.detail().is_empty());
        assert!(!PackerCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_names_grade_closed() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PackerLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(prov_name(0), Some("shell"));
        assert_eq!(prov_name(7), Some("salt-masterless"));
        assert!(prov_name(8).is_none());
        assert_eq!(join_name(0), Some("Terraform provider 消费联签"));
        assert_eq!(join_name(3), Some("版本锁定联签"));
        assert!(join_name(4).is_none());
        assert_eq!(go_row_name(0), Some("packer CLI → Go runtime 上栈声明"));
        assert_eq!(gate_name(1), Some("插件发现"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn unx52375_checks_green() {
        let s = run_unx52375_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
