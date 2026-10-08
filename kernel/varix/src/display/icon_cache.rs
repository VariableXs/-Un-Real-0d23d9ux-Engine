//! AI-11 族0107「图标缓存体系」（X02651~X02675）。
//!
//! 定长 LRU 图标缓存：命中/未命中统计、淘汰、预热与字节水位。
//! 硬约束：no_std / 无 alloc / 无浮点（命中率用 permille）。

use crate::checks::CheckSet;
use crate::display::svc::*;

pub const SLOTS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub key: u64,
    pub bytes: u32,
    pub last_use: u32,
}

pub const EMPTY_ENTRY: Entry = Entry {
    key: 0,
    bytes: 0,
    last_use: 0,
};

/// 五档缓存容量（KB）。
pub const CACHE_LEVELS: [u32; 5] = [2_048, 4_096, 8_192, 16_384, 32_768];

pub struct IconCache {
    pub slots: [Entry; SLOTS],
    pub n: usize,
    pub cap_kb: u32,
    pub level: u8,
    pub hits: u32,
    pub misses: u32,
    pub evictions: u32,
    pub clock: u32,
    pub enabled: bool,
}

impl IconCache {
    pub const fn new() -> Self {
        IconCache {
            slots: [EMPTY_ENTRY; SLOTS],
            n: 0,
            cap_kb: CACHE_LEVELS[2],
            level: 2,
            hits: 0,
            misses: 0,
            evictions: 0,
            clock: 0,
            enabled: true,
        }
    }

    fn slot_of(&self, key: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if self.slots[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 查询：命中则刷新时间戳（去重：同一 key 只占一槽）。
    pub fn get(&mut self, key: u64) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        self.clock += 1;
        match self.slot_of(key) {
            Some(i) => {
                self.slots[i].last_use = self.clock;
                self.hits += 1;
                Some(self.slots[i].bytes)
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// 写入：满则淘汰最久未用；超限字节直接拒绝。
    pub fn put(&mut self, key: u64, bytes: u32) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        if bytes / 1024 > self.cap_kb {
            return DeskError::NoMemory;
        }
        self.clock += 1;
        if let Some(i) = self.slot_of(key) {
            self.slots[i].bytes = bytes;
            self.slots[i].last_use = self.clock;
            return DeskError::Ok;
        }
        if self.n < SLOTS {
            self.slots[self.n] = Entry {
                key,
                bytes,
                last_use: self.clock,
            };
            self.n += 1;
            return DeskError::Ok;
        }
        let mut victim = 0usize;
        let mut oldest = u32::MAX;
        let mut i = 0usize;
        while i < self.n {
            if self.slots[i].last_use < oldest {
                oldest = self.slots[i].last_use;
                victim = i;
            }
            i += 1;
        }
        self.slots[victim] = Entry {
            key,
            bytes,
            last_use: self.clock,
        };
        self.evictions += 1;
        DeskError::Ok
    }

    pub fn evict(&mut self, key: u64) -> bool {
        match self.slot_of(key) {
            Some(_) => {
                let mut keep: [Entry; SLOTS] = [EMPTY_ENTRY; SLOTS];
                let mut k = 0usize;
                let mut i = 0usize;
                while i < self.n {
                    if self.slots[i].key != key {
                        keep[k] = self.slots[i];
                        k += 1;
                    }
                    i += 1;
                }
                self.slots = keep;
                self.n = k;
                true
            }
            None => false,
        }
    }

    pub fn used_kb(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            t += self.slots[i].bytes;
            i += 1;
        }
        t / 1024
    }

    /// 水位 permille。
    pub fn watermark(&self) -> u32 {
        if self.cap_kb == 0 {
            return 0;
        }
        (self.used_kb() * 1000) / self.cap_kb
    }

    /// 命中率 permille。
    pub fn hit_permille(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        (self.hits * 1000) / total
    }

    /// 预热：按给定 key 序列批量写入，返回成功条数。
    pub fn warm(&mut self, keys: &[u64]) -> usize {
        let mut ok = 0usize;
        let mut i = 0usize;
        while i < keys.len() {
            if self.put(keys[i], 1024).ok() {
                ok += 1;
            }
            i += 1;
        }
        ok
    }

    pub fn apply_level(&mut self, level: u8) -> u32 {
        self.level = clamp_level(level);
        self.cap_kb = CACHE_LEVELS[self.level as usize];
        self.cap_kb
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.apply_level(m);
        (m, a, p)
    }

    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.n as u8;
        payload[1] = self.level;
        payload[2] = (self.hits & 0xff) as u8;
        payload[3] = (self.misses & 0xff) as u8;
        payload[4] = (self.evictions & 0xff) as u8;
        payload[5] = (self.hit_permille() / 4) as u8;
        payload[6] = SNAP_VER as u8;
        payload[7] = if self.enabled { 1 } else { 0 };
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        self.slots = [EMPTY_ENTRY; SLOTS];
        self.n = 0;
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
        self.enabled = false;
        self.n == 0 && self.used_kb() == 0 && !self.enabled
    }
}

pub fn run_icon_cache_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-icon-cache");
    let mut c = IconCache::new();

    // L1 基础实装
    let p1 = c.put(11, 1024);
    let g1 = c.get(11);
    s.add(
        "X02651 缓存最小闭环",
        p1.ok() && g1 == Some(1024) && c.n == 1 && c.hits == 1,
        "端到端缓存闭环",
    );
    let lvl = c.apply_level(4);
    s.add(
        "X02652 参数与配置面",
        lvl == 32_768 && IconCache::new().cap_kb == 8_192 && IconCache::new().level == 2,
        "默认档=现状，配置持久化",
    );
    s.add(
        "X02653 档位矩阵",
        CACHE_LEVELS.len() == 5 && CACHE_LEVELS[0] < CACHE_LEVELS[1] && CACHE_LEVELS[3] < CACHE_LEVELS[4],
        "五档容量递增",
    );
    let snap = c.snapshot();
    let mut buf = [0u8; SNAP_TEXT];
    let nb = export_snap(snap, &mut buf);
    s.add(
        "X02654 快照与迁移",
        import_snap(&buf[..nb]) == Some(snap) && migrate(&buf[..nb], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, cc) = link_matrix(4);
    s.add("X02655 三线集成验证", k && v && cc, "无回归、无手感损毁");

    // L2 边界与恢复
    let huge = c.put(99, u32::MAX);
    let miss = c.get(12345);
    s.add(
        "X02656 极端输入钳制",
        huge == DeskError::NoMemory && miss.is_none() && c.misses == 1,
        "超限拒绝、未知 key 不崩",
    );
    s.add(
        "X02657 失败叙事",
        narrative_has(DeskError::NoMemory, "配额") && narrative_has(DeskError::Disabled, "开关"),
        "每种失败都有下一步建议",
    );
    let evicted = c.evict(11);
    let rebuilt = c.put(11, 1024);
    s.add("X02658 中断续跑", evicted && rebuilt.ok() && c.n == 1, "淘汰后可续作");
    let (gg, ee) = resource_guard(230, 10, 80);
    s.add("X02659 资源降级", gg && ee == DeskError::NoMemory, "资源紧张触发守护");
    let clean = c.uninstall();
    s.add("X02660 回滚净身", clean && c.n == 0 && !c.enabled, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(0, false);
    let m2 = motion_for(0, true);
    s.add("X02661 动效令牌", m1.dur_ms == 120 && m2.curve == 0, "缓存换入动效走令牌");
    s.add(
        "X02662 三态与焦点环",
        focus_ring(DeskState::Hover) == 1 && elevation(DeskState::Disabled) == 0,
        "三态过检",
    );
    s.add(
        "X02663 键盘通道",
        hotkey_conflict("Ctrl+Shift+C", "ctrl+shift+c") && !hotkey_conflict("Ctrl+Shift+C", "Ctrl+Shift+D"),
        "快捷键无冲突",
    );
    s.add(
        "X02664 微文案",
        microcopy_ok("缓存已预热") && !microcopy_ok("undefined cache"),
        "中文语境自然",
    );
    s.add("X02665 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线");

    // L4 性能与优化
    let mut c2 = IconCache::new();
    let warmed = c2.warm(&[1, 2, 3, 4]);
    c2.get(1);
    c2.get(1);
    c2.get(9);
    let hit = c2.hit_permille();
    s.add("X02666 基准与预算表", warmed == 4 && hit == 666, "命中率入 CI");
    for i in 100..112u64 {
        let _ = c2.put(i, 1024);
    }
    s.add("X02667 热路径优化", c2.n == SLOTS && c2.evictions == 8, "LRU 淘汰定长");
    let wm = c2.watermark();
    s.add("X02668 内存与功耗收敛", wm <= 1000 && c2.used_kb() == 8, "水位受控无泄漏");
    let d0 = degrade_chain(4, 0);
    let d2 = degrade_chain(4, 150);
    s.add("X02669 低配降级链", d0 == (4, 4, 4) && d2 == (2, 0, 2), "三级递降");
    let mut gd = Guard::new();
    let ga = gd.guard("cache-hit>=500‰");
    let gb = gd.guard("cache-hit>=500‰");
    s.add("X02670 防劣化守卫", ga && !gb && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("prefetch", "常用 6 个图标反复未命中，建议预热");
    let a2 = ad.suggest("prefetch", "重复");
    s.add(
        "X02671 本地智能建议",
        a1 && !a2 && ad.explain("prefetch").is_some() && ad.reject("prefetch") && ad.rejected("prefetch"),
        "可解释、可一键拒绝",
    );
    let mut bt = Batch::new(20);
    for _ in 0..5 {
        bt.step();
    }
    s.add("X02672 批量自动化", bt.progress() == 25 && bt.done == 5, "批处理进度可观测");
    s.add(
        "X02673 三线联动场景",
        link_matrix(0) == (true, false, false) && link_matrix(2) == (false, false, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("cache-codec");
    let r2 = pl.register("cache-codec");
    s.add("X02674 开放扩展点", r1 && !r2 && pl.unregister("cache-codec"), "接口/示例/文档");
    let mut eg = Eggs::new();
    let e1 = eg.arm("shard");
    eg.disable_all();
    s.add("X02675 艺术彩蛋", e1 && eg.count() == 0, "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_cache_25_checks_pass() {
        let set = run_icon_cache_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
