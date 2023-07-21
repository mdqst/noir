/* Autogenerated file, do not edit! */

/* eslint-disable */
import {
  AztecAddress,
  Contract,
  ContractFunctionInteraction,
  ContractMethod,
  DeployMethod,
  Wallet,
} from '@aztec/aztec.js';
import { ContractAbi } from '@aztec/foundation/abi';
import { Fr, Point } from '@aztec/foundation/fields';
import { AztecRPC } from '@aztec/types';

import { PokeableTokenContractAbi } from '../artifacts/index.js';

/**
 * Type-safe interface for contract PokeableToken;
 */
export class PokeableTokenContract extends Contract {
  constructor(
    /** The deployed contract's address. */
    address: AztecAddress,
    /** The wallet. */
    wallet: Wallet,
  ) {
    super(address, PokeableTokenContractAbi, wallet);
  }

  /**
   * Creates a tx to deploy a new instance of this contract.
   */
  public static deploy(
    rpc: AztecRPC,
    initial_supply: Fr | bigint | number | { toField: () => Fr },
    sender: Fr | bigint | number | { toField: () => Fr },
    recipient: Fr | bigint | number | { toField: () => Fr },
    poker: Fr | bigint | number | { toField: () => Fr },
  ) {
    return new DeployMethod(Point.ZERO, rpc, PokeableTokenContractAbi, Array.from(arguments).slice(1));
  }

  /**
   * Creates a tx to deploy a new instance of this contract using the specified public key to derive the address.
   */
  public static deployWithPublicKey(
    rpc: AztecRPC,
    publicKey: Point,
    initial_supply: Fr | bigint | number | { toField: () => Fr },
    sender: Fr | bigint | number | { toField: () => Fr },
    recipient: Fr | bigint | number | { toField: () => Fr },
    poker: Fr | bigint | number | { toField: () => Fr },
  ) {
    return new DeployMethod(publicKey, rpc, PokeableTokenContractAbi, Array.from(arguments).slice(2));
  }

  /**
   * Returns this contract's ABI.
   */
  public static get abi(): ContractAbi {
    return PokeableTokenContractAbi;
  }

  /** Type-safe wrappers for the public methods exposed by the contract. */
  public methods!: {
    /** getBalance(sender: field) */
    getBalance: ((sender: Fr | bigint | number | { toField: () => Fr }) => ContractFunctionInteraction) &
      Pick<ContractMethod, 'selector'>;

    /** poke() */
    poke: (() => ContractFunctionInteraction) & Pick<ContractMethod, 'selector'>;

    /** stev(contract_address: field, nonce: field, storage_slot: field, preimage: array) */
    stev: ((
      contract_address: Fr | bigint | number | { toField: () => Fr },
      nonce: Fr | bigint | number | { toField: () => Fr },
      storage_slot: Fr | bigint | number | { toField: () => Fr },
      preimage: (Fr | bigint | number | { toField: () => Fr })[],
    ) => ContractFunctionInteraction) &
      Pick<ContractMethod, 'selector'>;
  };
}
