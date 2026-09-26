//! AI-H1 隔离验证舱 lib：镜像主 crate 的模块路径（crate::checks /
//! crate::star::sbase / crate::h1star），使 h1star 域文件零修改可编译。
//! 收口门禁仍在主树 kernel/varix 跑全量——本舱只解决「兄弟分队在途
//! 代码红测阻断」的并行施工隔离问题。
#![cfg_attr(not(test), no_std)]
#![allow(dead_code)]

extern crate alloc;

pub mod checks;
pub mod star {
    pub mod sbase;
}
pub mod h1star;
