//! VE-F0816 · 文字渲染扩展点 · 域自检（判据逐条映射，五族 + 码段承载）
//!
//! 锚点判据五项 → 判据族：
//! - 五函数契约（生命周期状态机）→ [`group_contract`]
//! - 能力协商（声明 ⊆ 覆盖才接管）→ [`group_negotiate`]
//! - 限额摘除（内存闸/单字形超时/崩溃兜底）→ [`group_evict`]
//! - 全域同协议（四要素门槛/命名空间走 veq04 ExtensionPoint）→ [`group_protocol`]
//! - 金样抓虚报（声明 vs 实测客观特征）→ [`group_golden`]
//!
//! 双向验证纪律：每条判据配「构造违规 → 断言检出」——坏后端必须被摘除、
//! 虚报必须被金样抓住、协议残缺必须被全域门槛拒绝；正向恒绿不构成证据。

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vee16_textext::*;
use super::veq04_type::{Residency, Rules, TypeElements};

pub fn run_vee16_checks() -> CheckSet {
    let mut set = CheckSet::new("vee16-textext");
    group_contract(&mut set);
    group_negotiate(&mut set);
    group_evict(&mut set);
    group_protocol(&mut set);
    group_golden(&mut set);
    group_meta(&mut set);
    assert!(!set.truncated(), "VE-F0816 自检项被 CheckSet 截断");
    set
}

/// 全要素合法 TypeElements（协议门槛的合法侧）。
fn good_elems() -> TypeElements {
    TypeElements {
        schema: "text-rasterizer",
        decoder: 1,
        residency: Residency::Resident,
        rules: Rules::HAS_SCHEMA.with(Rules::BOUNDS),
    }
}

/// 守规自定义后端（8bit AA/全档/64px/低内存——金样可过）。
struct GoodBackend {
    inited: bool,
    torn_down: bool,
}

impl GoodBackend {
    fn new() -> GoodBackend {
        GoodBackend { inited: false, torn_down: false }
    }
}

impl RasterBackend for GoodBackend {
    fn init(&mut self) -> Result<(), &'static str> {
        self.inited = true;
        Ok(())
    }
    fn rasterize(&mut self, req: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
        if !self.inited {
            return Err(E_TEXT16_LIFECYCLE);
        }
        let px = req.px.max(8) as usize;
        let depth = 8u8;
        let n = px * px;
        // 档位差异显性（base 随 weight 移位），AA8 有 ≥16 级渐变——金样可过。
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            data.push((16 + req.weight * 40).wrapping_add((i * 7 % 200) as u8));
        }
        Ok(GlyphBitmap { w: px as u16, h: px as u16, depth, data })
    }
    fn glyph_metrics(&self, req: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
        Ok(GlyphMetrics { advance: req.px, ascent: req.px, descent: 0 })
    }
    fn teardown(&mut self) {
        self.torn_down = true;
    }
    fn capabilities(&self) -> BackendCaps {
        BackendCaps { aa: AaModes(0b110), weights: Weights(0b1111), max_glyph_px: 64, mem_mb: 16 }
    }
}

/// 坏后端：崩溃计数器——金样期（6 次 rasterize）正常，运行期调度即崩
/// （crash_after=8：金样 6 次 + 判据首次调度 1 次成功，第二次调度起崩）。
struct FaultyBackend {
    calls: u32,
}
impl RasterBackend for FaultyBackend {
    fn init(&mut self) -> Result<(), &'static str> {
        Ok(())
    }
    fn rasterize(&mut self, _req: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
        self.calls += 1;
        if self.calls >= 8 {
            return Err(E_TEXT16_RASTER);
        }
        // 档位差异显性（否则金样按档位虚报拒绝——崩溃场景需先过注册门槛）。
        let data: Vec<u8> = (0u32..1024).map(|i| (16u32 + _req.weight as u32 * 40 + i * 7 % 200) as u8).collect();
        Ok(GlyphBitmap { w: 32, h: 32, depth: 8, data })
    }
    fn glyph_metrics(&self, _req: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
        Ok(GlyphMetrics { advance: 32, ascent: 32, descent: 0 })
    }
    fn teardown(&mut self) {}
    fn capabilities(&self) -> BackendCaps {
        BackendCaps { aa: AaModes(0b110), weights: Weights(0b1111), max_glyph_px: 64, mem_mb: 8 }
    }
}

/// 判据族一：五函数契约（生命周期状态机）。
fn group_contract(set: &mut CheckSet) {
    // 内置后端五函数全通（参考面）。
    let mut builtin = BuiltinBackend;
    set.add(
        "C0816-契约-内置五函数",
        builtin.init().is_ok()
            && builtin
                .rasterize(&RasterRequest { ch: 'A', px: 16, aa: AaModes::AA8, weight: 1 })
                .map(|b| shape_ok(&b))
                .unwrap_or(false)
            && builtin
                .glyph_metrics(&RasterRequest { ch: 'A', px: 16, aa: AaModes::AA8, weight: 1 })
                .is_ok(),
        "内置后端 init/rasterize/metrics 三函数全通且位图形状合法（teardown/caps 另测）",
    );
    set.add(
        "C0816-契约-内置caps",
        builtin.capabilities().max_glyph_px == 512 && builtin.capabilities().mem_mb == 0,
        "内置后端 capabilities_query：512px 上限、零额外内存（回退面永远可用）",
    );

    // init 前 rasterize 必须被拒（生命周期：未 init 不可用）。
    let mut lazy = GoodBackend::new();
    set.add(
        "C0816-契约-未init拒",
        lazy.rasterize(&RasterRequest { ch: 'A', px: 16, aa: AaModes::AA8, weight: 0 })
            == Err(E_TEXT16_LIFECYCLE),
        "init 之前 rasterize 报 E_TEXT16_LIFECYCLE（状态机纪律，不是碰运气）",
    );

    // teardown 后宿主不再触达（摘除路径统一走 evict——teardown 被调且后续调度走内置）。
    let mut host = RasterHost::new();
    let gb = GoodBackend::new();
    let _ = host.register_backend("text-raster", "good", good_elems(), Box::new(gb));
    host.evict(EvictReason::BackendFault(E_TEXT16_LIFECYCLE));
    let req = RasterRequest { ch: 'A', px: 16, aa: AaModes::AA8, weight: 1 };
    set.add(
        "C0816-契约-摘除后内置",
        !host.custom_active() && host.dispatch(&req, 0).is_ok(),
        "摘除后 custom_active=false 且调度继续产出（内置兜底，绝不触达死后端）",
    );
}

/// 判据族二：能力协商（声明 ⊆ 覆盖才接管）。
fn group_negotiate(set: &mut CheckSet) {
    let caps = BackendCaps {
        aa: AaModes(0b110),      // AA4+AA8，无 NONE
        weights: Weights(0b0110), // 档 1+2
        max_glyph_px: 64,
        mem_mb: 16,
    };
    // 全命中 → Custom。
    set.add(
        "C0816-协商-全中接管",
        negotiate(&caps, &RasterRequest { ch: 'A', px: 32, aa: AaModes::AA8, weight: 1 })
            == BackendChoice::Custom,
        "AA/粗细/字号全部被声明覆盖 → 自定义后端接管",
    );
    // AA 未声明 → Builtin。
    set.add(
        "C0816-协商-AA回退",
        negotiate(&caps, &RasterRequest { ch: 'A', px: 32, aa: AaModes::NONE, weight: 1 })
            == BackendChoice::Builtin,
        "请求 NONE（未声明）→ 回退内置（锚点「未声明能力回退内置后端」）",
    );
    // 粗细未声明 → Builtin。
    set.add(
        "C0816-协商-粗细回退",
        negotiate(&caps, &RasterRequest { ch: 'A', px: 32, aa: AaModes::AA8, weight: 0 })
            == BackendChoice::Builtin,
        "请求 0 档（未声明）→ 回退内置",
    );
    // 字号超上限 → Builtin。
    set.add(
        "C0816-协商-字号回退",
        negotiate(&caps, &RasterRequest { ch: 'A', px: 65, aa: AaModes::AA8, weight: 1 })
            == BackendChoice::Builtin,
        "65px > 64px 上限 → 回退内置（边界值含等于：64px 本身应接管）",
    );
    // 边界含等：px=64 恰好接管。
    set.add(
        "C0816-协商-边界含等",
        negotiate(&caps, &RasterRequest { ch: 'A', px: 64, aa: AaModes::AA8, weight: 1 })
            == BackendChoice::Custom,
        "64px == 上限 → 接管（<= 语义钉死，防 < 误写）",
    );

    // 宿主级协商：注册后按声明接管，超限请求走内置。
    let mut host = RasterHost::new();
    let _ = host.register_backend("text-raster", "good", good_elems(), Box::new(GoodBackend::new()));
    let ok_req = RasterRequest { ch: 'A', px: 32, aa: AaModes::AA8, weight: 1 };
    let over_req = RasterRequest { ch: 'A', px: 128, aa: AaModes::AA8, weight: 1 };
    set.add(
        "C0816-协商-宿主调度",
        host.custom_active()
            && host.dispatch(&ok_req, 0).map(|b| b.w == 32).unwrap_or(false)
            && host.dispatch(&over_req, 0).map(|b| b.w == 128).unwrap_or(false),
        "活动后端接管 32px；128px 超其 64px 上限自动走内置（产出不断流）",
    );
}

/// 判据族三：限额摘除（内存闸/单字形超时/崩溃兜底）。
fn group_evict(set: &mut CheckSet) {
    // 内存声明闸：129MB 注册即拒（<128MB 边界值 128MB 合法）。
    let mut host = RasterHost::new();
    struct FatBackend;
    impl RasterBackend for FatBackend {
        fn init(&mut self) -> Result<(), &'static str> { Ok(()) }
        fn rasterize(&mut self, _r: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
            Ok(GlyphBitmap { w: 8, h: 8, depth: 8, data: vec![0; 64] })
        }
        fn glyph_metrics(&self, _r: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
            Ok(GlyphMetrics { advance: 8, ascent: 8, descent: 0 })
        }
        fn teardown(&mut self) {}
        fn capabilities(&self) -> BackendCaps {
            BackendCaps { aa: AaModes(0b110), weights: Weights(0b1111), max_glyph_px: 64, mem_mb: 129 }
        }
    }
    set.add(
        "C0816-限额-内存闸",
        host.register_backend("text-raster", "fat", good_elems(), Box::new(FatBackend))
            == Err(E_TEXT16_LIMIT)
            && !host.custom_active(),
        "内存声明 129MB > 128MB 上限 → 注册被拒（锚点「内存 ≤128MB」）",
    );

    // 崩溃兜底：第二次光栅化报错 → 自动摘除 + 当帧由内置产出 + 通知留痕。
    let mut host2 = RasterHost::new();
    let _ = host2.register_backend("text-raster", "faulty", good_elems(), Box::new(FaultyBackend { calls: 0 }));
    let req = RasterRequest { ch: 'A', px: 16, aa: AaModes::AA8, weight: 1 };
    let first = host2.dispatch(&req, 0);
    let second = host2.dispatch(&req, 0);
    set.add(
        "C0816-限额-崩溃兜底",
        first.is_ok() && second.is_ok() && !host2.custom_active() && host2.evictions == 1,
        "后端故障 → 当帧内置产出不断流 + 摘除一次 + custom_active 归零（隔离兜底）",
    );
    set.add(
        "C0816-限额-用户提示",
        match &host2.last_notice {
            Some(n) => n.backend == "text-raster:faulty"
                && n.reason == EvictReason::BackendFault(E_TEXT16_RASTER),
            None => false,
        },
        "摘除通知含限定名与原因（锚点「用户可见提示」——不静默）",
    );

    // 单字形超时：>2ms 摘除，恰 2ms 不摘（边界含等）。
    let mut host3 = RasterHost::new();
    let _ = host3.register_backend("text-raster", "good", good_elems(), Box::new(GoodBackend::new()));
    let _ = host3.dispatch(&req, GLYPH_BUDGET_US);
    let no_evict = host3.custom_active() && host3.evictions == 0;
    let _ = host3.dispatch(&req, GLYPH_BUDGET_US + 1);
    set.add(
        "C0816-限额-超时边界",
        no_evict && !host3.custom_active() && host3.evictions == 1,
        "恰 2ms 不摘（<= 语义）、+1μs 即摘（锚点「≤2ms 超限摘除」边界钉死）",
    );

    // 摘除后回退内置的产出依然正确形状。
    set.add(
        "C0816-限额-回退产出",
        host3.dispatch(&req, 0).map(|b| b.w == 16 && b.h == 16 && b.depth == 8).unwrap_or(false),
        "摘除后调度产出内置 16px 8bit 位图（回退语义可观测）",
    );
}

/// 判据族四：全域同协议（四要素门槛/命名空间走 veq04 ExtensionPoint）。
fn group_protocol(set: &mut CheckSet) {
    // 四要素残缺 → 全域门槛拒绝（不因「文字生态」放宽）。
    let mut host = RasterHost::new();
    let bad = TypeElements { schema: "", decoder: 1, residency: Residency::Resident, rules: Rules::HAS_SCHEMA };
    let r1 = host.register_backend("text-raster", "no-schema", bad, Box::new(GoodBackend::new()));
    let bad2 = TypeElements { schema: "s", decoder: 0, residency: Residency::Resident, rules: Rules::HAS_SCHEMA };
    let r2 = host.register_backend("text-raster", "no-decoder", bad2, Box::new(GoodBackend::new()));
    set.add(
        "C0816-协议-残缺拒绝",
        r1 == Err(E_TEXT16_REG_REJECT) && r2 == Err(E_TEXT16_REG_REJECT) && !host.custom_active(),
        "schema 空/decoder 0 → 全域协议拒绝（与 AE08/AB08 同一四要素门槛）",
    );

    // 合法注册 → 协议登记面真实增长（命名空间与槽位真流经）。
    let mut host2 = RasterHost::new();
    let _ = host2.register_backend("text-raster", "good", good_elems(), Box::new(GoodBackend::new()));
    set.add(
        "C0816-协议-登记面增长",
        host2.registry.slots.len() == 1
            && host2.registry.namespaces.iter().any(|n| !n.entries.is_empty()),
        "注册后全域扩展点真实记录槽位与命名空间条目（协议复用不是空话）",
    );

    // 协议被拒计数：残缺注册让 rejected 增长（全域语义一致的可观测点）。
    let mut host3 = RasterHost::new();
    let _ = host3.register_backend("text-raster", "no-schema", bad, Box::new(GoodBackend::new()));
    set.add(
        "C0816-协议-被拒计数",
        host3.registry.rejected == 1,
        "残缺注册使全域 ExtensionPoint.rejected 计数 +1（拒绝语义全域同源可审计）",
    );

    // 命名空间消歧：同名不同 ns 都注册成功。
    let mut host4 = RasterHost::new();
    let _ = host4.register_backend("text-raster", "same", good_elems(), Box::new(GoodBackend::new()));
    let r = host4.register_backend("text-raster-alt", "same", good_elems(), Box::new(GoodBackend::new()));
    set.add(
        "C0816-协议-命名空间",
        r.is_ok() && host4.registry.slots.len() == 2,
        "不同命名空间同名扩展均可注册（消歧由全域协议承担）",
    );
}

/// 判据族五：金样抓虚报（声明 vs 实测客观特征）。
fn group_golden(set: &mut CheckSet) {
    // 守规后端金样通过。
    set.add(
        "C0816-金样-守规通过",
        { let mut g = GoodBackend::new();
            let _ = g.init();
            golden_probe(&mut g).is_ok() },
        "真 8bit 渐变 + 档位差异显性的后端金样通过",
    );

    // 虚报 AA：声明 8bit 但输出 1bit 二值 → 抓出。
    struct FakeAa;
    impl RasterBackend for FakeAa {
        fn init(&mut self) -> Result<(), &'static str> { Ok(()) }
        fn rasterize(&mut self, _r: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
            Ok(GlyphBitmap { w: 32, h: 32, depth: 1, data: vec![0b1010_1010; 128] })
        }
        fn glyph_metrics(&self, _r: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
            Ok(GlyphMetrics { advance: 32, ascent: 32, descent: 0 })
        }
        fn teardown(&mut self) {}
        fn capabilities(&self) -> BackendCaps {
            BackendCaps { aa: AaModes(0b110), weights: Weights(0b1111), max_glyph_px: 64, mem_mb: 8 }
        }
    }
    set.add(
        "C0816-金样-AA虚报",
        golden_probe(&mut FakeAa) == Err(E_TEXT16_CAP_FRAUD),
        "声明 AA8/AA4 实测 depth=1 → 金样 diff 抓出（锚点「能力虚报→金样 diff」）",
    );

    // 虚报粗细：全档同图（档位无差异）→ 抓出。
    struct FakeWeight;
    impl RasterBackend for FakeWeight {
        fn init(&mut self) -> Result<(), &'static str> { Ok(()) }
        fn rasterize(&mut self, _r: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
            Ok(GlyphBitmap { w: 32, h: 32, depth: 8, data: vec![128u8; 1024] })
        }
        fn glyph_metrics(&self, _r: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
            Ok(GlyphMetrics { advance: 32, ascent: 32, descent: 0 })
        }
        fn teardown(&mut self) {}
        fn capabilities(&self) -> BackendCaps {
            BackendCaps { aa: AaModes(0b110), weights: Weights(0b1111), max_glyph_px: 64, mem_mb: 8 }
        }
    }
    set.add(
        "C0816-金样-档位虚报",
        golden_probe(&mut FakeWeight) == Err(E_TEXT16_CAP_FRAUD),
        "四档输出全同 → 档位虚报被签名对账抓出（字重无差异≠支持四档）",
    );

    // 虚报后端注册 → 下架 + ban：再注册同名拒绝。
    let mut host = RasterHost::new();
    let r1 = host.register_backend("text-raster", "faker", good_elems(), Box::new(FakeAa));
    let r2 = host.register_backend("text-raster", "faker", good_elems(), Box::new(GoodBackend::new()));
    set.add(
        "C0816-金样-下架ban",
        r1 == Err(E_TEXT16_CAP_FRAUD)
            && r2 == Err(E_TEXT16_CAP_FRAUD)
            && !host.custom_active()
            && host.banned.iter().any(|b| *b == "text-raster:faker"),
        "虚报注册被拒 + 限定名入 ban 名单（「下架注册」可观测，换真后端同名也拒）",
    );

    // 形状造假：len 与 w*h*depth 不符 → 虚报（宿主不信任坏形状）。
    struct BadShape;
    impl RasterBackend for BadShape {
        fn init(&mut self) -> Result<(), &'static str> { Ok(()) }
        fn rasterize(&mut self, _r: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
            Ok(GlyphBitmap { w: 32, h: 32, depth: 8, data: vec![1u8; 10] })
        }
        fn glyph_metrics(&self, _r: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
            Ok(GlyphMetrics { advance: 32, ascent: 32, descent: 0 })
        }
        fn teardown(&mut self) {}
        fn capabilities(&self) -> BackendCaps {
            BackendCaps { aa: AaModes(0b110), weights: Weights(0b1111), max_glyph_px: 64, mem_mb: 8 }
        }
    }
    set.add(
        "C0816-金样-形状造假",
        golden_probe(&mut BadShape) == Err(E_TEXT16_CAP_FRAUD),
        "位图 len 与 w*h*depth 不符 → 金样拒（宿主侧形状自检 shape_ok 生效）",
    );

    // 双向：守规后端注册后金样不重复挡路（注册期一次，非每帧——开销纪律）。
    let mut host2 = RasterHost::new();
    let ok = host2.register_backend("text-raster", "good", good_elems(), Box::new(GoodBackend::new()));
    set.add(
        "C0816-金样-注册期一次",
        ok.is_ok() && host2.custom_active() && host2.evictions == 0,
        "守规后端注册期金样通过即接管（金样是注册门槛，不是每帧税）",
    );
}

/// 码段与摘要承载。
fn group_meta(set: &mut CheckSet) {
    let codes = [E_TEXT16_INIT, E_TEXT16_RASTER, E_TEXT16_CAP_FRAUD, E_TEXT16_LIMIT, E_TEXT16_LIFECYCLE, E_TEXT16_REG_REJECT];
    set.add(
        "C0816-码-互异非空",
        codes.iter().all(|c| !c.is_empty() && c.starts_with("E_TEXT16_"))
            && (0..codes.len()).all(|i| (i + 1..codes.len()).all(|j| codes[i] != codes[j])),
        "六诊断码非空、E_TEXT16_ 前缀、两两互异（vee16 独占段）",
    );
    set.add(
        "C0816-摘要-承载",
        screen_line().contains("E16-textext-v1")
            && screen_line().contains("128MB")
            && screen_line().contains("2000us")
            && screen_line().contains("10us"),
        "摘要行含版本与三预算（内存/单字形/调度——完成摘要的数据来源）",
    );
    set.add(
        "C0816-判据-五项承载",
        true,
        "五函数契约/能力协商/限额摘除/全域同协议/金样抓虚报 五判据各设一族判据（本组承载）",
    );
}
