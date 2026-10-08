//! VE-F1619 · 几何一致性 · 域自检（判据逐条映射，五族）
//!
//! 锚点判据 → 判据族：
//! - 跨平台（三要件审计 + 算子白名单）→ [`group_audit`] / [`group_platform`]
//! - 双跑（量化双跑 + 脏化再跑 + 重排双跑）→ [`group_drun`]
//! - 跨版本（v1 解码语义冻结 + 老文件摘要钉死）→ [`group_xver`]
//! - 判据（收口自检）→ [`group_meta`]
//!
//! 双向验证纪律：不同量化档位的语料摘要必须互异（证明摘要真的在算字节，
//! 不是恒等函数）；脏化再跑必须与干净跑一致（证明无共享态是被实测的而非
//! 声明的）；判据侧独立实现的 FNV 与产线 FNV 对拍（同一标准两份实现，
//! 一致才说明两边都没写错）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vef22_consistency::*;

/// 判据侧独立 FNV-1a（标准常数独立抄录、循环写法与产线不同——
/// 两份实现同值才互证正确）。
fn ref_fnv(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 14695981039346656037;
    const PRIME: u64 = 1099511628211;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vef22_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F1619");
    group_audit(&mut set);
    group_platform(&mut set);
    group_drun(&mut set);
    group_xver(&mut set);
    group_meta(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 跨平台（静态面）：三要件审计
// ---------------------------------------------------------------------------

fn group_audit(set: &mut CheckSet) {
    // ① 审计全绿：12 行全在、证据全非空、闭集对账。
    set.add(
        "C19-AUD-01 三要件审计全绿（4环节×3要件）",
        determinism_audit().is_ok() && DETERMINISM_AUDIT.len() == 12,
        "环节集与要件集是闭集：增删环节或要件必须过判据",
    );

    // ② 证据句逐行非空（独立扫描，不依赖 audit 的内部实现）。
    let mut all_evidence = true;
    let mut k = 0usize;
    while k < DETERMINISM_AUDIT.len() {
        if DETERMINISM_AUDIT[k].evidence.is_empty() {
            all_evidence = false;
        }
        k += 1;
    }
    set.add(
        "C19-AUD-02 审计证据句逐行非空",
        all_evidence,
        "空证据=没审计；审计的价值在证据可读可查",
    );
}

// ---------------------------------------------------------------------------
// 跨平台（算子面）：白名单 / 违禁表
// ---------------------------------------------------------------------------

fn group_platform(set: &mut CheckSet) {
    // ③ 平台审计绿：两表非空、不相交、多项式替代在册。
    set.add(
        "C19-PLT-01 平台审计绿（白名单/违禁表不相交）",
        platform_audit().is_ok(),
        "白名单混入违禁算子=跨硬件摘要可分叉——两表不相交是硬判据",
    );

    // ④ 关键替代在册：sin_poly 在白名单、原生 sin 在违禁表（双向实测）。
    let mut poly_safe = false;
    let mut native_banned = false;
    let mut k = 0usize;
    while k < PLATFORM_SAFE_OPS.len() {
        if PLATFORM_SAFE_OPS[k].contains("sin_poly") {
            poly_safe = true;
        }
        k += 1;
    }
    let mut k = 0usize;
    while k < PLATFORM_BANNED_OPS.len() {
        if PLATFORM_BANNED_OPS[k].contains("f32::sin") {
            native_banned = true;
        }
        k += 1;
    }
    set.add(
        "C19-PLT-02 多项式替代在册且原生超越函数在违禁表",
        poly_safe && native_banned,
        "F1605 的取舍是实现保障不是技巧：替代缺席=跨平台一致性失去物质前提",
    );

    // ⑤ LE 字节序算子在白名单（字节级一致的传输底线）。
    let mut le_ok = false;
    let mut k = 0usize;
    while k < PLATFORM_SAFE_OPS.len() {
        if PLATFORM_SAFE_OPS[k].contains("to_le_bytes") || PLATFORM_SAFE_OPS[k].contains("from_le_bytes") {
            le_ok = true;
        }
        k += 1;
    }
    set.add(
        "C19-PLT-03 字节序显式算子在册",
        le_ok,
        "字节级确定=同一位型同一条线：LE 显式化是底线",
    );
}

// ---------------------------------------------------------------------------
// 双跑：量化双跑 + 脏化再跑 + 重排双跑
// ---------------------------------------------------------------------------

fn group_drun(set: &mut CheckSet) {
    // ⑥ 全语料双跑绿且摘要非零（非零=摘要真的在算字节）。
    let mut all_ok = CORPUS.len() >= 2;
    let mut k = 0usize;
    while k < CORPUS.len() {
        match double_run(&CORPUS[k]) {
            Ok(d) => {
                if d == 0 {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        k += 1;
    }
    set.add(
        "C19-DRN-01 全语料量化双跑一致且摘要非零",
        all_ok,
        "双跑一致是 F1605 确定性纪律的执行面验收",
    );

    // ⑦ 脏化再跑：先跑噪声语料再跑目标，摘要与干净跑一致（无共享态实测）。
    let noise_ok = double_run_after_noise(&CORPUS[CORPUS.len() - 1]).is_ok()
        && double_run_after_noise(&CORPUS[0]).is_ok();
    set.add(
        "C19-DRN-02 脏化再跑与干净跑一致（跑序无关）",
        noise_ok,
        "无共享态不是声明：污染任何假想静态后结果必须不变",
    );

    // ⑧ 不同量化档位的摘要互异（防恒绿：摘要恒等=对拍空转）。
    let d12 = double_run(&CORPUS[0]).unwrap_or(0);
    let d16 = double_run(&CORPUS[1]).unwrap_or(0);
    set.add(
        "C19-DRN-03 不同档位语料摘要互异（对拍非空转）",
        d12 != d16,
        "档位不同量化输出必不同——摘要相同则摘要函数或管线是恒等的",
    );

    // ⑨ 重排双跑一致（F1609 确定性纪律落地：索引域纯函数）。
    let verts = [
        [0.0f32, 0.0, 0.0],
        [1.0f32, 0.0, 0.0],
        [1.0f32, 1.0, 0.0],
        [0.0f32, 1.0, 0.0],
    ];
    let faces = [[0u32, 1, 2], [0, 2, 3]];
    set.add(
        "C19-DRN-04 Forsyth 重排双跑面序一致",
        reorder_double_run(&verts, &faces, 16).is_ok(),
        "重排不确定=顶点流水线的输入顺序漂移——下游缓存行为不可复现",
    );
}

// ---------------------------------------------------------------------------
// 跨版本：v1 解码语义冻结
// ---------------------------------------------------------------------------

fn group_xver(set: &mut CheckSet) {
    // ⑩ 版本承诺审计绿 + 版本号守恒 v1。
    set.add(
        "C19-XVR-01 版本承诺审计绿且版本守恒 v1",
        version_pledge_audit().is_ok() && VMESH_FORMAT_VERSION == 1,
        "版本演进只增不改：v1 语义保留是老文件可读的前提",
    );

    // ⑪ 老文件摘要与判据侧独立 FNV 相等（同标准两份实现互证）。
    //    判据侧按 F1617 规范手写 v1 参考流（56 字节布局）再以 ref_fnv 摘要。
    let mut stream: Vec<u8> = Vec::new();
    stream.extend_from_slice(&3u32.to_le_bytes());
    stream.extend_from_slice(&1u32.to_le_bytes());
    let vs = [[0.0f32, 0.0, 0.0], [1.0f32, 0.0, 0.0], [0.0f32, 1.0, 0.0]];
    let mut i = 0usize;
    while i < 3 {
        stream.extend_from_slice(&vs[i][0].to_le_bytes());
        stream.extend_from_slice(&vs[i][1].to_le_bytes());
        stream.extend_from_slice(&vs[i][2].to_le_bytes());
        i += 1;
    }
    stream.extend_from_slice(&0u32.to_le_bytes());
    stream.extend_from_slice(&1u32.to_le_bytes());
    stream.extend_from_slice(&2u32.to_le_bytes());
    let mine = ref_fnv(&stream);
    let prod = legacy_reference_digest();
    set.add(
        "C19-XVR-02 老文件参考摘要与判据侧独立重算相等",
        stream.len() == 56 && mine == prod && prod != 0,
        "摘要漂移=v1 解码语义被改=历史资产作废——钉在 CI 里等它",
    );

    // ⑬ 承诺条款三态：行数守恒 + 每条非空（文本是契约的一部分）。
    let mut ok = VERSION_PLEDGE.len() == 3;
    let mut k = 0usize;
    while k < VERSION_PLEDGE.len() {
        if VERSION_PLEDGE[k].is_empty() {
            ok = false;
        }
        k += 1;
    }
    set.add(
        "C19-XVR-03 版本承诺三条款闭集且全非空",
        ok,
        "空条款=没承诺；条款数守恒使增删条款必须过判据",
    );

    // ⑭ 老文件读出一致：参考流经产线解析后重编码，摘要仍等于参考摘要
    //    （解码-重编码闭环不变——「读出一致结果」的可执行化）。
    let round_ok = match crate::svstar2::vef18_geofuzz::parse_mesh_stream(&stream) {
        crate::svstar2::vef18_geofuzz::MeshStreamOutcome::Ok(m) => {
            let re = crate::svstar2::vef18_geofuzz::encode_mesh_stream(&m);
            ref_fnv(&re) == prod
        }
        _ => false,
    };
    set.add(
        "C19-XVR-04 老文件解码-重编码闭环摘要不变",
        round_ok,
        "格式演进后的最小承诺：v1 流读出再写回，一个字节都不许变",
    );
}

// ---------------------------------------------------------------------------
// 判据：收口自检
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑮ 实挂条数从 CheckSet 实取：前四族 13 条，META 段 2 条，合计 15。
    let before_meta = set.len();
    set.add(
        "C19-META-01 实挂条数+2(META)=声明条数15",
        before_meta == 13 && before_meta + 2 == 15,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑯ 判据名全集互异（重名=聚合器 tally 失真）。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C19-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 15,
        "重名判据让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
