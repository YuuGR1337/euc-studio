/*
 * `.euc_info` image block, layout 1 (spec: docs/spec/euc-info.md at the repository root).
 *
 * Every image built from this tree carries one 128-byte block at image offset 0x140, right after the vector
 * table. The OTA flasher, `euc image validate` and the studio read it to identify an image before it goes
 * near a wheel. This header is the C side of the format and crates/euc-image is the Rust side; both follow
 * the spec.
 *
 * All fields are little-endian (the byte order of every supported MCU). Strings are ASCII, NUL padded.
 */
#ifndef EUC_IMAGE_EUC_INFO_H
#define EUC_IMAGE_EUC_INFO_H

#include <stddef.h>
#include <stdint.h>

#define EUC_INFO_MAGIC      0x46435545U /* bytes 45 55 43 46, ASCII "EUCF" */
#define EUC_INFO_LAYOUT     1U
#define EUC_INFO_SIZE_BYTES 128U

/* String field sizes, NUL padding included. */
#define EUC_INFO_BOARD_BYTES   16U
#define EUC_INFO_PROFILE_BYTES 8U
#define EUC_INFO_GIT_SHA_BYTES 12U

#define EUC_INFO_FLAG_DEV 0x0001U /* dev build: unsigned or signed with a dev key */

struct euc_info {
    uint32_t magic;                       /* EUC_INFO_MAGIC */
    uint16_t layout;                      /* EUC_INFO_LAYOUT */
    uint16_t size;                        /* size of this block in bytes: EUC_INFO_SIZE_BYTES */
    char board[EUC_INFO_BOARD_BYTES];     /* board id, e.g. "begode-extreme" */
    char profile[EUC_INFO_PROFILE_BYTES]; /* "compat" or "clean" */
    uint16_t ver_major;                   /* firmware semver: major */
    uint16_t ver_minor;                   /* firmware semver: minor */
    uint16_t ver_patch;                   /* firmware semver: patch */
    uint16_t flags;                       /* EUC_INFO_FLAG_*; other bits 0 */
    char git_sha[EUC_INFO_GIT_SHA_BYTES]; /* short git sha of the source tree */
    uint32_t image_len;                   /* whole image in bytes; 0 until `euc image seal` */
    uint32_t crc32;                       /* CRC-32/ISO-HDLC over the image (spec); 0 until sealed */
    uint8_t signature[64];                /* Ed25519 over the same bytes; all zero = unsigned */
    uint32_t reserved;                    /* 0 */
};

_Static_assert(offsetof(struct euc_info, magic) == 0x00U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, layout) == 0x04U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, size) == 0x06U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, board) == 0x08U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, profile) == 0x18U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, ver_major) == 0x20U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, ver_minor) == 0x22U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, ver_patch) == 0x24U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, flags) == 0x26U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, git_sha) == 0x28U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, image_len) == 0x34U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, crc32) == 0x38U, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, signature) == 0x3CU, "euc_info layout 1");
_Static_assert(offsetof(struct euc_info, reserved) == 0x7CU, "euc_info layout 1");
_Static_assert(sizeof(struct euc_info) == EUC_INFO_SIZE_BYTES, "euc_info layout 1");

#endif
