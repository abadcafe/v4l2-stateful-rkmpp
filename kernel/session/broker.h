/* SPDX-License-Identifier: GPL-2.0 */
#ifndef RB_BROKER_H
#define RB_BROKER_H
#include <linux/fs.h>
/* module: session/broker; responsibility: offer video sessions to one service.
 * operations: publish, reoffer, withdraw, destroy, file operations;
 * entity: rb_offer;
 * tests: tests/kernel/test_sessions.py, test_lifecycle.py.
 * The callback runs while the offer is protected against withdrawal.
 * Accept consumes the offer even on failure. The callback must either attach
 * an endpoint or leave the client in a terminal error state; returning an
 * allocation error must never leave a live, undiscoverable session. */
struct rb_offer;
struct rb_session;
struct rb_offer *rb_broker_publish(struct rb_session *owner,
                                   int (*attach)(struct rb_session *owner));
void rb_broker_reoffer(struct rb_offer *offer);
void rb_broker_withdraw(struct rb_offer *offer);
void rb_broker_destroy(struct rb_offer *offer);
extern const struct file_operations rb_broker_fops;
#endif
