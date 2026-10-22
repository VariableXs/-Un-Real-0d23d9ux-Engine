//! v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
//! ===========================================================================
//! 主册锚 F244（全局快捷键集中注册表）。v2 三件事：
//! 1) 持久化 I/O：用户差异定长快照册（改键/禁用条目 → uid+键位+启用位）
//!    v2 容器序列化——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验
//!    和，四类损坏显性拒绝（与既有 VXHK 变长差异面并存于追加段）；
//! 2) UI 壳接线：设置中心快捷键页行清单（combo_str 显示串 + 启用/改键
//!    徽标位）+ 行命中测试 + 键盘遍历；
//! 3) 判定面扩展：run_hotkeyreg_v2_checks，首条即持久化 round-trip。

use crate::checks::CheckSet;

use super::checks::xors32;
use super::keys::{Combo, MOD_CTRL, MOD_SHIFT, combo_str};
use super::registry::{HK_CAP, HotkeyReg, RebindOutcome, fnv1a, uid_of_label};

// -- 持久化 I/O 面 ---------------------------------------------------------

/// 差异快照册容量（改键/禁用条目定容 12）。
pub const VX2_HK_SNAP_CAP: usize = 12;
/// v2 容器 payload 定长：条数 u32 + 12 槽×8B（uid u32 + mods + key u16 + flags）。
pub const VX2_HK_PAYLOAD: usize = 4 + VX2_HK_SNAP_CAP * 8;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_HK_BLOB: usize = 9 + VX2_HK_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或条数越出 payload 容量。
    BadLength,
    BadChecksum,
}

/// 一条差异快照（改键/禁用条目；uid 与既有持久化域一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HkSnap {
    pub uid: u32,
    pub mods: u8,
    pub key: u16,
    pub enabled: bool,
}

/// 用户差异定长快照册（启动装载面的 v2 承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HkSnapBook {
    pub entries: [Option<HkSnap>; VX2_HK_SNAP_CAP],
}

impl HkSnapBook {
    pub const fn new() -> HkSnapBook {
        HkSnapBook { entries: [None; VX2_HK_SNAP_CAP] }
    }

    /// 从注册表读出差异（overridden 或 disabled 的条目；超出容量显性
    /// 拒绝返回 None——不静默截断）。
    pub fn snapshot(reg: &HotkeyReg) -> Option<HkSnapBook> {
        let mut book = HkSnapBook::new();
        let mut n = 0usize;
        for i in 0..HK_CAP {
            // let-else + 卫语句前移（原嵌套 4 层压至 2 层，判定顺序
            // 等价：非差异条目跳过、超容显性拒绝、写入计数）。
            let Some(e) = reg.slot_at(i) else { continue };
            if !(e.overridden || !e.enabled) {
                continue;
            }
            if n >= VX2_HK_SNAP_CAP {
                return None;
            }
            book.entries[n] = Some(HkSnap { uid: e.uid(), mods: e.combo.mods, key: e.combo.key, enabled: e.enabled });
            n += 1;
        }
        Some(book)
    }

    /// 推到注册表：逐条走既有 rebind/set_enabled 正规门（冲突/未知 uid
    /// 如实跳过——返回实际生效条数，不静默吞差异）。
    pub fn apply_to(&self, reg: &mut HotkeyReg, ts: u64) -> usize {
        let mut applied = 0;
        for s in self.entries.iter().flatten() {
            let combo = Combo::new(s.mods, s.key);
            if !combo.sane() {
                continue;
            }
            if reg.rebind(s.uid, combo, ts) == RebindOutcome::Done && reg.set_enabled(s.uid, s.enabled, ts) {
                applied += 1;
            }
        }
        applied
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_HK_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        let mut n = 0usize;
        for s in self.entries.iter().flatten() {
            let p = 9 + n * 8;
            out[p..p + 4].copy_from_slice(&s.uid.to_le_bytes());
            out[p + 4] = s.mods;
            out[p + 5..p + 7].copy_from_slice(&s.key.to_le_bytes());
            // 修障登记（F244v2 check 1 红）：flags 必须落在 p+7——此前误写
            // p+6，与 key u16 的高位字节重叠，enabled=1 会把 key=0x43 解码
            // 成 0x143（round-trip 字段不等，装载面改键到幽灵键位）。
            out[p + 7] = s.enabled as u8;
            out[p + 6] = 0;
            n += 1;
        }
        out[5..9].copy_from_slice(&(n as u32).to_le_bytes());
        let end = 9 + VX2_HK_PAYLOAD;
        let crc = fnv1a(&out[..end - 4]);
        out[end - 4..end].copy_from_slice(&crc.to_le_bytes());
        VX2_HK_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<HkSnapBook, Vx2Error> {
        if blob.len() != VX2_HK_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_HK_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if fnv1a(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        let n = u32::from_le_bytes([blob[5], blob[6], blob[7], blob[8]]) as usize;
        if n > VX2_HK_SNAP_CAP {
            return Err(Vx2Error::BadLength);
        }
        let mut book = HkSnapBook::new();
        for k in 0..n {
            let p = 9 + k * 8;
            book.entries[k] = Some(HkSnap {
                uid: u32::from_le_bytes([blob[p], blob[p + 1], blob[p + 2], blob[p + 3]]),
                mods: blob[p + 4],
                key: u16::from_le_bytes([blob[p + 5], blob[p + 6]]),
                enabled: blob[p + 7] & 1 == 1,
            });
        }
        Ok(book)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 快捷键页行高（px）——v2 布局常量：F244 设置页行 28px。
pub const VX2_ROW_H_PX: i32 = 28;
/// 显示串缓冲（与 combo_str 截断纪律一致）。
pub const VX2_TEXT_CAP: usize = 24;
/// 键盘遍历键码（VK_UP/VK_DOWN/VK_RETURN 同码）。
pub const VX2_KEY_UP: u8 = 0x26;
pub const VX2_KEY_DOWN: u8 = 0x27;
pub const VX2_KEY_ENTER: u8 = 0x0D;

/// 快捷键页行绘制条目：显示串 + 行矩形 + 启用/改键徽标位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HkRow {
    pub uid: u32,
    pub label: &'static str,
    pub y: i32,
    pub h: i32,
    pub enabled: bool,
    pub overridden: bool,
    pub text_len: usize,
    pub text: [u8; VX2_TEXT_CAP],
}

/// 生成快捷键页行清单（槽序 = 表序；显示串直取 combo_str——显示面
/// 唯一事实源，不另拼键名）。
pub fn hk_rows(reg: &HotkeyReg, out: &mut [HkRow]) -> usize {
    let mut k = 0usize;
    for i in 0..HK_CAP {
        if k >= out.len() {
            break;
        }
        // let-else 展平（原 for > if > if let 嵌套 3 层 → 2 层）。
        let Some(e) = reg.slot_at(i) else { continue };
        let mut row = HkRow {
            uid: e.uid(),
            label: e.label,
            y: k as i32 * VX2_ROW_H_PX,
            h: VX2_ROW_H_PX,
            enabled: e.enabled,
            overridden: e.overridden,
            text_len: 0,
            text: [0u8; VX2_TEXT_CAP],
        };
        row.text_len = combo_str(e.combo, &mut row.text);
        out[k] = row;
        k += 1;
    }
    k
}

/// 行命中测试（列表内坐标；x ∈ [0, w) 且落在行内）。
pub fn hk_row_hit(rows: &[HkRow], n: usize, px: i32, py: i32, w: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| px >= 0 && px < w && py >= rows[k].y && py < rows[k].y + rows[k].h)
}

/// 键盘遍历结论：不动 / 移高亮（夹取）/ 激活当前行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListNav {
    Stay,
    Moved(usize),
    Activate(usize),
}

/// 键盘遍历：Up/Down 移高亮（首末行夹取）、Enter 激活。
pub fn hk_list_nav(hl: usize, n: usize, key: u8) -> ListNav {
    if n == 0 {
        return ListNav::Stay;
    }
    match key {
        VX2_KEY_UP => ListNav::Moved(hl.saturating_sub(1)),
        VX2_KEY_DOWN => ListNav::Moved((hl + 1).min(n - 1)),
        VX2_KEY_ENTER => ListNav::Activate(hl.min(n - 1)),
        _ => ListNav::Stay,
    }
}

// -- 判定面扩展 ------------------------------------------------------------

/// v2 判定组间上下文（私有结构——收敛判定 1 产物在判定 2~5 间的传参：
/// 源表 / 差异快照册 / 序列化缓冲 / 缓冲有效长度）。
struct V2Ctx {
    src: HotkeyReg,
    book: HkSnapBook,
    buf: [u8; VX2_HK_BLOB],
    len: usize,
}

/// v2 判定 1：持久化 round-trip（差异快照编→解→推新表→改键与禁用逐条
/// 一致）。快照构造失败 = 判定面前置不成立：五条判定全数显性记红并
/// 返回 None（可恢复路径，不 panic、不静默跳过）。
fn v2_chk_roundtrip(set: &mut CheckSet) -> Option<V2Ctx> {
    let mut src = HotkeyReg::new();
    let _ = src.preset_all(100);
    let _ = src.rebind(uid_of_label("sys.copy"), Combo::new(MOD_CTRL | MOD_SHIFT, 0x43), 200);
    let _ = src.set_enabled(uid_of_label("sys.undo"), false, 300);
    let book = match HkSnapBook::snapshot(&src) {
        Some(b) => b,
        None => {
            set.add("v2 persistence round-trip", false, "");
            set.add("v2 corruption explicitly rejected", false, "");
            set.add("v2 apply respects conflict gate", false, "");
            set.add("v2 rows display & nav", false, "");
            set.add("v2 fuzz 500 round-trips & checksum", false, "");
            return None;
        }
    };
    let mut buf = [0u8; VX2_HK_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut dst = HotkeyReg::new();
    let _ = dst.preset_all(400);
    match HkSnapBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            let applied = b2.apply_to(&mut dst, 500);
            let copy_ok = dst
                .entry_by_uid(uid_of_label("sys.copy"))
                .map_or(false, |e| e.combo == Combo::new(MOD_CTRL | MOD_SHIFT, 0x43) && e.overridden);
            let undo_ok = dst.entry_by_uid(uid_of_label("sys.undo")).map_or(false, |e| !e.enabled);
            set.add(
                "v2 persistence round-trip",
                b2 == book && applied == 2 && copy_ok && undo_ok && dst.audit_conflict_free(),
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }
    Some(V2Ctx { src, book, buf, len })
}

/// v2 判定 2~3：四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）
/// + 冲突门复核（快照键位撞已占用条目时 apply 显性跳过——先到先得
/// 同样适用于装载序，与 import_overrides 同纪律）。
fn v2_chk_corruption_and_gate(set: &mut CheckSet, ctx: &V2Ctx) {
    let mut m = ctx.buf;
    m[0] = b'X';
    let mut v = ctx.buf;
    v[4] = 9;
    let mut c = ctx.buf;
    c[12] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        HkSnapBook::from_bytes(&ctx.buf[..ctx.len - 1]) == Err(Vx2Error::BadLength)
            && HkSnapBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && HkSnapBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && HkSnapBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    let mut hostile = HotkeyReg::new();
    let _ = hostile.preset_all(600);
    let _ = hostile.rebind(uid_of_label("sys.paste"), Combo::new(MOD_CTRL | MOD_SHIFT, 0x43), 700);
    let skipped = ctx.book.apply_to(&mut hostile, 800);
    set.add(
        "v2 apply respects conflict gate",
        skipped == 1
            && hostile.entry_by_uid(uid_of_label("sys.copy")).map_or(false, |e| e.combo == Combo::new(MOD_CTRL, 0x43))
            && hostile.audit_conflict_free(),
        "",
    );
}

/// v2 fuzz 主体（判定 5：500 轮随机差异册 round-trip 逐字段相等 +
/// payload 任一字节翻位必被校验和捕获）。步进统一走 xors32 公共面。
/// 返回全部轮次是否通过。
fn v2_fuzz_roundtrip_500() -> bool {
    let mut x: u32 = 0x244A_11CC;
    let mut ok = true;
    for _ in 0..500u32 {
        xors32(&mut x);
        let mut b = HkSnapBook::new();
        for k in 0..((x >> 3) % 13) {
            xors32(&mut x);
            b.entries[k as usize] = Some(HkSnap {
                uid: x,
                mods: (x % 16) as u8,
                key: (x % 0x100) as u16,
                enabled: x & 1 == 1,
            });
        }
        let mut tbuf = [0u8; VX2_HK_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_HK_BLOB && HkSnapBook::from_bytes(&tbuf) == Ok(b);
        xors32(&mut x);
        tbuf[5 + (x as usize) % VX2_HK_PAYLOAD] ^= 0x20;
        ok &= HkSnapBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
    }
    ok
}

/// v2 判定 4~5：页面行清单（显示串/徽标/命中测试/键盘遍历夹取）+
/// fuzz 500 轮 round-trip 与校验和。
fn v2_chk_rows_and_fuzz(set: &mut CheckSet, ctx: &V2Ctx) {
    let mut rows = [HkRow { uid: 0, label: "", y: 0, h: 0, enabled: false, overridden: false, text_len: 0, text: [0u8; VX2_TEXT_CAP] }; 48];
    let rn = hk_rows(&ctx.src, &mut rows);
    // 修障登记（v2 check 4 红）：hk_rows 按哈希槽序输出（表序=槽序是
    // 文档明示的契约），sys.copy 未必落首槽、甚至未必落在草稿的 8 行
    // 视窗内——行清单扩到全量并按 label 定位目标行；显示串、徽标、
    // 命中、遍历与首末夹取判据全部保留。
    let copy_row = (0..rn).find(|&k| rows[k].label == "sys.copy");
    let t0 = copy_row.map(|k| core::str::from_utf8(&rows[k].text[..rows[k].text_len]).unwrap_or("?"));
    // 改键徽标判定按 Option 显性展开——不再用 unwrap_or(0) 兜底索引
    // （裸 unwrap 消灭岗位：缺失即显性 false，判定记红可观测）。
    let copy_overridden = copy_row.map_or(false, |k| rows[k].overridden);
    let mut hl = 0usize;
    for _ in 0..9 {
        if let ListNav::Moved(k) = hk_list_nav(hl, rn, VX2_KEY_DOWN) {
            hl = k;
        }
    }
    set.add(
        "v2 rows display & nav",
        rn == 46
            && copy_row.is_some()
            && t0 == Some("Ctrl+Shift+C")
            && copy_overridden
            && hk_row_hit(&rows, rn, 40, VX2_ROW_H_PX + 5, 600) == Some(1)
            && hk_row_hit(&rows, rn, 40, -3, 600).is_none()
            && hl == 9
            && hk_list_nav(rn - 1, rn, VX2_KEY_DOWN) == ListNav::Moved(rn - 1)
            && hk_list_nav(0, 1, VX2_KEY_ENTER) == ListNav::Activate(0),
        "",
    );
    set.add("v2 fuzz 500 round-trips & checksum", v2_fuzz_roundtrip_500(), "");
}

/// F244 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。判定按组
/// 拆分：round-trip / 损坏拒绝与冲突门 / 行清单与 fuzz——各单一职责，
/// 组间产物经 V2Ctx 显性传递。
pub fn run_hotkeyreg_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F244-hotkeyreg-v2");
    let Some(ctx) = v2_chk_roundtrip(&mut set) else {
        return set;
    };
    v2_chk_corruption_and_gate(&mut set, &ctx);
    v2_chk_rows_and_fuzz(&mut set, &ctx);
    set
}
