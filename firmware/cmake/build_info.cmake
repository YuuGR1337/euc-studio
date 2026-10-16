# Build identity for the .euc_info block: version (project VERSION), profile, dev flag and short git sha.
# Fixed at configure time and written to build/<preset>/generated/euc_build_info.h.

set(EUC_GIT_SHA "unknown")
find_package(Git QUIET)
if(GIT_FOUND)
    execute_process(
        COMMAND "${GIT_EXECUTABLE}" rev-parse --short=10 HEAD
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        OUTPUT_VARIABLE git_sha
        RESULT_VARIABLE git_result
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET)
    if(git_result EQUAL 0)
        set(EUC_GIT_SHA "${git_sha}")

        # Re-run configure when HEAD moves, so a new commit never ships with the previous sha.
        execute_process(
            COMMAND "${GIT_EXECUTABLE}" rev-parse --git-path HEAD
            WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
            OUTPUT_VARIABLE git_head_file
            OUTPUT_STRIP_TRAILING_WHITESPACE
            ERROR_QUIET)
        execute_process(
            COMMAND "${GIT_EXECUTABLE}" symbolic-ref -q HEAD
            WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
            OUTPUT_VARIABLE git_branch_ref
            OUTPUT_STRIP_TRAILING_WHITESPACE
            ERROR_QUIET)
        set(git_watch_files "${git_head_file}")
        if(git_branch_ref)
            execute_process(
                COMMAND "${GIT_EXECUTABLE}" rev-parse --git-path "${git_branch_ref}"
                WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
                OUTPUT_VARIABLE git_branch_file
                OUTPUT_STRIP_TRAILING_WHITESPACE
                ERROR_QUIET)
            list(APPEND git_watch_files "${git_branch_file}")
        endif()
        foreach(watch_file IN LISTS git_watch_files)
            cmake_path(ABSOLUTE_PATH watch_file BASE_DIRECTORY "${PROJECT_SOURCE_DIR}")
            if(EXISTS "${watch_file}")
                set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${watch_file}")
            endif()
        endforeach()
    endif()
endif()

if(EUC_DEV_BUILD)
    set(EUC_DEV_BUILD_VALUE 1)
else()
    set(EUC_DEV_BUILD_VALUE 0)
endif()

configure_file(
    "${CMAKE_CURRENT_LIST_DIR}/euc_build_info.h.in"
    "${CMAKE_BINARY_DIR}/generated/euc_build_info.h"
    @ONLY)

add_library(euc_build_info INTERFACE)
target_include_directories(euc_build_info INTERFACE "${CMAKE_BINARY_DIR}/generated")

message(STATUS
    "EUC image: board ${EUC_BOARD}, profile ${EUC_PROFILE}, version ${PROJECT_VERSION}, git ${EUC_GIT_SHA}")
