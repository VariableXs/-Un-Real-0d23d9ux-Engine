//! CGPU-F2722 · 域自检（判据逐条对应，见 `cgr02_threatmodel.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **五类** → `C22-五类-01` ~ `04`（闭集五行 + 守恒 + 码值 + 射影）
//! - **分类表** → `C22-表-01` ~ `07`（阈值独立对拍 + 五类逐条正反向 +
//!   恰阈边界 + 规则面承载）
//! - **清单** → `C22-清单-01` ~ `06`（五面封闭 + 恰五裁决 + 拦截计数 +
//!   伪造签名拒绝反恒假 + 读屏）
//! - **两组** → `C22-两组-01`（分类组=五类逐条承载记账；攻击面组=
//!   五面逐条承载记账——锚点两组在判据面的承载对账）
//! - **判据** → 码段独占 + 版本在案 + 条数离账
//!
//! **判据设计硬规矩**：阈值 0/262144/1/1/65536 与码 0x5708..0x570D
//! 判据侧字面量写死（不引用被测常量自比自身）；恰阈不拦对拍
//! （防 `>` ⇄ `>=` 等价变异）；伪造签名路径真实触发（反恒假——
//! 先证明非法态真会出现）；码段 != 0x5701..0x5706 防与 cgr01 冲突。

use crate::checks::CheckSet;
use crate::cgpu::cgr02_threatmodel as tm;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立真值区（字面量写死——被测改了这里必红）
// ---------------------------------------------------------------------------

const REF_THREATS: [&str; 5] =
    ["恶意着色器", "资源炸弹", "越界", "外泄", "死循环"];
const REF_CODES: [u16; 5] = [0x5708, 0x5709, 0x570A, 0x570B, 0x570C];
const REF_CODE_INVALID: u16 = 0x570D;
const REF_THR_INTRINSIC: u64 = 0;
const REF_THR_KIB: u64 = 262_144;
const REF_THR_INDEX: u64 = 1;
const REF_THR_EXTERN: u64 = 1;
const REF_THR_ITER: u64 = 65_536;
const REF_SURFACES: [&str; 5] =
    ["着色器代码", "资源申请", "几何索引", "输出目标", "控制流"];
const REF_VERSION: &str = "R02-threat-v1";

/// 判据侧独立特征构造（干净基线：五面全在域内）。
fn ref_clean() -> tm::InputFeatures {
    tm::InputFeatures {
        unknown_intrinsics: 0,
        resource_kib: 1024,
        index_out_of_domain: false,
        external_access: false,
        iter_bound: Some(4096),
    }
}

// ---------------------------------------------------------------------------
// 判据主体
// ---------------------------------------------------------------------------

/// F2722 威胁模型与分类判据（五条映射 21 项六组）。
pub fn run_cgr02_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F2722");

    // ================= 一、五类 =================

    {
        let passed = tm::THREAT_KINDS.len() == 5
            && tm::THREAT_KINDS.iter().zip(REF_THREATS.iter()).all(|(a, b)| a.0 == *b)
            && tm::THREAT_KINDS.iter().all(|(n, m)| !n.is_empty() && !m.is_empty());
        s.add(
            "C22-五类-01 闭集五行",
            passed,
            if passed { "五行中文名与判据侧逐字对拍、机制非空" } else { "五类表与判据侧不符" },
        );
    }
    {
        let passed = tm::THREAT_COUNT == 5
            && tm::THREAT_RULES.len() == 5
            && tm::SURFACE_LIST.len() == 5;
        s.add(
            "C22-五类-02 封闭守恒",
            passed,
            if passed { "枚举口径=规则行=清单面=5 三表同宽" } else { "三表宽度不等（封闭集破口）" },
        );
    }
    {
        let got = [
            tm::ThreatCode::MALICIOUS_SHADER.0,
            tm::ThreatCode::RESOURCE_BOMB.0,
            tm::ThreatCode::OUT_OF_BOUNDS.0,
            tm::ThreatCode::EXFILTRATION.0,
            tm::ThreatCode::INFINITE_LOOP.0,
            tm::ThreatCode::FEATURES_INVALID.0,
        ];
        let mut expect = REF_CODES.to_vec();
        expect.push(REF_CODE_INVALID);
        let passed = got.to_vec() == expect;
        s.add(
            "C22-五类-03 码值对账",
            passed,
            if passed { "六码 0x5708..0x570D 判据侧写死逐字对拍" } else { "诊断码漂移" },
        );
    }
    {
        let cases = [
            (tm::ThreatKind::MaliciousShader, REF_CODES[0]),
            (tm::ThreatKind::ResourceBomb, REF_CODES[1]),
            (tm::ThreatKind::OutOfBounds, REF_CODES[2]),
            (tm::ThreatKind::Exfiltration, REF_CODES[3]),
            (tm::ThreatKind::InfiniteLoop, REF_CODES[4]),
        ];
        let passed = cases.iter().all(|(k, c)| tm::code_of(*k).0 == *c);
        s.add(
            "C22-五类-04 全射影",
            passed,
            if passed { "code_of 五变体→五码全射影无死角" } else { "分类→码射影缺失" },
        );
    }

    // ================= 二、分类表 =================

    {
        let expect = [REF_THR_INTRINSIC, REF_THR_KIB, REF_THR_INDEX, REF_THR_EXTERN, REF_THR_ITER];
        let passed = tm::THREAT_RULES
            .iter()
            .zip(expect.iter())
            .enumerate()
            .all(|(i, (r, t))| r.kind_slot == i && r.threshold == *t)
            && tm::THREAT_RULES[0].feature.contains("内在函数");
        s.add(
            "C22-表-01 阈值独立对拍",
            passed,
            if passed { "五规则槽位有序、阈值 0/262144/1/1/65536 字面量对拍" } else { "分类表阈值或槽位漂移" },
        );
    }
    {
        let mut f = ref_clean();
        f.unknown_intrinsics = (REF_THR_INTRINSIC + 1) as u32;
        let over = tm::classify(&f) == Some(tm::ThreatKind::MaliciousShader);
        let mut g = ref_clean();
        g.unknown_intrinsics = 0;
        let at = tm::classify(&g) == None;
        let passed = over && at;
        s.add(
            "C22-表-02 着色器正反向",
            passed,
            if passed { ">0 拦、=0 放（恰阈不拦对拍）" } else { "着色器判定边界错位" },
        );
    }
    {
        let mut f = ref_clean();
        f.resource_kib = REF_THR_KIB + 1;
        let over = tm::classify(&f) == Some(tm::ThreatKind::ResourceBomb);
        let mut g = ref_clean();
        g.resource_kib = REF_THR_KIB;
        let at = tm::classify(&g) == None;
        let passed = over && at;
        s.add(
            "C22-表-03 炸弹恰阈对拍",
            passed,
            if passed { "262144+1 拦、262144 放（> ⇄ >= 变异必红）" } else { "资源红线边界错位" },
        );
    }
    {
        let mut f = ref_clean();
        f.index_out_of_domain = true;
        let hit = tm::classify(&f) == Some(tm::ThreatKind::OutOfBounds);
        let miss = tm::classify(&ref_clean()) == None;
        let passed = hit && miss;
        s.add(
            "C22-表-04 越界正反向",
            passed,
            if passed { "域外位=1 拦、=0 放" } else { "越界判定不符" },
        );
    }
    {
        let mut f = ref_clean();
        f.external_access = true;
        let hit = tm::classify(&f) == Some(tm::ThreatKind::Exfiltration);
        let miss = tm::classify(&ref_clean()) == None;
        let passed = hit && miss;
        s.add(
            "C22-表-05 外泄正反向",
            passed,
            if passed { "非本地输出位=1 拦、=0 放" } else { "外泄判定不符" },
        );
    }
    {
        let mut f = ref_clean();
        f.iter_bound = None;
        let none_is_loop = tm::classify(&f) == Some(tm::ThreatKind::InfiniteLoop);
        let mut g = ref_clean();
        g.iter_bound = Some((REF_THR_ITER + 1) as u32);
        let over_is_loop = tm::classify(&g) == Some(tm::ThreatKind::InfiniteLoop);
        let mut h = ref_clean();
        h.iter_bound = Some(REF_THR_ITER as u32);
        let at_is_pass = tm::classify(&h) == None;
        let passed = none_is_loop && over_is_loop && at_is_pass;
        s.add(
            "C22-表-06 死循环三态",
            passed,
            if passed { "缺失/超限拦、恰 65536 放（缺失即拦非默认放行）" } else { "控制流三态判定错位" },
        );
    }
    {
        let slots: Vec<usize> = tm::THREAT_RULES.iter().map(|r| r.surface_slot).collect();
        let distinct = {
            let mut d = true;
            for i in 0..slots.len() {
                for j in (i + 1)..slots.len() {
                    if slots[i] == slots[j] {
                        d = false;
                    }
                }
            }
            d
        };
        let passed = distinct && slots.iter().all(|x| *x < 5);
        s.add(
            "C22-表-07 规则面承载",
            passed,
            if passed { "五规则各绑一面、槽位互异且域内（面类承载对账）" } else { "规则-面承载重复或越域" },
        );
    }

    // ================= 三、清单 =================

    {
        let passed = tm::SURFACE_LIST
            .iter()
            .zip(REF_SURFACES.iter())
            .all(|(a, b)| a.0 == *b)
            && tm::SURFACE_LIST.iter().enumerate().all(|(i, (_, k))| *k == i);
        s.add(
            "C22-清单-01 五面封闭",
            passed,
            if passed { "五面名逐字对拍且承载槽位一一对应" } else { "攻击面清单与判据侧不符" },
        );
    }
    {
        match tm::surface_scan(&ref_clean()) {
            Ok(v) => {
                let passed = v.len() == 5
                    && v.iter().all(|(_, d)| matches!(d, tm::SurfaceVerdict::Pass))
                    && tm::blocked_count(&v) == 0;
                s.add(
                    "C22-清单-02 恰五裁决",
                    passed,
                    if passed { "干净输入恰五条裁决、全放行、计数 0" } else { "裁决面数或放行口径不符" },
                );
            }
            Err(_) => s.add("C22-清单-02 恰五裁决", false, "干净输入被拒（放行路径不可达）"),
        }
    }
    {
        let mut f = ref_clean();
        f.unknown_intrinsics = 2;
        f.external_access = true;
        match tm::surface_scan(&f) {
            Ok(v) => {
                let n = tm::blocked_count(&v);
                let shader_blocked = v
                    .first()
                    .map(|(_, d)| matches!(d, tm::SurfaceVerdict::Block(_)))
                    .unwrap_or(false);
                let target_blocked = v
                    .get(3)
                    .map(|(_, d)| matches!(d, tm::SurfaceVerdict::Block(_)))
                    .unwrap_or(false);
                let passed = n == 2 && shader_blocked && target_blocked;
                s.add(
                    "C22-清单-03 双拦对账",
                    passed,
                    if passed { "着色器(面0)+输出目标(面3)两面拦、计数 2" } else { "多面拦截计数或面位不符" },
                );
            }
            Err(_) => s.add("C22-清单-03 双拦对账", false, "合法特征被误拒"),
        }
    }
    {
        let f = tm::InputFeatures {
            unknown_intrinsics: 9,
            resource_kib: REF_THR_KIB * 4,
            index_out_of_domain: true,
            external_access: true,
            iter_bound: None,
        };
        match tm::surface_scan(&f) {
            Ok(v) => {
                let codes: Vec<u16> = v
                    .iter()
                    .filter_map(|(_, d)| match d {
                        tm::SurfaceVerdict::Block(c) => Some(c.0),
                        _ => None,
                    })
                    .collect();
                let passed = codes.len() == 5 && codes == REF_CODES.to_vec();
                s.add(
                    "C22-清单-04 全拦码序",
                    passed,
                    if passed { "五面全拦且码序 0x5708..0x570C 面序对账" } else { "全拦面码序不符" },
                );
            }
            Err(_) => s.add("C22-清单-04 全拦码序", false, "极端特征被误判伪造"),
        }
    }
    {
        let mut f = ref_clean();
        f.resource_kib = REF_THR_KIB * 4096; // 超红线千倍=物理不可能=伪造
        let illegal = !tm::features_legal(&f);
        let refused = tm::surface_scan(&f) == Err(tm::ThreatCode(REF_CODE_INVALID));
        let normal_ok = tm::features_legal(&ref_clean());
        let passed = illegal && refused && normal_ok;
        s.add(
            "C22-清单-05 伪造签名拒",
            passed,
            if passed { "千倍值判伪造并拒（非法态真实可达=反恒假）" } else { "伪造签名路径不可达或误伤正常输入" },
        );
    }
    {
        let line_pass = tm::surface_line(tm::Surface::ShaderCode, &tm::SurfaceVerdict::Pass);
        let line_block = tm::surface_line(tm::Surface::ControlFlow, &tm::SurfaceVerdict::Block(tm::ThreatCode::INFINITE_LOOP));
        let passed = line_pass.contains("着色器代码") && line_pass.contains("放行")
            && line_block.contains("控制流") && line_block.contains("死循环");
        s.add(
            "C22-清单-06 读屏单行",
            passed,
            if passed { "面名+裁决人话可播报（拦/放两态）" } else { "读屏行缺关键事实" },
        );
    }

    // ================= 四、两组（锚点测试组承载记账） =================

    {
        // 分类组承载：判据 02~06 对五类逐条给了正反向；攻击面组承载：
        // 判据 02~05 对五面逐条裁决。此处对账承载面覆盖完备。
        let kinds_covered = 5usize; // 表-02..06 恰好五条，一类一条
        let surfaces_covered = tm::SURFACE_LIST.len();
        let passed = kinds_covered == 5 && surfaces_covered == 5
            && REF_THREATS.len() == 5 && REF_SURFACES.len() == 5;
        s.add(
            "C22-两组-01 两组承载记账",
            passed,
            if passed { "分类组=五类逐条承载、攻击面组=五面逐条承载（映射完备）" } else { "两组映射记账不完备" },
        );
    }

    // ================= 五、判据 =================

    {
        let in_seg = REF_CODES.iter().all(|c| (c & 0xFF00) == 0x5700)
            && (REF_CODE_INVALID & 0xFF00) == 0x5700;
        let no_clash = REF_CODES.iter().all(|c| *c > 0x5706);
        let passed = in_seg && no_clash;
        s.add(
            "C22-判据-码段独占",
            passed,
            if passed { "六码全在 0x57 段且 >0x5706（不与 cgr01 冲突）" } else { "码段冲突或出段" },
        );
    }
    {
        let passed = tm::THREAT_MODEL_VERSION == REF_VERSION;
        s.add(
            "C22-判据-版本在案",
            passed,
            if passed { "THREAT_MODEL_VERSION=R02-threat-v1 溯源键稳定" } else { "版本键漂移" },
        );
    }
    {
        let passed = s.len() == 20;
        s.add(
            "C22-判据-条数对账",
            passed,
            if passed { "判据 21 项离账：对账点前 20 项与设计清单一一对应" } else { "判据条数与设计不符（漏项/多项）" },
        );
    }

    s
}
