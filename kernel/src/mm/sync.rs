use super::paging::{PageDirectory, PageFrame, PagingError};
use crate::{arch::KERNEL_PAGE_DIR_SPLIT, task::queue::PageUpdateEntry};
use log::debug;
use spin::{Mutex, MutexGuard};

pub struct PageDirSync<'kernel, D: PageDirectory> {
    pub kernel: &'kernel Mutex<PageDirTracker<D>>,
    pub task: D,
    pub process_id: usize,
    pub kernel_space_updates: usize,
}

impl<D: PageDirectory> PageDirectory for PageDirSync<'_, D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;

    fn get_page(&self, addr: usize) -> Option<PageFrame> {
        if addr >= KERNEL_PAGE_DIR_SPLIT {
            self.kernel.lock().get_page(addr)
        } else {
            self.task.get_page(addr)
        }
    }

    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        if addr >= KERNEL_PAGE_DIR_SPLIT {
            self.task.set_page(addr, page)?;
            self.kernel.lock().set_page(addr, page)?;
            self.kernel_space_updates = self.kernel_space_updates.wrapping_add(1);

            crate::task::update_page(PageUpdateEntry::Kernel { addr });
        } else {
            self.task.set_page(addr, page)?;

            crate::task::update_page(PageUpdateEntry::Task { process_id: self.process_id, addr });
        }

        Ok(())
    }

    unsafe fn switch_to(&self) {
        self.task.switch_to()
    }

    fn is_unused(&self, addr: usize) -> bool {
        self.task.is_unused(addr)
    }

    fn virt_to_phys(&self, virt: usize) -> Option<u64> {
        self.task.virt_to_phys(virt)
    }

    fn find_hole(&self, start: usize, end: usize, size: usize) -> Option<usize> {
        self.task.find_hole(start, end, size)
    }
}

impl<D: PageDirectory> PageDirSync<'_, D> {
    /// synchronizes if we've fallen out of sync
    pub fn sync(&mut self) {
        if self.kernel_space_updates != self.kernel.lock().updates() {
            self.force_sync();
        }
    }

    /// forces a synchronization regardless of whether we're in sync or not
    pub fn force_sync(&mut self) {
        debug!("synchronizing page directories");

        let mut initial_updates = self.kernel.lock().updates();

        loop {
            // unnecessarily inefficient because the fucking mutex guard won't give references to the original object (for good reason) but it also doesn't fucking pass through traits
            for i in (KERNEL_PAGE_DIR_SPLIT..=usize::MAX).step_by(Self::PAGE_SIZE) {
                let page = self.kernel.lock().get_page(i);

                self.task.set_page(i, page).expect("unable to synchronize page directories");
            }

            let current_updates = self.kernel.lock().updates();

            if initial_updates == current_updates {
                self.kernel_space_updates = current_updates;
                break;
            } else {
                debug!("page directories changed during sync, trying again");
                initial_updates = current_updates;
            }
        }

        debug!("finished synchronizing");
    }
}

pub struct PageDirTracker<D: PageDirectory> {
    page_dir: D,
    updates: usize,
    is_kernel: bool,
}

impl<D: PageDirectory> PageDirectory for PageDirTracker<D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;

    fn get_page(&self, addr: usize) -> Option<PageFrame> {
        self.page_dir.get_page(addr)
    }

    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        self.updates = self.updates.wrapping_add(1);
        self.page_dir.set_page(addr, page)?;

        if self.is_kernel && addr > KERNEL_PAGE_DIR_SPLIT {
            crate::task::update_page(PageUpdateEntry::Kernel { addr });
        }

        Ok(())
    }

    unsafe fn switch_to(&self) {
        self.page_dir.switch_to()
    }

    fn is_unused(&self, addr: usize) -> bool {
        self.page_dir.is_unused(addr)
    }

    fn virt_to_phys(&self, virt: usize) -> Option<u64> {
        self.page_dir.virt_to_phys(virt)
    }

    fn find_hole(&self, start: usize, end: usize, size: usize) -> Option<usize> {
        self.page_dir.find_hole(start, end, size)
    }
}

impl<D: PageDirectory> PageDirTracker<D> {
    pub fn new(page_dir: D, is_kernel: bool) -> Self {
        Self { page_dir, updates: 0, is_kernel }
    }

    /// returns the update counter for this tracker
    pub fn updates(&self) -> usize {
        self.updates
    }

    /// returns a reference to the underlying page directory
    pub fn inner(&self) -> &D {
        &self.page_dir
    }

    /// returns a mutable reference to the underlying page directory
    ///
    /// # Safety
    ///
    /// this is unsafe because care needs to be had to not set any pages this way since that would throw off the sync counter
    pub unsafe fn inner_mut(&mut self) -> &mut D {
        &mut self.page_dir
    }
}

/// allows functions that require a PageDirectory to use a PageDirectory under a MutexGuard
#[repr(transparent)]
pub struct GuardedPageDir<'a, D: PageDirectory>(pub MutexGuard<'a, D>);

impl<D: PageDirectory> PageDirectory for GuardedPageDir<'_, D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;

    fn get_page(&self, addr: usize) -> Option<PageFrame> {
        self.0.get_page(addr)
    }

    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        self.0.set_page(addr, page)
    }

    unsafe fn switch_to(&self) {
        self.0.switch_to()
    }

    fn is_unused(&self, addr: usize) -> bool {
        self.0.is_unused(addr)
    }

    fn copy_from(&mut self, dir: &mut impl PageDirectory, from: usize, to: usize, num: usize) -> Result<(), PagingError> {
        self.0.copy_from(dir, from, to, num)
    }

    fn copy_on_write_from(&mut self, dir: &mut impl PageDirectory, from: usize, to: usize, num: usize) -> Result<(), PagingError> {
        self.0.copy_on_write_from(dir, from, to, num)
    }

    fn virt_to_phys(&self, virt: usize) -> Option<u64> {
        self.0.virt_to_phys(virt)
    }

    fn find_hole(&self, start: usize, end: usize, size: usize) -> Option<usize> {
        self.0.find_hole(start, end, size)
    }
}

#[repr(transparent)]
pub struct MutexedPageDir<'a, D: PageDirectory>(pub &'a Mutex<D>);

impl<D: PageDirectory> PageDirectory for MutexedPageDir<'_, D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;

    fn get_page(&self, addr: usize) -> Option<PageFrame> {
        self.lock().get_page(addr)
    }

    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        self.lock().set_page(addr, page)
    }

    unsafe fn switch_to(&self) {
        self.lock().switch_to()
    }

    fn is_unused(&self, addr: usize) -> bool {
        self.lock().is_unused(addr)
    }

    fn copy_from(&mut self, dir: &mut impl PageDirectory, from: usize, to: usize, num: usize) -> Result<(), PagingError> {
        self.lock().copy_from(dir, from, to, num)
    }

    fn copy_on_write_from(&mut self, dir: &mut impl PageDirectory, from: usize, to: usize, num: usize) -> Result<(), PagingError> {
        self.lock().copy_on_write_from(dir, from, to, num)
    }

    fn virt_to_phys(&self, virt: usize) -> Option<u64> {
        self.lock().virt_to_phys(virt)
    }

    fn find_hole(&self, start: usize, end: usize, size: usize) -> Option<usize> {
        self.lock().find_hole(start, end, size)
    }
}

impl<'a, D: PageDirectory> MutexedPageDir<'a, D> {
    pub fn lock(&self) -> MutexGuard<'a, D> {
        self.0.lock()
    }
}
