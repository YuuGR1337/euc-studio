/*
 * Unit tests for core/include/euc/math/fixed.h: saturation at both ends of every helper, the rounding
 * direction of multiplications and narrowing, and exact round trips.
 */
#include "euc/math/fixed.h"
#include "unity.h"

void setUp(void)
{
}

void tearDown(void)
{
}

static void test_sat_i32_to_i16_passes_in_range_values(void)
{
    TEST_ASSERT_EQUAL_INT16(0, sat_i32_to_i16(0));
    TEST_ASSERT_EQUAL_INT16(INT16_MAX, sat_i32_to_i16(INT16_MAX));
    TEST_ASSERT_EQUAL_INT16(INT16_MIN, sat_i32_to_i16(INT16_MIN));
}

static void test_sat_i32_to_i16_clamps_out_of_range_values(void)
{
    TEST_ASSERT_EQUAL_INT16(INT16_MAX, sat_i32_to_i16(INT16_MAX + 1));
    TEST_ASSERT_EQUAL_INT16(INT16_MIN, sat_i32_to_i16(INT16_MIN - 1));
    TEST_ASSERT_EQUAL_INT16(INT16_MAX, sat_i32_to_i16(INT32_MAX));
    TEST_ASSERT_EQUAL_INT16(INT16_MIN, sat_i32_to_i16(INT32_MIN));
}

static void test_sat_i64_to_i32_clamps_out_of_range_values(void)
{
    TEST_ASSERT_EQUAL_INT32(-5, sat_i64_to_i32(-5));
    TEST_ASSERT_EQUAL_INT32(INT32_MAX, sat_i64_to_i32((int64_t)INT32_MAX + 1));
    TEST_ASSERT_EQUAL_INT32(INT32_MIN, sat_i64_to_i32((int64_t)INT32_MIN - 1));
    TEST_ASSERT_EQUAL_INT32(INT32_MAX, sat_i64_to_i32(INT64_MAX));
    TEST_ASSERT_EQUAL_INT32(INT32_MIN, sat_i64_to_i32(INT64_MIN));
}

static void test_clamp_i32_limits_to_the_range(void)
{
    TEST_ASSERT_EQUAL_INT32(10, clamp_i32(10, -20, 20));
    TEST_ASSERT_EQUAL_INT32(20, clamp_i32(21, -20, 20));
    TEST_ASSERT_EQUAL_INT32(-20, clamp_i32(-21, -20, 20));
    TEST_ASSERT_EQUAL_INT32(7, clamp_i32(INT32_MIN, 7, 7));
}

static void test_q15_add_and_sub_saturate(void)
{
    TEST_ASSERT_EQUAL_INT16(30000, q15_add_sat(20000, 10000));
    TEST_ASSERT_EQUAL_INT16(Q15_MAX, q15_add_sat(30000, 10000));
    TEST_ASSERT_EQUAL_INT16(Q15_MIN, q15_add_sat(-30000, -10000));
    TEST_ASSERT_EQUAL_INT16(Q15_MAX, q15_sub_sat(30000, -10000));
    TEST_ASSERT_EQUAL_INT16(Q15_MIN, q15_sub_sat(-30000, 10000));
    TEST_ASSERT_EQUAL_INT16(-1, q15_sub_sat(0, 1));
}

static void test_q15_neg_and_abs_saturate_minus_one(void)
{
    TEST_ASSERT_EQUAL_INT16(Q15_MAX, q15_neg_sat(Q15_MIN));
    TEST_ASSERT_EQUAL_INT16(Q15_MIN + 1, q15_neg_sat(Q15_MAX));
    TEST_ASSERT_EQUAL_INT16(Q15_MAX, q15_abs_sat(Q15_MIN));
    TEST_ASSERT_EQUAL_INT16(1234, q15_abs_sat(-1234));
    TEST_ASSERT_EQUAL_INT16(1234, q15_abs_sat(1234));
}

static void test_q15_mul_scales_by_one_half(void)
{
    const q15_t half = 16384;
    TEST_ASSERT_EQUAL_INT16(8192, q15_mul_sat(half, half));
    TEST_ASSERT_EQUAL_INT16(-8192, q15_mul_sat(half, -half));
}

static void test_q15_mul_rounds_toward_minus_infinity(void)
{
    /* 1 LSB * 0.5 = 0.5 LSB: positive goes to 0, negative goes to -1 (arithmetic shift, as the stock code).
     */
    TEST_ASSERT_EQUAL_INT16(0, q15_mul_sat(1, 16384));
    TEST_ASSERT_EQUAL_INT16(-1, q15_mul_sat(-1, 16384));
}

static void test_q15_mul_saturates_only_minus_one_squared(void)
{
    TEST_ASSERT_EQUAL_INT16(Q15_MAX, q15_mul_sat(Q15_MIN, Q15_MIN));
    TEST_ASSERT_EQUAL_INT16(Q15_MIN + 1, q15_mul_sat(Q15_MIN, Q15_MAX));
    TEST_ASSERT_EQUAL_INT16(Q15_MAX - 1, q15_mul_sat(Q15_MAX, Q15_MAX));
}

static void test_q15_q16_round_trip_is_exact(void)
{
    const q15_t samples[] = {Q15_MIN, -12345, -1, 0, 1, 12345, Q15_MAX};
    for (unsigned i = 0U; i < sizeof(samples) / sizeof(samples[0]); i++) {
        TEST_ASSERT_EQUAL_INT16(samples[i], q15_from_q16_sat(q16_from_q15(samples[i])));
    }
    TEST_ASSERT_EQUAL_INT32(Q16_ONE / 2, q16_from_q15(16384));
}

static void test_q15_from_q16_saturates_outside_minus_one_to_one(void)
{
    TEST_ASSERT_EQUAL_INT16(Q15_MAX, q15_from_q16_sat(Q16_ONE));
    TEST_ASSERT_EQUAL_INT16(Q15_MIN, q15_from_q16_sat(-Q16_ONE));
    TEST_ASSERT_EQUAL_INT16(Q15_MIN, q15_from_q16_sat(-2 * Q16_ONE));
}

static void test_q16_add_and_sub_saturate(void)
{
    TEST_ASSERT_EQUAL_INT32(3 * Q16_ONE, q16_add_sat(Q16_ONE, 2 * Q16_ONE));
    TEST_ASSERT_EQUAL_INT32(Q16_MAX, q16_add_sat(Q16_MAX, 1));
    TEST_ASSERT_EQUAL_INT32(Q16_MIN, q16_add_sat(Q16_MIN, -1));
    TEST_ASSERT_EQUAL_INT32(Q16_MAX, q16_sub_sat(Q16_MAX, -1));
    TEST_ASSERT_EQUAL_INT32(Q16_MIN, q16_sub_sat(Q16_MIN, 1));
}

static void test_q16_mul_matches_real_products(void)
{
    const q16_t one_and_half = Q16_ONE + Q16_ONE / 2;
    TEST_ASSERT_EQUAL_INT32(Q16_ONE * 9 / 4, q16_mul_sat(one_and_half, one_and_half));
    TEST_ASSERT_EQUAL_INT32(-6 * Q16_ONE, q16_mul_sat(-2 * Q16_ONE, 3 * Q16_ONE));
    /* 1 LSB * 0.5 rounds toward minus infinity. */
    TEST_ASSERT_EQUAL_INT32(0, q16_mul_sat(1, Q16_ONE / 2));
    TEST_ASSERT_EQUAL_INT32(-1, q16_mul_sat(-1, Q16_ONE / 2));
}

static void test_q16_mul_saturates(void)
{
    TEST_ASSERT_EQUAL_INT32(Q16_MAX, q16_mul_sat(256 * Q16_ONE, 256 * Q16_ONE));
    TEST_ASSERT_EQUAL_INT32(Q16_MIN, q16_mul_sat(-256 * Q16_ONE, 256 * Q16_ONE));
    TEST_ASSERT_EQUAL_INT32(Q16_MAX, q16_mul_sat(Q16_MIN, Q16_MIN));
}

static void test_q16_integer_conversions(void)
{
    TEST_ASSERT_EQUAL_INT32(-3 * Q16_ONE, q16_from_int_sat(-3));
    TEST_ASSERT_EQUAL_INT32(Q16_MAX, q16_from_int_sat(32768));
    TEST_ASSERT_EQUAL_INT32(Q16_MIN, q16_from_int_sat(-32768));
    TEST_ASSERT_EQUAL_INT32(Q16_MIN, q16_from_int_sat(INT32_MIN));
    TEST_ASSERT_EQUAL_INT32(2, q16_to_int_floor(2 * Q16_ONE + Q16_ONE / 2));
    TEST_ASSERT_EQUAL_INT32(-1, q16_to_int_floor(-Q16_ONE / 2));
    TEST_ASSERT_EQUAL_INT32(-32768, q16_to_int_floor(Q16_MIN));
}

int main(void)
{
    UNITY_BEGIN();
    RUN_TEST(test_sat_i32_to_i16_passes_in_range_values);
    RUN_TEST(test_sat_i32_to_i16_clamps_out_of_range_values);
    RUN_TEST(test_sat_i64_to_i32_clamps_out_of_range_values);
    RUN_TEST(test_clamp_i32_limits_to_the_range);
    RUN_TEST(test_q15_add_and_sub_saturate);
    RUN_TEST(test_q15_neg_and_abs_saturate_minus_one);
    RUN_TEST(test_q15_mul_scales_by_one_half);
    RUN_TEST(test_q15_mul_rounds_toward_minus_infinity);
    RUN_TEST(test_q15_mul_saturates_only_minus_one_squared);
    RUN_TEST(test_q15_q16_round_trip_is_exact);
    RUN_TEST(test_q15_from_q16_saturates_outside_minus_one_to_one);
    RUN_TEST(test_q16_add_and_sub_saturate);
    RUN_TEST(test_q16_mul_matches_real_products);
    RUN_TEST(test_q16_mul_saturates);
    RUN_TEST(test_q16_integer_conversions);
    return UNITY_END();
}
