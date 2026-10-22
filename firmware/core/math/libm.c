/*
 * sqrt, asin, atan and atan2 for the firmware (see euc/math/libm.h).
 *
 * euc_asin, euc_atan, euc_atan2 and euc_log are derived from fdlibm 5.3 (e_asin.c, s_atan.c, e_atan2.c,
 * e_log.c). They keep the fdlibm algorithms, constants and order of operations, because the stock firmware's
 * Keil library does the same and the ports must round alike. fdlibm carries this notice:
 *
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice
 * is preserved.
 * ====================================================
 *
 * euc_sqrt is our own: the exact integer square root of the scaled mantissa (a reciprocal-square-root
 * estimate refined to two corrected digits), rounded to nearest even. It replaced a digit-by-digit loop of
 * about 600 instructions; a target harness compared the two bit for bit on the simulator's Cortex-M3 core,
 * together with euc_asin over a dense sweep of [-1, 1] (docs/models/soc50-full-run-equivalence.md).
 */
#include "euc/math/libm.h"

#include <stdbool.h>
#include <stdint.h>

/* IEEE 754 double words -------------------------------------------------------------------------------- */

static uint64_t f64_bits(double value)
{
    union {
        double as_double;
        uint64_t as_bits;
    } pun = {.as_double = value};
    return pun.as_bits;
}

static double f64_from_bits(uint64_t bits)
{
    union {
        uint64_t as_bits;
        double as_double;
    } pun = {.as_bits = bits};
    return pun.as_double;
}

/* fdlibm's __HI: sign, exponent and top 20 mantissa bits. */
static int32_t high_word(double value)
{
    return (int32_t)(uint32_t)(f64_bits(value) >> 32U);
}

/* fdlibm's __LO: low 32 mantissa bits. */
static uint32_t low_word(double value)
{
    return (uint32_t)(f64_bits(value) & 0xFFFFFFFFU);
}

static double f64_abs(double value)
{
    return f64_from_bits(f64_bits(value) & 0x7FFFFFFFFFFFFFFFU);
}

static double f64_quiet_nan(void)
{
    return f64_from_bits(0x7FF8000000000000U);
}

#define F64_MANTISSA_BITS 52U
#define F64_EXPONENT_MASK 0x7FFU
#define F64_EXPONENT_BIAS 1023
#define F64_HIDDEN_BIT    ((uint64_t)1U << F64_MANTISSA_BITS)
#define F64_MANTISSA_MASK (F64_HIDDEN_BIT - 1U)

/* sqrt ------------------------------------------------------------------------------------------------- */

/*
 * 1 / sqrt(x) in Q1.15 at the middle of each 1/128 step of x in [0.25, 1):
 * round(2^15 / sqrt((i + 32.5) / 128)) for i = 0 .. 95.
 */
static const uint16_t rsqrt_start[96] = {
    65030U, 64052U, 63117U, 62222U, 61363U, 60540U, 59748U, 58987U, 58254U, 57548U, 56867U, 56210U,
    55574U, 54960U, 54366U, 53791U, 53233U, 52693U, 52169U, 51660U, 51165U, 50685U, 50218U, 49763U,
    49321U, 48890U, 48470U, 48061U, 47663U, 47273U, 46894U, 46523U, 46161U, 45807U, 45462U, 45124U,
    44793U, 44470U, 44153U, 43843U, 43540U, 43243U, 42951U, 42666U, 42386U, 42112U, 41843U, 41579U,
    41320U, 41065U, 40816U, 40571U, 40330U, 40093U, 39861U, 39632U, 39408U, 39187U, 38970U, 38756U,
    38546U, 38340U, 38136U, 37936U, 37739U, 37545U, 37354U, 37166U, 36980U, 36798U, 36618U, 36441U,
    36266U, 36093U, 35924U, 35756U, 35591U, 35428U, 35267U, 35109U, 34953U, 34798U, 34646U, 34496U,
    34347U, 34201U, 34056U, 33913U, 33772U, 33633U, 33496U, 33360U, 33225U, 33093U, 32962U, 32832U,
};

/*
 * root = floor(sqrt(m * 2^56)) for m in [2^52, 2^54), exactly; returns true when m * 2^56 - root^2 is not
 * zero. A table and two Newton steps give 1 / sqrt(m) to about 26 bits; the root then comes as two digits,
 * floor(sqrt(m)) and the 28 bits below it, each estimated with that reciprocal and corrected with the exact
 * remainder (computed modulo 2^64 because its true value is small).
 */
static bool sqrt_root(uint64_t m, uint64_t *root_out)
{
    const uint32_t x = (uint32_t)(m >> 22U);                     /* m / 2^54 in Q0.32, in [2^30, 2^32) */
    uint32_t r = (uint32_t)rsqrt_start[(x >> 25U) - 32U] << 15U; /* 1 / sqrt(x) in Q2.30 */
    for (uint32_t step = 0U; step < 2U; step++) {
        const uint64_t r_squared = ((uint64_t)r * r) >> 30U;                       /* R^2, Q.30 */
        const uint32_t x_r_squared = (uint32_t)(((uint64_t)x * r_squared) >> 32U); /* X R^2, Q.30 */
        r = (uint32_t)(((uint64_t)r * (0xC0000000U - x_r_squared)) >> 31U);        /* R (3 - X R^2) / 2 */
    }

    /* floor(sqrt(m)) in [2^26, 2^27): sqrt(m) = x * r / 2^35. */
    uint64_t high = ((uint64_t)x * r) >> 35U;
    int64_t rem = (int64_t)(m - (high * high));
    while (rem < 0) {
        high--;
        rem += (int64_t)((2U * high) + 1U);
    }
    while (rem > (int64_t)(2U * high)) {
        rem -= (int64_t)((2U * high) + 1U);
        high++;
    }

    /* The 28 bits below: about rem * 2^56 / (2 * high * 2^28) = rem * r / 2^30. */
    uint64_t root = (high << 28U) + (((uint64_t)rem * r) >> 30U);
    int64_t rem2 = (int64_t)((m << 56U) - (root * root));
    while (rem2 < 0) {
        root--;
        rem2 += (int64_t)((2U * root) + 1U);
    }
    while (rem2 > (int64_t)(2U * root)) {
        rem2 -= (int64_t)((2U * root) + 1U);
        root++;
    }
    *root_out = root;
    return rem2 != 0;
}

double euc_sqrt(double x)
{
    const uint64_t bits = f64_bits(x);
    const bool negative = (bits >> 63U) != 0U;
    int32_t exponent = (int32_t)((bits >> F64_MANTISSA_BITS) & F64_EXPONENT_MASK);
    uint64_t mantissa = bits & F64_MANTISSA_MASK;

    if ((exponent == 0) && (mantissa == 0U)) {
        return x; /* +0 and -0 */
    }
    if (exponent == (int32_t)F64_EXPONENT_MASK) {
        return ((mantissa == 0U) && negative) ? f64_quiet_nan() : x; /* NaN, +inf, -inf */
    }
    if (negative) {
        return f64_quiet_nan();
    }
    if (exponent == 0) {
        exponent = 1; /* denormal: normalise */
        while ((mantissa & F64_HIDDEN_BIT) == 0U) {
            mantissa <<= 1U;
            exponent--;
        }
    } else {
        mantissa |= F64_HIDDEN_BIT;
    }

    /* x = mantissa * 2^power with an even power, so sqrt(x) = sqrt(mantissa) * 2^(power / 2). */
    int32_t power = exponent - F64_EXPONENT_BIAS - (int32_t)F64_MANTISSA_BITS;
    if ((power % 2) != 0) {
        mantissa <<= 1U;
        power--;
    }

    /* root = floor(sqrt(mantissa * 2^56)): 55 bits, the 53 of the result and two below it. */
    uint64_t root;
    const bool inexact = sqrt_root(mantissa, &root);

    uint64_t result = root >> 2U;
    const uint64_t dropped_bits = root & 3U;
    const bool round_up = (dropped_bits > 2U) || ((dropped_bits == 2U) && (inexact || ((result & 1U) != 0U)));
    int32_t result_exponent = (power / 2) + 26 + F64_EXPONENT_BIAS;
    if (round_up) {
        result++;
        if (result == (F64_HIDDEN_BIT << 1U)) {
            result >>= 1U;
            result_exponent++;
        }
    }
    return f64_from_bits(((uint64_t)(uint32_t)result_exponent << F64_MANTISSA_BITS) |
                         (result & F64_MANTISSA_MASK));
}

/* asin (fdlibm e_asin.c) ------------------------------------------------------------------------------- */

static const double one = 1.00000000000000000000e+00;
static const double pio2_hi = 1.57079632679489655800e+00;
static const double pio2_lo = 6.12323399573676603587e-17;
static const double pio4_hi = 7.85398163397448278999e-01;
/* Rational approximation of (asin(x) - x) / x^3 */
static const double pS0 = 1.66666666666666657415e-01;
static const double pS1 = -3.25565818622400915405e-01;
static const double pS2 = 2.01212532134862925881e-01;
static const double pS3 = -4.00555345006794114027e-02;
static const double pS4 = 7.91534994289814532176e-04;
static const double pS5 = 3.47933107596021167570e-05;
static const double qS1 = -2.40339491173441421878e+00;
static const double qS2 = 2.02094576023350569471e+00;
static const double qS3 = -6.88283971605453293030e-01;
static const double qS4 = 7.70381505559019352791e-02;

double euc_asin(double x)
{
    double t;
    double w;
    double p;
    double q;
    double s;
    const int32_t hx = high_word(x);
    const int32_t ix = hx & 0x7FFFFFFF;

    if (ix >= 0x3FF00000) { /* |x| >= 1 */
        if ((((uint32_t)ix - 0x3FF00000U) | low_word(x)) == 0U) {
            return (x * pio2_hi) + (x * pio2_lo); /* asin(+-1) = +-pi/2 */
        }
        return f64_quiet_nan(); /* |x| > 1 */
    }
    if (ix < 0x3FE00000) {     /* |x| < 0.5 */
        if (ix < 0x3E400000) { /* |x| < 2^-27 */
            return x;
        }
        t = x * x;
        p = t * (pS0 + (t * (pS1 + (t * (pS2 + (t * (pS3 + (t * (pS4 + (t * pS5))))))))));
        q = one + (t * (qS1 + (t * (qS2 + (t * (qS3 + (t * qS4)))))));
        w = p / q;
        return x + (x * w);
    }
    /* 0.5 <= |x| < 1 */
    w = one - f64_abs(x);
    t = w * 0.5;
    p = t * (pS0 + (t * (pS1 + (t * (pS2 + (t * (pS3 + (t * (pS4 + (t * pS5))))))))));
    q = one + (t * (qS1 + (t * (qS2 + (t * (qS3 + (t * qS4)))))));
    s = euc_sqrt(t);
    if (ix >= 0x3FEF3333) { /* |x| > 0.975 */
        w = p / q;
        t = pio2_hi - ((2.0 * (s + (s * w))) - pio2_lo);
    } else {
        w = f64_from_bits(f64_bits(s) & 0xFFFFFFFF00000000U); /* s with the low word cleared */
        const double c = (t - (w * w)) / (s + w);
        const double r = p / q;
        p = ((2.0 * s) * r) - (pio2_lo - (2.0 * c));
        q = pio4_hi - (2.0 * w);
        t = pio4_hi - (p - q);
    }
    return (hx > 0) ? t : -t;
}

/* atan (fdlibm s_atan.c) ------------------------------------------------------------------------------- */

static const double atanhi[4] = {
    4.63647609000806093515e-01, /* atan(0.5) high part */
    7.85398163397448278999e-01, /* atan(1.0) high part */
    9.82793723247329054082e-01, /* atan(1.5) high part */
    1.57079632679489655800e+00, /* atan(inf) high part */
};

static const double atanlo[4] = {
    2.26987774529616870924e-17, /* atan(0.5) low part */
    3.06161699786838301793e-17, /* atan(1.0) low part */
    1.39033110312309984516e-17, /* atan(1.5) low part */
    6.12323399573676603587e-17, /* atan(inf) low part */
};

static const double aT[11] = {
    3.33333333333329318027e-01,  -1.99999999998764832476e-01, 1.42857142725034663711e-01,
    -1.11111104054623557880e-01, 9.09088713343650656196e-02,  -7.69187620504482999495e-02,
    6.66107313738753120669e-02,  -5.83357013379057348645e-02, 4.97687799461593236017e-02,
    -3.65315727442169155270e-02, 1.62858201153657823623e-02,
};

double euc_atan(double x_in)
{
    double x = x_in; /* fdlibm reduces the argument in place */
    const int32_t hx = high_word(x);
    const int32_t ix = hx & 0x7FFFFFFF;
    int32_t id;

    if (ix >= 0x44100000) { /* |x| >= 2^66 */
        if ((ix > 0x7FF00000) || ((ix == 0x7FF00000) && (low_word(x) != 0U))) {
            return x + x; /* NaN */
        }
        return (hx > 0) ? (atanhi[3] + atanlo[3]) : (-atanhi[3] - atanlo[3]);
    }
    if (ix < 0x3FDC0000) {     /* |x| < 0.4375 */
        if (ix < 0x3E200000) { /* |x| < 2^-29 */
            return x;
        }
        id = -1;
    } else {
        x = f64_abs(x);
        if (ix < 0x3FF30000) {     /* |x| < 1.1875 */
            if (ix < 0x3FE60000) { /* 7/16 <= |x| < 11/16 */
                id = 0;
                x = ((2.0 * x) - one) / (2.0 + x);
            } else { /* 11/16 <= |x| < 19/16 */
                id = 1;
                x = (x - one) / (x + one);
            }
        } else {
            if (ix < 0x40038000) { /* |x| < 2.4375 */
                id = 2;
                x = (x - 1.5) / (one + (1.5 * x));
            } else { /* 2.4375 <= |x| < 2^66 */
                id = 3;
                x = -1.0 / x;
            }
        }
    }
    /* Odd and even halves of sum(aT[i] * z^(i+1)) */
    const double z = x * x;
    const double w = z * z;
    const double s1 =
        z * (aT[0] + (w * (aT[2] + (w * (aT[4] + (w * (aT[6] + (w * (aT[8] + (w * aT[10]))))))))));
    const double s2 = w * (aT[1] + (w * (aT[3] + (w * (aT[5] + (w * (aT[7] + (w * aT[9]))))))));
    if (id < 0) {
        return x - (x * (s1 + s2));
    }
    const double result = atanhi[id] - (((x * (s1 + s2)) - atanlo[id]) - x);
    return (hx < 0) ? -result : result;
}

/* atan2 (fdlibm e_atan2.c) ----------------------------------------------------------------------------- */

static const double tiny = 1.0e-300;
static const double pi_o_4 = 7.8539816339744827900E-01;
static const double pi_o_2 = 1.5707963267948965580E+00;
static const double pi = 3.1415926535897931160E+00;
static const double pi_lo = 1.2246467991473531772E-16;

/* Quadrant m = 2 * sign(x) + sign(y). */
static double atan2_special(double y, double x, int32_t m, bool *handled)
{
    const int32_t ix = high_word(x) & 0x7FFFFFFF;
    const int32_t iy = high_word(y) & 0x7FFFFFFF;
    static const double y_zero[2] = {0.0, -0.0}; /* finite y over +inf: a zero with the sign of y */
    *handled = true;
    if (((uint32_t)iy | low_word(y)) == 0U) { /* y = 0 */
        if (m <= 1) {
            return y;
        }
        return (m == 2) ? (pi + tiny) : (-pi - tiny);
    }
    if (((uint32_t)ix | low_word(x)) == 0U) { /* x = 0 */
        return (high_word(y) < 0) ? (-pi_o_2 - tiny) : (pi_o_2 + tiny);
    }
    if (ix == 0x7FF00000) { /* x = +-inf */
        if (iy == 0x7FF00000) {
            static const double both_inf[4] = {1.0, -1.0, 3.0, -3.0};
            return (both_inf[m] * pi_o_4) + (((m % 2) == 0) ? tiny : -tiny);
        }
        if (m <= 1) {
            return y_zero[m];
        }
        return (m == 2) ? (pi + tiny) : (-pi - tiny);
    }
    if (iy == 0x7FF00000) { /* y = +-inf */
        return (high_word(y) < 0) ? (-pi_o_2 - tiny) : (pi_o_2 + tiny);
    }
    *handled = false;
    return 0.0;
}

double euc_atan2(double y, double x)
{
    const int32_t hx = high_word(x);
    const int32_t ix = hx & 0x7FFFFFFF;
    const uint32_t lx = low_word(x);
    const int32_t hy = high_word(y);
    const int32_t iy = hy & 0x7FFFFFFF;
    const uint32_t ly = low_word(y);

    if ((((uint32_t)ix | ((lx | (0U - lx)) >> 31U)) > 0x7FF00000U) ||
        (((uint32_t)iy | ((ly | (0U - ly)) >> 31U)) > 0x7FF00000U)) {
        return x + y; /* x or y is NaN */
    }
    if ((((uint32_t)hx - 0x3FF00000U) | lx) == 0U) {
        return euc_atan(y); /* x = 1.0 */
    }
    const int32_t m = (int32_t)(((uint32_t)hy >> 31U) | (((uint32_t)hx >> 30U) & 2U));

    bool handled = false;
    const double special = atan2_special(y, x, m, &handled);
    if (handled) {
        return special;
    }

    double z;
    const int32_t k = (iy - ix) >> 20;
    if (k > 60) {
        z = pi_o_2 + (0.5 * pi_lo); /* |y / x| > 2^60 */
    } else if ((hx < 0) && (k < -60)) {
        z = 0.0; /* |y| / x < -2^60 */
    } else {
        z = euc_atan(f64_abs(y / x));
    }
    switch (m) {
    case 0:
        return z; /* atan(+, +) */
    case 1:
        return -z; /* atan(-, +) */
    case 2:
        return pi - (z - pi_lo); /* atan(+, -) */
    default:
        return (z - pi_lo) - pi; /* atan(-, -) */
    }
}

/* log (fdlibm e_log.c) --------------------------------------------------------------------------------- */

static const double ln2_hi = 6.93147180369123816490e-01;
static const double ln2_lo = 1.90821492927058770002e-10;
static const double two54 = 1.80143985094819840000e+16;
/* Polynomial approximation of log(1 + f), f = (1 + s) / (1 - s) */
static const double Lg1 = 6.666666666666735130e-01;
static const double Lg2 = 3.999999999940941908e-01;
static const double Lg3 = 2.857142874366239149e-01;
static const double Lg4 = 2.222219843214978396e-01;
static const double Lg5 = 1.818357216161805012e-01;
static const double Lg6 = 1.531383769920937332e-01;
static const double Lg7 = 1.479819860511658591e-01;

static double with_high_word(double value, int32_t high)
{
    return f64_from_bits(((uint64_t)(uint32_t)high << 32U) | (uint64_t)low_word(value));
}

/* |f| < 2^-20: log(1 + f) from a short series. */
static double log_near_one(double f, int32_t k)
{
    if (f == 0.0) {
        if (k == 0) {
            return 0.0;
        }
        const double dk = (double)k;
        return (dk * ln2_hi) + (dk * ln2_lo);
    }
    const double r = (f * f) * (0.5 - (0.33333333333333333 * f));
    if (k == 0) {
        return f - r;
    }
    const double dk = (double)k;
    return (dk * ln2_hi) - ((r - (dk * ln2_lo)) - f);
}

double euc_log(double x_in)
{
    double x = x_in; /* fdlibm normalises the argument in place */
    int32_t hx = high_word(x);
    const uint32_t lx = low_word(x);
    int32_t k = 0;

    if (hx < 0x00100000) { /* x < 2^-1022 */
        if (((uint32_t)(hx & 0x7FFFFFFF) | lx) == 0U) {
            return f64_from_bits(0xFFF0000000000000U); /* log(+-0) = -inf */
        }
        if (hx < 0) {
            return f64_quiet_nan(); /* log(negative) */
        }
        k -= 54; /* subnormal: scale up */
        x *= two54;
        hx = high_word(x);
    }
    if (hx >= 0x7FF00000) {
        return x + x; /* inf or NaN */
    }
    k += (hx >> 20) - F64_EXPONENT_BIAS;
    hx &= 0x000FFFFF;
    const int32_t i = (hx + 0x95F64) & 0x100000;
    x = with_high_word(x, hx | (i ^ 0x3FF00000)); /* x or x / 2, in [sqrt(2)/2, sqrt(2)) */
    k += i >> 20;
    const double f = x - 1.0;
    if ((0x000FFFFF & (2 + hx)) < 3) {
        return log_near_one(f, k);
    }
    const double s = f / (2.0 + f);
    const double dk = (double)k;
    const double z = s * s;
    const double w = z * z;
    const double t1 = w * (Lg2 + (w * (Lg4 + (w * Lg6))));
    const double t2 = z * (Lg1 + (w * (Lg3 + (w * (Lg5 + (w * Lg7))))));
    const int32_t j = (hx - 0x6147A) | (0x6B851 - hx);
    const double r = t2 + t1;
    if (j > 0) {
        const double hfsq = (0.5 * f) * f;
        if (k == 0) {
            return f - (hfsq - (s * (hfsq + r)));
        }
        return (dk * ln2_hi) - ((hfsq - ((s * (hfsq + r)) + (dk * ln2_lo))) - f);
    }
    if (k == 0) {
        return f - (s * (f - r));
    }
    return (dk * ln2_hi) - (((s * (f - r)) - (dk * ln2_lo)) - f);
}
