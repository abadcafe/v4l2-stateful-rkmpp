/* SPDX-License-Identifier: GPL-2.0 */
#ifndef RB_IDENTITY_H
#define RB_IDENTITY_H
#include <linux/errno.h>
#include <linux/types.h>
/* module: session/identity; responsibility: admit non-reusable wire identities.
 * operations: next32/next64; entities: none; tests: identity_tests.c.
 * Exhaustion preserves both the current identity and the destination. */
static inline int rb_identity_next32(__u32 value, __u32 *next) {
  if (value == ~(__u32)0)
    return -EOVERFLOW;
  *next = value + 1;
  return 0;
}
static inline int rb_identity_next64(__u64 value, __u64 *next) {
  if (value == ~(__u64)0)
    return -EOVERFLOW;
  *next = value + 1;
  return 0;
}
#endif
