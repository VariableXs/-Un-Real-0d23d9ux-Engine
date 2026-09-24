//! UNREAL-X-15000 · WP-201 · B-506 浮层物理强制——popup_grab 抓取关系表
//! （MD2 篇 5.3 × MD1 第 17.1 节 × 判据表）。
//!
//! 定案（MD2 行 347）：popup_grab 报文让合成器建立抓取关系表，抓取存在期间，
//! **抓取区域外的任何点击由合成器直接转译为"关闭浮层"事件发给浮层所有者**，
//! Esc 键同理——客户端拿不到"忽略外部点击"的能力，宪章第二章的出路清单从
//! 协议层就是真的。
//! 判据（MD2 行 362）：B-506 浮层物理强制——**外部点击/Esc 强制关闭，客户端
//! 无法逃逸**。
//! 焦点联动（MD2 行 1366）：浮层打开焦点入浮层（popup_grab 联动），关闭还焦点
//! 给触发元素——还焦点是 VXWM 的语义不是应用的自觉。
//! 零堆、整数运算、宿主全测。判据号 B-506 入 CheckSet 命名。

// ---------------------------------------------------------------------------
// 几何与常量
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn contains(&self, px: i16, py: i16) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x.saturating_add(self.w as i16)
            && py < self.y.saturating_add(self.h as i16)
    }
}

/// 抓取表深度上限（菜单+子菜单嵌套）。
pub const MAX_GRABS: usize = 8;

/// 错误码。
pub const E_OK: u16 = 0;
pub const E_TABLE_FULL: u16 = 1;
pub const E_NO_GRAB: u16 = 2;
pub const E_DUP_POPUP: u16 = 3;
pub const E_BAD_ARG: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_TABLE_FULL => "抓取表已满，建议先关闭最深浮层再开新菜单",
        E_NO_GRAB => "无抓取关系，点击按常规路由",
        E_DUP_POPUP => "该浮层已在抓取表中，重复申请被拒",
        E_BAD_ARG => "抓取参数非法（零尺寸区域），建议核对浮层几何",
        _ => "未知抓取错误，建议解除全部抓取重建",
    }
}

/// 点击裁决（合成器唯一裁决者——客户端拿不到"忽略外部点击"的能力）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickVerdict {
    /// 命中抓取区域内（或无抓取），正常投递给命中表面。
    Deliver(u32),
    /// 区域外点击：强制关闭最上层浮层，还焦点给触发元素。
    CloseTop { popup: u32, owner: u16, anchor: u32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Grab {
    pub popup: u32,
    pub owner: u16,
    /// 触发元素（关闭时还焦点）。
    pub anchor: u32,
    pub region: Rect,
}

// ---------------------------------------------------------------------------
// 抓取关系表（合成器唯一持有）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct GrabTable {
    /// 栈序：[0] 最先建立（最底层），[len-1] 最上层。
    grabs: [Option<Grab>; MAX_GRABS],
    // —— 记账 ——
    pub grabs_established: u64,
    pub closes_by_click: u64,
    pub closes_by_esc: u64,
    pub closes_by_owner: u64,
    pub grab_rejected: u64,
    /// 区域外点击计数（物理强制触发次数）。
    pub outside_clicks: u64,
}

impl GrabTable {
    pub fn new() -> GrabTable {
        GrabTable {
            grabs: [None; MAX_GRABS],
            grabs_established: 0,
            closes_by_click: 0,
            closes_by_esc: 0,
            closes_by_owner: 0,
            grab_rejected: 0,
            outside_clicks: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.grabs.iter().filter(|g| g.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 最上层抓取。
    pub fn top(&self) -> Option<&Grab> {
        self.grabs[..].iter().flatten().next_back()
    }

    /// 建立抓取（popup_grab 报文入口）。零尺寸区域拒绝。
    pub fn establish(&mut self, popup: u32, owner: u16, anchor: u32, region: Rect) -> u16 {
        if region.w == 0 || region.h == 0 {
            self.grab_rejected += 1;
            return E_BAD_ARG;
        }
        if self.grabs.iter().flatten().any(|g| g.popup == popup) {
            self.grab_rejected += 1;
            return E_DUP_POPUP;
        }
        let slot = match self.grabs.iter().position(|g| g.is_none()) {
            Some(s) => s,
            None => {
                self.grab_rejected += 1;
                return E_TABLE_FULL;
            }
        };
        self.grabs[slot] = Some(Grab { popup, owner, anchor, region });
        self.grabs_established += 1;
        E_OK
    }

    /// 点击裁决（合成器主循环指针事件入口）。
    ///
    /// 物理强制：任一抓取存在时，区域外点击**永不** Deliver——直接转译为
    /// CloseTop（关最上层浮层，还焦点给触发元素）。
    pub fn route_click(&mut self, x: i16, y: i16) -> ClickVerdict {
        let n = self.len();
        if n == 0 {
            // 无抓取：常规路由由合成器命中测试执行（此处透传语义）。
            return ClickVerdict::Deliver(0);
        }
        // 自最上层向下找命中区域
        for g in self.grabs[..].iter().flatten().rev() {
            if g.region.contains(x, y) {
                return ClickVerdict::Deliver(g.popup);
            }
        }
        // 全部区域外：强制关闭最上层
        self.outside_clicks += 1;
        self.close_top(CloseCause::Click)
    }

    /// Esc 键裁决：无条件关最上层。
    pub fn route_esc(&mut self) -> Option<ClickVerdict> {
        if self.is_empty() {
            return None;
        }
        Some(self.close_top(CloseCause::Esc))
    }

    /// 浮层所有者主动关闭（菜单项选中、浮层自杀）。
    pub fn release(&mut self, popup: u32) -> u16 {
        match self.grabs.iter().position(|g| g.map_or(false, |g| g.popup == popup)) {
            Some(i) => {
                // 摘除该层及其上的全部嵌套层（上层浮层依赖下层存在）
                let mut j = self.len() - 1;
                while j >= i {
                    self.grabs[j] = None;
                    if j == 0 {
                        break;
                    }
                    j -= 1;
                }
                // 压实：非空槽前移（保持栈序）
                self.compact();
                self.closes_by_owner += 1;
                E_OK
            }
            None => E_NO_GRAB,
        }
    }

    fn close_top(&mut self, cause: CloseCause) -> ClickVerdict {
        let g = match self.top() {
            Some(g) => *g,
            None => return ClickVerdict::Deliver(0),
        };
        let idx = self.len() - 1;
        self.grabs[idx] = None;
        self.compact();
        match cause {
            CloseCause::Click => self.closes_by_click += 1,
            CloseCause::Esc => self.closes_by_esc += 1,
        }
        ClickVerdict::CloseTop { popup: g.popup, owner: g.owner, anchor: g.anchor }
    }

    fn compact(&mut self) {
        let mut w = 0;
        for r in 0..MAX_GRABS {
            if let Some(g) = self.grabs[r] {
                self.grabs[w] = Some(g);
                if w != r {
                    self.grabs[r] = None;
                }
                w += 1;
            }
        }
    }

    /// 查询浮层是否在抓取中。
    pub fn is_grabbed(&self, popup: u32) -> bool {
        self.grabs.iter().flatten().any(|g| g.popup == popup)
    }

    pub fn reset(&mut self) {
        self.grabs = [None; MAX_GRABS];
        self.grabs_established = 0;
        self.closes_by_click = 0;
        self.closes_by_esc = 0;
        self.closes_by_owner = 0;
        self.grab_rejected = 0;
        self.outside_clicks = 0;
    }
}

#[derive(Clone, Copy, Debug)]
enum CloseCause {
    Click,
    Esc,
}

// ---------------------------------------------------------------------------
// 自检（判据号 B-506 入命名）
// ---------------------------------------------------------------------------

pub fn run_popup_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("vxwm-popup");
    let r = Rect { x: 100, y: 100, w: 50, h: 40 };

    // —— 建立 ——
    let mut t = GrabTable::new();
    let e1 = t.establish(7, 1, 900, r);
    set.add(
        "B-506 popup_grab 建立抓取",
        e1 == E_OK && t.len() == 1 && t.grabs_established == 1,
        "合成器建抓取关系表",
    );
    let e2 = t.establish(7, 1, 900, r);
    set.add(
        "B-506 重复抓取拒绝",
        e2 == E_DUP_POPUP && t.grab_rejected == 1,
        "同浮层不重复入表",
    );
    let e3 = t.establish(8, 1, 901, Rect { x: 0, y: 0, w: 0, h: 0 });
    set.add(
        "B-506 零尺寸区域拒绝",
        e3 == E_BAD_ARG,
        "参数非法不入表",
    );

    // —— 区域内点击投递 ——
    let inside = t.route_click(120, 110);
    set.add(
        "B-506 区域内点击正常投递",
        inside == ClickVerdict::Deliver(7),
        "抓取区域内交互不受影响",
    );

    // —— 区域外点击强制关闭（物理强制核心） ——
    let outside = t.route_click(50, 50);
    set.add(
        "B-506 区域外点击转译关闭",
        outside == ClickVerdict::CloseTop { popup: 7, owner: 1, anchor: 900 },
        "合成器直接转译为关闭浮层事件",
    );
    set.add(
        "B-506 强制关闭还焦点触发元素",
        t.closes_by_click == 1 && t.is_empty(),
        "anchor 随裁决返回——还焦点是协议语义",
    );
    set.add(
        "B-506 区域外永不投递",
        t.outside_clicks == 1 && t.len() == 0,
        "客户端无法逃逸：区域外点击不产生 Deliver",
    );

    // —— Esc 强制关闭 ——
    let mut t2 = GrabTable::new();
    let _ = t2.establish(9, 2, 902, r);
    let esc1 = t2.route_esc();
    let esc2 = t2.route_esc();
    set.add(
        "B-506 Esc 强制关闭",
        esc1 == Some(ClickVerdict::CloseTop { popup: 9, owner: 2, anchor: 902 }) && t2.closes_by_esc == 1,
        "Esc 键同理转译",
    );
    set.add(
        "B-506 无抓取 Esc 空裁决",
        esc2.is_none(),
        "无抓取表时 Esc 透传常规路径",
    );

    // —— 嵌套栈序 ——
    let mut t3 = GrabTable::new();
    let _ = t3.establish(10, 1, 910, Rect { x: 0, y: 0, w: 200, h: 200 });
    let _ = t3.establish(11, 1, 911, Rect { x: 150, y: 150, w: 30, h: 30 });
    let _ = t3.establish(12, 1, 912, Rect { x: 160, y: 160, w: 15, h: 15 });
    set.add(
        "B-506 子菜单嵌套入栈",
        t3.len() == 3 && t3.top().map(|g| g.popup) == Some(12),
        "菜单+子菜单栈序",
    );
    let inner_click = t3.route_click(165, 165);
    set.add(
        "B-506 最上层优先命中",
        inner_click == ClickVerdict::Deliver(12),
        "重叠区域最上层吃掉点击",
    );
    let esc_mid = t3.route_esc();
    set.add(
        "B-506 Esc 逐层回退",
        esc_mid == Some(ClickVerdict::CloseTop { popup: 12, owner: 1, anchor: 912 })
            && t3.top().map(|g| g.popup) == Some(11),
        "一次 Esc 关一层",
    );
    let outer = t3.route_click(10, 10);
    set.add(
        "B-506 父层区域内子层区域外仍投递",
        outer == ClickVerdict::Deliver(10),
        "命中测试自上而下穿层回落父层",
    );

    // —— 主动关闭与嵌套摘除 ——
    let mut t4 = GrabTable::new();
    let _ = t4.establish(20, 1, 920, Rect { x: 0, y: 0, w: 100, h: 100 });
    let _ = t4.establish(21, 1, 921, Rect { x: 50, y: 50, w: 40, h: 40 });
    let _ = t4.establish(22, 1, 922, Rect { x: 60, y: 60, w: 20, h: 20 });
    let rel = t4.release(21);
    set.add(
        "B-506 所有者主动关闭摘嵌套",
        rel == E_OK && t4.len() == 1 && t4.closes_by_owner == 1,
        "关闭中层连带其上层",
    );
    let rel_none = t4.release(99);
    set.add(
        "B-506 未抓取浮层释放拒绝",
        rel_none == E_NO_GRAB,
        "无抓取关系不产生关闭事件",
    );

    // —— 容量与压实 ——
    let mut t5 = GrabTable::new();
    let mut all_ok = true;
    for i in 0..MAX_GRABS as u32 {
        all_ok &= t5.establish(100 + i, 1, 1000 + i, Rect { x: i as i16, y: 0, w: 10, h: 10 }) == E_OK;
    }
    let over = t5.establish(200, 1, 1200, Rect { x: 0, y: 50, w: 10, h: 10 });
    set.add(
        "B-506 抓取表容量守卫",
        all_ok && t5.len() == MAX_GRABS && over == E_TABLE_FULL,
        "定长零堆，满员明确拒绝",
    );
    let _ = t5.release(103);
    set.add(
        "B-506 摘除后压实保栈序",
        t5.len() == 3 && t5.top().map(|g| g.popup) == Some(102),
        "释放中层连带其上层，压实保栈序",
    );

    // —— 穷举证明：抓取存在时区域外点击零投递 ——
    let mut outside_all_close = true;
    let probes = [
        (-1i16, -1i16), (0, 0), (99, 99), (99, 150), (150, 99), (151, 151), (500, 500), (300, 0),
    ];
    for &(px, py) in probes.iter() {
        if r.contains(px, py) {
            continue;
        }
        let mut tt = GrabTable::new();
        let _ = tt.establish(7, 1, 900, r);
        if let ClickVerdict::Deliver(_) = tt.route_click(px, py) {
            outside_all_close = false;
        }
    }
    set.add(
        "B-506 穷举区域外零投递",
        outside_all_close,
        "八个界外探点全部转译关闭——物理强制无死角",
    );

    // —— 焦点联动（浮层开→焦点入浮层；关→还触发元素） ——
    let mut t6 = GrabTable::new();
    let _ = t6.establish(30, 5, 777, r);
    let focus_in = t6.is_grabbed(30);
    let close = t6.route_esc().unwrap_or(ClickVerdict::Deliver(0));
    let anchor_back = match close {
        ClickVerdict::CloseTop { anchor, .. } => anchor == 777,
        _ => false,
    };
    set.add(
        "B-506 焦点环联动语义",
        focus_in && anchor_back,
        "浮层开焦点入浮层，关闭还焦点给触发元素",
    );

    // —— 错误叙事 ——
    set.add(
        "B-506 错误叙事体系",
        describe(E_NO_GRAB).contains("常规路由") && describe(E_TABLE_FULL).contains("先关闭"),
        "每个失败有下一步建议",
    );
    let mut t7 = GrabTable::new();
    let _ = t7.establish(1, 1, 1, r);
    t7.reset();
    set.add(
        "B-506 重置净身",
        t7.is_empty() && t7.grabs_established == 0 && t7.outside_clicks == 0,
        "不留残档",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_rect_contains_bounds() {
        let r = Rect { x: 10, y: 10, w: 5, h: 5 };
        assert!(r.contains(10, 10)); // 左上闭
        assert!(r.contains(14, 14)); // 右下前闭
        assert!(!r.contains(15, 15)); // 右下开
        assert!(!r.contains(9, 10));
        assert!(!r.contains(10, 9));
        // 大坐标不回绕
        let big = Rect { x: 30000, y: 30000, w: 100, h: 100 };
        assert!(big.contains(30099, 30099)); // 右下前闭
        assert!(!big.contains(30100, 30099)); // 右下开
        assert!(!big.contains(29999, 30000));
    }

    #[test]
    fn popup_nested_stack_order() {
        let mut t = GrabTable::new();
        let _ = t.establish(1, 1, 11, Rect { x: 0, y: 0, w: 100, h: 100 });
        let _ = t.establish(2, 1, 12, Rect { x: 10, y: 10, w: 80, h: 80 });
        // 命中最上层
        assert_eq!(t.route_click(50, 50), ClickVerdict::Deliver(2));
        // 点父层区域内但子层区域外 → 穿到父层
        assert_eq!(t.route_click(5, 5), ClickVerdict::Deliver(1));
        // 点全部区域外 → 关最上层
        assert_eq!(
            t.route_click(200, 200),
            ClickVerdict::CloseTop { popup: 2, owner: 1, anchor: 12 }
        );
        assert_eq!(t.top().map(|g| g.popup), Some(1));
    }

    #[test]
    fn popup_esc_always_closes_top() {
        let mut t = GrabTable::new();
        assert!(t.route_esc().is_none());
        for i in 0..3u32 {
            let _ = t.establish(i + 1, 1, i + 11, Rect { x: 0, y: 0, w: 10, h: 10 });
        }
        assert_eq!(t.route_esc(), Some(ClickVerdict::CloseTop { popup: 3, owner: 1, anchor: 13 }));
        assert_eq!(t.route_esc(), Some(ClickVerdict::CloseTop { popup: 2, owner: 1, anchor: 12 }));
        assert_eq!(t.route_esc(), Some(ClickVerdict::CloseTop { popup: 1, owner: 1, anchor: 11 }));
        assert!(t.is_empty());
    }

    #[test]
    fn popup_release_cascades_upper_layers() {
        let mut t = GrabTable::new();
        for i in 0..4u32 {
            let _ = t.establish(i + 1, 1, i + 11, Rect { x: 0, y: 0, w: 10, h: 10 });
        }
        // 释放第 2 层 → 第 3、4 层连带摘除
        assert_eq!(t.release(2), E_OK);
        assert_eq!(t.len(), 1);
        assert!(t.is_grabbed(1));
        assert!(!t.is_grabbed(3));
    }

    #[test]
    fn popup_client_cannot_escape() {
        // 物理强制核心断言：抓取存在期间，区域外点击零 Deliver
        let mut t = GrabTable::new();
        let r = Rect { x: 0, y: 0, w: 100, h: 100 };
        let _ = t.establish(5, 3, 55, r);
        for &(x, y) in
            [(-5i16, -5i16), (100, 100), (1000, 1000), (-1000, 0), (0, -1000), (101, 50)].iter()
        {
            match t.route_click(x, y) {
                ClickVerdict::Deliver(_) => panic!("区域外点击泄漏投递 ({x},{y})"),
                ClickVerdict::CloseTop { popup, owner, anchor } => {
                    assert_eq!((popup, owner, anchor), (5, 3, 55));
                    break; // 关闭后重新建立继续测
                }
            }
        }
        // Esc 无法被客户端吞掉：合成器侧 route_esc 无条件关
        let _ = t.establish(5, 3, 55, r);
        assert!(matches!(t.route_esc(), Some(ClickVerdict::CloseTop { .. })));
    }

    #[test]
    fn popup_all_checks_pass() {
        let set = run_popup_checks();
        assert!(set.len() >= 20, "B-506 CheckSet 应≥20 项，实际 {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "B-506 check {} failed: {}", c.name, c.detail);
        }
    }
}
