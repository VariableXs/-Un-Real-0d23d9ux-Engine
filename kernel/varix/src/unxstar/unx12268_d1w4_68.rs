//! UNX-F12268 NtDeviceIoControlFile/NtFsControlFile 预告（0x64880 段）
//!
//! 卡面：控制码分派语义——CTL_CODE 四字段（设备/访问/功能/方法）解析、
//! 设备闭集与访问位闭集校验、方法域闭集；分派账册按控制码记账，重号
//! 拒止、印账冻结。DeviceIoControl 与 FsControl 的差别仅在第 16 位设备
//! 基域，分派模型共用同一张账。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 四字段解析：编码→字段→再编码须原值往返。
//! - J2 设备闭集：设备号不在已知设备集合必拒。
//! - J3 方法闭集：方法两位（0~3），其余必拒。
//! - J4 访问闭集：访问两位（0~3），其余必拒。
//! - J5 缓冲方法记账：Buffered/Direct/Neither 三类可回读。
//! - J6 重号分派必拒：同控制码不得重复入册。
//! - J7 印账冻结：seal 后分派必拒。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64880 独占 0x20）。
pub const SEG_BASE: u32 = 0x64880;
pub const DC_BAD_DEVICE: u32 = SEG_BASE + 0x01; // 设备越闭集
pub const DC_BAD_METHOD: u32 = SEG_BASE + 0x02; // 方法越闭集
pub const DC_BAD_ACCESS: u32 = SEG_BASE + 0x03; // 访问越闭集
pub const DC_DUP: u32 = SEG_BASE + 0x04; // 重号分派
pub const DC_FROZEN: u32 = SEG_BASE + 0x05; // 印账后分派

/// 设备闭集（模型内已知设备面）。
pub const DEV_DISK_FS: u16 = 0x08;
pub const DEV_CDROM_FS: u16 = 0x09;
pub const DEV_NETWORK_FS: u16 = 0x12;
pub const DEV_NAMED_PIPE: u16 = 0x11;
pub const DEV_MAILSLOT: u16 = 0x21;

/// 方法域（2 位）。
pub const METHOD_BUFFERED: u8 = 0;
pub const METHOD_IN_DIRECT: u8 = 1;
pub const METHOD_OUT_DIRECT: u8 = 2;
pub const METHOD_NEITHER: u8 = 3;

/// 访问域（2 位）。
pub const FILE_ANY_ACCESS: u8 = 0;
pub const FILE_READ_ACCESS: u8 = 1;
pub const FILE_WRITE_ACCESS: u8 = 2;

fn device_known(d: u16) -> bool {
    matches!(
        d,
        DEV_DISK_FS | DEV_CDROM_FS | DEV_NETWORK_FS | DEV_NAMED_PIPE | DEV_MAILSLOT
    )
}

/// 控制码四字段档。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CtlCode {
    pub device: u16,
    pub function: u16, // 12 位
    pub access: u8,    // 2 位
    pub method: u8,    // 2 位
}

impl CtlCode {
    pub fn encode(&self) -> u32 {
        ((self.device as u32) << 16)
            | ((self.access as u32) << 14)
            | ((self.function as u32) << 2)
            | (self.method as u32)
    }

    pub fn decode(raw: u32) -> CtlCode {
        CtlCode {
            device: (raw >> 16) as u16,
            function: ((raw >> 2) & 0x0FFF) as u16,
            access: ((raw >> 14) & 0x3) as u8,
            method: (raw & 0x3) as u8,
        }
    }

    /// 校验核：设备闭集 + 功能非零 + 方法/访问闭集。
    pub fn validate(&self) -> Result<(), u32> {
        if !device_known(self.device) {
            return Err(DC_BAD_DEVICE);
        }
        if self.function == 0 {
            return Err(DC_BAD_DEVICE);
        }
        if self.method > METHOD_NEITHER {
            return Err(DC_BAD_METHOD);
        }
        if self.access > FILE_WRITE_ACCESS {
            return Err(DC_BAD_ACCESS);
        }
        Ok(())
    }

    /// 缓冲方法记账名。
    pub fn method_name(&self) -> String {
        match self.method {
            METHOD_BUFFERED => "buffered".to_string(),
            METHOD_IN_DIRECT => "in_direct".to_string(),
            METHOD_OUT_DIRECT => "out_direct".to_string(),
            _ => "neither".to_string(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "ctl|dev{:x}|fn{}|acc{}|m{}",
            self.device, self.function, self.access, self.method
        )
    }
}

/// 分派账册。
pub struct DispatchBook {
    pub recs: Vec<CtlCode>,
    pub sealed: bool,
}

impl DispatchBook {
    pub fn new() -> DispatchBook {
        DispatchBook { recs: Vec::new(), sealed: false }
    }

    pub fn dispatch(&mut self, code: CtlCode) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_FROZEN);
        }
        code.validate()?;
        if self.recs.iter().any(|r| r.encode() == code.encode()) {
            return Err(DC_DUP);
        }
        self.recs.push(code);
        Ok(())
    }

    /// 按原始码查询。
    pub fn by_raw(&self, raw: u32) -> Option<&CtlCode> {
        self.recs.iter().find(|r| r.encode() == raw)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.encode());
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("codes={}", self.recs.len())
    }

    pub fn line_of(&self, raw: u32) -> String {
        match self.by_raw(raw) {
            Some(c) => c.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DispatchBook::new();
    let mk = |dev: u16, function: u16, access: u8, method: u8| CtlCode {
        device: dev,
        function,
        access,
        method,
    };
    // 1) 未知设备必拒。
    got[0] = b.dispatch(mk(0x77, 1, FILE_ANY_ACCESS, METHOD_BUFFERED))
        .err() == Some(DC_BAD_DEVICE);
    // 2) 零功能码必拒。
    got[1] = b.dispatch(mk(DEV_DISK_FS, 0, FILE_ANY_ACCESS, METHOD_BUFFERED))
        .err() == Some(DC_BAD_DEVICE);
    // 3) 越方法必拒。
    got[2] = b.dispatch(mk(DEV_DISK_FS, 2, FILE_ANY_ACCESS, 7))
        .err() == Some(DC_BAD_METHOD);
    // 4) 越访问必拒。
    got[3] = b.dispatch(mk(DEV_DISK_FS, 2, 5, METHOD_BUFFERED))
        .err() == Some(DC_BAD_ACCESS);
    // 5) 印账后分派必抓。
    let _ = b.dispatch(mk(DEV_DISK_FS, 2, FILE_ANY_ACCESS, METHOD_BUFFERED));
    b.seal();
    got[4] = b.dispatch(mk(DEV_DISK_FS, 3, FILE_ANY_ACCESS, METHOD_BUFFERED))
        .err() == Some(DC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12268_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12268");

    // J1 四字段往返。
    let j1 = {
        let c = CtlCode {
            device: DEV_DISK_FS,
            function: 0x123,
            access: FILE_WRITE_ACCESS,
            method: METHOD_IN_DIRECT,
        };
        CtlCode::decode(c.encode()) == c
    };
    s.add("J1-四字段往返", j1, "编码解码原值");

    // J2 设备闭集。
    let j2 = {
        let bad = CtlCode {
            device: 0x99,
            function: 1,
            access: 0,
            method: 0,
        };
        let ok = CtlCode {
            device: DEV_NETWORK_FS,
            function: 1,
            access: 0,
            method: 0,
        };
        bad.validate().err() == Some(DC_BAD_DEVICE) && ok.validate().is_ok()
    };
    s.add("J2-设备闭集", j2, "未知设备拒");

    // J3 方法闭集。
    let j3 = {
        let mk = |m: u8| CtlCode {
            device: DEV_NAMED_PIPE,
            function: 4,
            access: 0,
            method: m,
        };
        (0..=3).all(|m| mk(m).validate().is_ok()) && mk(4).validate().is_err()
    };
    s.add("J3-方法闭集", j3, "0..3 合法、4 拒");

    // J4 访问闭集。
    let j4 = {
        let mk = |a: u8| CtlCode {
            device: DEV_MAILSLOT,
            function: 2,
            access: a,
            method: 0,
        };
        (0..=2).all(|a| mk(a).validate().is_ok()) && mk(3).validate().is_err()
    };
    s.add("J4-访问闭集", j4, "any/read/write 合法、3 拒");

    // J5 缓冲方法记账。
    let j5 = {
        let c = CtlCode {
            device: DEV_CDROM_FS,
            function: 9,
            access: 0,
            method: METHOD_OUT_DIRECT,
        };
        c.method_name() == "out_direct" && c.render().contains("m2")
    };
    s.add("J5-方法记账", j5, "方法名可回读");

    // J6 重号分派必拒。
    let j6 = {
        let mut b = DispatchBook::new();
        let c = CtlCode {
            device: DEV_DISK_FS,
            function: 1,
            access: 0,
            method: 0,
        };
        let _ = b.dispatch(c);
        b.dispatch(c).err() == Some(DC_DUP)
    };
    s.add("J6-重号分派拒", j6, "同码不得重复");

    // J7 印账冻结。
    let j7 = {
        let mut b = DispatchBook::new();
        b.seal();
        b.dispatch(CtlCode {
            device: DEV_DISK_FS,
            function: 7,
            access: 0,
            method: 0,
        })
        .err() == Some(DC_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后必拒");

    // J8 指纹幂等。
    let j8 = {
        let mut b = DispatchBook::new();
        let _ = b.dispatch(CtlCode {
            device: DEV_DISK_FS,
            function: 1,
            access: 0,
            method: 0,
        });
        b.fold() == b.fold()
    };
    s.add("J8-指纹幂等", j8, "同态同指纹");

    // J9 回读渲染。
    let j9 = {
        let mut b = DispatchBook::new();
        let c = CtlCode {
            device: DEV_NETWORK_FS,
            function: 0x10,
            access: FILE_READ_ACCESS,
            method: METHOD_NEITHER,
        };
        let raw = c.encode();
        let _ = b.dispatch(c);
        b.line_of(raw).contains("dev12") && b.line_of(raw ^ 0x1) == "missing"
    };
    s.add("J9-回读渲染", j9, "原始码查账");

    // J10 零功能码拒。
    let j10 = {
        let c = CtlCode {
            device: DEV_DISK_FS,
            function: 0,
            access: 0,
            method: 0,
        };
        c.validate().err() == Some(DC_BAD_DEVICE)
    };
    s.add("J10-零功能码拒", j10, "功能码非零");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = DispatchBook::new();
    let _ = b.dispatch(CtlCode {
        device: DEV_DISK_FS,
        function: 1,
        access: 0,
        method: 0,
    });
    let _ = b.dispatch(CtlCode {
        device: DEV_NAMED_PIPE,
        function: 2,
        access: 0,
        method: METHOD_NEITHER,
    });
    s.add("摘要回读", b.summary().contains("codes=2"), "双码在册");
    s.add("方法异记账", b.recs[0].method != b.recs[1].method, "两类方法");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12268_ctl_roundtrip() {
        let raw = ((DEV_DISK_FS as u32) << 16) | (0x1 << 14) | (0x33 << 2) | 1;
        let c = CtlCode::decode(raw);
        assert_eq!(c.device, DEV_DISK_FS);
        assert_eq!(c.function, 0x33);
        assert_eq!(c.method, METHOD_IN_DIRECT);
        assert_eq!(c.encode(), raw);
    }

    #[test]
    fn unx12268_checks_green() {
        let s = run_unx12268_checks();
        assert!(s.all_passed(), "UNX-F12268 判据未全绿");
    }

    #[test]
    fn unx12268_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
