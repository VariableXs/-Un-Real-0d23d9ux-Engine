//! UNX-F10308 结果互读协议（跨域结果面 schema）（0x1b100 段）
//!
//! 锚点：UNX-F10308-J1 互读 schema——跨域结果记录逐字段名/型/序三对
//! schema，缺字段/错型/乱序名即拒读。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：schema 册（字段名→型码定序登记）+ 记录册（跨域结果记录）+
//! 解码核（记录逐字段对 schema：名序型三重校验）+ 注入核（缺字段/
//! 错型/乱名/重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b100 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b100;
pub const RS_DUPFIELD: u32 = SEG_BASE + 0x01; // 字段名重号
pub const RS_EMPTY: u32 = SEG_BASE + 0x02; // 字段名空
pub const RS_BADTYPE: u32 = SEG_BASE + 0x03; // 型码非法
pub const RS_DUPRID: u32 = SEG_BASE + 0x04; // 记录号重号
pub const RS_LEN: u32 = SEG_BASE + 0x05; // 字段数失配
pub const RS_FIELD: u32 = SEG_BASE + 0x06; // 字段名失配（含乱序）
pub const RS_TYPE: u32 = SEG_BASE + 0x07; // 字段型失配
pub const RS_NOREC: u32 = SEG_BASE + 0x08; // 未知记录读取

/// 型码：1=u32 2=str 3=bool。
pub const FT_U32: u8 = 1;
pub const FT_STR: u8 = 2;
pub const FT_BOOL: u8 = 3;

/// schema 字段定容。
pub const FIELD_CAP: usize = 8;
/// schema 字段数锚（五字段结果面）。
pub const FIELDS_NEEDED: usize = 5;
/// 记录册定容。
pub const REC_CAP: usize = 12;

/// 单个 schema 字段。
#[derive(Clone)]
pub struct FieldSpec {
    pub fname: String,
    pub tcode: u8,
}

impl FieldSpec {
    pub fn type_ok(t: u8) -> bool {
        t == FT_U32 || t == FT_STR || t == FT_BOOL
    }
}

/// 单个记录字段值。
#[derive(Clone)]
pub struct RecField {
    pub name: String,
    pub tcode: u8,
    pub ival: u32,
    pub sval: String,
}

/// 一条跨域结果记录。
#[derive(Clone)]
pub struct ResultRecord {
    pub rid: u32,
    pub vals: Vec<RecField>,
}

/// 结果互读协议核。
pub struct ResultReader {
    pub schema: Vec<FieldSpec>,
    pub records: Vec<ResultRecord>,
}

impl ResultReader {
    pub fn new() -> ResultReader {
        ResultReader { schema: Vec::new(), records: Vec::new() }
    }

    /// 登记 schema 字段（定序）。
    pub fn define(&mut self, fname: &str, tcode: u8) -> Result<(), u32> {
        if fname.is_empty() {
            return Err(RS_EMPTY);
        }
        if !FieldSpec::type_ok(tcode) {
            return Err(RS_BADTYPE);
        }
        if self.schema.iter().any(|f| f.fname == fname) {
            return Err(RS_DUPFIELD);
        }
        if self.schema.len() >= FIELD_CAP {
            return Err(RS_LEN);
        }
        self.schema.push(FieldSpec { fname: fname.to_string(), tcode });
        Ok(())
    }

    /// schema 齐备：字段数达锚且全合法。
    pub fn schema_ready(&self) -> bool {
        self.schema.len() == FIELDS_NEEDED
    }

    fn rec_pos(&self, rid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.records.len() {
            if self.records[k].rid == rid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 解码校验：记录字段数/名序/型三重对 schema。
    pub fn decode(&self, rec: &ResultRecord) -> Result<(), u32> {
        if rec.vals.len() != self.schema.len() {
            return Err(RS_LEN);
        }
        let mut k = 0usize;
        while k < self.schema.len() {
            if rec.vals[k].name != self.schema[k].fname {
                return Err(RS_FIELD);
            }
            if rec.vals[k].tcode != self.schema[k].tcode {
                return Err(RS_TYPE);
            }
            k += 1;
        }
        Ok(())
    }

    /// 提交记录：重号拒 + 解码校验过才入册。
    pub fn submit(&mut self, rec: ResultRecord) -> Result<(), u32> {
        if rec.rid == 0 || self.rec_pos(rec.rid).is_some() {
            return Err(RS_DUPRID);
        }
        if self.records.len() >= REC_CAP {
            return Err(RS_LEN);
        }
        self.decode(&rec)?;
        self.records.push(rec);
        Ok(())
    }

    /// 互读：按 rid 取记录并解码（对端读面）。
    pub fn cross_read(&self, rid: u32) -> Result<(), u32> {
        match self.rec_pos(rid) {
            None => Err(RS_NOREC),
            Some(k) => self.decode(&self.records[k]),
        }
    }

    /// 可读记录计数。
    pub fn readable_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.records.len() {
            if self.decode(&self.records[k]).is_ok() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 协议指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b1000;
        h = h.wrapping_mul(31).wrapping_add(self.schema.len() as u32);
        let mut k = 0usize;
        while k < self.schema.len() {
            h = h.wrapping_mul(31).wrapping_add(self.schema[k].tcode as u32);
            let bytes = self.schema[k].fname.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.records.len() as u32);
        let mut r = 0usize;
        while r < self.records.len() {
            h = h.wrapping_mul(31).wrapping_add(self.records[r].rid);
            r += 1;
        }
        h
    }
}

/// 按五字段 canonical schema 装配读面。
pub fn build_schema() -> ResultReader {
    let mut r = ResultReader::new();
    let _ = r.define("code", FT_U32);
    let _ = r.define("detail", FT_STR);
    let _ = r.define("green", FT_BOOL);
    let _ = r.define("ticks", FT_U32);
    let _ = r.define("tag", FT_STR);
    r
}

/// 按某 schema 拼一条合法记录（值仅占位，schema 只核名序型）。
pub fn make_record(rid: u32, schema: &ResultReader) -> ResultRecord {
    let mut vals: Vec<RecField> = Vec::new();
    let mut k = 0usize;
    while k < schema.schema.len() {
        let f = &schema.schema[k];
        vals.push(RecField {
            name: f.fname.to_string(),
            tcode: f.tcode,
            ival: k as u32,
            sval: "v".to_string(),
        });
        k += 1;
    }
    ResultRecord { rid, vals }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut r = build_schema();
    // 1) 缺字段（少一个）必拒读
    let mut short = make_record(1, &r);
    short.vals.pop();
    got[0] = r.decode(&short).err() == Some(RS_LEN);
    // 2) 错型必拒读
    let mut wrong = make_record(2, &r);
    wrong.vals[1].tcode = FT_U32;
    got[1] = r.decode(&wrong).err() == Some(RS_TYPE);
    // 3) 乱名（两字段名互换）必拒读
    let mut swap = make_record(3, &r);
    let n0 = swap.vals[0].name.to_string();
    swap.vals[0].name = swap.vals[1].name.to_string();
    swap.vals[1].name = n0;
    got[2] = r.decode(&swap).err() == Some(RS_FIELD);
    // 4) 未知记录读取必拒
    got[3] = r.cross_read(99).err() == Some(RS_NOREC);
    got
}

/// 批内判据。
pub fn run_unx10308_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10308");

    // J1 正路径：五字段 schema 齐备且合法记录入册
    let mut r = build_schema();
    let j1 = r.schema_ready()
        && r.submit(make_record(1, &r)).is_ok()
        && r.submit(make_record(2, &r)).is_ok();
    s.add("J1-schema齐备入册", j1, "五字段定序登记且两条记录解码入册");

    // J2 互读：两条记录均可跨域读取
    let j2 = r.cross_read(1).is_ok() && r.cross_read(2).is_ok();
    s.add("J2-互读命中", j2, "rid 定向互读两记录全过");

    // J3 注入检出：缺字段 + 错型
    let inj = inject_four();
    s.add("J3-缺字段错型检出", inj[0] && inj[1], "缺字段判长度失配且错型必拒");

    // J4 注入检出：乱名 + 未知记录
    s.add("J4-乱名未知检出", inj[2] && inj[3], "字段名乱序必拒且未知 rid 必拒");

    // J5 重号记录拒
    let j5 = r.submit(make_record(1, &r)).err() == Some(RS_DUPRID);
    s.add("J5-重号拒收", j5, "记录号重号必拒");

    // J6 零号记录拒
    let j6 = r.submit(make_record(0, &r)).err() == Some(RS_DUPRID);
    s.add("J6-零号拒收", j6, "零号记录按重号面拒");

    // J7 schema 字段重名拒
    let j7 = build_schema().define("code", FT_U32).err() == Some(RS_DUPFIELD);
    s.add("J7-字段重名拒", j7, "schema 字段名重号必拒");

    // J8 型码非法拒
    let j8 = ResultReader::new().define("x", 9).err() == Some(RS_BADTYPE);
    s.add("J8-型码非法拒", j8, "型码出三型域必拒");

    // J9 空字段名拒
    let j9 = ResultReader::new().define("", FT_U32).err() == Some(RS_EMPTY);
    s.add("J9-空名拒收", j9, "字段名空必拒");

    // J10 schema 未齐不许记录成批（先拒记录长度失配）
    let j10 = {
        let mut c = ResultReader::new();
        let _ = c.define("only", FT_U32);
        c.submit(make_record(1, &c)).is_err()
    };
    s.add("J10-短schema拒读", j10, "字段数不足锚时记录按长度失配拒");

    // J11 可读计数：两条记录全可读
    let j11 = r.readable_count() == 2 && r.records.len() == 2;
    s.add("J11-可读计数", j11, "册内记录全数可读");

    // J12 动态明细：协议指纹落账
    let fp = r.fold();
    s.add_dyn("J12-协议指纹", fp != 0, &format!("fold={:#x}", fp));

    // J13 动态明细：schema 字段数明细
    let sn = r.schema.len();
    s.add_dyn("J13-字段数明细", sn == FIELDS_NEEDED, &format!("fields={}", sn));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10308_schema_flow() {
        let mut r = build_schema();
        assert!(r.schema_ready());
        let rec = make_record(1, &r);
        assert!(r.decode(&rec).is_ok());
        assert!(r.submit(rec).is_ok());
        assert!(r.cross_read(1).is_ok());
        assert!(r.cross_read(9).err() == Some(RS_NOREC));
        let mut bad = make_record(2, &r);
        bad.vals[0].tcode = FT_STR;
        assert!(r.decode(&bad).err() == Some(RS_TYPE));
        bad.vals[0].tcode = FT_U32;
        bad.vals.pop();
        assert!(r.decode(&bad).err() == Some(RS_LEN));
        assert!(r.submit(make_record(0, &r)).err() == Some(RS_DUPRID));
    }

    #[test]
    fn unx10308_checks_green() {
        let s = run_unx10308_checks();
        assert!(s.all_passed(), "UNX-F10308 判据未全绿");
    }
}
