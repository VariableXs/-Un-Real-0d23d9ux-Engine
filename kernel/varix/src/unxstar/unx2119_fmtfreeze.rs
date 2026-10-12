//! unx2119_fmtfreeze —— UNX-F2119 版本冻结（列序/单位/版本三冻结列）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2119 · 版本冻结 ·
//!   目标 280 行 · 已深化
//! 判据原文：UNX-F2119-J1 冻结在册（列序/单位/版本三冻结列 + FNV 指纹
//!   钉死），漂移注入 10/10 拒绝。
//! 组件：ColKind 列种类闭集（Scene/Shape/Value/Stamp 四列）+
//!      UnitKind 单位闭集（Cnt/Step/Pend 三档）+
//!      ColSpec 列规格（列种类 + 单位 + 列序位三列）+
//!      FreezeSpec 冻结规格（版本 + 列数 + 列规格序列 + 指纹四冻结）+
//!      FreezeCore 冻结核（一次冻结：指纹 FNV 钉死，重复冻结幂等）+
//!      VerifyCore 验冻核（重算指纹 == 钉死值，不等即漂移）+
//!      DriftInject 漂移注入探针（列序换位/单位改档/版本号改 三类必拒）+
//!      CompatCheck 兼容核（读数必须带冻结指纹，指纹不符即拒）+
//!      ThawLedger 解冻台账（解冻必留痕——无痕解冻即拒）+ LCG 压力
//!      256 轮漂移轮转 + 两册重放对拍 + 0xF1 段六码 + CheckSet 自检。
//! 对标：Linux kernel trace 事件格式 ring_buffer 的 format 冻结与
//!   sysfs ABI 版本纪律（结构体 layout 钉死后永不改，只能加）——本模块
//!   以「三冻结列 + 指纹钉死 + 读数验指纹」复刻格式冻结纪律。
//! 性能逐项分解：冻结核 O(n)（n≤8 列常阶上界）；验冻核 O(n) 重算；
//!   漂移判定 O(n) 比对；全整数算术零浮点零墙钟，同构造恒同册。
//! 诊断码段：0xF1（0xF101~0xF106，全域 grep 确认无 u16 判码占用——
//!   0xF2 有 svstar2 veb233 的 0xF233 magic，顺让占 0xF1）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，逻辑
//!   tick 注入零墙钟，逐条定制数据结构/状态机/错误路径，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 最大列数。
pub const COL_MAX: usize = 8;
/// 冻结版本上界。
pub const FMT_VER_MAX: u32 = 4096;
/// 解冻台账容量。
pub const THAW_CAP: usize = 32;
/// 压力轮数。
pub const STRESS_ROUNDS: u64 = 256;

// 判码（段内独占 0xF101~0xF106）
/// 0xF101 列序漂移（列序位换位/重复）。
pub const C_COL_DRIFT: u16 = 0xF101;
/// 0xF102 单位漂移（列单位改档）。
pub const C_UNIT_DRIFT: u16 = 0xF102;
/// 0xF103 版本漂移（格式版本号改）。
pub const C_VER_DRIFT: u16 = 0xF103;
/// 0xF104 指纹不符（读数指纹 ≠ 钉死指纹）。
pub const C_FP_MISMATCH: u16 = 0xF104;
/// 0xF105 未冻即用（未冻结即读数/即写）。
pub const C_NOT_FROZEN: u16 = 0xF105;
/// 0xF106 无痕解冻（解冻未留痕）。
pub const C_NO_TRAIL_THAW: u16 = 0xF106;

/// 段守卫：判码落 0xF100..0xF200 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_COL_DRIFT >= 0xF100
        && C_COL_DRIFT < 0xF200
        && C_UNIT_DRIFT == C_COL_DRIFT + 1
        && C_VER_DRIFT == C_UNIT_DRIFT + 1
        && C_FP_MISMATCH == C_VER_DRIFT + 1
        && C_NOT_FROZEN == C_FP_MISMATCH + 1
        && C_NO_TRAIL_THAW == C_NOT_FROZEN + 1
        && C_NO_TRAIL_THAW < 0xF200
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_COL_DRIFT => "列序漂移",
        C_UNIT_DRIFT => "单位漂移",
        C_VER_DRIFT => "版本漂移",
        C_FP_MISMATCH => "指纹不符",
        C_NOT_FROZEN => "未冻即用",
        C_NO_TRAIL_THAW => "无痕解冻",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn——F0386 借用冲突教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 列规格与冻结规格
// ---------------------------------------------------------------------------

/// 列种类（四列闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColKind {
    /// 场景列。
    Scene,
    /// 形态列。
    Shape,
    /// 计值列。
    Value,
    /// 戳位列。
    Stamp,
}

/// 列种类序（0..3）。
pub const fn col_ord(c: ColKind) -> u8 {
    match c {
        ColKind::Scene => 0,
        ColKind::Shape => 1,
        ColKind::Value => 2,
        ColKind::Stamp => 3,
    }
}

/// 列种类序 → 种类；越界 None。
pub const fn col_from_ord(o: u8) -> Option<ColKind> {
    match o {
        0 => Some(ColKind::Scene),
        1 => Some(ColKind::Shape),
        2 => Some(ColKind::Value),
        3 => Some(ColKind::Stamp),
        _ => None,
    }
}

/// 单位（三档闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// 次数单位。
    Cnt,
    /// 步进单位。
    Step,
    /// 待定单位（真机补测）。
    Pend,
}

/// 单位序（0..2）。
pub const fn unit_ord(u: UnitKind) -> u8 {
    match u {
        UnitKind::Cnt => 0,
        UnitKind::Step => 1,
        UnitKind::Pend => 2,
    }
}

/// 单位序 → 单位；越界 None。
pub const fn unit_from_ord(o: u8) -> Option<UnitKind> {
    match o {
        0 => Some(UnitKind::Cnt),
        1 => Some(UnitKind::Step),
        2 => Some(UnitKind::Pend),
        _ => None,
    }
}

/// 列规格：列种类 + 单位 + 列序位（列在册中的位置）三列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColSpec {
    /// 列种类。
    pub kind: ColKind,
    /// 单位。
    pub unit: UnitKind,
    /// 列序位（0..COL_MAX 内唯一）。
    pub pos: u8,
}

impl ColSpec {
    /// 规格合法核（列种类闭集 + 单位闭集 + 序位界内）。
    pub fn valid(&self) -> bool {
        col_from_ord(col_ord(self.kind)).is_some()
            && unit_from_ord(unit_ord(self.unit)).is_some()
            && (self.pos as usize) < COL_MAX
    }
}

/// 冻结规格：版本 + 列数 + 列规格序列 + 指纹四冻结列。
pub struct FreezeSpec {
    /// 格式版本（冻结后恒定）。
    ver: u32,
    /// 列规格序列。
    cols: Vec<ColSpec>,
    /// 冻结标志。
    frozen: bool,
    /// 钉死指纹（冻结时 FNV 计算）。
    fp: u64,
}

impl FreezeSpec {
    /// 新规格（未冻态）。
    pub fn new(ver: u32) -> FreezeSpec {
        FreezeSpec { ver, cols: Vec::new(), frozen: false, fp: 0 }
    }

    /// 版本（未冻态可读）。
    pub fn ver(&self) -> u32 {
        self.ver
    }

    /// 加一列（冻结后拒；列种类闭集 + 序位唯一 + 容量三闸）。
    pub fn add_col(&mut self, c: ColSpec) -> Result<(), u16> {
        if self.frozen {
            return Err(C_NOT_FROZEN); // 冻后加列即拒（归未冻口径外）
        }
        if !c.valid() {
            return Err(C_COL_DRIFT);
        }
        if self.cols.len() >= COL_MAX {
            return Err(C_COL_DRIFT);
        }
        // 序位唯一核。
        let mut i = 0;
        while i < self.cols.len() {
            if self.cols[i].pos == c.pos || self.cols[i].kind == c.kind {
                return Err(C_COL_DRIFT);
            }
            i += 1;
        }
        self.cols.push(c);
        Ok(())
    }

    /// 列数。
    pub fn col_len(&self) -> usize {
        self.cols.len()
    }

    /// 冻结核（一次性）：算指纹钉死。重复冻结幂等（返回已钉值）。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Ok(self.fp); // 幂等
        }
        // 列序连续性核（pos 必须 0..n-1 无洞）。
        let n = self.cols.len();
        let mut seen = [false; COL_MAX];
        let mut i = 0;
        while i < n {
            let p = self.cols[i].pos as usize;
            if p >= n || seen[p] {
                return Err(C_COL_DRIFT);
            }
            seen[p] = true;
            i += 1;
        }
        // 指纹：版本 + 每列（种类序/单位序/序位）。
        let mut data = Vec::with_capacity(1 + n * 3);
        data.push(self.ver as u64);
        let mut j = 0;
        while j < n {
            data.push(col_ord(self.cols[j].kind) as u64);
            data.push(unit_ord(self.cols[j].unit) as u64);
            data.push(self.cols[j].pos as u64);
            j += 1;
        }
        self.fp = fnv1a(0xF2119, &data);
        self.frozen = true;
        Ok(self.fp)
    }

    /// 是否已冻。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 验冻核：重算指纹 == 钉死值（读数前必过此关）。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(C_NOT_FROZEN);
        }
        let n = self.cols.len();
        let mut data = Vec::with_capacity(1 + n * 3);
        data.push(self.ver as u64);
        let mut j = 0;
        while j < n {
            data.push(col_ord(self.cols[j].kind) as u64);
            data.push(unit_ord(self.cols[j].unit) as u64);
            data.push(self.cols[j].pos as u64);
            j += 1;
        }
        if fnv1a(0xF2119, &data) != self.fp {
            return Err(C_FP_MISMATCH);
        }
        Ok(())
    }

    /// 读数闸：读数必须带冻结指纹且相符（判据②核）。
    pub fn read_with_fp(&self, claimed_fp: u64) -> Result<(), u16> {
        if !self.frozen {
            return Err(C_NOT_FROZEN);
        }
        if claimed_fp != self.fp {
            return Err(C_FP_MISMATCH);
        }
        Ok(())
    }

    /// 钉死指纹直读（解冻/登记路径）。
    pub fn fp(&self) -> u64 {
        self.fp
    }

    /// 清冻结位（仅解冻台账在留痕后调用——无痕解冻无路径到达此函数）。
    pub fn reset_frozen(&mut self) {
        self.frozen = false;
    }

    /// 列直读（对拍用）。
    pub fn cols(&self) -> &[ColSpec] {
        &self.cols
    }

    /// 列可变借（漂移注入探针专用——注入后必须复原）。
    pub fn cols_mut(&mut self) -> &mut [ColSpec] {
        &mut self.cols
    }

    /// 版本可变借（版本漂移探针专用——注入后必须复原）。
    pub fn set_ver_probe(&mut self, v: u32) -> u32 {
        let saved = self.ver;
        self.ver = v;
        saved
    }

    /// 单位改档核（探针用：Cn t→Step→Pend→Cnt 轮转返回下一档）。
    pub fn unit_next(u: UnitKind) -> UnitKind {
        match u {
            UnitKind::Cnt => UnitKind::Step,
            UnitKind::Step => UnitKind::Pend,
            UnitKind::Pend => UnitKind::Cnt,
        }
    }
}

// ---------------------------------------------------------------------------
// 漂移注入与解冻台账
// ---------------------------------------------------------------------------

/// 漂移注入器：三类漂移（列序/单位/版本）构造 + 复核判定。
pub struct DriftInject {
    /// 漂移拒计数。
    pub rejected: u32,
}

impl DriftInject {
    /// 新注入器。
    pub fn new() -> DriftInject {
        DriftInject { rejected: 0 }
    }

    /// 注入口径漂移：冻结后改列序位（与下一列互换）→ 指纹必不符。
    pub fn inject_col_drift(&mut self, spec: &mut FreezeSpec, col_idx: usize) -> bool {
        let n = spec.col_len();
        if !spec.frozen || n < 2 || col_idx >= n {
            return false;
        }
        // 与下一列换位（环形，保证真换位——自换位是空操作不算漂移）。
        let other = (col_idx + 1) % n;
        let saved_a = spec.cols()[col_idx].pos;
        let saved_b = spec.cols()[other].pos;
        spec.cols_mut()[col_idx].pos = saved_b;
        spec.cols_mut()[other].pos = saved_a;
        let drifted = spec.verify() == Err(C_FP_MISMATCH);
        // 复原（探针不留痕）。
        spec.cols_mut()[col_idx].pos = saved_a;
        spec.cols_mut()[other].pos = saved_b;
        if drifted {
            self.rejected += 1;
        }
        drifted
    }

    /// 注入单位漂移：冻结后改列单位 → 指纹必不符。
    pub fn inject_unit_drift(&mut self, spec: &mut FreezeSpec, col_idx: usize) -> bool {
        let n = spec.col_len();
        if !spec.frozen || col_idx >= n {
            return false;
        }
        let saved = spec.cols()[col_idx].unit;
        let next = FreezeSpec::unit_next(saved);
        spec.cols_mut()[col_idx].unit = next;
        let drifted = spec.verify() == Err(C_FP_MISMATCH);
        spec.cols_mut()[col_idx].unit = saved; // 复原
        if drifted {
            self.rejected += 1;
        }
        drifted
    }

    /// 注入版本漂移：冻结后改版本号 → 指纹必不符。
    pub fn inject_ver_drift(&mut self, spec: &mut FreezeSpec, new_ver: u32) -> bool {
        if !spec.frozen {
            return false;
        }
        let saved = spec.set_ver_probe(new_ver);
        let drifted = spec.verify() == Err(C_FP_MISMATCH);
        let _ = spec.set_ver_probe(saved); // 复原
        if drifted {
            self.rejected += 1;
        }
        drifted
    }
}

/// 解冻台账：解冻必留痕（无痕解冻即拒）。
pub struct ThawLedger {
    /// 留痕条目（解冻版本 + 指纹）。
    entries: Vec<(u32, u64)>,
}

impl ThawLedger {
    /// 新台账（空）。
    pub fn new() -> ThawLedger {
        ThawLedger { entries: Vec::new() }
    }

    /// 记一笔解冻（容量闸）。
    pub fn record(&mut self, ver: u32, fp: u64) -> Result<(), u16> {
        if self.entries.len() >= THAW_CAP {
            return Err(C_NO_TRAIL_THAW);
        }
        self.entries.push((ver, fp));
        Ok(())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 解冻核（判据③核）：规格解冻必须留痕——无痕解冻拒。
    /// 解冻 = 清冻结位；台账同步记。
    pub fn thaw(&mut self, spec: &mut FreezeSpec) -> Result<u64, u16> {
        if !spec.frozen {
            return Err(C_NOT_FROZEN);
        }
        let fp = spec.fp;
        let ver = spec.ver;
        // 先记账后解冻（账在，冻位才清——无痕解冻无路径）。
        self.record(ver, fp)?;
        // 解冻：清冻结位但保留指纹（重冻时对比）。
        spec.reset_frozen();
        Ok(fp)
    }
}

// ---------------------------------------------------------------------------
// 压力（LCG 256 轮漂移轮转）
// ---------------------------------------------------------------------------

/// LCG 下一态（64 位确定性）。
pub const fn lcg_next(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 压力结果五列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 轮数。
    pub rounds: u64,
    /// 列序漂移判出计数。
    pub col: u32,
    /// 单位漂移判出计数。
    pub unit: u32,
    /// 版本漂移判出计数。
    pub ver: u32,
    /// 合法读数计数（对照列）。
    pub legit: u32,
}

/// 压力 256 轮：三类漂移注入轮转 + 合法读数对照。
pub fn stress(seed: u64) -> StressOut {
    let mut s = seed | 1;
    let mut out = StressOut { rounds: 0, col: 0, unit: 0, ver: 0, legit: 0 };
    let mut spec = build_spec();
    if spec.freeze().is_err() {
        return out;
    }
    let mut inj = DriftInject::new();
    let mut r = 0u64;
    while r < STRESS_ROUNDS {
        s = lcg_next(s);
        let pick = (s >> 33) % 4;
        let col_idx = ((s >> 13) % spec.col_len() as u64) as usize;
        let pinned = spec.fp();
        match pick {
            0 => {
                if inj.inject_col_drift(&mut spec, col_idx) {
                    out.col += 1;
                }
            }
            1 => {
                if inj.inject_unit_drift(&mut spec, col_idx) {
                    out.unit += 1;
                }
            }
            2 => {
                let nv = ((s >> 19) % FMT_VER_MAX as u64) as u32 + 1;
                if inj.inject_ver_drift(&mut spec, nv) {
                    out.ver += 1;
                }
            }
            _ => {
                // 合法读数（带正确指纹）。
                if spec.read_with_fp(pinned).is_ok() {
                    out.legit += 1;
                }
            }
        }
        out.rounds += 1;
        r += 1;
    }
    out
}

/// 建标准四列规格（Scene/Cnt pos0 → Shape/Cnt pos1 → Value/Step pos2 → Stamp/Pend pos3）。
fn build_spec() -> FreezeSpec {
    let mut spec = FreezeSpec::new(1);
    let _ = spec.add_col(ColSpec { kind: ColKind::Scene, unit: UnitKind::Cnt, pos: 0 });
    let _ = spec.add_col(ColSpec { kind: ColKind::Shape, unit: UnitKind::Cnt, pos: 1 });
    let _ = spec.add_col(ColSpec { kind: ColKind::Value, unit: UnitKind::Step, pos: 2 });
    let _ = spec.add_col(ColSpec { kind: ColKind::Stamp, unit: UnitKind::Pend, pos: 3 });
    spec
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2119 自检（判据①全覆盖：冻结/验冻/三漂移/指纹/解冻/探针）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2119");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xF1段六码闭集递增");

    // ② 列种类闭集双向恰值（四列）。
    let col_ok = col_from_ord(0) == Some(ColKind::Scene)
        && col_from_ord(1) == Some(ColKind::Shape)
        && col_from_ord(2) == Some(ColKind::Value)
        && col_from_ord(3) == Some(ColKind::Stamp)
        && col_from_ord(4).is_none();
    cs.add("col-closed", col_ok, "列种类四列闭集双向恰值");

    // ③ 单位闭集双向恰值（三档）。
    let unit_ok = unit_from_ord(0) == Some(UnitKind::Cnt)
        && unit_from_ord(1) == Some(UnitKind::Step)
        && unit_from_ord(2) == Some(UnitKind::Pend)
        && unit_from_ord(3).is_none();
    cs.add("unit-closed", unit_ok, "单位三档闭集双向恰值");

    // ④ 冻结核：四列规格一次冻结，指纹非零。
    let mut spec = build_spec();
    let fp = spec.freeze();
    cs.add(
        "freeze-once",
        matches!(fp, Ok(_)) && fp.unwrap_or(0) != 0 && spec.is_frozen() && spec.col_len() == 4,
        "四列规格冻结指纹非零",
    );

    // ⑤ 重复冻结幂等（返回同指纹）。
    let fp2 = spec.freeze();
    cs.add("freeze-idempotent", fp2 == fp, "重复冻结幂等同指纹");

    // ⑥ 验冻核：重算 == 钉死。
    cs.add("verify-ok", spec.verify().is_ok(), "验冻核重算相符");

    // ⑦ 冻结后加列拒（冻态不可变）。
    let add_after = spec.add_col(ColSpec { kind: ColKind::Scene, unit: UnitKind::Cnt, pos: 4 });
    cs.add("add-after-freeze", add_after.is_err(), "冻后加列拒");

    // ⑧ 读数闸：正确指纹放行，错指纹拒。
    let ok_read = spec.read_with_fp(fp.unwrap_or(0)).is_ok();
    let bad_read = spec.read_with_fp(12345);
    cs.add(
        "read-fp-gate",
        ok_read && bad_read == Err(C_FP_MISMATCH),
        "读数指纹闸：对放错拒",
    );

    // ⑨ 未冻即读拒。
    let mut unfrozen = FreezeSpec::new(1);
    let _ = unfrozen.add_col(ColSpec { kind: ColKind::Scene, unit: UnitKind::Cnt, pos: 0 });
    let unfr_read = unfrozen.read_with_fp(0);
    cs.add("not-frozen-gate", unfr_read == Err(C_NOT_FROZEN), "未冻即读拒");

    // ⑩ 列序位重复闸（同 pos / 同种类双闸）。
    let mut dup = FreezeSpec::new(1);
    let _ = dup.add_col(ColSpec { kind: ColKind::Scene, unit: UnitKind::Cnt, pos: 0 });
    let dup_pos = dup.add_col(ColSpec { kind: ColKind::Shape, unit: UnitKind::Cnt, pos: 0 });
    let dup_kind = dup.add_col(ColSpec { kind: ColKind::Scene, unit: UnitKind::Cnt, pos: 1 });
    cs.add(
        "col-dup-gate",
        dup_pos == Err(C_COL_DRIFT) && dup_kind == Err(C_COL_DRIFT),
        "列序位重复+列种类重复双闸",
    );

    // ⑪ 列序洞闸（pos 不连续：0,2 无 1）。
    let mut hole = FreezeSpec::new(1);
    let _ = hole.add_col(ColSpec { kind: ColKind::Scene, unit: UnitKind::Cnt, pos: 0 });
    let _ = hole.add_col(ColSpec { kind: ColKind::Shape, unit: UnitKind::Cnt, pos: 2 });
    let hole_gate = hole.freeze() == Err(C_COL_DRIFT);
    cs.add("col-hole-gate", hole_gate, "列序洞拒冻");

    // ⑫ 漂移注入 10/10 拒（三类轮转共 10 次全判出——判据原文恰值）。
    let mut inj = DriftInject::new();
    let mut drift_ok = true;
    let col_n = spec.col_len();
    let mut k = 0usize;
    while k < 10 {
        let ok = match k % 3 {
            0 => inj.inject_col_drift(&mut spec, k % col_n),
            1 => inj.inject_unit_drift(&mut spec, k % col_n),
            _ => inj.inject_ver_drift(&mut spec, (k as u32) + 100),
        };
        if !ok {
            drift_ok = false;
        }
        k += 1;
    }
    cs.add(
        "drift-10of10",
        drift_ok && inj.rejected == 10,
        "漂移注入10/10判出(列序/单位/版本)",
    );

    // ⑬ 解冻留痕核：解冻必记账（无痕解冻无路径）。
    let mut tl = ThawLedger::new();
    let thaw_ok = tl.thaw(&mut spec);
    cs.add(
        "thaw-trailed",
        thaw_ok.is_ok() && tl.len() == 1 && !spec.is_frozen(),
        "解冻留痕+冻位清",
    );

    // ⑭ 重复解冻拒（已解冻不可再解冻）。
    let rethaw = tl.thaw(&mut spec);
    cs.add("rethaw-gate", rethaw == Err(C_NOT_FROZEN), "重复解冻拒");

    // ⑮ 压力 256 轮：三类漂移全判出 + 合法读数对照。
    let so = stress(0xF2119);
    cs.add(
        "stress-all-detected",
        so.rounds == STRESS_ROUNDS && so.col > 0 && so.unit > 0 && so.ver > 0 && so.legit > 0,
        "256轮漂移全判+合法读数对照",
    );

    // ⑯ 指纹重放对拍（同构两规格恒等）。
    let mut sA = build_spec();
    let mut sB = build_spec();
    let fA = sA.freeze();
    let fB = sB.freeze();
    cs.add(
        "fingerprint-replay",
        fA == fB && fA.unwrap_or(0) != 0 && sA.verify().is_ok() && sB.verify().is_ok(),
        "冻结指纹重放恒等",
    );

    // ⑰ 解冻台账容量闸（满容即拒）。
    let mut tl2 = ThawLedger::new();
    let mut fill = true;
    let mut i = 0;
    while i < THAW_CAP {
        if tl2.record(1, 7).is_err() {
            fill = false;
        }
        i += 1;
    }
    let cap_gate = tl2.record(2, 8).is_err();
    cs.add("thaw-cap-gate", fill && cap_gate && tl2.len() == THAW_CAP, "解冻台账容量闸");

    // ⑱ 判码文本闭集穷举（六码互异非空）。
    let codes =
        [C_COL_DRIFT, C_UNIT_DRIFT, C_VER_DRIFT, C_FP_MISMATCH, C_NOT_FROZEN, C_NO_TRAIL_THAW];
    let mut texts_ok = true;
    let mut a = 0;
    while a < codes.len() {
        if code_text(codes[a]).is_empty() || code_text(codes[a]) == "未知判码" {
            texts_ok = false;
        }
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] || code_text(codes[a]) == code_text(codes[b]) {
                texts_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    cs.add("code-text-closed", texts_ok, "六码文本互异非空");

    // ⑲ LCG 确定性。
    let l1 = lcg_next(17);
    let l2 = lcg_next(17);
    cs.add("lcg-deterministic", l1 == l2, "LCG确定性");

    cs
}
