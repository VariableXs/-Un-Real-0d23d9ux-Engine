//! UNX-F11299 ldd 安全语义（不可信件诊断）（0x1f6e0 段）
//!
//! 锚点：UNX-F11299-J1 不可信二进制一律「警告 + 被动诊断」，执行面恒为零，
//! ldd 语义不新增任何执行入口。
//!
//! 纯功能模型：ldd 对目标二进制的处置分为两条安全路径——可信动态目标走
//! 装载器追踪（只读映射，不执行目标），不可信或静态目标转被动诊断（只读
//! 文件元数据与动态段）。判定不可信的三类条件是：组可写/他人可写位、
//! setuid 位、setgid 位。任何路径都必须满足两条硬不变量：executed 恒假、
//! surface_opened 恒假；不可信目标若缺警告，校验器直接拒账。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6e0;
/// 目标路径为空
pub const LD_EPATH: u32 = SEG_BASE + 0x01;
/// 策略允许执行，拒绝采纳
pub const LD_EEXEC: u32 = SEG_BASE + 0x02;
/// 不可信目标缺少警告
pub const LD_EWARN: u32 = SEG_BASE + 0x03;
/// 出现执行面（executed / surface 为真）
pub const LD_ESURF: u32 = SEG_BASE + 0x04;
/// 冻结后检查
pub const LD_EFROZEN: u32 = SEG_BASE + 0x05;
/// 重复登记同一路径
pub const LD_EDUP: u32 = SEG_BASE + 0x06;
/// 元数据非法（路径与模式不匹配的兜底）
pub const LD_EMETA: u32 = SEG_BASE + 0x07;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 组可写 / 他人可写位。
pub const MODE_GW: u32 = 0o020;
pub const MODE_OW: u32 = 0o002;

fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            x = fold_byte(x, *b);
        }
        k = k.wrapping_add(1);
    }
    x
}

fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut s = 0u32;
    while s < 32 {
        x = fold_byte(x, ((v >> s) & 0xff) as u8);
        s = s.wrapping_add(8);
    }
    x
}

// ---------------------------------------------------------------- 元数据
/// 待检二进制的只读元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinaryMeta {
    pub path: &'static str,
    pub mode: u32,
    pub uid: u32,
    pub dynamic: bool,
    pub setuid: bool,
    pub setgid: bool,
}

impl BinaryMeta {
    /// 三个不可信来源：组/他人可写、setuid、setgid。
    pub fn untrusted(&self) -> bool {
        (self.mode & (MODE_GW | MODE_OW)) != 0 || self.setuid || self.setgid
    }

    pub fn writable_by_others(&self) -> bool {
        (self.mode & MODE_OW) != 0
    }

    pub fn writable_by_group(&self) -> bool {
        (self.mode & MODE_GW) != 0
    }

    pub fn mode_bits_ok(&self) -> bool {
        self.mode <= 0o7777
    }
}

/// 判定结果的动作：追踪 / 被动诊断。两者都不执行目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Trace,
    Passive,
}

impl Action {
    pub fn name(&self) -> &'static str {
        match self {
            Action::Trace => "trace",
            Action::Passive => "passive",
        }
    }
}

/// 警告种类；None 仅供可信动态目标使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    None,
    NotDynamic,
    Untrusted,
}

impl Warning {
    pub fn text(&self) -> Option<&'static str> {
        match self {
            Warning::None => None,
            Warning::NotDynamic => Some("not a dynamic executable"),
            Warning::Untrusted => Some("warning: binary is not trusted; executing it is refused"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Warning::None => "none",
            Warning::NotDynamic => "not-dynamic",
            Warning::Untrusted => "untrusted",
        }
    }
}

/// 单次检查的判定：动作、警告、执行与执行面两个硬零。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub action: Action,
    pub warning: Warning,
    pub executed: bool,
    pub surface: bool,
}

impl Verdict {
    /// 硬不变量校验：执行面恒零；不可信必有警告；静态必有静态警告。
    pub fn invariant(&self, meta: &BinaryMeta) -> Result<(), u32> {
        if self.executed || self.surface {
            return Err(LD_ESURF);
        }
        if meta.untrusted() && self.warning != Warning::Untrusted {
            return Err(LD_EWARN);
        }
        if !meta.dynamic && self.warning != Warning::NotDynamic {
            return Err(LD_EWARN);
        }
        if self.action == Action::Trace && (meta.untrusted() || !meta.dynamic) {
            return Err(LD_EWARN);
        }
        Ok(())
    }

    pub fn digest(&self, meta: &BinaryMeta) -> u64 {
        let mut h = fold_str(FNV_OFF, meta.path);
        h = fold_u32(h, meta.mode);
        h = fold_u32(h, meta.uid);
        h = fold_byte(h, meta.dynamic as u8);
        h = fold_byte(h, meta.setuid as u8);
        h = fold_byte(h, meta.setgid as u8);
        h = fold_str(h, self.action.name());
        h = fold_str(h, self.warning.label());
        h = fold_byte(h, self.executed as u8);
        fold_byte(h, self.surface as u8)
    }
}

/// 安全策略：唯一的合法策略是「不执行」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub allow_exec: bool,
}

impl Policy {
    pub fn safe() -> Self {
        Policy { allow_exec: false }
    }

    pub fn unsafe_exec() -> Self {
        Policy { allow_exec: true }
    }
}

/// 纯判定：静态优先于不可信；可信动态才允许追踪。
pub fn decide(meta: &BinaryMeta) -> Verdict {
    if !meta.dynamic {
        Verdict {
            action: Action::Passive,
            warning: Warning::NotDynamic,
            executed: false,
            surface: false,
        }
    } else if meta.untrusted() {
        Verdict {
            action: Action::Passive,
            warning: Warning::Untrusted,
            executed: false,
            surface: false,
        }
    } else {
        Verdict {
            action: Action::Trace,
            warning: Warning::None,
            executed: false,
            surface: false,
        }
    }
}

/// 一条检查记录：元数据 + 判定 + 指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckRow {
    pub meta: BinaryMeta,
    pub verdict: Verdict,
    pub digest: u64,
}

/// ldd 安全检查账：只读判定，执行面恒零。
#[derive(Clone, Debug)]
pub struct LddAudit {
    policy: Policy,
    rows: Vec<CheckRow>,
    seal: Option<u64>,
    pub attempts: u32,
    pub rejects: u32,
    pub traces: u32,
    pub passive: u32,
    pub refusals: u32,
    pub warnings: u32,
    pub executed: u32,
    pub surfaces: u32,
}

impl LddAudit {
    /// 安全策略下的空账。
    pub fn safe() -> Self {
        LddAudit {
            policy: Policy::safe(),
            rows: Vec::new(),
            seal: None,
            attempts: 0,
            rejects: 0,
            traces: 0,
            passive: 0,
            refusals: 0,
            warnings: 0,
            executed: 0,
            surfaces: 0,
        }
    }

    /// 采纳策略：允许执行的策略一律拒。
    pub fn new(policy: Policy) -> Result<Self, u32> {
        if policy.allow_exec {
            return Err(LD_EEXEC);
        }
        Ok(LddAudit::safe())
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn rows(&self) -> &[CheckRow] {
        self.rows.as_slice()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    pub fn find(&self, path: &str) -> Option<&CheckRow> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.meta.path == path {
                    return Some(r);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 执行面计数：executed + surfaces，安全模型下恒为零。
    pub fn surface_total(&self) -> u32 {
        self.executed.wrapping_add(self.surfaces)
    }

    /// 所有不可信目标都带 Untrusted 警告。
    pub fn all_untrusted_warned(&self) -> bool {
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                if r.meta.untrusted() && r.verdict.warning != Warning::Untrusted {
                    return false;
                }
            }
            k = k.wrapping_add(1);
        }
        true
    }

    pub fn unique_paths(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                let mut seen = false;
                let mut j = 0usize;
                while j < k {
                    if let Some(prev) = self.rows.get(j) {
                        if prev.meta.path == r.meta.path {
                            seen = true;
                        }
                    }
                    j = j.wrapping_add(1);
                }
                if !seen {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 检查一个目标：空路径、非法模式、重复路径、不安全策略逐关拒。
    pub fn inspect(&mut self, meta: &BinaryMeta) -> Result<u64, u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LD_EFROZEN);
        }
        if meta.path.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LD_EPATH);
        }
        if !meta.mode_bits_ok() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LD_EMETA);
        }
        if self.policy.allow_exec {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LD_EEXEC);
        }
        if self.find(meta.path).is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(LD_EDUP);
        }
        let verdict = decide(meta);
        if let Err(e) = verdict.invariant(meta) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(e);
        }
        let digest = verdict.digest(meta);
        self.attempts = self.attempts.wrapping_add(1);
        match verdict.action {
            Action::Trace => self.traces = self.traces.wrapping_add(1),
            Action::Passive => self.passive = self.passive.wrapping_add(1),
        }
        if verdict.warning == Warning::Untrusted {
            self.refusals = self.refusals.wrapping_add(1);
        }
        if verdict.warning.text().is_some() {
            self.warnings = self.warnings.wrapping_add(1);
        }
        self.rows.push(CheckRow {
            meta: *meta,
            verdict,
            digest,
        });
        Ok(digest)
    }

    pub fn fold(&self) -> u64 {
        let mut h = fold_byte(FNV_OFF, self.policy.allow_exec as u8);
        let mut k = 0usize;
        while k < self.rows.len() {
            if let Some(r) = self.rows.get(k) {
                h = fold_str(h, r.meta.path);
                h = fold_u64(h, r.digest);
                h = fold_str(h, r.verdict.action.name());
                h = fold_str(h, r.verdict.warning.label());
            }
            k = k.wrapping_add(1);
        }
        h = fold_u32(h, self.attempts);
        h = fold_u32(h, self.traces);
        h = fold_u32(h, self.passive);
        h = fold_u32(h, self.refusals);
        h = fold_u32(h, self.warnings);
        fold_u32(h, self.rejects)
    }

    /// 幂等冻结：检查账与策略一并定格。
    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

fn fold_u64(h: u64, v: u64) -> u64 {
    fold_u32(fold_u32(h, v as u32), (v >> 32) as u32)
}

// ---------------------------------------------------------------- 基准案例
pub static TRUSTED_BIN: BinaryMeta = BinaryMeta {
    path: "/usr/bin/probe",
    mode: 0o755,
    uid: 0,
    dynamic: true,
    setuid: false,
    setgid: false,
};

pub static WORLD_WRITABLE: BinaryMeta = BinaryMeta {
    path: "/tmp/probe",
    mode: 0o777,
    uid: 1000,
    dynamic: true,
    setuid: false,
    setgid: false,
};

pub static STATIC_BIN: BinaryMeta = BinaryMeta {
    path: "/usr/bin/busybox",
    mode: 0o755,
    uid: 0,
    dynamic: false,
    setuid: false,
    setgid: false,
};

pub static SETUID_BIN: BinaryMeta = BinaryMeta {
    path: "/usr/bin/sudo",
    mode: 0o4755,
    uid: 0,
    dynamic: true,
    setuid: true,
    setgid: false,
};

pub static GROUP_WRITABLE: BinaryMeta = BinaryMeta {
    path: "/opt/team/probe",
    mode: 0o775,
    uid: 0,
    dynamic: true,
    setuid: false,
    setgid: false,
};

pub static LIBC_SO: BinaryMeta = BinaryMeta {
    path: "/usr/lib/x86_64-linux-gnu/libc.so.6",
    mode: 0o755,
    uid: 0,
    dynamic: true,
    setuid: false,
    setgid: false,
};

/// 基准账：六个目标，两追踪四被动，警告四条。
pub fn build_audit() -> LddAudit {
    let mut audit = LddAudit::safe();
    let all = [
        TRUSTED_BIN,
        WORLD_WRITABLE,
        STATIC_BIN,
        SETUID_BIN,
        GROUP_WRITABLE,
        LIBC_SO,
    ];
    let mut k = 0usize;
    while k < all.len() {
        if let Some(m) = all.get(k) {
            let _ = audit.inspect(m);
        }
        k = k.wrapping_add(1);
    }
    audit
}

/// 四路对抗：空路径、不安全策略、缺警告判定、冻结检查。
pub fn inject_four() -> [bool; 4] {
    let mut hits = [false; 4];

    let mut a = LddAudit::safe();
    let empty = BinaryMeta {
        path: "",
        mode: 0o755,
        uid: 0,
        dynamic: true,
        setuid: false,
        setgid: false,
    };
    hits[0] = a.inspect(&empty) == Err(LD_EPATH);

    hits[1] = match LddAudit::new(Policy::unsafe_exec()) {
        Ok(_) => false,
        Err(e) => e == LD_EEXEC,
    };

    let silent = Verdict {
        action: Action::Passive,
        warning: Warning::None,
        executed: false,
        surface: false,
    };
    hits[2] = silent.invariant(&WORLD_WRITABLE) == Err(LD_EWARN);

    let mut d = build_audit();
    let _ = d.freeze();
    let late = BinaryMeta {
        path: "/opt/late/probe",
        mode: 0o755,
        uid: 0,
        dynamic: true,
        setuid: false,
        setgid: false,
    };
    hits[3] = d.inspect(&late) == Err(LD_EFROZEN);

    hits
}

// ---------------------------------------------------------------- 判据
pub fn run_unx11299_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11299");
    let mut base = build_audit();

    // J1 六目标总账
    s.add(
        "J1-六目标总账",
        base.len() == 6
            && base.attempts == 6
            && base.traces == 2
            && base.passive == 4
            && base.unique_paths() == 6,
        "6 次检查：2 追踪 + 4 被动，路径各异",
    );

    // J2 不可信三例
    let mut untrusted = 0u32;
    let mut untrusted_ok = true;
    let mut k = 0usize;
    while k < base.rows().len() {
        if let Some(r) = base.rows().get(k) {
            if r.meta.untrusted() {
                untrusted = untrusted.wrapping_add(1);
                if r.verdict.warning != Warning::Untrusted || r.verdict.action != Action::Passive {
                    untrusted_ok = false;
                }
            }
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "J2-不可信三例",
        untrusted == 3
            && untrusted_ok
            && base.refusals == 3
            && base.all_untrusted_warned(),
        "可写/ setuid / 组可写三例全部警告且被动",
    );

    // J3 静态目标
    let static_row = base.find("/usr/bin/busybox");
    s.add(
        "J3-静态目标",
        static_row.map(|r| r.verdict.warning) == Some(Warning::NotDynamic)
            && static_row.map(|r| r.verdict.action) == Some(Action::Passive)
            && static_row.map(|r| r.meta.dynamic) == Some(false),
        "静态目标报 not a dynamic executable",
    );

    // J4 可信目标
    let trusted = base.find("/usr/bin/probe");
    let libc = base.find("/usr/lib/x86_64-linux-gnu/libc.so.6");
    s.add(
        "J4-可信目标",
        trusted.map(|r| r.verdict.action) == Some(Action::Trace)
            && trusted.map(|r| r.verdict.warning) == Some(Warning::None)
            && libc.map(|r| r.verdict.action) == Some(Action::Trace)
            && libc.map(|r| r.verdict.executed) == Some(false),
        "两个可信动态目标走追踪且无警告",
    );

    // J5 执行面为零
    let mut zero_rows = true;
    let mut j = 0usize;
    while j < base.rows().len() {
        if let Some(r) = base.rows().get(j) {
            if r.verdict.executed || r.verdict.surface {
                zero_rows = false;
            }
        }
        j = j.wrapping_add(1);
    }
    s.add(
        "J5-执行面为零",
        base.surface_total() == 0
            && base.executed == 0
            && base.surfaces == 0
            && zero_rows,
        "六条判定 executed/surface 全为假",
    );

    // J6 警告计数
    s.add(
        "J6-警告计数",
        base.warnings == 4,
        "三不可信加一静态共四条警告",
    );

    // J7 空路径拒
    let mut a = LddAudit::safe();
    let empty = BinaryMeta {
        path: "",
        mode: 0o755,
        uid: 0,
        dynamic: true,
        setuid: false,
        setgid: false,
    };
    s.add(
        "J7-空路径拒",
        a.inspect(&empty) == Err(LD_EPATH) && a.len() == 0 && a.rejects == 1,
        "空路径不进入检查账",
    );

    // J8 不安全策略拒
    s.add(
        "J8-不安全策略拒",
        LddAudit::new(Policy::unsafe_exec()).err() == Some(LD_EEXEC)
            && LddAudit::new(Policy::safe()).is_ok(),
        "允许执行的策略不予采纳",
    );

    // J9 可写位判定
    s.add(
        "J9-可写位判定",
        WORLD_WRITABLE.untrusted()
            && WORLD_WRITABLE.writable_by_others()
            && !TRUSTED_BIN.untrusted()
            && !TRUSTED_BIN.writable_by_others(),
        "他人可写即不可信，0755 可信",
    );

    // J10 setuid/setgid 判定
    let setgid = BinaryMeta {
        path: "/usr/bin/sg-probe",
        mode: 0o2755,
        uid: 0,
        dynamic: true,
        setuid: false,
        setgid: true,
    };
    s.add(
        "J10-setuid判定",
        SETUID_BIN.untrusted()
            && SETUID_BIN.setuid
            && setgid.untrusted()
            && setgid.setgid
            && !setgid.writable_by_others(),
        "setuid 与 setgid 位独立触发不可信",
    );

    // J11 组可写判定
    s.add(
        "J11-组可写判定",
        GROUP_WRITABLE.untrusted()
            && GROUP_WRITABLE.writable_by_group()
            && !GROUP_WRITABLE.writable_by_others(),
        "0775 因组可写而不可信",
    );

    // J12 判定与指纹稳定
    let one = decide(&TRUSTED_BIN);
    let two = decide(&TRUSTED_BIN);
    let bad = decide(&WORLD_WRITABLE);
    s.add(
        "J12-判定指纹稳定",
        one == two
            && one.digest(&TRUSTED_BIN) == two.digest(&TRUSTED_BIN)
            && one.digest(&TRUSTED_BIN) != bad.digest(&WORLD_WRITABLE)
            && bad.warning == Warning::Untrusted,
        "同输入判定与指纹一致，可信/不可信指纹相异",
    );

    // J13 重复路径拒
    let mut b = LddAudit::safe();
    let _ = b.inspect(&TRUSTED_BIN);
    s.add(
        "J13-重复路径拒",
        b.inspect(&TRUSTED_BIN) == Err(LD_EDUP) && b.len() == 1 && b.attempts == 1,
        "同路径二次检查拒，账长不变",
    );

    // J14 冻结拒检查
    let seal = base.freeze();
    let before = base.rejects;
    let late = base.inspect(&LIBC_SO);
    s.add(
        "J14-冻结拒检查",
        seal.is_some()
            && late == Err(LD_EFROZEN)
            && base.rejects == before.wrapping_add(1)
            && base.len() == 6,
        "冻结后检查拒，六条记录不变",
    );

    // J15 冻结幂等
    let again = base.freeze();
    s.add(
        "J15-冻结幂等",
        again.is_some() && again == seal && base.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J16 注入四拒
    let hits = inject_four();
    s.add(
        "J16-注入四拒",
        hits.iter().all(|h| *h),
        "空路径/坏策略/缺警告/冻结四路全拒",
    );

    // J17 安全明细
    s.add_dyn(
        "J17-安全明细",
        base.surface_total() == 0 && base.all_untrusted_warned(),
        &format!(
            "rows={} trace={} passive={} refuse={} warn={} exec={} surf={} rej={}",
            base.len(),
            base.traces,
            base.passive,
            base.refusals,
            base.warnings,
            base.executed,
            base.surfaces,
            base.rejects
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11299_trust_flow() {
        let mut audit = build_audit();
        assert_eq!(audit.len(), 6);
        assert_eq!(audit.attempts, 6);
        assert_eq!(audit.traces, 2);
        assert_eq!(audit.passive, 4);
        assert_eq!(audit.refusals, 3);
        assert_eq!(audit.warnings, 4);
        assert_eq!(audit.surface_total(), 0);
        assert!(audit.all_untrusted_warned());
        assert_eq!(audit.unique_paths(), 6);

        assert!(audit.find("/tmp/probe").map(|r| r.verdict.warning) == Some(Warning::Untrusted));
        assert!(audit.find("/usr/bin/probe").map(|r| r.verdict.action) == Some(Action::Trace));
        assert_eq!(decide(&STATIC_BIN).warning, Warning::NotDynamic);
        assert!(!decide(&TRUSTED_BIN).executed);

        let seal = audit.freeze();
        assert!(seal.is_some());
        assert_eq!(audit.freeze(), seal);
        assert_eq!(audit.inspect(&LIBC_SO).err(), Some(LD_EFROZEN));

        assert_eq!(
            LddAudit::new(Policy::unsafe_exec()).err(),
            Some(LD_EEXEC)
        );
        assert!(LddAudit::new(Policy::safe()).is_ok());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11299_checks_green() {
        let s = run_unx11299_checks();
        assert!(s.all_passed(), "UNX-F11299 判据未全绿");
    }
}
