/*
 * Inside the SIL library: the host calls the port makes (host.c) and the flash pages it keeps in host memory.
 * Not part of the C ABI (include/euc/sil.h).
 */
#ifndef EUC_SIL_PORT_H
#define EUC_SIL_PORT_H

#include <stdint.h>

/* Register access on the board's bus (1, 2 or 4 bytes). Each one costs simulated time and may let an
 * interrupt context run before it returns, as an access on the MCU can be followed by an interrupt. */
uint32_t sil_read(uint32_t address, uint32_t bytes);
void sil_write(uint32_t address, uint32_t bytes, uint32_t value);
/* CPU time without a bus access, in HCLK cycles. */
void sil_spin(uint32_t cycles);
/* Memory the board shares with the firmware, without time or side effects. */
void sil_load(uint32_t address, uint8_t *bytes, uint32_t count);
void sil_store(uint32_t address, const uint8_t *bytes, uint32_t count);
/* Stops the firmware for good with a reason the engine reports; does not return. */
void sil_fault(const char *what);

/* Flash storage pages (the app's ld_*_page symbols). The library keeps each in host memory; the board's
 * flash model holds the real content. */
#define SIL_FLASH_PAGE_BYTES     2048U
#define SIL_FLASH_PAGE_HALFWORDS (SIL_FLASH_PAGE_BYTES / 2U)

/* The host copies of the pages. The SIL link makes the app's ld_*_page symbols aliases of them
 * (ports/sim/CMakeLists.txt), as the target's linker script places those symbols on the flash pages. */
extern uint16_t sil_settings_page[SIL_FLASH_PAGE_HALFWORDS];
extern uint16_t sil_calibrated_page[SIL_FLASH_PAGE_HALFWORDS];
extern uint16_t sil_imu_offsets_page[SIL_FLASH_PAGE_HALFWORDS];
extern uint16_t sil_update_request_page[SIL_FLASH_PAGE_HALFWORDS];

/* The board address of a halfword in one of the pages; stops the firmware for any other pointer. */
uint32_t sil_flash_address(const uint16_t *halfword);
/* Reloads the host copy of the page that holds `address` from the board, after a flash write. */
void sil_flash_reload(uint32_t address);

#endif
