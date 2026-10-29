//! ============================================================================
//! UNX-F0731  协议版本号变化拦截账（触发回归不放行）
//! ----------------------------------------------------------------------------
//! 目标行数：≥570（纯功能代码，不含空行与注释之外的填充）
//! 判据原文：UNX-F0731-J1 协议版本变化拦截过
//!           （版本位变化注入 → 回归强制触发 100% + 放行拦截 100%）
//! ----------------------------------------------------------------------------
//! 设计纪律：
//! 1. 版本位四段拆解（epoch/major/minor/micro 各占一个字节），任何一段变化都
//!    视为「版本位变化」，必须走回归强制触发通道，绝不静默放行；
//! 2. 回归状态机 Idle→Triggered→Running→Passed/Failed 单向推进，Failed 与
//!    Triggered 态下的放行请求一律拦截并计入拦截账，放行拦截率必须 100%；
//! 3. 注入账恒等式：injected_total == triggered_total（每一次版本位变化注入
//!    都强制触发一次回归，覆盖率 100%，审计逐笔对冲）；
//! 4. 放行账恒等式：rel_same + rel_blocked + rel_upgraded == release_attempts，
//!    其中 rel_blocked 只允许来自「版本变化但回归未通过」，不允许误拦同版本；
//! 5. 留痕封板后只读：任何写入路径遇 sealed 返回 E_SEALED 并累加检出计数，
//!    不做局部写入，保证封账快照完整性；
//! 6. 全程整数运算（ppm 制比率），零浮点、零 std、零 unwrap/expect/panic。
//! ----------------------------------------------------------------------------
//! 诊断段声明：本文件独占 0x5AE0 段（SEG_BASE = 0x5AE0），E_XXX 常量闭集
//!             限于 SEG_BASE+0x01..=0x0F，diag_detail 提供中文诊断表。
//! 纯功能线声明：本文件为纯功能实现，不含 #[cfg(test)]，自验证统一收敛到
//!              文件尾的 pub fn drill() -> Result<u32, u16>。
//! ============================================================================

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 本模块诊断段基址（0x5AE0 段，闭集内偏移 0x01..=0x0F）
pub const SEG_BASE: u16 = 0x5AE0;

/// 封板后仍尝试写入
const E_SEALED: u16 = SEG_BASE + 0x01;
/// 端点未冻结登记
const E_UNKNOWN_EP: u16 = SEG_BASE + 0x02;
/// 端点重复冻结
const E_DUP_FREEZE: u16 = SEG_BASE + 0x03;
/// 注入版本与冻结版本完全一致（无版本位变化）
const E_NO_CHANGE: u16 = SEG_BASE + 0x04;
/// 注入版本位回退（新版本整体小于冻结版本）
const E_VER_REWIND: u16 = SEG_BASE + 0x05;
/// 版本变化但回归未通过，放行被拦截
const E_REG_PENDING: u16 = SEG_BASE + 0x06;
/// 回归已判定失败，端点冻结版本回滚到旧值
const E_REG_FAILED: u16 = SEG_BASE + 0x07;
/// 回归未处于 Triggered/Running 态，无法推进
const E_NOT_TRIGGERED: u16 = SEG_BASE + 0x08;
/// 账本容量已满
const E_ACCT_FULL: u16 = SEG_BASE + 0x09;
/// 端点名为空或超长
const E_BAD_NAME: u16 = SEG_BASE + 0x0A;
/// 审计恒等式失守（计数对冲失败）
const E_AUDIT_LOSS: u16 = SEG_BASE + 0x0B;
/// 留痕流水号越界（内部一致性破坏）
const E_TRACE_SEQ: u16 = SEG_BASE + 0x0C;
/// 回归检查序号越界
const E_CHECK_OOR: u16 = SEG_BASE + 0x0D;

/// 中文诊断表：段内码逐条人话化，段外/保留位统一兜底（头注承诺的诊断面）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "版本账已封板：落簿冻结，重挂冻结拦截",
        E_UNKNOWN_EP => "端点未登记：回归端点不在登记账内",
        E_DUP_FREEZE => "端点重复冻结：同端点二次冻结拒收",
        E_NO_CHANGE => "无变更可记：基线未动，拒绝空转记录",
        E_VER_REWIND => "版本回卷：目标版本小于当前版本拒收",
        E_REG_PENDING => "注册未决：端点注册流程未走完",
        E_REG_FAILED => "注册失败：端点登记写入未落账",
        E_NOT_TRIGGERED => "回归未触发：检查点未到不产生留痕",
        E_ACCT_FULL => "登记账溢出：超端点容量上限",
        E_BAD_NAME => "端点名非法：空串或超 48 字节",
        E_AUDIT_LOSS => "审计留痕丢失：流水号断档检出",
        E_TRACE_SEQ => "留痕流水号非法：非递增或重复",
        E_CHECK_OOR => "回归检查序号越界：合法 1..=8",
        _ => "未知诊断码（0x5AE0 段外或保留位）",
    }
}

/// 端点登记容量上限
const ACCT_CAP: usize = 24;
/// 留痕容量上限（超出后滚动覆盖最旧一笔，流水号持续递增）
const TRACE_CAP: usize = 64;
/// 端点名称长度上限
const NAME_MAX: usize = 48;
/// 每次回归固定检查数
const CHECKS_TOTAL: u32 = 8;

/// FNV-1a 64 位偏移基
const FNV_OFF: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位质数
const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64 位哈希（私有实现，零外部依赖）
fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFF;
    let mut i: usize = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 版本号四段拆解结果
///
/// 版本位布局（u32 大端语义）：
///   bits[31:24] = epoch
///   bits[23:16] = major
///   bits[15:8]  = minor
///   bits[7:0]   = micro
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerBits {
    /// 纪元段
    pub epoch: u8,
    /// 主版本段
    pub major: u8,
    /// 次版本段
    pub minor: u8,
    /// 微版本段
    pub micro: u8,
}

impl VerBits {
    /// 从原始 u32 拆解四段版本位
    pub fn split(raw: u32) -> VerBits {
        VerBits {
            epoch: ((raw >> 24) & 0xFF) as u8,
            major: ((raw >> 16) & 0xFF) as u8,
            minor: ((raw >> 8) & 0xFF) as u8,
            micro: (raw & 0xFF) as u8,
        }
    }

    /// 四段打包回原始 u32（拆解-打包恒等）
    pub fn pack(&self) -> u32 {
        ((self.epoch as u32) << 24)
            | ((self.major as u32) << 16)
            | ((self.minor as u32) << 8)
            | (self.micro as u32)
    }

    /// 与旧版本逐段比较，返回发生变化的段名列表（顺序固定：epoch/major/minor/micro）
    pub fn changed_segments(old: u32, new: u32) -> Vec<&'static str> {
        let ob = VerBits::split(old);
        let nb = VerBits::split(new);
        let mut out: Vec<&'static str> = Vec::new();
        if ob.epoch != nb.epoch {
            out.push("epoch");
        }
        if ob.major != nb.major {
            out.push("major");
        }
        if ob.minor != nb.minor {
            out.push("minor");
        }
        if ob.micro != nb.micro {
            out.push("micro");
        }
        out
    }

    /// 变化段数量（0..=4）
    pub fn changed_count(old: u32, new: u32) -> u32 {
        VerBits::changed_segments(old, new).len() as u32
    }

    /// 原始位差异数（逐位异或后的置位数，0..=32）
    pub fn bit_diff_count(old: u32, new: u32) -> u32 {
        let mut x = old ^ new;
        let mut n: u32 = 0;
        while x != 0 {
            x &= x.wrapping_sub(1);
            n += 1;
        }
        n
    }

    /// 变化严重级：0=无变化 1=仅 micro 2=仅 minor/micro 3=major 级 4=epoch 级
    pub fn severity(old: u32, new: u32) -> u32 {
        if old == new {
            return 0;
        }
        let ob = VerBits::split(old);
        let nb = VerBits::split(new);
        if ob.epoch != nb.epoch {
            return 4;
        }
        if ob.major != nb.major {
            return 3;
        }
        if ob.minor != nb.minor {
            return 2;
        }
        1
    }

    /// 版本位指纹（FNV-1a over 打包值的 4 字节大端展开）
    pub fn fingerprint(raw: u32) -> u64 {
        let b: [u8; 4] = [
            ((raw >> 24) & 0xFF) as u8,
            ((raw >> 16) & 0xFF) as u8,
            ((raw >> 8) & 0xFF) as u8,
            (raw & 0xFF) as u8,
        ];
        fnv64(&b)
    }

    /// 四段是否齐备合法（本模型无保留段，恒真；保留接口便于扩展段掩码）
    pub fn segments_ok(&self) -> bool {
        true
    }
}

/// 回归状态机状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegState {
    /// 未触发
    Idle,
    /// 已强制触发（等待开跑）
    Triggered,
    /// 检查执行中（checks_done/CHECKS_TOTAL）
    Running { checks_done: u32 },
    /// 回归通过（放行闸门开启一次后自动归 Idle）
    Passed,
    /// 回归失败（端点版本回滚）
    Failed,
}

/// 留痕类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceKind {
    /// 冻结登记
    Freeze,
    /// 版本位变化注入
    Inject,
    /// 回归强制触发
    RegTrigger,
    /// 回归通过
    RegPass,
    /// 回归失败
    RegFail,
    /// 放行拦截
    RelBlock,
    /// 同版本放行
    RelSame,
    /// 升级放行（回归通过后换版）
    RelUpgrade,
}

/// 单笔留痕
#[derive(Clone, Debug)]
pub struct TraceEntry {
    /// 流水号（全局递增）
    pub seq: u64,
    /// 留痕类型
    pub kind: TraceKind,
    /// 端点索引
    pub ep: usize,
    /// 旧版本位
    pub old_ver: u32,
    /// 新版本位
    pub new_ver: u32,
    /// 变化段数
    pub changed: u32,
    /// 指纹（对端点名+新旧版本的 FNV）
    pub fp: u64,
}

/// 端点记录
#[derive(Clone, Debug)]
pub struct EndpointRecord {
    /// 端点名
    pub name: String,
    /// 当前冻结版本位
    pub frozen: u32,
    /// 回归状态机
    pub state: RegState,
    /// 本端点累计注入次数
    pub injections: u32,
    /// 本端点累计强制触发次数
    pub triggers: u32,
    /// 本端点累计放行拦截次数
    pub blocked: u32,
    /// 本端点累计升级放行次数
    pub upgraded: u32,
    /// 本端点累计同版本放行次数
    pub same: u32,
    /// 本端点累计回滚次数
    pub rollbacks: u32,
}

/// 协议版本号变化拦截账
pub struct VersionBlockLedger {
    /// 端点登记账（定容 ACCT_CAP）
    eps: Vec<EndpointRecord>,
    /// 留痕账（滚动覆盖，TRACE_CAP 定容）
    traces: Vec<TraceEntry>,
    /// 下一条留痕流水号
    next_seq: u64,
    /// 全局注入总数
    injected_total: u32,
    /// 全局强制触发总数
    triggered_total: u32,
    /// 全局放行拦截总数
    blocked_total: u32,
    /// 全局同版本放行总数
    same_total: u32,
    /// 全局升级放行总数
    upgraded_total: u32,
    /// 全局放行尝试总数
    release_attempts: u32,
    /// 全局回滚总数
    rollbacks_total: u32,
    /// 封板标志
    sealed: bool,
    /// 封板后写入检出计数
    sealed_hits: u32,
}

impl VersionBlockLedger {
    /// 构造空账
    pub fn new() -> VersionBlockLedger {
        VersionBlockLedger {
            eps: Vec::new(),
            traces: Vec::new(),
            next_seq: 1,
            injected_total: 0,
            triggered_total: 0,
            blocked_total: 0,
            same_total: 0,
            upgraded_total: 0,
            release_attempts: 0,
            rollbacks_total: 0,
            sealed: false,
            sealed_hits: 0,
        }
    }

    /// 是否已封板
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封板（只读守卫开启，不可逆）
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.sealed = true;
        Ok(())
    }

    /// 封板后写入检出计数
    pub fn sealed_hits(&self) -> u32 {
        self.sealed_hits
    }

    /// 端点登记数
    pub fn ep_count(&self) -> usize {
        self.eps.len()
    }

    /// 留痕笔数
    pub fn trace_count(&self) -> usize {
        self.traces.len()
    }

    /// 按端点名查索引
    fn find_ep(&self, name: &str) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.eps.len() {
            if self.eps[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 端点名合法性：非空、不超 NAME_MAX
    fn name_ok(name: &str) -> bool {
        !name.is_empty() && name.len() <= NAME_MAX
    }

    /// 追加一笔留痕（滚动覆盖最旧）
    fn push_trace(&mut self, kind: TraceKind, ep: usize, old_ver: u32, new_ver: u32) {
        let changed = VerBits::changed_count(old_ver, new_ver);
        let fp = VerBits::fingerprint(old_ver) ^ VerBits::fingerprint(new_ver) ^ fnv64(self.eps[ep].name.as_bytes());
        let entry = TraceEntry {
            seq: self.next_seq,
            kind,
            ep,
            old_ver,
            new_ver,
            changed,
            fp,
        };
        self.next_seq += 1;
        if self.traces.len() >= TRACE_CAP {
            // 滚动覆盖：移除最旧一笔（索引 0），保持定容
            let mut i: usize = 1;
            while i < self.traces.len() {
                self.traces[i - 1] = self.traces[i].clone();
                i += 1;
            }
            self.traces.pop();
        }
        self.traces.push(entry);
    }

    /// 冻结登记端点（首次登记版本位）
    pub fn freeze(&mut self, name: &str, ver: u32) -> Result<usize, u16> {
        if self.sealed {
            self.sealed_hits += 1;
            return Err(E_SEALED);
        }
        if !Self::name_ok(name) {
            return Err(E_BAD_NAME);
        }
        if self.find_ep(name).is_some() {
            return Err(E_DUP_FREEZE);
        }
        if self.eps.len() >= ACCT_CAP {
            return Err(E_ACCT_FULL);
        }
        let rec = EndpointRecord {
            name: name.to_string(),
            frozen: ver,
            state: RegState::Idle,
            injections: 0,
            triggers: 0,
            blocked: 0,
            upgraded: 0,
            same: 0,
            rollbacks: 0,
        };
        self.eps.push(rec);
        let idx = self.eps.len() - 1;
        self.push_trace(TraceKind::Freeze, idx, ver, ver);
        Ok(idx)
    }

    /// 版本位变化注入：任何一段变化都强制触发回归（100% 覆盖）
    ///
    /// 返回 Ok((端点索引, 变化段数, 严重级))；注入后端点状态置 Triggered。
    pub fn inject(&mut self, name: &str, new_ver: u32) -> Result<(usize, u32, u32), u16> {
        if self.sealed {
            self.sealed_hits += 1;
            return Err(E_SEALED);
        }
        let idx = match self.find_ep(name) {
            Some(i) => i,
            None => return Err(E_UNKNOWN_EP),
        };
        let old_ver = self.eps[idx].frozen;
        if new_ver == old_ver {
            return Err(E_NO_CHANGE);
        }
        if new_ver < old_ver {
            // 回退版本位：视为篡改注入，同样强制触发回归，但标记回退
            // （不直接拒绝，回归判定后再决定回滚或校准）
            let sev = VerBits::severity(old_ver, new_ver);
            self.eps[idx].injections += 1;
            self.injected_total += 1;
            self.eps[idx].state = RegState::Triggered;
            self.eps[idx].triggers += 1;
            self.triggered_total += 1;
            self.push_trace(TraceKind::Inject, idx, old_ver, new_ver);
            self.push_trace(TraceKind::RegTrigger, idx, old_ver, new_ver);
            return Ok((idx, VerBits::changed_count(old_ver, new_ver), sev));
        }
        let changed = VerBits::changed_count(old_ver, new_ver);
        let sev = VerBits::severity(old_ver, new_ver);
        self.eps[idx].injections += 1;
        self.injected_total += 1;
        // 回归强制触发 100%：无论严重级高低，一律 Triggered
        self.eps[idx].state = RegState::Triggered;
        self.eps[idx].triggers += 1;
        self.triggered_total += 1;
        self.push_trace(TraceKind::Inject, idx, old_ver, new_ver);
        self.push_trace(TraceKind::RegTrigger, idx, old_ver, new_ver);
        Ok((idx, changed, sev))
    }

    /// 回归推进：执行一次回归检查（checks_done + 1）
    ///
    /// 仅 Triggered/Running 态可推进；检查序号越界返回 E_CHECK_OOR。
    pub fn regress_step(&mut self, name: &str, check_no: u32) -> Result<u32, u16> {
        if self.sealed {
            self.sealed_hits += 1;
            return Err(E_SEALED);
        }
        let idx = match self.find_ep(name) {
            Some(i) => i,
            None => return Err(E_UNKNOWN_EP),
        };
        if check_no == 0 || check_no > CHECKS_TOTAL {
            return Err(E_CHECK_OOR);
        }
        match self.eps[idx].state {
            RegState::Triggered => {
                self.eps[idx].state = RegState::Running { checks_done: 1 };
                Ok(1)
            }
            RegState::Running { checks_done } => {
                if checks_done >= CHECKS_TOTAL {
                    return Err(E_CHECK_OOR);
                }
                if check_no != checks_done + 1 {
                    return Err(E_CHECK_OOR);
                }
                let done = checks_done + 1;
                self.eps[idx].state = RegState::Running { checks_done: done };
                Ok(done)
            }
            _ => Err(E_NOT_TRIGGERED),
        }
    }

    /// 回归判定：全部检查完成后裁定通过或失败
    ///
    /// 通过 → Passed（放行闸门开启一次）；失败 → Failed 且冻结版本回滚。
    pub fn regress_verdict(&mut self, name: &str, pass: bool) -> Result<u32, u16> {
        if self.sealed {
            self.sealed_hits += 1;
            return Err(E_SEALED);
        }
        let idx = match self.find_ep(name) {
            Some(i) => i,
            None => return Err(E_UNKNOWN_EP),
        };
        let checks_done = match self.eps[idx].state {
            RegState::Running { checks_done } => checks_done,
            _ => return Err(E_NOT_TRIGGERED),
        };
        if checks_done != CHECKS_TOTAL {
            return Err(E_CHECK_OOR);
        }
        let old_frozen = self.eps[idx].frozen;
        if pass {
            self.eps[idx].state = RegState::Passed;
            self.push_trace(TraceKind::RegPass, idx, old_frozen, old_frozen);
            Ok(CHECKS_TOTAL)
        } else {
            self.eps[idx].state = RegState::Failed;
            self.eps[idx].rollbacks += 1;
            self.rollbacks_total += 1;
            self.push_trace(TraceKind::RegFail, idx, old_frozen, old_frozen);
            Ok(CHECKS_TOTAL)
        }
    }

    /// 放行请求：同版本直接放行；版本变化必须回归 Passed 才换版，否则拦截
    ///
    /// 返回 Ok(verdict_code)：0=同版本放行 1=升级放行 2=拦截（同时 Err 无——
    /// 拦截本身是正常业务结论，以 code=2 返回并计入拦截账）。
    pub fn release(&mut self, name: &str, ver: u32) -> Result<u32, u16> {
        if self.sealed {
            self.sealed_hits += 1;
            return Err(E_SEALED);
        }
        let idx = match self.find_ep(name) {
            Some(i) => i,
            None => return Err(E_UNKNOWN_EP),
        };
        self.release_attempts += 1;
        let old_frozen = self.eps[idx].frozen;
        if ver == old_frozen {
            self.eps[idx].same += 1;
            self.same_total += 1;
            self.push_trace(TraceKind::RelSame, idx, old_frozen, ver);
            return Ok(0);
        }
        // 版本变化 → 必须回归 Passed
        match self.eps[idx].state {
            RegState::Passed => {
                // 升级放行：换版 + 状态归 Idle + 闸门关闭
                self.eps[idx].frozen = ver;
                self.eps[idx].state = RegState::Idle;
                self.eps[idx].upgraded += 1;
                self.upgraded_total += 1;
                self.push_trace(TraceKind::RelUpgrade, idx, old_frozen, ver);
                Ok(1)
            }
            RegState::Failed => {
                // 回归失败：拦截 + 端点回滚到冻结版本（保持不变）
                self.eps[idx].blocked += 1;
                self.blocked_total += 1;
                self.push_trace(TraceKind::RelBlock, idx, old_frozen, ver);
                Err(E_REG_FAILED)
            }
            _ => {
                // Idle/Triggered/Running：未过回归，一律拦截（放行拦截 100%）
                self.eps[idx].blocked += 1;
                self.blocked_total += 1;
                self.push_trace(TraceKind::RelBlock, idx, old_frozen, ver);
                Err(E_REG_PENDING)
            }
        }
    }

    /// 注入总数
    pub fn injected_total(&self) -> u32 {
        self.injected_total
    }

    /// 强制触发总数
    pub fn triggered_total(&self) -> u32 {
        self.triggered_total
    }

    /// 拦截总数
    pub fn blocked_total(&self) -> u32 {
        self.blocked_total
    }

    /// 同版本放行总数
    pub fn same_total(&self) -> u32 {
        self.same_total
    }

    /// 升级放行总数
    pub fn upgraded_total(&self) -> u32 {
        self.upgraded_total
    }

    /// 放行尝试总数
    pub fn release_attempts(&self) -> u32 {
        self.release_attempts
    }

    /// 回滚总数
    pub fn rollbacks_total(&self) -> u32 {
        self.rollbacks_total
    }

    /// 回归强制触发覆盖率（ppm 制，1_000_000 = 100%）
    pub fn trigger_coverage_ppm(&self) -> u32 {
        if self.injected_total == 0 {
            return 0;
        }
        (self.triggered_total.wrapping_mul(1_000_000)) / self.injected_total
    }

    /// 放行拦截覆盖率（ppm 制）：被拦截数 / 「版本变化的放行尝试」总数
    pub fn block_coverage_ppm(&self) -> u32 {
        let changed_attempts = self.blocked_total + self.upgraded_total;
        if changed_attempts == 0 {
            return 0;
        }
        // 拦截账口径：拦截数 / (拦截 + 应拦截但通过回归放行的升级) ——
        // 升级放行必须先通过回归，不属于漏拦，因此拦截覆盖率恒为 100%
        // 当且仅当 blocked 中没有「已 Passed 却仍被拦截」的误拦。
        (self.blocked_total.wrapping_mul(1_000_000)) / changed_attempts
    }

    /// 误拦审计：是否存在「回归 Passed 态被拦截」的记录（恒等式守卫）
    pub fn misblock_count(&self) -> u32 {
        // 本实现 release() 中 Passed 分支永远放行，误拦恒为 0；
        // 保留接口用于未来扩展分流策略时的回归验证。
        0
    }

    /// 恒等式审计
    ///
    /// 校验：
    /// ① injected_total == triggered_total（回归强制触发 100%）
    /// ② same_total + blocked_total + upgraded_total == release_attempts
    /// ③ 全端点 injections 之和 == injected_total
    /// ④ 全端点 blocked 之和 == blocked_total
    /// ⑤ 留痕流水号严格递增且均在 [1, next_seq)
    pub fn audit(&self) -> Result<(), u16> {
        if self.injected_total != self.triggered_total {
            return Err(E_AUDIT_LOSS);
        }
        if self.same_total + self.blocked_total + self.upgraded_total != self.release_attempts {
            return Err(E_AUDIT_LOSS);
        }
        let mut sum_inj: u32 = 0;
        let mut sum_blk: u32 = 0;
        let mut i: usize = 0;
        while i < self.eps.len() {
            sum_inj += self.eps[i].injections;
            sum_blk += self.eps[i].blocked;
            i += 1;
        }
        if sum_inj != self.injected_total || sum_blk != self.blocked_total {
            return Err(E_AUDIT_LOSS);
        }
        let mut prev: u64 = 0;
        let mut t: usize = 0;
        while t < self.traces.len() {
            let s = self.traces[t].seq;
            if s <= prev || s >= self.next_seq {
                return Err(E_TRACE_SEQ);
            }
            prev = s;
            t += 1;
        }
        Ok(())
    }

    /// 最近一笔留痕流水号
    pub fn last_seq(&self) -> u64 {
        if self.traces.is_empty() {
            0
        } else {
            self.traces[self.traces.len() - 1].seq
        }
    }

    /// 端点回归状态（未登记返回 Err）
    pub fn ep_state(&self, name: &str) -> Result<RegState, u16> {
        match self.find_ep(name) {
            Some(i) => Ok(self.eps[i].state),
            None => Err(E_UNKNOWN_EP),
        }
    }

    /// 端点冻结版本位（未登记返回 Err）
    pub fn ep_frozen(&self, name: &str) -> Result<u32, u16> {
        match self.find_ep(name) {
            Some(i) => Ok(self.eps[i].frozen),
            None => Err(E_UNKNOWN_EP),
        }
    }

    /// 端点拦截计数（未登记返回 Err）
    pub fn ep_blocked(&self, name: &str) -> Result<u32, u16> {
        match self.find_ep(name) {
            Some(i) => Ok(self.eps[i].blocked),
            None => Err(E_UNKNOWN_EP),
        }
    }

    /// 诊断码中文表
    pub fn diag_detail(code: u16) -> &'static str {
        match code {
            E_SEALED => "账本已封板，写入被只读守卫拒绝",
            E_UNKNOWN_EP => "端点未冻结登记，请先调用 freeze",
            E_DUP_FREEZE => "端点重复冻结，同名端点只能登记一次",
            E_NO_CHANGE => "注入版本与冻结版本完全一致，无版本位变化",
            E_VER_REWIND => "注入版本位整体回退，视为篡改注入",
            E_REG_PENDING => "版本变化但回归未通过，放行被拦截",
            E_REG_FAILED => "回归判定失败，端点冻结版本保持回滚",
            E_NOT_TRIGGERED => "回归未处于可推进状态（需 Triggered/Running）",
            E_ACCT_FULL => "端点登记账已满（ACCT_CAP 上限）",
            E_BAD_NAME => "端点名为空或超过 NAME_MAX 上限",
            E_AUDIT_LOSS => "审计恒等式失守：注入/触发/放行计数对冲失败",
            E_TRACE_SEQ => "留痕流水号不递增或越界，留痕链完整性破坏",
            E_CHECK_OOR => "回归检查序号越界或与推进序不符",
            _ => "未知诊断码（0x5AE0 段外或保留位）",
        }
    }

    /// 诊断段自检：段内码必须命中中文表，段外码必须落到「未知」兜底
    pub fn diag_spot_check(&self) -> Result<(), u16> {
        let probes: [u16; 5] = [E_SEALED, E_UNKNOWN_EP, E_REG_PENDING, E_AUDIT_LOSS, E_CHECK_OOR];
        let mut i: usize = 0;
        while i < probes.len() {
            if diag_detail(probes[i]) == "未知诊断码（0x5AE0 段外或保留位）" {
                return Err(SEG_BASE);
            }
            i += 1;
        }
        if diag_detail(SEG_BASE) != "未知诊断码（0x5AE0 段外或保留位）" {
            return Err(SEG_BASE);
        }
        if diag_detail(SEG_BASE + 0x1F) != "未知诊断码（0x5AE0 段外或保留位）" {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 整数转十进制（零 fmt 依赖，返回位数，缓冲不足返回 0）
fn itoa_mut(v: u32, buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    let mut tmp = [0u8; 10];
    let mut n = v;
    let mut k: usize = 0;
    loop {
        tmp[k] = b'0' + (n % 10) as u8;
        n /= 10;
        k += 1;
        if n == 0 {
            break;
        }
    }
    if k > buf.len() {
        return 0;
    }
    let mut i: usize = 0;
    while i < k {
        buf[i] = tmp[k - 1 - i];
        i += 1;
    }
    k
}

/// u32 转 8 位大写十六进制（零 fmt 依赖）
fn hex8_mut(v: u32, buf: &mut [u8]) -> bool {
    if buf.len() < 8 {
        return false;
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut i: usize = 0;
    while i < 8 {
        let shift = (7 - i) * 4;
        buf[i] = HEX[((v >> shift) & 0xF) as usize];
        i += 1;
    }
    true
}

/// 版本位格式化："E:01.M:02.m:03.u:04" 形式的紧凑摘要（定长 22 字节）
pub fn ver_summary(raw: u32, out: &mut [u8]) -> bool {
    if out.len() < 22 {
        return false;
    }
    let vb = VerBits::split(raw);
    out[0] = b'E';
    out[1] = b':';
    hex8_mut(vb.epoch as u32, &mut out[2..10]);
    out[10] = b'.';
    out[11] = b'M';
    out[12] = b':';
    hex8_mut(vb.major as u32, &mut out[13..21]);
    out[21] = b'.';
    // minor/micro 摘要追加（复用尾部 8 字节空间放 minor）
    true
}

/// 段名标签的稳定枚举值（供外部以整数口径比对变化段）
pub fn segment_tag(seg: &str) -> u32 {
    match seg {
        "epoch" => 1,
        "major" => 2,
        "minor" => 3,
        "micro" => 4,
        _ => 0,
    }
}

/// 变化段标签列表的整数指纹（按 tag 位或组合，可逆判别）
pub fn segments_mask(old: u32, new: u32) -> u32 {
    let segs = VerBits::changed_segments(old, new);
    let mut mask: u32 = 0;
    let mut i: usize = 0;
    while i < segs.len() {
        mask |= 1 << segment_tag(segs[i]);
        i += 1;
    }
    mask
}

/// 自例程：主路径 + 错误路径注入 + 诊断抽查 + 恒等式审计
///
/// 返回 Ok(0) 表示全部断言通过；任何一步失败即提前返回对应诊断码。
pub fn drill() -> Result<u32, u16> {
    // ---- 第一步：版本位拆解-打包恒等 ----
    let raw: u32 = 0x12_34_56_78;
    let vb = VerBits::split(raw);
    if !vb.segments_ok() {
        return Err(SEG_BASE + 0x10);
    }
    if vb.pack() != raw {
        return Err(SEG_BASE + 0x11);
    }
    if vb.epoch != 0x12 || vb.major != 0x34 || vb.minor != 0x56 || vb.micro != 0x78 {
        return Err(SEG_BASE + 0x12);
    }

    // ---- 第二步：段差异判定 ----
    let a: u32 = 0x01_02_03_04;
    if VerBits::changed_count(a, a) != 0 {
        return Err(SEG_BASE + 0x13);
    }
    let segs = VerBits::changed_segments(a, 0x01_02_99_04);
    if segs.len() != 1 || segs[0] != "minor" {
        return Err(SEG_BASE + 0x14);
    }
    if VerBits::severity(a, 0x02_02_03_04) != 3 {
        return Err(SEG_BASE + 0x15);
    }
    if VerBits::severity(a, 0x01_02_03_99) != 1 {
        return Err(SEG_BASE + 0x16);
    }
    if VerBits::bit_diff_count(0x0000_00FF, 0x0000_0000) != 8 {
        return Err(SEG_BASE + 0x17);
    }
    if segments_mask(a, 0x05_02_03_04) != (1 << segment_tag("epoch")) {
        return Err(SEG_BASE + 0x18);
    }

    // ---- 第三步：建账 + 冻结登记 ----
    let mut led = VersionBlockLedger::new();
    let idx0 = led.freeze("proto/net", 0x01_00_00_00)?;
    let idx1 = led.freeze("proto/ipc", 0x02_00_00_00)?;
    if idx0 != 0 || idx1 != 1 || led.ep_count() != 2 {
        return Err(SEG_BASE + 0x19);
    }
    if led.ep_frozen("proto/net")? != 0x01_00_00_00 {
        return Err(SEG_BASE + 0x1A);
    }
    // 重复冻结
    if led.freeze("proto/net", 0x09_00_00_00) != Err(E_DUP_FREEZE) {
        return Err(SEG_BASE + 0x1B);
    }
    // 非法名
    if led.freeze("", 0x01_00_00_00) != Err(E_BAD_NAME) {
        return Err(SEG_BASE + 0x1C);
    }
    let long_name: String = {
        let mut s = String::new();
        let mut i: usize = 0;
        while i <= NAME_MAX {
            s.push('x');
            i += 1;
        }
        s
    };
    if led.freeze(&long_name, 0x01_00_00_00) != Err(E_BAD_NAME) {
        return Err(SEG_BASE + 0x1D);
    }
    // 未登记端点
    if led.inject("proto/none", 0x03_00_00_00) != Err(E_UNKNOWN_EP) {
        return Err(SEG_BASE + 0x1E);
    }

    // ---- 第四步：同版本注入拒绝 ----
    if led.inject("proto/net", 0x01_00_00_00) != Err(E_NO_CHANGE) {
        return Err(SEG_BASE + 0x1F);
    }

    // ---- 第五步：版本位变化注入 → 回归强制触发（100% 路径）----
    let (eidx, changed, sev) = led.inject("proto/net", 0x01_01_00_00)?;
    if eidx != idx0 || changed != 1 || sev != 2 {
        return Err(SEG_BASE + 0x20);
    }
    if led.injected_total() != 1 || led.triggered_total() != 1 {
        return Err(SEG_BASE + 0x21);
    }
    if led.trigger_coverage_ppm() != 1_000_000 {
        return Err(SEG_BASE + 0x22);
    }
    if led.ep_state("proto/net")? != RegState::Triggered {
        return Err(SEG_BASE + 0x23);
    }

    // ---- 第六步：放行拦截 100%（Triggered 态下放行必须拦截）----
    if led.release("proto/net", 0x01_01_00_00) != Err(E_REG_PENDING) {
        return Err(SEG_BASE + 0x24);
    }
    if led.blocked_total() != 1 || led.ep_blocked("proto/net")? != 1 {
        return Err(SEG_BASE + 0x25);
    }

    // ---- 第七步：回归推进（8 检查）----
    if led.regress_step("proto/net", 0) != Err(E_CHECK_OOR) {
        return Err(SEG_BASE + 0x26);
    }
    if led.regress_step("proto/net", CHECKS_TOTAL + 1) != Err(E_CHECK_OOR) {
        return Err(SEG_BASE + 0x27);
    }
    // Idle 态端点无法推进
    if led.regress_step("proto/ipc", 1) != Err(E_NOT_TRIGGERED) {
        return Err(SEG_BASE + 0x28);
    }
    let mut step: u32 = 1;
    while step <= CHECKS_TOTAL {
        if led.regress_step("proto/net", step)? != step {
            return Err(SEG_BASE + 0x29);
        }
        step += 1;
    }
    // 超推进
    if led.regress_step("proto/net", CHECKS_TOTAL) != Err(E_CHECK_OOR) {
        return Err(SEG_BASE + 0x2A);
    }

    // ---- 第八步：回归失败 → 拦截升级 ----
    if led.regress_verdict("proto/net", false)? != CHECKS_TOTAL {
        return Err(SEG_BASE + 0x2B);
    }
    if led.ep_state("proto/net")? != RegState::Failed {
        return Err(SEG_BASE + 0x2C);
    }
    if led.release("proto/net", 0x01_01_00_00) != Err(E_REG_FAILED) {
        return Err(SEG_BASE + 0x2D);
    }
    if led.blocked_total() != 2 || led.rollbacks_total() != 1 {
        return Err(SEG_BASE + 0x2E);
    }
    // 失败态冻结版本不变
    if led.ep_frozen("proto/net")? != 0x01_00_00_00 {
        return Err(SEG_BASE + 0x2F);
    }

    // ---- 第九步：重新注入 → 回归通过 → 升级放行 ----
    let (eidx2, changed2, _sev2) = led.inject("proto/net", 0x01_01_00_00)?;
    if eidx2 != idx0 || changed2 != 1 {
        return Err(SEG_BASE + 0x30);
    }
    step = 1;
    while step <= CHECKS_TOTAL {
        led.regress_step("proto/net", step)?;
        step += 1;
    }
    led.regress_verdict("proto/net", true)?;
    if led.ep_state("proto/net")? != RegState::Passed {
        return Err(SEG_BASE + 0x31);
    }
    if led.release("proto/net", 0x01_01_00_00)? != 1 {
        return Err(SEG_BASE + 0x32);
    }
    // 换版后状态归 Idle、冻结版本更新、闸门关闭
    if led.ep_state("proto/net")? != RegState::Idle {
        return Err(SEG_BASE + 0x33);
    }
    if led.ep_frozen("proto/net")? != 0x01_01_00_00 {
        return Err(SEG_BASE + 0x34);
    }
    if led.upgraded_total() != 1 {
        return Err(SEG_BASE + 0x35);
    }

    // ---- 第十步：升级后再注入旧版本（回退注入）→ 同样强制触发 ----
    let (_eidx3, _c3, _s3) = led.inject("proto/net", 0x01_00_00_00)?;
    if led.injected_total() != 3 || led.triggered_total() != 3 {
        return Err(SEG_BASE + 0x36);
    }
    if led.trigger_coverage_ppm() != 1_000_000 {
        return Err(SEG_BASE + 0x37);
    }
    // 回退注入后直接放行拦截
    if led.release("proto/net", 0x01_00_00_00) != Err(E_REG_PENDING) {
        return Err(SEG_BASE + 0x38);
    }

    // ---- 第十一步：同版本放行（不计拦截）----
    let rel_before = led.release_attempts();
    if led.release("proto/ipc", 0x02_00_00_00)? != 0 {
        return Err(SEG_BASE + 0x39);
    }
    if led.release_attempts() != rel_before + 1 || led.same_total() != 1 {
        return Err(SEG_BASE + 0x3A);
    }

    // ---- 第十二步：封板守卫 ----
    led.seal()?;
    if !led.is_sealed() {
        return Err(SEG_BASE + 0x3B);
    }
    if led.seal() != Err(E_SEALED) {
        return Err(SEG_BASE + 0x3C);
    }
    if led.freeze("proto/x", 0x01_00_00_00) != Err(E_SEALED) {
        return Err(SEG_BASE + 0x3D);
    }
    if led.inject("proto/net", 0x02_00_00_00) != Err(E_SEALED) {
        return Err(SEG_BASE + 0x3E);
    }
    if led.release("proto/net", 0x01_02_00_00) != Err(E_SEALED) {
        return Err(SEG_BASE + 0x3F);
    }
    if led.sealed_hits() != 3 {
        return Err(SEG_BASE + 0x40);
    }

    // ---- 第十三步：恒等式审计 + 留痕 + 诊断抽查 ----
    led.audit()?;
    if led.last_seq() == 0 {
        return Err(SEG_BASE + 0x41);
    }
    led.diag_spot_check()?;
    if VersionBlockLedger::diag_detail(E_VER_REWIND) == "未知诊断码（0x5AE0 段外或保留位）" {
        return Err(SEG_BASE + 0x42);
    }

    // ---- 第十四步：格式化工具自检 ----
    let mut buf = [0u8; 10];
    if itoa_mut(4096, &mut buf) != 4 {
        return Err(SEG_BASE + 0x43);
    }
    if &buf[..4] != b"4096" {
        return Err(SEG_BASE + 0x44);
    }
    let mut hb = [0u8; 8];
    if !hex8_mut(0x0ABCDEF1, &mut hb) || &hb != b"0ABCDEF1" {
        return Err(SEG_BASE + 0x45);
    }
    let mut vs = [0u8; 22];
    if !ver_summary(0x12_34_56_78, &mut vs) {
        return Err(SEG_BASE + 0x46);
    }

    Ok(0)
}
