//! v3 Loop Engineering Module
//!
//! 三层抑制机制的 native 侧部分。
//! feedback_loader 在 `paporot analyze` 入口处构建 FeedbackIndex，
//! 序列化为 JSON 写入 .Paporot/work/feedback_index.json。
//! WASM 侧 paporot-core 通过 host::read_file 读取该文件完成抑制。

pub mod feedback_loader;
