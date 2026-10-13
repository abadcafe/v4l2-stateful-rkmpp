// SPDX-License-Identifier: GPL-2.0
/* DMA heap lifetime is independent of a V4L2 queue or an exported fd. */
#include "memory.h"
#include "plane_view.h"
#include <linux/dma-buf.h>
#include <linux/dma-heap.h>
#include <linux/iosys-map.h>
#include <linux/slab.h>
#include <linux/videodev2.h>
struct rb_memory {
  struct dma_buf *buf;
};
static void *rb_alloc(struct vb2_buffer *vb, struct device *dev,
                      unsigned long size) {
  struct dma_heap *heap;
  struct rb_memory *m;
  heap = dma_heap_find("system");
  if (!heap)
    return ERR_PTR(-ENODEV);
  m = kzalloc(sizeof(*m), GFP_KERNEL);
  if (!m) {
    return ERR_PTR(-ENOMEM);
  }
  if (vb->num_planes == 2 && vb->type == V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE) {
    if (vb->planes[0].mem_priv) {
      struct rb_memory *parent = vb->planes[0].mem_priv;
      m->buf = rb_plane_view(parent->buf, vb2_plane_size(vb, 0), size);
    } else
      m->buf = dma_heap_buffer_alloc(heap, size + PAGE_ALIGN(size / 2),
                                     O_RDWR | O_CLOEXEC, 0);
  } else
    m->buf = dma_heap_buffer_alloc(heap, size, O_RDWR | O_CLOEXEC, 0);
  if (IS_ERR(m->buf)) {
    void *err = m->buf;
    kfree(m);
    return err;
  }
  return m;
}
static void rb_put(void *ptr) {
  struct rb_memory *m = ptr;
  dma_buf_put(m->buf);
  kfree(m);
}
static struct dma_buf *rb_export(struct vb2_buffer *vb, void *ptr,
                                 unsigned long flags) {
  struct rb_memory *m = ptr;
  get_dma_buf(m->buf);
  return m->buf;
}
static int rb_mmap(void *ptr, struct vm_area_struct *vma) {
  struct rb_memory *m = ptr;
  return dma_buf_mmap(m->buf, vma, 0);
}
static unsigned rb_users(void *ptr) { return 1; }
const struct vb2_mem_ops rb_heap_memops = {.alloc = rb_alloc,
                                           .put = rb_put,
                                           .get_dmabuf = rb_export,
                                           .mmap = rb_mmap,
                                           .num_users = rb_users};
