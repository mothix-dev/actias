pub mod heap;
pub mod paging;

pub fn init() {
    log!("initializing heap...");
    heap::init();
}
