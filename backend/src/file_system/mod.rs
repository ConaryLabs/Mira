// src/file_system/mod.rs
// File system operations module

pub mod operations;

pub use operations::{
    FileModification, get_file_history, get_modified_files, undo_file_modification,
    write_file_with_dirs, write_file_with_history,
};
