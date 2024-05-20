import {
  type Fr,
  MAX_NEW_NOTE_HASHES_PER_TX,
  MAX_NEW_NULLIFIERS_PER_TX,
  MAX_NOTE_ENCRYPTED_LOGS_PER_TX,
  MAX_NOTE_HASH_READ_REQUESTS_PER_TX,
  MAX_NULLIFIER_KEY_VALIDATION_REQUESTS_PER_TX,
  MAX_NULLIFIER_READ_REQUESTS_PER_TX,
  MembershipWitness,
  NULLIFIER_TREE_HEIGHT,
  NullifierKeyHint,
  PRIVATE_RESET_VARIANTS,
  type PrivateKernelData,
  PrivateKernelResetCircuitPrivateInputs,
  type PrivateKernelResetCircuitPrivateInputsVariants,
  PrivateKernelResetHints,
  type ScopedNullifier,
  type ScopedNullifierKeyValidationRequest,
  type ScopedReadRequest,
  buildNoteHashReadRequestHints,
  buildNullifierReadRequestHints,
  buildTransientDataHints,
} from '@aztec/circuits.js';
import { makeTuple } from '@aztec/foundation/array';
import { type Tuple } from '@aztec/foundation/serialize';

import { type ProvingDataOracle } from '../proving_data_oracle.js';
import { buildPrivateKernelResetOutputs } from './build_private_kernel_reset_outputs.js';

function getNullifierReadRequestHints<PENDING extends number, SETTLED extends number>(
  nullifierReadRequests: Tuple<ScopedReadRequest, typeof MAX_NULLIFIER_READ_REQUESTS_PER_TX>,
  nullifiers: Tuple<ScopedNullifier, typeof MAX_NEW_NULLIFIERS_PER_TX>,
  oracle: ProvingDataOracle,
  sizePending: PENDING,
  sizeSettled: SETTLED,
) {
  const getNullifierMembershipWitness = async (nullifier: Fr) => {
    const res = await oracle.getNullifierMembershipWitness(nullifier);
    if (!res) {
      throw new Error(`Cannot find the leaf for nullifier ${nullifier.toBigInt()}.`);
    }

    const { index, siblingPath, leafPreimage } = res;
    return {
      membershipWitness: new MembershipWitness(
        NULLIFIER_TREE_HEIGHT,
        index,
        siblingPath.toTuple<typeof NULLIFIER_TREE_HEIGHT>(),
      ),
      leafPreimage,
    };
  };

  return buildNullifierReadRequestHints(
    { getNullifierMembershipWitness },
    nullifierReadRequests,
    nullifiers,
    sizePending,
    sizeSettled,
  );
}

async function getMasterNullifierSecretKeys(
  nullifierKeyValidationRequests: Tuple<
    ScopedNullifierKeyValidationRequest,
    typeof MAX_NULLIFIER_KEY_VALIDATION_REQUESTS_PER_TX
  >,
  oracle: ProvingDataOracle,
) {
  const keys = makeTuple(MAX_NULLIFIER_KEY_VALIDATION_REQUESTS_PER_TX, NullifierKeyHint.empty);

  let keyIndex = 0;
  for (let i = 0; i < nullifierKeyValidationRequests.length; ++i) {
    const request = nullifierKeyValidationRequests[i].request;
    if (request.isEmpty()) {
      break;
    }
    keys[keyIndex] = new NullifierKeyHint(
      await oracle.getMasterNullifierSecretKey(request.masterNullifierPublicKey),
      i,
    );
    keyIndex++;
  }
  return {
    keysCount: keyIndex,
    keys,
  };
}

export async function buildPrivateKernelResetInputs(
  previousKernelData: PrivateKernelData,
  noteHashLeafIndexMap: Map<bigint, bigint>,
  oracle: ProvingDataOracle,
) {
  const publicInputs = previousKernelData.publicInputs;
  // Use max sizes, they will be trimmed down later.
  const {
    numPendingReadHints: noteHashPendingReadHints,
    numSettledReadHints: noteHashSettledReadHints,
    hints: noteHashReadRequestHints,
  } = await buildNoteHashReadRequestHints(
    oracle,
    publicInputs.validationRequests.noteHashReadRequests,
    publicInputs.end.newNoteHashes,
    noteHashLeafIndexMap,
    MAX_NOTE_HASH_READ_REQUESTS_PER_TX,
    MAX_NOTE_HASH_READ_REQUESTS_PER_TX,
  );

  const {
    numPendingReadHints: nullifierPendingReadHints,
    numSettledReadHints: nullifierSettledReadHints,
    hints: nullifierReadRequestHints,
  } = await getNullifierReadRequestHints(
    publicInputs.validationRequests.nullifierReadRequests,
    publicInputs.end.newNullifiers,
    oracle,
    MAX_NULLIFIER_READ_REQUESTS_PER_TX,
    MAX_NULLIFIER_READ_REQUESTS_PER_TX,
  );

  const { keysCount: nullifierKeysCount, keys: masterNullifierSecretKeys } = await getMasterNullifierSecretKeys(
    publicInputs.validationRequests.nullifierKeyValidationRequests,
    oracle,
  );

  const [
    transientNullifierIndexesForNoteHashes,
    transientNoteHashIndexesForNullifiers,
    transientNoteHashIndexesForLogs,
  ] = buildTransientDataHints(
    publicInputs.end.newNoteHashes,
    publicInputs.end.newNullifiers,
    publicInputs.end.noteEncryptedLogsHashes,
    MAX_NEW_NOTE_HASHES_PER_TX,
    MAX_NEW_NULLIFIERS_PER_TX,
    MAX_NOTE_ENCRYPTED_LOGS_PER_TX,
  );

  const expectedOutputs = buildPrivateKernelResetOutputs(
    previousKernelData.publicInputs.end.newNoteHashes,
    previousKernelData.publicInputs.end.newNullifiers,
    previousKernelData.publicInputs.end.noteEncryptedLogsHashes,
  );

  let privateInputs;

  for (const [sizeTag, hintSizes] of Object.entries(PRIVATE_RESET_VARIANTS)) {
    if (
      hintSizes.NOTE_HASH_PENDING_AMOUNT >= noteHashPendingReadHints &&
      hintSizes.NOTE_HASH_SETTLED_AMOUNT >= noteHashSettledReadHints &&
      hintSizes.NULLIFIER_PENDING_AMOUNT >= nullifierPendingReadHints &&
      hintSizes.NULLIFIER_SETTLED_AMOUNT >= nullifierSettledReadHints &&
      hintSizes.NULLIFIER_KEYS >= nullifierKeysCount
    ) {
      privateInputs = new PrivateKernelResetCircuitPrivateInputs(
        previousKernelData,
        expectedOutputs,
        new PrivateKernelResetHints(
          transientNullifierIndexesForNoteHashes,
          transientNoteHashIndexesForNullifiers,
          transientNoteHashIndexesForLogs,
          noteHashReadRequestHints,
          nullifierReadRequestHints,
          masterNullifierSecretKeys,
        ).trimToSizes(
          hintSizes.NOTE_HASH_PENDING_AMOUNT,
          hintSizes.NOTE_HASH_SETTLED_AMOUNT,
          hintSizes.NULLIFIER_PENDING_AMOUNT,
          hintSizes.NULLIFIER_SETTLED_AMOUNT,
          hintSizes.NULLIFIER_KEYS,
        ),
        sizeTag,
      );
      break;
    }
  }

  if (!privateInputs) {
    throw new Error('No private inputs found for the given hint sizes.');
  }

  return privateInputs as PrivateKernelResetCircuitPrivateInputsVariants;
}
