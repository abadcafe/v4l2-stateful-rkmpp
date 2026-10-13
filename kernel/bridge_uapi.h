/* SPDX-License-Identifier: GPL-2.0 WITH Linux-syscall-note */
#ifndef V4L2_RKMPP_BRIDGE_UAPI_H
#define V4L2_RKMPP_BRIDGE_UAPI_H
#include <linux/ioctl.h>
#include <linux/types.h>
#define RB_VERSION 2
#define RB_BATCH_MAX 16
#define RB_MAX_BUFFERS 32
/* All reserved fields must be zero. No pointer-valued protocol fields. */
struct rb_message {
  __u32 version, type;
  __u64 epoch;
  __u32 index, flags;
  __u64 timestamp;
  __u32 bytesused, codec;
  __u32 width, height, stride, rows;
  __u32 size, format;
  __u32 count, generation;
  __u32 colorspace, xfer_func, ycbcr_enc, quantization;
  __u32 pool_id, reserved[3];
};
struct rb_registered_buffer {
  __s32 fd;
  __u32 size;
};
struct rb_pool {
  __u32 version, type;
  __u64 epoch;
  __u32 pool_id, count, reserved[2];
  struct rb_registered_buffer buffers[RB_MAX_BUFFERS];
};
/* Replies are applied in order, with one errno result each. Events are consumed
 * only after copyout. A copyout fault after applying replies poisons the
 * session; never retry that exchange. Registration closes all new fds on
 * copyout failure. */
struct rb_batch {
  __u32 version, reply_count, event_count, reserved;
  struct rb_message replies[RB_BATCH_MAX], events[RB_BATCH_MAX];
  __s32 results[RB_BATCH_MAX];
};
enum rb_event {
  RB_PACKET = 1,
  RB_CAPTURE,
  RB_RECYCLE,
  RB_DRAIN,
  RB_RESET,
  RB_CLOSE,
  RB_RESUME
};
enum rb_reply {
  RB_LAYOUT = 32,
  RB_INPUT_DONE,
  RB_FRAME,
  RB_EOS,
  RB_STOPPED,
  RB_FAILED
};
#define RB_RESET_CAPTURE_ONLY (1U << 16)
#define RB_FRAME_LAST 1
#define RB_FMT_NV12 1
#define RB_FMT_P010 2
/* Broker ACCEPT returns a per-session anonymous fd as ioctl return value. */
#define RB_ACCEPT _IO('R', 0x80)
#define RB_EXCHANGE _IOWR('R', 0x84, struct rb_batch)
#define RB_REGISTER_POOL _IOWR('R', 0x85, struct rb_pool)
#endif
