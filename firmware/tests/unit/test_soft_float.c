/*
 * Unit tests for core/math/soft_float.c, which the target images use in place of libgcc's __aeabi_ddiv and
 * __aeabi_fcmp{lt,le,ge,gt}: euc_f64_div against the host's hardware division (IEEE 754, round to nearest
 * even, denormals kept) on random and constructed operands, with NaN results against libgcc's rules; the
 * float comparisons against the host's comparison operators.
 */
#include <float.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#include "euc/math/soft_float.h"
#include "unity.h"

#define RANDOM_SAMPLES 400000U

#define SIGN_BIT    0x8000000000000000ULL
#define INFINITE    0x7FF0000000000000ULL
#define FRACTION    0x000FFFFFFFFFFFFFULL
#define QUIET_BIT   0x0008000000000000ULL
#define DEFAULT_NAN 0x7FF8000000000000ULL

static uint64_t random_state = 0x2545F4914F6CDD1DU;

static uint64_t random_bits(void)
{
    random_state ^= random_state << 13U;
    random_state ^= random_state >> 7U;
    random_state ^= random_state << 17U;
    return random_state;
}

static uint64_t bits_of(double value)
{
    uint64_t bits;
    memcpy(&bits, &value, sizeof(bits));
    return bits;
}

static double double_of(uint64_t bits)
{
    double value;
    memcpy(&value, &bits, sizeof(value));
    return value;
}

static int is_nan_bits(uint64_t x)
{
    return ((x & INFINITE) == INFINITE) && ((x & FRACTION) != 0U);
}

/* The NaN libgcc's __aeabi_ddiv returns, as measured on the Cortex-M3 core; 0 when the result is no NaN. */
static uint64_t libgcc_nan(uint64_t a, uint64_t b)
{
    const int a_special = (a & INFINITE) == INFINITE;
    const int b_special = (b & INFINITE) == INFINITE;
    if (is_nan_bits(a) || (a_special && b_special)) {
        return a | QUIET_BIT;
    }
    if (is_nan_bits(b)) {
        return b | QUIET_BIT;
    }
    if (((a << 1U) == 0U) && ((b << 1U) == 0U)) {
        return a | DEFAULT_NAN;
    }
    return 0U;
}

static void check(uint64_t a, uint64_t b)
{
    const uint64_t got = euc_f64_div(a, b);
    const uint64_t nan = libgcc_nan(a, b);
    if (nan != 0U) {
        TEST_ASSERT_EQUAL_HEX64(nan, got);
        return;
    }
    TEST_ASSERT_EQUAL_HEX64(bits_of(double_of(a) / double_of(b)), got);
}

/* Exponents near every boundary: denormal, smallest normal, around 1, largest finite, infinity/NaN. */
static const uint64_t exponents[] = {0U,    1U,    2U,    3U,    52U,   53U,   54U,   1021U,
                                     1022U, 1023U, 1024U, 1025U, 2044U, 2045U, 2046U, 2047U};
static const uint64_t fractions[] = {0U,
                                     1U,
                                     2U,
                                     0x8000000000000ULL,
                                     0x8000000000001ULL,
                                     0xFFFFFFFFFFFFFULL,
                                     0xFFFFFFFFFFFFEULL,
                                     0x7FFFFFFFFFFFFULL};

static uint64_t random_operand(void)
{
    const uint64_t sign = random_bits() & SIGN_BIT;
    switch (random_bits() % 6U) {
    case 0U:
    case 1U:
        return random_bits(); /* any bits, NaNs and infinities included */
    case 2U:
        return sign | (exponents[random_bits() % 16U] << 52U) | fractions[random_bits() % 8U];
    case 3U:
        return sign | (exponents[random_bits() % 16U] << 52U) | (random_bits() & FRACTION);
    case 4U:
        return sign | (random_bits() & FRACTION); /* denormal or zero */
    default:
        return sign | (((random_bits() % 2046U) + 1U) << 52U) | (random_bits() & FRACTION);
    }
}

static void test_random_operands_match_the_hardware(void)
{
    for (uint32_t i = 0U; i < RANDOM_SAMPLES; i++) {
        check(random_operand(), random_operand());
    }
}

static void test_close_significands_and_exact_quotients(void)
{
    /* Quotients near 1 (ma just below or above mb) and exact quotients, where the remainder decides. */
    for (uint32_t i = 0U; i < (RANDOM_SAMPLES / 4U); i++) {
        const uint64_t b = (((random_bits() % 2046U) + 1U) << 52U) | (random_bits() & FRACTION);
        const uint64_t step = random_bits() % 8U;
        check(b + step, b);
        check(b - step, b);
        /* (b * small integer) / b is exact when the product is exact. */
        const double product = double_of(b) * (double)(1U + (random_bits() % 15U));
        check(bits_of(product), b);
    }
}

static void test_special_values(void)
{
    const uint64_t specials[] = {0U,
                                 SIGN_BIT,
                                 INFINITE,
                                 SIGN_BIT | INFINITE,
                                 DEFAULT_NAN | 5U,
                                 SIGN_BIT | DEFAULT_NAN | 6U,
                                 INFINITE | 7U, /* signalling NaN */
                                 1U,            /* smallest denormal */
                                 FRACTION,      /* largest denormal */
                                 0x0010000000000000ULL,
                                 0x7FEFFFFFFFFFFFFFULL,
                                 0x3FF0000000000000ULL,
                                 0xBFF0000000000000ULL,
                                 0x4008000000000000ULL};
    const uint32_t count = (uint32_t)(sizeof(specials) / sizeof(specials[0]));
    for (uint32_t i = 0U; i < count; i++) {
        for (uint32_t j = 0U; j < count; j++) {
            check(specials[i], specials[j]);
        }
    }
    TEST_ASSERT_EQUAL_HEX64(0x3FD5555555555555ULL, euc_f64_div(0x3FF0000000000000ULL, 0x4008000000000000ULL));
    TEST_ASSERT_EQUAL_HEX64(DEFAULT_NAN, euc_f64_div(0U, 0U));
    TEST_ASSERT_EQUAL_HEX64(SIGN_BIT | DEFAULT_NAN, euc_f64_div(SIGN_BIT, 0U));
    TEST_ASSERT_EQUAL_HEX64(SIGN_BIT | DEFAULT_NAN, euc_f64_div(SIGN_BIT | INFINITE, INFINITE));
    TEST_ASSERT_EQUAL_HEX64(DEFAULT_NAN, euc_f64_div(INFINITE, DEFAULT_NAN | 6U));
}

static void test_denormal_results_round_once(void)
{
    /* Ties in the denormal range: 3 * 2^-1075 is exactly halfway between 2^-1074 and 2 * 2^-1074. */
    check(3U, 0x4000000000000000ULL);
    check(5U, 0x4000000000000000ULL);
    check(0x0010000000000001ULL, 0x4340000000000000ULL);
    check(0x0010000000000003ULL, 0x4330000000000000ULL);
    check(0x0000000000000001ULL, 0x3FE0000000000000ULL);
    check(0x0000000000000001ULL, 0x7FEFFFFFFFFFFFFFULL);
    check(0x7FEFFFFFFFFFFFFFULL, 0x0000000000000001ULL); /* overflows to infinity */
    check(0x7FEFFFFFFFFFFFFFULL, 0x3FEFFFFFFFFFFFFFULL);
}

static float float_of(uint32_t bits)
{
    float value;
    memcpy(&value, &bits, sizeof(value));
    return value;
}

static void check_compare(uint32_t a, uint32_t b)
{
    const float x = float_of(a);
    const float y = float_of(b);
    TEST_ASSERT_EQUAL(x < y, euc_f32_less(a, b));
    TEST_ASSERT_EQUAL(x <= y, euc_f32_less_equal(a, b));
    TEST_ASSERT_EQUAL(y < x, euc_f32_less(b, a));
    TEST_ASSERT_EQUAL(y <= x, euc_f32_less_equal(b, a));
}

static void test_float_comparisons_match_the_hardware(void)
{
    /* Zeros of both signs, denormals, the normal boundary, 1, the largest finite, infinities and NaNs. */
    const uint32_t specials[] = {0x00000000U, 0x80000000U, 0x00000001U, 0x80000001U, 0x007FFFFFU,
                                 0x807FFFFFU, 0x00800000U, 0x80800000U, 0x3F800000U, 0xBF800000U,
                                 0x3F800001U, 0x7F7FFFFFU, 0xFF7FFFFFU, 0x7F800000U, 0xFF800000U,
                                 0x7FC00000U, 0xFFC00000U, 0x7F800001U, 0xFFFFFFFFU};
    const uint32_t count = (uint32_t)(sizeof(specials) / sizeof(specials[0]));
    for (uint32_t i = 0U; i < count; i++) {
        for (uint32_t j = 0U; j < count; j++) {
            check_compare(specials[i], specials[j]);
        }
    }
    for (uint32_t i = 0U; i < RANDOM_SAMPLES; i++) {
        const uint32_t a = (uint32_t)random_bits();
        check_compare(a, (uint32_t)random_bits());
        /* Neighbours and the same magnitude with the other sign. */
        check_compare(a, a + 1U);
        check_compare(a, a ^ 0x80000000U);
    }
}

void setUp(void)
{
}

void tearDown(void)
{
}

int main(void)
{
    UNITY_BEGIN();
    RUN_TEST(test_random_operands_match_the_hardware);
    RUN_TEST(test_close_significands_and_exact_quotients);
    RUN_TEST(test_special_values);
    RUN_TEST(test_denormal_results_round_once);
    RUN_TEST(test_float_comparisons_match_the_hardware);
    return UNITY_END();
}
