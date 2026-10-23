/* SPDX-License-Identifier: GPL-2.0 */
/* module: memory; responsibility: own DMA heap-backed queue storage.
 * operations: rb_heap_memops, rb_memory_export; entities: opaque rb_memory;
 * tests: tests/kernel/test_buffers.py and test_plane_storage.py. */
#ifndef RB_MEMORY_H
#define RB_MEMORY_H
#include <media/videobuf2-core.h>
extern const struct vb2_mem_ops rb_heap_memops;
/* Retain one file reference from a live queue buffer owned by rb_heap_memops.
 * The caller must transfer it to fd_install or release it with fput. */
struct file *rb_memory_export(struct vb2_buffer *buffer);
#endif
