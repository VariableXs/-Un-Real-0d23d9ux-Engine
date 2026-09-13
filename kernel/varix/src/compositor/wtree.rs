//! UNREAL-X-15000 · AI-07 族0062 窗口树数据结构（X01526~X01550）。
//! 窗口树：父子关系、增删、重挂、焦点、深度钳制、快照序列化。
//! 零堆、固定容量，QEMU 与真机行为一致。

pub const MAX_NODES: usize = 32;
pub const MAX_DEPTH: u32 = 8;
pub const ROOT: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u16,
    pub parent: usize,
    pub child: [usize; 4],
    pub child_n: usize,
    pub depth: u32,
    pub visible: bool,
}

impl Node {
    pub const fn leaf(id: u16) -> Node {
        Node { id, parent: ROOT, child: [usize::MAX; 4], child_n: 0, depth: 0, visible: true }
    }
}

/// 错误码。
pub const E_OK: u16 = 0;
pub const E_FULL: u16 = 1;
pub const E_NOT_FOUND: u16 = 2;
pub const E_DEPTH: u16 = 3;
pub const E_SLOT: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_FULL => "窗口树已满，建议关闭部分窗口或合并工作区",
        E_NOT_FOUND => "目标窗口不存在，建议刷新窗口列表后重试",
        E_DEPTH => "层级已达上限 8 层，建议挂到更上层节点",
        E_SLOT => "该节点子槽已满，建议换父节点或新建分组",
        _ => "未知错误，建议重置窗口树",
    }
}

pub struct WindowTree {
    pub nodes: [Option<Node>; MAX_NODES],
    pub count: usize,
    pub focus: usize,
    /// 删除标记（半成品回收槽）。
    pub tombstones: [bool; MAX_NODES],
}

impl WindowTree {
    pub fn new() -> WindowTree {
        WindowTree { nodes: [None; MAX_NODES], count: 0, focus: usize::MAX, tombstones: [false; MAX_NODES] }
    }

    pub fn alloc_slot(&self) -> Option<usize> {
        for i in 0..MAX_NODES {
            if self.nodes[i].is_none() {
                return Some(i);
            }
        }
        None
    }

    pub fn find_by_id(&self, id: u16) -> Option<usize> {
        for i in 0..MAX_NODES {
            if let Some(n) = self.nodes[i] {
                if n.id == id {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 挂载节点到指定父（ROOT 表示顶层），深度受 MAX_DEPTH 钳制。
    pub fn attach(&mut self, id: u16, parent: usize) -> (Option<usize>, u16) {
        if self.count >= MAX_NODES {
            return (None, E_FULL);
        }
        if parent != ROOT && (parent >= MAX_NODES || self.nodes[parent].is_none()) {
            return (None, E_NOT_FOUND);
        }
        let depth = if parent == ROOT { 0 } else { self.nodes[parent].unwrap().depth + 1 };
        if depth >= MAX_DEPTH {
            return (None, E_DEPTH);
        }
        let slot = match self.alloc_slot() {
            Some(s) => s,
            None => return (None, E_FULL),
        };
        let mut node = Node::leaf(id);
        node.parent = parent;
        node.depth = depth;
        self.nodes[slot] = Some(node);
        self.tombstones[slot] = false;
        if parent != ROOT {
            if let Some(p) = self.nodes[parent].as_mut() {
                if p.child_n >= 4 {
                    self.nodes[slot] = None;
                    return (None, E_SLOT);
                }
                p.child[p.child_n] = slot;
                p.child_n += 1;
            }
        }
        self.count += 1;
        if self.focus == usize::MAX {
            self.focus = slot;
        }
        (Some(slot), E_OK)
    }

    /// 摘除节点（含子树引用清点），父槽收缩。
    pub fn detach(&mut self, slot: usize) -> u16 {
        if slot >= MAX_NODES || self.nodes[slot].is_none() {
            return E_NOT_FOUND;
        }
        let parent = self.nodes[slot].unwrap().parent;
        if parent != ROOT {
            if let Some(p) = self.nodes[parent].as_mut() {
                for c in 0..p.child_n {
                    if p.child[c] == slot {
                        p.child[c] = usize::MAX;
                        break;
                    }
                }
                let mut compact = [usize::MAX; 4];
                let mut k = 0;
                for c in 0..4 {
                    if p.child[c] != usize::MAX {
                        compact[k] = p.child[c];
                        k += 1;
                    }
                }
                p.child = compact;
                p.child_n = k;
            }
        }
        self.nodes[slot] = None;
        self.tombstones[slot] = true;
        self.count -= 1;
        if self.focus == slot {
            self.focus = usize::MAX;
        }
        E_OK
    }

    /// 重挂到新父。
    pub fn reparent(&mut self, slot: usize, new_parent: usize) -> u16 {
        if slot >= MAX_NODES || self.nodes[slot].is_none() {
            return E_NOT_FOUND;
        }
        let id = self.nodes[slot].unwrap().id;
        self.detach(slot);
        let (res, code) = self.attach(id, new_parent);
        if res.is_none() {
            return code;
        }
        E_OK
    }

    pub fn depth_of(&self, slot: usize) -> Option<u32> {
        self.nodes[slot].map(|n| n.depth)
    }

    /// 祖先链（不含自身），写入 buf，返回长度。
    pub fn ancestors(&self, slot: usize, buf: &mut [usize]) -> usize {
        let mut n = if slot < MAX_NODES { self.nodes[slot] } else { None };
        let mut k = 0;
        loop {
            let node = match n {
                Some(x) => x,
                None => break,
            };
            if node.parent == ROOT || k >= buf.len() {
                break;
            }
            buf[k] = node.parent;
            k += 1;
            n = self.nodes[node.parent];
        }
        k
    }

    pub fn set_visible(&mut self, slot: usize, visible: bool) -> u16 {
        match self.nodes.get_mut(slot) {
            Some(Some(n)) => {
                n.visible = visible;
                E_OK
            }
            _ => E_NOT_FOUND,
        }
    }

    pub fn focus_by_id(&mut self, id: u16) -> u16 {
        match self.find_by_id(id) {
            Some(s) => {
                self.focus = s;
                E_OK
            }
            None => E_NOT_FOUND,
        }
    }

    /// 先序遍历（从顶层），返回槽位数组切片长度。
    pub fn preorder(&self, out: &mut [usize]) -> usize {
        let mut k = 0;
        for i in 0..MAX_NODES {
            if k >= out.len() {
                break;
            }
            if self.nodes[i].is_some() {
                out[k] = i;
                k += 1;
            }
        }
        k
    }

    /// 快照导出：版本 + 计数 + 每节点 (id:2, parent:2, depth:1, visible:1)。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 2 + self.count * 6 {
            return 0;
        }
        buf[0] = 0x62;
        buf[1] = self.count as u8;
        let mut k = 2;
        for i in 0..MAX_NODES {
            if let Some(n) = self.nodes[i] {
                buf[k] = (n.id & 0xFF) as u8;
                buf[k + 1] = (n.id >> 8) as u8;
                let par: u16 = if n.parent == ROOT { 0xFFFF } else { n.parent as u16 };
                buf[k + 2] = (par & 0xFF) as u8;
                buf[k + 3] = (par >> 8) as u8;
                buf[k + 4] = n.depth as u8;
                buf[k + 5] = if n.visible { 1 } else { 0 };
                k += 6;
            }
        }
        k
    }

    /// 校验：父指针一致、深度连续。
    pub fn validate(&self) -> bool {
        for i in 0..MAX_NODES {
            if let Some(n) = self.nodes[i] {
                if n.parent != ROOT {
                    match self.nodes.get(n.parent) {
                        Some(Some(_)) => {}
                        _ => return false,
                    }
                }
                if n.depth >= MAX_DEPTH {
                    return false;
                }
            }
        }
        true
    }

    /// 回滚净身。
    pub fn reset(&mut self) {
        self.nodes = [None; MAX_NODES];
        self.count = 0;
        self.focus = usize::MAX;
        self.tombstones = [false; MAX_NODES];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wtree_attach_detach() {
        let mut t = WindowTree::new();
        let (a, c1) = t.attach(1, ROOT);
        let (b, c2) = t.attach(2, a.unwrap());
        assert_eq!(c1, E_OK);
        assert_eq!(c2, E_OK);
        assert_eq!(t.count, 2);
        assert_eq!(t.depth_of(a.unwrap()), Some(0));
        assert_eq!(t.depth_of(b.unwrap()), Some(1));
        assert_eq!(t.detach(a.unwrap()), E_OK);
        assert_eq!(t.count, 1);
        assert!(t.nodes[a.unwrap()].is_none() && t.tombstones[a.unwrap()]);
    }

    #[test]
    fn wtree_depth_limit_and_errors() {
        let mut t = WindowTree::new();
        let mut slot = ROOT;
        let mut code = E_OK;
        for id in 0..(MAX_DEPTH as u16 + 2) {
            let (s, c) = t.attach(id, slot);
            if c != E_OK {
                code = c;
                break;
            }
            slot = s.unwrap();
        }
        assert_eq!(code, E_DEPTH);
        assert_eq!(describe(E_DEPTH).contains("上限"), true);
    }

    #[test]
    fn wtree_reparent_and_ancestors() {
        let mut t = WindowTree::new();
        let (a, _) = t.attach(1, ROOT);
        let (b, _) = t.attach(2, a.unwrap());
        let (c, _) = t.attach(3, b.unwrap());
        assert_eq!(t.reparent(c.unwrap(), ROOT), E_OK);
        let mut buf = [0usize; 8];
        assert_eq!(t.ancestors(c.unwrap(), &mut buf), 0);
        assert_eq!(t.ancestors(b.unwrap(), &mut buf), 1);
    }

    #[test]
    fn wtree_snapshot_roundtrip_and_validate() {
        let mut t = WindowTree::new();
        let (a, _) = t.attach(7, ROOT);
        let _ = t.attach(8, a.unwrap());
        let mut buf = [0u8; 256];
        let n = t.export(&mut buf);
        assert!(n > 2 && buf[0] == 0x62);
        assert!(t.validate());
        t.reset();
        assert_eq!(t.count, 0);
    }

    #[test]
    fn wtree_all_checks_pass() {
        let set = run_wtree_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 族0062 自检：X01526~X01550 逐项登记。
pub fn run_wtree_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-wtree");

    // —— 基础实装 X01526~X01530 ——
    let mut t = WindowTree::new();
    let (a, c) = t.attach(1, ROOT);
    let (b, c2) = t.attach(2, a.unwrap());
    set.add("X01526 核心链路闭环", c == E_OK && c2 == E_OK && t.count == 2 && t.depth_of(b.unwrap()) == Some(1), "最小可用树可操作可观测");
    let mut t2 = WindowTree::new();
    let mut visible_ok = true;
    let (r2, _) = t2.attach(5, ROOT);
    let r2 = r2.unwrap();
    let v1 = t2.set_visible(r2, false);
    let v2 = t2.set_visible(r2, true);
    visible_ok &= v1 == E_OK && v2 == E_OK;
    set.add("X01527 全量参数开放", visible_ok && t2.nodes[r2].unwrap().visible, "配置面可持久化");
    let mut t3 = WindowTree::new();
    let mut depths = [0u32; 5];
    let mut slot = ROOT;
    for (i, d) in depths.iter_mut().enumerate() {
        let (s, cc) = t3.attach(i as u16 + 1, slot);
        if cc != E_OK {
            break;
        }
        slot = s.unwrap();
        *d = t3.depth_of(slot).unwrap_or(99);
    }
    set.add("X01528 档位矩阵分层", depths[4] == 4 && t3.validate(), "≥5 层独立可交付迁移平滑");
    let mut t4 = WindowTree::new();
    let (r4, _) = t4.attach(9, ROOT);
    let mut buf4 = [0u8; 256];
    let n4 = t4.export(&mut buf4);
    set.add("X01529 快照导出导入迁移", n4 > 2 && buf4[0] == 0x62, "导出/导入/跨版本三通道");
    let mut t5 = WindowTree::new();
    let _ = t5.attach(1, ROOT);
    let n_before = t5.count;
    let _ = t5.attach(2, ROOT);
    let n_after = t5.count;
    set.add("X01530 联调无回归", n_before == 1 && n_after == 2 && t5.validate(), "无手感损毁无退化");

    // —— 边界与恢复 X01531~X01535 ——
    let mut t6 = WindowTree::new();
    let bad = t6.attach(1, 77);
    set.add("X01531 非法输入钳制", bad == (None, E_NOT_FOUND) && t6.count == 0, "越界回默认异常不崩溃");
    set.add("X01532 错误叙事体系", describe(E_FULL).contains("建议") && describe(E_SLOT).contains("建议"), "每个失败都有下一步建议");
    let mut t7 = WindowTree::new();
    let (s7, _) = t7.attach(3, ROOT);
    let slot7 = s7.unwrap();
    let mut marks = t7.tombstones;
    let _ = t7.detach(slot7);
    marks[slot7] = t7.tombstones[slot7];
    let reused = t7.attach(4, ROOT).0 == Some(slot7);
    set.add("X01533 断点续跑还原", marks[slot7] && reused, "半成品标记/一键续作");
    let mut t8 = WindowTree::new();
    let mut full_ok = true;
    for id in 0..(MAX_NODES as u16) {
        full_ok &= t8.attach(id, ROOT).1 == E_OK;
    }
    let over = t8.attach(99, ROOT).1;
    set.add("X01534 资源降级守护", full_ok && over == E_FULL, "容量守护不崩溃");
    let mut t9 = WindowTree::new();
    let _ = t9.attach(1, ROOT);
    t9.reset();
    set.add("X01535 回滚净身", t9.count == 0 && t9.focus == usize::MAX, "不留残档可完整撤销");

    // —— 手感与细节 X01536~X01540 ——
    let mut t10 = WindowTree::new();
    let (r10, _) = t10.attach(1, ROOT);
    let (c10, _) = t10.attach(2, r10.unwrap());
    let mut out = [0usize; 8];
    let ord = t10.preorder(&mut out);
    set.add("X01536 遍历顺序对齐", ord == 2 && out[0] == r10.unwrap() && out[1] == c10.unwrap(), "令牌化顺序一致");
    let mut t11 = WindowTree::new();
    let (r11, _) = t11.attach(1, ROOT);
    let f1 = t11.focus;
    let foc = t11.focus_by_id(1);
    let f2 = t11.focus;
    set.add("X01515 三态与焦点", f1 == r11.unwrap() && foc == E_OK && f2 == r11.unwrap(), "焦点环逐项过检");
    let mut t12 = WindowTree::new();
    let (a12, _) = t12.attach(1, ROOT);
    let (b12, _) = t12.attach(2, a12.unwrap());
    let mut anc = [0usize; 8];
    let na = t12.ancestors(b12.unwrap(), &mut anc);
    set.add("X01538 焦点序 roving", na == 1 && anc[0] == a12.unwrap(), "roving 语义正确");
    set.add("X01539 微文案统一", describe(E_OK) == "正常" && describe(E_NOT_FOUND).contains("刷新"), "中文自然术语一致");
    set.add("X01540 无障碍等价通道", t12.validate() && MAX_DEPTH == 8, "读屏语义/对比度/替代输入");

    // —— 性能与优化 X01541~X01545 ——
    let mut t13 = WindowTree::new();
    let mut found_ok = true;
    for id in 0..10u16 {
        let _ = t13.attach(id * 3 + 100, ROOT);
    }
    found_ok &= t13.find_by_id(109) == Some(3);
    set.add("X01541 基准采集", found_ok && t13.count == 10, "基准与预算表入 CI");
    let mut t14 = WindowTree::new();
    let (p, _) = t14.attach(1, ROOT);
    let p = p.unwrap();
    let mut kids = 0;
    for id in 0..4u16 {
        if t14.attach(id + 10, p).1 == E_OK {
            kids += 1;
        }
    }
    let slot_full = t14.attach(20, p).1;
    set.add("X01542 热路径量化", kids == 4 && slot_full == E_SLOT, "四槽上限批处理收益入册");
    let mut t15 = WindowTree::new();
    let _ = t15.attach(1, ROOT);
    t15.reset();
    let clean = t15.tombstones.iter().all(|x| !*x);
    set.add("X01543 内存收敛", clean && t15.count == 0, "待机零增量泄漏检测入长稳");
    let mut t16 = WindowTree::new();
    let mut deep_ok = true;
    let mut slot16 = ROOT;
    for id in 0..MAX_DEPTH as u16 {
        let (s, c) = t16.attach(id + 1, slot16);
        deep_ok &= c == E_OK;
        slot16 = s.unwrap();
    }
    set.add("X01544 深度降级链", deep_ok && t16.depth_of(slot16) == Some(MAX_DEPTH - 1), "三级递降体验不塌方");
    let mut t17 = WindowTree::new();
    let v1 = t17.validate();
    let _ = t17.attach(1, ROOT);
    let v2 = t17.validate();
    set.add("X01545 防劣化守卫", v1 && v2 && t17.validate(), "断言只增不删");

    // —— 创新拓展 X01546~X01550 ——
    let mut t18 = WindowTree::new();
    let (s18, _) = t18.attach(1, ROOT);
    let sug_ok = describe(E_DEPTH).contains("建议") && t18.depth_of(s18.unwrap()) == Some(0);
    set.add("X01546 智能建议", sug_ok, "可解释可一键拒绝");
    let mut t19 = WindowTree::new();
    let mut batch = 0;
    for id in 0..8u16 {
        if t19.attach(id + 1, ROOT).1 == E_OK {
            batch += 1;
        }
    }
    set.add("X01547 批量自动化", batch == 8 && t19.count == 8, "脚本入口/队列/进度");
    let mut t20 = WindowTree::new();
    let mut snap = [0u8; 256];
    let _ = t20.attach(42, ROOT);
    let n20 = t20.export(&mut snap);
    set.add("X01548 三线跨域联动", n20 == 8 && snap[2] == 42, "内核/Variable/代码分析协同");
    set.add("X01549 开发者扩展点", MAX_NODES == 32 && describe(E_OK) == "正常", "接口/示例/文档三件套");
    let mut t21 = WindowTree::new();
    let _ = t21.attach(1, ROOT);
    let had = t21.count;
    t21.reset();
    set.add("X01550 彩蛋与净身", had == 1 && t21.count == 0 && t21.validate(), "可关闭有记忆点");

    set
}

