use alloc::sync::Arc;
use spin::Mutex;

use super::PageDirectory;

/// tracks how many times a page directory was updated
pub struct PageDirTracker<D: PageDirectory> {
    page_dir: D,
    updates: usize,
}

impl<D: PageDirectory> PageDirTracker<D> {
    pub fn track(page_dir: D) -> Self {
        Self { page_dir, updates: 0 }
    }

    pub fn updates(&self) -> usize {
        self.updates
    }
}

impl<D: PageDirectory> PageDirectory for PageDirTracker<D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;
    type Reserved = D::Reserved;
    type RawKernelArea = D::RawKernelArea;

    fn new(current_dir: &impl PageDirectory) -> Result<Self, super::PagingError>
    where Self: Sized {
        Ok(Self {
            page_dir: D::new(current_dir)?,
            updates: 0,
        })
    }

    fn get_page(&self, addr: usize) -> Option<super::PageFrame> {
        self.page_dir.get_page(addr)
    }

    fn set_page(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<super::PageFrame>) -> Result<(), super::PagingError> {
        self.page_dir.set_page(current_dir, addr, page)?;
        self.updates = self.updates.wrapping_add(1);
        Ok(())
    }

    fn set_page_no_alloc(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<super::PageFrame>, reserved_memory: Option<Self::Reserved>) -> Result<(), super::PagingError> {
        self.page_dir.set_page_no_alloc(current_dir, addr, page, reserved_memory)?;
        self.updates = self.updates.wrapping_add(1);
        Ok(())
    }

    unsafe fn switch_to(&self) {
        self.page_dir.switch_to();
    }

    fn flush_page(addr: usize) {
        D::flush_page(addr);
    }

    fn get_raw_kernel_area(&self) -> &Self::RawKernelArea {
        self.page_dir.get_raw_kernel_area()
    }

    unsafe fn set_raw_kernel_area(&mut self, area: &Self::RawKernelArea) {
        self.page_dir.set_raw_kernel_area(area);
    }

    fn is_unused(&self, addr: usize) -> bool {
        self.page_dir.is_unused(addr)
    }

    fn virt_to_phys(&self, virt: usize) -> Option<crate::arch::PhysicalAddress> {
        self.page_dir.virt_to_phys(virt)
    }
}

/// manages keeping a pagedirectory synchronized with the kernel page directory
pub struct PageDirSync<D: PageDirectory> {
    sync_from: Arc<Mutex<PageDirTracker<D>>>,
    page_dir: D,
    kernel_region: super::ContiguousRegion<usize>,
    updates: usize,
}

impl<D: PageDirectory> PageDirSync<D> {
    /// creates a new PageDirSync set to synchronize from the given page directory and initially synchronizes it
    pub fn sync_from(dir: Arc<Mutex<PageDirTracker<D>>>, kernel_region: super::ContiguousRegion<usize>) -> Result<Self, super::PagingError> {
        let guard = dir.lock();
        let page_dir = D::new(&*guard)?;
        let updates = guard.updates;
        drop(guard);

        let mut state = Self {
            sync_from: dir,
            page_dir,
            kernel_region,
            updates,
        };
        state.force_synchronize();
        Ok(state)
    }

    /// forces this page directory to synchronize its kernel area with that of the kernel's page directory
    pub fn force_synchronize(&mut self) {
        let sync_from = self.sync_from.lock();
        unsafe {
            self.page_dir.set_raw_kernel_area(sync_from.get_raw_kernel_area());
        }
        self.updates = sync_from.updates;
    }

    /// checks whether this page directory and the kernel's page directory have gone out of sync, and re-synchronize them if so
    pub fn check_synchronize(&mut self) {
        let sync_from = self.sync_from.lock();

        if self.updates != sync_from.updates {
            unsafe {
                self.page_dir.set_raw_kernel_area(sync_from.get_raw_kernel_area());
            }
            self.updates = sync_from.updates;
        }
    }
}

impl<D: PageDirectory> PageDirectory for PageDirSync<D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;
    type Reserved = D::Reserved;
    type RawKernelArea = D::RawKernelArea;

    fn new(_current_dir: &impl PageDirectory) -> Result<Self, super::PagingError>
    where Self: Sized {
        Err(super::PagingError::Invalid)
    }

    fn get_page(&self, addr: usize) -> Option<super::PageFrame> {
        if self.kernel_region.contains((addr / Self::PAGE_SIZE) * Self::PAGE_SIZE) {
            self.sync_from.lock().get_page(addr)
        } else {
            self.page_dir.get_page(addr)
        }
    }

    #[allow(clippy::collapsible_else_if)]
    fn set_page(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<super::PageFrame>) -> Result<(), super::PagingError> {
        if current_dir.is_none() {
            let current_dir = SyncVirtToPhys { sync_from: self.sync_from.clone() };
            let current_dir = Some(&current_dir);

            if self.kernel_region.contains((addr / Self::PAGE_SIZE) * Self::PAGE_SIZE) {
                self.sync_from.lock().set_page(current_dir, addr, page)
            } else {
                self.page_dir.set_page(current_dir, addr, page)
            }
        } else {
            if self.kernel_region.contains((addr / Self::PAGE_SIZE) * Self::PAGE_SIZE) {
                self.sync_from.lock().set_page(current_dir, addr, page)
            } else {
                self.page_dir.set_page(current_dir, addr, page)
            }
        }
    }

    #[allow(clippy::collapsible_else_if)]
    fn set_page_no_alloc(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<super::PageFrame>, reserved_memory: Option<Self::Reserved>) -> Result<(), super::PagingError> {
        if current_dir.is_none() {
            let current_dir = SyncVirtToPhys { sync_from: self.sync_from.clone() };
            let current_dir = Some(&current_dir);

            if self.kernel_region.contains((addr / Self::PAGE_SIZE) * Self::PAGE_SIZE) {
                self.sync_from.lock().set_page_no_alloc(current_dir, addr, page, reserved_memory)
            } else {
                self.page_dir.set_page_no_alloc(current_dir, addr, page, reserved_memory)
            }
        } else {
            if self.kernel_region.contains((addr / Self::PAGE_SIZE) * Self::PAGE_SIZE) {
                self.sync_from.lock().set_page_no_alloc(current_dir, addr, page, reserved_memory)
            } else {
                self.page_dir.set_page_no_alloc(current_dir, addr, page, reserved_memory)
            }
        }
    }

    unsafe fn switch_to(&self) {
        self.page_dir.switch_to();
    }

    fn flush_page(addr: usize) {
        D::flush_page(addr);
    }

    fn get_raw_kernel_area(&self) -> &Self::RawKernelArea {
        panic!("get_raw_kernel_area() for PageDirSync is invalid");
    }

    unsafe fn set_raw_kernel_area(&mut self, _area: &Self::RawKernelArea) {
        panic!("set_raw_kernel_area() for PageDirSync is invalid");
    }

    fn is_unused(&self, addr: usize) -> bool {
        self.page_dir.is_unused(addr)
    }

    fn virt_to_phys(&self, virt: usize) -> Option<crate::arch::PhysicalAddress> {
        if self.kernel_region.contains((virt / Self::PAGE_SIZE) * Self::PAGE_SIZE) {
            self.sync_from.lock().virt_to_phys(virt)
        } else {
            self.page_dir.virt_to_phys(virt)
        }
    }
}

struct SyncVirtToPhys<D: PageDirectory> {
    sync_from: Arc<Mutex<PageDirTracker<D>>>,
}

impl<D: PageDirectory> PageDirectory for SyncVirtToPhys<D> {
    const PAGE_SIZE: usize = D::PAGE_SIZE;
    type Reserved = D::Reserved;
    type RawKernelArea = D::RawKernelArea;

    fn new(_current_dir: &impl PageDirectory) -> Result<Self, super::PagingError>
    where Self: Sized {
        unimplemented!();
    }

    fn get_page(&self, _addr: usize) -> Option<super::PageFrame> {
        unimplemented!();
    }

    fn set_page(&mut self, _current_dir: Option<&impl PageDirectory>, _addr: usize, _page: Option<super::PageFrame>) -> Result<(), super::PagingError> {
        unimplemented!();
    }

    fn set_page_no_alloc(
        &mut self,
        _current_dir: Option<&impl PageDirectory>,
        _addr: usize,
        _page: Option<super::PageFrame>,
        _reserved_memory: Option<Self::Reserved>,
    ) -> Result<(), super::PagingError> {
        unimplemented!();
    }

    unsafe fn switch_to(&self) {
        unimplemented!();
    }

    fn flush_page(_addr: usize) {
        unimplemented!();
    }

    fn get_raw_kernel_area(&self) -> &Self::RawKernelArea {
        unimplemented!();
    }

    unsafe fn set_raw_kernel_area(&mut self, _area: &Self::RawKernelArea) {
        unimplemented!();
    }

    fn virt_to_phys(&self, virt: usize) -> Option<crate::arch::PhysicalAddress> {
        self.sync_from.lock().virt_to_phys(virt)
    }
}
