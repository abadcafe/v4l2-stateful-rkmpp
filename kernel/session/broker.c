// SPDX-License-Identifier: GPL-2.0
/* Session discovery and exclusivity contract is declared in broker.h. */
#include "broker.h"
#include "../bridge_uapi.h"
#include <linux/atomic.h>
#include <linux/module.h>
#include <linux/poll.h>
#include <linux/slab.h>
struct rb_offer {
  struct list_head pending;
  struct rb_session *owner;
  int (*attach)(struct rb_session *);
};
static LIST_HEAD(pending_sessions);
static atomic_t pending_count = ATOMIC_INIT(0);
static DEFINE_MUTEX(broker_lock);
static DECLARE_WAIT_QUEUE_HEAD(broker_wait);
static bool broker_opened;
struct rb_offer *rb_broker_publish(struct rb_session *owner,
                                   int (*attach)(struct rb_session *)) {
  struct rb_offer *offer = kmalloc(sizeof(*offer), GFP_KERNEL);
  if (!offer)
    return ERR_PTR(-ENOMEM);
  offer->owner = owner;
  offer->attach = attach;
  mutex_lock(&broker_lock);
  list_add_tail(&offer->pending, &pending_sessions);
  atomic_inc(&pending_count);
  mutex_unlock(&broker_lock);
  wake_up_all(&broker_wait);
  return offer;
}
void rb_broker_reoffer(struct rb_offer *offer) {
  mutex_lock(&broker_lock);
  if (!list_empty(&offer->pending))
    atomic_dec(&pending_count);
  list_del_init(&offer->pending);
  list_add_tail(&offer->pending, &pending_sessions);
  atomic_inc(&pending_count);
  mutex_unlock(&broker_lock);
  wake_up_all(&broker_wait);
}
void rb_broker_withdraw(struct rb_offer *offer) {
  if (!offer)
    return;
  mutex_lock(&broker_lock);
  if (!list_empty(&offer->pending))
    atomic_dec(&pending_count);
  list_del_init(&offer->pending);
  mutex_unlock(&broker_lock);
}
void rb_broker_destroy(struct rb_offer *offer) {
  rb_broker_withdraw(offer);
  kfree(offer);
}
static long broker_ioctl(struct file *f, unsigned cmd, unsigned long arg) {
  struct rb_offer *offer;
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
    if (wait_event_interruptible(broker_wait, atomic_read(&pending_count) != 0))
      return -ERESTARTSYS;
  }
  offer = list_first_entry(&pending_sessions, struct rb_offer, pending);
  list_del_init(&offer->pending);
  atomic_dec(&pending_count);
  fd = offer->attach(offer->owner);
  mutex_unlock(&broker_lock);
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
  /* poll must not sleep behind ACCEPT while it acquires a session lock. */
  if (atomic_read(&pending_count))
    mask = EPOLLIN;
  return mask;
}
const struct file_operations rb_broker_fops = {.owner = THIS_MODULE,
                                               .open = broker_open,
                                               .release = broker_release,
                                               .poll = broker_poll,
                                               .unlocked_ioctl = broker_ioctl,
                                               .compat_ioctl = broker_ioctl,
                                               .llseek = no_llseek};
