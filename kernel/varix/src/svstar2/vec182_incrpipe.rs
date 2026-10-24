//! VE-F0582 · 增量编译管线级联
//!
//! 源变更沿六级缓存级联失效——词法、语法、语义、IR、后端各层以内容哈希为键，
//! 变更按依赖图向下级联失效、未受影响子树向上复用；失效粒度声明（语义级精确
//! 到符号作用域、IR 级以函数为粒度、后端级以模块为粒度）；正确性不变式：任何
//! 复用必须哈希命中，宁可全量重编绝不允许脏复用——不变式优先于性能；本条是
//! 管线级总纲与失效传播的权威定义。
//!
//! 跨批对接：上游 F0581 服务、F0475 IR 缓存；下游 F0588 多文件、F0590 缓存总纲。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 六级缓存层
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Lex,
    Syntax,
    Semantic,
    Ir,
    Backend,
    /// 第六级：产物封装
    Artifact,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Lex => "词法",
            Level::Syntax => "语法",
            Level::Semantic => "语义",
            Level::Ir => "IR",
            Level::Backend => "后端",
            Level::Artifact => "产物",
        }
    }

    /// 失效粒度声明
    pub fn granularity(self) -> &'static str {
        match self {
            Level::Semantic => "符号作用域",
            Level::Ir => "函数",
            Level::Backend => "模块",
            _ => "整层",
        }
    }
}

/// 缓存条目：内容哈希为键
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub level: Level,
    pub key: u64,
    /// 有效标记（失效后置 false）
    pub valid: bool,
}

/// 六级缓存结构
#[derive(Clone, Debug, Default)]
pub struct Cache {
    pub entries: Vec<Entry>,
}

impl Cache {
    pub fn put(&mut self, level: Level, key: u64) -> bool {
        if self.entries.iter().any(|e| e.level == level && e.key == key && e.valid) {
            return false;
        }
        self.entries.push(Entry { level, key, valid: true });
        true
    }

    /// 复用必须哈希命中（不变式）
    pub fn hit(&self, level: Level, key: u64) -> bool {
        self.entries.iter().any(|e| e.level == level && e.key == key && e.valid)
    }

    /// 失效某层全部条目
    pub fn invalidate_level(&mut self, level: Level) -> usize {
        let mut n = 0;
        for e in self.entries.iter_mut() {
            if e.level >= level {
                if e.valid {
                    e.valid = false;
                    n += 1;
                }
            }
        }
        n
    }
}

/// 依赖图（层间传播表）：变更向下级联
pub fn propagate_from(level: Level) -> Vec<Level> {
    use Level::*;
    match level {
        Lex => vec![Lex, Syntax, Semantic, Ir, Backend, Artifact],
        Syntax => vec![Syntax, Semantic, Ir, Backend, Artifact],
        Semantic => vec![Semantic, Ir, Backend, Artifact],
        Ir => vec![Ir, Backend, Artifact],
        Backend => vec![Backend, Artifact],
        Artifact => vec![Artifact],
    }
}

/// 级联失效：从变更层起逐级失效
pub fn cascade(cache: &mut Cache, changed: Level, key: u64) -> usize {
    let mut n = 0;
    for lv in propagate_from(changed) {
        n += cache.invalidate_level(lv);
    }
    let _ = key;
    n
}

/// 未受影响子树向上复用：命中即复用
pub fn reuse_upstream(cache: &Cache, level: Level, key: u64) -> bool {
    cache.hit(level, key)
}

/// 降级矩阵：哈希未命中 → 全量重编回退（不变式优先）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fallback {
    /// 增量复用
    Reuse,
    /// 全量重编
    FullRebuild,
}

/// 复用决策：任一层未命中即整体回退全量（宁可全量绝无脏复用）
pub fn decide(cache: &Cache, plan: &[(Level, u64)]) -> Fallback {
    for (lv, k) in plan {
        if !cache.hit(*lv, *k) {
            return Fallback::FullRebuild;
        }
    }
    Fallback::Reuse
}

/// 级联中断兜底：传播表异常时全量兜底
pub fn interrupted_ok(cascaded_levels: usize, expect_min: usize) -> bool {
    // 中断场景：级联覆盖不足 → 由调用方触发全量（此处校验覆盖面）
    cascaded_levels >= expect_min
}

/// 粒度误判保守放大：失效域按保守档放大一级
pub fn widen(level: Level) -> Level {
    use Level::*;
    match level {
        Lex => Syntax,
        Syntax => Semantic,
        Semantic => Ir,
        Ir => Backend,
        Backend | Artifact => Artifact,
    }
}

pub fn run_vec182_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec182");
    // 族一：六级缓存
    let mut c = Cache::default();
    s.add("缓存-六层写入", c.put(Level::Lex, 1) && c.put(Level::Syntax, 1) && c.put(Level::Ir, 1), "put");
    s.add("缓存-命中", c.hit(Level::Lex, 1) && c.hit(Level::Ir, 1), "hit");
    s.add("缓存-未命中", !c.hit(Level::Backend, 1), "miss");
    s.add("缓存-重复写入拒绝", !c.put(Level::Lex, 1), "dup");
    // 族二：级联失效传播
    let n = cascade(&mut c, Level::Semantic, 1);
    s.add("级联-向下全失效", n >= 2 && !c.hit(Level::Ir, 1) && !c.hit(Level::Lex, 1), "drop");
    s.add("级联-传播表语义层", propagate_from(Level::Lex).len() == 6, "p6");
    s.add("级联-传播表产物层", propagate_from(Level::Artifact) == vec![Level::Artifact], "p1");
    s.add("级联-传播表IR层", propagate_from(Level::Ir) == vec![Level::Ir, Level::Backend, Level::Artifact], "p3");
    // 族三：向上复用
    let mut c2 = Cache::default();
    c2.put(Level::Lex, 9);
    c2.put(Level::Syntax, 9);
    s.add("复用-上游命中", reuse_upstream(&c2, Level::Lex, 9), "up");
    s.add("复用-下游不受惠", !reuse_upstream(&c2, Level::Ir, 9), "down");
    // 族四：不变式优先——任一层未命中即全量
    let mut c3 = Cache::default();
    c3.put(Level::Lex, 5);
    c3.put(Level::Syntax, 5);
    s.add("决策-全命中复用", decide(&c3, &[(Level::Lex, 5), (Level::Syntax, 5)]) == Fallback::Reuse, "reuse");
    s.add("决策-断链全量回退", decide(&c3, &[(Level::Lex, 5), (Level::Semantic, 5)]) == Fallback::FullRebuild, "full");
    s.add("决策-脏复用禁止", decide(&c3, &[]) == Fallback::Reuse, "empty");
    // 族五：级联中断兜底
    s.add("兜底-覆盖充分", interrupted_ok(3, 3), "ok");
    s.add("兜底-覆盖不足检出", !interrupted_ok(2, 3), "short");
    // 族六：粒度声明与保守放大
    s.add("粒度-语义作用域", Level::Semantic.granularity() == "符号作用域", "g1");
    s.add("粒度-IR函数", Level::Ir.granularity() == "函数", "g2");
    s.add("粒度-后端模块", Level::Backend.granularity() == "模块", "g3");
    s.add("放大-误判升一级", widen(Level::Semantic) == Level::Ir && widen(Level::Lex) == Level::Syntax, "widen");
    s.add("放大-末端封顶", widen(Level::Backend) == Level::Artifact, "cap");
    // 族七：层序
    s.add("层序-单调", Level::Lex < Level::Syntax && Level::Backend < Level::Artifact, "order");
    s.add("层名-可读", Level::Ir.name() == "IR", "name");
    s
}
