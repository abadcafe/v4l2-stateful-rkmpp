// SPDX-License-Identifier: GPL-2.0
#include "capture.h"
#include <linux/errno.h>
#include <linux/slab.h>
enum phase { PREPARED, ACTIVE, DRAINING, STOPPED };
struct rb_capture {
  enum phase phase;
  u32 required, active, minimum;
};
struct rb_capture *rb_capture_new(u32 size) {
  struct rb_capture *c = kzalloc(sizeof(*c), GFP_KERNEL);
  if (c) {
    c->required = c->active = size;
    c->minimum = 18;
    c->phase = PREPARED;
  }
  return c;
}
void rb_capture_free(struct rb_capture *c) { kfree(c); }
void rb_capture_update(struct rb_capture *c, u32 size, u32 minimum, bool allocated) {
  c->required = size;
  c->minimum = minimum;
  c->phase = allocated ? DRAINING : PREPARED;
  if (!allocated)
    c->active = size;
}
void rb_capture_stop(struct rb_capture *c) {
  c->phase = STOPPED;
  c->active = c->required;
}
u32 rb_capture_size(const struct rb_capture *c) { return c->required; }
bool rb_capture_pending(const struct rb_capture *c) {
  return c->phase == DRAINING;
}
int rb_capture_prepare(const struct rb_capture *c, unsigned long capacity) {
  return capacity < (c->phase == DRAINING ? c->active : c->required) ? -EINVAL
                                                                     : 0;
}
bool rb_capture_fits(const struct rb_capture *c, unsigned count,
                     unsigned long capacity) {
  return count >= c->minimum && capacity >= c->required;
}
int rb_capture_activate(struct rb_capture *c, unsigned count,
                        unsigned long capacity) {
  if (!rb_capture_fits(c, count, capacity))
    return -EINVAL;
  c->active = c->required;
  c->phase = ACTIVE;
  return 0;
}
