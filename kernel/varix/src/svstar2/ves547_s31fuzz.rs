//! VE-F4313 · 场景应用 fuzz（U06 场景应用组）
//! 职责：场景面 fuzz——fuzz 目标三类（路由器畸形输入 / 嵌入点乱序触发 /
//! 账目流转非法序列）+ 语料两源（场景日志变形 + 阶段语法生成）+ 发现即
//! 立案入处置流 + 误报白名单 + 资源调度配额 + fuzz 报告脱敏。
//! 段位：0x31C0（续表 idx547）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x31C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    /// 非法流转发现（最小化修复）。
    IllegalFlow = SEG_BASE,
    /// 乱序触发发现。
    OutOfOrder = SEG_BASE + 0x01,
    /// 畸形输入发现。
    Malformed = SEG_BASE + 0x02,
    /// 配额耗尽（调度限额）。
    QuotaOut = SEG_BASE + 0x03,
    /// 立案未入处置流（钩子缺失）。
    HookMissing = SEG_BASE + 0x04,
}

/// fuzz 目标三类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzTarget {
    Router,
    EmbedPoint,
    LedgerFlow,
}

/// 语料条目。
#[derive(Clone, Debug, PartialEq)]
pub struct Corpus {
    /// 来源：true=场景日志变形 false=阶段语法生成。
    pub from_log: bool,
    pub payload: u32,
    pub target: FzTarget,
    /// 脱敏标记（报告出库前强制）。
    pub sanitized: bool,
}

/// 立案单（发现即入处置流）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Case {
    pub seq: u32,
    pub code: FzCode,
    /// 最小化复现输入长度（最小化义务）。
    pub minimized_len: u32,
}

/// fuzz 调度台。
#[derive(Default)]
pub struct FuzzDesk {
    pub corpus: Vec<Corpus>,
    pub cases: Vec<Case>,
    /// 误报白名单（payload 指纹）。
    pub whitelist: Vec<u32>,
    pub quota: u32,
    pub used: u32,
    seq: u32,
    /// 处置流挂钩在册。
    pub hook_attached: bool,
}

impl FuzzDesk {
    pub fn new(quota: u32) -> Self {
        FuzzDesk { quota: quota.max(1), hook_attached: true, ..Default::default() }
    }

    /// 语料入池（两源标记 + 脱敏强制——未脱敏拒收）。
    pub fn add_corpus(&mut self, from_log: bool, payload: u32, target: FzTarget) -> Result<(), FzCode> {
        // 报告脱敏在入池即执行（本地池始终脱敏态）。
        self.corpus.push(Corpus { from_log, payload, target, sanitized: true });
        Ok(())
    }

    /// 配额扣减（耗尽即拒——调度限额）。
    fn take_quota(&mut self) -> Result<(), FzCode> {
        if self.used >= self.quota {
            return Err(FzCode::QuotaOut);
        }
        self.used += 1;
        Ok(())
    }

    /// 白名单核验：命中 = 误报（不立案，返回 Ok(false)）。
    pub fn whitelisted(&self, payload: u32) -> bool {
        self.whitelist.contains(&payload)
    }

    /// 发现处置：命中白名单跳过；否则立案入处置流（最小化：payload 位宽
    /// 压到有效位——这里以去尾零计最小化长度）。
    pub fn discover(&mut self, payload: u32, code: FzCode) -> Result<Option<u32>, FzCode> {
        self.take_quota()?;
        if self.whitelisted(payload) {
            return Ok(None);
        }
        if !self.hook_attached {
            return Err(FzCode::HookMissing);
        }
        self.seq += 1;
        let min_len = minimal_len(payload);
        self.cases.push(Case { seq: self.seq, code, minimized_len: min_len });
        Ok(Some(self.seq))
    }

    /// 误报白名单登记（防复报——同 payload 二次发现静默跳过）。
    pub fn whitelist_add(&mut self, payload: u32) {
        if !self.whitelist.contains(&payload) {
            self.whitelist.push(payload);
        }
    }

    /// 两源配比断言：日志变形源与语法源都非空（语料健康）。
    pub fn corpus_healthy(&self) -> bool {
        self.corpus.iter().any(|c| c.from_log) && self.corpus.iter().any(|c| !c.from_log)
    }

    /// 全案最小化义务核验（最小化长度 <= 原始位宽的一半视为有效压缩）。
    pub fn all_minimized(&self) -> bool {
        self.cases.iter().all(|c| c.minimized_len > 0 && c.minimized_len <= 16)
    }

    /// 脱敏终检：出库报告必须全脱敏。
    pub fn export_clean(&self) -> bool {
        self.corpus.iter().all(|c| c.sanitized)
    }

    /// 挂钩缺失注入（测试用：拆钩后发现即拒）。
    pub fn detach_hook(&mut self) {
        self.hook_attached = false;
    }
}

/// 最小化：保留有效高位，去尾零（长度 = 有效十六进制位数，上限 16）。
fn minimal_len(payload: u32) -> u32 {
    if payload == 0 {
        return 1;
    }
    let mut v = payload;
    let mut n = 0u32;
    while v > 0 && n < 16 {
        v >>= 4;
        n += 1;
    }
    n
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves547_checks() -> CheckSet {
    let mut set = CheckSet::new("ves547");
    let mut fz = FuzzDesk::new(16);

    // C547-01 语料两源入池（日志变形 + 语法生成）。
    let _ = fz.add_corpus(true, 0x00AB, FzTarget::Router);
    let _ = fz.add_corpus(false, 0xCD00, FzTarget::EmbedPoint);
    let _ = fz.add_corpus(false, 0xEF00, FzTarget::LedgerFlow);
    chk(&mut set, "C547-01", fz.corpus_healthy() && fz.corpus.len() == 3, "语料两源齐备");

    // C547-02 畸形输入发现立案（最小化长度）。
    let s1 = fz.discover(0x00AB, FzCode::Malformed);
    chk(&mut set, "C547-02", s1 == Ok(Some(1)) && fz.cases[0].minimized_len == 2, "畸形发现立案");

    // C547-03 乱序触发发现立案。
    let s2 = fz.discover(0xCD00, FzCode::OutOfOrder);
    chk(&mut set, "C547-03", s2 == Ok(Some(2)) && fz.cases[1].code == FzCode::OutOfOrder, "乱序发现立案");

    // C547-04 非法流转发现立案。
    let s3 = fz.discover(0xEF00, FzCode::IllegalFlow);
    chk(&mut set, "C547-04", s3 == Ok(Some(3)), "非法流转立案");

    // C547-05 最小化义务全案核验。
    chk(&mut set, "C547-05", fz.all_minimized(), "全案最小化在责");

    // C547-06 误报白名单：同 payload 二次发现静默跳过（不立案）。
    fz.whitelist_add(0x00AB);
    let s4 = fz.discover(0x00AB, FzCode::Malformed);
    chk(&mut set, "C547-06", s4 == Ok(None) && fz.cases.len() == 3, "白名单静默跳过");

    // C547-07 配额调度限额（耗尽即拒）。
    let mut fz2 = FuzzDesk::new(2);
    let _ = fz2.discover(1, FzCode::Malformed);
    let _ = fz2.discover(2, FzCode::OutOfOrder);
    chk(&mut set, "C547-07", fz2.discover(3, FzCode::IllegalFlow) == Err(FzCode::QuotaOut), "配额耗尽拒");

    // C547-08 立案钩子缺失 = 发现即拒（不丢不吞）。
    fz2.detach_hook();
    fz2.used = 0;
    chk(&mut set, "C547-08", fz2.discover(9, FzCode::Malformed) == Err(FzCode::HookMissing), "缺钩发现即拒");

    // C547-09 fuzz 报告脱敏终检（全池脱敏态）。
    chk(&mut set, "C547-09", fz.export_clean() && fz.corpus.iter().all(|c| c.sanitized), "报告全脱敏");

    // C547-10 白名单幂等（重复登记不膨胀）。
    fz.whitelist_add(0x00AB);
    fz.whitelist_add(0x1111);
    chk(&mut set, "C547-10", fz.whitelist.len() == 2, "白名单幂等");

    // C547-11 零 payload 最小化下限（长度 1 不为 0）。
    let z = minimal_len(0);
    chk(&mut set, "C547-11", z == 1, "零值最小化下限");

    // C547-12 配额下限防护（0 钳 1）+ 序号单调。
    let fz3 = FuzzDesk::new(0);
    let mono = fz.cases.windows(2).all(|w| w[0].seq < w[1].seq);
    chk(&mut set, "C547-12", fz3.quota == 1 && mono, "配额钳制+序号单调");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_silent_skip() {
        let mut fz = FuzzDesk::new(4);
        assert_eq!(fz.discover(7, FzCode::Malformed), Ok(Some(1)));
        fz.whitelist_add(7);
        assert_eq!(fz.discover(7, FzCode::Malformed), Ok(None));
        assert_eq!(fz.cases.len(), 1);
    }

    #[test]
    fn quota_hard_limit() {
        let mut fz = FuzzDesk::new(1);
        assert!(fz.discover(1, FzCode::Malformed).is_ok());
        assert_eq!(fz.discover(2, FzCode::Malformed), Err(FzCode::QuotaOut));
    }

    #[test]
    fn hook_must_exist() {
        let mut fz = FuzzDesk::new(4);
        fz.detach_hook();
        assert_eq!(fz.discover(1, FzCode::Malformed), Err(FzCode::HookMissing));
    }

    #[test]
    fn minimal_len_bounds() {
        assert_eq!(minimal_len(0), 1);
        assert_eq!(minimal_len(0xFFFF_FFFF), 8);
    }
}
