//! UNX-F0165 · UEFI 变量读写策略与配额 · 诊断段 0x26F0
//!
//! 判据原文（施工书）：UEFI 运行期变量服务在内核侧的读写策略门：
//! ① 读一律放行但记账；② 写按「属性 × 配额 × 命名策略」三重闸；
//! ③ 删除需属性内 NonVolatile 位且记账；④ 配额按命名空间划分
//! （global / setup / vendor-{GUID 前缀}），单空间超配额拒写；
//! ⑤ AppendWrite 仅对具备 APPEND_BIT 的变量合法；
//! ⑥ 原子性：一次写要么全提交要么全不落账（账目守恒）。
//! ⑦ BootService 与 RuntimeService 两阶段权限不同：Runtime 拒一切写。
//!
//! 诊断码闭集（0x26F0 段）：
//! - E_NS      0x26F1 命名空间不识别
//! - E_QUOTA   0x26F2 命名空间配额超限
//! - E_ATTR    0x26F3 属性闸拒绝（写保护 / Runtime 阶段 / Append 违例）
//! - E_NAME    0x26F4 命名策略拒绝（空名 / 超长 / 非法字符）
//! - E_NOSUCH  0x26F5 删除或读取不存在的变量
//! - E_FROZEN  0x26F6 阶段已冻结为 Runtime，写通道关闭
//! - E_BAL     0x26F7 账目守恒破坏（提交与账不符）

use alloc::vec;
use crate::alloc::{string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x26F0;
pub const E_NS: u16 = SEG_BASE + 1; // 命名空间不识别
pub const E_QUOTA: u16 = SEG_BASE + 2; // 配额超限
pub const E_ATTR: u16 = SEG_BASE + 3; // 属性闸拒绝
pub const E_NAME: u16 = SEG_BASE + 4; // 命名策略拒绝
pub const E_NOSUCH: u16 = SEG_BASE + 5; // 不存在
pub const E_FROZEN: u16 = SEG_BASE + 6; // Runtime 冻结
pub const E_BAL: u16 = SEG_BASE + 7; // 账目守恒破坏

/// EFI 变量属性位（与 UEFI 规范对齐的子集）。
pub const ATTR_NV: u64 = 0x0000_0001; // NonVolatile
pub const ATTR_BS: u64 = 0x0000_0002; // BootServiceAccess
pub const ATTR_RT: u64 = 0x0000_0004; // RuntimeAccess
pub const ATTR_APPEND: u64 = 0x8000_0000_0000_0000; // AppendWrite（高位位）

/// 服务阶段：引导期可写，运行期只读。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Boot,
    Runtime,
}

/// 命名空间识别：按变量名前缀归桶，配额各自独立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Namespace {
    Global, // "EFI_" 前缀之外的全局面（含 Security 等系统名）
    Setup,  // "Setup" 前缀
    Vendor, // "Vnd" 前缀（模拟 vendor-{GUID} 空间）
}

/// 三空间配额字节数（闭集，不可配置——配额即策略）。
pub const QUOTA_GLOBAL: usize = 4096;
pub const QUOTA_SETUP: usize = 2048;
pub const QUOTA_VENDOR: usize = 1024;

/// 变量条目。
#[derive(Clone, Debug)]
pub struct VarEntry {
    pub name: String,
    pub attrs: u64,
    pub data: Vec<u8>,
}

/// 策略账本：读记账 + 写闸 + 删闸 + 配额 + 守恒。
pub struct VarPolicyBook {
    pub phase: Phase,
    pub vars: Vec<VarEntry>,
    pub read_count: u64,
    pub write_count: u64,
    pub delete_count: u64,
    pub committed_bytes: u64, // Σ 所有在册变量 len（守恒基准）
}

/// 命名合法性：非空、≤ 96 字节、仅 ASCII 可见字符。
pub fn name_ok(name: &str) -> Result<(), u16> {
    if name.is_empty() || name.len() > 96 {
        return Err(E_NAME);
    }
    for b in name.bytes() {
        if !(0x21..=0x7E).contains(&b) {
            return Err(E_NAME);
        }
    }
    Ok(())
}

/// 命名空间识别（前缀归桶，未识别即拒）。
pub fn namespace_of(name: &str) -> Result<Namespace, u16> {
    if name.starts_with("Setup") {
        Ok(Namespace::Setup)
    } else if name.starts_with("Vnd") {
        Ok(Namespace::Vendor)
    } else if name.starts_with("EFI_") || name.starts_with("Security") || name.starts_with("PK/")
        || name.starts_with("KEK/") || name.starts_with("db") || name.starts_with("dbx")
    {
        Ok(Namespace::Global)
    } else {
        Err(E_NS)
    }
}

/// 空间配额表。
pub fn quota_of(ns: Namespace) -> usize {
    match ns {
        Namespace::Global => QUOTA_GLOBAL,
        Namespace::Setup => QUOTA_SETUP,
        Namespace::Vendor => QUOTA_VENDOR,
    }
}

impl VarPolicyBook {
    pub fn new(phase: Phase) -> Result<Self, u16> {
        Ok(VarPolicyBook {
            phase,
            vars: Vec::new(),
            read_count: 0,
            write_count: 0,
            delete_count: 0,
            committed_bytes: 0,
        })
    }

    /// 阶段冻结：Boot → Runtime 单向。
    pub fn freeze_to_runtime(&mut self) -> Result<(), u16> {
        if self.phase == Phase::Runtime {
            return Err(E_FROZEN);
        }
        self.phase = Phase::Runtime;
        Ok(())
    }

    /// 空间当前占用量。
    fn ns_usage(&self, ns: Namespace) -> usize {
        self.vars
            .iter()
            .filter(|v| namespace_of(&v.name) == Ok(ns))
            .map(|v| v.data.len())
            .sum()
    }

    /// 读：一律放行 + 记账；不存在报 E_NOSUCH。
    pub fn read(&mut self, name: &str) -> Result<(u64, Vec<u8>), u16> {
        name_ok(name)?;
        self.read_count += 1;
        let v = self
            .vars
            .iter()
            .find(|v| v.name == name)
            .ok_or(E_NOSUCH)?;
        Ok((v.attrs, v.data.clone()))
    }

    /// 写（覆盖语义）：三重闸 —— 阶段闸 / 属性闸 / 配额闸。
    /// 属性闸细则：
    /// - 必须带 ATTR_BS（内核可见前提）；
    /// - 已存在变量不得改属性（属性一经落册即固化）；
    /// - APPEND_BIT 只允许在「新变量带位」或「旧变量已带位」时使用追加；
    ///   覆盖写不带 APPEND 位则正常替换。
    pub fn write(&mut self, name: &str, attrs: u64, data: &[u8]) -> Result<(), u16> {
        name_ok(name)?;
        // 阶段闸
        if self.phase == Phase::Runtime {
            return Err(E_FROZEN);
        }
        // 属性闸 1：必须 BootService 可见
        if attrs & ATTR_BS == 0 {
            return Err(E_ATTR);
        }
        let existing = self.vars.iter().position(|v| v.name == name);
        if let Some(idx) = existing {
            // 属性闸 2：属性固化，不得改写
            if self.vars[idx].attrs != attrs {
                return Err(E_ATTR);
            }
        }
        // 属性闸 3：Runtime 可见变量必须同时 BS|RT
        if attrs & ATTR_RT != 0 && attrs & ATTR_BS == 0 {
            return Err(E_ATTR);
        }
        // 配额闸：空间占用差值不得超配额
        let ns = namespace_of(name)?;
        let delta = match existing {
            Some(idx) => data.len() as i64 - self.vars[idx].data.len() as i64,
            None => data.len() as i64,
        };
        let usage = self.ns_usage(ns) as i64;
        if usage + delta > quota_of(ns) as i64 {
            return Err(E_QUOTA);
        }
        // 提交（全或无：先算好再落账）
        match existing {
            Some(idx) => {
                self.committed_bytes = self.committed_bytes + delta.max(0) as u64
                    - (-delta).max(0) as u64;
                self.vars[idx].data = data.to_vec();
            }
            None => {
                self.vars.push(VarEntry {
                    name: name.to_string(),
                    attrs,
                    data: data.to_vec(),
                });
                self.committed_bytes += data.len() as u64;
            }
        }
        self.write_count += 1;
        Ok(())
    }

    /// 追加写：仅对带 APPEND_BIT 的变量合法，数据尾接。
    pub fn append(&mut self, name: &str, tail: &[u8]) -> Result<(), u16> {
        name_ok(name)?;
        if self.phase == Phase::Runtime {
            return Err(E_FROZEN);
        }
        let idx = self
            .vars
            .iter()
            .position(|v| v.name == name)
            .ok_or(E_NOSUCH)?;
        if self.vars[idx].attrs & ATTR_APPEND == 0 {
            return Err(E_ATTR);
        }
        let ns = namespace_of(name)?;
        let new_len = self.vars[idx].data.len() + tail.len();
        if self.ns_usage(ns) - self.vars[idx].data.len() + new_len > quota_of(ns) {
            return Err(E_QUOTA);
        }
        self.vars[idx].data.extend_from_slice(tail);
        self.committed_bytes += tail.len() as u64;
        self.write_count += 1;
        Ok(())
    }

    /// 删：仅 NonVolatile 位在册的变量可由内核侧删（NV 位代表我方有固化权）；
    /// 不存在报 E_NOSUCH。
    pub fn delete(&mut self, name: &str) -> Result<(), u16> {
        name_ok(name)?;
        if self.phase == Phase::Runtime {
            return Err(E_FROZEN);
        }
        let idx = self
            .vars
            .iter()
            .position(|v| v.name == name)
            .ok_or(E_NOSUCH)?;
        if self.vars[idx].attrs & ATTR_NV == 0 {
            return Err(E_ATTR);
        }
        self.committed_bytes -= self.vars[idx].data.len() as u64;
        self.vars.remove(idx);
        self.delete_count += 1;
        Ok(())
    }

    /// 账目守恒：committed_bytes == Σ在册 len；且读写删三账非负。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u64 = self.vars.iter().map(|v| v.data.len() as u64).sum();
        if sum != self.committed_bytes {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 诊断文本。
    pub fn detail(code: u16) -> &'static str {
        match code {
            E_NS => "命名空间不识别：前缀不在 global/setup/vendor 三桶",
            E_QUOTA => "命名空间配额超限：先删旧变量或缩数据",
            E_ATTR => "属性闸拒绝：缺 BS 位/属性固化/Runtime 位组合非法",
            E_NAME => "命名策略拒绝：空名/超 96 字节/含非可见字符",
            E_NOSUCH => "变量不存在：先枚举确认名字",
            E_FROZEN => "阶段已冻结 Runtime：写通道关闭，仅读放行",
            E_BAL => "账目守恒破坏：提交总量与在册总量不符",
            _ => "未知诊断码",
        }
    }
}

// ── 确定性演练：合法 6 例 + 注入拦截 8 例 ────────────────────────
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：写读往返 + 读记账。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_VarA", ATTR_BS | ATTR_NV, &[1, 2, 3])?;
    let (a, d) = b.read("EFI_VarA")?;
    if a != ATTR_BS | ATTR_NV || d != vec![1, 2, 3] {
        return Err(E_BAL);
    }
    if b.read_count != 1 || b.write_count != 1 {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L2：三空间配额各自独立（写满 vendor 不影响 global）。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    let big = vec![0u8; QUOTA_VENDOR];
    b.write("VndX", ATTR_BS, &big)?;
    b.write("EFI_G1", ATTR_BS, &[7u8; 100])?;
    if b.ns_usage(Namespace::Vendor) != QUOTA_VENDOR {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L3：追加写合法路径（带 APPEND 位变量）。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("SetupLog", ATTR_BS | ATTR_APPEND, &[9])?;
    b.append("SetupLog", &[8, 7])?;
    let (_, d) = b.read("SetupLog")?;
    if d != vec![9, 8, 7] {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L4：覆盖写回缩配额占用（delta 为负）。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_Big", ATTR_BS, &[0u8; 500])?;
    b.write("EFI_Big", ATTR_BS, &[1u8; 100])?;
    if b.committed_bytes != 100 {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L5：删除 NV 变量 + 删账。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_Tmp", ATTR_BS | ATTR_NV, &[5; 10])?;
    b.delete("EFI_Tmp")?;
    if b.delete_count != 1 || !b.vars.is_empty() {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L6：Boot → Runtime 冻结单向 + Runtime 拒写放读。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_Keep", ATTR_BS | ATTR_RT, &[3])?;
    b.freeze_to_runtime()?;
    if b.freeze_to_runtime() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if b.write("EFI_New", ATTR_BS, &[1]) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    b.read("EFI_Keep")?; // Runtime 读放行
    if b.read_count != 1 {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // I1：未识别命名空间。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    if b.write("Weird_Var", ATTR_BS, &[1]) != Err(E_NS) {
        return Err(E_NS);
    }
    hits += 1;

    // I2：命名策略三连拒（空 / 超 96 / 非可见字符）。
    if name_ok("") != Err(E_NAME) || name_ok(&"x".repeat(97)) != Err(E_NAME)
        || name_ok("a b\tc") != Err(E_NAME)
    {
        return Err(E_NAME);
    }
    hits += 1;

    // I3：配额超限（setup 2048 上限，写 2049 拒）。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    if b.write("SetupHuge", ATTR_BS, &[0u8; QUOTA_SETUP + 1]) != Err(E_QUOTA) {
        return Err(E_QUOTA);
    }
    hits += 1;

    // I4：缺 BS 位的写拒。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    if b.write("EFI_NoBS", ATTR_NV, &[1]) != Err(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I5：属性固化——同变量改属性拒。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_Fix", ATTR_BS, &[1])?;
    if b.write("EFI_Fix", ATTR_BS | ATTR_NV, &[2]) != Err(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I6：对无 APPEND 位变量追加拒。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_Plain", ATTR_BS, &[1])?;
    if b.append("EFI_Plain", &[2]) != Err(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I7：删不存在的 / 删非 NV 位变量拒。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    if b.delete("EFI_Ghost") != Err(E_NOSUCH) {
        return Err(E_NOSUCH);
    }
    b.write("EFI_Vol", ATTR_BS, &[1])?; // 无 NV 位
    if b.delete("EFI_Vol") != Err(E_ATTR) {
        return Err(E_ATTR);
    }
    hits += 1;

    // I8：账目守恒注入——人为破 committed_bytes 必须被 audit 抓住。
    let mut b = VarPolicyBook::new(Phase::Boot)?;
    b.write("EFI_A", ATTR_BS, &[1, 2])?;
    b.committed_bytes = 99;
    if b.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    if hits != 14 {
        return Err(E_BAL);
    }
    Ok(())
}
