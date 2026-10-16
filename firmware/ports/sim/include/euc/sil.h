/*
 * C ABI of the software-in-the-loop library (libeuc_sil), the firmware build the simulator engine loads on
 * the host instead of running the image in the emulator.
 *
 * M0 exposes the quirk switches, so the studio can list them and flip one at a time. The firmware entry
 * points (init, the TIM8 / ADC / RTC / main-loop contexts) and the HAL backed by simulated peripherals come
 * with M4.
 *
 * Threading: one library instance runs one firmware instance, driven from one thread.
 * Compatibility: the engine checks euc_sil_abi_version() == EUC_SIL_ABI_VERSION before any other call.
 */
#ifndef EUC_SIL_H
#define EUC_SIL_H

#include <stdint.h>

#if defined(_WIN32)
#define EUC_SIL_API __declspec(dllexport)
#else
#define EUC_SIL_API __attribute__((visibility("default")))
#endif

#define EUC_SIL_ABI_VERSION 1U

/* Profiles for euc_sil_quirk_profile_mask(). */
#define EUC_SIL_PROFILE_COMPAT 0U
#define EUC_SIL_PROFILE_CLEAN  1U

EUC_SIL_API uint32_t euc_sil_abi_version(void);

/* Number of quirks; valid indexes are 0 .. count - 1 and match the bits of the masks below. */
EUC_SIL_API uint32_t euc_sil_quirk_count(void);

/* Quirk ID as written in config/quirks.def (e.g. "ASIN_ACCEL"), or NULL for an index out of range. */
EUC_SIL_API const char *euc_sil_quirk_name(uint32_t index);

/* One-line description of the stock behaviour, or NULL for an index out of range. */
EUC_SIL_API const char *euc_sil_quirk_summary(uint32_t index);

/* Quirks the given profile has (bit i = quirk i). 0 for an unknown profile. */
EUC_SIL_API uint64_t euc_sil_quirk_profile_mask(uint32_t profile);

/* Quirks active now. The library starts with the compat profile's mask. */
EUC_SIL_API uint64_t euc_sil_quirk_mask(void);

/* Replaces the active quirks. Bits at or above euc_sil_quirk_count() are ignored. */
EUC_SIL_API void euc_sil_quirk_set_mask(uint64_t mask);

#endif
