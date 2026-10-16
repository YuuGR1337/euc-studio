# core: pure C logic shared by target images and the SIL library. No HAL calls, no mutable globals.
# M0 is header-only (fixed-point math, quirk switches, image info layout); it becomes a static library when
# the first module with a .c file lands (F1).
add_library(euc_core INTERFACE)
target_include_directories(euc_core INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/include"
    "${PROJECT_SOURCE_DIR}/config")
