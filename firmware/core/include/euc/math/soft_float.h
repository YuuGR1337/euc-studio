/*
 * Faster soft-float helpers for the target images: IEEE 754 binary64 division and binary32 comparisons on
 * bit patterns.
 *
 * The MCU has no FPU, so every floating-point operation is a library call. The toolchain's soft-float divide
 * (libgcc's __aeabi_ddiv) takes about 530 instructions and each float comparison about 29, which made the
 * compat image's 496 Hz tick twice as slow as the stock firmware's
 * (docs/models/soc50-full-run-equivalence.md). These routines give libgcc's results bit for bit in a
 * fraction of that:
 *
 *   - euc_f64_div: the correctly rounded quotient, round to nearest, ties to even; results below the normal
 *     range are denormal (gradual underflow), as libgcc does for doubles (the KEIL_FTZ flush applies only
 *     where a result becomes a float); NaN results follow libgcc's choice of NaN bit for bit (see
 *     soft_float.c);
 *   - euc_f32_less, euc_f32_less_equal: the IEEE ordered comparisons, false when either operand is a NaN,
 *     with -0 equal to +0.
 *
 * Target images route the compiler's __aeabi_ddiv and __aeabi_fcmp{lt,le,ge,gt} calls here with the linker's
 * --wrap option (cmake/image.cmake). Add, subtract and multiply stay libgcc's: they are already fast, and
 * libgcc's subtraction rounds a few rare cases differently from IEEE
 * (docs/models/soc50-full-run-equivalence.md), so replacing it would change the compat image's results. Host
 * builds (unit tests, SIL) keep the hardware operations, whose results are the same.
 */
#ifndef EUC_MATH_SOFT_FLOAT_H
#define EUC_MATH_SOFT_FLOAT_H

#include <stdbool.h>
#include <stdint.h>

/* a / b on IEEE 754 binary64 bit patterns. */
uint64_t euc_f64_div(uint64_t a, uint64_t b);

/* a < b and a <= b on IEEE 754 binary32 bit patterns. */
bool euc_f32_less(uint32_t a, uint32_t b);
bool euc_f32_less_equal(uint32_t a, uint32_t b);

#endif
