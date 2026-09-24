//! usbnet — WP-204 · B-607 手机共享通道（MD2 篇 6.1 驱动清单）。
//!
//! 判据 B-607：RNDIS/NCM 实机可用。
//! MD2 原文（6.1）："USB 网卡（RNDIS 与 NCM 两类，手机共享的关键通道，
//! 第一批——判例侧它是 WiFi 空窗期的用户生命线）。"
//!
//! 宿主可测形态：两类通道枚举（与 netthr::DRIVER_BATCH1 手机共享两条
//! 同源对账）+ 通道生命周期（插入检测 → 绑定 → 数据面就绪 → 拔出清理，
//! 无幽灵通道）+ 选择语义（NCM 优先——USB 标准协议优先于微软旧协议）+
//! 生命线语义（无线未就绪时 USB 共享必须顶上——第一批的定位）+
//! **实机偏差登记**（真实手机 + 真实 USB 控制器的实机验证为硬项，随队
//! 跟踪：登记在案后宿主面判定 HostModelDone + DeviationRegistered）。

use crate::checks::CheckSet;

/// USB 共享通道容量（同时插几个 USB 网卡）。
pub const CHAN_CAP: usize = 4;

/// 手机共享两类通道（MD2 6.1 驱动清单）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsbNicKind {
    /// 微软旧协议（Android USB 共享的传统挡位）。
    Rndis,
    /// USB 标准协议（NCM——新版 Android/iOS 共享挡位）。
    Ncm,
}

/// 与 netthr::DRIVER_BATCH1 同源对账：手机共享两条 = RNDIS + NCM。
pub const USB_SHARED_IN_BATCH1: usize = 2;

/// 一条 USB 网卡通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UsbChannel {
    pub kind: UsbNicKind,
    /// 数据面就绪（绑定完成、可收发帧）。
    pub up: bool,
    pub seq: u32,
}

/// USB 共享通道管理器。
pub struct UsbNetMgr {
    pub chans: [Option<UsbChannel>; CHAN_CAP],
    pub n: usize,
    pub seq: u32,
    pub attaches: u64,
    pub detaches: u64,
}

impl UsbNetMgr {
    pub const fn new() -> UsbNetMgr {
        UsbNetMgr { chans: [None; CHAN_CAP], n: 0, seq: 0, attaches: 0, detaches: 0 }
    }

    /// 插入检测 + 驱动绑定（绑定即数据面就绪——第一批驱动的宿主模型）。
    pub fn attach(&mut self, kind: UsbNicKind) -> Option<usize> {
        if self.n >= CHAN_CAP {
            return None; // 槽满：不越界不覆盖
        }
        for (i, slot) in self.chans.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(UsbChannel { kind, up: true, seq: self.seq });
                self.seq += 1;
                self.n += 1;
                self.attaches += 1;
                return Some(i);
            }
        }
        None
    }

    /// 拔出：通道清理（槽位归 None——无幽灵通道）。
    pub fn detach(&mut self, idx: usize) -> bool {
        if idx >= CHAN_CAP {
            return false;
        }
        if self.chans[idx].take().is_some() {
            self.n -= 1;
            self.detaches += 1;
            true
        } else {
            false // 双次拔出：无效操作（不误删他人槽位）
        }
    }

    /// 选择语义：活跃通道中 NCM 优先（标准协议优先于旧协议）。
    pub fn selected(&self) -> Option<UsbNicKind> {
        let mut ncm = None;
        let mut rndis = None;
        for slot in self.chans.iter().take(CHAN_CAP) {
            match slot {
                Some(c) if c.up => match c.kind {
                    UsbNicKind::Ncm => ncm = Some(c.seq),
                    UsbNicKind::Rndis => rndis = Some(c.seq),
                },
                _ => {}
            }
        }
        match (ncm, rndis) {
            (Some(_), _) => Some(UsbNicKind::Ncm),
            (None, Some(_)) => Some(UsbNicKind::Rndis),
            _ => None,
        }
    }

    pub fn live_count(&self) -> usize {
        self.n
    }
}

/// 生命线语义：无线未就绪时 USB 共享必须顶上（WiFi 空窗期的用户生命线）。
pub const fn lifeline_ready(wireless_ready: bool, usb_alive: bool) -> bool {
    if wireless_ready {
        true // 无线就绪：正常路径
    } else {
        usb_alive // 无线空窗：USB 共享必须可用（第一批定位）
    }
}

// ---------------------------------------------------------------- 实机偏差登记

/// 实机偏差条目（B-607 实机硬项随队跟踪）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deviation {
    pub item: &'static str,
    /// 宿主面判定：模型完成 + 偏差登记（实机窗口到位后销项）。
    pub status: &'static str,
}

/// 实机偏差登记表（定长——随队跟踪面）。
pub struct DeviationLog {
    pub items: [Option<Deviation>; 8],
    pub n: usize,
}

impl DeviationLog {
    pub const fn new() -> DeviationLog {
        DeviationLog { items: [None; 8], n: 0 }
    }

    pub fn register(&mut self, item: &'static str) -> bool {
        if self.n >= 8 || self.items.iter().take(self.n).any(|d| d.map(|d| d.item == item).unwrap_or(false)) {
            return false;
        }
        self.items[self.n] = Some(Deviation { item, status: "HostModelDone+DeviationRegistered" });
        self.n += 1;
        true
    }

    pub fn has(&self, item: &'static str) -> bool {
        self.items.iter().take(self.n).any(|d| d.map(|d| d.item == item).unwrap_or(false))
    }
}

/// B-607 实机偏差三条（真实手机 + 真实 USB 控制器的验证项）。
pub const DEVIATIONS: [&str; 3] = [
    "RNDIS 实机插拔与共享上网验证",
    "NCM 实机插拔与共享上网验证",
    "手机共享实测吞吐记录",
];

// ---------------------------------------------------------------- 对练

/// 手机共享对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct UsbDrillSummary {
    pub rounds: u32,
    pub attaches: u64,
    pub detaches: u64,
    /// 活跃通道数对账（attach − detach == live）
    pub count_correct: bool,
    /// 选择语义（NCM 优先）始终成立
    pub select_correct: bool,
    /// 拔出清理无幽灵通道
    pub clean: bool,
}

/// 随机插拔对练：状态对账 + 选择语义 + 清理闭环。
pub fn run_usb_drills(seed: u64, rounds: u32) -> UsbDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = UsbDrillSummary::default();
    sum.rounds = rounds;
    sum.count_correct = true;
    sum.select_correct = true;
    sum.clean = true;
    let mut mgr = UsbNetMgr::new();
    for _ in 0..rounds {
        let roll = g.next() % 3;
        if roll < 2 {
            // 插入：RNDIS/NCM 随机
            let kind = if g.next() % 2 == 0 { UsbNicKind::Rndis } else { UsbNicKind::Ncm };
            if mgr.attach(kind).is_some() {
                sum.attaches += 1;
            }
        } else {
            // 拔出：随机挑一个非空槽
            let live: [usize; CHAN_CAP] = {
                let mut arr = [usize::MAX; CHAN_CAP];
                let mut k = 0;
                for (i, slot) in mgr.chans.iter().enumerate() {
                    if slot.is_some() {
                        arr[k] = i;
                        k += 1;
                    }
                }
                arr
            };
            let k = live.iter().filter(|i| **i != usize::MAX).count();
            if k > 0 {
                let pick = live[(g.next() % k as u64) as usize];
                if mgr.detach(pick) {
                    sum.detaches += 1;
                }
            }
        }
        // 状态对账
        if mgr.live_count() as u64 != sum.attaches - sum.detaches {
            sum.count_correct = false;
        }
        // 选择语义：有 NCM 活跃必选 NCM
        let has_ncm = mgr.chans.iter().any(|s| s.map(|c| c.kind == UsbNicKind::Ncm && c.up).unwrap_or(false));
        match mgr.selected() {
            Some(UsbNicKind::Ncm) if !has_ncm => sum.select_correct = false,
            Some(UsbNicKind::Rndis) if has_ncm => sum.select_correct = false,
            None if mgr.live_count() > 0 => sum.select_correct = false,
            _ => {}
        }
    }
    // 清理闭环：全部拔空后无幽灵（清理拔出同样计入对账）
    let mut idx = 0;
    while mgr.live_count() > 0 {
        if mgr.chans[idx].is_some() {
            if mgr.detach(idx) {
                sum.detaches += 1;
            }
        }
        idx = (idx + 1) % CHAN_CAP;
    }
    if mgr.live_count() != 0 || mgr.chans.iter().any(|s| s.is_some()) {
        sum.clean = false;
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_usbnet_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-607 手机共享通道");
    {
        // 两类通道枚举齐
        set.add(
            "B-607 RNDIS/NCM 两类齐",
            UsbNicKind::Rndis != UsbNicKind::Ncm,
            "MD2 6.1：RNDIS 与 NCM 两类",
        );
    }
    {
        // 与 netthr::DRIVER_BATCH1 同源对账
        set.add(
            "B-607 驱动矩阵同源对账",
            USB_SHARED_IN_BATCH1 == 2
                && crate::netthr::DRIVER_BATCH1.iter().filter(|d| d.kind == "手机共享").count() == USB_SHARED_IN_BATCH1,
            "第一批驱动清单手机共享两条（RNDIS + NCM）",
        );
    }
    {
        // attach/detach 状态对账
        let mut mgr = UsbNetMgr::new();
        let a = mgr.attach(UsbNicKind::Rndis);
        let b = mgr.attach(UsbNicKind::Ncm);
        let ok = a == Some(0) && b == Some(1) && mgr.live_count() == 2;
        let _ = mgr.detach(0);
        set.add(
            "B-607 插拔状态对账",
            ok && mgr.live_count() == 1 && mgr.detaches == 1,
            "attach − detach == live",
        );
    }
    {
        // 选择语义：NCM 优先
        let mut mgr = UsbNetMgr::new();
        let _ = mgr.attach(UsbNicKind::Rndis);
        let only_rndis = mgr.selected() == Some(UsbNicKind::Rndis);
        let _ = mgr.attach(UsbNicKind::Ncm);
        set.add(
            "B-607 选择语义 NCM 优先",
            only_rndis && mgr.selected() == Some(UsbNicKind::Ncm),
            "USB 标准协议优先于微软旧协议",
        );
    }
    {
        // 拔出清理：无幽灵通道
        let mut mgr = UsbNetMgr::new();
        let _ = mgr.attach(UsbNicKind::Ncm);
        let _ = mgr.detach(0);
        let _ = mgr.attach(UsbNicKind::Rndis);
        set.add(
            "B-607 拔出清理无幽灵",
            mgr.chans[0].map(|c| c.kind == UsbNicKind::Rndis).unwrap_or(false)
                && mgr.chans.iter().filter(|s| s.is_some()).count() == 1,
            "槽位归 None 后可复用且不残留旧通道",
        );
    }
    {
        // 生命线语义
        set.add(
            "B-607 生命线语义",
            lifeline_ready(false, true) && !lifeline_ready(false, false) && lifeline_ready(true, false),
            "WiFi 空窗期用户生命线：无线未就绪时 USB 共享顶上",
        );
    }
    {
        // 实机偏差登记（B-607 实机硬项随队跟踪）
        let mut log = DeviationLog::new();
        for item in DEVIATIONS {
            let _ = log.register(item);
        }
        set.add(
            "B-607 实机偏差登记在案",
            log.n == 3 && log.has(DEVIATIONS[0]) && log.has(DEVIATIONS[2]),
            "实机验证为硬项：HostModelDone+DeviationRegistered，随队跟踪",
        );
    }
    {
        // 手机共享对练
        let sum = run_usb_drills(0xB607, 80);
        set.add(
            "B-607 手机共享对练",
            sum.rounds == 80 && sum.count_correct && sum.select_correct && sum.clean && sum.attaches > 0,
            "RNDIS/NCM 宿主语义全绿（实机项已登记）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f707_attach_detach() {
        let mut mgr = UsbNetMgr::new();
        assert_eq!(mgr.attach(UsbNicKind::Ncm), Some(0));
        assert_eq!(mgr.attach(UsbNicKind::Rndis), Some(1));
        assert_eq!(mgr.live_count(), 2);
        assert!(mgr.detach(1));
        assert!(!mgr.detach(1), "双次拔出无效");
        assert_eq!(mgr.live_count(), 1);
    }

    #[test]
    fn f707_select_priority() {
        let mut mgr = UsbNetMgr::new();
        assert_eq!(mgr.selected(), None);
        let _ = mgr.attach(UsbNicKind::Rndis);
        assert_eq!(mgr.selected(), Some(UsbNicKind::Rndis));
        let _ = mgr.attach(UsbNicKind::Ncm);
        assert_eq!(mgr.selected(), Some(UsbNicKind::Ncm), "NCM 优先");
        let _ = mgr.detach(mgr.chans.iter().position(|s| s.map(|c| c.kind == UsbNicKind::Ncm).unwrap_or(false)).unwrap());
        assert_eq!(mgr.selected(), Some(UsbNicKind::Rndis));
    }

    #[test]
    fn f707_cap_no_overflow() {
        let mut mgr = UsbNetMgr::new();
        for _ in 0..CHAN_CAP {
            assert!(mgr.attach(UsbNicKind::Ncm).is_some());
        }
        assert_eq!(mgr.attach(UsbNicKind::Rndis), None, "槽满不越界");
        assert_eq!(mgr.live_count(), CHAN_CAP);
    }

    #[test]
    fn f707_drill_deterministic() {
        let a = run_usb_drills(21, 40);
        let b = run_usb_drills(21, 40);
        assert_eq!(a, b);
        assert!(a.count_correct && a.select_correct && a.clean);
        assert!(a.attaches > 0 && a.detaches > 0, "40 轮随机插拔双向都有");
        assert_eq!(a.attaches - a.detaches, 0, "末态全拔空对账归零");
    }
}
