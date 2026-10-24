//! VE-F0584 · 热编译失败回滚
//!
//! 失败即保留上一有效版本——原子切换纪律（新产物经验证通过才替换活跃绑定，
//! 替换瞬间原子，不存在半新半旧窗口）；三触发回滚：编译失败、验证失败、运行
//! 异常（加载后崩溃即回退并标记禁用该版本）；错误呈现三要素进编辑器诊断流
//! （与 F0508 家族同构）；跨文件变更失败的级联回滚（相关文件一并回退到一致
//! 快照）。
//!
//! 跨批对接：上游 F0581 服务、F0567 验证门禁；下游 F0588 级联、F0585 诊断流。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 版本产物
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Artifact {
    pub ver: u32,
    /// 验证状态
    pub verified: bool,
    /// 禁用标记（运行异常版本）
    pub disabled: bool,
}

/// 活跃绑定：原子切换协议
#[derive(Clone, Debug, Default)]
pub struct ActiveBinding {
    /// 当前活跃版本号
    pub active: Option<u32>,
    /// 历史版本池
    pub pool: Vec<Artifact>,
    /// 切换次数
    pub switches: u32,
}

impl ActiveBinding {
    /// 原子切换：验证通过才替换（fail-closed）
    pub fn switch(&mut self, art: Artifact) -> bool {
        if !art.verified || art.disabled {
            return false;
        }
        self.pool.push(art);
        self.active = self.pool.last().map(|a| a.ver);
        self.switches += 1;
        true
    }

    pub fn get(&self) -> Option<u32> {
        self.active
    }
}

/// 回滚触发三分类
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RollbackTrigger {
    /// 编译失败 → 保留旧版
    CompileFail,
    /// 验证失败 → 保留旧版加标记
    VerifyFail,
    /// 运行异常 → 回退加禁用
    RuntimeCrash,
}

/// 回滚执行：三触发处置
pub fn rollback(b: &mut ActiveBinding, trigger: RollbackTrigger, bad_ver: u32) -> bool {
    match trigger {
        RollbackTrigger::CompileFail => {
            // 编译失败：新版本未入池，保留旧版即可
            !b.pool.iter().any(|a| a.ver == bad_ver)
        }
        RollbackTrigger::VerifyFail => {
            // 验证失败：入池但标记未验证（留痕），保留旧版
            b.pool.push(Artifact { ver: bad_ver, verified: false, disabled: true });
            b.active = b.pool.iter().filter(|a| a.verified && !a.disabled).map(|a| a.ver).max();
            true
        }
        RollbackTrigger::RuntimeCrash => {
            // 运行异常：回退 + 标记禁用该版本
            if let Some(a) = b.pool.iter_mut().find(|a| a.ver == bad_ver) {
                a.disabled = true;
            }
            b.active = b.pool.iter().filter(|a| a.verified && !a.disabled).map(|a| a.ver).max();
            true
        }
    }
}

/// 错误呈现三要素（与 F0508 同构）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    pub source_line: u32,
    pub text: String,
    /// 修复建议
    pub fix_hint: String,
}

impl Diag {
    pub fn new(source_line: u32, text: &str, fix_hint: &str) -> Self {
        Diag { source_line, text: text.to_string(), fix_hint: fix_hint.to_string() }
    }

    pub fn complete(&self) -> bool {
        self.source_line > 0 && !self.text.is_empty()
    }
}

/// 诊断流（编辑器消费）
#[derive(Clone, Debug, Default)]
pub struct DiagStream {
    pub items: Vec<Diag>,
}

impl DiagStream {
    pub fn push(&mut self, d: Diag) -> bool {
        if !d.complete() {
            return false;
        }
        self.items.push(d);
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 一致快照：级联回滚目标
#[derive(Clone, Debug, Default)]
pub struct ConsistentSnapshot {
    /// 文件 → 版本号（一致时点）
    pub files: Vec<(String, u32)>,
}

impl ConsistentSnapshot {
    pub fn record(&mut self, file: &str, ver: u32) -> bool {
        if file.is_empty() {
            return false;
        }
        match self.files.iter_mut().find(|(f, _)| f == file) {
            Some(e) => {
                e.1 = ver;
                true
            }
            None => {
                self.files.push((file.to_string(), ver));
                true
            }
        }
    }

    /// 级联回滚：相关文件一并回退到一致快照
    pub fn restore_into(&self, bindings: &mut Vec<(String, u32)>) -> usize {
        let mut n = 0;
        for (f, v) in &self.files {
            if let Some(e) = bindings.iter_mut().find(|(bf, _)| bf == f) {
                if e.1 != *v {
                    e.1 = *v;
                    n += 1;
                }
            } else {
                bindings.push((f.clone(), *v));
                n += 1;
            }
        }
        n
    }
}

/// 半新半旧窗口检测：绑定必须整体一致（不存在文件间版本交叉）
pub fn consistent(bindings: &[(String, u32)], snap: &ConsistentSnapshot) -> bool {
    bindings.iter().all(|(f, v)| snap.files.iter().find(|(sf, _)| sf == f).map(|(_, sv)| sv == v).unwrap_or(true))
}

pub fn run_vec184_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec184");
    // 族一：原子切换
    let mut b = ActiveBinding::default();
    s.add("绑定-空活跃", b.get().is_none(), "none");
    s.add("切换-未验拒绝", !b.switch(Artifact { ver: 1, verified: false, disabled: false }), "novfy");
    s.add("切换-验证过成功", b.switch(Artifact { ver: 1, verified: true, disabled: false }), "ok");
    s.add("切换-活跃更新", b.get() == Some(1) && b.switches == 1, "act");
    b.switch(Artifact { ver: 2, verified: true, disabled: false });
    s.add("切换-原子递增", b.get() == Some(2) && b.switches == 2, "atomic");
    s.add("切换-禁用版本拒绝", !b.switch(Artifact { ver: 3, verified: true, disabled: true }), "dis");
    // 族二：三触发回滚
    let mut b2 = ActiveBinding::default();
    b2.switch(Artifact { ver: 1, verified: true, disabled: false });
    s.add("触发-编译失败保留旧版", rollback(&mut b2, RollbackTrigger::CompileFail, 2) && b2.get() == Some(1), "cf");
    s.add("触发-验证失败留痕", rollback(&mut b2, RollbackTrigger::VerifyFail, 2) && b2.get() == Some(1) && b2.pool.iter().any(|a| a.ver == 2 && !a.verified), "vf");
    b2.switch(Artifact { ver: 3, verified: true, disabled: false });
    s.add("触发-运行异常回退禁用", rollback(&mut b2, RollbackTrigger::RuntimeCrash, 3) && b2.get() == Some(1) && b2.pool.iter().find(|a| a.ver == 3).map(|a| a.disabled).unwrap_or(false), "rc");
    // 族三：错误呈现三要素
    let mut ds = DiagStream::default();
    s.add("诊断-三要素完整", ds.push(Diag::new(12, "类型不匹配", "检查绑定类型")), "ok");
    s.add("诊断-零行拒绝", !ds.push(Diag::new(0, "x", "y")), "noline");
    s.add("诊断-空文本拒绝", !ds.push(Diag::new(1, "", "y")), "notext");
    s.add("诊断-流入计数", ds.len() == 1 && !ds.is_empty(), "cnt");
    // 族四：一致快照级联回滚
    let mut snap = ConsistentSnapshot::default();
    snap.record("a.wg", 3);
    snap.record("b.wg", 5);
    let mut bindings = vec![("a.wg".to_string(), 9), ("b.wg".to_string(), 5)];
    s.add("快照-登记生效", snap.files.len() == 2, "rec");
    let n = snap.restore_into(&mut bindings);
    s.add("快照-级联回退变更数", n == 1, "n1");
    s.add("快照-回退后一致", consistent(&bindings, &snap), "consistent");
    // 族五：半新半旧窗口检出
    bindings[1].1 = 7;
    s.add("窗口-交叉版本检出", !consistent(&bindings, &snap), "mixed");
    bindings[1].1 = 5;
    s.add("窗口-恢复一致", consistent(&bindings, &snap), "fixed");
    // 族六：未登记文件视为可兼容
    s.add("快照-新文件兼容", consistent(&vec![("c.wg".to_string(), 1)], &snap), "newfile");
    s
}
