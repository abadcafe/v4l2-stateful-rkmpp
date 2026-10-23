// SPDX-License-Identifier: GPL-2.0
/* Geometry negotiation contract is declared in format.h. */
#include "format.h"
#include <linux/slab.h>
#include <media/v4l2-common.h>
#ifndef V4L2_PIX_FMT_AV1
#define V4L2_PIX_FMT_AV1 v4l2_fourcc('A', 'V', '0', '1')
#endif
enum v4l2_field rb_format_field(u32 field) {
  switch (field) {
  case 1:
    return V4L2_FIELD_INTERLACED_TB;
  case 2:
    return V4L2_FIELD_INTERLACED_BT;
  case 3:
    return V4L2_FIELD_TOP;
  case 4:
    return V4L2_FIELD_BOTTOM;
  case 5:
    return V4L2_FIELD_INTERLACED;
  default:
    return V4L2_FIELD_NONE;
  }
}
struct rb_format {
  struct rb_message layout;
  struct v4l2_pix_format_mplane input;
};
static const u32 codecs[] = {V4L2_PIX_FMT_H264, V4L2_PIX_FMT_HEVC,
                             V4L2_PIX_FMT_VP8, V4L2_PIX_FMT_VP9,
                             V4L2_PIX_FMT_AV1};
void rb_format_capture(const struct rb_format *f,
                       struct v4l2_pix_format_mplane *p) {
  memset(p, 0, sizeof(*p));
  p->width = f->layout.stride / (f->layout.format == RB_FMT_P010 ? 2 : 1);
  p->height = f->layout.rows ?: 64;
  p->pixelformat =
      f->layout.format == RB_FMT_P010 ? V4L2_PIX_FMT_P010 : V4L2_PIX_FMT_NV12;
  p->num_planes = 1;
  p->field = f->layout.field == 3 || f->layout.field == 4
                 ? V4L2_FIELD_ALTERNATE
                 : rb_format_field(f->layout.field);
  p->plane_fmt[0].bytesperline = f->layout.stride ?: 64;
  p->plane_fmt[0].sizeimage = f->layout.size ?: 6144;
  p->colorspace = f->layout.colorspace ?: f->input.colorspace;
  p->xfer_func = f->layout.xfer_func ?: f->input.xfer_func;
  p->ycbcr_enc = f->layout.ycbcr_enc ?: f->input.ycbcr_enc;
  p->quantization = f->layout.quantization ?: f->input.quantization;
}
void rb_format_normalize(struct v4l2_pix_format_mplane *p) {
  unsigned i;
  for (i = 0; i < ARRAY_SIZE(codecs); i++)
    if (codecs[i] == (*p).pixelformat)
      break;
  if (i == ARRAY_SIZE(codecs))
    (*p).pixelformat = V4L2_PIX_FMT_H264;
  (*p).width = clamp_t(u32, (*p).width, 64, 8192);
  (*p).height = clamp_t(u32, (*p).height, 64, 8192);
  if (!(*p).colorspace || (*p).colorspace > V4L2_COLORSPACE_DCI_P3)
    (*p).colorspace = V4L2_COLORSPACE_REC709;
  (*p).num_planes = 1;
  (*p).field = V4L2_FIELD_NONE;
  (*p).plane_fmt[0].sizeimage =
      clamp_t(u32, (*p).plane_fmt[0].sizeimage, 1024 * 1024, 16 * 1024 * 1024);
}
int rb_format_sizes(struct v4l2_frmsizeenum *e) {
  unsigned i;
  if (e->index)
    return -EINVAL;
  for (i = 0; i < ARRAY_SIZE(codecs); i++)
    if (e->pixel_format == codecs[i])
      break;
  if (i == ARRAY_SIZE(codecs))
    return -EINVAL;
  e->type = V4L2_FRMSIZE_TYPE_STEPWISE;
  e->stepwise = (struct v4l2_frmsize_stepwise){64, 8192, 1, 64, 8192, 1};
  return 0;
}

struct rb_format *rb_format_new(void) {
  struct rb_format *f = kzalloc(sizeof(*f), GFP_KERNEL);
  struct v4l2_pix_format_mplane p = {
      .width = 64, .height = 64, .pixelformat = V4L2_PIX_FMT_H264};
  if (f) {
    rb_format_normalize(&p);
    rb_format_set(f, &p);
  }
  return f;
}
void rb_format_free(struct rb_format *f) { kfree(f); }
const struct rb_message *rb_format_layout(const struct rb_format *f) {
  return &f->layout;
}
const struct v4l2_pix_format_mplane *
rb_format_input(const struct rb_format *f) {
  return &f->input;
}
void rb_format_set(struct rb_format *f,
                   const struct v4l2_pix_format_mplane *p) {
  f->input = *p;
  f->layout = (struct rb_message){.width = p->width,
                                  .height = p->height,
                                  .coded_width = p->width,
                                  .coded_height = p->height,
                                  .stride = ALIGN(p->width, 64),
                                  .rows = ALIGN(p->height, 16),
                                  .format = RB_FMT_NV12,
                                  .count = RB_MIN_CAPTURE};
  f->layout.size = f->layout.stride * f->layout.rows * 3 / 2;
}
int rb_format_update(struct rb_format *f, const struct rb_message *m) {
  u64 right = (u64)m->crop_left + m->width;
  u64 bottom = (u64)m->crop_top + m->height;
  unsigned bytes = m->format == RB_FMT_P010 ? 2 : 1;
  if (m->count < RB_MIN_CAPTURE || m->count > RB_MAX_BUFFERS || m->field > 5 ||
      m->colorspace > V4L2_COLORSPACE_DCI_P3 ||
      m->xfer_func > V4L2_XFER_FUNC_SMPTE2084 ||
      m->ycbcr_enc > V4L2_YCBCR_ENC_SMPTE240M ||
      m->quantization > V4L2_QUANTIZATION_LIM_RANGE || !m->width ||
      right > 8192 || !m->height || bottom > 8192 || m->coded_width < 64 ||
      m->coded_width < right || m->coded_width > 8192 || m->coded_height < 64 ||
      m->coded_height < bottom || m->coded_height > 8192 ||
      m->rows < m->coded_height || m->rows > 16384 || (m->rows & 1) ||
      m->stride < ALIGN(m->coded_width, 2) * bytes || m->stride > 65536 ||
      m->size < (u64)m->stride * m->rows * 3 / 2 ||
      m->size > 128 * 1024 * 1024 ||
      (m->format != RB_FMT_NV12 && m->format != RB_FMT_P010))
    return -EINVAL;
  f->layout = *m;
  f->input.width = m->coded_width;
  f->input.height = m->coded_height;
  return 0;
}
int rb_format_enumerate(const struct rb_format *f, struct v4l2_fmtdesc *d) {
  if (V4L2_TYPE_IS_OUTPUT(d->type)) {
    if (d->index >= ARRAY_SIZE(codecs))
      return -EINVAL;
    d->pixelformat = codecs[d->index];
    d->flags = V4L2_FMT_FLAG_COMPRESSED | V4L2_FMT_FLAG_DYN_RESOLUTION;
    if (d->pixelformat == V4L2_PIX_FMT_AV1)
      strscpy(d->description, "AV1 OBU Stream", sizeof(d->description));
  } else {
    if (d->index)
      return -EINVAL;
    d->pixelformat =
        f->layout.format == RB_FMT_P010 ? V4L2_PIX_FMT_P010 : V4L2_PIX_FMT_NV12;
  }
  return 0;
}
int rb_format_selection(const struct rb_format *f, struct v4l2_selection *s) {
  const struct rb_message *m = &f->layout;
  if (s->type != V4L2_BUF_TYPE_VIDEO_CAPTURE &&
      s->type != V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE)
    return -EINVAL;
  s->r = (struct v4l2_rect){.left = m->crop_left,
                            .top = m->crop_top,
                            .width = m->width,
                            .height = m->height};
  switch (s->target) {
  case V4L2_SEL_TGT_CROP:
  case V4L2_SEL_TGT_CROP_DEFAULT:
  case V4L2_SEL_TGT_COMPOSE:
  case V4L2_SEL_TGT_COMPOSE_DEFAULT:
  case V4L2_SEL_TGT_COMPOSE_BOUNDS:
    break;
  case V4L2_SEL_TGT_CROP_BOUNDS:
    s->r =
        (struct v4l2_rect){.width = f->input.width, .height = f->input.height};
    break;
  case V4L2_SEL_TGT_COMPOSE_PADDED:
    s->r = (struct v4l2_rect){.width = m->stride /
                                       (m->format == RB_FMT_P010 ? 2 : 1),
                              .height = m->rows};
    break;
  default:
    return -EINVAL;
  }
  return 0;
}
