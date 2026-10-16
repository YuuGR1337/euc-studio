/*
 * The .euc_info block of this image. The build fills every field except image_len, crc32 and signature, which
 * stay zero until `euc image seal` writes them.
 */
#include "euc/image/euc_info.h"
#include "board.h"
#include "euc_build_info.h"

/* Each string must fit its field with at least one NUL after it. */
_Static_assert(sizeof(BOARD_ID) <= EUC_INFO_BOARD_BYTES, "BOARD_ID is too long for .euc_info");
_Static_assert(sizeof(EUC_BUILD_PROFILE_NAME) <= EUC_INFO_PROFILE_BYTES, "profile name is too long");
_Static_assert(sizeof(EUC_BUILD_GIT_SHA) <= EUC_INFO_GIT_SHA_BYTES, "git sha is too long");

/* No code reads the block: `used` stops the compiler and KEEP in link_app.ld stops the linker dropping it. */
__attribute__((section(".euc_info"), used)) static const struct euc_info euc_info_block = {
    .magic = EUC_INFO_MAGIC,
    .layout = EUC_INFO_LAYOUT,
    .size = EUC_INFO_SIZE_BYTES,
    .board = BOARD_ID,
    .profile = EUC_BUILD_PROFILE_NAME,
    .ver_major = EUC_BUILD_VERSION_MAJOR,
    .ver_minor = EUC_BUILD_VERSION_MINOR,
    .ver_patch = EUC_BUILD_VERSION_PATCH,
    .flags = (EUC_BUILD_DEV != 0) ? EUC_INFO_FLAG_DEV : 0U,
    .git_sha = EUC_BUILD_GIT_SHA,
};
