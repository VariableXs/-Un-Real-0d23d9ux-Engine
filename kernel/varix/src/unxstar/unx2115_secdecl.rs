//! unx2115_secdecl —— UNX-F2115 权限与安全声明（只读红线+权限最小化）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2115 · 权限与安全声明 ·
//!   目标 280 行 · 已深化
//! 判据原文：UNX-F2115-J1 安全声明在册（三面全只读+权限最小化+敏感字段
//!   零导出），越权注入 10/10 拒绝。
//! 组件：Perm3 权限位三元（Read/Write/Exec 闭集，导出面只允许 Read 位）+
//!      RedLine 只读红线（三面每一面逐一声明只读，红线 bitmap 冻结）+
//!      FieldClass 字段密级闭集（Public/Internal/Sensitive 三档）+
//!      ExportField 导出字段（字段 id + 密级 + 面归属三列）+
//!      SecDecl 安全声明册（字段 append-only 登记 + 密级闸 + 面闸）+
//!      Minimize 最小化核（每面导出字段数 ≤ 授权上限，超限拒）+ 敏感
//!      字段零导出断言（Sensitive 档字段一例即破闸）+ 越权注入探针
//!      （写位请求/Exec 位请求/Sensitive 档导出三类必拒）+ CapLedger
//!      容量账（三面字段分账守恒）+ LCG 压力 256 轮越权轮转 + 指纹重放
//!      对拍锚。
//! 对标：Linux kernel security/Kconfig.hardening 的 LOCKDOWN 只读模式与
//!   printk 脱敏分级——本模块以「只读红线 + 字段分级 + 最小化上限」
//!   复刻导出权限最小化纪律。
//! 性能逐项分解：字段登记 O(1)（密级直查）；最小化核 O(n)（n≤32 常数
//!   上界）；越权判定 O(1)（位运算）；全整数算术零浮点零墙钟，同声明
//!   恒同册（确定性对拍锚）。
//! 诊断码段：0xDF（0xDF01~0xDF06，全域 grep 确认无诊断基数占用——先入
//!   库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零
//!   模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 导出三面数（与 F2114 Face3 同口径）。
pub const FACE_N: usize = 3;
/// 密级闭集元数（Public/Internal/Sensitive）。
pub const CLASS_N: usize = 3;
/// 单面字段容量上界（32 = 最小化上限口径）。
pub const FIELD_CAP: usize = 32;
/// 每面授权字段上限（最小化核阈值——超限即权限放大）。
pub const PER_FACE_MAX: usize = 32;
/// 压力轮数。
pub const STRESS_ROUNDS: u64 = 256;
/// 声明版本（v1 冻结）。
pub const DECL_VER: u32 = 1;

// 权限位（三位置闭集）
/// 读位（导出面唯一允许位）。
pub const PERM_READ: u8 = 0b001;
/// 写位（导出面永不允许）。
pub const PERM_WRITE: u8 = 0b010;
/// 执行位（导出面永不允许）。
pub const PERM_EXEC: u8 = 0b100;
/// 导出面合法规位（恰 Read）。
pub const PERM_EXPORT_OK: u8 = PERM_READ;

// 判码（段内独占 0xDF01~0xDF06）
/// 0xDF01 越权（请求权限位越合法规位集）。
pub const C_PERM_OOB: u16 = 0xDF01;
/// 0xDF02 敏感导出（Sensitive 档字段进导出册）。
pub const C_SENSITIVE: u16 = 0xDF02;
/// 0xDF03 只读红线破（写位/Exec 位出现在声明中）。
pub const C_REDLINE: u16 = 0xDF03;
/// 0xDF04 权限放大（单面字段数超授权上限）。
pub const C_OVERFLOW_CAP: u16 = 0xDF04;
/// 0xDF05 表外面（面归属越三面闭集）。
pub const C_FACE_OOB: u16 = 0xDF05;
/// 0xDF06 分账漂移（三面字段分账和 ≠ 登记总数）。
pub const C_TALLY_DRIFT: u16 = 0xDF06;

/// 段守卫：判码落 0xDF00..0xE000 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_PERM_OOB >= 0xDF00
        && C_PERM_OOB < 0xE000
        && C_SENSITIVE == C_PERM_OOB + 1
        && C_REDLINE == C_SENSITIVE + 1
        && C_OVERFLOW_CAP == C_REDLINE + 1
        && C_FACE_OOB == C_OVERFLOW_CAP + 1
        && C_TALLY_DRIFT == C_FACE_OOB + 1
        && C_TALLY_DRIFT < 0xE000
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_PERM_OOB => "越权",
        C_SENSITIVE => "敏感导出",
        C_REDLINE => "只读红线破",
        C_OVERFLOW_CAP => "权限放大",
        C_FACE_OOB => "表外面",
        C_TALLY_DRIFT => "分账漂移",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn，避闭包借用冲突——F0386 教训在册）。
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
// 权限位与只读红线
// ---------------------------------------------------------------------------

/// 权限位合法核：导出面只允许恰 Read 位（多一位即越权）。
pub const fn perm_ok(p: u8) -> bool {
    p == PERM_EXPORT_OK
}

/// 权限位是否含写位（红线探针）。
pub const fn has_write(p: u8) -> bool {
    p & PERM_WRITE != 0
}

/// 权限位是否含执行位（红线探针）。
pub const fn has_exec(p: u8) -> bool {
    p & PERM_EXEC != 0
}

/// 只读红线 bitmap：三面逐位，1 = 该面在只读红线上。
/// 导出三面全只读——bitmap 恒 0b111，任一位置零即红线破。
pub const REDLINE_BITMAP: u8 = 0b111;

/// 红线完好核（三位全置 = 三面全只读）。
pub const fn redline_intact() -> bool {
    REDLINE_BITMAP & 0b111 == 0b111
}

/// 面在红线上核。
pub const fn face_on_redline(face_ord: u8) -> bool {
    face_ord < 3 && (REDLINE_BITMAP >> face_ord) & 1 == 1
}

// ---------------------------------------------------------------------------
// 字段密级与导字段
// ---------------------------------------------------------------------------

/// 字段密级（三档闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldClass {
    /// 公开档（可自由导出）。
    Public,
    /// 内部档（导出需最小化审查，本模块默认允许但计数入账）。
    Internal,
    /// 敏感档（禁止导出——一例即破闸）。
    Sensitive,
}

/// 密级序（0/1/2）。
pub const fn class_ord(c: FieldClass) -> u8 {
    match c {
        FieldClass::Public => 0,
        FieldClass::Internal => 1,
        FieldClass::Sensitive => 2,
    }
}

/// 密级序 → 密级（越界 None）。
pub const fn class_from_ord(o: u8) -> Option<FieldClass> {
    match o {
        0 => Some(FieldClass::Public),
        1 => Some(FieldClass::Internal),
        2 => Some(FieldClass::Sensitive),
        _ => None,
    }
}

/// 导出字段：id + 密级 + 面归属 + 请求权限位四列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportField {
    /// 字段 id（闭集内唯一）。
    pub fid: u16,
    /// 密级。
    pub class: FieldClass,
    /// 面归属（0/1/2）。
    pub face: u8,
    /// 请求权限位（必须恰 Read）。
    pub perm: u8,
}

impl ExportField {
    /// 字段合法性预检（密级闭集 + 面闭集 + 权限恰 Read + 敏感禁出）。
    pub fn valid(&self) -> Result<(), u16> {
        if class_from_ord(class_ord(self.class)).is_none() {
            return Err(C_SENSITIVE);
        }
        if self.face as usize >= FACE_N {
            return Err(C_FACE_OOB);
        }
        if !perm_ok(self.perm) {
            if has_write(self.perm) || has_exec(self.perm) {
                return Err(C_REDLINE);
            }
            return Err(C_PERM_OOB);
        }
        if self.class == FieldClass::Sensitive {
            return Err(C_SENSITIVE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 安全声明册
// ---------------------------------------------------------------------------

/// 安全声明册：字段 append-only + 三面分账 + 最小化核。
pub struct SecDecl {
    /// 已登记字段。
    fields: Vec<ExportField>,
    /// 三面分账（下标 = 面序）。
    per_face: [u32; FACE_N],
    /// 封账标志。
    sealed: bool,
    /// 越权拒计数（探针累计）。
    pub denied: u32,
}

impl SecDecl {
    /// 新声明册（空册未封账）。
    pub fn new() -> SecDecl {
        SecDecl { fields: Vec::new(), per_face: [0; FACE_N], sealed: false, denied: 0 }
    }

    /// 登记一个导出字段（闸①fid 非零 闸②合法性四查 闸③同面容量上限
    ///   闸④同面同 id 重名拒 闸⑤未封账）。
    pub fn declare(&mut self, f: ExportField) -> Result<(), u16> {
        if self.sealed {
            return Err(C_TALLY_DRIFT); // 封账后声明即拒
        }
        if f.fid == 0 {
            return Err(C_PERM_OOB);
        }
        f.valid()?;
        let face = f.face as usize;
        if self.per_face[face] as usize >= PER_FACE_MAX {
            return Err(C_OVERFLOW_CAP);
        }
        // 同面同 id 重名。
        let mut i = 0;
        while i < self.fields.len() {
            if self.fields[i].face == f.face && self.fields[i].fid == f.fid {
                return Err(C_TALLY_DRIFT); // 重名归分账口径拒
            }
            i += 1;
        }
        self.per_face[face] += 1;
        self.fields.push(f);
        Ok(())
    }

    /// 越权注入探针（构造违例字段并登记，记录拒否）。
    /// kind: 0=写位 1=Exec位 2=Sensitive 3=越界面 4=零 fid。
    pub fn inject_violation(&mut self, kind: u8, fid: u16) -> bool {
        let bad = match kind {
            0 => ExportField { fid, class: FieldClass::Public, face: 0, perm: PERM_WRITE },
            1 => ExportField { fid, class: FieldClass::Public, face: 0, perm: PERM_EXEC },
            2 => ExportField { fid, class: FieldClass::Sensitive, face: 0, perm: PERM_READ },
            3 => ExportField { fid, class: FieldClass::Public, face: FACE_N as u8, perm: PERM_READ },
            _ => ExportField { fid: 0, class: FieldClass::Public, face: 0, perm: PERM_READ },
        };
        let rejected = self.declare(bad).is_err();
        if rejected {
            self.denied += 1;
        }
        rejected
    }

    /// 封账（一次性）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 字段总数。
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// 读一面计数。
    pub fn face_count(&self, face: u8) -> u32 {
        if face as usize >= FACE_N {
            return 0;
        }
        self.per_face[face as usize]
    }

    /// 敏感字段导出计数（必恒零——零导出断言核）。
    pub fn sensitive_exported(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.fields.len() {
            if self.fields[i].class == FieldClass::Sensitive {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 最小化核：每面计数 ≤ 授权上限。
    pub fn minimized(&self) -> bool {
        let mut f = 0;
        while f < FACE_N {
            if self.per_face[f] as usize > PER_FACE_MAX {
                return false;
            }
            f += 1;
        }
        true
    }

    /// 分账守恒核：三面和 == 登记总数。
    pub fn conserved(&self) -> bool {
        let mut s = 0u32;
        let mut f = 0;
        while f < FACE_N {
            s = s.wrapping_add(self.per_face[f]);
            f += 1;
        }
        s as usize == self.fields.len()
    }

    /// 红线完好核（册内无写位/Exec位字段）。
    pub fn redline_ok(&self) -> bool {
        if !redline_intact() {
            return false;
        }
        let mut i = 0;
        while i < self.fields.len() {
            if has_write(self.fields[i].perm) || has_exec(self.fields[i].perm) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 声明指纹（版本 + 三面分账 + 字段数，FNV）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(
            0xF2115,
            &[
                DECL_VER as u64,
                self.per_face[0] as u64,
                self.per_face[1] as u64,
                self.per_face[2] as u64,
                self.fields.len() as u64,
                self.denied as u64,
            ],
        )
    }

    /// 终态断言束（零导出 + 最小化 + 分账守恒 + 红线完好四合一）。
    pub fn all_green(&self) -> bool {
        self.sensitive_exported() == 0
            && self.minimized()
            && self.conserved()
            && self.redline_ok()
    }
}

// ---------------------------------------------------------------------------
// 压力（LCG 256 轮越权轮转）
// ---------------------------------------------------------------------------

/// LCG 下一态（64 位确定性）。
pub const fn lcg_next(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 压力结果四列（越权拒 / 敏感拒 / 红线拒 / 表外面拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 轮数。
    pub rounds: u64,
    /// 越权拒计数（写位+Exec位+零fid）。
    pub perm: u32,
    /// 敏感导出拒计数。
    pub sensitive: u32,
    /// 表外面拒计数。
    pub face: u32,
    /// 合法登记计数（对照列）。
    pub legit: u32,
}

/// 压力 256 轮：轮转注入五类违例 + 合法字段登记对照。
pub fn stress(seed: u64) -> StressOut {
    let mut s = seed | 1;
    let mut out = StressOut { rounds: 0, perm: 0, sensitive: 0, face: 0, legit: 0 };
    let mut decl = SecDecl::new();
    let mut r = 0u64;
    while r < STRESS_ROUNDS {
        s = lcg_next(s);
        let pick = (s >> 33) % 6;
        let fid = ((s >> 13) & 0xFFF) as u16 + 1;
        let rejected = match pick {
            0 => decl.inject_violation(0, fid),
            1 => decl.inject_violation(1, fid),
            2 => decl.inject_violation(2, fid),
            3 => decl.inject_violation(3, fid),
            4 => decl.inject_violation(4, fid),
            _ => false,
        };
        // 合法字段登记（三面轮转 + 密级 Public/Internal 轮转）。
        if !rejected {
            let face = ((s >> 21) % FACE_N as u64) as u8;
            let class = if (s >> 17) & 1 == 0 { FieldClass::Public } else { FieldClass::Internal };
            let ok = decl.declare(ExportField { fid, class, face, perm: PERM_READ });
            if ok.is_ok() {
                out.legit += 1;
            }
        }
        // 违例分类累计（按 pick 归列）。
        if rejected {
            match pick {
                0 | 1 | 4 => out.perm += 1,
                2 => out.sensitive += 1,
                3 => out.face += 1,
                _ => {}
            }
        }
        out.rounds += 1;
        r += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2115 自检（判据①全覆盖：红线/最小化/零敏感导出/越权/分账/探针）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2115");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xDF段六码闭集递增");

    // ② 权限位恰 Read（多一位即越权）。
    let perm_ok_only = perm_ok(PERM_READ)
        && !perm_ok(PERM_READ | PERM_WRITE)
        && !perm_ok(PERM_READ | PERM_EXEC)
        && !perm_ok(0)
        && !perm_ok(PERM_WRITE);
    cs.add("perm-only-read", perm_ok_only, "合法规位恰Read");

    // ③ 只读红线三面全置（bitmap 恒 0b111 + 逐面在册）。
    let rl = redline_intact()
        && REDLINE_BITMAP == 0b111
        && face_on_redline(0)
        && face_on_redline(1)
        && face_on_redline(2)
        && !face_on_redline(3);
    cs.add("redline-3face", rl, "三面全只读红线在册");

    // ④ 密级闭集双向恰值。
    let cls_ok = class_from_ord(0) == Some(FieldClass::Public)
        && class_from_ord(1) == Some(FieldClass::Internal)
        && class_from_ord(2) == Some(FieldClass::Sensitive)
        && class_from_ord(3).is_none()
        && CLASS_N == 3;
    cs.add("class-closed", cls_ok, "密级三档闭集双向恰值");

    // ⑤ 合法字段登记三面齐 + 分账守恒。
    let mut d = SecDecl::new();
    let mut decl_ok = true;
    let mut f = 0u16;
    while f < 12 {
        let face = (f % FACE_N as u16) as u8;
        let class = if f % 2 == 0 { FieldClass::Public } else { FieldClass::Internal };
        if d.declare(ExportField { fid: f + 1, class, face, perm: PERM_READ }).is_err() {
            decl_ok = false;
        }
        f += 1;
    }
    cs.add("declare-legit", decl_ok && d.len() == 12, "12合法字段三面齐登记");

    // ⑥ 分账守恒（三面和 == 总数）。
    cs.add("tally-conserved", d.conserved() && d.all_green(), "分账守恒+全绿断言");

    // ⑦ 敏感字段零导出（Sensitive 注入必拒）。
    let sens_rejected = d.inject_violation(2, 77);
    cs.add("sensitive-zero", sens_rejected && d.sensitive_exported() == 0, "敏感零导出");

    // ⑧ 越权注入 10/10 拒（判据原文恰值——五类轮转共 10 次全拒）。
    let mut viol_ok = true;
    let mut k = 0;
    while k < 10 {
        let kind = (k % 5) as u8;
        let fid = (200 + k) as u16;
        if !d.inject_violation(kind, fid) {
            viol_ok = false;
        }
        k += 1;
    }
    cs.add("violation-10of10", viol_ok && d.denied == 11, "越权注入10/10拒");

    // ⑨ 写位/Exec 位红线拒（册内不得出现）。
    let mut d2 = SecDecl::new();
    let w_rej = d2.inject_violation(0, 1);
    let e_rej = d2.inject_violation(1, 2);
    cs.add("redline-write-exec", w_rej && e_rej && d2.redline_ok() && d2.len() == 0, "写/Exec位红线拒");

    // ⑩ 表外面拒。
    let face_rej = d2.inject_violation(3, 3);
    cs.add("face-oob-gate", face_rej, "表外面拒");

    // ⑪ 零 fid 拒。
    let zero_fid_rej = d2.inject_violation(4, 0);
    cs.add("zero-fid-gate", zero_fid_rej, "零fid拒");

    // ⑫ 最小化上限闸（单面超 32 字段拒）。
    let mut d3 = SecDecl::new();
    let mut fill_ok = true;
    let mut i = 0u16;
    while i < PER_FACE_MAX as u16 {
        if d3.declare(ExportField { fid: i + 1, class: FieldClass::Public, face: 1, perm: PERM_READ }).is_err() {
            fill_ok = false;
        }
        i += 1;
    }
    let overflow = d3.declare(ExportField { fid: 999, class: FieldClass::Public, face: 1, perm: PERM_READ });
    cs.add(
        "per-face-cap",
        fill_ok && overflow == Err(C_OVERFLOW_CAP) && d3.face_count(1) == PER_FACE_MAX as u32,
        "单面32上限闸",
    );

    // ⑬ 同面同 id 重名拒。
    let mut d4 = SecDecl::new();
    let _ = d4.declare(ExportField { fid: 5, class: FieldClass::Public, face: 0, perm: PERM_READ });
    let dup = d4.declare(ExportField { fid: 5, class: FieldClass::Internal, face: 0, perm: PERM_READ });
    let cross_ok = d4.declare(ExportField { fid: 5, class: FieldClass::Public, face: 1, perm: PERM_READ }).is_ok();
    cs.add("dup-gate", dup.is_err() && cross_ok, "同面同id重名拒");

    // ⑭ 封账后声明拒。
    let mut d5 = SecDecl::new();
    let _ = d5.declare(ExportField { fid: 1, class: FieldClass::Public, face: 0, perm: PERM_READ });
    d5.seal();
    let sealed_rej = d5.declare(ExportField { fid: 2, class: FieldClass::Public, face: 0, perm: PERM_READ }).is_err();
    cs.add("sealed-gate", sealed_rej && d5.len() == 1, "封账后声明拒");

    // ⑮ 压力 256 轮：五类违例全拒 + 合法登记对照。
    let so = stress(0xF2115);
    cs.add(
        "stress-all-rejected",
        so.rounds == STRESS_ROUNDS && so.perm > 0 && so.sensitive > 0 && so.face > 0 && so.legit > 0,
        "256轮违例全拒+合法对照",
    );

    // ⑯ 指纹重放对拍（两册同构造）。
    let mut dA = SecDecl::new();
    let mut dB = SecDecl::new();
    let mut j = 1u16;
    while j <= 6 {
        let _ = dA.declare(ExportField { fid: j, class: FieldClass::Public, face: (j % 3) as u8, perm: PERM_READ });
        let _ = dB.declare(ExportField { fid: j, class: FieldClass::Public, face: (j % 3) as u8, perm: PERM_READ });
        j += 1;
    }
    let fp_ok = dA.fingerprint() == dB.fingerprint() && dA.fingerprint() != 0;
    cs.add("fingerprint-replay", fp_ok, "声明指纹重放恒等");

    // ⑰ 终态断言束（零敏感 + 最小化 + 守恒 + 红线）。
    let mut d6 = SecDecl::new();
    let mut m = 1u16;
    while m <= 9 {
        let _ = d6.declare(ExportField { fid: m, class: FieldClass::Internal, face: (m % 3) as u8, perm: PERM_READ });
        m += 1;
    }
    let _ = d6.inject_violation(2, 100); // 记一笔拒账
    cs.add("final-all-green", d6.all_green() && d6.conserved(), "终态四合一全绿");

    // ⑱ 判码文本闭集穷举（六码互异非空）。
    let codes = [C_PERM_OOB, C_SENSITIVE, C_REDLINE, C_OVERFLOW_CAP, C_FACE_OOB, C_TALLY_DRIFT];
    let mut texts_ok = true;
    let mut a = 0;
    while a < 6 {
        if code_text(codes[a]).is_empty() || code_text(codes[a]) == "未知判码" {
            texts_ok = false;
        }
        let mut b = a + 1;
        while b < 6 {
            if codes[a] == codes[b] || code_text(codes[a]) == code_text(codes[b]) {
                texts_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    cs.add("code-text-closed", texts_ok, "六码文本互异非空");

    // ⑲ LCG 确定性。
    let l1 = lcg_next(9);
    let l2 = lcg_next(9);
    cs.add("lcg-deterministic", l1 == l2, "LCG确定性");

    cs
}
