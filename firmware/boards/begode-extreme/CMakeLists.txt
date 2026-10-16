# Board support for the Begode Extreme main board: headers, linker script and the MCU port it runs on.
add_library(euc_board_begode_extreme INTERFACE)
target_include_directories(euc_board_begode_extreme INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(euc_board_begode_extreme INTERFACE euc_port_stm32f1)
target_link_options(euc_board_begode_extreme INTERFACE "-T${CMAKE_CURRENT_SOURCE_DIR}/link_app.ld")
set_property(TARGET euc_board_begode_extreme APPEND PROPERTY
    INTERFACE_LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/link_app.ld")
