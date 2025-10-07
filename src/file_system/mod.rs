// src/file_system/mod.rs
// File system operations module

pub mod operations;

pub use operations::{
    write_file_with_history,
    undo_file_modification,
    get_file_history,
    get_modified_files,
    FileModification,
};
