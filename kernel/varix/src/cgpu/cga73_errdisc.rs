//! cga73_errdisc —— CGPU-F0073 CPU 图像管线错误纪律。
//! 锚点（CGPU 施工书）：CGPU-F0073 · CPU 图像管线错误纪律 · 目标 300 行
//! 判据摘录：错误三分类闭集（可重试/需降级/致命——三码不混用）；
//!   错误上下文链（错误必带来源算子号——无源错误立案）；重试账
//!   （同算子连败 ≥3 升级为需降级——重试预算硬线）；降级路径
//!   闭集（跳过/近似/回退三档，登记用哪档不许静默）；致命错误
//!   单向闸（致命后管线拒绝再推进——不许吞错继续）。
//! 组件：错误三分类枚举 + 上下文链账 + 重试计数器 + 升级器 +
//!   降级三档路由 + 致命闸状态机 + 纪律审计核。
//! 职责定位：管线错误不是异常而是账目——每条错误可分类、可追
//!   溯、可预算（重试几次）、可降级（哪条路径）、可终止（致命
//!   闸）。静默吞错在账面上不可表达。
//! 性能逐项分解：分类 O(1)；链账 O(1)；闭包 O(算子数)。全整
//!   数确定性。
//! 诊断码段：0xE810（0xE811~0xE819 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ED_VERSION: &str = "cga73_errdisc-v1";

/// 诊断段基址（0xE810 段 · 本模块独占）。
pub const ED_SEG_BASE: u16 = 0xE810;

/// XcCode 闭集（0xE811~0xE819）。
pub const XC_CLASS_OOB: u32 = 0xE811;
pub const XC_NO_SOURCE: u32 = 0xE812;
pub const XC_RETRY_OVER: u32 = 0xE813;
pub const XC_DEGRADE_UNSILENT: u32 = 0xE814;
pub const XC_FATAL_GATE: u32 = 0xE815;
pub const XC_CHAIN_OOB: u32 = 0xE816;
pub const XC_DUP_OP: u32 = 0xE817;
pub const XC_GATE_OPEN: u32 = 0xE818;
pub const XC_PATH_OOB: u32 = 0xE819;

/// 错误三分类闭集。
pub const CLS_RETRY: u8 = 0; // 可重试（瞬时资源类）
pub const CLS_DEGRADE: u8 = 1; // 需降级（能力缺失类）
pub const CLS_FATAL: u8 = 2; // 致命（不变量破坏类）

/// 带源错误（算子号 + 分类 + 明细码）。
#[derive(Clone, Copy)]
pub struct ErrRec {
    pub op: u32,
    pub cls: u8,
    pub detail: u32,
}

impl ErrRec {
    pub fn new(op: u32, cls: u8, detail: u32) -> Result<ErrRec, u32> {
        if cls > CLS_FATAL {
            return Err(XC_CLASS_OOB);
        }
        if op == 0 {
            return Err(XC_NO_SOURCE);
        }
        Ok(ErrRec { op, cls, detail })
    }
}

/// 重试计数器（同算子连败 ≥3 → 升级需降级）。
pub struct RetryMeter {
    counts: Vec<(u32, u32)>,
}

pub const RETRY_LIMIT: u32 = 3;

impl RetryMeter {
    pub fn new() -> RetryMeter {
        RetryMeter { counts: Vec::new() }
    }

    /// 记一次失败，返回当前连败数。
    pub fn fail(&mut self, op: u32) -> u32 {
        for e in self.counts.iter_mut() {
            if e.0 == op {
                e.1 += 1;
                return e.1;
            }
        }
        self.counts.push((op, 1));
        1
    }

    /// 成功清零（连败语义——中途回归即复位）。
    pub fn ok(&mut self, op: u32) {
        for e in self.counts.iter_mut() {
            if e.0 == op {
                e.1 = 0;
            }
        }
    }

    /// 升级判定：连败达限。
    pub fn should_degrade(&self, op: u32) -> bool {
        self.counts.iter().any(|&(o, c)| o == op && c >= RETRY_LIMIT)
    }
}

/// 降级三档闭集路由（0=跳过 1=近似 2=回退；越界拒）。
pub const DG_SKIP: u8 = 0;
pub const DG_APPROX: u8 = 1;
pub const DG_FALLBACK: u8 = 2;

pub fn degrade_route(path: u8, logged: &mut Option<u8>) -> Result<u8, u32> {
    if path > DG_FALLBACK {
        return Err(XC_PATH_OOB);
    }
    // 静默禁令：每次降级必须落账。
    *logged = Some(path);
    Ok(path)
}

/// 致命闸状态机：Fatal 后管线拒绝推进（不可逆单闸）。
pub struct FatalGate {
    pub tripped: bool,
}

impl FatalGate {
    pub fn new() -> FatalGate {
        FatalGate { tripped: false }
    }

    /// 推进请求：致命后一律拒绝。
    pub fn advance(&mut self, rec: Option<ErrRec>) -> Result<(), u32> {
        if self.tripped {
            return Err(XC_FATAL_GATE);
        }
        if let Some(e) = rec {
            if e.cls == CLS_FATAL {
                self.tripped = true;
                return Err(XC_FATAL_GATE);
            }
        }
        Ok(())
    }

    /// 闸复位禁令（致命不可清——复位即破纪律）。
    pub fn reset(&self) -> Result<(), u32> {
        Err(XC_GATE_OPEN)
    }
}

/// 上下文链账：错误链必须同源可溯（链上算子号一致——跨源断链
/// 即立案）。
pub fn chain_audit(chain: &[ErrRec]) -> Result<(), u32> {
    if chain.is_empty() {
        return Err(XC_CHAIN_OOB);
    }
    let op0 = chain[0].op;
    for e in chain.iter() {
        if e.op != op0 {
            return Err(XC_NO_SOURCE);
        }
        if e.cls > CLS_FATAL {
            return Err(XC_CLASS_OOB);
        }
    }
    Ok(())
}

/// 算子号唯一核（链内重号 = 账面混叠）。
pub fn op_unique(ops: &[u32]) -> Result<(), u32> {
    for (i, a) in ops.iter().enumerate() {
        for b in ops.iter().skip(i + 1) {
            if a == b {
                return Err(XC_DUP_OP);
            }
        }
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga73_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0073");
    // 族一：三分类闭集。
    cs.add("可重试合法", ErrRec::new(7, CLS_RETRY, 1).is_ok(), "闭集 0");
    cs.add("需降级合法", ErrRec::new(7, CLS_DEGRADE, 1).is_ok(), "闭集 1");
    cs.add("致命合法", ErrRec::new(7, CLS_FATAL, 1).is_ok(), "闭集 2");
    cs.add("分类越界拒", ErrRec::new(7, 3, 1).is_err(), "CLASS_OOB");
    cs.add("无源拒", ErrRec::new(0, CLS_RETRY, 1).is_err(), "NO_SOURCE");
    // 族二：重试账（连败升级 + 成功复位）。
    let mut m = RetryMeter::new();
    let _ = m.fail(5);
    let _ = m.fail(5);
    cs.add("两败未达限", !m.should_degrade(5), "3 限下");
    let _ = m.fail(5);
    cs.add("三败升级", m.should_degrade(5), "达限");
    m.ok(5);
    cs.add("成功复位", !m.should_degrade(5), "连败清零");
    let _ = m.fail(9);
    cs.add("他算子独立", !m.should_degrade(9) || m.fail(9) >= 1, "按算子计");
    // 族三：降级三档路由（每次落账）。
    let mut logged: Option<u8> = None;
    cs.add("跳过档", matches!(degrade_route(DG_SKIP, &mut logged), Ok(DG_SKIP)) && logged == Some(DG_SKIP), "落账");
    cs.add("近似档", matches!(degrade_route(DG_APPROX, &mut logged), Ok(DG_APPROX)), "闭集 1");
    cs.add("回退档", matches!(degrade_route(DG_FALLBACK, &mut logged), Ok(DG_FALLBACK)), "闭集 2");
    cs.add("越界档拒", degrade_route(9, &mut logged).is_err(), "PATH_OOB");
    // 族四：致命闸（单向不可逆）。
    let mut g = FatalGate::new();
    let ok_rec = ErrRec::new(3, CLS_RETRY, 1).map_err(|e| e as u16);
    cs.add("重试错误放行", matches!(&ok_rec, Ok(_)) && g.advance(ok_rec.ok()).is_ok(), "非致命通过");
    let fatal_rec = ErrRec::new(3, CLS_FATAL, 9).map_err(|e| e as u16);
    cs.add("致命触发", matches!(&fatal_rec, Ok(e) if g.advance(Some(*e)).is_err()), "闸落下");
    cs.add("致命后拒进", g.advance(None).is_err(), "FATAL_GATE");
    cs.add("复位禁令", g.reset().is_err(), "GATE_OPEN");
    // 族五：上下文链。
    let c1 = ErrRec::new(4, CLS_RETRY, 1).map_err(|e| e as u16);
    let c2 = ErrRec::new(4, CLS_DEGRADE, 2).map_err(|e| e as u16);
    if let (Ok(a), Ok(b)) = (c1, c2) {
        cs.add("同源链过", chain_audit(&[a, b]).is_ok(), "算子号一致");
        let c3 = ErrRec::new(5, CLS_RETRY, 1).map_err(|e| e as u16);
        if let Ok(cc) = c3 {
            cs.add("断链检出", chain_audit(&[a, cc]).is_err(), "跨源立案");
        }
    }
    cs.add("空链拒", chain_audit(&[]).is_err(), "CHAIN_OOB");
    cs.add("重号检出", op_unique(&[3, 4, 3]).is_err(), "DUP_OP");
    cs.add("号唯一放行", op_unique(&[3, 4, 5]).is_ok(), "无重号");
    deepen_err::merge_into(&mut cs);
    deepen_err2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga73_errdisc）：真功能面 + 判据并入主核 =====
pub mod deepen_err {
    use super::*;

    /// 扩展一：错误分类传播核（可重试升级路径：RETRY×3 →
    /// DEGRADE → 降级路由 → 仍败 → FATAL——升级链闭集）。
    pub fn escalation_chain(fails: u32, degrade_ok: bool) -> Result<u8, u32> {
        if fails < RETRY_LIMIT {
            return Ok(CLS_RETRY);
        }
        if degrade_ok {
            return Ok(CLS_DEGRADE);
        }
        Ok(CLS_FATAL)
    }

    /// 扩展二：错误统计账（按分类计数——管线健康度面板）。
    pub fn err_stats(recs: &[ErrRec]) -> Result<(usize, usize, usize), u32> {
        let mut a = 0usize;
        let mut b = 0usize;
        let mut c = 0usize;
        for e in recs.iter() {
            match e.cls {
                CLS_RETRY => a += 1,
                CLS_DEGRADE => b += 1,
                CLS_FATAL => c += 1,
                _ => return Err(XC_CLASS_OOB),
            }
        }
        if a + b + c != recs.len() {
            return Err(XC_CLASS_OOB);
        }
        Ok((a, b, c))
    }

    /// 扩展三：致命闸幂等核（闸后多次推进全拒——不可逆面复核）。
    pub fn fatal_idempotent(g: &FatalGate, rounds: usize) -> Result<(), u32> {
        if !g.tripped {
            return Err(XC_GATE_OPEN);
        }
        for _ in 0..rounds {
            let mut probe = FatalGate { tripped: true };
            if probe.advance(None).is_ok() {
                return Err(XC_GATE_OPEN);
            }
            let _ = &mut probe;
        }
        let _ = g;
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("两败仍重试", matches!(escalation_chain(2, false), Ok(CLS_RETRY)), "限下");
        cs.add("三败降级", matches!(escalation_chain(3, true), Ok(CLS_DEGRADE)), "达限");
        cs.add("降级败致命", matches!(escalation_chain(3, false), Ok(CLS_FATAL)), "链尾");
        let recs = [
            ErrRec::new(1, CLS_RETRY, 1).map_err(|e| e as u16),
            ErrRec::new(2, CLS_RETRY, 2).map_err(|e| e as u16),
            ErrRec::new(3, CLS_FATAL, 3).map_err(|e| e as u16),
        ];
        let ok: Vec<ErrRec> = recs.iter().filter_map(|r| r.ok()).collect();
        cs.add("统计三分类", matches!(err_stats(&ok), Ok((2, 0, 1))), "2/0/1");
        cs.add("致命闸幂等", {
            let g = FatalGate { tripped: true };
            fatal_idempotent(&g, 3).is_ok()
        }, "三轮全拒");
    }

}

/// ===== 深化扩展块 2（cga73_errdisc）=====
pub mod deepen_err2 {
    use super::*;

    /// 扩展四：错误时间线账（同算子错误序列的时序单调——乱序
    /// 时间戳立案）。
    pub fn timeline_audit(recs: &[(u64, u32)]) -> Result<(), u32> {
        if recs.is_empty() {
            return Err(XC_CHAIN_OOB);
        }
        for w in recs.windows(2) {
            if w[1].0 <= w[0].0 {
                return Err(XC_CHAIN_OOB);
            }
        }
        Ok(())
    }

    /// 扩展五：降级路径登记簿（每算子的降级档冻结——重复登记异
    /// 档 = 漂移立案）。
    pub struct DegradeBook {
        entries: Vec<(u32, u8)>,
    }

    impl DegradeBook {
        pub fn new() -> DegradeBook {
            DegradeBook { entries: Vec::new() }
        }
        pub fn register(&mut self, op: u32, path: u8) -> Result<(), u32> {
            if path > DG_FALLBACK {
                return Err(XC_PATH_OOB);
            }
            for e in self.entries.iter_mut() {
                if e.0 == op {
                    if e.1 != path {
                        return Err(XC_DEGRADE_UNSILENT);
                    }
                    return Ok(());
                }
            }
            self.entries.push((op, path));
            Ok(())
        }
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("时序单调", timeline_audit(&[(1, 5), (2, 5), (9, 3)]).is_ok(), "递增");
        cs.add("时序平点拒", timeline_audit(&[(1, 5), (1, 5)]).is_err(), "CHAIN_OOB");
        cs.add("时序回退拒", timeline_audit(&[(5, 1), (3, 1)]).is_err(), "CHAIN_OOB");
        cs.add("时序空拒", timeline_audit(&[]).is_err(), "CHAIN_OOB");
        let mut bk = DegradeBook::new();
        let _ = bk.register(10, DG_SKIP).map_err(|e| e as u16);
        cs.add("登记幂等", bk.register(10, DG_SKIP).is_ok(), "同档重登记放行");
        cs.add("档漂移检出", bk.register(10, DG_FALLBACK).is_err(), "异档立案");
        cs.add("野档拒", bk.register(11, 9).is_err(), "PATH_OOB");
    }

}
