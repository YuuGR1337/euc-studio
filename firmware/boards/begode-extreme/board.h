/*
 * Begode Extreme main board: STM32F103RC-class MCU (ST or a GD32/APM32/AT32 clone, vendor not confirmed).
 *
 * Board constants only: facts about this PCB, with units in the names. No code, no tuning (stock tuning goes
 * in tuning_stock.c). The simulator's view of the same board is profiles/begode-extreme/board.toml.
 *
 * The memory map lives in link_app.ld, the one place it is written down. Code that needs an address of the
 * image takes it from the linker symbols (ld_*).
 */
#ifndef BOARD_BEGODE_EXTREME_H
#define BOARD_BEGODE_EXTREME_H

/* Identity --------------------------------------------------------------------------------------------- */

/* Board id in the .euc_info block and directory name under boards/ and profiles/ (at most 15 characters). */
#define BOARD_ID "begode-extreme"

/* Clocks ----------------------------------------------------------------------------------------------- */
/* HSE crystal and PLL settings: added with the clock driver (F5). */

/* Pins ------------------------------------------------------------------------------------------------- */
/* Motor PWM, halls, current sense, IMU SPI, UARTs, buttons and outputs: added with each driver (F1 to F5). */

/* Analog scaling --------------------------------------------------------------------------------------- */
/* Battery divider, current-sense gain, NTC: added with the ADC path (F2, F3). */

#endif
