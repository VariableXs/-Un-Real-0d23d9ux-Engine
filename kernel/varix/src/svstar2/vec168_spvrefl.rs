//! VE-F0568 · SPIR-V 反射信息
//!
//! 从模块提取接口元信息——描述符集布局（set、binding、类型、数组尺寸）、
//! 推量块布局、顶点输入位置、输出接口；工具生态双轨：spirv-reflect 轻量提取
//! 与 spirv-cross 交叉提取的选用规则（前者快、后者信息全）；与 VE 绑定模型
//! 对账（F0026）：反射回读与 VE 声明一致性校验——绑定遗漏或错位即编译告警；
//! 反射数据按模块哈希缓存。
//!
//! 跨批对接：上游 F0564 装饰、F0026 绑定模型；下游 F0579 选择器能力陈述。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 反射轨道
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReflectTrack {
    /// spirv-reflect：轻量快
    Lite,
    /// spirv-cross：交叉提取信息全
    Cross,
}

impl ReflectTrack {
    /// 选用规则：只需布局快查 → Lite；需要交叉对账完整接口 → Cross
    pub fn select(need_cross_reconcile: bool, large_module: bool) -> ReflectTrack {
        if need_cross_reconcile || !large_module {
            ReflectTrack::Cross
        } else {
            ReflectTrack::Lite
        }
    }
}

/// 绑定资源
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BindRes {
    pub set: u32,
    pub binding: u32,
    /// 类型（0=uniform 1=storage 2=sampler 3=texture）
    pub kind: u32,
    pub array_size: u32,
}

impl BindRes {
    pub fn key(&self) -> (u32, u32) {
        (self.set, self.binding)
    }
}

/// 推量块布局
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PushLayout {
    pub offset: u32,
    pub size: u32,
}

/// 反射提取器
#[derive(Clone, Debug, Default)]
pub struct Reflector {
    pub bindings: Vec<BindRes>,
    pub push: Option<PushLayout>,
    /// 顶点输入位置
    pub inputs: Vec<(u32, u32)>,
    /// 输出接口
    pub outputs: Vec<(u32, u32)>,
    /// 提取失败登记（回退 VE 侧自有接口表）
    pub fallback_used: bool,
}

impl Reflector {
    /// 全接口提取：从装饰集合构造
    pub fn extract(mut bindings: Vec<BindRes>, push: Option<PushLayout>) -> Self {
        bindings.sort_by_key(|b| b.key());
        Reflector { bindings, push, inputs: Vec::new(), outputs: Vec::new(), fallback_used: false }
    }

    /// 提取失败回退：登记并返回空表
    pub fn fallback() -> Self {
        Reflector { fallback_used: true, ..Default::default() }
    }

    pub fn lookup(&self, set: u32, binding: u32) -> Option<&BindRes> {
        self.bindings.iter().find(|b| b.set == set && b.binding == binding)
    }
}

/// 对账校验器：反射回读与 VE 声明一致性校验
#[derive(Clone, Debug, Default)]
pub struct Reconciler {
    /// VE 侧声明 (set, binding)
    pub ve_decls: Vec<(u32, u32)>,
}

impl Reconciler {
    pub fn declare(&mut self, set: u32, binding: u32) {
        self.ve_decls.push((set, binding));
    }

    /// 对账：绑定遗漏或错位检出
    /// 返回 (遗漏于反射, 反射多出)
    pub fn reconcile(&self, refl: &Reflector) -> (Vec<(u32, u32)>, Vec<(u32, u32)>) {
        let refl_keys: Vec<(u32, u32)> = refl.bindings.iter().map(|b| b.key()).collect();
        let missing: Vec<(u32, u32)> = self.ve_decls.iter().filter(|k| !refl_keys.contains(k)).cloned().collect();
        let extra: Vec<(u32, u32)> = refl_keys.iter().filter(|k| !self.ve_decls.contains(k)).cloned().collect();
        (missing, extra)
    }

    /// 错位检出：键对但 kind 不一致由调用方逐项比对
    pub fn kind_mismatch(&self, refl: &Reflector, ve_kinds: &[(u32, u32, u32)]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for (set, binding, kind) in ve_kinds {
            if let Some(b) = refl.lookup(*set, *binding) {
                if b.kind != *kind {
                    out.push((*set, *binding));
                }
            }
        }
        out
    }

    pub fn has_decl(&self, set: u32, binding: u32) -> bool {
        self.ve_decls.contains(&(set, binding))
    }
}

/// 反射缓存：按模块哈希缓存
#[derive(Clone, Debug, Default)]
pub struct ReflCache {
    entries: Vec<(u64, Reflector)>,
    /// 哈希升级（碰撞）计数
    pub hash_upgrades: u32,
}

impl ReflCache {
    pub fn new() -> Self {
        ReflCache::default()
    }

    pub fn get(&self, hash: u64) -> Option<&Reflector> {
        self.entries.iter().find(|(h, _)| *h == hash).map(|(_, r)| r)
    }

    pub fn put(&mut self, hash: u64, r: Reflector) -> bool {
        if self.entries.iter().any(|(h, _)| *h == hash) {
            // 键碰撞 → 哈希升级：扩位再试
            self.hash_upgrades += 1;
            let up = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            if self.entries.iter().any(|(h, _)| *h == up) {
                return false;
            }
            self.entries.push((up, r));
            return true;
        }
        self.entries.push((hash, r));
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// FNV-1a 模块哈希
pub fn module_hash(words: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

pub fn run_vec168_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec168");
    // 族一：双轨选用
    s.add("选轨-交叉对账用Cross", ReflectTrack::select(true, true) == ReflectTrack::Cross, "cross");
    s.add("选轨-小模块用Cross", ReflectTrack::select(false, false) == ReflectTrack::Cross, "small");
    s.add("选轨-大模块快查用Lite", ReflectTrack::select(false, true) == ReflectTrack::Lite, "lite");
    // 族二：全接口提取
    let binds = vec![
        BindRes { set: 0, binding: 2, kind: 1, array_size: 1 },
        BindRes { set: 0, binding: 0, kind: 0, array_size: 1 },
        BindRes { set: 1, binding: 0, kind: 3, array_size: 4 },
    ];
    let r = Reflector::extract(binds, Some(PushLayout { offset: 0, size: 64 }));
    s.add("提取-排序归一", r.bindings[0].key() == (0, 0) && r.bindings[2].key() == (1, 0), "sort");
    s.add("提取-查表命中", r.lookup(0, 2).map(|b| b.kind == 1).unwrap_or(false), "hit");
    s.add("提取-未命中", r.lookup(5, 5).is_none(), "miss");
    s.add("提取-推量块", r.push == Some(PushLayout { offset: 0, size: 64 }), "push");
    s.add("提取-数组尺寸", r.lookup(1, 0).map(|b| b.array_size == 4).unwrap_or(false), "arr");
    // 族三：回退登记
    let fb = Reflector::fallback();
    s.add("回退-标记登记", fb.fallback_used && fb.bindings.is_empty(), "fb");
    // 族四：对账——绑定遗漏
    let mut rc = Reconciler::default();
    rc.declare(0, 0);
    rc.declare(0, 1);
    let (miss, extra) = rc.reconcile(&r);
    s.add("对账-遗漏检出", miss == vec![(0, 1)], "miss");
    s.add("对账-反射多出", extra == vec![(1, 0)], "extra");
    // 完全一致
    rc.declare(1, 0);
    let (miss2, extra2) = rc.reconcile(&r);
    s.add("对账-一致零差异", miss2.is_empty() && extra2.is_empty(), "clean");
    s.add("对账-声明存在", rc.has_decl(1, 0) && !rc.has_decl(9, 9), "has");
    // 族五：错位检出
    let mm = rc.kind_mismatch(&r, &[(0, 0, 0), (0, 2, 9)]);
    s.add("对账-kind错位检出", mm == vec![(0, 2)], "kind");
    // 族六：缓存
    let mut cache = ReflCache::new();
    s.add("缓存-空未命中", cache.get(7).is_none(), "empty");
    s.add("缓存-放入", cache.put(7, r.clone()), "put");
    s.add("缓存-命中", cache.get(7).is_some(), "hit");
    s.add("缓存-碰撞哈希升级", cache.put(7, Reflector::fallback()) && cache.hash_upgrades == 1, "upg");
    s.add("缓存-计数", cache.len() == 2, "n2");
    // 族七：模块哈希确定性
    let h1 = module_hash(&[1, 2, 3]);
    let h2 = module_hash(&[1, 2, 3]);
    let h3 = module_hash(&[1, 2, 4]);
    s.add("哈希-同输入同值", h1 == h2, "same");
    s.add("哈希-异输入异值", h1 != h3, "diff");
    s
}
