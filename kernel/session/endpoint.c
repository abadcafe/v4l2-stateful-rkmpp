// SPDX-License-Identifier: GPL-2.0
/* module: session/endpoint; responsibility: transfer service ABI records and
 * descriptor ownership transactionally. operations: rb_endpoint_open;
 * entities: rb_endpoint; tests: tests/kernel batch, pool, copyout and
 * incarnation. No queue fields or video state representation are visible here.
 */
#include "endpoint.h"
#include <linux/anon_inodes.h>
#include <linux/fdtable.h>
#include <linux/file.h>
#include <linux/module.h>
#include <linux/pagemap.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
struct rb_endpoint {
  struct rb_session *session;
  const struct rb_endpoint_ops *ops;
  u64 incarnation;
  struct rb_batch batch;
};
static long register_pool(struct rb_endpoint *endpoint, void __user *arg) {
  struct rb_pool pool;
  struct file *files[RB_MAX_BUFFERS] = {0};
  unsigned i;
  int result;
  if (copy_from_user(&pool, arg, sizeof(pool)))
    return -EFAULT;
  if (pool.version != RB_VERSION ||
      memchr_inv(pool.reserved, 0, sizeof(pool.reserved)))
    return -EINVAL;
  for (i = 0; i < RB_MAX_BUFFERS; i++)
    pool.buffers[i] = (struct rb_registered_buffer){.fd = -1};
  result = endpoint->ops->export(endpoint->session, &pool, files);
  if (result)
    goto rollback;
  for (i = 0; i < pool.count; i++) {
    result = get_unused_fd_flags(O_CLOEXEC);
    if (result < 0)
      goto rollback;
    pool.buffers[i].fd = result;
  }
  if (copy_to_user(arg, &pool, sizeof(pool))) {
    result = -EFAULT;
    goto rollback;
  }
  /* Until copyout succeeds these slots have no file installed. A concurrent
   * close cannot acquire or recycle them, and rollback owns only reservations.
   */
  for (i = 0; i < pool.count; i++)
    fd_install(pool.buffers[i].fd, files[i]);
  return 0;
rollback:
  for (i = 0; i < RB_MAX_BUFFERS; i++)
    if (pool.buffers[i].fd >= 0)
      put_unused_fd(pool.buffers[i].fd);
  for (i = 0; i < RB_MAX_BUFFERS; i++)
    if (files[i])
      fput(files[i]);
  return result;
}
static long exchange(struct rb_endpoint *endpoint,
                     struct rb_batch __user *out) {
  struct rb_batch *batch = &endpoint->batch;
  unsigned i;
  if (copy_from_user(batch, out, offsetof(struct rb_batch, replies)))
    return -EFAULT;
  if (batch->version != RB_VERSION || batch->reply_count > RB_BATCH_MAX ||
      batch->event_count > RB_BATCH_MAX || batch->reserved)
    return -EINVAL;
  if (copy_from_user(batch->replies, out->replies,
                     batch->reply_count * sizeof(batch->replies[0])))
    return -EFAULT;
  if (fault_in_safe_writeable((void __user *)out, sizeof(*batch)))
    return -EFAULT;
  for (i = 0; i < batch->reply_count; i++)
    batch->results[i] =
        endpoint->ops->reply(endpoint->session, &batch->replies[i]);
  batch->event_count = endpoint->ops->events(endpoint->session, batch->events);
  if (copy_to_user(out, batch, offsetof(struct rb_batch, replies)) ||
      copy_to_user(out->events, batch->events,
                   batch->event_count * sizeof(batch->events[0])) ||
      copy_to_user(out->results, batch->results,
                   batch->reply_count * sizeof(batch->results[0]))) {
    if (batch->reply_count)
      endpoint->ops->poison(endpoint->session);
    return -EFAULT;
  }
  return endpoint->ops->commit(endpoint->session, batch->event_count);
}
static long service_ioctl(struct file *file, unsigned command,
                          unsigned long arg) {
  struct rb_endpoint *endpoint = file->private_data;
  int result;
  if (command != RB_EXCHANGE && command != RB_REGISTER_POOL)
    return -ENOTTY;
  result = endpoint->ops->begin(endpoint->session, endpoint->incarnation);
  if (result)
    return result;
  result = command == RB_EXCHANGE ? exchange(endpoint, (void __user *)arg)
                                  : register_pool(endpoint, (void __user *)arg);
  endpoint->ops->end(endpoint->session);
  return result;
}
static __poll_t service_poll(struct file *file, poll_table *wait) {
  struct rb_endpoint *endpoint = file->private_data;
  return endpoint->ops->poll(endpoint->session, endpoint->incarnation, file,
                             wait);
}
static int service_release(struct inode *inode, struct file *file) {
  struct rb_endpoint *endpoint = file->private_data;
  endpoint->ops->release(endpoint->session, endpoint->incarnation);
  kfree(endpoint);
  return 0;
}
static const struct file_operations service_fops = {
    .owner = THIS_MODULE,
    .unlocked_ioctl = service_ioctl,
    .compat_ioctl = service_ioctl,
    .poll = service_poll,
    .release = service_release,
    .llseek = no_llseek};
int rb_endpoint_open(struct rb_session *session, u64 incarnation,
                     const struct rb_endpoint_ops *ops) {
  struct rb_endpoint *endpoint = kmalloc(sizeof(*endpoint), GFP_KERNEL);
  int fd;
  if (!endpoint)
    return -ENOMEM;
  endpoint->session = session;
  endpoint->incarnation = incarnation;
  endpoint->ops = ops;
  fd = anon_inode_getfd("v4l2-rkmpp-session", &service_fops, endpoint,
                        O_RDWR | O_CLOEXEC);
  if (fd < 0)
    kfree(endpoint);
  return fd;
}
