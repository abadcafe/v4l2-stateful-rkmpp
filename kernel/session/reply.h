/* SPDX-License-Identifier: GPL-2.0 */
/* module: session/reply; responsibility: admit service reply envelopes.
 * operations: rb_reply_validate; entities: none;
 * tests: tests/kernel/test_reply_validation.py and test_batch.py.
 * This contract has no session-state or queue dependencies. */
#ifndef RB_SESSION_REPLY_H
#define RB_SESSION_REPLY_H
#include "../bridge_uapi.h"
int rb_reply_validate(const struct rb_message *message);
#endif
