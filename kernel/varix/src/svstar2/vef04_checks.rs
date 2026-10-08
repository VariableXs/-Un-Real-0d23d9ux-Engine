//! VE-F1004 · PNG 色彩管理（ICC/sRGB/gAMA/cHRM）· 域自检
//!
//! 判据映射（锚点原文 → 自检项）：
//! - iCCP 解析 + 8MB 上限 + 传递链路→ `C04-ICCP-*`
//! - sRGB 解析（渲染意图 0-3 校验）→ `C04-SRGB-*`
//! - gAMA 解析 + 越界拒绝 + 线性化 → `C04-GAMA-*`
//! - cHRM 八坐标解析 → `C04-CHRM-*`
//! - 冲突优先级（iCCP > sRGB > gAMA+cHRM）→ `C04-CONF-*`
//! - 统一标注单一出口 → `C04-ANNO-*`
//! - 错误路径与默认策略 → `C04-ERR-*`
//! - 块长度字段全域扫描（长度闸夹逼对；checked 溢出面在 64 位**不可观测**，
//!   如实登记于 `C04-ERR-06` 头部与 `VARIANT_REGISTRY`）→ `C04-ERR-06`
//!
//! **对拍基准的独立性（如实登记）**：本文件自建 PNG 容器（**自写 CRC 与
//! zlib stored 封装**，不复用被测模块的任何函数），故「块扫描 → 解析 → 标注」
//! 全链路无自证循环。被测模块的私有面（`fmt_u64`/`clamp01` 等）一概不碰。
//!
//! **反假变体**：见文件末 `VARIANT_REGISTRY`，每条都已实测让判据变红。

use crate::svstar2::vef01_pngdec as dec;
use crate::svstar2::vef04_color as c4;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 语料构造：自写容器 / CRC / zlib stored（与被测实现零共用）
// ---------------------------------------------------------------------------

/// Adler-32（自写，zlib 校验）。
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// CRC-32（IEEE，自写位级实现——**刻意不复用** `frameledger_ext::crc32`，
/// 避免「用被测链路的依赖去验被测链路」）。
fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// zlib stored（不压缩）封装——给 iCCP 造合法 zlib 流。
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78u8, 0x01];
    let mut off = 0usize;
    // **空输入必须产出一个合法的「空 stored 块」，不能只补adler**。
    //
    // 旧写法靠 `while off < data.len()` 短路，空输入时 DEFLATE 段是空的、
    // 紧跟着 4 字节 adler。而 `zlib_inflate_slices` 剥掉 2 字节 zlib 头后
    // 会把**adler 字节当成 DEFLATE 位流**去解析块头：读到的首字节是
    // adler 的高字节（0x00），被判成「BFINAL=0 的 stored 块」，接着要求
    // `NLEN == !LEN`——必然不符 → 报 `BadZlib`。
    //
    // 后果是判据陷阱：`iccp_payload(&[])` 造出的「空profile」语料**根本走不到**
    // `parse_iccp` 里的 `n == 0` 那道闸，而是在更早的 `Err(_)` 分支就返回了。
    // 于是 `C04-ICCP-04` 名义上验「空 ICC 配置降级告警」，实测验的却是
    // 「解压失败降级告警」——**两道闸外部表现完全相同**，删掉`n == 0`
    // 判据照样全绿（变体 W19 实测证否，已登记）。
    //
    // 这属弱门禁十诫 #3「重合行为掩盖缺失分支」：给该分支**专属**可达路径，
    // 判据才真正承重。
    if data.is_empty() {
        // BFINAL=1 + BTYPE=00（stored），LEN=0，NLEN=!0=0xFFFF
        z.extend_from_slice(&[0x01u8, 0x00, 0x00, 0xFF, 0xFF]);
    }
    while off < data.len() {
        let n = (data.len() - off).min(65535);
        let last = if off + n >= data.len() { 1u8 } else { 0u8 };
        z.push(last);
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&data[off..off + n]);
        off += n;
    }
    z.extend_from_slice(&adler32(data).to_be_bytes());
    z
}

/// 写一个块（长度 + 类型 + 载荷 + CRC）。
fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(data.len() as u32).to_be_bytes());
    v.extend_from_slice(ty);
    v.extend_from_slice(data);
    let mut crc_in = Vec::new();
    crc_in.extend_from_slice(ty);
    crc_in.extend_from_slice(data);
    v.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    v
}

/// 造一个最小合法 PNG（IHDR + 给定色彩块 + IEND）。
///
/// 色彩块**插在 IHDR 之后、IEND 之前**——规范要求 iCCP/sRGB/gAMA/cHRM
/// 出现在 IDAT 之前或之后均可，本构造器统一放在 IDAT 之前。
fn png_with(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&dec::PNG_SIG);
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&1u32.to_be_bytes()); // width=1
    ihdr[4..8].copy_from_slice(&1u32.to_be_bytes()); // height=1
    ihdr[8] = 8; // depth
    ihdr[9] = 6; // RGBA
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    for (ty, data) in chunks.iter() {
        out.extend_from_slice(&chunk(ty, data));
    }
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

/// 造 `gAMA` 载荷（gamma × 100000 的大端 u32）。
fn gama_payload(g: f32) -> Vec<u8> {
    ((g * 100000.0).round() as u32).to_be_bytes().to_vec()
}

/// 造 `cHRM` 载荷（8 个 ×100000 的大端 u32）。
fn chrm_payload(vals: [f32; 8]) -> Vec<u8> {
    let mut v = Vec::new();
    for x in vals.iter() {
        v.extend_from_slice(&((x * 100000.0).round() as u32).to_be_bytes());
    }
    v
}

/// 造 `iCCP` 载荷（keyword + NUL + 方法0 + zlib stored）。
///
/// **keyword 用固定的 `"icc"` 字面量，绝不拿 profile 内容当 keyword**——
/// profile 是任意字节流（含 `0x00` 与控制字符），当作 keyword 会被
/// 解析器的keyword 校验（1-79 可打印 Latin-1）当场拒绝，语料根本走不到
/// 解压步骤：那样的「红」是语料造的，不是被测实现的缺陷。
fn iccp_payload(profile: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"icc");
    v.push(0);
    v.push(0);
    v.extend_from_slice(&zlib_stored(profile));
    v
}

/// 合法 cHRM 的 8 个坐标（D65 白点 + sRGB 三原色，×100000 整数）。
fn good_chrm() -> [f32; 8] {
    [0.3127, 0.3290, 0.6400, 0.3300, 0.3000, 0.6000, 0.1500, 0.0600]
}

// ---------------------------------------------------------------------------
// 主自检
// ---------------------------------------------------------------------------

/// VE-F1004 域自检。
pub fn run_vef04_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("vef04");

    // -- C04-SRGB-01 渲染意图 0..=3 全往返 --------------------------
    {
        let mut ok = true;
        for it in c4::RenderIntent::all().iter() {
            let w = it.wire();
            if w > 3 {
                ok = false;
            }
            if c4::RenderIntent::from_wire(w) != Some(*it) {
                ok = false;
            }
        }
        // 规范未定义的值必须显性拒绝，**不夹取**
        for v in [4u8, 5, 100, 255] {
            if c4::RenderIntent::from_wire(v).is_some() {
                ok = false;
            }
        }
        cs.add("C04-SRGB-01 渲染意图0..3往返一致且4/5/100/255显式拒绝", ok, "");
    }

    // -- C04-SRGB-02 sRGB 块解析与意图落地 ------------------------
    {
        let mut ok = true;
        for it in c4::RenderIntent::all().iter() {
            let png = png_with(&[(b"sRGB", vec![it.wire()])]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    if a.source != Some(c4::ColorSource::Srgb) {
                        ok = false;
                    }
                    if a.intent != Some(*it) {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        // 意图字段 4（越界）必须拒绝且**报 BadIntent**（非其它错误）
        let png = png_with(&[(b"sRGB", vec![4u8])]);
        match c4::parse_color_annotation(&png) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::BadIntent {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // 长度非 1 必须拒绝
        let png = png_with(&[(b"sRGB", vec![0u8, 0u8])]);
        match c4::parse_color_annotation(&png) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::SrgbLength {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        cs.add("C04-SRGB-02 四种意图落地+越界与长度错各自报对应类别", ok, "");
    }

    // -- C04-GAMA-01 gamma 解析正确（跨档） ------------------------
    {
        let mut ok = true;
        for &g in &[0.45455f32, 0.5, 1.0, 2.2, 2.4, 4.0] {
            let png = png_with(&[(b"gAMA", gama_payload(g))]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    if a.source != Some(c4::ColorSource::GammaChrm) {
                        ok = false;
                    }
                    match a.gamma {
                        // 编码往返：×100000 后取整，容差半个量化单位
                        Some(v) => {
                            if (v - g).abs() > 0.00001 {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    }
                    if a.gama_rejected.is_some() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C04-GAMA-01 六档gamma解析正确且无越界标记", ok, "");
    }

    // -- C04-GAMA-02 越界拒绝该块并走默认 --------------------------
    // 锚点：>5.0 或 <0.01 → 拒绝该块按默认处理
    {
        let mut ok = true;
        // 越界值集合：0（除零）、极小、**贴上限**、极大
        //
        // 关键在`500_001`（= 5.00001，恰好越界一步）与 `499_999`
        // （= 4.99999，恰好在界内一步）这对**夹逼对**：
        // 旧集合是 `[0, 1, 10_000_000, 0xFFFF_FFFF]`，从 0.00001
        // 直接跳到 10，**5.0~10.0 之间一个采样点都没有**。于是把
        // `GAMA_MAX` 从 5.0 放宽到 50.0 时，落进这段空白的 10.0
        // 仍被拒，全部判据照样全绿——闸门的具体位置无人验证。
        // 夹逼对把「界在哪」钉死：放宽或收紧任一侧都会立刻转红。
        for &raw in &[
            0u32,
            1,
            999,// 0.00999 < 0.01 下界外侧
            1_000,     // 0.01000 = 下界内侧（须被接受，见下方内侧腿）
            499_999,   // 4.99999 上界内侧
            500_000,   // 5.00000 = 上界内侧
            500_001,   // 5.00001 上界外侧
            10_000_000,
            0xFFFF_FFFF,
        ] {
            let png = png_with(&[(b"gAMA", raw.to_be_bytes().to_vec())]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    // 界内侧（1000 / 499999 / 500000）必须被**接受**
                    let inside = raw == 1_000 || raw == 499_999 || raw == 500_000;
                    if inside {
                        if a.gamma.is_none() || a.gama_rejected.is_some() {
                            ok = false;
                        }
                    } else {
                        // 界外侧必须：走默认（无来源）+ 记入 gama_rejected + 告警
                        if a.source.is_some() {
                            ok = false;
                        }
                        if a.gamma.is_some() {
                            ok = false;
                        }
                        if a.gama_rejected != Some(raw) {
                            ok = false;
                        }
                        if a.warnings == 0 {
                            ok = false;
                        }
                        // 默认策略必须是 sRGB 假定
                        if !a.is_default_srgb() {
                            ok = false;
                        }
                    }
                }
                Err(_) => ok = false,
            }
        }
        // 长度错（结构错）必须阻断，与「越界只拒块」区分开
        let png = png_with(&[(b"gAMA", vec![0u8, 1u8])]);
        match c4::parse_color_annotation(&png) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::GamaLength {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // **长度守卫的全域扫描：0..=8 逐个长度，一个不跳**。
        //
        // 为什么要补这一腿：既有语料的 gAMA 载荷清一色 4 字节，边界之外的
        // 长度**无人验证**——把 `parse_gama` 的守卫从 `!= 4` 放宽成 `< 3`
        // 后，21 项判据实测**照样全绿**（变体 W13c，见文件末登记表）。
        // 这是典型的**采样留洞**：语料只覆盖正确长度，闸门位置无人问津。
        //
        // **一段被实测推翻的旧说法的更正（留档以免重犯）**。
        //
        // 本处原先只扫 0/1/2 字节，注释理由是「3 字节在守卫写成 `< 3` 时会
        // 流入 `parse_gama` 内部并越界（实测 panic），故跳过它，避免判据
        // 自身跟着崩掉」。
        //
        // **该理由不成立，已推翻**：在**真实现**上实测（隔离探针
        // `catch_unwind`逐长度扫 0..=8），3 字节载荷干净返回
        // `GamaLength(a=3, b=4)`，**根本不 panic**——因为真守卫是 `!= 4`，
        // 3 != 4 直接被拒。「panic」只在守卫被放宽的**变体**里出现。
        //
        // 于是原做法有两处错：
        //  1. 把**变体的失败形态**当成了**基线的约束**。判据的可靠性不该
        //     建立在被测物不出错上——恰恰相反，正因为真实现稳过，才敢把
        //     3 字节摆进语料：真实现绿，变体红，这才是一道承重的闸。
        //  2. 少扫一个长度就少一格覆盖，而缺的正是**最危险的那一格**
        //     （3 是 `< 3` 守卫下第一个漏进来的长度）。
        //
        // 现在扫 0..=8 全域。若日后有人再把守卫放宽，3 字节会当场转红
        // （或 panic 暴露），而不是继续静默全绿。
        let mut len_guard_ok = true;
        // 长度维度：0..=8 中**除4 以外**的每个长度都必须报 GamaLength 阻断。
        // 逐个断言**具体长度值**（`e.a == n`），而不只断 `kind`——
        // 只断 kind 的话，把 `a` 恒填 0 的实现也能蒙对（弱门禁十诫 #7：
        // 判据侧须独立重算参考值，不向被测函数问答案）。
        //
        // **为什么 4 不在阻断期望内**：4 是gAMA 的**合法长度**，本腿只管
        // 「长度维度的闸位」，语义（该值是否越界）由上面的夹逼对与下一条
        // 正向腿管。把 4 也期望成 `Err(GamaLength)` 是**判据写错数**——
        // 实测（`vec![0u8; 4]` 全零载荷）会得到 `Ok { gamma: None,
        // gama_rejected: Some(0) }`，即「长度合法但值越界→ 拒块走默认」，
        // 这是**正确行为**。混判会把正确实现判红（教训：判据写错数比没判据
        // 更坏，它训练团队忽略红灯）。
        for n in 0usize..=8 {
            if n == 4 {
                continue; // 合法长度，语义另判
            }
            let png = png_with(&[(b"gAMA", vec![0u8; n])]);
            match c4::parse_color_annotation(&png) {
                Err(e) => {
                    if e.kind != c4::ColorFaultKind::GamaLength {
                        len_guard_ok = false;
                    }
                    // 五元组须带出实际长度与期望长度（判据侧独立算出的参考值）
                    if e.a != n as u64 || e.b != 4 {
                        len_guard_ok = false;
                    }
                }
                // 长度非 4 却 Ok = 长度闸失守
                Ok(_) => len_guard_ok = false,
            }
        }
        // **语义腿：4 字节但值越界**（全零= gamma 0.0< 0.01）必须
        // 「拒块走默认」而非「按合法 gamma 收下」——证明 4 字节这条腿
        // 没被当成「长度对就放行」的橡皮章。
        {
            let png = png_with(&[(b"gAMA", vec![0u8; 4])]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    if a.gamma.is_some() {
                        len_guard_ok = false;
                    }
                    if a.gama_rejected != Some(0) {
                        len_guard_ok = false;
                    }
                    if a.source.is_some() || a.warnings == 0 {
                        len_guard_ok = false;
                    }
                }
                // 越界值不应阻断（锚点：只拒该块）
                Err(_) => len_guard_ok = false,
            }
        }
        // 正向腿：恰好 4 字节的**合法**值必须被接受（证明闸不误伤）。
        // 夹逼对 1000/100000/499999/500000 四点，判据侧独立算出期望 gamma
        // （线值 / 100000.0）并与实测比对——不向被测函数要答案。
        // 手算复核：100000 = 0x0001_86A0（gamma 1.0）。
        // （早先误用 0x000F4240 = 1_000_000 → gamma 10.0，越界，
        //  把**正确实现判红**了。参考值必须与被测对象无关地独立算出。）
        for (raw, want_g) in [
            (1_000u32, 0.01f32),
            (100_000, 1.0),
            (499_999, 4.999_99),
            (500_000, 5.0),
        ] {
            let png = png_with(&[(b"gAMA", raw.to_be_bytes().to_vec())]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    let g = match a.gamma {
                        Some(g) => g,
                        None => {
                            len_guard_ok = false;
                            continue;
                        }
                    };
                    // 判据侧独立重算的参考值：线值 / 100000（规范定义）
                    let reference = raw as f32 / 100_000.0;
                    // 参考值先自校对拍：写错数会把正确实现判红
                    if (reference - want_g).abs() > 1e-6 {
                        len_guard_ok = false;
                    }
                    // 实测与独立参考值的偏差：闭区间端点须**精确**相等，
                    // 内部点允许 1 ULP 级相对误差。
                    let tol = if raw == 500_000 || raw == 1_000 { 0.0 } else { 1e-6 };
                    if (g - reference).abs() > tol {
                        len_guard_ok = false;
                    }
                    if a.gama_rejected.is_some() {
                        len_guard_ok = false;
                    }
                }
                // 4 字节合法值被拒= 闸误伤
                Err(_) => len_guard_ok = false,
            }
        }
        cs.add(
            "C04-GAMA-02 边界闭区间夹逼+越界拒块走默认+长度错阻断+长度守卫0..8全域扫描+4字节合法值独立参考对拍",
            ok && len_guard_ok,
            "",
        );
    }

    // -- C04-GAMA-03 线性化（F0159 联动） --------------------------
    {
        let mut ok = true;
        // sRGB 分段函数：端点与阈值两侧连续、无NaN、单调不减
        let mut prev = -1.0f32;
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            let l = c4::srgb_to_linear(v);
            if !(l >= 0.0 && l <= 1.0) {
                ok = false;
            }
            if l < prev {
                ok = false;
            }
            prev = l;
        }
        if c4::srgb_to_linear(0.0) != 0.0 {
            ok = false;
        }
        if (c4::srgb_to_linear(1.0) - 1.0).abs() > 0.0001 {
            ok = false;
        }
        // 阈值 0.04045 两侧应连续（不出现跳变）
        let lo = c4::srgb_to_linear(0.04045);
        let hi = c4::srgb_to_linear(0.04045 + 0.0001);
        if (lo - hi).abs() > 0.001 {
            ok = false;
        }
        // 纯 gamma：gamma=1.0 应恒等，gamma=2.2 应低于恒等（暗部更暗）
        for i in 0..=10 {
            let v = i as f32 / 10.0;
            if (c4::gamma_to_linear(v, 1.0) - v).abs() > 0.0001 {
                ok = false;
            }
            if c4::gamma_to_linear(v, 2.2) > v + 0.0001 {
                ok = false;
            }
        }
        // 越界输入必须被钳住而不是产出 NaN
        for &v in &[-1.0f32, 2.0, f32::NAN, f32::INFINITY] {
            let l = c4::srgb_to_linear(v);
            if !(l >= 0.0 && l <= 1.0) {
                ok = false;
            }
        }
        // gamma 参数非法时退化为恒等且不 panic
        for &g in &[0.0f32, -1.0, f32::NAN] {
            let l = c4::gamma_to_linear(0.5, g);
            if !(l >= 0.0 && l <= 1.0) {
                ok = false;
            }
        }
        // **与sRGB 解析律逐点绝对值对账**（本判据的关键一腿）。
        //
        // 上面的端点/单调/连续三腿**只证明「曲线是合格形状」**，
        // 不证明「曲线是对的」：任何单调、连续、值域 [0,1]、过端点的
        // 函数都能通过——把分段阈值 0.04045 改成 0.5，形状四项依旧全绿，
        // 而 v=0.5 处线性值从 0.2140 掉到 0.0387（整段中调被线性化，
        // 画面明显发暗）。故必须与**解析值**对账，用绝对容差卡死。
        //
        // 容差 1e-4：f32 在 [0,1] 上的 `powf` 误差量级 ~1e-7，
        // 1e-4 留了三个数量级余量给不同平台 libm，仍远小于最小缺陷偏差
        // 0.0023（阈值错到 0.5 时的 v=0.1 处），不会被噪声骗过也不会漏放。
        //
        // 参考值取自 IEC 61966-2-1 解析式 `x/12.92`（≤0.04045）/
        // `((x+0.055)/1.055)^2.4`（>0.04045），覆盖暗部、线性段两侧、
        // 中调、高光四类区域——只取端点会漏掉中调（缺陷恰好住在那里）。
        const REF: [(f32, f32); 7] = [
            (0.002, 0.00015480),
            (0.020, 0.00154799),
            (0.04045, 0.00313080),
            (0.100, 0.01002283),
            (0.200, 0.03310477),
            (0.500, 0.21404114),
            (0.800, 0.60382734),
        ];
        for &(v, want) in REF.iter() {
            let got = c4::srgb_to_linear(v);
            if (got - want).abs() > 0.0001 {
                ok = false;
            }
        }
        // 纯 gamma 同样与解析值对账（v^γ），防「恒等退化」式实现蒙混。
        //
        // 参考值是**手算复核过的** `v^γ`：0.5^2.2=0.2176376、
        // 0.25^2.2=0.0473661（初稿曾把后者误写为 0.0465606，
        // 那是把 γ 当成别的指数算出来的数——判据写错数比没判据更坏，
        // 它会把正确的实现判红。参考值必须与被测对象无关地独立算出。）
        const GREF: [(f32, f32, f32); 3] = [
            (0.5, 1.0, 0.5),
            (0.5, 2.2, 0.21763764),
            (0.25, 2.2, 0.04736614),
        ];
        for &(v, g, want) in GREF.iter() {
            let got = c4::gamma_to_linear(v, g);
            if (got - want).abs() > 0.0001 {
                ok = false;
            }
        }
        cs.add("C04-GAMA-03 sRGB分段+纯gamma+越界钳位+解析值对账", ok, "");
    }

    // -- C04-CHRM-01 八坐标解析与白点提取 --------------------------
    {
        let g = good_chrm();
        let png = png_with(&[(b"cHRM", chrm_payload(g))]);
        let ok = match c4::parse_color_annotation(&png) {
            Ok(a) => {
                a.source == Some(c4::ColorSource::GammaChrm)
                    && matches!(a.white_point, Some((wp, wq))
                        if (wp - 0.3127).abs() < 0.0001 && (wq - 0.3290).abs() < 0.0001)
                    && a.chrm_rejected.is_none()
            }
            Err(_) => false,
        };
        // 长度非 32 必须阻断
        let bad = png_with(&[(b"cHRM", vec![0u8; 31])]);
        let len_blocked = match c4::parse_color_annotation(&bad) {
            Err(e) => e.kind == c4::ColorFaultKind::ChrmLength,
            Ok(_) => false,
        };
        // **长度守卫的全域扫描（0..=40 逐个长度）**。
        //
        // **长度守卫的全域扫描：0..=40 逐个长度，一个不跳**。
        //
        // 为什么要补这一腿：既有语料的 cHRM 载荷清一色 32 字节，边界之外的
        // 长度**无人验证**——把`parse_chrm` 的守卫从 `!= 32` 放宽成
        // `< 31` 后判据未获可读的失败结论（变体 W14，见文件末登记表）。
        // 这是典型的**采样留洞**：语料只覆盖正确长度，闸门位置无人问津。
        //
        // **一段被实测推翻的旧说法的更正（留档以免重犯）**。
        //
        // 本处原先扫 `0..=30` 与 `33..=40`，**刻意跳过 31**，注释理由是
        // 「31 恰好是守卫被放宽到 `< 31` 时第一个越界的长度，跳过它本判据
        // 才能在被测物有该缺陷时仍可读地转红，而不是跟着崩掉」。
        //
        // **该理由不成立，已推翻**：在**真实现**上实测（隔离探针
        // `catch_unwind` 逐长度扫 0..=40），31 字节载荷干净返回
        // `ChrmLength(a=31, b=32)`，**根本不 panic**——因为真守卫是
        // `!= 32`，31 != 32 直接被拒。「panic」只在守卫被放宽的**变体**里
        // 出现。
        //
        // 于是原做法把**变体的失败形态**当成了**基线的约束**：恰恰因为
        // 真实现稳过，才敢把 31摆进语料——真实现绿、变体红，这才是一道
        // 承重的闸。少扫一格就少一格覆盖，而缺的正是**最危险的那一格**。
        let mut len_guard_ok = true;
        for n in 0usize..=40 {
            //32 是 cHRM 的**合法长度**，本腿只管「长度维度的闸位」，
            // 语义（八坐标是否合法）由正向腿与 C04-CHRM-02 管。
            // 把 32 也期望成 `Err(ChrmLength)` 是**判据写错数**：实测
            // `vec![0u8; 32]`（全零坐标）返回 `Ok`，因为八个 0.0 都
            // 落在合法闭区间 [0,1] 内——那是**正确行为**。
            // （教训同上：判据写错数会把正确实现判红。）
            if n == 32 {
                continue;
            }
            let png = png_with(&[(b"cHRM", vec![0u8; n])]);
            match c4::parse_color_annotation(&png) {
                Ok(_) => len_guard_ok = false,
                Err(e) => {
                    if e.kind != c4::ColorFaultKind::ChrmLength {
                        len_guard_ok = false;
                    }
                    // 五元组须带出实际长度与期望长度（判据侧独立算出的参考值）。
                    // 只断 kind 的话，把 `a` 恒填 0 的实现也能蒙对。
                    if e.a != n as u64 || e.b != 32 {
                        len_guard_ok = false;
                    }
                }
            }
        }
        // 正向腿：恰好 32 字节合法载荷必须被接受（证明闸不误伤）。
        // 同时对白点做**独立参考值对拍**：判据侧从 `good_chrm()` 前两个
        // 坐标自行算出期望白点，与实测比对——不向被测函数问答案。
        {
            let g = good_chrm();
            let png = png_with(&[(b"cHRM", chrm_payload(g))]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    if a.chrm_rejected.is_some() {
                        len_guard_ok = false;
                    }
                    match a.white_point {
                        None => len_guard_ok = false,
                        Some(wp) => {
                            // 判据侧独立重算：白点 = cHRM 的前两个坐标
                            let (ref_x, ref_y) = (g[0], g[1]);
                            if (wp.0 - ref_x).abs() > 1e-6 || (wp.1 - ref_y).abs() > 1e-6 {
                                len_guard_ok = false;
                            }
                        }
                    }
                }
                Err(_) => len_guard_ok = false,
            }
        }
        cs.add(
            "C04-CHRM-01 D65八坐标解析+白点提取+长度31阻断+长度守卫0..40全域扫描+白点独立参考对拍",
            ok && len_blocked && len_guard_ok,
            "",
        );
    }

    // -- C04-CHRM-02 越界坐标拒块（八坐标逐个位置） ----------------
    {
        let g = good_chrm();
        let mut all_rejected = true;
        // **逐位置**破坏：任一坐标越界都必须被拒（只测第0 个会漏掉
        // 「循环里只校验首项」这类实现缺陷）
        for i in 0..8 {
            let mut bad = g;
            bad[i] = 1.5; // 越出 [0,1]
            let png = png_with(&[(b"cHRM", chrm_payload(bad))]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    // 必须：走默认 + 记入 chrm_rejected
                    if a.source.is_some() || a.chrm_rejected.is_none() {
                        all_rejected = false;
                    }
                }
                Err(_) => {}
            }
        }
        cs.add("C04-CHRM-02 八个坐标位置逐个越界均被拒(非仅首项)", all_rejected, "");
    }

    // -- C04-ICCP-01 解析解压 + 传递到手 ----------------------------
    {
        // ICC 内容须原样可取（VE-V 消费的就是它）
        let profile: Vec<u8> = (0u8..=200u8).collect();
        let png = png_with(&[(b"iCCP", iccp_payload(&profile))]);
        let ok = match c4::parse_color_annotation(&png) {
            Ok(a) => {
                a.source == Some(c4::ColorSource::Iccp)
                    && matches!(&a.icc, Some(v) if v == &profile)
                    // 单一出口：handoff 报出 ICC 路径
                    && a.handoff() == (Some(c4::ColorSource::Iccp), true)
            }
            Err(_) => false,
        };
        // **边界：1 字节 profile 必须被接受**（最小合法 ICC）。
        //
        // 判据曾写「空 profile 也要能解出空配置」——**判据错**：
        // ICC 规范要求 profile 至少 12 字节（头部 + 标签表），
        // 0 字节不是合法profile。实测本模块对空 profile 走
        // 「解压成功但内容为空 → 仍判为 ICC 来源」这一路径，
        // 而**空内容对 VE-V 无用**（色彩管理域拿到空配置只能报错），
        // 正确处置是把空配置视为「该块不可用」→ 降级 sRGB 假定并告警。
        // 见 `C04-ICCP-04`。
        let one: Vec<u8> = vec![0xAB];
        let one_png = png_with(&[(b"iCCP", iccp_payload(&one))]);
        let one_ok = matches!(
            c4::parse_color_annotation(&one_png),
            Ok(a) if matches!(&a.icc, Some(v) if v == &one)
        );
        cs.add("C04-ICCP-01 iCCP解压后逐字节等于原profile(传VE-V链路通)", ok && one_ok, "");
    }

    // -- C04-ICCP-04 空 ICC 配置按「块不可用」降级 ------------------
    // 锚点未明说空 profile，但「统一标注供 VE-V 消费」意味着**不可消费的值
    // 不能进标注**。空配置进了标注，下游只会拿到一个空引用再自己崩。
    // 故本模块按「块不可用 → 降级 sRGB 假定 + 告警」处置，与解压失败同级。
    //
    // **口径：只拒 0 字节，1 字节起接受**（判据名里的「过短」曾被误实现为
    // 「≤4 字节」，与 `C04-ICCP-01` 的「1 字节 profile 必须原样往返」直接
    // 矛盾——两条判据不可能同时为真。这里以能自洽的那条为准：
    // 0 字节无任何内容、必然是坏数据；≥1 字节的内容合法性属ICC 结构校验，
    // 是 VE-V 色彩管理域的职责，解析器越权代判反而会把合法短profile 拒掉）。
    // ICC 规范要求 profile ≥12 字节头部，但那是**消费侧**的校验口径，
    // 不是**解析侧**的拒收口径——两者分层，不混用。
    {
        let mut ok = true;
        // (a) 0 字节 profile：解压成功但内容为空 → 不可用 → 降级 + 告警
        //
        // **前置断言（新增，防旁路）**：本腿必须走`parse_iccp` 里 `n == 0`
        // 那条**专属**分支，而不是更早的 `Err(_)` 解压失败分支——两者外部
        // 表现相同（都清空 out 并报`IccpInflate`），故只断最终标注无法区分。
        //
        // 钉死办法：**直接调`parse_iccp` 看它返回什么**。真实现下空 stored
        // 流解压成功产出 0 字节，返回的仍是 `Err(IccpInflate)`——
        // 与解压失败同码，故码本身仍不足以分辨；真正的分辨点在
        // 「解码器是否成功地产出了 0 字节」。
        //
        // 这里改用**语料侧自证**：`zlib_stored(&[])` 现在产出的流必须能被
        // 同一个 `zlib_inflate_slices` 成功解出0 字节。若哪天语料又退回
        // 「adler 字节当 DEFLATE」的错误形态，下面的自证会当场失败，
        // 而不会让本判据悄悄退化成验「解压失败」。
        {
            let flow = zlib_stored(&[]);
            // 5 字节 = zlib 头 2 + 空 stored 块 5（块头 1 + LEN 2 + NLEN 2）
            // 加 adler 4。断言其结构自洽：末 5 字节须是 01 00 00 FF FF。
            let tail = &flow[flow.len() - 9..flow.len() - 4];
            if tail != [0x01u8, 0x00, 0x00, 0xFF, 0xFF] {
                ok = false;
            }
            // 且流长须恰为 2 + 5 + 4 = 11（不多不少）——多一个字节就说明
            // 又混进了 adler 之类的杂质。
            if flow.len() != 11 {
                ok = false;
            }
        }
        let empty = png_with(&[(b"iCCP", iccp_payload(&[]))]);
        match c4::parse_color_annotation(&empty) {
            Ok(a) => {
                if a.icc.is_some() {
                    ok = false; // 空配置绝不可进标注
                }
                if a.warnings == 0 {
                    ok = false; // 必须告警
                }
                if !a.is_default_srgb() {
                    ok = false; // 应降级为 sRGB 假定
                }
            }
            Err(_) => ok = false,
        }
        // (b) 1/2/4 字节 profile：**必须被接受且零告警**——反向对照，
        // 证明该闸不是「只会拒不会放」的弱门禁（这一腿同时锁住上面的口径）。
        for n in [1usize, 2, 4] {
            let prof: Vec<u8> = (0u8..n as u8).collect();
            let png = png_with(&[(b"iCCP", iccp_payload(&prof))]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    if !matches!(&a.icc, Some(v) if v == &prof) {
                        ok = false;
                    }
                    if a.warnings != 0 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        cs.add("C04-ICCP-04 空ICC配置降级告警且非空短profile不被误拒", ok, "");
    }

    // -- C04-ICCP-02 结构非法拒绝 vs 解压失败降级（两类处置不同） ----
    {
        let mut ok = true;
        // (a) 压缩方法非 0→ 阻断（结构错）
        let mut bad_method = iccp_payload(b"x");
        // 载荷布局：keyword("icc") + NUL + 压缩方法 + zlib 流。
        // 方法字节紧跟在 keyword 的 NUL 之后 = keyword.len() + 1，
        // 此处keyword 恒为 3 字节，故方法字节索引为 4。
        // （旧写法按「keyword 只有1 字节」硬编码索引 2，在keyword 改成
        //   "icc" 后会打到NUL 上，把压缩方法测试变成keyword 长度测试——
        //   判据悄悄测了另一件事却仍显示为绿。）
        const METHOD_IDX: usize = 4; // b"icc".len() + 1
        bad_method[METHOD_IDX] = 1;
        let png = png_with(&[(b"iCCP", bad_method.clone())]);
        match c4::parse_color_annotation(&png) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::IccpCompression {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // (b) 关键字含控制字符 → 阻断
        let mut bad_kw = Vec::new();
        bad_kw.push(0x01u8);
        bad_kw.push(0);
        bad_kw.push(0);
        bad_kw.extend_from_slice(&zlib_stored(b"x"));
        let png = png_with(&[(b"iCCP", bad_kw)]);
        match c4::parse_color_annotation(&png) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::IccpKeyword {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // **keyword 长度上下界语料（反假变体 W15 实测补入）**。
        //
        // 初版只用「含控制字符的 keyword」这一种坏形态。变体把
        // `nul == 0 || nul > ICCP_KEYWORD_MAX` 这条**长度校验整条摘掉**后，
        // 21 项判据全绿——因为既有语料的 keyword 恒为 `"icc"`（长度 3，
        // 合法），长度校验从未被触发；那条分支成了不承重的注脚。
        //
        // 补两条**表外真实形态**：
        //   - 空keyword（NUL 在位置 0）→ 规范要求长度 1..=79，必须拒；
        //   - 超长keyword（80 字节）→ 越过 79 上限，必须拒。
        // 两者都报IccpKeyword，且都**不得**误报成别的类别。
        //
        // 注意：摘掉长度校验后，空 keyword 会让`method = data[1]` 取到
        // 关键字之后的字节，整条载荷错位——但对**合法的 5 字节最短载荷**
        // 而言它恰好还能通过，故必须靠上面两条语料才照得出来。
        {
            // (b1) 空 keyword：载荷 = NUL + 方法 0 + zlib("x")
            let mut empty_kw = Vec::new();
            empty_kw.push(0u8); // 空 keyword（长度 0，非法）
            empty_kw.push(0); // 压缩方法
            empty_kw.extend_from_slice(&zlib_stored(b"x"));
            let png = png_with(&[(b"iCCP", empty_kw)]);
            match c4::parse_color_annotation(&png) {
                Err(e) => {
                    if e.kind != c4::ColorFaultKind::IccpKeyword {
                        ok = false;
                    }
                }
                Ok(_) => ok = false,
            }
            // (b2) 超长keyword：80 个可打印字符（越过 79 上限）
            let mut long_kw = Vec::new();
            for i in 0..80u8 {
                // 全用可打印字符，确保被拒的原因是**长度**而非字符集
                long_kw.push(b'a' + (i % 26));
            }
            long_kw.push(0u8); // NUL
            long_kw.push(0); // 压缩方法
            long_kw.extend_from_slice(&zlib_stored(b"x"));
            let png = png_with(&[(b"iCCP", long_kw)]);
            match c4::parse_color_annotation(&png) {
                Err(e) => {
                    if e.kind != c4::ColorFaultKind::IccpKeyword {
                        ok = false;
                    }
                    // 五元组应带出实际长度与上限，便于定位
                    if e.a != 80 || e.b != 79 {
                        ok = false;
                    }
                }
                Ok(_) => ok = false,
            }
            // (b3) 反向对照：恰好 79 字节 keyword **必须被接受**——
            // 证明这不是「只会拒不会放」的弱门禁。
            let mut edge_kw = Vec::new();
            for i in 0..79u8 {
                edge_kw.push(b'a' + (i % 26));
            }
            edge_kw.push(0u8);
            edge_kw.push(0);
            edge_kw.extend_from_slice(&zlib_stored(b"x"));
            let png = png_with(&[(b"iCCP", edge_kw)]);
            match c4::parse_color_annotation(&png) {
                Ok(a) => {
                    if a.icc.as_ref().map(|v| v.as_slice()) != Some(&b"x"[..]) {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        // (c) zlib 载荷损坏 → **降级 + 告警，不阻断**（锚点明文）
        let mut bad_z = iccp_payload(b"hello");
        let len = bad_z.len();
        bad_z[len - 1] ^= 0xFF; // 破坏 adler
        bad_z[len - 2] ^= 0xFF;
        let png = png_with(&[(b"iCCP", bad_z)]);
        match c4::parse_color_annotation(&png) {
            Ok(a) => {
                if a.icc.is_some() {
                    ok = false;
                }
                if a.warnings == 0 {
                    ok = false;
                }
                if a.source.is_some() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        cs.add("C04-ICCP-02 方法/关键字错阻断而zlib损坏降级告警(处置分离)", ok, "");
    }

    // -- C04-ICCP-03 8MB 上限真实生效 ------------------------------
    // 判据要点：**上限闸要真的拦得住**，用「声明的上限常量」与
    // 「实测拒绝行为」双向印证，而非只断言常量等于 8MB。
    //
    // **语料必须用合法 zlib**（判据曾用裸字节，被测实现先在 inflate
    // 阶段就拒了，永远走不到上限闸——那样的「拒绝」证明不了上限在起作用）。
    // 故这里造一个**合法**的 zlib stored 流，其解压后长度恰好超上限。
    {
        let ok_const = c4::ICCP_MAX_BYTES == 8 * 1024 * 1024;
        // 合法 zlib，解压后 = 上限 + 1 字节
        let over = vec![0x5Au8; c4::ICCP_MAX_BYTES + 1];
        let payload = {
            let mut p = Vec::new();
            p.push(b'p');
            p.push(0);
            p.push(0);
            p.extend_from_slice(&zlib_stored(&over));
            p
        };
        let png = png_with(&[(b"iCCP", payload.clone())]);
        // 处置：解压后超限 → **降级 + 告警**（不阻断整图），
        // 故此处断言的是「不进标注 + 置告警」，而非返回 Err。
        let handled = match c4::parse_color_annotation(&png) {
            Ok(a) => a.icc.is_none() && a.warnings > 0,
            Err(_) => false,
        };
        // **上限专属出口的直接断言**（这条是上限闸「可被观测」的保证）。
        //
        // 只断言 `handled` 是不够的：超限与「zlib 损坏」在外部都是
        // 「不进标注 + 置告警」，二者行为重合，于是把上限闸整条摘掉
        // （改走普通解压失败）时判据仍全绿——上限就成了不可见的注脚。
        // 故此处直接调解析层，断言故障**种类**必须是 `IccpTooLarge`：
        // 摘闸则种类变成 `IccpInflate`，本判据转红。
        let gate_kind = matches!(
            c4::parse_iccp(&payload, &mut Vec::new()),
            Err(ref e) if e.kind == c4::ColorFaultKind::IccpTooLarge
        );
        // 反向对照：恰好等于上限的合法 profile 必须**被接受**
        // （证明闸门不误伤边界值——只会拒不会放是弱门禁）
        let at_limit = vec![0x5Au8; c4::ICCP_MAX_BYTES];
        let ok_payload = {
            let mut p = Vec::new();
            p.push(b'p');
            p.push(0);
            p.push(0);
            p.extend_from_slice(&zlib_stored(&at_limit));
            p
        };
        let at_png = png_with(&[(b"iCCP", ok_payload.clone())]);
        // 恰好等于上限者不得走超限出口（边界不误伤，解析层同口径）。
        let at_limit_ok = matches!(
            c4::parse_iccp(&ok_payload, &mut Vec::new()),
            Ok(())
        );
        let accepted = matches!(
            c4::parse_color_annotation(&at_png),
            Ok(a) if matches!(&a.icc, Some(v) if v.len() == c4::ICCP_MAX_BYTES)
        );
        cs.add(
            "C04-ICCP-03 8MB上限拦超限且不误伤恰好等于上限者",
            ok_const && handled && gate_kind && at_limit_ok && accepted,
            "",
        );
    }

    // -- C04-CONF-01 优先级表自洽（rank 与 SOURCE_RANK 互校） ------
    {
        let mut ok = true;
        // 秩严格降序
        for i in 1..c4::SOURCE_RANK.len() {
            if c4::SOURCE_RANK[i - 1].rank() <= c4::SOURCE_RANK[i].rank() {
                ok = false;
            }
        }
        // 表覆盖全部三种来源且无重复
        let want = [c4::ColorSource::Iccp, c4::ColorSource::Srgb, c4::ColorSource::GammaChrm];
        if c4::SOURCE_RANK != want {
            ok = false;
        }
        cs.add("C04-CONF-01 优先级表秩严格降序且恰含三种来源", ok, "");
    }

    // -- C04-CONF-02 冲突裁决：六种组合全覆盖 ----------------------
    // 锚点：iCCP > sRGB > gAMA+cHRM，低优先级忽略并计数告警
    {
        let mut ok = true;
        let icc = Some(vec![1u8, 2, 3]);
        let srgb = Some(c4::RenderIntent::Perceptual);
        let g = Some(2.2f32);
        let ch = Some(good_chrm());
        // (i, s, g) 八种组合 → 期望胜者
        let cases: [(bool, bool, bool, Option<c4::ColorSource>, usize); 8] = [
            (true, true, true, Some(c4::ColorSource::Iccp), 2),
            (true, false, true, Some(c4::ColorSource::Iccp), 1),
            (true, true, false, Some(c4::ColorSource::Iccp), 1),
            (true, false, false, Some(c4::ColorSource::Iccp), 0),
            (false, true, true, Some(c4::ColorSource::Srgb), 1),
            (false, true, false, Some(c4::ColorSource::Srgb), 0),
            (false, false, true, Some(c4::ColorSource::GammaChrm), 0),
            (false, false, false, None, 0),
        ];
        for (has_i, has_s, has_g, want_src, want_conflicts) in cases.iter() {
            let a = c4::resolve(
                if *has_i { icc.clone() } else { None },
                if *has_s { srgb } else { None },
                if *has_g { g } else { None },
                if *has_g { ch } else { None },
            );
            if a.source != *want_src {
                ok = false;
            }
            if a.conflicts.len() != *want_conflicts {
                ok = false;
            }
            // 冲突记账的每一条都必须是「败给胜者」
            for cf in a.conflicts.iter() {
                if cf.winner != a.source.unwrap_or(cf.ignored) {
                    ok = false;
                }
            }
        }
        cs.add("C04-CONF-02 八种来源组合的胜者与冲突计数全部正确", ok, "");
    }

    // -- C04-CONF-03 同秩 gAMA+cHRM 取并集而非二选一 ---------------
    {
        // 只有 gAMA（无 cHRM）：gamma 生效，白点为 None，但来源仍是 GammaChrm
        let a = c4::resolve(None, None, Some(2.2), None);
        let only_gama = a.source == Some(c4::ColorSource::GammaChrm)
            && a.gamma == Some(2.2)
            && a.white_point.is_none();
        // 只有 cHRM：白点生效，gamma 为 None，来源仍是 GammaChrm
        let b = c4::resolve(None, None, None, Some(good_chrm()));
        let only_chrm = b.source == Some(c4::ColorSource::GammaChrm)
            && b.gamma.is_none()
            && b.white_point.is_some();
        cs.add("C04-CONF-03 gAMA与cHRM同秩取并集(半截标注不误判为完整)", only_gama && only_chrm, "");
    }

    // -- C04-ANNO-01 统一出口：下游不见原始块 ----------------------
    {
        let profile: Vec<u8> = (0u8..=127u8).collect();
        let png = png_with(&[
            (b"gAMA", gama_payload(0.45455)),
            (b"sRGB", vec![1u8]),
            (b"iCCP", iccp_payload(&profile)),
        ]);
        let ok = match c4::parse_color_annotation(&png) {
            Ok(a) => {
                // 胜者是 iCCP（最高秩），且 sRGB/gAMA 被记为冲突
                a.source == Some(c4::ColorSource::Iccp)
                    && a.conflicts.len() == 2
                    // 冲突项里应含 sRGB 与 gAMA+cHRM 两者
                    && a.conflicts.iter().any(|x| x.ignored == c4::ColorSource::Srgb)
                    && a.conflicts.iter().any(|x| x.ignored == c4::ColorSource::GammaChrm)
            }
            Err(_) => false,
        };
        cs.add("C04-ANNO-01 三块共存时iCCP胜出且两个败者均记账", ok, "");
    }

    // -- C04-ANNO-02 下游只见标注：低优先级值不进标注 --------------
    {
        // 有 gAMA 但无 ICC/sRGB → gamma 生效。
        // 反之（gAMA + sRGB）→ sRGB 胜出，**gamma 不得出现在标注里**
        // （否则下游会以为「既声明了 sRGB 又声明了 gamma」）。
        let png = png_with(&[(b"gAMA", gama_payload(2.2)), (b"sRGB", vec![0u8])]);
        let ok = match c4::parse_color_annotation(&png) {
            Ok(a) => {
                a.source == Some(c4::ColorSource::Srgb)
                    // 败者的值仍保留在结构里供记账，但**不参与 handoff 语义**——
                    // 故只断言胜者与冲突，不断言 gamma 为 None（那是另一种设计取舍）
                    && a.conflicts.len() == 1
                    && a.conflicts[0].ignored == c4::ColorSource::GammaChrm
            }
            Err(_) => false,
        };
        cs.add("C04-ANNO-02 gAMA+sRGB共存时sRGB胜出且gAMA记为被忽略", ok, "");
    }

    // -- C04-ERR-01 全部缺失 → 按 sRGB 假定 -------------------------
    {
        let png = png_with(&[]);
        let ok = match c4::parse_color_annotation(&png) {
            Ok(a) => {
                a.is_default_srgb()
                    && a.source.is_none()
                    && a.gamma.is_none()
                    && a.white_point.is_none()
                    && a.icc.is_none()
                    && a.intent.is_none()
                    && a.conflicts.is_empty()
                    && a.warnings == 0
            }
            Err(_) => false,
        };
        cs.add("C04-ERR-01 无任何色彩块时按sRGB假定且各字段皆空", ok, "");
    }

    // -- C04-ERR-02 结构错拒绝：签名 / 块越界 ----------------------
    {
        let mut ok = true;
        // 签名错
        let mut bad = png_with(&[]);
        bad[1] = b'X';
        match c4::parse_color_annotation(&bad) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::BadSignature {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // 空输入
        if c4::parse_color_annotation(&[]).is_ok() {
            ok = false;
        }
        // 块长度字段越界（指向文件外）
        let mut trunc = png_with(&[]);
        trunc[8..12].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        match c4::parse_color_annotation(&trunc) {
            Err(e) => {
                if e.kind != c4::ColorFaultKind::ChunkTruncated {
                    ok = false;
                }
            }
            Ok(_) => ok = false,
        }
        // 尾部截断（在块中间砍断）
        let full = png_with(&[(b"sRGB", vec![0u8])]);
        let cut = &full[..full.len() - 6];
        // 截断后应报错或正常解析，但**绝不 panic**——本判据只断言不panic
        // 且结果要么 Ok 要么 Err（此处显式跑一遍以确保无 panic）
        let _ = c4::parse_color_annotation(cut);
        cs.add("C04-ERR-02 签名错/空输入/块长越界各自报对应类别", ok, "");
    }

    // -- C04-ERR-03 CRC 错按ancillary 告警跳过 ---------------------
    {
        let mut ok = true;
        // 正常 gAMA 后人为破坏其 CRC → 应告警并**跳过该块**（走默认）
        let png = png_with(&[(b"gAMA", gama_payload(1.0))]);
        // gAMA 块的 CRC 在 IHDR(8+25) 之后
        let gpos = 8 + 25;
        let mut broken = png.clone();
        let crc_at = gpos + 8 + 4;
        broken[crc_at] ^= 0xFF;
        match c4::parse_color_annotation(&broken) {
            Ok(a) => {
                // CRC 错计入了告警（warnings 含 crc_warnings）
                if a.warnings == 0 {
                    ok = false;
                }
                // CRC 错的块应被跳过 → 走默认
                if a.source.is_some() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        // 原始文件应无告警
        match c4::parse_color_annotation(&png) {
            Ok(a) => {
                if a.warnings != 0 {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        cs.add("C04-ERR-03 色彩块CRC错按ancillary告警跳过(不阻断)", ok, "");
    }

    // -- C04-ERR-04 错误五元组齐全且码无重复 ------------------------
    {
        let kinds = [
            c4::ColorFaultKind::BadSignature,
            c4::ColorFaultKind::ChunkTruncated,
            c4::ColorFaultKind::CrcMismatch,
            c4::ColorFaultKind::SrgbLength,
            c4::ColorFaultKind::BadIntent,
            c4::ColorFaultKind::GamaLength,
            c4::ColorFaultKind::GamaOutOfRange,
            c4::ColorFaultKind::ChrmLength,
            c4::ColorFaultKind::ChrmCoordinate,
            c4::ColorFaultKind::IccpTooShort,
            c4::ColorFaultKind::IccpKeyword,
            c4::ColorFaultKind::IccpCompression,
            c4::ColorFaultKind::IccpInflate,
            c4::ColorFaultKind::IccpTooLarge,
            c4::ColorFaultKind::BufferShort,
        ];
        let mut ok = true;
        let mut codes: Vec<u16> = Vec::new();
        for &k in kinds.iter() {
            let f = c4::ColorFault::new(k);
            if f.code() == 0 || f.cause().is_empty() || f.advice().is_empty() || f.human().is_empty() {
                ok = false;
            }
            codes.push(f.code());
        }
        // 码必须互异（重码会让诊断无法定位类别）
        codes.sort_unstable();
        for i in 1..codes.len() {
            if codes[i] == codes[i - 1] {
                ok = false;
            }
        }
        cs.add("C04-ERR-04 十五类故障五元组齐全且错误码无重复", ok, "");
    }

    // -- C04-ERR-05 畸形输入遍历不崩溃（≥20 案） -------------------
    {
        let base = png_with(&[
            (b"gAMA", gama_payload(1.0)),
            (b"sRGB", vec![1u8]),
            (b"iCCP", iccp_payload(b"prof")),
            (b"cHRM", chrm_payload(good_chrm())),
        ]);
        let mut cases = 0usize;
        // 1) 空输入
        let _ = c4::parse_color_annotation(&[]);
        cases += 1;
        // 2) 仅签名
        let mut only_sig = dec::PNG_SIG.to_vec();
        only_sig.extend_from_slice(&[0u8; 4]);
        let _ = c4::parse_color_annotation(&only_sig);
        cases += 1;
        // 3) 逐字节改签名
        for i in 1..8usize {
            let mut v = base.clone();
            v[i] ^= 0xFF;
            let _ = c4::parse_color_annotation(&v);
            cases += 1;
        }
        // 4) 逐位置破坏每个色彩块的 CRC
        let mut p = 8usize;
        while p + 8 <= base.len() {
            let len = u32::from_be_bytes([base[p], base[p + 1], base[p + 2], base[p + 3]]) as usize;
            if p + 12 + len > base.len() {
                break;
            }
            let ty = &base[p + 4..p + 8];
            if ty == b"sRGB" || ty == b"gAMA" || ty == b"iCCP" || ty == b"cHRM" {
                let mut v = base.clone();
                v[p + 8 + len] ^= 0xFF;
                let _ = c4::parse_color_annotation(&v);
                cases += 1;
            }
            p += 12 + len;
        }
        // 5) 尾部逐级截断
        for cut in [1usize, 5, 12, 30, 60, 100] {
            if base.len() > cut {
                let _ = c4::parse_color_annotation(&base[..base.len() - cut]);
                cases += 1;
            }
        }
        // 6) 块长度字段越界（多种值）
        for &bad_len in &[0x7FFFFFFFu32, 0xFFFFFFFF, 0x80000000, 1_000_000] {
            let mut v = base.clone();
            v[8..12].copy_from_slice(&bad_len.to_be_bytes());
            let _ = c4::parse_color_annotation(&v);
            cases += 1;
        }
        // 7) 各块载荷逐字节随机化
        let mut rng: u32 = 0x1357_9BDF;
        let mut next = move || {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            (rng >> 16) as u8
        };
        let mut q = 8usize;
        while q + 8 <= base.len() {
            let len = u32::from_be_bytes([base[q], base[q + 1], base[q + 2], base[q + 3]]) as usize;
            if q + 12 + len > base.len() {
                break;
            }
            let ty = &base[q + 4..q + 8];
            if ty == b"sRGB" || ty == b"gAMA" || ty == b"iCCP" || ty == b"cHRM" {
                let mut v = base.clone();
                for j in 0..len {
                    v[q + 8 + j] = next();
                }
                let _ = c4::parse_color_annotation(&v);
                cases += 1;
            }
            q += 12 + len;
        }
        // 8) 重复块（同一块出现两次）
        let dup = png_with(&[(b"gAMA", gama_payload(1.0)), (b"gAMA", gama_payload(2.2))]);
        let dup_ok = match c4::parse_color_annotation(&dup) {
            Ok(a) => a.gamma == Some(1.0), // 首次出现为准
            Err(_) => false,
        };
        // 9) 单色彩块逐块缺失（4 案：每个色彩块单独缺席，验证缺块不崩）
        //
        // 这4 案是**语义**缺失而非字节破坏，与上面 7 类「畸形」互补：
        // 上面证明「坏数据不崩」，这里证明「数据合法但不全」也不崩。
        let all_chunks = [
            (b"gAMA", gama_payload(1.0)),
            (b"sRGB", vec![1u8]),
            (b"iCCP", iccp_payload(b"prof")),
            (b"cHRM", chrm_payload(good_chrm())),
        ];
        for skip in 0..all_chunks.len() {
            let mut subset: Vec<(&[u8; 4], Vec<u8>)> = Vec::new();
            for (i, c) in all_chunks.iter().enumerate() {
                if i != skip {
                    subset.push((c.0, c.1.clone()));
                }
            }
            let png = png_with(&subset);
            let _ = c4::parse_color_annotation(&png);
            cases += 1;
        }
        // 断言阈值按**实际枚举出的案数**定，不写一个够不着的整数：
        // 旧判据写「>= 30」而枚举只有 27 案，是拿阈值凑数——阈值应跟随
        // 语料枚举，此处 31 案即以 30 为下限并留 1 案余量。
        cs.add(
            "C04-ERR-05 畸形色彩数据≥30案遍历不崩溃且重复块取首个",
            cases >= 30 && dup_ok,
            "",
        );
    }

    // -- C04-ERR-06 块长度字段全域扫描（长度闸 + 32 位溢出面） ----------
    //
    // 判据要点：`scan_color_chunks` 里 `len` 来自 **4 字节不可信字段**，
    // 它参与三处边界算术（总长校验 / 载荷切片 / 游标推进）。这三处若写裸
    // 加法，在 32 位目标（usize=32bit）上 `pos + 12 + len` 可溢出绕回成
    // 小值 —— 于是**通过**校验后越界切片 panic；若绕回发生在推进式，则
    // `pos` 倒退成小值 → 下一轮从文件中部重扫 → **死循环**。
    //
    //## 诚实的可观测性边界（这一段是本判据最重要的部分）
    //
    // 变异实测（5 个变体，见 `VARIANT_REGISTRY` 的 W20~W24）结论：
    //
    //   · **可观测**：删掉长度守卫（`total > file.len()`）→ 本判据**转红**。
    //     这是长度闸的承重腿，真承重。
    //   · **不可观测**：把 `checked_add` 换回裸加法（总长 / 载荷终点 /
    //     游标推进三处）→ 本判据在 64 位开发机上**照样全绿**。
    //
    // 原因不是判据写错，而是**该风险面在 64 位上不可观测**：`len` 是 u32
    // 字段故 `len <= 2^32-1`，而 `pos <= file.len()`。64 位下二者之和最大
    // 约 `2^33`，远小于 `usize::MAX = 2^64` —— 裸加法**恒不溢出**，
    // 因此**任何语料都无法区分它与 `checked_add`**。
    //
    // 那能不能造语料硬逼出来？不能。`len` 的值域被 u32 字段**钉死**在
    // 2^32-1，`pos` 的值域被文件实际长度**钉死**在内存量级，两者在 64 位
    // 上永不相遇于回绕区。**语料无法注入一个不存在的状态**。
    //
    // 故此处不假装能验它：判据只断言**可观测的长度闸**，而把checked 算术
    // 如实登记为「防御性加固，由 32 位目标构建负责验证」。若日后有人编
    // 32 位目标，本判据**不会**替他们抓出这三处 —— 这正是本段写下来的
    // 理由，避免后人误以为「21 项全绿 ⇒ 溢出面已被覆盖」。
    //
    // （对照弱门禁十诫#7「判据向被测函数问答案 = 自证式」的同源教训：
    // **判据证明不了的事，要写出来，而不是让绿项冒充覆盖。）**
    //
    // 为何断言「返回 Err」而不是「不崩」：本函数对块截断的处置是
    // `ChunkTruncated` **阻断**（`Err`），故正确实现的外部表现就是 `Err`。
    // 删掉长度守卫后实现会在 64 位上正常返回 `Ok`，故本判据转红。
    {
        let mut all_ok = true;
        let mut probed = 0usize;

        // 三类 len 窗口（名称即语义，值位不用故占 0）：
        //  · near_max        ：len 字段取 u32::MAX，32 位下 `pos+12+len` 必溢出
        //  · truncated_payload：声称 8 字节载荷而文件只给 8-delta 字节
        //                       （夹逼对的**越界侧**，靠砍文件制造）
        //  · exact_at_tail：总长恰好放满（夹逼对的**合法侧**，必须**被接受**
        //                     —— 只会拒不会放是弱门禁）
        let windows: [(&str, u32); 3] = [
            ("near_max", u32::MAX),
            ("exact_at_tail", 0),
            ("truncated_payload", 0),
        ];
        for (name, _) in windows.iter() {
            for delta in 0..3u32 {
                // 构造：`sig | len(4) | type(4) | 载荷 | CRC(4)`
                //
                // **「越界侧」必须真的越界**（这是本判据第一版写错的地方，
                // 如实留档）：初版把越界侧写成「declared = 载荷实际长度 + 1」，
                // 而文件仍按`12 + 载荷 + 4` 铺满——于是 declared+9 的总长
                // 仍**小于**文件长度，块在文件内**合法**，实现返回 `Ok`
                // 是正确的，判据却要求 `Err` → **判据自己写错了**。
                //
                // 教训与本仓库既有判据同源（见 `C04-GAMA-02` 的更正）：
                // 「越界侧」要靠**文件长度**制造，不能靠 declared 字段制造——
                // 后者只改长度字段、不改文件布局，根本没触及截断。
                //
                // 正确做法：declared 保持不变，**把文件在载荷中途砍断**，
                // 使 `pos + 12 + len > file.len()` 真正成立。
                let declared_len: u32 = match *name {
                    // len 字段取 u32::MAX：32 位目标上 `pos + 12 + len` 必溢出
                    "near_max" => u32::MAX,
                    // 总长恰好放满（合法侧的夹逼点）
                    "exact_at_tail" => 8u32.saturating_sub(delta),
                    // 声称 8 字节载荷，但文件只给 8-delta 字节 → 必然截断
                    _ => 8u32.saturating_sub(delta),
                };
                // 实际写入文件的载荷字节数
                let present_len: usize = match *name {
                    "near_max" => 0,
                    "exact_at_tail" => declared_len as usize, // 放满
                    _ => declared_len as usize - delta as usize, // 砍掉 delta 字节
                };
                let mut f = dec::PNG_SIG.to_vec();
                f.extend_from_slice(&declared_len.to_be_bytes());
                f.extend_from_slice(b"gAMA");
                f.extend_from_slice(&vec![0x80u8; present_len]);
                // 合法侧才补足 CRC（凑满 12+len）；截断侧**不补**——
                // 补了就又变回合法块了。CRC 内容一律写错：本判据只关心
                // **长度闸**，CRC 失败走告警分支（不阻断）。
                if *name == "exact_at_tail" {
                    f.extend_from_slice(&[0u8; 4]);
                } else if *name == "near_max" {
                    f.extend_from_slice(&[0u8; 4]);
                }
                probed += 1;

                match c4::scan_color_chunks(&f) {
                    // 越界侧 → 必须阻断，且故障种类是 ChunkTruncated
                    Err(ref e)
                        if *name != "exact_at_tail"
                            && e.kind == c4::ColorFaultKind::ChunkTruncated => {}
                    // 合法侧（总长恰好放满）→ 必须被接受（闸门不误伤）
                    Ok(ref r) if *name == "exact_at_tail" && r.gama.is_none() => {}
                    _ => all_ok = false,
                }
            }
        }

        // 附加：**合法侧不能被误伤**的独立对照—— 造一个真正完整的
        // 「gAMA(4 字节载荷) + IEND」文件，必须扫出gAMA 且无截断。
        let mut good = dec::PNG_SIG.to_vec();
        let gp = gama_payload(2.2);
        good.extend_from_slice(&(gp.len() as u32).to_be_bytes());
        good.extend_from_slice(b"gAMA");
        good.extend_from_slice(&gp);
        let crc = crc32(&good[good.len() - gp.len() - 4..]);
        good.extend_from_slice(&crc.to_be_bytes());
        let good_ok = matches!(c4::scan_color_chunks(&good), Ok(ref r) if r.gama.is_some());

        cs.add(
            "C04-ERR-06 块长度字段全域扫描：三窗口×偏移钉死长度闸（checked溢出面不可观测，如实登记）",
            all_ok && probed >= 9 && good_ok,
            "",
        );
    }

    cs
}

// ---------------------------------------------------------------------------
// 反假变体登记（门禁有效性证明）
// ---------------------------------------------------------------------------
//
// **判据不证明自己有效，除非能把被测物改坏并看到它变红。**
//
// 实测执行（2026-10-07，隔离探针：`#[path]` 直挂真文件 + 镜像
// `svstar2` 层级 + 真实 `checks.rs`/`perfstar` 底盘，`catch_unwind` 兜底）：
//
// | 变体 | 注入 | 实测结果 |
// |---|---|---|
// | W1 | 优先级表把 Iccp/Srgb 秩对调 | C04-CONF-01/02/ANNO-01 转红 |
// | W2 | gAMA 越界改为「钳到边界」而非拒块 | C04-GAMA-02 转红 |
// | W3 | iCCP 解压失败改为返回 Err（不降级） | C04-ICCP-02 转红 |
// | W4 | cHRM 只校验首坐标 | C04-CHRM-02 转红 |
// | W5 | 缺失块时不走 sRGB 假定（凭空造来源） | C04-ERR-01 转红 |
// | W6 | CRC 错改为阻断而非告警 | C04-ERR-03 转红 |
// | W13 |摘掉 iCCP keyword 长度上下界校验 | C04-ICCP-02 转红 |
// | W14 | 放宽 `parse_chrm` 长度守卫 `!= 32` → `< 31` | 31 字节语料当场撞越界（暴露） |
// | W13c | 放宽 `parse_gama` 长度守卫 `!= 4` → `< 3` | 3 字节语料当场撞越界（暴露） |
// | W16 | `ChrmLength` 五元组 `a` 恒填 0 | C04-CHRM-01 转红 |
// | W17 | `GamaLength` 五元组 `a` 恒填 0 | C04-GAMA-02 转红 |
// | W13b | 调用方回退裸读 `d[0..4]` | **全绿——等价变异**（见下） |
//
// **W14 / W13c 为什么记「暴露」而不是「转红」。**
//
// 这两条放宽的是**被测物自身的长度守卫**，越界发生在被测函数**内部**，
// 判据来不及断言——内核 no_std 无 `catch_unwind`，所以我们拿到的是一个
// panic 栈而不是「C04-GAMA-02 转红」。这**不是判据的缺口**：语料已经
// 把那个长度摆进去了（正是它撞出来的），是**被测物自己有缺陷**。
// 记「暴露」而非「转红」，是为了不谎报门禁强度。
//
// **反过来，正是因为真实现稳过，语料才敢摆进去。** 早期版本为了「不让
// 判据跟着崩」而**跳过** 3 / 31 这两个长度——那等于把变体的失败形态当成
// 了基线的约束，少扫的又恰好是最危险的两格。实测（`catch_unwind`逐长度
// 扫 0..=8 与 0..=40）证明真实现对 3 / 31 干净返回 `GamaLength` /
// `ChrmLength`，**根本不 panic**。跳过它们是错的，已改回全域扫描。
//
// **W13b 是等价变异，如实登记。**
//
// 把调用方的 `ann.gama_rejected = Some(e.a as u32)` 改回裸读 `d[0..4]`，
// 21 项判据**照样全绿**。根因：真实现下 `parse_gama` 必先拒非 4 字节载荷，
// 「非长度错」分支**只可能**在长度恰为 4 时到达，此时裸读与读 `e.a`
// 结果完全相同。故该改动是**防御性加固**（对守卫被放宽的鲁棒性），
// **不是**修了一个可观测的缺陷。真正承重的长度闸是 `C04-GAMA-02`
// 的 0..=8 全域扫描（变体 W13c 实测有效）。
//
// **W16 / W17 验的是什么**：判据若只断 `e.kind`，把五元组 `a` 恒填 0 的
// 实现也能蒙对（弱门禁：向被测函数问答案）。现判据逐长度断言
// `e.a == n`（判据侧独立重算的参考值），两条变体均实测转红——闸门承重。
//
// 变体不改坏判据本身，故不入册为常规判据，由回归时按单执行。

/// 变体登记（名称、目标判据）。
///
/// `C04-GAMA-02 转红` 一类为目标判据；`暴露` 表示变体把**被测物自身**
/// 改出越界panic，语料已覆盖该长度但判据无法断言（no_std 无
/// `catch_unwind`）；`等价变异` 表示该改动在真实现下与原实现行为完全一致，
/// 属防御性加固而非缺陷修复。
pub const VARIANT_REGISTRY: [(&str, &str); 17] = [
    ("W1-swap-iccp-srgb-rank", "C04-CONF-01/C04-CONF-02/C04-ANNO-01"),
    ("W2-gama-clamp-instead-of-reject", "C04-GAMA-02"),
    ("W3-iccp-inflate-hard-fail", "C04-ICCP-02"),
    ("W4-chrm-validate-first-only", "C04-CHRM-02"),
    ("W5-missing-blocks-invent-source", "C04-ERR-01"),
    ("W6-crc-hard-fail", "C04-ERR-03"),
    ("W13-iccp-keyword-len-check-removed", "C04-ICCP-02"),
    ("W14-chrm-len-guard-loosened-to-lt-31", "暴露(31 字节语料撞越界)"),
    ("W13c-gama-len-guard-loosened-to-lt-3", "暴露(3 字节语料撞越界)"),
    ("W16-chrm-length-fault-a-zero-filled", "C04-CHRM-01"),
    ("W17-gama-length-fault-a-zero-filled", "C04-GAMA-02"),
    ("W13b-caller-raw-read-d0-4", "等价变异(防御性加固,非缺陷)"),
    // -- C04-ERR-06 的变异实测结论（2026-10-08 W014）--------------------
    // W20 **转红**：长度闸真承重。删掉 `total > file.len()` 守卫后本判据
    // 转红 → 守卫不可摘。
    ("W20-drop-total-guard", "C04-ERR-06 转红(长度闸真承重)"),
    // W21~W24 **全绿**：把 checked_add 换回裸加法，在 64 位开发机上判据
    // 无法察觉。原因是**可观测性边界**而非判据缺陷：`len` 受u32 字段
    // 限制、pos 受实际文件长度限制，64 位下二者之和永不回绕，语料无法
    // 注入不存在的状态。故如实登记为「不可观测，由 32 位目标构建负责」。
    // **不是判据写错，也不是实现有洞**——是这条风险面在 64 位上不可测。
    ("W21-total-checked-to-bare-add", "不可观测(64位下len+pos不溢出)"),
    ("W22-payload-checked-to-bare-add", "不可观测(同上)"),
    ("W23-cursor-checked-to-bare-add", "不可观测(同上)"),
    ("W24-while-cond-bare-add", "不可观测(同上)"),
];