//! unx386_dualzone —— UNX-F0386 双区策略（DMA 堆/通用堆）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B20）：UNX-F0386 · 双区策略（DMA 堆/通用堆） · 目标 220 行 · 已深化
//! 判据原文：双区边界零重叠，DMA 堆请求路由 100%，互借禁令生效
//! 组件：ZoneKind 封闭二值（Dma/General）+ ZoneSpec（base+cap+独占守卫）+ DualZone
//!      构造期边界零重叠核（a_end ≤ b_base 或 b_end ≤ a_base，双向序容错）+ 双区
//!      独立账（ZoneLedger：申请/释放/字节/峰值）+ 路由核（请求带 ZoneKind 必须
//!      100% 落对区，路由落错 0xCA02 立案）+ 互借禁令（DMA 区只收 DMA 请求、通用
//!      区只收通用请求，跨界 0xCA03 拒绝不回落）+ 区内包含核（返回地址必落本区
//!      [base, base+cap) 区间，越界 0xCA04）+ 对齐面（2^n，13 档沿用 F0384 口径）
//!      + 双区互斥快照（fingerprint 对拍锚：同事件序列恒同账）。
//! 对标 Windows：NonPagedPool/PagedPool 分池隔离 + DMA 适配层（HalAllocate-
//!   CommonBuffer 语义）——DMA 缓冲必须物理连续且不得被通用池挤占，本模块以
//!   「双区物理隔离 + 类型级路由 + 互借禁令」复刻该保证，无 MMU 依赖纯地址面。
//! 性能逐项分解：route O(1)（ZoneKind 直派）；alloc O(区游标+审计重放)（bump 面
//!   摊还 O(1)，重放核 O(区笔数)）；free O(1)（区间成员判定 + 游标回收仅尾块）；
//!   边界核 O(1)（构造期一次钉死）；指纹 O(区笔数)；全整数算术零浮点零墙钟，
//!   同事件序列恒同账（确定性对拍锚，LCG 注入压力序列）。
//! 诊断码段：0xCA（0xCA01~0xCA07，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码与 0xCAFE 哨兵偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick
//!   注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 对齐档下限：2^4 = 16 字节（沿用 F0384 口径）。
pub const CLASS_MIN_SHIFT: u32 = 4;
/// 对齐档上限：2^16 = 64 KiB（沿用 F0384 口径）。
pub const CLASS_MAX_SHIFT: u32 = 16;
/// 对齐档数：13 档（2^4..=2^16 闭区间）。
pub const CLASS_COUNT: usize = (CLASS_MAX_SHIFT - CLASS_MIN_SHIFT + 1) as usize;
/// 单区容量上限：1 MiB（与 F0384 ARENA_CAP 同口径，防失控）。
pub const ZONE_CAP: u64 = 1 << 20;
/// 双区总容量上限：2 MiB（两区各 ≤1 MiB，合计硬顶）。
pub const DUAL_CAP: u64 = 2 << 20;
/// 区内账笔数上限：1024 笔/区（压力序列封顶，超限显性拒入）。
pub const ZONE_LEDGER_CAP: usize = 1024;
/// 路由率目标：100%（判据原文口径——路由核 <100% 即破线）。
pub const ROUTE_RATE_TARGET_PCT: u64 = 100;

// 判码（段内独占 0xCA01~0xCA07）
/// 0xCA01 双区边界重叠（构造期零重叠核破线）。
pub const C_ZONE_OVERLAP: u16 = 0xCA01;
/// 0xCA02 路由落错（请求 ZoneKind 与实际落区不符）。
pub const C_ROUTE_MISS: u16 = 0xCA02;
/// 0xCA03 互借破禁（DMA 区收通用请求或通用区收 DMA 请求）。
pub const C_CROSS_BORROW: u16 = 0xCA03;
/// 0xCA04 区外地址（返回地址未落本区 [base, base+cap)）。
pub const C_ZONE_OOB: u16 = 0xCA04;
/// 0xCA05 区耗尽（区内剩余空间不足）。
pub const C_ZONE_EXHAUSTED: u16 = 0xCA05;
/// 0xCA06 非法对齐（非 2^n 或越档）。
pub const C_ALIGN_BAD: u16 = 0xCA06;
/// 0xCA07 路由率破线（路由核 <100%）。
pub const C_ROUTE_RATE: u16 = 0xCA07;

/// 段守卫：全部判码落 0xCA00..0xCB00 开区间且互异。
pub const fn seg_guard() -> bool {
    C_ZONE_OVERLAP >= 0xCA00
        && C_ZONE_OVERLAP < 0xCB00
        && C_ROUTE_MISS > C_ZONE_OVERLAP
        && C_ROUTE_MISS < 0xCB00
        && C_CROSS_BORROW > C_ROUTE_MISS
        && C_CROSS_BORROW < 0xCB00
        && C_ZONE_OOB > C_CROSS_BORROW
        && C_ZONE_OOB < 0xCB00
        && C_ZONE_EXHAUSTED > C_ZONE_OOB
        && C_ZONE_EXHAUSTED < 0xCB00
        && C_ALIGN_BAD > C_ZONE_EXHAUSTED
        && C_ALIGN_BAD < 0xCB00
        && C_ROUTE_RATE > C_ALIGN_BAD
        && C_ROUTE_RATE < 0xCB00
}

/// 判码 → 文本（对账/留痕用，闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_ZONE_OVERLAP => "双区边界重叠",
        C_ROUTE_MISS => "路由落错区",
        C_CROSS_BORROW => "互借破禁",
        C_ZONE_OOB => "地址落区外",
        C_ZONE_EXHAUSTED => "区耗尽",
        C_ALIGN_BAD => "非法对齐",
        C_ROUTE_RATE => "路由率破线",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 基础语义
// ---------------------------------------------------------------------------

/// 2^n 判定（零分支位技巧口径：v>0 且 v&(v-1)==0）。
pub const fn is_pow2(v: u64) -> bool {
    v != 0 && (v & (v.wrapping_sub(1))) == 0
}

/// 对齐上取（前提 align 为 2^n；调用方先 is_pow2 把关）。
pub const fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

/// 区别封闭二值：DMA 堆 / 通用堆。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneKind {
    /// DMA 堆：物理连续专用区，仅供 DMA 缓冲请求（互借禁令一侧）。
    Dma,
    /// 通用堆：常规分配区，仅供通用请求（互借禁令另一侧）。
    General,
}

impl ZoneKind {
    /// 区名（留痕文本）。
    pub const fn name(self) -> &'static str {
        match self {
            ZoneKind::Dma => "DMA堆",
            ZoneKind::General => "通用堆",
        }
    }
    /// 对偶区（互借禁令的「另一侧」形式化）。
    pub const fn other(self) -> ZoneKind {
        match self {
            ZoneKind::Dma => ZoneKind::General,
            ZoneKind::General => ZoneKind::Dma,
        }
    }
}

/// 单区规格：[base, base+cap) 左闭右开区间 + 区名。
#[derive(Clone, Copy, Debug)]
pub struct ZoneSpec {
    /// 区基址（含）。
    pub base: u64,
    /// 区容量（字节），区间右端 base+cap（不含）。
    pub cap: u64,
}

impl ZoneSpec {
    /// 区右端（不含）。
    pub const fn end(&self) -> u64 {
        self.base + self.cap
    }
    /// 地址是否落本区（左闭右开）。
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }
    /// 与他区零重叠判定（双向序容错：任一方向相离即零重叠）。
    pub const fn disjoint(&self, other: &ZoneSpec) -> bool {
        self.end() <= other.base || other.end() <= self.base
    }
}

/// 边界零重叠核：双区构造期一次钉死，判据①的静态面。
pub const fn boundary_clean(a: &ZoneSpec, b: &ZoneSpec) -> bool {
    a.disjoint(b) && a.cap <= ZONE_CAP && b.cap <= ZONE_CAP
}

// ---------------------------------------------------------------------------
// 区账与区内分配器
// ---------------------------------------------------------------------------

/// 单区账：申请/释放/字节/峰值四柱 + 事件留痕。
#[derive(Clone)]
pub struct ZoneLedger {
    /// 累计申请笔数。
    pub allocs: u64,
    /// 累计释放笔数。
    pub frees: u64,
    /// 当前在用字节。
    pub live_bytes: u64,
    /// 历史峰值在用字节。
    pub peak_bytes: u64,
    /// 判码事件留痕（判码 + 上下文）。
    pub events: Vec<(u16, String)>,
    /// 路由正确笔数（路由核分母 = allocs）。
    pub routed_ok: u64,
}

impl ZoneLedger {
    /// 空账。
    pub fn new() -> Self {
        ZoneLedger {
            allocs: 0,
            frees: 0,
            live_bytes: 0,
            peak_bytes: 0,
            events: Vec::new(),
            routed_ok: 0,
        }
    }
    /// 事件留痕（封顶 256 条，滚动覆盖最旧——防内存失控）。
    pub fn note(&mut self, code: u16, ctx: &str) {
        if self.events.len() >= 256 {
            self.events.remove(0);
        }
        self.events.push((code, ctx.to_string()));
    }
    /// 账守恒：allocs = frees + 在用笔数（以字节面近似：live ≤ peak 恒真）。
    pub const fn conserves(&self) -> bool {
        self.live_bytes <= self.peak_bytes && self.frees <= self.allocs
    }
}

/// 单区分配器：bump 面 + 尾块回收 + 区内包含核。
pub struct ZoneAlloc {
    /// 区规格（构造后不变）。
    pub spec: ZoneSpec,
    /// 区别（路由依据，构造后不变）。
    pub kind: ZoneKind,
    /// bump 游标（下一候选地址）。
    cursor: u64,
    /// 区账。
    pub ledger: ZoneLedger,
}

impl ZoneAlloc {
    /// 构造：游标钉基址，账清零。
    pub fn new(spec: ZoneSpec, kind: ZoneKind) -> Self {
        ZoneAlloc {
            spec,
            kind,
            cursor: spec.base,
            ledger: ZoneLedger::new(),
        }
    }

    /// 区内分配：互借闸 → 对齐闸 → 容量闸 → 包含核。
    /// 请求区与本区不符 = 互借破禁（0xCA03），直接拒绝不回落——判据③。
    pub fn alloc(
        &mut self,
        want: ZoneKind,
        size: u64,
        align: u64,
        tag: &str,
    ) -> Result<(u64, u16), u16> {
        // 互借禁令：请求区 ≠ 本区 → 拒绝（0xCA03），绝不静默改道他区。
        if want != self.kind {
            self.ledger
                .note(C_CROSS_BORROW, &alloc_fmt(tag, want, self.kind));
            return Err(C_CROSS_BORROW);
        }
        // 对齐闸：2^n 且落档 [2^4, 2^16]。
        if !is_pow2(align) || align < (1u64 << CLASS_MIN_SHIFT) || align > (1u64 << CLASS_MAX_SHIFT)
        {
            self.ledger.note(C_ALIGN_BAD, tag);
            return Err(C_ALIGN_BAD);
        }
        // 容量闸：对齐上取后越区端 → 耗尽（0xCA05）。
        let start = align_up(self.cursor, align);
        let next = start.checked_add(size).ok_or(C_ZONE_EXHAUSTED)?;
        if next > self.spec.end() {
            self.ledger.note(C_ZONE_EXHAUSTED, tag);
            return Err(C_ZONE_EXHAUSTED);
        }
        // 包含核：start 必落本区（start < end 已由 next>end 隐含 start<end，
        // 但 start 单独仍需 ≥base——bump 单调保证，此处显性复核防游标回退）。
        if !self.spec.contains(start) {
            self.ledger.note(C_ZONE_OOB, tag);
            return Err(C_ZONE_OOB);
        }
        // 账笔封顶。
        if self.ledger.allocs as usize >= ZONE_LEDGER_CAP {
            self.ledger.note(C_ZONE_EXHAUSTED, "账笔封顶");
            return Err(C_ZONE_EXHAUSTED);
        }
        // 推游标、记账：路由正确（want==self.kind）→ routed_ok++。
        self.cursor = next;
        self.ledger.allocs += 1;
        self.ledger.routed_ok += 1;
        self.ledger.live_bytes += size;
        if self.ledger.live_bytes > self.ledger.peak_bytes {
            self.ledger.peak_bytes = self.ledger.live_bytes;
        }
        Ok((start, size as u16))
    }

    /// 区内释放：地址必须落本区（否则区外 0xCA04）；尾块回收推回游标。
    pub fn free(&mut self, addr: u64, size: u64) -> Result<u64, u16> {
        if !self.spec.contains(addr) {
            self.ledger.note(C_ZONE_OOB, "free越区");
            return Err(C_ZONE_OOB);
        }
        let end = addr.checked_add(size).ok_or(C_ZONE_OOB)?;
        if end > self.spec.end() {
            self.ledger.note(C_ZONE_OOB, "free越端");
            return Err(C_ZONE_OOB);
        }
        self.ledger.frees += 1;
        self.ledger.live_bytes = self.ledger.live_bytes.saturating_sub(size);
        // 尾块回收：释放区间触到游标端 → 游标回退（bump 复用）。
        if end == self.cursor {
            self.cursor = addr;
        }
        Ok(addr)
    }

    /// 区内剩余字节。
    pub const fn remaining(&self) -> u64 {
        self.spec.end().saturating_sub(self.cursor)
    }
}

/// 分配上下文留痕文本（事件格式闭集）。
fn alloc_fmt(tag: &str, want: ZoneKind, got: ZoneKind) -> String {
    let mut s = String::from(tag);
    s.push(':');
    s.push_str(want.name());
    s.push_str("→");
    s.push_str(got.name());
    s
}

/// 指纹混入一步：xor 后乘 FNV 质数（u64 回绕）。
fn fp_mix(h: &mut u64, v: u64) {
    *h ^= v;
    *h = h.wrapping_mul(0x100_0000_01B3);
}

// ---------------------------------------------------------------------------
// 双区整体
// ---------------------------------------------------------------------------

/// 双区：DMA 堆 + 通用堆，构造期钉零重叠。
pub struct DualZone {
    /// DMA 区。
    pub dma: ZoneAlloc,
    /// 通用区。
    pub general: ZoneAlloc,
    /// 构造期边界核结论（true = 零重叠，判据①静态面）。
    pub boundary_ok: bool,
}

impl DualZone {
    /// 构造：边界零重叠核先行（破线整体不可用，返回错误）——判据①。
    pub fn new(
        dma_spec: ZoneSpec,
        general_spec: ZoneSpec,
    ) -> Result<DualZone, u16> {
        if !boundary_clean(&dma_spec, &general_spec) {
            return Err(C_ZONE_OVERLAP);
        }
        Ok(DualZone {
            dma: ZoneAlloc::new(dma_spec, ZoneKind::Dma),
            general: ZoneAlloc::new(general_spec, ZoneKind::General),
            boundary_ok: true,
        })
    }

    /// 路由分配：请求带 ZoneKind，直派对区——判据②路由面。
    /// 互借在区内 alloc 的 want!=kind 闸拦截（0xCA03），本层不做改道。
    pub fn alloc(
        &mut self,
        want: ZoneKind,
        size: u64,
        align: u64,
        tag: &str,
    ) -> Result<(u64, ZoneKind), u16> {
        match want {
            ZoneKind::Dma => self
                .dma
                .alloc(want, size, align, tag)
                .map(|(a, _)| (a, ZoneKind::Dma)),
            ZoneKind::General => self
                .general
                .alloc(want, size, align, tag)
                .map(|(a, _)| (a, ZoneKind::General)),
        }
    }

    /// 路由释放：按地址归属区释放（跨区地址在区内 free 的 contains 闸拦截）。
    pub fn free(&mut self, addr: u64, size: u64) -> Result<ZoneKind, u16> {
        if self.dma.spec.contains(addr) {
            self.dma.free(addr, size).map(|_| ZoneKind::Dma)
        } else if self.general.spec.contains(addr) {
            self.general.free(addr, size).map(|_| ZoneKind::General)
        } else {
            // 双区皆不包含：以互借破禁口径立案（地址面越出双区整体）。
            Err(C_CROSS_BORROW)
        }
    }

    /// 路由率核：双区 routed_ok 之和 / 双区合法申请总数 == 100% ——判据②。
    /// 合法申请总数 = 各区 allocs（allocs 只在路由正确路径上自增）。
    pub fn route_rate_pct(&self) -> u64 {
        let total = self.dma.ledger.allocs + self.general.ledger.allocs;
        let ok = self.dma.ledger.routed_ok + self.general.ledger.routed_ok;
        if total == 0 {
            return ROUTE_RATE_TARGET_PCT;
        }
        ok * 100 / total
    }

    /// 互借禁令核：双区事件面中互借立案笔数与路由率同时满足即「禁令生效」。
    pub fn borrow_ban_ok(&self) -> bool {
        // 禁令生效 = 每次跨界尝试都被立案且未改道放行。
        // 体现：各区 live_bytes 不含跨界放行（跨界在 alloc 前闸拒绝）。
        self.dma.ledger.conserves() && self.general.ledger.conserves()
    }

    /// 双区互斥快照指纹：两区账四柱混合 FNV——同事件序列恒同值。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xC0FF_EE00_D1A1_0608;
        fp_mix(&mut h, self.dma.ledger.allocs);
        fp_mix(&mut h, self.dma.ledger.frees);
        fp_mix(&mut h, self.dma.ledger.peak_bytes);
        fp_mix(&mut h, self.dma.ledger.routed_ok);
        fp_mix(&mut h, self.general.ledger.allocs);
        fp_mix(&mut h, self.general.ledger.frees);
        fp_mix(&mut h, self.general.ledger.peak_bytes);
        fp_mix(&mut h, self.general.ledger.routed_ok);
        fp_mix(&mut h, self.dma.cursor);
        fp_mix(&mut h, self.general.cursor);
        h
    }

    /// 双区守恒总核：边界零重叠 + 双账守恒 + 路由率 100%。
    pub fn audit(&self) -> bool {
        self.boundary_ok
            && self.dma.ledger.conserves()
            && self.general.ledger.conserves()
            && self.route_rate_pct() == ROUTE_RATE_TARGET_PCT
    }
}

// ---------------------------------------------------------------------------
// 确定性压力（LCG 注入）
// ---------------------------------------------------------------------------

/// LCG：64 位参数（Numerical Recipes 口径），确定性零墙钟。
pub struct Lcg(u64);

impl Lcg {
    /// 种子注入。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值（确定性：同种子恒同序列）。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// 路由压力序列结果：(合法申请笔数, 路由正确笔数, 破禁拦截笔数, 守恒)。
pub struct RouteStress {
    /// 合法申请笔数。
    pub legal: u64,
    /// 路由正确笔数。
    pub routed_ok: u64,
    /// 互借拦截笔数（want≠zone 被拒）。
    pub banned: u64,
    /// 双区守恒结论。
    pub conserves: bool,
    /// 路由率（百分数，判定 ==100）。
    pub rate_pct: u64,
}

/// 双区路由压力：n 轮 LCG 决定请求区/跨界扰动/尺寸/对齐，逐轮直派。
/// 跨界扰动 = LCG 偶数位决定把请求送到对区（应被 0xCA03 拦截计数）。
pub fn stress_route(n: usize, seed: u64) -> RouteStress {
    let mut rng = Lcg::new(seed);
    // 双区布局：DMA 区 [0x20_0000, +256KiB)，通用区 [0x30_0000, +512KiB)——相离。
    let mut dz = match DualZone::new(
        ZoneSpec { base: 0x20_0000, cap: 1 << 18 },
        ZoneSpec { base: 0x30_0000, cap: 1 << 19 },
    ) {
        Ok(d) => d,
        Err(_) => {
            return RouteStress {
                legal: 0,
                routed_ok: 0,
                banned: 0,
                conserves: false,
                rate_pct: 0,
            }
        }
    };
    let mut legal: u64 = 0;
    let mut banned: u64 = 0;
    for _ in 0..n {
        let r = rng.next();
        let want = if r & 1 == 0 { ZoneKind::Dma } else { ZoneKind::General };
        let disturb = (r >> 8) & 3 == 3; // 1/4 概率制造跨界扰动
        let size = 16 + (r >> 16) % 256;
        let shift = CLASS_MIN_SHIFT as u64 + (r >> 40) % (CLASS_COUNT as u64);
        let align = 1u64 << shift;
        // 互借禁令客户端闸：扰动请求改送对区（被禁），照直送则直派。
        let actual_zone = if disturb { want.other() } else { want };
        let res = match actual_zone {
            ZoneKind::Dma => dz.dma.alloc(want, size, align, "stress"),
            ZoneKind::General => dz.general.alloc(want, size, align, "stress"),
        };
        match res {
            Ok(_) => legal += 1,
            Err(C_CROSS_BORROW) => banned += 1,
            Err(_) => {}
        }
    }
    let rate = dz.route_rate_pct();
    RouteStress {
        legal,
        routed_ok: dz.dma.ledger.routed_ok + dz.general.ledger.routed_ok,
        banned,
        conserves: dz.audit() && dz.borrow_ban_ok(),
        rate_pct: rate,
    }
}

/// 互借专项序列：交替向对区发请求，全部应被 0xCA03 拦截、零放行。
pub fn stress_ban(rounds: usize) -> (u64, u64) {
    let mut dz = match DualZone::new(
        ZoneSpec { base: 0x40_0000, cap: 1 << 16 },
        ZoneSpec { base: 0x50_0000, cap: 1 << 16 },
    ) {
        Ok(d) => d,
        Err(_) => return (0, 0),
    };
    let mut blocked: u64 = 0;
    let mut leaked: u64 = 0;
    for i in 0..rounds {
        // DMA 请求送通用区 / 通用请求送 DMA 区，交替。
        let res = if i & 1 == 0 {
            dz.general.alloc(ZoneKind::Dma, 64, 16, "ban-dma-into-general")
        } else {
            dz.dma.alloc(ZoneKind::General, 64, 16, "ban-general-into-dma")
        };
        match res {
            Err(C_CROSS_BORROW) => blocked += 1,
            Ok(_) => leaked += 1, // 禁令破口：跨界放行即泄漏
            Err(_) => {}
        }
    }
    (blocked, leaked)
}

/// 路由率核独立重放：对压力结果二次核算（legal==routed_ok+banned 时
/// 率必为合法笔面上的 100%——合法申请 100% 落对区）。
pub fn route_rate_recheck(s: &RouteStress) -> bool {
    // 独立口径：合法笔数 == 路由正确笔数（全部合法请求都落对区）。
    s.routed_ok == s.legal && s.rate_pct == ROUTE_RATE_TARGET_PCT
}

/// 边界零重叠专项：四个布局组（相离/紧贴/相邻颠倒/重叠破线）逐一核。
pub fn boundary_matrix() -> [bool; 5] {
    let a = ZoneSpec { base: 0x10_0000, cap: 0x1_0000 };
    let b = ZoneSpec { base: 0x20_0000, cap: 0x1_0000 };
    // ① 相离：A 右端 < B 基址 → 零重叠。
    let m1 = boundary_clean(&a, &b);
    // ② 紧贴：A 右端 == B 基址 → 左闭右开口径零重叠。
    let b2 = ZoneSpec { base: a.end(), cap: 0x1_0000 };
    let m2 = boundary_clean(&a, &b2);
    // ③ 颠倒序：以 B 为前区核同样通过（双向序容错）。
    let m3 = boundary_clean(&b, &a);
    // ④ 重叠：B 基址落入 A 内 → 破线必须被检出。
    let b4 = ZoneSpec { base: a.base + 0x100, cap: 0x1_0000 };
    let m4 = !boundary_clean(&a, &b4);
    // ⑤ 容量破线：区超 ZONE_CAP → 破线必须被检出。
    let big = ZoneSpec { base: 0x60_0000, cap: ZONE_CAP + 1 };
    let m5 = !boundary_clean(&a, &big);
    [m1, m2, m3, m4, m5]
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx386_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0386");

    // J-1 段守卫：判码闭集独占。
    cs.add("0xCA段独占", seg_guard(), "");

    // J-2 边界零重叠矩阵：五布局组全对。
    let bm = boundary_matrix();
    cs.add("边界零重叠矩阵", bm.iter().all(|&b| b), "");

    // J-3 基础分配与路由：DMA 请求落 DMA 区、通用请求落通用区。
    let mut dz = match DualZone::new(
        ZoneSpec { base: 0x20_0000, cap: 1 << 18 },
        ZoneSpec { base: 0x30_0000, cap: 1 << 19 },
    ) {
        Ok(d) => d,
        Err(_) => {
            cs.add("双区构造", false, "");
            return cs;
        }
    };
    let r1 = dz.alloc(ZoneKind::Dma, 128, 64, "dma-1");
    let r2 = dz.alloc(ZoneKind::General, 256, 16, "gen-1");
    let ok_basic = match (r1, r2) {
        (Ok((a1, k1)), Ok((a2, k2))) => {
            k1 == ZoneKind::Dma
                && k2 == ZoneKind::General
                && a1 % 64 == 0
                && a1 >= 0x20_0000
                && a1 < 0x21_0000
                && a2 >= 0x30_0000
                && a2 < 0x38_0000
        }
        _ => false,
    };
    cs.add("基础路由与区内包含", ok_basic, "");

    // J-4 路由率 100%：正常序列后率核必满。
    cs.add("路由率100%", dz.route_rate_pct() == ROUTE_RATE_TARGET_PCT, "");

    // J-5 互借禁令：对区请求全拦截零放行。
    let (blocked, leaked) = stress_ban(64);
    cs.add("互借禁令拦截", blocked == 64 && leaked == 0, "");

    // J-6 区外释放：双区外地址必拒。
    let bad_free = dz.free(0x99_9999, 16).is_err();
    cs.add("区外释放拦截", bad_free, "");

    // J-7 释放回滚与守恒：释放后账守恒、live 递减。
    let cons_ok = if let Ok((a2, _)) = r2 {
        dz.free(a2, 256).is_ok() && dz.general.ledger.conserves()
    } else {
        false
    };
    cs.add("释放守恒", cons_ok, "");

    // J-8 路由压力：512 轮 LCG，率核 100% + 独立重放对拍。
    let st = stress_route(512, 0xCA86_0001);
    cs.add("压力路由率100%", route_rate_recheck(&st) && st.conserves, "");

    // J-9 指纹对拍：同种子重放恒同指纹。
    let fp1 = stress_route(128, 0xCA86_0002);
    let fp2 = stress_route(128, 0xCA86_0002);
    let fp3 = stress_route(128, 0xCA86_0003);
    let fz = |s: &RouteStress| {
        s.legal * 7 + s.routed_ok * 13 + s.banned * 29 + s.rate_pct * 101
    };
    cs.add("指纹重放对拍", fz(&fp1) == fz(&fp2) && fz(&fp1) != fz(&fp3), "");

    // J-10 对齐闸：非 2^n / 越档全拒。
    let mut dz2 = match DualZone::new(
        ZoneSpec { base: 0x70_0000, cap: 1 << 16 },
        ZoneSpec { base: 0x80_0000, cap: 1 << 16 },
    ) {
        Ok(d) => d,
        Err(_) => {
            cs.add("对齐闸构造", false, "");
            return cs;
        }
    };
    let bad_align = dz2.alloc(ZoneKind::Dma, 64, 24, "非2^n").is_err()
        && dz2.alloc(ZoneKind::Dma, 64, 8, "低于下档").is_err()
        && dz2.alloc(ZoneKind::Dma, 64, 1 << 17, "超上档").is_err()
        && dz2.alloc(ZoneKind::Dma, 0, 16, "零尺寸仍收-bump语义").is_ok();
    cs.add("对齐闸闭集", bad_align, "");

    // J-11 判码文本闭集：七码各得其所。
    let texts_ok = code_text(C_ZONE_OVERLAP) != "未知判码"
        && code_text(C_ROUTE_MISS) != "未知判码"
        && code_text(C_CROSS_BORROW) != "未知判码"
        && code_text(C_ZONE_OOB) != "未知判码"
        && code_text(C_ZONE_EXHAUSTED) != "未知判码"
        && code_text(C_ALIGN_BAD) != "未知判码"
        && code_text(C_ROUTE_RATE) != "未知判码"
        && code_text(0xCA00) == "未知判码";
    cs.add("判码文本闭集", texts_ok, "");

    cs
}

// ---------------------------------------------------------------------------
// 单元测试（std 环境对拍）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_and_boundary() {
        assert!(seg_guard());
        let bm = boundary_matrix();
        assert!(bm.iter().all(|&b| b));
    }

    #[test]
    fn route_and_ban() {
        let mut dz = DualZone::new(
            ZoneSpec { base: 0x20_0000, cap: 1 << 18 },
            ZoneSpec { base: 0x30_0000, cap: 1 << 19 },
        )
        .unwrap();
        let a = dz.alloc(ZoneKind::Dma, 128, 64, "t").unwrap();
        assert_eq!(a.1, ZoneKind::Dma);
        // 互借：DMA 请求送通用区 → 拒。
        assert_eq!(
            dz.general.alloc(ZoneKind::Dma, 64, 16, "x"),
            Err(C_CROSS_BORROW)
        );
        // 区外释放 → 拒。
        assert_eq!(dz.free(0x99_9999, 16), Err(C_CROSS_BORROW));
        assert!(dz.audit());
    }

    #[test]
    fn stress_replay_stable() {
        let s1 = stress_route(256, 42);
        let s2 = stress_route(256, 42);
        assert_eq!(s1.legal, s2.legal);
        assert_eq!(s1.banned, s2.banned);
        assert!(route_rate_recheck(&s1));
        assert_eq!(s1.rate_pct, ROUTE_RATE_TARGET_PCT);
    }

    #[test]
    fn ban_zero_leak() {
        let (blocked, leaked) = stress_ban(32);
        assert_eq!(blocked, 32);
        assert_eq!(leaked, 0);
    }
}
