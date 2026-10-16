/*
 * Unit tests for the compile-time quirk switches (core/include/euc/quirks.h). Built twice, once with
 * EUC_PROFILE_COMPAT and once with EUC_PROFILE_CLEAN, as test_quirks_compat and test_quirks_clean.
 */
#include "euc/quirks.h"
#include "unity.h"

#if defined(EUC_PROFILE_COMPAT)
#define EXPECTED_VALUE(compat, clean) (compat)
#elif defined(EUC_PROFILE_CLEAN)
#define EXPECTED_VALUE(compat, clean) (clean)
#else
#error "test_quirks.c is built with EUC_PROFILE_COMPAT or EUC_PROFILE_CLEAN"
#endif

/* EUC_QUIRK must stay an integer constant expression on target builds, so unused branches compile away. */
_Static_assert(EUC_QUIRK(ASIN_ACCEL) == EXPECTED_VALUE(1, 0), "EUC_QUIRK is a compile-time constant");

void setUp(void)
{
}

void tearDown(void)
{
}

static void test_each_quirk_has_its_profile_value(void)
{
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary)                                                 \
    TEST_ASSERT_EQUAL_INT_MESSAGE(EXPECTED_VALUE(compat, clean), EUC_QUIRK(id), #id);
#include "quirks.def"
#undef EUC_QUIRK_DEF
}

static void test_ids_follow_the_list_order(void)
{
    int expected_id = 0;
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary)                                                 \
    TEST_ASSERT_EQUAL_INT_MESSAGE(expected_id, EUC_QUIRK_ID_##id, #id);                                      \
    expected_id++;
#include "quirks.def"
#undef EUC_QUIRK_DEF
    TEST_ASSERT_EQUAL_INT(EUC_QUIRK_COUNT, expected_id);
}

int main(void)
{
    UNITY_BEGIN();
    RUN_TEST(test_each_quirk_has_its_profile_value);
    RUN_TEST(test_ids_follow_the_list_order);
    return UNITY_END();
}
