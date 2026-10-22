//! unx186_msrwhite —— UNX-F0186 MSR 访问白名单（防裸写硬件红线面）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0186 · MSR 访问白名单 · 目标 260 行 · 已深化
//! 判据原文：UNX-F0186-J1 白名单外 MSR 读写被拦截并记缺陷账，红线（防裸写硬件）
//! 组件：MSR 白名单闭集（内核必需 MSR 逐条登记：读白名单/写白名单两栏）
//!       + 访问闸（rdmsr/wrmsr 语义分级：白名单内放行、白名单外拦截+落账）
//!       + 缺陷账（append-only：msr 索引/方向/来源三位一列）
//!       + 白名单指纹（FNV-1a 32 位冻结，白名单漂移可检）
//!       + 注入核（白名单外读/外写/只读 MSR 写/封账后改表四类注入 10/10 必抓）。
//! 职责定位：MSR 裸写防线的唯一权威闸——任何模块 MSR 访问必须过闸；
//!   双栏闭集（读/写分离）保证只读 MSR 不可被写；缺陷账使每次拦截可追。
//! 性能逐项分解：查闸 O(log n)（白名单 ≤64 条有序二分）；落账 O(1) 均摊；
//!   指纹 O(n)。全整数确定性（LCG 注入，零墙钟）。
//! 诊断码段：0xE180（0xE181~0xE188 闭集；空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const MW_VERSION: &str = "unx186_msrwhite-v1";

/// 诊断段基址（0xE180 段 · 本模块独占）。
pub const MW_SEG_BASE: u16 = 0xE180;

/// XcCode 闭集（0xE181~0xE188）。
pub const XC_MSR_BLACK: u32 = 0xE181;
pub const XC_WRITE_RDONLY: u32 = 0xE182;
pub const XC_TABLE_DRIFT: u32 = 0xE183;
pub const XC_SEALED_WRITE: u32 = 0xE184;
pub const XC_MIN_ENTRIES: u32 = 0xE185;
pub const XC_INJECT_MISSED: u32 = 0xE186;
pub const XC_LEDGER_BROKEN: u32 = 0xE187;
pub const XC_RANGE_BAD: u32 = 0xE188;

/// 中文诊断文本（闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_MSR_BLACK => ("MSR 索引不在白名单（裸写防线拦截）", "按白名单登记流程申请，禁止绕闸"),
        XC_WRITE_RDONLY => ("对只读 MSR 发起写（写栏未放行）", "核对白名单写栏位与硬件手册"),
        XC_TABLE_DRIFT => ("白名单指纹与冻结值不一致（表漂移）", "重算指纹并复核白名单增删"),
        XC_SEALED_WRITE => ("封账后尝试改白名单（冻结被破坏）", "封账后只能开新版账"),
        XC_MIN_ENTRIES => ("白名单少于最小条目数", "确认内核必需 MSR 全登记再测"),
        XC_INJECT_MISSED => ("注入故障未被检出（守卫失守）", "修复注入核比对逻辑后重跑"),
        XC_LEDGER_BROKEN => ("缺陷账 append-only 性被破坏（序号跳号）", "检查落账路径是否绕过账务封装"),
        XC_RANGE_BAD => ("MSR 索引超 32 位界或落入保留段", "按 Intel SDM MSR 地址空间规约"),
        _ => ("未知诊断码", "查 0xE180 段闭集登记表"),
    }
}

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

/// FNV-1a 32 位（白名单指纹）。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 白名单条目（读/写双栏位图：bit0=可读 bit1=可写）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsrEntry {
    /// MSR 索引（ECX 值）。
    pub index: u32,
    /// 权限位图（0b01=只读 0b11=读写）。
    pub perm: u8,
}

/// 访问方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsrOp {
    Read,
    Write,
}

impl MsrOp {
    pub const fn num(self) -> u8 {
        match self {
            MsrOp::Read => 0,
            MsrOp::Write => 1,
        }
    }
}

/// 缺陷账一条（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct ViolRow {
    /// 被拦 MSR 索引。
    pub index: u32,
    /// 方向（0=读 1=写）。
    pub op: u8,
    /// 违规类别码。
    pub code: u32,
}

/// MSR 访问闸错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsrError {
    /// 白名单外访问。
    Blacklisted(u32, MsrOp),
    /// 只读 MSR 写。
    WriteRdonly(u32),
    /// 封账后改表。
    SealedWrite,
    /// 表内条目非法（perm 两位闭集外）。
    PermBad(u32),
    /// 账序跳号。
    LedgerBroken(u32),
}

/// MSR 白名单闸。
pub struct MsrGate {
    /// 有序白名单（按 index 升序，二分闸）。
    table: Vec<MsrEntry>,
    rows: Vec<ViolRow>,
    next_seq: u32,
    frozen: u32,
    sealed: bool,
}

impl MsrGate {
    pub fn new() -> MsrGate {
        MsrGate {
            table: Vec::new(),
            rows: Vec::new(),
            next_seq: 1,
            frozen: 0,
            sealed: false,
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.next_seq as usize == self.rows.len() + 1
    }

    pub fn table_len(&self) -> usize {
        self.table.len()
    }

    pub fn viol_count(&self) -> usize {
        self.rows.len()
    }

    /// 白名单登记（有序插入 + perm 闭集校验）。
    pub fn register(&mut self, index: u32, perm: u8) -> Result<(), MsrError> {
        if self.sealed {
            return Err(MsrError::SealedWrite);
        }
        if perm & !0b11 != 0 {
            return Err(MsrError::PermBad(index));
        }
        let pos = self.lower_bound(index);
        if pos < self.table.len() && self.table[pos].index == index {
            // 已登记则升权（不降权，防误锁内核必需写路径）
            if perm > self.table[pos].perm {
                self.table[pos].perm = perm;
            }
            return Ok(());
        }
        self.table.insert(
            pos,
            MsrEntry {
                index,
                perm,
            },
        );
        Ok(())
    }

    /// 有序下界（二分）。
    fn lower_bound(&self, index: u32) -> usize {
        let mut lo = 0usize;
        let mut hi = self.table.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.table[mid].index < index {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// 访问闸（唯一入口）：白名单内且栏位放行 → Ok；否则拦截落账。
    pub fn access(&mut self, index: u32, op: MsrOp) -> Result<(), MsrError> {
        // 修正（真缺陷·恒假闸）：原写 `if index > 0xFFFF_FFFF { … XC_RANGE_BAD }`。
        // `index` 的类型就是 `u32`，其上界恰是 0xFFFF_FFFF，该条件**永假**——
        // 这道「范围闸」从未拦下任何输入，反而让人误以为越界 MSR 号有独立
        // 归因路径。x86 的 MSR 号本就是 32 位地址空间，越界只可能来自调用方
        // 的窄化转换（在类型系统里已挡掉），故此处**只保留唯一权威**：
        // 白名单表成员资格（`lower_bound` 二分 + 精确相等）。仍然落账拦截，
        // 归因码统一为 `XC_MSR_BLACK`，不制造永不出现的错误码。
        let pos = self.lower_bound(index);
        let found = pos < self.table.len() && self.table[pos].index == index;
        if !found {
            self.ban(index, op, XC_MSR_BLACK as u32);
            return Err(MsrError::Blacklisted(index, op));
        }
        let perm = self.table[pos].perm;
        match op {
            MsrOp::Read => {
                if perm & 0b01 == 0 {
                    self.ban(index, op, XC_MSR_BLACK as u32);
                    return Err(MsrError::Blacklisted(index, op));
                }
                Ok(())
            }
            MsrOp::Write => {
                if perm & 0b10 == 0 {
                    self.ban(index, op, XC_WRITE_RDONLY as u32);
                    return Err(MsrError::WriteRdonly(index));
                }
                Ok(())
            }
        }
    }

    fn ban(&mut self, index: u32, op: MsrOp, code: u32) {
        let seq = self.next_seq;
        self.rows.push(ViolRow {
            index,
            op: op.num(),
            code,
        });
        self.next_seq = seq + 1;
    }

    pub fn row_at(&self, i: usize) -> Option<ViolRow> {
        self.rows.get(i).copied()
    }

    /// 白名单指纹（有序表确定性）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for e in &self.table {
            buf.extend_from_slice(&e.index.to_le_bytes());
            buf.push(e.perm);
        }
        fnv1a32(&buf)
    }

    /// 封账（指纹冻结；封后改表拒）。
    pub fn seal(&mut self) -> u32 {
        self.frozen = self.fingerprint();
        self.sealed = true;
        self.frozen
    }

    /// 冻结指纹核对。
    pub fn verify_frozen(&self) -> bool {
        self.sealed && self.fingerprint() == self.frozen
    }
}

/// 内核必需 MSR 最小白名单（J1 最小台账面：≥12 条）。
/// 来源：Intel SDM 常用闭集（确定性登记，零硬件访问）。
pub fn bootstrap_kernel_set(g: &mut MsrGate) -> Result<(), MsrError> {
    // (index, perm)：0b11=读写 0b01=只读
    const SET: [(u32, u8); 12] = [
        (0x0000_0010, 0b01), // IA32_TIME_STAMP_COUNTER（读）
        (0x0000_001B, 0b01), // IA32_APIC_BASE（读；重定位走显式流程）
        (0x0000_0174, 0b11), // IA32_SYSENTER_CS
        (0x0000_0175, 0b11), // IA32_SYSENTER_ESP
        (0x0000_0176, 0b11), // IA32_SYSENTER_EIP
        (0x0000_0177, 0b01), // IA32_SYSENTER_MSP（占位读）
        (0x0000_017A, 0b11), // IA32_MCG_STATUS
        (0x0000_01D9, 0b11), // IA32_DEBUGCTL
        (0x0000_0277, 0b11), // IA32_PAT
        (0x0000_0298, 0b01), // IA32_MTRR_DEF_TYPE（读）
        (0xC000_0080, 0b11), // IA32_EFER
        (0xC000_0102, 0b11), // IA32_KERNEL_GS_BASE
    ];
    for (i, p) in SET {
        g.register(i, p)?;
    }
    Ok(())
}

/// 注入核：四类违规 + 守卫 10/10 必抓。
pub fn inject_probe() -> Result<bool, u32> {
    let mut caught = 0u32;
    let mut g = MsrGate::new();
    bootstrap_kernel_set(&mut g).map_err(|_| XC_INJECT_MISSED as u32)?;
    // 1) 白名单外读
    if g.access(0x0000_0034, MsrOp::Read).is_err() {
        caught += 1;
    }
    // 2) 白名单外写
    if g.access(0x0000_0C80, MsrOp::Write).is_err() {
        caught += 1;
    }
    // 3) 只读 MSR 写（IA32_TIME_STAMP_COUNTER 只读）
    if g.access(0x0000_0010, MsrOp::Write).is_err() {
        caught += 1;
    }
    // 4) 保留段写（MSR 地址空间 0x4000_0000+ 的 AMD 段未登记）
    if g.access(0xC001_0000, MsrOp::Write).is_err() {
        caught += 1;
    }
    // 5) 账四拦全落账
    if g.viol_count() == 4 {
        caught += 1;
    }
    // 6) 封账后改表拒
    let _ = g.seal();
    if g.register(0x0000_003A, 0b11).is_err() {
        caught += 1;
    }
    // 7) 冻结指纹一致
    if g.verify_frozen() {
        caught += 1;
    }
    // 8) 白名单内读放行（守卫不误伤）
    let mut g2 = MsrGate::new();
    bootstrap_kernel_set(&mut g2).map_err(|_| XC_INJECT_MISSED as u32)?;
    if g2.access(0x0000_001B, MsrOp::Read).is_ok() {
        caught += 1;
    }
    // 9) 白名单内写放行（EFER 读写）
    if g2.access(0xC000_0080, MsrOp::Write).is_ok() {
        caught += 1;
    }
    // 10) 最小台账 ≥12
    if g2.table_len() >= 12 {
        caught += 1;
    }
    if caught != 10 {
        return Err(XC_INJECT_MISSED as u32);
    }
    Ok(true)
}

/// 域自检（`run_unx186_checks`）：J1 判据逐条可失败断言。
pub fn run_unx186_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx186");

    // C186-01 白名单外读拦截（J1 核心）
    {
        let mut g = MsrGate::new();
        bootstrap_kernel_set(&mut g).ok();
        let r = g.access(0x0000_00FE, MsrOp::Read);
        let ok = matches!(r, Err(MsrError::Blacklisted(0xFE, MsrOp::Read))) && g.viol_count() == 1;
        s.add("C186-01-外读拦截落账", ok, if ok { "" } else { "外读漏放" });
    }

    // C186-02 白名单外写拦截
    {
        let mut g = MsrGate::new();
        bootstrap_kernel_set(&mut g).ok();
        let r = g.access(0x0000_019C, MsrOp::Write);
        let ok = matches!(r, Err(MsrError::Blacklisted(0x19C, MsrOp::Write)));
        s.add("C186-02-外写拦截落账", ok, if ok { "" } else { "外写漏放" });
    }

    // C186-03 只读 MSR 写拒（读/写双栏分离）
    {
        let mut g = MsrGate::new();
        bootstrap_kernel_set(&mut g).ok();
        let r = g.access(0x0000_0298, MsrOp::Write);
        let ok = matches!(r, Err(MsrError::WriteRdonly(0x298)))
            && g.row_at(0).map(|x| x.code == XC_WRITE_RDONLY as u32).unwrap_or(false);
        s.add("C186-03-只读写拒", ok, if ok { "" } else { "双栏失守" });
    }

    // C186-04 白名单内放行 + 账零污染（守卫）
    {
        let mut g = MsrGate::new();
        bootstrap_kernel_set(&mut g).ok();
        let r1 = g.access(0x0000_0174, MsrOp::Write);
        let r2 = g.access(0xC000_0102, MsrOp::Read);
        let ok = r1.is_ok() && r2.is_ok() && g.viol_count() == 0;
        s.add("C186-04-白名单内放行", ok, if ok { "" } else { "闸误伤合法访问" });
    }

    // C186-05 指纹漂移检出（表改 → 指纹变）
    {
        let mut g = MsrGate::new();
        bootstrap_kernel_set(&mut g).ok();
        let f1 = g.fingerprint();
        let _ = g.register(0x0000_003A, 0b11);
        let f2 = g.fingerprint();
        let ok = f1 != f2;
        s.add("C186-05-表漂移指纹可检", ok, if ok { "" } else { "指纹锚失守" });
    }

    // C186-06 最小台账 ≥12（J1 台账面）
    {
        let mut g = MsrGate::new();
        bootstrap_kernel_set(&mut g).ok();
        let ok = g.table_len() >= 12;
        s.add("C186-06-最小台账", ok, if ok { "" } else { "白名单缺条目" });
    }

    // C186-07 注入 10/10 全抓
    {
        let ok = inject_probe() == Ok(true);
        s.add("C186-07-注入全抓", ok, if ok { "" } else { "注入核漏检" });
    }

    s
}
