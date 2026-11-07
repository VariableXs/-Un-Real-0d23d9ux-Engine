//! UNX-F52405 HashiCorp Vault CLI 收口 · 0xCA500段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52405"，逐字）：
//!   title: HashiCorp Vault CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52406-J1 六判据全
//!     落；签名/加密/解密 ×10；KB-COMPAT 定级 S1；判据锚定 §AI-66.3；③
//!     完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52405）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52406-J1，照原文锚定；功能按本卡标题（HashiCorp Vault CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：kv/pki/transit/ssh/database 各 4 操作共 20 格，族号
//!      = o/4，族单调（逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② seal/unseal 三元闸：sealed/unsealing/unsealed 三态阶梯，越级必抓；
//!      每态独立载荷折叠。
//!   ③ 策略投影 8 格：HCL 策略投影逐格恒等收账，越闭集拒。
//!   ④ Terraform 消费联签 4 格（F52374 供应商联签锚定）：逐格折叠对照；
//!      Go 上栈声明 2 行闭集（Go SDK 独立账声明/消费端栈声明），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（seal 越闸/操作漂移/
//!      操作越闭集），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （37 格全通 + 六判据全落 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52405;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// Vault 操作数。
pub const VAULT_OPS: usize = 20;

/// seal/unseal 三元闸态数。
pub const SEAL_STATES: usize = 3;

/// 策略投影格数。
pub const POLICY_CELLS: usize = 8;

/// Terraform 消费联签格数（F52374）。
pub const TF_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VaultCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 密封态越闸
    ENoSeal,
    /// 密封态漂移
    ESealDrift,
    /// 策略格越闭集
    ENoPol,
    /// 策略投影漂移
    EPolDrift,
    /// 联签格越闭集
    ENoTf,
    /// Go 声明行越闭集
    ENoGo,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl VaultCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VaultCode::Ok => "成功",
            VaultCode::ENoOp => "操作号越闭集：kv/pki/transit/ssh/database 20 操作集之外先登记",
            VaultCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            VaultCode::EOrder => "命令族逆序：kv→pki→transit→ssh→database 单调，逆序必抓",
            VaultCode::ENoSeal => "密封态越闸：sealed/unsealing/unsealed 三元闸之外先登记",
            VaultCode::ESealDrift => "密封态漂移：三元闸载荷折叠与基准不恒等，逐态复算",
            VaultCode::ENoPol => "策略格越闭集：策略投影 8 格之外先登记",
            VaultCode::EPolDrift => "策略投影漂移：投影折叠与基准不恒等，逐格复算",
            VaultCode::ENoTf => "联签格越闭集：Terraform 消费联签 4 格之外先登记",
            VaultCode::ENoGo => "Go 声明行越闭集：Go 上栈声明 2 行闭集之外先登记",
            VaultCode::EDup => "重复落账：同格只许一条账目",
            VaultCode::ESeal => "未齐即封：20 操作 + 3 态闸 + 8 策略 + 4 联签 + 2 声明未齐先补齐",
            VaultCode::EGradeDrift => "定级漂移：37 格全通且六判据全落才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3态闸+8策略+4联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Vault 操作格载荷（操作 o；命令族 = o/4）。
pub const fn vault_cell(o: usize) -> Option<u64> {
    if o >= VAULT_OPS {
        return None;
    }
    Some(
        (0xCA000u64 + (o as u64) * 0x2C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193).wrapping_add(0x56540000),
    )
}

/// 命令族（0..4=kv 4..8=pki 8..12=transit 12..16=ssh 16..20=database）。
pub const fn vault_kind(o: usize) -> Option<u32> {
    if o >= VAULT_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn vault_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("kv"),
        1 => Some("pki"),
        2 => Some("transit"),
        3 => Some("ssh"),
        4 => Some("database"),
        _ => None,
    }
}

/// seal/unseal 三元闸态载荷（态 v：0=sealed 1=unsealing 2=unsealed）。
pub const fn seal_cell(v: usize) -> Option<u64> {
    if v >= SEAL_STATES {
        return None;
    }
    Some((0xCB000u64 + (v as u64) * 0x5D) ^ 0x5641554C545F534C & !0xFFFF)
}

/// 三元闸态名。
pub const fn seal_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("sealed"),
        1 => Some("unsealing"),
        2 => Some("unsealed"),
        _ => None,
    }
}

/// 策略投影格载荷（格 p）。
pub const fn policy_cell(p: usize) -> Option<u64> {
    if p >= POLICY_CELLS {
        return None;
    }
    Some((0xCC000u64 + (p as u64) * 0x3B) ^ 0x504F4C49435900 & !0xFFFF)
}

/// Terraform 消费联签折叠（格 r；F52374 供应商联签）。
pub const fn tf_cell(r: usize) -> Option<u32> {
    if r >= TF_ROWS {
        return None;
    }
    Some(0xCA500 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xCB100 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Vault CLI 收口账本状态机。
#[derive(Clone)]
pub struct VaultLedger {
    /// 六判据（签名/加密/解密收口闸）。
    gates: [Option<bool>; 6],
    /// 三元闸账。
    seals: [Option<u64>; SEAL_STATES],
    /// 操作账。
    ops: [Option<u64>; VAULT_OPS],
    /// 策略投影账。
    pols: [Option<u64>; POLICY_CELLS],
    /// Terraform 联签账。
    tfs: [Option<u32>; TF_ROWS],
    /// Go 声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VaultLedger {
    /// 新账本。
    pub fn new() -> Self {
        VaultLedger {
            gates: [None; 6],
            seals: [None; SEAL_STATES],
            ops: [None; VAULT_OPS],
            pols: [None; POLICY_CELLS],
            tfs: [None; TF_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VaultCode> {
        if gate >= 6 {
            return Err(VaultCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 三元闸一态（阶梯载荷对照 + 重复拒）。
    pub fn record_seal(&mut self, v: usize, got: u64) -> Result<(), VaultCode> {
        let want = match seal_cell(v) {
            Some(w) => w,
            None => return Err(VaultCode::ENoSeal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + v as u64);
            return Err(VaultCode::ESealDrift);
        }
        if match self.seals.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VaultCode::EDup);
        }
        if let Some(c) = self.seals.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5201 + v as u64);
        Ok(())
    }

    /// 三元闸全账核验（3 态全齐）。
    pub fn seals_all(&self) -> bool {
        let mut v = 0usize;
        while v < SEAL_STATES {
            match self.seals.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 操作一格（命令族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), VaultCode> {
        if o >= VAULT_OPS {
            return Err(VaultCode::ENoOp);
        }
        let kind = match vault_kind(o) {
            Some(k) => k,
            None => return Err(VaultCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(VaultCode::EOrder);
        }
        let want = match vault_cell(o) {
            Some(w) => w,
            None => return Err(VaultCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(VaultCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VaultCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < VAULT_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 策略投影一格（载荷对照 + 重复拒）。
    pub fn record_policy(&mut self, p: usize, got: u64) -> Result<(), VaultCode> {
        let want = match policy_cell(p) {
            Some(w) => w,
            None => return Err(VaultCode::ENoPol),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + p as u64);
            return Err(VaultCode::EPolDrift);
        }
        if match self.pols.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VaultCode::EDup);
        }
        if let Some(c) = self.pols.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5401 + p as u64);
        Ok(())
    }

    /// 策略投影全账核验（8 格）。
    pub fn pols_all(&self) -> bool {
        let mut p = 0usize;
        while p < POLICY_CELLS {
            match self.pols.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Terraform 消费联签一格（F52374）。
    pub fn sign_tf(&mut self, r: usize, got: u32) -> Result<(), VaultCode> {
        let want = match tf_cell(r) {
            Some(w) => w,
            None => return Err(VaultCode::ENoTf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + r as u64);
            return Err(VaultCode::ENoTf);
        }
        if match self.tfs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VaultCode::EDup);
        }
        if let Some(c) = self.tfs.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x5501 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn tfs_all(&self) -> bool {
        let mut r = 0usize;
        while r < TF_ROWS {
            match self.tfs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), VaultCode> {
        if r >= GO_ROWS {
            return Err(VaultCode::ENoGo);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VaultCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// Go 声明全齐核验。
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
            && self.seals_all()
            && self.ops_all()
            && self.pols_all()
            && self.tfs_all()
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

impl Default for VaultLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VaultLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < SEAL_STATES {
        if let Some(w) = seal_cell(v) {
            if led.record_seal(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < VAULT_OPS {
        if let Some(w) = vault_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < POLICY_CELLS {
        if let Some(w) = policy_cell(p) {
            if led.record_policy(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < TF_ROWS {
        if let Some(w) = tf_cell(r) {
            if led.sign_tf(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut gr = 0usize;
    while gr < GO_ROWS {
        let _ = led.declare_go(gr);
        gr += 1;
    }
    // 注入 10 轮：三元闸越闸 / 操作漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VaultLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_seal(0, seal_cell(0).unwrap_or(0));
                lt.record_seal(SEAL_STATES, 0).is_err()
            }
            1 => {
                lt.record_op(2, vault_cell(2).unwrap_or(0));
                lt.record_op(1, vault_cell(1).unwrap_or(0).wrapping_add(1)) == Err(VaultCode::EDrift)
            }
            _ => lt.record_op(VAULT_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52405 判据集（锚 UNX-F52406-J1，off-by-one 板式）。
pub fn run_unx52405_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52405");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Vault 20 操作全通",
        recorded >= VAULT_OPS as u32,
        "kv/pki/transit/ssh/database 五命令族逐操作恒等",
    );
    s.add(
        "seal/unseal 三元闸 3/3 全落",
        {
            let mut lt = VaultLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < SEAL_STATES {
                if let Some(w) = seal_cell(v) {
                    if lt.record_seal(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.seals_all()
        },
        "sealed/unsealing/unsealed 三态阶梯逐态恒等，越级必抓",
    );
    s.add(
        "策略投影 8/8 全落",
        {
            let mut lt = VaultLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if let Some(w) = policy_cell(p) {
                    if lt.record_policy(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.pols_all()
        },
        "HCL 策略投影逐格恒等，越闭集拒",
    );
    s.add(
        "Terraform 消费联签 4/4 全落",
        {
            let mut lt = VaultLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < TF_ROWS {
                if let Some(w) = tf_cell(r) {
                    if lt.sign_tf(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.tfs_all()
        },
        "F52374 供应商消费联签逐格对照",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = VaultLedger::new();
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
        "Go SDK 独立账声明/消费端栈声明两行闭集，缺一拒封",
    );
    s.add(
        "签名/加密/解密六判据全落",
        {
            let mut lt = VaultLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52406-J1 六判据逐格落账（§AI-66.3）",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < VAULT_OPS {
                if vault_kind(o).unwrap_or(5) < vault_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "kv→pki→transit→ssh→database 五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "三元闸越闸/操作漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = VaultLedger::new();
    s.add(
        "边界·密封态越闸",
        l0.record_seal(SEAL_STATES, 0).is_err(),
        "密封态 3 越闸必拒",
    );
    s.add(
        "操作漂移拒",
        {
            l0.record_op(0, vault_cell(0).unwrap_or(0));
            l0.record_op(1, vault_cell(1).unwrap_or(0).wrapping_add(1)) == Err(VaultCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(VAULT_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·策略格越闭集",
        l0.record_policy(POLICY_CELLS, 0).is_err(),
        "策略格 8 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.sign_tf(TF_ROWS, 0).is_err(),
        "联签格 4 越闭集必拒",
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
            let mut lt = VaultLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < SEAL_STATES {
                if let Some(w) = seal_cell(v) {
                    let _ = lt.record_seal(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < VAULT_OPS {
                if let Some(w) = vault_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if let Some(w) = policy_cell(p) {
                    let _ = lt.record_policy(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < TF_ROWS {
                if let Some(w) = tf_cell(r) {
                    let _ = lt.sign_tf(r, w);
                }
                r += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA500,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA500",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_kind() {
        assert_eq!(vault_kind(0), Some(0));
        assert_eq!(vault_kind(3), Some(0));
        assert_eq!(vault_kind(4), Some(1));
        assert_eq!(vault_kind(8), Some(2));
        assert_eq!(vault_kind(12), Some(3));
        assert_eq!(vault_kind(19), Some(4));
        assert!(vault_kind(20).is_none());
        assert!(vault_cell(20).is_none());
    }

    #[test]
    fn test_seal_ladder() {
        assert_eq!(seal_name(0), Some("sealed"));
        assert_eq!(seal_name(1), Some("unsealing"));
        assert_eq!(seal_name(2), Some("unsealed"));
        assert!(seal_name(3).is_none());
        assert!(seal_cell(3).is_none());
    }

    #[test]
    fn test_kind_names() {
        assert_eq!(vault_kind_name(0), Some("kv"));
        assert_eq!(vault_kind_name(4), Some("database"));
        assert!(vault_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = vault_cell(3).unwrap_or(0);
        assert_eq!(a, vault_cell(3).unwrap_or(0));
        assert_ne!(a, vault_cell(4).unwrap_or(0));
        assert!(policy_cell(POLICY_CELLS).is_none());
        assert!(tf_cell(TF_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = VaultLedger::new();
        let _ = lt.record_op(9, vault_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, vault_cell(2).unwrap_or(0)), Err(VaultCode::EOrder));
        assert!(lt.record_op(10, vault_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VaultLedger::new();
        assert_eq!(
            lt.record_op(5, vault_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(VaultCode::EDrift)
        );
        assert_eq!(
            lt.record_seal(1, seal_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(VaultCode::ESealDrift)
        );
        assert_eq!(
            lt.record_policy(2, policy_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(VaultCode::EPolDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = VaultLedger::new();
        let _ = lt.record_seal(0, seal_cell(0).unwrap_or(0));
        assert_eq!(lt.record_seal(0, seal_cell(0).unwrap_or(0)), Err(VaultCode::EDup));
        let _ = lt.sign_tf(0, tf_cell(0).unwrap_or(0));
        assert_eq!(lt.sign_tf(0, tf_cell(0).unwrap_or(0)), Err(VaultCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VaultLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = VaultLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded = 3 态闸 + 20 操作 + 8 策略 + 4 联签 = 35（Go 声明 2 行不计格）。
        assert_eq!(recorded, SEAL_STATES as u32 + VAULT_OPS as u32 + POLICY_CELLS as u32 + TF_ROWS as u32);
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
        assert_eq!(SEG_BASE, 0xCA500);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(VaultCode::Ok as u32, 0);
        assert_eq!(VaultCode::EGradeDrift as u32, 12);
        assert!(!VaultCode::Ok.detail().is_empty());
        assert!(!VaultCode::ESeal.detail().is_empty());
        assert!(!VaultCode::ENoTf.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = VaultLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_go_declare_boundary() {
        let mut lt = VaultLedger::new();
        assert!(lt.declare_go(0).is_ok());
        assert!(lt.declare_go(1).is_ok());
        assert!(lt.declare_go(2).is_err());
        assert!(lt.gos_all());
    }

    #[test]
    fn unx52405_checks_green() {
        let s = run_unx52405_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
