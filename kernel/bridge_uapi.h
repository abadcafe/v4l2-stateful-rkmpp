/* SPDX-License-Identifier: GPL-2.0 WITH Linux-syscall-note */
/* module: bridge_uapi; responsibility: define the shared bridge wire ABI.
 * operations: accept, exchange and pool registration records;
 * entities: rb_message, rb_batch, rb_registered_buffer, rb_pool, event/reply
 * enums; tests: Rust wire_tests, tests/support/native_tests.py and kernel
 * protocol validation tests. */
#ifndef V4L2_RKMPP_BRIDGE_UAPI_H
#define V4L2_RKMPP_BRIDGE_UAPI_H
#include <linux/ioctl.h>
#include <linux/types.h>
#define RB_VERSION 12
#define RB_BATCH_MAX 16
#define RB_MAX_BUFFERS 32
#define RB_MIN_CAPTURE 2
/* field: 0 progressive, 1 interlaced top first, 2 interlaced bottom first,
 * 3 top only, 4 bottom only, 5 interlaced with unspecified order. */
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
  __u32 pool_id, data_offset, crop_left, crop_top, coded_width, coded_height,
      field, reserved[1];
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
 * session; never retry that exchange. Registration installs new fds only after
 * successful copyout; failures release uninstalled slots and file references.
 */
/* Only header, results[0..reply_count) and events[0..event_count) are
 * written back. Unused array storage is not an output of RB_EXCHANGE. */
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
  RB_FAILED,
  RB_DRAINING
};
#define RB_CAPTURE_REUSE 1U
/* CAPTURE_REUSE republishes retained storage permissions, including after the
 * kernel consumes an empty EOS LAST. It is followed by RECYCLE only for slots
 * still queued in the kernel; every other slot remains pinned until QBUF.
 * It also acknowledges a pending LAYOUT when the client requests that reuse. */
#define RB_RESET_CAPTURE_ONLY (1U << 16)
#define RB_RESET_ABORT_DRAIN (1U << 17)
/* PACKET.count and RECYCLE.count are nonzero queue-submission identities,
 * independent of pool/index reuse. INPUT_DONE and FRAME respectively echo
 * that identity. Old completions cannot acknowledge a newly queued buffer.
 * Counters never wrap: exhaustion poisons the session.
 * RESET.count is a nonzero stop-transaction identity echoed by STOPPED;
 * CAPTURE-only stops share their epoch and generation but never this identity.
 * DRAINING.count is the last
 * input belonging to the ending bitstream, including a split end marker. */
/* PACKET/FRAME flags preserve V4L2_BUF_FLAG_TSTAMP_SRC_MASK verbatim. */
/* Only documented flags are accepted: INPUT_DONE permits RB_FRAME_ERROR;
 * FRAME additionally permits TSTAMP_SRC_SOE. Control replies, RECYCLE,
 * DRAIN, CLOSE and RESUME require zero flags. */
#define RB_FRAME_ERROR 2
#define RB_FMT_NV12 1
#define RB_FMT_P010 2
/* EOS completes an outstanding DRAIN or DRAINING exactly once. No FRAME or
 * LAYOUT may follow completion until RESUME. STOPPED acknowledges one RESET;
 * already-inflight replies may precede that acknowledgement. Failed endpoints
 * never regain access: after both queues are stopped and freed, ACCEPT supplies
 * a new endpoint for the video fd's new decoder incarnation. */
/* LAYOUT echoes the current generation and advances it exactly once on
 * success. Stale layouts cannot roll back a negotiated format; exhaustion
 * rejects the transition instead of reusing a generation. */
/* Broker ACCEPT returns a per-incarnation anonymous fd as ioctl return value.
 */
#define RB_ACCEPT _IO('R', 0x80)
#define RB_EXCHANGE _IOWR('R', 0x84, struct rb_batch)
#define RB_REGISTER_POOL _IOWR('R', 0x85, struct rb_pool)
#endif
