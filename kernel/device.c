// SPDX-License-Identifier: GPL-2.0
/* Device registration; session ownership is hidden in session.c. */
#include "session.h"
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <media/v4l2-device.h>
#include <media/v4l2-ioctl.h>
static char *control_name = "v4l2-rkmpp-control";
module_param(control_name, charp, 0444);
static struct video_device *video;
static struct v4l2_device v4l2;
static struct miscdevice broker = {.minor = MISC_DYNAMIC_MINOR,
                                   .name = "v4l2-rkmpp-control",
                                   .fops = &rb_broker_fops,
                                   .mode = 0600};
static int __init bridge_init(void) {
  int r;
  strscpy(v4l2.name, "v4l2_rkmpp_bridge", sizeof(v4l2.name));
  r = v4l2_device_register(NULL, &v4l2);
  if (r)
    return r;
  video = video_device_alloc();
  if (!video) {
    r = -ENOMEM;
    goto unreg;
  }
  strscpy(video->name, "v4l2_rkmpp_bridge", sizeof(video->name));
  video->v4l2_dev = &v4l2;
  video->fops = &rb_video_fops;
  video->ioctl_ops = &rb_video_ioctls;
  video->release = video_device_release;
  video->vfl_dir = VFL_DIR_M2M;
  video->device_caps = V4L2_CAP_VIDEO_M2M_MPLANE | V4L2_CAP_STREAMING;
  r = video_register_device(video, VFL_TYPE_VIDEO, -1);
  if (r) {
    video_device_release(video);
    goto unreg;
  }
  broker.name = control_name;
  r = misc_register(&broker);
  if (r) {
    video_unregister_device(video);
    goto unreg;
  }
  pr_info("v4l2_rkmpp_bridge: /dev/video%d registered\n", video->num);
  return 0;
unreg:
  v4l2_device_unregister(&v4l2);
  return r;
}
static void __exit bridge_exit(void) {
  misc_deregister(&broker);
  video_unregister_device(video);
  v4l2_device_unregister(&v4l2);
}
module_init(bridge_init);
module_exit(bridge_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Rockchip MPP userspace stateful V4L2 bridge");
MODULE_IMPORT_NS(DMA_BUF);
