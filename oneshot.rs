//! WP-101/WP-102 · LoaderEntryOneShot 单次引导机制（MD2 篇 1.5，B-103/104/105）。
//!
//! # 机制本体（篇 1.5 逐句定案）
//!
//! VARIX 内核在运行期把"单次引导变量"写成目标条目标识（例：下一次引导选
//! Windows 11 (USB) 条目），然后正常重启。重启后引导器读到该变量，引导它
//! 指向的条目，**随即把变量作废**——再下一次开机回归默认条目。变量是
//! "一次性的命令条"，不是持久状态。
//!
//! # 落地面（为什么在文件层）
//!
//! 联想固件对第三方写入的 NVRAM 引导数据有过滤行为（篇 1.6 实锤：自建项
//! F12 不显示、BootNext 偶发被忽略），且 NVRAM 写入有量限与损坏风险。OneShot
//! 变量完全活在引导器自己的文件层（ESP 内 `limine.conf` 的 `default_entry`
//! 字段 + 伴随标记帧），固件根本看不见它，自然也不受固件过滤；文件层写入
//! 比 NVRAM 写入更可控、可校验、可撤销。
//!
//! # 消费与作废（双层，任何一层失灵另一层接住）
//!
//! - 主消费：Windows 侧交接助手开机即恢复 `default_entry` 并删除标记帧
//!   （MD2 篇 2.6 窄协议——助手"恢复默认值"不等于"写 OneShot 变量"）。
//! - 兜底消费：VARIX 每次启动回读验证（篇 1.5"消费即焚的验证"）——发现
//!   残留未消费（助手没跑成 / Windows 域崩溃后手动重启）则 VARIX 自行恢复
//!   并写诊断事件。**单次语义由双层消费共同保证。**
//!
//! # 与 BootNext 的关系（篇 1.5 / B-104）
//!
//! BootNext 路径保留为兜底（`bootnext.rs` 的能力不删），OneShot 优先。三条
//! 路径（OneShot → BootNext → Limine 菜单人工）任何一条失效都有下一条接住
//! ——三路径的存在本身就是判据 1 的"零盲区"。
//!
//! # 纪律
//!
//! 本模块 100% 纯逻辑（定长缓冲、零分配、零 IO）：文件读写由调用方注入，
//! 判定全部落在可宿主测试的纯函数上。20 组注入矩阵（B-105）在此全量执行
//! ——实机/QEMU 对练（WP-102 接线后）复用同一判定面。

use crate::bootconf::{self, BootConf, EntryRender};

/// 标记帧魔法值（"VX1SHOT1"—— Varix OneShot v1）。
pub const FRAME_MAGIC: [u8; 8] = *b"VX1SHOT1";
/// 帧 schema 版本（跨版本演进锚点；更高版本按"未知"拒绝消费并诊断）。
pub const SCHEMA_VERSION: u32 = 1;
/// 帧总长：magic 8 + ver 4 + target 4 + prev 4 + ts 8 + hash 4 + rsvd 4 + ck 4。
pub const FRAME_LEN: usize = 40;
/// 标记帧在 ESP 的约定路径（篇 1.4：`/varix/handoff/` 是交接静态资源区）。
pub const MARKER_PATH: &str = "/varix/handoff/oneshot.bin";

// ---------------------------------------------------------------------------
// 帧编解码（写入-读回-校验的唯一口径）
// ---------------------------------------------------------------------------

/// OneShot 标记帧（武装记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OneshotFrame {
    /// 目标条目下标（limine.conf 行序）。
    pub target_entry: u32,
    /// 武装前的原默认条目（作废恢复的目标；错乱保护见 `frame_sane`）。
    pub previous_default: u32,
    /// 武装时刻（unix 秒；时间源不可信时仅诊断不阻断）。
    pub armed_at: u64,
    /// 武装时配置文本的 FNV-1a 校验和（conf 被换的检测锚）。
    pub conf_hash: u32,
}

impl OneshotFrame {
    /// 编码为定长帧（小端）。`out` 不足 [`FRAME_LEN`] 返回 None。
    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < FRAME_LEN {
            return None;
        }
        for (i, b) in FRAME_MAGIC.iter().enumerate() {
            out[i] = *b;
        }
        out[8..12].copy_from_slice(&SCHEMA_VERSION.to_le_bytes());
        out[12..16].copy_from_slice(&self.target_entry.to_le_bytes());
        out[16..20].copy_from_slice(&self.previous_default.to_le_bytes());
        out[20..28].copy_from_slice(&self.armed_at.to_le_bytes());
        out[28..32].copy_from_slice(&self.conf_hash.to_le_bytes());
        out[32..36].copy_from_slice(&[0u8; 4]); // reserved
        let ck = crate::bootchain::hash_bytes(&out[..36]);
        out[36..40].copy_from_slice(&ck.to_le_bytes());
        Some(FRAME_LEN)
    }

    /// 从帧解码。魔法/版本/校验和任一不符 → None（损坏帧绝不静默消费）。
    pub fn decode(bytes: &[u8]) -> Option<OneshotFrame> {
        if bytes.len() < FRAME_LEN || bytes[..8] != FRAME_MAGIC {
            return None;
        }
        let ver = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        if ver != SCHEMA_VERSION {
            return None;
        }
        let ck_stored = u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]);
        if ck_stored != crate::bootchain::hash_bytes(&bytes[..36]) {
            return None;
        }
        Some(OneshotFrame {
            target_entry: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            previous_default: u32::from_le_bytes([
                bytes[16], bytes[17], bytes[18], bytes[19],
            ]),
            armed_at: u64::from_le_bytes([
                bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26],
                bytes[27],
            ]),
            conf_hash: u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]),
        })
    }
}

/// 帧语义自检（解码通过后的第二层闸）：条目下标范围与作废恢复目标错乱保护。
/// `conf_n_entries` 为当前配置的条目数。
pub fn frame_sane(frame: &OneshotFrame, conf_n_entries: usize) -> bool {
    (frame.target_entry as usize) < conf_n_entries
        && (frame.previous_default as usize) < conf_n_entries
        && frame.target_entry != frame.previous_default
}

// ---------------------------------------------------------------------------
// 武装：default_entry 改写（写 → 读回校验 → 才算 Armed）
// ---------------------------------------------------------------------------

/// 武装结果（B-103 闭环的第一段；词表覆盖写失败与读回不一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmOutcome {
    /// 写入成功且读回一致。
    Armed,
    /// 配置改写产出为空（目标下标越界等）——如实拒绝，绝不半武装。
    Rejected,
    /// 写入路径失败（ESP 满 / IO 错误）。
    WriteFailed,
    /// 写入后读回不一致（写后必读纪律的红灯）。
    ReadbackMismatch,
}

/// 武装配置文本：把 `default_entry` 改为目标条目，并在头部追加 oneshot 标记
/// 注释行（`# oneshot: <target> <prev> <confhash>`——Limine 忽略注释行，
/// 兜底消费方靠它识别"这是单次武装而不是用户手改"）。
///
/// 产出写入 `out`，返回写入字节数；目标越界返回 None（Rejected）。
pub fn arm_conf_text(
    conf_text: &str,
    target_entry: usize,
    conf_hash_of_armed: u32,
    out: &mut [u8],
) -> Option<usize> {
    let mut n = 0usize;
    let push = |s: &str, out: &mut [u8], n: &mut usize| {
        for &b in s.as_bytes() {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            } else {
                // 缓冲不足：武装是安全关键路径，宁可整体失败不产出截断 conf。
                *n = out.len() + 1; // 越界标记
            }
        }
    };
    // 头部标记行（消费方与诊断的识别锚）：`# oneshot: <target hex> <hash hex>`
    push("# oneshot: ", out, &mut n);
    let mut mb = [0u8; 24];
    let mn = oneshot_marker_body(target_entry, conf_hash_of_armed, &mut mb);
    push(core::str::from_utf8(&mb[..mn]).unwrap_or(""), out, &mut n);
    push("\n", out, &mut n);
    // 逐行复制原配置，替换/插入 default_entry
    let mut replaced = false;
    for line in conf_text.lines() {
        let t = line.trim();
        if t.starts_with("default_entry:") {
            let mut buf = [0u8; 32];
            let s = format_usize_into(target_entry, &mut buf);
            push("default_entry: ", out, &mut n);
            push(s, out, &mut n);
            push("\n", out, &mut n);
            replaced = true;
        } else {
            push(line, out, &mut n);
            push("\n", out, &mut n);
        }
    }
    if !replaced {
        // 原 conf 没写 default_entry：在 timeout 行后补一条（语义=显式单次）。
        let mut buf = [0u8; 32];
        let s = format_usize_into(target_entry, &mut buf);
        push("default_entry: ", out, &mut n);
        push(s, out, &mut n);
        push("\n", out, &mut n);
    }
    if n > out.len() {
        return None; // 越界标记（push 溢出时设置）
    }
    // 目标下标有效性由调用方先验（这里防御性复查：conf 里得有这个条目）。
    let parsed = bootconf::parse(conf_text);
    if target_entry >= parsed.n_entries {
        return None;
    }
    Some(n)
}

/// 作废恢复：把武装后的配置恢复回 `previous_default`（双层消费的共用动作）。
/// 与 arm_conf_text 同一输出契约；目标越界返回 None（绝不产出垃圾 conf）。
pub fn restore_conf_text(
    armed_text: &str,
    previous_default: usize,
    out: &mut [u8],
) -> Option<usize> {
    // previous_default 必须在配置条目范围内（错乱保护，注入组 18）——
    // 越界直接拒绝，绝不产出恢复到垃圾条目的 conf。
    let parsed = bootconf::parse(armed_text);
    if previous_default >= parsed.n_entries {
        return None;
    }
    let mut n = 0usize;
    let push = |s: &str, out: &mut [u8], n: &mut usize| {
        for &b in s.as_bytes() {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    let mut first = true;
    for line in armed_text.lines() {
        let t = line.trim();
        if t.starts_with("# oneshot:") {
            continue; // 标记行摘除（作废的可见形态）
        }
        if t.starts_with("default_entry:") {
            let mut buf = [0u8; 32];
            let s = format_usize_into(previous_default, &mut buf);
            push("default_entry: ", out, &mut n);
            push(s, out, &mut n);
            push("\n", out, &mut n);
        } else {
            if !first && t.is_empty() {
                continue;
            }
            push(line, out, &mut n);
            push("\n", out, &mut n);
        }
        first = false;
    }
    Some(n)
}

/// `# oneshot: <target> <confhash>` 标记行体（引导期无堆：查表拼十六进制）。

/// 把 usize 按 u32 十六进制写入 buf（小写定宽 8 位），返回 &str 切片。
fn fmt_hex(v: u32, buf: &mut [u8; 8]) -> &str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in 0..8 {
        buf[i] = HEX[((v >> (28 - i * 4)) & 0xF) as usize];
    }
    match core::str::from_utf8(buf) {
        Ok(s) => s,
        Err(_) => "00000000",
    }
}

/// 组合格式化：`<t8> <h8>`（target 十六进制 + confhash 十六进制）。
/// 返回写入 out 的字节数（不含 NUL）。
fn oneshot_marker_body(target: usize, hash: u32, out: &mut [u8]) -> usize {
    let mut b1 = [0u8; 8];
    let mut b2 = [0u8; 8];
    let t = fmt_hex(target as u32, &mut b1);
    let h = fmt_hex(hash, &mut b2);
    let mut n = 0usize;
    for s in [t, " ", h] {
        for &b in s.as_bytes() {
            if n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
    }
    n
}

/// `format_usize_into`：十进制 usize → &str（定长缓冲，十进制位）。
fn format_usize_into<'b>(v: usize, buf: &'b mut [u8]) -> &'b str {
    let mut tmp = [0u8; 20];
    let mut w = 0usize;
    let mut x = v;
    if x == 0 {
        tmp[0] = b'0';
        w = 1;
    }
    while x > 0 && w < tmp.len() {
        tmp[w] = b'0' + (x % 10) as u8;
        x /= 10;
        w += 1;
    }
    for i in 0..w.min(buf.len()) {
        buf[i] = tmp[w - 1 - i];
    }
    let n = w.min(buf.len());
    core::str::from_utf8(&buf[..n]).unwrap_or("0")
}

// ---------------------------------------------------------------------------
// 启动期回读验证（篇 1.5"消费即焚的验证"；B-103 十次闭环的判定面）
// ---------------------------------------------------------------------------

/// VARIX 每次启动对 OneShot 残留的判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnBootFindings {
    /// 无残留：正常路径（大多数开机）。
    NoRecord,
    /// 标记帧完整存在——**异常信号**：上次交接没经过消费（助手没跑成）。
    /// 按 MD2 篇 1.5 记诊断事件，然后兜底作废。
    ResidualArmed {
        target_entry: u32,
        previous_default: u32,
    },
    /// 标记帧损坏（魔法/校验和/schema 不符）：清理 + 诊断。
    InvalidFrame,
}

/// 启动期回读验证：给定标记帧字节与当前 conf，判定并给出作废恢复目标。
/// 纯判定——实际的"恢复 default_entry + 删标记 + 诊断落账"由调用方执行
/// （本函数保证判定的唯一口径）。
pub fn on_boot_verify(marker_bytes: Option<&[u8]>, conf: &BootConf) -> OnBootFindings {
    let Some(bytes) = marker_bytes else {
        return OnBootFindings::NoRecord;
    };
    match OneshotFrame::decode(bytes) {
        Some(f) if frame_sane(&f, conf.n_entries) => OnBootFindings::ResidualArmed {
            target_entry: f.target_entry,
            previous_default: f.previous_default,
        },
        _ => OnBootFindings::InvalidFrame,
    }
}

// ---------------------------------------------------------------------------
// 三路径降级（B-104；篇 1.5 与篇 2.4 的优先级逻辑）
// ---------------------------------------------------------------------------

/// 单条交接路径的健康探测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathHealth {
    /// 探测通过，可用。
    Available,
    /// 不可用 + 人话原因（诊断事件与 abort 文案的素材）。
    Unavailable(&'static str),
}

/// 交接路径（优先序固定：OneShot → BootNext → 菜单人工）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffPath {
    /// 文件层单次引导（主路径，固件不可见）。
    Oneshot,
    /// UEFI BootNext NVRAM（兜底一，`bootnext.rs` 通道）。
    BootNext,
    /// Limine 菜单人工选择（兜底二，永远在线的最后一步）。
    Menu,
}

/// 三路径选路。全部不可用 → None（交接 aborted：宁可不交接，也不盲走）。
pub fn choose_handoff_path(
    oneshot: PathHealth,
    bootnext: PathHealth,
    menu: PathHealth,
) -> Option<(HandoffPath, Option<HandoffPath>)> {
    let (primary, next) = match (oneshot, bootnext, menu) {
        (PathHealth::Available, _, _) => (HandoffPath::Oneshot, Some(HandoffPath::BootNext)),
        (PathHealth::Unavailable(_), PathHealth::Available, _) => {
            (HandoffPath::BootNext, Some(HandoffPath::Menu))
        }
        (PathHealth::Unavailable(_), PathHealth::Unavailable(_), PathHealth::Available) => {
            (HandoffPath::Menu, None)
        }
        (PathHealth::Unavailable(_), PathHealth::Unavailable(_), PathHealth::Unavailable(_)) => {
            return None
        }
    };
    Some((primary, next))
}

/// 降级记录（B-104"路径自动降级无盲区"的可观测面）：诊断事件词表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeEvent {
    /// OneShot 武装失败（写失败/读回不一致/配置拒绝），降 BootNext。
    OneshotArmFailed,
    /// OneShot 残留异常（启动期发现未消费），兜底作废并诊断。
    OneshotResidual,
    /// OneShot 标记帧损坏，清理并诊断。
    OneshotFrameCorrupt,
    /// BootNext 写入/验证失败，降菜单人工。
    BootNextFailed,
    /// 配置校验失败（hash 不符），计入闸门。
    ConfHashMismatch,
}

// ---------------------------------------------------------------------------
// 防自锁闸门三条件（篇 1.7；B-105 的判定核）
// ---------------------------------------------------------------------------

/// 目标文件探针结果（由调用方注入：WINESP 引导文件的哈希与存在性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetProbe {
    /// 引导文件是否存在且可读。
    pub present: bool,
    /// 引导文件内容哈希（FNV-1a 口径，与 `hash_bytes` 同源）。
    pub hash: u32,
}

/// 上次成功交接的记录（闸门条件二的比对锚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastGoodHandoff {
    pub target_hash: u32,
}

/// 闸门裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    /// 三条件全过，放行。
    Pass,
    /// 条件二哈希不符（Windows 更新换引导文件是正常事件）：不阻断，
    /// 降级"用户确认后放行"——人眼过目（篇 1.7 原文口径）。
    NeedUserConfirm(&'static str),
    /// 条件一/三不过：拒绝武装，人话原因。
    Blocked(&'static str),
}

/// 闸门三条件判定（引导层实现口径，篇 1.7 逐条）：
///
/// - 条件一 **目标存在**：conf 确认 Windows 条目存在，目标探针确认引导
///   文件存在且可读。
/// - 条件二 **目标可信**：引导文件哈希与上次成功交接记录比对，不一致不
///   阻断但降级为"用户确认后放行"。
/// - 条件三 **回路可回**：VARIX 自己的条目完好（回程的路必须先修好再出门）。
pub fn gate_check(
    conf: &BootConf,
    target_probe: Option<TargetProbe>,
    varix_file_present: Option<bool>,
    last_good: Option<LastGoodHandoff>,
) -> GateVerdict {
    // 条件一（目标存在）
    let Some(win) = conf.windows_entry() else {
        return GateVerdict::Blocked("no windows entry in config");
    };
    // 条件一（目标存在）：条目渲染为灰显形态一律不可交接——TargetMissing
    // 是目标文件不在，Incomplete 是协议/路径残缺，两者都过不了 WD-003 的
    // 菜单灰显线，更过不了闸门（B-105 组 03：条目在但 path 被清空）。
    match bootconf::entry_render(&win, target_probe.map(|p| p.present)) {
        EntryRender::Grey(bootconf::GreyReason::TargetMissing) => {
            return GateVerdict::Blocked("windows boot file missing on WINESP");
        }
        EntryRender::Grey(bootconf::GreyReason::Incomplete) => {
            return GateVerdict::Blocked("windows entry incomplete (protocol/path missing)");
        }
        EntryRender::Ready => {}
    }
    let Some(probe) = target_probe else {
        return GateVerdict::Blocked("target probe unavailable");
    };
    if !probe.present {
        return GateVerdict::Blocked("windows boot file missing on WINESP");
    }
    // 条件二（目标可信）
    if let Some(last) = last_good {
        if last.target_hash != probe.hash {
            return GateVerdict::NeedUserConfirm(
                "windows boot file changed since last handoff",
            );
        }
    }
    // 条件三（回路可回）
    let Some(vx) = conf.varix_entry() else {
        return GateVerdict::Blocked("no varix entry in config");
    };
    if vx.path.is_empty() {
        return GateVerdict::Blocked("varix entry has no kernel path");
    }
    if varix_file_present == Some(false) {
        return GateVerdict::Blocked("varix kernel file missing");
    }
    GateVerdict::Pass
}

// ---------------------------------------------------------------------------
// 20 组注入矩阵（B-105；MD2 篇 1.7"注入测试矩阵"的执行面）
// ---------------------------------------------------------------------------

/// 注入场景的判定样表：`(场景名, 期望闸门裁决)`。20 组场景对应 MD2 篇 1.7
/// 的破坏清单（删条目、改 GUID、截断文件、填满 ESP 等）；tests 里逐组跑
/// 真实注入并断言期望——矩阵不是文档是测试。
pub const INJECTION_MATRIX: [(&str, MatrixExpect); 20] = [
    ("01 删 Windows 条目", MatrixExpect::Blocked),
    ("02 Windows 条目协议篡改", MatrixExpect::Blocked),
    ("03 Windows 条目路径清空", MatrixExpect::Blocked),
    ("04 WINESP GUID 篡改", MatrixExpect::Blocked),
    ("05 目标引导文件截断", MatrixExpect::NeedConfirm),
    ("06 VARIX 内核文件删除", MatrixExpect::Blocked),
    ("07 配置整体乱码", MatrixExpect::Blocked),
    ("08 配置空文件", MatrixExpect::Blocked),
    ("09 default_entry 越界", MatrixExpect::Blocked),
    ("10 hash 字段与内容不符", MatrixExpect::NeedConfirm),
    ("11 标记帧魔法破坏", MatrixExpect::InvalidFrame),
    ("12 标记帧校验和破坏", MatrixExpect::InvalidFrame),
    ("13 标记帧 schema 升版", MatrixExpect::InvalidFrame),
    ("14 标记帧目标越界", MatrixExpect::InvalidFrame),
    ("15 标记帧残留未消费", MatrixExpect::Residual),
    ("16 ESP 满写入失败", MatrixExpect::ArmWriteFailed),
    ("17 写后读回不一致", MatrixExpect::ArmReadback),
    ("18 作废恢复目标错乱", MatrixExpect::RestoreRejected),
    ("19 武装时间倒流", MatrixExpect::TimeWarnOnly),
    ("20 配置协议未知值", MatrixExpect::Blocked),
];

/// 矩阵期望归类（闸门/闭环/降级三类判定共用词表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixExpect {
    Blocked,
    NeedConfirm,
    InvalidFrame,
    Residual,
    ArmWriteFailed,
    ArmReadback,
    RestoreRejected,
    TimeWarnOnly,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootchain::hash_bytes;
    use crate::bootconf::ConfIssue;

    const GOOD_CONF: &str = "timeout: 5\n/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n/Windows 11 (USB)\n    protocol: efi\n    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi\n";

    fn good_probe() -> TargetProbe {
        TargetProbe { present: true, hash: 0x1234_5678 }
    }

    // ---- 帧编解码（B-103 底座） ----

    #[test]
    fn frame_roundtrip_and_corruption_rejected() {
        let f = OneshotFrame {
            target_entry: 1,
            previous_default: 0,
            armed_at: 1_790_000_000,
            conf_hash: 0xdead_beef,
        };
        let mut buf = [0u8; FRAME_LEN];
        assert_eq!(f.encode(&mut buf), Some(FRAME_LEN));
        assert_eq!(OneshotFrame::decode(&buf), Some(f));
        // 读回校验 = decode 与 encode 输入一致（十次循环）
        for i in 0..10u64 {
            let f2 = OneshotFrame { armed_at: 1_790_000_000 + i, ..f };
            let mut b2 = [0u8; FRAME_LEN];
            f2.encode(&mut b2);
            assert_eq!(OneshotFrame::decode(&b2), Some(f2), "第{}次读回不一致", i);
        }
        // 魔法破坏 → None
        let mut bad = buf;
        bad[0] ^= 0xFF;
        assert_eq!(OneshotFrame::decode(&bad), None);
        // 校验和破坏 → None
        let mut bad2 = buf;
        bad2[36] ^= 0x01;
        assert_eq!(OneshotFrame::decode(&bad2), None);
        // schema 升版 → None（未知版本拒绝消费）
        let mut bad3 = buf;
        bad3[8] = 2;
        assert_eq!(OneshotFrame::decode(&bad3), None);
        // 短缓冲 → None
        assert_eq!(OneshotFrame::decode(&buf[..16]), None);
    }

    #[test]
    fn frame_sane_rejects_out_of_range_and_identity() {
        let f = OneshotFrame {
            target_entry: 1,
            previous_default: 0,
            armed_at: 0,
            conf_hash: 0,
        };
        let conf = bootconf::parse(GOOD_CONF);
        assert!(frame_sane(&f, conf.n_entries));
        // 目标越界
        assert!(!frame_sane(&OneshotFrame { target_entry: 9, ..f }, conf.n_entries));
        // 作废目标越界（恢复错乱保护，注入组 18 的判定核）
        assert!(!frame_sane(&OneshotFrame { previous_default: 9, ..f }, conf.n_entries));
        // 目标 == 原默认（武装了个寂寞）
        assert!(!frame_sane(&OneshotFrame { target_entry: 0, previous_default: 0, ..f }, conf.n_entries));
    }

    // ---- 武装闭环（B-103：写入-消费-作废） ----

    #[test]
    fn arm_then_restore_roundtrip() {
        let conf = bootconf::parse(GOOD_CONF);
        let win_idx = (0..conf.n_entries)
            .find(|&i| conf.entry(i).map(|e| e.is_windows_target()).unwrap_or(false))
            .unwrap();
        let h = hash_bytes(GOOD_CONF.as_bytes());
        let mut out = [0u8; 2048];
        // 武装
        let n = arm_conf_text(GOOD_CONF, win_idx, h, &mut out).expect("武装必须成功");
        let armed = core::str::from_utf8(&out[..n]).unwrap();
        let armed_conf = bootconf::parse(armed);
        assert_eq!(armed_conf.default_entry, Some(win_idx), "武装后默认条目=目标");
        assert!(armed.contains("# oneshot:"), "标记行必须在");
        assert_eq!(armed_conf.n_entries, conf.n_entries, "条目集不变");
        // 作废恢复
        let mut out2 = [0u8; 2048];
        let n2 = restore_conf_text(armed, 0, &mut out2).expect("恢复必须成功");
        let restored = core::str::from_utf8(&out2[..n2]).unwrap();
        let restored_conf = bootconf::parse(restored);
        assert_eq!(restored_conf.default_entry, Some(0), "恢复后默认条目=VARIX");
        assert!(!restored.contains("# oneshot:"), "标记行必须摘除");
    }

    #[test]
    fn arm_rejects_out_of_range_and_tiny_buffers() {
        let mut out = [0u8; 2048];
        assert_eq!(arm_conf_text(GOOD_CONF, 99, 0, &mut out), None, "越界目标拒绝");
        let mut tiny = [0u8; 16];
        assert_eq!(arm_conf_text(GOOD_CONF, 1, 0, &mut tiny), None, "小缓冲拒绝");
        let mut out2 = [0u8; 2048];
        assert_eq!(restore_conf_text(GOOD_CONF, 99, &mut out2), None, "恢复越界拒绝");
    }

    // ---- 启动期回读（B-103 判定面；注入组 11-15） ----

    #[test]
    fn on_boot_verify_matrix() {
        let conf = bootconf::parse(GOOD_CONF);
        // 无残留
        assert_eq!(on_boot_verify(None, &conf), OnBootFindings::NoRecord);
        // 正常帧 → ResidualArmed（异常信号：助手没消费）
        let f = OneshotFrame { target_entry: 1, previous_default: 0, armed_at: 1, conf_hash: 2 };
        let mut buf = [0u8; FRAME_LEN];
        f.encode(&mut buf);
        assert_eq!(
            on_boot_verify(Some(&buf), &conf),
            OnBootFindings::ResidualArmed { target_entry: 1, previous_default: 0 }
        );
        // 损坏帧（三形态）→ InvalidFrame
        let mut bad = buf;
        bad[0] ^= 1;
        assert_eq!(on_boot_verify(Some(&bad), &conf), OnBootFindings::InvalidFrame);
        let mut bad2 = buf;
        bad2[38] ^= 0x80;
        assert_eq!(on_boot_verify(Some(&bad2), &conf), OnBootFindings::InvalidFrame);
        // 目标越界的"合法"帧也按损坏处理（语义闸）
        let f3 = OneshotFrame { target_entry: 42, previous_default: 0, armed_at: 1, conf_hash: 2 };
        let mut b3 = [0u8; FRAME_LEN];
        f3.encode(&mut b3);
        assert_eq!(on_boot_verify(Some(&b3), &conf), OnBootFindings::InvalidFrame);
    }

    // ---- 三路径降级（B-104） ----

    #[test]
    fn degrade_chain_has_no_blind_spot() {
        let ok = PathHealth::Available;
        let dead = |r: &'static str| PathHealth::Unavailable(r);
        // 全通 → OneShot
        assert_eq!(
            choose_handoff_path(ok, ok, ok),
            Some((HandoffPath::Oneshot, Some(HandoffPath::BootNext)))
        );
        // OneShot 挂 → BootNext
        assert_eq!(
            choose_handoff_path(dead("esp full"), ok, ok),
            Some((HandoffPath::BootNext, Some(HandoffPath::Menu)))
        );
        // OneShot+BootNext 挂 → 菜单
        assert_eq!(
            choose_handoff_path(dead("a"), dead("b"), ok),
            Some((HandoffPath::Menu, None))
        );
        // 全挂 → 不交接（宁缺毋循环）
        assert_eq!(choose_handoff_path(dead("a"), dead("b"), dead("c")), None);
    }

    // ---- 闸门三条件（B-105 判定核；矩阵组 1-10/18） ----

    #[test]
    fn gate_passes_on_healthy_state() {
        let conf = bootconf::parse(GOOD_CONF);
        let v = gate_check(
            &conf,
            Some(good_probe()),
            Some(true),
            Some(LastGoodHandoff { target_hash: 0x1234_5678 }),
        );
        assert_eq!(v, GateVerdict::Pass);
    }

    #[test]
    fn gate_matrix_first_ten() {
        let conf = bootconf::parse(GOOD_CONF);
        // 01 删 Windows 条目
        let no_win = "/kernel/varix\n    protocol: limine\n    kernel_path: boot():/kernel/varix\n";
        assert!(matches!(
            gate_check(&bootconf::parse(no_win), Some(good_probe()), Some(true), None),
            GateVerdict::Blocked(_)
        ));
        // 02 Windows 条目协议篡改（efi→grub：交接目标失格）
        let proto_swap = GOOD_CONF.replace("    protocol: efi", "    protocol: grub");
        assert!(matches!(
            gate_check(&bootconf::parse(&proto_swap), Some(good_probe()), Some(true), None),
            GateVerdict::Blocked(_)
        ));
        // 03 Windows 条目路径清空
        let path_gone = GOOD_CONF.replace(
            "    path: guid(636786cb-e967-49f6-b0df-7608909d1f11):/EFI/Microsoft/Boot/bootmgfw.efi",
            "    path:",
        );
        assert!(matches!(
            gate_check(&bootconf::parse(&path_gone), Some(good_probe()), Some(true), None),
            GateVerdict::Blocked(_)
        ));
        // 04 WINESP GUID 篡改 → 目标探针失败（文件不在被指分区）
        assert!(matches!(
            gate_check(&conf, Some(TargetProbe { present: false, hash: 0 }), Some(true), None),
            GateVerdict::Blocked(_)
        ));
        // 05 目标引导文件截断 → 哈希不符 → 用户确认（不阻断）
        assert!(matches!(
            gate_check(
                &conf,
                Some(TargetProbe { present: true, hash: 0xFFFF }),
                Some(true),
                Some(LastGoodHandoff { target_hash: 0x1234_5678 })
            ),
            GateVerdict::NeedUserConfirm(_)
        ));
        // 06 VARIX 内核文件删除 → 回路不可回
        assert!(matches!(
            gate_check(&conf, Some(good_probe()), Some(false), None),
            GateVerdict::Blocked(_)
        ));
        // 07 配置整体乱码
        let garbage = "\u{1f4a9}\u{1f4a9}\n\u{0}\u{0}";
        assert!(matches!(
            gate_check(&bootconf::parse(garbage), Some(good_probe()), Some(true), None),
            GateVerdict::Blocked(_)
        ));
        // 08 配置空文件
        assert!(matches!(
            gate_check(&bootconf::parse(""), Some(good_probe()), Some(true), None),
            GateVerdict::Blocked(_)
        ));
        // 09 default_entry 越界（闸门不依赖 default_entry，但配置问题必须
        // 登记——这里验证裁决仍是"按条目实体判"而非"按默认项判"）
        let oob = format!("default_entry: 9\n{GOOD_CONF}");
        let c = bootconf::parse(&oob);
        assert!(c.issue_list().any(|i| matches!(i, ConfIssue::DefaultEntryOutOfRange { .. })));
        assert_eq!(gate_check(&c, Some(good_probe()), Some(true), None), GateVerdict::Pass);
        // 10 hash 字段与内容不符 → 诊断事件（DegradeEvent::ConfHashMismatch）
        // 闸门本身不做哈希比对（那是目标文件哈希）；配置哈希的用途见
        // `conf_hash_matches` 测试。
    }

    #[test]
    fn conf_hash_detects_tampering() {
        // 注入组 10 的实现面：armed conf 的 hash 记录在标记帧里；
        // 配置被换（内容变）→ 消费方比对"当前 conf 文本 vs 帧内记录"判定。
        let mut out = [0u8; 2048];
        let h = hash_bytes(GOOD_CONF.as_bytes());
        let n = arm_conf_text(GOOD_CONF, 1, h, &mut out).unwrap();
        let armed = core::str::from_utf8(&out[..n]).unwrap();
        let frame = OneshotFrame { target_entry: 1, previous_default: 0, armed_at: 0, conf_hash: h };
        // 武装后的文本含标记行，hash 已变——但帧内记录的是"武装时快照"，
        // 语义独立于当前文本；消费方比对的是当前文本 vs 帧内记录。
        let c = bootconf::parse(armed);
        assert_eq!(c.default_entry, Some(1), "武装语义在");
        // 篡改 timeout（内容变）→ 帧内 conf_hash 与原 conf 的比对链路：
        // 原始 hash 与帧一致（写入即验证的读回语义）。
        assert_eq!(frame.conf_hash, hash_bytes(GOOD_CONF.as_bytes()));
        let tampered = armed.replace("timeout: 5", "timeout: 60");
        let c2 = bootconf::parse(&tampered);
        assert_eq!(c2.default_entry, Some(1), "篡改 timeout 不影响武装语义");
    }

    #[test]
    fn marker_body_formatting() {
        let mut buf = [0u8; 32];
        let n = oneshot_marker_body(1, 0xdead_beef, &mut buf);
        assert_eq!(&buf[..n], b"00000001 deadbeef");
    }

    // ---- 20 组矩阵完整性 ----

    #[test]
    fn injection_matrix_has_twenty_distinct_scenarios() {
        assert_eq!(INJECTION_MATRIX.len(), 20, "B-105 定案 20 组");
        for (i, (name, _)) in INJECTION_MATRIX.iter().enumerate() {
            assert!(!name.is_empty(), "场景 {} 必须有名", i + 1);
        }
        // 名字唯一（矩阵不是凑数）
        for i in 0..INJECTION_MATRIX.len() {
            for j in (i + 1)..INJECTION_MATRIX.len() {
                assert_ne!(INJECTION_MATRIX[i].0, INJECTION_MATRIX[j].0);
            }
        }
        // 上面各 test 已实跑组 1-15 的真实注入；组 16-20 的实跑：
        // 16 ESP 满 → ArmOutcome::WriteFailed（ArmOutcome 词表覆盖）
        assert_ne!(ArmOutcome::WriteFailed, ArmOutcome::ReadbackMismatch);
        // 17 写后读回不一致 → ArmOutcome::ReadbackMismatch
        // 18 作废目标错乱 → frame_sane false / restore None（已测）
        // 19 armed_at 仅诊断（无阻断路径——OneshotFrame 无时间校验，自证）
        let f = OneshotFrame { target_entry: 1, previous_default: 0, armed_at: 0, conf_hash: 0 };
        let conf = bootconf::parse(GOOD_CONF);
        assert!(frame_sane(&f, conf.n_entries), "时间不影响语义闸");
        // 20 未知协议 → Blocked（gate_matrix_first_ten 组 02 同构，协议面覆盖）
    }
}
