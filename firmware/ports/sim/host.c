/*
 * The SIL library's side of the host boundary: the board the engine attaches, thread mode, the interrupt
 * contexts, the flash pages and the probes (C ABI: include/euc/sil.h).
 *
 * Every host call answers what to do next. An index makes the port run that interrupt context at once, before
 * the code that made the call goes on, which is how an interrupt preempts on the MCU; when the context
 * returns the host is told and answers again (another context: a tail-chain). EUC_SIL_STOP ends the firmware
 * for good: the port jumps back out of every context to euc_sil_run(), and the engine drops this library
 * instance, whose state is half way through whatever it was doing.
 */
#include <setjmp.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "app.h"
#include "euc/sil.h"
#include "sil_port.h"
#include "stm32f1/irqn.h"

/* The board addresses of the storage pages, as boards/begode-extreme/link_app.ld places them (CMake reads
 * them from the linker script). */
#if !defined(SIL_SETTINGS_PAGE_ADDRESS) || !defined(SIL_IMAGE_ADDRESS)
#error "SIL_IMAGE_ADDRESS and the page addresses come from ports/sim/CMakeLists.txt"
#endif

/* Vector table offset register (PM0056 4.4.4). */
#define SCB_VTOR 0xE000ED08U

uint16_t sil_settings_page[SIL_FLASH_PAGE_HALFWORDS];
uint16_t sil_calibrated_page[SIL_FLASH_PAGE_HALFWORDS];
uint16_t sil_imu_offsets_page[SIL_FLASH_PAGE_HALFWORDS];
uint16_t sil_update_request_page[SIL_FLASH_PAGE_HALFWORDS];

struct flash_page {
    uint16_t *halfwords;
    uint32_t address;
};

static const struct flash_page flash_pages[] = {
    {sil_settings_page, SIL_SETTINGS_PAGE_ADDRESS},
    {sil_calibrated_page, SIL_CALIBRATED_PAGE_ADDRESS},
    {sil_imu_offsets_page, SIL_IMU_OFFSETS_PAGE_ADDRESS},
    {sil_update_request_page, SIL_UPDATE_REQUEST_PAGE_ADDRESS},
};

#define FLASH_PAGE_COUNT (sizeof(flash_pages) / sizeof(flash_pages[0]))

/* The app's interrupt handlers (apps/begode-extreme/control.c), by the names the vector table gives them, as
 * ports/stm32f1/startup.c declares them for the image. */
void TIM8_UP_IRQHandler(void);
void ADC1_2_IRQHandler(void);
void USART1_IRQHandler(void);
void RTC_IRQHandler(void);

struct context {
    int32_t irq;
    const char *name;
    void (*handler)(void);
};

/* The contexts, in no particular order: the host finds them by IRQ number. */
static const struct context contexts[] = {
    {(int32_t)TIM8_UP_IRQn, "TIM8 update", TIM8_UP_IRQHandler},
    {(int32_t)ADC1_2_IRQn, "ADC1/2 end of injection (FOC)", ADC1_2_IRQHandler},
    {(int32_t)USART1_IRQn, "USART1 (BLE)", USART1_IRQHandler},
    {(int32_t)RTC_IRQn, "RTC second (496 Hz tick)", RTC_IRQHandler},
};

#define CONTEXT_COUNT (sizeof(contexts) / sizeof(contexts[0]))

static struct euc_sil_host host;
static bool attached;
static jmp_buf stop_point;

bool euc_sil_attach(const struct euc_sil_host *table)
{
    attached = (table != NULL) && (table->read != NULL) && (table->write != NULL) && (table->spin != NULL) &&
               (table->returned != NULL) && (table->load != NULL) && (table->store != NULL) &&
               (table->fault != NULL);
    if (attached) {
        host = *table;
    }
    return attached;
}

/* Does what a host call answered: runs the contexts it names, one after the other, until it says go on. */
static void follow(int32_t answer)
{
    int32_t next = answer;
    while (next != EUC_SIL_GO_ON) {
        if ((next < 0) || ((uint32_t)next >= CONTEXT_COUNT)) {
            longjmp(stop_point, 1); /* EUC_SIL_STOP, or an answer that names no context */
        }
        contexts[next].handler();
        next = host.returned(host.context, (uint32_t)next);
    }
}

uint32_t sil_read(uint32_t address, uint32_t bytes)
{
    uint32_t value = 0U;
    follow(host.read(host.context, address, bytes, &value));
    return value;
}

void sil_write(uint32_t address, uint32_t bytes, uint32_t value)
{
    follow(host.write(host.context, address, bytes, value));
}

void sil_spin(uint32_t cycles)
{
    follow(host.spin(host.context, cycles));
}

void sil_load(uint32_t address, uint8_t *bytes, uint32_t count)
{
    host.load(host.context, address, bytes, count);
}

void sil_store(uint32_t address, const uint8_t *bytes, uint32_t count)
{
    host.store(host.context, address, bytes, count);
}

void sil_fault(const char *what)
{
    host.fault(host.context, what);
    longjmp(stop_point, 1);
}

uint32_t sil_flash_address(const uint16_t *halfword)
{
    const uintptr_t at = (uintptr_t)halfword;
    for (uint32_t i = 0U; i < FLASH_PAGE_COUNT; i++) {
        const uintptr_t start = (uintptr_t)flash_pages[i].halfwords;
        if ((at >= start) && ((at - start) < SIL_FLASH_PAGE_BYTES)) {
            return flash_pages[i].address + (uint32_t)(at - start);
        }
    }
    sil_fault("flash write to a pointer outside the storage pages");
    return 0U; /* not reached: sil_fault stops the firmware */
}

void sil_flash_reload(uint32_t address)
{
    for (uint32_t i = 0U; i < FLASH_PAGE_COUNT; i++) {
        if ((address >= flash_pages[i].address) &&
            ((address - flash_pages[i].address) < SIL_FLASH_PAGE_BYTES)) {
            sil_load(flash_pages[i].address, (uint8_t *)flash_pages[i].halfwords, SIL_FLASH_PAGE_BYTES);
        }
    }
}

uint32_t euc_sil_run(void)
{
    static bool started;
    if (!attached) {
        return EUC_SIL_NOT_ATTACHED;
    }
    if (started) {
        return EUC_SIL_SPENT;
    }
    started = true;
    if (setjmp(stop_point) == 0) {
        /* The reset path (ports/stm32f1/startup.c): VTOR at the image's vector table, as SystemInit does. */
        sil_write(SCB_VTOR, 4U, SIL_IMAGE_ADDRESS);
        for (uint32_t i = 0U; i < FLASH_PAGE_COUNT; i++) {
            sil_flash_reload(flash_pages[i].address);
        }
        app_init();
        for (;;) {
            app_main_loop_pass();
            sil_spin(EUC_SIL_MAIN_LOOP_PASS_CYCLES);
        }
    }
    return EUC_SIL_STOPPED;
}

uint32_t euc_sil_context_count(void)
{
    return (uint32_t)CONTEXT_COUNT;
}

int32_t euc_sil_context_irq(uint32_t index)
{
    return (index < CONTEXT_COUNT) ? contexts[index].irq : -1;
}

const char *euc_sil_context_name(uint32_t index)
{
    return (index < CONTEXT_COUNT) ? contexts[index].name : NULL;
}

/* Probes ------------------------------------------------------------------------------------------------ */

struct probe {
    const char *name;
    const char *kind;
    const char *role;
    const char *note;
    size_t offset;
    size_t size;
};

/* tools/gen_probes.py --emit-sil writes the list (the same one probes.toml is made from); the offsets and
 * sizes are this build's. */
static const struct probe probes[] = {
#define EUC_SIL_PROBE(name, member, kind, role, note)                                                        \
    {name, kind, role, note, offsetof(struct app, member), sizeof(app.member)},
#include "euc_sil_probes.h"
#undef EUC_SIL_PROBE
};

#define PROBE_COUNT (sizeof(probes) / sizeof(probes[0]))

uint32_t euc_sil_probe_count(void)
{
    return (uint32_t)PROBE_COUNT;
}

const char *euc_sil_probe_name(uint32_t index)
{
    return (index < PROBE_COUNT) ? probes[index].name : NULL;
}

const char *euc_sil_probe_kind(uint32_t index)
{
    return (index < PROBE_COUNT) ? probes[index].kind : NULL;
}

const char *euc_sil_probe_role(uint32_t index)
{
    return (index < PROBE_COUNT) ? probes[index].role : NULL;
}

const char *euc_sil_probe_note(uint32_t index)
{
    return (index < PROBE_COUNT) ? probes[index].note : NULL;
}

uint32_t euc_sil_probe_offset(uint32_t index)
{
    return (index < PROBE_COUNT) ? (uint32_t)probes[index].offset : 0U;
}

uint32_t euc_sil_probe_size(uint32_t index)
{
    return (index < PROBE_COUNT) ? (uint32_t)probes[index].size : 0U;
}

const void *euc_sil_state(void)
{
    return &app;
}

uint32_t euc_sil_state_bytes(void)
{
    return (uint32_t)sizeof(app);
}
