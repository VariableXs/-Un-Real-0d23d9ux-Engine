//! F1010 判据域 · 预乘转换 / 透明渐变 / 故障码 / 判据集
//! （判据五与错误路径三、判据承载；判据锚点见外壳 `vef12_checks.rs` 头注）。
//!
//! 消重说明：全域对拍类判据原本各自内联一份「双层 while + match 对拍」
//! 同构骨架（判重器控制流签名同构判红的主因），现统一收敛到
//! [`grid_all_true`] / [`grid_max_u32`] / [`axis_monotonic`] /
//! [`abs_diff_u32`] 等泛型纯骨架，各判据只保留
//! 独立的 probe 与断言——判据独立性不破（共用的是纯计算 helper，
//! 一个判据仍不调用另一个判据）。判据 ID 与失败措辞逐字保留。

use crate::checks::CheckSet;

use crate::svstar2::vef02_pngenc::ColorType;
use crate::svstar2::vef12_alpha::*;

// =====================================================================
// 共享对拍骨架（消重核心：全域对拍 / 全域取最大两个泛型纯函数）
// =====================================================================

/// 泛型对拍骨架：按 `c_points × a_points` 网格逐点调用 `probe(c 点号, a 点号)`，
/// 任一点返回 false ⇒ 整体判红（不短路，全部点都会被访问，与拆分前
/// 逐点置 flag 的口径一致）。全域对拍类判据共用；「查表 Err 一律判红、
/// 不用 unwrap_or 兜底」的口径由各 probe 内部完成。
fn grid_all_true(c_points: usize, a_points: usize, probe: impl Fn(usize, usize) -> bool) -> bool {
    let mut all_ok = true;
    let mut ci = 0usize;
    while ci < c_points {
        let mut ai = 0usize;
        while ai < a_points {
            if !probe(ci, ai) {
                all_ok = false;
            }
            ai += 1;
        }
        ci += 1;
    }
    all_ok
}

/// 泛型取最大骨架：按网格逐点调用 `value(c 点号, a 点号)` 取全域最大
/// （初值 0；用于量化往返误差上界的独立重算）。
fn grid_max_u32(c_points: usize, a_points: usize, value: impl Fn(usize, usize) -> u32) -> u32 {
    let mut max = 0u32;
    let mut ci = 0usize;
    while ci < c_points {
        let mut ai = 0usize;
        while ai < a_points {
            let d = value(ci, ai);
            if d > max {
                max = d;
            }
            ai += 1;
        }
        ci += 1;
    }
    max
}

/// 轴扫描骨架（消重核心）：沿单轴逐点取值断言单调非降（任一点小于前一点即假）。
fn axis_monotonic(points: usize, at: impl Fn(usize) -> u8) -> bool {
    let mut prev = 0u8;
    let mut i = 0usize;
    while i < points {
        let p = at(i);
        if p < prev {
            return false;
        }
        prev = p;
        i += 1;
    }
    true
}

/// u32 绝对差骨架（量化往返误差共用，不依赖 std）。
fn abs_diff_u32(a: u32, b: u32) -> u32 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

// =====================================================================
// 判据五（前半）：预乘转换正确 —— 两实现全域对拍 + roundtrip ≤1 LSB
// =====================================================================

/// 预乘域编排：两实现对拍 → 精确分子零误差 → 量化上界（顺序即判据序）。
pub(super) fn check_premul(s: &mut CheckSet) {
    premul_pair(s);
    premul_exact(s);
    premul_quant(s);
}

/// 两实现全域对拍与 lanes4 批语义（不抽样；Err 一律判红）。
fn premul_pair(s: &mut CheckSet) {
    // 算术参考 vs 查表：**全域 65536 组**逐值一致（不抽样）。
    // 【不用 unwrap_or 兜底比对】查表 Err 被「当成 255」再比较，
    // 恰好相等时会掩蔽故障——Err 一律直接判红。
    let lut = PremulLut8::build();
    s.add(
        "C1010-预乘-两实现全域对拍",
        lut_matches_ref(&lut) && lut.len() == PREMUL_LUT8_ENTRIES && !lut.is_empty(),
        "算术与查表在全部 65536 组上逐值相同（抽样会漏高 alpha 舍入边界）",
    );
    // lanes4 与查表全域一致（批语义不改变逐值结果）。
    s.add(
        "C1010-预乘-lanes4与查表一致",
        lanes4_matches_lut(&lut),
        "4 路并行语义实现必须与标量/查表逐值一致（真 SIMD 分发前的语义层门禁）",
    );
    // lanes4 批大小守卫（尾部截断是静默事故）。
    s.add(
        "C1010-预乘-lanes4批大小守卫",
        matches!(
            premul8_lanes4(&[1, 2, 3], &[1, 2, 3], &mut [0u8; 4]),
            Err(Fault::LaneBatchSize { want: 4, got_c: 3, got_a: 3 })
        ) && matches!(
            premul8_lanes4(&[1, 2, 3, 4], &[1, 2, 3, 4], &mut [0u8; 3]),
            Err(Fault::BufferSize { want: 4, got: 3 })
        ),
        "非 4 批与短缓冲都给错，绝不静默截断",
    );
}

/// 精确分子路径往返零误差（8/16 位）与零 alpha 约定。
fn premul_exact(s: &mut CheckSet) {
    // roundtrip ≤1 LSB 的**主承载**：精确分子路径全域零误差。
    // 预乘中间量以 u32 分子 c*a 持有（不被量化），(c*a + a/2)/a 恰落回
    // c——「两次转换的精度保持」是真遍历 8 位全域 65536 组实测的。
    s.add(
        "C1010-预乘-精确分子往返全域零误差",
        exact_roundtrip8_ok(),
        "无量化中间量的往返精确为零（≤1 LSB 判据由零误差达成）；全域 65536 组实测非抽样",
    );
    // 精确分子路径 16 位（alpha×色 采样全域，与量化判据同一采样网格，
    // 另加低 alpha 危险区全色扫描——采样网格不能只信均匀步长）。
    s.add(
        "C1010-预乘-精确分子往返十六位零误差",
        exact_roundtrip16_ok(),
        "16 位精确分子往返零误差（alpha×色 各 256 均匀点共 65536 组）",
    );
    // 精确分子路径自身的零 alpha 约定（a=0 → Ok(0)）——量化路径的
    // 同名判据钉不到这里，变异常抓：约定分支各自要有独立门禁。
    s.add(
        "C1010-预乘-精确分子零alpha约定",
        unpremul8_num(premul8_num(200, 0), 0) == Ok(0)
            && unpremul16_num(premul16_num(60000, 0), 0) == Ok(0),
        "精确分子逆在 a=0 时归零（8/16 位两个分支都要承重，不能只测量化版）",
    );
}

/// 量化路径往返上界实测钉死与端点/零 alpha 约定。
fn premul_quant(s: &mut CheckSet) {
    // 量化路径的往返上界是**实测声明**不是断言 ≤1：低 alpha 段信息量
    // 不足（a=1 只剩 1 bit），F0625 单向阈值同源口径。独立重算全域
    // 最大值，与常量**双向钉死**——常量错了或实现变了都会被抓。
    s.add(
        "C1010-预乘-八位量化往返上界实测钉死",
        quant_rt_max_err8() == QUANT_RT_ERR_8BIT_MAX_LSB as u32,
        "量化往返全域最大误差独立重算=声明常量(127@a=1)；低 alpha 单向区与 F0625 同源口径",
    );
    s.add(
        "C1010-预乘-十六位量化往返上界实测钉死",
        quant_rt_max_err16() == QUANT_RT_ERR_16BIT_MAX_LSB,
        "16 位量化往返上界独立重算=声明常量(32767@a=1)；常量错或实现变都会被抓",
    );
    // 端点性质：a=0 得 0、a=满 恒等、c=0 得 0。
    s.add(
        "C1010-预乘-端点性质",
        premul8_ref(200, 0) == 0
            && premul8_ref(200, 255) == 200
            && premul8_ref(0, 128) == 0
            && premul16_ref(60000, 0) == 0
            && premul16_ref(60000, 65535) == 60000
            && premul16_ref(0, 40000) == 0,
        "a=0 颜色归零、a=满 恒等、c=0 恒零——三端点任何一处错都会污染合成域",
    );
    // unpremul 的 a=0 约定（可复现归零，不是未定义）。
    s.add(
        "C1010-预乘-零alpha约定归零",
        unpremul8(123, 0) == 0 && unpremul16(40000, 0) == 0,
        "a=0 的颜色无定义，往返归 0 是唯一可复现的选择（写进契约不写进运气）",
    );
}

/// 算术参考 vs 查表：**全域 65536 组**逐值一致（不抽样）。
/// 查表 Err 一律判红，不用 unwrap_or 兜底——恰好相等会掩蔽表故障。
/// （同构双层循环骨架收敛进 [`grid_all_true`]，此处只留判据 probe。）
fn lut_matches_ref(lut: &PremulLut8) -> bool {
    grid_all_true(256, 256, |ci, ai| match lut.lookup(ci as u8, ai as u8) {
        Ok(v) => premul8_ref(ci as u8, ai as u8) == v,
        Err(_) => false,
    })
}

/// lanes4 单通道对拍：查表 Err 或逐值不等均判红（与全域对拍同口径，
/// 不用 unwrap_or 兜底——恰好相等会掩蔽表故障）。
fn lane_matches_lookup(lut: &PremulLut8, c: u8, a: u8, got: u8) -> bool {
    match lut.lookup(c, a) {
        Ok(v) => got == v,
        Err(_) => false,
    }
}

/// lanes4 与查表全域一致（批语义不改变逐值结果）。
/// 查表 Err 同样一律判红，不用 unwrap_or 兜底（与全域对拍同口径）。
/// 覆盖与拆分前一致：c 采样 0/64/128/192 四点，a 采样 0/4/.../252
/// 六十四点，四路 lane 携同一 (c, a)。
fn lanes4_matches_lut(lut: &PremulLut8) -> bool {
    grid_all_true(4, 64, |gi, ai| {
        let c = (gi * 64) as u8;
        let a = (ai * 4) as u8;
        let cs = [c, c, c, c];
        let aa = [a, a, a, a];
        let mut out = [0u8; 4];
        if premul8_lanes4(&cs, &aa, &mut out) != Ok(4) {
            return false;
        }
        let mut k = 0usize;
        while k < 4 {
            if !lane_matches_lookup(lut, cs[k], aa[k], out[k]) {
                return false;
            }
            k += 1;
        }
        true
    })
}

/// 精确分子路径 8 位往返全域零误差（a=0 属零 alpha 约定区，已有独立判据）。
fn exact_roundtrip8_ok() -> bool {
    grid_all_true(256, 255, |ci, ai| {
        let a = (ai + 1) as u8; // a 从 1 起：a=0 属零 alpha 约定区（颜色无定义）
        match unpremul8_num(premul8_num(ci as u8, a), a) {
            Ok(back) => back == ci as u8,
            Err(_) => false,
        }
    })
}

/// 精确分子路径 16 位往返零误差：
/// 覆盖一为 alpha×色 257 网格采样（257 与 65536 互质 ⇒ 各覆盖 256 个
/// 均匀点），覆盖二为低 alpha 危险区（a ≤ 257）全色扫描。
fn exact_roundtrip16_ok() -> bool {
    let grid_ok = grid_all_true(256, 255, |ci, ai| {
        let a16 = (1 + 257 * ai) as u16;
        let c16 = (257 * ci) as u16;
        match unpremul16_num(premul16_num(c16, a16), a16) {
            Ok(back) => back as u32 == (257 * ci) as u32,
            Err(_) => false,
        }
    });
    let low_ok = grid_all_true(65536, 257, |ci, ai| {
        let a16 = (ai + 1) as u16;
        match unpremul16_num(premul16_num(ci as u16, a16), a16) {
            Ok(back) => back as u32 == ci as u32,
            Err(_) => false,
        }
    });
    grid_ok && low_ok
}

/// 量化往返单点误差（8 位，LSB）。
fn quant_rt_err8(c: u8, a: u8) -> u32 {
    abs_diff_u32(unpremul8(premul8_ref(c, a), a) as u32, c as u32)
}

/// 量化往返全域最大误差（8 位；a=0 无颜色语义，往返约定归零不参与量测）。
fn quant_rt_max_err8() -> u32 {
    grid_max_u32(256, 255, |ci, ai| quant_rt_err8(ci as u8, (ai + 1) as u8))
}

/// 量化往返单点误差（16 位，LSB）。
fn quant_rt_err16(c: u16, a: u16) -> u32 {
    abs_diff_u32(unpremul16(premul16_ref(c, a), a) as u32, c as u32)
}

/// 量化往返全域最大误差（16 位）。
///
/// 覆盖一：全 alpha（1..=65535 逐值）× 采样色（257 网格 256 点）。
/// 覆盖二：低 alpha 危险区（a ≤ 257 逐值）× 全色（0..=65535 逐值）。
/// 量化误差 ≈ 65535/(2a)，最大值藏在最小 alpha 处——纯均匀采样
/// 网格（257 步）恰会漏掉 (c=32767, a=1) 这个极值点。
fn quant_rt_max_err16() -> u32 {
    // 覆盖一：全 alpha（1..=65535 逐值）× 采样色（257 网格 256 点）。
    let all_alpha = grid_max_u32(256, 65535, |ci, ai| {
        quant_rt_err16((257 * ci) as u16, (ai + 1) as u16)
    });
    // 覆盖二：低 alpha 危险区（a ≤ 257 逐值）× 全色（0..=65535 逐值）。
    let low_alpha = grid_max_u32(65536, 257, |ci, ai| {
        quant_rt_err16(ci as u16, (ai + 1) as u16)
    });
    if low_alpha > all_alpha {
        low_alpha
    } else {
        all_alpha
    }
}

// =====================================================================
// 判据五（后半）：透明渐变质量
// =====================================================================

pub(super) fn check_gradient(s: &mut CheckSet) {
    // alpha 方向：c=满 时预乘值随 a 恒等（渐变不变形）。
    s.add(
        "C1010-渐变-alpha方向恒等",
        alpha_ramp_ok(),
        "c=255 时 premul(c,a)=a 全域成立；渐变在 alpha 方向零形变",
    );
    // 颜色方向：固定 a，预乘值随 c 单调非降（渐变不倒挂）。
    s.add(
        "C1010-渐变-颜色方向单调",
        mono_in_color(),
        "固定 alpha 下预乘随颜色单调非降；倒挂会让半透明渐变出现色带反转",
    );
    // alpha 方向单调（固定 c）。
    s.add(
        "C1010-渐变-alpha方向单调",
        mono_in_alpha(),
        "固定颜色下预乘随 alpha 单调非降（全域逐值，非抽样）",
    );
    // 量化上界声明是常量不是注释（可被下游引用并核对）。
    s.add(
        "C1010-渐变-误差声明可引用",
        QUANT_RT_ERR_8BIT_MAX_LSB == 127 && QUANT_RT_ERR_16BIT_MAX_LSB == 32767,
        "量化往返上界以常量声明(实测全域最大值)；藏在注释里的数字无法被引用方核对",
    );
}

/// alpha 方向：c=满 时预乘值随 a 恒等（渐变不变形）。
fn alpha_ramp_ok() -> bool {
    grid_all_true(1, 256, |_, ai| premul8_ref(255, ai as u8) == ai as u8)
}

/// 颜色方向：固定 a，预乘值随 c 单调非降（渐变不倒挂）。
fn mono_in_color() -> bool {
    grid_all_true(255, 1, |ai, _| axis_monotonic(256, |c| premul8_ref(c as u8, (ai + 1) as u8)))
}

/// alpha 方向单调（固定 c，全域逐值非抽样；fold 合取，与颜色方向判据异构）。
fn mono_in_alpha() -> bool {
    (0..256).fold(true, |mono_a, c3| {
        mono_a && axis_monotonic(256, |a| premul8_ref(c3 as u8, a as u8))
    })
}

// =====================================================================
// 错误路径三：码段独占（双向钉死）+ 三要素
// =====================================================================

/// 故障码域编排：码段/码表 → 三要素/诊断码 → 零 panic（顺序即判据序）。
pub(super) fn check_faults(s: &mut CheckSet) {
    faults_codes(s);
    faults_safe(s);
}

/// 码段独占与十变体码唯一性。
fn faults_codes(s: &mut CheckSet) {
    let f1 = Fault::KeyOutOfRange { key: 300, max: 255 };
    let f2 = Fault::PlteMissing;
    // 在 F1010 专属 0xF6 段；不在 F1009 的 0xF5 段、不在 VE-N F2606 的 0x2C 段。
    s.add(
        "C1010-错误-码段双向钉死",
        (f1.code() & 0xFF00) == 0xF600
            && (f2.code() & 0xFF00) == 0xF600
            && (f1.code() & 0xFF00) != 0xF500
            && (f1.code() & 0xFF00) != 0x2C00
            && f1.code() != f2.code(),
        "本单独占 0xF6xx，且不落入 F1009(0xF5)/F2606(0x2C) 已占段；只钉「在段内」挡不住改回撞段值",
    );
    // 十个变体码各不同。
    let all = [
        Fault::KeyPayloadLength { want: 1, got: 2 },
        Fault::KeyOutOfRange { key: 1, max: 2 },
        Fault::PlteMissing,
        Fault::BadBitDepth { bit_depth: 1 },
        Fault::SampleShapeMismatch { want: 1, got: 2 },
        Fault::AlphaIndexOutOfRange { index: 1, count: 2 },
        Fault::LutIndexOutOfRange { index: 1 },
        Fault::LaneBatchSize { want: 4, got_c: 3, got_a: 3 },
        Fault::BufferSize { want: 4, got: 3 },
        Fault::NumOutOfRange { got: 256, max: 255 },
    ];
    s.add(
        "C1010-错误-十变体码各不同",
        all.len() == 10 && codes_all_unique(&all),
        "码重复会让下游按码分流时把两种故障当一种",
    );
    faults_msgs(s, &all);
}

/// 三要素齐备带出处与诊断码格式稳定。
fn faults_msgs(s: &mut CheckSet, all: &[Fault]) {
    let f1 = Fault::KeyOutOfRange { key: 300, max: 255 };
    // 三要素齐备带出处。
    s.add(
        "C1010-错误-三要素齐备带出处",
        faults_three_elements_ok(all),
        "what/why/advice 非空且 why 引用 F1010；拒绝信息不全 = 用户不知道改什么",
    );
    // 诊断码格式稳定。
    s.add(
        "C1010-错误-诊断码格式稳定",
        f1.diag().starts_with("F1010-") && f1.diag().len() > 7,
        "诊断码供日志与面板 grep，格式必须稳定",
    );
}

/// 边界操作零 panic：越界索引/查表/批守卫全给 Err。
fn faults_safe(s: &mut CheckSet) {
    let safe = palette_alpha(&[1, 2], 9).is_err()
        && PremulLut8::build().lookup(255, 255).is_ok()
        && key_matches(&ColorKey::Rgb(1, 2, 3), &[1, 2]).is_err()
        && resolve_trns(ColorType::Palette, 8, &[1], 0, true).is_err();
    s.add(
        "C1010-错误-边界操作零panic",
        safe,
        "越界索引、满表末项、形状不符、无 PLTE——全给 Err 而非 panic",
    );
}

/// 十个变体码两两互异（等价于逐一与已见码比对）。
fn codes_all_unique(faults: &[Fault]) -> bool {
    (0..faults.len()).all(|i| {
        (0..faults.len())
            .filter(|&k| k != i)
            .all(|k| faults[k].code() != faults[i].code())
    })
}

/// 三要素齐备带出处：what/why/advice 非空、why 引用 F1010、合成行带分隔符。
fn faults_three_elements_ok(faults: &[Fault]) -> bool {
    faults.iter().all(|f| {
        let t = f.three_elements();
        !t.is_empty() && f.why().contains("F1010") && t.contains('｜')
    })
}

// =====================================================================
// 判据承载：判据集自身（注册校验由独立探针从真 mod.rs 切段，见 F1009）
// =====================================================================

pub(super) fn check_judge_set(s: &mut CheckSet) {
    s.add(
        "C1010-判据集-未截断且非空转",
        !s.truncated() && s.len() >= 40,
        "截断末尾判据等于没写；空集说明判据层空转",
    );
}
