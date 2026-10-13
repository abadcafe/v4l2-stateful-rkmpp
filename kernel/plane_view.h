/* SPDX-License-Identifier: GPL-2.0 */
#ifndef RB_PLANE_VIEW_H
#define RB_PLANE_VIEW_H
#include <linux/dma-buf.h>
/* Page-aligned view, sharing storage and reservation fences with its parent. */
struct dma_buf *rb_plane_view(struct dma_buf *parent, size_t offset,
                              size_t size);
#endif
