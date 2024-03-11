import { L1ToL2Message, NewInboxLeaf } from '@aztec/circuit-types';
import { L1_TO_L2_MSG_SUBTREE_HEIGHT } from '@aztec/circuits.js/constants';
import { Fr } from '@aztec/foundation/fields';

/**
 * A simple in-memory implementation of an L1 to L2 message store
 * that handles message duplication.
 * TODO(#4492): Clean this up
 */
export class NewL1ToL2MessageStore {
  /**
   * A map containing the entry key to the corresponding L1 to L2
   * messages (and the number of times the message has been seen).
   */
  protected store: Map<string, Buffer> = new Map();

  #l1ToL2MessagesSubtreeSize = 2 ** L1_TO_L2_MSG_SUBTREE_HEIGHT;

  constructor() {}

  addMessage(message: NewInboxLeaf) {
    if (message.index >= this.#l1ToL2MessagesSubtreeSize) {
      throw new Error(`Message index ${message.index} out of subtree range`);
    }
    const key = `${message.blockNumber}-${message.index}`;
    this.store.set(key, message.leaf);
  }

  getMessages(blockNumber: bigint): Buffer[] {
    const messages: Buffer[] = [];
    let undefinedMessageFound = false;
    for (let messageIndex = 0; messageIndex < this.#l1ToL2MessagesSubtreeSize; messageIndex++) {
      // This is inefficient but probably fine for now.
      const key = `${blockNumber}-${messageIndex}`;
      const message = this.store.get(key);
      if (message) {
        if (undefinedMessageFound) {
          throw new Error(`L1 to L2 message gap found in block ${blockNumber}`);
        }
        messages.push(message);
      } else {
        undefinedMessageFound = true;
        // We continue iterating over messages here to verify that there are no more messages after the undefined one.
        // --> If this was the case this would imply there is some issue with log fetching.
      }
    }
    return messages;
  }
}

/**
 * A simple in-memory implementation of an L1 to L2 message store
 * that handles message duplication.
 */
export class L1ToL2MessageStore {
  /**
   * A map containing the entry key to the corresponding L1 to L2
   * messages (and the number of times the message has been seen).
   */
  protected store: Map<bigint, L1ToL2MessageAndCount> = new Map();

  constructor() {}

  addMessage(entryKey: Fr, message: L1ToL2Message) {
    const entryKeyBigInt = entryKey.toBigInt();
    const msgAndCount = this.store.get(entryKeyBigInt);
    if (msgAndCount) {
      msgAndCount.count++;
    } else {
      this.store.set(entryKeyBigInt, { message, count: 1 });
    }
  }

  getMessage(entryKey: Fr): L1ToL2Message | undefined {
    return this.store.get(entryKey.value)?.message;
  }

  getMessageAndCount(entryKey: Fr): L1ToL2MessageAndCount | undefined {
    return this.store.get(entryKey.value);
  }
}

/**
 * Specifically for the store that will hold pending messages
 * for removing messages or fetching multiple messages.
 */
export class PendingL1ToL2MessageStore extends L1ToL2MessageStore {
  getEntryKeys(limit: number): Fr[] {
    if (limit < 1) {
      return [];
    }
    // fetch `limit` number of messages from the store with the highest fee.
    // Note the store has multiple of the same message. So if a message has count 2, include both of them in the result:
    const messages: Fr[] = [];
    const sortedMessages = Array.from(this.store.values()).sort((a, b) => b.message.fee - a.message.fee);
    for (const messageAndCount of sortedMessages) {
      for (let i = 0; i < messageAndCount.count; i++) {
        messages.push(messageAndCount.message.entryKey!);
        if (messages.length === limit) {
          return messages;
        }
      }
    }
    return messages;
  }

  removeMessage(entryKey: Fr) {
    // ignore 0 - entryKey is a hash, so a 0 can probabilistically never occur. It is best to skip it.
    if (entryKey.equals(Fr.ZERO)) {
      return;
    }

    const entryKeyBigInt = entryKey.value;
    const msgAndCount = this.store.get(entryKeyBigInt);
    if (!msgAndCount) {
      throw new Error(`Unable to remove message: L1 to L2 Message with key ${entryKeyBigInt} not found in store`);
    }
    if (msgAndCount.count > 1) {
      msgAndCount.count--;
    } else {
      this.store.delete(entryKeyBigInt);
    }
  }
}

/**
 * Useful to keep track of the number of times a message has been seen.
 */
type L1ToL2MessageAndCount = {
  /**
   * The message.
   */
  message: L1ToL2Message;
  /**
   * The number of times the message has been seen.
   */
  count: number;
};
