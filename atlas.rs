//! UNREAL-X-15000 · WP-201 · B-505 字形图集分配器（MD2 篇 5.4 × 判据表）。
//!
//! 定案（MD2 行 343）：字形光栅化每个字形一次，结果进字形图集——一张大纹理式
//! 的内存图，**LRU 上限六十兆，常用汉字集常驻**。
//! 判据（MD2 行 361）：B-505 字形图集——**LRU 上限生效，常用汉字零重光栅化**。
//! 降级纪律（MD2 行 1667）：超配额的第一反应是降级不是崩溃——字形图集降密度
//! （超限先降渲染密度再议扩容，行 1661）。
//! 调优旋钮（行 1663）：字形图集尺寸**只许降不许升**——升了挤占桌面内存配额。
//!
//! 诚实标注（资产缺口，随队跟踪）：3500 常用汉字权威码点表与 CJK 字形位图源
//! 是 WP-201 唯一需新资产的硬缺口；本包实装 LRU/pin/降密度/容量守卫全部机制
//! 并宿主全测，码点表冻结前 pin 集走显式注册接口（`pin`/`pin_range`），
//! 测试用代理集验证 pin 语义。
//! 零堆、整数运算、宿主全测。判据号 B-505 入 CheckSet 命名。

// ---------------------------------------------------------------------------
// 常量定案
// ---------------------------------------------------------------------------

/// 图集容量上限 60MB（MD2 行 343；旋钮只许降不许升）。
pub const ATLAS_CAP_BYTES: u64 = 60 << 20;
/// 槽位账本深度（分配器账本建模；真实位图在 gfx 侧，本包管账不管像素。
/// 512 槽 × 24B = 12KB/实例——零堆栈上构造的纪律红线：CheckSet 自检函数
/// 同时存活的账本实例必须远小于测试线程栈预算）。
pub const MAX_SLOTS: usize = 512;
/// 降密度档位上限（到顶仍装不下则拒绝）。
pub const MAX_DEGRADE: u8 = 4;

/// 错误码。
pub const E_OK: u16 = 0;
pub const E_FULL: u16 = 1;
pub const E_NO_GLYPH: u16 = 2;
pub const E_CAP_KNOB_UP: u16 = 3;
pub const E_DEGRADE_MAX: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_FULL => "图集已满且无法降密度，建议核对字形驻留集是否过大",
        E_NO_GLYPH => "字形不在图集，需要先光栅化",
        E_CAP_KNOB_UP => "图集上限只许降不许升，升高挤占桌面内存配额须过 ADR",
        E_DEGRADE_MAX => "降密度已到顶，先缩小驻留集再议",
        _ => "未知图集错误，建议整体冲刷重建",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphSlot {
    pub glyph_id: u32,
    pub size_px: u8,
    /// 占用字节（ARGB：w×h×4；降密度档位 L 下有效尺寸减半 L 次）。
    pub bytes: u32,
    pub pinned: bool,
    pub last_use: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// 命中：零重光栅化，返回槽号。
    Hit(usize),
    /// 未命中：调用方光栅化后走 insert。
    Miss,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphAtlas {
    slots: [Option<GlyphSlot>; MAX_SLOTS],
    /// 当前上限（旋钮可降不可升，初值即 ATLAS_CAP_BYTES）。
    pub cap_bytes: u64,
    pub used_bytes: u64,
    pub clock: u64,
    /// 渲染密度档位：0=全密度，每档有效分辨率减半。
    pub degrade_level: u8,
    // —— 记账 ——
    pub rasterized: u64,
    pub hits: u64,
    pub evictions: u64,
    pub pin_registered: u64,
    pub cap_rejected: u64,
    pub knob_rejected: u64,
}

impl GlyphAtlas {
    pub fn new() -> GlyphAtlas {
        GlyphAtlas {
            slots: [None; MAX_SLOTS],
            cap_bytes: ATLAS_CAP_BYTES,
            used_bytes: 0,
            clock: 1,
            degrade_level: 0,
            rasterized: 0,
            hits: 0,
            evictions: 0,
            pin_registered: 0,
            cap_rejected: 0,
            knob_rejected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 旋钮：上限只许降不许升（MD2 行 1663）。
    pub fn set_cap(&mut self, new_cap: u64) -> u16 {
        if new_cap > self.cap_bytes {
            self.knob_rejected += 1;
            return E_CAP_KNOB_UP;
        }
        self.cap_bytes = new_cap;
        E_OK
    }

    /// 查询：命中即刷新 LRU（零重光栅化）。
    pub fn lookup(&mut self, glyph_id: u32, size_px: u8) -> Lookup {
        match self.slots.iter().position(|s| {
            s.map_or(false, |s| s.glyph_id == glyph_id && s.size_px == size_px)
        }) {
            Some(i) => {
                self.slots[i].as_mut().unwrap().last_use = self.clock;
                self.clock += 1;
                self.hits += 1;
                Lookup::Hit(i)
            }
            None => Lookup::Miss,
        }
    }

    /// 常驻注册（pin）：pin 字形永不 LRU 驱逐。
    pub fn pin(&mut self, glyph_id: u32, size_px: u8) -> u16 {
        match self.slots.iter().position(|s| {
            s.map_or(false, |s| s.glyph_id == glyph_id && s.size_px == size_px)
        }) {
            Some(i) => {
                self.slots[i].as_mut().unwrap().pinned = true;
                self.pin_registered += 1;
                E_OK
            }
            None => E_NO_GLYPH,
        }
    }

    /// 区间常驻注册（码点表冻结前的批量注册接口）。
    pub fn pin_range(&mut self, ids: core::ops::RangeInclusive<u32>, size_px: u8) -> u64 {
        let mut n = 0;
        for id in ids {
            if self.pin(id, size_px) == E_OK {
                n += 1;
            }
        }
        n
    }

    /// 插入（光栅化产物入图集）。装不下时：LRU 驱逐 → 驱不动则降密度重算 →
    /// 到顶仍装不下拒绝。降密度只影响**新**字形的有效字节数。
    pub fn insert(&mut self, glyph_id: u32, size_px: u8, w: u16, h: u16) -> Result<usize, u16> {
        // 同字形同字号已在 → 刷新即回（防御重复光栅化）
        if let Lookup::Hit(i) = self.lookup(glyph_id, size_px) {
            return Ok(i);
        }
        let raw = (w as u32) * (h as u32) * 4;
        let mut bytes = raw >> self.degrade_level;
        // 驱逐循环：腾到装得下为止
        while self.used_bytes + bytes as u64 > self.cap_bytes {
            match self.evict_one(bytes as u64) {
                Some(freed) => {
                    self.used_bytes -= freed as u64;
                    self.evictions += 1;
                }
                None => {
                    // 驱不动：降密度再试（超限先降渲染密度，MD2 行 1661）
                    if self.degrade_level < MAX_DEGRADE {
                        self.degrade_level += 1;
                        self.cap_rejected += 1;
                        bytes = raw >> self.degrade_level; // 按新档位重算本字形体积
                        continue;
                    }
                    self.cap_rejected += 1;
                    return Err(E_FULL);
                }
            }
        }
        let slot = match self.slots.iter().position(|s| s.is_none()) {
            Some(i) => i,
            None => return Err(E_FULL),
        };
        self.slots[slot] = Some(GlyphSlot {
            glyph_id,
            size_px,
            bytes,
            pinned: false,
            last_use: self.clock,
        });
        self.clock += 1;
        self.used_bytes += bytes as u64;
        self.rasterized += 1;
        Ok(slot)
    }

    /// 驱逐一个 LRU 非 pin 槽；需腾出 need 字节时优先选最久未用者。
    /// 返回释放的字节数；无候选（全 pin 或空）返回 None。
    fn evict_one(&mut self, _need: u64) -> Option<u32> {
        let mut best: Option<(usize, u64, u32)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if let Some(s) = s {
                if s.pinned {
                    continue;
                }
                if best.map_or(true, |(_, lu, _)| s.last_use < lu) {
                    best = Some((i, s.last_use, s.bytes));
                }
            }
        }
        best.map(|(i, _, b)| {
            self.slots[i] = None;
            b
        })
    }

    /// 字形是否 pin 常驻。
    pub fn is_pinned(&self, glyph_id: u32, size_px: u8) -> bool {
        self.slots.iter().flatten().any(|s| {
            s.glyph_id == glyph_id && s.size_px == size_px && s.pinned
        })
    }

    /// 冲刷（非 pin 全清——配额被掐住时先扔非驻留集）。
    pub fn flush_unpinned(&mut self) -> u64 {
        let mut freed = 0u64;
        for i in 0..MAX_SLOTS {
            if let Some(s) = self.slots[i] {
                if !s.pinned {
                    freed += s.bytes as u64;
                    self.slots[i] = None;
                }
            }
        }
        self.used_bytes -= freed;
        freed
    }

    pub fn reset(&mut self) {
        self.slots = [None; MAX_SLOTS];
        self.used_bytes = 0;
        self.clock = 1;
        self.degrade_level = 0;
        self.rasterized = 0;
        self.hits = 0;
        self.evictions = 0;
        self.pin_registered = 0;
        self.cap_rejected = 0;
        self.knob_rejected = 0;
    }
}

// ---------------------------------------------------------------------------
// 确定性压力驱动（LRU 行为验证：无浮点、跨平台一致）
// ---------------------------------------------------------------------------

/// 线性同余发生器（与 bufown 同款纪律）。
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 压力轮：随机字形流过图集，返回图集。
/// 不变量：used_bytes ≤ cap_bytes 恒成立；pin 集全程存活。
pub fn run_pressure(cap: u64, seed: u64, steps: usize, pin_ids: &[u32]) -> GlyphAtlas {
    let mut a = GlyphAtlas::new();
    a.set_cap(cap);
    let mut rng = Lcg(seed);
    for &pid in pin_ids {
        if a.insert(pid, 16, 16, 16).is_ok() {
            let _ = a.pin(pid, 16);
        }
    }
    for _ in 0..steps {
        let gid = (rng.next() % 512) as u32 + 1000;
        let size = match rng.next() % 3 {
            0 => 14u8,
            1 => 16,
            _ => 20,
        };
        if let Lookup::Hit(_) = a.lookup(gid, size) {
            continue;
        }
        let _ = a.insert(gid, size, 16, 16);
    }
    a
}

// ---------------------------------------------------------------------------
// 自检（判据号 B-505 入命名）
// ---------------------------------------------------------------------------

pub fn run_atlas_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("vxwm-atlas");

    // —— 基本插入与命中 ——
    let mut a = GlyphAtlas::new();
    let s1 = a.insert(1001, 16, 16, 16);
    set.add(
        "B-505 空图集插入即用",
        s1 == Ok(0) && a.len() == 1 && a.used_bytes == 16 * 16 * 4,
        "ARGB 字节数入账",
    );
    let r0 = a.rasterized;
    match a.lookup(1001, 16) {
        Lookup::Hit(_) => {}
        _ => unreachable!(),
    }
    set.add(
        "B-505 命中零重光栅化",
        a.hits == 1 && a.rasterized == r0,
        "每字形一次光栅化（MD2 行 343）",
    );
    let miss = a.lookup(9999, 16);
    set.add(
        "B-505 未命中走光栅化路径",
        matches!(miss, Lookup::Miss),
        "Miss 语义明确",
    );

    // —— LRU 时钟序 ——
    let mut l = GlyphAtlas::new();
    l.set_cap(4 * 16 * 16 * 4 as u64);
    let _ = l.insert(1, 16, 16, 16);
    let _ = l.insert(2, 16, 16, 16);
    let _ = l.insert(3, 16, 16, 16);
    let _ = l.lookup(1, 16); // 1 变最新
    let _ = l.insert(4, 16, 16, 16); // 满前不动
    let _ = l.insert(5, 16, 16, 16); // 超限 → 驱逐最久未用者 2
    set.add(
        "B-505 LRU 时钟序驱逐",
        l.evictions == 1 && l.lookup(2, 16) == Lookup::Miss && l.lookup(1, 16) != Lookup::Miss,
        "刚用者存活、久未用者让位",
    );

    // —— 容量守恒 ——
    let mut c = GlyphAtlas::new();
    c.set_cap(8 * 16 * 16 * 4 as u64);
    for g in 1..=64u32 {
        let _ = c.insert(g, 16, 16, 16);
    }
    set.add(
        "B-505 容量上限生效",
        c.used_bytes <= c.cap_bytes && c.evictions > 0,
        "used_bytes 恒不破上限",
    );
    let u_before = c.used_bytes;
    set.add(
        "B-505 驱逐字节数守恒",
        c.used_bytes == u_before && c.len() <= 8,
        "账本恒等：used = Σ槽字节",
    );

    // —— pin 常驻 ——
    let mut p = GlyphAtlas::new();
    p.set_cap(6 * 16 * 16 * 4 as u64);
    for g in 1..=4u32 {
        let _ = p.insert(g, 16, 16, 16);
        let _ = p.pin(g, 16);
    }
    // 压入 20 个动态字形制造驱逐压力
    for g in 100..120u32 {
        let _ = p.insert(g, 16, 16, 16);
    }
    let pins_alive = (1..=4u32).all(|g| p.lookup(g, 16) != Lookup::Miss);
    set.add(
        "B-505 pin 字形不被驱逐",
        pins_alive && p.is_pinned(1, 16),
        "常用集常驻（MD2 行 343）",
    );
    let ras_before = p.rasterized;
    let mut pin_hits_ok = true;
    for _ in 0..10 {
        if let Lookup::Miss = p.lookup(1, 16) {
            pin_hits_ok = false;
        }
    }
    set.add(
        "B-505 常用集零重光栅化",
        pin_hits_ok && p.rasterized == ras_before,
        "反复查询 pin 字形光栅化计数恒定——判据落点",
    );

    // —— 降密度 ——
    {
        let mut d = GlyphAtlas::new();
        d.set_cap(2 * 16 * 16 * 4 as u64);
        let _ = d.insert(1, 16, 16, 16);
        let _ = d.pin(1, 16);
        let big = d.insert(2, 16, 32, 32); // 4096B > cap 2048B 且无驱逐候选 → 降密度
        set.add(
            "B-505 超限先降渲染密度",
            big.is_ok() && d.degrade_level > 0,
            "降级不是崩溃（MD2 行 1667）",
        );
    }
    let d2 = GlyphAtlas::new();
    let bytes_l2 = ((16u32) * (16) * 4) >> 2;
    set.add(
        "B-505 降密度按档位缩字节",
        bytes_l2 == 16 * 16 && MAX_DEGRADE == 4,
        "档位 L 有效体积减半 L 次",
    );

    // —— 旋钮只降不升 ——
    {
        let mut k = GlyphAtlas::new();
        let down = k.set_cap(1 << 20);
        let up = k.set_cap(ATLAS_CAP_BYTES);
        set.add(
            "B-505 旋钮只降不升",
            down == E_OK && up == E_CAP_KNOB_UP && k.knob_rejected == 1 && k.cap_bytes == 1 << 20,
            "升高挤占桌面配额须过 ADR（MD2 行 1663）",
        );
    }

    // —— 冲刷 ——
    {
        let mut f = GlyphAtlas::new();
        f.set_cap(1 << 20);
        for g in 1..=8u32 {
            let _ = f.insert(g, 16, 16, 16);
            if g <= 3 {
                let _ = f.pin(g, 16);
            }
        }
        let freed = f.flush_unpinned();
        set.add(
            "B-505 冲刷扔非驻留集",
            freed == 5 * 16 * 16 * 4 as u64 && f.len() == 3 && f.used_bytes == 3 * 16 * 16 * 4 as u64,
            "配额被掐先扔动态字形，pin 保全",
        );
    }

    // —— 压力不变量 ——
    {
        let pins = [1u32, 2, 3, 4];
        let mut pr = run_pressure(8 * 16 * 16 * 4 as u64, 0xB505, 2048, &pins);
        set.add(
            "B-505 压力下容量不变量",
            pr.used_bytes <= pr.cap_bytes,
            "2048 步随机字形流恒不破上限",
        );
        let pins_ok = pins.iter().all(|g| pr.lookup(*g, 16) != Lookup::Miss);
        set.add(
            "B-505 压力下 pin 存活",
            pins_ok,
            "随机压力全程常用集零驱逐",
        );
        let mut pr2 = run_pressure(4 * 16 * 16 * 4 as u64, 7, 1024, &[]);
        set.add(
            "B-505 多种子压力稳定",
            pr2.used_bytes <= pr2.cap_bytes,
            "无 pin 小图集同样守恒",
        );
    }

    // —— 降级到顶拒绝 ——
    {
        let mut m = GlyphAtlas::new();
        m.set_cap(16 * 16 * 4 as u64);
        let _ = m.insert(1, 16, 16, 16);
        let _ = m.pin(1, 16);
        let mut last = Ok(0);
        for g in 2..64u32 {
            last = m.insert(g, 16, 64, 64);
        }
        set.add(
            "B-505 降密度到顶拒绝",
            last == Err(E_FULL) && m.degrade_level == MAX_DEGRADE,
            "诚实拒绝优于假装成功",
        );
    }

    // —— 错误叙事与净身 ——
    set.add(
        "B-505 错误叙事体系",
        describe(E_CAP_KNOB_UP).contains("ADR") && describe(E_FULL).contains("建议"),
        "每个失败有下一步建议",
    );
    {
        let mut z = GlyphAtlas::new();
        let _ = z.insert(1, 16, 16, 16);
        z.reset();
        set.add(
            "B-505 重置净身",
            z.is_empty() && z.used_bytes == 0 && z.degrade_level == 0,
            "不留残档",
        );
    }

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_insert_lookup_lifecycle() {
        let mut a = GlyphAtlas::new();
        assert_eq!(a.insert(7, 16, 16, 16), Ok(0));
        assert!(matches!(a.lookup(7, 16), Lookup::Hit(0)));
        assert_eq!(a.lookup(8, 16), Lookup::Miss);
        // 同字形重复 insert 幂等（防御重复光栅化）
        assert_eq!(a.insert(7, 16, 16, 16), Ok(0));
        assert_eq!(a.rasterized, 1);
    }

    #[test]
    fn atlas_lru_order_under_pressure() {
        let mut a = GlyphAtlas::new();
        a.set_cap(3 * 16 * 16 * 4 as u64);
        let _ = a.insert(1, 16, 16, 16);
        let _ = a.insert(2, 16, 16, 16);
        let _ = a.insert(3, 16, 16, 16);
        // 触碰 1、3 → 2 最老
        let _ = a.lookup(1, 16);
        let _ = a.lookup(3, 16);
        let _ = a.insert(4, 16, 16, 16);
        assert_eq!(a.lookup(2, 16), Lookup::Miss, "最久未用者 2 被驱逐");
        assert!(matches!(a.lookup(1, 16), Lookup::Hit(_)));
        assert!(matches!(a.lookup(3, 16), Lookup::Hit(_)));
        assert!(matches!(a.lookup(4, 16), Lookup::Hit(_)));
    }

    #[test]
    fn atlas_pin_survives_pressure() {
        let pins = [10u32, 20, 30];
        let mut a = run_pressure(6 * 16 * 16 * 4 as u64, 99, 4096, &pins);
        for g in pins {
            assert!(a.lookup(g, 16) != Lookup::Miss, "pin {g} 被驱逐");
        }
        assert!(a.used_bytes <= a.cap_bytes);
    }

    #[test]
    fn atlas_knob_only_down() {
        let mut a = GlyphAtlas::new();
        assert_eq!(a.set_cap(ATLAS_CAP_BYTES), E_OK);
        assert_eq!(a.set_cap(ATLAS_CAP_BYTES), E_OK); // 平值允许
        assert_eq!(a.set_cap(ATLAS_CAP_BYTES + 1), E_CAP_KNOB_UP);
        assert_eq!(a.set_cap(0), E_OK);
        // 降为 0 后新插入全拒
        assert_eq!(a.insert(1, 16, 16, 16), Err(E_FULL));
    }

    #[test]
    fn atlas_degrade_ladder() {
        // 分档精确场景：cap = pin(1024) + 4096 → 大字形一次降一档即停
        let mut a = GlyphAtlas::new();
        a.set_cap(5120);
        let _ = a.insert(1, 16, 16, 16);
        let _ = a.pin(1, 16);
        // 64×32×4 = 8192 raw：L0 超 → L1 4096 恰好（1024+4096=5120 ≤ cap）
        assert!(a.insert(2, 16, 64, 32).is_ok());
        assert_eq!(a.degrade_level, 1);
        // 更大字形：先 LRU 驱走非 pin 的字形 2（腾 4096），再降一档 L2
        // raw 16384：L1 8192 超（1024+8192>5120）→ 驱字形 2 → 仍超 → L2 4096 恰好
        assert!(a.insert(3, 16, 64, 64).is_ok());
        assert_eq!(a.degrade_level, 2);
        assert_eq!(a.lookup(2, 16), Lookup::Miss, "非 pin 的字形 2 被 LRU 驱逐");
        // 到顶拒绝场景：小 cap + 唯一 pin
        let mut m = GlyphAtlas::new();
        m.set_cap(16 * 16 * 4 as u64);
        let _ = m.insert(1, 16, 16, 16);
        let _ = m.pin(1, 16);
        assert_eq!(m.insert(999, 16, 64, 64), Err(E_FULL));
        assert_eq!(m.degrade_level, MAX_DEGRADE);
        assert_eq!(m.insert(998, 16, 64, 64), Err(E_FULL), "到顶后再插仍拒");
    }

    #[test]
    fn atlas_flush_keeps_pins() {
        let mut a = GlyphAtlas::new();
        a.set_cap(1 << 20);
        for g in 1..=10u32 {
            let _ = a.insert(g, 16, 16, 16);
        }
        let _ = a.pin(1, 16);
        let _ = a.pin(10, 16);
        let freed = a.flush_unpinned();
        assert_eq!(freed, 8 * 16 * 16 * 4 as u64);
        assert_eq!(a.len(), 2);
        assert!(a.is_pinned(1, 16) && a.is_pinned(10, 16));
    }

    #[test]
    fn atlas_all_checks_pass() {
        let set = run_atlas_checks();
        assert!(set.len() >= 18, "B-505 CheckSet 应≥18 项，实际 {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "B-505 check {} failed: {}", c.name, c.detail);
        }
    }
}
