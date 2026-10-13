// SPDX-License-Identifier: GPL-2.0
/* V4L2 ownership and service transport. Codec parsing stays in MPP. */
#include "session.h"
#include "bridge_uapi.h"
#include "memory.h"
#include <linux/anon_inodes.h>
#include <linux/dma-buf.h>
#include <linux/fdtable.h>
#include <linux/file.h>
#include <linux/kref.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/poll.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <media/v4l2-ctrls.h>
#include <media/v4l2-device.h>
#include <media/v4l2-event.h>
#include <media/v4l2-ioctl.h>
#include <media/videobuf2-v4l2.h>
#define RB_EVENTS 128
#define RB_CAPTURE_OFFSET (1UL << 30)
#ifndef V4L2_PIX_FMT_AV1
#define V4L2_PIX_FMT_AV1 v4l2_fourcc('A', 'V', '0', '1')
#endif
struct rb_session {
  struct kref ref;
  struct mutex lock;
  struct v4l2_fh fh;
  struct vb2_queue output, capture;
  struct v4l2_ctrl_handler ctrls;
  struct list_head pending;
  wait_queue_head_t wait;
  struct rb_message events[RB_EVENTS];
  unsigned head, tail;
  u64 epoch;
  u32 generation, codec, input_size, sequence, output_pool, capture_pool;
  struct rb_message layout;
  struct v4l2_pix_format_mplane input_format;
  bool reconfig;
  bool attached, closed, failed, stopping, stopped, out_started, cap_started,
      eos, draining;
  bool output_sent[RB_MAX_BUFFERS], available[RB_MAX_BUFFERS];
};
static LIST_HEAD(pending_sessions);
static DEFINE_MUTEX(broker_lock);
static DECLARE_WAIT_QUEUE_HEAD(broker_wait);
static unsigned live_sessions;
static bool broker_opened;
static const u32 codecs[] = {V4L2_PIX_FMT_H264, V4L2_PIX_FMT_HEVC,
                             V4L2_PIX_FMT_VP8, V4L2_PIX_FMT_VP9,
                             V4L2_PIX_FMT_AV1};
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
  mutex_lock(&broker_lock);
  live_sessions--;
  mutex_unlock(&broker_lock);
  kfree(s);
}
static int setup(struct vb2_queue *q, unsigned *nb, unsigned *np,
                 unsigned sizes[], struct device *devs[]) {
  struct rb_session *s = vb2_get_drv_priv(q);
  unsigned size;
  if (s->failed || s->stopping)
    return -EIO;
  size = V4L2_TYPE_IS_OUTPUT(q->type) ? s->input_size : s->layout.size;
  if (!size)
    return -EINVAL;
  if (!V4L2_TYPE_IS_OUTPUT(q->type) && s->layout.format == RB_FMT_P010) {
    unsigned y = s->layout.stride * s->layout.rows;
    if (*np) {
      if (*np != 2 || sizes[0] < y || sizes[1] < y / 2)
        return -EINVAL;
      return 0;
    }
    *np = 2;
    sizes[0] = y;
    sizes[1] = y / 2;
  } else {
    if (*np) {
      if (*np != 1 || sizes[0] < size)
        return -EINVAL;
      return 0;
    }
    *np = 1;
    sizes[0] = size;
  }
  *nb = min_t(unsigned, *nb, RB_MAX_BUFFERS);
  if (!V4L2_TYPE_IS_OUTPUT(q->type))
    *nb = max_t(unsigned, *nb, 18);
  return 0;
}
static int prepare(struct vb2_buffer *b) {
  struct rb_session *s = vb2_get_drv_priv(b->vb2_queue);
  if (s->failed || s->stopping)
    return -EIO;
  if (V4L2_TYPE_IS_OUTPUT(b->type) &&
      vb2_get_plane_payload(b, 0) > vb2_plane_size(b, 0))
    return -EINVAL;
  return 0;
}
static void submit_packet(struct rb_session *s, struct vb2_buffer *b) {
  if (s->output_sent[b->index])
    return;
  s->output_sent[b->index] = true;
  push(s, (struct rb_message){.type = RB_PACKET,
                              .index = b->index,
                              .bytesused = vb2_get_plane_payload(b, 0),
                              .timestamp = b->timestamp,
                              .codec = s->codec});
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
static void queued(struct vb2_buffer *b) {
  struct rb_session *s = vb2_get_drv_priv(b->vb2_queue);
  if (V4L2_TYPE_IS_OUTPUT(b->type)) {
    if (s->out_started)
      submit_packet(s, b);
  } else {
    s->available[b->index] = true;
    if (s->cap_started && s->out_started) {
      if (s->eos && last_buffer(s))
        return;
      push(s, (struct rb_message){.type = RB_RECYCLE, .index = b->index});
    }
  }
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
    for (i = 0; i < q->num_buffers; i++)
      if (q->bufs[i]->state == VB2_BUF_STATE_ACTIVE)
        submit_packet(s, q->bufs[i]);
  } else {
    s->cap_started = true;
    push(s, (struct rb_message){.type = RB_CAPTURE, .count = q->num_buffers});
    for (i = 0; i < q->num_buffers; i++)
      if (s->available[i])
        push(s, (struct rb_message){.type = RB_RECYCLE, .index = i});
  }
  return 0;
}
static void stop(struct vb2_queue *q) {
  struct rb_session *s = vb2_get_drv_priv(q);
  long waited;
  bool capture_only = !V4L2_TYPE_IS_OUTPUT(q->type) && s->reconfig;
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
                .flags = q->type | (capture_only ? RB_RESET_CAPTURE_ONLY : 0)});
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
    memset(s->output_sent, 0, sizeof(s->output_sent));
  } else {
    s->cap_started = false;
    memset(s->available, 0, sizeof(s->available));
  }
  return_all(q, VB2_BUF_STATE_ERROR);
  s->eos = false;
  if (!capture_only)
    s->draining = false;
  s->reconfig = false;
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
  if (V4L2_TYPE_IS_OUTPUT(d->type)) {
    if (d->index >= ARRAY_SIZE(codecs))
      return -EINVAL;
    d->pixelformat = codecs[d->index];
    d->flags = V4L2_FMT_FLAG_COMPRESSED | V4L2_FMT_FLAG_DYN_RESOLUTION;
    strscpy(d->description, "Compressed bitstream", sizeof(d->description));
  } else {
    struct rb_session *s = session(p);
    if (d->index)
      return -EINVAL;
    d->pixelformat =
        s->layout.format == RB_FMT_P010 ? V4L2_PIX_FMT_P010 : V4L2_PIX_FMT_NV12;
  }
  return 0;
}
static void capture_fmt(struct rb_session *s,
                        struct v4l2_pix_format_mplane *p) {
  memset(p, 0, sizeof(*p));
  p->width = s->layout.width ?: 64;
  p->height = s->layout.rows ?: 64;
  p->pixelformat =
      s->layout.format == RB_FMT_P010 ? V4L2_PIX_FMT_P010 : V4L2_PIX_FMT_NV12;
  p->num_planes = 1;
  p->field = V4L2_FIELD_NONE;
  p->plane_fmt[0].bytesperline = s->layout.stride ?: 64;
  p->plane_fmt[0].sizeimage = s->layout.size ?: 6144;
  if (s->layout.format == RB_FMT_P010) {
    p->num_planes = 2;
    p->plane_fmt[0].sizeimage = s->layout.stride * s->layout.rows;
    p->plane_fmt[1].sizeimage = p->plane_fmt[0].sizeimage / 2;
    p->plane_fmt[1].bytesperline = s->layout.stride;
  }
  p->colorspace = s->layout.colorspace ?: s->input_format.colorspace;
  p->xfer_func = s->layout.xfer_func ?: s->input_format.xfer_func;
  p->ycbcr_enc = s->layout.ycbcr_enc ?: s->input_format.ycbcr_enc;
  p->quantization = s->layout.quantization ?: s->input_format.quantization;
}
static int try_fmt(struct file *f, void *p, struct v4l2_format *fmt) {
  struct rb_session *s = session(p);
  unsigned i;
  if (!queue_for(s, fmt->type))
    return -EINVAL;
  if (!V4L2_TYPE_IS_OUTPUT(fmt->type)) {
    capture_fmt(s, &fmt->fmt.pix_mp);
    return 0;
  }
  for (i = 0; i < ARRAY_SIZE(codecs); i++)
    if (codecs[i] == fmt->fmt.pix_mp.pixelformat)
      break;
  if (i == ARRAY_SIZE(codecs))
    fmt->fmt.pix_mp.pixelformat = V4L2_PIX_FMT_H264;
  fmt->fmt.pix_mp.width = clamp_t(u32, fmt->fmt.pix_mp.width, 64, 8192);
  fmt->fmt.pix_mp.height = clamp_t(u32, fmt->fmt.pix_mp.height, 64, 8192);
  if (!fmt->fmt.pix_mp.colorspace ||
      fmt->fmt.pix_mp.colorspace > V4L2_COLORSPACE_DCI_P3)
    fmt->fmt.pix_mp.colorspace = V4L2_COLORSPACE_REC709;
  fmt->fmt.pix_mp.num_planes = 1;
  fmt->fmt.pix_mp.field = V4L2_FIELD_NONE;
  fmt->fmt.pix_mp.plane_fmt[0].sizeimage =
      clamp_t(u32, fmt->fmt.pix_mp.plane_fmt[0].sizeimage, 1024 * 1024,
              16 * 1024 * 1024);
  return 0;
}
static int get_fmt(struct file *f, void *p, struct v4l2_format *fmt) {
  struct rb_session *s = session(p);
  if (!queue_for(s, fmt->type))
    return -EINVAL;
  if (V4L2_TYPE_IS_OUTPUT(fmt->type)) {
    fmt->fmt.pix_mp = s->input_format;
  } else
    capture_fmt(s, &fmt->fmt.pix_mp);
  return 0;
}
static int set_fmt(struct file *f, void *p, struct v4l2_format *fmt) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, fmt->type);
  int r;
  if (!q)
    return -EINVAL;
  if (vb2_is_busy(q))
    return -EBUSY;
  r = try_fmt(f, p, fmt);
  if (r)
    return r;
  if (V4L2_TYPE_IS_OUTPUT(fmt->type)) {
    s->codec = fmt->fmt.pix_mp.pixelformat;
    s->input_size = fmt->fmt.pix_mp.plane_fmt[0].sizeimage;
    s->input_format = fmt->fmt.pix_mp;
  }
  return 0;
}
static int selection(struct file *f, void *p, struct v4l2_selection *sel) {
  struct rb_session *s = session(p);
  if (sel->type != V4L2_BUF_TYPE_VIDEO_CAPTURE &&
      sel->type != V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE)
    return -EINVAL;
  switch (sel->target) {
  case V4L2_SEL_TGT_COMPOSE:
  case V4L2_SEL_TGT_COMPOSE_DEFAULT:
    sel->r = (struct v4l2_rect){.width = s->layout.width,
                                .height = s->layout.height};
    break;
  case V4L2_SEL_TGT_COMPOSE_BOUNDS:
  case V4L2_SEL_TGT_COMPOSE_PADDED:
    sel->r =
        (struct v4l2_rect){.width = s->layout.width, .height = s->layout.rows};
    break;
  default:
    return -EINVAL;
  }
  return 0;
}
static int reqbufs(struct file *f, void *p, struct v4l2_requestbuffers *r) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, r->type);
  int ret = q ? vb2_reqbufs(q, r) : -EINVAL;
  if (!ret) {
    u32 *id = V4L2_TYPE_IS_OUTPUT(r->type) ? &s->output_pool : &s->capture_pool;
    if (++*id == 0) ++*id;
  }
  return ret;
}
static int querybuf(struct file *f, void *p, struct v4l2_buffer *b) {
  struct vb2_queue *q = queue_for(session(p), b->type);
  int r = q ? vb2_querybuf(q, b) : -EINVAL;
  if (!r && !V4L2_TYPE_IS_OUTPUT(b->type) && b->memory == V4L2_MEMORY_MMAP)
    for (unsigned i = 0; i < b->length; i++)
      b->m.planes[i].m.mem_offset += RB_CAPTURE_OFFSET;
  return r;
}
static int qbuf(struct file *f, void *p, struct v4l2_buffer *b) {
  struct rb_session *s = session(p);
  struct vb2_queue *q = queue_for(s, b->type);
  if (s->failed || s->stopping)
    return -EIO;
  if (V4L2_TYPE_IS_OUTPUT(b->type) && s->draining)
    return -EPIPE;
  return q ? vb2_qbuf(q, NULL, b) : -EINVAL;
}
static int dqbuf(struct file *f, void *p, struct v4l2_buffer *b) {
  struct vb2_queue *q = queue_for(session(p), b->type);
  return q ? vb2_dqbuf(q, b, f->f_flags & O_NONBLOCK) : -EINVAL;
}
static int expbuf(struct file *f, void *p, struct v4l2_exportbuffer *e) {
  struct vb2_queue *q = queue_for(session(p), e->type);
  return q ? vb2_expbuf(q, e) : -EINVAL;
}
static int streamon(struct file *f, void *p, enum v4l2_buf_type type) {
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
  int r = try_cmd(f, p, c);
  if (r)
    return r;
  if (c->cmd == V4L2_DEC_CMD_STOP) {
    if (!s->out_started || s->draining)
      return 0;
    s->draining = true;
    push(s, (struct rb_message){.type = RB_DRAIN});
  } else {
    s->capture.last_buffer_dequeued = false;
    s->draining = false;
    push(s, (struct rb_message){.type = RB_RESUME});
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
  unsigned i;
  if (e->index)
    return -EINVAL;
  for (i = 0; i < ARRAY_SIZE(codecs); i++)
    if (e->pixel_format == codecs[i])
      break;
  if (i == ARRAY_SIZE(codecs))
    return -EINVAL;
  e->type = V4L2_FRMSIZE_TYPE_STEPWISE;
  e->stepwise = (struct v4l2_frmsize_stepwise){64, 8192, 2, 64, 8192, 2};
  return 0;
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
  mutex_lock(&broker_lock);
  live_sessions++;
  mutex_unlock(&broker_lock);
  s = kzalloc(sizeof(*s), GFP_KERNEL);
  if (!s) {
    mutex_lock(&broker_lock);
    live_sessions--;
    mutex_unlock(&broker_lock);
    return -ENOMEM;
  }
  kref_init(&s->ref);
  mutex_init(&s->lock);
  init_waitqueue_head(&s->wait);
  INIT_LIST_HEAD(&s->pending);
  s->epoch = 1;
  s->codec = V4L2_PIX_FMT_H264;
  s->input_size = 1024 * 1024;
  s->input_format =
      (struct v4l2_pix_format_mplane){.width = 64,
                                      .height = 64,
                                      .pixelformat = V4L2_PIX_FMT_H264,
                                      .num_planes = 1,
                                      .field = V4L2_FIELD_NONE,
                                      .colorspace = V4L2_COLORSPACE_REC709};
  s->input_format.plane_fmt[0].sizeimage = s->input_size;
  s->layout = (struct rb_message){.width = 64,
                                  .height = 64,
                                  .stride = 64,
                                  .rows = 64,
                                  .size = 6144,
                                  .format = RB_FMT_NV12};
  v4l2_fh_init(&s->fh, video_devdata(f));
  f->private_data = &s->fh;
  v4l2_ctrl_handler_init(&s->ctrls, 1);
  v4l2_ctrl_new_std(&s->ctrls, NULL, V4L2_CID_MIN_BUFFERS_FOR_CAPTURE, 18, 18,
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
  mutex_lock(&broker_lock);
  list_add_tail(&s->pending, &pending_sessions);
  mutex_unlock(&broker_lock);
  wake_up_all(&broker_wait);
  return 0;
error:
  v4l2_ctrl_handler_free(&s->ctrls);
  v4l2_fh_exit(&s->fh);
  kref_put(&s->ref, destroy);
  return r;
}
static int video_release(struct file *f) {
  struct rb_session *s = session(f->private_data);
  mutex_lock(&broker_lock);
  list_del_init(&s->pending);
  mutex_unlock(&broker_lock);
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
static long video_ioctl(struct file *f, unsigned cmd, unsigned long arg) {
  struct rb_session *s = session(f->private_data);
  long r;
  if (mutex_lock_interruptible(&s->lock))
    return -ERESTARTSYS;
  r = video_ioctl2(f, cmd, arg);
  mutex_unlock(&s->lock);
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
  if (!s->failed && (s->out_started || s->cap_started))
    mask &= ~EPOLLERR;
  mutex_unlock(&s->lock);
  return mask;
}
static int video_mmap(struct file *f, struct vm_area_struct *vma) {
  struct rb_session *s = session(f->private_data);
  struct vb2_queue *q = &s->output;
  int r;
  if (vma->vm_pgoff >= (RB_CAPTURE_OFFSET >> PAGE_SHIFT)) {
    vma->vm_pgoff -= RB_CAPTURE_OFFSET >> PAGE_SHIFT;
    q = &s->capture;
  }
  mutex_lock(&s->lock);
  r = vb2_mmap(q, vma);
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
static int reply(struct rb_session *s, const struct rb_message *m) {
  struct vb2_buffer *b;
  struct v4l2_event ev = {0};
  unsigned i;
  if (m->version != RB_VERSION ||
      memchr_inv(m->reserved, 0, sizeof(m->reserved)))
    return -EINVAL;
  if (m->epoch != s->epoch)
    return -ESTALE;
  if (s->closed)
    return -EPIPE;
  switch (m->type) {
  case RB_LAYOUT:
    if (m->width < 64 || m->width > 8192 || m->height < 64 ||
        m->height > 8192 || (m->width & 1) || (m->height & 1) ||
        m->rows < m->height || m->rows > 16384 || (m->rows & 1) ||
        m->stride < m->width * (m->format == RB_FMT_P010 ? 2 : 1) ||
        m->stride > 65536 || m->size < (u64)m->stride * m->rows * 3 / 2 ||
        m->size > 128 * 1024 * 1024 ||
        (m->format != RB_FMT_NV12 && m->format != RB_FMT_P010))
      return -EINVAL;
    if (s->cap_started) {
      s->reconfig = true;
      s->eos = true;
      last_buffer(s);
    }
    s->layout = *m;
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
    b = s->output.bufs[m->index];
    if (b->state != VB2_BUF_STATE_ACTIVE)
      return -EINVAL;
    s->output_sent[m->index] = false;
    vb2_buffer_done(b, VB2_BUF_STATE_DONE);
    return 0;
  case RB_FRAME:
    if (m->pool_id != s->capture_pool || m->generation != s->generation || m->index >= s->capture.num_buffers ||
        !s->available[m->index] || !s->cap_started)
      return -ESTALE;
    b = s->capture.bufs[m->index];
    if (m->bytesused > s->layout.size)
      return -EINVAL;
    s->available[m->index] = false;
    if (b->num_planes == 2) {
      vb2_set_plane_payload(b, 0, s->layout.stride * s->layout.rows);
      vb2_set_plane_payload(b, 1, s->layout.stride * s->layout.rows / 2);
    } else
      vb2_set_plane_payload(b, 0, m->bytesused);
    b->timestamp = m->timestamp;
    to_vb2_v4l2_buffer(b)->sequence = s->sequence++;
    to_vb2_v4l2_buffer(b)->field = V4L2_FIELD_NONE;
    vb2_buffer_done(b, VB2_BUF_STATE_DONE);
    return 0;
  case RB_EOS:
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
  if (copy_from_user(&pool, arg, sizeof(pool))) return -EFAULT;
  if (pool.version != RB_VERSION || memchr_inv(pool.reserved, 0, sizeof(pool.reserved))) return -EINVAL;
  q = queue_for(s, pool.type);
  if (!q) return -EINVAL;
  if (s->closed || s->failed || s->stopping) return -EPIPE;
  if (!pool.pool_id || pool.epoch != s->epoch ||
      pool.pool_id != (V4L2_TYPE_IS_OUTPUT(pool.type) ? s->output_pool : s->capture_pool)) return -ESTALE;
  pool.count = q->num_buffers;
  if (!pool.count || pool.count > RB_MAX_BUFFERS) return -EINVAL;
  for (i = 0; i < RB_MAX_BUFFERS; i++) pool.buffers[i] = (struct rb_registered_buffer){.fd = -1};
  for (i = 0; i < pool.count; i++) {
    struct v4l2_exportbuffer e = {.type = pool.type, .index = i, .flags = O_RDWR | O_CLOEXEC};
    r = vb2_expbuf(q, &e);
    if (r) goto rollback;
    pool.buffers[i].fd = e.fd;
    pool.buffers[i].size = vb2_plane_size(q->bufs[i], 0);
    exported++;
  }
  if (!copy_to_user(arg, &pool, sizeof(pool))) return 0;
  r = -EFAULT;
rollback:
  for (i = 0; i < exported; i++) close_fd(pool.buffers[i].fd);
  return r;
}
static long exchange(struct rb_session *s, void __user *arg, struct rb_batch *batch) {
  unsigned i;
  if (batch->version != RB_VERSION || batch->reply_count > RB_BATCH_MAX || batch->reserved) return -EINVAL;
  /* Probe copyout before any mutation; a later concurrent unmap fails closed. */
  if (copy_to_user(arg, batch, sizeof(*batch))) return -EFAULT;
  memset(batch->results, 0, sizeof(batch->results));
  for (i = 0; i < batch->reply_count; i++) batch->results[i] = reply(s, &batch->replies[i]);
  batch->event_count = min_t(unsigned, s->head - s->tail, RB_BATCH_MAX);
  memset(batch->events, 0, sizeof(batch->events));
  for (i = 0; i < batch->event_count; i++) batch->events[i] = s->events[(s->tail + i) % RB_EVENTS];
  if (copy_to_user(arg, batch, sizeof(*batch))) {
    if (batch->reply_count) fail(s);
    return -EFAULT;
  }
  s->tail += batch->event_count;
  if (!batch->event_count && (s->closed || s->failed)) return -EPIPE;
  return 0;
}
static long service_ioctl(struct file *f, unsigned cmd, unsigned long arg) {
  struct rb_session *s = f->private_data;
  struct rb_batch *batch = NULL;
  long r;
  if (cmd == RB_EXCHANGE) {
    batch = memdup_user((void __user *)arg, sizeof(*batch));
    if (IS_ERR(batch)) return PTR_ERR(batch);
  } else if (cmd != RB_REGISTER_POOL) return -ENOTTY;
  if (mutex_lock_interruptible(&s->lock)) { kfree(batch); return -ERESTARTSYS; }
  r = batch ? exchange(s, (void __user *)arg, batch) : register_pool(s, (void __user *)arg);
  mutex_unlock(&s->lock);
  kfree(batch);
  return r;
}
static __poll_t service_poll(struct file *f, poll_table *wait) {
  struct rb_session *s = f->private_data;
  __poll_t mask = 0;
  poll_wait(f, &s->wait, wait);
  mutex_lock(&s->lock);
  if (s->head != s->tail)
    mask |= EPOLLIN;
  if (s->closed)
    mask |= EPOLLHUP;
  if (s->failed)
    mask |= EPOLLERR;
  mutex_unlock(&s->lock);
  return mask;
}
static int service_release(struct inode *inode, struct file *f) {
  struct rb_session *s = f->private_data;
  mutex_lock(&s->lock);
  s->attached = false;
  if (!s->closed)
    fail(s);
  mutex_unlock(&s->lock);
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
static long broker_ioctl(struct file *f, unsigned cmd, unsigned long arg) {
  struct rb_session *s;
  int fd;
  if (cmd != RB_ACCEPT)
    return -ENOTTY;
  for (;;) {
    mutex_lock(&broker_lock);
    if (!list_empty(&pending_sessions))
      break;
    mutex_unlock(&broker_lock);
    if (f->f_flags & O_NONBLOCK)
      return -EAGAIN;
    if (wait_event_interruptible(broker_wait,
                                 !list_empty_careful(&pending_sessions)))
      return -ERESTARTSYS;
  }
  s = list_first_entry(&pending_sessions, struct rb_session, pending);
  list_del_init(&s->pending);
  kref_get(&s->ref);
  mutex_unlock(&broker_lock);
  mutex_lock(&s->lock);
  s->attached = true;
  mutex_unlock(&s->lock);
  fd = anon_inode_getfd("v4l2-rkmpp-session", &service_fops, s, O_RDWR | O_CLOEXEC);
  if (fd < 0) {
    mutex_lock(&s->lock);
    fail(s);
    mutex_unlock(&s->lock);
    kref_put(&s->ref, destroy);
  }
  return fd;
}
static int broker_open(struct inode *inode, struct file *f) {
  int r = 0;
  mutex_lock(&broker_lock);
  if (broker_opened)
    r = -EBUSY;
  else
    broker_opened = true;
  mutex_unlock(&broker_lock);
  return r;
}
static int broker_release(struct inode *inode, struct file *f) {
  mutex_lock(&broker_lock);
  broker_opened = false;
  mutex_unlock(&broker_lock);
  return 0;
}
static __poll_t broker_poll(struct file *f, poll_table *wait) {
  __poll_t mask = 0;
  poll_wait(f, &broker_wait, wait);
  mutex_lock(&broker_lock);
  if (!list_empty(&pending_sessions))
    mask = EPOLLIN;
  mutex_unlock(&broker_lock);
  return mask;
}
const struct file_operations rb_broker_fops = {.owner = THIS_MODULE,
                                               .open = broker_open,
                                               .release = broker_release,
                                               .poll = broker_poll,
                                               .unlocked_ioctl = broker_ioctl,
                                               .compat_ioctl = broker_ioctl,
                                               .llseek = no_llseek};
