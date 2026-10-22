/*
 * C ABI of the software-in-the-loop library (libeuc_sil): the firmware (core, the Begode Extreme app and the
 * host port in ports/sim) compiled for the host, which the simulator engine runs in place of the emulated
 * image (docs/sil.md).
 *
 * HAL boundary: struct euc_sil_host. The port drives the board through it with the register sequences the
 * STM32F1 port writes on the MCU, and the engine answers from the same peripheral models, plant and board
 * devices the emulated image runs against. The firmware's own code runs natively and takes no simulated time:
 * each register access, each pure-CPU wait (spin) and each main-loop pass charges a fixed cost instead.
 *
 * Contexts: euc_sil_run() is thread mode (start-up, then the main loop) and returns only when the engine
 * stops it. The interrupt contexts (TIM8 update, ADC end of injection, USART1, RTC second) run when a host
 * call answers with their index: the board raised that interrupt, and it preempts what was running. Their
 * handlers are the app's own (apps/begode-extreme/control.c).
 *
 * Quirks: EUC_QUIRK(ID) reads a run-time mask here (euc_sil_quirk_set_mask), so a run can switch any quirk;
 * set it before euc_sil_run().
 *
 * Threading and lifetime: one library instance runs one firmware instance, from one thread, once: its static
 * state is the firmware's RAM. The engine loads a fresh copy of the file for every power-on.
 * Compatibility: the engine checks euc_sil_abi_version() == EUC_SIL_ABI_VERSION before any other call.
 */
#ifndef EUC_SIL_H
#define EUC_SIL_H

#include <stdbool.h>
#include <stdint.h>

#if defined(_WIN32)
#define EUC_SIL_API __declspec(dllexport)
#else
#define EUC_SIL_API __attribute__((visibility("default")))
#endif

/* 1: quirk switches only. 2: the firmware, its contexts, the host HAL and the probes. */
#define EUC_SIL_ABI_VERSION 2U

/* Profiles for euc_sil_quirk_profile_mask(). */
#define EUC_SIL_PROFILE_COMPAT 0U
#define EUC_SIL_PROFILE_CLEAN  1U

/* What the firmware does after a host call: go on (EUC_SIL_GO_ON), run the context with this index first
 * (0 .. euc_sil_context_count() - 1), or stop for good (EUC_SIL_STOP: power lost, reset, end of run). */
#define EUC_SIL_GO_ON (-1)
#define EUC_SIL_STOP  (-2)

/* euc_sil_run() results. */
#define EUC_SIL_STOPPED      0U /* the host answered EUC_SIL_STOP */
#define EUC_SIL_NOT_ATTACHED 1U /* no host: euc_sil_attach() was not called or refused the table */
#define EUC_SIL_SPENT        2U /* this instance already ran; load a fresh copy */

/* Emulated SRAM from this address up holds the port's DMA buffers (the BMS reply, the LED strip bit slots),
 * which the board's DMA reads and writes. The engine keeps its own use of SRAM below it. */
#define EUC_SIL_DMA_RAM_ADDRESS 0x20008000U

/* CPU time one pass of the main loop stands for, in HCLK cycles, charged after each pass. */
#define EUC_SIL_MAIN_LOOP_PASS_CYCLES 300U

/*
 * The board, as the engine gives it to the firmware. `context` goes back as the first argument of every call.
 * The int32_t results are EUC_SIL_GO_ON, EUC_SIL_STOP or a context index.
 */
struct euc_sil_host {
    void *context;
    /* Register read or write of `bytes` (1, 2 or 4, naturally aligned) at `address` on the board's bus. */
    int32_t (*read)(void *context, uint32_t address, uint32_t bytes, uint32_t *value);
    int32_t (*write)(void *context, uint32_t address, uint32_t bytes, uint32_t value);
    /* `cycles` HCLK cycles of CPU time without a bus access (a pause loop, a main-loop pass). */
    int32_t (*spin)(void *context, uint32_t cycles);
    /* The context with this index finished (its exception return). */
    int32_t (*returned)(void *context, uint32_t index);
    /* Memory the board shares with the firmware (flash pages, DMA buffers in SRAM), copied without time or
     * side effects. */
    void (*load)(void *context, uint32_t address, uint8_t *bytes, uint32_t count);
    void (*store)(void *context, uint32_t address, const uint8_t *bytes, uint32_t count);
    /* The port met a request it cannot serve; the engine halts the run with this text. A call to it is
     * followed by the firmware stopping. */
    void (*fault)(void *context, const char *what);
};

EUC_SIL_API uint32_t euc_sil_abi_version(void);

/* Quirks ----------------------------------------------------------------------------------------------- */

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

/* Firmware ----------------------------------------------------------------------------------------------- */

/* Gives the firmware its board. Copies the table; false (and no host) when a function in it is NULL. */
EUC_SIL_API bool euc_sil_attach(const struct euc_sil_host *host);

/* Thread mode: power-on start-up (apps/begode-extreme/main.c app_init), then the main loop. Returns only when
 * a host call answers EUC_SIL_STOP (EUC_SIL_STOPPED), or at once without a host or on a second call. */
EUC_SIL_API uint32_t euc_sil_run(void);

/* The interrupt contexts: how many, the device interrupt number (IRQn, 0 = WWDG) each one serves, and a
 * name for people. -1 and NULL for an index out of range. */
EUC_SIL_API uint32_t euc_sil_context_count(void);
EUC_SIL_API int32_t euc_sil_context_irq(uint32_t index);
EUC_SIL_API const char *euc_sil_context_name(uint32_t index);

/* Probes --------------------------------------------------------------------------------------------------
 */

/*
 * Named places in the firmware's state (struct app), the same list as profiles/begode-extreme/probes.toml
 * (tools/gen_probes.py), with this build's offsets. Name, kind ("u8", "s16", "f32", ...), role ("cutoff" or
 * NULL) and note as in the probe file; NULL and 0 for an index out of range.
 */
EUC_SIL_API uint32_t euc_sil_probe_count(void);
EUC_SIL_API const char *euc_sil_probe_name(uint32_t index);
EUC_SIL_API const char *euc_sil_probe_kind(uint32_t index);
EUC_SIL_API const char *euc_sil_probe_role(uint32_t index);
EUC_SIL_API const char *euc_sil_probe_note(uint32_t index);
EUC_SIL_API uint32_t euc_sil_probe_offset(uint32_t index);
EUC_SIL_API uint32_t euc_sil_probe_size(uint32_t index);

/* The firmware's state the probe offsets count from, and its size. Read it only while the firmware waits in a
 * host call. */
EUC_SIL_API const void *euc_sil_state(void);
EUC_SIL_API uint32_t euc_sil_state_bytes(void);

#endif
