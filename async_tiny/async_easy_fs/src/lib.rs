#![no_std]
extern crate alloc;
use alloc::boxed::Box;
use async_trait::async_trait;

#[async_trait]
pub trait AsyncBlockDevive {
    async fn read(&self, block_id: usize, buf: &mut [u8]);
    async fn write(&self, block_id: usize, buf: &[u8]);
}