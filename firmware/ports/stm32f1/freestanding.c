/*
 * The four C library functions a freestanding GCC build still needs.
 *
 * Images link with -nostdlib, so there is no C library. GCC may still emit calls to memcpy, memmove, memset
 * and memcmp for structure copies and initialisation (its documentation requires a freestanding environment
 * to provide them), and the reset path uses memcpy and memset directly.
 *
 * Byte loops: small, obviously correct, and fast enough for the few hundred bytes the firmware moves. This
 * file is compiled with -fno-tree-loop-distribute-patterns, so GCC cannot turn these loops back into calls to
 * themselves.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

void *memcpy(void *restrict destination, const void *restrict source, size_t length_bytes)
{
    uint8_t *to = destination;
    const uint8_t *from = source;
    for (size_t i = 0U; i < length_bytes; i++) {
        to[i] = from[i];
    }
    return destination;
}

void *memmove(void *destination, const void *source, size_t length_bytes)
{
    uint8_t *to = destination;
    const uint8_t *from = source;
    if ((uintptr_t)to < (uintptr_t)from) {
        for (size_t i = 0U; i < length_bytes; i++) {
            to[i] = from[i];
        }
    } else {
        for (size_t i = length_bytes; i > 0U; i--) {
            to[i - 1U] = from[i - 1U];
        }
    }
    return destination;
}

void *memset(void *destination, int value, size_t length_bytes)
{
    uint8_t *to = destination;
    const uint8_t byte = (uint8_t)value;
    for (size_t i = 0U; i < length_bytes; i++) {
        to[i] = byte;
    }
    return destination;
}

int memcmp(const void *left, const void *right, size_t length_bytes)
{
    const uint8_t *a = left;
    const uint8_t *b = right;
    for (size_t i = 0U; i < length_bytes; i++) {
        if (a[i] != b[i]) {
            return (a[i] < b[i]) ? -1 : 1;
        }
    }
    return 0;
}
