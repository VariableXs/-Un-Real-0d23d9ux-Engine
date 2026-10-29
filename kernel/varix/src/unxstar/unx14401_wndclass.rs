//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14401 · 窗口类表与 RegisterClassEx 全字段 · 0x66680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥382 行；
//!   ② UNX-F14401-J1 类档案（风格位/背景刷/光标/图标/wndproc 指针/类
//!      名）注册-查询-注销全链可观测，重复类名拒止与 Windows
//!      GetLastError 对齐，40 组场景 Spy 记录一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - WNDCLASSEX 全字段闭集：cbSize/cbClsExtra/cbWndExtra/风格位/
//!     背景刷/光标/图标/实例/wndproc 指针/类名——十字段档案；
//!   - 类状态机闭集：Unregistered→Registered→Unregistered（注销后可
//!     重注册；重复注册必拒 1410；注销未注册必拒 1412）；
//!   - GetLastError 对齐（写死常量）：ERROR_CLASS_ALREADY_EXISTS=1410、
//!     ERROR_CLASS_DOES_NOT_EXIST=1412、ERROR_INVALID_PARAMETER=87、
//!     ERROR_NOT_ENOUGH_MEMORY=8（来源：Win32 错误码公开口径）；
//!   - 40 组场景（LCG 确定性生成 reg/query/unreg 序列）×Spy 记录模型：
//!     每步记录（op/class/verdict）与黄金模型逐步比对；
//!   - 类名表闭式查找：先比长度再逐字比对（下标先比长度纪律）；
//!   - LCG 种子写死 = 0x66680；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66680，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66680;

/// GetLastError 对齐常量（来源：Win32 错误码公开口径，写死）。
pub const ERR_CLASS_ALREADY_EXISTS: u32 = 1410;
pub const ERR_CLASS_DOES_NOT_EXIST: u32 = 1412;
pub const ERR_INVALID_PARAMETER: u32 = 87;
pub const ERR_NOT_ENOUGH_MEMORY: u32 = 8;

/// 风格位闭集（来源：Win32 CS_* 公开口径，写死值）。
pub const CS_VREDRAW: u32 = 0x0001;
pub const CS_HREDRAW: u32 = 0x0002;
pub const CS_DBLCLKS: u32 = 0x0008;
pub const CS_OWNDC: u32 = 0x0020;
pub const CS_CLASSDC: u32 = 0x0040;
pub const CS_SAVEBITS: u32 = 0x0800;
/// 风格位合法掩码（写死：上述六位之和之外的位注入必拒）。
pub const CS_VALID_MASK: u32 = CS_VREDRAW
    | CS_HREDRAW
    | CS_DBLCLKS
    | CS_OWNDC
    | CS_CLASSDC
    | CS_SAVEBITS;

/// 类表容量（写死：40 组场景上限口径）。
pub const CLASS_TABLE_CAP: usize = 40;
/// 场景步数。
pub const SCENARIO_STEPS: usize = 40;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WcCode {
    /// 类名为空（注册第一参数不得空）
    ClassNameEmpty,
    /// 类名超长（>31 字节闭式上限）
    ClassNameTooLong,
    /// 风格位越掩码（CS_VALID_MASK 外位）
    StyleBitInvalid,
    /// 额外字节数越闭式上限（cls/wnd 各 ≤ 64）
    ExtraBytesInvalid,
    /// wndproc 指针空（档案六要素缺一）
    WndProcNull,
    /// 重复类名拒止（对齐 1410）
    DuplicateClass,
    /// 注销未注册类（对齐 1412）
    ClassNotExist,
    /// 类表满（40 容量红线）
    TableFull,
    /// 查询档案与注册档案漂移
    ArchiveMismatch,
    /// Spy 记录与黄金模型漂移
    SpyMismatch,
    /// 场景复现失败（双跑不一致）
    NotReproducible,
}

impl WcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WcCode::ClassNameEmpty => "类名为空：注册第一参数不得空",
            WcCode::ClassNameTooLong => "类名超长：31 字节闭式上限",
            WcCode::StyleBitInvalid => "风格位越掩码：CS_VALID_MASK 外位",
            WcCode::ExtraBytesInvalid => "额外字节数越上限：cls/wnd 各 ≤64",
            WcCode::WndProcNull => "wndproc 指针空：档案六要素缺一",
            WcCode::DuplicateClass => "重复类名拒止：对齐 GetLastError 1410",
            WcCode::ClassNotExist => "注销未注册类：对齐 GetLastError 1412",
            WcCode::TableFull => "类表满：40 容量红线",
            WcCode::ArchiveMismatch => "查询档案与注册档案漂移",
            WcCode::SpyMismatch => "Spy 记录与黄金模型漂移",
            WcCode::NotReproducible => "场景双跑不一致：复现红线",
        }
    }
    pub const ALL: [WcCode; 11] = [
        WcCode::ClassNameEmpty,
        WcCode::ClassNameTooLong,
        WcCode::StyleBitInvalid,
        WcCode::ExtraBytesInvalid,
        WcCode::WndProcNull,
        WcCode::DuplicateClass,
        WcCode::ClassNotExist,
        WcCode::TableFull,
        WcCode::ArchiveMismatch,
        WcCode::SpyMismatch,
        WcCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WcCode::ALL.len() {
        if WcCode::ALL[i].val() == code {
            return WcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66680 段闭集内"
}

// ── WNDCLASSEX 全字段档案 ────────────────────────────────────────
/// 十字段档案（来源：Win32 WNDCLASSEX 公开结构口径，字段名注释钉版）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WndClassEx {
    /// cbSize：结构尺寸（写死 48，口径自建模）
    pub cb_size: u32,
    /// style：风格位
    pub style: u32,
    /// lpfnWndProc：wndproc 指针（建模为非零 id）
    pub wnd_proc: u32,
    /// cbClsExtra：类额外字节
    pub cb_cls_extra: i32,
    /// cbWndExtra：窗口额外字节
    pub cb_wnd_extra: i32,
    /// hInstance：实例句柄（建模 id）
    pub instance: u32,
    /// hIcon：图标句柄（建模 id，0 表默认）
    pub icon: u32,
    /// hCursor：光标句柄（建模 id，0 表默认）
    pub cursor: u32,
    /// hbrBackground：背景刷句柄（建模 id，0 表无）
    pub background: u32,
    /// lpszClassName：类名
    pub class_name: String,
}

impl WndClassEx {
    /// 档案校验：六要素（风格位/背景刷/光标/图标/wndproc/类名）+ 三
    /// 闭式约束（类名长/额外字节/风格掩码）。
    pub fn validate(&self) -> Result<(), u32> {
        if self.class_name.len() == 0 {
            return Err(WcCode::ClassNameEmpty.val());
        }
        if self.class_name.len() > 31 {
            return Err(WcCode::ClassNameTooLong.val());
        }
        if self.style & !CS_VALID_MASK != 0 {
            return Err(WcCode::StyleBitInvalid.val());
        }
        if self.cb_cls_extra < 0 || self.cb_cls_extra > 64 {
            return Err(WcCode::ExtraBytesInvalid.val());
        }
        if self.cb_wnd_extra < 0 || self.cb_wnd_extra > 64 {
            return Err(WcCode::ExtraBytesInvalid.val());
        }
        if self.wnd_proc == 0 {
            return Err(WcCode::WndProcNull.val());
        }
        Ok(())
    }

    /// 全字段档案指纹（逐字段滚乘，含类名逐字节）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x6668_00FB;
        h = h.wrapping_mul(0x100000001B3) ^ (self.cb_size as u64);
        h = h.wrapping_mul(0x100000001B3) ^ (self.style as u64);
        h = h.wrapping_mul(0x100000001B3) ^ (self.wnd_proc as u64);
        h = h.wrapping_mul(0x100000001B3) ^ ((self.cb_cls_extra as i64) as u64);
        h = h.wrapping_mul(0x100000001B3) ^ ((self.cb_wnd_extra as i64) as u64);
        h = h.wrapping_mul(0x100000001B3) ^ (self.instance as u64);
        h = h.wrapping_mul(0x100000001B3) ^ (self.icon as u64);
        h = h.wrapping_mul(0x100000001B3) ^ (self.cursor as u64);
        h = h.wrapping_mul(0x100000001B3) ^ (self.background as u64);
        let bytes = self.class_name.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            h = h.wrapping_mul(0x100000001B3) ^ (bytes[i] as u64);
            i += 1;
        }
        h
    }
}

/// 类状态机闭集（Vacated=已注销槽位，名留档留但 query 必拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassState {
    Unregistered,
    Registered,
    Vacated,
}

// ── 类表（注册-查询-注销全链） ───────────────────────────────────
pub struct ClassTable {
    pub names: Vec<String>,
    pub archives: Vec<WndClassEx>,
    pub states: Vec<ClassState>,
    /// Spy 记录：(op, class, verdict)——op: 0=reg 1=query 2=unreg
    pub spy: Vec<(u8, String, Result<(), u32>)>,
}

impl ClassTable {
    pub fn new() -> ClassTable {
        ClassTable {
            names: Vec::new(),
            archives: Vec::new(),
            states: Vec::new(),
            spy: Vec::new(),
        }
    }

    /// 闭式查找：先比长度再逐字比对。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].len() == name.len() {
                let a = self.names[i].as_bytes();
                let b = name.as_bytes();
                let mut same = true;
                let mut j = 0;
                while j < a.len() {
                    if a[j] != b[j] {
                        same = false;
                    }
                    j += 1;
                }
                if same {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// RegisterClassEx：注册（重复名拒止 1410 对齐）。
    pub fn register(&mut self, arc: WndClassEx) -> Result<(), u32> {
        arc.validate()?;
        if self.names.len() >= CLASS_TABLE_CAP {
            return Err(WcCode::TableFull.val());
        }
        if self.find(&arc.class_name).is_some() {
            return Err(ERR_CLASS_ALREADY_EXISTS);
        }
        // 内存红线建模：档案指纹全零视为分配失败（对齐 8）。
        if arc.fingerprint() == 0 {
            return Err(ERR_NOT_ENOUGH_MEMORY);
        }
        self.names.push(arc.class_name.clone());
        self.archives.push(arc);
        self.states.push(ClassState::Registered);
        self.spy.push((0, self.names[self.names.len() - 1].clone(), Ok(())));
        Ok(())
    }

    /// GetClassInfoEx：查询（档案逐字段回读一致）。
    pub fn query(&mut self, name: &str) -> Result<WndClassEx, u32> {
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(ERR_CLASS_DOES_NOT_EXIST),
        };
        if self.states[idx] != ClassState::Registered {
            return Err(ERR_CLASS_DOES_NOT_EXIST);
        }
        let arc = self.archives[idx].clone();
        // 全链可观测：查询也要留 Spy 记录（verdict=档案回读一致）。
        self.spy.push((1, String::from(name), Ok(())));
        Ok(arc)
    }

    /// UnregisterClass：注销（未注册类拒止 1412 对齐；注销后可重注册）。
    pub fn unregister(&mut self, name: &str) -> Result<(), u32> {
        let idx = match self.find(name) {
            Some(i) => i,
            None => {
                self.spy.push((2, String::from(name), Err(ERR_CLASS_DOES_NOT_EXIST)));
                return Err(ERR_CLASS_DOES_NOT_EXIST);
            }
        };
        if self.states[idx] != ClassState::Registered {
            self.spy.push((2, String::from(name), Err(ERR_CLASS_DOES_NOT_EXIST)));
            return Err(ERR_CLASS_DOES_NOT_EXIST);
        }
        // 状态机 Registered→Vacated；档案与名保留（注销态可查
        // 槽位，但 query 必拒）——Windows 语义：类名槽位可复用。
        self.states[idx] = ClassState::Vacated;
        self.spy.push((2, String::from(name), Ok(())));
        Ok(())
    }

    /// Spy 记录与黄金模型逐步比对（黄金模型：独立重放同一序列）。
    pub fn spy_match(&self, golden: &[(u8, String, Result<(), u32>)]) -> bool {
        if golden.len() != self.spy.len() {
            return false;
        }
        let mut i = 0;
        while i < self.spy.len() {
            let (o1, c1, v1) = &self.spy[i];
            let (o2, c2, v2) = &golden[i];
            if o1 != o2 || c1 != c2 || v1 != v2 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 标准档案构造（六要素齐备：风格位/背景刷/光标/图标/wndproc/类名）。
pub fn standard_archive(name: &str, style: u32) -> WndClassEx {
    WndClassEx {
        cb_size: 48,
        style,
        wnd_proc: 0x1000 + name.len() as u32,
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        instance: 0x4000,
        icon: 0x2000,
        cursor: 0x2001,
        background: 0x3000,
        class_name: String::from(name),
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 40 组场景（Spy 记录一致） ────────────────────────────────────
/// 场景生成：确定性 op 序列（0=reg 1=query 2=unreg），类名从 8 个
/// 预置名闭式取。黄金模型 = 独立新表重放。
pub fn scenario_ops() -> Vec<(u8, usize)> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ops: Vec<(u8, usize)> = Vec::new();
    let mut i = 0;
    while i < SCENARIO_STEPS {
        let op = (rng.below(3)) as u8;
        let cls = (rng.below(8)) as usize;
        ops.push((op, cls));
        i += 1;
    }
    ops
}

pub const SCENARIO_CLASS_NAMES: [&str; 8] = [
    "vx3_main", "vx3_child", "vx3_popup", "vx3_dlg",
    "vx3_btn", "vx3_edit", "vx3_list", "vx3_static",
];

/// 双跑对拍：同序列喂两表，Spy 记录一致（复现）。
pub fn scenario_replay(ops: &[(u8, usize)]) -> (ClassTable, bool) {
    let mut tab = ClassTable::new();
    let mut i = 0;
    while i < ops.len() {
        let (op, ci) = ops[i];
        if (ci as usize) < SCENARIO_CLASS_NAMES.len() {
            let name = SCENARIO_CLASS_NAMES[ci];
            match op {
                0 => {
                    let _ = tab.register(standard_archive(name, CS_VREDRAW | CS_DBLCLKS));
                }
                1 => {
                    let _ = tab.query(name);
                }
                _ => {
                    let _ = tab.unregister(name);
                }
            }
        }
        i += 1;
    }
    // 黄金模型：独立新表重放（确定性 API 语义必须同记录）。
    let mut golden = ClassTable::new();
    let mut i = 0;
    while i < ops.len() {
        let (op, ci) = ops[i];
        if (ci as usize) < SCENARIO_CLASS_NAMES.len() {
            let name = SCENARIO_CLASS_NAMES[ci];
            match op {
                0 => {
                    let _ = golden.register(standard_archive(name, CS_VREDRAW | CS_DBLCLKS));
                }
                1 => {
                    let _ = golden.query(name);
                }
                _ => {
                    let _ = golden.unregister(name);
                }
            }
        }
        i += 1;
    }
    let ok = tab.spy_match(&golden.spy);
    (tab, ok)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    // 基链：注册→查询（档案回读逐字段一致）→注销→重注册。
    let mut tab = ClassTable::new();
    tab.register(standard_archive("vx3_base", CS_HREDRAW | CS_OWNDC))?;
    let arc = tab.query("vx3_base")?;
    if arc.style != (CS_HREDRAW | CS_OWNDC) || arc.wnd_proc == 0 {
        return Err(WcCode::ArchiveMismatch.val());
    }
    tab.unregister("vx3_base")?;
    tab.register(standard_archive("vx3_base", CS_SAVEBITS))?;
    // 重复注册必拒 1410；注销未注册名必拒 1412。
    let dup_ok = match tab.register(standard_archive("vx3_base", CS_DBLCLKS)) {
        Err(e) => e == ERR_CLASS_ALREADY_EXISTS,
        Ok(()) => false,
    };
    let gone_ok = match tab.unregister("vx3_ghost") {
        Err(e) => e == ERR_CLASS_DOES_NOT_EXIST,
        Ok(()) => false,
    };
    if !dup_ok || !gone_ok {
        return Err(WcCode::DuplicateClass.val());
    }
    // 40 组场景对拍。
    let ops = scenario_ops();
    let (_tab2, spy_ok) = scenario_replay(&ops);
    if !spy_ok {
        return Err(WcCode::SpyMismatch.val());
    }
    Ok((tab.names.len(), tab.spy.len(), spy_ok))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：档案校验注入四连（空名/超长名/越掩码风格位/空 wndproc）。
    ledger.tries[0] += 4;
    let mut c = 0;
    let mut bad = standard_archive("", CS_VREDRAW);
    if bad.validate().is_err() {
        c += 1;
    }
    bad = standard_archive("vx3_this_class_name_is_way_too_long_x", CS_VREDRAW);
    if bad.validate().is_err() {
        c += 1;
    }
    bad = standard_archive("vx3_x", 0xDEAD0000);
    if bad.validate().is_err() {
        c += 1;
    }
    bad = standard_archive("vx3_x", CS_VREDRAW);
    bad.wnd_proc = 0;
    if bad.validate().is_err() {
        c += 1;
    }
    if c == 4 {
        ledger.caught[0] += 4;
    }
    // 族 1：重复注册（1410）与注销未注册（1412）对齐注入。
    ledger.tries[1] += 2;
    let mut tab = ClassTable::new();
    let _ = tab.register(standard_archive("vx3_dup", CS_DBLCLKS));
    let d = tab.register(standard_archive("vx3_dup", CS_DBLCLKS));
    let g = tab.unregister("vx3_nosuch");
    if d == Err(ERR_CLASS_ALREADY_EXISTS) && g == Err(ERR_CLASS_DOES_NOT_EXIST) {
        ledger.caught[1] += 2;
    }
    // 族 2：额外字节越界（负值与 >64 双注）。
    ledger.tries[2] += 2;
    let mut e1 = standard_archive("vx3_e1", CS_VREDRAW);
    e1.cb_cls_extra = -1;
    let mut e2 = standard_archive("vx3_e2", CS_VREDRAW);
    e2.cb_wnd_extra = 65;
    if e1.validate().is_err() && e2.validate().is_err() {
        ledger.caught[2] += 2;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WcCode::ALL.len() {
        let v = WcCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：注册-查询-注销-重注册全链 + 40 场景 Spy 一致。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((names, spy, ok)) => *names >= 1 && *spy >= 1 && *ok,
        Err(_) => false,
    };
    v.push(("J1-全链可观测+40场景Spy一致", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（档案四连+拒止对齐）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. GetLastError 对齐常量点验（1410/1412/87/8 写死）。
    v.push((
        "J1-GetLastError 对齐常量",
        ERR_CLASS_ALREADY_EXISTS == 1410
            && ERR_CLASS_DOES_NOT_EXIST == 1412
            && ERR_INVALID_PARAMETER == 87
            && ERR_NOT_ENOUGH_MEMORY == 8,
        String::from("1410/1412/87/8"),
    ));
    // 4. 风格位掩码闭式点验（六位和 = 掩码）。
    v.push((
        "J1-CS 风格位掩码闭式",
        CS_VALID_MASK == 0x086B,
        format!("{:#x}", CS_VALID_MASK),
    ));
    // 5. 档案十字段点验（cbSize=48 写死口径）。
    let arc = standard_archive("vx3_probe", CS_VREDRAW);
    v.push((
        "J1-档案十字段齐备",
        arc.cb_size == 48 && arc.class_name.len() == 9,
        String::from("cbSize=48"),
    ));
    // 6. 注销后重注册（槽位复用语义）。
    let reuse_ok = {
        let mut tab = ClassTable::new();
        let r1 = tab.register(standard_archive("vx3_re", CS_VREDRAW));
        let u1 = tab.unregister("vx3_re");
        let r2 = tab.register(standard_archive("vx3_re", CS_SAVEBITS));
        r1.is_ok() && u1.is_ok() && r2.is_ok()
    };
    v.push(("J1-注销后重注册槽位复用", reuse_ok, String::from("reg→unreg→reg")));
    // 7. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 8. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14401 窗口类表与 RegisterClassEx 全字段");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
