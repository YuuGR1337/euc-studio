/*
 * System set-up that runs before main().
 */
#include <stdint.h>

#include "stm32f1/device.h"
#include "stm32f1/system.h"

void SystemInit(void)
{
    /*
     * The bootloader enters the image with VTOR still at its own table (0x08000000). Point the core at ours,
     * wherever the board linker script put the image (0x08003000 on the Begode Extreme). The stock
     * application does the same as the first line of its main().
     */
    SCB->VTOR = (uint32_t)(uintptr_t)&stm32f1_vector_table;
    __DSB();

    /*
     * The clock tree stays as the bootloader left it (HSI 8 MHz after a cold reset). HSE 8 MHz x 9 = 72 MHz
     * set-up per RM0008 comes with the F5 port, together with a clean hand-over of anything the bootloader
     * left enabled (NVIC lines, USART1).
     */
}
