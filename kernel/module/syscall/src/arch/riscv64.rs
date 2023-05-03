macro_rules! syscall {
    ($($name:ident($a:ident, $($b:ident, $($c:ident, $($d:ident, $($e:ident, $($f:ident, $($g:ident, $($h:ident, )?)?)?)?)?)?)?);)+) => {
        $(
            pub unsafe fn $name($a: usize, $($b: usize, $($c: usize, $($d: usize, $($e: usize, $($f: usize, $($g: usize, $($h: usize, )?)?)?)?)?)?)?) -> isize {
                let _ret: isize;

                core::arch::asm!(
                    "ecall",
                    in("a7") $a,
                    $(
                        in("a0") $b,
                        $(
                            in("a1") $c,
                            $(
                                in("a2") $d,
                                $(
                                    in("a3") $e,
                                    $(
                                        in("a4") $f,
                                        $(
                                            in("a5") $g,
                                            $(
                                                in("a6") $h,
                                            )?
                                        )?
                                    )?
                                )?
                            )?
                        )?
                    )?
                    lateout("a0") _ret,
                    options(nostack),
                );
                _ret
            }
        )+
    };
}

syscall! {
    syscall0(a,z, );
    syscall1(a, b, z, );
    syscall2(a, b, c, z, );
    syscall3(a, b, c, d, z, );
    syscall4(a, b, c, d, e, z, );
    syscall5(a, b, c, d, e, f, z, );
    syscall6(a, b, c, d, e, f, g, z, );
}
