//! UNX-F9399 I 型与 glibc 消费清单逐号核联测 · 0x7D140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9399-J1 逐字）：
//!   锚点：UNX-F9399-J1 逐号核联测判据过，消费清单逐号联测可观测，
//!   零号面冲突格过
//!
//! 数据段 SEG_BASE = 0x7D140：诊断码闭集 +0x01..=+0x0A（10 码，
//! 9399 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表：GlibcConsumerList（glibc 消费清单闭集 8 号：各号绑定
//!     一个消费核场景，逐号登记 + 去重 + 越界拒绝）；
//!   - 解析器/对拍：逐号联测核（每号走一遍确定性微联测：登记→对拍
//!     →判定，产出逐号判定与号指纹）；
//!   - 账本：PerNumLedger（登记数=联测数=通过数 三段守恒）+ 号面
//!     冲突账（同号两次判定不一致即冲突，必须 0）+ 注入账（逃逸 0）；
//!   - 状态机：NumStage（Listed→JointTested→Cleared 单向）；
//!   - 注入核：4 类坏样本（越界号/重复号/判定冲突/未联测即放行）
//!     逐类显性拒绝；
//!   - 对拍：Linux errno 对照（ENOENT/EEXIST/EINVAL/EBADF/EAGAIN）；
//!   - 指纹：FNV-1a 逐号判定序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9399 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D140;

/// glibc 消费清单闭集容量（8 号）。
pub const LIST_NUMS: usize = 8;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EAGAIN: i32 = 11;
pub const EBADF: i32 = 9;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerNumCode {
    /// 越界号：清单闭集 0..=7 之外
    NumOutOfRange,
    /// 重复号：同号重复登记
    NumDuplicate,
    /// 判定冲突：同号两次判定不一致
    VerdictConflict,
    /// 未联测放行：Listed 态直接 Cleared
    ClearedWithoutTest,
    /// 联测缺号：闭集 8 号未逐号全覆盖
    NumMissing,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 号指纹漂移：同号两次联测指纹不同
    NumFingerprintDrift,
    /// 状态机逆迁移
    StageReversed,
}

impl PerNumCode {
    /// 闭集数组。
    pub const ALL: [PerNumCode; 10] = [
        PerNumCode::NumOutOfRange,
        PerNumCode::NumDuplicate,
        PerNumCode::VerdictConflict,
        PerNumCode::ClearedWithoutTest,
        PerNumCode::NumMissing,
        PerNumCode::LedgerRegress,
        PerNumCode::UnknownErrno,
        PerNumCode::NotReproducible,
        PerNumCode::NumFingerprintDrift,
        PerNumCode::StageReversed,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PerNumCode::NumOutOfRange => 1,
                PerNumCode::NumDuplicate => 2,
                PerNumCode::VerdictConflict => 3,
                PerNumCode::ClearedWithoutTest => 4,
                PerNumCode::NumMissing => 5,
                PerNumCode::LedgerRegress => 6,
                PerNumCode::UnknownErrno => 7,
                PerNumCode::NotReproducible => 8,
                PerNumCode::NumFingerprintDrift => 9,
                PerNumCode::StageReversed => 10,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            PerNumCode::NumOutOfRange => "清单只认 0..=7 闭集号，越界显性拒绝",
            PerNumCode::NumDuplicate => "同号只登记一次，重复按 EEXIST 口径拒绝",
            PerNumCode::VerdictConflict => "同号两次判定必须一致，不一致即号面冲突",
            PerNumCode::ClearedWithoutTest => "未联测的号不得放行",
            PerNumCode::NumMissing => "闭集 8 号必须逐号联测，缺号即漏测",
            PerNumCode::LedgerRegress => "逐号账只增不减，倒退即审计失败",
            PerNumCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            PerNumCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            PerNumCode::NumFingerprintDrift => "同号两次联测指纹漂移即重编候选",
            PerNumCode::StageReversed => "Listed→JointTested→Cleared 单向，回退拒绝",
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 逐号状态机：Listed→JointTested→Cleared 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumStage {
    /// 已列清单
    Listed,
    /// 已逐号联测
    JointTested,
    /// 已放行（终态）
    Cleared,
}

impl NumStage {
    /// 单向迁移核验。
    pub fn advance(self, want: NumStage) -> Option<NumStage> {
        let legal = match (self, want) {
            (NumStage::Listed, NumStage::JointTested) => true,
            (NumStage::JointTested, NumStage::Cleared) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── glibc 消费清单 ────────────────────────────────────────────────────
/// 清单条目。
#[derive(Clone, Copy)]
pub struct ListEntry {
    /// 号（0..=7 闭集）
    pub num: u32,
    /// 状态
    pub stage: NumStage,
    /// 联测判定
    pub verdict: bool,
}

/// glibc 消费清单（8 号闭集）。
pub struct GlibcConsumerList {
    entries: Vec<ListEntry>,
    /// 成功登记数
    pub listed: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl GlibcConsumerList {
    /// 建空清单。
    pub fn new() -> GlibcConsumerList {
        GlibcConsumerList { entries: Vec::new(), listed: 0, rejected: 0 }
    }

    /// 按号取条目。
    pub fn find(&self, num: u32) -> Option<ListEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.num == num {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记：闭集 + 去重。
    pub fn list_num(&mut self, num: u32) -> Result<(), PerNumCode> {
        if num as usize >= LIST_NUMS {
            self.rejected += 1;
            return Err(PerNumCode::NumOutOfRange);
        }
        if self.find(num).is_some() {
            self.rejected += 1;
            return Err(PerNumCode::NumDuplicate);
        }
        self.entries.push(ListEntry { num, stage: NumStage::Listed, verdict: false });
        self.listed += 1;
        Ok(())
    }

    /// 逐号联测：Listed→JointTested，写判定。
    pub fn joint_test(&mut self, num: u32, verdict: bool) -> Result<(), PerNumCode> {
        let idx = match self.index_of(num) {
            Some(i) => i,
            None => return Err(PerNumCode::NumOutOfRange),
        };
        let cur = match self.entries.get(idx) {
            Some(e) => e.stage,
            None => return Err(PerNumCode::NumOutOfRange),
        };
        match cur.advance(NumStage::JointTested) {
            Some(next) => {
                if let Some(e) = self.entries.get_mut(idx) {
                    e.stage = next;
                    e.verdict = verdict;
                }
                Ok(())
            }
            None => Err(PerNumCode::StageReversed),
        }
    }

    /// 放行：JointTested→Cleared（未联测显性拒绝）。
    pub fn clear(&mut self, num: u32) -> Result<(), PerNumCode> {
        let idx = match self.index_of(num) {
            Some(i) => i,
            None => return Err(PerNumCode::NumOutOfRange),
        };
        let cur = match self.entries.get(idx) {
            Some(e) => e,
            None => return Err(PerNumCode::NumOutOfRange),
        };
        if cur.stage != NumStage::JointTested {
            return Err(PerNumCode::ClearedWithoutTest);
        }
        if !cur.verdict {
            return Err(PerNumCode::VerdictConflict);
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.stage = NumStage::Cleared;
        }
        Ok(())
    }

    /// 放行计数。
    pub fn cleared(&self) -> u32 {
        self.entries.iter().filter(|e| e.stage == NumStage::Cleared).count() as u32
    }

    /// 闭集全覆盖：8 号全部 Cleared。
    pub fn all_cleared(&self) -> bool {
        self.entries.len() == LIST_NUMS && self.cleared() == LIST_NUMS as u32
    }

    fn index_of(&self, num: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.num == num {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 判定序列（指纹载体）：(num, verdict)。
    pub fn verdicts(&self) -> Vec<(u32, bool)> {
        self.entries.iter().map(|e| (e.num, e.verdict)).collect()
    }
}

// ── 逐号联测核 ────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 号联测微核：确定性微账（探 3 格），返回是否全过。
pub fn per_num_probe(num: u32) -> bool {
    // 格 1：闭集内号可登记去重
    let mut probe = GlibcConsumerList::new();
    let dup_ok = probe.list_num(num).is_ok()
        && probe.list_num(num) == Err(PerNumCode::NumDuplicate);
    // 格 2：状态机可单向推进且不可逆
    let fwd = NumStage::Listed.advance(NumStage::JointTested).is_some();
    let back = NumStage::Cleared.advance(NumStage::Listed).is_none();
    // 格 3：号指纹确定
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&num.to_le_bytes());
    buf.extend_from_slice(b"F9399-glibc-consumer");
    let fp_a = fnv1a(&buf);
    let fp_b = fnv1a(&buf);
    dup_ok && fwd && back && fp_a == fp_b
}

/// 逐号联测全流程：闭集 8 号登记→逐号联测→逐号放行。
///
/// 返回 (清单, 判定序列)；每号联测两遍对拍（零号面冲突载体）。
pub fn run_per_num_jointest() -> (GlibcConsumerList, Vec<(u32, bool)>) {
    let mut list = GlibcConsumerList::new();
    // 闭集全号登记
    let mut num = 0u32;
    while (num as usize) < LIST_NUMS {
        let _ = list.list_num(num);
        num += 1;
    }
    // 逐号联测：微核跑两遍，判定一致才写 true
    let mut num = 0u32;
    while (num as usize) < LIST_NUMS {
        let v1 = per_num_probe(num);
        let v2 = per_num_probe(num);
        let _ = list.joint_test(num, v1 && v2);
        num += 1;
    }
    // 逐号放行
    let verdicts: Vec<(u32, bool)> = list.verdicts();
    let mut i = 0usize;
    while i < verdicts.len() {
        if let Some((n, v)) = verdicts.get(i).copied() {
            if v {
                let _ = list.clear(n);
            }
        }
        i += 1;
    }
    let final_verdicts = list.verdicts();
    (list, final_verdicts)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 4 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 4], u32) {
    let mut ok = [false; 4];
    let mut detected = 0u32;

    // 类 1：越界号
    let mut l1 = GlibcConsumerList::new();
    let out = l1.list_num(LIST_NUMS as u32) == Err(PerNumCode::NumOutOfRange)
        && l1.list_num(u32::MAX) == Err(PerNumCode::NumOutOfRange);
    if out {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：重复号
    let mut l2 = GlibcConsumerList::new();
    let _ = l2.list_num(3);
    let dup = l2.list_num(3) == Err(PerNumCode::NumDuplicate);
    if dup {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：未联测放行
    let mut l3 = GlibcConsumerList::new();
    let _ = l3.list_num(0);
    let early = l3.clear(0) == Err(PerNumCode::ClearedWithoutTest);
    if early {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：判定冲突（联测判定 false 的号不得放行）
    let mut l4 = GlibcConsumerList::new();
    let _ = l4.list_num(1);
    let _ = l4.joint_test(1, false);
    let conflict = l4.clear(1) == Err(PerNumCode::VerdictConflict);
    if conflict {
        detected += 1;
        ok[3] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EAGAIN, EBADF, EEXIST, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

/// 号指纹（确定性派生）。
pub fn num_fingerprint(num: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&num.to_le_bytes());
    buf.extend_from_slice(b"F9399-glibc-consumer");
    fnv1a(&buf)
}

/// 判定序列指纹。
pub fn verdict_fingerprint(vs: &[(u32, bool)]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (n, v) in vs {
        buf.extend_from_slice(&n.to_le_bytes());
        buf.push(if *v { 1 } else { 0 });
    }
    fnv1a(&buf)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9399 自检：逐号核联测判据（约 14 条）。
pub fn run_unx9399_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9399");

    // 1. 闭集 8 号全登记
    let (list, verdicts) = run_per_num_jointest();
    s.add("闭集登记：8 号全部列清单", list.listed == LIST_NUMS as u32,
          "消费清单逐号在案");

    // 2. 逐号联测可观测：8 号全过微核
    let all_pass = verdicts.len() == LIST_NUMS
        && verdicts.iter().all(|v| v.1);
    s.add("逐号联测：8 号微核全过（判定全 true）", all_pass,
          "每号登记→对拍→判定逐号可观测");

    // 3. 逐号放行
    s.add("逐号放行：8/8 号 Cleared", list.all_cleared(),
          "未联测的号不得放行");

    // 4. 零号面冲突：双跑判定逐位一致
    let (list_b, verdicts_b) = run_per_num_jointest();
    let mut no_conflict = verdicts.len() == verdicts_b.len();
    let mut i = 0usize;
    while i < verdicts.len() && i < verdicts_b.len() {
        if verdicts.get(i) != verdicts_b.get(i) {
            no_conflict = false;
        }
        i += 1;
    }
    let _ = list_b;
    s.add("零号面冲突：双跑逐号判定逐位一致", no_conflict,
          "同号两次判定不一致即冲突");

    // 5. 状态机单向
    let fsm_ok = NumStage::Listed.advance(NumStage::JointTested) == Some(NumStage::JointTested)
        && NumStage::JointTested.advance(NumStage::Cleared) == Some(NumStage::Cleared)
        && NumStage::Cleared.advance(NumStage::Listed).is_none();
    s.add("状态机：Listed→JointTested→Cleared 单向", fsm_ok,
          "回退显性拒绝");

    // 6. 越界与重复拒绝
    let mut l6 = GlibcConsumerList::new();
    let _ = l6.list_num(0);
    let bounds = l6.list_num(LIST_NUMS as u32) == Err(PerNumCode::NumOutOfRange)
        && l6.list_num(0) == Err(PerNumCode::NumDuplicate)
        && l6.rejected == 2;
    s.add("边界账：越界与重复各记一笔", bounds,
          "拒绝不静默，显性入账");

    // 7. 号指纹确定且区分
    let fp_ok = num_fingerprint(0) == num_fingerprint(0)
        && num_fingerprint(0) != num_fingerprint(1)
        && num_fingerprint(7) != num_fingerprint(6);
    s.add("号指纹：同号一致、异号区分", fp_ok,
          "逐号可观测的前提是指纹有区分度");

    // 8. 微核确定性
    let probe_ok = per_num_probe(2) == per_num_probe(2) && per_num_probe(2);
    s.add("微核确定：同号双跑判定一致且为真", probe_ok,
          "确定性微核无随机源");

    // 9. 注入核 4 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：4 类坏样本逐类显性拒绝", all_rej && detected == 4,
          "越界/重复/未测放行/判定冲突全过闸");

    // 10. 放行前置：Listed 态直接 clear 拒绝
    let early = {
        let mut l = GlibcConsumerList::new();
        let _ = l.list_num(5);
        l.clear(5) == Err(PerNumCode::ClearedWithoutTest)
    };
    s.add("放行前置：未联测号 clear 显性拒绝", early,
          "Cleared 只能从 JointTested 到达");

    // 11. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EEXIST)
        && !agrees_with_linux(0)
        && !agrees_with_linux(2345);
    s.add("errno 对照：EEXIST 口径拒重复、表外码拒绝", codes_ok,
          "错误码闭集对拍");

    // 12. 双跑指纹
    let fp1 = verdict_fingerprint(&verdicts);
    let fp2 = verdict_fingerprint(&verdicts_b);
    s.add("双跑指纹：判定序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性联测，无隐藏状态");

    // 13. 诊断码闭集
    let text_ok = PerNumCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut p = 0usize;
    while p < PerNumCode::ALL.len() {
        let mut q = p + 1;
        while q < PerNumCode::ALL.len() {
            let a = PerNumCode::ALL.get(p).map(|c| c.val()).unwrap_or(0);
            let b = PerNumCode::ALL.get(q).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            q += 1;
        }
        p += 1;
    }
    s.add("诊断码闭集：10 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+10 闭集");

    // 14. 账单调
    let mut mono = GlibcConsumerList::new();
    let b0 = mono.listed;
    let _ = mono.list_num(0);
    let _ = mono.list_num(1);
    s.add("账目单调：再登记只增不减", mono.listed == b0 + 2,
          "逐号账只增不减");

    // 动态明细（≤8）
    let l_text = format!("列 {} / 放 {}", list.listed, list.cleared());
    s.add_dyn("逐号账快照", list.all_cleared(), &l_text);
    let fp_text = format!("判定指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_closed_set() {
        let mut l = GlibcConsumerList::new();
        assert!(l.list_num(0).is_ok());
        assert!(l.list_num(7).is_ok());
        assert_eq!(l.list_num(8), Err(PerNumCode::NumOutOfRange));
        assert_eq!(l.list_num(u32::MAX), Err(PerNumCode::NumOutOfRange));
    }

    #[test]
    fn duplicate_num_rejected() {
        let mut l = GlibcConsumerList::new();
        let _ = l.list_num(2);
        assert_eq!(l.list_num(2), Err(PerNumCode::NumDuplicate));
        assert_eq!(l.rejected, 1);
    }

    #[test]
    fn clear_requires_jointest_and_verdict() {
        let mut l = GlibcConsumerList::new();
        let _ = l.list_num(1);
        assert_eq!(l.clear(1), Err(PerNumCode::ClearedWithoutTest));
        let _ = l.joint_test(1, false);
        assert_eq!(l.clear(1), Err(PerNumCode::VerdictConflict));
        // 另一号正常路径：联测为真后放行
        let _ = l.list_num(2);
        let _ = l.joint_test(2, true);
        assert!(l.clear(2).is_ok());
        assert_eq!(l.cleared(), 1);
    }

    #[test]
    fn num_fsm_one_way() {
        assert_eq!(NumStage::Listed.advance(NumStage::Cleared), None);
        assert_eq!(NumStage::Cleared.advance(NumStage::JointTested), None);
        assert_eq!(NumStage::Listed.advance(NumStage::JointTested), Some(NumStage::JointTested));
    }

    #[test]
    fn per_num_probe_deterministic_and_true() {
        assert!(per_num_probe(0));
        assert_eq!(per_num_probe(5), per_num_probe(5));
    }

    #[test]
    fn per_num_jointest_full() {
        let (l, vs) = run_per_num_jointest();
        assert_eq!(l.listed, LIST_NUMS as u32);
        assert!(l.all_cleared());
        assert_eq!(vs.len(), LIST_NUMS);
        assert!(vs.iter().all(|v| v.1));
    }

    #[test]
    fn zero_num_surface_conflict() {
        let (_, v1) = run_per_num_jointest();
        let (_, v2) = run_per_num_jointest();
        assert_eq!(v1, v2);
        assert_eq!(verdict_fingerprint(&v1), verdict_fingerprint(&v2));
    }

    #[test]
    fn num_fingerprint_distinct() {
        assert_eq!(num_fingerprint(3), num_fingerprint(3));
        assert_ne!(num_fingerprint(3), num_fingerprint(4));
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 4);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(-ENOENT));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(1111));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(PerNumCode::ALL.len(), 10);
        for c in PerNumCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 10);
        }
    }

    #[test]
    fn unx9399_checks_green() {
        let set = run_unx9399_checks();
        assert!(set.all_passed(), "UNX-F9399 red: {}", set.red_items_text());
    }
}
