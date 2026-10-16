/*
 * Unit tests for the SIL port's quirk switches (ports/sim): the C ABI the engine uses, and EUC_QUIRK() in the
 * runtime profile reading the mask that ABI sets.
 */
#include <stdint.h>
#include <string.h>

#include "euc/quirks.h"
#include "euc/sil.h"
#include "unity.h"

/* The masks the COMPAT and CLEAN columns of config/quirks.def describe. */
static uint64_t expected_profile_mask(uint32_t profile)
{
    uint64_t compat_mask = 0U;
    uint64_t clean_mask = 0U;
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary)                                                 \
    compat_mask |= (uint64_t)(compat) << EUC_QUIRK_ID_##id;                                                  \
    clean_mask |= (uint64_t)(clean) << EUC_QUIRK_ID_##id;
#include "quirks.def"
#undef EUC_QUIRK_DEF
    return (profile == EUC_SIL_PROFILE_COMPAT) ? compat_mask : clean_mask;
}

void setUp(void)
{
}

/* Every test after the first starts from the compat profile, the library's start-up state. */
void tearDown(void)
{
    euc_sil_quirk_set_mask(euc_sil_quirk_profile_mask(EUC_SIL_PROFILE_COMPAT));
}

static void test_library_starts_in_the_compat_profile(void)
{
    TEST_ASSERT_EQUAL_UINT64(expected_profile_mask(EUC_SIL_PROFILE_COMPAT), euc_sil_quirk_mask());
}

static void test_abi_version_and_count(void)
{
    TEST_ASSERT_EQUAL_UINT32(EUC_SIL_ABI_VERSION, euc_sil_abi_version());
    TEST_ASSERT_EQUAL_UINT32((uint32_t)EUC_QUIRK_COUNT, euc_sil_quirk_count());
}

static void test_profile_masks_match_the_quirk_list(void)
{
    TEST_ASSERT_EQUAL_UINT64(expected_profile_mask(EUC_SIL_PROFILE_COMPAT),
                             euc_sil_quirk_profile_mask(EUC_SIL_PROFILE_COMPAT));
    TEST_ASSERT_EQUAL_UINT64(expected_profile_mask(EUC_SIL_PROFILE_CLEAN),
                             euc_sil_quirk_profile_mask(EUC_SIL_PROFILE_CLEAN));
    TEST_ASSERT_EQUAL_UINT64(0U, euc_sil_quirk_profile_mask(2U));
}

static void test_each_quirk_switches_alone(void)
{
    for (uint32_t i = 0U; i < euc_sil_quirk_count(); i++) {
        euc_sil_quirk_set_mask(UINT64_C(1) << i);
        TEST_ASSERT_EQUAL_UINT64(UINT64_C(1) << i, euc_sil_quirk_mask());
        for (uint32_t j = 0U; j < euc_sil_quirk_count(); j++) {
            TEST_ASSERT_EQUAL_MESSAGE(i == j, euc_quirk_is_active((enum euc_quirk_id)j),
                                      euc_sil_quirk_name(j));
        }
    }
}

static void test_euc_quirk_macro_reads_the_mask(void)
{
    euc_sil_quirk_set_mask(0U);
    TEST_ASSERT_FALSE(EUC_QUIRK(ASIN_ACCEL));
    euc_sil_quirk_set_mask(UINT64_C(1) << EUC_QUIRK_ID_ASIN_ACCEL);
    TEST_ASSERT_TRUE(EUC_QUIRK(ASIN_ACCEL));
    TEST_ASSERT_FALSE(EUC_QUIRK(NO_WATCHDOG));
}

static void test_mask_bits_beyond_the_list_are_ignored(void)
{
    euc_sil_quirk_set_mask(UINT64_MAX);
    TEST_ASSERT_EQUAL_UINT64((UINT64_C(1) << euc_sil_quirk_count()) - 1U, euc_sil_quirk_mask());
    TEST_ASSERT_FALSE(euc_quirk_is_active(EUC_QUIRK_COUNT));
}

static void test_each_quirk_has_a_name_and_a_summary(void)
{
    TEST_ASSERT_EQUAL_STRING("ASIN_ACCEL", euc_sil_quirk_name((uint32_t)EUC_QUIRK_ID_ASIN_ACCEL));
    for (uint32_t i = 0U; i < euc_sil_quirk_count(); i++) {
        TEST_ASSERT_NOT_NULL(euc_sil_quirk_name(i));
        TEST_ASSERT_NOT_NULL(euc_sil_quirk_summary(i));
        TEST_ASSERT_TRUE(strlen(euc_sil_quirk_summary(i)) > 0U);
    }
}

static void test_index_out_of_range_gives_null(void)
{
    TEST_ASSERT_NULL(euc_sil_quirk_name(euc_sil_quirk_count()));
    TEST_ASSERT_NULL(euc_sil_quirk_summary(euc_sil_quirk_count()));
}

int main(void)
{
    UNITY_BEGIN();
    RUN_TEST(test_library_starts_in_the_compat_profile); /* must stay first: it checks the start-up state */
    RUN_TEST(test_abi_version_and_count);
    RUN_TEST(test_profile_masks_match_the_quirk_list);
    RUN_TEST(test_each_quirk_switches_alone);
    RUN_TEST(test_euc_quirk_macro_reads_the_mask);
    RUN_TEST(test_mask_bits_beyond_the_list_are_ignored);
    RUN_TEST(test_each_quirk_has_a_name_and_a_summary);
    RUN_TEST(test_index_out_of_range_gives_null);
    return UNITY_END();
}
