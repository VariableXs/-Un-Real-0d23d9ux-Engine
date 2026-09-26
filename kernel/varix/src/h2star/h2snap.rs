//! H2 域快照序列化层 · 深化批次一（持久化 I/O 层纵深——骨架→器官）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F253 快速访问固定**：固定状态持久化（重启验证）——固定表
//!   （顺序=拖拽序）编码落盘；
//! - **F266 后退/前进**：历史栈的会话快照（重启恢复导航位置）；
//! - **F273 文档上次位置记忆**：光标/滚动/选区三恢复精度的元数据
//!   落盘；**损坏元数据容错（读不出则从头，不报错）**——本层的
//!   解码失败返回默认态，不向上抛错（判据原文的机制保证）；
//! - **F286 壁纸多屏设置**：模式+逐屏引用的配置快照；
//! - **F298 快速设置磁贴编辑**：磁贴布局（顺序+档位）持久化；
//!   「恢复默认」= 空表解码回默认清单。
//!
//! 管道纪律：编码→校验和→经 [`h2persist::AtomicChannel`] 原子写
//! （暂存→校验→提交，断电三段注入演练在 h2persist 已证）；本层
//! 负责**格式**：版本头 + 长度前缀 + FNV-1a 校验和——读到半截/
//! 篡改数据时诚实拒绝（坏数据不装好），F273 场景调用方降级默认态。

use crate::checks::CheckSet;
use crate::h2star::h2persist::{AtomicChannel, ReadResult, fnv1a64, FORMAT_VERSION};

use alloc::string::String;
use alloc::vec::Vec;

/// 魔数（格式识别——不是本格式的一律拒绝，防串包）。
const MAGIC: &[u8; 7] = b"H2SNAP1";

// ---------------------------------------------------------------------------
// 编解码原语（小端；长度前缀一律 u32）
// ---------------------------------------------------------------------------

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        if self.pos + n > self.buf.len() {
            return Err("数据截断");
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, &'static str> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn str(&mut self) -> Result<String, &'static str> {
        let len = self.u32()? as usize;
        // 长度上限防线：异常大长度直接拒（防坏数据撑爆内存）。
        if len > 1 << 20 {
            return Err("长度越界");
        }
        let b = self.take(len)?;
        String::from_utf8(b.to_vec()).map_err(|_| "非 UTF-8")
    }
    /// 是否正好读完（尾巴有多余字节=格式不符，拒绝）。
    fn drained(&self) -> bool {
        self.pos == self.buf.len()
    }
}

/// 封包：魔数 + 版本 + 载荷长度 + 载荷 + 载荷校验和。
pub fn seal(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 23);
    out.extend_from_slice(MAGIC);
    put_u32(&mut out, FORMAT_VERSION);
    put_u32(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    put_u64(&mut out, fnv1a64(payload));
    out
}

/// 拆包：魔数/版本/长度/校验和四道关，任何一道不过都拒绝
/// （坏数据不装好——半截、篡改、串包一律 Err）。
pub fn unseal(raw: &[u8]) -> Result<Vec<u8>, &'static str> {
    if raw.len() < 23 {
        return Err("包长不足");
    }
    if &raw[..7] != MAGIC {
        return Err("魔数不符");
    }
    let mut r = Reader::new(raw);
    let _magic = r.take(7)?;
    let ver = r.u32()?;
    if ver != FORMAT_VERSION {
        return Err("版本不符");
    }
    let len = r.u32()? as usize;
    if len > 1 << 24 {
        return Err("载荷越界");
    }
    let payload = r.take(len)?.to_vec();
    let sum = r.u64()?;
    if fnv1a64(&payload) != sum {
        return Err("校验和不过");
    }
    if !r.drained() {
        return Err("包尾多余字节");
    }
    Ok(payload)
}

// ---------------------------------------------------------------------------
// F253 快速访问固定表
// ---------------------------------------------------------------------------

/// 编码固定表（顺序=拖拽序——重启后顺序还原）。
pub fn encode_pins(pinned: &[String]) -> Vec<u8> {
    let mut p = Vec::new();
    put_u32(&mut p, pinned.len() as u32);
    for s in pinned {
        put_str(&mut p, s);
    }
    seal(&p)
}

/// 解码固定表。`ok=false` 表示数据坏——调用方按「恢复默认空表」降级
/// （不向上抛错——F273 同纪律泛化到全部快照）。
pub fn decode_pins(raw: &[u8]) -> (Vec<String>, bool) {
    let payload = match unseal(raw) {
        Ok(p) => p,
        Err(_) => return (Vec::new(), false),
    };
    let mut r = Reader::new(&payload);
    let n = match r.u32() {
        Ok(n) if n as usize <= 1024 => n,
        _ => return (Vec::new(), false),
    };
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        match r.str() {
            Ok(s) => out.push(s),
            Err(_) => return (Vec::new(), false),
        }
    }
    if !r.drained() {
        return (Vec::new(), false);
    }
    (out, true)
}

// ---------------------------------------------------------------------------
// F266 导航栈快照
// ---------------------------------------------------------------------------

/// 编码导航栈（后退栈 + 当前位 + 前进栈——重启恢复三段全量）。
pub fn encode_nav(back: &[String], current: &str, fwd: &[String]) -> Vec<u8> {
    let mut p = Vec::new();
    put_u32(&mut p, back.len() as u32);
    for s in back {
        put_str(&mut p, s);
    }
    put_str(&mut p, current);
    put_u32(&mut p, fwd.len() as u32);
    for s in fwd {
        put_str(&mut p, s);
    }
    seal(&p)
}

/// 解码导航栈（坏数据 → 全 None，调用方从默认位置起栈）。
pub fn decode_nav(raw: &[u8]) -> Option<(Vec<String>, String, Vec<String>)> {
    let payload = unseal(raw).ok()?;
    let mut r = Reader::new(&payload);
    let nb = r.u32().ok()?;
    if nb as usize > 200 {
        return None;
    }
    let mut back = Vec::with_capacity(nb as usize);
    for _ in 0..nb {
        back.push(r.str().ok()?);
    }
    let current = r.str().ok()?;
    let nf = r.u32().ok()?;
    if nf as usize > 200 {
        return None;
    }
    let mut fwd = Vec::with_capacity(nf as usize);
    for _ in 0..nf {
        fwd.push(r.str().ok()?);
    }
    if !r.drained() {
        return None;
    }
    Some((back, current, fwd))
}

// ---------------------------------------------------------------------------
// F273 文档位置记忆
// ---------------------------------------------------------------------------

/// 文档位置三件套（光标偏移 / 滚动行 / 选区区间）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocPos {
    pub cursor: u64,
    pub scroll_row: u64,
    /// 选区（None=无选区）。
    pub sel: Option<(u64, u64)>,
}

pub fn encode_docpos(path: &str, pos: &DocPos) -> Vec<u8> {
    let mut p = Vec::new();
    put_str(&mut p, path);
    put_u64(&mut p, pos.cursor);
    put_u64(&mut p, pos.scroll_row);
    match pos.sel {
        None => put_u32(&mut p, 0),
        Some((a, b)) => {
            put_u32(&mut p, 1);
            put_u64(&mut p, a);
            put_u64(&mut p, b);
        }
    }
    seal(&p)
}

/// 解码文档位置。**判据「损坏元数据容错」的落点**：任何解码失败
/// 返回 None，调用方理解为「从头开始」，不报错不打扰。
pub fn decode_docpos(raw: &[u8]) -> Option<(String, DocPos)> {
    let payload = unseal(raw).ok()?;
    let mut r = Reader::new(&payload);
    let path = r.str().ok()?;
    let cursor = r.u64().ok()?;
    let scroll_row = r.u64().ok()?;
    let has_sel = r.u32().ok()?;
    let sel = match has_sel {
        0 => None,
        1 => {
            let a = r.u64().ok()?;
            let b = r.u64().ok()?;
            Some((a, b))
        }
        _ => return None,
    };
    if !r.drained() {
        return None;
    }
    Some((path, DocPos { cursor, scroll_row, sel }))
}

// ---------------------------------------------------------------------------
// F286 壁纸配置快照
// ---------------------------------------------------------------------------

/// 壁纸三模式（与 wallmulti 模块同枚举语义——此处为落盘编码位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallMode {
    PerScreen = 0,
    Splice = 1,
    Clone = 2,
}

pub fn encode_wall(mode: WallMode, screens: &[String]) -> Vec<u8> {
    let mut p = Vec::new();
    put_u32(&mut p, mode as u32);
    put_u32(&mut p, screens.len() as u32);
    for s in screens {
        put_str(&mut p, s);
    }
    seal(&p)
}

pub fn decode_wall(raw: &[u8]) -> Option<(WallMode, Vec<String>)> {
    let payload = unseal(raw).ok()?;
    let mut r = Reader::new(&payload);
    let m = r.u32().ok()?;
    let mode = match m {
        0 => WallMode::PerScreen,
        1 => WallMode::Splice,
        2 => WallMode::Clone,
        _ => return None,
    };
    let n = r.u32().ok()?;
    if n > 16 {
        return None;
    }
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        out.push(r.str().ok()?);
    }
    if !r.drained() {
        return None;
    }
    Some((mode, out))
}

// ---------------------------------------------------------------------------
// F298 磁贴布局快照
// ---------------------------------------------------------------------------

/// 编码磁贴布局：(磁贴 id, 档位 0=小 1=宽 2=中) 序列——顺序即布局序。
pub fn encode_tiles(tiles: &[(u32, u8)]) -> Vec<u8> {
    let mut p = Vec::new();
    put_u32(&mut p, tiles.len() as u32);
    for (id, span) in tiles {
        put_u32(&mut p, *id);
        p.push(*span);
    }
    seal(&p)
}

pub fn decode_tiles(raw: &[u8]) -> Option<Vec<(u32, u8)>> {
    let payload = unseal(raw).ok()?;
    let mut r = Reader::new(&payload);
    let n = r.u32().ok()?;
    if n > 256 {
        return None;
    }
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let id = r.u32().ok()?;
        let span = r.take(1).ok()?[0];
        if span > 2 {
            return None;
        }
        out.push((id, span));
    }
    if !r.drained() {
        return None;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2snap_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2snap");
    // --- 封包四道关：截断/篡改/串包/版本 全拒。 ---
    let good = seal(b"hello");
    set.add("h2snap roundtrip", unseal(&good).as_deref() == Ok(b"hello".as_slice()), "seal/unseal");
    set.add("h2snap truncated", unseal(&good[..10]).is_err(), "half pack refused");
    let mut tampered = good.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    set.add("h2snap tamper refused", unseal(&tampered).is_err(), "checksum gate");
    set.add("h2snap wrong magic", unseal(b"OTHER1234567890123456789").is_err(), "no cross-feed");
    // --- F253 固定表 round-trip + 拖拽序保持。 ---
    let pins = ["S:\\素材", "D:\\2\\14", "C:\\Users"].iter().map(|s| String::from(*s)).collect::<Vec<_>>();
    let enc = encode_pins(&pins);
    let (dec, ok) = decode_pins(&enc);
    set.add("h2snap F253 order kept", ok && dec == pins, "drag order");
    // 损坏 → 默认空表 + ok=false（不抛错）。
    let (dec_bad, ok_bad) = decode_pins(&enc[..enc.len() / 2]);
    set.add("h2snap F253 corrupt→default", !ok_bad && dec_bad.is_empty(), "graceful fallback");
    // --- F266 导航栈 round-trip。 ---
    let enc_nav = encode_nav(
        &["C:\\", "C:\\Users"].iter().map(|s| String::from(*s)).collect::<Vec<_>>(),
        "C:\\Users\\me",
        &["C:\\Users\\me\\docs"].iter().map(|s| String::from(*s)).collect::<Vec<_>>(),
    );
    let nav = decode_nav(&enc_nav);
    set.add(
        "h2snap F266 three segments",
        nav.as_ref().map(|(b, c, f)| b.len() == 2 && c == "C:\\Users\\me" && f.len() == 1).unwrap_or(false),
        "back/cur/fwd",
    );
    // --- F273 三恢复精度 + 损坏容错。 ---
    let pos = DocPos { cursor: 4_096, scroll_row: 38, sel: Some((1_024, 2_048)) };
    let enc_doc = encode_docpos("笔记.md", &pos);
    let dec_doc = decode_docpos(&enc_doc);
    set.add(
        "h2snap F273 triple restore",
        dec_doc.as_ref().map(|(p, d)| p == "笔记.md" && *d == pos).unwrap_or(false),
        "cursor/scroll/sel",
    );
    let (p_bad, d_bad) = (decode_docpos(&[]), decode_docpos(&enc_doc[..8]));
    set.add(
        "h2snap F273 corrupt honest",
        p_bad.is_none() && d_bad.is_none(),
        "read-fail→from-top",
    );
    // 无选区形制 round-trip。
    let bare = DocPos { cursor: 1, scroll_row: 0, sel: None };
    set.add(
        "h2snap F273 no-sel",
        decode_docpos(&encode_docpos("a.txt", &bare)).map(|(_, d)| d == bare).unwrap_or(false),
        "sel none",
    );
    // --- F286 壁纸配置 round-trip + 越界模式拒。 ---
    let enc_w = encode_wall(WallMode::Splice, &["S:\\wall4k.png".into()]);
    set.add(
        "h2snap F286 mode",
        decode_wall(&enc_w).map(|(m, s)| m == WallMode::Splice && s.len() == 1).unwrap_or(false),
        "splice encode",
    );
    set.add("h2snap F286 bad mode", decode_wall(&seal(&[9, 0, 0, 0])).is_none(), "enum gate");
    // --- F298 磁贴布局 round-trip + 越界档位拒。 ---
    let tiles = [(1u32, 0u8), (2, 1), (3, 2)];
    let enc_t = encode_tiles(&tiles);
    set.add(
        "h2snap F298 layout",
        decode_tiles(&enc_t).as_deref() == Some(tiles.as_slice()),
        "order+span",
    );
    set.add("h2snap F298 bad span", decode_tiles(&seal(&[1, 0, 0, 0, 9])).is_none(), "span gate");
    // --- 经 h2persist 原子写通道落盘-回读（全管道演练）。 ---
    let mut ch = AtomicChannel::new("h2.f253.pins");
    let w = ch.write(&enc, 100);
    let read_back: Option<ReadResult> = ch.read().ok();
    set.add(
        "h2snap via atomic channel",
        w == crate::h2star::h2persist::WriteOutcome::Committed
            && read_back.map(|r| decode_pins(&r.data) == (pins.clone(), true)).unwrap_or(false),
        "pipeline end-to-end",
    );
    // 断电注入：提交段中断 → 调用方按 RolledBack 语义执行回滚 →
    // 撤销位里的旧快照（pins）原样找回（先备份后修改——红线④收口）。
    let _ = ch.write(b"junk-half", 200);
    ch.fail_at = Some(crate::h2star::h2persist::FailStage::Commit);
    let outcome = ch.write(b"new-pins", 300);
    let rolled = ch.rollback();
    set.add(
        "h2snap power-cut keeps old",
        outcome == crate::h2star::h2persist::WriteOutcome::RolledBack
            && rolled
            && ch.read().map(|r| decode_pins(&r.data) == (pins, true)).unwrap_or(false),
        "rollback intact",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2snap_all_green() {
        let set = run_h2snap_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2snap 自检红 {f}/{p}");
    }

    #[test]
    fn oversize_length_refused() {
        // 长度越界防线：1MB 上限——坏数据不许撑爆内存。
        let mut p = Vec::new();
        put_u32(&mut p, u32::MAX);
        p.extend_from_slice(b"x");
        assert!(unseal(&seal(&p)).is_err() || decode_pins(&seal(&p)).1 == false);
    }

    #[test]
    fn empty_and_max_pins() {
        let (e, ok) = decode_pins(&encode_pins(&[]));
        assert!(ok && e.is_empty(), "空表也是合法快照");
        let many: Vec<String> = (0..100).map(|i| alloc::format!("p{i}")).collect();
        let (d2, ok2) = decode_pins(&encode_pins(&many));
        assert!(ok2 && d2.len() == 100);
    }
}
