//! unx0397_bigbypass —— UNX-F0397 大块分配旁路（>512 直页帧）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0397 · 大块分配旁路 · 目标 240 行
//! 判据原文：UNX-F0397-J1 请求尺寸超过阈值（512 字节模型）时
//!           旁路堆分配器直接走页帧池（按页取整）；旁路判定
//!           可重放（同尺寸同路径）；旁路块释放直还页帧池
//!           （不走堆 free）；路径记账：堆路径/旁路路径各自
//!           计数，总请求 = 两路之和（账平）；阈值可配（配置
//!           闸：0 = 全堆路径，超大 = 全旁路），配置越域拒。
//! 组件：路由器（阈值比较 + 路径选择）+ 双路径账（堆路/旁路
//!       计数与字节量）+ 页帧取整（字节 → 页数，向上取整）+
//!       配置闸 + 路由对账。
//! 职责定位：大块旁路是「堆的泄洪道」——大块进堆会加速碎片化
//!   且常驻时间长，直连页帧池让大块生命周期与页账直接挂钩；
//!   阈值是路由的唯一裁判：同尺寸必同路径（确定性），双路账
//!   平（不漏不重）是路由器诚实的底线。
//! 性能逐项分解：路由 O(1)；页取整 O(1)；对账 O(1)。
//! 诊断码段：0x20E00（0x20E01~0x20E09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BB_VERSION: &str = "unx0397_bigbypass-v1";

/// 诊断段基址（0x20E00 段 · 本模块独占）。
pub const BB_SEG_BASE: u32 = 0x20E00;
pub const XC_THRESH_OOR: u32 = 0x20E01; // 阈值越域（>1MB）
pub const XC_ZERO_REQ: u32 = 0x20E02; // 零字节请求
pub const XC_REQ_BIG: u32 = 0x20E03; // 请求超单上限
pub const XC_ROUTE_UNBAL: u32 = 0x20E04; // 路由账不平
pub const XC_BYTE_UNBAL: u32 = 0x20E05; // 字节账不平
pub const XC_REPLAY: u32 = 0x20E06; // 路由重放不一致
pub const XC_PAGE_OVF: u32 = 0x20E07; // 页数溢出
pub const XC_FREE_TWICE: u32 = 0x20E08; // 旁路块双释
pub const XC_NOT_MINE: u32 = 0x20E09; // 释放非本路块

/// 路径两态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// 堆路径。
    Heap,
    /// 旁路（直页帧池）。
    Bypass,
}

/// 路由器。
pub struct Router {
    /// 阈值（字节）：> 阈值走旁路。
    pub thresh: u32,
    /// 单请求上限。
    pub max_req: u32,
    heap_cnt: u32,
    bypass_cnt: u32,
    heap_bytes: u64,
    bypass_bytes: u64,
    /// 旁路块在册（页数）。
    bypass_open: Vec<u32>,
}

impl Router {
    /// 建：阈值 + 单上限。
    pub fn new(thresh: u32, max_req: u32) -> Result<Router, u32> {
        if thresh > 1 << 20 {
            return Err(XC_THRESH_OOR);
        }
        if max_req == 0 || max_req > 1 << 24 {
            return Err(XC_REQ_BIG);
        }
        Ok(Router {
            thresh,
            max_req,
            heap_cnt: 0,
            bypass_cnt: 0,
            heap_bytes: 0,
            bypass_bytes: 0,
            bypass_open: Vec::new(),
        })
    }

    /// 路径判定（纯函数）。
    pub fn route_of(&self, bytes: u32) -> Result<Route, u32> {
        if bytes == 0 {
            return Err(XC_ZERO_REQ);
        }
        if bytes > self.max_req {
            return Err(XC_REQ_BIG);
        }
        if bytes > self.thresh {
            Ok(Route::Bypass)
        } else {
            Ok(Route::Heap)
        }
    }

    /// 分配：按路由记账；旁路则按页取整入册。返回路径。
    pub fn alloc(&mut self, bytes: u32) -> Result<Route, u32> {
        let r = self.route_of(bytes)?;
        match r {
            Route::Heap => {
                self.heap_cnt += 1;
                self.heap_bytes += bytes as u64;
            }
            Route::Bypass => {
                self.bypass_cnt += 1;
                self.bypass_bytes += bytes as u64;
                self.bypass_open.push(pages_of(bytes)?);
            }
        }
        Ok(r)
    }

    /// 旁路释放：从在册弹一笔页数直还；空册拒。
    pub fn bypass_free(&mut self) -> Result<u32, u32> {
        match self.bypass_open.pop() {
            Some(p) => Ok(p),
            None => Err(XC_NOT_MINE),
        }
    }

    /// 路由账平：堆计数 + 旁路计数 = 总请求（调用方持有）。
    pub fn counts(&self) -> (u32, u32) {
        (self.heap_cnt, self.bypass_cnt)
    }

    /// 字节账平：堆字节 + 旁路字节 = 总字节。
    pub fn bytes(&self) -> (u64, u64) {
        (self.heap_bytes, self.bypass_bytes)
    }

    /// 旁路在册数。
    pub fn bypass_open_count(&self) -> usize {
        self.bypass_open.len()
    }
}

/// 页取整：字节 → 页数（4096/页，向上取整）。
pub fn pages_of(bytes: u32) -> Result<u32, u32> {
    if bytes == 0 {
        return Err(XC_ZERO_REQ);
    }
    if bytes > (1 << 24) {
        return Err(XC_PAGE_OVF);
    }
    Ok((bytes + 4095) / 4096)
}

/// 路由重放核：同参数两路由器同请求序列，计数与字节全等。
pub fn replay_eq(thresh: u32, max_req: u32, seq: &[u32]) -> Result<(), u32> {
    let mut a = match Router::new(thresh, max_req) {
        Ok(r) => r,
        Err(_) => return Err(XC_THRESH_OOR),
    };
    let mut b = match Router::new(thresh, max_req) {
        Ok(r) => r,
        Err(_) => return Err(XC_THRESH_OOR),
    };
    for &s in seq.iter() {
        let _ = a.alloc(s);
        let _ = b.alloc(s);
    }
    if a.counts() != b.counts() || a.bytes() != b.bytes() {
        return Err(XC_REPLAY);
    }
    Ok(())
}

pub fn run_unx0397_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0397_bigbypass");
    // 族一：路径判定。
    let r = match Router::new(512, 1 << 20) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建路由", false, "拒");
            return cs;
        }
    };
    cs.add("小走堆", r.route_of(100) == Ok(Route::Heap), "100 ≤ 512");
    cs.add("贴阈走堆", r.route_of(512) == Ok(Route::Heap), "等于不走");
    cs.add("超阈旁路", r.route_of(513) == Ok(Route::Bypass), "513 > 512");
    cs.add("大走旁路", r.route_of(65536) == Ok(Route::Bypass), "64KB");
    cs.add("零拒", r.route_of(0) == Err(XC_ZERO_REQ), "bytes=0");
    cs.add("超上限拒", r.route_of((1 << 20) + 1) == Err(XC_REQ_BIG), "> max");
    // 族二：双路记账。
    let mut m = match Router::new(512, 1 << 20) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建路由2", false, "拒");
            return cs;
        }
    };
    let _ = m.alloc(100);
    let _ = m.alloc(512);
    let _ = m.alloc(4096);
    let _ = m.alloc(1000);
    let (hc, bc) = m.counts();
    cs.add("堆路 2 笔", hc == 2, "100 + 512");
    cs.add("旁路 2 笔", bc == 2, "4096 + 1000");
    cs.add("路由账平", hc + bc == 4, "总 4 笔");
    let (hb, pb) = m.bytes();
    cs.add("堆字节 612", hb == 612, "100 + 512");
    cs.add("旁路字节 5096", pb == 5096, "4096 + 1000");
    cs.add("字节账平", hb + pb == 5708, "总字节");
    cs.add("旁路在册 2", m.bypass_open_count() == 2, "页数入册");
    // 旁路释放。
    cs.add("弹 1 页", m.bypass_free() == Ok(1), "1000 → 1 页");
    cs.add("弹 1 页", m.bypass_free() == Ok(1), "4096 → 1 页");
    cs.add("空册拒", m.bypass_free() == Err(XC_NOT_MINE), "已清");
    // 族三：页取整。
    cs.add("1 字节 1 页", pages_of(1) == Ok(1), "向上取整");
    cs.add("4095 1 页", pages_of(4095) == Ok(1), "差 1 取整");
    cs.add("4096 1 页", pages_of(4096) == Ok(1), "整页");
    cs.add("4097 2 页", pages_of(4097) == Ok(2), "进 2 页");
    cs.add("8192 2 页", pages_of(8192) == Ok(2), "整 2 页");
    cs.add("零字节拒", pages_of(0) == Err(XC_ZERO_REQ), "bytes=0");
    // 族四：阈值配置闸。
    cs.add("全堆（阈 1MB）", {
        let t = match Router::new(1 << 20, 1 << 20) {
            Ok(x) => x,
            Err(_) => {
                cs.add("建路由3", false, "拒");
                return cs;
            }
        };
        t.route_of((1 << 20) - 1) == Ok(Route::Heap)
    }, "顶阈之下全堆");
    cs.add("全旁路（阈 0）", {
        let t = match Router::new(0, 1 << 20) {
            Ok(x) => x,
            Err(_) => {
                cs.add("建路由4", false, "拒");
                return cs;
            }
        };
        t.route_of(1) == Ok(Route::Bypass)
    }, "阈 0 全旁路");
    cs.add("阈超域拒", matches!(Router::new((1 << 20) + 1, 1 << 20), Err(XC_THRESH_OOR)), ">1MB");
    cs.add("上限零拒", matches!(Router::new(512, 0), Err(XC_REQ_BIG)), "max=0");
    // 族五：重放确定性。
    let seq: [u32; 6] = [64, 600, 4096, 100, 512, 1024];
    cs.add("重放一致", replay_eq(512, 1 << 20, &seq).is_ok(), "计数与字节齐");
    cs
}
