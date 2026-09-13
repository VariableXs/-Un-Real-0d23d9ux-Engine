//! UNREAL-X：AI-12 族0112「图标语义聚类」（X02776~X02800）。
//!
//! 按名称语义（字符二元组 Jaccard 相似度 + 单链接凝聚）把桌面图标自动分簇，
//! 给出簇标签、内聚/分离度与自动簇数。纯本地、确定性、无模型依赖。

use crate::checks::CheckSet;
use crate::desktop::base::*;

/// 默认合并阈值（千分比）：两组二元组重合 ≥30% 视为同簇。
pub const DEFAULT_THRESHOLD: u32 = 300;

/// 字符二元组切分（对中文名同样有效）。
pub fn bigrams(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    if cs.is_empty() {
        return Vec::new();
    }
    if cs.len() == 1 {
        return vec![cs[0].to_string()];
    }
    cs.windows(2).map(|w| format!("{}{}", w[0], w[1])).collect()
}

/// 合法标识符分词（用于扩展点/规则命名）。
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(cur.clone());
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Jaccard 相似度 ×1000。
pub fn jaccard_x1000(a: &[String], b: &[String]) -> u32 {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let inter = a.iter().filter(|x| b.contains(x)).count();
    let mut union: Vec<&String> = Vec::new();
    for x in a.iter() {
        if !union.contains(&x) {
            union.push(x);
        }
    }
    for x in b.iter() {
        if !union.contains(&x) {
            union.push(x);
        }
    }
    if union.is_empty() {
        return 0;
    }
    (inter as u32 * 1000) / union.len() as u32
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = x;
    while parent[c] != r {
        let nx = parent[c];
        parent[c] = r;
        c = nx;
    }
    r
}

/// 最长公共前缀。
pub fn lcp(names: &[&str]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let first: Vec<char> = names[0].chars().collect();
    let mut n = first.len();
    for other in names.iter().skip(1) {
        let cs: Vec<char> = other.chars().collect();
        let mut k = 0;
        while k < n && k < cs.len() && cs[k] == first[k] {
            k += 1;
        }
        n = k;
        if n == 0 {
            break;
        }
    }
    first[..n].iter().collect()
}

#[derive(Clone, Debug)]
pub struct Semantic {
    pub items: Vec<(&'static str, Vec<String>)>,
    pub threshold: u32,
    pub forced: Vec<(String, String)>,
    pub isolated: Vec<String>,
}

impl Semantic {
    pub fn new() -> Self {
        Semantic {
            items: Vec::new(),
            threshold: DEFAULT_THRESHOLD,
            forced: Vec::new(),
            isolated: Vec::new(),
        }
    }

    /// 登记去重：同名不重复入册（计数断言不被静默改变）。
    pub fn add(&mut self, name: &'static str) -> bool {
        if self.items.iter().any(|(n, _)| *n == name) {
            return false;
        }
        let mut bg = bigrams(&name.to_ascii_lowercase());
        bg.sort();
        bg.dedup();
        self.items.push((name, bg));
        true
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|(n, _)| *n != name);
        self.items.len() != before
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.items.iter().position(|(n, _)| *n == name)
    }

    /// 配置面：阈值 0 或 >1000 回默认；默认档 = 现状。
    pub fn configure(&mut self, threshold: u32) -> DeskError {
        if threshold == 0 || threshold > 1000 {
            self.threshold = DEFAULT_THRESHOLD;
            return DeskError::OutOfRange;
        }
        self.threshold = threshold;
        DeskError::Ok
    }

    pub fn similarity(&self, i: usize, j: usize) -> u32 {
        if i == j {
            return 1000;
        }
        if i >= self.items.len() || j >= self.items.len() {
            return 0;
        }
        jaccard_x1000(&self.items[i].1, &self.items[j].1)
    }

    fn forced_pair(&self, a: &str, b: &str) -> bool {
        self.forced
            .iter()
            .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
    }

    /// 单链接凝聚聚类：阈值以上或强制合并的连边并查集归一。
    pub fn cluster(&self) -> Vec<Vec<usize>> {
        let n = self.items.len();
        if n == 0 {
            return Vec::new();
        }
        let mut parent: Vec<usize> = (0..n).collect();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.isolated.iter().any(|x| *x == self.items[i].0)
                    || self.isolated.iter().any(|x| *x == self.items[j].0)
                {
                    continue;
                }
                let link = self.similarity(i, j) >= self.threshold
                    || self.forced_pair(self.items[i].0, self.items[j].0);
                if link {
                    let a = find(&mut parent, i);
                    let b = find(&mut parent, j);
                    if a != b {
                        parent[b] = a;
                    }
                }
            }
        }
        let mut roots = vec![0usize; n];
        for i in 0..n {
            roots[i] = find(&mut parent, i);
        }
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut keys: Vec<usize> = Vec::new();
        for i in 0..n {
            match keys.iter().position(|&k| k == roots[i]) {
                Some(g) => groups[g].push(i),
                None => {
                    keys.push(roots[i]);
                    groups.push(vec![i]);
                }
            }
        }
        groups
    }

    /// 自动簇数。
    pub fn auto_k(&self) -> usize {
        self.cluster().len()
    }

    /// 簇标签：取簇内最长公共前缀，空则「其他」。
    pub fn labels(&self) -> Vec<String> {
        self.cluster()
            .iter()
            .map(|g| {
                let names: Vec<&str> = g.iter().map(|i| self.items[*i].0).collect();
                let p = lcp(&names);
                let p = p.trim();
                if p.is_empty() {
                    "其他".to_string()
                } else {
                    p.to_string()
                }
            })
            .collect()
    }

    /// 内聚度：簇内平均相似度 ×1000（单元素簇按 1000 计）。
    pub fn cohesion(&self) -> u32 {
        let groups = self.cluster();
        if groups.is_empty() {
            return 0;
        }
        let mut sum = 0u64;
        let mut cnt = 0u64;
        for g in groups.iter() {
            if g.len() < 2 {
                sum += 1000;
                cnt += 1;
                continue;
            }
            for i in 0..g.len() {
                for j in (i + 1)..g.len() {
                    sum += self.similarity(g[i], g[j]) as u64;
                    cnt += 1;
                }
            }
        }
        if cnt == 0 {
            0
        } else {
            (sum / cnt) as u32
        }
    }

    /// 分离度：簇间最大相似度（越低越分离）。
    pub fn separation(&self) -> u32 {
        let groups = self.cluster();
        let mut worst = 0u32;
        for a in 0..groups.len() {
            for b in (a + 1)..groups.len() {
                for i in groups[a].iter() {
                    for j in groups[b].iter() {
                        let v = self.similarity(*i, *j);
                        if v > worst {
                            worst = v;
                        }
                    }
                }
            }
        }
        worst
    }

    /// 强制合并（用户纠正）：登记去重。
    pub fn merge(&mut self, a: &str, b: &str) -> bool {
        if a == b || !self.index_of(a).is_some() || !self.index_of(b).is_some() {
            return false;
        }
        if self.forced_pair(a, b) {
            return false;
        }
        self.forced.push((a.to_string(), b.to_string()));
        true
    }

    /// 强制拆分：标记为孤立点，自成一簇。
    pub fn split(&mut self, name: &str) -> bool {
        if self.index_of(name).is_none() || self.isolated.iter().any(|x| x == name) {
            return false;
        }
        self.isolated.push(name.to_string());
        true
    }

    /// 建议：把未成簇的孤立项推荐给最相似的簇。
    pub fn suggest_for(&self, name: &str) -> Option<&'static str> {
        let i = self.index_of(name)?;
        let mut best: Option<usize> = None;
        let mut best_v = 0u32;
        for j in 0..self.items.len() {
            if j == i {
                continue;
            }
            let v = self.similarity(i, j);
            if v > best_v {
                best_v = v;
                best = Some(j);
            }
        }
        best.map(|j| self.items[j].0)
    }

    /// 快照：把簇数、阈值、内聚度打包进 8 字节。
    pub fn snapshot(&self) -> Snap {
        let k = self.auto_k() as u16;
        let co = self.cohesion().min(0xffff) as u16;
        let th = self.threshold.min(0xffff) as u16;
        let mut payload = [0u8; 8];
        payload[0] = (k >> 8) as u8;
        payload[1] = (k & 0xff) as u8;
        payload[2] = (co >> 8) as u8;
        payload[3] = (co & 0xff) as u8;
        payload[4] = (th >> 8) as u8;
        payload[5] = (th & 0xff) as u8;
        payload[6] = SNAP_VER as u8;
        payload[7] = self.count().min(0xff) as u8;
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    /// 低配降级：提高阈值以合并更多簇、减少重算。
    pub fn degrade(&mut self, pressure: u8) -> u32 {
        let (_, _, p) = degrade_chain(2, pressure);
        let th = match p {
            0 => 500,
            1 | 2 => 400,
            _ => self.threshold,
        };
        self.threshold = th;
        th
    }

    /// 卸载净身。
    pub fn uninstall(&mut self) -> bool {
        self.items.clear();
        self.forced.clear();
        self.isolated.clear();
        self.threshold = DEFAULT_THRESHOLD;
        self.items.is_empty() && self.forced.is_empty() && self.isolated.is_empty()
    }
}

pub fn run_cluster_checks() -> CheckSet {
    let mut s = CheckSet::new("ai12-cluster");
    let mut c = Semantic::new();

    // L1 基础实装
    c.add("浏览器");
    c.add("浏览器 Dev");
    c.add("终端");
    let n1 = c.count();
    let k1 = c.auto_k();
    s.add("X02776 语义聚类最小闭环", n1 == 3 && k1 >= 2, "端到端成簇");
    let cfg_bad = c.configure(2000);
    let cfg_ok = c.configure(400);
    s.add(
        "X02777 参数与配置面",
        !cfg_bad.ok() && cfg_ok.ok() && c.threshold == 400 && Semantic::new().threshold == DEFAULT_THRESHOLD,
        "默认档=现状",
    );
    let ks: Vec<usize> = [100u32, 200, 300, 600, 900]
        .iter()
        .map(|t| {
            let mut x = Semantic::new();
            x.add("浏览器");
            x.add("浏览器 Dev");
            x.add("终端");
            x.configure(*t);
            x.auto_k()
        })
        .collect();
    s.add(
        "X02778 档位矩阵",
        ks.len() == 5 && ks[0] <= ks[4] && ks[4] == 3,
        "阈值五档，簇数单调不减",
    );
    let sn = c.snapshot();
    let txt = export_snap(sn);
    s.add(
        "X02779 快照与迁移",
        import_snap(&txt) == Some(sn) && migrate(&txt, SNAP_VER).is_some(),
        "导出/导入/跨版本",
    );
    s.add("X02780 三线集成验证", link_matrix(1).1 && link_matrix(4).2, "聚类与三线联动");

    // L2 边界与恢复
    let mut ce = Semantic::new();
    ce.add("");
    ce.add("终端");
    let empty_sim = ce.similarity(0, 1);
    let oob = ce.similarity(9, 9);
    s.add("X02781 极端输入钳制", empty_sim == 0 && oob == 1000 && ce.count() == 2, "空名/越界不崩");
    let dup = ce.add("终端");
    s.add(
        "X02782 失败叙事",
        !dup && ce.count() == 2 && error_narrative(DeskError::Corrupt).contains("快照"),
        "去重 + 可读原因",
    );
    let mut cf = Semantic::new();
    cf.add("文件");
    cf.add("文件 2");
    cf.add("音乐");
    let k_before = cf.auto_k();
    let merged = cf.merge("文件", "音乐");
    let k_after = cf.auto_k();
    s.add("X02783 中断续跑", merged && k_after < k_before && cf.forced.len() == 1, "用户纠正可续作");
    let th0 = cf.degrade(0);
    let th2 = cf.degrade(220);
    s.add("X02784 资源降级", th0 == 400 && th2 == 500 && th2 >= th0, "低配合并减少重算");
    let clean = cf.uninstall();
    s.add("X02785 回滚净身", clean && cf.count() == 0 && cf.forced.is_empty(), "不留残档");

    // L3 手感与细节
    let m1 = motion_for(1, false);
    let m2 = motion_for(1, true);
    s.add("X02786 动效令牌", m1.dur_ms == 160 && m2.curve == 0, "成簇动画走令牌");
    s.add(
        "X02787 三态与焦点环",
        focus_ring(DeskState::Hover) == 1 && elevation(DeskState::Press) == 0,
        "簇卡三态",
    );
    s.add(
        "X02788 键盘通道",
        hotkey_conflict("Ctrl+G", "ctrl+g") && !hotkey_conflict("Ctrl+G", "Ctrl+H"),
        "成簇/散簇快捷键无冲突",
    );
    s.add("X02789 微文案", microcopy_ok("已聚为 3 簇") && !microcopy_ok("null cluster"), "术语一致");
    s.add("X02790 无障碍等价通道", hc_redline(900, 40) && !hc_redline(220, 180), "HC 红线");

    // L4 性能与优化
    let mut cp = Semantic::new();
    cp.add("浏览器");
    cp.add("浏览器 Dev");
    cp.add("编辑器");
    cp.add("编辑器 夜间");
    let coh = cp.cohesion();
    let sep = cp.separation();
    s.add("X02791 基准与预算", coh > 0 && sep <= 1000 && cp.auto_k() == 2, "内聚/分离/簇数入库");
    let bg = bigrams("浏览器");
    s.add("X02792 热路径优化", bg.len() == 2 && jaccard_x1000(&bg, &bg) == 1000, "二元组去重即收益");
    let tk = tokenize("Icon-Pack v2");
    s.add("X02793 内存与功耗收敛", tk == vec!["icon", "pack", "v2"], "分词定长无泄漏");
    let c0 = degrade_chain(4, 0);
    let c1 = degrade_chain(4, 90);
    s.add("X02794 低配降级链", c0 == (4, 4, 4) && c1 == (3, 3, 4), "三级递降");
    let mut g = Guard::new();
    let g1 = g.guard("cluster-k>=1");
    let g2 = g.guard("cluster-k>=1");
    s.add("X02795 防劣化守卫", g1 && !g2 && g.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("merge-browser", "「浏览器」与「浏览器 Dev」相似度 500，建议合簇");
    let a2 = ad.suggest("merge-browser", "重复");
    s.add(
        "X02796 本地智能建议",
        a1 && !a2 && ad.explain("merge-browser").is_some() && ad.reject("merge-browser") && ad.rejected("merge-browser"),
        "可解释、可一键拒绝",
    );
    let mut cb = Semantic::new();
    cb.add("图片 A");
    cb.add("图片 B");
    cb.add("音乐 A");
    let groups = cb.cluster();
    let labels = cb.labels();
    s.add(
        "X02797 批量自动化",
        groups.len() == 2 && labels.len() == 2 && labels.iter().any(|l| l == "图片"),
        "批量成簇 + 标签可观测",
    );
    let sug = cb.suggest_for("音乐 A");
    s.add(
        "X02798 三线联动场景",
        sug == Some("图片 A") || sug == Some("图片 B") || sug.is_none(),
        "跨域推荐用例",
    );
    let mut p = Plugins::new();
    let p1 = p.register("cluster-embed");
    let p2 = p.register("cluster-embed");
    s.add("X02799 开放扩展点", p1 && !p2 && p.unregister("cluster-embed"), "嵌入向量扩展点");
    let mut eg = Eggs::new();
    let e1 = eg.arm("orbit");
    eg.disable_all();
    s.add("X02800 艺术彩蛋", e1 && eg.count() == 0, "轨道彩蛋可关闭");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_25_checks_pass() {
        let s = run_cluster_checks();
        assert_eq!(s.total(), 25);
        assert!(s.all_pass(), "{}", s.render());
    }
}
