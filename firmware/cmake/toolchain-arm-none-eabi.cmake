# Cross toolchain for the controller MCU: Arm GNU Toolchain (arm-none-eabi), Cortex-M3, soft float.
#
# The compiler is searched in this order:
#   1. $ARM_GNU_TOOLCHAIN_DIR/bin (CI sets it)
#   2. ~/Applications/arm-gnu-toolchain/<release>/bin, newest release first (no PATH edit needed)
#   3. PATH
# Tested release: Arm GNU Toolchain 15.3.rel2 (GCC 15.3.1).

set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_SYSTEM_PROCESSOR arm)

set(euc_toolchain_hints "")
if(DEFINED ENV{ARM_GNU_TOOLCHAIN_DIR})
    list(APPEND euc_toolchain_hints "$ENV{ARM_GNU_TOOLCHAIN_DIR}/bin")
endif()
file(GLOB euc_local_toolchains LIST_DIRECTORIES true "$ENV{HOME}/Applications/arm-gnu-toolchain/*/bin")
list(SORT euc_local_toolchains COMPARE NATURAL ORDER DESCENDING)
list(APPEND euc_toolchain_hints ${euc_local_toolchains})

find_program(EUC_ARM_GCC arm-none-eabi-gcc HINTS ${euc_toolchain_hints} REQUIRED)
get_filename_component(euc_toolchain_bin "${EUC_ARM_GCC}" DIRECTORY)

set(CMAKE_C_COMPILER "${EUC_ARM_GCC}")
set(CMAKE_OBJCOPY "${euc_toolchain_bin}/arm-none-eabi-objcopy" CACHE FILEPATH "objcopy for the target")
set(CMAKE_SIZE "${euc_toolchain_bin}/arm-none-eabi-size" CACHE FILEPATH "size for the target")

# Bare metal: there is no C library to link a test executable against.
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

# Architecture flags go on every compile and link line, so the driver also picks the matching libgcc multilib
# (thumb/v7-m/nofp). No FPU on this MCU: float and double are libgcc soft-float calls.
set(CMAKE_C_FLAGS_INIT "-mcpu=cortex-m3 -mthumb -mfloat-abi=soft")

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
