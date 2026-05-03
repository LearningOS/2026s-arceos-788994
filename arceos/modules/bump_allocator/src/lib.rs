#![no_std]

use allocator::{AllocError, AllocResult, BaseAllocator, ByteAllocator, PageAllocator};
use core::ptr::NonNull;

/// 早期内存分配器实现
/// 采用双端内存布局：字节分配从起始地址向后增加，页面分配从结束地址向前减少
pub struct EarlyAllocator<const SIZE: usize> {
    start: usize,
    end: usize,
    b_pos: usize,       // 字节分配当前指针 (向前)
    p_pos: usize,       // 页面分配当前指针 (向后)
    alloc_count: usize, // 记录当前存活的字节分配数量
}

impl<const SIZE: usize> EarlyAllocator<SIZE> {
    pub const fn new() -> Self {
        Self {
            start: 0,
            end: 0,
            b_pos: 0,
            p_pos: 0,
            alloc_count: 0,
        }
    }

    /// 辅助函数：向上对齐
    fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }

    /// 辅助函数：向下对齐
    fn align_down(addr: usize, align: usize) -> usize {
        addr & !(align - 1)
    }
}

impl<const SIZE: usize> BaseAllocator for EarlyAllocator<SIZE> {
    fn init(&mut self, start: usize, size: usize) {
        self.start = start;
        self.end = start + size;
        self.b_pos = start;
        self.p_pos = start + size;
        self.alloc_count = 0;
    }

    fn add_memory(&mut self, _start: usize, _size: usize) -> AllocResult {
        // 早期分配器通常不支持动态追加不连续的内存区域
        Err(AllocError::InvalidParam)
    }
}

impl<const SIZE: usize> ByteAllocator for EarlyAllocator<SIZE> {
    fn alloc(
        &mut self,
        layout: core::alloc::Layout,
    ) -> AllocResult<NonNull<u8>> {
        let align = layout.align();
        let size = layout.size();
        
        let alloc_start = Self::align_up(self.b_pos, align);
        let alloc_end = alloc_start + size;

        // 检查是否与页面分配区域重叠
        if alloc_end > self.p_pos {
            return Err(AllocError::NoMemory);
        }

        self.b_pos = alloc_end;
        self.alloc_count += 1;
        Ok(NonNull::new(alloc_start as *mut u8).unwrap())
    }

    fn dealloc(&mut self, _pos: NonNull<u8>, _layout: core::alloc::Layout) {
        if self.alloc_count > 0 {
            self.alloc_count -= 1;
        }
        // 当所有字节分配都释放时，重置字节分配区域指针 (Bump 重置)
        if self.alloc_count == 0 {
            self.b_pos = self.start;
        }
    }

    fn total_bytes(&self) -> usize {
        self.end - self.start
    }

    fn used_bytes(&self) -> usize {
        self.b_pos - self.start
    }

    fn available_bytes(&self) -> usize {
        if self.p_pos > self.b_pos {
            self.p_pos - self.b_pos
        } else {
            0
        }
    }
}

impl<const SIZE: usize> PageAllocator for EarlyAllocator<SIZE> {
    const PAGE_SIZE: usize = SIZE;

    fn alloc_pages(
        &mut self,
        num_pages: usize,
        align_pow2: usize,
    ) -> AllocResult<usize> {
        let size = num_pages * Self::PAGE_SIZE;
        // 页面分配从 p_pos 向前分配，因此起始地址需要向下寻找并对齐
        let potential_start = self.p_pos.saturating_sub(size);
        let alloc_start = Self::align_down(potential_start, align_pow2);

        // 检查是否与字节分配区域重叠
        if alloc_start < self.b_pos || alloc_start < self.start {
            return Err(AllocError::NoMemory);
        }

        self.p_pos = alloc_start;
        Ok(alloc_start)
    }

    fn dealloc_pages(&mut self, _pos: usize, _num_pages: usize) {
        // 根据设计要求：页面区域永远不会被释放[cite: 5]
    }

    fn total_pages(&self) -> usize {
        (self.end - self.start) / Self::PAGE_SIZE
    }

    fn used_pages(&self) -> usize {
        (self.end - self.p_pos) / Self::PAGE_SIZE
    }

    fn available_pages(&self) -> usize {
        (self.p_pos - self.b_pos) / Self::PAGE_SIZE
    }
}