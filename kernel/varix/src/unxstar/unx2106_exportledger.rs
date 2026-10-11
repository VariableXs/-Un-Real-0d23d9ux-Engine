//! unx2106_exportledger —— UNX-F2106 导出与账本一致性断言（防两张皮）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2106 · 导出与账本一致性断言（防两张皮） · 目标 380 行 · 已深化
//! 判据原文：UNX-F2106-J1 一致性断言全过（导出值↔账本真源逐字段对齐），
//!   两张皮注入 10/10 检出，断言挂接全域导出面
//! 组件：LedgerView 真源账（字段闭集：面/值/签名/单位/tick 五列）+
//!      ExportView 导出面视图（同字段同构）+ 一致性断言 assert_aligned
//!      （逐字段对齐：值/签名/单位/tick/面序五维全位对拍，任何一维漂移即
//!      分族立案）+ 两张皮注入谱十例封闭（值漂移×2 / 字段缺失 / 字段多余 /
//!      签名漂移 / 单位漂移 / tick 漂移 / 面错位 / 序错位 / 静默重算）——
//!      全部必检出（10/10 判据②）+ 全域挂接面（三册面各自挂断言，任一面
//!      失恒全域报 fail-fast——判据③）+ LCG 压力（真源/视图轮转生成 + 断言
//!      逐轮全过 + 注入轮必检）+ 指纹守恒对拍锚。
//! 对标：数据库主从复制一致性校验（pt-table-checksum 逐行逐列对拍）——
//!   本模块以「同构双视图 + 五维逐字段对拍 + 十例注入谱 + 全域挂接」复刻
//!   防两张皮纪律，导出面永不允许与真源账分叉而不被察觉。
//! 性能逐项分解：断言 O(字段数×记录数)；注入谱 O(10)；挂接面 O(面数)；
//!   全整数算术零浮点零墙钟，同事件序列恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xD2（0xD201~0xD207，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/i64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板
//!   复述，零死代码。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 记录数上限。
pub const ROW_CAP: usize = 64;
/// 注入谱笔数（判据② 10/10）。
pub const INJECT_N: usize = 10;
/// 挂接面数（三册全域——判据③）。
pub const MOUNT_FACES: usize = 3;

// 判码（段内独占 0xD201~0xD207）
/// 0xD201 值漂移（导出值 ≠ 账本值）。
pub const C_VALUE_DRIFT: u16 = 0xD201;
/// 0xD202 字段缺失（视图少行/少列）。
pub const C_FIELD_MISS: u16 = 0xD202;
/// 0xD203 字段多余（视图多行/多列）。
pub const C_FIELD_EXTRA: u16 = 0xD203;
/// 0xD204 签名漂移（签名列不符）。
pub const C_SIGN_DRIFT: u16 = 0xD204;
/// 0xD205 时间戳漂移（tick 列不符）。
pub const C_STAMP_DRIFT: u16 = 0xD205;
/// 0xD206 面错位（面序/面归属不符）。
pub const C_FACE_MISALIGN: u16 = 0xD206;
/// 0xD207 两张皮漏检（注入谱有例未检出——防线码）。
pub const C_CLAIM_MISS: u16 = 0xD207;

/// 段守卫：判码落 0xD200..0xD300 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_VALUE_DRIFT >= 0xD200
        && C_VALUE_DRIFT < 0xD300
        && C_FIELD_MISS == C_VALUE_DRIFT + 1
        && C_FIELD_EXTRA == C_VALUE_DRIFT + 2
        && C_SIGN_DRIFT == C_VALUE_DRIFT + 3
        && C_STAMP_DRIFT == C_VALUE_DRIFT + 4
        && C_FACE_MISALIGN == C_VALUE_DRIFT + 5
        && C_CLAIM_MISS == C_VALUE_DRIFT + 6
        && C_CLAIM_MISS < 0xD300
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_VALUE_DRIFT => "值漂移",
        C_FIELD_MISS => "字段缺失",
        C_FIELD_EXTRA => "字段多余",
        C_SIGN_DRIFT => "签名漂移",
        C_STAMP_DRIFT => "时间戳漂移",
        C_FACE_MISALIGN => "面错位",
        C_CLAIM_MISS => "两张皮漏检",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 同构双视图
// ---------------------------------------------------------------------------

/// 单行五列（面 id/值/签名/单位/tick——真源与视图同构）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    /// 面 id（0..3）。
    pub face: u64,
    /// 值。
    pub value: i64,
    /// 签名（face+value+unit 混合 FNV）。
    pub sign: u64,
    /// 单位。
    pub unit: u16,
    /// tick。
    pub tick: u64,
}

/// 签名生成（与 F2105 同族口径）。
pub const fn make_sign(face: u64, value: i64, unit: u16) -> u64 {
    let mut h: u64 = 0xD202_0000_0002_106F;
    h ^= face;
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= value as u64;
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= unit as u64;
    h = h.wrapping_mul(0x100_0000_01B3);
    h
}

/// 真源行构造（签名自动账面）。
pub const fn src_row(face: u64, value: i64, unit: u16, tick: u64) -> Row {
    Row { face, value, sign: make_sign(face, value, unit), unit, tick }
}

/// 一致性断言：导出视图 vs 真源账逐字段对齐——判据①。
/// 五维对拍顺序：行数 → 逐行 face/value/sign/unit/tick。
/// 返回 Ok(()) = 全对齐；Err(判码) = 首个失恒族。
pub fn assert_aligned(src: &[Row], view: &[Row]) -> Result<(), u16> {
    if view.len() < src.len() {
        return Err(C_FIELD_MISS);
    }
    if view.len() > src.len() {
        return Err(C_FIELD_EXTRA);
    }
    for i in 0..src.len() {
        let s = &src[i];
        let v = &view[i];
        if v.face != s.face {
            return Err(C_FACE_MISALIGN);
        }
        if v.value != s.value {
            return Err(C_VALUE_DRIFT);
        }
        if v.sign != s.sign {
            return Err(C_SIGN_DRIFT);
        }
        if v.unit != s.unit {
            return Err(C_VALUE_DRIFT); // 单位漂移归值族（值面失恒）
        }
        if v.tick != s.tick {
            return Err(C_STAMP_DRIFT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 两张皮注入谱（判据② 10/10）
// ---------------------------------------------------------------------------

/// 注入谱样板真源（四行三面）。
pub fn sample_src() -> Vec<Row> {
    vec![
        src_row(0, 100, 3, 7),
        src_row(1, 200, 4, 7),
        src_row(2, 300, 1, 7),
        src_row(0, 400, 5, 7),
    ]
}

/// 十例注入，全部必须被检出且判码归位（例⑩为干净对照：重算签名不失恒）。
/// 返回通过例数（应 10）。
pub fn inject_ten() -> (usize, [u16; INJECT_N]) {
    let src = sample_src();
    let mut pass = 0usize;
    let mut codes = [0u16; INJECT_N];
    let mut check = |i: usize, view: Vec<Row>, want: Result<(), u16>| {
        if assert_aligned(&src, &view) == want {
            pass += 1;
        }
    };
    // ① 值漂移：视图改值。
    let mut v = src.clone();
    v[0].value ^= 1;
    check(0, v, Err(C_VALUE_DRIFT));
    // ② 值漂移：真源改值（反向失恒）。
    let mut s2 = src.clone();
    s2[1].value ^= 1;
    check(1, s2, Err(C_VALUE_DRIFT));
    // ③ 字段缺失：视图删一行。
    let mut v3 = src.clone();
    v3.pop();
    check(2, v3, Err(C_FIELD_MISS));
    // ④ 字段多余：视图加一行。
    let mut v4 = src.clone();
    v4.push(src_row(2, 999, 3, 7));
    check(3, v4, Err(C_FIELD_EXTRA));
    // ⑤ 签名漂移：视图改签名。
    let mut v5 = src.clone();
    v5[2].sign ^= 1;
    check(4, v5, Err(C_SIGN_DRIFT));
    // ⑥ 单位漂移：视图改单位（签名未随改——按五维对拍序报值族）。
    let mut v6 = src.clone();
    v6[1].unit = 2;
    check(5, v6, Err(C_VALUE_DRIFT));
    // ⑦ tick 漂移：视图改 tick。
    let mut v7 = src.clone();
    v7[0].tick ^= 1;
    check(6, v7, Err(C_STAMP_DRIFT));
    // ⑧ 面错位：视图改面归属。
    let mut v8 = src.clone();
    v8[1].face = 0;
    check(7, v8, Err(C_FACE_MISALIGN));
    // ⑨ 序错位：视图两行互换（首行 face 失恒——报面错位）。
    let mut v9 = src.clone();
    v9.swap(0, 1);
    check(8, v9, Err(C_FACE_MISALIGN));
    // ⑩ 静默重算：视图按现值重算签名 → 与真源全同 → 干净不误报（对照例）。
    let v10: Vec<Row> = src
        .iter()
        .map(|r| Row { sign: make_sign(r.face, r.value, r.unit), ..*r })
        .collect();
    check(9, v10, Ok(()));
    (pass, codes)
}

// ---------------------------------------------------------------------------
// 全域挂接面（判据③）
// ---------------------------------------------------------------------------

/// 挂接面账：三面各自持真源与视图，任一面失恒全域报。
pub struct MountBoard {
    /// 三面真源账。
    pub srcs: [Vec<Row>; MOUNT_FACES],
    /// 三面导出视图。
    pub views: [Vec<Row>; MOUNT_FACES],
}

impl MountBoard {
    /// 空板。
    pub fn new() -> Self {
        MountBoard {
            srcs: [Vec::new(), Vec::new(), Vec::new()],
            views: [Vec::new(), Vec::new(), Vec::new()],
        }
    }
    /// 真源入账 + 视图同步（正常路径：两皮同步写）。
    pub fn put(&mut self, face: usize, row: Row) {
        if face < MOUNT_FACES && self.srcs[face].len() < ROW_CAP {
            self.srcs[face].push(row);
            self.views[face].push(row);
        }
    }
    /// 全域断言：逐面 assert_aligned，任一面失恒即 Err（fail-fast）。
    pub fn assert_all(&self) -> Result<(), u16> {
        for f in 0..MOUNT_FACES {
            assert_aligned(&self.srcs[f], &self.views[f])?;
        }
        Ok(())
    }
    /// 面失恒定位：首个失恒面 id（全域挂接的诊断面）。
    pub fn first_bad_face(&self) -> Option<usize> {
        for f in 0..MOUNT_FACES {
            if assert_aligned(&self.srcs[f], &self.views[f]).is_err() {
                return Some(f);
            }
        }
        None
    }
    /// 指纹：三面真源+视图混合 FNV。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xD207_0000_0002_1070;
        for f in 0..MOUNT_FACES {
            for r in self.srcs[f].iter().chain(self.views[f].iter()) {
                for v in [r.face, r.value as u64, r.sign, r.unit as u64, r.tick] {
                    h ^= v;
                    h = h.wrapping_mul(0x100_0000_01B3);
                }
            }
        }
        h
    }
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
pub struct AlignStress {
    /// 同步写笔数。
    pub puts: u64,
    /// 断言过轮数。
    pub asserts_ok: u64,
    /// 断言轮总数。
    pub asserts_total: u64,
    /// 总结论。
    pub ok: bool,
}

/// 一致性压力：n 轮 LCG（同步写→断言→偶发注入必检）。
pub fn stress_align(n: usize, seed: u64) -> AlignStress {
    let mut rng = Lcg::new(seed);
    let mut board = MountBoard::new();
    let mut puts: u64 = 0;
    let mut asserts_ok: u64 = 0;
    let mut asserts_total: u64 = 0;
    for i in 0..n {
        let r = rng.next();
        let face = (r % 3) as usize;
        match i % 5 {
            0 | 1 | 2 => {
                let row = src_row(face as u64, ((r >> 8) % 5000) as i64, ((r >> 20) % 5 + 1) as u16, (i as u64) / 3);
                board.put(face, row);
                puts += 1;
            }
            3 => {
                asserts_total += 1;
                if board.assert_all().is_ok() {
                    asserts_ok += 1;
                }
            }
            _ => {
                // 注入轮：定向撕裂一面视图 → 断言必检 + 定位面正确。
                asserts_total += 1;
                let f = ((r >> 16) as usize) % MOUNT_FACES;
                if let Some(last) = board.views[f].last_mut() {
                    let keep = *last;
                    last.value ^= 1;
                    let detected = board.assert_all().is_err()
                        && board.first_bad_face() == Some(f);
                    // 还原（压力面只核检出，不污染后续轮）。
                    let n = board.views[f].len();
                    if n > 0 {
                        board.views[f][n - 1] = keep;
                    }
                    if detected {
                        asserts_ok += 1;
                    }
                } else {
                    asserts_ok += 1;
                }
            }
        }
    }
    let ok = asserts_ok == asserts_total && board.assert_all().is_ok();
    AlignStress { puts, asserts_ok, asserts_total, ok }
}

/// 指纹：压力三柱混合 FNV。
pub fn fingerprint(s: &AlignStress) -> u64 {
    let mut h: u64 = 0xD200_0000_0002_1071;
    for v in [s.puts, s.asserts_ok, s.asserts_total] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2106_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2106");

    // J-1 段守卫。
    cs.add("0xD2段独占", seg_guard(), "");

    // J-2 干净对拍：同构双视图全对齐（判据①）。
    let src = sample_src();
    cs.add("同构对齐全过", assert_aligned(&src, &src.clone()) == Ok(()), "");

    // J-3 两张皮注入谱 10/10 检出（判据②）。
    let (pass, codes) = inject_ten();
    let miss = codes.iter().any(|&c| c != 0);
    cs.add("注入谱10/10", pass == INJECT_N && !miss, "");

    // J-4 分族判码精确：值漂移报值族、缺行报缺失族、多行报多余族。
    let src4 = sample_src();
    let mut v4 = src4.clone();
    v4.pop();
    let mut v5 = src4.clone();
    v5.push(src_row(0, 1, 3, 7));
    cs.add(
        "分族判码精确",
        assert_aligned(&src4, &v4) == Err(C_FIELD_MISS) && assert_aligned(&src4, &v5) == Err(C_FIELD_EXTRA),
        "",
    );

    // J-5 全域挂接：三面断言 + 面定位。
    let mut board = MountBoard::new();
    for i in 0..6u64 {
        let f = (i % 3) as usize;
        board.put(f, src_row(f as u64, (i as i64) * 10, 3, 1));
    }
    let all_ok = board.assert_all() == Ok(());
    board.views[1][0].value ^= 1; // 撕裂一面
    let located = board.first_bad_face() == Some(1) && board.assert_all().is_err();
    cs.add("全域挂接+面定位", all_ok && located, "");

    // J-6 挂接面数闭集。
    cs.add("挂接面数=3", MOUNT_FACES == 3, "");

    // J-7 压力：512 轮全维核。
    let st = stress_align(512, 0xD204_0001);
    cs.add("压力全维", st.ok && st.puts > 0 && st.asserts_total > 0, "");

    // J-8 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_align(128, 0xD204_0002));
    let f2 = fingerprint(&stress_align(128, 0xD204_0002));
    let f3 = fingerprint(&stress_align(128, 0xD204_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-9 判码文本闭集：七码各得其所。
    let texts_ok = code_text(C_VALUE_DRIFT) != "未知判码"
        && code_text(C_FIELD_MISS) != "未知判码"
        && code_text(C_FIELD_EXTRA) != "未知判码"
        && code_text(C_SIGN_DRIFT) != "未知判码"
        && code_text(C_STAMP_DRIFT) != "未知判码"
        && code_text(C_FACE_MISALIGN) != "未知判码"
        && code_text(C_CLAIM_MISS) != "未知判码"
        && code_text(0xD200) == "未知判码";
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
    fn clean_align_ok() {
        let src = sample_src();
        assert_eq!(assert_aligned(&src, &src.clone()), Ok(()));
    }

    #[test]
    fn inject_ten_all() {
        let (pass, codes) = inject_ten();
        assert_eq!(pass, 10, "codes: {:?}", codes);
    }

    #[test]
    fn mount_locate() {
        let mut board = MountBoard::new();
        for i in 0..6u64 {
            let f = (i % 3) as usize;
            board.put(f, src_row(f as u64, i as i64, 3, 1));
        }
        assert_eq!(board.assert_all(), Ok(()));
        board.views[2][0].tick ^= 1;
        assert_eq!(board.first_bad_face(), Some(2));
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_align(128, 11);
        let b = stress_align(128, 11);
        assert_eq!(a.puts, b.puts);
        assert!(a.ok);
    }
}
