obj-m += v4l2_rkmpp_bridge.o
v4l2_rkmpp_bridge-y := device.o format.o session.o session/broker.o session/capture.o memory.o
KDIR ?= /usr/lib/modules/$(shell uname -r)/build
all:
	$(MAKE) -C $(KDIR) M=$(CURDIR) modules
clean:
	$(MAKE) -C $(KDIR) M=$(CURDIR) clean
