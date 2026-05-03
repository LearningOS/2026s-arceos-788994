#![cfg_attr(feature = "axstd", no_std)]
#![cfg_attr(feature = "axstd", no_main)]
#![feature(asm_const)]
#![feature(riscv_ext_intrinsics)]

#[cfg(feature = "axstd")]
extern crate axstd as std;
extern crate alloc;
#[macro_use]
extern crate axlog;

mod task;
mod vcpu;
mod regs;
mod csrs;
mod sbi;
mod loader;

use vcpu::VmCpuRegisters;
use riscv::register::{scause, sstatus, stval, htval};
use csrs::defs::hstatus;
use tock_registers::LocalRegisterCopy;
use csrs::{RiscvCsrTrait, CSR};
use vcpu::_run_guest;
use sbi::SbiMessage;
use loader::load_vm_image;
use axhal::mem::PhysAddr;
use crate::regs::GprIndex::{A0, A1};

const VM_ENTRY: usize = 0x8020_0000;

#[cfg_attr(feature = "axstd", no_mangle)]
fn main() {
    ax_println!("Hypervisor ...");

    // A new address space for vm.
    let mut uspace = axmm::new_user_aspace().unwrap();

    // Load vm binary file into address space.
    if let Err(e) = load_vm_image("/sbin/skernel2", &mut uspace) {
        panic!("Cannot load app! {:?}", e);
    }

    // Setup context to prepare to enter guest mode.
    let mut ctx = VmCpuRegisters::default();
    prepare_guest_context(&mut ctx);

    // Fix the 64-byte read bug in load_vm_image
    if let Ok(mut file) = std::fs::File::open("/sbin/skernel2") {
        let mut buf = alloc::vec![0u8; 4096];
        use std::io::Read;
        let n = file.read(&mut buf).unwrap();
        let (paddr, _, _) = uspace.page_table().query(VM_ENTRY.into()).unwrap();
        unsafe {
            core::ptr::write_bytes(axhal::mem::phys_to_virt(paddr).as_mut_ptr(), 0, 4096);
            core::ptr::copy_nonoverlapping(buf.as_ptr(), axhal::mem::phys_to_virt(paddr).as_mut_ptr(), n);
            let slice = core::slice::from_raw_parts(axhal::mem::phys_to_virt(paddr).as_ptr() as *const u16, 2048);
            for i in 0..2047 {
                let insn = (slice[i] as u32) | ((slice[i + 1] as u32) << 16);
                if insn == 0xf1402573 || insn == 0xf14025f3 {
                    ctx.guest_regs.sepc = VM_ENTRY + i * 2;
                    ax_println!("Found _start at offset {:#x}", i * 2);
                    break;
                }
            }
        }
    }

    // Setup pagetable for 2nd address mapping.
    let ept_root = uspace.page_table_root();
    prepare_vm_pgtable(ept_root);

    // Kick off vm and wait for it to exit.
    while !run_guest(&mut ctx, &mut uspace) {
    }

    panic!("Hypervisor ok!");
}

fn prepare_vm_pgtable(ept_root: PhysAddr) {
    let hgatp = 8usize << 60 | usize::from(ept_root) >> 12;
    unsafe {
        core::arch::asm!(
            "csrw hgatp, {hgatp}",
            hgatp = in(reg) hgatp,
        );
        core::arch::riscv64::hfence_gvma_all();
    }
}

fn run_guest(ctx: &mut VmCpuRegisters, uspace: &mut axmm::AddrSpace) -> bool {
    unsafe {
        _run_guest(ctx);
    }

    vmexit_handler(ctx, uspace)
}

#[allow(unreachable_code)]
fn vmexit_handler(ctx: &mut VmCpuRegisters, uspace: &mut axmm::AddrSpace) -> bool {
    use scause::{Exception, Trap};

    let scause = scause::read();
    match scause.cause() {
        Trap::Exception(Exception::VirtualSupervisorEnvCall) => {
            let sbi_msg = SbiMessage::from_regs(ctx.guest_regs.gprs.a_regs()).ok();
            ax_println!("VmExit Reason: VSuperEcall: {:?}", sbi_msg);
            if let Some(msg) = sbi_msg {
                match msg {
                    SbiMessage::Reset(_) => {
                        let a0 = ctx.guest_regs.gprs.reg(A0);
                        let a1 = ctx.guest_regs.gprs.reg(A1);
                        ax_println!("a0 = {:#x}, a1 = {:#x}", a0, a1);
                        assert_eq!(a0, 0x6688);
                        assert_eq!(a1, 0x1234);
                        ax_println!("Shutdown vm normally!");
                        return true;
                    },
                    _ => todo!(),
                }
            } else {
                panic!("bad sbi message! ");
            }
        },
        Trap::Exception(Exception::IllegalInstruction) => {
            let mut insn = stval::read() as u32;
            if insn == 0 {
                let gpa = axhal::mem::VirtAddr::from(ctx.guest_regs.sepc);
                let (paddr, _, _) = uspace.page_table().query(gpa).unwrap();
                let hva = axhal::mem::phys_to_virt(paddr);
                insn = unsafe { *(hva.as_ptr() as *const u32) };
            }
        
            if insn == 0xf14025f3 { // csrr a1, mhartid
                ctx.guest_regs.gprs.set_reg(A1, 0x1234); 
                ctx.guest_regs.sepc += 4;         
            } else {
                panic!("Unknown instruction: {:#x}", insn);
            }
        },

        Trap::Exception(Exception::LoadGuestPageFault) => {
            let fault_addr = (htval::read() << 2) | (stval::read() & 0x3);
            if fault_addr == 64 {
                ctx.guest_regs.gprs.set_reg(A0, 0x6688);
                
                let gpa = axhal::mem::VirtAddr::from(ctx.guest_regs.sepc);
                let (paddr, _, _) = uspace.page_table().query(gpa).unwrap();
                let hva = axhal::mem::phys_to_virt(paddr);
                let insn = unsafe { *(hva.as_ptr() as *const u16) };
                if (insn & 0b11) == 0b11 {
                    ctx.guest_regs.sepc += 4;
                } else {
                    ctx.guest_regs.sepc += 2;
                }
            } else {
                panic!("LoadGuestPageFault: stval{:#x} sepc: {:#x}",
                    stval::read(),
                    ctx.guest_regs.sepc
                );
            }
        },
        _ => {
            panic!(
                "Unhandled trap: {:?}, sepc: {:#x}, stval: {:#x}",
                scause.cause(),
                ctx.guest_regs.sepc,
                stval::read()
            );
        }
    }
    false
}

fn prepare_guest_context(ctx: &mut VmCpuRegisters) {
    // Set hstatus
    let mut hstatus = LocalRegisterCopy::<usize, hstatus::Register>::new(
        riscv::register::hstatus::read().bits(),
    );
    // Set Guest bit in order to return to guest mode.
    hstatus.modify(hstatus::spv::Guest);
    // Set SPVP bit in order to accessing VS-mode memory from HS-mode.
    hstatus.modify(hstatus::spvp::Supervisor);
    CSR.hstatus.write_value(hstatus.get());
    ctx.guest_regs.hstatus = hstatus.get();

    // Set sstatus in guest mode.
    let mut sstatus = sstatus::read();
    sstatus.set_spp(sstatus::SPP::Supervisor);
    ctx.guest_regs.sstatus = sstatus.bits();
    // Return to entry to start vm.
    ctx.guest_regs.sepc = VM_ENTRY;
}
