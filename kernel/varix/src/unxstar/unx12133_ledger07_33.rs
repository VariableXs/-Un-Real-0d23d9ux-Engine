//! UNX-F12133 — 损坏 .pdata/.xdata 拒止与红账核
//! 域: UNX-D1 (Windows NT 内核语义模型) · B07 批 · 展开码族
//! 语义: 装载期对 .pdata 函数表与 .xdata 展开信息做完整性拒止：
//!       ① SIZE — RUNTIME_FUNCTION 三元组不足 12 字节（表长非 12 倍数）；
//!       ② UNSORTED — 表项 BeginAddress 不按序（二分查找前提被破坏）；
//!       ③ OOB — 函数区间 [Begin,End) 越出镜像边界；
//!       ④ UNWIND_OOB — UnwindInfo RVA 越出 .xdata 边界或非 4 对齐；
//!       ⑤ FROZEN — 红账 seal 后仍受理新红条。
//!       每次拒止写入 append-only 红账（RedLedger），红条只增不删；
//!       表通过全部四道闸则入绿账。折叠指纹覆盖红/绿两账。
//! 载荷: 红账 Vec<(code, detail)> + 绿账 Vec<RtOk> + 折叠指纹 + seal。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a2c0;

pub const RD_SIZE: u32 = SEG_BASE + 0x01; // 表长非 12 倍数
pub const RD_UNSORTED: u32 = SEG_BASE + 0x02; // BeginAddress 乱序
pub const RD_OOB: u32 = SEG_BASE + 0x03; // 函数区间越出镜像
pub const RD_UNWIND_OOB: u32 = SEG_BASE + 0x04; // UnwindInfo 越界/不对齐
pub const RD_FROZEN: u32 = SEG_BASE + 0x05; // 红账冻结仍受理

/// RUNTIME_FUNCTION 三元组宽度。
pub const RT_STRIDE: u32 = 12;

/// 通过四道闸的函数项。
#[derive(Clone, Copy, Debug)]
pub struct RtOk {
    pub begin: u32,
    pub end: u32,
    pub unwind: u32,
}

impl RtOk {
    pub fn span(&self) -> u32 {
        self.end - self.begin
    }
}

/// 原始表项（未校验）。
#[derive(Clone, Copy, Debug)]
pub struct RtRaw {
    pub begin: u32,
    pub end: u32,
    pub unwind: u32,
}

/// 拒止与红账核。
pub struct RedLedger {
    reds: Vec<(u32, String)>,
    greens: Vec<RtOk>,
    image_size: u32,
    xdata_size: u32,
    frozen: bool,
}

impl RedLedger {
    pub fn new(image_size: u32, xdata_size: u32) -> Self {
        RedLedger {
            reds: Vec::new(),
            greens: Vec::new(),
            image_size,
            xdata_size,
            frozen: false,
        }
    }

    /// 装载整张 .pdata 表：先闸 ① 表长，再逐项闸 ②③④。
    /// 表长不合格时整体拒收（连扫描都不做）。
    pub fn load_table(&mut self, raw: &[RtRaw], table_bytes: u32) -> Result<usize, u32> {
        if self.frozen {
            self.reds.push((RD_FROZEN, "load after seal".to_string()));
            return Err(RD_FROZEN);
        }
        if table_bytes % RT_STRIDE != 0 || table_bytes / RT_STRIDE != raw.len() as u32 {
            self.reds.push((RD_SIZE, format!("bytes={} n={}", table_bytes, raw.len())));
            return Err(RD_SIZE);
        }
        let mut prev_begin = 0u32;
        for (i, r) in raw.iter().enumerate() {
            // 闸 ②：BeginAddress 非降序（允许等值？口径：严格递增，等值也拒）
            if i > 0 && r.begin <= prev_begin {
                self.reds.push((RD_UNSORTED, format!("idx={} begin={}", i, r.begin)));
                return Err(RD_UNSORTED);
            }
            prev_begin = r.begin;
            // 闸 ③：函数区间越出镜像
            if r.begin >= r.end || r.end > self.image_size {
                self.reds.push((RD_OOB, format!("begin={} end={}", r.begin, r.end)));
                return Err(RD_OOB);
            }
            // 闸 ④：UnwindInfo 越出 .xdata 或非 4 对齐
            if r.unwind >= self.xdata_size || r.unwind % 4 != 0 {
                self.reds.push((RD_UNWIND_OOB, format!("unwind={}", r.unwind)));
                return Err(RD_UNWIND_OOB);
            }
            self.greens.push(RtOk { begin: r.begin, end: r.end, unwind: r.unwind });
        }
        Ok(self.greens.len())
    }

    /// 单项装载入口（逐项闸链：排序 → 区间 → 展开位）。
    pub fn admit(&mut self, r: RtRaw) -> Result<usize, u32> {
        if self.frozen {
            self.reds.push((RD_FROZEN, "admit after seal".to_string()));
            return Err(RD_FROZEN);
        }
        if let Some(last) = self.greens.last() {
            if r.begin <= last.begin {
                self.reds.push((RD_UNSORTED, format!("begin={} vs {}", r.begin, last.begin)));
                return Err(RD_UNSORTED);
            }
        }
        if r.begin >= r.end || r.end > self.image_size {
            self.reds.push((RD_OOB, format!("begin={} end={}", r.begin, r.end)));
            return Err(RD_OOB);
        }
        if r.unwind >= self.xdata_size || r.unwind % 4 != 0 {
            self.reds.push((RD_UNWIND_OOB, format!("unwind={}", r.unwind)));
            return Err(RD_UNWIND_OOB);
        }
        self.greens.push(RtOk { begin: r.begin, end: r.end, unwind: r.unwind });
        Ok(self.greens.len() - 1)
    }

    /// 红条计数（append-only：只增不删，无删接口）。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    pub fn green_count(&self) -> usize {
        self.greens.len()
    }

    /// 第 i 条红条的错误码。
    pub fn red_code(&self, i: usize) -> Option<u32> {
        self.reds.get(i).map(|(c, _)| *c)
    }

    /// 全绿判定：无红条且绿账非空。
    pub fn all_green(&self) -> bool {
        self.reds.is_empty() && !self.greens.is_empty()
    }

    /// 绿账区间总跨度。
    pub fn green_span(&self) -> u32 {
        self.greens.iter().map(|g| g.span()).sum()
    }

    /// 折叠指纹：211 进制卷积，红绿两账全混入。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for (c, _d) in &self.reds {
            h = h.wrapping_mul(211).wrapping_add(*c);
        }
        for g in &self.greens {
            h = h.wrapping_mul(211).wrapping_add(g.begin);
            h = h.wrapping_mul(211).wrapping_add(g.end);
            h = h.wrapping_mul(211).wrapping_add(g.unwind ^ (g.unwind >> 3));
        }
        h.wrapping_mul(211).wrapping_add(self.greens.len() as u32)
    }

    /// 冻结红账与装载通道。
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        self.fold()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 五况注入核：每况踩一个独立检出分支。
pub fn inject_five() -> [Result<usize, u32>; 5] {
    let raw = [RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 }];
    // RD_SIZE：表字节数与条目数不符（3 字节余数）
    let mut l0 = RedLedger::new(0x10000, 0x1000);
    let r0 = l0.load_table(&raw, RT_STRIDE + 3);
    // RD_UNSORTED：第二项 begin 回退
    let raw2 = [
        RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 },
        RtRaw { begin: 0x80, end: 0x90, unwind: 0x80 },
    ];
    let mut l1 = RedLedger::new(0x10000, 0x1000);
    let r1 = l1.load_table(&raw2, RT_STRIDE * 2);
    // RD_OOB：end 越出镜像
    let mut l2 = RedLedger::new(0x1000, 0x1000);
    let r2 = l2.admit(RtRaw { begin: 0x100, end: 0x2000, unwind: 0x40 });
    // RD_UNWIND_OOB：非 4 对齐
    let mut l3 = RedLedger::new(0x10000, 0x1000);
    let r3 = l3.admit(RtRaw { begin: 0x100, end: 0x200, unwind: 0x42 });
    // RD_FROZEN：seal 后受理
    let mut l4 = RedLedger::new(0x10000, 0x1000);
    l4.seal();
    let r4 = l4.admit(RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 });
    [r0, r1, r2, r3, r4]
}

/// 判据核：14 条拒止与红账语义判据。
pub fn run_unx12133_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F12133");
    let mut l = RedLedger::new(0x10000, 0x1000);
    let a = l.admit(RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 });
    let b = l.admit(RtRaw { begin: 0x200, end: 0x380, unwind: 0x80 });
    cs.add("admit_two_ok", a.is_ok() && b.is_ok(), "two green entries");
    cs.add("green_span", l.green_span() == 0x100 + 0x180, "0x100 + 0x180");
    cs.add("all_green_true", l.all_green(), "no reds, greens exist");
    cs.add("stride_const", RT_STRIDE == 12, "12-byte triple");
    cs.add("rt_ok_span", RtOk { begin: 0, end: 0x30, unwind: 0 }.span() == 0x30,
        "span helper");
    cs.add("size_branch", inject_five()[0].err() == Some(RD_SIZE), "table bytes gate");
    cs.add("unsorted_branch", inject_five()[1].err() == Some(RD_UNSORTED),
        "order gate fires");
    cs.add("oob_branch", inject_five()[2].err() == Some(RD_OOB), "image bound gate");
    cs.add("unwind_oob_branch", inject_five()[3].err() == Some(RD_UNWIND_OOB),
        "xdata bound/align gate");
    cs.add("frozen_branch", inject_five()[4].err() == Some(RD_FROZEN), "seal gate");
    cs.add("equal_begin_rejected", {
        let mut ll = RedLedger::new(0x10000, 0x1000);
        let _ = ll.admit(RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 });
        ll.admit(RtRaw { begin: 0x100, end: 0x300, unwind: 0x40 })
            .err() == Some(RD_UNSORTED)
    }, "equal begin treated as unsorted");
    cs.add("zero_span_rejected", {
        let mut ll = RedLedger::new(0x10000, 0x1000);
        ll.admit(RtRaw { begin: 0x100, end: 0x100, unwind: 0x40 }).err() == Some(RD_OOB)
    }, "empty interval is OOB");
    cs.add("red_append_only", {
        let inj = inject_five();
        // 注入五账各产一条红：红账只增不删
        inj.iter().all(|r| r.is_err())
    }, "five independent reds");
    cs.add("fold_seal_differs", {
        let mut l1 = RedLedger::new(0x100, 0x100);
        let _ = l1.admit(RtRaw { begin: 0x10, end: 0x20, unwind: 0x0 });
        let f1 = l1.seal();
        let mut l2 = RedLedger::new(0x100, 0x100);
        let _ = l2.admit(RtRaw { begin: 0x10, end: 0x30, unwind: 0x0 });
        let f2 = l2.seal();
        f1 != f2
    }, "fold distinguishes ledgers");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_good_table() {
        let raw = [
            RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 },
            RtRaw { begin: 0x200, end: 0x260, unwind: 0x80 },
        ];
        let mut l = RedLedger::new(0x10000, 0x1000);
        assert_eq!(l.load_table(&raw, RT_STRIDE * 2), Ok(2));
        assert!(l.all_green());
        assert_eq!(l.green_span(), 0x100 + 0x60);
    }

    #[test]
    fn test_size_gate() {
        let raw = [RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 }];
        let mut l = RedLedger::new(0x10000, 0x1000);
        assert!(l.load_table(&raw, 11).err() == Some(RD_SIZE));
        assert!(l.load_table(&raw, RT_STRIDE + 12).err() == Some(RD_SIZE));
        assert_eq!(l.red_count(), 2);
        assert!(l.red_code(0) == Some(RD_SIZE));
    }

    #[test]
    fn test_order_and_bounds() {
        let mut l = RedLedger::new(0x10000, 0x1000);
        assert!(l.admit(RtRaw { begin: 0x100, end: 0x200, unwind: 0x40 }).is_ok());
        assert!(l.admit(RtRaw { begin: 0x40, end: 0x80, unwind: 0x80 })
            .err() == Some(RD_UNSORTED));
        assert!(l.admit(RtRaw { begin: 0x200, end: 0x20000, unwind: 0x80 })
            .err() == Some(RD_OOB));
        assert!(l.admit(RtRaw { begin: 0x200, end: 0x300, unwind: 0x2000 })
            .err() == Some(RD_UNWIND_OOB));
        assert_eq!(l.green_count(), 1);
        assert_eq!(l.red_count(), 3);
    }

    #[test]
    fn test_seal_and_inject() {
        let mut l = RedLedger::new(0x100, 0x100);
        let f = l.seal();
        assert!(l.is_frozen());
        assert_eq!(f, l.fold());
        assert!(inject_five().iter().all(|r| r.is_err()));
    }
}