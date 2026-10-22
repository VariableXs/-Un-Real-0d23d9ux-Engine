//! unx542_lvlfilter —— UNX-F0542 级别过滤账（显性丢弃）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0542 · 级别过滤账（显性丢弃）· 目标 220 行 · 已深化
//! 判据原文：低于阈值的日志条目显性丢弃并落账（零静默吞），阈值变更全程可追
//! 组件：五级闭集（DBG=0/INFO=1/WARN=2/ERR=3/FATAL=4）
//!       + 阈值闸（低于阈值丢弃，但每次丢弃必落账——显性丢弃语义）
//!       + 过滤账（append-only：级别/丢弃数/阈值快照三列）
//!       + 阈值变更账（旧值→新值逐次留痕，变更者四态闭集）
//!       + 对拍核（过滤指纹 + 注入核三类必抓）。
//! 职责定位：日志面唯一过滤权威——任何级别过滤必须过闸，杜绝静默吞条；
//!   丢弃账使"看不见的日志"可审计；阈值变更账使过滤策略全程可追。
//! 性能逐项分解：过闸 O(1)；丢弃落账 O(1) 均摊；指纹 O(账行数)。
//!   全整数确定性（LCG，零墙钟）。
//! 诊断码段：0xE280（0xE281~0xE288 闭集；空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const LF_VERSION: &str = "unx542_lvlfilter-v1";

/// 诊断段基址（0xE280 段 · 本模块独占）。
pub const LF_SEG_BASE: u16 = 0xE280;

/// XcCode 闭集（0xE281~0xE288）。
pub const XC_LVL_BAD: u32 = 0xE281;
pub const XC_THR_BAD: u32 = 0xE282;
pub const XC_SILENT_LOSS: u32 = 0xE283;
pub const XC_FINGER_DRIFT: u32 = 0xE284;
pub const XC_ACTOR_BAD: u32 = 0xE285;
pub const XC_INJECT_MISSED: u32 = 0xE286;
pub const XC_LEDGER_BROKEN: u32 = 0xE287;
pub const XC_SEALED_WRITE: u32 = 0xE288;

/// 中文诊断文本（闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_LVL_BAD => ("日志级别越界（超出五级闭集 0~4）", "级别常量必须取自五级闭集"),
        XC_THR_BAD => ("阈值越界（超出五级闭集 0~4）", "阈值同样取五级闭集"),
        XC_SILENT_LOSS => ("丢弃未落账（静默吞条违规）", "所有丢弃必须显性过账"),
        XC_FINGER_DRIFT => ("过滤指纹与冻结值不一致（账漂移）", "复核丢弃序列或重跑指纹"),
        XC_ACTOR_BAD => ("阈值变更者不在四态闭集（boot/self/api/cmd）", "变更者登记取闭集"),
        XC_INJECT_MISSED => ("注入故障未被检出（守卫失守）", "修复注入核比对逻辑后重跑"),
        XC_LEDGER_BROKEN => ("过滤账 append-only 性被破坏（序号跳号）", "检查落账路径是否绕过账务封装"),
        XC_SEALED_WRITE => ("封账后仍尝试过闸/改阈值（冻结被破坏）", "封账后只读对拍"),
        _ => ("未知诊断码", "查 0xE280 段闭集登记表"),
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

/// FNV-1a 32 位（过滤指纹）。
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

/// 五级闭集。
pub const LVL_DBG: u8 = 0;
pub const LVL_INFO: u8 = 1;
pub const LVL_WARN: u8 = 2;
pub const LVL_ERR: u8 = 3;
pub const LVL_FATAL: u8 = 4;

/// 阈值变更者四态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    Boot,
    SelfCfg,
    Api,
    Cmd,
}

impl Actor {
    pub const fn num(self) -> u8 {
        match self {
            Actor::Boot => 0,
            Actor::SelfCfg => 1,
            Actor::Api => 2,
            Actor::Cmd => 3,
        }
    }
    pub fn from_num(n: u8) -> Option<Actor> {
        match n {
            0 => Some(Actor::Boot),
            1 => Some(Actor::SelfCfg),
            2 => Some(Actor::Api),
            3 => Some(Actor::Cmd),
            _ => None,
        }
    }
}

/// 丢弃账一行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct DropRow {
    /// 被丢级别。
    pub lvl: u8,
    /// 本行聚合丢弃数。
    pub count: u64,
    /// 丢弃时阈值快照。
    pub thr: u8,
}

/// 阈值变更账一行。
#[derive(Clone, Copy, Debug)]
pub struct ThrRow {
    pub old: u8,
    pub new: u8,
    pub actor: u8,
}

/// 过滤闸错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfError {
    LvlBad(u8),
    ThrBad(u8),
    ActorBad(u8),
    Sealed,
}

/// 级别过滤账闸。
pub struct LvlFilter {
    thr: u8,
    drops: Vec<DropRow>,
    thrs: Vec<ThrRow>,
    next_seq: u32,
    passed: u64,
    frozen: u32,
    sealed: bool,
}

impl LvlFilter {
    /// 初闸（boot 语义：初始阈值须闭集内）。
    pub fn new(thr: u8) -> Result<LvlFilter, LfError> {
        if thr > LVL_FATAL {
            return Err(LfError::ThrBad(thr));
        }
        Ok(LvlFilter {
            thr,
            drops: Vec::new(),
            thrs: Vec::new(),
            next_seq: 1,
            passed: 0,
            frozen: 0,
            sealed: false,
        })
    }

    /// 常量合法阈值快捷构造（闭集外回退 WARN 默认账——仅供判据面，零 panic）。
    pub fn fresh(thr: u8) -> LvlFilter {
        match Self::new(thr) {
            Ok(f) => f,
            Err(_) => LvlFilter {
                thr: LVL_WARN,
                drops: Vec::new(),
                thrs: Vec::new(),
                next_seq: 1,
                passed: 0,
                frozen: 0,
                sealed: false,
            },
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.next_seq as usize == self.drops.len() + 1
    }

    pub fn threshold(&self) -> u8 {
        self.thr
    }

    pub fn drop_rows(&self) -> usize {
        self.drops.len()
    }

    pub fn thr_rows(&self) -> usize {
        self.thrs.len()
    }

    pub fn passed(&self) -> u64 {
        self.passed
    }

    pub fn row_at(&self, i: usize) -> Option<DropRow> {
        self.drops.get(i).copied()
    }

    pub fn thr_row_at(&self, i: usize) -> Option<ThrRow> {
        self.thrs.get(i).copied()
    }

    /// 阈值变更（逐次留痕：old→new + 变更者）。
    pub fn set_threshold(&mut self, new: u8, actor: Actor) -> Result<(), LfError> {
        if self.sealed {
            return Err(LfError::Sealed);
        }
        if new > LVL_FATAL {
            return Err(LfError::ThrBad(new));
        }
        if actor.num() > 3 {
            return Err(LfError::ActorBad(actor.num()));
        }
        self.thrs.push(ThrRow {
            old: self.thr,
            new,
            actor: actor.num(),
        });
        self.thr = new;
        Ok(())
    }

    /// 过闸：≥阈值放行；<阈值显性丢弃并落账（零静默吞）。
    /// 同级连续丢弃聚合进同行（count 累加），阈值变化后开新行。
    pub fn admit(&mut self, lvl: u8) -> Result<bool, LfError> {
        if self.sealed {
            return Err(LfError::Sealed);
        }
        if lvl > LVL_FATAL {
            return Err(LfError::LvlBad(lvl));
        }
        if lvl >= self.thr {
            self.passed += 1;
            return Ok(true);
        }
        // 显性丢弃：落账
        match self.drops.last_mut() {
            Some(r) if r.lvl == lvl && r.thr == self.thr => {
                r.count += 1;
            }
            _ => {
                let seq = self.next_seq;
                self.drops.push(DropRow {
                    lvl,
                    count: 1,
                    thr: self.thr,
                });
                self.next_seq = seq + 1;
            }
        }
        Ok(false)
    }

    /// 过滤指纹（账序确定性）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for r in &self.drops {
            buf.push(r.lvl);
            buf.extend_from_slice(&r.count.to_le_bytes());
            buf.push(r.thr);
        }
        fnv1a32(&buf)
    }

    pub fn seal(&mut self) -> u32 {
        self.frozen = self.fingerprint();
        self.sealed = true;
        self.frozen
    }

    pub fn verify_frozen(&self) -> bool {
        self.sealed && self.fingerprint() == self.frozen
    }
}

/// 注入核：三类违规 + 守卫 10/10 必抓。
pub fn inject_probe() -> Result<bool, u32> {
    let mut caught = 0u32;
    // 1) 级别越界
    let mut f = LvlFilter::fresh(LVL_INFO);
    if f.admit(5).is_err() {
        caught += 1;
    }
    // 2) 阈值越界
    if f.set_threshold(9, Actor::Api).is_err() {
        caught += 1;
    }
    // 3) 变更者闭集（非法 actor 编号必须不可构造）
    if Actor::from_num(7).is_none() {
        caught += 1;
    }
    // 4) 丢弃必落账（DBG<INFO 被丢且有账）
    if f.admit(LVL_DBG).is_ok() && f.drop_rows() == 1 {
        caught += 1;
    }
    // 5) 放行不落账
    let mut f2 = LvlFilter::fresh(LVL_WARN);
    if f2.admit(LVL_ERR).is_ok() && f2.drop_rows() == 0 {
        caught += 1;
    }
    // 6) 同级聚合（3 次 DBG 一行 count=3）
    let _ = f2.admit(LVL_DBG);
    let _ = f2.admit(LVL_DBG);
    if f2.drop_rows() == 1 && f2.row_at(0).map(|r| r.count == 3).unwrap_or(false) {
        caught += 1;
    }
    // 7) 阈值变更后开新行（thr 快照不同）
    let _ = f2.set_threshold(LVL_ERR, Actor::Cmd);
    let _ = f2.admit(LVL_INFO);
    if f2.drop_rows() == 2 && f2.row_at(1).map(|r| r.thr == LVL_ERR).unwrap_or(false) {
        caught += 1;
    }
    // 8) 变更账逐次留痕（old→new）
    if f2.thr_row_at(0).map(|r| r.old == LVL_WARN && r.new == LVL_ERR && r.actor == Actor::Cmd.num()).unwrap_or(false) {
        caught += 1;
    }
    // 9) 封账后过闸拒
    let _ = f2.seal();
    if f2.admit(LVL_FATAL).is_err() && f2.verify_frozen() {
        caught += 1;
    }
    // 10) 全程账序守恒
    if f.ledger_ok() && f2.ledger_ok() {
        caught += 1;
    }
    if caught != 10 {
        return Err(XC_INJECT_MISSED as u32);
    }
    Ok(true)
}

/// 域自检（`run_unx542_checks`）：判据逐条可失败断言。
pub fn run_unx542_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx542");

    // C542-01 低于阈值显性丢弃（有账，零静默）
    {
        let mut f = LvlFilter::fresh(LVL_INFO);
        let r = f.admit(LVL_DBG);
        let ok = r == Ok(false) && f.drop_rows() == 1;
        s.add("C542-01-丢弃必落账", ok, if ok { "" } else { "静默吞条" });
    }

    // C542-02 达阈值放行且不落账
    {
        let mut f = LvlFilter::fresh(LVL_WARN);
        let r = f.admit(LVL_WARN);
        let ok = r == Ok(true) && f.drop_rows() == 0 && f.passed() == 1;
        s.add("C542-02-达线放行无账", ok, if ok { "" } else { "放行误落账" });
    }

    // C542-03 同级聚合语义
    {
        let mut f = LvlFilter::fresh(LVL_INFO);
        for _ in 0..5 {
            let _ = f.admit(LVL_DBG);
        }
        let ok = f.drop_rows() == 1
            && f.row_at(0).map(|r| r.count == 5 && r.lvl == LVL_DBG).unwrap_or(false);
        s.add("C542-03-同级丢弃聚合", ok, if ok { "" } else { "聚合失守" });
    }

    // C542-04 阈值变更全程可追（变更账 + 新行快照）
    {
        let mut f = LvlFilter::fresh(LVL_DBG);
        let _ = f.set_threshold(LVL_ERR, Actor::Api);
        let _ = f.admit(LVL_WARN);
        let ok = f.thr_rows() == 1
            && f.thr_row_at(0).map(|r| r.old == LVL_DBG && r.new == LVL_ERR && r.actor == Actor::Api.num()).unwrap_or(false)
            && f.row_at(0).map(|r| r.thr == LVL_ERR).unwrap_or(false);
        s.add("C542-04-阈值变更可追", ok, if ok { "" } else { "变更失追" });
    }

    // C542-05 级别闭集与阈值闭集双重校验
    {
        let mut f = LvlFilter::fresh(LVL_INFO);
        let r1 = f.admit(7);
        let r2 = f.set_threshold(255, Actor::Cmd);
        let ok = matches!(r1, Err(LfError::LvlBad(7))) && matches!(r2, Err(LfError::ThrBad(255)));
        s.add("C542-05-闭集双校验", ok, if ok { "" } else { "闭集失守" });
    }

    // C542-06 指纹漂移检出
    {
        let mut f = LvlFilter::fresh(LVL_INFO);
        let _ = f.admit(LVL_DBG);
        let f1 = f.fingerprint();
        let _ = f.admit(LVL_DBG);
        let f2 = f.fingerprint();
        let ok = f1 != f2;
        s.add("C542-06-指纹漂移可检", ok, if ok { "" } else { "指纹锚失守" });
    }

    // C542-07 注入 10/10 全抓
    {
        let ok = inject_probe() == Ok(true);
        s.add("C542-07-注入全抓", ok, if ok { "" } else { "注入核漏检" });
    }

    s
}
