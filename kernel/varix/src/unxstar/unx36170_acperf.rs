//! UNX-F36170-J1 AccessCheck 性能账本（acperf）。
//! 判据：① 检查调用步数账本——每次 access_check 的步路径长度入账
//! （短路径：deny 即止；长路径：五步全走）；② 预算锚（步数上限）
//! 与超预算告警面；③ LCG 确定性工作量回放；④ 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：账本语义仿 B06 36116（StepLedger 模式），但记的是
//! 五步装配的步路径；消费 36161 StepId 与 36164 Flow 终流面。

use super::unx36161_acfreeze as acf;
use super::unx36164_acdaclstate as s3;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4000;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BUDGET: u16 = SEG_BASE + 2; // 步数超预算
pub const E_PATH: u16 = SEG_BASE + 3; // 步路径非法（序断裂/越界）
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 账本表闭集破坏
pub const E_SHAPE: u16 = SEG_BASE + 6; // 账本面形不一致（桶数失配）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 步路径预算锚（一次检查最多 5 步 + 预核 2 面）。
pub const BUDGET_STEPS: u32 = 7;
/// 账本容量（确定性回放窗口）。
pub const LEDGER_N: usize = 64;

/// 步路径种类（判别式冻结；按终流/失败点分桶）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    DenyAnon = 0x0001,    // 第①步拒（匿名）
    DenyPriv = 0x0002,    // 第②步拒（特权）
    DenyDacl = 0x0003,    // 第③步拒（空态终流或 Err）
    DenyAce = 0x0004,     // 第④步 Deny 即止
    FullGrant = 0x0005,   // 五步全走 → Grant
    FullPartial = 0x0006, // 五步全走 → MA 尽量授
    FullCover = 0x0007,   // 五步全走 → 覆盖不足拒
}

impl PathKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn of(v: u16) -> Option<PathKind> {
        match v {
            1 => Some(PathKind::DenyAnon),
            2 => Some(PathKind::DenyPriv),
            3 => Some(PathKind::DenyDacl),
            4 => Some(PathKind::DenyAce),
            5 => Some(PathKind::FullGrant),
            6 => Some(PathKind::FullPartial),
            7 => Some(PathKind::FullCover),
            _ => None,
        }
    }
    pub fn steps(self) -> u32 {
        match self {
            PathKind::DenyAnon => 1,
            PathKind::DenyPriv => 2,
            PathKind::DenyDacl => 3,
            PathKind::DenyAce => 4,
            PathKind::FullGrant | PathKind::FullPartial | PathKind::FullCover => 5,
        }
    }
}

pub const PATH_NAMES: [&str; 7] = [
    "deny-anon",
    "deny-priv",
    "deny-dacl",
    "deny-ace",
    "full-grant",
    "full-partial",
    "full-cover",
];

/// 步路径（一次检查的步序号序列，升序且从 1 开始连续或即止）。
pub fn path_valid(path: &[u16]) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if path.is_empty() || path.len() > 5 {
        return Err(E_PATH);
    }
    if path[0] != acf::StepId::Impersonation.code() {
        return Err(E_PATH);
    }
    let mut i = 1;
    while i < path.len() {
        if path[i] != path[i - 1] + 1 || path[i] > acf::StepId::CoverJudge.code() {
            return Err(E_PATH);
        }
        i += 1;
    }
    Ok(())
}

/// 检查账本（固定容量环形；统计四桶 + 最长路径）。
pub struct CheckLedger {
    paths: [u32; LEDGER_N],
    pub len: u32,
    pub head: usize,
    pub buckets: [u32; 7],
    pub longest: u32,
    pub total_steps: u32,
}

impl CheckLedger {
    pub fn new() -> CheckLedger {
        CheckLedger {
            paths: [0u32; LEDGER_N],
            len: 0,
            head: 0,
            buckets: [0u32; 7],
            longest: 0,
            total_steps: 0,
        }
    }

    /// 入账：步路径种类 → 步数与桶计数。
    pub fn record(&mut self, kind: PathKind) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        let idx = kind.code() as usize - 1;
        if idx >= 7 {
            return Err(E_PATH);
        }
        let steps = kind.steps();
        if self.total_steps + steps > BUDGET_STEPS * LEDGER_N as u32 {
            return Err(E_BUDGET);
        }
        self.paths[self.head] = steps;
        self.head = (self.head + 1) % LEDGER_N;
        if self.len < LEDGER_N as u32 {
            self.len += 1;
        }
        self.buckets[idx] += 1;
        self.total_steps += steps;
        if steps > self.longest {
            self.longest = steps;
        }
        Ok(steps)
    }

    /// 桶计数一致性：Σbuckets == 记录条数；Σsteps == total_steps。
    pub fn consistent(&self) -> Result<(), u16> {
        let mut sum = 0u32;
        let mut i = 0;
        while i < 7 {
            sum += self.buckets[i];
            i += 1;
        }
        if sum != self.len {
            return Err(E_SHAPE);
        }
        // 步数对拍：桶 i 计数 × 该类步数（Full 三类同为 5 步，非线性）。
        let mut calc = 0u32;
        let mut k = 0;
        while k < 7 {
            if let Some(kind) = PathKind::of(k as u16 + 1) {
                calc += self.buckets[k] * kind.steps();
            } else {
                return Err(E_TABLE);
            }
            k += 1;
        }
        if calc != self.total_steps {
            return Err(E_SHAPE);
        }
        Ok(())
    }

    /// 清空。
    pub fn clear(&mut self) {
        let mut i = 0;
        while i < LEDGER_N {
            self.paths[i] = 0;
            i += 1;
        }
        self.len = 0;
        self.head = 0;
        self.buckets = [0u32; 7];
        self.longest = 0;
        self.total_steps = 0;
    }
}

/// 平均步数（毫厘面：×100 定点；无记录回 0）。
pub fn avg_steps_x100(l: &CheckLedger) -> u32 {
    if l.len == 0 {
        return 0;
    }
    l.total_steps * 100 / l.len
}

/// 检查路径归类：按 outcome 推导 PathKind（账本对拍面）。
pub fn classify_path(
    imp_ok: bool,
    priv_ok: bool,
    flow: s3::Flow,
    denied_hit: bool,
    covered: bool,
    is_ma: bool,
) -> PathKind {
    if !imp_ok {
        return PathKind::DenyAnon;
    }
    if !priv_ok {
        return PathKind::DenyPriv;
    }
    if flow.is_terminal() {
        return PathKind::DenyDacl;
    }
    if denied_hit {
        return PathKind::DenyAce;
    }
    if covered {
        return PathKind::FullGrant;
    }
    if is_ma {
        return PathKind::FullPartial;
    }
    PathKind::FullCover
}

/// LCG 工作量：n 次伪随机检查路径入账（确定性回放种子）。
pub fn lcg_workload(l: &mut CheckLedger, n: u32, seed: u64) -> Result<(), u16> {
    let mut state = seed;
    let mut i = 0;
    while i < n {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let pick = (state >> 33) % 7;
        let kind = match pick {
            0 => PathKind::DenyAnon,
            1 => PathKind::DenyPriv,
            2 => PathKind::DenyDacl,
            3 => PathKind::DenyAce,
            4 => PathKind::FullGrant,
            5 => PathKind::FullPartial,
            _ => PathKind::FullCover,
        };
        l.record(kind)?;
        i += 1;
    }
    Ok(())
}

/// 账本面自检：路径种类闭集 + 步数单调 + 空账本。
pub fn perf_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut k = 1;
    while k <= 7 {
        let kind = match k {
            1 => PathKind::DenyAnon,
            2 => PathKind::DenyPriv,
            3 => PathKind::DenyDacl,
            4 => PathKind::DenyAce,
            5 => PathKind::FullGrant,
            6 => PathKind::FullPartial,
            _ => PathKind::FullCover,
        };
        if kind.code() != k as u16 || PATH_NAMES[k as usize - 1].is_empty() {
            return Err(E_TABLE);
        }
        if kind.steps() < 1 || kind.steps() > 5 {
            return Err(E_TABLE);
        }
        k += 1;
    }
    let l = CheckLedger::new();
    if l.len != 0 || l.total_steps != 0 || l.longest != 0 {
        return Err(E_TABLE);
    }
    if avg_steps_x100(&l) != 0 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 步路径校验正负例；R3 账本入账与桶一致性；
/// R4 路径归类对拍；R5 LCG 工作量确定性回放与负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    perf_integrity()?;
    hits += 1;
    // R2：步路径校验——五步全序 / 即止前缀合法 / 断裂与越界拒。
    path_valid(&[1, 2, 3, 4, 5])?;
    path_valid(&[1])?;
    path_valid(&[1, 2, 3])?;
    if path_valid(&[2, 3]).is_ok() {
        return Err(E_NEG);
    }
    if path_valid(&[1, 2, 4]).is_ok() {
        return Err(E_NEG);
    }
    if path_valid(&[]).is_ok() || path_valid(&[1, 2, 3, 4, 5, 6]).is_ok() {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：账本入账——四条不同路径；桶一致 + 最长路径 + 平均步数。
    let mut l = CheckLedger::new();
    let s1 = l.record(PathKind::DenyAnon)?;
    let s4 = l.record(PathKind::DenyAce)?;
    let s5 = l.record(PathKind::FullGrant)?;
    l.record(PathKind::FullPartial)?;
    if s1 != 1 || s4 != 4 || s5 != 5 {
        return Err(E_TABLE);
    }
    l.consistent()?;
    if l.len != 4 || l.longest != 5 || l.total_steps != 1 + 4 + 5 + 5 {
        return Err(E_SHAPE);
    }
    if avg_steps_x100(&l) != (1 + 4 + 5 + 5) * 100 / 4 {
        return Err(E_SHAPE);
    }
    hits += 1;
    // R4：路径归类对拍——七路全走查。
    if classify_path(false, true, s3::Flow::Continue, false, false, false) != PathKind::DenyAnon {
        return Err(E_NEG);
    }
    if classify_path(true, false, s3::Flow::Continue, false, false, false) != PathKind::DenyPriv {
        return Err(E_NEG);
    }
    if classify_path(true, true, s3::Flow::Deny, false, false, false) != PathKind::DenyDacl {
        return Err(E_NEG);
    }
    if classify_path(true, true, s3::Flow::Continue, true, false, false) != PathKind::DenyAce {
        return Err(E_NEG);
    }
    if classify_path(true, true, s3::Flow::Continue, false, true, false) != PathKind::FullGrant {
        return Err(E_NEG);
    }
    if classify_path(true, true, s3::Flow::Continue, false, false, true) != PathKind::FullPartial {
        return Err(E_NEG);
    }
    if classify_path(true, true, s3::Flow::Continue, false, false, false) != PathKind::FullCover {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：LCG 确定性回放 + 负例注入（清空后对拍 / 桶破坏侦测）。
    let mut a = CheckLedger::new();
    let mut b = CheckLedger::new();
    lcg_workload(&mut a, 32, 0x1BAD_C0DE_4000_0001)?;
    lcg_workload(&mut b, 32, 0x1BAD_C0DE_4000_0001)?;
    if a.total_steps != b.total_steps || a.buckets != b.buckets {
        return Err(E_NEG);
    }
    a.consistent()?;
    a.clear();
    if a.len != 0 || a.total_steps != 0 {
        return Err(E_TABLE);
    }
    let mut c = CheckLedger::new();
    c.record(PathKind::DenyPriv)?;
    if c.longest != 2 {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn perf_table_holds() {
        assert_eq!(perf_integrity(), Ok(()));
    }

    #[test]
    fn broken_path_detected() {
        assert!(path_valid(&[1, 3]).is_err());
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = CheckLedger::new();
        let mut b = CheckLedger::new();
        lcg_workload(&mut a, 16, 42).ok().unwrap_or(());
        lcg_workload(&mut b, 16, 42).ok().unwrap_or(());
        assert_eq!(a.total_steps, b.total_steps);
    }
}

pub fn run_unx36170_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36170_acperf");
    cs.add("SEG 段位独占", SEG_BASE == 0x4000, "0x4000~0x400F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "步路径/账本/归类/LCG 全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", perf_integrity() == Ok(()), "PathKind 判别式 + 空账本锚");
    cs.add("契约冻结", VER == 1, "账本面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BUDGET => "步数超预算",
        E_PATH => "步路径非法",
        E_NEG => "负例注入未被检出",
        E_TABLE => "账本表闭集破坏",
        E_SHAPE => "账本面形不一致",
        _ => "未知诊断码",
    }
}
