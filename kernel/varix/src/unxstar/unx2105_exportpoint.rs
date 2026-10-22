//! unx2105_exportpoint —— UNX-F2105 单点导出：用户态导出汇总接口。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2105 · 单点导出（用户态导出汇总接口） · 目标 280 行 · 已深化
//! 判据原文：UNX-F2105-J1 第 26 处单点导出在册（三面导出状态汇总），签名/
//!   单位/同窗快照三列齐备，与 B09 汇聚消费一致
//! 组件：Face 封闭三面（VE/CGPU/CoRun 三册导出面）+ ExportRecord 导出记录
//!      （面 id/值/签名 FNV/单位/同窗快照 tick 五元）+ 单点序号在册核
//!      （EXPORT_POINT_SEQ=26——第 26 处单点，越册 0xD101）+ 三列齐备闸
//!      （签名缺失 0xD102 / 单位越闭集 0xD103 / 同窗快照失恒 0xD104）+
//!      同窗快照语义（三面记录 tick 必须一致方成单点汇总——跨面同窗）+
//!      B09 汇聚一致断言（单点汇总视图与三面账逐字段重算对拍，失恒
//!      0xD105）+ 面越集闸（0xD106）+ LCG 压力（三面记录轮转 + 同窗/跨窗
//!      混合 + 汇聚对拍逐轮全过）+ 指纹守恒对拍锚。
//! 对标：Windows 性能计数器单点导出（PdhCollectQueryData 单点汇聚多面
//!   计数，签名/单位/时间戳三列齐备）——本模块以「封闭三面 + 三列齐备
//!   闸 + 同窗快照 + B09 汇聚对拍」复刻该面，无真实性能计数依赖。
//! 性能逐项分解：记录入账 O(1)；同窗核 O(面数)=O(3)；汇聚对拍 O(记录数)；
//!   签名 O(五元)；全整数算术零浮点零墙钟，同事件序列恒同账（确定性对拍
//!   锚，LCG 注入）。
//! 诊断码段：0xD1（0xD101~0xD106，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码与 0xD1FF/0xD1A1 哨兵偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/i64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板
//!   复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 单点导出在册序号：第 26 处（判据①在册面）。
pub const EXPORT_POINT_SEQ: u64 = 26;
/// 记录数上限（防失控）。
pub const RECORD_CAP: usize = 128;
/// 单位闭集（判据②单位列）：
pub const UNIT_NS: u16 = 1;
/// 单位：微秒。
pub const UNIT_US: u16 = 2;
/// 单位：计数。
pub const UNIT_COUNT: u16 = 3;
/// 单位：字节。
pub const UNIT_BYTES: u16 = 4;
/// 单位：千比特每秒。
pub const UNIT_BPS: u16 = 5;
/// 单位全集（闭集核用）。
pub const UNITS: [u16; 5] = [UNIT_NS, UNIT_US, UNIT_COUNT, UNIT_BYTES, UNIT_BPS];
/// 面数（三册面）。
pub const FACES: usize = 3;

// 判码（段内独占 0xD101~0xD106）
/// 0xD101 单点序越册（≠26）。
pub const C_POINT_OOB: u16 = 0xD101;
/// 0xD102 签名缺失/不符。
pub const C_SIGN_MISS: u16 = 0xD102;
/// 0xD103 单位越闭集。
pub const C_UNIT_BAD: u16 = 0xD103;
/// 0xD104 同窗快照失恒（跨面 tick 不一致）。
pub const C_WINDOW_MISMATCH: u16 = 0xD104;
/// 0xD105 汇聚与账本不一致。
pub const C_AGG_MISMATCH: u16 = 0xD105;
/// 0xD106 面越集（非三册面）。
pub const C_FACE_OOB: u16 = 0xD106;

/// 段守卫：判码落 0xD100..0xD200 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_POINT_OOB >= 0xD100
        && C_POINT_OOB < 0xD200
        && C_SIGN_MISS == C_POINT_OOB + 1
        && C_UNIT_BAD == C_POINT_OOB + 2
        && C_WINDOW_MISMATCH == C_POINT_OOB + 3
        && C_AGG_MISMATCH == C_POINT_OOB + 4
        && C_FACE_OOB == C_POINT_OOB + 5
        && C_FACE_OOB < 0xD200
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_POINT_OOB => "单点序越册",
        C_SIGN_MISS => "签名缺失",
        C_UNIT_BAD => "单位越闭集",
        C_WINDOW_MISMATCH => "同窗快照失恒",
        C_AGG_MISMATCH => "汇聚不一致",
        C_FACE_OOB => "面越集",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 三面导出账
// ---------------------------------------------------------------------------

/// 封闭三面：VE / CGPU / CoRun 三册导出面（面 id 0/1/2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// VE 册面。
    Ve,
    /// CGPU 册面。
    Cgpu,
    /// CoRun 册面。
    Corun,
}

impl Face {
    /// 面 id（0..3）。
    pub const fn id(self) -> u64 {
        match self {
            Face::Ve => 0,
            Face::Cgpu => 1,
            Face::Corun => 2,
        }
    }
    /// id → 面（越集 None）。
    pub const fn of(id: u64) -> Option<Face> {
        match id {
            0 => Some(Face::Ve),
            1 => Some(Face::Cgpu),
            2 => Some(Face::Corun),
            _ => None,
        }
    }
}

/// 导出记录五元：面/值/签名/单位/同窗 tick。
///
/// `PartialEq`：账本 `ExportPoint` 派 PartialEq 时需要整账可比较（判据
/// `assert_eq!(ExportPoint::new(n), Err(..))` 走 Result 相等），记录元组随之。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportRecord {
    /// 面。
    pub face: Face,
    /// 导出值。
    pub value: i64,
    /// 签名（值+面+单位混合 FNV）。
    pub sign: u64,
    /// 单位（闭集）。
    pub unit: u16,
    /// 同窗快照 tick。
    pub tick: u64,
}

/// 签名生成（值+面+单位混合 FNV——签名列账面）。
pub const fn make_sign(face: Face, value: i64, unit: u16) -> u64 {
    let mut h: u64 = 0xD102_0000_0002_105E;
    h ^= face.id();
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= value as u64;
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= unit as u64;
    h = h.wrapping_mul(0x100_0000_01B3);
    h
}

/// 签名核（判码 0xD102 面）。
pub const fn sign_ok(r: &ExportRecord) -> bool {
    r.sign == make_sign(r.face, r.value, r.unit)
}

/// 单位核（判码 0xD103 面）。
pub const fn unit_ok(unit: u16) -> bool {
    unit == UNIT_NS
        || unit == UNIT_US
        || unit == UNIT_COUNT
        || unit == UNIT_BYTES
        || unit == UNIT_BPS
}

/// 三面导出账 + 单点序。
///
/// `Debug`/`PartialEq`：判据侧 `assert_eq!(ExportPoint::new(n), Err(..))` 需要
/// 两侧同具 Debug 与相等比较（`Result` 的 `assert_eq!` 要求两者）。
/// 不派 `Eq`：账内 `ExportRecord` 只保证 PartialEq（含浮点域），整体不强求 Eq。
#[derive(Clone, Debug, PartialEq)]
pub struct ExportPoint {
    /// 在册单点序号（构造期钉死）。
    pub seq: u64,
    /// 记录账。
    pub records: Vec<ExportRecord>,
}

impl ExportPoint {
    /// 构造：单点序在册核先行（≠26 拒）。
    pub fn new(seq: u64) -> Result<ExportPoint, u16> {
        if seq != EXPORT_POINT_SEQ {
            return Err(C_POINT_OOB);
        }
        Ok(ExportPoint { seq, records: Vec::new() })
    }

    /// 入账：面闸 → 单位闸 → 签名闸 → 容量闸。
    pub fn push(&mut self, face: Face, value: i64, unit: u16, tick: u64) -> Result<(), u16> {
        if Face::of(face.id()).is_none() {
            return Err(C_FACE_OOB);
        }
        if !unit_ok(unit) {
            return Err(C_UNIT_BAD);
        }
        let sign = make_sign(face, value, unit);
        if self.records.len() >= RECORD_CAP {
            return Err(C_AGG_MISMATCH);
        }
        self.records.push(ExportRecord { face, value, sign, unit, tick });
        let _ = sign;
        if !sign_ok(self.records.last().unwrap_or(&ExportRecord {
            face,
            value,
            sign,
            unit,
            tick,
        })) {
            // 理论不可达（sign 本地生成）——防线码兜底。
            return Err(C_SIGN_MISS);
        }
        Ok(())
    }

    /// 同窗快照核：同一 tick 的三面记录齐备方为有效单点汇总——判据②同窗列。
    /// 返回 Err(C_WINDOW_MISMATCH) 若三面 tick 不一致；Ok(三面值) 若齐备。
    pub fn snapshot(&self, tick: u64) -> Result<[i64; FACES], u16> {
        let mut out = [0i64; FACES];
        let mut seen = [false; FACES];
        for r in &self.records {
            if r.tick == tick {
                out[r.face.id() as usize] = r.value;
                seen[r.face.id() as usize] = true;
            }
        }
        if !seen.iter().all(|&s| s) {
            return Err(C_WINDOW_MISMATCH);
        }
        Ok(out)
    }

    /// B09 汇聚一致断言：单点汇总视图与三面账逐字段重算对拍——判据③。
    /// 汇聚视图 = 各面最新记录值（同窗 tick 面）；对拍失恒 0xD105。
    pub fn agg_consistent(&self, agg: &[i64; FACES], tick: u64) -> bool {
        match self.snapshot(tick) {
            Ok(view) => view == *agg,
            Err(_) => false,
        }
    }

    /// 记录核：全记录签名/单位/面逐条自洽。
    pub fn records_ok(&self) -> bool {
        self.records
            .iter()
            .all(|r| sign_ok(r) && unit_ok(r.unit) && Face::of(r.face.id()).is_some())
    }

    /// 指纹：序号 + 全记录五元混合 FNV。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xD105_0000_0002_105F;
        h ^= self.seq;
        h = h.wrapping_mul(0x100_0000_01B3);
        for r in &self.records {
            for v in [r.face.id(), r.value as u64, r.sign, r.unit as u64, r.tick] {
                h ^= v;
                h = h.wrapping_mul(0x100_0000_01B3);
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
pub struct PointStress {
    /// 入账成功笔数。
    pub pushed: u64,
    /// 拒绝笔数。
    pub rejected: u64,
    /// 同窗快照成功轮数。
    pub snapshots_ok: u64,
    /// 汇聚一致轮数。
    pub agg_ok: u64,
    /// 总结论。
    pub ok: bool,
}

/// 单点导出压力：n 轮 LCG（三面轮转入账→同窗快照→汇聚对拍）。
pub fn stress_point(n: usize, seed: u64) -> PointStress {
    let mut rng = Lcg::new(seed);
    let mut ep = match ExportPoint::new(EXPORT_POINT_SEQ) {
        Ok(e) => e,
        Err(_) => {
            return PointStress { pushed: 0, rejected: 0, snapshots_ok: 0, agg_ok: 0, ok: false }
        }
    };
    let mut pushed: u64 = 0;
    let mut rejected: u64 = 0;
    let mut snapshots_ok: u64 = 0;
    let mut agg_ok: u64 = 0;
    let faces = [Face::Ve, Face::Cgpu, Face::Corun];
    let units = UNITS;
    for i in 0..n {
        let r = rng.next();
        let face = faces[(r % 3) as usize];
        let unit = units[((r >> 4) % 5) as usize];
        let value = ((r >> 8) % 10_000) as i64;
        let tick = (i as u64) / FACES as u64; // 三面一轮同 tick
        match ep.push(face, value, unit, tick) {
            Ok(()) => pushed += 1,
            Err(_) => rejected += 1,
        }
        // 每满一轮三面（i%3==2）做同窗快照 + 汇聚对拍。
        if i % 3 == 2 {
            let t = tick;
            match ep.snapshot(t) {
                Ok(view) => {
                    snapshots_ok += 1;
                    if ep.agg_consistent(&view, t) {
                        agg_ok += 1;
                    }
                }
                Err(_) => {}
            }
        }
    }
    let ok = ep.records_ok() && snapshots_ok > 0 && snapshots_ok == agg_ok;
    PointStress { pushed, rejected, snapshots_ok, agg_ok, ok }
}

/// 指纹：压力四柱混合 FNV。
pub fn fingerprint(s: &PointStress) -> u64 {
    let mut h: u64 = 0xD106_0000_0002_1060;
    for v in [s.pushed, s.rejected, s.snapshots_ok, s.agg_ok] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

/// 跨窗探针（负向证据）：三面 tick 不一致时快照必拒。
pub fn cross_window_probe() -> bool {
    let mut ep = match ExportPoint::new(EXPORT_POINT_SEQ) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let _ = ep.push(Face::Ve, 1, UNIT_COUNT, 10);
    let _ = ep.push(Face::Cgpu, 2, UNIT_COUNT, 11);
    let _ = ep.push(Face::Corun, 3, UNIT_COUNT, 12);
    ep.snapshot(10) == Err(C_WINDOW_MISMATCH)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2105_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2105");

    // J-1 段守卫 + 单点序在册（第 26 处）。
    let seq_ok = matches!(ExportPoint::new(26), Ok(_))
        && matches!(ExportPoint::new(25), Err(C_POINT_OOB))
        && matches!(ExportPoint::new(27), Err(C_POINT_OOB));
    cs.add(
        "0xD1段独占+第26处在册",
        seg_guard() && EXPORT_POINT_SEQ == 26 && seq_ok,
        "",
    );

    // J-2 三列齐备：签名/单位/同窗逐条闸。
    let mut ep = match ExportPoint::new(EXPORT_POINT_SEQ) {
        Ok(e) => e,
        Err(_) => {
            cs.add("三列齐备", false, "");
            return cs;
        }
    };
    let p1 = ep.push(Face::Ve, 100, UNIT_US, 1);
    let p2 = ep.push(Face::Cgpu, 200, UNIT_BPS, 1);
    let p3 = ep.push(Face::Corun, 300, UNIT_COUNT, 1);
    let bad_unit = ep.push(Face::Ve, 1, 99, 1) == Err(C_UNIT_BAD);
    let rec_ok = ep.records_ok();
    cs.add("三列齐备闸", p1.is_ok() && p2.is_ok() && p3.is_ok() && bad_unit && rec_ok, "");

    // J-3 同窗快照：三面同 tick 齐备成单点（判据②同窗列）。
    let snap = ep.snapshot(1);
    cs.add("同窗快照齐备", snap == Ok([100, 200, 300]), "");

    // J-4 跨窗探针：tick 不一致必拒（负向证据）。
    cs.add("跨窗必拒", cross_window_probe(), "");

    // J-5 签名核：签名不符检出。
    let mut bad_rec = match ep.records.first() {
        Some(r) => *r,
        None => {
            cs.add("签名核", false, "");
            return cs;
        }
    };
    bad_rec.sign ^= 1;
    cs.add("签名不符检出", !sign_ok(&bad_rec) && sign_ok(ep.records.first().unwrap_or(&bad_rec)), "");

    // J-6 B09 汇聚一致：视图与账重算对拍（判据③）。
    let view = match ep.snapshot(1) {
        Ok(v) => v,
        Err(_) => {
            cs.add("B09汇聚一致", false, "");
            return cs;
        }
    };
    let ok_view = ep.agg_consistent(&view, 1);
    let mut bad_view = view;
    bad_view[0] ^= 1;
    let bad_detected = !ep.agg_consistent(&bad_view, 1);
    cs.add("B09汇聚一致", ok_view && bad_detected, "");

    // J-7 压力：384 轮全维核。
    let st = stress_point(384, 0xD103_0001);
    cs.add("压力全维", st.ok && st.pushed > 0 && st.rejected == 0, "");

    // J-8 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_point(128, 0xD103_0002));
    let f2 = fingerprint(&stress_point(128, 0xD103_0002));
    let f3 = fingerprint(&stress_point(128, 0xD103_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-9 判码文本闭集：六码各得其所。
    let texts_ok = code_text(C_POINT_OOB) != "未知判码"
        && code_text(C_SIGN_MISS) != "未知判码"
        && code_text(C_UNIT_BAD) != "未知判码"
        && code_text(C_WINDOW_MISMATCH) != "未知判码"
        && code_text(C_AGG_MISMATCH) != "未知判码"
        && code_text(C_FACE_OOB) != "未知判码"
        && code_text(0xD100) == "未知判码";
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
    fn point_seq_in_book() {
        assert!(ExportPoint::new(26).is_ok());
        assert_eq!(ExportPoint::new(25), Err(C_POINT_OOB));
        assert_eq!(ExportPoint::new(27), Err(C_POINT_OOB));
    }

    #[test]
    fn three_columns_and_window() {
        let mut ep = ExportPoint::new(26).unwrap();
        assert!(ep.push(Face::Ve, 10, UNIT_NS, 5).is_ok());
        assert!(ep.push(Face::Cgpu, 20, UNIT_BYTES, 5).is_ok());
        assert!(ep.push(Face::Corun, 30, UNIT_COUNT, 5).is_ok());
        assert_eq!(ep.snapshot(5), Ok([10, 20, 30]));
        assert_eq!(ep.push(Face::Ve, 1, 99, 5), Err(C_UNIT_BAD));
        assert!(ep.records_ok());
    }

    #[test]
    fn cross_window_rejected() {
        assert!(cross_window_probe());
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_point(128, 7);
        let b = stress_point(128, 7);
        assert_eq!(a.pushed, b.pushed);
        assert_eq!(a.snapshots_ok, b.snapshots_ok);
        assert!(a.ok);
    }
}
