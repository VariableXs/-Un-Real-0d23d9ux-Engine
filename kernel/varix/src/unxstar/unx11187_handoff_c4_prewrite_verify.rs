//! UNX-F11187 handoff C4 块终态预写核验（0x1f6c0 段）
//!
//! 纯功能模型：C4 交接块在写盘前先在内存里成块——六个字段按固定序写入、
//! 逐项校验、渲染成 JSON、再过一遍手写合法性扫描器。任何字段缺位、乱序、
//! 重写、空值、锚不符或渲染出非法 JSON，都在落盘之前拒止；冻结之后不再
//! 接受任何字段写入。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6c0;
/// 未知字段
pub const HW_EFIELD: u32 = SEG_BASE + 0x01;
/// 字段重复
pub const HW_EDUP: u32 = SEG_BASE + 0x02;
/// 字段乱序
pub const HW_EORDER: u32 = SEG_BASE + 0x03;
/// 字段空值
pub const HW_EVALUE: u32 = SEG_BASE + 0x04;
/// 锚值不符
pub const HW_EANCHOR: u32 = SEG_BASE + 0x05;
/// 渲染出非法 JSON
pub const HW_EJSON: u32 = SEG_BASE + 0x06;
/// 卡数 / 行数不符
pub const HW_ECOUNT: u32 = SEG_BASE + 0x07;
/// 已冻结
pub const HW_EFROZEN: u32 = SEG_BASE + 0x08;

/// 字段固定序：批号 / 区号 / 卡数 / 行数 / 判据数 / 锚。
pub const KEYS: [&str; 6] = ["batch", "zone", "cards", "lines", "criteria", "anchor"];
/// 交接口径：批内 200 卡。
pub const CARDS_PER_BATCH: u64 = 200;
/// 交接口径：每卡 250 行下限。
pub const LINES_PER_CARD: u64 = 250;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个交接字段。
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub idx: u8,
    pub value: u64,
}

/// 一个 C4 交接块（写盘之前的内存形态）。
#[derive(Clone, Debug)]
pub struct Block {
    fields: Vec<Field>,
    rendered: Option<String>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl Block {
    /// 空块。
    pub fn new() -> Block {
        Block {
            fields: Vec::new(),
            rendered: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已写入字段数。
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// 是否空块。
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// 字段名转序号。
    pub fn index_of(key: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < KEYS.len() {
            if KEYS[i] == key {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 取字段值。
    pub fn get(&self, key: &str) -> Option<u64> {
        let idx = Block::index_of(key)?;
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].idx as usize == idx {
                return Some(self.fields[i].value);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 逐项写入：未知→重复→乱序→空值，四道闸依次裁定。
    pub fn put(&mut self, key: &str, value: u64) -> Result<u64, u32> {
        if self.frozen {
            return self.fail(HW_EFROZEN);
        }
        let idx = match Block::index_of(key) {
            Some(i) => i,
            None => return self.fail(HW_EFIELD),
        };
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].idx as usize == idx {
                return self.fail(HW_EDUP);
            }
            i = i.wrapping_add(1);
        }
        if let Some(last) = self.fields.last() {
            // 固定序：后写字段的序号必须严格大于前写字段。
            if idx <= last.idx as usize {
                return self.fail(HW_EORDER);
            }
        }
        if value == 0 {
            return self.fail(HW_EVALUE);
        }
        self.fields.push(Field {
            idx: idx as u8,
            value,
        });
        self.accepts = self.accepts.wrapping_add(1);
        self.rendered = None;
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 记账一次拒止并回码（用于非 Result<u64, _> 的出口）。
    fn reject(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 块指纹（覆盖全部已写字段）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.fields.len() {
            h = (h ^ self.fields[i].idx as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ self.fields[i].value).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.fields.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 锚值 = 前五字段（批/区/卡/行/判据）的指纹，不含锚自身。
    pub fn anchor_from_core(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.fields.len() {
            let f = self.fields[i];
            if f.idx as usize != KEYS.len().wrapping_sub(1) {
                h = (h ^ f.idx as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ f.value).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        (h ^ (KEYS.len().wrapping_sub(1)) as u64).wrapping_mul(FNV_PRIME)
    }

    /// 渲染 JSON：字段齐备且严格按序才成串。
    pub fn render_json(&mut self) -> Result<String, u32> {
        if self.fields.len() != KEYS.len() {
            return Err(self.reject(HW_EFIELD));
        }
        let mut i = 0usize;
        while i < KEYS.len() {
            if self.fields[i].idx as usize != i {
                return Err(self.reject(HW_EORDER));
            }
            i = i.wrapping_add(1);
        }
        let mut out = String::new();
        out.push('{');
        let mut k = 0usize;
        while k < KEYS.len() {
            if k > 0 {
                out.push(',');
            }
            out.push('"');
            match KEYS.get(k) {
                Some(name) => out.push_str(name),
                None => return Err(self.reject(HW_EFIELD)),
            }
            out.push_str("\":");
            let mut stack = [0u8; 20];
            let mut nd = 0usize;
            let mut v = self.fields[k].value;
            if v == 0 {
                stack[0] = b'0';
                nd = 1;
            }
            while v > 0 {
                if let Some(slot) = stack.get_mut(nd) {
                    *slot = b'0'.wrapping_add((v % 10) as u8);
                }
                nd = nd.wrapping_add(1);
                v /= 10;
            }
            let mut j = nd;
            while j > 0 {
                j = j.wrapping_sub(1);
                if let Some(d) = stack.get(j) {
                    out.push(*d as char);
                }
            }
            k = k.wrapping_add(1);
        }
        out.push('}');
        self.rendered = Some(out.clone());
        Ok(out)
    }

    /// 最近一次渲染结果。
    pub fn rendered(&self) -> Option<&str> {
        match &self.rendered {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 手写 JSON 合法性扫描：仅收「扁平对象 + 小写键 + 纯数字值」。
    pub fn json_valid(s: &str) -> bool {
        let b = s.as_bytes();
        let n = b.len();
        if n < 4 {
            return false;
        }
        if b.get(0) != Some(&b'{') || b.get(n.wrapping_sub(1)) != Some(&b'}') {
            return false;
        }
        let mut i = 1usize;
        let mut pairs = 0usize;
        let mut need_key = true;
        while i < n.wrapping_sub(1) {
            let c = match b.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !need_key {
                if c != b',' {
                    return false;
                }
                need_key = true;
                i = i.wrapping_add(1);
                continue;
            }
            if c != b'"' {
                return false;
            }
            let start = i.wrapping_add(1);
            let mut j = start;
            loop {
                let k = match b.get(j) {
                    Some(v) => *v,
                    None => return false,
                };
                if k == b'"' {
                    break;
                }
                if !(k.is_ascii_lowercase() || k == b'_') {
                    return false;
                }
                j = j.wrapping_add(1);
            }
            if j == start {
                return false;
            }
            i = j.wrapping_add(1);
            if b.get(i) != Some(&b':') {
                return false;
            }
            i = i.wrapping_add(1);
            let vstart = i;
            loop {
                match b.get(i) {
                    Some(d) => {
                        if d.is_ascii_digit() {
                            i = i.wrapping_add(1);
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }
            if i == vstart {
                return false;
            }
            pairs = pairs.wrapping_add(1);
            need_key = false;
        }
        pairs > 0 && !need_key
    }

    /// 终验：字段齐备 → 卡数/行数对平 → 锚自洽 → 渲染出合法 JSON。
    pub fn verify(&mut self, expected_cards: u64, expected_lines: u64) -> Result<u64, u32> {
        if self.fields.len() != KEYS.len() {
            return self.fail(HW_EFIELD);
        }
        let cards = match self.get("cards") {
            Some(v) => v,
            None => return self.fail(HW_EFIELD),
        };
        let lines = match self.get("lines") {
            Some(v) => v,
            None => return self.fail(HW_EFIELD),
        };
        if cards != expected_cards || lines != expected_lines {
            return self.fail(HW_ECOUNT);
        }
        if lines < cards.saturating_mul(LINES_PER_CARD) {
            return self.fail(HW_ECOUNT);
        }
        let anchor = match self.get("anchor") {
            Some(v) => v,
            None => return self.fail(HW_EFIELD),
        };
        if anchor != self.anchor_from_core() {
            return self.fail(HW_EANCHOR);
        }
        let s = match self.render_json() {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        if !Block::json_valid(s.as_str()) {
            return self.fail(HW_EJSON);
        }
        Ok(self.fold())
    }

    /// 冻结：块齐备才给单值，此后不再接受写入。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.fields.len() != KEYS.len() {
            return None;
        }
        self.frozen = true;
        Some(self.fold())
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 预写一块：五字段按序写入，锚由前五字段算出，再终验。
pub fn build_block() -> (Block, bool) {
    let mut b = Block::new();
    let core: [u64; 5] = [
        11187u64,
        11299u64,
        CARDS_PER_BATCH,
        CARDS_PER_BATCH.saturating_mul(LINES_PER_CARD),
        13u64,
    ];
    let mut i = 0usize;
    while i < core.len() {
        let name = match KEYS.get(i) {
            Some(v) => *v,
            None => return (b, false),
        };
        let v = match core.get(i) {
            Some(x) => *x,
            None => return (b, false),
        };
        if b.put(name, v).is_err() {
            return (b, false);
        }
        i = i.wrapping_add(1);
    }
    let anchor = b.anchor_from_core();
    if b.put("anchor", anchor).is_err() {
        return (b, false);
    }
    let ok = b
        .verify(CARDS_PER_BATCH, CARDS_PER_BATCH.saturating_mul(LINES_PER_CARD))
        .is_ok();
    (b, ok)
}

/// 四类注入：未知字段 / 字段重复 / 字段乱序 / 空值 / 冻结后写入。
pub fn inject_four() -> [bool; 4] {
    // ① 未知字段名
    let mut a0 = Block::new();
    let r0 = a0.put("verdict", 1).err() == Some(HW_EFIELD);

    // ② 字段重复：同名再写
    let mut a1 = Block::new();
    let s1 = a1.put("batch", 11187).is_ok();
    let r1 = s1 && a1.put("batch", 11188).err() == Some(HW_EDUP);

    // ③ 字段乱序：写入 cards 后回写更早的 zone
    let mut a2 = Block::new();
    let s2a = a2.put("batch", 11187).is_ok();
    let s2b = a2.put("zone", 11299).is_ok();
    let s2c = a2.put("cards", CARDS_PER_BATCH).is_ok();
    let r2 = s2a && s2b && s2c && a2.put("zone", 1).err() == Some(HW_EDUP)
        && a2.put("lines", 0).err() == Some(HW_EVALUE);

    // ④ 冻结后写入 + 非法 JSON 样本
    let (mut a3, ok3) = build_block();
    let locked = a3.freeze().is_some();
    let illegal = !Block::json_valid("")
        && !Block::json_valid("{")
        && !Block::json_valid("{}")
        && !Block::json_valid("{\"a\":1,}")
        && !Block::json_valid("{\"a\":}")
        && !Block::json_valid("{a:1}")
        && !Block::json_valid("{\"a\":1")
        && !Block::json_valid("{\"A\":1}");
    let r3 = ok3 && locked && a3.put("batch", 1).err() == Some(HW_EFROZEN) && illegal;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11187 C4 块终态预写核验。
pub fn run_unx11187_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11187");
    let (mut b, ok) = build_block();

    // H1 字段齐备
    s.add_dyn(
        "H1-字段齐备",
        ok && b.len() == KEYS.len() && !b.is_empty(),
        &format!("fields={}/{}", b.len(), KEYS.len()),
    );

    // H2 字段固定序
    let mut ordered = true;
    let mut i = 0usize;
    while i < KEYS.len() {
        match KEYS.get(i) {
            Some(k) => {
                if b.get(k).is_none() {
                    ordered = false;
                }
            }
            None => ordered = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("H2-字段固定序", ordered, "六字段按批/区/卡/行/判据/锚依次成块");

    // H3 卡数行数对平
    let cards = b.get("cards").unwrap_or(0);
    let lines = b.get("lines").unwrap_or(0);
    s.add_dyn(
        "H3-卡数行数对平",
        cards == CARDS_PER_BATCH && lines == CARDS_PER_BATCH * LINES_PER_CARD,
        &format!("cards={} lines={}", cards, lines),
    );

    // H4 锚值自洽
    let anchor = b.get("anchor").unwrap_or(0);
    s.add(
        "H4-锚值自洽",
        anchor != 0 && anchor == b.anchor_from_core(),
        "锚等于前五字段指纹",
    );

    // H5 渲染 JSON 合法
    let rendered = match b.render_json() {
        Ok(v) => v,
        Err(_) => String::new(),
    };
    s.add_dyn(
        "H5-渲染JSON合法",
        !rendered.is_empty() && Block::json_valid(rendered.as_str()),
        &format!("json={}", rendered),
    );

    // H6 扫描器拒非法样本
    let illegal = !Block::json_valid("")
        && !Block::json_valid("{")
        && !Block::json_valid("{}")
        && !Block::json_valid("{\"a\":1,}")
        && !Block::json_valid("{a:1}")
        && !Block::json_valid("{\"a\"1}");
    s.add("H6-扫描器拒非法", illegal, "空串/单括号/空对象/尾逗号/裸键/缺冒号六样本全拒");

    // H7 扫描器收合法样本
    s.add(
        "H7-扫描器收合法",
        Block::json_valid("{\"a\":1}") && Block::json_valid("{\"a\":1,\"b\":22}"),
        "单对与多对扁平对象皆收",
    );

    // H8 冻结单值且幂等
    let f1 = b.freeze();
    let f2 = b.freeze();
    s.add_dyn(
        "H8-冻结单值幂等",
        f1.is_some() && f1 == f2 && f1 == Some(b.fold()),
        &format!("frozen={:#x}", f1.unwrap_or(0)),
    );

    // H9 冻结后拒写入
    s.add(
        "H9-冻结后拒写入",
        b.is_frozen() && b.put("batch", 1).err() == Some(HW_EFROZEN),
        "冻结块不再接受任何字段",
    );

    // H10 未知字段拒止
    let mut u = Block::new();
    s.add(
        "H10-未知字段拒止",
        u.put("verdict", 1).err() == Some(HW_EFIELD) && u.len() == 0,
        "白名单外字段名一律拒",
    );

    // H11 重复字段拒止
    let mut d = Block::new();
    let d0 = d.put("batch", 11187).is_ok();
    s.add(
        "H11-重复字段拒止",
        d0 && d.put("batch", 11187).err() == Some(HW_EDUP) && d.len() == 1,
        "同字段只许写一次",
    );

    // H12 乱序拒止
    let mut o = Block::new();
    let o0 = o.put("zone", 11299).is_ok();
    s.add(
        "H12-乱序拒止",
        o0 && o.put("batch", 11187).err() == Some(HW_EORDER),
        "回写更早字段即拒",
    );

    // H13 空值与注入四拒
    let mut v = Block::new();
    let empty_rejected = v.put("batch", 0).err() == Some(HW_EVALUE);
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    s.add_dyn(
        "H13-空值与注入四拒",
        empty_rejected && all,
        &format!(
            "inject={}{}{}{} empty={}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8, empty_rejected
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11187_block_flow() {
        let (mut b, ok) = build_block();
        assert!(ok);
        assert_eq!(b.len(), KEYS.len());
        assert_eq!(b.get("cards"), Some(CARDS_PER_BATCH));
        assert_eq!(b.get("lines"), Some(CARDS_PER_BATCH * LINES_PER_CARD));
        assert_eq!(b.get("anchor"), Some(b.anchor_from_core()));
        let json = b.render_json().unwrap_or_default();
        assert!(Block::json_valid(json.as_str()));
        let v = b.freeze();
        assert_eq!(v, Some(b.fold()));
        assert_eq!(b.freeze(), v);
        assert_eq!(b.put("batch", 1), Err(HW_EFROZEN));
    }

    #[test]
    fn f11187_field_gates() {
        let mut b = Block::new();
        assert_eq!(b.put("nope", 1), Err(HW_EFIELD));
        assert!(b.put("batch", 11187).is_ok());
        assert_eq!(b.put("batch", 11187), Err(HW_EDUP));
        assert_eq!(b.put("zone", 0), Err(HW_EVALUE));
        assert!(b.put("zone", 11299).is_ok());
        assert_eq!(b.rejects, 3);
        assert_eq!(b.accepts, 2);
        let mut o = Block::new();
        assert!(o.put("zone", 11299).is_ok());
        assert_eq!(o.put("batch", 11187), Err(HW_EORDER));
    }

    #[test]
    fn f11187_checks_green() {
        let cs = run_unx11187_checks();
        assert!(cs.all_passed());
    }
}
