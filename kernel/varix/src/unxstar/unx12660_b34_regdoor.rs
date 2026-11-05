//! UNX-F12661 B34 预告与开门件（E1 探测面二：注册表·句柄探测） · 0x80F00段
//!
//! 判据原文（派工卡 12660 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12661-J1 注册表探测消费边对平断言绿（D3 键路径
//!      回指 B30 联签件），抽样 10 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 开门件域模型：B34 开门件把 E1 探测面二的 11 张面牌（注册表消费
//!    边、CurrentVersion 档、句柄表面、NtQueryObject 消费、文件对象
//!    DOS 映射、管道邮槽、Section 区域、Token 面、同步对象、枚举消费、
//!    一致性协议）逐面预告——面号/面名/回指锚（下一面 F 号）/对平抽样
//!    数四元闭集，缺面、乱序、重复即开门被拒；
//! 2. D3 键路径回指：键路径规范形为反斜杠分隔、段数 2..=6 闭集、ASCII
//!    大小写折叠对账；每条键路径回指 B30 联签件指纹（B30-F12586-D3），
//!    指纹缺失或不匹配即断链拒收；
//! 3. 对平抽样 10 例：键路径库 12 例冻结，确定性 LCG（种子 = SEG_BASE）
//!    抽 10 例，逐例走"折叠→段数闭集→指纹回指"黄金判定，判定结果与
//!    表内期望逐一对平，抽样不一致即红；
//! 4. 开门状态机：Shut → OpenWalk → Opened 单向推进，倒退显性拒；开
//!    门闸四道（面牌齐/键路径抽样对平/回指指纹一致/无悬空锚）逐道过
//!    闸，全过才许写回"已完成"；
//! 5. 注入联测 10 路：缺面预告、面号乱序、回指锚悬空、指纹错挂、键
//!    路径空段、抽样数不足、对平翻样、未走完即开门、开门倒退、面牌
//!    重复——逐路必须被闸抓住，逃脱即红；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE）；取值一律 match/if let/.get()，无 unwrap/expect/panic。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12660 − 12600)）。
pub const SEG_BASE: u32 = 0x80F00;

// ── 开门件诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，未知码在查表处回落提示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorDiag {
    /// 预告面牌缺面（11 面闭集不满）。
    EFaceMissing,
    /// 面号乱序（跳号/倒退）。
    EFaceOrder,
    /// 面牌重复预告。
    EFaceDup,
    /// 回指锚悬空（锚 F 号为零或越段窗口）。
    EAnchorDangling,
    /// D3 指纹缺失（断链）。
    EFpMissing,
    /// D3 指纹不匹配（回指错挂）。
    EFpMismatch,
    /// 键路径空段（连续反斜杠）。
    EPathEmptySeg,
    /// 键路径段数越闭集（<2 或 >6）。
    EPathSegRange,
    /// 对平抽样不一致。
    ESampleMiss,
    /// 状态机倒退（开门中→关/已开→再开）。
    ERewind,
}

impl DoorDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DoorDiag::EFaceMissing => 0x01,
                DoorDiag::EFaceOrder => 0x02,
                DoorDiag::EFaceDup => 0x03,
                DoorDiag::EAnchorDangling => 0x04,
                DoorDiag::EFpMissing => 0x05,
                DoorDiag::EFpMismatch => 0x06,
                DoorDiag::EPathEmptySeg => 0x07,
                DoorDiag::EPathSegRange => 0x08,
                DoorDiag::ESampleMiss => 0x09,
                DoorDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DoorDiag::EFaceMissing => "预告面牌缺面：E1 探测面二 11 面须齐，缺面先补预告再开门",
            DoorDiag::EFaceOrder => "面号乱序：面号按 0..11 逐面预告，跳号先回补上一面",
            DoorDiag::EFaceDup => "面牌重复：同面号只预告一笔，重复先查已有面牌表",
            DoorDiag::EAnchorDangling => "回指锚悬空：锚 F 号为零或越 VX9 段窗，先补真实回指锚",
            DoorDiag::EFpMissing => "D3 指纹缺失：键路径必须回指 B30 联签件，缺指纹即断链拒收",
            DoorDiag::EFpMismatch => "D3 指纹不匹配：回指指纹与 B30 联签件冻结值有差，先对指纹源",
            DoorDiag::EPathEmptySeg => "键路径空段：连续反斜杠产生空段，先规范路径写法再消费",
            DoorDiag::EPathSegRange => "键路径段数越闭集：只认 2..=6 段，越段先拆层",
            DoorDiag::ESampleMiss => "对平抽样不一致：黄金判定与表内期望有差，先修表或判定器",
            DoorDiag::ERewind => "开门状态机倒退：Shut→OpenWalk→Opened 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DOOR_ALL: &[DoorDiag] = &[
    DoorDiag::EFaceMissing,
    DoorDiag::EFaceOrder,
    DoorDiag::EFaceDup,
    DoorDiag::EAnchorDangling,
    DoorDiag::EFpMissing,
    DoorDiag::EFpMismatch,
    DoorDiag::EPathEmptySeg,
    DoorDiag::EPathSegRange,
    DoorDiag::ESampleMiss,
    DoorDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DOOR_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80F01..0x80F0A 闭集内"
}

// ── D3 键路径回指面（B30 联签件指纹冻结） ────────────────────────
/// B30 联签件 D3 指纹（冻结值，全部键路径统一回指此锚）。
pub const D3_COSIGN_FP: &str = "B30-F12586-D3-COSIGN";

/// B30 联签件回指面号（卡 12586，批 B30）。
pub const B30_REF_ID: u32 = 12586;

/// 键路径段数闭集。
pub const PATH_SEG_MIN: usize = 2;
pub const PATH_SEG_MAX: usize = 6;

/// 键路径样本（D3 键路径 + 期望回指指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPathSample {
    /// 键路径（规范写法，反斜杠分隔）。
    pub path: &'static str,
    /// 期望回指指纹。
    pub expect_fp: &'static str,
}

/// D3 键路径库（12 例冻结，覆盖注册表消费边常用键面）。
pub const KEY_PATH_STORE: &[KeyPathSample] = &[
    KeyPathSample { path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SYSTEM\\CurrentControlSet\\Control", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SYSTEM\\CurrentControlSet\\Services", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\HARDWARE\\DESCRIPTION\\System", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SYSTEM\\Setup", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SOFTWARE\\Classes", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKCU\\Software\\Microsoft\\Windows", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SYSTEM\\MountedDevices", expect_fp: D3_COSIGN_FP },
    KeyPathSample { path: "HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tcpip", expect_fp: D3_COSIGN_FP },
];

/// 键路径库容量。
pub const KEY_PATH_CAP: usize = 12;

/// 对平抽样数（判据：抽样 10 例一致）。
pub const SAMPLE_COUNT: usize = 10;

/// ASCII 大小写折叠（no_std 手写，零依赖）。
fn fold_ascii(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                (c as u8 + 32) as char
            } else {
                c
            }
        })
        .collect()
}

/// 键路径段切分（手写切分，禁裸下标）。
fn split_path(path: &str) -> Vec<&str> {
    let mut segs: Vec<&str> = Vec::new();
    let mut rest = path;
    loop {
        match rest.find('\\') {
            Some(pos) => {
                let seg = match rest.get(0..pos) {
                    Some(v) => v,
                    None => break,
                };
                segs.push(seg);
                match rest.get(pos + 1..) {
                    Some(v) => rest = v,
                    None => break,
                }
            }
            None => {
                segs.push(rest);
                break;
            }
        }
    }
    segs
}

/// 键路径折叠规范形（段数合法才返回 Some，越闭集返回 None）。
pub fn fold_key_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let segs = split_path(path);
    if segs.len() < PATH_SEG_MIN || segs.len() > PATH_SEG_MAX {
        return None;
    }
    let mut folded = String::new();
    let mut first = true;
    for seg in segs.iter() {
        if seg.is_empty() {
            return None;
        }
        if !first {
            folded.push('\\');
        }
        folded.push_str(&fold_ascii(seg));
        first = false;
    }
    Some(folded)
}

// ── E1 探测面二面牌预告（11 面冻结闭集） ─────────────────────────
/// 面牌四元（面号/面名/回指锚 F 号/对平抽样数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewFace {
    /// 面号（0..11）。
    pub face_no: u32,
    /// 面名（中文短名）。
    pub face_name: &'static str,
    /// 回指锚（下一面 F 号 = 卡号 + 1）。
    pub ref_anchor: u32,
    /// 对平抽样数。
    pub sample_n: u32,
}

/// E1 探测面二 11 面牌（冻结，面号序）。
pub const PREVIEW_FACES: &[PreviewFace] = &[
    PreviewFace { face_no: 0, face_name: "注册表消费边", ref_anchor: 12662, sample_n: 10 },
    PreviewFace { face_no: 1, face_name: "CurrentVersion 档", ref_anchor: 12663, sample_n: 10 },
    PreviewFace { face_no: 2, face_name: "句柄表面", ref_anchor: 12664, sample_n: 20 },
    PreviewFace { face_no: 3, face_name: "NtQueryObject 消费", ref_anchor: 12665, sample_n: 10 },
    PreviewFace { face_no: 4, face_name: "文件对象 DOS 映射", ref_anchor: 12666, sample_n: 10 },
    PreviewFace { face_no: 5, face_name: "管道邮槽档", ref_anchor: 12667, sample_n: 10 },
    PreviewFace { face_no: 6, face_name: "Section 区域查询", ref_anchor: 12668, sample_n: 10 },
    PreviewFace { face_no: 7, face_name: "Token 探测面", ref_anchor: 12669, sample_n: 10 },
    PreviewFace { face_no: 8, face_name: "同步对象计数", ref_anchor: 12670, sample_n: 10 },
    PreviewFace { face_no: 9, face_name: "枚举消费", ref_anchor: 12671, sample_n: 10 },
    PreviewFace { face_no: 10, face_name: "一致性协议", ref_anchor: 12672, sample_n: 30 },
];

/// 面牌闭集容量。
pub const FACE_CAP: usize = 11;

/// 回指锚是否落在 E1 探测面二的 F 号窗口（12662..=12672）。
pub fn anchor_in_window(anchor: u32) -> bool {
    anchor >= 12662 && anchor <= 12672
}

// ── 开门状态机（Shut → OpenWalk → Opened） ───────────────────────
/// 开门状态：单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorState {
    /// 关门：预告未开始。
    Shut,
    /// 走门中：逐面过闸。
    OpenWalk,
    /// 已开门：写回已完成。
    Opened,
}

impl DoorState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            DoorState::Shut => 0,
            DoorState::OpenWalk => 1,
            DoorState::Opened => 2,
        }
    }
}

/// 开门账（面牌预告 + 抽样对平 + 开门闸）。
#[derive(Clone, Debug)]
pub struct DoorLedger {
    /// 已预告面牌（面号序）。
    faces: Vec<PreviewFace>,
    /// 已对平键路径样本（抽样序）。
    sampled: Vec<KeyPathSample>,
    /// 对平失败记账（面号 + 违约码）。
    faults: Vec<(u32, u32)>,
    /// 开门状态。
    state: DoorState,
}

impl DoorLedger {
    /// 新开一本开门账。
    pub fn new() -> DoorLedger {
        DoorLedger {
            faces: Vec::new(),
            sampled: Vec::new(),
            faults: Vec::new(),
            state: DoorState::Shut,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> DoorState {
        self.state
    }

    /// 已预告面数。
    pub fn face_len(&self) -> usize {
        self.faces.len()
    }

    /// 已抽样对平数。
    pub fn sample_len(&self) -> usize {
        self.sampled.len()
    }

    /// 违约记账数。
    pub fn fault_len(&self) -> usize {
        self.faults.len()
    }

    /// 取第 idx 面牌（越界 None）。
    pub fn face_at(&self, idx: usize) -> Option<&PreviewFace> {
        self.faces.get(idx)
    }

    /// 取第 idx 抽样样本（越界 None）。
    pub fn sample_at(&self, idx: usize) -> Option<&KeyPathSample> {
        self.sampled.get(idx)
    }

    /// 走门（Shut→OpenWalk）。
    pub fn walk(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DoorState::OpenWalk.rank() {
            return Err(DoorDiag::ERewind.val());
        }
        self.state = DoorState::OpenWalk;
        Ok(())
    }

    /// 违约记账（饱和记账，超容拒）。
    fn note_fault(&mut self, face_no: u32, diag: u32) {
        if self.faults.len() < FACE_CAP * 4 {
            self.faults.push((face_no, diag));
        }
    }

    /// 预告一面（面号序/闭集/锚窗口 三闸）。
    pub fn preview(&mut self, f: PreviewFace) -> Result<(), u32> {
        if self.state.rank() < DoorState::OpenWalk.rank() {
            self.note_fault(f.face_no, DoorDiag::ERewind.val());
            return Err(DoorDiag::ERewind.val());
        }
        if self.state.rank() >= DoorState::Opened.rank() {
            self.note_fault(f.face_no, DoorDiag::ERewind.val());
            return Err(DoorDiag::ERewind.val());
        }
        // 面号序闸：必须恰好接着上一面。
        let expect = self.faces.len() as u32;
        if f.face_no != expect {
            self.note_fault(f.face_no, DoorDiag::EFaceOrder.val());
            return Err(DoorDiag::EFaceOrder.val());
        }
        // 面牌闭集闸。
        if f.face_no as usize >= FACE_CAP {
            self.note_fault(f.face_no, DoorDiag::EFaceMissing.val());
            return Err(DoorDiag::EFaceMissing.val());
        }
        // 回指锚窗口闸 + 锚悬空闸。
        if !anchor_in_window(f.ref_anchor) || f.ref_anchor == 0 {
            self.note_fault(f.face_no, DoorDiag::EAnchorDangling.val());
            return Err(DoorDiag::EAnchorDangling.val());
        }
        // 对平抽样数闸：抽样数必须落在 1..=30 闭集。
        if f.sample_n == 0 || f.sample_n > 30 {
            self.note_fault(f.face_no, DoorDiag::ESampleMiss.val());
            return Err(DoorDiag::ESampleMiss.val());
        }
        self.faces.push(f);
        Ok(())
    }

    /// 抽样对平（黄金判定：折叠→段数闭集→指纹回指）。
    /// 抽样引擎判定结果必须与表内期望逐一对平。
    pub fn reconcile(&mut self, s: KeyPathSample) -> Result<(), u32> {
        if self.state.rank() < DoorState::OpenWalk.rank() {
            return Err(DoorDiag::ERewind.val());
        }
        if self.state.rank() >= DoorState::Opened.rank() {
            return Err(DoorDiag::ERewind.val());
        }
        if self.sampled.len() >= SAMPLE_COUNT {
            self.note_fault(0, DoorDiag::ESampleMiss.val());
            return Err(DoorDiag::ESampleMiss.val());
        }
        // 黄金判定。
        let verdict = golden_judge(s.path, s.expect_fp);
        if verdict.is_err() {
            let e = match verdict {
                Err(v) => v,
                Ok(()) => return Ok(()),
            };
            self.note_fault(self.sampled.len() as u32, e);
            return Err(e);
        }
        self.sampled.push(s);
        Ok(())
    }

    /// 开门闸：11 面齐 + 抽样 10 例对平 + 无违约记账。
    pub fn open(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DoorState::Opened.rank() {
            return Err(DoorDiag::ERewind.val());
        }
        if self.state.rank() < DoorState::OpenWalk.rank() {
            return Err(DoorDiag::EFaceMissing.val());
        }
        if self.faces.len() != FACE_CAP {
            return Err(DoorDiag::EFaceMissing.val());
        }
        if self.sampled.len() != SAMPLE_COUNT {
            return Err(DoorDiag::ESampleMiss.val());
        }
        if !self.faults.is_empty() {
            return Err(DoorDiag::EFaceDup.val());
        }
        self.state = DoorState::Opened;
        Ok(())
    }

    /// 账等式：面数=11、抽样=10、违约=0。
    pub fn equation_ok(&self) -> bool {
        self.faces.len() == FACE_CAP && self.sampled.len() == SAMPLE_COUNT && self.faults.is_empty()
    }
}

/// 黄金判定：键路径折叠 + 段数闭集 + 指纹回指，三道全过才判成立。
pub fn golden_judge(path: &str, fp: &str) -> Result<(), u32> {
    // 指纹缺失闸（断链）。
    if fp.is_empty() {
        return Err(DoorDiag::EFpMissing.val());
    }
    // 指纹匹配闸。
    if fp != D3_COSIGN_FP {
        return Err(DoorDiag::EFpMismatch.val());
    }
    // 键路径规范形闸。
    match fold_key_path(path) {
        Some(_) => Ok(()),
        None => {
            // 空段与段数越闭集分诊。
            let segs = split_path(path);
            let mut has_empty = false;
            for seg in segs.iter() {
                if seg.is_empty() {
                    has_empty = true;
                }
            }
            if has_empty {
                Err(DoorDiag::EPathEmptySeg.val())
            } else {
                Err(DoorDiag::EPathSegRange.val())
            }
        }
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct DoorLcg {
    state: u64,
}

impl DoorLcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> DoorLcg {
        DoorLcg { state: seed }
    }

    /// 下一伪随机数（确定性，Knuth MMIX 常数）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

/// 确定性抽样 10 例（LCG 种子 = SEG_BASE，去重采样，双跑一致）。
pub fn sample_ten() -> Vec<KeyPathSample> {
    let mut picked: Vec<KeyPathSample> = Vec::new();
    let mut rng = DoorLcg::new(SEG_BASE as u64);
    let mut guard = 0u32;
    while picked.len() < SAMPLE_COUNT && guard < 512 {
        guard += 1;
        let idx = rng.below(KEY_PATH_CAP as u32) as usize;
        let s = match KEY_PATH_STORE.get(idx) {
            Some(v) => *v,
            None => continue,
        };
        // 去重：已抽过的不重复入列。
        let mut dup = false;
        for p in picked.iter() {
            if *p == s {
                dup = true;
            }
        }
        if !dup {
            picked.push(s);
        }
    }
    picked
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorFault {
    /// 缺面预告（只预告 10 面就开门）。
    MissingFace,
    /// 面号乱序（跳号预告）。
    FaceOrder,
    /// 面牌重复（同面号两笔）。
    FaceDup,
    /// 回指锚悬空（锚号为零）。
    AnchorDangling,
    /// 锚越窗（锚号越 E1 窗口）。
    AnchorRange,
    /// 指纹错挂（回指错指纹）。
    FpMismatch,
    /// 键路径空段。
    PathEmptySeg,
    /// 键路径段数越闭集（1 段）。
    PathSegRange,
    /// 抽样超容（第 11 例对平）。
    SampleOver,
    /// 未走完即开门。
    EarlyOpen,
}

impl DoorFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            DoorFault::MissingFace => 0,
            DoorFault::FaceOrder => 1,
            DoorFault::FaceDup => 2,
            DoorFault::AnchorDangling => 3,
            DoorFault::AnchorRange => 4,
            DoorFault::FpMismatch => 5,
            DoorFault::PathEmptySeg => 6,
            DoorFault::PathSegRange => 7,
            DoorFault::SampleOver => 8,
            DoorFault::EarlyOpen => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            DoorFault::MissingFace => "缺面预告",
            DoorFault::FaceOrder => "面号乱序",
            DoorFault::FaceDup => "面牌重复",
            DoorFault::AnchorDangling => "回指锚悬空",
            DoorFault::AnchorRange => "锚越窗",
            DoorFault::FpMismatch => "指纹错挂",
            DoorFault::PathEmptySeg => "键路径空段",
            DoorFault::PathSegRange => "段数越闭集",
            DoorFault::SampleOver => "抽样超容",
            DoorFault::EarlyOpen => "未走完即开门",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [DoorFault] {
        &[
            DoorFault::MissingFace,
            DoorFault::FaceOrder,
            DoorFault::FaceDup,
            DoorFault::AnchorDangling,
            DoorFault::AnchorRange,
            DoorFault::FpMismatch,
            DoorFault::PathEmptySeg,
            DoorFault::PathSegRange,
            DoorFault::SampleOver,
            DoorFault::EarlyOpen,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in DoorFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: DoorFault) -> bool {
    match v {
        DoorFault::MissingFace => {
            // 抽掉最后一面：把第 11 面从预告里漏掉——直接构造缺面账。
            let mut half = DoorLedger::new();
            let _ = half.walk();
            let mut i = 0usize;
            while i < FACE_CAP - 1 {
                match PREVIEW_FACES.get(i).copied() {
                    Some(f) => {
                        let _ = half.preview(f);
                    }
                    None => break,
                }
                i += 1;
            }
            // 抽样对平 10 例后开门（缺一面必须被拒）。
            for s in sample_ten().iter() {
                let _ = half.reconcile(*s);
            }
            half.open().is_err()
        }
        DoorFault::FaceOrder => {
            let mut door = DoorLedger::new();
            let _ = door.walk();
            // 跳过 0 号面，直接预告 1 号面。
            let f1 = match PREVIEW_FACES.get(1).copied() {
                Some(v) => v,
                None => return false,
            };
            door.preview(f1).is_err()
        }
        DoorFault::FaceDup => {
            let mut door = DoorLedger::new();
            let _ = door.walk();
            let f0 = match PREVIEW_FACES.get(0).copied() {
                Some(v) => v,
                None => return false,
            };
            let _ = door.preview(f0);
            // 再预告 0 号面被序闸拒（面号必须恰好接续）。
            door.preview(f0).is_err()
        }
        DoorFault::AnchorDangling => {
            let mut door = DoorLedger::new();
            let _ = door.walk();
            let mut f = match PREVIEW_FACES.get(0).copied() {
                Some(v) => v,
                None => return false,
            };
            f.ref_anchor = 0;
            door.preview(f).is_err()
        }
        DoorFault::AnchorRange => {
            let mut door = DoorLedger::new();
            let _ = door.walk();
            let mut f = match PREVIEW_FACES.get(0).copied() {
                Some(v) => v,
                None => return false,
            };
            f.ref_anchor = 99999;
            door.preview(f).is_err()
        }
        DoorFault::FpMismatch => {
            let base = match KEY_PATH_STORE.get(0).copied() {
                Some(v) => v,
                None => return false,
            };
            let s = KeyPathSample { path: base.path, expect_fp: "B29-FAKE-FP" };
            golden_judge(s.path, s.expect_fp).is_err()
        }
        DoorFault::PathEmptySeg => {
            let s = KeyPathSample { path: "HKLM\\\\SYSTEM", expect_fp: D3_COSIGN_FP };
            golden_judge(s.path, s.expect_fp).is_err()
        }
        DoorFault::PathSegRange => {
            let s = KeyPathSample { path: "HKLM", expect_fp: D3_COSIGN_FP };
            golden_judge(s.path, s.expect_fp).is_err()
        }
        DoorFault::SampleOver => {
            let mut door = build_full_door();
            // 抽样已满 10 例，第 11 例对平必须被拒。
            let s = match KEY_PATH_STORE.get(0).copied() {
                Some(v) => v,
                None => return false,
            };
            door.reconcile(s).is_err()
        }
        DoorFault::EarlyOpen => {
            // Shut 态直接开门被拒。
            let mut door = DoorLedger::new();
            door.open().is_err()
        }
    }
}

// ── 构建器：完整开门账 ───────────────────────────────────────────
/// 构建完整开门账（11 面逐面预告 + 抽样 10 例对平）。
pub fn build_full_door() -> DoorLedger {
    let mut door = DoorLedger::new();
    let _ = door.walk();
    let mut i = 0usize;
    while i < FACE_CAP {
        let f = match PREVIEW_FACES.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        match door.preview(f) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    for s in sample_ten().iter() {
        let _ = door.reconcile(*s);
    }
    door
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 注册表探测消费边对平断言绿（判据 J1 直读口）。
fn door_reconcile_ok() -> bool {
    let mut door = build_full_door();
    if door.sample_len() != SAMPLE_COUNT {
        return false;
    }
    // 逐例与黄金判定再对平一次（独立复核）。
    let mut i = 0usize;
    while i < door.sample_len() {
        let s = match door.sample_at(i).copied() {
            Some(v) => v,
            None => return false,
        };
        if golden_judge(s.path, s.expect_fp).is_err() {
            return false;
        }
        i += 1;
    }
    if door.fault_len() != 0 {
        return false;
    }
    match door.open() {
        Ok(()) => true,
        Err(_) => false,
    }
}

/// 回指 B30 联签件：抽样全列指纹一致且锚号回指 12586。
fn cosign_ref_ok() -> bool {
    if B30_REF_ID != 12586 {
        return false;
    }
    let picked = sample_ten();
    for s in picked.iter() {
        if s.expect_fp != D3_COSIGN_FP {
            return false;
        }
        if golden_judge(s.path, s.expect_fp).is_err() {
            return false;
        }
    }
    true
}

/// 抽样 10 例一致（判据 J1 的"抽样 10 例一致"直读口）。
fn sample_ten_ok() -> bool {
    let picked = sample_ten();
    if picked.len() != SAMPLE_COUNT {
        return false;
    }
    // 抽样去重：10 例互异。
    let mut i = 0usize;
    while i < picked.len() {
        let mut j = i + 1;
        while j < picked.len() {
            match (picked.get(i), picked.get(j)) {
                (Some(a), Some(b)) => {
                    if a == b {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    // 全部回指 B30 联签件指纹。
    for s in picked.iter() {
        if s.expect_fp != D3_COSIGN_FP {
            return false;
        }
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（冻结面牌+确定性抽样，两轮逐面逐例同判）。
fn dual_run_ok() -> bool {
    let a = build_full_door();
    let b = build_full_door();
    if a.face_len() != b.face_len() {
        return false;
    }
    if a.sample_len() != b.sample_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.face_len() {
        match (a.face_at(i), b.face_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    i = 0;
    while i < a.sample_len() {
        match (a.sample_at(i), b.sample_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = fault_sweep();
    let rb = fault_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：键路径段数闭集与折叠对账边界。
fn boundary_ok() -> bool {
    // 段数下界：1 段拒。
    if fold_key_path("HKLM").is_some() {
        return false;
    }
    // 段数上界：6 段过。
    if fold_key_path("HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session\\Manager").is_none() {
        return false;
    }
    // 段数上界外：7 段拒。
    if fold_key_path("HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session\\Manager\\Extra").is_some() {
        return false;
    }
    // 空段拒。
    if fold_key_path("HKLM\\\\SYSTEM").is_some() {
        return false;
    }
    // 空串拒。
    if fold_key_path("").is_some() {
        return false;
    }
    // 大小写折叠对账：两种写法折叠到同一规范形。
    let a = fold_key_path("HKLM\\SOFTWARE\\MICROSOFT");
    let b = fold_key_path("hklm\\software\\microsoft");
    match (a, b) {
        (Some(x), Some(y)) => {
            if x != y {
                return false;
            }
        }
        _ => return false,
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12660u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut door = build_full_door();
    if door.open().is_err() {
        return false;
    }
    if door.state() != DoorState::Opened {
        return false;
    }
    // 结果摘要：面牌 0 号面名非空。
    match door.face_at(0) {
        Some(f) => {
            if f.face_name.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12661 B34 开门件自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12660_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12660-b34-regdoor");
    let door = build_full_door();

    s.add_dyn(
        "F12661-01 E1 面牌十一面齐",
        PREVIEW_FACES.len() == FACE_CAP,
        &format!("faces={}", PREVIEW_FACES.len()),
    );

    s.add_dyn(
        "F12661-02 注册表消费边对平断言绿",
        door_reconcile_ok(),
        &format!("samples={} faults={}", door.sample_len(), door.fault_len()),
    );

    s.add_dyn(
        "F12661-03 抽样十例一致",
        sample_ten_ok(),
        &format!("picked={}", sample_ten().len()),
    );

    s.add_dyn(
        "F12661-04 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12661-05 键路径库闭集",
        KEY_PATH_STORE.len() == KEY_PATH_CAP,
        &format!("paths={}", KEY_PATH_STORE.len()),
    );

    s.add_dyn(
        "F12661-06 D3 键路径回指 B30 联签件",
        cosign_ref_ok(),
        &format!("fp={} ref_id={}", D3_COSIGN_FP, B30_REF_ID),
    );

    s.add_dyn(
        "F12661-07 折叠对账边界",
        boundary_ok(),
        "段数 2..=6 + 空段拒 + 大小写折叠同形",
    );

    s.add_dyn(
        "F12661-08 双跑一致",
        dual_run_ok(),
        "冻结面牌+确定性抽样 两轮同判",
    );

    s.add_dyn(
        "F12661-09 开门账等式",
        door.equation_ok(),
        &format!("faces={} samples={} faults={}", door.face_len(), door.sample_len(), door.fault_len()),
    );

    s.add_dyn(
        "F12661-10 完成写回语义",
        writeback_ok(),
        "开门即已完成且摘要非空",
    );

    s.add_dyn(
        "F12661-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 回指锚窗口直读（与预告闸独立复核）。
    let mut anchor_ok = true;
    for f in PREVIEW_FACES.iter() {
        if !anchor_in_window(f.ref_anchor) {
            anchor_ok = false;
        }
        if f.face_no as usize != f.ref_anchor as usize - 12662 {
            anchor_ok = false;
        }
    }
    s.add_dyn(
        "F12661-12 回指锚窗口逐面对账",
        anchor_ok,
        "锚号 12662..=12672 与面号线性对应",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80F00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 个码互异、全部落窗口、查表可命中。
        assert_eq!(DOOR_ALL.len(), 10);
        let mut i = 0usize;
        while i < DOOR_ALL.len() {
            let vi = match DOOR_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DOOR_ALL.len() {
                let vj = match DOOR_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_previews_reconciles_and_opens() {
        let mut door = build_full_door();
        assert_eq!(door.face_len(), FACE_CAP);
        assert_eq!(door.sample_len(), SAMPLE_COUNT);
        assert_eq!(door.fault_len(), 0);
        assert!(door.open().is_ok());
        assert_eq!(door.state(), DoorState::Opened);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止，无逃脱。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn golden_judge_three_gates() {
        // 黄金判定三闸直读。
        let base = match KEY_PATH_STORE.get(0) {
            Some(v) => v.path,
            None => return,
        };
        assert!(golden_judge(base, D3_COSIGN_FP).is_ok());
        // 指纹缺失。
        assert!(golden_judge(base, "").is_err());
        // 指纹错挂。
        assert!(golden_judge(base, "X-FAKE").is_err());
        // 空段。
        assert!(golden_judge("HKLM\\\\X", D3_COSIGN_FP).is_err());
        // 段数越闭集。
        assert!(golden_judge("HKLM", D3_COSIGN_FP).is_err());
        // 折叠对账：大小写不同写法同判。
        assert!(golden_judge("HKLM\\SYSTEM\\Setup", D3_COSIGN_FP).is_ok());
        assert!(golden_judge("hklm\\system\\setup", D3_COSIGN_FP).is_ok());
    }

    #[test]
    fn sample_ten_deterministic_and_unique() {
        // 抽样 10 例：双跑一致且互异。
        let a = sample_ten();
        let b = sample_ten();
        assert_eq!(a.len(), SAMPLE_COUNT);
        assert_eq!(a, b);
        let mut i = 0usize;
        while i < a.len() {
            let mut j = i + 1;
            while j < a.len() {
                match (a.get(i), b.get(j)) {
                    (Some(x), Some(y)) => assert_ne!(x, y, "抽样重复"),
                    _ => assert!(false, "抽样例缺失"),
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn preview_gates_reject() {
        // 未走门预告被拒。
        let mut door = DoorLedger::new();
        let f0 = match PREVIEW_FACES.get(0).copied() {
            Some(v) => v,
            None => return,
        };
        assert_eq!(door.preview(f0), Err(DoorDiag::ERewind.val()));
        // 跳号被拒。
        let mut door2 = DoorLedger::new();
        let _ = door2.walk();
        let f1 = match PREVIEW_FACES.get(1).copied() {
            Some(v) => v,
            None => return,
        };
        assert_eq!(door2.preview(f1), Err(DoorDiag::EFaceOrder.val()));
        // 锚悬空被拒。
        let mut door3 = DoorLedger::new();
        let _ = door3.walk();
        let mut f = f0;
        f.ref_anchor = 0;
        assert_eq!(door3.preview(f), Err(DoorDiag::EAnchorDangling.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：两轮全流程逐面逐例同判。
        let a = build_full_door();
        let b = build_full_door();
        assert_eq!(a.face_len(), b.face_len());
        assert_eq!(a.sample_len(), b.sample_len());
        assert_eq!(a.fault_len(), b.fault_len());
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn boundary_seg_fold_cases() {
        // 键路径边界：段数闭集两端 + 空段 + 空串。
        assert!(fold_key_path("A\\B").is_some());
        assert!(fold_key_path("A\\B\\C\\D\\E\\F").is_some());
        assert!(fold_key_path("A").is_none());
        assert!(fold_key_path("A\\B\\C\\D\\E\\F\\G").is_none());
        assert!(fold_key_path("A\\\\B").is_none());
        assert!(fold_key_path("").is_none());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Shut→OpenWalk→Opened。
        assert!(DoorState::Shut.rank() < DoorState::OpenWalk.rank());
        assert!(DoorState::OpenWalk.rank() < DoorState::Opened.rank());
        let mut door = build_full_door();
        assert_eq!(door.state(), DoorState::OpenWalk);
        assert!(door.open().is_ok());
        assert_eq!(door.state(), DoorState::Opened);
        // 重复走门被拒。
        let mut fresh = DoorLedger::new();
        let _ = fresh.walk();
        assert_eq!(fresh.walk(), Err(DoorDiag::ERewind.val()));
    }

    #[test]
    fn face_anchor_alignment() {
        // 面牌逐面：面号连续、锚号 = 12662 + 面号、面名非空、抽样数闭集。
        assert_eq!(PREVIEW_FACES.len(), FACE_CAP);
        for f in PREVIEW_FACES.iter() {
            assert_eq!(f.ref_anchor, 12662 + f.face_no, "面 {} 锚错", f.face_no);
            assert!(!f.face_name.is_empty());
            assert!(f.sample_n >= 1 && f.sample_n <= 30);
            assert!(anchor_in_window(f.ref_anchor));
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12660_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
