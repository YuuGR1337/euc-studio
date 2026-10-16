/*
 * Fixed-point types and saturating integer arithmetic for the integer control paths (FOC, halls, SVPWM).
 *
 * Formats
 *   q15_t   Q1.15 in int16_t: value = raw / 32768, range [-1, 1 - 2^-15].
 *           Currents, voltages and duties normalised to full scale (as in the stock motor code).
 *   q16_t   Q16.16 in int32_t: value = raw / 65536, range [-32768, 32768 - 2^-16].
 *           Gains and intermediate results that need integer headroom.
 *
 * Rules shared by every helper in this file
 *   - Results saturate to the range of the result type. Nothing wraps.
 *   - Multiplications and narrowing conversions drop fraction bits with an arithmetic shift right,
 *     which rounds toward minus infinity. The stock code does the same, so ports can be bit-exact.
 *   - Pure static inline functions: no division, no floating point, no state.
 *
 * C11 leaves the right shift of a negative value implementation-defined. GCC and Clang, the only compilers
 * this firmware supports, shift arithmetically; the assertion below stops any other compiler at build time.
 */
#ifndef EUC_MATH_FIXED_H
#define EUC_MATH_FIXED_H

#include <stdint.h>

/* cppcheck-suppress shiftNegativeLHS ; this line checks exactly that behaviour */
_Static_assert((-1 >> 1) == -1, "fixed-point helpers need an arithmetic right shift of signed values");

typedef int16_t q15_t;
typedef int32_t q16_t;

#define Q15_FRAC_BITS 15
#define Q15_MAX       ((q15_t)INT16_MAX) /* 1 - 2^-15 */
#define Q15_MIN       ((q15_t)INT16_MIN) /* -1 */

#define Q16_FRAC_BITS 16
#define Q16_ONE       ((q16_t)65536)
#define Q16_MAX       ((q16_t)INT32_MAX)
#define Q16_MIN       ((q16_t)INT32_MIN)

/* Generic saturation ----------------------------------------------------------------------------------- */

/* Clamps a 32-bit value to the int16_t range. */
static inline int16_t sat_i32_to_i16(int32_t value)
{
    if (value > INT16_MAX) {
        return INT16_MAX;
    }
    if (value < INT16_MIN) {
        return INT16_MIN;
    }
    return (int16_t)value;
}

/* Clamps a 64-bit value to the int32_t range. */
static inline int32_t sat_i64_to_i32(int64_t value)
{
    if (value > INT32_MAX) {
        return INT32_MAX;
    }
    if (value < INT32_MIN) {
        return INT32_MIN;
    }
    return (int32_t)value;
}

/* Clamps value to [min_value, max_value]. The caller guarantees min_value <= max_value. */
static inline int32_t clamp_i32(int32_t value, int32_t min_value, int32_t max_value)
{
    if (value > max_value) {
        return max_value;
    }
    if (value < min_value) {
        return min_value;
    }
    return value;
}

/* Q1.15 ------------------------------------------------------------------------------------------------ */

static inline q15_t q15_add_sat(q15_t a, q15_t b)
{
    return sat_i32_to_i16((int32_t)a + (int32_t)b);
}

static inline q15_t q15_sub_sat(q15_t a, q15_t b)
{
    return sat_i32_to_i16((int32_t)a - (int32_t)b);
}

/* -(-1) does not fit in Q1.15; it saturates to Q15_MAX. */
static inline q15_t q15_neg_sat(q15_t a)
{
    return sat_i32_to_i16(-(int32_t)a);
}

static inline q15_t q15_abs_sat(q15_t a)
{
    return (a < 0) ? q15_neg_sat(a) : a;
}

/* a * b, rounded toward minus infinity. Only (-1) * (-1) saturates. */
static inline q15_t q15_mul_sat(q15_t a, q15_t b)
{
    const int32_t product_q30 = (int32_t)a * (int32_t)b;
    return sat_i32_to_i16(product_q30 >> Q15_FRAC_BITS);
}

/* Widens Q1.15 to Q16.16 without loss. */
static inline q16_t q16_from_q15(q15_t value)
{
    return (q16_t)value * 2;
}

/* Narrows Q16.16 to Q1.15: drops one fraction bit (toward minus infinity) and saturates outside [-1, 1). */
static inline q15_t q15_from_q16_sat(q16_t value)
{
    return sat_i32_to_i16(value >> (Q16_FRAC_BITS - Q15_FRAC_BITS));
}

/* Q16.16 ----------------------------------------------------------------------------------------------- */

static inline q16_t q16_add_sat(q16_t a, q16_t b)
{
    return sat_i64_to_i32((int64_t)a + (int64_t)b);
}

static inline q16_t q16_sub_sat(q16_t a, q16_t b)
{
    return sat_i64_to_i32((int64_t)a - (int64_t)b);
}

/* a * b, rounded toward minus infinity. */
static inline q16_t q16_mul_sat(q16_t a, q16_t b)
{
    const int64_t product_q32 = (int64_t)a * (int64_t)b;
    return sat_i64_to_i32(product_q32 >> Q16_FRAC_BITS);
}

/* Integer to Q16.16; saturates outside [-32768, 32767]. */
static inline q16_t q16_from_int_sat(int32_t value)
{
    return sat_i64_to_i32((int64_t)value * Q16_ONE);
}

/* Integer part, rounded toward minus infinity (-0.5 gives -1). */
static inline int32_t q16_to_int_floor(q16_t value)
{
    return value >> Q16_FRAC_BITS;
}

#endif
