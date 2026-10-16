/*
 * STM32F1 reset-time setup shared by startup.c and system.c.
 */
#ifndef STM32F1_SYSTEM_H
#define STM32F1_SYSTEM_H

#include <stddef.h>

#include "stm32f1/device.h"

typedef void (*stm32f1_handler_t)(void);

/*
 * Vector table of the high-density line: 16 Cortex-M3 entries + 60 IRQs = 76 words (0x130 bytes). It must sit
 * at the first byte of the image; the linker script places it with the .isr_vector section.
 */
struct stm32f1_vector_table {
    const void *initial_stack_pointer;        /* loaded into MSP by the core at reset */
    stm32f1_handler_t reset;                  /* exception 1 */
    stm32f1_handler_t nmi;                    /* exception 2 */
    stm32f1_handler_t hard_fault;             /* exception 3 */
    stm32f1_handler_t mem_manage;             /* exception 4 */
    stm32f1_handler_t bus_fault;              /* exception 5 */
    stm32f1_handler_t usage_fault;            /* exception 6 */
    stm32f1_handler_t reserved_7_to_10[4];    /* exceptions 7 to 10 */
    stm32f1_handler_t sv_call;                /* exception 11 */
    stm32f1_handler_t debug_monitor;          /* exception 12 */
    stm32f1_handler_t reserved_13;            /* exception 13 */
    stm32f1_handler_t pend_sv;                /* exception 14 */
    stm32f1_handler_t sys_tick;               /* exception 15 */
    stm32f1_handler_t irq[STM32F1_IRQ_COUNT]; /* exception 16 + IRQn_Type */
};

_Static_assert(offsetof(struct stm32f1_vector_table, irq) == 0x40U, "IRQ 0 is vector entry 16");
_Static_assert(sizeof(struct stm32f1_vector_table) == 0x130U, "76 vector entries of 4 bytes");

extern const struct stm32f1_vector_table stm32f1_vector_table;

/*
 * Runs once from Reset_Handler, after .data and .bss are initialised and before main(). CMSIS name.
 */
void SystemInit(void);

#endif
