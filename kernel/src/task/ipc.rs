use super::{
    cpu::{CPUThread, ThreadID},
    get_process, get_cpus,
    syscalls::MINIMUM_MAPPING_ADDR,
    RegisterQueueEntry,
};
use crate::{
    arch::KERNEL_PAGE_DIR_SPLIT,
    mm::paging::{find_hole, get_page_dir, map_memory_from, validate_region, PageDirectory, PageFrame, ProcessOrKernelPageDir},
};
use common::types::{Errno, ProcessID, Result};
use log::{debug, trace, warn};

pub const HIGHEST_MESSAGE_NUM: u32 = u32::pow(2, 20) - 1; // 20 bits, inclusive

/// message passing internals- used by the send message syscall and other cpus to send a message if the receiving process is on the same cpu
pub fn send_message(thread_id: ThreadID, cpu_thread: &CPUThread, regs: &mut crate::arch::Registers, process_num: u32, message: u32, data: Option<(u64, usize)>) -> Result<()> {
    // TODO: find available thread

    let process_id = ProcessID { process: process_num, thread: 1 };

    let handler;
    let current_cpu;
    let stack_pointer;
    {
        let process = get_process(process_id.process).ok_or(Errno::NoSuchProcess)?;
        handler = *process.message_handlers.get(&message).ok_or(Errno::InvalidArgument)?;
        let thread = process.threads.get(process_id.thread as usize).ok_or(Errno::NoSuchProcess)?;
        current_cpu = thread.cpu;
        stack_pointer = thread.register_queue.current().registers.stack_pointer();
    }

    debug!(
        "CPU {thread_id} sending message {message} to process {process_id} (entry @ {:#x}, priority {})",
        handler.entry_point, handler.priority
    );

    if current_cpu.is_none() || current_cpu == Some(thread_id) {
        let mut process_page_dir = ProcessOrKernelPageDir::Process(process_num);

        // map data into process's memory if we can
        let (data, data_len) = if handler.has_data && let Some((data, data_len)) = data {
            trace!("mapping data in");
            if let Some(hole) = find_hole(&process_page_dir, MINIMUM_MAPPING_ADDR, KERNEL_PAGE_DIR_SPLIT, crate::arch::PageDirectory::PAGE_SIZE) {
                trace!("found hole @ {hole:#x}");
                process_page_dir
                    .set_page(
                        hole,
                        Some(PageFrame {
                            addr: data,
                            present: true,
                            user_mode: true,
                            writable: true,
                            ..Default::default()
                        }),
                    )
                    .map_err(|_| Errno::OutOfMemory)?;
                trace!("mapped");

                (Some(hole), data_len)
            } else {
                return Err(Errno::OutOfMemory);
            }
        } else {
            (None, 0)
        };

        let mut finish_sending_message = || -> Result<()> {
            let arguments = if handler.has_data {
                crate::util::abi::CallBuilder::new(crate::platform::PLATFORM_ABI)?
                    .argument(&message)?
                    .argument(&data.unwrap_or(0))?
                    .argument(&data_len)?
                    .finish()?
            } else {
                crate::util::abi::CallBuilder::new(crate::platform::PLATFORM_ABI)?.argument(&message)?.finish()?
            };

            let stack_pointer = (stack_pointer - arguments.stack.len()) & !15; // align to 16 bytes

            // check whether or not this process is already running on this cpu
            let on_cpu = cpu_thread.task_queue.lock().current().map(|c| c.id()) == Some(process_id);

            if arguments.should_write_stack {
                // make sure our stack is valid
                trace!("validating stack");
                if !validate_region(&process_page_dir, stack_pointer, arguments.stack.len()) {
                    return Err(Errno::BadAddress);
                }

                unsafe {
                    trace!("writing stack");
                    if on_cpu {
                        core::slice::from_raw_parts_mut(stack_pointer as *mut u8, arguments.stack.len()).copy_from_slice(&arguments.stack)
                    } else {
                        map_memory_from(&mut get_page_dir(Some(thread_id)), &mut process_page_dir, stack_pointer, arguments.stack.len(), |s| {
                            s.copy_from_slice(&arguments.stack)
                        })
                        .map_err(|_| Errno::OutOfMemory)?;
                    }
                }
            }

            let mut registers = crate::arch::Registers::new_task(handler.entry_point, stack_pointer);

            if arguments.should_write_registers {
                registers.call(&arguments)?;
            }

            let mut process = get_process(process_id.process).ok_or(Errno::NoSuchProcess)?;
            let thread = process.threads.get_mut(process_id.thread as usize).ok_or(Errno::NoSuchProcess)?;

            if on_cpu {
                thread.register_queue.current_mut().registers.transfer(regs);
            }

            thread.register_queue.push(RegisterQueueEntry {
                registers,
                message_num: Some(message),
                message_data: data,
            })?;

            if on_cpu {
                // we're already running this process, so just immediately switch to our new set of registers
                regs.transfer(&registers);
            } else {
                // queue this process for execution
                trace!("queueing process");
                let mut task_queue = cpu_thread.task_queue.lock();
                task_queue.remove_thread(process_id);

                let mut entry = super::queue::TaskQueueEntry::new(process_id, thread.priority);
                entry.set_sub_priority(handler.priority);
                task_queue.insert(entry)?;
            }

            Ok(())
        };

        if let Err(err) = finish_sending_message() {
            // if we encounter an error and had mapped data in prior, unmap the data before returning the error
            if let Some(addr) = data {
                if let Err(err) = process_page_dir.set_page(addr, None) {
                    warn!("couldn't free page while trying to clean up after failed send_message(): {err:?}");

                    return Err(Errno::OutOfMemory);
                }
            }

            return Err(err);
        }

        debug!("message sent");
    } else {
        // ask other CPU to handle this message

        let current_cpu = current_cpu.unwrap();

        let cpus = get_cpus().expect("CPUs not initialized");
        let thread = cpus.get_thread(current_cpu).expect("couldn't get CPU thread");

        debug!("forwarding message to CPU {current_cpu}");
        thread.send_message(super::cpu::Message::SendMessage { process: process_num, message, data })?;

        assert!(crate::arch::send_interrupt_to_cpu(current_cpu, crate::arch::MESSAGE_INT), "failed to send interrupt");
    }

    Ok(())
}
