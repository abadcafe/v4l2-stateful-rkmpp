// SPDX-License-Identifier: GPL-2.0
/* module: session; responsibility: enforce V4L2 buffer ownership transitions.
 * operations: video file/ioctl operations; entity: opaque rb_session;
 * tests: tests/kernel queue, commands, frame replies and reconfiguration.
 * Format negotiation and broker discovery are independent child contracts. */
#include "session.h"
#include "bridge_uapi.h"
#include "format.h"
#include "memory.h"
#include "session/broker.h"
#include "session/capture.h"
#include <linux/anon_inodes.h>
#include <linux/dma-buf.h>
#include <linux/fdtable.h>
#include <linux/file.h>
#include <linux/kref.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/poll.h>
#include <linux/pagemap.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <media/v4l2-ctrls.h>
#include <media/v4l2-device.h>
#include <media/v4l2-event.h>
#include <media/v4l2-ioctl.h>
#include <media/videobuf2-v4l2.h>
#define RB_EVENTS 128
#define RB_CAPTURE_SLOT VIDEO_MAX_FRAME
enum rb_flow {
  RB_FLOW_RUNNING,
  RB_FLOW_DRAINING,
  RB_FLOW_DRAINED,
  RB_FLOW_RESTARTABLE
};
struct rb_session {
  struct kref ref;
  struct mutex lock, ioctls;
  struct v4l2_fh fh;
  struct vb2_queue output, capture;
  struct v4l2_ctrl_handler ctrls;
  struct rb_offer *offer;
  wait_queue_head_t wait;
  struct rb_message events[RB_EVENTS];
  unsigned head, tail, capture_dequeuers, dequeuers;
  u64 epoch, incarnation;
  u32 generation, codec, input_size, sequence, output_pool, capture_pool;
  struct rb_format *format;
  struct rb_capture *capture_contract;
  u32 output_sequence;
  bool attached, closed, failed, stopping, stopped, out_started, cap_started,
      eos;
  enum rb_flow flow;
  bool output_sent[RB_MAX_BUFFERS], available[RB_MAX_BUFFERS];
  bool before_stop[RB_MAX_BUFFERS];
  u32 input_serial, output_serial[RB_MAX_BUFFERS];
  u32 capture_serial, capture_token[RB_MAX_BUFFERS];
  u32 unpaired_field;
};
/* An endpoint belongs to one decoder incarnation, not the lifetime of the
 * video fd. Old DMA-BUF exports retain old storage but cannot acquire new work. */
struct rb_endpoint {
  struct rb_session *session;
  u64 incarnation;
  struct rb_batch batch; /* Serialized by session.lock; no per-exchange allocation. */
};
static bool drain_busy(const struct rb_session *s) {
  return s->flow == RB_FLOW_DRAINING || s->flow == RB_FLOW_DRAINED;
}
static bool drain_complete(const struct rb_session *s) {
  return s->flow == RB_FLOW_DRAINED || s->flow == RB_FLOW_RESTARTABLE;
}
static int attach_service(void *owner);
static struct rb_session *session(void *priv) {
  return container_of(priv, struct rb_session, fh);
}
static struct vb2_queue *queue_for(struct rb_session *s, unsigned type) {
  if (type == V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE)
    return &s->output;
  if (type == V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE)
    return &s->capture;
  return NULL;
}
static void fail(struct rb_session *s) {
  s->failed = true;
  memset(s->available, 0, sizeof(s->available));
  memset(s->output_sent, 0, sizeof(s->output_sent));
  vb2_queue_error(&s->output);
  vb2_queue_error(&s->capture);
  wake_up_all(&s->wait);
}
static void push(struct rb_session *s, struct rb_message m) {
  if (s->head - s->tail == RB_EVENTS) {
    fail(s);
    return;
  }
  m.pool_id = m.type == RB_PACKET ? s->output_pool : s->capture_pool;
  m.version = RB_VERSION;
  m.epoch = s->epoch;
  m.generation = s->generation;
  s->events[s->head++ % RB_EVENTS] = m;
  if (s->head - s->tail == 1)
    wake_up_all(&s->wait);
}
static void return_all(struct vb2_queue *q, enum vb2_buffer_state state) {
  unsigned i;
  for (i = 0; i < q->num_buffers; i++)
    if (q->bufs[i] && q->bufs[i]->state == VB2_BUF_STATE_ACTIVE)
      vb2_buffer_done(q->bufs[i], state);
}
static void destroy(struct kref *ref) {
  struct rb_session *s = container_of(ref, struct rb_session, ref);
  rb_broker_destroy(s->offer);
  rb_format_free(s->format);
  rb_capture_free(s->capture_contract);
  kfree(s);
}
static int setup(struct vb2_queue *q, unsigned *nb, unsigned *np,
                 unsigned sizes[], struct device *devs[]) {
  struct rb_session *s = vb2_get_drv_priv(q);
  unsigned size;
  if (s->failed || s->stopping)
    return -EIO;
  size = V4L2_TYPE_IS_OUTPUT(q->type) ? s->input_size
                                      : rb_capture_size(s->capture_contract);
  if (!size)
    return -EINVAL;
  if (*np) {
    if (*np != 1 || sizes[0] < size)
      return -EINVAL;
    return 0;
  }
  *np = 1;
  sizes[0] = size;
  *nb = min_t(unsigned, *nb, RB_MAX_BUFFERS);
  if (!V4L2_TYPE_IS_OUTPUT(q->type))
    *nb = max_t(unsigned, *nb, rb_format_layout(s->format)->count);
  return 0;
}
static int prepare(struct vb2_buffer *b) {
  struct rb_session *s = vb2_get_drv_priv(b->vb2_queue);
  if (s->failed || s->stopping)
    return -EIO;
  if (V4L2_TYPE_IS_OUTPUT(b->type) &&
      vb2_get_plane_payload(b, 0) > vb2_plane_size(b, 0))
    return -EINVAL;
  if (!V4L2_TYPE_IS_OUTPUT(b->type))
    return rb_capture_prepare(s->capture_contract, vb2_plane_size(b, 0));
  return 0;
}
static void submit_packet(struct rb_session *s, struct vb2_buffer *b) {
  if (s->output_sent[b->index] || s->flow != RB_FLOW_RUNNING)
    return;
  if (s->input_serial == U32_MAX) {
    fail(s);
    return;
  }
  s->output_serial[b->index] = ++s->input_serial;
  s->output_sent[b->index] = true;
  push(s, (struct rb_message){.type = RB_PACKET,
                              .count = s->input_serial,
                              .index = b->index,
                              .bytesused = vb2_get_plane_payload(b, 0) -
                                           b->planes[0].data_offset,
                              .data_offset = b->planes[0].data_offset,
                              .timestamp = b->timestamp,
                              .flags = to_vb2_v4l2_buffer(b)->flags &
                                       V4L2_BUF_FLAG_TSTAMP_SRC_MASK,
                              .codec = s->codec});
}
/* vb2's queued list preserves QBUF order, including deferred input. */
static void submit_queued(struct rb_session *s) {
  struct vb2_buffer *b;
  list_for_each_entry(b, &s->output.queued_list,
                      queued_entry) if (b->state == VB2_BUF_STATE_ACTIVE)
      submit_packet(s, b);
}
static bool last_buffer(struct rb_session *s) {
  unsigned i;
  for (i = 0; i < s->capture.num_buffers; i++)
    if (s->available[i]) {
      struct vb2_buffer *b = s->capture.bufs[i];
      s->available[i] = false;
      to_vb2_v4l2_buffer(b)->flags |= V4L2_BUF_FLAG_LAST;
      for (unsigned p = 0; p < b->num_planes; p++)
        vb2_set_plane_payload(b, p, 0);
      vb2_buffer_done(b, VB2_BUF_STATE_DONE);
      s->eos = false;
      return true;
    }
  return false;
}
/* A sufficient pool may resume before the client dequeues the source-change
 * marker. Keep that empty marker pending until DQBUF requests it; otherwise
 * START would leave a stale LAST buffer in videobuf2's completed queue. */
static void complete_last(struct rb_session *s) {
  if (s->eos &&
      (!rb_capture_pending(s->capture_contract) || s->capture_dequeuers))
    last_buffer(s);
  if (s->eos)
    wake_up_all(&s->capture.done_wq);
}
static void queued(struct vb2_buffer *b) {
  struct rb_session *s = vb2_get_drv_priv(b->vb2_queue);
  if (V4L2_TYPE_IS_OUTPUT(b->type)) {
    to_vb2_v4l2_buffer(b)->sequence = s->output_sequence++;
    if (s->out_started)
      submit_packet(s, b);
  } else {
    if (s->capture_serial == U32_MAX) {
      fail(s);
      return;
    }
    s->capture_token[b->index] = ++s->capture_serial;
    s->available[b->index] = true;
    if (s->cap_started && s->out_started) {
      if (s->eos) {
        complete_last(s);
        if (!s->available[b->index])
          return;
      }
      push(s, (struct rb_message){.type = RB_RECYCLE, .index = b->index,
                                  .count = s->capture_token[b->index]});
    }
  }
}
static unsigned long capture_capacity(struct rb_session *s) {
  unsigned long capacity = ULONG_MAX;
  for (unsigned i = 0; i < s->capture.num_buffers; i++) {
    struct vb2_buffer *b = s->capture.bufs[i];
    if (b->num_planes != 1)
      return 0;
    capacity = min(capacity, vb2_plane_size(b, 0));
  }
  return capacity;
}
static bool capture_fits(struct rb_session *s) {
  return rb_capture_fits(s->capture_contract, s->capture.num_buffers,
                         capture_capacity(s));
}
static void activate_capture(struct rb_session *s) {
  if (WARN_ON_ONCE(rb_capture_activate(
          s->capture_contract, s->capture.num_buffers, capture_capacity(s))))
    return;
  s->eos = false;
  s->capture.last_buffer_dequeued = false;
  push(s, (struct rb_message){.type = RB_CAPTURE,
                              .count = s->capture.num_buffers,
                              .flags = RB_CAPTURE_REUSE});
  for (unsigned i = 0; i < s->capture.num_buffers; i++)
    if (s->available[i])
      push(s, (struct rb_message){.type = RB_RECYCLE, .index = i,
                                  .count = s->capture_token[i]});
}
static int start(struct vb2_queue *q, unsigned count) {
  struct rb_session *s = vb2_get_drv_priv(q);
  unsigned i;
  if (s->failed) {
    return_all(q, VB2_BUF_STATE_QUEUED);
    return -EIO;
  }
  if (V4L2_TYPE_IS_OUTPUT(q->type)) {
    s->out_started = true;
    submit_queued(s);
  } else {
    if (!rb_capture_pending(s->capture_contract) &&
        rb_capture_activate(s->capture_contract, q->num_buffers,
                            capture_capacity(s))) {
      return_all(q, VB2_BUF_STATE_QUEUED);
      return -EINVAL;
    }
    s->cap_started = true;
    s->sequence = 0;
    s->unpaired_field = 0;
    if (rb_capture_pending(s->capture_contract) && capture_fits(s)) {
      activate_capture(s);
      return 0;
    }
    complete_last(s);
    push(s, (struct rb_message){.type = RB_CAPTURE, .count = q->num_buffers});
    for (i = 0; i < q->num_buffers; i++)
      if (s->available[i])
        push(s, (struct rb_message){.type = RB_RECYCLE, .index = i,
                                  .count = s->capture_token[i]});
    if (drain_complete(s)) {
      s->flow = RB_FLOW_RUNNING;
      memset(s->before_stop, 0, sizeof(s->before_stop));
      push(s, (struct rb_message){.type = RB_RESUME});
      if (s->out_started)
        submit_queued(s);
    }
  }
  return 0;
}
static void stop(struct vb2_queue *q) {
  struct rb_session *s = vb2_get_drv_priv(q);
  long waited;
  bool capture_only = !V4L2_TYPE_IS_OUTPUT(q->type);
  bool abort_drain = capture_only && s->flow != RB_FLOW_RUNNING;
  if (!s->attached && !capture_only) {
    s->epoch++;
    s->tail = s->head;
  }
  /* A dead service poisons the session: its buffers are never reused. */
  if (!s->failed && s->attached && !s->closed) {
    s->stopping = true;
    s->stopped = false;
    if (!capture_only) {
      s->epoch++;
      s->tail = s->head;
    }
    push(s, (struct rb_message){
                .type = RB_RESET,
                .flags = q->type | (capture_only ? RB_RESET_CAPTURE_ONLY : 0) |
                         (abort_drain ? RB_RESET_ABORT_DRAIN : 0)});
    mutex_unlock(&s->lock);
    waited = wait_event_timeout(s->wait,
                                READ_ONCE(s->stopped) || READ_ONCE(s->failed),
                                msecs_to_jiffies(5000));
    mutex_lock(&s->lock);
    if (!waited)
      fail(s);
    s->stopping = false;
  }
  if (V4L2_TYPE_IS_OUTPUT(q->type)) {
    s->out_started = false;
    s->output_sequence = 0;
    memset(s->output_sent, 0, sizeof(s->output_sent));
  } else {
    s->cap_started = false;
    memset(s->available, 0, sizeof(s->available));
  }
  return_all(q, VB2_BUF_STATE_ERROR);
  s->eos = false;
  if (abort_drain || !capture_only) {
    s->flow = abort_drain ? RB_FLOW_RESTARTABLE : RB_FLOW_RUNNING;
    memset(s->before_stop, 0, sizeof(s->before_stop));
  }
  rb_capture_stop(s->capture_contract);
}
static const struct vb2_ops queue_ops = {.queue_setup = setup,
                                         .buf_prepare = prepare,
                                         .buf_queue = queued,
                                         .start_streaming = start,
                                         .stop_streaming = stop,
                                         .wait_prepare = vb2_ops_wait_prepare,
                                         .wait_finish = vb2_ops_wait_finish};
static int querycap(struct file *f, void *p, struct v4l2_capability *c) {
  strscpy(c->driver, "v4l2_rkmpp", sizeof(c->driver));
  strscpy(c->card, "v4l2_rkmpp_bridge", sizeof(c->card));
  strscpy(c->bus_info, "platform:v4l2_rkmpp_bridge", sizeof(c->bus_info));
  return 0;
}
static int enum_fmt(struct file *f, void *p, struct v4l2_fmtdesc *d) {
  return rb_format_enumerate(session(p)->format, d);
}
static int try_fmt(struct file *f, void *p, struct v4l2_format *fmt) {
  struct rb_session *s = session(p);
  if (!queue_for(s, fmt->type))
    return -EINVAL;
  if (V4L2_TYPE_IS_OUTPUT(fmt->type))
    rb_format_normalize(&fmt->fmt.pix_mp);
  else
    rb_format_capture(s->format, &fmt->fmt.pix_mp);
  return 0;
}
static int get_fmt(struct file *f, void *p, struct v4l2_format *fmt) {
  struct rb_session *s = session(p);
  if (!queue_for(s, fmt->type))
    return -EINVAL;
  if (V4L2_TYPE_IS_OUTPUT(fmt->type)) {
    fmt->fmt.pix_mp = *rb_format_input(s->format);
  } else
    rb_format_capture(s->format, &fmt->fmt.pix_mp);
  return 0;
}
static int set_fmt(struct file *f, void *p, struct v4l2_format *fmt) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, fmt->type);
  int r;
  if (s->failed)
    return -EIO;
  if (!q)
    return -EINVAL;
  if (vb2_is_busy(q) ||
      (V4L2_TYPE_IS_OUTPUT(fmt->type) && vb2_is_busy(&s->capture)))
    return -EBUSY;
  r = try_fmt(f, p, fmt);
  if (r)
    return r;
  if (V4L2_TYPE_IS_OUTPUT(fmt->type)) {
    s->codec = fmt->fmt.pix_mp.pixelformat;
    s->input_size = fmt->fmt.pix_mp.plane_fmt[0].sizeimage;
    rb_format_set(s->format, &fmt->fmt.pix_mp);
    v4l2_ctrl_s_ctrl(v4l2_ctrl_find(&s->ctrls, V4L2_CID_MIN_BUFFERS_FOR_CAPTURE), 18);
    rb_capture_update(s->capture_contract, rb_format_layout(s->format)->size,
                      rb_format_layout(s->format)->count,
                      false);
  }
  return 0;
}
static int selection(struct file *f, void *p, struct v4l2_selection *sel) {
  return rb_format_selection(session(p)->format, sel);
}
static int reqbufs(struct file *f, void *p, struct v4l2_requestbuffers *r) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, r->type);
  int ret;
  if (r->count && (s->failed || s->stopping))
    return -EIO;
  ret = q ? vb2_reqbufs(q, r) : -EINVAL;
  if (!ret) {
    u32 *id = V4L2_TYPE_IS_OUTPUT(r->type) ? &s->output_pool : &s->capture_pool;
    if (++*id == 0)
      ++*id;
  }
  return ret;
}
/* mmap offsets are opaque cookies, independent of allocation sizes. */
static void buffer_cookie(struct v4l2_buffer *b) {
  if (b->memory == V4L2_MEMORY_MMAP)
    b->m.planes[0].m.mem_offset =
        (b->index + (V4L2_TYPE_IS_OUTPUT(b->type) ? 0 : RB_CAPTURE_SLOT))
        << PAGE_SHIFT;
}
static int querybuf(struct file *f, void *p, struct v4l2_buffer *b) {
  struct vb2_queue *q = queue_for(session(p), b->type);
  int r = q ? vb2_querybuf(q, b) : -EINVAL;
  if (!r)
    buffer_cookie(b);
  return r;
}
static int qbuf(struct file *f, void *p, struct v4l2_buffer *b) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, b->type);
  if (s->failed || s->stopping)
    return -EIO;
  int r = q ? vb2_qbuf(q, NULL, b) : -EINVAL;
  if (!r)
    buffer_cookie(b);
  return r;
}
static int dqbuf(struct file *f, void *p, struct v4l2_buffer *b) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, b->type);
  int ret;
  bool capture = q == &s->capture;
  s->dequeuers++;
  if (capture) {
    s->capture_dequeuers++;
    if (s->cap_started)
      complete_last(s);
  }
  ret = q ? vb2_dqbuf(q, b, f->f_flags & O_NONBLOCK) : -EINVAL;
  if (capture)
    s->capture_dequeuers--;
  s->dequeuers--;
  wake_up_all(&s->wait);
  if (!ret)
    buffer_cookie(b);
  if (!ret && V4L2_TYPE_IS_OUTPUT(b->type))
    s->before_stop[b->index] = false;
  if (drain_complete(s) && !rb_capture_pending(s->capture_contract) &&
      s->capture.last_buffer_dequeued &&
      !memchr_inv(s->before_stop, 0, sizeof(s->before_stop)))
    s->flow = RB_FLOW_RESTARTABLE;
  return ret;
}
static int expbuf(struct file *f, void *p, struct v4l2_exportbuffer *e) {
  struct vb2_queue *q = queue_for(session(p), e->type);
  return q ? vb2_expbuf(q, e) : -EINVAL;
}
static int streamon(struct file *f, void *p, enum v4l2_buf_type type) {
  if (session(p)->failed)
    return -EIO;
  struct vb2_queue *q = queue_for(session(p), type);
  return q ? vb2_streamon(q, type) : -EINVAL;
}
static int streamoff(struct file *f, void *p, enum v4l2_buf_type type) {
  struct vb2_queue *q = queue_for(session(p), type);
  return q ? vb2_streamoff(q, type) : -EINVAL;
}
static int try_cmd(struct file *f, void *p, struct v4l2_decoder_cmd *c) {
  if (c->cmd != V4L2_DEC_CMD_STOP && c->cmd != V4L2_DEC_CMD_START)
    return -EINVAL;
  c->flags = 0;
  memset(&c->raw, 0, sizeof(c->raw));
  return 0;
}
static int cmd(struct file *f, void *p, struct v4l2_decoder_cmd *c) {
  struct rb_session *s = session(p);
  if (s->failed)
    return -EIO;
  int r = try_cmd(f, p, c);
  if (r)
    return r;
  if (c->cmd == V4L2_DEC_CMD_START && rb_capture_pending(s->capture_contract)) {
    if (!s->cap_started || !capture_fits(s))
      return -EINVAL;
    activate_capture(s);
    return 0;
  }
  if (drain_busy(s))
    return -EBUSY;
  if (c->cmd == V4L2_DEC_CMD_STOP) {
    if (!s->out_started || !s->cap_started || drain_complete(s))
      return 0;
    s->flow = RB_FLOW_DRAINING;
    for (unsigned i = 0; i < s->output.num_buffers; i++)
      s->before_stop[i] = s->output.bufs[i]->state == VB2_BUF_STATE_ACTIVE ||
                          (s->output.bufs[i]->state == VB2_BUF_STATE_DONE ||
                           s->output.bufs[i]->state == VB2_BUF_STATE_ERROR);
    push(s, (struct rb_message){.type = RB_DRAIN});
  } else {
    if (!drain_complete(s))
      return 0;
    s->capture.last_buffer_dequeued = false;
    s->flow = RB_FLOW_RUNNING;
    push(s, (struct rb_message){.type = RB_RESUME});
    if (s->out_started)
      submit_queued(s);
  }
  return 0;
}
static int subscribe(struct v4l2_fh *fh,
                     const struct v4l2_event_subscription *sub) {
  if (sub->type == V4L2_EVENT_CTRL)
    return v4l2_ctrl_subscribe_event(fh, sub);
  if (sub->type == V4L2_EVENT_SOURCE_CHANGE)
    return v4l2_src_change_event_subscribe(fh, sub);
  if (sub->type == V4L2_EVENT_EOS)
    return v4l2_event_subscribe(fh, sub, 2, NULL);
  return -EINVAL;
}
static int sizes(struct file *f, void *p, struct v4l2_frmsizeenum *e) {
  return rb_format_sizes(e);
}
const struct v4l2_ioctl_ops rb_video_ioctls = {
    .vidioc_querycap = querycap,
    .vidioc_enum_fmt_vid_out = enum_fmt,
    .vidioc_enum_fmt_vid_cap = enum_fmt,
    .vidioc_g_fmt_vid_out_mplane = get_fmt,
    .vidioc_g_fmt_vid_cap_mplane = get_fmt,
    .vidioc_s_fmt_vid_out_mplane = set_fmt,
    .vidioc_s_fmt_vid_cap_mplane = set_fmt,
    .vidioc_try_fmt_vid_out_mplane = try_fmt,
    .vidioc_try_fmt_vid_cap_mplane = try_fmt,
    .vidioc_g_selection = selection,
    .vidioc_reqbufs = reqbufs,
    .vidioc_querybuf = querybuf,
    .vidioc_qbuf = qbuf,
    .vidioc_dqbuf = dqbuf,
    .vidioc_expbuf = expbuf,
    .vidioc_streamon = streamon,
    .vidioc_streamoff = streamoff,
    .vidioc_try_decoder_cmd = try_cmd,
    .vidioc_decoder_cmd = cmd,
    .vidioc_subscribe_event = subscribe,
    .vidioc_unsubscribe_event = v4l2_event_unsubscribe,
    .vidioc_enum_framesizes = sizes};
static int init_queue(struct rb_session *s, struct vb2_queue *q,
                      unsigned type) {
  q->type = type;
  q->io_modes = VB2_MMAP;
  q->drv_priv = s;
  q->buf_struct_size = sizeof(struct vb2_v4l2_buffer);
  q->ops = &queue_ops;
  q->mem_ops = &rb_heap_memops;
  q->timestamp_flags = V4L2_BUF_FLAG_TIMESTAMP_COPY;
  q->lock = &s->lock;
  return vb2_queue_init(q);
}
static int video_open(struct file *f) {
  struct rb_session *s;
  int r;
  s = kzalloc(sizeof(*s), GFP_KERNEL);
  if (!s) {
    return -ENOMEM;
  }
  kref_init(&s->ref);
  mutex_init(&s->lock);
  mutex_init(&s->ioctls);
  init_waitqueue_head(&s->wait);
  s->epoch = 1;
  s->incarnation = 1;
  s->codec = V4L2_PIX_FMT_H264;
  s->input_size = 1024 * 1024;
  s->format = rb_format_new();
  if (!s->format) {
    kref_put(&s->ref, destroy);
    return -ENOMEM;
  }
  s->capture_contract = rb_capture_new(rb_format_layout(s->format)->size);
  if (!s->capture_contract) {
    kref_put(&s->ref, destroy);
    return -ENOMEM;
  }
  v4l2_fh_init(&s->fh, video_devdata(f));
  f->private_data = &s->fh;
  v4l2_ctrl_handler_init(&s->ctrls, 1);
  v4l2_ctrl_new_std(&s->ctrls, NULL, V4L2_CID_MIN_BUFFERS_FOR_CAPTURE, 18, RB_MAX_BUFFERS,
                    1, 18);
  v4l2_ctrl_new_std_menu(&s->ctrls, NULL, V4L2_CID_MPEG_VIDEO_H264_PROFILE,
                         V4L2_MPEG_VIDEO_H264_PROFILE_HIGH,
                         BIT(V4L2_MPEG_VIDEO_H264_PROFILE_EXTENDED),
                         V4L2_MPEG_VIDEO_H264_PROFILE_HIGH);
  v4l2_ctrl_new_std_menu(&s->ctrls, NULL, V4L2_CID_MPEG_VIDEO_VP9_PROFILE, 2,
                         BIT(1), 0);
  v4l2_ctrl_new_std_menu(&s->ctrls, NULL, V4L2_CID_MPEG_VIDEO_HEVC_PROFILE, 2,
                         0, 0);
  s->fh.ctrl_handler = &s->ctrls;
  r = s->ctrls.error;
  if (r)
    goto error;
  r = init_queue(s, &s->output, V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE);
  if (r)
    goto error;
  r = init_queue(s, &s->capture, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE);
  if (r) {
    vb2_queue_release(&s->output);
    goto error;
  }
  v4l2_fh_add(&s->fh);
  s->offer = rb_broker_publish(s, attach_service);
  if (IS_ERR(s->offer)) {
    r = PTR_ERR(s->offer);
    s->offer = NULL;
    v4l2_fh_del(&s->fh);
    vb2_queue_release(&s->capture);
    vb2_queue_release(&s->output);
    goto error;
  }
  return 0;
error:
  v4l2_ctrl_handler_free(&s->ctrls);
  v4l2_fh_exit(&s->fh);
  kref_put(&s->ref, destroy);
  return r;
}
static int video_release(struct file *f) {
  struct rb_session *s = session(f->private_data);
  rb_broker_withdraw(s->offer);
  mutex_lock(&s->lock);
  vb2_queue_release(&s->output);
  vb2_queue_release(&s->capture);
  s->closed = true;
  push(s, (struct rb_message){.type = RB_CLOSE});
  v4l2_fh_del(&s->fh);
  v4l2_fh_exit(&s->fh);
  v4l2_ctrl_handler_free(&s->ctrls);
  mutex_unlock(&s->lock);
  kref_put(&s->ref, destroy);
  return 0;
}
/* Called with the video ioctl transaction held, but without session.lock:
 * broker callbacks acquire session.lock, so withdrawal must precede it. */
static int reinitialize(struct rb_session *s) {
  struct rb_format *format = rb_format_new();
  struct rb_capture *capture;
  struct v4l2_event discarded;
  if (!format)
    return -ENOMEM;
  capture = rb_capture_new(rb_format_layout(format)->size);
  if (!capture) {
    rb_format_free(format);
    return -ENOMEM;
  }
  rb_broker_withdraw(s->offer);
  wait_event(s->wait, !READ_ONCE(s->dequeuers));
  mutex_lock(&s->lock);
  /* Queue teardown has already returned every buffer. Existing blocked DQBUF
   * callers must finish observing that teardown before initialization. */
  while (!v4l2_event_dequeue(&s->fh, &discarded, 1))
    ;
  s->incarnation++;
  s->epoch++;
  s->attached = false;
  s->head = s->tail = 0;
  s->generation = 0;
  s->codec = V4L2_PIX_FMT_H264;
  s->input_size = 1024 * 1024;
  s->sequence = s->output_sequence = s->input_serial = s->capture_serial = 0;
  s->unpaired_field = 0;
  s->eos = s->stopping = s->stopped = false;
  s->out_started = s->cap_started = false;
  s->flow = RB_FLOW_RUNNING;
  memset(s->output_sent, 0, sizeof(s->output_sent));
  memset(s->available, 0, sizeof(s->available));
  memset(s->before_stop, 0, sizeof(s->before_stop));
  memset(s->output_serial, 0, sizeof(s->output_serial));
  memset(s->capture_token, 0, sizeof(s->capture_token));
  rb_format_free(s->format);
  rb_capture_free(s->capture_contract);
  s->format = format;
  s->capture_contract = capture;
  /* REQBUFS(0) cancels vb2 queues, including their error state. */
  s->failed = false;
  wake_up_all(&s->wait);
  mutex_unlock(&s->lock);
  rb_broker_reoffer(s->offer);
  return 0;
}
static long video_ioctl(struct file *f, unsigned cmd, unsigned long arg) {
  struct rb_session *s = session(f->private_data);
  bool serial = cmd != VIDIOC_DQBUF;
  bool recover = false;
  long r;
  /* Event dequeue owns its event lock. DQBUF releases the session lock
   * while waiting. Neither may prevent QBUF or service acknowledgements. */
  if (cmd == VIDIOC_DQEVENT)
    return video_ioctl2(f, cmd, arg);
  /* stop_streaming drops lock to await the service. Keep other mutating
   * ioctls serialized across that entire transaction, including both queues. */
  if (serial && mutex_lock_interruptible(&s->ioctls))
    return -ERESTARTSYS;
  if (mutex_lock_interruptible(&s->lock)) {
    r = -ERESTARTSYS;
    goto out;
  }
  r = video_ioctl2(f, cmd, arg);
  recover = !r && cmd == VIDIOC_REQBUFS && s->failed &&
            !s->output.num_buffers && !s->capture.num_buffers &&
            !vb2_is_streaming(&s->output) && !vb2_is_streaming(&s->capture);
  mutex_unlock(&s->lock);
  if (recover)
    r = reinitialize(s);
out:
  if (serial)
    mutex_unlock(&s->ioctls);
  return r;
}
static __poll_t video_poll(struct file *f, poll_table *wait) {
  struct rb_session *s = session(f->private_data);
  __poll_t mask = 0;
  poll_wait(f, &s->fh.wait, wait);
  mutex_lock(&s->lock);
  if (v4l2_event_pending(&s->fh))
    mask |= EPOLLPRI;
  mask |= vb2_poll(&s->output, f, wait);
  mask |= vb2_poll(&s->capture, f, wait);
  if (s->cap_started && rb_capture_pending(s->capture_contract) && s->eos &&
      memchr_inv(s->available, 0, sizeof(s->available)))
    mask |= EPOLLIN | EPOLLRDNORM;
  if (!s->failed && (s->out_started || s->cap_started))
    mask &= ~EPOLLERR;
  mutex_unlock(&s->lock);
  return mask;
}
static int video_mmap(struct file *f, struct vm_area_struct *vma) {
  struct rb_session *s = session(f->private_data);
  unsigned long slot = vma->vm_pgoff;
  struct vb2_queue *q;
  unsigned index;
  int r = -EINVAL;
  if (slot >= 2 * RB_CAPTURE_SLOT)
    return -EINVAL;
  q = slot < RB_CAPTURE_SLOT ? &s->output : &s->capture;
  index = slot % RB_CAPTURE_SLOT;
  mutex_lock(&s->lock);
  if (index < q->num_buffers && q->bufs[index]) {
    vma->vm_pgoff = q->bufs[index]->planes[0].m.offset >> PAGE_SHIFT;
    r = vb2_mmap(q, vma);
  }
  mutex_unlock(&s->lock);
  return r;
}
const struct v4l2_file_operations rb_video_fops = {.owner = THIS_MODULE,
                                                   .open = video_open,
                                                   .release = video_release,
                                                   .unlocked_ioctl =
                                                       video_ioctl,
                                                   .poll = video_poll,
                                                   .mmap = video_mmap};
static u32 frame_sequence(struct rb_session *s, u32 field) {
  if (field == 3 || field == 4) {
    if (s->unpaired_field && s->unpaired_field != field) {
      s->unpaired_field = 0;
      return s->sequence - 1;
    }
    s->unpaired_field = field;
  } else {
    s->unpaired_field = 0;
  }
  return s->sequence++;
}
/* State admission precedes all reply-specific mutations. Storage identity is
 * checked separately: a valid slot/generation never authorizes an illegal
 * transition (notably a second EOS or a frame after the drain boundary). */
static int admit_reply(const struct rb_session *s, u32 type) {
  if (s->closed || s->failed)
    return -EPIPE;
  if (type == RB_FAILED)
    return 0;
  switch (type) {
  case RB_STOPPED:
    return s->stopping && !s->stopped ? 0 : -EBUSY;
  case RB_EOS:
    return s->flow == RB_FLOW_DRAINING ? 0 : -EBUSY;
  case RB_LAYOUT:
  case RB_FRAME:
  case RB_DRAINING:
    return s->flow == RB_FLOW_RUNNING || s->flow == RB_FLOW_DRAINING
               ? 0 : -EBUSY;
  default:
    return 0;
  }
}
static int reply(struct rb_session *s, const struct rb_message *m) {
  struct vb2_buffer *b;
  struct v4l2_event ev = {0};
  unsigned i;
  int admitted;
  if (m->version != RB_VERSION ||
      memchr_inv(m->reserved, 0, sizeof(m->reserved)))
    return -EINVAL;
  if (m->epoch != s->epoch)
    return -ESTALE;
  admitted = admit_reply(s, m->type);
  if (admitted)
    return admitted;
  switch (m->type) {
  case RB_LAYOUT:
    if (rb_format_update(s->format, m))
      return -EINVAL;
    v4l2_ctrl_s_ctrl(v4l2_ctrl_find(&s->ctrls, V4L2_CID_MIN_BUFFERS_FOR_CAPTURE), m->count);
    rb_capture_update(s->capture_contract, rb_format_layout(s->format)->size,
                      rb_format_layout(s->format)->count,
                      vb2_is_busy(&s->capture));
    if (rb_capture_pending(s->capture_contract)) {
      s->eos = true;
      if (s->cap_started)
        complete_last(s);
    }
    s->generation++;
    ev.type = V4L2_EVENT_SOURCE_CHANGE;
    ev.u.src_change.changes = V4L2_EVENT_SRC_CH_RESOLUTION;
    v4l2_event_queue_fh(&s->fh, &ev);
    return 0;
  case RB_INPUT_DONE:
    if (m->pool_id != s->output_pool)
      return -ESTALE;
    if (m->index >= s->output.num_buffers || !s->output_sent[m->index])
      return -EINVAL;
    if (!m->count || m->count != s->output_serial[m->index])
      return -ESTALE;
    b = s->output.bufs[m->index];
    if (b->state != VB2_BUF_STATE_ACTIVE)
      return -EINVAL;
    s->output_sent[m->index] = false;
    vb2_buffer_done(b, m->flags & RB_FRAME_ERROR ? VB2_BUF_STATE_ERROR
                                                 : VB2_BUF_STATE_DONE);
    return 0;
  case RB_FRAME:
    if (rb_capture_pending(s->capture_contract) ||
        m->pool_id != s->capture_pool || m->generation != s->generation ||
        m->index >= s->capture.num_buffers || !s->available[m->index] ||
        !s->cap_started || !m->count ||
        m->count != s->capture_token[m->index])
      return -ESTALE;
    b = s->capture.bufs[m->index];
    if (m->field > 5 ||
        (m->flags & V4L2_BUF_FLAG_TSTAMP_SRC_MASK) >
            V4L2_BUF_FLAG_TSTAMP_SRC_SOE ||
        m->bytesused > rb_capture_size(s->capture_contract) ||
        m->bytesused > vb2_plane_size(b, 0))
      return -EINVAL;
    s->available[m->index] = false;
    vb2_set_plane_payload(b, 0, m->bytesused);
    b->timestamp = m->timestamp;
    to_vb2_v4l2_buffer(b)->flags =
        (to_vb2_v4l2_buffer(b)->flags & ~V4L2_BUF_FLAG_TSTAMP_SRC_MASK) |
        (m->flags & V4L2_BUF_FLAG_TSTAMP_SRC_MASK);
    to_vb2_v4l2_buffer(b)->sequence = frame_sequence(s, m->field);
    to_vb2_v4l2_buffer(b)->field = rb_format_field(m->field);
    vb2_buffer_done(b, m->flags & RB_FRAME_ERROR ? VB2_BUF_STATE_ERROR
                                                 : VB2_BUF_STATE_DONE);
    return 0;
  case RB_DRAINING:
    if (s->flow != RB_FLOW_RUNNING && s->flow != RB_FLOW_DRAINING)
      return -EBUSY;
    if (!m->count || m->count > s->input_serial)
      return -EINVAL;
    s->flow = RB_FLOW_DRAINING;
    for (i = 0; i < s->output.num_buffers; i++)
      s->before_stop[i] = s->output_serial[i] <= m->count &&
                          (s->output_sent[i] ||
                           s->output.bufs[i]->state == VB2_BUF_STATE_DONE ||
                           s->output.bufs[i]->state == VB2_BUF_STATE_ERROR);
    return 0;
  case RB_EOS:
    if (m->pool_id != s->capture_pool || m->generation != s->generation ||
        rb_capture_pending(s->capture_contract))
      return -ESTALE;
    s->flow = RB_FLOW_DRAINED;
    s->eos = true;
    last_buffer(s);
    ev.type = V4L2_EVENT_EOS;
    v4l2_event_queue_fh(&s->fh, &ev);
    return 0;
  case RB_STOPPED:
    s->stopped = true;
    wake_up_all(&s->wait);
    return 0;
  case RB_FAILED:
    fail(s);
    for (i = 0; i < 2; i++)
      return_all(i ? &s->capture : &s->output, VB2_BUF_STATE_ERROR);
    return 0;
  default:
    return -EINVAL;
  }
}
static long register_pool(struct rb_session *s, void __user *arg) {
  struct rb_pool pool;
  struct vb2_queue *q;
  unsigned i, exported = 0;
  int r;
  if (copy_from_user(&pool, arg, sizeof(pool)))
    return -EFAULT;
  if (pool.version != RB_VERSION ||
      memchr_inv(pool.reserved, 0, sizeof(pool.reserved)))
    return -EINVAL;
  q = queue_for(s, pool.type);
  if (!q)
    return -EINVAL;
  if (s->closed || s->failed || s->stopping)
    return -EPIPE;
  if (!pool.pool_id || pool.epoch != s->epoch ||
      pool.pool_id !=
          (V4L2_TYPE_IS_OUTPUT(pool.type) ? s->output_pool : s->capture_pool))
    return -ESTALE;
  pool.count = q->num_buffers;
  if (!pool.count || pool.count > RB_MAX_BUFFERS)
    return -EINVAL;
  for (i = 0; i < RB_MAX_BUFFERS; i++)
    pool.buffers[i] = (struct rb_registered_buffer){.fd = -1};
  for (i = 0; i < pool.count; i++) {
    struct v4l2_exportbuffer e = {
        .type = pool.type, .index = i, .flags = O_RDWR | O_CLOEXEC};
    r = vb2_expbuf(q, &e);
    if (r)
      goto rollback;
    pool.buffers[i].fd = e.fd;
    pool.buffers[i].size = vb2_plane_size(q->bufs[i], 0);
    exported++;
  }
  if (!copy_to_user(arg, &pool, sizeof(pool)))
    return 0;
  r = -EFAULT;
rollback:
  for (i = 0; i < exported; i++)
    close_fd(pool.buffers[i].fd);
  return r;
}
static long exchange(struct rb_session *s, void __user *arg,
                     struct rb_batch *batch) {
  unsigned i;
  struct rb_batch __user *out = arg;
  if (batch->version != RB_VERSION || batch->reply_count > RB_BATCH_MAX ||
      batch->reserved)
    return -EINVAL;
  /* Probe copyout before any mutation; a later concurrent unmap fails closed.
   */
  if (fault_in_safe_writeable(arg, sizeof(*batch)))
    return -EFAULT;
  for (i = 0; i < batch->reply_count; i++)
    batch->results[i] = reply(s, &batch->replies[i]);
  batch->event_count = min_t(unsigned, s->head - s->tail, RB_BATCH_MAX);
  for (i = 0; i < batch->event_count; i++)
    batch->events[i] = s->events[(s->tail + i) % RB_EVENTS];
  if (copy_to_user(out, batch, offsetof(struct rb_batch, replies)) ||
      copy_to_user(out->events, batch->events,
                   batch->event_count * sizeof(batch->events[0])) ||
      copy_to_user(out->results, batch->results,
                   batch->reply_count * sizeof(batch->results[0]))) {
    if (batch->reply_count)
      fail(s);
    return -EFAULT;
  }
  s->tail += batch->event_count;
  if (!batch->event_count && (s->closed || s->failed))
    return -EPIPE;
  return 0;
}
static long service_ioctl(struct file *f, unsigned cmd, unsigned long arg) {
  struct rb_endpoint *endpoint = f->private_data;
  struct rb_session *s = endpoint->session;
  long r;
  if (cmd != RB_EXCHANGE && cmd != RB_REGISTER_POOL)
    return -ENOTTY;
  if (mutex_lock_interruptible(&s->lock))
    return -ERESTARTSYS;
  if (endpoint->incarnation != s->incarnation)
    r = -EPIPE;
  else if (cmd == RB_EXCHANGE) {
    struct rb_batch *batch = &endpoint->batch;
    struct rb_batch __user *in = (void __user *)arg;
    if (copy_from_user(batch, in, offsetof(struct rb_batch, replies)))
      r = -EFAULT;
    else if (batch->reply_count > RB_BATCH_MAX)
      r = -EINVAL;
    else if (copy_from_user(batch->replies, in->replies,
                           batch->reply_count * sizeof(batch->replies[0])))
      r = -EFAULT;
    else
      r = exchange(s, (void __user *)arg, &endpoint->batch);
  } else
    r = register_pool(s, (void __user *)arg);
  mutex_unlock(&s->lock);
  return r;
}
static __poll_t service_poll(struct file *f, poll_table *wait) {
  struct rb_endpoint *endpoint = f->private_data;
  struct rb_session *s = endpoint->session;
  __poll_t mask = 0;
  poll_wait(f, &s->wait, wait);
  mutex_lock(&s->lock);
  if (endpoint->incarnation != s->incarnation) {
    mask = EPOLLHUP | EPOLLERR;
    goto out;
  }
  if (s->head != s->tail)
    mask |= EPOLLIN;
  if (s->closed)
    mask |= EPOLLHUP;
  if (s->failed)
    mask |= EPOLLERR;
out:
  mutex_unlock(&s->lock);
  return mask;
}
static int service_release(struct inode *inode, struct file *f) {
  struct rb_endpoint *endpoint = f->private_data;
  struct rb_session *s = endpoint->session;
  mutex_lock(&s->lock);
  if (endpoint->incarnation == s->incarnation) {
    s->attached = false;
    if (!s->closed)
      fail(s);
  }
  mutex_unlock(&s->lock);
  kfree(endpoint);
  kref_put(&s->ref, destroy);
  return 0;
}
static const struct file_operations service_fops = {
    .owner = THIS_MODULE,
    .unlocked_ioctl = service_ioctl,
    .compat_ioctl = service_ioctl,
    .poll = service_poll,
    .release = service_release,
    .llseek = no_llseek};
static int attach_service(void *owner) {
  struct rb_session *s = owner;
  struct rb_endpoint *endpoint;
  int fd;
  mutex_lock(&s->lock);
  if (s->closed || s->failed || s->attached) {
    mutex_unlock(&s->lock);
    return -EPIPE;
  }
  endpoint = kmalloc(sizeof(*endpoint), GFP_KERNEL);
  if (!endpoint) {
    fd = -ENOMEM;
    goto failed;
  }
  kref_get(&s->ref);
  endpoint->session = s;
  endpoint->incarnation = s->incarnation;
  s->attached = true;
  fd = anon_inode_getfd("v4l2-rkmpp-session", &service_fops, endpoint,
                        O_RDWR | O_CLOEXEC);
  if (fd < 0)
    goto failed;
  mutex_unlock(&s->lock);
  return fd;
failed:
  /* The broker has consumed this offer. Every allocation failure must
   * terminate the client queues, including failure before taking a ref. */
  s->attached = false;
  fail(s);
  mutex_unlock(&s->lock);
  if (endpoint) {
    kfree(endpoint);
    kref_put(&s->ref, destroy);
  }
  return fd;
}
