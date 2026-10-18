/* SPDX-License-Identifier: GPL-2.0 */
#ifndef RB_CAPTURE_H
#define RB_CAPTURE_H
#include <linux/types.h>
/* module: session/capture; responsibility: admit CAPTURE storage across layout
 * generations. The old draining requirement is private to this contract.
 * operations: new/free, update/stop/activate, prepare/fits/pending/size;
 * entity: opaque rb_capture; tests: tests/kernel/test_reconfigure.py. */
struct rb_capture;
struct rb_capture *rb_capture_new(u32 size);
void rb_capture_free(struct rb_capture *c);
void rb_capture_update(struct rb_capture *c, u32 size, u32 minimum, bool allocated);
void rb_capture_stop(struct rb_capture *c);
int rb_capture_activate(struct rb_capture *c, unsigned count,
                        unsigned long capacity);
int rb_capture_prepare(const struct rb_capture *c, unsigned long capacity);
bool rb_capture_fits(const struct rb_capture *c, unsigned count,
                     unsigned long capacity);
bool rb_capture_pending(const struct rb_capture *c);
u32 rb_capture_size(const struct rb_capture *c);
#endif
