/* SPDX-License-Identifier: GPL-2.0 */
#ifndef RB_SESSION_H
#define RB_SESSION_H
#include "session/broker.h"
#include <linux/fs.h>
#include <media/v4l2-dev.h>
#include <media/v4l2-ioctl.h>
/* Public entry points are standard kernel operations, not session internals. */
extern const struct v4l2_file_operations rb_video_fops;
extern const struct v4l2_ioctl_ops rb_video_ioctls;
#endif
