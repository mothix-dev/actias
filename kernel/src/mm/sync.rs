use super::paging::{PageDirectory, PageFrame, PagingError};
use crate::{
    arch::KERNEL_PAGE_DIR_SPLIT,
    task::queue::PageUpdateEntry,
};
use spin::Mutex;
use log::debug;

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
}

impl<D: PageDirectory> PageDirectory for PageDirTracker<D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;

    fn get_page(&self, addr: usize) -> Option<PageFrame> {
        self.page_dir.get_page(addr)
    }

    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        self.updates = self.updates.wrapping_add(1);
        self.page_dir.set_page(addr, page)
    }

    unsafe fn switch_to(&self) {
        self.page_dir.switch_to()
    }
}

impl<D: PageDirectory> PageDirTracker<D> {
    pub fn new(page_dir: D) -> Self {
        Self {
            page_dir,
            updates: 0,
        }
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
