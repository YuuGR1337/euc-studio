/*
 * CMSIS device header for STM32F103 high-density parts (STM32F103xC/D/E) and pin-compatible clones.
 *
 * Our own definitions, written from the reference manual RM0008 (vector table: "Vector table for other
 * STM32F10xxx devices"). It gives CMSIS-Core the interrupt numbers and core configuration it needs, then
 * includes core_cm3.h for the Cortex-M3 core registers (SCB, NVIC, SysTick) and intrinsics.
 * Peripheral register maps (RCC, GPIO, TIM, ADC, ...) are added with the drivers that use them.
 */
#ifndef STM32F1_DEVICE_H
#define STM32F1_DEVICE_H

#include "stm32f1/irqn.h"

/*
 * Core configuration for core_cm3.h. CMSIS fixes these names, so they break the reserved-identifier rules.
 * NOLINTBEGIN(bugprone-reserved-identifier,cert-dcl37-c,cert-dcl51-cpp)
 */
#define __CM3_REV              0x0101U /* r1p1: CPUID 0x411FC231 on ST STM32F103 */
#define __MPU_PRESENT          0U      /* this firmware does not use the MPU */
#define __VTOR_PRESENT         1U
#define __NVIC_PRIO_BITS       4U /* 16 priority levels, upper nibble of each IPR byte */
#define __Vendor_SysTickConfig 0U
/* NOLINTEND(bugprone-reserved-identifier,cert-dcl37-c,cert-dcl51-cpp) */

#include "core_cm3.h"

#endif
