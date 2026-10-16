# SIL port: the firmware compiled for the host as a shared library the simulator engine loads.
# Core is built with the runtime profile here, so quirks are switches instead of constants.
add_library(euc_port_sim OBJECT sil.c)
euc_target_defaults(euc_port_sim)
target_include_directories(euc_port_sim PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_link_libraries(euc_port_sim PUBLIC euc_core)
target_compile_definitions(euc_port_sim PUBLIC EUC_PROFILE_RUNTIME=1)
set_target_properties(euc_port_sim PROPERTIES
    POSITION_INDEPENDENT_CODE ON
    C_VISIBILITY_PRESET hidden)

# build/<preset>/libeuc_sil.{so,dylib} or euc_sil.dll
add_library(euc_sil SHARED)
target_link_libraries(euc_sil PRIVATE euc_port_sim)
set_target_properties(euc_sil PROPERTIES
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}")
