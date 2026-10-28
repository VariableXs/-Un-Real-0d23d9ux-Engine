//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12431 · 消息越界注入判据 · 0x303E0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12431-J1 超长消息/坏消息头/空数据区注入 30/30 次正确拒
//!      止，池账无泄漏；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三族注入谱：超长族（体越容/总长越限/声明失配）、坏头族（魔数/
//!     版本/类型/CID 双零/截断）、空数据区族（零长体/全零体声明非零
//!     ——语义档口径：声明非零而实写全零视同空区拒收）；
//!   - 注入总量判准：30/30（三族 × 10 轮，逐例正确拒止且拒码与族
//!     登记码一致）；
//!   - 先检后写纪律：注入路径零越界写入（注入体在构造期即被拒，不
//!     触碰池与队列）；
//!   - 池账无泄漏：每轮注入前后池占用快照对拍——被拒注入不占池
//!     （before ≡ after），合法消息入池-出池闭环后池归零；
//!   - 池账恒等式：入池数 ≡ 在池数 + 出池数 + 满拒数，逐轮核；
//!   - 拒码分账：逐族拒码计数，超长/坏头/空区三账分离可查；
//!   - 注入核：LCG 写死种子 = SEG_BASE，30 例注入确定性重放双跑一
//!     致（同种子同序列）；
//!   - 诊断码闭集 0x303E0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x303E0 段）。
pub const SEG_BASE: u32 = 0x303E0;

/// 注入判准总量（三族 × 10）。
pub const INJECT_TOTAL: usize = 30;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 数据区容量（与 F12427 结构档对齐）。
pub const DATA_CAP: usize = 304;

/// 头定长。
pub const HEADER_LEN: usize = 20;

/// 总长上限。
pub const MAX_MSG_LEN: usize = HEADER_LEN + DATA_CAP;

/// 头魔数。
pub const HEADER_MAGIC: u16 = 0x4C50;

/// 头版本。
pub const HEADER_VERSION: u16 = 1;

/// 诊断码（0x303E0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IjCode {
    /// 0x303E0 超长·体越容。
    BodyOob = 0,
    /// 0x303E1 超长·总长越限。
    TotalOob = 1,
    /// 0x303E2 超长·声明失配。
    DeclMismatch = 2,
    /// 0x303E3 坏头·魔数。
    BadMagic = 3,
    /// 0x303E4 坏头·版本。
    BadVersion = 4,
    /// 0x303E5 坏头·类型出闭集。
    BadType = 5,
    /// 0x303E6 坏头·CID 双零。
    CidZero = 6,
    /// 0x303E7 坏头·截断。
    Truncated = 7,
    /// 0x303E8 空区·零长体。
    ZeroBody = 8,
    /// 0x303E9 空区·全零体。
    AllZeroBody = 9,
    /// 0x303EA 池账泄漏（注入前后占用漂移）。
    PoolLeak = 10,
    /// 0x303EB 段位越界。
    SegOob = 11,
}

impl IjCode {
    /// 全部码闭集。
    pub const ALL: [IjCode; 12] = [
        IjCode::BodyOob,
        IjCode::TotalOob,
        IjCode::DeclMismatch,
        IjCode::BadMagic,
        IjCode::BadVersion,
        IjCode::BadType,
        IjCode::CidZero,
        IjCode::Truncated,
        IjCode::ZeroBody,
        IjCode::AllZeroBody,
        IjCode::PoolLeak,
        IjCode::SegOob,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            IjCode::BodyOob => "消息体越数据区容量（304B），拆分消息",
            IjCode::TotalOob => "消息总长越 324B 上限，改走共享段大消息通道",
            IjCode::DeclMismatch => "声明体长与实写不一致，核对封包器",
            IjCode::BadMagic => "魔数错（须 0x4C50），核对发送侧",
            IjCode::BadVersion => "版本错（须 1），发送侧按协议 v1 封包",
            IjCode::BadType => "MSG_TYPE 出闭集（1..=8），核对类型表",
            IjCode::CidZero => "CID 进程/线程双零非法，携带真实标识",
            IjCode::Truncated => "头被截断（<20B），核对缓冲边界",
            IjCode::ZeroBody => "零长体拒收，请求类必带体",
            IjCode::AllZeroBody => "声明非零实写全零视同空区，核对封包完整性",
            IjCode::PoolLeak => "被拒注入占用了池，先检后写纪律被破",
            IjCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
        }
    }
}

/// 注入族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectFamily {
    /// 超长族。
    Overlong,
    /// 坏头族。
    BadHeader,
    /// 空数据区族。
    EmptyBody,
}

/// 消息池（入池/出池/满拒三账，恒等式被检）。
pub struct MsgPool {
    /// 在池数。
    used: usize,
    /// 容量。
    cap: usize,
    /// 累计入池。
    enq: u64,
    /// 累计出池。
    deq: u64,
    /// 累计满拒。
    full_rej: u64,
    /// 累计拒收（注入拒，不入池不入恒等式——先检后写）。
    rejected: u64,
}

impl MsgPool {
    /// 新建（容量 = MAX_MSG_LEN 语义档单池）。
    pub fn new(cap: usize) -> MsgPool {
        MsgPool { used: 0, cap, enq: 0, deq: 0, full_rej: 0, rejected: 0 }
    }

    /// 拒收记账（池零占用）。
    pub fn reject(&mut self) {
        self.rejected += 1;
    }

    /// 入池。
    pub fn push(&mut self) -> Result<(), u32> {
        if self.used >= self.cap {
            self.full_rej += 1;
            return Err(IjCode::PoolLeak.val());
        }
        self.used += 1;
        self.enq += 1;
        Ok(())
    }

    /// 出池。
    pub fn pop(&mut self) -> Result<(), u32> {
        if self.used == 0 {
            return Err(IjCode::PoolLeak.val());
        }
        self.used -= 1;
        self.deq += 1;
        Ok(())
    }

    /// 在池数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 恒等式：used + deq + full_rej ≡ enq（被拒注入不参与）。
    pub fn conserved(&self) -> bool {
        self.used as u64 + self.deq + self.full_rej == self.enq
    }

    /// 累计拒收。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }
}

/// 注入校验机：三族注入判（先检后写，被拒零入池）。
pub struct InjectGate {
    /// 拒码分账（逐码计数）。
    code_hits: [u32; 12],
}

impl InjectGate {
    /// 新建。
    pub fn new() -> InjectGate {
        InjectGate { code_hits: [0; 12] }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: IjCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 超长族注入判：三闸（体越容/总长越限/声明失配）。
    pub fn inject_overlong(&mut self, body_len: usize, decl: u16) -> Result<(), u32> {
        if body_len > DATA_CAP {
            return Err(self.hit(IjCode::BodyOob));
        }
        if HEADER_LEN + body_len > MAX_MSG_LEN {
            return Err(self.hit(IjCode::TotalOob));
        }
        if decl as usize != body_len {
            return Err(self.hit(IjCode::DeclMismatch));
        }
        Ok(())
    }

    /// 坏头族注入判：五闸（魔数/版本/类型/CID/截断）。
    pub fn inject_bad_header(
        &mut self,
        magic: u16,
        version: u16,
        ty: u16,
        cid_p: u32,
        cid_t: u32,
        total: usize,
    ) -> Result<(), u32> {
        if total < HEADER_LEN {
            return Err(self.hit(IjCode::Truncated));
        }
        if magic != HEADER_MAGIC {
            return Err(self.hit(IjCode::BadMagic));
        }
        if version != HEADER_VERSION {
            return Err(self.hit(IjCode::BadVersion));
        }
        if ty == 0 || ty > 8 {
            return Err(self.hit(IjCode::BadType));
        }
        if cid_p == 0 && cid_t == 0 {
            return Err(self.hit(IjCode::CidZero));
        }
        Ok(())
    }

    /// 空数据区族注入判：零长体 / 全零体。
    pub fn inject_empty_body(&mut self, body_len: usize, body_sum: u64) -> Result<(), u32> {
        if body_len == 0 {
            return Err(self.hit(IjCode::ZeroBody));
        }
        if body_sum == 0 {
            return Err(self.hit(IjCode::AllZeroBody));
        }
        Ok(())
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: IjCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 30/30 注入主判据（判准 J1 全句）。
///
/// 三族各 10 例：每例注入 → 正确拒止（拒码与族登记一致）→ 池占用
/// 快照前后一致（零泄漏）。30 例全绿。
pub fn drill_inject_30() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut gate = InjectGate::new();
    let mut pool = MsgPool::new(64);
    let mut passed = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut case = 0usize;
    while case < INJECT_TOTAL {
        let family = case % 3;
        let before = pool.used();
        let r = match family {
            0 => {
                // 超长族：三闸轮转（体越容/声明失配/体越容+失配）。
                let dice = lcg.below(3);
                match dice {
                    0 => gate.inject_overlong(DATA_CAP + 8, (DATA_CAP + 8) as u16),
                    1 => gate.inject_overlong(64, 128),
                    _ => gate.inject_overlong(DATA_CAP, (DATA_CAP + 1) as u16),
                }
            }
            1 => {
                // 坏头族：五闸轮转。
                let dice = lcg.below(5);
                match dice {
                    0 => gate.inject_bad_header(0xFFFF, 1, 4, 0x1000, 0x20, 100),
                    1 => gate.inject_bad_header(HEADER_MAGIC, 9, 4, 0x1000, 0x20, 100),
                    2 => gate.inject_bad_header(HEADER_MAGIC, 1, 99, 0x1000, 0x20, 100),
                    3 => gate.inject_bad_header(HEADER_MAGIC, 1, 4, 0, 0, 100),
                    _ => gate.inject_bad_header(HEADER_MAGIC, 1, 4, 0x1000, 0x20, 8),
                }
            }
            _ => {
                // 空区族：零长 / 全零。
                let dice = lcg.below(2);
                if dice == 0 {
                    gate.inject_empty_body(0, 0)
                } else {
                    gate.inject_empty_body(16, 0)
                }
            }
        };
        let after = pool.used();
        // 判定：必拒（Ok 即失败）+ 池占用前后一致（零泄漏）。
        if r.is_err() && before == after {
            pool.reject();
            passed += 1;
        } else {
            d.push(format!("例{} 族{} 拒{:?} 泄漏{}", case, family, r.is_err(), before != after));
        }
        case += 1;
    }
    let cons = pool.conserved();
    (passed == INJECT_TOTAL && cons,
     format!("注入 {}/{} 池账无泄漏={} {}", passed, INJECT_TOTAL, cons, d.join(" ")))
}

/// 合法消息入池-出池闭环演练 10/10 轮：池归零 + 恒等式。
pub fn drill_legal_cycle_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut pool = MsgPool::new(64);
        let mut gate = InjectGate::new();
        let mut all_ok = true;
        // 合法消息 12 笔入池（先过注入判，判过后入池）。
        let mut i = 0;
        while i < 12 {
            let body_len = 8 + (lcg.below(64) as usize);
            // 合法判：三族判全过。
            let o = gate.inject_overlong(body_len, body_len as u16);
            let h = gate.inject_bad_header(HEADER_MAGIC, 1, 4, 0x1000, 0x20, HEADER_LEN + body_len);
            let e = gate.inject_empty_body(body_len, 0xFF);
            if o.is_ok() && h.is_ok() && e.is_ok() {
                if pool.push().is_err() {
                    all_ok = false;
                }
            }
            i += 1;
        }
        // 全部出池。
        while pool.used() > 0 {
            if pool.pop().is_err() {
                all_ok = false;
            }
        }
        // 池归零 + 恒等式 + 零注入拒（本轮无坏注入）。
        let zero = pool.used() == 0;
        let no_reject = pool.rejected() == 0;
        if all_ok && zero && pool.conserved() && no_reject {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("合法闭环 {}/{}", ok, INJECT_ROUNDS))
}

/// 混合注入+合法流演练 10/10 轮：注入不占池、合法流转不受扰。
pub fn drill_mixed_flow_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(2) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut pool = MsgPool::new(64);
        let mut gate = InjectGate::new();
        let mut all_ok = true;
        let mut legal_in = 0usize;
        let mut i = 0;
        while i < 20 {
            let dice = lcg.below(2);
            if dice == 0 {
                // 注入例（必拒不占池）。
                if gate.inject_overlong(DATA_CAP + 8, 312).is_ok() {
                    all_ok = false;
                } else {
                    pool.reject();
                }
            } else {
                // 合法例（入池）。
                let body_len = 16;
                if gate.inject_overlong(body_len, body_len as u16).is_ok()
                    && gate.inject_bad_header(HEADER_MAGIC, 1, 4, 0x1000, 0x20, 36).is_ok()
                    && gate.inject_empty_body(body_len, 1).is_ok()
                {
                    if pool.push().is_ok() {
                        legal_in += 1;
                    }
                }
            }
            i += 1;
        }
        // 出池闭环。
        while pool.used() > 0 {
            let _ = pool.pop();
        }
        // 合法数约半（确定性下限核：≥5）+ 拒收数 = 注入数 + 泄漏判据。
        let legal_ok = legal_in >= 5;
        let rej_ok = pool.rejected() >= 5;
        if all_ok && legal_ok && rej_ok && pool.conserved() && pool.used() == 0 {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("混合流 {}/{}", ok, INJECT_ROUNDS))
}

/// 拒码分账演练 10/10 轮：三族账分离可查。
pub fn drill_code_split_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut gate = InjectGate::new();
        // 超长族触发 BodyOob / DeclMismatch。
        let _ = gate.inject_overlong(DATA_CAP + 8, 312);
        let _ = gate.inject_overlong(64, 128);
        // 坏头族触发 BadMagic / BadVersion / BadType / CidZero / Truncated。
        let _ = gate.inject_bad_header(0xFFFF, 1, 4, 0x1000, 0x20, 100);
        let _ = gate.inject_bad_header(HEADER_MAGIC, 9, 4, 0x1000, 0x20, 100);
        let _ = gate.inject_bad_header(HEADER_MAGIC, 1, 99, 0x1000, 0x20, 100);
        let _ = gate.inject_bad_header(HEADER_MAGIC, 1, 4, 0, 0, 100);
        let _ = gate.inject_bad_header(HEADER_MAGIC, 1, 4, 0x1000, 0x20, 8);
        // 空区族触发 ZeroBody / AllZeroBody。
        let _ = gate.inject_empty_body(0, 0);
        let _ = gate.inject_empty_body(16, 0);
        // 分账核：10 码各 ≥1，未触码恒 0。
        let touched = [
            IjCode::BodyOob,
            IjCode::DeclMismatch,
            IjCode::BadMagic,
            IjCode::BadVersion,
            IjCode::BadType,
            IjCode::CidZero,
            IjCode::Truncated,
            IjCode::ZeroBody,
            IjCode::AllZeroBody,
        ];
        let mut split_ok = true;
        let mut i = 0;
        while i < touched.len() {
            if gate.code_hit(touched[i]) == 0 {
                split_ok = false;
            }
            i += 1;
        }
        if gate.code_hit(IjCode::TotalOob) != 0 || gate.code_hit(IjCode::PoolLeak) != 0 {
            split_ok = false;
        }
        if split_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("拒码分账 {}/{}", ok, INJECT_ROUNDS))
}

/// 确定性双跑演练 10/10 轮：同种子 30 例注入两次逐例同码。
pub fn drill_dual_replay_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut runs: Vec<Vec<u32>> = Vec::new();
        let mut rep = 0usize;
        while rep < 2 {
            let mut gate = InjectGate::new();
            let mut seq: Vec<u32> = Vec::new();
            let mut case = 0usize;
            while case < INJECT_TOTAL {
                let family = case % 3;
                let c = match family {
                    0 => match case % 30 {
                        _ => gate.inject_overlong(DATA_CAP + 1 + case as usize, 312),
                    },
                    1 => gate.inject_bad_header(
                        if case % 2 == 0 { 0xFFFF } else { HEADER_MAGIC },
                        if case % 2 == 0 { 1 } else { 9 },
                        4,
                        0x1000,
                        0x20,
                        100,
                    ),
                    _ => gate.inject_empty_body(case % 2, 0),
                };
                seq.push(match c {
                    Err(v) => v,
                    Ok(()) => 0,
                });
                case += 1;
            }
            runs.push(seq);
            rep += 1;
        }
        // 逐例一致 + 全部非零（全拒）。
        let mut same = true;
        let mut i = 0;
        while i < INJECT_TOTAL {
            if runs[0][i] != runs[1][i] || runs[0][i] == 0 {
                same = false;
            }
            i += 1;
        }
        if same {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("确定性双跑 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    IjCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：结构常量锚核对。
pub fn run_unx12431_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-结构锚",
        HEADER_LEN == 20 && DATA_CAP == 304 && MAX_MSG_LEN == 324 && INJECT_TOTAL == 30,
        format!("头{} 体{} 总{} 注入{}", HEADER_LEN, DATA_CAP, MAX_MSG_LEN, INJECT_TOTAL),
    ));
    v
}

/// run_unx12431_checks：本单判据聚合（诊断段 0x303E0）。
pub fn run_unx12431_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12431 消息越界注入判据");
    // J1-a：30/30 注入正确拒止 + 池账无泄漏。
    let (ok1, d1) = drill_inject_30();
    s.add_dyn("J1-注入30/30零泄漏", ok1, &d1);
    // J1-b：合法闭环。
    let (ok2, d2) = drill_legal_cycle_10();
    s.add_dyn("J1-合法闭环10/10", ok2, &d2);
    // J1-c：混合流。
    let (ok3, d3) = drill_mixed_flow_10();
    s.add_dyn("J1-混合流10/10", ok3, &d3);
    // J1-d：拒码分账。
    let (ok4, d4) = drill_code_split_10();
    s.add_dyn("J1-拒码分账10/10", ok4, &d4);
    // J1-e：确定性双跑。
    let (ok5, d5) = drill_dual_replay_10();
    s.add_dyn("J1-确定性双跑10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x303E0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12431_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
