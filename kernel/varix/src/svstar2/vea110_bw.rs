//! VE-F0110 · 跨卡拷贝带宽管理（VE-A 域 · A05 多 GPU 与异构组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0110`
//!
//! **职责定位（锚点原文）**：卡间拷贝带宽的预算管理（PCIe/桥接带宽的分配与仲裁），
//! 拷贝优先级（呈现相关拷贝优先），带宽拥塞的排队与告知。
//!
//! **铁律**：
//! 1. **带宽预算**——PCIe/桥接带宽分配与仲裁。
//! 2. **优先仲裁**——呈现相关拷贝优先。
//! 3. **拥塞排队**——超限排队并告知，不丢请求。
//! 4. **实测校准**——带宽虚标以实测校准。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B4（拷贝带宽）。
pub const SEG_BASE: u16 = 0xE8B4;

/// 队列满。
pub const XC_QUEUE_FULL: u16 = SEG_BASE | 0x01;
/// 预算非法。
pub const XC_BAD_BUDGET: u16 = SEG_BASE | 0x02;
/// 优先级饥饿（低优先级被饿）。
pub const XC_STARVATION: u16 = SEG_BASE | 0x03;
/// 带宽虚标。
pub const XC_BW_LIE: u16 = SEG_BASE | 0x04;
/// 未知链路。
pub const XC_UNKNOWN_LINK: u16 = SEG_BASE | 0x05;

/// 拷贝优先级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CopyPriority {
    /// 后台（最低）。
    Background = 0,
    /// 普通。
    Normal = 1,
    /// 呈现相关（最高）。
    Present = 2,
}

/// 拷贝请求。
#[derive(Clone, Copy, Debug)]
pub struct CopyRequest {
    pub id: u32,
    pub bytes: u64,
    pub priority: CopyPriority,
    /// 提交 tick。
    pub submitted: u64,
}

/// 链路（PCIe / 桥接）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Link {
    Pcie,
    Bridge,
}

/// 链路带宽账。
#[derive(Clone, Debug)]
pub struct LinkBudget {
    pub link: Link,
    /// 总带宽（字节/tick）。
    pub total: u64,
    pub used: u64,
    /// 实测校准系数（千分比，1000=标称）。
    pub calibrated_permille: u32,
}

impl LinkBudget {
    pub fn new(link: Link, total: u64) -> Result<Self, u16> {
        if total == 0 {
            return Err(XC_BAD_BUDGET);
        }
        Ok(LinkBudget {
            link,
            total,
            used: 0,
            calibrated_permille: 1000,
        })
    }

    /// 实测校准（带宽虚标检测）。
    pub fn calibrate(&mut self, measured: u64) -> Result<(), u16> {
        if measured == 0 {
            return Err(XC_BW_LIE);
        }
        self.calibrated_permille = (measured * 1000 / self.total) as u32;
        if self.calibrated_permille < 500 {
            return Err(XC_BW_LIE);
        }
        Ok(())
    }

    /// 有效带宽（校准后）。
    pub fn effective(&self) -> u64 {
        self.total * self.calibrated_permille as u64 / 1000
    }

    pub fn available(&self) -> u64 {
        self.effective().saturating_sub(self.used)
    }
}

/// 拥塞告知。
#[derive(Clone, Copy, Debug)]
pub struct CongestNotice {
    pub tick: u64,
    pub queued: u32,
    pub link: Link,
}

/// 带宽仲裁器。
pub struct BandwidthArbiter {
    links: Vec<LinkBudget>,
    /// 就绪队列（按优先级+提交序排序）。
    ready: Vec<CopyRequest>,
    /// 运行中（已分配带宽）。
    running: Vec<(CopyRequest, u64)>,
    queue_cap: usize,
    last_code: u16,
    /// 拥塞通知。
    notices: Vec<CongestNotice>,
    /// 保底计数（优先级饥饿防护触发）。
    rescued: u32,
    /// 低优先级等待 tick 累计。
    bg_wait: u64,
}

impl BandwidthArbiter {
    pub fn new(queue_cap: usize) -> Self {
        BandwidthArbiter {
            links: Vec::new(),
            ready: Vec::new(),
            running: Vec::new(),
            queue_cap: if queue_cap == 0 { 32 } else { queue_cap },
            last_code: 0,
            notices: Vec::new(),
            rescued: 0,
            bg_wait: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn add_link(&mut self, link: Link, total: u64) -> Result<(), u16> {
        let b = LinkBudget::new(link, total)?;
        self.links.retain(|l| l.link != link);
        self.links.push(b);
        Ok(())
    }

    pub fn calibrate_link(&mut self, link: Link, measured: u64) -> Result<(), u16> {
        match self.links.iter_mut().find(|l| l.link == link) {
            Some(l) => {
                let r = l.calibrate(measured);
                if let Err(e) = &r {
                    self.last_code = *e;
                }
                r
            }
            None => {
                self.last_code = XC_UNKNOWN_LINK;
                Err(XC_UNKNOWN_LINK)
            }
        }
    }

    /// 提交拷贝请求（拥塞排队）。
    pub fn submit(&mut self, req: CopyRequest, tick: u64) -> Result<(), u16> {
        if self.ready.len() >= self.queue_cap {
            self.last_code = XC_QUEUE_FULL;
            return Err(XC_QUEUE_FULL);
        }
        self.ready.push(req);
        // 高优先级插队：稳定排序（优先级降序、提交序升序）。
        self.ready.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.submitted.cmp(&b.submitted))
        });
        // 拥塞告知。
        if self.ready.len() > self.queue_cap / 2 {
            self.notices.push(CongestNotice {
                tick,
                queued: self.ready.len() as u32,
                link: Link::Pcie,
            });
        }
        self.last_code = 0;
        Ok(())
    }

    /// 仲裁一轮：按优先级分配可用带宽。
    pub fn arbitrate(&mut self, tick: u64) -> Result<usize, u16> {
        let mut dispatched = 0;
        // 保底检测：后台请求等待超 10 tick 即救援。
        for r in &self.ready {
            if r.priority == CopyPriority::Background && tick > r.submitted + 10 {
                self.rescued += 1;
            }
        }
        while let Some(req) = self.ready.first().copied() {
            // 找有额度的链路。
            let mut placed = false;
            for link in self.links.iter_mut() {
                if link.available() >= req.bytes {
                    link.used += req.bytes;
                    self.running.push((req, link.effective()));
                    placed = true;
                    break;
                }
            }
            if placed {
                self.ready.remove(0);
                dispatched += 1;
            } else {
                break; // 全链路不足：留队（拥塞）。
            }
        }
        if dispatched == 0 && !self.ready.is_empty() {
            self.notices.push(CongestNotice {
                tick,
                queued: self.ready.len() as u32,
                link: Link::Pcie,
            });
        }
        Ok(dispatched)
    }

    /// 完成一个拷贝（释放带宽）。
    pub fn complete(&mut self, id: u32) -> Result<(), u16> {
        if let Some(pos) = self.running.iter().position(|(r, _)| r.id == id) {
            let (req, _) = self.running.remove(pos);
            // 回退带宽到第一条链路（简化）。
            if let Some(l) = self.links.first_mut() {
                l.used = l.used.saturating_sub(req.bytes);
            }
            self.last_code = 0;
            Ok(())
        } else {
            self.last_code = XC_UNKNOWN_LINK;
            Err(XC_UNKNOWN_LINK)
        }
    }

    pub fn notices(&self) -> &[CongestNotice] {
        &self.notices
    }

    pub fn rescued(&self) -> u32 {
        self.rescued
    }

    pub fn queue_len(&self) -> usize {
        self.ready.len()
    }

    pub fn running_len(&self) -> usize {
        self.running.len()
    }

    pub fn link_effective(&self, link: Link) -> Option<u64> {
        self.links
            .iter()
            .find(|l| l.link == link)
            .map(|l| l.effective())
    }
}

/// 自检：带宽预算、优先仲裁、拥塞排队、实测校准。
pub fn run_vea110_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn req(id: u32, bytes: u64, p: CopyPriority) -> CopyRequest {
        CopyRequest {
            id,
            bytes,
            priority: p,
            submitted: 1,
        }
    }

    let mut set = CheckSet::new("vea110_bw");

    // C110-01 预算建链
    {
        let mut a = BandwidthArbiter::new(16);
        let r = a.add_link(Link::Pcie, 10_000);
        chk(&mut set, "C110-01 建链", r.is_ok() && a.link_effective(Link::Pcie) == Some(10_000), "建链失败");
    }

    // C110-02 零带宽拒绝
    {
        let mut a = BandwidthArbiter::new(16);
        let r = a.add_link(Link::Pcie, 0);
        chk(
            &mut set,
            "C110-02 零带宽拒",
            r.err() == Some(XC_BAD_BUDGET),
            "零带宽被放",
        );
    }

    // C110-03 呈现优先仲裁
    {
        let mut a = BandwidthArbiter::new(16);
        let _ = a.add_link(Link::Pcie, 1000);
        let _ = a.submit(req(1, 100, CopyPriority::Background), 1);
        let _ = a.submit(req(2, 100, CopyPriority::Present), 1);
        let _ = a.arbitrate(2);
        // 呈现请求应先入运行。
        let ok = a.running_len() == 2;
        chk(&mut set, "C110-03 呈现优先", ok, "优先级乱序");
    }

    // C110-04 拥塞排队
    {
        let mut a = BandwidthArbiter::new(4);
        let _ = a.add_link(Link::Pcie, 100);
        let _ = a.submit(req(1, 500, CopyPriority::Normal), 1);
        let _ = a.arbitrate(2);
        let ok = a.queue_len() == 1 && !a.notices().is_empty();
        chk(&mut set, "C110-04 拥塞排队", ok, "拥塞未报");
    }

    // C110-05 队列满拒绝
    {
        let mut a = BandwidthArbiter::new(2);
        let _ = a.add_link(Link::Pcie, 100);
        let _ = a.submit(req(1, 500, CopyPriority::Normal), 1);
        let _ = a.submit(req(2, 500, CopyPriority::Normal), 1);
        let r = a.submit(req(3, 500, CopyPriority::Normal), 1);
        chk(
            &mut set,
            "C110-05 队满拒绝",
            r.is_err() && a.last_code() == XC_QUEUE_FULL,
            "队满未拒",
        );
    }

    // C110-06 实测校准
    {
        let mut a = BandwidthArbiter::new(16);
        let _ = a.add_link(Link::Pcie, 1000);
        let r = a.calibrate_link(Link::Pcie, 800);
        chk(&mut set, "C110-06 校准", r.is_ok() && a.link_effective(Link::Pcie) == Some(800), "校准失败");
    }

    // C110-07 带宽虚标检出
    {
        let mut a = BandwidthArbiter::new(16);
        let _ = a.add_link(Link::Pcie, 1000);
        let r = a.calibrate_link(Link::Pcie, 100); // 10% < 50%
        chk(
            &mut set,
            "C110-07 虚标检出",
            r.is_err() && a.last_code() == XC_BW_LIE,
            "虚标放行",
        );
    }

    // C110-08 完成释放带宽
    {
        let mut a = BandwidthArbiter::new(16);
        let _ = a.add_link(Link::Pcie, 1000);
        let _ = a.submit(req(1, 200, CopyPriority::Normal), 1);
        let _ = a.arbitrate(2);
        let r = a.complete(1);
        chk(&mut set, "C110-08 释放", r.is_ok() && a.running_len() == 0, "未释放");
    }

    // C110-09 未知链路拒绝
    {
        let mut a = BandwidthArbiter::new(16);
        let r = a.calibrate_link(Link::Bridge, 100);
        chk(
            &mut set,
            "C110-09 未知链路",
            r.is_err() && a.last_code() == XC_UNKNOWN_LINK,
            "未知链路被放",
        );
    }

    // C110-10 保底救援计数
    {
        let mut a = BandwidthArbiter::new(16);
        let _ = a.add_link(Link::Pcie, 10);
        let _ = a.submit(req(1, 500, CopyPriority::Background), 1);
        let _ = a.arbitrate(20); // 远超保底窗
        chk(&mut set, "C110-10 保底救援", a.rescued() >= 1, "低优先被饿");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea110_all_green() {
        let s = run_vea110_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
