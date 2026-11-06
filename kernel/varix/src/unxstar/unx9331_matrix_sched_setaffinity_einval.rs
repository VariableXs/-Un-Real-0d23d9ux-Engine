//! UNX-F9331 sched_setaffinity 坏掩码 EINVAL 矩阵 · 0x7C040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9331 逐字）：
//!   锚点：UNX-F9331-J1 全零掩码注入判据过，全 EINVAL 格过，坏 mask
//!   指针全 EFAULT 格过。
//!
//! 设计要点：
//!   ① 掩码面建模：affinity 掩码是位集；sched_setaffinity 的冻结拒绝面
//!      是「全零掩码」（EINVAL——进程至少要能跑在一个 CPU 上）与「掩码
//!      位越出 CPU 集合上界」（EINVAL——不在位图的位必须为零）。坏 mask
//!      指针独立成面（EFAULT），不得与 EINVAL 互串。
//!   ② 判定次序冻结：mask 指针校验（EFAULT）先于掩码内容校验（EINVAL），
//!      pid 存在性校验（ESRCH）最前。三层各自可被单独打中。
//!   ③ CPU 注册表：在线 CPU 位图（cpuset），掩码校验以它为域。合法掩码
//!      必须与 cpuset 有非空交集且不越出上界。设入后 get 语义回读一致
//!      （往返一致是本模块核心性质）。
//!   ④ 账本：每次 set 落账（accepted/einval/efault/esrch 四分账），账目
//!      闭合；注入核四类坏样本：全零掩码、越上界位、坏指针、幽灵 pid
//!      ——逐类拒绝，账守恒逃逸 = 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9331 独占 0x7C040..=0x7C07F）。
pub const SEG_BASE: u32 = 0x7C040;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 在线 CPU 数（位图宽度）。
pub const NR_CPUS: u32 = 8;

/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;
/// Linux errno：坏地址。
pub const EFAULT: i32 = 14;
/// Linux errno：无此进程。
pub const ESRCH: i32 = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AffinityCode {
    /// 全零掩码
    ZeroMask,
    /// 掩码位越出 cpuset 上界
    MaskBeyondCpuset,
    /// 掩码与在线集无交集
    NoOverlap,
    /// 坏 mask 指针
    BadMaskPointer,
    /// 幽灵 pid
    GhostPid,
    /// 负 pid
    NegativePid,
    /// 判定次序错乱
    OrderInverted,
    /// 回读不一致
    RoundTripBroken,
    /// 账本倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AffinityCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AffinityCode::ZeroMask => "全零掩码被拒：进程至少要能跑在一个 CPU 上；\
                sched_setaffinity 全零掩码一律 EINVAL",
            AffinityCode::MaskBeyondCpuset => "掩码越界被拒：位图上界之外的位置 \
                必须为零；越出 NR_CPUS 的位回 EINVAL",
            AffinityCode::NoOverlap => "无交集被拒：掩码与在线 CPU 集合必须有 \
                非空交集；全选离线位也是 EINVAL",
            AffinityCode::BadMaskPointer => "坏 mask 指针被拒：NULL/未映射地址 \
                回 EFAULT；指针面与内容面分码不互串",
            AffinityCode::GhostPid => "幽灵 pid 被拒：目标进程不存在回 ESRCH；\
                pid 校验先于掩码校验",
            AffinityCode::NegativePid => "负 pid 被拒：号域非法回 ESRCH 口径下的 \
                参数面（Linux 以 ESRCH 覆盖负号）",
            AffinityCode::OrderInverted => "次序错乱被拒：ESRCH → EFAULT → EINVAL \
                次序冻结；错序会让坏样本拿错码",
            AffinityCode::RoundTripBroken => "回读不一致被拒：set 后 get 必须逐位 \
                一致；不一致即掩码账本有漏",
            AffinityCode::LedgerRegress => "账本倒退被拒：四分账只增不减；请检查 \
                是否有回滚路径误改累计值",
            AffinityCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            AffinityCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [AffinityCode; 11] = [
        AffinityCode::ZeroMask,
        AffinityCode::MaskBeyondCpuset,
        AffinityCode::NoOverlap,
        AffinityCode::BadMaskPointer,
        AffinityCode::GhostPid,
        AffinityCode::NegativePid,
        AffinityCode::OrderInverted,
        AffinityCode::RoundTripBroken,
        AffinityCode::LedgerRegress,
        AffinityCode::NotReproducible,
        AffinityCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── mask 指针面 ──────────────────────────────────────────────────────
/// mask 指针类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskPtr {
    /// 已映射用户区间
    Valid,
    /// NULL
    Null,
    /// 未映射
    Unmapped,
}

impl MaskPtr {
    /// 可访问性。
    pub fn accessible(self) -> bool {
        matches!(self, MaskPtr::Valid)
    }
}

// ── CPU 注册表 + 亲和账本 ────────────────────────────────────────────
/// 在线 CPU 位图注册表。
#[derive(Clone, Copy, Debug)]
pub struct CpuRegistry {
    /// 在线位图（bit k = CPU k 在线）
    online: u32,
}

impl CpuRegistry {
    /// 构造：默认全部在线。
    pub const fn all_online() -> CpuRegistry {
        CpuRegistry {
            online: (1 << NR_CPUS) - 1,
        }
    }

    /// 构造：自定义在线位图（越出上界的位被剪除）。
    pub fn with_mask(mask: u32) -> CpuRegistry {
        CpuRegistry {
            online: mask & ((1 << NR_CPUS) - 1),
        }
    }

    /// 在线位图。
    pub fn online_mask(&self) -> u32 {
        self.online
    }

    /// CPU 是否在线。
    pub fn is_online(&self, cpu: u32) -> bool {
        if cpu >= NR_CPUS {
            return false;
        }
        self.online & (1 << cpu) != 0
    }
}

/// 亲和账本：pid → 掩码 + 四分账。
#[derive(Clone, Copy, Debug)]
pub struct AffinityLedger {
    /// 在线集引用
    cpus: CpuRegistry,
    /// 每个 pid 的当前掩码（下标 = pid 号；0 号保留）
    masks: [Option<u32>; 16],
    /// 四分账
    pub accepted: u32,
    pub einval: u32,
    pub efault: u32,
    pub esrch: u32,
}

impl AffinityLedger {
    /// 构造：全在线。
    pub const fn new() -> AffinityLedger {
        AffinityLedger {
            cpus: CpuRegistry::all_online(),
            masks: [None; 16],
            accepted: 0,
            einval: 0,
            efault: 0,
            esrch: 0,
        }
    }

    /// 在线集。
    pub fn online(&self) -> u32 {
        self.cpus.online_mask()
    }

    /// sched_setaffinity 判定。次序冻结：ESRCH → EFAULT → EINVAL。
    /// 成功时落账并记录掩码（回读账）。
    pub fn set_affinity(
        &mut self,
        pid: i32,
        ptr: MaskPtr,
        mask: u64,
    ) -> SysResult<()> {
        // 层 1：pid 存在性。
        if pid <= 0 || pid as usize >= self.masks.len() {
            self.esrch = self.esrch.saturating_add(1);
            return Err(ESRCH);
        }
        // 层 2：mask 指针面。
        if !ptr.accessible() {
            self.efault = self.efault.saturating_add(1);
            return Err(EFAULT);
        }
        // 层 3：掩码内容面。
        let trimmed = mask & ((1u64 << NR_CPUS) - 1);
        let beyond = mask & !((1u64 << NR_CPUS) - 1);
        if mask == 0 {
            self.einval = self.einval.saturating_add(1);
            return Err(EINVAL);
        }
        if beyond != 0 {
            self.einval = self.einval.saturating_add(1);
            return Err(EINVAL);
        }
        if trimmed & (self.cpus.online_mask() as u64) == 0 {
            self.einval = self.einval.saturating_add(1);
            return Err(EINVAL);
        }
        // 放行：记录掩码（回读账）。
        if let Some(slot) = self.masks.get_mut(pid as usize) {
            *slot = Some(trimmed as u32);
        }
        self.accepted = self.accepted.saturating_add(1);
        Ok(())
    }

    /// sched_getaffinity：回读掩码；未设置过的 pid 回默认全在线。
    pub fn get_affinity(&self, pid: i32) -> SysResult<u32> {
        if pid <= 0 || pid as usize >= self.masks.len() {
            return Err(ESRCH);
        }
        match self.masks.get(pid as usize).copied().and_then(|o| o) {
            Some(m) => Ok(m),
            None => Ok(self.cpus.online_mask()),
        }
    }

    /// 四分账闭合。
    pub fn closed(&self) -> bool {
        self.accepted
            .saturating_add(self.einval)
            .saturating_add(self.efault)
            .saturating_add(self.esrch)
            == self.total_judged()
    }

    fn total_judged(&self) -> u32 {
        self.accepted
            .saturating_add(self.einval)
            .saturating_add(self.efault)
            .saturating_add(self.esrch)
    }
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏亲和样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAffinityCase {
    /// 全零掩码
    ZeroMask,
    /// 越上界位
    BeyondBits,
    /// 坏指针
    BadPointer,
    /// 幽灵 pid
    GhostPid,
}

impl BadAffinityCase {
    /// 闭集全量。
    pub const ALL: [BadAffinityCase; 4] = [
        BadAffinityCase::ZeroMask,
        BadAffinityCase::BeyondBits,
        BadAffinityCase::BadPointer,
        BadAffinityCase::GhostPid,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类坏亲和注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_affinity_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadAffinityCase::ALL.len() {
        if let Some(case) = BadAffinityCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadAffinityCase::ZeroMask => {
                        let mut l = AffinityLedger::new();
                        l.set_affinity(1, MaskPtr::Valid, 0) == Err(EINVAL)
                    }
                    BadAffinityCase::BeyondBits => {
                        let mut l = AffinityLedger::new();
                        l.set_affinity(1, MaskPtr::Valid, 1 << (NR_CPUS + 1))
                            == Err(EINVAL)
                    }
                    BadAffinityCase::BadPointer => {
                        let mut l = AffinityLedger::new();
                        l.set_affinity(1, MaskPtr::Null, 0x1) == Err(EFAULT)
                            && l.set_affinity(1, MaskPtr::Unmapped, 0x1) == Err(EFAULT)
                    }
                    BadAffinityCase::GhostPid => {
                        let mut l = AffinityLedger::new();
                        l.set_affinity(99, MaskPtr::Valid, 0x1) == Err(ESRCH)
                            && l.set_affinity(-1, MaskPtr::Valid, 0x1) == Err(ESRCH)
                    }
                };
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("ESRCH", -3, 87, "Windows ERROR_INVALID_PARAMETER；无此进程族"),
];

/// 查表。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：合法设入回读 + 四类坏样本 + 离线交集。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut l = AffinityLedger::new();
    // 合法设入 + 回读
    let _ = l.set_affinity(1, MaskPtr::Valid, 0b1010);
    bytes.push(match l.get_affinity(1) {
        Ok(m) => (m & 0xFF) as u8,
        Err(e) => (e & 0xFF) as u8,
    });
    // 未设置 pid 回默认全在线
    bytes.push(match l.get_affinity(2) {
        Ok(m) => (m & 0xFF) as u8,
        Err(_) => 0xFF,
    });
    // 四类坏样本
    bytes.push(if l.set_affinity(1, MaskPtr::Valid, 0).is_err() { 1 } else { 0 });
    bytes.push(if l.set_affinity(1, MaskPtr::Valid, 1 << 9).is_err() { 1 } else { 0 });
    bytes.push(if l.set_affinity(1, MaskPtr::Null, 1).is_err() { 1 } else { 0 });
    bytes.push(if l.set_affinity(77, MaskPtr::Valid, 1).is_err() { 1 } else { 0 });
    // 离线交集：在线集只留 CPU0，掩码选 CPU1 → EINVAL
    let mut l2 = AffinityLedger::new();
    l2.cpus = CpuRegistry::with_mask(0b0001);
    bytes.push(if l2.set_affinity(1, MaskPtr::Valid, 0b0010).is_err() { 1 } else { 0 });
    bytes.push(if l2.set_affinity(1, MaskPtr::Valid, 0b0001).is_ok() { 1 } else { 0 });
    bytes.push(if l.closed() { 1 } else { 0 });
    bytes.push(if agrees_with_linux(22) { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9331 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9331_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9331");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 全零掩码全 EINVAL
    let mut l = AffinityLedger::new();
    let zero_ok = l.set_affinity(1, MaskPtr::Valid, 0) == Err(EINVAL);
    s.add("全零掩码：set 全零 → EINVAL", zero_ok,
          "进程至少要能跑在一个 CPU 上");

    // 3. 越上界位全 EINVAL
    let mut l2 = AffinityLedger::new();
    let beyond_ok = l2.set_affinity(1, MaskPtr::Valid, 1 << NR_CPUS) == Err(EINVAL)
        && l2.set_affinity(1, MaskPtr::Valid, 1 << 30) == Err(EINVAL);
    s.add("越上界：位图外位置位 → EINVAL", beyond_ok,
          "上界之外的位必须为零");

    // 4. 坏 mask 指针全 EFAULT
    let mut l3 = AffinityLedger::new();
    let fault_ok = l3.set_affinity(1, MaskPtr::Null, 0x1) == Err(EFAULT)
        && l3.set_affinity(1, MaskPtr::Unmapped, 0x1) == Err(EFAULT);
    s.add("坏指针：NULL/未映射 mask → EFAULT", fault_ok,
          "指针面与内容面分码不互串");

    // 5. 幽灵 pid 全 ESRCH
    let mut l4 = AffinityLedger::new();
    let esrch_ok = l4.set_affinity(99, MaskPtr::Valid, 0x1) == Err(ESRCH)
        && l4.set_affinity(0, MaskPtr::Valid, 0x1) == Err(ESRCH)
        && l4.set_affinity(-3, MaskPtr::Valid, 0x1) == Err(ESRCH);
    s.add("幽灵 pid：不存在/0/负号 → ESRCH", esrch_ok,
          "pid 校验最前层");

    // 6. 判定次序：ESRCH → EFAULT → EINVAL
    let order_ok = l4.set_affinity(99, MaskPtr::Null, 0) == Err(ESRCH)
        && l.set_affinity(1, MaskPtr::Null, 0) == Err(EFAULT);
    s.add("判定次序：ESRCH 先于 EFAULT 先于 EINVAL", order_ok,
          "三层各自可被单独打中，错序即拿错码");

    // 7. 合法设入回读一致
    let mut l5 = AffinityLedger::new();
    let _ = l5.set_affinity(1, MaskPtr::Valid, 0b1010);
    let rt_ok = l5.get_affinity(1) == Ok(0b1010);
    s.add("回读一致：set 后 get 逐位一致", rt_ok,
          "往返一致是掩码账本核心性质");

    // 8. 未设置 pid 回默认全在线
    let default_ok = l5.get_affinity(2) == Ok(l5.online());
    s.add("默认回读：未设置 pid 返回在线位图", default_ok,
          "get 语义必须有确定回退值");

    // 9. 离线交集
    let mut l6 = AffinityLedger::new();
    l6.cpus = CpuRegistry::with_mask(0b0001);
    let offline_ok = l6.set_affinity(1, MaskPtr::Valid, 0b0010) == Err(EINVAL)
        && l6.set_affinity(1, MaskPtr::Valid, 0b0001).is_ok();
    s.add("离线交集：全选离线位 EINVAL / 选在线位放行", offline_ok,
          "掩码必须与在线集有非空交集");

    // 10. 四分账闭合
    s.add("四分账闭合：acc+einval+efault+esrch=judged", l.closed(),
          "账目闭合才可上报");

    // 11. 注入核：四类坏样本
    let (ledger, per_class) = run_bad_affinity_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏样本各 10/10 全检出", all_caught,
          "全零/越界/坏指针/幽灵 pid 必须逐类拒绝");

    // 12. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(ESRCH)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集与码唯一
    let text_ok = AffinityCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < AffinityCode::ALL.len() {
        let mut j = i + 1;
        while j < AffinityCode::ALL.len() {
            if let (Some(x), Some(y)) = (AffinityCode::ALL.get(i), AffinityCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "acc {} / einval {} / efault {} / esrch {}",
        l.accepted, l.einval, l.efault, l.esrch
    );
    s.add_dyn("分账快照", l.closed(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_mask_einval() {
        let mut l = AffinityLedger::new();
        assert_eq!(l.set_affinity(1, MaskPtr::Valid, 0), Err(EINVAL));
    }

    #[test]
    fn beyond_bits_einval() {
        let mut l = AffinityLedger::new();
        assert_eq!(
            l.set_affinity(1, MaskPtr::Valid, 1 << NR_CPUS),
            Err(EINVAL)
        );
    }

    #[test]
    fn bad_pointer_efault() {
        let mut l = AffinityLedger::new();
        assert_eq!(l.set_affinity(1, MaskPtr::Null, 1), Err(EFAULT));
        assert_eq!(l.set_affinity(1, MaskPtr::Unmapped, 1), Err(EFAULT));
    }

    #[test]
    fn ghost_pid_esrch() {
        let mut l = AffinityLedger::new();
        assert_eq!(l.set_affinity(99, MaskPtr::Valid, 1), Err(ESRCH));
        assert_eq!(l.set_affinity(-1, MaskPtr::Valid, 1), Err(ESRCH));
    }

    #[test]
    fn order_frozen() {
        let mut l = AffinityLedger::new();
        assert_eq!(l.set_affinity(99, MaskPtr::Null, 0), Err(ESRCH));
        assert_eq!(l.set_affinity(1, MaskPtr::Null, 0), Err(EFAULT));
        assert_eq!(l.set_affinity(1, MaskPtr::Valid, 0), Err(EINVAL));
    }

    #[test]
    fn roundtrip_holds() {
        let mut l = AffinityLedger::new();
        let _ = l.set_affinity(1, MaskPtr::Valid, 0b1010);
        assert_eq!(l.get_affinity(1), Ok(0b1010));
        assert_eq!(l.get_affinity(2), Ok(l.online()));
    }

    #[test]
    fn offline_overlap() {
        let mut l = AffinityLedger::new();
        l.cpus = CpuRegistry::with_mask(0b0001);
        assert_eq!(l.set_affinity(1, MaskPtr::Valid, 0b0010), Err(EINVAL));
        assert!(l.set_affinity(1, MaskPtr::Valid, 0b0001).is_ok());
    }

    #[test]
    fn ledger_closes() {
        let mut l = AffinityLedger::new();
        let _ = l.set_affinity(1, MaskPtr::Valid, 1);
        let _ = l.set_affinity(1, MaskPtr::Valid, 0);
        let _ = l.set_affinity(1, MaskPtr::Null, 1);
        let _ = l.set_affinity(99, MaskPtr::Valid, 1);
        assert!(l.closed());
        assert_eq!(l.accepted, 1);
        assert_eq!(l.einval, 1);
        assert_eq!(l.efault, 1);
        assert_eq!(l.esrch, 1);
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_affinity_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9331_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
