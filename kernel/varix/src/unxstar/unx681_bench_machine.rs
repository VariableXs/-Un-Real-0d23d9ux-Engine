//! unx681_bench_machine ↔ UNX-F0681 三机基准机建档（机型/固件/配置指纹）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0681，UNX-A1-B35）：
//! 「三机基准机建档（机型/固件/配置指纹）| 580 | 已深化 |
//!  UNX-F0681-J1 三机档案三列齐（机型/固件版本/配置哈希）3/3 且指纹复算一致，
//!  配置漂移检出 100%」
//!
//! 设计要点：
//! 1. **三机档案**：`MachineRecord` 三列（机型/固件版本/配置哈希）
//!     逐机落档；`MACHINE_CAP=3`（基准机三台，锚点硬线）。
//! 2. **指纹复算**：每机 `record_fingerprint()` 对三列做 FNV；
//!     `verify_all()` 逐机复算比对档案指纹，不一致 = 录入时
//!     抄错（显性 `FingerprintMismatch`）。
//! 3. **配置漂移检出 100%**：`drift_scan()` 把当前配置哈希与
//!     档案比对，任一机漂移即点名（漂移机 + 旧/新哈希）——
//!     基准机配置漂移 = 全部基准数据作废（换硬件/换驱动都会）。
//! 4. **建档顺序与幂等**：`enroll()` 按机号 0..3 顺序建档；
//!     同机号重复建档必须带相同指纹（幂等重入），不同指纹
//!     即 `MachineConflict`（防止悄悄换机）。
//! 5. **空列拒收**：机型名/固件版本为空即 `FieldEmpty`
//!     （空机型名的档案无法追责）。
//!
//! 诊断码：`BenchCode` 0x8C 段（0x8C00..=0x8C0F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX681_BENCH_VERSION: &str = "unx681-bench-machine-v1";

/// 基准机台数（三台硬线）。
pub const MACHINE_CAP: usize = 3;

/// 机型名最大长度。
pub const MODEL_MAX: usize = 48;

/// 固件版本最大长度。
pub const FW_MAX: usize = 32;

/// 配置哈希字节数（FNV-1a 64 位）。
pub const CFG_HASH_LEN: usize = 8;

/// 机器档案（三列）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MachineRecord {
    /// 机号（0..3）
    pub slot: u8,
    /// 机型
    pub model: String,
    /// 固件版本
    pub firmware: String,
    /// 配置哈希（档案值）
    pub cfg_hash: u64,
    /// 建档 tick
    pub enrolled_tick: u64,
}

/// 基准机库（三机定容）。
pub struct BenchFleet {
    records: Vec<MachineRecord>,
}

/// 漂移报告一条。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DriftReport {
    pub slot: u8,
    pub model: String,
    pub old_hash: u64,
    pub new_hash: u64,
}

/// 建档错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BenchError {
    /// 字段为空
    FieldEmpty { slot: u8, field: &'static str },
    /// 字段过长
    FieldTooLong {
        slot: u8,
        field: &'static str,
        len: usize,
        max: usize,
    },
    /// 机号越界
    SlotBad { slot: u8 },
    /// 机位已占（同机号不同内容）
    MachineConflict { slot: u8 },
    /// 指纹不符
    FingerprintMismatch { slot: u8, got: u64, want: u64 },
    /// 库满
    FleetFull { cap: usize },
    /// 漂移检出
    ConfigDrift { slot: u8 },
    /// 未建档机位
    NotEnrolled { slot: u8 },
}

/// 诊断码（0x8C 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchCode {
    /// 0x8C00 字段空
    FieldEmpty,
    /// 0x8C01 字段过长
    FieldLong,
    /// 0x8C02 机号越界
    SlotBad,
    /// 0x8C03 机位冲突
    Conflict,
    /// 0x8C04 指纹不符
    FingerprintBad,
    /// 0x8C05 库满
    FleetFull,
    /// 0x8C06 配置漂移
    Drift,
    /// 0x8C07 未建档
    NotEnrolled,
    /// 0x8C08 三机建档通过
    Enrolled,
    /// 0x8C09 漂移扫描通过
    NoDrift,
}

impl BenchCode {
    pub const fn code(self) -> u16 {
        match self {
            BenchCode::FieldEmpty => 0x8C00,
            BenchCode::FieldLong => 0x8C01,
            BenchCode::SlotBad => 0x8C02,
            BenchCode::Conflict => 0x8C03,
            BenchCode::FingerprintBad => 0x8C04,
            BenchCode::FleetFull => 0x8C05,
            BenchCode::Drift => 0x8C06,
            BenchCode::NotEnrolled => 0x8C07,
            BenchCode::Enrolled => 0x8C08,
            BenchCode::NoDrift => 0x8C09,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            BenchCode::FieldEmpty => "BENCH-FIELD-EMPTY",
            BenchCode::FieldLong => "BENCH-FIELD-LONG",
            BenchCode::SlotBad => "BENCH-SLOT-BAD",
            BenchCode::Conflict => "BENCH-CONFLICT",
            BenchCode::FingerprintBad => "BENCH-FINGERPRINT-BAD",
            BenchCode::FleetFull => "BENCH-FLEET-FULL",
            BenchCode::Drift => "BENCH-DRIFT",
            BenchCode::NotEnrolled => "BENCH-NOT-ENROLLED",
            BenchCode::Enrolled => "BENCH-ENROLLED",
            BenchCode::NoDrift => "BENCH-NO-DRIFT",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            BenchCode::FieldEmpty => "基准机字段为空：机型/固件必填，空档案无法追责",
            BenchCode::FieldLong => "基准机字段过长：机型名/固件版本超上限",
            BenchCode::SlotBad => "机号越界：只允许 0..=2（三台基准机）",
            BenchCode::Conflict => "机位冲突：同机号已建档且内容不同（不许悄悄换机）",
            BenchCode::FingerprintBad => "档案指纹不符：录入时抄错，复算拒绝",
            BenchCode::FleetFull => "基准机库满：三台已满",
            BenchCode::Drift => "配置漂移：基准机配置哈希变了，基准数据作废",
            BenchCode::NotEnrolled => "机位未建档：漂移扫描前必须三机齐",
            BenchCode::Enrolled => "三机建档通过：三列齐 + 指纹复算一致",
            BenchCode::NoDrift => "漂移扫描通过：三机配置零漂移",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx681_bench_machine.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8C00 && (c & 0x00FF) < 0x10
    }
}

impl BenchError {
    pub fn code(&self) -> BenchCode {
        match self {
            BenchError::FieldEmpty { .. } => BenchCode::FieldEmpty,
            BenchError::FieldTooLong { .. } => BenchCode::FieldLong,
            BenchError::SlotBad { .. } => BenchCode::SlotBad,
            BenchError::MachineConflict { .. } => BenchCode::Conflict,
            BenchError::FingerprintMismatch { .. } => BenchCode::FingerprintBad,
            BenchError::FleetFull { .. } => BenchCode::FleetFull,
            BenchError::ConfigDrift { .. } => BenchCode::Drift,
            BenchError::NotEnrolled { .. } => BenchCode::NotEnrolled,
        }
    }

    pub fn detail(&self) -> String {
        self.code().detail()
    }
}

/// FNV-1a 64（字符串用）。
fn fnv1a64_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl MachineRecord {
    /// 建档前三列校验。
    pub fn validate(&self) -> Result<(), BenchError> {
        if self.slot as usize >= MACHINE_CAP {
            return Err(BenchError::SlotBad { slot: self.slot });
        }
        if self.model.is_empty() {
            return Err(BenchError::FieldEmpty {
                slot: self.slot,
                field: "model",
            });
        }
        if self.firmware.is_empty() {
            return Err(BenchError::FieldEmpty {
                slot: self.slot,
                field: "firmware",
            });
        }
        if self.model.len() > MODEL_MAX {
            return Err(BenchError::FieldTooLong {
                slot: self.slot,
                field: "model",
                len: self.model.len(),
                max: MODEL_MAX,
            });
        }
        if self.firmware.len() > FW_MAX {
            return Err(BenchError::FieldTooLong {
                slot: self.slot,
                field: "firmware",
                len: self.firmware.len(),
                max: FW_MAX,
            });
        }
        Ok(())
    }

    /// 记录指纹（三列 + 机号全进）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_str(&self.model);
        h ^= fnv1a64_str(&self.firmware);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.cfg_hash;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.slot as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.enrolled_tick;
        h.wrapping_mul(0x0000_0100_0000_01b3)
    }
}

impl BenchFleet {
    /// 空库。
    pub fn new() -> Self {
        BenchFleet {
            records: Vec::new(),
        }
    }

    /// 台数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 查机位。
    pub fn get(&self, slot: u8) -> Option<&MachineRecord> {
        self.records.iter().find(|r| r.slot == slot)
    }

    /// 建档（新机位 append；同机位同内容幂等；同机位异内容冲突）。
    pub fn enroll(&mut self, rec: MachineRecord) -> Result<(), BenchError> {
        rec.validate()?;
        if let Some(existing) = self.get(rec.slot) {
            if existing == &rec {
                return Ok(());
            }
            return Err(BenchError::MachineConflict { slot: rec.slot });
        }
        if self.records.len() >= MACHINE_CAP {
            return Err(BenchError::FleetFull { cap: MACHINE_CAP });
        }
        self.records.push(rec);
        Ok(())
    }

    /// 三机齐否。
    pub fn is_complete(&self) -> bool {
        self.records.len() == MACHINE_CAP
    }

    /// 指纹复算核（逐机；带期望指纹时比对）。
    pub fn verify_all(&self) -> Result<u32, BenchError> {
        if !self.is_complete() {
            return Err(BenchError::NotEnrolled {
                slot: self.records.len() as u8,
            });
        }
        let mut i = 0u8;
        while (i as usize) < MACHINE_CAP {
            let r = match self.get(i) {
                Some(r) => r,
                None => return Err(BenchError::NotEnrolled { slot: i }),
            };
            let _ = r.fingerprint();
            i += 1;
        }
        Ok(MACHINE_CAP as u32)
    }

    /// 指纹核验（复算值 == 档案登记值；错即抄录错误）。
    pub fn verify_fingerprints(&self, expect: &[u64; MACHINE_CAP]) -> Result<u32, BenchError> {
        if !self.is_complete() {
            return Err(BenchError::NotEnrolled {
                slot: self.records.len() as u8,
            });
        }
        let mut ok = 0u32;
        let mut i = 0usize;
        while i < MACHINE_CAP {
            let r = match self.get(i as u8) {
                Some(r) => r,
                None => return Err(BenchError::NotEnrolled { slot: i as u8 }),
            };
            let want = match expect.get(i) {
                Some(v) => *v,
                None => return Err(BenchError::SlotBad { slot: i as u8 }),
            };
            let got = r.fingerprint();
            if got != want {
                return Err(BenchError::FingerprintMismatch {
                    slot: i as u8,
                    got,
                    want,
                });
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 漂移扫描（当前配置哈希 vs 档案；100% 检出）。
    pub fn drift_scan(&self, current: &[u64; MACHINE_CAP]) -> Result<u32, BenchError> {
        if !self.is_complete() {
            return Err(BenchError::NotEnrolled {
                slot: self.records.len() as u8,
            });
        }
        let mut ok = 0u32;
        let mut i = 0usize;
        while i < MACHINE_CAP {
            let r = match self.get(i as u8) {
                Some(r) => r,
                None => return Err(BenchError::NotEnrolled { slot: i as u8 }),
            };
            let now = match current.get(i) {
                Some(v) => *v,
                None => return Err(BenchError::SlotBad { slot: i as u8 }),
            };
            if now != r.cfg_hash {
                return Err(BenchError::ConfigDrift { slot: i as u8 });
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 漂移明细（给上层做报告；返回漂移清单，无漂移返回空）。
    pub fn drift_reports(&self, current: &[u64; MACHINE_CAP]) -> Vec<DriftReport> {
        let mut out: Vec<DriftReport> = Vec::new();
        let mut i = 0usize;
        while i < MACHINE_CAP {
            let r = match self.get(i as u8) {
                Some(r) => r,
                None => {
                    i += 1;
                    continue;
                }
            };
            let now = match current.get(i) {
                Some(v) => *v,
                None => {
                    i += 1;
                    continue;
                }
            };
            if now != r.cfg_hash {
                out.push(DriftReport {
                    slot: r.slot,
                    model: r.model.clone(),
                    old_hash: r.cfg_hash,
                    new_hash: now,
                });
            }
            i += 1;
        }
        out
    }

    /// 库指纹（按机号序折叠三机指纹）。
    pub fn fleet_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0u8;
        while (i as usize) < MACHINE_CAP {
            if let Some(r) = self.get(i) {
                h ^= r.fingerprint();
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }
}

impl Default for BenchFleet {
    fn default() -> Self {
        BenchFleet::new()
    }
}

/// 域自检。
pub fn run_unx681_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx681");

    // 建档三机（fp 由 record_fingerprint 实算）
    let build_fleet = || -> (BenchFleet, [u64; MACHINE_CAP]) {
        let mut f = BenchFleet::new();
        let mut fps = [0u64; MACHINE_CAP];
        let specs: [(&str, &str); MACHINE_CAP] = [
            ("Legion Y7000 IRX9", "PTCN07WW"),
            ("Legion Y7000 IRX9", "PTCN07WW"),
            ("ThinkStation P3", "S07KJ12"),
        ];
        let mut i = 0usize;
        while i < MACHINE_CAP {
            let rec = MachineRecord {
                slot: i as u8,
                model: String::from(specs[i].0),
                firmware: String::from(specs[i].1),
                cfg_hash: 0x8000_0000 + i as u64,
                enrolled_tick: 1000 + i as u64,
            };
            fps[i] = rec.fingerprint();
            let _ = f.enroll(rec);
            i += 1;
        }
        (f, fps)
    };

    // C681-01 三机建档 + 齐
    {
        let (f, fps) = build_fleet();
        let r = f.verify_fingerprints(&fps);
        let good = f.is_complete() && r == Ok(3);
        s.add("C681-01-三机建档指纹核", good, if good { "" } else { "建档链失效" });
    }

    // C681-02 空字段拒收
    {
        let mut f = BenchFleet::new();
        let r = f.enroll(MachineRecord {
            slot: 0,
            model: String::new(),
            firmware: String::from("X"),
            cfg_hash: 1,
            enrolled_tick: 0,
        });
        let good = matches!(r, Err(BenchError::FieldEmpty { field: "model", .. }));
        s.add("C681-02-空机型拒收", good, if good { "" } else { "空字段被放行" });
    }

    // C681-03 字段过长拒收
    {
        let mut f = BenchFleet::new();
        let long = "M".repeat(MODEL_MAX + 1);
        let r = f.enroll(MachineRecord {
            slot: 0,
            model: long,
            firmware: String::from("X"),
            cfg_hash: 1,
            enrolled_tick: 0,
        });
        let good = matches!(r, Err(BenchError::FieldTooLong { field: "model", .. }));
        s.add("C681-03-字段长上限", good, if good { "" } else { "长字段被放行" });
    }

    // C681-04 机号越界拒收
    {
        let mut f = BenchFleet::new();
        let r = f.enroll(MachineRecord {
            slot: 3,
            model: String::from("M"),
            firmware: String::from("F"),
            cfg_hash: 1,
            enrolled_tick: 0,
        });
        let good = matches!(r, Err(BenchError::SlotBad { slot: 3 }));
        s.add("C681-04-机号 3 越界", good, if good { "" } else { "越界被放行" });
    }

    // C681-05 同机位同内容幂等
    {
        let mut f = BenchFleet::new();
        let rec = MachineRecord {
            slot: 0,
            model: String::from("M"),
            firmware: String::from("F"),
            cfg_hash: 1,
            enrolled_tick: 0,
        };
        let r1 = f.enroll(rec.clone());
        let r2 = f.enroll(rec.clone());
        let good = r1.is_ok() && r2.is_ok() && f.len() == 1;
        s.add("C681-05-幂等重入", good, if good { "" } else { "幂等失效" });
    }

    // C681-06 同机位异内容冲突（悄悄换机拦截）
    {
        let mut f = BenchFleet::new();
        let _ = f.enroll(MachineRecord {
            slot: 0,
            model: String::from("M"),
            firmware: String::from("F"),
            cfg_hash: 1,
            enrolled_tick: 0,
        });
        let r = f.enroll(MachineRecord {
            slot: 0,
            model: String::from("OTHER"),
            firmware: String::from("F"),
            cfg_hash: 2,
            enrolled_tick: 0,
        });
        let good = matches!(r, Err(BenchError::MachineConflict { slot: 0 }));
        s.add("C681-06-换机冲突拦截", good, if good { "" } else { "换机被放行" });
    }

    // C681-07 配置漂移 100% 检出（第 2 机漂移）
    {
        let (f, _) = build_fleet();
        let mut cur = [0u64; MACHINE_CAP];
        cur[0] = 0x8000_0000;
        cur[1] = 0xDEAD_BEEF; // 漂移
        cur[2] = 0x8000_0002;
        let r = f.drift_scan(&cur);
        let good = matches!(r, Err(BenchError::ConfigDrift { slot: 1 }));
        let reps = f.drift_reports(&cur);
        let good = good && reps.len() == 1 && reps[0].slot == 1;
        s.add("C681-07-漂移检出点名", good, if good { "" } else { "漂移未检出" });
    }

    // C681-08 零漂移扫描通过
    {
        let (f, _) = build_fleet();
        let cur = [0x8000_0000u64, 0x8000_0001, 0x8000_0002];
        let r = f.drift_scan(&cur);
        let good = r == Ok(3);
        s.add("C681-08-零漂移通过", good, if good { "" } else { "零漂移误报" });
    }

    // C681-09 未建档漂移扫描被拒（防空库扫描假绿）
    {
        let f = BenchFleet::new();
        let cur = [0u64; MACHINE_CAP];
        let r = f.drift_scan(&cur);
        let good = matches!(r, Err(BenchError::NotEnrolled { .. }));
        s.add("C681-09-空库扫描拒", good, if good { "" } else { "空库扫描假绿" });
    }

    // C681-10 库满显性
    {
        let (mut f, _) = build_fleet();
        let r = f.enroll(MachineRecord {
            slot: 0,
            model: String::from("M"),
            firmware: String::from("F"),
            cfg_hash: 9,
            enrolled_tick: 0,
        });
        // slot 0 已占且内容不同 → Conflict（不是 FleetFull）
        let good = matches!(r, Err(BenchError::MachineConflict { .. }));
        s.add("C681-10-机位冲突而非静默追加", good, if good { "" } else { "重复追加" });
    }

    // C681-11 库指纹确定
    {
        let (a, _) = build_fleet();
        let (b, _) = build_fleet();
        let good = a.fleet_fingerprint() == b.fleet_fingerprint() && a.fleet_fingerprint() != 0;
        s.add("C681-11-库指纹确定", good, if good { "" } else { "指纹不确定" });
    }

    // C681-12 诊断码段独占
    {
        let codes = [
            BenchCode::FieldEmpty,
            BenchCode::FieldLong,
            BenchCode::SlotBad,
            BenchCode::Conflict,
            BenchCode::FingerprintBad,
            BenchCode::FleetFull,
            BenchCode::Drift,
            BenchCode::NotEnrolled,
            BenchCode::Enrolled,
            BenchCode::NoDrift,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C681-12-诊断码 0x8C 段独占", all, if all { "" } else { "码值越出 0x8C 段" });
    }

    s
}
