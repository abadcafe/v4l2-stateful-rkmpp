/* SPDX-License-Identifier: GPL-2.0 */
/* module: session/endpoint; responsibility: own one service ABI transaction.
 * operations: rb_endpoint_open; entities: opaque session and its consumer-owned
 * transition contract; tests: tests/kernel batch, pool, copyout and
 * reinitialize. */
#ifndef RB_SESSION_ENDPOINT_H
#define RB_SESSION_ENDPOINT_H
#include "../bridge_uapi.h"
#include <linux/poll.h>
struct rb_session;
struct rb_endpoint_ops {
  int (*begin)(struct rb_session *session, u64 incarnation);
  void (*end)(struct rb_session *session);
  int (*reply)(struct rb_session *session, const struct rb_message *message);
  unsigned (*events)(struct rb_session *session, struct rb_message *messages);
  int (*commit)(struct rb_session *session, unsigned count);
  void (*poison)(struct rb_session *session);
  /* Each populated file transfers a reference even on partial failure.
   * Export never installs descriptors in the caller's fd table. */
  int (*export)(struct rb_session *session, struct rb_pool *pool,
                struct file **files);
  __poll_t (*poll)(struct rb_session *session, u64 incarnation,
                   struct file *file, poll_table *wait);
  void (*release)(struct rb_session *session, u64 incarnation);
};
/* On success the endpoint owns the caller's session reference. On failure the
 * reference remains with the caller. begin/end serialize the complete ioctl. */
int rb_endpoint_open(struct rb_session *session, u64 incarnation,
                     const struct rb_endpoint_ops *ops);
#endif
