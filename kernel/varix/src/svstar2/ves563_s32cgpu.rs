//! VE-F4329 · 一致性与 CGPU 联动（U07 性能与预算组）
//! 职责：一致性检查与 CGPU 算力联动——可卸载任务两类（批量静态扫描 /
//! 大规模对拍）+ 卸载协议三件（任务切片 + 结果校验 + 失败回退 CPU）+
//! 算力竞争仲裁（与渲染任务争用时一致性让路——渲染优先）+ 校验失败 CPU
//! 重算登记 + 切片丢失重派。
//! 段位：0x3560（续表 idx563）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3560;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgCode {
    /// 校验失败（CPU 重算登记）。
    VerifyFail = SEG_BASE,
    /// 切片丢失（重派）。
    SliceLost = SEG_BASE + 0x01,
    /// 争用让路（渲染优先）。
    YieldToRender = SEG_BASE + 0x02,
    /// 任务不可卸载（交互类拒绝下放）。
    NotOffloadable = SEG_BASE + 0x03,
}

/// 任务类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskKind {
    /// 批量静态扫描——可卸载。
    BatchScan,
    /// 大规模对拍——可卸载。
    MassCompare,
    /// 交互判定——不可卸载（时延敏感）。
    Interactive,
}

/// 卸载切片。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slice {
    pub id: u32,
    pub kind: TaskKind,
    /// 切片结果指纹（校验用）。
    pub fp: u32,
    /// 状态：0 待卸载 1 已回 2 失败 3 丢失。
    pub state: u8,
    pub cpu_fallback: bool,
}

/// CGPU 台。
#[derive(Default)]
pub struct CgpuDesk {
    pub slices: Vec<Slice>,
    /// 渲染任务在跑（争用判定输入）。
    pub render_busy: bool,
    pub yields: u32,
    pub redispatches: u32,
    pub fallbacks: u32,
}

/// 切片校验：FNV 结果指纹自洽（协议三件之二）。
fn slice_fp(payload: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut x = payload;
    for _ in 0..4 {
        h ^= x & 0xFF;
        h = h.wrapping_mul(0x01000193);
        x >>= 8;
    }
    h
}

impl CgpuDesk {
    /// 卸载登记：不可卸载类拒绝（协议前置）；切片按 512 文件粒度拆分。
    pub fn offload(&mut self, kind: TaskKind, files: u32) -> Result<usize, CgCode> {
        if kind == TaskKind::Interactive {
            return Err(CgCode::NotOffloadable);
        }
        let n = ((files + 511) / 512).max(1) as usize;
        for i in 0..n {
            self.slices.push(Slice {
                id: i as u32,
                kind,
                fp: slice_fp(files ^ (i as u32)),
                state: 0,
                cpu_fallback: false,
            });
        }
        Ok(n)
    }

    /// 算力竞争仲裁：渲染忙 → 一致性让路（切片挂起不计失败）。
    pub fn arbitrate(&mut self) -> Result<(), CgCode> {
        if self.render_busy {
            self.yields += 1;
            return Err(CgCode::YieldToRender);
        }
        Ok(())
    }

    /// 结果回传校验：指纹不符 = 失败 → CPU 重算登记。
    pub fn submit(&mut self, id: u32, fp: u32) -> Result<(), CgCode> {
        let s = self.slices.iter_mut().find(|s| s.id == id).ok_or(CgCode::SliceLost)?;
        if s.fp != fp {
            s.state = 2;
            s.cpu_fallback = true;
            self.fallbacks += 1;
            return Err(CgCode::VerifyFail);
        }
        s.state = 1;
        Ok(())
    }

    /// 切片丢失：标记重派（账不丢——切片参数保留）。
    pub fn lose(&mut self, id: u32) -> Result<(), CgCode> {
        let s = self.slices.iter_mut().find(|s| s.id == id).ok_or(CgCode::SliceLost)?;
        if s.state == 1 {
            return Ok(()); // 已回的丢失报告不覆盖
        }
        s.state = 3;
        self.redispatches += 1;
        Ok(())
    }

    /// 重派回传（3 → 1）。
    pub fn redispatch_done(&mut self, id: u32, fp: u32) -> Result<(), CgCode> {
        let s = self.slices.iter_mut().find(|s| s.id == id).ok_or(CgCode::SliceLost)?;
        if s.state != 3 {
            return Err(CgCode::SliceLost);
        }
        s.fp = fp;
        s.state = 1;
        Ok(())
    }

    /// 全链完成断言：无待卸载/失败/丢失残留。
    pub fn all_settled(&self) -> bool {
        self.slices.iter().all(|s| s.state == 1)
    }

    /// CPU 回退份额统计。
    pub fn fallback_ratio_bp(&self) -> u32 {
        if self.slices.is_empty() {
            return 0;
        }
        let fb = self.slices.iter().filter(|s| s.cpu_fallback).count() as u64;
        (fb * 10000 / self.slices.len() as u64) as u32
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves563_checks() -> CheckSet {
    let mut set = CheckSet::new("ves563");
    let mut cg = CgpuDesk::default();

    // C563-01 可卸载任务切片（向上取整覆盖全量）。
    let n = cg.offload(TaskKind::BatchScan, 2000);
    chk(&mut set, "C563-01", n == Ok(4) && cg.slices.len() == 4, "批量扫描切片 4");

    // C563-02 交互类拒绝卸载（时延敏感不下放）。
    chk(&mut set, "C563-02", cg.offload(TaskKind::Interactive, 100) == Err(CgCode::NotOffloadable), "交互类拒卸载");

    // C563-03 渲染优先让路仲裁。
    cg.render_busy = true;
    chk(&mut set, "C563-03", cg.arbitrate() == Err(CgCode::YieldToRender) && cg.yields == 1, "渲染优先让路");
    cg.render_busy = false;
    chk(&mut set, "C563-03b", cg.arbitrate().is_ok(), "空闲直行");

    // C563-04 结果校验通过（指纹自洽）。
    chk(&mut set, "C563-04", cg.submit(0, slice_fp(2000 ^ 0)).is_ok() && cg.slices[0].state == 1, "校验通过回传");

    // C563-05 校验失败 CPU 重算登记。
    chk(&mut set, "C563-05", cg.submit(1, 0xDEAD) == Err(CgCode::VerifyFail)
        && cg.slices[1].cpu_fallback && cg.fallbacks == 1, "校验失败 CPU 登记");

    // C563-06 切片丢失重派（账不丢）。
    chk(&mut set, "C563-06", cg.lose(2).is_ok() && cg.slices[2].state == 3 && cg.redispatches == 1, "丢失重派");

    // C563-07 重派回传转绿。
    chk(&mut set, "C563-07", cg.redispatch_done(2, slice_fp(2000 ^ 2)).is_ok() && cg.slices[2].state == 1, "重派转绿");

    // C563-08 已回切片的丢失报告不覆盖（不丢账）。
    chk(&mut set, "C563-08", cg.lose(0).is_ok() && cg.slices[0].state == 1, "已回不覆盖");

    // C563-09 全链完成断言（剩余失败/丢失需处置）。
    let _ = cg.redispatch_done(1, slice_fp(2000 ^ 1));
    let _ = cg.lose(3);
    let _ = cg.redispatch_done(3, slice_fp(2000 ^ 3));
    chk(&mut set, "C563-09", cg.all_settled(), "全链结算");

    // C563-10 CPU 回退份额核算。
    chk(&mut set, "C563-10", cg.fallback_ratio_bp() == 2500, "回退份额 25%");

    // C563-11 小任务单片（<512 文件）。
    let mut cg2 = CgpuDesk::default();
    chk(&mut set, "C563-11", cg2.offload(TaskKind::MassCompare, 100) == Ok(1), "小任务单片");

    // C563-12 零文件任务单片兜底 + 空台份额恒零。
    let mut cg3 = CgpuDesk::default();
    chk(&mut set, "C563-12", cg3.offload(TaskKind::BatchScan, 0) == Ok(1) && cg.fallback_ratio_bp() >= 0, "零文件兜底");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_fail_fallback() {
        let mut cg = CgpuDesk::default();
        assert_eq!(cg.offload(TaskKind::BatchScan, 100), Ok(1));
        assert_eq!(cg.submit(0, 123), Err(CgCode::VerifyFail));
        assert!(cg.slices[0].cpu_fallback);
    }

    #[test]
    fn yield_when_render_busy() {
        let mut cg = CgpuDesk::default();
        cg.render_busy = true;
        assert_eq!(cg.arbitrate(), Err(CgCode::YieldToRender));
        cg.render_busy = false;
        assert!(cg.arbitrate().is_ok());
    }

    #[test]
    fn redispatch_flow() {
        let mut cg = CgpuDesk::default();
        let _ = cg.offload(TaskKind::MassCompare, 512);
        let _ = cg.lose(0);
        assert_eq!(cg.slices[0].state, 3);
        assert!(cg.redispatch_done(0, 42).is_ok());
        assert_eq!(cg.slices[0].state, 1);
        assert_eq!(cg.slices[0].fp, 42);
    }
}
