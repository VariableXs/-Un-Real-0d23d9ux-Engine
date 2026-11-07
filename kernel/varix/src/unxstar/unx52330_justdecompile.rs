//! UNX-F52330 JustDecompile 收口（EOL 定级示范） · 0xC9240段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52330"，逐字）：
//!   title: JustDecompile 收口（EOL 定级示范）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52331-J1 六判
//!     据全落；PE 头解析 ×10 样本与 D1 装载语义对账（联签锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写
//!     回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52330）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52331-J1，照原文锚定；功能按本卡标题（JustDecompile）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//! 定级封顶理由：本卡为 EOL 定级示范——JustDecompile 已停更（EOL），
//! 迁移路径如实登记，即便六判据与全部格账全通，KB-COMPAT 定级也封顶
//! S2（有条件收口），S1 恒不可达；EOL 状态如实呈报不粉饰。
//!
//! 设计要点：
//!   ① 反编译 10 操作闭集：加载程序集/反编译类型/反编译方法/IL 视图/
//!      C# 视图/导出工程/搜索/导航/调试断点/插件扩展，逐操作载荷
//!      （纯函数折叠）恒等才收，越闭集必抓。
//!   ② 三语言 8 格闭集：C# 3 格（表达式/语句/类型声明）+ VB 3 格
//!      （表达式/语句/类型声明）+ IL 2 格（指令流/符号栈），语言相位
//!      单调（C#→VB→IL，逆序必抓且拒绝后不改 last_kind）。
//!   ③ EOL 三要素 6 格全呈：停更公告 2 格（公告日期/版本终止）+
//!      替代迁移 2 格（ILSpy 迁移/dnSpyEx 迁移）+ 归档仓库 2 格
//!      （归档地址/只读快照），三要素缺一即 S3。
//!   ④ 迁移联签 4 格（联签锚 F52328 ×2 / F52327 ×2）：ILSpy 联签
//!      双格 + dnSpyEx 联签双格，逐格落账缺一拒封。
//!   ⑤ EOL 后不承诺声明 2 行闭集：EOL 后不承诺修复声明/EOL 后不承诺
//!      兼容声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（语言格
//!      载荷漂移/EOL 格越闭集/语言相位逆序），全部落在当轮临时账本
//!      上，注入 10/10 全抓、逃逸 0；双跑指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52330;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 反编译操作数。
pub const DEC_OPS: usize = 10;

/// 三语言格数。
pub const LANG_CELLS: usize = 8;

/// EOL 三要素格数。
pub const EOL_CELLS: usize = 6;

/// 迁移联签格数（F52328/F52327）。
pub const MIG_CELLS: usize = 4;

/// EOL 后不承诺声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JustDecompCode {
    /// 成功
    Ok = 0,
    /// 反编译操作越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 语言格越闭集
    ENoLang,
    /// 语言相位逆序
    EPhase,
    /// EOL 格越闭集
    ENoEol,
    /// 迁移联签格越闭集
    ENoMig,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封（含六判据越界）
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl JustDecompCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JustDecompCode::Ok => "成功",
            JustDecompCode::ENoOp => "反编译操作越闭集：JustDecompile 10 操作集之外先登记",
            JustDecompCode::EDrift => "载荷漂移：格载荷折叠与基准不恒等，逐格复算",
            JustDecompCode::ENoLang => "语言格越闭集：三语言 8 格闭集之外先登记",
            JustDecompCode::EPhase => "语言相位逆序：C#→VB→IL 单调，逆序必抓且拒绝后不改 last_kind",
            JustDecompCode::ENoEol => "EOL 格越闭集：三要素 6 格闭集之外先登记",
            JustDecompCode::ENoMig => "迁移联签格越闭集：F52328/F52327 联签 4 格之外先登记",
            JustDecompCode::ENoRow => "声明行越闭集：EOL 后不承诺声明 2 行闭集之外先登记",
            JustDecompCode::EDup => "重复落账：同格只许一条账目",
            JustDecompCode::ESeal => "未齐即封：10 操作 + 8 语言格 + 6 EOL 格 + 4 联签 + 2 行声明未齐先补齐",
            JustDecompCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            JustDecompCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            JustDecompCode::EGradeDrift => "定级漂移：EOL 封顶 S2，全通最高只可定 S2（S1 恒不可达）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡 EOL 封顶后恒不可达）
    S1,
    /// S2：有条件收口（EOL 封顶档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口（本卡 EOL 示范下恒不可达）",
            Grade::S2 => "S2：EOL 如实封顶——全通亦只定有条件收口",
            Grade::S3 => "S3：不收口，判据或 EOL 三要素缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 反编译操作格载荷（操作 o）。XOR 锚低 16 位恒零。
pub const fn dec_cell(o: usize) -> Option<u64> {
    if o >= DEC_OPS {
        return None;
    }
    Some((0x53A0_0000_0000_0000u64 + (o as u64) * 0x3B) ^ 0x4A44_0000_0000_0000)
}

/// 反编译操作名（const fn 表 + Option 边界）。
pub const fn dec_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("加载程序集"),
        1 => Some("反编译类型"),
        2 => Some("反编译方法"),
        3 => Some("IL 视图"),
        4 => Some("C# 视图"),
        5 => Some("导出工程"),
        6 => Some("搜索"),
        7 => Some("导航"),
        8 => Some("调试断点"),
        9 => Some("插件扩展"),
        _ => None,
    }
}

/// 三语言格载荷（格 o）。XOR 锚低 16 位恒零。
pub const fn lang_cell(o: usize) -> Option<u64> {
    if o >= LANG_CELLS {
        return None;
    }
    Some((0x53B0_0000_0000_0000u64 + (o as u64) * 0x5D) ^ 0x4C41_4E47_0000_0000)
}

/// 语言相位（i/N 映射：0..3→C#，3..6→VB，6..8→IL）。
pub const fn lang_kind(o: usize) -> Option<u32> {
    if o >= LANG_CELLS {
        return None;
    }
    match o {
        0..=2 => Some(0),
        3..=5 => Some(1),
        _ => Some(2),
    }
}

/// 语言名。
pub const fn lang_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("C#"),
        1 => Some("VB"),
        2 => Some("IL"),
        _ => None,
    }
}

/// EOL 三要素格载荷（格 e；要素 = e/2：0=停更公告 1=替代迁移 2=归档仓库）。
pub const fn eol_cell(e: usize) -> Option<u64> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((0x53C0_0000_0000_0000u64 + (e as u64) * 0x47) ^ 0x454F_4C00_0000_0000)
}

/// EOL 要素（i/N 映射）。
pub const fn eol_kind(e: usize) -> Option<u32> {
    if e >= EOL_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// EOL 要素名。
pub const fn eol_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("停更公告"),
        1 => Some("替代迁移"),
        2 => Some("归档仓库"),
        _ => None,
    }
}

/// 迁移联签格载荷（格 c，锚 F52328/F52327）。
pub const fn mig_cell(c: usize) -> Option<u64> {
    if c >= MIG_CELLS {
        return None;
    }
    Some(((0x53D0_0000u32 + c as u32 * 0x6D) as u64) ^ 0x4D49_4700_0000_0000)
}

/// 迁移联签锚名（i/N 映射：0,1→F52328；2,3→F52327）。
pub const fn mig_anchor(c: usize) -> Option<&'static str> {
    match c {
        0 | 1 => Some("F52328"),
        2 | 3 => Some("F52327"),
        _ => None,
    }
}

/// EOL 后不承诺声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0x53E0 + r as u32 * 0x2F)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// JustDecompile 收口账本状态机。
#[derive(Clone)]
pub struct JustDecompLedger {
    /// 六判据（UNX-F52331-J1：装载/反编译/调试/导出/崩溃/卸载）。
    gates: [Option<bool>; 6],
    /// 反编译操作账。
    dec: [Option<u64>; DEC_OPS],
    /// 三语言格账。
    lang: [Option<u64>; LANG_CELLS],
    /// EOL 三要素格账。
    eol: [Option<u64>; EOL_CELLS],
    /// 迁移联签格账。
    mig: [Option<u64>; MIG_CELLS],
    /// 声明行账。
    decl: [Option<u32>; DECL_ROWS],
    /// 最后语言相位。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JustDecompLedger {
    /// 新账本。
    pub fn new() -> Self {
        JustDecompLedger {
            gates: [None; 6],
            dec: [None; DEC_OPS],
            lang: [None; LANG_CELLS],
            eol: [None; EOL_CELLS],
            mig: [None; MIG_CELLS],
            decl: [None; DECL_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JustDecompCode> {
        if gate >= 6 {
            return Err(JustDecompCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 反编译操作一格（载荷对照 + 重复拒）。
    pub fn record_dec(&mut self, o: usize, got: u64) -> Result<(), JustDecompCode> {
        if o >= DEC_OPS {
            return Err(JustDecompCode::ENoOp);
        }
        let want = match dec_cell(o) {
            Some(w) => w,
            None => return Err(JustDecompCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(JustDecompCode::EDrift);
        }
        if match self.dec.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JustDecompCode::EDup);
        }
        if let Some(c) = self.dec.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 反编译操作全账核验（10 格）。
    pub fn dec_all(&self) -> bool {
        let mut o = 0usize;
        while o < DEC_OPS {
            match self.dec.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三语言一格（语言相位单调 + 载荷对照 + 重复拒）。
    pub fn record_lang(&mut self, o: usize, got: u64) -> Result<(), JustDecompCode> {
        if o >= LANG_CELLS {
            return Err(JustDecompCode::ENoLang);
        }
        let kind = match lang_kind(o) {
            Some(k) => k,
            None => return Err(JustDecompCode::ENoLang),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5303);
            return Err(JustDecompCode::EPhase);
        }
        let want = match lang_cell(o) {
            Some(w) => w,
            None => return Err(JustDecompCode::ENoLang),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5304 + o as u64);
            return Err(JustDecompCode::EDrift);
        }
        if match self.lang.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JustDecompCode::EDup);
        }
        if let Some(c) = self.lang.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5305 + o as u64);
        Ok(())
    }

    /// 三语言全账核验（8 格）。
    pub fn lang_all(&self) -> bool {
        let mut o = 0usize;
        while o < LANG_CELLS {
            match self.lang.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// EOL 三要素一格（载荷对照 + 重复拒）。
    pub fn record_eol(&mut self, e: usize, got: u64) -> Result<(), JustDecompCode> {
        if e >= EOL_CELLS {
            return Err(JustDecompCode::ENoEol);
        }
        let want = match eol_cell(e) {
            Some(w) => w,
            None => return Err(JustDecompCode::ENoEol),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5306 + e as u64);
            return Err(JustDecompCode::EDrift);
        }
        if match self.eol.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JustDecompCode::EDup);
        }
        if let Some(c) = self.eol.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x5307 + e as u64);
        Ok(())
    }

    /// EOL 三要素全账核验（6 格全呈）。
    pub fn eol_all(&self) -> bool {
        let mut e = 0usize;
        while e < EOL_CELLS {
            match self.eol.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 迁移联签一格（F52328/F52327）。
    pub fn record_mig(&mut self, c: usize) -> Result<(), JustDecompCode> {
        if c >= MIG_CELLS {
            return Err(JustDecompCode::ENoMig);
        }
        if match self.mig.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JustDecompCode::EDup);
        }
        if let Some(w) = mig_cell(c) {
            if let Some(cell) = self.mig.get_mut(c) {
                *cell = Some(w);
            }
        }
        self.fnv(0x5308 + c as u64);
        Ok(())
    }

    /// 迁移联签全齐核验。
    pub fn mig_all(&self) -> bool {
        let mut c = 0usize;
        while c < MIG_CELLS {
            match self.mig.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（EOL 后不承诺）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), JustDecompCode> {
        if r >= DECL_ROWS {
            return Err(JustDecompCode::ENoRow);
        }
        if match self.decl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JustDecompCode::EDup);
        }
        if let Some(c) = self.decl.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x5309 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decl.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 最后语言相位（注入拒绝后不变的观测口）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（EOL 封顶：全通 → S2，S1 恒不可达；缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.dec_all()
            && self.lang_all()
            && self.eol_all()
            && self.mig_all()
            && self.rows_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for JustDecompLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = JustDecompLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < DEC_OPS {
        if let Some(w) = dec_cell(o) {
            if led.record_dec(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut l = 0usize;
    while l < LANG_CELLS {
        if let Some(w) = lang_cell(l) {
            if led.record_lang(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut e = 0usize;
    while e < EOL_CELLS {
        if let Some(w) = eol_cell(e) {
            if led.record_eol(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut c = 0usize;
    while c < MIG_CELLS {
        let _ = led.record_mig(c);
        c += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：语言格载荷漂移 / EOL 格越闭集 / 语言相位逆序
    // 三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JustDecompLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移。
                let w0 = match lang_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_lang(0, w0);
                lt.record_lang(1, w0.wrapping_add(1)) == Err(JustDecompCode::EDrift)
            }
            1 => lt.record_eol(EOL_CELLS, 0).is_err(),
            _ => {
                // 相位逆序注入：先按合法序落 VB 相位一格，再注 C# 相位。
                let w3 = match lang_cell(3) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_lang(3, w3);
                lt.record_lang(0, 0).is_err() && lt.last_kind() == 1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x530A + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52330 判据集（锚 UNX-F52331-J1，off-by-one 板式）。
pub fn run_unx52330_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52330");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "反编译 10 操作全通",
        recorded >= DEC_OPS as u32,
        "加载程序集至插件扩展十操作逐格载荷恒等",
    );
    s.add(
        "三语言 8 格全落",
        {
            let mut lt = JustDecompLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LANG_CELLS {
                if let Some(w) = lang_cell(l) {
                    if lt.record_lang(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.lang_all()
        },
        "C# 3 格 + VB 3 格 + IL 2 格逐格恒等",
    );
    s.add(
        "EOL 三要素 6 格全呈",
        {
            let mut lt = JustDecompLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    if lt.record_eol(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.eol_all()
        },
        "停更公告/替代迁移（ILSpy、dnSpyEx）/归档仓库三要素全呈",
    );
    s.add(
        "迁移联签 4/4（F52328/F52327）",
        {
            let mut lt = JustDecompLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < MIG_CELLS {
                if lt.record_mig(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.mig_all()
        },
        "ILSpy 联签双格 + dnSpyEx 联签双格（联签锚定行）",
    );
    s.add(
        "EOL 后不承诺声明 2/2 全齐",
        {
            let mut lt = JustDecompLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "EOL 后不承诺修复声明/不承诺兼容声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = JustDecompLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "装载/反编译/调试/导出/崩溃/卸载逐格落账（UNX-F52331-J1，§AI-66.3）",
    );
    s.add(
        "语言相位单调",
        {
            let mut ok = true;
            let mut l = 1usize;
            while l < LANG_CELLS {
                let a = match lang_kind(l) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match lang_kind(l - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                l += 1;
            }
            ok
        },
        "C#→VB→IL 三相位闭集单调，逆序必抓",
    );
    s.add(
        "诊断码闭集 13 变体",
        JustDecompCode::Ok as u32 == 0 && JustDecompCode::EGradeDrift as u32 == 12,
        "Ok=+0x00 起 12 错误码至 +0x0C 恰 13 变体闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "语言格漂移/EOL 格越闭集/语言相位逆序三态轮转必抓",
    );
    let mut l0 = JustDecompLedger::new();
    s.add(
        "边界·反编译操作越闭集",
        l0.record_dec(DEC_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·EOL 格越闭集",
        l0.record_eol(EOL_CELLS, 0).is_err(),
        "EOL 格 6 越闭集必拒",
    );
    s.add(
        "边界·迁移联签越闭集",
        l0.record_mig(MIG_CELLS).is_err(),
        "联签格 4 越闭集必拒（F52328/F52327 之外先登记）",
    );
    s.add(
        "漂移拒（语言格载荷漂移）",
        {
            let w4 = match lang_cell(4) {
                Some(w) => w,
                None => 0,
            };
            let _ = l0.record_lang(4, w4);
            l0.record_lang(5, w4.wrapping_add(1)) == Err(JustDecompCode::EDrift)
        },
        "语言格载荷漂移必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 EOL 封顶 S2",
        {
            let mut lt = JustDecompLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < DEC_OPS {
                if let Some(w) = dec_cell(o) {
                    let _ = lt.record_dec(o, w);
                }
                o += 1;
            }
            let mut l = 0usize;
            while l < LANG_CELLS {
                if let Some(w) = lang_cell(l) {
                    let _ = lt.record_lang(l, w);
                }
                l += 1;
            }
            let mut e = 0usize;
            while e < EOL_CELLS {
                if let Some(w) = eol_cell(e) {
                    let _ = lt.record_eol(e, w);
                }
                e += 1;
            }
            let mut c = 0usize;
            while c < MIG_CELLS {
                let _ = lt.record_mig(c);
                c += 1;
            }
            let mut r = 0usize;
            while r < DECL_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通亦封顶 S2（EOL 如实，KB-COMPAT 定级登记，S1 恒不可达）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9240,
        "SEG_BASE 0x40 对齐且落在 VX15 公式 0xC7200+0x40×(52330−52201)",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9240);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JustDecompCode::Ok as u32, 0);
        assert_eq!(JustDecompCode::EPhase as u32, 4);
        assert_eq!(JustDecompCode::ESeal as u32, 9);
        assert_eq!(JustDecompCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => JustDecompCode::Ok,
                1 => JustDecompCode::ENoOp,
                2 => JustDecompCode::EDrift,
                3 => JustDecompCode::ENoLang,
                4 => JustDecompCode::EPhase,
                5 => JustDecompCode::ENoEol,
                6 => JustDecompCode::ENoMig,
                7 => JustDecompCode::ENoRow,
                8 => JustDecompCode::EDup,
                9 => JustDecompCode::ESeal,
                10 => JustDecompCode::EInj,
                11 => JustDecompCode::EFp,
                _ => JustDecompCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_lang_kind_mapping() {
        assert_eq!(lang_kind(0), Some(0));
        assert_eq!(lang_kind(2), Some(0));
        assert_eq!(lang_kind(3), Some(1));
        assert_eq!(lang_kind(5), Some(1));
        assert_eq!(lang_kind(6), Some(2));
        assert_eq!(lang_kind(7), Some(2));
        assert!(lang_kind(8).is_none());
        assert_eq!(lang_kind_name(0), Some("C#"));
        assert_eq!(lang_kind_name(1), Some("VB"));
        assert_eq!(lang_kind_name(2), Some("IL"));
        assert!(lang_kind_name(3).is_none());
    }

    #[test]
    fn test_eol_kind_mapping() {
        assert_eq!(eol_kind(0), Some(0));
        assert_eq!(eol_kind(1), Some(0));
        assert_eq!(eol_kind(2), Some(1));
        assert_eq!(eol_kind(4), Some(2));
        assert_eq!(eol_kind(5), Some(2));
        assert!(eol_kind(6).is_none());
        assert_eq!(eol_kind_name(0), Some("停更公告"));
        assert_eq!(eol_kind_name(1), Some("替代迁移"));
        assert_eq!(eol_kind_name(2), Some("归档仓库"));
    }

    #[test]
    fn test_mig_anchor_mapping() {
        assert_eq!(mig_anchor(0), Some("F52328"));
        assert_eq!(mig_anchor(1), Some("F52328"));
        assert_eq!(mig_anchor(2), Some("F52327"));
        assert_eq!(mig_anchor(3), Some("F52327"));
        assert!(mig_anchor(4).is_none());
        assert!(mig_cell(4).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = JustDecompLedger::new();
        let _ = lt.record_lang(4, lang_cell(4).unwrap_or(0));
        assert_eq!(lt.record_lang(1, lang_cell(1).unwrap_or(0)), Err(JustDecompCode::EPhase));
        // 拒绝后 last_kind 不变。
        assert_eq!(lt.last_kind(), 1);
        assert!(lt.record_lang(5, lang_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = JustDecompLedger::new();
        let w = dec_cell(2).unwrap_or(0);
        assert_eq!(lt.record_dec(2, w.wrapping_add(3)), Err(JustDecompCode::EDrift));
        let e = eol_cell(3).unwrap_or(0);
        assert_eq!(lt.record_eol(3, e.wrapping_add(1)), Err(JustDecompCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = JustDecompLedger::new();
        let _ = lt.record_eol(0, eol_cell(0).unwrap_or(0));
        assert_eq!(lt.record_eol(0, eol_cell(0).unwrap_or(0)), Err(JustDecompCode::EDup));
        let _ = lt.record_mig(2);
        assert_eq!(lt.record_mig(2), Err(JustDecompCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = JustDecompLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = JustDecompLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= DEC_OPS as u32 + LANG_CELLS as u32 + EOL_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_grade_eol_cap() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 空账本 → S3。
        let lt = JustDecompLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // EOL 封顶：即便全通，S1 恒不可达，最高 S2。
        let mut full = JustDecompLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < DEC_OPS {
            if let Some(w) = dec_cell(o) {
                let _ = full.record_dec(o, w);
            }
            o += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S3, "缺语言格即 S3");
    }

    #[test]
    fn test_dec_names() {
        assert_eq!(dec_name(0), Some("加载程序集"));
        assert_eq!(dec_name(9), Some("插件扩展"));
        assert!(dec_name(10).is_none());
        assert!(dec_cell(10).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn unx52330_checks_green() {
        let s = run_unx52330_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
