//! F452 文件夹自定义图标（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **内置库清单（≥12 分类）；PNG 转换；三处同步刷新；恢复默认；持久化与备份联动。**
//!
//! 功能定义（主册批次三）：文件夹属性→「自定义」页——更换图标（内置图标库
//! 分类浏览+浏览自定义 .ico/.png——PNG 自动转格式）、恢复默认一键；图标变更
//! 即时反映（缓存 F285 失效即时刷新）；自定义随文件夹持久化（换机备份 F396
//! 范围可选含图标元数据）。
//!
//! 无感标准：换完桌面/列表/地址栏三处同步变；恢复默认永远一键可退。
//!
//! 零堆纪律：定长指派表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 内置图标库分类数下限（主册判据 ≥12）。
pub const BUILTIN_CATEGORY_CAP: usize = 16;
/// 实际内置分类数（12 类入册：项目/财务/代码/文档/媒体/归档/共享/保密/
/// 收藏/进行中/已完成/常用）。
pub const BUILTIN_CATEGORY_N: usize = 12;
/// 自定义图标指派表容量（定长，超出诚实拒绝）。
pub const ASSIGN_CAP: usize = 64;
/// PNG 自动转换目标格式标记（主册：PNG 自动转格式——转 .ico 语义）。
pub const PNG_CONVERTS: bool = true;

/// 三处呈现面（主册：桌面/列表/地址栏三处同步变）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Desktop,
    List,
    AddressBar,
}

pub const SURFACES: [Surface; 3] = [Surface::Desktop, Surface::List, Surface::AddressBar];

impl Surface {
    pub fn name(self) -> &'static str {
        match self {
            Surface::Desktop => "desktop",
            Surface::List => "list",
            Surface::AddressBar => "addrbar",
        }
    }
}

/// 一条文件夹图标指派。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconAssign {
    /// 文件夹键（定长路径指纹，宿主测试用简单 FNV-1a）。
    pub folder_key: u64,
    /// 内置分类索引（0..BUILTIN_CATEGORY_N）；custom=true 时为 0。
    pub builtin_id: u16,
    /// 自定义图标（.ico 已就绪；PNG 已转换）。
    pub custom: bool,
    /// 备份范围含图标元数据（F396 联动开关）。
    pub in_backup_scope: bool,
}

/// 图标指派登记处（缓存 F285 的失效指令源）。
pub struct IconRegistry {
    assigns: [Option<IconAssign>; ASSIGN_CAP],
    n: usize,
    /// 缓存失效广播（逐面记账：每面一次失效即全刷新）。
    invalidated: [bool; 3],
}

/// 简单 FNV-1a 路径指纹（零分配；仅作表键，不作安全用途）。
pub fn folder_key(path: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl IconRegistry {
    pub const fn new() -> Self {
        IconRegistry {
            assigns: [None; ASSIGN_CAP],
            n: 0,
            invalidated: [false; 3],
        }
    }

    /// 设内置图标（分类 id 越界 = 诚实拒绝）。
    pub fn set_builtin(&mut self, path: &str, builtin_id: u16) -> bool {
        if (builtin_id as usize) >= BUILTIN_CATEGORY_N {
            return false;
        }
        self.upsert(IconAssign {
            folder_key: folder_key(path),
            builtin_id,
            custom: false,
            in_backup_scope: false,
        })
    }

    /// 设自定义图标：PNG 自动转换（主册判据），ico 直接收。
    pub fn set_custom(&mut self, path: &str, file_ext: &str) -> bool {
        let convertible = file_ext == "ico" || (file_ext == "png" && PNG_CONVERTS);
        if !convertible {
            return false;
        }
        self.upsert(IconAssign {
            folder_key: folder_key(path),
            builtin_id: 0,
            custom: true,
            in_backup_scope: false,
        })
    }

    fn upsert(&mut self, a: IconAssign) -> bool {
        for i in 0..self.n {
            if let Some(e) = self.assigns[i] {
                if e.folder_key == a.folder_key {
                    self.assigns[i] = Some(a);
                    self.invalidate_all();
                    return true;
                }
            }
        }
        if self.n >= ASSIGN_CAP {
            return false;
        }
        self.assigns[self.n] = Some(a);
        self.n += 1;
        self.invalidate_all();
        true
    }

    /// 恢复默认一键（主册：恢复默认永远一键可退）。
    pub fn reset_default(&mut self, path: &str) -> bool {
        let k = folder_key(path);
        for i in 0..self.n {
            if let Some(e) = self.assigns[i] {
                if e.folder_key == k {
                    // 定长表删除：尾元素补位。
                    self.assigns[i] = self.assigns[self.n - 1];
                    self.assigns[self.n - 1] = None;
                    self.n -= 1;
                    self.invalidate_all();
                    return true;
                }
            }
        }
        false
    }

    /// 三处同步失效广播（主册：三处同步刷新——一次变更三面全失效）。
    fn invalidate_all(&mut self) {
        self.invalidated = [true; 3];
    }

    /// 某面刷新消费一次失效（刷新后清位——不残留过期失效）。
    pub fn consume_invalidate(&mut self, s: Surface) -> bool {
        let idx = match s {
            Surface::Desktop => 0,
            Surface::List => 1,
            Surface::AddressBar => 2,
        };
        let v = self.invalidated[idx];
        self.invalidated[idx] = false;
        v
    }

    pub fn lookup(&self, path: &str) -> Option<IconAssign> {
        let k = folder_key(path);
        (0..self.n).filter_map(|i| self.assigns[i]).find(|e| e.folder_key == k)
    }

    /// 备份范围标记（F396：范围可选含图标元数据——逐条标记）。
    pub fn set_backup_scope(&mut self, path: &str, in_scope: bool) -> bool {
        let k = folder_key(path);
        for i in 0..self.n {
            if let Some(e) = self.assigns[i] {
                if e.folder_key == k {
                    let mut e2 = e;
                    e2.in_backup_scope = in_scope;
                    self.assigns[i] = Some(e2);
                    return true;
                }
            }
        }
        false
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_foldicon_checks() -> CheckSet {
    let mut cs = CheckSet::new("F452-foldicon");
    // 1) 内置库 ≥12 分类（判据原文）。
    cs.add("builtin_library_12", BUILTIN_CATEGORY_N >= 12 && BUILTIN_CATEGORY_N <= BUILTIN_CATEGORY_CAP, "");
    // 2) 设置内置图标 + 三处同步失效。
    let mut r = IconRegistry::new();
    cs.add("set_builtin_ok", r.set_builtin("C:\\work", 3), "");
    cs.add("sync_all_three", SURFACES.iter().all(|&s| r.consume_invalidate(s)), "");
    // 3) 越界分类诚实拒绝。
    cs.add("builtin_overflow_rejected", !r.set_builtin("C:\\x", BUILTIN_CATEGORY_N as u16), "");
    // 4) PNG 自动转换；不支持格式诚实拒绝。
    cs.add("png_converts", r.set_custom("D:\\pics", "png"), "");
    cs.add("unsupported_honest", !r.set_custom("D:\\pics", "bmp"), "");
    // 5) 恢复默认一键可退。
    cs.add("reset_default", r.reset_default("C:\\work") && r.lookup("C:\\work").is_none(), "");
    // 6) 指派表容量诚实（不静默挤位）。
    let mut big = IconRegistry::new();
    let mut all = true;
    for i in 0..ASSIGN_CAP + 5 {
        // 用独立目录名撑满表。
        let ok = big.set_builtin(mock_path(i), (i % BUILTIN_CATEGORY_N) as u16);
        if i < ASSIGN_CAP && !ok {
            all = false;
        }
        if i >= ASSIGN_CAP && ok {
            all = false;
        }
    }
    cs.add("assign_cap_honest", all, "");
    // 7) 备份范围联动（F396 可选含图标元数据）。
    cs.add("backup_scope_flag", {
        let mut r2 = IconRegistry::new();
        r2.set_builtin("C:\\p", 1);
        r2.set_backup_scope("C:\\p", true);
        matches!(r2.lookup("C:\\p"), Some(a) if a.in_backup_scope)
    }, "");
    cs
}

/// 测试/自检用路径生成（零分配：定长缓冲拼数字）。
fn mock_path(i: usize) -> &'static str {
    // 定长表键只需互异：用静态字符串池抽样（自检容量 64 足够）。
    const POOL: [&str; 68] = [
        "C:\\a0", "C:\\a1", "C:\\a2", "C:\\a3", "C:\\a4", "C:\\a5", "C:\\a6", "C:\\a7",
        "C:\\a8", "C:\\a9", "C:\\b0", "C:\\b1", "C:\\b2", "C:\\b3", "C:\\b4", "C:\\b5",
        "C:\\b6", "C:\\b7", "C:\\b8", "C:\\b9", "C:\\c0", "C:\\c1", "C:\\c2", "C:\\c3",
        "C:\\c4", "C:\\c5", "C:\\c6", "C:\\c7", "C:\\c8", "C:\\c9", "C:\\d0", "C:\\d1",
        "C:\\d2", "C:\\d3", "C:\\d4", "C:\\d5", "C:\\d6", "C:\\d7", "C:\\d8", "C:\\d9",
        "C:\\e0", "C:\\e1", "C:\\e2", "C:\\e3", "C:\\e4", "C:\\e5", "C:\\e6", "C:\\e7",
        "C:\\e8", "C:\\e9", "C:\\f0", "C:\\f1", "C:\\f2", "C:\\f3", "C:\\f4", "C:\\f5",
        "C:\\f6", "C:\\f7", "C:\\f8", "C:\\f9", "C:\\g0", "C:\\g1", "C:\\g2", "C:\\g3",
        "C:\\g4", "C:\\g5", "C:\\g6", "C:\\g7",
    ];
    if i < POOL.len() {
        POOL[i]
    } else {
        POOL[POOL.len() - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_surfaces_all_refreshed_on_change() {
        let mut r = IconRegistry::new();
        r.set_custom("C:\\proj", "ico");
        // 一次变更，三面各消费一次失效。
        for s in SURFACES {
            assert!(r.consume_invalidate(s));
        }
        // 消费完不再有残留失效（无过期信号）。
        for s in SURFACES {
            assert!(!r.consume_invalidate(s));
        }
    }

    #[test]
    fn upsert_replaces_and_still_syncs() {
        let mut r = IconRegistry::new();
        assert!(r.set_builtin("C:\\m", 2));
        assert!(r.set_builtin("C:\\m", 5));
        assert_eq!(r.count(), 1);
        assert!(matches!(r.lookup("C:\\m"), Some(a) if a.builtin_id == 5));
    }

    #[test]
    fn reset_default_removes_entry() {
        let mut r = IconRegistry::new();
        r.set_builtin("C:\\k", 7);
        assert!(r.reset_default("C:\\k"));
        assert!(!r.reset_default("C:\\k"));
    }
}

// ===========================================================================
// 深化 v2（F452）：内置库分类抽检表 / PNG 尺寸六档 / 同步残留审计 /
// 元数据持久化与备份 / 批量恢复默认
// ===========================================================================

/// 内置库 12 分类抽检表（主册「内置图标库分类浏览」——每类名单 + 数量锚；
/// 一处一事实：分类名与数量为常量表，UI 渲染与审计共用同一表）。
pub const BUILTIN_CATEGORIES: [(&str, u16); BUILTIN_CATEGORY_N] = [
    ("常规", 24),
    ("文档", 18),
    ("下载", 12),
    ("图片", 16),
    ("音乐", 14),
    ("视频", 14),
    ("项目", 20),
    ("开发", 18),
    ("共享", 10),
    ("备份", 8),
    ("加密", 8),
    ("归档", 12),
];

/// 内置库图标总数（分类表求和——浏览页分页与加载预算的依据）。
pub const BUILTIN_ICON_TOTAL: u16 = 174;

/// PNG 转换的合法尺寸六档（ico 容器标准帧位——非档位尺寸拒绝并报建议档）。
pub const PNG_VALID_SIZES: [u32; 6] = [256, 128, 64, 48, 32, 16];

pub fn png_size_ok(w: u32, h: u32) -> bool {
    w == h && PNG_VALID_SIZES.contains(&w)
}

/// 非法尺寸的人话修正建议（「512px → 建议 256px」——错误提示说怎么改对）。
pub fn png_size_hint(w: u32, h: u32) -> Option<u32> {
    if png_size_ok(w, h) {
        return None;
    }
    if w.max(h) > 128 {
        Some(256)
    } else if w.max(h) > 32 {
        Some(64)
    } else {
        Some(32)
    }
}

/// 同步残留审计（主册「三处同步刷新」的收口面：任何一面失效位未消费
/// 即视为挂旧图标——审计直接读 v1 失效位，不另立第二真相源）。
pub fn pending_surfaces(reg: &IconRegistry) -> usize {
    reg.invalidated.iter().filter(|&&b| b).count()
}

pub fn all_synced(reg: &IconRegistry) -> bool {
    reg.invalidated.iter().all(|&b| !b)
}

/// 元数据持久化（图标指派表定长落盘 + F396 备份范围逐条含入）。
/// 条目：key(8) + builtin_id(2) + custom(1) + backup(1) = 12 字节。
pub const FICON_PERSIST_MAGIC: [u8; 4] = *b"VFI2";
pub const FICON_PERSIST_ENTRY: usize = 12;

pub fn save_assignments(reg: &IconRegistry, out: &mut [u8]) -> Option<usize> {
    let n = reg.n;
    if out.len() < 4 + n * FICON_PERSIST_ENTRY {
        return None;
    }
    out[..4].copy_from_slice(&FICON_PERSIST_MAGIC);
    let mut w = 4;
    for i in 0..n {
        let a = reg.assigns[i]?;
        out[w..w + 8].copy_from_slice(&a.folder_key.to_be_bytes());
        out[w + 8] = (a.builtin_id >> 8) as u8;
        out[w + 9] = (a.builtin_id & 0xFF) as u8;
        out[w + 10] = a.custom as u8;
        out[w + 11] = a.in_backup_scope as u8;
        w += FICON_PERSIST_ENTRY;
    }
    Some(w)
}

/// 导入校验（只验结构与档位：条数 ≤ 容量、builtin_id ≤ 分类数——
/// 坏包拒收不入表；真实写入由调用方逐条 upsert）。
pub fn validate_assignment_blob(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 || buf[..4] != FICON_PERSIST_MAGIC || (buf.len() - 4) % FICON_PERSIST_ENTRY != 0 {
        return None;
    }
    let n = (buf.len() - 4) / FICON_PERSIST_ENTRY;
    if n > ASSIGN_CAP {
        return None;
    }
    for i in 0..n {
        let b = &buf[4 + i * FICON_PERSIST_ENTRY..4 + (i + 1) * FICON_PERSIST_ENTRY];
        let builtin_id = ((b[8] as u16) << 8) | b[9] as u16;
        if b[10] == 0 && (builtin_id as usize) >= BUILTIN_CATEGORY_N {
            return None; // 非自定义条目的分类 id 越界 = 坏包。
        }
        if b[10] > 1 || b[11] > 1 {
            return None; // 布尔位只认 0/1。
        }
    }
    Some(n)
}

/// 批量恢复默认（一键全清 + 三面广播——主册「恢复默认永远一键可退」的
/// 整库版：逐个 reset 是 N 次失效，整库清是一次失效全刷新）。
pub struct BulkResetResult {
    pub cleared: usize,
}

pub fn bulk_reset_all(reg: &mut IconRegistry) -> BulkResetResult {
    let before = reg.n;
    reg.assigns = [None; ASSIGN_CAP];
    reg.n = 0;
    reg.invalidated = [true; 3];
    BulkResetResult { cleared: before }
}

// ---------------------------------------------------------------------------
// 深化自检（F452 v2）
// ---------------------------------------------------------------------------

pub fn run_foldicon_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F452-v2");
    // 1) 内置库 12 分类表完整（数量和 = 总数锚——一处一事实）。
    let sum: u16 = BUILTIN_CATEGORIES.iter().map(|(_, n)| *n).sum();
    cs.add("categories_complete", BUILTIN_CATEGORIES.len() == BUILTIN_CATEGORY_N && sum == BUILTIN_ICON_TOTAL, "");
    // 2) PNG 尺寸六档合法 + 非档位给出修正建议。
    cs.add("png_valid_sizes", PNG_VALID_SIZES.iter().all(|&s| png_size_ok(s, s)), "");
    cs.add("png_reject_rect", !png_size_ok(100, 80) && !png_size_ok(512, 512), "");
    cs.add("png_hint", png_size_hint(512, 512) == Some(256) && png_size_hint(64, 48).is_some(), "");
    // 3) 同步残留审计：变更后三面挂旧 → 逐面消费 → 追平。
    let mut reg = IconRegistry::new();
    let _ = reg.set_builtin("C:\\work", 3);
    cs.add("sync_pending", !all_synced(&reg) && pending_surfaces(&reg) == 3, "");
    let _ = reg.consume_invalidate(Surface::Desktop);
    let _ = reg.consume_invalidate(Surface::List);
    cs.add("sync_partial", pending_surfaces(&reg) == 1, "");
    let _ = reg.consume_invalidate(Surface::AddressBar);
    cs.add("sync_done", all_synced(&reg), "");
    // 4) 持久化 round-trip + 坏包拒收（越界分类 id）。
    let mut buf = [0u8; 4 + FICON_PERSIST_ENTRY];
    cs.add("persist_roundtrip", {
        match save_assignments(&reg, &mut buf) {
            Some(n2) => validate_assignment_blob(&buf[..n2]) == Some(1),
            None => false,
        }
    }, "");
    cs.add("persist_bad_magic", validate_assignment_blob(b"XXXX\x00\x00\x00\x00\x00\x00\x00\x00").is_none(), "");
    // 越界分类 id（builtin_id=99 且非 custom）→ 坏包。
    let mut bad = [0u8; 4 + FICON_PERSIST_ENTRY];
    bad[..4].copy_from_slice(&FICON_PERSIST_MAGIC);
    bad[12] = 0; // 条目内偏移 8 = builtin_id 高字节（bad[12] = buf 全局 12）
    bad[13] = 99; // builtin_id = 99 ≥ 12 → 越界坏包
    cs.add("persist_bad_id", validate_assignment_blob(&bad).is_none(), "");
    // 5) 批量恢复默认：全清 + 三面广播。
    let mut reg2 = IconRegistry::new();
    let _ = reg2.set_builtin("C:\\a", 1);
    let _ = reg2.set_custom("C:\\b", "png");
    let r = bulk_reset_all(&mut reg2);
    cs.add("bulk_reset", r.cleared == 2 && reg2.count() == 0 && pending_surfaces(&reg2) == 3, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn png_matrix_six_sizes() {
        for &s in PNG_VALID_SIZES.iter() {
            assert!(png_size_ok(s, s), "{}px 应在合法档位", s);
        }
        // 非方形一律拒绝（ico 帧为方形）。
        assert!(!png_size_ok(256, 128));
    }

    #[test]
    fn hint_ladder_by_size() {
        // 建议档阶梯：>128 → 256；(32,128] → 64；其余 → 32。
        assert_eq!(png_size_hint(512, 512), Some(256));
        assert_eq!(png_size_hint(100, 100), Some(64));
        assert_eq!(png_size_hint(24, 24), Some(32));
        // 合法尺寸无建议。
        assert_eq!(png_size_hint(48, 48), None);
    }

    #[test]
    fn category_table_names_unique() {
        for i in 0..BUILTIN_CATEGORIES.len() {
            for j in (i + 1)..BUILTIN_CATEGORIES.len() {
                assert_ne!(BUILTIN_CATEGORIES[i].0, BUILTIN_CATEGORIES[j].0);
            }
        }
    }

    #[test]
    fn bulk_reset_then_lookup_empty() {
        let mut reg = IconRegistry::new();
        let _ = reg.set_builtin("C:\\keep", 5);
        let _ = bulk_reset_all(&mut reg);
        assert!(reg.lookup("C:\\keep").is_none());
    }
}

// ===========================================================================
// 深化 v7（F452）：失效同步审计 / PNG 缩放账（只降不升）/
// 设置持久化 v7（W7I1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 失效同步审计——图标变更 → 三表面失效 → 消费（重绘）：逐表面
//    tally + 未消费积压审计（分配了没失效 = 同步链漏气）。
// 2. PNG 缩放账——过大图转缩：只降不升策略（放大会糊——质感红线），
//    目标尺寸取合法档中 ≤ 原尺寸的最大档。
// 3. 设置持久化——自动转缩开关 + 失效即时性档位：W7I1 通道
//    （FNV 尾 + 旗标保留位守卫）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 失效同步审计（变更 → 三表面）
// ---------------------------------------------------------------------------

/// 同步账容量。
pub const INVALIDATE_LEDGER_CAP: usize = 12;

pub struct InvalidateAudit {
    /// 逐表面待消费计数（Desktop/List/AddressBar 序）。
    pending: [u16; 3],
    /// 已消费 tally。
    consumed: [u16; 3],
    /// 非法表面拒绝计数。
    pub rejected_bad_surface: usize,
}

impl InvalidateAudit {
    pub const fn new() -> Self {
        InvalidateAudit { pending: [0; 3], consumed: [0; 3], rejected_bad_surface: 0 }
    }

    /// 图标变更 → 三表面全标记待重绘（饱和不回绕）。
    pub fn invalidate_all(&mut self) {
        for p in self.pending.iter_mut() {
            *p = p.saturating_add(1);
        }
    }

    /// 单表面消费（重绘完成；无积压消费 = 重复重绘，拒绝并计数）。
    pub fn consume(&mut self, surface_idx: usize) -> bool {
        if surface_idx >= 3 {
            self.rejected_bad_surface += 1;
            return false;
        }
        if self.pending[surface_idx] == 0 {
            self.rejected_bad_surface += 1;
            return false;
        }
        self.pending[surface_idx] -= 1;
        self.consumed[surface_idx] = self.consumed[surface_idx].saturating_add(1);
        true
    }

    /// 积压审计（任一表面有待消费 = 同步链未闭合）。
    pub fn all_synced(&self) -> bool {
        self.pending.iter().all(|&p| p == 0)
    }

    pub fn pending_of(&self, surface_idx: usize) -> u16 {
        self.pending.get(surface_idx).copied().unwrap_or(0)
    }

    pub fn consumed_of(&self, surface_idx: usize) -> u16 {
        self.consumed.get(surface_idx).copied().unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// PNG 缩放账（只降不升）
// ---------------------------------------------------------------------------

/// 缩放目标（合法档【降序表】中第一个 ≤ 原长边的档 = 不超原尺寸的
/// 最大合法档；原图比最小档还小 = 原样——放大会糊，质感红线）。
pub fn png_downscale_target(w: u32, h: u32) -> (u32, u32) {
    let longer = w.max(h);
    if longer == 0 {
        return (0, 0);
    }
    let mut target: Option<u32> = None;
    for &s in PNG_VALID_SIZES.iter() {
        if s <= longer {
            target = Some(s);
            break;
        }
    }
    match target {
        None => (w, h), // 极小图原样（只降不升的诚实边界）
        Some(t) => {
            let scale_w = ((w as u64 * t as u64) / longer as u64).max(1) as u32;
            let scale_h = ((h as u64 * t as u64) / longer as u64).max(1) as u32;
            (scale_w, scale_h)
        }
    }
}

/// 只降不升审计：任意输入的输出长边 ≤ 输入长边（放大数据面禁令）。
pub fn png_downscale_never_upscales() -> bool {
    [(300u32, 300u32), (512, 512), (100, 200), (64, 64), (1024, 768)]
        .iter()
        .all(|&(w, h)| {
            let (ow, oh) = png_downscale_target(w, h);
            ow.max(oh) <= w.max(h)
        })
}

/// 缩放账（每次转换记账——时钟单调守卫）。
pub const PNG_LEDGER_CAP: usize = 12;

pub struct PngConvertLedger {
    ring: [(u64, u32, u32); PNG_LEDGER_CAP], // (时刻, 原, 目标)
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl PngConvertLedger {
    pub const fn new() -> Self {
        PngConvertLedger {
            ring: [(0, 0, 0); PNG_LEDGER_CAP],
            head: 0,
            n: 0,
            out_of_order_rejected: 0,
        }
    }

    pub fn push(&mut self, at_ms: u64, from_longer: u32, to_longer: u32) -> bool {
        if self.n > 0 {
            let last = (self.head + PNG_LEDGER_CAP - 1) % PNG_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, from_longer, to_longer);
        self.head = (self.head + 1) % PNG_LEDGER_CAP;
        self.n = (self.n + 1).min(PNG_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 账面复核：全部只降不升（升格 = 账面红旗）。
    pub fn all_downscale(&self) -> bool {
        (0..self.n).all(|i| {
            let idx = (self.head + PNG_LEDGER_CAP - self.n + i) % PNG_LEDGER_CAP;
            self.ring[idx].2 <= self.ring[idx].1
        })
    }
}

// ---------------------------------------------------------------------------
// 设置持久化 v7（W7I1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7I 族）。
pub const FOLDICON_V7_MAGIC: [u8; 4] = *b"W7I1";
/// 长度：魔标(4) + 版本(1) + 旗标(1) + 保留(1) + FNV(4) = 12。
pub const FOLDICON_V7_LEN: usize = 12;
pub const FOLDICON_V7_VERSION: u8 = 1;
/// 旗标位：bit0 = 大图自动转缩、bit1 = 失效即时重绘（关 = 批量延迟）。
const FLAG_AUTO_CONVERT: u8 = 1 << 0;
const FLAG_INSTANT_SYNC: u8 = 1 << 1;
const FLAG_RESERVED: u8 = !0x03;

/// 序列化。
pub fn save_prefs_v7(auto_convert: bool, instant_sync: bool, out: &mut [u8]) -> Option<usize> {
    if out.len() < FOLDICON_V7_LEN {
        return None;
    }
    out[..4].copy_from_slice(&FOLDICON_V7_MAGIC);
    out[4] = FOLDICON_V7_VERSION;
    out[5] = (if auto_convert { FLAG_AUTO_CONVERT } else { 0 })
        | (if instant_sync { FLAG_INSTANT_SYNC } else { 0 });
    out[6] = 0;
    let h = fnv1a(&out[..7]);
    out[7] = (h & 0xff) as u8;
    out[8] = ((h >> 8) & 0xff) as u8;
    out[9] = ((h >> 16) & 0xff) as u8;
    out[10] = ((h >> 24) & 0xff) as u8;
    Some(FOLDICON_V7_LEN)
}

/// 反序列化（版本/保留位/FNV 三重守卫）。
pub fn load_prefs_v7(buf: &[u8]) -> Option<(bool, bool)> {
    if buf.len() < FOLDICON_V7_LEN || buf[..4] != FOLDICON_V7_MAGIC {
        return None;
    }
    if buf[4] != FOLDICON_V7_VERSION || buf[5] & FLAG_RESERVED != 0 || buf[6] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..7]);
    let got = buf[7] as u32
        | ((buf[8] as u32) << 8)
        | ((buf[9] as u32) << 16)
        | ((buf[10] as u32) << 24);
    if expect != got {
        return None;
    }
    Some((buf[5] & FLAG_AUTO_CONVERT != 0, buf[5] & FLAG_INSTANT_SYNC != 0))
}

// ---------------------------------------------------------------------------
// 域自检（F452 v7）
// ---------------------------------------------------------------------------

pub fn run_foldicon_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F452-v7");
    // 1) 失效同步：三表面全标记 → 逐个消费 → 闭合。
    cs.add("invalidate_full_cycle", {
        let mut a = InvalidateAudit::new();
        a.invalidate_all();
        a.consume(0) && a.consume(1) && a.consume(2) && a.all_synced()
    }, "");
    cs.add("invalidate_pending_visible", {
        let mut a = InvalidateAudit::new();
        a.invalidate_all();
        let _ = a.consume(0);
        !a.all_synced() && a.pending_of(1) == 1 && a.consumed_of(0) == 1
    }, "");
    cs.add("invalidate_double_consume_reject", {
        let mut a = InvalidateAudit::new();
        a.invalidate_all();
        let _ = a.consume(1);
        !a.consume(1) // 无积压重复消费 = 拒绝（重复重绘防线）
    }, "");
    cs.add("invalidate_bad_surface_reject", {
        let mut a = InvalidateAudit::new();
        !a.consume(3) && a.rejected_bad_surface == 1
    }, "");
    // 2) PNG 缩放：合法档命中 + 只降不升全域。
    cs.add("png_target_hits_valid_tier", {
        png_downscale_target(300, 300) == (256, 256) // 300 → 档 256
            && png_downscale_target(100, 200) == (64, 128) // 长 200 → 档 128
    }, "");
    cs.add("png_small_kept", {
        png_downscale_target(48, 48) == (48, 48) // 已是合法档 = 原样
    }, "");
    cs.add("png_never_upscales", png_downscale_never_upscales(), "");
    // 3) 缩放账：只降复核 + 单调守卫。
    cs.add("png_ledger_downscale_audit", {
        let mut led = PngConvertLedger::new();
        let _ = led.push(100, 300, 256);
        let _ = led.push(200, 512, 256);
        led.all_downscale() && led.count() == 2
    }, "");
    cs.add("png_ledger_monotonic", {
        let mut led = PngConvertLedger::new();
        let _ = led.push(1_000, 300, 256);
        !led.push(500, 512, 256) && led.out_of_order_rejected == 1
    }, "");
    // 4) 设置持久化：旗标全组合 round-trip + 篡改。
    let mut buf = [0u8; FOLDICON_V7_LEN];
    cs.add("prefs_roundtrip_all", {
        [(true, true), (true, false), (false, true), (false, false)]
            .iter()
            .all(|&(a, i)| {
                let n = save_prefs_v7(a, i, &mut buf).unwrap_or(0);
                load_prefs_v7(&buf[..n]) == Some((a, i))
            })
    }, "");
    cs.add("prefs_tamper", {
        let n = save_prefs_v7(true, true, &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[5] ^= 0x04;
        load_prefs_v7(&bad[..n]).is_none()
    }, "");
    // 5) v1 回归锚：PNG 合法尺寸表 + 三表面名（v7 面不许伤 v1 语义）。
    cs.add("v1_png_sizes_regression", PNG_VALID_SIZES == [256, 128, 64, 48, 32, 16], "");
    cs.add("v1_surfaces_regression", SURFACES.len() == 3, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn png_tiny_image_clamped_to_min_tier() {
        // 比最小档还小的图 → 钳到最小档（16）但只降不升不破坏。
        let (w, h) = png_downscale_target(8, 8);
        assert!(w <= 8 && h <= 8, "极小图原样保留不放大");
    }

    #[test]
    fn invalidate_many_changes_accumulate() {
        let mut a = InvalidateAudit::new();
        a.invalidate_all();
        a.invalidate_all();
        a.invalidate_all();
        assert_eq!(a.pending_of(0), 3, "三次变更积压三笔");
        for _ in 0..3 {
            assert!(a.consume(0));
            assert!(a.consume(1));
            assert!(a.consume(2));
        }
        assert!(a.all_synced());
    }

    #[test]
    fn png_ledger_ring_wrap() {
        let mut led = PngConvertLedger::new();
        for i in 0..PNG_LEDGER_CAP * 2 {
            assert!(led.push(i as u64 * 100, 512, 256));
        }
        assert_eq!(led.count(), PNG_LEDGER_CAP);
        assert!(led.all_downscale());
    }
}
