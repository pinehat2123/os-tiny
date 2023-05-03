#![no_std]

extern crate alloc;

mod bitmap;
mod block_cache;
mod block_dev;
mod efs;
mod layout;
mod vfs;

mod config {
    pub const BLOCK_SZ: usize = 512;
    pub const BLOCK_SIZE: usize = 512;
    pub const CACHE_SIZE: usize = 100;
}
use bitmap::Bitmap;
use block_cache::{block_cache_sync_all, get_block_cache};
pub use block_dev::BlockDevice;
pub use config::*;
pub use efs::EasyFileSystem;
use layout::*;
pub use vfs::Inode;

#[cfg(feature = "async_async_trait")]
mod asynchronous_async_trait {
    use alloc::boxed::Box;
    use async_trait::async_trait;
    #[async_trait]
    pub trait AsyncBlockDevice {
        async fn read(&self, block_id: usize, buf: &mut [u8]);
        async fn write(&self, block_id: usize, buf: &[u8]);
    }
}
#[cfg(feature = "async_async_trait")]
pub use asynchronous_async_trait::AsyncBlockDevice;

#[cfg(feature = "async_generic")]
mod asynchronous_generic {
    pub trait AsyncBlockDevice {
        type NextFuture<'a>: Future<Output = Option<(&'a [u8], &'a [u8])>>;

        fn read(&self, block_id: usize, buf: &mut [u8]) -> Self::NextFuture<'_>;
        fn write(&self, block_id: usize, buf: &[u8]) -> Self::NextFuture<'_>;
    }
}

#[derive(Debug)]
pub enum EasyError {
    NotFound,
    CreateFileError,
}
