/*
 * Faster soft-float helpers for the target images (see euc/math/soft_float.h): IEEE 754 binary64 division
 * and binary32 comparisons, with libgcc's results bit for bit.
 *
 * Written for this project from the IEEE 754 rules; no library or vendor code is used. The quotient comes
 * from a 32-bit reciprocal of the divisor and two 28-bit digits, each corrected with an exact remainder, so
 * it is the correctly rounded quotient whatever the estimates' error. Comparisons order the sign-magnitude
 * encodings as unsigned numbers.
 *
 * Verification: tests/unit/test_soft_float.c compares every routine with the host's hardware operations on
 * random and constructed operands (zeros, signs, infinities, NaNs, denormals and the normal boundary, exact
 * ties, the largest and smallest exponents), and NaN results with libgcc's rules. A private target harness
 * compared them bit for bit with libgcc's __aeabi_ddiv and __aeabi_fcmp{lt,le,ge,gt} on the simulator's
 * Cortex-M3 core (docs/models/soc50-full-run-equivalence.md records the counts).
 */
#include "euc/math/soft_float.h"

#include <stdbool.h>
#include <stdint.h>

#define SIGN_BIT      0x8000000000000000ULL
#define INFINITY_BITS 0x7FF0000000000000ULL
#define FRACTION      0x000FFFFFFFFFFFFFULL
#define HIDDEN_BIT    0x0010000000000000ULL
#define QUIET_BIT     0x0008000000000000ULL
#define DEFAULT_NAN   0x7FF8000000000000ULL
#define EXPONENT_MAX  0x7FFU
#define NEGATIVE      0x8000000000000000ULL /* a remainder at or above this is negative */

/* Ten extra bits below the result's last place: the round bit, eight more, and the sticky bit. */
#define EXTRA_BITS 10U
#define HALF_ULP   0x200U
#define EXTRA_MASK 0x3FFU

/* The comparisons are a dozen instructions: with -Os GCC would rather call them than copy them into each
 * entry point, and the call costs a third again. */
#if defined(__GNUC__)
#define ALWAYS_INLINE __attribute__((always_inline))
#else
#define ALWAYS_INLINE
#endif

#define F32_SIGN        0x80000000U
#define F32_INFINITY_X2 0xFF000000U /* infinity's bits shifted left past the sign: larger means NaN */

static uint32_t exponent_of(uint64_t x)
{
    return (uint32_t)(x >> 52U) & EXPONENT_MAX;
}

static bool is_nan(uint64_t x)
{
    return (exponent_of(x) == EXPONENT_MAX) && ((x & FRACTION) != 0U);
}

static bool is_zero(uint64_t x)
{
    return (x << 1U) == 0U;
}

/* x >> n for n >= 1, with every bit shifted out folded into bit 0. */
static uint64_t shift_right_sticky(uint64_t x, uint32_t n)
{
    uint64_t result;
    if (n >= 64U) {
        result = (x != 0U) ? 1ULL : 0ULL;
    } else {
        const uint32_t kept = 64U - n;
        const uint64_t lost = x << kept;
        result = (x >> n) | ((lost != 0U) ? 1ULL : 0ULL);
    }
    return result;
}

/*
 * Rounds and packs sign * significand * 2^(exponent - 1023 - 62): significand has its leading one at bit 62
 * and EXTRA_BITS bits below the result's last place, the lowest of them sticky. The exponent may lie outside
 * the normal range: above it the result is infinite, below it the significand is shifted into a denormal
 * first and rounded once (round to nearest, ties to even). Adding the significand, hidden bit included, to
 * the exponent field lets a carry out of rounding step the exponent, up to infinity.
 */
static uint64_t round_pack(uint64_t sign, int32_t exponent, uint64_t significand)
{
    uint64_t sig = significand;
    int32_t exp = exponent;
    if (exp >= (int32_t)EXPONENT_MAX) {
        return sign | INFINITY_BITS;
    }
    if (exp < 1) {
        const uint32_t shift = 1U - (uint32_t)exp;
        sig = shift_right_sticky(sig, shift);
        exp = 1;
    }
    const uint32_t rest = (uint32_t)sig & EXTRA_MASK;
    sig >>= EXTRA_BITS;
    if ((rest > HALF_ULP) || ((rest == HALF_ULP) && ((sig & 1U) != 0U))) {
        sig++;
    }
    const uint64_t biased = (uint64_t)(uint32_t)exp - 1U;
    return sign | ((biased << 52U) + sig);
}

/*
 * A finite, nonzero operand as significand * 2^(exponent - 1075) with the significand in [2^52, 2^53).
 * Denormals are normalised, so their exponent goes below 1.
 */
static int32_t unpack(uint64_t x, uint64_t *significand)
{
    int32_t exp = (int32_t)exponent_of(x);
    uint64_t sig = x & FRACTION;
    if (exp == 0) {
        exp = 1;
        while ((sig & HIDDEN_BIT) == 0U) {
            sig <<= 1U;
            exp--;
        }
    } else {
        sig |= HIDDEN_BIT;
    }
    *significand = sig;
    return exp;
}

/*
 * About 2^63 / y for y in [2^31, 2^32), within a few parts in 2^30: a 32-by-16-bit hardware divide gives
 * 15 bits, and one Newton step r += r (2^63 - y r) / 2^63 doubles them.
 */
static uint32_t reciprocal(uint32_t y)
{
    const uint32_t start = (0x7FFFFFFFU / (y >> 16U)) << 16U;
    const uint64_t product = (uint64_t)y * (uint64_t)start;
    const uint64_t half = 0x8000000000000000ULL;
    uint64_t estimate;
    if (product <= half) {
        const uint64_t error = half - product; /* below 2^49 */
        estimate = (uint64_t)start + (((uint64_t)start * (error >> 31U)) >> 32U);
    } else {
        const uint64_t error = product - half;
        estimate = (uint64_t)start - (((uint64_t)start * (error >> 31U)) >> 32U);
    }
    return (estimate > (uint64_t)UINT32_MAX) ? UINT32_MAX : (uint32_t)estimate;
}

/*
 * floor(numerator * 2^shift_in / divisor) for divisor in [2^52, 2^53), numerator below 2^(81 - shift_in)
 * and a quotient below 2^28 + 2^27; *remainder gets the remainder. recip is about 2^84 / divisor, so
 * (numerator >> (49 - shift_in)) * recip >> 35 estimates the digit to a few units. The remainder is computed
 * modulo 2^64: its true value is small, so a value at or above 2^63 means it is negative, and the corrections
 * bring it into [0, divisor).
 */
static uint32_t digit(uint64_t numerator, uint32_t shift_in, uint64_t divisor, uint32_t recip,
                      uint64_t *remainder)
{
    const uint32_t drop = 49U - shift_in;
    const uint64_t top = numerator >> drop;
    uint32_t q = (uint32_t)(((uint64_t)(uint32_t)top * (uint64_t)recip) >> 35U);
    uint64_t rem = (numerator << shift_in) - ((uint64_t)q * divisor);
    while (rem >= NEGATIVE) {
        q--;
        rem += divisor;
    }
    while (rem >= divisor) {
        q++;
        rem -= divisor;
    }
    *remainder = rem;
    return q;
}

/*
 * Zeros, infinities and NaNs. NaN results follow libgcc's __aeabi_ddiv bit for bit (measured on the Cortex-M3
 * core): a NaN dividend comes back quiet with its sign and payload; an infinite dividend over a NaN or an
 * infinity comes back as inf | quiet with its sign; a NaN divisor otherwise comes back quiet; 0 / 0 is a
 * quiet NaN with the dividend's sign. Returns true with *result set for these; false for two finite, nonzero
 * operands.
 */
static bool divide_special(uint64_t a, uint64_t b, uint64_t sign, uint64_t *result)
{
    bool special = true;
    if (exponent_of(a) == EXPONENT_MAX) {
        *result = (is_nan(a) || (exponent_of(b) == EXPONENT_MAX)) ? (a | QUIET_BIT) : (sign | INFINITY_BITS);
    } else if (exponent_of(b) == EXPONENT_MAX) {
        *result = is_nan(b) ? (b | QUIET_BIT) : sign; /* x / NaN; x / infinity = 0 */
    } else if (is_zero(b)) {
        *result = is_zero(a) ? (a | DEFAULT_NAN) : (sign | INFINITY_BITS);
    } else if (is_zero(a)) {
        *result = sign;
    } else {
        special = false;
    }
    return special;
}

uint64_t euc_f64_div(uint64_t a, uint64_t b)
{
    const uint64_t sign = (a ^ b) & SIGN_BIT;
    uint64_t ma;
    uint64_t mb;
    int32_t exp_a;
    int32_t exp_b;
    const uint32_t biased_a = exponent_of(a);
    const uint32_t biased_b = exponent_of(b);
    if (((biased_a - 1U) < (EXPONENT_MAX - 1U)) && ((biased_b - 1U) < (EXPONENT_MAX - 1U))) {
        /* Both normal, the usual case: exponents 1..0x7FE. */
        ma = (a & FRACTION) | HIDDEN_BIT;
        mb = (b & FRACTION) | HIDDEN_BIT;
        exp_a = (int32_t)biased_a;
        exp_b = (int32_t)biased_b;
    } else {
        uint64_t result;
        if (divide_special(a, b, sign, &result)) {
            return result;
        }
        exp_a = unpack(a, &ma);
        exp_b = unpack(b, &mb);
    }
    int32_t exp = (exp_a - exp_b) + 1023;
    if (ma < mb) {
        ma <<= 1U;
        exp--;
    }

    /*
     * ma / mb lies in [1, 2). The 56-bit quotient floor(ma * 2^55 / mb) is two digits: 28 bits plus the
     * leading one, then 28 bits from the remainder. Both use the same reciprocal of mb's top 32 bits.
     */
    const uint32_t recip = reciprocal((uint32_t)(mb >> 21U));
    uint64_t remainder;
    const uint32_t high = digit(ma, 27U, mb, recip, &remainder);
    const uint32_t low = digit(remainder, 28U, mb, recip, &remainder);
    const uint64_t quotient = ((uint64_t)high << 28U) | (uint64_t)low;

    /* The quotient's leading one is at bit 55; move it to bit 62 and add the sticky bit. */
    uint64_t sig = quotient << 7U;
    if (remainder != 0U) {
        sig |= 1U;
    }
    return round_pack(sign, exp, sig);
}

/* binary32 comparisons ---------------------------------------------------------------------------------- */

static inline bool f32_unordered(uint32_t a, uint32_t b) ALWAYS_INLINE;
static inline bool f32_unordered(uint32_t a, uint32_t b)
{
    return ((a << 1U) > F32_INFINITY_X2) || ((b << 1U) > F32_INFINITY_X2);
}

/*
 * The sign-magnitude encoding orders like an unsigned number for two negatives reversed, and like the
 * encoding with its sign bit flipped otherwise (any negative below any non-negative). Both zeros compare
 * equal, whatever their signs.
 */
static inline bool f32_less(uint32_t a, uint32_t b) ALWAYS_INLINE;
static inline bool f32_less(uint32_t a, uint32_t b)
{
    bool less;
    if (f32_unordered(a, b) || (((a | b) << 1U) == 0U)) {
        less = false; /* a NaN, or +-0 against +-0 */
    } else if (((a & b) & F32_SIGN) != 0U) {
        less = a > b; /* both negative: the larger magnitude is the smaller value */
    } else {
        less = (a ^ F32_SIGN) < (b ^ F32_SIGN);
    }
    return less;
}

static inline bool f32_less_equal(uint32_t a, uint32_t b) ALWAYS_INLINE;
static inline bool f32_less_equal(uint32_t a, uint32_t b)
{
    bool less_equal;
    if (f32_unordered(a, b)) {
        less_equal = false;
    } else if (((a | b) << 1U) == 0U) {
        less_equal = true; /* +-0 against +-0 */
    } else if (((a & b) & F32_SIGN) != 0U) {
        less_equal = a >= b;
    } else {
        less_equal = (a ^ F32_SIGN) <= (b ^ F32_SIGN);
    }
    return less_equal;
}

bool euc_f32_less(uint32_t a, uint32_t b)
{
    return f32_less(a, b);
}

bool euc_f32_less_equal(uint32_t a, uint32_t b)
{
    return f32_less_equal(a, b);
}

/* Target entry points ---------------------------------------------------------------------------------- */

#if defined(__arm__) && defined(__SOFTFP__)

/*
 * The EABI helpers, reached through the linker's --wrap options (cmake/image.cmake). With the soft-float
 * calling convention a double travels in r0:r1 like a uint64_t and a float in r0 like a uint32_t, so the bit
 * patterns pass unchanged.
 */
union f64_bits {
    double as_double;
    uint64_t as_bits;
};

union f32_bits {
    float as_float;
    uint32_t as_bits;
};

/* NOLINTBEGIN(bugprone-reserved-identifier,cert-dcl37-c,cert-dcl51-cpp): the linker's --wrap fixes the
 * __wrap___aeabi_* names. */
double __wrap___aeabi_ddiv(double a, double b);
int __wrap___aeabi_fcmplt(float a, float b);
int __wrap___aeabi_fcmple(float a, float b);
int __wrap___aeabi_fcmpge(float a, float b);
int __wrap___aeabi_fcmpgt(float a, float b);

double __wrap___aeabi_ddiv(double a, double b)
{
    const union f64_bits x = {.as_double = a};
    const union f64_bits y = {.as_double = b};
    const union f64_bits result = {.as_bits = euc_f64_div(x.as_bits, y.as_bits)};
    return result.as_double;
}

int __wrap___aeabi_fcmplt(float a, float b)
{
    const union f32_bits x = {.as_float = a};
    const union f32_bits y = {.as_float = b};
    return f32_less(x.as_bits, y.as_bits) ? 1 : 0;
}

int __wrap___aeabi_fcmple(float a, float b)
{
    const union f32_bits x = {.as_float = a};
    const union f32_bits y = {.as_float = b};
    return f32_less_equal(x.as_bits, y.as_bits) ? 1 : 0;
}

int __wrap___aeabi_fcmpge(float a, float b)
{
    const union f32_bits x = {.as_float = a};
    const union f32_bits y = {.as_float = b};
    return f32_less_equal(y.as_bits, x.as_bits) ? 1 : 0;
}

int __wrap___aeabi_fcmpgt(float a, float b)
{
    const union f32_bits x = {.as_float = a};
    const union f32_bits y = {.as_float = b};
    return f32_less(y.as_bits, x.as_bits) ? 1 : 0;
}

/* NOLINTEND(bugprone-reserved-identifier,cert-dcl37-c,cert-dcl51-cpp) */

#endif
