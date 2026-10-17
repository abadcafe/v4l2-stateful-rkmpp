// SPDX-License-Identifier: GPL-2.0
/* DMA heap lifetime is independent of a V4L2 queue or an exported fd. */
#include "memory.h"
#include <linux/dma-buf.h>
#include <linux/dma-heap.h>
#include <linux/file.h>
#include <linux/iosys-map.h>
#include <linux/slab.h>
#include <linux/videodev2.h>
struct rb_memory {
  struct dma_buf *buf;
  enum dma_data_direction direction;
  bool mapped, cpu_owned;
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
  m->buf = dma_heap_buffer_alloc(heap, size, O_RDWR | O_CLOEXEC, 0);
  if (IS_ERR(m->buf)) {
    void *err = m->buf;
    kfree(m);
    return err;
  }
  m->direction =
      V4L2_TYPE_IS_OUTPUT(vb->type) ? DMA_TO_DEVICE : DMA_FROM_DEVICE;
  return m;
}
static void rb_put(void *ptr) {
  struct rb_memory *m = ptr;
  if (m->cpu_owned)
    WARN_ON_ONCE(dma_buf_end_cpu_access(m->buf, m->direction));
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
  int r = dma_buf_mmap(m->buf, vma, 0);
  if (r)
    return r;
  if (!m->mapped) {
    r = dma_buf_begin_cpu_access(m->buf, m->direction);
    if (r)
      return r;
    m->mapped = m->cpu_owned = true;
  }
  return 0;
}
/* MMAP clients transfer CPU ownership at QBUF/DQBUF. Export-only clients
 * (Chromium) synchronize their DMA-BUF imports without these cache operations.
 */
static void rb_prepare(void *ptr) {
  struct rb_memory *m = ptr;
  if (m->cpu_owned) {
    WARN_ON_ONCE(dma_buf_end_cpu_access(m->buf, m->direction));
    m->cpu_owned = false;
  }
}
static void rb_finish(void *ptr) {
  struct rb_memory *m = ptr;
  if (m->mapped && !m->cpu_owned) {
    WARN_ON_ONCE(dma_buf_begin_cpu_access(m->buf, m->direction));
    m->cpu_owned = true;
  }
}
static unsigned rb_users(void *ptr) {
  struct rb_memory *m = ptr;
  /* dma_buf_mmap installs the exporter file in each VMA. Its reference
   * count tracks mappings (including fork), exports and the queue owner. */
  return file_count(m->buf->file);
}
const struct vb2_mem_ops rb_heap_memops = {.alloc = rb_alloc,
                                           .put = rb_put,
                                           .get_dmabuf = rb_export,
                                           .mmap = rb_mmap,
                                           .prepare = rb_prepare,
                                           .finish = rb_finish,
                                           .num_users = rb_users};
