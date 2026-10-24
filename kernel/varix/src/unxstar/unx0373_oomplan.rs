//! unx0373_oomplan —— UNX-F0373 早年 OOM 预案（关键池保底）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0373 · OOM 预案 · 目标 240 行
//! 判据原文：UNX-F0373-J1 早年内存枯竭预案：页帧池水位分级
//!           （绿/黄/红三级，阈值千分制）；黄级 = 收紧（非关键
//!           分配拒）；红级 = 关键池保底（关键类分配走保底配额，
//!           非关键全拒）；水位跨级入账（降级与回升双向可查）。
//! 组件：水位分级器（阈值 + 迟滞）+ 关键分类闸（保底配额）+
//!       等级账（跨级事件 + 回升迟滞）。
//! 职责定位：OOM 预案是「枯竭时的交通规则」——枯竭不是匀质
//!   的：页表/中断栈这类关键分配不能死，大缓冲这类非关键分
//!   配必须先死。保底配额是关键分配的生命线；迟滞（回升阈低
//!   于降级阈）防止水位在阈值附近抖动导致等级风暴。
//! 性能逐项分解：分级 O(1)；分类闸 O(1)；等级账 O(事件数)。
//! 诊断段基址：0x209C0（0x209C1~0x209C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const OP_VERSION: &str = "unx0373_oomplan-v1";

/// 诊断段基址（0x209C0 段 · 本模块独占）。
pub const OP_SEG_BASE: u32 = 0x209C0;
pub const XC_HYST_BAD: u32 = 0x209C1; // 迟滞配置破
pub const XC_LEVEL_OOR: u32 = 0x209C2; // 等级越域
pub const XC_CLASS_OOR: u32 = 0x209C3; // 分配类越域
pub const XC_QUOTA_OUT: u32 = 0x209C4; // 保底配额耗尽
pub const XC_NONKEY_DENY: u32 = 0x209C5; // 非关键红级拒
pub const XC_EVENT_OOR: u32 = 0x209C6; // 事件账越域
pub const XC_MILLI_OOR: u32 = 0x209C7; // 水位千分越域
pub const XC_NO_EVENT: u32 = 0x209C8; // 无跨级谎报
pub const XC_POOL_ZERO: u32 = 0x209C9; // 零容量池

/// 水位等级。
pub const L_GREEN: u8 = 0;
pub const L_YELLOW: u8 = 1;
pub const L_RED: u8 = 2;

/// 分配类。
pub const C_CRITICAL: u8 = 1; // 关键（页表/中断栈）
pub const C_NORMAL: u8 = 2; // 普通
pub const C_BULK: u8 = 3; // 大缓冲（非关键）

/// OOM 预案器。
pub struct OomPlan {
    pool_pages: u32,
    /// 降级阈（用量千分）：≥yellow_at → 黄；≥red_at → 红。
    yellow_at: u32,
    red_at: u32,
    /// 回升阈（黄→绿需用量 < back_at）。
    back_at: u32,
    level: u8,
    /// 关键保底配额（红级下关键分配的最大页数）。
    quota: u32,
    quota_left: u32,
    /// 等级事件账：(旧级, 新级, 水位)。
    events: Vec<(u8, u8, u32)>,
}

impl OomPlan {
    pub fn new(pool_pages: u32, yellow_at: u32, red_at: u32, back_at: u32, quota: u32) -> Result<OomPlan, u32> {
        if pool_pages == 0 {
            return Err(XC_POOL_ZERO);
        }
        // 迟滞闭合：back_at < yellow_at ≤ red_at ≤ 1000。
        if back_at >= yellow_at || yellow_at > red_at || red_at > 1000 {
            return Err(XC_HYST_BAD);
        }
        Ok(OomPlan {
            pool_pages,
            yellow_at,
            red_at,
            back_at,
            level: L_GREEN,
            quota,
            quota_left: quota,
            events: Vec::new(),
        })
    }

    fn usage_milli(&self, used: u32) -> u32 {
        ((used as u64) * 1000 / self.pool_pages as u64) as u32
    }

    /// 水位评估（双向迟滞）：降级即时，回升需 < back_at。
    pub fn evaluate(&mut self, used: u32) -> Result<u8, u32> {
        let milli = self.usage_milli(used);
        if milli > 1000 {
            return Err(XC_MILLI_OOR);
        }
        let old = self.level;
        let new = match self.level {
            L_GREEN if milli >= self.red_at => L_RED,
            L_GREEN if milli >= self.yellow_at => L_YELLOW,
            L_YELLOW if milli >= self.red_at => L_RED,
            L_YELLOW if milli < self.back_at => L_GREEN,
            L_RED if milli < self.back_at => L_GREEN,
            _ => self.level,
        };
        if new != old {
            if self.events.len() >= 64 {
                return Err(XC_EVENT_OOR);
            }
            self.events.push((old, new, milli));
            self.level = new;
            // 回升至绿：保底配额复原。
            if new == L_GREEN {
                self.quota_left = self.quota;
            }
        }
        Ok(self.level)
    }

    /// 当前级。
    pub fn level(&self) -> u8 {
        self.level
    }

    /// 分配请求闸：按级与类分流。
    pub fn request(&mut self, class: u8, pages: u32) -> Result<(), u32> {
        if !matches!(class, C_CRITICAL | C_NORMAL | C_BULK) {
            return Err(XC_CLASS_OOR);
        }
        match self.level {
            L_GREEN => Ok(()),
            L_YELLOW => {
                if class == C_BULK {
                    Err(XC_NONKEY_DENY) // 黄级先掐大缓冲
                } else {
                    Ok(())
                }
            }
            L_RED => {
                if class != C_CRITICAL {
                    Err(XC_NONKEY_DENY)
                } else if self.quota_left >= pages {
                    self.quota_left -= pages;
                    Ok(())
                } else {
                    Err(XC_QUOTA_OUT)
                }
            }
            _ => Err(XC_LEVEL_OOR),
        }
    }

    /// 配额余量。
    pub fn quota_left(&self) -> u32 {
        self.quota_left
    }

    /// 跨级事件账。
    pub fn events(&self) -> &[(u8, u8, u32)] {
        &self.events
    }

    /// 无跨级谎报核：查事件必须真实发生过。
    pub fn event_at(&self, i: usize) -> Result<(u8, u8, u32), u32> {
        if i >= self.events.len() {
            return Err(XC_NO_EVENT);
        }
        Ok(self.events[i])
    }
}

pub fn run_unx0373_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0373_oomplan");
    // 族一：构建与迟滞。
    let mut op = match OomPlan::new(1024, 700, 900, 500, 32) {
        Ok(p) => p,
        Err(_) => {
            cs.add("预案构建", false, "被拒");
            return cs;
        }
    };
    cs.add("初绿", op.level() == L_GREEN, "低水位");
    cs.add("迟滞破拒", OomPlan::new(1024, 500, 900, 700, 8).is_err(), "back≥yellow");
    cs.add("级序破拒", OomPlan::new(1024, 900, 700, 500, 8).is_err(), "yellow>red");
    cs.add("零池拒", OomPlan::new(0, 700, 900, 500, 8).is_err(), "POOL");
    // 族二：降级。
    let _ = op.evaluate(760); // 742‰ → 黄
    cs.add("黄级", op.level() == L_YELLOW, "≥700");
    cs.add("黄掐大缓冲", op.request(C_BULK, 8) == Err(XC_NONKEY_DENY), "BULK 拒");
    cs.add("黄放关键", op.request(C_CRITICAL, 2).is_ok(), "关键过");
    cs.add("黄放普通", op.request(C_NORMAL, 2).is_ok(), "普通过");
    let _ = op.evaluate(930); // 908‰ → 红
    cs.add("红级", op.level() == L_RED, "≥900");
    // 族三：红级保底。
    cs.add("红掐普通", op.request(C_NORMAL, 1) == Err(XC_NONKEY_DENY), "非关键拒");
    cs.add("红保底放行", op.request(C_CRITICAL, 12).is_ok(), "配额 32→20");
    cs.add("配额递减", op.quota_left() == 20, "12 页已扣");
    cs.add("配额耗尽拒", op.request(C_CRITICAL, 21) == Err(XC_QUOTA_OUT), "超余量");
    cs.add("配额内再放", op.request(C_CRITICAL, 20).is_ok(), "20→0");
    cs.add("配额空拒", op.request(C_CRITICAL, 1) == Err(XC_QUOTA_OUT), "QUOTA_OUT");
    // 族四：回升迟滞。
    let _ = op.evaluate(600); // 585‰ ≥ back(500) → 仍红
    cs.add("迟滞不升", op.level() == L_RED, "585>500");
    let _ = op.evaluate(400); // 390‰ < 500 → 绿
    cs.add("回升绿", op.level() == L_GREEN, "<500");
    cs.add("配额复原", op.quota_left() == 32, "回升复位");
    // 族五：事件账。
    let evs = op.events();
    cs.add("事件三笔", evs.len() == 3, "绿→黄→红→绿");
    cs.add("事件序", matches!(op.event_at(0), Ok((L_GREEN, L_YELLOW, _))), "首跨黄");
    cs.add("事件越位拒", op.event_at(9).is_err(), "NO_EVENT");
    // 族六：分类闸直查。
    cs.add("类越域拒", op.request(9, 1).is_err(), "CLASS_OOR");
    cs.add("绿级全放", op.request(C_BULK, 64).is_ok(), "绿无闸");
    cs
}
