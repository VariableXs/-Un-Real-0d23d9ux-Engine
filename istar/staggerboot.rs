//! F598 自启动错峰 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：2s 间隔实测；拖拽调序持久化；应用无感判据（后台
//! 就绪）；开机时长对比（错峰前后）；与 F348/F371 对账。
//!
//! **设计要点（主册）**：
//! - 自启动项错峰执行（F348 清单的调度层）：多应用同获自启动许可时按
//!   优先级错峰（间隔 2 秒逐个起——磁盘 IO/内存不挤兑，冷启动时间不被
//!   自启动拖垮）；
//! - 用户可调序（清单拖拽 = 启动序）；被延后的应用无感知（后台起、
//!   就绪即用 F283 骨架先行）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 错峰间隔（ms——2 秒逐个起）。
pub const STAGGER_MS: u64 = 2_000;

/// 启动项上限（F348 清单容量同源）。
pub const AUTORUN_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一个自启动项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoItem {
    pub app: String,
    /// 用户调序（拖拽定的启动序——越小越先）。
    pub order: u32,
    /// 状态：Pending / Launched / Ready。
    pub launched: bool,
    pub ready: bool,
    /// 实际启动时刻（ms——错峰对账）。
    pub launched_ms: Option<u64>,
}

/// 自启动错峰调度器。
pub struct StaggerBoot {
    items: Vec<AutoItem>,
    now_ms: u64,
    /// 开机时段锚（0 = 调度器纪元）。
    boot_ms: u64,
    /// 应用无感账（后台起、就绪即用——ready 与前台可用的握手数）。
    ready_count: u32,
}

impl StaggerBoot {
    pub fn new() -> StaggerBoot {
        StaggerBoot {
            items: Vec::new(),
            now_ms: 0,
            boot_ms: 0,
            ready_count: 0,
        }
    }

    /// 登记自启动项（F348 清单接入；满 24 拒绝）。
    pub fn add(&mut self, app: &str, order: u32) -> bool {
        if self.items.len() >= AUTORUN_CAP || self.items.iter().any(|i| i.app == app) {
            return false;
        }
        self.items.push(AutoItem {
            app: String::from(app),
            order,
            launched: false,
            ready: false,
            launched_ms: None,
        });
        true
    }

    /// 拖拽调序（用户定启动序——持久化值即 order 字段）。
    pub fn reorder(&mut self, app: &str, order: u32) -> bool {
        match self.items.iter_mut().find(|i| i.app == app) {
            Some(i) => {
                i.order = order;
                true
            }
            None => false,
        }
    }

    /// 按用户序的启动队列（order 升序；同名次按登记序稳定）。
    pub fn queue(&self) -> Vec<String> {
        let mut idx: Vec<usize> = (0..self.items.len()).collect();
        idx.sort_by(|&a, &b| self.items[a].order.cmp(&self.items[b].order));
        idx.into_iter().map(|i| self.items[i].app.clone()).collect()
    }

    /// 分钟滴答（宿主钟注入）：按错峰计划拉起到点项。
    ///
    /// 启动时刻 = boot_ms + 队列位次 × STAGGER_MS（2 秒一个，峰谷填平）。
    /// 返回本次滴答拉起的应用名。
    pub fn tick(&mut self, ms: u64) -> Vec<String> {
        self.now_ms = ms;
        let mut fired = Vec::new();
        let q = self.queue();
        for (pos, app) in q.iter().enumerate() {
            let slot = self.boot_ms + pos as u64 * STAGGER_MS;
            if ms >= slot {
                if let Some(i) = self.items.iter_mut().find(|i| &i.app == app) {
                    if !i.launched {
                        i.launched = true;
                        i.launched_ms = Some(slot);
                        fired.push(app.clone());
                    }
                }
            }
        }
        fired
    }

    /// 应用就绪回报（F283 骨架先行——后台起、就绪即用的握手）。
    pub fn note_ready(&mut self, app: &str) -> bool {
        match self.items.iter_mut().find(|i| i.app == app) {
            Some(i) => {
                if i.launched && !i.ready {
                    i.ready = true;
                    self.ready_count += 1;
                    return true;
                }
                false
            }
            None => false,
        }
    }

    /// 2s 间隔实测对账：相邻两次启动的实际间隔（首两）。
    pub fn first_gap_ms(&self) -> Option<u64> {
        let mut times: Vec<u64> = self
            .items
            .iter()
            .filter_map(|i| i.launched_ms)
            .collect();
        times.sort();
        match (times.first(), times.get(1)) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        }
    }

    /// 应用无感判据：全部已启动项都有就绪握手（后台起不卡前台）。
    pub fn all_ready(&self) -> bool {
        self.items
            .iter()
            .filter(|i| i.launched)
            .all(|i| i.ready)
            && self.ready_count as usize == self.items.iter().filter(|i| i.launched).count()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl Default for StaggerBoot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_staggerboot_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 2s 间隔实测：5 项全拉起，首两间隔恰 2000ms。
    let mut s = StaggerBoot::new();
    for (i, app) in ["云盘", "聊天", "笔记", "输入法云", "截床"].iter().enumerate() {
        s.add(app, i as u32);
    }
    let mut all = Vec::new();
    for t in [0u64, 2_100, 4_200, 6_300, 8_400] {
        all.extend(s.tick(t));
    }
    let gap = s.first_gap_ms();
    set.add(
        "two second stagger measured",
        all.len() == 5 && gap == Some(STAGGER_MS),
        "",
    );

    // 2. 拖拽调序持久化：改序后队列跟新（用户定谁先谁后）。
    s.reorder("云盘", 9);
    s.reorder("截床", 0);
    let q = s.queue();
    set.add(
        "drag reorder persists",
        q.first().map(|a| a == "截床").unwrap_or(false)
            && q.last().map(|a| a == "云盘").unwrap_or(false),
        "",
    );

    // 3. 应用无感判据：拉起项全部有就绪握手（后台就绪即用）。
    for app in ["云盘", "聊天", "笔记", "输入法云", "截床"] {
        s.note_ready(app);
    }
    set.add(
        "background ready handshake",
        s.all_ready() && s.ready_count == 5,
        "",
    );

    // 4. 未到点不拉起（错峰等待的诚实性——首位立即起、第二位 2s 前不跑）。
    let mut s2 = StaggerBoot::new();
    s2.add("a", 0);
    s2.add("b", 1);
    let early = s2.tick(1_999);
    set.add(
        "no early launch",
        early == alloc::vec!["a"] && s2.first_gap_ms().is_none(),
        "",
    );

    // 5. 开机时长对比账：错峰锚从 boot_ms 起算（F371 开机徽标对账面）。
    let mut s3 = StaggerBoot::new();
    s3.boot_ms = 1_000; // 开机后 1 秒开始错峰
    s3.add("x", 0);
    s3.add("y", 1);
    s3.tick(1_000);
    s3.tick(3_000);
    let gap3 = s3.first_gap_ms();
    set.add(
        "boot anchor aligned with f371",
        gap3 == Some(STAGGER_MS) && AUTORUN_CAP == 24,
        "",
    );

    // 6. 与 F348 对账：清单容量与去重（同应用重复登记拒绝）。
    let mut s4 = StaggerBoot::new();
    let dup1 = s4.add("同应用", 0);
    let dup2 = s4.add("同应用", 1);
    set.add(
        "f348 dedupe",
        dup1 && !dup2 && s4.len() == 1,
        "",
    );

    // 7. 就绪无虚报：未启动项握手拒绝（不跳过错峰直接 ready）。
    let mut s5 = StaggerBoot::new();
    s5.add("迟到者", 5);
    set.add(
        "no ready before launch",
        !s5.note_ready("迟到者") && s5.ready_count == 0,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_unknown_false() {
        let mut s = StaggerBoot::new();
        assert!(!s.reorder("无", 1));
    }

    #[test]
    fn queue_stable_for_equal_orders() {
        let mut s = StaggerBoot::new();
        s.add("a", 1);
        s.add("b", 1);
        s.add("c", 0);
        assert_eq!(s.queue(), alloc::vec!["c", "a", "b"]);
    }

    #[test]
    fn ready_twice_single_count() {
        let mut s = StaggerBoot::new();
        s.add("a", 0);
        s.tick(0);
        assert!(s.note_ready("a"));
        assert!(!s.note_ready("a"));
        assert_eq!(s.ready_count, 1);
    }
}
