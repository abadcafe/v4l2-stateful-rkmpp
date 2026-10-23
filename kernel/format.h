/* SPDX-License-Identifier: GPL-2.0 */
#ifndef RB_FORMAT_H
#define RB_FORMAT_H
#include "bridge_uapi.h"
#include <linux/videodev2.h>
/* module: format; responsibility: negotiate decoder image geometry.
 * operations: new/free, layout/input, normalize/capture, set/update,
 * enumerate/sizes/selection/field; entity: opaque rb_format;
 * tests: tests/kernel/test_formats.py, test_layout_reply.py. */
struct rb_format;
enum v4l2_field rb_format_field(u32 field);
struct rb_format *rb_format_new(void);
void rb_format_free(struct rb_format *f);
const struct rb_message *rb_format_layout(const struct rb_format *f);
const struct v4l2_pix_format_mplane *rb_format_input(const struct rb_format *f);
void rb_format_normalize(struct v4l2_pix_format_mplane *p);
void rb_format_capture(const struct rb_format *f,
                       struct v4l2_pix_format_mplane *p);
void rb_format_set(struct rb_format *f, const struct v4l2_pix_format_mplane *p);
int rb_format_update(struct rb_format *f, const struct rb_message *m);
int rb_format_enumerate(const struct rb_format *f, struct v4l2_fmtdesc *d);
int rb_format_sizes(struct v4l2_frmsizeenum *e);
int rb_format_selection(const struct rb_format *f, struct v4l2_selection *s);
#endif
