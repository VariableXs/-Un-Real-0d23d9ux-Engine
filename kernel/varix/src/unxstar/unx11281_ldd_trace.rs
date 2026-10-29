//! UNX-F11281 ldd 语义引擎（trace 模式深化）（0x1f510 段）
//!
//! trace 模式只**装载**不**执行**：从目标件的 DT_NEEDED 位掩码出发，按依赖表
//! 逐条解析 SONAME，递归展开传递依赖，遇到已访问者即停（环与菱形去重）。
//! 目录表定容 8 条，每条记名字、路径、装载基址、依赖位与「是否缺失」；
//! 路径为空即渲染成 `名字 => not found`，基础件 linux-vdso 渲染成
//! `名字 (0x基址)`（无 `=>`），其余渲染成 `名字 => 路径 (0x基址)`。
//! 判据函数拒三种非法输入：mode 不是 trace（DIRECT 走别的路径）、
//! 标志位带 F_EXEC（trace 绝不许执行目标件）、根掩码为空或含目录外位。
//! 账本对每条样本现场跑一遍 trace，累计「已找到行数 / 缺失行数」与逐行
//! FNV 格式摘要；replay 重跑 30 条样本，要求两张计数与摘要三者全部复现。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、封册后写账；
//! samples 越界（0 或 65）与根掩码越界都只是样本，进规则产码，不拒收。

use crate::alloc::format;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f510 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f510;

/// 条目 id 越界。
pub const LD_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const LD_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const LD_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const LD_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const LD_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const LD_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算 trace 与登记账不符。
pub const LD_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const LD_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const LD_EDOBS: u32 = SEG_BASE + 0x09;
/// 行数/摘要账不平。
pub const LD_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：mode 不是 trace。
pub const TV_MODE: u32 = 1;
/// 判据码：标志位要求执行目标件。
pub const TV_EXEC: u32 = 2;
/// 判据码：根掩码为空。
pub const TV_ROOT: u32 = 3;
/// 判据码：根掩码含目录外位。
pub const TV_UNKNOWN: u32 = 4;
/// 判据码：样本数越界。
pub const TV_SAMPLES: u32 = 5;

/// 模式：trace（只装载）。
pub const MODE_TRACE: u32 = 1;
/// 模式：--list 直调（另一条路径，本模块不受理）。
pub const MODE_DIRECT: u32 = 2;
/// 目录定容。
pub const LIB_CAP: usize = 8;
/// 目录位掩码（8 条全在域）。
pub const CATALOG_MASK: u32 = 0xff;
/// 依赖展开深度上限。
pub const DEPTH_MAX: u32 = 8;
/// 样本数上限。
pub const SAMPLES_MAX: u32 = 64;
/// 样本条数（判据面固定 30 条）。
pub const SAMPLES: usize = 30;
/// 条目表定容（30 条 + 2 条余量）。
pub const CAP: usize = 32;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

/// 标志位：要求执行目标件（trace 严禁）。
pub const F_EXEC: u32 = 1 << 0;
/// 标志位：强制重新展开（对判据无影响，仅记账）。
pub const F_FORCE: u32 = 1 << 1;
/// 合法标志位并集。
pub const FLAG_MASK: u32 = F_EXEC | F_FORCE;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_81;

/// 目录条目：名字 + 路径 + 基址 + 依赖位 + 特殊件标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lib {
    pub name: &'static str,
    pub path: &'static str,
    pub base: u64,
    pub deps: u32,
    pub special: bool,
}

/// 八个目录条目；路径为空者视为磁盘上缺失。
pub const CATALOG: [Lib; LIB_CAP] = [
    Lib { name: "libc.so.6", path: "/lib/libc.so.6", base: 0x7f00_0000_0000, deps: 1 << 1, special: false },
    Lib { name: "ld-musl-x86_64.so.1", path: "/lib/ld-musl-x86_64.so.1", base: 0x7f00_0001_0000, deps: 0, special: false },
    Lib { name: "libm.so.6", path: "/lib/libm.so.6", base: 0x7f00_0002_0000, deps: 1 << 0, special: false },
    Lib { name: "libssl.so.3", path: "/usr/lib/libssl.so.3", base: 0x7f00_0003_0000, deps: (1 << 0) | (1 << 4), special: false },
    Lib { name: "libcrypto.so.3", path: "/usr/lib/libcrypto.so.3", base: 0x7f00_0004_0000, deps: 1 << 0, special: false },
    Lib { name: "libz.so.1", path: "/usr/lib/libz.so.1", base: 0x7f00_0005_0000, deps: 1 << 0, special: false },
    Lib { name: "libfoo.so.1", path: "", base: 0, deps: 0, special: false },
    Lib { name: "linux-vdso.so.1", path: "linux-vdso.so.1", base: 0x7fff_ff00_0000, deps: 0, special: true },
];

/// trace 结果：逐行输出 + 找到数 + 缺失数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceOut {
    pub lines: Vec<String>,
    pub found: u32,
    pub missing: u32,
}

impl TraceOut {
    pub fn new() -> TraceOut {
        TraceOut { lines: Vec::new(), found: 0, missing: 0 }
    }

    /// 逐行 FNV-1a 摘要（格式账的指纹）。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.lines.len() {
            if let Some(l) = self.lines.get(k) {
                let mut b = 0usize;
                let bytes = l.as_bytes();
                while b < bytes.len() {
                    if let Some(v) = bytes.get(b) {
                        h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
                    }
                    b = b.wrapping_add(1);
                }
                h = (h ^ 0x0a).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 逐条展开：先根后依赖，深度优先，已访问即剪枝。
fn visit(idx: usize, visited: &mut u32, out: &mut TraceOut, depth: u32) {
    if depth > DEPTH_MAX {
        return;
    }
    let bit = 1u32 << (idx as u32 & 31);
    if *visited & bit != 0 {
        return;
    }
    *visited |= bit;
    let lib = match CATALOG.get(idx) {
        Some(l) => *l,
        None => return,
    };
    if lib.path.is_empty() {
        out.missing = out.missing.wrapping_add(1);
        out.lines.push(format!("{} => not found", lib.name));
        return;
    }
    if lib.special {
        out.lines.push(format!("{} (0x{:x})", lib.name, lib.base));
    } else {
        out.lines.push(format!("{} => {} (0x{:x})", lib.name, lib.path, lib.base));
    }
    out.found = out.found.wrapping_add(1);
    let mut b = 0u32;
    while b < LIB_CAP as u32 {
        if lib.deps & (1u32 << b) != 0 {
            visit(b as usize, visited, out, depth.wrapping_add(1));
        }
        b = b.wrapping_add(1);
    }
}

/// 对一个根位掩码跑 trace（不执行任何东西）。
pub fn trace(root: u32) -> TraceOut {
    let mut out = TraceOut::new();
    let mut visited = 0u32;
    let mut b = 0u32;
    while b < LIB_CAP as u32 {
        if root & (1u32 << b) != 0 {
            visit(b as usize, &mut visited, &mut out, 0);
        }
        b = b.wrapping_add(1);
    }
    out
}

/// 一条 trace 样本：根掩码 + 模式 + 样本数 + 标志位 + 指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceCase {
    pub id: u32,
    pub root: u32,
    pub mode: u32,
    pub samples: u32,
    pub flags: u32,
    pub hash: u64,
}

impl TraceCase {
    pub const fn blank() -> TraceCase {
        TraceCase { id: 0, root: 0, mode: 0, samples: 0, flags: 0, hash: 0 }
    }

    pub fn render(&self) -> String {
        format!("T{} root={:02x} mode{} n{} fl{:x} h={:x}",
            self.id, self.root, self.mode, self.samples, self.flags, self.hash)
    }
}

/// 纯判据：模式 → 执行位 → 根空 → 根越界 → 样本数。
pub fn trace_verdict(c: &TraceCase) -> u32 {
    if c.mode != MODE_TRACE {
        return TV_MODE;
    }
    if c.flags & F_EXEC != 0 {
        return TV_EXEC;
    }
    if c.root == 0 {
        return TV_ROOT;
    }
    if c.root & !CATALOG_MASK != 0 {
        return TV_UNKNOWN;
    }
    if c.samples == 0 || c.samples > SAMPLES_MAX {
        return TV_SAMPLES;
    }
    0
}

/// trace 账本：样本 + 判据 + 六槽直方 + 找到/缺失行 + 格式摘要。
#[derive(Clone, Copy, Debug)]
pub struct TraceLedger {
    cases: [TraceCase; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 7],
    found_lines: u32,
    missing_lines: u32,
    format_digest: u64,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl TraceLedger {
    pub const fn new() -> TraceLedger {
        TraceLedger {
            cases: [TraceCase::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 7],
            found_lines: 0,
            missing_lines: 0,
            format_digest: FNV_OFF ^ TAG,
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：封册 → id 域 → 重复 → 顺序 → 定容。
    pub fn register(&mut self, c: TraceCase) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(LD_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(LD_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(LD_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(LD_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(LD_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：判据放行的样本才真跑 trace，累计行数与格式摘要。
    pub fn register_ruled(&mut self, c: TraceCase) -> Result<(usize, u32), u32> {
        let idx = self.register(c)?;
        let v = trace_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if v == 0 {
            let out = trace(c.root);
            self.found_lines = self.found_lines.wrapping_add(out.found);
            self.missing_lines = self.missing_lines.wrapping_add(out.missing);
            self.format_digest = (self.format_digest ^ out.digest()).wrapping_mul(FNV_PRIME);
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(LD_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(LD_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(LD_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn found_lines(&self) -> u32 {
        self.found_lines
    }
    pub fn missing_lines(&self) -> u32 {
        self.missing_lines
    }
    pub fn format_digest(&self) -> u64 {
        self.format_digest
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 6 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn case_at(&self, i: usize) -> Option<TraceCase> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// 重跑全部样本：返回 (找到行, 缺失行, 格式摘要, 跑过的样本数)。
    pub fn retrace(&self) -> (u32, u32, u64, u32) {
        let mut found = 0u32;
        let mut missing = 0u32;
        let mut dig = FNV_OFF ^ TAG;
        let mut ran = 0u32;
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                if trace_verdict(&c) == 0 {
                    let out = trace(c.root);
                    found = found.wrapping_add(out.found);
                    missing = missing.wrapping_add(out.missing);
                    dig = (dig ^ out.digest()).wrapping_mul(FNV_PRIME);
                    ran = ran.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        (found, missing, dig, ran)
    }

    /// FNV-1a：样本全字段 + 判据 + 观测位 + 直方 + 行数与格式摘要。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.found_lines as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.missing_lines as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.format_digest).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.root as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.mode as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.samples as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.flags as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.hash).wrapping_mul(FNV_PRIME);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(b) = self.seen.get(k) {
                h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < 7 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h
    }

    /// 复账：全登记、全观测、全复算，且行数/摘要三值与重跑结果一致。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(LD_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(LD_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(LD_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(LD_EBALANCE);
        }
        let (found, missing, dig, _ran) = self.retrace();
        if found != self.found_lines || missing != self.missing_lines || dig != self.format_digest {
            return Err(LD_EBALANCE);
        }
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(LD_EUNOBS),
            };
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(LD_EMISMATCH),
            };
            if trace_verdict(&c) != v {
                return Err(LD_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(LD_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：复账后上锁，二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(LD_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 构造一条 trace 样本（id 由调用方补）。
pub fn trace_case(root: u32, mode: u32, samples: u32, flags: u32) -> TraceCase {
    TraceCase { id: 1, root, mode, samples, flags, hash: root as u64 ^ 0x8100 }
}

/// 三十条规格样本：根掩码按 3 的倍数铺开，覆盖 8 条目录（含缺失件）。
pub fn spec_of(id: usize) -> (TraceCase, u32) {
    if id < 1 || id > SAMPLES {
        return (TraceCase::blank(), LD_EID);
    }
    let root = (id as u32).wrapping_mul(3) & CATALOG_MASK;
    let flags = if id % 2 == 0 { F_FORCE } else { 0 };
    (trace_case(root, MODE_TRACE, SAMPLES as u32, flags), 0)
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> TraceLedger {
    let mut l = TraceLedger::new();
    let mut i = 1usize;
    while i <= SAMPLES {
        let (mut c, expect) = spec_of(i);
        c.id = i as u32;
        if l.register_ruled(c).is_ok() {
            let _ = l.observe(c.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = TraceLedger::new();
    let mut hi = trace_case(0x01, MODE_TRACE, 30, 0);
    hi.id = 2;
    let _ = a.register(hi);
    let bad_a = a.register(trace_case(0x01, MODE_TRACE, 30, 0)).err() == Some(LD_EORDER);

    let mut b = TraceLedger::new();
    let one = trace_case(0x01, MODE_TRACE, 30, 0);
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(LD_EDUP);

    let mut c = TraceLedger::new();
    let mut wide = trace_case(0x01, MODE_TRACE, 30, 0);
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(LD_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = trace_case(0x01, MODE_TRACE, 30, 0);
    late.id = 40;
    let bad_d = d.register(late).err() == Some(LD_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压 trace 语义与 30 样本格式账，后四项压拒绝面。
pub fn run_unx11281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11281");

    let layout = SEG_BASE == 0x1f510
        && LD_EID == SEG_BASE + 0x01
        && LD_EORDER == SEG_BASE + 0x02
        && LD_EDUP == SEG_BASE + 0x03
        && LD_EFULL == SEG_BASE + 0x04
        && LD_EFROZEN == SEG_BASE + 0x05
        && LD_EUNSEEN == SEG_BASE + 0x06
        && LD_EMISMATCH == SEG_BASE + 0x07
        && LD_EUNOBS == SEG_BASE + 0x08
        && LD_EDOBS == SEG_BASE + 0x09
        && LD_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f510 段十码连续");

    let catalog_ok = CATALOG.len() == LIB_CAP
        && CATALOG_MASK == 0xff
        && CATALOG.get(6).map(|l| l.path.is_empty()).unwrap_or(false)
        && CATALOG.get(7).map(|l| l.special).unwrap_or(false);
    s.add("S2-目录表八条与缺失/特殊件", catalog_ok,
        "第 6 条路径为空（缺库），第 7 条为 vdso 特殊件");

    let o1 = trace(0x01);
    s.add("S3-单根展开顺序与行格式", o1.lines.len() == 2
        && o1.found == 2
        && o1.missing == 0
        && o1.lines.get(0).map(|l| l.as_str()) == Some("libc.so.6 => /lib/libc.so.6 (0x7f0000000000)")
        && o1.lines.get(1).map(|l| l.as_str()) == Some("ld-musl-x86_64.so.1 => /lib/ld-musl-x86_64.so.1 (0x7f0000010000)"),
        "libc 在前、解释器在后，行格式逐字");

    let o2 = trace((1 << 2) | (1 << 3));
    let mut libc_hits = 0u32;
    let mut li = 0usize;
    while li < o2.lines.len() {
        if let Some(l) = o2.lines.get(li) {
            if l.starts_with("libc.so.6 ") {
                libc_hits = libc_hits.wrapping_add(1);
            }
        }
        li = li.wrapping_add(1);
    }
    let has_crypto = o2.lines.get(4).map(|l| l.contains("libcrypto")).unwrap_or(false);
    s.add("S4-传递依赖与菱形去重", o2.lines.len() == 5
        && o2.found == 5
        && libc_hits == 1
        && has_crypto
        && o2.lines.get(0).map(|l| l.contains("libm")).unwrap_or(false),
        "libm→libc→解释器，libssl 再接 libcrypto；libc 只出现一次");

    let miss = trace(1 << 6);
    let vdso = trace(1 << 7);
    s.add("S5-缺库行渲染 not found", miss.lines.len() == 1
        && miss.found == 0
        && miss.missing == 1
        && miss.lines.get(0).map(|l| l.as_str()) == Some("libfoo.so.1 => not found")
        && vdso.lines.get(0).map(|l| l.as_str()) == Some("linux-vdso.so.1 (0x7fffff000000)")
        && vdso.lines.get(0).map(|l| l.contains("=>")).map(|b| !b).unwrap_or(false),
        "缺库一条 not found 行；vdso 首行无 => 直接跟基址");

    let mode_gate = trace_verdict(&trace_case(1, MODE_TRACE, 30, 0)) == 0
        && trace_verdict(&trace_case(1, MODE_DIRECT, 30, 0)) == TV_MODE
        && trace_verdict(&trace_case(1, 0, 30, 0)) == TV_MODE
        && trace_verdict(&trace_case(1, MODE_TRACE, 30, F_EXEC)) == TV_EXEC
        && trace_verdict(&trace_case(1, MODE_DIRECT, 30, F_EXEC)) == TV_MODE;
    s.add("S7-trace 不许执行目标件", mode_gate,
        "DIRECT 与未知模式判 TV_MODE（先于执行位），F_EXEC 判 TV_EXEC");

    let shape_gate = trace_verdict(&trace_case(1, MODE_TRACE, 30, 0)) == 0
        && trace_verdict(&trace_case(0, MODE_TRACE, 30, 0)) == TV_ROOT
        && trace_verdict(&trace_case(0x100, MODE_TRACE, 30, 0)) == TV_UNKNOWN
        && trace_verdict(&trace_case(0x01, MODE_TRACE, 0, 0)) == TV_SAMPLES
        && trace_verdict(&trace_case(0x01, MODE_TRACE, SAMPLES_MAX + 1, 0)) == TV_SAMPLES
        && trace_verdict(&trace_case(0x01, MODE_TRACE, SAMPLES_MAX, 0)) == 0;
    s.add("S8-根掩码与样本数域", shape_gate,
        "空根 TV_ROOT、目录外位 TV_UNKNOWN、0/65 样本 TV_SAMPLES");

    let led = build_ledger();
    let (rf, rm, rd, ran) = led.retrace();
    s.add("S9-三十样本全跑且全绿", led.len() == SAMPLES
        && led.zero_verdicts() == SAMPLES as u32
        && led.found_lines() + led.missing_lines() == rf + rm
        && ran == SAMPLES as u32
        && rf > 0
        && rm > 0,
        "30 条样本全过判据，找到行与缺失行合计与重跑一致");

    s.add("S10-格式账摘要可复现", led.format_digest() == rd
        && rd != FNV_OFF ^ TAG
        && build_ledger().format_digest() == rd,
        "逐行 FNV 摘要与重跑相同，两次构建同值");

    let mut g = TraceLedger::new();
    let mut c2 = trace_case(0x02, MODE_TRACE, 30, 0);
    c2.id = 2;
    let _ = g.register(c2);
    let mut c1 = trace_case(0x01, MODE_TRACE, 30, 0);
    c1.id = 1;
    let bad_a = g.register(c1).err() == Some(LD_EORDER);
    let bad_b = g.register(c2).err() == Some(LD_EDUP);
    let mut wide = trace_case(0x01, MODE_TRACE, 30, 0);
    wide.id = 0;
    let bad_c = g.register(wide).err() == Some(LD_EID);
    let mut full = build_ledger();
    let mut extra = trace_case(0x01, MODE_TRACE, 30, 0);
    extra.id = 31;
    let ok_extra = full.register(extra).is_ok();
    let mut second = trace_case(0x02, MODE_TRACE, 30, 0);
    second.id = 32;
    let ok_second = full.register(second).is_ok();
    let mut over = trace_case(0x03, MODE_TRACE, 30, 0);
    over.id = 33;
    let bad_d = full.register(over).err() == Some(LD_EFULL);
    s.add("S11-顺序重复越界定容四闸", bad_a && bad_b && bad_c && bad_d
        && ok_extra
        && ok_second
        && g.accepts() == 1 && g.rejects() == 3 && g.attempts() == 4
        && inject_four() == [true, true, true, true],
        "四闸独立；第 31 条在容量内，第 33 条 EFULL");

    let mut ob = TraceLedger::new();
    let mut one = trace_case(0x01, MODE_TRACE, 30, 0);
    one.id = 1;
    let _ = ob.register_ruled(one);
    let miss_ob = ob.replay().err() == Some(LD_EUNOBS);
    let unseen = ob.observe(9, 0).err() == Some(LD_EUNSEEN);
    let wrong = ob.observe(1, TV_EXEC) == Ok(false);
    let dob = ob.observe(1, 0).err() == Some(LD_EDOBS);
    s.add("S12-观测面三门", miss_ob && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let root = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = trace_case(0x01, MODE_TRACE, 30, 0);
    late.id = 40;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", root != 0
        && again == Err(LD_EFROZEN)
        && post == Some(LD_EFROZEN)
        && fz.root() == Some(root)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", root, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11281_trace_format() {
        let o = trace(0x01);
        assert_eq!(o.lines.len(), 2);
        assert_eq!(o.found, 2);
        assert_eq!(o.missing, 0);
        assert_eq!(o.lines.get(0).map(|l| l.as_str()),
            Some("libc.so.6 => /lib/libc.so.6 (0x7f0000000000)"));
        let v = trace(1 << 7);
        assert_eq!(v.lines.get(0).map(|l| l.as_str()),
            Some("linux-vdso.so.1 (0x7fffff000000)"));
        let m = trace(1 << 6);
        assert_eq!(m.lines.get(0).map(|l| l.as_str()), Some("libfoo.so.1 => not found"));
    }

    #[test]
    fn unx11281_dedup_and_order() {
        let o = trace((1 << 2) | (1 << 3));
        assert_eq!(o.lines.len(), 5);
        assert!(o.lines.get(0).map(|l| l.contains("libm")).unwrap_or(false));
        assert!(o.lines.get(3).map(|l| l.contains("libssl")).unwrap_or(false));
        assert!(o.lines.get(4).map(|l| l.contains("libcrypto")).unwrap_or(false));
        let a = trace(0x01);
        let b = trace(0x01);
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), trace(0x02).digest());
    }

    #[test]
    fn unx11281_ledger_thirty() {
        let l = build_ledger();
        assert_eq!(l.len(), 30);
        assert_eq!(l.zero_verdicts(), 30);
        let (f, m, d, ran) = l.retrace();
        assert_eq!(ran, 30);
        assert_eq!(f, l.found_lines());
        assert_eq!(m, l.missing_lines());
        assert_eq!(d, l.format_digest());
        assert!(l.replay().is_ok());
    }

    #[test]
    fn unx11281_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = TraceLedger::new();
        let c = trace_case(0x01, MODE_TRACE, 30, 0);
        assert!(l.register(c).is_ok());
        assert_eq!(l.register(c).err(), Some(LD_EDUP));
        assert_eq!(l.attempts(), 2);
        assert_eq!(l.rejects(), 1);
        assert_eq!(trace_verdict(&trace_case(0x01, MODE_TRACE, 30, F_EXEC)), TV_EXEC);
        assert_eq!(trace_verdict(&trace_case(0x01, MODE_DIRECT, 30, 0)), TV_MODE);
    }

    #[test]
    fn unx11281_freeze_and_checks() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.freeze(), Err(LD_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11281_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
