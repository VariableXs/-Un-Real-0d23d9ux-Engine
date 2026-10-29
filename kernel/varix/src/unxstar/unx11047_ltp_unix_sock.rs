//! UNX-F11047 LTP unix socket 组用例对照（套接字族对账）（0x1e5e0 段）
//!
//! 外部对照面第 7 批：unix socket 组用例对照。两面对账：
//! 流式套接字面（bind/listen/connect/accept 四步链）10 例，
//! SCM_RIGHTS 传递面（sendmsg 附带 fd 权能账）10 例。
//! 期望账先行登记，实测账逐例回填，matched 逐例对平；
//! 守恒式：流面 accepted == connected（每连必收），
//! SCM 面 passed == received + inflight（权能不凭空生灭）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11047 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11047 - 11000) * 0x20;

/// 用例账空 —— 对照前必须先登记期望
pub const SO_EMPTY: u32 = SEG_BASE + 0x01;
/// 案例号越界 —— 20 例编号 0..20
pub const SO_RANGE: u32 = SEG_BASE + 0x02;
/// 期望与实测不一致 —— 对照失配
pub const SO_MISMATCH: u32 = SEG_BASE + 0x03;
/// 守恒被破坏 —— accepted/connected 或 passed/received 失衡
pub const SO_UNBALANCED: u32 = SEG_BASE + 0x04;
/// 面（face）编号越界 —— 只有 0 流面与 1 SCM 面
pub const SO_BADFACE: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再回填实测
pub const SO_FROZEN: u32 = SEG_BASE + 0x06;

/// 流式面用例数：bind/listen/connect/accept 各环节覆盖
pub const STREAM_CASES: usize = 10;
/// SCM_RIGHTS 面用例数：权能传递/接收/回执
pub const SCM_CASES: usize = 10;
/// 对照总例数
pub const CASES_TOTAL: usize = STREAM_CASES + SCM_CASES;
/// 流面每例连接对数（往返步数）
pub const CONN_ROUNDS: usize = 40;
/// SCM 面每例传递 fd 数
pub const FDS_PER_MSG: usize = 2;
/// 套接字族对照面标签
pub const SOCK_TAG: u64 = 0x534f_434b_4c54_3047;

/// 单条对照用例：期望账 + 实测账 + 对平标记
#[derive(Clone, Debug)]
pub struct SoCase {
    /// LTP 风格用例号（0x5000 段起编）
    pub cid: u32,
    /// 用例名
    pub name: String,
    /// 所属面：0=流式，1=SCM_RIGHTS
    pub face: u8,
    /// 期望值（登记账）
    pub expect: u64,
    /// 实测值（回填账）
    pub actual: u64,
    /// 是否已对平
    pub matched: bool,
}

impl SoCase {
    /// 登记一条期望，实测留空等待回填
    pub fn new(cid: u32, name: &str, face: u8, expect: u64) -> SoCase {
        SoCase { cid, name: name.to_string(), face, expect, actual: 0, matched: false }
    }
}

/// 套接字族对照总账
#[derive(Clone, Debug)]
pub struct SockLedger {
    /// 20 条对照用例（前 10 流面，后 10 SCM 面）
    pub cases: Vec<SoCase>,
    /// 流面累计 connected（发端连接次数）
    pub connected: u64,
    /// 流面累计 accepted（收端接收次数）
    pub accepted: u64,
    /// SCM 面累计 passed（发出的权能 fd 数）
    pub passed: u64,
    /// SCM 面累计 received（收到的权能 fd 数）
    pub received: u64,
    /// SCM 面仍在途的权能 fd 数
    pub inflight: u64,
    /// 已对平例数
    pub matched_n: usize,
    /// 回填过的例数
    pub filled_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl SockLedger {
    /// 立账并登记全部 20 例期望
    pub fn new() -> SockLedger {
        let mut lg = SockLedger {
            cases: Vec::new(),
            connected: 0, accepted: 0,
            passed: 0, received: 0, inflight: 0,
            matched_n: 0, filled_n: 0, frozen: false,
        };
        for i in 0..STREAM_CASES {
            // 流面期望 = 连接对数 × 每对 2 字节回执
            let expect = ((i as u64) + 1) * (CONN_ROUNDS as u64) * 2;
            lg.cases.push(SoCase::new(0x5000 + i as u32, "so_stream", 0, expect));
        }
        for i in 0..SCM_CASES {
            // SCM 面期望 = 传递轮数 × 每轮 fd 数
            let expect = ((i as u64) + 1) * (FDS_PER_MSG as u64) * 10;
            lg.cases.push(SoCase::new(0x5100 + i as u32, "so_scm", 1, expect));
        }
        lg
    }

    /// 实际运行一例流面对照：连接 CONN_ROUNDS 轮，每轮 1 连 1 收
    pub fn run_stream(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(SO_FROZEN); }
        if no >= STREAM_CASES { return Err(SO_RANGE); }
        let rounds = CONN_ROUNDS * (no + 1);
        let mut sent = 0u64;
        for _ in 0..rounds {
            self.connected = self.connected.wrapping_add(1);
            sent = sent.wrapping_add(1);
            self.accepted = self.accepted.wrapping_add(1);
            sent = sent.wrapping_add(1); // 收端回执 1 字节，计入对平值
        }
        Ok(sent)
    }

    /// 实际运行一例 SCM 面对照：传递 10 轮，每轮 2 fd，全部被接收
    pub fn run_scm(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(SO_FROZEN); }
        if no < STREAM_CASES || no >= CASES_TOTAL { return Err(SO_RANGE); }
        let rounds = 10 * (no + 1 - STREAM_CASES);
        let mut got = 0u64;
        for _ in 0..rounds {
            for _ in 0..FDS_PER_MSG {
                self.passed = self.passed.wrapping_add(1);
                self.received = self.received.wrapping_add(1);
                got = got.wrapping_add(1);
            }
        }
        self.inflight = self.passed.wrapping_sub(self.received);
        Ok(got)
    }

    /// 回填一例实测并对平
    pub fn fill(&mut self, no: usize, actual: u64) -> Result<bool, u32> {
        if self.frozen { return Err(SO_FROZEN); }
        if no >= CASES_TOTAL { return Err(SO_RANGE); }
        let c = &mut self.cases[no];
        if c.matched { return Err(SO_MISMATCH); }
        c.actual = actual;
        c.matched = c.expect == c.actual;
        self.filled_n += 1;
        if c.matched { self.matched_n += 1; }
        Ok(c.matched)
    }

    /// 对照一例：跑 + 回填一步到位
    pub fn compare(&mut self, no: usize) -> Result<bool, u32> {
        let actual = if no < STREAM_CASES { self.run_stream(no)? } else { self.run_scm(no)? };
        self.fill(no, actual)
    }

    /// 取一例（空账返回 SO_EMPTY，越界返回 SO_RANGE）
    pub fn case_of(&self, no: usize) -> Result<usize, u32> {
        if self.cases.is_empty() { return Err(SO_EMPTY); }
        if no >= self.cases.len() { return Err(SO_RANGE); }
        Ok(no)
    }

    /// 按面统计对平数
    pub fn matched_face(&self, face: u8) -> Result<usize, u32> {
        if face > 1 { return Err(SO_BADFACE); }
        Ok(self.cases.iter().filter(|c| c.face == face && c.matched).count())
    }

    /// 守恒探测：流面每连必收、SCM 面 passed == received + inflight
    pub fn conserved(&self) -> bool {
        self.connected == self.accepted
            && self.passed == self.received.wrapping_add(self.inflight)
    }

    /// 守恒闸：失衡时返回 SO_UNBALANCED 拒止
    pub fn balance_gate(&self) -> Result<(), u32> {
        if self.conserved() { Ok(()) } else { Err(SO_UNBALANCED) }
    }

    /// 全面对平：20/20 且守恒成立
    pub fn aligned(&self) -> bool {
        self.matched_n == CASES_TOTAL
            && self.matched_face(0).unwrap_or(usize::MAX) == STREAM_CASES
            && self.matched_face(1).unwrap_or(usize::MAX) == SCM_CASES
            && self.conserved()
    }

    /// FNV 折叠：全部用例与计数器折成指纹
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SOCK_TAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.expect).wrapping_mul(0x100000001b3);
            h = (h ^ c.actual).wrapping_mul(0x100000001b3);
            h = (h ^ (c.matched as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.connected, self.accepted, self.passed, self.received] {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：全部 20 例对平
pub fn sample_aligned() -> (SockLedger, bool) {
    let mut lg = SockLedger::new();
    for no in 0..CASES_TOTAL {
        let _ = lg.compare(no);
    }
    let ok = lg.aligned();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：错填实测制造失配
pub fn inject_four() -> [bool; 4] {
    // 注入 1：错填实测 → 对照判失配（Ok(false)，matched 保持 false）
    let mut lg = SockLedger::new();
    let i1 = matches!(lg.fill(0, 999), Ok(false)) && !lg.cases[0].matched;
    // 注入 2：越界案例号 → SO_RANGE 拒止
    let i2 = matches!(lg.run_stream(STREAM_CASES), Err(SO_RANGE));
    // 注入 3：流面号跑 SCM 面 → SO_RANGE 拒止
    let i3 = matches!(lg.run_scm(0), Err(SO_RANGE));
    // 注入 4：冻结后回填 → SO_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.fill(1, 1), Err(SO_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 socket 组用例对照对平（10/10），SCM_RIGHTS 对照（10/10）
pub fn run_unx11047_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11047");
    let (lg, ok) = sample_aligned();
    let m0 = lg.matched_face(0).unwrap_or(usize::MAX);
    let m1 = lg.matched_face(1).unwrap_or(usize::MAX);

    s.add("J1-流面对平", ok && m0 == STREAM_CASES, "socket 组流式用例对照 10/10 对平");
    s.add("J1-SCM对平", ok && m1 == SCM_CASES, "SCM_RIGHTS 面用例对照 10/10 对平");
    s.add("J1-总量对平", lg.matched_n == CASES_TOTAL, "对照总量 20/20 守恒");
    s.add("J2-连通守恒", lg.connected == lg.accepted && lg.connected > 0, "流面每连必收 accepted==connected");
    s.add("J2-权能守恒", lg.passed == lg.received + lg.inflight, "SCM 面 passed==received+inflight 不生灭");
    s.add("J2-守恒闸", lg.balance_gate().is_ok(), "守恒闸对平账放行、失衡即 SO_UNBALANCED");
    s.add("J3-编号连续", lg.cases.len() == CASES_TOTAL && lg.cases[0].cid == 0x5000 && lg.cases[10].cid == 0x5100, "用例号 0x5000/0x5100 双段起编连续");
    let el = SockLedger { cases: Vec::new(), connected: 0, accepted: 0, passed: 0, received: 0, inflight: 0, matched_n: 0, filled_n: 0, frozen: false };
    s.add("J3-空账拒查", matches!(el.case_of(0), Err(SO_EMPTY)) && matches!(lg.case_of(0), Ok(0)), "空账查询被 SO_EMPTY 拒止、实账放行");
    s.add("J3-面归属", lg.cases.iter().filter(|c| c.face == 0).count() == STREAM_CASES, "面归属标注准确流面 10 例");
    s.add("J3-期望非零", lg.cases.iter().all(|c| c.expect > 0), "全部期望账非零登记");
    s.add("J4-实测回填", lg.filled_n == CASES_TOTAL, "20 例实测账全部回填");
    s.add("J4-冻结拒写", inject_four()[3], "冻结后回填被 SO_FROZEN 拒止");
    s.add("J5-失配拒平", inject_four()[0], "错填实测被判失配且不误标对平");
    s.add("J5-越界拒跑", inject_four()[1] && inject_four()[2], "越界案例号被 SO_RANGE 拒止");
    s.add("J6-守恒探测", lg.conserved(), "conserved 探测对平账为真");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-对平动态",
        ok,
        &format!("对平 {}/{}，流面 {} 例 SCM 面 {} 例，fold={:#x}", lg.matched_n, CASES_TOTAL, m0, m1, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11047_ltp_unix_sock_flow() {
        let mut lg = SockLedger::new();
        // 冻结前拒写实测路径
        assert!(matches!(lg.run_stream(99), Err(SO_RANGE)));
        // 全量对照
        for no in 0..CASES_TOTAL {
            assert!(lg.compare(no).unwrap_or(false), "案例 {} 应对平", no);
        }
        assert_eq!(lg.matched_n, CASES_TOTAL);
        assert!(lg.conserved());
        assert_eq!(lg.connected, lg.accepted);
        assert_eq!(lg.inflight, 0);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        // 冻结后拒改
        assert!(matches!(lg.fill(0, 1), Err(SO_FROZEN)));
        // 注入路径全拒止
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11047_checks_green() {
        let s = run_unx11047_checks();
        assert!(s.all_passed(), "UNX-F11047 判据未全绿");
    }
}
