# Third-party code, fetched at configure time from pinned release archives (sha256 checked).
# Nothing is vendored.
include(FetchContent)

if(CMAKE_CROSSCOMPILING)
    # CMSIS 6 Core (Apache-2.0): Cortex-M core registers (SCB, NVIC, SysTick) and intrinsics. Headers only;
    # the STM32F1 device definitions are our own (ports/stm32f1/include/stm32f1/device.h).
    FetchContent_Declare(cmsis6
        URL https://github.com/ARM-software/CMSIS_6/archive/refs/tags/v6.3.0.tar.gz
        URL_HASH SHA256=331df74000876b8fb07933658cbf402c4d9f831b429258792d2c34ab5a6c5bf7
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
        # The archive has no CMake project; point at a directory that does not exist so nothing is added.
        SOURCE_SUBDIR no-cmake-project)
    FetchContent_MakeAvailable(cmsis6)

    add_library(cmsis_core INTERFACE)
    target_include_directories(cmsis_core SYSTEM INTERFACE "${cmsis6_SOURCE_DIR}/CMSIS/Core/Include")
else()
    # Unity (MIT): unit test framework for the host tests.
    FetchContent_Declare(unity
        URL https://github.com/ThrowTheSwitch/Unity/archive/refs/tags/v2.6.1.tar.gz
        URL_HASH SHA256=b41a66d45a6b99758fb3202ace6178177014d52fc524bf1f72687d93e9867292
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
        SYSTEM)
    FetchContent_MakeAvailable(unity)
endif()
