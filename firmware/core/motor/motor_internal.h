/*
 * Helpers shared by the motor modules. Not part of the public interface.
 */
#ifndef EUC_MOTOR_INTERNAL_H
#define EUC_MOTOR_INTERNAL_H

#include <stdint.h>

/*
 * Two's-complement wrap to 16 bits, for values that are meant to wrap (electrical angles, step counters) and
 * for the stock narrowing behind quirk INT16_WRAP. Written with defined arithmetic, so no conversion of an
 * out-of-range value is implementation-defined: (value + 32768) mod 65536, taken from the unsigned sum
 * (2^32 is a multiple of 65536), minus 32768. A few instructions, so the FOC interrupt's callers inline it.
 */
static inline int16_t motor_wrap_i16(int32_t value)
{
    const uint32_t offset = ((uint32_t)value + 32768U) & 0xFFFFU;
    return (int16_t)((int32_t)offset - 32768);
}

#endif
