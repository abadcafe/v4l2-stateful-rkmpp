// SPDX-License-Identifier: GPL-2.0
/* Export a plane of a DMA-BUF without copying its pixels. */
#include "plane_view.h"
#include <linux/dma-buf.h>
#include <linux/dma-mapping.h>
#include <linux/iosys-map.h>
#include <linux/slab.h>
struct view {
  struct dma_buf *parent;
  size_t offset, size;
};
struct mapping {
  struct sg_table table;
  struct sg_table *parent_table;
};
static int attach(struct dma_buf *buf, struct dma_buf_attachment *a) {
  struct view *v = buf->priv;
  a->priv = dma_buf_attach(v->parent, a->dev);
  return PTR_ERR_OR_ZERO(a->priv);
}
static void detach(struct dma_buf *buf, struct dma_buf_attachment *a) {
  struct view *v = buf->priv;
  dma_buf_detach(v->parent, a->priv);
}
static struct sg_table *map(struct dma_buf_attachment *a,
                            enum dma_data_direction dir) {
  struct view *v = a->dmabuf->priv;
  struct mapping *m;
  struct scatterlist *sg, *out;
  size_t skip = v->offset, left = v->size;
  unsigned i, n = 0;
  int r;
  m = kzalloc(sizeof(*m), GFP_KERNEL);
  if (!m)
    return ERR_PTR(-ENOMEM);
  m->parent_table = dma_buf_map_attachment(a->priv, dir);
  if (IS_ERR(m->parent_table)) {
    void *e = m->parent_table;
    kfree(m);
    return e;
  }
  for_each_sg(m->parent_table->sgl, sg, m->parent_table->orig_nents, i) {
    size_t take;
    if (skip >= sg->length) {
      skip -= sg->length;
      continue;
    }
    take = min_t(size_t, left, sg->length - skip);
    if (take)
      n++;
    left -= take;
    skip = 0;
    if (!left)
      break;
  }
  if (left || !n) {
    r = -EINVAL;
    goto parent;
  }
  r = sg_alloc_table(&m->table, n, GFP_KERNEL);
  if (r)
    goto parent;
  skip = v->offset;
  left = v->size;
  out = m->table.sgl;
  for_each_sg(m->parent_table->sgl, sg, m->parent_table->orig_nents, i) {
    size_t take, off;
    if (skip >= sg->length) {
      skip -= sg->length;
      continue;
    }
    take = min_t(size_t, left, sg->length - skip);
    off = sg->offset + skip;
    sg_set_page(out, nth_page(sg_page(sg), off >> PAGE_SHIFT), take,
                offset_in_page(off));
    left -= take;
    skip = 0;
    if (!left)
      break;
    out = sg_next(out);
  }
  r = dma_map_sgtable(a->dev, &m->table, dir, 0);
  if (r)
    goto table;
  return &m->table;
table:
  sg_free_table(&m->table);
parent:
  dma_buf_unmap_attachment(a->priv, m->parent_table, dir);
  kfree(m);
  return ERR_PTR(r);
}
static void unmap(struct dma_buf_attachment *a, struct sg_table *table,
                  enum dma_data_direction dir) {
  struct mapping *m = container_of(table, struct mapping, table);
  dma_unmap_sgtable(a->dev, table, dir, 0);
  sg_free_table(table);
  dma_buf_unmap_attachment(a->priv, m->parent_table, dir);
  kfree(m);
}
static int mmap_view(struct dma_buf *buf, struct vm_area_struct *vma) {
  struct view *v = buf->priv;
  return dma_buf_mmap(v->parent, vma,
                      (v->offset >> PAGE_SHIFT) + vma->vm_pgoff);
}
static int vmap_view(struct dma_buf *buf, struct iosys_map *map) {
  struct view *v = buf->priv;
  int r = dma_buf_vmap(v->parent, map);
  if (!r)
    iosys_map_incr(map, v->offset);
  return r;
}
static void vunmap_view(struct dma_buf *buf, struct iosys_map *map) {
  struct view *v = buf->priv;
  struct iosys_map base = *map;
  iosys_map_incr(&base, -v->offset);
  dma_buf_vunmap(v->parent, &base);
}
static int begin(struct dma_buf *buf, enum dma_data_direction dir) {
  struct view *v = buf->priv;
  return dma_buf_begin_cpu_access(v->parent, dir);
}
static int end(struct dma_buf *buf, enum dma_data_direction dir) {
  struct view *v = buf->priv;
  return dma_buf_end_cpu_access(v->parent, dir);
}
static void release(struct dma_buf *buf) {
  struct view *v = buf->priv;
  dma_buf_put(v->parent);
  kfree(v);
}
static const struct dma_buf_ops ops = {.attach = attach,
                                       .detach = detach,
                                       .map_dma_buf = map,
                                       .unmap_dma_buf = unmap,
                                       .mmap = mmap_view,
                                       .vmap = vmap_view,
                                       .vunmap = vunmap_view,
                                       .begin_cpu_access = begin,
                                       .end_cpu_access = end,
                                       .release = release};
struct dma_buf *rb_plane_view(struct dma_buf *parent, size_t offset,
                              size_t size) {
  DEFINE_DMA_BUF_EXPORT_INFO(info);
  struct view *v;
  struct dma_buf *buf;
  if (!size || !IS_ALIGNED(offset, PAGE_SIZE) || offset > parent->size ||
      size > parent->size - offset)
    return ERR_PTR(-EINVAL);
  v = kmalloc(sizeof(*v), GFP_KERNEL);
  if (!v)
    return ERR_PTR(-ENOMEM);
  get_dma_buf(parent);
  *v = (struct view){parent, offset, size};
  info.ops = &ops;
  info.size = size;
  info.flags = O_RDWR;
  info.priv = v;
  info.resv = parent->resv;
  buf = dma_buf_export(&info);
  if (IS_ERR(buf)) {
    dma_buf_put(parent);
    kfree(v);
  }
  return buf;
}
