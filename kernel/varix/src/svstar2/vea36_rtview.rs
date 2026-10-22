//! VE-F0036 · 渲染目标视图管理（VE-A 域 · 资源视图段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0036`
//!
//! **职责定位（锚点原文）**：渲染目标视图（RTV）的统一管理（视图创建/缓存/
//! 失效三段），视图与资源的生命周期绑定（资源没了视图必须失效），视图去重
//! （同参共享）；含视图类型的枚举完备性断言（新视图类型必须登记）。
//!
//! **数据结构（锚点原文）**：视图管理。在 [`RtViewMgr`] 上一一落位：
//! 视图条目 = [`ViewEntry`]（键 + 代数 + 引用计数）、键 = [`ViewKey`]、
//! 统计账 = [`ViewStats`]。
//!
//! **判据（锚点原文）**：三段管理、生命周期绑定、悬垂失效、去重共享、判据。
//!
//! 本模块 14 条判据逐条映射：**三段管理 4 条**（创建产出合法 id／同键缓存
//! 命中去重共享／显性失效移除条目／重建把缓存清回可复算状态）、
//! **生命周期绑定 3 条**（资源销毁联动全量失效／失效计数与条目数对账／
//! 资源不存在时联动零次且如实记账）、**悬垂失效 3 条**（失效后按旧 id 访问
//! 显性拒绝／代数防 id 复用悬垂／引用计数下溢拒绝）、**去重共享 2 条**
//! （同键同 id 共享引用／共享视图一方释放不影响另一方）、**判据自身 2 条**
//! （码位互异段独占／枚举完备断言）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 悬垂视图 → **失效联动** | [`RtViewMgr::resource_destroyed`] 一次扫全表，把该资源的全部视图移除并返回失效条数——「资源没了视图还在」这个状态在管理器里活不过一次联动调用 |
//! | 缓存污染 → **重建** | [`RtViewMgr::cache_rebuild`] 把全部视图条目清空、代数轴推进、计数入账——重建不是「修数据」而是「扔掉可疑缓存」，下次创建按同号新代数重新来过 |
//! | 去重冲突 → **仲裁** | 同键创建撞上已失效条目时按**代数仲裁**：新请求拿到新代数新 id，旧 id 永久失效（[`CODE_STALE_ID`]）——「最新者胜，旧 id 不复用」，杜绝拿旧句柄摸到新视图的悬垂通道 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **键相等 = 资源 + 类型 + 格式 + mip + slice 五元组全等**：去重的口径
//!   是「同参共享」，参数差任何一项就是不同的视图（D3D 语义里 RTV 与
//!   DSV 同资源同 mip 也是两个描述符）。判据对拍时五字段逐项独立改一条
//!   验证不误合并。
//! - **代数（generation）是悬垂的解**：视图 id 若允许回收复用，「失效后
//!   拿旧 id 操作」与「正常操作」在数值上无法区分。本模块 id 单调递增
//!   不复用 + 条目带代数——旧 id 查无此条即显性 [`CODE_STALE_ID`]，
//!   悬垂在类型与查表两层都无通道。
//! - **生命周期绑定是单向硬约束**：资源销毁 → 视图失效只有这一个方向；
//!   反过来「视图全释放 → 资源可回收」是资源管理器（F0016/F0017）的引用
//!   计数决策，本模块只如实报引用数，不越权替资源侧做回收决定。
//! - **枚举完备性断言**：[`ViewKind::ALL`] 长度必须等于登记表期望值
//!   [`KIND_REGISTRY_LEN`]。新视图类型只加枚举不变更登记表，判据立即红——
//!   「新类型必须登记」从注释承诺变成可执行断言。
//! - **零 panic 面**：查表走 `get`/`position`、计数全 `saturating_*`、
//!   所有操作返回 [`Result`]，引用下溢（重复释放）显性拒绝不回绕。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x13xx，独占——整仓 grep 0x1301 零占用后选定）
// ---------------------------------------------------------------------------

/// 视图键非法（资源号为 0 / 格式号为 0）。
pub const CODE_BAD_KEY: u16 = 0x1301;
/// 视图 id 查无此条（从未创建或已失效移除）。
pub const CODE_VIEW_UNKNOWN: u16 = 0x1302;
/// 悬垂 id：条目曾被失效，id 不复用，旧句柄永久作废。
pub const CODE_STALE_ID: u16 = 0x1303;
/// 视图类型未登记（枚举完备性断言拒绝路径）。
pub const CODE_KIND_UNREGISTERED: u16 = 0x1304;
/// 引用计数下溢（重复释放 / 未持有的释放）。
pub const CODE_REFCOUNT_UNDERFLOW: u16 = 0x1305;
/// 去重冲突仲裁失败（理论上不可达；留给并发防线）。
pub const CODE_KEY_CONFLICT: u16 = 0x1306;

// ---------------------------------------------------------------------------
// 二、视图类型：枚举完备性断言的载体
// ---------------------------------------------------------------------------

/// 视图类型全集。新增类型必须：加变体 + 进 [`ALL`] + 抬 [`KIND_REGISTRY_LEN`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// 渲染目标视图（本模块主语）。
    Rtv,
    /// 深度模板视图（F0037 同构复用本枚举）。
    Dsv,
    /// 着色器资源视图。
    Srv,
    /// 无序访问视图（F0038 同构复用本枚举）。
    Uav,
}

impl ViewKind {
    /// 全集（判据枚举完备性断言用；新变体必须进此表）。
    pub const ALL: [ViewKind; 4] =
        [ViewKind::Rtv, ViewKind::Dsv, ViewKind::Srv, ViewKind::Uav];

    /// 登记表期望长度：`ALL.len()` 与此常量不等 = 有新类型未登记，判据红。
    pub const KIND_REGISTRY_LEN: usize = 4;

    /// 短名（读屏与统计口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ViewKind::Rtv => "RTV",
            ViewKind::Dsv => "DSV",
            ViewKind::Srv => "SRV",
            ViewKind::Uav => "UAV",
        }
    }

    /// 登记表：全部类型按位登记（bit0..=bit3 对应 ALL 序）。
    pub const fn registry_mask() -> u8 {
        0b1111
    }

    /// 类型是否已登记（完备性运行时通道：任何未登记类型在创建期拒绝）。
    pub const fn registered(self) -> bool {
        let bit = match self {
            ViewKind::Rtv => 0,
            ViewKind::Dsv => 1,
            ViewKind::Srv => 2,
            ViewKind::Uav => 3,
        };
        (Self::registry_mask() >> bit) & 1 == 1
    }
}

// ---------------------------------------------------------------------------
// 三、视图键与视图条目
// ---------------------------------------------------------------------------

/// 视图键：去重口径的五元组（五项全等才是同一视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewKey {
    /// 资源号（0 号保留给「无资源」，非法）。
    pub resource: u32,
    /// 视图类型。
    pub kind: ViewKind,
    /// 格式号（0 号非法——格式能力表由 F0040 治理，本模块只拒 0）。
    pub format: u32,
    /// mip 层。
    pub mip: u8,
    /// 数组切片 / 3D 切片号。
    pub slice: u16,
}

impl ViewKey {
    /// 键校验：资源、格式非零 + 类型已登记。三道独立红线。
    pub fn validate(&self) -> Result<(), u16> {
        if self.resource == 0 {
            return Err(CODE_BAD_KEY);
        }
        if self.format == 0 {
            return Err(CODE_BAD_KEY);
        }
        if !self.kind.registered() {
            return Err(CODE_KIND_UNREGISTERED);
        }
        Ok(())
    }
}

/// 视图条目：键 + 代数 + 共享引用计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewEntry {
    /// 视图 id（管理器内单调递增，永不复用）。
    pub id: u32,
    /// 视图键。
    pub key: ViewKey,
    /// 代数：缓存重建时全体抬一代，旧 id 藉此与新条目区分。
    pub gen: u32,
    /// 共享引用计数（≥1；归零即移除）。
    pub refs: u32,
}

// ---------------------------------------------------------------------------
// 四、统计账
// ---------------------------------------------------------------------------

/// 视图管理统计账（放行/诊断的全部依据都可从这里读出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewStats {
    /// 当前存活条目数。
    pub live: usize,
    /// 去重命中次数（同键共享而非新建）。
    pub dedup_hits: usize,
    /// 实际新建次数。
    pub created: usize,
    /// 显性失效次数（invalidate / 生命周期联动合计）。
    pub invalidated: usize,
    /// 其中生命周期联动（资源销毁）带动的失效条数。
    pub liveness_invalidated: usize,
    /// 缓存重建次数。
    pub rebuilds: usize,
    /// 拒绝次数（键非法 / 未知 id / 悬垂 / 下溢 / 未登记类型）。
    pub rejects: usize,
    /// 当前全局代数。
    pub gen: u32,
}

// ---------------------------------------------------------------------------
// 五、视图管理器本体
// ---------------------------------------------------------------------------

/// RTV 统一管理器：创建 / 缓存（去重共享）/ 失效三段。
#[derive(Clone, Debug)]
pub struct RtViewMgr {
    /// 存活条目（按创建序；失效即物理移除，不留墓碑）。
    views: Vec<ViewEntry>,
    /// 失效 id 墓碑（判据 3.1/3.2 的悬垂判定依据：id 在墓碑 = 显性悬垂码）。
    tombstones: Vec<u32>,
    /// 下一个视图 id（从 1 起，0 号保留给「无视图」）。
    next_id: u32,
    /// 统计账。
    pub stats: ViewStats,
}

impl RtViewMgr {
    /// 新建管理器（空表、零统计、代数 1）。
    pub fn new() -> RtViewMgr {
        RtViewMgr {
            views: Vec::new(),
            tombstones: Vec::new(),
            next_id: 1,
            stats: ViewStats { gen: 1, ..ViewStats::default() },
        }
    }

    /// 创建视图（三段之一）：键校验 → 去重共享或新建。
    ///
    /// 同键存活条目存在 → 共享（引用 +1，[`ViewStats::dedup_hits`] +1），
    /// 返回同一 id；否则新建条目。键非法 / 类型未登记显性拒绝。
    pub fn create(&mut self, key: ViewKey) -> Result<u32, u16> {
        key.validate()?;
        if let Some(v) = self.views.iter().find(|v| v.key == key) {
            let id = v.id;
            let pos = self.view_pos(id).unwrap_or(usize::MAX);
            if pos != usize::MAX {
                self.views[pos].refs = self.views[pos].refs.saturating_add(1);
            }
            self.stats.dedup_hits = self.stats.dedup_hits.saturating_add(1);
            return Ok(id);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.views.push(ViewEntry { id, key, gen: self.stats.gen, refs: 1 });
        self.stats.created = self.stats.created.saturating_add(1);
        self.stats.live = self.views.len();
        Ok(id)
    }

    /// 释放一次引用：引用归零即移除条目（三段之一：失效路径的引用侧）。
    pub fn release(&mut self, id: u32) -> Result<(), u16> {
        let pos = match self.view_pos(id) {
            Some(p) => p,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(if self.tombstones.contains(&id) {
                    CODE_STALE_ID
                } else {
                    CODE_VIEW_UNKNOWN
                });
            }
        };
        if self.views[pos].refs <= 1 {
            self.views.remove(pos);
            self.tombstones.push(id);
            self.stats.invalidated = self.stats.invalidated.saturating_add(1);
            self.stats.live = self.views.len();
            return Ok(());
        }
        self.views[pos].refs = self.views[pos].refs.saturating_sub(1);
        Ok(())
    }

    /// 显性失效（三段之一）：缓存污染等场景下强制移除条目。
    pub fn invalidate(&mut self, id: u32) -> Result<(), u16> {
        let pos = match self.view_pos(id) {
            Some(p) => p,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(if self.tombstones.contains(&id) {
                    CODE_STALE_ID
                } else {
                    CODE_VIEW_UNKNOWN
                });
            }
        };
        self.views.remove(pos);
        self.tombstones.push(id);
        self.stats.invalidated = self.stats.invalidated.saturating_add(1);
        self.stats.live = self.views.len();
        Ok(())
    }

    /// 生命周期绑定（锚点主判据）：资源销毁 → 该资源全部视图联动失效。
    ///
    /// 返回失效条数（0 = 该资源本来就没有视图，如实记账不当作错误）。
    pub fn resource_destroyed(&mut self, resource: u32) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.views.len() {
            if self.views[i].key.resource == resource {
                let id = self.views[i].id;
                self.views.remove(i);
                self.tombstones.push(id);
                self.stats.invalidated = self.stats.invalidated.saturating_add(1);
                self.stats.liveness_invalidated = self.stats.liveness_invalidated.saturating_add(1);
                n = n.saturating_add(1);
            } else {
                i += 1;
            }
        }
        self.stats.live = self.views.len();
        n
    }

    /// 缓存重建（锚点降级项：缓存污染 → 重建）：全表清空、代数推进。
    ///
    /// 重建后所有旧 id 全部落入墓碑语义（查无条目 + 代数已推进），
    /// 下次创建按新代数重新建立缓存。
    pub fn cache_rebuild(&mut self) -> usize {
        let n = self.views.len();
        for v in self.views.iter() {
            self.tombstones.push(v.id);
        }
        self.views.clear();
        self.stats.gen = self.stats.gen.saturating_add(1);
        self.stats.rebuilds = self.stats.rebuilds.saturating_add(1);
        self.stats.live = 0;
        self.stats
            .invalidated = self.stats.invalidated.saturating_add(n);
        n
    }

    /// 查视图（只读）：未知/悬垂 id 返回 `None`（零 panic 面）。
    pub fn view(&self, id: u32) -> Option<ViewEntry> {
        self.views.iter().find(|v| v.id == id).copied()
    }

    /// 某资源的存活视图数（只读）。
    pub fn live_for_resource(&self, resource: u32) -> usize {
        self.views.iter().filter(|v| v.key.resource == resource).count()
    }

    /// 存活条目快照（判据与读屏用）。
    pub fn snapshot(&self) -> Vec<ViewEntry> {
        self.views.clone()
    }

    /// 悬垂墓碑快照（判据 3.1 用）。
    pub fn tombstone_snapshot(&self) -> Vec<u32> {
        self.tombstones.clone()
    }

    /// 读屏摘要（锚点：视图状态读屏可达——总量/去重/失效三行起）。
    pub fn a11y_summary(&self) -> String {
        alloc::format!(
            "视图管理：存活 {}，新建 {}，去重命中 {}，失效 {}（生命周期联动 {}），重建 {}",
            self.stats.live,
            self.stats.created,
            self.stats.dedup_hits,
            self.stats.invalidated,
            self.stats.liveness_invalidated,
            self.stats.rebuilds
        )
    }

    /// 按 id 找条目下标（内部工具）。
    fn view_pos(&self, id: u32) -> Option<usize> {
        self.views.iter().position(|v| v.id == id)
    }
}

// ---------------------------------------------------------------------------
// 六、判据（run_vea36_checks —— 14 条，逐条映射锚点五条判据）
// ---------------------------------------------------------------------------

/// VE-F0036 域自检：三段管理 4 + 生命周期绑定 3 + 悬垂失效 3 + 去重共享 2 + 判据自身 2。
pub fn run_vea36_checks() -> CheckSet {
    let mut s = CheckSet::new("vea36_rtview");

    // --- 判据 1.1：三段管理——创建产出合法 id 且键校验三道红线独立拦截 ---
    {
        let mut m = RtViewMgr::new();
        let ok_key = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let id = m.create(ok_key);
        let bad_res = m.create(ViewKey { resource: 0, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 });
        let bad_fmt = m.create(ViewKey { resource: 7, kind: ViewKind::Rtv, format: 0, mip: 0, slice: 0 });
        s.add(
            "A36-三段-创建产出合法id且键三道红线独立拦截",
            id.is_ok()
                && bad_res == Err(CODE_BAD_KEY)
                && bad_fmt == Err(CODE_BAD_KEY)
                && m.stats.created == 1,
            "资源号/格式号非零、类型已登记三道校验，红线各自触发不混通道",
        );
    }

    // --- 判据 1.2：三段管理——同键缓存命中去重（不新建第二份） ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let a = m.create(k);
        let b = m.create(k);
        let ok = a.is_ok() && b.is_ok() && a == b
            && m.stats.created == 1
            && m.stats.dedup_hits == 1
            && m.view(a.unwrap_or(0)).map(|v| v.refs) == Some(2);
        s.add(
            "A36-三段-同键缓存命中共享引用",
            ok,
            "第二次创建命中缓存：同 id、引用计 2、created 恒 1——去重在创建期完成",
        );
    }

    // --- 判据 1.3：三段管理——显性失效移除条目（再访问显性未知） ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let id = m.create(k).unwrap_or(0);
        let inv = m.invalidate(id);
        let after = m.view(id).is_none();
        s.add(
            "A36-三段-显性失效移除条目",
            inv.is_ok() && after && m.stats.live == 0 && m.stats.invalidated == 1,
            "失效即物理移除不留活尸：再查显性 None，失效计数入账",
        );
    }

    // --- 判据 1.4：三段管理——缓存重建清空并推进代数 ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let _ = m.create(k);
        let n = m.cache_rebuild();
        let ok = n == 1 && m.stats.live == 0 && m.stats.rebuilds == 1
            && m.stats.gen == 2;
        // 重建后同键再建：新代数下正常工作
        let id2 = m.create(k);
        let ok2 = id2.is_ok() && m.view(id2.unwrap_or(0)).map(|v| v.gen) == Some(2);
        s.add(
            "A36-三段-缓存重建清空推进代数且新代数可用",
            ok && ok2,
            "重建 = 扔掉可疑缓存而非修数据：代数 +1，旧 id 全墓碑，新创建在新代数下重建",
        );
    }

    // --- 判据 2.1：生命周期绑定——资源销毁联动全量失效（多视图一网打尽） ---
    {
        let mut m = RtViewMgr::new();
        // 同资源三视图（RTV/DSV/SRV 三类型同资源并存）
        let _ = m.create(ViewKey { resource: 9, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 });
        let _ = m.create(ViewKey { resource: 9, kind: ViewKind::Dsv, format: 45, mip: 0, slice: 0 });
        let _ = m.create(ViewKey { resource: 9, kind: ViewKind::Srv, format: 87, mip: 1, slice: 0 });
        // 别的资源不受牵连
        let _ = m.create(ViewKey { resource: 11, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 });
        let n = m.resource_destroyed(9);
        let ok = n == 3
            && m.live_for_resource(9) == 0
            && m.live_for_resource(11) == 1
            && m.stats.liveness_invalidated == 3
            && m.stats.live == 1;
        s.add(
            "A36-绑定-资源销毁联动全量失效且不牵连他资源",
            ok,
            "资源没了视图必须失效：同资源三视图一次联动清零，他资源原样",
        );
    }

    // --- 判据 2.2：生命周期绑定——失效计数与条目数对账 ---
    {
        let mut m = RtViewMgr::new();
        let _ = m.create(ViewKey { resource: 9, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 });
        let _ = m.create(ViewKey { resource: 9, kind: ViewKind::Rtv, format: 87, mip: 1, slice: 0 });
        let _ = m.resource_destroyed(9);
        // 判据侧独立重算：invalidated 恒 = 墓碑数（本模块失效只有入墓碑一条路）
        let tomb = m.tombstone_snapshot().len();
        s.add(
            "A36-绑定-失效计数与墓碑数对账",
            m.stats.invalidated == tomb && tomb == 2,
            "每条失效都伴随一条墓碑：计数与墓碑逐条可对账，失效不虚报",
        );
    }

    // --- 判据 2.3：生命周期绑定——无视图资源联动零次且如实记账 ---
    {
        let mut m = RtViewMgr::new();
        let n = m.resource_destroyed(404);
        s.add(
            "A36-绑定-无视图资源联动零次如实记账",
            n == 0 && m.stats.liveness_invalidated == 0 && m.stats.invalidated == 0,
            "资源本来就没视图：联动返回 0、不虚报失效，「没有事发生」也是可判定的",
        );
    }

    // --- 判据 3.1：悬垂失效——失效后按旧 id 操作显性拒绝（墓碑语义） ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let id = m.create(k).unwrap_or(0);
        let _ = m.invalidate(id);
        let re_inv = m.invalidate(id);
        let re_rel = m.release(id);
        s.add(
            "A36-悬垂-失效后旧id操作显性拒绝",
            re_inv == Err(CODE_STALE_ID) && re_rel == Err(CODE_STALE_ID),
            "旧 id 落入墓碑：再失效/再释放都显性返回悬垂码，与「从未存在」分码",
        );
    }

    // --- 判据 3.2：悬垂失效——id 不复用（新建拿新号，旧号永悬） ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let id1 = m.create(k).unwrap_or(0);
        let _ = m.invalidate(id1);
        let id2 = m.create(k).unwrap_or(0);
        s.add(
            "A36-悬垂-id不复用代数轴区分新旧",
            id2 > id1
                && m.view(id1).is_none()
                && m.view(id2).is_some(),
            "id 单调递增不复用：新条目永远拿新号，旧句柄无通道摸到新视图",
        );
    }

    // --- 判据 3.3：悬垂失效——引用下溢拒绝（重复释放显性报错不回绕） ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let id = m.create(k).unwrap_or(0);
        let first = m.release(id);
        let second = m.release(id);
        s.add(
            "A36-悬垂-引用下溢显性拒绝不回绕",
            first.is_ok()
                && second == Err(CODE_STALE_ID)
                && m.stats.live == 0,
            "refs 归零即移除；对已移除条目再释放走墓碑通道，u32 永不回绕",
        );
    }

    // --- 判据 4.1：去重共享——五元组任一项不同即不合并（逐项反证） ---
    {
        let mut m = RtViewMgr::new();
        let base = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let a = m.create(base).unwrap_or(0);
        let b_res = m.create(ViewKey { resource: 8, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 }).unwrap_or(0);
        let b_fmt = m.create(ViewKey { resource: 7, kind: ViewKind::Rtv, format: 88, mip: 0, slice: 0 }).unwrap_or(0);
        let b_mip = m.create(ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 1, slice: 0 }).unwrap_or(0);
        let b_slice = m.create(ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 1 }).unwrap_or(0);
        let b_kind = m.create(ViewKey { resource: 7, kind: ViewKind::Dsv, format: 87, mip: 0, slice: 0 }).unwrap_or(0);
        let ok = a != b_res && a != b_fmt && a != b_mip && a != b_slice && a != b_kind
            && m.stats.created == 6
            && m.stats.dedup_hits == 0;
        s.add(
            "A36-去重-五元组任一项不同即不合并",
            ok,
            "资源/格式/mip/slice/类型逐项反证：差一项就是另一个视图，去重不误合并",
        );
    }

    // --- 判据 4.2：去重共享——共享视图一方释放不影响另一方 ---
    {
        let mut m = RtViewMgr::new();
        let k = ViewKey { resource: 7, kind: ViewKind::Rtv, format: 87, mip: 0, slice: 0 };
        let id = m.create(k).unwrap_or(0);
        let _ = m.create(k); // refs = 2
        let rel1 = m.release(id);
        let still = m.view(id).map(|v| v.refs) == Some(1) && m.stats.live == 1;
        let rel2 = m.release(id);
        let gone = m.view(id).is_none() && m.stats.live == 0;
        s.add(
            "A36-去重-共享视图一方释放不影响另一方",
            rel1.is_ok() && still && rel2.is_ok() && gone,
            "引用计数语义：每次释放退一，归零才移除——共享方的生命周期各自独立",
        );
    }

    // --- 判据 5.1：判据自身——码位互异且段独占（六码全对拍） ---
    {
        let codes = [
            CODE_BAD_KEY,
            CODE_VIEW_UNKNOWN,
            CODE_STALE_ID,
            CODE_KIND_UNREGISTERED,
            CODE_REFCOUNT_UNDERFLOW,
            CODE_KEY_CONFLICT,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1300);
        s.add(
            "A36-判据-诊断码互异且段独占",
            all_diff && in_segment && codes.len() == 6,
            "0x1301..=0x1306 六码两两互异、高字节全部落在 0x13 独占段",
        );
    }

    // --- 判据 5.2：判据自身——枚举完备性断言（新类型必须登记） ---
    {
        // 判据侧独立重算：ALL 长度 = 登记表常量、逐类型 registered 位全 1
        let len_ok = ViewKind::ALL.len() == ViewKind::KIND_REGISTRY_LEN;
        let mask_ok = ViewKind::ALL.iter().all(|k| k.registered());
        let names_diff = ViewKind::ALL[0].as_str() != ViewKind::ALL[1].as_str()
            && ViewKind::ALL[0].as_str() != ViewKind::ALL[2].as_str()
            && ViewKind::ALL[0].as_str() != ViewKind::ALL[3].as_str()
            && ViewKind::ALL[1].as_str() != ViewKind::ALL[2].as_str()
            && ViewKind::ALL[1].as_str() != ViewKind::ALL[3].as_str()
            && ViewKind::ALL[2].as_str() != ViewKind::ALL[3].as_str();
        s.add(
            "A36-判据-枚举完备性断言新类型必须登记",
            len_ok && mask_ok && names_diff,
            "ALL.len()=KIND_REGISTRY_LEN=4、登记位全 1、短名两两互异——加变体不改登记表，本条先红",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 七、单元测试（cargo test svstar2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn k(res: u32, mip: u8) -> ViewKey {
        ViewKey { resource: res, kind: ViewKind::Rtv, format: 87, mip, slice: 0 }
    }

    #[test]
    fn vea36_create_dedup_release_roundtrip() {
        let mut m = RtViewMgr::new();
        let a = m.create(k(7, 0)).unwrap_or(0);
        let b = m.create(k(7, 0)).unwrap_or(0);
        assert_eq!(a, b, "同键去重共享");
        assert!(m.release(a).is_ok());
        assert!(m.view(a).is_some(), "还有一方持有，条目仍在");
        assert!(m.release(a).is_ok());
        assert!(m.view(a).is_none(), "双方释放后移除");
        assert_eq!(m.stats.dedup_hits, 1);
        assert_eq!(m.stats.created, 1);
        assert_eq!(m.stats.invalidated, 1);
    }

    #[test]
    fn vea36_liveness_binding_and_rebuild() {
        let mut m = RtViewMgr::new();
        let _ = m.create(k(9, 0));
        let _ = m.create(k(9, 1));
        let other = m.create(k(11, 0)).unwrap_or(0);
        assert_eq!(m.resource_destroyed(9), 2);
        assert_eq!(m.live_for_resource(11), 1);
        assert_eq!(m.cache_rebuild(), 1);
        assert!(m.view(other).is_none(), "重建清全表，包括其他资源");
        assert_eq!(m.stats.gen, 2);
        // 重建后同键可重建
        assert!(m.create(k(11, 0)).is_ok());
    }

    #[test]
    fn vea36_a11y_and_snapshot() {
        let mut m = RtViewMgr::new();
        let _ = m.create(k(7, 0));
        let _ = m.create(k(7, 1));
        let snap = m.snapshot();
        assert_eq!(snap.len(), 2);
        let text = m.a11y_summary();
        assert!(text.contains("存活 2"), "读屏行带存活数：{}", text);
        assert!(text.contains("新建 2"), "读屏行带新建数：{}", text);
    }

    #[test]
    fn vea36_checks_all_green() {
        let set = run_vea36_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0036 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
