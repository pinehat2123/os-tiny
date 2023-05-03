use core::mem::MaybeUninit;

use crate::AsyncBlockDevice;

use super::{BlockDevice, BLOCK_SZ};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use lazy_static::*;
use spin::Mutex;

mod cache {
    use alloc::vec::Vec;
    pub trait Cache<const N: usize> {
        type Key;
        type Value;
        fn get(&mut self, key: &Self::Key) -> Option<Self::Value>;
        fn put(&mut self, key: &Self::Key, value: Self::Value) -> Option<(Self::Key, Self::Value)>;
        fn all(&mut self) -> Vec<(Self::Key, Self::Value)>;
    }
    #[derive(Clone, Copy)]
    pub struct Node<K: Eq + PartialEq + Copy, V: Clone> {
        key: K,
        value: V,
        cnt: usize,
        time: usize,
        dirty: bool,
    }

    impl<K: Eq + PartialEq + Copy, V: Clone> Node<K, V> {
        pub fn new(key: K, value: V) -> Self {
            Self {
                key,
                value,
                cnt: 0,
                time: 0,
                dirty: false,
            }
        }
    }

    impl<K: Eq + PartialEq + Copy, V: Clone> PartialEq for Node<K, V> {
        fn eq(&self, other: &Self) -> bool {
            self.cnt == other.cnt
        }
    }

    impl<K: Eq + PartialEq + Copy, V: Clone> Eq for Node<K, V> {}

    impl<K: Eq + PartialEq + Copy, V: Clone> Ord for Node<K, V> {
        fn cmp(&self, other: &Self) -> core::cmp::Ordering {
            self.cnt
                .cmp(&other.cnt)
                .then_with(|| self.time.cmp(&other.time))
        }
    }

    impl<K: Eq + PartialEq + Copy, V: Clone> PartialOrd for Node<K, V> {
        fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    pub struct LFUCache<K: Eq + PartialEq + Copy, V: Clone, const N: usize> {
        data: [Node<K, V>; N],
        size: usize,
        time: usize,
    }

    impl<K: Eq + PartialEq + Copy, V: Clone, const N: usize> LFUCache<K, V, N> {
        /// 初始化
        pub fn init(data: [Node<K, V>; N]) -> Self {
            Self {
                data,
                size: N,
                time: 0,
            }
        }

        pub fn empty(data: [Node<K, V>; N]) -> Self {
            Self {
                data,
                size: 0,
                time: 0,
            }
        }
    }

    impl<K: Eq + PartialEq + Copy, V: Clone, const N: usize> Cache<N> for LFUCache<K, V, N> {
        type Key = K;
        type Value = V;
        fn get(&mut self, key: &Self::Key) -> Option<Self::Value> {
            self.time += 1;
            let time = self.time;
            self.data[0..self.size]
                .iter_mut()
                .find(|i| i.key == *key)
                .map(|node| {
                    // 更新结点时间和访问次数
                    node.time = time;
                    node.cnt += 1;
                    node.value.clone()
                })
        }
        fn put(&mut self, key: &Self::Key, value: Self::Value) -> Option<(Self::Key, Self::Value)> {
            self.time += 1;
            if let Some(node) = self.data.iter_mut().find(|i| i.key == *key) {
                node.value = value;
                node.cnt += 1;
                node.time = self.time;
                // 写脏
                node.dirty = true;
                return None;
            } else {
                if self.size < N {
                    // 缓存未满
                    self.data[self.size].key = *key;
                    self.data[self.size].value = value;
                    self.data[self.size].cnt = 1;
                    self.data[self.size].time = self.time;
                    self.size += 1;
                    return None;
                } else {
                    // 缓存已满
                    // 顺序排序
                    self.data[0..self.size].sort_by(|a, b| a.cmp(b));
                    // 淘汰第一项
                    let node = &mut self.data[0];
                    let write_back = (node.key, node.value.clone());
                    node.key = *key;
                    node.value = value;
                    node.cnt = 1;
                    node.time = self.time;
                    // 如果数据已经被写脏，现在需要写回
                    match node.dirty {
                        true => Some(write_back),
                        false => None,
                    }
                }
            }
        }
        fn all(&mut self) -> Vec<(Self::Key, Self::Value)> {
            self.data[0..self.size]
                .iter()
                .map(|n| (n.key, n.value.clone()))
                .collect()
        }
    }
}
pub(crate) use cache::*;

pub struct AsyncBlockCache<
    C: Cache<N, Key = usize, Value = [u8; B]> + Send + Sync,
    const B: usize,
    const N: usize,
> {
    block_id: usize,
    block_device: Arc<dyn AsyncBlockDevice + Send + Sync>,
    cache: AsyncMutex<C>,
    modified: bool,
}

impl
    AsyncBlockCache<
        LFUCache<usize, [u8; crate::BLOCK_SIZE], crate::CACHE_SIZE>,
        crate::BLOCK_SIZE,
        crate::CACHE_SIZE,
    >
{
    pub fn init(block_id: usize, device: Arc<dyn AsyncBlockDevice + Send + Sync>) -> Self {
        let mut data: [MaybeUninit<Node<usize, [u8; crate::BLOCK_SIZE]>>; crate::CACHE_SIZE] =
            unsafe { MaybeUninit::uninit().assume_init() };
        for elem in &mut data[..] {
            *elem = MaybeUninit::new(Node::new(0, [0; crate::BLOCK_SIZE]));
        }
        let nodes = unsafe {
            core::mem::transmute::<_, [Node<usize, [u8; crate::BLOCK_SIZE]>; crate::CACHE_SIZE]>(
                data,
            )
        };
        let lfu_cache = LFUCache::empty(nodes);
        Self {
            block_id,
            block_device: device,
            cache: AsyncMutex::new(lfu_cache),
            modified: false,
        }
    }

    /// 异步方式从块缓存中读取一个块
    pub async fn read_block(&self, block_id: usize) -> [u8; BLOCK_SIZE] {
        {
            // 申请锁
            let mut s = self.cache.lock().await;
            if let Some(block) = s.get(&block_id) {
                // 如果想要读取的块在缓冲层中，则读出来直接返回，不用读写块设备
                return block;
            }
        } // 释放锁
          // 如果要读取的块不在缓冲层中，则需要从块设备读取
        let mut data = [0; BLOCK_SIZE];
        self.block_device.read(block_id, &mut data).await;
        // 将读取到的块写入到缓冲层
        let mut s = self.cache.lock().await; // 申请锁
        let write_back = s.put(&block_id, data.clone());
        drop(s); // 释放锁
        if let Some((id, mut block)) = write_back {
            // 如果有需要写回到块设备的数据，这里写回
            self.block_device.write(id, &mut block).await;
        }
        data
    }

    /// 异步方式往块缓冲中写入一个块
    pub async fn write_block(&self, block_id: usize, buf: [u8; BLOCK_SIZE]) {
        let mut s = self.cache.lock().await; // 申请锁
        let write_back = s.put(&block_id, buf);
        drop(s); // 释放锁
        if let Some((id, mut block)) = write_back {
            self.block_device.write(id, &mut block).await;
        }
    }

    /// 异步，写穿方式往缓冲区中写入一个块
    pub async fn write_sync(&self, block_id: usize, buf: [u8; BLOCK_SIZE]) {
        self.write_block(block_id, buf.clone()).await;
        self.block_device.write(block_id, &buf).await
    }

    /// 将缓冲层中的所有数据写回到块设备
    pub async fn sync(&self) {
        let mut s = self.cache.lock().await;
        for (id, block) in s.all() {
            self.block_device.write(id, &block).await;
        }
    }
}

// pub struct BlockCache {
//     cache: Vec<u8>,
//     block_id: usize,
//     block_device: Arc<dyn BlockDevice>,
//     modified: bool,
// }
//
// impl BlockCache {
//     /// Load a new BlockCache from disk.
//     pub fn new(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Self {
//         // for alignment and move effciency
//         let mut cache = vec![0u8; BLOCK_SZ];
//         block_device.read_block(block_id, &mut cache);
//         Self {
//             cache,
//             block_id,
//             block_device,
//             modified: false,
//         }
//     }
//
//     fn addr_of_offset(&self, offset: usize) -> usize {
//         &self.cache[offset] as *const _ as usize
//     }
//
//     pub fn get_ref<T>(&self, offset: usize) -> &T
//     where
//         T: Sized,
//     {
//         let type_size = core::mem::size_of::<T>();
//         assert!(offset + type_size <= BLOCK_SZ);
//         let addr = self.addr_of_offset(offset);
//         unsafe { &*(addr as *const T) }
//     }
//
//     pub fn get_mut<T>(&mut self, offset: usize) -> &mut T
//     where
//         T: Sized,
//     {
//         let type_size = core::mem::size_of::<T>();
//         assert!(offset + type_size <= BLOCK_SZ);
//         self.modified = true;
//         let addr = self.addr_of_offset(offset);
//         unsafe { &mut *(addr as *mut T) }
//     }
//
//     pub fn read<T, V>(&self, offset: usize, f: impl FnOnce(&T) -> V) -> V {
//         f(self.get_ref(offset))
//     }
//
//     pub fn modify<T, V>(&mut self, offset: usize, f: impl FnOnce(&mut T) -> V) -> V {
//         f(self.get_mut(offset))
//     }
//
//     pub fn sync(&mut self) {
//         if self.modified {
//             self.modified = false;
//             self.block_device.write_block(self.block_id, &self.cache);
//         }
//     }
// }
//
// impl Drop for BlockCache {
//     fn drop(&mut self) {
//         self.sync()
//     }
// }
//
// const BLOCK_CACHE_SIZE: usize = 16;
//
// pub struct BlockCacheManager {
//     queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,
// }
//
// impl BlockCacheManager {
//     pub fn new() -> Self {
//         Self {
//             queue: VecDeque::new(),
//         }
//     }
//
//     pub fn get_block_cache(
//         &mut self,
//         block_id: usize,
//         block_device: Arc<dyn BlockDevice>,
//     ) -> Arc<Mutex<BlockCache>> {
//         if let Some(pair) = self.queue.iter().find(|pair| pair.0 == block_id) {
//             Arc::clone(&pair.1)
//         } else {
//             // substitute
//             if self.queue.len() == BLOCK_CACHE_SIZE {
//                 // from front to tail
//                 if let Some((idx, _)) = self
//                     .queue
//                     .iter()
//                     .enumerate()
//                     .find(|(_, pair)| Arc::strong_count(&pair.1) == 1)
//                 {
//                     self.queue.drain(idx..=idx);
//                 } else {
//                     panic!("Run out of BlockCache!");
//                 }
//             }
//             // load block into mem and push back
//             let block_cache = Arc::new(Mutex::new(BlockCache::new(
//                 block_id,
//                 Arc::clone(&block_device),
//             )));
//             self.queue.push_back((block_id, Arc::clone(&block_cache)));
//             block_cache
//         }
//     }
// }
//
// lazy_static! {
//     pub static ref BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> =
//         Mutex::new(BlockCacheManager::new());
// }
//
// pub fn get_block_cache(
//     block_id: usize,
//     block_device: Arc<dyn BlockDevice>,
// ) -> Arc<Mutex<BlockCache>> {
//     BLOCK_CACHE_MANAGER
//         .lock()
//         .get_block_cache(block_id, block_device)
// }
//
// pub fn block_cache_sync_all() {
//     let manager = BLOCK_CACHE_MANAGER.lock();
//     for (_, cache) in manager.queue.iter() {
//         cache.lock().sync();
//     }
// }
//
