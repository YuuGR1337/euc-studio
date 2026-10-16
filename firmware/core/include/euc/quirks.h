/*
 * Compat / clean behaviour switches.
 *
 * Every stock behaviour we reproduce only for lock-step with the stock binary is one quirk in
 * config/quirks.def. Code tests a quirk like this:
 *
 *     if (EUC_QUIRK(ASIN_ACCEL)) {
 *         ... stock behaviour ...
 *     } else {
 *         ... clean behaviour ...
 *     }
 *
 * Target images: EUC_QUIRK(ID) is an integer constant expression (0 or 1). The compiler drops the branch the
 * profile does not use, so neither profile pays flash for the other.
 * SIL library (host): EUC_QUIRK(ID) reads a run-time mask that ports/sim owns, so the studio can flip one
 * quirk at a time on a running simulation.
 *
 * The build defines exactly one of EUC_PROFILE_COMPAT, EUC_PROFILE_CLEAN or EUC_PROFILE_RUNTIME.
 */
#ifndef EUC_QUIRKS_H
#define EUC_QUIRKS_H

#include <stdbool.h>

#if (defined(EUC_PROFILE_COMPAT) + defined(EUC_PROFILE_CLEAN) + defined(EUC_PROFILE_RUNTIME)) != 1
#error "Define exactly one of EUC_PROFILE_COMPAT, EUC_PROFILE_CLEAN or EUC_PROFILE_RUNTIME."
#endif

/* Index of each quirk, in config/quirks.def order. It is also the bit position in the SIL mask. */
enum euc_quirk_id {
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary) EUC_QUIRK_ID_##id,
#include "quirks.def"
#undef EUC_QUIRK_DEF
    EUC_QUIRK_COUNT
};

_Static_assert((unsigned int)EUC_QUIRK_COUNT <= 64U, "the SIL quirk mask is 64 bits wide");

#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary)                                                 \
    _Static_assert((((compat) == 0) || ((compat) == 1)) && (((clean) == 0) || ((clean) == 1)),               \
                   #id ": COMPAT and CLEAN must be 0 or 1");
#include "quirks.def"
#undef EUC_QUIRK_DEF

#if defined(EUC_PROFILE_RUNTIME)

/* Provided by the SIL port (ports/sim). */
bool euc_quirk_is_active(enum euc_quirk_id id);

#define EUC_QUIRK(ID) euc_quirk_is_active(EUC_QUIRK_ID_##ID)

#else

#if defined(EUC_PROFILE_COMPAT)
#define EUC_QUIRK_PROFILE_COLUMN(compat, clean) (compat)
#else
#define EUC_QUIRK_PROFILE_COLUMN(compat, clean) (clean)
#endif

/* Value of each quirk in the profile this image is built for. */
enum {
#define EUC_QUIRK_DEF(id, compat, clean, stock_ref, summary)                                                 \
    EUC_QUIRK_VALUE_##id = EUC_QUIRK_PROFILE_COLUMN(compat, clean),
#include "quirks.def"
#undef EUC_QUIRK_DEF
};

#define EUC_QUIRK(ID) (EUC_QUIRK_VALUE_##ID != 0)

#endif

#endif
