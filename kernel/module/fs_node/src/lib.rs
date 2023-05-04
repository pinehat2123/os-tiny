#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
extern crate spin;
extern crate memory;

use alloc::{string::String, vec::Vec, boxed::Box, sync::{Arc, Weak}};
use spin::Mutex;
use memory::UserBuffer;


pub type DirRef = Arc<Mutex<Box<Directory + Send>>>;
pub type WeakDirRef = Weak<Mutex<Box<Directory + Send>>>;
pub type FileRef = Arc<Mutex<Box<File + Send>>>;

pub trait FsNode {
    fn get_path_as_string(&self) -> String {
        let mut path = self.get_name();
        if let Ok(cur_dir) =  self.get_parent_dir() {
            path.insert_str(0, &format!("{}/", &cur_dir.lock().get_path_as_string()));
            return path;
        }
        return path;
    }
    fn get_name(&self) -> String;
    fn get_parent_dir(&self) -> Result<DirRef, &'static str>;
}

pub trait File : FsNode {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, &'static str>;
    fn write(&mut self, buffer: &[u8]) -> Result<usize, &'static str>;
    fn delete(self) -> Result<(), &'static str>;
    fn size(&self) -> usize;
    fn as_mapping(&self) -> Result<&UserBuffer, &'static str>;
}

pub trait Directory : FsNode + Send {
    fn get_child(&self, child_name: &str) -> Option<FileOrDir>;
    fn insert_child(&mut self, child: FileOrDir) -> Result<(), &'static str>;
    fn list_children(&mut self) -> Vec<String>;
}


#[derive(Clone)]
pub enum FileOrDir {
    File(FileRef),
    Dir(DirRef),
}

impl FsNode for FileOrDir {
    /// Recursively gets the absolute pathname as a String
    fn get_path_as_string(&self) -> String {
        return match self {
            FileOrDir::File(file) => file.lock().get_path_as_string(),
            FileOrDir::Dir(dir) => dir.lock().get_path_as_string(),
        };
    }
    fn get_name(&self) -> String {
        return match self {
            FileOrDir::File(file) => file.lock().get_name(),
            FileOrDir::Dir(dir) => dir.lock().get_name(),
        };
    }
    fn get_parent_dir(&self) -> Result<DirRef, &'static str> {
        return match self {
            FileOrDir::File(file) => file.lock().get_parent_dir(),
            FileOrDir::Dir(dir) => dir.lock().get_parent_dir(),
        };
    }
}

