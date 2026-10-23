// SPDX-License-Identifier: GPL-2.0
/* Public identity admission contract: advance exactly once, never reuse zero,
 * reject exhaustion without mutating the destination, including in-place use.
 */
#include "identity.h"
#include <assert.h>

int main(void) {
  __u32 small = 0;
  __u64 large = 0;
  assert(rb_identity_next32(small, &small) == 0 && small == 1);
  assert(rb_identity_next64(large, &large) == 0 && large == 1);
  small = ~(__u32)0 - 1;
  large = ~(__u64)0 - 1;
  assert(rb_identity_next32(small, &small) == 0 && small == ~(__u32)0);
  assert(rb_identity_next64(large, &large) == 0 && large == ~(__u64)0);
  assert(rb_identity_next32(small, &small) == -EOVERFLOW);
  assert(rb_identity_next64(large, &large) == -EOVERFLOW);
  assert(small == ~(__u32)0 && large == ~(__u64)0);
  small = 17;
  large = 19;
  assert(rb_identity_next32(~(__u32)0, &small) == -EOVERFLOW && small == 17);
  assert(rb_identity_next64(~(__u64)0, &large) == -EOVERFLOW && large == 19);
  return 0;
}
