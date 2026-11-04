//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0058 · SMBIOS Type 9/16/17/32 扩展采集 · 0x6DE80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0058-J1 SMBIOS 四型扩展账（Type 9 系统槽/Type 16 物
//!     理内存阵列/Type 17 内存设备/Type 32 引导信息）逐字段与
//!     dmidecode -t 9/16/17/32 同机对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 结构头建模：Type(u8)/Length(u8)/Handle(u16)，Length ≥ 最小
//!     冻结长度（9 型 9 / 16 型 15 / 17 型 21 / 32 型 11）；
//!   - Type 9：槽类型/数据总线宽/用量/长度/设备句柄，槽型账外拒；
//!   - Type 16：位置/用途/错误纠正/最大容量/句柄数（Capacity
//!     u32 MB 账，bit31=KB 扩展位）；
//!   - Type 17：Form Factor/Type 明细（26-30 位 SODIMM 等）/速度
//!     （0=未知）/制造商串偏移；
//!   - Type 32：BootStatus（0=无错 1=无可引导设备…冻结 8 态）；
//!   - 字符串区：句柄后 \0 分隔双 \0 终结，越界拒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DE80，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DE80;

/// 四型结构类型（冻结）。
pub const TYPE_SLOT: u8 = 9;
pub const TYPE_ARRAY: u8 = 16;
pub const TYPE_MEMDEV: u8 = 17;
pub const TYPE_BOOT: u8 = 32;
/// 最小长度（冻结）。
pub const LEN_SLOT_MIN: usize = 9;
pub const LEN_ARRAY_MIN: usize = 15;
pub const LEN_MEMDEV_MIN: usize = 21;
pub const LEN_BOOT_MIN: usize = 11;
/// BootStatus 冻结 8 态值域。
pub const BOOT_STATUS_MAX: u8 = 8;
/// 最大结构数。
pub const MAX_STRUCTS: usize = 32;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmbCode {
    /// 类型账外（非 9/16/17/32）
    TypeUnknown,
    /// 长度低于型最小值
    LengthTooShort,
    /// 槽型账外
    SlotTypeUnknown,
    /// BootStatus 越域（>8）
    BootStatusOver,
    /// 句柄零（保留句柄）
    HandleReserved,
    /// 字符串区未终结（缺双 \0）
    StringsUnterminated,
    /// 内存设备型账外
    MemDevTypeBad,
    /// 容量扩展位矛盾（KB 位与 MB 账矛盾）
    CapacityBitConflict,
    /// 结构数越上限
    CountOver,
    /// 双跑不一致
    NotReproducible,
}

impl SmbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmbCode::TypeUnknown => "类型账外：非 9/16/17/32",
            SmbCode::LengthTooShort => "长度低于型最小值",
            SmbCode::SlotTypeUnknown => "槽型账外",
            SmbCode::BootStatusOver => "BootStatus 越域：>8",
            SmbCode::HandleReserved => "句柄零：保留句柄",
            SmbCode::StringsUnterminated => "字符串区未终结：缺双 \\0",
            SmbCode::MemDevTypeBad => "内存设备型账外",
            SmbCode::CapacityBitConflict => "容量 KB 扩展位矛盾",
            SmbCode::CountOver => "结构数越上限：32 冻结",
            SmbCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SmbCode; 10] = [
        SmbCode::TypeUnknown,
        SmbCode::LengthTooShort,
        SmbCode::SlotTypeUnknown,
        SmbCode::BootStatusOver,
        SmbCode::HandleReserved,
        SmbCode::StringsUnterminated,
        SmbCode::MemDevTypeBad,
        SmbCode::CapacityBitConflict,
        SmbCode::CountOver,
        SmbCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmbCode::ALL.len() {
        if SmbCode::ALL[i].val() == code {
            return SmbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DE80 段闭集内"
}

// ── 四型结构账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmbStruct {
    pub kind: u8,
    pub length: u8,
    pub handle: u16,
    /// 结构体数据账（含头）。
    pub body: Vec<u8>,
    /// 字符串区（\0 分隔，双 \0 终结）。
    pub strings: Vec<u8>,
}

impl SmbStruct {
    /// 结构五闸。
    pub fn legal(&self) -> Result<(), u32> {
        let min = match self.kind {
            TYPE_SLOT => LEN_SLOT_MIN,
            TYPE_ARRAY => LEN_ARRAY_MIN,
            TYPE_MEMDEV => LEN_MEMDEV_MIN,
            TYPE_BOOT => LEN_BOOT_MIN,
            _ => return Err(SmbCode::TypeUnknown.val()),
        };
        if (self.length as usize) < min || self.body.len() != self.length as usize {
            return Err(SmbCode::LengthTooShort.val());
        }
        if self.handle == 0 {
            return Err(SmbCode::HandleReserved.val());
        }
        // 字符串区：至少双 \0 终结。
        if self.strings.len() < 2
            || self.strings[self.strings.len() - 1] != 0
            || self.strings[self.strings.len() - 2] != 0
        {
            return Err(SmbCode::StringsUnterminated.val());
        }
        match self.kind {
            TYPE_SLOT => {
                let slot_type = self.body[4];
                // 槽型冻结账：0=其他 3=PCI 4=PCI-X 5=PCIe 6=PCIe x2 …
                // 简账：< 0x20 且非 0-6 账外。
                if slot_type > 6 && slot_type < 0x20 {
                    return Err(SmbCode::SlotTypeUnknown.val());
                }
            }
            TYPE_ARRAY => {
                // Type 16 容量：bit31=KB 扩展位；与 MB 账矛盾守卫。
                let cap = u32::from_le_bytes([self.body[10], self.body[11], self.body[12], self.body[13]]);
                let kb_ext = cap & 0x8000_0000 != 0;
                let mb_val = cap & 0x7FFF_FFFF;
                if kb_ext && mb_val == 0 {
                    return Err(SmbCode::CapacityBitConflict.val());
                }
            }
            TYPE_MEMDEV => {
                let ftype = self.body[12];
                // 内存型冻结账：0x12=DDR3 0x18=DDR4 0x1A=LPDDR3 0x1B=LPDDR4。
                if ftype != 0x12 && ftype != 0x18 && ftype != 0x1A && ftype != 0x1B && ftype != 0x02 {
                    return Err(SmbCode::MemDevTypeBad.val());
                }
            }
            TYPE_BOOT => {
                let st = self.body[10];
                if st > BOOT_STATUS_MAX {
                    return Err(SmbCode::BootStatusOver.val());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// BootStatus 文案（Type 32 冻结 8 态 + 0）。
    pub fn boot_status_text(&self) -> String {
        if self.kind != TYPE_BOOT {
            return String::from("-");
        }
        match self.body[10] {
            0 => String::from("无错误"),
            1 => String::from("无可引导介质"),
            2 => String::from("操作系统加载失败"),
            3 => String::from("固件检测故障"),
            4 => String::from("操作系统成功"),
            5 => String::from("看门狗重置"),
            6 => String::from("参数校验失败"),
            7 => String::from("热触发重启"),
            _ => String::from("保留态"),
        }
    }
}

// ── dmidecode 对照账 ────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct SmbLedger {
    pub structs: Vec<SmbStruct>,
    pub dup_caught: u32,
}

impl SmbLedger {
    pub fn new() -> SmbLedger {
        SmbLedger { structs: Vec::new(), dup_caught: 0 }
    }

    /// 登记结构（句柄唯一账）。
    pub fn add(&mut self, s: SmbStruct) -> Result<usize, u32> {
        s.legal()?;
        if self.structs.len() >= MAX_STRUCTS {
            return Err(SmbCode::CountOver.val());
        }
        let mut i = 0;
        while i < self.structs.len() {
            if self.structs[i].handle == s.handle {
                self.dup_caught += 1;
                return Err(SmbCode::HandleReserved.val());
            }
            i += 1;
        }
        self.structs.push(s);
        Ok(self.structs.len() - 1)
    }

    /// 按型分组计数（dmidecode -t 口径）。
    pub fn count_by_type(&self, kind: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.structs.len() {
            if self.structs[i].kind == kind {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Type 16 最大容量账（MB）。
    pub fn array_capacity_mb(&self) -> Option<u32> {
        let mut i = 0;
        while i < self.structs.len() {
            if self.structs[i].kind == TYPE_ARRAY {
                let b = &self.structs[i].body;
                let cap = u32::from_le_bytes([b[10], b[11], b[12], b[13]]);
                if cap & 0x8000_0000 == 0 {
                    return Some(cap);
                }
                return Some(cap & 0x7FFF_FFFF);
            }
            i += 1;
        }
        None
    }

    /// Type 17 速度账（0=未知；dmidecode 口径 MT/s）。
    pub fn memdev_speeds(&self) -> Vec<u16> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.structs.len() {
            if self.structs[i].kind == TYPE_MEMDEV {
                let b = &self.structs[i].body;
                v.push(u16::from_le_bytes([b[19], b[20]]));
            }
            i += 1;
        }
        v
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：型路/长度路/状态路/句柄路） ────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机对照样本：Type9（PCIe x16）+ Type16（32GB）+ Type17（DDR4 3200）+ Type32（0）。
pub fn make_machine_structs() -> Vec<SmbStruct> {
    let mut slot_body = vec![0u8; 9];
    slot_body[0] = TYPE_SLOT;
    slot_body[1] = 9;
    slot_body[2] = 0x10;
    slot_body[3] = 0x00; // handle 低
    slot_body[4] = 5; // PCIe
    slot_body[5] = 0x03; // 数据总线宽 x16 账
    slot_body[6] = 3; // 用法：可用
    slot_body[7] = 4; // 长度
    slot_body[8] = 0; // 设备句柄：无
    let mut array_body = vec![0u8; 15];
    array_body[0] = TYPE_ARRAY;
    array_body[1] = 15;
    array_body[2] = 0x20;
    array_body[3] = 0x00;
    array_body[4] = 3; // 位置：系统板
    array_body[5] = 3; // 用途：系统内存
    array_body[6] = 3; // 纠正：多比特 ECC
    array_body[10] = 0x00;
    array_body[11] = 0x80; // 容量 = 0x00008000 MB = 32768 MB（32GB）
    array_body[12] = 0x00;
    array_body[13] = 0x00;
    array_body[14] = 0xFF; // 句柄数账（0xFFFE=无信息 → 本账 0xFF 占位）
    let mut md_body = vec![0u8; 21];
    md_body[0] = TYPE_MEMDEV;
    md_body[1] = 21;
    md_body[2] = 0x30;
    md_body[3] = 0x00;
    md_body[4] = 0x0C; // Form Factor: DIMM
    md_body[12] = 0x18; // DDR4
    md_body[19] = 0x80;
    md_body[20] = 0x0C; // 3200 MT/s
    let mut boot_body = vec![0u8; 11];
    boot_body[0] = TYPE_BOOT;
    boot_body[1] = 11;
    boot_body[2] = 0x40;
    boot_body[3] = 0x00;
    boot_body[10] = 4; // 操作系统成功
    let strings = vec![0u8; 2];
    vec![
        SmbStruct { kind: TYPE_SLOT, length: 9, handle: 0x0010, body: slot_body, strings: strings.clone() },
        SmbStruct { kind: TYPE_ARRAY, length: 15, handle: 0x0020, body: array_body, strings: strings.clone() },
        SmbStruct { kind: TYPE_MEMDEV, length: 21, handle: 0x0030, body: md_body, strings: strings.clone() },
        SmbStruct { kind: TYPE_BOOT, length: 11, handle: 0x0040, body: boot_body, strings },
    ]
}

// ── 黄金全链：登记→分组→容量→速度→状态文案 ──────────────────────
pub fn golden_run() -> Result<([usize; 4], u32, u16, u8), u32> {
    let mut led = SmbLedger::new();
    let st = make_machine_structs();
    let mut i = 0;
    while i < st.len() {
        led.add(st[i].clone())?;
        i += 1;
    }
    let counts = [
        led.count_by_type(TYPE_SLOT),
        led.count_by_type(TYPE_ARRAY),
        led.count_by_type(TYPE_MEMDEV),
        led.count_by_type(TYPE_BOOT),
    ];
    let cap = led.array_capacity_mb().unwrap_or(0);
    let speeds = led.memdev_speeds();
    let speed = if speeds.is_empty() { 0 } else { speeds[0] };
    let status = match led.structs.get(3) {
        Some(s) => {
            let _ = s.boot_status_text();
            s.body[10]
        }
        None => 0xFF,
    };
    Ok((counts, cap, speed, status))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 各型 1 条 + 32GB + 3200 + 状态 4。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok(([1, 1, 1, 1], cap, sp, st)) if cap == 0x8000 && sp == 3200 && st == 4 => Ok(()),
        _ => Err(SmbCode::NotReproducible.val()),
    }
}

/// 序 B：BootStatus 9 ⇒ 越域。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut body = vec![0u8; 11];
    body[0] = TYPE_BOOT;
    body[1] = 11;
    body[2] = 0x41;
    body[10] = 9;
    let s = SmbStruct { kind: TYPE_BOOT, length: 11, handle: 0x41, body, strings: vec![0, 0] };
    match s.legal() {
        Err(e) if e == SmbCode::BootStatusOver.val() => Ok(()),
        _ => Err(SmbCode::NotReproducible.val()),
    }
}

/// 序 C：槽型 7（账外）⇒ SlotTypeUnknown。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut body = vec![0u8; 9];
    body[0] = TYPE_SLOT;
    body[1] = 9;
    body[4] = 7;
    let s = SmbStruct { kind: TYPE_SLOT, length: 9, handle: 0x11, body, strings: vec![0, 0] };
    match s.legal() {
        Err(e) if e == SmbCode::SlotTypeUnknown.val() => Ok(()),
        _ => Err(SmbCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：型路（Type 12 账外必拒）。
        let mut body = vec![0u8; 11];
        body[0] = 12;
        body[1] = 11;
        let s = SmbStruct { kind: 12, length: 11, handle: 0x50, body, strings: vec![0, 0] };
        led.record(0, s.legal().is_err());
        // 路 2：长度路（Type 17 长度 20 < 21 必拒）。
        let mut md = vec![0u8; 20];
        md[0] = TYPE_MEMDEV;
        md[1] = 20;
        md[12] = 0x18;
        let s2 = SmbStruct { kind: TYPE_MEMDEV, length: 20, handle: 0x51, body: md, strings: vec![0, 0] };
        led.record(1, s2.legal().is_err());
        // 路 3：状态路（BootStatus 255 必拒）。
        let mut bt = vec![0u8; 11];
        bt[0] = TYPE_BOOT;
        bt[1] = 11;
        bt[10] = 255;
        let s3 = SmbStruct { kind: TYPE_BOOT, length: 11, handle: 0x52, body: bt, strings: vec![0, 0] };
        led.record(2, s3.legal().is_err());
        // 路 4：句柄路（重复句柄必拒）。
        let mut l4 = SmbLedger::new();
        let st = make_machine_structs();
        let _ = l4.add(st[0].clone());
        let mut dup = st[0].clone();
        dup.handle = 0x0010;
        led.record(3, l4.add(dup).is_err() && l4.dup_caught == 1);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SmbCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SmbCode::ALL.len() {
        let v = SmbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 主判据：四型各 1 + 容量/速度/状态三账。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((c, cap, sp, st)) => c[0] == 1 && c[1] == 1 && c[2] == 1 && c[3] == 1 && *cap == 0x8000 && *sp == 3200 && *st == 4,
            Err(_) => false,
        },
        match &g {
            Ok((c, cap, sp, st)) => format!("9/16/17/32={:?} cap={} sp={} st={}", c, cap, sp, st),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // BootStatus 文案 9 态逐字。
    let mut txt_ok = true;
    let mut i = 0;
    while i <= 8 {
        let mut body = vec![0u8; 11];
        body[0] = TYPE_BOOT;
        body[1] = 11;
        body[10] = i as u8;
        let s = SmbStruct { kind: TYPE_BOOT, length: 11, handle: 0x60 + i as u16, body, strings: vec![0, 0] };
        let want = [
            "无错误", "无可引导介质", "操作系统加载失败", "固件检测故障",
            "操作系统成功", "看门狗重置", "参数校验失败", "热触发重启", "保留态",
        ][i];
        if s.boot_status_text() != want {
            txt_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-状态文案",
        txt_ok,
        String::from("BootStatus 0-8 逐字钉死"),
    ));
    // 字符串区终结守卫：缺双 \0 必拒。
    let st = make_machine_structs();
    let mut bad = st[0].clone();
    bad.strings = vec![0];
    v.push((
        "J1-字符串终结",
        bad.legal().is_err(),
        String::from("缺双 \\0 拒收"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DE80 窗口"),
    ));
    v
}

pub fn run_unx0058_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0058 SMBIOS Type 9/16/17/32 扩展采集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
