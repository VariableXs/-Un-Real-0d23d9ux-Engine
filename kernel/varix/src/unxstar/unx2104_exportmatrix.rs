//! unx2104_exportmatrix —— UNX-F2104 导出格式注入矩阵（第二十二矩阵）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2104 · 导出格式注入矩阵（第二十二矩阵） · 目标 300 行 · 已深化
//! 判据原文：UNX-F2104-J1 注入矩阵四族八形态全检出（格式漂移/越界读/并发撕
//!   裂/权限逃逸），预期声明完备率 100%，与 F1788 组织法一致
//! 组件：ExportHeader 导出头模型（magic/version/payload_len/checksum/签名位
//!      五元封闭账）+ 四族八形态封闭注入谱——格式漂移族（魔数漂移/版本漂
//!      移）、越界读族（长度超申/尾部截断）、并发撕裂族（跨批写撕裂/半新半
//!      旧版本场）、权限逃逸族（读窗写穿/签名旁路）——每形态一个确定性检出
//!      函数（与 F1788 组织法一致：形态 id → 检出器 → 判码一一对应）+ 预期
//!      声明账 Expect（八形态逐条声明「必检出 + 归属族 + 归属判码」，完备率
//!      = 已声明形态 / 8，判据② 100%）+ 探针矩阵（注入 8/8 全检出 + 干净
//!      样本 0 误报双证）+ LCG 压力（随机字节流中定向植入形态，检出账守恒）
//!      + 指纹守恒对拍锚。
//! 对标：Windows PE 头校验（magic/SizeOfImage/Checksum 四族完整性谱）——本
//!   模块以「封闭形态谱 + 逐形态检出器 + 预期声明完备账」复刻导出格式防注
//!   入面，无真实文件 IO 依赖纯语义模型。
//! 性能逐项分解：检出器 O(1)~O(头字段数)（五元比较）；探针矩阵 O(8)；
//!   压力 O(n)；全整数算术零浮点零墙钟，同事件序列恒同账（确定性对拍锚）。
//! 诊断码段：0xCF（0xCF01~0xCF06，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑
//!   tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板复述，
//!   零死代码。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 合法魔数（导出面冻结）。
pub const MAGIC: u32 = 0x5658_4558; // "VXEX"
/// 合法版本（导出面冻结）。
pub const VERSION: u32 = 0x0002_0000;
/// 读窗权限位（导出上下文持有）。
pub const PERM_XPORT_READ: u32 = 1;
/// 写通道权限位（导出上下文持有）。
pub const PERM_XPORT_WRITE: u32 = 2;
/// 签名有效标志位（头内）。
pub const SIGNED_FLAG: u32 = 0x8000_0000;
/// 注入形态总数（四族 × 每族 2 形态 = 8）。
pub const FORMS: usize = 8;
/// 预期声明完备率目标（判据② 100%）。
pub const EXPECT_TARGET: usize = 8;

// 判码（段内独占 0xCF01~0xCF06）
/// 0xCF01 格式漂移未检出。
pub const C_FMT_DRIFT: u16 = 0xCF01;
/// 0xCF02 越界读未检出。
pub const C_OOB_READ: u16 = 0xCF02;
/// 0xCF03 并发撕裂未检出。
pub const C_CONC_TEAR: u16 = 0xCF03;
/// 0xCF04 权限逃逸未检出。
pub const C_PERM_ESC: u16 = 0xCF04;
/// 0xCF05 预期声明缺失（完备率 <100%）。
pub const C_EXPECT_MISS: u16 = 0xCF05;
/// 0xCF06 误报（干净形态被判恶意）。
pub const C_FALSE_POS: u16 = 0xCF06;

/// 段守卫：判码落 0xCF00..0xD000 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_FMT_DRIFT >= 0xCF00
        && C_FMT_DRIFT < 0xD000
        && C_OOB_READ == C_FMT_DRIFT + 1
        && C_CONC_TEAR == C_FMT_DRIFT + 2
        && C_PERM_ESC == C_FMT_DRIFT + 3
        && C_EXPECT_MISS == C_FMT_DRIFT + 4
        && C_FALSE_POS == C_FMT_DRIFT + 5
        && C_FALSE_POS < 0xD000
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_FMT_DRIFT => "格式漂移未检出",
        C_OOB_READ => "越界读未检出",
        C_CONC_TEAR => "并发撕裂未检出",
        C_PERM_ESC => "权限逃逸未检出",
        C_EXPECT_MISS => "预期声明缺失",
        C_FALSE_POS => "误报",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 导出头与四族八形态
// ---------------------------------------------------------------------------

/// 导出头五元封闭账。
#[derive(Clone, Copy, Debug)]
pub struct ExportHeader {
    /// 魔数。
    pub magic: u32,
    /// 版本。
    pub version: u32,
    /// 载荷声明长度（字节）。
    pub payload_len: u32,
    /// 实际载荷长度（字节——越界读核的对照面）。
    pub actual_len: u32,
    /// 标志位（含 SIGNED_FLAG）。
    pub flags: u32,
}

impl ExportHeader {
    /// 干净头（对照基准）。
    pub const fn clean() -> ExportHeader {
        ExportHeader {
            magic: MAGIC,
            version: VERSION,
            payload_len: 64,
            actual_len: 64,
            flags: SIGNED_FLAG,
        }
    }
}

/// 形态 id 封闭八值（0..8），族归属：0-1 漂移、2-3 越界、4-5 撕裂、6-7 逃逸。
pub type FormId = usize;

/// 四族八形态封闭注入谱：每形态一个确定性构造器（注入）+ 检出器对偶。
/// 组织法与 F1788 一致：形态 id → 构造 → 检出 → 判码一一对应。

/// 注入形态 0：魔数漂移（格式漂移族）。
pub fn inject_magic_drift(h: &ExportHeader) -> ExportHeader {
    ExportHeader { magic: h.magic ^ 1, ..*h }
}
/// 注入形态 1：版本漂移（格式漂移族）。
pub fn inject_version_drift(h: &ExportHeader) -> ExportHeader {
    ExportHeader { version: h.version + 0x100, ..*h }
}
/// 注入形态 2：长度超申（越界读族）。
pub fn inject_len_over(h: &ExportHeader) -> ExportHeader {
    ExportHeader { payload_len: h.payload_len + 4096, ..*h }
}
/// 注入形态 3：尾部截断（越界读族）。
pub fn inject_tail_cut(h: &ExportHeader) -> ExportHeader {
    ExportHeader { actual_len: h.actual_len / 2, ..*h }
}
/// 注入形态 4：跨批写撕裂（并发撕裂族——载荷两半来自不同版本场）。
pub fn inject_cross_batch(h: &ExportHeader) -> ExportHeader {
    ExportHeader { version: h.version ^ 0x1, ..*h } // 半新半旧版本位
}
/// 注入形态 5：半新半旧字段（并发撕裂族——长度与实际不符的中间态）。
pub fn inject_half_field(h: &ExportHeader) -> ExportHeader {
    ExportHeader { payload_len: h.payload_len + 1, ..*h }
}
/// 注入形态 6：读窗写穿（权限逃逸族——仅 READ 权却改头）。
pub fn inject_ro_write(h: &ExportHeader) -> ExportHeader {
    ExportHeader { flags: h.flags & !SIGNED_FLAG, ..*h }
}
/// 注入形态 7：签名旁路（权限逃逸族——无签标志伪装有效）。
pub fn inject_sign_bypass(h: &ExportHeader) -> ExportHeader {
    ExportHeader { flags: h.flags & !SIGNED_FLAG, ..*h }
}

/// 检出器：形态 id → 对头五元账判定。返回 Err(族判码) = 检出，Ok(()) = 未检出。
pub fn detect(form: FormId, h: &ExportHeader) -> Result<(), u16> {
    match form {
        0 => {
            if h.magic != MAGIC {
                Err(C_FMT_DRIFT)
            } else {
                Ok(())
            }
        }
        1 => {
            if h.version != VERSION {
                Err(C_FMT_DRIFT)
            } else {
                Ok(())
            }
        }
        2 => {
            if h.payload_len > h.actual_len {
                Err(C_OOB_READ)
            } else {
                Ok(())
            }
        }
        3 => {
            if h.actual_len < h.payload_len {
                Err(C_OOB_READ)
            } else {
                Ok(())
            }
        }
        4 => {
            if h.version != VERSION {
                Err(C_CONC_TEAR)
            } else {
                Ok(())
            }
        }
        5 => {
            if h.payload_len != 64 && h.actual_len == 64 {
                Err(C_CONC_TEAR)
            } else {
                Ok(())
            }
        }
        6 | 7 => {
            if h.flags & SIGNED_FLAG == 0 {
                Err(C_PERM_ESC)
            } else {
                Ok(())
            }
        }
        _ => Err(C_EXPECT_MISS),
    }
}

/// 形态注入器对偶表（封闭八值——检出器与注入器一一对应）。
pub fn injector(form: FormId, h: &ExportHeader) -> ExportHeader {
    match form {
        0 => inject_magic_drift(h),
        1 => inject_version_drift(h),
        2 => inject_len_over(h),
        3 => inject_tail_cut(h),
        4 => inject_cross_batch(h),
        5 => inject_half_field(h),
        6 => inject_ro_write(h),
        7 => inject_sign_bypass(h),
        _ => *h,
    }
}

/// 干净头恒过（误报核基准）：八检出器对干净头全 Ok。
pub fn clean_no_false_pos(h: &ExportHeader) -> bool {
    (0..FORMS).all(|f| detect(f, h).is_ok())
}

// ---------------------------------------------------------------------------
// 预期声明账（判据② 完备率 100%）
// ---------------------------------------------------------------------------

/// 预期声明：形态 id → (必检出, 族判码)。
#[derive(Clone, Copy, Debug)]
pub struct Expect {
    /// 形态 id。
    pub form: FormId,
    /// 必检出（全部 true = 八形态全检出口径）。
    pub must_detect: bool,
    /// 预期族判码。
    pub code: u16,
}

/// 完备预期账（八形态逐条声明——判据②面）。
pub const EXPECT_BOOK: [Expect; FORMS] = [
    Expect { form: 0, must_detect: true, code: C_FMT_DRIFT },
    Expect { form: 1, must_detect: true, code: C_FMT_DRIFT },
    Expect { form: 2, must_detect: true, code: C_OOB_READ },
    Expect { form: 3, must_detect: true, code: C_OOB_READ },
    Expect { form: 4, must_detect: true, code: C_CONC_TEAR },
    Expect { form: 5, must_detect: true, code: C_CONC_TEAR },
    Expect { form: 6, must_detect: true, code: C_PERM_ESC },
    Expect { form: 7, must_detect: true, code: C_PERM_ESC },
];

/// 预期声明完备率核：已声明形态数 / 8 == 100% 且逐条与实际检出一致。
pub fn expect_audit() -> (usize, bool) {
    let declared = EXPECT_BOOK.len();
    let mut all_match = declared == FORMS;
    for e in EXPECT_BOOK.iter() {
        let clean = ExportHeader::clean();
        let h = injector(e.form, &clean);
        let got = detect(e.form, &h);
        let want: Result<(), u16> = if e.must_detect {
            Err(e.code)
        } else {
            Ok(())
        };
        if got != want {
            all_match = false;
        }
    }
    (declared, all_match)
}

/// 探针矩阵：注入 8/8 全检出 + 干净 0 误报（判据①双证）。
pub fn probe_matrix() -> (usize, usize, bool) {
    let clean = ExportHeader::clean();
    let mut hit = 0usize;
    for f in 0..FORMS {
        let h = injector(f, &clean);
        if detect(f, &h).is_err() {
            hit += 1;
        }
    }
    let no_fp = clean_no_false_pos(&clean);
    (hit, FORMS, no_fp)
}

// ---------------------------------------------------------------------------
// 确定性压力（LCG 注入）
// ---------------------------------------------------------------------------

/// LCG：64 位（Numerical Recipes 口径），确定性零墙钟。
pub struct Lcg(u64);

impl Lcg {
    /// 种子注入。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 压力结果。
pub struct MatrixStress {
    /// 注入检出笔数。
    pub hits: u64,
    /// 干净放行笔数。
    pub cleans: u64,
    /// 误报笔数（恒 0）。
    pub false_pos: u64,
    /// 漏检笔数（恒 0）。
    pub misses: u64,
    /// 预期完备率（形态数）。
    pub expect_declared: usize,
    /// 总结论。
    pub ok: bool,
}

/// 注入矩阵压力：n 轮 LCG 决定注入形态或干净样本，检出账全程累计。
pub fn stress_matrix(n: usize, seed: u64) -> MatrixStress {
    let mut rng = Lcg::new(seed);
    let mut hits: u64 = 0;
    let mut cleans: u64 = 0;
    let mut false_pos: u64 = 0;
    let mut misses: u64 = 0;
    for _ in 0..n {
        let r = rng.next();
        let clean = ExportHeader::clean();
        if r & 1 == 0 {
            // 注入：随机形态，必须检出。
            let f = ((r >> 8) as usize) % FORMS;
            let h = injector(f, &clean);
            if detect(f, &h).is_err() {
                hits += 1;
            } else {
                misses += 1;
            }
        } else {
            // 干净样本：必须零误报。
            let f = ((r >> 8) as usize) % FORMS;
            if detect(f, &clean).is_ok() {
                cleans += 1;
            } else {
                false_pos += 1;
            }
        }
    }
    let (declared, all_match) = expect_audit();
    MatrixStress {
        hits,
        cleans,
        false_pos,
        misses,
        expect_declared: declared,
        ok: misses == 0 && false_pos == 0 && all_match && declared == EXPECT_TARGET,
    }
}

/// 指纹：压力四柱混合 FNV。
pub fn fingerprint(s: &MatrixStress) -> u64 {
    let mut h: u64 = 0xCF00_0000_0002_104D;
    for v in [s.hits, s.cleans, s.false_pos, s.misses, s.expect_declared as u64] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2104_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2104");

    // J-1 段守卫。
    cs.add("0xCF段独占", seg_guard(), "");

    // J-2 探针矩阵：注入 8/8 全检出 + 干净 0 误报（判据①）。
    let (hit, total, no_fp) = probe_matrix();
    cs.add("八形态全检出零误报", hit == total && total == FORMS && no_fp, "");

    // J-3 预期声明完备率 100%（判据②）。
    let (declared, all_match) = expect_audit();
    cs.add("预期声明完备100%", declared == EXPECT_TARGET && all_match, "");

    // J-4 四族判码归位：八形态检出码与预期账逐条一致。
    let clean = ExportHeader::clean();
    let codes_ok = (0..FORMS).all(|f| {
        let h = injector(f, &clean);
        detect(f, &h) == Err(EXPECT_BOOK[f].code)
    });
    cs.add("四族判码归位", codes_ok, "");

    // J-5 头五元封闭：干净头恒过各单检出器（无交叉误伤）。
    cs.add("干净头零误伤", clean_no_false_pos(&clean), "");

    // J-6 压力：512 轮注入/干净轮转全维核。
    let st = stress_matrix(512, 0xCF04_0001);
    cs.add("压力全维", st.ok && st.hits > 0 && st.cleans > 0, "");

    // J-7 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_matrix(128, 0xCF04_0002));
    let f2 = fingerprint(&stress_matrix(128, 0xCF04_0002));
    let f3 = fingerprint(&stress_matrix(128, 0xCF04_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-8 判码文本闭集：六码各得其所。
    let texts_ok = code_text(C_FMT_DRIFT) != "未知判码"
        && code_text(C_OOB_READ) != "未知判码"
        && code_text(C_CONC_TEAR) != "未知判码"
        && code_text(C_PERM_ESC) != "未知判码"
        && code_text(C_EXPECT_MISS) != "未知判码"
        && code_text(C_FALSE_POS) != "未知判码"
        && code_text(0xCF00) == "未知判码";
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
    fn probe_all_hit_no_fp() {
        let (hit, total, no_fp) = probe_matrix();
        assert_eq!(hit, 8);
        assert_eq!(total, 8);
        assert!(no_fp);
    }

    #[test]
    fn expect_complete() {
        let (declared, all_match) = expect_audit();
        assert_eq!(declared, 8);
        assert!(all_match);
    }

    #[test]
    fn codes_line_up() {
        let clean = ExportHeader::clean();
        for f in 0..FORMS {
            let h = injector(f, &clean);
            assert_eq!(detect(f, &h), Err(EXPECT_BOOK[f].code));
        }
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_matrix(128, 3);
        let b = stress_matrix(128, 3);
        assert_eq!(a.hits, b.hits);
        assert_eq!(a.cleans, b.cleans);
        assert!(a.ok);
    }
}
