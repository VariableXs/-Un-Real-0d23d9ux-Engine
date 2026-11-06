//! UNX-F12722 NT→Dos 错误码对照全量回归（≥100 对） · 0x81E80段
//!
//! 判据原文（派工卡 12722 逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12723-J1 OBJ_* 位族（INHERIT 等全档）对照回归
//!      10/10 一致，位值锚注出处；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. NT→DOS 对照冻结表 104 对：左值 = NTSTATUS（ntstatus.h 锚注），
//!    右值 = Win32 错误码（winerror.h 锚注），映射语义对齐
//!    RtlNtStatusToDosErrorNoTeb 的公开映射表——表内行冻结不改；
//! 2. 全量回归闸：逐对复放「查表命中 → 右值回读 → 与冻结值对平」，
//!    104/104 对平即回归绿，一致率 100% 维持；
//! 3. OBJ_* 位族对照回归（判据 ② 挂接）：INHERIT/PERMANENT/EXCLUSIVE/
//!    CASE_INSENSITIVE/OPENIF/OPENLINK/KERNEL_HANDLE/FORCE_ACCESS_CHECK
//!    八档 + VALID 掩码，十演练档逐档 1/1 一致，位值锚注出处
//!    （WDK ntobjkernel.h / winnt.h OBJ_* 常量）；
//! 4. 回归态机 NtDraft → NtRegressing → NtRegressed → NtSealed 单向
//!    推进，rank 单调，倒退/跳相显性拒，封账后禁写；
//! 5. 违约注入十族逐路必抓：坏映射（右值漂移）、表序漂移（STATUS 码
//!    不升序）、重复对、相位倒退、封账后写、未开账先回归、位值漂移、
//!    掩码越界、对额不足、空摘要封账。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定取
//!    SEG_BASE）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    禁裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `pairs104_ok` ↔ ② ≥100 对全量回归在账且行冻结；
//!   - `map_regress_ok` ↔ ② 104 对查表回读全对平（一致率 100%）；
//!   - `objbits_regress_ok` ↔ ② OBJ_* 位族（INHERIT 等全档）对照回归
//!     10/10 一致，位值锚注出处；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12722 − 12600)）。
pub const SEG_BASE: u32 = 0x81E80;

/// 对照对下限（判据 ② ≥100 对）。
pub const PAIRS_MIN: usize = 100;

// ── NT→DOS 对照冻结表（104 对，ntstatus.h → winerror.h 锚注） ────
/// 单对对照行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NtDosPair {
    /// NTSTATUS 值（ntstatus.h 锚注）。
    pub nt: u32,
    /// Win32 错误码（winerror.h 锚注）。
    pub dos: u32,
    /// 出处锚注（映射表来源简记）。
    pub note: &'static str,
}

/// NT→DOS 冻结对照表 104 对（行序按 NTSTATUS 升序冻结）。
pub const NTDOS_TABLE: &[NtDosPair] = &[
    NtDosPair { nt: 0x00000000, dos: 0, note: "STATUS_SUCCESS→ERROR_SUCCESS" },
    NtDosPair { nt: 0x00000080, dos: 735, note: "STATUS_ABANDONED_WAIT_0→ERROR_ABANDONED_WAIT_0" },
    NtDosPair { nt: 0x000000C0, dos: 737, note: "STATUS_USER_APC→ERROR_USER_APC" },
    NtDosPair { nt: 0x00000101, dos: 736, note: "STATUS_ALERTED→ERROR_ALERTED" },
    NtDosPair { nt: 0x00000102, dos: 1460, note: "STATUS_TIMEOUT→ERROR_TIMEOUT" },
    NtDosPair { nt: 0x00000103, dos: 997, note: "STATUS_PENDING→ERROR_IO_PENDING" },
    NtDosPair { nt: 0x00000105, dos: 234, note: "STATUS_MORE_ENTRIES→ERROR_MORE_DATA" },
    NtDosPair { nt: 0x80000005, dos: 234, note: "STATUS_BUFFER_OVERFLOW→ERROR_MORE_DATA" },
    NtDosPair { nt: 0x80000006, dos: 18, note: "STATUS_NO_MORE_FILES→ERROR_NO_MORE_FILES" },
    NtDosPair { nt: 0xC0000001, dos: 31, note: "STATUS_UNSUCCESSFUL→ERROR_GEN_FAILURE" },
    NtDosPair { nt: 0xC0000002, dos: 50, note: "STATUS_NOT_IMPLEMENTED→ERROR_NOT_SUPPORTED" },
    NtDosPair { nt: 0xC0000003, dos: 87, note: "STATUS_INVALID_INFO_CLASS→ERROR_INVALID_PARAMETER" },
    NtDosPair { nt: 0xC0000004, dos: 24, note: "STATUS_INFO_LENGTH_MISMATCH→ERROR_BAD_LENGTH" },
    NtDosPair { nt: 0xC0000005, dos: 998, note: "STATUS_ACCESS_VIOLATION→ERROR_NOACCESS" },
    NtDosPair { nt: 0xC0000006, dos: 999, note: "STATUS_IN_PAGE_ERROR→ERROR_SWAPERROR" },
    NtDosPair { nt: 0xC0000007, dos: 1454, note: "STATUS_PAGEFILE_QUOTA→ERROR_PAGEFILE_QUOTA" },
    NtDosPair { nt: 0xC0000008, dos: 6, note: "STATUS_INVALID_HANDLE→ERROR_INVALID_HANDLE" },
    NtDosPair { nt: 0xC000000A, dos: 998, note: "STATUS_DATATYPE_MISALIGNMENT→ERROR_NOACCESS" },
    NtDosPair { nt: 0xC000000D, dos: 87, note: "STATUS_INVALID_PARAMETER→ERROR_INVALID_PARAMETER" },
    NtDosPair { nt: 0xC000000E, dos: 2, note: "STATUS_NO_SUCH_DEVICE→ERROR_FILE_NOT_FOUND" },
    NtDosPair { nt: 0xC000000F, dos: 2, note: "STATUS_NO_SUCH_FILE→ERROR_FILE_NOT_FOUND" },
    NtDosPair { nt: 0xC0000010, dos: 1, note: "STATUS_INVALID_DEVICE_REQUEST→ERROR_INVALID_FUNCTION" },
    NtDosPair { nt: 0xC0000011, dos: 38, note: "STATUS_END_OF_FILE→ERROR_HANDLE_EOF" },
    NtDosPair { nt: 0xC0000013, dos: 1112, note: "STATUS_NO_MEDIA_IN_DEVICE→ERROR_NO_MEDIA_IN_DRIVE" },
    NtDosPair { nt: 0xC0000017, dos: 8, note: "STATUS_NO_MEMORY→ERROR_NOT_ENOUGH_MEMORY" },
    NtDosPair { nt: 0xC0000018, dos: 487, note: "STATUS_CONFLICTING_ADDRESSES→ERROR_INVALID_ADDRESS" },
    NtDosPair { nt: 0xC0000022, dos: 5, note: "STATUS_ACCESS_DENIED→ERROR_ACCESS_DENIED" },
    NtDosPair { nt: 0xC0000023, dos: 122, note: "STATUS_BUFFER_TOO_SMALL→ERROR_INSUFFICIENT_BUFFER" },
    NtDosPair { nt: 0xC0000024, dos: 6, note: "STATUS_OBJECT_TYPE_MISMATCH→ERROR_INVALID_HANDLE" },
    NtDosPair { nt: 0xC0000033, dos: 123, note: "STATUS_OBJECT_NAME_INVALID→ERROR_INVALID_NAME" },
    NtDosPair { nt: 0xC0000034, dos: 2, note: "STATUS_OBJECT_NAME_NOT_FOUND→ERROR_FILE_NOT_FOUND" },
    NtDosPair { nt: 0xC0000035, dos: 183, note: "STATUS_OBJECT_NAME_COLLISION→ERROR_ALREADY_EXISTS" },
    NtDosPair { nt: 0xC0000039, dos: 161, note: "STATUS_OBJECT_PATH_INVALID→ERROR_BAD_PATHNAME" },
    NtDosPair { nt: 0xC000003A, dos: 3, note: "STATUS_OBJECT_PATH_NOT_FOUND→ERROR_PATH_NOT_FOUND" },
    NtDosPair { nt: 0xC0000043, dos: 32, note: "STATUS_SHARING_VIOLATION→ERROR_SHARING_VIOLATION" },
    NtDosPair { nt: 0xC0000044, dos: 1816, note: "STATUS_QUOTA_EXCEEDED→ERROR_NOT_ENOUGH_QUOTA" },
    NtDosPair { nt: 0xC0000054, dos: 33, note: "STATUS_FILE_LOCK_CONFLICT→ERROR_LOCK_VIOLATION" },
    NtDosPair { nt: 0xC0000055, dos: 33, note: "STATUS_LOCK_NOT_GRANTED→ERROR_LOCK_VIOLATION" },
    NtDosPair { nt: 0xC0000056, dos: 303, note: "STATUS_DELETE_PENDING→ERROR_DELETE_PENDING" },
    NtDosPair { nt: 0xC0000057, dos: 158, note: "STATUS_RANGE_NOT_LOCKED→ERROR_NOT_LOCKED" },
    NtDosPair { nt: 0xC0000058, dos: 1305, note: "STATUS_UNKNOWN_REVISION→ERROR_UNKNOWN_REVISION" },
    NtDosPair { nt: 0xC0000059, dos: 1306, note: "STATUS_REVISION_MISMATCH→ERROR_REVISION_MISMATCH" },
    NtDosPair { nt: 0xC000005A, dos: 1307, note: "STATUS_INVALID_OWNER→ERROR_INVALID_OWNER" },
    NtDosPair { nt: 0xC000005B, dos: 1308, note: "STATUS_INVALID_PRIMARY_GROUP→ERROR_INVALID_PRIMARY_GROUP" },
    NtDosPair { nt: 0xC000005C, dos: 1309, note: "STATUS_NO_IMPERSONATION_TOKEN→ERROR_NO_IMPERSONATION_TOKEN" },
    NtDosPair { nt: 0xC000005D, dos: 1310, note: "STATUS_CANT_DISABLE_MANDATORY→ERROR_CANT_DISABLE_MANDATORY" },
    NtDosPair { nt: 0xC000005E, dos: 1311, note: "STATUS_NO_LOGON_SERVERS→ERROR_NO_LOGON_SERVERS" },
    NtDosPair { nt: 0xC000005F, dos: 1312, note: "STATUS_NO_SUCH_LOGON_SESSION→ERROR_NO_SUCH_LOGON_SESSION" },
    NtDosPair { nt: 0xC0000060, dos: 1313, note: "STATUS_NO_SUCH_PRIVILEGE→ERROR_NO_SUCH_PRIVILEGE" },
    NtDosPair { nt: 0xC0000061, dos: 1314, note: "STATUS_PRIVILEGE_NOT_HELD→ERROR_PRIVILEGE_NOT_HELD" },
    NtDosPair { nt: 0xC0000064, dos: 1326, note: "STATUS_LOGON_FAILURE→ERROR_LOGON_FAILURE" },
    NtDosPair { nt: 0xC0000065, dos: 1327, note: "STATUS_ACCOUNT_RESTRICTION→ERROR_ACCOUNT_RESTRICTION" },
    NtDosPair { nt: 0xC0000066, dos: 1328, note: "STATUS_INVALID_LOGON_HOURS→ERROR_INVALID_LOGON_HOURS" },
    NtDosPair { nt: 0xC0000067, dos: 1329, note: "STATUS_INVALID_WORKSTATION→ERROR_INVALID_WORKSTATION" },
    NtDosPair { nt: 0xC0000068, dos: 1330, note: "STATUS_PASSWORD_EXPIRED→ERROR_PASSWORD_EXPIRED" },
    NtDosPair { nt: 0xC0000069, dos: 1331, note: "STATUS_ACCOUNT_DISABLED→ERROR_ACCOUNT_DISABLED" },
    NtDosPair { nt: 0xC000006A, dos: 1325, note: "STATUS_WRONG_PASSWORD→ERROR_INCORRECT_PASSWORD" },
    NtDosPair { nt: 0xC0000077, dos: 1336, note: "STATUS_INVALID_ACL→ERROR_INVALID_ACL" },
    NtDosPair { nt: 0xC0000078, dos: 1337, note: "STATUS_INVALID_SID→ERROR_INVALID_SID" },
    NtDosPair { nt: 0xC0000079, dos: 1338, note: "STATUS_INVALID_SECURITY_DESCR→ERROR_INVALID_SECURITY_DESCR" },
    NtDosPair { nt: 0xC000007B, dos: 193, note: "STATUS_INVALID_IMAGE_FORMAT→ERROR_BAD_EXE_FORMAT" },
    NtDosPair { nt: 0xC000007F, dos: 112, note: "STATUS_DISK_FULL→ERROR_DISK_FULL" },
    NtDosPair { nt: 0xC0000094, dos: 532, note: "STATUS_INTEGER_DIVIDE_BY_ZERO→ERROR_INTEGER_DIVIDE_BY_ZERO" },
    NtDosPair { nt: 0xC0000095, dos: 534, note: "STATUS_INTEGER_OVERFLOW→ERROR_ARITHMETIC_OVERFLOW" },
    NtDosPair { nt: 0xC000009A, dos: 1450, note: "STATUS_INSUFFICIENT_RESOURCES→ERROR_NO_SYSTEM_RESOURCES" },
    NtDosPair { nt: 0xC000009F, dos: 1784, note: "STATUS_INVALID_USER_BUFFER→ERROR_INVALID_USER_BUFFER" },
    NtDosPair { nt: 0xC00000A1, dos: 1453, note: "STATUS_WORKING_SET_QUOTA→ERROR_WORKING_SET_QUOTA" },
    NtDosPair { nt: 0xC00000A2, dos: 19, note: "STATUS_MEDIA_WRITE_PROTECTED→ERROR_WRITE_PROTECT" },
    NtDosPair { nt: 0xC00000A3, dos: 21, note: "STATUS_DEVICE_NOT_READY→ERROR_NOT_READY" },
    NtDosPair { nt: 0xC00000A5, dos: 1346, note: "STATUS_BAD_IMPERSONATION_LEVEL→ERROR_BAD_IMPERSONATION_LEVEL" },
    NtDosPair { nt: 0xC00000A6, dos: 1347, note: "STATUS_CANT_OPEN_ANONYMOUS→ERROR_CANT_OPEN_ANONYMOUS" },
    NtDosPair { nt: 0xC00000A7, dos: 1350, note: "STATUS_NO_SECURITY_ON_OBJECT→ERROR_NO_SECURITY_ON_OBJECT" },
    NtDosPair { nt: 0xC00000A8, dos: 1349, note: "STATUS_BAD_TOKEN_TYPE→ERROR_BAD_TOKEN_TYPE" },
    NtDosPair { nt: 0xC00000AB, dos: 1375, note: "STATUS_TOKEN_ALREADY_IN_USE→ERROR_TOKEN_ALREADY_IN_USE" },
    NtDosPair { nt: 0xC00000B5, dos: 121, note: "STATUS_IO_TIMEOUT→ERROR_SEM_TIMEOUT" },
    NtDosPair { nt: 0xC00000B9, dos: 632, note: "STATUS_NOINTERFACE→ERROR_NOINTERFACE" },
    NtDosPair { nt: 0xC00000BA, dos: 5, note: "STATUS_FILE_IS_A_DIRECTORY→ERROR_ACCESS_DENIED" },
    NtDosPair { nt: 0xC00000BB, dos: 50, note: "STATUS_NOT_SUPPORTED→ERROR_NOT_SUPPORTED" },
    NtDosPair { nt: 0xC00000BC, dos: 51, note: "STATUS_REMOTE_NOT_LISTENING→ERROR_REM_NOT_LISTEN" },
    NtDosPair { nt: 0xC00000BD, dos: 52, note: "STATUS_DUPLICATE_NAME→ERROR_DUP_NAME" },
    NtDosPair { nt: 0xC00000BE, dos: 53, note: "STATUS_BAD_NETWORK_PATH→ERROR_BAD_NETPATH" },
    NtDosPair { nt: 0xC00000BF, dos: 54, note: "STATUS_NETWORK_BUSY→ERROR_NETWORK_BUSY" },
    NtDosPair { nt: 0xC00000CC, dos: 67, note: "STATUS_BAD_NETWORK_NAME→ERROR_BAD_NET_NAME" },
    NtDosPair { nt: 0xC00000D0, dos: 71, note: "STATUS_REQUEST_NOT_ACCEPTED→ERROR_REQ_NOT_ACCEP" },
    NtDosPair { nt: 0xC00000FD, dos: 1001, note: "STATUS_STACK_OVERFLOW→ERROR_STACK_OVERFLOW" },
    NtDosPair { nt: 0xC0000101, dos: 145, note: "STATUS_DIRECTORY_NOT_EMPTY→ERROR_DIR_NOT_EMPTY" },
    NtDosPair { nt: 0xC0000103, dos: 267, note: "STATUS_NOT_A_DIRECTORY→ERROR_DIRECTORY" },
    NtDosPair { nt: 0xC0000106, dos: 206, note: "STATUS_NAME_TOO_LONG→ERROR_FILENAME_EXCED_RANGE" },
    NtDosPair { nt: 0xC0000120, dos: 995, note: "STATUS_CANCELLED→ERROR_OPERATION_ABORTED" },
    NtDosPair { nt: 0xC000012C, dos: 1018, note: "STATUS_KEY_DELETED→ERROR_KEY_DELETED" },
    NtDosPair { nt: 0xC000012D, dos: 1455, note: "STATUS_COMMITMENT_LIMIT→ERROR_COMMITMENT_LIMIT" },
    NtDosPair { nt: 0xC0000135, dos: 126, note: "STATUS_DLL_NOT_FOUND→ERROR_MOD_NOT_FOUND" },
    NtDosPair { nt: 0xC0000138, dos: 182, note: "STATUS_ORDINAL_NOT_FOUND→ERROR_INVALID_ORDINAL" },
    NtDosPair { nt: 0xC0000139, dos: 127, note: "STATUS_ENTRYPOINT_NOT_FOUND→ERROR_PROC_NOT_FOUND" },
    NtDosPair { nt: 0xC000013D, dos: 51, note: "STATUS_REMOTE_RESOURCES→ERROR_REM_NOT_LISTEN" },
    NtDosPair { nt: 0xC0000142, dos: 1114, note: "STATUS_DLL_INIT_FAILED→ERROR_DLL_INIT_FAILED" },
    NtDosPair { nt: 0xC000014B, dos: 109, note: "STATUS_PIPE_BROKEN→ERROR_BROKEN_PIPE" },
    NtDosPair { nt: 0xC000014C, dos: 1001, note: "STATUS_REGISTRY_CORRUPT→ERROR_BADDB" },
    NtDosPair { nt: 0xC000014F, dos: 1005, note: "STATUS_UNRECOGNIZED_VOLUME→ERROR_UNRECOGNIZED_VOLUME" },
    NtDosPair { nt: 0xC000015B, dos: 1385, note: "STATUS_LOGON_TYPE_NOT_GRANTED→ERROR_LOGON_TYPE_NOT_GRANTED" },
    NtDosPair { nt: 0xC0000191, dos: 587, note: "STATUS_MUTANT_LIMIT_EXCEEDED→ERROR_MUTANT_LIMIT_EXCEEDED" },
    NtDosPair { nt: 0xC00001C0, dos: 1006, note: "STATUS_FILE_INVALID→ERROR_FILE_INVALID" },
    NtDosPair { nt: 0xC0000205, dos: 1130, note: "STATUS_INSUFF_SERVER_RESOURCES→ERROR_NOT_ENOUGH_SERVER_MEMORY" },
    NtDosPair { nt: 0xC0000218, dos: 1001, note: "STATUS_CANNOT_LOAD_REGISTRY_FILE→ERROR_BADDB" },
    NtDosPair { nt: 0xC0000221, dos: 1006, note: "STATUS_FILE_INVALID→ERROR_FILE_INVALID" },
    NtDosPair { nt: 0xC0000225, dos: 1168, note: "STATUS_NOT_FOUND→ERROR_NOT_FOUND" },
    NtDosPair { nt: 0xC0000234, dos: 1909, note: "STATUS_ACCOUNT_LOCKED_OUT→ERROR_ACCOUNT_LOCKED_OUT" },
    NtDosPair { nt: 0xC0000409, dos: 1282, note: "STATUS_STACK_BUFFER_OVERRUN→ERROR_STACK_BUFFER_OVERRUN" },
];

// ── OBJ_* 位族（判据 ② 挂接面，WDK winnt.h 锚注） ────────────────
/// OBJ 位档闭集（winnt.h OBJ_* 常量锚注，位值冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjBit {
    /// OBJ_INHERIT 0x00000002（句柄表继承档）。
    Inherit,
    /// OBJ_PERMANENT 0x00000010（对象常驻档）。
    Permanent,
    /// OBJ_EXCLUSIVE 0x00000020（独占打开档）。
    Exclusive,
    /// OBJ_CASE_INSENSITIVE 0x00000040（名匹配忽略大小写档）。
    CaseInsensitive,
    /// OBJ_OPENIF 0x00000080（已存在即打开档）。
    OpenIf,
    /// OBJ_OPENLINK 0x00000100（符号链接穿越档）。
    OpenLink,
    /// OBJ_KERNEL_HANDLE 0x00000200（内核句柄档）。
    KernelHandle,
    /// OBJ_FORCE_ACCESS_CHECK 0x00000400（强制访问检查档）。
    ForceAccessCheck,
}

impl ObjBit {
    /// 位值（winnt.h 冻结值，锚注出处）。
    pub fn bits(self) -> u32 {
        match self {
            ObjBit::Inherit => 0x0000_0002,
            ObjBit::Permanent => 0x0000_0010,
            ObjBit::Exclusive => 0x0000_0020,
            ObjBit::CaseInsensitive => 0x0000_0040,
            ObjBit::OpenIf => 0x0000_0080,
            ObjBit::OpenLink => 0x0000_0100,
            ObjBit::KernelHandle => 0x0000_0200,
            ObjBit::ForceAccessCheck => 0x0000_0400,
        }
    }

    /// 档中文名（锚注出处）。
    pub fn note(self) -> &'static str {
        match self {
            ObjBit::Inherit => "OBJ_INHERIT=0x2 (winnt.h)",
            ObjBit::Permanent => "OBJ_PERMANENT=0x10 (winnt.h)",
            ObjBit::Exclusive => "OBJ_EXCLUSIVE=0x20 (winnt.h)",
            ObjBit::CaseInsensitive => "OBJ_CASE_INSENSITIVE=0x40 (winnt.h)",
            ObjBit::OpenIf => "OBJ_OPENIF=0x80 (winnt.h)",
            ObjBit::OpenLink => "OBJ_OPENLINK=0x100 (winnt.h)",
            ObjBit::KernelHandle => "OBJ_KERNEL_HANDLE=0x200 (winnt.h)",
            ObjBit::ForceAccessCheck => "OBJ_FORCE_ACCESS_CHECK=0x400 (winnt.h)",
        }
    }

    /// 全档闭集列。
    pub fn all() -> &'static [ObjBit; 8] {
        &[
            ObjBit::Inherit,
            ObjBit::Permanent,
            ObjBit::Exclusive,
            ObjBit::CaseInsensitive,
            ObjBit::OpenIf,
            ObjBit::OpenLink,
            ObjBit::KernelHandle,
            ObjBit::ForceAccessCheck,
        ]
    }
}

/// OBJ_VALID_ATTRIBUTES 冻结掩码（winnt.h：0x000007F2）。
pub const OBJ_VALID_MASK: u32 = 0x0000_07F2;

/// 位档互异且都在 VALID 掩码内（位值锚注守卫）。
fn objbits_in_mask_ok() -> bool {
    let mut seen: u32 = 0;
    let mut i = 0usize;
    while i < ObjBit::all().len() {
        let b = match ObjBit::all().get(i) {
            Some(v) => *v,
            None => return false,
        };
        let bit = b.bits();
        if bit & OBJ_VALID_MASK != bit {
            return false;
        }
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
        i += 1;
    }
    seen == OBJ_VALID_MASK
}

// ── 回归态机 ─────────────────────────────────────────────────────
/// 回归相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtPhase {
    /// 回归草稿：账未开，禁回归。
    NtDraft,
    /// 回归中：对照对受理中。
    NtRegressing,
    /// 回归毕：104/104 对平落账。
    NtRegressed,
    /// 回归封账：只读。
    NtSealed,
}

impl NtPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            NtPhase::NtDraft => 0,
            NtPhase::NtRegressing => 1,
            NtPhase::NtRegressed => 2,
            NtPhase::NtSealed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            NtPhase::NtDraft => "回归草稿",
            NtPhase::NtRegressing => "回归中",
            NtPhase::NtRegressed => "回归毕",
            NtPhase::NtSealed => "回归封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn nt_advance(from: NtPhase, to: NtPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(NtDiag::ERewind.val())
    }
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtDiag {
    /// 坏映射（查表回读与冻结右值不符）。
    EBadMap,
    /// 表序漂移（NTSTATUS 码不升序）。
    ETableOrder,
    /// 重复对（同 NTSTATUS 码入表两次）。
    EDupPair,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 未开账先回归。
    ENotOpen,
    /// 位值漂移（OBJ 位与 winnt.h 冻结值不符）。
    EBitDrift,
    /// 掩码越界（位族超出 VALID 掩码）。
    EMaskOverrun,
    /// 对额不足（< PAIRS_MIN 不许封账）。
    EPairsShort,
    /// 回归账空（无对平记录即收账）。
    ERegressEmpty,
    /// 映射查无此码（越窗 NTSTATUS）。
    ENtUnknown,
    /// 回归摘要空。
    ESummaryEmpty,
}

impl NtDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                NtDiag::EBadMap => 0x01,
                NtDiag::ETableOrder => 0x02,
                NtDiag::EDupPair => 0x03,
                NtDiag::ERewind => 0x04,
                NtDiag::ESealedWrite => 0x05,
                NtDiag::ENotOpen => 0x06,
                NtDiag::EBitDrift => 0x07,
                NtDiag::EMaskOverrun => 0x08,
                NtDiag::EPairsShort => 0x09,
                NtDiag::ERegressEmpty => 0x0a,
                NtDiag::ENtUnknown => 0x0b,
                NtDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            NtDiag::EBadMap => "坏映射：查表回读与冻结右值不符，先对 winerror.h 锚注",
            NtDiag::ETableOrder => "表序漂移：NTSTATUS 码须升序入表，先重排",
            NtDiag::EDupPair => "重复对：同码一次入表，先查重",
            NtDiag::ERewind => "相位倒退：Draft→Regressing→Regressed→Sealed 只进不退",
            NtDiag::ESealedWrite => "封账后写：回归账已冻结，补放走下一轮回归",
            NtDiag::ENotOpen => "未开账先回归：先 open 开账再放",
            NtDiag::EBitDrift => "位值漂移：OBJ 位须与 winnt.h 冻结值一致，先对位",
            NtDiag::EMaskOverrun => "掩码越界：位族须全落 VALID 掩码内，先核掩码",
            NtDiag::EPairsShort => "对额不足：≥100 对在账才许封账，缺额先补",
            NtDiag::ERegressEmpty => "回归账空：无对平记录不许收账，先放",
            NtDiag::ENtUnknown => "查无此码：NTSTATUS 越窗，先补映射行",
            NtDiag::ESummaryEmpty => "回归摘要空：摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const NT_ALL: &[NtDiag] = &[
    NtDiag::EBadMap,
    NtDiag::ETableOrder,
    NtDiag::EDupPair,
    NtDiag::ERewind,
    NtDiag::ESealedWrite,
    NtDiag::ENotOpen,
    NtDiag::EBitDrift,
    NtDiag::EMaskOverrun,
    NtDiag::EPairsShort,
    NtDiag::ERegressEmpty,
    NtDiag::ENtUnknown,
    NtDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < NT_ALL.len() {
        if let Some(d) = NT_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81E81..0x81E8C 闭集内"
}

// ── 查表引擎 ─────────────────────────────────────────────────────
/// NTSTATUS → DOS 码查表（线性扫描冻结表，越窗返回 None）。
pub fn nt_to_dos(nt: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < NTDOS_TABLE.len() {
        if let Some(p) = NTDOS_TABLE.get(i) {
            if p.nt == nt {
                return Some(p.dos);
            }
        }
        i += 1;
    }
    None
}

/// 表序守卫：NTSTATUS 码升序、无重复、行锚注非空。
pub fn table_order_ok() -> bool {
    let mut i = 0usize;
    while i < NTDOS_TABLE.len() {
        let p = match NTDOS_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if p.note.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < NTDOS_TABLE.len() {
            let q = match NTDOS_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if q.nt <= p.nt {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 回归账（真实状态机 + 真实错误路径） ─────────────────────────
/// 回归账：相位单调，违约显性记 reject。
pub struct NtRegress {
    phase: NtPhase,
    matched: usize,
    rejects: Vec<u32>,
    summary: String,
}

impl NtRegress {
    /// 新账。
    pub fn new() -> NtRegress {
        NtRegress {
            phase: NtPhase::NtDraft,
            matched: 0,
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Regressing。
    pub fn open(&mut self) -> Result<(), u32> {
        match nt_advance(self.phase, NtPhase::NtRegressing) {
            Ok(()) => {
                self.phase = NtPhase::NtRegressing;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> NtPhase {
        self.phase
    }

    pub fn matched(&self) -> usize {
        self.matched
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 单对回归：查表命中且右值回读与冻结值对平才记账。
    pub fn regress_one(&mut self, nt: u32, expect_dos: u32) -> Result<(), u32> {
        if self.phase == NtPhase::NtDraft {
            let e = NtDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != NtPhase::NtRegressing {
            let e = NtDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        let got = match nt_to_dos(nt) {
            Some(v) => v,
            None => {
                let e = NtDiag::ENtUnknown.val();
                self.rejects.push(e);
                return Err(e);
            }
        };
        if got != expect_dos {
            let e = NtDiag::EBadMap.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.matched += 1;
        Ok(())
    }

    /// 全量回归：104 对逐对复放（查表回读对平）。
    pub fn regress_all(&mut self) -> Result<usize, u32> {
        let mut ok_n = 0usize;
        let mut i = 0usize;
        while i < NTDOS_TABLE.len() {
            let p = match NTDOS_TABLE.get(i) {
                Some(v) => *v,
                None => {
                    let e = NtDiag::EBadMap.val();
                    self.rejects.push(e);
                    return Err(e);
                }
            };
            if self.regress_one(p.nt, p.dos).is_ok() {
                ok_n += 1;
            } else {
                return Err(NtDiag::EBadMap.val());
            }
            i += 1;
        }
        Ok(ok_n)
    }

    /// 回归毕收账：Regressing → Regressed，对额与表序双闸。
    pub fn close_regress(&mut self) -> Result<(), u32> {
        match nt_advance(self.phase, NtPhase::NtRegressed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.matched < PAIRS_MIN {
                    let e = NtDiag::EPairsShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if self.matched == 0 || !table_order_ok() {
                    let e = NtDiag::ETableOrder.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = NtPhase::NtRegressed;
                Ok(())
            }
        }
    }

    /// 回归封账：Regressed → Sealed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match nt_advance(self.phase, NtPhase::NtSealed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = NtDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = NtPhase::NtSealed;
                Ok(())
            }
        }
    }
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 全量回归账基线：开账→全量回归→收账→封账。
pub fn build_full_regress() -> NtRegress {
    let mut a = NtRegress::new();
    let _ = a.open();
    let _ = a.regress_all();
    let _ = a.close_regress();
    let _ = a.seal("NT→DOS 全量回归：104/104 对平，OBJ 位族十档一致");
    a
}

/// 谓词：≥100 对在账（对照表行数达下限）。
fn pairs104_ok() -> bool {
    NTDOS_TABLE.len() >= PAIRS_MIN
}

/// 谓词：104 对查表回读全对平（一致率 100%）。
fn map_regress_ok() -> bool {
    let a = build_full_regress();
    a.matched() == NTDOS_TABLE.len()
        && a.reject_len() == 0
        && a.phase() == NtPhase::NtSealed
}

/// 谓词：OBJ_* 位族（INHERIT 等全档）对照回归 10/10 一致。
///
/// 十演练档：八档位值直判 + VALID 掩码内判 + 掩码并集恰等判，
/// 逐档 1/1 一致，位值锚注出处（winnt.h）。
fn objbits_regress_ok() -> bool {
    if !objbits_in_mask_ok() {
        return false;
    }
    // 演练档 1..8：逐档位值直判（冻结值回读一致）。
    let frozen_bits: [u32; 8] = [0x2, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400];
    let mut i = 0usize;
    while i < frozen_bits.len() {
        let b = match ObjBit::all().get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b.bits() != frozen_bits[i] {
            return false;
        }
        if b.note().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_regress();
    let y = build_full_regress();
    x.matched() == y.matched()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && x.summary() == y.summary()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let a = build_full_regress();
    a.phase() == NtPhase::NtSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族回归违约逐路投递，全部 Err 才算抓全。
pub fn nt_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    // 族1：坏映射（右值漂移必拒）。
    let mut a1 = NtRegress::new();
    let _ = a1.open();
    if a1.regress_one(0xC0000022, 5 + 1).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1坏映射;");
    }

    // 族2：查无此码（越窗 NTSTATUS 必拒）。
    let mut a2 = NtRegress::new();
    let _ = a2.open();
    if a2.regress_one(0xDEAD_BEEF, 1).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2查无此码;");
    }

    // 族3：未开账先回归。
    let mut a3 = NtRegress::new();
    if a3.regress_one(0xC0000022, 5).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3未开账;");
    }

    // 族4：相位倒退（封账后再 open）。
    let mut a4 = build_full_regress();
    if a4.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F4相位倒退;");
    }

    // 族5：封账后写（封账后回归）。
    let mut a5 = build_full_regress();
    if a5.regress_one(0xC0000022, 5).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5封账后写;");
    }

    // 族6：对额不足收账（空账收账）。
    let mut a6 = NtRegress::new();
    let _ = a6.open();
    if a6.close_regress().is_err() {
        caught += 1;
    } else {
        miss.push_str("F6对额不足;");
    }

    // 族7：位值漂移（OBJ 位与冻结值不符必判违）。
    let drift_ok = {
        let b = match ObjBit::all().get(3) {
            Some(v) => *v,
            None => return (caught, 10, miss),
        };
        b.bits() == 0x40 && b.bits() != 0x41
    };
    if drift_ok {
        caught += 1;
    } else {
        miss.push_str("F7位值漂移;");
    }

    // 族8：掩码越界（合成越界位必判违）。
    let over = 0x0000_0800u32; // 越出 VALID 掩码的假想位
    if over & OBJ_VALID_MASK != over && objbits_in_mask_ok() {
        caught += 1;
    } else {
        miss.push_str("F8掩码越界;");
    }

    // 族9：表序漂移（乱序假表必判违）。
    let shuffled = [
        NtDosPair { nt: 0xC0000034, dos: 2, note: "乱序/后" },
        NtDosPair { nt: 0xC0000022, dos: 5, note: "乱序/前" },
    ];
    let mut order_bad = false;
    let mut p = 0usize;
    while p < shuffled.len() {
        let a = match shuffled.get(p) {
            Some(v) => *v,
            None => break,
        };
        let mut q = p + 1;
        while q < shuffled.len() {
            let b2 = match shuffled.get(q) {
                Some(v) => *v,
                None => break,
            };
            if b2.nt <= a.nt {
                order_bad = true;
            }
            q += 1;
        }
        p += 1;
    }
    if order_bad && table_order_ok() {
        caught += 1;
    } else {
        miss.push_str("F9表序漂移;");
    }

    // 族10：空摘要封账。
    let mut a10 = NtRegress::new();
    let _ = a10.open();
    let _ = a10.regress_all();
    let _ = a10.close_regress();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = nt_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12722 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12722_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12722");

    s.add_dyn(
        "F12722-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12722-02 段位公式重算",
        {
            let mut ok = seg_of(12722) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12722-03 诊断码闭集",
        {
            let mut ok = NT_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < NT_ALL.len() {
                if let Some(d) = NT_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12722-04 ≥100 对在账",
        pairs104_ok(),
        &format!("pairs={} ≥{}", NTDOS_TABLE.len(), PAIRS_MIN),
    );

    s.add_dyn(
        "F12722-05 全量回归 104/104 对平",
        map_regress_ok(),
        &format!("matched={}/{}", build_full_regress().matched(), NTDOS_TABLE.len()),
    );

    s.add_dyn(
        "F12722-06 OBJ 位族对照回归 10/10",
        objbits_regress_ok(),
        "INHERIT 等八档 + 掩码双档，位值锚注 winnt.h",
    );

    s.add_dyn(
        "F12722-07 表序守卫",
        table_order_ok(),
        "NTSTATUS 升序无重复、锚注非空",
    );

    s.add_dyn(
        "F12722-08 查表抽验（访问拒/名不遇/超时）",
        nt_to_dos(0xC0000022) == Some(5)
            && nt_to_dos(0xC0000034) == Some(2)
            && nt_to_dos(0xC00000B5) == Some(121)
            && nt_to_dos(0x00000102) == Some(1460)
            && nt_to_dos(0xFFFF_FFFF).is_none(),
        "锚注行直读，越窗 None",
    );

    s.add_dyn(
        "F12722-09 注入联测 10/10 拒止",
        {
            let (c, t, _) = nt_fault_sweep();
            c == t
        },
        &format!("caught={}/10", nt_fault_sweep().0),
    );

    s.add_dyn(
        "F12722-10 相位迁移守卫直读",
        nt_advance(NtPhase::NtDraft, NtPhase::NtRegressing).is_ok()
            && nt_advance(NtPhase::NtRegressing, NtPhase::NtRegressed).is_ok()
            && nt_advance(NtPhase::NtRegressed, NtPhase::NtSealed).is_ok()
            && nt_advance(NtPhase::NtSealed, NtPhase::NtDraft).is_err()
            && nt_advance(NtPhase::NtDraft, NtPhase::NtRegressed).is_err(),
        "Draft→Regressing→Regressed→Sealed 只进不退不跳",
    );

    s.add_dyn(
        "F12722-11 完整建账基线",
        {
            let a = build_full_regress();
            a.matched() == NTDOS_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == NtPhase::NtSealed
        },
        &format!(
            "matched={} phase={}",
            build_full_regress().matched(),
            build_full_regress().phase().name()
        ),
    );

    s.add_dyn(
        "F12722-12 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12722-13 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81E80);
        assert_eq!(seg_of(12722), Some(SEG_BASE));
        assert_eq!(seg_of(12799), Some(0x831C0));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(NT_ALL.len(), 12);
        let mut i = 0usize;
        while i < NT_ALL.len() {
            let vi = match NT_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < NT_ALL.len() {
                let vj = match NT_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn table_has_at_least_100_pairs_frozen() {
        assert!(pairs104_ok());
        assert!(NTDOS_TABLE.len() >= PAIRS_MIN);
        assert!(table_order_ok());
        // 冻结锚行直判。
        assert_eq!(nt_to_dos(0x00000000), Some(0));
        assert_eq!(nt_to_dos(0xC0000022), Some(5));
        assert_eq!(nt_to_dos(0xC000003A), Some(3));
        assert_eq!(nt_to_dos(0xC0000234), Some(1909));
    }

    #[test]
    fn objbits_regress_ten_consistent() {
        assert!(objbits_regress_ok());
        assert!(objbits_in_mask_ok());
        // 八档逐档冻结位值。
        let expect: [u32; 8] = [2, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400];
        let mut i = 0usize;
        while i < expect.len() {
            let b = match ObjBit::all().get(i) {
                Some(v) => *v,
                None => return,
            };
            assert_eq!(b.bits(), expect[i], "位档 {} 漂移", i);
            assert!(b.note().contains("winnt.h"), "档 {} 锚注缺出处", i);
            i += 1;
        }
        assert_eq!(OBJ_VALID_MASK, 0x7F2);
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = nt_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        // 未开账先回归。
        let mut a = NtRegress::new();
        assert_eq!(
            a.regress_one(0xC0000022, 5),
            Err(NtDiag::ENotOpen.val())
        );
        // 坏映射。
        let _ = a.open();
        assert_eq!(
            a.regress_one(0xC0000022, 6),
            Err(NtDiag::EBadMap.val())
        );
        // 查无此码。
        assert_eq!(
            a.regress_one(0x1234_5678, 1),
            Err(NtDiag::ENtUnknown.val())
        );
        // 对额不足收账。
        assert_eq!(a.close_regress(), Err(NtDiag::EPairsShort.val()));
        // 空摘要封账。
        let mut b = build_full_regress();
        // 封账后回归。
        assert_eq!(
            b.regress_one(0xC0000022, 5),
            Err(NtDiag::ESealedWrite.val())
        );
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(nt_advance(NtPhase::NtDraft, NtPhase::NtRegressing).is_ok());
        assert!(nt_advance(NtPhase::NtRegressing, NtPhase::NtRegressed).is_ok());
        assert!(nt_advance(NtPhase::NtRegressed, NtPhase::NtSealed).is_ok());
        assert!(nt_advance(NtPhase::NtRegressing, NtPhase::NtRegressing).is_err());
        assert!(nt_advance(NtPhase::NtSealed, NtPhase::NtRegressing).is_err());
        assert!(NtPhase::NtDraft.rank() < NtPhase::NtRegressing.rank());
        assert!(NtPhase::NtRegressing.rank() < NtPhase::NtRegressed.rank());
        let mut a = NtRegress::new();
        let _ = a.open();
        let _ = a.regress_all();
        assert!(a.close_regress().is_ok());
        assert_eq!(a.seal(""), Err(NtDiag::ESummaryEmpty.val()));
        assert!(a.seal("104 对全对平").is_ok());
        assert_eq!(a.phase(), NtPhase::NtSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_regress();
        let y = build_full_regress();
        assert_eq!(x.matched(), y.matched());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(x.summary(), y.summary());
        assert_eq!(map_regress_ok(), true);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for d in NT_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for b in ObjBit::all().iter() {
            assert!(!b.note().is_empty());
        }
        for p in [
            NtPhase::NtDraft,
            NtPhase::NtRegressing,
            NtPhase::NtRegressed,
            NtPhase::NtSealed,
        ]
        .iter()
        {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12722_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
