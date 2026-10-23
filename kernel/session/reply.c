// SPDX-License-Identifier: GPL-2.0
#include "reply.h"
#include <linux/errno.h>
#include <linux/string.h>
#include <linux/videodev2.h>

int rb_reply_validate(const struct rb_message *m) {
  u32 flags = 0;
  if (m->version != RB_VERSION ||
      memchr_inv(m->reserved, 0, sizeof(m->reserved)))
    return -EINVAL;
  switch (m->type) {
  case RB_FRAME:
    flags = V4L2_BUF_FLAG_TSTAMP_SRC_SOE | RB_FRAME_ERROR;
    break;
  case RB_INPUT_DONE:
    flags = RB_FRAME_ERROR;
    break;
  case RB_LAYOUT:
  case RB_EOS:
  case RB_STOPPED:
  case RB_FAILED:
  case RB_DRAINING:
    break;
  default:
    return -EINVAL;
  }
  return m->flags & ~flags ? -EINVAL : 0;
}
