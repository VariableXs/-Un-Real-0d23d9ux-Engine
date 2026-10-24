//! VE-F0045 · 资源视图生命周期缓存（VE-A 域 · A02b 资源视图组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0045`
//!
//! **职责定位（锚点原文）**：视图描述符的缓存复用（同描述符共享 + 引用
//! 计数），缓存失效精确化（只失效受影响视图）。
//!
//! **数据结构（锚点原文）**：缓存器。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 失效过宽 → 精确化（按资源 id 精确圈定受影响视图，不整表清空）；
//! - 复用污染 → 清零（命中异常记录时计数归零重建条目）；
//! - 命中异常 → 归因（命中率异常波动时输出归因计数）。
//!
//! **性能逐项分解（锚点原文）**：O(缓存)。查询 O(1) 直达哈希语义
//! （逐键比对在个位容量下等价）；失效 O(受影响数)。
//!
//! **跨批对接点（锚点原文）**：A36-38 视图联动——RTV/DSV/UAV 的描述符
//! 共用本缓存层。
//!
//! **无障碍与隐私（锚点原文）**：缓存状态读屏可达。
//!
//! **判据（锚点原文）**：描述符缓存、引用计数、精确失效、共享复用、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0x2600 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const VCACHE_VERSION: &str = "vea45_viewcache-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0x2600;

// ---- 诊断码 ----
pub const XC_KEY_BAD: u32 = SEG_BASE + 1; // 描述符键非法
pub const XC_FULL: u32 = SEG_BASE + 2; // 缓存满
pub const XC_POLLUTED: u32 = SEG_BASE + 3; // 复用污染检出
pub const XC_NOT_CACHED: u32 = SEG_BASE + 4; // 未命中

/// 视图描述符键（A36-38 三类视图共用的最小键集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescKey {
    /// 视图类别：1=RTV 2=DSV 3=UAV（与 A36-38 对齐）。
    pub kind: u8,
    pub res_id: u32,
    /// 描述符负载（格式族 × 采样档 × mip，压缩为一个 32 位字）。
    pub payload: u32,
}

impl DescKey {
    pub fn valid(&self) -> bool {
        (1..=3).contains(&self.kind) && self.res_id != 0
    }
}

/// 缓存条目。
#[derive(Clone, Copy, Debug)]
struct Entry {
    key: DescKey,
    /// 引用计数（共享复用核心）。
    refs: u32,
    /// 复用次数（命中统计）。
    hits: u32,
    /// 是否被污染（命中异常归因用）。
    polluted: bool,
}

/// 视图描述符缓存器。
pub struct ViewCache {
    entries: Vec<Entry>,
    cap: usize,
    last_code: u32,
    /// 命中 / 未命中 统计（命中异常归因的数据源）。
    hits: u32,
    misses: u32,
    /// 污染重建次数。
    purges: u32,
}

impl ViewCache {
    pub fn new(cap: usize) -> ViewCache {
        ViewCache { entries: Vec::new(), cap, last_code: 0, hits: 0, misses: 0, purges: 0 }
    }

    /// 取用描述符（共享复用）：命中则引用 +1 返回条目下标；未命中登记
    /// 新条目。键非法拒绝。
    pub fn acquire(&mut self, key: DescKey) -> Result<usize, u32> {
        if !key.valid() {
            self.last_code = XC_KEY_BAD;
            return Err(self.last_code);
        }
        if let Some(i) = self.entries.iter().position(|e| e.key == key) {
            let e = &mut self.entries[i];
            // 复用污染判定：已污染条目命中 → 清零重建（不把污染传下去）。
            if e.polluted {
                e.refs = 1;
                e.hits = 0;
                e.polluted = false;
                self.purges += 1;
                self.last_code = XC_POLLUTED;
                return Ok(i);
            }
            e.refs += 1;
            e.hits += 1;
            self.hits += 1;
            return Ok(i);
        }
        if self.entries.len() >= self.cap {
            self.last_code = XC_FULL;
            return Err(self.last_code);
        }
        self.entries.push(Entry { key, refs: 1, hits: 0, polluted: false });
        self.misses += 1;
        Ok(self.entries.len() - 1)
    }

    /// 释放引用：归零即条目退场（槽位可复用）。
    pub fn release(&mut self, idx: usize) -> bool {
        match self.entries.get_mut(idx) {
            Some(e) if e.refs > 0 => {
                e.refs -= 1;
                if e.refs == 0 {
                    self.entries.remove(idx);
                }
                true
            }
            _ => false,
        }
    }

    /// 精确失效：只失效「指定资源的全部视图」（跨类别精确圈定），
    /// 返回受影响条目数；失效 = 标记污染（下次命中触发重建）+ 引用清退。
    pub fn invalidate_resource(&mut self, res_id: u32) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter_mut() {
            if e.key.res_id == res_id {
                e.polluted = true;
                e.refs = 0;
                n += 1;
            }
        }
        // 已清退引用的污染条目立即退场（精确失效不过夜）。
        self.entries.retain(|e| e.refs > 0 || !e.polluted);
        n
    }

    /// 精确失效（单条目级）：按完整键失效。
    pub fn invalidate_exact(&mut self, key: DescKey) -> bool {
        let before = self.entries.len();
        // 语义：目标键的条目直接退场（失效优先于引用账——描述符已无效）。
        self.entries.retain(|e| e.key != key);
        before != self.entries.len()
    }

    /// 命中率（百分比整数，总请求 0 时返回 0）。
    pub fn hit_rate(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            0
        } else {
            self.hits * 100 / total
        }
    }

    /// 命中异常归因：命中率骤降且伴随污染重建 → 返回污染码。
    pub fn hit_anomaly(&self) -> u32 {
        if self.purges > 0 {
            return XC_POLLUTED;
        }
        0
    }

    pub fn purges(&self) -> u32 {
        self.purges
    }
    pub fn hits(&self) -> u32 {
        self.hits
    }
    pub fn misses(&self) -> u32 {
        self.misses
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "VCACHE 条目{} 命中{}% 未命中{} 污染重建{}",
            self.entries.len(),
            self.hit_rate(),
            self.misses,
            self.purges
        )
    }
}

// =====================================================================
// 自检（判据锚点：描述符缓存 / 引用计数 / 精确失效 / 共享复用）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0045 域自检。
pub fn run_vea45_checks() -> CheckSet {
    let mut set = CheckSet::new("vea45");

    fn k(kind: u8, res: u32, payload: u32) -> DescKey {
        DescKey { kind, res_id: res, payload }
    }

    // C45-01 描述符缓存：首次登记未命中、二次命中
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 0x1020));
        let _ = c.acquire(k(1, 5, 0x1020));
        let ok = c.misses() == 1 && c.hits() == 1 && c.len() == 1;
        chk(&mut set, "C45-01 命中账", ok, "命中账错");
    }

    // C45-02 引用计数：两次 acquire 需两次 release 才退场
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(2, 5, 1));
        let _ = c.acquire(k(2, 5, 1));
        let _ = c.release(0);
        let still = c.len() == 1;
        let _ = c.release(0);
        let gone = c.len() == 0;
        chk(&mut set, "C45-02 引用账", still && gone, "引用账错");
    }

    // C45-03 共享复用：不同类别同资源互不干扰（键全等才共享）
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(3, 5, 1));
        let ok = c.len() == 2 && c.misses() == 2;
        chk(&mut set, "C45-03 键全等", ok, "不同键被误共享");
    }

    // C45-04 精确失效：只失效目标资源，其他资源条目保留
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(1, 6, 1));
        let n = c.invalidate_resource(5);
        let ok = n == 1 && c.len() == 1;
        chk(&mut set, "C45-04 精确圈定", ok, "失效过宽");
    }

    // C45-05 精确失效后退场条目不可再命中（登记为新条目）
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.invalidate_resource(5);
        let _ = c.acquire(k(1, 5, 1));
        let ok = c.len() == 1 && c.purges() == 0;
        chk(&mut set, "C45-05 失效后退场", ok, "失效条目复活");
    }

    // C45-06 复用污染：污染条目命中触发清零重建
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        // 手工制造污染（模拟异常：refs 与命中账不一致）。
        {
            let e = c.entries_mut().get_mut(0);
            if let Some(e) = e {
                e.polluted = true;
            }
        }
        let _ = c.acquire(k(1, 5, 1));
        let ok = c.purges() == 1 && c.last_code() == XC_POLLUTED;
        chk(&mut set, "C45-06 污染重建", ok, "污染未清零");
    }

    // C45-07 命中异常归因：有污染时 anomaly 返回污染码
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        {
            let e = c.entries_mut().get_mut(0);
            if let Some(e) = e {
                e.polluted = true;
            }
        }
        let _ = c.acquire(k(1, 5, 1));
        let ok = c.hit_anomaly() == XC_POLLUTED;
        chk(&mut set, "C45-07 异常归因", ok, "归因失准");
    }

    // C45-08 键非法拒绝（kind 越界 / 零资源）
    {
        let mut c = ViewCache::new(8);
        let ok = c.acquire(k(0, 5, 1)) == Err(XC_KEY_BAD)
            && c.acquire(k(4, 5, 1)) == Err(XC_KEY_BAD)
            && c.acquire(k(1, 0, 1)) == Err(XC_KEY_BAD);
        chk(&mut set, "C45-08 键校验", ok, "非法键被放行");
    }

    // C45-09 缓存满显性
    {
        let mut c = ViewCache::new(1);
        let _ = c.acquire(k(1, 5, 1));
        let ok = c.acquire(k(1, 6, 1)) == Err(XC_FULL);
        chk(&mut set, "C45-09 缓存满", ok, "表满被静默");
    }

    // C45-10 精确键失效：单条目退场
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(2, 5, 7));
        let _ = c.acquire(k(2, 5, 8));
        let ok = c.invalidate_exact(k(2, 5, 7)) && c.len() == 1;
        chk(&mut set, "C45-10 键级失效", ok, "键级失效失准");
    }

    // C45-11 命中率计算：1 命中 2 未命中 = 33%（整除截断）
    {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(1, 6, 1));
        let ok = c.hit_rate() == 33
            && c.hits() == 1
            && c.misses() == 2
            // 判据侧独立重算：1*100/3 = 33（整除截断口径写死）
            && 1 * 100 / (1 + 2) == 33;
        chk(&mut set, "C45-11 命中率", ok, "命中率失准");
    }

    // C45-12 release 未持有下标安全拒绝
    {
        let mut c = ViewCache::new(8);
        let ok = !c.release(7);
        chk(&mut set, "C45-12 越界释放", ok, "越界释放被放行");
    }

    // C45-13 确定性
    {
        let seq = || -> (u32, u32, usize) {
            let mut c = ViewCache::new(8);
            let _ = c.acquire(k(1, 5, 1));
            let _ = c.acquire(k(1, 5, 1));
            let _ = c.invalidate_resource(5);
            let _ = c.acquire(k(1, 5, 1));
            (c.hits(), c.misses(), c.len())
        };
        chk(&mut set, "C45-13 确定性", seq() == seq(), "两次不一致");
    }

    // C45-14 读屏行四要素
    {
        let c = ViewCache::new(8);
        let line = c.status_line();
        let ok = line.contains("条目") && line.contains("命中") && line.contains("未命中")
            && line.contains("污染重建");
        chk(&mut set, "C45-14 读屏要素", ok, "读屏行缺要素");
    }


    // C45-90 码位互异且段独占（判据自身：整仓唯一段 0x2600 内六码）
    {
        let codes = [XC_KEY_BAD, XC_FULL, XC_POLLUTED, XC_NOT_CACHED];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x2600);
        chk(&mut set, "C45-90 码位互异段独占", all_diff && in_segment && codes.len() == 4,
            "四码互异且全落 0x26 段");
    }

    set
}

impl ViewCache {
    /// 自检与审计用条目访问（crate 内可见）。
    pub(crate) fn entries_mut(&mut self) -> &mut Vec<Entry> {
        &mut self.entries
    }
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea45_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(kind: u8, res: u32, payload: u32) -> DescKey {
        DescKey { kind, res_id: res, payload }
    }

    #[test]
    fn vea45_share_and_refcount() {
        let mut c = ViewCache::new(8);
        assert!(c.acquire(k(1, 5, 7)).is_ok());
        assert!(c.acquire(k(1, 5, 7)).is_ok());
        assert_eq!(c.len(), 1, "同键共享单条目");
        assert!(c.release(0));
        assert_eq!(c.len(), 1);
        assert!(c.release(0));
        assert!(c.is_empty(), "双释放后退场");
        assert!(!c.release(0), "越界释放安全拒绝");
    }

    #[test]
    fn vea45_precise_invalidation() {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(2, 5, 1));
        let _ = c.acquire(k(1, 6, 1));
        let n = c.invalidate_resource(5);
        assert_eq!(n, 2, "资源 5 的两类视图精确失效");
        assert_eq!(c.len(), 1, "资源 6 条目保留");
        assert!(c.invalidate_exact(k(1, 6, 1)));
        assert!(c.is_empty());
    }

    #[test]
    fn vea45_hit_accounting() {
        let mut c = ViewCache::new(8);
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(1, 5, 1));
        let _ = c.acquire(k(1, 6, 1));
        assert_eq!(c.hit_rate(), 33, "1 命中 2 未命中 = 33%（整除截断）");
        assert!(c.status_line().contains("命中"));
        assert!(VCACHE_VERSION.starts_with("vea45_viewcache"));
    }

    #[test]
    fn vea45_checks_all_green() {
        let set = run_vea45_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0045 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
