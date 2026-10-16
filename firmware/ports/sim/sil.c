/*
 * Software-in-the-loop port: quirk switches.
 *
 * quirk_active[] is the only mutable state at file scope. It belongs to this port, not to core: core reads it
 * through EUC_QUIRK(), which the runtime profile maps to euc_quirk_is_active().
 */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "euc/quirks.h"
#include "euc/sil.h"

struct quirk_description {
    const char *name;
    const char *summary;
    bool compat;
    bool clean;
};

static const struct quirk_description quirk_descriptions[EUC_QUIRK_COUNT] = {
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary)                                                 \
    [EUC_QUIRK_ID_##id] = {#id, summary, compat, clean},
#include "quirks.def"
#undef EUC_QUIRK_DEF
};

static bool quirk_active[EUC_QUIRK_COUNT] = {
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary) [EUC_QUIRK_ID_##id] = (compat),
#include "quirks.def"
#undef EUC_QUIRK_DEF
};

bool euc_quirk_is_active(enum euc_quirk_id id)
{
    return ((size_t)id < (size_t)EUC_QUIRK_COUNT) && quirk_active[id];
}

uint32_t euc_sil_abi_version(void)
{
    return EUC_SIL_ABI_VERSION;
}

uint32_t euc_sil_quirk_count(void)
{
    return (uint32_t)EUC_QUIRK_COUNT;
}

const char *euc_sil_quirk_name(uint32_t index)
{
    return (index < (uint32_t)EUC_QUIRK_COUNT) ? quirk_descriptions[index].name : NULL;
}

const char *euc_sil_quirk_summary(uint32_t index)
{
    return (index < (uint32_t)EUC_QUIRK_COUNT) ? quirk_descriptions[index].summary : NULL;
}

uint64_t euc_sil_quirk_profile_mask(uint32_t profile)
{
    if ((profile != EUC_SIL_PROFILE_COMPAT) && (profile != EUC_SIL_PROFILE_CLEAN)) {
        return 0U;
    }
    uint64_t mask = 0U;
    for (uint32_t i = 0U; i < (uint32_t)EUC_QUIRK_COUNT; i++) {
        const bool active =
            (profile == EUC_SIL_PROFILE_COMPAT) ? quirk_descriptions[i].compat : quirk_descriptions[i].clean;
        if (active) {
            mask |= UINT64_C(1) << i;
        }
    }
    return mask;
}

uint64_t euc_sil_quirk_mask(void)
{
    uint64_t mask = 0U;
    for (uint32_t i = 0U; i < (uint32_t)EUC_QUIRK_COUNT; i++) {
        if (quirk_active[i]) {
            mask |= UINT64_C(1) << i;
        }
    }
    return mask;
}

void euc_sil_quirk_set_mask(uint64_t mask)
{
    for (uint32_t i = 0U; i < (uint32_t)EUC_QUIRK_COUNT; i++) {
        quirk_active[i] = ((mask >> i) & 1U) != 0U;
    }
}
