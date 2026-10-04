//! F219 排序与视图记忆 · 全部按文件夹记住。
//!
//! **判据（主册）**：用户对列表做的每个选择都被记住：排序（名称/日期/
//! 类型/大小）、升降序、图标/列表/详情视图、详情视图的列宽与列顺序——
//! 全部按文件夹记住，新目录继承用户上次的全局默认；设置列表同理。
//! 「记住」是默认行为；「重置为默认」入口在每个列表右键菜单底部。
//!
//! **验收（主册第一句）**：记忆项清单（排序/视图/列宽/列序四类）齐备；
//! 重启后保持验证（序列化 round-trip）；继承逻辑三态用例（新建目录取
//! 全局默认 / 已设目录取自身 / 重置回默认）；重置入口存在性审计。
//!
//! **设计要点**：
//! - [`ViewMemory`] 四类记忆项一体承载（sort key+dir / view mode /
//!   col widths / col order），全 Copy 小体，零堆热路径；
//! - [`DirMemory`] 目录记忆表：路径 → 记忆，容量 [`MEM_CAP`] 封顶 +
//!   LRU 淘汰（最久未访问者出局，命中即续期）；
//! - [`ViewService`] 三态继承逻辑：已设目录取自身 / 未设目录取全局
//!   默认（[`MemSource`] 显性区分）/ 重置删除自身记忆回默认；
//! - 定长编码序列化（magic+version+逐条目 28+64 字节）——重启保持的
//!   判定面，解码全字段校验、垃圾/截断显性拒绝；
//! - 重置入口登记表 [`RESET_SURFACES`]：每个列表右键菜单底部的存在性
//!   审计（漏一处即缺陷）。
//!
//! **依赖锚点**：crate::checks::CheckSet；时间注入式（毫秒戳驱动 LRU
//! 续期与淘汰）；alloc Vec/String 仅用于记忆表与序列化面（有容量上限）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 目录记忆表容量上限（LRU 淘汰——记忆表是唯一进堆面，有界）。
pub const MEM_CAP: usize = 64;

/// 详情视图列数上限（名称/修改日期/类型/大小 + 备用列）。
pub const MAX_COLS: usize = 8;

/// 序列化：每条目录记忆的定长载荷（4 头 + 16 宽 + 8 序）。
pub const SER_ENTRY_BYTES: usize = 28;

/// 序列化：路径定长槽（1 字节长度 + 63 字节内容零填充）。
pub const SER_PATH_BYTES: usize = 64;

/// 序列化魔数（便携配置「换 U 盘跟着走」的身份面）。
pub const SER_MAGIC: [u8; 4] = *b"VMEM";

/// 序列化版本（字段布局演进守门）。
pub const SER_VERSION: u8 = 1;

/// 全局默认列宽初值（名称/修改日期/类型/大小——Windows 资源管理器惯例）。
pub const DEFAULT_COL_WIDTHS: [u16; 4] = [220, 140, 120, 100];

/// 「重置为默认」入口登记表——判据要求每个列表右键菜单底部都有。
pub const RESET_SURFACES: [&str; 4] = ["file-list", "settings-list", "task-view", "theme-list"];

// ---------------------------------------------------------------------------
// 四类记忆项
// ---------------------------------------------------------------------------

/// 排序键（名称/日期/类型/大小）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Date,
    Type,
    Size,
}

impl SortKey {
    pub fn from_u8(v: u8) -> Option<SortKey> {
        match v {
            0 => Some(SortKey::Name),
            1 => Some(SortKey::Date),
            2 => Some(SortKey::Type),
            3 => Some(SortKey::Size),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// 升降序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    pub fn from_u8(v: u8) -> Option<SortDir> {
        match v {
            0 => Some(SortDir::Asc),
            1 => Some(SortDir::Desc),
            _ => None,
        }
    }
}

/// 视图模式（图标/列表/详情）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Icons,
    List,
    Details,
}

impl ViewMode {
    pub fn from_u8(v: u8) -> Option<ViewMode> {
        match v {
            0 => Some(ViewMode::Icons),
            1 => Some(ViewMode::List),
            2 => Some(ViewMode::Details),
            _ => None,
        }
    }
}

/// 一份完整视图记忆：四类记忆项一体（排序/方向/视图/列宽/列序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewMemory {
    /// 排序键。
    pub sort_key: SortKey,
    /// 升降序。
    pub sort_dir: SortDir,
    /// 视图模式。
    pub view_mode: ViewMode,
    /// 详情视图列宽（px，仅前 col_count 列有效）。
    pub col_widths: [u16; MAX_COLS],
    /// 详情视图列顺序（列索引的展示序，值域 0..MAX_COLS）。
    pub col_order: [u8; MAX_COLS],
    /// 有效列数（钳入 1..=MAX_COLS）。
    pub col_count: u8,
}

impl ViewMemory {
    /// 全局默认（判据「新目录继承用户上次的全局默认」的出厂初值）。
    pub fn global_default() -> ViewMemory {
        let mut widths = [0u16; MAX_COLS];
        let mut order = [0u8; MAX_COLS];
        for i in 0..DEFAULT_COL_WIDTHS.len() {
            widths[i] = DEFAULT_COL_WIDTHS[i];
            order[i] = i as u8;
        }
        ViewMemory {
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            view_mode: ViewMode::Details,
            col_widths: widths,
            col_order: order,
            col_count: DEFAULT_COL_WIDTHS.len() as u8,
        }
    }

    /// 收敛非法字段（列数钳制、列序值域校验——越界列序回恒等序）。
    pub fn sanitized(mut self) -> ViewMemory {
        self.col_count = self.col_count.clamp(1, MAX_COLS as u8);
        for o in self.col_order.iter_mut() {
            if *o >= MAX_COLS as u8 {
                *o = 0;
            }
        }
        self
    }
}

// ---------------------------------------------------------------------------
// 目录记忆表（LRU）
// ---------------------------------------------------------------------------

/// 记忆来源（继承三态的显性区分面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemSource {
    /// 该目录自身设过——取自身记忆。
    OwnMemory,
    /// 未设过——继承全局默认。
    GlobalDefault,
}

/// 目录记忆表：路径 → 视图记忆，容量封顶 + LRU 淘汰。
pub struct DirMemory {
    entries: Vec<(String, ViewMemory, u64)>, // (路径, 记忆, 最近使用 ms)
    cap: usize,
    /// 命中/未命中/淘汰计数（审计面）。
    pub hits: u32,
    pub misses: u32,
    pub evictions: u32,
}

impl DirMemory {
    pub fn new(cap: usize) -> DirMemory {
        DirMemory {
            entries: Vec::new(),
            cap: cap.max(1),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 查询：命中续期（LRU touch），未命中计数。
    pub fn get(&mut self, path: &str, now_ms: u64) -> Option<ViewMemory> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == path) {
            e.2 = now_ms;
            self.hits += 1;
            Some(e.1)
        } else {
            self.misses += 1;
            None
        }
    }

    /// 记住：存在则更新，不存在则插入；超容淘汰最久未用者。
    pub fn remember(&mut self, path: &str, mem: ViewMemory, now_ms: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == path) {
            e.1 = mem;
            e.2 = now_ms;
            return;
        }
        if self.entries.len() >= self.cap {
            // LRU：逐出 last_used 最小者（平局取最早插入者）。
            let mut victim = 0usize;
            let mut oldest = u64::MAX;
            for (i, e) in self.entries.iter().enumerate() {
                if e.2 < oldest {
                    oldest = e.2;
                    victim = i;
                }
            }
            self.entries.remove(victim);
            self.evictions += 1;
        }
        self.entries.push((String::from(path), mem, now_ms));
    }

    /// 「重置为默认」：删除自身记忆（回退全局默认），返回是否确有记忆。
    pub fn forget(&mut self, path: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.0 == path) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// 全部条目快照（序列化面）。
    pub fn snapshot(&self) -> &[(String, ViewMemory, u64)] {
        &self.entries
    }
}

// ---------------------------------------------------------------------------
// 视图服务（三态继承逻辑）
// ---------------------------------------------------------------------------

/// 视图记忆服务：全局默认 + 目录记忆表的判定入口。
pub struct ViewService {
    default: ViewMemory,
    dirs: DirMemory,
    /// 累计重置次数（重置入口审计的消费计数）。
    pub resets: u32,
}

impl ViewService {
    pub fn new(default: ViewMemory) -> ViewService {
        ViewService {
            default,
            dirs: DirMemory::new(MEM_CAP),
            resets: 0,
        }
    }

    /// 解析目录应生效的视图：已设取自身 / 未设继承全局默认（来源显性）。
    pub fn resolve(&mut self, path: &str, now_ms: u64) -> (ViewMemory, MemSource) {
        match self.dirs.get(path, now_ms) {
            Some(m) => (m, MemSource::OwnMemory),
            None => (self.default, MemSource::GlobalDefault),
        }
    }

    /// 记住某目录的选择（「记住是默认行为」——每次用户改动即调用）。
    pub fn set(&mut self, path: &str, mem: ViewMemory, now_ms: u64) {
        self.dirs.remember(path, mem.sanitized(), now_ms);
    }

    /// 用户改动全局默认（只影响尚未自设的目录——继承语义）。
    pub fn set_default(&mut self, mem: ViewMemory) {
        self.default = mem.sanitized();
    }

    pub fn default(&self) -> ViewMemory {
        self.default
    }

    /// 重置：删自身记忆，此后 resolve 回全局默认。
    pub fn reset(&mut self, path: &str) -> bool {
        let gone = self.dirs.forget(path);
        if gone {
            self.resets += 1;
        }
        gone
    }

    pub fn dirs(&self) -> &DirMemory {
        &self.dirs
    }

    pub fn dirs_mut(&mut self) -> &mut DirMemory {
        &mut self.dirs
    }
}

/// 重置入口存在性审计：判据要求每个列表右键菜单底部都有该入口，
/// 漏一处即缺陷（登记制——界面侧登记自己的入口）。
pub fn audit_reset_entries(registered: &[&str]) -> bool {
    RESET_SURFACES.iter().all(|s| registered.contains(s))
}

// ---------------------------------------------------------------------------
// 序列化（重启保持的判定面）
// ---------------------------------------------------------------------------

/// 把一条记忆编码进 28 字节定长槽。
fn encode_mem(out: &mut [u8], m: &ViewMemory) {
    out[0] = m.sort_key.to_u8();
    out[1] = m.sort_dir as u8;
    out[2] = m.view_mode as u8;
    out[3] = m.col_count;
    for i in 0..MAX_COLS {
        let w = m.col_widths[i].to_le_bytes();
        out[4 + i * 2] = w[0];
        out[5 + i * 2] = w[1];
        out[20 + i] = m.col_order[i];
    }
}

/// 从 28 字节定长槽解码；全字段校验，非法即 None（垃圾显性拒绝）。
fn decode_mem(src: &[u8]) -> Option<ViewMemory> {
    if src.len() < SER_ENTRY_BYTES {
        return None;
    }
    let sort_key = SortKey::from_u8(src[0])?;
    let sort_dir = SortDir::from_u8(src[1])?;
    let view_mode = ViewMode::from_u8(src[2])?;
    let col_count = src[3];
    if col_count == 0 || col_count as usize > MAX_COLS {
        return None;
    }
    let mut widths = [0u16; MAX_COLS];
    for i in 0..MAX_COLS {
        widths[i] = u16::from_le_bytes([src[4 + i * 2], src[5 + i * 2]]);
    }
    let mut order = [0u8; MAX_COLS];
    order.copy_from_slice(&src[20..28]);
    for o in order.iter() {
        if *o >= MAX_COLS as u8 {
            return None;
        }
    }
    Some(ViewMemory { sort_key, sort_dir, view_mode, col_widths: widths, col_order: order, col_count })
}

/// 编码：魔数 + 版本 + 全局默认 + 目录记忆表（定长布局，便携配置面）。
pub fn encode_all(default: &ViewMemory, dirs: &DirMemory) -> Vec<u8> {
    let entries = dirs.snapshot();
    let mut out = Vec::with_capacity(7 + SER_ENTRY_BYTES + entries.len() * (SER_PATH_BYTES + SER_ENTRY_BYTES));
    out.extend_from_slice(&SER_MAGIC);
    out.push(SER_VERSION);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut slot = [0u8; SER_ENTRY_BYTES];
    encode_mem(&mut slot, default);
    out.extend_from_slice(&slot);
    for (path, mem, _last) in entries {
        let pb = path.as_bytes();
        let n = pb.len().min(SER_PATH_BYTES - 1);
        out.push(n as u8);
        out.extend_from_slice(&pb[..n]);
        for _ in n..SER_PATH_BYTES - 1 {
            out.push(0);
        }
        let mut slot = [0u8; SER_ENTRY_BYTES];
        encode_mem(&mut slot, mem);
        out.extend_from_slice(&slot);
    }
    out
}

/// 解码：布局/魔数/版本/字段全校验，返回 (全局默认, 目录记忆条目)。
pub fn decode_all(bytes: &[u8]) -> Option<(ViewMemory, Vec<(String, ViewMemory)>)> {
    if bytes.len() < 7 + SER_ENTRY_BYTES {
        return None;
    }
    if bytes[0..4] != SER_MAGIC || bytes[4] != SER_VERSION {
        return None;
    }
    let count = u16::from_le_bytes([bytes[5], bytes[6]]) as usize;
    let default = decode_mem(&bytes[7..7 + SER_ENTRY_BYTES])?;
    let mut out = Vec::new();
    let mut off = 7 + SER_ENTRY_BYTES;
    for _ in 0..count {
        if off + SER_PATH_BYTES + SER_ENTRY_BYTES > bytes.len() {
            return None; // 截断显性拒绝。
        }
        let n = bytes[off] as usize;
        if n >= SER_PATH_BYTES {
            return None;
        }
        let path = core::str::from_utf8(&bytes[off + 1..off + 1 + n]).ok()?;
        let mem = decode_mem(&bytes[off + SER_PATH_BYTES..off + SER_PATH_BYTES + SER_ENTRY_BYTES])?;
        out.push((String::from(path), mem));
        off += SER_PATH_BYTES + SER_ENTRY_BYTES;
    }
    Some((default, out))
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F219 自检（判据：四类记忆项 + 三态继承 + round-trip + LRU + 入口审计）。
pub fn run_viewmem_checks() -> CheckSet {
    let mut set = CheckSet::new("F219-viewmem");

    // 1. 四类记忆项全部记住（排序/方向/视图/列宽/列序一体）。
    let mut svc = ViewService::new(ViewMemory::global_default());
    let mut m = ViewMemory::global_default();
    m.sort_key = SortKey::Size;
    m.sort_dir = SortDir::Desc;
    m.view_mode = ViewMode::Icons;
    m.col_widths[0] = 300;
    m.col_order = [3, 2, 1, 0, 4, 5, 6, 7];
    svc.set("/home/user/项目", m, 100);
    let (got, src) = svc.resolve("/home/user/项目", 200);
    set.add(
        "four memory categories remembered",
        src == MemSource::OwnMemory
            && got.sort_key == SortKey::Size
            && got.sort_dir == SortDir::Desc
            && got.view_mode == ViewMode::Icons
            && got.col_widths[0] == 300
            && got.col_order[0] == 3,
        "",
    );

    // 2. 目录间隔离：A 目录的设置不泄给 B。
    let (b, src_b) = svc.resolve("/home/user/下载", 300);
    set.add(
        "per-directory isolation",
        src_b == MemSource::GlobalDefault && b.sort_key == SortKey::Name,
        "",
    );

    // 3. 三态继承之一：未设目录继承全局默认。
    set.add("new dir inherits global default", src_b == MemSource::GlobalDefault, "");

    // 4. 已设目录取自身（覆盖默认）。
    set.add("set dir takes own memory", src == MemSource::OwnMemory, "");

    // 5. 全局默认改动只影响未设目录。
    let mut d2 = ViewMemory::global_default();
    d2.sort_key = SortKey::Date;
    svc.set_default(d2);
    let (b2, _) = svc.resolve("/home/user/下载", 400);
    let (a2, _) = svc.resolve("/home/user/项目", 400);
    set.add(
        "default change hits unset dirs only",
        b2.sort_key == SortKey::Date && a2.sort_key == SortKey::Size,
        "",
    );

    // 6. 重置回默认（三态之三）。
    let was = svc.reset("/home/user/项目");
    let (a3, src3) = svc.resolve("/home/user/项目", 500);
    set.add(
        "reset falls back to global default",
        was && src3 == MemSource::GlobalDefault && a3.sort_key == SortKey::Date,
        "",
    );

    // 7. LRU 淘汰：容量封顶，最久未用者出局，命中续期者存活。
    let mut dm = DirMemory::new(4);
    for i in 0..5 {
        let path = alloc::format!("/dir{}", i);
        if i == 1 {
            dm.get("/dir0", 5000); // 续期 dir0 → dir1 成为最旧。
        }
        dm.remember(&path, ViewMemory::global_default(), 1000 + i);
    }
    set.add(
        "lru cap + eviction + renewal",
        dm.len() == 4
            && dm.evictions == 1
            && dm.snapshot().iter().all(|(p, _, _)| p != "/dir1"),
        "",
    );

    // 8. 序列化 round-trip（重启保持判定面）。
    let mut svc2 = ViewService::new(ViewMemory::global_default());
    svc2.set("/a", m, 10);
    svc2.set("/很长/中文/目录路径也可以", m, 11);
    let blob = encode_all(&svc2.default(), svc2.dirs());
    let (d3, entries) = decode_all(&blob).expect("合法编码必须可解码");
    set.add(
        "serialize round-trip (restart persistence)",
        d3 == svc2.default()
            && entries.len() == 2
            && entries[0].1 == m
            && entries[1].0 == "/很长/中文/目录路径也可以",
        "",
    );

    // 9. 垃圾与截断显性拒绝。
    let mut garbage = blob.clone();
    garbage[0] = b'X';
    let mut truncated = blob.clone();
    truncated.truncate(20);
    set.add(
        "garbage magic + truncated rejected",
        decode_all(&garbage).is_none() && decode_all(&truncated).is_none(),
        "",
    );

    // 10. 字段校验：非法排序键/列数拒绝。
    let mut bad = blob.clone();
    bad[7] = 9; // sort_key 越界
    set.add("invalid field rejected", decode_all(&bad).is_none(), "");

    // 11. 重置入口审计：四个列表全部登记才算绿。
    let full = ["file-list", "settings-list", "task-view", "theme-list"];
    let partial = ["file-list", "settings-list", "task-view"];
    set.add(
        "reset-entry audit: all four surfaces",
        audit_reset_entries(&full) && !audit_reset_entries(&partial),
        "",
    );

    // 12. sanitize：列数越界钳制、列序越界回零。
    let mut dirty = ViewMemory::global_default();
    dirty.col_count = 99;
    dirty.col_order[2] = 200;
    let clean = dirty.sanitized();
    set.add(
        "sanitized clamps col count + order",
        clean.col_count == MAX_COLS as u8 && clean.col_order[2] == 0,
        "",
    );

    // 13. fuzz：800 轮随机合法记忆编解码一致性（xorshift32 驱动）。
    let mut x: u32 = 0x6C078965;
    let mut ok = true;
    for i in 0..800u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let key = SortKey::from_u8((x % 4) as u8).unwrap_or(SortKey::Name);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let dir = if x % 2 == 0 { SortDir::Asc } else { SortDir::Desc };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let vm = ViewMode::from_u8((x % 3) as u8).unwrap_or(ViewMode::List);
        let mut mem = ViewMemory::global_default();
        mem.sort_key = key;
        mem.sort_dir = dir;
        mem.view_mode = vm;
        mem.col_count = 1 + ((x % MAX_COLS as u32) as u8);
        let mut svc3 = ViewService::new(ViewMemory::global_default());
        let path = alloc::format!("/fuzz/{}", i % 97);
        svc3.set(&path, mem, i as u64);
        let blob3 = encode_all(&svc3.default(), svc3.dirs());
        match decode_all(&blob3) {
            Some((_, e)) => {
                if e.len() != 1 || e[0].1 != mem {
                    ok = false;
                }
            }
            None => ok = false,
        }
    }
    set.add("fuzz 800 rounds encode/decode consistent", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherit_three_states_end_to_end() {
        let mut svc = ViewService::new(ViewMemory::global_default());
        // 新目录 → 默认。
        let (v, s) = svc.resolve("/新建", 1);
        assert_eq!(s, MemSource::GlobalDefault);
        assert_eq!(v.view_mode, ViewMode::Details);
        // 已设 → 自身。
        let mut m = ViewMemory::global_default();
        m.view_mode = ViewMode::List;
        svc.set("/新建", m, 2);
        let (v, s) = svc.resolve("/新建", 3);
        assert_eq!(s, MemSource::OwnMemory);
        assert_eq!(v.view_mode, ViewMode::List);
        // 重置 → 回默认。
        assert!(svc.reset("/新建"));
        let (_, s) = svc.resolve("/新建", 4);
        assert_eq!(s, MemSource::GlobalDefault);
        assert_eq!(svc.resets, 1);
        assert!(!svc.reset("/新建"), "重置无记忆目录返回 false");
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut dm = DirMemory::new(3);
        dm.remember("/a", ViewMemory::global_default(), 10);
        dm.remember("/b", ViewMemory::global_default(), 20);
        dm.remember("/c", ViewMemory::global_default(), 30);
        dm.get("/a", 40); // /a 续期 → /b 成为最旧。
        dm.remember("/d", ViewMemory::global_default(), 50);
        assert_eq!(dm.len(), 3);
        assert!(dm.snapshot().iter().all(|(p, _, _)| p != "/b"));
        assert_eq!(dm.evictions, 1);
    }

    #[test]
    fn serialize_round_trip_keeps_everything() {
        let mut svc = ViewService::new(ViewMemory::global_default());
        let mut m = ViewMemory::global_default();
        m.sort_key = SortKey::Type;
        m.col_widths[1] = 250;
        m.col_count = 3;
        svc.set("/etc", m, 5);
        svc.set_default(m);
        let blob = encode_all(&svc.default(), svc.dirs());
        let (d, e) = decode_all(&blob).unwrap();
        assert_eq!(d, m);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0], (String::from("/etc"), m));
    }

    #[test]
    fn decode_rejects_bad_layouts() {
        assert!(decode_all(&[]).is_none());
        let mut junk = vec![0u8; 64];
        junk[0..4].copy_from_slice(&SER_MAGIC);
        junk[4] = 99; // 版本错
        assert!(decode_all(&junk).is_none());
        junk[4] = SER_VERSION;
        junk[5] = 0xFF; // 条目数虚高 → 截断拒绝
        junk[6] = 0xFF;
        assert!(decode_all(&junk).is_none());
    }

    #[test]
    fn fuzz_selfcheck_green() {
        let set = run_viewmem_checks();
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F219 验收定义的实装细化，非新立项）：持久化面 = VXH1
// 带校验和的记忆记录（v1 encode_all 无校验和——v2 补 FNV-1a 门，载荷复用
// 既有 28 字节槽，零重复布局定义）；壳接线面 = 继承三态显性判定（新建/已
// 设/重置）+ 重置回默认值表；判定面 = run_viewmem_v2_checks。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 记录定长：4 魔数 + 1 版本 + 28 载荷（复用 SER_ENTRY_BYTES 槽）+ 4 校验。
pub const V2_MEM_RECORD_BYTES: usize = 4 + 1 + SER_ENTRY_BYTES + 4;

/// v2 持久化错误：四类损坏输入 + 字段域越界，全部显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength, BadField }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 带校验和的记忆记录编码（VXH1 + 版本 + 28 字节槽 + FNV-1a）。
/// 与 v1 encode_all 的分工：v1 面向全量表快照，v2 面向单条记忆的
/// 完整性门（校验错可检出——防静默损坏回灌）。
pub fn encode_mem_checked(m: &ViewMemory) -> [u8; V2_MEM_RECORD_BYTES] {
    let mut out = [0u8; V2_MEM_RECORD_BYTES];
    out[..4].copy_from_slice(&V2_MAGIC);
    out[4] = V2_VERSION;
    let mut slot = [0u8; SER_ENTRY_BYTES];
    encode_mem(&mut slot, m);
    out[5..5 + SER_ENTRY_BYTES].copy_from_slice(&slot);
    let sum = v2_fnv1a(&out[..V2_MEM_RECORD_BYTES - 4]);
    out[V2_MEM_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
    out
}

/// 带校验和的记忆记录解码：长度/魔数/版本/校验四门 + 字段域校验
/// （decode_mem 的 Option 全字段校验映射为 BadField）。
pub fn decode_mem_checked(b: &[u8]) -> Result<ViewMemory, V2PersistError> {
    if b.len() < V2_MEM_RECORD_BYTES { return Err(V2PersistError::BadLength); }
    if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
    if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
    let sum = u32::from_le_bytes([
        b[V2_MEM_RECORD_BYTES - 4],
        b[V2_MEM_RECORD_BYTES - 3],
        b[V2_MEM_RECORD_BYTES - 2],
        b[V2_MEM_RECORD_BYTES - 1],
    ]);
    if v2_fnv1a(&b[..V2_MEM_RECORD_BYTES - 4]) != sum {
        return Err(V2PersistError::BadChecksum);
    }
    decode_mem(&b[5..5 + SER_ENTRY_BYTES]).ok_or(V2PersistError::BadField)
}

// ---------------------------------------------------------------------------
// UI 壳接线：继承三态显性判定 + 重置回默认值表
// ---------------------------------------------------------------------------

/// 继承三态（判据「继承逻辑三态用例」的显性判定面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InheritTri {
    /// 新建目录——取全局默认。
    FreshDefault,
    /// 已设目录——取自身记忆。
    OwnSet,
    /// 重置后——删除自身记忆回默认。
    ResetFallback,
}

/// 三态判定：目录是否持有自身记忆 × 是否刚被重置（重置优先——刚重置
/// 的目录即使表内尚有残留也走回退态，与 ViewService::reset 删除语义同真值）。
pub fn inherit_verdict(has_own: bool, just_reset: bool) -> InheritTri {
    if just_reset {
        InheritTri::ResetFallback
    } else if has_own {
        InheritTri::OwnSet
    } else {
        InheritTri::FreshDefault
    }
}

/// 重置回默认值表：全局默认的只读快照（重置入口消费的唯一真值源——
/// 判据「重置为默认」入口在每个列表右键菜单底部）。
pub fn reset_defaults() -> ViewMemory {
    ViewMemory::global_default()
}

/// 重置产物核验：四类记忆项（排序/方向/视图/列宽/列序）逐字段与默认表
/// 一致（「重置回默认」的记录面读数，防半重置）。
pub fn reset_equals_default(m: &ViewMemory) -> bool {
    let d = reset_defaults();
    m.sort_key == d.sort_key
        && m.sort_dir == d.sort_dir
        && m.view_mode == d.view_mode
        && m.col_widths == d.col_widths
        && m.col_order == d.col_order
        && m.col_count == d.col_count
}

/// F219 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_viewmem_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F219-viewmem-v2");
    let mut m = ViewMemory::global_default();
    m.sort_key = SortKey::Size;
    m.sort_dir = SortDir::Desc;
    m.view_mode = ViewMode::Icons;
    m.col_widths[0] = 300;
    let blob = encode_mem_checked(&m);
    // 1. 验「重启后保持验证（序列化 round-trip）」v2 带校验和面：
    //    记录编码→解码逐字段相等。
    set.add("v2 checked record round-trip", decode_mem_checked(&blob) == Ok(m), "");
    // 2. 四类损坏输入全部拒绝 + 字段域越界（魔数/版本/校验/长度/字段）。
    // 缺陷账本：现象=「corruption five-way rejected」红；根因=bad_field 翻
    // 载荷字节 [5] 后未重算校验和（原注释「校验和仍对」不成立——字节 5 在
    // 校验覆盖区内），校验门先行返回 BadChecksum，BadField 分支不可达；
    // 修法=翻位后按同一 FNV-1a 口径重算校验和再送入，使字段门被真实测到
    // （判据「五类损坏逐一显性拒绝」要求每分支可达），不改实现。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    let mut bad_field = blob;
    bad_field[5] = 9; // sort_key 越界（重算校验和——只测字段门这一分支）
    let fs = v2_fnv1a(&bad_field[..V2_MEM_RECORD_BYTES - 4]);
    bad_field[V2_MEM_RECORD_BYTES - 4..].copy_from_slice(&fs.to_le_bytes());
    set.add(
        "corruption five-way rejected",
        decode_mem_checked(&bad_magic) == Err(V2PersistError::BadMagic)
            && decode_mem_checked(&bad_ver) == Err(V2PersistError::BadVersion)
            && decode_mem_checked(&bad_sum) == Err(V2PersistError::BadChecksum)
            && decode_mem_checked(&blob[..36]) == Err(V2PersistError::BadLength)
            && decode_mem_checked(&bad_field) == Err(V2PersistError::BadField),
        "",
    );
    // 3. 验「继承逻辑三态用例：新建/已设/重置」判定矩阵。
    set.add(
        "inherit tri-state verdicts",
        inherit_verdict(false, false) == InheritTri::FreshDefault
            && inherit_verdict(true, false) == InheritTri::OwnSet
            && inherit_verdict(true, true) == InheritTri::ResetFallback
            && inherit_verdict(false, true) == InheritTri::ResetFallback,
        "",
    );
    // 4. 验「重置回默认」四类记忆项全覆盖：默认表字段核对 + 防半重置。
    let d = reset_defaults();
    set.add(
        "reset table equals default",
        d.sort_key == SortKey::Name
            && d.sort_dir == SortDir::Asc
            && d.view_mode == ViewMode::Details
            && d.col_widths[..DEFAULT_COL_WIDTHS.len()] == DEFAULT_COL_WIDTHS
            && reset_equals_default(&ViewMemory::global_default()),
        "",
    );
    // 5. 校验和真值：载荷被篡改时校验门必须红（v1 面 decode_all 无校验和
    //    的补强点——防静默损坏回灌）。
    let mut tampered = encode_mem_checked(&m);
    tampered[8] ^= 0x01;
    set.add("checksum gate catches payload tamper", decode_mem_checked(&tampered).is_err(), "");
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn checked_record_round_trip_and_reject() {
        let mut m = ViewMemory::global_default();
        m.col_count = 3;
        m.col_order = [2, 1, 0, 3, 4, 5, 6, 7];
        let blob = encode_mem_checked(&m);
        assert_eq!(decode_mem_checked(&blob), Ok(m));
        assert_eq!(decode_mem_checked(&[]), Err(V2PersistError::BadLength));
        let mut junk = vec![0u8; V2_MEM_RECORD_BYTES];
        // 缺陷账本：现象=该单测红（actual BadMagic ≠ expected BadVersion）；
        // 根因=junk 未写合法魔数，解码门序（长度→魔数→版本→校验→字段）在
        // 魔数门即拒绝，版本门不可达；修法=先写入 V2_MAGIC 再翻版本字节，
        // 使版本门被真实测到，不改实现与门序。
        junk[0..4].copy_from_slice(&V2_MAGIC);
        junk[4] = 99;
        assert_eq!(decode_mem_checked(&junk), Err(V2PersistError::BadVersion));
    }

    #[test]
    fn tri_state_and_reset_table() {
        assert_eq!(inherit_verdict(true, false), InheritTri::OwnSet);
        let d = reset_defaults();
        assert!(reset_equals_default(&d));
        let mut half = ViewMemory::global_default();
        half.sort_key = SortKey::Date; // 半重置（只改排序）→ 不算回默认
        assert!(!reset_equals_default(&half));
    }

    #[test]
    fn viewmem_v2_selfcheck_all_green() {
        let set = run_viewmem_v2_checks();
        assert!(set.all_passed(), "F219 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
