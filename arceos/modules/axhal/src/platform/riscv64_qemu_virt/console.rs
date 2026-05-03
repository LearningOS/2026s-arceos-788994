/// Writes a byte to the console.
pub fn putchar(c: u8) {
    #[allow(deprecated)]
    sbi_rt::legacy::console_putchar(c as usize);
}

/// Reads a byte from the console, or returns [`None`] if no input is available.
pub fn getchar() -> Option<u8> {
    #[allow(deprecated)]
    match sbi_rt::legacy::console_getchar() as isize {
        -1 => None,
        c => Some(c as u8),
    }
}

pub fn write_bytes(buf: &[u8]) -> usize {
    // \x1b[31m 是红色，\x1b[0m 是重置颜色[cite: 5]
    crate::platform::riscv64_qemu_virt::console::putchar(b'\x1b');
    crate::platform::riscv64_qemu_virt::console::putchar(b'[');
    crate::platform::riscv64_qemu_virt::console::putchar(b'3');
    crate::platform::riscv64_qemu_virt::console::putchar(b'1');
    crate::platform::riscv64_qemu_virt::console::putchar(b'm');

    for &c in buf {
        crate::platform::riscv64_qemu_virt::console::putchar(c);
    }

    crate::platform::riscv64_qemu_virt::console::putchar(b'\x1b');
    crate::platform::riscv64_qemu_virt::console::putchar(b'[');
    crate::platform::riscv64_qemu_virt::console::putchar(b'0');
    crate::platform::riscv64_qemu_virt::console::putchar(b'm');
    buf.len()
}
