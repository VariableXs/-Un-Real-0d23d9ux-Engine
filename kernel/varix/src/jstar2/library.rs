//! F628 指针方案库管理器 · 完整设计（STAR I 主册 J-C 组）· v2 深化版。
//!
//! **判据（主册原文）**：缩略墙渲染（含动态帧循环）；三视图过滤；导入
//! 导出往返一致；上限提醒不静默删；与 E4 同源对账（两处清单 100% 一致）。
//!
//! **定位（主册边界声明）**：E4 是设置页的切换入口（前柜），方案库是
//! 管理面（库房）——E4 列表即库房清单的投影（`e4_projection()` 唯一
//! 投影口，对账 = 两处清单逐项相等）。
//!
//! **库房语义**：
//! - 容量 50（超限 `add` 返回 [`AddOutcome::OverflowReminder`]——携带
//!   建议淘汰名单（最久未用优先），**由用户决定删谁**，库房绝不静默删）；
//! - 三视图：标签 / 收藏 / 最近使用（`last_used` 降序）——过滤是投影
//!   不是移动（条目唯一存储）；视图计数面（徽标数字）与组合过滤
//!   （收藏 ∩ 标签——收藏夹里再分门别类）；库内按名搜索（判据「双
//!   入口可达」的库房内入口：翻墙太长就搜）；
//! - 缩略墙：每方案 15 态预览（实际像素盒式降采样——墙上看到的是
//!   真图不是占位块）+ 动态帧循环游标（`thumb_frame_index_at` 按注入
//!   时钟推进帧游标——动画方案在墙上会动，时序确定性可测）+ 悬停
//!   播放/暂停语义（`set_wall_playing`——不悬停不空转，墙面的礼貌）；
//! - 标签管理：打/摘/全库清单（去重排序）/计数（标签面板的数字面）；
//! - 导入导出：单方案 .vxcur 序列化往返（jbase 容器），导出哈希 ==
//!   导入后重序列化哈希（jbase 指纹口径）；
//! - 体检联动：`record_report` 把 F627 报告挂到条目（指纹对账——报告
//!   的 `scheme_fingerprint` 必须等于当前内容指纹，改版后旧报告失效）；
//!   `health_sweep()` 全库扫描过期报告（改版后未复检的条目数——
//!   报告与内容同步的维护面）；
//! - 删除的 active 语义：删掉在用方案时 `active_name` 如实清空
//!   （E4 前柜不留幽灵指针——回退默认由 E4/设置页决定，库房只诚实）。

use crate::checks::CheckSet;
use crate::jstar2::checker::HealthReport;
use crate::jstar2::jbase::{
    parse_vxcur, serialize_vxcur, vxcur_fingerprint, CursorSchemeModel, PointerState,
    ALL_STATES, VXCUR_MAX_BYTES,
};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 库房容量上限（判据「上限提醒不静默删」）。
pub const LIBRARY_CAP: usize = 50;

/// 墙面缩略预览边长（盒式降采样目标——墙上一格）。
pub const WALL_THUMB_PX: u16 = 16;

// ---------------------------------------------------------------------------
// 条目模型
// ---------------------------------------------------------------------------

/// 库房条目（唯一存储；E4 投影与缩略墙都从这读）。
#[derive(Clone, Debug)]
pub struct SchemeEntry {
    pub model: CursorSchemeModel,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub added_ms: u64,
    pub last_used_ms: u64,
    /// 最近体检报告（None = 未体检/改版后失效）。
    pub report: Option<HealthReport>,
    /// 动态帧游标（缩略墙循环播放进度，按态记）。
    thumb_frame: usize,
    /// 墙面播放开关（悬停播放语义——默认播放）。
    wall_playing: bool,
}

impl SchemeEntry {
    pub fn fingerprint(&self) -> u64 {
        vxcur_fingerprint(&self.model)
    }

    pub fn name(&self) -> &str {
        &self.model.name
    }

    /// 墙面播放状态（只读面）。
    pub fn is_wall_playing(&self) -> bool {
        self.wall_playing
    }
}

/// 库房。
#[derive(Clone, Debug)]
pub struct SchemeLibrary {
    entries: Vec<SchemeEntry>,
    /// 当前应用中的方案名（E4 前柜的「在用」标记；库房不做切换语义，
    /// 只如实记录——切换动作归 E4/设置页）。
    pub active_name: String,
    now_ms: u64,
}

/// 入库结果（上限提醒不静默删的机制面）。
#[derive(Clone, Debug)]
pub enum AddOutcome {
    /// 已入库（条目指纹）。
    Added(u64),
    /// 库满被拒——携带建议淘汰名单（最久未用前 `suggest` 个），
    /// 由用户决定；库房分毫未动。
    OverflowReminder {
        suggestion: Vec<String>,
        pending: String,
    },
    /// 重名（同内容指纹）——幂等命中，返回既有指纹。
    Duplicate(u64),
}

/// 视图过滤（三视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryView {
    /// 全部（标签视图基线：按标签过滤是参数化版本）。
    All,
    Favorites,
    Recent,
}

impl SchemeLibrary {
    pub fn new(now_ms: u64) -> SchemeLibrary {
        SchemeLibrary { entries: Vec::new(), active_name: String::new(), now_ms }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn now(&self) -> u64 {
        self.now_ms
    }

    pub fn set_now(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    fn push_entry(&mut self, model: CursorSchemeModel) -> u64 {
        let fp = vxcur_fingerprint(&model);
        self.entries.push(SchemeEntry {
            model,
            tags: Vec::new(),
            favorite: false,
            added_ms: self.now_ms,
            last_used_ms: self.now_ms,
            report: None,
            thumb_frame: 0,
            wall_playing: true,
        });
        fp
    }

    /// 入库（满 50 时返回提醒而非静默删）。
    pub fn add(&mut self, model: CursorSchemeModel) -> AddOutcome {
        let fp = vxcur_fingerprint(&model);
        if self.entries.iter().any(|e| e.fingerprint() == fp) {
            return AddOutcome::Duplicate(fp);
        }
        if self.entries.len() >= LIBRARY_CAP {
            // 建议名单：最久未用优先（不静默删——只建议）。
            let mut by_old: Vec<&SchemeEntry> = self.entries.iter().collect();
            by_old.sort_by_key(|e| e.last_used_ms);
            let suggestion =
                by_old.iter().take(3).map(|e| e.model.name.clone()).collect();
            return AddOutcome::OverflowReminder {
                suggestion,
                pending: model.name.clone(),
            };
        }
        self.push_entry(model);
        AddOutcome::Added(fp)
    }

    /// 入库后强制放行（用户在提醒面板上明确淘汰后调用——库房自己的
    /// 决策仍守上限，这条入口是「用户已拍板」的通道）。
    pub fn add_after_user_eviction(
        &mut self,
        evict_name: &str,
        model: CursorSchemeModel,
    ) -> Result<u64, &'static str> {
        let before = self.entries.len();
        self.entries.retain(|e| e.model.name != evict_name);
        if self.entries.len() == before {
            return Err("淘汰目标不存在——拒绝执行（不静默）");
        }
        if self.entries.len() >= LIBRARY_CAP {
            return Err("淘汰后仍满库——异常拒绝");
        }
        Ok(self.push_entry(model))
    }

    /// 删除（在用方案被删 → `active_name` 如实清空——E4 前柜不留幽灵）。
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        let was_active = self.active_name == name;
        self.entries.retain(|e| e.model.name != name);
        let removed = self.entries.len() != before;
        if removed && was_active {
            self.active_name = String::new();
        }
        removed
    }

    pub fn get(&self, name: &str) -> Option<&SchemeEntry> {
        self.entries.iter().find(|e| e.model.name == name)
    }

    /// 指纹查重（内容级幂等判断——F630/F635 导入链前置检查用，不插入）。
    pub fn contains_fingerprint(&self, fp: u64) -> bool {
        self.entries.iter().any(|e| e.fingerprint() == fp)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut SchemeEntry> {
        self.entries.iter_mut().find(|e| e.model.name == name)
    }

    // ---- 标签管理 -------------------------------------------------------

    /// 打标签（幂等——重复打同一标签不加第二条）。
    pub fn tag(&mut self, name: &str, tag: &str) -> bool {
        let Some(e) = self.get_mut(name) else { return false };
        if !e.tags.iter().any(|t| t == tag) {
            e.tags.push(String::from(tag));
        }
        true
    }

    /// 摘标签（不存在摘不动 → false）。
    pub fn untag(&mut self, name: &str, tag: &str) -> bool {
        let Some(e) = self.get_mut(name) else { return false };
        let before = e.tags.len();
        e.tags.retain(|t| t != tag);
        e.tags.len() != before
    }

    /// 全库标签清单（去重 + 字节序——标签面板的数据源，一处一事实）。
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for e in &self.entries {
            for t in &e.tags {
                if !tags.iter().any(|g| g == t) {
                    tags.push(t.clone());
                }
            }
        }
        tags.sort();
        tags
    }

    /// 标签计数（标签名 → 条目数——标签面板的数字面）。
    pub fn tag_counts(&self) -> Vec<(String, usize)> {
        self.all_tags()
            .into_iter()
            .map(|t| {
                let n = self.entries.iter().filter(|e| e.tags.iter().any(|g| *g == t)).count();
                (t, n)
            })
            .collect()
    }

    pub fn set_favorite(&mut self, name: &str, fav: bool) -> bool {
        let Some(e) = self.get_mut(name) else { return false };
        e.favorite = fav;
        true
    }

    // ---- E4 前柜联动 -----------------------------------------------------

    /// E4 前柜切换回写（库房记录「在用」并刷新 last_used）。
    pub fn mark_active(&mut self, name: &str) -> bool {
        if self.get(name).is_none() {
            return false;
        }
        self.active_name = String::from(name);
        let now = self.now_ms;
        if let Some(e) = self.get_mut(name) {
            e.last_used_ms = now;
        }
        true
    }

    /// 显式使用登记（不切换只刷新最近使用——墙上的预览点击、体检
    /// 打开等「用过但没切换」的场景）。
    pub fn touch(&mut self, name: &str) -> bool {
        let now = self.now_ms;
        match self.get_mut(name) {
            Some(e) => {
                e.last_used_ms = now;
                true
            }
            None => false,
        }
    }

    // ---- 三视图（投影不是移动）----

    pub fn view(&self, v: LibraryView) -> Vec<&SchemeEntry> {
        let mut list: Vec<&SchemeEntry> = match v {
            LibraryView::All => self.entries.iter().collect(),
            LibraryView::Favorites => self.entries.iter().filter(|e| e.favorite).collect(),
            LibraryView::Recent => {
                let mut l: Vec<&SchemeEntry> = self.entries.iter().collect();
                l.sort_by(|a, b| b.last_used_ms.cmp(&a.last_used_ms));
                l
            }
        };
        if v != LibraryView::Recent {
            list.sort_by(|a, b| a.model.name.cmp(&b.model.name));
        }
        list
    }

    /// 标签过滤视图（参数化标签视图）。
    pub fn view_by_tag(&self, tag: &str) -> Vec<&SchemeEntry> {
        let mut l: Vec<&SchemeEntry> =
            self.entries.iter().filter(|e| e.tags.iter().any(|t| t == tag)).collect();
        l.sort_by(|a, b| a.model.name.cmp(&b.model.name));
        l
    }

    /// 组合过滤：收藏 ∩ 标签（收藏夹里再分门别类——收藏多了以后
    /// 平铺就是第二次捉迷藏）。
    pub fn view_favorites_tagged(&self, tag: &str) -> Vec<&SchemeEntry> {
        let mut l: Vec<&SchemeEntry> = self
            .entries
            .iter()
            .filter(|e| e.favorite && e.tags.iter().any(|t| t == tag))
            .collect();
        l.sort_by(|a, b| a.model.name.cmp(&b.model.name));
        l
    }

    /// 库内按名搜索（子串命中——「双入口可达」的库房内入口；空查询
    /// 返回全部，视图语义与 All 一致）。
    pub fn search(&self, query: &str) -> Vec<&SchemeEntry> {
        let mut l: Vec<&SchemeEntry> = self
            .entries
            .iter()
            .filter(|e| query.is_empty() || e.model.name.contains(query))
            .collect();
        l.sort_by(|a, b| a.model.name.cmp(&b.model.name));
        l
    }

    /// 视图计数（徽标数字面——页签上那个数）。
    pub fn view_counts(&self) -> (usize, usize, usize) {
        (self.entries.len(), self.entries.iter().filter(|e| e.favorite).count(), {
            let mut l: Vec<&SchemeEntry> = self.entries.iter().collect();
            l.sort_by_key(|e| core::cmp::Reverse(e.last_used_ms));
            l.len()
        })
    }

    // ---- E4 同源对账 ----

    /// E4 前柜清单投影（唯一投影口——E4 页面渲染直接用这份数据，
    /// 两处清单 100% 一致是构造保证，对账函数供 CI/自检复核）。
    pub fn e4_projection(&self) -> Vec<(String, bool)> {
        let mut l: Vec<(String, bool)> = self
            .entries
            .iter()
            .map(|e| (e.model.name.clone(), e.model.name == self.active_name))
            .collect();
        l.sort_by(|a, b| a.0.cmp(&b.0));
        l
    }

    /// 对账：E4 侧带来的清单与库房投影逐项相等（判据 100% 一致；
    /// E4 侧多出的幽灵名同样失配——投影面不容两套事实）。
    pub fn reconcile_e4(&self, e4_list: &[(String, bool)]) -> bool {
        self.e4_projection() == e4_list.to_vec()
    }

    // ---- 缩略墙 ----

    /// 某方案某态在时钟 `at_ms` 的墙预览帧号（动态帧循环：按帧延时推进；
    /// 静态方案恒 0；悬停暂停语义——暂停时冻结当前游标不推进）。
    pub fn thumb_frame_index_at(&mut self, name: &str, st: PointerState, at_ms: u64) -> Option<usize> {
        let e = self.entries.iter_mut().find(|e| e.model.name == name)?;
        let sf = e.model.state(st)?;
        if sf.frames.is_empty() {
            return None;
        }
        if !e.wall_playing {
            return Some(e.thumb_frame);
        }
        let idx = if sf.frames.len() > 1 {
            let total: u64 = sf.frames.iter().map(|f| f.delay_ms.max(1) as u64).sum();
            let phase = if total == 0 { 0 } else { at_ms % total };
            let mut idx = 0usize;
            let mut acc = 0u64;
            for (i, f) in sf.frames.iter().enumerate() {
                acc += f.delay_ms.max(1) as u64;
                if phase < acc {
                    idx = i;
                    break;
                }
                idx = i;
            }
            idx
        } else {
            0
        };
        e.thumb_frame = idx;
        Some(idx)
    }

    /// 墙面播放/暂停（悬停播放语义——暂停冻结游标，恢复从暂停位续播）。
    pub fn set_wall_playing(&mut self, name: &str, playing: bool) -> bool {
        match self.get_mut(name) {
            Some(e) => {
                e.wall_playing = playing;
                true
            }
            None => false,
        }
    }

    /// 墙上单方案的 15 态预览清单（态缺即缺——墙上如实显示缺口）。
    pub fn thumb_wall(&self, name: &str) -> Option<Vec<(PointerState, bool)>> {
        let e = self.get(name)?;
        Some(
            ALL_STATES
                .iter()
                .map(|st| (*st, e.model.state(*st).is_some()))
                .collect(),
        )
    }

    /// 墙面某态某帧的实际像素预览（盒式降采样到 `size` 边长——墙上
    /// 看到的是真图不是占位块；盒内有实体取实体均值，全空透明）。
    pub fn thumb_preview(&self, name: &str, st: PointerState, frame_i: usize, size: u16) -> Option<PixBufLike> {
        let e = self.get(name)?;
        let sf = e.model.state(st)?;
        let f = sf.frames.get(frame_i)?;
        if size == 0 {
            return None;
        }
        let step = (f.w as u32 / size as u32).max(1) as i64;
        let mut rows: Vec<Vec<[u8; 4]>> = Vec::with_capacity(size as usize);
        for ty in 0..size as i64 {
            let mut row = Vec::with_capacity(size as usize);
            for tx in 0..size as i64 {
                let mut sr: u32 = 0;
                let mut sg: u32 = 0;
                let mut sb: u32 = 0;
                let mut n: u32 = 0;
                let mut max_a: u8 = 0;
                for oy in 0..step {
                    for ox in 0..step {
                        let sx = tx * step + ox;
                        let sy = ty * step + oy;
                        if sx >= f.w as i64 || sy >= f.h as i64 {
                            continue;
                        }
                        let base = ((sy * f.w as i64 + sx) * 4) as usize;
                        if base + 3 < f.px.len() {
                            let rgba = [f.px[base], f.px[base + 1], f.px[base + 2], f.px[base + 3]];
                            if rgba[3] >= 128 {
                                sr += rgba[0] as u32;
                                sg += rgba[1] as u32;
                                sb += rgba[2] as u32;
                                n += 1;
                                max_a = max_a.max(rgba[3]);
                            }
                        }
                    }
                }
                if n > 0 {
                    row.push([(sr / n) as u8, (sg / n) as u8, (sb / n) as u8, max_a]);
                } else {
                    row.push([0, 0, 0, 0]);
                }
            }
            rows.push(row);
        }
        Some(PixBufLike { size, rows })
    }

    // ---- 体检联动 ----

    /// 挂体检报告（指纹对账：报告必须属于当前内容，否则拒绝挂载）。
    pub fn record_report(&mut self, name: &str, report: HealthReport) -> Result<(), &'static str> {
        let Some(e) = self.get_mut(name) else { return Err("方案不存在") };
        if e.fingerprint() != report.scheme_fingerprint {
            return Err("报告指纹与当前内容不一致——改版后旧报告失效，请重跑 F627 体检");
        }
        e.report = Some(report);
        Ok(())
    }

    pub fn report_of(&self, name: &str) -> Option<&HealthReport> {
        self.get(name).and_then(|e| e.report.as_ref())
    }

    /// 全库体检扫描：报告过期（指纹与当前内容不符）或缺失的条目数
    /// ——报告与内容同步的维护面（扫出来提醒复检，不静默过期）。
    pub fn health_sweep(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| match &e.report {
                None => true,
                Some(r) => r.scheme_fingerprint != e.fingerprint(),
            })
            .map(|e| e.model.name.clone())
            .collect()
    }

    // ---- 导入导出 ----

    /// 导出：方案 → .vxcur 字节（往返一致判据的导出侧）。
    pub fn export_scheme(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.get(name)?;
        Some(serialize_vxcur(&e.model))
    }

    /// 导入：.vxcur 字节 → 入库为副本（指纹幂等；上限提醒同 `add`）。
    pub fn import_scheme(&mut self, bytes: &[u8]) -> Result<AddOutcome, &'static str> {
        if bytes.len() > VXCUR_MAX_BYTES {
            return Err("文件超过 4MB 容器上限——拒绝导入");
        }
        let model = parse_vxcur(bytes).map_err(|_| "文件解析失败——不是合法的 .vxcur 指针方案")?;
        Ok(self.add(model))
    }

    /// 导入并改名（分享件带自己的名字进来时用户想用别的名字——
    /// 改名发生在指纹计算前：内容相同名字不同 = 两条目）。
    pub fn import_scheme_as(&mut self, bytes: &[u8], rename: &str) -> Result<AddOutcome, &'static str> {
        if bytes.len() > VXCUR_MAX_BYTES {
            return Err("文件超过 4MB 容器上限——拒绝导入");
        }
        let mut model = parse_vxcur(bytes).map_err(|_| "文件解析失败——不是合法的 .vxcur 指针方案")?;
        if !rename.is_empty() {
            model.name = String::from(rename);
        }
        Ok(self.add(model))
    }

    /// 往返对拍：导出 → 导入 → 指纹一致（判据「导入导出往返一致」）。
    pub fn roundtrip_fingerprint(&self, name: &str) -> Option<u64> {
        let bytes = self.export_scheme(name)?;
        let mut fresh = SchemeLibrary::new(self.now_ms);
        match fresh.import_scheme(&bytes) {
            Ok(AddOutcome::Added(fp)) | Ok(AddOutcome::Duplicate(fp)) => {
                let _ = fp;
                Some(vxcur_fingerprint(&fresh.entries.first()?.model))
            }
            _ => None,
        }
    }
}

/// 墙面预览的轻量像素面（不借用 jbase PixBuf——渲染层只读消费）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixBufLike {
    pub size: u16,
    pub rows: Vec<Vec<[u8; 4]>>,
}

impl PixBufLike {
    pub fn get(&self, x: u16, y: u16) -> Option<[u8; 4]> {
        self.rows.get(y as usize)?.get(x as usize).copied()
    }

    /// 实体像素数（预览非空对账面）。
    pub fn solid_count(&self) -> usize {
        self.rows.iter().flatten().filter(|p| p[3] > 0).count()
    }
}

/// 缩略墙帧的轻量视图（渲染层取用，不借用整条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorFrameLite {
    pub w: u16,
    pub h: u16,
    pub hot_x: u16,
    pub hot_y: u16,
    pub delay_ms: u32,
}

impl From<&crate::jstar2::jbase::CursorFrame> for CursorFrameLite {
    fn from(f: &crate::jstar2::jbase::CursorFrame) -> CursorFrameLite {
        CursorFrameLite { w: f.w, h: f.h, hot_x: f.hot_x, hot_y: f.hot_y, delay_ms: f.delay_ms }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F628 自检。
pub fn run_library_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F628");

    // 1. 入库/容量：50 满库提醒 + 建议名单 + 拒绝不静默（分毫未动）。
    let mut lib = SchemeLibrary::new(1000);
    for i in 0..LIBRARY_CAP {
        let mut m = builtin_default_scheme();
        m.name = alloc::format!("方案{i:02}");
        assert!(matches!(lib.add(m), AddOutcome::Added(_)));
    }
    set.add("library fills to 50", lib.len() == LIBRARY_CAP, "");
    let mut extra = builtin_default_scheme();
    extra.name = String::from("第51个");
    match lib.add(extra) {
        AddOutcome::OverflowReminder { suggestion, pending } => {
            set.add(
                "overflow reminder with suggestion not silent",
                suggestion.len() == 3 && pending == "第51个" && lib.len() == LIBRARY_CAP,
                "",
            );
        }
        _ => set.add("overflow reminder with suggestion not silent", false, "wrong outcome"),
    }

    // 2. 用户拍板淘汰后强制通道：淘汰最旧、入新件。
    let oldest = lib.view(LibraryView::Recent).last().unwrap().name().to_string();
    let mut fresh = builtin_default_scheme();
    fresh.name = String::from("新来的");
    let r = lib.add_after_user_eviction(&oldest, fresh);
    set.add(
        "user-eviction gate admits after explicit choice",
        matches!(r, Ok(_)) && lib.len() == LIBRARY_CAP && lib.get(&oldest).is_none(),
        "",
    );

    // 3. 三视图：收藏过滤 + 最近排序。
    let mut lib2 = SchemeLibrary::new(0);
    for (i, name) in ["甲", "乙", "丙"].iter().enumerate() {
        let mut m = builtin_default_scheme();
        m.name = String::from(*name);
        let _ = lib2.add(m);
        if i == 1 {
            lib2.set_favorite(name, true);
        }
        lib2.set_now(1000 + i as u64 * 100);
        let _ = lib2.mark_active(name);
    }
    let favs = lib2.view(LibraryView::Favorites);
    let recent = lib2.view(LibraryView::Recent);
    set.add(
        "three views filter and order",
        favs.len() == 1 && favs[0].name() == "乙" && recent[0].name() == "丙",
        "",
    );

    // 4. 标签视图 + 全库标签清单/计数（面板数据面）。
    lib2.tag("甲", "手绘");
    lib2.tag("丙", "手绘");
    set.add("tag view filters", lib2.view_by_tag("手绘").len() == 2, "");
    lib2.tag("甲", "手绘"); // 重复打标幂等
    lib2.tag("乙", "系统");
    let all_tags = lib2.all_tags();
    let counts = lib2.tag_counts();
    set.add(
        "all tags deduped with counts",
        all_tags == alloc::vec![String::from("手绘"), String::from("系统")]
            && counts.len() == 2
            && counts[0] == (String::from("手绘"), 2)
            && counts[1] == (String::from("系统"), 1),
        "",
    );
    set.add("untag honest on missing", !lib2.untag("甲", "不存在"), "");

    // 5. E4 同源对账：投影与清单逐项一致 + 篡改/幽灵名都失配。
    let proj = lib2.e4_projection();
    set.add("e4 reconcile passes on projection", lib2.reconcile_e4(&proj), "");
    let mut tampered = proj.clone();
    tampered[0].1 = !tampered[0].1;
    set.add("e4 reconcile fails on tamper", !lib2.reconcile_e4(&tampered), "");
    let mut ghost = proj.clone();
    ghost.push((String::from("幽灵方案"), false));
    set.add("e4 reconcile rejects extra ghost name", !lib2.reconcile_e4(&ghost), "");

    // 6. 动态帧循环：3 帧 100ms 均分 → 时钟 0/100/200/350 落帧 0/1/2/0。
    use crate::jstar2::jbase::{builtin_glyph, CursorFrame};
    let mut lib3 = SchemeLibrary::new(0);
    let mut anim = builtin_default_scheme();
    anim.name = String::from("动画件");
    let mut frames: Vec<CursorFrame> = Vec::new();
    for i in 0..3u8 {
        let mut f = builtin_glyph(PointerState::Busy);
        f.delay_ms = 100;
        f.px[0] = i * 80; // 帧间可区分
        frames.push(f);
    }
    anim.set_state(PointerState::Busy, frames);
    let _ = lib3.add(anim);
    let seq = [
        lib3.thumb_frame_index_at("动画件", PointerState::Busy, 0),
        lib3.thumb_frame_index_at("动画件", PointerState::Busy, 100),
        lib3.thumb_frame_index_at("动画件", PointerState::Busy, 200),
        lib3.thumb_frame_index_at("动画件", PointerState::Busy, 350),
    ];
    set.add(
        "thumb cycle phases 0/1/2/0",
        seq == [Some(0), Some(1), Some(2), Some(0)],
        "",
    );

    // 7. 悬停播放/暂停：暂停冻结游标、恢复从暂停位续播。
    let _ = lib3.set_wall_playing("动画件", false);
    lib3.thumb_frame_index_at("动画件", PointerState::Busy, 0); // 冻结在 0
    let frozen = lib3.thumb_frame_index_at("动画件", PointerState::Busy, 250);
    let _ = lib3.set_wall_playing("动画件", true);
    let resumed = lib3.thumb_frame_index_at("动画件", PointerState::Busy, 250);
    let missing_refused = !lib3.set_wall_playing("不存在", true);
    set.add(
        "wall pause freezes and resume continues",
        frozen == Some(0) && resumed == Some(2) && missing_refused,
        "",
    );

    // 8. 墙面像素预览：真图降采样（16×16、实体均值），占位块不上墙。
    let prev = lib3.thumb_preview("动画件", PointerState::Busy, 0, WALL_THUMB_PX).unwrap();
    let empty_prev = SchemeLibrary::new(0).thumb_preview("没有", PointerState::Normal, 0, 16);
    set.add(
        "wall preview renders real pixels",
        prev.size == 16 && prev.solid_count() > 0 && empty_prev.is_none(),
        "",
    );

    // 9. 导入导出往返指纹一致。
    match lib2.roundtrip_fingerprint("甲") {
        Some(fp) => set.add(
            "export/import roundtrip fingerprint",
            fp == lib2.get("甲").unwrap().fingerprint(),
            "",
        ),
        None => set.add("export/import roundtrip fingerprint", false, "roundtrip failed"),
    }

    // 10. 体检报告挂载 + 指纹失配拒绝 + 全库过期扫描。
    use crate::jstar2::checker::inspect;
    let rep = inspect(&lib2.get("甲").unwrap().model.clone());
    set.add("report attaches on matching fingerprint", lib2.record_report("甲", rep).is_ok(), "");
    let mut lib2b = lib2.clone();
    let stale = {
        let mut m = lib2.get("甲").unwrap().model.clone();
        m.author = String::from("改版者");
        inspect(&m)
    };
    set.add(
        "stale report rejected by fingerprint",
        lib2b.record_report("甲", stale).is_err(),
        "",
    );
    // 扫描面：乙/丙未体检 → 进清单；甲已挂报告 → 不进。
    let sweep = lib2.health_sweep();
    set.add(
        "health sweep lists unchecked entries",
        sweep.contains(&String::from("乙")) && sweep.contains(&String::from("丙")) && !sweep.contains(&String::from("甲")),
        "",
    );

    // 11. 库内搜索 + 组合过滤 + 视图计数。
    lib2.set_favorite("丙", true);
    let hits = lib2.search("乙");
    let combo = lib2.view_favorites_tagged("手绘");
    let (all_n, fav_n, recent_n) = lib2.view_counts();
    set.add(
        "search combined filter and view counts",
        hits.len() == 1 && hits[0].name() == "乙"
            && combo.len() == 1 && combo[0].name() == "丙"
            && (all_n, fav_n, recent_n) == (3, 2, 3),
        "",
    );
    set.add("search empty query returns all", lib2.search("").len() == 3, "");

    // 12. 删除在用方案 → active 如实清空（E4 前柜不留幽灵）。
    let mut lib4 = SchemeLibrary::new(0);
    let _ = lib4.add(named_default("现任"));
    let _ = lib4.add(named_default("路人"));
    let _ = lib4.mark_active("现任");
    set.add(
        "removing active clears honest state",
        lib4.remove("现任") && lib4.active_name.is_empty() && lib4.get("现任").is_none(),
        "",
    );
    // 删非在用方案不影响 active。
    let _ = lib4.mark_active("路人");
    let _ = lib4.remove("不存在");
    set.add(
        "removing inactive keeps active",
        lib4.active_name == "路人" && !lib4.remove("不存在"),
        "",
    );

    // 13. 导入改名：同内容不同名 = 两条目（名字在指纹前改）。
    let bytes = lib4.export_scheme("路人").unwrap();
    let mut lib5 = SchemeLibrary::new(0);
    let _ = lib5.import_scheme(&bytes);
    let renamed = lib5.import_scheme_as(&bytes, "别名件");
    set.add(
        "import with rename creates distinct entry",
        matches!(renamed, Ok(AddOutcome::Added(_)))
            && lib5.len() == 2
            && lib5.get("别名件").is_some(),
        "",
    );

    set
}

fn named_default(name: &str) -> CursorSchemeModel {
    let mut m = crate::jstar2::jbase::builtin_default_scheme();
    m.name = String::from(name);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::builtin_default_scheme;

    fn named(name: &str) -> CursorSchemeModel {
        let mut m = builtin_default_scheme();
        m.name = String::from(name);
        m
    }

    #[test]
    fn overflow_reminder_never_silent_drops() {
        let mut lib = SchemeLibrary::new(0);
        for i in 0..LIBRARY_CAP {
            assert!(matches!(lib.add(named(&alloc::format!("s{i:02}"))), AddOutcome::Added(_)));
        }
        match lib.add(named("溢出件")) {
            AddOutcome::OverflowReminder { suggestion, .. } => {
                assert_eq!(suggestion.len(), 3);
                assert_eq!(lib.len(), LIBRARY_CAP, "库房分毫未动——不静默删");
            }
            _ => panic!("should remind"),
        }
    }

    #[test]
    fn e4_projection_is_library_truth() {
        let mut lib = SchemeLibrary::new(0);
        let _ = lib.add(named("甲"));
        let _ = lib.add(named("乙"));
        let _ = lib.mark_active("乙");
        let proj = lib.e4_projection();
        // 投影按名排序（UTF-8 字节序：乙 U+4E59 < 甲 U+7532）。
        assert_eq!(proj, alloc::vec![(String::from("乙"), true), (String::from("甲"), false)]);
        assert!(lib.reconcile_e4(&proj));
        assert!(!lib.reconcile_e4(&[(String::from("甲"), true), (String::from("乙"), false)]));
    }

    #[test]
    fn recent_view_orders_by_last_used() {
        let mut lib = SchemeLibrary::new(0);
        for (i, n) in ["甲", "乙", "丙"].iter().enumerate() {
            let _ = lib.add(named(n));
            lib.set_now(100 + i as u64);
            let _ = lib.mark_active(n);
        }
        let names: Vec<&str> = lib.view(LibraryView::Recent).iter().map(|e| e.name()).collect();
        assert_eq!(names, alloc::vec!["丙", "乙", "甲"]);
    }

    #[test]
    fn thumb_cycle_advances_and_wraps() {
        use crate::jstar2::jbase::{builtin_glyph, CursorFrame};
        let mut lib = SchemeLibrary::new(0);
        let mut m = named("动画");
        let mut frames: Vec<CursorFrame> = Vec::new();
        for _ in 0..3 {
            let mut f = builtin_glyph(PointerState::Busy);
            f.delay_ms = 100;
            frames.push(f);
        }
        m.set_state(PointerState::Busy, frames);
        let _ = lib.add(m);
        assert_eq!(lib.thumb_frame_index_at("动画", PointerState::Busy, 0), Some(0));
        assert_eq!(lib.thumb_frame_index_at("动画", PointerState::Busy, 100), Some(1));
        assert_eq!(lib.thumb_frame_index_at("动画", PointerState::Busy, 250), Some(2));
        assert_eq!(lib.thumb_frame_index_at("动画", PointerState::Busy, 300), Some(0));
    }

    #[test]
    fn roundtrip_fingerprint_stable() {
        let mut lib = SchemeLibrary::new(0);
        let _ = lib.add(named("件"));
        let fp = lib.get("件").unwrap().fingerprint();
        assert_eq!(lib.roundtrip_fingerprint("件"), Some(fp));
    }

    #[test]
    fn stale_report_rejected_by_fingerprint() {
        let mut lib = SchemeLibrary::new(0);
        let _ = lib.add(named("件"));
        let stale = {
            let mut m = lib.get("件").unwrap().model.clone();
            m.author = String::from("改版者");
            crate::jstar2::checker::inspect(&m)
        };
        assert!(lib.record_report("件", stale).is_err());
    }

    #[test]
    fn user_eviction_gate_honest_errors() {
        let mut lib = SchemeLibrary::new(0);
        let _ = lib.add(named("件"));
        assert!(lib.add_after_user_eviction("不存在", named("新")).is_err());
    }

    #[test]
    fn wall_pause_freeze_semantics() {
        use crate::jstar2::jbase::{builtin_glyph, CursorFrame};
        let mut lib = SchemeLibrary::new(0);
        let mut m = named("件");
        let mut frames: Vec<CursorFrame> = Vec::new();
        for _ in 0..2 {
            let mut f = builtin_glyph(PointerState::Busy);
            f.delay_ms = 100;
            frames.push(f);
        }
        m.set_state(PointerState::Busy, frames);
        let _ = lib.add(m);
        lib.thumb_frame_index_at("件", PointerState::Busy, 0);
        lib.set_wall_playing("件", false);
        // 暂停期间任意时钟都冻结在同一帧。
        assert_eq!(lib.thumb_frame_index_at("件", PointerState::Busy, 90), Some(0));
        assert_eq!(lib.thumb_frame_index_at("件", PointerState::Busy, 190), Some(0));
        lib.set_wall_playing("件", true);
        assert_eq!(lib.thumb_frame_index_at("件", PointerState::Busy, 190), Some(1));
    }

    #[test]
    fn tag_counts_and_search_agree() {
        let mut lib = SchemeLibrary::new(0);
        for (n, t) in [("甲", "红"), ("乙", "红"), ("丙", "蓝")] {
            let _ = lib.add(named(n));
            lib.tag(n, t);
        }
        assert_eq!(lib.tag_counts(), alloc::vec![(String::from("红"), 2), (String::from("蓝"), 1)]);
        assert_eq!(lib.all_tags().len(), 2);
        assert_eq!(lib.search("乙").len(), 1);
        assert!(lib.get("乙").unwrap().tags.first().unwrap() == "红");
    }

    #[test]
    fn touch_updates_recent_without_switch() {
        let mut lib = SchemeLibrary::new(0);
        let _ = lib.add(named("甲"));
        let _ = lib.add(named("乙"));
        lib.set_now(10);
        let _ = lib.mark_active("甲");
        lib.set_now(20);
        assert!(lib.touch("乙"));
        let names: Vec<&str> = lib.view(LibraryView::Recent).iter().map(|e| e.name()).collect();
        assert_eq!(names[0], "乙", "touch 刷新最近使用");
        assert_eq!(lib.active_name, "甲", "touch 不切换在用");
        assert!(!lib.touch("没有"));
    }

    #[test]
    fn preview_empty_blocks_are_transparent() {
        let mut lib = SchemeLibrary::new(0);
        let _ = lib.add(named("实心"));
        let prev = lib.thumb_preview("实心", PointerState::Normal, 0, 8).unwrap();
        assert_eq!(prev.size, 8);
        assert!(prev.solid_count() > 0, "内置方案首帧有实体");
        assert_eq!(prev.get(0, 0), Some([0, 0, 0, 0]), "空白角块透明");
    }
}
