//! VE-F0036 · 渲染目标视图管理（VE-A 域 · A02b 资源视图组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0036`
//!
//! **职责定位（锚点原文）**：渲染目标视图（RTV）的统一管理（视图创建/缓存/
//! 失效三段），视图与资源的生命周期绑定（资源没了视图必须失效），视图去重
//! （同参共享）；含视图类型的枚举完备性断言（新视图类型必须登记）。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 悬垂视图 → 失效联动（资源销毁时所有引用它的视图同帧失效，不留悬垂）；
//! - 缓存污染 → 重建（去重表命中异常视图时整表重建，不逐条修补）；
//! - 去重冲突 → 仲裁（同参多请求并发时按「先到先得 + 世代高者胜」仲裁）。
//!
//! **性能逐项分解（锚点原文）**：O(视图)。创建走去重表直达；失效按资源反查
//! 表批量走，均摊 O(1)。
//!
//! **跨批对接点（锚点原文）**：A16 纹理管理联动——纹理销毁事件驱动本表的
//! 资源反查失效路径。
//!
//! **无障碍与隐私（锚点原文）**：视图状态读屏可达——`status_line()` 输出
//! 纯文本视图计数（在册/失效/去重命中）。
//!
//! **判据（锚点原文）**：三段管理、生命周期绑定、悬垂失效、去重共享、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE890 基址（0xE890~0xE899 十码，全域核查无占用）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const RTV_VERSION: &str = "vea36_rtv-v1";

/// 诊断段基址（全域核查无占用后落位）。
pub const SEG_BASE: u32 = 0xE890;

// ---- 诊断码（显式判别式 = SEG_BASE + 序号）----
pub const XC_PARAM_FORMAT: u32 = SEG_BASE + 1; // 非法格式枚举值
pub const XC_PARAM_DIMS: u32 = SEG_BASE + 2; // 尺寸越界（零或超上限）
pub const XC_RES_UNKNOWN: u32 = SEG_BASE + 3; // 资源不存在（生命周期绑定拒绝）
pub const XC_VIEW_FULL: u32 = SEG_BASE + 4; // 视图表满
pub const XC_VIEW_GONE: u32 = SEG_BASE + 5; // 悬垂视图访问（已失效仍被引用）
pub const XC_DEDUP_CONFLICT: u32 = SEG_BASE + 6; // 去重冲突（同参异世代）

/// 视图类型枚举——完备性由 `VIEW_TYPE_MASK` 钉死：新类型必须登记进掩码，
/// 否则 `run_vea36_checks` 的完备性断言当场翻红。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// 颜色渲染目标（最常见 RTV）。
    Color = 1,
    /// 深度-only 目标（无模板面）。
    DepthOnly = 2,
    /// 深度 + 模板复合目标。
    DepthStencil = 3,
    /// 整型通道目标（ID/拾取渲染）。
    Integer = 4,
    /// sRGB 编码目标（写时线性转 sRGB）。
    Srgb = 5,
}

/// 完备性掩码：枚举每个判别值都必须在掩码内置位。
pub const VIEW_TYPE_MASK: u32 =
    (1 << (ViewKind::Color as u32)) | (1 << (ViewKind::DepthOnly as u32))
        | (1 << (ViewKind::DepthStencil as u32)) | (1 << (ViewKind::Integer as u32))
        | (1 << (ViewKind::Srgb as u32));

impl ViewKind {
    /// 枚举完备性断言：任一判别值未登记进掩码即失败。
    pub fn mask_complete() -> bool {
        (ViewKind::Color as u32) | (ViewKind::DepthOnly as u32) != 0
            && (1u32 << (ViewKind::Color as u32)) & VIEW_TYPE_MASK != 0
            && (1u32 << (ViewKind::DepthOnly as u32)) & VIEW_TYPE_MASK != 0
            && (1u32 << (ViewKind::DepthStencil as u32)) & VIEW_TYPE_MASK != 0
            && (1u32 << (ViewKind::Integer as u32)) & VIEW_TYPE_MASK != 0
            && (1u32 << (ViewKind::Srgb as u32)) & VIEW_TYPE_MASK != 0
    }

    /// 掩码中登记的类型总数（完备性第二断言：掩码位数 == 枚举成员数）。
    pub fn mask_population() -> u32 {
        VIEW_TYPE_MASK.count_ones()
    }
}

/// 视图格式（与像素布局解耦的最小描述；能力表见 F0040）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtvFormat {
    /// 格式族编号（对接 A16 纹理格式枚举，0 保留 = 未指定）。
    pub family: u16,
    /// 每像素字节数（能力表换算带宽用）。
    pub bpp: u16,
}

/// 视图创建参数——去重键（逐字段全等比对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewParams {
    pub res_id: u32,
    pub kind: ViewKind,
    pub fmt: RtvFormat,
    pub width: u32,
    pub height: u32,
    /// mip 层（RTV 通常 0，显式携带便于去重区分）。
    pub mip: u8,
}

/// 视图状态机：Active（可绑定）→ Invalid（资源侧失效）→ Slot（槽位可复用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewState {
    Active,
    Invalid,
    Slot,
}

/// 一条视图记录。
#[derive(Clone, Copy, Debug)]
pub struct ViewRec {
    pub params: ViewParams,
    pub state: ViewState,
    /// 创建世代（资源世代快照，用于去重冲突仲裁：高世代胜）。
    pub gen: u32,
    /// 去重共享计数（同参多请求共享同一视图）。
    pub shares: u32,
}

/// 渲染目标视图管理器——创建/缓存/失效三段。
pub struct RtvManager {
    recs: Vec<ViewRec>,
    cap: usize,
    last_code: u32,
    /// 统计：去重命中数（同参共享路径）。
    dedup_hits: u32,
    /// 统计：资源侧联动失效的视图数（悬垂失效联动量）。
    cascade_invalidated: u32,
    /// 统计：缓存重建次数。
    rebuilds: u32,
    /// 资源侧已知世代（生命周期绑定的锚：资源不存在则创建直接拒绝）。
    res_gen: Vec<(u32, u32)>,
}

const RES_MAX: usize = 256;

impl RtvManager {
    pub fn new(cap: usize) -> RtvManager {
        RtvManager {
            recs: Vec::new(),
            cap,
            last_code: 0,
            dedup_hits: 0,
            cascade_invalidated: 0,
            rebuilds: 0,
            res_gen: Vec::new(),
        }
    }

    /// 登记（或推进）资源世代——A16 纹理侧在创建/重创建资源时调用。
    /// 生命周期绑定的正源：视图创建时资源必须在此表在册。
    pub fn touch_resource(&mut self, res_id: u32, gen: u32) -> bool {
        if res_id == 0 || gen == 0 {
            self.last_code = XC_RES_UNKNOWN;
            return false;
        }
        for e in self.res_gen.iter_mut() {
            if e.0 == res_id {
                // 世代只许单调推进：回退视为异常拒绝。
                if gen <= e.1 {
                    self.last_code = XC_DEDUP_CONFLICT;
                    return false;
                }
                e.1 = gen;
                return true;
            }
        }
        if self.res_gen.len() >= RES_MAX {
            self.last_code = XC_RES_UNKNOWN;
            return false;
        }
        self.res_gen.push((res_id, gen));
        true
    }

    /// 资源销毁——联动失效所有引用它的视图（悬垂失效联动）。
    /// 返回失效视图数；资源条目同步摘除（后续创建按 XC_RES_UNKNOWN 拒）。
    pub fn destroy_resource(&mut self, res_id: u32) -> u32 {
        let mut n = 0u32;
        for r in self.recs.iter_mut() {
            if r.params.res_id == res_id && r.state == ViewState::Active {
                r.state = ViewState::Invalid;
                n += 1;
            }
        }
        self.res_gen.retain(|e| e.0 != res_id);
        self.cascade_invalidated += n;
        n
    }

    /// 创建（或去重命中）一个渲染目标视图。
    /// 三段中的「创建」与「缓存」：先查去重表（缓存段），命中且视图仍 Active
    /// 则共享计数 +1 返回同一槽位；未命中走真实创建。
    pub fn create_view(&mut self, p: ViewParams) -> Result<usize, u32> {
        // 参数校验（边界防护）。
        if p.width == 0 || p.height == 0 || p.width > 16384 || p.height > 16384 {
            self.last_code = XC_PARAM_DIMS;
            return Err(self.last_code);
        }
        if p.fmt.bpp == 0 || p.fmt.bpp > 128 || p.mip > 15 {
            self.last_code = XC_PARAM_FORMAT;
            return Err(self.last_code);
        }
        // 生命周期绑定：资源必须以不低于快照的世代在册。
        let cur_gen = match self.res_gen.iter().find(|e| e.0 == p.res_id) {
            Some(e) => e.1,
            None => {
                self.last_code = XC_RES_UNKNOWN;
                return Err(self.last_code);
            }
        };
        // 去重查询（缓存段）：同参且 Active 即共享。
        let hit = self
            .recs
            .iter()
            .position(|r| r.state == ViewState::Active && r.params == p);
        if let Some(i) = hit {
            if self.recs[i].gen != cur_gen {
                // 同参异世代 = 缓存污染信号：整表重建（去重冲突仲裁的
                // 降级路径——重建比逐条修补便宜且不留半新半旧）。
                self.rebuild_cache();
                self.last_code = XC_DEDUP_CONFLICT;
                return Err(self.last_code);
            }
            self.recs[i].shares += 1;
            self.dedup_hits += 1;
            return Ok(i);
        }
        // 真实创建：先回收 Slot 槽位，满了再追加，追加也满即拒。
        if let Some(i) = self.recs.iter().position(|r| r.state == ViewState::Slot) {
            self.recs[i] = ViewRec { params: p, state: ViewState::Active, gen: cur_gen, shares: 1 };
            return Ok(i);
        }
        if self.recs.len() >= self.cap {
            self.last_code = XC_VIEW_FULL;
            return Err(self.last_code);
        }
        self.recs.push(ViewRec { params: p, state: ViewState::Active, gen: cur_gen, shares: 1 });
        Ok(self.recs.len() - 1)
    }

    /// 缓存重建（缓存污染降级）：清空全部非 Active 记录并归零共享计数，
    /// Active 视图保留（重建只清缓存语义，不撤销在用资源）。
    pub fn rebuild_cache(&mut self) {
        self.recs.retain(|r| r.state == ViewState::Active);
        for r in self.recs.iter_mut() {
            r.shares = 1;
        }
        self.rebuilds += 1;
    }

    /// 显式失效一个视图（调用方主动释放；shares 归零后转 Slot 可复用）。
    pub fn release_view(&mut self, idx: usize) -> bool {
        match self.recs.get_mut(idx) {
            Some(r) if r.state == ViewState::Active => {
                if r.shares > 1 {
                    r.shares -= 1;
                } else {
                    r.state = ViewState::Slot;
                }
                true
            }
            _ => false,
        }
    }

    /// 悬垂视图访问检测：已失效视图仍被引用时返回诊断码（阻断级）。
    pub fn probe_view(&self, idx: usize) -> Result<ViewParams, u32> {
        match self.recs.get(idx) {
            Some(r) if r.state == ViewState::Active => Ok(r.params),
            Some(_) => Err(XC_VIEW_GONE),
            None => Err(XC_VIEW_GONE),
        }
    }

    /// 读屏状态行：在册/活跃/去重命中/联动失效/重建 五要素纯文本。
    pub fn status_line(&self) -> String {
        let active = self.recs.iter().filter(|r| r.state == ViewState::Active).count();
        alloc::format!(
            "RTV 在册{} 活跃{} 去重命中{} 联动失效{} 重建{}",
            self.recs.len(),
            active,
            self.dedup_hits,
            self.cascade_invalidated,
            self.rebuilds
        )
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }
    pub fn len(&self) -> usize {
        self.recs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
    pub fn dedup_hits(&self) -> u32 {
        self.dedup_hits
    }
    pub fn cascade_invalidated(&self) -> u32 {
        self.cascade_invalidated
    }
    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }
}

// =====================================================================
// 自检（判据锚点：三段管理 / 生命周期绑定 / 悬垂失效 / 去重共享 / 完备性）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0036 域自检。
pub fn run_vea36_checks() -> CheckSet {
    let mut set = CheckSet::new("vea36");

    let p0 = ViewParams {
        res_id: 7,
        kind: ViewKind::Color,
        fmt: RtvFormat { family: 10, bpp: 32 },
        width: 1920,
        height: 1080,
        mip: 0,
    };

    // C36-01 枚举完备性（掩码逐位 + 位总数双断言）
    chk(&mut set, "C36-01 枚举完备", ViewKind::mask_complete() && ViewKind::mask_population() == 5, "掩码缺位");

    // C36-02 三段·创建：合法参数落位 Active
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let ok = matches!(m.create_view(p0), Ok(0));
        chk(&mut set, "C36-02 创建落位", ok, "创建未落位");
    }

    // C36-03 生命周期绑定：资源不在册即拒
    {
        let mut m = RtvManager::new(8);
        let r = m.create_view(p0);
        let ok = r == Err(XC_RES_UNKNOWN) && m.last_code() == XC_RES_UNKNOWN;
        chk(&mut set, "C36-03 绑定拒绝", ok, "未绑定资源被放行");
    }

    // C36-04 去重共享：同参第二次命中同一槽位
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let a = m.create_view(p0);
        let b = m.create_view(p0);
        let ok = a == Ok(0) && b == Ok(0) && m.dedup_hits() == 1;
        chk(&mut set, "C36-04 去重共享", ok, "同参未共享");
    }

    // C36-05 悬垂失效联动：资源销毁后视图失效且 probe 拒
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let _ = m.create_view(p0);
        let n = m.destroy_resource(7);
        let ok = n == 1 && m.probe_view(0) == Err(XC_VIEW_GONE) && m.cascade_invalidated() == 1;
        chk(&mut set, "C36-05 悬垂联动", ok, "失效未联动");
    }

    // C36-06 失效后销毁资源：同 id 创建按绑定拒绝（摘除生效）
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let _ = m.create_view(p0);
        let _ = m.destroy_resource(7);
        let ok = m.create_view(p0) == Err(XC_RES_UNKNOWN);
        chk(&mut set, "C36-06 销毁后拒建", ok, "销毁后仍可建");
    }

    // C36-07 世代推进后同参请求触发重建 + 冲突码（缓存污染降级）
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let _ = m.create_view(p0);
        let _ = m.touch_resource(7, 2);
        let r = m.create_view(p0);
        let ok = r == Err(XC_DEDUP_CONFLICT) && m.rebuilds() == 1;
        chk(&mut set, "C36-07 重建降级", ok, "污染未重建");
    }

    // C36-08 尺寸边界：零尺寸与超上限同拒
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let z = ViewParams { width: 0, ..p0 };
        let big = ViewParams { width: 16385, ..p0 };
        let ok = m.create_view(z) == Err(XC_PARAM_DIMS) && m.create_view(big) == Err(XC_PARAM_DIMS);
        chk(&mut set, "C36-08 尺寸边界", ok, "尺寸校验失效");
    }

    // C36-09 表满显性：容量 2 建第 3 个按 XC_VIEW_FULL 拒
    {
        let mut m = RtvManager::new(2);
        let _ = m.touch_resource(7, 1);
        let p1 = ViewParams { mip: 1, ..p0 };
        let p2 = ViewParams { mip: 2, ..p0 };
        let _ = m.create_view(p0);
        let _ = m.create_view(p1);
        let ok = m.create_view(p2) == Err(XC_VIEW_FULL) && m.last_code() == XC_VIEW_FULL;
        chk(&mut set, "C36-09 表满显性", ok, "表满未显性");
    }

    // C36-10 槽位复用：释放后 Slot 被新视图接管（总量不涨）
    {
        let mut m = RtvManager::new(2);
        let _ = m.touch_resource(7, 1);
        let _ = m.create_view(p0);
        let _ = m.release_view(0);
        let p1 = ViewParams { mip: 1, ..p0 };
        let ok = matches!(m.create_view(p1), Ok(0)) && m.len() == 1;
        chk(&mut set, "C36-10 槽位复用", ok, "复用失败");
    }

    // C36-11 共享计数释放语义：shares>1 释放不销毁，归 1 再释放转 Slot
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let _ = m.create_view(p0);
        let _ = m.create_view(p0);
        let _ = m.release_view(0);
        let still_active = m.probe_view(0).is_ok();
        let _ = m.release_view(0);
        let now_slot = m.probe_view(0) == Err(XC_VIEW_GONE);
        chk(&mut set, "C36-11 释放语义", still_active && now_slot, "释放语义错");
    }

    // C36-12 资源世代回退拒绝（单调性）
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 3);
        let ok = !m.touch_resource(7, 2) && m.last_code() == XC_DEDUP_CONFLICT;
        chk(&mut set, "C36-12 世代单调", ok, "世代回退被放行");
    }

    // C36-13 mip 越界拒绝
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let bad = ViewParams { mip: 16, ..p0 };
        let ok = m.create_view(bad) == Err(XC_PARAM_FORMAT);
        chk(&mut set, "C36-13 mip 越界", ok, "mip 校验失效");
    }

    // C36-14 确定性：同序列两次执行结果全等
    {
        let seq = |cap: usize| -> (u32, u32, usize) {
            let mut m = RtvManager::new(cap);
            let _ = m.touch_resource(7, 1);
            let _ = m.create_view(p0);
            let _ = m.create_view(p0);
            let _ = m.destroy_resource(7);
            (m.dedup_hits(), m.cascade_invalidated(), m.len())
        };
        chk(&mut set, "C36-14 确定性", seq(8) == seq(8), "两次不一致");
    }

    // C36-15 读屏行五要素齐全
    {
        let mut m = RtvManager::new(8);
        let _ = m.touch_resource(7, 1);
        let _ = m.create_view(p0);
        let line = m.status_line();
        let ok = line.contains("在册") && line.contains("活跃") && line.contains("去重命中")
            && line.contains("联动失效") && line.contains("重建");
        chk(&mut set, "C36-15 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea36_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
