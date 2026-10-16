# Host unit tests (Unity). Run with: ctest --preset host-debug   (or host-sil)

# euc_add_unit_test(<name> SOURCES <file>... [LIBRARIES <target>...] [DEFINITIONS <macro>...])
function(euc_add_unit_test name)
    cmake_parse_arguments(PARSE_ARGV 1 arg "" "" "SOURCES;LIBRARIES;DEFINITIONS")
    add_executable(${name} ${arg_SOURCES})
    euc_target_defaults(${name})
    target_link_libraries(${name} PRIVATE unity ${arg_LIBRARIES})
    target_compile_definitions(${name} PRIVATE ${arg_DEFINITIONS})
    add_test(NAME ${name} COMMAND ${name})
endfunction()

euc_add_unit_test(test_fixed
    SOURCES unit/test_fixed.c
    LIBRARIES euc_core)

euc_add_unit_test(test_quirks_compat
    SOURCES unit/test_quirks.c
    LIBRARIES euc_core
    DEFINITIONS EUC_PROFILE_COMPAT=1)

euc_add_unit_test(test_quirks_clean
    SOURCES unit/test_quirks.c
    LIBRARIES euc_core
    DEFINITIONS EUC_PROFILE_CLEAN=1)

euc_add_unit_test(test_sil
    SOURCES unit/test_sil.c
    LIBRARIES euc_port_sim)
